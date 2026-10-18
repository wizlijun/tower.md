//! Rebuildable request bookkeeping for baseline App and publication actions.
//!
//! No constructor accepts public event vectors. The actual-history integration
//! owns construction; pending producer intents never certify an accepted action.
use crate::{
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError, HubProtocol, NewEvent},
    recovery_contract::{
        self,
        baseline::{ApprovalFact, Verdict},
        recorded::{
            request_event_eid, BaselineCreatedAction, BaselineReviewAction, BaselineReviewRecorded,
            PublicationPrepareAction, RequestBody, RequestMeta, RequestReceipt,
        },
    },
    recovery_outbox::{self, RecoveryOutbox},
};
use std::collections::BTreeMap;

/// A retriable original request whose acceptance/materialization cannot now be
/// ruled out. Adapters must retain its identity rather than resolve rejection.
pub(crate) fn uncertain_request(request_id: &str, reason: impl std::fmt::Display) -> String {
    format!("baseline request {request_id} uncertain: {reason}")
}

#[path = "draft_save.rs"]
mod save;
pub(crate) use save::PreparedDraftSaved;
pub use save::{save_draft_shared, DraftSaveReceipt, DraftSaveValidity};

pub type DraftSaveAction = crate::recovery_contract::recorded::DraftSaveAction<
    crate::baseline_schema::BaselineSubmit,
    crate::baseline_draft::DraftEdit,
>;
pub type DraftSavedRecorded = crate::recovery_contract::recorded::DraftSavedRecorded<
    crate::baseline_schema::BaselineSubmit,
    crate::baseline_draft::DraftEdit,
>;
pub use crate::recovery_contract::recorded::{DraftDelivery, DraftSavedResult};

