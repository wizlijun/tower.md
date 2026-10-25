//! M3 scheduler and integrator (FR-11, FR-14, FR-30–32). The queue and the
//! merge marks are projections of accepted events, created lazily so the
//! v0.1.1 replay is unchanged; the daemon keeps only disposable process
//! handles. `plan` is the pure decision of one scheduling pass.
//!
//! Progress still only comes from evidence: a delegate's "done" report only
//! asks the integrator to verify; `task.merged` stays the daemon's proof.
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::dev_state::{tasks, timings, write_tasks, Stamp};
use crate::run_record;
use crate::run_report::{Outcome, ReportStatus};
use crate::task_model::{ReleaseReason, RunState, Task, TaskState};
use towermd_core::hub::Event;
use towermd_core::project_state::valid_oid;

/// The scheduler claims, launches and releases dispatched runs.
pub const SCHEDULER: &str = "process:towermd-scheduler";
/// The integrator verifies and fast-forwards `main`, one task at a time.
pub const INTEGRATOR: &str = "process:towermd-integrator";
const CLI: &str = "process:towermd-cli";

pub const QUEUED: &str = "run.queued";
pub const DEQUEUED: &str = "run.dequeued";
pub const MERGE_DONE: &str = "merge.done";
pub const MERGE_REJECTED: &str = "merge.rejected";
/// Lazily created groups.
pub const QUEUE: &str = "RunQueue";
pub const MERGE: &str = "Merge";
/// The gate name a rebase or verification rejection counts against (FR-57).
pub const MERGE_GATE: &str = "merge";

pub const DEFAULT_CONCURRENCY: usize = 4;
pub const DEFAULT_HARNESS: &str = "claude-code";
const DEFAULT_TIMEOUT_MINUTES: u64 = 60;
const DEFAULT_ATTEMPTS: u32 = 3;

/// Integrator stages a rejection can come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// The branch does not rebase cleanly onto `main`: the delegate redoes it.
    Rebase,
    /// Verification stopped without a result on the rebased branch.
    Verify,
    /// `main` refused the fast-forward (moved, or cannot be written).
    Publish,
}
impl Stage {
    /// Rebase and verify rejections are the delegate's to fix and count
    /// toward the auto-fix budget; a publish rejection is not its fault.
    fn counts(self) -> bool {
        matches!(self, Stage::Rebase | Stage::Verify)
    }
}

/// FR-30 role → harness and model, per module override.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Role {
    pub harness: String,
    pub model: Option<String>,
}

/// `runner:` in `config.yaml`. Missing or malformed values keep defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// FR-31: dispatched runs at once on this host.
    pub concurrency: usize,
    pub executor: Role,
    pub modules: BTreeMap<String, Role>,
    pub timeout: Duration,
    pub max_attempts: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            concurrency: DEFAULT_CONCURRENCY,
            executor: Role { harness: DEFAULT_HARNESS.into(), model: None },
            modules: BTreeMap::new(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_MINUTES * 60),
            max_attempts: DEFAULT_ATTEMPTS,
        }
    }
}

fn role(value: Option<&serde_yaml::Value>, fallback: &Role) -> Role {
    let text = |key: &str| value.and_then(|v| v.get(key)).and_then(serde_yaml::Value::as_str)
        .map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    Role { harness: text("harness").unwrap_or_else(|| fallback.harness.clone()), model: text("model").or_else(|| fallback.model.clone()) }
}

