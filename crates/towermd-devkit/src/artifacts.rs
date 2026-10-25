//! WP-X raw run output (FR-08, FR-75–78). Raw output never goes into git.
//! After a dispatched attempt ends the daemon records where its raw files
//! are, with their content hashes (`artifact.archived`, immutable):
//!
//! - **Arca**: when the `arca` CLI is installed and `<docs_dir>/artifacts/`
//!   already is an Arca dataset (a person ran `arca setup` or `arca adopt`),
//!   the files are copied to `<docs_dir>/artifacts/<run>/<n>/` and handed to
//!   `arca add`. Tower.md never sets up a dataset, pushes, or runs `arca gc`.
//! - **Local** otherwise, or when any Arca step fails: the files stay in
//!   `<git-common-dir>/towermd/runs/<run>/<n>/`, inside the git directory and
//!   so never tracked; the App labels them "仅本机".
//!
//! Arca is called as a subprocess and never required. `TOWERMD_ARCA` names
//! another program (a mock in tests); set to an empty value it turns Arca off.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use towermd_core::hub::Event;
use towermd_core::hub_protocol::NewEvent;

use crate::runner;

pub const ARCHIVER: &str = "process:towermd-archiver";
pub const ARCHIVED: &str = "artifact.archived";
pub const GROUP: &str = "Artifact";
pub const DIR: &str = "artifacts";
const FILES: [&str; 2] = [runner::RAW, runner::STDERR];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    Arca,
    /// Only on this machine.
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub name: String,
    /// Arca: relative to the repository root. Local: relative to the git common dir.
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    /// What `arca resolve` called it, when it said.
    #[serde(default)]
    pub arca: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub run: String,
    pub task: String,
    pub attempt: u32,
    pub store: Store,
    pub files: Vec<File>,
    /// Why the files are only local although Arca was looked for.
    #[serde(default)]
    pub note: Option<String>,
}

pub fn archives(state: &Value) -> Result<BTreeMap<String, Archive>, String> {
    state.get(GROUP).map_or(Ok(BTreeMap::new()), |group| serde_json::from_value(group.clone()).map_err(|e| e.to_string()))
}

/// Run attempts whose raw files are on this machine only.
pub fn local_only(state: &Value) -> Result<usize, String> {
    Ok(archives(state)?.values().filter(|archive| archive.store == Store::Local).count())
}

/// The `arca` program, if one answers `--version`.
pub fn arca() -> Option<PathBuf> {
    let program = match std::env::var_os("TOWERMD_ARCA") {
        Some(value) if value.is_empty() => return None,
        Some(value) => PathBuf::from(value),
        None => PathBuf::from("arca"),
    };
    let ok = Command::new(&program).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .status().is_ok_and(|status| status.success());
    ok.then_some(program)
}

fn hash(path: &Path) -> Option<(String, u64)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 { break }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Some((hex::encode(hasher.finalize()), bytes))
}

