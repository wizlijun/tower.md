//! Decision review (core spec FR-186). Keeping or voiding a decision that
//! came due is a person's act: only the App reaches the route, signed
//! `human:local`; the CLI has none. Rewriting is answering the question anew.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{decision_review::{self, Reviewed}, hub_protocol::NewEvent, project_runtime::RuntimeError};

const HUMAN: &str = "human:local";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review { request_id: String, outcome: Outcome, #[serde(default)] review_on: Option<String> }

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome { Keep, Void }

impl Review {
    fn outcome(&self) -> Result<Reviewed, String> {
        match (&self.outcome, &self.review_on) {
            (Outcome::Keep, review_on) => Ok(Reviewed::Keep { review_on: review_on.clone() }),
            (Outcome::Void, None) => Ok(Reviewed::Void {}),
            (Outcome::Void, Some(_)) => Err("作废的决策不再复查，不能带复查日期".into()),
        }
    }
}

fn invalid(error: RuntimeError) -> ApiError {
    match error { RuntimeError::Invalid(message) => bad(message), other => uncertain(other) }
}

pub async fn review(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Review>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    valid_scan_id(&input.request_id).map_err(bad)?;
    let outcome = input.outcome().map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let data = serde_json::to_value(&outcome).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == decision_review::REVIEWED && old.obj == id && old.data == data => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => NewEvent { eid: input.request_id.clone(), host: server.endpoint.host.clone(), actor: HUMAN.into(),
                kind: decision_review::REVIEWED.into(), layer: decision_review::LAYER.into(), obj: id, data, guards: vec![] },
        };
        // The reducer refuses a decision that is missing, superseded or already voided.
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "review":runtime.projection().get(decision_review::GROUP).and_then(|group| group.get(&event.obj)).cloned()})))
    }).await.map_err(uncertain)?
}
