//! Follow-ups, quiet objects and resurfacing (core spec FR-134–136). Adding
//! or completing a follow-up and settling a resurfaced object are a person's
//! acts: only the App reaches these routes, signed `human:local`; the CLI has
//! none. Which follow-ups are due and which objects are quiet or resurfaced is
//! derived on every state read from the event history and never stored.
use std::collections::BTreeSet;

use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{config::Config, delegation::Phase, delegation_projection, follow_ups::{self, Added, When},
    hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}, quiet, tower_api};
use towermd_devkit::dev_state::KITS;

const HUMAN: &str = "human:local";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Add { request_id: String, text: String, when: When }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Act { request_id: String }

fn invalid(error: RuntimeError) -> ApiError {
    match error { RuntimeError::Invalid(message) => bad(message), other => uncertain(other) }
}

fn project_id(project: &Project, config: Option<&Config>) -> Result<String, String> {
    config.map(|config| config.project_id().to_owned()).or_else(|| project.project_id.clone()).ok_or_else(|| "项目尚未初始化".into())
}

/// FR-134–136 over the kits' objects and the project itself.
pub(super) fn rhythm(runtime: &ProjectRuntime, project: &Project) -> Result<quiet::Rhythm, String> {
    let state = runtime.projection();
    let config = Config::load(&project.root).ok().flatten();
    let settings = config.as_ref().map(Config::quiet).unwrap_or_default();
    let id = project_id(project, config.as_ref())?;
    let items = delegation_projection::delegations(state, KITS)?;
    let objects: Vec<String> = items.iter().map(|item| item.id.clone()).collect();
    let done: BTreeSet<String> = items.iter().filter(|item| item.phase == Phase::Done).map(|item| item.id.clone()).collect();
    let signals = tower_api::read(state)?.signals;
    let follow_ups = follow_ups::read(state)?;
    Ok(quiet::rhythm(runtime.canonical_receipts(), &quiet::Inputs { now: quiet::instant(now()), project: &id,
        objects: &objects, done: &done, signals: &signals, follow_ups: &follow_ups, settings: &settings }))
}

/// Derived for the App on every state read; never persisted.
pub(super) fn rhythm_value(runtime: &ProjectRuntime, project: &Project) -> Value {
    match rhythm(runtime, project) {
        Ok(rhythm) => serde_json::to_value(rhythm).unwrap_or(Value::Null),
        Err(error) => { eprintln!("休眠与浮现暂不可用：{error}"); Value::Null }
    }
}

/// A follow-up watches the project, a kit object, or a target someone
/// already reported or signalled on; anything else is a typo.
fn known_target(runtime: &ProjectRuntime, project: &Project, target: &str) -> Result<bool, String> {
    let state = runtime.projection();
    let config = Config::load(&project.root).ok().flatten();
    if project_id(project, config.as_ref())? == target { return Ok(true) }
    if delegation_projection::delegations(state, KITS)?.iter().any(|item| item.id == target) { return Ok(true) }
    let log = tower_api::read(state)?;
    Ok(log.signals.iter().any(|entry| entry.target == target) || log.reports.iter().any(|entry| entry.target == target))
}

/// Replays the same request or refuses a reused ID; otherwise builds `fresh`.
fn request(runtime: &ProjectRuntime, server: &Server, request_id: &str, kind: &str, obj: &str, data: &Value,
    fresh: impl FnOnce() -> Result<(), ApiError>) -> Result<NewEvent, ApiError> {
    match human_tasks::original(runtime, request_id).map_err(bad)? {
        Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == kind && old.obj == obj && old.data == *data => Ok(old),
        Some(_) => Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
        None => {
            fresh()?;
            Ok(NewEvent { eid: request_id.into(), host: server.endpoint.host.clone(), actor: HUMAN.into(), kind: kind.into(),
                layer: follow_ups::LAYER.into(), obj: obj.into(), data: data.clone(), guards: vec![] })
        }
    }
}

fn accepted(runtime: &ProjectRuntime, event: &towermd_core::hub::Event) -> Json<Value> {
    Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
        "follow_up":runtime.projection().get(follow_ups::GROUP).and_then(|group| group.get(&event.obj)).cloned()}))
}

pub async fn add(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Add>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    valid_scan_id(&input.request_id).map_err(bad)?;
    follow_ups::check_text(&input.text).map_err(|e| rejected_as(ErrorCode::Usage)(e))?;
    input.when.check().map_err(|e| rejected_as(ErrorCode::Usage)(e))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let data = serde_json::to_value(Added { text: input.text, when: input.when.clone() }).map_err(bad)?;
        let request = request(runtime, &server, &input.request_id, follow_ups::ADDED, &input.request_id, &data, || {
            match input.when.target() {
                Some(target) if !known_target(runtime, &project, target).map_err(bad)? =>
                    Err(rejected_as(ErrorCode::NotFound)(format!("没有对象 {target}"))),
                _ => Ok(()),
            }
        })?;
        // Like an idea, a follow-up waits for the next due snapshot.
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(accepted(runtime, &event))
    }).await.map_err(uncertain)?
}

pub async fn done(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Act>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let request = request(runtime, &server, &input.request_id, follow_ups::DONE, &id, &json!({}), || Ok(()))?;
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(accepted(runtime, &event))
    }).await.map_err(uncertain)?
}

/// "看过了" on a resurfaced object (FR-136). Only an object that is
/// resurfaced right now can be settled; a replay of the same request is not
/// judged again.
pub async fn seen(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Act>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let request = request(runtime, &server, &input.request_id, follow_ups::SEEN, &id, &json!({}), || {
            let rhythm = rhythm(runtime, &project).map_err(bad)?;
            if rhythm.objects.iter().any(|object| object.id == id && object.resurfaced.is_some()) { Ok(()) }
            else { Err(rejected_as(ErrorCode::Conflict)("这个对象现在没有浮现")) }
        })?;
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","object":event.obj})))
    }).await.map_err(uncertain)?
}
