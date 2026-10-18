//! Task completion inputs and lifecycle. Only the Session producer can mint an
//! execution ticket; serialized reports and job DTOs confer no authority.
pub use crate::recovery_contract::finish::{
    EvidenceResult, FinishEvidence, FrozenFinish, VerificationLogRef,
};
use crate::{
    hub_protocol::{Event, NewEvent},
    recovery_contract::{
        self as contract,
        recorded::ReviewContext,
        verification::{report_hash, VerificationReport},
    },
    snapshot::StateProjection,
    tasks::{Run, Task},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailureAction {
    Running,
    GatefailedHold,
}
pub fn failure_action(prior: u32) -> (u32, FailureAction) {
    let next = prior.saturating_add(1);
    (
        next,
        if next >= 3 {
            FailureAction::GatefailedHold
        } else {
            FailureAction::Running
        },
    )
}
pub fn fixed_deadline(entered_at: u64) -> Result<u64, String> {
    entered_at
        .checked_add(3_600_000)
        .ok_or("deadline overflow".into())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    Queued,
    Running,
    Recording,
    Completed,
    Interrupted,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FinishStatus {
    pub request_id: String,
    pub input: FrozenFinish,
    pub phase: JobPhase,
    pub started_eid: String,
    pub completed_eid: String,
    pub diagnostic: Option<String>,
    pub review_context: Option<ReviewContext>,
    pub checked_gates: BTreeMap<String, bool>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunFinish {
    pub entered_verifying_at: u64,
    pub merge_deadline: u64,
    pub failures: u32,
    pub consumed_failures: BTreeSet<String>,
    pub jobs: BTreeMap<String, FinishStatus>,
}
impl RunFinish {
    pub(crate) fn read(run: &Run) -> Result<Self, String> {
        let state: Self = run
            .extra
            .get("finish")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|e| format!("invalid finish lifecycle: {e}"))?
            .unwrap_or_default();
        if !state.jobs.is_empty()
            && (fixed_deadline(state.entered_verifying_at)? != state.merge_deadline
                || state.failures as usize != state.consumed_failures.len())
        {
            return Err("finish deadline/failure identity differs".into());
        }
        Ok(state)
    }
    pub(crate) fn put(&self, run: &mut Run) -> Result<(), String> {
        run.extra.insert(
            "finish".into(),
            serde_json::to_value(self).map_err(|e| e.to_string())?,
        );
        Ok(())
    }
}
/// Not Clone or Deserialize. A synchronized Session and actual check mint it.
#[derive(Debug)]
pub struct FinishTicket {
    pub(crate) status: FinishStatus,
    pub(crate) session: crate::project_session::SnapshotSessionId,
}
impl FinishTicket {
    pub fn status(&self) -> &FinishStatus {
        &self.status
    }
}
#[derive(Debug)]
pub enum FinishStart {
    Queued(FinishTicket),
    Existing(FinishStatus),
}
/// One accepted Running transition. Consuming it is the only execution entry.
#[derive(Debug)]
pub struct ExecutionTicket {
    pub(crate) status: FinishStatus,
    pub(crate) session: crate::project_session::SnapshotSessionId,
}
/// Actual executor output, never accepted from JSON or a client-provided report.
#[derive(Debug)]
pub struct ExecutedFinish {
    pub(crate) status: FinishStatus,
    pub(crate) session: crate::project_session::SnapshotSessionId,
    pub(crate) result: Result<VerificationReport, String>,
}
pub fn execute(
    ticket: ExecutionTicket,
    timeout: Duration,
    cancelled: Arc<AtomicBool>,
) -> ExecutedFinish {
    let input = &ticket.status.input;
    let result = if input.commands.is_empty() {
        Ok(VerificationReport {
            commit: input.commit.clone(),
            tree: input.tree.clone(),
            executions: vec![],
        })
    } else {
        crate::verifier::verify_sealed(&crate::verifier::VerificationRequest {
            root: input.root.clone(),
            common_dir: input.common_dir.clone(),
            revision: input.commit.clone(),
            commands: input.commands.clone(),
            timeout,
            log_root: input.common_dir.join("towermd/cache/verification"),
            cancelled,
        })
        .and_then(|sealed| {
            if sealed.root() != input.root
                || sealed.common_dir() != input.common_dir
                || sealed.commands() != input.commands
                || sealed.report().commit != input.commit
                || sealed.report().tree != input.tree
            {
                return Err("sealed verifier differs from the exact finish ticket".into());
            }
            Ok(sealed.report().clone())
        })
    };
    ExecutedFinish {
        status: ticket.status,
        session: ticket.session,
        result,
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct FinishOutcome {
    pub status: FinishStatus,
    pub actual: Event,
    pub evidence: Option<FinishEvidence>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Started {
    pub task: Task,
    pub run: Run,
    pub input: FrozenFinish,
    pub review_context: Option<ReviewContext>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Completed {
    pub task: Task,
    pub run: Run,
    pub evidence: FinishEvidence,
    pub report: VerificationReport,
    pub hold: Value,
    pub review_context: Option<ReviewContext>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Transition {
    pub task: Task,
    pub run: Run,
    pub evidence_id: String,
    pub diagnostic: Option<String>,
}
pub(crate) fn is_event(kind: &str) -> bool {
    matches!(
        kind,
        "finish.started" | "finish.running" | "finish.completed" | "finish.interrupted"
    )
}
pub(crate) fn status_in(run: &Run, id: &str) -> Result<FinishStatus, String> {
    RunFinish::read(run)?
        .jobs
        .get(id)
        .cloned()
        .ok_or("finish job is not in the actual Run".into())
}
pub(crate) fn make_evidence(
    input: &FrozenFinish,
    eid: &str,
    report: &VerificationReport,
    checked: &BTreeMap<String, bool>,
) -> Result<FinishEvidence, String> {
    let result = if input.commands.is_empty() {
        EvidenceResult::NoTests
    } else if report
        .executions
        .iter()
        .all(|e| e.result == "passed" && e.exit_code == Some(0) && !e.timed_out)
    {
        EvidenceResult::Passed
    } else {
        EvidenceResult::Failed
    };
    let evidence = FinishEvidence {
        id: input.evidence_id.clone(),
        kind: "test".into(),
        task: input.task_id.clone(),
        run: input.run_id.clone(),
        module: input.module_id.clone(),
        commit: input.commit.clone(),
        tree: input.tree.clone(),
        base: input.base.clone(),
        authority: input.authority.clone(),
        baseline: input.baseline.clone(),
        plan_hash: input.plan_hash.clone(),
        commands_hash: input.commands_hash.clone(),
        fences: input.fences.clone(),
        result: result.clone(),
        generated_by: "process:towermd-verifier".into(),
        log: VerificationLogRef {
            report_eid: eid.into(),
            report_sha256: report_hash(report)?,
        },
        gates: {
            let mut gates = checked.clone();
            gates.insert("G3".into(), result == EvidenceResult::Passed);
            gates
        },
        extra: BTreeMap::from([("type".into(), json!("Evidence"))]),
    };
    contract::validate_finish_report(input, eid, &evidence, report)?;
    Ok(evidence)
}
pub(crate) fn gatefailed_hold(input: &FrozenFinish) -> Result<Value, String> {
    let key =
        contract::canonical_hash(&(input.project_id.as_str(), input.run_id.as_str(), "G3", 3u32))?;
    Ok(
        json!({"type":"Hold","id":format!("H-{key}"),"kind":"gatefailed","gate":"G3","task":input.task_id,"run":input.run_id,"baseline":input.baseline,"tree":input.tree,"fences":input.fences,"threshold":3}),
    )
}
fn validate_envelope(event: &Event, task: &Task, run: &Run) -> Result<(), String> {
    if task.object_type != "Task"
        || run.object_type != "Run"
        || event.actor != "process:towermd-verifier"
        || event.layer != "L4"
        || event.obj != task.spec.id
        || run.task != task.spec.id
        || event.host != run.host
        || event.guards != run.leases
        || event.epoch != event.guards.first().map(|f| f.epoch)
        || event.guards.is_empty()
    {
        return Err("finish envelope/registered Task/Run binding differs".into());
    }
    let expected = std::iter::once(&task.spec.module)
        .chain(task.spec.touches.iter())
        .map(|id| format!("module:{id}"))
        .collect::<BTreeSet<_>>();
    if expected != event.guards.iter().map(|f| f.name.clone()).collect()
        || expected.len() != event.guards.len()
        || event
            .guards
            .iter()
            .any(|f| f.holder != run.id || f.epoch == 0)
    {
        return Err("finish lacks complete module/touches fences".into());
    }
    Ok(())
}
/// Validate all objects before atomically changing the projection. Reports are
/// historical data here; only the producer's sealed execution can create them.
pub(crate) fn project(state: &mut StateProjection, event: &Event) -> Result<(), String> {
    if event.stale {
        return Ok(());
    }
    let (task, run, hold, evidence) = match event.kind.as_str() {
        "finish.started" => {
            let started: Started =
                serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            validate_envelope(event, &started.task, &started.run)?;
            let input = &started.input;
            for oid in [&input.commit, &input.tree, &input.base, &input.authority] {
                contract::object_id(oid)?;
            }
            if input.baseline.is_empty()
                || !input.root.is_absolute()
                || !input.common_dir.is_absolute()
            {
                return Err("finish frozen baseline/coordinates invalid".into());
            }
            let job = status_in(&started.run, &input.evidence_id)?;
            if job.review_context != started.review_context
                || ["G0", "G2", "G5"]
                    .iter()
                    .any(|g| job.checked_gates.get(*g) != Some(&true))
                || job.input != *input
                || job.started_eid != event.eid
                || job.phase != JobPhase::Queued
                || input.project_id.is_empty()
                || input.task_id != started.task.spec.id
                || input.run_id != started.run.id
                || input.module_id != started.task.spec.module
                || input.host != started.run.host
                || input.fences != started.run.leases
                || input.root.to_string_lossy() != started.run.worktree
                || fixed_deadline(input.entered_verifying_at)? != input.merge_deadline
                || started.task.state != "verifying"
                || started.run.state != "verifying"
                || contract::verification::commands_hash(&input.commands)? != input.commands_hash
            {
                return Err("finish.started differs from frozen exact input/lifecycle".into());
            }
            let current: Run = serde_json::from_value(
                state
                    .objects
                    .get("Run")
                    .and_then(|x| x.get(&input.run_id))
                    .cloned()
                    .ok_or("finish.started lacks original actual Run")?,
            )
            .map_err(|e| e.to_string())?;
            if current.state == "held"
                || state
                    .objects
                    .get("Task")
                    .and_then(|tasks| tasks.get(&input.task_id))
                    .is_some_and(|task| task["state"] == "held")
            {
                return Err("finish.started cannot cross the current Hold".into());
            }
            let old = RunFinish::read(&current)?;
            if old.jobs.contains_key(&input.evidence_id) {
                return if serde_json::to_value(&started.run).map_err(|e| e.to_string())?
                    == serde_json::to_value(current).map_err(|e| e.to_string())?
                {
                    Ok(())
                } else {
                    Err("finish started identity conflict".into())
                };
            }
            if old
                .jobs
                .values()
                .any(|job| matches!(job.phase, JobPhase::Queued | JobPhase::Running))
            {
                return Err("another finish verification is already active".into());
            }
            let mut expected = old;
            if expected.jobs.is_empty() {
                expected.entered_verifying_at = input.entered_verifying_at;
                expected.merge_deadline = input.merge_deadline;
            }
            if expected.entered_verifying_at != input.entered_verifying_at
                || expected.merge_deadline != input.merge_deadline
            {
                return Err("finish reset the original deadline".into());
            }
            expected.jobs.insert(input.evidence_id.clone(), job);
            let mut expected_run = current.clone();
            expected_run.state = "verifying".into();
            expected.put(&mut expected_run)?;
            if serde_json::to_value(expected_run).map_err(|e| e.to_string())?
                != serde_json::to_value(&started.run).map_err(|e| e.to_string())?
            {
                return Err("finish.started changed unrelated Run fields".into());
            }
            (started.task, started.run, Value::Null, None)
        }
        "finish.completed" => {
            let completed: Completed =
                serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            validate_envelope(event, &completed.task, &completed.run)?;
            let current: Run = serde_json::from_value(
                state
                    .objects
                    .get("Run")
                    .and_then(|x| x.get(&completed.run.id))
                    .cloned()
                    .ok_or("finish completion lacks actual Run")?,
            )
            .map_err(|e| e.to_string())?;
            let mut lifecycle = RunFinish::read(&current)?;
            let job = lifecycle
                .jobs
                .get(&completed.evidence.id)
                .cloned()
                .ok_or("completion lacks original started job")?;
            if completed.review_context != job.review_context {
                return Err("completion changed original G1 context".into());
            }
            if job.completed_eid != event.eid {
                return Err("completion changed preallocated original eid".into());
            }
            contract::validate_finish_report(
                &job.input,
                &event.eid,
                &completed.evidence,
                &completed.report,
            )?;
            if completed.evidence
                != make_evidence(
                    &job.input,
                    &event.eid,
                    &completed.report,
                    &job.checked_gates,
                )?
            {
                return Err("completion gate evidence differs from actual check".into());
            }
            if let Some(old) = state
                .objects
                .get("Evidence")
                .and_then(|x| x.get(&completed.evidence.id))
            {
                return if old == &json!(completed.evidence) {
                    Ok(())
                } else {
                    Err("conflicting immutable finish Evidence".into())
                };
            }
            if job.phase != JobPhase::Running {
                return Err("completion requires accepted Running job".into());
            }
            let started = state
                .objects
                .get("Run")
                .and_then(|_| lifecycle.jobs.get(&completed.evidence.id))
                .ok_or("missing finish job")?;
            if started.input.plan_hash.is_some() && completed.review_context.is_none() {
                return Err("completion lost original G1 context".into());
            }
            let mut expected_run = current;
            expected_run.state = "verifying".into();
            if completed.evidence.result != EvidenceResult::Passed {
                if !lifecycle
                    .consumed_failures
                    .insert(completed.evidence.id.clone())
                {
                    return Err("failure Evidence already consumed".into());
                }
                let (count, action) = failure_action(lifecycle.failures);
                lifecycle.failures = count;
                expected_run.state = if action == FailureAction::GatefailedHold {
                    "held"
                } else {
                    "running"
                }
                .into();
            }
            let wanted_hold = if expected_run.state == "held" {
                gatefailed_hold(&job.input)?
            } else {
                Value::Null
            };
            if completed.hold != wanted_hold || completed.task.state != expected_run.state {
                return Err("finish failure threshold/Hold state differs".into());
            }
            let status = lifecycle
                .jobs
                .get_mut(&completed.evidence.id)
                .expect("checked job");
            status.phase = JobPhase::Recording;
            lifecycle.put(&mut expected_run)?;
            if json!(expected_run) != json!(completed.run) {
                return Err("completion changed unrelated Run/lifecycle fields".into());
            }
            (
                completed.task,
                completed.run,
                completed.hold,
                Some(completed.evidence),
            )
        }
        "finish.running" | "finish.interrupted" => {
            let change: Transition =
                serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            validate_envelope(event, &change.task, &change.run)?;
            let current: Run = serde_json::from_value(
                state
                    .objects
                    .get("Run")
                    .and_then(|x| x.get(&change.run.id))
                    .cloned()
                    .ok_or("finish transition lacks actual Run")?,
            )
            .map_err(|e| e.to_string())?;
            let mut lifecycle = RunFinish::read(&current)?;
            let job = lifecycle
                .jobs
                .get_mut(&change.evidence_id)
                .ok_or("finish transition lacks original started job")?;
            let target = if event.kind == "finish.running" {
                JobPhase::Running
            } else {
                JobPhase::Interrupted
            };
            if (target == JobPhase::Running && job.phase != JobPhase::Queued)
                || (target == JobPhase::Interrupted
                    && !matches!(job.phase, JobPhase::Queued | JobPhase::Running))
                || (target == JobPhase::Interrupted
                    && change
                        .diagnostic
                        .as_ref()
                        .is_none_or(|d| d.trim().is_empty()))
            {
                return Err("invalid finish phase transition".into());
            }
            job.phase = target;
            job.diagnostic = change.diagnostic;
            let mut expected = current;
            expected.state = if event.kind == "finish.interrupted" {
                "running"
            } else {
                "verifying"
            }
            .into();
            lifecycle.put(&mut expected)?;
            if json!(expected) != json!(change.run) || change.task.state != expected.state {
                return Err("finish transition altered unrelated Run".into());
            }
            (change.task, change.run, Value::Null, None)
        }
        _ => return Err("not a finish lifecycle event".into()),
    };
    let mut expected_task: Task = serde_json::from_value(
        state
            .objects
            .get("Task")
            .and_then(|x| x.get(&task.spec.id))
            .cloned()
            .ok_or("finish lacks actual Task")?,
    )
    .map_err(|e| e.to_string())?;
    if expected_task.state == "merged" {
        return Err("finish cannot revive merged Task".into());
    }
    expected_task.state = task.state.clone();
    if json!(expected_task) != json!(task) {
        return Err("finish changed unrelated Task fields".into());
    }
    if !hold.is_null() {
        let id = hold["id"].as_str().ok_or("finish Hold lacks ID")?;
        if state
            .objects
            .get("Hold")
            .and_then(|x| x.get(id))
            .is_some_and(|old| old != &hold)
        {
            return Err("conflicting immutable gatefailed Hold".into());
        }
    }
    state
        .objects
        .entry("Task".into())
        .or_default()
        .insert(task.spec.id.clone(), json!(task));
    state
        .objects
        .entry("Run".into())
        .or_default()
        .insert(run.id.clone(), json!(run));
    if let Some(evidence) = evidence {
        state
            .objects
            .entry("Evidence".into())
            .or_default()
            .insert(evidence.id.clone(), json!(evidence));
    }
    if let Some(id) = hold["id"].as_str().map(str::to_owned) {
        state
            .objects
            .entry("Hold".into())
            .or_default()
            .insert(id, hold);
    }
    Ok(())
}
pub(crate) fn original_status(event: &NewEvent) -> Result<FinishStatus, String> {
    let start: Started = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if event.kind != "finish.started" {
        return Err("not original finish.started".into());
    }
    status_in(&start.run, &start.input.evidence_id)
}

// A typed gatefailed Hold has no Human opening and can grant no approval.
// The full snapshot provenance is proved before this finite view is used.
pub(crate) fn without_verification_holds(
    projection: &StateProjection,
) -> Result<StateProjection, String> {
    let mut human = projection.clone();
    if let Some(holds) = human.objects.get_mut("Hold") {
        let canonical = projection
            .human
            .as_ref()
            .ok_or("finish Human split requires canonical mode")?;
        let extra = holds
            .keys()
            .filter(|id| !canonical.openings.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        for id in extra {
            let raw = holds.get(&id).expect("selected Hold");
            let mut matched = false;
            for value in projection
                .objects
                .get("Run")
                .into_iter()
                .flat_map(|runs| runs.values())
            {
                let run: Run = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
                let finish = RunFinish::read(&run)?;
                if finish.failures < 3 {
                    continue;
                }
                for consumed in &finish.consumed_failures {
                    let job = finish
                        .jobs
                        .get(consumed)
                        .ok_or("consumed finish failure lacks immutable job")?;
                    let evidence = projection
                        .objects
                        .get("Evidence")
                        .and_then(|e| e.get(consumed))
                        .ok_or("gatefailed Hold lacks immutable Evidence")?;
                    if evidence["run"] != run.id
                        || evidence["log"]["report_eid"] != job.completed_eid
                        || !matches!(evidence["result"].as_str(), Some("failed" | "no_tests"))
                    {
                        return Err("gatefailed Hold Evidence binding differs".into());
                    }
                    if raw == &gatefailed_hold(&job.input)? {
                        matched = true;
                    }
                }
            }
            if !matched {
                return Err("unknown Hold outside actual Human/finish provenance".into());
            }
            holds.remove(&id);
        }
        if holds.is_empty() {
            human.objects.remove("Hold");
        }
    }
    Ok(human)
}
/// Rebuild authorization from the actual chronological Human prefix, never
/// from an embedded approval or a receipt-only historical role.
pub(crate) fn validate_review_at(
    catalog: &crate::catalog::Catalog,
    human: &crate::human_intervention::HumanState,
    seen: &BTreeMap<String, Event>,
    event: &Event,
) -> Result<(), String> {
    if event.stale || !matches!(event.kind.as_str(), "finish.started" | "finish.completed") {
        return Ok(());
    }
    let run: Run = serde_json::from_value(event.data["run"].clone()).map_err(|e| e.to_string())?;
    let id = if event.kind == "finish.started" {
        event.data["input"]["evidence_id"].as_str()
    } else {
        event.data["evidence"]["id"].as_str()
    }
    .ok_or("finish lacks Evidence identity")?;
    let status = status_in(&run, id)?;
    let input = &status.input;
    let source = crate::catalog::Catalog::at_revision(&catalog.root, &input.authority)?
        .ok_or("finish frozen authority unavailable")?;
    if source.config.project_id() != input.project_id || source.common_dir != catalog.common_dir {
        return Err("finish frozen authority project mismatch".into());
    }
    let mut high = false;
    for fence in &input.fences {
        let id = fence
            .name
            .strip_prefix("module:")
            .ok_or("finish has non-module fence")?;
        high |= source
            .objects
            .get(id)
            .filter(|m| m.doc_type() == Some("Module"))
            .ok_or("finish unknown authoritative Module")?
            .get_str("risk")
            == Some("high");
    }
    // Low risk does not require G1, but a voluntary context has the same
    // original Human/Gate proof obligations as a required high-risk context.
    if !high && status.review_context.is_none() && input.plan_hash.is_none() {
        return Ok(());
    }
    let review = status
        .review_context
        .as_ref()
        .ok_or("high finish lacks original G1 context")?;
    let subject = crate::human_intervention::ReviewSubject::Plan {
        sha256: input
            .plan_hash
            .clone()
            .ok_or("high finish lacks current Plan")?,
    };
    let binding = crate::human_intervention::ReviewBinding {
        run: input.run_id.clone(),
        task: input.task_id.clone(),
        baseline: input.baseline.clone(),
        fences: input.fences.clone(),
        subject: subject.clone(),
    };
    if human.current_plan_subject(&input.run_id)? != subject {
        return Err("finish Plan superseded at acceptance".into());
    }
    let (opening, reviewed) = human.review_originals(&binding)?;
    if opening != &review.opening
        || reviewed != &review.reviewed
        || review.gate != crate::human_intervention::ReviewGate::G1
        || seen.get(&review.gate_evidence.eid) != Some(&review.gate_evidence)
        || !(opening.seq < reviewed.seq
            && reviewed.seq < review.gate_evidence.seq
            && review.gate_evidence.seq < event.seq)
    {
        return Err("finish original G1 roles/order differ".into());
    }
    let gate: contract::recorded::GateEvidence =
        serde_json::from_value(review.gate_evidence.data["evidence"].clone())
            .map_err(|e| e.to_string())?;
    contract::validate_gate_layout(&gate)?;
    let gate_id = gate.id.strip_prefix("E-").ok_or("gate lacks UUID")?;
    if review.gate_evidence.actor != "process:towermd-gates"
        || review.gate_evidence.host != input.host
        || review.gate_evidence.kind != "evidence.recorded"
        || review.gate_evidence.layer != "L5"
        || review.gate_evidence.obj != gate.id
        || review.gate_evidence.guards != input.fences
        || review.gate_evidence.stale
        || review.gate_evidence.eid
            != contract::recorded::request_event_eid(gate_id, "evidence.recorded")?
    {
        return Err("finish G1 gate envelope differs".into());
    }
    if gate.project_id != input.project_id
        || gate.task != input.task_id
        || gate.run != input.run_id
        || gate.module != input.module_id
        || gate.commit.as_ref() != Some(&input.commit)
        || gate.tree.as_ref() != Some(&input.tree)
        || gate.base.as_ref() != Some(&input.base)
        || gate.authority.as_ref() != Some(&input.authority)
        || gate.baseline.as_ref() != Some(&input.baseline)
        || gate.plan_hash != input.plan_hash
        || gate.commands_hash.as_ref() != Some(&input.commands_hash)
        || gate.fences != input.fences
        || gate.plan_opening_eid.as_ref() != Some(&opening.eid)
        || gate.result != contract::recorded::GateResult::Passed
        || gate.gate != crate::task_gates::Gate::G1
        || gate.review
            != Some(contract::recorded::GateReviewLink {
                opening: opening.clone(),
                reviewed: reviewed.clone(),
                completed_eid: None,
            })
    {
        return Err("finish G1 gate differs from actual exact input".into());
    }
    Ok(())
}
/// Recovery of a locally unaccepted completion requires the real, still sealed
/// local execution artifacts. Already accepted history never needs these paths.
pub(crate) fn check_sealed_logs(
    input: &FrozenFinish,
    report: &VerificationReport,
) -> Result<(), String> {
    if input.commands.is_empty() {
        if !report.executions.is_empty() {
            return Err("NoTests has unexpected execution logs".into());
        }
        return Ok(());
    }
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let root = input.common_dir.join("towermd/cache/verification");
    let mut directory = None;
    for (index, execution) in report.executions.iter().enumerate() {
        for (stream, locator, wanted) in [
            ("stdout", &execution.stdout_log, &execution.stdout_sha256),
            ("stderr", &execution.stderr_log, &execution.stderr_sha256),
        ] {
            let path = std::path::Path::new(locator);
            let parent = path.parent().ok_or("sealed log lacks directory")?;
            if parent.parent() != Some(root.as_path())
                || path.file_name().and_then(|n| n.to_str())
                    != Some(format!("{index}-{stream}.log").as_str())
                || std::fs::canonicalize(path).map_err(|e| e.to_string())? != path
                || !std::fs::symlink_metadata(path)
                    .map_err(|e| e.to_string())?
                    .is_file()
            {
                return Err(
                    "finish sealed log is outside its exact repository execution directory".into(),
                );
            }
            if directory.as_ref().is_some_and(|known| known != parent) {
                return Err("finish logs mix executions".into());
            }
            directory = Some(parent.to_path_buf());
            let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 8192];
            loop {
                let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            if hex::encode(hash.finalize()) != *wanted {
                return Err("finish sealed log hash changed".into());
            }
        }
    }
    let directory = directory.ok_or("nonempty verification lacks sealed logs")?;
    if std::fs::read(directory.join("report.json")).map_err(|e| e.to_string())?
        != contract::canonical_json_bytes(report)?
        || std::fs::read_to_string(directory.join("report.sha256")).map_err(|e| e.to_string())?
            != format!("{}\n", report_hash(report)?)
    {
        return Err("finish sealed report bytes/hash changed".into());
    }
    Ok(())
}
