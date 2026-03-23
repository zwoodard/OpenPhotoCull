//! Prototype: Apple Vision framework for face/eye detection from Rust.
//! Uses objc2 crate for safe ObjC message dispatch (handles ARM64 ABI correctly).
//!
//! Run with: cargo run --release --bin vision_bench -- <image_path>

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This benchmark requires macOS");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() {
    use std::path::Path;
    use std::time::Instant;

    let args: Vec<String> = std::env::args().collect();
    let image_path = args.get(1).expect("Usage: vision_bench <path_to_image>");
    let path = Path::new(image_path);

    println!("=== Vision Framework Benchmark ===");
    println!("File: {}", path.display());

    let data = std::fs::read(path).unwrap();

    // Load as CGImage via ImageIO
    let cg_image = unsafe { macos_vision::load_cgimage(&data) };
    if cg_image.is_null() {
        eprintln!("Failed to load CGImage");
        std::process::exit(1);
    }

    unsafe {
        let w = macos_vision::CGImageGetWidth(cg_image);
        let h = macos_vision::CGImageGetHeight(cg_image);
        println!("Image: {}x{}", w, h);
    }
    println!();

    // Detect faces
    let t = Instant::now();
    let faces = macos_vision::detect_closed_eyes(&data, None);
    let ms = t.elapsed().as_millis();

    println!("[full-res detection]  {}ms", ms);
    println!("  Faces: {}", faces.len());
    for (i, f) in faces.iter().enumerate() {
        println!("  Face #{}: left={:.2} right={:.2} → {}",
            i + 1, f.left_eye_open, f.right_eye_open,
            if f.has_closed_eyes { "CLOSED EYES" } else { "eyes open" });
    }

    // Test at 1024px
    println!();
    let t = Instant::now();
    let faces_small = macos_vision::detect_closed_eyes(&data, Some(1024));
    let ms_small = t.elapsed().as_millis();
    println!("[1024px detection]    {}ms  faces={}", ms_small, faces_small.len());
    for (i, f) in faces_small.iter().enumerate() {
        println!("  Face #{}: left={:.2} right={:.2} → {}",
            i + 1, f.left_eye_open, f.right_eye_open,
            if f.has_closed_eyes { "CLOSED EYES" } else { "eyes open" });
    }

    // Timing stability
    println!();
    let mut times = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let _ = macos_vision::detect_closed_eyes(&data, Some(1024));
        times.push(t.elapsed().as_millis());
    }
    times.sort();
    println!("[5x 1024px]  {:?}ms  median={}ms", times, times[2]);

    // Test all photos in directory
    if let Some(parent) = path.parent() {
        println!();
        println!("=== Batch test on directory ===");
        let mut total_faces = 0;
        let mut closed_count = 0;
        let mut total_ms = 0u128;
        let mut file_count = 0;

        for entry in std::fs::read_dir(parent).unwrap().flatten() {
            let p = entry.path();
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if ext != "jpg" && ext != "jpeg" { continue; }

            let data = std::fs::read(&p).unwrap();
            let t = Instant::now();
            let faces = macos_vision::detect_closed_eyes(&data, Some(1024));
            let ms = t.elapsed().as_millis();
            total_ms += ms;
            file_count += 1;

            let any_closed = faces.iter().any(|f| f.has_closed_eyes);
            total_faces += faces.len();
            if any_closed { closed_count += 1; }

            if !faces.is_empty() {
                let name = p.file_name().unwrap().to_string_lossy();
                println!("  {:>4}ms  {} faces  {}  {}",
                    ms, faces.len(),
                    if any_closed { "CLOSED" } else { "ok    " },
                    name);
            }
        }

        println!();
        println!("  {} files, {} faces found, {} with closed eyes", file_count, total_faces, closed_count);
        println!("  Total: {}ms  ({:.1}ms/image)", total_ms, total_ms as f64 / file_count.max(1) as f64);
    }

    unsafe { macos_vision::CGImageRelease(cg_image) };
}

#[cfg(target_os = "macos")]
mod macos_vision {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;
    use std::ffi::c_void;

