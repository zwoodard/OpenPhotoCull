//! Generate test images with controlled blur and exposure levels.
//! Creates variations of a source image for threshold testing.
//!
//! Run with: cargo run --release --bin gen_test_images -- <source_jpeg> <output_dir>

use image::{DynamicImage, GenericImageView, imageops::FilterType};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let source_path = args.get(1).expect("Usage: gen_test_images <source.jpg> <output_dir>");
    let output_dir = args.get(2).unwrap_or(&"test-photos/synthetic".to_string()).clone();

    std::fs::create_dir_all(&output_dir).unwrap();

    println!("Loading source: {}", source_path);
    let img = image::open(source_path).unwrap();
    let (w, h) = (img.width(), img.height());
    println!("  {}x{}", w, h);

    // Work at analysis size for consistency
    let img = if w > 1024 || h > 1024 {
        img.resize(1024, 1024, FilterType::Lanczos3)
    } else {
        img
    };

    // ── Blur variations ──
    // Apply Gaussian blur at increasing kernel sizes
    println!("\nGenerating blur levels...");
    let blur_levels = [
        (0.0, "sharp"),
        (0.5, "blur_slight"),
        (1.0, "blur_mild"),
        (2.0, "blur_moderate"),
        (4.0, "blur_heavy"),
        (8.0, "blur_extreme"),
    ];

    for (sigma, name) in &blur_levels {
        let blurred = if *sigma == 0.0 {
            img.clone()
        } else {
            img.blur(*sigma)
        };
        let path = format!("{}/{}.jpg", output_dir, name);
        blurred.save(&path).unwrap();

        // Compute Laplacian variance to show expected score
        let variance = compute_laplacian_variance(&blurred);
        println!("  {} → variance={:.1}  (sigma={})", name, variance, sigma);
    }

    // ── Exposure variations ──
    // Adjust brightness to simulate over/under exposure
    println!("\nGenerating exposure levels...");
    let exposure_levels: Vec<(f32, &str)> = vec![
        (0.1, "exp_very_dark"),
        (0.3, "exp_dark"),
        (0.5, "exp_underexposed"),
        (0.7, "exp_slightly_dark"),
        (1.0, "exp_normal"),
        (1.4, "exp_slightly_bright"),
        (1.8, "exp_overexposed"),
        (2.5, "exp_bright"),
        (4.0, "exp_very_bright"),
    ];

    for (factor, name) in &exposure_levels {
        let adjusted = adjust_brightness(&img, *factor);
        let path = format!("{}/{}.jpg", output_dir, name);
        adjusted.save(&path).unwrap();

        let (mean_lum, pct_under, pct_over) = compute_exposure_stats(&adjusted);
        println!("  {} → mean={:.2} under={:.1}% over={:.1}%  (factor={})",
            name, mean_lum, pct_under * 100.0, pct_over * 100.0, factor);
    }

    // ── Combined ──
    println!("\nGenerating combined (blur + exposure)...");
    let blurred_dark = adjust_brightness(&img.blur(3.0), 0.4);
    blurred_dark.save(format!("{}/combo_blurry_dark.jpg", output_dir)).unwrap();
    let v = compute_laplacian_variance(&blurred_dark);
    let (m, u, o) = compute_exposure_stats(&blurred_dark);
    println!("  combo_blurry_dark → blur_var={:.1} mean={:.2} under={:.1}% over={:.1}%", v, m, u*100.0, o*100.0);

    println!("\nDone! {} test images in {}", blur_levels.len() + exposure_levels.len() + 1, output_dir);
    println!("\nRun benchmark on them:");
    println!("  ./src-tauri/target/release/bench {} 8", output_dir);
}

fn compute_laplacian_variance(image: &DynamicImage) -> f64 {
    let gray = image.to_luma8();
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    if w < 3 || h < 3 { return 0.0; }

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
    (sum_sq / count as f64) - (mean * mean)
}

fn compute_exposure_stats(image: &DynamicImage) -> (f64, f64, f64) {
    let gray = image.to_luma8();
    let pixels = gray.as_raw();
    let total = pixels.len() as f64;
    if total == 0.0 { return (0.0, 0.0, 0.0); }

    let mut histogram = [0u64; 256];
    let mut lum_sum = 0u64;
    for &p in pixels {
        histogram[p as usize] += 1;
        lum_sum += p as u64;
    }

    let mean = (lum_sum as f64 / total) / 255.0;
    let under: f64 = histogram[..25].iter().sum::<u64>() as f64 / total;
    let over: f64 = histogram[230..].iter().sum::<u64>() as f64 / total;
    (mean, under, over)
}

fn adjust_brightness(image: &DynamicImage, factor: f32) -> DynamicImage {
    let rgb = image.to_rgb8();
    let mut out = rgb.clone();

    for pixel in out.pixels_mut() {
        for c in pixel.0.iter_mut() {
            *c = ((*c as f32 * factor).min(255.0).max(0.0)) as u8;
        }
    }

    DynamicImage::ImageRgb8(out)
}
