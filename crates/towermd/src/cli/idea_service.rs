//! Ideas and their promotion (core spec FR-181–183). Anyone jots an idea:
//! from the App it is signed `human:local`, from the CLI `process:towermd-cli`.
//! Promoting it into a commitment is a person's act, so only the App reaches
//! that route; the CLI has no `promote`. A task promotion is built here from
//! the actual published baseline, exactly like the App's own task creation.
use super::business::{now, ready, uncertain, open_failure, runtime_failure};
use super::*;
use towermd_core::{hub_protocol::NewEvent, ideas::{self, Promote}, project_runtime::RuntimeError};
use towermd_devkit::{dev_state, task_model::{self, TaskDraft, TaskOrigin}};

const HUMAN: &str = "human:local";
const CLI: &str = "process:towermd-cli";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Add { request_id: String, text: String }

/// `as` is `project` (the sentence becomes the goal) or `task` with a draft.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Promotion {
    request_id: String,
    #[serde(rename = "as")]
    kind: String,
    #[serde(default)]
    task: Option<TaskDraft>,
}

fn invalid(error: RuntimeError) -> ApiError {
    match error { RuntimeError::Invalid(message) => bad(message), other => runtime_failure(other) }
}

pub async fn add(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Add>) -> Result<Json<Value>, ApiError> {
    let actor = if crate::daemon::authorized_app(&headers, &server) { HUMAN }
        else { tasks::require_cli(&headers, &server)?; CLI };
    valid_scan_id(&input.request_id).map_err(bad)?;
    ideas::check_text(&input.text).map_err(|e| rejected_as(ErrorCode::Usage)(e))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let data = serde_json::to_value(ideas::Added { text: input.text }).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == actor && old.kind == ideas::ADDED
                && old.obj == input.request_id && old.data == data => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => NewEvent { eid: input.request_id.clone(), host: server.endpoint.host.clone(), actor: actor.into(),
                kind: ideas::ADDED.into(), layer: ideas::LAYER.into(), obj: input.request_id, data, guards: vec![] },
        };
        // Like signals, an idea waits for the next due snapshot.
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "idea":runtime.projection()[ideas::GROUP][&event.obj]})))
    }).await.map_err(uncertain)?
}

pub async fn promote(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Promotion>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let request = if let Some(old) = human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            let same = serde_json::from_value::<Promote>(old.data.clone()).is_ok_and(|body| body.kind == input.kind
                && (input.kind != dev_state::PROMOTE_TASK || dev_state::promoted_task(&old.kind, &old.data).ok().flatten()
                    .is_some_and(|task| Some(&task.input) == input.task.as_ref())));
            if old.host != server.endpoint.host || old.actor != HUMAN || old.kind != ideas::PROMOTED || old.obj != id || !same {
                return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他立项"));
            }
            old
        } else {
            let body = match (input.kind.as_str(), input.task) {
                (ideas::AS_PROJECT, None) => Promote { kind: input.kind, target: None, object: None },
                (dev_state::PROMOTE_TASK, Some(draft)) => {
                    let baseline = tasks::baseline(&project, runtime).map_err(bad)?;
                    let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
                    let target = draft.id.clone();
                    let action = task_model::prepare_add(&baseline, &projection, draft, TaskOrigin::AppHuman).map_err(bad)?;
                    Promote { kind: input.kind, target: Some(target), object: Some(serde_json::to_value(action).map_err(bad)?) }
                }
                _ => return Err(bad("立项只能立为项目，或带任务草稿立为任务")),
            };
            NewEvent { eid: input.request_id, host: server.endpoint.host.clone(), actor: HUMAN.into(),
                kind: ideas::PROMOTED.into(), layer: ideas::LAYER.into(), obj: id,
                data: serde_json::to_value(body).map_err(bad)?, guards: vec![] }
        };
        let event = runtime.append(request, now).map_err(invalid)?;
        // A commitment a person made is published at once, like a human task.
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "idea":runtime.projection()[ideas::GROUP][&event.obj]})))
    }).await.map_err(uncertain)?
}
