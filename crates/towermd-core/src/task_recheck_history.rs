//! Purpose-specific proof for App Task recheck. Source Q is an actual Git
//! snapshot and S must be its immediate next original Event; no caller vectors
//! or restored inner events enter the global actual/Human/Run schedule.
use super::*;
use crate::{
    hub_protocol::Lease,
    proposals::{self, TaskRecheckView},
    snapshot::StateProjection,
};

// A finite receipt proof owned only by its actual history. No constructor,
// deserializer, Clone implementation or Session field exposes this result.
pub(super) struct VerifiedTaskRecheckOriginals {
    verified: Vec<Event>,
    originals: Vec<Event>,
    sources: Option<SourceFingerprint>,
}

type SourceFingerprint = (std::path::PathBuf, String, String, Option<String>, String);

fn source_fingerprint(catalog: &Catalog) -> Result<SourceFingerprint, String> {
    crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?;
    Ok((
        catalog.common_dir.clone(),
        catalog.config.to_yaml(),
        crate::catalog::authority_revision(&catalog.root)?,
        crate::snapshot_git::private_revision(&catalog.root)?,
        snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?,
    ))
}

impl VerifiedTaskRecheckOriginals {
    pub(super) fn probe(&self, catalog: &Catalog) -> Result<(), String> {
        if let Some(sources) = &self.sources {
            if &source_fingerprint(catalog)? != sources {
                return Err(
                    "Task recheck sources/approval refs changed during history proof".into(),
                );
            }
        } else if !self.verified.is_empty() || !self.originals.is_empty() {
            return Err("Task recheck original proof lacks its actual source fingerprint".into());
        }
        Ok(())
    }
}

