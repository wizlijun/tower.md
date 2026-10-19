//! Live adapters over the existing terminal-G5 and sole main-entry proof.
use super::*;
use crate::{
    finish::EvidenceResult,
    recovery_contract::recorded::{IntegrationGateRecorded, MainGateFailureRecorded},
    recovery_git::{self, MergeProofInput, TaskTrailers, TrailerContext},
};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize)]
pub struct TaskTrailersOutcome {
    pub task: String,
    pub run: String,
    pub evidence: String,
    pub trailers: String,
    pub integration_gate: Event,
}
#[derive(Clone, Debug, Serialize)]
pub struct MainMergeDiagnostic {
    pub commit: String,
    pub task: Option<String>,
    pub reason: String,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct MainObservation {
    pub merged: Vec<Event>,
    pub blocked: Vec<MainMergeDiagnostic>,
}
fn git(catalog: &Catalog, args: &[&str]) -> Result<String, String> {
    crate::repo::git_timeout(&catalog.root, args, std::time::Duration::from_secs(10))
}
// A valid parsed Task footer contains its exact Run ID as a contiguous
// string (the parser only trims surrounding whitespace). Search the complete
// actual first-parent chain: this is a negative hint, never a merge proof.
fn main_mentions_runs(
    catalog: &Catalog,
    runs: impl Iterator<Item = impl AsRef<str>>,
) -> Result<bool, String> {
    let mut args = vec![
        "log".to_owned(),
        "--first-parent".into(),
        "--fixed-strings".into(),
        "--format=%H".into(),
    ];
    for run in runs {
        args.push(format!("--grep={}", run.as_ref()));
    }
    args.push(catalog.revision.clone());
    args.push("--".into());
    Ok(!git(
        catalog,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )?
    .is_empty())
}
fn parents(catalog: &Catalog, commit: &str) -> Result<Vec<String>, String> {
    contract::object_id(commit)?;
    Ok(git(catalog, &["rev-list", "--parents", "-n", "1", commit])?
        .split_whitespace()
        .skip(1)
        .map(str::to_owned)
        .collect())
}
// Retain the original UUIDv7 timestamp, and domain-separate the complete finite
// intent. Neither a random ID nor a mutable cache is needed for retry/restart.
fn intent_uuid(request_id: &str, domain: &str, context: &impl Serialize) -> Result<String, String> {
    request_event_eid(request_id, "evidence.recorded")?;
    let source = uuid::Uuid::parse_str(request_id).map_err(|e| e.to_string())?;
    let digest = contract::canonical_hash(&(
        "towermd.task-integration",
        1u32,
        domain,
        request_id,
        context,
    ))?;
    let mut bytes = *source.as_bytes();
    for (index, slot) in bytes.iter_mut().enumerate().skip(6) {
        *slot =
            u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    bytes[6] = (bytes[6] & 15) | 0x70;
    bytes[8] = (bytes[8] & 63) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}
fn gate(
    input: &FrozenFinish,
    status: &FinishStatus,
    id: &str,
    which: Gate,
    now: u64,
) -> Result<GateEvidence, String> {
    Ok(GateEvidence {
        object_type: "Evidence".into(),
        id: format!("E-{id}"),
        kind: "gate".into(),
        gate: which,
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
        plan_opening_eid: status
            .review_context
            .as_ref()
            .map(|c| c.opening.eid.clone()),
        commands_hash: Some(input.commands_hash.clone()),
        fences: input.fences.clone(),
        result: GateResult::Passed,
        review: None,
        diagnostic: None,
        generated: GateGenerated {
            by: "process:towermd-gates".into(),
            at: crate::hub::timestamp(now)
                .map_err(|e| e.to_string())?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        },
        extra: BTreeMap::new(),
    })
}
fn gate_original(
    input: &FrozenFinish,
    evidence: GateEvidence,
    data: Value,
) -> Result<NewEvent, String> {
    contract::validate_gate_layout(&evidence)?;
    let id = evidence.id.strip_prefix("E-").ok_or("gate lacks ID")?;
    Ok(NewEvent {
        eid: request_event_eid(id, "evidence.recorded")?,
        host: input.host.clone(),
        actor: "process:towermd-gates".into(),
        kind: "evidence.recorded".into(),
        layer: "L5".into(),
        obj: evidence.id.clone(),
        data,
        guards: input.fences.clone(),
    })
}
// Generated.at belongs to the first intent. Every other byte must be identical;
// a retry never replans/rebinds an existing pending or accepted gate.
fn original_gate(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    wanted: NewEvent,
    now: u64,
) -> Result<NewEvent, String> {
    let Some(original) = known_original(session, catalog, inputs, &wanted.eid, now)? else {
        return Ok(wanted);
    };
    let mut compared = wanted;
    compared.data["evidence"]["generated"]["at"] =
        original.data["evidence"]["generated"]["at"].clone();
    if compared != original {
        return Err("integration gate original identity/body conflict".into());
    }
    Ok(original)
}
fn append_gate(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    id: &str,
    original: NewEvent,
    now: u64,
    check: impl FnOnce(&ProjectSession) -> Result<(), String>,
) -> Result<Event, String> {
    // A rejected-footer carrier is diagnostic even when the actual acceptance
    // is stale. Its exact receipt may be repaired; it can never become a gate
    // permission or install a Hold through stale projection.
    let failure_diagnostic = if original.data.get("main_gate_failure").is_some() {
        crate::integration_failure::decode_original(&original)?;
        true
    } else {
        false
    };
    let inputs = outbox.read_inputs().map_err(|e| uncertain(id, e))?;
    let history = history_shared(session, catalog, &inputs, now).map_err(|e| uncertain(id, e))?;
    let receipt = history
        .integration_gate_receipt(catalog, &original)
        .map_err(|e| uncertain(id, e))?;
    let result = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        check(live).map_err(HubError::Invalid)?;
        match receipt.as_ref() {
            Some(receipt) => recovery_outbox::append_finish_durable(
                &mut *live.hub,
                outbox,
                original,
                now,
                receipt,
            ),
            None => recovery_outbox::append_durable(&mut *live.hub, outbox, original, now),
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
                "integration gate duplicate lacks complete original receipt",
            ))
        }
        Err(HubError::Invalid(e)) => return Err(e),
        Err(e) => return Err(uncertain(id, e)),
    };
    synchronized.map_err(|e| uncertain(id, e))?;
    if actual.stale && !failure_diagnostic {
        return Err(uncertain(id, "integration gate accepted stale"));
    }
    Ok(actual)
}
pub(super) fn current_run(
    live: &ProjectSession,
    catalog: &Catalog,
    host: &str,
    status: &FinishStatus,
    now: u64,
) -> Result<(Task, Run), String> {
    let input = &status.input;
    if now >= input.merge_deadline
        || input.project_id != catalog.config.project_id()
        || input.common_dir != catalog.common_dir
        || input.host != host
    {
        return Err("integration requires original current project/host/deadline".into());
    }
    let projection = live.projection()?;
    let task: Task = serde_json::from_value(
        projection
            .objects
            .get("Task")
            .and_then(|v| v.get(&input.task_id))
            .cloned()
            .ok_or("integration Task absent")?,
    )
    .map_err(|e| e.to_string())?;
    let run: Run = serde_json::from_value(
        projection
            .objects
            .get("Run")
            .and_then(|v| v.get(&input.run_id))
            .cloned()
            .ok_or("integration Run absent")?,
    )
    .map_err(|e| e.to_string())?;
    crate::tasks::validate_task(&task.spec, catalog)?;
    let expected_fences = std::iter::once(&task.spec.module)
        .chain(task.spec.touches.iter())
        .map(|id| format!("module:{id}"))
        .collect::<BTreeSet<_>>();
    if expected_fences
        != run
            .leases
            .iter()
            .map(|f| f.name.clone())
            .collect::<BTreeSet<_>>()
        || expected_fences.len() != run.leases.len()
    {
        return Err("integration lacks full main Module/touches fences".into());
    }
    if task.state != "verifying"
        || run.state != "verifying"
        || run.task != task.spec.id
        || run.host != host
        || run.worktree != input.root.to_string_lossy()
        || run.branch != format!("t/{}", task.spec.id)
        || task.spec.module != input.module_id
        || run.leases != input.fences
        || task
            .extra
            .get("needs_recheck")
            .is_some_and(|v| v != &Value::Bool(false))
        || task.spec.depends_on.iter().any(|id| {
            projection
                .objects
                .get("Task")
                .and_then(|v| v.get(id))
                .is_none_or(|v| v["state"] != "merged")
        })
        || run.leases.iter().any(|f| {
            live.head
                .leases
                .get(&f.name)
                .is_none_or(|l| l.fence() != *f || l.expires_at <= now)
        })
    {
        return Err(
            "integration requires current verifying Run, dependencies and complete fences".into(),
        );
    }
    require_same_review(live, catalog, &task, status)?;
    Ok((task, run))
}
fn current_live(
    live: &ProjectSession,
    catalog: &Catalog,
    host: &str,
    status: &FinishStatus,
    now: u64,
) -> Result<(Task, Run), String> {
    let (task, run) = current_run(live, catalog, host, status, now)?;
    require_reviews(live, catalog, &task, &status.input, ReviewGate::G4)?;
    Ok((task, run))
}
fn completed_status(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    host: &str,
    run: &Run,
    outbox: &mut RecoveryOutbox,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<(FinishStatus, Event), String> {
    let mut candidates = Vec::new();
    for job in RunFinish::read(run)?
        .jobs
        .values()
        .filter(|job| matches!(job.phase, JobPhase::Completed | JobPhase::Recording))
    {
        let Some(view) = ProjectSession::finish_view_shared(
            session,
            catalog,
            host,
            &job.request_id,
            outbox,
            clock,
        )?
        else {
            continue;
        };
        if view
            .evidence
            .as_ref()
            .is_some_and(|e| e.result == EvidenceResult::Passed)
        {
            candidates.push((
                view.status,
                view.actual
                    .ok_or("passed completion lacks actual receipt")?,
            ));
        }
    }
    candidates.sort_by_key(|(_, actual)| actual.seq);
    candidates
        .pop()
        .ok_or("trailers require an original passed finish completion".into())
}
impl ProjectSession {
    /// Only the actual Task worktree may request trailers. The output is ready
    /// after independent G4 and terminal G5 are durably confirmed, never merged.
    #[allow(clippy::too_many_arguments)]
    pub fn task_trailers_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        task: &str,
        run: Option<&str>,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<TaskTrailersOutcome, String> {
        let inputs = outbox.read_inputs()?;
        let (actual_task, actual_run, _, initial_report, _) =
            checked(session, catalog, &inputs, host, run, &clock)?;
        if actual_task.spec.id != task {
            return Err("trailers Task differs from actual Run".into());
        }
        let (status, completed) =
            completed_status(session, catalog, host, &actual_run, outbox, &clock)?;
        let input = &status.input;
        let inputs = outbox.read_inputs()?;
        let baseline = current_baseline(catalog, &inputs)?.ok_or("trailers baseline absent")?;
        if baseline.baseline != input.baseline {
            return Err("trailers baseline changed since finish".into());
        }
        task_gates::check_g5(catalog, input, &initial_report.commit, &initial_report.base)
            .map_err(|e| format!("terminal G5: {e:?}"))?;
        let now = clock()?;
        let review = Self::continue_shared(session, catalog, &inputs, now, |live| {
            let (task, _) = current_run(live, catalog, host, &status, now)?;
            Ok::<_, String>(require_reviews(live, catalog, &task, input, ReviewGate::G4))
        })
        .map_err(|e| e.to_string())??;
        let review = match review {
            Ok(review) => review,
            Err(reason) => {
                // Mint only from actual accepted verifier phases. P3 keeps its
                // original ledger/identity/guards and validates this finite
                // frozen subject again against the actual candidate/source.
                if high_risk(catalog, &actual_task)? {
                    let inputs = outbox.read_inputs()?;
                    let subject = prove_frozen_g4_subject(
                        session,
                        catalog,
                        &inputs,
                        host,
                        &input.run_id,
                        &input.tree,
                        &clock,
                    )
                    .map_err(|error| match error {
                        FrozenG4Error::Source(reason) => uncertain(&status.request_id, reason),
                        FrozenG4Error::Refused(reason) => reason,
                    })?
                    .ok_or("G4 requires actual original passed verifier phases")?;
                    let id = intent_uuid(
                        &status.request_id,
                        "g4-review-opening",
                        &(input, &completed, &status.review_context),
                    )?;
                    let opened = Self::request_finish_g4_shared(
                        session, catalog, host, &id, subject, outbox, &clock,
                    )?;
                    return Err(format!(
                        "ReviewBlocked G4: {}; Hold {}",
                        reason, opened.receipt.result_ref.hold
                    ));
                }
                return Err(reason);
            }
        };
        let g4 = if let Some((opening, reviewed)) = review {
            let id = intent_uuid(
                &status.request_id,
                "g4",
                &(input, &completed, &opening, &reviewed),
            )?;
            let mut evidence = gate(input, &status, &id, Gate::G4, now)?;
            evidence.review = Some(GateReviewLink {
                opening: opening.clone(),
                reviewed: reviewed.clone(),
                completed_eid: Some(completed.eid.clone()),
            });
            let wanted = gate_original(input, evidence.clone(), json!({"evidence":evidence}))?;
            let original = original_gate(session, catalog, &inputs, wanted, now)?;
            Some(append_gate(
                session,
                catalog,
                outbox,
                &id,
                original,
                now,
                |live| {
                    let final_now = clock()?;
                    if final_now < now {
                        return Err("trailers clock moved backwards".into());
                    }
                    let (task, _) = current_live(live, catalog, host, &status, final_now)?;
                    if require_reviews(live, catalog, &task, input, ReviewGate::G4)?
                        != Some((opening, reviewed))
                    {
                        return Err("G4 review changed before append".into());
                    }
                    Ok(())
                },
            )?)
        } else {
            None
        };
        let inputs = outbox.read_inputs()?;
        let (_, run, identity, report, now) = checked(
            session,
            catalog,
            &inputs,
            host,
            Some(&actual_run.id),
            &clock,
        )?;
        task_gates::check_g5(catalog, input, &report.commit, &report.base)
            .map_err(|e| format!("terminal G5: {e:?}"))?;
        let captured_parents = parents(catalog, &report.commit)?;
        let id = intent_uuid(
            &status.request_id,
            "terminal-g5",
            &(
                input,
                &completed,
                &report.commit,
                &report.tree,
                &report.base,
                &captured_parents,
                &run.leases,
                &status.review_context,
                &g4,
            ),
        )?;
        let mut evidence = gate(input, &status, &id, Gate::G5, now)?;
        evidence.commit = Some(report.commit.clone());
        evidence.tree = Some(report.tree.clone());
        evidence.base = Some(report.base.clone());
        evidence.authority = Some(report.base.clone());
        let recorded = IntegrationGateRecorded {
            evidence: evidence.clone(),
            completed_eid: completed.eid.clone(),
            parents: captured_parents.clone(),
        };
        let wanted = gate_original(input, evidence, json!(recorded))?;
        let original = original_gate(session, catalog, &inputs, wanted, now)?;
        // All mutable Git inputs are actually checked again immediately before
        // the final live Session/fence/source CAS, including message-only amend.
        let (_, current_run, identity2, current_report, end) = checked(
            session,
            catalog,
            &inputs,
            host,
            Some(&actual_run.id),
            &clock,
        )?;
        if identity != identity2
            || report.commit != current_report.commit
            || report.tree != current_report.tree
            || report.base != current_report.base
            || run.leases != current_run.leases
            || parents(catalog, &report.commit)? != captured_parents
        {
            return Err("trailers Git/Session changed before terminal G5".into());
        }
        let integration_gate = append_gate(session, catalog, outbox, &id, original, end, |live| {
            let final_now = clock()?;
            if final_now < end || live.snapshot_session_id() != identity {
                return Err("trailers final clock/Session changed".into());
            }
            current_live(live, catalog, host, &status, final_now).map(|_| ())
        })?;
        let fresh = outbox.read_inputs()?;
        let (_, _, _, output_report, output_at) =
            checked(session, catalog, &fresh, host, Some(&actual_run.id), &clock)?;
        if output_report.commit != report.commit
            || output_report.tree != report.tree
            || output_report.base != report.base
        {
            return Err("trailers HEAD/base changed after terminal receipt".into());
        }
        Self::continue_shared(session, catalog, &fresh, output_at, |live| {
            current_live(live, catalog, host, &status, output_at)
        })
        .map_err(|e| e.to_string())??;
        let epoch = input
            .fences
            .iter()
            .find(|f| f.name == format!("module:{}", input.module_id))
            .ok_or("trailers missing main Module fence")?
            .epoch;
        let trailers = TaskTrailers {
            task: input.task_id.clone(),
            run: input.run_id.clone(),
            module: input.module_id.clone(),
            evidence: input.evidence_id.clone(),
            epoch,
        };
        Ok(TaskTrailersOutcome {
            task: trailers.task.clone(),
            run: trailers.run.clone(),
            evidence: trailers.evidence.clone(),
            trailers: recovery_git::render_task_trailers(&trailers)?,
            integration_gate,
        })
    }
}

// Only the current authorization is optional. Projection/source/decoding errors
// remain errors rather than being mistaken for an expired business context.
fn failure_run_current(
    live: &ProjectSession,
    host: &str,
    original: &Run,
    now: u64,
) -> Result<bool, String> {
    let projection = live.projection()?;
    let Some(raw) = projection
        .objects
        .get("Run")
        .and_then(|runs| runs.get(&original.id))
    else {
        return Ok(false);
    };
    let current: Run = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
    Ok(current.host == host
        && current.task == original.task
        && current.worktree == original.worktree
        && current.branch == original.branch
        && current.leases == original.leases
        && matches!(current.state.as_str(), "running" | "verifying" | "held")
        && current.leases.iter().all(|f| {
            live.head
                .leases
                .get(&f.name)
                .is_some_and(|l| l.fence() == *f && l.expires_at > now)
        }))
}
// Only finite, independently proved prerequisites produce this diagnostic.
// A source/I/O or an unclassified proof error remains an error/diagnostic.
#[allow(
    clippy::too_many_arguments,
    reason = "finite producer requires explicit session, source, original scope and clock"
)]
fn record_main_failure(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    host: &str,
    outbox: &mut RecoveryOutbox,
    claim: &Event,
    candidate: &str,
    trailers: &TaskTrailers,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<bool, String> {
    let id = crate::integration_failure::stable_uuid(claim, candidate, trailers)?;
    let eid = request_event_eid(&id, "evidence.recorded")?;
    let inputs = outbox.read_inputs()?;
    let now = clock()?;
    let history = history_shared(session, catalog, &inputs, now)?;
    let original = if let Some(original) = known_original(session, catalog, &inputs, &eid, now)? {
        let record = crate::integration_failure::decode_original(&original)?;
        if record.claim != *claim || record.candidate != candidate || record.trailers != *trailers {
            return Err("main failure logical scope identity conflict".into());
        }
        history.validate_main_failure_record(catalog, &record)?;
        original
    } else {
        let through = history.main_observed_through(catalog)?;
        let rejection = history
            .main_rejection(catalog, claim, candidate, trailers, through)?
            .ok_or("main proof failure is not a stable prerequisite rejection")?;
        let input = history.main_failure_input(catalog, trailers, through)?;
        let task: Task =
            serde_json::from_value(claim.data["task"].clone()).map_err(|e| e.to_string())?;
        let run: Run =
            serde_json::from_value(claim.data["run"].clone()).map_err(|e| e.to_string())?;
        let evidence = GateEvidence {
            object_type: "Evidence".into(),
            id: format!("E-{id}"),
            kind: "gate".into(),
            gate: Gate::G5,
            project_id: catalog.config.project_id().into(),
            task: task.spec.id,
            run: run.id,
            module: task.spec.module,
            commit: Some(candidate.into()),
            tree: Some(git(
                catalog,
                &["rev-parse", &format!("{candidate}^{{tree}}")],
            )?),
            base: Some(catalog.revision.clone()),
            authority: Some(catalog.revision.clone()),
            baseline: input.as_ref().map(|f| f.baseline.clone()),
            plan_hash: input.as_ref().and_then(|f| f.plan_hash.clone()),
            plan_opening_eid: None,
            commands_hash: input.as_ref().map(|f| f.commands_hash.clone()),
            fences: run.leases,
            result: GateResult::Failed,
            review: None,
            diagnostic: serde_json::to_value(&rejection)
                .map_err(|e| e.to_string())?
                .as_str()
                .map(str::to_owned),
            generated: GateGenerated {
                by: "process:towermd-gates".into(),
                at: crate::hub::timestamp(now)
                    .map_err(|e| e.to_string())?
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            },
            extra: BTreeMap::new(),
        };
        let mut record = MainGateFailureRecorded {
            schema: 1,
            evidence,
            hold: Value::Null,
            claim: claim.clone(),
            candidate: candidate.into(),
            authority: catalog.revision.clone(),
            trailers: trailers.clone(),
            observed_through_seq: through,
            rejection,
        };
        let provenance = json!(crate::integration_failure::context(&record));
        record
            .evidence
            .extra
            .insert("main_failure".into(), provenance);
        record.hold = crate::integration_failure::hold(&record)?;
        let original = NewEvent {
            eid,
            host: host.into(),
            actor: "process:towermd-gates".into(),
            kind: "evidence.recorded".into(),
            layer: "L5".into(),
            obj: record.evidence.id.clone(),
            guards: record.evidence.fences.clone(),
            data: json!({"evidence":record.evidence,"main_gate_failure":record}),
        };
        let record = crate::integration_failure::decode_original(&original)?;
        history.validate_main_failure_record(catalog, &record)?;
        original
    };
    let actual_claim_run: Run =
        serde_json::from_value(claim.data["run"].clone()).map_err(|e| e.to_string())?;
    // An actual accepted original is only durability maintenance. Expiry cannot
    // revoke its receipt, and repairing it cannot authorize a new fact.
    let accepted = history
        .integration_gate_receipt(catalog, &original)?
        .is_some();
    if !accepted {
        let current = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
            failure_run_current(live, host, &actual_claim_run, now)
        })
        .map_err(|e| e.to_string())??;
        if !current {
            return Ok(false);
        }
    }
    append_gate(session, catalog, outbox, &id, original, now, |live| {
        if accepted {
            return Ok(());
        }
        let final_now = clock()?;
        if final_now < now {
            return Err("main failure clock moved backwards".into());
        }
        if !failure_run_current(live, host, &actual_claim_run, final_now)? {
            return Err("main failure no longer has the same actual current Run/fences".into());
        }
        Ok(())
    })?;
    Ok(true)
}
// Only the sole proof below can mint this finite historical-fact scope. It
// does not renew/reacquire leases, authorize execution, or accept caller flags.
struct EstablishedMain {
    session: SnapshotSessionId,
    task: Task,
    run: Run,
}
impl EstablishedMain {
    fn capture(
        live: &ProjectSession,
        catalog: &Catalog,
        host: &str,
        status: &FinishStatus,
        proved: &NewEvent,
        now: u64,
    ) -> Result<Self, String> {
        if now < status.input.merge_deadline
            || status.input.project_id != catalog.config.project_id()
            || status.input.common_dir != catalog.common_dir
            || status.input.host != host
        {
            return Err("historical main fact lacks its exact expired original scope".into());
        }
        let state = live.projection()?;
        let run: Run = serde_json::from_value(
            state
                .objects
                .get("Run")
                .and_then(|runs| runs.get(&status.input.run_id))
                .cloned()
                .ok_or("historical Run absent")?,
        )
        .map_err(|e| e.to_string())?;
        let task: Task = serde_json::from_value(
            state
                .objects
                .get("Task")
                .and_then(|tasks| tasks.get(&status.input.task_id))
                .cloned()
                .ok_or("historical Task absent")?,
        )
        .map_err(|e| e.to_string())?;
        let original_task: Task =
            serde_json::from_value(proved.data["task"].clone()).map_err(|e| e.to_string())?;
        if !finish::conflicting_runs(state, &run.task, &run.id).is_empty() {
            return Err("historical_merge_conflict: another active Run owns this Task".into());
        }
        if task.state == "merged"
            || !matches!(
                run.state.as_str(),
                "running" | "verifying" | "held" | "expired" | "released"
            )
            || json!(task.spec) != json!(original_task.spec)
            || run.task != task.spec.id
            || run.host != host
            || run.worktree != status.input.root.to_string_lossy()
            || run.branch != format!("t/{}", task.spec.id)
            || run.leases != status.input.fences
            || proved.data["proof"]["claim"]["data"]["run"]["id"] != run.id
            || proved.data["proof"]["claim"]["data"]["run"]["leases"] != json!(run.leases)
            || finish::run_deadline(&run)? != Some(status.input.merge_deadline)
            || task
                .extra
                .get("needs_recheck")
                .is_some_and(|value| value != &Value::Bool(false))
        {
            return Err("historical main fact current Task/Run identity differs".into());
        }
        Ok(Self {
            session: live.snapshot_session_id(),
            task,
            run,
        })
    }
    fn check(&self, live: &ProjectSession) -> Result<(), String> {
        let state = live.projection()?;
        if live.snapshot_session_id() != self.session
            || state
                .objects
                .get("Task")
                .and_then(|tasks| tasks.get(&self.task.spec.id))
                != Some(&json!(self.task))
            || state
                .objects
                .get("Run")
                .and_then(|runs| runs.get(&self.run.id))
                != Some(&json!(self.run))
            || !finish::conflicting_runs(state, &self.run.task, &self.run.id).is_empty()
        {
            return Err("historical main fact final Task/Run CAS changed".into());
        }
        Ok(())
    }
}
fn append_merge(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    original: NewEvent,
    status: Option<&FinishStatus>,
    host: &str,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<Event, String> {
    let id = original.eid.clone();
    let inputs = outbox.read_inputs().map_err(|e| uncertain(&id, e))?;
    let now = clock().map_err(|e| uncertain(&id, e))?;
    let history = history_shared(session, catalog, &inputs, now).map_err(|e| uncertain(&id, e))?;
    // This capability comes only from the proved actual outer archive/tail.
    // It is solely durability repair, not a renewed integration permission.
    let receipt = history
        .merge_receipt(catalog, &original)
        .map_err(|e| uncertain(&id, e))?;
    let commit = original.data["commit"]
        .as_str()
        .ok_or("canonical merge missing commit")?;
    let trailers = match recovery_git::parse_trailer_context(&git(
        catalog,
        &["show", "--no-patch", "--format=%B", commit],
    )?)? {
        TrailerContext::Task(trailers) => trailers,
        _ => return Err("canonical merge lacks actual Task trailers".into()),
    };
    let proved = recovery_git::prove_merge(
        catalog,
        &history,
        &MergeProofInput {
            commit: commit.into(),
            trailers,
        },
    )?
    .into_event();
    if proved != original {
        return Err("pending canonical merge differs from sole proof".into());
    }
    let established = if receipt.is_none() {
        let status = status.ok_or("new canonical merge has no current original Run proof")?;
        ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
            if now >= status.input.merge_deadline {
                EstablishedMain::capture(live, catalog, host, status, &proved, now).map(Some)
            } else {
                current_live(live, catalog, host, status, now).map(|_| None)
            }
        })
        .map_err(|e| uncertain(&id, e))??
    } else {
        None
    };
    let end = clock()?;
    if end < now {
        return Err(uncertain(&id, "observer clock moved backwards"));
    }
    let result = ProjectSession::continue_shared(session, catalog, &inputs, end, |live| {
        if receipt.is_none() {
            let status = status.ok_or_else(|| {
                HubError::Invalid("new canonical merge has no current Run proof".into())
            })?;
            let final_now = clock().map_err(HubError::Invalid)?;
            if final_now < end {
                return Err(HubError::Invalid("observer clock moved backwards".into()));
            }
            if let Some(established) = &established {
                established.check(live).map_err(HubError::Invalid)?;
            } else {
                current_live(live, catalog, host, status, final_now).map_err(HubError::Invalid)?;
            }
        }
        match receipt.as_ref() {
            Some(receipt) => recovery_outbox::append_merge_durable(
                &mut *live.hub,
                outbox,
                original,
                end,
                receipt,
            ),
            None => recovery_outbox::append_durable(&mut *live.hub, outbox, original, end),
        }
    })
    .map_err(|e| uncertain(&id, e))?;
    // As in finish, accepted business is synchronized even when confirm failed.
    let synchronized = outbox.read_inputs().and_then(|fresh| {
        ProjectSession::sync_shared(session, catalog, &fresh, end).map_err(|e| e.to_string())
    });
    let actual = match result {
        Ok(AppendReceipt::Event(actual)) => *actual,
        Ok(AppendReceipt::Duplicate { .. }) => {
            return Err(uncertain(
                &id,
                "canonical merge duplicate lacks original receipt",
            ))
        }
        Err(HubError::Invalid(e)) => return Err(e),
        Err(e) => return Err(uncertain(&id, e)),
    };
    synchronized.map_err(|e| uncertain(&id, e))?;
    if actual.stale {
        return Err(uncertain(&id, "canonical merge accepted stale"));
    }
    Ok(actual)
}
// Cleanup is derived only from an actual accepted, uniquely proved canonical
// and its installed ended Run. It neither changes Task state nor releases a
// lease acquired later by another holder/epoch.
fn cleanup_merged_leases(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    host: &str,
    outbox: &mut RecoveryOutbox,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<(), String> {
    let inputs = outbox.read_inputs()?;
    let now = clock()?;
    let needed = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        Ok::<_, String>(
            live.projection()?
                .objects
                .get("Run")
                .into_iter()
                .flat_map(|runs| runs.values())
                .any(|raw| {
                    raw["state"] == "ended"
                        && raw["host"] == host
                        && serde_json::from_value::<Run>(raw.clone()).is_ok_and(|run| {
                            run.leases.iter().any(|f| {
                                live.head
                                    .leases
                                    .get(&f.name)
                                    .is_some_and(|lease| lease.fence() == *f)
                            })
                        })
                }),
        )
    })
    .map_err(|e| e.to_string())??;
    if !needed {
        return Ok(());
    }
    let history = history_shared(session, catalog, &inputs, now)?;
    let mut canonical = Vec::new();
    for event in history
        .actual_outer_events(catalog)?
        .into_iter()
        .filter(|event| event.kind == "merge.done" && !event.stale && event.host == host)
    {
        let original = recovery_outbox::original_of(event);
        let receipt = history
            .merge_receipt(catalog, &original)?
            .ok_or("merge cleanup lacks actual canonical receipt")?;
        receipt.event_for(catalog.config.project_id(), &original)?;
        let run: Run =
            serde_json::from_value(event.data["run"].clone()).map_err(|e| e.to_string())?;
        if run.state != "ended" || run.host != host || run.task != event.obj {
            return Err("canonical cleanup Run identity differs".into());
        }
        canonical.push((event.clone(), run));
    }
    let finished = clock()?;
    if finished < now {
        return Err("merge cleanup clock moved backwards".into());
    }
    let result = ProjectSession::continue_shared(session, catalog, &inputs, finished, |live| {
        for (event, run) in &canonical {
            let projection = live.projection().map_err(HubError::Invalid)?;
            let Some(raw) = projection
                .objects
                .get("Run")
                .and_then(|runs| runs.get(&run.id))
            else {
                continue;
            };
            let current: Run = serde_json::from_value(raw.clone())
                .map_err(|e| HubError::Invalid(e.to_string()))?;
            if current.state != "ended"
                || current.id != run.id
                || current.task != run.task
                || current.host != run.host
                || current.branch != run.branch
                || current.worktree != run.worktree
                || current.leases != run.leases
            {
                continue;
            }
            for fence in &run.leases {
                // Re-read head before every independently atomic release. A
                // replaced epoch is skipped; Hub still CAS-checks each fence.
                let head = live
                    .hub
                    .subscribe(live.hub.contiguous_seq(), finished)?
                    .head;
                if !head
                    .leases
                    .get(&fence.name)
                    .is_some_and(|lease| lease.fence() == *fence)
                {
                    continue;
                }
                match live.hub.lease_release(
                    crate::hub_protocol::ReleaseLease {
                        host: host.into(),
                        fences: vec![fence.clone()],
                    },
                    finished,
                ) {
                    Ok(_) | Err(HubError::FenceExpired { .. }) => {}
                    Err(error) => return Err(HubError::Uncertain(uncertain(&event.eid, error))),
                }
            }
        }
        Ok::<_, HubError>(())
    })
    .map_err(|e| e.to_string())?;
    // Even a partially successful release is an actual system transition.
    // Reconcile it before returning the original error; it is not a new event.
    let synchronized = outbox.read_inputs().and_then(|fresh| {
        ProjectSession::sync_shared(session, catalog, &fresh, finished).map_err(|e| e.to_string())
    });
    result.map_err(|e| e.to_string())?;
    synchronized
}
// A complete confirmed line may exist while its fsync is still uncertain.
// Those originals are repair work too, but only an actual-history capability
// below can accept their receipt; JSON presence never authorizes an append.
fn integration_repairs(
    inputs: &RecoveryInputs,
    host: &str,
) -> Result<BTreeMap<String, NewEvent>, String> {
    let mut originals = BTreeMap::new();
    let in_scope = |kind: &str, event_host: &str, actor: &str, layer: &str, data: &Value| {
        event_host == host
            && (kind == "merge.done"
                || (kind == "evidence.recorded"
                    && actor == "process:towermd-gates"
                    && layer == "L5"
                    && data["evidence"]["kind"] == "gate"))
    };
    let pending = inputs
        .pending()
        .iter()
        .filter(|event| {
            in_scope(
                &event.kind,
                &event.host,
                &event.actor,
                &event.layer,
                &event.data,
            )
        })
        .cloned();
    // Filter before the complete receipt predicate: ordinary Task/Ask rows
    // neither need confirmation repair here nor a confirmed-list lookup.
    let confirmed = inputs
        .confirmed()
        .iter()
        .filter(|event| {
            in_scope(
                &event.kind,
                &event.host,
                &event.actor,
                &event.layer,
                &event.data,
            )
        })
        .filter(|actual| !inputs.receipt_confirmed(actual))
        .map(recovery_outbox::original_of);
    for original in pending.chain(confirmed) {
        if originals
            .get(&original.eid)
            .is_some_and(|old| old != &original)
        {
            return Err("integration repair original identity/body conflict".into());
        }
        originals.insert(original.eid.clone(), original);
    }
    Ok(originals)
}
impl ProjectSession {
    /// A scheduling hint only. No-finish Runs must be observed too, since a
    /// forged Evidence footer must be diagnosed rather than silently ignored.
    pub fn has_task_merge_candidates(&self) -> Result<bool, String> {
        Ok(self
            .projection()?
            .objects
            .get("Run")
            .into_iter()
            .flat_map(|runs| runs.values())
            .any(|run| {
                matches!(
                    run["state"].as_str(),
                    Some("running" | "verifying" | "held")
                ) || (matches!(run["state"].as_str(), Some("expired" | "released"))
                    && run.get("finish").is_some()
                    && self
                        .projection()
                        .ok()
                        .and_then(|state| state.objects.get("Task"))
                        .and_then(|tasks| run["task"].as_str().and_then(|id| tasks.get(id)))
                        .is_some_and(|task| task["state"] != "merged"))
                    || (run["state"] == "ended"
                        && serde_json::from_value::<Run>(run.clone()).is_ok_and(|run| {
                            run.leases.iter().any(|f| {
                                self.head
                                    .leases
                                    .get(&f.name)
                                    .is_some_and(|lease| lease.fence() == *f)
                            })
                        }))
            }))
    }