/// Complete original request for materialization/recovery. This describes the
/// server's typed request; it does not certify a saved ref or an accepted receipt.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedDraftSaveRequest {
    body: RequestBody<DraftSaveAction>,
    meta: RequestMeta,
}
impl PreparedDraftSaveRequest {
    pub fn prepare(body: RequestBody<DraftSaveAction>, request_id: &str) -> Result<Self, String> {
        let meta = RequestMeta {
            request_id: request_id.into(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Self::from_recorded(body, meta)
    }
    pub(crate) fn from_recorded(
        body: RequestBody<DraftSaveAction>,
        meta: RequestMeta,
    ) -> Result<Self, String> {
        request_event_eid(&meta.request_id, "draft.saved")?;
        if body.project_id.is_empty() || recovery_contract::canonical_hash(&body)? != meta.body_hash
        {
            return Err("draft save request identity/hash mismatch".into());
        }
        crate::hub::validate_token("host", &body.host).map_err(|e| e.to_string())?;
        if let Some(run) = &body.run {
            crate::hub::validate_token("run", run).map_err(|e| e.to_string())?;
        }
        let human = body.actor.strip_prefix("human:").is_some_and(|id| {
            !id.is_empty()
                && !id.contains([':', '/'])
                && !id.chars().any(|c| c.is_whitespace() || c.is_control())
        });
        match &body.action {
            DraftSaveAction::CreateDraft {
                baseline,
                main_revision,
            } => {
                crate::baseline_current::baseline_number(baseline)?;
                recovery_contract::object_id(main_revision)?;
                if !human || body.run.is_some() {
                    return Err("draft creation requires a human request without a run".into());
                }
            }
            DraftSaveAction::SubmitBaseline { submit } => {
                crate::baseline_schema::parse_submit(
                    &serde_json::to_value(submit).map_err(|e| e.to_string())?,
                )?;
                if submit.project_id != body.project_id
                    || !(human || body.actor == "process:towermd-cli")
                {
                    return Err("submit request project/actor mismatch".into());
                }
            }
            DraftSaveAction::EditDraft {
                id,
                expected_revision,
                ..
            } => {
                recovery_contract::repo_path(id.as_bytes())?;
                if id.contains('/') || !human {
                    return Err("draft editing requires a human request and safe id".into());
                }
                recovery_contract::object_id(expected_revision)?;
            }
        }
        Ok(Self { body, meta })
    }
    pub fn body(&self) -> &RequestBody<DraftSaveAction> {
        &self.body
    }
    pub fn metadata(&self) -> &RequestMeta {
        &self.meta
    }
    pub fn draft_id(&self) -> &str {
        match &self.body.action {
            DraftSaveAction::CreateDraft { .. } => &self.meta.request_id,
            DraftSaveAction::SubmitBaseline { submit } => &submit.scan_id,
            DraftSaveAction::EditDraft { id, .. } => id,
        }
    }
    pub fn expected_revision(&self) -> Option<&str> {
        match &self.body.action {
            DraftSaveAction::SubmitBaseline { .. } | DraftSaveAction::CreateDraft { .. } => None,
            DraftSaveAction::EditDraft {
                expected_revision, ..
            } => Some(expected_revision),
        }
    }
}

/// The exact typed identity of one App baseline-review request. Constructing it
/// validates shape and identity; it does not establish App origin or approval.
#[derive(Clone, Debug, PartialEq)]
pub struct BaselineRequest {
    pub(crate) body: RequestBody<BaselineReviewAction>,
    pub(crate) meta: RequestMeta,
    pub(crate) event_eid: String,
    pub(crate) event_kind: &'static str,
}
impl BaselineRequest {
    pub fn body(&self) -> &RequestBody<BaselineReviewAction> {
        &self.body
    }
    pub fn metadata(&self) -> &RequestMeta {
        &self.meta
    }
    pub fn prepare(
        project_id: &str,
        host: &str,
        actor: &str,
        request_id: &str,
        action: BaselineReviewAction,
    ) -> Result<Self, String> {
        let human = actor
            .strip_prefix("human:")
            .filter(|id| !id.is_empty() && !id.contains([':', '/']))
            .ok_or("baseline review request requires a human actor")?;
        crate::hub::validate_token("human", human).map_err(|e| e.to_string())?;
        crate::hub::validate_token("host", host).map_err(|e| e.to_string())?;
        let BaselineReviewAction::ReviewBaseline {
            binding, verdict, ..
        } = &action;
        if project_id.is_empty() || binding.project_id != project_id {
            return Err("baseline review request project mismatch".into());
        }
        let event_kind = match verdict {
            Verdict::Approved => "baseline.approved",
            Verdict::Rejected => "baseline.rejected",
        };
        let event_eid = request_event_eid(request_id, event_kind)?;
        let body = RequestBody {
            project_id: project_id.into(),
            host: host.into(),
            actor: actor.into(),
            // App baseline review has no implicit or explicit Task run binding.
            run: None,
            action,
        };
        let meta = RequestMeta {
            request_id: request_id.into(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Ok(Self {
            body,
            meta,
            event_eid,
            event_kind,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RequestIdentity {
    Review(BaselineRequest),
    Save(PreparedDraftSaveRequest),
    Preparation {
        body: RequestBody<PublicationPrepareAction>,
        meta: RequestMeta,
    },
    Created {
        body: RequestBody<BaselineCreatedAction>,
        meta: RequestMeta,
    },
}
impl RequestIdentity {
    fn meta(&self) -> &RequestMeta {
        match self {
            Self::Review(r) => &r.meta,
            Self::Save(r) => &r.meta,
            Self::Preparation { meta, .. } | Self::Created { meta, .. } => meta,
        }
    }
    fn project(&self) -> &str {
        match self {
            Self::Review(r) => &r.body.project_id,
            Self::Save(r) => &r.body.project_id,
            Self::Preparation { body, .. } => &body.project_id,
            Self::Created { body, .. } => &body.project_id,
        }
    }
}

#[derive(Clone, Debug)]
struct Entry {
    request: RequestIdentity,
    original: Option<NewEvent>,
    accepted: Option<Event>,
}

/// One project/request_id index across App mutations and typed publication
/// actions. In-flight/materialized entries reserve identity, never a receipt.
pub(crate) struct BaselineRequestLedger {
    project_id: String,
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug)]
pub struct RecordedBaselineReceipt {
    pub receipt: RequestReceipt<ApprovalFact>,
    pub original: Event,
}
#[derive(Debug)]
pub struct RecordedDraftSaveReceipt {
    pub receipt: RequestReceipt<DraftSavedResult>,
    pub original: Event,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BaselineRequestStatus {
    Review {
        request_body: RequestBody<BaselineReviewAction>,
        p3_request: RequestMeta,
        receipt: Option<RequestReceipt<ApprovalFact>>,
    },
    DraftSave {
        request_body: RequestBody<DraftSaveAction>,
        p3_request: RequestMeta,
        receipt: Option<RequestReceipt<DraftSavedResult>>,
    },
}

impl BaselineRequestLedger {
    pub(crate) fn require_undecided(&self, request: &BaselineRequest) -> Result<(), String> {
        let BaselineReviewAction::ReviewBaseline { binding, .. } = &request.body.action;
        for entry in self.entries.values() {
            if let RequestIdentity::Review(previous) = &entry.request {
                let BaselineReviewAction::ReviewBaseline {
                    binding: previous, ..
                } = &previous.body.action;
                if entry
                    .accepted
                    .as_ref()
                    .is_some_and(|actual| !actual.stale && actual.guards.is_empty())
                    && previous == binding
                {
                    return Err("baseline review conflict: this exact draft already has a terminal decision; retry the original request".into());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn from_history(
        catalog: &Catalog,
        inputs: &recovery_outbox::RecoveryInputs,
        history: &crate::recovery_git::VerifiedRecoveryHistory,
    ) -> Result<Self, String> {
        inputs.probe(catalog)?;
        let mut ledger = Self::empty(catalog.config.project_id());
        for event in history.baseline_request_originals(catalog)? {
            ledger.accepted(event)?;
        }
        for event in history.publication_prepared_originals(catalog)? {
            ledger.accepted(event)?;
        }
        for event in history.publication_created_originals(catalog)? {
            ledger.accepted(event)?;
        }
        for original in inputs
            .pending()
            .iter()
            .filter(|e| is_indexed_request(&e.kind))
        {
            ledger.pending(original)?;
        }
        for saved in crate::snapshot_service::read_all_drafts(catalog).map_err(|e| e.to_string())? {
            ledger.reserve(RequestIdentity::Save(saved.request().clone()))?;
        }
        inputs.probe(catalog)?;
        Ok(ledger)
    }
    fn empty(project_id: &str) -> Self {
        Self {
            project_id: project_id.into(),
            entries: BTreeMap::new(),
        }
    }
    pub(crate) fn reserve(&mut self, request: RequestIdentity) -> Result<(), String> {
        if self.matching_identity(&request)?.is_none() {
            self.entries.insert(
                request.meta().request_id.clone(),
                Entry {
                    request,
                    original: None,
                    accepted: None,
                },
            );
        }
        Ok(())
    }
    /// Preserve process-local reservations across actual-history refreshes. This
    /// never manufactures an Event or receipt and cannot overwrite a real fact.
    pub(crate) fn retain_inflight(&mut self, previous: &Self) -> Result<(), String> {
        for entry in previous.entries.values().filter(|e| e.original.is_none()) {
            self.reserve(entry.request.clone())?;
        }
        Ok(())
    }
    fn pending(&mut self, original: &NewEvent) -> Result<(), String> {
        self.insert(original, None)
    }
    fn accepted(&mut self, event: &Event) -> Result<(), String> {
        let original = recovery_outbox::original_of(event);
        recovery_outbox::validate_actual(&original, event)?;
        self.insert(&original, Some(event))
    }
    fn insert(&mut self, original: &NewEvent, accepted: Option<&Event>) -> Result<(), String> {
        let request = decode_identity(&self.project_id, original)?;
        if let Some(previous) = self.matching_identity(&request)? {
            if previous.original.as_ref().is_some_and(|e| e != original) {
                return Err("baseline request has conflicting original or receipt".into());
            }
            if previous
                .accepted
                .as_ref()
                .zip(accepted)
                .is_some_and(|(a, b)| a != b)
            {
                return Err("baseline request has conflicting accepted receipts".into());
            }
            if previous.accepted.is_some() || (previous.original.is_some() && accepted.is_none()) {
                return Ok(());
            }
        }
        self.entries.insert(
            request.meta().request_id.clone(),
            Entry {
                request,
                original: Some(original.clone()),
                accepted: accepted.cloned(),
            },
        );
        Ok(())
    }
    fn matching_identity(&self, request: &RequestIdentity) -> Result<Option<&Entry>, String> {
        if self.project_id != request.project() {
            return Err("request ledger belongs to another project".into());
        }
        let Some(entry) = self.entries.get(&request.meta().request_id) else {
            return Ok(None);
        };
        if entry.request != *request {
            return Err("request_id reused with a different baseline action/body".into());
        }
        Ok(Some(entry))
    }
    #[cfg(test)]
    fn matching(&self, request: &BaselineRequest) -> Result<Option<&Entry>, String> {
        self.matching_identity(&RequestIdentity::Review(request.clone()))
    }
    pub(crate) fn retry_original(
        &self,
        catalog: &Catalog,
        request: &RequestIdentity,
        hub: &mut dyn HubProtocol,
        outbox: &mut RecoveryOutbox,
        now_ms: u64,
    ) -> Result<Option<Event>, HubError> {
        outbox
            .read_inputs()
            .map_err(HubError::Uncertain)?
            .check_prepared_context(catalog)
            .map_err(HubError::Invalid)?;
        let Some(entry) = self.matching_identity(request).map_err(HubError::Invalid)? else {
            return Ok(None);
        };
        let Some(original) = &entry.original else {
            return Ok(None);
        };
        if let Some(actual) = &entry.accepted {
            outbox
                .append_pending(original)
                .map_err(HubError::Uncertain)?;
            outbox
                .persist_confirmed(original, actual)
                .map_err(HubError::Uncertain)?;
            return Ok(Some(actual.clone()));
        }
        match recovery_outbox::append_durable(hub, outbox, original.clone(), now_ms)
            .map_err(|error| HubError::Uncertain(error.to_string()))?
        {
            AppendReceipt::Event(actual) => Ok(Some(*actual)),
            AppendReceipt::Duplicate { .. } => Err(HubError::Uncertain(
                "baseline retry lacks actual original receipt".into(),
            )),
        }
    }
    pub(crate) fn retry(
        &self,
        catalog: &Catalog,
        request: &BaselineRequest,
        hub: &mut dyn HubProtocol,
        outbox: &mut RecoveryOutbox,
        now_ms: u64,
    ) -> Result<Option<RecordedBaselineReceipt>, HubError> {
        self.retry_original(
            catalog,
            &RequestIdentity::Review(request.clone()),
            hub,
            outbox,
            now_ms,
        )?
        .map(|event| recorded_receipt(&self.project_id, event).map_err(HubError::Invalid))
        .transpose()
    }
    pub(crate) fn retry_save(
        &self,
        catalog: &Catalog,
        request: &PreparedDraftSaveRequest,
        hub: &mut dyn HubProtocol,
        outbox: &mut RecoveryOutbox,
        now_ms: u64,
    ) -> Result<Option<RecordedDraftSaveReceipt>, HubError> {
        self.retry_original(
            catalog,
            &RequestIdentity::Save(request.clone()),
            hub,
            outbox,
            now_ms,
        )?
        .map(|event| recorded_save_receipt(&self.project_id, event).map_err(HubError::Invalid))
        .transpose()
    }
    pub(crate) fn preparations(&self, publication_id: &str) -> Vec<(RequestIdentity, NewEvent)> {
        self.entries
            .values()
            .filter_map(|entry| {
                if let RequestIdentity::Preparation { body, .. } = &entry.request {
                    let PublicationPrepareAction::PreparePublication {
                        publication_id: original,
                        ..
                    } = &body.action;
                    if original == publication_id {
                        return entry
                            .original
                            .as_ref()
                            .map(|event| (entry.request.clone(), event.clone()));
                    }
                }
                None
            })
            .collect()
    }
    pub(crate) fn status(
        &self,
        inputs: &recovery_outbox::RecoveryInputs,
        id: &str,
    ) -> Result<Option<BaselineRequestStatus>, String> {
        request_event_eid(id, "draft.saved")?;
        let Some(entry) = self.entries.get(id) else {
            return Ok(None);
        };
        let confirmed = entry
            .accepted
            .as_ref()
            .filter(|actual| inputs.receipt_confirmed(actual));
        Ok(Some(match &entry.request {
            RequestIdentity::Review(r) => BaselineRequestStatus::Review {
                request_body: r.body.clone(),
                p3_request: r.meta.clone(),
                receipt: confirmed
                    .map(|e| recorded_receipt(&self.project_id, e.clone()).map(|r| r.receipt))
                    .transpose()?,
            },
            RequestIdentity::Save(r) => BaselineRequestStatus::DraftSave {
                request_body: r.body.clone(),
                p3_request: r.meta.clone(),
                receipt: confirmed
                    .map(|e| recorded_save_receipt(&self.project_id, e.clone()).map(|r| r.receipt))
                    .transpose()?,
            },
            RequestIdentity::Preparation { .. } | RequestIdentity::Created { .. } => {
                return Ok(None)
            }
        }))
    }
}

/// Read actual recorded history and current materialized requests without
/// opening a Hub, registering a project or coordinating a pending producer.
pub fn read_baseline_request(
    catalog: &Catalog,
    inputs: &recovery_outbox::RecoveryInputs,
    request_id: &str,
) -> Result<Option<BaselineRequestStatus>, String> {
    request_event_eid(request_id, "draft.saved")?;
    let refs =
        crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?;
    let history = crate::recovery_git::read_recovery_history(catalog, inputs)?;
    let ledger = BaselineRequestLedger::from_history(catalog, inputs, &history)?;
    if crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())? != refs {
        return Err("draft refs changed during request read".into());
    }
    inputs.probe(catalog)?;
    ledger.status(inputs, request_id)
}

pub(crate) fn recorded_receipt(
    project_id: &str,
    event: Event,
) -> Result<RecordedBaselineReceipt, String> {
    let recorded = decode(project_id, &recovery_outbox::original_of(&event))?;
    Ok(RecordedBaselineReceipt {
        receipt: recorded.p3_receipt,
        original: event,
    })
}

pub(crate) fn is_baseline_review(kind: &str) -> bool {
    matches!(kind, "baseline.approved" | "baseline.rejected")
}

pub(crate) fn is_baseline_request(kind: &str) -> bool {
    is_baseline_review(kind) || kind == "draft.saved"
}

pub(crate) fn is_indexed_request(kind: &str) -> bool {
    is_baseline_request(kind)
        || matches!(kind, "baseline.publication_prepared" | "baseline.created")
}
// Explicitly reviewed recovery support. Adding another indexed action does not
// automatically permit it inside an accepted-receipt audit envelope.
pub(crate) fn is_restorable_request(kind: &str) -> bool {
    matches!(
        kind,
        "baseline.approved"
            | "baseline.rejected"
            | "draft.saved"
            | "baseline.publication_prepared"
            | "baseline.created"
    )
}

fn decode_identity(project_id: &str, event: &NewEvent) -> Result<RequestIdentity, String> {
    if matches!(
        event.kind.as_str(),
        "baseline.publication_prepared" | "baseline.created"
    ) {
        return crate::project_session::publication_events::decode_request(project_id, event);
    }
    if event.kind == "draft.saved" {
        let record = decode_saved(project_id, event)?;
        return Ok(RequestIdentity::Save(
            PreparedDraftSaveRequest::from_recorded(record.request_body, record.p3_request)?,
        ));
    }
    let record = decode(project_id, event)?;
    Ok(RequestIdentity::Review(BaselineRequest::prepare(
        project_id,
        &record.request_body.host,
        &record.request_body.actor,
        &record.p3_request.request_id,
        record.request_body.action,
    )?))
}

fn decode_saved(project_id: &str, event: &NewEvent) -> Result<DraftSavedRecorded, String> {
    if event.kind != "draft.saved" {
        return Err("not a typed draft save record".into());
    }
    let record: DraftSavedRecorded =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let request = PreparedDraftSaveRequest::from_recorded(
        record.request_body.clone(),
        record.p3_request.clone(),
    )?;
    let result = &record.result;
    recovery_contract::object_id(&result.revision)?;
    recovery_contract::sha256_hex(&result.content_hash)?;
    let target = crate::repository_writer::RepositoryRef::draft(request.draft_id())?;
    if request.body.project_id != project_id
        || result.draft_id != request.draft_id()
        || result.reference != target.name()
        || record.p3_receipt.request_id != request.meta.request_id
        || record.p3_receipt.body_hash != request.meta.body_hash
        || record.p3_receipt.result_ref != *result
        || event.eid != request_event_eid(&request.meta.request_id, "draft.saved")?
        || event.actor != request.body.actor
        || event.host != request.body.host
        || event.obj != result.draft_id
        || event.layer != "L1"
        || !event.guards.is_empty()
    {
        return Err("draft save request/result differs from original event envelope".into());
    }
    Ok(record)
}

pub(crate) fn recorded_save_receipt(
    project_id: &str,
    event: Event,
) -> Result<RecordedDraftSaveReceipt, String> {
    let record = decode_saved(project_id, &recovery_outbox::original_of(&event))?;
    Ok(RecordedDraftSaveReceipt {
        receipt: record.p3_receipt,
        original: event,
    })
}

fn decode(project_id: &str, event: &NewEvent) -> Result<BaselineReviewRecorded, String> {
    if !matches!(
        event.kind.as_str(),
        "baseline.approved" | "baseline.rejected"
    ) {
        return Err("request ledger only accepts shared baseline review records".into());
    }
    recovery_contract::validate_recorded_data(project_id, &event.kind, &event.data)?;
    let recorded: BaselineReviewRecorded =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if event.eid != recorded.approval.approval_eid
        || event.actor != recorded.request_body.actor
        || event.host != recorded.request_body.host
        || event.obj != recorded.approval.binding.object_id
        || event.layer != "L1"
    {
        return Err("baseline request record differs from original event envelope".into());
    }
    Ok(recorded)
}

/// Internal audit envelope for an already reader-verified original. It retains
/// the complete accepted coordinates and cannot itself act as a new approval.
pub(crate) fn recovery_receipt(
    project_id: &str,
    original: &Event,
    host: &str,
) -> Result<NewEvent, String> {
    let new = recovery_outbox::original_of(original);
    recovery_outbox::validate_actual(&new, original)?;
    if !is_restorable_request(&new.kind) {
        return Err("request recovery audit does not support this typed action".into());
    }
    decode_identity(project_id, &new)?;
    crate::hub::validate_token("host", host).map_err(|e| e.to_string())?;
    let hash = recovery_contract::canonical_hash(original)?;
    Ok(NewEvent {
        eid: crate::events::derived_eid(&hash, "request.receipt.restored")?,
        host: host.into(),
        actor: "process:towermd-daemon".into(),
        kind: "request.receipt.restored".into(),
        layer: "L5".into(),
        obj: original.obj.clone(),
        guards: vec![],
        data: serde_json::json!({"schema":1,"original":original}),
    })
}

pub(crate) fn restored_original(project_id: &str, event: &Event) -> Result<Option<Event>, String> {
    if event.kind != "request.receipt.restored" {
        return Ok(None);
    }
    if event.stale
        || event.epoch.is_some()
        || event.cost_usd.is_some()
        || event.legacy
        || event.actor != "process:towermd-daemon"
        || event.layer != "L5"
        || !event.guards.is_empty()
        || event.data["schema"] != 1
    {
        return Err("invalid baseline request recovery receipt envelope".into());
    }
    let original: Event = recovery_contract::deserialize_event(event.data["original"].clone())
        .map_err(|e| e.to_string())?;
    let canonical = recovery_receipt(project_id, &original, &event.host)?;
    if recovery_outbox::original_of(event) != canonical || event.seq <= original.seq {
        return Err("baseline request restored receipt differs from exact original".into());
    }
    Ok(Some(original))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery_contract::baseline::ReviewBinding;
    use serde_json::json;

    fn request(verdict: Verdict) -> BaselineRequest {
        BaselineRequest::prepare(
            "project",
            "host",
            "human:owner",
            "0192f0aa-0000-7000-8000-000000000001",
            BaselineReviewAction::ReviewBaseline {
                binding: ReviewBinding {
                    project_id: "project".into(),
                    object_id: "scan".into(),
                    content_hash: "a".repeat(64),
                    target_baseline: "towermd/B0".into(),
                    draft_revision: "b".repeat(40),
                },
                verdict,
                comment: "exact original comment".into(),
            },
        )
        .unwrap()
    }
    /// A layout fixture only. It is not an actual-history reader result or proof.
    fn pending(request: &BaselineRequest) -> NewEvent {
        let BaselineReviewAction::ReviewBaseline {
            binding,
            verdict,
            comment,
        } = &request.body.action;
        let approval = ApprovalFact {
            approval_eid: request.event_eid.clone(),
            binding: binding.clone(),
            actor: request.body.actor.clone(),
            verdict: verdict.clone(),
            comment: comment.clone(),
        };
        let recorded = BaselineReviewRecorded {
            approval: approval.clone(),
            request_body: request.body.clone(),
            p3_request: request.meta.clone(),
            p3_receipt: RequestReceipt {
                request_id: request.meta.request_id.clone(),
                body_hash: request.meta.body_hash.clone(),
                result_ref: approval,
            },
        };
        NewEvent {
            eid: request.event_eid.clone(),
            host: "host".into(),
            actor: "human:owner".into(),
            kind: request.event_kind.into(),
            layer: "L1".into(),
            obj: "scan".into(),
            data: serde_json::to_value(recorded).unwrap(),
            guards: vec![],
        }
    }

    #[test]
    fn identity_reuses_shared_canonical_hash_and_uuid_event_derivation() {
        let request = request(Verdict::Approved);
        assert_eq!(
            request.meta.body_hash,
            recovery_contract::canonical_hash(&request.body).unwrap()
        );
        assert_eq!(
            request.event_eid,
            request_event_eid(&request.meta.request_id, "baseline.approved").unwrap()
        );
        assert!(request.body.run.is_none());
        assert!(BaselineRequest::prepare(
            "project",
            "host",
            "process:towermd-cli",
            &request.meta.request_id,
            request.body.action.clone()
        )
        .is_err());
        assert!(BaselineRequest::prepare(
            "project",
            "host",
            "human:owner",
            "not-a-uuid",
            request.body.action.clone()
        )
        .is_err());
        assert!(BaselineRequest::prepare(
            "other",
            "host",
            "human:owner",
            &request.meta.request_id,
            request.body.action.clone()
        )
        .is_err());
    }
    #[test]
    fn pending_is_uncertain_even_when_it_contains_a_planned_success_receipt() {
        let request = request(Verdict::Approved);
        let mut ledger = BaselineRequestLedger::empty("project");
        ledger.pending(&pending(&request)).unwrap();
        let entry = ledger.matching(&request).unwrap().unwrap();
        assert!(entry.accepted.is_none());
        assert_eq!(
            decode("project", entry.original.as_ref().unwrap())
                .unwrap()
                .p3_receipt
                .result_ref
                .verdict,
            Verdict::Approved
        );
        // matching deliberately supplies no successful receipt API.
        assert_eq!(entry.original.as_ref().unwrap().eid, request.event_eid);
    }
    #[test]
    fn baseline_review_envelope_requires_the_shared_l1_contract() {
        for verdict in [Verdict::Approved, Verdict::Rejected] {
            let request = request(verdict);
            let original = pending(&request);
            assert_eq!(original.layer, "L1");
            assert!(decode("project", &original).is_ok());
            for field in ["layer", "actor", "host", "obj", "eid"] {
                let mut changed = original.clone();
                match field {
                    "layer" => changed.layer = "L5".into(),
                    "actor" => changed.actor = "human:other".into(),
                    "host" => changed.host = "other-host".into(),
                    "obj" => changed.obj = "other-draft".into(),
                    "eid" => changed.eid = "a".repeat(64),
                    _ => unreachable!(),
                }
                assert!(decode("project", &changed).is_err(), "{field}");
                let mut ledger = BaselineRequestLedger::empty("project");
                assert!(ledger.pending(&changed).is_err(), "{field}");
                assert!(ledger.entries.is_empty());
            }
        }
    }
    #[test]
    fn same_id_cannot_change_verdict_comment_binding_actor_or_host() {
        let original = request(Verdict::Approved);
        let mut ledger = BaselineRequestLedger::empty("project");
        ledger.pending(&pending(&original)).unwrap();
        assert!(ledger.matching(&request(Verdict::Rejected)).is_err());
        for key in ["comment", "binding", "actor", "host"] {
            let mut body = serde_json::to_value(&original.body).unwrap();
            match key {
                "comment" => body["action"]["comment"] = json!("different"),
                "binding" => body["action"]["binding"]["draft_revision"] = json!("c".repeat(40)),
                _ => body[key] = json!("different"),
            }
            let body = serde_json::from_value(body).unwrap();
            let changed = BaselineRequest {
                meta: RequestMeta {
                    body_hash: recovery_contract::canonical_hash(&body).unwrap(),
                    ..original.meta.clone()
                },
                body,
                ..original.clone()
            };
            assert!(ledger.matching(&changed).is_err(), "{key}");
        }
    }
    #[test]
    fn duplicate_pending_must_preserve_every_original_field_and_receipt() {
        let request = request(Verdict::Approved);
        let original = pending(&request);
        let mut ledger = BaselineRequestLedger::empty("project");
        ledger.pending(&original).unwrap();
        ledger.pending(&original).unwrap();
        let mut changed = original.clone();
        changed.guards = vec![crate::hub_protocol::Fence {
            name: "main".into(),
            holder: "writer".into(),
            epoch: 7,
        }];
        assert!(ledger.pending(&changed).is_err());
        let mut changed = original.clone();
        changed.data["p3_receipt"]["result_ref"]["comment"] = json!("rewritten result");
        assert!(ledger.pending(&changed).is_err());
        let mut changed = original.clone();
        changed.host = "other-host".into();
        assert!(ledger.pending(&changed).is_err());
        assert_eq!(
            ledger
                .matching(&request)
                .unwrap()
                .unwrap()
                .original
                .as_ref()
                .unwrap(),
            &original
        );
    }
    #[test]
    fn unrelated_p3_and_authorization_fields_are_not_a_generic_event_escape_hatch() {
        let request = request(Verdict::Approved);
        let original = pending(&request);
        let mut ledger = BaselineRequestLedger::empty("project");
        for kind in [
            "hold.reviewed",
            "baseline.created",
            "hold.answered",
            "draft.saved",
        ] {
            let mut event = original.clone();
            event.kind = kind.into();
            assert!(ledger.pending(&event).is_err(), "{kind}");
        }
        let mut event = original;
        event.data["request_body"]["run"] = json!("implicit-run");
        assert!(ledger.pending(&event).is_err());
        assert!(ledger.entries.is_empty());
    }

    #[test]
    fn pending_and_confirmation_fsync_failures_never_return_a_success_receipt() {
        use crate::{
            project_session::ProjectSession, recovery_outbox::RecoveryContext, test_support,
        };
        use std::sync::Mutex;
        let root = tempfile::tempdir().unwrap();
        let catalog = test_support::catalog(root.path());
        let runtime = tempfile::tempdir().unwrap();
        let mut outbox = RecoveryOutbox::open(
            &runtime.path().join("outbox"),
            RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        let session = Mutex::new(
            ProjectSession::open(
                &runtime.path().join("hub"),
                &catalog,
                &outbox.read_inputs().unwrap(),
                100,
            )
            .unwrap(),
        );
        let request = request(Verdict::Approved);
        let original = pending(&request);
        outbox.append_pending(&original).unwrap();
        outbox.fail_next_pending();
        assert!(ProjectSession::retry_baseline_review_shared(
            &session,
            &catalog,
            &mut outbox,
            &request,
            100
        )
        .is_err());
        assert!(session.lock().unwrap().events(0).is_empty());
        for now in [101, 102] {
            outbox.fail_next_confirmation();
            assert!(ProjectSession::retry_baseline_review_shared(
                &session,
                &catalog,
                &mut outbox,
                &request,
                now
            )
            .unwrap_err()
            .contains("uncertain"));
            let events = session.lock().unwrap().events(0);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].eid, original.eid);
        }
        let receipt = ProjectSession::retry_baseline_review_shared(
            &session,
            &catalog,
            &mut outbox,
            &request,
            103,
        )
        .unwrap()
        .unwrap();
        assert_eq!(receipt.original.eid, original.eid);
        assert_eq!(
            outbox.read_inputs().unwrap().confirmed(),
            &[receipt.original]
        );
        assert_eq!(session.lock().unwrap().events(0).len(), 1);
    }

    #[test]
    fn audit_wrapper_preserves_full_original_and_rejects_tampering_and_other_actions() {
        let storage = tempfile::tempdir().unwrap();
        let mut hub = crate::hub::Hub::open(storage.path()).unwrap();
        let original = pending(&request(Verdict::Approved));
        let AppendReceipt::Event(original) = hub.append(original, 100).unwrap() else {
            panic!("actual original Event")
        };
        let wrapper = recovery_receipt("project", &original, "recovery-host").unwrap();
        let AppendReceipt::Event(wrapper) = hub.append(wrapper, 101).unwrap() else {
            panic!("actual audit Event")
        };
        assert_eq!(
            restored_original("project", &wrapper).unwrap().unwrap(),
            *original
        );
        for field in [
            "stale",
            "host",
            "actor",
            "guards",
            "body_hash",
            "sequence",
            "cost",
        ] {
            let mut changed = (*wrapper).clone();
            match field {
                "stale" => changed.data["original"]["stale"] = json!(true),
                "host" => changed.data["original"]["host"] = json!("other"),
                "actor" => changed.data["original"]["actor"] = json!("human:other"),
                "guards" => {
                    changed.data["original"]["guards"] =
                        json!([{"name":"main","holder":"old","epoch":7}])
                }
                "body_hash" => {
                    changed.data["original"]["data"]["p3_request"]["body_hash"] =
                        json!("c".repeat(64))
                }
                "sequence" => changed.data["original"]["seq"] = json!(999),
                _ => changed.data["original"]["cost_usd"] = json!(0),
            }
            assert!(restored_original("project", &changed).is_err(), "{field}");
        }
        let mut changed = (*wrapper).clone();
        changed.data["original"] = serde_json::to_value(&wrapper).unwrap();
        assert!(restored_original("project", &changed).is_err());
        let mut changed = (*wrapper).clone();
        changed.data["schema"] = json!(2);
        assert!(restored_original("project", &changed).is_err());
        assert!(restored_original("other-project", &wrapper).is_err());
    }

    #[test]
    fn stale_guarded_original_remains_historical_in_the_audit_envelope() {
        let storage = tempfile::tempdir().unwrap();
        let mut hub = crate::hub::Hub::open(storage.path()).unwrap();
        let mut original = pending(&request(Verdict::Rejected));
        original.guards.push(crate::hub_protocol::Fence {
            name: "module:A".into(),
            holder: "old-run".into(),
            epoch: 7,
        });
        let AppendReceipt::Event(original) = hub.append(original, 100).unwrap() else {
            panic!("actual original Event")
        };
        assert!(original.stale);
        let wrapper = recovery_receipt("project", &original, "recovery-host").unwrap();
        let AppendReceipt::Event(wrapper) = hub.append(wrapper, 101).unwrap() else {
            panic!("actual audit Event")
        };
        let restored = restored_original("project", &wrapper).unwrap().unwrap();
        assert_eq!(restored, *original);
        assert!(restored.stale);
        assert_eq!(restored.guards[0].holder, "old-run");
    }
}
