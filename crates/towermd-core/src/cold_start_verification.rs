//! Project actual verifier output onto candidate objects; never approve a baseline.
use std::collections::BTreeSet;
use std::path::{Component, Path};

use sha2::{Digest, Sha256};

use crate::cold_start::ScanReport;
use crate::okf::Document;
use crate::verifier::{VerificationReport, VerificationStatus};

pub fn validate_verification(scan: &ScanReport) -> Result<(), String> {
    let Some(report) = &scan.verification else {
        return Ok(());
    };
    use VerificationStatus::*;
    if report.actor != "process:towermd-verifier"
        || report.revision != scan.revision
        || report.tree != scan.tree
    {
        return Err("验证报告的来源、tree 或署名不匹配".into());
    }
    let started =
        chrono::DateTime::parse_from_rfc3339(&report.started_at).map_err(|_| "验证开始时间无效")?;
    let finished = chrono::DateTime::parse_from_rfc3339(&report.finished_at)
        .map_err(|_| "验证结束时间无效")?;
    if finished < started {
        return Err("验证时间顺序无效".into());
    }
    if report.commands.len() != scan.commands.len() || report.modules.len() != scan.modules.len() {
        return Err("验证报告的命令或模块数量不匹配".into());
    }
    let mut seen = BTreeSet::new();
    for result in &report.commands {
        if !seen.insert(&result.id) {
            return Err("验证报告重复命令".into());
        }
        let command = scan
            .commands
            .iter()
            .find(|c| c.id == result.id)
            .ok_or("验证报告含未知命令")?;
        if result.directory != command.directory
            || result.argv != ["/bin/sh", "-c", command.command.as_str()]
            || result.modules != command.modules
            || result.status == NoTests
        {
            return Err("验证命令的 cwd、argv 或模块绑定不匹配".into());
        }
        if result.status == Passed
            && (result.exit_code != Some(0) || !report.cleanup_complete || result.log.is_none())
        {
            return Err("通过结果缺少成功退出或完整清理与日志".into());
        }
        match (&result.log, &result.log_sha256) {
            (Some(log), Some(hash))
                if log.starts_with(&format!("towermd/verify/{}/logs/", scan.id))
                    && hash.len() == 64
                    && hash.bytes().all(|b| b.is_ascii_hexdigit()) => {}
            (None, None) if result.status != Passed => {}
            _ => return Err("验证日志引用或摘要无效".into()),
        }
    }
    seen.clear();
    for result in &report.modules {
        if !seen.insert(&result.module) {
            return Err("验证报告重复模块".into());
        }
        let module = scan
            .modules
            .iter()
            .find(|m| m.id == result.module)
            .ok_or("验证报告含未知模块")?;
        if result.command_ids != module.command_ids {
            return Err("模块验证命令绑定不匹配".into());
        }
        let commands: Vec<_> = report
            .commands
            .iter()
            .filter(|c| result.command_ids.contains(&c.id))
            .collect();
        if commands.len() != result.command_ids.len()
            || result.shared != commands.iter().any(|c| c.modules.len() > 1)
        {
            return Err("模块的共享验证命令不匹配".into());
        }
        if (result.status == NoTests) != commands.is_empty() {
            return Err("无测试状态与实际命令不匹配".into());
        }
        let expected = if commands.is_empty() {
            NoTests
        } else {
            [Failed, TimedOut, Cancelled, NotRun]
                .into_iter()
                .find(|state| commands.iter().any(|command| command.status == *state))
                .unwrap_or(Passed)
        };
        if result.status != expected {
            return Err("模块状态与原始命令结果不一致".into());
        }
        if result.status == Passed
            && (!report.cleanup_complete || commands.iter().any(|c| c.status != Passed))
        {
            return Err("模块通过状态缺少全部命令的成功证据".into());
        }
    }
    Ok(())
}

/// A cached result is only readable while its bounded local log bytes still match.
pub fn validate_verification_logs(scan: &ScanReport, common: &Path) -> Result<(), String> {
    validate_verification(scan)?;
    let Some(report) = &scan.verification else {
        return Ok(());
    };
    for result in &report.commands {
        let Some(log) = &result.log else { continue };
        let parts: Vec<_> = Path::new(log).components().collect();
        let mut path = common.to_path_buf();
        for (index, component) in parts.iter().enumerate() {
            let Component::Normal(part) = component else {
                return Err("验证日志路径不安全".into());
            };
            path.push(part);
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|_| "验证日志已缺失；请重新扫描验证，不能沿用原结果")?;
            if (index + 1 < parts.len() && !meta.file_type().is_dir())
                || (index + 1 == parts.len() && !meta.file_type().is_file())
            {
                return Err("验证日志路径包含链接或非法文件".into());
            }
        }
        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        if meta.len() > 8 * 1024 * 1024 {
            return Err("验证日志超过读取上限".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if result.log_sha256.as_deref() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
            return Err("验证日志摘要已改变；不能沿用原结果".into());
        }
    }
    Ok(())
}

pub fn apply_verification(scan: &mut ScanReport, report: VerificationReport) -> Result<(), String> {
    // Validate a clone before changing any candidate object.
    let mut checked = scan.clone();
    checked.verification = Some(report);
    validate_verification(&checked)?;
    let report = checked.verification.as_ref().unwrap();
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(report).map_err(|e| e.to_string())?)
    );
    for node in &mut checked.view.nodes {
        let Some(result) = report
            .modules
            .iter()
            .find(|m| node.object_id.as_deref() == Some(m.module.as_str()))
        else {
            continue;
        };
        let source = Document::parse(&node.content)?;
        let state = match result.status {
            VerificationStatus::Passed => "通过",
            VerificationStatus::Failed => "失败",
            VerificationStatus::NoTests => "无测试（未发现验证命令）",
            VerificationStatus::TimedOut => "超时",
            VerificationStatus::Cancelled => "已取消",
            VerificationStatus::NotRun => "尚未执行",
        };
        let summary = format!("初始验证：{state}；候选尚未批准。");
        let body = source
            .body()
            .replace("候选验证命令尚未执行，不构成证据。", &summary)
            .replace("未发现验证命令；补测试任务仅为候选。", &summary);
        let mut object = Document::new("Module", &body)?;
        for (key, value) in source.front() {
            object.set(key.as_str().ok_or("模块字段名无效")?, value.clone())?;
        }
        object.set(
            "verification",
            serde_yaml::to_value(&result.status).map_err(|e| e.to_string())?,
        )?;
        object.set(
            "initial_evidence",
            serde_yaml::to_value(serde_json::json!({
                "actor": report.actor, "tree": report.tree, "revision": report.revision,
                "scan": checked.id, "report_sha256": digest,
                "started_at": report.started_at, "finished_at": report.finished_at,
                "status": result.status, "commands": result.command_ids,
                "shared": result.shared, "cleanup_complete": report.cleanup_complete
            }))
            .map_err(|e| e.to_string())?,
        )?;
        node.content = object.to_text();
    }
    checked
        .diagnostics
        .retain(|message| !message.starts_with("仅 S1–S4 候选："));
    checked.diagnostics.push(
        "S5 结果来自固定提交的实际命令运行；尚未完成 S6 综合和 S8 批准，不认证任务完成。".into(),
    );
    if !report.cleanup_complete {
        checked
            .diagnostics
            .push("验证进程或临时工作树未完整清理，不能接受通过结果；请查看日志诊断。".into());
    }
    checked.view.diagnostics = checked.diagnostics.clone();
    *scan = checked;
    Ok(())
}
