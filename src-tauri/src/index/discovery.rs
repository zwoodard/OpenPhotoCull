use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "tiff", "tif", "webp", "heic", "heif", "cr2", "cr3", "nef", "arw",
    "dng", "orf", "rw2", "raf", "pef", "srw",
];

pub struct DiscoveredImage {
    pub path: PathBuf,
    pub file_size: u64,
    pub modified_at: u64,
}

pub fn discover_images(root: &Path) -> Vec<DiscoveredImage> {
    let mut images = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());

        if let Some(ext) = ext {
            if SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
                if let Ok(meta) = entry.metadata() {
                    let modified_at = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    images.push(DiscoveredImage {
                        path: path.to_path_buf(),
                        file_size: meta.len(),
                        modified_at,
                    });
                }
            }
        }
    }

    images
}
