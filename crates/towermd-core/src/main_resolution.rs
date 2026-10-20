//! Explicit human resolution of a fixed, unauthorized main commit.
//! This never creates a baseline or authenticates system state/configuration.
use std::{collections::BTreeMap, path::Path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::{events, hub::Event, main_writer::{WriteKind, WriteRequest},
    okf::Document, path_guard::{self, ChangedPath, Rule}, snapshot_publication::{bytes, text}};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation { Claim, Revert }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub version: u32,
    pub request_id: String,
    pub operation: Operation,
    pub project_id: String,
    pub docs_dir: String,
    pub import_anchor: String,
    pub main_revision: String,
    pub target_commit: String,
    pub parent_commit: String,
    pub changes: Vec<ChangedPath>,
    pub diff: String,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub operation: Operation,
    pub target_commit: String,
    pub resolution_commit: String,
    pub approval_eid: String,
    pub content_hash: String,
}

impl Preview {
    pub fn hash(&self) -> Result<String, String> {
        let mut input = self.clone();
        input.content_hash.clear();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&input).map_err(|error| error.to_string())?)))
    }
    pub fn validate_approval(&self, event: &Event) -> Result<(), String> {
        let id = uuid::Uuid::parse_str(&self.request_id).map_err(|_| "人工确认 ID 无效")?;
        if self.version != 1 || id.get_version_num() != 7 || id.to_string() != self.request_id
            || self.project_id.trim().is_empty() || self.docs_dir.trim().is_empty() || self.changes.is_empty()
        { return Err("人工确认预览格式无效".into()); }
        for oid in [&self.import_anchor, &self.main_revision, &self.target_commit, &self.parent_commit] {
            events::derived_eid(oid, "main.change.resolved")?;
        }
        if event.kind != "main.resolution.approved" || event.actor != "human:local"
            || event.layer != "L1" || event.obj != self.target_commit || event.eid != self.request_id
            || event.stale || !event.guards.is_empty() || event.epoch.is_some()
            || event.data != serde_json::to_value(self).map_err(|error| error.to_string())?
            || self.hash()? != self.content_hash
        { return Err("主干处理缺少与冻结预览一致的 App 人工确认原件".into()); }
        Ok(())
    }
}

pub fn from_approval(event: &Event) -> Result<Preview, String> {
    let preview: Preview = serde_json::from_value(event.data.clone()).map_err(|error| error.to_string())?;
    preview.validate_approval(event)?;
    Ok(preview)
}

pub fn prepare(
    root: &Path, main: &str, project_id: &str, docs_dir: &str,
    target: &str, operation: Operation, request_id: &str,
) -> Result<Preview, String> {
    let id = uuid::Uuid::parse_str(request_id).map_err(|_| "确认请求 ID 无效")?;
    if id.get_version_num() != 7 || id.to_string() != request_id {
        return Err("人工确认请求必须使用规范 UUIDv7".into());
    }
    let history = path_guard::inspect_main(root, main, project_id, docs_dir, None)?;
    let commit = history.commits.iter().find(|entry| entry.commit == target)
        .ok_or("目标提交不在当前项目启用后的完整 main 历史中")?;
    if commit.parents.len() != 1 {
        return Err("自动处理仅支持单父提交；合并提交需要明确的主线方案".into());
    }
    let parent = &commit.parents[0];
    let report = path_guard::check(root, parent, target, docs_dir, &[])?;
    if !report.violations.iter().any(|item| matches!(item.rule, Rule::ProtectedPath | Rule::SlowObject)) {
        return Err("目标没有待处理的受保护改动".into());
    }
    for change in &report.changes {
        let path = change.path.to_ascii_lowercase();
        let docs = docs_dir.to_ascii_lowercase();
        if path == ".towermd" || path.starts_with(".towermd/")
            || ["state", "baselines"].iter().any(|directory| {
                path == format!("{docs}/{directory}") || path.starts_with(&format!("{docs}/{directory}/"))
            })
        { return Err(format!("系统配置、快照与基线清单不能经人工认领或普通撤销改变：{}", change.path)); }
        let rules: Vec<_> = report.violations.iter().filter(|item| item.path == change.path).collect();
        if rules.iter().any(|item| matches!(item.rule, Rule::InvalidObject | Rule::UnsupportedEntry)) {
            return Err(format!("对象层级或文件模式无法完整核验：{}", change.path));
        }
        if rules.iter().any(|item| item.rule == Rule::ProtectedPath)
            && path != format!("{docs}/tower.canvas")
            && !rules.iter().any(|item| item.rule == Rule::SlowObject)
        { return Err(format!("只能认领或撤销慢层用户对象与基线画布：{}", change.path)); }
        for (mode, oid) in [(&change.old_mode, &change.old_oid), (&change.new_mode, &change.new_oid)] {
            if mode == "000000" { continue; }
            if operation == Operation::Revert && mode != "100644" {
                return Err(format!("当前不能自动撤销 {mode} 模式，必须保留原文件权限：{}", change.path));
            }
            if path.ends_with(".md") {
                let content = blob(root, oid)?;
                if let Ok(document) = std::str::from_utf8(&content).map_err(|_| ()).and_then(|text| Document::parse(text).map_err(|_| ())) {
                    if document.doc_type() == Some("Baseline") {
                        return Err("人工处理不能批准或改写 Baseline 清单".into());
                    }
                }
            }
        }
        if operation == Operation::Revert {
            let actual = tree_entry(root, main, &change.path)?;
            let expected = (change.new_mode != "000000").then(|| (change.new_mode.clone(), change.new_oid.clone()));
            if actual != expected {
                return Err(format!("目标之后同一路径已有改动，不能自动撤销：{}", change.path));
            }
        }
    }
    let diff = bytes(root, &["diff", "--binary", "--full-index", "--no-ext-diff", "--no-textconv", "--no-renames", parent, target, "--"])?;
    if diff.len() > 2 * 1024 * 1024 { return Err("待确认 diff 超过 2 MiB，无法完整展示，不能自动处理".into()); }
    let diff = String::from_utf8(diff).map_err(|_| "待确认 diff 不是 UTF-8，无法完整展示")?;
    let mut preview = Preview {
        version: 1, request_id: request_id.into(), operation, project_id: project_id.into(),
        docs_dir: docs_dir.into(), import_anchor: history.import_anchor, main_revision: main.into(),
        target_commit: target.into(), parent_commit: parent.clone(), changes: report.changes, diff,
        content_hash: String::new(),
    };
    preview.content_hash = preview.hash()?;
    Ok(preview)
}

