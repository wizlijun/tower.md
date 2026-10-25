//! Daemon-owned task verification. Jobs and controls are disposable; requests,
//! frozen inputs, reports and G5 receipts are canonical hub/outbox events.
use super::business::{now, ready, uncertain, open_failure};
use super::*;
use towermd_core::{events::derived_eid, hub::Event, hub_protocol::NewEvent, main_inspection::{self, Inspection}, project_runtime::ProjectRuntime};
use towermd_devkit::{dev_state, task_finish::{self, FinishRequest}, task_reviews, task_model::{Run, RunState}};

#[derive(Clone, Debug)]
pub(super) struct FinishJob {
    status: &'static str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    cwd: PathBuf,
    #[serde(default)]
    run: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    request_id: String,
    cwd: PathBuf,
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    task: Option<String>,
    /// FR-209 self-report per criterion; recorded, never completes anything.
    #[serde(default)]
    claims: Vec<task_finish::Claim>,
    /// FR-28 run report; recorded with the evidence, never moves the task.
    #[serde(default)]
    report: Option<towermd_devkit::run_report::RunReport>,
}

pub(super) fn event(server: &Server, eid: &str, kind: &str, object: &str, data: Value, run: Option<&Run>) -> NewEvent {
    NewEvent {
        eid: eid.into(), host: server.endpoint.host.clone(),
        actor: if kind == "task.finish.requested" { "process:towermd-cli" }
            else if kind.starts_with("task.finish.") { "process:towermd-verifier" }
            else { "process:towermd" }.into(),
        kind: kind.into(), layer: "L4".into(), obj: object.into(), data,
        guards: run.map_or_else(Vec::new, |run|run.fences.clone()),
    }
}
pub(super) fn accepted(runtime: &mut ProjectRuntime, input: NewEvent) -> Result<Event, String> {
    let receipt = runtime.append(input, now).map_err(|error|error.to_string())?;
    if receipt.stale { return Err("事件的模块租约已失效，原件已保存但不认证执行结果".into()); }
    Ok(receipt)
}
pub(super) fn unheld(runtime:&ProjectRuntime,run:&str)->Result<(),String>{
    let task=runtime.projection()["Run"][run]["task"].as_str().ok_or("运行缺少任务")?;
    if towermd_core::question_projection::blocks_run(runtime.projection(),run)?
        || towermd_core::question_projection::blocks_task(runtime.projection(),task)?{
        return Err("任务仍有未解决提问，须等待有效回答再继续".into());
    }
    Ok(())
}
fn stored_request(runtime: &ProjectRuntime, id: &str) -> Result<FinishRequest, String> {
    serde_json::from_value(runtime.projection()["Evidence"][id]["request"].clone()).map_err(|_|"验证请求不存在".into())
}
fn job_value(runtime: &ProjectRuntime, id: &str) -> Result<Value, String> {
    let entry = runtime.projection()["Evidence"].get(id).ok_or("验证请求不存在")?;
    if entry["kind"] != "finish" { return Err("该 ID 不是任务验证".into()); }
    Ok(json!({
        "evidence_id":id,"status":entry["status"],"task":entry["task"],"run":entry["run"],
        "error":entry["error"],"finish_eid":entry["completion_eid"],
        "g5_eid":entry["g5_eid"],"evidence":entry["evidence"],"merge":entry["merge"],
    }))
}
pub(super) fn latest(runtime: &ProjectRuntime, run: &str) -> Option<String> {
    runtime.projection()["Evidence"].as_object()?.iter()
        .filter(|(_, entry)| entry["kind"] == "finish" && entry["run"] == run)
        .filter_map(|(id, entry)| Some((runtime.receipt(entry["request_eid"].as_str()?)?.seq, id.clone())))
        .max_by_key(|(seq, _)| *seq).map(|(_, id)| id)
}
/// G4 binds a code review to the tree the run's latest passed finish tested.
pub(super) fn tested_tree(runtime: &ProjectRuntime, run: &str) -> Option<String> {
    let entry = &runtime.projection()["Evidence"][latest(runtime, run)?];
    if entry["status"] != "passed" { return None; }
    let finish = runtime.receipt(entry["completion_eid"].as_str()?)?;
    task_finish::completion_from_event(finish).ok().map(|evidence| evidence.input.tree)
}
fn active_request(runtime: &ProjectRuntime, run: &str) -> Option<String> {
    let id = latest(runtime, run)?;
    matches!(runtime.projection()["Evidence"][&id]["status"].as_str(), Some("queued" | "running")).then_some(id)
}
pub(super) fn replay_pending(runtime: &mut ProjectRuntime) -> Result<(), String> {
    for input in runtime.pending().map_err(|e|e.to_string())? {
        if input.kind.starts_with("task.finish.") || matches!(input.kind.as_str(), "task.g5.completed" | "task.merged" | "main.inspected") {
            // A stale replay is retained by the hub but never advances a task.
            runtime.append(input, now).map_err(|e|e.to_string())?;
        }
    }
    Ok(())
}

