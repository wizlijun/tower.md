//! Text-source frames (core spec FR-163–169, FR-174–177, FR-180, appendix
//! A.2 FR-170–173, FR-179).
//!
//! A template that says `source: file` names a parser; a frame placed from
//! it remembers one repository file. The file is the truth: what the frame
//! shows is parsed from it on every read and never stored, so it rebuilds
//! from the text alone.
//!
//! Canvas and command edits do not change the frame. They wait in the
//! frame's pending list as writes (`card.source`), and the daemon's single
//! main writer turns each into the smallest line patch of the file, then
//! records `frame.source_written`. A delegate's edit of a frame a person set
//! is a proposal first (FR-166). An edit the file no longer allows stays,
//! drawn dashed, until a person redoes or drops it (FR-167).
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    cards::{Op, Pending, Why},
    delegation_projection::Mark,
    frames::{self, Frame, Frames, SetBy, Who, GROUP},
    hub::Event,
    plantuml::{self, Parsed, Status},
    question_model::human_actor,
};

/// The daemon's single writer records each write it made.
pub const WRITTEN: &str = "frame.source_written";
/// A person points the frame at the file's new path, in the App only.
pub const RELINKED: &str = "frame.source_relinked";
pub const WRITER: &str = "process:towermd";
pub const SOURCE_FILE: &str = "file";
pub const PATH_CHARS: usize = 256;
pub const KEY_CHARS: usize = 200;
pub const NOTE_CHARS: usize = 2000;
pub const LABEL_CHARS: usize = 100;
/// FR-179: notes past this are better kept in Markdown.
pub const NOTE_LONG: usize = 300;
/// FR-179: files past this are worth splitting.
pub const FILE_LARGE: usize = 256 * 1024;
/// FR-177: hours a bound frame's file may stay unparsable before it speaks up.
pub const ACTIVE_AFTER_HOURS: i64 = 24;
const LISTED_LINES: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Why,
    Adjust,
    Question,
}

/// What a `source: file` template adds to a template.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Spec {
    pub parser: String,
    pub file_types: Vec<String>,
    /// Stereotype or color as written, to one of the template's states.
    pub status_map: BTreeMap<String, String>,
    pub callout_map: BTreeMap<String, NoteKind>,
    /// More tokens for states from the frame's rules (`Rules::status_map`);
    /// read like `status_map`, never written by the canvas.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub ruled: BTreeMap<String, String>,
}

/// The file a frame shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub path: String,
}

