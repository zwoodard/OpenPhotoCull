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

/// Re-run duplicate, scene, and person grouping with new thresholds.
/// Uses cached phashes/embeddings from the last scan — no image re-decoding needed.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegroupParams {
    pub duplicate_threshold: u32,
    pub scene_window_secs: i64,
    pub person_similarity: f32,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegroupResult {
    pub duplicate_groups: HashMap<String, Vec<String>>,
    pub scene_groups: HashMap<String, Vec<String>>,
    pub person_groups: HashMap<String, Vec<PersonGroupEntry>>,
    /// Updated per-image analysis with new group IDs
    pub analysis: HashMap<String, AnalysisResults>,
}

#[tauri::command]
pub async fn regroup(
    params: RegroupParams,
    state: State<'_, Arc<AppState>>,
) -> Result<RegroupResult, String> {
    use crate::commands::scan::{find_duplicate_groups, find_scene_groups, DupEntry};

    let gdata = state.grouping_data.read().map_err(|e| e.to_string())?;
    if gdata.is_empty() {
        return Err("No scan data available. Scan a folder first.".into());
    }

    // Build DupEntries from cached data
    let dup_entries: Vec<DupEntry> = gdata
        .iter()
        .enumerate()
        .map(|(i, g)| DupEntry {
            index: i,
            timestamp: g.timestamp,
            phash: g.phash.clone(),
        })
        .collect();

    let duplicate_groups =
        find_duplicate_groups(&dup_entries, 5, params.duplicate_threshold);
    let scene_groups =
        find_scene_groups(&dup_entries, params.scene_window_secs);

    // Face re-clustering
    let face_entries: Vec<(String, u32, Vec<f32>)> = gdata
        .iter()
        .flat_map(|g| {
            g.face_embeddings
                .iter()
                .enumerate()
                .map(|(fi, emb)| (g.image_id.clone(), fi as u32, emb.clone()))
        })
        .collect();
    let person_groups =
        crate::pipeline::face_grouping::cluster_faces(&face_entries, params.person_similarity);

    // Build face lookup
    let mut face_person_map: HashMap<(String, u32), String> = HashMap::new();
    for (pid, members) in &person_groups {
        for (img_id, fi) in members {
            face_person_map.insert((img_id.clone(), *fi), pid.clone());
        }
    }

    // Update analysis in state
    let mut analysis_guard = state.analysis.write().map_err(|e| e.to_string())?;
    let analysis = analysis_guard.as_mut().ok_or("No analysis results yet")?;

    let mut dup_group_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut scene_group_map: HashMap<String, Vec<String>> = HashMap::new();

    for (i, g) in gdata.iter().enumerate() {
        if let Some(result) = analysis.results.get_mut(&g.image_id) {
            // Update duplicate group
            result.duplicate_group_id = duplicate_groups.get(&i).cloned();
            if let Some(gid) = &result.duplicate_group_id {
                dup_group_map
                    .entry(gid.clone())
                    .or_default()
                    .push(g.image_id.clone());
            }

            // Update scene group
            result.scene_group_id = scene_groups.get(&i).cloned();
            if let Some(sid) = &result.scene_group_id {
                scene_group_map
                    .entry(sid.clone())
                    .or_default()
                    .push(g.image_id.clone());
            }

            // Update face person IDs
            if let Some(ref mut faces) = result.faces {
                for face in faces.iter_mut() {
                    face.person_id =
                        face_person_map.get(&(g.image_id.clone(), face.face_index)).cloned();
                }
            }
        }
    }

    analysis.duplicate_groups = dup_group_map.clone();
    analysis.scene_groups = scene_group_map.clone();
    analysis.person_groups = person_groups.clone();

    let updated_analysis = analysis.results.clone();

    // Build person group response
    let person_group_entries: HashMap<String, Vec<PersonGroupEntry>> = person_groups
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
        .collect();

    Ok(RegroupResult {
        duplicate_groups: dup_group_map,
        scene_groups: scene_group_map,
        person_groups: person_group_entries,
        analysis: updated_analysis,
    })
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
