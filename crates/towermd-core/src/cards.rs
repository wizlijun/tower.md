//! Cards in a frame and the frame's rules at work (core spec FR-146–153,
//! FR-162, FR-187).
//!
//! A delegate or a source writes cards with the Tower API's sixth verb,
//! `card` (`towermd frame add/update/move/link/archive` on the command
//! line): the event carries the whole envelope, signed by the entry that
//! accepted it, and one fence on the frame's lease `frame:<frame-id>`. A
//! person writes cards from the App with the same operations, signed
//! `human:<id>`, without a lease.
//!
//! Every delegate write is checked against the template's format and the
//! frame's rules; a write that fails is rejected with the reason and nothing
//! is stored. A passing write still waits for a person when the frame is
//! set by a person (a proposal, shown dashed), when it overwrites what a
//! person set (a suggestion, FR-150), or when a free card would enter a
//! done state that the rules say needs a person (a confirmation, FR-152).
//! Rule changes from delegates are always proposals. Nothing waiting ever
//! takes effect by itself (`frame.resolved`, signed by a person).
//!
//! Bound cards (FR-148) are not stored: they are read from the core objects
//! each time (`view`), so their state follows the object and a done state
//! only appears once the object is done by FR-124.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation::Phase,
    delegation_projection::{self as delegations, Mark, Verdict},
    frames::{self, mark, Axis, FieldKind, Frame, Frames, Mode, RulesInput, SetBy, Structure, Until, Who, GROUP},
    hub::Event,
    kit::Kit,
    question_model::{human_actor, valid_id},
    tower_api::{self, check_signer, Envelope, Verb},
};

/// Cards are fast-layer progress.
pub const LAYER: &str = tower_api::LAYER;
pub const ADDED: &str = "card.added";
pub const UPDATED: &str = "card.updated";
pub const MOVED: &str = "card.moved";
pub const LINKED: &str = "card.linked";
pub const ARCHIVED: &str = "card.archived";
/// A delegate asks to change the rules; only a person decides.
pub const RULES_PROPOSED: &str = "frame.rules_proposed";
/// A person changed the rules (FR-146), in the App only.
pub const RULES_CHANGED: &str = "frame.rules_changed";
/// A person accepted or ignored one waiting item, in the App only.
pub const RESOLVED: &str = "frame.resolved";
/// FR-149: a person drags cards to fixed places in their frame, or lets
/// them go back to the derived layout. Human only, at the frame's layer.
pub const PINNED: &str = "card.pinned";
/// At most this many pins in one event.
pub const PINS_MAX: usize = 200;
/// FR-187: one writer per frame at a time.
pub const LEASE_PREFIX: &str = "frame:";
/// A delegate holds the frame for a burst of writes, renewed by each one.
pub const LEASE_TTL_MS: u64 = 60_000;
/// Text and reference field values.
pub const VALUE_CHARS: usize = 512;
/// FR-165: an edit of a text-source frame's file, queued for the writer.
pub const SOURCE: &str = "card.source";
const CARD_KINDS: [&str; 7] = [ADDED, UPDATED, MOVED, LINKED, ARCHIVED, RULES_PROPOSED, SOURCE];

/// One `card` operation, the body of the verb and of a person's write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    Add {
        title: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        fields: BTreeMap<String, Value>,
        /// Nesting and trees: the card this one sits under, fixed at add.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
    },
    /// A `null` field value clears the field.
    Update {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        fields: BTreeMap<String, Value>,
    },
    Move { state: String },
    Link { to: String },
    Archive {},
    Rules { rules: RulesInput },
    /// FR-165: a text-source frame's cards come from its file; edits wait
    /// as writes of that file.
    Source { change: crate::source_frames::Change },
}

impl Op {
    pub fn kind(&self) -> &'static str {
        match self {
            Op::Add { .. } => ADDED,
            Op::Update { .. } => UPDATED,
            Op::Move { .. } => MOVED,
            Op::Link { .. } => LINKED,
            Op::Archive {} => ARCHIVED,
            Op::Rules { .. } => RULES_PROPOSED,
            Op::Source { .. } => SOURCE,
        }
    }
    /// Add and rule proposals name the frame; the rest name a card.
    pub fn on_frame(&self) -> bool {
        matches!(self, Op::Add { .. } | Op::Rules { .. } | Op::Source { .. })
    }
    fn label(&self) -> &'static str {
        match self {
            Op::Add { .. } => "加卡片",
            Op::Update { .. } => "改文本",
            Op::Move { .. } => "改状态",
            Op::Link { .. } => "加连线",
            Op::Archive {} => "归档卡片",
            Op::Rules { .. } => "提议改规则",
            Op::Source { .. } => "改文本源",
        }
    }
}

