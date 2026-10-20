#[derive(Debug, serde::Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: None,
        }
    }
}

fn intervention_app_data(app: &tauri::AppHandle) -> Result<std::path::PathBuf, AppError> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map_err(|error| AppError::new("app_data_unavailable", error.to_string()))
}

#[tauri::command]
pub async fn load_interventions(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::intervention_requests::InterventionRequests>,
    token: String,
) -> Result<serde_json::Value, AppError> {
    let data = intervention_app_data(&app)?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::intervention_requests::load(&registry, &requests, &token, &data)
    })
    .await
    .map_err(|error| AppError::new("control_task_failed", error.to_string()))?
}
#[tauri::command]
pub async fn prepare_intervention(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::intervention_requests::InterventionRequests>,
    token: String,
    action_ticket: String,
    action: towermd_core::intervention_control::AppControlAction,
) -> Result<crate::intervention_requests::PreparedIntervention, AppError> {
    let data = intervention_app_data(&app)?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::intervention_requests::prepare(
            &registry,
            &requests,
            &token,
            &action_ticket,
            action,
            &data,
        )
    })
    .await
    .map_err(|error| AppError::new("control_task_failed", error.to_string()))?
}
#[tauri::command]
pub async fn pending_interventions(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::intervention_requests::InterventionRequests>,
    token: String,
) -> Result<Vec<crate::intervention_requests::PreparedIntervention>, AppError> {
    let data = intervention_app_data(&app)?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::intervention_requests::pending(&registry, &requests, &token, &data)
    })
    .await
    .map_err(|error| AppError::new("control_task_failed", error.to_string()))?
}
#[tauri::command]
pub async fn submit_intervention(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::intervention_requests::InterventionRequests>,
    token: String,
    request_id: String,
) -> Result<crate::draft_requests::EditOutcome, AppError> {
    let data = intervention_app_data(&app)?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::intervention_requests::submit(&registry, &requests, &token, &request_id, &data)
    })
    .await
    .map_err(|error| AppError::new("control_task_failed", error.to_string()))?
}
#[tauri::command]
pub async fn request_resume(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::intervention_requests::InterventionRequests>,
    token: String,
) -> Result<(), AppError> {
    let data = intervention_app_data(&app)?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::intervention_requests::request_resume(&registry, &requests, &token, &data)
    })
    .await
    .map_err(|error| AppError::new("control_task_failed", error.to_string()))?
}

#[tauri::command]
pub async fn choose_project(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
) -> Result<Option<crate::project::SelectedProject>, AppError> {
    use tauri_plugin_dialog::DialogExt;
    let registry = registry.inner().clone();
    let request = registry.begin_selection()?;
    tauri::async_runtime::spawn_blocking(move || {
        let selection = app
            .dialog()
            .file()
            .blocking_pick_folder()
            .map(|path| {
                path.into_path()
                    .map_err(|error| AppError::new("project_path_invalid", error.to_string()))
            })
            .transpose()?;
        registry.finish_selection(request, selection)
    })
    .await
    .map_err(|error| AppError::new("project_task_failed", format!("选择项目失败：{error}")))?
}

#[tauri::command]
pub async fn initialize_project(
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    token: String,
    docs_dir: String,
) -> Result<crate::project::InitOutput, AppError> {
    crate::project::initialize(registry.inner().clone(), token, docs_dir).await
}

#[tauri::command]
pub async fn load_project_overview(
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    token: String,
) -> Result<towermd_core::project_overview::ProjectOverview, AppError> {
    let registry = registry.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::project::overview(&registry, &token))
        .await
        .map_err(|error| AppError::new("project_task_failed", format!("项目读取失败：{error}")))?
}

