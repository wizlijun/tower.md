//! Frames: a template placed on the canvas (core spec section 8, FR-141–148,
//! FR-153, FR-156).
//!
//! A template is an extension manifest with `type: template`. Its layout is
//! three settings the core alone implements: two axes and a structure
//! (FR-141). A person places a template on the canvas; that creates a frame
//! (`frame.created`, signed `human:<id>`, sent from the App only). The frame
//! freezes the template's whole definition, unknown keys included, so replay
//! never depends on which templates are installed now. Only a person moves
//! or resizes a frame (`frame.placed`, FR-145).
//!
//! Each frame carries its own rules (FR-146): up to 300 characters of
//! requirements plus structured constraints, defaulting from the template.
//! Frames live in the hub and are archived to `state/` by snapshot; the
//! `Frame` group is created on first use, so older projections stay
//! byte-identical.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    delegation_projection::Mark,
    hub::Event,
    question_model::{human_actor, valid_id},
};

pub const GROUP: &str = "Frame";
/// The registered object type (`crate::types::TYPES`).
pub const TYPE: &str = "Frame";
/// A human-set minor change: frames and their rules only change by a
/// person, but they are not slow-layer commitments.
pub const LAYER: &str = "L3";
pub const CREATED: &str = "frame.created";
pub const PLACED: &str = "frame.placed";
/// Frame IDs read as `C-<name>` (FR-154).
pub const ID_PREFIX: &str = "C-";
pub const TEMPLATE_KIND: &str = "template";
/// FR-146: the rules' natural-language part.
pub const RULES_CHARS: usize = 300;
pub const TITLE_CHARS: usize = 60;
/// FR-151: card text limits a template may only tighten.
pub const CARD_TITLE_MAX: u32 = 30;
pub const CARD_BODY_MAX: u32 = 100;
pub const LANE_MAX: u32 = 999;
/// FR-156: the longest `review_every`, in days (`quiet::MAX_DAYS`).
pub const REVIEW_DAYS_MAX: u32 = crate::quiet::MAX_DAYS as u32;
/// Frame geometry in canvas units; JSON Canvas uses integers.
pub const MIN_SIZE: i64 = 160;
pub const MAX_SIZE: i64 = 20_000;
pub const COORD_LIMIT: i64 = 1_000_000;
/// The card's own fields; a template's `card_fields` may not reuse them.
pub const BUILT_IN_FIELDS: [&str; 3] = ["title", "body", "state"];
/// The category field that takes its values from the template's states.
pub const STATE_FIELD: &str = "state";
pub const ORDER_ADDED: &str = "added";

/// One axis (FR-141): by category, by time, by order of arrival, or none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    None,
    /// Lanes or rows by a field. `values` are the lanes in order; on the
    /// `state` field they default to the template's states. Empty values
    /// mean the lanes come from the cards (FR-149).
    Category { field: String, values: Vec<String> },
    /// Ordered by a `date` card field.
    Time { field: String },
    /// Ordered by when cards were added.
    Order,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Structure {
    Nesting,
    Relations,
    /// A way to show nesting.
    Tree,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub x: Axis,
    pub y: Axis,
    /// Empty means "none"; sorted and unique.
    pub structure: Vec<Structure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Date,
    Ref,
    Number,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub title_max: u32,
    pub body_max: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_lane_max: Option<u32>,
}

/// FR-153: who a frame belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetBy {
    Human,
    Delegate,
}

/// FR-148: bound cards project core objects; free cards are written by
/// delegates and sources; a mixed frame holds both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Bound,
    Free,
    Mixed,
}

/// FR-146: who may do one kind of write. A person always may.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    Human,
    Anyone,
}

/// Transitions: a listed state may only move to the listed states; states
/// not listed move freely (FR-142).
pub type Transitions = BTreeMap<String, Vec<String>>;

/// A `type: template` manifest, checked. Parse it from the manifest as
/// written; keep that whole manifest as the definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Template {
    pub id: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub layout: Layout,
    pub states: Vec<String>,
    pub transitions: Transitions,
    pub done_states: Vec<String>,
    pub card_fields: BTreeMap<String, FieldKind>,
    pub limits: Limits,
    pub default_set_by: SetBy,
    pub export: Vec<String>,
}

/// FR-146: the frame's rules. Every field is explicit in the event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub text: String,
    pub add_card: Who,
    pub add_link: Who,
    pub edit_text: Who,
    pub change_state: Who,
    /// Card fields in use; a subset of the template's.
    pub fields: Vec<String>,
    /// No looser than the template's.
    pub transitions: Transitions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_lane_max: Option<u32>,
    pub require_source: bool,
    /// FR-152: a free card entering a done state needs a person.
    pub done_needs_human: bool,
    /// FR-162: after this the frame is a read-only record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_until: Option<Until>,
    /// FR-156: days without activity after which the frame resurfaces
    /// (FR-135, FR-136), like `quiet.check_every` for one object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_every: Option<u32>,
    /// FR-161: no callouts beside this frame's cards.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub callouts_off: bool,
}

