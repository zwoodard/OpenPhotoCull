//! Diagnostic: extract face embeddings from test photos and print stats.
//! Usage: cargo run --release --bin face_debug -- <image1.jpg> [image2.jpg ...]

use photo_scrub_lib::pipeline::face_grouping;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: face_debug <image1.jpg> [image2.jpg ...]");
        std::process::exit(1);
    }

    let thumb_dir = std::env::temp_dir().join("face_debug_thumbs");
    std::fs::create_dir_all(&thumb_dir).unwrap();

    let mut all_embeddings: Vec<(String, Vec<f32>)> = Vec::new();

    for path_str in &args {
        let path = Path::new(path_str);
        let img = match image::open(path) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Failed to open {}: {}", path_str, e);
                continue;
            }
        };

        // Detect faces using closed_eyes (which gives us bounding boxes)
        let jpeg_data = std::fs::read(path).ok();
        let closed_eyes =
            photo_scrub_lib::pipeline::closed_eyes::detect(jpeg_data.as_deref(), &img);

        let face_boxes: Vec<[f64; 4]> = closed_eyes
            .as_ref()
            .map(|ce| ce.faces.iter().filter_map(|f| f.bounding_box).collect())
            .unwrap_or_default();

        println!(
            "\n=== {} ===\n  Faces detected: {}",
            path.file_name().unwrap().to_string_lossy(),
            face_boxes.len()
        );

        if face_boxes.is_empty() {
            continue;
        }

        let file_id = path.file_stem().unwrap().to_string_lossy().to_string();
        let (faces, embeddings) =
            face_grouping::extract_faces(&img, &face_boxes, &thumb_dir, &file_id);

        for (i, emb) in embeddings.iter().enumerate() {
            if emb.is_empty() {
                println!("  Face {}: EMPTY embedding", i);
                continue;
            }

            let min = emb.iter().cloned().fold(f32::INFINITY, f32::min);
            let max = emb.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let mean: f32 = emb.iter().sum::<f32>() / emb.len() as f32;
            let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nonzero = emb.iter().filter(|x| **x != 0.0).count();

            println!(
                "  Face {}: len={}, min={:.4}, max={:.4}, mean={:.4}, L2norm={:.4}, nonzero={}/{}",
                i,
                emb.len(),
                min,
                max,
                mean,
                norm,
                nonzero,
                emb.len()
            );
            println!(
                "    first 8: {:?}",
                &emb[..8.min(emb.len())]
            );

            all_embeddings.push((
                format!(
                    "{}:face{}",
                    path.file_name().unwrap().to_string_lossy(),
                    i
                ),
                emb.clone(),
            ));
        }
    }

    // Pairwise distances
    if all_embeddings.len() >= 2 {
        println!("\n=== Pairwise cosine similarity ===");
        for i in 0..all_embeddings.len() {
            for j in (i + 1)..all_embeddings.len() {
                let a = &all_embeddings[i].1;
                let b = &all_embeddings[j].1;
                if a.len() != b.len() || a.is_empty() {
                    println!(
                        "  {} vs {}: INCOMPATIBLE (len {} vs {})",
                        all_embeddings[i].0,
                        all_embeddings[j].0,
                        a.len(),
                        b.len()
                    );
                    continue;
                }
                let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
                let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
                let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
                let cosine = if norm_a > 0.0 && norm_b > 0.0 {
                    dot / (norm_a * norm_b)
                } else {
                    0.0
                };
                let l2: f32 = a
                    .iter()
                    .zip(b.iter())
                    .map(|(x, y)| (x - y) * (x - y))
                    .sum::<f32>()
                    .sqrt();
                println!(
                    "  {} vs {}: cosine={:.4}, L2={:.2}",
                    all_embeddings[i].0, all_embeddings[j].0, cosine, l2
                );
            }
        }
    }
}
