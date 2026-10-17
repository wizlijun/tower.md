use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::{Config, CONFIG_PATH};
use crate::init::normalize_docs_dir;
use crate::okf::Document;
use crate::repo;
use crate::types::{self, LayerResolution};

#[derive(Debug, Clone, serde::Serialize)]
pub struct CatalogDiagnostic {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

#[derive(Debug)]
pub struct Catalog {
    pub root: PathBuf,
    pub common_dir: PathBuf,
    pub branch: String,
    pub revision: String,
    pub config: Config,
    pub objects: BTreeMap<String, Document>,
    pub object_paths: BTreeMap<String, String>,
    pub protected_paths: Vec<String>,
    pub diagnostics: Vec<CatalogDiagnostic>,
}

fn authority_coordinates(dir: &Path) -> Result<Option<(PathBuf, PathBuf, String)>, String> {
    let root = repo::toplevel(dir)?;
    let common = repo::git(&root, &["rev-parse", "--git-common-dir"])?;
    let common_dir = root
        .join(common)
        .canonicalize()
        .map_err(|error| format!("无法解析 git-common-dir：{error}"))?;
    let reference = main_reference(&root)?;
    let Some(revision) = read_authority_reference(&root, reference)? else {
        return Ok(None);
    };
    Ok(Some((root, common_dir, revision)))
}

fn read_configuration(root: &Path, revision: &str) -> Result<Option<Config>, String> {
    let entries = tree_entries(root, revision, CONFIG_PATH)?;
    let Some(entry) = entries.iter().find(|entry| entry.path == CONFIG_PATH) else {
        return Ok(None);
    };
    if !entry.is_file() {
        return Err(format!("权威 main 的 {CONFIG_PATH} 不是普通文件"));
    }
    let config = Config::parse(&read_blob(root, entry)?)?;
    let docs_dir = safe_docs_dir(config.docs_dir())?;
    let mut value: serde_yaml::Value = serde_yaml::from_str(&config.to_yaml())
        .map_err(|error| format!("无法解析配置：{error}"))?;
    if value.get("hub").and_then(serde_yaml::Value::as_str) != Some("local") {
        return Err("M0-2 只支持 hub: local，请在权威 main 配置本地 hub".into());
    }
    value["docs_dir"] = docs_dir.clone().into();
    let config = Config::parse(
        &serde_yaml::to_string(&value).map_err(|error| format!("无法解析配置：{error}"))?,
    )?;
    Ok(Some(config))
}

impl Catalog {
    pub fn load(dir: &Path) -> Result<Self, String> {
        Self::load_optional(dir)?.ok_or_else(|| String::from(
            "权威 main 不可读；请先将 init 结果提交到 main，有 origin 时须首次成功 fetch origin/main",
        ))
    }

    pub fn load_optional(dir: &Path) -> Result<Option<Self>, String> {
        let Some((root, common_dir, revision)) = authority_coordinates(dir)? else {
            return Ok(None);
        };
        Self::read_revision(root, common_dir, revision)
    }
    pub(crate) fn configuration_at_authority(dir: &Path) -> Result<(PathBuf, Config), String> {
        let (root, common_dir, revision) = authority_coordinates(dir)?.ok_or("权威 main 不可读")?;
        let config = read_configuration(&root, &revision)?.ok_or("权威 main 配置不存在")?;
        Ok((common_dir, config))
    }

    pub(crate) fn at_revision(dir: &Path, revision: &str) -> Result<Option<Self>, String> {
        let root = repo::toplevel(dir)?;
        let common = repo::git(&root, &["rev-parse", "--git-common-dir"])?;
        let common_dir = root
            .join(common)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let revision = repo::git(
            &root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{revision}^{{commit}}"),
            ],
        )?;
        Self::read_revision(root, common_dir, revision)
    }

    fn read_revision(
        root: PathBuf,
        common_dir: PathBuf,
        revision: String,
    ) -> Result<Option<Self>, String> {
        let Some(config) = read_configuration(&root, &revision)? else {
            return Ok(None);
        };
        let docs_dir = config.docs_dir();
        let branch = repo::git(&root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let prefix = format!("{docs_dir}/");
        let mut objects = BTreeMap::new();
        let mut paths = BTreeMap::new();
        let mut protected_paths = Vec::new();
        let mut diagnostics = Vec::new();
        for entry in tree_entries(&root, &revision, ".")? {
            if !entry.is_file()
                || !entry.path.ends_with(".md")
                || entry.path.starts_with(".towermd/")
                || entry.path.starts_with(&format!("{prefix}state/"))
            {
                continue;
            }
            let text = read_blob(&root, &entry)?;
            if text.lines().next() != Some("---") {
                continue;
            }
            let document = match Document::parse(&text) {
                Ok(document) => document,
                Err(_) if ordinary_frontmatter(&text) => continue,
                Err(error) => return Err(format!("{}：无法判定对象层级：{error}", entry.path)),
            };
            let layer = types::layer_of(&document);
            if layer == LayerResolution::Invalid {
                diagnostics.push(CatalogDiagnostic {
                    code: "invalid_layer",
                    path: entry.path.clone(),
                    message: "非法 layer；按慢层保护，不采用 type 默认层".into(),
                });
            }
            if layer.is_protected() {
                protected_paths.push(entry.path.clone());
            }
            let Some(relative) = entry.path.strip_prefix(&prefix) else {
                continue;
            };
            if relative == "AGENTS.md"
                || relative.split('/').any(|part| part == ".towermd")
                || ["state/", "artifacts/"]
                    .iter()
                    .any(|directory| relative.starts_with(directory))
            {
                continue;
            }
            if document.get("id").is_none() {
                continue;
            }
            let id = document
                .get_str("id")
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| format!("{} 的 id 必须为非空字符串", entry.path))?;
            if let Some(previous) = paths.insert(id.to_string(), entry.path.clone()) {
                return Err(format!("重复对象 ID `{id}`：{previous} 与 {}", entry.path));
            }
            objects.insert(id.to_string(), document);
        }
        Ok(Some(Self {
            root,
            common_dir,
            branch,
            revision,
            config,
            objects,
            object_paths: paths,
            protected_paths,
            diagnostics,
        }))
    }

