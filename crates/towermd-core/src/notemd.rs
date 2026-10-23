//! Note.md connector (FR-79, FR-80). Tower.md and Note.md only cite each
//! other: a reference records the Note.md OKF id plus a block reference, and
//! a summary is a plain `.md` file dropped into the vault's capture folder.
//! The connector never writes `.note.md`, never links, never overwrites a
//! vault file and never runs git in the vault. Entering the knowledge web
//! stays a human step inside Note.md.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

use crate::okf::Document;

/// `generated.by` of every summary (FR-80).
pub const WRITER: &str = "process:towermd";
/// Our own folder inside the vault's capture directory.
pub const CAPTURE_FOLDER: &str = "towermd";
/// Note.md's default quick-note inbox, used when the vault sets none.
pub const DEFAULT_INBOX: &str = "inbox";

/// A reference to Note.md knowledge (FR-79). The OKF id (concept path
/// without `.md`) and the block id carry the reference; `path` is only a
/// locating hint, so a stale path never breaks it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteRef {
    pub okf_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl NoteRef {
    pub fn check(&self) -> Result<(), String> {
        concept_id(&self.okf_id)?;
        if let Some(block) = &self.block {
            if !block_id(block) {
                return Err(format!("Note.md 块引用无效：`{block}`，应为 b- 加 6 位小写十六进制"));
            }
        }
        if let Some(path) = &self.path {
            vault_path(path)?;
            if !path.ends_with(".md") {
                return Err(format!("Note.md 路径提示必须是 .md 文件：`{path}`"));
            }
        }
        if let Some(title) = &self.title {
            if title.trim().is_empty() || title.chars().any(char::is_control) {
                return Err("Note.md 来源标题不能为空或含控制字符".into());
            }
        }
        Ok(())
    }

    /// Parses a Note.md block citation `((page.md#b-1a2b3c))`.
    pub fn from_citation(text: &str) -> Result<Self, String> {
        let inner = text.trim().strip_prefix("((").and_then(|rest| rest.strip_suffix("))"))
            .ok_or_else(|| format!("不是 Note.md 块引用：`{text}`"))?;
        let (page, block) = inner.rsplit_once('#').ok_or_else(|| format!("块引用缺少 #：`{text}`"))?;
        let page = page.strip_prefix('/').unwrap_or(page);
        let okf_id = page.strip_suffix(".md")
            .ok_or_else(|| format!("块引用必须指明 .md 页面：`{text}`"))?;
        let reference = Self {
            okf_id: okf_id.into(),
            block: Some(block.into()),
            path: Some(page.into()),
            title: None,
        };
        reference.check()?;
        Ok(reference)
    }

    /// The citation as Note.md writes it, when the reference has a block.
    pub fn citation(&self) -> Option<String> {
        self.block.as_ref().map(|block| format!("(({}#{block}))", self.resource()))
    }

    fn resource(&self) -> String {
        self.path.clone().unwrap_or_else(|| format!("{}.md", self.okf_id))
    }

    /// One OKF `sources` entry; `resource` is the locating hint, `okf_id`
    /// and `block` carry the reference.
    pub fn to_source(&self, key: &str) -> Value {
        let mut entry = Mapping::new();
        entry.insert("id".into(), key.into());
        entry.insert("resource".into(), self.resource().into());
        if let Some(title) = &self.title {
            entry.insert("title".into(), title.as_str().into());
        }
        entry.insert("okf_id".into(), self.okf_id.as_str().into());
        if let Some(block) = &self.block {
            entry.insert("block".into(), block.as_str().into());
        }
        Value::Mapping(entry)
    }

    /// `None` for a source that is not a Note.md reference; tolerant of any
    /// other keys an entry carries.
    pub fn from_source(entry: &Value) -> Option<Result<Self, String>> {
        let okf_id = entry.get("okf_id")?;
        let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_string);
        let reference = Self {
            okf_id: okf_id.as_str().unwrap_or_default().into(),
            block: text("block"),
            path: text("resource").filter(|resource| vault_path(resource).is_ok() && resource.ends_with(".md")),
            title: text("title"),
        };
        Some(reference.check().map(|_| reference))
    }
}

