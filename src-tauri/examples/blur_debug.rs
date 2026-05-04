//! Per-image blur diagnostic: dumps every BlurResult field plus the intent
//! verdict for each image in a folder. Used to validate the new tile-based
//! detection logic on real photos.
//!
//! Run: cargo run --release --features bench-bins --bin blur_debug -- <folder>

use image_hasher::{HashAlg, HasherConfig};
use std::path::PathBuf;

use photo_scrub_lib::commands::scan::process_single_image;
use photo_scrub_lib::index::discovery;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let folder = args.get(1).map(|s| s.as_str()).unwrap_or("test_data");
    let root = PathBuf::from(folder);
    if !root.is_dir() {
        eprintln!("Error: {} is not a directory", folder);
        std::process::exit(1);
    }

    let discovered = discovery::discover_images(&root);
    if discovered.is_empty() {
        eprintln!("No images discovered in {}", folder);
        std::process::exit(1);
    }

    let cache_dir = std::env::temp_dir().join("opc-blur-debug-thumbs");
    std::fs::create_dir_all(&cache_dir).ok();
    for entry in std::fs::read_dir(&cache_dir).into_iter().flatten().flatten() {
        std::fs::remove_file(entry.path()).ok();
    }

    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::DoubleGradient)
        .hash_size(16, 16)
        .to_hasher();

    photo_scrub_lib::pipeline::closed_eyes::warmup_face_detection_model();

    println!(
        "{:<48}  {:>10}  {:>10}  {:>10}  {:>8}  {:>8}  {:>8}  {:>6}  {:>6}  {:>10}  {:>16}",
        "file",
        "global",
        "mean_tile",
        "max_tile",
        "p95_tile",
        "sharp%",
        "cluster%",
        "bokeh",
        "shake",
        "is_blurry",
        "intent",
    );
    println!("{}", "-".repeat(160));

    for disc in &discovered {
        let result = process_single_image(disc, &cache_dir, &hasher, false);
        let name = disc
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();

        if let Some(b) = &result.analysis.blur {
            let intent = blur_intent(b);
            println!(
                "{:<48}  {:>10.1}  {:>10.1}  {:>10.1}  {:>8.1}  {:>7.1}%  {:>7.1}%  {:>6}  {:>6}  {:>10}  {:>16}",
                truncate(&name, 48),
                b.laplacian_variance,
                b.mean_tile_variance,
                b.max_tile_variance,
                b.p95_tile_variance,
                b.sharp_tile_fraction * 100.0,
                b.largest_sharp_cluster * 100.0,
                yn(b.bokeh_likely),
                yn(b.shake_risk),
                b.is_blurry,
                intent,
            );
        } else {
            println!("{:<48}  (no blur result)", name);
        }

        if let Some(exif) = &result.image.exif {
            println!(
                "    exif: aperture={:?}  focal={:?}mm  shutter={:?}  iso={:?}",
                exif.aperture, exif.focal_length_mm, exif.shutter_speed, exif.iso
            );
        }
    }
}

fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "-" }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { format!("{}…", &s[..n - 1]) }
}

/// Mirrors the TS blurIntent() in src/store/index.ts.
fn blur_intent(b: &photo_scrub_lib::index::store::BlurResult) -> &'static str {
    let sharp_frac_required = if b.bokeh_likely { 0.02 } else { 0.05 };
    let has_sharp_region = b.max_tile_variance >= 500.0
        || b.largest_sharp_cluster >= 0.03
        || b.sharp_tile_fraction >= sharp_frac_required;
    if has_sharp_region {
        if b.mean_tile_variance >= 100.0 && !b.bokeh_likely {
            "Sharp"
        } else {
            "IntentionalBokeh"
        }
    } else if b.shake_risk {
        "ShakeBlur"
    } else {
        "OutOfFocus"
    }
}
