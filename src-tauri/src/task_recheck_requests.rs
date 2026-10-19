//! Fixed native Task recheck transport. Tickets bind a displayed original review;
//! durable retries preserve the UUID, body, binding and locally derived actor.
use crate::{
    commands::AppError,
    daemon_client,
    draft_requests::{self, io_error, write_new},
    project::ProjectRegistry,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use towermd_core::{
    proposals::{
        RecheckRequest, ResolveRecheckAction, TaskRecheckAction, TaskRecheckBinding,
        TaskRecheckOutcome, TaskRecheckView,
    },
    recovery_contract::recorded::{request_event_eid, RequestBody, RequestMeta},
};

#[derive(Default, Clone)]
pub struct TaskRecheckRequests(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    views: BTreeMap<String, Viewed>,
    bound: BTreeMap<String, String>,
    inflight: BTreeSet<String>,
    preparing: BTreeSet<(String, PathBuf, String)>,
}
struct Viewed {
    token: String,
    directory: PathBuf,
    common_dir: PathBuf,
    instance: String,
    host: String,
    actor: String,
    view: TaskRecheckView,
    revision: String,
    source: towermd_core::recovery_outbox::RecoveryContext,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedRecheck {
    schema: u32,
    directory: PathBuf,
    common_dir: PathBuf,
    request: RequestBody<ResolveRecheckAction>,
    metadata: RequestMeta,
    subject: TaskRecheckView,
}
#[derive(Clone, Serialize)]
pub struct PreparedTaskRecheck {
    pub request_id: String,
    pub binding: TaskRecheckBinding,
    pub comment: String,
    pub human_actor: String,
    pub body_hash: String,
    pub subject: TaskRecheckView,
}
#[derive(Serialize)]
pub(crate) struct TaskRecheckHttpRequest<'a> {
    pub directory: &'a Path,
    pub human_id: &'a str,
    pub action: TaskRecheckAction,
}
fn action(stored: &QueuedRecheck) -> (&TaskRecheckBinding, &str) {
    let ResolveRecheckAction::ResolveTaskRecheck { binding, comment } = &stored.request.action;
    (binding, comment)
}
fn summary(stored: &QueuedRecheck) -> PreparedTaskRecheck {
    let (binding, comment) = action(stored);
    PreparedTaskRecheck {
        request_id: stored.metadata.request_id.clone(),
        binding: binding.clone(),
        comment: comment.into(),
        human_actor: stored.request.actor.clone(),
        body_hash: stored.metadata.body_hash.clone(),
        subject: stored.subject.clone(),
    }
}
fn original_action(stored: &QueuedRecheck) -> TaskRecheckAction {
    let (binding, comment) = action(stored);
    TaskRecheckAction {
        request_id: stored.metadata.request_id.clone(),
        binding: binding.clone(),
        comment: comment.into(),
    }
}
fn path(app_data: &Path, id: &str) -> Result<PathBuf, AppError> {
    let validated = draft_requests::request_path(app_data, id)?;
    let parent = validated.parent().ok_or_else(|| io_error("请求无父目录"))?;
    let queue = parent.join("task-rechecks");
    fs::create_dir_all(&queue).map_err(io_error)?;
    if !fs::symlink_metadata(&queue)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("Task 复核队列不是普通目录"));
    }
    fs::File::open(parent)
        .and_then(|p| p.sync_all())
        .map_err(io_error)?;
    Ok(queue.join(
        validated
            .file_name()
            .ok_or_else(|| io_error("请求无文件名"))?,
    ))
}
fn read(app_data: &Path, id: &str) -> Result<QueuedRecheck, AppError> {
    let file = path(app_data, id)?;
    let meta = fs::symlink_metadata(&file).map_err(io_error)?;
    if !meta.file_type().is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err(io_error("原 Task 复核请求文件类型或大小无效"));
    }
    let mut bytes = vec![];
    fs::File::open(file)
        .map_err(io_error)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let stored: QueuedRecheck = serde_json::from_slice(&bytes).map_err(io_error)?;
    stored.subject.validate().map_err(io_error)?;
    if stored.subject.binding != *action(&stored).0 {
        return Err(io_error("原请求与持久化 Task 原件绑定不同"));
    }
    let original = RecheckRequest::prepare(
        &stored.request.project_id,
        &stored.request.host,
        &stored.request.actor,
        &original_action(&stored),
    )
    .map_err(io_error)?;
    if stored.schema != 1
        || stored.metadata.request_id != id
        || original.body() != &stored.request
        || original.metadata() != &stored.metadata
    {
        return Err(io_error("原 Task 复核正文与共享 canonical hash 不一致"));
    }
    Ok(stored)
}
fn check(catalog: &towermd_core::catalog::Catalog, stored: &QueuedRecheck) -> Result<(), AppError> {
    if catalog.config.project_id() != stored.request.project_id
        || catalog.root != stored.directory
        || catalog.common_dir != stored.common_dir
    {
        return Err(AppError::new(
            "request_context_changed",
            "项目或工作树已变化；不会替换原 Task 复核正文",
        ));
    }
    Ok(())
}
fn checked_view(id: &str, value: Value) -> Result<TaskRecheckView, AppError> {
    let view: TaskRecheckView = serde_json::from_value(value).map_err(io_error)?;
    // Shared Core verifies Task text/hash and the entire publication/reason set.
    view.validate().map_err(io_error)?;
    if view.binding.task_id != id {
        return Err(AppError::new(
            "protocol_error",
            "Task 复核原件不是该任务的真实未决复核",
        ));
    }
    Ok(view)
}
fn check_source_identity(
    registry: &ProjectRegistry,
    token: &str,
    expected: &towermd_core::catalog::Catalog,
    exact_revision: bool,
) -> Result<(), AppError> {
    let current = draft_requests::context(registry, token)?;
    if current.root != expected.root
        || current.common_dir != expected.common_dir
        || current.config.project_id() != expected.config.project_id()
        || (exact_revision && current.revision != expected.revision)
        || towermd_core::recovery_outbox::RecoveryContext::from_catalog(&current)
            .map_err(io_error)?
            != towermd_core::recovery_outbox::RecoveryContext::from_catalog(expected)
                .map_err(io_error)?
    {
        return Err(AppError::new(
            "request_context_changed",
            "Task 复核来源已变化；保留原请求",
        ));
    }
    Ok(())
}
fn check_source(
    registry: &ProjectRegistry,
    token: &str,
    expected: &towermd_core::catalog::Catalog,
) -> Result<(), AppError> {
    check_source_identity(registry, token, expected, true)
}
struct Preparing<'a> {
    requests: &'a TaskRecheckRequests,
    key: (String, PathBuf, String),
}
impl Drop for Preparing<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.requests.0.lock() {
            state.preparing.remove(&self.key);
        }
    }
}
pub fn load(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
) -> Result<Value, AppError> {
    load_with(registry, requests, token, hold, app_data, || {
        daemon_client::discover(app_data)
    })
}
fn load_with(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<Value, AppError> {
    draft_requests::safe_id(hold)?;
    let catalog = draft_requests::context(registry, token)?;
    let client = connect()?;
    registry.resolve(token)?;
    let view = checked_view(hold, client.task_recheck(&catalog.root, hold)?)?;
    let actor = draft_requests::identity()?;
    check_source(registry, token, &catalog)?;
    let ticket = uuid::Uuid::now_v7().to_string();
    let mut value = serde_json::to_value(&view).map_err(io_error)?;
    let source =
        towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog).map_err(io_error)?;
    requests.0.lock().map_err(io_error)?.views.insert(
        ticket.clone(),
        Viewed {
            token: token.into(),
            directory: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            instance: client.instance().into(),
            host: client.host().into(),
            actor: actor.clone(),
            revision: catalog.revision.clone(),
            source,
            view,
        },
    );
    value["recheck_ticket"] = json!(ticket);
    value["human_actor"] = json!(actor);
    value["project_id"] = json!(catalog.config.project_id());
    value["original_requests"] = json!(entries(registry, requests, token, hold, app_data, false)?);
    Ok(value)
}
pub fn prepare(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    ticket: &str,
    comment: String,
    app_data: &Path,
) -> Result<PreparedTaskRecheck, AppError> {
    prepare_with(registry, requests, token, ticket, comment, app_data, || {
        daemon_client::discover(app_data)
    })
}
#[allow(clippy::too_many_arguments)]
fn prepare_with(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    ticket: &str,
    comment: String,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<PreparedTaskRecheck, AppError> {
    let catalog = draft_requests::context(registry, token)?;
    let source =
        towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog).map_err(io_error)?;
    let (view, displayed_actor, host, instance) = {
        let state = requests.0.lock().map_err(io_error)?;
        let viewed = state
            .views
            .get(ticket)
            .ok_or_else(|| AppError::new("recheck_not_loaded", "请先加载完整 Task 复核原件"))?;
        if viewed.token != token
            || viewed.directory != catalog.root
            || viewed.common_dir != catalog.common_dir
            || viewed.revision != catalog.revision
            || viewed.source != source
        {
            return Err(AppError::new(
                "request_context_changed",
                "项目选择已变化，请重新加载 Task 复核",
            ));
        }
        (
            viewed.view.clone(),
            viewed.actor.clone(),
            viewed.host.clone(),
            viewed.instance.clone(),
        )
    };
    if view.binding.publication_ids.is_empty() {
        return Err(AppError::new(
            "request_conflict",
            "本任务已无未决 publication；请重新读取态势",
        ));
    }
    let task = view.binding.task_id.clone();
    let key = (
        catalog.config.project_id().to_owned(),
        catalog.common_dir.clone(),
        task.clone(),
    );
    {
        let mut state = requests.0.lock().map_err(io_error)?;
        if !state.preparing.insert(key.clone()) {
            return Err(AppError::new("request_busy", "本任务的复核请求正在保存"));
        }
    }
    let _preparing = Preparing { requests, key };
    if !entries(registry, requests, token, &task, app_data, true)?.is_empty() {
        return Err(AppError::new(
            "request_pending",
            "已有原复核请求等待确认；请重试原 UUID",
        ));
    }
    let actor = draft_requests::identity()?;
    if actor != displayed_actor {
        return Err(AppError::new(
            "request_context_changed",
            "本机人工身份已变化，请重新加载复核",
        ));
    }
    let client = connect()?;
    if client.host() != host || client.instance() != instance {
        return Err(AppError::new(
            "request_context_changed",
            "daemon 实例或 host 已变化，请重新加载复核",
        ));
    }
    registry.resolve(token)?;
    let current = checked_view(&task, client.task_recheck(&catalog.root, &task)?)?;
    if current != view {
        return Err(AppError::new(
            "request_conflict",
            "Task 文本、基线、publication 或 Run/lease 绑定已变化；请重新读取",
        ));
    }
    check_source(registry, token, &catalog)?;
    if !entries(registry, requests, token, &task, app_data, true)?.is_empty() {
        return Err(AppError::new("request_pending", "已有原复核请求等待确认"));
    }
    let id = uuid::Uuid::now_v7().to_string();
    let prepared = RecheckRequest::prepare(
        catalog.config.project_id(),
        client.host(),
        &actor,
        &TaskRecheckAction {
            request_id: id.clone(),
            binding: view.binding.clone(),
            comment,
        },
    )
    .map_err(io_error)?;
    let stored = QueuedRecheck {
        schema: 1,
        directory: catalog.root.clone(),
        common_dir: catalog.common_dir.clone(),
        request: prepared.body().clone(),
        metadata: prepared.metadata().clone(),
        subject: view,
    };
    check_source(registry, token, &catalog)?;
    write_new(&path(app_data, &id)?, &stored)?;
    requests
        .0
        .lock()
        .map_err(io_error)?
        .bound
        .insert(id, token.into());
    Ok(summary(&stored))
}
pub fn pending(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
) -> Result<Vec<PreparedTaskRecheck>, AppError> {
    entries(registry, requests, token, hold, app_data, true)
}
fn entries(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
    pending_only: bool,
) -> Result<Vec<PreparedTaskRecheck>, AppError> {
    draft_requests::safe_id(hold)?;
    let catalog = draft_requests::context(registry, token)?;
    let directory = draft_requests::queue(app_data)?.join("task-rechecks");
    if !directory.exists() {
        return Ok(vec![]);
    }
    if !fs::symlink_metadata(&directory)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("Task 复核队列不是普通目录"));
    }
    let mut found = vec![];
    for item in fs::read_dir(directory).map_err(io_error)? {
        let file = item.map_err(io_error)?.path();
        if file.extension().and_then(|v| v.to_str()) != Some("json")
            || (pending_only && file.with_extension("resolved").exists())
        {
            continue;
        }
        let id = file
            .file_stem()
            .and_then(|v| v.to_str())
            .ok_or_else(|| io_error("请求文件名无效"))?;
        let stored = read(app_data, id)?;
        if stored.request.project_id == catalog.config.project_id()
            && stored.common_dir == catalog.common_dir
            && action(&stored).0.task_id == hold
        {
            check(&catalog, &stored)?;
            requests
                .0
                .lock()
                .map_err(io_error)?
                .bound
                .insert(id.into(), token.into());
            found.push(summary(&stored));
        }
    }
    check_source(registry, token, &catalog)?;
    Ok(found)
}
struct Inflight<'a> {
    requests: &'a TaskRecheckRequests,
    id: String,
}
impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.requests.0.lock() {
            state.inflight.remove(&self.id);
        }
    }
}
fn confirmed(stored: &QueuedRecheck, value: &Value) -> bool {
    let Ok(outcome) = serde_json::from_value::<TaskRecheckOutcome>(value.clone()) else {
        return false;
    };
    let result = &outcome.receipt.result_ref;
    let Ok(eid) = request_event_eid(&stored.metadata.request_id, "task.recheck_resolved") else {
        return false;
    };
    outcome.receipt.request_id == stored.metadata.request_id
        && outcome.receipt.body_hash == stored.metadata.body_hash
        && result.request_id == stored.metadata.request_id
        && result.binding == *action(stored).0
        && result.actor == stored.request.actor
        && result.resolved_eid == eid
}
pub fn submit(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<draft_requests::EditOutcome, AppError> {
    submit_with(registry, requests, token, id, app_data, || {
        daemon_client::discover(app_data)
    })
}
fn submit_with(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    id: &str,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<draft_requests::EditOutcome, AppError> {
    let stored = read(app_data, id)?;
    let catalog = draft_requests::context(registry, token)?;
    check(&catalog, &stored)?;
    {
        let mut state = requests.0.lock().map_err(io_error)?;
        if state.bound.get(id).map(String::as_str) != Some(token) {
            return Err(AppError::new(
                "request_not_prepared",
                "请先读取本项目原 Task 复核请求",
            ));
        }
        if !state.inflight.insert(id.into()) {
            return Err(AppError::new("request_busy", "原 Task 复核请求正在发送"));
        }
    }
    let _inflight = Inflight {
        requests,
        id: id.into(),
    };
    let client = connect()?;
    registry.resolve(token)?;
    if client.host() != stored.request.host || draft_requests::identity()? != stored.request.actor {
        return Err(AppError::new(
            "request_context_changed",
            "原 Task 复核的 host 或人工身份已变化；保留原请求",
        ));
    }
    let human_id = stored
        .request
        .actor
        .strip_prefix("human:")
        .ok_or_else(|| io_error("原复核不是人工身份"))?;
    check_source_identity(registry, token, &catalog, false)?;
    let result = client.task_recheck_action(&TaskRecheckHttpRequest {
        directory: &stored.directory,
        human_id,
        action: original_action(&stored),
    });
    check_source_identity(registry, token, &catalog, false)?;
    let (state, response, error) = match result {
        Ok(value) if confirmed(&stored, &value) => ("confirmed", Some(value), None),
        Ok(_) => (
            "uncertain",
            None,
            Some(AppError::new(
                "request_uncertain",
                "Task 复核回执缺少原请求或精确绑定；请重试原请求",
            )),
        ),
        Err(error) => {
            let state = match error.code.as_str() {
                "request_conflict" => "conflict",
                "request_refused" => "rejected",
                _ => "uncertain",
            };
            (state, None, Some(error))
        }
    };
    if state != "uncertain" {
        let resolved = path(app_data, id)?.with_extension("resolved");
        if !resolved.exists() {
            write_new(&resolved, &json!({"state":state,"response":response}))?;
        }
    }
    Ok(draft_requests::EditOutcome {
        state,
        request_id: id.into(),
        response,
        error,
    })
}
#[cfg(test)]
#[path = "task_recheck_requests_tests.rs"]
mod tests;
