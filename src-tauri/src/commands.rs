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