/// A card as it stands. `changed` and `by` are its last modifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    /// `<frame-id>.<n>`, numbered in the order cards were added.
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    /// FR-150: what a person set last (`title`, `body`, `state` or a field
    /// name); a delegate may only suggest a change to these.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub human: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    pub added: Mark,
    pub changed: Mark,
    /// The self-reported sender of the last delegate write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// FR-153: a delegate wrote to a frame a person set.
    Proposal,
    /// FR-150: a delegate would overwrite what a person set.
    Suggestion,
    /// FR-152: a free card would enter a done state.
    Confirm,
    /// FR-146: a delegate asks to change the rules.
    Rules,
    /// FR-165: an edit queued for the writer to patch into the source
    /// file; with a note, the file no longer allows it (FR-167).
    Write,
}

/// One item waiting for a person; `id` is the sequence of the event that
/// raised it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub id: u64,
    pub why: Why,
    pub target: String,
    pub op: Op,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub by: Mark,
    /// FR-167: why the source file refused this write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesChanged {
    pub rules: frames::Rules,
}

/// `card.pinned`: each card to its new place, or `null` to unpin it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pinned {
    pub pins: BTreeMap<String, Option<frames::Pin>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolved {
    pub pending: u64,
    pub accept: bool,
}

/// The frame a card ID belongs to: `<frame-id>.<n>`.
pub fn card_frame(id: &str) -> Result<&str, String> {
    let frame = id
        .rsplit_once('.')
        .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0'))
        .map(|(frame, _)| frame)
        .ok_or_else(|| format!("卡片ID须形如 <容器ID>.<序号>：{id}；绑定卡片跟随核心对象，不能直接写"))?;
    frames::check_id(frame)?;
    Ok(frame)
}

/// The frame an operation on `target` writes to.
pub fn frame_of<'a>(op: &Op, target: &'a str) -> Result<&'a str, String> {
    if op.on_frame() {
        frames::check_id(target)?;
        Ok(target)
    } else {
        card_frame(target)
    }
}

pub fn lease_name(frame: &str) -> String {
    format!("{LEASE_PREFIX}{frame}")
}

/// Whether `kind` is a delegate's write that needs the frame's lease.
pub fn is_write(kind: &str) -> bool {
    CARD_KINDS.contains(&kind)
}

fn one_line(text: &str, max: u32, label: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().any(char::is_control) {
        return Err(format!("{label}须是非空单行文本"));
    }
    if text.chars().count() > max as usize {
        return Err(format!("{label}不能超过 {max} 字"));
    }
    Ok(())
}

fn body(text: &str, max: u32) -> Result<(), String> {
    if text.chars().any(|c| c.is_control() && c != '\n') {
        return Err("正文不能含控制字符".into());
    }
    if text.chars().count() > max as usize {
        return Err(format!("正文不能超过 {max} 字"));
    }
    Ok(())
}

fn field_value(name: &str, kind: FieldKind, value: &Value) -> Result<(), String> {
    let text = value.as_str();
    let ok = match kind {
        FieldKind::Text => text.is_some_and(|text| one_line(text, VALUE_CHARS as u32, "").is_ok()),
        FieldKind::Ref => text.is_some_and(|text| !text.trim().is_empty() && text.chars().count() <= VALUE_CHARS && !text.chars().any(char::is_control)),
        FieldKind::Date => text.is_some_and(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()),
        FieldKind::Number => value.is_number(),
    };
    if !ok {
        let what = match kind {
            FieldKind::Text => "不超过 512 字的单行文本",
            FieldKind::Ref => "不超过 512 字的引用",
            FieldKind::Date => "YYYY-MM-DD 日期",
            FieldKind::Number => "数字",
        };
        return Err(format!("字段 {name} 须是{what}"));
    }
    Ok(())
}

/// The template's kind of a field the rules make usable.
fn usable(frame: &Frame, name: &str) -> Result<FieldKind, String> {
    if !frame.rules.fields.iter().any(|field| field == name) {
        return Err(format!("字段 {name} 不是这个容器可用的卡片字段（可用：{}）", frame.rules.fields.join("、")));
    }
    let template = frames::Template::parse(&frame.template.definition).map_err(|problems| problems.join("；"))?;
    template.card_fields.get(name).copied().ok_or_else(|| format!("字段 {name} 不在模板里"))
}

/// Category axes on card fields with fixed values; time axes. Both need the
/// card to say where it goes.
fn axis_fields(frame: &Frame) -> Vec<(&str, Option<&[String]>)> {
    [&frame.layout.x, &frame.layout.y]
        .into_iter()
        .filter_map(|axis| match axis {
            Axis::Category { field, values } if field != frames::STATE_FIELD => Some((field.as_str(), Some(values.as_slice()))),
            Axis::Time { field } => Some((field.as_str(), None)),
            _ => None,
        })
        .filter(|(field, _)| frame.rules.fields.iter().any(|usable| usable == field))
        .collect()
}

