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
