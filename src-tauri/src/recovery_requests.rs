//! App-only adapter for immutable recovery requests issued by the daemon.
//! Views are disposable admission state; the original request remains in the daemon.
use crate::{commands::AppError, daemon_client, draft_requests, project::ProjectRegistry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use towermd_core::{catalog::Catalog, recovery::RecoveryRequest, snapshot_service::SnapshotReport};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRequest {
    request: RecoveryRequest,
    directory: PathBuf,
    common_dir: PathBuf,
    expires_at: u64,
}
#[derive(Default, Clone)]
pub struct RebuildRequests(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    views: BTreeMap<String, Viewed>,
    attempted: BTreeSet<String>,
}
#[derive(Clone)]
struct Viewed {
    token: String,
    directory: PathBuf,
    common_dir: PathBuf,
    stored: StoredRequest,
    actor: String,
    instance: String,
}
#[derive(Serialize)]
pub struct RecoveryView {
    pub request: RecoveryRequest,
    pub human_actor: String,
    pub can_confirm: bool,
}
fn safe_id(id: &str) -> Result<(), AppError> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(AppError::new(
            "recovery_id_invalid",
            "请使用 CLI 已登记的 64 位小写十六进制 request_id",
        ));
    }
    Ok(())
}
fn checked_saved(catalog: &Catalog, id: &str, value: Value) -> Result<StoredRequest, AppError> {
    let saved: StoredRequest = serde_json::from_value(value)
        .map_err(|error| AppError::new("protocol_error", format!("恢复请求结构无效：{error}")))?;
    if saved.request.request_id != id
        || saved.request.project_id != catalog.config.project_id()
        || saved.common_dir != catalog.common_dir
        || saved.expires_at != saved.request.expires_at
        || saved.request.status != "awaiting_app_confirmation"
        || saved.request.expires_at <= saved.request.requested_at
        || saved.request.expires_at - saved.request.requested_at > 300_000
    {
        return Err(AppError::new(
            "recovery_context_changed",
            "原恢复请求的项目、身份或期限与所选项目不一致",
        ));
    }
    let actual = Catalog::load(&saved.directory)
        .map_err(|error| AppError::new("recovery_context_changed", error))?;
    if actual.common_dir != catalog.common_dir
        || actual.config.project_id() != catalog.config.project_id()
    {
        return Err(AppError::new(
            "recovery_context_changed",
            "原恢复请求目录属于另一项目或 clone",
        ));
    }
    Ok(saved)
}
pub fn load(
    registry: &ProjectRegistry,
    requests: &RebuildRequests,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<RecoveryView, AppError> {
    load_with(registry, requests, token, id, || {
        daemon_client::discover(app_data)
    })
}
fn load_with(
    registry: &ProjectRegistry,
    requests: &RebuildRequests,
    token: &str,
    id: &str,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<RecoveryView, AppError> {
    safe_id(id)?;
    let catalog = draft_requests::context(registry, token)?;
    let actor = draft_requests::identity()?;
    let client = connect()?;
    registry.resolve(token)?;
    let stored = checked_saved(&catalog, id, client.saved_rebuild(id)?)?;
    registry.resolve(token)?;
    let mut state = requests.0.lock().map_err(draft_requests::io_error)?;
    let can_confirm = !state.attempted.contains(id);
    state.views.insert(
        id.into(),
        Viewed {
            token: token.into(),
            directory: catalog.root,
            common_dir: catalog.common_dir,
            stored: stored.clone(),
            actor: actor.clone(),
            instance: client.instance().into(),
        },
    );
    Ok(RecoveryView {
        request: stored.request,
        human_actor: actor,
        can_confirm,
    })
}
pub fn confirm(
    registry: &ProjectRegistry,
    requests: &RebuildRequests,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<SnapshotReport, AppError> {
    confirm_with(registry, requests, token, id, || {
        daemon_client::discover(app_data)
    })
}
fn confirm_with(
    registry: &ProjectRegistry,
    requests: &RebuildRequests,
    token: &str,
    id: &str,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<SnapshotReport, AppError> {
    safe_id(id)?;
    let viewed = {
        let state = requests.0.lock().map_err(draft_requests::io_error)?;
        if state.attempted.contains(id) {
            return Err(AppError::new("recovery_already_attempted","此 ID 已尝试确认；保留原请求审计，不能再次安装。需要再次恢复时请运行 CLI 取得新 ID"));
        }
        state.views.get(id).cloned().ok_or_else(|| {
            AppError::new("recovery_not_viewed", "请先读取并审阅已登记的原恢复请求")
        })?
    };
    let catalog = draft_requests::context(registry, token)?;
    if viewed.token != token
        || viewed.directory != catalog.root
        || viewed.common_dir != catalog.common_dir
        || viewed.actor != draft_requests::identity()?
    {
        return Err(AppError::new(
            "recovery_context_changed",
            "原恢复请求的项目或本机人工身份已变化，请重新读取",
        ));
    }
    let client = connect()?;
    registry.resolve(token)?;
    if viewed.instance != client.instance() {
        return Err(AppError::new(
            "recovery_conflict",
            "daemon 实例已变化；请重新读取原请求，不能沿旧实例确认",
        ));
    }
    let saved = checked_saved(&catalog, id, client.saved_rebuild(id)?)?;
    if saved != viewed.stored {
        return Err(AppError::new(
            "recovery_conflict",
            "已查看的原恢复请求发生变化；不会替换来源或延长期限",
        ));
    }
    registry.resolve(token)?;
    {
        let mut state = requests.0.lock().map_err(draft_requests::io_error)?;
        if !state.attempted.insert(id.into()) {
            return Err(AppError::new(
                "recovery_already_attempted",
                "原恢复请求已开始确认，不会重复提交",
            ));
        }
    }
    let value = client.confirm_rebuild(id, &viewed.actor)?;
    registry.resolve(token)?;
    let report: SnapshotReport = serde_json::from_value(value).map_err(|error| {
        AppError::new(
            "recovery_uncertain",
            format!("恢复响应无法核实，不会自动重试：{error}"),
        )
    })?;
    let valid_commit = report.commit.as_deref().is_some_and(|commit| {
        matches!(commit.len(), 40 | 64)
            && commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });
    if report.status != "rebuilt"
        || !valid_commit
        || report.seq <= saved.request.seq_boundary
        || report.epoch <= saved.request.epoch_boundary
        || report.reason.is_some()
    {
        return Err(AppError::new(
            "recovery_uncertain",
            "恢复响应缺少真实提交或更高边界；不能推断已完成，也不会自动重试",
        ));
    }
    Ok(report)
}
#[cfg(test)]
#[path = "recovery_requests_tests.rs"]
mod tests;
