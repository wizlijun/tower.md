//! A finite capability for this writer's newly produced Q. Complete immutable
//! publication/history/population proof runs before Q; no restored request or
//! caller-supplied Event can construct the actual-written half of this proof.
use super::*;
use crate::{
    proposals::{self, RecheckFact, RecheckRequest, TaskRecheckView},
    recovery_contract::recorded::BaselineCreated,
    recovery_git::{self, PublicationRecheckPlan, VerifiedPublication},
    recovery_outbox::{self, RecoveryInputs},
    snapshot_service::WrittenPublicationQ,
    tasks::Task,
};
use std::collections::BTreeMap;

pub(crate) struct OwnPublicationPreparation {
    writer: uuid::Uuid,
    identity: SnapshotSessionId,
    before: RecordedPrefix,
    inputs: RecoveryInputs,
    revision: String,
    private: Option<String>,
    drafts: String,
    config: String,
    root: std::path::PathBuf,
    common: std::path::PathBuf,
    purpose: CheckpointPurpose,
    q: Option<CapturedQ>,
}

enum CheckpointPurpose {
    Publication(Box<PublicationPurpose>),
    TaskRecheck(Box<TaskRecheckPurpose>),
}

struct PublicationPurpose {
    canonical: NewEvent,
    tasks: BTreeMap<String, Task>,
    facts: Vec<RecheckFact>,
    recorded: Option<BaselineCreated>,
}

struct TaskRecheckPurpose {
    request: RecheckRequest,
    subject: TaskRecheckView,
    recorded: Option<Event>,
}

struct CapturedQ {
    before: RecordedPrefix,
    snapshot: Snapshot,
    fence: Fence,
    at: u64,
}

fn population(projection: &StateProjection) -> Result<BTreeMap<String, Task>, String> {
    projection
        .objects
        .get("Task")
        .into_iter()
        .flat_map(|objects| objects.iter())
        .map(|(id, value)| {
            let task: Task = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            if task.spec.id != *id {
                return Err("own publication population has a mismatched Task identity".into());
            }
            Ok((id.clone(), task))
        })
        .collect()
}

impl OwnPublicationPreparation {
    fn prepare_prefix(
        session: &std::sync::Mutex<ProjectSession>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        now: u64,
    ) -> Result<(SnapshotSessionId, RecordedPrefix, Option<String>, String), String> {
        inputs.probe(catalog)?;
        let private = crate::snapshot_git::private_revision(&catalog.root)?;
        let drafts =
            crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?;
        ProjectSession::sync_shared(session, catalog, inputs, now).map_err(|e| e.to_string())?;
        let (identity, before) = {
            let live = session.lock().map_err(|_| "session lock poisoned")?;
            live.ensure_active()?;
            if live.tasks.needs_reconciliation(inputs) || live.prepared_key.is_none() {
                return Err("own publication requires a fully proved reconciled prefix".into());
            }
            let before = RecordedPrefix::new(&live);
            if before.sources.as_ref()
                != Some(&(
                    catalog.revision.clone(),
                    private.clone(),
                    inputs.digest().into(),
                    drafts.clone(),
                ))
                || before.proof_key.as_ref()
                    != Some(
                        &ProjectSession::key(&before.session, catalog, inputs, &private, &drafts)
                            .map_err(|e| e.to_string())?,
                    )
            {
                return Err("own publication lacks its exact installed source proof".into());
            }
            (live.snapshot_session_id(), before)
        };
        Ok((identity, before, private, drafts))
    }

    pub(crate) fn prepare(
        writer: uuid::Uuid,
        session: &std::sync::Mutex<ProjectSession>,
        catalog: &Catalog,
        inputs: RecoveryInputs,
        proof: &VerifiedPublication,
        now: u64,
    ) -> Result<Self, String> {
        let (identity, before, private, drafts) =
            Self::prepare_prefix(session, catalog, &inputs, now)?;
        let tasks = population(&before.projection)?;
        let canonical = recovery_git::baseline_created_event(proof)?;
        let commit = canonical.data["commit"]
            .as_str()
            .ok_or("publication lacks commit")?;
        let history = recovery_git::read_recovery_history(catalog, &inputs)?;
        if history
            .publication_created_original(&canonical.eid)
            .is_some()
        {
            return Err(
                "existing publication S requires its original fixed population proof".into(),
            );
        }
        let actual = recovery_git::prove_publication(catalog, &history, commit)?;
        if recovery_git::baseline_created_event(&actual)? != canonical {
            return Err("own publication differs from complete current Git proof".into());
        }
        let (proved, facts, prepared_seq) =
            recovery_git::publication_population_facts(catalog, &actual, &tasks)?;
        if proved != canonical
            || history
                .actual_outer_events(catalog)?
                .iter()
                .any(|event| event.kind == "hub.rebuilt" && event.seq > prepared_seq)
        {
            return Err("own publication population crosses a rebuilt sequence".into());
        }
        let prepared = Self {
            writer,
            identity,
            before,
            inputs,
            revision: catalog.revision.clone(),
            private,
            drafts,
            config: catalog.config.to_yaml(),
            root: catalog.root.clone(),
            common: catalog.common_dir.clone(),
            purpose: CheckpointPurpose::Publication(Box::new(PublicationPurpose {
                canonical,
                tasks,
                facts,
                recorded: None,
            })),
            q: None,
        };
        prepared.check_source(session, catalog, now)?;
        Ok(prepared)
    }

