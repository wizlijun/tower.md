//! The one registered L4 exception in a proposed publication payload.
use super::{validate_proposal, ProposalAnalysis, ProposalImpact, ProposalInput};
use crate::{
    baseline_draft::{self, DraftBundle, DraftObject, FrozenDraft},
    catalog::Catalog,
    okf::Document,
    recovery_contract::{self, baseline::PublicationFreeze},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProposalCarrier {
    pub schema: u32,
    pub input: ProposalInput,
    pub impact: ProposalImpact,
    pub baseline: String,
    pub main_revision: String,
    pub run: String,
    pub payload_hash: String,
    // Old descriptive carriers remain readable, but cannot become an accepted
    // Proposal without the original live producer binding saved before the ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<super::ProposalBinding>,
}

/// Construct a pure, explicit proposal subject. This has no live-run or write
/// authority; the typed producer must independently validate all run fences.
pub fn attach_proposal(
    catalog: &Catalog,
    analysis: &ProposalAnalysis,
    input: &ProposalInput,
    run: &str,
) -> Result<FrozenDraft, String> {
    validate_proposal(input)?;
    crate::hub::validate_token("run", run).map_err(|e| e.to_string())?;
    if analysis.bundle.objects.values().any(|object| {
        Document::parse(&object.text).is_ok_and(|doc| doc.doc_type() == Some("Proposal"))
    }) {
        return Err("proposal subject already contains a carrier".into());
    }
    let payload = baseline_draft::freeze(catalog, &analysis.bundle)?;
    let carrier = ProposalCarrier {
        schema: 1,
        input: input.clone(),
        impact: analysis.impact.clone(),
        baseline: analysis.baseline.clone(),
        main_revision: analysis.main_revision.clone(),
        run: run.into(),
        payload_hash: payload.content_hash,
        binding: None,
    };
    let mut doc = Document::new("Proposal", "")?;
    for (key, value) in [
        ("id", serde_json::json!(analysis.bundle.id)),
        ("layer", serde_json::json!("L4")),
        ("kind", serde_json::json!("slow_layer_change")),
        (
            "draft_baseline",
            serde_json::json!(analysis.bundle.target_baseline),
        ),
        ("requested_by", serde_json::json!("process:towermd-cli")),
        ("reason", serde_json::json!(input.reason)),
        (
            "alternatives",
            serde_json::to_value(&input.alternatives).map_err(|e| e.to_string())?,
        ),
        ("recommendation", serde_json::json!(input.recommendation)),
        (
            "impact",
            serde_json::to_value(&carrier.impact).map_err(|e| e.to_string())?,
        ),
        (
            "proposal",
            serde_json::to_value(&carrier).map_err(|e| e.to_string())?,
        ),
    ] {
        doc.set(key, serde_yaml::to_value(value).map_err(|e| e.to_string())?)?;
    }
    let object = DraftObject {
        id: analysis.bundle.id.clone(),
        path: format!(
            "{}/proposals/{}.md",
            analysis.bundle.docs_dir, analysis.bundle.id
        ),
        text: doc.to_text(),
    };
    carrier_object(&object, &analysis.bundle)?;
    let mut bundle = analysis.bundle.clone();
    if bundle.objects.insert(object.id.clone(), object).is_some() {
        return Err("proposal ID is occupied by another payload object".into());
    }
    let frozen = baseline_draft::freeze(catalog, &bundle)?;
    validate_proposal_subject(&frozen)?;
    Ok(frozen)
}

fn field(doc: &Document, key: &str) -> Result<serde_json::Value, String> {
    serde_json::to_value(
        doc.get(key)
            .ok_or_else(|| format!("proposal carrier lacks {key}"))?,
    )
    .map_err(|e| e.to_string())
}

fn read_carrier(
    text: &str,
    id: &str,
    path: &str,
    docs_dir: &str,
    target: &str,
) -> Result<ProposalCarrier, String> {
    baseline_draft::safe_id(id)?;
    let doc = Document::parse(text)?;
    if doc.doc_type() != Some("Proposal")
        || doc.get_str("id") != Some(id)
        || path != format!("{docs_dir}/proposals/{id}.md")
        || doc.get("layer").is_some() && doc.get_str("layer") != Some("L4")
    {
        return Err("proposal carrier type/id/path/layer mismatch".into());
    }
    let value: ProposalCarrier =
        serde_json::from_value(field(&doc, "proposal")?).map_err(|e| e.to_string())?;
    validate_proposal(&value.input)?;
    recovery_contract::object_id(&value.main_revision)?;
    recovery_contract::sha256_hex(&value.payload_hash)?;
    crate::hub::validate_token("run", &value.run).map_err(|e| e.to_string())?;
    let n = crate::baseline_current::baseline_number(&value.baseline)?;
    if value.schema != 1
        || n.checked_add(1).map(|n| format!("towermd/B{n}")).as_deref() != Some(target)
        || field(&doc, "kind")? != "slow_layer_change"
        || field(&doc, "draft_baseline")? != target
        || field(&doc, "requested_by")? != "process:towermd-cli"
        || field(&doc, "reason")? != value.input.reason
        || field(&doc, "alternatives")?
            != serde_json::to_value(&value.input.alternatives).map_err(|e| e.to_string())?
        || field(&doc, "recommendation")? != value.input.recommendation
        || field(&doc, "impact")?
            != serde_json::to_value(&value.impact).map_err(|e| e.to_string())?
    {
        return Err("proposal carrier fixed metadata differs from original input/analysis".into());
    }
    for ids in [
        &value.impact.intents,
        &value.impact.modules,
        &value.impact.interfaces,
        &value.impact.acceptances,
        &value.impact.tasks,
    ] {
        for id in ids {
            baseline_draft::safe_id(id)?;
        }
        if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("proposal impact IDs are not sorted and unique".into());
        }
    }
    for path in &value.impact.paths {
        crate::catalog::validate_pattern(path)?;
    }
    if value.impact.paths.is_empty() || value.impact.paths.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err("proposal impact paths are not nonempty, sorted and unique".into());
    }
    for (required, declared) in [
        (&value.impact.intents, &value.input.affected_intents),
        (&value.impact.modules, &value.input.affected_modules),
        (&value.impact.interfaces, &value.input.affected_interfaces),
        (&value.impact.tasks, &value.input.affected_tasks),
    ] {
        if required.iter().any(|id| !declared.contains(id)) {
            return Err("proposal carrier input omits computed impact IDs".into());
        }
    }
    for path in &value.impact.paths {
        if !value
            .input
            .affected_paths
            .iter()
            .any(|cover| crate::catalog::pattern_covers(cover, path))
        {
            return Err("proposal carrier input omits computed impact paths".into());
        }
    }
    Ok(value)
}

