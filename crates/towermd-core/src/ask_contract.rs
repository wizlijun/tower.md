//! Finite Ask persistent views. These DTOs describe an original request, never
//! an accepted receipt or authority to edit a Hold or restore a Run.
use crate::{
    hub_protocol::Fence,
    recovery_contract::recorded::{RequestBody, RequestMeta, RequestReceipt},
    tasks::SelfReported,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Recipient {
    pub run: String,
    pub task: String,
    pub host: String,
    pub worktree: String,
    pub branch: String,
    pub fences: Vec<Fence>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WaitSlice {
    pub id: String,
    pub recipient_run: String,
    pub started_ms: u64,
    pub deadline_ms: u64,
    pub finished_ms: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Reply {
    pub id: String,
    pub text: String,
    pub actor: String,
    #[serde(default)]
    pub relayed_from: Option<String>,
    #[serde(default)]
    pub self_reported: Option<SelfReported>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub decision_revision: Option<String>,
    pub at_ms: u64,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AskState {
    Pending,
    Answered,
    Deferred,
    Expired,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AskResult {
    pub hold: String,
    pub state: AskState,
    pub reply: Option<Reply>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AskAction {
    AskOpen {
        question: String,
        options: Vec<String>,
        recommended: Option<usize>,
        context: String,
    },
    AskWait {
        hold: String,
    },
    Relay {
        hold: String,
        text: String,
        self_reported: Option<SelfReported>,
    },
    // An internal tick intent. No CLI/App adapter accepts this action as input.
    Attention {
        hold: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AskBodyView {
    pub kind: String,
    pub fingerprint: String,
    pub question: String,
    pub module: String,
    pub options: Vec<String>,
    pub recommended: Option<usize>,
    pub context: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AskHoldView {
    #[serde(rename = "type")]
    pub object_type: String,
    pub id: String,
    pub body: AskBodyView,
    #[serde(default)]
    pub recipients: Vec<Recipient>,
    pub opened_ms: u64,
    pub opened_seq: u64,
    pub reply: Option<Reply>,
    #[serde(default)]
    pub waits: Vec<WaitSlice>,
    #[serde(default)]
    pub delivered: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub attention: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
/// recipient_added serializes exactly these five fields (slice is absent).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AskResultRef {
    pub hold: String,
    pub opening_eid: String,
    pub recipient: Option<Recipient>,
    pub state: AskState,
    pub reply: Option<Reply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice: Option<WaitSlice>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AskRecipientAddedRecorded {
    pub hold: AskHoldView,
    pub added: Recipient,
    pub opening_eid: String,
    pub request_body: RequestBody<AskAction>,
    pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<AskResultRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AskRecorded {
    pub hold: AskHoldView,
    pub opening_eid: String,
    pub recipient: Option<Recipient>,
    pub slice: Option<WaitSlice>,
    pub request_body: RequestBody<AskAction>,
    pub p3_request: RequestMeta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p3_receipt: Option<RequestReceipt<AskResultRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_reply: Option<Reply>,
}

pub(crate) enum AskData {
    Ordinary(Box<AskRecorded>),
    Added(Box<AskRecipientAddedRecorded>),
}
impl AskData {
    pub(crate) fn hold(&self) -> &AskHoldView {
        match self {
            Self::Ordinary(r) => &r.hold,
            Self::Added(r) => &r.hold,
        }
    }
    pub(crate) fn body(&self) -> &RequestBody<AskAction> {
        match self {
            Self::Ordinary(r) => &r.request_body,
            Self::Added(r) => &r.request_body,
        }
    }
    pub(crate) fn meta(&self) -> &RequestMeta {
        match self {
            Self::Ordinary(r) => &r.p3_request,
            Self::Added(r) => &r.p3_request,
        }
    }
    pub(crate) fn receipt(&self) -> Option<&RequestReceipt<AskResultRef>> {
        match self {
            Self::Ordinary(r) => r.p3_receipt.as_ref(),
            Self::Added(r) => Some(&r.p3_receipt),
        }
    }
}
pub(crate) fn ask_kind(kind: &str) -> bool {
    matches!(
        kind,
        "hold.opened"
            | "hold.recipient_added"
            | "hold.requested"
            | "hold.wait_started"
            | "hold.wait_finished"
            | "hold.answered"
            | "hold.attention"
    )
}
pub(crate) fn is_ask_request(kind: &str, data: &Value) -> bool {
    ask_kind(kind)
        && !crate::ask_answer_contract::is_app_answer(kind, data)
        && data["hold"]["body"]["kind"] == "ask"
        && ["request_body", "p3_request", "p3_receipt"]
            .iter()
            .any(|key| data.get(key).is_some())
}
pub(crate) fn validate_recipient(recipient: &Recipient) -> Result<(), String> {
    for (key, value) in [
        ("run", &recipient.run),
        ("task", &recipient.task),
        ("host", &recipient.host),
    ] {
        crate::hub::validate_token(key, value).map_err(|e| e.to_string())?;
    }
    if !std::path::Path::new(&recipient.worktree).is_absolute()
        || recipient.branch != format!("t/{}", recipient.task)
        || recipient.fences.is_empty()
    {
        return Err("Ask recipient lacks canonical Run coordinates/fences".into());
    }
    crate::hub::validate_names(recipient.fences.iter().map(|f| f.name.as_str()))
        .map_err(|e| e.to_string())?;
    for fence in &recipient.fences {
        crate::hub::validate_fence(fence).map_err(|e| e.to_string())?;
        if fence.holder != recipient.run {
            return Err("Ask recipient fences belong to another Run".into());
        }
    }
    Ok(())
}
pub(crate) fn validate_body(
    body: &RequestBody<AskAction>,
    meta: &RequestMeta,
) -> Result<(), String> {
    crate::recovery_contract::recorded::request_event_eid(&meta.request_id, "hold.opened")?;
    crate::hub::validate_token("host", &body.host).map_err(|e| e.to_string())?;
    if let Some(run) = &body.run {
        crate::hub::validate_token("run", run).map_err(|e| e.to_string())?;
    }
    let expected = match &body.action {
        AskAction::Attention { .. } => "process:towermd-daemon",
        _ => "process:towermd-cli",
    };
    if body.project_id.is_empty()
        || body.actor != expected
        || crate::recovery_contract::canonical_hash(body)? != meta.body_hash
    {
        return Err("Ask original request identity/hash mismatch".into());
    }
    match &body.action {
        AskAction::AskOpen {
            question,
            options,
            recommended,
            ..
        } => {
            if crate::human_intervention::normalize_question(question).is_empty()
                || recommended.is_some_and(|i| i >= options.len())
            {
                return Err("AskOpen question/recommended is invalid".into());
            }
        }
        AskAction::AskWait { hold }
        | AskAction::Attention { hold }
        | AskAction::Relay { hold, .. } => {
            crate::hub::validate_token("Hold ID", hold).map_err(|e| e.to_string())?
        }
    }
    if matches!(&body.action, AskAction::Attention { .. }) && body.run.is_some() {
        return Err("attention is a system intent without a Run binding".into());
    }
    if let AskAction::Relay {
        text,
        self_reported,
        ..
    } = &body.action
    {
        if text.trim().is_empty() {
            return Err("Ask relay answer must not be empty".into());
        }
        if self_reported
            .as_ref()
            .is_some_and(|r| r.harness.trim().is_empty() || r.version.trim().is_empty())
        {
            return Err("relay self-reported identity is incomplete".into());
        }
    }
    Ok(())
}
pub(crate) fn decode(
    project_id: &str,
    event: &crate::hub_protocol::NewEvent,
) -> Result<AskData, String> {
    if !is_ask_request(&event.kind, &event.data) {
        return Err("not a finite typed Ask event".into());
    }
    let record = if event.kind == "hold.recipient_added" {
        AskData::Added(serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?)
    } else {
        AskData::Ordinary(serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?)
    };
    let body = record.body();
    let meta = record.meta();
    let hold = record.hold();
    validate_body(body, meta)?;
    crate::hub::validate_token("Hold ID", &hold.id).map_err(|e| e.to_string())?;
    if body.project_id != project_id
        || hold.object_type != "Hold"
        || hold.body.kind != "ask"
        || hold.body.fingerprint
            != crate::human_intervention::fingerprint(&hold.body.module, &hold.body.question)
        || hold
            .body
            .recommended
            .is_some_and(|i| i >= hold.body.options.len())
        || event.obj != hold.id
        || event.host != body.host
        || event.layer != "L5"
        || event.eid
            != crate::recovery_contract::recorded::request_event_eid(&meta.request_id, &event.kind)?
    {
        return Err("Ask data/envelope differs from original request".into());
    }
    if event.kind == "hold.opened" {
        let attention = event.data["hold"]["attention_request_id"]
            .as_str()
            .ok_or("new typed Ask lacks fixed attention request UUID")?;
        crate::recovery_contract::recorded::request_event_eid(attention, "hold.attention")?;
        if attention == meta.request_id {
            return Err("AskOpen cannot reuse its UUID for attention".into());
        }
        let AskAction::AskOpen {
            question,
            options,
            recommended,
            context,
        } = &body.action
        else {
            return Err("only AskOpen creates a Hold".into());
        };
        if &hold.body.question != question
            || &hold.body.options != options
            || &hold.body.recommended != recommended
            || &hold.body.context != context
        {
            return Err("Ask opening changed original question/options/context".into());
        }
    }
    if let AskAction::AskOpen { question, .. } = &body.action {
        if hold.body.fingerprint
            != crate::human_intervention::fingerprint(&hold.body.module, question)
        {
            return Err("Ask request names a different normalized question".into());
        }
    }
    for recipient in &hold.recipients {
        validate_recipient(recipient)?;
    }
    let runs = hold
        .recipients
        .iter()
        .map(|r| r.run.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if runs.len() != hold.recipients.len() {
        return Err("Ask Hold repeats a recipient run".into());
    }
    let (recipient, opening) = match &record {
        AskData::Added(r) => {
            if !matches!(&body.action, AskAction::AskOpen { .. }) {
                return Err("only explicit AskOpen may add a recipient".into());
            }
            if r.p3_receipt.result_ref.recipient.as_ref() != Some(&r.added)
                || r.p3_receipt.result_ref.state != AskState::Pending
                || r.p3_receipt.result_ref.reply.is_some()
                || r.p3_receipt.result_ref.slice.is_some()
            {
                return Err(
                    "recipient_added result must be the exact pending five-field result".into(),
                );
            }
            (Some(&r.added), &r.opening_eid)
        }
        AskData::Ordinary(r) => (r.recipient.as_ref(), &r.opening_eid),
    };
    crate::hub::validate_token("opening eid", opening).map_err(|e| e.to_string())?;
    if let Some(recipient) = recipient {
        validate_recipient(recipient)?;
        if recipient.host != body.host || body.run.as_ref().is_some_and(|run| run != &recipient.run)
        {
            return Err("Ask request and frozen recipient differ".into());
        }
    }
    let expected_actor = if event.kind == "hold.wait_finished" {
        "process:towermd-daemon"
    } else if event.kind == "hold.answered" && matches!(&body.action, AskAction::AskOpen { .. }) {
        "process:towermd-dedupe"
    } else {
        body.actor.as_str()
    };
    let expected_guards = if matches!(&body.action, AskAction::Attention { .. })
        || event.kind == "hold.wait_finished"
    {
        vec![]
    } else {
        recipient.map(|r| r.fences.clone()).unwrap_or_default()
    };
    if event.actor != expected_actor || event.guards != expected_guards {
        return Err("Ask producer actor/full guards differ from frozen recipient".into());
    }
    if let Some(receipt) = record.receipt() {
        let result = &receipt.result_ref;
        if receipt.request_id != meta.request_id
            || receipt.body_hash != meta.body_hash
            || result.hold != hold.id
            || result.opening_eid != *opening
            || result.recipient.as_ref() != recipient
        {
            return Err("Ask receipt differs from its original request/result".into());
        }
        if event.kind == "hold.answered"
            && (result.state != AskState::Answered || result.reply != hold.reply)
        {
            return Err("Ask answer receipt differs from accepted Reply".into());
        }
    }
    if let AskData::Ordinary(r) = &record {
        let nonterminal = event.kind == "hold.wait_started"
            || (event.kind == "hold.opened" && r.planned_reply.is_some());
        if r.p3_receipt.is_none() != nonterminal {
            return Err("Ask phase has an invalid terminal receipt".into());
        }
        if r.planned_reply.is_some()
            && (!matches!(&body.action, AskAction::AskOpen { .. })
                || !matches!(event.kind.as_str(), "hold.opened" | "hold.answered"))
        {
            return Err("planned Reply is limited to Decision AskOpen phases".into());
        }
        if let Some(reply) = &r.planned_reply {
            if reply.actor != "process:towermd-dedupe"
                || reply
                    .decision
                    .as_ref()
                    .is_none_or(|id| crate::hub::validate_token("Decision ID", id).is_err())
                || reply
                    .decision_revision
                    .as_ref()
                    .is_none_or(|oid| crate::recovery_contract::object_id(oid).is_err())
                || reply.text.trim().is_empty()
                || reply.relayed_from.is_some()
                || reply.self_reported.is_some()
            {
                return Err(
                    "Ask planned Reply lacks a frozen authoritative Decision identity".into(),
                );
            }
        }
        if let Some(slice) = &r.slice {
            if r.recipient
                .as_ref()
                .is_none_or(|to| to.run != slice.recipient_run)
                || slice.deadline_ms < slice.started_ms
                || slice
                    .finished_ms
                    .is_some_and(|end| end < slice.started_ms || end > slice.deadline_ms)
            {
                return Err("Ask observed slice differs from its frozen recipient/bounds".into());
            }
        }
        if let Some(receipt) = &r.p3_receipt {
            if receipt.result_ref.slice != r.slice {
                return Err("Ask receipt changed the observed slice".into());
            }
        }
    }
    match &record {
        AskData::Added(_) => {}
        AskData::Ordinary(r) => match &body.action {
            AskAction::AskOpen { .. }
                if !matches!(
                    event.kind.as_str(),
                    "hold.opened" | "hold.requested" | "hold.answered"
                ) =>
            {
                return Err("AskOpen has an unsupported phase".into())
            }
            AskAction::AskWait { hold: id }
                if id != &hold.id
                    || !matches!(
                        event.kind.as_str(),
                        "hold.requested" | "hold.wait_started" | "hold.wait_finished"
                    ) =>
            {
                return Err("AskWait has an unsupported phase".into())
            }
            AskAction::Relay { hold: id, .. }
                if id != &hold.id || event.kind != "hold.answered" =>
            {
                return Err("Relay only answers its original Ask Hold".into())
            }
            AskAction::Attention { hold: id }
                if id != &hold.id || event.kind != "hold.attention" || r.p3_receipt.is_none() =>
            {
                return Err("attention has an invalid original intent".into())
            }
            _ => {}
        },
    }
    Ok(record)
}
