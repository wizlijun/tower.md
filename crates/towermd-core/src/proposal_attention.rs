//! Read-only projection of current, actually accepted Proposal subjects. No
//! raw tail, ref-only object or historical inner receipt creates a review item.
use super::{ProposalAttention, ProposalAttentionState};
use crate::{
    baseline_review::{self, Verdict},
    catalog::Catalog,
    proposals,
    recovery_outbox::{self, RecoveryInputs},
    request_ledger::BaselineRequestLedger,
    snapshot_service,
};

pub(crate) fn read_proposal_attention(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    ledger: &BaselineRequestLedger,
) -> (Vec<ProposalAttention>, Vec<String>) {
    let mut pending = vec![];
    let mut diagnostics=ledger.proposal_attention_unqualified_ids().into_iter().map(|id|format!("历史或尚未核实的提案 {id}：只见原请求/保存原件，未核实当前实际接受资格；不推断拒绝、批准或出版。仅可核对原件与原请求。" )).collect::<Vec<_>>();
    // This ledger came from the same PreparedSession complete verified history.
    // Its actual_proposals roles are invalidated by actual Hub rebuilds.
    for actual in ledger.proposal_attention_candidates() {
        let observed = (|| -> Result<Option<ProposalAttention>, String> {
            let saved=snapshot_service::read_draft(catalog,&actual.obj).map_err(|e|e.to_string())?
                .ok_or("actual Proposal draft ref is absent; original acceptance is historical, not rejected")?;
            let accepted=ledger.accepted_proposal(&actual.obj,saved.revision())?
                .ok_or("current draft revision no longer matches its actual Proposal acceptance; no fresh review item")?;
            if accepted != actual {
                return Err("Proposal actual acceptance changed in the proved ledger".into());
            }
            let record = proposals::decode_created(
                catalog.config.project_id(),
                &recovery_outbox::original_of(&accepted),
            )?;
            proposals::require_saved(&record, &saved)?;
            let binding = baseline_review::review_binding(saved.frozen(), saved.revision());
            let decision = ledger.terminal_baseline_review(&binding)?;
            if decision
                .as_ref()
                .is_some_and(|fact| fact.verdict == Verdict::Rejected)
            {
                return Ok(None);
            }
            if let Some(fact) = decision {
                // A terminal approval cannot become a new review item. Its
                // publication status belongs to the existing original receipt.
                return Err(format!("original approval recorded; read original request receipt for publication status; approval_eid={}; do not prepare another approval",fact.approval_eid));
            }
            let view = baseline_review::review_view(catalog, inputs, &actual.obj)?
                .ok_or("Proposal review subject disappeared")?;
            if view.binding != binding {
                return Err("Proposal review and current saved five-field binding differ".into());
            }
            if binding.target_baseline == "towermd/B0" {
                return Err("initial B0 draft is not a later Proposal".into());
            }
            // Existing check_current verifies the protected source and the real
            // publication/tag chain. A past/published target cannot be pending.
            if !view.publish_blockers.is_empty() {
                return Err(format!("subject is historical or not currently eligible: {}; no rejection or approval is inferred",view.publish_blockers.join("; ")));
            }
            let at = chrono::DateTime::parse_from_rfc3339(&accepted.t)
                .map_err(|e| e.to_string())?
                .timestamp_millis();
            let created_at_ms =
                u64::try_from(at).map_err(|_| "Proposal timestamp precedes epoch")?;
            Ok(Some(ProposalAttention {
                state: ProposalAttentionState::ReviewPending,
                binding,
                created_seq: accepted.seq,
                created_at_ms,
                reason: record.result.carrier.input.reason,
                recommendation: record.result.carrier.input.recommendation,
                impact: record.result.carrier.impact,
            }))
        })();
        match observed {
            Ok(Some(item)) => pending.push(item),
            Ok(None) => {}
            Err(reason) => diagnostics.push(format!(
                "提案 {} 未列入当前待评审关注：{reason}",
                actual.obj
            )),
        }
    }
    pending.sort_by(|a, b| {
        b.impact
            .tasks
            .len()
            .cmp(&a.impact.tasks.len())
            .then_with(|| a.created_at_ms.cmp(&b.created_at_ms))
            .then_with(|| a.binding.object_id.cmp(&b.binding.object_id))
    });
    (pending, diagnostics)
}
