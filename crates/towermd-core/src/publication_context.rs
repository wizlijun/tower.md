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
    let mut reads = PublicationReads::new(catalog)?;
    let history = fixed_source_with_reads(catalog, commit, &mut reads)?;
    reads.probe(catalog)?;
    Ok(history)
}

pub(super) fn fixed_source_with_reads(
    catalog: &Catalog,
    commit: &str,
    reads: &mut PublicationReads,
) -> Result<VerifiedRecoveryHistory, String> {
    let saved = reads.snapshot(catalog, commit)?;
    let sources = snapshot_service::SnapshotSources {
        main: Some(saved),
        private: None,
        private_revision: None,
    };
    read_history_sources_with_reads(catalog, &[], &[], &[], sources, || Ok(()), reads)
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
        let mut reads = PublicationReads::new(catalog)?;
        self.check_with_reads(catalog, parent, binding, &mut reads)?;
        reads.probe(catalog)
    }

    pub(super) fn check_with_reads(
        &self,
        catalog: &Catalog,
        parent: &str,
        binding: &contract::baseline::ReviewBinding,
        reads: &mut PublicationReads,
    ) -> Result<(), String> {
        if self.schema != 1
            || self.original.stale
            || !ancestor(catalog, &self.source_commit, parent)?
        {
            return Err("publication Proposal source is not its actual durable ancestor".into());
        }
        let key =
            contract::canonical_hash(&json!({"context":self,"binding":binding,"parent":parent}))?;
        if reads.begin_proposal_context(catalog, &key)? {
            // The real ancestry predicate above and both owner probes remain
            // current even when the immutable complete node is reused.
            return reads.probe(catalog);
        }
        let result = (|| {
            let history = fixed_source_with_reads(catalog, &self.source_commit, reads)?;
            let outer = history.actual_outer_events(catalog)?;
            if !outer.contains(&&self.original) {
                return Err("publication Proposal has no complete actual source prefix".into());
            }
            proposals::validate_actual_history(catalog, &history, reads)?;
            let record = proposals::decode_created(
                catalog.config.project_id(),
                &crate::recovery_outbox::original_of(&self.original),
            )?;
            let saved = reads.saved_draft(
                catalog,
                catalog,
                &binding.object_id,
                &binding.draft_revision,
            )?;
            proposals::require_saved(&record, &saved)?;
            if crate::baseline_review::review_binding(saved.frozen(), saved.revision()) != *binding
            {
                return Err("publication Proposal differs from original approved binding".into());
            }
            let prior = parent_label(&binding.target_baseline)?
                .ok_or("Proposal cannot publish initial B0")?;
            if record.result.binding.baseline != prior {
                return Err("publication Proposal parent baseline changed".into());
            }
            Ok(())
        })();
        reads.finish_proposal_context(catalog, &key, result)
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
    let mut reads = PublicationReads::new(catalog)?;
    let plan = publication_recheck_plan_with_reads(catalog, proof, source_commit, &mut reads)?;
    reads.probe(catalog)?;
    Ok(plan)
}

fn publication_recheck_plan_with_reads(
    catalog: &Catalog,
    proof: &VerifiedPublication,
    source_commit: &str,
    reads: &mut PublicationReads,
) -> Result<PublicationRecheckPlan, String> {
    let history = fixed_source_with_reads(catalog, source_commit, reads)?;
    let selected = history
        .sources
        .clone()
        .select()?
        .ok_or("publication population lacks its exact durable Q")?;
    let through_seq = selected.snapshot.through_seq;
    let tasks = history.task_population_at(catalog, through_seq)?;
    let (created, facts, prepared_seq) =
        publication_population_facts_with_reads(catalog, proof, &tasks, reads)?;
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
    let mut reads = PublicationReads::new(catalog)?;
    let facts = publication_population_facts_with_reads(catalog, proof, tasks, &mut reads)?;
    reads.probe(catalog)?;
    Ok(facts)
}

