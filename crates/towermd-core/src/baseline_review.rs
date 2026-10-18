//! Review real private draft refs. Receipt acceptance and current validity are
//! separate: no function here publishes main, creates a tag, or changes a draft.
pub use crate::recovery_contract::baseline::{ApprovalFact, ReviewBinding, Verdict};
use crate::{
    baseline_draft::{freeze, FrozenDraft},
    catalog::Catalog,
    okf::Document,
    project_session::ProjectSession,
    recovery_contract::{
        self,
        recorded::{BaselineReviewAction, BaselineReviewRecorded, RequestReceipt},
    },
    recovery_outbox::{RecoveryInputs, RecoveryOutbox},
    request_ledger::{BaselineRequest, RecordedBaselineReceipt},
    snapshot_service::{read_draft, SavedDraft},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewAction {
    pub request_id: String,
    pub binding: ReviewBinding,
    pub verdict: Verdict,
    pub comment: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathDiff {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReviewView {
    pub binding: ReviewBinding,
    pub main_revision: String,
    pub current_baseline: Option<String>,
    pub draft: FrozenDraft,
    pub diff: Vec<PathDiff>,
    pub interface_modules: Vec<String>,
    pub publish_blockers: Vec<String>,
    pub verification: ReviewVerification,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ReviewVerification {
    Available {
        scan_id: String,
        source_revision: String,
        report_eid: String,
        report: Box<crate::verifier::VerificationReport>,
        evidence: Vec<serde_json::Value>,
    },
    Unavailable {
        reason: String,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewValidity {
    Current,
    HistoricalOnly { reason: String },
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReviewResult {
    pub receipt: RequestReceipt<ApprovalFact>,
    pub current_validity: ReviewValidity,
}

pub fn matches_binding(left: &ReviewBinding, right: &ReviewBinding) -> bool {
    left == right
}
pub fn review_binding(draft: &FrozenDraft, revision: &str) -> ReviewBinding {
    ReviewBinding {
        project_id: draft.bundle.project_id.clone(),
        object_id: draft.bundle.id.clone(),
        content_hash: draft.content_hash.clone(),
        target_baseline: draft.bundle.target_baseline.clone(),
        draft_revision: revision.into(),
    }
}

pub fn review_view(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    id: &str,
) -> Result<Option<ReviewView>, String> {
    let Some(saved) = read_draft(catalog, id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let frozen = saved.frozen();
    let source = Catalog::at_revision(&catalog.root, &frozen.bundle.source_revision)?
        .ok_or("draft source has no project configuration")?;
    let mut changes = BTreeMap::new();
    for (path, file) in &frozen.files {
        changes.insert(
            path.clone(),
            PathDiff {
                path: path.clone(),
                before: source.read_file(path)?,
                after: Some(file.text.clone()),
            },
        );
    }
    for path in &frozen.bundle.deletions {
        changes.insert(
            path.clone(),
            PathDiff {
                path: path.clone(),
                before: source.read_file(path)?,
                after: None,
            },
        );
    }
    let interfaces = affected_interface_modules(changes.values());
    let mut blockers = vec![];
    if let Err(reason) = check_current(catalog, inputs, &saved) {
        blockers.push(reason);
    }
    let current_baseline = match crate::baseline_current::current_baseline(catalog, inputs) {
        Ok(current) => current.map(|value| value.baseline),
        Err(reason) => {
            blockers.push(reason);
            None
        }
    };
    if frozen.bundle.target_baseline != "towermd/B0" {
        blockers
            .push("后续基线出版、提案与任务复核尚未接通；当前仅支持保存、编辑与审阅草稿".into());
    }
    let verification =
        match crate::coldstart_evidence::read_completed(catalog, inputs, &frozen.bundle.id)? {
            Some(completed)
                if completed.scan_id() == frozen.bundle.id
                    && completed.input().project_id == frozen.bundle.project_id
                    && completed.input().project_id == catalog.config.project_id()
                    && completed.input().common_dir == catalog.common_dir
                    && completed.input().revision == frozen.bundle.source_revision =>
            {
                ReviewVerification::Available {
                    scan_id: completed.scan_id().into(),
                    source_revision: completed.input().revision.clone(),
                    report_eid: completed.report_eid().into(),
                    report: Box::new(completed.report().clone()),
                    evidence: completed.evidence().to_vec(),
                }
            }
            Some(_) => ReviewVerification::Unavailable {
                reason:
                    "recorded verification does not match this draft's fixed scan/project/source"
                        .into(),
            },
            None => ReviewVerification::Unavailable {
                reason: "no complete accepted coldstart verification is recorded for this draft"
                    .into(),
            },
        };
    Ok(Some(ReviewView {
        binding: review_binding(frozen, saved.revision()),
        main_revision: catalog.revision.clone(),
        current_baseline,
        draft: frozen.clone(),
        diff: changes.into_values().collect(),
        interface_modules: interfaces.into_iter().collect(),
        publish_blockers: blockers,
        verification,
    }))
}

fn affected_interface_modules<'a>(
    changes: impl IntoIterator<Item = &'a PathDiff>,
) -> BTreeSet<String> {
    let mut modules = BTreeSet::new();
    for change in changes {
        if change.before == change.after {
            continue;
        }
        // Diff sides are actual repository documents, including deleted and
        // opaque entries. Only understood Interface endpoints imply impact.
        for text in change.before.iter().chain(change.after.iter()) {
            let Ok(document) = Document::parse(text) else {
                continue;
            };
            if document.doc_type() == Some("Interface") {
                for key in ["from", "to"] {
                    if let Some(module) = document.get_str(key) {
                        modules.insert(module.to_owned());
                    }
                }
            }
        }
    }
    modules
}

fn check_current(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    saved: &SavedDraft,
) -> Result<(), String> {
    if saved.frozen().bundle.project_id != catalog.config.project_id() {
        return Err("draft project mismatch".into());
    }
    crate::baseline_current::validate_draft_target(
        catalog,
        inputs,
        &saved.frozen().bundle.target_baseline,
    )?;
    // freeze verifies the exact fixed source and current complete protected map,
    // while ignoring state/artifacts and unrelated source-code advancement.
    if freeze(catalog, &saved.frozen().bundle)? != *saved.frozen() {
        return Err("current protected source differs from the viewed draft".into());
    }
    Ok(())
}

fn current_validity(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    binding: &ReviewBinding,
) -> ReviewValidity {
    let result = (|| {
        let current = Catalog::load(&catalog.root)?;
        if current.common_dir != catalog.common_dir
            || current.config.project_id() != binding.project_id
        {
            return Err("current project/clone changed".into());
        }
        let saved = read_draft(&current, &binding.object_id)
            .map_err(|e| e.to_string())?
            .ok_or("draft ref is no longer present")?;
        if !matches_binding(binding, &review_binding(saved.frozen(), saved.revision())) {
            return Err("current draft revision/content/target changed".into());
        }
        check_current(&current, inputs, &saved)
    })();
    match result {
        Ok(()) => ReviewValidity::Current,
        Err(reason) => ReviewValidity::HistoricalOnly { reason },
    }
}

fn outcome(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    recorded: RecordedBaselineReceipt,
) -> ReviewResult {
    let current_validity = if recorded.original.stale || !recorded.original.guards.is_empty() {
        ReviewValidity::HistoricalOnly {
            reason: "original review has historical guards/stale status".into(),
        }
    } else {
        current_validity(catalog, inputs, &recorded.receipt.result_ref.binding)
    };
    ReviewResult {
        receipt: recorded.receipt,
        current_validity,
    }
}

/// Only this actual-reader path constructs the first-review input for Session.
/// ReviewView and client JSON cannot construct this value.
pub(crate) struct PreparedBaselineReview {
    request: BaselineRequest,
    event: crate::hub_protocol::NewEvent,
}
impl PreparedBaselineReview {
    pub(crate) fn request(&self) -> &BaselineRequest {
        &self.request
    }
    pub(crate) fn event(&self) -> &crate::hub_protocol::NewEvent {
        &self.event
    }
}

pub fn review_baseline_shared(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    action: &ReviewAction,
    actor: &str,
    host: &str,
    now: u64,
) -> Result<ReviewResult, String> {
    let request = BaselineRequest::prepare(
        catalog.config.project_id(),
        host,
        actor,
        &action.request_id,
        BaselineReviewAction::ReviewBaseline {
            binding: action.binding.clone(),
            verdict: action.verdict.clone(),
            comment: action.comment.clone(),
        },
    )?;
    // Idempotency precedes current-subject checks: a historical success retains
    // its original body and receipt even after the draft/target has moved.
    if let Some(recorded) =
        ProjectSession::retry_baseline_review_shared(session, catalog, outbox, &request, now)?
    {
        return Ok(outcome(
            catalog,
            &outbox.read_inputs().map_err(|reason| {
                crate::request_ledger::uncertain_request(&action.request_id, reason)
            })?,
            recorded,
        ));
    }
    if action.verdict == Verdict::Approved && action.binding.target_baseline != "towermd/B0" {
        return Err("后续基线出版、提案与任务复核尚未接通，暂不能批准此草稿".into());
    }
    let saved = read_draft(catalog, &action.binding.object_id)
        .map_err(|e| e.to_string())?
        .ok_or("baseline review conflict: draft ref is absent")?;
    check_current(
        catalog,
        &outbox.read_inputs().map_err(|reason| {
            crate::request_ledger::uncertain_request(&action.request_id, reason)
        })?,
        &saved,
    )
    .map_err(|e| format!("baseline review conflict: {e}"))?;
    if !matches_binding(
        &action.binding,
        &review_binding(saved.frozen(), saved.revision()),
    ) {
        return Err("baseline review conflict: viewed binding differs from actual draft".into());
    }
    #[cfg(test)]
    test_hooks::run_before();
    let checked = read_draft(catalog, &action.binding.object_id)
        .map_err(|e| e.to_string())?
        .ok_or("baseline review conflict: draft ref disappeared")?;
    if !matches_binding(
        &action.binding,
        &review_binding(checked.frozen(), checked.revision()),
    ) {
        return Err("baseline review conflict: draft changed before recording review".into());
    }
    check_current(
        catalog,
        &outbox.read_inputs().map_err(|reason| {
            crate::request_ledger::uncertain_request(&action.request_id, reason)
        })?,
        &checked,
    )
    .map_err(|e| format!("baseline review conflict: {e}"))?;
    let approval = ApprovalFact {
        approval_eid: request.event_eid.clone(),
        binding: action.binding.clone(),
        actor: actor.into(),
        verdict: action.verdict.clone(),
        comment: action.comment.clone(),
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
    let event = crate::hub_protocol::NewEvent {
        eid: request.event_eid.clone(),
        host: host.into(),
        actor: actor.into(),
        kind: request.event_kind.into(),
        layer: "L1".into(),
        obj: action.binding.object_id.clone(),
        guards: vec![],
        data: serde_json::to_value(recorded).map_err(|e| e.to_string())?,
    };
    recovery_contract::validate_recorded_data(
        catalog.config.project_id(),
        &event.kind,
        &event.data,
    )?;
    let prepared = PreparedBaselineReview { request, event };
    let accepted =
        ProjectSession::record_baseline_review_shared(session, catalog, outbox, &prepared, now)?;
    #[cfg(test)]
    test_hooks::run_after();
    Ok(outcome(
        catalog,
        &outbox.read_inputs().map_err(|reason| {
            crate::request_ledger::uncertain_request(&action.request_id, reason)
        })?,
        accepted,
    ))
}

#[cfg(test)]
mod test_hooks {
    use std::cell::RefCell;
    thread_local! {pub(super) static BEFORE:RefCell<Option<Box<dyn FnOnce()>>>=RefCell::new(None);pub(super) static AFTER:RefCell<Option<Box<dyn FnOnce()>>>=RefCell::new(None);}
    pub(super) fn run_before() {
        let hook = BEFORE.with(|cell| cell.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
    pub(super) fn run_after() {
        let hook = AFTER.with(|cell| cell.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
}

#[cfg(test)]
#[path = "baseline_review_tests.rs"]
mod tests;
