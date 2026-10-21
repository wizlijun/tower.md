//! Frozen publication previews and the shared reader for approved baselines.
//! A tag or a Module document alone never activates a baseline.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{baseline::BaselineDraft, draft_store, review_verification, verifier::VerificationStatus};
use towermd_core::{hub::Event, okf::Document, repo};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub request_id: String,
    pub draft_id: String,
    pub project_id: String,
    pub source_revision: String,
    pub target: String,
    pub prepared_at: String,
    pub content_hash: String,
    pub files: BTreeMap<String, Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removals: Vec<String>,
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}

fn target_number(target: &str) -> Result<u64, String> {
    let number: u64 = target
        .strip_prefix('B')
        .ok_or("基线编号无效")?
        .parse()
        .map_err(|_| "基线编号无效")?;
    if target != format!("B{number}") {
        return Err("基线编号必须是规范形式".into());
    }
    Ok(number)
}

pub fn preview_time() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn prepare(
    draft: &BaselineDraft,
    common_dir: &Path,
    request_id: &str,
    target: &str,
    prepared_at: &str,
) -> Result<Publication, String> {
    let id = uuid::Uuid::parse_str(request_id).map_err(|_| "批准请求 ID 无效")?;
    if id.get_version_num() != 7 || id.to_string() != request_id {
        return Err("批准请求须使用 UUIDv7".into());
    }
    target_number(target)?;
    chrono::DateTime::parse_from_rfc3339(prepared_at).map_err(|_| "批准预览时间无效")?;
    let review = draft.review.as_ref().ok_or("请先审阅并确认模块验证命令")?;
    if !review.pending_modules.is_empty()
        || review
            .modules
            .iter()
            .any(|m| m.verification_pending || !m.commands_confirmed)
    {
        return Err("所有模块须确认验证命令，修改后的模块须重新验证".into());
    }
    let evidence = review.verification.as_ref().ok_or("请先验证当前审阅草稿")?;
    review_verification::validate_logs(draft, common_dir)?;
    if !evidence.report.cleanup_complete
        || evidence.report.modules.iter().any(|m| {
            !matches!(
                m.status,
                VerificationStatus::Passed | VerificationStatus::NoTests
            )
        })
    {
        return Err("验证未通过，不能出版基线；无测试模块须保留补测试任务".into());
    }
    let docs = draft
        .scan
        .view
        .docs_dir
        .as_deref()
        .ok_or("草稿缺少文档目录")?;
    let mut files = draft_store::files(draft)?;
    let mut objects = BTreeMap::new();
    for node in &draft.view.nodes {
        if let (Some(object_id), Some(path)) = (&node.object_id, &node.file) {
            let mut doc = Document::parse(&node.content)?;
            doc.set("status", "stable".into())?;
            doc.set("baseline", target.into())?;
            doc.set(
                "verified",
                serde_yaml::to_value(json!({
                    "by":"human:local","at":prepared_at,
                }))
                .map_err(|e| e.to_string())?,
            )?;
            if node.object_type.as_deref() == Some("Module") {
                let module = review
                    .modules
                    .iter()
                    .find(|m| &m.id == object_id)
                    .ok_or("模块审阅记录缺失")?;
                if module.commands.is_empty() {
                    doc.set("suggested_task", "补充模块验证测试".into())?;
                }
            }
            let text = doc.to_text();
            let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
            let full_path = format!("{docs}/{path}");
            objects.insert(object_id.clone(), json!({"path":full_path,"sha256":hash}));
            files.insert(full_path, text.into_bytes());
        }
    }
    // Baseline manifest is part of the content hash; it does not hash itself.
    let mut manifest = Document::new("Baseline", "经 App 人工审阅确认的基线。\n")?;
    for (key, value) in [
        ("id", json!(target)),
        ("layer", json!("L1")),
        ("status", json!("stable")),
        ("objects", json!(objects)),
        ("approved_by", json!("human:local")),
        ("approved_at", json!(prepared_at)),
        ("git_tag", json!(format!("towermd/{target}"))),
        ("draft_id", json!(draft.id)),
        ("approval_eid", json!(request_id)),
        ("source_revision", json!(draft.scan.revision)),
        (
            "canvas_sha256",
            json!(format!(
                "{:x}",
                Sha256::digest(
                    files
                        .get(&format!("{docs}/tower.canvas"))
                        .ok_or("草稿缺少画布")?
                )
            )),
        ),
    ] {
        manifest.set(key, serde_yaml::to_value(value).map_err(|e| e.to_string())?)?;
    }
    files.insert(
        format!("{docs}/baselines/{target}.md"),
        manifest.to_text().into_bytes(),
    );
    let mut publication = Publication {
        request_id: request_id.into(),
        draft_id: draft.id.clone(),
        project_id: draft.scan.project_id.clone(),
        source_revision: draft.scan.revision.clone(),
        target: target.into(),
        prepared_at: prepared_at.into(),
        content_hash: String::new(),
        files,
        removals: Vec::new(),
    };
    publication.content_hash = publication.hash()?;
    Ok(publication)
}