fn publication_population_facts_with_reads(
    catalog: &Catalog,
    proof: &VerifiedPublication,
    tasks: &BTreeMap<String, Task>,
    reads: &mut PublicationReads,
) -> Result<(NewEvent, Vec<RecheckFact>, u64), String> {
    let created = baseline_created_event(proof)?;
    let commit = created.data["commit"]
        .as_str()
        .ok_or("created lacks canonical publication commit")?;
    let metadata = check_publication_payload_with_reads(
        catalog,
        &mut HistoryCandidate {
            originals: BTreeMap::new(),
        },
        commit,
        reads,
    )?;
    let parents = reads.parents(catalog, commit)?;
    let parent = parents
        .split_whitespace()
        .nth(1)
        .ok_or("publication parent missing")?;
    let before = reads.catalog(catalog, parent)?;
    let saved = reads.saved_draft(
        catalog,
        catalog,
        &metadata.manifest_proof.binding.object_id,
        &metadata.manifest_proof.binding.draft_revision,
    )?;
    let impact = proposals::analyze_verified_publication_tasks(
        &before,
        saved.frozen(),
        tasks,
        proof,
        catalog,
        reads,
    )?;
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
    let mut reads = PublicationReads::new(catalog)?;
    let plan = source_before_created_with_reads(catalog, history, proof, actual, &mut reads)?;
    reads.probe(catalog)?;
    Ok(plan)
}

fn source_before_created_with_reads(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    proof: &VerifiedPublication,
    actual: &Event,
    reads: &mut PublicationReads,
) -> Result<PublicationRecheckPlan, String> {
    validate_binding(catalog, history)?;
    reads.probe(catalog)?;
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
        let Ok(saved) = reads.snapshot(catalog, commit) else {
            continue;
        };
        if saved.snapshot.through_seq != q {
            continue;
        }
        let plan = publication_recheck_plan_with_reads(catalog, proof, commit, reads)?;
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
#[cfg(test)]
pub(super) fn validate_required_history<'a>(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    actuals: impl Iterator<Item = &'a Event>,
) -> Result<(), String> {
    let mut reads = PublicationReads::new(catalog)?;
    validate_required_history_with_reads(catalog, history, actuals, &mut reads)?;
    reads.probe(catalog)
}

pub(super) fn validate_required_history_with_reads<'a>(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    actuals: impl Iterator<Item = &'a Event>,
    reads: &mut PublicationReads,
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
                let proof = prove_publication_with_reads(catalog, history, commit, reads)?;
                let plan = publication_recheck_plan_with_reads(
                    catalog,
                    &proof,
                    &record.source_commit,
                    reads,
                )?;
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
    // One owner for this complete hydration only. Every publication, accepted
    // original and Q is still independently verified; only immutable Git reads
    // and completed Proposal predicate nodes share this finite operation.
    let mut reads = PublicationReads::new(catalog)?;
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
        let proof = prove_publication_with_reads(catalog, history, &commit, &mut reads)?;
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
            Some(source) => publication_recheck_plan_with_reads(
                catalog, &proof, &source, &mut reads,
            )
            .and_then(|plan| {
                check_created(&plan, actual)?;
                Ok(plan)
            }),
            None => source_before_created_with_reads(catalog, history, &proof, actual, &mut reads),
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
    reads.probe(catalog)?;
    Ok((blocks, unavailable))
}