impl Spec {
    /// `None` for a template without `source`; problems fail closed.
    pub fn read(definition: &Value, states: &[String]) -> Result<Option<Spec>, Vec<String>> {
        let Some(map) = definition.as_object() else { return Ok(None) };
        let source = match map.get("source") {
            None | Some(Value::Null) => {
                let stray = ["parser", "file_types", "status_map", "callout_map"].into_iter().find(|key| map.contains_key(*key));
                return match stray {
                    Some(key) => Err(vec![format!("{key} 只用于 source: file 的模板")]),
                    None => Ok(None),
                };
            }
            Some(source) => source,
        };
        let mut problems = Vec::new();
        if source.as_str() != Some(SOURCE_FILE) {
            problems.push(format!("source 只能是 {SOURCE_FILE}"));
        }
        let parser = map.get("parser").and_then(Value::as_str).unwrap_or_default().to_owned();
        if parser != plantuml::PARSER {
            problems.push(format!("parser 须是 {}（不认识的解析器不能使用）", plantuml::PARSER));
        }
        let file_types: Vec<String> = match map.get("file_types").and_then(Value::as_array) {
            Some(items) if !items.is_empty() => items.iter().filter_map(Value::as_str).map(str::to_ascii_lowercase).collect(),
            _ => Vec::new(),
        };
        let valid_type = |ext: &String| ext.len() >= 2 && ext.len() <= 16 && ext.starts_with('.') && ext[1..].bytes().all(|b| b.is_ascii_alphanumeric());
        if file_types.is_empty() || !file_types.iter().all(valid_type)
            || file_types.len() != map.get("file_types").and_then(Value::as_array).map_or(0, Vec::len)
        {
            problems.push("file_types 须是 .ext 形式的非空列表".into());
        }
        let mut status_map = BTreeMap::new();
        match map.get("status_map") {
            None | Some(Value::Null) => {}
            Some(Value::Object(entries)) => {
                for (token, state) in entries {
                    match state.as_str() {
                        Some(state) if states.iter().any(|known| known == state) && token_ok(token) => {
                            status_map.insert(token.clone(), state.to_owned());
                        }
                        _ => problems.push(format!("status_map 的 {token} 须写成 <<x>> 或 #颜色，并对应模板的状态")),
                    }
                }
            }
            Some(_) => problems.push("status_map 须是映射".into()),
        }
        let mut callout_map = BTreeMap::new();
        match map.get("callout_map") {
            None | Some(Value::Null) => {}
            Some(Value::Object(entries)) => {
                for (token, kind) in entries {
                    match serde_json::from_value::<NoteKind>(kind.clone()) {
                        Ok(kind) if token.starts_with("<<") && token.ends_with(">>") && token_ok(token) => {
                            callout_map.insert(token.clone(), kind);
                        }
                        _ => problems.push(format!("callout_map 的 {token} 须是 <<x>>，对应 why、adjust 或 question")),
                    }
                }
            }
            Some(_) => problems.push("callout_map 须是映射".into()),
        }
        if problems.is_empty() { Ok(Some(Spec { parser, file_types, status_map, callout_map, ruled: BTreeMap::new() })) } else { Err(problems) }
    }

    /// The token that writes `state`.
    pub fn token(&self, state: &str) -> Option<&str> {
        self.status_map.iter().find(|(_, value)| *value == state).map(|(token, _)| token.as_str())
    }

    pub fn note_token(&self, kind: NoteKind) -> Option<&str> {
        self.callout_map.iter().find(|(_, value)| **value == kind).map(|(token, _)| token.as_str())
    }

    pub fn state_of(&self, element: &plantuml::Element) -> Option<String> {
        element.stereotypes.iter().chain(element.color.iter()).find_map(|token| self.status_map.get(token).or_else(|| self.ruled.get(token)).cloned())
    }

    /// Whether `token` means a state, by the template or the frame's rules.
    pub fn maps(&self, token: &str) -> bool {
        self.status_map.contains_key(token) || self.ruled.contains_key(token)
    }
}

pub(crate) fn token_ok(token: &str) -> bool {
    let inner = token.strip_prefix("<<").and_then(|t| t.strip_suffix(">>")).or_else(|| token.strip_prefix('#'));
    inner.is_some_and(|inner| !inner.is_empty() && inner.len() <= 40 && !inner.contains(char::is_whitespace) && !inner.contains(['<', '>']))
}

/// The frame's spec, read again from its frozen template.
pub fn spec(frame: &Frame) -> Result<Spec, String> {
    match Spec::read(&frame.template.definition, &frame.states) {
        Ok(Some(spec)) => Ok(Spec { ruled: frame.rules.status_map.clone(), ..spec }),
        Ok(None) => Err("这个容器的模板不是文本源模板".into()),
        Err(problems) => Err(problems.join("；")),
    }
}

/// A repository-relative POSIX path the template's types allow. The daemon
/// also refuses protected paths, which depend on the project's docs folder.
pub fn check_path(path: &str, spec: &Spec) -> Result<(), String> {
    let plain = !path.is_empty() && path.chars().count() <= PATH_CHARS && !path.starts_with('/')
        && !path.contains('\\') && !path.chars().any(char::is_control)
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..");
    if !plain {
        return Err(format!("源文件须是仓库内的相对路径，不含 . 或 ..，不超过 {PATH_CHARS} 字"));
    }
    let lower = path.to_ascii_lowercase();
    if lower == ".towermd" || lower.starts_with(".towermd/") || lower.starts_with(".git/") {
        return Err("源文件不能放在 .towermd/ 或 .git/ 里".into());
    }
    if !spec.file_types.iter().any(|ext| lower.ends_with(ext.as_str())) {
        return Err(format!("源文件的类型须是 {}", spec.file_types.join("、")));
    }
    Ok(())
}

