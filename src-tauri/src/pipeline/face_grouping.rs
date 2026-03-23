//! Face grouping by person via Apple Vision framework (macOS only).
//!
//! Uses VNGenerateImageFeaturePrintRequest to create per-face embeddings,
//! then clusters them to identify unique people across images.
//!
//! Performance: ~15-30ms per face for feature print generation.

use std::collections::HashMap;
use std::path::Path;

use crate::index::store::FaceInfo;

/// Extract face embeddings and generate face crop thumbnails.
/// Returns a Vec of FaceInfo per detected face, plus raw embeddings for clustering.
///
/// On non-macOS platforms, returns empty results.
#[cfg(not(target_os = "macos"))]
pub fn extract_faces(
    _image: &image::DynamicImage,
    _face_boxes: &[[f64; 4]],
    _thumb_cache_dir: &Path,
    _image_id: &str,
) -> (Vec<FaceInfo>, Vec<Vec<f32>>) {
    (vec![], vec![])
}

#[cfg(target_os = "macos")]
pub fn extract_faces(
    image: &image::DynamicImage,
    face_boxes: &[[f64; 4]],
    thumb_cache_dir: &Path,
    image_id: &str,
) -> (Vec<FaceInfo>, Vec<Vec<f32>>) {
    let mut faces = Vec::new();
    let mut embeddings = Vec::new();

    let (img_w, img_h) = (image.width(), image.height());
    if img_w == 0 || img_h == 0 {
        return (faces, embeddings);
    }

    for (i, bbox) in face_boxes.iter().enumerate() {
        // Convert normalized bbox (bottom-left origin) to pixel coords (top-left origin)
        // Tight crop (5% padding) for embedding — minimizes background influence.
        // Wider crop (20% padding) for the thumbnail display.
        let emb_pad = 0.05;
        let thumb_pad = 0.20;

        let tight_bx = (bbox[0] - emb_pad * bbox[2]).max(0.0);
        let tight_by = (1.0 - bbox[1] - bbox[3] - emb_pad * bbox[3]).max(0.0);
        let tight_bw = (bbox[2] * (1.0 + 2.0 * emb_pad)).min(1.0 - tight_bx);
        let tight_bh = (bbox[3] * (1.0 + 2.0 * emb_pad)).min(1.0 - tight_by);

        let wide_bx = (bbox[0] - thumb_pad * bbox[2]).max(0.0);
        let wide_by = (1.0 - bbox[1] - bbox[3] - thumb_pad * bbox[3]).max(0.0);
        let wide_bw = (bbox[2] * (1.0 + 2.0 * thumb_pad)).min(1.0 - wide_bx);
        let wide_bh = (bbox[3] * (1.0 + 2.0 * thumb_pad)).min(1.0 - wide_by);

        let tight_px = (tight_bx * img_w as f64) as u32;
        let tight_py = (tight_by * img_h as f64) as u32;
        let tight_pw = ((tight_bw * img_w as f64) as u32).min(img_w - tight_px).max(1);
        let tight_ph = ((tight_bh * img_h as f64) as u32).min(img_h - tight_py).max(1);

        let wide_px = (wide_bx * img_w as f64) as u32;
        let wide_py = (wide_by * img_h as f64) as u32;
        let wide_pw = ((wide_bw * img_w as f64) as u32).min(img_w - wide_px).max(1);
        let wide_ph = ((wide_bh * img_h as f64) as u32).min(img_h - wide_py).max(1);

        let tight_crop = image.crop_imm(tight_px, tight_py, tight_pw, tight_ph);
        let wide_crop = image.crop_imm(wide_px, wide_py, wide_pw, wide_ph);

        // Save face crop thumbnail (wide crop for display)
        let face_thumb_name = format!("{}_face_{}.jpg", &image_id[..16.min(image_id.len())], i);
        let face_thumb_path = thumb_cache_dir.join(&face_thumb_name);
        if !face_thumb_path.exists() {
            let small = wide_crop.resize(120, 120, image::imageops::FilterType::Triangle);
            let rgb = small.to_rgb8();
            let mut buf = Vec::new();
            if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 75)
                .encode(&rgb, rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                .is_ok()
            {
                std::fs::write(&face_thumb_path, &buf).ok();
            }
        }

        // Generate feature print embedding (tight crop for identity)
        let embedding = generate_feature_print(&tight_crop);

        faces.push(FaceInfo {
            face_index: i as u32,
            bounding_box: *bbox,
            person_id: None, // filled in after clustering
            face_thumbnail_path: Some(face_thumb_path.to_string_lossy().to_string()),
        });

        if let Some(emb) = embedding {
            embeddings.push(emb);
        } else {
            embeddings.push(vec![]); // placeholder for index alignment
        }
    }

    (faces, embeddings)
}

