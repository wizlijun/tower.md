//! Immutable S6 originals in private Git refs; projections never own unique data.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::baseline::{self, BaselineDraft, Submission};
use crate::catalog::Catalog;
use crate::cold_start::ScanReport;
use crate::review::{self, ReviewAction};
use crate::{cold_start, repo};

const PREFIX: &str = "refs/towermd/drafts/";
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    id: String,
    producer: String,
    created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review: Option<ReviewInput>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewInput {
    base_id: String,
    base_created_at: String,
    actor: String,
    actions: Vec<ReviewAction>,
}

pub fn valid_id(id: &str) -> Result<(), String> {
    let digest = id.strip_prefix("d-").ok_or("综合草稿 ID 无效")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("综合草稿 ID 无效".into());
    }
    Ok(())
}

pub fn submission_id(submission: &Submission, producer: &str) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(submission, producer)).map_err(|e| e.to_string())?;
    Ok(format!("d-{}", hex::encode(Sha256::digest(bytes))))
}

fn git(
    root: &Path,
    args: &[&str],
    index: Option<&Path>,
    input: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let mut options = vec![
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "commit.gpgsign=false",
    ];
    options.extend_from_slice(args);
    let mut environment = vec![
        ("GIT_AUTHOR_NAME", OsStr::new("process:towermd-cli")),
        ("GIT_AUTHOR_EMAIL", OsStr::new("towermd@localhost")),
        ("GIT_COMMITTER_NAME", OsStr::new("process:towermd-cli")),
        ("GIT_COMMITTER_EMAIL", OsStr::new("towermd@localhost")),
    ];
    if let Some(index) = index {
        environment.push(("GIT_INDEX_FILE", index.as_os_str()));
    }
    let output = repo::git_timeout_env_input_output(root, &options, TIMEOUT, &environment, input)?;
    if !output.status.success() {
        return Err(format!(
            "草稿 Git 操作失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn oid(bytes: Vec<u8>) -> Result<String, String> {
    let value = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let value = value.trim();
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("草稿 Git 对象 ID 无效".into());
    }
    Ok(value.into())
}

pub fn private_commit(root: &Path, id: &str) -> Result<Option<String>, String> {
    valid_id(id)?;
    let reference = format!("{PREFIX}{id}");
    let refs = repo::git_timeout(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            &reference,
        ],
        TIMEOUT,
    )?;
    Ok(refs.lines().find_map(|line| {
        let (name, sha) = line.split_once(' ')?;
        (name == reference).then(|| sha.to_owned())
    }))
}

fn original_path(id: &str, name: &str) -> String {
    format!(".towermd/drafts/{id}/{name}.json")
}

fn read_blob(root: &Path, commit: &str, path: &str) -> Result<Vec<u8>, String> {
    let object = format!("{commit}:{path}");
    let size = repo::git_timeout(root, &["cat-file", "-s", &object], TIMEOUT)?
        .parse::<usize>()
        .map_err(|_| "草稿对象大小无效")?;
    if size > 16 * 1024 * 1024 {
        return Err("综合草稿原件超过 16 MiB".into());
    }
    git(root, &["cat-file", "blob", &object], None, None)
}

fn files(draft: &BaselineDraft) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let docs = draft
        .scan
        .view
        .docs_dir
        .as_deref()
        .ok_or("扫描缺少文档目录")?;
    let mut files = BTreeMap::new();
    for node in &draft.view.nodes {
        if let Some(path) = node.file.as_ref().filter(|_| node.object_id.is_some()) {
            if files
                .insert(format!("{docs}/{path}"), node.content.as_bytes().to_vec())
                .is_some()
            {
                return Err("草稿对象使用了重复文件路径".into());
            }
        }
    }
    for node in &draft.view.nodes {
        if node.object_id.is_none()
            && node
                .file
                .as_ref()
                .is_some_and(|path| files.contains_key(path))
        {
            return Err("Markdown 引用与草稿对象输出路径冲突；不能用新对象覆盖引用原文".into());
        }
    }
    // Use the same JSON Canvas encoder as the scan; the draft has its own view.
    let mut projection = draft.scan.clone();
    projection.view = draft.view.clone();
    let mut canvas = cold_start::canvas_json(&projection)?;
    if draft.review.is_some() {
        // Published JSON Canvas file paths are repository-relative. A reference
        // retains its original path and never becomes a copied object document.
        let mut value: serde_json::Value =
            serde_json::from_str(&canvas).map_err(|e| e.to_string())?;
        for (raw, node) in value["nodes"]
            .as_array_mut()
            .ok_or("缺少节点")?
            .iter_mut()
            .zip(&draft.view.nodes)
        {
            if node.object_id.is_some() {
                raw["file"] = serde_json::json!(format!(
                    "{docs}/{}",
                    node.file.as_deref().ok_or("对象缺少路径")?
                ));
            } else {
                raw.as_object_mut().ok_or("节点无效")?.remove("towermd");
            }
        }
        for (raw, edge) in value["edges"]
            .as_array_mut()
            .ok_or("缺少关系")?
            .iter_mut()
            .zip(&draft.view.edges)
        {
            raw["fromEnd"] = serde_json::json!(edge.from_end.as_deref().unwrap_or("none"));
            raw["toEnd"] = serde_json::json!(edge.to_end.as_deref().unwrap_or("none"));
        }
        canvas = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    }
    files.insert(format!("{docs}/tower.canvas"), canvas.into_bytes());
    Ok(files)
}

