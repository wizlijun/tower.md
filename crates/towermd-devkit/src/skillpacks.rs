//! Locked workflow skill packs for headless runs (FR-51–54, NFR-09).
//!
//! A person copies a pack into `.towermd/skillpacks/<name>@<version>/` and
//! declares it, with its content hash, in `.towermd/skillpacks.yaml`; both
//! are L3 process configuration, changed only through a person's commit.
//! Tower.md reads them from a committed revision, never from a working tree,
//! verifies the hash and assembles, per run and role, a directory holding
//! only the enabled skills. The runner passes it to the harness explicitly;
//! the user's own harness configuration is never touched.
//!
//! The hash is SHA-256 over the lines `<sha256 of file>  ./<path>\n` of
//! every file in the pack, sorted by path, i.e. what
//! `find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256`
//! prints inside the pack directory.
//!
//! The lock fails closed: a broken lock, a missing or altered copy, an
//! unknown role or a skill both enabled and disabled refuses the run.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_yaml::{Mapping, Value as Yaml};
use sha2::{Digest, Sha256};
use towermd_core::repo;

use crate::harness::{Adapter, Skills};
use crate::runner::RunConfig;

pub const LOCK: &str = ".towermd/skillpacks.yaml";
pub const DIR: &str = ".towermd/skillpacks";
/// The `type` of every lock entry (OKF, `towermd_core::types`).
pub const TYPE: &str = "SkillPack";
/// The L5 event recording what a run loaded (spec 6.3).
pub const LOADED: &str = "skill.loaded";
/// `<run_dir>/skillpacks/`: the assembled packs of one run.
pub const ASSEMBLED: &str = "skillpacks";
const GIT: Duration = Duration::from_secs(30);

/// Who a run's skills are for (FR-52).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Controller,
    Executor,
    Reviewer,
}

impl Role {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "controller" => Some(Self::Controller),
            "executor" => Some(Self::Executor),
            "reviewer" => Some(Self::Reviewer),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Controller => "controller",
            Self::Executor => "executor",
            Self::Reviewer => "reviewer",
        }
    }
}

/// Where a skill's step that stops for a person goes in Tower.md (FR-53).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InteractionPoint {
    pub maps_to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// One locked pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    pub name: String,
    pub version: String,
    pub origin: Option<String>,
    /// `sha256:<hex>` of the copy.
    pub hash: String,
    pub enabled_for: BTreeMap<Role, Vec<String>>,
    pub disabled: BTreeSet<String>,
    pub interaction_points: BTreeMap<String, InteractionPoint>,
}

impl Pack {
    /// `.towermd/skillpacks/<name>@<version>`.
    pub fn path(&self) -> String {
        format!("{DIR}/{}@{}", self.name, self.version)
    }
}

/// What one run loaded, for `skill.loaded`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Loaded {
    pub name: String,
    pub version: String,
    pub hash: String,
    pub skills: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub interaction_points: BTreeMap<String, InteractionPoint>,
    #[serde(skip)]
    pub dir: PathBuf,
}

/// `[A-Za-z0-9._+-]+`, not starting with a dot: one path segment.
fn segment(what: &str, value: &str) -> Result<(), String> {
    let ok = !value.is_empty() && !value.starts_with('.')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    if ok { Ok(()) } else { Err(format!("{what}「{value}」只能用字母、数字和 . _ - +，且不能以点开头")) }
}

fn text(entry: &Mapping, key: &str, pack: &str) -> Result<String, String> {
    match entry.get(key) {
        Some(Yaml::String(value)) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        _ => Err(format!("技能包 {pack} 缺少 {key}")),
    }
}

fn names(value: Option<&Yaml>, what: &str) -> Result<Vec<String>, String> {
    let list: Vec<String> = match value {
        None | Some(Yaml::Null) => Vec::new(),
        Some(value) => serde_yaml::from_value(value.clone()).map_err(|_| format!("{what} 须是技能名列表"))?,
    };
    for name in &list {
        segment("技能名", name)?;
    }
    Ok(list)
}

fn maps_to(value: &str) -> bool {
    matches!(value, "hold.question" | "hold.review")
        || value.strip_prefix("gate.G").is_some_and(|n| matches!(n, "0" | "1" | "2" | "3" | "4" | "5" | "6"))
}

