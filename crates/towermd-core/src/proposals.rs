//! Read-only proposal input and impact analysis. No API here saves or approves a change.
#[path = "proposal_payload.rs"]
mod payload;
#[path = "proposal_recheck.rs"]
mod recheck;
#[path = "proposal_recorded.rs"]
mod recorded;
#[path = "task_recheck_recorded.rs"]
mod task_recheck_recorded;
use crate::{
    baseline_current::{self, CurrentBaseline},
    baseline_draft::{self, DraftBundle, DraftObject},
    catalog::{self, Catalog},
    okf::Document,
    recovery_contract,
    recovery_outbox::RecoveryInputs,
    repo,
    snapshot_service::read_draft,
    tasks::Task,
    types::{self, LayerResolution},
};
pub(crate) use payload::{attach_bound_proposal, carrier_object, validate_publication_carrier};
pub use payload::{attach_proposal, validate_proposal_subject, ProposalCarrier};
pub(crate) use recheck::{
    apply_required, decode_required, required_event, required_record, RequireRecheckAction,
};
pub use recheck::{
    task_content_hash, unresolved_publications, RecheckFact, RecheckRunBinding, TaskRecheckAction,
    TaskRecheckBinding, TaskRecheckResolution, TaskRecheckView,
};
pub(crate) use recorded::{decode_created, proposal_input, require_saved, validate_actual_history};
pub use recorded::{ProposalBinding, ProposalCreated, ProposalOutcome, ProposalResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) use task_recheck_recorded::{
    apply_resolved, apply_restored, decode_resolved, invalidate_guarded, recheck_view,
    remember_required, resolution_recovery_receipt, resolved_event, restored_resolution,
};
pub use task_recheck_recorded::{RecheckProjection, RecheckRequest, ResolveRecheckAction};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskRecheckOutcome {
    pub receipt: crate::recovery_contract::recorded::RequestReceipt<TaskRecheckResolution>,
    pub current_validity: crate::request_ledger::ReviewValidity,
}

