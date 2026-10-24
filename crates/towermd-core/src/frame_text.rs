//! A frame as text and text as a new frame (core spec FR-155, FR-144).
//!
//! Export reads a frame as it stands and writes Markdown (lanes as
//! headings, two category axes as a table, nesting as an indented list) or
//! Mermaid (relations as a flowchart, a tree as a mindmap, a time axis as a
//! timeline, one category axis as a kanban). It is derived on every read and
//! never stored.
//!
//! Import reads Markdown lists, headings and tables and Mermaid mindmap,
//! flowchart, timeline and kanban text into the cards and lines a person's
//! new frame starts with (`frames::Created::cards`), mapped onto the
//! template the person picked. Anything that does not fit is reported by
//! line and nothing is created; nothing is dropped silently. Import never
//! writes into an existing frame: that stays the card verbs, which turn a
//! delegate's writes into proposals where a person set the frame.
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::{
    cards::{Op, View, Write},
    frames::{Axis, Layout, Structure, Template, IMPORT_MAX, STATE_FIELD},
};

pub const MARKDOWN: &str = "markdown";
pub const MERMAID: &str = "mermaid";
/// Pasted text larger than this is refused before parsing.
pub const TEXT_MAX: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Diagram {
    Flowchart,
    Mindmap,
    Timeline,
    Kanban,
}

fn lane_axes(layout: &Layout) -> Vec<&Axis> {
    [&layout.x, &layout.y].into_iter().filter(|axis| matches!(axis, Axis::Category { .. } | Axis::Time { .. })).collect()
}

fn nested(layout: &Layout) -> bool {
    layout.structure.iter().any(|kind| matches!(kind, Structure::Nesting | Structure::Tree))
}

fn related(layout: &Layout) -> bool {
    layout.structure.contains(&Structure::Relations)
}

fn diagram(layout: &Layout) -> Option<Diagram> {
    let axes = lane_axes(layout);
    if related(layout) {
        Some(Diagram::Flowchart)
    } else if nested(layout) {
        Some(Diagram::Mindmap)
    } else if axes.iter().any(|axis| matches!(axis, Axis::Time { .. })) {
        Some(Diagram::Timeline)
    } else if axes.len() == 1 {
        Some(Diagram::Kanban)
    } else {
        None
    }
}

/// The text forms a frame of this layout can take, the template's declared
/// `export` first. Markdown fits every layout.
pub fn formats(layout: &Layout, declared: &[String]) -> Vec<&'static str> {
    let mut supported = vec![MARKDOWN];
    if diagram(layout).is_some() {
        supported.push(MERMAID);
    }
    let mut ordered: Vec<&'static str> = declared.iter().filter_map(|name| supported.iter().copied().find(|format| format == name)).collect();
    ordered.dedup();
    for format in supported {
        if !ordered.contains(&format) {
            ordered.push(format);
        }
    }
    ordered
}

/// One card as export sees it: free cards first in the order added, then
/// bound cards. Archived cards and waiting proposals are not exported.
struct Entry {
    id: String,
    title: String,
    body: String,
    lanes: Vec<String>,
    parent: Option<String>,
    links: Vec<String>,
    extra: BTreeMap<String, Value>,
}

fn entries(view: &View) -> Vec<Entry> {
    let frame = &view.frame;
    let axes = lane_axes(&frame.layout);
    let axis_field = |axis: &Axis| match axis {
        Axis::Category { field, .. } | Axis::Time { field } => Some(field.clone()),
        _ => None,
    };
    let fields: Vec<String> = axes.iter().filter_map(|axis| axis_field(axis)).collect();
    let mut found: Vec<Entry> = frame.cards.iter().filter(|card| !card.archived).map(|card| Entry {
        id: card.id.clone(),
        title: card.title.clone(),
        body: card.body.clone(),
        lanes: fields.iter().map(|field| if field == STATE_FIELD {
            card.state.clone().unwrap_or_default()
        } else {
            card.fields.get(field).map(scalar).unwrap_or_default()
        }).collect(),
        parent: card.parent.clone(),
        links: card.links.clone(),
        extra: card.fields.iter().filter(|(name, _)| !fields.contains(name)).map(|(name, value)| (name.clone(), value.clone())).collect(),
    }).collect();
    found.extend(view.bound.iter().map(|card| Entry {
        id: card.id.clone(),
        title: card.title.clone(),
        body: String::new(),
        lanes: fields.iter().map(|field| if field == STATE_FIELD { card.state.clone() } else { String::new() }).collect(),
        parent: None,
        links: Vec::new(),
        extra: BTreeMap::new(),
    }));
    found
}

