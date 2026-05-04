//! Micro-benchmark: isolate face embedding costs.
//! Tests: CGImage creation, Vision request overhead, neural net inference,
//! and whether batching/sequential processing helps.
//!
//! Run: cargo run --release --bin face_perf -- <folder>

use rayon::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

use photo_scrub_lib::index::discovery;
use photo_scrub_lib::pipeline::face_grouping;
use photo_scrub_lib::thumbnail;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let folder = args.get(1).map(|s| s.as_str()).unwrap_or("test-photos/sampled2");
    let root = PathBuf::from(folder);

    let discovered = discovery::discover_images(&root);
    let cache_dir = std::env::temp_dir().join("opc-face-perf");
    std::fs::create_dir_all(&cache_dir).ok();

    println!("=== Face Embedding Performance ===");
    println!("Scanning {} for images with faces...", folder);
    println!();

    // First pass: find images with faces using closed_eyes detection
    let mut face_images: Vec<(String, image::DynamicImage, Vec<[f64; 4]>)> = Vec::new();

    for disc in &discovered {
        let path_str = disc.path.to_string_lossy().to_string();
        let id = thumbnail::cache_key(&path_str, disc.modified_at);
        let processed = thumbnail::process_image(&disc.path, &cache_dir, &id);

        if let Some(ref img) = processed.analysis_image {
            let ext = disc.path.extension().and_then(|e| e.to_str())
                .map(|e| e.to_lowercase()).unwrap_or_default();
            let jpeg_data = if ext == "jpg" || ext == "jpeg" {
                std::fs::read(&disc.path).ok()
            } else {
                None
            };
            let closed_eyes = photo_scrub_lib::pipeline::closed_eyes::detect(
                jpeg_data.as_deref(), img,
            );

            let boxes: Vec<[f64; 4]> = closed_eyes.as_ref()
                .map(|ce| ce.faces.iter().filter_map(|f| f.bounding_box).collect())
                .unwrap_or_default();

            if !boxes.is_empty() {
                let name = disc.path.file_name().unwrap().to_string_lossy().to_string();
                face_images.push((name, img.clone(), boxes));
            }
        }
    }

    let total_faces: usize = face_images.iter().map(|(_, _, b)| b.len()).sum();
    println!("Found {} images with {} total faces", face_images.len(), total_faces);
    println!();

    // Test 1: Current approach — sequential per-image, each face gets its own Vision call
    println!("--- Test 1: Current (sequential per image, one Vision call per face) ---");
    let t = Instant::now();
    let mut embed_count = 0;
    for (name, img, boxes) in &face_images {
        let img_start = Instant::now();
        let (_, embeddings) = face_grouping::extract_faces(img, boxes, &cache_dir, name);
        let ms = img_start.elapsed().as_millis();
        let valid = embeddings.iter().filter(|e| !e.is_empty()).count();
        embed_count += valid;
        println!("  {} faces, {}ms ({} valid embeddings)  {}",
            boxes.len(), ms, valid, name);
    }
    let total_ms = t.elapsed().as_millis();
    println!("  TOTAL: {}ms for {} embeddings ({:.0}ms/face)",
        total_ms, embed_count, total_ms as f64 / embed_count.max(1) as f64);

    // Test 2: All faces sequentially on one thread (no rayon contention)
    println!();
    println!("--- Test 2: Same but after warmup (model cached) ---");
    let t = Instant::now();
    embed_count = 0;
    for (name, img, boxes) in &face_images {
        let (_, embeddings) = face_grouping::extract_faces(img, boxes, &cache_dir, name);
        embed_count += embeddings.iter().filter(|e| !e.is_empty()).count();
    }
    let total_ms2 = t.elapsed().as_millis();
    println!("  TOTAL: {}ms for {} embeddings ({:.0}ms/face)",
        total_ms2, embed_count, total_ms2 as f64 / embed_count.max(1) as f64);

    // Test 3: Parallel with rayon (8 threads)
    println!();
    println!("--- Test 3: Parallel (8 threads via rayon) ---");
    let pool = rayon::ThreadPoolBuilder::new().num_threads(8).build().unwrap();
    let t = Instant::now();
    let par_results: Vec<usize> = pool.install(|| {
        face_images.par_iter().map(|(name, img, boxes)| {
            let (_, embeddings) = face_grouping::extract_faces(img, boxes, &cache_dir, name);
            embeddings.iter().filter(|e| !e.is_empty()).count()
        }).collect()
    });
    let total_ms3 = t.elapsed().as_millis();
    let par_count: usize = par_results.iter().sum();
    println!("  TOTAL: {}ms for {} embeddings ({:.0}ms/face)",
        total_ms3, par_count, total_ms3 as f64 / par_count.max(1) as f64);

    // Test 4: Parallel with 2 threads (less contention)
    println!();
    println!("--- Test 4: Parallel (2 threads) ---");
    let pool2 = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
    let t = Instant::now();
    let par_results2: Vec<usize> = pool2.install(|| {
        face_images.par_iter().map(|(name, img, boxes)| {
            let (_, embeddings) = face_grouping::extract_faces(img, boxes, &cache_dir, name);
            embeddings.iter().filter(|e| !e.is_empty()).count()
        }).collect()
    });
    let total_ms4 = t.elapsed().as_millis();
    let par_count2: usize = par_results2.iter().sum();
    println!("  TOTAL: {}ms for {} embeddings ({:.0}ms/face)",
        total_ms4, par_count2, total_ms4 as f64 / par_count2.max(1) as f64);

    println!();
    println!("=== Summary ===");
    println!("  Sequential cold:  {}ms", total_ms);
    println!("  Sequential warm:  {}ms  ({:.1}x vs cold)", total_ms2, total_ms as f64 / total_ms2.max(1) as f64);
    println!("  Parallel 8 thr:   {}ms  ({:.1}x vs seq warm)", total_ms3, total_ms2 as f64 / total_ms3.max(1) as f64);
    println!("  Parallel 2 thr:   {}ms  ({:.1}x vs seq warm)", total_ms4, total_ms2 as f64 / total_ms4.max(1) as f64);

    std::fs::remove_dir_all(&cache_dir).ok();
}
