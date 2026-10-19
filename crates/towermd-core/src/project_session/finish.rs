//! Finite finish producer; no report supplied by a client can enter this path.
mod frozen_review;
mod integration;
mod maintenance;
use super::{ProjectSession, SnapshotSessionId};
use crate::{
    baseline_current::current_baseline,
    catalog::Catalog,
    finish::{
        self, Completed, ExecutedFinish, ExecutionTicket, FinishOutcome, FinishStart, FinishStatus,
        FinishTicket, FrozenFinish, JobPhase, RunFinish, Started, Transition,
    },
    hub_protocol::{AppendReceipt, Event, HubError, NewEvent},
    human_intervention::{ReviewBinding, ReviewGate, ReviewSubject},
    recovery_contract::{
        self as contract,
        recorded::{
            request_event_eid, GateEvidence, GateGenerated, GateResult, GateReviewLink,
            ReviewContext,
        },
        verification::VerificationCommand,
    },
    recovery_outbox::{self, RecoveryInputs, RecoveryOutbox},
    task_gates::{self, CheckReport, Gate},
    tasks::{Run, Task},
};
pub(super) use frozen_review::{prove_frozen_g4_subject, FrozenG4Error, FrozenG4Subject};
pub use integration::{MainMergeDiagnostic, MainObservation, TaskTrailersOutcome};
pub use maintenance::{FinishExpiry, FinishExpiryOutcome, FinishMaintenance};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Mutex};