/// Observe without hiding blocked or incomplete inspection from the App.
/// Only the remote-tracking ref is fetched; user main/index/worktrees stay put.
pub(super) fn observe_authority(
    server: &Server, project: &Project, runtime: &mut ProjectRuntime,
) -> Result<(Catalog, Inspection), String> {
    let catalog=refresh_authority(project)?;
    inspect_catalog(server,project,runtime,catalog)
}
fn refresh_authority(project:&Project)->Result<Catalog,String>{
    if repo::git(&project.root, &["remote"])?.lines().any(|name|name == "origin") {
        repo::git_timeout(&project.root, &["-c","core.hooksPath=/dev/null","fetch","--no-tags",
            "--no-write-fetch-head","--recurse-submodules=no","origin","+refs/heads/main:refs/remotes/origin/main"], Duration::from_secs(30))?;
    }
    current_catalog(project)
}
fn inspect_catalog(server:&Server,project:&Project,runtime:&mut ProjectRuntime,catalog:Catalog)->Result<(Catalog,Inspection),String>{
    let previous = runtime.projection()["MainInspection"]["eid"].as_str()
        .and_then(|id|runtime.receipt(id)).and_then(|event|{
            if event.data["complete"]==true{event.data["main"].as_str()}else{event.data["previous_main"].as_str()}
        }).map(str::to_string);
    let inspection = main_inspection::inspect(&project.root, &catalog.revision,
        catalog.config.project_id(), catalog.config.docs_dir(), previous.as_deref(),
        &server.endpoint.host, |id|runtime.receipt(id).cloned(), &towermd_devkit::dev_state::ASSEMBLY);
    for alert in &inspection.alerts {
        if runtime.receipt(&alert.eid).is_none() { accepted(runtime, alert.clone())?; }
    }
    if inspection.complete {
        let previous_inspection = runtime.projection()["MainInspection"]["eid"].as_str().unwrap_or("initial").to_owned();
        let incomplete = runtime.projection()["Alert"].as_object().ok_or("告警投影无效")?
            .iter().filter(|(_, alert)|alert["resolved"]!=true && alert["data"]["reason"]=="inspection_incomplete")
            .map(|(id,_)|id.clone()).collect::<Vec<_>>();
        for id in incomplete {
            // Bind the clear to the original alert and preceding accepted
            // observation, so retries reuse it and a later recurrence can
            // have its own clear without rewriting either original.
            let eid = towermd_core::events::derived_eid(&catalog.revision,
                &format!("alert.cleared.inspection_incomplete.{id}.{previous_inspection}"))?;
            if runtime.receipt(&eid).is_none() {
                accepted(runtime, NewEvent {
                    eid,host:server.endpoint.host.clone(),actor:"process:towermd".into(),
                    kind:"alert.cleared".into(),layer:"L5".into(),obj:id,
                    data:json!({"reason":"inspection_complete","main":catalog.revision}),
                    guards:vec![runtime.main_fence().map_err(|e| e.to_string())?],
                })?;
            }
        }
    }
    // FR-125: gates and thresholds that main loosened since the last
    // inspection, raised before it is recorded so a crash retries it.
    if let Some(previous)=&previous {
        super::weakenings::raise_for_main(runtime,&server.endpoint.host,&catalog.root,catalog.config.project_id(),previous,&catalog.revision)?;
    }
    let recorded=&runtime.projection()["MainInspection"];
    if recorded["main"]!=catalog.revision || recorded["passed"]!=inspection.passed || recorded["complete"]!=inspection.complete
        || recorded["error"]!=json!(inspection.error) {
        let eid = Uuid::now_v7().to_string();
        accepted(runtime, NewEvent {
            eid, host:server.endpoint.host.clone(),actor:"process:towermd".into(),
            kind:"main.inspected".into(),layer:"L5".into(),obj:catalog.config.project_id().into(),
            data:json!({"main":catalog.revision,"previous_main":previous,"complete":inspection.complete,"passed":inspection.passed,"error":inspection.error}),
            guards:vec![runtime.main_fence().map_err(|e| e.to_string())?],
        })?;
    }
    Ok((catalog, inspection))
}
pub(super) fn inspect_authority(server: &Server, project: &Project, runtime: &mut ProjectRuntime) -> Result<Catalog, String> {
    let (catalog, inspection) = observe_authority(server, project, runtime)?;
    if !inspection.complete || !inspection.passed {
        return Err(inspection.error.unwrap_or_else(||format!("main 含 {} 笔未获批准的受保护变更，须先处理禁止中断", inspection.blocked_commits.len())));
    }
    Ok(catalog)
}
fn alert(server: &Server, runtime: &mut ProjectRuntime, id: &str, task: &str, error: &str) -> Result<(), String> {
    let object = format!("gate-{id}");
    if runtime.projection()["Alert"].get(&object).is_some() { return Ok(()); }
    accepted(runtime, NewEvent {
        eid:Uuid::now_v7().to_string(),host:server.endpoint.host.clone(),actor:"process:towermd".into(),
        kind:"alert.raised".into(),layer:"L5".into(),obj:object,
        data:json!({"reason":"task_gate_failed","detail":{"task":task,"evidence_id":id,"severity":"blocking","error":error}}),
        guards:vec![],
    })?;
    Ok(())
}
fn interrupt(server: &Server, runtime: &mut ProjectRuntime, id: &str, reason: &str) -> Result<(), String> {
    let request = stored_request(runtime, id)?;
    let entry = &runtime.projection()["Evidence"][id];
    if !matches!(entry["status"].as_str(), Some("queued" | "running")) { return Ok(()); }
    let mut data = json!({"request_eid":entry["request_eid"],"error":reason});
    // 12.1 禁止: a G2 stop holds the task for a person, not only the attempt.
    if reason.starts_with(task_finish::G2_BLOCKED) { data["guard"] = json!(task_finish::G2); }
    accepted(runtime, event(server, &request.interrupted_eid, "task.finish.interrupted", id, data, None))?;
    alert(server, runtime, id, &request.run.task, reason)
}