/// One canvas or command edit of the file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Add {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alias: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<String>,
    },
    Title { element: String, title: String },
    State { element: String, state: String },
    Link {
        from: String,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        dashed: bool,
    },
    Note { element: String, kind: NoteKind, text: String },
    /// FR-175: a repair the person saw, bound to the file it was offered for.
    Fix { fix: String, sha256: String },
    /// A person creates the file with an empty diagram.
    Init {},
}

impl Change {
    fn label(&self) -> &'static str {
        match self {
            Change::Add { .. } => "加元素",
            Change::Title { .. } | Change::Note { .. } => "改文字",
            Change::State { .. } => "改状态",
            Change::Link { .. } => "加连线",
            Change::Fix { .. } => "修复文件",
            Change::Init {} => "新建文件",
        }
    }
    /// FR-146: the rule that lets a delegate make this edit.
    pub fn who(&self, rules: &frames::Rules) -> Who {
        match self {
            Change::Add { .. } => rules.add_card,
            Change::Link { .. } => rules.add_link,
            Change::Title { .. } | Change::Note { .. } => rules.edit_text,
            Change::State { .. } => rules.change_state,
            Change::Fix { .. } | Change::Init {} => Who::Human,
        }
    }
}

fn one_line(text: &str, max: usize, label: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().any(char::is_control) || text.chars().count() > max {
        return Err(format!("{label}须是非空单行文本，不超过 {max} 字"));
    }
    Ok(())
}

fn key(element: &str) -> Result<(), String> {
    one_line(element, KEY_CHARS, "元素标识")
}

/// What the event alone can tell; the file is checked when it is written.
pub fn check(frame: &Frame, change: &Change, person: bool) -> Result<(), String> {
    let spec = spec(frame)?;
    if !person && change.who(&frame.rules) == Who::Human {
        return Err(format!("容器规则只允许人{}", change.label()));
    }
    let state = |state: &str| -> Result<(), String> {
        if !frame.states.iter().any(|known| known == state) {
            return Err(format!("状态 {state} 不在模板的状态集合里（{}）", frame.states.join("、")));
        }
        spec.token(state).map(|_| ()).ok_or_else(|| format!("容器规则没有写明状态 {state} 用什么写法"))
    };
    match change {
        Change::Add { title, alias, parent, state: wanted } => {
            one_line(title, frame.limits.title_max as usize, "标题")?;
            if let Some(alias) = alias {
                one_line(alias, 64, "别名")?;
            }
            if let Some(parent) = parent {
                key(parent)?;
            }
            if let Some(wanted) = wanted {
                state(wanted)?;
            }
        }
        Change::Title { element, title } => {
            key(element)?;
            one_line(title, frame.limits.title_max as usize, "标题")?;
        }
        Change::State { element, state: wanted } => {
            key(element)?;
            state(wanted)?;
        }
        Change::Link { from, to, label, .. } => {
            key(from)?;
            key(to)?;
            if from == to {
                return Err("元素不能连向自己".into());
            }
            if let Some(label) = label {
                one_line(label, LABEL_CHARS, "连线标签")?;
            }
        }
        Change::Note { element, text, .. } => {
            key(element)?;
            if text.trim().is_empty() || text.chars().any(|c| c.is_control() && c != '\n') || text.chars().count() > NOTE_CHARS {
                return Err(format!("旁注须非空，不超过 {NOTE_CHARS} 字"));
            }
        }
        Change::Fix { fix, sha256 } => {
            one_line(fix, 64, "修复建议")?;
            if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
                return Err("修复建议须绑定文件内容的 sha256".into());
            }
        }
        Change::Init {} => {}
    }
    if !person && matches!(change, Change::Fix { .. } | Change::Init {}) {
        return Err(format!("只有人能{}", change.label()));
    }
    Ok(())
}

