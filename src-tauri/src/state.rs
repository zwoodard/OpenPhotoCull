use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::index::store::{AnalysisIndex, ImageIndex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Keep,
    Delete,
    Unmarked,
}

pub struct AppState {
    pub index: RwLock<Option<ImageIndex>>,
    pub analysis: RwLock<Option<AnalysisIndex>>,
    pub marks: RwLock<HashMap<String, Mark>>,
    pub thumbnail_dir: PathBuf,
}

impl AppState {
    pub fn new() -> Self {
        let cache_dir = directories::ProjectDirs::from("com", "openphotocull", "OpenPhotoCull")
            .map(|d| d.cache_dir().join("thumbs"))
            .unwrap_or_else(|| PathBuf::from("/tmp/photoscrub/thumbs"));

        std::fs::create_dir_all(&cache_dir).ok();

        Self {
            index: RwLock::new(None),
            analysis: RwLock::new(None),
            marks: RwLock::new(HashMap::new()),
            thumbnail_dir: cache_dir,
        }
    }
}
