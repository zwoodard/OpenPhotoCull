//! Benchmark comparing CPU vs Apple framework image processing paths.
//! Tests: turbojpeg vs Image I/O (CGImageSource) for JPEG decoding,
//! and fast_image_resize vs Core Graphics for resizing.
//!
//! Run with: cargo run --release --bin gpu_bench -- <jpeg_path>

use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let jpeg_path = args.get(1).expect("Usage: gpu_bench <path_to_jpeg>");
    let path = Path::new(jpeg_path);

    println!("=== Apple Framework vs CPU Benchmark ===");
    println!("File: {}", path.display());
    let file_size = std::fs::metadata(path).unwrap().len();
    println!("Size: {:.1} MB", file_size as f64 / 1024.0 / 1024.0);
    println!();

    // --- CPU path: turbojpeg ---
    let data = std::fs::read(path).unwrap();

    // turbojpeg full decode
    {
        let t = Instant::now();
        let mut dec = turbojpeg::Decompressor::new().unwrap();
        let hdr = dec.read_header(&data).unwrap();
        let mut buf = vec![0u8; hdr.width * hdr.height * 3];
        dec.decompress(&data, turbojpeg::Image {
            pixels: buf.as_mut_slice(),
            width: hdr.width, height: hdr.height,
            pitch: hdr.width * 3,
            format: turbojpeg::PixelFormat::RGB,
        }).unwrap();
        println!("[turbojpeg full]   {}ms  ({}x{})", t.elapsed().as_millis(), hdr.width, hdr.height);
    }

    // turbojpeg 1/4 decode
    {
        let t = Instant::now();
        let mut dec = turbojpeg::Decompressor::new().unwrap();
        dec.set_scaling_factor(turbojpeg::ScalingFactor::ONE_QUARTER).unwrap();
        let hdr = dec.read_header(&data).unwrap();
        let mut buf = vec![0u8; hdr.width * hdr.height * 3];
        dec.decompress(&data, turbojpeg::Image {
            pixels: buf.as_mut_slice(),
            width: hdr.width, height: hdr.height,
            pitch: hdr.width * 3,
            format: turbojpeg::PixelFormat::RGB,
        }).unwrap();
        println!("[turbojpeg 1/4]    {}ms  ({}x{})", t.elapsed().as_millis(), hdr.width, hdr.height);
    }

    // turbojpeg 1/8 decode
    {
        let t = Instant::now();
        let mut dec = turbojpeg::Decompressor::new().unwrap();
        dec.set_scaling_factor(turbojpeg::ScalingFactor::ONE_EIGHTH).unwrap();
        let hdr = dec.read_header(&data).unwrap();
        let mut buf = vec![0u8; hdr.width * hdr.height * 3];
        dec.decompress(&data, turbojpeg::Image {
            pixels: buf.as_mut_slice(),
            width: hdr.width, height: hdr.height,
            pitch: hdr.width * 3,
            format: turbojpeg::PixelFormat::RGB,
        }).unwrap();
        println!("[turbojpeg 1/8]    {}ms  ({}x{})", t.elapsed().as_millis(), hdr.width, hdr.height);
    }

    println!();

    // --- Apple path: CGImageSource (Image I/O) ---
    // Image I/O can decode JPEGs with thumbnail generation at a target size,
    // which may use hardware acceleration on Apple Silicon.
    // We call it via raw FFI since the objc2 bindings would add deps.
    unsafe {
        apple_imageio_bench(path, &data);
    }
}

