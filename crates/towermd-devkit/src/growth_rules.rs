//! Spec 9.2 rows 2–4 and FR-22: what one growth step must show before a
//! person can approve it. Growing asks only for a smoke test once a baseline
//! exists; settled asks for every module's commands, a contract file or test
//! per interface, and a note for every affected object. The rules only block
//! a preview or a submission; they approve nothing and change no structure.
use std::collections::BTreeMap;

use towermd_core::{config::Config, okf::Document, project_view::CanvasNode};

use crate::{
    growth::{Mode, Settings},
    proposal_model::Impact,
    review::{safe_path, ReviewedModule},
    verifier::{VerificationReport, VerificationStatus},
};

/// The mode from `main`'s `config.yaml` and the approved module count, the
/// same reading the growth signals use.
pub fn mode(config: &Config, approved_modules: usize) -> Mode {
    Settings::from_config(config.section("signals"), config.section("growth")).mode(approved_modules)
}

/// Default verification (row 4) and FR-22 for publishing a reviewed draft
/// whose `report` ran at the source revision. `first` is B0: a growing seed
/// may start untested, its first task being the smoke test. Every later step
/// must leave `main` passing at least one real command; settled needs every
/// module verified by its own commands.
pub fn verification(mode: Mode, first: bool, modules: &[ReviewedModule], report: &VerificationReport) -> Result<(), String> {
    match mode {
        Mode::Settled => {
            let untested: Vec<&str> = modules.iter().filter(|m| m.commands.is_empty()).map(|m| m.id.as_str())
                .chain(report.modules.iter().filter(|m| m.status != VerificationStatus::Passed).map(|m| m.module.as_str()))
                .collect();
            if untested.is_empty() { Ok(()) } else {
                Err(format!("定型期每个模块都须由验证命令通过：{}", unique(untested)))
            }
        }
        Mode::Growing if first => Ok(()),
        Mode::Growing => {
            if report.commands.iter().any(|c| c.status == VerificationStatus::Passed) { Ok(()) } else {
                Err("每一步生长后 main 须通过验证：至少一条冒烟测试须真实通过".into())
            }
        }
    }
}

/// Interface contracts (row 3): settled needs a contract file or test. The
/// `contract` text must name a repo-relative file that `exists` at the
/// source revision or in the publication; growing accepts one sentence.
pub fn interface_contracts(mode: Mode, nodes: &[CanvasNode], exists: impl Fn(&str) -> bool) -> Result<(), String> {
    if mode == Mode::Growing { return Ok(()); }
    let mut missing = Vec::new();
    for node in nodes.iter().filter(|n| n.object_type.as_deref() == Some("Interface")) {
        let document = Document::parse(&node.content)?;
        let named = document.get_str("contract").is_some_and(|text| paths(text).any(&exists));
        if !named { missing.push(node.object_id.as_deref().unwrap_or(node.id.as_str())); }
    }
    if missing.is_empty() { Ok(()) } else {
        Err(format!("定型期接口须有契约文件或测试，在 contract 中写出仓库内相对路径：{}", unique(missing)))
    }
}

/// Path-like words in a contract sentence, stripped of quoting.
fn paths(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| "`'\"，。,;:：；()（）「」[]".contains(c)))
        .filter(|word| safe_path(word))
}

/// Impact analysis (row 2): notes may only explain objects the daemon
/// derived; settled needs a note for every affected intent, module,
/// interface and task. Paths are covered by their modules.
pub fn impact_notes(mode: Mode, impact: &Impact, notes: &BTreeMap<String, String>) -> Result<(), String> {
    let affected: Vec<&String> = impact.intents.iter().chain(&impact.modules).chain(&impact.interfaces).chain(&impact.tasks).collect();
    if let Some(id) = notes.keys().find(|id| !affected.contains(id)) {
        return Err(format!("impact_notes 只能说明守护进程推算出的受影响对象：{id}"));
    }
    if notes.values().any(|note| note.trim().is_empty()) {
        return Err("impact_notes 的说明不能为空".into());
    }
    if mode == Mode::Growing { return Ok(()); }
    let missing: Vec<&str> = affected.into_iter().filter(|id| !notes.contains_key(*id)).map(String::as_str).collect();
    if missing.is_empty() { Ok(()) } else {
        Err(format!("定型期提案须附完整影响分析，在 impact_notes 中按 ID 说明：{}", unique(missing)))
    }
}

fn unique(mut ids: Vec<&str>) -> String {
    ids.sort_unstable();
    ids.dedup();
    ids.join("、")
}

#[cfg(test)]
#[path = "growth_rules_tests.rs"]
mod tests;