/// Routes an op on a text-source frame. `Ok(false)` leaves ordinary frames
/// and rule proposals to `cards`.
pub fn admit(frames: &mut Frames, frame_id: &str, op: &Op, at: &Mark, from: Option<&str>) -> Result<bool, String> {
    let frame = frames.get_mut(frame_id).ok_or_else(|| format!("容器 {frame_id} 不存在"))?;
    let change = match (op, &frame.source) {
        (Op::Source { .. }, None) => return Err("这个容器不是文本源容器".into()),
        (Op::Source { change }, Some(_)) => change.clone(),
        (Op::Rules { .. }, _) | (_, None) => return Ok(false),
        (_, Some(_)) => return Err("文本源容器的卡片来自文件：用 towermd frame edit 改，或直接编辑文件".into()),
    };
    let person = from.is_none();
    check(frame, &change, person)?;
    let why = match &change {
        _ if !person && frame.set_by == SetBy::Human => Why::Proposal,
        Change::State { state, .. } if !person && frame.rules.done_needs_human && frame.done_states.contains(state) => Why::Confirm,
        _ => Why::Write,
    };
    frame.pending.push(Pending {
        id: at.seq, why, target: frame_id.to_owned(), op: Op::Source { change }, from: from.map(str::to_owned), by: at.clone(), note: None,
    });
    Ok(true)
}

/// A person's decision on a waiting text-source edit: accepting turns it
/// into a write (again, for a failed one); ignoring drops it.
pub fn resolve(frame: &mut Frame, item: Pending, accept: bool) -> Result<(), String> {
    if accept {
        if let Op::Source { change } = &item.op {
            check(frame, change, true).map_err(|e| format!("修改已不适用，只能忽略：{e}"))?;
        }
        frame.pending.push(Pending { why: Why::Write, note: None, ..item });
    }
    Ok(())
}

/// `frame.source_written`: what the single writer did with one write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Written {
    pub pending: u64,
    pub path: String,
    /// The commit that wrote it; none when the file already read so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The file's content after the write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Why the file no longer allows the edit; the item stays, dashed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
}

/// The last write the frame made into its file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub sha256: String,
    pub at: Mark,
}

/// `frame.source_relinked`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relinked {
    pub path: String,
}

fn hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// The writer's receipts and a person's relinks; `Ok(false)` for others.
pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != WRITTEN && event.kind != RELINKED {
        return Ok(false);
    }
    if event.stale {
        return Ok(true);
    }
    let mut all = frames::read(state)?;
    let frame = all.get_mut(&event.obj).ok_or("容器不存在")?;
    let spec = spec(frame)?;
    if event.kind == RELINKED {
        if !human_actor(&event.actor) || event.layer != frames::LAYER || !event.guards.is_empty() {
            return Err("重新关联源文件只能由人在 App 中发出".into());
        }
        let input: Relinked = serde_json::from_value(event.data.clone()).map_err(|e| format!("重新关联正文无效：{e}"))?;
        check_path(&input.path, &spec)?;
        let source = frame.source.as_mut().ok_or("这个容器不是文本源容器")?;
        if source.path == input.path {
            return Err("源文件路径没有变化".into());
        }
        source.path = input.path;
    } else {
        if event.actor != WRITER || event.layer != frames::LAYER || event.guards.len() != 1 || event.guards[0].name != "main"
            || event.epoch != Some(event.guards[0].epoch)
        {
            return Err("写回源文件的回执须由持有主干租约的写入者记录".into());
        }
        let done: Written = serde_json::from_value(event.data.clone()).map_err(|e| format!("写回回执无效：{e}"))?;
        let path = frame.source.as_ref().map(|source| source.path.clone()).ok_or("这个容器不是文本源容器")?;
        if done.path != path {
            return Err("写回回执的路径与容器的源文件不符".into());
        }
        let index = frame.pending.iter()
            .position(|item| item.id == done.pending && item.why == Why::Write && item.note.is_none())
            .ok_or("没有排队中的这项写入")?;
        match (&done.failed, &done.sha256, &done.commit) {
            (Some(reason), None, None) if !reason.trim().is_empty() => {
                frame.pending[index].note = Some(crate::tower_api::clip(reason, 200));
            }
            (None, Some(sha), commit) if hex64(sha) && commit.as_ref().is_none_or(|c| crate::events::derived_eid(c, WRITTEN).is_ok()) => {
                frame.pending.remove(index);
                frame.source_write = Some(Record { commit: done.commit.clone(), sha256: sha.clone(), at: frames::mark(event) });
            }
            _ => return Err("写回回执须是写入（带 sha256）或失败原因之一".into()),
        }
    }
    state[GROUP] = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    Ok(true)
}

