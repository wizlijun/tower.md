//! M3-B scheduler and integrator. The scheduler claims module leases, then
//! hands a queued task to a delegate through the runner (FR-14, FR-31); the
//! integrator rebases its branch onto the newest `main`, has it verified and
//! fast-forwards `main` through the single writer, one run at a time (FR-11,
//! FR-32). Only the host holding `main` does either. What this daemon keeps
//! here is disposable: live harness controls and the last reason a step waits.
use super::business::{now, ready, uncertain, open_failure, runtime_failure};
use super::*;
use std::collections::BTreeSet;
use tokio::sync::watch;
use towermd_core::{hub::Event, hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}};
use towermd_devkit::{
    budget::{self, Budget}, dev_state, gateway, harness, run_record, runner,
    run_report::{self, Outcome, ReportStatus, RunFinished, Stop, Tokens},
    schedule::{self, Settings, Stage, Step, INTEGRATOR, SCHEDULER},
    skillpacks::{self, Role}, task_finish, task_model::{self, Dispatch, ReleaseReason, Run, RunBinding}, task_reviews,
};

/// A blocked gate is recorded once per cause; the cause is re-judged after this.
const GATE_RECHECK_MS: u64 = 10 * 60 * 1000;
const GIT_STEP: Duration = Duration::from_secs(120);
const INTEGRATOR_NAME: &str = "Tower.md integrator";
const INTEGRATOR_EMAIL: &str = "towermd@localhost";

#[derive(Default)]
pub(super) struct Scheduler {
    /// Runs whose harness this daemon runs now; `true` stops one.
    live: Mutex<BTreeMap<String, watch::Sender<bool>>>,
    /// Why a queued task or a run is not moving, shown by `dev queue`.
    waiting: Mutex<BTreeMap<String, String>>,
    /// `run:stage` → the cause a gate last blocked on, and when.
    gates: Mutex<BTreeMap<String, (String, u64)>>,
}

/// Run a pass soon, off the caller's thread: after a request, a harness or a
/// verification ends. The periodic maintenance runs one every ten seconds.
pub(super) fn kick(handle: tokio::runtime::Handle, server: Arc<Server>, project: Arc<Project>) {
    let spawner = handle.clone();
    spawner.spawn_blocking(move || {
        let Ok(mut slot) = project.runtime.lock() else { return };
        let Ok(runtime) = ready(&mut slot, &server, &project) else { return };
        if runtime.tick(now).is_err() || !runtime.holds_main() { return; }
        if let Err(error) = pass(&server, &project, runtime) { eprintln!("调度待重试：{error}"); }
    });
}

/// One scheduling pass, under the project's runtime lock, on the main holder.
pub(super) fn pass(server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime) -> Result<(), String> {
    if server.shutting_down.load(Ordering::SeqCst) { return Ok(()); }
    let handle = tokio::runtime::Handle::try_current().map_err(|_| "调度须在守护进程运行时中执行")?;
    let catalog = current_catalog(project)?;
    let settings = Settings::from_config(catalog.config.section("runner"));
    let live = project.scheduler.live.lock().map_err(|_| "调度状态不可用")?.keys().cloned().collect::<BTreeSet<_>>();
    for step in schedule::plan(runtime.projection(), &server.endpoint.host, &live, &settings)? {
        let key = match &step {
            Step::Dispatch { task, .. } | Step::Dequeue { task, .. } => task.clone(),
            Step::Stop { run } | Step::Release { run, .. } | Step::Renew { run } | Step::Orphaned { run }
            | Step::Launch { run, .. } | Step::Integrate { run, .. } | Step::RejectVerify { run, .. }
            | Step::GiveUp { run, .. } => run.clone(),
        };
        let result = execute(server, project, runtime, &handle, &catalog, &settings, step);
        let mut waiting = project.scheduler.waiting.lock().map_err(|_| "调度状态不可用")?;
        match result {
            Ok(()) => { waiting.remove(&key); }
            Err(error) => { waiting.insert(key, error); }
        }
    }
    super::controllers::pass(server, project, runtime, &handle, &catalog, &live)
}