pub(crate) fn recheck_source_view(
    catalog: &Catalog,
    commit: &str,
    seq: u64,
    target: &str,
    task: &str,
    now: u64,
    request: &proposals::RecheckRequest,
) -> Result<TaskRecheckView, String> {
    #[cfg(test)]
    crate::snapshot_service::own_publication_boundary(
        &catalog.root,
        "BeforeTaskRecheckHistoricalSourceProof",
    )?;
    require_recheck_source_reachable(catalog, commit)?;
    let history = publication_context::fixed_source(catalog, commit)?;
    recheck_view_in_source(catalog, commit, seq, target, task, now, request, &history)
}
fn require_recheck_source_reachable(catalog: &Catalog, commit: &str) -> Result<(), String> {
    let private = crate::snapshot_git::private_revision(&catalog.root)?;
    let reachable = ancestor(catalog, commit, &catalog.revision)?
        || match private {
            Some(ref revision) => ancestor(catalog, commit, revision)?,
            None => false,
        };
    if !reachable {
        return Err("Task recheck Q is outside actual main/private source ancestry".into());
    }
    Ok(())
}
// The extra owner is the same finite read scope, never an extra grant.
#[allow(
    clippy::too_many_arguments,
    reason = "the exact source subject and its finite read scope remain explicit"
)]
fn recheck_source_view_with_reads(
    catalog: &Catalog,
    commit: &str,
    seq: u64,
    target: &str,
    task: &str,
    now: u64,
    request: &proposals::RecheckRequest,
    reads: &mut PublicationReads,
) -> Result<TaskRecheckView, String> {
    require_recheck_source_reachable(catalog, commit)?;
    let history = publication_context::fixed_source_with_reads(catalog, commit, reads)?;
    recheck_view_in_source(catalog, commit, seq, target, task, now, request, &history)
}
// This private helper consumes only the locally read complete source. It never
// returns a verified history, imports its roles or grants a live lease.
#[allow(
    clippy::too_many_arguments,
    reason = "the complete recheck subject and actual source remain explicit"
)]
fn recheck_view_in_source(
    catalog: &Catalog,
    commit: &str,
    seq: u64,
    target: &str,
    task: &str,
    now: u64,
    request: &proposals::RecheckRequest,
    history: &VerifiedRecoveryHistory,
) -> Result<TaskRecheckView, String> {
    let saved = history
        .sources
        .clone()
        .select()?
        .ok_or("Task recheck source snapshot missing")?;
    if saved.commit != commit || saved.snapshot.through_seq != seq {
        return Err("Task recheck source is not its exact durable Q".into());
    }
    let mut projection = StateProjection::default();
    let mut leases = BTreeMap::<String, Lease>::new();
    let mut baseline = None;
    for event in history.accepted_outer_events(catalog, seq)? {
        if !event.stale {
            if event.kind == "hub.rebuilt" {
                leases.clear();
            }
            for lease in event.updated_leases() {
                leases.insert(lease.name.clone(), lease.clone());
            }
            for name in event.released_lease_names() {
                leases.remove(&name);
            }
            if event.kind == "baseline.created" {
                baseline = event.data["target"].as_str().map(str::to_owned);
            }
        }
        projection.apply_recorded(event)?;
    }
    for kind in ["Task", "Run"] {
        if projection.objects.get(kind).cloned().unwrap_or_default()
            != saved
                .snapshot
                .projection
                .objects
                .get(kind)
                .cloned()
                .unwrap_or_default()
        {
            return Err(format!(
                "Task recheck actual {kind} source differs from checkpoint"
            ));
        }
    }
    if baseline.as_deref() != Some(target) {
        return Err("Task recheck source does not prove its exact current baseline".into());
    }
    let proof = saved.snapshot.task_recheck_proof()?;
    proof.require_request(request)?;
    if proof.subject.binding.task_id != task || proof.subject.binding.target_baseline != target {
        return Err("Task recheck Q proof belongs to another original request/subject".into());
    }
    if let Some(running) = &proof.subject.binding.running {
        for fence in &running.fences {
            // This is a local historical subject map, never a restored Hub
            // lease. Require the original archived grant/fence before using Q's
            // exact expiry captured by the sole writer from the actual head.
            let lease = leases
                .get_mut(&fence.name)
                .ok_or("Task recheck Q lacks original historical Run grant")?;
            if lease.fence() != *fence {
                return Err("Task recheck Q expiry cannot replace an original Run fence".into());
            }
            lease.expires_at = running.lease_expires_at[&fence.name];
        }
    }
    let view = proposals::recheck_view(&projection, &leases, target, task, now)?;
    if view != proof.subject {
        return Err("Task recheck Q audit proof differs from complete historical subject".into());
    }
    Ok(view)
}
fn validate_resolution_once(
    catalog: &Catalog,
    event: &Event,
    verified: &mut Vec<Event>,
    reads: &mut PublicationReads,
) -> Result<(), String> {
    let record = proposals::decode_resolved(
        catalog.config.project_id(),
        &crate::recovery_outbox::original_of(event),
    )?;
    crate::recovery_outbox::validate_actual(&crate::recovery_outbox::original_of(event), event)?;
    if !event.stale && record.source_seq.checked_add(1) != Some(event.seq) {
        return Err("Task recheck original is not S=Q+1".into());
    }
    let now = if event.stale {
        0
    } else {
        crate::hub::event_time(event).map_err(|e| e.to_string())?
    };
    let request = proposals::RecheckRequest::from_recorded(
        record.request_body.clone(),
        record.p3_request.clone(),
    )?;
    // Every row retains its typed body, full actual envelope, adjacent S/Q and
    // original time checks. Only an identical complete original can share this
    // one history construction's already successful Git source proof.
    if verified.contains(event) {
        return Ok(());
    }
    let actual = recheck_source_view_with_reads(
        catalog,
        &record.source_commit,
        record.source_seq,
        &record.subject.binding.target_baseline,
        &event.obj,
        now,
        &request,
        reads,
    )?;
    if actual != record.subject {
        return Err("Task recheck differs from the complete actual source Q subject".into());
    }
    verified.push(event.clone());
    Ok(())
}

