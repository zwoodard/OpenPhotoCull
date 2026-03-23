//! CLI benchmark harness for the image processing pipeline.
//! Run with: cargo run --release --bin bench -- <folder_path>

use image_hasher::{HashAlg, HasherConfig};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

// Pull in the library crate
use photo_scrub_lib::index::discovery;
use photo_scrub_lib::index::metadata;
use photo_scrub_lib::index::store::{AnalysisResults, BlurResult, ExposureResult};
use photo_scrub_lib::thumbnail;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let folder = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("test-photos/sampled2");
    let root = PathBuf::from(folder);

    if !root.is_dir() {
        eprintln!("Error: {} is not a directory", folder);
        std::process::exit(1);
    }

    let num_threads: usize = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    println!("=== Photo Scrub Benchmark ===");
    println!("Folder: {}", root.display());
    println!("Threads: {}", num_threads);
    println!();

    // Phase 1: Discovery
    let t = Instant::now();
    let discovered = discovery::discover_images(&root);
    let discovery_ms = t.elapsed().as_millis();
    let total = discovered.len();
    println!(
        "[discovery]   {} images found in {}ms",
        total, discovery_ms
    );

    let total_bytes: u64 = discovered.iter().map(|d| d.file_size).sum();
    println!(
        "              Total size: {:.1} MB",
        total_bytes as f64 / 1024.0 / 1024.0
    );
    println!();

    // Phase 2: EXIF extraction (header only, should be fast)
    let t = Instant::now();
    let _exif_results: Vec<_> = discovered
        .par_iter()
        .map(|d| metadata::extract_metadata(&d.path))
        .collect();
    let exif_ms = t.elapsed().as_millis();
    println!(
        "[exif]        {}ms ({:.1} imgs/sec)",
        exif_ms,
        total as f64 / (exif_ms as f64 / 1000.0)
    );

    // Phase 3: Full pipeline — decode + resize + thumbnail + blur + exposure + hash
    let cache_dir = std::env::temp_dir().join("photoscrub-bench-thumbs");
    std::fs::create_dir_all(&cache_dir).ok();
    // Clear thumbnail cache to force regeneration
    for entry in std::fs::read_dir(&cache_dir).into_iter().flatten() {
        if let Ok(entry) = entry {
            std::fs::remove_file(entry.path()).ok();
        }
    }

    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::DoubleGradient)
        .hash_size(16, 16)
        .to_hasher();

    let counter = Arc::new(AtomicUsize::new(0));
    let per_image_times: Arc<std::sync::Mutex<Vec<(String, u128)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .unwrap();

    let t = Instant::now();
    let results: Vec<Option<AnalysisResults>> = pool.install(|| {
        discovered
            .par_iter()
            .map(|disc| {
                let img_start = Instant::now();
                let path_str = disc.path.to_string_lossy().to_string();
                let file_name = disc
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let id = thumbnail::cache_key(&path_str, disc.modified_at);
                let processed = thumbnail::process_image(&disc.path, &cache_dir, &id);

                let analysis = processed.analysis_image.as_ref().map(|analysis_img| {
                    let blur = compute_blur(analysis_img);
                    let exposure = compute_exposure(analysis_img);
                    let _hash = hasher.hash_image(analysis_img);

                    AnalysisResults {
                        blur: Some(blur),
                        exposure: Some(exposure),
                        duplicate_group_id: None,
                        scene_group_id: None,
                        closed_eyes: None,
                        subject_focus: None,
                        faces: None,
                    }
                });

                let img_ms = img_start.elapsed().as_millis();
                let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                per_image_times
                    .lock()
                    .unwrap()
                    .push((file_name.clone(), img_ms));

                if done % 10 == 0 || done == total {
                    let elapsed = t.elapsed().as_millis();
                    let rate = done as f64 / (elapsed as f64 / 1000.0);
                    eprint!(
                        "\r  [{}/{}] {:.1} imgs/sec  ({})     ",
                        done, total, rate, file_name
                    );
                }

                analysis
            })
            .collect()
    });
    let pipeline_ms = t.elapsed().as_millis();
    eprintln!();

    println!();
    println!(
        "[pipeline]    {}ms ({:.1} imgs/sec)",
        pipeline_ms,
        total as f64 / (pipeline_ms as f64 / 1000.0)
    );

    // Phase 4: With cached thumbnails (second run)
    counter.store(0, Ordering::Relaxed);
    let t = Instant::now();
    let _results2: Vec<Option<AnalysisResults>> = pool.install(|| {
        discovered
            .par_iter()
            .map(|disc| {
                let path_str = disc.path.to_string_lossy().to_string();
                let id = thumbnail::cache_key(&path_str, disc.modified_at);
                let processed = thumbnail::process_image(&disc.path, &cache_dir, &id);

                processed.analysis_image.as_ref().map(|analysis_img| {
                    let blur = compute_blur(analysis_img);
                    let exposure = compute_exposure(analysis_img);
                    let _hash = hasher.hash_image(analysis_img);

                    AnalysisResults {
                        blur: Some(blur),
                        exposure: Some(exposure),
                        duplicate_group_id: None,
                        scene_group_id: None,
                        closed_eyes: None,
                        subject_focus: None,
                        faces: None,
                    }
                })
            })
            .collect()
    });
    let cached_ms = t.elapsed().as_millis();
    println!(
        "[cached run]  {}ms ({:.1} imgs/sec)",
        cached_ms,
        total as f64 / (cached_ms as f64 / 1000.0)
    );

    // Stats
    println!();
    println!("=== Per-image breakdown (slowest 10) ===");
    let mut times = per_image_times.lock().unwrap().clone();
    times.sort_by(|a, b| b.1.cmp(&a.1));
    for (name, ms) in times.iter().take(10) {
        println!("  {:>6}ms  {}", ms, name);
    }

    println!();
    println!("=== Per-image breakdown (fastest 10) ===");
    for (name, ms) in times.iter().rev().take(10) {
        println!("  {:>6}ms  {}", ms, name);
    }

    // Histogram
    println!();
    println!("=== Time distribution ===");
    let mut buckets = [0u32; 10]; // 0-100, 100-200, ..., 900+
    for (_, ms) in &times {
        let bucket = std::cmp::min((*ms / 100) as usize, 9);
        buckets[bucket] += 1;
    }
    let max_count = *buckets.iter().max().unwrap_or(&1);
    for (i, &count) in buckets.iter().enumerate() {
        let label = if i == 9 {
            format!("  900+ ms")
        } else {
            format!("{:>3}-{:<3}ms", i * 100, (i + 1) * 100)
        };
        let bar_len = if max_count > 0 {
            (count as f64 / max_count as f64 * 40.0) as usize
        } else {
            0
        };
        println!("  {} {:>3} {}", label, count, "#".repeat(bar_len));
    }

    // Analysis summary
    let blurry = results
        .iter()
        .flatten()
        .filter(|r| r.blur.as_ref().is_some_and(|b| b.is_blurry))
        .count();
    let exposure_issues = results
        .iter()
        .flatten()
        .filter(|r| {
            r.exposure
                .as_ref()
                .is_some_and(|e| e.verdict != "Normal")
        })
        .count();

    println!();
    println!("=== Analysis results ===");
    println!("  Blurry:          {}/{}", blurry, total);
    println!("  Exposure issues: {}/{}", exposure_issues, total);
    println!(
        "  Total time:      {}ms (discovery + exif + pipeline)",
        discovery_ms + exif_ms + pipeline_ms
    );

    // Phase 5: Detailed per-step breakdown of a large image
    println!();
    println!("=== Detailed breakdown (largest JPEG) ===");
    let biggest_jpeg = discovered
        .iter()
        .filter(|d| {
            d.path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
                .unwrap_or(false)
        })
        .max_by_key(|d| d.file_size);

    if let Some(big) = biggest_jpeg {
        let name = big.path.file_name().unwrap().to_string_lossy();
        println!("  File: {} ({:.1} MB)", name, big.file_size as f64 / 1024.0 / 1024.0);

        // Decode via image crate (zune-jpeg, pure Rust)
        let t = Instant::now();
        let img = image::open(&big.path).unwrap();
        let decode_ms = t.elapsed().as_millis();
        println!("  decode (zune):  {}ms  ({}x{})", decode_ms, img.width(), img.height());

        // Decode via turbojpeg (libjpeg-turbo, SIMD/NEON) — full res
        let data = std::fs::read(&big.path).unwrap();
        {
            let t = Instant::now();
            let mut dec = turbojpeg::Decompressor::new().unwrap();
            let hdr = dec.read_header(&data).unwrap();
            let mut rgb_buf = vec![0u8; hdr.width * hdr.height * 3];
            dec.decompress(&data, turbojpeg::Image {
                pixels: rgb_buf.as_mut_slice(),
                width: hdr.width,
                height: hdr.height,
                pitch: hdr.width * 3,
                format: turbojpeg::PixelFormat::RGB,
            }).unwrap();
            let turbo_ms = t.elapsed().as_millis();
            println!("  decode (turbo full): {}ms  ({}x{})  [{:.1}x vs zune]",
                turbo_ms, hdr.width, hdr.height,
                decode_ms as f64 / turbo_ms.max(1) as f64);
        }

        // Decode via turbojpeg with DCT 1/4 scaling
        {
            let t = Instant::now();
            let mut dec = turbojpeg::Decompressor::new().unwrap();
            dec.set_scaling_factor(turbojpeg::ScalingFactor::ONE_QUARTER).unwrap();
            let hdr = dec.read_header(&data).unwrap();
            let mut rgb_buf = vec![0u8; hdr.width * hdr.height * 3];
            dec.decompress(&data, turbojpeg::Image {
                pixels: rgb_buf.as_mut_slice(),
                width: hdr.width,
                height: hdr.height,
                pitch: hdr.width * 3,
                format: turbojpeg::PixelFormat::RGB,
            }).unwrap();
            let turbo_ms = t.elapsed().as_millis();
            println!("  decode (turbo 1/4): {}ms  ({}x{})  [{:.1}x vs zune]",
                turbo_ms, hdr.width, hdr.height,
                decode_ms as f64 / turbo_ms.max(1) as f64);
        }

        // Decode via turbojpeg with DCT 1/8 scaling
        {
            let t = Instant::now();
            let mut dec = turbojpeg::Decompressor::new().unwrap();
            dec.set_scaling_factor(turbojpeg::ScalingFactor::ONE_EIGHTH).unwrap();
            let hdr = dec.read_header(&data).unwrap();
            let mut rgb_buf = vec![0u8; hdr.width * hdr.height * 3];
            dec.decompress(&data, turbojpeg::Image {
                pixels: rgb_buf.as_mut_slice(),
                width: hdr.width,
                height: hdr.height,
                pitch: hdr.width * 3,
                format: turbojpeg::PixelFormat::RGB,
            }).unwrap();
            let turbo_ms = t.elapsed().as_millis();
            println!("  decode (turbo 1/8): {}ms  ({}x{})  [{:.1}x vs zune]",
                turbo_ms, hdr.width, hdr.height,
                decode_ms as f64 / turbo_ms.max(1) as f64);
        }

        // into_rgba8 (consumes)
        let t = Instant::now();
        let rgba = img.into_rgba8();
        let rgba_ms = t.elapsed().as_millis();
        println!("  into_rgba8:   {}ms  ({:.1} MB)", rgba_ms, rgba.len() as f64 / 1024.0 / 1024.0);

        // SIMD resize to 1024
        let (src_w, src_h) = (rgba.width(), rgba.height());
        let scale = 1024.0 / std::cmp::max(src_w, src_h) as f64;
        let dst_w = std::cmp::max(1, (src_w as f64 * scale) as u32);
        let dst_h = std::cmp::max(1, (src_h as f64 * scale) as u32);

        let t = Instant::now();
        {
            use fast_image_resize as fr;
            let src_image = fr::images::Image::from_vec_u8(
                src_w, src_h, rgba.into_raw(), fr::PixelType::U8x4,
            ).unwrap();
            let mut dst_image = fr::images::Image::new(dst_w, dst_h, fr::PixelType::U8x4);
            let mut resizer = fr::Resizer::new();
            resizer.resize(
                &src_image, &mut dst_image,
                Some(&fr::ResizeOptions::new().resize_alg(
                    fr::ResizeAlg::Convolution(fr::FilterType::Bilinear),
                )),
            ).unwrap();
            let buf = dst_image.into_vec();
            let small = image::RgbaImage::from_raw(dst_w, dst_h, buf).unwrap();
            let small_dyn = image::DynamicImage::ImageRgba8(small);
            let resize_ms = t.elapsed().as_millis();
            println!("  SIMD resize:  {}ms  ({}x{})", resize_ms, dst_w, dst_h);

            // Thumbnail from small
            let t = Instant::now();
            let thumb = small_dyn.resize(300, 300, image::imageops::FilterType::Triangle).to_rgb8();
            let mut buf = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 80)
                .encode(&thumb, thumb.width(), thumb.height(), image::ExtendedColorType::Rgb8)
                .unwrap();
            let thumb_ms = t.elapsed().as_millis();
            println!("  thumbnail:    {}ms  ({} bytes)", thumb_ms, buf.len());

            // Blur
            let t = Instant::now();
            let _blur = compute_blur(&small_dyn);
            let blur_ms = t.elapsed().as_millis();
            println!("  blur:         {}ms", blur_ms);

            // Exposure
            let t = Instant::now();
            let _exp = compute_exposure(&small_dyn);
            let exp_ms = t.elapsed().as_millis();
            println!("  exposure:     {}ms", exp_ms);

            // Perceptual hash
            let t = Instant::now();
            let _hash = hasher.hash_image(&small_dyn);
            let hash_ms = t.elapsed().as_millis();
            println!("  phash:        {}ms", hash_ms);
        }
    }

    // Cleanup
    std::fs::remove_dir_all(&cache_dir).ok();
}