/// FR-162: keep updating until an object is done or a day has passed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Until {
    /// `YYYY-MM-DD`, inclusive, in UTC.
    Date(String),
    /// A core object that has a delegation; done means FR-124 done.
    Object(String),
}

/// The rules a person may set when creating a frame; the rest default from
/// the template.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesInput {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub add_card: Option<Who>,
    #[serde(default)]
    pub add_link: Option<Who>,
    #[serde(default)]
    pub edit_text: Option<Who>,
    #[serde(default)]
    pub change_state: Option<Who>,
    #[serde(default)]
    pub fields: Option<Vec<String>>,
    #[serde(default)]
    pub transitions: Option<Transitions>,
    #[serde(default)]
    pub per_lane_max: Option<u32>,
    #[serde(default)]
    pub require_source: Option<bool>,
    #[serde(default)]
    pub done_needs_human: Option<bool>,
    #[serde(default)]
    pub update_until: Option<Until>,
    #[serde(default)]
    pub review_every: Option<u32>,
    #[serde(default)]
    pub callouts_off: Option<bool>,
}

impl Rules {
    /// The rules with a person's or a proposal's choices laid over them.
    pub fn merged(&self, input: RulesInput) -> Rules {
        Rules {
            text: input.text.unwrap_or_else(|| self.text.clone()),
            add_card: input.add_card.unwrap_or(self.add_card),
            add_link: input.add_link.unwrap_or(self.add_link),
            edit_text: input.edit_text.unwrap_or(self.edit_text),
            change_state: input.change_state.unwrap_or(self.change_state),
            fields: input.fields.unwrap_or_else(|| self.fields.clone()),
            transitions: input.transitions.unwrap_or_else(|| self.transitions.clone()),
            per_lane_max: input.per_lane_max.or(self.per_lane_max),
            require_source: input.require_source.unwrap_or(self.require_source),
            done_needs_human: input.done_needs_human.unwrap_or(self.done_needs_human),
            update_until: input.update_until.or_else(|| self.update_until.clone()),
            review_every: input.review_every.or(self.review_every),
            callouts_off: input.callouts_off.unwrap_or(self.callouts_off),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// The template as frozen into the frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frozen {
    pub id: String,
    pub version: String,
    /// The manifest as written, unknown keys included.
    pub definition: Value,
}

/// `frame.created`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Created {
    pub title: String,
    pub template: Frozen,
    pub set_by: SetBy,
    pub mode: Mode,
    pub rules: Rules,
    pub place: Place,
    /// Bound and mixed frames: the core objects shown as bound cards; empty
    /// means every object that has a delegation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<String>,
    /// FR-144: cards and lines read from pasted text, a person's own writes
    /// that land with the frame. Only free frames start with cards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cards: Vec<crate::cards::Write>,
}

/// Cards and lines one `frame.created` may start with.
pub const IMPORT_MAX: usize = 200;

/// `frame.placed`: a person moved or resized the frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placed {
    pub place: Place,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub id: String,
    pub title: String,
    pub template: Frozen,
    /// Read from the frozen definition when the frame was created.
    pub layout: Layout,
    pub states: Vec<String>,
    pub done_states: Vec<String>,
    pub limits: Limits,
    pub set_by: SetBy,
    pub mode: Mode,
    pub rules: Rules,
    pub place: Place,
    pub created: Mark,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placed: Option<Mark>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<String>,
    /// Free cards in the order they were added (H2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cards: Vec<crate::cards::Card>,
    /// Waiting for a person: proposals, suggestions, confirmations and rule
    /// proposals. Nothing here ever times out into effect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<crate::cards::Pending>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules_changed: Option<Mark>,
    /// FR-149: cards a person dragged, in frame coordinates. Layout never
    /// moves them; every other card's place is derived (`card.pinned`, H3).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pins: BTreeMap<String, Pin>,
    /// FR-154: the last approved write of this frame into `tower.canvas`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized: Option<crate::frame_canvas::Record>,
}

/// Where a person put a card inside its frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub x: i64,
    pub y: i64,
}

impl Frame {
    /// States a free card may enter without being done.
    pub fn open_states(&self) -> Vec<&String> {
        self.states.iter().filter(|state| !self.done_states.contains(state)).collect()
    }
}

pub type Frames = BTreeMap<String, Frame>;

pub fn read(state: &Value) -> Result<Frames, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(Frames::new()),
    }
}

fn one_line(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
}

fn field_name(name: &str) -> bool {
    name.len() <= 32
        && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn strings(value: &Value, key: &str, problems: &mut Vec<String>) -> Vec<String> {
    match value {
        Value::Null => Vec::new(),
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(text) if one_line(text, TITLE_CHARS) && !out.iter().any(|seen| seen == text) => out.push(text.to_owned()),
                    _ => {
                        problems.push(format!("{key} 须是不重复的非空单行字符串"));
                        return Vec::new();
                    }
                }
            }
            out
        }
        _ => {
            problems.push(format!("{key} 须是列表"));
            Vec::new()
        }
    }
}

