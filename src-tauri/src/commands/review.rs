use std::sync::Arc;
use tauri::State;

use crate::state::{AppState, Mark};

#[tauri::command]
pub async fn set_mark(
    image_id: String,
    mark: Mark,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut marks = state.marks.write().map_err(|e| e.to_string())?;
    marks.insert(image_id, mark);
    Ok(())
}

#[tauri::command]
pub async fn bulk_set_mark(
    image_ids: Vec<String>,
    mark: Mark,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut marks = state.marks.write().map_err(|e| e.to_string())?;
    for id in image_ids {
        marks.insert(id, mark.clone());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_full_image_path(
    image_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let index_guard = state.index.read().map_err(|e| e.to_string())?;
    let index = index_guard.as_ref().ok_or("No images indexed")?;
    let image = index.get(&image_id).ok_or("Image not found")?;
    Ok(image.path.clone())
}
