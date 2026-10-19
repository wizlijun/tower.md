//! Finite control receipt/recipient history, never a new current human fact.
use super::*;
use crate::{
    ask_contract::Recipient,
    human_intervention::Hold,
    intervention_control::{self as control, ControlRecorded, ControlRequest},
};
impl BaselineRequestLedger {
    pub(crate) fn control_claim(&self, recipient: &Recipient) -> Result<Event, String> {
        self.delivery_sources
            .values()
            .find(|event| {
                if event.kind != "task.claimed" || event.stale {
                    return false;
                }
                let Ok(run) =
                    serde_json::from_value::<crate::tasks::Run>(event.data["run"].clone())
                else {
                    return false;
                };
                run.id == recipient.run
                    && run.task == recipient.task
                    && run.host == recipient.host
                    && run.worktree == recipient.worktree
                    && run.branch == recipient.branch
                    && run.leases == recipient.fences
            })
            .cloned()
            .ok_or("control recipient lacks exact verified original claim".into())
    }
    pub(crate) fn control_phase(
        &self,
        request: &ControlRequest,
    ) -> Result<Option<(NewEvent, Option<Event>)>, String> {
        let Some(entry) = self.matching_identity(&RequestIdentity::Control(request.clone()))?
        else {
            return Ok(None);
        };
        Ok(entry
            .original
            .clone()
            .map(|original| (original, entry.accepted.clone())))
    }
    pub(crate) fn control_prompt_phase(
        &self,
        request: &ControlRequest,
    ) -> Result<Option<(NewEvent, Option<Event>)>, String> {
        Ok(self
            .matching_identity(&RequestIdentity::Control(request.clone()))?
            .and_then(|entry| entry.phases.get("human.prompt"))
            .cloned())
    }
    pub(crate) fn control_records(&self) -> Result<Vec<(Event, Hold)>, String> {
        let mut result = Vec::new();
        for entry in self
            .entries
            .values()
            .filter(|entry| matches!(&entry.request, RequestIdentity::Control(_)))
        {
            if let Some(event) = &entry.accepted {
                let ControlRecorded { hold, .. } =
                    control::decode(&self.project_id, &recovery_outbox::original_of(event))?;
                result.push((event.clone(), hold));
            }
        }
        result.sort_by_key(|(event, _)| event.seq);
        Ok(result)
    }
}