impl Publication {
    pub fn hash(&self) -> Result<String, String> {
        if !self.removals.is_empty() {
            return digest(&("baseline-publication-v2",&self.request_id,&self.draft_id,&self.project_id,
                &self.source_revision,&self.target,&self.prepared_at,&self.files,&self.removals));
        }
        digest(&(
            "baseline-publication-v1",
            &self.request_id,
            &self.draft_id,
            &self.project_id,
            &self.source_revision,
            &self.target,
            &self.prepared_at,
            &self.files,
        ))
    }

    pub fn check_approval(&self, event: &Event) -> Result<(), String> {
        if event.stale
            || event.actor != "human:local"
            || event.kind != "baseline.approved"
            || event.eid != self.request_id
            || event.obj != self.target
            || event.layer != "L1"
            || event.data != serde_json::to_value(self).map_err(|e| e.to_string())?
            || self.hash()? != self.content_hash
        {
            return Err("批准原件与冻结出版内容不一致".into());
        }
        Ok(())
    }

    /// Check actual committed bytes and authoritative tag/main reachability
    /// against the exact accepted approval. A cached local tag is insufficient
    /// when this repository has an origin.
    pub fn prove_commit(&self, root: &Path, commit: &str, approval: &Event) -> Result<(), String> {
        self.check_approval(approval)?;
        target_number(&self.target)?;
        let tag = format!("refs/tags/towermd/{}", self.target);
        let publication =
            towermd_core::main_writer::read_publication(root, &tag)?.ok_or("权威基线标签尚不存在")?;
        if publication.commit != commit {
            return Err("权威基线标签没有指向批准的出版提交".into());
        }
        let parents = repo::git_timeout(
            root,
            &["rev-list", "--parents", "-n", "1", commit],
            Duration::from_secs(30),
        )?;
        let parents: Vec<_> = parents.split_whitespace().collect();
        if parents.len() != 2 || parents[1] != self.source_revision {
            return Err("基线出版提交必须直接承接批准的来源提交".into());
        }
        let changes = repo::git_timeout(
            root,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", commit],
            Duration::from_secs(30),
        )?;
        if changes.lines().any(|path| !self.files.contains_key(path) && !self.removals.iter().any(|removed|removed==path)) {
            return Err("基线提交包含未经批准的文件".into());
        }
        let mut unique=std::collections::BTreeSet::new();
        for path in &self.removals {
            if !unique.insert(path)||self.files.contains_key(path) {
                return Err("出版删除路径重复或同时被写入".into());
            }
            let old=repo::git_timeout(root,&["ls-tree",&self.source_revision,"--",path],Duration::from_secs(30))?;
            let actual=repo::git_timeout(root,&["ls-tree",commit,"--",path],Duration::from_secs(30))?;
            if !old.starts_with("100644 blob ")||!actual.is_empty(){return Err("出版删除对象没有按批准内容完整移除".into());}
        }
        for (path, expected) in &self.files {
            let mode = repo::git_timeout(
                root,
                &["ls-tree", commit, "--", path],
                Duration::from_secs(30),
            )?;
            if !mode.starts_with("100644 blob ") {
                return Err("基线对象不是普通文件".into());
            }
            let object = format!("{commit}:{path}");
            let output = repo::git_timeout_env_input_output(
                root,
                &["cat-file", "blob", &object],
                Duration::from_secs(30),
                &[],
                None,
            )?;
            if !output.status.success() || &output.stdout != expected {
                return Err(format!("基线文件与批准内容不一致：{path}"));
            }
        }
        Ok(())
    }
}

pub fn from_approval(event: &Event) -> Result<Publication, String> {
    let publication: Publication =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    publication.check_approval(event)?;
    Ok(publication)
}

pub fn verified_baseline(
    root: &Path,
    commit: &str,
    approval: &Event,
) -> Result<crate::task_model::VerifiedBaseline, String> {
    use crate::task_model::{ModuleScope, PublishedBaseline, VerifiedBaseline};
    let publication = from_approval(approval)?;
    publication.prove_commit(root, commit, approval)?;
    let mut modules = BTreeMap::new();
    let mut decisions = std::collections::BTreeSet::new();
    for (path, bytes) in &publication.files {
        if !path.ends_with(".md") {
            continue;
        }
        let document = Document::parse(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)?;
        match document.doc_type() {
            Some("Module") => {
                let id = document.get_str("id").ok_or("出版模块缺少 ID")?.to_owned();
                let paths: Vec<String> = serde_yaml::from_value(
                    document.get("paths").cloned().ok_or("出版模块缺少路径")?,
                )
                .map_err(|e| e.to_string())?;
                if modules.insert(id, ModuleScope { paths }).is_some() {
                    return Err("出版模块 ID 重复".into());
                }
            }
            Some("Decision") => {
                decisions.insert(document.get_str("id").ok_or("出版决策缺少 ID")?.to_owned());
            }
            _ => {}
        }
    }
    VerifiedBaseline::from_published(PublishedBaseline {
        tag: format!("towermd/{}", publication.target),
        commit: commit.into(),
        approval_eid: approval.eid.clone(),
        content_hash: publication.content_hash,
        modules,
        decisions,
    })
}