#[cfg(test)]
pub(super) fn exercise_completed_proposal_context_for_test(catalog: &Catalog, commit: &str) {
    let mut reads = PublicationReads::new(catalog).unwrap();
    let (metadata, _) = read_publication_metadata_with_reads(catalog, commit, &mut reads).unwrap();
    let context = metadata
        .manifest_proof
        .proposal_context
        .as_ref()
        .expect("actual later baseline Proposal context");
    let binding = &metadata.manifest_proof.binding;
    let parents = reads.parents(catalog, commit).unwrap();
    let parent = parents.split_whitespace().nth(1).unwrap();
    let key =
        contract::canonical_hash(&json!({"context":context,"binding":binding,"parent":parent}))
            .unwrap();
    for _ in 0..2 {
        context
            .check_with_reads(catalog, parent, binding, &mut reads)
            .unwrap();
    }
    assert_eq!(
        reads.counts().get(&(key.clone(), "proposal_context")),
        Some(&1)
    );

    for kind in ["saved_draft", "frozen_source"] {
        let counts = reads.counts();
        let static_reads = counts
            .iter()
            .filter(|((_, actual), _)| *actual == kind)
            .collect::<Vec<_>>();
        assert!(!static_reads.is_empty());
        assert!(static_reads.iter().all(|(_, count)| **count == 1));
    }
    exercise_static_proposal_reads(catalog, binding);

    // A complete original Event change must not hit another node's success.
    // The fixed source still has only its actual, unchanged original outer.
    let mut changed = context.clone();
    changed.original.data["x-not-in-actual-source"] = json!(true);
    for _ in 0..2 {
        assert!(changed
            .check_with_reads(catalog, parent, binding, &mut reads)
            .is_err());
    }
    let changed_key =
        contract::canonical_hash(&json!({"context":changed,"binding":binding,"parent":parent}))
            .unwrap();
    assert_eq!(
        reads.counts().get(&(changed_key, "proposal_context")),
        Some(&2),
        "failed nodes cannot be cached; active state must be removed"
    );

    let mut changed_binding = binding.clone();
    changed_binding.content_hash = "0".repeat(64);
    assert!(context
        .check_with_reads(catalog, parent, &changed_binding, &mut reads)
        .is_err());
    // A different descendant of Q is a separate complete predicate node. The
    // outer publication still checks its own exact parent independently.
    assert_ne!(commit, parent);
    context
        .check_with_reads(catalog, commit, binding, &mut reads)
        .unwrap();
    let parent_key =
        contract::canonical_hash(&json!({"context":context,"binding":binding,"parent":commit}))
            .unwrap();
    assert_ne!(key, parent_key);
    assert_eq!(
        reads.counts().get(&(parent_key, "proposal_context")),
        Some(&1)
    );

    let reference = format!("refs/tags/{}", binding.target_baseline);
    git(catalog, &["update-ref", &reference, &context.source_commit]).unwrap();
    assert!(
        context
            .check_with_reads(catalog, parent, binding, &mut reads)
            .is_err(),
        "cached complete node must still see actual tag drift"
    );
    assert!(reads
        .saved_draft(
            catalog,
            catalog,
            &binding.object_id,
            &binding.draft_revision
        )
        .is_err());
    git(catalog, &["update-ref", &reference, commit]).unwrap();
    context
        .check_with_reads(catalog, parent, binding, &mut reads)
        .unwrap();

    // A static value hit must still reject current private/draft source drift.
    let original_private = crate::snapshot_git::private_revision(&catalog.root).unwrap();
    let alternate = if original_private.as_deref() == Some(context.source_commit.as_str()) {
        commit
    } else {
        &context.source_commit
    };
    git(catalog, &["update-ref", "refs/towermd/snapshot", alternate]).unwrap();
    assert!(reads
        .saved_draft(
            catalog,
            catalog,
            &binding.object_id,
            &binding.draft_revision
        )
        .is_err());
    match original_private {
        Some(original) => {
            git(catalog, &["update-ref", "refs/towermd/snapshot", &original]).unwrap();
        }
        None => {
            git(catalog, &["update-ref", "-d", "refs/towermd/snapshot"]).unwrap();
        }
    }
    let draft_ref = format!("refs/towermd/drafts/{}", binding.object_id);
    git(catalog, &["update-ref", &draft_ref, &context.source_commit]).unwrap();
    assert!(reads
        .saved_draft(
            catalog,
            catalog,
            &binding.object_id,
            &binding.draft_revision
        )
        .is_err());
    git(
        catalog,
        &["update-ref", &draft_ref, &binding.draft_revision],
    )
    .unwrap();
    context
        .check_with_reads(catalog, parent, binding, &mut reads)
        .unwrap();

    let mut fresh = PublicationReads::new(catalog).unwrap();
    context
        .check_with_reads(catalog, parent, binding, &mut fresh)
        .unwrap();
    assert_eq!(
        fresh.counts().get(&(key.clone(), "proposal_context")),
        Some(&1),
        "no success crosses the owner boundary"
    );
    for kind in ["saved_draft", "frozen_source"] {
        let counts = fresh.counts();
        let reads = counts
            .iter()
            .filter(|((_, actual), _)| *actual == kind)
            .collect::<Vec<_>>();
        assert!(
            !reads.is_empty(),
            "the fresh owner must run the real static reader"
        );
        assert!(reads.iter().all(|(_, count)| **count == 1));
    }
    let mut active = PublicationReads::new(catalog).unwrap();
    assert!(!active.begin_proposal_context(catalog, &key).unwrap());
    assert!(context
        .check_with_reads(catalog, parent, binding, &mut active)
        .unwrap_err()
        .contains("cyclic"));
    assert!(active
        .finish_proposal_context(
            catalog,
            &key,
            Err("test interrupted incomplete node".into())
        )
        .is_err());
    context
        .check_with_reads(catalog, parent, binding, &mut active)
        .unwrap();
}

