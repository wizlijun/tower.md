//! Task drill-downs and the person's task actions (spec 15.5, 16.2). Reads
//! serve any local client; reassigning, pausing and asking for a report are
//! the person's actions and come only from the App. In v0.1 Tower.md does not
//! control harness processes: pausing and asking for a report are prompts the
//! delegate reads at its next checkpoint, and reassigning releases the run's
//! leases and puts the task back to ready. Nothing here completes a task.
use super::business::{now, ready, uncertain, open_failure};
use super::reviews::{accepted, append, same_or_new};
use super::*;
use towermd_core::{hub_protocol::NewEvent, project_runtime::RuntimeError, question_model::PromptScope, question_projection, reviews as core_reviews};
use towermd_devkit::{dev_state, task_model::{ReleaseReason, RunState, TaskEvent}};

const HUMAN: &str = "human:local";
/// The canned prompts behind "pause" and "report" (v0.1: delivered as prompts).
pub const PAUSE: &str = "请在下一个安全检查点暂停当前任务并汇报进度。保留当前工作；继续执行前先等待明确的恢复指令，并确认任务仍由你领取。";
pub const REPORT: &str = "请在下一个回合结束后汇报当前任务：已完成的变更、实际验证证据、阻塞问题与下一步。自述完成不能替代完成验证和合入主干的认证。";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reassign {
    request_id: String,
    /// The run the person saw; a newer run is not ended by a stale click.
    run: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canned {
    request_id: String,
}

fn client(headers: &HeaderMap, server: &Server) -> Result<(), ApiError> {
    if crate::daemon::authorized_client(headers, server) { Ok(()) } else { Err(denied()) }
}

/// Reassign (spec 15.5, v0.1): the run ends, its leases are released and the
/// task is ready for anyone; a gate failure stays held for the person.
pub async fn reassign(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Reassign>) -> Result<Json<Value>, ApiError> {
    super::delegations::app(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let action = serde_json::to_value(TaskEvent::Released { run_id: input.run.clone(), reason: ReleaseReason::Reassigned }).map_err(bad)?;
        let request = same_or_new(runtime, NewEvent { eid: input.request_id, host: server.endpoint.host.clone(), actor: HUMAN.into(),
            kind: "task.released".into(), layer: "L4".into(), obj: id, data: action, guards: vec![] })?;
        if runtime.receipt(&request.eid).is_none() {
            let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
            let task = projection.tasks.get(&request.obj).ok_or_else(|| rejected_as(ErrorCode::NotFound)("任务不存在"))?;
            if task.active_run.as_deref() != Some(input.run.as_str()) {
                return Err(rejected_as(ErrorCode::Conflict)("任务的当前运行已变化，请刷新后再重派"));
            }
        }
        let event = append(runtime, request)?;
        let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
        if let Some(run) = projection.runs.get(&input.run).filter(|run| run.state != RunState::Active) {
            match runtime.release_work_leases(run.fences.clone(), now) {
                Ok(_) | Err(RuntimeError::Fenced(_)) => {}
                Err(error) => return Err(uncertain(error)),
            }
        }
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event))
    }).await.map_err(uncertain)?
}

async fn canned(server: Arc<Server>, headers: HeaderMap, handle: String, task: String, request_id: String, text: &str) -> Result<Json<Value>, ApiError> {
    let request = super::questions::PromptRequest { request_id, scope: PromptScope::Task { task }, text: text.into(), targets: vec![] };
    super::questions::prompt(State(server), headers, RoutePath(handle), Json(request)).await
}

/// Pause (spec 15.5, v0.1): a prompt asking the delegate to stop at its next
/// checkpoint. The task state does not change on the person's word alone.
pub async fn pause(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Canned>) -> Result<Json<Value>, ApiError> {
    canned(server, headers, handle, id, input.request_id, PAUSE).await
}

/// Ask for a report (spec 15.5, v0.1): a prompt; the report is self-reported.
pub async fn report(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Canned>) -> Result<Json<Value>, ApiError> {
    canned(server, headers, handle, id, input.request_id, REPORT).await
}

