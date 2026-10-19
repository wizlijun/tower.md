//! Sole-writer materialization followed by durable proposal acceptance.
use super::{read_draft, SnapshotWriter};
use crate::{
    catalog::Catalog,
    hub_protocol::NewEvent,
    project_session::ProjectSession,
    proposals::*,
    recovery_contract::recorded::{request_event_eid, RequestBody, RequestReceipt},
    recovery_outbox::RecoveryOutbox,
    request_ledger::{uncertain_request, DraftSaveAction, PreparedDraftSaveRequest},
};
use std::sync::Mutex;
impl SnapshotWriter {
    #[allow(clippy::too_many_arguments)]
    pub fn save_proposal(
        &self,
        session: &Mutex<ProjectSession>,
        catalog: &Catalog,
        input: &ProposalInput,
        run: Option<&str>,
        request_id: &str,
        outbox: &mut RecoveryOutbox,
        host: &str,
        mut clock: impl FnMut() -> Result<u64, String>,
    ) -> Result<ProposalOutcome, String> {
        let request = PreparedDraftSaveRequest::prepare(
            RequestBody {
                project_id: catalog.config.project_id().into(),
                actor: "process:towermd-cli".into(),
                host: host.into(),
                run: run.map(str::to_owned),
                action: DraftSaveAction::Propose {
                    input: input.clone(),
                },
            },
            request_id,
        )?;
        let started = clock().map_err(|e| uncertain_request(request_id, e))?;
        if let Some(original) =
            ProjectSession::retry_proposal_shared(session, catalog, outbox, &request, started)?
        {
            return ProjectSession::proposal_outcome_shared(
                session,
                catalog,
                &outbox.read_inputs()?,
                original,
                started,
            );
        }
        let _writing = self
            .writing
            .try_lock()
            .map_err(|_| uncertain_request(request_id, "repository writer busy or poisoned"))?;
        let inputs = outbox
            .read_inputs()
            .map_err(|e| uncertain_request(request_id, e))?;
        let saved =
            read_draft(catalog, request_id).map_err(|e| uncertain_request(request_id, e))?;
        let context = if saved.is_some() {
            None
        } else {
            Some(ProjectSession::prepare_proposal_shared(
                session, catalog, &inputs, host, run, started, true,
            )?)
        };
        let (saved, draft) = if let Some(saved) = saved {
            if saved.request() != &request {
                return Err("request_id materialized with a different original body".into());
            }
            let carrier = validate_proposal_subject(saved.frozen())?
                .ok_or("materialized proposal lacks its carrier")?;
            if carrier.binding.is_none()
                || carrier.input != *input
                || run.is_some_and(|selector| selector != carrier.run)
            {
                return Err(uncertain_request(
                    request_id,
                    "saved proposal belongs to its old Run/baseline; cannot restage",
                ));
            }
            // No ref mutation or new draft. Remote delivery remains unknown on recovery.
            let delivery = crate::request_ledger::DraftDelivery::LocalSavedRemotePending {
                reason: "original remote delivery is not confirmed during recovery".into(),
            };
            let result = crate::request_ledger::DraftSavedResult {
                draft_id: request_id.into(),
                revision: saved.revision().into(),
                reference: crate::repository_writer::RepositoryRef::draft(request_id)?
                    .name()
                    .into(),
                content_hash: saved.frozen().content_hash.clone(),
                delivery,
            };
            (saved, result)
        } else {
            let context = context.as_ref().ok_or("new proposal capture is absent")?;
            let analysis = analyze_proposal(catalog, &inputs, input, &context.tasks, request_id)?;
            if analysis.baseline != context.binding.baseline
                || analysis.main_revision != context.baseline.main_revision
            {
                return Err(uncertain_request(
                    request_id,
                    "proposal analysis source changed",
                ));
            }
            let frozen = crate::proposals::attach_bound_proposal(
                catalog,
                &analysis,
                input,
                &context.binding,
            )?;
            let materialized = self
                .save_draft_locked(&_writing, session, catalog, &frozen, &request, &mut clock)
                .map_err(|e| uncertain_request(request_id, e))?;
            let saved = read_draft(catalog, request_id)
                .map_err(|e| uncertain_request(request_id, e))?
                .ok_or_else(|| {
                    uncertain_request(request_id, "proposal draft missing after save")
                })?;
            (saved, materialized.result().clone())
        };
        #[cfg(test)]
        if tests::stop_after_materialization() {
            return Err(uncertain_request(
                request_id,
                "injected stop after actual private ref and before proposal pending",
            ));
        }
        let carrier =
            validate_proposal_subject(saved.frozen())?.ok_or("saved proposal lacks carrier")?;
        let binding = carrier.binding.clone().ok_or_else(|| {
            uncertain_request(request_id, "legacy proposal carrier has no original fences")
        })?;
        let result = ProposalResult {
            draft,
            binding: binding.clone(),
            carrier,
        };
        let record = ProposalCreated {
            result: result.clone(),
            request_body: request.body().clone(),
            p3_request: request.metadata().clone(),
            p3_receipt: RequestReceipt {
                request_id: request_id.into(),
                body_hash: request.metadata().body_hash.clone(),
                result_ref: result,
            },
        };
        let event = NewEvent {
            eid: request_event_eid(request_id, "proposal.created")?,
            host: host.into(),
            actor: "process:towermd-cli".into(),
            kind: "proposal.created".into(),
            layer: "L4".into(),
            obj: request_id.into(),
            guards: binding.fences,
            data: serde_json::to_value(record).map_err(|e| e.to_string())?,
        };
        let finished = clock().map_err(|e| uncertain_request(request_id, e))?;
        if finished < started {
            return Err(uncertain_request(
                request_id,
                "clock moved backwards during proposal",
            ));
        }
        let original = ProjectSession::record_proposal_shared(
            session,
            catalog,
            outbox,
            &request,
            event,
            context.as_ref(),
            finished,
        )?;
        ProjectSession::proposal_outcome_shared(
            session,
            catalog,
            &outbox.read_inputs()?,
            original,
            finished,
        )
    }
}

#[cfg(test)]
#[path = "proposal_save_tests.rs"]
mod tests;
