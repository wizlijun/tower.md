//! App Task recheck uses the existing sole writer. Persist Q before appending
//! the actual human S=Q+1, so a lost Hub can retain ready review without trusting
//! arbitrary confirmed-only Task/Run JSON. GET never enters this module.
use super::{SnapshotWriter, TryLockError};
use crate::{
    catalog::Catalog,
    project_session::ProjectSession,
    proposals::{RecheckRequest, TaskRecheckAction, TaskRecheckOutcome},
    recovery_outbox::RecoveryOutbox,
    request_ledger::{uncertain_request, RequestIdentity},
};
use std::sync::Mutex;
impl SnapshotWriter {
    #[allow(
        clippy::too_many_arguments,
        reason = "same writer, project, App identity and durable original request are explicit"
    )]
    pub fn resolve_task_recheck(
        &self,
        session: &Mutex<ProjectSession>,
        catalog: &Catalog,
        host: &str,
        actor: &str,
        action: &TaskRecheckAction,
        outbox: &mut RecoveryOutbox,
        mut clock: impl FnMut() -> Result<u64, String>,
    ) -> Result<TaskRecheckOutcome, String> {
        let request = RecheckRequest::prepare(catalog.config.project_id(), host, actor, action)?;
        let id = &request.meta.request_id;
        let first = clock().map_err(|e| uncertain_request(id, e))?;
        // Original UUID/body collision and durable receipt repair precede every
        // current eligibility check and every new checkpoint.
        if let Some(original) = ProjectSession::retry_task_recheck_shared(
            session,
            catalog,
            outbox,
            &RequestIdentity::RecheckResolved(request.clone()),
            id,
            first,
        )? {
            return ProjectSession::task_recheck_outcome_shared(
                session, catalog, outbox, original, first,
            );
        }
        let _writing = match self.writing.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => {
                return Err(uncertain_request(
                    id,
                    "repository writer busy; retry the original Task recheck UUID",
                ))
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(uncertain_request(id, "repository writer lock poisoned"))
            }
        };
        let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
        let viewed = ProjectSession::load_task_recheck_shared(
            session,
            catalog,
            &inputs,
            &action.binding.task_id,
            first,
        )?;
        if viewed.binding != action.binding {
            return Err("Task recheck binding changed; reload subject before confirming".into());
        }
        let mut accepted = None;
        let mut complete = |source: &str,
                            snapshot: &crate::snapshot::Snapshot,
                            at: u64,
                            box_: &mut RecoveryOutbox,
                            clock: &mut dyn FnMut() -> Result<u64, String>|
         -> Result<(), String> {
            if at < first {
                return Err(uncertain_request(
                    id,
                    "clock moved backwards before Task recheck",
                ));
            }
            let current = Catalog::load(&catalog.root).map_err(|e| uncertain_request(id, e))?;
            if current.common_dir != catalog.common_dir
                || current.config.to_yaml() != catalog.config.to_yaml()
            {
                return Err(uncertain_request(id, "Task recheck project/config changed"));
            }
            let subject = crate::recovery_git::recheck_source_view(
                &current,
                source,
                snapshot.through_seq,
                &action.binding.target_baseline,
                &action.binding.task_id,
                at,
                &request,
            )
            .map_err(|e| uncertain_request(id, e))?;
            #[cfg(test)]
            crate::snapshot_service::own_publication_boundary(
                &catalog.root,
                "AfterTaskRecheckSourceProof",
            )?;
            if subject.binding != action.binding {
                return Err("Task recheck subject or lease expiry changed during checkpoint; reload before confirming".into());
            }
            let proof = snapshot.task_recheck_proof()?;
            accepted = Some(ProjectSession::record_task_recheck_shared(
                session,
                &current,
                box_,
                &request,
                source,
                snapshot.through_seq,
                subject,
                proof.generation,
                at,
                clock,
            )?);
            Ok(())
        };
        let report = self
            .write_publication_checkpoint(
                &_writing,
                session,
                catalog,
                host,
                &mut clock,
                outbox,
                Some(&request),
                Some(&mut complete),
            )
            .map_err(|e| uncertain_request(id, e))?;
        let original = accepted.ok_or_else(|| {
            uncertain_request(
                id,
                format!("Task recheck checkpoint incomplete: {}", report.status),
            )
        })?;
        let current = Catalog::load(&catalog.root).map_err(|e| uncertain_request(id, e))?;
        let now = clock().map_err(|e| uncertain_request(id, e))?;
        ProjectSession::task_recheck_outcome_shared(session, &current, outbox, original, now)
            .map_err(|e| uncertain_request(id, e))
    }
}
