//! Fixed-deadline maintenance over actual originals, never a Job cache report.
use super::*;
use crate::hub_protocol::{ReleaseLease, RenewLease};
use serde::Serialize;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Default, Serialize)]
pub struct FinishMaintenance {
    pub renewed: Vec<Event>,
    pub expired: Vec<FinishExpiryOutcome>,
    // Process-local preparation; serialization cannot create or transport it.
    #[serde(skip)]
    pub expirations: Vec<FinishExpiry>,
}
pub struct FinishExpiry {
    project: String,
    common: PathBuf,
    configuration: String,
    session: SnapshotSessionId,
    task: Task,
    run: Run,
    deadline: u64,
    started: FinishStatus,
    evidence_ids: Vec<String>,
    authority_unavailable: Option<String>,
}
impl FinishExpiry {
    pub fn run_id(&self) -> &str {
        &self.run.id
    }
    pub fn evidence_ids(&self) -> &[String] {
        &self.evidence_ids
    }
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct FinishExpiryOutcome {
    pub run: String,
    pub evidence_ids: Vec<String>,
    pub deadline: u64,
    pub actual: Event,
}

fn task_run(live: &ProjectSession, id: &str) -> Result<(Task, Run), String> {
    let state = live.projection()?;
    let run: Run = serde_json::from_value(
        state
            .objects
            .get("Run")
            .and_then(|runs| runs.get(id))
            .cloned()
            .ok_or("finish maintenance Run absent")?,
    )
    .map_err(|e| e.to_string())?;
    let task: Task = serde_json::from_value(
        state
            .objects
            .get("Task")
            .and_then(|tasks| tasks.get(&run.task))
            .cloned()
            .ok_or("finish maintenance Task absent")?,
    )
    .map_err(|e| e.to_string())?;
    Ok((task, run))
}
fn active(run: &Run) -> bool {
    matches!(run.state.as_str(), "running" | "verifying" | "held")
}
fn expiry_id(status: &FinishStatus) -> Result<String, String> {
    request_event_eid(&status.input.run_id, "task.released")
}
fn expiry_outcome(actual: Event) -> Result<FinishExpiryOutcome, String> {
    finish::validate_expiry_event(&actual)?;
    let run: Run = serde_json::from_value(actual.data["run"].clone()).map_err(|e| e.to_string())?;
    let lifecycle = RunFinish::read(&run)?;
    let first = lifecycle
        .jobs
        .values()
        .find(|job| actual.data["finish_expiry"]["started_eid"] == job.started_eid)
        .ok_or("expired Run has no original finish lifecycle")?;
    if actual.kind != "task.released"
        || actual.actor != "process:towermd-daemon"
        || actual.layer != "L5"
        || actual.obj != run.task
        || actual.host != run.host
        || !actual.guards.is_empty()
        || actual.stale
        || actual.legacy
        || actual.eid != expiry_id(first)?
        || run.state != "expired"
        || actual.data["task"]["state"] != "ready"
        || actual.data["task"]["id"] != run.task
        || actual.data["finish_expiry"]["schema"] != 1
        || actual.data["finish_expiry"]["entered_verifying_at"] != lifecycle.entered_verifying_at
        || actual.data["finish_expiry"]["merge_deadline"] != lifecycle.merge_deadline
        || actual.data["finish_expiry"]["started_eid"] != first.started_eid
        || crate::project_session::event_millis(&actual)? < lifecycle.merge_deadline
    {
        return Err("invalid actual fixed-deadline expiry receipt".into());
    }
    Ok(FinishExpiryOutcome {
        run: run.id,
        evidence_ids: lifecycle.jobs.keys().cloned().collect(),
        deadline: lifecycle.merge_deadline,
        actual,
    })
}

fn prove_expiry_outcome(
    history: &crate::recovery_git::VerifiedRecoveryHistory,
    catalog: &Catalog,
    actual: Event,
) -> Result<FinishExpiryOutcome, String> {
    let original = recovery_outbox::original_of(&actual);
    // Rebuild's outer guarded started may be stale while the original
    // accepted role remains valid historical receipt material. It grants no
    // execution/cleanup permission. Require the actual outer expiry as well.
    if !history.actual_outer_events(catalog)?.contains(&&actual) {
        return Err("expiry result lacks its actual outer role".into());
    }
    let accepted = history
        .expiry_original(catalog, &original)?
        .ok_or("expiry result lacks accepted original provenance")?;
    expiry_outcome(accepted.clone())
}

/// Read only the expiry belonging to this exact durable lifecycle. The caller
/// supplies the actual outer schedule already proved in its finite lookup;
/// no cache or confirmed-only row can supply the Event displayed here.
pub(super) fn expiry_for_view(
    history: &crate::recovery_git::VerifiedRecoveryHistory,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    status: &FinishStatus,
    actual_outer: &[&Event],
) -> Result<Option<Event>, String> {
    let eid = expiry_id(status)?;
    let Some(actual) = actual_outer.iter().copied().find(|event| event.eid == eid) else {
        return Ok(None);
    };
    let original = recovery_outbox::original_of(actual);
    if read_original(inputs, &eid)?
        .as_ref()
        .is_some_and(|stored| stored != &original)
    {
        return Err("finish expiry original UUID/body conflict".into());
    }
    // The outer membership above came from the same VerifiedHistory. Reuse
    // its finite original validator without another history/source capture.
    let accepted = history
        .expiry_original(catalog, &original)?
        .ok_or("finish expiry view lacks original acceptance")?;
    let proved = expiry_outcome(accepted)?;
    let run: Run = serde_json::from_value(proved.actual.data["run"].clone())
        .map_err(|error| error.to_string())?;
    let mut recorded = finish::status_in(&run, &status.input.evidence_id)?;
    // Completed's persisted Run phase is Recording; the existing reader
    // derives Completed only from its separately proved terminal receipt.
    if recorded.phase == JobPhase::Recording && status.phase == JobPhase::Completed {
        recorded.phase = JobPhase::Completed;
    }
    if recorded != *status
        || run.id != status.input.run_id
        || proved.run != status.input.run_id
        || run.task != status.input.task_id
        || run.host != status.input.host
        || status.input.project_id != catalog.config.project_id()
        || status.input.common_dir != catalog.common_dir
        || proved.deadline != status.input.merge_deadline
        || !proved.evidence_ids.contains(&status.input.evidence_id)
    {
        return Err("finish expiry view differs from the complete original status/input".into());
    }
    Ok(Some(proved.actual))
}

pub(super) fn historical_view(
    history: &crate::recovery_git::VerifiedRecoveryHistory,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    projection: &crate::snapshot::StateProjection,
    host: &str,
    request_id: &str,
) -> Result<Option<finish::FinishView>, String> {
    for receipt in history.historical_expiry_receipts(catalog)? {
        let actual_expiry = receipt.original();
        if actual_expiry.host != host {
            continue;
        }
        let run: Run =
            serde_json::from_value(actual_expiry.data["run"].clone()).map_err(|e| e.to_string())?;
        let Some(mut status) = RunFinish::read(&run)?
            .jobs
            .into_values()
            .find(|status| status.request_id == request_id)
        else {
            continue;
        };
        let phase_ids = [
            status.started_eid.clone(),
            request_event_eid(request_id, "finish.running")?,
            status.completed_eid.clone(),
            request_event_eid(request_id, "finish.interrupted")?,
        ];
        for actual in receipt
            .context()
            .iter()
            .filter(|event| phase_ids.contains(&event.eid))
            .chain(std::iter::once(actual_expiry))
        {
            if read_original(inputs, &actual.eid)?
                .as_ref()
                .is_some_and(|original| original != &recovery_outbox::original_of(actual))
            {
                return Err("historical finish original UUID/body conflict".into());
            }
        }
        let completed = receipt
            .context()
            .iter()
            .find(|event| event.eid == status.completed_eid);
        let interrupted_eid = request_event_eid(request_id, "finish.interrupted")?;
        let interrupted = receipt
            .context()
            .iter()
            .find(|event| event.eid == interrupted_eid);
        let (actual, evidence, report) = if let Some(actual) = completed {
            let completed: Completed =
                serde_json::from_value(actual.data.clone()).map_err(|e| e.to_string())?;
            contract::validate_finish_report(
                &status.input,
                &actual.eid,
                &completed.evidence,
                &completed.report,
            )?;
            status.phase = JobPhase::Completed;
            (
                Some(actual.clone()),
                Some(completed.evidence),
                Some(completed.report),
            )
        } else if let Some(actual) = interrupted {
            status.phase = JobPhase::Interrupted;
            (Some(actual.clone()), None, None)
        } else {
            (None, None, None)
        };
        let ended = projection
            .objects
            .get("Run")
            .and_then(|runs| runs.get(&run.id))
            .is_some_and(|current| current["state"] == "ended")
            && projection
                .objects
                .get("Task")
                .and_then(|tasks| tasks.get(&run.task))
                .is_some_and(|current| current["state"] == "merged");
        return Ok(Some(finish::FinishView {
            status,
            run_state: if ended { "ended" } else { "expired" }.into(),
            actual,
            expiry_actual: Some(actual_expiry.clone()),
            evidence,
            report,
        }));
    }
    Ok(None)
}

use crate::finish::conflicting_runs;

impl ProjectSession {
    /// Conservative in-memory routing only; malformed lifecycle requires proof.
    pub fn has_finish_maintenance(&self, host: &str, now: u64) -> Result<bool, String> {
        let projection = self.projection()?;
        for raw in projection
            .objects
            .get("Run")
            .into_iter()
            .flat_map(|runs| runs.values())
        {
            let Ok(run) = serde_json::from_value::<Run>(raw.clone()) else {
                return Ok(true);
            };
            if run.host != host || !active(&run) {
                continue;
            }
            let Ok(lifecycle) = RunFinish::read(&run) else {
                return Ok(true);
            };
            if lifecycle.jobs.is_empty() {
                continue;
            }
            if now < lifecycle.entered_verifying_at || now >= lifecycle.merge_deadline {
                return Ok(true);
            }
            if !lifecycle.jobs.values().any(|job| {
                matches!(
                    job.phase,
                    JobPhase::Queued | JobPhase::Running | JobPhase::Recording
                )
            }) {
                continue;
            }
            let Some(task) = projection
                .objects
                .get("Task")
                .and_then(|tasks| tasks.get(&run.task))
                .and_then(|raw| serde_json::from_value::<Task>(raw.clone()).ok())
            else {
                return Ok(true);
            };
            let mut expected: Vec<_> = std::iter::once(&task.spec.module)
                .chain(task.spec.touches.iter())
                .map(|module| format!("module:{module}"))
                .collect();
            expected.sort();
            expected.dedup();
            let mut actual: Vec<_> = run.leases.iter().map(|fence| fence.name.clone()).collect();
            actual.sort();
            if task.spec.id != run.task || run.leases.is_empty() || actual != expected {
                return Ok(true);
            }
            for fence in &run.leases {
                let Some(lease) = self.head.leases.get(&fence.name).filter(|lease| {
                    fence.holder == run.id && lease.fence() == *fence && lease.expires_at > now
                }) else {
                    return Ok(true);
                };
                // Match the actual renewal producer below. Before this boundary
                // there is no renewal work; this hint never authorizes a write.
                let renewed_at = lease.expires_at.saturating_sub(900_000);
                if now < renewed_at || now >= renewed_at.saturating_add(60_000) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "explicit owned hints and authority diagnostics do not grant maintenance authority"
    )]
    pub fn maintain_finishes_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        owned_evidence_ids: &BTreeSet<String>,
        authority_unavailable: Option<&str>,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<FinishMaintenance, String> {
        // Actual Git facts, including a merge first observed after the deadline,
        // always precede cancellation or regular TTL release.
        Self::observe_task_merges_shared(session, catalog, host, outbox, &clock)?;
        let inputs = outbox.read_inputs()?;
        let now = clock()?;
        let history = history_shared(session, catalog, &inputs, now)?;
        let outer = history.actual_outer_events(catalog)?;
        let mut result = FinishMaintenance::default();
        for receipt in history.historical_expiry_receipts(catalog)? {
            let actual = receipt.original();
            if actual.host != host {
                continue;
            }
            if !inputs.receipt_confirmed(actual) {
                let original = recovery_outbox::original_of(actual);
                let fresh = outbox.read_inputs()?;
                Self::continue_shared(session, catalog, &fresh, clock()?, |live| {
                    recovery_outbox::append_expiry_receipt_durable(
                        &mut *live.hub,
                        outbox,
                        original,
                        now,
                        &receipt,
                    )
                })
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            }
            result.expired.push(expiry_outcome(actual.clone())?);
        }
        // Only actual accepted outer terminals are repairable here. Pending-only
        // output cannot gain an execution/result permission after the deadline.
        for actual in outer.iter().copied().filter(|actual| {
            actual.host == host
                && !actual.stale
                && matches!(
                    actual.kind.as_str(),
                    "finish.completed" | "finish.interrupted"
                )
        }) {
            if inputs.receipt_confirmed(actual) {
                continue;
            }
            let original = recovery_outbox::original_of(actual);
            let receipt = history
                .actual_finish_receipt(catalog, &original)?
                .ok_or("accepted terminal lost its actual outer receipt")?;
            let fresh = outbox.read_inputs()?;
            Self::continue_shared(session, catalog, &fresh, clock()?, |live| {
                recovery_outbox::append_finish_durable(
                    &mut *live.hub,
                    outbox,
                    original,
                    now,
                    &receipt,
                )
            })
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        }
        // Full confirmed rows with uncertain fsync are still durability work.
        for actual in outer.iter().copied().filter(|actual| {
            actual.host == host
                && actual.kind == "task.released"
                && actual.data.get("finish_expiry").is_some()
        }) {
            if result
                .expired
                .iter()
                .any(|observed| observed.actual.eid == actual.eid)
            {
                continue;
            }
            let observed = prove_expiry_outcome(&history, catalog, actual.clone())?;
            if inputs.receipt_confirmed(&observed.actual) {
                result.expired.push(observed);
                continue;
            }
            let original = recovery_outbox::original_of(actual);
            let receipt = history
                .task_receipts(catalog, actual.seq, std::slice::from_ref(&original))?
                .remove(&original.eid)
                .ok_or("accepted expiry lost its actual outer receipt")?;
            let fresh = outbox.read_inputs()?;
            Self::continue_shared(session, catalog, &fresh, clock()?, |live| {
                recovery_outbox::append_task_durable(
                    &mut *live.hub,
                    outbox,
                    original,
                    now,
                    &receipt,
                )
            })
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            result.expired.push(observed);
        }
        let inputs = outbox.read_inputs()?;
        Self::sync_shared(session, catalog, &inputs, clock()?).map_err(|e| e.to_string())?;
        let statuses = Self::finish_jobs_shared(session, catalog, &inputs, host, clock()?)?;
        let mut runs = BTreeMap::<String, Vec<FinishStatus>>::new();
        for status in statuses {
            runs.entry(status.input.run_id.clone())
                .or_default()
                .push(status);
        }
        for (id, jobs) in runs {
            let now = clock()?;
            let (task, run, identity) =
                Self::continue_shared(session, catalog, &inputs, now, |live| {
                    let (task, run) = task_run(live, &id)?;
                    Ok::<_, String>((task, run, live.snapshot_session_id()))
                })
                .map_err(|e| e.to_string())??;
            if run.host != host || !active(&run) || task.state == "merged" {
                continue;
            }
            let lifecycle = RunFinish::read(&run)?;
            let first = jobs
                .iter()
                .min_by_key(|job| (&job.input.entered_verifying_at, &job.request_id))
                .ok_or("finish maintenance lacks actual started original")?;
            if first.input.merge_deadline != lifecycle.merge_deadline
                || first.input.entered_verifying_at != lifecycle.entered_verifying_at
                || jobs.iter().any(|job| job.input.fences != run.leases)
            {
                return Err("finish maintenance original deadline/fences differ".into());
            }
            if matches!(
                finish::expiry_action(now, lifecycle.merge_deadline, false),
                finish::ExpiryAction::Release
            ) {
                result.expirations.push(FinishExpiry {
                    project: catalog.config.project_id().into(),
                    common: catalog.common_dir.clone(),
                    configuration: catalog.config.to_yaml(),
                    session: identity,
                    task,
                    run,
                    deadline: lifecycle.merge_deadline,
                    started: first.clone(),
                    evidence_ids: lifecycle.jobs.keys().cloned().collect(),
                    authority_unavailable: authority_unavailable.map(str::to_owned),
                });
                continue;
            }
            if !jobs.iter().any(|job| {
                owned_evidence_ids.contains(&job.input.evidence_id)
                    && matches!(
                        job.phase,
                        JobPhase::Queued | JobPhase::Running | JobPhase::Recording
                    )
            }) {
                continue;
            }
            let renewed = Self::continue_shared(session, catalog, &inputs, now, |live| {
                let final_now = clock()?;
                let (current_task, current_run) = task_run(live, &id)?;
                if live.snapshot_session_id() != identity
                    || json!(current_task) != json!(task)
                    || json!(current_run) != json!(run)
                    || final_now < now
                    || final_now >= lifecycle.merge_deadline
                {
                    return Err("finish renewal current Run/source changed".into());
                }
                // All original fences must still belong to this Run. A single
                // replaced/expired fence prevents renewal; never reacquire it.
                let mut due = false;
                for fence in &run.leases {
                    let lease = live
                        .head
                        .leases
                        .get(&fence.name)
                        .filter(|lease| lease.fence() == *fence && lease.expires_at > final_now)
                        .ok_or("finish renewal lost an original fence")?;
                    due |= final_now
                        >= lease
                            .expires_at
                            .saturating_sub(900_000)
                            .saturating_add(60_000);
                }
                if !due {
                    return Ok(None);
                }
                live.hub
                    .lease_renew(
                        RenewLease {
                            host: host.into(),
                            fences: run.leases.clone(),
                            ttl_ms: 900_000,
                        },
                        final_now,
                    )
                    .map(|receipt| Some(receipt.event))
                    .map_err(|e| e.to_string())
            })
            .map_err(|e| e.to_string())??;
            if let Some(actual) = renewed {
                Self::sync_shared(session, catalog, &inputs, now).map_err(|e| e.to_string())?;
                result.renewed.push(actual);
            }
        }
        Ok(result)
    }

    /// Caller has cancelled and drained only its actual owned workers outside
    /// every producer/outbox/Session/registry lock. No PID is accepted here.
    pub fn expire_finish_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        expiry: FinishExpiry,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<Option<FinishExpiryOutcome>, String> {
        if expiry.project != catalog.config.project_id()
            || expiry.common != catalog.common_dir
            || expiry.configuration != catalog.config.to_yaml()
            || expiry.run.host != host
        {
            return Err("finish expiry project/clone/host/configuration changed".into());
        }
        Self::observe_task_merges_shared(session, catalog, host, outbox, &clock)?;
        let inputs = outbox.read_inputs()?;
        let now = clock()?;
        let history = history_shared(session, catalog, &inputs, now)?;
        let eid = expiry_id(&expiry.started)?;
        if let Some(receipt) = history
            .historical_expiry_receipts(catalog)?
            .into_iter()
            .find(|receipt| receipt.original().eid == eid)
        {
            let actual = receipt.original().clone();
            let original = recovery_outbox::original_of(&actual);
            let fresh = outbox.read_inputs()?;
            Self::continue_shared(session, catalog, &fresh, now, |live| {
                recovery_outbox::append_expiry_receipt_durable(
                    &mut *live.hub,
                    outbox,
                    original,
                    now,
                    &receipt,
                )
            })
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            let fresh = outbox.read_inputs()?;
            Self::sync_shared(session, catalog, &fresh, clock()?).map_err(|e| e.to_string())?;
            return Ok(Some(expiry_outcome(actual)?));
        }
        let mut old = read_original(&inputs, &eid)?;
        for actual in history
            .actual_outer_events(catalog)?
            .into_iter()
            .filter(|actual| actual.eid == eid)
        {
            let original = recovery_outbox::original_of(actual);
            if old.as_ref().is_some_and(|old| old != &original) {
                return Err("finish expiry original body conflict".into());
            }
            old = Some(original);
        }
        let receipt = old
            .as_ref()
            .map(|original| {
                history.task_receipts(
                    catalog,
                    history
                        .actual_outer_events(catalog)?
                        .iter()
                        .map(|event| event.seq)
                        .max()
                        .unwrap_or(0),
                    std::slice::from_ref(original),
                )
            })
            .transpose()?
            .and_then(|mut receipts| receipts.remove(&eid));
        if let (Some(receipt), Some(original)) = (&receipt, &old) {
            prove_expiry_outcome(
                &history,
                catalog,
                receipt
                    .event_for(catalog.config.project_id(), original)?
                    .clone(),
            )?;
        }
        if receipt.is_none() {
            let current = Self::continue_shared(session, catalog, &inputs, now, |live| {
                let (task, run) = task_run(live, &expiry.run.id)?;
                if task.state == "merged" && run.state == "ended" {
                    return Ok(false);
                }
                if live.snapshot_session_id() != expiry.session
                    || json!(task) != json!(expiry.task)
                    || json!(run) != json!(expiry.run)
                    || now < expiry.deadline
                    || finish::run_deadline(&run)? != Some(expiry.deadline)
                    || !conflicting_runs(live.projection()?, &run.task, &run.id).is_empty()
                {
                    return Err("finish expiry original Task/Run/deadline conflict".into());
                }
                Ok::<_, String>(true)
            })
            .map_err(|e| e.to_string())??;
            if !current {
                return Ok(None);
            }
        }
        let mut task = expiry.task.clone();
        task.state = "ready".into();
        let mut run = expiry.run.clone();
        run.state = "expired".into();
        let mut wanted = NewEvent {
            eid: eid.clone(),
            host: host.into(),
            actor: "process:towermd-daemon".into(),
            kind: "task.released".into(),
            layer: "L5".into(),
            obj: run.task.clone(),
            guards: vec![],
            data: json!({"task":task,"run":run,"finish_expiry":{"schema":1,
                "entered_verifying_at":expiry.started.input.entered_verifying_at,
                "merge_deadline":expiry.deadline,"started_eid":expiry.started.started_eid,
                "authority_unavailable":expiry.authority_unavailable}}),
        };
        let original = if let Some(original) = old {
            if receipt.is_none() {
                // The first pending intent owns its diagnostic and complete
                // body. New state cannot be overwritten by that older copy.
                wanted.data["finish_expiry"]["authority_unavailable"] =
                    original.data["finish_expiry"]["authority_unavailable"].clone();
                if wanted != original {
                    return Err(uncertain(&eid, "expiry pending original body changed"));
                }
            }
            original
        } else {
            wanted
        };
        let finished = clock()?;
        if finished < now {
            return Err(uncertain(&eid, "finish expiry clock moved backwards"));
        }
        let appended = Self::continue_shared(session, catalog, &inputs, finished, |live| {
            let final_now = clock().map_err(HubError::Invalid)?;
            if final_now < finished {
                return Err(HubError::Invalid(
                    "finish expiry final clock moved backwards".into(),
                ));
            }
            if let Some(receipt) = &receipt {
                return recovery_outbox::append_task_durable(
                    &mut *live.hub,
                    outbox,
                    original,
                    final_now,
                    receipt,
                );
            }
            let (task, run) = task_run(live, &expiry.run.id).map_err(HubError::Invalid)?;
            if live.snapshot_session_id() != expiry.session
                || json!(task) != json!(expiry.task)
                || json!(run) != json!(expiry.run)
                || final_now < expiry.deadline
                || !conflicting_runs(
                    live.projection().map_err(HubError::Invalid)?,
                    &run.task,
                    &run.id,
                )
                .is_empty()
            {
                return Err(HubError::Invalid(
                    "finish expiry final Task/Run CAS changed".into(),
                ));
            }
            // Release each still-exact fence independently. A replacement epoch
            // is never released and the Hub performs the final actual CAS.
            for fence in &run.leases {
                let head = live
                    .hub
                    .subscribe(live.hub.contiguous_seq(), final_now)?
                    .head;
                if head
                    .leases
                    .get(&fence.name)
                    .is_some_and(|lease| lease.fence() == *fence)
                {
                    match live.hub.lease_release(
                        ReleaseLease {
                            host: host.into(),
                            fences: vec![fence.clone()],
                        },
                        final_now,
                    ) {
                        Ok(_) | Err(HubError::FenceExpired { .. }) => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            recovery_outbox::append_durable(&mut *live.hub, outbox, original, final_now)
        })
        .map_err(|e| uncertain(&eid, e))?;
        let synchronized = outbox.read_inputs().and_then(|fresh| {
            Self::sync_shared(session, catalog, &fresh, finished).map_err(|e| e.to_string())
        });
        let actual = match appended {
            Ok(AppendReceipt::Event(actual)) => *actual,
            Ok(AppendReceipt::Duplicate { .. }) => {
                return Err(uncertain(
                    &eid,
                    "expiry duplicate lacks complete actual receipt",
                ))
            }
            Err(error) => return Err(uncertain(&eid, error)),
        };
        synchronized.map_err(|e| uncertain(&eid, e))?;
        Ok(Some(expiry_outcome(actual)?))
    }
}