    pub(crate) fn prepare_recheck(
        writer: uuid::Uuid,
        session: &std::sync::Mutex<ProjectSession>,
        catalog: &Catalog,
        inputs: RecoveryInputs,
        request: &RecheckRequest,
        now: u64,
    ) -> Result<Self, String> {
        let (identity, before, private, drafts) =
            Self::prepare_prefix(session, catalog, &inputs, now)?;
        // This complete baseline/publication proof belongs before Q. The
        // captured source fingerprint is probed again after this slow read.
        let baseline = crate::baseline_current::current_baseline(catalog, &inputs)?
            .ok_or("Task recheck needs current approved baseline")?;
        let subject = {
            let mut live = session.lock().map_err(|_| "session lock poisoned")?;
            let subject =
                Self::recheck_subject(&live, &baseline.baseline, &request.binding().task_id, now)?;
            if subject.binding != *request.binding() {
                return Err(
                    "Task recheck binding changed; reload subject before confirming".into(),
                );
            }
            live.baseline_requests
                .as_mut()
                .ok_or("Task recheck request history unprepared")?
                .reserve(crate::request_ledger::RequestIdentity::RecheckResolved(
                    request.clone(),
                ))?;
            subject
        };
        let prepared = Self {
            writer,
            identity,
            before,
            inputs,
            revision: catalog.revision.clone(),
            private,
            drafts,
            config: catalog.config.to_yaml(),
            root: catalog.root.clone(),
            common: catalog.common_dir.clone(),
            purpose: CheckpointPurpose::TaskRecheck(Box::new(TaskRecheckPurpose {
                request: request.clone(),
                subject,
                recorded: None,
            })),
            q: None,
        };
        prepared.check_source(session, catalog, now)?;
        Ok(prepared)
    }

    fn recheck_subject(
        live: &ProjectSession,
        baseline: &str,
        task: &str,
        now: u64,
    ) -> Result<TaskRecheckView, String> {
        live.tasks.require_publication_population()?;
        let view =
            proposals::recheck_view(live.projection()?, &live.head.leases, baseline, task, now)?;
        if live.tasks.publication_blocked(task) && view.reasons.is_empty() {
            return Err(
                "Task publication markers are incomplete; repair the original publication first"
                    .into(),
            );
        }
        Ok(view)
    }

    fn matches_prefix(&self, live: &mut ProjectSession, now: u64) -> Result<(), String> {
        live.ensure_active()?;
        let before = &self.before;
        let current = live.capture(now).map_err(|e| e.to_string())?;
        if live.snapshot_session_id() != self.identity
            || live.projection != before.projection
            || live.tasks.pending_claim != before.pending
            || live.tasks.needs_reconciliation(&self.inputs)
            || live.verified_sources != before.sources
            || current.restored
            || current.base != before.session.base
            || !current.tail.starts_with(&before.session.tail)
            || !renewal_suffix_matches(
                &before.session.head,
                &current.tail[before.session.tail.len()..],
                &current.previous_head,
                &current.head,
                Some(&before.observed_head),
            )
        {
            return Err("own publication verified prefix changed outside actual renewals".into());
        }
        Ok(())
    }

    /// This is only the finite owner's fence callback. Other snapshot writers
    /// continue to use validate_authority_shared and complete synchronization.
    pub(crate) fn check_source(
        &self,
        session: &std::sync::Mutex<ProjectSession>,
        catalog: &Catalog,
        now: u64,
    ) -> Result<(), String> {
        if catalog.root != self.root
            || catalog.common_dir != self.common
            || catalog.revision != self.revision
            || catalog.config.to_yaml() != self.config
        {
            return Err("own publication project/authority changed".into());
        }
        ProjectSession::probe_actual_binding(catalog, &self.inputs, &self.private)
            .map_err(|e| e.to_string())?;
        if crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?
            != self.drafts
        {
            return Err("own publication approval/tag refs changed".into());
        }
        let mut live = session.lock().map_err(|_| "session lock poisoned")?;
        self.matches_prefix(&mut live, now)
    }

