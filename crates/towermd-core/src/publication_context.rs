//! Finite Git-backed Proposal and publication population contexts. These readers
//! never import embedded original events into current accepted roles or grants.
use super::*;
use crate::{
    baseline_draft::FrozenDraft,
    proposals::{self, RecheckFact},
    tasks::Task,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProposalPublicationContext {
    schema: u32,
    source_commit: String,
    #[serde(deserialize_with = "contract::deserialize_event")]
    original: Event,
}

pub(super) fn parent_label(target: &str) -> Result<Option<String>, String> {
    Ok(baseline_number(target)?
        .checked_sub(1)
        .map(|number| format!("towermd/B{number}")))
}

pub(super) fn check_parent(
    catalog: &Catalog,
    manifest: &ManifestPublication,
    parent: Option<&str>,
) -> Result<(), String> {
    let expected = parent_label(&manifest.binding.target_baseline)?;
    if manifest.parent_baseline != expected {
        return Err("publication parent baseline changed".into());
    }
    if let Some(label) = expected {
        let prior = peel_baseline(catalog, &label)?;
        if !ancestor(catalog, &prior, parent.unwrap_or(&catalog.revision))? {
            return Err("publication prior baseline is outside its actual parent chain".into());
        }
    }
    Ok(())
}

pub(super) fn fixed_source(
    catalog: &Catalog,
    commit: &str,
) -> Result<VerifiedRecoveryHistory, String> {
    let saved = snapshot_service::read_snapshot_commit(catalog, commit)?;
    let sources = snapshot_service::SnapshotSources {
        main: Some(saved),
        private: None,
        private_revision: None,
    };
    // Only an actual immutable Git snapshot supplies this source. No JSON tail
    // or confirmed-only row is supplied to the accepted outer schedule.
    read_history_sources(catalog, &[], &[], &[], sources, || Ok(()))
}

impl ProposalPublicationContext {
    pub(super) fn capture(
        catalog: &Catalog,
        history: &VerifiedRecoveryHistory,
        frozen: &FrozenDraft,
        binding: &contract::baseline::ReviewBinding,
    ) -> Result<Option<Self>, String> {
        if baseline_number(&binding.target_baseline)? == 0 {
            return Ok(None);
        }
        let actual = history
            .actual_outer_events(catalog)?
            .into_iter()
            .filter(|event| {
                !event.stale
                    && event.kind == "proposal.created"
                    && event.obj == binding.object_id
                    && event.data["result"]["draft"]["revision"] == binding.draft_revision
            })
            .collect::<Vec<_>>();
        if actual.len() != 1 {
            return Err("publication requires one actual Proposal outer event".into());
        }
        let source = history
            .sources
            .clone()
            .select()?
            .ok_or("publication Proposal must first be saved in a durable snapshot")?;
        let context = Self {
            schema: 1,
            source_commit: source.commit,
            original: actual[0].clone(),
        };
        context.check(catalog, &catalog.revision, binding)?;
        let record = proposals::decode_created(
            catalog.config.project_id(),
            &crate::recovery_outbox::original_of(&context.original),
        )?;
        if record.result.draft.content_hash != frozen.content_hash {
            return Err("publication Proposal frozen hash changed".into());
        }
        Ok(Some(context))
    }

    pub(super) fn check(
        &self,
        catalog: &Catalog,
        parent: &str,
        binding: &contract::baseline::ReviewBinding,
    ) -> Result<(), String> {
        if self.schema != 1
            || self.original.stale
            || !ancestor(catalog, &self.source_commit, parent)?
        {
            return Err("publication Proposal source is not its actual durable ancestor".into());
        }
        let history = fixed_source(catalog, &self.source_commit)?;
        let outer = history.actual_outer_events(catalog)?;
        if !outer.contains(&&self.original) {
            return Err("publication Proposal has no complete actual source prefix".into());
        }
        proposals::validate_actual_history(catalog, &history)?;
        let record = proposals::decode_created(
            catalog.config.project_id(),
            &crate::recovery_outbox::original_of(&self.original),
        )?;
        let saved =
            snapshot_service::read_draft_at(catalog, &binding.object_id, &binding.draft_revision)
                .map_err(|error| error.to_string())?;
        proposals::require_saved(&record, &saved)?;
        if crate::baseline_review::review_binding(saved.frozen(), saved.revision()) != *binding {
            return Err("publication Proposal differs from original approved binding".into());
        }
        let prior =
            parent_label(&binding.target_baseline)?.ok_or("Proposal cannot publish initial B0")?;
        if record.result.binding.baseline != prior {
            return Err("publication Proposal parent baseline changed".into());
        }
        Ok(())
    }
}

pub(crate) fn publication_context_check(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    frozen: &FrozenDraft,
    binding: &contract::baseline::ReviewBinding,
) -> Result<(), String> {
    ProposalPublicationContext::capture(catalog, history, frozen, binding).map(|_| ())
}

pub(crate) struct PublicationRecheckPlan {
    pub(crate) tasks: BTreeMap<String, Task>,
    pub(crate) facts: Vec<RecheckFact>,
    pub(crate) created: NewEvent,
    pub(crate) source_commit: String,
    pub(crate) through_seq: u64,
}

pub(crate) fn publication_recheck_plan(
    catalog: &Catalog,
    proof: &VerifiedPublication,
    source_commit: &str,
) -> Result<PublicationRecheckPlan, String> {
    let history = fixed_source(catalog, source_commit)?;
    let selected = history
        .sources
        .clone()
        .select()?
        .ok_or("publication population lacks its exact durable Q")?;
    let through_seq = selected.snapshot.through_seq;
    let tasks = history.task_population_at(catalog, through_seq)?;
    let (created, facts, prepared_seq) = publication_population_facts(catalog, proof, &tasks)?;
    if history
        .actual_outer_events(catalog)?
        .iter()
        .any(|event| event.kind == "hub.rebuilt" && event.seq > prepared_seq)
    {
        return Err("publication S cannot be inferred from a rebuilt replacement sequence".into());
    }
    Ok(PublicationRecheckPlan {
        tasks,
        facts,
        created,
        source_commit: source_commit.into(),
        through_seq,
    })
}

/// The immutable Git/impact part of a population proof. The own writer combines
/// this with its fully verified Session prefix and its exact produced Q; this
/// helper alone neither certifies a population nor grants an append capability.
pub(crate) fn publication_population_facts(
    catalog: &Catalog,
    proof: &VerifiedPublication,
    tasks: &BTreeMap<String, Task>,
) -> Result<(NewEvent, Vec<RecheckFact>, u64), String> {
    let created = baseline_created_event(proof)?;
    let commit = created.data["commit"]
        .as_str()
        .ok_or("created lacks canonical publication commit")?;
    let metadata = check_publication_payload(
        catalog,
        &mut HistoryCandidate {
            originals: BTreeMap::new(),
        },
        commit,
    )?;
    let parent = git(catalog, &["rev-parse", &format!("{commit}^")])?;
    let before =
        Catalog::at_revision(&catalog.root, &parent)?.ok_or("publication parent lacks Catalog")?;
    let saved = snapshot_service::read_draft_at(
        catalog,
        &metadata.manifest_proof.binding.object_id,
        &metadata.manifest_proof.binding.draft_revision,
    )
    .map_err(|error| error.to_string())?;
    let impact =
        proposals::analyze_verified_publication_tasks(&before, saved.frozen(), tasks, proof)?;
    let old_baseline = metadata
        .manifest_proof
        .parent_baseline
        .clone()
        .ok_or("later publication lacks prior baseline")?;
    let facts = impact
        .tasks
        .iter()
        .map(|task| RecheckFact {
            task: task.clone(),
            old_baseline: old_baseline.clone(),
            new_baseline: metadata.manifest_proof.binding.target_baseline.clone(),
            publication_id: metadata.manifest_proof.publication_id.clone(),
            reasons: vec!["获批准的新基线改变了任务涉及的模块、路径、接口或验收依赖".into()],
        })
        .collect();
    Ok((created, facts, metadata.manifest_proof.prepared_event.seq))
}

fn check_created(plan: &PublicationRecheckPlan, actual: &Event) -> Result<(), String> {
    if actual.stale
        || crate::recovery_outbox::original_of(actual) != plan.created
        || plan.through_seq.checked_add(1) != Some(actual.seq)
    {
        return Err("publication creation lacks its exact adjacent durable Q population".into());
    }
    crate::recovery_outbox::validate_actual(&plan.created, actual)
}

fn source_before_created(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    proof: &VerifiedPublication,
    actual: &Event,
) -> Result<PublicationRecheckPlan, String> {
    validate_binding(catalog, history)?;
    let q = actual
        .seq
        .checked_sub(1)
        .ok_or("publication creation sequence is invalid")?;
    let private = history.snapshot_sources().private_revision.clone();
    let refs = snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?;
    if crate::snapshot_git::private_revision(&catalog.root)? != private {
        return Err("publication Q sources changed before search".into());
    }
    let mut args = vec!["log", "--format=%H", catalog.revision.as_str()];
    if let Some(private) = private.as_deref() {
        args.push(private);
    }
    let mut chosen: Option<(snapshot_service::SavedSnapshot, PublicationRecheckPlan)> = None;
    // Search only real ancestors of the already-proved authority/private tips.
    // A recent snapshot or a matching numeric watermark alone is not Q proof.
    let listing = git(catalog, &args)?;
    for commit in listing.lines().collect::<BTreeSet<_>>() {
        let Ok(saved) = snapshot_service::read_snapshot_commit(catalog, commit) else {
            continue;
        };
        if saved.snapshot.through_seq != q {
            continue;
        }
        let plan = publication_recheck_plan(catalog, proof, commit)?;
        check_created(&plan, actual)?;
        if let Some((prior, _)) = &chosen {
            if !snapshot_service::equivalent(prior, &saved) {
                return Err("publication S has ambiguous non-equivalent durable Q sources; keep tasks blocked".into());
            }
        } else {
            chosen = Some((saved, plan));
        }
    }
    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision
        || crate::snapshot_git::private_revision(&catalog.root)? != private
        || snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())? != refs
    {
        return Err("publication Q sources/approval refs changed during search".into());
    }
    chosen.map(|(_, plan)| plan).ok_or(
        "publication original S population has no actual durable Q; keep tasks blocked".into(),
    )
}

