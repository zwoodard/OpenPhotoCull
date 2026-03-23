#[cfg(target_os = "macos")]
mod apple_imageio;

use image::DynamicImage;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const THUMB_SIZE: u32 = 300;
const THUMB_QUALITY: u8 = 80;
const ANALYSIS_SIZE: u32 = 1024;

/// File size threshold (bytes) above which we use Apple Image I/O for JPEG decode.
/// Image I/O has framework overhead (~20-40ms) that makes it slower for small files.
/// For files > 1MB, the hardware decoder advantage more than compensates.
const IMAGEIO_SIZE_THRESHOLD: u64 = 1_000_000;

pub fn cache_key(path: &str, modified_at: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hasher.update(modified_at.to_le_bytes());
    hex::encode(hasher.finalize())
}

pub fn thumbnail_path(cache_dir: &Path, key: &str) -> PathBuf {
    // v2 suffix invalidates pre-orientation-fix thumbnails
    cache_dir.join(format!("{}_v2.jpg", key))
}

pub struct ProcessedImage {
    pub thumbnail_path: Option<String>,
    /// Original full-resolution dimensions.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Analysis-sized image (1024px max). Caller should run analyzers then drop.
    pub analysis_image: Option<DynamicImage>,
}

/// Decode + resize to analysis size + generate thumbnail.
///
/// Decode strategy (in priority order):
/// 1. macOS + JPEG > 1MB → Apple Image I/O (hardware decoder, ~65ms for 16MB file)
/// 2. JPEG → turbojpeg with DCT scaling (SIMD, ~175ms for 16MB file)
/// 3. Other formats → image crate + SIMD resize
pub fn process_image(
    image_path: &Path,
    cache_dir: &Path,
    cache_key: &str,
) -> ProcessedImage {
    let thumb_path = thumbnail_path(cache_dir, cache_key);
    let thumb_cached = thumb_path.exists();

    let fail = |w: Option<u32>, h: Option<u32>| ProcessedImage {
        thumbnail_path: if thumb_cached {
            Some(thumb_path.to_string_lossy().to_string())
        } else {
            None
        },
        width: w,
        height: h,
        analysis_image: None,
    };

    let ext = image_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let is_jpeg = ext == "jpg" || ext == "jpeg";
    let file_size = std::fs::metadata(image_path).map(|m| m.len()).unwrap_or(0);

    // Try Apple Image I/O for large JPEGs on macOS.
    // This decodes + resizes to ANALYSIS_SIZE in a single hardware-accelerated call.
    #[cfg(target_os = "macos")]
    if is_jpeg && file_size > IMAGEIO_SIZE_THRESHOLD {
        if let Some((img, orig_w, orig_h)) =
            apple_imageio::decode_jpeg_resized(image_path, ANALYSIS_SIZE)
        {
            // Get original dimensions — Image I/O may not return them from properties,
            // so fall back to EXIF or turbojpeg header.
            let (w, h) = if orig_w > 0 && orig_h > 0 {
                (orig_w, orig_h)
            } else {
                jpeg_dimensions(image_path).unwrap_or((img.width(), img.height()))
            };

            if !thumb_cached {
                write_thumbnail(&img, &thumb_path);
            }

            return ProcessedImage {
                thumbnail_path: Some(thumb_path.to_string_lossy().to_string()),
                width: Some(w),
                height: Some(h),
                analysis_image: Some(img),
            };
        }
        // Fall through to CPU path on failure
    }

    // CPU path: decode image
    let decoded = load_image(image_path);

    let Some(img) = decoded else {
        return fail(None, None);
    };

    // Get original dimensions (for DCT-scaled JPEGs, decoded dims != original dims)
    let (w, h) = if is_jpeg {
        jpeg_dimensions(image_path).unwrap_or((img.width(), img.height()))
    } else {
        (img.width(), img.height())
    };

    // Read EXIF orientation for manual rotation (turbojpeg/image crate don't auto-rotate)
    let orientation = crate::index::metadata::read_orientation(image_path).unwrap_or(1);

    // Resize to analysis size if needed
    let (decoded_w, decoded_h) = (img.width(), img.height());
    let analysis = if decoded_w <= ANALYSIS_SIZE && decoded_h <= ANALYSIS_SIZE {
        img
    } else {
        match fast_resize_owned(img, ANALYSIS_SIZE) {
            Some(small) => small,
            None => return fail(Some(w), Some(h)),
        }
    };

    // Apply EXIF orientation (rotate/flip as needed)
    let analysis = crate::index::metadata::apply_orientation(analysis, orientation);

    if !thumb_cached {
        write_thumbnail(&analysis, &thumb_path);
    }

    // Report dimensions post-rotation (orientation 5-8 swap width/height)
    let (final_w, final_h) = if orientation >= 5 && orientation <= 8 {
        (h, w)
    } else {
        (w, h)
    };

    ProcessedImage {
        thumbnail_path: Some(thumb_path.to_string_lossy().to_string()),
        width: Some(final_w),
        height: Some(final_h),
        analysis_image: Some(analysis),
    }
}