impl Settings {
    pub fn from_config(section: Option<&serde_yaml::Value>) -> Self {
        let defaults = Self::default();
        let positive = |key: &str| section.and_then(|s| s.get(key)).and_then(serde_yaml::Value::as_u64).filter(|v| *v > 0);
        let executor = role(section.and_then(|s| s.get("roles")).and_then(|r| r.get("executor")), &defaults.executor);
        let modules = section.and_then(|s| s.get("modules")).and_then(serde_yaml::Value::as_mapping).into_iter()
            .flat_map(|map| map.iter())
            .filter_map(|(id, value)| Some((id.as_str()?.to_owned(), role(Some(value), &executor))))
            .collect();
        Self {
            concurrency: positive("concurrency").and_then(|v| usize::try_from(v).ok()).unwrap_or(defaults.concurrency),
            modules,
            timeout: positive("timeout_minutes").map_or(defaults.timeout, |m| Duration::from_secs(m.saturating_mul(60))),
            max_attempts: positive("max_attempts").and_then(|v| u32::try_from(v).ok()).unwrap_or(defaults.max_attempts),
            executor,
        }
    }
    /// The executor for a module: its override, else the project's.
    pub fn role(&self, module: &str) -> &Role {
        self.modules.get(module).unwrap_or(&self.executor)
    }
}

/// One task waiting to be run by a delegate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    pub task: String,
    /// Asked for explicitly; else the configured role decides at dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub by: String,
    pub at: Stamp,
}

