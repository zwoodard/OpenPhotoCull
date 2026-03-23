//! Closed eye detection via Apple Vision framework (macOS only).
//!
//! Uses VNDetectFaceLandmarksRequest to find faces, then measures the
//! eye aspect ratio (height/width of eye contour) to estimate openness.
//!
//! Performance: ~28ms per image at 1024px resolution (after model warm-up).

use crate::index::store::ClosedEyesResult;

#[cfg(target_os = "macos")]
use crate::index::store::FaceEyeResult;

/// Detect faces and assess eye openness from raw JPEG data.
/// Returns None on non-macOS platforms or if detection fails.
///
/// The `analysis_image` parameter is a DynamicImage at analysis resolution.
/// On macOS we ignore it and use the raw JPEG data directly with Image I/O
/// (faster path — avoids pixel format conversion).
#[cfg(target_os = "macos")]
pub fn detect(jpeg_data: Option<&[u8]>, analysis_image: &image::DynamicImage) -> Option<ClosedEyesResult> {
    // Prefer raw JPEG path (Vision can decode via ImageIO natively).
    // Fall back to pixel-buffer path if no JPEG data available.
    if let Some(data) = jpeg_data {
        detect_from_jpeg(data)
    } else {
        detect_from_pixels(analysis_image)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn detect(_jpeg_data: Option<&[u8]>, _analysis_image: &image::DynamicImage) -> Option<ClosedEyesResult> {
    None // Not available on this platform
}

#[cfg(target_os = "macos")]
fn detect_from_jpeg(jpeg_data: &[u8]) -> Option<ClosedEyesResult> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::c_void;

    unsafe {
        // Load as CGImage thumbnail at 1024px (fast path via hardware decoder)
        let cf_data = CFDataCreate(
            kCFAllocatorDefault,
            jpeg_data.as_ptr(),
            jpeg_data.len() as isize,
        );
        if cf_data.is_null() { return None; }

        let source = CGImageSourceCreateWithData(cf_data, std::ptr::null());
        CFRelease(cf_data);
        if source.is_null() { return None; }

        // Create thumbnail at 1024px for faster detection
        let max_size: i32 = 1024;
        let cf_num = CFNumberCreate(kCFAllocatorDefault, 3, &max_size as *const i32 as *const c_void);
        let keys = [
            kCGImageSourceCreateThumbnailFromImageAlways,
            kCGImageSourceThumbnailMaxPixelSize,
            kCGImageSourceCreateThumbnailWithTransform,
        ];
        let vals = [kCFBooleanTrue, cf_num, kCFBooleanTrue];
        let opts = CFDictionaryCreate(
            kCFAllocatorDefault, keys.as_ptr(), vals.as_ptr(), 3,
            kCFTypeDictionaryKeyCallBacks.as_ptr() as _, kCFTypeDictionaryValueCallBacks.as_ptr() as _,
        );

        let cg_image = CGImageSourceCreateThumbnailAtIndex(source, 0, opts);
        CFRelease(opts); CFRelease(cf_num); CFRelease(source);
        if cg_image.is_null() { return None; }

        let result = run_face_detection(cg_image);
        CGImageRelease(cg_image);
        result
    }
}

#[cfg(target_os = "macos")]
fn detect_from_pixels(image: &image::DynamicImage) -> Option<ClosedEyesResult> {
    // For non-JPEG images, we'd need to create a CGImage from pixel data.
    // This is more complex — for now, skip face detection on non-JPEGs.
    let _ = image;
    None
}

#[cfg(target_os = "macos")]
unsafe fn run_face_detection(cg_image: *const std::ffi::c_void) -> Option<ClosedEyesResult> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    let request_cls = AnyClass::get(c"VNDetectFaceLandmarksRequest")?;
    let request: *mut AnyObject = msg_send![request_cls, alloc];
    let request: *mut AnyObject = msg_send![request, init];
    if request.is_null() { return None; }

    let dict_cls = AnyClass::get(c"NSDictionary")?;
    let empty_dict: *mut AnyObject = msg_send![dict_cls, alloc];
    let empty_dict: *mut AnyObject = msg_send![empty_dict, init];

    let handler_cls = AnyClass::get(c"VNImageRequestHandler")?;
    let handler: *mut AnyObject = msg_send![handler_cls, alloc];
    let handler: *mut AnyObject = msg_send![handler, initWithCGImage: cg_image, options: empty_dict];
    if handler.is_null() {
        let _: () = msg_send![request, release];
        let _: () = msg_send![empty_dict, release];
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
        return None;
    }

    let observations: *mut AnyObject = msg_send![request, results];
    let count: usize = if !observations.is_null() {
        msg_send![observations, count]
    } else {
        0
    };

    let mut faces = Vec::new();
    let mut any_closed = false;

    for i in 0..count {
        let obs: *mut AnyObject = msg_send![observations, objectAtIndex: i];
        if obs.is_null() { continue; }

        // Extract bounding box via raw objc_msgSend.
        // boundingBox returns CGRect (4 doubles) which is returned in
        // registers on ARM64. We use a raw function pointer cast.
        let bounding_box = {
            use std::ffi::c_void;
            #[link(name = "objc", kind = "dylib")]
            extern "C" {
                fn sel_registerName(name: *const u8) -> *const c_void;
                fn objc_msgSend();
            }
            type BBoxFn = unsafe extern "C" fn(*const c_void, *const c_void) -> [f64; 4];
            let sel = sel_registerName(b"boundingBox\0".as_ptr());
            let fn_ptr: BBoxFn = std::mem::transmute(objc_msgSend as *const ());
            let raw = fn_ptr(obs as *const _ as *const c_void, sel);
            Some([raw[0], raw[1], raw[2], raw[3]])
        };

        let landmarks: *mut AnyObject = msg_send![obs, landmarks];
        let (left, right) = if !landmarks.is_null() {
            (eye_openness(landmarks, true), eye_openness(landmarks, false))
        } else {
            (1.0, 1.0)
        };
        let closed = left < 0.3 || right < 0.3;
        if closed { any_closed = true; }

        faces.push(FaceEyeResult {
            left_eye_open: left,
            right_eye_open: right,
            eyes_closed: closed,
            bounding_box,
        });
    }

    let _: () = msg_send![request, release];
    let _: () = msg_send![handler, release];
    let _: () = msg_send![empty_dict, release];

    Some(ClosedEyesResult {
        face_count: faces.len() as u32,
        faces,
        has_closed_eyes: any_closed,
    })
}

