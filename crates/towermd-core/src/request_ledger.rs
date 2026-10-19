//! Rebuildable project request bookkeeping for App and publication actions.
//!
//! No constructor accepts public event vectors. The actual-history integration
//! owns construction; pending producer intents never certify an accepted action.
use crate::{
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError, HubProtocol, NewEvent},
    human_intervention::{HumanCommand, ReviewBinding, ReviewGate, ReviewSubject},
    recovery_contract::{
        self,
        baseline::{ApprovalFact, Verdict},
        recorded::{
            request_event_eid, BaselineCreatedAction, BaselineReviewAction, BaselineReviewRecorded,
            PublicationPrepareAction, RequestBody, RequestMeta, RequestReceipt, ReviewBodyView,
            ReviewHoldView, ReviewOpenView, TaskReviewAction, TaskReviewRecorded, TaskReviewResult,
        },
    },
    recovery_outbox::{self, RecoveryOutbox},
};
use std::collections::{BTreeMap, BTreeSet};
mod ask_answer;
mod intervention_control;

/// A retriable original request whose acceptance/materialization cannot now be
/// ruled out. Adapters must retain its identity rather than resolve rejection.
pub(crate) fn uncertain_request(request_id: &str, reason: impl std::fmt::Display) -> String {
    format!("baseline request {request_id} uncertain: {reason}")
}

