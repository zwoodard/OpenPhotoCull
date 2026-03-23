use std::collections::HashMap;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;

use crate::index::store::AnalysisResults;
use crate::pipeline::registry::ProgressEvent;
use crate::state::AppState;

#[tauri::command]
pub async fn run_analysis(
    on_progress: Channel<ProgressEvent>,
    state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, AnalysisResults>, String> {
    // Analysis is now done during scan_folder in a single pass.
    // This command just returns the already-computed results.
    let analysis_guard = state.analysis.read().map_err(|e| e.to_string())?;
    let analysis = analysis_guard.as_ref().ok_or("No analysis results yet")?;

    on_progress
        .send(ProgressEvent {
            phase: "Analysis complete (computed during scan)".into(),
            current: analysis.results.len(),
            total: analysis.results.len(),
            elapsed_ms: 0,
            current_file: None,
            step_timings: None,
        })
        .ok();

    Ok(analysis.results.clone())
}

#[tauri::command]
pub async fn get_duplicate_groups(
    state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, Vec<String>>, String> {
    let analysis_guard = state.analysis.read().map_err(|e| e.to_string())?;
    let analysis = analysis_guard.as_ref().ok_or("No analysis results yet")?;
    Ok(analysis.duplicate_groups.clone())
}

#[tauri::command]
pub async fn get_scene_groups(
    state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, Vec<String>>, String> {
    let analysis_guard = state.analysis.read().map_err(|e| e.to_string())?;
    let analysis = analysis_guard.as_ref().ok_or("No analysis results yet")?;
    Ok(analysis.scene_groups.clone())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonGroupEntry {
    pub image_id: String,
    pub face_index: u32,
}

#[tauri::command]
pub async fn get_person_groups(
    state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, Vec<PersonGroupEntry>>, String> {
    let analysis_guard = state.analysis.read().map_err(|e| e.to_string())?;
    let analysis = analysis_guard.as_ref().ok_or("No analysis results yet")?;
    Ok(analysis
        .person_groups
        .iter()
        .map(|(pid, members)| {
            (
                pid.clone(),
                members
                    .iter()
                    .map(|(img_id, fi)| PersonGroupEntry {
                        image_id: img_id.clone(),
                        face_index: *fi,
                    })
                    .collect(),
            )
        })
        .collect())
}