fn execute(
    server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime, handle: &tokio::runtime::Handle,
    catalog: &Catalog, settings: &Settings, step: Step,
) -> Result<(), String> {
    match step {
        Step::Stop { run } => {
            if let Some(control) = project.scheduler.live.lock().map_err(|_| "调度状态不可用")?.get(&run) {
                let _ = control.send(true);
            }
            Ok(())
        }
        Step::Release { run, reason } => release(server, runtime, &run, reason),
        Step::Renew { run } => {
            let fences = active(runtime, &run)?.fences;
            runtime.renew_work_leases(fences, super::tasks::MODULE_TTL, now).map(|_| ()).map_err(|e| e.to_string())
        }
        Step::Orphaned { run } => orphaned(runtime, &run, Outcome::Interrupted, "守护进程重启时运行进程已失联；结果未知"),
        Step::Launch { run, note } => launch(server, project, runtime, handle, catalog, settings, &run, note),
        Step::Integrate { run, evidence_id } => integrate(server, project, runtime, handle, &run, evidence_id.as_deref()),
        Step::RejectVerify { run, evidence_id, reason } => {
            let run = active(runtime, &run)?;
            reject(server, runtime, &run, Stage::Verify, &catalog.revision, Some(&evidence_id), &reason)
        }
        Step::GiveUp { run, task, finish_eid, status } => {
            if runtime.projection()["Alert"].get(format!("dispatch-{finish_eid}")).is_none() {
                append(runtime, schedule::stop_alert(&server.endpoint.host, &task, Some(&run), &finish_eid,
                    &format!("委托报告「{}」，没有完成任务", schedule::status_name(status))))?;
            }
            dequeue(server, runtime, &task, "委托没有完成任务，等人决定")
        }
        Step::Dequeue { task, reason } => dequeue(server, runtime, &task, &reason),
        Step::Dispatch { task, harness, model } => dispatch(server, project, runtime, catalog, &task, harness, model),
    }
}

fn append(runtime: &mut ProjectRuntime, input: NewEvent) -> Result<Event, String> {
    let receipt = runtime.append(input, now).map_err(|e| e.to_string())?;
    if receipt.stale { return Err("事件的租约已失效，没有生效".into()); }
    Ok(receipt)
}
fn active(runtime: &ProjectRuntime, run: &str) -> Result<Run, String> {
    dev_state::tasks(runtime.projection())?.runs.get(run).cloned().ok_or_else(|| "运行不存在".into())
}

fn dequeue(server: &Server, runtime: &mut ProjectRuntime, task: &str, reason: &str) -> Result<(), String> {
    if !schedule::queue(runtime.projection())?.contains_key(task) { return Ok(()); }
    append(runtime, NewEvent {
        eid: Uuid::now_v7().to_string(), host: server.endpoint.host.clone(), actor: SCHEDULER.into(),
        kind: schedule::DEQUEUED.into(), layer: "L5".into(), obj: task.into(),
        data: json!({"task": task, "reason": reason}), guards: Vec::new(),
    })?;
    Ok(())
}