impl Pack {
    fn parse(value: &Yaml) -> Result<Self, String> {
        let Yaml::Mapping(entry) = value else { return Err("技能包条目须是映射".into()) };
        let name = match entry.get("name") {
            Some(Yaml::String(name)) => name.trim().to_owned(),
            _ => return Err("技能包条目缺少 name".into()),
        };
        segment("技能包名", &name)?;
        if entry.get("type").and_then(Yaml::as_str) != Some(TYPE) {
            return Err(format!("技能包 {name} 的 type 须是 {TYPE}"));
        }
        let version = match entry.get("version") {
            Some(Yaml::String(version)) => version.trim().to_owned(),
            Some(Yaml::Number(version)) => version.to_string(),
            _ => return Err(format!("技能包 {name} 缺少 version")),
        };
        segment("版本", &version)?;
        let hash = text(entry, "hash", &name)?;
        if !hash.strip_prefix("sha256:").is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) {
            return Err(format!("技能包 {name} 的 hash 须写成 sha256:<64 位小写十六进制>"));
        }
        let origin = match entry.get("origin") {
            None | Some(Yaml::Null) => None,
            Some(Yaml::String(origin)) => Some(origin.trim().to_owned()),
            Some(_) => return Err(format!("技能包 {name} 的 origin 须是字符串")),
        };
        let disabled: BTreeSet<String> = names(entry.get("disabled"), "disabled")?.into_iter().collect();
        let mut enabled_for = BTreeMap::new();
        match entry.get("enabled_for") {
            None | Some(Yaml::Null) => {}
            Some(Yaml::Mapping(roles)) => {
                for (role, skills) in roles {
                    let key = role.as_str().unwrap_or_default();
                    let role = Role::parse(key).ok_or_else(|| format!("技能包 {name} 的角色「{key}」未知：只有 controller、executor、reviewer"))?;
                    let skills = names(Some(skills), "enabled_for")?;
                    if let Some(skill) = skills.iter().find(|skill| disabled.contains(*skill)) {
                        return Err(format!("技能包 {name} 的 {skill} 同时被启用和禁用"));
                    }
                    enabled_for.insert(role, skills);
                }
            }
            Some(_) => return Err(format!("技能包 {name} 的 enabled_for 须是角色到技能列表的映射")),
        }
        let mut interaction_points = BTreeMap::new();
        match entry.get("interaction_points") {
            None | Some(Yaml::Null) => {}
            Some(Yaml::Mapping(points)) => {
                for (skill, point) in points {
                    let skill = skill.as_str().unwrap_or_default().to_owned();
                    segment("技能名", &skill)?;
                    let target = point.get("maps_to").and_then(Yaml::as_str).unwrap_or_default();
                    if !maps_to(target) {
                        return Err(format!("技能包 {name} 的交互点 {skill} 须映射到 hold.question、hold.review 或 gate.G0–G6"));
                    }
                    let scope = point.get("scope").and_then(Yaml::as_str).map(str::to_owned);
                    interaction_points.insert(skill, InteractionPoint { maps_to: target.to_owned(), scope });
                }
            }
            Some(_) => return Err(format!("技能包 {name} 的 interaction_points 须是映射")),
        }
        Ok(Self { name, version, origin, hash, enabled_for, disabled, interaction_points })
    }
}

/// Parses the lock. Unknown keys are ignored; Tower.md never writes it.
pub fn parse(text: &str) -> Result<Vec<Pack>, String> {
    let map = match serde_yaml::from_str(text) {
        Ok(Yaml::Mapping(map)) => map,
        Ok(Yaml::Null) => return Ok(Vec::new()),
        Ok(_) => return Err(format!("{LOCK} 顶层必须是映射")),
        Err(error) => return Err(format!("{LOCK} 不是合法 YAML：{error}")),
    };
    let packs = match map.get("packs") {
        None | Some(Yaml::Null) => return Ok(Vec::new()),
        Some(Yaml::Sequence(packs)) => packs.iter().map(Pack::parse).collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(format!("{LOCK} 的 packs 必须是列表")),
    };
    let mut seen = BTreeSet::new();
    for pack in &packs {
        if !seen.insert(&pack.name) {
            return Err(format!("{LOCK} 中技能包 {} 出现了两次", pack.name));
        }
    }
    Ok(packs)
}

/// The lock at `revision`; none when the file is absent there.
pub fn read(root: &Path, revision: &str) -> Result<Vec<Pack>, String> {
    let commit = repo::git_timeout(root, &["rev-parse", "--verify", &format!("{revision}^{{commit}}")], GIT)?;
    if repo::git_bytes_timeout(root, &["ls-tree", "-z", &commit, "--", LOCK], GIT)?.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = repo::git_bytes_timeout(root, &["cat-file", "blob", &format!("{commit}:{LOCK}")], GIT)?;
    parse(std::str::from_utf8(&bytes).map_err(|_| format!("{LOCK} 不是 UTF-8"))?)
}

/// One file of a pack copy: its path inside the pack, bytes and mode.
struct File {
    path: String,
    bytes: Vec<u8>,
    executable: bool,
}

