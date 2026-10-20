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
        let mut checked_clock = || {
            let now = clock()?;
            if now < first {
                return Err(uncertain_request(
                    id,
                    "clock moved backwards before Task recheck",
                ));
            }
            Ok(now)
        };
        let original = self
            .write_own_task_recheck_checkpoint(
                &_writing,
                session,
                catalog,
                host,
                &mut checked_clock,
                outbox,
                &request,
            )
            .map_err(|e| uncertain_request(id, e))?;
        let current = Catalog::load(&catalog.root).map_err(|e| uncertain_request(id, e))?;
        let now = clock().map_err(|e| uncertain_request(id, e))?;
        ProjectSession::task_recheck_outcome_shared(session, &current, outbox, original, now)
            .map_err(|e| uncertain_request(id, e))
    }
}