/// Rebuild from immutable input, not a cached view. Missing logs are reported by
/// the caller separately so the original synthesis remains readable.
pub fn load(catalog: &Catalog, id: &str) -> Result<BaselineDraft, String> {
    let commit = private_commit(&catalog.root, id)?.ok_or("综合草稿不存在")?;
    let metadata: Metadata = serde_json::from_slice(&read_blob(
        &catalog.root,
        &commit,
        &original_path(id, "metadata"),
    )?)
    .map_err(|e| e.to_string())?;
    let submission = baseline::parse(
        serde_json::from_slice(&read_blob(
            &catalog.root,
            &commit,
            &original_path(id, "submission"),
        )?)
        .map_err(|e| e.to_string())?,
    )?;
    let scan: ScanReport = serde_json::from_slice(&read_blob(
        &catalog.root,
        &commit,
        &original_path(id, "scan"),
    )?)
    .map_err(|e| e.to_string())?;
    let base_id = submission_id(&submission, &metadata.producer)?;
    let expected_id = match &metadata.review {
        Some(input) if input.base_id == base_id => review_id(input)?,
        Some(_) => return Err("审阅草稿的综合原件不匹配".into()),
        None => base_id.clone(),
    };
    if metadata.id != id
        || expected_id != id
        || scan.project_id != catalog.config.project_id()
        || scan.view.docs_dir.as_deref() != Some(catalog.config.docs_dir())
    {
        return Err("综合草稿原件与项目或摘要不一致".into());
    }
    let tree = repo::git_timeout(
        &catalog.root,
        &["rev-parse", &format!("{}^{{tree}}", scan.revision)],
        TIMEOUT,
    )?;
    if tree != scan.tree {
        return Err("综合草稿源 tree 不一致".into());
    }
    let parents = repo::git_timeout(
        &catalog.root,
        &["rev-list", "--parents", "-n", "1", &commit],
        TIMEOUT,
    )?;
    if parents.split_whitespace().collect::<Vec<_>>() != [commit.as_str(), scan.revision.as_str()] {
        return Err("综合草稿不以原扫描提交为唯一父提交".into());
    }
    let base = baseline::synthesize(
        &scan,
        submission,
        &metadata.producer,
        metadata
            .review
            .as_ref()
            .map(|r| r.base_created_at.as_str())
            .unwrap_or(&metadata.created_at),
        &base_id,
    )?;
    let draft = match &metadata.review {
        Some(input) => review::replay(
            &base,
            &input.actions,
            &input.actor,
            id,
            &metadata.created_at,
            &references(&catalog.root, &scan.revision, &input.actions)?,
        )?,
        None => base,
    };
    let output = files(&draft)?;
    let mut allowed: std::collections::BTreeSet<_> = output.keys().cloned().collect();
    for name in ["metadata", "submission", "scan"] {
        allowed.insert(original_path(id, name));
    }
    let changed = git(
        &catalog.root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-z",
            &scan.revision,
            &commit,
        ],
        None,
        None,
    )?;
    for path in changed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        if !allowed.contains(std::str::from_utf8(path).map_err(|_| "草稿路径不是 UTF-8")?) {
            return Err("综合草稿包含输入和对象之外的源码变更".into());
        }
    }
    for path in &allowed {
        let entry = git(
            &catalog.root,
            &["ls-tree", "-z", &commit, "--", path],
            None,
            None,
        )?;
        if !entry.starts_with(b"100644 blob ") {
            return Err("综合草稿文件类型或权限无效".into());
        }
    }
    for (path, bytes) in output {
        if read_blob(&catalog.root, &commit, &path)? != bytes {
            return Err("综合草稿投影与耐久原件不一致".into());
        }
    }
    Ok(draft)
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn scratch(common: &Path) -> Result<Scratch, String> {
    let parent = common.join("towermd");
    match fs::symlink_metadata(&parent) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => return Err("草稿临时目录不是普通目录".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&parent).map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    }
    let path = parent.join(format!(".draft-index-{}", Uuid::now_v7()));
    fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(Scratch(path))
}