#[tauri::command]
pub async fn start_candidate_scan(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    scans: tauri::State<'_, crate::daemon_client::CandidateScans>,
    token: String,
) -> Result<serde_json::Value, AppError> {
    use tauri::Manager;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let scans = scans.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::daemon_client::start(&registry, &scans, &token, &app_data)
    })
    .await
    .map_err(|e| AppError::new("scan_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn wait_candidate_scan(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    scans: tauri::State<'_, crate::daemon_client::CandidateScans>,
    token: String,
    scan_id: String,
) -> Result<serde_json::Value, AppError> {
    use tauri::Manager;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let scans = scans.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::daemon_client::wait(&registry, &scans, &token, &scan_id, &app_data)
    })
    .await
    .map_err(|e| AppError::new("scan_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_candidate_draft(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    scans: tauri::State<'_, crate::daemon_client::CandidateScans>,
    token: String,
    scan_id: String,
) -> Result<serde_json::Value, AppError> {
    use tauri::Manager;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let scans = scans.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::daemon_client::draft(&registry, &scans, &token, &scan_id, &app_data)
    })
    .await
    .map_err(|e| AppError::new("scan_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_draft(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    token: String,
    id: String,
) -> Result<serde_json::Value, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::draft_requests::load(&registry, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("draft_task_failed", e.to_string()))?
}
#[tauri::command]
pub async fn prepare_draft_edit(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::draft_requests::DraftRequests>,
    token: String,
    id: String,
    expected_revision: String,
    edit: towermd_core::baseline_draft::DraftEdit,
) -> Result<crate::draft_requests::PreparedEdit, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::draft_requests::prepare(
            &registry,
            &requests,
            &token,
            &id,
            &expected_revision,
            edit,
            &data,
        )
    })
    .await
    .map_err(|e| AppError::new("draft_task_failed", e.to_string()))?
}
#[tauri::command]
pub async fn pending_draft_edits(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::draft_requests::DraftRequests>,
    token: String,
    id: String,
) -> Result<Vec<crate::draft_requests::PreparedEdit>, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::draft_requests::pending(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("draft_task_failed", e.to_string()))?
}
#[tauri::command]
pub async fn submit_draft_edit(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::draft_requests::DraftRequests>,
    token: String,
    request_id: String,
) -> Result<crate::draft_requests::EditOutcome, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::draft_requests::submit(&registry, &requests, &token, &request_id, &data)
    })
    .await
    .map_err(|e| AppError::new("draft_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_baseline_review(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::review_requests::ReviewRequests>,
    token: String,
    id: String,
) -> Result<serde_json::Value, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::review_requests::load(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn prepare_baseline_review(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::review_requests::ReviewRequests>,
    token: String,
    review_ticket: String,
    verdict: towermd_core::baseline_review::Verdict,
    comment: String,
) -> Result<crate::review_requests::PreparedReview, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::review_requests::prepare(
            &registry,
            &requests,
            &token,
            &review_ticket,
            verdict,
            comment,
            &data,
        )
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn pending_baseline_reviews(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::review_requests::ReviewRequests>,
    token: String,
    id: String,
) -> Result<Vec<crate::review_requests::PreparedReview>, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::review_requests::pending(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn submit_baseline_review(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::review_requests::ReviewRequests>,
    token: String,
    request_id: String,
) -> Result<crate::draft_requests::EditOutcome, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::review_requests::submit(&registry, &requests, &token, &request_id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_current_baseline(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    token: String,
) -> Result<Option<towermd_core::baseline_current::CurrentBaseline>, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::draft_requests::load_current(&registry, &token, &data)
    })
    .await
    .map_err(|e| AppError::new("draft_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn prepare_create_baseline_draft(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::draft_requests::DraftRequests>,
    token: String,
    baseline: String,
    main_revision: String,
) -> Result<crate::draft_requests::PreparedCreation, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::draft_requests::prepare_create(
            &registry,
            &requests,
            &token,
            &baseline,
            &main_revision,
            &data,
        )
    })
    .await
    .map_err(|e| AppError::new("draft_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn pending_draft_creations(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::draft_requests::DraftRequests>,
    token: String,
) -> Result<Vec<crate::draft_requests::PreparedCreation>, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::draft_requests::pending_creations(&registry, &requests, &token, &data)
    })
    .await
    .map_err(|e| AppError::new("draft_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn submit_create_baseline_draft(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::draft_requests::DraftRequests>,
    token: String,
    request_id: String,
) -> Result<crate::draft_requests::EditOutcome, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::draft_requests::submit_create(&registry, &requests, &token, &request_id, &data)
    })
    .await
    .map_err(|e| AppError::new("draft_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_rebuild_request(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::recovery_requests::RebuildRequests>,
    token: String,
    id: String,
) -> Result<crate::recovery_requests::RecoveryView, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::new("app_data_unavailable", error.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::recovery_requests::load(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|error| AppError::new("recovery_task_failed", error.to_string()))?
}
#[tauri::command]
pub async fn confirm_rebuild(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::recovery_requests::RebuildRequests>,
    token: String,
    id: String,
) -> Result<towermd_core::snapshot_service::SnapshotReport, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::new("app_data_unavailable", error.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::recovery_requests::confirm(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|error| AppError::new("recovery_task_failed", error.to_string()))?
}

#[tauri::command]
pub async fn load_task_review(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_review_requests::TaskReviewRequests>,
    token: String,
    id: String,
) -> Result<serde_json::Value, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_review_requests::load(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn prepare_task_review(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_review_requests::TaskReviewRequests>,
    token: String,
    review_ticket: String,
    verdict: towermd_core::recovery_contract::task_review::ReviewVerdict,
    comment: String,
) -> Result<crate::task_review_requests::PreparedTaskReview, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_review_requests::prepare(
            &registry,
            &requests,
            &token,
            &review_ticket,
            verdict,
            comment,
            &data,
        )
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn pending_task_reviews(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_review_requests::TaskReviewRequests>,
    token: String,
    id: String,
) -> Result<Vec<crate::task_review_requests::PreparedTaskReview>, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_review_requests::pending(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn submit_task_review(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_review_requests::TaskReviewRequests>,
    token: String,
    request_id: String,
) -> Result<crate::draft_requests::EditOutcome, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_review_requests::submit(&registry, &requests, &token, &request_id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_task_recheck(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_recheck_requests::TaskRecheckRequests>,
    token: String,
    id: String,
) -> Result<serde_json::Value, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_recheck_requests::load(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("recheck_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_task_recheck_history(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_recheck_requests::TaskRecheckRequests>,
    token: String,
    id: String,
) -> Result<Vec<crate::task_recheck_requests::PreparedTaskRecheck>, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_recheck_requests::history(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("recheck_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn prepare_task_recheck(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_recheck_requests::TaskRecheckRequests>,
    token: String,
    recheck_ticket: String,
    comment: String,
) -> Result<crate::task_recheck_requests::PreparedTaskRecheck, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_recheck_requests::prepare(
            &registry,
            &requests,
            &token,
            &recheck_ticket,
            comment,
            &data,
        )
    })
    .await
    .map_err(|e| AppError::new("recheck_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn pending_task_rechecks(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_recheck_requests::TaskRecheckRequests>,
    token: String,
    id: String,
) -> Result<Vec<crate::task_recheck_requests::PreparedTaskRecheck>, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_recheck_requests::pending(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("recheck_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn submit_task_recheck(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::task_recheck_requests::TaskRecheckRequests>,
    token: String,
    request_id: String,
) -> Result<crate::draft_requests::EditOutcome, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_recheck_requests::submit(&registry, &requests, &token, &request_id, &data)
    })
    .await
    .map_err(|e| AppError::new("recheck_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_ask_answer(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::ask_answer_requests::AskAnswerRequests>,
    token: String,
    id: String,
) -> Result<serde_json::Value, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::ask_answer_requests::load(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn prepare_ask_answer(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::ask_answer_requests::AskAnswerRequests>,
    token: String,
    answer_ticket: String,
    text: String,
) -> Result<crate::ask_answer_requests::PreparedAskAnswer, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::ask_answer_requests::prepare(
            &registry,
            &requests,
            &token,
            &answer_ticket,
            text,
            &data,
        )
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn pending_ask_answers(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::ask_answer_requests::AskAnswerRequests>,
    token: String,
    id: String,
) -> Result<Vec<crate::ask_answer_requests::PreparedAskAnswer>, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::ask_answer_requests::pending(&registry, &requests, &token, &id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn submit_ask_answer(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    requests: tauri::State<'_, crate::ask_answer_requests::AskAnswerRequests>,
    token: String,
    request_id: String,
) -> Result<crate::draft_requests::EditOutcome, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    let registry = registry.inner().clone();
    let requests = requests.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::ask_answer_requests::submit(&registry, &requests, &token, &request_id, &data)
    })
    .await
    .map_err(|e| AppError::new("review_task_failed", e.to_string()))?
}

#[tauri::command]
pub async fn load_task_progress(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    observations: tauri::State<'_, crate::canvas_observations::CanvasObservations>,
    token: String,
) -> Result<crate::canvas_observations::ObservedProgress, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::new("app_data_unavailable", error.to_string()))?;
    let registry = registry.inner().clone();
    let observations = observations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let progress = crate::task_progress::load(&registry, &token, &data)?;
        registry.resolve(&token)?;
        let observation = observations.observe(&token, &progress, &data);
        Ok(crate::canvas_observations::ObservedProgress {
            progress,
            observation,
        })
    })
    .await
    .map_err(|error| AppError::new("task_progress_unavailable", error.to_string()))?
}

#[tauri::command]
pub async fn open_task_progress_stream(
    app: tauri::AppHandle,
    registry: tauri::State<'_, crate::project::ProjectRegistry>,
    token: String,
    after: u64,
) -> Result<crate::task_progress::ProgressStream, AppError> {
    use tauri::Manager;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::new("app_data_unavailable", error.to_string()))?;
    let registry = registry.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::task_progress::stream(&registry, &token, after, &data)
    })
    .await
    .map_err(|error| AppError::new("task_progress_unavailable", error.to_string()))?
}