    pub fn read_file(&self, path: &str) -> Result<Option<String>, String> {
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path.contains('\0')
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err("路径必须是仓库根内规范的 POSIX 相对路径".into());
        }
        let entries = tree_entries(&self.root, &self.revision, path)?;
        match entries.iter().find(|entry| entry.path == path) {
            Some(entry) if entry.is_file() => read_blob(&self.root, entry).map(Some),
            Some(_) => Err("拒绝读取符号链接、子模块或非普通文件".into()),
            None => Ok(None),
        }
    }
}

fn ordinary_frontmatter(text: &str) -> bool {
    let mut lines = text.lines().skip(1);
    let mut frontmatter = String::new();
    for line in lines.by_ref() {
        if line == "---" {
            return serde_yaml::from_str::<serde_yaml::Mapping>(&frontmatter).is_ok_and(
                |mapping| !mapping.contains_key("type") && !mapping.contains_key("layer"),
            );
        }
        frontmatter.push_str(line);
        frontmatter.push('\n');
    }
    false
}

struct TreeEntry {
    mode: String,
    oid: String,
    size: Option<usize>,
    path: String,
}

impl TreeEntry {
    fn is_file(&self) -> bool {
        matches!(self.mode.as_str(), "100644" | "100755")
    }
}

fn tree_entries(root: &Path, revision: &str, path: &str) -> Result<Vec<TreeEntry>, String> {
    let output = repo::git(
        root,
        &[
            "--literal-pathspecs",
            "ls-tree",
            "-rz",
            "--full-tree",
            "--long",
            revision,
            "--",
            path,
        ],
    )?;
    output
        .split_terminator('\0')
        .map(|record| {
            let invalid = || format!("无法解析 git ls-tree 记录：{record:?}");
            let (metadata, path) = record.split_once('\t').ok_or_else(invalid)?;
            let fields: Vec<_> = metadata.split_whitespace().collect();
            if fields.len() != 4
                || !matches!(fields[2].len(), 40 | 64)
                || !fields[2].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(invalid());
            }
            let size = if fields[1] == "blob" {
                Some(fields[3].parse().map_err(|_| invalid())?)
            } else {
                None
            };
            Ok(TreeEntry {
                mode: fields[0].to_string(),
                oid: fields[2].to_string(),
                size,
                path: path.to_string(),
            })
        })
        .collect()
}

fn read_blob(root: &Path, entry: &TreeEntry) -> Result<String, String> {
    let mut text = repo::git(root, &["cat-file", "blob", &entry.oid])
        .map_err(|error| format!("无法读取 {}：{error}", entry.path))?;
    if entry.size == Some(text.len() + 1) {
        text.push('\n');
    }
    if entry.size != Some(text.len()) {
        return Err(format!("{} 的 blob 长度不匹配", entry.path));
    }
    Ok(text)
}

fn safe_docs_dir(raw: &str) -> Result<String, String> {
    let normalized = normalize_docs_dir(raw)?;
    if normalized.contains(['*', '?', '[', ']', '{', '}']) {
        return Err(format!("docs_dir 不能包含 glob：{raw}"));
    }
    Ok(normalized)
}