#[cfg(test)]
fn exercise_static_proposal_reads(catalog: &Catalog, binding: &contract::baseline::ReviewBinding) {
    // Same real B1 fixture, but no warmed context can conceal a distinct key.
    let mut owner = PublicationReads::new(catalog).unwrap();
    let reads = &mut owner;
    let count = |reads: &PublicationReads, kind| {
        reads
            .counts()
            .iter()
            .filter(|((_, actual), _)| *actual == kind)
            .map(|(_, count)| *count)
            .sum::<usize>()
    };
    let saved = reads
        .saved_draft(
            catalog,
            catalog,
            &binding.object_id,
            &binding.draft_revision,
        )
        .unwrap();
    let first = count(reads, "saved_draft");
    let repeated = reads
        .saved_draft(
            catalog,
            catalog,
            &binding.object_id,
            &binding.draft_revision,
        )
        .unwrap();
    assert_eq!(repeated.revision(), saved.revision());
    assert_eq!(repeated.request(), saved.request());
    assert_eq!(repeated.frozen(), saved.frozen());
    assert_eq!(count(reads, "saved_draft"), first);

    // A distinct, actual historical Catalog is a distinct immutable read key.
    let source = Catalog::at_revision(&catalog.root, &saved.frozen().bundle.source_revision)
        .unwrap()
        .unwrap();
    assert_ne!(source.revision, catalog.revision);
    reads
        .saved_draft(
            catalog,
            &source,
            &binding.object_id,
            &binding.draft_revision,
        )
        .unwrap();
    assert_eq!(count(reads, "saved_draft"), first + 1);
    reads
        .saved_draft(
            catalog,
            &source,
            &binding.object_id,
            &binding.draft_revision,
        )
        .unwrap();
    assert_eq!(count(reads, "saved_draft"), first + 1);

    // Same real tree under another ID/revision never borrows the warmed draft.
    for (id, revision) in [
        ("other-proposal", binding.draft_revision.as_str()),
        (binding.object_id.as_str(), catalog.revision.as_str()),
    ] {
        let before = count(reads, "saved_draft");
        for _ in 0..2 {
            assert!(reads.saved_draft(catalog, catalog, id, revision).is_err());
        }
        assert_eq!(
            count(reads, "saved_draft"),
            before + 2,
            "failed static reads are not cached"
        );
    }
    let frozen = reads
        .frozen_source(catalog, &source, &saved.frozen().bundle)
        .unwrap();
    assert_eq!(&frozen, saved.frozen());
    let first = count(reads, "frozen_source");
    assert_eq!(
        reads
            .frozen_source(catalog, &source, &saved.frozen().bundle)
            .unwrap(),
        frozen
    );
    assert_eq!(count(reads, "frozen_source"), first);
    // The complete bundle matters even when all object bytes and Git OIDs match.
    let mut changed = saved.frozen().bundle.clone();
    changed
        .questions
        .push("another complete immutable subject".into());
    let different = reads.frozen_source(catalog, &source, &changed).unwrap();
    assert_ne!(different, frozen);
    assert_ne!(different.content_hash, frozen.content_hash);
    assert_eq!(count(reads, "frozen_source"), first + 1);
    changed.canvas_text = "{invalid actual canvas".into();
    let before = count(reads, "frozen_source");
    for _ in 0..2 {
        assert!(reads.frozen_source(catalog, &source, &changed).is_err());
    }
    assert_eq!(count(reads, "frozen_source"), before + 2);
    // Historical source revision is never replaced by the owner's current main.
    let before = count(reads, "frozen_source");
    for _ in 0..2 {
        assert!(reads
            .frozen_source(catalog, catalog, &saved.frozen().bundle)
            .is_err());
    }
    assert_eq!(count(reads, "frozen_source"), before + 2);
}
