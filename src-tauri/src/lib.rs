pub mod cli;
pub mod commands;
pub mod project;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(project::ProjectRegistry::default())
        .invoke_handler(tauri::generate_handler![
            commands::choose_project,
            commands::initialize_project
        ])
        .run(tauri::generate_context!())
        .expect("Tower.md 桌面运行失败");
}
