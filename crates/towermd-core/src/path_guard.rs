//! Read-only FR-16/17 guard and complete-history input for FR-103 inspection.
//! No tag, trailer, or agent claim authorizes a protected write here.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::{
    config::{Config, CONFIG_PATH},
    repo, types,
};

const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedPath {
    pub path: String,
    pub status: String,
    pub old_mode: String,
    pub new_mode: String,
    pub old_oid: String,
    pub new_oid: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    OutsideAllowedPaths,
    ProtectedPath,
    SlowObject,
    InvalidObject,
    UnsupportedEntry,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Violation {
    pub path: String,
    pub rule: Rule,
    pub reason: String,
}
/// Process configuration (core spec FR-03, 9.2): gate declarations, extension
/// configuration, the project's own settings and its locked skill packs
/// (FR-51). Changing only these is a minor change, which a person registers;
/// anything else protected is a major change. Both still enter main only
/// through an approved path.
pub fn process_config(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path == ".towermd/gates.yaml" || path == ".towermd/config.yaml" || path == ".towermd/skillpacks.yaml"
        || path.starts_with(".towermd/extensions/") || path.starts_with(".towermd/skillpacks/")
}

/// `minor` when every protected path a commit changed is process
/// configuration, else `major`.
pub fn change_class(violations: &[Violation]) -> &'static str {
    if !violations.is_empty() && violations.iter().all(|violation| process_config(&violation.path)) { "minor" } else { "major" }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuardReport {
    pub base: String,
    pub head: String,
    pub base_tree: String,
    pub head_tree: String,
    pub changes: Vec<ChangedPath>,
    pub violations: Vec<Violation>,
    pub passed: bool,
}

fn oid(value: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("路径守卫只接受固定完整 Git 对象 ID".into());
    }
    Ok(())
}
fn bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut options = vec!["--no-replace-objects", "-c", "core.hooksPath=/dev/null"];
    options.extend_from_slice(args);
    let output = repo::git_timeout_env_output(root, &options, TIMEOUT, &[])?;
    if !output.status.success() {
        return Err(format!(
            "无法完整读取 Git：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}
fn text(root: &Path, args: &[&str]) -> Result<String, String> {
    String::from_utf8(bytes(root, args)?)
        .map(|text| text.trim().into())
        .map_err(|_| "Git 结果不是 UTF-8".into())
}
fn tree(root: &Path, value: &str) -> Result<String, String> {
    oid(value)?;
    let tree = text(
        root,
        &["rev-parse", "--verify", &format!("{value}^{{tree}}")],
    )?;
    oid(&tree)?;
    Ok(tree)
}
fn literal_path(path: &str) -> Result<(), String> {
    // The existing task matcher supplies the same path grammar as task creation.
    if !path_is_allowed(path, &[path.into()])? {
        return Err("路径不是具体文件".into());
    }
    Ok(())
}
fn docs_path(docs: &str) -> Result<(), String> {
    literal_path(docs)?;
    Ok(())
}
pub fn protected_path(path: &str, docs: &str) -> bool {
    // Case folding also protects the same physical path on default macOS volumes.
    let path = path.to_ascii_lowercase();
    let docs = docs.to_ascii_lowercase();
    path == ".towermd"
        || path.starts_with(".towermd/")
        || path == format!("{docs}/tower.canvas")
        || ["intents", "arch", "specs", "baselines", "state"]
            .iter()
            .any(|name| {
                let prefix = format!("{docs}/{name}");
                path == prefix || path.starts_with(&(prefix + "/"))
            })
}
fn blob(root: &Path, object: &str) -> Result<Vec<u8>, String> {
    oid(object)?;
    let size = text(root, &["cat-file", "-s", object])?
        .parse::<usize>()
        .map_err(|_| "Git blob 大小无效")?;
    if size > 16 * 1024 * 1024 {
        return Err("Markdown 对象超过 16 MiB，无法完整核验".into());
    }
    bytes(root, &["cat-file", "blob", object])
}
fn frontmatter(text: &str) -> Result<Option<serde_yaml::Mapping>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(|line| line.trim_end_matches(['\r', '\n'])) != Some("---") {
        return Ok(None);
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
    }
    if !closed {
        return Err("frontmatter 未闭合，无法排除慢层对象".into());
    }
    match serde_yaml::from_str::<Value>(&yaml).map_err(|e| format!("frontmatter 无效：{e}"))? {
        Value::Mapping(mapping) => Ok(Some(mapping)),
        Value::Null => Ok(None),
        _ => Err("frontmatter 不是映射，无法排除慢层对象".into()),
    }
}
fn object_rule(root: &Path, object: &str) -> Result<Option<(Rule, String)>, String> {
    let bytes = blob(root, object)?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(_) => {
            return Ok(Some((
                Rule::InvalidObject,
                "Markdown 不是 UTF-8，无法排除慢层对象".into(),
            )))
        }
    };
    let front = match frontmatter(text) {
        Ok(Some(front)) => front,
        Ok(None) => return Ok(None),
        Err(error) => return Ok(Some((Rule::InvalidObject, error))),
    };
    if let Some(layer) = front.get("layer") {
        return Ok(match layer.as_str().and_then(types::Layer::parse) {
            Some(layer) if layer.is_slow() => Some((
                Rule::SlowObject,
                format!("对象 layer={} 属于慢层", layer.as_str()),
            )),
            Some(_) => None,
            None => Some((
                Rule::InvalidObject,
                "非法 layer 必须按受保护对象处理".into(),
            )),
        });
    }
    if let Some(kind) = front.get("type") {
        let Some(kind) = kind.as_str().filter(|kind| !kind.trim().is_empty()) else {
            return Ok(Some((
                Rule::InvalidObject,
                "对象 type 无效，无法判定默认层".into(),
            )));
        };
        if let Some(info) = types::lookup(kind).filter(|info| info.default_layer.is_slow()) {
            return Ok(Some((
                Rule::SlowObject,
                format!(
                    "{} 的默认层 {} 属于慢层",
                    info.name,
                    info.default_layer.as_str()
                ),
            )));
        }
    }
    // Ordinary Markdown or unrelated YAML metadata is not automatically an OKF object.
    Ok(None)
}
fn changes(root: &Path, base: &str, head: &str) -> Result<Vec<ChangedPath>, String> {
    let raw = bytes(
        root,
        &[
            "diff-tree",
            "--raw",
            "-z",
            "--no-abbrev",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--no-commit-id",
            "-r",
            base,
            head,
            "--",
        ],
    )?;
    let records: Vec<_> = raw
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect();
    if records.len() % 2 != 0 {
        return Err("raw diff 不完整".into());
    }
    let mut result = Vec::new();
    for pair in records.chunks_exact(2) {
        let header = std::str::from_utf8(pair[0]).map_err(|_| "diff 头不是 UTF-8")?;
        let parts: Vec<_> = header
            .strip_prefix(':')
            .ok_or("diff 缺少模式头")?
            .split(' ')
            .collect();
        if parts.len() != 5 || !matches!(parts[4], "A" | "D" | "M" | "T") {
            return Err("diff 状态不受支持".into());
        }
        oid(parts[2])?;
        oid(parts[3])?;
        let path = std::str::from_utf8(pair[1])
            .map_err(|_| "改动路径不是 UTF-8，无法完整核验")?
            .to_string();
        literal_path(&path)?;
        result.push(ChangedPath {
            path,
            status: parts[4].into(),
            old_mode: parts[0].into(),
            new_mode: parts[1].into(),
            old_oid: parts[2].into(),
            new_oid: parts[3].into(),
        });
    }
    Ok(result)
}

fn inspect_changes(
    root: &Path,
    changes: &[ChangedPath],
    docs: &str,
    allowed: Option<&[String]>,
) -> Result<Vec<Violation>, String> {
    let mut violations = Vec::new();
    for change in changes {
        if let Some(allowed) = allowed {
            if !path_is_allowed(&change.path, allowed)? {
                violations.push(Violation {
                    path: change.path.clone(),
                    rule: Rule::OutsideAllowedPaths,
                    reason: "改动不属于任务 allowed_paths".into(),
                });
            }
        }
        if protected_path(&change.path, docs) {
            violations.push(Violation {
                path: change.path.clone(),
                rule: Rule::ProtectedPath,
                reason: "配置、慢层目录、基线画布或快照路径只能由授权写入者修改".into(),
            });
        }
        for (side, mode, object) in [
            ("改动前", &change.old_mode, &change.old_oid),
            ("改动后", &change.new_mode, &change.new_oid),
        ] {
            if mode == "000000" {
                continue;
            }
            if !matches!(mode.as_str(), "100644" | "100755") {
                violations.push(Violation {
                    path: change.path.clone(),
                    rule: Rule::UnsupportedEntry,
                    reason: format!(
                        "{side}模式 {mode} 为链接、子模块或不支持的条目，不能绕过路径守卫"
                    ),
                });
                continue;
            }
            if change.path.to_ascii_lowercase().ends_with(".md") {
                if let Some((rule, reason)) = object_rule(root, object)? {
                    violations.push(Violation {
                        path: change.path.clone(),
                        rule,
                        reason: format!("{side}：{reason}"),
                    });
                }
            }
        }
    }
    Ok(violations)
}

/// A tree-to-tree check includes both sides of a rename as deletion and addition.
/// A caller checking a task history must call this for every actual parent edge,
/// not only the net diff, so a protected edit followed by a revert stays visible.
pub fn check(
    root: &Path,
    base: &str,
    head: &str,
    docs: &str,
    allowed: &[String],
) -> Result<GuardReport, String> {
    docs_path(docs)?;
    for rule in allowed {
        path_is_within(rule, rule)?;
    }
    let base_tree = tree(root, base)?;
    let head_tree = tree(root, head)?;
    let changes = changes(root, &base_tree, &head_tree)?;
    let violations = inspect_changes(root, &changes, docs, Some(allowed))?;
    Ok(GuardReport {
        base: base.into(),
        head: head.into(),
        base_tree,
        head_tree,
        changes,
        passed: violations.is_empty(),
        violations,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InspectedCommit {
    pub commit: String,
    pub parents: Vec<String>,
    pub violations: Vec<Violation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MainInspection {
    pub main: String,
    pub import_anchor: String,
    pub commits: Vec<InspectedCommit>,
}
fn config_at(root: &Path, commit: &str) -> Result<Option<Config>, String> {
    let raw = bytes(root, &["ls-tree", "-z", commit, "--", CONFIG_PATH])?;
    if raw.is_empty() {
        return Ok(None);
    }
    let raw = std::str::from_utf8(&raw).map_err(|_| "配置 tree 条目无效")?;
    let (metadata, path) = raw
        .trim_end_matches('\0')
        .split_once('\t')
        .ok_or("配置条目无效")?;
    let parts: Vec<_> = metadata.split(' ').collect();
    if parts.len() != 3
        || !matches!(parts[0], "100644" | "100755")
        || parts[1] != "blob"
        || path != CONFIG_PATH
    {
        return Err("项目配置不是普通文件".into());
    }
    let content = String::from_utf8(blob(root, parts[2])?).map_err(|_| "项目配置不是 UTF-8")?;
    Config::parse(&content).map(Some)
}
fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> Result<bool, String> {
    oid(ancestor)?;
    oid(descendant)?;
    let result = repo::git_timeout_env_output(
        root,
        &[
            "--no-replace-objects",
            "merge-base",
            "--is-ancestor",
            ancestor,
            descendant,
        ],
        TIMEOUT,
        &[],
    )?;
    match result.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err("无法证明历史祖先关系".into()),
    }
}

/// Enumerates every commit outside the import anchor's ancestors. Results are
/// inputs for matching authentic hub receipts by commit SHA, never approvals.
pub fn inspect_main(
    root: &Path,
    main: &str,
    project_id: &str,
    docs: &str,
    previous_main: Option<&str>,
) -> Result<MainInspection, String> {
    inspect_main_inner(root, main, project_id, docs, previous_main)
        .map_err(|error| format!("inspection_incomplete: {error}"))
}
fn inspect_main_inner(
    root: &Path,
    main: &str,
    project_id: &str,
    docs: &str,
    previous_main: Option<&str>,
) -> Result<MainInspection, String> {
    oid(main)?;
    docs_path(docs)?;
    if text(root, &["rev-parse", "--is-shallow-repository"])? != "false" {
        return Err("浅历史不能证明首次导入锚点".into());
    }
    if let Some(previous) = previous_main {
        if !is_ancestor(root, previous, main)? {
            return Err("权威 main 非快进回退或改写".into());
        }
    }
    let current = config_at(root, main)?.ok_or("权威 main 没有项目配置")?;
    if current.project_id() != project_id || current.docs_dir() != docs {
        return Err("权威 main 的项目来源或文档目录不匹配".into());
    }
    let history = text(
        root,
        &["rev-list", "--topo-order", "--reverse", "--parents", main],
    )?;
    let mut ancestors_have_project = BTreeMap::<String, bool>::new();
    let mut records = Vec::new();
    let mut anchors = Vec::new();
    for line in history.lines() {
        let mut parts = line.split_whitespace();
        let commit = parts.next().ok_or("历史提交缺失")?.to_string();
        oid(&commit)?;
        let parents: Vec<String> = parts.map(str::to_owned).collect();
        let mut inherited = false;
        for parent in &parents {
            inherited |= *ancestors_have_project
                .get(parent)
                .ok_or("历史缺少父提交，无法完整核验")?;
        }
        let present =
            config_at(root, &commit)?.is_some_and(|config| config.project_id() == project_id);
        if present && !inherited {
            anchors.push(commit.clone());
        }
        ancestors_have_project.insert(commit.clone(), inherited || present);
        records.push((commit, parents));
    }
    if anchors.len() != 1 {
        return Err(format!("首次 project_id 引入点不是唯一：{}", anchors.len()));
    }
    let anchor = anchors.remove(0);
    let imported: BTreeSet<_> = text(root, &["rev-list", &anchor])?
        .lines()
        .map(str::to_owned)
        .collect();
    let mut inspected = Vec::new();
    for (commit, parents) in records {
        if imported.contains(&commit) {
            continue;
        }
        if parents.is_empty() {
            return Err("导入后出现来源不明的根提交".into());
        }
        let mut violations = Vec::new();
        for parent in &parents {
            let changes = changes(root, parent, &commit)?;
            violations.extend(inspect_changes(root, &changes, docs, None)?);
        }
        // No tag lookup: the caller must match each protected commit against an
        // actual accepted baseline/snapshot/proposal receipt in the hub.
        inspected.push(InspectedCommit {
            commit,
            parents,
            violations,
        });
    }
    Ok(MainInspection {
        main: main.into(),
        import_anchor: anchor,
        commits: inspected,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRule {
    base: String,
    directory: bool,
}
impl PathRule {
    pub fn parse(value: &str) -> Result<Self, String> {
        let (base, directory) = if let Some(base) = value.strip_suffix("/**") {
            (base, true)
        } else if let Some(base) = value.strip_suffix('/') {
            (base, true)
        } else {
            (value, false)
        };
        if base.is_empty()
            || base.starts_with('/')
            || base.contains(['\\', ':', '*', '?', '[', ']'])
            || base.chars().any(char::is_control)
            || base.split('/').any(|part| {
                part.is_empty() || part == "." || part == ".." || part.eq_ignore_ascii_case(".git")
            })
        {
            return Err(format!(
                "路径仅支持项目内字面文件、目录/ 或目录/**：{value}"
            ));
        }
        Ok(Self {
            base: base.into(),
            directory,
        })
    }
    pub fn covers(&self, child: &Self) -> bool {
        if self.directory {
            (self.base == child.base && child.directory)
                || child
                    .base
                    .strip_prefix(&self.base)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        } else {
            !child.directory && self.base == child.base
        }
    }
}

/// Whether one declared scope is contained by another. No shell glob expansion.
pub fn path_is_within(child: &str, parent: &str) -> Result<bool, String> {
    Ok(PathRule::parse(parent)?.covers(&PathRule::parse(child)?))
}

pub fn path_is_allowed(path: &str, allowed: &[String]) -> Result<bool, String> {
    let path = PathRule::parse(path)?;
    if path.directory {
        return Err("改动路径必须是具体文件".into());
    }
    for rule in allowed {
        if PathRule::parse(rule)?.covers(&path) {
            return Ok(true);
        }
    }
    Ok(false)
}