/// Read JPEG dimensions from header without decoding.
fn jpeg_dimensions(path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(path).ok()?;
    let mut dec = turbojpeg::Decompressor::new().ok()?;
    let header = dec.read_header(&data).ok()?;
    Some((header.width as u32, header.height as u32))
}

/// Write a JPEG thumbnail from an already-small image.
fn write_thumbnail(img: &DynamicImage, path: &Path) {
    let thumb = if img.width() <= THUMB_SIZE && img.height() <= THUMB_SIZE {
        img.to_rgb8()
    } else {
        let scale = THUMB_SIZE as f64 / std::cmp::max(img.width(), img.height()) as f64;
        let tw = std::cmp::max(1, (img.width() as f64 * scale) as u32);
        let th = std::cmp::max(1, (img.height() as f64 * scale) as u32);
        img.resize_exact(tw, th, image::imageops::FilterType::Triangle)
            .to_rgb8()
    };

    let mut buf = Vec::new();
    if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, THUMB_QUALITY)
        .encode(&thumb, thumb.width(), thumb.height(), image::ExtendedColorType::Rgb8)
        .is_ok()
    {
        std::fs::write(path, &buf).ok();
    }
}

/// SIMD-accelerated resize that CONSUMES the input image.
fn fast_resize_owned(img: DynamicImage, max_dim: u32) -> Option<DynamicImage> {
    use fast_image_resize as fr;

    let (src_w, src_h) = (img.width(), img.height());
    if src_w == 0 || src_h == 0 {
        return None;
    }

    let scale = max_dim as f64 / std::cmp::max(src_w, src_h) as f64;
    let dst_w = std::cmp::max(1, (src_w as f64 * scale) as u32);
    let dst_h = std::cmp::max(1, (src_h as f64 * scale) as u32);

    let rgba = img.into_rgba8();

    let src_image = fr::images::Image::from_vec_u8(
        src_w,
        src_h,
        rgba.into_raw(),
        fr::PixelType::U8x4,
    )
    .ok()?;

    let mut dst_image = fr::images::Image::new(dst_w, dst_h, fr::PixelType::U8x4);

    let mut resizer = fr::Resizer::new();
    resizer
        .resize(
            &src_image,
            &mut dst_image,
            Some(
                &fr::ResizeOptions::new()
                    .resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Bilinear)),
            ),
        )
        .ok()?;

    drop(src_image);

    let buf = dst_image.into_vec();
    let rgba_img = image::RgbaImage::from_raw(dst_w, dst_h, buf)?;
    Some(DynamicImage::ImageRgba8(rgba_img))
}

fn load_image(path: &Path) -> Option<DynamicImage> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "jpg" | "jpeg" => load_jpeg_turbo(path).or_else(|| image::open(path).ok()),
        "heic" | "heif" => {
            tracing::warn!("HEIC not yet supported: {:?}", path);
            None
        }
        _ => image::open(path).ok(),
    }
}

/// Decode JPEG using libjpeg-turbo with DCT scaling.
fn load_jpeg_turbo(path: &Path) -> Option<DynamicImage> {
    let data = std::fs::read(path).ok()?;
    let mut decompressor = turbojpeg::Decompressor::new().ok()?;

    let header = decompressor.read_header(&data).ok()?;
    let max_dim = std::cmp::max(header.width, header.height);

    let scale = if max_dim / 8 >= ANALYSIS_SIZE as usize {
        turbojpeg::ScalingFactor::ONE_EIGHTH
    } else if max_dim / 4 >= ANALYSIS_SIZE as usize {
        turbojpeg::ScalingFactor::ONE_QUARTER
    } else if max_dim / 2 >= ANALYSIS_SIZE as usize {
        turbojpeg::ScalingFactor::ONE_HALF
    } else {
        turbojpeg::ScalingFactor::ONE
    };

    decompressor.set_scaling_factor(scale).ok()?;

    let scaled_header = decompressor.read_header(&data).ok()?;
    let (w, h) = (scaled_header.width, scaled_header.height);

    let mut buf = vec![0u8; w * h * 3];
    decompressor
        .decompress(
            &data,
            turbojpeg::Image {
                pixels: buf.as_mut_slice(),
                width: w,
                height: h,
                pitch: w * 3,
                format: turbojpeg::PixelFormat::RGB,
            },
        )
        .ok()?;

    let rgb_img = image::RgbImage::from_raw(w as u32, h as u32, buf)?;
    Some(DynamicImage::ImageRgb8(rgb_img))
}

/// Fallback: load + resize for analysis only.
pub fn load_for_analysis(path: &Path) -> Option<DynamicImage> {
    let img = load_image(path)?;
    let (w, h) = (img.width(), img.height());
    if w <= ANALYSIS_SIZE && h <= ANALYSIS_SIZE {
        Some(img)
    } else {
        fast_resize_owned(img, ANALYSIS_SIZE)
    }
}
