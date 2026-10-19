//! Fixed App controls, using the existing native tickets and durable UUID queue.
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
    catalog::Catalog,
    intervention_control::{
        AppControlAction, ControlAction, ControlOutcome, ControlRequest, ControlView,
    },
    recovery_contract::recorded::{request_event_eid, RequestBody, RequestMeta},
};

#[derive(Default, Clone)]
pub struct InterventionRequests(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    views: BTreeMap<String, Viewed>,
    bound: BTreeMap<String, String>,
    inflight: BTreeSet<String>,
    preparing: BTreeSet<(String, PathBuf)>,
}
struct Viewed {
    token: String,
    directory: PathBuf,
    common_dir: PathBuf,
    instance: String,
    host: String,
    actor: String,
    source: CatalogSource,
    view: ControlView,
}
#[derive(Clone, Debug, PartialEq)]
struct CatalogSource {
    revision: String,
    recovery: towermd_core::recovery_outbox::RecoveryContext,
}
impl CatalogSource {
    fn from(catalog: &Catalog) -> Result<Self, AppError> {
        Ok(Self {
            revision: catalog.revision.clone(),
            recovery: towermd_core::recovery_outbox::RecoveryContext::from_catalog(catalog)
                .map_err(io_error)?,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedControl {
    schema: u32,
    directory: PathBuf,
    common_dir: PathBuf,
    request: RequestBody<ControlAction>,
    metadata: RequestMeta,
}
#[derive(Serialize)]
pub struct PreparedIntervention {
    request_id: String,
    project_id: String,
    human_actor: String,
    body_hash: String,
    action: AppControlAction,
}
#[derive(Serialize)]
pub(crate) struct ControlHttpRequest<'a> {
    pub directory: &'a Path,
    pub request_id: &'a str,
    pub human_id: &'a str,
    pub action: &'a AppControlAction,
}
struct Busy<'a> {
    requests: &'a InterventionRequests,
    request: Option<String>,
    preparation: Option<(String, PathBuf)>,
}
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.requests.0.lock() {
            if let Some(id) = &self.request {
                state.inflight.remove(id);
            }
            if let Some(key) = &self.preparation {
                state.preparing.remove(key);
            }
        }
    }
}
fn preparing<'a>(
    requests: &'a InterventionRequests,
    catalog: &Catalog,
) -> Result<Busy<'a>, AppError> {
    let key = (
        catalog.config.project_id().to_owned(),
        catalog.common_dir.clone(),
    );
    if !requests
        .0
        .lock()
        .map_err(io_error)?
        .preparing
        .insert(key.clone())
    {
        return Err(AppError::new(
            "request_busy",
            "该项目正在保存一个原控制请求",
        ));
    }
    Ok(Busy {
        requests,
        request: None,
        preparation: Some(key),
    })
}
fn sending<'a>(
    requests: &'a InterventionRequests,
    token: &str,
    id: &str,
) -> Result<Busy<'a>, AppError> {
    let mut state = requests.0.lock().map_err(io_error)?;
    if state.bound.get(id).map(String::as_str) != Some(token) {
        return Err(AppError::new(
            "request_not_prepared",
            "请先读取本项目原控制队列",
        ));
    }
    if !state.inflight.insert(id.into()) {
        return Err(AppError::new("request_busy", "此原控制请求正在发送"));
    }
    Ok(Busy {
        requests,
        request: Some(id.into()),
        preparation: None,
    })
}
fn path(app_data: &Path, id: &str) -> Result<PathBuf, AppError> {
    let validated = draft_requests::request_path(app_data, id)?;
    let parent = validated
        .parent()
        .ok_or_else(|| io_error("原请求无父目录"))?;
    let directory = parent.join("interventions");
    fs::create_dir_all(&directory).map_err(io_error)?;
    if !fs::symlink_metadata(&directory)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("控制请求队列不是普通目录"));
    }
    fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    Ok(directory.join(
        validated
            .file_name()
            .ok_or_else(|| io_error("原请求文件名缺失"))?,
    ))
}
fn read(app_data: &Path, id: &str) -> Result<QueuedControl, AppError> {
    let file = path(app_data, id)?;
    let metadata = fs::symlink_metadata(&file).map_err(io_error)?;
    if !metadata.file_type().is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err(io_error("原控制请求文件类型或大小无效"));
    }
    let mut bytes = vec![];
    fs::File::open(file)
        .map_err(io_error)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let stored: QueuedControl = serde_json::from_slice(&bytes).map_err(io_error)?;
    let original = ControlRequest::from_recorded(stored.request.clone(), stored.metadata.clone())
        .map_err(io_error)?;
    let role = if matches!(&stored.request.action, ControlAction::ResumeAll) {
        stored.request.actor == "process:towermd-cli"
    } else {
        stored.request.actor.starts_with("human:")
    };
    if stored.schema != 1
        || !role
        || original.body() != &stored.request
        || original.metadata() != &stored.metadata
        || stored.metadata.request_id != id
    {
        return Err(io_error("原控制正文与canonical身份不同"));
    }
    Ok(stored)
}
fn check(catalog: &Catalog, stored: &QueuedControl) -> Result<(), AppError> {
    if stored.request.project_id != catalog.config.project_id()
        || stored.directory != catalog.root
        || stored.common_dir != catalog.common_dir
    {
        return Err(AppError::new(
            "request_context_changed",
            "项目或工作树已变化；保留原控制正文",
        ));
    }
    Ok(())
}
fn check_source(
    registry: &ProjectRegistry,
    token: &str,
    expected: &Catalog,
) -> Result<(), AppError> {
    let current = draft_requests::context(registry, token)?;
    if current.root != expected.root
        || current.common_dir != expected.common_dir
        || current.config.project_id() != expected.config.project_id()
        || CatalogSource::from(&current)? != CatalogSource::from(expected)?
    {
        return Err(AppError::new(
            "request_context_changed",
            "控制操作期间项目来源改变；不会替换原件",
        ));
    }
    Ok(())
}
fn app_action(action: &ControlAction) -> Result<AppControlAction, AppError> {
    match action {
        ControlAction::Interject { scope, text } => Ok(AppControlAction::Interject {
            scope: scope.clone(),
            text: text.clone(),
        }),
        ControlAction::PauseAll => Ok(AppControlAction::PauseAll {}),
        ControlAction::ConfirmResume {
            request,
            pause_revision,
            runs,
        } => Ok(AppControlAction::ConfirmResume {
            request: request.clone(),
            pause_revision: pause_revision.clone(),
            runs: runs.clone(),
        }),
        ControlAction::ResumeAll => Err(io_error("process恢复请求不是人工确认")),
    }
}
fn summary(stored: &QueuedControl) -> Result<PreparedIntervention, AppError> {
    Ok(PreparedIntervention {
        request_id: stored.metadata.request_id.clone(),
        project_id: stored.request.project_id.clone(),
        human_actor: stored.request.actor.clone(),
        body_hash: stored.metadata.body_hash.clone(),
        action: app_action(&stored.request.action)?,
    })
}
fn checked_view(catalog: &Catalog, value: Value) -> Result<ControlView, AppError> {
    let view: ControlView = serde_json::from_value(value).map_err(io_error)?;
    if view.project_id != catalog.config.project_id()
        || view
            .active_runs
            .iter()
            .map(|run| &run.run)
            .collect::<BTreeSet<_>>()
            .len()
            != view.active_runs.len()
        || view.paused != view.pause_revision.is_some()
    {
        return Err(io_error("控制视图项目或暂停身份不一致"));
    }
    if let Some(resume) = &view.resume_request {
        if !view.paused || Some(&resume.pause_revision) != view.pause_revision.as_ref() {
            return Err(io_error("恢复请求不是当前暂停版本"));
        }
    }
    Ok(view)
}
fn same_subject(first: &ControlView, second: &ControlView) -> bool {
    first.project_id == second.project_id
        && first.paused == second.paused
        && first.pause_revision == second.pause_revision
        && first.active_runs == second.active_runs
        && first.resume_request == second.resume_request
}
pub fn load(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
    token: &str,
    app_data: &Path,
) -> Result<Value, AppError> {
    load_with(registry, requests, token, app_data, || {
        daemon_client::discover(app_data)
    })
}
fn load_with(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
    token: &str,
    _app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<Value, AppError> {
    let catalog = draft_requests::context(registry, token)?;
    let client = connect()?;
    registry.resolve(token)?;
    let view = checked_view(&catalog, client.interventions(&catalog.root)?)?;
    let actor = draft_requests::identity()?;
    check_source(registry, token, &catalog)?;
    let ticket = uuid::Uuid::now_v7().to_string();
    let mut value = serde_json::to_value(&view).map_err(io_error)?;
    requests.0.lock().map_err(io_error)?.views.insert(
        ticket.clone(),
        Viewed {
            token: token.into(),
            directory: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            instance: client.instance().into(),
            host: client.host().into(),
            actor: actor.clone(),
            source: CatalogSource::from(&catalog)?,
            view,
        },
    );
    value["action_ticket"] = json!(ticket);
    value["human_actor"] = json!(actor);
    Ok(value)
}
fn originals(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
    token: &str,
    app_data: &Path,
    process: bool,
) -> Result<Vec<QueuedControl>, AppError> {
    let catalog = draft_requests::context(registry, token)?;
    let directory = draft_requests::queue(app_data)?.join("interventions");
    if !directory.exists() {
        return Ok(vec![]);
    }
    if !fs::symlink_metadata(&directory)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("控制请求队列不是普通目录"));
    }
    let mut found = vec![];
    for item in fs::read_dir(directory).map_err(io_error)? {
        let file = item.map_err(io_error)?.path();
        if file.extension().and_then(|value| value.to_str()) != Some("json")
            || file.with_extension("resolved").exists()
        {
            continue;
        }
        let id = file
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| io_error("原请求文件名无效"))?;
        let stored = read(app_data, id)?;
        if stored.request.project_id == catalog.config.project_id()
            && stored.common_dir == catalog.common_dir
            && matches!(&stored.request.action, ControlAction::ResumeAll) == process
        {
            check(&catalog, &stored)?;
            requests
                .0
                .lock()
                .map_err(io_error)?
                .bound
                .insert(id.into(), token.into());
            found.push(stored);
        }
    }
    registry.resolve(token)?;
    Ok(found)
}
pub fn pending(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
    token: &str,
    app_data: &Path,
) -> Result<Vec<PreparedIntervention>, AppError> {
    originals(registry, requests, token, app_data, false)?
        .iter()
        .map(summary)
        .collect()
}
pub fn prepare(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
    token: &str,
    ticket: &str,
    action: AppControlAction,
    app_data: &Path,
) -> Result<PreparedIntervention, AppError> {
    prepare_with(registry, requests, token, ticket, action, app_data, || {
        daemon_client::discover(app_data)
    })
}
#[allow(clippy::too_many_arguments)]
fn prepare_with(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
    token: &str,
    ticket: &str,
    action: AppControlAction,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<PreparedIntervention, AppError> {
    let catalog = draft_requests::context(registry, token)?;
    let _preparing = preparing(requests, &catalog)?;
    let (subject, actor, host, instance, source) = {
        let state = requests.0.lock().map_err(io_error)?;
        let viewed = state
            .views
            .get(ticket)
            .ok_or_else(|| AppError::new("control_not_loaded", "请先读取实际控制状态"))?;
        if viewed.token != token
            || viewed.directory != catalog.root
            || viewed.common_dir != catalog.common_dir
        {
            return Err(AppError::new(
                "request_context_changed",
                "项目选择已改变，请重新读取",
            ));
        }
        (
            viewed.view.clone(),
            viewed.actor.clone(),
            viewed.host.clone(),
            viewed.instance.clone(),
            viewed.source.clone(),
        )
    };
    if CatalogSource::from(&catalog)? != source || draft_requests::identity()? != actor {
        return Err(AppError::new(
            "request_context_changed",
            "来源或本机身份已变化，请重新读取",
        ));
    }
    if !originals(registry, requests, token, app_data, false)?.is_empty() {
        return Err(AppError::new(
            "request_pending",
            "已有原控制请求，须先协调同一UUID",
        ));
    }
    let client = connect()?;
    if client.host() != host || client.instance() != instance {
        return Err(AppError::new(
            "request_context_changed",
            "daemon实例或host已改变，请重新读取",
        ));
    }
    let current = checked_view(&catalog, client.interventions(&catalog.root)?)?;
    if !same_subject(&subject, &current) {
        return Err(AppError::new(
            "request_conflict",
            "暂停版本或活动范围已变化，请重新核对",
        ));
    }
    if let AppControlAction::ConfirmResume {
        request,
        pause_revision,
        runs,
    } = &action
    {
        let expected = towermd_core::intervention_control::ResumeRequestView {
            request: request.clone(),
            pause_revision: pause_revision.clone(),
            runs: runs.clone(),
        };
        if current.resume_request.as_ref() != Some(&expected)
            || !current.paused
            || current
                .active_runs
                .iter()
                .map(|run| run.run.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                != *runs
        {
            return Err(AppError::new(
                "request_conflict",
                "恢复确认不同于实际请求的完整范围",
            ));
        }
    }
    let id = uuid::Uuid::now_v7().to_string();
    let original = ControlRequest::prepare(
        catalog.config.project_id(),
        client.host(),
        &actor,
        &id,
        action.into(),
    )
    .map_err(io_error)?;
    let stored = QueuedControl {
        schema: 1,
        directory: catalog.root.clone(),
        common_dir: catalog.common_dir.clone(),
        request: original.body().clone(),
        metadata: original.metadata().clone(),
    };
    check_source(registry, token, &catalog)?;
    write_new(&path(app_data, &id)?, &stored)?;
    requests
        .0
        .lock()
        .map_err(io_error)?
        .bound
        .insert(id, token.into());
    summary(&stored)
}
fn confirmed(stored: &QueuedControl, value: &Value) -> bool {
    let Ok(outcome) = serde_json::from_value::<ControlOutcome>(value.clone()) else {
        return false;
    };
    let Ok(original) =
        ControlRequest::from_recorded(stored.request.clone(), stored.metadata.clone())
    else {
        return false;
    };
    let result = &outcome.receipt.result_ref;
    let Ok(eid) = request_event_eid(&stored.metadata.request_id, "hold.opened") else {
        return false;
    };
    if outcome.receipt.request_id != stored.metadata.request_id
        || outcome.receipt.body_hash != stored.metadata.body_hash
        || result.hold != format!("H-{}", stored.metadata.request_id)
        || result.opening_eid != eid
        || result.runs.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return false;
    }
    match &stored.request.action {
        ControlAction::Interject { .. } => result.pause_revision.is_none(),
        ControlAction::PauseAll => original
            .pause_revision()
            .is_ok_and(|revision| result.pause_revision.as_ref() == Some(&revision)),
        ControlAction::ResumeAll => result.pause_revision.as_ref().is_some_and(|revision| {
            revision.len() == 64
                && revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }),
        ControlAction::ConfirmResume {
            pause_revision,
            runs,
            ..
        } => result.pause_revision.as_ref() == Some(pause_revision) && &result.runs == runs,
    }
}
fn finish(
    app_data: &Path,
    id: &str,
    result: Result<Value, AppError>,
    stored: &QueuedControl,
) -> Result<draft_requests::EditOutcome, AppError> {
    let (state, response, error) = match result {
        Ok(value) if confirmed(stored, &value) => ("confirmed", Some(value), None),
        Ok(_) => (
            "uncertain",
            None,
            Some(AppError::new(
                "request_uncertain",
                "控制回执与原UUID、正文或范围不同；仅可重试原请求",
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
pub fn submit(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
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
    requests: &InterventionRequests,
    token: &str,
    id: &str,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
) -> Result<draft_requests::EditOutcome, AppError> {
    let stored = read(app_data, id)?;
    let catalog = draft_requests::context(registry, token)?;
    check(&catalog, &stored)?;
    let _sending = sending(requests, token, id)?;
    let action = app_action(&stored.request.action)?;
    let client = connect()?;
    if client.host() != stored.request.host || draft_requests::identity()? != stored.request.actor {
        return Err(AppError::new(
            "request_context_changed",
            "原host或本机人工身份改变；保留原件",
        ));
    }
    registry.resolve(token)?;
    let human = stored
        .request
        .actor
        .strip_prefix("human:")
        .ok_or_else(|| io_error("控制操作不是人工请求"))?;
    let result = client.intervention_action(&ControlHttpRequest {
        directory: &stored.directory,
        request_id: id,
        human_id: human,
        action: &action,
    });
    check_source(registry, token, &catalog)?;
    finish(app_data, id, result, &stored)
}
pub fn request_resume(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
    token: &str,
    app_data: &Path,
) -> Result<(), AppError> {
    request_resume_with(
        registry,
        requests,
        token,
        app_data,
        || daemon_client::discover(app_data),
        |root, id| {
            let exe = std::env::current_exe().map_err(io_error)?;
            let cli = crate::cli::resolve_cli(&exe, cfg!(debug_assertions))?;
            crate::cli::run_cli(
                &cli,
                root,
                crate::cli::CliAction::RequestResume {
                    request_id: id.into(),
                },
            )
        },
    )
}
fn request_resume_with(
    registry: &ProjectRegistry,
    requests: &InterventionRequests,
    token: &str,
    app_data: &Path,
    connect: impl FnOnce() -> Result<daemon_client::DaemonClient, AppError>,
    execute: impl FnOnce(&Path, &str) -> Result<crate::cli::CliOutput, AppError>,
) -> Result<(), AppError> {
    let catalog = draft_requests::context(registry, token)?;
    let _preparing = preparing(requests, &catalog)?;
    let client = connect()?;
    registry.resolve(token)?;
    let mut pending = originals(registry, requests, token, app_data, true)?;
    if pending.len() > 1 {
        return Err(AppError::new(
            "request_pending",
            "有多个原process恢复请求，须逐项核对，不生成新UUID",
        ));
    }
    let stored = if let Some(original) = pending.pop() {
        check(&catalog, &original)?;
        if original.request.host != client.host() {
            return Err(AppError::new(
                "request_context_changed",
                "原恢复请求host改变，保留原UUID",
            ));
        }
        original
    } else {
        let view = checked_view(&catalog, client.interventions(&catalog.root)?)?;
        if !view.paused {
            return Err(AppError::new(
                "request_conflict",
                "当前未暂停，无需创建恢复请求",
            ));
        }
        let id = uuid::Uuid::now_v7().to_string();
        let original = ControlRequest::prepare(
            catalog.config.project_id(),
            client.host(),
            "process:towermd-cli",
            &id,
            ControlAction::ResumeAll,
        )
        .map_err(io_error)?;
        let stored = QueuedControl {
            schema: 1,
            directory: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            request: original.body().clone(),
            metadata: original.metadata().clone(),
        };
        check_source(registry, token, &catalog)?;
        write_new(&path(app_data, &id)?, &stored)?;
        requests
            .0
            .lock()
            .map_err(io_error)?
            .bound
            .insert(id, token.into());
        stored
    };
    let _sending = sending(requests, token, &stored.metadata.request_id)?;
    check_source(registry, token, &catalog)?;
    let result = execute(&stored.directory, &stored.metadata.request_id)
        .and_then(|output| crate::cli::parse_result_stdout(&output.stdout));
    client.verify_control_instance()?;
    check_source(registry, token, &catalog)?;
    let outcome = finish(app_data, &stored.metadata.request_id, result, &stored)?;
    if outcome.state == "confirmed" {
        Ok(())
    } else {
        Err(outcome.error.unwrap_or_else(|| {
            AppError::new("request_uncertain", "恢复请求尚未确定，重试保留原UUID")
        }))
    }
}
#[cfg(test)]
#[path = "intervention_requests_tests.rs"]
mod tests;