pub(crate) fn publication_recheck_plan_for_created(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    proof: &VerifiedPublication,
    created: &Event,
) -> Result<PublicationRecheckPlan, String> {
    let source = history
        .candidate
        .originals
        .values()
        .filter(|event| event.kind == "task.recheck_required")
        .filter_map(|event| proposals::required_record(event).ok())
        .find(|record| record.created.eid == created.eid)
        .map(|record| record.source_commit);
    if let Some(source) = source {
        let plan = publication_recheck_plan(catalog, proof, &source)?;
        check_created(&plan, created)?;
        Ok(plan)
    } else {
        source_before_created(catalog, history, proof, created)
    }
}

// Plans belong only to this one complete history proof. No accepted role,
// Session cache or nested fixed-source read receives this temporary reuse.
pub(super) fn validate_required_history<'a>(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    actuals: impl Iterator<Item = &'a Event>,
) -> Result<(), String> {
    let mut plans: Vec<(Event, PublicationRecheckPlan)> = Vec::new();
    let mut refs = None;
    for actual in actuals {
        // Every row retains its own complete typed original, project and order
        // checks, even when another Task shares its publication population.
        let record = proposals::required_record(actual)?;
        if record.request_body.project_id != catalog.config.project_id()
            || actual.seq <= record.created.seq
        {
            return Err("task recheck marker project/order changed".into());
        }
        if refs.is_none() {
            refs =
                Some(snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?);
        }
        let found = plans.iter().position(|(created, plan)| {
            *created == record.created && plan.source_commit == record.source_commit
        });
        let index = match found {
            Some(index) => index,
            None => {
                let commit = record.created.data["commit"]
                    .as_str()
                    .ok_or("recheck publication lacks commit")?;
                let proof = prove_publication(catalog, history, commit)?;
                let plan = publication_recheck_plan(catalog, &proof, &record.source_commit)?;
                plans.push((record.created.clone(), plan));
                plans.len() - 1
            }
        };
        let plan = &plans[index].1;
        check_created(plan, &record.created)?;
        if !plan.facts.contains(&record.fact) {
            return Err("task recheck marker differs from actual publication S population".into());
        }
    }
    if let Some(refs) = refs {
        if snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())? != refs {
            return Err("publication recheck approval/tag refs changed during proof".into());
        }
    }
    Ok(())
}

