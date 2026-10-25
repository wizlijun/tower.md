//! The development kit's side of delegation: each task is a delegable
//! object, each claim one delegation, finish evidence the `check` evidence
//! and finish claims the `claimed` evidence. Tasks complete only by checks.
use serde_json::Value;
use towermd_core::{
    delegation::{Evidence, KitAttempt, KitDelegation, Phase, SourceKind, Stamp},
    delegation_projection::Finalized,
    hub::Event,
};

use crate::{
    dev_state::{tasks, timings, write_tasks},
    task_finish::FinishRequest,
    task_model::{RunState, TaskState},
};

/// FR-121: a module's verify commands are the built-in deterministic check
/// `core.verify`; every task carries it, so only checks complete a task.
pub const VERIFY_CHECK: &str = "core.verify";

pub(crate) fn delegations(state: &Value) -> Result<Vec<KitDelegation>, String> {
    let projection = tasks(state)?;
    let timing = timings(state)?;
    let evidence = state["Evidence"].as_object().ok_or("Evidence 投影无效")?;
    let stops = out_of_bounds(state);
    let mut items = Vec::new();
    for task in projection.tasks.values() {
        let id = &task.input.id;
        let phase = match task.state {
            TaskState::Proposed if task.input.clarify => Phase::Clarifying,
            TaskState::Proposed => Phase::Proposed,
            TaskState::Ready => Phase::Ready,
            TaskState::Paused if task.active_run.is_none() => Phase::Ready,
            TaskState::Running | TaskState::Held | TaskState::Paused | TaskState::Failed => Phase::Running,
            TaskState::Verifying => Phase::ClaimedDone,
            TaskState::Merged => Phase::Done,
        };
        let entries: Vec<_> = evidence.values().filter(|entry| entry["task"] == **id && entry["kind"] == "finish").collect();
        // Runs from a projection saved before v0.7 have no times; they are
        // left out rather than given invented ones.
        let mut attempts: Vec<KitAttempt> = projection.runs.values()
            .filter(|run| run.task == *id)
            .filter_map(|run| timing.get(&run.binding.id).map(|times| (run, times)))
            .map(|(run, times)| {
                let mine = entries.iter().filter(|entry| entry["run"] == *run.binding.id);
                let merged = mine.clone().find_map(|entry| entry["merge"]["tree"].as_str());
                let tested = mine.filter_map(|entry| times.finishes.get(entry["id"].as_str()?)
                    .zip(entry["input"]["tree"].as_str())).max_by_key(|(stamp, _)| stamp.seq).map(|(_, tree)| tree);
                KitAttempt {
                    id: run.binding.id.clone(), delegate: run.binding.self_reported.clone(),
                    delegated: stamp(&times.claimed), delivered: times.delivered.as_ref().map(stamp),
                    done: times.done.as_ref().map(stamp), active: run.state == RunState::Active,
                    version: merged.or(tested).map(str::to_owned),
                    out_of_bounds: Some(stops.get(run.binding.id.as_str()).copied().unwrap_or(0)),
                }
            })
            .collect();
        attempts.sort_by_key(|attempt| attempt.delegated.seq);
        let mut found = Vec::new();
        for entry in entries {
            let evidence_id = entry["id"].as_str().ok_or("任务证据缺少 ID")?;
            let request: FinishRequest = serde_json::from_value(entry["request"].clone()).map_err(|e| e.to_string())?;
            let run = request.run.binding.id.clone();
            let version = entry["input"]["tree"].as_str().map(str::to_owned);
            if let Some(passed) = entry["evidence"]["passed"].as_bool() {
                found.push(Evidence {
                    id: evidence_id.to_owned(), source_kind: SourceKind::Check, criteria: vec![], passed: Some(passed),
                    version: version.clone(), actor: entry["completion"]["actor"].as_str().unwrap_or_default().to_owned(),
                    at: entry["completion"]["t"].as_str().unwrap_or_default().to_owned(), note: None, attempt: run.clone(),
                    check: Some(VERIFY_CHECK.into()),
                });
            }
            let at = match timing.get(&run).and_then(|times| times.finishes.get(evidence_id)) {
                Some(stamp) => stamp.at.clone(),
                None => chrono::DateTime::from_timestamp_millis(request.requested_at_ms as i64)
                    .ok_or("finish 请求时间无效")?.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            };
            let actor = request.run.binding.self_reported.as_ref()
                .map_or("process:towermd-cli".to_owned(), |source| format!("{}/{}", source.name, source.version));
            for (index, claim) in request.claims.iter().enumerate() {
                found.push(Evidence {
                    id: format!("{evidence_id}#claim-{index}"), source_kind: SourceKind::Claimed,
                    criteria: vec![claim.criterion], passed: None, version: version.clone(), actor: actor.clone(),
                    at: at.clone(), note: Some(claim.note.clone()), attempt: run.clone(), check: None,
                });
            }
        }
        let run = task.active_run.as_ref().and_then(|run| projection.runs.get(run));
        items.push(KitDelegation {
            id: id.clone(), title: task.input.title.clone(), brief: task.input.brief(),
            work_kind: task.input.module.clone(), phase, human_can_complete: false, attempts, evidence: found,
            checks: crate::task_reviews::checks(run.is_some_and(|run| run.high_risk)),
            facets: crate::task_reviews::facets(task, run),
        });
    }
    Ok(items)
}

/// FR-202 越界: each G2 stop (the forbidden interrupt, 12.1 禁止) counts once
/// for the run it stopped. Stops recorded before that interrupt existed
/// carry no run and are not counted.
fn out_of_bounds(state: &Value) -> std::collections::BTreeMap<&str, u32> {
    let mut counts = std::collections::BTreeMap::new();
    let alerts = state.get("Alert").and_then(Value::as_object).into_iter().flat_map(|alerts| alerts.values());
    for alert in alerts.filter(|alert| alert["data"]["reason"] == crate::task_model::FORBIDDEN) {
        if let Some(run) = alert["data"]["detail"]["run"].as_str() {
            *counts.entry(run).or_insert(0) += 1;
        }
    }
    counts
}

fn stamp(stamp: &crate::dev_state::Stamp) -> Stamp {
    Stamp { at: stamp.at.clone(), eid: stamp.eid.clone(), seq: stamp.seq }
}

/// The person's final card updates the task; a send-back reopens it.
pub(crate) fn react(state: &mut Value, event: &Event) -> Result<bool, String> {
    let mut projection = tasks(state)?;
    match event.kind.as_str() {
        "delegation.finalized" => {
            let data: Finalized = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            projection.finalize(&event.obj, &data.brief)?;
        }
        "acceptance.sent_back" if projection.tasks.contains_key(&event.obj) => projection.reopen(&event.obj)?,
        _ => return Ok(false),
    }
    write_tasks(state, &projection)?;
    Ok(true)
}