/// The committed copy of `pack`, refused if it holds anything but plain files.
fn files(root: &Path, commit: &str, pack: &Pack) -> Result<Vec<File>, String> {
    let prefix = format!("{}/", pack.path());
    let listing = repo::git_bytes_timeout(root, &["--literal-pathspecs", "ls-tree", "-r", "-z", commit, "--", &prefix], GIT)?;
    let mut files = Vec::new();
    for row in listing.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        let row = std::str::from_utf8(row).map_err(|_| format!("技能包 {} 中有非 UTF-8 路径", pack.name))?;
        let (metadata, path) = row.split_once('\t').ok_or("技能包 tree 条目无效")?;
        let mut fields = metadata.split(' ');
        let (mode, kind, oid) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""), fields.next().unwrap_or(""));
        if kind != "blob" || !matches!(mode, "100644" | "100755") {
            return Err(format!("技能包 {} 只能含普通文件：{path}", pack.name));
        }
        let bytes = repo::git_bytes_timeout(root, &["cat-file", "blob", oid], GIT)?;
        let inner = path.strip_prefix(&prefix).ok_or("技能包 tree 条目越界")?.to_owned();
        files.push(File { path: inner, bytes, executable: mode == "100755" });
    }
    if files.is_empty() {
        return Err(format!("{} 未提交或为空：先把技能包副本提交到仓库", pack.path()));
    }
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok(files)
}

fn digest(files: &[File]) -> String {
    let mut manifest = String::new();
    for file in files {
        manifest.push_str(&format!("{:x}  ./{}\n", Sha256::digest(&file.bytes), file.path));
    }
    format!("sha256:{:x}", Sha256::digest(manifest.as_bytes()))
}

fn write(path: &Path, file: &File) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, &file.bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if file.executable {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Assembles the packs `role` uses at `revision` under `out`, one
/// `<out>/<name>/skills/<skill>/` per enabled skill, after checking every
/// locked hash. Packs that enable nothing for `role` are left out.
pub fn assemble(root: &Path, revision: &str, role: Role, out: &Path) -> Result<Vec<Loaded>, String> {
    let commit = repo::git_timeout(root, &["rev-parse", "--verify", &format!("{revision}^{{commit}}")], GIT)?;
    let packs = read(root, &commit)?;
    if out.exists() {
        fs::remove_dir_all(out).map_err(|e| e.to_string())?;
    }
    let mut loaded = Vec::new();
    for pack in &packs {
        let files = files(root, &commit, pack)?;
        let actual = digest(&files);
        if actual != pack.hash {
            return Err(format!("技能包 {} 的内容哈希与锁定不符：锁定 {}，实际 {actual}", pack.path(), pack.hash));
        }
        let skills = pack.enabled_for.get(&role).cloned().unwrap_or_default();
        for skill in &skills {
            if !files.iter().any(|file| file.path == format!("skills/{skill}/SKILL.md")) {
                return Err(format!("技能包 {} 没有技能 {skill}（缺 skills/{skill}/SKILL.md）", pack.path()));
            }
        }
        if skills.is_empty() {
            continue;
        }
        let dir = out.join(&pack.name);
        for file in &files {
            let keep = skills.iter().any(|skill| file.path.starts_with(&format!("skills/{skill}/")));
            if keep {
                write(&dir.join(&file.path), file)?;
            }
        }
        let interaction_points = pack.interaction_points.iter()
            .filter(|(skill, _)| skills.contains(skill))
            .map(|(skill, point)| (skill.clone(), point.clone())).collect();
        loaded.push(Loaded { name: pack.name.clone(), version: pack.version.clone(), hash: pack.hash.clone(),
            skills, interaction_points, dir });
    }
    Ok(loaded)
}

/// Gives the run its role's locked skills, as far as its harness takes
/// them, in `<run_dir>/skillpacks/`. A harness without a skill channel gets
/// none, and the lock is still checked so a broken one never runs.
pub fn wire(config: &mut RunConfig, adapter: &dyn Adapter, root: &Path, revision: &str, role: Role) -> Result<Vec<Loaded>, String> {
    let loaded = assemble(root, revision, role, &config.run_dir.join(ASSEMBLED))?;
    config.skill_dirs = match adapter.capabilities().channels.skills {
        Skills::None => return Ok(Vec::new()),
        Skills::PluginDir => loaded.iter().map(|pack| pack.dir.clone()).collect(),
        Skills::SkillsDir => loaded.iter().map(|pack| pack.dir.join("skills")).collect(),
    };
    Ok(loaded)
}

#[cfg(test)]
#[path = "skillpacks_tests.rs"]
mod tests;