const EDITABLE_TYPES: [&str; 4] = ["Intent", "Module", "Interface", "Acceptance"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposalSource {
    Draft {
        id: String,
        revision: String,
    },
    Branch {
        revision: String,
        base_revision: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProposalInput {
    pub source: ProposalSource,
    pub reason: String,
    pub affected_intents: Vec<String>,
    pub affected_modules: Vec<String>,
    pub affected_interfaces: Vec<String>,
    pub affected_tasks: Vec<String>,
    pub affected_paths: Vec<String>,
    pub alternatives: Vec<Alternative>,
    pub recommendation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub name: String,
    pub consequences: String,
}

pub fn validate_proposal(input: &ProposalInput) -> Result<(), String> {
    if input.reason.trim().is_empty() {
        return Err("proposal reason is required".into());
    }
    if input.recommendation.trim().is_empty()
        || input.alternatives.is_empty()
        || input
            .alternatives
            .iter()
            .any(|a| a.name.trim().is_empty() || a.consequences.trim().is_empty())
    {
        return Err("proposal requires complete alternatives and a recommendation".into());
    }
    let mut names = BTreeSet::new();
    for alternative in &input.alternatives {
        if !names.insert(&alternative.name) {
            return Err("duplicate alternative name".into());
        }
    }
    match &input.source {
        ProposalSource::Draft { id, revision } => {
            safe_id(id)?;
            recovery_contract::object_id(revision)?;
        }
        ProposalSource::Branch {
            revision,
            base_revision,
        } => {
            recovery_contract::object_id(revision)?;
            recovery_contract::object_id(base_revision)?;
        }
    }
    for ids in [
        &input.affected_intents,
        &input.affected_modules,
        &input.affected_interfaces,
        &input.affected_tasks,
    ] {
        let mut seen = BTreeSet::new();
        for id in ids {
            safe_id(id)?;
            if !seen.insert(id) {
                return Err("duplicate affected object ID".into());
            }
        }
    }
    let mut paths = BTreeSet::new();
    for path in &input.affected_paths {
        catalog::validate_pattern(path)?;
        if !paths.insert(path) {
            return Err("duplicate affected path".into());
        }
    }
    Ok(())
}

fn safe_id(id: &str) -> Result<(), String> {
    baseline_draft::safe_id(id)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProposalChange {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProposalImpact {
    pub intents: Vec<String>,
    pub modules: Vec<String>,
    pub interfaces: Vec<String>,
    pub acceptances: Vec<String>,
    pub tasks: Vec<String>,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProposalAnalysis {
    pub baseline: String,
    pub main_revision: String,
    pub bundle: DraftBundle,
    pub impact: ProposalImpact,
    pub changes: Vec<ProposalChange>,
}

/// Analyze immutable repository inputs against a verified current baseline.
/// `tasks` must be the caller's synchronized read-only Task snapshot. This is
/// an analysis result, not a receipt, run authorization or saved proposal.
pub fn analyze_proposal(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    input: &ProposalInput,
    tasks: &BTreeMap<String, Task>,
    proposal_id: &str,
) -> Result<ProposalAnalysis, String> {
    validate_proposal(input)?;
    safe_id(proposal_id)?;
    let current = baseline_current::current_baseline(catalog, inputs)?
        .ok_or("proposal requires an actually approved current baseline")?;
    let (mut bundle, source_revision) = match &input.source {
        ProposalSource::Draft { id, revision } => {
            let saved = read_draft(catalog, id)
                .map_err(|e| e.to_string())?
                .ok_or("proposal draft source ref is absent")?;
            if saved.revision() != revision {
                return Err("proposal draft source revision changed".into());
            }
            baseline_current::validate_draft_target(
                catalog,
                inputs,
                &saved.frozen().bundle.target_baseline,
            )?;
            // The ordinary freeze rejects protected-source drift; a historical
            // saved ref cannot silently become a proposal for a different base.
            if baseline_draft::freeze(catalog, &saved.frozen().bundle)? != *saved.frozen() {
                return Err("proposal draft differs from its actual frozen source".into());
            }
            (
                saved.frozen().bundle.clone(),
                saved.frozen().bundle.source_revision.clone(),
            )
        }
        ProposalSource::Branch {
            revision,
            base_revision,
        } => (
            branch_bundle(
                catalog,
                &current,
                proposal_id,
                revision,
                base_revision,
                None,
            )?,
            revision.clone(),
        ),
    };
    prepare_payload(&mut bundle, proposal_id);
    let frozen = baseline_draft::freeze(catalog, &bundle)?;
    let analysis = analyze_frozen(catalog, &current, input, tasks, frozen, &source_revision)?;
    // A mutable ref may have moved while the immutable source was analyzed.
    if let ProposalSource::Draft { id, revision } = &input.source {
        if read_draft(catalog, id)
            .map_err(|e| e.to_string())?
            .is_none_or(|saved| saved.revision() != revision)
        {
            return Err("proposal draft source ref changed during analysis".into());
        }
    }
    if baseline_current::current_baseline(catalog, inputs)?.as_ref() != Some(&current) {
        return Err("proposal current baseline changed during analysis".into());
    }
    Ok(analysis)
}

fn prepare_payload(bundle: &mut DraftBundle, proposal_id: &str) {
    bundle.id = proposal_id.into();
    // Candidate Tasks, scan Evidence and unrelated source objects are not part
    // of the proposed slow-layer publication payload.
    bundle.objects.retain(|_, object| {
        Document::parse(&object.text)
            .is_ok_and(|doc| EDITABLE_TYPES.contains(&doc.doc_type().unwrap_or_default()))
    });
}

// One analysis algorithm for the live producer and exact historical replay.
// The callers independently establish their current or historical source role.
fn analyze_frozen(
    catalog: &Catalog,
    current: &CurrentBaseline,
    input: &ProposalInput,
    tasks: &BTreeMap<String, Task>,
    frozen: baseline_draft::FrozenDraft,
    source_revision: &str,
) -> Result<ProposalAnalysis, String> {
    let changes = actual_changes(catalog, &frozen.bundle)?;
    if changes.is_empty() {
        return Err("proposal has no actual slow-layer or canvas changes".into());
    }
    let impact = analyze_impact(catalog, &frozen.bundle, &changes, tasks, source_revision)?;
    declared_coverage(input, &impact, catalog, &frozen.bundle, tasks)?;
    Ok(ProposalAnalysis {
        baseline: current.baseline.clone(),
        main_revision: current.main_revision.clone(),
        bundle: frozen.bundle,
        impact,
        changes,
    })
}

// Only the actual-history validator calls this with its original source Catalog,
// publication label and preceding canonical Task population. This never consults
// a mutable draft ref/current tags and cannot establish a new accepted event.
fn analyze_historical(
    catalog: &Catalog,
    baseline: &str,
    input: &ProposalInput,
    tasks: &BTreeMap<String, Task>,
    proposal_id: &str,
    owner: &Catalog,
    reads: &mut crate::recovery_git::PublicationReads,
) -> Result<ProposalAnalysis, String> {
    validate_proposal(input)?;
    safe_id(proposal_id)?;
    let current = CurrentBaseline {
        baseline: baseline.into(),
        main_revision: catalog.revision.clone(),
    };
    let (mut bundle, source_revision) = match &input.source {
        ProposalSource::Draft { id, revision } => {
            let saved = reads.saved_draft(owner, catalog, id, revision)?;
            if saved.frozen().bundle.target_baseline
                != baseline_current::next_baseline(Some(&current))?
            {
                return Err(
                    "historical proposal source draft target differs from its publication prefix"
                        .into(),
                );
            }
            (
                saved.frozen().bundle.clone(),
                saved.frozen().bundle.source_revision.clone(),
            )
        }
        ProposalSource::Branch {
            revision,
            base_revision,
        } => (
            branch_bundle(
                catalog,
                &current,
                proposal_id,
                revision,
                base_revision,
                Some((owner, &mut *reads)),
            )?,
            revision.clone(),
        ),
    };
    prepare_payload(&mut bundle, proposal_id);
    let source = reads.catalog(owner, &bundle.source_revision)?;
    if source.common_dir != catalog.common_dir
        || source.config.to_yaml() != catalog.config.to_yaml()
        || reads.source_directory(owner, &source)? != reads.source_directory(owner, catalog)?
    {
        return Err(
            "historical proposal payload source/project/protected directory differs".into(),
        );
    }
    let frozen = reads.frozen_source(owner, &source, &bundle)?;
    analyze_frozen(catalog, &current, input, tasks, frozen, &source_revision)
}

fn commit(catalog: &Catalog, revision: &str) -> Result<(), String> {
    recovery_contract::object_id(revision)?;
    if repo::git_object_type(&catalog.root, revision)?.as_deref() != Some("commit") {
        return Err("proposal source must be a complete actual commit OID".into());
    }
    Ok(())
}

fn branch_bundle(
    catalog: &Catalog,
    current: &CurrentBaseline,
    id: &str,
    revision: &str,
    base: &str,
    mut reads: Option<(&Catalog, &mut crate::recovery_git::PublicationReads)>,
) -> Result<DraftBundle, String> {
    commit(catalog, revision)?;
    commit(catalog, base)?;
    for descendant in [&catalog.revision, revision] {
        repo::git(
            &catalog.root,
            &["merge-base", "--is-ancestor", base, descendant],
        )
        .map_err(|_| {
            "proposal branch base must be an ancestor of both authority main and source".to_string()
        })?;
    }
    let (before, after) = match reads.as_mut() {
        Some((owner, reads)) => (reads.catalog(owner, base)?, reads.catalog(owner, revision)?),
        None => (
            std::sync::Arc::new(
                Catalog::at_revision(&catalog.root, base)?
                    .ok_or("proposal branch base lacks project configuration")?,
            ),
            std::sync::Arc::new(
                Catalog::at_revision(&catalog.root, revision)?
                    .ok_or("proposal branch source lacks project configuration")?,
            ),
        ),
    };
    for source in [&before, &after] {
        if source.common_dir != catalog.common_dir
            || source.read_file(crate::config::CONFIG_PATH)?
                != catalog.read_file(crate::config::CONFIG_PATH)?
        {
            return Err("proposal branch project/configuration changed".into());
        }
    }
    let (base_directory, current_directory) = match reads.as_mut() {
        Some((owner, reads)) => (
            reads.source_directory(owner, &before)?,
            reads.source_directory(owner, catalog)?,
        ),
        None => (
            crate::recovery_git::publication_base_directory(&before, base)?,
            crate::recovery_git::publication_base_directory(catalog, &catalog.revision)?,
        ),
    };
    if base_directory != current_directory {
        return Err(
            "proposal branch base protected directory differs from current authority".into(),
        );
    }
    let mut bundle = baseline_draft::current_source_bundle(catalog, id, &current.baseline)?;
    let mut replacements = Vec::new();
    for change in crate::task_gates::changes(&catalog.root, base, revision)
        .map_err(|error| format!("proposal source diff rejected: {error:?}"))?
    {
        let path =
            String::from_utf8(change.path).map_err(|_| "proposal source has non-UTF8 path")?;
        if path == crate::config::CONFIG_PATH {
            return Err("proposal branch cannot change project configuration".into());
        }
        if path == format!("{}/tower.canvas", catalog.config.docs_dir()) {
            if change.old_mode != 0 && change.old_mode != change.new_mode {
                return Err("proposal cannot change the protected canvas file mode".into());
            }
            bundle.canvas_text = after
                .read_file(&path)?
                .ok_or("proposal cannot delete the protected canvas")?;
            continue;
        }
        let old = editable_at(&before, &path)?;
        let new = editable_at(&after, &path)?;
        if (old.is_some() || new.is_some())
            && change.old_mode != 0
            && change.new_mode != 0
            && change.old_mode != change.new_mode
        {
            return Err("proposal cannot change a protected object file mode".into());
        }
        if let (Some(old), Some(new)) = (&old, &new) {
            if old.id != new.id
                || Document::parse(&old.text)?.doc_type() != Document::parse(&new.text)?.doc_type()
            {
                return Err("proposal branch cannot replace an unrelated object ID/type".into());
            }
        }
        if old.is_some() || new.is_some() {
            replacements.push((path, old, new));
        }
    }
    for (path, old, _) in &replacements {
        if let Some(old) = old {
            bundle.objects.remove(&old.id);
            bundle.deletions.push(path.clone());
        }
    }
    for (path, _, new) in replacements {
        if let Some(new) = new {
            bundle.deletions.retain(|deleted| deleted != &path);
            if bundle.objects.insert(new.id.clone(), new).is_some() {
                return Err("duplicate proposal object ID".into());
            }
        }
    }
    bundle.deletions.sort();
    bundle.deletions.dedup();
    Ok(bundle)
}

fn editable_at(catalog: &Catalog, path: &str) -> Result<Option<DraftObject>, String> {
    if !path.ends_with(".md") {
        return Ok(None);
    }
    let Some(text) = catalog.read_file(path)? else {
        return Ok(None);
    };
    let parsed = Document::parse(&text);
    let protected = catalog.protected_paths.iter().any(|p| p == path);
    let reserved = ["intents", "arch", "specs"]
        .iter()
        .any(|dir| path.starts_with(&format!("{}/{dir}/", catalog.config.docs_dir())));
    let doc = match parsed {
        Ok(doc) => doc,
        Err(error) if protected || reserved => {
            return Err(format!(
                "unclassifiable protected proposal object {path}: {error}"
            ))
        }
        Err(_) => return Ok(None),
    };
    let layer = types::layer_of(&doc);
    if layer == LayerResolution::Invalid {
        return Err(format!("invalid proposal object layer at {path}"));
    }
    let info = match types::lookup(doc.doc_type().unwrap_or_default()) {
        Some(info) => info,
        None if protected || reserved || layer.is_protected() => {
            return Err(format!("unknown protected proposal object type at {path}"));
        }
        // Ordinary Markdown metadata outside the protected object area is not
        // a Tower object, even when another tool also uses a `type` key.
        None => return Ok(None),
    };
    if !EDITABLE_TYPES.contains(&info.name) {
        if layer.is_protected() {
            return Err(format!("unsupported protected proposal object at {path}"));
        }
        return Ok(None);
    }
    if !matches!(layer, LayerResolution::Valid(value)|LayerResolution::Missing(Some(value)) if value == info.default_layer)
    {
        return Err(format!("proposal object type/layer differs at {path}"));
    }
    let id = doc.get_str("id").ok_or("proposal object lacks id")?;
    if catalog.object_paths.get(id).map(String::as_str) != Some(path) {
        return Err("proposal slow object is outside configured object directory".into());
    }
    safe_id(id)?;
    Ok(Some(DraftObject {
        id: id.into(),
        path: path.into(),
        text,
    }))
}

fn actual_changes(catalog: &Catalog, bundle: &DraftBundle) -> Result<Vec<ProposalChange>, String> {
    let mut after: BTreeMap<String, String> = bundle
        .objects
        .values()
        .map(|object| (object.path.clone(), object.text.clone()))
        .collect();
    after.insert(
        format!("{}/tower.canvas", bundle.docs_dir),
        bundle.canvas_text.clone(),
    );
    let mut paths: BTreeSet<String> = after.keys().cloned().collect();
    paths.extend(bundle.deletions.iter().cloned());
    let mut changes = Vec::new();
    for path in paths {
        let before = catalog.read_file(&path)?;
        let after = after.remove(&path);
        if before != after {
            changes.push(ProposalChange {
                path,
                before,
                after,
            });
        }
    }
    Ok(changes)
}

fn strings(doc: &Document, key: &str) -> Result<Vec<String>, String> {
    doc.get(key)
        .map(|value| {
            serde_yaml::from_value(value.clone())
                .map_err(|_| format!("proposal {key} must be a string array"))
        })
        .unwrap_or(Ok(vec![]))
}

#[derive(Default)]
struct Impact {
    intents: BTreeSet<String>,
    modules: BTreeSet<String>,
    interfaces: BTreeSet<String>,
    acceptances: BTreeSet<String>,
    tasks: BTreeSet<String>,
    paths: BTreeSet<String>,
}

fn analyze_impact(
    catalog: &Catalog,
    bundle: &DraftBundle,
    changes: &[ProposalChange],
    tasks: &BTreeMap<String, Task>,
    source_revision: &str,
) -> Result<ProposalImpact, String> {
    analyze_impact_for(catalog, bundle, changes, tasks, source_revision, false)
}

/// Recompute conflicts over the caller's exact publication-time population,
/// rather than the Proposal's earlier affected_tasks declaration. This is pure
/// analysis of immutable Git payloads, never proof that publication occurred.
/// `before` must be the actual authority parent, not the published new tree.
pub fn analyze_publication_tasks(
    before: &Catalog,
    frozen: &baseline_draft::FrozenDraft,
    tasks: &BTreeMap<String, Task>,
) -> Result<ProposalImpact, String> {
    validate_proposal_subject(frozen)?
        .ok_or("later publication requires its exact Proposal carrier")?;
    if baseline_draft::freeze(before, &frozen.bundle)? != *frozen {
        return Err(
            "publication task analysis differs from the exact frozen protected source".into(),
        );
    }
    publication_task_impact(before, frozen, tasks)
}

/// Only an already proved actual publication can audit its own occupied target.
/// New proposal/review preparation keeps the public strict target gate above.
pub(crate) fn analyze_verified_publication_tasks(
    before: &Catalog,
    frozen: &baseline_draft::FrozenDraft,
    tasks: &BTreeMap<String, Task>,
    proof: &crate::recovery_git::VerifiedPublication,
    owner: &Catalog,
    reads: &mut crate::recovery_git::PublicationReads,
) -> Result<ProposalImpact, String> {
    validate_proposal_subject(frozen)?
        .ok_or("later publication requires its exact Proposal carrier")?;
    let event = crate::recovery_git::baseline_created_event(proof)?;
    let record: crate::recovery_contract::recorded::BaselineCreated =
        serde_json::from_value(event.data).map_err(|error| error.to_string())?;
    let parent = repo::git(
        &before.root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{}^", record.result.commit),
        ],
    )?;
    if before.revision != parent
        || record.request_body.project_id != before.config.project_id()
        || record.result.target != frozen.bundle.target_baseline
        || record.result.content_hash != frozen.content_hash
    {
        return Err(
            "publication task audit differs from its proved exact parent/target/content".into(),
        );
    }
    let reference = format!("refs/tags/{}", record.result.target);
    let direct = repo::git(
        &before.root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(symref)",
            &reference,
        ],
    )?;
    let tagged = repo::git(
        &before.root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )?;
    if direct != format!("{reference}\0") || tagged != record.result.commit {
        return Err("publication task audit lacks its exact actual direct target tag".into());
    }
    let source = reads.catalog(owner, &frozen.bundle.source_revision)?;
    if source.common_dir != before.common_dir
        || source.config.to_yaml() != before.config.to_yaml()
        || reads.source_directory(owner, &source)? != reads.source_directory(owner, before)?
        || reads.frozen_source(owner, &source, &frozen.bundle)? != *frozen
    {
        return Err(
            "publication task audit differs from exact fixed source/protected directory".into(),
        );
    }
    publication_task_impact(before, frozen, tasks)
}

fn publication_task_impact(
    before: &Catalog,
    frozen: &baseline_draft::FrozenDraft,
    tasks: &BTreeMap<String, Task>,
) -> Result<ProposalImpact, String> {
    let mut payload = frozen.bundle.clone();
    payload
        .objects
        .remove(&payload.id)
        .ok_or("publication Proposal carrier is absent")?;
    let changes = actual_changes(before, &payload)?;
    analyze_impact_for(
        before,
        &payload,
        &changes,
        tasks,
        &payload.source_revision,
        true,
    )
}

fn analyze_impact_for(
    catalog: &Catalog,
    bundle: &DraftBundle,
    changes: &[ProposalChange],
    tasks: &BTreeMap<String, Task>,
    source_revision: &str,
    publication: bool,
) -> Result<ProposalImpact, String> {
    let mut impact = Impact::default();
    for change in changes {
        impact.paths.insert(change.path.clone());
        for text in change.before.iter().chain(change.after.iter()) {
            if change.path.ends_with("/tower.canvas") {
                continue;
            }
            let doc = Document::parse(text)?;
            let id = doc
                .get_str("id")
                .ok_or("changed proposal object lacks ID")?
                .to_owned();
            match doc.doc_type().unwrap() {
                "Intent" => {
                    impact.intents.insert(id);
                    impact.modules.extend(strings(&doc, "allocated_to")?);
                }
                "Module" => {
                    impact.modules.insert(id);
                    impact.paths.extend(strings(&doc, "paths")?);
                    impact.interfaces.extend(strings(&doc, "interfaces")?);
                }
                "Interface" => {
                    impact.interfaces.insert(id);
                    for key in ["from", "to"] {
                        impact.modules.insert(
                            doc.get_str(key)
                                .ok_or("proposal interface lacks endpoint")?
                                .to_owned(),
                        );
                    }
                }
                "Acceptance" => {
                    impact.acceptances.insert(id);
                    impact.modules.insert(
                        doc.get_str("module")
                            .ok_or("proposal Acceptance lacks module")?
                            .to_owned(),
                    );
                }
                _ => return Err("changed proposal payload is not an editable slow object".into()),
            }
        }
    }
    let mut documents: Vec<Document> = catalog.objects.values().cloned().collect();
    documents.extend(
        bundle
            .objects
            .values()
            .map(|o| Document::parse(&o.text))
            .collect::<Result<Vec<_>, _>>()?,
    );
    for doc in &documents {
        if doc.doc_type() == Some("Interface")
            && doc
                .get_str("id")
                .is_some_and(|id| impact.interfaces.contains(id))
        {
            for key in ["from", "to"] {
                impact.modules.insert(
                    doc.get_str(key)
                        .ok_or("proposal interface lacks endpoint")?
                        .to_owned(),
                );
            }
        }
    }
    let mut task_paths = BTreeSet::new();
    for doc in &documents {
        let Some(id) = doc.get_str("id") else {
            continue;
        };
        match doc.doc_type().unwrap_or_default() {
            "Intent"
                if strings(doc, "allocated_to")?
                    .iter()
                    .any(|id| impact.modules.contains(id)) =>
            {
                impact.intents.insert(id.into());
            }
            "Acceptance"
                if doc
                    .get_str("module")
                    .is_some_and(|id| impact.modules.contains(id)) =>
            {
                impact.acceptances.insert(id.into());
            }
            "Module" if impact.modules.contains(id) => {
                let paths = strings(doc, "paths")?;
                task_paths.extend(paths.iter().cloned());
                impact.paths.extend(paths);
            }
            _ => {}
        }
    }
    for path in &impact.paths {
        catalog::validate_pattern(path)?;
    }
    for (id, task) in tasks {
        if task.object_type != "Task" || task.spec.id != *id {
            return Err("proposal Task snapshot ID/type mismatch".into());
        }
        if !matches!(task.state.as_str(), "ready" | "running")
            && !(publication && matches!(task.state.as_str(), "verifying" | "held"))
        {
            continue;
        }
        let references = std::iter::once(&task.spec.module)
            .chain(task.spec.touches.iter())
            .any(|module| impact.modules.contains(module))
            || task
                .spec
                .acceptance
                .iter()
                .any(|id| impact.acceptances.contains(id));
        for path in &task.spec.allowed_paths {
            catalog::validate_pattern(path)?;
        }
        if references
            || task_path_impact(
                catalog,
                source_revision,
                &task.spec.allowed_paths,
                &task_paths,
                publication,
            )?
        {
            impact.tasks.insert(id.clone());
        }
    }
    Ok(ProposalImpact {
        intents: impact.intents.into_iter().collect(),
        modules: impact.modules.into_iter().collect(),
        interfaces: impact.interfaces.into_iter().collect(),
        acceptances: impact.acceptances.into_iter().collect(),
        tasks: impact.tasks.into_iter().collect(),
        paths: impact.paths.into_iter().collect(),
    })
}

fn tree_matches(
    catalog: &Catalog,
    revision: &str,
    patterns: &[String],
) -> Result<BTreeSet<Vec<u8>>, String> {
    if patterns.is_empty() {
        return Ok(BTreeSet::new());
    }
    let patterns: Vec<_> = patterns.iter().map(|p| format!(":(glob){p}")).collect();
    // ls-tree does not implement Git's glob pathspec magic. A read-only diff
    // against Git's empty tree enumerates the exact tree with the same native
    // matcher used by the existing task scope consumer. No temporary index.
    let empty = repo::git_timeout(
        &catalog.root,
        &["hash-object", "-t", "tree", "--stdin"],
        std::time::Duration::from_secs(10),
    )?;
    let mut args = vec![
        "diff-tree",
        "--no-commit-id",
        "-r",
        "--name-only",
        "-z",
        &empty,
        revision,
        "--",
    ];
    args.extend(patterns.iter().map(String::as_str));
    let output = repo::git_bytes_timeout(&catalog.root, &args, std::time::Duration::from_secs(10))?;
    Ok(output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(Vec::from)
        .collect())
}

fn task_path_impact(
    catalog: &Catalog,
    source: &str,
    allowed: &[String],
    affected: &BTreeSet<String>,
    publication: bool,
) -> Result<bool, String> {
    for path in allowed {
        catalog::validate_pattern(path)?;
    }
    if allowed.is_empty() || affected.is_empty() {
        return Ok(false);
    }
    let affected: Vec<_> = affected.iter().cloned().collect();
    for revision in [&catalog.revision, source] {
        let a = tree_matches(catalog, revision, allowed)?;
        let b = tree_matches(catalog, revision, &affected)?;
        if !a.is_disjoint(&b) {
            return Ok(true);
        }
    }
    // Existing files cannot prove disjoint scopes for files created later.
    // No second glob interpreter: uncertain symbolic ranges conservatively
    // remain in the required declaration rather than silently omitting a Task.
    Ok(allowed.iter().any(|a| {
        affected.iter().any(|b| {
            if publication {
                !catalog::patterns_provably_disjoint(a, b)
            } else {
                catalog::pattern_covers(a, b)
                    || catalog::pattern_covers(b, a)
                    || a.contains(['*', '?'])
                    || b.contains(['*', '?'])
            }
        })
    }))
}

fn declared_coverage(
    input: &ProposalInput,
    impact: &ProposalImpact,
    catalog: &Catalog,
    bundle: &DraftBundle,
    tasks: &BTreeMap<String, Task>,
) -> Result<(), String> {
    for (label, required, declared, kind) in [
        (
            "intents",
            &impact.intents,
            &input.affected_intents,
            "Intent",
        ),
        (
            "modules",
            &impact.modules,
            &input.affected_modules,
            "Module",
        ),
        (
            "interfaces",
            &impact.interfaces,
            &input.affected_interfaces,
            "Interface",
        ),
    ] {
        for id in required {
            if !declared.contains(id) {
                return Err(format!(
                    "proposal affected_{label} omits actual object {id}"
                ));
            }
        }
        for id in declared {
            let candidate = bundle
                .objects
                .get(id)
                .map(|o| Document::parse(&o.text))
                .transpose()?;
            if !candidate
                .as_ref()
                .or_else(|| catalog.objects.get(id))
                .is_some_and(|doc| doc.doc_type() == Some(kind))
            {
                return Err(format!(
                    "proposal affected_{label} contains unknown/wrong type object {id}"
                ));
            }
        }
    }
    for id in &impact.tasks {
        if !input.affected_tasks.contains(id) {
            return Err(format!("proposal affected_tasks omits actual Task {id}"));
        }
    }
    for id in &input.affected_tasks {
        if !tasks.contains_key(id) {
            return Err(format!(
                "proposal affected_tasks contains unknown Task {id}"
            ));
        }
    }
    for path in &impact.paths {
        if !input
            .affected_paths
            .iter()
            .any(|cover| catalog::pattern_covers(cover, path))
        {
            return Err(format!("proposal affected_paths omits actual path {path}"));
        }
    }
    Ok(())
}
