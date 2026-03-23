use std::sync::Arc;
use tauri::State;

use crate::state::{AppState, Mark};

#[derive(serde::Serialize)]
pub struct DeleteResult {
    pub deleted: usize,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn execute_deletes(
    state: State<'_, Arc<AppState>>,
) -> Result<DeleteResult, String> {
    let marks = state.marks.read().map_err(|e| e.to_string())?;
    let index_guard = state.index.read().map_err(|e| e.to_string())?;
    let index = index_guard.as_ref().ok_or("No images indexed")?;

    let to_delete: Vec<String> = marks
        .iter()
        .filter(|(_, mark)| **mark == Mark::Delete)
        .map(|(id, _)| id.clone())
        .collect();

    let mut deleted = 0;
    let mut errors = Vec::new();
    let mut failed_ids = std::collections::HashSet::new();

    for id in &to_delete {
        if let Some(image) = index.get(id) {
            match trash::delete(&image.path) {
                Ok(()) => deleted += 1,
                Err(e) => {
                    errors.push(format!("{}: {}", image.file_name, e));
                    failed_ids.insert(id.clone());
                }
            }
        }
    }

    // Remove marks only for successfully deleted images
    drop(marks);
    let mut marks = state.marks.write().map_err(|e| e.to_string())?;
    for id in &to_delete {
        if !failed_ids.contains(id) {
            marks.remove(id);
        }
    }

    Ok(DeleteResult { deleted, errors })
}
