use image::DynamicImage;

use super::traits::Analyzer;
use crate::index::store::{AnalysisResults, ExposureResult, IndexedImage};

/// Exposure analysis using luminance histogram.
pub struct ExposureAnalyzer {
    /// Threshold for percentage of pixels considered underexposed (in bottom bins)
    pub under_threshold: f64,
    /// Threshold for percentage of pixels considered overexposed (in top bins)
    pub over_threshold: f64,
}

impl Default for ExposureAnalyzer {
    fn default() -> Self {
        Self {
            under_threshold: 0.30, // 30% of pixels in darkest region
            over_threshold: 0.30,  // 30% of pixels in brightest region
        }
    }
}

impl Analyzer for ExposureAnalyzer {
    fn name(&self) -> &str {
        "exposure"
    }

    fn analyze_single(
        &self,
        image: &DynamicImage,
        _meta: &IndexedImage,
        results: &mut AnalysisResults,
    ) {
        let gray = image.to_luma8();
        let pixels = gray.as_raw();
        let total = pixels.len() as f64;

        if total == 0.0 {
            return;
        }

        // Build histogram (256 bins)
        let mut histogram = [0u64; 256];
        let mut lum_sum = 0u64;
        for &p in pixels {
            histogram[p as usize] += 1;
            lum_sum += p as u64;
        }

        let mean_luminance = (lum_sum as f64 / total) / 255.0;

        // Bottom 25 bins = underexposed region
        let under_count: u64 = histogram[..25].iter().sum();
        let pct_underexposed = under_count as f64 / total;

        // Top 25 bins = overexposed region
        let over_count: u64 = histogram[230..].iter().sum();
        let pct_overexposed = over_count as f64 / total;

        let verdict = if pct_underexposed > self.under_threshold && pct_overexposed > self.over_threshold
        {
            "HighContrast"
        } else if pct_underexposed > self.under_threshold {
            "Underexposed"
        } else if pct_overexposed > self.over_threshold {
            "Overexposed"
        } else {
            "Normal"
        };

        results.exposure = Some(ExposureResult {
            mean_luminance,
            pct_underexposed,
            pct_overexposed,
            verdict: verdict.to_string(),
        });
    }
}
