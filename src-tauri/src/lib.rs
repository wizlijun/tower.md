pub mod ask_answer_requests;
pub mod canvas_observations;
pub mod cli;
pub mod commands;
pub mod daemon_client;
pub mod draft_requests;
pub mod intervention_requests;
pub mod project;
pub mod recovery_requests;
mod review_canvas;
pub mod review_requests;
pub mod task_progress;
pub mod task_recheck_requests;
pub mod task_review_requests;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(project::ProjectRegistry::default())
        .manage(canvas_observations::CanvasObservations::default())
        .manage(draft_requests::DraftRequests::default())
        .manage(review_requests::ReviewRequests::default())
        .manage(task_review_requests::TaskReviewRequests::default())
        .manage(task_recheck_requests::TaskRecheckRequests::default())
        .manage(ask_answer_requests::AskAnswerRequests::default())
        .manage(intervention_requests::InterventionRequests::default())
        .manage(recovery_requests::RebuildRequests::default())
        .manage(daemon_client::CandidateScans::default())
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                use tauri::Manager;
                if let Ok(data) = window.app_handle().path().app_data_dir() {
                    if let Err(error) = window
                        .state::<canvas_observations::CanvasObservations>()
                        .close_all(&data)
                    {
                        eprintln!("Tower.md canvas observation: {}", error.message);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::choose_project,
            commands::initialize_project,
            commands::load_project_overview,
            commands::load_task_progress,
            commands::open_task_progress_stream,
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
            commands::load_task_recheck,
            commands::prepare_task_recheck,
            commands::pending_task_rechecks,
            commands::submit_task_recheck,
            commands::load_task_review,
            commands::prepare_task_review,
            commands::pending_task_reviews,
            commands::submit_task_review,
            commands::load_ask_answer,
            commands::prepare_ask_answer,
            commands::pending_ask_answers,
            commands::submit_ask_answer,
            commands::load_interventions,
            commands::prepare_intervention,
            commands::pending_interventions,
            commands::submit_intervention,
            commands::request_resume
        ])
        .run(tauri::generate_context!())
        .expect("Tower.md 桌面运行失败");
}