/// Check the exact analyzed payload without granting approval or certifying
/// that a live producer accepted it. An edited subject must be proposed again.
pub fn validate_proposal_subject(frozen: &FrozenDraft) -> Result<Option<ProposalCarrier>, String> {
    let freeze = baseline_draft::publication_freeze(frozen)?;
    let texts = frozen
        .files
        .iter()
        .map(|(path, file)| (path.clone(), file.text.clone()))
        .collect();
    validate_publication_carrier(&freeze, &texts)
}

pub(crate) fn carrier_object(
    object: &DraftObject,
    bundle: &DraftBundle,
) -> Result<ProposalCarrier, String> {
    if object.id != bundle.id {
        return Err("proposal carrier must use the precise draft ID".into());
    }
    read_carrier(
        &object.text,
        &bundle.id,
        &object.path,
        &bundle.docs_dir,
        &bundle.target_baseline,
    )
}

/// Validate the exact embedded proposal and the payload it analyzed. The
/// resulting value is descriptive metadata, never a trusted producer receipt.
pub(crate) fn validate_publication_carrier(
    freeze: &PublicationFreeze,
    texts: &BTreeMap<String, String>,
) -> Result<Option<ProposalCarrier>, String> {
    let mut carrier = None;
    for (path, text) in texts {
        if path == &format!("{}/tower.canvas", freeze.docs_dir) {
            continue;
        }
        let doc = Document::parse(text)?;
        if doc.doc_type() != Some("Proposal") {
            continue;
        }
        if carrier.is_some() {
            return Err("publication contains multiple proposal carriers".into());
        }
        carrier = Some((
            path.clone(),
            read_carrier(
                text,
                &freeze.draft_id,
                path,
                &freeze.docs_dir,
                &freeze.target_baseline,
            )?,
        ));
    }
    let Some((path, carrier)) = carrier else {
        return Ok(None);
    };
    let mut payload = freeze.clone();
    payload.files.remove(&path);
    let mut payload_texts = texts.clone();
    payload_texts.remove(&path);
    if recovery_contract::baseline::publication_content_hash(&payload, &payload_texts)?
        != carrier.payload_hash
    {
        return Err("proposal payload changed since impact analysis; create a newly analyzed proposal before approval".into());
    }
    Ok(Some(carrier))
}

/// Only the live producer uses this before materialization. The DTO alone is
/// not an acceptance capability; actual outer provenance is proved separately.
pub(crate) fn attach_bound_proposal(
    catalog: &Catalog,
    analysis: &ProposalAnalysis,
    input: &ProposalInput,
    binding: &super::ProposalBinding,
) -> Result<FrozenDraft, String> {
    let mut frozen = attach_proposal(catalog, analysis, input, &binding.run)?;
    let object = frozen
        .bundle
        .objects
        .get_mut(&frozen.bundle.id)
        .ok_or("proposal carrier is absent")?;
    let mut document = Document::parse(&object.text)?;
    let mut carrier: ProposalCarrier =
        serde_json::from_value(field(&document, "proposal")?).map_err(|error| error.to_string())?;
    carrier.binding = Some(binding.clone());
    document.set(
        "proposal",
        serde_yaml::to_value(carrier).map_err(|error| error.to_string())?,
    )?;
    object.text = document.to_text();
    frozen = baseline_draft::freeze(catalog, &frozen.bundle)?;
    validate_proposal_subject(&frozen)?;
    Ok(frozen)
}
