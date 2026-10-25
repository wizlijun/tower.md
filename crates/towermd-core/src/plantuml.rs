//! A PlantUML subset, read and patched line by line (core spec FR-164,
//! FR-165, FR-170, FR-172, FR-174–176, appendix A.2). Written in Rust inside
//! the core: it never runs PlantUML, never reads the network and never
//! expands macros. What it does not understand it says so, with the line and
//! the reason, and keeps byte for byte.
//!
//! FR-172 phase 2: a `@startuml` with participants and messages is a
//! sequence diagram. Participants are elements and messages are links in
//! file order; groups, activations, separators and delays are understood and
//! kept but not drawn. Half arrows (`-\\`, `-/`), async (`->>`), lost
//! (`->x`) and boundary messages (`[->`, `->]`) are read too; a boundary end
//! is the key `[` (left edge) or `]` (right edge).
//!
//! Work breakdowns may turn a branch with `<` or `>` after the markers; Gantt
//! charts read tasks, durations, dates, dependencies, progress and
//! milestones, in days from the chart's origin.
//!
//! Patching only ever replaces or inserts whole lines next to the element it
//! is about; every other byte of the file, line endings included, stays.
use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;

pub const PARSER: &str = "builtin:plantuml-v1";
/// FR-178: the parser reads at most this much.
pub const MAX_BYTES: usize = 1 << 20;
const INCLUDE_DEPTH: usize = 4;
const INCLUDES_MAX: usize = 32;
/// Unknown lines listed one by one; the rest are only counted.
const UNKNOWN_LISTED: usize = 200;
const SHOWN_CHARS: usize = 120;

const ELEMENTS: [&str; 33] = [
    "component", "class", "interface", "abstract", "enum", "annotation", "actor", "usecase", "rectangle", "object",
    "node", "folder", "frame", "package", "namespace", "database", "cloud", "queue", "artifact", "card", "file",
    "storage", "agent", "boundary", "control", "entity", "collections", "hexagon", "person", "label", "stack",
    "participant", "map",
];
/// Element keywords whose `{ }` hold members rather than nested elements.
const BODIES: [&str; 7] = ["class", "interface", "abstract", "enum", "annotation", "entity", "map"];
const NEUTRAL: [&str; 16] = [
    "skinparam", "hide", "show", "title", "header", "footer", "caption", "scale", "!theme", "!pragma",
    "allowmixing", "allow_mixing", "set", "mainframe", "left side", "right side",
];
const DIRECTIONS: [&str; 2] = ["left to right direction", "top to bottom direction"];
/// Sequence groups closed by `end`; `else` splits the innermost.
const SEQUENCE_GROUPS: [&str; 9] = ["alt", "opt", "loop", "par", "par2", "break", "critical", "group", "box"];
/// Sequence lines that change no participant or message.
const SEQUENCE_MARKS: [&str; 7] = ["activate", "deactivate", "destroy", "return", "autonumber", "newpage", "delay"];
/// Gantt lines that change no task: scales, language, the today mark.
const GANTT_NEUTRAL: [&str; 6] = ["printscale", "projectscale", "ganttscale", "zoom", "language", "today"];
const GANTT_STATE: &str = "甘特图任务的进度写在文件里（is NN% completed），画布不改状态";
const C4_ELEMENTS: [&str; 22] = [
    "Person", "Person_Ext", "System", "System_Ext", "SystemDb", "SystemDb_Ext", "SystemQueue", "SystemQueue_Ext",
    "Container", "ContainerDb", "ContainerQueue", "Container_Ext", "ContainerDb_Ext", "Component", "ComponentDb",
    "ComponentQueue", "Component_Ext", "Boundary", "System_Boundary", "Container_Boundary", "Enterprise_Boundary",
    "Deployment_Node",
];
const C4_LINKS: [&str; 14] = [
    "Rel", "Rel_U", "Rel_D", "Rel_L", "Rel_R", "Rel_Up", "Rel_Down", "Rel_Left", "Rel_Right", "Rel_Back",
    "Rel_Neighbor", "BiRel", "Lay_U", "Lay_D",
];

pub const FROM_MACRO: &str = "来自宏，只读";
pub const CONFLICT: &str = "标识冲突，只读";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    /// FR-175: syntax the subset proves wrong; editing is locked.
    Unparsable,
    TooLarge,
    /// Only UTF-8 is read and written (GBK and others stay read-only).
    NotUtf8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagram {
    /// `uml`, `mindmap`, `wbs` or what follows `@start`.
    pub kind: String,
    pub line: usize,
    pub supported: bool,
    /// FR-172 phase 2: a `uml` diagram with participants and messages; the
    /// canvas reads participants across and messages in file order.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sequence: bool,
    /// Gantt: the day task offsets count from, `YYYY-MM-DD`; the project
    /// start, else the earliest date written. None when no date is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// FR-172 phase 2: a Gantt task's place in time, in days from the chart's
/// origin; the end is `start + days`, exclusive.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Time {
    /// None when it cannot be worked out, as in a dependency cycle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<i64>,
    /// `is 40% completed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u8>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub milestone: bool,
    /// Its start is written (a date or another task): a new dependency
    /// would contradict it.
    #[serde(skip)]
    anchored: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Quoted,
    Bracket,
    Paren,
    Colon,
    Bare,
    /// A mind map or work breakdown line: the text after the markers.
    Tree,
}