/// `GET /tasks/:id`: the task, its card, runs, delegation and rules in force.
pub async fn task(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>) -> Result<Json<Value>, ApiError> {
    client(&headers, &server)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let task = runtime.projection()["Task"].get(&id).cloned().ok_or_else(|| rejected_as(ErrorCode::NotFound)("任务不存在"))?;
        let runs: Vec<Value> = runtime.projection()["Run"].as_object().into_iter().flatten()
            .filter(|(_, run)| run["task"] == id.as_str()).map(|(_, run)| run.clone()).collect();
        let module = task["input"]["module"].as_str().unwrap_or_default();
        let temp_rules = super::temp_rules::for_task(runtime.projection(), &id, module).map_err(bad)?;
        let delegation = super::delegations::task_details(runtime, &project, &id).map_err(bad)?;
        let card = super::tasks::card(runtime, &project, &id).map_err(bad)?;
        Ok(Json(json!({"task":task,"runs":runs,"delegation":delegation,"temp_rules":temp_rules,"task_card":card})))
    }).await.map_err(uncertain)?
}

fn document(document: &towermd_core::okf::Document) -> Value {
    json!({"front":document.front(),"body":document.body()})
}

/// `GET /modules/:id`: the module on main, the contracts and acceptance that
/// name it, and its tasks.
pub async fn module(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>) -> Result<Json<Value>, ApiError> {
    client(&headers, &server)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project).map_err(bad)?;
        let module = catalog.objects.get(&id).filter(|object| object.doc_type() == Some("Module"))
            .ok_or_else(|| rejected_as(ErrorCode::NotFound)("main 上没有这个模块"))?;
        let names = |object: &towermd_core::okf::Document, keys: &[&str]| keys.iter().any(|key| object.get_str(key) == Some(id.as_str()));
        let related = |kind: &str, keys: &[&str]| -> Vec<Value> {
            catalog.objects.iter().filter(|(_, object)| object.doc_type() == Some(kind) && names(object, keys))
                .map(|(key, object)| json!({"id":key,"object":document(object)})).collect()
        };
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let tasks: Vec<Value> = runtime.projection()["Task"].as_object().into_iter().flatten()
            .filter(|(_, task)| task["input"]["module"] == id.as_str() || task["input"]["touches"].as_array().is_some_and(|touches| touches.iter().any(|m| m == id.as_str())))
            .map(|(_, task)| task.clone()).collect();
        Ok(Json(json!({"id":id,"revision":catalog.revision,"module":document(module),
            "interfaces":related("Interface", &["from", "to"]),"acceptance":related("Acceptance", &["module"]),"tasks":tasks})))
    }).await.map_err(uncertain)?
}

/// `GET /holds`: what waits on a person, by kind. The attention list ranks
/// them; this is the plain inventory.
pub async fn holds(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>) -> Result<Json<Value>, ApiError> {
    client(&headers, &server)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let state = runtime.projection();
        let questions = question_projection::read(state).map_err(bad)?;
        let questions: Vec<Value> = questions.holds.iter()
            .filter(|(id, _)| questions.answers.get(*id).is_none_or(|answer| !answer.effective()))
            .map(|(id, question)| json!({"id":id,"task":question.binding.task,"run":question.binding.run,"text":question.draft.text,
                "options":question.draft.options,"recommended":question.draft.recommended,"opened_at":question.opened_at,"deadline":question.deadline}))
            .collect();
        let reviews: Vec<Value> = core_reviews::read(state).map_err(bad)?.into_iter()
            .filter(|(_, review)| review.open()).map(|(id, review)| json!({"id":id,"review":review})).collect();
        let interrupts: Vec<Value> = dev_state::tasks(state).map_err(bad)?.tasks.into_iter()
            .filter_map(|(id, task)| task.interrupt.filter(|interrupt| !interrupt.kind.live()).map(|interrupt| json!({"task":id,"state":task.state,"interrupt":interrupt})))
            .collect();
        Ok(Json(json!({"questions":questions,"reviews":reviews,"interrupts":interrupts})))
    }).await.map_err(uncertain)?
}
