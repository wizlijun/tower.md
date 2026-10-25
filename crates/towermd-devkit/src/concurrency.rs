//! FR-31: when the tasks dispatched side by side keep interrupting a
//! person, suggest a lower `runner.concurrency`. Measured with core spec
//! section 15's "human interrupts per task" and "interrupt waiting time",
//! computed from the projection when asked and never stored. It only
//! reminds: the limit changes when a person edits `config.yaml`.
use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;
use towermd_core::attention_list::parse_time;
use towermd_core::{question_model::CONTROLLER, question_projection, reviews};

use crate::task_model::{RunState, GATE_EXHAUSTED, FORBIDDEN, INFO_MISSING};
use crate::{dev_state, permissions, run_record};

/// The parallel delegates a person can keep an eye on (spec FR-31: 2–3).
pub const CALIBRATED: usize = 3;
/// A person-bound interrupt open this long counts as a bottleneck.
pub const SLOW_WAIT_MINUTES: u64 = 30;

/// Alerts that stop a task for a person (spec 12.1).
const HUMAN: &[&str] = &[GATE_EXHAUSTED, FORBIDDEN, INFO_MISSING, run_record::RUN_ERROR, run_record::BUDGET_EXHAUSTED,
    run_record::BUDGET_UNKNOWN, permissions::ALERT];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hint {
    /// `runner.concurrency` as configured.
    pub limit: usize,
    /// Tasks with a dispatched run in progress.
    pub active: usize,
    /// Interrupts of those tasks that went to a person: questions the
    /// controller did not answer, reviews, and the 12.1 stops.
    pub interrupts: usize,
    /// The longest a person-bound interrupt of those tasks still open has waited.
    pub longest_wait_minutes: Option<u64>,
    /// A lower limit worth considering; never applied by Tower.md.
    pub suggest: Option<usize>,
    pub reason: Option<String>,
}

/// `now` in milliseconds since the epoch.
pub fn hint(state: &Value, limit: usize, now: u64) -> Result<Hint, String> {
    let projection = dev_state::tasks(state)?;
    let active_runs: BTreeSet<&str> = projection.runs.values()
        .filter(|run| run.dispatch.is_some() && run.state == RunState::Active).map(|run| run.binding.id.as_str()).collect();
    let active: BTreeSet<&str> = projection.runs.values()
        .filter(|run| active_runs.contains(run.binding.id.as_str())).map(|run| run.task.as_str()).collect();
    let mut interrupts = 0;
    let mut waits = Vec::new();
    let mut open = |since: u64| if since > 0 { waits.push(now.saturating_sub(since) / 60_000) };
    let questions = question_projection::read(state)?;
    for (hold, question) in &questions.holds {
        if !active_runs.contains(question.binding.run.as_str()) { continue }
        match questions.answers.get(hold) {
            Some(answer) if answer.actor == CONTROLLER => continue,
            Some(_) => interrupts += 1,
            None => { interrupts += 1; open(question.opened_at) }
        }
    }
    for review in reviews::read(state)?.values().filter(|review| active.contains(review.request.object.as_str())) {
        interrupts += 1;
        if review.open() { open(parse_time(&review.requested.at)) }
    }
    let records = run_record::records(state)?;
    for alert in state.get("Alert").and_then(Value::as_object).into_iter().flat_map(|alerts| alerts.values()) {
        let task = alert["data"]["detail"]["task"].as_str().unwrap_or_default();
        if !active.contains(task) || !alert["data"]["reason"].as_str().is_some_and(|reason| HUMAN.contains(&reason)) { continue }
        interrupts += 1;
        if alert["resolved"] == true { continue }
        // The runner's alerts carry no time of their own: their run's finish has it.
        let at = alert["at"].as_str().map(str::to_owned).or_else(|| records.values().flat_map(|record| &record.finishes)
            .find(|finish| alert["event"].as_str() == Some(finish.at.eid.as_str())).map(|finish| finish.at.at.clone()));
        open(at.as_deref().map_or(0, parse_time));
    }
    let longest_wait_minutes = waits.into_iter().max();
    let (count, crowded) = (active.len(), interrupts >= active.len().max(1));
    let slow = longest_wait_minutes.is_some_and(|wait| wait >= SLOW_WAIT_MINUTES);
    let suggest = (limit > CALIBRATED && count >= 2 && (crowded || slow))
        .then_some(if interrupts >= 2 * count { CALIBRATED - 1 } else { CALIBRATED });
    let reason = suggest.map(|to| {
        let wait = longest_wait_minutes.map_or(String::new(), |wait| format!("，最久的已等 {wait} 分钟"));
        format!("并行的 {count} 项任务共有 {interrupts} 次人工中断（每任务 {:.1}）{wait}：可以把 config.yaml 的 runner.concurrency \
            从 {limit} 降到 {to}；Tower.md 只提示，不改配置", interrupts as f64 / count as f64)
    });
    Ok(Hint { limit, active: count, interrupts, longest_wait_minutes, suggest, reason })
}