fn uncertain(id: &str, reason: impl std::fmt::Display) -> String {
    crate::request_ledger::uncertain_request(id, reason)
}
fn commands(catalog: &Catalog, module: &str) -> Result<Vec<VerificationCommand>, String> {
    let module = catalog
        .objects
        .get(module)
        .filter(|o| o.doc_type() == Some("Module"))
        .ok_or("finish requires authoritative Module")?;
    let approved = module
        .get("verify")
        .cloned()
        .unwrap_or(serde_yaml::Value::Sequence(vec![]));
    let entries = approved
        .as_sequence()
        .ok_or("authoritative Module.verify must be an approved command list")?;
    let mut commands = Vec::new();
    for entry in entries {
        if let Some(command) = entry.as_str() {
            commands.push(VerificationCommand {
                cwd: ".".into(),
                argv: vec!["/bin/sh".into(), "-c".into(), command.into()],
            });
        } else {
            let approved: crate::baseline_draft::VerifyCommand =
                serde_yaml::from_value(entry.clone())
                    .map_err(|e| format!("invalid approved typed verify command: {e}"))?;
            commands.push(VerificationCommand {
                cwd: approved.cwd.into(),
                argv: approved.argv,
            });
        }
    }
    crate::verifier::validate_commands(&commands)?;
    Ok(commands)
}
fn high_risk(catalog: &Catalog, task: &Task) -> Result<bool, String> {
    let mut high = false;
    for id in std::iter::once(&task.spec.module).chain(task.spec.touches.iter()) {
        high |= catalog
            .objects
            .get(id)
            .filter(|o| o.doc_type() == Some("Module"))
            .ok_or("finish references missing authoritative Module")?
            .get_str("risk")
            == Some("high");
    }
    Ok(high)
}
fn actual_context(
    live: &ProjectSession,
    catalog: &Catalog,
    host: &str,
    run: Option<&str>,
    now: u64,
) -> Result<(Task, Run), String> {
    live.projection()?;
    let (task, run) = live.tasks.finish_context(catalog, host, run)?;
    crate::tasks::validate_task(&task.spec, catalog)?;
    if task.state == "merged"
        || !matches!(task.state.as_str(), "running" | "verifying" | "held")
        || run.branch != format!("t/{}", task.spec.id)
    {
        return Err("finish requires exact active Task branch".into());
    }
    if run.leases.iter().any(|f| {
        live.head
            .leases
            .get(&f.name)
            .is_none_or(|l| l.fence() != *f || l.expires_at <= now)
    }) {
        return Err("finish requires complete current fences".into());
    }
    Ok((task, run))
}
fn capture(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    host: &str,
    run: Option<&str>,
    now: u64,
) -> Result<(Task, Run, SnapshotSessionId), String> {
    ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
        actual_context(live, catalog, host, run, now)
            .map(|(task, run)| (task, run, live.snapshot_session_id()))
    })
    .map_err(|e| e.to_string())?
}
fn checked(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    host: &str,
    run: Option<&str>,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<(Task, Run, SnapshotSessionId, CheckReport, u64), String> {
    checked_at(session, catalog, inputs, host, run, clock, None)
}
fn checked_frozen(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    status: &FinishStatus,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<(Task, Run, SnapshotSessionId, CheckReport, u64), String> {
    let result = checked_at(
        session,
        catalog,
        inputs,
        &status.input.host,
        Some(&status.input.run_id),
        clock,
        Some(&status.input),
    )?;
    let (task, run, _, report, _) = &result;
    let input = &status.input;
    if input.project_id != catalog.config.project_id()
        || input.root != catalog.root
        || input.common_dir != catalog.common_dir
        || input.task_id != task.spec.id
        || input.run_id != run.id
        || input.module_id != task.spec.module
        || input.fences != run.leases
        || input.commit != report.commit
        || input.tree != report.tree
        || input.base != report.base
        || input.commands != commands(catalog, &task.spec.module)?
    {
        return Err("finish frozen inputs differ from the actual current check".into());
    }
    Ok(result)
}
fn checked_at(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    host: &str,
    run: Option<&str>,
    clock: &impl Fn() -> Result<u64, String>,
    frozen: Option<&FrozenFinish>,
) -> Result<(Task, Run, SnapshotSessionId, CheckReport, u64), String> {
    let now = clock()?;
    let (task, run, identity) = capture(session, catalog, inputs, host, run, now)?;
    let leases = session
        .lock()
        .map_err(|_| "session lock poisoned")?
        .head
        .leases
        .clone();
    let report = match frozen.filter(|frozen| frozen.authority != catalog.revision) {
        Some(frozen) => task_gates::check_frozen(catalog, &task, &run, host, &leases, now, frozen),
        None => task_gates::check(catalog, &task, &run, host, &leases, now),
    }
    .map_err(|e| format!("finish check: {e:?}"))?;
    let end = clock()?;
    if end < now {
        return Err("finish clock moved backwards during check".into());
    }
    let (task2, run2, identity2) = capture(session, catalog, inputs, host, Some(&run.id), end)?;
    if identity != identity2 || json!((&task, &run)) != json!((&task2, &run2)) {
        return Err("finish context changed during check".into());
    }
    if ["G0", "G2", "G5"]
        .iter()
        .any(|g| report.gates.get(*g) != Some(&true))
    {
        return Err("finish lacks actual G0/G2/G5 check".into());
    }
    Ok((task, run, identity, report, end))
}
fn binding(input: &FrozenFinish, gate: ReviewGate) -> Result<ReviewBinding, String> {
    Ok(ReviewBinding {
        run: input.run_id.clone(),
        task: input.task_id.clone(),
        baseline: input.baseline.clone(),
        fences: input.fences.clone(),
        subject: match gate {
            ReviewGate::G1 => ReviewSubject::Plan {
                sha256: input
                    .plan_hash
                    .clone()
                    .ok_or("ReviewBlocked G1: current Plan missing")?,
            },
            ReviewGate::G4 => ReviewSubject::Tree {
                tree: input.tree.clone(),
            },
        },
    })
}
fn require_reviews(
    live: &ProjectSession,
    catalog: &Catalog,
    task: &Task,
    input: &FrozenFinish,
    gate: ReviewGate,
) -> Result<Option<(Event, Event)>, String> {
    if !high_risk(catalog, task)? {
        return Ok(None);
    }
    let human = live.human()?;
    let plan = human
        .current_plan_subject(&input.run_id)
        .map_err(|e| format!("ReviewBlocked G1: {e}"))?;
    if plan
        != (ReviewSubject::Plan {
            sha256: input
                .plan_hash
                .clone()
                .ok_or("ReviewBlocked G1: Plan missing")?,
        })
    {
        return Err("ReviewBlocked G1: current Plan changed".into());
    }
    let binding = binding(input, gate)?;
    let (opening, reviewed) = human
        .review_originals(&binding)
        .map_err(|e| format!("ReviewBlocked: {e}"))?;
    Ok(Some((opening.clone(), reviewed.clone())))
}
fn require_same_review(
    live: &ProjectSession,
    catalog: &Catalog,
    task: &Task,
    status: &FinishStatus,
) -> Result<(), String> {
    let actual = require_reviews(live, catalog, task, &status.input, ReviewGate::G1)?;
    match (&status.review_context, actual) {
        (None, None) => Ok(()),
        (Some(expected), Some((opening, reviewed)))
            if expected.opening == opening && expected.reviewed == reviewed =>
        {
            Ok(())
        }
        _ => Err("ReviewBlocked G1: original opening/answer was superseded".into()),
    }
}
fn gate_uuid(request_id: &str) -> Result<String, String> {
    let original = uuid::Uuid::parse_str(request_id).map_err(|e| e.to_string())?;
    let mut bytes = *original.as_bytes();
    let hash = contract::sha256(format!("towermd:finish:g1:{request_id}").as_bytes());
    for (index, slot) in bytes.iter_mut().enumerate().skip(6) {
        *slot =
            u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    bytes[6] = (bytes[6] & 15) | 0x70;
    bytes[8] = (bytes[8] & 63) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}
// The source-only Git reader deliberately has no live tail. Capture it only
// from the real synchronized Session, feed the existing sole proof reader,
// then reject a changed source/Session/head before consuming its result.
fn same_history_prefix(before: &super::SessionCapture, after: &super::SessionCapture) -> bool {
    if before == after {
        return true;
    }
    if before.restored
        || after.restored
        || !before.fresh.is_empty()
        || before.base != after.base
        || before.head.floor_seq != after.head.floor_seq
        || before.head.ack != after.head.ack
        || before.head.epoch_high_water != after.head.epoch_high_water
        || before.head.contiguous_seq != before.cursor
        || after.cursor < before.cursor
        || after.cursor > after.head.contiguous_seq
        || after.previous_head.contiguous_seq < before.cursor
        || after.previous_head.contiguous_seq > after.head.contiguous_seq
        || after.previous_head.floor_seq != before.head.floor_seq
        || after.previous_head.ack != before.head.ack
        || after.previous_head.epoch_high_water != before.head.epoch_high_water
        || !after.tail.starts_with(&before.tail)
    {
        return false;
    }
    let suffix = &after.tail[before.tail.len()..];
    if after.fresh
        != suffix
            .iter()
            .filter(|event| event.seq > after.cursor)
            .cloned()
            .collect::<Vec<_>>()
    {
        return false;
    }
    super::renewal_suffix_matches(
        &before.head,
        suffix,
        &after.previous_head,
        &after.head,
        None,
    )
}
// A read-once history for one finite, still-unwritten request. Neither the
// serialized Job nor prepared_key can create this scope. Any business/source
// change falls back to the original reader; actual probe errors remain errors.
struct FinishReadScope {
    request_id: Option<String>,
    root: std::path::PathBuf,
    common_dir: std::path::PathBuf,
    authority: String,
    configuration: String,
    inputs: RecoveryInputs,
    session: SnapshotSessionId,
    captured: super::SessionCapture,
    recorded: super::RecordedPrefix,
    history: crate::recovery_git::VerifiedRecoveryHistory,
}
impl FinishReadScope {
    fn matches(
        &self,
        session: &Mutex<ProjectSession>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        request_id: &str,
        now: u64,
    ) -> Result<bool, String> {
        if self.request_id.as_deref() != Some(request_id)
            || self.root != catalog.root
            || self.common_dir != catalog.common_dir
            || self.authority != catalog.revision
            || self.configuration != catalog.config.to_yaml()
            || self.inputs.validate_task_writes(inputs, &[]).is_err()
        {
            return Ok(false);
        }
        // This is still the normal actual source/protocol preparation. The
        // stored digest alone never certifies that physical inputs stayed put.
        ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
            let current = live.capture(now).map_err(|e| e.to_string())?;
            if live.snapshot_session_id() != self.session
                || live.projection != self.recorded.projection
                || live.tasks.pending_claim != self.recorded.pending
                || live.verified_sources != self.recorded.sources
                || live.prepared_head.as_ref() != Some(&current.head)
                || !same_history_prefix(&self.captured, &current)
            {
                return Ok(false);
            }
            let Some((_, private, _, drafts)) = &live.verified_sources else {
                return Ok(false);
            };
            let key = ProjectSession::key(&current, catalog, inputs, private, drafts)
                .map_err(|e| e.to_string())?;
            Ok(live.prepared_key.as_ref() == Some(&key))
        })
        .map_err(|e| e.to_string())?
    }
}
struct FinishLookup {
    status: Option<FinishStatus>,
    unused_history: Option<FinishReadScope>,
    historical: Option<finish::FinishView>,
    expiry_actual: Option<Event>,
}
fn history_shared(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    now: u64,
) -> Result<crate::recovery_git::VerifiedRecoveryHistory, String> {
    history_scope_shared(session, catalog, inputs, now, None).map(|scope| scope.history)
}
fn history_scope_shared(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    now: u64,
    request_id: Option<&str>,
) -> Result<FinishReadScope, String> {
    let (identity, captured) =
        ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
            live.capture(now)
                .map(|capture| (live.snapshot_session_id(), capture))
        })
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let history = crate::recovery_git::prepare_session_history(catalog, inputs, &captured)?;
    let sealed = ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
        live.capture(now).map(|current| {
            if live.snapshot_session_id() == identity && same_history_prefix(&captured, &current) {
                Some((current, super::RecordedPrefix::new(live)))
            } else {
                None
            }
        })
    })
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let (captured, recorded) =
        sealed.ok_or("finish actual history capture changed during proof")?;
    Ok(FinishReadScope {
        request_id: request_id.map(str::to_owned),
        root: catalog.root.clone(),
        common_dir: catalog.common_dir.clone(),
        authority: catalog.revision.clone(),
        configuration: catalog.config.to_yaml(),
        inputs: inputs.clone(),
        session: identity,
        captured,
        recorded,
        history,
    })
}
fn claim_dependency(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    task: &Task,
    run: &Run,
    now: u64,
) -> Result<(), String> {
    let history = history_shared(session, catalog, inputs, now)?;
    claim_dependency_in_history(catalog, inputs, task, run, &history)
}
fn claim_dependency_in_history(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    task: &Task,
    run: &Run,
    history: &crate::recovery_git::VerifiedRecoveryHistory,
) -> Result<(), String> {
    let through = history
        .snapshot_sources()
        .clone()
        .select()?
        .map_or(0, |saved| saved.snapshot.through_seq);
    let actual = history.actual_outer_events(catalog)?;
    let claims = actual
        .iter()
        .filter(|e| !e.stale && e.kind == "task.claimed" && e.data["run"]["id"] == run.id)
        .collect::<Vec<_>>();
    if claims.len() != 1 {
        return Err("finish requires one actual original Task claim".into());
    }
    let claim = *claims[0];
    crate::snapshot::recorded_run(claim, &claim.data["run"])?;
    let mut original_task: Task =
        serde_json::from_value(claim.data["task"].clone()).map_err(|e| e.to_string())?;
    original_task.state = task.state.clone();
    if json!(original_task) != json!(task)
        || claim.data["run"]["leases"] != json!(run.leases)
        || claim.data["run"]["host"] != run.host
        || claim.data["run"]["worktree"] != run.worktree
        || claim.data["run"]["branch"] != run.branch
        || claim.actor != "process:towermd-cli"
        || claim.layer != "L5"
    {
        return Err("finish differs from the actual complete claim".into());
    }
    if claim.seq > through && !inputs.receipt_confirmed(claim) {
        return Err("finish original claim confirmation is uncertain".into());
    }
    Ok(())
}
fn completed_data(
    mut task: Task,
    mut run: Run,
    status: &FinishStatus,
    report: crate::recovery_contract::verification::VerificationReport,
) -> Result<Completed, String> {
    let mut lifecycle = RunFinish::read(&run)?;
    if lifecycle.jobs.get(&status.input.evidence_id) != Some(status)
        || status.phase != JobPhase::Running
    {
        return Err("finish Running identity changed".into());
    }
    let evidence = finish::make_evidence(
        &status.input,
        &status.completed_eid,
        &report,
        &status.checked_gates,
    )?;
    let mut hold = Value::Null;
    run.state = "verifying".into();
    if evidence.result != finish::EvidenceResult::Passed {
        if !lifecycle.consumed_failures.insert(evidence.id.clone()) {
            return Err("failure Evidence already consumed".into());
        }
        let (next, action) = finish::failure_action(lifecycle.failures);
        lifecycle.failures = next;
        run.state = if action == finish::FailureAction::GatefailedHold {
            hold = finish::gatefailed_hold(&status.input)?;
            "held"
        } else {
            "running"
        }
        .into();
    }
    lifecycle
        .jobs
        .get_mut(&status.input.evidence_id)
        .ok_or("missing Running job")?
        .phase = JobPhase::Recording;
    lifecycle.put(&mut run)?;
    task.state = run.state.clone();
    Ok(Completed {
        task,
        run,
        evidence,
        report,
        hold,
        review_context: status.review_context.clone(),
    })
}
fn after_running(inputs: &RecoveryInputs, status: &FinishStatus, now: u64) -> Result<(), String> {
    if now < status.input.entered_verifying_at {
        return Err(uncertain(
            &status.request_id,
            "finish clock moved backwards",
        ));
    }
    let running_eid = request_event_eid(&status.request_id, "finish.running")?;
    if let Some(actual) = inputs.confirmed().iter().find(|e| e.eid == running_eid) {
        let at = chrono::DateTime::parse_from_rfc3339(&actual.t)
            .map_err(|e| e.to_string())?
            .timestamp_millis();
        if at < 0 || now < (at as u64) {
            return Err(uncertain(
                &status.request_id,
                "finish clock moved backwards after Running",
            ));
        }
    }
    Ok(())
}
// Only a real live check can create this final-CAS observation. It is not a
// receipt or an execution permission and cannot be deserialized by a caller.
struct PendingProof {
    task: Task,
    run: Run,
    session: SnapshotSessionId,
    now: u64,
}
impl PendingProof {
    fn final_check(
        &self,
        live: &ProjectSession,
        catalog: &Catalog,
        status: &FinishStatus,
        now: u64,
    ) -> Result<(), String> {
        if now < self.now || now >= status.input.merge_deadline {
            return Err(uncertain(
                &status.request_id,
                "pending finish final clock/deadline changed",
            ));
        }
        let (task, run) = actual_context(
            live,
            catalog,
            &status.input.host,
            Some(&status.input.run_id),
            now,
        )?;
        if self.session != live.snapshot_session_id()
            || json!((&self.task, &self.run)) != json!((&task, &run))
        {
            return Err(uncertain(
                &status.request_id,
                "pending finish final Task/Run capture changed",
            ));
        }
        require_same_review(live, catalog, &task, status)
    }
}
fn pending_proof(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &RecoveryOutbox,
    status: &FinishStatus,
    original: &NewEvent,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<Option<PendingProof>, String> {
    let inputs = outbox.read_inputs()?;
    if inputs
        .confirmed()
        .iter()
        .any(|e| recovery_outbox::original_of(e) == *original)
    {
        return Ok(None);
    }
    let captured_at = clock().map_err(|e| uncertain(&status.request_id, e))?;
    let history = history_shared(session, catalog, &inputs, captured_at)?;
    let actual = history.actual_outer_events(catalog)?;
    if let Some(event) = actual.into_iter().find(|e| e.eid == original.eid) {
        recovery_outbox::validate_actual(original, event)?;
        return Ok(None);
    }
    // Raw pending is intent, not an execution capability. Reprove its actual
    // claim/current guards and, for completion, real sealed verifier artifacts.
    let (task, run, identity, report, now) =
        checked_frozen(session, catalog, &inputs, status, clock)?;
    if catalog.root != status.input.root
        || catalog.common_dir != status.input.common_dir
        || report.commit != status.input.commit
        || report.tree != status.input.tree
        || report.base != status.input.base
        || commands(catalog, &task.spec.module)? != status.input.commands
        || run.leases != status.input.fences
        || now >= status.input.merge_deadline
    {
        return Err(uncertain(
            &status.request_id,
            "pending finish no longer matches actual frozen input",
        ));
    }
    if original.kind == "finish.started" && (task.state == "held" || run.state == "held") {
        return Err("pending finish.started cannot cross the current Hold".into());
    }
    if original.kind == "finish.started" {
        let persisted = history
            .snapshot_sources()
            .clone()
            .select()?
            .map_or(0, |saved| saved.snapshot.through_seq);
        let actual = history.actual_outer_events(catalog)?;
        for old in RunFinish::read(&run)?
            .jobs
            .values()
            .filter(|job| job.phase == JobPhase::Recording)
        {
            let receipt = actual
                .iter()
                .find(|event| event.eid == old.completed_eid && !event.stale)
                .ok_or_else(|| {
                    uncertain(
                        &status.request_id,
                        "prior finish completion lacks actual original receipt",
                    )
                })?;
            if receipt.seq > persisted && !inputs.receipt_confirmed(receipt) {
                return Err(uncertain(
                    &status.request_id,
                    "prior finish original confirmation is still uncertain",
                ));
            }
        }
    }
    let baseline =
        current_baseline(catalog, &inputs)?.ok_or("pending finish has no approved baseline")?;
    if baseline.baseline != status.input.baseline {
        return Err(uncertain(
            &status.request_id,
            "pending finish baseline changed",
        ));
    }
    claim_dependency(session, catalog, &inputs, &task, &run, now)?;
    ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        require_same_review(live, catalog, &task, status)
    })
    .map_err(|e| uncertain(&status.request_id, e))??;
    if original.kind == "finish.completed" {
        let completed: Completed =
            serde_json::from_value(original.data.clone()).map_err(|e| e.to_string())?;
        let current = finish::status_in(&run, &status.input.evidence_id)?;
        if current.phase != JobPhase::Running
            || current.input != status.input
            || completed.review_context != current.review_context
        {
            return Err(uncertain(
                &status.request_id,
                "pending completion lacks original current Running context",
            ));
        }
        contract::validate_finish_report(
            &current.input,
            &original.eid,
            &completed.evidence,
            &completed.report,
        )?;
        if completed.evidence
            != finish::make_evidence(
                &current.input,
                &original.eid,
                &completed.report,
                &current.checked_gates,
            )?
        {
            return Err("pending completion differs from actual check".into());
        }
        finish::check_sealed_logs(&current.input, &completed.report)
            .map_err(|e| uncertain(&status.request_id, e))?;
        after_running(&inputs, &current, now)?;
        if json!(completed_data(
            task.clone(),
            run.clone(),
            &current,
            completed.report.clone()
        )?) != original.data
        {
            return Err("pending completion changed immutable Task/Run/result bytes".into());
        }
    }
    Ok(Some(PendingProof {
        task,
        run,
        session: identity,
        now,
    }))
}
fn read_original(inputs: &RecoveryInputs, eid: &str) -> Result<Option<NewEvent>, String> {
    let mut found = None;
    for original in inputs
        .pending()
        .iter()
        .filter(|e| e.eid == eid)
        .cloned()
        .chain(
            inputs
                .confirmed()
                .iter()
                .filter(|e| e.eid == eid)
                .map(recovery_outbox::original_of),
        )
    {
        if found.as_ref().is_some_and(|old| old != &original) {
            return Err("finish original identity conflict".into());
        }
        found = Some(original);
    }
    Ok(found)
}
fn known_original(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    eid: &str,
    now: u64,
) -> Result<Option<NewEvent>, String> {
    if let Some(original) = read_original(inputs, eid)? {
        return Ok(Some(original));
    }
    let history = history_shared(session, catalog, inputs, now)?;
    let mut found = None;
    for actual in history
        .actual_outer_events(catalog)?
        .into_iter()
        .filter(|e| e.eid == eid)
    {
        let original = recovery_outbox::original_of(actual);
        if found.as_ref().is_some_and(|old| old != &original) {
            return Err("conflicting actual finish original body".into());
        }
        found = Some(original);
    }
    Ok(found)
}
// Only the two new, live producers below can request this scope. It is neither
// deserializable nor used by original-request repair, completion, or Human/G1.
#[derive(Clone, Copy)]
enum OwnFinishPhase {
    Started,
    Running,
}
impl OwnFinishPhase {
    fn kind(self) -> &'static str {
        match self {
            Self::Started => "finish.started",
            Self::Running => "finish.running",
        }
    }
}
struct OwnFinishAppend {
    phase: OwnFinishPhase,
    authority: String,
    original: NewEvent,
    identity: SnapshotSessionId,
    before: super::RecordedPrefix,
    tasks: crate::tasks::TaskService,
    human: crate::human_intervention::HumanState,
}
impl OwnFinishAppend {
    fn capture(
        live: &ProjectSession,
        phase: OwnFinishPhase,
        input: &FrozenFinish,
        original: &NewEvent,
    ) -> Option<Self> {
        if original.kind != phase.kind() || live.prepared_key.is_none() {
            return None;
        }
        Some(Self {
            phase,
            authority: input.authority.clone(),
            original: original.clone(),
            identity: live.snapshot_session_id(),
            before: super::RecordedPrefix::new(live),
            tasks: live.tasks.clone(),
            human: live.human.clone(),
        })
    }
    // Any mismatch only declines reuse. The ordinary full synchronization below
    // remains responsible for its original diagnostics and source/CAS refusal.
    fn install(
        self,
        session: &Mutex<ProjectSession>,
        catalog: &Catalog,
        before_inputs: &RecoveryInputs,
        after_inputs: &RecoveryInputs,
        actual: &Event,
        now: u64,
    ) -> Result<bool, String> {
        if actual.stale || actual.legacy || !after_inputs.receipt_confirmed(actual) {
            return Ok(false);
        }
        recovery_outbox::validate_actual(&self.original, actual)?;
        before_inputs.validate_task_writes(after_inputs, &[(&self.original, Some(actual))])?;
        let Some((revision, private, digest, drafts)) = &self.before.sources else {
            return Ok(false);
        };
        if revision != &catalog.revision
            || digest != before_inputs.digest()
            || self.before.proof_key.as_ref()
                != Some(&ProjectSession::key(
                    &self.before.session,
                    catalog,
                    before_inputs,
                    private,
                    drafts,
                )?)
        {
            return Ok(false);
        }
        ProjectSession::probe_actual_binding(catalog, after_inputs, private)?;
        if crate::snapshot_service::draft_ref_fingerprint(catalog)
            .map_err(|error| error.to_string())?
            != *drafts
        {
            return Ok(false);
        }
        // Read the actual frozen Catalog, including protected blobs, rather than
        // treating equal revision text as proof of physical source availability.
        if matches!(self.phase, OwnFinishPhase::Started) {
            let seen = self
                .before
                .session
                .tail
                .iter()
                .map(|event| (event.eid.clone(), event.clone()))
                .collect();
            finish::validate_review_at(catalog, &self.human, &seen, actual)?;
        } else {
            let frozen = Catalog::at_revision(&catalog.root, &self.authority)?
                .ok_or("finish frozen authority unavailable")?;
            if frozen.root != catalog.root
                || frozen.common_dir != catalog.common_dir
                || frozen.config.project_id() != catalog.config.project_id()
            {
                return Ok(false);
            }
        }
        if self.authority != catalog.revision {
            let current = Catalog::at_revision(&catalog.root, &catalog.revision)?
                .ok_or("finish current authority unavailable")?;
            if current.root != catalog.root
                || current.common_dir != catalog.common_dir
                || current.config.to_yaml() != catalog.config.to_yaml()
            {
                return Ok(false);
            }
        }
        let capture = {
            let mut live = session.lock().map_err(|_| "session lock poisoned")?;
            live.bind_catalog(catalog)?;
            if live.snapshot_session_id() != self.identity || !self.before.business_matches(&live) {
                return Ok(false);
            }
            live.capture(now)?
        };
        let recorded = &self.before.session;
        if capture.fresh.is_empty()
            || capture.restored
            || capture.cursor != recorded.cursor
            || recorded.head != self.before.observed_head
            || capture.previous_head.floor_seq != recorded.head.floor_seq
            || capture.previous_head.ack != recorded.head.ack
            || capture.previous_head.epoch_high_water != recorded.head.epoch_high_water
            || recorded.head.contiguous_seq != recorded.cursor
            || capture.base != recorded.base
            || capture.head.floor_seq != recorded.head.floor_seq
            || capture.head.ack != recorded.head.ack
            || capture.head.epoch_high_water != recorded.head.epoch_high_water
            || !capture.tail.starts_with(&recorded.tail)
            || capture.tail[recorded.tail.len()..] != capture.fresh
            || capture
                .fresh
                .iter()
                .filter(|event| *event == actual)
                .count()
                != 1
        {
            return Ok(false);
        }
        let mut system = crate::hub::SystemState::from_checkpoint(&crate::hub::SystemCheckpoint {
            floor_seq: recorded.head.floor_seq,
            epoch_high_water: recorded.head.epoch_high_water,
            leases: recorded.head.leases.clone(),
            receipts: Default::default(),
            ack: recorded.head.ack.clone(),
            checkpoint: json!({}),
            legacy_through_seq: 0,
        })
        .map_err(|error| error.to_string())?;
        let mut tasks = self.tasks;
        let mut projection = self.before.projection.clone();
        let mut through = recorded.cursor;
        let mut previous_matches = capture.previous_head.contiguous_seq == through
            && capture.previous_head.leases == system.leases;
        for event in &capture.fresh {
            if event.legacy
                || event.stale
                || through.checked_add(1) != Some(event.seq)
                || (event != actual
                    && !matches!(
                        event.lease_delta,
                        Some(crate::hub::SystemDelta::Renew { .. })
                    ))
            {
                return Ok(false);
            }
            // The actual Hub verifier validates finite renewal metadata and the
            // complete guards/epoch/stale role of our exact business receipt.
            system.replay(event, 0).map_err(|error| error.to_string())?;
            ProjectSession::apply_prepared_event(&mut tasks, &mut projection, event)?;
            through = event.seq;
            if through == capture.previous_head.contiguous_seq {
                previous_matches = capture.previous_head.leases == system.leases
                    && capture.previous_head.epoch_high_water == system.epoch;
            }
        }
        if !previous_matches
            || through != capture.head.contiguous_seq
            || system.leases != capture.head.leases
            || system.epoch != capture.head.epoch_high_water
        {
            return Ok(false);
        }
        // No Git/source read is performed under the Session lock. Recheck the
        // exact physical sources outside it, then the same capture/business CAS.
        ProjectSession::probe_actual_binding(catalog, after_inputs, private)?;
        if crate::snapshot_service::draft_ref_fingerprint(catalog)
            .map_err(|error| error.to_string())?
            != *drafts
        {
            return Ok(false);
        }
        let private = private.clone();
        let drafts = drafts.clone();
        let mut live = session.lock().map_err(|_| "session lock poisoned")?;
        live.bind_catalog(catalog)?;
        if live.snapshot_session_id() != self.identity
            || !self.before.business_matches(&live)
            || live.capture(now)? != capture
        {
            return Ok(false);
        }
        let delta = super::PreparedTaskDelta {
            before: self.before,
            projection,
            tasks,
        };
        // install_task_delta can only fail before the business projection is
        // installed. Preserve its observation bookkeeping on a declined reuse.
        let observations = (
            live.summary.clone(),
            live.first_business_ms,
            live.periodic_pending_since,
            live.last_snapshot_ms,
        );
        if let Err(error) =
            live.install_task_delta(capture, delta, catalog, after_inputs, &private, &drafts)
        {
            (
                live.summary,
                live.first_business_ms,
                live.periodic_pending_since,
                live.last_snapshot_ms,
            ) = observations;
            return Err(error.into());
        }
        live.verified_sources = Some((
            catalog.revision.clone(),
            private,
            after_inputs.digest().into(),
            drafts,
        ));
        Ok(true)
    }
}
// The caller has already proved source identity. Fresh exact outbox bytes are
// always synchronized, including Hub acceptance followed by a failed confirm.
fn append(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    id: &str,
    original: NewEvent,
    now: u64,
    check: impl FnOnce(&ProjectSession) -> Result<(), String>,
) -> Result<Event, String> {
    append_scoped(session, catalog, outbox, id, original, now, None, check)
}
#[allow(clippy::too_many_arguments)]
fn append_scoped(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    id: &str,
    original: NewEvent,
    now: u64,
    own: Option<(OwnFinishPhase, &FrozenFinish)>,
    check: impl FnOnce(&ProjectSession) -> Result<(), String>,
) -> Result<Event, String> {
    let inputs = outbox.read_inputs().map_err(|e| uncertain(id, e))?;
    inputs.probe(catalog).map_err(|e| uncertain(id, e))?;
    let receipt = if !inputs.confirmed().iter().any(|e| e.eid == original.eid) {
        crate::recovery_git::read_recovery_history(catalog, &inputs)
            .and_then(|h| h.finish_receipt(catalog, &original))
            .map_err(|e| uncertain(id, e))?
    } else {
        None
    };
    let mut own_append = None;
    let result = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        check(live).map_err(HubError::Invalid)?;
        if receipt.is_none()
            && !inputs
                .pending()
                .iter()
                .any(|event| event.eid == original.eid)
            && !inputs
                .confirmed()
                .iter()
                .any(|event| event.eid == original.eid)
        {
            own_append = own
                .and_then(|(phase, input)| OwnFinishAppend::capture(live, phase, input, &original));
        }
        if let Some(receipt) = &receipt {
            recovery_outbox::append_finish_durable(&mut *live.hub, outbox, original, now, receipt)
        } else {
            recovery_outbox::append_durable(&mut *live.hub, outbox, original, now)
        }
    })
    .map_err(|e| uncertain(id, e))?;
    #[cfg(test)]
    own_append_tests::at_boundary(
        session,
        catalog,
        outbox,
        &inputs,
        &mut own_append,
        &result,
        now,
    );
    let synchronized = outbox.read_inputs().and_then(|fresh| {
        if let (Some(own), Ok(AppendReceipt::Event(actual))) = (own_append, &result) {
            if own
                .install(session, catalog, &inputs, &fresh, actual, now)
                .unwrap_or(false)
            {
                return Ok(());
            }
        }
        ProjectSession::sync_shared(session, catalog, &fresh, now).map_err(|e| e.to_string())
    });
    let actual = match result {
        Ok(AppendReceipt::Event(actual)) => *actual,
        Ok(AppendReceipt::Duplicate { .. }) => {
            return Err(uncertain(
                id,
                "finish duplicate lacks complete original receipt",
            ))
        }
        Err(HubError::Invalid(e)) => return Err(e),
        Err(e) => return Err(uncertain(id, e)),
    };
    synchronized.map_err(|e| uncertain(id, e))?;
    if actual.stale {
        return Err(format!("finish original {} was accepted stale", actual.eid));
    }
    Ok(actual)
}
fn event(status: &FinishStatus, kind: &str, data: Value) -> Result<NewEvent, String> {
    Ok(NewEvent {
        eid: request_event_eid(&status.request_id, kind)?,
        host: status.input.host.clone(),
        actor: "process:towermd-verifier".into(),
        kind: kind.into(),
        layer: "L4".into(),
        obj: status.input.task_id.clone(),
        data,
        guards: status.input.fences.clone(),
    })
}
fn gate_event(
    status: &FinishStatus,
    opening: Event,
    reviewed: Event,
    now: u64,
) -> Result<NewEvent, String> {
    let input = &status.input;
    let gate_id = gate_uuid(&status.request_id)?;
    let id = &gate_id;
    let evidence = GateEvidence {
        object_type: "Evidence".into(),
        id: format!("E-{id}"),
        kind: "gate".into(),
        gate: Gate::G1,
        project_id: input.project_id.clone(),
        task: input.task_id.clone(),
        run: input.run_id.clone(),
        module: input.module_id.clone(),
        commit: Some(input.commit.clone()),
        tree: Some(input.tree.clone()),
        base: Some(input.base.clone()),
        authority: Some(input.authority.clone()),
        baseline: Some(input.baseline.clone()),
        plan_hash: input.plan_hash.clone(),
        plan_opening_eid: Some(opening.eid.clone()),
        commands_hash: Some(input.commands_hash.clone()),
        fences: input.fences.clone(),
        result: GateResult::Passed,
        review: Some(GateReviewLink {
            opening,
            reviewed,
            completed_eid: None,
        }),
        diagnostic: None,
        generated: GateGenerated {
            by: "process:towermd-gates".into(),
            at: crate::hub::timestamp(now)
                .map_err(|e| e.to_string())?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        },
        extra: BTreeMap::from([
            ("finish_input".into(), json!(status.input)),
            ("checked_gates".into(), json!(status.checked_gates)),
        ]),
    };
    contract::validate_gate_layout(&evidence)?;
    Ok(NewEvent {
        eid: request_event_eid(id, "evidence.recorded")?,
        host: input.host.clone(),
        actor: "process:towermd-gates".into(),
        kind: "evidence.recorded".into(),
        layer: "L5".into(),
        obj: evidence.id.clone(),
        data: json!({"evidence":evidence}),
        guards: input.fences.clone(),
    })
}
impl ProjectSession {
    /// Read/repair one original request before considering a current execution
    /// context. Completed is proved by the exact durable completion receipt,
    /// never by a cached Job phase or an isolated Evidence object.
    pub fn finish_view_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        request_id: &str,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<Option<finish::FinishView>, String> {
        let lookup =
            Self::retry_finish_lookup_shared(session, catalog, host, request_id, outbox, &clock)?;
        if let Some(historical) = lookup.historical {
            return Ok(Some(historical));
        }
        let Some(status) = lookup.status else {
            return Ok(None);
        };
        let inputs = outbox.read_inputs().map_err(|e| uncertain(request_id, e))?;
        let terminal = match status.phase {
            JobPhase::Completed => Some(status.completed_eid.clone()),
            JobPhase::Interrupted => Some(request_event_eid(request_id, "finish.interrupted")?),
            _ => None,
        };
        let actual = terminal
            .map(|eid| {
                inputs
                    .confirmed()
                    .iter()
                    .find(|e| e.eid == eid)
                    .cloned()
                    .ok_or_else(|| {
                        uncertain(
                            request_id,
                            "terminal finish lacks its exact confirmed receipt",
                        )
                    })
            })
            .transpose()?;
        let (evidence, report) = if status.phase == JobPhase::Completed {
            let actual = actual.as_ref().ok_or("missing completed receipt")?;
            let completed: Completed = serde_json::from_value(actual.data.clone())
                .map_err(|e| uncertain(request_id, e))?;
            contract::validate_finish_report(
                &status.input,
                &actual.eid,
                &completed.evidence,
                &completed.report,
            )
            .map_err(|e| uncertain(request_id, e))?;
            (Some(completed.evidence), Some(completed.report))
        } else {
            (None, None)
        };
        let run_state = Self::continue_shared(
            session,
            catalog,
            &inputs,
            clock().map_err(|e| uncertain(request_id, e))?,
            |live| {
                live.projection()?
                    .objects
                    .get("Run")
                    .and_then(|runs| runs.get(&status.input.run_id))
                    .and_then(|run| run["state"].as_str())
                    .map(str::to_owned)
                    .ok_or_else(|| "finish Run is absent from verified projection".to_owned())
            },
        )
        .map_err(|e| uncertain(request_id, e))?
        .map_err(|e| uncertain(request_id, e))?;
        Ok(Some(finish::FinishView {
            status,
            run_state,
            actual,
            expiry_actual: lookup.expiry_actual,
            evidence,
            report,
        }))
    }

