//! Reducer for the runner's `run.*` events (6.3, 10.2, 12.1). The record
//! group is created lazily, so journals without runner events replay to the
//! v0.1.1 projection unchanged.
//!
//! Only the runner's own observations move a task, and only away from done:
//! a failed run makes the task failed, or held with an error interrupt on a
//! crash or the third failure; a budget stop makes it held with a budget
//! interrupt. The report is recorded and never moves anything (FR-33).
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::dev_state::{tasks, write_tasks, Stamp};
use crate::run_report::{
    ErrorClass, Outcome, ReportStatus, RunCheckpoint, RunFinished, RunRetry, RunStarted, Stop, CHECKPOINT,
    FAILURES_BEFORE_HOLD, FINISHED, RETRY, RUNNER, STARTED,
};
use crate::task_model::{InterruptKind, RunState, TaskState};
use towermd_core::hub::Event;
use towermd_core::project_state::insert_unique;

pub const GROUP: &str = "RunRecord";

/// Alert reasons this module raises; the App names them by reason.
pub const RUN_ERROR: &str = "run_error";
pub const BUDGET_EXHAUSTED: &str = "budget_exhausted";
pub const BUDGET_UNKNOWN: &str = "budget_unknown";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FinishMark {
    pub at: Stamp,
    pub outcome: Outcome,
    pub status: ReportStatus,
    pub stop: Option<Stop>,
    pub error_class: Option<ErrorClass>,
    pub cost_usd: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub task: String,
    pub module: String,
    pub agent: String,
    pub budget_usd: Option<f64>,
    /// Set while a harness process of this run is live.
    pub running: Option<Stamp>,
    pub starts: u32,
    pub retries: u32,
    /// Runs the runner observed failing; counted per task across runs.
    pub failures: u32,
    /// Failures a person already decided on (a resolved error interrupt);
    /// the next hold counts only the ones after it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub forgiven: u32,
    /// Sum of the reported costs only.
    pub known_cost_usd: f64,
    /// Sessions that ended without a reported cost.
    pub unknown_cost: u32,
    pub finishes: Vec<FinishMark>,
    /// 6.3 `run.checkpoint`: commits seen while a process was live, oldest
    /// first. Omitted when empty, so earlier projections keep their shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<CheckpointMark>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointMark {
    pub at: Stamp,
    pub commit: String,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

pub fn records(state: &Value) -> Result<BTreeMap<String, RunRecord>, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}

/// Failures the runner observed for one task, over all its runs.
pub fn task_failures(records: &BTreeMap<String, RunRecord>, task: &str) -> u32 {
    records.values().filter(|record| record.task == task).map(|record| record.failures).sum()
}

/// Failures since a person last decided on the task's error interrupt.
fn open_failures(records: &BTreeMap<String, RunRecord>, task: &str) -> u32 {
    records.values().filter(|record| record.task == task).map(|record| record.failures - record.forgiven).sum()
}

/// A person decided on the task's interrupt: the failures so far are seen.
/// The record group exists only once the runner ran, so nothing is created.
pub(crate) fn forgive(state: &mut Value, task: &str) -> Result<(), String> {
    if state.get(GROUP).is_none() {
        return Ok(());
    }
    let mut all = records(state)?;
    for record in all.values_mut().filter(|record| record.task == task) {
        record.forgiven = record.failures;
    }
    state[GROUP] = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    Ok(())
}

fn check_signer(event: &Event, agent: Option<&str>) -> Result<(), String> {
    let signed = event.actor == RUNNER || agent.is_some_and(|agent| event.actor == agent);
    if !signed || event.layer != "L5" || !event.guards.is_empty() {
        return Err("运行事件须由运行器在 L5 署名".into());
    }
    Ok(())
}

