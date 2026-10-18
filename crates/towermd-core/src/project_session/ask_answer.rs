//! Minimal App Answer producer. Reads do not renew or append; a saved original
//! UUID is repaired before considering current Ask eligibility.
use super::ProjectSession;
use crate::{
    ask_answer_contract::{
        self, AppAnswerAction, AppAnswerRecorded, AppAnswerRequest, AskAnswerOutcome,
        AskAnswerRecipient, AskAnswerResult, AskAnswerValidity, AskAnswerView, AskAnswerViewAction,
        AskOpeningBinding,
    },
    ask_contract::AskHoldView,
    catalog::Catalog,
    hub_protocol::{AppendReceipt, HubError, NewEvent},
    human_intervention::Reply,
    recovery_contract::recorded::{request_event_eid, RequestReceipt},
    recovery_outbox::{self, RecoveryInputs, RecoveryOutbox},
    request_ledger::{uncertain_request, RequestIdentity},
};
use std::{collections::BTreeMap, sync::Mutex};

fn view(
    live: &ProjectSession,
    catalog: &Catalog,
    hold: &str,
    now: u64,
) -> Result<AskAnswerView, String> {
    live.projection()?;
    let (raw, canonical) = live.human.projection();
    let opening = canonical
        .openings
        .get(hold)
        .filter(|e| !e.stale && e.seq > canonical.invalidated_through);
    let Some(opening) = opening else {
        return live
            .baseline_requests
            .as_ref()
            .and_then(|l| l.historical_ask(hold))
            .ok_or_else(|| "Ask has no current actual opening or verified archive history".into());
    };
    if opening.data["hold"]["body"]["kind"] != "ask" {
        return Err("App Answer only accepts Ask, never Review/Prompt/Pause".into());
    }
    // A finite Answer may reference only an actual typed Ask producer. Older
    // metadata-free Holds remain readable by their original Human consumer.
    crate::ask_contract::decode(
        catalog.config.project_id(),
        &recovery_outbox::original_of(opening),
    )?;
    let current_hold = raw
        .get(hold)
        .ok_or("Ask lacks its complete current raw Hold")?
        .clone();
    let typed: AskHoldView =
        serde_json::from_value(current_hold.clone()).map_err(|e| e.to_string())?;
    let recipients = typed
        .recipients
        .iter()
        .map(|recipient| AskAnswerRecipient {
            actionable: !recipient.fences.is_empty()
                && recipient.fences.iter().all(|f| {
                    f.holder == recipient.run
                        && live
                            .head
                            .leases
                            .get(&f.name)
                            .is_some_and(|l| l.fence() == *f && l.expires_at > now)
                }),
            recipient: recipient.clone(),
        })
        .collect();
    Ok(AskAnswerView {
        project_id: catalog.config.project_id().into(),
        opening: opening.clone(),
        current_hold,
        binding: AskOpeningBinding::from_opening(opening)?,
        current_validity: AskAnswerValidity::Current,
        recipients,
        actions: if typed.reply.is_none() {
            vec![AskAnswerViewAction::Answer]
        } else {
            vec![]
        },
    })
}
impl ProjectSession {
    /// Only synchronizes an already selected, source-proved Session. No lease
    /// acquisition/renewal, durability repair, or missing-Hub creation occurs.
    pub fn load_ask_intervention_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        hold: &str,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<AskAnswerView, String> {
        crate::hub::validate_token("Hold ID", hold).map_err(|e| e.to_string())?;
        let now = clock()?;
        Self::continue_shared(session, catalog, inputs, now, |live| {
            view(live, catalog, hold, now)
        })
        .map_err(|e| e.to_string())?
    }
    /// Authentication belongs to the adapter's fixed AppGate. The core validates
    /// the finite human identity and proves all sources and current raw state.
    #[allow(
        clippy::too_many_arguments,
        reason = "immutable App identity and same-project durable outbox remain explicit"
    )]
    pub fn answer_ask_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        actor: &str,
        request_id: &str,
        action: &AppAnswerAction,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<AskAnswerOutcome, String> {
        let request = AppAnswerRequest::prepare(
            catalog.config.project_id(),
            host,
            actor,
            request_id,
            action,
        )?;
        let identity = RequestIdentity::AskAnswer(request.clone());
        let first_now = clock().map_err(|e| uncertain_request(request_id, e))?;
        let event = match super::human_review::retry_shared(
            session, catalog, outbox, &identity, request_id, first_now,
        )? {
            Some(event) => event,
            None => {
                let now = clock().map_err(|e| uncertain_request(request_id, e))?;
                if now < first_now {
                    return Err(uncertain_request(
                        request_id,
                        "clock moved backwards before App Answer",
                    ));
                }
                let inputs = super::human_review::new_review_inputs(
                    session, catalog, outbox, request_id, now,
                )?;
                let AppAnswerAction::Answer {
                    hold,
                    opening,
                    text,
                } = action;
                let result = Self::continue_shared(session, catalog, &inputs, now, |live| {
                    let ledger = live.baseline_requests.as_ref().ok_or_else(|| {
                        HubError::Invalid("App Answer request history is not prepared".into())
                    })?;
                    if let Some(event) =
                        ledger.retry_original(catalog, &identity, &mut *live.hub, outbox, now)?
                    {
                        return Ok(Some(event));
                    }
                    let current = view(live, catalog, hold, now).map_err(HubError::Invalid)?;
                    if current.current_validity != AskAnswerValidity::Current {
                        return Err(HubError::Invalid(
                            "historical Ask is read-only; a new late answer is not supported"
                                .into(),
                        ));
                    }
                    opening
                        .validate(&current.opening)
                        .map_err(HubError::Invalid)?;
                    let raw: AskHoldView = serde_json::from_value(current.current_hold.clone())
                        .map_err(|e| HubError::Invalid(e.to_string()))?;
                    if raw.reply.is_some() {
                        return Err(HubError::Invalid(
                            "Ask already answered; retry the original request".into(),
                        ));
                    }
                    if now < raw.opened_ms
                        || raw
                            .waits
                            .iter()
                            .any(|s| s.finished_ms.is_none() && now < s.started_ms)
                    {
                        return Err(HubError::Uncertain(
                            "App Answer clock predates current Ask/wait".into(),
                        ));
                    }
                    let eid = request_event_eid(request_id, "hold.answered")
                        .map_err(HubError::Invalid)?;
                    let reply = Reply {
                        id: eid.clone(),
                        text: text.clone(),
                        actor: actor.into(),
                        relayed_from: None,
                        self_reported: None,
                        decision: None,
                        decision_revision: None,
                        at_ms: now,
                        extra: BTreeMap::new(),
                    };
                    let mut raw = current.current_hold;
                    ask_answer_contract::install_reply(&mut raw, &reply)
                        .map_err(HubError::Invalid)?;
                    let record = AppAnswerRecorded {
                        hold: raw,
                        opening: opening.clone(),
                        opening_event: current.opening,
                        request_body: request.body.clone(),
                        p3_request: request.meta.clone(),
                        p3_receipt: RequestReceipt {
                            request_id: request_id.into(),
                            body_hash: request.meta.body_hash.clone(),
                            result_ref: AskAnswerResult {
                                hold: hold.clone(),
                                opening_eid: opening.eid.clone(),
                                reply,
                            },
                        },
                    };
                    let event = NewEvent {
                        eid,
                        host: host.into(),
                        actor: actor.into(),
                        kind: "hold.answered".into(),
                        layer: "L5".into(),
                        obj: hold.clone(),
                        guards: vec![],
                        data: serde_json::to_value(record)
                            .map_err(|e| HubError::Invalid(e.to_string()))?,
                    };
                    ask_answer_contract::decode(catalog.config.project_id(), &event)
                        .map_err(HubError::Invalid)?;
                    match recovery_outbox::append_durable(&mut *live.hub, outbox, event, now)? {
                        AppendReceipt::Event(event) => Ok(Some(*event)),
                        AppendReceipt::Duplicate { .. } => Err(HubError::Uncertain(
                            "App Answer duplicate has no actual original receipt".into(),
                        )),
                    }
                })
                .map_err(|e| uncertain_request(request_id, e))?;
                Self::finish_baseline_request_shared(
                    session, catalog, outbox, request_id, result, now,
                )?
                .ok_or("App Answer returned no actual receipt")?
            }
        };
        let record = ask_answer_contract::decode(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&event),
        )
        .map_err(|e| uncertain_request(request_id, e))?;
        let inputs = outbox
            .read_inputs()
            .map_err(|e| uncertain_request(request_id, e))?;
        let current = Self::load_ask_intervention_shared(
            session,
            catalog,
            &inputs,
            &record.p3_receipt.result_ref.hold,
            &clock,
        )
        .map_err(|e| uncertain_request(request_id, e))?;
        Ok(AskAnswerOutcome {
            receipt: record.p3_receipt,
            current_validity: if event.stale {
                AskAnswerValidity::HistoricalOnly {
                    reason: "original App Answer was accepted stale".into(),
                }
            } else {
                current.current_validity
            },
        })
    }
}