unsafe fn apple_imageio_bench(path: &Path, data: &[u8]) {
    use std::ffi::c_void;

    // Link against ImageIO and CoreGraphics frameworks
    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        fn CGImageSourceCreateWithData(
            data: *const c_void, // CFDataRef
            options: *const c_void, // CFDictionaryRef
        ) -> *const c_void; // CGImageSourceRef

        fn CGImageSourceCreateThumbnailAtIndex(
            isrc: *const c_void,
            index: usize,
            options: *const c_void,
        ) -> *const c_void; // CGImageRef

        fn CGImageSourceCreateImageAtIndex(
            isrc: *const c_void,
            index: usize,
            options: *const c_void,
        ) -> *const c_void; // CGImageRef
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGImageGetWidth(image: *const c_void) -> usize;
        fn CGImageGetHeight(image: *const c_void) -> usize;
        fn CGImageGetBytesPerRow(image: *const c_void) -> usize;
        fn CGImageGetDataProvider(image: *const c_void) -> *const c_void;
        fn CGDataProviderCopyData(provider: *const c_void) -> *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDataCreate(
            allocator: *const c_void,
            bytes: *const u8,
            length: isize,
        ) -> *const c_void;

        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;

        fn CFNumberCreate(
            allocator: *const c_void,
            the_type: isize,
            value_ptr: *const c_void,
        ) -> *const c_void;

        fn CFDataGetLength(data: *const c_void) -> isize;
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;

        fn CFRelease(cf: *const c_void);

        static kCFAllocatorDefault: *const c_void;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        static kCFBooleanTrue: *const c_void;
    }

    // ImageIO option keys
    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        static kCGImageSourceCreateThumbnailFromImageAlways: *const c_void;
        static kCGImageSourceThumbnailMaxPixelSize: *const c_void;
        static kCGImageSourceCreateThumbnailWithTransform: *const c_void;
        static kCGImageSourceShouldCache: *const c_void;
    }

    // Create CFData from JPEG bytes
    let cf_data = CFDataCreate(kCFAllocatorDefault, data.as_ptr(), data.len() as isize);
    if cf_data.is_null() { println!("  Failed to create CFData"); return; }

    let source = CGImageSourceCreateWithData(cf_data, std::ptr::null());
    if source.is_null() { println!("  Failed to create image source"); CFRelease(cf_data); return; }

    // Full decode via ImageIO
    {
        let t = Instant::now();
        let image = CGImageSourceCreateImageAtIndex(source, 0, std::ptr::null());
        if !image.is_null() {
            let w = CGImageGetWidth(image);
            let h = CGImageGetHeight(image);
            // Force pixel access to ensure decode actually happens
            let provider = CGImageGetDataProvider(image);
            let pixel_data = CGDataProviderCopyData(provider);
            let decode_ms = t.elapsed().as_millis();
            let data_len = if !pixel_data.is_null() { CFDataGetLength(pixel_data) } else { 0 };
            println!("[ImageIO full]     {}ms  ({}x{}, {} bytes)", decode_ms, w, h, data_len);
            if !pixel_data.is_null() { CFRelease(pixel_data); }
            CFRelease(image);
        } else {
            println!("[ImageIO full]     FAILED");
        }
    }

    // Thumbnail decode at 1024px via ImageIO (hardware-accelerated path)
    {
        let max_size: i32 = 1024;
        let cf_number = CFNumberCreate(
            kCFAllocatorDefault,
            9, // kCFNumberSInt32Type
            &max_size as *const i32 as *const c_void,
        );

        let keys = [
            kCGImageSourceCreateThumbnailFromImageAlways,
            kCGImageSourceThumbnailMaxPixelSize,
            kCGImageSourceCreateThumbnailWithTransform,
            kCGImageSourceShouldCache,
        ];
        let values = [
            kCFBooleanTrue,
            cf_number,
            kCFBooleanTrue,
            kCFBooleanTrue as *const c_void,
        ];

        let options = CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr(),
            values.as_ptr(),
            4,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );

        let t = Instant::now();
        let thumb = CGImageSourceCreateThumbnailAtIndex(source, 0, options);
        if !thumb.is_null() {
            let w = CGImageGetWidth(thumb);
            let h = CGImageGetHeight(thumb);
            // Force pixel access
            let provider = CGImageGetDataProvider(thumb);
            let pixel_data = CGDataProviderCopyData(provider);
            let decode_ms = t.elapsed().as_millis();
            println!("[ImageIO 1024px]   {}ms  ({}x{})", decode_ms, w, h);
            if !pixel_data.is_null() { CFRelease(pixel_data); }
            CFRelease(thumb);
        } else {
            println!("[ImageIO 1024px]   FAILED");
        }

        CFRelease(options);
        CFRelease(cf_number);
    }

    // Thumbnail at 300px (for thumbnail generation comparison)
    {
        let max_size: i32 = 300;
        let cf_number = CFNumberCreate(
            kCFAllocatorDefault,
            9,
            &max_size as *const i32 as *const c_void,
        );

        let keys = [
            kCGImageSourceCreateThumbnailFromImageAlways,
            kCGImageSourceThumbnailMaxPixelSize,
            kCGImageSourceCreateThumbnailWithTransform,
        ];
        let values = [
            kCFBooleanTrue,
            cf_number,
            kCFBooleanTrue,
        ];

        let options = CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr(),
            values.as_ptr(),
            3,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );

        let t = Instant::now();
        let thumb = CGImageSourceCreateThumbnailAtIndex(source, 0, options);
        if !thumb.is_null() {
            let w = CGImageGetWidth(thumb);
            let h = CGImageGetHeight(thumb);
            let provider = CGImageGetDataProvider(thumb);
            let pixel_data = CGDataProviderCopyData(provider);
            let decode_ms = t.elapsed().as_millis();
            println!("[ImageIO 300px]    {}ms  ({}x{})", decode_ms, w, h);
            if !pixel_data.is_null() { CFRelease(pixel_data); }
            CFRelease(thumb);
        } else {
            println!("[ImageIO 300px]    FAILED");
        }

        CFRelease(options);
        CFRelease(cf_number);
    }

    CFRelease(source);
    CFRelease(cf_data);
}
