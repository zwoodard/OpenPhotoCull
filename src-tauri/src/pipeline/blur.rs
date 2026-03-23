use image::DynamicImage;

use super::traits::Analyzer;
use crate::index::store::{AnalysisResults, BlurResult, IndexedImage};

/// Blur detection using Laplacian variance.
/// Lower variance = blurrier image.
pub struct BlurAnalyzer {
    pub threshold: f64,
}

impl Default for BlurAnalyzer {
    fn default() -> Self {
        Self { threshold: 100.0 }
    }
}

impl Analyzer for BlurAnalyzer {
    fn name(&self) -> &str {
        "blur"
    }

    fn analyze_single(
        &self,
        image: &DynamicImage,
        _meta: &IndexedImage,
        results: &mut AnalysisResults,
    ) {
        let gray = image.to_luma8();
        let (w, h) = (gray.width() as usize, gray.height() as usize);

        if w < 3 || h < 3 {
            results.blur = Some(BlurResult {
                laplacian_variance: 0.0,
                is_blurry: true,
            });
            return;
        }

        // Apply 3x3 Laplacian kernel: [0,1,0; 1,-4,1; 0,1,0]
        let pixels = gray.as_raw();
        let mut sum = 0.0f64;
        let mut sum_sq = 0.0f64;
        let mut count = 0u64;

        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let center = pixels[y * w + x] as f64;
                let top = pixels[(y - 1) * w + x] as f64;
                let bottom = pixels[(y + 1) * w + x] as f64;
                let left = pixels[y * w + (x - 1)] as f64;
                let right = pixels[y * w + (x + 1)] as f64;

                let laplacian = top + bottom + left + right - 4.0 * center;
                sum += laplacian;
                sum_sq += laplacian * laplacian;
                count += 1;
            }
        }

        let mean = sum / count as f64;
        let variance = (sum_sq / count as f64) - (mean * mean);

        results.blur = Some(BlurResult {
            laplacian_variance: variance,
            is_blurry: variance < self.threshold,
        });
    }
}
