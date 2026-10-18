//! Native-only, fixed draft requests. The queue preserves original input; only
//! the daemon's durable receipt certifies saving. No Flow document is accepted.
use crate::{commands::AppError, daemon_client, project::ProjectRegistry};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use towermd_core::{
    baseline_draft::{DraftEdit, FrozenDraft},
    canvas::CanvasDocument,
    okf::Document,
    recovery_contract::recorded::{RequestBody, RequestMeta},
    request_ledger::{DraftSaveAction, PreparedDraftSaveRequest},
};

#[derive(Default, Clone)]
pub struct DraftRequests(Arc<Mutex<RequestState>>);
#[derive(Default)]
struct RequestState {
    bound: BTreeMap<String, String>,
    inflight: BTreeSet<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedEdit {
    schema: u32,
    directory: PathBuf,
    common_dir: PathBuf,
    request: RequestBody<DraftSaveAction>,
    metadata: RequestMeta,
}
#[derive(Serialize)]
pub(crate) struct EditHttpRequest<'a> {
    action: &'static str,
    directory: &'a Path,
    human_id: &'a str,
    request_id: &'a str,
    id: &'a str,
    expected_revision: &'a str,
    edit: &'a DraftEdit,
}
#[derive(Clone, Serialize)]
pub struct PreparedEdit {
    pub request_id: String,
    pub draft_id: String,
    pub expected_revision: String,
    pub human_actor: String,
}
#[derive(Debug, Serialize)]
pub struct EditOutcome {
    pub state: &'static str,
    pub request_id: String,
    pub response: Option<Value>,
    pub error: Option<AppError>,
}
pub(crate) fn io_error(error: impl ToString) -> AppError {
    AppError::new("request_queue_failed", error.to_string())
}
fn human(value: &str) -> Result<&str, AppError> {
    if value.is_empty()
        || value.contains([':', '/'])
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        Err(AppError::new(
            "human_identity_invalid",
            "本机 USER 缺失或不符合人工身份格式；不能编辑草稿",
        ))
    } else {
        Ok(value)
    }
}
pub fn identity() -> Result<String, AppError> {
    let user = std::env::var("USER")
        .map_err(|_| AppError::new("human_identity_invalid", "本机 USER 缺失；不能编辑草稿"))?;
    Ok(format!("human:{}", human(&user)?))
}
pub(crate) fn safe_id(id: &str) -> Result<(), AppError> {
    if id.is_empty()
        || id.len() > 160
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        || matches!(id, "." | "..")
    {
        return Err(AppError::new(
            "draft_id_invalid",
            "草稿 ID 只能包含字母、数字、短横线、下划线和点",
        ));
    }
    Ok(())
}
pub(crate) fn queue(app_data: &Path) -> Result<PathBuf, AppError> {
    let path = app_data.join("draft-requests");
    fs::create_dir_all(&path).map_err(io_error)?;
    fs::File::open(app_data)
        .and_then(|parent| parent.sync_all())
        .map_err(io_error)?;
    if !fs::symlink_metadata(&path)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("请求队列不是普通目录"));
    }
    Ok(path)
}
pub(crate) fn request_path(app_data: &Path, id: &str) -> Result<PathBuf, AppError> {
    let parsed = uuid::Uuid::parse_str(id).map_err(io_error)?;
    if parsed.get_version_num() != 7 || parsed.to_string() != id {
        return Err(io_error("请求 ID 不是原 UUIDv7"));
    }
    Ok(queue(app_data)?.join(format!("{id}.json")))
}
pub(crate) fn write_new(path: &Path, value: &impl Serialize) -> Result<(), AppError> {
    let bytes = serde_json::to_vec(value).map_err(io_error)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io_error)?;
    file.write_all(&bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    fs::File::open(path.parent().ok_or_else(|| io_error("队列无父目录"))?)
        .and_then(|directory| directory.sync_all())
        .map_err(io_error)
}
fn read_queue(app_data: &Path, id: &str) -> Result<QueuedEdit, AppError> {
    let path = request_path(app_data, id)?;
    let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
    if !metadata.file_type().is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err(io_error("请求队列文件类型或大小无效"));
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(io_error)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let stored: QueuedEdit = serde_json::from_slice(&bytes).map_err(io_error)?;
    let recomputed =
        PreparedDraftSaveRequest::prepare(stored.request.clone(), id).map_err(io_error)?;
    if stored.schema != 1 || &stored.metadata != recomputed.metadata() {
        return Err(io_error("原请求正文与共享 canonical hash 不一致"));
    }
    Ok(stored)
}
fn edit_fields(stored: &QueuedEdit) -> Result<(&str, &str, &DraftEdit), AppError> {
    match &stored.request.action {
        DraftSaveAction::EditDraft {
            id,
            expected_revision,
            edit,
        } => Ok((id, expected_revision, edit)),
        _ => Err(io_error("队列不是有限草稿编辑请求")),
    }
}
fn summary(stored: &QueuedEdit) -> Result<PreparedEdit, AppError> {
    let (id, revision, _) = edit_fields(stored)?;
    Ok(PreparedEdit {
        request_id: stored.metadata.request_id.clone(),
        draft_id: id.into(),
        expected_revision: revision.into(),
        human_actor: stored.request.actor.clone(),
    })
}
pub(crate) fn context(
    registry: &ProjectRegistry,
    token: &str,
) -> Result<towermd_core::catalog::Catalog, AppError> {
    let root = registry.initialized_root(token)?;
    towermd_core::catalog::Catalog::load(&root).map_err(|e| AppError::new("project_read_failed", e))
}
fn check_context(
    catalog: &towermd_core::catalog::Catalog,
    stored: &QueuedEdit,
) -> Result<(), AppError> {
    if catalog.config.project_id() != stored.request.project_id
        || catalog.common_dir != stored.common_dir
        || catalog.root != stored.directory
    {
        return Err(AppError::new(
            "request_context_changed",
            "原请求的项目或工作树已变化；不会改写原请求重发",
        ));
    }
    Ok(())
}
fn project_saved(
    catalog: &towermd_core::catalog::Catalog,
    id: &str,
    mut value: Value,
) -> Result<Value, AppError> {
    if value["state"] != "saved" {
        return Err(AppError::new("draft_not_saved", "此 ID 尚无已保存的 S6 草稿。请让已有会话中的 Agent 使用 baseline submit 提交；扫描候选不自动转为 S6。"));
    }
    if !matches!(
        value["current_validity"]["state"].as_str(),
        Some("current" | "historical_only")
    ) {
        return Err(AppError::new(
            "protocol_error",
            "已保存草稿缺少 current_validity",
        ));
    }
    let draft: FrozenDraft = serde_json::from_value(value["draft"].clone())
        .map_err(|e| AppError::new("protocol_error", e.to_string()))?;
    if value["id"] != id
        || value["project_id"] != catalog.config.project_id()
        || draft.bundle.id != id
        || draft.bundle.project_id != catalog.config.project_id()
        || value["revision"].as_str().is_none_or(str::is_empty)
        || value["request_id"].as_str().is_none_or(str::is_empty)
    {
        return Err(AppError::new(
            "protocol_error",
            "已保存草稿身份与所选项目不一致",
        ));
    }
    let canvas = CanvasDocument::decode(&draft.bundle.canvas_text)
        .map_err(|e| AppError::new("protocol_error", e))?;
    let entries = |nodes: bool| -> Vec<Value> {
        (if nodes {
            canvas.nodes()
        } else {
            canvas.edges()
        })
        .iter()
        .map(|entry| json!({"index":entry.index,"opaque":entry.opaque,"raw":entry.raw}))
        .collect()
    };
    value["canvas_view"] =
        json!({"nodes":entries(true),"edges":entries(false),"diagnostics":canvas.diagnostics()});
    value["object_views"] = json!(draft.bundle.objects.values().map(|object| {
        let doc = Document::parse(&object.text).ok();
        let fields:BTreeMap<_,_>=["paths","verify","module","from","to"].into_iter().filter_map(|key| {
            doc.as_ref().and_then(|doc|doc.get(key)).map(|value| (key,serde_json::to_value(value).unwrap_or(Value::Null)))
        }).collect();
        json!({"id":object.id,"path":object.path,"text":object.text,"type":doc.as_ref().and_then(|doc|doc.doc_type()),"title":doc.as_ref().and_then(|doc|doc.get_str("title")),"fields":fields})
    }).collect::<Vec<_>>());
    value["human_actor"] = json!(identity()?);
    Ok(value)
}
pub fn load(
    registry: &ProjectRegistry,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<Value, AppError> {
    safe_id(id)?;
    let catalog = context(registry, token)?;
    let client = daemon_client::discover(app_data)?;
    registry.resolve(token)?;
    let value = client.saved(&catalog.root, id)?;
    registry.resolve(token)?;
    project_saved(&catalog, id, value)
}
pub fn prepare(
    registry: &ProjectRegistry,
    requests: &DraftRequests,
    token: &str,
    id: &str,
    revision: &str,
    edit: DraftEdit,
    app_data: &Path,
) -> Result<PreparedEdit, AppError> {
    safe_id(id)?;
    let catalog = context(registry, token)?;
    let actor = identity()?;
    let client = daemon_client::discover(app_data)?;
    registry.resolve(token)?;
    let saved = project_saved(&catalog, id, client.saved(&catalog.root, id)?)?;
    if saved["current_validity"]["state"] != "current" {
        return Err(AppError::new(
            "request_conflict",
            "此草稿只有历史效力，不能继续编辑；请重新准备当前基线草稿",
        ));
    }
    if saved["revision"] != revision {
        return Err(AppError::new(
            "request_conflict",
            "草稿 revision 已变化；请刷新后重新审阅",
        ));
    }
    let request_id = uuid::Uuid::now_v7().to_string();
    let prepared = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: catalog.config.project_id().into(),
            host: client.host().into(),
            actor,
            run: None,
            action: DraftSaveAction::EditDraft {
                id: id.into(),
                expected_revision: revision.into(),
                edit,
            },
        },
        &request_id,
    )
    .map_err(io_error)?;
    let stored = QueuedEdit {
        schema: 1,
        directory: catalog.root,
        common_dir: catalog.common_dir,
        request: prepared.body().clone(),
        metadata: prepared.metadata().clone(),
    };
    registry.resolve(token)?;
    write_new(&request_path(app_data, &request_id)?, &stored)?;
    requests
        .0
        .lock()
        .map_err(io_error)?
        .bound
        .insert(request_id, token.into());
    summary(&stored)
}
pub fn pending(
    registry: &ProjectRegistry,
    requests: &DraftRequests,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<Vec<PreparedEdit>, AppError> {
    safe_id(id)?;
    let catalog = context(registry, token)?;
    let mut found = vec![];
    for entry in fs::read_dir(queue(app_data)?).map_err(io_error)? {
        let path = entry.map_err(io_error)?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Some(request_id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.with_extension("resolved").exists() {
            continue;
        }
        let stored = read_queue(app_data, request_id)?;
        if stored.request.project_id == catalog.config.project_id()
            && stored.common_dir == catalog.common_dir
            && edit_fields(&stored)?.0 == id
        {
            check_context(&catalog, &stored)?;
            requests
                .0
                .lock()
                .map_err(io_error)?
                .bound
                .insert(request_id.into(), token.into());
            found.push(summary(&stored)?);
        }
    }
    registry.resolve(token)?;
    Ok(found)
}
struct Inflight<'a> {
    requests: &'a DraftRequests,
    id: String,
}
impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        if let Ok(mut ids) = self.requests.0.lock() {
            ids.inflight.remove(&self.id);
        }
    }
}
pub fn submit(
    registry: &ProjectRegistry,
    requests: &DraftRequests,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<EditOutcome, AppError> {
    submit_with(registry, requests, token, id, app_data, || {
        daemon_client::discover(app_data)
    })
}
fn submit_with(
    registry: &ProjectRegistry,
    requests: &DraftRequests,
    token: &str,
    id: &str,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<EditOutcome, AppError> {
    let stored = read_queue(app_data, id)?;
    let catalog = context(registry, token)?;
    check_context(&catalog, &stored)?;
    {
        let mut state = requests.0.lock().map_err(io_error)?;
        if state.bound.get(id).map(String::as_str) != Some(token) {
            return Err(AppError::new(
                "request_not_prepared",
                "请先读取当前项目的原请求队列",
            ));
        }
        if !state.inflight.insert(id.into()) {
            return Err(AppError::new(
                "request_busy",
                "原请求正在发送，请勿重复点击",
            ));
        }
    }
    let _inflight = Inflight {
        requests,
        id: id.into(),
    };
    let client = connect()?;
    registry.resolve(token)?;
    let (draft_id, revision, edit) = edit_fields(&stored)?;
    let human_id = stored
        .request
        .actor
        .strip_prefix("human:")
        .ok_or_else(|| io_error("原请求不是人工编辑"))?;
    let original = match client.original_request(&stored.directory, id) {
        Ok(original) => original,
        Err(error) => return Ok(original_read_uncertain(id, error)),
    };
    registry.resolve(token)?;
    let result = if let Some(original) = original {
        if original["kind"] != "draft_save"
            || original["p3_request"] != serde_json::to_value(&stored.metadata).map_err(io_error)?
            || original["request_body"]
                != serde_json::to_value(&stored.request).map_err(io_error)?
        {
            return Err(AppError::new(
                "request_conflict",
                "服务端原请求与本机耐久正文不一致；不会替换正文重试",
            ));
        }
        client.retry_original(&stored.directory, id)
    } else {
        if client.host() != stored.request.host || identity()? != stored.request.actor {
            return Err(AppError::new(
                "request_context_changed",
                "尚未找到服务端原请求，host或人工身份已变化；不会改写原正文",
            ));
        }
        client.edit(&EditHttpRequest {
            action: "edit",
            directory: &stored.directory,
            human_id,
            request_id: id,
            id: draft_id,
            expected_revision: revision,
            edit,
        })
    };
    registry.resolve(token)?;
    let (state, response, error) = match result {
        Ok(value) => {
            let result = &value["receipt"]["result_ref"];
            let confirmed = value["receipt"]["request_id"] == id
                && value["receipt"]["body_hash"] == stored.metadata.body_hash
                && result["draft_id"] == draft_id
                && result["revision"].is_string()
                && result["content_hash"].is_string()
                && result["reference"] == format!("refs/towermd/drafts/{draft_id}")
                && matches!(
                    result["delivery"]["state"].as_str(),
                    Some("local_saved" | "remote_saved" | "local_saved_remote_pending")
                )
                && matches!(
                    value["current_validity"]["state"].as_str(),
                    Some("current" | "historical_only")
                );
            if confirmed {
                ("confirmed", Some(value), None)
            } else {
                (
                    "uncertain",
                    None,
                    Some(AppError::new(
                        "request_uncertain",
                        "保存回执缺少原请求身份或结果；请重试原请求",
                    )),
                )
            }
        }
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
        let path = request_path(app_data, id)?.with_extension("resolved");
        if !path.exists() {
            write_new(&path, &json!({"state":state,"response":response}))?;
        }
    }
    Ok(EditOutcome {
        state,
        request_id: id.into(),
        response,
        error,
    })
}

// A failed status read cannot establish that an earlier send was refused.
// Leave the exact durable request pending for an explicit retry.
pub(crate) fn original_read_uncertain(id: &str, cause: AppError) -> EditOutcome {
    let mut error = AppError::new(
        "request_uncertain",
        format!("原请求 {id} 的状态读取未获确认；请重试原请求"),
    );
    error.detail = Some(
        json!({"code":"request_uncertain","request_id":id,
        "error":cause.message,"cause":{"code":cause.code,"detail":cause.detail}})
        .to_string(),
    );
    EditOutcome {
        state: "uncertain",
        request_id: id.into(),
        response: None,
        error: Some(error),
    }
}

#[cfg(test)]
#[path = "draft_requests_tests.rs"]
pub(crate) mod tests;