/// Where an element is written, for patching.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Decl {
    /// 0-based line index.
    line: usize,
    /// Byte span of the name as written, quotes or brackets included.
    name: (usize, usize),
    style: Style,
    /// Byte offset after the name and alias, where stereotypes go.
    head_end: usize,
    /// Closing `}` line of a container.
    close: Option<usize>,
    indent: String,
    /// Mind maps: the marker character and depth.
    marker: Option<(char, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Element {
    pub key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub keyword: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// As written, `<<done>>`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stereotypes: Vec<String>,
    /// As written, `#lightgreen`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<String>,
    /// 1-based; 0 when the element is implicit or comes from another file.
    pub line: usize,
    pub diagram: usize,
    /// Named by a link without being declared.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub implicit: bool,
    /// FR-176: why canvas edits of this element are disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<String>,
    /// Gantt tasks and milestones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<Time>,
    #[serde(skip)]
    decl: Option<Decl>,
    #[serde(skip)]
    parent_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Link {
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dashed: bool,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<String>,
    /// Sequence diagrams: how the message ends, `async` (`->>`), `half`
    /// (`-\\`, `-/`), `lost` (`->x`) or `circle` (`->o`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<&'static str>,
    /// A sequence message from or to the diagram's edge: that end's key is
    /// `[` (left edge) or `]` (right edge).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub boundary: bool,
    #[serde(skip)]
    ends: (usize, usize),
    /// Two or more dashes, `-->`: a reply in a sequence diagram.
    #[serde(skip)]
    long: bool,
    /// A boundary message: whether the edge is the `from` end, and its key.
    #[serde(skip)]
    edge: Option<(bool, &'static str)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Note {
    /// The element it sits beside; none for a floating note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// A note on the link written on this line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_link: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stereotypes: Vec<String>,
    pub text: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<String>,
    #[serde(skip)]
    target_index: Option<usize>,
    #[serde(skip)]
    alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Line {
    pub line: usize,
    pub text: String,
    pub reason: String,
}

/// FR-175: a repair the person may apply after seeing it; never automatic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fix {
    pub id: String,
    pub title: String,
    /// What would change, as `+ line` and `- line`.
    pub diff: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Parsed {
    pub status: Status,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagrams: Vec<Diagram>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// FR-174: lines not understood, with the reason; kept as written.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<Line>,
    pub unknown_lines: usize,
    /// What makes the file unparsable.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Line>,
    /// Layout hints the canvas does not use.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<Line>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<Fix>,
    /// Lines that are not blank or comments.
    pub meaningful: usize,
    /// FR-174: percent of meaningful lines understood.
    pub understood: u32,
    #[serde(skip)]
    eol: String,
    #[serde(skip)]
    ends: Vec<(usize, usize)>,
}

impl Parsed {
    fn empty(status: Status) -> Self {
        Parsed {
            status, diagrams: Vec::new(), elements: Vec::new(), links: Vec::new(), notes: Vec::new(), unknown: Vec::new(),
            unknown_lines: 0, problems: Vec::new(), hints: Vec::new(), includes: Vec::new(), fixes: Vec::new(),
            meaningful: 0, understood: 0, eol: "\n".into(), ends: Vec::new(),
        }
    }
    pub fn element(&self, key: &str) -> Option<&Element> {
        self.elements.iter().find(|element| element.key == key)
    }
}

/// Reads one repository file for a local `!include`; `None` when it is
/// missing. The path is already normalized and inside the repository.
pub type Include<'a> = dyn FnMut(&str) -> Option<Vec<u8>> + 'a;

fn clip(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= SHOWN_CHARS {
        return text.to_owned();
    }
    text.chars().take(SHOWN_CHARS).collect::<String>() + "…"
}

/// Splits keeping each line's ending: (content, ending).
fn split(text: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(at) => {
                let (line, tail) = rest.split_at(at + 1);
                let content = line.strip_suffix("\r\n").or_else(|| line.strip_suffix('\n')).unwrap_or(line);
                out.push((content, &line[content.len()..]));
                rest = tail;
            }
            None => {
                out.push((rest, ""));
                rest = "";
            }
        }
    }
    out
}

/// `dir/../x.puml` resolved against the including file; `None` when it
/// would leave the repository or is not a plain relative path.
pub fn resolve(base: &str, target: &str) -> Option<String> {
    if target.is_empty() || target.starts_with('/') || target.contains('\\') || target.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn ident(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn ident_char(c: char, next: Option<char>) -> bool {
    if c == '.' {
        return next.is_some_and(char::is_alphanumeric);
    }
    if c == '/' || c == '\\' {
        // `B //- A`, a half arrow, starts here.
        return !next.is_some_and(|next| "-./\\".contains(next));
    }
    !c.is_whitespace() && !"-<>*|#[](){}:\"=~^+,;'!@$".contains(c)
}

/// A name as written: `"x"`, `[x]`, `(x)`, `:x:` or a bare word. Returns the
/// name, its style and the bytes it took.
fn name_at(text: &str) -> Option<(String, Style, usize)> {
    let mut chars = text.chars();
    let first = chars.next()?;
    let closing = match first {
        '"' => Some(('"', Style::Quoted)),
        '[' => Some((']', Style::Bracket)),
        '(' => Some((')', Style::Paren)),
        ':' => Some((':', Style::Colon)),
        _ => None,
    };
    if let Some((close, style)) = closing {
        let end = text[1..].find(close)? + 1;
        let name = &text[1..end];
        if name.trim().is_empty() || (style != Style::Quoted && name.contains(['\n', '"'])) {
            return None;
        }
        return Some((name.to_owned(), style, end + 1));
    }
    let mut end = 0;
    let indices: Vec<(usize, char)> = text.char_indices().collect();
    for (i, (at, c)) in indices.iter().enumerate() {
        if !ident_char(*c, indices.get(i + 1).map(|(_, c)| *c)) {
            break;
        }
        end = at + c.len_utf8();
    }
    (end > 0).then(|| (text[..end].to_owned(), Style::Bare, end))
}

fn skip_ws(text: &str, at: usize) -> usize {
    at + (text[at..].len() - text[at..].trim_start().len())
}

/// `<<x>>` stereotypes and a `#color`, from `at`; returns them and where
/// they stop.
fn decorations(text: &str, mut at: usize) -> (Vec<String>, Option<String>, usize) {
    let (mut stereotypes, mut color) = (Vec::new(), None);
    loop {
        let start = skip_ws(text, at);
        let rest = &text[start..];
        if rest.starts_with("<<") {
            let Some(end) = rest.find(">>") else { break };
            stereotypes.push(rest[..end + 2].to_owned());
            at = start + end + 2;
        } else if rest.starts_with('#') && color.is_none() {
            let end = rest[1..].find(|c: char| c.is_whitespace() || c == '{').map_or(rest.len(), |end| end + 1);
            if end <= 1 {
                break;
            }
            color = Some(rest[..end].to_owned());
            at = start + end;
        } else {
            break;
        }
    }
    (stereotypes, color, at)
}

struct Declared {
    keyword: String,
    name: String,
    alias: Option<String>,
    stereotypes: Vec<String>,
    color: Option<String>,
    opens: bool,
    decl: Decl,
}

/// `keyword name [as alias] [<<x>>] [#c] [{]`, or the `[x]`, `(x)`, `:x:`
/// shorthands; `None` when anything is left over.
fn declaration(line: &str, index: usize) -> Option<Declared> {
    let start = skip_ws(line, 0);
    let indent = line[..start].to_owned();
    let trimmed = &line[start..];
    let (keyword, mut at) = match ELEMENTS.iter().find(|word| {
        trimmed.strip_prefix(**word).is_some_and(|rest| rest.starts_with(char::is_whitespace))
    }) {
        Some(word) => (word.to_string(), skip_ws(line, start + word.len())),
        None => {
            let keyword = match trimmed.chars().next()? {
                '[' => "component",
                '(' => "usecase",
                ':' => "actor",
                _ => return None,
            };
            (keyword.to_owned(), start)
        }
    };
    let (mut name, mut style, taken) = name_at(&line[at..])?;
    let mut span = (at, at + taken);
    at += taken;
    let mut alias = None;
    let after = skip_ws(line, at);
    if let Some(rest) = line[after..].strip_prefix("as") {
        if rest.starts_with(char::is_whitespace) {
            let from = skip_ws(line, after + 2);
            let (other, other_style, taken) = name_at(&line[from..])?;
            if style == Style::Bare && other_style != Style::Bare {
                // `A1 as "Title"`: the bare word is the alias.
                alias = Some(std::mem::replace(&mut name, other));
                style = other_style;
                span = (from, from + taken);
            } else if other_style == Style::Bare {
                alias = Some(other);
            } else {
                return None;
            }
            at = from + taken;
        }
    }
    let head_end = at;
    let (stereotypes, color, mut at) = decorations(line, at);
    at = skip_ws(line, at);
    let opens = line[at..].starts_with('{');
    if opens {
        at = skip_ws(line, at + 1);
    }
    if at != line.len() {
        return None;
    }
    let decl = Decl { line: index, name: span, style, head_end, close: None, indent, marker: None };
    Some(Declared { keyword, name, alias, stereotypes, color, opens, decl })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ref {
    Name(String, Style),
}

struct Arrow {
    len: usize,
    reversed: bool,
    dashed: bool,
    hint: bool,
    long: bool,
    head: Option<&'static str>,
}

fn arrow(text: &str) -> Option<Arrow> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let body = |b: u8| b == b'-' || b == b'.';
    // `\`, `/`, `\\` or `//` at `at`: a half arrow head, by its length.
    let half = |at: usize| match bytes.get(at) {
        Some(b'/' | b'\\') if bytes.get(at + 1) == bytes.get(at) => 2,
        Some(b'/' | b'\\') => 1,
        _ => 0,
    };
    let mut reversed = false;
    let mut head = None;
    if text.starts_with("<|") {
        i = 2;
        reversed = true;
    } else if text.starts_with("<<") && bytes.get(2).is_some_and(|b| body(*b)) {
        i = 2;
        reversed = true;
        head = Some("async");
    } else if half(0) > 0 && bytes.get(half(0)).is_some_and(|b| body(*b)) {
        i = half(0);
        reversed = true;
        head = Some("half");
    } else if bytes.first().is_some_and(|b| b"xo".contains(b)) && bytes.get(1) == Some(&b'<') && bytes.get(2).is_some_and(|b| body(*b)) {
        // `x<-`, `o<-`: the reversed lost or circle head.
        i = 2;
        reversed = true;
        head = Some(if bytes[0] == b'x' { "lost" } else { "circle" });
    } else if text.starts_with('<') {
        i = 1;
        reversed = true;
    } else if bytes.first().is_some_and(|b| b"*ox#}+^".contains(b)) && bytes.get(1).is_some_and(|b| body(*b)) {
        i = 1;
    }
    let (mut count, mut dashed, mut hint) = (0, false, false);
    while i < bytes.len() {
        let b = bytes[i];
        if body(b) {
            dashed |= b == b'.';
            count += 1;
            i += 1;
        } else if b == b'[' && count > 0 {
            let end = text[i..].find(']')? + i;
            hint |= text[i..end].contains("hidden") || text[i..end].contains("norank");
            i = end + 1;
        } else if b.is_ascii_lowercase() && count > 0 {
            let end = text[i..].find(|c: char| !c.is_ascii_lowercase()).map_or(text.len(), |end| end + i);
            if !["up", "down", "left", "right", "u", "d", "l", "r", "le", "ri", "do"].contains(&&text[i..end])
                || !bytes.get(end).is_some_and(|b| body(*b))
            {
                break;
            }
            hint = true;
            i = end;
        } else {
            break;
        }
    }
    if count == 0 {
        return None;
    }
    let tail_end = |n: usize| bytes.get(i + n).is_none_or(|b| b.is_ascii_whitespace() || b"[(\":".contains(b));
    if text[i..].starts_with("|>") {
        i += 2;
        reversed = false;
    } else if text[i..].starts_with(">>") {
        i += 2;
        reversed = false;
        head = Some("async");
    } else if text[i..].starts_with('>') {
        i += 1;
        reversed = false;
        // A sequence message lost (`->x`) or ending in a circle (`->o`),
        // also at the diagram's edge (`->x]`).
        if bytes.get(i).is_some_and(|b| b"xo".contains(b))
            && bytes.get(i + 1).is_none_or(|b| b.is_ascii_whitespace() || b"[(\":]?".contains(b))
        {
            head = Some(if bytes[i] == b'x' { "lost" } else { "circle" });
            i += 1;
        }
    } else if half(i) > 0 {
        i += half(i);
        reversed = false;
        head = Some("half");
    } else if bytes.get(i).is_some_and(|b| b"*ox#{+^".contains(b)) && tail_end(1) {
        i += 1;
    }
    Some(Arrow { len: i, reversed, dashed, hint, long: count >= 2, head })
}

struct Linked {
    from: Ref,
    to: Ref,
    label: Option<String>,
    dashed: bool,
    hint: bool,
    long: bool,
    head: Option<&'static str>,
}

fn cardinality(line: &str, at: usize) -> usize {
    let at = skip_ws(line, at);
    if line[at..].starts_with('"') {
        if let Some(end) = line[at + 1..].find('"') {
            return skip_ws(line, at + end + 2);
        }
    }
    at
}

fn link(line: &str) -> Option<Linked> {
    let at = skip_ws(line, 0);
    let (from, from_style, taken) = name_at(&line[at..])?;
    let at = cardinality(line, at + taken);
    let shape = arrow(&line[at..])?;
    let at = cardinality(line, at + shape.len);
    let (to, to_style, taken) = name_at(&line[at..])?;
    let label = message_end(line, at + taken)?;
    let (from, to) = (Ref::Name(from, from_style), Ref::Name(to, to_style));
    let (from, to) = if shape.reversed { (to, from) } else { (from, to) };
    Some(Linked { from, to, label, dashed: shape.dashed, hint: shape.hint, long: shape.long, head: shape.head })
}

/// What follows a link's target from `at`: sequence activation marks
/// (`++`, `--`, `**`, `!!`), decorations, then an optional `: label`. None
/// when anything else is left.
fn message_end(line: &str, at: usize) -> Option<Option<String>> {
    let mut at = skip_ws(line, at);
    while ["++", "--", "**", "!!"].iter().any(|mark| line[at..].starts_with(mark)) {
        at = skip_ws(line, at + 2);
    }
    let (_, _, after) = decorations(line, at);
    let at = skip_ws(line, after);
    match &line[at..] {
        "" => Some(None),
        rest => Some(Some(rest.strip_prefix(':')?.trim().to_owned()).filter(|label| !label.is_empty())),
    }
}

/// A sequence message from or to the diagram's edge: `[-> A`, `?-> A`,
/// `[<- A`, `A ->]`, `A ->?`, `A <-]`.
struct Bounded {
    inner: Ref,
    /// The left edge (`[` or `?` before the arrow), else the right.
    left: bool,
    /// From the edge to the participant.
    incoming: bool,
    label: Option<String>,
    dashed: bool,
    long: bool,
    head: Option<&'static str>,
}

fn boundary(line: &str) -> Option<Bounded> {
    let text = &line[skip_ws(line, 0)..];
    if let Some(rest) = text.strip_prefix(['[', '?']) {
        // `[o->` and `[x->`: a circle or cross at the edge.
        let rest = match rest.as_bytes() {
            [b'o' | b'x', b'-' | b'.' | b'<', ..] => &rest[1..],
            _ => rest,
        };
        let shape = arrow(rest)?;
        let at = skip_ws(rest, shape.len);
        let (name, style, taken) = name_at(&rest[at..])?;
        let label = message_end(rest, at + taken)?;
        return Some(Bounded {
            inner: Ref::Name(name, style), left: true, incoming: !shape.reversed, label, dashed: shape.dashed, long: shape.long,
            head: shape.head,
        });
    }
    let (name, style, taken) = name_at(text)?;
    let at = skip_ws(text, taken);
    let shape = arrow(&text[at..])?;
    let rest = text[at + shape.len..].strip_prefix([']', '?'])?;
    let label = message_end(rest, 0)?;
    Some(Bounded { inner: Ref::Name(name, style), left: false, incoming: shape.reversed, label, dashed: shape.dashed, long: shape.long, head: shape.head })
}

/// `[name]` at the start of `text`: the name and the bytes it took.
fn bracket(text: &str) -> Option<(&str, usize)> {
    let rest = text.strip_prefix('[')?;
    let end = rest.find(']')?;
    let name = &rest[..end];
    (!name.trim().is_empty() && !name.contains('[')).then_some((name, end + 2))
}

/// `text` without a leading `word` (any case) and the space after it.
fn after<'t>(text: &'t str, word: &str) -> Option<&'t str> {
    let head = text.get(..word.len())?;
    let rest = &text[word.len()..];
    (head.eq_ignore_ascii_case(word) && (rest.is_empty() || rest.starts_with(char::is_whitespace))).then(|| rest.trim_start())
}

fn date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()
}

/// Where a Gantt clause puts a task, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Anchor<T> {
    Date(NaiveDate),
    Start(T),
    End(T),
}