fn scalar(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())
}

/// Lanes of one axis in display order: declared values (the template's
/// states on the state field), then the rest as they appear; dates sorted.
fn lanes(frame_axis: &Axis, states: &[String], values: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen: Vec<String> = match frame_axis {
        Axis::Category { field, values } if values.is_empty() && field == STATE_FIELD => states.to_vec(),
        Axis::Category { values, .. } => values.clone(),
        _ => Vec::new(),
    };
    let declared = seen.len();
    for value in values {
        if !seen.contains(&value) {
            seen.push(value);
        }
    }
    if matches!(frame_axis, Axis::Time { .. }) {
        seen[declared..].sort();
    }
    seen
}

/// The frame as text in `format` (`markdown` or `mermaid`).
pub fn export(view: &View, format: &str) -> Result<String, String> {
    let frame = &view.frame;
    if !formats(&frame.layout, &[]).contains(&format) {
        return Err(format!("容器 {} 不能导出为 {format}（可用：{}）", frame.id, formats(&frame.layout, &[]).join("、")));
    }
    let found = entries(view);
    let axes = lane_axes(&frame.layout);
    let order: Vec<Vec<String>> = axes.iter().enumerate()
        .map(|(index, axis)| lanes(axis, &frame.states, found.iter().map(|entry| entry.lanes[index].clone())))
        .collect();
    Ok(if format == MARKDOWN {
        markdown(&frame.title, &found, &order, axes.iter().all(|axis| matches!(axis, Axis::Category { .. })))
    } else {
        mermaid(&frame.title, diagram(&frame.layout).expect("checked"), &found, &order)
    })
}

fn markdown(title: &str, found: &[Entry], order: &[Vec<String>], categories: bool) -> String {
    let mut out = format!("# {}\n", one_line(title));
    let titles: BTreeMap<&str, &str> = found.iter().map(|entry| (entry.id.as_str(), entry.title.as_str())).collect();
    let list = |out: &mut String, group: &[&Entry]| {
        out.push('\n');
        let ids: Vec<&str> = group.iter().map(|entry| entry.id.as_str()).collect();
        for root in group.iter().filter(|entry| entry.parent.as_deref().is_none_or(|parent| !ids.contains(&parent))) {
            item(out, root, group, &titles, 0);
        }
    };
    match order.len() {
        2 if categories => {
            let cell = |x: &str, y: &str| found.iter().filter(|entry| entry.lanes[0] == x && entry.lanes[1] == y)
                .map(|entry| cell_text(&entry.title)).collect::<Vec<_>>().join("<br>");
            out.push_str(&format!("\n| | {} |\n|{}\n", order[0].iter().map(|x| cell_text(x)).collect::<Vec<_>>().join(" | "),
                " --- |".repeat(order[0].len() + 1)));
            for y in &order[1] {
                let cells: Vec<String> = order[0].iter().map(|x| cell(x, y)).collect();
                out.push_str(&format!("| {} | {} |\n", cell_text(y), cells.join(" | ")));
            }
        }
        0 => list(&mut out, &found.iter().collect::<Vec<_>>()),
        _ => {
            for x in &order[0] {
                let group: Vec<&Entry> = found.iter().filter(|entry| &entry.lanes[0] == x).collect();
                if group.is_empty() {
                    continue;
                }
                out.push_str(&format!("\n## {}\n", one_line(x)));
                if order.len() == 1 {
                    list(&mut out, &group);
                    continue;
                }
                for y in &order[1] {
                    let inner: Vec<&Entry> = group.iter().copied().filter(|entry| &entry.lanes[1] == y).collect();
                    if !inner.is_empty() {
                        out.push_str(&format!("\n### {}\n", one_line(y)));
                        list(&mut out, &inner);
                    }
                }
            }
        }
    }
    out
}