fn only_keys(map: &Map<String, Value>, allowed: &[&str], key: &str, problems: &mut Vec<String>) {
    for name in map.keys() {
        if !allowed.contains(&name.as_str()) {
            problems.push(format!("{key} 中有不认识的键 {name}"));
        }
    }
}

fn axis(value: Option<&Value>, key: &str, problems: &mut Vec<String>) -> Axis {
    let value = match value {
        None | Some(Value::Null) => return Axis::None,
        Some(Value::String(text)) if text == "none" => return Axis::None,
        Some(Value::Object(map)) => map,
        Some(_) => {
            problems.push(format!("{key} 须是 none 或映射"));
            return Axis::None;
        }
    };
    let field = |name: &str, problems: &mut Vec<String>| match value.get(name).and_then(Value::as_str) {
        Some(field) if field_name(field) => field.to_owned(),
        _ => {
            problems.push(format!("{key}.{name} 须是字段名"));
            String::new()
        }
    };
    if value.contains_key("category") {
        only_keys(value, &["category", "values"], key, problems);
        let field = field("category", problems);
        let values = strings(value.get("values").unwrap_or(&Value::Null), &format!("{key}.values"), problems);
        Axis::Category { field, values }
    } else if value.contains_key("time") {
        only_keys(value, &["time"], key, problems);
        Axis::Time { field: field("time", problems) }
    } else if value.contains_key("order") {
        only_keys(value, &["order"], key, problems);
        if value.get("order").and_then(Value::as_str) != Some(ORDER_ADDED) {
            problems.push(format!("{key}.order 只能是 {ORDER_ADDED}"));
        }
        Axis::Order
    } else {
        problems.push(format!("{key} 须是 category、time、order 或 none"));
        Axis::None
    }
}

fn structure(value: Option<&Value>, problems: &mut Vec<String>) -> Vec<Structure> {
    let items: Vec<&Value> = match value {
        None | Some(Value::Null) => return Vec::new(),
        Some(Value::String(text)) if text == "none" => return Vec::new(),
        Some(item @ Value::String(_)) => vec![item],
        Some(Value::Array(items)) => items.iter().collect(),
        Some(_) => {
            problems.push("layout.structure 须是 none、nesting、relations、tree 或它们的列表".into());
            return Vec::new();
        }
    };
    let mut out = BTreeSet::new();
    for item in items {
        match serde_json::from_value::<Structure>(item.clone()) {
            Ok(kind) => {
                out.insert(kind);
            }
            Err(_) => problems.push(format!("layout.structure 不认识 {item}")),
        }
    }
    out.into_iter().collect()
}

fn transitions(value: Option<&Value>, states: &[String], key: &str, problems: &mut Vec<String>) -> Transitions {
    let map = match value {
        None | Some(Value::Null) => return Transitions::new(),
        Some(Value::Object(map)) => map,
        Some(_) => {
            problems.push(format!("{key} 须是映射"));
            return Transitions::new();
        }
    };
    let mut out = Transitions::new();
    for (from, to) in map {
        if !states.contains(from) {
            problems.push(format!("{key} 中的 {from} 不在状态集合里"));
        }
        let to = strings(to, &format!("{key}.{from}"), problems);
        if let Some(state) = to.iter().find(|state| !states.contains(state)) {
            problems.push(format!("{key}.{from} 中的 {state} 不在状态集合里"));
        }
        out.insert(from.clone(), to);
    }
    out
}

fn limit(map: &Map<String, Value>, key: &str, min: u32, max: u32, problems: &mut Vec<String>) -> Option<u32> {
    let value = map.get(key)?;
    match value.as_u64() {
        Some(n) if n >= u64::from(min) && n <= u64::from(max) => Some(n as u32),
        _ => {
            problems.push(format!("limits.{key} 须是 {min} 到 {max} 的整数"));
            None
        }
    }
}

