use image::DynamicImage;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::ipc::Channel;

use super::blur::BlurAnalyzer;
use super::duplicates::DuplicateAnalyzer;
use super::exposure::ExposureAnalyzer;
use super::traits::Analyzer;
use crate::index::store::{AnalysisIndex, AnalysisResults, ImageIndex};
use crate::thumbnail;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub elapsed_ms: u64,
    pub current_file: Option<String>,
    pub step_timings: Option<HashMap<String, u64>>,
}

pub struct AnalyzerRegistry {
    single_analyzers: Vec<Box<dyn Analyzer>>,
    batch_analyzers: Vec<Box<dyn Analyzer>>,
}

impl AnalyzerRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            single_analyzers: Vec::new(),
            batch_analyzers: Vec::new(),
        };

        registry.register(Box::new(BlurAnalyzer::default()));
        registry.register(Box::new(ExposureAnalyzer::default()));
        registry.register(Box::new(DuplicateAnalyzer::default()));

        registry
    }

    pub fn register(&mut self, analyzer: Box<dyn Analyzer>) {
        if analyzer.requires_batch() {
            self.batch_analyzers.push(analyzer);
        } else {
            self.single_analyzers.push(analyzer);
        }
    }

    /// Run all analyzers. `cached_images` contains pre-decoded images from the
    /// scan phase (keyed by image id). Images not in the cache will be decoded
    /// on demand as a fallback.
    pub fn run_all(
        &self,
        index: &ImageIndex,
        cached_images: HashMap<String, DynamicImage>,
        on_progress: &Channel<ProgressEvent>,
    ) -> AnalysisIndex {
        let total = index.images.len();
        let counter = Arc::new(AtomicUsize::new(0));
        let global_start = Instant::now();
        let mut step_timings: HashMap<String, u64> = HashMap::new();

        // Wrap cache in Arc<Mutex> so rayon workers can take from it
        let cache = Arc::new(Mutex::new(cached_images));
        let cache_hits = Arc::new(AtomicUsize::new(0));
        let cache_misses = Arc::new(AtomicUsize::new(0));

        // Phase 1: Run single-image analyzers in parallel
        on_progress
            .send(ProgressEvent {
                phase: "Analyzing blur + exposure...".into(),
                current: 0,
                total,
                elapsed_ms: 0,
                current_file: None,
                step_timings: None,
            })
            .ok();

        let phase_start = Instant::now();

        let results: Vec<AnalysisResults> = index
            .images
            .par_iter()
            .map(|img| {
                let mut result = AnalysisResults::default();

                // Try to use cached image first, fall back to re-decode
                let dyn_img = cache
                    .lock()
                    .unwrap()
                    .remove(&img.id)
                    .map(|img| {
                        cache_hits.fetch_add(1, Ordering::Relaxed);
                        img
                    })
                    .or_else(|| {
                        cache_misses.fetch_add(1, Ordering::Relaxed);
                        thumbnail::load_for_analysis(Path::new(&img.path))
                    });

                if let Some(dyn_img) = dyn_img {
                    for analyzer in &self.single_analyzers {
                        analyzer.analyze_single(&dyn_img, img, &mut result);
                    }
                }

                let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if done % 5 == 0 || done == total {
                    on_progress
                        .send(ProgressEvent {
                            phase: "Analyzing blur + exposure...".into(),
                            current: done,
                            total,
                            elapsed_ms: global_start.elapsed().as_millis() as u64,
                            current_file: Some(img.file_name.clone()),
                            step_timings: None,
                        })
                        .ok();
                }

                result
            })
            .collect();

        let single_ms = phase_start.elapsed().as_millis() as u64;
        step_timings.insert("blur+exposure".into(), single_ms);

        let hits = cache_hits.load(Ordering::Relaxed);
        let misses = cache_misses.load(Ordering::Relaxed);
        tracing::info!(
            "Analysis image cache: {} hits, {} misses ({}% hit rate)",
            hits,
            misses,
            if hits + misses > 0 {
                hits * 100 / (hits + misses)
            } else {
                0
            }
        );

        // Phase 2: Run batch analyzers (duplicates)
        if !self.batch_analyzers.is_empty() {
            on_progress
                .send(ProgressEvent {
                    phase: "Preparing duplicate detection...".into(),
                    current: 0,
                    total,
                    elapsed_ms: global_start.elapsed().as_millis() as u64,
                    current_file: None,
                    step_timings: Some(step_timings.clone()),
                })
                .ok();

            // For batch analyzers we need (image, metadata) pairs.
            // The cache is mostly drained by now, so re-decode what's needed.
            let load_start = Instant::now();

            let batch_data: Vec<_> = index
                .images
                .par_iter()
                .map(|img| {
                    let dyn_img = cache
                        .lock()
                        .unwrap()
                        .remove(&img.id)
                        .or_else(|| thumbnail::load_for_analysis(Path::new(&img.path)))
                        .unwrap_or_else(|| DynamicImage::new_rgb8(1, 1));
                    (dyn_img, img.clone())
                })
                .collect();

            let load_ms = load_start.elapsed().as_millis() as u64;
            step_timings.insert("dup_image_load".into(), load_ms);

            let dup_start = Instant::now();

            let mut results_mut = results;
            for analyzer in &self.batch_analyzers {
                on_progress
                    .send(ProgressEvent {
                        phase: format!("Running {} analysis...", analyzer.name()),
                        current: 0,
                        total,
                        elapsed_ms: global_start.elapsed().as_millis() as u64,
                        current_file: None,
                        step_timings: Some(step_timings.clone()),
                    })
                    .ok();

                analyzer.analyze_batch(&batch_data, &mut results_mut);
            }

            let dup_ms = dup_start.elapsed().as_millis() as u64;
            step_timings.insert("duplicate_detect".into(), dup_ms);

            on_progress
                .send(ProgressEvent {
                    phase: "Analysis complete".into(),
                    current: total,
                    total,
                    elapsed_ms: global_start.elapsed().as_millis() as u64,
                    current_file: None,
                    step_timings: Some(step_timings),
                })
                .ok();

            let mut analysis_map = HashMap::new();
            let mut duplicate_groups: HashMap<String, Vec<String>> = HashMap::new();

            for (i, result) in results_mut.into_iter().enumerate() {
                let id = index.images[i].id.clone();
                if let Some(ref group_id) = result.duplicate_group_id {
                    duplicate_groups
                        .entry(group_id.clone())
                        .or_default()
                        .push(id.clone());
                }
                analysis_map.insert(id, result);
            }

            return AnalysisIndex {
                results: analysis_map,
                duplicate_groups,
                scene_groups: HashMap::new(),
                person_groups: HashMap::new(),
            };
        }

        let mut analysis_map = HashMap::new();
        for (i, result) in results.into_iter().enumerate() {
            let id = index.images[i].id.clone();
            analysis_map.insert(id, result);
        }

        AnalysisIndex {
            results: analysis_map,
            duplicate_groups: HashMap::new(),
            scene_groups: HashMap::new(),
            person_groups: HashMap::new(),
        }
    }
}