/// Re-read immutable Git inputs, including both sides of every object's layer.
pub fn verify_preview(root: &Path, preview: &Preview) -> Result<(), String> {
    let actual = prepare(root, &preview.main_revision, &preview.project_id, &preview.docs_dir,
        &preview.target_commit, preview.operation, &preview.request_id)?;
    if actual != *preview { return Err("预览与实际项目来源、提交 diff 或摘要不一致".into()); }
    Ok(())
}

pub fn revert_request(root: &Path, preview: &Preview, approval: &Event, epoch: u64) -> Result<WriteRequest, String> {
    preview.validate_approval(approval)?;
    verify_preview(root, preview)?;
    if preview.operation != Operation::Revert { return Err("此确认不是撤销操作".into()); }
    let mut changes = BTreeMap::new();
    for change in &preview.changes {
        changes.insert(change.path.clone(), if change.old_mode == "000000" { None } else { Some(blob(root, &change.old_oid)?) });
    }
    Ok(WriteRequest {
        docs_dir: preview.docs_dir.clone(), epoch,
        kind: WriteKind::Slow { expected_base: preview.main_revision.clone(), tag: None },
        changes,
        message: format!("Revert unauthorized main change {}\n\nTowermd-Resolution: {}\nTowermd-Approval: {}\nTowermd-Content: {}",
            preview.target_commit, preview.target_commit, preview.request_id, preview.content_hash),
    })
}

pub fn prove_commit(root: &Path, preview: &Preview, approval: &Event, resolved: &str) -> Result<Resolution, String> {
    preview.validate_approval(approval)?;
    verify_preview(root, preview)?;
    events::derived_eid(resolved, "main.change.resolved")?;
    match preview.operation {
        Operation::Claim if resolved != preview.target_commit => return Err("认领只能指向原固定提交".into()),
        Operation::Claim => {}
        Operation::Revert => {
            let parents = text(root, &["rev-list", "--parents", "-n", "1", resolved])?;
            let parents: Vec<_> = parents.split_whitespace().collect();
            if parents.len() != 2 || parents[1] != preview.main_revision {
                return Err("撤销提交没有直接承接已批准的 main 来源".into());
            }
            let message = text(root, &["show", "-s", "--format=%B", resolved])?;
            for expected in [
                format!("Towermd-Resolution: {}", preview.target_commit),
                format!("Towermd-Approval: {}", preview.request_id),
                format!("Towermd-Content: {}", preview.content_hash),
            ] {
                let key = expected.split_once(':').unwrap().0;
                let lines: Vec<_> = message.lines().filter(|line| line.starts_with(&format!("{key}:"))).collect();
                if lines != [expected.as_str()] { return Err("撤销提交 trailer 与人工确认不一致".into()); }
            }
            if text(root, &["show", "-s", "--format=%an", resolved])? != "process:towermd" {
                return Err("撤销提交必须由唯一主干写入者生成".into());
            }
            let actual = path_guard::check(root, &preview.main_revision, resolved, &preview.docs_dir, &[])?;
            if actual.changes.len() != preview.changes.len() {
                return Err("撤销提交改动范围与已确认逆向 diff 不一致".into());
            }
            for change in &preview.changes {
                let reversed = actual.changes.iter().find(|entry| entry.path == change.path)
                    .ok_or("撤销缺少已确认路径")?;
                if reversed.old_mode != change.new_mode || reversed.new_mode != change.old_mode
                    || reversed.old_oid != change.new_oid || reversed.new_oid != change.old_oid
                { return Err(format!("撤销未精确恢复原字节与模式：{}", change.path)); }
            }
        }
    }
    Ok(Resolution {
        operation: preview.operation, target_commit: preview.target_commit.clone(),
        resolution_commit: resolved.into(), approval_eid: approval.eid.clone(), content_hash: preview.content_hash.clone(),
    })
}