impl Template {
    /// Checks a manifest as written. Unknown top-level keys are kept in the
    /// definition and ignored; any problem with what the core reads fails
    /// closed: the template cannot be placed.
    pub fn parse(definition: &Value) -> Result<Self, Vec<String>> {
        let mut problems = Vec::new();
        let Some(map) = definition.as_object() else {
            return Err(vec!["模板清单须是映射".into()]);
        };
        let text = |key: &str, problems: &mut Vec<String>| match map.get(key).and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => Some(value.to_owned()),
            _ => {
                problems.push(format!("缺少 {key}"));
                None
            }
        };
        let id = text("id", &mut problems).unwrap_or_default();
        if !id.is_empty() && !crate::extensions::valid_id(&id) {
            problems.push("模板 id 只能含小写字母、数字、_ 和 -".into());
        }
        let version = text("version", &mut problems).unwrap_or_default();
        if map.get("type").and_then(Value::as_str) != Some(TEMPLATE_KIND) {
            problems.push(format!("type 须是 {TEMPLATE_KIND}"));
        }
        let title = match map.get("title") {
            None | Some(Value::Null) => None,
            Some(Value::String(title)) if one_line(title, TITLE_CHARS) => Some(title.clone()),
            Some(_) => {
                problems.push(format!("title 须是不超过 {TITLE_CHARS} 字的单行字符串"));
                None
            }
        };
        let states = strings(map.get("states").unwrap_or(&Value::Null), "states", &mut problems);
        let transitions = transitions(map.get("transitions"), &states, "transitions", &mut problems);
        let done_states = strings(map.get("done_states").unwrap_or(&Value::Null), "done_states", &mut problems);
        if let Some(state) = done_states.iter().find(|state| !states.contains(state)) {
            problems.push(format!("done_states 中的 {state} 不在状态集合里"));
        }
        let mut card_fields = BTreeMap::new();
        match map.get("card_fields") {
            None | Some(Value::Null) => {}
            Some(Value::Object(fields)) => {
                for (name, kind) in fields {
                    if !field_name(name) || BUILT_IN_FIELDS.contains(&name.as_str()) {
                        problems.push(format!("card_fields 的字段名 {name} 无效或与卡片自带字段重名"));
                        continue;
                    }
                    match serde_json::from_value::<FieldKind>(kind.clone()) {
                        Ok(kind) => {
                            card_fields.insert(name.clone(), kind);
                        }
                        Err(_) => problems.push(format!("card_fields.{name} 须是 text、date、ref 或 number")),
                    }
                }
            }
            Some(_) => problems.push("card_fields 须是映射".into()),
        }
        let layout = match map.get("layout") {
            Some(Value::Object(layout)) => {
                only_keys(layout, &["x", "y", "structure"], "layout", &mut problems);
                let mut x = axis(layout.get("x"), "layout.x", &mut problems);
                let mut y = axis(layout.get("y"), "layout.y", &mut problems);
                for axis in [&mut x, &mut y] {
                    check_axis(axis, &states, &card_fields, &mut problems);
                }
                let same = match (&x, &y) {
                    (Axis::Category { field: a, .. }, Axis::Category { field: b, .. }) | (Axis::Time { field: a }, Axis::Time { field: b }) => a == b,
                    (Axis::Order, Axis::Order) => true,
                    _ => false,
                };
                if same {
                    problems.push("横轴与纵轴不能用同一个字段".into());
                }
                Layout { x, y, structure: structure(layout.get("structure"), &mut problems) }
            }
            _ => {
                problems.push("缺少 layout 映射".into());
                Layout { x: Axis::None, y: Axis::None, structure: Vec::new() }
            }
        };
        let limits = match map.get("limits") {
            None | Some(Value::Null) => Limits { title_max: CARD_TITLE_MAX, body_max: CARD_BODY_MAX, per_lane_max: None },
            Some(Value::Object(limits)) => {
                only_keys(limits, &["title_max", "body_max", "per_lane_max"], "limits", &mut problems);
                Limits {
                    title_max: limit(limits, "title_max", 1, CARD_TITLE_MAX, &mut problems).unwrap_or(CARD_TITLE_MAX),
                    body_max: limit(limits, "body_max", 0, CARD_BODY_MAX, &mut problems).unwrap_or(CARD_BODY_MAX),
                    per_lane_max: limit(limits, "per_lane_max", 1, LANE_MAX, &mut problems),
                }
            }
            Some(_) => {
                problems.push("limits 须是映射".into());
                Limits { title_max: CARD_TITLE_MAX, body_max: CARD_BODY_MAX, per_lane_max: None }
            }
        };
        // Safety first: a template that does not say belongs to a person.
        let default_set_by = match map.get("default_set_by") {
            None | Some(Value::Null) => SetBy::Human,
            Some(value) => serde_json::from_value(value.clone()).unwrap_or_else(|_| {
                problems.push("default_set_by 须是 human 或 delegate".into());
                SetBy::Human
            }),
        };
        let export = strings(map.get("export").unwrap_or(&Value::Null), "export", &mut problems);
        if problems.is_empty() {
            Ok(Self { id, version, title, layout, states, transitions, done_states, card_fields, limits, default_set_by, export })
        } else {
            Err(problems)
        }
    }

    /// A manifest file's text: the definition as written and the template,
    /// or every problem.
    pub fn from_yaml(text: &str) -> (Value, Result<Self, Vec<String>>) {
        match serde_yaml::from_str::<Value>(text) {
            Ok(definition) => {
                let template = Self::parse(&definition);
                (definition, template)
            }
            Err(error) => (Value::Null, Err(vec![format!("清单不是合法 YAML：{error}")])),
        }
    }

    /// `<id>@<version>`, as FR-154 writes it.
    pub fn reference(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    /// FR-146: the rules a frame starts with, the person's choices applied.
    pub fn rules(&self, input: RulesInput) -> Result<Rules, String> {
        let rules = Rules {
            text: input.text.unwrap_or_default(),
            add_card: input.add_card.unwrap_or(Who::Anyone),
            add_link: input.add_link.unwrap_or(Who::Anyone),
            edit_text: input.edit_text.unwrap_or(Who::Anyone),
            change_state: input.change_state.unwrap_or(Who::Anyone),
            fields: input.fields.unwrap_or_else(|| self.card_fields.keys().cloned().collect()),
            transitions: input.transitions.unwrap_or_else(|| self.transitions.clone()),
            per_lane_max: input.per_lane_max.or(self.limits.per_lane_max),
            require_source: input.require_source.unwrap_or(false),
            done_needs_human: input.done_needs_human.unwrap_or(true),
            update_until: input.update_until,
            review_every: input.review_every,
            callouts_off: input.callouts_off.unwrap_or(false),
        };
        self.check_rules(&rules)?;
        Ok(rules)
    }

    /// Rules may only narrow what the template allows.
    pub fn check_rules(&self, rules: &Rules) -> Result<(), String> {
        if rules.text.chars().count() > RULES_CHARS || rules.text.chars().any(|c| c.is_control() && c != '\n') {
            return Err(format!("规则要求不超过 {RULES_CHARS} 字"));
        }
        let mut seen = BTreeSet::new();
        if let Some(field) = rules.fields.iter().find(|field| !self.card_fields.contains_key(*field) || !seen.insert(*field)) {
            return Err(format!("规则里的字段 {field} 不是模板的卡片字段或重复"));
        }
        let mut problems = Vec::new();
        let value = serde_json::to_value(&rules.transitions).map_err(|e| e.to_string())?;
        let transitions = transitions(Some(&value), &self.states, "规则的状态转换", &mut problems);
        if let Some(problem) = problems.into_iter().next() {
            return Err(problem);
        }
        for (from, allowed) in &self.transitions {
            let narrowed = transitions.get(from).is_some_and(|to| to.iter().all(|state| allowed.contains(state)));
            if !narrowed {
                return Err(format!("规则不能放宽模板对 {from} 的状态转换"));
            }
        }
        if let Some(max) = rules.per_lane_max {
            if max == 0 || max > self.limits.per_lane_max.unwrap_or(LANE_MAX) {
                return Err("规则的每栏上限须为正数，且不超过模板的上限".into());
            }
        }
        if rules.require_source && !rules.fields.iter().any(|field| self.card_fields.get(field) == Some(&FieldKind::Ref)) {
            return Err("必须填来源时，规则里要有一个 ref 字段".into());
        }
        match &rules.update_until {
            Some(Until::Date(day)) if chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err() => {
                return Err("update_until 的日期须是 YYYY-MM-DD".into());
            }
            Some(Until::Object(id)) => valid_id(id).map_err(|e| format!("update_until 的对象无效：{e}"))?,
            _ => {}
        }
        if rules.review_every.is_some_and(|days| !(1..=REVIEW_DAYS_MAX).contains(&days)) {
            return Err(format!("复查间隔须是 1 到 {REVIEW_DAYS_MAX} 天"));
        }
        Ok(())
    }
}