fn item(out: &mut String, entry: &Entry, group: &[&Entry], titles: &BTreeMap<&str, &str>, depth: usize) {
    let indent = "  ".repeat(depth);
    out.push_str(&format!("{indent}- {}\n", one_line(&entry.title)));
    for line in entry.body.lines() {
        out.push_str(&format!("{indent}  {line}\n"));
    }
    if !entry.extra.is_empty() {
        out.push_str(&format!("{indent}  <!-- fields {} -->\n", serde_json::to_string(&entry.extra).unwrap_or_default()));
    }
    for to in &entry.links {
        if let Some(title) = titles.get(to.as_str()) {
            out.push_str(&format!("{indent}  → {}\n", one_line(title)));
        }
    }
    for child in group.iter().filter(|child| child.parent.as_deref() == Some(entry.id.as_str())) {
        item(out, child, group, titles, depth + 1);
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn cell_text(text: &str) -> String {
    one_line(text).replace('|', "\\|")
}

/// Mermaid labels keep the characters that would end them as entity codes.
fn label(text: &str) -> String {
    one_line(text).replace('"', "#quot;").replace('[', "#91;").replace(']', "#93;").replace(':', "#58;")
}

fn decode(text: &str) -> String {
    text.replace("#quot;", "\"").replace("#34;", "\"").replace("#91;", "[").replace("#93;", "]").replace("#58;", ":")
        .replace("#40;", "(").replace("#41;", ")")
}

fn mermaid(title: &str, kind: Diagram, found: &[Entry], order: &[Vec<String>]) -> String {
    let node = |entry: &Entry| format!("n{}", found.iter().position(|other| other.id == entry.id).unwrap_or(0) + 1);
    let mut out = String::new();
    match kind {
        Diagram::Kanban => {
            out.push_str("kanban\n");
            for (index, lane) in order[0].iter().enumerate() {
                out.push_str(&format!("  lane{}[{}]\n", index + 1, label(lane)));
                for entry in found.iter().filter(|entry| &entry.lanes[0] == lane) {
                    out.push_str(&format!("    {}[{}]\n", node(entry), label(&entry.title)));
                }
            }
        }
        Diagram::Mindmap => {
            out.push_str(&format!("mindmap\n  root[{}]\n", label(title)));
            fn branch(out: &mut String, found: &[Entry], parent: Option<&str>, depth: usize, node: &dyn Fn(&Entry) -> String) {
                for entry in found.iter().filter(|entry| entry.parent.as_deref() == parent) {
                    out.push_str(&format!("{}{}[{}]\n", "  ".repeat(depth), node(entry), label(&entry.title)));
                    branch(out, found, Some(&entry.id), depth + 1, node);
                }
            }
            branch(&mut out, found, None, 2, &node);
        }
        Diagram::Flowchart => {
            out.push_str(&format!("---\ntitle: {}\n---\nflowchart TD\n", one_line(title)));
            fn group(out: &mut String, found: &[Entry], parent: Option<&str>, depth: usize, node: &dyn Fn(&Entry) -> String) {
                for entry in found.iter().filter(|entry| entry.parent.as_deref() == parent) {
                    let indent = "  ".repeat(depth);
                    if found.iter().any(|child| child.parent.as_deref() == Some(entry.id.as_str())) {
                        out.push_str(&format!("{indent}subgraph {}[\"{}\"]\n", node(entry), label(&entry.title)));
                        group(out, found, Some(&entry.id), depth + 1, node);
                        out.push_str(&format!("{indent}end\n"));
                    } else {
                        out.push_str(&format!("{indent}{}[\"{}\"]\n", node(entry), label(&entry.title)));
                    }
                }
            }
            group(&mut out, found, None, 1, &node);
            for entry in found {
                for to in &entry.links {
                    if let Some(target) = found.iter().find(|other| &other.id == to) {
                        out.push_str(&format!("  {} --> {}\n", node(entry), node(target)));
                    }
                }
            }
        }
        Diagram::Timeline => {
            out.push_str(&format!("timeline\n  title {}\n", label(title)));
            let periods = |out: &mut String, group: &[&Entry], indent: &str| {
                for date in &order[0] {
                    let events: Vec<String> = group.iter().filter(|entry| &entry.lanes[0] == date).map(|entry| label(&entry.title)).collect();
                    if !date.is_empty() && !events.is_empty() {
                        out.push_str(&format!("{indent}{date} : {}\n", events.join(" : ")));
                    }
                }
            };
            if order.len() == 2 {
                for section in &order[1] {
                    let group: Vec<&Entry> = found.iter().filter(|entry| &entry.lanes[1] == section).collect();
                    if !group.is_empty() {
                        out.push_str(&format!("  section {}\n", label(section)));
                        periods(&mut out, &group, "    ");
                    }
                }
            } else {
                periods(&mut out, &found.iter().collect::<Vec<_>>(), "  ");
            }
        }
    }
    out
}

/// Text read into cards, before it meets a template. `lanes` are by
/// position: the first is the heading, column, kanban lane or timeline
/// date; the second the sub-heading, table row or timeline section.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Parsed {
    pub kind: &'static str,
    pub title: Option<String>,
    pub items: Vec<Item>,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Item {
    pub line: usize,
    pub title: String,
    pub body: String,
    pub lanes: Vec<String>,
    pub parent: Option<usize>,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Link {
    pub line: usize,
    pub from: usize,
    pub to: usize,
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// Reads pasted text; every line it cannot place is a problem.
pub fn parse(text: &str) -> Result<Parsed, Vec<String>> {
    if text.len() > TEXT_MAX {
        return Err(vec![format!("文本超过 {} KiB", TEXT_MAX / 1024)]);
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut start = 0;
    let mut front_title = None;
    // Mermaid front matter: `---`, `title: …`, `---`.
    if lines.first().is_some_and(|line| line.trim() == "---") {
        if let Some(end) = lines.iter().skip(1).position(|line| line.trim() == "---") {
            front_title = lines[1..=end].iter().find_map(|line| line.trim().strip_prefix("title:")).map(|title| title.trim().to_owned());
            start = end + 2;
        }
    }
    let first = lines.iter().enumerate().skip(start).find(|(_, line)| !line.trim().is_empty() && !line.trim().starts_with("%%"));
    let head = first.map(|(_, line)| line.trim().split_whitespace().next().unwrap_or_default()).unwrap_or_default();
    let body = first.map(|(index, _)| index + 1).unwrap_or(lines.len());
    let mut parsed = match head {
        "mindmap" => mindmap(&lines, body),
        "flowchart" | "graph" => flowchart(&lines, body),
        "timeline" => timeline(&lines, body),
        "kanban" => kanban(&lines, body),
        "@startuml" | "@startmindmap" => return Err(vec!["PlantUML 文本暂不能导入，请用 Mermaid 或 Markdown 列表".into()]),
        "sequenceDiagram" | "gantt" | "classDiagram" | "stateDiagram" | "stateDiagram-v2" | "erDiagram" | "journey" | "pie"
        | "quadrantChart" | "gitGraph" | "requirementDiagram" | "C4Context" | "sankey-beta" | "xychart-beta" | "block-beta" => {
            return Err(vec![format!("Mermaid {head} 暂不能导入；可导入 mindmap、flowchart、timeline、kanban 或 Markdown 列表")]);
        }
        _ => markdown_items(&lines, start),
    }?;
    if parsed.title.is_none() {
        parsed.title = front_title;
    }
    if parsed.items.is_empty() {
        return Err(vec!["没有读出任何卡片".into()]);
    }
    if parsed.items.len() + parsed.links.len() > IMPORT_MAX {
        return Err(vec![format!("一次最多导入 {IMPORT_MAX} 项（卡片加连线），这里有 {} 项", parsed.items.len() + parsed.links.len())]);
    }
    Ok(parsed)
}

/// `id[text]`, `id(text)`, `id((text))`, `id{{text}}`, `id>text]`, `id["text"]`
/// and plain `text` read as `(id, text)`.
fn node_text(raw: &str) -> (String, String) {
    let raw = raw.split(":::").next().unwrap_or_default();
    let raw = raw.split("@{").next().unwrap_or_default().trim();
    let Some(open) = raw.find(['[', '(', '{', '>']) else {
        return (raw.to_owned(), decode(raw));
    };
    let id = raw[..open].trim().to_owned();
    let inner = raw[open..].trim_start_matches(['[', '(', '{', '>', '/', '\\']).trim_end_matches([']', ')', '}', '/', '\\']);
    let inner = inner.trim();
    let inner = inner.strip_prefix('"').and_then(|text| text.strip_suffix('"')).unwrap_or(inner);
    let inner = inner.strip_prefix('`').and_then(|text| text.strip_suffix('`')).unwrap_or(inner);
    let id = if id.is_empty() { inner.to_owned() } else { id };
    (id, decode(inner.trim()))
}

fn mindmap(lines: &[&str], from: usize) -> Result<Parsed, Vec<String>> {
    let mut parsed = Parsed { kind: "mermaid mindmap", ..Parsed::default() };
    let mut root: Option<usize> = None;
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(from) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") || trimmed.starts_with("::") {
            continue;
        }
        let indent = indent_of(line);
        let (_, text) = node_text(trimmed);
        let Some(root_indent) = root else {
            root = Some(indent);
            parsed.title = Some(text);
            continue;
        };
        if indent <= root_indent {
            return Err(vec![format!("第 {} 行：mindmap 只能有一个根节点", index + 1)]);
        }
        while stack.last().is_some_and(|(depth, _)| *depth >= indent) {
            stack.pop();
        }
        parsed.items.push(Item { line: index + 1, title: text, parent: stack.last().map(|(_, item)| *item), ..Item::default() });
        stack.push((indent, parsed.items.len() - 1));
    }
    Ok(parsed)
}

fn kanban(lines: &[&str], from: usize) -> Result<Parsed, Vec<String>> {
    let mut parsed = Parsed { kind: "mermaid kanban", ..Parsed::default() };
    let mut column: Option<(usize, String)> = None;
    let mut problems = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(from) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }
        let indent = indent_of(line);
        let (_, text) = node_text(trimmed);
        match &column {
            Some((depth, lane)) if indent > *depth => {
                parsed.items.push(Item { line: index + 1, title: text, lanes: vec![lane.clone()], ..Item::default() });
            }
            Some((depth, _)) if indent < *depth => problems.push(format!("第 {} 行：缩进比栏还浅，读不出它属于哪一栏", index + 1)),
            _ => column = Some((indent, text)),
        }
    }
    if problems.is_empty() { Ok(parsed) } else { Err(problems) }
}

fn timeline(lines: &[&str], from: usize) -> Result<Parsed, Vec<String>> {
    let mut parsed = Parsed { kind: "mermaid timeline", ..Parsed::default() };
    let mut section: Option<String> = None;
    let mut period: Option<String> = None;
    for (index, line) in lines.iter().enumerate().skip(from) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("title ") {
            parsed.title = Some(decode(title.trim()));
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("section ") {
            section = Some(decode(name.trim()));
            period = None;
            continue;
        }
        let mut parts = trimmed.split(':').map(str::trim);
        let head = parts.next().unwrap_or_default();
        if !head.is_empty() {
            period = Some(head.to_owned());
        }
        let Some(date) = period.clone() else {
            return Err(vec![format!("第 {} 行：事件前面没有时间", index + 1)]);
        };
        for event in parts.filter(|event| !event.is_empty()) {
            let mut lanes = vec![date.clone()];
            lanes.extend(section.clone());
            parsed.items.push(Item { line: index + 1, title: decode(event), lanes, ..Item::default() });
        }
    }
    Ok(parsed)
}

/// Splits `a --> b -->|x| c` into nodes; `--` before a label joins it.
fn edge_parts(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut parts = vec![String::new()];
    let mut depth = 0i32;
    let mut quoted = false;
    let mut index = 0;
    let mut in_label = false;
    while index < chars.len() {
        let c = chars[index];
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted {
            match c {
                '[' | '(' | '{' => depth += 1,
                ']' | ')' | '}' => depth -= 1,
                _ => {}
            }
        }
        let next = chars.get(index + 1).copied().unwrap_or(' ');
        let arrow = !quoted && depth <= 0 && matches!((c, next), ('-', '-') | ('=', '=') | ('-', '.') | ('<', '-') | ('<', '='));
        if arrow {
            let begin = index;
            while index < chars.len() && "-=.<>".contains(chars[index]) {
                index += 1;
            }
            if index < chars.len() && matches!(chars[index], 'o' | 'x') && chars.get(index + 1).is_none_or(|c| c.is_whitespace()) {
                index += 1;
            }
            let token: String = chars[begin..index].iter().collect();
            let opens_label = !token.contains('>') && token.len() == 2 && !in_label;
            if in_label {
                in_label = false;
            } else if opens_label {
                in_label = true;
                continue;
            }
            // `|label|` right after an arrow belongs to the line, not a node.
            let mut rest = index;
            while rest < chars.len() && chars[rest].is_whitespace() {
                rest += 1;
            }
            if chars.get(rest) == Some(&'|') {
                if let Some(close) = chars[rest + 1..].iter().position(|c| *c == '|') {
                    index = rest + close + 2;
                }
            }
            parts.push(String::new());
            continue;
        }
        if !in_label {
            parts.last_mut().expect("one part").push(c);
        }
        index += 1;
    }
    parts.into_iter().map(|part| part.trim().trim_end_matches(';').trim().to_owned()).collect()
}

fn flowchart(lines: &[&str], from: usize) -> Result<Parsed, Vec<String>> {
    let mut parsed = Parsed { kind: "mermaid flowchart", ..Parsed::default() };
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups: Vec<usize> = Vec::new();
    let mut problems = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(from) {
        let trimmed = line.trim().trim_end_matches(';').trim();
        let word = trimmed.split_whitespace().next().unwrap_or_default();
        if trimmed.is_empty() || trimmed.starts_with("%%")
            || matches!(word, "direction" | "classDef" | "class" | "style" | "linkStyle" | "click") {
            continue;
        }
        if trimmed == "end" {
            if groups.pop().is_none() {
                problems.push(format!("第 {} 行：多出的 end", index + 1));
            }
            continue;
        }
        let mut place = |raw: &str, parsed: &mut Parsed| -> Option<usize> {
            let (id, text) = node_text(raw);
            if id.is_empty() {
                return None;
            }
            if let Some(found) = ids.get(&id) {
                if raw.contains(['[', '(', '{', '>']) {
                    parsed.items[*found].title = text;
                }
                return Some(*found);
            }
            parsed.items.push(Item { line: index + 1, title: text, parent: groups.last().copied(), ..Item::default() });
            ids.insert(id, parsed.items.len() - 1);
            Some(parsed.items.len() - 1)
        };
        if let Some(rest) = trimmed.strip_prefix("subgraph") {
            let rest = rest.trim();
            let raw = match rest.find('[') {
                Some(open) if !rest[..open].trim().is_empty() => format!("{}{}", rest[..open].trim(), &rest[open..]),
                _ => rest.to_owned(),
            };
            match place(&raw, &mut parsed) {
                Some(group) => groups.push(group),
                None => problems.push(format!("第 {} 行：子图没有名字", index + 1)),
            }
            continue;
        }
        let parts = edge_parts(trimmed);
        if parts.iter().any(|part| part.contains('&')) {
            problems.push(format!("第 {} 行：暂不支持用 & 连多个节点，请拆成多行", index + 1));
            continue;
        }
        if parts.iter().any(String::is_empty) {
            problems.push(format!("第 {} 行：读不出连线两端的节点", index + 1));
            continue;
        }
        let nodes: Vec<usize> = parts.iter().filter_map(|part| place(part, &mut parsed)).collect();
        for pair in nodes.windows(2) {
            if pair[0] != pair[1] && !parsed.links.iter().any(|link| link.from == pair[0] && link.to == pair[1]) {
                parsed.links.push(Link { line: index + 1, from: pair[0], to: pair[1] });
            }
        }
    }
    if !groups.is_empty() {
        problems.push("子图缺少 end".into());
    }
    if problems.is_empty() { Ok(parsed) } else { Err(problems) }
}

fn list_item(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ ")) {
        rest
    } else {
        let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        trimmed[digits..].strip_prefix(". ").or_else(|| trimmed[digits..].strip_prefix(") "))?
    };
    let rest = rest.trim();
    Some(["[ ] ", "[x] ", "[X] "].iter().find_map(|mark| rest.strip_prefix(mark)).unwrap_or(rest).trim())
}