pub fn prove_resolution(root: &Path, project_id: &str, docs_dir: &str, event: &Event, approval: &Event) -> Result<Resolution, String> {
    let resolution: Resolution = serde_json::from_value(event.data.clone()).map_err(|error| error.to_string())?;
    let preview = from_approval(approval)?;
    if preview.project_id != project_id || preview.docs_dir != docs_dir
        || event.eid != events::derived_eid(&resolution.target_commit, "main.change.resolved")?
        || event.kind != "main.change.resolved" || event.obj != resolution.target_commit || event.layer != "L1"
        || !matches!(event.actor.as_str(), "process:towermd" | "process:towermd-recovery")
        || event.stale || event.guards.len() != 1 || event.guards[0].name != "main"
        || event.epoch != Some(event.guards[0].epoch)
    { return Err("主干处理原件的项目、身份或主干租约无效".into()); }
    let actual = prove_commit(root, &preview, approval, &resolution.resolution_commit)?;
    if actual != resolution { return Err("主干处理原件与实际人工确认、Git 提交不一致".into()); }
    Ok(actual)
}

/// Find an already-published inverse after an unknown push result. The trailer
/// merely locates candidates; the complete proof above authenticates each one.
pub fn find_revert(root: &Path, main: &str, preview: &Preview, approval: &Event) -> Result<Option<String>, String> {
    if preview.operation != Operation::Revert { return Err("不是撤销确认".into()); }
    if !is_ancestor(root, &preview.main_revision, main)? { return Err("main 不再继承已批准来源".into()); }
    let range = format!("{}..{main}", preview.main_revision);
    let commits = text(root, &["rev-list", &range])?;
    let marker = format!("Towermd-Approval: {}", approval.eid);
    let mut found = None;
    for commit in commits.lines() {
        let message = text(root, &["show", "-s", "--format=%B", commit])?;
        if !message.lines().any(|line| line == marker) { continue; }
        prove_commit(root, preview, approval, commit)?;
        if found.replace(commit.to_owned()).is_some() { return Err("同一撤销确认对应多个实际提交，需复核".into()); }
    }
    Ok(found)
}

pub fn is_ancestor(root: &Path, ancestor: &str, head: &str) -> Result<bool, String> {
    events::derived_eid(ancestor, "main.change.resolved")?;
    events::derived_eid(head, "main.change.resolved")?;
    let base = text(root, &["merge-base", ancestor, head])?;
    Ok(base == ancestor)
}

fn blob(root: &Path, oid: &str) -> Result<Vec<u8>, String> {
    let size: usize = text(root, &["cat-file", "-s", oid])?.parse().map_err(|_| "Git blob 大小无效")?;
    if size > 16 * 1024 * 1024 { return Err("自动撤销单个文件上限为 16 MiB".into()); }
    bytes(root, &["cat-file", "blob", oid])
}
fn tree_entry(root: &Path, commit: &str, path: &str) -> Result<Option<(String, String)>, String> {
    let listing = bytes(root, &["--literal-pathspecs", "ls-tree", "-z", commit, "--", path])?;
    if listing.is_empty() { return Ok(None); }
    let value = std::str::from_utf8(&listing[..listing.len() - 1]).map_err(|_| "Git 路径不是 UTF-8")?;
    let (metadata, actual_path) = value.split_once('\t').ok_or("Git tree 条目无效")?;
    let fields: Vec<_> = metadata.split_whitespace().collect();
    if actual_path != path || fields.len() != 3 || fields[1] != "blob" { return Err("路径不是唯一普通文件".into()); }
    Ok(Some((fields[0].into(), fields[2].into())))
}