fn release(server: &Server, runtime: &mut ProjectRuntime, id: &str, reason: ReleaseReason) -> Result<(), String> {
    let projection = dev_state::tasks(runtime.projection())?;
    let run = projection.runs.get(id).cloned().ok_or("运行不存在")?;
    let action = task_model::prepare_release(&projection, id, reason)?;
    append(runtime, NewEvent {
        eid: Uuid::now_v7().to_string(), host: server.endpoint.host.clone(), actor: SCHEDULER.into(),
        kind: "task.released".into(), layer: "L4".into(), obj: run.task.clone(),
        data: serde_json::to_value(action).map_err(|e| e.to_string())?, guards: run.fences.clone(),
    })?;
    match runtime.release_work_leases(run.fences, now) {
        Ok(_) | Err(RuntimeError::Fenced(_)) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// The runner could not record the end itself (lost process or failed sink).
fn orphaned(runtime: &mut ProjectRuntime, id: &str, outcome: Outcome, error: &str) -> Result<(), String> {
    let records = run_record::records(runtime.projection())?;
    let record = records.get(id).ok_or("运行没有开始记录")?;
    if record.running.is_none() { return Ok(()); }
    let host = runtime.projection()["Run"][id]["binding"]["host"].as_str().unwrap_or_default().to_owned();
    let mut finished = RunFinished {
        run: id.into(), task: record.task.clone(), agent: record.agent.clone(), outcome, status: ReportStatus::Failed,
        stop: (outcome == Outcome::Interrupted).then_some(Stop::Interrupted), exit_code: None, signal: None,
        crashed: false, report: None, summary: run_report::clip(error), error_class: None, error: Some(error.into()),
        // Nothing reported a cost: unknown, never 0.
        cost_usd: None, tokens: Tokens::default(), session: None, attempts: 1, raw: None,
    };
    finished.classify(false);
    append(runtime, NewEvent {
        eid: Uuid::now_v7().to_string(), host, actor: run_report::RUNNER.into(), kind: run_report::FINISHED.into(),
        layer: "L5".into(), obj: record.task.clone(), data: json!(finished), guards: Vec::new(),
    })?;
    Ok(())
}

/// The runner's events go to the hub through this daemon's runtime.
struct Sink {
    server: Arc<Server>,
    project: Arc<Project>,
    started: bool,
}
impl runner::RunSink for Sink {
    fn emit(&mut self, event: NewEvent) -> Result<(), String> {
        let mut slot = self.project.runtime.lock().map_err(|_| "项目会话不可用")?;
        let runtime = ready(&mut slot, &self.server, &self.project).map_err(|e| e.to_string())?;
        let started = event.kind == run_report::STARTED;
        runtime.append(event, now).map_err(|e| e.to_string())?;
        self.started |= started;
        Ok(())
    }
}

/// `TOWERMD_HARNESS_<ID>` points an adapter at another program (a mock in
/// tests, a wrapper locally); otherwise the adapter's own program on PATH.
pub(super) fn program(adapter: &dyn harness::Adapter) -> Option<PathBuf> {
    std::env::var_os(format!("TOWERMD_HARNESS_{}", adapter.id().to_ascii_uppercase().replace('-', "_"))).map(PathBuf::from)
}

#[allow(clippy::too_many_arguments)]
fn launch(
    server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime, handle: &tokio::runtime::Handle,
    catalog: &Catalog, settings: &Settings, id: &str, note: Option<String>,
) -> Result<(), String> {
    let projection = dev_state::tasks(runtime.projection())?;
    let run = projection.runs.get(id).cloned().ok_or("运行不存在")?;
    let task = projection.tasks.get(&run.task).cloned().ok_or("任务不存在")?;
    let dispatch = run.dispatch.clone().ok_or("运行不是调度器派发的")?;
    super::finish::unheld(runtime, id)?;
    let Some(adapter) = harness::find(&dispatch.harness) else {
        return stop(server, runtime, &run.task, Some(id), &format!("{}-harness", run.task), &format!("未知 harness：{}", dispatch.harness));
    };
    // 12.1: a budget refusal stops dispatching; a person decides.
    let limits = Budget::from_config(catalog.config.section("budget"));
    let spend = budget::module_spend(runtime.projection(), &run.module)?;
    let budget_usd = match budget::admit(limits, spend, adapter.capabilities().reports_cost) {
        Ok(limit) => limit,
        Err(refusal) => {
            append(runtime, budget::refusal_event(&server.endpoint.host, &run.task, &run.module, refusal, limits, spend))?;
            return dequeue(server, runtime, &run.task, refusal.reason());
        }
    };
    let starts = run_record::records(runtime.projection())?.get(id).map_or(0, |record| record.starts);
    let mut config = runner::RunConfig {
        host: server.endpoint.host.clone(), run: id.into(), task: run.task.clone(), module: run.module.clone(),
        worktree: PathBuf::from(&run.binding.worktree),
        run_dir: runner::run_dir(&project.common, id).join((starts + 1).to_string()),
        task_card: schedule::card(&task, note.as_deref()), model: dispatch.model.clone(),
        permission: harness::Permission::WorkspaceWrite, budget_usd, timeout: settings.timeout, grace: runner::GRACE,
        max_attempts: settings.max_attempts, mcp_config: None, permission_tool: None, skill_dirs: Vec::new(), env: Vec::new(),
    };
    // FR-51–54, FR-60: the executor's locked skills from the approved
    // revision and the interaction gateway, both in this attempt's run
    // directory and passed explicitly. A broken lock never runs.
    let key = format!("{id}-{}", starts + 1);
    let loaded = match skillpacks::wire(&mut config, adapter, &project.root, &catalog.revision, Role::Executor) {
        Ok(loaded) => loaded,
        Err(error) => return stop(server, runtime, &run.task, Some(id), &key, &error),
    };
    let towermd = std::env::current_exe().map_err(|e| format!("找不到 towermd 程序：{e}"))?;
    if let Err(error) = gateway::wire(&mut config, adapter, &towermd) {
        return stop(server, runtime, &run.task, Some(id), &key, &error);
    }
    if !loaded.is_empty() {
        append(runtime, NewEvent {
            eid: Uuid::now_v7().to_string(), host: server.endpoint.host.clone(), actor: SCHEDULER.into(),
            kind: skillpacks::LOADED.into(), layer: "L5".into(), obj: id.into(),
            data: json!({"task": run.task, "run": id, "attempt": starts + 1, "role": Role::Executor.as_str(),
                "revision": catalog.revision, "packs": loaded}),
            guards: Vec::new(),
        })?;
    }
    let (control, receiver) = watch::channel(false);
    project.scheduler.live.lock().map_err(|_| "调度状态不可用")?.insert(id.into(), control);
    let (server, project, handle, id, task) = (Arc::clone(server), Arc::clone(project), handle.clone(), id.to_owned(), run.task);
    let program = program(adapter);
    std::thread::spawn(move || {
        let mut sink = Sink { server: Arc::clone(&server), project: Arc::clone(&project), started: false };
        let result = handle.block_on(async {
            let probed = runner::probe(adapter, program).await?;
            runner::run(&probed, &config, &mut sink, receiver).await
        });
        if let Err(error) = result {
            if let Ok(mut slot) = project.runtime.lock() {
                if let Ok(runtime) = ready(&mut slot, &server, &project) {
                    let recorded = if sink.started {
                        // Started but the end was not recorded: a counted failure.
                        orphaned(runtime, &id, Outcome::Failed, &error)
                    } else {
                        stop(&server, runtime, &task, Some(&id), &key, &error)
                    };
                    if let Err(record) = recorded { eprintln!("运行结束待记录：{record}"); }
                }
            }
        }
        if let Ok(mut live) = project.scheduler.live.lock() { live.remove(&id); }
        kick(handle, server, project);
    });
    Ok(())
}

/// The scheduler cannot go on with this task: one alert, then off the queue.
fn stop(server: &Server, runtime: &mut ProjectRuntime, task: &str, run: Option<&str>, key: &str, error: &str) -> Result<(), String> {
    if runtime.projection()["Alert"].get(format!("dispatch-{key}")).is_none() {
        append(runtime, schedule::stop_alert(&server.endpoint.host, task, run, key, error))?;
    }
    dequeue(server, runtime, task, error)
}

fn dispatch(
    server: &Server, project: &Project, runtime: &mut ProjectRuntime, catalog: &Catalog,
    id: &str, harness: String, model: Option<String>,
) -> Result<(), String> {
    if harness::find(&harness).is_none() {
        return stop(server, runtime, id, None, &format!("{id}-harness"), &format!("未知 harness：{harness}"));
    }
    let approved = super::tasks::baseline(project, runtime)?;
    let projection = dev_state::tasks(runtime.projection())?;
    if towermd_core::question_projection::blocks_task(runtime.projection(), id)? {
        return Err("任务仍有未解决提问".into());
    }
    if let Err(error) = task_model::check_g0(&approved, &projection, id) {
        super::tasks::ask_creator(runtime, server, SCHEDULER, &approved, &projection, id, &error).map_err(|e| e.to_string())?;
        return Err(error);
    }
    let branch = format!("t/{id}");
    let path = project.common.join("towermd").join("worktrees").join(id);
    worktree(project, &path, &branch, &catalog.revision)?;
    let (worktree, current) = super::tasks::coordinates(project, &path)?;
    if current != branch { return Err(format!("派发工作树不在 {branch} 分支上")); }
    let binding = RunBinding { id: format!("R-{}", Uuid::now_v7()), host: server.endpoint.host.clone(), worktree, branch, self_reported: None };
    let task = projection.tasks.get(id).ok_or("任务不存在")?;
    let names = std::iter::once(&task.input.module).chain(&task.input.touches).map(|module| format!("module:{module}")).collect();
    let leases = runtime.acquire_work_leases(&binding.id, names, super::tasks::MODULE_TTL, now).map_err(|error| match error {
        RuntimeError::Fenced(_) => format!("模块租约被占用：{error}"),
        other => other.to_string(),
    })?;
    let fences = leases.iter().map(|lease| lease.fence()).collect::<Vec<_>>();
    let action = task_model::prepare_dispatch(&approved, &projection, id, binding, &leases, now(), Dispatch { harness, model });
    let result = action.and_then(|action| {
        append(runtime, NewEvent {
            eid: Uuid::now_v7().to_string(), host: server.endpoint.host.clone(), actor: SCHEDULER.into(),
            kind: "task.claimed".into(), layer: "L4".into(), obj: id.into(),
            data: serde_json::to_value(action).map_err(|e| e.to_string())?, guards: fences.clone(),
        })
    });
    if let Err(error) = result {
        let _ = runtime.release_work_leases(fences, now);
        return Err(error);
    }
    Ok(())
}

/// `<git-common-dir>/towermd/worktrees/<task>` on `t/<task>`: outside the
/// user's tree, reused by the next run of the same task.
fn worktree(project: &Project, path: &Path, branch: &str, main: &str) -> Result<(), String> {
    if path.join(".git").exists() {
        return Ok(());
    }
    repo::git(&project.root, &["worktree", "prune"])?;
    let target = path.to_str().ok_or("工作树路径不是 UTF-8")?;
    let exists = repo::git(&project.root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok();
    if exists {
        repo::git_timeout(&project.root, &["worktree", "add", target, branch], GIT_STEP)?;
    } else {
        repo::git_timeout(&project.root, &["worktree", "add", "-b", branch, target, main], GIT_STEP)?;
    }
    Ok(())
}

fn git_as_integrator(dir: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut all = vec!["-c", "user.name=Tower.md integrator", "-c", "user.email=towermd@localhost",
        "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"];
    all.extend_from_slice(args);
    repo::git_timeout_env_input_output(dir, &all, GIT_STEP, &[], None)
}
fn tail(output: &std::process::Output) -> String {
    run_report::clip(String::from_utf8_lossy(&output.stderr).trim())
}

/// `merge.rejected`; the same refusal of the same attempt is recorded once.
fn reject(
    server: &Server, runtime: &mut ProjectRuntime, run: &Run, stage: Stage, base: &str,
    evidence_id: Option<&str>, reason: &str,
) -> Result<(), String> {
    let previous = schedule::merges(runtime.projection())?.remove(&run.binding.id);
    if previous.is_some_and(|mark| mark.status == "rejected" && mark.stage == Some(stage) && mark.base == base
        && mark.evidence_id.as_deref() == evidence_id && mark.reason.as_deref() == Some(reason)) {
        return Err(reason.into());
    }
    append(runtime, NewEvent {
        eid: Uuid::now_v7().to_string(), host: server.endpoint.host.clone(), actor: INTEGRATOR.into(),
        kind: schedule::MERGE_REJECTED.into(), layer: "L5".into(), obj: run.task.clone(),
        data: json!({"task": run.task, "run": run.binding.id, "evidence_id": evidence_id, "base": base,
            "stage": stage, "reason": reason}),
        guards: vec![runtime.main_fence().map_err(|e| e.to_string())?],
    })?;
    Ok(())
}

/// A declared gate that blocks is recorded as evidence once per cause.
fn gated(server: &Server, project: &Project, runtime: &mut ProjectRuntime, catalog: &Catalog, run: &Run, stage: &str) -> Result<(), String> {
    let key = format!("{}:{stage}", run.binding.id);
    let cause = format!("{}\n{}", catalog.revision, serde_json::to_string(&super::gates::in_force(runtime.projection())?).map_err(|e| e.to_string())?);
    let current = now();
    if let Some((last, at)) = project.scheduler.gates.lock().map_err(|_| "调度状态不可用")?.get(&key) {
        if last == &cause && current.saturating_sub(*at) < GATE_RECHECK_MS { return Err("声明的门禁仍未通过".into()); }
    }
    let judgement = super::gates::enforce(server, runtime, catalog, &run.task, stage, false)?;
    let mut memo = project.scheduler.gates.lock().map_err(|_| "调度状态不可用")?;
    match judgement.declared_blocks() {
        Some(reason) => { memo.insert(key, (cause, current)); Err(reason) }
        None => { memo.remove(&key); Ok(()) }
    }
}

/// FR-11/FR-32: rebase onto the newest main, verify that exact head, then
/// fast-forward. One run at a time: the pass holds the runtime lock.
fn integrate(
    server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime, handle: &tokio::runtime::Handle,
    id: &str, evidence_id: Option<&str>,
) -> Result<(), String> {
    let run = active(runtime, id)?;
    super::finish::unheld(runtime, id)?;
    let catalog = super::finish::inspect_authority(server, project, runtime)?;
    let main = catalog.revision.clone();
    let worktree = PathBuf::from(&run.binding.worktree);
    // The integrator merges commits only; it never commits for the delegate.
    let dirty = repo::git(&worktree, &["status", "--porcelain", "--untracked-files=no"])?;
    if !dirty.is_empty() {
        return reject(server, runtime, &run, Stage::Rebase, &main, None, "工作树有未提交的改动；集成器只合并已提交的内容");
    }
    let head = repo::git(&worktree, &["rev-parse", "HEAD"])?;
    let based = repo::git(&worktree, &["merge-base", "--is-ancestor", &main, &head]).is_ok();
    if !based {
        let output = git_as_integrator(&worktree, &["rebase", "--no-autostash", &main])?;
        if !output.status.success() {
            let _ = git_as_integrator(&worktree, &["rebase", "--abort"]);
            return reject(server, runtime, &run, Stage::Rebase, &main, None, &format!("变基到最新 main 有冲突：{}", tail(&output)));
        }
    }
    let head = repo::git(&worktree, &["rev-parse", "HEAD"])?;
    if head == main {
        return reject(server, runtime, &run, Stage::Rebase, &main, None, "任务分支没有可合并的提交");
    }
    if let Some(evidence) = evidence_id {
        match publish(server, project, runtime, &run, &catalog, evidence) {
            Ok(()) => return Ok(()),
            // The verified head is still current: nothing new to verify.
            Err((tested, error)) if tested == head => return Err(error),
            Err(_) => {}
        }
    }
    gated(server, project, runtime, &catalog, &run, "finish")?;
    let (request, control) = super::finish::request_integration(server, project, runtime, &run)?;
    let (server, project, handle) = (Arc::clone(server), Arc::clone(project), handle.clone());
    std::thread::spawn(move || {
        super::finish::run_job(&server, &project, request, control);
        kick(handle, server, project);
    });
    Ok(())
}

/// G4, G5 and the admission commit, then `main` moves through the single
/// writer. `Err((tested head, reason))` when it did not publish.
fn publish(
    server: &Server, project: &Project, runtime: &mut ProjectRuntime, run: &Run, catalog: &Catalog, id: &str,
) -> Result<(), (String, String)> {
    let entry = runtime.projection()["Evidence"][id].clone();
    let finish = entry["completion_eid"].as_str().and_then(|eid| runtime.receipt(eid)).cloned()
        .ok_or_else(|| (String::new(), "完成原件缺失".to_owned()))?;
    let evidence = task_finish::completion_from_event(&finish).map_err(|e| (String::new(), e))?;
    let tested = evidence.input.revision.clone();
    let stay = |error: String| (tested.clone(), error);
    gated(server, project, runtime, catalog, run, "trailers").map_err(stay)?;
    let file = super::finish::main_gates(catalog).map_err(stay)?;
    task_reviews::g4(runtime.projection(), run, &file, &evidence.input.tree).map_err(stay)?;
    let baseline = super::tasks::baseline(project, runtime).map_err(stay)?;
    let projection = dev_state::tasks(runtime.projection()).map_err(stay)?;
    let leases = runtime.work_leases(now()).into_values().collect::<Vec<_>>();
    let g5_eid = Uuid::now_v7().to_string();
    // A rebase onto a state-only move keeps the tested code: same manifest.
    let fresh = task_finish::prepare_g5(catalog, &baseline, &projection, &finish, &g5_eid, &leases, now()).map_err(stay)?;
    let reuse = entry["g5_eid"].as_str().and_then(|eid| runtime.receipt(eid)).cloned().filter(|original| {
        task_finish::g5_from_event(&finish, original).is_ok_and(|gate| gate.main_base == fresh.main_base
            && gate.task_parent == fresh.task_parent && gate.tree == fresh.tree && gate.task_revision == fresh.task_revision)
    });
    let gate = match reuse {
        Some(original) => original,
        None => super::finish::accepted(runtime, super::finish::event(server, &g5_eid, task_finish::G5_EVENT, id, json!(fresh), Some(run))).map_err(stay)?,
    };
    let trailers = task_finish::trailers(&project.root, &finish, &gate).map_err(stay)?;
    let admission = admission(project, &fresh, &trailers).map_err(stay)?;
    if let Err(error) = runtime.advance(&catalog.revision, &admission, now) {
        return reject(server, runtime, run, Stage::Publish, &catalog.revision, Some(id), &format!("main 没有快进：{error}")).map_err(stay);
    }
    // The task branch shows what landed; the tree is the same, so its worktree stays clean.
    let _ = repo::git(&project.root, &["update-ref", &format!("refs/heads/{}", run.binding.branch), &admission, &fresh.task_revision]);
    append(runtime, NewEvent {
        eid: Uuid::now_v7().to_string(), host: server.endpoint.host.clone(), actor: INTEGRATOR.into(),
        kind: schedule::MERGE_DONE.into(), layer: "L5".into(), obj: run.task.clone(),
        data: json!({"task": run.task, "run": run.binding.id, "evidence_id": id, "base": catalog.revision, "commit": admission}),
        guards: vec![runtime.main_fence().map_err(|e| stay(e.to_string()))?],
    }).map_err(stay)?;
    // Certify at once from the moved main: task.merged and released leases.
    if let Ok(mut observed) = project.finish_observed_main.lock() { *observed = None; }
    super::finish::maintain_project(server, project, runtime).map_err(stay)?;
    Ok(())
}

/// The tested tree on the task tip's parent, ending in the exact trailers;
/// author and message from the delegate's tip, committed by the integrator.
fn admission(project: &Project, gate: &task_finish::G5Evidence, trailers: &str) -> Result<String, String> {
    let original = repo::git(&project.root, &["show", "-s", "--format=%B", &gate.task_revision])?;
    let body = original.lines().filter(|line| !line.starts_with("Towermd-")).collect::<Vec<_>>().join("\n");
    let body = body.trim_end();
    let message = format!("{}\n\n{trailers}", if body.is_empty() { "task" } else { body });
    let author = repo::git(&project.root, &["show", "-s", "--format=%an%x00%ae%x00%aI", &gate.task_revision])?;
    let mut fields = author.split('\0');
    let (name, email, date) = (fields.next().unwrap_or_default(), fields.next().unwrap_or_default(), fields.next().unwrap_or_default());
    let environment: [(&str, &std::ffi::OsStr); 5] = [
        ("GIT_AUTHOR_NAME", name.as_ref()), ("GIT_AUTHOR_EMAIL", email.as_ref()), ("GIT_AUTHOR_DATE", date.as_ref()),
        ("GIT_COMMITTER_NAME", INTEGRATOR_NAME.as_ref()), ("GIT_COMMITTER_EMAIL", INTEGRATOR_EMAIL.as_ref()),
    ];
    let output = repo::git_timeout_env_input_output(&project.root,
        &["-c", "commit.gpgsign=false", "commit-tree", &gate.tree, "-p", &gate.task_parent, "-F", "-"],
        GIT_STEP, &environment, Some(message.as_bytes()))?;
    if !output.status.success() { return Err(format!("无法生成接入提交：{}", tail(&output))); }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

// ---- HTTP: `dev run`, `dev queue`, `dev dequeue` ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enqueue {
    request_id: String,
    task: String,
    #[serde(default)]
    harness: Option<String>,
    #[serde(default)]
    model: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remove {
    request_id: String,
    #[serde(default)]
    reason: Option<String>,
}

fn snapshot(project: &Project, runtime: &ProjectRuntime) -> Result<Value, String> {
    let live = project.scheduler.live.lock().map_err(|_| "调度状态不可用")?.keys().cloned().collect::<Vec<_>>();
    let waiting = project.scheduler.waiting.lock().map_err(|_| "调度状态不可用")?.clone();
    let projection = dev_state::tasks(runtime.projection())?;
    let runs = projection.runs.values().filter(|run| run.dispatch.is_some() && run.state == task_model::RunState::Active)
        .map(|run| json!({"run": run.binding.id, "task": run.task, "dispatch": run.dispatch,
            "worktree": run.binding.worktree, "live": live.contains(&run.binding.id),
            "task_state": projection.tasks.get(&run.task).map(|task| task.state),
            "waiting": waiting.get(&run.binding.id)}))
        .collect::<Vec<_>>();
    let queue = schedule::queue(runtime.projection())?.into_values()
        .map(|queued| json!({"task": queued.task, "harness": queued.harness, "model": queued.model, "by": queued.by,
            "at": queued.at.at, "waiting": waiting.get(&queued.task)}))
        .collect::<Vec<_>>();
    Ok(json!({"queue": queue, "runs": runs, "merges": runtime.projection().get(schedule::MERGE).cloned().unwrap_or_else(|| json!({})),
        "archive": super::controllers::status(project, runtime)?}))
}

pub async fn enqueue(
    State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Enqueue>,
) -> Result<Json<Value>, ApiError> {
    super::tasks::require_cli(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    let (task_server, task_project) = (Arc::clone(&server), Arc::clone(&project));
    let value = tokio::task::spawn_blocking(move || {
        let mut slot = task_project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &task_server, &task_project).map_err(open_failure)?;
        if let Some(original) = runtime.receipt(&input.request_id) {
            if original.kind != schedule::QUEUED || original.obj != input.task {
                return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他动作"));
            }
        } else {
            let event = runtime.append(NewEvent {
                eid: input.request_id, host: task_server.endpoint.host.clone(), actor: "process:towermd-cli".into(),
                kind: schedule::QUEUED.into(), layer: "L5".into(), obj: input.task.clone(),
                data: json!({"task": input.task, "harness": input.harness, "model": input.model}), guards: Vec::new(),
            }, now).map_err(|error| match error {
                RuntimeError::Invalid(_) => rejected_as(ErrorCode::Conflict)(error),
                other => runtime_failure(other),
            })?;
            if event.stale { return Err(rejected_as(ErrorCode::StaleEpoch)("派发请求没有生效")); }
        }
        snapshot(&task_project, runtime).map_err(bad)
    }).await.map_err(uncertain)??;
    if let Ok(handle) = tokio::runtime::Handle::try_current() { kick(handle, server, project); }
    Ok(Json(value))
}

pub async fn list(
    State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) { return Err(denied()); }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        Ok(Json(snapshot(&project, runtime).map_err(bad)?))
    }).await.map_err(uncertain)?
}

pub async fn remove(
    State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, task)): RoutePath<(String, String)>,
    Json(input): Json<Remove>,
) -> Result<Json<Value>, ApiError> {
    super::tasks::require_cli(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    let (task_server, task_project) = (Arc::clone(&server), Arc::clone(&project));
    let value = tokio::task::spawn_blocking(move || {
        let mut slot = task_project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &task_server, &task_project).map_err(open_failure)?;
        if let Some(original) = runtime.receipt(&input.request_id) {
            if original.kind != schedule::DEQUEUED || original.obj != task {
                return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他动作"));
            }
        } else {
            let reason = input.reason.filter(|reason| !reason.trim().is_empty()).unwrap_or_else(|| "命令行取消派发".into());
            let event = runtime.append(NewEvent {
                eid: input.request_id, host: task_server.endpoint.host.clone(), actor: "process:towermd-cli".into(),
                kind: schedule::DEQUEUED.into(), layer: "L5".into(), obj: task.clone(),
                data: json!({"task": task, "reason": reason}), guards: Vec::new(),
            }, now).map_err(|error| match error {
                RuntimeError::Invalid(_) => rejected_as(ErrorCode::Conflict)(error),
                other => runtime_failure(other),
            })?;
            if event.stale { return Err(rejected_as(ErrorCode::StaleEpoch)("取消派发没有生效")); }
        }
        snapshot(&task_project, runtime).map_err(bad)
    }).await.map_err(uncertain)??;
    if let Ok(handle) = tokio::runtime::Handle::try_current() { kick(handle, server, project); }
    Ok(Json(value))
}

/// `dev adapters`: what the daemon would dispatch to, probed as the scheduler
/// probes (the same program override), with each adapter's capabilities.
pub async fn adapters(
    State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) { return Err(denied()); }
    project(&server, &handle).map_err(bad)?;
    let mut items = Vec::new();
    for adapter in harness::adapters() {
        let probed = runner::probe(adapter, program(adapter)).await;
        items.push(json!({
            "id": adapter.id(), "capabilities": adapter.capabilities(),
            "installed": probed.is_ok(),
            "version": probed.as_ref().ok().map(|harness| harness.version.clone()),
            "program": probed.as_ref().ok().map(|harness| harness.program.display().to_string()),
            "error": probed.err(),
        }));
    }
    Ok(Json(json!({"items": items})))
}