pub fn save(
    catalog: &Catalog,
    scan: &ScanReport,
    submission: Submission,
    producer: &str,
) -> Result<BaselineDraft, String> {
    let id = submission_id(&submission, producer)?;
    if private_commit(&catalog.root, &id)?.is_some() {
        return load(catalog, &id);
    }
    if catalog.revision != scan.revision || !catalog.matches_current_source()? {
        return Err("权威来源已改变，请重新扫描后综合".into());
    }
    cold_start::validate_verification_logs(scan, &catalog.common_dir)?;
    let draft = baseline::synthesize(scan, submission, producer, &Utc::now().to_rfc3339(), &id)?;
    persist(catalog, draft, None)
}

fn review_id(input: &ReviewInput) -> Result<String, String> {
    let bytes = serde_json::to_vec(&("s7-review-v1", input)).map_err(|e| e.to_string())?;
    Ok(format!("d-{}", hex::encode(Sha256::digest(bytes))))
}

fn references(
    root: &Path,
    revision: &str,
    actions: &[ReviewAction],
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for action in actions {
        if let ReviewAction::Reference { path, .. } = action {
            if path.is_empty()
                || path.starts_with('/')
                || path.contains('\\')
                || path.contains(':')
                || path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".." || p == ".git")
                || !path.to_ascii_lowercase().ends_with(".md")
                || path.chars().any(char::is_control)
            {
                return Err("引用必须是项目内 Markdown 文件的相对路径".into());
            }
            let entry = git(root, &["ls-tree", "-z", revision, "--", path], None, None)?;
            if !entry.starts_with(b"100644 blob ") && !entry.starts_with(b"100755 blob ") {
                return Err("引用必须是固定源提交中已有的普通 Markdown 文件".into());
            }
            result.insert(
                path.clone(),
                String::from_utf8(read_blob(root, revision, path)?)
                    .map_err(|_| "引用 Markdown 不是 UTF-8")?,
            );
        }
    }
    Ok(result)
}

/// App adapter supplies the fixed local human identity, never caller JSON.
/// This only saves an unapproved draft and cannot publish a baseline.
pub fn save_review(
    catalog: &Catalog,
    parent_id: &str,
    action: ReviewAction,
) -> Result<BaselineDraft, String> {
    persist_review(catalog, prepare_review(catalog, parent_id, action)?)
}

pub struct PreparedReview {
    draft: BaselineDraft,
    input: ReviewInput,
    existing: bool,
}

/// No mutation. Failures here prove that this request did not save a revision.
pub fn prepare_review(
    catalog: &Catalog,
    parent_id: &str,
    action: ReviewAction,
) -> Result<PreparedReview, String> {
    let parent = load(catalog, parent_id)?;
    let base_id = submission_id(&parent.submission, &parent.producer)?;
    let mut actions = parent
        .review
        .as_ref()
        .map(|r| r.actions.clone())
        .unwrap_or_default();
    actions.push(action);
    if actions.len() > 1024 {
        return Err("草稿审阅已超过 1024 次修改".into());
    }
    let base = load(catalog, &base_id)?;
    let input = ReviewInput {
        base_id: base_id.clone(),
        base_created_at: base.created_at.clone(),
        actor: "human:local".into(),
        actions,
    };
    let id = review_id(&input)?;
    // Retry the same original request, even if main moved after it succeeded.
    if private_commit(&catalog.root, &id)?.is_some() {
        return Ok(PreparedReview {
            draft: load(catalog, &id)?,
            input,
            existing: true,
        });
    }
    if catalog.revision != parent.scan.revision || !catalog.matches_current_source()? {
        return Err("草稿来源已过期，请重新扫描综合后审阅".into());
    }
    let draft = review::replay(
        &base,
        &input.actions,
        &input.actor,
        &id,
        &Utc::now().to_rfc3339(),
        &references(&catalog.root, &base.scan.revision, &input.actions)?,
    )?;
    files(&draft)?;
    Ok(PreparedReview {
        draft,
        input,
        existing: false,
    })
}

