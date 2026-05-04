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
        let blur = compute_blur(&gray, exif.as_ref());
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

        let face_boxes: Vec<[f64; 4]> = closed_eyes.as_ref()
            .map(|ce| ce.faces.iter().filter_map(|f| f.bounding_box).collect())
            .unwrap_or_default();

        // Subject-mask source: prefer faces (already detected, free); fall back
        // to Vision saliency for non-human subjects only when no faces were
        // found, so people-photos don't pay the extra Vision call.
        let subject_focus = if !face_boxes.is_empty() {
            compute_subject_focus(&gray, &face_boxes, "face")
        } else if let Some(boxes) = crate::pipeline::saliency::detect(jpeg_data.as_deref()) {
            if boxes.is_empty() {
                None
            } else {
                compute_subject_focus(&gray, &boxes, "saliency")
            }
        } else {
            None
        };

        // Override the tile-based default classification using subject-focus
        // when it returns a definitive verdict — Vision tells us *where* the
        // subject is, so a soft subject + soft background outweighs scattered
        // sharp tiles that would otherwise rescue the photo.
        let blur = if let Some(sf) = subject_focus.as_ref() {
            let mut b = blur;
            match sf.verdict.as_str() {
                "AllBlurry" | "SubjectBlurry" => b.is_blurry = true,
                "SubjectSharp" => b.is_blurry = false,
                _ => {}
            }
            b
        } else {
            blur
        };

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
    // Run all warmups concurrently via rayon since they load different models.
    let face_grouping = true;
    {
        let warmup_start = Instant::now();
        rayon::scope(|s| {
            s.spawn(|_| crate::pipeline::closed_eyes::warmup_face_detection_model());
            s.spawn(|_| crate::pipeline::saliency::warmup_saliency_model());
            if face_grouping {
                s.spawn(|_| crate::pipeline::face_grouping::warmup_feature_print_model());
            }
        });
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

/// Canonical sharpness threshold (Laplacian variance). A photo with global
/// variance below this is "globally soft"; a tile above it is "locally sharp".
const SHARP_THRESHOLD: f64 = 100.0;

/// Tile size in pixels for spatial blur analysis. On a 1024px analysis image
/// this yields a ~16×N grid — fine enough to localize a subject, coarse enough
/// to be cheap and noise-robust.
const TILE_SIZE: usize = 64;

fn compute_blur(gray: &image::GrayImage, exif: Option<&metadata::ExifMetadata>) -> BlurResult {
    let (w, h) = (gray.width() as usize, gray.height() as usize);

    if w < 3 || h < 3 {
        return BlurResult {
            laplacian_variance: 0.0,
            mean_tile_variance: 0.0,
            max_tile_variance: 0.0,
            p95_tile_variance: 0.0,
            sharp_tile_fraction: 0.0,
            largest_sharp_cluster: 0.0,
            bokeh_likely: false,
            shake_risk: false,
            is_blurry: true,
        };
    }

    let pixels = gray.as_raw();

    // ── Tile-major scan: each tile's inner loop is a tight 3-scalar accumulator,
    // mirroring the original global Laplacian loop. Global stats are derived
    // by summing across tiles after the fact, so no per-pixel Vec writes. ──
    let tiles_x = w.div_ceil(TILE_SIZE);
    let tiles_y = h.div_ceil(TILE_SIZE);
    let n_tiles = tiles_x * tiles_y;
    let mut tile_sum = vec![0.0f64; n_tiles];
    let mut tile_sum_sq = vec![0.0f64; n_tiles];
    let mut tile_count = vec![0u32; n_tiles];

    for ty in 0..tiles_y {
        let y_start = (ty * TILE_SIZE).max(1);
        let y_end = ((ty + 1) * TILE_SIZE).min(h - 1);
        if y_start >= y_end {
            continue;
        }
        for tx in 0..tiles_x {
            let x_start = (tx * TILE_SIZE).max(1);
            let x_end = ((tx + 1) * TILE_SIZE).min(w - 1);
            if x_start >= x_end {
                continue;
            }
            let mut sum = 0.0f64;
            let mut sum_sq = 0.0f64;
            let mut count = 0u32;
            for y in y_start..y_end {
                let row = y * w;
                let row_above = (y - 1) * w;
                let row_below = (y + 1) * w;
                for x in x_start..x_end {
                    let center = pixels[row + x] as f64;
                    let top = pixels[row_above + x] as f64;
                    let bottom = pixels[row_below + x] as f64;
                    let left = pixels[row + (x - 1)] as f64;
                    let right = pixels[row + (x + 1)] as f64;
                    let laplacian = top + bottom + left + right - 4.0 * center;
                    sum += laplacian;
                    sum_sq += laplacian * laplacian;
                    count += 1;
                }
            }
            let ti = ty * tiles_x + tx;
            tile_sum[ti] = sum;
            tile_sum_sq[ti] = sum_sq;
            tile_count[ti] = count;
        }
    }

    let global_sum: f64 = tile_sum.iter().sum();
    let global_sum_sq: f64 = tile_sum_sq.iter().sum();
    let global_count: u64 = tile_count.iter().map(|&c| c as u64).sum();
    if global_count == 0 {
        return BlurResult {
            laplacian_variance: 0.0,
            mean_tile_variance: 0.0,
            max_tile_variance: 0.0,
            p95_tile_variance: 0.0,
            sharp_tile_fraction: 0.0,
            largest_sharp_cluster: 0.0,
            bokeh_likely: false,
            shake_risk: false,
            is_blurry: true,
        };
    }
    let global_mean = global_sum / global_count as f64;
    let global_variance = (global_sum_sq / global_count as f64) - (global_mean * global_mean);

    // Per-tile variance, ignoring undersized edge tiles (need a meaningful sample).
    let min_tile_pixels = (TILE_SIZE * TILE_SIZE / 4) as u32;
    let mut tile_vars: Vec<f64> = (0..tile_sum.len())
        .filter(|&i| tile_count[i] >= min_tile_pixels)
        .map(|i| {
            let n = tile_count[i] as f64;
            let m = tile_sum[i] / n;
            (tile_sum_sq[i] / n) - (m * m)
        })
        .collect();

    if tile_vars.is_empty() {
        return BlurResult {
            laplacian_variance: global_variance,
            mean_tile_variance: global_variance,
            max_tile_variance: global_variance,
            p95_tile_variance: global_variance,
            sharp_tile_fraction: 0.0,
            largest_sharp_cluster: 0.0,
            bokeh_likely: false,
            shake_risk: false,
            is_blurry: global_variance < SHARP_THRESHOLD,
        };
    }

    let mean_tile_variance = tile_vars.iter().sum::<f64>() / tile_vars.len() as f64;
    let max_tile_variance = tile_vars.iter().cloned().fold(0.0f64, f64::max);

    // P95 via partial sort (cheap for ≤ ~400 tiles).
    let p95_idx = ((tile_vars.len() as f64 * 0.95) as usize).min(tile_vars.len() - 1);
    tile_vars.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p95_tile_variance = tile_vars[p95_idx];

    // Sharp tile fraction (recompute on the original grid for cluster analysis).
    let sharp_grid: Vec<bool> = (0..tile_sum.len())
        .map(|i| {
            if tile_count[i] < min_tile_pixels {
                return false;
            }
            let n = tile_count[i] as f64;
            let m = tile_sum[i] / n;
            let v = (tile_sum_sq[i] / n) - (m * m);
            v >= SHARP_THRESHOLD
        })
        .collect();

    let sharp_count = sharp_grid.iter().filter(|&&s| s).count();
    let total_valid = (0..tile_count.len())
        .filter(|&i| tile_count[i] >= min_tile_pixels)
        .count()
        .max(1);
    let sharp_tile_fraction = sharp_count as f64 / total_valid as f64;

    // Largest connected cluster of sharp tiles (4-connectivity flood fill).
    let largest_sharp_cluster =
        largest_cluster_size(&sharp_grid, tiles_x, tiles_y) as f64 / total_valid as f64;

    // ── EXIF intent signals ──
    let (bokeh_likely, shake_risk) = exif_intent(exif);

    // Default classification — frontend may override via threshold + tile metrics.
    // A "strong sharp region" rescues a globally-soft frame:
    //  - either a single tile is well above the noise/texture floor (peak detector), or
    //  - a meaningful coherent area is sharp (fraction AND cluster, together — neither
    //    on its own, since slightly soft photos can have scattered "sharp-ish" tiles
    //    from JPEG artifacts or stray texture).
    // Bokeh-likely shots get relaxed thresholds because intentional shallow DOF puts
    // most of the frame out of focus on purpose.
    let (peak_thresh, frac_thresh, cluster_thresh) = if bokeh_likely {
        (700.0, 0.04, 0.025)
    } else {
        (900.0, 0.12, 0.06)
    };
    let has_strong_sharp = max_tile_variance >= peak_thresh
        || (sharp_tile_fraction >= frac_thresh && largest_sharp_cluster >= cluster_thresh);
    let is_blurry = !has_strong_sharp && global_variance < SHARP_THRESHOLD;

    BlurResult {
        laplacian_variance: global_variance,
        mean_tile_variance,
        max_tile_variance,
        p95_tile_variance,
        sharp_tile_fraction,
        largest_sharp_cluster,
        bokeh_likely,
        shake_risk,
        is_blurry,
    }
}

fn largest_cluster_size(grid: &[bool], w: usize, h: usize) -> usize {
    let mut visited = vec![false; grid.len()];
    let mut best = 0usize;
    let mut stack: Vec<usize> = Vec::with_capacity(64);

    for start in 0..grid.len() {
        if !grid[start] || visited[start] {
            continue;
        }
        stack.clear();
        stack.push(start);
        visited[start] = true;
        let mut size = 0usize;
        while let Some(i) = stack.pop() {
            size += 1;
            let x = i % w;
            let y = i / w;
            // 4-neighbors
            if x > 0 {
                let n = i - 1;
                if grid[n] && !visited[n] {
                    visited[n] = true;
                    stack.push(n);
                }
            }
            if x + 1 < w {
                let n = i + 1;
                if grid[n] && !visited[n] {
                    visited[n] = true;
                    stack.push(n);
                }
            }
            if y > 0 {
                let n = i - w;
                if grid[n] && !visited[n] {
                    visited[n] = true;
                    stack.push(n);
                }
            }
            if y + 1 < h {
                let n = i + w;
                if grid[n] && !visited[n] {
                    visited[n] = true;
                    stack.push(n);
                }
            }
        }
        if size > best {
            best = size;
        }
    }
    best
}

/// Derive intent flags from EXIF.
/// `bokeh_likely`: wide aperture or long-lens narrow-DOF shooting.
/// `shake_risk`: shutter slower than the reciprocal-focal-length rule of thumb.
fn exif_intent(exif: Option<&metadata::ExifMetadata>) -> (bool, bool) {
    let Some(exif) = exif else {
        return (false, false);
    };

    let aperture = exif.aperture;
    let focal = exif.focal_length_mm;

    // Bokeh likely: f < 2.8, or f < 4.0 on a long lens (≥ 70mm).
    let bokeh_likely = match (aperture, focal) {
        (Some(a), _) if a < 2.8 => true,
        (Some(a), Some(f)) if a < 4.0 && f >= 70.0 => true,
        _ => false,
    };

    // Shake risk: shutter (sec) * focal_length > 1.0 (i.e. shutter > 1/focal).
    // We don't know if a tripod was used or stabilization was on; this is just a flag.
    let shake_risk = match (parse_shutter_secs(exif.shutter_speed.as_deref()), focal) {
        (Some(s), Some(f)) if f > 0.0 => s * f > 1.0,
        _ => false,
    };

    (bokeh_likely, shake_risk)
}

/// Parse an EXIF shutter speed string like "1/250", "1/1.6 s", or "0.5" into seconds.
fn parse_shutter_secs(s: Option<&str>) -> Option<f64> {
    let raw = s?.trim().trim_matches('"').trim();
    let cleaned = raw
        .trim_end_matches('s')
        .trim_end_matches(" s")
        .trim();
    if let Some((num, den)) = cleaned.split_once('/') {
        let n: f64 = num.trim().parse().ok()?;
        let d: f64 = den.trim().parse().ok()?;
        if d > 0.0 { Some(n / d) } else { None }
    } else {
        cleaned.parse().ok()
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

use crate::index::store::SubjectFocusResult;

/// Compare Laplacian variance inside subject bboxes vs. outside.
/// `boxes` are normalized (0-1) with bottom-left origin (Vision convention).
/// `source` labels where the boxes came from for downstream debugging:
/// "face" or "saliency".
fn compute_subject_focus(
    gray: &image::GrayImage,
    boxes: &[[f64; 4]],
    source: &str,
) -> Option<SubjectFocusResult> {
    if boxes.is_empty() {
        return None;
    }

    let (w, h) = (gray.width() as usize, gray.height() as usize);
    if w < 3 || h < 3 {
        return None;
    }

    let pixels = gray.as_raw();

    // Accumulate Laplacian variance separately for subject and background regions.
    let mut subj_sum = 0.0f64;
    let mut subj_sum_sq = 0.0f64;
    let mut subj_count = 0u64;
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

            let nx = x as f64 / w as f64;
            let ny = 1.0 - (y as f64 / h as f64);
            let in_subj = boxes.iter().any(|b| {
                nx >= b[0] && nx <= b[0] + b[2] && ny >= b[1] && ny <= b[1] + b[3]
            });

            if in_subj {
                subj_sum += laplacian;
                subj_sum_sq += laplacian * laplacian;
                subj_count += 1;
            } else {
                bg_sum += laplacian;
                bg_sum_sq += laplacian * laplacian;
                bg_count += 1;
            }
        }
    }

    if subj_count == 0 || bg_count == 0 {
        return None;
    }

    let subj_mean = subj_sum / subj_count as f64;
    let subj_var = (subj_sum_sq / subj_count as f64) - (subj_mean * subj_mean);
    let bg_mean = bg_sum / bg_count as f64;
    let bg_var = (bg_sum_sq / bg_count as f64) - (bg_mean * bg_mean);

    let focus_ratio = if bg_var > 0.001 { subj_var / bg_var } else { 10.0 };
    let threshold = 100.0;

    let verdict = if subj_var >= threshold && focus_ratio >= 0.8 {
        "SubjectSharp"
    } else if subj_var < threshold && bg_var >= threshold {
        "BackFocus"
    } else if subj_var < threshold && bg_var < threshold {
        "AllBlurry"
    } else {
        "SubjectBlurry"
    };

    Some(SubjectFocusResult {
        subject_blur_variance: subj_var,
        background_blur_variance: bg_var,
        focus_ratio,
        verdict: verdict.to_string(),
        subject_source: source.to_string(),
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

#[cfg(test)]
mod blur_tests {
    use super::*;
    use image::GrayImage;

    fn make_gray(w: u32, h: u32, fill: impl Fn(u32, u32) -> u8) -> GrayImage {
        let mut img = GrayImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(x, y, image::Luma([fill(x, y)]));
            }
        }
        img
    }

    fn checker(scale: u32) -> impl Fn(u32, u32) -> u8 {
        move |x, y| if ((x / scale) ^ (y / scale)) & 1 == 0 { 0 } else { 255 }
    }

    /// A globally-uniform blurry image (single tone) is correctly flagged.
    #[test]
    fn flat_image_is_blurry() {
        let img = make_gray(512, 384, |_, _| 128);
        let r = compute_blur(&img, None);
        assert!(r.is_blurry, "flat image should be blurry");
        assert!(r.max_tile_variance < SHARP_THRESHOLD);
        assert!(r.sharp_tile_fraction < 0.01);
    }

    /// Globally sharp checkerboard — not blurry, sharp_fraction high.
    #[test]
    fn sharp_checker_is_not_blurry() {
        let img = make_gray(512, 384, checker(8));
        let r = compute_blur(&img, None);
        assert!(!r.is_blurry);
        assert!(r.sharp_tile_fraction > 0.5);
        assert!(r.max_tile_variance > SHARP_THRESHOLD * 5.0);
    }

    /// THE DOG PHOTOSHOOT: small sharp subject in the center, soft (uniform)
    /// background everywhere else. Global variance is low but a sharp region
    /// exists locally — must NOT be classified as blurry.
    #[test]
    fn sharp_subject_on_bokeh_background_is_kept() {
        // 512×384 frame, ~96×96 sharp center patch, rest is flat 128.
        let cx_lo = 208;
        let cx_hi = 304;
        let cy_lo = 144;
        let cy_hi = 240;
        let img = make_gray(512, 384, |x, y| {
            if x >= cx_lo && x < cx_hi && y >= cy_lo && y < cy_hi {
                checker(4)(x, y)
            } else {
                128
            }
        });
        let r = compute_blur(&img, None);
        assert!(
            r.max_tile_variance > SHARP_THRESHOLD * 5.0,
            "expected a sharp tile, got max={}",
            r.max_tile_variance
        );
        assert!(
            !r.is_blurry,
            "sharp-subject-on-soft-bg must not be flagged blurry (max_tile={}, sharp_frac={})",
            r.max_tile_variance, r.sharp_tile_fraction
        );
    }

    /// EXIF biasing: a wide-aperture shot with even smaller sharp area still
    /// passes the more permissive bokeh-likely fraction threshold.
    #[test]
    fn wide_aperture_lowers_sharp_fraction_bar() {
        // ~64×64 sharp patch on a 512×384 frame → ~4% of tiles sharp.
        // Without bokeh hint, default 5% threshold rejects.
        // With wide aperture (f/1.8), 2% threshold accepts.
        let img = make_gray(512, 384, |x, y| {
            if (224..288).contains(&x) && (160..224).contains(&y) {
                checker(4)(x, y)
            } else {
                128
            }
        });
        let exif_open = metadata::ExifMetadata {
            date_time_original: None,
            gps_lat: None,
            gps_lng: None,
            camera_make: None,
            camera_model: None,
            focal_length_mm: Some(85.0),
            aperture: Some(1.8),
            iso: Some(200),
            shutter_speed: Some("1/250".into()),
            orientation: None,
        };
        let r_open = compute_blur(&img, Some(&exif_open));
        assert!(r_open.bokeh_likely);
        // Even though the sharp fraction is small, it still has a sharp region
        // with strong max-tile variance — must not be blurry.
        assert!(!r_open.is_blurry);
    }

    /// Shake-risk EXIF parsing: 1/30s at 200mm exceeds reciprocal rule.
    #[test]
    fn shake_risk_detected_from_long_exposure() {
        let exif = metadata::ExifMetadata {
            date_time_original: None,
            gps_lat: None,
            gps_lng: None,
            camera_make: None,
            camera_model: None,
            focal_length_mm: Some(200.0),
            aperture: Some(5.6),
            iso: Some(800),
            shutter_speed: Some("1/30".into()),
            orientation: None,
        };
        let (bokeh, shake) = exif_intent(Some(&exif));
        assert!(!bokeh);
        assert!(shake, "1/30s @ 200mm should flag shake risk");
    }

    #[test]
    fn shutter_parse() {
        assert_eq!(parse_shutter_secs(Some("1/250")), Some(1.0 / 250.0));
        assert_eq!(parse_shutter_secs(Some("1/250 s")), Some(1.0 / 250.0));
        assert_eq!(parse_shutter_secs(Some("0.5")), Some(0.5));
        assert_eq!(parse_shutter_secs(Some("\"1/8000\"")), Some(1.0 / 8000.0));
        assert_eq!(parse_shutter_secs(None), None);
    }

    /// Real-photo regression check. Run with:
    ///   cargo test --manifest-path src-tauri/Cargo.toml --lib \
    ///       blur_tests::real_photos -- --ignored --nocapture
    /// Loads the two reference photos and asserts the dog stays sharp,
    /// the all-blurry shot stays blurry. Skipped by default so unit-test
    /// runs stay hermetic.
    #[test]
    #[ignore]
    fn real_photos() {
        // Manifest dir is src-tauri/, so test_data is one level up.
        let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let dog = project_root.join("test_data/dog_in_focus_blurry_background.jpg");
        let blurry = project_root.join("test_data/everything_blurry.jpg");
        assert!(dog.exists(), "missing {}", dog.display());
        assert!(blurry.exists(), "missing {}", blurry.display());

        for (path, label, expect_blurry) in [
            (&dog, "dog_in_focus_blurry_background", false),
            (&blurry, "everything_blurry", true),
        ] {
            let exif = metadata::extract_metadata(path);
            // Decode at full resolution then resize to ~1024px (matches the
            // pipeline's analysis image). Keeping the resize step matters —
            // tile-size constants were tuned for that resolution.
            let img = image::open(path).unwrap_or_else(|e| {
                panic!("failed to decode {}: {}", path.display(), e)
            });
            let resized = img.resize(1024, 1024, image::imageops::FilterType::Triangle);
            let gray = resized.to_luma8();
            let r = compute_blur(&gray, exif.as_ref());

            println!("\n── {} ──", label);
            println!("  dims:                 {}×{}", gray.width(), gray.height());
            println!("  global laplacian:     {:.1}", r.laplacian_variance);
            println!("  mean tile variance:   {:.1}", r.mean_tile_variance);
            println!("  max tile variance:    {:.1}", r.max_tile_variance);
            println!("  p95 tile variance:    {:.1}", r.p95_tile_variance);
            println!("  sharp tile fraction:  {:.1}%", r.sharp_tile_fraction * 100.0);
            println!("  largest sharp cluster:{:.1}%", r.largest_sharp_cluster * 100.0);
            println!("  bokeh_likely:         {}", r.bokeh_likely);
            println!("  shake_risk:           {}", r.shake_risk);
            println!("  is_blurry:            {}", r.is_blurry);
            if let Some(e) = &exif {
                println!(
                    "  exif: aperture={:?}  focal={:?}mm  shutter={:?}  iso={:?}",
                    e.aperture, e.focal_length_mm, e.shutter_speed, e.iso
                );
            }

            assert_eq!(
                r.is_blurry, expect_blurry,
                "{}: expected is_blurry={}, got {} (max_tile={:.1}, sharp_frac={:.1}%, mean_tile={:.1})",
                label, expect_blurry, r.is_blurry,
                r.max_tile_variance, r.sharp_tile_fraction * 100.0, r.mean_tile_variance
            );
        }
    }

    /// Saliency-based subject focus on photos with no human faces.
    /// Verifies the saliency fallback fires and produces sensible verdicts.
    /// Run with: cargo test ... blur_tests::saliency_subject_focus -- --ignored --nocapture
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn saliency_subject_focus() {
        use std::time::Instant;
        let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let dog = project_root.join("test_data/dog_in_focus_blurry_background.jpg");
        let blurry = project_root.join("test_data/everything_blurry.jpg");

        // Warm up the saliency model so the first per-image timing is honest.
        let warmup_start = Instant::now();
        crate::pipeline::saliency::warmup_saliency_model();
        println!("[warmup] saliency model: {}ms", warmup_start.elapsed().as_millis());

        for (path, label, expect_subject_sharp) in [
            (&dog, "dog_in_focus_blurry_background", true),
            (&blurry, "everything_blurry", false),
        ] {
            let jpeg = std::fs::read(path).expect("read jpeg");
            let img = image::open(path).expect("decode");
            let resized = img.resize(1024, 1024, image::imageops::FilterType::Triangle);
            let gray = resized.to_luma8();

            let t = Instant::now();
            let boxes = crate::pipeline::saliency::detect(Some(&jpeg))
                .expect("saliency returned None");
            let saliency_ms = t.elapsed().as_millis();

            println!("\n── {} ──", label);
            println!("  saliency call: {}ms, {} salient object(s)", saliency_ms, boxes.len());
            for (i, b) in boxes.iter().enumerate() {
                println!(
                    "    box[{}]: x={:.2} y={:.2} w={:.2} h={:.2} (covers {:.1}% of frame)",
                    i, b[0], b[1], b[2], b[3], b[2] * b[3] * 100.0
                );
            }

            assert!(
                !boxes.is_empty(),
                "{}: saliency must produce at least one bounding box",
                label
            );

            let sf = compute_subject_focus(&gray, &boxes, "saliency")
                .expect("compute_subject_focus returned None");

            println!(
                "  subject_focus: {} (subj={:.1}, bg={:.1}, ratio={:.2}, source={})",
                sf.verdict, sf.subject_blur_variance, sf.background_blur_variance,
                sf.focus_ratio, sf.subject_source
            );

            assert_eq!(sf.subject_source, "saliency");

            if expect_subject_sharp {
                assert_eq!(
                    sf.verdict, "SubjectSharp",
                    "{}: expected SubjectSharp verdict from saliency, got {} \
                     (subj_var={:.1}, bg_var={:.1}, ratio={:.2})",
                    label, sf.verdict, sf.subject_blur_variance,
                    sf.background_blur_variance, sf.focus_ratio
                );
            } else {
                // For everything_blurry, "AllBlurry" or "SubjectBlurry" are both
                // acceptable — the point is it should NOT be SubjectSharp.
                assert_ne!(
                    sf.verdict, "SubjectSharp",
                    "{}: blurry photo should not be SubjectSharp",
                    label
                );
            }
        }
    }
}