fn check_axis(axis: &mut Axis, states: &[String], fields: &BTreeMap<String, FieldKind>, problems: &mut Vec<String>) {
    match axis {
        Axis::Category { field, values } if field == STATE_FIELD => {
            if states.is_empty() {
                problems.push("按状态分栏的模板须声明 states".into());
            } else if values.is_empty() {
                *values = states.to_vec();
            } else if let Some(value) = values.iter().find(|value| !states.contains(value)) {
                problems.push(format!("按状态分栏的值 {value} 不在状态集合里"));
            }
        }
        Axis::Category { field, .. } if !field.is_empty() && !fields.contains_key(field) => {
            problems.push(format!("分类字段 {field} 不是卡片字段"));
        }
        Axis::Time { field } if !field.is_empty() && fields.get(field) != Some(&FieldKind::Date) => {
            problems.push(format!("时间轴字段 {field} 须是 date 类型的卡片字段"));
        }
        _ => {}
    }
}

pub fn check_id(id: &str) -> Result<(), String> {
    valid_id(id)?;
    if id.len() <= ID_PREFIX.len() || !id.starts_with(ID_PREFIX) {
        return Err(format!("容器ID须以 {ID_PREFIX} 开头"));
    }
    Ok(())
}

pub fn check_title(title: &str) -> Result<(), String> {
    if !one_line(title, TITLE_CHARS) {
        return Err(format!("容器标题须非空、不换行、不超过 {TITLE_CHARS} 字"));
    }
    Ok(())
}

pub fn check_place(place: &Place) -> Result<(), String> {
    let within = |value: i64| (-COORD_LIMIT..=COORD_LIMIT).contains(&value);
    let sized = |value: i64| (MIN_SIZE..=MAX_SIZE).contains(&value);
    if !within(place.x) || !within(place.y) || !sized(place.width) || !sized(place.height) {
        return Err(format!("容器位置超出画布，或尺寸不在 {MIN_SIZE} 到 {MAX_SIZE} 之间"));
    }
    Ok(())
}