fn check_fields(frame: &Frame, fields: &BTreeMap<String, Value>, clearing: bool) -> Result<(), String> {
    for (name, value) in fields {
        let kind = usable(frame, name)?;
        if value.is_null() {
            if !clearing {
                return Err(format!("字段 {name} 不能为空"));
            }
            if axis_fields(frame).iter().any(|(field, _)| field == name) {
                return Err(format!("字段 {name} 决定卡片在哪一栏，不能清空"));
            }
            continue;
        }
        field_value(name, kind, value)?;
        if let Some((_, Some(values))) = axis_fields(frame).into_iter().find(|(field, _)| field == name) {
            if !values.is_empty() && !value.as_str().is_some_and(|text| values.iter().any(|v| v == text)) {
                return Err(format!("字段 {name} 只能取 {}", values.join("、")));
            }
        }
    }
    Ok(())
}

fn has_source(frame: &Frame, card: &Card) -> bool {
    card.fields.keys().any(|name| usable(frame, name) == Ok(FieldKind::Ref))
}

/// FR-146: what the rules let a delegate do.
fn permitted(frame: &Frame, op: &Op) -> Result<(), String> {
    let who = match op {
        Op::Add { .. } | Op::Archive {} => frame.rules.add_card,
        Op::Link { .. } => frame.rules.add_link,
        Op::Update { .. } => frame.rules.edit_text,
        Op::Move { .. } => frame.rules.change_state,
        Op::Rules { .. } => Who::Anyone,
        Op::Source { change } => change.who(&frame.rules),
    };
    if who == Who::Human {
        return Err(format!("容器规则只允许人{}", op.label()));
    }
    Ok(())
}

fn live<'a>(frames: &'a Frames, id: &str) -> Result<&'a Card, String> {
    let frame = frames.get(card_frame(id)?).ok_or_else(|| format!("卡片 {id} 所在的容器不存在"))?;
    match frame.cards.iter().find(|card| card.id == id) {
        Some(card) if !card.archived => Ok(card),
        Some(_) => Err(format!("卡片 {id} 已归档")),
        None => Err(format!("卡片 {id} 不存在")),
    }
}

