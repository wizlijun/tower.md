//! Source-bound Proposal production. No slow-layer approval is granted here.
use super::{ProjectSession, SnapshotSessionId};
use crate::{
    baseline_current::{current_baseline, CurrentBaseline},
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError, NewEvent},
    proposals::{ProposalBinding, ProposalCreated, ProposalOutcome},
    recovery_outbox::{self, RecoveryInputs, RecoveryOutbox},
    request_ledger::{
        uncertain_request, PreparedDraftSaveRequest, RequestIdentity, ReviewValidity,
    },
    tasks::Task,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

pub(crate) struct ProposalContext {
    pub(crate) binding: ProposalBinding,
    pub(crate) baseline: CurrentBaseline,
    pub(crate) tasks: BTreeMap<String, Task>,
    pub(crate) session: SnapshotSessionId,
    pub(crate) private: Option<String>,
    task_run_hash: String,
}

fn captured(
    live: &ProjectSession,
    catalog: &Catalog,
    host: &str,
    selector: Option<&str>,
    now: u64,
    baseline: &CurrentBaseline,
) -> Result<ProposalContext, String> {
    let (task, run) = live.tasks.check_context(catalog, host, selector)?;
    crate::tasks::validate_task(&task.spec, catalog)?;
    let expected: BTreeSet<_> = std::iter::once(&task.spec.module)
        .chain(task.spec.touches.iter())
        .map(|id| format!("module:{id}"))
        .collect();
    let names: BTreeSet<_> = run.leases.iter().map(|f| f.name.clone()).collect();
    if run.host != host
        || run.worktree != catalog.root.to_string_lossy()
        || run.branch != catalog.branch
        || run.branch != format!("t/{}", task.spec.id)
        || run.state != "running"
        || task.state != "running"
        || run.task != task.spec.id
        || task
            .extra
            .get("needs_recheck")
            .is_some_and(|value| value != &serde_json::Value::Bool(false))
        || names != expected
        || names.len() != run.leases.len()
        || run.leases.iter().any(|f| {
            f.holder != run.id
                || f.epoch == 0
                || live.head.leases.get(&f.name).is_none_or(|lease| {
                    lease.holder != f.holder || lease.epoch != f.epoch || lease.expires_at <= now
                })
        })
    {
        return Err(
            "proposal requires the exact active Task worktree and complete live Run fences".into(),
        );
    }
    let tasks = live
        .projection()?
        .objects
        .get("Task")
        .into_iter()
        .flat_map(|objects| objects.iter())
        .map(|(id, value)| {
            serde_json::from_value(value.clone())
                .map(|task| (id.clone(), task))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    Ok(ProposalContext {
        task_run_hash: crate::recovery_contract::canonical_hash(&(&task, &run))?,
        binding: ProposalBinding {
            task: task.spec.id,
            run: run.id,
            baseline: baseline.baseline.clone(),
            fences: run.leases,
        },
        baseline: baseline.clone(),
        tasks,
        session: live.snapshot_session_id(),
        private: live
            .verified_sources
            .as_ref()
            .ok_or("proposal sources are not prepared")?
            .1
            .clone(),
    })
}
fn actual_worktree(catalog: &Catalog) -> Result<(), String> {
    let actual = Catalog::load(&catalog.root)?;
    if actual.common_dir != catalog.common_dir
        || actual.root != catalog.root
        || actual.branch != catalog.branch
        || actual.revision != catalog.revision
        || actual.config.to_yaml() != catalog.config.to_yaml()
        || catalog.branch == "HEAD"
    {
        return Err("proposal worktree/project/config/source changed".into());
    }
    Ok(())
}

impl ProjectSession {
    pub fn read_proposal_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        id: &str,
        revision: &str,
        now: u64,
    ) -> Result<Option<Event>, String> {
        crate::repository_writer::RepositoryRef::draft(id)?;
        crate::recovery_contract::object_id(revision)?;
        let refs =
            crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?;
        let event = Self::continue_shared(session, catalog, inputs, now, |live| {
            if live
                .verified_sources
                .as_ref()
                .is_none_or(|sources| sources.3 != refs)
            {
                return Err("proposal draft refs changed during read".into());
            }
            live.baseline_requests
                .as_ref()
                .ok_or("request history is not prepared")?
                .accepted_proposal(id, revision)
        })
        .map_err(|e| e.to_string())??;
        let Some(event) = event else {
            return Ok(None);
        };
        let record = crate::proposals::decode_created(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&event),
        )?;
        let saved = crate::snapshot_service::read_draft_at(catalog, id, revision)
            .map_err(|error| error.to_string())?;
        crate::proposals::require_saved(&record, &saved)?;
        // Close the immutable reads with the normal source/Session proof and
        // make sure a concurrent rebuild did not invalidate this actual role.
        Self::continue_shared(session, catalog, inputs, now, |live| {
            if live
                .verified_sources
                .as_ref()
                .is_none_or(|sources| sources.3 != refs)
            {
                return Err("proposal draft refs changed during immutable read".into());
            }
            let current = live
                .baseline_requests
                .as_ref()
                .ok_or("request history is not prepared")?
                .accepted_proposal(id, revision)?;
            if current.as_ref() != Some(&event) {
                return Err("proposal actual acceptance changed during immutable read".into());
            }
            Ok(Some(event))
        })
        .map_err(|e| e.to_string())?
    }

    pub(crate) fn retry_proposal_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        request: &PreparedDraftSaveRequest,
        now: u64,
    ) -> Result<Option<Event>, String> {
        crate::proposals::proposal_input(request)?;
        let id = &request.metadata().request_id;
        let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
        let result = Self::continue_shared(session, catalog, &inputs, now, |live| {
            let ledger = live
                .baseline_requests
                .as_mut()
                .ok_or_else(|| HubError::Invalid("request history is not prepared".into()))?;
            let identity = RequestIdentity::Save(request.clone());
            ledger
                .reserve(identity.clone())
                .map_err(HubError::Invalid)?;
            ledger.retry_original(catalog, &identity, &mut *live.hub, outbox, now)
        })
        .map_err(|e| uncertain_request(id, e))?;
        Self::finish_baseline_request_shared(session, catalog, outbox, id, result, now)
    }

    pub(crate) fn prepare_proposal_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        host: &str,
        selector: Option<&str>,
        now: u64,
        renew: bool,
    ) -> Result<ProposalContext, String> {
        actual_worktree(catalog)?;
        let baseline = current_baseline(catalog, inputs)?
            .ok_or("proposal requires an approved current baseline")?;
        Self::continue_shared(session, catalog, inputs, now, |live| {
            if renew {
                live.renew_binding(catalog, host, selector, now)?;
            }
            captured(live, catalog, host, selector, now, &baseline)
        })
        .map_err(|e| e.to_string())?
    }

    pub(crate) fn record_proposal_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        request: &PreparedDraftSaveRequest,
        event: NewEvent,
        expected: Option<&ProposalContext>,
        now: u64,
    ) -> Result<Event, String> {
        let id = &request.metadata().request_id;
        let record = crate::proposals::decode_created(catalog.config.project_id(), &event)?;
        let refs = crate::snapshot_service::draft_ref_fingerprint(catalog)
            .map_err(|e| uncertain_request(id, e))?;
        let saved = crate::snapshot_service::read_draft(catalog, request.draft_id())
            .map_err(|e| uncertain_request(id, e))?
            .ok_or_else(|| uncertain_request(id, "proposal draft disappeared"))?;
        crate::proposals::require_saved(&record, &saved).map_err(|e| uncertain_request(id, e))?;
        actual_worktree(catalog).map_err(|e| uncertain_request(id, e))?;
        let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
        let baseline = current_baseline(catalog, &inputs)
            .map_err(|e| uncertain_request(id, e))?
            .ok_or("proposal approved baseline disappeared")?;
        if baseline.baseline != record.result.binding.baseline
            || baseline.main_revision != record.result.carrier.main_revision
            || expected.is_some_and(|expected| {
                baseline != expected.baseline || record.result.binding != expected.binding
            })
        {
            return Err(uncertain_request(id, "proposal baseline/binding changed"));
        }
        // An unaccepted saved ref may be recovered only with its original frozen
        // fences. Expired originals are recorded stale; never recaptured at a new epoch.
        let recovered_context = if expected.is_none() {
            Self::continue_shared(session, catalog, &inputs, now, |live| {
                let all_live = event.guards.iter().all(|f| {
                    live.head.leases.get(&f.name).is_some_and(|lease| {
                        lease.holder == f.holder && lease.epoch == f.epoch && lease.expires_at > now
                    })
                });
                if all_live {
                    captured(
                        live,
                        catalog,
                        &request.body().host,
                        Some(&record.result.binding.run),
                        now,
                        &baseline,
                    )
                    .map(Some)
                } else {
                    // This is a proved stale original, not an ignored preparation
                    // or I/O error. It cannot authorize a new live proposal.
                    Ok(None)
                }
            })
            .map_err(|e| uncertain_request(id, e))?
            .map_err(|e| uncertain_request(id, e))?
        } else {
            None
        };
        if let Some(current) = &recovered_context {
            if current.binding != record.result.binding {
                return Err(uncertain_request(id, "saved proposal Run fences changed"));
            }
            let analysis = crate::proposals::analyze_proposal(
                catalog,
                &inputs,
                crate::proposals::proposal_input(request)?,
                &current.tasks,
                request.draft_id(),
            )
            .map_err(|e| uncertain_request(id, e))?;
            let rebuilt = crate::proposals::attach_bound_proposal(
                catalog,
                &analysis,
                crate::proposals::proposal_input(request)?,
                &current.binding,
            )
            .map_err(|e| uncertain_request(id, e))?;
            // Keep the exact saved carrier bytes, including unknown OKF keys.
            // The typed carrier and payload hash prove analysis equality without
            // regenerating or normalizing the stored document.
            if crate::proposals::validate_proposal_subject(&rebuilt)?
                != crate::proposals::validate_proposal_subject(saved.frozen())?
            {
                return Err(uncertain_request(
                    id,
                    "saved proposal impact/source changed; cannot restage",
                ));
            }
        }
        let expected = expected.or(recovered_context.as_ref());
        let result = Self::continue_shared_bound(session, catalog, &inputs, now, expected.map(|context| &context.private), |live| {
            if live.verified_sources.as_ref().is_none_or(|sources| sources.3 != refs) {
                return Err(HubError::Uncertain("proposal draft refs changed before acceptance".into()));
            }
            if let Some(expected) = expected {
                let current = captured(live, catalog, &request.body().host, Some(&expected.binding.run), now, &baseline)
                    .map_err(HubError::Invalid)?;
                if current.session != expected.session || current.task_run_hash != expected.task_run_hash
                    || current.binding != expected.binding || crate::recovery_contract::canonical_hash(&current.tasks).map_err(HubError::Invalid)?
                        != crate::recovery_contract::canonical_hash(&expected.tasks).map_err(HubError::Invalid)? {
                    return Err(HubError::Uncertain("proposal Task/Run/impact population changed".into()));
                }
            } else if event.guards.iter().all(|f| live.head.leases.get(&f.name)
                .is_some_and(|lease| lease.holder == f.holder && lease.epoch == f.epoch && lease.expires_at > now)) {
                return Err(HubError::Uncertain("saved proposal current context could not be proved; cannot grant a new acceptance".into()));
            }
            let ledger = live.baseline_requests.as_mut().ok_or_else(|| HubError::Invalid("request history is not prepared".into()))?;
            let identity = RequestIdentity::Save(request.clone());
            ledger.reserve(identity.clone()).map_err(HubError::Invalid)?;
            if let Some(original) = ledger.retry_original(catalog, &identity, &mut *live.hub, outbox, now)? { return Ok(Some(original)); }
            // The private draft already preserves this exact request. A pending
            // outbox write/fsync failure must keep its UUID uncertain, while
            // proven semantic refusals retain their existing classification.
            let receipt = recovery_outbox::append_durable(&mut *live.hub, outbox, event, now)
                .map_err(|error| match error {
                    HubError::Io(reason) => HubError::Uncertain(reason),
                    other => other,
                })?;
            match receipt {
                AppendReceipt::Event(actual) => Ok(Some(*actual)),
                AppendReceipt::Duplicate { .. } => Err(HubError::Uncertain("proposal duplicate lacks actual original Event".into())),
            }
        }).map_err(|e| uncertain_request(id, e))?;
        let actual =
            Self::finish_baseline_request_shared(session, catalog, outbox, id, result, now)?
                .ok_or_else(|| uncertain_request(id, "proposal has no accepted Event"))?;
        if actual.stale {
            return Err(uncertain_request(
                id,
                "proposal original was accepted stale; it cannot authorize a current proposal",
            ));
        }
        Ok(actual)
    }

    pub(crate) fn proposal_outcome_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        original: Event,
        now: u64,
    ) -> Result<ProposalOutcome, String> {
        let record: ProposalCreated = crate::proposals::decode_created(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&original),
        )?;
        let binding = &record.result.binding;
        let actual = Self::read_proposal_shared(
            session,
            catalog,
            inputs,
            &record.result.draft.draft_id,
            &record.result.draft.revision,
            now,
        )?;
        // Physical/source failures are not historical business validity. Prove
        // them outside the downgradable Task/Run context result.
        actual_worktree(catalog)?;
        let baseline = current_baseline(catalog, inputs)?;
        let current = if original.stale || actual.is_none() {
            Err("proposal has no current actual outer acceptance".into())
        } else if let Some(baseline) = baseline {
            Self::continue_shared(session, catalog, inputs, now, |live| {
                captured(
                    live,
                    catalog,
                    &original.host,
                    Some(&binding.run),
                    now,
                    &baseline,
                )
                .and_then(|context| {
                    if context.binding == *binding {
                        Ok(())
                    } else {
                        Err("proposal current Task/Run/baseline changed".into())
                    }
                })
            })
            .map_err(|error| error.to_string())?
        } else {
            Err("proposal has no current approved baseline".into())
        };
        // Even a historical receipt must not conceal a failed source proof.
        Self::sync_shared(session, catalog, inputs, now).map_err(|e| e.to_string())?;
        let current_validity = match current {
            Ok(()) => ReviewValidity::Current,
            Err(reason) => ReviewValidity::HistoricalOnly { reason },
        };
        Ok(ProposalOutcome {
            receipt: record.p3_receipt,
            current_validity,
        })
    }
}
