use image_hasher::{HashAlg, HasherConfig};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::ipc::Channel;
use tauri::State;

use crate::index::discovery;
use crate::index::metadata;
use crate::index::store::{
    AnalysisResults, BlurResult, ExposureResult, ImageIndex, IndexedImage,
};
use crate::pipeline::registry::ProgressEvent;
use crate::state::AppState;
use crate::thumbnail;

fn progress(
    phase: &str,
    current: usize,
    total: usize,
    start: &Instant,
    current_file: Option<&str>,
    step_timings: Option<&HashMap<String, u64>>,
) -> ProgressEvent {
    ProgressEvent {
        phase: phase.to_string(),
        current,
        total,
        elapsed_ms: start.elapsed().as_millis() as u64,
        current_file: current_file.map(|s| s.to_string()),
        step_timings: step_timings.cloned(),
    }
}

/// Per-image result from the single-pass pipeline.
pub struct SinglePassResult {
    pub image: IndexedImage,
    pub analysis: AnalysisResults,
    pub phash: Option<Vec<u8>>,
    /// Per-face embeddings for person clustering (index-aligned with analysis.faces)
    pub face_embeddings: Vec<Vec<f32>>,
}

/// Process a single image through the full analysis pipeline.
/// This is the core per-image work: EXIF → decode → resize → thumbnail →
/// blur → exposure → phash → closed eyes → subject focus → (optional) face embeddings.
///
/// When `face_grouping` is false, face crops/thumbnails and embeddings are skipped,
/// but closed eye detection and subject focus still run (they only need face bounding boxes).
pub fn process_single_image(
    disc: &crate::index::discovery::DiscoveredImage,
    thumb_dir: &std::path::Path,
    hasher: &image_hasher::Hasher,
    face_grouping: bool,
) -> SinglePassResult {
    let path_str = disc.path.to_string_lossy().to_string();
    let file_name = disc.path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let id = thumbnail::cache_key(&path_str, disc.modified_at);

    let exif = metadata::extract_metadata(&disc.path);
    let processed = thumbnail::process_image(&disc.path, thumb_dir, &id);

    let (analysis, phash, face_embs) = if let Some(ref analysis_img) = processed.analysis_image {
        let gray = analysis_img.to_luma8();
        let blur = compute_blur(&gray);
        let exposure = compute_exposure(&gray);
        let hash = hasher.hash_image(analysis_img);
        let phash_bytes = hash.as_bytes().to_vec();

        let ext = disc.path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        let jpeg_data = if ext == "jpg" || ext == "jpeg" {
            std::fs::read(&disc.path).ok()
        } else {
            None
        };
        let closed_eyes = crate::pipeline::closed_eyes::detect(
            jpeg_data.as_deref(),
            analysis_img,
        );

        let subject_focus = compute_subject_focus(&gray, closed_eyes.as_ref());

        let face_boxes: Vec<[f64; 4]> = closed_eyes.as_ref()
            .map(|ce| ce.faces.iter().filter_map(|f| f.bounding_box).collect())
            .unwrap_or_default();
        let (face_infos, face_embs) = if face_grouping && !face_boxes.is_empty() {
            crate::pipeline::face_grouping::extract_faces(
                analysis_img, &face_boxes, thumb_dir, &id,
            )
        } else {
            (vec![], vec![])
        };

        (
            AnalysisResults {
                blur: Some(blur),
                exposure: Some(exposure),
                duplicate_group_id: None,
                scene_group_id: None,
                closed_eyes,
                subject_focus,
                faces: if face_infos.is_empty() { None } else { Some(face_infos) },
            },
            Some(phash_bytes),
            face_embs,
        )
    } else {
        (AnalysisResults::default(), None, vec![])
    };

    SinglePassResult {
        image: IndexedImage {
            id,
            path: path_str,
            file_name,
            file_size: disc.file_size,
            modified_at: disc.modified_at,
            width: processed.width,
            height: processed.height,
            thumbnail_path: processed.thumbnail_path,
            exif,
        },
        analysis,
        phash,
        face_embeddings: face_embs,
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub images: Vec<IndexedImage>,
    pub analysis: HashMap<String, AnalysisResults>,
    pub duplicate_groups: HashMap<String, Vec<String>>,
    pub scene_groups: HashMap<String, Vec<String>>,
    pub person_groups: HashMap<String, Vec<crate::commands::analyze::PersonGroupEntry>>,
}

#[tauri::command]
pub async fn scan_folder(
    path: String,
    on_progress: Channel<ProgressEvent>,
    state: State<'_, Arc<AppState>>,
) -> Result<ScanResult, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err("Invalid directory path".into());
    }

    let global_start = Instant::now();
    let mut step_timings: HashMap<String, u64> = HashMap::new();

    // Phase 1: Discover images (directory walk only — very fast)
    on_progress
        .send(progress("Discovering images...", 0, 0, &global_start, None, None))
        .ok();

    let phase_start = Instant::now();
    let discovered = discovery::discover_images(&root);
    let total = discovered.len();
    let discovery_ms = phase_start.elapsed().as_millis() as u64;
    step_timings.insert("discovery".into(), discovery_ms);

    on_progress
        .send(progress(
            &format!("Found {} images ({}ms). Processing...", total, discovery_ms),
            0,
            total,
            &global_start,
            None,
            Some(&step_timings),
        ))
        .ok();

    // Phase 2: SINGLE PASS — for each image:
    //   1. Read EXIF (header only, fast)
    //   2. Decode image pixels (the expensive part)
    //   3. SIMD resize to 1024px ASAP (drops full-res, frees ~100MB)
    //   4. Generate thumbnail from already-small image
    //   5. Run blur detection on the resized image
    //   6. Run exposure analysis on the resized image
    //   7. Compute perceptual hash for duplicate detection
    //   8. Drop the analysis image — constant memory regardless of photo count
    //
    // With DCT-scaled JPEG decoding (turbojpeg), each thread only needs ~5MB
    // peak memory instead of ~200MB, so we can safely use more threads.
    // Benchmarking shows 8 threads is optimal (50 imgs/sec vs 37 at 4 threads).
    let thumb_dir = state.thumbnail_dir.clone();
    let counter = Arc::new(AtomicUsize::new(0));
    let phase_start = Instant::now();

    // Perceptual hasher — thread-safe, shared across rayon workers
    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::DoubleGradient)
        .hash_size(16, 16)
        .to_hasher();

    // Pre-warm Vision models to avoid ~500ms-2s cold-start on first images.
    // Run both warmups concurrently via rayon since they load different models.
    let face_grouping = true;
    {
        let warmup_start = Instant::now();
        rayon::join(
            || crate::pipeline::closed_eyes::warmup_face_detection_model(),
            || {
                if face_grouping {
                    crate::pipeline::face_grouping::warmup_feature_print_model();
                }
            },
        );
        tracing::info!("Vision model warmup: {}ms", warmup_start.elapsed().as_millis());
    }

    let num_threads = std::cmp::min(8, rayon::current_num_threads());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

    let results: Vec<SinglePassResult> = pool.install(|| {
    discovered
        .par_iter()
        .map(|disc| {
            let result = process_single_image(disc, &thumb_dir, &hasher, face_grouping);

            let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if done % 5 == 0 || done == total {
                on_progress
                    .send(ProgressEvent {
                        phase: format!("Processing ({} threads)...", num_threads),
                        current: done,
                        total,
                        elapsed_ms: global_start.elapsed().as_millis() as u64,
                        current_file: Some(result.image.file_name.clone()),
                        step_timings: None,
                    })
                    .ok();
            }

            result
        })
        .collect()
    }); // end pool.install

    let process_ms = phase_start.elapsed().as_millis() as u64;
    step_timings.insert("process".into(), process_ms);
    tracing::info!("Pipeline processing: {}ms for {} images ({:.1} imgs/sec)",
        process_ms, total, total as f64 / (process_ms as f64 / 1000.0));

    // Phase 3: Duplicate grouping — operates on hashes + timestamps only, no images
    on_progress
        .send(progress(
            "Grouping duplicates...",
            total,
            total,
            &global_start,
            None,
            Some(&step_timings),
        ))
        .ok();

    let dup_start = Instant::now();

    // Collect data needed for duplicate grouping
    let dup_entries: Vec<DupEntry> = results
        .iter()
        .enumerate()
        .map(|(i, r)| DupEntry {
            index: i,
            timestamp: r
                .image
                .exif
                .as_ref()
                .and_then(|e| e.date_time_original.as_ref())
                .and_then(|d| parse_exif_date(d))
                .unwrap_or(r.image.modified_at as i64),
            phash: r.phash.clone(),
        })
        .collect();

    let duplicate_groups = find_duplicate_groups(&dup_entries, 5, 10);
    let scene_groups = find_scene_groups(&dup_entries, 60);

    // Face clustering: collect all face embeddings and cluster by person
    let face_entries: Vec<(String, u32, Vec<f32>)> = results
        .iter()
        .flat_map(|r| {
            r.face_embeddings
                .iter()
                .enumerate()
                .map(|(fi, emb)| (r.image.id.clone(), fi as u32, emb.clone()))
        })
        .collect();
    let person_groups = crate::pipeline::face_grouping::cluster_faces(&face_entries, 0.65);

    // Cache grouping inputs for later regrouping without re-scan
    {
        let gdata: Vec<crate::state::GroupingData> = results
            .iter()
            .enumerate()
            .map(|(i, r)| crate::state::GroupingData {
                image_id: r.image.id.clone(),
                timestamp: dup_entries[i].timestamp,
                phash: r.phash.clone(),
                face_embeddings: r.face_embeddings.clone(),
            })
            .collect();
        *state.grouping_data.write().map_err(|e| e.to_string())? = gdata;
    }

    // Build a lookup: (image_id, face_index) -> person_id
    let mut face_person_map: HashMap<(String, u32), String> = HashMap::new();
    for (pid, members) in &person_groups {
        for (img_id, fi) in members {
            face_person_map.insert((img_id.clone(), *fi), pid.clone());
        }
    }

    let dup_ms = dup_start.elapsed().as_millis() as u64;
    step_timings.insert("grouping".into(), dup_ms);
    tracing::info!("Grouping + caching: {}ms", dup_ms);

    let build_start = Instant::now();
    // Build final data structures
    let mut images = Vec::with_capacity(results.len());
    let mut analysis_map = HashMap::with_capacity(results.len());
    let mut dup_group_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut scene_group_map: HashMap<String, Vec<String>> = HashMap::new();

    for (i, mut r) in results.into_iter().enumerate() {
        if let Some(group_id) = duplicate_groups.get(&i) {
            r.analysis.duplicate_group_id = Some(group_id.clone());
            dup_group_map
                .entry(group_id.clone())
                .or_default()
                .push(r.image.id.clone());
        }
        if let Some(scene_id) = scene_groups.get(&i) {
            r.analysis.scene_group_id = Some(scene_id.clone());
            scene_group_map
                .entry(scene_id.clone())
                .or_default()
                .push(r.image.id.clone());
        }
        // Assign person IDs to detected faces
        if let Some(ref mut faces) = r.analysis.faces {
            for face in faces.iter_mut() {
                if let Some(pid) = face_person_map.get(&(r.image.id.clone(), face.face_index)) {
                    face.person_id = Some(pid.clone());
                }
            }
        }
        analysis_map.insert(r.image.id.clone(), r.analysis);
        images.push(r.image);
    }

    tracing::info!("Build data structures: {}ms", build_start.elapsed().as_millis());

    // Store clones in state, keep originals for the IPC response
    let store_start = Instant::now();
    let index = ImageIndex::new(root, images.clone());
    *state.index.write().map_err(|e| e.to_string())? = Some(index);
    *state.analysis.write().map_err(|e| e.to_string())? =
        Some(crate::index::store::AnalysisIndex {
            results: analysis_map.clone(),
            duplicate_groups: dup_group_map.clone(),
            scene_groups: scene_group_map.clone(),
            person_groups: person_groups.clone(),
        });

    tracing::info!("Store in state: {}ms", store_start.elapsed().as_millis());
    tracing::info!("TOTAL scan_folder wall clock: {}ms", global_start.elapsed().as_millis());

    on_progress
        .send(progress(
            "Complete",
            total,
            total,
            &global_start,
            None,
            Some(&step_timings),
        ))
        .ok();

    // Build person group entries for the response
    let person_group_entries: HashMap<String, Vec<crate::commands::analyze::PersonGroupEntry>> =
        person_groups.iter().map(|(pid, members)| {
            (pid.clone(), members.iter().map(|(img_id, fi)| {
                crate::commands::analyze::PersonGroupEntry {
                    image_id: img_id.clone(),
                    face_index: *fi,
                }
            }).collect())
        }).collect();

    // Return everything in one shot — no additional IPC round trips needed
    Ok(ScanResult {
        images,
        analysis: analysis_map,
        duplicate_groups: dup_group_map,
        scene_groups: scene_group_map,
        person_groups: person_group_entries,
    })
}

