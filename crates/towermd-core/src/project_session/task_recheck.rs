//! Fixed App Task recheck operations; no Task lease mutation or agent bypass.
use super::ProjectSession;
use crate::{
    baseline_current::current_baseline,
    catalog::Catalog,
    hub_protocol::Event,
    proposals::{self, TaskRecheckOutcome, TaskRecheckView},
    recovery_outbox::{self, RecoveryInputs, RecoveryOutbox},
    request_ledger::{RequestIdentity, ReviewValidity},
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