pub async fn check(
    State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Context>,
) -> Result<Json<Value>, ApiError> {
    tasks::require_cli(&headers, &server)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let run = tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),true).map_err(bad)?.ok_or_else(||bad("没有活动运行"))?;
        unheld(runtime,&run.binding.id).map_err(bad)?;
        let catalog = inspect_authority(&server,&project,runtime).map_err(bad)?;
        let baseline = tasks::baseline(&project,runtime).map_err(bad)?;
        let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
        let leases = runtime.work_leases(now()).into_values().collect::<Vec<_>>();
        let frozen = task_finish::prepare(&catalog,&baseline,&projection,&run.binding.id,"check",&leases,now(),Limits::default()).map_err(|error|{
            let captured=super::proposals::capture_run_violations(&server,&project,runtime,&catalog,&run);
            rejected_as(ErrorCode::GuardBlocked)(match captured {Ok(ids) if !ids.is_empty()=>format!("{error}；原始越界差异已保存为草稿提案：{}",ids.join(", ")),Err(capture)=>format!("{error}；提案捕获待重试：{capture}"),_=>error})
        })?;
        Ok(Json(json!({"passed":true,"base":frozen.main_base,"head":frozen.revision,"tree":frozen.tree})))
    }).await.map_err(uncertain)?
}

