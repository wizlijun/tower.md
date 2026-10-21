//! Real verifier results for the complete, fixed input of an unapproved S7 draft.
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::baseline::BaselineDraft;
use crate::cold_start::{self, ScanCommand, ScanModule, ScanReport};
use towermd_core::okf::Document;
use crate::verifier::{Limits, VerificationPlan, VerificationReport, VerificationStatus};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewVerification {
    pub parent_id: String,
    pub input_sha256: String,
    pub run_id: String,
    pub limits: Limits,
    pub report: VerificationReport,
}

/// Canonical command IDs share identical cwd/command pairs across modules. The
/// original S5 scan remains unchanged; this projection is only verifier input.
pub fn scan(draft: &BaselineDraft, run_id: &str) -> Result<ScanReport, String> {
    if run_id.is_empty()
        || run_id.len() > 128
        || !run_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("审阅验证运行 ID 无效".into());
    }
    let review = draft
        .review
        .as_ref()
        .ok_or("请先在 App 中审阅草稿并确认验证命令")?;
    let mut scan = draft.scan.clone();
    scan.id = run_id.into();
    scan.view = draft.view.clone();
    scan.verification = None;
    scan.commands.clear();
    scan.modules.clear();
    let mut commands = BTreeMap::<(String, String), ScanCommand>::new();
    for module in &review.modules {
        let mut command_ids = Vec::new();
        for command in &module.commands {
            let key = (command.directory.clone(), command.command.clone());
            let id = format!(
                "cmd-{:x}",
                Sha256::digest(serde_json::to_vec(&key).map_err(|e| e.to_string())?)
            );
            command_ids.push(id.clone());
            commands
                .entry(key)
                .or_insert_with(|| ScanCommand {
                    id,
                    directory: command.directory.clone(),
                    command: command.command.clone(),
                    modules: Vec::new(),
                })
                .modules
                .push(module.id.clone());
        }
        scan.modules.push(ScanModule {
            id: module.id.clone(),
            title: module.title.clone(),
            paths: module.files.clone(),
            files: module.files.clone(),
            confidence: 1.0,
            rationale: "App 审阅后的模块与验证命令".into(),
            command_ids,
            suggested_task: module
                .commands
                .is_empty()
                .then(|| "补充模块验证测试".into()),
        });
    }
    scan.commands = commands.into_values().collect();
    Ok(scan)
}

/// Bind every execution input, plus module ownership and interface contracts.
/// Cosmetic card coordinates and module names do not affect executable evidence.
pub fn input_sha256(draft: &BaselineDraft, limits: &Limits) -> Result<String, String> {
    let scan = scan(draft, "input")?;
    let modules: Vec<_> = scan
        .modules
        .iter()
        .map(|module| (&module.id, &module.files, &module.command_ids))
        .collect();
    let interfaces: Vec<_> = draft
        .view
        .nodes
        .iter()
        .filter(|node| node.object_type.as_deref() == Some("Interface"))
        .map(|node| (&node.id, &node.content))
        .collect();
    let bytes = serde_json::to_vec(&(
        "s7-verification-v1",
        &scan.project_id,
        &scan.revision,
        &scan.tree,
        &draft.review.as_ref().unwrap().base_id,
        modules,
        &scan.commands,
        interfaces,
        limits,
    ))
    .map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn plan(
    draft: &BaselineDraft,
    root: &Path,
    run_id: &str,
    limits: Limits,
) -> Result<VerificationPlan, String> {
    let scan = scan(draft, run_id)?;
    Ok(VerificationPlan {
        root: root.to_owned(),
        id: scan.id,
        revision: scan.revision,
        tree: scan.tree,
        commands: scan.commands,
        modules: scan.modules,
        limits,
    })
}

/// Validate before changing any projection. The durable original report remains
/// readable after cache eviction; accepting it as current evidence also requires
/// validate_logs(). No result here approves B0 or authenticates task completion.
pub fn apply(draft: &mut BaselineDraft, verification: ReviewVerification) -> Result<(), String> {
    crate::draft_store::valid_id(&verification.parent_id)?;
    if input_sha256(draft, &verification.limits)? != verification.input_sha256 {
        return Err("审阅验证输入摘要与当前模块、命令或接口不一致".into());
    }
    let mut scan = scan(draft, &verification.run_id)?;
    scan.verification = Some(verification.report.clone());
    cold_start::validate_verification(&scan)?;
    let report_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&verification.report).map_err(|e| e.to_string())?)
    );
    let mut checked = draft.clone();
    for result in &verification.report.modules {
        let module = checked
            .review
            .as_mut()
            .unwrap()
            .modules
            .iter_mut()
            .find(|module| module.id == result.module)
            .ok_or("验证含未知审阅模块")?;
        module.verification_pending = !verification.report.cleanup_complete
            || matches!(
                result.status,
                VerificationStatus::NotRun
                    | VerificationStatus::Cancelled
                    | VerificationStatus::TimedOut
            );
        let node = checked
            .view
            .nodes
            .iter_mut()
            .find(|node| node.object_id.as_deref() == Some(result.module.as_str()))
            .ok_or("验证模块缺少草稿节点")?;
        let original = Document::parse(&node.content)?;
        let mut front = original.front().clone();
        front.remove(serde_yaml::Value::from("initial_evidence"));
        let header = serde_yaml::to_string(&front).map_err(|e| e.to_string())?;
        let mut document = Document::parse(&format!("---\n{header}---\n{}", original.body()))?;
        document.set(
            "verification",
            serde_yaml::to_value(result.status).map_err(|e| e.to_string())?,
        )?;
        document.set(
            "review_verification_pending",
            module.verification_pending.into(),
        )?;
        document.set(
            "review_evidence",
            serde_yaml::to_value(serde_json::json!({
                "actor": verification.report.actor,
                "revision": verification.report.revision,
                "tree": verification.report.tree,
                "input_sha256": verification.input_sha256,
                "report_sha256": report_hash,
                "run_id": verification.run_id,
                "started_at": verification.report.started_at,
                "finished_at": verification.report.finished_at,
                "status": result.status,
                "commands": result.command_ids,
                "shared": result.shared,
                "cleanup_complete": verification.report.cleanup_complete,
            }))
            .map_err(|e| e.to_string())?,
        )?;
        node.content = document.to_text();
    }
    let review = checked.review.as_mut().unwrap();
    review.pending_modules = review
        .modules
        .iter()
        .filter(|module| module.verification_pending)
        .map(|module| module.id.clone())
        .collect();
    review.verification = Some(verification);
    checked
        .view
        .diagnostics
        .retain(|message| !message.starts_with("S7 人工审阅草稿；"));
    checked
        .view
        .diagnostics
        .push("S7 验证来自固定提交及当前审阅输入的真实命令；草稿仍未批准。".into());
    *draft = checked;
    Ok(())
}

pub fn validate_logs(draft: &BaselineDraft, common: &Path) -> Result<(), String> {
    let verification = draft
        .review
        .as_ref()
        .and_then(|review| review.verification.as_ref())
        .ok_or("审阅草稿没有重新验证结果")?;
    if input_sha256(draft, &verification.limits)? != verification.input_sha256 {
        return Err("验证输入已改变".into());
    }
    let mut scan = scan(draft, &verification.run_id)?;
    scan.verification = Some(verification.report.clone());
    cold_start::validate_verification_logs(&scan, common)
}