/// One element as the canvas draws it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Card {
    pub key: String,
    pub title: String,
    pub keyword: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub done: bool,
    /// Elements this one links to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<String>,
    pub line: usize,
    pub diagram: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub implicit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<String>,
    /// Gantt tasks: start, length and progress in days.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<plantuml::Time>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Callout {
    pub kind: NoteKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The first line, clipped; the whole note is in `full`.
    pub text: String,
    pub full: String,
    pub line: usize,
}

/// FR-179: passive findings, one per kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Lint {
    pub code: &'static str,
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<usize>,
    /// `unmapped_status`: the tokens a person may write into the frame's
    /// rules as states.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Shown {
    pub cards: Vec<Card>,
    pub links: Vec<plantuml::Link>,
    pub callouts: Vec<Callout>,
}

/// The parsed file in the frame's terms: states from `status_map`, note
/// kinds from `callout_map`.
pub fn shown(frame: &Frame, spec: &Spec, parsed: &Parsed) -> Shown {
    let cards = parsed.elements.iter().map(|element| {
        let state = spec.state_of(element);
        Card {
            key: element.key.clone(), title: element.name.clone(), keyword: element.keyword.clone(), parent: element.parent.clone(),
            done: state.as_ref().is_some_and(|state| frame.done_states.contains(state)), state,
            // A boundary message's other end is the diagram's edge, not a card.
            links: parsed.links.iter().filter(|link| link.from == element.key && !link.boundary).map(|link| link.to.clone()).collect(),
            body: element.body.clone(), line: element.line, diagram: element.diagram, implicit: element.implicit,
            read_only: element.read_only.clone(), time: element.time.clone(),
        }
    }).collect();
    let callouts = parsed.notes.iter().map(|note| {
        let kind = note.stereotypes.iter().find_map(|token| spec.callout_map.get(token).copied()).unwrap_or(NoteKind::Why);
        let first = note.text.lines().next().unwrap_or_default();
        Callout { kind, target: note.target.clone(), text: crate::tower_api::clip(first, crate::callouts::TEXT_CHARS), full: note.text.clone(), line: note.line }
    }).collect();
    Shown { cards, links: parsed.links.clone(), callouts }
}

fn lines(items: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut out: Vec<usize> = items.filter(|line| *line > 0).collect();
    out.sort_unstable();
    out.dedup();
    out.truncate(LISTED_LINES);
    out
}