pub fn validate_paths(
    allowed: &[String],
    module_paths: &[String],
    docs_dir: &str,
) -> Result<(), String> {
    let docs_dir = safe_docs_dir(docs_dir)?;
    let mut protected = vec![".towermd".to_string(), ".git".to_string()];
    for name in [
        "intents",
        "arch",
        "specs",
        "baselines",
        "state",
        "tower.canvas",
    ] {
        protected.push(format!("{docs_dir}/{name}"));
    }
    for path in allowed.iter().chain(module_paths) {
        validate_pattern(path)?;
    }
    for path in allowed {
        if path
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git") || part.eq_ignore_ascii_case(".towermd"))
        {
            return Err(format!("路径 `{path}` 包含保护目录，不能授权"));
        }
        let literal_prefix = path
            .split('/')
            .take_while(|part| !has_glob(part))
            .collect::<Vec<_>>()
            .join("/");
        let prefix = literal_prefix.to_lowercase();
        for reserved in &protected {
            let reserved = reserved.to_lowercase();
            if prefix.is_empty() || within(&prefix, &reserved) || within(&reserved, &prefix) {
                return Err(format!(
                    "路径 `{path}` 可能覆盖保护区 `{reserved}`，不能授权"
                ));
            }
        }
    }
    for requested in allowed {
        let covered = module_paths.iter().any(|module| {
            requested == module
                || module == "**" && simple_path(requested).is_some()
                || module.strip_suffix("/**").is_some_and(|prefix| {
                    !has_glob(prefix)
                        && simple_path(requested).is_some_and(|path| {
                            path != prefix && within(path, prefix)
                                || requested.ends_with("/**") && path == prefix
                        })
                })
        });
        if !covered {
            return Err(format!("无法证明 `{requested}` 是模块 paths 的子集"));
        }
    }
    Ok(())
}

fn within(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|tail| tail.starts_with('/'))
}

pub fn validate_protected_paths(allowed: &[String], protected: &[String]) -> Result<(), String> {
    for path in protected {
        if path.contains(['\\', '\0'])
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
        {
            return Err(format!("非法保护对象路径：`{path}`"));
        }
    }
    for pattern in allowed {
        validate_pattern(pattern)?;
        let normalized = pattern.to_lowercase();
        let literal_prefix = normalized
            .split('/')
            .take_while(|part| !has_glob(part))
            .collect::<Vec<_>>()
            .join("/");
        for path in protected {
            let normalized_path = path.to_lowercase();
            let overlaps = if !has_glob(&normalized) {
                normalized == normalized_path
            } else {
                literal_prefix.is_empty() || within(&normalized_path, &literal_prefix)
            };
            if overlaps {
                return Err(format!(
                    "路径 `{pattern}` 可能覆盖慢层保护对象 `{path}`，不能授权"
                ));
            }
        }
    }
    Ok(())
}

fn has_glob(path: &str) -> bool {
    path.contains(['*', '?', '[', ']', '{', '}'])
}

fn simple_path(pattern: &str) -> Option<&str> {
    let path = pattern.strip_suffix("/**").unwrap_or(pattern);
    (!has_glob(path)).then_some(path)
}

fn validate_pattern(pattern: &str) -> Result<(), String> {
    if pattern.chars().any(char::is_control)
        || pattern.contains(['\\', ':', '[', ']', '{', '}'])
        || pattern.split('/').any(|part| {
            part.is_empty() || matches!(part, "." | "..") || part.contains("**") && part != "**"
        })
    {
        return Err(format!("非法或暂不支持的路径 glob：`{pattern}`"));
    }
    Ok(())
}

pub fn main_reference(root: &Path) -> Result<&'static str, String> {
    let remotes = repo::git(root, &["remote"])?;
    Ok(if remotes.lines().any(|remote| remote == "origin") {
        "refs/remotes/origin/main"
    } else {
        "refs/heads/main"
    })
}

pub(crate) fn read_authority_reference(
    root: &Path,
    reference: &str,
) -> Result<Option<String>, String> {
    // One actual read provides exact identity, symbolic target, type and parsed commit tree.
    let output = repo::git_output(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(symref)%00%(objecttype)%00%(tree)",
            reference,
        ],
    )?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err(format!(
            "权威 main 引用读取失败：{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let record = String::from_utf8(output.stdout)
        .map_err(|error| format!("无法解析权威 main 引用：{error}"))?;
    let record = record.strip_suffix('\n').unwrap_or(&record);
    if record.is_empty() {
        if repo::git(root, &["symbolic-ref", "--quiet", reference]).is_ok() {
            return Err("权威 main 是符号引用；拒绝读取用户分支".into());
        }
        return Ok(None);
    }
    let fields: Vec<_> = record.split('\0').collect();
    let valid_oid = |oid: &str| {
        matches!(oid.len(), 40 | 64)
            && oid
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if record.contains('\n') || fields.len() != 5 || fields[0] != reference || !valid_oid(fields[1])
    {
        return Err("无法解析精确权威 main 引用".into());
    }
    if !fields[2].is_empty() {
        return Err("权威 main 是符号引用；拒绝读取用户分支".into());
    }
    match fields[3] {
        "commit" if valid_oid(fields[4]) => Ok(Some(fields[1].into())),
        "commit" => Err("权威 main commit 缺少合法 tree".into()),
        "tag" | "tree" | "blob" => repo::git(
            root,
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        )
        .map(Some),
        _ => Err("权威 main 引用 objecttype 非法".into()),
    }
}

/// Lightweight authority probe; no document or archive decoding.
pub(crate) fn authority_revision(root: &Path) -> Result<String, String> {
    read_authority_reference(root, main_reference(root)?)?.ok_or_else(|| "权威 main 不可读".into())
}