/// One write, checked and applied. `person` writes skip what binds only
/// delegates: transitions, the source rule and the done confirmation.
fn change(frames: &mut Frames, frame_id: &str, op: &Op, target: &str, at: &Mark, by: Option<&str>, person: bool) -> Result<String, String> {
    let frame = frames.get(frame_id).ok_or_else(|| format!("容器 {frame_id} 不存在"))?;
    let index = |frame: &Frame| frame.cards.iter().position(|card| card.id == target);
    let stamp = |card: &mut Card, keys: Vec<String>| {
        card.changed = at.clone();
        card.by = by.map(str::to_owned);
        if person {
            card.human.extend(keys);
        }
    };
    match op {
        Op::Add { title, body: text, state, fields, parent } => {
            if frame.mode == Mode::Bound {
                return Err("绑定容器的卡片跟随核心对象，不能新增自由卡片；要放自由卡片请用混合模式".into());
            }
            one_line(title, frame.limits.title_max, "标题")?;
            body(text, frame.limits.body_max)?;
            let state = match (state, frame.states.first()) {
                (Some(_), None) => return Err("这个容器没有状态".into()),
                (None, None) => None,
                (None, Some(_)) => frame.open_states().first().map(|state| (*state).clone()),
                (Some(state), Some(_)) => {
                    if !frame.states.contains(state) {
                        return Err(format!("状态 {state} 不在模板的状态集合里（{}）", frame.states.join("、")));
                    }
                    if !person && frame.done_states.contains(state) && frame.rules.done_needs_human {
                        return Err(format!("进入完成类状态 {state} 须由人确认；先加到其他状态，再用 frame move 申请"));
                    }
                    Some(state.clone())
                }
            };
            check_fields(frame, fields, false)?;
            if let Some((field, _)) = axis_fields(frame).into_iter().find(|(field, _)| !fields.contains_key(*field)) {
                return Err(format!("卡片须填字段 {field}，它决定卡片的位置"));
            }
            if let Some(parent) = parent {
                if !frame.layout.structure.iter().any(|kind| matches!(kind, Structure::Nesting | Structure::Tree)) {
                    return Err("这个容器的结构不是嵌套或树，卡片不能有上级".into());
                }
                if card_frame(parent)? != frame_id {
                    return Err("上级卡片须在同一个容器里".into());
                }
                live(frames, parent)?;
            }
            let id = format!("{frame_id}.{}", frame.cards.len() + 1);
            let mut card = Card {
                id: id.clone(), title: title.clone(), body: text.clone(), state, fields: fields.clone(), parent: parent.clone(),
                links: Vec::new(), human: BTreeSet::new(), archived: false, added: at.clone(), changed: at.clone(), by: None,
            };
            if !person && frame.rules.require_source && !has_source(frame, &card) {
                return Err("容器规则要求填来源：在 ref 类型的字段里写上出处".into());
            }
            let mut keys: Vec<String> = ["title"].into_iter().map(str::to_owned).collect();
            if !card.body.is_empty() {
                keys.push("body".into());
            }
            if card.state.is_some() {
                keys.push(frames::STATE_FIELD.into());
            }
            keys.extend(card.fields.keys().cloned());
            stamp(&mut card, keys);
            frames.get_mut(frame_id).expect("checked").cards.push(card);
            Ok(id)
        }
        Op::Update { title, body: text, fields } => {
            let card = live(frames, target)?.clone();
            if title.is_none() && text.is_none() && fields.is_empty() {
                return Err("没有要改的内容".into());
            }
            if let Some(title) = title {
                one_line(title, frame.limits.title_max, "标题")?;
            }
            if let Some(text) = text {
                body(text, frame.limits.body_max)?;
            }
            check_fields(frame, fields, true)?;
            let mut next = card.clone();
            let mut keys = Vec::new();
            if let Some(title) = title {
                next.title = title.clone();
                keys.push("title".to_owned());
            }
            if let Some(text) = text {
                next.body = text.clone();
                keys.push("body".to_owned());
            }
            for (name, value) in fields {
                if value.is_null() {
                    next.fields.remove(name);
                    next.human.remove(name);
                } else {
                    next.fields.insert(name.clone(), value.clone());
                    keys.push(name.clone());
                }
            }
            if !person && frame.rules.require_source && !has_source(frame, &next) {
                return Err("容器规则要求填来源，不能去掉 ref 字段".into());
            }
            stamp(&mut next, keys);
            let frame = frames.get_mut(frame_id).expect("checked");
            let at = index(frame).expect("live");
            frame.cards[at] = next;
            Ok(target.to_owned())
        }
        Op::Move { state } => {
            let card = live(frames, target)?;
            if frame.states.is_empty() {
                return Err("这个容器没有状态".into());
            }
            if !frame.states.contains(state) {
                return Err(format!("状态 {state} 不在模板的状态集合里（{}）", frame.states.join("、")));
            }
            let from = card.state.clone().unwrap_or_default();
            if from == *state {
                return Err(format!("卡片已经是 {state}"));
            }
            if !person {
                if let Some(allowed) = frame.rules.transitions.get(&from) {
                    if !allowed.contains(state) {
                        return Err(format!("容器规则不允许从 {from} 转到 {state}（只能转到 {}）", allowed.join("、")));
                    }
                }
            }
            let frame = frames.get_mut(frame_id).expect("checked");
            let at = index(frame).expect("live");
            let card = &mut frame.cards[at];
            card.state = Some(state.clone());
            stamp(card, vec![frames::STATE_FIELD.to_owned()]);
            Ok(target.to_owned())
        }
        Op::Link { to } => {
            let card = live(frames, target)?;
            if !frame.layout.structure.contains(&Structure::Relations) {
                return Err("这个容器的结构不是关系线，不能加连线".into());
            }
            live(frames, to)?;
            if to == target {
                return Err("卡片不能连向自己".into());
            }
            if card.links.contains(to) {
                return Err(format!("{target} 已经连到 {to}"));
            }
            let frame = frames.get_mut(frame_id).expect("checked");
            let at = index(frame).expect("live");
            let card = &mut frame.cards[at];
            card.links.push(to.clone());
            stamp(card, Vec::new());
            Ok(target.to_owned())
        }
        Op::Archive {} => {
            live(frames, target)?;
            if frame.cards.iter().any(|card| !card.archived && card.parent.as_deref() == Some(target)) {
                return Err("卡片下还有未归档的卡片，先归档它们".into());
            }
            let frame = frames.get_mut(frame_id).expect("checked");
            let at = index(frame).expect("live");
            let card = &mut frame.cards[at];
            card.archived = true;
            stamp(card, Vec::new());
            Ok(target.to_owned())
        }
        Op::Rules { .. } => Err("规则提案不改卡片".into()),
        Op::Source { .. } => Err("文本源容器的修改由写入者写进文件".into()),
    }
}