/// FR-179: the first-party PlantUML check. Passive only, never a gate.
pub fn lint(spec: &Spec, parsed: &Parsed, size: usize) -> Vec<Lint> {
    let mut out = Vec::new();
    if let Some(problem) = parsed.problems.first() {
        out.push(Lint { code: "syntax", text: format!("文件无法解析：{}", problem.reason), lines: lines(parsed.problems.iter().map(|p| p.line)), tokens: Vec::new() });
    }
    let plain: Vec<&plantuml::Element> = parsed.elements.iter()
        .filter(|e| e.alias.is_none() && !e.implicit && e.read_only.is_none() && !matches!(e.keyword.as_str(), "mindmap" | "wbs"))
        .collect();
    if !plain.is_empty() {
        out.push(Lint { code: "no_alias", text: format!("{} 个元素没有别名；改名会被当成删除加新增", plain.len()), lines: lines(plain.iter().map(|e| e.line)), tokens: Vec::new() });
    }
    let mut unmapped: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for element in &parsed.elements {
        for token in element.stereotypes.iter().chain(element.color.iter()) {
            if !spec.maps(token) && !spec.callout_map.contains_key(token) {
                unmapped.entry(token).or_default().push(element.line);
            }
        }
    }
    if !unmapped.is_empty() {
        let tokens: Vec<&str> = unmapped.keys().copied().collect();
        out.push(Lint {
            code: "unmapped_status",
            text: format!("{} 不在 status_map 里，只当装饰；要当状态请写进容器规则", tokens.join("、")),
            lines: lines(unmapped.values().flatten().copied()),
            tokens: tokens.iter().map(|token| token.to_string()).collect(),
        });
    }
    let long: Vec<usize> = parsed.notes.iter().filter(|n| n.text.chars().count() > NOTE_LONG).map(|n| n.line).collect();
    if !long.is_empty() {
        out.push(Lint { code: "long_note", text: format!("{} 条旁注超过 {NOTE_LONG} 字；建议移到 Markdown 并链接", long.len()), lines: lines(long.into_iter()), tokens: Vec::new() });
    }
    if size > FILE_LARGE {
        out.push(Lint { code: "large_file", text: format!("文件有 {} KB；建议拆分", size / 1024), lines: Vec::new(), tokens: Vec::new() });
    }
    if !parsed.hints.is_empty() {
        out.push(Lint { code: "layout_hint", text: "画布视图不使用这些布局提示；文件里原样保留".into(), lines: lines(parsed.hints.iter().map(|h| h.line)), tokens: Vec::new() });
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Ok,
    Unparsable,
    Missing,
    TooLarge,
    NotUtf8,
}

/// FR-177: one signal per frame, passive unless bound work waits on it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Signal {
    pub level: &'static str,
    pub text: String,
}

/// What a text-source frame shows, derived on each read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct View {
    pub path: String,
    pub status: FileStatus,
    /// The commit whose file is read; none when the file is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// FR-175: the last commit that parsed, shown while the file does not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown_from: Option<String>,
    /// Canvas edits wait until the file parses again.
    pub locked: bool,
    /// Where git's rename detection found the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed_to: Option<String>,
    pub cards: Vec<Card>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<plantuml::Link>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub callouts: Vec<Callout>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagrams: Vec<plantuml::Diagram>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<plantuml::Line>,
    pub unknown_lines: usize,
    pub understood: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<plantuml::Line>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<plantuml::Fix>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lint: Vec<Lint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<Signal>,
    /// The file's sha256, which a fix binds to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// The file as one commit has it.