/// The integrator's latest word on one run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeMark {
    pub task: String,
    pub run: String,
    /// `done` or `rejected`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<Stage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    /// The `main` the integrator worked against.
    pub base: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub at: Stamp,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueData {
    task: String,
    #[serde(default)]
    harness: Option<String>,
    #[serde(default)]
    model: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DequeueData {
    task: String,
    reason: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeData {
    task: String,
    run: String,
    #[serde(default)]
    evidence_id: Option<String>,
    base: String,
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    stage: Option<Stage>,
    #[serde(default)]
    reason: Option<String>,
}

pub fn queue(state: &Value) -> Result<BTreeMap<String, Queued>, String> {
    match state.get(QUEUE) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}
pub fn merges(state: &Value) -> Result<BTreeMap<String, MergeMark>, String> {
    match state.get(MERGE) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}
fn stamp(event: &Event) -> Stamp {
    Stamp { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq }
}

pub(crate) fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    match event.kind.as_str() {
        QUEUED => queued(state, event)?,
        DEQUEUED => dequeued(state, event)?,
        MERGE_DONE | MERGE_REJECTED => merged(state, event)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn requester(actor: &str) -> bool {
    actor == CLI || actor.starts_with("human:")
}

fn queued(state: &mut Value, event: &Event) -> Result<(), String> {
    let data: QueueData = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if !requester(&event.actor) || event.layer != "L5" || !event.guards.is_empty() || event.obj != data.task {
        return Err("派发请求须由命令行或 App 在 L5 发出".into());
    }
    if data.harness.as_deref().is_some_and(|id| crate::harness::find(id).is_none())
        || data.model.as_deref().is_some_and(|model| model.trim().is_empty() || model.len() > 128)
    {
        return Err("未知的 harness 或无效模型名".into());
    }
    let projection = tasks(state)?;
    let task = projection.tasks.get(&data.task).ok_or("任务不存在")?;
    if task.state == TaskState::Merged || task.active_run.is_some() {
        return Err("任务已完成或已有活动运行，不能再排队".into());
    }
    let mut all = queue(state)?;
    if all.contains_key(&data.task) {
        return Err("任务已在派发队列中".into());
    }
    all.insert(data.task.clone(), Queued { task: data.task, harness: data.harness, model: data.model,
        by: event.actor.clone(), at: stamp(event) });
    state[QUEUE] = serde_json::to_value(all).map_err(|e| e.to_string())?;
    Ok(())
}

fn dequeued(state: &mut Value, event: &Event) -> Result<(), String> {
    let data: DequeueData = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if !(requester(&event.actor) || event.actor == SCHEDULER) || event.layer != "L5" || !event.guards.is_empty()
        || event.obj != data.task || data.reason.trim().is_empty()
    {
        return Err("取消派发须由命令行、App 或调度器在 L5 发出".into());
    }
    let mut all = queue(state)?;
    if all.remove(&data.task).is_none() {
        return Err("任务不在派发队列中".into());
    }
    state[QUEUE] = serde_json::to_value(all).map_err(|e| e.to_string())?;
    Ok(())
}

fn merged(state: &mut Value, event: &Event) -> Result<(), String> {
    let data: MergeData = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let main_guard = event.guards.len() == 1 && event.guards[0].name == "main" && event.guards[0].epoch > 0;
    if event.actor != INTEGRATOR || event.layer != "L5" || !main_guard || event.obj != data.task || !valid_oid(&data.base) {
        return Err("合并结果须由持有 main 的集成器在 L5 记录".into());
    }
    let mut projection = tasks(state)?;
    let run = projection.runs.get(&data.run).ok_or("合并结果缺少运行")?;
    if run.task != data.task || run.dispatch.is_none() {
        return Err("合并结果只属于调度器派发的运行".into());
    }
    let done = event.kind == MERGE_DONE;
    if done {
        let evidence = data.evidence_id.as_deref().ok_or("合并缺少证据 ID")?;
        let entry = &state["Evidence"][evidence];
        if entry["run"] != data.run || entry["status"] != "passed" || !entry["g5_eid"].is_string()
            || data.commit.as_deref().is_none_or(|commit| !valid_oid(commit))
            || data.stage.is_some() || data.reason.is_some()
        {
            return Err("合并须绑定该运行已通过的证据和 G5，以及实际提交".into());
        }
    } else {
        let stage = data.stage.ok_or("拒绝须写明阶段")?;
        let reason = data.reason.as_deref().filter(|r| !r.trim().is_empty()).ok_or("拒绝须写明原因")?;
        if data.commit.is_some() {
            return Err("拒绝不带合并提交".into());
        }
        if stage.counts() {
            if let Some(task) = projection.tasks.get_mut(&data.task) {
                // The delegate redoes the work on the newest main.
                if task.active_run.as_deref() == Some(data.run.as_str()) && task.state == TaskState::Verifying {
                    task.state = TaskState::Running;
                }
            }
            projection.gate_failed(&data.task, MERGE_GATE, reason, &event.eid)?;
            write_tasks(state, &projection)?;
        }
    }
    let mark = MergeMark { task: data.task.clone(), run: data.run.clone(),
        status: if done { "done" } else { "rejected" }.into(), stage: data.stage, reason: data.reason,
        evidence_id: data.evidence_id, base: data.base, commit: data.commit, at: stamp(event) };
    let mut all = merges(state)?;
    all.insert(data.run, mark);
    state[MERGE] = serde_json::to_value(all).map_err(|e| e.to_string())?;
    if done {
        let mut waiting = queue(state)?;
        if waiting.remove(&data.task).is_some() {
            state[QUEUE] = serde_json::to_value(waiting).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The headless task card (10.4): the delegation card's fields plus the
/// paths and checks the delegate is held to. `note` says why it runs again.
pub fn card(task: &Task, note: Option<&str>) -> String {
    let input = &task.input;
    let mut text = format!("# {} {}\n\n- 模块：{}\n- 基线：{}\n", input.id, input.title, input.module, task.baseline);
    if !input.touches.is_empty() {
        text += &format!("- 也会触及：{}\n", input.touches.join("、"));
    }
    text += "\n## 允许修改的路径\n\n";
    for path in &input.allowed_paths {
        text += &format!("- `{path}`\n");
    }
    if let Some(why) = &input.why {
        text += &format!("\n## 为什么\n\n{why}\n");
    }
    text += "\n## 完成标准\n\n";
    for (index, criterion) in input.acceptance.iter().enumerate() {
        text += &format!("{}. {criterion}\n", index + 1);
    }
    if let Some(limits) = &input.limits {
        text += &format!("\n## 限制\n\n{limits}\n");
    }
    if !input.checkpoints.is_empty() {
        text += "\n## 检查点\n\n";
        for checkpoint in &input.checkpoints {
            text += &format!("- {}{}\n", checkpoint.text, checkpoint.due.as_ref().map_or(String::new(), |due| format!("（{due}）")));
        }
    }
    if let Some(check) = &input.self_check {
        text += &format!("\n## 自检\n\n{check}\n");
    }
    text += "\n## 交付\n\n在当前工作树的任务分支上提交全部改动；集成器会变基到最新 main、运行模块验证，通过后才合并。\n";
    if let Some(note) = note {
        text += &format!("\n## 上一次为什么没有合并\n\n{note}\n");
    }
    text
}

/// One decision of a scheduling pass, carried out by the daemon in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Interrupt a live harness: the task was paused, held or cancelled.
    Stop { run: String },
    /// Give the run up; a queued task is dispatched again as a new run.
    Release { run: String, reason: ReleaseReason },
    /// Keep the run's module leases while it works or waits to merge.
    Renew { run: String },
    /// The daemon lost the process (restart): record it as interrupted.
    Orphaned { run: String },
    /// Start (or restart) the delegate; `note` tells it what failed.
    Launch { run: String, note: Option<String> },
    /// Rebase, verify and fast-forward, serially.
    Integrate { run: String },
    /// Verification stopped without a result: a counted rejection.
    RejectVerify { run: String, evidence_id: String, reason: String },
    /// The delegate reported it cannot finish: a person decides.
    GiveUp { run: String, task: String, finish_eid: String, status: ReportStatus, summary: Option<String> },
    Dequeue { task: String, reason: String },
    /// Claim a new run for a queued task.
    Dispatch { task: String, harness: String, model: Option<String> },
}

fn evidence_after(state: &Value, run: &str, after: u64) -> Option<(String, String, Option<String>)> {
    let timing = timings(state).ok()?.remove(run)?;
    let (id, _) = timing.finishes.iter().filter(|(_, at)| at.seq > after).max_by_key(|(_, at)| at.seq)?;
    let entry = &state["Evidence"][id];
    Some((id.clone(), entry["status"].as_str().unwrap_or_default().to_owned(), entry["error"].as_str().map(str::to_owned)))
}

/// One pass over this host's dispatched runs and the queue. `live` are the
/// runs whose harness this daemon is running now.
pub fn plan(state: &Value, host: &str, live: &BTreeSet<String>, settings: &Settings) -> Result<Vec<Step>, String> {
    let projection = tasks(state)?;
    let waiting = queue(state)?;
    let records = run_record::records(state)?;
    let marks = merges(state)?;
    let mut steps = Vec::new();
    let mut busy = 0;
    for run in projection.runs.values().filter(|run| run.state == RunState::Active && run.binding.host == host && run.dispatch.is_some()) {
        busy += 1;
        let id = run.binding.id.clone();
        let Some(task) = projection.tasks.get(&run.task) else { continue };
        let is_live = live.contains(&id);
        let queued = waiting.contains_key(&run.task);
        let blocked = projection.paused_all || task.needs_review || matches!(task.state, TaskState::Held | TaskState::Paused);
        if !queued || blocked {
            if is_live {
                steps.push(Step::Stop { run: id });
            } else if !queued {
                steps.push(Step::Release { run: id, reason: ReleaseReason::Voluntary });
            }
            continue;
        }
        if task.state == TaskState::Failed {
            if !is_live {
                // Under three failures the runner leaves it to a new run (12.1).
                steps.push(Step::Release { run: id, reason: ReleaseReason::Reassigned });
            }
            continue;
        }
        if matches!(task.state, TaskState::Running | TaskState::Verifying) {
            steps.push(Step::Renew { run: id.clone() });
        }
        if is_live {
            continue;
        }
        let record = records.get(&id);
        if task.state == TaskState::Verifying {
            steps.push(Step::Integrate { run: id });
            continue;
        }
        if task.state != TaskState::Running {
            continue;
        }
        if record.is_some_and(|record| record.running.is_some()) {
            steps.push(Step::Orphaned { run: id });
            continue;
        }
        let Some(last) = record.and_then(|record| record.finishes.last()) else {
            steps.push(Step::Launch { run: id, note: None });
            continue;
        };
        match (last.outcome, last.status) {
            (Outcome::Reported, ReportStatus::Done) => {
                let rejected = marks.get(&id).filter(|mark| mark.status == "rejected" && mark.at.seq > last.at.seq);
                match evidence_after(state, &id, last.at.seq) {
                    Some((evidence, status, error)) if status == "interrupted"
                        && rejected.is_none_or(|mark| mark.evidence_id.as_deref() != Some(evidence.as_str())) =>
                    {
                        steps.push(Step::RejectVerify { run: id, evidence_id: evidence,
                            reason: error.unwrap_or_else(|| "验证中断".into()) });
                    }
                    Some((_, status, _)) if status == "failed" => {
                        steps.push(Step::Launch { run: id, note: Some("模块验证没有通过；请看证据中的失败命令，修好后再提交。".into()) });
                    }
                    _ if rejected.is_some() => {
                        let mark = rejected.unwrap();
                        steps.push(Step::Launch { run: id, note: Some(format!("集成器拒绝合并：{}", mark.reason.clone().unwrap_or_default())) });
                    }
                    _ => steps.push(Step::Integrate { run: id }),
                }
            }
            (Outcome::Reported, status) => steps.push(Step::GiveUp { run: id, task: run.task.clone(),
                finish_eid: last.at.eid.clone(), status, summary: None }),
            // Interrupted (or resumed after a decision): run it again.
            _ => steps.push(Step::Launch { run: id, note: None }),
        }
    }
    let mut candidates = waiting.values().collect::<Vec<_>>();
    candidates.sort_by_key(|queued| (std::cmp::Reverse(projection.tasks.get(&queued.task).map_or(0, |t| t.input.priority)), queued.at.seq));
    for queued in candidates {
        let Some(task) = projection.tasks.get(&queued.task) else {
            steps.push(Step::Dequeue { task: queued.task.clone(), reason: "任务不存在".into() });
            continue;
        };
        if task.state == TaskState::Merged {
            steps.push(Step::Dequeue { task: queued.task.clone(), reason: "任务已完成".into() });
            continue;
        }
        if busy >= settings.concurrency {
            break;
        }
        if projection.paused_all || task.needs_review || task.active_run.is_some()
            || !matches!(task.state, TaskState::Proposed | TaskState::Ready)
        {
            continue;
        }
        let role = settings.role(&task.input.module);
        busy += 1;
        steps.push(Step::Dispatch { task: queued.task.clone(),
            harness: queued.harness.clone().unwrap_or_else(|| role.harness.clone()),
            model: queued.model.clone().or_else(|| role.model.clone()) });
    }
    Ok(steps)
}

/// The scheduler's interrupt when a delegate reported it cannot finish.
pub fn give_up_alert(host: &str, task: &str, run: &str, finish_eid: &str, status: ReportStatus) -> towermd_core::hub_protocol::NewEvent {
    let title = "派发的运行没有完成，已停止派发，等人决定";
    towermd_core::hub_protocol::NewEvent {
        eid: uuid::Uuid::now_v7().to_string(), host: host.into(), actor: SCHEDULER.into(), kind: "alert.raised".into(),
        layer: "L5".into(), obj: format!("dispatch-{finish_eid}"), guards: Vec::new(),
        data: json!({"reason": run_record::RUN_ERROR, "title": title, "detail": {
            "task": task, "run": run, "severity": "blocking", "error": format!("运行报告状态：{}", serde_json::to_value(status).ok()
                .and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()),
        }}),
    }
}