#[cfg(target_os = "macos")]
unsafe fn eye_openness(landmarks: *mut objc2::runtime::AnyObject, is_left: bool) -> f64 {
    use objc2::msg_send;

    let region: *mut objc2::runtime::AnyObject = if is_left {
        msg_send![landmarks, leftEye]
    } else {
        msg_send![landmarks, rightEye]
    };
    if region.is_null() { return 1.0; }

    let npoints: usize = msg_send![region, pointCount];
    if npoints < 4 { return 1.0; }

    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;

    for i in 0..npoints {
        let p: [f32; 2] = msg_send![region, pointAtIndex: i];
        let (x, y) = (p[0] as f64, p[1] as f64);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }

    let width = max_x - min_x;
    let height = max_y - min_y;
    if width < 0.001 { return 1.0; }

    let aspect = height / width;
    ((aspect - 0.05) / 0.30).clamp(0.0, 1.0)
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
    fn CFDataCreate(a: *const std::ffi::c_void, b: *const u8, l: isize) -> *const std::ffi::c_void;
    fn CFNumberCreate(a: *const std::ffi::c_void, t: isize, v: *const std::ffi::c_void) -> *const std::ffi::c_void;
    fn CFDictionaryCreate(a: *const std::ffi::c_void, k: *const *const std::ffi::c_void, v: *const *const std::ffi::c_void, c: isize, kcb: *const std::ffi::c_void, vcb: *const std::ffi::c_void) -> *const std::ffi::c_void;
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
    fn CGImageSourceCreateWithData(d: *const std::ffi::c_void, o: *const std::ffi::c_void) -> *const std::ffi::c_void;
    fn CGImageSourceCreateThumbnailAtIndex(s: *const std::ffi::c_void, i: usize, o: *const std::ffi::c_void) -> *const std::ffi::c_void;
}