pub struct Read {
    pub revision: String,
    pub bytes: Vec<u8>,
    pub parsed: Parsed,
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// Builds the view from the file at the current revision (`None` when
/// missing), the last revision that parsed, and how long it has failed.
pub fn view(frame: &Frame, spec: &Spec, current: Option<Read>, last_good: Option<Read>, renamed_to: Option<String>, failing_hours: Option<i64>, bound_open: bool) -> View {
    let path = frame.source.as_ref().map(|source| source.path.clone()).unwrap_or_default();
    let mut out = View {
        path, status: FileStatus::Missing, revision: None, shown_from: None, locked: true, renamed_to, cards: Vec::new(), links: Vec::new(),
        callouts: Vec::new(), diagrams: Vec::new(), unknown: Vec::new(), unknown_lines: 0, understood: 0, problems: Vec::new(),
        fixes: Vec::new(), includes: Vec::new(), lint: Vec::new(), signal: None, sha256: None,
    };
    let fallback = |out: &mut View, last_good: Option<Read>| {
        if let Some(good) = last_good {
            let shown = shown(frame, spec, &good.parsed);
            (out.cards, out.links, out.callouts) = (shown.cards, shown.links, shown.callouts);
            out.shown_from = Some(good.revision);
            // The canvas picks its layout from the diagrams, so they follow the reading it shows.
            out.diagrams = good.parsed.diagrams;
        }
    };
    match current {
        None => fallback(&mut out, last_good),
        Some(read) => {
            out.revision = Some(read.revision.clone());
            out.sha256 = Some(sha256(&read.bytes));
            let parsed = read.parsed;
            out.status = match parsed.status {
                Status::Ok => FileStatus::Ok,
                Status::Unparsable => FileStatus::Unparsable,
                Status::TooLarge => FileStatus::TooLarge,
                Status::NotUtf8 => FileStatus::NotUtf8,
            };
            out.lint = lint(spec, &parsed, read.bytes.len());
            if parsed.status == Status::Ok {
                let shown = shown(frame, spec, &parsed);
                (out.cards, out.links, out.callouts) = (shown.cards, shown.links, shown.callouts);
                out.locked = false;
                out.diagrams = parsed.diagrams;
            } else {
                fallback(&mut out, last_good);
            }
            out.unknown = parsed.unknown;
            out.unknown_lines = parsed.unknown_lines;
            out.understood = parsed.understood;
            out.problems = parsed.problems;
            out.fixes = parsed.fixes;
            out.includes = parsed.includes;
        }
    }
    let trouble = match out.status {
        FileStatus::Ok => None,
        FileStatus::Unparsable => Some(format!("{} 当前无法解析，画布编辑已锁定", out.path)),
        FileStatus::Missing => Some(format!("源文件 {} 缺失，容器只读；可在 App 里重新关联", out.path)),
        FileStatus::TooLarge => Some(format!("{} 超过 1 MB 上限，只读", out.path)),
        FileStatus::NotUtf8 => Some(format!("{} 不是 UTF-8，只读", out.path)),
    };
    out.signal = trouble.map(|text| {
        let active = bound_open && failing_hours.is_some_and(|hours| hours >= ACTIVE_AFTER_HOURS);
        Signal { level: if active { "active" } else { "passive" }, text }
    });
    out
}

/// FR-169: what changed between two readings of the file, from the parse
/// results alone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub links_added: usize,
    pub links_removed: usize,
    pub notes_added: usize,
    pub notes_removed: usize,
    /// (element, from, to)
    pub states: Vec<(String, Option<String>, Option<String>)>,
    pub text: String,
}

