//! Fixed App review transport. A WebView receives a native ticket for one exact
//! fetched binding; it cannot replace the binding, actor, host or request body.
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
    baseline_draft::FrozenDraft,
    baseline_review::{ApprovalFact, ReviewAction, ReviewBinding, ReviewValidity, Verdict},
    recovery_contract::recorded::{
        request_event_eid, BaselineReviewAction, RequestBody, RequestMeta, RequestReceipt,
    },
    request_ledger::BaselineRequest,
};

#[derive(Default, Clone)]
pub struct ReviewRequests(Arc<Mutex<State>>);
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
    binding: ReviewBinding,
    actor: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedReview {
    schema: u32,
    directory: PathBuf,
    common_dir: PathBuf,
    request: RequestBody<BaselineReviewAction>,
    metadata: RequestMeta,
}
#[derive(Clone, Serialize)]
pub struct PreparedReview {
    pub request_id: String,
    pub binding: ReviewBinding,
    pub verdict: Verdict,
    pub comment: String,
    pub human_actor: String,
}
#[derive(Serialize)]
pub(crate) struct ReviewHttpRequest<'a> {
    pub directory: &'a Path,
    pub human_id: &'a str,
    pub action: ReviewAction,
}
#[derive(Deserialize)]
struct ReviewResponse {
    receipt: RequestReceipt<ApprovalFact>,
    current_validity: ReviewValidity,
}
fn action(stored: &QueuedReview) -> (&ReviewBinding, &Verdict, &str) {
    let BaselineReviewAction::ReviewBaseline {
        binding,
        verdict,
        comment,
    } = &stored.request.action;
    (binding, verdict, comment)
}
fn summary(stored: &QueuedReview) -> PreparedReview {
    let (binding, verdict, comment) = action(stored);
    PreparedReview {
        request_id: stored.metadata.request_id.clone(),
        binding: binding.clone(),
        verdict: verdict.clone(),
        comment: comment.into(),
        human_actor: stored.request.actor.clone(),
    }
}
fn path(app_data: &Path, id: &str) -> Result<PathBuf, AppError> {
    let validated = draft_requests::request_path(app_data, id)?;
    let parent = validated.parent().ok_or_else(|| io_error("请求无父目录"))?;
    let queue = parent.join("reviews");
    fs::create_dir_all(&queue).map_err(io_error)?;
    if !fs::symlink_metadata(&queue)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("审阅队列不是普通目录"));
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
    let path = path(app_data, id)?;
    let meta = fs::symlink_metadata(&path).map_err(io_error)?;
    if !meta.file_type().is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err(io_error("原审阅请求文件类型或大小无效"));
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(io_error)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let stored: QueuedReview = serde_json::from_slice(&bytes).map_err(io_error)?;
    let original = BaselineRequest::prepare(
        &stored.request.project_id,
        &stored.request.host,
        &stored.request.actor,
        id,
        stored.request.action.clone(),
    )
    .map_err(io_error)?;
    if stored.schema != 1
        || original.body() != &stored.request
        || original.metadata() != &stored.metadata
    {
        return Err(io_error("原审阅正文与共享 canonical hash 不一致"));
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
            "项目或工作树已变化；不会替换原审阅正文",
        ));
    }
    Ok(())
}
fn checked_view(
    catalog: &towermd_core::catalog::Catalog,
    id: &str,
    value: &Value,
) -> Result<ReviewBinding, AppError> {
    let binding: ReviewBinding =
        serde_json::from_value(value["binding"].clone()).map_err(io_error)?;
    let draft: FrozenDraft = serde_json::from_value(value["draft"].clone()).map_err(io_error)?;
    if binding.project_id != catalog.config.project_id()
        || binding.object_id != id
        || draft.bundle.id != id
        || draft.bundle.project_id != binding.project_id
        || draft.content_hash != binding.content_hash
        || draft.bundle.target_baseline != binding.target_baseline
        || binding.draft_revision.is_empty()
        || !value["diff"].is_array()
        || !value["publish_blockers"].is_array()
        || !value["interface_modules"].is_array()
    {
        return Err(AppError::new(
            "protocol_error",
            "审阅内容缺少完整绑定或不属于当前项目",
        ));
    }
    for diff in value["diff"].as_array().unwrap() {
        serde_json::from_value::<towermd_core::baseline_review::PathDiff>(diff.clone())
            .map_err(io_error)?;
    }
    Ok(binding)
}
pub fn load(
    registry: &ProjectRegistry,
    requests: &ReviewRequests,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<Value, AppError> {
    draft_requests::safe_id(id)?;
    let catalog = draft_requests::context(registry, token)?;
    let client = daemon_client::discover(app_data)?;
    registry.resolve(token)?;
    let mut value = client.review(&catalog.root, id)?;
    let binding = checked_view(&catalog, id, &value)?;
    let actor = draft_requests::identity()?;
    registry.resolve(token)?;
    let ticket = uuid::Uuid::now_v7().to_string();
    requests.0.lock().map_err(io_error)?.views.insert(
        ticket.clone(),
        Viewed {
            token: token.into(),
            directory: catalog.root,
            common_dir: catalog.common_dir,
            binding,
            actor: actor.clone(),
        },
    );
    value["review_ticket"] = json!(ticket);
    value["human_actor"] = json!(actor);
    value["original_requests"] = json!(entries(registry, requests, token, id, app_data, false)?);
    Ok(value)
}
pub fn prepare(
    registry: &ProjectRegistry,
    requests: &ReviewRequests,
    token: &str,
    ticket: &str,
    verdict: Verdict,
    comment: String,
    app_data: &Path,
) -> Result<PreparedReview, AppError> {
    let catalog = draft_requests::context(registry, token)?;
    let (binding, displayed_actor) = {
        let state = requests.0.lock().map_err(io_error)?;
        let viewed = state
            .views
            .get(ticket)
            .ok_or_else(|| AppError::new("review_not_loaded", "请先加载完整审阅内容"))?;
        if viewed.token != token
            || viewed.directory != catalog.root
            || viewed.common_dir != catalog.common_dir
        {
            return Err(AppError::new(
                "request_context_changed",
                "项目选择已变化，请重新加载审阅",
            ));
        }
        (viewed.binding.clone(), viewed.actor.clone())
    };
    let actor = draft_requests::identity()?;
    if actor != displayed_actor {
        return Err(AppError::new(
            "request_context_changed",
            "本机人工身份已变化，请重新加载审阅后再决定",
        ));
    }
    let client = daemon_client::discover(app_data)?;
    registry.resolve(token)?;
    let current = client.review(&catalog.root, &binding.object_id)?;
    if checked_view(&catalog, &binding.object_id, &current)? != binding {
        return Err(AppError::new(
            "request_conflict",
            "草稿 revision 或内容已变化，请重新加载并审阅；不会替换已读 binding",
        ));
    }
    if verdict == Verdict::Approved
        && current["publish_blockers"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    {
        return Err(AppError::new(
            "request_conflict",
            "存在发布阻碍，请重新审阅服务端原因",
        ));
    }
    let id = uuid::Uuid::now_v7().to_string();
    let prepared = BaselineRequest::prepare(
        catalog.config.project_id(),
        client.host(),
        &actor,
        &id,
        BaselineReviewAction::ReviewBaseline {
            binding,
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
    requests: &ReviewRequests,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<Vec<PreparedReview>, AppError> {
    entries(registry, requests, token, id, app_data, true)
}
fn entries(
    registry: &ProjectRegistry,
    requests: &ReviewRequests,
    token: &str,
    id: &str,
    app_data: &Path,
    pending_only: bool,
) -> Result<Vec<PreparedReview>, AppError> {
    draft_requests::safe_id(id)?;
    let catalog = draft_requests::context(registry, token)?;
    let dir = draft_requests::queue(app_data)?.join("reviews");
    if !dir.exists() {
        return Ok(vec![]);
    }
    if !fs::symlink_metadata(&dir)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("审阅队列不是普通目录"));
    }
    let mut found = vec![];
    for item in fs::read_dir(dir).map_err(io_error)? {
        let file = item.map_err(io_error)?.path();
        if file.extension().and_then(|v| v.to_str()) != Some("json")
            || (pending_only && file.with_extension("resolved").exists())
        {
            continue;
        }
        let request_id = file
            .file_stem()
            .and_then(|v| v.to_str())
            .ok_or_else(|| io_error("请求文件名无效"))?;
        let stored = read(app_data, request_id)?;
        if stored.request.project_id == catalog.config.project_id()
            && stored.common_dir == catalog.common_dir
            && action(&stored).0.object_id == id
        {
            check(&catalog, &stored)?;
            requests
                .0
                .lock()
                .map_err(io_error)?
                .bound
                .insert(request_id.into(), token.into());
            found.push(summary(&stored));
        }
    }
    registry.resolve(token)?;
    Ok(found)
}
struct Inflight<'a> {
    requests: &'a ReviewRequests,
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
    let Ok(response) = serde_json::from_value::<ReviewResponse>(value.clone()) else {
        return false;
    };
    let (binding, verdict, comment) = action(stored);
    let fact = &response.receipt.result_ref;
    let _validity = &response.current_validity;
    let event_kind = match verdict {
        Verdict::Approved => "baseline.approved",
        Verdict::Rejected => "baseline.rejected",
    };
    let Ok(expected_eid) = request_event_eid(&stored.metadata.request_id, event_kind) else {
        return false;
    };
    response.receipt.request_id == stored.metadata.request_id
        && response.receipt.body_hash == stored.metadata.body_hash
        && fact.binding == *binding
        && fact.verdict == *verdict
        && fact.actor == stored.request.actor
        && fact.comment == comment
        && fact.approval_eid == expected_eid
}
pub fn submit(
    registry: &ProjectRegistry,
    requests: &ReviewRequests,
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
    requests: &ReviewRequests,
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
                "请先读取本项目原审阅请求",
            ));
        }
        if !state.inflight.insert(id.into()) {
            return Err(AppError::new("request_busy", "原审阅请求正在发送"));
        }
    }
    let _inflight = Inflight {
        requests,
        id: id.into(),
    };
    let client = connect()?;
    registry.resolve(token)?;
    let original = match client.original_request(&stored.directory, id) {
        Ok(original) => original,
        Err(error) => return Ok(draft_requests::original_read_uncertain(id, error)),
    };
    registry.resolve(token)?;
    let (binding, verdict, comment) = action(&stored);
    let result = if let Some(original) = original {
        if original["kind"] != "review"
            || original["p3_request"] != serde_json::to_value(&stored.metadata).map_err(io_error)?
            || original["request_body"]
                != serde_json::to_value(&stored.request).map_err(io_error)?
        {
            return Err(AppError::new(
                "request_conflict",
                "服务端原审阅与本机正文不一致；不会替换 binding 重试",
            ));
        }
        client.retry_original(&stored.directory, id)
    } else {
        if client.host() != stored.request.host
            || draft_requests::identity()? != stored.request.actor
        {
            return Err(AppError::new(
                "request_context_changed",
                "原审阅尚未接受且 host 或人工身份已变化",
            ));
        }
        let human_id = stored
            .request
            .actor
            .strip_prefix("human:")
            .ok_or_else(|| io_error("原审阅不是人工身份"))?;
        client.review_action(
            &binding.object_id,
            &ReviewHttpRequest {
                directory: &stored.directory,
                human_id,
                action: ReviewAction {
                    request_id: id.into(),
                    binding: binding.clone(),
                    verdict: verdict.clone(),
                    comment: comment.into(),
                },
            },
        )
    };
    registry.resolve(token)?;
    let (state, response, error) = match result {
        Ok(value) if confirmed(&stored, &value) => ("confirmed", Some(value), None),
        Ok(_) => (
            "uncertain",
            None,
            Some(AppError::new(
                "request_uncertain",
                "审阅回执缺少原请求或精确 binding；请重试原请求",
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
#[path = "review_requests_tests.rs"]
mod tests;