    /// Keep only actual, completely replayed renewal Events. This is the same
    /// system suffix proof as shared preparation, without repeating Git history
    /// while the producer already owns the unchanged business prefix.
    pub(crate) fn absorb_renewals(
        &self,
        live: &mut ProjectSession,
        catalog: &Catalog,
        now: u64,
    ) -> Result<(), String> {
        self.matches_prefix(live, now)?;
        let capture = live.capture(now).map_err(|e| e.to_string())?;
        for event in &capture.fresh {
            ProjectSession::apply_prepared_event(&mut live.tasks, &mut live.projection, event)
                .map_err(|e| e.to_string())?;
            live.observe(event)?;
            live.cursor = event.seq;
            live.tail.push(event.clone());
            live.applied_count += 1;
        }
        live.head = capture.head;
        live.summary.cursor = live.cursor;
        let installed = RecordedPrefix::new(live);
        let state = SessionCapture {
            cursor: live.cursor,
            previous_head: live.head.clone(),
            head: live.head.clone(),
            base: live.base.clone(),
            tail: live.tail.clone(),
            fresh: vec![],
            restored: false,
        };
        // A complete proof may already have advanced the same pure-renew prefix;
        // only its coordinates change, never the complete business projection.
        if installed.sources != self.before.sources {
            return Err("own publication renewal source changed".into());
        }
        live.prepared_key = Some(
            ProjectSession::key(&state, catalog, &self.inputs, &self.private, &self.drafts)
                .map_err(|e| e.to_string())?,
        );
        live.prepared_head = Some(live.head.clone());
        Ok(())
    }

    /// Called under the actual writer's checkpoint lock, after complete task
    /// maintenance and main acquisition, before releasing Q for Git I/O.
    pub(crate) fn capture_q(
        &mut self,
        live: &mut ProjectSession,
        catalog: &Catalog,
        snapshot: &Snapshot,
        fence: &Fence,
        now: u64,
    ) -> Result<(), String> {
        self.matches_prefix(live, now)?;
        let before = RecordedPrefix::new(live);
        let mut expected = live.checkpoint(catalog.config.project_id(), fence, now)?;
        match &self.purpose {
            CheckpointPurpose::Publication(publication) => {
                if crate::recovery_contract::canonical_hash(&population(&snapshot.projection)?)?
                    != crate::recovery_contract::canonical_hash(&publication.tasks)?
                {
                    return Err("own publication Q population changed".into());
                }
            }
            CheckpointPurpose::TaskRecheck(recheck) => {
                let proof = snapshot.task_recheck_proof()?;
                proof.require_request(&recheck.request)?;
                if proof.subject != recheck.subject
                    || proof.generation != live.task_recheck_generation()
                    || Self::recheck_subject(
                        live,
                        &recheck.request.binding().target_baseline,
                        &recheck.request.binding().task_id,
                        now,
                    )? != recheck.subject
                {
                    return Err("Task recheck subject or lease expiry changed before Q; reload before confirming".into());
                }
                expected.recheck_lease_proof = Some(proof.clone());
            }
        }
        if self.q.is_some()
            || live.cursor != live.hub.contiguous_seq()
            || live.prepared_key.as_ref()
                != Some(
                    &ProjectSession::key(
                        &before.session,
                        catalog,
                        &self.inputs,
                        &self.private,
                        &self.drafts,
                    )
                    .map_err(|e| e.to_string())?,
                )
            || snapshot.through_seq != live.cursor
            || expected != *snapshot
        {
            return Err("own publication Q differs from exact proved checkpoint".into());
        }
        self.q = Some(CapturedQ {
            before,
            snapshot: snapshot.clone(),
            fence: fence.clone(),
            at: now,
        });
        Ok(())
    }

