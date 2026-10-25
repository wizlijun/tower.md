//! FR-107: freeze executable inputs, certify G3/G5, and prove an actual main
//! admission. The historical proof reader never grants execution permission.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use towermd_core::catalog::Catalog;
use crate::cold_start::{ScanCommand, ScanModule, ScanReport};
use towermd_core::hub::{Event, Lease};
use towermd_core::okf::Document;
use towermd_core::project_view::ProjectView;
use crate::task_model::{self, Run, RunState, Task, TaskProjection, TaskState, VerifiedBaseline};
use crate::verifier::{Limits, VerificationPlan, VerificationReport, VerificationStatus};
use towermd_core::{path_guard, repo};

pub const FINISH_EVENT: &str = "task.finish.completed";
pub const G5_EVENT: &str = "task.g5.completed";
pub const VERIFYING_TTL_MS: u64 = 60 * 60 * 1000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishRequest {
    pub evidence_id: String,
    pub run: Run,
    pub requested_at_ms: u64,
    pub deadline_ms: u64,
    pub started_eid: String,
    pub completion_eid: String,
    pub interrupted_eid: String,
    /// The delegate's per-criterion account (FR-209 `claimed`). It never
    /// completes the task; only the checks and the merge proof do.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<Claim>,
    /// The delegate's run report (FR-28). A self-report like `claims`: it is
    /// recorded with the evidence and never moves the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<crate::run_report::RunReport>,
}

