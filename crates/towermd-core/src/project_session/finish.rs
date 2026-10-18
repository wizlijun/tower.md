//! Finite finish producer; no report supplied by a client can enter this path.
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
    let now = clock()?;
    let (task, run, identity) = capture(session, catalog, inputs, host, run, now)?;
    let leases = session
        .lock()
        .map_err(|_| "session lock poisoned")?
        .head
        .leases
        .clone();
    let report = task_gates::check(catalog, &task, &run, host, &leases, now)
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
fn history_shared(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    now: u64,
) -> Result<crate::recovery_git::VerifiedRecoveryHistory, String> {
    let (identity, captured) =
        ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
            live.capture(now)
                .map(|capture| (live.snapshot_session_id(), capture))
        })
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let history = crate::recovery_git::prepare_session_history(catalog, inputs, &captured)?;
    let unchanged = ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
        live.capture(now)
            .map(|current| live.snapshot_session_id() == identity && current == captured)
    })
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if !unchanged {
        return Err("finish actual history capture changed during proof".into());
    }
    Ok(history)
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
    let (task, run, identity, report, now) = checked(
        session,
        catalog,
        &inputs,
        &status.input.host,
        Some(&status.input.run_id),
        clock,
    )?;
    if catalog.root != status.input.root
        || catalog.common_dir != status.input.common_dir
        || catalog.revision != status.input.authority
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
    let inputs = outbox.read_inputs().map_err(|e| uncertain(id, e))?;
    inputs.probe(catalog).map_err(|e| uncertain(id, e))?;
    let receipt = if !inputs.confirmed().iter().any(|e| e.eid == original.eid) {
        crate::recovery_git::read_recovery_history(catalog, &inputs)
            .and_then(|h| h.finish_receipt(catalog, &original))
            .map_err(|e| uncertain(id, e))?
    } else {
        None
    };
    let result = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        check(live).map_err(HubError::Invalid)?;
        if let Some(receipt) = &receipt {
            recovery_outbox::append_finish_durable(&mut *live.hub, outbox, original, now, receipt)
        } else {
            recovery_outbox::append_durable(&mut *live.hub, outbox, original, now)
        }
    })
    .map_err(|e| uncertain(id, e))?;
    let synchronized = outbox.read_inputs().and_then(|fresh| {
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
        let eid = request_event_eid(request_id, "finish.started")?;
        let inputs = outbox.read_inputs().map_err(|e| uncertain(request_id, e))?;
        inputs
            .probe(catalog)
            .map_err(|e| uncertain(request_id, e))?;
        let Some(original) = known_original(
            session,
            catalog,
            &inputs,
            &eid,
            clock().map_err(|e| uncertain(request_id, e))?,
        )?
        else {
            return Ok(None);
        };
        let initial = finish::original_status(&original)?;
        if initial.request_id != request_id
            || initial.input.host != host
            || initial.input.project_id != catalog.config.project_id()
            || initial.input.common_dir != catalog.common_dir
        {
            return Err("finish request identity conflict".into());
        }
        let mut status = initial;
        for kind in [
            "finish.started",
            "finish.running",
            "finish.completed",
            "finish.interrupted",
        ] {
            let inputs = outbox.read_inputs().map_err(|e| uncertain(request_id, e))?;
            if let Some(original) = known_original(
                session,
                catalog,
                &inputs,
                &request_event_eid(request_id, kind)?,
                clock().map_err(|e| uncertain(request_id, e))?,
            )? {
                if original != event(&status, kind, original.data.clone())? {
                    return Err("finish original envelope/UUID identity conflict".into());
                }
                let proof = pending_proof(session, catalog, outbox, &status, &original, &clock)?;
                let now = clock().map_err(|e| uncertain(request_id, e))?;
                let actual = append(
                    session,
                    catalog,
                    outbox,
                    request_id,
                    original,
                    now,
                    |live| {
                        proof.as_ref().map_or(Ok(()), |proof| {
                            proof.final_check(live, catalog, &status, now)
                        })
                    },
                )?;
                let run: Run = serde_json::from_value(actual.data["run"].clone())
                    .map_err(|e| e.to_string())?;
                status = finish::status_in(&run, &status.input.evidence_id)?;
                if kind == "finish.completed" {
                    status.phase = JobPhase::Completed;
                }
            }
        }
        Ok(Some(status))
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
        if let Some(status) =
            Self::retry_finish_shared(session, catalog, host, request_id, outbox, &clock)?
        {
            if run.is_some_and(|r| r != status.input.run_id) {
                return Err("finish request Run identity conflict".into());
            }
            if status.phase == JobPhase::Queued {
                let inputs = outbox.read_inputs()?;
                let (_, actual, identity, report, _) = checked(
                    session,
                    catalog,
                    &inputs,
                    host,
                    Some(&status.input.run_id),
                    &clock,
                )?;
                if report.commit != status.input.commit
                    || report.tree != status.input.tree
                    || actual.leases != status.input.fences
                {
                    return Err("queued finish input changed".into());
                }
                return Ok(FinishStart::Queued(FinishTicket {
                    status,
                    session: identity,
                }));
            }
            return Ok(FinishStart::Existing(status));
        }
        let inputs = outbox.read_inputs()?;
        inputs.probe(catalog)?;
        if Self::task_reconciliation_needed_shared(session, catalog, &inputs)? {
            Self::reconcile_pending_shared(session, catalog, &inputs, clock()?, outbox)?;
        }
        let inputs = outbox.read_inputs()?;
        let baseline = current_baseline(catalog, &inputs)?
            .ok_or("finish requires an actually approved published baseline")?;
        let (mut task, mut actual_run, identity, report, _now) =
            checked(session, catalog, &inputs, host, run, &clock)?;
        if task.state == "held" {
            return Err("finish is blocked by the current Hold".into());
        }
        claim_dependency(session, catalog, &inputs, &task, &actual_run, _now)?;
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
        append(session, catalog, outbox, request_id, started, now, |live| {
            let (current_task, current) =
                actual_context(live, catalog, host, Some(&status.input.run_id), now)?;
            if json!(expected_before) != json!((&current_task, &current))
                || live.snapshot_session_id() != identity
                || current.leases != status.input.fences
            {
                return Err("finish Session/fences changed before start".into());
            }
            require_same_review(live, catalog, &task, &status)
        })?;
        Ok(FinishStart::Queued(FinishTicket {
            status,
            session: identity,
        }))
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
        let (mut task, mut run, identity, report, now) = checked(
            session,
            catalog,
            &inputs,
            &status.input.host,
            Some(&status.input.run_id),
            &clock,
        )?;
        if identity != ticket.session
            || report.commit != status.input.commit
            || report.tree != status.input.tree
            || catalog.revision != status.input.authority
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
        append(
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
        let status = &executed.status;
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
        let report = match executed.result {
            Ok(report) => report,
            Err(reason) => {
                return Self::interrupt_finish_shared(
                    session,
                    catalog,
                    &status.input.host,
                    &status.request_id,
                    &reason,
                    outbox,
                    clock,
                )
            }
        };
        finish::check_sealed_logs(&status.input, &report)
            .map_err(|e| uncertain(&status.request_id, e))?;
        let inputs = outbox.read_inputs()?;
        let (task, run, identity, checked, now) = checked(
            session,
            catalog,
            &inputs,
            &status.input.host,
            Some(&status.input.run_id),
            &clock,
        )
        .map_err(|e| uncertain(&status.request_id, e))?;
        if identity != executed.session
            || checked.commit != status.input.commit
            || checked.tree != status.input.tree
            || checked.base != status.input.base
            || catalog.revision != status.input.authority
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
        let actual = append(
            session,
            catalog,
            outbox,
            request_id,
            original,
            now,
            |live| {
                let current = actual_context(live, catalog, host, Some(&status.input.run_id), now)?;
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
mod voluntary_fixture;
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
