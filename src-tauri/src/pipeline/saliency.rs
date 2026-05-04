//! Class-agnostic subject detection via Apple Vision saliency (macOS only).
//!
//! Uses VNGenerateObjectnessBasedSaliencyImageRequest to find the salient
//! object(s) in the frame regardless of class — works on dogs, products,
//! flowers, anything Vision considers "object-like".
//!
//! The result is a small set of normalized bounding boxes (bottom-left
//! origin, like Vision's other observations), used by compute_subject_focus
//! when face detection finds nothing.
//!
//! Expected runtime: ~15-30ms per image after warm-up. Only invoked when
//! face detection yielded zero faces, so people-photos don't pay the cost.

/// Pre-warm the saliency model. Call once at scan start.
#[cfg(target_os = "macos")]
pub fn warmup_saliency_model() {
    let mut buf = Vec::new();
    let img = image::RgbImage::from_pixel(8, 8, image::Rgb([128, 128, 128]));
    image::codecs::jpeg::JpegEncoder::new(&mut buf)
        .encode(&img, 8, 8, image::ExtendedColorType::Rgb8)
        .ok();
    if !buf.is_empty() {
        let _ = detect_from_jpeg(&buf);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn warmup_saliency_model() {}

/// Detect salient object bounding boxes from raw JPEG data.
/// Boxes are normalized (0-1) with bottom-left origin (Vision convention).
/// Returns an empty Vec if no salient objects were found, None on platform
/// or detection failure.
#[cfg(target_os = "macos")]
pub fn detect(jpeg_data: Option<&[u8]>) -> Option<Vec<[f64; 4]>> {
    detect_from_jpeg(jpeg_data?)
}

#[cfg(not(target_os = "macos"))]
pub fn detect(_jpeg_data: Option<&[u8]>) -> Option<Vec<[f64; 4]>> {
    None
}

#[cfg(target_os = "macos")]
fn detect_from_jpeg(jpeg_data: &[u8]) -> Option<Vec<[f64; 4]>> {
    use std::ffi::c_void;

    unsafe {
        let cf_data = CFDataCreate(
            kCFAllocatorDefault,
            jpeg_data.as_ptr(),
            jpeg_data.len() as isize,
        );
        if cf_data.is_null() {
            return None;
        }

        let source = CGImageSourceCreateWithData(cf_data, std::ptr::null());
        CFRelease(cf_data);
        if source.is_null() {
            return None;
        }

        // Saliency works fine on a small thumbnail — the model is coarse
        // anyway. 512px keeps inference fast.
        let max_size: i32 = 512;
        let cf_num =
            CFNumberCreate(kCFAllocatorDefault, 3, &max_size as *const i32 as *const c_void);
        let keys = [
            kCGImageSourceCreateThumbnailFromImageAlways,
            kCGImageSourceThumbnailMaxPixelSize,
            kCGImageSourceCreateThumbnailWithTransform,
        ];
        let vals = [kCFBooleanTrue, cf_num, kCFBooleanTrue];
        let opts = CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr(),
            vals.as_ptr(),
            3,
            kCFTypeDictionaryKeyCallBacks.as_ptr() as _,
            kCFTypeDictionaryValueCallBacks.as_ptr() as _,
        );

        let cg_image = CGImageSourceCreateThumbnailAtIndex(source, 0, opts);
        CFRelease(opts);
        CFRelease(cf_num);
        CFRelease(source);
        if cg_image.is_null() {
            return None;
        }

        let result = run_saliency(cg_image);
        CGImageRelease(cg_image);
        result
    }
}

#[cfg(target_os = "macos")]
unsafe fn run_saliency(cg_image: *const std::ffi::c_void) -> Option<Vec<[f64; 4]>> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    let request_cls = AnyClass::get(c"VNGenerateObjectnessBasedSaliencyImageRequest")?;
    let request: *mut AnyObject = msg_send![request_cls, alloc];
    let request: *mut AnyObject = msg_send![request, init];
    if request.is_null() {
        return None;
    }

    let dict_cls = match AnyClass::get(c"NSDictionary") {
        Some(c) => c,
        None => {
            let _: () = msg_send![request, release];
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
            return None;
        }
    };
    let handler: *mut AnyObject = msg_send![handler_cls, alloc];
    let handler: *mut AnyObject =
        msg_send![handler, initWithCGImage: cg_image, options: empty_dict];
    if handler.is_null() {
        let _: () = msg_send![request, release];
        let _: () = msg_send![empty_dict, release];
        return None;
    }

    let array_cls = match AnyClass::get(c"NSArray") {
        Some(c) => c,
        None => {
            let _: () = msg_send![request, release];
            let _: () = msg_send![handler, release];
            let _: () = msg_send![empty_dict, release];
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
        return None;
    }

    // results -> NSArray<VNSaliencyImageObservation *>
    // Each observation has a .salientObjects -> NSArray<VNRectangleObservation *>
    // Each rectangle has a .boundingBox (CGRect, normalized).
    let observations: *mut AnyObject = msg_send![request, results];
    let obs_count: usize = if !observations.is_null() {
        msg_send![observations, count]
    } else {
        0
    };

    let mut boxes = Vec::new();
    for i in 0..obs_count {
        let obs: *mut AnyObject = msg_send![observations, objectAtIndex: i];
        if obs.is_null() {
            continue;
        }

        let salient_objects: *mut AnyObject = msg_send![obs, salientObjects];
        if salient_objects.is_null() {
            continue;
        }
        let n: usize = msg_send![salient_objects, count];
        for j in 0..n {
            let rect_obs: *mut AnyObject = msg_send![salient_objects, objectAtIndex: j];
            if rect_obs.is_null() {
                continue;
            }
            // boundingBox returns CGRect (4 doubles); use raw msgSend cast
            // for the same reason closed_eyes.rs does.
            let bbox = {
                use std::ffi::c_void;
                #[link(name = "objc", kind = "dylib")]
                extern "C" {
                    fn sel_registerName(name: *const u8) -> *const c_void;
                    fn objc_msgSend();
                }
                type BBoxFn =
                    unsafe extern "C" fn(*const c_void, *const c_void) -> [f64; 4];
                let sel = sel_registerName(b"boundingBox\0".as_ptr());
                let fn_ptr: BBoxFn = std::mem::transmute(objc_msgSend as *const ());
                fn_ptr(rect_obs as *const _ as *const c_void, sel)
            };
            // Sanity-check: width and height should be > 0.
            if bbox[2] > 0.0 && bbox[3] > 0.0 {
                boxes.push(bbox);
            }
        }
    }

    let _: () = msg_send![request, release];
    let _: () = msg_send![handler, release];
    let _: () = msg_send![empty_dict, release];

    Some(boxes)
}

// ── Apple framework FFI ──

#[cfg(target_os = "macos")]
#[link(name = "Vision", kind = "framework")]
extern "C" {}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: *const std::ffi::c_void;
    static kCFBooleanTrue: *const std::ffi::c_void;
    static kCFTypeDictionaryKeyCallBacks: [u8; 0];
    static kCFTypeDictionaryValueCallBacks: [u8; 0];
    fn CFDataCreate(
        a: *const std::ffi::c_void,
        b: *const u8,
        l: isize,
    ) -> *const std::ffi::c_void;
    fn CFNumberCreate(
        a: *const std::ffi::c_void,
        t: isize,
        v: *const std::ffi::c_void,
    ) -> *const std::ffi::c_void;
    fn CFDictionaryCreate(
        a: *const std::ffi::c_void,
        k: *const *const std::ffi::c_void,
        v: *const *const std::ffi::c_void,
        c: isize,
        kcb: *const std::ffi::c_void,
        vcb: *const std::ffi::c_void,
    ) -> *const std::ffi::c_void;
    fn CFRelease(cf: *const std::ffi::c_void);
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGImageRelease(i: *const std::ffi::c_void);
}

#[cfg(target_os = "macos")]
#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    static kCGImageSourceCreateThumbnailFromImageAlways: *const std::ffi::c_void;
    static kCGImageSourceThumbnailMaxPixelSize: *const std::ffi::c_void;
    static kCGImageSourceCreateThumbnailWithTransform: *const std::ffi::c_void;
    fn CGImageSourceCreateWithData(
        d: *const std::ffi::c_void,
        o: *const std::ffi::c_void,
    ) -> *const std::ffi::c_void;
    fn CGImageSourceCreateThumbnailAtIndex(
        s: *const std::ffi::c_void,
        i: usize,
        o: *const std::ffi::c_void,
    ) -> *const std::ffi::c_void;
}
