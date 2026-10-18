//! Derived, read-only Ask archive views. Inner frozen coordinates are never
//! accepted as proof that a current opening or current recipient exists.
use super::*;
use crate::ask_answer_contract::{
    self, AskAnswerRecipient, AskAnswerValidity, AskAnswerView, AskOpeningBinding,
};

impl BaselineRequestLedger {
    pub(crate) fn historical_ask(&self, hold: &str) -> Option<AskAnswerView> {
        self.historical_asks.get(hold).cloned()
    }
    pub(super) fn restore_historical_asks(
        &mut self,
        outer: &[&Event],
        receipts: &[&Event],
    ) -> Result<(), String> {
        for opening in outer.iter().filter(|e| {
            e.kind == "hold.opened" && crate::ask_contract::is_ask_request(&e.kind, &e.data)
        }) {
            let opened = crate::ask_contract::decode(
                &self.project_id,
                &recovery_outbox::original_of(opening),
            )?;
            let hold = opened.hold();
            let mut raw = opening.data["hold"].clone();
            let mut display_opening = (**opening).clone();
            let mut app_answer: Option<crate::ask_answer_contract::AppAnswerRecorded> = None;
            // Actual outer evolution is useful as historical text, irrespective
            // of whether its original guards survived the rebuild barrier.
            for event in outer.iter().filter(|e| {
                e.obj == hold.id && e.seq >= opening.seq && crate::ask_contract::ask_kind(&e.kind)
            }) {
                if crate::ask_contract::is_ask_request(&event.kind, &event.data) {
                    let record = crate::ask_contract::decode(
                        &self.project_id,
                        &recovery_outbox::original_of(event),
                    )?;
                    let eid = match record {
                        crate::ask_contract::AskData::Ordinary(r) => r.opening_eid,
                        crate::ask_contract::AskData::Added(r) => r.opening_eid,
                    };
                    if eid != opening.eid {
                        return Err(
                            "historical Ask update refers to another original opening".into()
                        );
                    }
                    raw = event.data["hold"].clone();
                }
            }
            for event in receipts.iter().filter(|e| {
                e.obj == hold.id && ask_answer_contract::is_app_answer(&e.kind, &e.data)
            }) {
                let original = recovery_outbox::original_of(event);
                validate_human_original(&original, None, outer)?;
                let record = ask_answer_contract::decode(&self.project_id, &original)?;
                if recovery_outbox::original_of(&record.opening_event)
                    != recovery_outbox::original_of(opening)
                {
                    continue;
                }
                if let Some(previous) = &app_answer {
                    if previous != &record {
                        return Err("historical Ask has conflicting first App answers".into());
                    }
                }
                display_opening = record.opening_event.clone();
                raw = record.hold.clone();
                app_answer = Some(record);
            }
            let frozen: crate::ask_contract::AskHoldView =
                serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
            let view = AskAnswerView {
                project_id: self.project_id.clone(),
                binding: AskOpeningBinding::from_opening(&display_opening)?,
                opening: display_opening,
                current_hold: raw,
                current_validity: AskAnswerValidity::HistoricalOnly {
                    reason:
                        "Ask archive history does not restore its old Run or permit a new answer"
                            .into(),
                },
                recipients: frozen
                    .recipients
                    .into_iter()
                    .map(|recipient| AskAnswerRecipient {
                        recipient,
                        actionable: false,
                    })
                    .collect(),
                actions: vec![],
            };
            if let Some(previous) = self.historical_asks.get(&hold.id) {
                if recovery_outbox::original_of(&previous.opening)
                    != recovery_outbox::original_of(&view.opening)
                    || previous.current_hold != view.current_hold
                {
                    return Err("historical Ask opening identity or original answer changed".into());
                }
            } else {
                self.historical_asks.insert(hold.id.clone(), view);
            }
        }
        Ok(())
    }
}
