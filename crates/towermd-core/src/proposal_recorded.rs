//! Typed proposal receipts. A carrier is content; only a proved actual outer
//! event can establish that this exact materialized proposal was accepted.
use super::{ProposalCarrier, ProposalInput};
use crate::{
    catalog::Catalog,
    hub_protocol::{Fence, NewEvent},
    recovery_contract::{
        self,
        recorded::{request_event_eid, RequestBody, RequestMeta, RequestReceipt},
    },
    recovery_outbox::original_of,
    request_ledger::{DraftSaveAction, DraftSavedResult, PreparedDraftSaveRequest, ReviewValidity},
    snapshot_service::SavedDraft,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalBinding {
    pub task: String,
    pub run: String,
    pub baseline: String,
    pub fences: Vec<Fence>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalResult {
    pub draft: DraftSavedResult,
    pub binding: ProposalBinding,
    pub carrier: ProposalCarrier,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalCreated {
    pub result: ProposalResult,
    pub request_body: RequestBody<DraftSaveAction>,
    pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<ProposalResult>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalOutcome {
    pub receipt: RequestReceipt<ProposalResult>,
    pub current_validity: ReviewValidity,
}

pub(crate) fn decode_created(project: &str, event: &NewEvent) -> Result<ProposalCreated, String> {
    let record: ProposalCreated =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let request = PreparedDraftSaveRequest::from_recorded(
        record.request_body.clone(),
        record.p3_request.clone(),
    )?;
    let DraftSaveAction::Propose { input } = &request.body().action else {
        return Err("proposal.created requires the original Propose action".into());
    };
    let result = &record.result;
    let binding = &result.binding;
    crate::hub::validate_token("task", &binding.task).map_err(|e| e.to_string())?;
    crate::hub::validate_token("run", &binding.run).map_err(|e| e.to_string())?;
    recovery_contract::object_id(&result.draft.revision)?;
    recovery_contract::sha256_hex(&result.draft.content_hash)?;
    recovery_contract::sha256_hex(&result.carrier.payload_hash)?;
    recovery_contract::object_id(&result.carrier.main_revision)?;
    crate::baseline_current::baseline_number(&binding.baseline)?;
    let names: BTreeSet<_> = binding.fences.iter().map(|f| &f.name).collect();
    if record.request_body.project_id != project
        || record.request_body.actor != "process:towermd-cli"
        || record
            .request_body
            .run
            .as_ref()
            .is_some_and(|selector| selector != &binding.run)
        || result.carrier.schema != 1
        || result.carrier.input != *input
        || result.carrier.run != binding.run
        || result.carrier.baseline != binding.baseline
        || result.carrier.binding.as_ref() != Some(binding)
        || binding.fences.is_empty()
        || names.len() != binding.fences.len()
        || binding
            .fences
            .iter()
            .any(|f| !f.name.starts_with("module:") || f.holder != binding.run || f.epoch == 0)
        || result.draft.draft_id != request.draft_id()
        || result.draft.reference
            != crate::repository_writer::RepositoryRef::draft(request.draft_id())?.name()
        || event.kind != "proposal.created"
        || event.layer != "L4"
        || event.actor != request.body().actor
        || event.host != request.body().host
        || event.obj != request.draft_id()
        || event.guards != binding.fences
        || event.eid != request_event_eid(&record.p3_request.request_id, "proposal.created")?
        || record.p3_receipt.request_id != record.p3_request.request_id
        || record.p3_receipt.body_hash != record.p3_request.body_hash
        || record.p3_receipt.result_ref != *result
    {
        return Err("proposal original request/receipt/carrier/full-fences mismatch".into());
    }
    Ok(record)
}

pub(crate) fn require_saved(record: &ProposalCreated, saved: &SavedDraft) -> Result<(), String> {
    let request = PreparedDraftSaveRequest::from_recorded(
        record.request_body.clone(),
        record.p3_request.clone(),
    )?;
    if saved.request() != &request
        || saved.revision() != record.result.draft.revision
        || saved.frozen().content_hash != record.result.draft.content_hash
        || super::validate_proposal_subject(saved.frozen())?.as_ref()
            != Some(&record.result.carrier)
    {
        return Err("proposal receipt does not name its exact original saved draft".into());
    }
    Ok(())
}

pub(crate) fn proposal_input(request: &PreparedDraftSaveRequest) -> Result<&ProposalInput, String> {
    match &request.body().action {
        DraftSaveAction::Propose { input } => Ok(input),
        _ => Err("not a proposal request".into()),
    }
}

/// Validate accepted proposals against the actual Task/Run schedule, rather
/// than trusting a self-described binding or an inner confirmation role.
pub(crate) fn validate_actual_history(
    catalog: &Catalog,
    history: &crate::recovery_git::VerifiedRecoveryHistory,
    reads: &mut crate::recovery_git::PublicationReads,
) -> Result<(), String> {
    let events = history.actual_outer_events(catalog)?;
    if !events
        .iter()
        .any(|event| !event.stale && event.kind == "proposal.created")
    {
        return Ok(());
    }
    let mut projection = crate::snapshot::StateProjection::default();
    let mut baseline: Option<String> = None;
    let mut publication_commit: Option<String> = None;
    let mut claimed = std::collections::BTreeMap::new();
    for event in events {
        if event.stale {
            continue;
        }
        if event.kind == "hub.rebuilt" {
            claimed.clear();
        }
        if event.kind == "baseline.created" {
            baseline = Some(
                event.data["target"]
                    .as_str()
                    .ok_or("actual publication lacks target")?
                    .into(),
            );
            publication_commit = Some(
                event.data["commit"]
                    .as_str()
                    .ok_or("actual publication lacks commit")?
                    .into(),
            );
        }
        if event.kind == "task.claimed" {
            let run: crate::tasks::Run =
                serde_json::from_value(event.data["run"].clone()).map_err(|e| e.to_string())?;
            claimed.insert(run.id, event.seq);
        }
        if event.kind == "proposal.created" {
            let record = decode_created(catalog.config.project_id(), &original_of(event))?;
            let binding = &record.result.binding;
            let task: crate::tasks::Task = projection
                .objects
                .get("Task")
                .and_then(|tasks| tasks.get(&binding.task))
                .cloned()
                .ok_or_else(|| "proposal lacks its preceding actual Task".to_string())
                .and_then(|raw| serde_json::from_value(raw).map_err(|e| e.to_string()))?;
            let run: crate::tasks::Run = projection
                .objects
                .get("Run")
                .and_then(|runs| runs.get(&binding.run))
                .cloned()
                .ok_or_else(|| "proposal lacks its preceding actual Run".to_string())
                .and_then(|raw| serde_json::from_value(raw).map_err(|e| e.to_string()))?;
            let source = Catalog::at_revision(&catalog.root, &record.result.carrier.main_revision)?
                .ok_or("proposal original authority has no project configuration")?;
            if source.common_dir != catalog.common_dir
                || source.config.to_yaml() != catalog.config.to_yaml()
            {
                return Err("proposal original project/configuration source changed".into());
            }
            crate::tasks::validate_task(&task.spec, &source)?;
            let expected: BTreeSet<_> = std::iter::once(&task.spec.module)
                .chain(task.spec.touches.iter())
                .map(|id| format!("module:{id}"))
                .collect();
            let names: BTreeSet<_> = binding.fences.iter().map(|f| f.name.clone()).collect();
            if task.object_type != "Task"
                || task.state != "running"
                || task.spec.id != binding.task
                || task
                    .extra
                    .get("needs_recheck")
                    .is_some_and(|flag| flag != &serde_json::Value::Bool(false))
                || run.object_type != "Run"
                || run.state != "running"
                || run.id != binding.run
                || run.task != binding.task
                || run.host != event.host
                || run.branch != format!("t/{}", task.spec.id)
                || !std::path::Path::new(&run.worktree).is_absolute()
                || run.leases != binding.fences
                || expected != names
                || claimed.get(&run.id).is_none_or(|seq| *seq >= event.seq)
                || baseline.as_ref() != Some(&binding.baseline)
            {
                return Err("proposal accepted provenance differs from actual preceding Task/Run/full-fences/baseline".into());
            }
            // A source OID is historical authority only on the actual main line,
            // after the publication proved by this same actual outer prefix.
            let main = crate::repo::git(
                &catalog.root,
                &["rev-list", "--first-parent", &catalog.revision],
            )?;
            if !main.lines().any(|revision| revision == source.revision) {
                return Err(
                    "proposal original authority is not on actual main first-parent history".into(),
                );
            }
            crate::repo::git(
                &catalog.root,
                &[
                    "merge-base",
                    "--is-ancestor",
                    publication_commit
                        .as_deref()
                        .ok_or("proposal lacks preceding actual publication")?,
                    &source.revision,
                ],
            )?;
            // The accepted result must name the exact immutable writer commit,
            // even if its user-facing ref has since moved or been deleted.
            let saved = reads.saved_draft(
                catalog,
                &source,
                &record.result.draft.draft_id,
                &record.result.draft.revision,
            )?;
            require_saved(&record, &saved)?;
            let tasks = history.task_population_at(catalog, event.seq - 1)?;
            let analysis = super::analyze_historical(
                &source,
                &binding.baseline,
                &record.result.carrier.input,
                &tasks,
                &record.result.draft.draft_id,
                catalog,
                reads,
            )?;
            let payload_source =
                Catalog::at_revision(&source.root, &analysis.bundle.source_revision)?
                    .ok_or("accepted proposal payload source is absent")?;
            let payload = reads.frozen_source(catalog, &payload_source, &analysis.bundle)?;
            let mut saved_payload = saved.frozen().bundle.clone();
            saved_payload
                .objects
                .remove(&record.result.draft.draft_id)
                .ok_or("accepted proposal saved payload lacks carrier")?;
            if analysis.baseline != record.result.carrier.baseline
                || analysis.main_revision != record.result.carrier.main_revision
                || analysis.impact != record.result.carrier.impact
                || payload.content_hash != record.result.carrier.payload_hash
                || saved_payload != analysis.bundle
            {
                return Err(
                    "proposal accepted payload/source/impact differs from original actual analysis"
                        .into(),
                );
            }
        }
        // The canonical reader already proves merge.done and rejects raw
        // task.state merged. Reuse its one projection implementation.
        projection.apply_recorded(event)?;
    }
    Ok(())
}