pub fn persist_review(
    catalog: &Catalog,
    prepared: PreparedReview,
) -> Result<BaselineDraft, String> {
    if prepared.existing {
        return load(catalog, &prepared.draft.id);
    }
    persist(catalog, prepared.draft, Some(prepared.input))
}

fn persist(
    catalog: &Catalog,
    draft: BaselineDraft,
    review: Option<ReviewInput>,
) -> Result<BaselineDraft, String> {
    let id = draft.id.clone();
    let scan = &draft.scan;
    let temporary = scratch(&catalog.common_dir)?;
    let index = temporary.0.join("index");
    git(
        &catalog.root,
        &["read-tree", &scan.revision],
        Some(&index),
        None,
    )?;
    let mut output = files(&draft)?;
    output.insert(
        original_path(&id, "submission"),
        serde_json::to_vec(&draft.submission).map_err(|e| e.to_string())?,
    );
    output.insert(
        original_path(&id, "scan"),
        serde_json::to_vec(scan).map_err(|e| e.to_string())?,
    );
    output.insert(
        original_path(&id, "metadata"),
        serde_json::to_vec(&Metadata {
            id: id.clone(),
            producer: draft.producer.clone(),
            created_at: draft.created_at.clone(),
            review,
        })
        .map_err(|e| e.to_string())?,
    );
    for (path, bytes) in output {
        let blob = oid(git(
            &catalog.root,
            &["hash-object", "-w", "--stdin"],
            None,
            Some(&bytes),
        )?)?;
        git(
            &catalog.root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "100644",
                &blob,
                &path,
            ],
            Some(&index),
            None,
        )?;
    }
    let tree = oid(git(&catalog.root, &["write-tree"], Some(&index), None)?)?;
    let commit = oid(git(
        &catalog.root,
        &["commit-tree", &tree, "-p", &scan.revision],
        None,
        Some(if draft.review.is_some() {
            b"Tower.md S7 draft (unapproved)\n\nActor: human:local\n"
        } else {
            b"Tower.md S6 draft (unapproved)\n\nActor: process:towermd-cli\n"
        }),
    )?)?;
    if !catalog.matches_current_source()? {
        return Err("综合提交期间权威来源已改变，请重新扫描".into());
    }
    let reference = format!("{PREFIX}{id}");
    let zero = "0".repeat(commit.len());
    if let Err(error) = git(
        &catalog.root,
        &["update-ref", &reference, &commit, &zero],
        None,
        None,
    ) {
        // A concurrent byte-identical request may have won the CAS.
        return load(catalog, &id).map_err(|_| error);
    }
    load(catalog, &id)
}

pub fn list(catalog: &Catalog) -> Result<Vec<BaselineDraft>, String> {
    let refs = repo::git_timeout(
        &catalog.root,
        &["for-each-ref", "--format=%(refname)", PREFIX],
        TIMEOUT,
    )?;
    let mut drafts = Vec::new();
    for reference in refs.lines() {
        let id = reference.strip_prefix(PREFIX).ok_or("草稿引用无效")?;
        drafts.push(load(catalog, id)?);
    }
    drafts.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(drafts)
}

/// Only the private draft ref is published. Main and local branches are untouched.
pub fn push_if_remote(catalog: &Catalog, id: &str) -> Result<(), String> {
    valid_id(id)?;
    let remotes = repo::git_timeout(&catalog.root, &["remote"], TIMEOUT)?;
    if remotes.lines().any(|name| name == "origin") {
        let reference = format!("{PREFIX}{id}");
        git(
            &catalog.root,
            &[
                "push",
                "--porcelain",
                "origin",
                &format!("{reference}:{reference}"),
            ],
            None,
            None,
        )
        .map_err(|e| format!("综合草稿已在本地保存，私有引用推送失败；用相同输入重试：{e}"))?;
    }
    Ok(())
}