pub async fn start(
    State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Action>,
) -> Result<Json<Value>, ApiError> {
    tasks::require_cli(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    if server.shutting_down.load(Ordering::SeqCst) { return Err(bad("守护进程正在停止")); }
    let project = project(&server,&handle).map_err(bad)?;
    let task_server = Arc::clone(&server); let task_project = Arc::clone(&project);
    let (value, launch) = tokio::task::spawn_blocking(move || {
        let mut slot = task_project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime = ready(&mut slot,&task_server,&task_project).map_err(open_failure)?;
        replay_pending(runtime).map_err(uncertain)?;
        let id = format!("E-{}",input.request_id);
        if let Some(original) = runtime.receipt(&input.request_id) {
            if original.kind != "task.finish.requested" || original.obj != id { return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他动作")); }
            let request = stored_request(runtime,&id).map_err(bad)?;
            let (worktree,branch) = tasks::coordinates(&task_project,&input.cwd).map_err(bad)?;
            if request.run.binding.host != task_server.endpoint.host
                || input.run.as_ref().is_some_and(|run|run != &request.run.binding.id)
                || input.task.as_ref().is_some_and(|task|task != &request.run.task)
                || (input.run.is_none() && (worktree != request.run.binding.worktree || branch != request.run.binding.branch))
            { return Err(rejected_as(ErrorCode::Conflict)("原 finish 请求属于另一运行")); }
            if request.claims != input.claims || request.report != input.report { return Err(rejected_as(ErrorCode::Conflict)("同一 finish 请求的自述不同")); }
            return Ok((job_value(runtime,&id).map_err(bad)?,None));
        }
        let run = tasks::renew(runtime,&task_server,&task_project,&input.cwd,input.run.as_deref(),true).map_err(bad)?.ok_or_else(||bad("没有活动运行"))?;
        unheld(runtime,&run.binding.id).map_err(bad)?;
        if input.task.as_ref().is_some_and(|task|task != &run.task) { return Err(rejected_as(ErrorCode::Conflict)("任务与当前运行不一致")); }
        let criteria = dev_state::tasks(runtime.projection()).map_err(bad)?.tasks.get(&run.task).map_or(0,|task|task.input.acceptance.len());
        if input.claims.iter().any(|claim|claim.criterion >= criteria || claim.note.trim().is_empty()) {
            return Err(bad(format!("自述须指向已有完成标准（序号从 0 起，共 {criteria} 条）且说明非空")));
        }
        if let Some(report) = &input.report { report.validate().map_err(|error| bad(format!("运行报告无效：{error}")))?; }
        if let Some(id) = active_request(runtime,&run.binding.id) { return Ok((job_value(runtime,&id).map_err(bad)?,None)); }
        // FR-122: a failed declared gate refuses finish before any work.
        let catalog = current_catalog(&task_project).map_err(bad)?;
        gates::require_clear(&gates::enforce(&task_server,runtime,&catalog,&run.task,"finish",false).map_err(uncertain)?)?;
        // G1 (FR-58): the plan review of this attempt, when asked or required.
        task_reviews::g1(runtime.projection(),&run).map_err(rejected_as(ErrorCode::GateFailed))?;
        let current = now();
        let deadline = deadline(runtime,&run.binding.id,current);
        if deadline <= current { return Err(bad("验证期限已到，请先释放旧运行再重新领取")); }
        let request = FinishRequest {
            evidence_id:id.clone(),run:run.clone(),requested_at_ms:deadline-task_finish::VERIFYING_TTL_MS,
            deadline_ms:deadline,started_eid:Uuid::now_v7().to_string(),completion_eid:Uuid::now_v7().to_string(),
            interrupted_eid:Uuid::now_v7().to_string(),claims:input.claims,report:input.report,
        };
        accepted(runtime,event(&task_server,&input.request_id,"task.finish.requested",&id,serde_json::to_value(&request).map_err(bad)?,Some(&run))).map_err(uncertain)?;
        let control = Arc::new(AtomicBool::new(false));
        task_project.controls.lock().map_err(|_|bad("验证控制不可用"))?.insert(id.clone(),Arc::clone(&control));
        task_project.finish_jobs.lock().map_err(|_|bad("验证状态不可用"))?.insert(id.clone(),FinishJob{status:"queued"});
        Ok::<_,ApiError>((job_value(runtime,&id).map_err(bad)?,Some((request,control))))
    }).await.map_err(uncertain)??;
    if let Some((request,control)) = launch {
        tokio::task::spawn_blocking(move || run_job(&server,&project,request,control));
    }
    Ok(Json(value))
}

/// Re-finish while awaiting merge must not reset the original deadline.
fn deadline(runtime:&ProjectRuntime, run:&str, current:u64)->u64 {
    latest(runtime,run).and_then(|id| {
        (runtime.projection()["Evidence"][&id]["status"] == "passed")
            .then(||stored_request(runtime,&id).ok().map(|request|request.deadline_ms)).flatten()
    }).unwrap_or(current.saturating_add(task_finish::VERIFYING_TTL_MS))
}

/// FR-32: the integrator asks to verify a dispatched run once its branch is
/// rebased onto the newest main. Same request as `dev finish`, signed by the
/// integrator; the caller has checked holds and declared gates.
pub(super) fn request_integration(server:&Server, project:&Project, runtime:&mut ProjectRuntime, run:&Run)
    -> Result<(FinishRequest, Arc<AtomicBool>), String> {
    unheld(runtime,&run.binding.id)?;
    if active_request(runtime,&run.binding.id).is_some() { return Err("运行已有验证请求".into()); }
    task_reviews::g1(runtime.projection(),run)?;
    let current=now();
    let deadline=deadline(runtime,&run.binding.id,current);
    if deadline<=current { return Err("验证期限已到".into()); }
    let id=format!("E-{}",Uuid::now_v7());
    let request=FinishRequest{
        evidence_id:id.clone(),run:run.clone(),requested_at_ms:deadline-task_finish::VERIFYING_TTL_MS,
        deadline_ms:deadline,started_eid:Uuid::now_v7().to_string(),completion_eid:Uuid::now_v7().to_string(),
        interrupted_eid:Uuid::now_v7().to_string(),claims:Vec::new(),report:None,
    };
    let mut input=event(server,&Uuid::now_v7().to_string(),"task.finish.requested",&id,json!(request),Some(run));
    input.actor=towermd_devkit::schedule::INTEGRATOR.into();
    accepted(runtime,input)?;
    let control=Arc::new(AtomicBool::new(false));
    project.controls.lock().map_err(|_|"验证控制不可用")?.insert(id.clone(),Arc::clone(&control));
    project.finish_jobs.lock().map_err(|_|"验证状态不可用")?.insert(id,FinishJob{status:"queued"});
    Ok((request,control))
}

pub(super) fn run_job(server:&Server, project:&Project, request:FinishRequest, control:Arc<AtomicBool>) {
    let result = (|| {
        let frozen = {
            let mut slot = project.runtime.lock().map_err(|_|"项目会话不可用")?;
            let runtime = ready(&mut slot,server,project).map_err(|e|e.to_string())?;
            unheld(runtime,&request.run.binding.id)?;
            let catalog = inspect_authority(server,project,runtime)?;
            let baseline = tasks::baseline(project,runtime)?;
            let projection = dev_state::tasks(runtime.projection())?;
            let leases = runtime.work_leases(now()).into_values().collect::<Vec<_>>();
            let mut input = task_finish::prepare(&catalog,&baseline,&projection,&request.run.binding.id,
                &request.evidence_id,&leases,now(),Limits::default()).map_err(|error|{
                    let captured=super::proposals::capture_run_violations(server,project,runtime,&catalog,&request.run);
                    match captured {Ok(ids) if !ids.is_empty()=>format!("{error}；原始越界差异已保存为草稿提案：{}",ids.join(", ")),Err(capture)=>format!("{error}；提案捕获待重试：{capture}"),_=>error}
                })?;
            input.requested_at_ms=request.requested_at_ms; input.deadline_ms=request.deadline_ms;
            if now() >= input.deadline_ms { return Err("验证开始前固定期限已过".into()); }
            accepted(runtime,event(server,&request.started_eid,"task.finish.started",&request.evidence_id,json!(input),Some(&request.run)))?;
            input
        };
        let lane = loop {
            if control.load(Ordering::SeqCst) || server.shutting_down.load(Ordering::SeqCst) || now() >= request.deadline_ms {
                control.store(true,Ordering::SeqCst); return Err("验证启动前已取消或达到固定期限".into());
            }
            match server.verification_lane.try_lock() {
                Ok(lane)=>break lane,
                Err(std::sync::TryLockError::WouldBlock)=>std::thread::sleep(Duration::from_millis(50)),
                Err(_)=>return Err("验证调度不可用".into()),
            }
        };
        project.finish_jobs.lock().map_err(|_|"验证状态不可用")?.insert(request.evidence_id.clone(),FinishJob{status:"running"});
        {
            let mut slot=project.runtime.lock().map_err(|_|"项目会话不可用")?;
            let runtime=ready(&mut slot,server,project).map_err(|e|e.to_string())?;
            unheld(runtime,&request.run.binding.id)?;
            let baseline=tasks::baseline(project,runtime)?;
            task_finish::validate_current(&frozen,&baseline,&dev_state::tasks(runtime.projection())?,
                &runtime.work_leases(now()).into_values().collect::<Vec<_>>(),now())?;
        }
        let report = crate::worker::run(&task_finish::verification_plan(&frozen,&project.root),&control,&server.runtime)?;
        drop(lane);
        let payload = task_finish::complete(frozen,report,&request.completion_eid,&project.common)?;
        let mut slot = project.runtime.lock().map_err(|_|"项目会话不可用")?;
        let runtime = ready(&mut slot,server,project).map_err(|e|e.to_string())?;
        accepted(runtime,event(server,&request.completion_eid,task_finish::FINISH_EVENT,&request.evidence_id,json!(payload),Some(&request.run)))?;
        if !payload.passed { alert(server,runtime,&request.evidence_id,&request.run.task,"真实模块验证未通过；进度未认证")?; }
        Ok::<_,String>(())
    })();
    if let Err(error) = result {
        if let Ok(mut slot) = project.runtime.lock() {
            if let Some(runtime) = slot.as_mut() {
                // Replay a durable completion first: an unknown receipt must
                // not be replaced with an invented interruption.
                if replay_pending(runtime).is_ok() {
                    if let Err(record_error)=interrupt(server,runtime,&request.evidence_id,&error) {
                        eprintln!("finish 中断待重试：{record_error}");
                    }
                }
            }
        }
    }
    if let Ok(mut jobs)=project.finish_jobs.lock() { jobs.remove(&request.evidence_id); }
    if let Ok(mut controls)=project.controls.lock() { controls.remove(&request.evidence_id); }
}

pub async fn get(
    State(server): State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,
) -> Result<Json<Value>,ApiError> {
    if !authorized_client(&headers,&server) { return Err(denied()); }
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        replay_pending(runtime).map_err(uncertain)?;
        Ok(Json(job_value(runtime,&id).map_err(bad)?))
    }).await.map_err(uncertain)?
}