fn compute_blur(image: &image::DynamicImage) -> BlurResult {
    let gray = image.to_luma8();
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
            let c = pixels[y * w + x] as f64;
            let t = pixels[(y - 1) * w + x] as f64;
            let b = pixels[(y + 1) * w + x] as f64;
            let l = pixels[y * w + (x - 1)] as f64;
            let r = pixels[y * w + (x + 1)] as f64;
            let lap = t + b + l + r - 4.0 * c;
            sum += lap;
            sum_sq += lap * lap;
            count += 1;
        }
    }
    let mean = sum / count as f64;
    let variance = (sum_sq / count as f64) - (mean * mean);
    BlurResult {
        laplacian_variance: variance,
        is_blurry: variance < 100.0,
    }
}

fn compute_exposure(image: &image::DynamicImage) -> ExposureResult {
    let gray = image.to_luma8();
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
    let pct_underexposed: f64 = histogram[..25].iter().sum::<u64>() as f64 / total;
    let pct_overexposed: f64 = histogram[230..].iter().sum::<u64>() as f64 / total;
    let verdict = if pct_underexposed > 0.30 && pct_overexposed > 0.30 {
        "HighContrast"
    } else if pct_underexposed > 0.30 {
        "Underexposed"
    } else if pct_overexposed > 0.30 {
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
