//! A finite current G4 subject proved from actual accepted verifier records.
//! No caller-supplied FrozenFinish/JSON can construct this capability.
use super::*;

pub(in crate::project_session) struct FrozenG4Subject {
    status: FinishStatus,
    completed: Event,
    session: SnapshotSessionId,
}
impl FrozenG4Subject {
    pub(in crate::project_session) fn tree(&self) -> &str {
        &self.status.input.tree
    }
    pub(in crate::project_session) fn run(&self) -> &str {
        &self.status.input.run_id
    }
    pub(in crate::project_session) fn baseline(&self) -> &str {
        &self.status.input.baseline
    }
    pub(in crate::project_session) fn check_current(
        &self,
        live: &ProjectSession,
        catalog: &Catalog,
        host: &str,
        now: u64,
    ) -> Result<(), String> {
        if live.snapshot_session_id() != self.session {
            return Err("frozen G4 Session changed".into());
        }
        super::integration::current_run(live, catalog, host, &self.status, now)?;
        if self.completed.eid != self.status.completed_eid {
            return Err("frozen G4 completion identity changed".into());
        }
        Ok(())
    }
    pub(in crate::project_session) fn check_actual_candidate(
        &self,
        catalog: &Catalog,
        head: &str,
    ) -> Result<(), task_gates::GateError> {
        task_gates::check_g5(catalog, &self.status.input, head, &catalog.revision)
    }
    pub(in crate::project_session) fn check_candidate(
        &self,
        catalog: &Catalog,
        report: &CheckReport,
    ) -> Result<(), task_gates::GateError> {
        task_gates::check_g5(catalog, &self.status.input, &report.commit, &report.base)
    }
}
pub(in crate::project_session) enum FrozenG4Error {
    Source(String),
    Refused(String),
}

pub(in crate::project_session) fn prove_frozen_g4_subject(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    host: &str,
    run_id: &str,
    tree: &str,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<Option<FrozenG4Subject>, FrozenG4Error> {
    let now = clock().map_err(FrozenG4Error::Source)?;
    let (run, identity) = ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
        let (_, run) = actual_context(live, catalog, host, Some(run_id), now)?;
        Ok::<_, String>((run, live.snapshot_session_id()))
    })
    .map_err(|e| FrozenG4Error::Source(e.to_string()))?
    .map_err(FrozenG4Error::Refused)?;
    let jobs = RunFinish::read(&run)
        .map_err(FrozenG4Error::Source)?
        .jobs
        .into_values()
        .filter(|job| {
            matches!(job.phase, JobPhase::Completed | JobPhase::Recording) && job.input.tree == tree
        })
        .collect::<Vec<_>>();
    if jobs.is_empty() {
        return Ok(None);
    }
    let history = history_shared(session, catalog, inputs, now).map_err(FrozenG4Error::Source)?;
    let outer = history
        .actual_outer_events(catalog)
        .map_err(FrozenG4Error::Source)?;
    let mut proved = Vec::new();
    for status in jobs {
        let Some(started) = outer
            .iter()
            .find(|e| e.eid == status.started_eid && !e.stale)
        else {
            return Err(FrozenG4Error::Source(
                "frozen G4 lacks actual outer started".into(),
            ));
        };
        let Some(completed) = outer
            .iter()
            .find(|e| e.eid == status.completed_eid && !e.stale)
        else {
            return Err(FrozenG4Error::Source(
                "frozen G4 lacks actual outer completed".into(),
            ));
        };
        let original = finish::original_status(&recovery_outbox::original_of(started))
            .map_err(FrozenG4Error::Source)?;
        let end: Completed = serde_json::from_value(completed.data.clone())
            .map_err(|e| FrozenG4Error::Source(e.to_string()))?;
        contract::validate_finish_report(&status.input, &completed.eid, &end.evidence, &end.report)
            .map_err(FrozenG4Error::Source)?;
        if end.evidence.result != finish::EvidenceResult::Passed {
            continue;
        }
        if original.request_id != status.request_id
            || original.input != status.input
            || original.review_context != status.review_context
            || original.checked_gates != status.checked_gates
            || original.completed_eid != status.completed_eid
            || original.started_eid != status.started_eid
            || end.review_context != status.review_context
            || !end.hold.is_null()
            || started.seq >= completed.seq
        {
            return Err(FrozenG4Error::Source(
                "frozen G4 original phase/context differs".into(),
            ));
        }
        for (event, kind) in [
            (*started, "finish.started"),
            (*completed, "finish.completed"),
        ] {
            recovery_outbox::validate_actual(&recovery_outbox::original_of(event), event)
                .map_err(FrozenG4Error::Source)?;
            if event.kind != kind
                || event.actor != "process:towermd-verifier"
                || event.host != host
                || event.obj != status.input.task_id
                || event.layer != "L4"
                || event.guards != status.input.fences
            {
                return Err(FrozenG4Error::Source(
                    "frozen G4 original producer/guards differ".into(),
                ));
            }
        }
        proved.push((completed.seq, status, (**completed).clone()));
    }
    proved.sort_by_key(|(seq, _, _)| *seq);
    let Some((_, status, completed)) = proved.pop() else {
        return Ok(None);
    };
    let subject = FrozenG4Subject {
        status,
        completed,
        session: identity,
    };
    let end = clock().map_err(FrozenG4Error::Source)?;
    if end < now {
        return Err(FrozenG4Error::Source(
            "frozen G4 clock moved backwards".into(),
        ));
    }
    ProjectSession::continue_shared(session, catalog, inputs, end, |live| {
        subject.check_current(live, catalog, host, end)
    })
    .map_err(|e| FrozenG4Error::Source(e.to_string()))?
    .map_err(FrozenG4Error::Refused)?;
    Ok(Some(subject))
}