/// Everything `frame.created` must satisfy, given who signed it. The daemon
/// runs this before appending; the reducer runs it again on replay.
pub fn check_created(created: &Created) -> Result<Template, String> {
    check_title(&created.title)?;
    let template = Template::parse(&created.template.definition)
        .map_err(|problems| format!("模板清单有问题：{}", problems.join("；")))?;
    if template.id != created.template.id || template.version != created.template.version {
        return Err("冻结的模板与它的 id 或版本不一致".into());
    }
    match created.mode {
        Mode::Free if !created.objects.is_empty() => return Err("自由容器不绑定核心对象".into()),
        Mode::Free => {}
        Mode::Bound | Mode::Mixed => {
            if template.done_states.is_empty() || template.states.len() <= template.done_states.len() {
                return Err("绑定与混合模式需要模板既有完成类状态，也有未完成的状态".into());
            }
            let mut seen = BTreeSet::new();
            for id in &created.objects {
                valid_id(id)?;
                if !seen.insert(id) {
                    return Err(format!("绑定对象 {id} 重复"));
                }
            }
        }
    }
    template.check_rules(&created.rules)?;
    check_place(&created.place)?;
    if !created.cards.is_empty() {
        if created.mode != Mode::Free {
            return Err("只有自由容器可以带着导入的卡片创建".into());
        }
        if created.cards.len() > IMPORT_MAX {
            return Err(format!("一次最多导入 {IMPORT_MAX} 项"));
        }
        if let Some(write) = created.cards.iter().find(|write| !matches!(write.op, crate::cards::Op::Add { .. } | crate::cards::Op::Link { .. })) {
            return Err(format!("导入只能加卡片和连线，不能 {}", write.op.kind()));
        }
    }
    Ok(template)
}

pub(crate) fn mark(event: &Event) -> Mark {
    Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() }
}

