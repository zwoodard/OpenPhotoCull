pub mod commands;
pub mod imaging;
pub mod index;
pub mod pipeline;
pub mod state;
pub mod thumbnail;

use std::sync::Arc;

pub fn run() {
    tracing_subscriber::fmt::init();

    let app_state = Arc::new(state::AppState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::scan::scan_folder,
            commands::analyze::run_analysis,
            commands::analyze::get_duplicate_groups,
            commands::analyze::get_scene_groups,
            commands::analyze::get_person_groups,
            commands::review::set_mark,
            commands::review::bulk_set_mark,
            commands::review::get_full_image_path,
            commands::export::execute_deletes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