/// FR-150: the part of a delegate's update a person set splits off as a
/// suggestion. Returns what applies now and what waits.
fn split(card: &Card, op: &Op) -> (Option<Op>, Option<Op>) {
    match op {
        Op::Update { title, body, fields } => {
            let guarded = |key: &str| card.human.contains(key);
            let (mut now_fields, mut later_fields) = (BTreeMap::new(), BTreeMap::new());
            for (name, value) in fields {
                if guarded(name) { later_fields.insert(name.clone(), value.clone()) } else { now_fields.insert(name.clone(), value.clone()) };
            }
            let pick = |value: &Option<String>, key: &str, later: bool| value.clone().filter(|_| guarded(key) == later);
            let now = Op::Update { title: pick(title, "title", false), body: pick(body, "body", false), fields: now_fields };
            let later = Op::Update { title: pick(title, "title", true), body: pick(body, "body", true), fields: later_fields };
            let empty = |op: &Op| matches!(op, Op::Update { title: None, body: None, fields } if fields.is_empty());
            ((!empty(&now)).then_some(now), (!empty(&later)).then_some(later))
        }
        Op::Move { .. } if card.human.contains(frames::STATE_FIELD) => (None, Some(op.clone())),
        _ => (Some(op.clone()), None),
    }
}

fn wait(frame: &mut Frame, why: Why, target: &str, op: Op, from: Option<String>, by: Mark) {
    // A newer suggestion, confirmation or rule proposal from the same sender
    // on the same target replaces the older one; proposals all stay.
    if why != Why::Proposal {
        frame.pending.retain(|item| !(item.why == why && item.target == target && item.from == from && item.op.kind() == op.kind()));
    }
    frame.pending.push(Pending { id: by.seq, why, target: target.into(), op, from, by, note: None });
}

/// A delegate's write: rejected, waiting, or applied (FR-146–153).
fn delegate(frames: &mut Frames, frame_id: &str, op: Op, target: &str, at: &Mark, from: &str) -> Result<(), String> {
    let frame = frames.get(frame_id).ok_or_else(|| format!("容器 {frame_id} 不存在"))?;
    permitted(frame, &op)?;
    if let Op::Rules { rules } = &op {
        let proposed = frame.rules.merged(rules.clone());
        if proposed == frame.rules {
            return Err("提议的规则与现有规则相同".into());
        }
        template(frame)?.check_rules(&proposed)?;
        let op = Op::Rules { rules: full(&proposed) };
        wait(frames.get_mut(frame_id).expect("checked"), Why::Rules, target, op, Some(from.into()), at.clone());
        return Ok(());
    }
    // Every write is checked whole first: a non-compliant write is
    // rejected even where it would only have waited.
    change(&mut frames.clone(), frame_id, &op, target, at, Some(from), false)?;
    let frame = frames.get_mut(frame_id).expect("checked");
    if frame.set_by == SetBy::Human {
        wait(frame, Why::Proposal, target, op, Some(from.into()), at.clone());
        return Ok(());
    }
    let (now, later) = match frame.cards.iter().find(|card| card.id == target) {
        Some(card) => split(card, &op),
        None => (Some(op), None),
    };
    if let Some(later) = later {
        wait(frame, Why::Suggestion, target, later, Some(from.into()), at.clone());
    }
    let Some(now) = now else { return Ok(()) };
    if let Op::Move { state } = &now {
        if frame.rules.done_needs_human && frame.done_states.contains(state) {
            wait(frame, Why::Confirm, target, now, Some(from.into()), at.clone());
            return Ok(());
        }
    }
    change(frames, frame_id, &now, target, at, Some(from), false).map(|_| ())
}

fn full(rules: &frames::Rules) -> RulesInput {
    RulesInput {
        text: Some(rules.text.clone()), add_card: Some(rules.add_card), add_link: Some(rules.add_link),
        edit_text: Some(rules.edit_text), change_state: Some(rules.change_state), fields: Some(rules.fields.clone()),
        transitions: Some(rules.transitions.clone()), per_lane_max: rules.per_lane_max, require_source: Some(rules.require_source),
        done_needs_human: Some(rules.done_needs_human), update_until: rules.update_until.clone(),
        review_every: rules.review_every, callouts_off: Some(rules.callouts_off),
    }
}

fn template(frame: &Frame) -> Result<frames::Template, String> {
    frames::Template::parse(&frame.template.definition).map_err(|problems| format!("模板清单有问题：{}", problems.join("；")))
}

/// One core object as a bound frame shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: String,
    pub title: String,
    pub phase: Phase,
    /// FR-124: proven by checks, or confirmed by a person where only a
    /// person can complete it. A delegate's claim is never enough.
    pub done: bool,
}

