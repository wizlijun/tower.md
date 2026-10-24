//! Templates and frames (core spec FR-141–146, FR-153). Anyone the daemon
//! knows may list templates. Creating, moving or resizing a frame is a
//! person's act: only the App reaches those routes, signed `human:local`,
//! and the CLI has no command for them (CLI spec, 2026-10-23 ruling). The
//! App names a template; the daemon reads it from what ships and what is
//! installed on main, and freezes it into the event, so the App can never
//! supply a definition of its own.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{
    extensions::Registry,
    frames::{self, Created, Frozen, Mode, Place, Placed, RulesInput, SetBy},
    hub_protocol::NewEvent,
    project_runtime::RuntimeError,
};

const HUMAN: &str = "human:local";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    request_id: String,
    id: String,
    title: String,
    template: String,
    #[serde(default)]
    set_by: Option<SetBy>,
    #[serde(default)]
    rules: RulesInput,
    place: Place,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    request_id: String,
    place: Place,
}

fn invalid(error: RuntimeError) -> ApiError {
    match error {
        RuntimeError::Invalid(message) => bad(message),
        other => uncertain(other),
    }
}

pub async fn templates(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let registry = Registry::installed(&project.root).map_err(uncertain)?;
        Ok(Json(crate::templates::list(&registry)))
    }).await.map_err(uncertain)?
}

/// The `frame.created` body for this request, from the template as it is now.
fn created(project: &Project, input: &Create) -> Result<Created, ApiError> {
    let registry = Registry::installed(&project.root).map_err(uncertain)?;
    let entry = crate::templates::available(&registry).into_iter().find(|entry| entry.id == input.template)
        .ok_or_else(|| rejected_as(ErrorCode::NotFound)(format!("没有模板 {}", input.template)))?;
    let template = entry.template.map_err(|problems| bad(format!("模板 {} 有问题，不能使用：{}", input.template, problems.join("；"))))?;
    let rules = template.rules(input.rules.clone()).map_err(rejected_as(ErrorCode::Usage))?;
    let body = Created {
        title: input.title.clone(),
        template: Frozen { id: template.id.clone(), version: template.version.clone(), definition: entry.definition },
        set_by: input.set_by.unwrap_or(template.default_set_by),
        mode: Mode::Free,
        rules,
        place: input.place,
    };
    frames::check_created(&body).map_err(rejected_as(ErrorCode::Usage))?;
    Ok(body)
}

pub async fn create(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Create>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&input.request_id).map_err(bad)?;
    frames::check_id(&input.id).map_err(rejected_as(ErrorCode::Usage))?;
    frames::check_title(&input.title).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            // A retry is the same person's same request, whatever main holds now.
            Some(old) => {
                let same = serde_json::from_value::<Created>(old.data.clone()).is_ok_and(|body| body.title == input.title
                    && body.template.id == input.template && body.place == input.place
                    && input.set_by.is_none_or(|set_by| set_by == body.set_by));
                if old.host != server.endpoint.host || old.actor != HUMAN || old.kind != frames::CREATED || old.obj != input.id || !same {
                    return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容"));
                }
                old
            }
            None => {
                if runtime.projection().get(frames::GROUP).is_some_and(|group| group.get(&input.id).is_some()) {
                    return Err(rejected_as(ErrorCode::Conflict)("容器ID已存在"));
                }
                let data = serde_json::to_value(created(&project, &input)?).map_err(bad)?;
                NewEvent { eid: input.request_id.clone(), host: server.endpoint.host.clone(), actor: HUMAN.into(),
                    kind: frames::CREATED.into(), layer: frames::LAYER.into(), obj: input.id.clone(), data, guards: vec![] }
            }
        };
        let event = runtime.append(request, now).map_err(invalid)?;
        // A frame a person placed is published at once, like a human task.
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "frame":runtime.projection()[frames::GROUP][&event.obj]})))
    }).await.map_err(uncertain)?
}

pub async fn place(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Move>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&input.request_id).map_err(bad)?;
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    frames::check_place(&input.place).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let data = serde_json::to_value(Placed { place: input.place }).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == frames::PLACED
                && old.obj == id && old.data == data => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => NewEvent { eid: input.request_id, host: server.endpoint.host.clone(), actor: HUMAN.into(),
                kind: frames::PLACED.into(), layer: frames::LAYER.into(), obj: id, data, guards: vec![] },
        };
        // Moving a frame waits for the next due snapshot, like a signal.
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "frame":runtime.projection()[frames::GROUP][&event.obj]})))
    }).await.map_err(uncertain)?
}
