//! Finite App Answer wire and original-request validation. These values do not
//! authenticate an App or grant authority from a receipt-only recovery role.
use crate::{
    ask_contract::{self, AskHoldView, Recipient, Reply},
    hub_protocol::{Event, NewEvent},
    recovery_contract::{
        self,
        recorded::{request_event_eid, RequestBody, RequestMeta, RequestReceipt},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskOpeningBinding {
    pub eid: String,
    pub sha256: String,
}
impl AskOpeningBinding {
    pub fn from_opening(opening: &Event) -> Result<Self, String> {
        Ok(Self {
            eid: opening.eid.clone(),
            sha256: recovery_contract::canonical_hash(opening)?,
        })
    }
    pub fn validate(&self, opening: &Event) -> Result<(), String> {
        if self != &Self::from_opening(opening)? {
            return Err("Ask opening binding differs from actual Event".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AppAnswerAction {
    Answer {
        hold: String,
        opening: AskOpeningBinding,
        text: String,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum AskAnswerValidity {
    Current,
    HistoricalOnly { reason: String },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskAnswerRecipient {
    pub recipient: Recipient,
    pub actionable: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskAnswerViewAction {
    Answer,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskAnswerView {
    pub project_id: String,
    pub opening: Event,
    pub current_hold: Value,
    pub binding: AskOpeningBinding,
    pub current_validity: AskAnswerValidity,
    pub recipients: Vec<AskAnswerRecipient>,
    pub actions: Vec<AskAnswerViewAction>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskAnswerResult {
    pub hold: String,
    pub opening_eid: String,
    pub reply: Reply,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskAnswerOutcome {
    pub receipt: RequestReceipt<AskAnswerResult>,
    pub current_validity: AskAnswerValidity,
}
#[derive(Clone, Debug, PartialEq)]
pub struct AppAnswerRequest {
    pub(crate) body: RequestBody<AppAnswerAction>,
    pub(crate) meta: RequestMeta,
}
impl AppAnswerRequest {
    /// Syntax and immutable request identity only. The adapter must enforce AppGate.
    pub fn prepare(
        project: &str,
        host: &str,
        actor: &str,
        id: &str,
        action: &AppAnswerAction,
    ) -> Result<Self, String> {
        let body = RequestBody {
            project_id: project.into(),
            host: host.into(),
            actor: actor.into(),
            run: None,
            action: action.clone(),
        };
        let meta = RequestMeta {
            request_id: id.into(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Self::from_recorded(body, meta)
    }
    pub fn body(&self) -> &RequestBody<AppAnswerAction> {
        &self.body
    }
    pub fn metadata(&self) -> &RequestMeta {
        &self.meta
    }
    pub(crate) fn from_recorded(
        body: RequestBody<AppAnswerAction>,
        meta: RequestMeta,
    ) -> Result<Self, String> {
        request_event_eid(&meta.request_id, "hold.answered")?;
        let human = body
            .actor
            .strip_prefix("human:")
            .filter(|s| !s.is_empty() && !s.contains([':', '/']))
            .ok_or("App Answer requires an App human actor")?;
        crate::hub::validate_token("human", human).map_err(|e| e.to_string())?;
        crate::hub::validate_token("host", &body.host).map_err(|e| e.to_string())?;
        let AppAnswerAction::Answer {
            hold,
            opening,
            text,
        } = &body.action;
        crate::hub::validate_token("Hold ID", hold).map_err(|e| e.to_string())?;
        crate::hub::validate_token("opening eid", &opening.eid).map_err(|e| e.to_string())?;
        recovery_contract::sha256_hex(&opening.sha256)?;
        if body.project_id.is_empty()
            || body.run.is_some()
            || text.trim().is_empty()
            || recovery_contract::canonical_hash(&body)? != meta.body_hash
        {
            return Err("App Answer request identity/hash/text/run mismatch".into());
        }
        Ok(Self { body, meta })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppAnswerRecorded {
    pub hold: Value,
    pub opening: AskOpeningBinding,
    // Original coordinates are preserved through receipt-only recovery. This
    // inner value never substitutes for an actual outer opening authorization.
    pub opening_event: Event,
    pub request_body: RequestBody<AppAnswerAction>,
    pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<AskAnswerResult>,
}
pub(crate) fn is_app_answer(kind: &str, data: &Value) -> bool {
    kind == "hold.answered" && data["request_body"]["action"]["action"] == "answer"
}
pub(crate) fn decode(project: &str, event: &NewEvent) -> Result<AppAnswerRecorded, String> {
    if !is_app_answer(&event.kind, &event.data) {
        return Err("not a finite App Answer".into());
    }
    let record: AppAnswerRecorded =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let request =
        AppAnswerRequest::from_recorded(record.request_body.clone(), record.p3_request.clone())?;
    let AppAnswerAction::Answer {
        hold,
        opening,
        text,
    } = &request.body.action;
    let raw: AskHoldView =
        serde_json::from_value(record.hold.clone()).map_err(|e| e.to_string())?;
    let original = &record.opening_event;
    let opened = ask_contract::decode(project, &crate::recovery_outbox::original_of(original))?;
    crate::recovery_outbox::validate_actual(
        &crate::recovery_outbox::original_of(original),
        original,
    )?;
    let reply = raw
        .reply
        .as_ref()
        .ok_or("App Answer lacks its first Reply")?;
    if request.body.project_id != project
        || raw.object_type != "Hold"
        || raw.body.kind != "ask"
        || raw.id != *hold
        || original.kind != "hold.opened"
        || original.stale
        || original.seq == 0
        || opened.hold().id != *hold
        || raw.opened_seq != original.seq
        || raw.opened_ms != opened.hold().opened_ms
        || record.opening != *opening
        || AskOpeningBinding::from_opening(original)? != *opening
        || event.eid != request_event_eid(&request.meta.request_id, "hold.answered")?
        || event.actor != request.body.actor
        || event.host != request.body.host
        || event.layer != "L5"
        || event.obj != *hold
        || !event.guards.is_empty()
        || reply.id != event.eid
        || reply.text != *text
        || reply.actor != request.body.actor
        || reply.relayed_from.is_some()
        || reply.self_reported.is_some()
        || reply.decision.is_some()
        || reply.decision_revision.is_some()
        || !reply.extra.is_empty()
        || reply.at_ms < raw.opened_ms
        || record.p3_receipt.request_id != request.meta.request_id
        || record.p3_receipt.body_hash != request.meta.body_hash
        || record.p3_receipt.result_ref
            != (AskAnswerResult {
                hold: hold.clone(),
                opening_eid: original.eid.clone(),
                reply: reply.clone(),
            })
    {
        return Err("App Answer differs from its finite original opening/request/receipt".into());
    }
    Ok(record)
}
/// One pure raw delta shared with Relay. Unknown nested Hold data stays exact.
pub(crate) fn install_reply(raw: &mut Value, reply: &Reply) -> Result<(), String> {
    raw["reply"] = serde_json::json!(reply);
    for slice in raw["waits"].as_array_mut().ok_or("Ask waits missing")? {
        if slice["finished_ms"].is_null() {
            slice["finished_ms"] = serde_json::json!(reply.at_ms.min(
                slice["deadline_ms"]
                    .as_u64()
                    .ok_or("Ask slice deadline missing")?
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "ask_answer_contract_tests.rs"]
mod tests;
