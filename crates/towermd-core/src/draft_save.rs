//! The typed save producer. Git materialization and business acceptance remain
//! separate steps; only an actual reader result can prepare the latter.
use super::*;
use crate::{
    baseline_draft::{apply_edit_at, freeze, freeze_for_save, from_scan},
    project_session::ProjectSession,
    snapshot_service::{read_draft, DraftMaterialization, SavedDraft, SnapshotWriter},
};
use serde::Serialize;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DraftSaveValidity {
    Current,
    HistoricalOnly { reason: String },
}
#[derive(Clone, Debug, Serialize)]
pub struct DraftSaveReceipt {
    pub receipt: RequestReceipt<DraftSavedResult>,
    pub current_validity: DraftSaveValidity,
}

pub(crate) struct PreparedDraftSaved {
    request: PreparedDraftSaveRequest,
    event: NewEvent,
}
impl PreparedDraftSaved {
    pub(crate) fn request(&self) -> &PreparedDraftSaveRequest {
        &self.request
    }
    pub(crate) fn event(&self) -> &NewEvent {
        &self.event
    }
}

fn validity(catalog: &Catalog, result: &DraftSavedResult) -> DraftSaveValidity {
    let check = (|| {
        let current = Catalog::load(&catalog.root)?;
        if current.common_dir != catalog.common_dir
            || current.config.project_id() != catalog.config.project_id()
        {
            return Err("current project/clone changed".into());
        }
        let saved = read_draft(&current, &result.draft_id)
            .map_err(|e| e.to_string())?
            .ok_or("draft ref is absent")?;
        if saved.revision() != result.revision || saved.frozen().content_hash != result.content_hash
        {
            return Err("current draft revision/content changed".into());
        }
        if freeze(&current, &saved.frozen().bundle)? != *saved.frozen() {
            return Err("current draft source/target is no longer valid".into());
        }
        Ok(())
    })();
    match check {
        Ok(()) => DraftSaveValidity::Current,
        Err(reason) => DraftSaveValidity::HistoricalOnly { reason },
    }
}
fn outcome(catalog: &Catalog, recorded: RecordedDraftSaveReceipt) -> DraftSaveReceipt {
    let current_validity = if recorded.original.stale || !recorded.original.guards.is_empty() {
        DraftSaveValidity::HistoricalOnly {
            reason: "original save has historical stale/guards status".into(),
        }
    } else {
        validity(catalog, &recorded.receipt.result_ref)
    };
    DraftSaveReceipt {
        receipt: recorded.receipt,
        current_validity,
    }
}

fn prepared(
    catalog: &Catalog,
    saved: &SavedDraft,
    materialized: Option<&DraftMaterialization>,
) -> Result<PreparedDraftSaved, String> {
    let request = saved.request();
    if request.body().project_id != catalog.config.project_id()
        || request.draft_id() != saved.frozen().bundle.id
    {
        return Err("saved draft request project/id mismatch".into());
    }
    let reference = crate::repository_writer::RepositoryRef::draft(request.draft_id())?;
    let result = if let Some(actual) = materialized {
        if actual.result().draft_id != request.draft_id()
            || actual.revision() != saved.revision()
            || actual.content_hash() != saved.frozen().content_hash
            || actual.reference() != reference.name()
        {
            return Err("actual materialization differs from verified saved ref".into());
        }
        actual.result().clone()
    } else {
        // Only the absence of an actual origin proves local-only delivery. A
        // local hub or a surviving ref cannot certify a past remote push.
        let delivery = match crate::repository_writer::read_git(&catalog.root, &["remote"]) {
            Ok(remotes) if !remotes.lines().any(|name| name == "origin") => {
                DraftDelivery::LocalSaved
            }
            _ => DraftDelivery::LocalSavedRemotePending {
                reason: "remote delivery not confirmed during recovery".into(),
            },
        };
        DraftSavedResult {
            draft_id: request.draft_id().into(),
            revision: saved.revision().into(),
            reference: reference.name().into(),
            content_hash: saved.frozen().content_hash.clone(),
            delivery,
        }
    };
    let record = DraftSavedRecorded {
        result: result.clone(),
        request_body: request.body().clone(),
        p3_request: request.metadata().clone(),
        p3_receipt: RequestReceipt {
            request_id: request.metadata().request_id.clone(),
            body_hash: request.metadata().body_hash.clone(),
            result_ref: result,
        },
    };
    let event = NewEvent {
        eid: request_event_eid(&request.metadata().request_id, "draft.saved")?,
        host: request.body().host.clone(),
        actor: request.body().actor.clone(),
        kind: "draft.saved".into(),
        layer: "L1".into(),
        obj: request.draft_id().into(),
        guards: vec![],
        data: serde_json::to_value(record).map_err(|e| e.to_string())?,
    };
    decode_saved(catalog.config.project_id(), &event)?;
    Ok(PreparedDraftSaved {
        request: request.clone(),
        event,
    })
}

