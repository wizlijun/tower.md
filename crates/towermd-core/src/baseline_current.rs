//! Verified, read-only authority for opening a later baseline draft.
use crate::{catalog::Catalog, recovery_outbox::RecoveryInputs};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentBaseline {
    pub baseline: String,
    pub main_revision: String,
}

pub(crate) fn baseline_number(label: &str) -> Result<u64, String> {
    let text = label
        .strip_prefix("towermd/B")
        .ok_or("invalid precise baseline target")?;
    let number: u64 = text.parse().map_err(|_| "invalid baseline number")?;
    if text != number.to_string() {
        return Err("noncanonical baseline number".into());
    }
    Ok(number)
}

pub(crate) fn next_baseline(current: Option<&CurrentBaseline>) -> Result<String, String> {
    match current {
        None => Ok("towermd/B0".into()),
        Some(current) => Ok(format!(
            "towermd/B{}",
            baseline_number(&current.baseline)?
                .checked_add(1)
                .ok_or("baseline number exhausted")?
        )),
    }
}

fn observed_tags(catalog: &Catalog) -> Result<String, String> {
    crate::repo::git(
        &catalog.root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(symref)",
            "refs/tags/towermd/",
        ],
    )
}

/// Every baseline label must be direct, canonical and part of a complete real
/// publication chain on authoritative main. Private refs and bare tags grant no authority.
pub fn current_baseline(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
) -> Result<Option<CurrentBaseline>, String> {
    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
        return Err("current baseline conflict: authoritative main changed".into());
    }
    inputs.probe(catalog)?;
    let before = observed_tags(catalog)?;
    let mut tags = BTreeMap::new();
    for line in before.lines() {
        let fields: Vec<_> = line.split('\0').collect();
        if fields.len() != 3 {
            return Err("invalid baseline tag observation".into());
        }
        let Some(label) = fields[0].strip_prefix("refs/tags/") else {
            continue;
        };
        if !label.starts_with("towermd/B") {
            continue;
        }
        let number = baseline_number(label)?;
        if !fields[2].is_empty() {
            return Err("baseline tag is symbolic".into());
        }
        crate::recovery_contract::object_id(fields[1])?;
        tags.insert(number, label.to_string());
    }
    let mut latest = None;
    if !tags.is_empty() {
        let history = crate::recovery_git::read_recovery_history(catalog, inputs)?;
        for (expected, (number, label)) in tags.iter().enumerate() {
            if *number != expected as u64 {
                return Err("publication baseline chain has gap".into());
            }
            let commit = crate::repo::git(
                &catalog.root,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("refs/tags/{label}^{{commit}}"),
                ],
            )?;
            let proof = crate::recovery_git::prove_publication(catalog, &history, &commit)?;
            let event = crate::recovery_git::baseline_created_event(&proof)?;
            if event.data["tag"] != *label || event.data["target"] != *label {
                return Err("baseline tag differs from proved publication".into());
            }
            latest = Some(CurrentBaseline {
                baseline: label.clone(),
                main_revision: catalog.revision.clone(),
            });
        }
    }
    inputs.probe(catalog)?;
    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision
        || observed_tags(catalog)? != before
    {
        return Err("current baseline conflict: authority or tags changed during proof".into());
    }
    Ok(latest)
}

/// Validate a saved draft's target against current actual publication history.
/// This is read-only validation; it does not establish approval or publication.
pub fn validate_draft_target(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    target: &str,
) -> Result<(), String> {
    let current = current_baseline(catalog, inputs)?;
    if next_baseline(current.as_ref())? != target {
        return Err("draft conflict: current baseline or target changed".into());
    }
    Ok(())
}
