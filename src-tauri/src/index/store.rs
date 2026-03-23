use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::metadata::ExifMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedImage {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub file_size: u64,
    pub modified_at: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumbnail_path: Option<String>,
    pub exif: Option<ExifMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlurResult {
    pub laplacian_variance: f64,
    pub is_blurry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposureResult {
    pub mean_luminance: f64,
    pub pct_underexposed: f64,
    pub pct_overexposed: f64,
    pub verdict: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceEyeResult {
    pub left_eye_open: f64,
    pub right_eye_open: f64,
    pub eyes_closed: bool,
    /// Face bounding box in normalized coords [x, y, width, height] (0.0-1.0).
    /// Origin is bottom-left (Apple Vision convention).
    pub bounding_box: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedEyesResult {
    pub face_count: u32,
    pub faces: Vec<FaceEyeResult>,
    pub has_closed_eyes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectFocusResult {
    /// Laplacian variance within face/subject regions
    pub subject_blur_variance: f64,
    /// Laplacian variance outside face regions
    pub background_blur_variance: f64,
    /// subject / background ratio. >1 = subject sharper than bg (good)
    pub focus_ratio: f64,
    /// "SubjectSharp" | "SubjectBlurry" | "BackFocus" | "AllBlurry"
    pub verdict: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceInfo {
    pub face_index: u32,
    pub bounding_box: [f64; 4],
    pub person_id: Option<String>,
    pub face_thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResults {
    pub blur: Option<BlurResult>,
    pub exposure: Option<ExposureResult>,
    pub duplicate_group_id: Option<String>,
    pub scene_group_id: Option<String>,
    pub closed_eyes: Option<ClosedEyesResult>,
    pub subject_focus: Option<SubjectFocusResult>,
    pub faces: Option<Vec<FaceInfo>>,
}

pub struct ImageIndex {
    pub root: PathBuf,
    pub images: Vec<IndexedImage>,
    pub by_id: HashMap<String, usize>,
}

impl ImageIndex {
    pub fn new(root: PathBuf, images: Vec<IndexedImage>) -> Self {
        let by_id = images
            .iter()
            .enumerate()
            .map(|(i, img)| (img.id.clone(), i))
            .collect();
        Self { root, images, by_id }
    }

    pub fn get(&self, id: &str) -> Option<&IndexedImage> {
        self.by_id.get(id).map(|&i| &self.images[i])
    }
}

pub struct AnalysisIndex {
    pub results: HashMap<String, AnalysisResults>,
    pub duplicate_groups: HashMap<String, Vec<String>>,
    pub scene_groups: HashMap<String, Vec<String>>,
    /// person-id -> [(image-id, face-index)]
    pub person_groups: HashMap<String, Vec<(String, u32)>>,
}