/// One self-reported note on one done-when criterion (0-based index).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub criterion: usize,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    pub path: String,
    pub mode: String,
    pub oid: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenFinish {
    pub version: u32,
    pub evidence_id: String,
    pub project_id: String,
    pub docs_dir: String,
    pub baseline_commit: String,
    pub baseline_hash: String,
    pub task: Task,
    pub run: Run,
    pub main_base: String,
    pub revision: String,
    pub tree: String,
    /// Complete manifest also permits cold-clone verification after an amend or
    /// squash made the originally tested commit/tree unreachable.
    pub manifest: Vec<TreeEntry>,
    pub commands: Vec<ScanCommand>,
    pub modules: Vec<ScanModule>,
    pub limits: Limits,
    pub requested_at_ms: u64,
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLog {
    pub completion_eid: String,
    pub report_sha256: String,
    pub command_id: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishEvidence {
    pub input: FrozenFinish,
    pub input_sha256: String,
    pub completion_eid: String,
    pub report: VerificationReport,
    pub report_sha256: String,
    pub logs: Vec<EvidenceLog>,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct G5Evidence {
    pub completion_eid: String,
    pub evidence_id: String,
    pub finish_eid: String,
    pub finish_event_sha256: String,
    pub main_base: String,
    pub task_revision: String,
    pub task_parent: String,
    pub tree: String,
    pub checked_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergeProof {
    pub task_id: String,
    pub run_id: String,
    pub evidence_id: String,
    pub finish_eid: String,
    pub g5_eid: String,
    pub admission_commit: String,
    pub main_revision: String,
    pub tree: String,
    pub epoch: u64,
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)))
}
fn oid(value: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("finish 仅接受完整 Git 对象 ID".into());
    }
    Ok(())
}
fn safe_id(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 128
        || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    { return Err("证据 ID 必须是安全的单段标识".into()); }
    Ok(())
}
fn git_bytes(root: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut options = vec!["--no-replace-objects", "-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false"];
    options.extend_from_slice(args);
    let output = repo::git_timeout_env_input_output(root, &options, Duration::from_secs(30), &[], input)?;
    if !output.status.success() {
        return Err(format!("finish 无法完整读取 Git：{}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output.stdout)
}
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    String::from_utf8(git_bytes(root, args, None)?).map(|value| value.trim().into())
        .map_err(|_| "Git 结果不是 UTF-8".into())
}
fn fixed(root: &Path, value: &str, kind: &str) -> Result<String, String> {
    oid(value)?;
    let result = git(root, &["rev-parse", "--verify", &format!("{value}^{{{kind}}}")])?;
    oid(&result)?;
    Ok(result)
}
fn manifest(root: &Path, tree: &str) -> Result<Vec<TreeEntry>, String> {
    oid(tree)?;
    let bytes = git_bytes(root, &["ls-tree", "-rz", "--full-tree", tree], None)?;
    let mut entries = Vec::new();
    for record in bytes.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
        let text = std::str::from_utf8(record).map_err(|_| "仓库路径不是 UTF-8，无法完整冻结证据")?;
        let (metadata, path) = text.split_once('\t').ok_or("Git tree 记录无效")?;
        let fields: Vec<_> = metadata.split(' ').collect();
        if fields.len() != 3 { return Err("Git tree 元数据无效".into()); }
        entries.push(TreeEntry { path: path.into(), mode: fields[0].into(), oid: fields[2].into() });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Hash Git's raw tree encoding without writing objects or requiring the old
/// blob objects. This is why a cold clone can verify an unreachable G3 tree.
fn manifest_tree(root: &Path, entries: &[TreeEntry]) -> Result<String, String> {
    let mut directories: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    directories.insert(String::new(), Vec::new());
    let mut paths = BTreeSet::new();
    let mut oid_len = None;
    for entry in entries {
        oid(&entry.oid)?;
        if oid_len.replace(entry.oid.len()).is_some_and(|len| len != entry.oid.len())
            || !matches!(entry.mode.as_str(), "100644" | "100755" | "120000" | "160000")
            || entry.path.is_empty() || entry.path.contains('\0')
            || entry.path.split('/').any(|part| matches!(part, "" | "." | ".." | ".git"))
            || !paths.insert(entry.path.clone())
        { return Err("冻结 tree manifest 不完整或含重复路径".into()); }
        let (parent, name) = entry.path.rsplit_once('/').unwrap_or(("", entry.path.as_str()));
        directories.entry(parent.into()).or_default().push((name.into(), entry.mode.clone(), entry.oid.clone()));
        let mut ancestor = parent;
        while !ancestor.is_empty() {
            directories.entry(ancestor.into()).or_default();
            ancestor = ancestor.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
    if directories.keys().any(|directory| paths.contains(directory)) {
        return Err("冻结 tree 的文件和目录重叠".into());
    }
    let mut names: Vec<_> = directories.keys().cloned().collect();
    names.sort_by_key(|name| std::cmp::Reverse(name.split('/').count()));
    // Empty root has the same component count as top-level directories.
    names.retain(|name| !name.is_empty());
    names.push(String::new());
    let mut result = String::new();
    for directory in names {
        let mut items = directories.remove(&directory).ok_or("tree 目录丢失")?;
        items.sort_by(|a, b| {
            let key = |item: &(String, String, String)| if item.1 == "40000" { format!("{}/", item.0) } else { item.0.clone() };
            key(a).as_bytes().cmp(key(b).as_bytes())
        });
        let mut encoded = Vec::new();
        for (name, mode, object) in items {
            encoded.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            for index in (0..object.len()).step_by(2) {
                encoded.push(u8::from_str_radix(&object[index..index + 2], 16).map_err(|_| "无效 tree OID")?);
            }
        }
        result = String::from_utf8(git_bytes(root, &["hash-object", "-t", "tree", "--stdin"], Some(&encoded))?)
            .map_err(|_| "tree 哈希无效")?.trim().into();
        if !directory.is_empty() {
            let (parent, name) = directory.rsplit_once('/').unwrap_or(("", directory.as_str()));
            directories.get_mut(parent).ok_or("tree 父目录丢失")?
                .push((name.into(), "40000".into(), result.clone()));
        }
    }
    Ok(result)
}
fn state_path(path: &str, docs: &str) -> bool {
    path.starts_with(&format!("{docs}/state/")) || path == format!("{docs}/tower.canvas")
}
fn code_manifest(entries: &[TreeEntry], docs: &str) -> Vec<TreeEntry> {
    entries.iter().filter(|entry| !state_path(&entry.path, docs)).cloned().collect()
}
fn parents(root: &Path, commit: &str) -> Result<Vec<String>, String> {
    oid(commit)?;
    let text = git(root, &["show", "-s", "--format=%P", commit])?;
    let result = text.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    for parent in &result { oid(parent)?; }
    Ok(result)
}
/// The gate a G2 stop at finish names (12.1 禁止), and how its error starts.
pub const G2: &str = "G2";
pub const G2_BLOCKED: &str = "G2 路径守卫失败";
/// G2 refused a tool request before it ran: a dangerous command or a sensitive file.
pub const G2_TOOL_BLOCKED: &str = "G2 拦截危险命令或敏感文件";
/// The project's extra sensitive paths from `gates.yaml` on main (G2). They
/// only add to the built-in list; a broken file adds none here and fails
/// the gate check of its own.
fn sensitive_paths(catalog: &Catalog) -> Result<Vec<String>, String> {
    Ok(towermd_core::gates::GateFile::from_text(catalog.read_file(towermd_core::gates::GATES_PATH)?.as_deref()).sensitive_paths)
}
/// G3's optional check (`behavior_tests` in main's `gates.yaml`): the files
/// the task adds or modifies, net of its commits, must include a test when
/// they change behavior. Off when not declared.
fn behavior_tests(catalog: &Catalog, head: &str) -> Result<(), String> {
    let Some(rule) = crate::behavior_tests::Rule::from_gates(catalog.read_file(towermd_core::gates::GATES_PATH)?.as_deref())
        .map_err(|error| format!("{}：{error}", crate::behavior_tests::UNTESTED))? else { return Ok(()) };
    let changed = git_bytes(&catalog.root, &["diff", "--no-renames", "--diff-filter=d", "--name-only", "-z", &catalog.revision, head], None)?;
    let changed = String::from_utf8(changed).map_err(|_| "Git 结果不是 UTF-8")?;
    rule.check(&changed.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect::<Vec<_>>())
}
/// A linear fast-forward task; guard every parent edge, including reverted
/// changes. Net-tree equality cannot hide a protected intermediate edit.
/// `sensitive` adds G2's sensitive-file rule to a new finish; a merge proof
/// of an accepted finish passes `None` so the past is not judged again.
fn task_chain(root: &Path, base: &str, head: &str, docs: &str, allowed: &[String], sensitive: Option<&[String]>) -> Result<(), String> {
    oid(base)?; oid(head)?;
    git(root, &["merge-base", "--is-ancestor", base, head])?;
    if base == head { return Err("任务没有待合入提交".into()); }
    let range = format!("{base}..{head}");
    for commit in git(root, &["rev-list", "--reverse", &range])?.lines() {
        let edge = parents(root, commit)?;
        if edge.len() != 1 { return Err("任务必须变基后快进合并，不接受 merge commit".into()); }
        let mut guard = path_guard::check(root, &edge[0], commit, docs, allowed)?;
        if let Some(extra) = sensitive {
            guard.violations.extend(path_guard::sensitive_violations(&guard.changes, extra));
        }
        if !guard.passed || !guard.violations.is_empty() {
            return Err(format!("{G2_BLOCKED}：{}", guard.violations.iter()
                .map(|violation| format!("{}：{}", violation.path, violation.reason)).collect::<Vec<_>>().join("；")));
        }
    }
    Ok(())
}
fn state_only_advance(root: &Path, old: &str, new: &str, docs: &str) -> Result<(), String> {
    if old == new { return Ok(()); }
    oid(old)?; oid(new)?;
    git(root, &["merge-base", "--is-ancestor", old, new])?;
    for commit in git(root, &["rev-list", "--reverse", &format!("{old}..{new}")])?.lines() {
        let edge = parents(root, commit)?;
        if edge.len() != 1 { return Err("main 含非线性推进，必须重新 finish".into()); }
        let changes = path_guard::check(root, &edge[0], commit, docs, &[format!("{docs}/")])?;
        if changes.changes.iter().any(|change| !state_path(&change.path, docs)) {
            return Err("main 的代码、配置或其他路径已推进，必须重新 finish".into());
        }
    }
    Ok(())
}

fn current_run(
    baseline: &VerifiedBaseline, projection: &TaskProjection, run_id: &str,
    leases: &[Lease], now_ms: u64,
) -> Result<(Task, Run), String> {
    let run = projection.runs.get(run_id).ok_or("运行不存在")?;
    let task = projection.tasks.get(&run.task).ok_or("运行的任务不存在")?;
    if run.state != RunState::Active || task.active_run.as_deref() != Some(run_id)
        || !matches!(task.state, TaskState::Running | TaskState::Verifying)
        || task.baseline != baseline.tag() || run.baseline != baseline.tag()
    { return Err("运行、任务状态或活动批准基线已改变".into()); }
    let mut ready = projection.clone();
    let candidate = ready.tasks.get_mut(&run.task).unwrap();
    candidate.state = TaskState::Ready; candidate.active_run = None;
    task_model::check_g0(baseline, &ready, &run.task)?;
    let names: BTreeSet<_> = std::iter::once(&task.input.module).chain(&task.input.touches)
        .map(|id| format!("module:{id}")).collect();
    let actual: BTreeSet<_> = run.fences.iter().map(|fence| fence.name.clone()).collect();
    if names != actual || actual.len() != run.fences.len()
        || run.fences.iter().any(|fence| fence.epoch == 0 || fence.holder != run_id
            || !leases.iter().any(|lease| lease.fence() == *fence && lease.expires_at > now_ms))
    { return Err("G0：主模块与 touches 完整租约已失效".into()); }
    Ok((task.clone(), run.clone()))
}
fn current_head(catalog: &Catalog, run: &Run) -> Result<String, String> {
    if !catalog.matches_current_source()? { return Err("权威 main 已推进，请刷新后重试".into()); }
    let root = Path::new(&run.binding.worktree);
    if root.canonicalize().map_err(|e| e.to_string())? != root
        || repo::toplevel(root)? != root
        || root.join(git(root, &["rev-parse", "--git-common-dir"])?).canonicalize().map_err(|e| e.to_string())? != catalog.common_dir
        || git(root, &["symbolic-ref", "--short", "HEAD"])? != run.binding.branch
        || run.binding.branch != format!("t/{}", run.task)
    { return Err("运行的仓库、工作树或任务分支已改变".into()); }
    if !git_bytes(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"], None)?.is_empty() {
        return Err("工作树有未提交改动，finish 拒绝执行".into());
    }
    git(root, &["rev-parse", "--verify", "HEAD"])
}
// Published module documents can retain scan metadata (id/modules), while
// reviewed commands contain only execution fields. Read both without relaxing
// the strict ReviewedCommand DTO accepted by human review requests.
#[derive(Deserialize)]
struct ApprovedCommand {
    directory: String,
    command: String,
}

fn approved_commands(catalog: &Catalog, baseline: &VerifiedBaseline, task: &Task) -> Result<(Vec<ScanModule>, Vec<ScanCommand>), String> {
    let mut combined: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut modules = Vec::new();
    let mut ids: Vec<_> = std::iter::once(task.input.module.clone()).chain(task.input.touches.iter().cloned()).collect();
    ids.sort();
    for id in ids {
        let path = catalog.object_paths.get(&id).ok_or("main 缺少已批准模块")?;
        let source = String::from_utf8(git_bytes(&catalog.root, &["show", &format!("{}:{path}", baseline.commit())], None)?)
            .map_err(|_| "批准 Module 不是 UTF-8")?;
        if catalog.read_file(path)?.as_deref() != Some(source.as_str()) {
            return Err("main 的模块与已批准基线不同，须先批准新基线".into());
        }
        let document = Document::parse(&source)?;
        if document.doc_type() != Some("Module") || document.get_str("id") != Some(id.as_str()) {
            return Err("批准基线中的模块身份无效".into());
        }
        let commands: Vec<ApprovedCommand> = serde_yaml::from_value(document.get("verification_commands").cloned()
            .unwrap_or_else(|| serde_yaml::Value::Sequence(Vec::new()))).map_err(|e| format!("批准验证命令无效：{e}"))?;
        for command in commands {
            if command.command.trim().is_empty() || command.command.contains('\0')
                || (!command.directory.is_empty() && !task_model::path_is_allowed(&command.directory, &[command.directory.clone()])?)
            { return Err("批准验证命令的 cwd 或命令无效".into()); }
            combined.entry((command.directory, command.command)).or_default().insert(id.clone());
        }
        modules.push(ScanModule {
            id: id.clone(), title: document.get_str("title").unwrap_or(&id).into(),
            paths: baseline.modules().get(&id).ok_or("基线缺少模块路径")?.paths.clone(),
            files: Vec::new(), confidence: 1.0, rationale: "已批准基线的模块验证命令".into(),
            command_ids: Vec::new(), suggested_task: None,
        });
    }
    let commands: Vec<_> = combined.into_iter().enumerate().map(|(index, ((directory, command), modules))| ScanCommand {
        id: format!("C-{}", index + 1), directory, command, modules: modules.into_iter().collect(),
    }).collect();
    for module in &mut modules {
        module.command_ids = commands.iter().filter(|command| command.modules.contains(&module.id)).map(|command| command.id.clone()).collect();
    }
    Ok((modules, commands))
}

/// Caller refreshes origin/main first, and serializes this preparation with
/// task/lease changes. No verifier process or user-worktree write occurs here.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    catalog: &Catalog, baseline: &VerifiedBaseline, projection: &TaskProjection,
    run_id: &str, evidence_id: &str, leases: &[Lease], now_ms: u64, limits: Limits,
) -> Result<FrozenFinish, String> {
    safe_id(evidence_id)?;
    let (task, run) = current_run(baseline, projection, run_id, leases, now_ms)?;
    let revision = current_head(catalog, &run)?;
    git(&catalog.root, &["merge-base", "--is-ancestor", baseline.commit(), &catalog.revision])?;
    task_chain(&catalog.root, &catalog.revision, &revision, catalog.config.docs_dir(), &task.input.allowed_paths, Some(&sensitive_paths(catalog)?))?;
    behavior_tests(catalog, &revision)?;
    let (modules, commands) = approved_commands(catalog, baseline, &task)?;
    let tree = fixed(&catalog.root, &revision, "tree")?;
    let manifest = manifest(&catalog.root, &tree)?;
    Ok(FrozenFinish {
        version: 1, evidence_id: evidence_id.into(), project_id: catalog.config.project_id().into(),
        docs_dir: catalog.config.docs_dir().into(), baseline_commit: baseline.commit().into(),
        baseline_hash: baseline.content_hash().into(), task, run, main_base: catalog.revision.clone(),
        revision, tree, manifest, commands, modules, limits, requested_at_ms: now_ms,
        deadline_ms: now_ms.checked_add(VERIFYING_TTL_MS).ok_or("验证期限溢出")?,
    })
}

pub fn verification_plan(input: &FrozenFinish, root: &Path) -> VerificationPlan {
    VerificationPlan {
        root: root.into(), id: input.evidence_id.clone(), revision: input.revision.clone(),
        tree: input.tree.clone(), commands: input.commands.clone(), modules: input.modules.clone(), limits: input.limits.clone(),
    }
}
fn as_scan(input: &FrozenFinish, report: &VerificationReport) -> ScanReport {
    ScanReport {
        id: input.evidence_id.clone(), project_id: input.project_id.clone(), reference: "main".into(),
        revision: input.revision.clone(), tree: input.tree.clone(),
        view: ProjectView { title: String::new(), reference: "main".into(), revision: input.revision.clone(),
            docs_dir: Some(input.docs_dir.clone()), diagnostics: Vec::new(), nodes: Vec::new(), edges: Vec::new(), unplaced: Vec::new() },
        modules: input.modules.clone(), commands: input.commands.clone(), unassigned: Vec::new(),
        source_file_count: 0, assigned_file_count: 0, coverage: 0.0, diagnostics: Vec::new(),
        hotspots: Vec::new(), couplings: Vec::new(), verification: Some(report.clone()),
        dependencies: Vec::new(), checks: None, origin: None,
    }
}
fn logs(eid: &str, hash: &str, report: &VerificationReport) -> Vec<EvidenceLog> {
    report.commands.iter().filter_map(|command| Some(EvidenceLog {
        completion_eid: eid.into(), report_sha256: hash.into(), command_id: command.id.clone(),
        path: command.log.clone()?, sha256: command.log_sha256.clone()?,
    })).collect()
}
fn passed(report: &VerificationReport) -> bool {
    report.cleanup_complete && !report.modules.is_empty() && !report.commands.is_empty()
        && report.modules.iter().all(|module| module.status == VerificationStatus::Passed)
        && report.commands.iter().all(|command| command.status == VerificationStatus::Passed)
}

/// The daemon fixes completion_eid before putting this exact payload in its
/// durable outbox. Failed reports are evidence too; they never certify a merge.
pub fn complete(input: FrozenFinish, report: VerificationReport, completion_eid: &str, common: &Path) -> Result<FinishEvidence, String> {
    if completion_eid.is_empty() { return Err("完成证据缺少原 eid".into()); }
    crate::cold_start::validate_verification_logs(&as_scan(&input, &report), common)?;
    let report_sha256 = digest(&report)?;
    Ok(FinishEvidence {
        input_sha256: digest(&input)?, input, completion_eid: completion_eid.into(),
        logs: logs(completion_eid, &report_sha256, &report), passed: passed(&report), report, report_sha256,
    })
}
/// Validate the canonical completion envelope without Git or local log access.
/// Failed reports remain readable/replayable; only the proof reader requires a
/// passing report and a manifest that hashes to the original evidence tree.
pub fn completion_from_event(event: &Event) -> Result<FinishEvidence, String> {
    let evidence: FinishEvidence = serde_json::from_value(event.data.clone()).map_err(|e| format!("完成证据无效：{e}"))?;
    let input = &evidence.input;
    safe_id(&input.evidence_id)?;
    if event.seq == 0 || event.stale || event.kind != FINISH_EVENT || event.actor != "process:towermd-verifier"
        || event.layer != "L4" || event.obj != input.evidence_id || event.eid != evidence.completion_eid
        || event.host != input.run.binding.host || event.guards != input.run.fences
        || event.epoch != event.guards.first().map(|fence| fence.epoch)
        || input.version != 1 || input.run.task != input.task.input.id
        || input.run.module != input.task.input.module || input.run.baseline != input.task.baseline
        || input.run.fences.is_empty() || input.run.primary_fence().is_none()
        || input.deadline_ms != input.requested_at_ms.checked_add(VERIFYING_TTL_MS).ok_or("验证期限溢出")?
        || evidence.input_sha256 != digest(input)? || evidence.report_sha256 != digest(&evidence.report)?
        || evidence.logs != logs(&event.eid, &evidence.report_sha256, &evidence.report)
        || evidence.passed != passed(&evidence.report)
    { return Err("完成证据必须来自原完整已接受 Event，冻结输入、报告和日志引用须一致".into()); }
    crate::cold_start::validate_verification(&as_scan(input, &evidence.report))?;
    Ok(evidence)
}
fn accepted_finish(root: &Path, event: &Event) -> Result<FinishEvidence, String> {
    let evidence = completion_from_event(event)?;
    let input = &evidence.input;
    if manifest_tree(root, &input.manifest).map_err(|error| format!("冻结 manifest 无法重建：{error}"))? != input.tree {
        return Err("冻结 manifest 不能重建原验证 tree".into());
    }
    if !evidence.passed { return Err("G3 验证未通过，不能生成合并许可或认证完成".into()); }
    Ok(evidence)
}

/// Current producer checks are intentionally separate from historical proof.
/// A caller observing an already admitted Task must not ask for a new rebase.
pub fn validate_current(
    input: &FrozenFinish, baseline: &VerifiedBaseline, projection: &TaskProjection,
    leases: &[Lease], now_ms: u64,
) -> Result<(), String> {
    if now_ms < input.requested_at_ms || now_ms >= input.deadline_ms {
        return Err("验证的固定 60 分钟期限已失效".into());
    }
    let (task, run) = current_run(baseline, projection, &input.run.binding.id, leases, now_ms)?;
    if task.input != input.task.input || task.origin != input.task.origin || run != input.run
        || baseline.commit() != input.baseline_commit || baseline.content_hash() != input.baseline_hash
    { return Err("任务、运行或批准输入已经改变，必须重新 finish".into()); }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_g5(
    catalog: &Catalog, baseline: &VerifiedBaseline, projection: &TaskProjection,
    finish: &Event, completion_eid: &str, leases: &[Lease], now_ms: u64,
) -> Result<G5Evidence, String> {
    let evidence = accepted_finish(&catalog.root, finish)?;
    let input = &evidence.input;
    if now_ms < input.requested_at_ms || now_ms >= input.deadline_ms || completion_eid.is_empty() {
        return Err("验证的固定 60 分钟期限已失效".into());
    }
    validate_current(input, baseline, projection, leases, now_ms)?;
    let task = &input.task;
    let run = &input.run;
    if catalog.config.project_id() != input.project_id || catalog.config.docs_dir() != input.docs_dir
    { return Err("任务、运行或批准输入已经改变，必须重新 finish".into()); }
    let revision = current_head(catalog, &run)?;
    state_only_advance(&catalog.root, &input.main_base, &catalog.revision, &input.docs_dir)?;
    task_chain(&catalog.root, &catalog.revision, &revision, &input.docs_dir, &task.input.allowed_paths, Some(&sensitive_paths(catalog)?))?;
    let tree = fixed(&catalog.root, &revision, "tree")?;
    if code_manifest(&manifest(&catalog.root, &tree)?, &input.docs_dir) != code_manifest(&input.manifest, &input.docs_dir) {
        return Err("当前 Task tree 与 G3 证据不一致，必须重新 finish".into());
    }
    let task_parent = parents(&catalog.root, &revision)?.into_iter().next().ok_or("任务没有父提交")?;
    Ok(G5Evidence {
        completion_eid: completion_eid.into(), evidence_id: input.evidence_id.clone(),
        finish_eid: finish.eid.clone(), finish_event_sha256: digest(finish)?,
        main_base: catalog.revision.clone(), task_revision: revision, task_parent, tree, checked_at_ms: now_ms,
    })
}
fn accepted_g5(finish: &Event, evidence: &FinishEvidence, event: &Event) -> Result<G5Evidence, String> {
    let gate: G5Evidence = serde_json::from_value(event.data.clone()).map_err(|e| format!("G5 证据无效：{e}"))?;
    if event.seq <= finish.seq || event.stale || event.kind != G5_EVENT || event.actor != "process:towermd"
        || event.layer != "L4" || event.obj != evidence.input.evidence_id || event.eid != gate.completion_eid
        || event.host != finish.host || event.guards != finish.guards
        || event.epoch != event.guards.first().map(|fence| fence.epoch)
        || gate.evidence_id != evidence.input.evidence_id || gate.finish_eid != finish.eid
        || gate.finish_event_sha256 != digest(finish)? || gate.checked_at_ms < evidence.input.requested_at_ms
        || gate.checked_at_ms >= evidence.input.deadline_ms
    { return Err("G5 必须是绑定原 G3 Event 的已接受终态证据".into()); }
    for id in [&gate.main_base, &gate.task_revision, &gate.task_parent, &gate.tree] { oid(id)?; }
    Ok(gate)
}

pub fn g5_from_event(finish: &Event, event: &Event) -> Result<G5Evidence, String> {
    accepted_g5(finish, &completion_from_event(finish)?, event)
}

/// Call only after durable G5 acceptance. The normal daemon must still recheck
/// current leases/holds/deadline before returning this text to a running agent.
pub fn trailers(root: &Path, finish: &Event, g5: &Event) -> Result<String, String> {
    let evidence = accepted_finish(root, finish)?;
    accepted_g5(finish, &evidence, g5)?;
    let input = &evidence.input;
    let epoch = input.run.primary_fence().ok_or("缺少主 Module fence")?.epoch;
    Ok(format!("Towermd-Task: {}\nTowermd-Run: {}\nTowermd-Module: {}\nTowermd-Evidence: {}\nTowermd-Epoch: {epoch}\n",
        input.task.input.id, input.run.binding.id, input.task.input.module, input.evidence_id))
}
fn exact_trailers(message: &str, expected: &str) -> Result<(), String> {
    let block = message.trim_end().rsplit("\n\n").next().unwrap_or("");
    let mut actual = BTreeMap::new();
    for line in block.lines() {
        let (key, value) = line.split_once(": ").ok_or("末个 trailer block 格式无效")?;
        if key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value.trim().is_empty()
        { return Err("末个 trailer block 格式无效".into()); }
        // Other Git trailers preserve contributor attribution but do not grant
        // Tower certification. Reserve case variants of our namespace as well.
        if key.to_ascii_lowercase().starts_with("towermd-")
            && actual.insert(key, value).is_some()
        { return Err("末个 trailer block 存在重复 Tower.md 项".into()); }
    }
    let expected: BTreeMap<_, _> = expected.lines().filter_map(|line| line.split_once(": ")).collect();
    if actual != expected { return Err("末个 trailer block 的 Tower.md 项必须完整、唯一且与验证证据一致".into()); }
    Ok(())
}

/// Pure historical proof. `main` must be the caller's freshly read authority;
/// `admission` is the actual last Task commit on main's first-parent chain.
/// This function neither renews a lease nor changes task state.
pub fn prove_merge(root: &Path, main: &str, admission: &str, finish: &Event, g5: &Event) -> Result<MergeProof, String> {
    oid(main)?; oid(admission)?;
    let evidence = accepted_finish(root, finish)?;
    let gate = accepted_g5(finish, &evidence, g5)?;
    let input = &evidence.input;
    if !git(root, &["rev-list", "--first-parent", main])?.lines().any(|commit| commit == admission) {
        return Err("Task tip 不是实际 main first-parent 接入提交".into());
    }
    let edge = parents(root, admission)?;
    if edge.len() != 1 || edge[0] != gate.task_parent {
        return Err("实际 main 接入提交的 Task 父 OID 与终态 G5 不同".into());
    }
    let tree = fixed(root, admission, "tree")?;
    if tree != gate.tree || code_manifest(&manifest(root, &tree)?, &input.docs_dir) != code_manifest(&input.manifest, &input.docs_dir) {
        return Err("实际 main 接入 tree 与原 G3/G5 不一致".into());
    }
    state_only_advance(root, &input.main_base, &gate.main_base, &input.docs_dir)?;
    task_chain(root, &gate.main_base, admission, &input.docs_dir, &input.task.input.allowed_paths, None)?;
    exact_trailers(&git(root, &["show", "-s", "--format=%B", admission])?, &trailers(root, finish, g5)?)?;
    Ok(MergeProof {
        task_id: input.task.input.id.clone(), run_id: input.run.binding.id.clone(),
        evidence_id: input.evidence_id.clone(), finish_eid: finish.eid.clone(), g5_eid: g5.eid.clone(),
        admission_commit: admission.into(), main_revision: main.into(), tree,
        epoch: input.run.primary_fence().ok_or("缺少主 Module fence")?.epoch,
    })
}

#[cfg(test)]
#[path = "task_finish_tests.rs"]
mod tests;