pub async fn trailers(
    State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Action>,
) -> Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?; valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        replay_pending(runtime).map_err(uncertain)?;
        let run=tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),true).map_err(bad)?.ok_or_else(||bad("没有活动运行"))?;
        unheld(runtime,&run.binding.id).map_err(bad)?;
        if input.task.as_ref().is_some_and(|id|id != &run.task) { return Err(rejected_as(ErrorCode::Conflict)("任务与当前运行不一致")); }
        let catalog=inspect_authority(&server,&project,runtime).map_err(bad)?;
        let id=latest(runtime,&run.binding.id).ok_or_else(||rejected_as(ErrorCode::GateFailed)("尚未 finish"))?;
        let entry=&runtime.projection()["Evidence"][&id];
        if entry["status"] != "passed" { return Err(rejected_as(ErrorCode::GateFailed)("当前 finish 尚未通过真实 G3 验证")); }
        let finish=runtime.receipt(entry["completion_eid"].as_str().ok_or_else(||bad("完成原件缺失"))?).cloned().ok_or_else(||bad("完成原件缺失"))?;
        let fresh=runtime.receipt(&input.request_id).is_none();
        gates::require_clear(&gates::enforce(&server,runtime,&catalog,&run.task,"trailers",fresh).map_err(uncertain)?)?;
        // G4 (FR-58): the code review of the tested tree, before admission.
        let tested=task_finish::completion_from_event(&finish).map_err(bad)?.input.tree;
        task_reviews::g4(runtime.projection(),&run,&tested).map_err(rejected_as(ErrorCode::GateFailed))?;
        let baseline=tasks::baseline(&project,runtime).map_err(bad)?;
        let projection=dev_state::tasks(runtime.projection()).map_err(bad)?;
        let leases=runtime.work_leases(now()).into_values().collect::<Vec<_>>();
        let gate=if let Some(original)=runtime.receipt(&input.request_id).cloned() {
            let gate=task_finish::g5_from_event(&finish,&original).map_err(bad)?;
            let fresh=task_finish::prepare_g5(&catalog,&baseline,&projection,&finish,&input.request_id,&leases,now()).map_err(bad)?;
            if gate.main_base != fresh.main_base || gate.task_parent != fresh.task_parent || gate.tree != fresh.tree {
                return Err(bad("原 G5 请求已固定另一基底或 tree；重新执行 trailers 使用新请求 ID"));
            }
            original
        } else {
            let gate=task_finish::prepare_g5(&catalog,&baseline,&projection,&finish,&input.request_id,&leases,now()).map_err(bad)?;
            accepted(runtime,event(&server,&input.request_id,task_finish::G5_EVENT,&id,json!(gate),Some(&run))).map_err(uncertain)?
        };
        let text=task_finish::trailers(&project.root,&finish,&gate).map_err(bad)?;
        Ok(Json(json!({"evidence_id":id,"g5_eid":gate.eid,"trailers":text})))
    }).await.map_err(uncertain)?
}

