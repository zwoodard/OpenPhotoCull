//! CLI benchmark exercising the full ingestion pipeline via process_single_image.
//! No duplicated logic — calls the same code path as the app.
//!
//! Run: cargo run --release --bin bench -- <folder_path> [threads]

use image_hasher::{HashAlg, HasherConfig};
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use photo_scrub_lib::commands::scan::process_single_image;
use photo_scrub_lib::index::discovery;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let folder = args.get(1).map(|s| s.as_str()).unwrap_or("test-photos/sampled2");
    let root = PathBuf::from(folder);
    if !root.is_dir() {
        eprintln!("Error: {} is not a directory", folder);
        std::process::exit(1);
    }
    let num_threads: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let face_enabled = !args.iter().any(|a| a == "--no-faces");

    println!("=== OpenPhotoCull Pipeline Benchmark ===");
    println!("Folder:  {}", root.display());
    println!("Threads: {}", num_threads);
    println!("Faces:   {}", if face_enabled { "enabled" } else { "disabled" });
    println!();

    // Discovery
    let t = Instant::now();
    let discovered = discovery::discover_images(&root);
    let discovery_ms = t.elapsed().as_millis();
    let total = discovered.len();
    let total_bytes: u64 = discovered.iter().map(|d| d.file_size).sum();
    println!("[discovery]   {} images, {:.1} MB in {}ms", total, total_bytes as f64 / 1e6, discovery_ms);

    // Setup
    let cache_dir = std::env::temp_dir().join("opc-bench-thumbs");
    std::fs::create_dir_all(&cache_dir).ok();
    for entry in std::fs::read_dir(&cache_dir).into_iter().flatten().flatten() {
        std::fs::remove_file(entry.path()).ok();
    }

    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::DoubleGradient)
        .hash_size(16, 16)
        .to_hasher();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .unwrap();

    // Pre-warm Vision models (same as app does)
    let t = Instant::now();
    rayon::join(
        || photo_scrub_lib::pipeline::closed_eyes::warmup_face_detection_model(),
        || {
            if face_enabled {
                photo_scrub_lib::pipeline::face_grouping::warmup_feature_print_model();
            }
        },
    );
    println!("[warmup]      Vision models loaded in {}ms", t.elapsed().as_millis());

    // Pipeline run — calls the exact same code path as the app
    let counter = Arc::new(AtomicUsize::new(0));
    let per_image_ms: Arc<std::sync::Mutex<Vec<(String, u128, u64)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    println!();
    let t_pipeline = Instant::now();
    let results: Vec<_> = pool.install(|| {
        discovered
            .par_iter()
            .map(|disc| {
                let img_start = Instant::now();
                let result = process_single_image(disc, &cache_dir, &hasher, face_enabled);
                let ms = img_start.elapsed().as_millis();

                per_image_ms.lock().unwrap().push((
                    result.image.file_name.clone(),
                    ms,
                    disc.file_size,
                ));

                let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if done % 10 == 0 || done == total {
                    let elapsed = t_pipeline.elapsed().as_millis();
                    let rate = done as f64 / (elapsed as f64 / 1000.0);
                    eprint!("\r  [{}/{}] {:.1} imgs/sec  ({})     ",
                        done, total, rate, result.image.file_name);
                }
                result
            })
            .collect()
    });
    let pipeline_ms = t_pipeline.elapsed().as_millis();
    eprintln!();
    println!("[pipeline]    {}ms ({:.1} imgs/sec)", pipeline_ms, total as f64 / (pipeline_ms as f64 / 1000.0));

    // Cached run (thumbnails already generated)
    counter.store(0, Ordering::Relaxed);
    let t = Instant::now();
    pool.install(|| {
        discovered.par_iter().for_each(|disc| {
            let _ = process_single_image(disc, &cache_dir, &hasher, face_enabled);
        });
    });
    let cached_ms = t.elapsed().as_millis();
    println!("[cached run]  {}ms ({:.1} imgs/sec)", cached_ms, total as f64 / (cached_ms as f64 / 1000.0));

    // Stats
    let mut times = per_image_ms.lock().unwrap().clone();
    times.sort_by(|a, b| b.1.cmp(&a.1));

    println!();
    println!("=== Per-image (slowest 10) ===");
    for (name, ms, size) in times.iter().take(10) {
        let faces = results.iter()
            .find(|r| r.image.file_name == *name)
            .and_then(|r| r.analysis.closed_eyes.as_ref())
            .map(|c| c.face_count)
            .unwrap_or(0);
        println!("  {:>6}ms  {} ({:.1}MB, {} faces)", ms, name, *size as f64 / 1e6, faces);
    }

    println!();
    println!("=== Per-image (fastest 10) ===");
    for (name, ms, _) in times.iter().rev().take(10) {
        println!("  {:>6}ms  {}", ms, name);
    }

    // Time distribution
    println!();
    println!("=== Time distribution ===");
    let mut buckets = [0u32; 10];
    for (_, ms, _) in &times {
        let bucket = std::cmp::min((*ms / 100) as usize, 9);
        buckets[bucket] += 1;
    }
    let max_count = *buckets.iter().max().unwrap_or(&1);
    for (i, &count) in buckets.iter().enumerate() {
        let label = if i == 9 { "  900+ ms".to_string() }
        else { format!("{:>3}-{:<3}ms", i * 100, (i + 1) * 100) };
        let bar = "#".repeat(if max_count > 0 { (count as f64 / max_count as f64 * 40.0) as usize } else { 0 });
        println!("  {} {:>3} {}", label, count, bar);
    }

    // Face summary
    let total_faces: u32 = results.iter()
        .filter_map(|r| r.analysis.closed_eyes.as_ref())
        .map(|c| c.face_count)
        .sum();
    let with_faces = results.iter()
        .filter(|r| r.analysis.closed_eyes.as_ref().is_some_and(|c| c.face_count > 0))
        .count();
    let with_embeddings = results.iter()
        .filter(|r| !r.face_embeddings.is_empty())
        .count();
    println!();
    println!("=== Summary ===");
    println!("  {} faces in {} images ({} with faces, {} with embeddings)",
        total_faces, total, with_faces, with_embeddings);
    println!("  Blurry: {}", results.iter().filter(|r| r.analysis.blur.as_ref().is_some_and(|b| b.is_blurry)).count());
    println!("  Exposure issues: {}", results.iter().filter(|r| r.analysis.exposure.as_ref().is_some_and(|e| e.verdict != "Normal")).count());
    println!("  Duplicates: {} groups", results.iter().filter(|r| r.phash.is_some()).count());

    std::fs::remove_dir_all(&cache_dir).ok();
}