    /// Only SnapshotWriter can create WrittenPublicationQ, after actual Git
    /// production. Consuming this value never imports a serialized proof.
    pub(crate) fn record_written(
        &mut self,
        session: &std::sync::Mutex<ProjectSession>,
        written: WrittenPublicationQ,
        outbox: &mut RecoveryOutbox,
        clock: &mut dyn FnMut() -> Result<u64, String>,
    ) -> Result<(), String> {
        let q = self.q.take().ok_or("own publication lacks captured Q")?;
        let (writer, commit, snapshot) = written.parts();
        if writer != &self.writer || snapshot != &q.snapshot {
            return Err("own publication written seal differs from its writer/Q/time".into());
        }
        let current = Catalog::load(&self.root)?;
        let private = crate::snapshot_git::private_revision(&self.root)?;
        let parents = crate::repo::git(&self.root, &["rev-list", "--parents", "-n", "1", commit])?;
        let mut expected_parents = vec![commit, self.revision.as_str()];
        if let Some(previous) = self
            .private
            .as_deref()
            .filter(|previous| *previous != self.revision.as_str())
        {
            expected_parents.push(previous);
        }
        let own_main = current.revision == commit && private == self.private;
        let own_private = current.revision == self.revision && private.as_deref() == Some(commit);
        if (!own_main && !own_private)
            || current.common_dir != self.common
            || parents.split_whitespace().collect::<Vec<_>>() != expected_parents
            || current.config.to_yaml() != self.config
            || crate::snapshot_service::draft_ref_fingerprint(&current)
                .map_err(|e| e.to_string())?
                != self.drafts
        {
            return Err("own publication actual written source/approval refs changed".into());
        }
        let inputs = outbox.read_inputs()?;
        self.inputs.validate_task_writes(&inputs, &[])?;
        ProjectSession::probe_actual_binding(&current, &inputs, &private)
            .map_err(|e| e.to_string())?;
        if matches!(&self.purpose, CheckpointPurpose::TaskRecheck(_)) {
            #[cfg(test)]
            crate::snapshot_service::own_publication_boundary(
                &self.root,
                "AfterTaskRecheckSourceProof",
            )?;
            return self.record_recheck_written(session, &current, commit, q, outbox, clock);
        }
        let CheckpointPurpose::Publication(publication) = &self.purpose else {
            return Err("own publication checkpoint purpose changed".into());
        };
        let now = clock()?;
        if now < q.at {
            return Err("own publication clock moved backwards".into());
        }
        let identity = super::publication_events::decode_request(
            current.config.project_id(),
            &publication.canonical,
        )?;
        let actual = {
            let mut live = session.lock().map_err(|_| "session lock poisoned")?;
            live.bind_catalog(&current)?;
            let capture = live.capture(now).map_err(|e| e.to_string())?;
            if live.snapshot_session_id() != self.identity
                || !q.before.business_matches(&live)
                || capture != q.before.session
                || capture.head.contiguous_seq != q.snapshot.through_seq
                || capture
                    .head
                    .leases
                    .get("main")
                    .is_none_or(|lease| lease.fence() != q.fence || lease.expires_at <= now)
            {
                return Err("publication population Q changed before canonical creation; save a fresh prefix".into());
            }
            live.baseline_requests
                .as_mut()
                .ok_or("publication request ledger missing")?
                .reserve(identity)?;
            match recovery_outbox::append_durable(
                &mut *live.hub,
                outbox,
                publication.canonical.clone(),
                now,
            )
            .map_err(|e| e.to_string())?
            {
                AppendReceipt::Event(actual)
                    if !actual.stale
                        && q.snapshot.through_seq.checked_add(1) == Some(actual.seq)
                        && recovery_outbox::original_of(&actual) == publication.canonical =>
                {
                    *actual
                }
                _ => return Err("own publication lacks exact adjacent durable S receipt".into()),
            }
        };
        let plan = PublicationRecheckPlan {
            tasks: publication.tasks.clone(),
            facts: publication.facts.clone(),
            created: publication.canonical.clone(),
            source_commit: commit.into(),
            through_seq: q.snapshot.through_seq,
        };
        // All expensive replay/receipt recovery is deliberately after actual S.
        ProjectSession::sync_shared(session, &current, &outbox.read_inputs()?, now)
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        crate::snapshot_service::own_publication_boundary(&self.root, "AfterCreatedBeforeMarkers")?;
        ProjectSession::record_publication_rechecks_shared(
            session,
            &current,
            outbox,
            &plan,
            &actual,
            clock()?,
        )?;
        if let CheckpointPurpose::Publication(publication) = &mut self.purpose {
            publication.recorded =
                Some(serde_json::from_value(actual.data).map_err(|e| e.to_string())?);
        }
        Ok(())
    }