/// True when every commit from `main` back to `passed` is a snapshot this
/// runtime's own writer put on main. Any other commit — an external push, a
/// person's commit, a look-alike snapshot — ends the walk and takes the
/// complete inspection path.
fn only_own_snapshots(runtime:&ProjectRuntime,passed:&str,main:&str)->bool{
    let mut commit=main;
    while commit!=passed {
        match runtime.own_snapshot_parent(commit) { Some(parent)=>commit=parent, None=>return false }
    }
    commit!=main
}

/// Called under the project's runtime lock by the existing daemon maintenance.
pub(super) const MAIN_INSPECTION_BLOCKED:&str="main 完整检查未通过，暂停合并认证";
pub(super) fn maintain_project(server:&Server,project:&Project,runtime:&mut ProjectRuntime)->Result<(),String> {
    replay_pending(runtime)?;
    let current=now();
    let entries=runtime.projection()["Evidence"].as_object().ok_or("Evidence 投影无效")?
        .iter().filter(|(_,entry)|entry["kind"]=="finish").map(|(id,entry)|(id.clone(),entry.clone())).collect::<Vec<_>>();
    for (id,entry) in &entries {
        let request:FinishRequest=serde_json::from_value(entry["request"].clone()).map_err(|e|e.to_string())?;
        if latest(runtime,&request.run.binding.id).as_deref()!=Some(id.as_str()) { continue; }
        let live=dev_state::tasks(runtime.projection())?.runs.get(&request.run.binding.id)
            .is_some_and(|run|run.state==RunState::Active);
        let job=project.finish_jobs.lock().map_err(|_|"验证状态不可用")?.get(id).cloned();
        if current<request.deadline_ms && live && unheld(runtime,&request.run.binding.id).is_err(){
            if let Some(control)=project.controls.lock().map_err(|_|"验证控制不可用")?.get(id){control.store(true,Ordering::SeqCst);}
            continue;
        }
        if current>=request.deadline_ms || !live {
            if let Some(control)=project.controls.lock().map_err(|_|"验证控制不可用")?.get(id) { control.store(true,Ordering::SeqCst); }
            if live && matches!(entry["status"].as_str(),Some("queued"|"running"|"passed")) {
                runtime.release_work_leases(request.run.fences.clone(),now).map_err(|e|e.to_string())?;
            }
            if job.is_none() { interrupt(server,runtime,id,"验证期限已到或原运行租约已失效")?; }
        } else if matches!(entry["status"].as_str(),Some("queued"|"running")) {
            match job {
                Some(job) if matches!(job.status,"queued"|"running")=>{
                    runtime.renew_work_leases(request.run.fences.clone(),tasks::MODULE_TTL,now).map_err(|e|e.to_string())?;
                }
                _=>interrupt(server,runtime,id,"守护进程在验证完成前退出；保留冻结输入，结果未知，请重新 finish")?,
            }
        }
    }
    // Complete inspection remains useful even when there are no active tasks.
    let catalog=refresh_authority(project)?;
    let inspected=&runtime.projection()["MainInspection"];
    let authority_ok=if inspected["main"]==catalog.revision && inspected["complete"]==true {
        inspected["passed"]==true
    }else if inspected["complete"]==true && inspected["passed"]==true
        && inspected["main"].as_str().is_some_and(|passed|only_own_snapshots(runtime,passed,&catalog.revision)) {
        // Main moved only by this daemon's own snapshot commits on top of a
        // passed complete inspection; their receipts already proved them.
        true
    }else{
        let (_,inspection)=inspect_catalog(server,project,runtime,current_catalog(project)?)?;
        inspection.complete && inspection.passed
    };
    if !authority_ok{return Err(MAIN_INSPECTION_BLOCKED.into());}
    if project.finish_observed_main.lock().map_err(|_|"主干观察状态不可用")?.as_deref()==Some(catalog.revision.as_str()){
        return Ok(());
    }
    let candidates=entries.into_iter().filter(|(id,entry)|entry["status"]=="passed"
        && entry["g5_eid"].is_string()
        && entry["run"].as_str().is_some_and(|run|latest(runtime,run).as_deref()==Some(id.as_str()))).collect::<Vec<_>>();
    if candidates.is_empty(){
        *project.finish_observed_main.lock().map_err(|_|"主干观察状态不可用")?=Some(catalog.revision);
        return Ok(());
    }
    let main=repo::git(&project.root,&["--no-replace-objects","rev-list","--first-parent",&catalog.revision])?;
    let baseline=tasks::baseline(project,runtime)?;
    for (id,entry) in candidates {
        let finish=runtime.receipt(entry["completion_eid"].as_str().ok_or("完成原件缺失")?).cloned().ok_or("完成原件缺失")?;
        let gate=runtime.receipt(entry["g5_eid"].as_str().ok_or("G5 原件缺失")?).cloned().ok_or("G5 原件缺失")?;
        let evidence=task_finish::completion_from_event(&finish)?;
        if unheld(runtime,&evidence.input.run.binding.id).is_err(){continue;}
        let projection=dev_state::tasks(runtime.projection())?;
        let leases=runtime.work_leases(now()).into_values().collect::<Vec<_>>();
        if task_finish::validate_current(&evidence.input,&baseline,&projection,&leases,now()).is_err() {continue;}
        for admission in main.lines() {
            let message=repo::git(&project.root,&["--no-replace-objects","show","-s","--format=%B",admission])?;
            if !message.lines().any(|line|line==format!("Towermd-Evidence: {id}")){continue;}
            match task_finish::prove_merge(&project.root,&catalog.revision,admission,&finish,&gate) {
                Ok(proof)=>{
                    let eid=derived_eid(admission,"task.merged")?;
                    accepted(runtime,event(server,&eid,"task.merged",&proof.task_id,json!(proof),Some(&evidence.input.run)))?;
                    runtime.release_work_leases(evidence.input.run.fences.clone(),now).map_err(|e|e.to_string())?;
                    break;
                }
                Err(error)=>alert(server,runtime,&id,&evidence.input.run.task,&error)?,
            }
        }
    }
    *project.finish_observed_main.lock().map_err(|_|"主干观察状态不可用")?=Some(catalog.revision);
    Ok(())
}