    /// Normal observer: the immutable candidate set comes from each original
    /// Run authority, never from the newest snapshot commit or a client report.
    /// The caller serializes producers and runs this before its expiry phase.
    pub fn observe_task_merges_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<MainObservation, String> {
        // Accepted gate intents may only repair their exact original receipt.
        // Unknown/pending-only gates are left to their original finite producer.
        let gate_inputs = outbox.read_inputs()?;
        let pending_gates = integration_repairs(&gate_inputs, host)?
            .into_values()
            .filter(|original| original.kind == "evidence.recorded")
            .collect::<Vec<_>>();
        for original in pending_gates {
            let evidence: GateEvidence = serde_json::from_value(original.data["evidence"].clone())
                .map_err(|e| e.to_string())?;
            if !matches!(evidence.gate, Gate::G4 | Gate::G5) {
                continue;
            }
            let inputs = outbox.read_inputs()?;
            let now = clock()?;
            let history = history_shared(session, catalog, &inputs, now)?;
            if history
                .integration_gate_receipt(catalog, &original)?
                .is_some()
            {
                let id = original.eid.clone();
                // The private exact receipt causes append_durable to persist
                // only the original acceptance, never a new Hub append.
                append_gate(session, catalog, outbox, &id, original, now, |_| Ok(()))?;
            }
        }
        let inputs = outbox.read_inputs()?;
        let now = clock()?;
        let merge_repairs = integration_repairs(&inputs, host)?
            .into_values()
            .filter(|original| original.kind == "merge.done")
            .collect::<Vec<_>>();
        let has_pending_merge = !merge_repairs.is_empty();
        // Accepted originals retain priority. Without a local canonical intent,
        // do not pay for all verifier originals before the negative Git scan.
        let jobs = if has_pending_merge {
            Self::finish_jobs_shared(session, catalog, &inputs, host, now)?
        } else {
            Vec::new()
        };
        let mut result = MainObservation::default();
        let mut seen = BTreeSet::new();
        // Confirm-failed accepted merge has already made the Run ended and may
        // have removed its mutable Job projection. Original intent wins first.
        for original in &merge_repairs {
            let status = jobs
                .iter()
                .find(|job| original.data["run"]["id"] == job.input.run_id);
            if let Some(status) = status.filter(|status| now >= status.input.merge_deadline) {
                let repair_history = history_shared(session, catalog, &inputs, now)?;
                if repair_history.merge_receipt(catalog, original)?.is_none() {
                    let conflicts = Self::continue_shared(session, catalog, &inputs, now, |live| {
                        Ok::<_, String>(finish::conflicting_runs(
                            live.projection()?,
                            &status.input.task_id,
                            &status.input.run_id,
                        ))
                    })
                    .map_err(|e| e.to_string())??;
                    if !conflicts.is_empty() {
                        // A conflict is a refusal only after the pending body
                        // has passed the same sole proof; schema/Git errors
                        // remain errors and cannot hide behind the conflict.
                        let commit = original.data["commit"]
                            .as_str()
                            .ok_or("pending canonical lacks commit")?;
                        let TrailerContext::Task(trailers) = recovery_git::parse_trailer_context(
                            &git(catalog, &["show", "--no-patch", "--format=%B", commit])?,
                        )?
                        else {
                            return Err("pending canonical lacks Task trailers".into());
                        };
                        if recovery_git::prove_merge(
                            catalog,
                            &repair_history,
                            &MergeProofInput {
                                commit: commit.into(),
                                trailers,
                            },
                        )?
                        .into_event()
                            != *original
                        {
                            return Err("pending canonical differs from sole proof".into());
                        }
                        result.blocked.push(MainMergeDiagnostic {
                            commit: commit.into(),
                            task: Some(status.input.task_id.clone()),
                            reason: format!("historical_merge_conflict: active Runs {conflicts:?}"),
                        });
                        continue;
                    }
                }
            }
            let actual = append_merge(
                session,
                catalog,
                outbox,
                original.clone(),
                status,
                host,
                &clock,
            )?;
            seen.insert(actual.eid.clone());
            result.merged.push(actual);
        }
        let inputs = outbox.read_inputs()?;
        let now = clock()?;
        let active_runs = Self::continue_shared(session, catalog, &inputs, now, |live| {
            let mut runs = BTreeMap::new();
            for value in live
                .projection()?
                .objects
                .get("Run")
                .into_iter()
                .flat_map(|runs| runs.values())
            {
                let run: Run = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
                if run.host == host
                    && (matches!(run.state.as_str(), "running" | "verifying" | "held")
                        || (matches!(run.state.as_str(), "expired" | "released")
                            && run.extra.contains_key("finish")))
                {
                    runs.insert(run.id.clone(), run);
                }
            }
            Ok::<_, String>(runs)
        })
        .map_err(|e| e.to_string())??;
        if active_runs.is_empty() {
            // No local Run can authorize a new integration. This is an actual
            // synchronized projection read, not an outbox/cache absence guess;
            // accepted original repair above still ran before this return.
            cleanup_merged_leases(session, catalog, host, outbox, &clock)?;
            let fresh = outbox.read_inputs()?;
            Self::continue_shared(session, catalog, &fresh, clock()?, |_| ())
                .map_err(|e| e.to_string())?;
            return Ok(result);
        }
        if !has_pending_merge && !main_mentions_runs(catalog, active_runs.keys())? {
            cleanup_merged_leases(session, catalog, host, outbox, &clock)?;
            let fresh = outbox.read_inputs()?;
            // This NoOp covers exactly the physical input set read before the
            // scan, including the absence of new pending repair work.
            inputs.validate_task_writes(&fresh, &[])?;
            let finished = clock()?;
            if finished < now {
                return Err("main candidate scan clock moved backwards".into());
            }
            Self::continue_shared(session, catalog, &fresh, finished, |live| {
                let mut current = BTreeSet::new();
                for raw in live
                    .projection()?
                    .objects
                    .get("Run")
                    .into_iter()
                    .flat_map(|runs| runs.values())
                {
                    let run: Run =
                        serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
                    if run.host == host
                        && (matches!(run.state.as_str(), "running" | "verifying" | "held")
                            || (matches!(run.state.as_str(), "expired" | "released")
                                && run.extra.contains_key("finish")))
                    {
                        current.insert(run.id);
                    }
                }
                if current != active_runs.keys().cloned().collect() {
                    return Err("main candidate scan active Run source changed".to_owned());
                }
                Ok::<_, String>(())
            })
            .map_err(|e| e.to_string())??;
            return Ok(result);
        }
        let jobs = if has_pending_merge {
            jobs
        } else {
            Self::finish_jobs_shared(session, catalog, &inputs, host, now)?
        };
        let history = history_shared(session, catalog, &inputs, now)?;
        let active = active_runs.keys().cloned().collect::<BTreeSet<_>>();
        let claims = history.main_task_claims(catalog, host, &active)?;
        let mut commits = BTreeSet::new();
        for claim in &claims {
            let id = claim.data["run"]["id"]
                .as_str()
                .ok_or("claim has no Run ID")?;
            if !active_runs.contains_key(id) {
                continue;
            }
            let mut roots = jobs
                .iter()
                .filter(|job| job.input.run_id == id)
                .map(|job| job.input.authority.as_str())
                .collect::<BTreeSet<_>>();
            if let Some(source) = claim.data["run"]["task_card"]["revision"]
                .as_str()
                .filter(|source| contract::object_id(source).is_ok())
            {
                roots.insert(source);
            }
            // Legacy optional Taskcard metadata is not a new business gate.
            // Without any safe lower bound inspect actual main, then bind each
            // footer to the same proved claim. This fallback grants no proof.
            if roots.is_empty() {
                commits.extend(
                    git(
                        catalog,
                        &["rev-list", "--first-parent", "--reverse", &catalog.revision],
                    )?
                    .lines()
                    .map(str::to_owned),
                );
            }
            for root in roots {
                contract::object_id(root)?;
                for commit in git(
                    catalog,
                    &[
                        "rev-list",
                        "--first-parent",
                        "--reverse",
                        &format!("{root}..{}", catalog.revision),
                    ],
                )?
                .lines()
                {
                    commits.insert(commit.to_owned());
                }
            }
        }
        let ordered = if commits.is_empty() {
            Vec::new()
        } else {
            git(
                catalog,
                &["rev-list", "--first-parent", "--reverse", &catalog.revision],
            )?
            .lines()
            .filter(|commit| commits.contains(*commit))
            .map(str::to_owned)
            .collect()
        };
        for commit in ordered {
            let message = git(catalog, &["show", "--no-patch", "--format=%B", &commit])?;
            let trailers = match recovery_git::parse_trailer_context(&message) {
                Ok(TrailerContext::Task(trailers)) => trailers,
                Ok(_) => continue,
                Err(reason) => {
                    result.blocked.push(MainMergeDiagnostic {
                        commit,
                        task: None,
                        reason,
                    });
                    continue;
                }
            };
            let Some(claim) = claims.iter().find(|claim| {
                claim.data["run"]["id"] == trailers.run && claim.data["task"]["id"] == trailers.task
            }) else {
                continue;
            };
            if !active_runs.contains_key(&trailers.run) {
                continue;
            }
            let job = jobs.iter().find(|job| {
                job.input.run_id == trailers.run
                    && job.input.task_id == trailers.task
                    && job.input.evidence_id == trailers.evidence
            });
            let eid = crate::events::derived_eid(&commit, "merge.done")?;
            if seen.contains(&eid) {
                continue;
            }
            let inputs = outbox.read_inputs()?;
            let now = clock()?;
            let history = history_shared(session, catalog, &inputs, now)?;
            let finite_rejection = history.main_rejection(
                catalog,
                claim,
                &commit,
                &trailers,
                history.main_observed_through(catalog)?,
            )?;
            if let Some(job) =
                job.filter(|job| finite_rejection.is_none() && now < job.input.merge_deadline)
            {
                // Source errors remain outer errors. A stable live gate refusal
                // is a diagnostic so the caller can continue its existing
                // expiry phase; no canonical append or lease action occurs.
                let current = Self::continue_shared(session, catalog, &inputs, now, |live| {
                    current_live(live, catalog, host, job, now)
                })
                .map_err(|e| e.to_string())?;
                if let Err(reason) = current {
                    result.blocked.push(MainMergeDiagnostic {
                        commit,
                        task: Some(trailers.task),
                        reason,
                    });
                    continue;
                }
            }
            let proved = match recovery_git::prove_merge(
                catalog,
                &history,
                &MergeProofInput {
                    commit: commit.clone(),
                    trailers: trailers.clone(),
                },
            ) {
                Ok(proved) => proved.into_event(),
                Err(reason) => {
                    let through = history.main_observed_through(catalog)?;
                    if history
                        .main_rejection(catalog, claim, &commit, &trailers, through)?
                        .is_some()
                    {
                        record_main_failure(
                            session, catalog, host, outbox, claim, &commit, &trailers, &clock,
                        )?;
                    }
                    result.blocked.push(MainMergeDiagnostic {
                        commit,
                        task: Some(trailers.task),
                        reason,
                    });
                    continue;
                }
            };
            let job = job.ok_or("proved merge has no current original finish Job")?;
            if now < job.input.merge_deadline {
                let baseline = crate::baseline_current::current_baseline_in_history(
                    catalog, &inputs, &history,
                )?
                .ok_or("main observer baseline absent")?;
                if baseline.baseline != job.input.baseline {
                    return Err("main observer baseline changed since finish".into());
                }
            }
            // The current Run's complete identity must still be the original
            // claim; live gates below do not borrow a different worktree Run.
            let current = active_runs
                .get(&trailers.run)
                .ok_or("main observer Run disappeared")?;
            if claim.data["run"]["leases"] != json!(current.leases)
                || claim.data["run"]["worktree"] != current.worktree
                || claim.data["run"]["branch"] != current.branch
            {
                return Err("main observer current Run differs from original claim".into());
            }
            if now >= job.input.merge_deadline {
                let permitted = Self::continue_shared(session, catalog, &inputs, now, |live| {
                    EstablishedMain::capture(live, catalog, host, job, &proved, now)
                })
                .map_err(|e| e.to_string())?;
                if let Err(reason) = permitted {
                    result.blocked.push(MainMergeDiagnostic {
                        commit,
                        task: Some(trailers.task),
                        reason,
                    });
                    continue;
                }
            }
            let actual = append_merge(session, catalog, outbox, proved, Some(job), host, &clock)?;
            seen.insert(actual.eid.clone());
            result.merged.push(actual);
        }
        cleanup_merged_leases(session, catalog, host, outbox, &clock)?;
        // Close even a no-op observer over the actual current source/Session.
        let fresh = outbox.read_inputs()?;
        Self::continue_shared(session, catalog, &fresh, clock()?, |_| ())
            .map_err(|e| e.to_string())?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "integration/confirmation_tests.rs"]
mod confirmation_tests;
