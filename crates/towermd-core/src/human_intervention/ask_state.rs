//! Finite Ask transitions. Only actual non-stale outer Events reach this path.
use super::*;
use crate::ask_contract::{self, AskData};

pub(super) fn apply(
    canonical: &mut HumanProjection,
    holds: &mut BTreeMap<String, Value>,
    event: &Event,
) -> Result<(), String> {
    let (hold, mut raw) = hold_value(event)?;
    let typed = ask_contract::is_ask_request(&event.kind, &event.data);
    let record = if typed {
        Some(ask_contract::decode(
            event.data["request_body"]["project_id"]
                .as_str()
                .ok_or("Ask lacks project")?,
            &crate::recovery_outbox::original_of(event),
        )?)
    } else {
        None
    };
    let app_answer = if crate::ask_answer_contract::is_app_answer(&event.kind, &event.data) {
        Some(crate::ask_answer_contract::decode(
            event.data["request_body"]["project_id"]
                .as_str()
                .ok_or("App Answer lacks project")?,
            &crate::recovery_outbox::original_of(event),
        )?)
    } else {
        None
    };
    let actual_ms = u64::try_from(
        chrono::DateTime::parse_from_rfc3339(&event.t)
            .map_err(|e| e.to_string())?
            .timestamp_millis(),
    )
    .map_err(|_| "Ask event has negative time")?;
    if event.kind == "hold.opened" {
        if let Some(previous) = canonical.openings.get(&hold.id) {
            return if previous == event {
                Ok(())
            } else {
                Err("Ask opening identity was reused".into())
            };
        }
        if hold.reply.is_some()
            || !hold.waits.is_empty()
            || !hold.delivered.is_empty()
            || hold.attention
        {
            return Err("Ask opening is not a fresh unanswered Hold".into());
        }
        if typed
            && (hold.recipients.len() != 1 || hold.opened_seq != 0 || hold.opened_ms > actual_ms)
        {
            return Err("typed Ask opening requires exactly one original recipient".into());
        }
        if let Some(AskData::Ordinary(r)) = &record {
            if r.opening_eid != event.eid || r.recipient.as_ref() != hold.recipients.first() {
                return Err("Ask opening recipient/eid mismatch".into());
            }
        }
        raw["opened_seq"] = json!(event.seq);
        canonical.openings.insert(hold.id.clone(), event.clone());
        holds.insert(hold.id, raw);
        return Ok(());
    }
    let original = canonical
        .openings
        .get(&hold.id)
        .ok_or("Ask update lacks actual outer opening")?;
    if original.seq >= event.seq || original.stale || original.data["hold"]["body"]["kind"] != "ask"
    {
        return Err("Ask update lacks an earlier actual non-stale Ask opening".into());
    }
    let previous = holds
        .get(&hold.id)
        .ok_or("Ask update lacks current effective Hold")?;
    let old: Hold = serde_json::from_value(previous.clone()).map_err(|e| e.to_string())?;
    if hold.opened_seq != original.seq || hold.opened_ms != old.opened_ms {
        return Err("Ask update changed original opening sequence/time".into());
    }
    if let Some(record) = &record {
        let opening = match record {
            AskData::Ordinary(r) => &r.opening_eid,
            AskData::Added(r) => &r.opening_eid,
        };
        if opening != &original.eid {
            return Err("Ask update refers to another opening".into());
        }
    }
    if let Some(answer) = &app_answer {
        if answer.opening_event != *original || original.seq <= canonical.invalidated_through {
            return Err("App Answer requires the exact current actual outer opening".into());
        }
    }
    let mut expected = previous.clone();
    match event.kind.as_str() {
        "hold.recipient_added" => {
            let Some(AskData::Added(r)) = &record else {
                return Err("recipient addition requires its finite original request".into());
            };
            if old.reply.is_some() || old.recipients.iter().any(|p| p.run == r.added.run) {
                return Err("cannot join an answered Ask or replace an existing run".into());
            }
            expected["recipients"]
                .as_array_mut()
                .ok_or("Ask recipients is not an array")?
                .push(serde_json::to_value(&r.added).map_err(|e| e.to_string())?);
        }
        "hold.requested" => {} // A receipt-only observation never edits effective state.
        "hold.wait_started" => {
            let Some(AskData::Ordinary(r)) = &record else {
                return Err("Ask slice requires its original request".into());
            };
            let recipient = r
                .recipient
                .as_ref()
                .ok_or("wait lacks its frozen recipient")?;
            if !old.recipients.contains(recipient) {
                return Err("wait cannot join an Ask".into());
            }
            let slice = r.slice.as_ref().ok_or("wait lacks slice")?;
            let planned = next_slice(
                &old,
                &recipient.run,
                slice.started_ms,
                &r.p3_request.request_id,
            )?
            .ok_or("Ask cannot start another slice")?;
            if *slice != planned || slice.started_ms > actual_ms || r.p3_receipt.is_some() {
                return Err("Ask started slice differs from bounded original intent".into());
            }
            expected["waits"]
                .as_array_mut()
                .ok_or("Ask waits is not an array")?
                .push(json!(slice));
        }
        "hold.wait_finished" => {
            let Some(AskData::Ordinary(r)) = &record else {
                return Err("Ask finish lacks original request".into());
            };
            let slice = r.slice.as_ref().ok_or("Ask finish lacks slice")?;
            let before = old
                .waits
                .iter()
                .find(|s| s.id == slice.id)
                .ok_or("Ask finish lacks actual outer started slice")?;
            let finish = slice.finished_ms.ok_or("Ask finish is not finished")?;
            let expected_finish = old
                .reply
                .as_ref()
                .map(|r| r.at_ms.min(before.deadline_ms))
                .unwrap_or(before.deadline_ms);
            let mut target = before.clone();
            target.finished_ms = Some(expected_finish);
            if (old.reply.is_none() && actual_ms < before.deadline_ms)
                || *slice != target
                || slice.id != r.p3_request.request_id
                || finish < slice.started_ms
                || r.p3_receipt.is_none()
            {
                return Err("Ask finish changed original slice/budget".into());
            }
            if let Some(prior) = before.finished_ms {
                if prior != finish {
                    return Err("Ask slice was already settled differently".into());
                }
            }
            for item in expected["waits"]
                .as_array_mut()
                .ok_or("Ask waits is not an array")?
            {
                if item["id"] == slice.id {
                    item["finished_ms"] = json!(finish);
                }
            }
        }
        "hold.answered" => {
            if old.reply.is_some() {
                return Err("Ask already has its immutable first answer".into());
            }
            let reply = hold.reply.as_ref().ok_or("Ask answer lacks Reply")?;
            if reply.text.trim().is_empty()
                || reply.at_ms < old.opened_ms
                || reply.at_ms > actual_ms
            {
                return Err("Ask answer text/time invalid".into());
            }
            if let Some(AskData::Ordinary(r)) = &record {
                match &r.request_body.action {
                    ask_contract::AskAction::Relay {
                        text,
                        self_reported,
                        ..
                    } => {
                        if reply.id
                            != crate::recovery_contract::recorded::request_event_eid(
                                &r.p3_request.request_id,
                                "hold.answered",
                            )?
                            || reply.text != *text
                            || reply.self_reported != *self_reported
                            || reply.actor != "process:towermd-cli"
                            || reply.relayed_from.as_deref() != Some("human")
                            || reply.decision.is_some()
                            || reply.decision_revision.is_some()
                        {
                            return Err("relay changed the original answer/provenance".into());
                        }
                    }
                    ask_contract::AskAction::AskOpen { .. } => {
                        let opened = ask_contract::decode(
                            &r.request_body.project_id,
                            &crate::recovery_outbox::original_of(original),
                        )?;
                        let AskData::Ordinary(opened) = opened else {
                            return Err("Decision answer lacks its own planned opening".into());
                        };
                        if opened.p3_request != r.p3_request
                            || opened.planned_reply.as_ref() != Some(reply)
                            || r.planned_reply.as_ref() != Some(reply)
                        {
                            return Err(
                                "Decision answer differs from its frozen opening plan".into()
                            );
                        }
                    }
                    _ => return Err("unsupported Ask answer producer".into()),
                }
            }
            expected["reply"] = json!(reply);
            for item in expected["waits"]
                .as_array_mut()
                .ok_or("Ask waits is not an array")?
            {
                if item["finished_ms"].is_null() {
                    let deadline = item["deadline_ms"]
                        .as_u64()
                        .ok_or("Ask slice deadline missing")?;
                    item["finished_ms"] = json!(reply.at_ms.min(deadline));
                }
            }
        }
        "hold.attention" => {
            if old.reply.is_some()
                || old.attention
                || actual_ms < old.opened_ms.saturating_add(1_800_000)
            {
                return Err("Ask attention is not a fresh pending transition".into());
            }
            let Some(AskData::Ordinary(r)) = &record else {
                return Err("attention lacks its finite original system intent".into());
            };
            if !ask_contract::is_ask_request(&original.kind, &original.data)
                || original.data["hold"]["attention_request_id"] != r.p3_request.request_id
                || r.request_body.host != original.host
            {
                return Err("attention lacks its actual typed opening's fixed identity".into());
            }
            expected["attention"] = json!(true);
        }
        _ => return Err("unsupported Ask transition".into()),
    }
    if raw != expected {
        return Err("Ask transition changed unrelated known/unknown Hold fields".into());
    }
    if event.kind != "hold.requested" {
        holds.insert(hold.id, raw);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../project_session/ask_late_acceptance_tests.rs"]
mod late_acceptance_tests;
