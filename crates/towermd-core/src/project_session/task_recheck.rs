//! Fixed App Task recheck operations; no Task lease mutation or agent bypass.
use super::ProjectSession;
use crate::{
    baseline_current::current_baseline,
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError},
    proposals::{self, RecheckRequest, TaskRecheckOutcome, TaskRecheckView},
    recovery_outbox::{self, RecoveryInputs, RecoveryOutbox},
    request_ledger::{uncertain_request, RequestIdentity, ReviewValidity},
};
use std::sync::Mutex;
fn view(
    live: &ProjectSession,
    baseline: &str,
    task: &str,
    now: u64,
) -> Result<TaskRecheckView, String> {
    live.tasks.require_publication_population()?;
    let view = proposals::recheck_view(live.projection()?, &live.head.leases, baseline, task, now)?;
    if live.tasks.publication_blocked(task) && view.reasons.is_empty() {
        return Err(
            "Task publication markers are incomplete; repair the original publication first".into(),
        );
    }
    Ok(view)
}
impl ProjectSession {
    pub(crate) fn retry_task_recheck_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        identity: &RequestIdentity,
        id: &str,
        now: u64,
    ) -> Result<Option<Event>, String> {
        super::human_review::retry_shared(session, catalog, outbox, identity, id, now)
    }
    /// Pure App read: synchronizes existing proven sources, never takes leases,
    /// repairs pending writes, writes snapshots, or creates a missing Hub.
    pub fn load_task_recheck_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        task: &str,
        now: u64,
    ) -> Result<TaskRecheckView, String> {
        crate::baseline_draft::safe_id(task)?;
        let baseline = current_baseline(catalog, inputs)?
            .ok_or("Task recheck needs current approved baseline")?;
        Self::continue_shared(session, catalog, inputs, now, |live| {
            view(live, &baseline.baseline, task, now)
        })
        .map_err(|e| e.to_string())?
    }
    /// Called only by the same existing SnapshotWriter after it durably saved Q.
    /// Git/source proof runs outside this function; final CAS and append share
    /// one Session lock. Snapshot callbacks cannot mint a caller-supplied Q.
    #[allow(
        clippy::too_many_arguments,
        reason = "same source Q, exact subject and durable request remain explicit"
    )]
    pub(crate) fn record_task_recheck_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        request: &RecheckRequest,
        source: &str,
        q: u64,
        subject: TaskRecheckView,
        generation: u64,
        q_at: u64,
        clock: &mut dyn FnMut() -> Result<u64, String>,
    ) -> Result<Event, String> {
        let id = &request.meta.request_id;
        let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
        let baseline = current_baseline(catalog, &inputs)
            .map_err(|e| uncertain_request(id, e))?
            .ok_or("Task recheck current baseline disappeared")?;
        let event = proposals::resolved_event(request, source, q, subject.clone())?;
        // Reconstruct the historical Q prefix at its original capture time.
        // This timestamp grants no S authority: the final locked boundary below
        // samples the same physical clock after every slow preparation.
        let prepared_at = q_at;
        let result=Self::continue_shared(session,catalog,&inputs,prepared_at,|live| {
            let ledger=live.baseline_requests.as_mut().ok_or_else(||HubError::Invalid("Task recheck request history unprepared".into()))?;
            let identity=RequestIdentity::RecheckResolved(request.clone());ledger.reserve(identity.clone()).map_err(HubError::Invalid)?;
            if let Some(original)=ledger.retry_original(catalog,&identity,&mut *live.hub,outbox,prepared_at)? {return Ok(Some(original));}
            #[cfg(test)]
            crate::snapshot_service::own_publication_boundary(&catalog.root, "BeforeTaskRecheckFinalAppend").map_err(HubError::Invalid)?;
            // Every slow source/baseline/sync proof has finished. Sample the
            // same physical clock under the final Session lock; subscribe
            // settles actual expiry before the Q/generation/subject CAS.
            let now = clock().map_err(HubError::Invalid)?;
            if now < q_at {return Err(HubError::Uncertain("clock moved backwards before Task recheck S".into()));}
            live.refresh_system_head(now).map_err(HubError::Invalid)?;
            if live.task_recheck_generation()!=generation {
                return Err(HubError::Uncertain("Task recheck source Q generation changed before S; retry original request".into()));
            }
            if live.cursor!=q || live.hub.contiguous_seq()!=q {return Err(HubError::Uncertain("Task recheck source Q advanced before S; retry original request".into()));}
            let current=view(live,&baseline.baseline,&request.binding().task_id,now).map_err(HubError::Invalid)?;
            if current.binding!=*request.binding()||current!=subject {return Err(HubError::Invalid("Task recheck binding changed; reload the exact Task, baseline, publications and live fences".into()));}
            match recovery_outbox::append_durable(&mut *live.hub,outbox,event,now)? {
                AppendReceipt::Event(actual)=>Ok(Some(*actual)),
                AppendReceipt::Duplicate{..}=>Err(HubError::Uncertain("Task recheck duplicate lacks its actual original Event".into())),
            }
        }).map_err(|e|uncertain_request(id,e))?;
        let finished_at = clock().map_err(|e| uncertain_request(id, e))?;
        Self::finish_baseline_request_shared(session, catalog, outbox, id, result, finished_at)?
            .ok_or_else(|| uncertain_request(id, "Task recheck has no actual original receipt"))
    }
    pub(crate) fn task_recheck_outcome_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &RecoveryOutbox,
        event: Event,
        now: u64,
    ) -> Result<TaskRecheckOutcome, String> {
        let record = proposals::decode_resolved(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&event),
        )?;
        let inputs = outbox.read_inputs()?;
        let binding = &record.subject.binding;
        // Source failures propagate. A proved changed subject only downgrades
        // business validity; it does not rewrite the immutable original receipt.
        Self::sync_shared(session, catalog, &inputs, now).map_err(|e| e.to_string())?;
        let baseline = current_baseline(catalog, &inputs)?
            .ok_or("Task recheck baseline disappeared after acceptance")?;
        let current = Self::continue_shared(session, catalog, &inputs, now, |live| {
            view(live, &baseline.baseline, &binding.task_id, now)
        })
        .map_err(|e| e.to_string())?;
        let validity = match current {
            Ok(current)
                if !event.stale
                    && current.binding.target_baseline == binding.target_baseline
                    && current.binding.task_content_hash == binding.task_content_hash
                    && current.binding.running == binding.running
                    && current
                        .binding
                        .publication_ids
                        .iter()
                        .all(|id| binding.publication_ids.contains(id)) =>
            {
                ReviewValidity::Current
            }
            Ok(_) => ReviewValidity::HistoricalOnly {
                reason: "Task recheck original subject, baseline, publication set or Run changed"
                    .into(),
            },
            Err(reason) => ReviewValidity::HistoricalOnly { reason },
        };
        Ok(TaskRecheckOutcome {
            receipt: record.p3_receipt,
            current_validity: validity,
        })
    }
}