/// Every object that has a delegation, with whether it is done.
pub fn objects(state: &Value, kits: &[&dyn Kit]) -> Result<Vec<Object>, String> {
    let log = delegations::read(state)?;
    Ok(delegations::delegations(state, kits)?
        .into_iter()
        .map(|item| {
            let latest = item.attempts.last();
            let confirmed = latest.is_some_and(|attempt| {
                item.human_can_complete
                    && item.checks.is_empty()
                    && log.objects.get(&item.id).is_some_and(|entry| {
                        entry.verdicts.iter().any(|verdict| verdict.attempt == attempt.id && matches!(verdict.data, Verdict::Confirmed(_)))
                    })
            });
            let done = item.phase == Phase::Done || latest.is_some_and(|attempt| attempt.done.is_some()) || confirmed;
            Object { id: item.id, title: item.title, phase: item.phase, done }
        })
        .collect())
}

/// FR-162: past `update_until`, a frame takes no more writes. `today` is a
/// UTC `YYYY-MM-DD`.
pub fn frozen(frame: &Frame, today: &str, done: impl Fn(&str) -> bool) -> bool {
    match &frame.rules.update_until {
        None => false,
        Some(Until::Date(day)) => today > day.as_str(),
        Some(Until::Object(id)) => done(id),
    }
}

fn check_open(state: &Value, kits: &[&dyn Kit], frame: &Frame, event: &Event) -> Result<(), String> {
    let today = event.t.get(..10).unwrap_or_default();
    let found = match &frame.rules.update_until {
        Some(Until::Object(_)) => objects(state, kits)?,
        _ => Vec::new(),
    };
    if frozen(frame, today, |id| found.iter().any(|object| object.id == id && object.done)) {
        return Err(format!("容器 {} 已过 update_until，冻结为只读记录，不再接受写入", frame.id));
    }
    Ok(())
}

/// A person's waiting-item decision, re-checked against the frame as it is
/// now; a stale item can only be ignored.
fn resolve(frames: &mut Frames, frame_id: &str, input: &Resolved, at: &Mark) -> Result<(), String> {
    let frame = frames.get_mut(frame_id).ok_or("容器不存在")?;
    let index = frame.pending.iter().position(|item| item.id == input.pending).ok_or("待处理项不存在或已处理")?;
    let item = frame.pending.remove(index);
    if matches!(item.op, Op::Source { .. }) {
        return crate::source_frames::resolve(frame, item, input.accept);
    }
    if !input.accept {
        return Ok(());
    }
    match &item.op {
        Op::Rules { rules } => {
            let rules = frame.rules.merged(rules.clone());
            template(frame)?.check_rules(&rules).map_err(|e| format!("规则提案已不适用：{e}"))?;
            frame.rules = rules;
            frame.rules_changed = Some(at.clone());
        }
        op => {
            change(frames, frame_id, op, &item.target, at, None, true).map_err(|e| format!("提案已不适用，只能忽略：{e}"))?;
        }
    }
    Ok(())
}

/// FR-149: only cards the frame shows can be pinned, inside the frame's
/// largest size; a proposal is not a card yet.
fn pin(frame: &mut Frame, input: &Pinned, is_object: impl Fn(&str) -> bool) -> Result<(), String> {
    if input.pins.is_empty() || input.pins.len() > PINS_MAX {
        return Err(format!("一次固定 1 到 {PINS_MAX} 张卡片"));
    }
    let mut pins = frame.pins.clone();
    for (id, place) in &input.pins {
        // A text-source frame's cards are its file's elements, by key.
        let free = frame.cards.iter().any(|card| &card.id == id && !card.archived)
            || (frame.source.is_some() && !id.trim().is_empty() && id.chars().count() <= crate::source_frames::KEY_CHARS && !id.chars().any(char::is_control));
        let bound = frame.mode != Mode::Free && valid_id(id).is_ok()
            && if frame.objects.is_empty() { is_object(id) } else { frame.objects.contains(id) };
        if !free && !bound {
            return Err(format!("{id} 不是容器 {} 里的卡片", frame.id));
        }
        match place {
            Some(place) => {
                if !(0..=frames::MAX_SIZE).contains(&place.x) || !(0..=frames::MAX_SIZE).contains(&place.y) {
                    return Err(format!("{id} 的位置须在容器内（0 到 {}）", frames::MAX_SIZE));
                }
                pins.insert(id.clone(), *place);
            }
            None => {
                pins.remove(id);
            }
        }
    }
    if pins == frame.pins {
        return Err("位置没有变化".into());
    }
    frame.pins = pins;
    Ok(())
}

fn person(event: &Event, layer: &str) -> Result<(), String> {
    if !human_actor(&event.actor) || event.layer != layer || !event.guards.is_empty() {
        return Err(format!("{} 只能由人在 App 中发出", event.kind));
    }
    Ok(())
}

