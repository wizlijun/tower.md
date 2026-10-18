//! Fixed native Ask Answer transport. Tickets bind a displayed original review;
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
    ask_answer_contract::{
        AppAnswerAction, AppAnswerRequest, AskAnswerOutcome, AskAnswerValidity, AskAnswerView,
        AskAnswerViewAction, AskOpeningBinding,
    },
    human_intervention::AskHoldView,
    recovery_contract::recorded::{request_event_eid, RequestBody, RequestMeta},
};

#[derive(Default, Clone)]
pub struct AskAnswerRequests(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    views: BTreeMap<String, Viewed>,
    bound: BTreeMap<String, String>,
    inflight: BTreeSet<String>,
    preparing: BTreeSet<(String, PathBuf, String)>,
}
// Membership serializes only preparation of this exact project's Hold. No
// native mutex is held while reading the actual daemon/source or fsyncing.
struct Preparing<'a> {
    requests: &'a AskAnswerRequests,
    key: (String, PathBuf, String),
}
impl Drop for Preparing<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.requests.0.lock() {
            state.preparing.remove(&self.key);
        }
    }
}
struct Viewed {
    token: String,
    directory: PathBuf,
    common_dir: PathBuf,
    instance: String,
    host: String,
    actor: String,
    view: AskAnswerView,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedAnswer {
    schema: u32,
    directory: PathBuf,
    common_dir: PathBuf,
    request: RequestBody<AppAnswerAction>,
    metadata: RequestMeta,
}
#[derive(Clone, Serialize)]
pub struct PreparedAskAnswer {
    pub request_id: String,
    pub hold: String,
    pub binding: AskOpeningBinding,
    pub text: String,
    pub human_actor: String,
}
#[derive(Serialize)]
pub(crate) struct AskAnswerHttpRequest<'a> {
    pub directory: &'a Path,
    pub request_id: &'a str,
    pub human_id: &'a str,
    pub action: &'a AppAnswerAction,
}
fn action(stored: &QueuedAnswer) -> (&str, &AskOpeningBinding, &str) {
    let AppAnswerAction::Answer {
        hold,
        opening,
        text,
    } = &stored.request.action;
    (hold, opening, text)
}
fn summary(stored: &QueuedAnswer) -> PreparedAskAnswer {
    let (hold, binding, text) = action(stored);
    PreparedAskAnswer {
        request_id: stored.metadata.request_id.clone(),
        hold: hold.into(),
        binding: binding.clone(),
        text: text.into(),
        human_actor: stored.request.actor.clone(),
    }
}
fn path(app_data: &Path, id: &str) -> Result<PathBuf, AppError> {
    let validated = draft_requests::request_path(app_data, id)?;
    let parent = validated.parent().ok_or_else(|| io_error("请求无父目录"))?;
    let queue = parent.join("ask-answers");
    fs::create_dir_all(&queue).map_err(io_error)?;
    if !fs::symlink_metadata(&queue)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("本次问答队列不是普通目录"));
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
fn read(app_data: &Path, id: &str) -> Result<QueuedAnswer, AppError> {
    let file = path(app_data, id)?;
    let meta = fs::symlink_metadata(&file).map_err(io_error)?;
    if !meta.file_type().is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err(io_error("原 本次问答请求文件类型或大小无效"));
    }
    let mut bytes = vec![];
    fs::File::open(file)
        .map_err(io_error)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let stored: QueuedAnswer = serde_json::from_slice(&bytes).map_err(io_error)?;
    let original = AppAnswerRequest::prepare(
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
        return Err(io_error("原 本次问答正文与共享 canonical hash 不一致"));
    }
    Ok(stored)
}
fn check(catalog: &towermd_core::catalog::Catalog, stored: &QueuedAnswer) -> Result<(), AppError> {
    if catalog.config.project_id() != stored.request.project_id
        || catalog.root != stored.directory
        || catalog.common_dir != stored.common_dir
    {
        return Err(AppError::new(
            "request_context_changed",
            "项目或工作树已变化；不会替换原 本次问答正文",
        ));
    }
    Ok(())
}
fn checked_view(
    catalog: &towermd_core::catalog::Catalog,
    id: &str,
    value: Value,
) -> Result<AskAnswerView, AppError> {
    let view: AskAnswerView = serde_json::from_value(value).map_err(io_error)?;
    view.binding.validate(&view.opening).map_err(io_error)?;
    let original: AskHoldView =
        serde_json::from_value(view.opening.data["hold"].clone()).map_err(io_error)?;
    let current: AskHoldView =
        serde_json::from_value(view.current_hold.clone()).map_err(io_error)?;
    let historical = matches!(
        view.current_validity,
        AskAnswerValidity::HistoricalOnly { .. }
    );
    if view.project_id != catalog.config.project_id()
        || view.opening.kind != "hold.opened"
        || view.opening.obj != id
        || view.opening.layer != "L5"
        || view.opening.actor != "process:towermd-cli"
        || view.opening.seq == 0
        || (view.opening.stale && !historical)
        || original.object_type != "Hold"
        || current.object_type != "Hold"
        || original.id != id
        || current.id != id
        || original.body.kind != "ask"
        || original.body != current.body
        || original.reply.is_some()
        || original.recipients.len() != 1
        || view.opening.guards != original.recipients[0].fences
        || view.opening.epoch != view.opening.guards.first().map(|f| f.epoch)
        || current.opened_ms != original.opened_ms
        || (!historical && current.opened_seq != view.opening.seq)
        || view
            .recipients
            .iter()
            .map(|r| &r.recipient)
            .collect::<Vec<_>>()
            != current.recipients.iter().collect::<Vec<_>>()
        || (historical
            && (!view.actions.is_empty() || view.recipients.iter().any(|r| r.actionable)))
        || (current.reply.is_some() && !view.actions.is_empty())
    {
        return Err(AppError::new(
            "protocol_error",
            "Ask original/project/recipient binding differs",
        ));
    }
    Ok(view)
}
fn check_source(
    registry: &ProjectRegistry,
    token: &str,
    expected: &towermd_core::catalog::Catalog,
) -> Result<(), AppError> {
    let current = draft_requests::context(registry, token)?;
    if current.root != expected.root
        || current.common_dir != expected.common_dir
        || current.config.project_id() != expected.config.project_id()
        || current.revision != expected.revision
        || towermd_core::recovery_outbox::RecoveryContext::from_catalog(&current)
            .map_err(io_error)?
            != towermd_core::recovery_outbox::RecoveryContext::from_catalog(expected)
                .map_err(io_error)?
    {
        return Err(AppError::new(
            "request_context_changed",
            "Ask source changed; retain original request",
        ));
    }
    Ok(())
}
pub fn load(
    registry: &ProjectRegistry,
    requests: &AskAnswerRequests,
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
    requests: &AskAnswerRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<Value, AppError> {
    draft_requests::safe_id(hold)?;
    let catalog = draft_requests::context(registry, token)?;
    let client = connect()?;
    registry.resolve(token)?;
    let view = checked_view(&catalog, hold, client.ask_answer(&catalog.root, hold)?)?;
    let actor = draft_requests::identity()?;
    check_source(registry, token, &catalog)?;
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
    value["answer_ticket"] = json!(ticket);
    value["human_actor"] = json!(actor);
    value["original_requests"] = json!(entries(registry, requests, token, hold, app_data, false)?);
    Ok(value)
}
pub fn prepare(
    registry: &ProjectRegistry,
    requests: &AskAnswerRequests,
    token: &str,
    ticket: &str,
    text: String,
    app_data: &Path,
) -> Result<PreparedAskAnswer, AppError> {
    prepare_with(registry, requests, token, ticket, text, app_data, || {
        daemon_client::discover(app_data)
    })
}
#[allow(clippy::too_many_arguments)]
fn prepare_with(
    registry: &ProjectRegistry,
    requests: &AskAnswerRequests,
    token: &str,
    ticket: &str,
    text: String,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<PreparedAskAnswer, AppError> {
    let catalog = draft_requests::context(registry, token)?;
    let (view, displayed_actor, host, instance) = {
        let state = requests.0.lock().map_err(io_error)?;
        let viewed = state
            .views
            .get(ticket)
            .ok_or_else(|| AppError::new("answer_not_loaded", "请先加载完整问答原件"))?;
        if viewed.token != token
            || viewed.directory != catalog.root
            || viewed.common_dir != catalog.common_dir
        {
            return Err(AppError::new(
                "request_context_changed",
                "项目选择已变化，请重新加载问答",
            ));
        }
        (
            viewed.view.clone(),
            viewed.actor.clone(),
            viewed.host.clone(),
            viewed.instance.clone(),
        )
    };
    if view.current_validity != AskAnswerValidity::Current {
        return Err(AppError::new(
            "request_conflict",
            "历史问答只读，请重新取得当前问答",
        ));
    }
    let current_hold: AskHoldView =
        serde_json::from_value(view.current_hold.clone()).map_err(io_error)?;
    if current_hold.reply.is_some() || !view.actions.contains(&AskAnswerViewAction::Answer) {
        return Err(AppError::new(
            "request_conflict",
            "Ask already answered or unavailable; read original or retry saved request",
        ));
    }
    let key = (
        catalog.config.project_id().to_owned(),
        catalog.common_dir.clone(),
        view.opening.obj.clone(),
    );
    {
        let mut state = requests.0.lock().map_err(io_error)?;
        if !state.preparing.insert(key.clone()) {
            return Err(AppError::new(
                "request_busy",
                "本次回答正在保存，请等待原请求",
            ));
        }
    }
    let _preparing = Preparing { requests, key };
    if !entries(registry, requests, token, &view.opening.obj, app_data, true)?.is_empty() {
        return Err(AppError::new(
            "request_pending",
            "已有原回答等待确认，请读取队列并重试原请求",
        ));
    }
    let actor = draft_requests::identity()?;
    if actor != displayed_actor {
        return Err(AppError::new(
            "request_context_changed",
            "本机人工身份已变化，请重新加载问答",
        ));
    }
    let client = connect()?;
    if client.host() != host || client.instance() != instance {
        return Err(AppError::new(
            "request_context_changed",
            "daemon 实例或 host 已变化，请重新加载问答",
        ));
    }
    registry.resolve(token)?;
    let current = checked_view(
        &catalog,
        &view.opening.obj,
        client.ask_answer(&catalog.root, &view.opening.obj)?,
    )?;
    if current != view {
        return Err(AppError::new(
            "request_conflict",
            "本次问答原件或有效性已变化，请重新加载；不会替换已读绑定",
        ));
    }
    check_source(registry, token, &catalog)?;
    if !entries(registry, requests, token, &view.opening.obj, app_data, true)?.is_empty() {
        return Err(AppError::new(
            "request_pending",
            "已有原回答等待确认，请读取队列并重试原请求",
        ));
    }
    let id = uuid::Uuid::now_v7().to_string();
    let prepared = AppAnswerRequest::prepare(
        catalog.config.project_id(),
        client.host(),
        &actor,
        &id,
        &AppAnswerAction::Answer {
            hold: view.opening.obj,
            opening: view.binding,
            text,
        },
    )
    .map_err(io_error)?;
    let stored = QueuedAnswer {
        schema: 1,
        directory: catalog.root.clone(),
        common_dir: catalog.common_dir.clone(),
        request: prepared.body().clone(),
        metadata: prepared.metadata().clone(),
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
    requests: &AskAnswerRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
) -> Result<Vec<PreparedAskAnswer>, AppError> {
    entries(registry, requests, token, hold, app_data, true)
}
fn entries(
    registry: &ProjectRegistry,
    requests: &AskAnswerRequests,
    token: &str,
    hold: &str,
    app_data: &Path,
    pending_only: bool,
) -> Result<Vec<PreparedAskAnswer>, AppError> {
    draft_requests::safe_id(hold)?;
    let catalog = draft_requests::context(registry, token)?;
    let directory = draft_requests::queue(app_data)?.join("ask-answers");
    if !directory.exists() {
        return Ok(vec![]);
    }
    if !fs::symlink_metadata(&directory)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("本次问答队列不是普通目录"));
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
    requests: &'a AskAnswerRequests,
    id: String,
}
impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.requests.0.lock() {
            state.inflight.remove(&self.id);
        }
    }
}
fn confirmed(stored: &QueuedAnswer, value: &Value) -> bool {
    let Ok(outcome) = serde_json::from_value::<AskAnswerOutcome>(value.clone()) else {
        return false;
    };
    let (hold, binding, text) = action(stored);
    let result = &outcome.receipt.result_ref;
    let Ok(eid) = request_event_eid(&stored.metadata.request_id, "hold.answered") else {
        return false;
    };
    let reply = &result.reply;
    outcome.receipt.request_id == stored.metadata.request_id
        && outcome.receipt.body_hash == stored.metadata.body_hash
        && result.hold == hold
        && result.opening_eid == binding.eid
        && reply.id == eid
        && reply.actor == stored.request.actor
        && reply.text == text
        && reply.relayed_from.is_none()
        && reply.self_reported.is_none()
        && reply.decision.is_none()
        && reply.decision_revision.is_none()
        && reply.extra.is_empty()
}
pub fn submit(
    registry: &ProjectRegistry,
    requests: &AskAnswerRequests,
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
    requests: &AskAnswerRequests,
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
                "请先读取本项目原 本次问答请求",
            ));
        }
        if !state.inflight.insert(id.into()) {
            return Err(AppError::new("request_busy", "原 本次问答请求正在发送"));
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
            "原 本次问答的 host 或人工身份已变化；保留原请求",
        ));
    }
    let human_id = stored
        .request
        .actor
        .strip_prefix("human:")
        .ok_or_else(|| io_error("原问答不是人工身份"))?;
    let result = client.ask_answer_action(&AskAnswerHttpRequest {
        directory: &stored.directory,
        request_id: id,
        human_id,
        action: &stored.request.action,
    });
    check_source(registry, token, &catalog)?;
    let (state, response, error) = match result {
        Ok(value) if confirmed(&stored, &value) => ("confirmed", Some(value), None),
        Ok(_) => (
            "uncertain",
            None,
            Some(AppError::new(
                "request_uncertain",
                "本次问答回执缺少原请求或精确绑定；请重试原请求",
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
#[path = "ask_answer_requests_tests.rs"]
mod tests;