    fn record_recheck_written(
        &mut self,
        session: &std::sync::Mutex<ProjectSession>,
        current: &Catalog,
        commit: &str,
        q: CapturedQ,
        outbox: &mut RecoveryOutbox,
        clock: &mut dyn FnMut() -> Result<u64, String>,
    ) -> Result<(), String> {
        let CheckpointPurpose::TaskRecheck(recheck) = &self.purpose else {
            return Err("Task recheck checkpoint purpose changed".into());
        };
        let proof = q.snapshot.task_recheck_proof()?;
        proof.require_request(&recheck.request)?;
        let event = proposals::resolved_event(
            &recheck.request,
            commit,
            q.snapshot.through_seq,
            recheck.subject.clone(),
        )?;
        let result = (|| -> Result<Option<Event>, HubError> {
            let mut live = session
                .lock()
                .map_err(|_| HubError::Invalid("session lock poisoned".into()))?;
            live.bind_catalog(current).map_err(HubError::Invalid)?;
            #[cfg(test)]
            crate::snapshot_service::own_publication_boundary(
                &self.root,
                "BeforeTaskRecheckFinalAppend",
            )
            .map_err(HubError::Invalid)?;
            // Every physical source probe has finished. This same-clock read
            // and subscribe settle actual expiry under the final Session lock.
            let now = clock().map_err(HubError::Invalid)?;
            if now < q.at {
                return Err(HubError::Uncertain(
                    "clock moved backwards before Task recheck S".into(),
                ));
            }
            live.refresh_system_head(now).map_err(HubError::Invalid)?;
            let capture = live
                .capture(now)
                .map_err(|e| HubError::Invalid(e.to_string()))?;
            if live.task_recheck_generation() != proof.generation {
                return Err(HubError::Uncertain(
                    "Task recheck source Q generation changed before S; retry original request"
                        .into(),
                ));
            }
            if live.cursor != q.snapshot.through_seq
                || capture.head.contiguous_seq != q.snapshot.through_seq
                || live.hub.contiguous_seq() != q.snapshot.through_seq
            {
                return Err(HubError::Uncertain(
                    "Task recheck source Q advanced before S; retry original request".into(),
                ));
            }
            if live.snapshot_session_id() != self.identity
                || !q.before.business_matches(&live)
                || capture != q.before.session
                || capture
                    .head
                    .leases
                    .get("main")
                    .is_none_or(|lease| lease.fence() != q.fence || lease.expires_at <= now)
                || Self::recheck_subject(
                    &live,
                    &recheck.request.binding().target_baseline,
                    &recheck.request.binding().task_id,
                    now,
                )
                .map_err(HubError::Invalid)?
                    != recheck.subject
            {
                return Err(HubError::Invalid("Task recheck binding changed; reload the exact Task, baseline, publications and live fences".into()));
            }
            live.baseline_requests
                .as_mut()
                .ok_or_else(|| HubError::Invalid("Task recheck request history unprepared".into()))?
                .reserve(crate::request_ledger::RequestIdentity::RecheckResolved(
                    recheck.request.clone(),
                ))
                .map_err(HubError::Invalid)?;
            match recovery_outbox::append_durable(&mut *live.hub, outbox, event.clone(), now)? {
                AppendReceipt::Event(actual)
                    if !actual.stale
                        && q.snapshot.through_seq.checked_add(1) == Some(actual.seq)
                        && recovery_outbox::original_of(&actual) == event =>
                {
                    Ok(Some(*actual))
                }
                _ => Err(HubError::Uncertain(
                    "Task recheck lacks exact adjacent durable S receipt".into(),
                )),
            }
        })();
        // Accepted/uncertain originals must still enter the continuous view.
        // The complete replay is now after the adjacent S append, never between Q and S.
        let finished_at = clock()?;
        let actual = ProjectSession::finish_baseline_request_shared(
            session,
            current,
            outbox,
            &recheck.request.metadata().request_id,
            result,
            finished_at,
        )?
        .ok_or("Task recheck has no actual original receipt")?;
        if let CheckpointPurpose::TaskRecheck(recheck) = &mut self.purpose {
            recheck.recorded = Some(actual);
        }
        Ok(())
    }

    pub(crate) fn take_recorded(&mut self) -> Option<BaselineCreated> {
        match &mut self.purpose {
            CheckpointPurpose::Publication(publication) => publication.recorded.take(),
            CheckpointPurpose::TaskRecheck(_) => None,
        }
    }

    pub(crate) fn take_recheck_recorded(&mut self) -> Option<Event> {
        match &mut self.purpose {
            CheckpointPurpose::TaskRecheck(recheck) => recheck.recorded.take(),
            CheckpointPurpose::Publication(_) => None,
        }
    }
}