/// Generate a feature print embedding for a face crop image.
/// Uses VNGenerateImageFeaturePrintRequest on a CGImage created from pixel data.
#[cfg(target_os = "macos")]
fn generate_feature_print(face_crop: &image::DynamicImage) -> Option<Vec<f32>> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::c_void;

    let rgb = face_crop.to_rgb8();
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }

    unsafe {
        // Create CGImage from RGB pixels via bitmap context
        let color_space = CGColorSpaceCreateDeviceRGB();
        let bytes_per_row = w * 4;
        let bitmap_info: u32 = 5 | (4 << 12); // kCGImageAlphaNoneSkipLast | kCGBitmapByteOrder32Big

        // Convert RGB to RGBX (add padding byte)
        let mut rgbx = Vec::with_capacity(w * h * 4);
        for pixel in rgb.pixels() {
            rgbx.push(pixel[0]);
            rgbx.push(pixel[1]);
            rgbx.push(pixel[2]);
            rgbx.push(255);
        }

        let context = CGBitmapContextCreate(
            rgbx.as_mut_ptr() as *mut c_void,
            w, h, 8, bytes_per_row,
            color_space, bitmap_info,
        );
        CGColorSpaceRelease(color_space);
        if context.is_null() { return None; }

        let cg_image = CGBitmapContextCreateImage(context);
        CGContextRelease(context);
        if cg_image.is_null() { return None; }

        // Run VNGenerateImageFeaturePrintRequest
        let request_cls = match AnyClass::get(c"VNGenerateImageFeaturePrintRequest") {
            Some(c) => c,
            None => { CGImageRelease(cg_image); return None; }
        };
        let request: *mut AnyObject = msg_send![request_cls, alloc];
        let request: *mut AnyObject = msg_send![request, init];
        if request.is_null() {
            CGImageRelease(cg_image);
            return None;
        }

        let dict_cls = match AnyClass::get(c"NSDictionary") {
            Some(c) => c,
            None => {
                let _: () = msg_send![request, release];
                CGImageRelease(cg_image);
                return None;
            }
        };
        let empty_dict: *mut AnyObject = msg_send![dict_cls, alloc];
        let empty_dict: *mut AnyObject = msg_send![empty_dict, init];

        let handler_cls = match AnyClass::get(c"VNImageRequestHandler") {
            Some(c) => c,
            None => {
                let _: () = msg_send![request, release];
                let _: () = msg_send![empty_dict, release];
                CGImageRelease(cg_image);
                return None;
            }
        };
        let handler: *mut AnyObject = msg_send![handler_cls, alloc];
        let handler: *mut AnyObject = msg_send![handler, initWithCGImage: cg_image, options: empty_dict];

        if handler.is_null() {
            let _: () = msg_send![request, release];
            let _: () = msg_send![empty_dict, release];
            CGImageRelease(cg_image);
            return None;
        }

        let array_cls = match AnyClass::get(c"NSArray") {
            Some(c) => c,
            None => {
                let _: () = msg_send![request, release];
                let _: () = msg_send![handler, release];
                let _: () = msg_send![empty_dict, release];
                CGImageRelease(cg_image);
                return None;
            }
        };
        let array: *mut AnyObject = msg_send![array_cls, arrayWithObject: request];

        let mut error: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = msg_send![handler, performRequests: array, error: &mut error];

        if !ok {
            let _: () = msg_send![request, release];
            let _: () = msg_send![handler, release];
            let _: () = msg_send![empty_dict, release];
            CGImageRelease(cg_image);
            return None;
        }

        // Get VNFeaturePrintObservation from results
        let observations: *mut AnyObject = msg_send![request, results];
        let count: usize = if !observations.is_null() {
            msg_send![observations, count]
        } else {
            0
        };

        let result = if count > 0 {
            let obs: *mut AnyObject = msg_send![observations, objectAtIndex: 0usize];
            if !obs.is_null() {
                // Get element count and data
                let element_count: usize = msg_send![obs, elementCount];
                let data: *const AnyObject = msg_send![obs, data];
                if !data.is_null() && element_count > 0 {
                    let bytes_ptr: *const u8 = msg_send![data, bytes];
                    let length: usize = msg_send![data, length];
                    if !bytes_ptr.is_null() && length >= element_count * 4 {
                        let float_ptr = bytes_ptr as *const f32;
                        let slice = std::slice::from_raw_parts(float_ptr, element_count);
                        Some(slice.to_vec())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let _: () = msg_send![request, release];
        let _: () = msg_send![handler, release];
        let _: () = msg_send![empty_dict, release];
        CGImageRelease(cg_image);

        result
    }
}

/// Cluster face embeddings using average-linkage agglomerative clustering
/// with cosine similarity. `similarity_threshold` is the minimum average cosine
/// similarity between a candidate face and all members of an existing cluster
/// for it to be merged in (0.0 = unrelated, 1.0 = identical).
///
/// Average-linkage prevents the transitive "chaining" problem of single-linkage,
/// where two dissimilar faces merge because each is similar to a shared third face.
pub fn cluster_faces(
    entries: &[(String, u32, Vec<f32>)], // (image_id, face_index, embedding)
    similarity_threshold: f32,
) -> HashMap<String, Vec<(String, u32)>> {
    if entries.len() < 2 {
        return HashMap::new();
    }

    // Filter out entries with empty embeddings
    let valid: Vec<(usize, &(String, u32, Vec<f32>))> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.2.is_empty())
        .collect();

    if valid.len() < 2 {
        return HashMap::new();
    }

    let n = valid.len();

    // Pre-compute the full NxN cosine similarity matrix
    let norms: Vec<f32> = valid
        .iter()
        .map(|(_, e)| {
            let sum: f32 = e.2.iter().map(|x| x * x).sum();
            sum.sqrt()
        })
        .collect();

    let mut sim = vec![vec![0.0f32; n]; n];
    for i in 0..n {
        sim[i][i] = 1.0;
        if norms[i] == 0.0 {
            continue;
        }
        for j in (i + 1)..n {
            if norms[j] == 0.0 {
                continue;
            }
            let dot: f32 = valid[i]
                .1
                .2
                .iter()
                .zip(valid[j].1 .2.iter())
                .map(|(a, b)| a * b)
                .sum();
            let s = dot / (norms[i] * norms[j]);
            sim[i][j] = s;
            sim[j][i] = s;
        }
    }

    // Average-linkage agglomerative clustering with Lance-Williams update.
    // Maintain an inter-cluster distance matrix so each merge is O(n), not O(n^2).
    let mut clusters: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let mut alive: Vec<bool> = vec![true; n]; // which cluster indices are still active
    // csim[i][j] = average pairwise similarity between cluster i and j
    // Initially just the point-to-point similarities.
    let mut csim = sim.clone();

    loop {
        // Find most similar pair among alive clusters
        let mut best_sim = f32::NEG_INFINITY;
        let mut best_i = 0;
        let mut best_j = 0;
        for i in 0..clusters.len() {
            if !alive[i] { continue; }
            for j in (i + 1)..clusters.len() {
                if !alive[j] { continue; }
                if csim[i][j] > best_sim {
                    best_sim = csim[i][j];
                    best_i = i;
                    best_j = j;
                }
            }
        }

        if best_sim < similarity_threshold || best_i == best_j {
            break;
        }

        // Merge j into i, update inter-cluster similarities via Lance-Williams
        let ni = clusters[best_i].len() as f32;
        let nj = clusters[best_j].len() as f32;
        for k in 0..clusters.len() {
            if !alive[k] || k == best_i || k == best_j { continue; }
            // Average-linkage: new_sim(i+j, k) = (ni*sim(i,k) + nj*sim(j,k)) / (ni+nj)
            let new_sim = (ni * csim[best_i][k] + nj * csim[best_j][k]) / (ni + nj);
            csim[best_i][k] = new_sim;
            csim[k][best_i] = new_sim;
        }

        // Move members from j to i
        let members_j = std::mem::take(&mut clusters[best_j]);
        clusters[best_i].extend(members_j);
        alive[best_j] = false;
    }

    // Build result — every face gets a person-id
    let mut result: HashMap<String, Vec<(String, u32)>> = HashMap::new();
    let mut group_counter = 0u32;
    for (ci, cluster) in clusters.iter().enumerate() {
        if !alive[ci] || cluster.is_empty() { continue; }
        group_counter += 1;
        let pid = format!("person-{}", group_counter);
        for &fi in cluster {
            result
                .entry(pid.clone())
                .or_default()
                .push((valid[fi].1 .0.clone(), valid[fi].1 .1));
        }
    }

    result
}

// ── Apple framework FFI ──

#[cfg(target_os = "macos")]
#[link(name = "Vision", kind = "framework")]
extern "C" {}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *const std::ffi::c_void;
    fn CGColorSpaceRelease(space: *const std::ffi::c_void);
    fn CGBitmapContextCreate(
        data: *mut std::ffi::c_void,
        width: usize, height: usize,
        bits_per_component: usize, bytes_per_row: usize,
        space: *const std::ffi::c_void, bitmap_info: u32,
    ) -> *const std::ffi::c_void;
    fn CGBitmapContextCreateImage(context: *const std::ffi::c_void) -> *const std::ffi::c_void;
    fn CGContextRelease(context: *const std::ffi::c_void);
    fn CGImageRelease(image: *const std::ffi::c_void);
}