pub fn apply(state: &mut Value, event: &Event, kits: &[&dyn crate::kit::Kit]) -> Result<bool, String> {
    let created = match event.kind.as_str() {
        CREATED => true,
        PLACED => false,
        crate::frame_canvas::APPROVED | crate::frame_canvas::MATERIALIZED => return crate::frame_canvas::apply(state, event),
        _ => return crate::cards::apply(state, event, kits),
    };
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err(format!("{} 的层或 fences 无效", event.kind));
    }
    if !human_actor(&event.actor) {
        return Err("容器只由人创建、移动和缩放".into());
    }
    check_id(&event.obj)?;
    let mut frames = read(state)?;
    if created {
        let body: Created = serde_json::from_value(event.data.clone()).map_err(|e| format!("建容器正文无效：{e}"))?;
        let template = check_created(&body)?;
        if frames.contains_key(&event.obj) {
            return Err("容器ID重复".into());
        }
        frames.insert(event.obj.clone(), Frame {
            id: event.obj.clone(),
            title: body.title,
            template: body.template,
            layout: template.layout,
            states: template.states,
            done_states: template.done_states,
            limits: template.limits,
            set_by: body.set_by,
            mode: body.mode,
            rules: body.rules,
            place: body.place,
            created: mark(event),
            placed: None,
            objects: body.objects,
            cards: Vec::new(),
            pending: Vec::new(),
            rules_changed: None,
            pins: BTreeMap::new(),
            materialized: None,
        });
        // FR-144: what the person pasted lands as their own writes, checked
        // one by one exactly as if they had added each card by hand.
        let at = mark(event);
        for (index, write) in body.cards.iter().enumerate() {
            let target = write.target.as_deref().unwrap_or(&event.obj);
            if crate::cards::frame_of(&write.op, target)? != event.obj {
                return Err(format!("导入的第 {} 项不在这个容器里", index + 1));
            }
            crate::cards::change(&mut frames, &event.obj, &write.op, target, &at, None, true)
                .map_err(|error| format!("导入的第 {} 项：{error}", index + 1))?;
        }
    } else {
        let body: Placed = serde_json::from_value(event.data.clone()).map_err(|e| format!("容器位置正文无效：{e}"))?;
        check_place(&body.place)?;
        let frame = frames.get_mut(&event.obj).ok_or("容器不存在")?;
        frame.place = body.place;
        frame.placed = Some(mark(event));
    }
    state[GROUP] = serde_json::to_value(&frames).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "C-reading";

    fn kanban() -> Value {
        json!({"id":"kanban","version":"1.0.0","type":"template","title":"看板",
            "layout":{"x":{"category":"state"},"y":{"order":"added"},"structure":"none"},
            "states":["To Do","In Progress","Waiting for Review","Done"],
            "transitions":{"Waiting for Review":["Done","In Progress"]},
            "done_states":["Done"],
            "card_fields":{"owner":"text","due":"date","source":"ref"},
            "limits":{"title_max":30,"body_max":100,"per_lane_max":7},
            "default_set_by":"delegate","export":["mermaid","markdown"],
            "x-future":{"kept":true}})
    }

    fn created(definition: Value) -> Value {
        let template = Template::parse(&definition).unwrap();
        json!({"title":"读论文","template":{"id":template.id,"version":template.version,"definition":definition},
            "set_by":"delegate","mode":"free","rules":template.rules(RulesInput::default()).unwrap(),
            "place":{"x":10,"y":20,"width":880,"height":420}})
    }

    fn apply_(state: &mut Value, event: &Event) -> Result<bool, String> {
        apply(state, event, &[])
    }

    fn event(seq: u64, kind: &str, actor: &str, obj: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":"2026-10-23T01:00:00Z","actor":actor,
            "type":kind,"layer":"L3","obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    #[test]
    fn the_spec_kanban_manifest_reads_with_its_layout() {
        let template = Template::parse(&kanban()).unwrap();
        assert_eq!(template.reference(), "kanban@1.0.0");
        assert_eq!(template.layout.x, Axis::Category { field: "state".into(), values: template.states.clone() });
        assert_eq!(template.layout.y, Axis::Order);
        assert!(template.layout.structure.is_empty());
        assert_eq!(template.default_set_by, SetBy::Delegate);
        assert_eq!(template.limits.per_lane_max, Some(7));
        assert_eq!(template.card_fields["due"], FieldKind::Date);
    }

    #[test]
    fn layouts_compose_two_axes_and_a_structure() {
        let gantt = json!({"id":"gantt","version":"1","type":"template",
            "layout":{"x":{"time":"start"},"y":{"category":"task"},"structure":["relations"]},
            "card_fields":{"start":"date","task":"text"}});
        let template = Template::parse(&gantt).unwrap();
        assert_eq!(template.layout.x, Axis::Time { field: "start".into() });
        assert_eq!(template.layout.y, Axis::Category { field: "task".into(), values: vec![] });
        assert_eq!(template.layout.structure, vec![Structure::Relations]);
        let mindmap = json!({"id":"mindmap","version":"1","type":"template","layout":{"structure":["tree","nesting","tree"]}});
        let template = Template::parse(&mindmap).unwrap();
        assert_eq!((template.layout.x, template.layout.y), (Axis::None, Axis::None));
        assert_eq!(template.layout.structure, vec![Structure::Nesting, Structure::Tree]);
        assert_eq!(template.default_set_by, SetBy::Human, "unstated ownership belongs to a person");
    }

    #[test]
    fn a_broken_template_fails_closed_with_every_problem() {
        let broken = json!({"id":"Bad Id","type":"check",
            "layout":{"x":{"category":"state"},"y":{"time":"owner"},"z":"none","structure":"cloud"},
            "card_fields":{"owner":"text","title":"text"},"done_states":["Gone"],
            "limits":{"title_max":31},"default_set_by":"robot"});
        let problems = Template::parse(&broken).unwrap_err();
        for expected in ["缺少 version", "type 须是 template", "layout 中有不认识的键 z", "按状态分栏的模板须声明 states",
            "时间轴字段 owner 须是 date 类型的卡片字段", "layout.structure 不认识 \"cloud\"", "done_states 中的 Gone 不在状态集合里",
            "limits.title_max 须是 1 到 30 的整数", "default_set_by 须是 human 或 delegate"] {
            assert!(problems.iter().any(|problem| problem.contains(expected)), "{expected} missing from {problems:?}");
        }
        assert!(problems.iter().any(|problem| problem.contains("title")));
        let same = json!({"id":"x","version":"1","type":"template","layout":{"x":{"order":"added"},"y":{"order":"added"}}});
        assert_eq!(Template::parse(&same).unwrap_err(), vec!["横轴与纵轴不能用同一个字段".to_owned()]);
    }

    #[test]
    fn rules_default_from_the_template_and_may_only_narrow_it() {
        let template = Template::parse(&kanban()).unwrap();
        let rules = template.rules(RulesInput::default()).unwrap();
        assert_eq!(rules.fields, ["due", "owner", "source"]);
        assert_eq!(rules.transitions, template.transitions);
        assert_eq!(rules.per_lane_max, Some(7));
        assert!(rules.done_needs_human && !rules.require_source);
        let narrowed = RulesInput {
            text: Some("每读完一篇论文，在'已读'加一张卡片".into()), add_card: Some(Who::Human),
            transitions: Some(BTreeMap::from([("Waiting for Review".into(), vec!["Done".into()])])),
            per_lane_max: Some(5), require_source: Some(true), ..Default::default()
        };
        assert!(template.rules(narrowed).is_ok());
        let looser = RulesInput { transitions: Some(Transitions::new()), ..Default::default() };
        assert_eq!(template.rules(looser).unwrap_err(), "规则不能放宽模板对 Waiting for Review 的状态转换");
        let wider = RulesInput { per_lane_max: Some(8), ..Default::default() };
        assert!(template.rules(wider).is_err());
        let long = RulesInput { text: Some("字".repeat(RULES_CHARS + 1)), ..Default::default() };
        assert!(template.rules(long).is_err());
        let unknown = RulesInput { fields: Some(vec!["mood".into()]), ..Default::default() };
        assert!(template.rules(unknown).is_err());
        let sourceless = RulesInput { fields: Some(vec!["owner".into()]), require_source: Some(true), ..Default::default() };
        assert!(template.rules(sourceless).is_err());
    }

    #[test]
    fn other_events_are_not_frames_and_an_untouched_projection_has_no_group() {
        let mut state = json!({});
        assert!(!apply_(&mut state, &event(1, "idea.added", "human:local", ID, json!({}))).unwrap());
        assert_eq!(state, json!({}));
        assert!(read(&state).unwrap().is_empty());
    }

    #[test]
    fn a_person_creates_a_frame_that_freezes_the_template_as_written() {
        let mut state = json!({});
        assert!(apply_(&mut state, &event(1, CREATED, "human:local", ID, created(kanban()))).unwrap());
        let frame = &read(&state).unwrap()[ID];
        assert_eq!(frame.title, "读论文");
        assert_eq!(frame.template.definition["x-future"], json!({"kept":true}), "unknown keys survive");
        assert_eq!(frame.states.len(), 4);
        assert_eq!(frame.set_by, SetBy::Delegate);
        assert_eq!(frame.place, Place { x: 10, y: 20, width: 880, height: 420 });
        assert_eq!(frame.created.actor, "human:local");
        let round: Frames = serde_json::from_value(state[GROUP].clone()).unwrap();
        assert_eq!(serde_json::to_value(&round).unwrap(), state[GROUP]);
        let again = event(2, CREATED, "human:local", ID, created(kanban()));
        assert_eq!(apply_(&mut state, &again).unwrap_err(), "容器ID重复");
    }

    #[test]
    fn only_a_person_creates_or_moves_a_frame() {
        for actor in ["process:towermd-cli", "pkg:kanban", "claude-code/2.0", "human:"] {
            let mut state = json!({});
            assert_eq!(apply_(&mut state, &event(1, CREATED, actor, ID, created(kanban()))).unwrap_err(), "容器只由人创建、移动和缩放");
        }
        let mut state = json!({});
        apply_(&mut state, &event(1, CREATED, "human:local", ID, created(kanban()))).unwrap();
        let place = json!({"place":{"x":-40,"y":0,"width":900,"height":500}});
        assert!(apply_(&mut state, &event(2, PLACED, "process:towermd-cli", ID, place.clone())).is_err());
        assert!(apply_(&mut state, &event(3, PLACED, "human:local", ID, place)).unwrap());
        let frame = &read(&state).unwrap()[ID];
        assert_eq!(frame.place, Place { x: -40, y: 0, width: 900, height: 500 });
        assert_eq!(frame.placed.as_ref().unwrap().seq, 3);
        let missing = event(4, PLACED, "human:local", "C-other", json!({"place":{"x":0,"y":0,"width":900,"height":500}}));
        assert_eq!(apply_(&mut state, &missing).unwrap_err(), "容器不存在");
    }

    #[test]
    fn a_malformed_frame_is_rejected() {
        let mut state = json!({});
        let wrong_layer = {
            let mut event = event(1, CREATED, "human:local", ID, created(kanban()));
            event.layer = "L1".into();
            event
        };
        assert!(apply_(&mut state, &wrong_layer).is_err(), "frames are never slow-layer writes");
        assert!(apply_(&mut state, &event(1, CREATED, "human:local", "reading", created(kanban()))).is_err());
        let mut forged = created(kanban());
        forged["template"]["version"] = json!("2.0.0");
        assert_eq!(apply_(&mut state, &event(1, CREATED, "human:local", ID, forged)).unwrap_err(), "冻结的模板与它的 id 或版本不一致");
        let mut bound = created(kanban());
        bound["objects"] = json!(["T-1"]);
        assert_eq!(apply_(&mut state, &event(1, CREATED, "human:local", ID, bound)).unwrap_err(), "自由容器不绑定核心对象");
        let stateless = json!({"id":"mindmap","version":"1","type":"template","layout":{"structure":"tree"}});
        let mut bound = created(stateless);
        bound["mode"] = json!("bound");
        assert!(apply_(&mut state, &event(1, CREATED, "human:local", ID, bound)).unwrap_err().contains("完成类状态"));
        let mut tiny = created(kanban());
        tiny["place"]["width"] = json!(10);
        assert!(apply_(&mut state, &event(1, CREATED, "human:local", ID, tiny)).is_err());
        let mut extra = created(kanban());
        extra["actor"] = json!("human:local");
        assert!(apply_(&mut state, &event(1, CREATED, "human:local", ID, extra)).is_err());
        let mut broken = created(kanban());
        broken["template"]["definition"]["layout"]["x"] = json!({"category":"mood"});
        assert!(apply_(&mut state, &event(1, CREATED, "human:local", ID, broken)).unwrap_err().starts_with("模板清单有问题"));
        let mut loose = created(kanban());
        loose["rules"]["transitions"] = json!({});
        assert!(apply_(&mut state, &event(1, CREATED, "human:local", ID, loose)).is_err());
        assert_eq!(state, json!({}));
    }
}