/// `[at|on] YYYY-MM-DD` or `[at] [task]'s start|end`.
fn anchor(text: &str) -> Option<Anchor<&str>> {
    let text = after(text, "at").or_else(|| after(text, "on")).unwrap_or(text).trim();
    if let Some((name, taken)) = bracket(text) {
        return match text[taken..].trim().to_ascii_lowercase().as_str() {
            "'s end" => Some(Anchor::End(name)),
            "'s start" => Some(Anchor::Start(name)),
            _ => None,
        };
    }
    date(text).map(Anchor::Date)
}

enum Clause<'t> {
    Days(i64),
    Starts(Anchor<&'t str>),
    Ends(Anchor<&'t str>),
    Happens(Anchor<&'t str>),
    Completed(u8),
    /// Understood and not drawn: `is colored in …`.
    Kept,
}

/// One part of a Gantt task line, between `and`s.
fn clause(text: &str) -> Option<Clause<'_>> {
    let text = text.trim();
    if let Some(rest) = after(text, "lasts").or_else(|| after(text, "requires")) {
        let mut words = rest.split_whitespace();
        let count: i64 = words.next()?.parse().ok().filter(|n| (0..=100_000).contains(n))?;
        let unit = words.next()?.to_ascii_lowercase();
        if words.next().is_some() {
            return None;
        }
        return match unit.as_str() {
            "day" | "days" => Some(Clause::Days(count)),
            "week" | "weeks" => Some(Clause::Days(count * 7)),
            _ => None,
        };
    }
    if let Some(rest) = after(text, "starts") {
        return anchor(rest).map(Clause::Starts);
    }
    if let Some(rest) = after(text, "ends") {
        return anchor(rest).map(Clause::Ends);
    }
    if let Some(rest) = after(text, "happens") {
        return anchor(rest).map(Clause::Happens);
    }
    let rest = after(text, "is")?;
    if let Some(colour) = after(rest, "colored").and_then(|rest| after(rest, "in")) {
        return (!colour.is_empty()).then_some(Clause::Kept);
    }
    let (percent, word) = rest.split_once('%')?;
    let percent: u8 = percent.trim().parse().ok().filter(|percent| *percent <= 100)?;
    ["complete", "completed"].contains(&word.trim().to_ascii_lowercase().as_str()).then_some(Clause::Completed(percent))
}

/// Splits at ` and ` outside brackets.
fn clauses(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let (mut out, mut from, mut depth, mut at) = (Vec::new(), 0, 0i32, 0);
    while at < bytes.len() {
        match bytes[at] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        if depth == 0 && bytes[at..].len() >= 5 && bytes[at..at + 5].eq_ignore_ascii_case(b" and ") {
            out.push(&text[from..at]);
            from = at + 5;
            at += 5;
            continue;
        }
        at += 1;
    }
    out.push(&text[from..]);
    out
}

/// What a Gantt chart says about one task, resolved once the chart is read.
#[derive(Clone, Debug, Default)]
struct Facts {
    days: Option<i64>,
    start: Option<Anchor<usize>>,
    end: Option<Anchor<usize>>,
    completed: Option<u8>,
    milestone: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Frame {
    Element(usize),
    Body(usize),
    Neutral,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Block {
    /// Lines up to the end word belong to a note.
    Note(usize, &'static [&'static str]),
    /// Style or layout lines, kept and not modelled.
    Neutral(&'static [&'static str]),
    /// Braced style blocks, counted.
    Braces(usize),
    /// Preprocessor definitions, not expanded.
    Macro(&'static str),
    /// A mind map node's text up to `;`.
    Text(usize),
    Comment,
}

struct State<'a, 'b> {
    out: Parsed,
    path: String,
    include: &'a mut Include<'b>,
    /// How deep the current `!include` is.
    includes: usize,
    /// Gantt: what each task's lines say, by element index.
    facts: BTreeMap<usize, Facts>,
    /// Gantt: `Project starts`, by diagram.
    starts: BTreeMap<usize, NaiveDate>,
    /// Gantt tasks by (is alias, diagram, name), for lookups in long charts.
    tasks: BTreeMap<(bool, usize, String), usize>,
}

struct Pass {
    from: Option<String>,
    diagram: Option<usize>,
    stack: Vec<Frame>,
    block: Option<Block>,
    /// Mind map nodes by depth.
    tree: Vec<usize>,
    /// Open sequence groups, innermost last, with the line that opened each.
    groups: Vec<(String, usize)>,
    /// Gantt: the last task a line was about, for `then`.
    last: Option<usize>,
}

impl State<'_, '_> {
    fn unknown(&mut self, pass: &Pass, line: usize, text: &str, reason: &str) {
        if pass.from.is_some() {
            return;
        }
        self.out.unknown_lines += 1;
        if self.out.unknown.len() < UNKNOWN_LISTED {
            self.out.unknown.push(Line { line: line + 1, text: clip(text), reason: reason.into() });
        }
    }
    fn problem(&mut self, pass: &Pass, line: usize, text: &str, reason: &str) {
        if pass.from.is_none() {
            self.out.problems.push(Line { line: line + 1, text: clip(text), reason: reason.into() });
        }
    }
    fn hint(&mut self, pass: &Pass, line: usize, text: &str) {
        if pass.from.is_none() {
            self.out.hints.push(Line { line: line + 1, text: clip(text), reason: "画布视图不使用布局提示，文件里原样保留".into() });
        }
    }

    fn container(pass: &Pass) -> Option<usize> {
        pass.stack.iter().rev().find_map(|frame| match frame {
            Frame::Element(index) => Some(*index),
            _ => None,
        })
    }

    fn push(&mut self, pass: &Pass, mut element: Element) -> usize {
        element.parent_index = Self::container(pass);
        if let Some(from) = &pass.from {
            element.read_only = Some(format!("来自 {from}，只读"));
            element.line = 0;
            element.decl = None;
        }
        element.diagram = pass.diagram.unwrap_or_default();
        self.out.elements.push(element);
        self.out.elements.len() - 1
    }

    /// The element a link or note names, declaring it implicitly if needed.
    fn find(&mut self, pass: &Pass, name: &Ref, line: usize) -> usize {
        let Ref::Name(name, style) = name;
        let found = self.out.elements.iter().position(|element| element.alias.as_deref() == Some(name.as_str()))
            .or_else(|| {
                let same: Vec<usize> = self.out.elements.iter().enumerate()
                    .filter(|(_, element)| element.name == *name).map(|(index, _)| index).collect();
                let inside = Self::container(pass);
                same.iter().find(|index| self.out.elements[**index].parent_index == inside).or(same.first()).copied()
            });
        if let Some(index) = found {
            return index;
        }
        let keyword = match style {
            Style::Bracket => "component",
            Style::Paren => "usecase",
            Style::Colon => "actor",
            _ => "implicit",
        };
        self.push(pass, Element {
            key: String::new(), name: name.clone(), alias: None, keyword: keyword.into(), parent: None,
            stereotypes: Vec::new(), color: None, body: Vec::new(), line: if pass.from.is_some() { 0 } else { line + 1 },
            diagram: 0, implicit: true, read_only: None, time: None, decl: None, parent_index: None,
        })
    }

    fn note(&mut self, pass: &mut Pass, index: usize, trimmed: &str) -> bool {
        let rest = ["note", "hnote", "rnote"].iter().find_map(|word| {
            trimmed.strip_prefix(word).filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        });
        let Some(rest) = rest else { return false };
        let rest = rest.trim();
        let (body, text) = match rest.find(':') {
            Some(at) if !rest[..at].contains('"') => (rest[..at].trim(), Some(rest[at + 1..].trim().to_owned())),
            _ => (rest, None),
        };
        let mut words = body;
        let (mut target, mut on_link, mut alias, mut floating_text) = (None, None, None, None);
        if let Some(after) = words.strip_prefix("on link") {
            on_link = self.out.links.len().checked_sub(1);
            if on_link.is_none() {
                return false;
            }
            words = after.trim();
        } else if words.starts_with('"') {
            let Some((name, _, taken)) = name_at(words) else { return false };
            floating_text = Some(name);
            words = words[taken..].trim();
        } else if let Some(position) = ["left", "right", "top", "bottom", "over"].iter().find(|word| words.starts_with(**word)) {
            words = words[position.len()..].trim();
            if let Some(after) = words.strip_prefix("of ").or_else(|| (*position == "over").then_some(words)) {
                let after = after.trim();
                let Some((name, style, taken)) = name_at(after) else { return false };
                target = Some(self.find(pass, &Ref::Name(name, style), index));
                words = after[taken..].trim();
                if *position == "over" {
                    // `note over A, B`: beside the first.
                    words = words.split_once(',').map_or(words, |(_, more)| {
                        let more = more.trim();
                        name_at(more).map_or(more, |(_, _, taken)| more[taken..].trim())
                    });
                }
            } else {
                target = self.out.elements.len().checked_sub(1);
                if target.is_none() {
                    return false;
                }
            }
        }
        if let Some(after) = words.strip_prefix("as ") {
            let after = after.trim();
            let Some((name, Style::Bare, taken)) = name_at(after) else { return false };
            alias = Some(name);
            words = after[taken..].trim();
        }
        let (stereotypes, _, at) = decorations(words, 0);
        if !words[at..].trim().is_empty() {
            return false;
        }
        let multi = text.is_none() && floating_text.is_none();
        let read_only = pass.from.as_ref().map(|from| format!("来自 {from}，只读"));
        self.out.notes.push(Note {
            target: None, on_link: None, stereotypes, text: text.or(floating_text).unwrap_or_default(),
            line: if pass.from.is_some() { 0 } else { index + 1 }, read_only, target_index: target, alias,
        });
        if let Some(link) = on_link {
            self.out.notes.last_mut().expect("pushed").on_link = Some(self.out.links[link].line);
        }
        if multi {
            pass.block = Some(Block::Note(self.out.notes.len() - 1, &["end note", "endnote", "end hnote", "endhnote", "end rnote", "endrnote"]));
        }
        true
    }

    fn macro_call(&mut self, pass: &mut Pass, index: usize, trimmed: &str) -> bool {
        let Some(open) = trimmed.find('(') else { return false };
        let name = &trimmed[..open];
        if !ident(name) {
            return false;
        }
        let rest = trimmed[open + 1..].trim_end();
        let (inner, opens) = match rest.strip_suffix('{').map(str::trim_end) {
            Some(before) => (before, true),
            None => (rest, false),
        };
        let Some(inner) = inner.strip_suffix(')') else { return false };
        let args: Vec<String> = split_args(inner);
        if C4_ELEMENTS.contains(&name) && !args.is_empty() && ident(&args[0]) {
            let title = args.get(1).cloned().unwrap_or_else(|| args[0].clone());
            let element = Element {
                key: String::new(), name: title, alias: Some(args[0].clone()), keyword: name.to_owned(), parent: None,
                stereotypes: Vec::new(), color: None, body: Vec::new(), line: index + 1, diagram: 0, implicit: false,
                read_only: Some(FROM_MACRO.into()), time: None, decl: None, parent_index: None,
            };
            let at = self.push(pass, element);
            if opens {
                pass.stack.push(Frame::Element(at));
            }
            return true;
        }
        if C4_LINKS.contains(&name) && args.len() >= 2 && !opens {
            let from = self.find(pass, &Ref::Name(args[0].clone(), Style::Bare), index);
            let to = self.find(pass, &Ref::Name(args[1].clone(), Style::Bare), index);
            let line = if pass.from.is_some() { 0 } else { index + 1 };
            self.out.links.push(Link {
                from: String::new(), to: String::new(), label: args.get(2).cloned(), dashed: false, line,
                read_only: Some(FROM_MACRO.into()), head: None, boundary: false, ends: (from, to), long: false, edge: None,
            });
            return true;
        }
        false
    }

    fn uml(&mut self, pass: &mut Pass, index: usize, line: &str, trimmed: &str) {
        let lower = trimmed.to_ascii_lowercase();
        if trimmed == "}" {
            match pass.stack.pop() {
                Some(Frame::Element(at) | Frame::Body(at)) => {
                    if let Some(decl) = self.out.elements[at].decl.as_mut() {
                        decl.close = Some(index);
                    }
                }
                Some(Frame::Neutral) => {}
                None => self.problem(pass, index, line, "多出来的 }"),
            }
            return;
        }
        if let Some(Frame::Body(at)) = pass.stack.last() {
            let at = *at;
            self.out.elements[at].body.push(clip(trimmed));
            return;
        }
        if DIRECTIONS.contains(&lower.as_str()) {
            self.hint(pass, index, line);
            return;
        }
        if lower == "together {" || lower == "together{" {
            pass.stack.push(Frame::Neutral);
            return;
        }
        if self.layout(pass, index, line, trimmed, &lower) {
            return;
        }
        if self.note(pass, index, trimmed) {
            return;
        }
        if self.sequence(pass, index, line, trimmed, &lower) {
            return;
        }
        if self.edge_message(pass, index, line) {
            return;
        }
        if let Some(found) = declaration(line, index) {
            let Declared { keyword, name, alias, stereotypes, color, opens, decl } = found;
            let body = opens && BODIES.contains(&keyword.as_str());
            let element = Element {
                key: String::new(), name, alias, keyword, parent: None, stereotypes, color, body: Vec::new(),
                line: index + 1, diagram: 0, implicit: false, read_only: None, time: None, decl: Some(decl), parent_index: None,
            };
            if element.keyword == "participant" {
                self.mark_sequence(pass);
            }
            let at = self.push(pass, element);
            if opens {
                pass.stack.push(if body { Frame::Body(at) } else { Frame::Element(at) });
            }
            return;
        }
        if let Some(found) = link(line) {
            if found.hint {
                self.hint(pass, index, line);
            }
            let from = self.find(pass, &found.from, index);
            let to = self.find(pass, &found.to, index);
            let line_no = if pass.from.is_some() { 0 } else { index + 1 };
            let read_only = pass.from.as_ref().map(|from| format!("来自 {from}，只读"));
            self.out.links.push(Link {
                from: String::new(), to: String::new(), label: found.label, dashed: found.dashed, line: line_no, read_only,
                head: found.head, boundary: false, ends: (from, to), long: found.long, edge: None,
            });
            return;
        }
        if self.macro_call(pass, index, trimmed) {
            return;
        }
        if trimmed.starts_with(':') && trimmed.ends_with(';') || ["start", "stop", "endif", "fork", "end fork"].contains(&lower.as_str()) {
            self.unknown(pass, index, line, "活动图写法第 3 阶段才支持");
            return;
        }
        if trimmed.contains('(') && trimmed.trim_end().ends_with(')') {
            self.unknown(pass, index, line, "宏调用第 1 阶段不展开");
            return;
        }
        self.unknown(pass, index, line, "不认识的写法");
    }

    /// Style, title, legend and preprocessor lines, shared by every kind
    /// read line by line; true when the line was one.
    fn layout(&mut self, pass: &mut Pass, index: usize, line: &str, trimmed: &str, lower: &str) -> bool {
        if lower.starts_with("<style>") {
            if !lower.contains("</style>") {
                pass.block = Some(Block::Neutral(&["</style>"]));
            }
            return true;
        }
        for (word, ends) in [
            ("legend", &["endlegend", "end legend"][..]),
            ("title", &["endtitle", "end title"][..]),
            ("header", &["endheader", "end header"][..]),
            ("footer", &["endfooter", "end footer"][..]),
        ] {
            let alone = lower == word || (word == "legend" && lower.strip_prefix("legend ").is_some_and(|rest| {
                rest.split_whitespace().all(|w| ["left", "right", "top", "bottom", "center"].contains(&w))
            }));
            if alone {
                pass.block = Some(Block::Neutral(ends));
                return true;
            }
        }
        if (lower.starts_with("skinparam") || lower.starts_with("sprite")) && lower.ends_with('{') {
            pass.block = Some(Block::Braces(1));
            return true;
        }
        if NEUTRAL.iter().any(|word| lower == *word || lower.starts_with(&format!("{word} ")) || lower.starts_with(&format!("{word}<"))) {
            return true;
        }
        if lower.starts_with("sprite ") {
            return true;
        }
        if let Some(rest) = lower.strip_prefix('!') {
            self.directive(pass, index, line, trimmed, rest);
            return true;
        }
        false
    }

    /// A boundary message (FR-172 phase 2): a link whose edge end has the
    /// key `[` or `]`.
    fn edge_message(&mut self, pass: &Pass, index: usize, line: &str) -> bool {
        let Some(found) = boundary(line) else { return false };
        let at = self.find(pass, &found.inner, index);
        let line_no = if pass.from.is_some() { 0 } else { index + 1 };
        let read_only = pass.from.as_ref().map(|from| format!("来自 {from}，只读"));
        self.out.links.push(Link {
            from: String::new(), to: String::new(), label: found.label, dashed: found.dashed, line: line_no, read_only,
            head: found.head, boundary: true, ends: (at, at), long: found.long,
            edge: Some((found.incoming, if found.left { "[" } else { "]" })),
        });
        self.mark_sequence(pass);
        true
    }

    /// The task a Gantt line names, by alias then by name, declaring it
    /// implicitly when it is not yet known.
    fn task_find(&self, pass: &Pass, name: &str) -> Option<usize> {
        let diagram = pass.diagram.unwrap_or_default();
        let key = (diagram, name.to_owned());
        self.tasks.get(&(true, key.0, key.1.clone())).or_else(|| self.tasks.get(&(false, key.0, key.1))).copied()
    }

    fn task_at(&mut self, pass: &Pass, name: &str, index: usize) -> usize {
        if let Some(found) = self.task_find(pass, name) {
            return found;
        }
        let at = self.push(pass, Element {
            key: String::new(), name: name.to_owned(), alias: None, keyword: "task".into(), parent: None,
            stereotypes: Vec::new(), color: None, body: Vec::new(), line: if pass.from.is_some() { 0 } else { index + 1 },
            diagram: 0, implicit: true, read_only: None, time: None, decl: None, parent_index: None,
        });
        self.tasks.insert((false, pass.diagram.unwrap_or_default(), name.to_owned()), at);
        at
    }

    fn depends(&mut self, pass: &Pass, index: usize, from: usize, to: usize) {
        let line = if pass.from.is_some() { 0 } else { index + 1 };
        let read_only = pass.from.as_ref().map(|from| format!("来自 {from}，只读"));
        self.out.links.push(Link {
            from: String::new(), to: String::new(), label: None, dashed: false, line, read_only, head: None, boundary: false,
            ends: (from, to), long: false, edge: None,
        });
    }

    /// FR-172 phase 2: one line of a Gantt chart. Tasks are elements,
    /// dependencies are links; closed days and other calendar lines are
    /// kept and listed, since the canvas counts calendar days.
    fn gantt(&mut self, pass: &mut Pass, index: usize, line: &str, trimmed: &str) {
        let lower = trimmed.to_ascii_lowercase();
        if self.layout(pass, index, line, trimmed, &lower) {
            return;
        }
        if GANTT_NEUTRAL.iter().any(|word| after(&lower, word).is_some()) || (lower.starts_with("--") && lower.ends_with("--") && lower.len() >= 4) {
            return;
        }
        if let Some(rest) = after(trimmed, "project").and_then(|rest| after(rest, "starts")) {
            let rest = after(rest, "the").or_else(|| after(rest, "at")).or_else(|| after(rest, "on")).unwrap_or(rest);
            match (date(rest), pass.diagram) {
                (Some(day), Some(diagram)) => {
                    self.starts.insert(diagram, day);
                }
                _ => self.unknown(pass, index, line, "甘特图日期只认 YYYY-MM-DD"),
            }
            return;
        }
        let (then, statement) = match after(trimmed, "then") {
            Some(rest) => (true, rest),
            None => (false, trimmed),
        };
        if !statement.starts_with('[') {
            let calendar = [" closed", " open", " off "].iter().any(|word| lower.contains(word)) || lower.ends_with(" off");
            let reason = if calendar { "画布按日历天排，不扣除休息日" } else { "不认识的甘特图写法" };
            return self.unknown(pass, index, line, reason);
        }
        if !self.task(pass, index, line, statement, then) {
            self.unknown(pass, index, line, "不认识的甘特图写法");
        }
    }

    /// `[task] [as [alias]] [on {…}] clause [and clause]…`, or `[a] -> [b]`.
    /// False, keeping nothing, when any part is not understood.
    fn task(&mut self, pass: &mut Pass, index: usize, line: &str, statement: &str, then: bool) -> bool {
        let Some((name, taken)) = bracket(statement) else { return false };
        let start = statement.as_ptr() as usize - line.as_ptr() as usize;
        let mut rest = statement[taken..].trim_start();
        let mut alias = None;
        if let Some(named) = after(rest, "as") {
            let Some((other, used)) = bracket(named) else { return false };
            alias = Some(other);
            rest = named[used..].trim_start();
        }
        while let Some(on) = after(rest, "on").filter(|on| on.starts_with('{')) {
            let mut on = on;
            while on.starts_with('{') {
                let Some(end) = on.find('}') else { return false };
                on = on[end + 1..].trim_start();
            }
            rest = on;
        }
        let mut chain = Vec::new();
        let mut parts = Vec::new();
        if rest.starts_with("->") {
            while let Some(next) = rest.strip_prefix("->") {
                let Some((other, used)) = bracket(next.trim_start()) else { return false };
                chain.push(other);
                rest = next.trim_start()[used..].trim_start();
            }
            if !rest.is_empty() || then {
                return false;
            }
        } else {
            for part in clauses(rest) {
                let Some(found) = clause(part) else { return false };
                parts.push(found);
            }
        }
        let previous = match (then, pass.last) {
            (true, None) => return false,
            (true, Some(previous)) => Some(previous),
            (false, _) => None,
        };
        let known = alias.and_then(|alias| self.task_find(pass, alias)).or_else(|| self.task_find(pass, name));
        let subject = known.unwrap_or_else(|| self.task_at(pass, name, index));
        if let (None, Some(alias)) = (&self.out.elements[subject].alias, alias) {
            let element = &mut self.out.elements[subject];
            element.alias = Some(alias.to_owned());
            element.name = name.to_owned();
            let diagram = pass.diagram.unwrap_or_default();
            self.tasks.insert((true, diagram, alias.to_owned()), subject);
            self.tasks.entry((false, diagram, name.to_owned())).or_insert(subject);
        }
        if self.out.elements[subject].decl.is_none() && pass.from.is_none() {
            let element = &mut self.out.elements[subject];
            element.implicit = false;
            element.line = index + 1;
            element.decl = Some(Decl {
                line: index, name: (start, start + taken), style: Style::Bracket, head_end: start + taken, close: None,
                indent: line[..skip_ws(line, 0)].to_owned(), marker: None,
            });
        } else if pass.from.is_some() {
            self.out.elements[subject].implicit = false;
        }
        let place = |state: &mut Self, at: Anchor<&str>| -> Anchor<usize> {
            match at {
                Anchor::Date(day) => Anchor::Date(day),
                Anchor::Start(other) | Anchor::End(other) => {
                    let other_at = state.task_at(pass, other, index);
                    state.depends(pass, index, other_at, subject);
                    if matches!(at, Anchor::Start(_)) { Anchor::Start(other_at) } else { Anchor::End(other_at) }
                }
            }
        };
        let mut facts = self.facts.get(&subject).cloned().unwrap_or_default();
        if let Some(previous) = previous {
            self.depends(pass, index, previous, subject);
            facts.start = Some(Anchor::End(previous));
        }
        for part in parts {
            match part {
                Clause::Days(days) => facts.days = Some(days),
                Clause::Starts(at) => facts.start = Some(place(self, at)),
                Clause::Ends(at) => facts.end = Some(place(self, at)),
                Clause::Happens(at) => {
                    facts.start = Some(place(self, at));
                    facts.milestone = true;
                }
                Clause::Completed(percent) => facts.completed = Some(percent),
                Clause::Kept => {}
            }
        }
        self.facts.insert(subject, facts);
        let mut before = subject;
        for other in chain {
            let next = self.task_at(pass, other, index);
            self.depends(pass, index, before, next);
            self.facts.entry(next).or_default().start = Some(Anchor::End(before));
            before = next;
        }
        pass.last = Some(subject);
        true
    }

    fn mark_sequence(&mut self, pass: &Pass) {
        if let Some(diagram) = pass.diagram {
            self.out.diagrams[diagram].sequence = true;
        }
    }

    /// FR-172 phase 2: sequence groups, activations, separators and delays.
    /// They are understood and kept; none of them is drawn.
    fn sequence(&mut self, pass: &mut Pass, index: usize, line: &str, trimmed: &str, lower: &str) -> bool {
        let word = lower.split_whitespace().next().unwrap_or_default();
        let alone_or_text = |word: &str| lower == word || lower.strip_prefix(word).is_some_and(|rest| rest.starts_with(char::is_whitespace));
        if lower == "end" || lower == "end box" || lower == "endbox" {
            let closes_box = lower != "end";
            match pass.groups.last() {
                Some((open, _)) if (open == "box") == closes_box => {
                    pass.groups.pop();
                }
                Some((open, at)) => {
                    let reason = format!("{trimmed} 与第 {} 行的 {open} 不配对", at + 1);
                    self.problem(pass, index, line, &reason);
                }
                None => self.problem(pass, index, line, &format!("{trimmed} 前没有对应的块")),
            }
            self.mark_sequence(pass);
            return true;
        }
        if SEQUENCE_GROUPS.contains(&word) && alone_or_text(word) {
            pass.groups.push((word.to_owned(), index));
            self.mark_sequence(pass);
            return true;
        }
        if alone_or_text("else") {
            if !pass.groups.last().is_some_and(|(open, _)| open != "box") {
                self.problem(pass, index, line, "else 前没有对应的 alt 块");
            }
            self.mark_sequence(pass);
            return true;
        }
        if alone_or_text("ref") {
            if !trimmed.contains(':') {
                pass.block = Some(Block::Neutral(&["end ref", "endref"]));
            }
            self.mark_sequence(pass);
            return true;
        }
        if alone_or_text("create") {
            // `create participant X`: the declaration after the word, at the
            // same byte offsets so the canvas can still edit it.
            let start = skip_ws(line, 0) + "create".len();
            let padded = format!("{}{}", " ".repeat(start), &line[start..]);
            if let Some(found) = declaration(&padded, index) {
                let Declared { keyword, name, alias, stereotypes, color, opens: false, decl } = found else { return false };
                let element = Element {
                    key: String::new(), name, alias, keyword, parent: None, stereotypes, color, body: Vec::new(),
                    line: index + 1, diagram: 0, implicit: false, read_only: None, time: None, decl: Some(Decl { indent: line[..skip_ws(line, 0)].to_owned(), ..decl }), parent_index: None,
                };
                self.push(pass, element);
            }
            self.mark_sequence(pass);
            return true;
        }
        let separator = (lower.starts_with("==") && lower.ends_with("==") && lower.len() >= 4)
            || (lower.starts_with("...") && lower.ends_with("...")) || (lower.starts_with("||") && lower.ends_with("||"));
        if separator || SEQUENCE_MARKS.iter().any(|word| alone_or_text(word)) {
            self.mark_sequence(pass);
            return true;
        }
        false
    }

    fn directive(&mut self, pass: &mut Pass, index: usize, line: &str, trimmed: &str, lower: &str) {
        let word = lower.split_whitespace().next().unwrap_or_default();
        match word {
            "include" | "include_once" | "include_many" | "includesub" => {
                let target = trimmed.split_once(char::is_whitespace).map(|(_, rest)| rest.trim()).unwrap_or_default();
                let target = target.split('!').next().unwrap_or_default().trim();
                if target.starts_with('<') {
                    self.unknown(pass, index, line, "标准库引入第 1 阶段不展开");
                } else if target.contains("://") {
                    self.unknown(pass, index, line, "远程引用：解析器从不访问网络");
                } else {
                    self.follow(pass, index, line, target);
                }
            }
            "includeurl" | "import" => self.unknown(pass, index, line, "远程引用：解析器从不访问网络"),
            "theme" | "pragma" => {}
            "procedure" | "unquoted" | "function" | "definelong" => {
                let end = match word {
                    "definelong" => "!enddefinelong",
                    "function" => "!endfunction",
                    _ => "!endprocedure",
                };
                let end = if lower.starts_with("unquoted function") { "!endfunction" } else { end };
                self.unknown(pass, index, line, "预处理器定义第 1 阶段不展开");
                pass.block = Some(Block::Macro(end));
            }
            _ => self.unknown(pass, index, line, "预处理器第 1 阶段不展开"),
        }
    }

    fn follow(&mut self, pass: &mut Pass, index: usize, line: &str, target: &str) {
        let resolved = pass.from.as_deref().map_or_else(|| resolve(&self.path, target), |from| resolve(from, target));
        let Some(path) = resolved else {
            return self.unknown(pass, index, line, "引入路径不在仓库里");
        };
        if self.includes >= INCLUDE_DEPTH || self.out.includes.len() >= INCLUDES_MAX {
            return self.unknown(pass, index, line, "引入太多或嵌套太深");
        }
        if path == self.path || self.out.includes.contains(&path) {
            return;
        }
        let Some(bytes) = (self.include)(&path) else {
            return self.unknown(pass, index, line, "引入的文件不存在");
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return self.unknown(pass, index, line, "引入的文件不是 UTF-8");
        };
        if text.len() > MAX_BYTES {
            return self.unknown(pass, index, line, "引入的文件超过大小上限");
        }
        self.out.includes.push(path.clone());
        self.includes += 1;
        let mut inner = Pass { from: Some(path), diagram: pass.diagram, stack: pass.stack.clone(), block: None, tree: Vec::new(), groups: Vec::new(), last: None };
        for (at, (content, _)) in split(&text).into_iter().enumerate() {
            let trimmed = content.trim();
            if trimmed.is_empty() || trimmed.starts_with('\'') || trimmed.starts_with("@start") || trimmed.starts_with("@end") {
                continue;
            }
            self.body(&mut inner, at, content, trimmed);
        }
        self.includes -= 1;
    }

    fn tree(&mut self, pass: &mut Pass, index: usize, line: &str, trimmed: &str) {
        let lower = trimmed.to_ascii_lowercase();
        if NEUTRAL.iter().any(|word| lower == *word || lower.starts_with(&format!("{word} "))) {
            return;
        }
        let marker = trimmed.chars().next().unwrap_or(' ');
        if !"*+-".contains(marker) {
            return self.unknown(pass, index, line, "不认识的思维导图写法");
        }
        let depth = trimmed.chars().take_while(|c| *c == marker).count();
        let start = skip_ws(line, 0) + depth;
        let mut at = start;
        // A work breakdown turns a branch left or right: `**<`, `**>`.
        if pass.diagram.is_some_and(|d| self.out.diagrams[d].kind == "wbs") && line[at..].starts_with(['<', '>']) {
            at += 1;
        }
        let mut color = None;
        if line[at..].starts_with("[#") {
            let Some(end) = line[at..].find(']') else { return self.unknown(pass, index, line, "不认识的思维导图写法") };
            color = Some(line[at + 1..at + end].to_owned());
            at += end + 1;
        }
        if line[at..].starts_with('_') {
            at += 1;
        }
        let multi = line[at..].trim_start().starts_with(':') && !line.trim_end().ends_with(';');
        at = skip_ws(line, at);
        let full = line[at..].trim_end();
        let (text, stereotypes) = match full.find("<<") {
            Some(cut) if full.ends_with(">>") => {
                let (stereotypes, _, end) = decorations(full, cut);
                if end == full.len() { (full[..cut].trim_end(), stereotypes) } else { (full, Vec::new()) }
            }
            _ => (full, Vec::new()),
        };
        let name = text.trim_start_matches(':').trim_end_matches(';').trim().to_owned();
        if name.is_empty() && !multi {
            return self.unknown(pass, index, line, "思维导图节点没有文字");
        }
        if pass.tree.len() < depth - 1 {
            return self.problem(pass, index, line, "思维导图节点跳过了上一层");
        }
        pass.tree.truncate(depth - 1);
        let parent = pass.tree.last().copied();
        let decl = Decl {
            line: index, name: (at, at + text.len()), style: Style::Tree, head_end: line.trim_end().len(),
            close: None, indent: line[..skip_ws(line, 0)].to_owned(), marker: Some((marker, depth)),
        };
        let element = Element {
            key: String::new(), name, alias: None, keyword: pass.diagram.map(|d| self.out.diagrams[d].kind.clone()).unwrap_or_default(),
            parent: None, stereotypes, color, body: Vec::new(), line: index + 1, diagram: 0, implicit: false,
            read_only: multi.then(|| "多行节点，只读".to_owned()), time: None, decl: Some(decl), parent_index: None,
        };
        let at = self.push(pass, element);
        self.out.elements[at].parent_index = parent;
        pass.tree.push(at);
        if multi {
            pass.block = Some(Block::Text(at));
        }
    }

    /// One line inside a diagram, or inside an included file.
    fn body(&mut self, pass: &mut Pass, index: usize, line: &str, trimmed: &str) {
        let lower = trimmed.to_ascii_lowercase();
        if let Some(block) = pass.block.clone() {
            match block {
                Block::Note(note, ends) => {
                    if ends.contains(&lower.as_str()) {
                        pass.block = None;
                    } else {
                        let text = &mut self.out.notes[note].text;
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(trimmed);
                    }
                }
                Block::Neutral(ends) => {
                    if ends.iter().any(|end| lower.starts_with(end)) {
                        pass.block = None;
                    }
                }
                Block::Braces(depth) => {
                    let depth = (depth + trimmed.matches('{').count()).saturating_sub(trimmed.matches('}').count());
                    pass.block = (depth > 0).then_some(Block::Braces(depth));
                }
                Block::Macro(end) => {
                    self.unknown(pass, index, line, "预处理器定义第 1 阶段不展开");
                    if lower.starts_with(end) {
                        pass.block = None;
                    }
                }
                Block::Text(element) => {
                    self.out.elements[element].body.push(clip(trimmed.trim_end_matches(';')));
                    if trimmed.ends_with(';') {
                        pass.block = None;
                    }
                }
                Block::Comment => {}
            }
            return;
        }
        let kind = pass.diagram.map(|d| self.out.diagrams[d].kind.as_str()).unwrap_or("uml");
        match kind {
            "mindmap" | "wbs" => self.tree(pass, index, line, trimmed),
            "gantt" => self.gantt(pass, index, line, trimmed),
            _ => self.uml(pass, index, line, trimmed),
        }
    }
}

fn split_args(inner: &str) -> Vec<String> {
    let (mut out, mut current, mut quoted) = (Vec::new(), String::new(), false);
    for c in inner.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut current).trim().to_owned()),
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() || !out.is_empty() {
        out.push(current.trim().to_owned());
    }
    out.into_iter().map(|arg| arg.trim_matches('"').to_owned()).collect()
}

