//! Task interrupts (spec 10.2, 12.1): what holds a task for a person, and
//! the person's way out. Questions and reviews hold a working task while they
//! are open; gate failures, G2 stops, missing information and the runner's
//! error and budget stops hold it until a person decides in the App.
//!
//! Every interrupt that waits on a decision has one alert, keyed by the
//! event that opened it, so the attention list shows it; the alert closes
//! when the interrupt ends. Nothing here completes work: a decision only
//! lets the delegate try again, ends the run, or sends the card back.
use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::dev_state::{tasks, tasks_scoped, write_scoped, write_tasks};
use crate::task_model::{InfoRequested, Interrupt, InterruptKind, InterruptResolved};
use towermd_core::gates::{self, Recorded, Verdict};
use towermd_core::hub::Event;
use towermd_core::project_state::insert_unique;
use towermd_core::{question_projection, reviews, waivers};

/// A person's decision on a held task (App only).
pub const RESOLVED: &str = "task.interrupt.resolved";
/// G0 found the card short of information (D6): a question to its creator.
pub const INFO_REQUESTED: &str = "task.info_requested";
/// The builtin check every development task passes (D4).
pub const VERIFY: &str = "core.verify";

pub type Snapshot = BTreeMap<String, Option<Interrupt>>;

/// The interrupts before an event, to tell which ones it opened or ended.
/// Only tasks that have one are listed: every event asks, so no task without
/// an interrupt is read.
pub fn snapshot(state: &Value) -> Result<Snapshot, String> {
    state["Task"].as_object().ok_or("Task 投影无效")?.iter()
        .filter(|(_, task)| !task["interrupt"].is_null())
        .map(|(id, task)| Ok((id.clone(), Some(serde_json::from_value(task["interrupt"].clone()).map_err(|e| e.to_string())?))))
        .collect()
}

/// The two task events this module owns.
pub(crate) fn reduce(state: &mut Value, event: &Event) -> Result<bool, String> {
    match event.kind.as_str() {
        RESOLVED => {
            let mut projection = tasks(state)?;
            if !event.actor.starts_with("human:") || event.layer != "L4" || !event.guards.is_empty() {
                return Err("中断只能由人在 App 中处理".into());
            }
            let input: InterruptResolved = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            if input.note.as_ref().is_some_and(|note| note.trim().is_empty() || note.chars().count() > 2_000) {
                return Err("处理说明不能为空或超过 2000 字".into());
            }
            let kind = projection.tasks.get(&event.obj).and_then(|task| task.interrupt.as_ref()).map(|interrupt| interrupt.kind);
            projection.resolve_interrupt(&event.obj, &input)?;
            write_tasks(state, &projection)?;
            if kind == Some(InterruptKind::RunError) {
                crate::run_record::forgive(state, &event.obj)?;
            }
        }
        INFO_REQUESTED => {
            if event.actor != "process:towermd-cli" || event.layer != "L4" || !event.guards.is_empty() {
                return Err("补充信息请求只能由领取流程记录".into());
            }
            let mut projection = tasks(state)?;
            let input: InfoRequested = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            if input.reason.trim().is_empty() || input.reason.chars().count() > 2_000 {
                return Err("补充信息请求须说明缺什么".into());
            }
            projection.request_info(&event.obj, &input.reason, &event.eid)?;
            write_tasks(state, &projection)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Core events that move tasks: gate evidence counts a failure, a waiver
/// ends the gate failure it names.
pub(crate) fn observe(state: &mut Value, event: &Event) -> Result<(), String> {
    match event.kind.as_str() {
        gates::RECORDED if event.actor == gates::TOWER => {
            let Ok(Recorded::Gate { object, stage, verdict: Verdict::Blocked, gates, .. }) =
                serde_json::from_value::<Recorded>(event.data.clone()) else { return Ok(()) };
            // Builtin checks are counted where they run (the verifier).
            let Some(gate) = gates.iter().find(|gate| gate.blocks() && !gate.builtin) else { return Ok(()) };
            if stage != "finish" {
                return Ok(());
            }
            let mut projection = tasks(state)?;
            if projection.tasks.contains_key(&object) {
                projection.gate_failed(&object, &gate.gate, gate.reason.as_deref().unwrap_or("检查未通过"), &event.eid)?;
                write_tasks(state, &projection)?;
            }
        }
        waivers::GRANTED => {
            let granted: waivers::Granted = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            let mut projection = tasks(state)?;
            if projection.waived(&event.obj, &granted.gate) {
                write_tasks(state, &projection)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// After any task change: open questions and reviews hold or release working
/// tasks, and each decision-waiting interrupt has exactly one open alert.
pub(crate) fn settle(state: &mut Value, event: &Event, before: &Snapshot) -> Result<(), String> {
    // Only a working task, or one a question or review holds, can change.
    let mut candidates = Vec::new();
    for (id, task) in state["Task"].as_object().ok_or("Task 投影无效")? {
        let live = !task["interrupt"].is_null()
            && serde_json::from_value::<Interrupt>(task["interrupt"].clone()).map_err(|e| e.to_string())?.kind.live();
        if live || !task["active_run"].is_null() {
            candidates.push(id.clone());
        }
    }
    if !candidates.is_empty() {
        let ids: Vec<&str> = candidates.iter().map(String::as_str).collect();
        let mut projection = tasks_scoped(state, &ids, &[])?;
        let open_reviews = reviews::read(state)?;
        let questions = question_projection::blocked_tasks(state)?;
        let mut changed = false;
        for id in &candidates {
            let review = reviews::open_for(&open_reviews, id).next().is_some();
            changed |= projection.settle(id, questions.contains(id), review, &event.eid);
        }
        if changed {
            write_scoped(state, &projection)?;
        }
    }
    let after = snapshot(state)?;
    let ids: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    for id in ids {
        let old = before.get(id).cloned().flatten();
        let new = after.get(id).and_then(Option::as_ref);
        if let Some(old) = &old {
            if new.is_none_or(|new| new.opened != old.opened) {
                if let Some(alert) = state.get_mut("Alert").and_then(|alerts| alerts.get_mut(&old.opened)) {
                    if alert["resolved"] != true {
                        alert["resolved"] = json!(true);
                        alert["resolved_by"] = json!(event.eid);
                    }
                }
            }
        }
        let Some(new) = new else { continue };
        let Some(reason) = new.kind.alert_reason() else { continue };
        if old.is_some_and(|old| old.opened == new.opened) {
            continue;
        }
        let run = state["Task"][id.as_str()]["active_run"].clone();
        insert_unique(state, "Alert", &new.opened, json!({
            "id": new.opened, "event": new.opened, "at": event.t, "title": title(new.kind), "resolved": false,
            "data": {"reason": reason, "detail": {
                "task": id, "run": run, "gate": new.gate, "severity": "blocking", "error": new.reason,
            }},
        }))?;
    }
    Ok(())
}

fn title(kind: InterruptKind) -> &'static str {
    match kind {
        InterruptKind::GateFailed => "硬门禁自动修复次数用完",
        InterruptKind::MissingInfo => "委托卡缺信息，任务未派出",
        InterruptKind::Forbidden => "改动被路径守卫拦截",
        _ => "任务中断中",
    }
}

