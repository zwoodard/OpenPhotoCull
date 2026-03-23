use image::DynamicImage;
use image_hasher::{HashAlg, HasherConfig};
use std::collections::HashMap;

use super::traits::Analyzer;
use crate::index::store::{AnalysisResults, IndexedImage};

/// Duplicate detection using EXIF date clustering + perceptual hashing.
pub struct DuplicateAnalyzer {
    /// Time window in seconds for EXIF date clustering
    pub time_window_secs: i64,
    /// Maximum Hamming distance for perceptual hash match
    pub hash_distance_threshold: u32,
}

impl Default for DuplicateAnalyzer {
    fn default() -> Self {
        Self {
            time_window_secs: 5,
            hash_distance_threshold: 10,
        }
    }
}

impl Analyzer for DuplicateAnalyzer {
    fn name(&self) -> &str {
        "duplicates"
    }

    fn analyze_single(
        &self,
        _image: &DynamicImage,
        _meta: &IndexedImage,
        _results: &mut AnalysisResults,
    ) {
        // No-op: duplicate detection requires batch mode
    }

    fn requires_batch(&self) -> bool {
        true
    }

    fn analyze_batch(
        &self,
        images: &[(DynamicImage, IndexedImage)],
        results: &mut [AnalysisResults],
    ) {
        if images.is_empty() {
            return;
        }

        // Phase 1: Cluster by time window
        let clusters = self.cluster_by_time(images);

        // Phase 2: Compute perceptual hashes
        let hasher = HasherConfig::new()
            .hash_alg(HashAlg::DoubleGradient)
            .hash_size(16, 16)
            .to_hasher();

        let hashes: Vec<_> = images
            .iter()
            .map(|(img, _)| hasher.hash_image(img))
            .collect();

        // Phase 3: Find duplicates within clusters
        let mut group_id_counter = 0u64;
        // Map from image index to group id
        let mut image_groups: HashMap<usize, String> = HashMap::new();
        // Union-find parent array
        let n = images.len();
        let mut parent: Vec<usize> = (0..n).collect();

        fn find(parent: &mut [usize], x: usize) -> usize {
            if parent[x] != x {
                parent[x] = find(parent, parent[x]);
            }
            parent[x]
        }

        fn union(parent: &mut [usize], a: usize, b: usize) {
            let ra = find(parent, a);
            let rb = find(parent, b);
            if ra != rb {
                parent[ra] = rb;
            }
        }

        for cluster in &clusters {
            if cluster.len() < 2 {
                continue;
            }

            // Compare all pairs within cluster
            for i in 0..cluster.len() {
                for j in (i + 1)..cluster.len() {
                    let idx_a = cluster[i];
                    let idx_b = cluster[j];
                    let dist = hashes[idx_a].dist(&hashes[idx_b]);
                    if dist <= self.hash_distance_threshold {
                        union(&mut parent, idx_a, idx_b);
                    }
                }
            }
        }

        // Collect groups
        let mut root_to_group: HashMap<usize, String> = HashMap::new();
        for i in 0..n {
            let root = find(&mut parent, i);
            if root != i || parent.iter().enumerate().any(|(j, _)| j != i && find(&mut parent.clone(), j) == root) {
                // This image is part of a group
                let root = find(&mut parent, i);
                // Check if this root actually has other members
                let has_group = (0..n).any(|j| j != root && find(&mut parent, j) == root);
                if has_group || root != i {
                    let group_id = root_to_group.entry(root).or_insert_with(|| {
                        group_id_counter += 1;
                        format!("dup-{}", group_id_counter)
                    });
                    image_groups.insert(i, group_id.clone());
                }
            }
        }

        // Write results
        for (i, result) in results.iter_mut().enumerate() {
            result.duplicate_group_id = image_groups.get(&i).cloned();
        }
    }
}

impl DuplicateAnalyzer {
    fn cluster_by_time(&self, images: &[(DynamicImage, IndexedImage)]) -> Vec<Vec<usize>> {
        // Parse timestamps and sort
        let mut timed: Vec<(usize, i64)> = images
            .iter()
            .enumerate()
            .map(|(i, (_, meta))| {
                let ts = meta
                    .exif
                    .as_ref()
                    .and_then(|e| e.date_time_original.as_ref())
                    .and_then(|d| parse_exif_date(d))
                    .unwrap_or(meta.modified_at as i64);
                (i, ts)
            })
            .collect();

        timed.sort_by_key(|&(_, ts)| ts);

        // Group into clusters where adjacent images are within time_window
        let mut clusters: Vec<Vec<usize>> = Vec::new();
        let mut current_cluster: Vec<usize> = Vec::new();
        let mut last_ts: Option<i64> = None;

        for (idx, ts) in timed {
            if let Some(prev_ts) = last_ts {
                if (ts - prev_ts).abs() > self.time_window_secs {
                    if current_cluster.len() >= 2 {
                        clusters.push(std::mem::take(&mut current_cluster));
                    } else {
                        current_cluster.clear();
                    }
                }
            }
            current_cluster.push(idx);
            last_ts = Some(ts);
        }

        if current_cluster.len() >= 2 {
            clusters.push(current_cluster);
        }

        // Also add a catch-all cluster for images without timestamps
        // (they'll be compared against each other)
        if clusters.is_empty() && images.len() > 1 {
            // If no time-based clusters, create one big cluster
            // but limit to reasonable size to avoid O(n^2) explosion
            let all_indices: Vec<usize> = (0..images.len()).collect();
            // Split into chunks of 50 to keep pairwise comparison tractable
            for chunk in all_indices.chunks(50) {
                if chunk.len() >= 2 {
                    clusters.push(chunk.to_vec());
                }
            }
        }

        clusters
    }
}

fn parse_exif_date(date_str: &str) -> Option<i64> {
    // EXIF date format: "2024:01:15 14:30:00" or "2024-01-15T14:30:00"
    let cleaned = date_str
        .replace("\"", "")
        .trim()
        .to_string();

    // Try chrono parsing
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%d %H:%M:%S") {
        return Some(dt.and_utc().timestamp());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y:%m:%d %H:%M:%S") {
        return Some(dt.and_utc().timestamp());
    }

    None
}