// ── Inline analysis functions (no trait overhead, no allocation) ──

fn compute_blur(gray: &image::GrayImage) -> BlurResult {
    let (w, h) = (gray.width() as usize, gray.height() as usize);

    if w < 3 || h < 3 {
        return BlurResult {
            laplacian_variance: 0.0,
            is_blurry: true,
        };
    }

    let pixels = gray.as_raw();
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut count = 0u64;

    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let center = pixels[y * w + x] as f64;
            let top = pixels[(y - 1) * w + x] as f64;
            let bottom = pixels[(y + 1) * w + x] as f64;
            let left = pixels[y * w + (x - 1)] as f64;
            let right = pixels[y * w + (x + 1)] as f64;

            let laplacian = top + bottom + left + right - 4.0 * center;
            sum += laplacian;
            sum_sq += laplacian * laplacian;
            count += 1;
        }
    }

    let mean = sum / count as f64;
    let variance = (sum_sq / count as f64) - (mean * mean);
    let threshold = 100.0;

    BlurResult {
        laplacian_variance: variance,
        is_blurry: variance < threshold,
    }
}

fn compute_exposure(gray: &image::GrayImage) -> ExposureResult {
    let pixels = gray.as_raw();
    let total = pixels.len() as f64;

    if total == 0.0 {
        return ExposureResult {
            mean_luminance: 0.0,
            pct_underexposed: 0.0,
            pct_overexposed: 0.0,
            verdict: "Normal".into(),
        };
    }

    let mut histogram = [0u64; 256];
    let mut lum_sum = 0u64;
    for &p in pixels {
        histogram[p as usize] += 1;
        lum_sum += p as u64;
    }

    let mean_luminance = (lum_sum as f64 / total) / 255.0;
    let under_count: u64 = histogram[..25].iter().sum();
    let pct_underexposed = under_count as f64 / total;
    let over_count: u64 = histogram[230..].iter().sum();
    let pct_overexposed = over_count as f64 / total;

    let under_thresh = 0.30;
    let over_thresh = 0.30;

    let verdict = if pct_underexposed > under_thresh && pct_overexposed > over_thresh {
        "HighContrast"
    } else if pct_underexposed > under_thresh {
        "Underexposed"
    } else if pct_overexposed > over_thresh {
        "Overexposed"
    } else {
        "Normal"
    };

    ExposureResult {
        mean_luminance,
        pct_underexposed,
        pct_overexposed,
        verdict: verdict.to_string(),
    }
}