/// The op a write carries and who sent it: a person's op as is, or a
/// delegate's whole envelope.
fn written(event: &Event) -> Result<(Op, Option<String>), String> {
    if human_actor(&event.actor) {
        if event.layer != LAYER || !event.guards.is_empty() {
            return Err(format!("{} 的层或 fences 无效", event.kind));
        }
        let op: Op = serde_json::from_value(event.data.clone()).map_err(|e| format!("卡片操作无效：{e}"))?;
        if matches!(op, Op::Rules { .. }) {
            return Err("人在 App 里直接改规则，不提规则提案".into());
        }
        return Ok((op, None));
    }
    check_signer(&event.actor)?;
    if event.layer != LAYER {
        return Err(format!("{} 的层无效", event.kind));
    }
    let envelope: Envelope = serde_json::from_value(event.data.clone()).map_err(|e| format!("{} 信封无效：{e}", event.kind))?;
    if envelope.verb != Verb::Card || envelope.target != event.obj {
        return Err(format!("{} 的动词或对象与信封不符", event.kind));
    }
    envelope.check()?;
    let op = envelope.card()?;
    let frame = frame_of(&op, &event.obj)?;
    let lease = lease_name(frame);
    if !matches!(event.guards.as_slice(), [fence] if fence.name == lease && fence.holder == envelope.from.id) {
        return Err(format!("写卡片须持有容器租约 {lease}"));
    }
    Ok((op, Some(envelope.from.id)))
}

/// Card writes, rule changes and resolutions. `Ok(false)` for other events.
pub fn apply(state: &mut Value, event: &Event, kits: &[&dyn Kit]) -> Result<bool, String> {
    let kind = event.kind.as_str();
    if !CARD_KINDS.contains(&kind) && kind != RULES_CHANGED && kind != RESOLVED && kind != PINNED {
        return Ok(false);
    }
    let mut all = frames::read(state)?;
    let at = mark(event);
    match kind {
        RULES_CHANGED => {
            person(event, frames::LAYER)?;
            let input: RulesChanged = serde_json::from_value(event.data.clone()).map_err(|e| format!("规则正文无效：{e}"))?;
            let frame = all.get_mut(&event.obj).ok_or("容器不存在")?;
            template(frame)?.check_rules(&input.rules)?;
            if input.rules == frame.rules {
                return Err("规则没有变化".into());
            }
            frame.rules = input.rules;
            frame.rules_changed = Some(at);
        }
        RESOLVED => {
            person(event, frames::LAYER)?;
            let input: Resolved = serde_json::from_value(event.data.clone()).map_err(|e| format!("处理正文无效：{e}"))?;
            let frame = all.get(&event.obj).ok_or("容器不存在")?;
            if input.accept {
                check_open(state, kits, frame, event)?;
            }
            resolve(&mut all, &event.obj, &input, &at)?;
        }
        PINNED => {
            person(event, frames::LAYER)?;
            let input: Pinned = serde_json::from_value(event.data.clone()).map_err(|e| format!("固定位置正文无效：{e}"))?;
            let frame = all.get(&event.obj).ok_or("容器不存在")?;
            check_open(state, kits, frame, event)?;
            let found = match (frame.mode, frame.objects.is_empty()) {
                (Mode::Free, _) | (_, false) => Vec::new(),
                _ => objects(state, kits)?,
            };
            pin(all.get_mut(&event.obj).ok_or("容器不存在")?, &input, |id| found.iter().any(|object| object.id == id))?;
        }
        _ => {
            let (op, from) = written(event)?;
            if op.kind() != kind {
                return Err(format!("{kind} 与操作 {} 不符", op.kind()));
            }
            let frame_id = frame_of(&op, &event.obj)?.to_owned();
            let frame = all.get(&frame_id).ok_or_else(|| format!("容器 {frame_id} 不存在"))?;
            check_open(state, kits, frame, event)?;
            match from {
                _ if crate::source_frames::admit(&mut all, &frame_id, &op, &at, from.as_deref())? => {}
                Some(from) => delegate(&mut all, &frame_id, op, &event.obj, &at, &from)?,
                None => {
                    change(&mut all, &frame_id, &op, &event.obj, &at, None, true)?;
                }
            }
        }
    }
    state[GROUP] = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    Ok(true)
}

/// FR-151: the lane a card is in, by the frame's category axes.
pub fn lane(frame: &Frame, card: &Card) -> Vec<String> {
    [&frame.layout.x, &frame.layout.y]
        .into_iter()
        .filter_map(|axis| match axis {
            Axis::Category { field, .. } if field == frames::STATE_FIELD => Some(card.state.clone().unwrap_or_default()),
            Axis::Category { field, .. } => Some(card.fields.get(field).and_then(Value::as_str).unwrap_or_default().to_owned()),
            _ => None,
        })
        .collect()
}

