//! Task drill-downs and the person's task actions (spec 15.5, 16.2). Reads
//! serve any local client; reassigning, pausing, resuming and asking for a
//! report are the person's actions and come only from the App. Pausing marks
//! the task paused, so nothing claims or dispatches it, stops a dispatched
//! run and tells a claimed delegate to stop at its next checkpoint; resuming
//! puts it back. Reassigning releases the run's leases and puts the task back
//! to ready. Nothing here completes a task.
use super::business::{now, ready, uncertain, open_failure};
use super::reviews::{accepted, append, same_or_new};
use super::*;
use towermd_core::{hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}, question_model::PromptScope, question_projection, reviews as core_reviews};
use towermd_devkit::{dev_state, task_model::{ReleaseReason, RunState, TaskEvent, TaskState}};

const HUMAN: &str = "human:local";
/// The canned prompts behind "pause", "resume" and "report", delivered to
/// the task's live delegates.
pub const PAUSE: &str = "请在下一个安全检查点暂停当前任务并汇报进度。保留当前工作；继续执行前先等待明确的恢复指令，并确认任务仍由你领取。";
pub const RESUME: &str = "App 中的人已恢复此任务。请先通过 towermd status 确认任务仍由你领取、项目没有暂停，再按当前租约继续；本消息不证明任何进度。";
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

/// Pause (spec 15.5): the task is paused, so no claim or dispatch takes it
/// and the scheduler stops a dispatched run; a claimed delegate is told to
/// stop at its next checkpoint. The work stays where it is.
pub async fn pause(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Canned>) -> Result<Json<Value>, ApiError> {
    pause_or_resume(server, headers, handle, TaskEvent::Paused { task_id: id }, input.request_id).await
}

/// Resume (spec 15.5): the task goes back to where it was; a held task stays
/// held. A live delegate is told it may go on; the scheduler dispatches a
/// task whose run ended anew.
pub async fn resume(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Canned>) -> Result<Json<Value>, ApiError> {
    pause_or_resume(server, headers, handle, TaskEvent::Resumed { task_id: id }, input.request_id).await
}

async fn pause_or_resume(server: Arc<Server>, headers: HeaderMap, handle: String, action: TaskEvent, request_id: String) -> Result<Json<Value>, ApiError> {
    super::delegations::app(&headers, &server)?;
    valid_scan_id(&request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    let (kind, text, id) = match &action {
        TaskEvent::Paused { task_id } => ("task.paused", PAUSE, task_id.clone()),
        TaskEvent::Resumed { task_id } => ("task.resumed", RESUME, task_id.clone()),
        _ => return Err(bad("不是暂停或继续")),
    };
    let handle = tokio::runtime::Handle::current();
    let (scheduler, owner) = (server.clone(), project.clone());
    let reply = tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let data = serde_json::to_value(&action).map_err(bad)?;
        let request = same_or_new(runtime, NewEvent { eid: request_id.clone(), host: server.endpoint.host.clone(), actor: HUMAN.into(),
            kind: kind.into(), layer: "L4".into(), obj: id.clone(), data, guards: vec![] })?;
        if runtime.receipt(&request.eid).is_none() && dev_state::tasks(runtime.projection()).map_err(bad)?.tasks.get(&id).is_none() {
            return Err(rejected_as(ErrorCode::NotFound)("任务不存在"));
        }
        let event = append(runtime, request)?;
        let notice = super::questions::task_notice(runtime, &server, &request_id, "prompt.queued", &id, text)?;
        runtime.snapshot(now).map_err(uncertain)?;
        let Json(mut reply) = accepted(&event);
        reply["prompt"] = json!(notice.map(|notice| notice.eid));
        Ok(Json(reply))
    }).await.map_err(uncertain)??;
    // The scheduler stops a paused task's dispatched run, or dispatches a resumed one.
    super::schedules::kick(handle, scheduler, owner);
    Ok(reply)
}

/// Ask for a report (spec 15.5, v0.1): a prompt; the report is self-reported.
pub async fn report(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Canned>) -> Result<Json<Value>, ApiError> {
    canned(server, headers, handle, id, input.request_id, REPORT).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rollback {
    request_id: String,
    /// The merge the person saw: a certified admission commit of this task.
    commit: String,
}

const GIT_STEP: std::time::Duration = std::time::Duration::from_secs(120);

/// Rollback (FR-13): a person reverts one merge of a task from the App. The
/// daemon prepares one revert commit on the main it reads now (the task's
/// changes `main_base..admission` applied in reverse by a three-way merge),
/// records the person's approval of that exact commit, and fast-forwards
/// main through the single write module. A conflict refuses and lists the
/// paths; nothing is resolved automatically and main is never rewritten.
pub async fn rollback(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Rollback>) -> Result<Json<Value>, ApiError> {
    super::delegations::app(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let conflict = |message: &str| rejected_as(ErrorCode::Conflict)(message);
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        runtime.tick(now).map_err(uncertain)?;
        if !runtime.holds_main() { return Err(conflict("本机守护进程不持有主干租约，不能写 main")); }
        let approval = match runtime.receipt(&input.request_id).cloned() {
            Some(old) => {
                if old.kind != dev_state::ROLLBACK_APPROVED || old.actor != HUMAN || old.obj != id || old.data["admission"] != input.commit.as_str() {
                    return Err(conflict("请求 ID 已用于其他操作"));
                }
                old
            }
            None => {
                let request = prepare_rollback(&project, runtime, &server, &input.request_id, &id, &input.commit)?;
                append(runtime, request)?
            }
        };
        let (base, revert) = (approval.data["base"].as_str().unwrap_or_default().to_owned(), approval.data["revert"].as_str().unwrap_or_default().to_owned());
        let done = towermd_core::events::derived_eid(&revert, dev_state::ROLLED_BACK).map_err(bad)?;
        if runtime.receipt(&done).is_none() {
            let main = current_catalog(&project).map_err(uncertain)?.revision;
            if main == base {
                runtime.advance(&base, &revert, now).map_err(uncertain)?;
            } else if !repo::git_timeout_env_output(&project.root, &["merge-base", "--is-ancestor", &revert, &main], GIT_STEP, &[]).map_err(uncertain)?.status.success() {
                return Err(conflict("批准后 main 已推进，这次回滚没有执行；请刷新后重新发起"));
            }
            append(runtime, NewEvent { eid: done, host: server.endpoint.host.clone(), actor: "process:towermd".into(),
                kind: dev_state::ROLLED_BACK.into(), layer: "L4".into(), obj: id.clone(),
                data: json!({"task": id, "approval": approval.eid, "admission": approval.data["admission"], "revert": revert}),
                guards: vec![runtime.main_fence().map_err(uncertain)?] })?;
        }
        runtime.snapshot(now).map_err(uncertain)?;
        let Json(mut reply) = accepted(&approval);
        reply["revert"] = json!(revert);
        Ok(Json(reply))
    }).await.map_err(uncertain)?
}

