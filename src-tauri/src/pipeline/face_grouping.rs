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
        let pad = 0.2; // 20% padding around face
        let bx = (bbox[0] - pad * bbox[2]).max(0.0);
        let by = (1.0 - bbox[1] - bbox[3] - pad * bbox[3]).max(0.0); // flip y
        let bw = (bbox[2] * (1.0 + 2.0 * pad)).min(1.0 - bx);
        let bh = (bbox[3] * (1.0 + 2.0 * pad)).min(1.0 - by);

        let px = (bx * img_w as f64) as u32;
        let py = (by * img_h as f64) as u32;
        let pw = ((bw * img_w as f64) as u32).min(img_w - px).max(1);
        let ph = ((bh * img_h as f64) as u32).min(img_h - py).max(1);

        // Crop face region
        let face_crop = image.crop_imm(px, py, pw, ph);

        // Save face crop thumbnail
        let face_thumb_name = format!("{}_face_{}.jpg", &image_id[..16.min(image_id.len())], i);
        let face_thumb_path = thumb_cache_dir.join(&face_thumb_name);
        if !face_thumb_path.exists() {
            let small = face_crop.resize(120, 120, image::imageops::FilterType::Triangle);
            let rgb = small.to_rgb8();
            let mut buf = Vec::new();
            if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 75)
                .encode(&rgb, rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                .is_ok()
            {
                std::fs::write(&face_thumb_path, &buf).ok();
            }
        }

        // Generate feature print embedding
        let embedding = generate_feature_print(&face_crop);

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
        let request_cls = AnyClass::get(c"VNGenerateImageFeaturePrintRequest")?;
        let request: *mut AnyObject = msg_send![request_cls, alloc];
        let request: *mut AnyObject = msg_send![request, init];
        if request.is_null() {
            CGImageRelease(cg_image);
            return None;
        }

        let dict_cls = AnyClass::get(c"NSDictionary")?;
        let empty_dict: *mut AnyObject = msg_send![dict_cls, alloc];
        let empty_dict: *mut AnyObject = msg_send![empty_dict, init];

        let handler_cls = AnyClass::get(c"VNImageRequestHandler")?;
        let handler: *mut AnyObject = msg_send![handler_cls, alloc];
        let handler: *mut AnyObject = msg_send![handler, initWithCGImage: cg_image, options: empty_dict];

        if handler.is_null() {
            let _: () = msg_send![request, release];
            let _: () = msg_send![empty_dict, release];
            CGImageRelease(cg_image);
            return None;
        }

        let array_cls = AnyClass::get(c"NSArray")?;
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

/// Cluster face embeddings into person groups using simple agglomerative clustering.
/// Returns a map of person_id -> [(image_id, face_index)].
pub fn cluster_faces(
    entries: &[(String, u32, Vec<f32>)], // (image_id, face_index, embedding)
    distance_threshold: f32,
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

    // Union-find for clustering
    let n = valid.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }

    // Compare all pairs and union if distance below threshold
    for i in 0..n {
        for j in (i + 1)..n {
            let dist = l2_distance(&valid[i].1 .2, &valid[j].1 .2);
            if dist < distance_threshold {
                let ri = find(&mut parent, i);
                let rj = find(&mut parent, j);
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    // Collect groups with 2+ members
    let mut root_to_group: HashMap<usize, String> = HashMap::new();
    let mut result: HashMap<String, Vec<(String, u32)>> = HashMap::new();
    let mut group_counter = 0u32;

    for i in 0..n {
        let root = find(&mut parent, i);
        let has_group = (0..n).any(|j| j != i && find(&mut parent, j) == root);
        if !has_group && root == i {
            // Single face, no group
            // Still assign a person-id for identification
            group_counter += 1;
            let pid = format!("person-{}", group_counter);
            result
                .entry(pid)
                .or_default()
                .push((valid[i].1 .0.clone(), valid[i].1 .1));
            continue;
        }

        let pid = root_to_group.entry(root).or_insert_with(|| {
            group_counter += 1;
            format!("person-{}", group_counter)
        });
        result
            .entry(pid.clone())
            .or_default()
            .push((valid[i].1 .0.clone(), valid[i].1 .1));
    }

    result
}

fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::MAX;
    }
    let sum: f32 = a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum();
    sum.sqrt()
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