pub(crate) fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    match event.kind.as_str() {
        STARTED => started(state, event)?,
        FINISHED => finished(state, event)?,
        RETRY => retried(state, event)?,
        CHECKPOINT => checkpoint(state, event)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn started(state: &mut Value, event: &Event) -> Result<(), String> {
    check_signer(event, None)?;
    let data: RunStarted = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let projection = tasks(state)?;
    let run = projection.runs.get(&data.run).ok_or("运行开始缺少已领取的运行")?;
    let task = projection.tasks.get(&run.task).ok_or("运行开始缺少任务")?;
    if event.obj != run.task || data.task != run.task || run.state != RunState::Active
        || task.active_run.as_deref() != Some(run.binding.id.as_str()) || task.state != TaskState::Running
        || data.agent.split_once('/').is_none_or(|(a, b)| a.is_empty() || b.is_empty())
        || data.budget_usd.is_some_and(|budget| !budget.is_finite() || budget < 0.0)
    {
        return Err("运行开始须对应运行中任务的当前运行".into());
    }
    let mut all = records(state)?;
    let record = all.entry(data.run.clone()).or_insert_with(|| RunRecord {
        task: run.task.clone(), module: run.module.clone(), agent: data.agent.clone(), budget_usd: data.budget_usd,
        running: None, starts: 0, retries: 0, failures: 0, forgiven: 0, known_cost_usd: 0.0, unknown_cost: 0, finishes: Vec::new(),
        checkpoints: Vec::new(),
    });
    if record.running.is_some() {
        return Err("运行已有活动进程".into());
    }
    record.agent = data.agent;
    record.budget_usd = data.budget_usd;
    record.running = Some(Stamp { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq });
    record.starts += 1;
    state[GROUP] = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    Ok(())
}

fn retried(state: &mut Value, event: &Event) -> Result<(), String> {
    let data: RunRetry = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let mut all = records(state)?;
    let record = all.get_mut(&data.run).ok_or("重试缺少运行记录")?;
    check_signer(event, Some(&record.agent))?;
    let expected = if event.actor == RUNNER { "runner" } else { "harness" };
    if event.obj != record.task || record.running.is_none() || data.source != expected {
        return Err("重试须对应活动进程，来源与署名一致".into());
    }
    record.retries += 1;
    state[GROUP] = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    Ok(())
}

fn checkpoint(state: &mut Value, event: &Event) -> Result<(), String> {
    check_signer(event, None)?;
    let data: RunCheckpoint = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let mut all = records(state)?;
    let record = all.get_mut(&data.run).ok_or("检查点缺少运行记录")?;
    if record.running.is_none() || event.obj != record.task || data.task != record.task
        || !towermd_core::project_state::valid_oid(&data.commit)
    {
        return Err("检查点须对应活动进程的完整提交号".into());
    }
    if record.checkpoints.iter().any(|mark| mark.commit == data.commit) {
        return Err("检查点已记录".into());
    }
    record.checkpoints.push(CheckpointMark { at: Stamp { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq },
        commit: data.commit });
    state[GROUP] = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    Ok(())
}

fn finished(state: &mut Value, event: &Event) -> Result<(), String> {
    check_signer(event, None)?;
    let data: RunFinished = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if let Some(report) = &data.report {
        report.validate()?;
    }
    let mut all = records(state)?;
    let record = all.get_mut(&data.run).ok_or("运行结束缺少运行开始")?;
    if record.running.is_none() || event.obj != record.task || data.task != record.task || data.agent != record.agent
        || data.cost_usd.is_some_and(|cost| !cost.is_finite() || cost < 0.0)
        || (data.outcome == Outcome::Reported && (data.report.is_none() || data.exit_code != Some(0)))
        || (data.report.is_none() && data.status != ReportStatus::Failed)
        || (data.stop == Some(Stop::Budget)) && data.outcome != Outcome::Budget
        || (data.outcome == Outcome::Budget) && !matches!(data.stop, None | Some(Stop::Budget))
        || (data.stop == Some(Stop::Interrupted)) != (data.outcome == Outcome::Interrupted)
    {
        return Err("运行结束记录与运行开始或观察结果不一致".into());
    }
    record.running = None;
    match data.cost_usd {
        Some(cost) => record.known_cost_usd += cost,
        None => record.unknown_cost += 1,
    }
    if data.outcome == Outcome::Failed {
        record.failures += 1;
    }
    record.finishes.push(FinishMark { at: Stamp { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq },
        outcome: data.outcome, status: data.status, stop: data.stop, error_class: data.error_class, cost_usd: data.cost_usd });
    let failures = task_failures(&all, &data.task);
    let open = open_failures(&all, &data.task);
    let record = &all[&data.run];
    let (known, unknown, budget) = (record.known_cost_usd, record.unknown_cost, record.budget_usd);
    state[GROUP] = serde_json::to_value(&all).map_err(|e| e.to_string())?;

    let mut projection = tasks(state)?;
    if let Some(run) = projection.runs.get_mut(&data.run) {
        run.cost_usd = (unknown == 0).then_some(known);
    }
    let current = projection.tasks.get(&data.task).is_some_and(|task| {
        task.active_run.as_deref() == Some(data.run.as_str()) && task.state == TaskState::Running
    });
    let mut alert = None;
    if current {
        // A hold carries the interrupt a person decides on (12.1).
        let held = match data.outcome {
            Outcome::Budget => Some((InterruptKind::Budget, BUDGET_EXHAUSTED, "运行达到预算上限，已停止并保留检查点")),
            Outcome::Failed if data.crashed || open >= FAILURES_BEFORE_HOLD => Some((InterruptKind::RunError, RUN_ERROR,
                if data.crashed { "harness 崩溃" } else { "同一任务失败 3 次" })),
            Outcome::Failed => {
                projection.tasks.get_mut(&data.task).unwrap().state = TaskState::Failed;
                None
            }
            Outcome::Reported | Outcome::Interrupted => None,
        };
        if let Some((kind, reason, title)) = held {
            projection.hold(&data.task, &data.run, kind, None, title, &event.eid);
            alert = Some((reason, title));
        }
    }
    if alert.is_none() && data.error_class == Some(ErrorClass::Design) {
        // The delegate's suspicion interrupts a person; it moves nothing.
        alert = Some((RUN_ERROR, "运行报告怀疑是设计类错误"));
    }
    if alert.is_none() && budget.is_some() && data.cost_usd.is_none() {
        alert = Some((BUDGET_UNKNOWN, "设了预算，但这次运行没有报告费用"));
    }
    write_tasks(state, &projection)?;
    if let Some((reason, title)) = alert {
        insert_unique(state, "Alert", &event.eid, json!({
            "id": event.eid, "event": event.eid, "title": title, "resolved": false,
            "data": {"reason": reason, "detail": {
                "task": data.task, "run": data.run, "agent": data.agent, "severity": "blocking",
                "error": data.error.clone().unwrap_or_else(|| data.summary.clone()),
                "error_class": data.error_class, "failures": failures,
                "cost_usd": data.cost_usd, "budget_usd": budget,
            }},
        }))?;
    }
    Ok(())
}