/// A receipt identifies the immutable request result. It grants no current
/// approval; consumers must use HumanState::require_human_review separately.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRequestResult {
    pub hold: String,
    pub binding: ReviewBinding,
    pub opening_eid: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRequestedRecorded {
    pub hold: ReviewHoldView,
    pub request_body: RequestBody<HumanCommand>,
    pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<ReviewRequestResult>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewValidity {
    Current,
    HistoricalOnly { reason: String },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReviewRequestOutcome {
    pub receipt: RequestReceipt<ReviewRequestResult>,
    pub current_validity: ReviewValidity,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReviewOutcome {
    pub receipt: RequestReceipt<TaskReviewResult>,
    pub current_validity: ReviewValidity,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReviewView {
    pub project_id: String,
    pub opening: Event,
    pub current_hold: serde_json::Value,
    pub binding: ReviewBinding,
    pub current_validity: ReviewValidity,
}

/// A started slice needs current effective state. A proved accepted terminal
/// needs only original durability repair; it never authorizes a new transition.
pub(crate) enum AskTickIntent {
    Current(AskRequest),
    ConfirmAccepted(AskRequest),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AskRequest {
    pub(crate) body: RequestBody<crate::ask_contract::AskAction>,
    pub(crate) meta: RequestMeta,
}
impl AskRequest {
    pub(crate) fn prepare(
        project: &str,
        host: &str,
        id: &str,
        action: crate::ask_contract::AskAction,
        run: Option<&str>,
    ) -> Result<Self, String> {
        let actor = if matches!(&action, crate::ask_contract::AskAction::Attention { .. }) {
            "process:towermd-daemon"
        } else {
            "process:towermd-cli"
        };
        let body = RequestBody {
            project_id: project.into(),
            host: host.into(),
            actor: actor.into(),
            run: run.map(str::to_owned),
            action,
        };
        let meta = RequestMeta {
            request_id: id.into(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Self::from_recorded(body, meta)
    }
    fn from_recorded(
        body: RequestBody<crate::ask_contract::AskAction>,
        meta: RequestMeta,
    ) -> Result<Self, String> {
        crate::ask_contract::validate_body(&body, &meta)?;
        Ok(Self { body, meta })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InterventionRequest {
    pub(crate) body: RequestBody<HumanCommand>,
    pub(crate) meta: RequestMeta,
}
impl InterventionRequest {
    pub(crate) fn prepare(
        project_id: &str,
        host: &str,
        request_id: &str,
        command: &HumanCommand,
        run: Option<&str>,
    ) -> Result<Self, String> {
        let body = RequestBody {
            project_id: project_id.into(),
            actor: "process:towermd-cli".into(),
            host: host.into(),
            run: run.map(str::to_owned),
            action: command.clone(),
        };
        let meta = RequestMeta {
            request_id: request_id.into(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Self::from_recorded(body, meta)
    }
    fn from_recorded(body: RequestBody<HumanCommand>, meta: RequestMeta) -> Result<Self, String> {
        request_event_eid(&meta.request_id, "hold.opened")?;
        crate::hub::validate_token("host", &body.host).map_err(|e| e.to_string())?;
        if let Some(run) = &body.run {
            crate::hub::validate_token("run", run).map_err(|e| e.to_string())?;
        }
        if body.project_id.is_empty()
            || body.actor != "process:towermd-cli"
            || recovery_contract::canonical_hash(&body)? != meta.body_hash
        {
            return Err("intervention request identity/hash mismatch".into());
        }
        match &body.action {
            HumanCommand::ReviewRequest {
                gate: ReviewGate::G1,
                content: Some(_),
                ..
            }
            | HumanCommand::ReviewRequest {
                gate: ReviewGate::G4,
                content: None,
                ..
            } => {}
            _ => {
                return Err("only G1 with UTF-8 content or G4 without content is supported".into())
            }
        }
        Ok(Self { body, meta })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskReviewRequest {
    pub(crate) body: RequestBody<TaskReviewAction>,
    pub(crate) meta: RequestMeta,
}
impl TaskReviewRequest {
    /// Validate a typed request identity. This does not authenticate an App or
    /// prove acceptance/current approval.
    pub fn prepare(
        project_id: &str,
        host: &str,
        actor: &str,
        request_id: &str,
        action: &TaskReviewAction,
    ) -> Result<Self, String> {
        let body = RequestBody {
            project_id: project_id.into(),
            host: host.into(),
            actor: actor.into(),
            run: None,
            action: action.clone(),
        };
        let meta = RequestMeta {
            request_id: request_id.into(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Self::from_recorded(body, meta)
    }
    pub fn body(&self) -> &RequestBody<TaskReviewAction> {
        &self.body
    }
    pub fn metadata(&self) -> &RequestMeta {
        &self.meta
    }
    fn from_recorded(
        body: RequestBody<TaskReviewAction>,
        meta: RequestMeta,
    ) -> Result<Self, String> {
        request_event_eid(&meta.request_id, "hold.reviewed")?;
        let human = body
            .actor
            .strip_prefix("human:")
            .filter(|id| !id.is_empty() && !id.contains([':', '/']))
            .ok_or("task review requires an App human actor")?;
        crate::hub::validate_token("human", human).map_err(|e| e.to_string())?;
        crate::hub::validate_token("host", &body.host).map_err(|e| e.to_string())?;
        let TaskReviewAction::Review { hold, binding, .. } = &body.action;
        crate::hub::validate_token("Hold ID", hold).map_err(|e| e.to_string())?;
        validate_task_review_binding(binding)?;
        if body.project_id.is_empty()
            || body.run.is_some()
            || recovery_contract::canonical_hash(&body)? != meta.body_hash
        {
            return Err("task review request identity/hash/run mismatch".into());
        }
        Ok(Self { body, meta })
    }
}

pub(crate) fn validate_task_review_binding(binding: &ReviewBinding) -> Result<(), String> {
    crate::baseline_current::baseline_number(&binding.baseline)?;
    crate::hub::validate_token("run", &binding.run).map_err(|e| e.to_string())?;
    crate::hub::validate_token("task", &binding.task).map_err(|e| e.to_string())?;
    if binding.fences.is_empty() {
        return Err("task review requires complete nonempty fences".into());
    }
    crate::hub::validate_names(binding.fences.iter().map(|f| f.name.as_str()))
        .map_err(|e| e.to_string())?;
    for fence in &binding.fences {
        crate::hub::validate_fence(fence).map_err(|e| e.to_string())?;
        if fence.holder != binding.run {
            return Err("task review fence belongs to another run".into());
        }
    }
    match &binding.subject {
        ReviewSubject::Plan { sha256 } => recovery_contract::sha256_hex(sha256),
        ReviewSubject::Tree { tree } => recovery_contract::object_id(tree),
    }
}

pub(crate) fn is_human_request(kind: &str, data: &serde_json::Value) -> bool {
    if crate::intervention_control::is_control(kind, data) {
        return true;
    }
    crate::ask_answer_contract::is_app_answer(kind, data)
        || crate::ask_contract::is_ask_request(kind, data)
        || kind == "hold.reviewed"
        || (matches!(kind, "hold.opened" | "hold.requested")
            && ["request_body", "p3_request", "p3_receipt"]
                .iter()
                .any(|key| data.get(key).is_some()))
}

/// Receipt references are checked against actual outer opening bytes, including
/// older metadata-free typed openings. Callers supply either the proved actual
/// outer schedule or the separate verified own-receipt roles; neither path
/// projects a receipt-only record into Human authorization.
pub(crate) fn validate_human_original(
    event: &NewEvent,
    sequence: Option<u64>,
    history: &[&Event],
) -> Result<(), String> {
    if crate::intervention_control::is_control(&event.kind, &event.data) {
        let project = event.data["request_body"]["project_id"]
            .as_str()
            .ok_or("control lacks project")?;
        let record = crate::intervention_control::decode(project, event)?;
        return crate::intervention_control::validate_sources(&record, sequence, history);
    }
    if crate::ask_answer_contract::is_app_answer(&event.kind, &event.data) {
        let project = event.data["request_body"]["project_id"]
            .as_str()
            .ok_or("App Answer lacks project")?;
        let record = crate::ask_answer_contract::decode(project, event)?;
        if !history.iter().any(|opening| {
            opening.kind == "hold.opened"
                && recovery_outbox::original_of(opening)
                    == recovery_outbox::original_of(&record.opening_event)
                && sequence.is_none_or(|seq| opening.seq < seq)
        }) {
            return Err("App Answer lacks its matching actual original Ask opening".into());
        }
        return Ok(());
    }
    if crate::ask_contract::is_ask_request(&event.kind, &event.data) {
        let project = event.data["request_body"]["project_id"]
            .as_str()
            .ok_or("Ask lacks project")?;
        let record = crate::ask_contract::decode(project, event)?;
        if event.kind == "hold.opened" {
            return Ok(());
        }
        let opening = history
            .iter()
            .find(|opening| {
                opening.kind == "hold.opened"
                    && opening.obj == event.obj
                    && opening.data["hold"]["body"]["kind"] == "ask"
                    && sequence.is_none_or(|seq| opening.seq < seq)
            })
            .ok_or("Ask receipt lacks its actual original opening")?;
        let opening_eid = match &record {
            crate::ask_contract::AskData::Ordinary(r) => &r.opening_eid,
            crate::ask_contract::AskData::Added(r) => &r.opening_eid,
        };
        if opening_eid != &opening.eid {
            return Err("Ask receipt refers to another original opening".into());
        }
        if event.kind == "hold.wait_finished" {
            let started = history
                .iter()
                .find(|started| {
                    started.kind == "hold.wait_started"
                        && started.obj == event.obj
                        && started.data["p3_request"] == event.data["p3_request"]
                        && sequence.is_none_or(|seq| started.seq < seq)
                })
                .ok_or("Ask finish lacks original actual started receipt")?;
            if started.data["request_body"] != event.data["request_body"]
                || started.data["recipient"] != event.data["recipient"]
            {
                return Err("Ask finish changed original wait identity".into());
            }
        }
        return Ok(());
    }
    if event.kind == "hold.opened" {
        return Ok(());
    }
    let original = history
        .iter()
        .filter(|opening| {
            opening.kind == "hold.opened"
                && opening.layer == "L5"
                && opening.obj == event.obj
                && sequence.is_none_or(|seq| opening.seq < seq)
        })
        .find(|opening| opening.data.get("hold").is_some())
        .ok_or("typed human request lacks its actual outer Review opening")?;
    let raw: ReviewHoldView =
        serde_json::from_value(original.data["hold"].clone()).map_err(|e| e.to_string())?;
    let ReviewBodyView::Review {
        binding,
        plan_content,
        ..
    } = &raw.body;
    let gate = match binding.subject {
        ReviewSubject::Plan { .. } => ReviewGate::G1,
        ReviewSubject::Tree { .. } => ReviewGate::G4,
    };
    recovery_contract::validate_review_open(&ReviewOpenView {
        hold: raw.clone(),
        gate,
        binding: binding.clone(),
        plan_content: plan_content.clone(),
    })?;
    if original.actor != "process:towermd-cli"
        || original.obj != raw.id
        || original.guards != binding.fences
    {
        return Err("actual outer Review opening envelope mismatch".into());
    }
    let mut expected = serde_json::to_value(&raw).map_err(|e| e.to_string())?;
    if event.kind == "hold.requested" {
        expected = original.data["hold"].clone();
        let record: ReviewRequestedRecorded =
            serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
        if record.p3_receipt.result_ref.opening_eid != original.eid {
            return Err("Review receipt names a different original opening".into());
        }
    } else if event.kind == "hold.reviewed" {
        let record: TaskReviewRecorded =
            serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
        expected["body"]["verdict"] = serde_json::json!(record.fact.verdict);
        expected["body"]["comment"] = serde_json::json!(record.comment);
        expected["reply"] = serde_json::json!(record.hold.reply);
    } else {
        return Err("unsupported human receipt kind".into());
    }
    if event.data["hold"] != expected {
        return Err("human receipt altered immutable raw opening fields".into());
    }
    Ok(())
}

pub(crate) fn decode_review_request(
    project_id: &str,
    event: &NewEvent,
) -> Result<ReviewRequestedRecorded, String> {
    if !matches!(event.kind.as_str(), "hold.opened" | "hold.requested") {
        return Err("not a typed Review request record".into());
    }
    let record: ReviewRequestedRecorded =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let request =
        InterventionRequest::from_recorded(record.request_body.clone(), record.p3_request.clone())?;
    let ReviewBodyView::Review {
        binding,
        plan_content,
        source_commit,
        source_path,
        ..
    } = &record.hold.body;
    validate_task_review_binding(binding)?;
    let HumanCommand::ReviewRequest {
        gate,
        content,
        source_commit: requested_commit,
        source_path: requested_path,
    } = &request.body.action
    else {
        return Err("not a ReviewRequest".into());
    };
    recovery_contract::validate_review_open(&ReviewOpenView {
        hold: record.hold.clone(),
        gate: gate.clone(),
        binding: binding.clone(),
        plan_content: plan_content.clone(),
    })?;
    if request.body.project_id != project_id
        || request
            .body
            .run
            .as_ref()
            .is_some_and(|run| run != &binding.run)
        || content != plan_content
        || event.actor != request.body.actor
        || event.host != request.body.host
        || event.layer != "L5"
        || event.obj != record.hold.id
        || event.guards != binding.fences
        || event.eid != request_event_eid(&request.meta.request_id, &event.kind)?
        || record.p3_receipt.request_id != request.meta.request_id
        || record.p3_receipt.body_hash != request.meta.body_hash
        || record.p3_receipt.result_ref.hold != record.hold.id
        || record.p3_receipt.result_ref.binding != *binding
    {
        return Err("Review request/receipt differs from original envelope".into());
    }
    crate::hub::validate_token("opening eid", &record.p3_receipt.result_ref.opening_eid)
        .map_err(|e| e.to_string())?;
    if event.kind == "hold.opened"
        && (record.hold.opened_seq != 0
            || record.p3_receipt.result_ref.opening_eid != event.eid
            || source_commit != requested_commit
            || source_path != requested_path)
    {
        return Err("new Review opening differs from its original request".into());
    }
    Ok(record)
}

pub(crate) fn decode_task_review(
    project_id: &str,
    event: &NewEvent,
) -> Result<TaskReviewRecorded, String> {
    if event.kind != "hold.reviewed" {
        return Err("not a typed task Review answer".into());
    }
    recovery_contract::validate_recorded_data(project_id, "hold.reviewed", &event.data)?;
    let record: TaskReviewRecorded =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    TaskReviewRequest::from_recorded(record.request_body.clone(), record.p3_request.clone())?;
    validate_task_review_binding(&record.fact.binding)?;
    if event.actor != record.request_body.actor
        || event.host != record.request_body.host
        || event.obj != record.hold.id
        || event.layer != "L5"
        || event.eid != record.fact.answer_eid
        || event.guards != record.fact.binding.fences
    {
        return Err("task Review answer differs from original envelope".into());
    }
    Ok(record)
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
            DraftSaveAction::Propose { input } => {
                crate::proposals::validate_proposal(input)?;
                if body.actor != "process:towermd-cli" {
                    return Err("Propose requires the CLI process identity".into());
                }
                request_event_eid(&meta.request_id, "proposal.created")?;
            }
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
            DraftSaveAction::CreateDraft { .. } | DraftSaveAction::Propose { .. } => {
                &self.meta.request_id
            }
            DraftSaveAction::SubmitBaseline { submit } => &submit.scan_id,
            DraftSaveAction::EditDraft { id, .. } => id,
        }
    }
    pub fn expected_revision(&self) -> Option<&str> {
        match &self.body.action {
            DraftSaveAction::SubmitBaseline { .. }
            | DraftSaveAction::CreateDraft { .. }
            | DraftSaveAction::Propose { .. } => None,
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
    Intervention(InterventionRequest),
    TaskReview(TaskReviewRequest),
    Ask(AskRequest),
    AskAnswer(crate::ask_answer_contract::AppAnswerRequest),
    Delivery(crate::cli_delivery::PreparedDeliveryRequest),
    Control(crate::intervention_control::ControlRequest),
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
            Self::Intervention(r) => &r.meta,
            Self::TaskReview(r) => &r.meta,
            Self::Ask(r) => &r.meta,
            Self::AskAnswer(r) => &r.meta,
            Self::Delivery(r) => &r.meta,
            Self::Control(r) => &r.meta,
            Self::Save(r) => &r.meta,
            Self::Preparation { meta, .. } | Self::Created { meta, .. } => meta,
        }
    }
    fn project(&self) -> &str {
        match self {
            Self::Review(r) => &r.body.project_id,
            Self::Intervention(r) => &r.body.project_id,
            Self::TaskReview(r) => &r.body.project_id,
            Self::Ask(r) => &r.body.project_id,
            Self::AskAnswer(r) => &r.body.project_id,
            Self::Delivery(r) => &r.body.project_id,
            Self::Control(r) => &r.body.project_id,
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
    // Only finite Ask opened/started progression; final original remains immutable.
    phases: BTreeMap<String, (NewEvent, Option<Event>)>,
}

/// One project/request_id index across App mutations and typed publication
/// actions. In-flight/materialized entries reserve identity, never a receipt.
pub(crate) struct BaselineRequestLedger {
    project_id: String,
    entries: BTreeMap<String, Entry>,
    unproven_human_replays: BTreeSet<String>,
    historical_reviews: BTreeMap<String, TaskReviewView>,
    historical_asks: BTreeMap<String, crate::ask_answer_contract::AskAnswerView>,
    // Current acceptance never comes from Entry.accepted's historical role.
    actual_proposals: BTreeMap<String, Event>,
    unproven_proposal_replays: BTreeSet<String>,
    delivery_sources: BTreeMap<String, Event>,
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
        let (proposal_originals, proposal_outer) =
            history.proposal_request_history(catalog, inputs)?;
        for original in proposal_originals {
            ledger.accepted(original)?;
        }
        for actual in proposal_outer {
            if actual.kind == "hub.rebuilt" && !actual.stale {
                ledger.actual_proposals.clear();
            }
            if actual.kind != "proposal.created" {
                continue;
            }
            let original = recovery_outbox::original_of(actual);
            let record = crate::proposals::decode_created(&ledger.project_id, &original)?;
            if actual.stale {
                ledger.pending(&original)?;
                let identity = decode_identity(&ledger.project_id, &original)?;
                if !ledger.matching_identity(&identity)?.is_some_and(|entry| {
                    entry.original.as_ref() == Some(&original)
                        && entry.accepted.as_ref().is_some_and(|old| !old.stale)
                }) {
                    ledger
                        .unproven_proposal_replays
                        .insert(record.p3_request.request_id.clone());
                }
            } else {
                ledger.accepted(actual)?;
                if ledger
                    .actual_proposals
                    .insert(actual.obj.clone(), actual.clone())
                    .is_some_and(|previous| previous != *actual)
                {
                    return Err("conflicting actual Proposal acceptance".into());
                }
            }
        }
        let human = history.human_request_history(catalog, inputs)?;
        let mut receipt_history = human.accepted_originals.clone();
        receipt_history.extend(human.actual_outer.iter().copied());
        let mut control_history = receipt_history.clone();
        control_history.extend(history.control_claim_originals(catalog)?);
        // Only own-request bookkeeping consumes original accepted roles. A new
        // stale replay cannot replace the old Event's seq/time/stale fields.
        // Ordering is proved by Human's actual outer replay, not by this ledger.
        for event in &human.accepted_originals {
            if is_human_request(&event.kind, &event.data) {
                validate_human_original(
                    &recovery_outbox::original_of(event),
                    None,
                    if crate::intervention_control::is_control(&event.kind, &event.data) {
                        &control_history
                    } else {
                        &receipt_history
                    },
                )?;
                ledger.accepted(event)?;
            }
        }
        let human_history = human.actual_outer;
        for event in &human_history {
            if is_human_request(&event.kind, &event.data) {
                let original = recovery_outbox::original_of(event);
                validate_human_original(
                    &original,
                    if event.stale { None } else { Some(event.seq) },
                    if crate::intervention_control::is_control(&event.kind, &event.data) {
                        &control_history
                    } else {
                        &human_history
                    },
                )?;
                if event.stale {
                    ledger.pending(&original)?;
                    let identity = decode_identity(&ledger.project_id, &original)?;
                    let accepted_role =
                        ledger.matching_identity(&identity)?.is_some_and(|entry| {
                            (entry.original.as_ref() == Some(&original) && entry.accepted.is_some())
                                || entry.phases.get(&original.kind).is_some_and(
                                    |(phase, accepted)| phase == &original && accepted.is_some(),
                                )
                        });
                    if !accepted_role {
                        // Track the exact phase/eid, not the whole request. A
                        // missing phase receipt cannot invalidate another known
                        // original terminal, or turn a known phase into unknown.
                        let key = if matches!(&identity, RequestIdentity::Ask(_)) {
                            original.eid.clone()
                        } else {
                            identity.meta().request_id.clone()
                        };
                        ledger.unproven_human_replays.insert(key);
                    }
                } else {
                    ledger.accepted(event)?;
                }
            }
        }
        // A typed actual opening reserves its one future system identity. This
        // is collision bookkeeping, never authority to execute attention.
        for opening in human_history.iter().filter(|event| {
            !event.stale
                && event.kind == "hold.opened"
                && crate::ask_contract::is_ask_request(&event.kind, &event.data)
        }) {
            let record = crate::ask_contract::decode(
                &ledger.project_id,
                &recovery_outbox::original_of(opening),
            )?;
            let id = opening.data["hold"]["attention_request_id"]
                .as_str()
                .ok_or("Ask opening lacks attention identity")?;
            ledger.reserve(RequestIdentity::Ask(AskRequest::prepare(
                &ledger.project_id,
                &opening.host,
                id,
                crate::ask_contract::AskAction::Attention {
                    hold: record.hold().id.clone(),
                },
                None,
            )?))?;
        }
        ledger.restore_historical_reviews(&human_history)?;
        ledger.restore_historical_asks(&human_history, &receipt_history)?;
        ledger.delivery_sources = history.delivery_sources(catalog, inputs)?;
        let source_refs: Vec<_> = ledger.delivery_sources.values().collect();
        let mut deliveries = Vec::new();
        for event in &human_history {
            if event.kind == "prompt.delivered" {
                let record = crate::cli_delivery::decode(
                    &ledger.project_id,
                    &recovery_outbox::original_of(event),
                )?;
                crate::cli_delivery::validate_sources(&record, Some(event.seq), &source_refs)?;
                deliveries.push((*event).clone());
            }
            if let Some(original) = restored_original(&ledger.project_id, event)? {
                if original.kind == "prompt.delivered" {
                    let record = crate::cli_delivery::decode(
                        &ledger.project_id,
                        &recovery_outbox::original_of(&original),
                    )?;
                    crate::cli_delivery::validate_sources(
                        &record,
                        Some(original.seq),
                        &source_refs,
                    )?;
                    deliveries.push(original);
                }
            }
        }
        // Original receipt roles are private durability bookkeeping. They do
        // not become a new actual delivery or update Human until the finite
        // actual receipt audit is present in the outer schedule.
        for original in history.delivery_originals(catalog, inputs)? {
            deliveries.push(original.clone());
        }
        // Confirmed bytes alone never mint a delivery. They must match the
        // original acceptance proved by the actual outer or finite wrapper.
        for event in deliveries {
            ledger.accepted(&event)?;
        }
        for event in inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "prompt.delivered")
        {
            let request =
                decode_identity(&ledger.project_id, &recovery_outbox::original_of(event))?;
            let RequestIdentity::Delivery(request) = request else {
                unreachable!("finite delivery identity");
            };
            let key = crate::cli_delivery::decode(
                &ledger.project_id,
                &recovery_outbox::original_of(event),
            )?
            .group_key;
            let actual = ledger
                .delivery_phase(&request, &key)?
                .and_then(|(_, actual)| actual);
            if actual.as_ref() != Some(event) {
                return Err("delivery confirmed lacks matching actual original role".into());
            }
        }

        for original in inputs.pending().iter().filter(|e| {
            (is_indexed_request(&e.kind)
                && !matches!(
                    e.kind.as_str(),
                    "hold.opened"
                        | "hold.requested"
                        | "hold.reviewed"
                        | "hold.recipient_added"
                        | "hold.wait_started"
                        | "hold.wait_finished"
                        | "hold.answered"
                        | "hold.attention"
                ))
                || is_human_request(&e.kind, &e.data)
        }) {
            if is_human_request(&original.kind, &original.data) {
                validate_human_original(
                    original,
                    None,
                    if crate::intervention_control::is_control(&original.kind, &original.data) {
                        &control_history
                    } else {
                        &human_history
                    },
                )?;
            }
            if original.kind == "prompt.delivered" {
                let record = crate::cli_delivery::decode(&ledger.project_id, original)?;
                let sources: Vec<_> = ledger.delivery_sources.values().collect();
                crate::cli_delivery::validate_sources(&record, None, &sources)?;
            }
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
            unproven_human_replays: BTreeSet::new(),
            historical_reviews: BTreeMap::new(),
            historical_asks: BTreeMap::new(),
            actual_proposals: BTreeMap::new(),
            unproven_proposal_replays: BTreeSet::new(),
            delivery_sources: BTreeMap::new(),
        }
    }
    pub(crate) fn accepted_proposal(
        &self,
        id: &str,
        revision: &str,
    ) -> Result<Option<Event>, String> {
        let Some(actual) = self.actual_proposals.get(id) else {
            return Ok(None);
        };
        let record = crate::proposals::decode_created(
            &self.project_id,
            &recovery_outbox::original_of(actual),
        )?;
        Ok((!actual.stale && record.result.draft.revision == revision).then(|| actual.clone()))
    }
    pub(crate) fn historical_task_review(&self, hold: &str) -> Option<TaskReviewView> {
        self.historical_reviews.get(hold).cloned()
    }
    fn restore_historical_reviews(&mut self, history: &[&Event]) -> Result<(), String> {
        for opening in history.iter().filter(|event| {
            event.kind == "hold.opened"
                && event.layer == "L5"
                && event.data["hold"]["body"]["kind"] == "review"
        }) {
            let raw: ReviewHoldView = serde_json::from_value(opening.data["hold"].clone())
                .map_err(|error| error.to_string())?;
            let ReviewBodyView::Review {
                binding,
                plan_content,
                ..
            } = &raw.body;
            let gate = match binding.subject {
                ReviewSubject::Plan { .. } => ReviewGate::G1,
                ReviewSubject::Tree { .. } => ReviewGate::G4,
            };
            recovery_contract::validate_review_open(&ReviewOpenView {
                hold: raw.clone(),
                gate,
                binding: binding.clone(),
                plan_content: plan_content.clone(),
            })?;
            if opening.obj != raw.id
                || opening.actor != "process:towermd-cli"
                || opening.guards != binding.fences
                || opening.epoch != binding.fences.first().map(|fence| fence.epoch)
            {
                return Err("historical Review opening envelope mismatch".into());
            }
            let mut current_hold = opening.data["hold"].clone();
            let mut answer: Option<&Event> = None;
            for event in history
                .iter()
                .filter(|event| event.kind == "hold.reviewed" && event.obj == raw.id)
            {
                let original = recovery_outbox::original_of(event);
                decode_task_review(&self.project_id, &original)?;
                validate_human_original(&original, None, history)?;
                if let Some(previous) = answer {
                    if recovery_outbox::original_of(previous) != original {
                        return Err("historical Review has conflicting original answers".into());
                    }
                }
                answer = Some(event);
                current_hold = event.data["hold"].clone();
            }
            let view = TaskReviewView {
                project_id: self.project_id.clone(),
                opening: (**opening).clone(),
                current_hold,
                binding: binding.clone(),
                current_validity: ReviewValidity::HistoricalOnly {
                    reason: "Review archive history does not establish a current Run or approval"
                        .into(),
                },
            };
            if let Some(previous) = self.historical_reviews.get(&raw.id) {
                if recovery_outbox::original_of(&previous.opening)
                    != recovery_outbox::original_of(opening)
                    || previous.current_hold != view.current_hold
                {
                    return Err("historical Review opening identity was reused".into());
                }
            } else {
                self.historical_reviews.insert(raw.id, view);
            }
        }
        Ok(())
    }
    pub(crate) fn reserve(&mut self, request: RequestIdentity) -> Result<(), String> {
        if self.matching_identity(&request)?.is_none() {
            self.entries.insert(
                request.meta().request_id.clone(),
                Entry {
                    request,
                    original: None,
                    accepted: None,
                    phases: BTreeMap::new(),
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
        self.matching_identity(&request)?;
        if let RequestIdentity::Delivery(delivery) = &request {
            let key = crate::cli_delivery::decode(&self.project_id, original)?.group_key;
            let entry = self
                .entries
                .entry(delivery.meta.request_id.clone())
                .or_insert_with(|| Entry {
                    request: request.clone(),
                    original: None,
                    accepted: None,
                    phases: BTreeMap::new(),
                });
            if let Some((previous, actual)) = entry.phases.get_mut(&key) {
                if previous != original
                    || actual.as_ref().zip(accepted).is_some_and(|(a, b)| a != b)
                {
                    return Err("delivery group has conflicting original/actual receipt".into());
                }
                if actual.is_none() {
                    *actual = accepted.cloned();
                }
            } else {
                entry
                    .phases
                    .insert(key, (original.clone(), accepted.cloned()));
            }
            return Ok(());
        }

        let phase = if crate::intervention_control::is_control(&original.kind, &original.data) {
            original.kind == "human.prompt"
        } else if crate::ask_contract::is_ask_request(&original.kind, &original.data) {
            crate::ask_contract::decode(&self.project_id, original)?
                .receipt()
                .is_none()
        } else {
            false
        };
        let entry = self
            .entries
            .entry(request.meta().request_id.clone())
            .or_insert_with(|| Entry {
                request,
                original: None,
                accepted: None,
                phases: BTreeMap::new(),
            });
        if let RequestIdentity::Ask(request) = &entry.request {
            let terminal = match &request.body.action {
                crate::ask_contract::AskAction::AskOpen { .. } => "hold.answered",
                crate::ask_contract::AskAction::AskWait { .. } => "hold.wait_finished",
                _ => "",
            };
            if phase
                && entry
                    .original
                    .as_ref()
                    .is_some_and(|event| event.kind != terminal)
                || !phase && !entry.phases.is_empty() && original.kind != terminal
            {
                return Err("Ask terminal kind differs from its frozen finite phase".into());
            }
        }
        if phase {
            if let Some((before, actual)) = entry.phases.get_mut(&original.kind) {
                if before != original || actual.as_ref().zip(accepted).is_some_and(|(a, b)| a != b)
                {
                    return Err("Ask phase has conflicting original/actual receipt".into());
                }
                if actual.is_none() {
                    *actual = accepted.cloned();
                }
            } else {
                entry
                    .phases
                    .insert(original.kind.clone(), (original.clone(), accepted.cloned()));
            }
            return Ok(());
        }
        if entry
            .original
            .as_ref()
            .is_some_and(|before| before != original)
            || entry
                .accepted
                .as_ref()
                .zip(accepted)
                .is_some_and(|(a, b)| a != b)
        {
            return Err("baseline request has conflicting original or receipt".into());
        }
        if entry.original.is_none() {
            entry.original = Some(original.clone());
        }
        if entry.accepted.is_none() {
            entry.accepted = accepted.cloned();
        }
        Ok(())
    }
    pub(crate) fn require_unused_request_id(&self, id: &str) -> Result<(), String> {
        request_event_eid(id, "hold.attention")?;
        if self.entries.contains_key(id) {
            return Err("attention UUID collides with an existing original request".into());
        }
        Ok(())
    }
    pub(crate) fn ask_tick_intents(
        &self,
        inputs: &recovery_outbox::RecoveryInputs,
    ) -> Vec<AskTickIntent> {
        self.entries
            .values()
            .filter_map(|entry| {
                let RequestIdentity::Ask(request) = &entry.request else {
                    return None;
                };
                let terminal = match &request.body.action {
                    crate::ask_contract::AskAction::AskWait { .. } => "hold.wait_finished",
                    crate::ask_contract::AskAction::Attention { .. } => "hold.attention",
                    _ => return None,
                };
                if entry
                    .original
                    .as_ref()
                    .is_some_and(|original| original.kind == terminal)
                    && entry
                        .accepted
                        .as_ref()
                        .is_some_and(|actual| !inputs.receipt_confirmed(actual))
                {
                    // retry_original will persist this exact verified Event. Neither
                    // a new settlement nor a current effective Hold is required.
                    return Some(AskTickIntent::ConfirmAccepted(request.clone()));
                }
                match &request.body.action {
                    crate::ask_contract::AskAction::AskWait { .. }
                        if entry.original.is_none()
                            && entry.phases.contains_key("hold.wait_started") =>
                    {
                        Some(AskTickIntent::Current(request.clone()))
                    }
                    crate::ask_contract::AskAction::Attention { .. }
                        if entry.original.is_some() && entry.accepted.is_none() =>
                    {
                        Some(AskTickIntent::Current(request.clone()))
                    }
                    _ => None,
                }
            })
            .collect()
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
        if matches!(request, RequestIdentity::Delivery(_)) {
            return Err(HubError::Invalid(
                "delivery batch requires its finite group retry".into(),
            ));
        }
        let saved = entry
            .original
            .as_ref()
            .map(|original| (original, entry.accepted.as_ref()))
            .or_else(|| {
                entry
                    .phases
                    .values()
                    .next()
                    .map(|(original, actual)| (original, actual.as_ref()))
            });
        let Some((original, accepted)) = saved else {
            return Ok(None);
        };
        let replay_key = if matches!(request, RequestIdentity::Ask(_)) {
            &original.eid
        } else {
            &request.meta().request_id
        };
        if matches!(request, RequestIdentity::Save(request) if matches!(request.body().action, DraftSaveAction::Propose { .. }))
            && self
                .unproven_proposal_replays
                .contains(&request.meta().request_id)
        {
            return Err(HubError::Uncertain(format!(
                "proposal request {} has only a stale outer replay without its original non-stale accepted receipt",
                request.meta().request_id,
            )));
        }
        if self.unproven_human_replays.contains(replay_key) {
            return Err(HubError::Uncertain(format!(
                "human request {} has only a stale outer replay, without the original accepted receipt",
                request.meta().request_id,
            )));
        }
        if matches!(request, RequestIdentity::Control(_)) && accepted.is_none() {
            return Err(HubError::Uncertain(
                "pending control requires exact current scope/barrier validation by its producer"
                    .into(),
            ));
        }
        if let Some(actual) = accepted {
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
            RequestIdentity::Save(r)
                if matches!(r.body.action, DraftSaveAction::Propose { .. }) =>
            {
                return Ok(None)
            }
            RequestIdentity::Save(r) => BaselineRequestStatus::DraftSave {
                request_body: r.body.clone(),
                p3_request: r.meta.clone(),
                receipt: confirmed
                    .map(|e| recorded_save_receipt(&self.project_id, e.clone()).map(|r| r.receipt))
                    .transpose()?,
            },
            RequestIdentity::Preparation { .. }
            | RequestIdentity::Created { .. }
            | RequestIdentity::Intervention(_)
            | RequestIdentity::TaskReview(_)
            | RequestIdentity::Ask(_)
            | RequestIdentity::AskAnswer(_)
            | RequestIdentity::Delivery(_)
            | RequestIdentity::Control(_) => return Ok(None),
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
    is_baseline_review(kind) || matches!(kind, "draft.saved" | "proposal.created")
}

pub(crate) fn is_indexed_request(kind: &str) -> bool {
    is_baseline_request(kind)
        || kind == "prompt.delivered"
        || kind == "human.prompt"
        || crate::ask_contract::ask_kind(kind)
        || matches!(
            kind,
            "baseline.publication_prepared"
                | "baseline.created"
                | "hold.opened"
                | "hold.requested"
                | "hold.reviewed"
        )
}
// Explicitly reviewed recovery support. Adding another indexed action does not
// automatically permit it inside an accepted-receipt audit envelope.
pub(crate) fn is_restorable_request(kind: &str) -> bool {
    matches!(
        kind,
        "baseline.approved"
            | "baseline.rejected"
            | "draft.saved"
            | "proposal.created"
            | "baseline.publication_prepared"
            | "baseline.created"
            | "hold.wait_finished"
            | "hold.attention"
    )
}

// Only this exact new typed Answer joins the finite recovery audit set.
// Existing Relay/Decision hold.answered events keep their original behavior.
pub(crate) fn is_restorable_original(original: &NewEvent) -> bool {
    original.kind == "prompt.delivered"
        || crate::intervention_control::is_control(&original.kind, &original.data)
        || is_restorable_request(&original.kind)
        || crate::ask_answer_contract::is_app_answer(&original.kind, &original.data)
}

// This finite receipt-only recovery role is not a Human transition. In
// particular a raw pending intent cannot establish original acceptance.
pub(crate) fn validate_ask_system_terminal(
    project_id: &str,
    original: &NewEvent,
) -> Result<(), String> {
    if !matches!(
        original.kind.as_str(),
        "hold.wait_finished" | "hold.attention"
    ) || original.actor != "process:towermd-daemon"
        || original.layer != "L5"
        || !original.guards.is_empty()
        || original.data["hold"]["body"]["kind"] != "ask"
    {
        return Err("Ask system recovery receipt requires its exact finite typed envelope".into());
    }
    let record = crate::ask_contract::decode(project_id, original)?;
    if record.receipt().is_none() {
        return Err("Ask system recovery receipt lacks its original terminal receipt".into());
    }
    Ok(())
}

fn decode_identity(project_id: &str, event: &NewEvent) -> Result<RequestIdentity, String> {
    if crate::intervention_control::is_control(&event.kind, &event.data) {
        let record = crate::intervention_control::decode(project_id, event)?;
        return Ok(RequestIdentity::Control(
            crate::intervention_control::ControlRequest::from_recorded(
                record.request_body,
                record.p3_request,
            )?,
        ));
    }
    if event.kind == "prompt.delivered" {
        let record = crate::cli_delivery::decode(project_id, event)?;
        return Ok(RequestIdentity::Delivery(
            crate::cli_delivery::PreparedDeliveryRequest::from_recorded(
                record.request_body,
                record.p3_request,
            )?,
        ));
    }
    if crate::ask_answer_contract::is_app_answer(&event.kind, &event.data) {
        let record = crate::ask_answer_contract::decode(project_id, event)?;
        return Ok(RequestIdentity::AskAnswer(
            crate::ask_answer_contract::AppAnswerRequest::from_recorded(
                record.request_body,
                record.p3_request,
            )?,
        ));
    }
    if crate::ask_contract::is_ask_request(&event.kind, &event.data) {
        let record = crate::ask_contract::decode(project_id, event)?;
        return Ok(RequestIdentity::Ask(AskRequest::from_recorded(
            record.body().clone(),
            record.meta().clone(),
        )?));
    }
    if is_human_request(&event.kind, &event.data) {
        return match event.kind.as_str() {
            "hold.opened" | "hold.requested" => {
                let record = decode_review_request(project_id, event)?;
                Ok(RequestIdentity::Intervention(
                    InterventionRequest::from_recorded(record.request_body, record.p3_request)?,
                ))
            }
            "hold.reviewed" => {
                let record = decode_task_review(project_id, event)?;
                Ok(RequestIdentity::TaskReview(
                    TaskReviewRequest::from_recorded(record.request_body, record.p3_request)?,
                ))
            }
            _ => unreachable!("finite human request classifier"),
        };
    }
    if matches!(
        event.kind.as_str(),
        "baseline.publication_prepared" | "baseline.created"
    ) {
        return crate::project_session::publication_events::decode_request(project_id, event);
    }
    if event.kind == "proposal.created" {
        let record = crate::proposals::decode_created(project_id, event)?;
        return Ok(RequestIdentity::Save(
            PreparedDraftSaveRequest::from_recorded(record.request_body, record.p3_request)?,
        ));
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
    if matches!(request.body().action, DraftSaveAction::Propose { .. }) {
        return Err("Propose must be recorded as proposal.created, never draft.saved".into());
    }
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
    if !is_restorable_original(&new) {
        return Err("request recovery audit does not support this typed action".into());
    }
    if matches!(new.kind.as_str(), "hold.wait_finished" | "hold.attention") {
        validate_ask_system_terminal(project_id, &new)?;
        if original.stale {
            return Err("Ask system recovery receipt lacks original non-stale acceptance".into());
        }
    }
    if new.kind == "proposal.created" && original.stale {
        return Err("Proposal recovery audit lacks its original non-stale acceptance".into());
    }
    if crate::ask_answer_contract::is_app_answer(&new.kind, &new.data) && original.stale {
        return Err("App Answer recovery audit lacks its original non-stale acceptance".into());
    }
    if crate::intervention_control::is_control(&new.kind, &new.data) && original.stale {
        return Err("control recovery audit lacks original non-stale acceptance".into());
    }
    if new.kind == "prompt.delivered" && original.stale {
        return Err("delivery recovery audit lacks original non-stale acceptance".into());
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
    // Expiry has no P3RequestBody or ledger entry. This is layout decoding
    // only; the Catalog/history reader separately proves source and rebuild.
    if crate::finish::expiry_receipt::is_wrapper(event) {
        return Ok(
            crate::finish::expiry_receipt::decode(project_id, event)?.map(|record| record.original)
        );
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

impl BaselineRequestLedger {
    pub(crate) fn delivery_phase(
        &self,
        request: &crate::cli_delivery::PreparedDeliveryRequest,
        key: &str,
    ) -> Result<Option<(NewEvent, Option<Event>)>, String> {
        Ok(self
            .matching_identity(&RequestIdentity::Delivery(request.clone()))?
            .and_then(|entry| entry.phases.get(key))
            .cloned())
    }
    // Called only when the private Session Task-delta replay and its final
    // source/capture CAS have succeeded. These complete accepted outer claim
    // Events are immutable recipient provenance, never new execution grants.
    pub(crate) fn install_verified_claim_sources(
        &mut self,
        events: &[Event],
    ) -> Result<(), String> {
        let claims: Vec<_> = events
            .iter()
            .filter(|event| event.kind == "task.claimed" && !event.stale && !event.legacy)
            .collect();
        // Validate the entire batch before mutating this derived index.
        for claim in &claims {
            recovery_outbox::validate_actual(&recovery_outbox::original_of(claim), claim)?;
            crate::cli_delivery::validate_source_event(&self.project_id, claim)?;
            if self
                .delivery_sources
                .get(&claim.eid)
                .is_some_and(|previous| previous != *claim)
            {
                return Err("incremental claim source original identity conflicts".into());
            }
        }
        for claim in claims {
            self.delivery_sources
                .insert(claim.eid.clone(), (*claim).clone());
        }
        Ok(())
    }
    pub(crate) fn delivery_source(&self, eid: &str) -> Option<&Event> {
        self.delivery_sources.get(eid)
    }
    pub(crate) fn delivery_reply_source(&self, hold: &str, reply: &str) -> Option<&Event> {
        self.delivery_sources.values().find(|event| {
            event.obj == hold
                && matches!(event.kind.as_str(), "hold.answered" | "hold.reviewed")
                && event.data["hold"]["reply"]["id"].as_str() == Some(reply)
        })
    }
    pub(crate) fn delivery_historical_reviews(&self) -> Vec<TaskReviewView> {
        self.historical_reviews.values().cloned().collect()
    }
    pub(crate) fn delivery_archived_runs(&self) -> Result<Vec<crate::tasks::Run>, String> {
        self.delivery_sources
            .values()
            .filter(|event| event.kind == "task.claimed")
            .map(|event| {
                let mut run: crate::tasks::Run = serde_json::from_value(event.data["run"].clone())
                    .map_err(|error| error.to_string())?;
                run.state = "archived".into();
                Ok(run)
            })
            .collect()
    }
    pub(crate) fn delivery_historical_holds(
        &self,
    ) -> Vec<crate::ask_answer_contract::AskAnswerView> {
        self.historical_asks.values().cloned().collect()
    }
    pub(crate) fn delivery_requests(
        &self,
        inputs: &crate::recovery_outbox::RecoveryInputs,
    ) -> Vec<crate::cli_delivery::PreparedDeliveryRequest> {
        self.entries
            .values()
            .filter_map(|entry| {
                if let RequestIdentity::Delivery(request) = &entry.request {
                    let complete = request.body.action.groups.iter().all(|group| {
                        group
                            .key()
                            .ok()
                            .and_then(|key| entry.phases.get(&key))
                            .and_then(|(_, actual)| actual.as_ref())
                            .is_some_and(|actual| !actual.stale && inputs.receipt_confirmed(actual))
                    });
                    (!complete).then(|| request.clone())
                } else {
                    None
                }
            })
            .collect()
    }
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

    // Typed producer-body fixture plus a real Hub/outbox; this is not a CLI/App
    // producer or an authorization fixture (the integration tests cover those).
    fn guarded_review_original(project_id: &str) -> (InterventionRequest, NewEvent) {
        let command = HumanCommand::ReviewRequest {
            gate: ReviewGate::G1,
            content: Some("original plan\r\n".into()),
            source_commit: None,
            source_path: None,
        };
        let request = InterventionRequest::prepare(
            project_id,
            "host",
            &uuid::Uuid::now_v7().to_string(),
            &command,
            None,
        )
        .unwrap();
        let binding = crate::human_intervention::ReviewBinding {
            run: "old-run".into(),
            task: "T-old".into(),
            baseline: "towermd/B0".into(),
            fences: vec![crate::hub_protocol::Fence {
                name: "module:A".into(),
                holder: "old-run".into(),
                epoch: 7,
            }],
            subject: ReviewSubject::Plan {
                sha256: recovery_contract::sha256(b"original plan\r\n"),
            },
        };
        let hold = ReviewHoldView {
            object_type: "Hold".into(),
            id: "H-old".into(),
            body: ReviewBodyView::Review {
                binding: binding.clone(),
                verdict: None,
                plan_content: Some("original plan\r\n".into()),
                source_commit: None,
                source_path: None,
                comment: None,
            },
            opened_ms: 100,
            opened_seq: 0,
            reply: None,
            extra: BTreeMap::new(),
        };
        let eid = request_event_eid(&request.meta.request_id, "hold.opened").unwrap();
        let recorded = ReviewRequestedRecorded {
            hold,
            request_body: request.body.clone(),
            p3_request: request.meta.clone(),
            p3_receipt: RequestReceipt {
                request_id: request.meta.request_id.clone(),
                body_hash: request.meta.body_hash.clone(),
                result_ref: ReviewRequestResult {
                    hold: "H-old".into(),
                    binding: binding.clone(),
                    opening_eid: eid.clone(),
                },
            },
        };
        let original = NewEvent {
            eid,
            host: "host".into(),
            actor: "process:towermd-cli".into(),
            kind: "hold.opened".into(),
            layer: "L5".into(),
            obj: "H-old".into(),
            data: serde_json::to_value(recorded).unwrap(),
            guards: binding.fences,
        };
        (request, original)
    }

    #[test]
    fn original_confirmed_stale_review_is_a_durable_receipt_without_an_outer_authorization_role() {
        let root = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(root.path());
        let storage = tempfile::tempdir().unwrap();
        let mut hub = crate::hub::Hub::open(storage.path()).unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let mut outbox = RecoveryOutbox::open(
            &runtime.path().join("outbox"),
            crate::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        let (request, original) = guarded_review_original(catalog.config.project_id());
        outbox.append_pending(&original).unwrap();
        let AppendReceipt::Event(actual) = hub.append(original.clone(), 100).unwrap() else {
            panic!("actual guarded original receipt")
        };
        assert!(actual.stale, "the old Run has no actual lease");
        outbox.persist_confirmed(&original, &actual).unwrap();
        let inputs = outbox.read_inputs().unwrap();
        let history = crate::recovery_git::read_recovery_history(&catalog, &inputs).unwrap();
        let roles = history.human_request_history(&catalog, &inputs).unwrap();
        assert_eq!(roles.accepted_originals, vec![actual.as_ref()]);
        assert!(
            roles.actual_outer.is_empty(),
            "confirmed-only cannot hydrate HumanState"
        );
        let ledger = BaselineRequestLedger::from_history(&catalog, &inputs, &history).unwrap();
        assert!(
            ledger.historical_task_review("H-old").is_none(),
            "history view requires an actual outer opening"
        );
        let digest = inputs.digest().to_owned();
        let events = hub.events(0);
        let retry = ledger
            .retry_original(
                &catalog,
                &RequestIdentity::Intervention(request),
                &mut hub,
                &mut outbox,
                101,
            )
            .unwrap()
            .unwrap();
        assert_eq!(retry, *actual, "original seq/time/stale must remain exact");
        assert_eq!(hub.events(0), events);
        assert_eq!(outbox.read_inputs().unwrap().digest(), digest);
        assert!(retry.stale);
    }

    #[test]
    fn human_receipt_roles_reject_each_original_field_change_and_conflicting_actual_coordinates() {
        let (request, original) = guarded_review_original("project");
        let storage = tempfile::tempdir().unwrap();
        let mut hub = crate::hub::Hub::open(storage.path()).unwrap();
        let AppendReceipt::Event(actual) = hub.append(original.clone(), 100).unwrap() else {
            panic!("actual original")
        };
        let mut ledger = BaselineRequestLedger::empty("project");
        ledger.accepted(&actual).unwrap();
        for field in [
            "eid", "host", "actor", "type", "layer", "obj", "data", "guards",
        ] {
            let mut changed = serde_json::to_value(&original).unwrap();
            match field {
                "data" => {
                    changed["data"]["hold"]["future"] = json!({"changed":true});
                }
                "guards" => {
                    changed["guards"][0]["epoch"] = json!(8);
                }
                _ => {
                    changed[field] = json!("replacement");
                }
            }
            let changed: NewEvent = serde_json::from_value(changed).unwrap();
            assert!(ledger.pending(&changed).is_err(), "{field}");
        }
        for field in ["seq", "t", "stale"] {
            let mut changed = serde_json::to_value(actual.as_ref()).unwrap();
            match field {
                "seq" => changed[field] = json!(actual.seq + 1),
                "t" => changed[field] = json!("2026-10-18T00:00:00Z"),
                "stale" => changed[field] = json!(!actual.stale),
                _ => unreachable!(),
            }
            let changed: Event = serde_json::from_value(changed).unwrap();
            assert!(ledger.accepted(&changed).is_err(), "{field}");
        }
        assert!(BaselineRequestLedger::empty("other-project")
            .accepted(&actual)
            .is_err());
        assert_eq!(
            ledger
                .matching_identity(&RequestIdentity::Intervention(request))
                .unwrap()
                .unwrap()
                .accepted
                .as_ref(),
            Some(actual.as_ref())
        );
    }
}