fn cells(row: &str) -> Vec<String> {
    let row = row.trim().trim_start_matches('|');
    let row = row.strip_suffix('|').filter(|rest| !rest.ends_with('\\')).unwrap_or(row);
    let mut out = vec![String::new()];
    let mut chars = row.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => out.last_mut().expect("one cell").push(chars.next().expect("peeked")),
            '|' => out.push(String::new()),
            _ => out.last_mut().expect("one cell").push(c),
        }
    }
    out.into_iter().map(|cell| cell.trim().to_owned()).collect()
}

fn markdown_items(lines: &[&str], from: usize) -> Result<Parsed, Vec<String>> {
    let mut parsed = Parsed { kind: "markdown", ..Parsed::default() };
    let mut problems = Vec::new();
    let mut heading: (Option<String>, Option<String>) = (None, None);
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut pending_links: Vec<(usize, usize, String)> = Vec::new();
    let mut table: Option<Vec<String>> = None;
    for (index, line) in lines.iter().enumerate().skip(from) {
        let trimmed = line.trim();
        let number = index + 1;
        if trimmed.is_empty() {
            table = None;
            continue;
        }
        if trimmed.starts_with('|') {
            let row = cells(trimmed);
            match &table {
                None => table = Some(row),
                Some(_) if row.iter().all(|cell| !cell.is_empty() && cell.chars().all(|c| matches!(c, '-' | ':'))) => {}
                Some(header) => {
                    let y = row.first().cloned().unwrap_or_default();
                    for (column, cell) in row.iter().enumerate().skip(1) {
                        let x = header.get(column).cloned().unwrap_or_default();
                        for title in cell.split("<br>").flat_map(|part| part.split("<br/>")).flat_map(|part| part.split("<br />")) {
                            let title = title.trim();
                            if !title.is_empty() {
                                parsed.items.push(Item { line: number, title: title.to_owned(), lanes: vec![x.clone(), y.clone()], ..Item::default() });
                            }
                        }
                    }
                }
            }
            stack.clear();
            continue;
        }
        table = None;
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes > 0 && trimmed[hashes..].starts_with(' ') {
            let text = trimmed[hashes..].trim().to_owned();
            stack.clear();
            match hashes {
                1 if parsed.title.is_none() && parsed.items.is_empty() => parsed.title = Some(text),
                1 | 2 => heading = (Some(text), None),
                3 => heading.1 = Some(text),
                _ => problems.push(format!("第 {number} 行：最多读到三级标题")),
            }
            continue;
        }
        let indent = indent_of(line);
        if let Some(title) = list_item(line) {
            while stack.last().is_some_and(|(depth, _)| *depth >= indent) {
                stack.pop();
            }
            let lanes = [heading.0.clone(), heading.1.clone()].into_iter().map_while(|lane| lane).collect();
            parsed.items.push(Item { line: number, title: title.to_owned(), lanes, parent: stack.last().map(|(_, item)| *item), ..Item::default() });
            stack.push((indent, parsed.items.len() - 1));
            continue;
        }
        // A continuation line belongs to the deepest item it is indented under.
        let Some(&(_, owner)) = stack.iter().rev().find(|(depth, _)| indent > *depth) else {
            problems.push(format!("第 {number} 行：不是列表项、标题或表格"));
            continue;
        };
        if let Some(json) = trimmed.strip_prefix("<!-- fields ").and_then(|rest| rest.strip_suffix("-->")) {
            match serde_json::from_str::<BTreeMap<String, Value>>(json.trim()) {
                Ok(fields) => parsed.items[owner].fields.extend(fields),
                Err(error) => problems.push(format!("第 {number} 行：字段注释读不懂（{error}）")),
            }
        } else if let Some(to) = trimmed.strip_prefix("→ ") {
            pending_links.push((number, owner, to.trim().to_owned()));
        } else {
            let body = &mut parsed.items[owner].body;
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(trimmed);
        }
    }
    for (line, from, title) in pending_links {
        let matches: Vec<usize> = parsed.items.iter().enumerate().filter(|(_, item)| item.title == title).map(|(index, _)| index).collect();
        match matches.as_slice() {
            [to] => parsed.links.push(Link { line, from, to: *to }),
            [] => problems.push(format!("第 {line} 行：连线指向的「{title}」不存在")),
            _ => problems.push(format!("第 {line} 行：连线指向的「{title}」不止一张，无法确定")),
        }
    }
    if problems.is_empty() { Ok(parsed) } else { Err(problems) }
}

