pub mod cli;
pub mod commands;
pub mod daemon_client;
pub mod draft_requests;
pub mod project;
pub mod recovery_requests;
mod review_canvas;
pub mod review_requests;
pub mod task_review_requests;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(project::ProjectRegistry::default())
        .manage(draft_requests::DraftRequests::default())
        .manage(review_requests::ReviewRequests::default())
        .manage(task_review_requests::TaskReviewRequests::default())
        .manage(recovery_requests::RebuildRequests::default())
        .manage(daemon_client::CandidateScans::default())
        .invoke_handler(tauri::generate_handler![
            commands::choose_project,
            commands::initialize_project,
            commands::load_project_overview,
            commands::load_rebuild_request,
            commands::confirm_rebuild,
            commands::start_candidate_scan,
            commands::wait_candidate_scan,
            commands::load_candidate_draft,
            commands::load_draft,
            commands::load_current_baseline,
            commands::prepare_create_baseline_draft,
            commands::pending_draft_creations,
            commands::submit_create_baseline_draft,
            commands::prepare_draft_edit,
            commands::pending_draft_edits,
            commands::submit_draft_edit,
            commands::load_baseline_review,
            commands::prepare_baseline_review,
            commands::pending_baseline_reviews,
            commands::submit_baseline_review,
            commands::load_task_review,
            commands::prepare_task_review,
            commands::pending_task_reviews,
            commands::submit_task_review
        ])
        .run(tauri::generate_context!())
        .expect("Tower.md 桌面运行失败");
}