/// FR-151: past the lane limit a write still lands, and the writer is told
/// to merge or archive by the rules.
pub fn crowded(frame: &Frame, card_id: &str) -> Option<String> {
    let max = frame.rules.per_lane_max?;
    let card = frame.cards.iter().find(|card| card.id == card_id && !card.archived)?;
    let lane = lane(frame, card);
    let count = frame.cards.iter().filter(|other| !other.archived && self::lane(frame, other) == lane).count();
    let name = if lane.iter().all(String::is_empty) { "容器".to_owned() } else { format!("「{}」", lane.join(" / ")) };
    (count > max as usize).then(|| format!("{name}已有 {count} 张卡片，超过上限 {max}；请按容器规则合并或归档（towermd frame archive <卡片>）"))
}

/// What one accepted event did to its frame, for the writer's receipt.
pub fn outcome(state: &Value, frame_id: &str, seq: u64) -> Result<Value, String> {
    let all = frames::read(state)?;
    let frame = all.get(frame_id).ok_or("容器不存在")?;
    let waiting: Vec<Value> = frame.pending.iter().filter(|item| item.id == seq)
        .map(|item| serde_json::json!({"id": item.id, "why": item.why, "target": item.target})).collect();
    let card = frame.cards.iter().find(|card| card.changed.seq == seq).map(|card| card.id.clone());
    let status = match (&card, waiting.first().and_then(|item| item["why"].as_str())) {
        (Some(_), Some(_)) => "partly_suggested",
        (Some(_), None) => "applied",
        (None, Some("proposal")) => "proposed",
        (None, Some("suggestion")) => "suggested",
        (None, Some("confirm")) => "awaiting_confirmation",
        (None, Some("write")) => "queued",
        (None, Some(_)) => "proposed",
        (None, None) => "applied",
    };
    let hint = card.as_deref().and_then(|id| crowded(frame, id));
    Ok(serde_json::json!({"status": status, "card": card, "pending": waiting, "hint": hint}))
}

/// A bound card: the object's state in this frame's states.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    pub id: String,
    pub title: String,
    pub state: String,
    pub phase: Phase,
    pub done: bool,
}

/// FR-148, FR-152: an object's phase in the frame's states. Not done maps
/// to the open states in order; only a done object reaches a done state.
pub fn bound_state(frame: &Frame, object: &Object) -> Option<String> {
    if object.done {
        return frame.done_states.first().cloned();
    }
    let open = frame.open_states();
    let step = match object.phase {
        Phase::Clarifying | Phase::Proposed | Phase::Ready => 0,
        Phase::Running => 1,
        Phase::ClaimedDone | Phase::Done => 2,
    };
    open.get(step.min(open.len().saturating_sub(1))).map(|state| (*state).clone())
}

/// A frame as the App and `frame show` read it: the stored frame plus its
/// bound cards and whether it is frozen. Derived, never stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    #[serde(flatten)]
    pub frame: Frame,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bound: Vec<Bound>,
    pub frozen: bool,
    /// FR-160: derived, never stored; empty when the rules turn them off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callouts: Vec<crate::callouts::Callout>,
}

pub fn view(state: &Value, kits: &[&dyn Kit], today: &str) -> Result<Vec<View>, String> {
    let all = frames::read(state)?;
    if all.is_empty() {
        return Ok(Vec::new());
    }
    let found = objects(state, kits)?;
    let questions = crate::question_projection::read(state)?;
    Ok(all.into_values().map(|frame| {
        let bound = match frame.mode {
            Mode::Free => Vec::new(),
            Mode::Bound | Mode::Mixed => found.iter()
                .filter(|object| frame.objects.is_empty() || frame.objects.contains(&object.id))
                .filter_map(|object| Some(Bound {
                    id: object.id.clone(), title: tower_api::clip(&object.title, frames::CARD_TITLE_MAX as usize),
                    state: bound_state(&frame, object)?, phase: object.phase, done: object.done,
                }))
                .collect(),
        };
        let frozen = frozen(&frame, today, |id| found.iter().any(|object| object.id == id && object.done));
        let shown: Vec<String> = bound.iter().map(|card| card.id.clone()).collect();
        let callouts = crate::callouts::for_frame(&questions, &frame, &shown);
        View { frame, bound, frozen, callouts }
    }).collect())
}

/// The envelope check for the `card` verb: the body parses and names the
/// right kind of target.
pub fn check(op: &Op, target: &str) -> Result<(), String> {
    valid_id(target)?;
    frame_of(op, target)?;
    if let Op::Link { to } = op {
        card_frame(to)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