/// Rebuild only admission barriers from actual publication/created/Q proofs.
/// A pending or unknowable S blocks admission without fabricating a Task Event
/// or persisting derived flags in a snapshot before their actual markers exist.
pub(crate) fn hydrate_publication_rechecks(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    through: u64,
) -> Result<(BTreeSet<String>, bool), String> {
    let mut blocks = BTreeSet::new();
    let mut unavailable = false;
    let outer = history.accepted_outer_events(catalog, through)?;
    let mut replay = crate::snapshot::StateProjection::default();
    for event in &outer {
        replay.apply_recorded(event)?;
    }
    for label in git(
        catalog,
        &["for-each-ref", "--format=%(refname)", "refs/tags/towermd/"],
    )?
    .lines()
    {
        let Some(label) = label.strip_prefix("refs/tags/") else {
            continue;
        };
        if !label.starts_with("towermd/B") || baseline_number(label)? == 0 {
            continue;
        }
        let commit = peel_baseline(catalog, label)?;
        let proof = prove_publication(catalog, history, &commit)?;
        let canonical = baseline_created_event(&proof)?;
        let actual = history.publication_created_original(&canonical.eid);
        let Some(actual) = actual else {
            unavailable = true;
            continue;
        };
        let source = outer
            .iter()
            .filter(|event| event.kind == "task.recheck_required")
            .filter_map(|event| proposals::required_record(event).ok())
            .find(|record| record.created.eid == actual.eid)
            .map(|record| record.source_commit);
        let plan = match source {
            Some(source) => publication_recheck_plan(catalog, &proof, &source).and_then(|plan| {
                check_created(&plan, actual)?;
                Ok(plan)
            }),
            None => source_before_created(catalog, history, &proof, actual),
        };
        let Ok(plan) = plan else {
            unavailable = true;
            continue;
        };
        for fact in plan.facts {
            // Unknown/missing markers remain blocked. A verified actual resolution
            // clears only IDs retained in its reconstructible projection cache.
            let resolved = replay
                .rechecks
                .as_ref()
                .is_some_and(|cache| cache.reviewed(&fact.task, &fact.publication_id));
            if !resolved {
                if !outer.iter().any(|event| {
                    !event.stale
                        && event.kind == "task.recheck_required"
                        && proposals::required_record(event).is_ok_and(|record| record.fact == fact)
                }) {
                    unavailable = true;
                }
                blocks.insert(fact.task);
            }
        }
    }
    Ok((blocks, unavailable))
}
