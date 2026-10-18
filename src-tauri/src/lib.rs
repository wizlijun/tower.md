pub mod cli;
pub mod commands;
pub mod daemon_client;
pub mod project;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(project::ProjectRegistry::default())
        .manage(daemon_client::CandidateScans::default())
        .invoke_handler(tauri::generate_handler![
            commands::choose_project,
            commands::initialize_project,
            commands::load_project_overview,
            commands::start_candidate_scan,
            commands::wait_candidate_scan,
            commands::load_candidate_draft
        ])
        .run(tauri::generate_context!())
        .expect("Tower.md 桌面运行失败");
}
