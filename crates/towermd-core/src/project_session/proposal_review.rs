//! Exact actual Proposal subject for the existing baseline review/writer path.
//! A saved ref, serialized View or confirmed-only Event cannot mint this value.
use super::{ProjectSession, SnapshotSessionId};
use crate::{
    baseline_review::{review_binding, ReviewBinding},
    catalog::Catalog,
    hub_protocol::Event,
    proposals,
    recovery_outbox::{self, RecoveryInputs},
    snapshot_service::read_draft,
};
use std::{path::PathBuf, sync::Mutex};

pub(crate) struct VerifiedProposalSubject {
    event: Event,
    binding: ReviewBinding,
    common: PathBuf,
    authority: String,
    configuration: String,
    session: SnapshotSessionId,
    drafts: String,
}
impl VerifiedProposalSubject {
    pub(crate) fn capture_shared(
        session: &Mutex<ProjectSession>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        binding: &ReviewBinding,
        now: u64,
    ) -> Result<Self, String> {
        if binding.target_baseline == "towermd/B0" {
            return Err("initial B0 is not a Proposal subject".into());
        }
        let drafts = crate::snapshot_service::draft_ref_fingerprint(catalog)
            .map_err(|error| error.to_string())?;
        let saved = read_draft(catalog, &binding.object_id)
            .map_err(|e| e.to_string())?
            .ok_or("Proposal review draft ref is absent")?;
        if review_binding(saved.frozen(), saved.revision()) != *binding {
            return Err("Proposal viewed binding differs from current saved draft".into());
        }
        let event = ProjectSession::read_proposal_shared(
            session,
            catalog,
            inputs,
            &binding.object_id,
            &binding.draft_revision,
            now,
        )?
        .ok_or("Proposal review requires its actual nonstale accepted outer event")?;
        let record = proposals::decode_created(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&event),
        )?;
        proposals::require_saved(&record, &saved)?;
        let identity = ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
            let actual = live
                .baseline_requests
                .as_ref()
                .ok_or("Proposal request history is not prepared")?
                .accepted_proposal(&binding.object_id, &binding.draft_revision)?;
            if actual.as_ref() != Some(&event) {
                return Err("Proposal actual role changed while preparing review".into());
            }
            Ok::<_, String>(live.snapshot_session_id())
        })
        .map_err(|e| e.to_string())??;
        Ok(Self {
            event,
            binding: binding.clone(),
            common: catalog.common_dir.clone(),
            authority: catalog.revision.clone(),
            configuration: catalog.config.to_yaml(),
            session: identity,
            drafts,
        })
    }
    pub(crate) fn check_shared(
        &self,
        session: &Mutex<ProjectSession>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        now: u64,
    ) -> Result<(), String> {
        if self.common != catalog.common_dir
            || self.authority != catalog.revision
            || self.configuration != catalog.config.to_yaml()
        {
            return Err("Proposal review authority/project/common/configuration changed".into());
        }
        let saved = read_draft(catalog, &self.binding.object_id)
            .map_err(|e| e.to_string())?
            .ok_or("Proposal review draft ref disappeared")?;
        if review_binding(saved.frozen(), saved.revision()) != self.binding {
            return Err("Proposal draft revision/content/target changed before review".into());
        }
        let record = proposals::decode_created(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&self.event),
        )?;
        proposals::require_saved(&record, &saved)?;
        ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
            let actual = live
                .baseline_requests
                .as_ref()
                .ok_or("Proposal request history is not prepared")?
                .accepted_proposal(&self.binding.object_id, &self.binding.draft_revision)?;
            self.check_live(live)?;
            if live.snapshot_session_id() != self.session || actual.as_ref() != Some(&self.event) {
                return Err("Proposal original acceptance/Session changed before review".into());
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?
    }
    pub(crate) fn check_live(&self, live: &ProjectSession) -> Result<(), String> {
        if live.snapshot_session_id() != self.session
            || live
                .verified_sources
                .as_ref()
                .is_none_or(|sources| sources.0 != self.authority || sources.3 != self.drafts)
            || live.identity.as_ref()
                != Some(&(self.binding.project_id.clone(), self.common.clone()))
        {
            return Err("Proposal source/Session changed before the final approval append".into());
        }
        let actual = live
            .baseline_requests
            .as_ref()
            .ok_or("Proposal ledger is not prepared")?
            .accepted_proposal(&self.binding.object_id, &self.binding.draft_revision)?;
        if actual.as_ref() != Some(&self.event) {
            return Err("Proposal actual original changed before approval append".into());
        }
        Ok(())
    }
}