    /// A scheduling hint over the already installed verified projection only.
    /// Malformed/unknown lifecycle bytes still require full discovery; this
    /// boolean never authorizes execution or completion.
    pub fn has_finish_jobs(&self) -> Result<bool, String> {
        Ok(self
            .projection()?
            .objects
            .get("Run")
            .into_iter()
            .flat_map(|runs| runs.values())
            .any(|run| run.get("finish").is_some()))
    }

    /// Discover only jobs whose full original start is in the same verified
    /// actual outer schedule. A client-written Run.extra or Job index cannot
    /// introduce a job here, and this reader never mints a ticket.
    pub fn finish_jobs_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        host: &str,
        now: u64,
    ) -> Result<Vec<FinishStatus>, String> {
        Self::finish_jobs_for_run_shared(session, catalog, inputs, host, now, None)
    }
    fn finish_jobs_for_run_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        host: &str,
        now: u64,
        run_id: Option<&str>,
    ) -> Result<Vec<FinishStatus>, String> {
        let candidates = Self::continue_shared(session, catalog, inputs, now, |live| {
            let mut candidates = Vec::new();
            for raw in live
                .projection()?
                .objects
                .get("Run")
                .into_iter()
                .flat_map(|runs| runs.values())
            {
                let run: Run = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
                if run.host != host {
                    continue;
                }
                for job in RunFinish::read(&run)?.jobs.into_values() {
                    if job.input.project_id != catalog.config.project_id()
                        || job.input.common_dir != catalog.common_dir
                        || job.input.host != host
                        || job.input.run_id != run.id
                    {
                        return Err("finish Job project/Run identity differs".to_owned());
                    }
                    candidates.push(job);
                }
            }
            Ok::<_, String>(candidates)
        })
        .map_err(|e| e.to_string())??;
        // All candidates were parsed and their structural identities checked
        // above. Only the actual current Run may narrow original-receipt proof;
        // public discovery passes None and preserves the full proof.
        let candidates = candidates
            .into_iter()
            .filter(|job| run_id.is_none_or(|run| job.input.run_id == run))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(candidates);
        }
        let history = history_shared(session, catalog, inputs, now)?;
        let actual = history.actual_outer_events(catalog)?;
        for job in &candidates {
            let start = actual
                .iter()
                .find(|event| event.eid == job.started_eid && !event.stale)
                .ok_or("finish Job has no actual original start")?;
            let original = finish::original_status(&recovery_outbox::original_of(start))?;
            if original.request_id != job.request_id
                || original.input != job.input
                || original.review_context != job.review_context
                || original.checked_gates != job.checked_gates
                || original.completed_eid != job.completed_eid
            {
                return Err("finish Job differs from actual original frozen start".into());
            }
        }
        Ok(candidates)
    }

    /// New CLI activity deduplicates only an actual active job with exactly the
    /// currently checked frozen input. Historical retries use finish_view_shared
    /// directly and retain their original-UUID priority.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_current_finish_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        request_id: &str,
        run: Option<&str>,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<FinishStart, String> {
        let inputs = outbox.read_inputs()?;
        if Self::task_reconciliation_needed_shared(session, catalog, &inputs)? {
            Self::reconcile_pending_shared(session, catalog, &inputs, clock()?, outbox)?;
        }
        let inputs = outbox.read_inputs()?;
        let now = clock()?;
        let (_, actual, _) = capture(session, catalog, &inputs, host, run, now)?;
        let active = Self::finish_jobs_for_run_shared(
            session,
            catalog,
            &inputs,
            host,
            now,
            Some(&actual.id),
        )?
        .into_iter()
        .filter(|job| {
            job.input.run_id == actual.id
                && matches!(job.phase, JobPhase::Queued | JobPhase::Running)
        })
        .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err("multiple active finish jobs in one Run".into());
        }
        if let Some(job) = active.first() {
            let (task, actual, _, report, now) =
                checked_frozen(session, catalog, &inputs, job, &clock)?;
            let input = &job.input;
            let baseline = current_baseline(catalog, &inputs)?
                .ok_or("finish requires an approved baseline")?;
            if input.commit != report.commit
                || input.tree != report.tree
                || input.base != report.base
                || input.baseline != baseline.baseline
                || input.fences != actual.leases
                || input.root != catalog.root
                || input.common_dir != catalog.common_dir
                || input.commands != commands(catalog, &task.spec.module)?
            {
                return Err(
                    "active finish has different frozen input; wait for its original ID".into(),
                );
            }
            Self::continue_shared(session, catalog, &inputs, now, |live| {
                require_same_review(live, catalog, &task, job)
            })
            .map_err(|e| e.to_string())??;
            return Self::begin_finish_shared(
                session,
                catalog,
                host,
                &job.request_id,
                Some(&actual.id),
                outbox,
                clock,
            );
        }
        Self::begin_finish_shared(session, catalog, host, request_id, run, outbox, clock)
    }

    /// Original UUID lookup precedes current business gates. A historical job is
    /// never granted a new execution capability by replaying its old request.
    pub fn retry_finish_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        request_id: &str,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<Option<FinishStatus>, String> {
        Self::retry_finish_lookup_shared(session, catalog, host, request_id, outbox, clock)
            .map(|lookup| lookup.status)
    }
    fn retry_finish_lookup_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        request_id: &str,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<FinishLookup, String> {
        request_event_eid(request_id, "finish.started")?;
        let inputs = outbox.read_inputs().map_err(|e| uncertain(request_id, e))?;
        inputs
            .probe(catalog)
            .map_err(|e| uncertain(request_id, e))?;
        // One complete actual history serves this finite phase lookup. Missing
        // Running/completed/interrupted phases do not each start another proof.
        let scope = history_scope_shared(
            session,
            catalog,
            &inputs,
            clock().map_err(|e| uncertain(request_id, e))?,
            Some(request_id),
        )
        .map_err(|e| uncertain(request_id, e))?;
        if let Some(historical) = maintenance::historical_view(
            &scope.history,
            catalog,
            &inputs,
            &scope.recorded.projection,
            host,
            request_id,
        )? {
            let fresh = outbox.read_inputs().map_err(|e| uncertain(request_id, e))?;
            Self::continue_shared(
                session,
                catalog,
                &fresh,
                clock().map_err(|e| uncertain(request_id, e))?,
                |_| (),
            )
            .map_err(|e| uncertain(request_id, e))?;
            return Ok(FinishLookup {
                status: Some(historical.status.clone()),
                unused_history: None,
                historical: Some(historical),
                expiry_actual: None,
            });
        }
        let actual_outer = scope
            .history
            .actual_outer_events(catalog)
            .map_err(|e| uncertain(request_id, e))?;
        let mut originals = BTreeMap::new();
        for kind in [
            "finish.started",
            "finish.running",
            "finish.completed",
            "finish.interrupted",
        ] {
            let phase_eid = request_event_eid(request_id, kind)?;
            let mut original = read_original(&inputs, &phase_eid)?;
            for actual in actual_outer.iter().filter(|event| event.eid == phase_eid) {
                let candidate = recovery_outbox::original_of(actual);
                if original
                    .as_ref()
                    .is_some_and(|original| original != &candidate)
                {
                    return Err("conflicting actual finish original body".into());
                }
                original = Some(candidate);
            }
            if let Some(original) = original {
                originals.insert(kind, original);
            }
        }
        let Some(original) = originals.get("finish.started") else {
            drop(actual_outer);
            return Ok(FinishLookup {
                status: None,
                // A partial other phase is still an unresolved original intent.
                // Keep the old None result, but do not export an absence scope.
                unused_history: originals.is_empty().then_some(scope),
                historical: None,
                expiry_actual: None,
            });
        };
        let initial = finish::original_status(original)?;
        if initial.request_id != request_id
            || initial.input.host != host
            || initial.input.project_id != catalog.config.project_id()
            || initial.input.common_dir != catalog.common_dir
        {
            return Err("finish request identity conflict".into());
        }
        let expired = scope
            .recorded
            .projection
            .objects
            .get("Run")
            .and_then(|runs| runs.get(&initial.input.run_id))
            .is_some_and(|run| run["state"] == "expired");
        if expired
            && !actual_outer
                .iter()
                .any(|event| event.eid == initial.started_eid && !event.stale)
        {
            return Err("expired finish has no actual original started receipt".into());
        }
        let mut status = initial;
        for kind in [
            "finish.started",
            "finish.running",
            "finish.completed",
            "finish.interrupted",
        ] {
            let inputs = outbox.read_inputs().map_err(|e| uncertain(request_id, e))?;
            if let Some(original) = originals.get(kind) {
                if *original != event(&status, kind, original.data.clone())? {
                    return Err("finish original envelope/UUID identity conflict".into());
                }
                // A durable expired Run closes execution permission. Preserve
                // unresolved pending output as intent, while repairing every
                // actually accepted original phase with its original identity.
                if expired && !actual_outer.iter().any(|actual| actual.eid == original.eid) {
                    continue;
                }
                let confirmed = inputs.confirmed().iter().find(|actual| {
                    actual.eid == original.eid
                        && inputs.receipt_confirmed(actual)
                        && actual_outer.contains(actual)
                });
                let actual = if let Some(actual) = confirmed {
                    recovery_outbox::validate_actual(original, actual)?;
                    if actual.stale {
                        return Err(format!("finish original {} was accepted stale", actual.eid));
                    }
                    actual.clone()
                } else {
                    let proof = pending_proof(session, catalog, outbox, &status, original, &clock)?;
                    let now = clock().map_err(|e| uncertain(request_id, e))?;
                    append(
                        session,
                        catalog,
                        outbox,
                        request_id,
                        original.clone(),
                        now,
                        |live| {
                            proof.as_ref().map_or(Ok(()), |proof| {
                                proof.final_check(live, catalog, &status, now)
                            })
                        },
                    )?
                };
                let run: Run = serde_json::from_value(actual.data["run"].clone())
                    .map_err(|e| e.to_string())?;
                status = finish::status_in(&run, &status.input.evidence_id)?;
                if kind == "finish.completed" {
                    status.phase = JobPhase::Completed;
                }
            }
        }
        // Even a fully confirmed lookup closes against the actual current
        // source and Session. Reuse changes reads, never grants a stale result.
        let fresh = outbox.read_inputs().map_err(|e| uncertain(request_id, e))?;
        let expiry_actual =
            maintenance::expiry_for_view(&scope.history, catalog, &fresh, &status, &actual_outer)
                .map_err(|error| uncertain(request_id, error))?;
        Self::continue_shared(
            session,
            catalog,
            &fresh,
            clock().map_err(|e| uncertain(request_id, e))?,
            |_| (),
        )
        .map_err(|e| uncertain(request_id, e))?;
        Ok(FinishLookup {
            status: Some(status),
            unused_history: None,
            historical: None,
            expiry_actual,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn begin_finish_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        request_id: &str,
        run: Option<&str>,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<FinishStart, String> {
        let lookup =
            Self::retry_finish_lookup_shared(session, catalog, host, request_id, outbox, &clock)?;
        let historical = lookup.historical.is_some();
        let mut unused_history = lookup.unused_history;
        if let Some(status) = lookup.status {
            if run.is_some_and(|r| r != status.input.run_id) {
                return Err("finish request Run identity conflict".into());
            }
            if historical {
                return Ok(FinishStart::Existing(status));
            }
            if status.phase == JobPhase::Queued {
                let inputs = outbox.read_inputs()?;
                let (_, actual, identity, report, _) =
                    checked_frozen(session, catalog, &inputs, &status, &clock)?;
                if report.commit != status.input.commit
                    || report.tree != status.input.tree
                    || actual.leases != status.input.fences
                {
                    return Err("queued finish input changed".into());
                }
                return Ok(FinishStart::Queued(FinishTicket::queued(status, identity)));
            }
            return Ok(FinishStart::Existing(status));
        }
        let inputs = outbox.read_inputs()?;
        inputs.probe(catalog)?;
        if Self::task_reconciliation_needed_shared(session, catalog, &inputs)? {
            // Even a successful/no-op coordination ends this read-only scope.
            unused_history = None;
            Self::reconcile_pending_shared(session, catalog, &inputs, clock()?, outbox)?;
        }
        let inputs = outbox.read_inputs()?;
        let reuse = match &unused_history {
            Some(scope) => scope.matches(session, catalog, &inputs, request_id, clock()?)?,
            None => false,
        };
        let baseline = if reuse {
            crate::baseline_current::current_baseline_in_history(
                catalog,
                &inputs,
                &unused_history
                    .as_ref()
                    .expect("matched private scope")
                    .history,
            )?
        } else {
            unused_history = None;
            current_baseline(catalog, &inputs)?
        }
        .ok_or("finish requires an actually approved published baseline")?;
        let (mut task, mut actual_run, identity, report, _now) =
            checked(session, catalog, &inputs, host, run, &clock)?;
        if task.state == "held" {
            return Err("finish is blocked by the current Hold".into());
        }
        let reuse = match &unused_history {
            Some(scope) => scope.matches(session, catalog, &inputs, request_id, _now)?,
            None => false,
        };
        if reuse {
            claim_dependency_in_history(
                catalog,
                &inputs,
                &task,
                &actual_run,
                &unused_history
                    .as_ref()
                    .expect("matched private scope")
                    .history,
            )?;
        } else {
            claim_dependency(session, catalog, &inputs, &task, &actual_run, _now)?;
        }
        // Nothing survives prior Recording coordination or any following append.
        drop(unused_history);
        let mut lifecycle = RunFinish::read(&actual_run)?;
        if lifecycle
            .jobs
            .values()
            .any(|j| matches!(j.phase, JobPhase::Queued | JobPhase::Running))
        {
            return Err("another finish job is already active".into());
        }
        // Recording requires the exact original UUID to be coordinated first.
        for old in lifecycle
            .jobs
            .values()
            .filter(|j| j.phase == JobPhase::Recording)
        {
            let settled =
                Self::retry_finish_shared(session, catalog, host, &old.request_id, outbox, &clock)?
                    .ok_or_else(|| {
                        uncertain(
                            &old.request_id,
                            "prior completion has no actual original started chain",
                        )
                    })?;
            if settled.phase != JobPhase::Completed {
                return Err(uncertain(
                    &old.request_id,
                    "prior completed receipt requires original coordination",
                ));
            }
        }
        let inputs = outbox.read_inputs()?;
        let (fresh_task, fresh_run, fresh_identity, refreshed, refreshed_at) = checked(
            session,
            catalog,
            &inputs,
            host,
            Some(&actual_run.id),
            &clock,
        )?;
        if fresh_identity != identity
            || json!((&fresh_task, &fresh_run)) != json!((&task, &actual_run))
            || refreshed.commit != report.commit
            || refreshed.tree != report.tree
        {
            return Err("finish context changed while coordinating prior completions".into());
        }
        let now = refreshed_at;
        if let Some(pending) = inputs
            .pending()
            .iter()
            .find(|e| finish::is_event(&e.kind) && e.data["run"]["id"] == actual_run.id)
        {
            return Err(uncertain(
                request_id,
                format!(
                    "prior finish original {} requires coordination",
                    pending.eid
                ),
            ));
        }
        if lifecycle.jobs.is_empty() {
            lifecycle.entered_verifying_at = now;
            lifecycle.merge_deadline = finish::fixed_deadline(now)?;
        }
        if now >= lifecycle.merge_deadline {
            return Err("finish original merge deadline expired".into());
        }
        let commands = commands(catalog, &task.spec.module)?;
        let plan_hash = if high_risk(catalog, &task)? {
            Self::continue_shared(session, catalog, &inputs, now, |live| {
                match live
                    .human()?
                    .current_plan_subject(&actual_run.id)
                    .map_err(|e| format!("ReviewBlocked G1: {e}"))?
                {
                    ReviewSubject::Plan { sha256 } => Ok::<_, String>(Some(sha256)),
                    _ => Err("ReviewBlocked G1: current subject is not a Plan".into()),
                }
            })
            .map_err(|e| e.to_string())??
        } else {
            None
        };
        let input = FrozenFinish {
            evidence_id: format!("E-{request_id}"),
            project_id: catalog.config.project_id().into(),
            host: host.into(),
            root: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            task_id: task.spec.id.clone(),
            run_id: actual_run.id.clone(),
            module_id: task.spec.module.clone(),
            commit: report.commit,
            tree: report.tree,
            base: report.base,
            authority: catalog.revision.clone(),
            baseline: baseline.baseline,
            plan_hash,
            commands_hash: contract::verification::commands_hash(&commands)?,
            commands,
            fences: actual_run.leases.clone(),
            entered_verifying_at: lifecycle.entered_verifying_at,
            merge_deadline: lifecycle.merge_deadline,
        };
        let mut status = FinishStatus {
            request_id: request_id.into(),
            input,
            phase: JobPhase::Queued,
            started_eid: request_event_eid(request_id, "finish.started")?,
            completed_eid: request_event_eid(request_id, "finish.completed")?,
            diagnostic: None,
            review_context: None,
            checked_gates: report.gates,
        };
        let reviews = Self::continue_shared(session, catalog, &inputs, now, |live| {
            require_reviews(live, catalog, &task, &status.input, ReviewGate::G1)
        })
        .map_err(|e| e.to_string())??;
        if let Some((opening, reviewed)) = reviews {
            let prior_gate = known_original(
                session,
                catalog,
                &outbox.read_inputs()?,
                &request_event_eid(&gate_uuid(request_id)?, "evidence.recorded")?,
                now,
            )?;
            let gate = if let Some(prior) = prior_gate {
                let old: FrozenFinish =
                    serde_json::from_value(prior.data["evidence"]["finish_input"].clone())
                        .map_err(|e| e.to_string())?;
                let mut wanted = status.input.clone();
                wanted.entered_verifying_at = old.entered_verifying_at;
                wanted.merge_deadline = old.merge_deadline;
                if old != wanted
                    || prior.data["evidence"]["review"]["opening"] != json!(opening)
                    || prior.data["evidence"]["review"]["reviewed"] != json!(reviewed)
                {
                    return Err("finish original G1 gate identity conflict".into());
                }
                status.input = old;
                lifecycle.entered_verifying_at = status.input.entered_verifying_at;
                lifecycle.merge_deadline = status.input.merge_deadline;
                prior
            } else {
                gate_event(&status, opening.clone(), reviewed.clone(), now)?
            };
            let actual = append(session, catalog, outbox, request_id, gate, now, |live| {
                require_reviews(live, catalog, &task, &status.input, ReviewGate::G1).map(|_| ())
            })?;
            status.review_context = Some(ReviewContext {
                gate: ReviewGate::G1,
                opening,
                reviewed,
                gate_evidence: actual,
            });
        }
        if now >= status.input.merge_deadline {
            return Err("finish original deadline expired".into());
        }
        let expected_before = (task.clone(), actual_run.clone());
        task.state = "verifying".into();
        actual_run.state = "verifying".into();
        lifecycle
            .jobs
            .insert(status.input.evidence_id.clone(), status.clone());
        lifecycle.put(&mut actual_run)?;
        let started = event(
            &status,
            "finish.started",
            json!(Started {
                task: task.clone(),
                run: actual_run.clone(),
                input: status.input.clone(),
                review_context: status.review_context.clone()
            }),
        )?;
        let actual_started = append_scoped(
            session,
            catalog,
            outbox,
            request_id,
            started,
            now,
            Some((OwnFinishPhase::Started, &status.input)),
            |live| {
                let (current_task, current) =
                    actual_context(live, catalog, host, Some(&status.input.run_id), now)?;
                if json!(expected_before) != json!((&current_task, &current))
                    || live.snapshot_session_id() != identity
                    || current.leases != status.input.fences
                {
                    return Err("finish Session/fences changed before start".into());
                }
                require_same_review(live, catalog, &task, &status)
            },
        )?;
        let ticket = FinishTicket::admitted(status, identity, &actual_started)
            .map_err(|error| uncertain(request_id, error))?;
        Ok(FinishStart::Queued(ticket))
    }
    pub fn launch_finish_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        ticket: FinishTicket,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<ExecutionTicket, String> {
        let status = &ticket.status;
        let inputs = outbox.read_inputs()?;
        let (mut task, mut run, identity, report, now) =
            checked_frozen(session, catalog, &inputs, status, &clock)?;
        if identity != ticket.session
            || report.commit != status.input.commit
            || report.tree != status.input.tree
            || run.leases != status.input.fences
            || now >= status.input.merge_deadline
        {
            return Err("finish ticket no longer matches current execution input".into());
        }
        let baseline = current_baseline(catalog, &inputs)?
            .ok_or("finish baseline disappeared before launch")?;
        if baseline.baseline != status.input.baseline {
            return Err("finish baseline changed before launch".into());
        }
        let final_now = clock().map_err(|e| uncertain(&status.request_id, e))?;
        if final_now < now || final_now >= status.input.merge_deadline {
            return Err(uncertain(
                &status.request_id,
                "finish launch clock/deadline changed",
            ));
        }
        let now = final_now;
        let expected_before = (task.clone(), run.clone());
        let mut lifecycle = RunFinish::read(&run)?;
        let job = lifecycle
            .jobs
            .get_mut(&status.input.evidence_id)
            .ok_or("missing accepted finish job")?;
        if *job != *status || job.phase != JobPhase::Queued {
            return Err("finish job is not queued".into());
        }
        job.phase = JobPhase::Running;
        let running = job.clone();
        lifecycle.put(&mut run)?;
        task.state = "verifying".into();
        let original = event(
            status,
            "finish.running",
            json!(Transition {
                task,
                run,
                evidence_id: status.input.evidence_id.clone(),
                diagnostic: None
            }),
        )?;
        append_scoped(
            session,
            catalog,
            outbox,
            &status.request_id,
            original,
            now,
            Some((OwnFinishPhase::Running, &status.input)),
            |live| {
                let (task, run) = actual_context(
                    live,
                    catalog,
                    &status.input.host,
                    Some(&status.input.run_id),
                    now,
                )?;
                if json!(expected_before) != json!((&task, &run))
                    || live.snapshot_session_id() != ticket.session
                    || finish::status_in(&run, &status.input.evidence_id)? != *status
                {
                    return Err("finish queued CAS changed".into());
                }
                require_same_review(live, catalog, &task, status)
            },
        )?;
        Ok(ExecutionTicket {
            status: running,
            session: identity,
        })
    }
    pub fn record_finish_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        executed: ExecutedFinish,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<FinishOutcome, String> {
        Self::record_borrowed_finish_shared(session, catalog, &executed, outbox, clock)
    }
    /// Retain the real non-deserializable execution capability across a source
    /// race. The caller may only retry recording; no second execution is minted.
    pub fn record_borrowed_finish_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        executed: &ExecutedFinish,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<FinishOutcome, String> {
        let status = &executed.status;
        if !executed.cleanup_confirmed() {
            // A worker returning an infrastructure error does not prove its
            // owned process group has exited. Repair only an actual terminal;
            // raw pending output cannot become a new transition in this path.
            let inputs = outbox
                .read_inputs()
                .map_err(|e| uncertain(&status.request_id, e))?;
            let history = history_shared(session, catalog, &inputs, clock()?)
                .map_err(|e| uncertain(&status.request_id, e))?;
            let interrupted_eid = request_event_eid(&status.request_id, "finish.interrupted")?;
            let actual = history
                .actual_outer_events(catalog)?
                .into_iter()
                .find(|actual| {
                    !actual.stale
                        && (actual.eid == status.completed_eid || actual.eid == interrupted_eid)
                });
            let Some(actual) = actual else {
                return Err(uncertain(
                    &status.request_id,
                    format!(
                        "owned process cleanup is unconfirmed: {}",
                        executed
                            .result
                            .as_ref()
                            .err()
                            .map(String::as_str)
                            .unwrap_or("cleanup diagnostics unavailable")
                    ),
                ));
            };
            let original = recovery_outbox::original_of(actual);
            if original != event(status, &actual.kind, actual.data.clone())? {
                return Err("unconfirmed cleanup terminal original identity differs".into());
            }
            let run: Run =
                serde_json::from_value(actual.data["run"].clone()).map_err(|e| e.to_string())?;
            let mut done = finish::status_in(&run, &status.input.evidence_id)?;
            if done.input != status.input
                || done.request_id != status.request_id
                || done.started_eid != status.started_eid
                || done.completed_eid != status.completed_eid
                || done.review_context != status.review_context
                || done.checked_gates != status.checked_gates
            {
                return Err("unconfirmed cleanup terminal frozen input differs".into());
            }
            let evidence = if actual.kind == "finish.completed" {
                let completed: Completed =
                    serde_json::from_value(actual.data.clone()).map_err(|e| e.to_string())?;
                contract::validate_finish_report(
                    &status.input,
                    &actual.eid,
                    &completed.evidence,
                    &completed.report,
                )?;
                done.phase = JobPhase::Completed;
                Some(completed.evidence)
            } else if actual.kind == "finish.interrupted" {
                done.phase = JobPhase::Interrupted;
                None
            } else {
                return Err("unconfirmed cleanup has no finite accepted terminal".into());
            };
            let receipt = history
                .actual_finish_receipt(catalog, &original)?
                .ok_or("unconfirmed cleanup lost its actual terminal receipt")?;
            let now = clock().map_err(|e| uncertain(&status.request_id, e))?;
            let fresh = outbox
                .read_inputs()
                .map_err(|e| uncertain(&status.request_id, e))?;
            let appended = Self::continue_shared(session, catalog, &fresh, now, |live| {
                recovery_outbox::append_finish_durable(
                    &mut *live.hub,
                    outbox,
                    original,
                    now,
                    &receipt,
                )
            })
            .map_err(|e| uncertain(&status.request_id, e))?
            .map_err(|e| uncertain(&status.request_id, e))?;
            let fresh = outbox
                .read_inputs()
                .map_err(|e| uncertain(&status.request_id, e))?;
            Self::sync_shared(session, catalog, &fresh, clock()?)
                .map_err(|e| uncertain(&status.request_id, e))?;
            let AppendReceipt::Event(actual) = appended else {
                return Err(uncertain(
                    &status.request_id,
                    "terminal duplicate lacks full actual receipt",
                ));
            };
            return Ok(FinishOutcome {
                status: done,
                actual: *actual,
                evidence,
            });
        }
        if let Some(original) = known_original(
            session,
            catalog,
            &outbox.read_inputs()?,
            &status.completed_eid,
            clock().map_err(|e| uncertain(&status.request_id, e))?,
        )? {
            let proof = pending_proof(session, catalog, outbox, status, &original, &clock)?;
            let now = clock().map_err(|e| uncertain(&status.request_id, e))?;
            let actual = append(
                session,
                catalog,
                outbox,
                &status.request_id,
                original,
                now,
                |live| {
                    proof.as_ref().map_or(Ok(()), |proof| {
                        proof.final_check(live, catalog, status, now)
                    })
                },
            )?;
            let mut done = status.clone();
            done.phase = JobPhase::Completed;
            return Ok(FinishOutcome {
                status: done,
                evidence: Some(
                    serde_json::from_value(actual.data["evidence"].clone())
                        .map_err(|e| e.to_string())?,
                ),
                actual,
            });
        }
        let report = match &executed.result {
            Ok(report) => report.clone(),
            Err(reason) => {
                // The first interrupt may already be accepted while its local
                // confirmation/source sync failed. Coordinate that exact UUID
                // first; never manufacture another transition from Interrupted.
                if let Some(current) = Self::finish_view_shared(
                    session,
                    catalog,
                    &status.input.host,
                    &status.request_id,
                    outbox,
                    &clock,
                )? {
                    if current.status.input != status.input {
                        return Err("finish interrupted original input differs".into());
                    }
                    if current.status.phase == JobPhase::Interrupted {
                        let actual = current.actual.ok_or_else(|| {
                            uncertain(
                                &status.request_id,
                                "interrupted finish lacks its actual durable receipt",
                            )
                        })?;
                        return Ok(FinishOutcome {
                            status: current.status,
                            actual,
                            evidence: None,
                        });
                    }
                }
                return Self::interrupt_finish_shared(
                    session,
                    catalog,
                    &status.input.host,
                    &status.request_id,
                    reason,
                    outbox,
                    clock,
                );
            }
        };
        finish::check_sealed_logs(&status.input, &report)
            .map_err(|e| uncertain(&status.request_id, e))?;
        let inputs = outbox.read_inputs()?;
        let (task, run, identity, checked, now) =
            checked_frozen(session, catalog, &inputs, status, &clock)
                .map_err(|e| uncertain(&status.request_id, e))?;
        if identity != executed.session
            || checked.commit != status.input.commit
            || checked.tree != status.input.tree
            || checked.base != status.input.base
            || now >= status.input.merge_deadline
        {
            return Err(uncertain(
                &status.request_id,
                "finish input/Session/deadline changed before result recording",
            ));
        }
        let current_baseline = current_baseline(catalog, &inputs)
            .map_err(|e| uncertain(&status.request_id, e))?
            .ok_or("finish baseline disappeared")?;
        if current_baseline.baseline != status.input.baseline {
            return Err(uncertain(
                &status.request_id,
                "finish approved baseline changed",
            ));
        }
        let final_now = clock().map_err(|e| uncertain(&status.request_id, e))?;
        if final_now < now || final_now >= status.input.merge_deadline {
            return Err(uncertain(
                &status.request_id,
                "finish final clock/deadline changed",
            ));
        }
        after_running(&inputs, status, final_now)?;
        let expected_before = (task.clone(), run.clone());
        let completed = completed_data(task, run, status, report)?;
        let evidence = completed.evidence.clone();
        let original = event(status, "finish.completed", json!(completed))?;
        let now = final_now;
        let actual = append(
            session,
            catalog,
            outbox,
            &status.request_id,
            original,
            now,
            |live| {
                let (task, run) = actual_context(
                    live,
                    catalog,
                    &status.input.host,
                    Some(&status.input.run_id),
                    now,
                )?;
                if json!(expected_before) != json!((&task, &run))
                    || live.snapshot_session_id() != executed.session
                    || finish::status_in(&run, &status.input.evidence_id)? != *status
                {
                    return Err("finish final Running CAS changed".into());
                }
                require_same_review(live, catalog, &task, status)
            },
        )?;
        let mut done = status.clone();
        done.phase = JobPhase::Completed;
        Ok(FinishOutcome {
            status: done,
            actual,
            evidence: Some(evidence),
        })
    }
    /// Caller must know its worker has stopped. No failure Evidence is fabricated.
    #[allow(clippy::too_many_arguments)]
    pub fn interrupt_finish_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        request_id: &str,
        diagnostic: &str,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<FinishOutcome, String> {
        if diagnostic.trim().is_empty() {
            return Err("finish interruption requires a diagnostic".into());
        }
        let inputs = outbox.read_inputs()?;
        let original = read_original(&inputs, &request_event_eid(request_id, "finish.started")?)?
            .ok_or("interrupted finish has no original started input")?;
        let status = finish::original_status(&original)?;
        if status.input.host != host {
            return Err("finish interruption host differs".into());
        }
        let now = clock()?;
        if now >= status.input.merge_deadline {
            // Deadline closes new transitions, not an already accepted
            // terminal receipt. Never turn a late cancellation/infra result
            // back into Running while its last renewal is still alive.
            if let Some(view) =
                Self::finish_view_shared(session, catalog, host, request_id, outbox, &clock)?
            {
                if view.status.input != status.input {
                    return Err("finish interruption original input differs".into());
                }
                if matches!(
                    view.status.phase,
                    JobPhase::Completed | JobPhase::Interrupted
                ) {
                    let actual = view.actual.ok_or_else(|| {
                        uncertain(request_id, "terminal lacks its actual original receipt")
                    })?;
                    return Ok(FinishOutcome {
                        status: view.status,
                        actual,
                        evidence: view.evidence,
                    });
                }
            }
            return Err(uncertain(
                request_id,
                "finish deadline prevents a new interruption",
            ));
        }
        let (mut task, mut run, identity) = capture(
            session,
            catalog,
            &inputs,
            host,
            Some(&status.input.run_id),
            now,
        )?;
        let expected_before = (task.clone(), run.clone());
        let mut lifecycle = RunFinish::read(&run)?;
        let job = lifecycle
            .jobs
            .get_mut(&status.input.evidence_id)
            .ok_or("missing interrupted job")?;
        if !matches!(job.phase, JobPhase::Queued | JobPhase::Running) {
            return Err("finish no longer has an interruptible worker".into());
        }
        job.phase = JobPhase::Interrupted;
        job.diagnostic = Some(diagnostic.into());
        let interrupted = job.clone();
        lifecycle.put(&mut run)?;
        run.state = "running".into();
        task.state = "running".into();
        let original = event(
            &status,
            "finish.interrupted",
            json!(Transition {
                task,
                run,
                evidence_id: status.input.evidence_id.clone(),
                diagnostic: Some(diagnostic.into())
            }),
        )?;
        let final_now = clock()?;
        if final_now < now || final_now >= status.input.merge_deadline {
            return Err(uncertain(
                request_id,
                "finish interruption final clock/deadline changed",
            ));
        }
        let actual = append(
            session,
            catalog,
            outbox,
            request_id,
            original,
            final_now,
            |live| {
                let checked_now = clock()?;
                if checked_now < final_now || checked_now >= status.input.merge_deadline {
                    return Err(uncertain(
                        request_id,
                        "finish interruption append clock/deadline changed",
                    ));
                }
                let current =
                    actual_context(live, catalog, host, Some(&status.input.run_id), checked_now)?;
                if live.snapshot_session_id() != identity
                    || json!(expected_before) != json!(current)
                {
                    return Err(uncertain(
                        request_id,
                        "finish interruption final Task/Run capture changed",
                    ));
                }
                Ok(())
            },
        )?;
        Ok(FinishOutcome {
            status: interrupted,
            actual,
            evidence: None,
        })
    }
    pub fn require_finish_review_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        input: &FrozenFinish,
        gate: ReviewGate,
        inputs: &RecoveryInputs,
        now: u64,
    ) -> Result<(), String> {
        Self::continue_shared(session, catalog, inputs, now, |live| {
            let (task, run) = actual_context(live, catalog, &input.host, Some(&input.run_id), now)?;
            if run.leases != input.fences {
                return Err("finish Review fences changed".into());
            }
            require_reviews(live, catalog, &task, input, gate).map(|_| ())
        })
        .map_err(|e| e.to_string())?
    }
}

#[cfg(test)]
#[path = "../../tests/support/finish_fixture.rs"]
pub(super) mod voluntary_fixture;
#[cfg(test)]
mod voluntary_review_tests {
    use super::voluntary_fixture::*;
    use super::*;
    use crate::{
        baseline_review::review_baseline_shared,
        human_intervention::{HumanCommand, ReviewVerdict},
        recovery_contract::{baseline::Verdict, recorded::TaskReviewAction},
        snapshot_service::PublicationReport,
        tasks::{TaskCommand, TaskSpec},
    };
    use std::sync::Arc;

    #[test]
    fn non_high_voluntary_g1_replays_only_actual_matching_human_and_gate_roles() {
        let mut f=configured_fixture(&[("docs/tower/specs/AC-voluntary.md","---\ntype: Acceptance\nid: AC-voluntary\nmodule: A-alpha\ncriteria: [actual voluntary review]\n---\n")], &["exit 0"], false);
        let baseline_action = action(&f, Verdict::Approved);
        let approval = review_baseline_shared(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &baseline_action,
            "human:owner",
            "host",
            NOW + 3,
        )
        .unwrap();
        assert!(matches!(
            f.writer
                .publish_baseline(
                    &f.session,
                    &f.catalog,
                    &mut f.outbox,
                    &approval.receipt.result_ref.approval_eid,
                    "host",
                    || Ok(NOW + 5)
                )
                .unwrap(),
            PublicationReport::Recorded { .. }
        ));
        git(f.root.path(), &["switch", "-qc", "t/T-voluntary", "main"]);
        f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
        ProjectSession::execute_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            "host",
            TaskCommand::Add {
                task: TaskSpec {
                    id: "T-voluntary".into(),
                    title: "voluntary G1".into(),
                    module: "A-alpha".into(),
                    touches: vec![],
                    allowed_paths: vec!["alpha/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC-voluntary".into()],
                    decisions: vec![],
                },
            },
            None,
            NOW + 10,
            &mut f.outbox,
        )
        .unwrap();
        ProjectSession::execute_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            "host",
            TaskCommand::Claim {
                id: "T-voluntary".into(),
                harness: None,
                harness_version: None,
            },
            None,
            NOW + 11,
            &mut f.outbox,
        )
        .unwrap();
        let opening = ProjectSession::request_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            &uuid::Uuid::now_v7().to_string(),
            &HumanCommand::ReviewRequest {
                gate: ReviewGate::G1,
                content: Some("actual optional Plan\n".into()),
                source_commit: None,
                source_path: None,
            },
            None,
            &mut f.outbox,
            || Ok(NOW + 20),
        )
        .unwrap();
        ProjectSession::answer_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            "human:owner",
            &uuid::Uuid::now_v7().to_string(),
            &TaskReviewAction::Review {
                hold: opening.receipt.result_ref.hold.clone(),
                binding: opening.receipt.result_ref.binding.clone(),
                verdict: ReviewVerdict::Approved,
                comment: "approve this optional Plan".into(),
            },
            &mut f.outbox,
            || Ok(NOW + 21),
        )
        .unwrap();

        // The current producer deliberately omits optional G1. Obtain its real
        // checked/frozen input, stop that queued job, then exercise the existing
        // wire contract with an actual accepted second freeze. No ReviewFact,
        // verification report or recovery proof is filled in by this fixture.
        let first_id = uuid::Uuid::now_v7().to_string();
        let FinishStart::Queued(first) = ProjectSession::begin_finish_shared(
            &f.session,
            &f.catalog,
            "host",
            &first_id,
            None,
            &mut f.outbox,
            || Ok(NOW + 30),
        )
        .unwrap() else {
            panic!()
        };
        let mut status = first.status().clone();
        assert!(status.review_context.is_none());
        assert!(status.input.plan_hash.is_none());
        drop(first);
        ProjectSession::interrupt_finish_shared(
            &f.session,
            &f.catalog,
            "host",
            &first_id,
            "queued owner stopped before execution",
            &mut f.outbox,
            || Ok(NOW + 31),
        )
        .unwrap();
        let inputs = f.outbox.read_inputs().unwrap();
        let (mut task, mut run, _, check, _) = checked(
            &f.session,
            &f.catalog,
            &inputs,
            "host",
            Some(&status.input.run_id),
            &|| Ok(NOW + 32),
        )
        .unwrap();
        assert!(!high_risk(&f.catalog, &task).unwrap());
        assert_eq!(check.commit, status.input.commit);
        assert_eq!(check.tree, status.input.tree);
        assert_eq!(check.base, status.input.base);
        status.request_id = uuid::Uuid::now_v7().to_string();
        status.input.evidence_id = format!("E-{}", status.request_id);
        status.started_eid = request_event_eid(&status.request_id, "finish.started").unwrap();
        status.completed_eid = request_event_eid(&status.request_id, "finish.completed").unwrap();
        status.checked_gates = check.gates;
        let (actual_opening, actual_reviewed) = {
            let live = f.session.lock().unwrap();
            let human = live.human().unwrap();
            let ReviewSubject::Plan { sha256 } = human.current_plan_subject(&run.id).unwrap()
            else {
                panic!()
            };
            status.input.plan_hash = Some(sha256);
            let (opening, reviewed) = human
                .review_originals(&binding(&status.input, ReviewGate::G1).unwrap())
                .unwrap();
            (opening.clone(), reviewed.clone())
        };
        let gate = gate_event(
            &status,
            actual_opening.clone(),
            actual_reviewed.clone(),
            NOW + 33,
        )
        .unwrap();
        let actual_gate = append(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &status.request_id,
            gate,
            NOW + 33,
            |live| {
                live.human()?
                    .review_originals(&binding(&status.input, ReviewGate::G1)?)
                    .map(|_| ())
            },
        )
        .unwrap();
        status.review_context = Some(ReviewContext {
            gate: ReviewGate::G1,
            opening: actual_opening,
            reviewed: actual_reviewed,
            gate_evidence: actual_gate,
        });
        let before = (task.clone(), run.clone());
        let mut lifecycle = RunFinish::read(&run).unwrap();
        lifecycle
            .jobs
            .insert(status.input.evidence_id.clone(), status.clone());
        lifecycle.put(&mut run).unwrap();
        task.state = "verifying".into();
        run.state = "verifying".into();
        let original = event(
            &status,
            "finish.started",
            json!(Started {
                task,
                run,
                input: status.input.clone(),
                review_context: status.review_context.clone()
            }),
        )
        .unwrap();
        let actual = append(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &status.request_id,
            original,
            NOW + 34,
            |live| {
                let current = actual_context(
                    live,
                    &f.catalog,
                    "host",
                    Some(&status.input.run_id),
                    NOW + 34,
                )?;
                if json!(current) != json!(before) {
                    return Err("actual voluntary freeze context changed".into());
                }
                Ok(())
            },
        )
        .unwrap();
        assert!(!actual.stale);
        assert_eq!(
            f.outbox
                .read_inputs()
                .unwrap()
                .confirmed()
                .iter()
                .find(|e| e.eid == actual.eid),
            Some(&actual)
        );
        let seen = f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.seq < actual.seq)
            .map(|e| (e.eid.clone(), e.clone()))
            .collect::<BTreeMap<_, _>>();
        {
            let live = f.session.lock().unwrap();
            let human = live.human().unwrap();
            finish::validate_review_at(&f.catalog, human, &seen, &actual).unwrap();
            for fault in [
                "missing_context",
                "missing_plan",
                "wrong_plan",
                "changed_original_answer",
                "wrong_gate_guards",
                "missing_actual_gate",
            ] {
                let mut altered = actual.clone();
                let mut known = seen.clone();
                let job = &mut altered.data["run"]["finish"]["jobs"][&status.input.evidence_id];
                match fault {
                    "missing_context" => job["review_context"] = Value::Null,
                    "missing_plan" => job["input"]["plan_hash"] = Value::Null,
                    "wrong_plan" => {
                        job["input"]["plan_hash"] = json!(contract::sha256(b"different Plan"))
                    }
                    "changed_original_answer" => {
                        job["review_context"]["reviewed"]["data"]["comment"] =
                            json!("not the accepted answer")
                    }
                    "wrong_gate_guards" => {
                        job["review_context"]["gate_evidence"]["guards"][0]["epoch"] =
                            json!(status.input.fences[0].epoch + 1)
                    }
                    "missing_actual_gate" => {
                        known.remove(&status.review_context.as_ref().unwrap().gate_evidence.eid);
                    }
                    _ => unreachable!(),
                }
                assert!(
                    finish::validate_review_at(&f.catalog, human, &known, &altered).is_err(),
                    "{fault}"
                );
            }
        }
        let expected = f.session.lock().unwrap().projection().unwrap().clone();
        assert_eq!(
            f.writer
                .write(
                    &f.session,
                    &f.catalog,
                    "host",
                    true,
                    || Ok(NOW + 40),
                    &mut f.outbox
                )
                .unwrap()
                .status,
            "written"
        );
        f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
        drop(f.session);
        let reopened = ProjectSession::open(
            &f.catalog.common_dir.join("towermd"),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            NOW + 41,
        )
        .unwrap();
        assert_eq!(reopened.projection().unwrap().objects, expected.objects);
        let restored: Run = serde_json::from_value(
            reopened.projection().unwrap().objects["Run"][&status.input.run_id].clone(),
        )
        .unwrap();
        assert_eq!(
            finish::status_in(&restored, &status.input.evidence_id).unwrap(),
            status
        );
        assert_eq!(
            reopened
                .human()
                .unwrap()
                .review_originals(&binding(&status.input, ReviewGate::G1).unwrap())
                .unwrap()
                .0,
            &status.review_context.as_ref().unwrap().opening
        );
    }
}

