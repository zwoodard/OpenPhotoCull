use image::DynamicImage;

use crate::index::store::{AnalysisResults, IndexedImage};

/// Trait for image analysis plugins.
/// Implement this trait to add new analysis capabilities.
pub trait Analyzer: Send + Sync {
    /// Unique name for this analyzer
    fn name(&self) -> &str;

    /// Analyze a single image. Receives the downsampled image + metadata.
    /// Should mutate the results in-place (setting its own field).
    fn analyze_single(
        &self,
        image: &DynamicImage,
        meta: &IndexedImage,
        results: &mut AnalysisResults,
    );

    /// Whether this analyzer needs to see all images at once (e.g., duplicates).
    fn requires_batch(&self) -> bool {
        false
    }

    /// Batch analysis for analyzers that need global context.
    /// Default implementation calls analyze_single for each image.
    fn analyze_batch(
        &self,
        images: &[(DynamicImage, IndexedImage)],
        results: &mut [AnalysisResults],
    ) {
        for (i, (img, meta)) in images.iter().enumerate() {
            self.analyze_single(img, meta, &mut results[i]);
        }
    }
}
