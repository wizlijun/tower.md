//! Starter kits (core spec FR-189). The kits ship as first-party manifests:
//! research and writing with `towermd`, software development with the dev
//! kit. Which kits are recommended is derived on every state read; adopting
//! or dismissing one is a person's act, only from the App, signed
//! `human:local`. The CLI has no command for it.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{delegation_projection, frames, hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}, starters::{self, Kit}};
use towermd_devkit::dev_state::KITS;

const HUMAN: &str = "human:local";
const MANIFESTS: [&str; 3] = [
    towermd_devkit::STARTER,
    include_str!("../../starters/research.yaml"),
    include_str!("../../starters/writing.yaml"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Act { request_id: String }

pub(crate) fn kits() -> Vec<Kit> {
    MANIFESTS.iter().map(|text| Kit::parse(text).expect("first-party starter kit")).collect()
}

/// Derived for the App on every state read; never persisted. A project has
/// content once it has a frame or an object with a delegation.
pub(super) fn view_value(runtime: &ProjectRuntime) -> Value {
    let state = runtime.projection();
    let content = frames::read(state).map(|all| !all.is_empty()).unwrap_or(false)
        || delegation_projection::delegations(state, KITS).map(|items| !items.is_empty()).unwrap_or(false);
    match starters::view(state, &kits(), content) {
        Ok(view) => serde_json::to_value(view).unwrap_or(Value::Null),
        Err(error) => { eprintln!("起步套装暂不可用：{error}"); Value::Null }
    }
}

async fn choose(server: Arc<Server>, headers: HeaderMap, handle: String, id: String, request_id: String, kind: &'static str) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    valid_scan_id(&request_id).map_err(bad)?;
    if !kits().iter().any(|kit| kit.id == id) {
        return Err(rejected_as(ErrorCode::NotFound)(format!("没有起步套装 {id}")));
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let request = match human_tasks::original(runtime, &request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == kind && old.obj == id => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => NewEvent { eid: request_id, host: server.endpoint.host.clone(), actor: HUMAN.into(), kind: kind.into(),
                layer: starters::LAYER.into(), obj: id, data: json!({}), guards: vec![] },
        };
        let event = runtime.append(request, now).map_err(|error| match error {
            RuntimeError::Invalid(message) => bad(message), other => uncertain(other),
        })?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","starter":event.obj,
            "starters":view_value(runtime)})))
    }).await.map_err(uncertain)?
}

pub async fn adopt(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Act>) -> Result<Json<Value>, ApiError> {
    choose(server, headers, handle, id, input.request_id, starters::ADOPTED).await
}

pub async fn dismiss(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Act>) -> Result<Json<Value>, ApiError> {
    choose(server, headers, handle, id, input.request_id, starters::DISMISSED).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_first_party_kits_parse_and_change_no_rule() {
        let ids: Vec<String> = kits().into_iter().map(|kit| kit.id).collect();
        assert_eq!(ids, ["software", "research", "writing"]);
        let names = &kits()[2].names;
        assert_eq!(names["Task"], "章节");
    }
}