#[cfg(test)]
mod history_reuse_tests {
    use super::*;
    #[test]
    fn actual_renewal_prefix_reuses_history_but_business_extension_does_not() {
        let mut f = crate::ask_fixture::claimed();
        let inputs = f.outbox.read_inputs().unwrap();
        ProjectSession::sync_shared(
            &f.session,
            &f.catalog,
            &inputs,
            crate::ask_fixture::NOW + 20,
        )
        .unwrap();
        let before = f
            .session
            .lock()
            .unwrap()
            .capture(crate::ask_fixture::NOW + 20)
            .unwrap();
        f.session
            .lock()
            .unwrap()
            .renew_binding(&f.catalog, "host", None, crate::ask_fixture::NOW + 21)
            .unwrap();
        let fresh = f
            .session
            .lock()
            .unwrap()
            .capture(crate::ask_fixture::NOW + 21)
            .unwrap();
        assert!(fresh.tail.len() > before.tail.len());
        assert!(same_history_prefix(&before, &fresh));
        ProjectSession::sync_shared(
            &f.session,
            &f.catalog,
            &inputs,
            crate::ask_fixture::NOW + 21,
        )
        .unwrap();
        let installed = f
            .session
            .lock()
            .unwrap()
            .capture(crate::ask_fixture::NOW + 21)
            .unwrap();
        assert!(same_history_prefix(&before, &installed));
        ProjectSession::execute_shared(
            &f.session,
            &f.catalog,
            &inputs,
            "host",
            crate::tasks::TaskCommand::Add {
                task: crate::tasks::TaskSpec {
                    id: "T-history-other".into(),
                    title: "actual business extension".into(),
                    module: "A-alpha".into(),
                    touches: vec![],
                    allowed_paths: vec!["alpha/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC-ask".into()],
                    decisions: vec![],
                },
            },
            None,
            crate::ask_fixture::NOW + 22,
            &mut f.outbox,
        )
        .unwrap();
        let business = f
            .session
            .lock()
            .unwrap()
            .capture(crate::ask_fixture::NOW + 22)
            .unwrap();
        assert!(!same_history_prefix(&before, &business));
    }
}