fn end_word(kind: &str) -> String {
    format!("@end{kind}")
}

/// Reads `bytes`, the content of `path`. Never fails: what is not
/// understood is listed, and what is wrong makes the result unparsable.
pub fn parse(path: &str, bytes: &[u8], include: &mut Include<'_>) -> Parsed {
    if bytes.len() > MAX_BYTES {
        return Parsed::empty(Status::TooLarge);
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(bytes);
    let Ok(text) = std::str::from_utf8(bytes) else { return Parsed::empty(Status::NotUtf8) };
    let lines = split(text);
    let mut out = Parsed::empty(Status::Ok);
    out.eol = if text.contains("\r\n") { "\r\n".into() } else { "\n".into() };
    let mut state = State { out, path: path.to_owned(), include, includes: 0, facts: BTreeMap::new(), starts: BTreeMap::new(), tasks: BTreeMap::new() };
    let mut pass = Pass { from: None, diagram: None, stack: Vec::new(), block: None, tree: Vec::new(), groups: Vec::new(), last: None };
    for (index, (line, _)) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if pass.block == Some(Block::Comment) {
            if trimmed.contains("'/") {
                pass.block = None;
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('\'') {
            continue;
        }
        if trimmed.starts_with("/'") {
            if !trimmed[2..].contains("'/") {
                pass.block = Some(Block::Comment);
            }
            continue;
        }
        state.out.meaningful += 1;
        if ["<<<<<<<", "=======", ">>>>>>>"].iter().any(|marker| trimmed.starts_with(marker))
            && (trimmed.len() == 7 || trimmed.as_bytes()[7] == b' ')
        {
            state.problem(&pass, index, line, "文件里有合并冲突标记");
            continue;
        }
        if let Some(kind) = trimmed.strip_prefix("@start") {
            let kind = kind.split(|c: char| c.is_whitespace() || c == '(').next().unwrap_or_default().to_ascii_lowercase();
            if pass.diagram.is_some() {
                state.problem(&pass, index, line, "上一张图还没有 @end 就开始了新图");
                continue;
            }
            let supported = ["uml", "mindmap", "wbs", "gantt"].contains(&kind.as_str());
            state.out.diagrams.push(Diagram { kind: kind.clone(), line: index + 1, supported, sequence: false, origin: None });
            state.out.ends.push((index, usize::MAX));
            pass.diagram = Some(state.out.diagrams.len() - 1);
            pass.stack.clear();
            pass.tree.clear();
            pass.groups.clear();
            pass.last = None;
            if !supported {
                state.unknown(&pass, index, line, &format!("不支持的图种 @start{kind}，只能在原图视图中查看"));
            }
            continue;
        }
        if trimmed.starts_with("@end") {
            let Some(diagram) = pass.diagram.take() else {
                state.problem(&pass, index, line, "@end 前没有对应的 @start");
                continue;
            };
            state.out.ends[diagram].1 = index;
            if !state.out.diagrams[diagram].supported {
                state.unknown(&pass, index, line, "不支持的图种");
                continue;
            }
            if let Some(block) = pass.block.take() {
                let reason = match block {
                    Block::Note(..) => "多行 note 没有 end note",
                    _ => "多行块没有结束",
                };
                state.problem(&pass, index, line, reason);
            }
            let open = pass.stack.len();
            if open > 0 {
                state.problem(&pass, index, line, &format!("有 {open} 个 {{ 没有闭合"));
                let indent = line[..skip_ws(line, 0)].to_owned();
                let mut diff: Vec<String> = (0..open).map(|_| format!("+ {indent}}}")).collect();
                diff.push(format!("  {trimmed}"));
                state.out.fixes.push(Fix { id: format!("close-{}", index + 1), title: format!("在第 {} 行前补 {open} 个 }}", index + 1), diff });
            }
            if let Some((word, _)) = pass.groups.first() {
                let reason = format!("时序图有 {} 个块没有 end（最外层是第 {} 行的 {word}）", pass.groups.len(), pass.groups[0].1 + 1);
                state.problem(&pass, index, line, &reason);
            }
            pass.stack.clear();
            pass.groups.clear();
            continue;
        }
        match pass.diagram {
            None => state.unknown(&pass, index, line, "在 @startuml 与 @enduml 之外"),
            Some(diagram) if !state.out.diagrams[diagram].supported => {
                state.unknown(&pass, index, line, "不支持的图种");
            }
            Some(_) => state.body(&mut pass, index, line, trimmed),
        }
    }
    if let Some(diagram) = pass.diagram {
        let kind = state.out.diagrams[diagram].kind.clone();
        state.problem(&pass, lines.len().saturating_sub(1), lines.last().map_or("", |line| line.0), &format!("缺少 {}", end_word(&kind)));
        state.out.fixes.push(Fix { id: "end".into(), title: format!("在文件末尾补 {}", end_word(&kind)), diff: vec![format!("+ {}", end_word(&kind))] });
    }
    if state.out.meaningful > 0 && state.out.diagrams.is_empty() {
        state.problem(&pass, 0, lines.first().map_or("", |line| line.0), "文件里没有 @startuml");
    }
    schedule(&mut state.out, &state.facts, &state.starts);
    let mut out = state.out;
    finish(&mut out);
    out
}

/// FR-172 phase 2: each Gantt task's start and length, in days from the
/// chart's origin. Dependencies are followed with an explicit stack, so a
/// long chain cannot overflow; a task in a cycle gets no start.
fn schedule(out: &mut Parsed, facts: &BTreeMap<usize, Facts>, starts: &BTreeMap<usize, NaiveDate>) {
    #[derive(Clone, Copy)]
    enum Mark {
        Fresh,
        Busy,
        Done(Option<i64>, Option<i64>),
    }
    let none = Facts::default();
    let fact = |index: usize| facts.get(&index).unwrap_or(&none);
    let anchored = |at: &Option<Anchor<usize>>| match at {
        Some(Anchor::Start(other) | Anchor::End(other)) => Some(*other),
        _ => None,
    };
    for diagram in 0..out.diagrams.len() {
        if out.diagrams[diagram].kind != "gantt" {
            continue;
        }
        let tasks: Vec<usize> = (0..out.elements.len()).filter(|index| out.elements[*index].diagram == diagram).collect();
        let written = tasks.iter().flat_map(|index| [&fact(*index).start, &fact(*index).end]).filter_map(|at| match at {
            Some(Anchor::Date(day)) => Some(*day),
            _ => None,
        });
        let origin = starts.get(&diagram).copied().or_else(|| written.min());
        out.diagrams[diagram].origin = origin.map(|day| day.format("%Y-%m-%d").to_string());
        let offset = |day: NaiveDate| origin.map(|origin| (day - origin).num_days());
        let mut marks = vec![Mark::Fresh; out.elements.len()];
        for root in &tasks {
            let mut stack = vec![*root];
            while let Some(&index) = stack.last() {
                if let Mark::Done(..) = marks[index] {
                    stack.pop();
                    continue;
                }
                marks[index] = Mark::Busy;
                let facts = fact(index);
                let waiting = [anchored(&facts.start), anchored(&facts.end)].into_iter().flatten()
                    .find(|other| matches!(marks[*other], Mark::Fresh));
                if let Some(other) = waiting {
                    stack.push(other);
                    continue;
                }
                // A busy dependency is on the stack: a cycle, so unknown.
                let point = |at: &Anchor<usize>| match at {
                    Anchor::Date(day) => offset(*day),
                    Anchor::Start(other) => if let Mark::Done(start, _) = marks[*other] { start } else { None },
                    Anchor::End(other) => if let Mark::Done(_, end) = marks[*other] { end } else { None },
                };
                // `ends 2026-01-10` includes that day.
                let end = facts.end.as_ref().map(|at| point(at).map(|end| end + i64::from(matches!(at, Anchor::Date(_)))));
                let start = match (&facts.start, end, facts.days) {
                    (Some(at), _, _) => point(at),
                    (None, Some(Some(end)), Some(days)) => Some(end - days),
                    (None, Some(None), _) => None,
                    _ => Some(0),
                };
                let end = match (start, facts.milestone, facts.days) {
                    (Some(start), true, _) => Some(start),
                    (Some(start), false, Some(days)) => Some(start + days),
                    _ => end.flatten(),
                };
                marks[index] = Mark::Done(start, end);
                stack.pop();
            }
        }
        for index in tasks {
            let facts = fact(index);
            let Mark::Done(start, end) = marks[index] else { continue };
            let days = match (start, end) {
                (Some(start), Some(end)) => Some((end - start).max(0)),
                _ => facts.days,
            };
            let element = &mut out.elements[index];
            if facts.milestone {
                element.keyword = "milestone".into();
            }
            element.time = Some(Time { start, days, completed: facts.completed, milestone: facts.milestone, anchored: facts.start.is_some() });
        }
    }
}

/// Keys and parents, alias conflicts, note targets and the ratio.
fn finish(out: &mut Parsed) {
    let mut aliases: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, element) in out.elements.iter().enumerate() {
        if let Some(alias) = &element.alias {
            aliases.entry(alias.clone()).or_default().push(index);
        }
    }
    for indices in aliases.values().filter(|indices| indices.len() > 1) {
        for index in indices {
            out.elements[*index].read_only = Some(CONFLICT.into());
        }
    }
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for element in out.elements.iter().filter(|element| element.alias.is_none()) {
        *names.entry(element.name.clone()).or_default() += 1;
    }
    let mut taken: BTreeMap<String, usize> = BTreeMap::new();
    for index in 0..out.elements.len() {
        let element = &out.elements[index];
        let mut key = match &element.alias {
            Some(alias) => alias.clone(),
            None if names[&element.name] > 1 => {
                let parent = element.parent_index.map(|parent| out.elements[parent].key.clone());
                parent.map_or_else(|| element.name.clone(), |parent| format!("{parent}/{}", element.name))
            }
            None => element.name.clone(),
        };
        let seen = taken.entry(key.clone()).or_default();
        *seen += 1;
        if *seen > 1 {
            key = format!("{key}~{seen}");
            if out.elements[index].read_only.is_none() {
                out.elements[index].read_only = Some(CONFLICT.into());
            }
        }
        out.elements[index].key = key;
    }
    for index in 0..out.elements.len() {
        out.elements[index].parent = out.elements[index].parent_index.map(|parent| out.elements[parent].key.clone());
    }
    let floating: BTreeMap<String, usize> = out.notes.iter().enumerate()
        .filter_map(|(index, note)| note.alias.clone().map(|alias| (alias, index))).collect();
    for link in out.links.iter_mut() {
        // FR-172: in a sequence diagram `-->` is the dashed reply.
        let diagram = out.elements[link.ends.0].diagram;
        let sequence = out.diagrams.get(diagram).is_some_and(|diagram| diagram.sequence);
        link.dashed |= link.long && sequence;
        if !sequence {
            link.head = None;
        }
    }
    let mut drop = Vec::new();
    for (index, link) in out.links.iter_mut().enumerate() {
        link.from = out.elements[link.ends.0].key.clone();
        link.to = out.elements[link.ends.1].key.clone();
        if let Some((from, edge)) = link.edge {
            *(if from { &mut link.from } else { &mut link.to }) = edge.to_owned();
            continue;
        }
        // `N1 .. A1` ties a floating note to A1; it is not a link.
        let (from, to) = (&out.elements[link.ends.0], &out.elements[link.ends.1]);
        for (end, other) in [(from, link.ends.1), (to, link.ends.0)] {
            if end.implicit && end.alias.is_none() {
                if let Some(note) = floating.get(&end.name) {
                    out.notes[*note].target_index.get_or_insert(other);
                    drop.push(index);
                }
            }
        }
    }
    for note in &mut out.notes {
        note.target = note.target_index.and_then(|index| out.elements.get(index)).map(|element| element.key.clone());
    }
    let note_names: Vec<String> = floating.keys().cloned().collect();
    if !drop.is_empty() {
        let mut index = 0;
        out.links.retain(|_| {
            index += 1;
            !drop.contains(&(index - 1))
        });
        out.elements.retain(|element| !(element.implicit && element.alias.is_none() && note_names.contains(&element.name)));
    }
    // Indices into `elements` are stale past here: targets are keys now.
    let lines = out.unknown_lines.min(out.meaningful);
    out.understood = if out.meaningful == 0 { 100 } else { ((out.meaningful - lines) * 100 / out.meaningful) as u32 };
    if !out.problems.is_empty() {
        out.status = Status::Unparsable;
    }
}

/// A canvas or command edit, made as the smallest line change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit<'a> {
    /// `token` is a status stereotype or color from the template's map.
    Add { title: &'a str, alias: Option<&'a str>, parent: Option<&'a str>, token: Option<&'a str> },
    Title { key: &'a str, title: &'a str },
    /// Removes every token in `remove`, then writes `token`.
    State { key: &'a str, token: Option<&'a str>, remove: &'a [String] },
    Link { from: &'a str, to: &'a str, label: Option<&'a str>, dashed: bool },
    Note { key: &'a str, token: Option<&'a str>, text: &'a str },
    Fix { id: &'a str },
}

/// The text a new file starts with.
pub fn starter(eol: &str) -> String {
    format!("@startuml{eol}@enduml{eol}")
}

fn editable(element: &Element) -> Result<&Decl, String> {
    if let Some(reason) = &element.read_only {
        return Err(format!("元素 {} {reason}，不能在画布上改", element.key));
    }
    element.decl.as_ref().ok_or_else(|| format!("元素 {} 是隐式元素，没有声明行；先在文件里声明它", element.key))
}

/// How a link or note names an element in a new line.
fn reference(element: &Element) -> Result<String, String> {
    if let Some(alias) = &element.alias {
        if ident(alias) {
            return Ok(alias.clone());
        }
    }
    if element.read_only.as_deref() == Some(CONFLICT) {
        return Err(format!("元素 {} {CONFLICT}", element.key));
    }
    if ident(&element.name) {
        return Ok(element.name.clone());
    }
    if element.keyword == "component" && !element.name.contains(']') {
        return Ok(format!("[{}]", element.name));
    }
    Err(format!("元素 {} 没有别名；先在文件里给它写 as <别名>", element.key))
}

fn quoted(text: &str) -> Result<String, String> {
    if text.trim().is_empty() || text.contains('"') || text.chars().any(char::is_control) {
        return Err("文字须非空、单行，且不含双引号".into());
    }
    Ok(format!("\"{text}\""))
}

fn element<'p>(parsed: &'p Parsed, key: &str) -> Result<&'p Element, String> {
    parsed.element(key).ok_or_else(|| format!("文件里没有元素 {key}"))
}

fn uml_end(parsed: &Parsed, diagram: usize) -> Result<usize, String> {
    match (parsed.diagrams.get(diagram), parsed.ends.get(diagram)) {
        (Some(found), Some((_, end))) if found.kind == "uml" && *end != usize::MAX => Ok(*end),
        (Some(found), _) if found.kind != "uml" => Err(format!("@start{} 图不能加连线或旁注", found.kind)),
        _ => Err("文件里还没有 @startuml … @enduml".into()),
    }
}

fn token_at(content: &str, token: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = content[from..].find(token) {
        let at = from + found;
        let end = at + token.len();
        let before = at == 0 || content[..at].ends_with(char::is_whitespace);
        let after = end == content.len() || content[end..].starts_with(char::is_whitespace) || content[end..].starts_with('{')
            || token.starts_with("<<");
        if before && after {
            return Some(at);
        }
        from = end;
    }
    None
}

/// Applies `edit` to the file's bytes; fails without writing anything when
/// the file is not in a state the edit can be made in.
pub fn patch(path: &str, bytes: &[u8], edit: &Edit<'_>, include: &mut Include<'_>) -> Result<Vec<u8>, String> {
    let parsed = parse(path, bytes, include);
    let text = std::str::from_utf8(bytes.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(bytes)).map_err(|_| "文件不是 UTF-8，只读".to_owned())?;
    let bom = bytes.starts_with(b"\xEF\xBB\xBF");
    let mut lines: Vec<(String, String)> = split(text).into_iter().map(|(content, end)| (content.to_owned(), end.to_owned())).collect();
    match parsed.status {
        Status::Ok => {}
        Status::Unparsable if matches!(edit, Edit::Fix { .. }) => {}
        Status::Unparsable => return Err("文件当前无法解析，画布编辑已锁定；先修好文件".into()),
        Status::TooLarge => return Err("文件超过 1 MB 上限，只读".into()),
        Status::NotUtf8 => return Err("文件不是 UTF-8，只读".into()),
    }
    let eol = parsed.eol.clone();
    let insert = |lines: &mut Vec<(String, String)>, at: usize, new: Vec<String>| {
        // A last line without an ending gets one before lines follow it.
        if at == lines.len() {
            if let Some(last) = lines.last_mut() {
                if last.1.is_empty() {
                    last.1 = eol.clone();
                }
            }
        }
        for (offset, line) in new.into_iter().enumerate() {
            lines.insert(at + offset, (line, eol.clone()));
        }
    };
    let mut wanted: Option<String> = None;
    let diagram_of = |key: &str| parsed.element(key).map_or(0, |found| found.diagram);
    let gantt = match edit {
        Edit::Add { .. } => gantt_at(&parsed, 0),
        Edit::Title { key, .. } | Edit::State { key, .. } | Edit::Note { key, .. } => gantt_at(&parsed, diagram_of(key)),
        Edit::Link { to, .. } => gantt_at(&parsed, diagram_of(to)),
        Edit::Fix { .. } => false,
    };
    match edit {
        _ if gantt => wanted = gantt_patch(&parsed, &mut lines, edit, &insert)?,
        Edit::Add { title, alias, parent, token } => {
            let token = token.map(|token| format!(" {token}")).unwrap_or_default();
            let parent = parent.map(|key| element(&parsed, key)).transpose()?;
            let tree = parent.and_then(|parent| parent.decl.as_ref()).and_then(|decl| decl.marker)
                .or_else(|| parsed.diagrams.first().filter(|d| d.kind == "mindmap" || d.kind == "wbs").map(|_| ('*', 0)));
            if let Some((marker, depth)) = tree {
                if alias.is_some() {
                    return Err("思维导图节点按文字识别，没有别名".into());
                }
                if title.chars().any(char::is_control) || title.trim().is_empty() {
                    return Err("文字须非空、单行".into());
                }
                let at = match parent {
                    Some(parent) => {
                        let decl = editable(parent)?;
                        let (_, depth) = decl.marker.ok_or("上级不是思维导图节点")?;
                        let end = parsed.ends.get(parent.diagram).map_or(usize::MAX, |(_, end)| *end);
                        parsed.elements.iter().filter_map(|e| e.decl.as_ref())
                            .filter(|d| d.line > decl.line && d.line < end && d.marker.is_some_and(|(_, other)| other <= depth))
                            .map(|d| d.line).min().unwrap_or(end)
                    }
                    None if parsed.elements.is_empty() => uml_end_any(&parsed)?,
                    None => return Err("思维导图只能有一个根；新节点须有上级".into()),
                };
                let indent = parent.and_then(|p| p.decl.as_ref()).map(|d| d.indent.clone()).unwrap_or_default();
                insert(&mut lines, at, vec![format!("{indent}{} {title}{token}", marker.to_string().repeat(depth + 1))]);
                wanted = Some(title.to_string());
            } else {
                let alias = new_alias(&parsed, *alias)?;
                let sequence = parsed.diagrams.first().is_some_and(|diagram| diagram.sequence);
                if sequence && parent.is_some() {
                    return Err("时序图的参与方没有上级".into());
                }
                let (at, indent) = match parent {
                    Some(parent) => {
                        let decl = editable(parent)?;
                        let close = decl.close.ok_or_else(|| format!("上级 {} 不是带 {{ }} 的容器", parent.key))?;
                        if BODIES.contains(&parent.keyword.as_str()) {
                            return Err(format!("{} 是类，里面是成员不是元素", parent.key));
                        }
                        (close, format!("{}  ", decl.indent))
                    }
                    None => (uml_end(&parsed, 0)?, String::new()),
                };
                let keyword = if sequence { "participant" } else { "rectangle" };
                insert(&mut lines, at, vec![format!("{indent}{keyword} {} as {alias}{token}", quoted(title)?)]);
                wanted = Some(alias);
            }
        }
        Edit::Title { key, title } => {
            let found = element(&parsed, key)?;
            let decl = editable(found)?.clone();
            let line = &mut lines[decl.line].0;
            let written = match decl.style {
                Style::Tree => {
                    if title.trim().is_empty() || title.chars().any(char::is_control) {
                        return Err("文字须非空、单行".into());
                    }
                    title.to_string()
                }
                Style::Quoted | Style::Bracket | Style::Paren | Style::Colon | Style::Bare => {
                    let (open, close) = match decl.style {
                        Style::Bracket => ("[", "]"),
                        Style::Paren => ("(", ")"),
                        Style::Colon => (":", ":"),
                        _ => ("\"", "\""),
                    };
                    if title.contains(['"', ']', ')', ':']) || title.trim().is_empty() || title.chars().any(char::is_control) {
                        return Err("文字须非空、单行，且不含引号和括号".into());
                    }
                    let shown = format!("{open}{title}{close}");
                    match &found.alias {
                        Some(_) => shown,
                        None if ident(&found.name) => format!("{shown} as {}", found.name),
                        None => return Err(format!("元素 {key} 没有别名，改名会变成另一个元素；先在文件里写 as <别名>")),
                    }
                }
            };
            line.replace_range(decl.name.0..decl.name.1, &written);
            wanted = Some(if decl.style == Style::Tree { title.to_string() } else { found.key.clone() });
        }
        Edit::State { key, token, remove } => {
            let found = element(&parsed, key)?;
            let decl = editable(found)?.clone();
            let tree = decl.style == Style::Tree;
            if tree && token.is_some_and(|token| token.starts_with('#')) {
                return Err("思维导图节点的状态只用构造型".into());
            }
            let from = if tree { decl.name.1 } else { decl.head_end };
            let line = &mut lines[decl.line].0;
            for old in remove.iter() {
                while let Some(at) = token_at(&line[from..], old).map(|at| at + from) {
                    let start = if line[..at].ends_with(' ') { at - 1 } else { at };
                    line.replace_range(start..at + old.len(), "");
                }
            }
            if let Some(token) = token {
                if token.starts_with('#') && found.color.as_ref().is_some_and(|color| !remove.contains(color)) {
                    return Err(format!("元素 {key} 已有装饰颜色，不能再用颜色表示状态"));
                }
                let at = if tree { line.trim_end().len() } else { decl.head_end };
                line.insert_str(at, &format!(" {token}"));
            }
            wanted = Some(found.key.clone());
        }
        Edit::Link { from, to, label, dashed } => {
            let (a, b) = (element(&parsed, from)?, element(&parsed, to)?);
            let at = uml_end(&parsed, a.diagram)?;
            let label = match label {
                Some(label) if label.chars().any(char::is_control) => return Err("连线标签须是单行".into()),
                Some(label) if !label.trim().is_empty() => format!(" : {}", label.trim()),
                _ => String::new(),
            };
            let shape = match (parsed.diagrams.get(a.diagram).is_some_and(|diagram| diagram.sequence), *dashed) {
                (true, true) => "-->",
                (true, false) => "->",
                (false, true) => "..>",
                (false, false) => "-->",
            };
            insert(&mut lines, at, vec![format!("{} {shape} {}{label}", reference(a)?, reference(b)?)]);
        }
        Edit::Note { key, token, text } => {
            let found = element(&parsed, key)?;
            let at = uml_end(&parsed, found.diagram)?;
            if text.trim().is_empty() || text.chars().any(|c| c.is_control() && c != '\n') {
                return Err("旁注须非空".into());
            }
            let head = format!("note right of {}{}", reference(found)?, token.map(|t| format!(" {t}")).unwrap_or_default());
            let new = if text.contains('\n') || text.to_ascii_lowercase().contains("end note") {
                let mut new = vec![head];
                new.extend(text.lines().map(|line| format!("  {line}")));
                new.push("end note".into());
                new
            } else {
                vec![format!("{head} : {}", text.trim())]
            };
            insert(&mut lines, at, new);
        }
        Edit::Fix { id } => {
            if parsed.status != Status::Unparsable || !parsed.fixes.iter().any(|fix| fix.id == *id) {
                return Err("修复建议已不适用；文件已经变化".into());
            }
            if *id == "end" {
                let kind = parsed.diagrams.last().map(|d| d.kind.clone()).unwrap_or_else(|| "uml".into());
                let at = lines.len();
                insert(&mut lines, at, vec![end_word(&kind)]);
            } else if let Some(line) = id.strip_prefix("close-").and_then(|n| n.parse::<usize>().ok()) {
                let fix = parsed.fixes.iter().find(|fix| fix.id == *id).expect("found");
                let new: Vec<String> = fix.diff.iter().filter_map(|row| row.strip_prefix("+ ")).map(str::to_owned).collect();
                insert(&mut lines, line - 1, new);
            }
        }
    }
    let mut out = if bom { "\u{FEFF}".to_owned() } else { String::new() };
    for (content, end) in &lines {
        out.push_str(content);
        out.push_str(end);
    }
    let bytes = out.into_bytes();
    let check = parse(path, &bytes, include);
    // A fix may uncover the next problem; it only has to remove its own.
    let fixed = matches!(edit, Edit::Fix { id } if !check.fixes.iter().any(|fix| fix.id == *id) && check.problems.len() <= parsed.problems.len());
    if check.status != Status::Ok && !fixed {
        return Err(format!("修改后文件无法解析：{}", check.problems.first().map_or(String::new(), |p| p.reason.clone())));
    }
    if let Some(wanted) = wanted {
        let present = check.elements.iter().any(|e| e.key == wanted || e.name == wanted);
        if !present {
            return Err("修改后找不到目标元素；没有写入".into());
        }
    }
    Ok(bytes)
}

/// The alias asked for, or the first free `E<n>`.
fn new_alias(parsed: &Parsed, alias: Option<&str>) -> Result<String, String> {
    let taken = |name: &str| parsed.elements.iter().any(|e| e.key == name || e.alias.as_deref() == Some(name) || e.name == name);
    match alias {
        Some(alias) if !ident(alias) => Err(format!("别名 {alias} 须以字母或 _ 开头，只含字母、数字和 _")),
        Some(alias) if taken(alias) => Err(format!("别名 {alias} 已被使用")),
        Some(alias) => Ok(alias.to_string()),
        None => Ok((1..).map(|n| format!("E{n}")).find(|candidate| !taken(candidate)).expect("unbounded")),
    }
}

fn gantt_at(parsed: &Parsed, diagram: usize) -> bool {
    parsed.diagrams.get(diagram).is_some_and(|found| found.kind == "gantt")
}

fn task_name(title: &str) -> Result<&str, String> {
    if title.trim().is_empty() || title.contains(['[', ']']) || title.chars().any(char::is_control) {
        return Err("任务名须非空、单行，且不含方括号".into());
    }
    Ok(title.trim())
}

/// How a new Gantt line names a task.
fn task_ref(element: &Element) -> String {
    format!("[{}]", element.alias.as_deref().unwrap_or(&element.name))
}

/// FR-172 phase 2: the canvas edits a Gantt chart may make. A new task is
/// `[title] as [E1] lasts 1 day`, a dependency `[B] starts at [A]'s end`;
/// progress, notes and parents stay in the file. Returns the key to find
/// after the edit.
fn gantt_patch(
    parsed: &Parsed, lines: &mut Vec<(String, String)>, edit: &Edit<'_>, insert: &dyn Fn(&mut Vec<(String, String)>, usize, Vec<String>),
) -> Result<Option<String>, String> {
    match edit {
        Edit::Add { title, alias, parent, token } => {
            if parent.is_some() {
                return Err("甘特图的任务没有上级".into());
            }
            if token.is_some() {
                return Err(GANTT_STATE.into());
            }
            let title = task_name(title)?;
            let alias = new_alias(parsed, *alias)?;
            let at = uml_end_any(parsed)?;
            insert(lines, at, vec![format!("[{title}] as [{alias}] lasts 1 day")]);
            Ok(Some(alias))
        }
        Edit::Title { key, title } => {
            let found = element(parsed, key)?;
            let decl = editable(found)?.clone();
            let title = task_name(title)?;
            let written = format!("[{}]", found.name);
            let elsewhere = lines.iter().enumerate().find(|(at, (content, _))| *at != decl.line && content.contains(&written));
            if let Some((at, _)) = elsewhere {
                return Err(format!("任务 {} 在第 {} 行也被提到，改名会断开那里的引用；先在文件里给它写 as [别名]，别处改用别名", found.name, at + 1));
            }
            lines[decl.line].0.replace_range(decl.name.0..decl.name.1, &format!("[{title}]"));
            Ok(Some(found.alias.clone().unwrap_or_else(|| title.to_owned())))
        }
        Edit::State { .. } => Err(GANTT_STATE.into()),
        Edit::Note { .. } => Err("甘特图不能加旁注".into()),
        Edit::Link { from, to, label, dashed } => {
            if *dashed {
                return Err("甘特图的依赖没有虚线".into());
            }
            if label.is_some_and(|label| !label.trim().is_empty()) {
                return Err("甘特图的依赖不带标签".into());
            }
            let (a, b) = (element(parsed, from)?, element(parsed, to)?);
            editable(b)?;
            if b.time.as_ref().is_some_and(|time| time.anchored) {
                return Err(format!("任务 {} 已有开始日期或前置任务；在文件里改", b.key));
            }
            // Would `a` come after `b` already? Then this closes a cycle.
            let mut seen = vec![b.key.as_str()];
            let mut at = 0;
            while let Some(current) = seen.get(at).copied() {
                if current == a.key {
                    return Err(format!("这条依赖会成环：{} 已排在 {} 之后", a.key, b.key));
                }
                for link in parsed.links.iter().filter(|link| link.from == current) {
                    if !seen.contains(&link.to.as_str()) {
                        seen.push(&link.to);
                    }
                }
                at += 1;
            }
            let end = parsed.ends.get(b.diagram).map(|(_, end)| *end).filter(|end| *end != usize::MAX).ok_or("文件里还没有 @start … @end")?;
            insert(lines, end, vec![format!("{} starts at {}'s end", task_ref(b), task_ref(a))]);
            Ok(None)
        }
        Edit::Fix { .. } => Err("修复建议已不适用；文件已经变化".into()),
    }
}

fn uml_end_any(parsed: &Parsed) -> Result<usize, String> {
    match parsed.ends.first() {
        Some((_, end)) if *end != usize::MAX => Ok(*end),
        _ => Err("文件里还没有 @start … @end".into()),
    }
}

#[cfg(test)]
#[path = "plantuml_tests.rs"]
mod tests;