/// The parsed text as the cards and lines of new frame `frame`, made with
/// `template` and the usable `fields`. Lanes land on the template's
/// category and time axes in order (x, then y).
pub fn writes(parsed: &Parsed, frame: &str, template: &Template, fields: &[String]) -> Result<Vec<Write>, Vec<String>> {
    let axes = lane_axes(&template.layout);
    let mut problems = Vec::new();
    let mut out = Vec::new();
    let name = template.title.clone().unwrap_or_else(|| template.id.clone());
    for item in &parsed.items {
        let at = item.line;
        let mut state = None;
        let mut values = item.fields.clone();
        let chars = item.title.chars().count();
        if chars == 0 || chars > template.limits.title_max as usize {
            problems.push(format!("第 {at} 行：卡片标题须有 1 到 {} 字，「{}」有 {chars} 字", template.limits.title_max, item.title));
        }
        if item.body.chars().count() > template.limits.body_max as usize {
            problems.push(format!("第 {at} 行：正文超过 {} 字", template.limits.body_max));
        }
        for (position, lane) in item.lanes.iter().enumerate().filter(|(_, lane)| !lane.is_empty()) {
            match axes.get(position) {
                None => problems.push(format!("第 {at} 行：模板「{name}」没有第 {} 根分类或时间轴，放不下「{lane}」", position + 1)),
                Some(Axis::Category { field, .. }) if field == STATE_FIELD => {
                    if template.states.contains(lane) {
                        state = Some(lane.clone());
                    } else {
                        problems.push(format!("第 {at} 行：「{lane}」不是模板「{name}」的状态（{}）", template.states.join("、")));
                    }
                }
                Some(Axis::Category { field, values: allowed }) => {
                    if !allowed.is_empty() && !allowed.contains(lane) {
                        problems.push(format!("第 {at} 行：{field} 只能取 {}，不能是「{lane}」", allowed.join("、")));
                    }
                    values.insert(field.clone(), Value::String(lane.clone()));
                }
                Some(Axis::Time { field }) => {
                    if chrono::NaiveDate::parse_from_str(lane, "%Y-%m-%d").is_err() {
                        problems.push(format!("第 {at} 行：「{lane}」不是 YYYY-MM-DD 日期，排不上时间轴"));
                    }
                    values.insert(field.clone(), Value::String(lane.clone()));
                }
                Some(_) => {}
            }
        }
        for axis in &axes {
            if let Axis::Category { field, .. } | Axis::Time { field } = axis {
                if field != STATE_FIELD && fields.contains(field) && !values.contains_key(field) {
                    problems.push(format!("第 {at} 行：卡片没有 {field}，它决定卡片在模板「{name}」里的位置"));
                }
            }
        }
        if item.parent.is_some() && !nested(&template.layout) {
            problems.push(format!("第 {at} 行：模板「{name}」的结构不是嵌套或树，放不下上下级"));
        }
        out.push(Write { target: None, op: Op::Add {
            title: item.title.clone(), body: item.body.clone(), state, fields: values,
            parent: item.parent.map(|parent| format!("{frame}.{}", parent + 1)),
        } });
    }
    if !parsed.links.is_empty() && !related(&template.layout) {
        problems.push(format!("第 {} 行：模板「{name}」没有关系线，放不下 {} 条连线", parsed.links[0].line, parsed.links.len()));
    }
    out.extend(parsed.links.iter().map(|link| Write {
        target: Some(format!("{frame}.{}", link.from + 1)),
        op: Op::Link { to: format!("{frame}.{}", link.to + 1) },
    }));
    if problems.is_empty() { Ok(out) } else { Err(problems) }
}

#[cfg(test)]
#[path = "frame_text_tests.rs"]
mod tests;