/// Every valid Note.md reference in a document's `sources`; anything else,
/// including malformed entries, is left alone.
pub fn note_refs(document: &Document) -> Vec<NoteRef> {
    document.get("sources").and_then(Value::as_sequence).into_iter().flatten()
        .filter_map(NoteRef::from_source).filter_map(Result::ok).collect()
}

/// `sources` entries for references, keyed `note-1`, `note-2`, …
pub fn note_sources(references: &[NoteRef]) -> Vec<Value> {
    references.iter().enumerate()
        .map(|(index, reference)| reference.to_source(&format!("note-{}", index + 1))).collect()
}

fn block_id(value: &str) -> bool {
    value.strip_prefix("b-").is_some_and(|hex| {
        hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn concept_id(value: &str) -> Result<(), String> {
    vault_path(value)?;
    if value.ends_with(".md") || value.contains(['(', ')', '#', '[', ']']) {
        return Err(format!("Note.md OKF id 无效：`{value}`，应为去掉 .md 的库内路径"));
    }
    Ok(())
}

/// A vault-relative path in Note.md's own terms: no absolute, no `..`, no
/// drive prefix, no backslash, no empty or `.` segment.
fn vault_path(value: &str) -> Result<(), String> {
    let bad = value.is_empty()
        || value != value.trim()
        || value.starts_with('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value.split('/').any(|segment| segment.is_empty() || segment == "." || segment == ".." || drive(segment));
    if bad {
        return Err(format!("Note.md 库内路径无效：`{value}`"));
    }
    Ok(())
}

fn drive(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Resolves `notemd.vault`: absolute, `~/…` under `home`, or relative to the
/// repository root. `None` when `~` is used and no home is known.
pub fn resolve_vault(raw: &str, root: &Path, home: Option<&Path>) -> Option<PathBuf> {
    if raw == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.map(|home| home.join(rest));
    }
    let path = Path::new(raw);
    Some(if path.is_absolute() { path.to_path_buf() } else { root.join(path) })
}

/// The vault's capture directory as Note.md resolves it: `inboxDir` from
/// `.notemd/settings.json` when present and valid, otherwise `inbox`.
pub fn inbox_dir(vault: &Path) -> String {
    fs::read(vault.join(".notemd/settings.json")).ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|settings| settings.get("inboxDir").and_then(|dir| dir.as_str()).and_then(relative_dir))
        .unwrap_or_else(|| DEFAULT_INBOX.into())
}

/// Note.md's `validate_rel_dir`, so both sides land on the same folder.
fn relative_dir(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return None;
    }
    let mut parts = Vec::new();
    for segment in trimmed.split(['/', '\\']).map(str::trim) {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." || drive(segment) {
            return None;
        }
        parts.push(segment);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// One OKF source of a summary. `resource` is a non-followable descriptor
/// such as `git:<sha>` or `towermd:<project>/<id>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub resource: String,
    pub title: String,
}

pub fn commit_source(commit: &str) -> Source {
    Source { id: "commit".into(), resource: format!("git:{commit}"), title: format!("提交 {}", short(commit)) }
}

pub fn object_source(key: &str, project_id: &str, id: &str, title: &str) -> Source {
    Source { id: key.into(), resource: format!("towermd:{project_id}/{id}"), title: title.into() }
}

pub fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

pub fn commit_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A summary waiting to be written. `name` is the stable file stem.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub name: String,
    pub doc_type: &'static str,
    pub title: String,
    pub description: String,
    pub sources: Vec<Source>,
    pub notes: Vec<NoteRef>,
    pub body: String,
}

impl Capture {
    /// The OKF document: frontmatter, body, then one footnote per source.
    pub fn render(&self, at: &str) -> Result<String, String> {
        file_name(&self.name)?;
        let mut body = self.body.trim_end().to_string();
        body.push_str("\n\n");
        for source in &self.sources {
            body.push_str(&format!("[^{}]: {}\n", source.id, source.title));
        }
        for (index, note) in self.notes.iter().enumerate() {
            let label = note.title.as_deref().unwrap_or(&note.okf_id);
            body.push_str(&format!("[^note-{}]: {label}\n", index + 1));
        }
        let mut document = Document::new(self.doc_type, &body)?;
        document.set("title", self.title.as_str().into())?;
        document.set("description", self.description.as_str().into())?;
        document.set("tags", Value::Sequence(vec!["towermd".into()]))?;
        let mut generated = Mapping::new();
        generated.insert("by".into(), WRITER.into());
        generated.insert("at".into(), at.into());
        document.set("generated", Value::Mapping(generated))?;
        let mut sources: Vec<Value> = self.sources.iter().map(|source| {
            let mut entry = Mapping::new();
            entry.insert("id".into(), source.id.as_str().into());
            entry.insert("resource".into(), source.resource.as_str().into());
            entry.insert("title".into(), source.title.as_str().into());
            Value::Mapping(entry)
        }).collect();
        sources.extend(note_sources(&self.notes));
        document.set("sources", Value::Sequence(sources))?;
        Ok(document.to_text())
    }
}

/// A stable, plain file stem: never hidden, never a `.note` page.
fn file_name(name: &str) -> Result<(), String> {
    let bad = name.is_empty()
        || name.len() > 120
        || name.starts_with('.')
        || name.to_ascii_lowercase().ends_with(".note")
        || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if bad {
        return Err(format!("摘要文件名无效：`{name}`"));
    }
    Ok(())
}

/// The summary name of a decision, when it is one people have accepted.
pub fn decision_name(document: &Document) -> Option<String> {
    let id = document.get_str("id")?;
    let accepted = matches!(document.get_str("status"), Some("accepted" | "stable"));
    let name = format!("decision-{id}");
    (document.doc_type() == Some("Decision") && accepted && file_name(&name).is_ok()).then_some(name)
}

/// FR-80 decision summary; `commit` is the main commit that recorded it.
pub fn decision_summary(project_id: &str, document: &Document, commit: &str) -> Option<Capture> {
    let name = decision_name(document)?;
    if !commit_id(commit) {
        return None;
    }
    let id = document.get_str("id")?;
    let text = |key: &str| document.get_str(key).map(str::trim).filter(|value| !value.is_empty());
    let decided = text("decision").map(str::to_string)
        .unwrap_or_else(|| document.body().trim().to_string());
    let heading = text("title").or_else(|| text("question")).unwrap_or(id);
    let mut body = format!("# 决策 {id}：{heading}\n\n");
    if let Some(question) = text("question") {
        body.push_str(&format!("问题：{question}\n\n"));
    }
    body.push_str(&format!("{decided}\n\n"));
    body.push_str(&format!("在 Tower.md 中接受（决策 {id}[^decision]），记录于提交 `{}`。[^commit]\n", short(commit)));
    let notes = note_refs(document);
    if !notes.is_empty() {
        body.push_str("\n引用的 Note.md 知识：\n\n");
        for (index, note) in notes.iter().enumerate() {
            let cited = note.citation().unwrap_or_else(|| note.okf_id.clone());
            body.push_str(&format!("- `{cited}`[^note-{}]\n", index + 1));
        }
    }
    let description = decided.lines().find(|line| !line.trim().is_empty()).unwrap_or(heading);
    Some(Capture {
        name,
        doc_type: "Decision Summary",
        title: format!("决策 {id}：{heading}"),
        description: description.chars().take(200).collect(),
        sources: vec![commit_source(commit), object_source("decision", project_id, id, &format!("Tower.md 决策 {id}"))],
        notes,
        body,
    })
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    vault: String,
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    seen: BTreeSet<String>,
}

/// Writes summaries into one vault, remembering what it already wrote in a
/// machine-local ledger. The ledger is derived: losing it, or pointing the
/// project at another vault, starts a new baseline that marks everything
/// already closed as seen and writes nothing, so connecting a vault never
/// floods its inbox. Files people moved or deleted are never recreated.
pub struct Writeback {
    directory: PathBuf,
    ledger_path: PathBuf,
    ledger: Ledger,
    baseline: bool,
    at: String,
}

impl Writeback {
    /// `project` is the repository the daemon serves; a vault whose capture
    /// folder falls inside it is refused, so writeback never edits the tree.
    pub fn open(vault: &Path, ledger_path: &Path, project: &Path) -> Result<Self, String> {
        if !fs::metadata(vault).is_ok_and(|meta| meta.is_dir()) {
            return Err(format!("Note.md 库不是已存在的目录：{}", vault.display()));
        }
        let vault = vault.canonicalize().map_err(|error| format!("无法解析 Note.md 库：{error}"))?;
        let directory = vault.join(inbox_dir(&vault)).join(CAPTURE_FOLDER);
        if let Ok(project) = project.canonicalize() {
            if directory.starts_with(&project) {
                return Err("Note.md 捕获目录位于项目仓库内；回写只写库外目录".into());
            }
        }
        let name = vault.to_string_lossy().into_owned();
        let previous = fs::read(ledger_path).ok()
            .and_then(|bytes| serde_json::from_slice::<Ledger>(&bytes).ok())
            .filter(|ledger| ledger.vault == name);
        let baseline = previous.is_none();
        Ok(Self {
            directory,
            ledger_path: ledger_path.to_path_buf(),
            ledger: previous.unwrap_or(Ledger { vault: name, ..Ledger::default() }),
            baseline,
            at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// True on the first pass for this vault: callers mark, never write.
    pub fn baseline(&self) -> bool {
        self.baseline
    }

    /// The main commit whose decisions were last fully offered.
    pub fn scanned(&self) -> Option<&str> {
        self.ledger.main.as_deref()
    }

    pub fn set_scanned(&mut self, main: &str) {
        self.ledger.main = Some(main.into());
    }

    pub fn pending(&self, name: &str) -> bool {
        !self.baseline && !self.ledger.seen.contains(name)
    }

    pub fn mark(&mut self, name: &str) {
        self.ledger.seen.insert(name.into());
    }

    /// Creates `<capture>/towermd/<name>.md` if absent. An existing file is
    /// never touched; it only counts as written.
    pub fn write(&mut self, capture: &Capture) -> Result<Option<PathBuf>, String> {
        let text = capture.render(&self.at)?;
        fs::create_dir_all(&self.directory)
            .map_err(|error| format!("无法创建 Note.md 捕获目录 {}：{error}", self.directory.display()))?;
        if !fs::symlink_metadata(&self.directory).is_ok_and(|meta| meta.is_dir()) {
            return Err(format!("Note.md 捕获目录不是普通目录：{}", self.directory.display()));
        }
        let path = self.directory.join(format!("{}.md", capture.name));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                self.mark(&capture.name);
                return Ok(None);
            }
            Err(error) => return Err(format!("无法写入 {}：{error}", path.display())),
        };
        if let Err(error) = file.write_all(text.as_bytes()).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(format!("无法写入 {}：{error}", path.display()));
        }
        self.mark(&capture.name);
        Ok(Some(path))
    }

    pub fn save(&self) -> Result<(), String> {
        let parent = self.ledger_path.parent().ok_or("回写账本路径无效")?;
        fs::create_dir_all(parent).map_err(|error| format!("无法创建回写账本目录：{error}"))?;
        let temporary = self.ledger_path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&self.ledger).map_err(|error| error.to_string())?;
        fs::write(&temporary, bytes).map_err(|error| format!("无法写入回写账本：{error}"))?;
        fs::rename(&temporary, &self.ledger_path).map_err(|error| format!("无法写入回写账本：{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn capture(name: &str) -> Capture {
        Capture {
            name: name.into(),
            doc_type: "Task Summary",
            title: "T-1 修复登录".into(),
            description: "Tower.md 任务 T-1 已关闭".into(),
            sources: vec![commit_source(SHA), object_source("task", "p1", "T-1", "Tower.md 任务 T-1")],
            notes: vec![],
            body: "# T-1\n\n完成。[^task][^commit]\n".into(),
        }
    }

    fn files(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() { stack.push(path) } else { found.push(path) }
            }
        }
        found.sort();
        found
    }

    #[test]
    fn note_refs_parse_citations_and_reject_bad_ids() {
        let reference = NoteRef::from_citation("((notes/raft.md#b-1a2b3c))").unwrap();
        assert_eq!(reference.okf_id, "notes/raft");
        assert_eq!(reference.block.as_deref(), Some("b-1a2b3c"));
        assert_eq!(reference.citation().as_deref(), Some("((notes/raft.md#b-1a2b3c))"));
        assert_eq!(NoteRef::from_citation("((/raft.note.md#b-000000))").unwrap().okf_id, "raft.note");
        for bad in ["((raft.md#b-1A2B3C))", "((raft.md#b-12345))", "((raft#b-123456))", "((#b-123456))",
            "((../raft.md#b-123456))", "raft.md#b-123456", "((C:/raft.md#b-123456))"] {
            assert!(NoteRef::from_citation(bad).is_err(), "{bad}");
        }
        for okf_id in ["", "a//b", "a/./b", "a/../b", "/abs", "x.md", "a\\b", "x#y", " x"] {
            let reference = NoteRef { okf_id: okf_id.into(), block: None, path: None, title: None };
            assert!(reference.check().is_err(), "{okf_id:?}");
        }
    }

    #[test]
    fn note_sources_round_trip_and_reads_are_tolerant() {
        let reference = NoteRef { okf_id: "kb/raft".into(), block: Some("b-00ff00".into()),
            path: Some("archive/raft.md".into()), title: Some("Raft 笔记".into()) };
        let mut document = Document::new("Decision", "x\n").unwrap();
        let mut sources = note_sources(std::slice::from_ref(&reference));
        sources.push(serde_yaml::from_str("{ id: web, resource: 'https://example.com', extra: 1 }").unwrap());
        sources.push(serde_yaml::from_str("{ okf_id: '../escape', block: b-zzzzzz }").unwrap());
        sources.push(Value::from("not a mapping"));
        document.set("sources", Value::Sequence(sources)).unwrap();
        let reread = Document::parse(&document.to_text()).unwrap();
        assert_eq!(note_refs(&reread), vec![reference.clone()]);
        let entry = &reread.get("sources").unwrap().as_sequence().unwrap()[0];
        assert_eq!(entry.get("resource").unwrap(), "archive/raft.md");
        // A dead path hint never breaks the reference; it only stops hinting.
        let moved: Value = serde_yaml::from_str("{ okf_id: kb/raft, block: b-00ff00, resource: 'https://x/raft' }").unwrap();
        assert_eq!(NoteRef::from_source(&moved).unwrap().unwrap().path, None);
        assert!(serde_json::from_value::<NoteRef>(serde_json::json!({"okf_id":"a","other":1})).is_err());
    }

    #[test]
    fn vault_resolution_and_inbox_follow_note_md() {
        let root = Path::new("/repo");
        assert_eq!(resolve_vault("~/notes", root, Some(Path::new("/home/u"))), Some(PathBuf::from("/home/u/notes")));
        assert_eq!(resolve_vault("~/notes", root, None), None);
        assert_eq!(resolve_vault("../vault", root, None), Some(PathBuf::from("/repo/../vault")));
        let vault = tempfile::tempdir().unwrap();
        assert_eq!(inbox_dir(vault.path()), "inbox");
        fs::create_dir(vault.path().join(".notemd")).unwrap();
        for (raw, expected) in [("' 收件 / 箱 '", "收件/箱"), ("'./a//b/'", "a/b"), ("'../x'", "inbox"),
            ("'/abs'", "inbox"), ("'C:/x'", "inbox"), ("'  '", "inbox"), ("3", "inbox")] {
            let raw = raw.replace('\'', "\"");
            fs::write(vault.path().join(".notemd/settings.json"), format!("{{\"inboxDir\":{raw}}}")).unwrap();
            assert_eq!(inbox_dir(vault.path()), expected, "{raw}");
        }
    }

    #[test]
    fn summaries_are_plain_okf_markdown() {
        let text = capture("task-T-1-0123456").render("2026-10-23T00:00:00Z").unwrap();
        let document = Document::parse(&text).unwrap();
        assert_eq!(document.doc_type(), Some("Task Summary"));
        assert_eq!(document.get("generated").unwrap().get("by").unwrap(), WRITER);
        let sources = document.get("sources").unwrap().as_sequence().unwrap();
        assert_eq!(sources[0].get("resource").unwrap(), &Value::from(format!("git:{SHA}")));
        assert_eq!(sources[1].get("resource").unwrap(), "towermd:p1/T-1");
        assert!(text.contains("[^commit]: 提交 0123456\n") && text.contains("[^task]: Tower.md 任务 T-1\n"));
        assert!(!text.contains("[[") && !text.contains("]("));
        for bad in ["", ".hidden", "x.note", "X.NOTE", "a/b", "a b"] {
            assert!(capture(bad).render("t").is_err(), "{bad:?}");
        }
    }

    #[test]
    fn decisions_summarize_only_when_accepted_and_keep_note_refs() {
        let text = format!("---\ntype: Decision\nid: D-7\nstatus: accepted\nquestion: 用哪种共识？\ndecision: 用 Raft\nsources:\n  - {{ id: n, resource: kb/raft.md, okf_id: kb/raft, block: b-00ff00 }}\n---\n正文\n");
        let document = Document::parse(&text).unwrap();
        let capture = decision_summary("p1", &document, SHA).unwrap();
        assert_eq!(capture.name, "decision-D-7");
        assert!(capture.body.contains("`((kb/raft.md#b-00ff00))`[^note-1]"));
        let rendered = Document::parse(&capture.render("t").unwrap()).unwrap();
        assert_eq!(note_refs(&rendered).len(), 1);
        assert!(rendered.body().contains("[^note-1]: kb/raft\n"));
        assert!(decision_summary("p1", &document, "abc").is_none());
        for status in ["draft", "superseded", ""] {
            let draft = Document::parse(&text.replace("status: accepted", &format!("status: '{status}'"))).unwrap();
            assert_eq!(decision_name(&draft), None);
        }
        assert_eq!(decision_name(&Document::parse(&text.replace("type: Decision", "type: Proposal")).unwrap()), None);
    }

    #[test]
    fn writeback_baselines_then_writes_once_and_never_overwrites() {
        let vault = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let ledger = state.path().join("notemd/ledger.json");
        fs::write(vault.path().join("existing.md"), "keep\n").unwrap();

        let mut first = Writeback::open(vault.path(), &ledger, project.path()).unwrap();
        assert!(first.baseline() && !first.pending("old"));
        first.mark("old");
        first.save().unwrap();
        assert_eq!(files(vault.path()), vec![vault.path().join("existing.md")]);

        let mut second = Writeback::open(vault.path(), &ledger, project.path()).unwrap();
        assert!(!second.baseline() && !second.pending("old") && second.pending("task-T-1-0123456"));
        let written = second.write(&capture("task-T-1-0123456")).unwrap().unwrap();
        assert_eq!(written, vault.path().canonicalize().unwrap().join("inbox/towermd/task-T-1-0123456.md"));
        let bytes = fs::read(&written).unwrap();
        fs::write(&written, "edited by a person\n").unwrap();
        assert_eq!(second.write(&capture("task-T-1-0123456")).unwrap(), None);
        assert_eq!(fs::read_to_string(&written).unwrap(), "edited by a person\n");
        second.save().unwrap();
        assert!(!bytes.is_empty());

        fs::remove_file(&written).unwrap();
        let third = Writeback::open(vault.path(), &ledger, project.path()).unwrap();
        assert!(!third.pending("task-T-1-0123456"), "a deleted summary is never recreated");
        assert_eq!(fs::read_to_string(vault.path().join("existing.md")).unwrap(), "keep\n");
        for file in files(vault.path()) {
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.ends_with(".md") && !name.ends_with(".note.md"), "{name}");
        }

        let other = tempfile::tempdir().unwrap();
        assert!(Writeback::open(other.path(), &ledger, project.path()).unwrap().baseline(), "a new vault starts a new baseline");
    }

    #[test]
    fn writeback_refuses_missing_vaults_and_vaults_inside_the_project() {
        let project = tempfile::tempdir().unwrap();
        let ledger = project.path().join("ledger.json");
        assert!(Writeback::open(&project.path().join("missing"), &ledger, project.path()).is_err());
        fs::write(project.path().join("file"), "").unwrap();
        assert!(Writeback::open(&project.path().join("file"), &ledger, project.path()).is_err());
        assert!(Writeback::open(project.path(), &ledger, project.path()).is_err());
        assert!(!ledger.exists());
    }
}