pub fn summary(before: Option<&Shown>, after: &Shown) -> Summary {
    let empty = Shown { cards: Vec::new(), links: Vec::new(), callouts: Vec::new() };
    let before = before.unwrap_or(&empty);
    let old: BTreeMap<&str, &Card> = before.cards.iter().map(|card| (card.key.as_str(), card)).collect();
    let new: BTreeMap<&str, &Card> = after.cards.iter().map(|card| (card.key.as_str(), card)).collect();
    let mut out = Summary {
        added: new.keys().filter(|key| !old.contains_key(*key)).map(|key| new[key].title.clone()).collect(),
        removed: old.keys().filter(|key| !new.contains_key(*key)).map(|key| old[key].title.clone()).collect(),
        ..Summary::default()
    };
    let pairs = |shown: &Shown| -> Vec<(String, String)> { shown.links.iter().map(|l| (l.from.clone(), l.to.clone())).collect() };
    let (a, b) = (pairs(before), pairs(after));
    out.links_added = b.iter().filter(|link| !a.contains(link)).count();
    out.links_removed = a.iter().filter(|link| !b.contains(link)).count();
    let notes = |shown: &Shown| -> Vec<(Option<String>, String)> { shown.callouts.iter().map(|c| (c.target.clone(), c.full.clone())).collect() };
    let (a, b) = (notes(before), notes(after));
    out.notes_added = b.iter().filter(|note| !a.contains(note)).count();
    out.notes_removed = a.iter().filter(|note| !b.contains(note)).count();
    for (key, card) in &new {
        if let Some(was) = old.get(key) {
            if was.state != card.state {
                out.states.push((card.title.clone(), was.state.clone(), card.state.clone()));
            }
        }
    }
    let mut parts = Vec::new();
    let mut count = |n: usize, what: &str| {
        if n > 0 {
            parts.push(format!("{what} {n} 个"));
        }
    };
    count(out.added.len(), "新增元素");
    count(out.removed.len(), "移除元素");
    count(out.links_added, "新增连线");
    count(out.links_removed, "移除连线");
    count(out.notes_added, "新增旁注");
    count(out.notes_removed, "移除旁注");
    for (title, from, to) in &out.states {
        parts.push(format!("{title} 状态由{}变为{}", from.as_deref().unwrap_or("无"), to.as_deref().unwrap_or("无")));
    }
    out.text = if parts.is_empty() { "没有模型上的变化".into() } else { parts.join("；") };
    out
}

/// The edit as the patcher takes it, given the file's current parse.
pub fn edit<'a>(spec: &'a Spec, change: &'a Change) -> Result<plantuml::Edit<'a>, String> {
    Ok(match change {
        Change::Add { title, alias, parent, state } => plantuml::Edit::Add {
            title, alias: alias.as_deref(), parent: parent.as_deref(),
            token: state.as_deref().map(|state| spec.token(state).ok_or("状态没有写法")).transpose()?,
        },
        Change::Title { element, title } => plantuml::Edit::Title { key: element, title },
        Change::State { element, state } => plantuml::Edit::State {
            key: element, token: Some(spec.token(state).ok_or("状态没有写法")?), remove: &[],
        },
        Change::Link { from, to, label, dashed } => plantuml::Edit::Link { from, to, label: label.as_deref(), dashed: *dashed },
        Change::Note { element, kind, text } => plantuml::Edit::Note { key: element, token: spec.note_token(*kind), text },
        Change::Fix { fix, .. } => plantuml::Edit::Fix { id: fix },
        Change::Init {} => return Err("新建文件不是修补".into()),
    })
}

/// The file's bytes after `change`, or why the file no longer allows it.
/// `Ok(None)` for `init` on a file that already exists.
pub fn write(spec: &Spec, path: &str, current: Option<&[u8]>, change: &Change, include: &mut plantuml::Include<'_>) -> Result<Option<Vec<u8>>, String> {
    match (change, current) {
        (Change::Init {}, None) => Ok(Some(plantuml::starter("\n").into_bytes())),
        (Change::Init {}, Some(_)) => Ok(None),
        (_, None) => Err(format!("源文件 {path} 不存在")),
        (Change::Fix { sha256: bound, .. }, Some(bytes)) if sha256(bytes) != *bound => Err("文件已变化，修复建议失效".into()),
        (Change::State { .. }, Some(bytes)) => {
            // Every mapped token goes, so one state replaces another.
            let remove: Vec<String> = spec.status_map.keys().chain(spec.ruled.keys()).cloned().collect();
            let Ok(plantuml::Edit::State { key, token, .. }) = edit(spec, change) else { unreachable!("state edit") };
            plantuml::patch(path, bytes, &plantuml::Edit::State { key, token, remove: &remove }, include).map(Some)
        }
        (_, Some(bytes)) => plantuml::patch(path, bytes, &edit(spec, change)?, include).map(Some),
    }
}

#[cfg(test)]
#[path = "source_frames_tests.rs"]
mod tests;