fn coordinate_saved(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    saved: &SavedDraft,
    materialized: Option<&DraftMaterialization>,
    now: u64,
) -> Result<RecordedDraftSaveReceipt, String> {
    let coordinate = (|| {
        if let Some(receipt) =
            ProjectSession::retry_draft_save_shared(session, catalog, outbox, saved.request(), now)?
        {
            return Ok(receipt);
        }
        let prepared = prepared(catalog, saved, materialized)?;
        ProjectSession::record_draft_saved_shared(session, catalog, outbox, &prepared, now)
    })();
    coordinate.map_err(|reason| uncertain_request(&saved.request().metadata().request_id, reason))
}

/// Owns the complete Submit/Edit path. Callers provide a typed original request,
/// never a client FrozenDraft, raw result, Event or successful receipt.
pub fn save_draft_shared(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    writer: &SnapshotWriter,
    request: &PreparedDraftSaveRequest,
    mut now: impl FnMut() -> Result<u64, String>,
) -> Result<DraftSaveReceipt, String> {
    // Reject a malformed destination before any request coordination. After
    // this point an earlier attempt may already have materialized/been accepted.
    crate::repository_writer::RepositoryRef::draft(request.draft_id())?;
    let request_id = &request.metadata().request_id;
    if let Some(receipt) = ProjectSession::retry_draft_save_shared(
        session,
        catalog,
        outbox,
        request,
        now().map_err(|reason| uncertain_request(request_id, reason))?,
    )? {
        return Ok(outcome(catalog, receipt));
    }
    let existing = read_draft(catalog, request.draft_id())
        .map_err(|error| uncertain_request(request_id, error))?;
    if let Some(saved) = &existing {
        if saved.request_id() == request.metadata().request_id {
            if saved.request() != request {
                return Err("request_id materialized with a different original body".into());
            }
            return Ok(outcome(
                catalog,
                coordinate_saved(
                    session,
                    catalog,
                    outbox,
                    saved,
                    None,
                    now().map_err(|reason| uncertain_request(request_id, reason))?,
                )?,
            ));
        }
    }
    let frozen = match &request.body().action {
        DraftSaveAction::SubmitBaseline { submit } => {
            if existing.is_some() {
                return Err("baseline submit conflict: draft already exists".into());
            }
            let completed = crate::coldstart_evidence::read_completed(
                catalog,
                &outbox.read_inputs()?,
                &submit.scan_id,
            )?
            .ok_or("baseline submit requires a complete accepted coldstart")?;
            freeze_for_save(catalog, &from_scan(catalog, &completed, submit.clone())?)?
        }
        DraftSaveAction::EditDraft {
            expected_revision,
            edit,
            ..
        } => {
            let saved = existing.ok_or("draft edit conflict: draft ref is absent")?;
            if saved.revision() != expected_revision {
                return Err("draft edit conflict: expected revision changed".into());
            }
            // A tip cannot be overwritten until its original save has a durable
            // receipt. Therefore current refs suffice to recover unfinished saves.
            coordinate_saved(
                session,
                catalog,
                outbox,
                &saved,
                None,
                now().map_err(|reason| {
                    uncertain_request(&saved.request().metadata().request_id, reason)
                })?,
            )?;
            freeze_for_save(
                catalog,
                &apply_edit_at(&catalog.root, &saved.frozen().bundle, edit)?,
            )?
        }
    };
    let actual = writer
        .save_draft(session, catalog, &frozen, request, &mut now)
        .map_err(|e| e.to_string())?;
    let record = (|| {
        let saved = read_draft(catalog, request.draft_id())
            .map_err(|error| error.to_string())?
            .ok_or("materialized draft ref disappeared before recording")?;
        if saved.request() != request {
            return Err("materialized draft original request changed before recording".into());
        }
        let receipt = coordinate_saved(session, catalog, outbox, &saved, Some(&actual), now()?)?;
        Ok(outcome(catalog, receipt))
    })();
    record.map_err(|reason: String| uncertain_request(request_id, reason))
}

#[cfg(test)]
#[path = "draft_save_tests.rs"]
mod tests;