/// Only this actual history constructor can seal the finite original proofs.
/// Nested fixed-source histories always start independent and unsealed.
pub(super) fn validate_history_with_reads<'a>(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    actuals: impl Iterator<Item = &'a Event>,
    reads: &mut PublicationReads,
) -> Result<VerifiedTaskRecheckOriginals, String> {
    let mut verified = Vec::new();
    let mut sources = None;
    let mut validate = |event: &Event| {
        if sources.is_none() {
            sources = Some(source_fingerprint(catalog)?);
        }
        validate_resolution_once(catalog, event, &mut verified, reads)
    };
    for event in actuals {
        validate(event)?;
    }
    let originals = collect_originals(catalog, history, &mut validate)?;
    validate_binding(catalog, history)?;
    let proof = VerifiedTaskRecheckOriginals {
        verified,
        originals,
        sources,
    };
    proof.probe(catalog)?;
    Ok(proof)
}

pub(crate) fn task_recheck_originals(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
) -> Result<Vec<Event>, String> {
    let proof = history
        .recheck_receipts
        .as_ref()
        .ok_or("Task recheck history original proof is not sealed")?;
    proof.probe(catalog)?;
    let mut reads = PublicationReads::new(catalog)?;
    let mut verified = proof.verified.clone();
    let originals = collect_originals(catalog, history, |event| {
        validate_resolution_once(catalog, event, &mut verified, &mut reads)
    })?;
    if verified != proof.verified || originals != proof.originals {
        return Err("Task recheck history differs from its sealed complete originals".into());
    }
    proof.probe(catalog)?;
    reads.probe(catalog)?;
    Ok(originals)
}

fn collect_originals(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    mut validate: impl FnMut(&Event) -> Result<(), String>,
) -> Result<Vec<Event>, String> {
    validate_binding(catalog, history)?;
    let mut originals = BTreeMap::new();
    for event in history
        .candidate
        .originals
        .values()
        .filter(|event| event.kind == "task.recheck_resolved")
    {
        validate(event)?;
        if !event.stale {
            originals.insert(event.eid.clone(), event.clone());
        }
    }
    let outer = history.actual_outer_events(catalog)?;
    for event in &outer {
        if let Some(original) = proposals::restored_resolution(catalog.config.project_id(), event)?
        {
            validate(&original)?;
            if !outer.iter().any(|barrier| {
                !barrier.stale
                    && barrier.kind == "hub.rebuilt"
                    && barrier.seq > original.seq
                    && barrier.seq < event.seq
            }) {
                return Err("Task recheck receipt audit lacks its actual rebuild barrier".into());
            }
            if originals
                .insert(original.eid.clone(), original.clone())
                .is_some_and(|prior| prior != original)
            {
                return Err("Task recheck original receipt conflict".into());
            }
        }
    }
    Ok(originals.into_values().collect())
}

pub(crate) fn validate_recheck_intent(
    catalog: &Catalog,
    event: &crate::hub_protocol::NewEvent,
) -> Result<(), String> {
    let record = proposals::decode_resolved(catalog.config.project_id(), event)?;
    let request = proposals::RecheckRequest::from_recorded(
        record.request_body.clone(),
        record.p3_request.clone(),
    )?;
    let subject = recheck_source_view(
        catalog,
        &record.source_commit,
        record.source_seq,
        &record.subject.binding.target_baseline,
        &event.obj,
        0,
        &request,
    )?;
    if subject != record.subject {
        return Err("Task recheck pending intent differs from actual original Q".into());
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn validate_resolution_rows_for_test(
    catalog: &Catalog,
    actuals: &[Event],
) -> Result<(), String> {
    let mut reads = PublicationReads::new(catalog)?;
    let mut verified = Vec::new();
    for event in actuals {
        validate_resolution_once(catalog, event, &mut verified, &mut reads)?;
    }
    reads.probe(catalog)
}
