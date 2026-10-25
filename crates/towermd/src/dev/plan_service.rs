//! FR-67 and spec 12.5: a person's global or module input goes to the
//! controller, which previews an action plan (`controllers::pass`); the plan
//! runs only when the person confirms it here, or the person takes over.
//! Both are the person's actions and come only from the App; the command
//! line has no way to confirm a plan, and nothing confirms it on a timeout.
use super::business::{now, ready, uncertain, open_failure};
use super::reviews::{accepted, append, same_or_new};
use super::*;
use towermd_core::hub_protocol::NewEvent;
use towermd_devkit::controller_plan::{self, PlanEvent, Scope, Status, Step, CHANGED};

const HUMAN: &str = "human:local";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    request_id: String,
    text: String,
    /// The module the input was given on; none for the whole project.
    #[serde(default)]
    module: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Confirm,
    TakeOver,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolve {
    request_id: String,
    decision: Decision,
    #[serde(default)]
    note: Option<String>,
}

/// The plan a request opens.
pub fn plan_id(request_id: &str) -> String {
    format!("plan-{request_id}")
}

fn human(server: &Server, eid: String, obj: String, change: &PlanEvent) -> Result<NewEvent, ApiError> {
    Ok(NewEvent { eid, host: server.endpoint.host.clone(), actor: HUMAN.into(), kind: CHANGED.into(), layer: "L4".into(), obj,
        data: serde_json::to_value(change).map_err(bad)?, guards: vec![] })
}

/// Hand the person's input to the controller; the next pass consults it.
pub async fn request(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Request>) -> Result<Json<Value>, ApiError> {
    super::delegations::app(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    let runtime_handle = tokio::runtime::Handle::current();
    let (scheduler, owner) = (server.clone(), project.clone());
    let reply = tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let scope = match input.module { Some(module) => Scope::Module { module }, None => Scope::Global };
        let change = PlanEvent::Requested { scope, text: input.text.trim().to_owned() };
        let request = same_or_new(runtime, human(&server, input.request_id.clone(), plan_id(&input.request_id), &change)?)?;
        let event = append(runtime, request)?;
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event))
    }).await.map_err(uncertain)?;
    super::schedules::kick(runtime_handle, scheduler, owner);
    reply
}

/// Confirm the previewed plan, or take over; the delegates of paused or
/// resumed tasks are told, as when the person does it by hand.
pub async fn resolve(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Resolve>) -> Result<Json<Value>, ApiError> {
    super::delegations::app(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    let runtime_handle = tokio::runtime::Handle::current();
    let (scheduler, owner) = (server.clone(), project.clone());
    let reply = tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let note = input.note.map(|note| note.trim().to_owned()).filter(|note| !note.is_empty());
        let change = match input.decision { Decision::Confirm => PlanEvent::Confirmed { note }, Decision::TakeOver => PlanEvent::TakenOver { note } };
        let request = same_or_new(runtime, human(&server, input.request_id.clone(), id.clone(), &change)?)?;
        let plan = controller_plan::plans(runtime.projection()).map_err(bad)?.remove(&id)
            .ok_or_else(|| rejected_as(ErrorCode::NotFound)("计划不存在"))?;
        if runtime.receipt(&request.eid).is_none() && matches!(change, PlanEvent::Confirmed { .. }) && plan.status != Status::Previewed {
            return Err(rejected_as(ErrorCode::Conflict)("计划已处理或还没有预览，请刷新"));
        }
        let event = append(runtime, request)?;
        if matches!(change, PlanEvent::Confirmed { .. }) {
            for step in &plan.steps {
                let text = match step { Step::Pause { .. } => super::task_app::PAUSE, Step::Resume { .. } => super::task_app::RESUME, _ => continue };
                super::questions::task_notice(runtime, &server, &format!("{}-{}", input.request_id, step.task()), "prompt.queued", step.task(), text)?;
            }
        }
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event))
    }).await.map_err(uncertain)?;
    // The scheduler stops paused runs and dispatches new or resumed tasks.
    super::schedules::kick(runtime_handle, scheduler, owner);
    reply
}
