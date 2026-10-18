//! Rebuildable request bookkeeping for the shared baseline-review contract.
//!
//! No constructor accepts public event vectors. The actual-history integration
//! owns construction; pending producer intents never certify an accepted review.
use crate::{
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError, HubProtocol, NewEvent},
    recovery_contract::{
        self,
        baseline::{ApprovalFact, Verdict},
        recorded::{
            request_event_eid, BaselineReviewAction, BaselineReviewRecorded, RequestBody,
            RequestMeta, RequestReceipt,
        },
    },
    recovery_outbox::{self, RecoveryOutbox},
};
use std::collections::BTreeMap;

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
            DraftSaveAction::SubmitBaseline { submit } => &submit.scan_id,
            DraftSaveAction::EditDraft { id, .. } => id,
        }
    }
    pub fn expected_revision(&self) -> Option<&str> {
        match &self.body.action {
            DraftSaveAction::SubmitBaseline { .. } => None,
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

#[derive(Clone, Debug)]
struct Entry {
    body: RequestBody<BaselineReviewAction>,
    meta: RequestMeta,
    original: NewEvent,
    accepted: Option<Event>,
    receipt: RequestReceipt<ApprovalFact>,
}

/// There is deliberately no public or crate-visible empty/raw-record constructor.
/// The actual-history reader will supply the only production construction path.
pub(crate) struct BaselineRequestLedger {
    project_id: String,
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug)]
pub struct RecordedBaselineReceipt {
    pub receipt: RequestReceipt<ApprovalFact>,
    /// Original accepted event, including its original guards/stale status. The
    /// caller must independently recheck the current review binding for validity.
    pub original: Event,
}

impl BaselineRequestLedger {
    pub(crate) fn require_undecided(&self, request: &BaselineRequest) -> Result<(), String> {
        let BaselineReviewAction::ReviewBaseline { binding, .. } = &request.body.action;
        if self.entries.values().any(|entry| {
            entry
                .accepted
                .as_ref()
                .is_some_and(|actual| !actual.stale && actual.guards.is_empty())
                && entry.receipt.result_ref.binding == *binding
        }) {
            return Err("baseline review conflict: this exact draft already has a terminal decision; retry the original request".into());
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
        for event in history.baseline_review_originals(catalog)? {
            ledger.accepted(event)?;
        }
        for original in inputs
            .pending()
            .iter()
            .filter(|original| is_baseline_review(&original.kind))
        {
            ledger.pending(original)?;
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

    // Only the actual core reader/integration may call these private ingestion
    // functions. Unit fixtures below exercise layouts, never reader provenance.
    fn pending(&mut self, original: &NewEvent) -> Result<(), String> {
        let recorded = decode(&self.project_id, original)?;
        self.insert(original, None, recorded)
    }

    fn accepted(&mut self, event: &Event) -> Result<(), String> {
        let original = recovery_outbox::original_of(event);
        recovery_outbox::validate_actual(&original, event)?;
        let recorded = decode(&self.project_id, &original)?;
        self.insert(&original, Some(event), recorded)
    }

    fn insert(
        &mut self,
        original: &NewEvent,
        accepted: Option<&Event>,
        recorded: BaselineReviewRecorded,
    ) -> Result<(), String> {
        let id = recorded.p3_request.request_id.clone();
        if let Some(previous) = self.entries.get(&id) {
            if previous.meta != recorded.p3_request || previous.body != recorded.request_body {
                return Err("request_id reused with a different baseline review body".into());
            }
            if previous.receipt != recorded.p3_receipt || previous.original != *original {
                return Err("baseline review request has conflicting original or receipt".into());
            }
            if previous
                .accepted
                .as_ref()
                .zip(accepted)
                .is_some_and(|(a, b)| a != b)
            {
                return Err("baseline review request has conflicting accepted receipts".into());
            }
            if previous.accepted.is_some() || accepted.is_none() {
                return Ok(());
            }
        }
        self.entries.insert(
            id,
            Entry {
                body: recorded.request_body,
                meta: recorded.p3_request,
                original: original.clone(),
                accepted: accepted.cloned(),
                receipt: recorded.p3_receipt,
            },
        );
        Ok(())
    }

    fn matching(&self, request: &BaselineRequest) -> Result<Option<&Entry>, String> {
        if self.project_id != request.body.project_id {
            return Err("request ledger belongs to another project".into());
        }
        let Some(entry) = self.entries.get(&request.meta.request_id) else {
            return Ok(None);
        };
        if entry.meta != request.meta || entry.body != request.body {
            return Err("request_id reused with a different baseline review body".into());
        }
        if entry.original.eid != request.event_eid || entry.original.kind != request.event_kind {
            return Err("baseline review request original identity mismatch".into());
        }
        Ok(Some(entry))
    }

    /// Retry an already known original. A pending envelope's planned result is
    /// never returned: only the shared durable append's actual Event, or a real
    /// reader-confirmed original whose confirmation has just been fsynced, can
    /// yield a receipt. No new review decision or replacement event is planned.
    pub(crate) fn retry(
        &self,
        catalog: &Catalog,
        request: &BaselineRequest,
        hub: &mut dyn HubProtocol,
        outbox: &mut RecoveryOutbox,
        now_ms: u64,
    ) -> Result<Option<RecordedBaselineReceipt>, HubError> {
        if catalog.config.project_id() != self.project_id {
            return Err(HubError::Invalid(
                "request ledger Catalog project mismatch".into(),
            ));
        }
        outbox
            .read_inputs()
            .map_err(HubError::Io)?
            .check_prepared_context(catalog)
            .map_err(HubError::Invalid)?;
        let Some(entry) = self.matching(request).map_err(HubError::Invalid)? else {
            return Ok(None);
        };
        let actual = if let Some(actual) = &entry.accepted {
            // The reader already established this exact original. Preserve its
            // original identity/guards instead of re-appending it to a new hub.
            outbox
                .append_pending(&entry.original)
                .map_err(HubError::Uncertain)?;
            outbox
                .persist_confirmed(&entry.original, actual)
                .map_err(HubError::Uncertain)?;
            actual.clone()
        } else {
            match recovery_outbox::append_durable(hub, outbox, entry.original.clone(), now_ms)? {
                AppendReceipt::Event(actual) => *actual,
                AppendReceipt::Duplicate { .. } => {
                    return Err(HubError::Uncertain(
                        "baseline retry lacks actual original receipt".into(),
                    ))
                }
            }
        };
        let recorded = decode(&self.project_id, &recovery_outbox::original_of(&actual))
            .map_err(HubError::Invalid)?;
        if recorded.p3_receipt != entry.receipt {
            return Err(HubError::Invalid(
                "confirmed baseline receipt differs from original request".into(),
            ));
        }
        Ok(Some(RecordedBaselineReceipt {
            receipt: recorded.p3_receipt,
            original: actual,
        }))
    }
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
    decode(project_id, &new)?;
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
        assert_eq!(entry.receipt.result_ref.verdict, Verdict::Approved);
        // matching deliberately supplies no successful receipt API.
        assert_eq!(entry.original.eid, request.event_eid);
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
            ledger.matching(&request).unwrap().unwrap().original,
            original
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