// ── Duplicate grouping (hash-only, no image data) ──

pub struct DupEntry {
    pub index: usize,
    pub timestamp: i64,
    pub phash: Option<Vec<u8>>,
}

pub fn find_duplicate_groups(
    entries: &[DupEntry],
    time_window_secs: i64,
    hash_distance_threshold: u32,
) -> HashMap<usize, String> {
    if entries.len() < 2 {
        return HashMap::new();
    }

    // Cluster by timestamp
    let mut sorted: Vec<(usize, i64)> = entries.iter().map(|e| (e.index, e.timestamp)).collect();
    sorted.sort_by_key(|&(_, ts)| ts);

    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = vec![sorted[0].0];
    let mut last_ts = sorted[0].1;

    for &(idx, ts) in &sorted[1..] {
        if (ts - last_ts).abs() <= time_window_secs {
            current.push(idx);
        } else {
            if current.len() >= 2 {
                clusters.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            current.push(idx);
        }
        last_ts = ts;
    }
    if current.len() >= 2 {
        clusters.push(current);
    }

    // Union-find on perceptual hashes within clusters
    let n = entries.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }

    for cluster in &clusters {
        for i in 0..cluster.len() {
            for j in (i + 1)..cluster.len() {
                let a = cluster[i];
                let b = cluster[j];
                if let (Some(ha), Some(hb)) = (&entries[a].phash, &entries[b].phash) {
                    let dist = hamming_distance(ha, hb);
                    if dist <= hash_distance_threshold {
                        let ra = find(&mut parent, a);
                        let rb = find(&mut parent, b);
                        if ra != rb {
                            parent[ra] = rb;
                        }
                    }
                }
            }
        }
    }

    // Collect groups — only include images whose root differs from themselves
    let mut root_to_group: HashMap<usize, String> = HashMap::new();
    let mut result: HashMap<usize, String> = HashMap::new();
    let mut group_counter = 0u64;

    for i in 0..n {
        let root = find(&mut parent, i);
        // Check if this root has at least 2 members
        if root == i {
            let has_members = (0..n).any(|j| j != i && find(&mut parent, j) == root);
            if !has_members {
                continue;
            }
        }
        let group_id = root_to_group.entry(root).or_insert_with(|| {
            group_counter += 1;
            format!("dup-{}", group_counter)
        });
        result.insert(i, group_id.clone());
    }

    result
}

