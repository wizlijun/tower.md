//! Native project-token binding for a fixed, purely observational daemon read.
use crate::{commands::AppError, daemon_client, draft_requests, project::ProjectRegistry};
use std::path::Path;
use towermd_core::app_read_model::{TaskProgressSource, TaskProgressState, TaskProgressView};

pub fn load(
    registry: &ProjectRegistry,
    token: &str,
    app_data: &Path,
) -> Result<TaskProgressView, AppError> {
    load_with(registry, token, || daemon_client::discover(app_data))
}
fn load_with(
    registry: &ProjectRegistry,
    token: &str,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<TaskProgressView, AppError> {
    let catalog = draft_requests::context(registry, token).map_err(|mut error| {
        if error.code == "project_not_initialized" {
            error.message = "任务进度需要权威 main 上的项目配置；本次读取不会初始化项目".into();
        }
        error
    })?;
    let client = connect()?;
    registry.resolve(token)?;
    let view: TaskProgressView = serde_json::from_value(client.task_progress(&catalog.root)?)
        .map_err(|error| AppError::new("protocol_error", format!("任务进度字段不完整：{error}")))?;
    let current = draft_requests::context(registry, token)?;
    if current.common_dir != catalog.common_dir
        || current.root != catalog.root
        || current.revision != catalog.revision
        || current.config.to_yaml() != catalog.config.to_yaml()
        || view.project_id != catalog.config.project_id()
        || view.common_dir != catalog.common_dir.to_string_lossy().as_ref()
        || view.revision != catalog.revision
    {
        return Err(AppError::new(
            "project_changed",
            "所选项目或来源已变化，请重新读取任务进度",
        ));
    }
    let coherent = match (&view.state, &view.source) {
        (TaskProgressState::Recorded, TaskProgressSource::HubJournal) => {
            matches!((view.floor_seq,view.through_seq),(Some(floor),Some(through)) if floor<=through)
        }
        (
            TaskProgressState::RecoveryRequired,
            TaskProgressSource::RecoverySnapshot { commit, reference },
        ) => {
            view.through_seq.is_some()
                && view.floor_seq.is_none()
                && view.runs.is_empty()
                && view.evidence.is_empty()
                && matches!(commit.len(), 40 | 64)
                && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
                && matches!(reference.as_str(), "main" | "refs/towermd/snapshot")
        }
        (TaskProgressState::Unavailable, TaskProgressSource::None) => {
            view.floor_seq.is_none()
                && view.through_seq.is_none()
                && view.tasks.is_empty()
                && view.runs.is_empty()
                && view.evidence.is_empty()
        }
        _ => false,
    };
    if !coherent
        || view.tasks.iter().any(|value| {
            value["type"] != "Task" || !value["id"].is_string() || !value["state"].is_string()
        })
        || view.runs.iter().any(|value| {
            value["type"] != "Run"
                || !value["id"].is_string()
                || !value["task"].is_string()
                || !value["state"].is_string()
        })
        || view
            .evidence
            .iter()
            .any(|value| value["type"] != "Evidence" || !value["id"].is_string())
    {
        return Err(AppError::new(
            "protocol_error",
            "任务进度来源、水位或对象类型不一致",
        ));
    }
    registry.resolve(token)?;
    Ok(view)
}

#[cfg(test)]
#[path = "task_progress_tests.rs"]
mod tests;