/// Recovery reads original accepted events and actual main history only.
/// Pending requests, local job files and arbitrary report JSON are excluded.
/// The runtime attaches its new recovery main fence after this proof returns.
pub(super) fn recover_merges(evidence:&towermd_core::project_runtime::RecoveryEvidence<'_>)->Result<Vec<NewEvent>,String>{
    let originals=evidence.snapshot.events.iter().chain(evidence.confirmed).cloned().collect::<Vec<_>>();
    let canonical=towermd_core::project_runtime::canonical_events(&originals)?;
    let catalog=Catalog::load(evidence.root)?;
    // Establish the same complete import boundary. Actual protected-publication
    // facts are verified by the caller's recovery/inspection pass.
    towermd_core::path_guard::inspect_main(evidence.root,&catalog.revision,catalog.config.project_id(),
        catalog.config.docs_dir(),None)?;
    let main=repo::git(evidence.root,&["--no-replace-objects","rev-list","--first-parent",&catalog.revision])?;
    let mut output=Vec::new();
    for admission in main.lines(){
        let eid=derived_eid(admission,"task.merged")?;
        if canonical.contains_key(&eid){continue;}
        let message=repo::git(evidence.root,&["--no-replace-objects","show","-s","--format=%B",admission])?;
        let Some(id)=message.lines().find_map(|line|line.strip_prefix("Towermd-Evidence: ")) else{continue;};
        let finishes=canonical.values().filter(|event|event.kind==task_finish::FINISH_EVENT && event.obj==id && !event.stale);
        let mut proof_found=None;
        for finish in finishes{
            let mut gates=canonical.values().filter(|event|event.kind==task_finish::G5_EVENT
                && event.obj==id && !event.stale).collect::<Vec<_>>();
            gates.sort_by_key(|event|std::cmp::Reverse(event.seq));
            for gate in gates{
                if let Ok(proof)=task_finish::prove_merge(evidence.root,&catalog.revision,admission,finish,gate){
                    proof_found=Some((proof,finish.host.clone()));
                    break;
                }
            }
            if proof_found.is_some(){break;}
        }
        if let Some((proof,host))=proof_found{
            output.push(NewEvent{
                eid,host,actor:"process:towermd-recovery".into(),kind:"task.merged".into(),
                layer:"L4".into(),obj:proof.task_id.clone(),data:json!(proof),guards:vec![],
            });
        }
    }
    Ok(output)
}
