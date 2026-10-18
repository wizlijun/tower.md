//! Fixed native Task Review transport. Tickets bind a displayed original review;
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
    recovery_contract::{
        recorded::{
            request_event_eid, RequestBody, RequestMeta, ReviewBodyView, ReviewHoldView,
            TaskReviewAction,
        },
        task_review::{ReviewBinding, ReviewVerdict},
    },
    request_ledger::{ReviewValidity, TaskReviewOutcome, TaskReviewRequest, TaskReviewView},
};

#[derive(Default, Clone)]
pub struct TaskReviewRequests(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    views: BTreeMap<String, Viewed>,
    bound: BTreeMap<String, String>,
    inflight: BTreeSet<String>,
}
struct Viewed {
    token: String,
    directory: PathBuf,
    common_dir: PathBuf,
    instance: String,
    host: String,
    actor: String,
    view: TaskReviewView,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedReview {
    schema: u32,
    directory: PathBuf,
    common_dir: PathBuf,
    request: RequestBody<TaskReviewAction>,
    metadata: RequestMeta,
}
#[derive(Clone, Serialize)]
pub struct PreparedTaskReview {
    pub request_id: String,
    pub hold: String,
    pub binding: ReviewBinding,
    pub verdict: ReviewVerdict,
    pub comment: String,
    pub human_actor: String,
}
#[derive(Serialize)]
pub(crate) struct TaskReviewHttpRequest<'a> {
    pub directory: &'a Path,
    pub request_id: &'a str,
    pub human_id: &'a str,
    pub action: &'a TaskReviewAction,
}
fn action(stored: &QueuedReview) -> (&str, &ReviewBinding, &ReviewVerdict, &str) {
    let TaskReviewAction::Review {
        hold,
        binding,
        verdict,
        comment,
    } = &stored.request.action;
    (hold, binding, verdict, comment)
}
fn summary(stored: &QueuedReview) -> PreparedTaskReview {
    let (hold, binding, verdict, comment) = action(stored);
    PreparedTaskReview {
        request_id: stored.metadata.request_id.clone(),
        hold: hold.into(),
        binding: binding.clone(),
        verdict: verdict.clone(),
        comment: comment.into(),
        human_actor: stored.request.actor.clone(),
    }
}
fn path(app_data: &Path, id: &str) -> Result<PathBuf, AppError> {
    let validated = draft_requests::request_path(app_data, id)?;
    let parent = validated.parent().ok_or_else(|| io_error("请求无父目录"))?;
    let queue = parent.join("task-reviews");
    fs::create_dir_all(&queue).map_err(io_error)?;
    if !fs::symlink_metadata(&queue)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("Task 审阅队列不是普通目录"));
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
fn read(app_data: &Path, id: &str) -> Result<QueuedReview, AppError> {
    let file = path(app_data, id)?;
    let meta = fs::symlink_metadata(&file).map_err(io_error)?;
    if !meta.file_type().is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err(io_error("原 Task 审阅请求文件类型或大小无效"));
    }
    let mut bytes = vec![];
    fs::File::open(file)
        .map_err(io_error)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let stored: QueuedReview = serde_json::from_slice(&bytes).map_err(io_error)?;
    let original = TaskReviewRequest::prepare(
        &stored.request.project_id,
        &stored.request.host,
        &stored.request.actor,
        id,
        &stored.request.action,
    )
    .map_err(io_error)?;
    if stored.schema != 1
        || original.body() != &stored.request
        || original.metadata() != &stored.metadata
    {
        return Err(io_error("原 Task 审阅正文与共享 canonical hash 不一致"));
    }
    Ok(stored)
}
fn check(catalog: &towermd_core::catalog::Catalog, stored: &QueuedReview) -> Result<(), AppError> {
    if catalog.config.project_id() != stored.request.project_id
        || catalog.root != stored.directory
        || catalog.common_dir != stored.common_dir
    {
        return Err(AppError::new(
            "request_context_changed",
            "项目或工作树已变化；不会替换原 Task 审阅正文",
        ));
    }
    Ok(())
}
fn checked_view(
    catalog: &towermd_core::catalog::Catalog,
    id: &str,
    value: Value,
) -> Result<TaskReviewView, AppError> {
    let view: TaskReviewView = serde_json::from_value(value).map_err(io_error)?;
    let original: ReviewHoldView =
        serde_json::from_value(view.opening.data["hold"].clone()).map_err(io_error)?;
    let current: ReviewHoldView =
        serde_json::from_value(view.current_hold.clone()).map_err(io_error)?;
    let ReviewBodyView::Review {
        binding: original_binding,
        ..
    } = &original.body;
    let ReviewBodyView::Review {
        binding: current_binding,
        ..
    } = &current.body;
    if view.project_id != catalog.config.project_id()
        || view.opening.kind != "hold.opened"
        || view.opening.obj != id
        || original.id != id
        || current.id != id
        || original.object_type != "Hold"
        || current.object_type != "Hold"
        || original_binding != &view.binding
        || current_binding != &view.binding
        || view.opening.guards != view.binding.fences
        || (view.opening.stale && view.current_validity == ReviewValidity::Current)
        || view.opening.seq == 0
        || view.opening.layer != "L5"
        || view.opening.epoch != view.binding.fences.first().map(|fence| fence.epoch)
        || view.opening.actor != "process:towermd-cli"
    {
        return Err(AppError::new(
            "protocol_error",
            "Task 审阅原件缺少完整绑定或不属于当前项目",
        ));
    }
    Ok(view)
}
pub fn load(
    registry: &ProjectRegistry,
    requests: &TaskReviewRequests,
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
    requests: &TaskReviewRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<Value, AppError> {
    draft_requests::safe_id(hold)?;
    let catalog = draft_requests::context(registry, token)?;
    let client = connect()?;
    registry.resolve(token)?;
    let view = checked_view(&catalog, hold, client.task_review(&catalog.root, hold)?)?;
    let actor = draft_requests::identity()?;
    registry.resolve(token)?;
    let ticket = uuid::Uuid::now_v7().to_string();
    let mut value = serde_json::to_value(&view).map_err(io_error)?;
    requests.0.lock().map_err(io_error)?.views.insert(
        ticket.clone(),
        Viewed {
            token: token.into(),
            directory: catalog.root,
            common_dir: catalog.common_dir,
            instance: client.instance().into(),
            host: client.host().into(),
            actor: actor.clone(),
            view,
        },
    );
    value["review_ticket"] = json!(ticket);
    value["human_actor"] = json!(actor);
    value["original_requests"] = json!(entries(registry, requests, token, hold, app_data, false)?);
    Ok(value)
}
pub fn prepare(
    registry: &ProjectRegistry,
    requests: &TaskReviewRequests,
    token: &str,
    ticket: &str,
    verdict: ReviewVerdict,
    comment: String,
    app_data: &Path,
) -> Result<PreparedTaskReview, AppError> {
    prepare_with(
        registry,
        requests,
        token,
        ticket,
        verdict,
        comment,
        app_data,
        || daemon_client::discover(app_data),
    )
}
#[allow(clippy::too_many_arguments)]
fn prepare_with(
    registry: &ProjectRegistry,
    requests: &TaskReviewRequests,
    token: &str,
    ticket: &str,
    verdict: ReviewVerdict,
    comment: String,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<PreparedTaskReview, AppError> {
    let catalog = draft_requests::context(registry, token)?;
    let (view, displayed_actor, host, instance) = {
        let state = requests.0.lock().map_err(io_error)?;
        let viewed = state
            .views
            .get(ticket)
            .ok_or_else(|| AppError::new("review_not_loaded", "请先加载完整 Task 审阅原件"))?;
        if viewed.token != token
            || viewed.directory != catalog.root
            || viewed.common_dir != catalog.common_dir
        {
            return Err(AppError::new(
                "request_context_changed",
                "项目选择已变化，请重新加载 Task 审阅",
            ));
        }
        (
            viewed.view.clone(),
            viewed.actor.clone(),
            viewed.host.clone(),
            viewed.instance.clone(),
        )
    };
    if view.current_validity != ReviewValidity::Current {
        return Err(AppError::new(
            "request_conflict",
            "历史 Task 审阅只读，请重新取得当前审阅",
        ));
    }
    let current_hold: ReviewHoldView =
        serde_json::from_value(view.current_hold.clone()).map_err(io_error)?;
    if !matches!(
        current_hold.body,
        ReviewBodyView::Review { verdict: None, .. }
    ) || current_hold.reply.is_some()
    {
        return Err(AppError::new(
            "request_conflict",
            "Task 审阅已有答复，请读取原答复或重试原请求",
        ));
    }
    let actor = draft_requests::identity()?;
    if actor != displayed_actor {
        return Err(AppError::new(
            "request_context_changed",
            "本机人工身份已变化，请重新加载审阅",
        ));
    }
    let client = connect()?;
    if client.host() != host || client.instance() != instance {
        return Err(AppError::new(
            "request_context_changed",
            "daemon 实例或 host 已变化，请重新加载审阅",
        ));
    }
    registry.resolve(token)?;
    let current = checked_view(
        &catalog,
        &view.opening.obj,
        client.task_review(&catalog.root, &view.opening.obj)?,
    )?;
    if current != view {
        return Err(AppError::new(
            "request_conflict",
            "Task 审阅原件或有效性已变化，请重新加载；不会替换已读绑定",
        ));
    }
    let id = uuid::Uuid::now_v7().to_string();
    let prepared = TaskReviewRequest::prepare(
        catalog.config.project_id(),
        client.host(),
        &actor,
        &id,
        &TaskReviewAction::Review {
            hold: view.opening.obj,
            binding: view.binding,
            verdict,
            comment,
        },
    )
    .map_err(io_error)?;
    let stored = QueuedReview {
        schema: 1,
        directory: catalog.root,
        common_dir: catalog.common_dir,
        request: prepared.body().clone(),
        metadata: prepared.metadata().clone(),
    };
    registry.resolve(token)?;
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
    requests: &TaskReviewRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
) -> Result<Vec<PreparedTaskReview>, AppError> {
    entries(registry, requests, token, hold, app_data, true)
}
fn entries(
    registry: &ProjectRegistry,
    requests: &TaskReviewRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
    pending_only: bool,
) -> Result<Vec<PreparedTaskReview>, AppError> {
    draft_requests::safe_id(hold)?;
    let catalog = draft_requests::context(registry, token)?;
    let directory = draft_requests::queue(app_data)?.join("task-reviews");
    if !directory.exists() {
        return Ok(vec![]);
    }
    if !fs::symlink_metadata(&directory)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("Task 审阅队列不是普通目录"));
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
            && action(&stored).0 == hold
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
    registry.resolve(token)?;
    Ok(found)
}
struct Inflight<'a> {
    requests: &'a TaskReviewRequests,
    id: String,
}
impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.requests.0.lock() {
            state.inflight.remove(&self.id);
        }
    }
}
fn confirmed(stored: &QueuedReview, value: &Value) -> bool {
    let Ok(outcome) = serde_json::from_value::<TaskReviewOutcome>(value.clone()) else {
        return false;
    };
    let (hold, binding, verdict, comment) = action(stored);
    let result = &outcome.receipt.result_ref;
    let Ok(eid) = request_event_eid(&stored.metadata.request_id, "hold.reviewed") else {
        return false;
    };
    outcome.receipt.request_id == stored.metadata.request_id
        && outcome.receipt.body_hash == stored.metadata.body_hash
        && result.hold == hold
        && result.fact.binding == *binding
        && result.fact.verdict == *verdict
        && result.verdict == *verdict
        && result.comment == comment
        && result.fact.actor == stored.request.actor
        && result.fact.answer_eid == eid
}
pub fn submit(
    registry: &ProjectRegistry,
    requests: &TaskReviewRequests,
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
    requests: &TaskReviewRequests,
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
                "请先读取本项目原 Task 审阅请求",
            ));
        }
        if !state.inflight.insert(id.into()) {
            return Err(AppError::new("request_busy", "原 Task 审阅请求正在发送"));
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
            "原 Task 审阅的 host 或人工身份已变化；保留原请求",
        ));
    }
    let human_id = stored
        .request
        .actor
        .strip_prefix("human:")
        .ok_or_else(|| io_error("原审阅不是人工身份"))?;
    let result = client.task_review_action(&TaskReviewHttpRequest {
        directory: &stored.directory,
        request_id: id,
        human_id,
        action: &stored.request.action,
    });
    registry.resolve(token)?;
    let (state, response, error) = match result {
        Ok(value) if confirmed(&stored, &value) => ("confirmed", Some(value), None),
        Ok(_) => (
            "uncertain",
            None,
            Some(AppError::new(
                "request_uncertain",
                "Task 审阅回执缺少原请求或精确绑定；请重试原请求",
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
#[path = "task_review_requests_tests.rs"]
mod tests;
