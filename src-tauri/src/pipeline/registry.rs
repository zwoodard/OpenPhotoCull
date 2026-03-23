use std::collections::HashMap;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub elapsed_ms: u64,
    pub current_file: Option<String>,
    pub step_timings: Option<HashMap<String, u64>>,
}