/// The person's approval of one revert commit, prepared on current main.
fn prepare_rollback(project: &Project, runtime: &ProjectRuntime, server: &Server, request_id: &str, task: &str, admission: &str) -> Result<NewEvent, ApiError> {
    let conflict = |message: &str| rejected_as(ErrorCode::Conflict)(message);
    let tasks = dev_state::tasks(runtime.projection()).map_err(bad)?;
    let state = tasks.tasks.get(task).ok_or_else(|| rejected_as(ErrorCode::NotFound)("任务不存在"))?.state;
    if state != TaskState::Merged { return Err(conflict("只有已合入主干的任务能回滚")); }
    let entry = runtime.projection()["Evidence"].as_object().into_iter().flatten().map(|(_, entry)| entry)
        .find(|entry| entry["task"] == task && entry["status"] == "merged" && entry["merge"]["admission_commit"] == admission)
        .ok_or_else(|| conflict("这不是该任务已认证的合入提交"))?;
    if entry.get("rollback").is_some() { return Err(conflict("这次合入已有回滚批准")); }
    let main_base = entry["g5"]["data"]["main_base"].as_str().ok_or_else(|| bad("合入证据缺少 main 基底"))?;
    let catalog = current_catalog(project).map_err(uncertain)?;
    let main = catalog.revision.as_str();
    let git = |args: &[&str]| repo::git_timeout_env_output(&project.root, args, GIT_STEP, &[]).map_err(uncertain);
    if !git(&["merge-base", "--is-ancestor", admission, main])?.status.success() {
        return Err(conflict("该合入不在当前 main 上"));
    }
    // Apply main_base..admission in reverse onto main: a three-way merge whose
    // base is the admission. Conflicts stop here for the person to resolve.
    let merged = git(&["merge-tree", "--write-tree", "--name-only", "--no-messages", &format!("--merge-base={admission}"), main, main_base])?;
    let text = String::from_utf8_lossy(&merged.stdout).to_string();
    let mut lines = text.lines();
    let tree = lines.next().unwrap_or_default().to_owned();
    match merged.status.code() {
        Some(0) => {}
        Some(1) => {
            let paths = lines.filter(|line| !line.is_empty()).collect::<Vec<_>>().join("、");
            return Err(conflict(&format!("回滚与之后合入 main 的改动冲突，未做任何改动；冲突文件：{paths}。请先手工处理这些文件再回滚")));
        }
        _ => return Err(uncertain(format!("无法计算回滚：{}", String::from_utf8_lossy(&merged.stderr).trim()))),
    }
    let current = repo::git_timeout(&project.root, &["rev-parse", &format!("{main}^{{tree}}")], GIT_STEP).map_err(uncertain)?;
    if tree == current { return Err(conflict("main 上已没有这次合入的改动，无需回滚")); }
    let message = format!("Revert task {task}\n\nReverts the changes task {task} merged in {admission}\n(from {main_base}), as a person approved in the Tower.md App (request {request_id}).\n");
    let environment: [(&str, &std::ffi::OsStr); 4] = [
        ("GIT_AUTHOR_NAME", "Tower.md".as_ref()), ("GIT_AUTHOR_EMAIL", "towermd@localhost".as_ref()),
        ("GIT_COMMITTER_NAME", "Tower.md".as_ref()), ("GIT_COMMITTER_EMAIL", "towermd@localhost".as_ref()),
    ];
    let output = repo::git_timeout_env_input_output(&project.root, &["-c", "commit.gpgsign=false", "commit-tree", &tree, "-p", main, "-F", "-"],
        GIT_STEP, &environment, Some(message.as_bytes())).map_err(uncertain)?;
    if !output.status.success() { return Err(uncertain(format!("无法生成回滚提交：{}", String::from_utf8_lossy(&output.stderr).trim()))); }
    let revert = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(NewEvent { eid: request_id.into(), host: server.endpoint.host.clone(), actor: HUMAN.into(),
        kind: dev_state::ROLLBACK_APPROVED.into(), layer: "L4".into(), obj: task.into(),
        data: json!({"task": task, "admission": admission, "main_base": main_base, "base": main, "revert": revert}), guards: vec![] })
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