fn arca_run(program: &Path, root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program).args(args).current_dir(root).stdin(Stdio::null()).output()
        .map_err(|error| format!("无法运行 arca：{error}"))?;
    if !output.status.success() {
        return Err(format!("arca {} 失败：{}", args[0], String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The identity `arca resolve --json` gives, read leniently.
fn identity(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    ["hash", "blake3", "id", "version"].iter().find_map(|key| value[key].as_str().map(str::to_owned))
}

fn to_arca(program: &Path, root: &Path, docs_dir: &str, run: &str, attempt: u32, files: &mut [File], source: &Path) -> Result<(), String> {
    let dataset = root.join(docs_dir).join(DIR);
    if !dataset.join(".arca").is_dir() {
        return Err(format!("{docs_dir}/{DIR} 还不是 Arca 数据集；Tower.md 不替你建立（由人运行 arca setup 或 arca adopt）"));
    }
    let target = dataset.join(run).join(attempt.to_string());
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    for file in files.iter_mut() {
        let relative = format!("{docs_dir}/{DIR}/{run}/{attempt}/{}", file.name);
        let copy = target.join(&file.name);
        std::fs::copy(source.join(&file.name), &copy).map_err(|e| e.to_string())?;
        if hash(&copy).map(|(sha, _)| sha).as_deref() != Some(file.sha256.as_str()) {
            return Err("复制到 Arca 数据集的文件与原文件不同".into());
        }
        arca_run(program, root, &["add", &relative])?;
        file.arca = arca_run(program, root, &["resolve", &relative, "--json"]).ok().and_then(|text| identity(&text));
        file.path = relative;
    }
    Ok(())
}

/// Archive one attempt's raw files. Never fails: the worst case is local.
pub fn archive(root: &Path, docs_dir: &str, common: &Path, arca: Option<&Path>, run: &str, task: &str, attempt: u32) -> Archive {
    let source = runner::run_dir(common, run).join(attempt.to_string());
    let local: Vec<File> = FILES.iter().filter_map(|name| {
        let (sha256, bytes) = hash(&source.join(name))?;
        Some(File { name: (*name).into(), path: format!("towermd/runs/{run}/{attempt}/{name}"), sha256, bytes, arca: None })
    }).collect();
    let mut archive = Archive { run: run.into(), task: task.into(), attempt, store: Store::Local, files: local.clone(), note: None };
    let Some(program) = arca else {
        archive.note = Some("没有安装 Arca，原始输出只留在本机".into());
        return archive;
    };
    if archive.files.is_empty() { return archive }
    match to_arca(program, root, docs_dir, run, attempt, &mut archive.files, &source) {
        Ok(()) => archive.store = Store::Arca,
        Err(error) => { archive.files = local; archive.note = Some(error); }
    }
    archive
}

/// FR-78 `arca doctor --json` at daemon start: how many files are not yet
/// on a hub. `None` when Arca is absent or its answer is unreadable.
pub fn unsynced(program: &Path, root: &Path) -> Option<u64> {
    let text = arca_run(program, root, &["doctor", "--json"]).ok()?;
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    ["unsynced", "local_only", "not_pushed"].iter().find_map(|key| value[key].as_u64())
}

pub fn event(host: &str, archive: &Archive) -> Result<NewEvent, String> {
    Ok(NewEvent { eid: uuid::Uuid::now_v7().to_string(), host: host.into(), actor: ARCHIVER.into(), kind: ARCHIVED.into(),
        layer: "L5".into(), obj: archive.task.clone(), data: serde_json::to_value(archive).map_err(|e| e.to_string())?,
        guards: Vec::new() })
}

fn safe(path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
}

pub(crate) fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != ARCHIVED { return Ok(false) }
    let archive: Archive = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if event.actor != ARCHIVER || event.layer != "L5" || !event.guards.is_empty() || event.obj != archive.task {
        return Err("原始输出归档须由归档进程在 L5 记录".into());
    }
    let records = crate::run_record::records(state)?;
    let record = records.get(&archive.run).ok_or("归档的运行没有开始记录")?;
    if record.task != archive.task || archive.attempt == 0 || archive.attempt > record.starts {
        return Err("归档与运行记录不符".into());
    }
    let valid = archive.files.iter().all(|file| FILES.contains(&file.name.as_str()) && safe(&file.path)
        && file.sha256.len() == 64 && file.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
    if !valid || (archive.store == Store::Arca && archive.files.is_empty()) {
        return Err("归档文件记录无效".into());
    }
    let key = format!("{}/{}", archive.run, archive.attempt);
    let group = state.as_object_mut().ok_or("项目投影格式无效")?.entry(GROUP).or_insert_with(|| json!({}));
    let value = serde_json::to_value(&archive).map_err(|e| e.to_string())?;
    match group.get(&key) {
        // Immutable: a correction is a new piece of evidence, not an edit.
        Some(old) if old != &value => return Err("该次运行的原始输出已归档".into()),
        Some(_) => {}
        None => { group[&key] = value; }
    }
    Ok(true)
}