    // ── Framework links ──

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub static kCFAllocatorDefault: *const c_void;
        pub static kCFBooleanTrue: *const c_void;
        pub static kCFTypeDictionaryKeyCallBacks: [u8; 0];
        pub static kCFTypeDictionaryValueCallBacks: [u8; 0];
        pub fn CFDataCreate(a: *const c_void, b: *const u8, l: isize) -> *const c_void;
        pub fn CFNumberCreate(a: *const c_void, t: isize, v: *const c_void) -> *const c_void;
        pub fn CFDictionaryCreate(a: *const c_void, k: *const *const c_void, v: *const *const c_void, c: isize, kcb: *const c_void, vcb: *const c_void) -> *const c_void;
        pub fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGImageGetWidth(i: *const c_void) -> usize;
        pub fn CGImageGetHeight(i: *const c_void) -> usize;
        pub fn CGImageRelease(i: *const c_void);
    }

    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        pub static kCGImageSourceCreateThumbnailFromImageAlways: *const c_void;
        pub static kCGImageSourceThumbnailMaxPixelSize: *const c_void;
        pub static kCGImageSourceCreateThumbnailWithTransform: *const c_void;
        pub fn CGImageSourceCreateWithData(d: *const c_void, o: *const c_void) -> *const c_void;
        pub fn CGImageSourceCreateImageAtIndex(s: *const c_void, i: usize, o: *const c_void) -> *const c_void;
        pub fn CGImageSourceCreateThumbnailAtIndex(s: *const c_void, i: usize, o: *const c_void) -> *const c_void;
    }

    #[link(name = "Vision", kind = "framework")]
    extern "C" {}

    pub unsafe fn load_cgimage(jpeg_data: &[u8]) -> *const c_void {
        let cf = CFDataCreate(kCFAllocatorDefault, jpeg_data.as_ptr(), jpeg_data.len() as isize);
        if cf.is_null() { return std::ptr::null(); }
        let src = CGImageSourceCreateWithData(cf, std::ptr::null());
        CFRelease(cf);
        if src.is_null() { return std::ptr::null(); }
        let img = CGImageSourceCreateImageAtIndex(src, 0, std::ptr::null());
        CFRelease(src);
        img
    }

    unsafe fn load_cgimage_thumbnail(jpeg_data: &[u8], max_size: i32) -> *const c_void {
        let cf = CFDataCreate(kCFAllocatorDefault, jpeg_data.as_ptr(), jpeg_data.len() as isize);
        if cf.is_null() { return std::ptr::null(); }
        let src = CGImageSourceCreateWithData(cf, std::ptr::null());
        CFRelease(cf);
        if src.is_null() { return std::ptr::null(); }

        let num = CFNumberCreate(kCFAllocatorDefault, 3, &max_size as *const i32 as *const c_void);
        let keys = [kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceThumbnailMaxPixelSize, kCGImageSourceCreateThumbnailWithTransform];
        let vals = [kCFBooleanTrue, num, kCFBooleanTrue];
        let opts = CFDictionaryCreate(kCFAllocatorDefault, keys.as_ptr(), vals.as_ptr(), 3,
            kCFTypeDictionaryKeyCallBacks.as_ptr() as _, kCFTypeDictionaryValueCallBacks.as_ptr() as _);
        let img = CGImageSourceCreateThumbnailAtIndex(src, 0, opts);
        CFRelease(opts); CFRelease(num); CFRelease(src);
        img
    }

    #[derive(Debug)]
    pub struct FaceDetectionResult {
        pub left_eye_open: f64,
        pub right_eye_open: f64,
        pub has_closed_eyes: bool,
    }

    /// Detect faces and assess eye openness in a JPEG image.
    /// `max_size`: if Some, decode at this resolution first (faster for large images).
    pub fn detect_closed_eyes(jpeg_data: &[u8], max_size: Option<i32>) -> Vec<FaceDetectionResult> {
        unsafe { detect_closed_eyes_inner(jpeg_data, max_size) }
    }

    unsafe fn detect_closed_eyes_inner(jpeg_data: &[u8], max_size: Option<i32>) -> Vec<FaceDetectionResult> {
        let cg_image = if let Some(size) = max_size {
            load_cgimage_thumbnail(jpeg_data, size)
        } else {
            load_cgimage(jpeg_data)
        };
        if cg_image.is_null() { return vec![]; }

        let results = run_vision_detection(cg_image);
        CGImageRelease(cg_image);
        results
    }

    unsafe fn run_vision_detection(cg_image: *const c_void) -> Vec<FaceDetectionResult> {
        let mut results = Vec::new();

        // VNDetectFaceLandmarksRequest
        let request_cls = AnyClass::get(c"VNDetectFaceLandmarksRequest").unwrap();
        let request: *mut AnyObject = msg_send![request_cls, alloc];
        let request: *mut AnyObject = msg_send![request, init];
        if request.is_null() { return results; }

        // NSDictionary (empty options)
        let dict_cls = AnyClass::get(c"NSDictionary").unwrap();
        let empty_dict: *mut AnyObject = msg_send![dict_cls, alloc];
        let empty_dict: *mut AnyObject = msg_send![empty_dict, init];

        // VNImageRequestHandler
        let handler_cls = AnyClass::get(c"VNImageRequestHandler").unwrap();
        let handler: *mut AnyObject = msg_send![handler_cls, alloc];
        let handler: *mut AnyObject = msg_send![handler, initWithCGImage: cg_image options: empty_dict];
        if handler.is_null() {
            let _: () = msg_send![request, release];
            let _: () = msg_send![empty_dict, release];
            return results;
        }

        // NSArray with request
        let array_cls = AnyClass::get(c"NSArray").unwrap();
        let array: *mut AnyObject = msg_send![array_cls, arrayWithObject: request];

        // Perform
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = msg_send![handler, performRequests: array error: &mut error];

        if !ok {
            if !error.is_null() {
                let desc: *mut AnyObject = msg_send![error, localizedDescription];
                let cstr: *const i8 = msg_send![desc, UTF8String];
                if !cstr.is_null() {
                    eprintln!("Vision error: {:?}", std::ffi::CStr::from_ptr(cstr));
                }
            }
            let _: () = msg_send![request, release];
            let _: () = msg_send![handler, release];
            let _: () = msg_send![empty_dict, release];
            return results;
        }

        // Get observations
        let observations: *mut AnyObject = msg_send![request, results];
        if observations.is_null() {
            let _: () = msg_send![request, release];
            let _: () = msg_send![handler, release];
            let _: () = msg_send![empty_dict, release];
            return results;
        }

        let count: usize = msg_send![observations, count];

        for i in 0..count {
            let obs: *mut AnyObject = msg_send![observations, objectAtIndex: i];
            if obs.is_null() { continue; }

            let landmarks: *mut AnyObject = msg_send![obs, landmarks];
            if landmarks.is_null() { continue; }

            let left = eye_openness_from_landmarks(landmarks, true);
            let right = eye_openness_from_landmarks(landmarks, false);

            results.push(FaceDetectionResult {
                left_eye_open: left,
                right_eye_open: right,
                has_closed_eyes: left < 0.3 || right < 0.3,
            });
        }

        let _: () = msg_send![request, release];
        let _: () = msg_send![handler, release];
        let _: () = msg_send![empty_dict, release];
        results
    }

    unsafe fn eye_openness_from_landmarks(landmarks: *mut AnyObject, is_left: bool) -> f64 {
        let region: *mut AnyObject = if is_left {
            msg_send![landmarks, leftEye]
        } else {
            msg_send![landmarks, rightEye]
        };
        if region.is_null() { return 1.0; }

        let npoints: usize = msg_send![region, pointCount];
        if npoints < 4 { return 1.0; }

        // Read eye contour points
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;

        for i in 0..npoints {
            // pointAtIndex: returns simd_float2 (two f32s packed) on newer APIs,
            // or CGPoint (two f64s) depending on the API version.
            // VNFaceLandmarkRegion2D.pointAtIndex: returns vector_float2 (simd)
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

        // Eye aspect ratio → openness (0 = closed, 1 = wide open)
        let aspect = height / width;
        ((aspect - 0.05) / 0.30).clamp(0.0, 1.0)
    }
}