#[cfg(test)]
#[path = "finish_append_tests.rs"]
mod own_append_tests;

#[cfg(test)]
#[path = "finish_expiry_tests.rs"]
mod expiry_receipt_tests;

#[cfg(test)]
#[path = "finish_cleanup_tests.rs"]
mod cleanup_confirmation_tests;

#[cfg(test)]
mod finite_history_scope_tests {
    use super::*;
    use crate::ask_fixture::{claimed, git, Fixture, NOW};

    fn unused(f: &mut Fixture, at: u64) -> (String, FinishReadScope) {
        let id = uuid::Uuid::now_v7().to_string();
        let lookup = ProjectSession::retry_finish_lookup_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &mut f.outbox,
            || Ok(at),
        )
        .unwrap();
        assert!(lookup.status.is_none());
        (id, lookup.unused_history.unwrap())
    }

    #[test]
    fn unused_request_scope_keeps_actual_baseline_claim_and_only_verified_renewals() {
        let mut f = claimed();
        let (id, scope) = unused(&mut f, NOW + 20);
        let inputs = f.outbox.read_inputs().unwrap();
        assert!(scope
            .matches(&f.session, &f.catalog, &inputs, &id, NOW + 21)
            .unwrap());
        assert!(!scope
            .matches(
                &f.session,
                &f.catalog,
                &inputs,
                &uuid::Uuid::now_v7().to_string(),
                NOW + 21,
            )
            .unwrap());
        let shared = crate::baseline_current::current_baseline_in_history(
            &f.catalog,
            &inputs,
            &scope.history,
        )
        .unwrap();
        assert_eq!(shared, current_baseline(&f.catalog, &inputs).unwrap());
        assert_eq!(shared.unwrap().baseline, "towermd/B0");
        let (task, run, _) =
            capture(&f.session, &f.catalog, &inputs, "host", None, NOW + 21).unwrap();
        claim_dependency_in_history(&f.catalog, &inputs, &task, &run, &scope.history).unwrap();
        f.session
            .lock()
            .unwrap()
            .renew_binding(&f.catalog, "host", None, NOW + 22)
            .unwrap();
        assert!(scope
            .matches(&f.session, &f.catalog, &inputs, &id, NOW + 22)
            .unwrap());
        claim_dependency_in_history(&f.catalog, &inputs, &task, &run, &scope.history).unwrap();
    }

    #[test]
    fn actual_business_extension_misses_scope_and_original_reader_accepts_fresh_history() {
        let mut f = claimed();
        let (id, scope) = unused(&mut f, NOW + 20);
        let before = f.outbox.read_inputs().unwrap();
        ProjectSession::execute_shared(
            &f.session,
            &f.catalog,
            &before,
            "host",
            crate::tasks::TaskCommand::Add {
                task: crate::tasks::TaskSpec {
                    id: "T-finite-history-other".into(),
                    title: "actual finite history extension".into(),
                    module: "A-alpha".into(),
                    touches: vec![],
                    allowed_paths: vec!["alpha/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC-ask".into()],
                    decisions: vec![],
                },
            },
            None,
            NOW + 21,
            &mut f.outbox,
        )
        .unwrap();
        let fresh = f.outbox.read_inputs().unwrap();
        assert!(!scope
            .matches(&f.session, &f.catalog, &fresh, &id, NOW + 22)
            .unwrap());
        let independent = history_shared(&f.session, &f.catalog, &fresh, NOW + 22).unwrap();
        assert!(independent
            .actual_outer_events(&f.catalog)
            .unwrap()
            .iter()
            .any(|event| { event.kind == "task.state" && event.obj == "T-finite-history-other" }));
        let (task, run, _) =
            capture(&f.session, &f.catalog, &fresh, "host", None, NOW + 22).unwrap();
        claim_dependency(&f.session, &f.catalog, &fresh, &task, &run, NOW + 22).unwrap();
    }

    #[test]
    fn scope_cannot_turn_missing_actual_git_object_into_a_cache_miss_or_success() {
        let mut f = claimed();
        let (id, scope) = unused(&mut f, NOW + 20);
        let inputs = f.outbox.read_inputs().unwrap();
        let tree = git(f.root.path(), &["rev-parse", "main^{tree}"]);
        let object = f
            .catalog
            .common_dir
            .join("objects")
            .join(&tree[..2])
            .join(&tree[2..]);
        let withheld = object.with_extension("finite-history-withheld");
        std::fs::rename(&object, &withheld).unwrap();
        let result = scope.matches(&f.session, &f.catalog, &inputs, &id, NOW + 21);
        std::fs::rename(&withheld, &object).unwrap();
        assert!(
            result.is_err(),
            "a real missing source object is not an ordinary reuse miss"
        );
        assert!(scope
            .matches(&f.session, &f.catalog, &inputs, &id, NOW + 22)
            .unwrap());
    }

    #[test]
    fn shared_history_does_not_authorize_a_new_bare_baseline_tag() {
        let mut f = claimed();
        let (_, scope) = unused(&mut f, NOW + 20);
        let inputs = f.outbox.read_inputs().unwrap();
        let before = f.session.lock().unwrap().applied_seq();
        git(f.root.path(), &["tag", "towermd/B1", &f.catalog.revision]);
        assert!(crate::baseline_current::current_baseline_in_history(
            &f.catalog,
            &inputs,
            &scope.history,
        )
        .is_err());
        assert!(current_baseline(&f.catalog, &inputs).is_err());
        assert_eq!(f.session.lock().unwrap().applied_seq(), before);
    }

    #[test]
    fn actual_started_receipt_retry_exports_no_unused_history_and_appends_nothing() {
        let mut f = claimed();
        let id = uuid::Uuid::now_v7().to_string();
        let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            None,
            &mut f.outbox,
            || Ok(NOW + 20),
        )
        .unwrap() else {
            panic!("actual new request was not queued")
        };
        let started = ticket.status().started_eid.clone();
        let before = f.outbox.read_inputs().unwrap();
        assert_eq!(
            before
                .confirmed()
                .iter()
                .filter(|e| e.eid == started)
                .count(),
            1
        );
        let head = f.session.lock().unwrap().applied_seq();
        let lookup = ProjectSession::retry_finish_lookup_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &mut f.outbox,
            || Ok(NOW + 21),
        )
        .unwrap();
        assert_eq!(lookup.status.unwrap(), *ticket.status());
        assert!(lookup.unused_history.is_none());
        let after = f.outbox.read_inputs().unwrap();
        before.validate_task_writes(&after, &[]).unwrap();
        assert_eq!(f.session.lock().unwrap().applied_seq(), head);
    }

    #[test]
    fn other_phase_pending_keeps_legacy_none_without_exporting_an_absence_scope() {
        let mut f = claimed();
        let id = uuid::Uuid::now_v7().to_string();
        let inputs = f.outbox.read_inputs().unwrap();
        let (task, run, _) =
            capture(&f.session, &f.catalog, &inputs, "host", None, NOW + 20).unwrap();
        // A real fsynced producer intent is not an accepted lifecycle. Deliberately
        // leave started absent: this partial queue must never prove all phases absent.
        let original = NewEvent {
            eid: request_event_eid(&id, "finish.running").unwrap(),
            host: "host".into(),
            actor: "process:towermd-verifier".into(),
            kind: "finish.running".into(),
            layer: "L4".into(),
            obj: task.spec.id.clone(),
            guards: run.leases.clone(),
            data: json!(Transition {
                task,
                run,
                evidence_id: format!("E-{id}"),
                diagnostic: None,
            }),
        };
        f.outbox.append_pending(&original).unwrap();
        let before = f.outbox.read_inputs().unwrap();
        assert!(before.pending().contains(&original));
        assert!(!before
            .confirmed()
            .iter()
            .any(|event| event.eid == original.eid));
        let applied = f.session.lock().unwrap().applied_seq();
        let lookup = ProjectSession::retry_finish_lookup_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &mut f.outbox,
            || Ok(NOW + 21),
        )
        .unwrap();
        assert!(lookup.status.is_none());
        assert!(lookup.unused_history.is_none());
        assert!(ProjectSession::retry_finish_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &mut f.outbox,
            || Ok(NOW + 22),
        )
        .unwrap()
        .is_none());
        let after = f.outbox.read_inputs().unwrap();
        before.validate_task_writes(&after, &[]).unwrap();
        assert!(after.pending().contains(&original));
        assert!(!after
            .confirmed()
            .iter()
            .any(|event| event.eid == original.eid));
        assert_eq!(f.session.lock().unwrap().applied_seq(), applied);
    }
}

#[cfg(test)]
#[path = "finish_admission_tests.rs"]
mod admission_tests;