fn hamming_distance(a: &[u8], b: &[u8]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

use crate::index::metadata::parse_exif_date;

// ── Subject focus detection ──

use crate::index::store::{ClosedEyesResult, SubjectFocusResult};

fn compute_subject_focus(
    gray: &image::GrayImage,
    closed_eyes: Option<&ClosedEyesResult>,
) -> Option<SubjectFocusResult> {
    let faces = closed_eyes?;
    if faces.face_count == 0 {
        return None;
    }

    // Collect face bounding boxes (Vision uses bottom-left origin, normalized 0-1)
    let boxes: Vec<[f64; 4]> = faces
        .faces
        .iter()
        .filter_map(|f| f.bounding_box)
        .collect();

    if boxes.is_empty() {
        return None;
    }

    let (w, h) = (gray.width() as usize, gray.height() as usize);
    if w < 3 || h < 3 {
        return None;
    }

    let pixels = gray.as_raw();

    // Accumulate Laplacian variance separately for face and background regions
    let mut face_sum = 0.0f64;
    let mut face_sum_sq = 0.0f64;
    let mut face_count = 0u64;
    let mut bg_sum = 0.0f64;
    let mut bg_sum_sq = 0.0f64;
    let mut bg_count = 0u64;

    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let center = pixels[y * w + x] as f64;
            let top = pixels[(y - 1) * w + x] as f64;
            let bottom = pixels[(y + 1) * w + x] as f64;
            let left = pixels[y * w + (x - 1)] as f64;
            let right = pixels[y * w + (x + 1)] as f64;
            let laplacian = top + bottom + left + right - 4.0 * center;

            // Check if pixel is inside any face bounding box
            // Vision bbox: [x, y, w, h] with bottom-left origin
            let nx = x as f64 / w as f64;
            let ny = 1.0 - (y as f64 / h as f64); // flip to bottom-left origin
            let in_face = boxes.iter().any(|b| {
                nx >= b[0] && nx <= b[0] + b[2] && ny >= b[1] && ny <= b[1] + b[3]
            });

            if in_face {
                face_sum += laplacian;
                face_sum_sq += laplacian * laplacian;
                face_count += 1;
            } else {
                bg_sum += laplacian;
                bg_sum_sq += laplacian * laplacian;
                bg_count += 1;
            }
        }
    }

    if face_count == 0 || bg_count == 0 {
        return None;
    }

    let face_mean = face_sum / face_count as f64;
    let face_var = (face_sum_sq / face_count as f64) - (face_mean * face_mean);
    let bg_mean = bg_sum / bg_count as f64;
    let bg_var = (bg_sum_sq / bg_count as f64) - (bg_mean * bg_mean);

    let focus_ratio = if bg_var > 0.001 { face_var / bg_var } else { 10.0 };
    let threshold = 100.0;

    let verdict = if face_var >= threshold && focus_ratio >= 0.8 {
        "SubjectSharp" // face is sharp (bg may be blurry = intentional bokeh)
    } else if face_var < threshold && bg_var >= threshold {
        "BackFocus" // background sharp but face blurry
    } else if face_var < threshold && bg_var < threshold {
        "AllBlurry"
    } else {
        "SubjectBlurry"
    };

    Some(SubjectFocusResult {
        subject_blur_variance: face_var,
        background_blur_variance: bg_var,
        focus_ratio,
        verdict: verdict.to_string(),
    })
}

// ── Scene grouping (timestamp-based, broader than duplicates) ──

pub fn find_scene_groups(
    entries: &[DupEntry],
    scene_window_secs: i64,
) -> HashMap<usize, String> {
    if entries.len() < 2 {
        return HashMap::new();
    }

    let mut sorted: Vec<(usize, i64)> = entries.iter().map(|e| (e.index, e.timestamp)).collect();
    sorted.sort_by_key(|&(_, ts)| ts);

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = vec![sorted[0].0];
    let mut last_ts = sorted[0].1;

    for &(idx, ts) in &sorted[1..] {
        if (ts - last_ts).abs() <= scene_window_secs {
            current.push(idx);
        } else {
            if current.len() >= 2 {
                groups.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            current.push(idx);
        }
        last_ts = ts;
    }
    if current.len() >= 2 {
        groups.push(current);
    }

    let mut result = HashMap::new();
    for (i, group) in groups.iter().enumerate() {
        let scene_id = format!("scene-{}", i + 1);
        for &idx in group {
            result.insert(idx, scene_id.clone());
        }
    }
    result
}
