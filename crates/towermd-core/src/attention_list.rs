//! The attention list (core spec §7: FR-40, FR-128–131, FR-134–136): which
//! items need a person, in which order, how many reach the first screen,
//! when each may interrupt, and the "since your last visit" digest. The
//! daemon serves the list with the project state; the App and the command
//! line only display it. Everything here is derived from the state body the
//! daemon already sends (projection, delegation view, rhythm, settings), so
//! nothing is stored and the list is rebuilt on every read.
//!
//! Generic items come from core groups: open questions, proposals, alerts,
//! open reviews, Tower API signals, spot checks, unanswerable requests, due
//! follow-ups and resurfaced objects. A kit adds its own alert wording, folds
//! alerts a later item supersedes (`Kit::folded`), names review stages,
//! places its objects (`Kit::locate`) and names what is stuck in its own
//! groups (`Kit::stuck`).
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Timelike, Datelike};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::attention::FIRST_SCREEN;
use crate::kit::Kit;

/// FR-129: a card body has at most this many characters.
pub const CARD_CHARS: usize = 100;
/// Away longer than this, the digest gives counts instead of names (FR-131).
pub const COARSE_AFTER_MS: u64 = 24 * 60 * 60 * 1000;
/// No recorded visit: the digest covers the last 7 days (FR-42).
pub const FALLBACK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Only Tower.md itself may sign `core.` signals.
const TOWER: &str = crate::tower_api::TOWER;
const WEAKENING: &str = crate::weakening::KIND;
const SILENT: &str = "core.silent";
const STUCK: &str = "core.stuck";
const LEVELS: [&str; 4] = ["log", "passive", "active", "time_sensitive"];

/// The nineteen kinds of attention item. JSON carries the English name; `label`
/// is what the App and the card show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Blocked,
    CheckFailed,
    Question,
    Proposal,
    Alert,
    SpotCheck,
    Silent,
    Stuck,
    Unanswerable,
    Weakening,
    FollowUp,
    Resurfaced,
    WaiverExpiring,
    DecisionReview,
    /// A run went wrong: a crash, repeated failures or a suspected design problem.
    Error,
    /// A run reached its spending limit, or its cost cannot be counted.
    Budget,
    /// An object stopped and waits for a person's choice; it never resumes by waiting.
    Interrupt,
    /// A delegate asked a person for a review; it never passes by waiting.
    Review,
    /// A delegate asks to use a tool now; unanswered it is denied, never allowed.
    Permission,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Blocked => "禁止", Kind::CheckFailed => "检查未通过", Kind::Question => "提问", Kind::Proposal => "待批提案",
            Kind::Alert => "告警", Kind::SpotCheck => "抽查", Kind::Silent => "失联", Kind::Stuck => "兜圈子",
            Kind::Unanswerable => "无人可答", Kind::Weakening => "承诺弱化", Kind::FollowUp => "跟进", Kind::Resurfaced => "浮现",
            Kind::WaiverExpiring => "豁免到期", Kind::Error => "错误", Kind::Budget => "预算", Kind::Interrupt => "中断",
            Kind::Review => "评审", Kind::DecisionReview => "复查决策", Kind::Permission => "授权",
        }
    }
    /// Shape per kind, so an item never relies on colour alone.
    pub fn symbol(self) -> &'static str {
        match self {
            Kind::Blocked => "⊘", Kind::CheckFailed => "✕", Kind::Question => "?", Kind::Proposal => "◇", Kind::Alert => "!",
            Kind::SpotCheck => "◎", Kind::Silent => "◷", Kind::Stuck => "↻", Kind::Unanswerable => "∅", Kind::Weakening => "⇣",
            Kind::FollowUp => "⚑", Kind::Resurfaced => "↥", Kind::WaiverExpiring => "⌛", Kind::Error => "‼", Kind::Budget => "¤",
            Kind::Interrupt => "⏸", Kind::Review => "⚖", Kind::DecisionReview => "✎", Kind::Permission => "⚿",
        }
    }
    /// FR-129 "why" and "what you must do" when the item has no detail of its own.
    fn texts(self) -> (&'static str, &'static str) {
        match self {
            Kind::Blocked => ("人定内容只能经人批准进入主干", "认领、撤销或转提案"),
            Kind::CheckFailed => ("完成只认检查证据", "看证据后决定重跑或插话"),
            Kind::Question => ("受托者等你回答才能继续", "回答"),
            Kind::Proposal => ("改动涉及人定内容，须人批准", "批准或驳回"),
            Kind::Alert => ("来源自报，不是证据", "核对后决定是否插话"),
            Kind::SpotCheck => ("已完成的任务按比例抽查", "复核后确认或退回"),
            Kind::Silent => ("请求过了期限仍无人回答（Tower.md 判断）", "插话、换人或撤回请求"),
            Kind::Stuck => ("同一件事反复打转（Tower.md 判断）", "插话、改写委托卡或暂停"),
            Kind::Unanswerable => ("还没有受托者声明能回答这个节点", "等待、撤回或改问别人"),
            Kind::Weakening => ("完成标准、检查或门禁被放宽", "核对后驳回、要求恢复或接受"),
            Kind::FollowUp => ("你设的到点条件满足了", "看一眼，看完完成跟进"),
            Kind::Resurfaced => ("休眠的对象有了回来的理由", "看上次到哪了，再决定接着做或放下"),
            Kind::WaiverExpiring => ("豁免不续期，到期后门禁恢复拦截", "到期前修好，或到期后另行决定"),
            Kind::DecisionReview => ("到了你定的复查日期", "保留、改写或作废"),
            Kind::Error => ("运行器观察到运行崩溃、反复失败或疑似设计问题", "重试、改写任务、换执行工具或放弃"),
            Kind::Budget => ("运行到了预算上限，或费用未知不能计入预算", "追加预算、换模型或停止"),
            Kind::Interrupt => ("任务停下等你决定，不会超时放行", "再修、改写、重派或豁免"),
            Kind::Review => ("受托者请人评审，评审不会超时通过", "通过、要求修改或驳回"),
            Kind::Permission => ("受托者请求使用工具，超时未答即拒绝", "本次允许或拒绝"),
        }
    }
}

/// One attention candidate before ordering.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    /// Milliseconds since the epoch; 0 when unknown.
    pub at: u64,
    /// Objects the item concerns, as the kit places them.
    pub objects: Vec<String>,
    /// Areas those objects sit in.
    pub areas: Vec<String>,
    /// The card's object when it is neither a placed object nor an area.
    pub object: Option<String>,
    pub detail: Option<String>,
    pub action: String,
    /// FR-128 class 1: the sender declared it time-sensitive.
    pub urgent: bool,
    /// The delegated object whose card or reports answer this item.
    pub delegation: Option<String>,
    /// The merged signal's target, when the item is a signal.
    pub signal: Option<String>,
}

impl Item {
    fn new(id: impl Into<String>, kind: Kind, title: impl Into<String>, at: u64, action: impl Into<String>) -> Self {
        Self { id: id.into(), kind, title: title.into(), at, objects: vec![], areas: vec![], object: None, detail: None,
            action: action.into(), urgent: false, delegation: None, signal: None }
    }
}

/// Where a kit places one of its objects: its own area plus any others it reaches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Located {
    pub area: String,
    pub reaches: Vec<String>,
}

/// What a kit says about an alert reason it owns. Without a title the
/// alert's own title (or its reason) is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertText {
    pub kind: Kind,
    pub title: Option<String>,
    pub action: String,
}

/// FR-130's timings. `summary` (passive signals) only reaches the digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Timing {
    Now,
    NextVisit,
}

/// One ordered item as the App and the command line show it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    #[serde(flatten)]
    pub item: Item,
    pub label: &'static str,
    pub symbol: &'static str,
    /// FR-128 class, 1 to 4.
    pub class: u8,
    pub timing: Timing,
    /// FR-135: about quiet objects only; waits under "later".
    pub quiet: bool,
    /// FR-129: the one-line card, at most `CARD_CHARS` characters.
    pub card: String,
}

/// The ordered list: awake items first, then quiet ones; the first `shown`
/// are the first screen, the rest wait under "later".
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct List {
    pub items: Vec<Entry>,
    pub shown: usize,
    pub asleep: usize,
    pub budget: usize,
}

/// FR-128 classes: 1 time-sensitive or protection-related; 2 a person is
/// waited on; 3 everything else; 4 due follow-ups, resurfaced objects and
/// decisions due for review: reminders a person set for themselves.
pub fn class(item: &Item) -> u8 {
    match item.kind {
        _ if item.urgent => 1,
        // A permission request expires into a denial within minutes.
        Kind::Blocked | Kind::Weakening | Kind::Permission => 1,
        Kind::FollowUp | Kind::Resurfaced | Kind::DecisionReview => 4,
        Kind::Question | Kind::Proposal | Kind::Review | Kind::Interrupt => 2,
        _ => 3,
    }
}

fn reach(item: &Item) -> usize {
    item.objects.iter().chain(&item.areas).collect::<BTreeSet<_>>().len()
}

/// Oldest known time first; an unknown time never counts as the longest wait.
fn older(a: &Item, b: &Item) -> Ordering {
    let known = |at: u64| if at == 0 { u64::MAX } else { at };
    known(a.at).cmp(&known(b.at))
}

/// FR-128 total order: class first; within the waiting class the longest
/// wait leads, elsewhere the widest reach (FR-40); then the older item, then the ID.
pub fn compare(a: &Item, b: &Item) -> Ordering {
    let (ca, cb) = (class(a), class(b));
    if ca != cb {
        return ca.cmp(&cb);
    }
    let by_wait = older(a, b);
    let by_reach = reach(b).cmp(&reach(a));
    let first = if ca == 2 { by_wait.then(by_reach) } else { by_reach.then(by_wait) };
    first.then_with(|| a.id.cmp(&b.id))
}

pub fn timing(item: &Item) -> Timing {
    if item.urgent { Timing::Now } else { Timing::NextVisit }
}

// ---------- lenient JSON reading ----------

fn record(value: Option<&Value>) -> &Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    value.and_then(Value::as_object).unwrap_or_else(|| EMPTY.get_or_init(Map::new))
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}
fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
/// JavaScript's `String(value)` for the scalars a card may quote.
/// JavaScript truthiness of a JSON value.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(_) => true,
    }
}
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".into(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(_) | Value::Object(_) => "[object Object]".into(),
    }
}

/// Milliseconds of an RFC 3339 time, or of a local time without an offset; 0 when unreadable.
pub fn parse_time(text: &str) -> u64 {
    if let Ok(time) = DateTime::parse_from_rfc3339(text) {
        return u64::try_from(time.timestamp_millis()).unwrap_or(0);
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            if let Some(time) = Local.from_local_datetime(&naive).earliest() {
                return u64::try_from(time.timestamp_millis()).unwrap_or(0);
            }
        }
    }
    0
}
fn timestamp(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => number.as_f64().filter(|ms| ms.is_finite() && *ms > 0.0).map_or(0, |ms| ms as u64),
        Some(Value::String(text)) => parse_time(text),
        _ => 0,
    }
}
fn local(at: u64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(i64::try_from(at).ok()?).single()
}

/// Local month, day and time; unknown times say so instead of guessing.
pub fn when(at: u64) -> String {
    match local(at).filter(|_| at > 0) {
        Some(date) => format!("{:02}-{:02} {:02}:{:02}", date.month(), date.day(), date.hour(), date.minute()),
        None => "时间未知".into(),
    }
}

// ---------- cards (FR-129) ----------

/// The six segments of one item, in order: when, object, what, why, suggestion, ask.
pub struct Card {
    pub when: String,
    pub object: String,
    pub what: String,
    pub why: String,
    pub suggest: String,
    pub ask: String,
}

pub fn card(item: &Item) -> Card {
    let object = item.object.clone().unwrap_or_else(|| match item.objects.as_slice() {
        [first, _, ..] => format!("{first} 等 {} 个任务", item.objects.len()),
        [first] => format!("任务 {first}"),
        [] if !item.areas.is_empty() => format!("模块 {}", item.areas.join("、")),
        [] => "项目".into(),
    });
    let (why, ask) = item.kind.texts();
    Card { when: when(item.at), object, what: item.title.clone(), why: item.detail.clone().unwrap_or_else(|| why.into()),
        suggest: item.action.clone(), ask: ask.into() }
}

fn trim(text: &str) -> Vec<char> {
    let text = text.trim();
    let end = text.trim_end_matches(|c: char| matches!(c, '。' | '.' | '；' | ';' | '，' | ',') || c.is_whitespace());
    end.chars().collect()
}

/// One line, fixed order, at most `limit` characters. When too long, the
/// longest segments are shortened first (with "…"), so short ones such as
/// the time, the object and what you must do stay whole.
pub fn card_text(card: &Card, limit: usize) -> String {
    let parts: Vec<Vec<char>> = [&card.object, &card.what, &card.why, &card.suggest, &card.ask].iter().map(|part| trim(part)).collect();
    let render = |p: &[Vec<char>]| {
        let s = |i: usize| p[i].iter().collect::<String>();
        format!("{} · {}：{}。因为{}。建议{}。需要你：{}。", card.when, s(0), s(1), s(2), s(3), s(4))
    };
    let fixed = render(&vec![Vec::new(); 5]).chars().count() as i64;
    let mut budget = limit as i64 - fixed;
    if budget >= parts.iter().map(Vec::len).sum::<usize>() as i64 {
        return render(&parts);
    }
    // Water-filling: the largest cap that fits; parts above it are cut.
    let mut sorted: Vec<i64> = parts.iter().map(|part| part.len() as i64).collect();
    sorted.sort_unstable();
    let mut cap = 0;
    for (index, length) in sorted.iter().enumerate() {
        let rest = (sorted.len() - index) as i64;
        let level = budget.div_euclid(rest);
        if *length > level {
            cap = level;
            break;
        }
        budget -= length;
        cap = *length;
    }
    let cut: Vec<Vec<char>> = parts.iter().map(|part| {
        if part.len() as i64 <= cap { part.clone() } else {
            let mut kept: Vec<char> = part[..cap.max(1) as usize - 1].to_vec();
            kept.push('…');
            kept
        }
    }).collect();
    render(&cut)
}

// ---------- signals ----------

struct Signal<'a> {
    entry: &'a Value,
    from: &'a str,
    target: &'a str,
    kind: &'a str,
    level: usize,
    title: &'a str,
    last_at: u64,
    actor: &'a str,
}

fn signals(group: Option<&Value>) -> Vec<Signal<'_>> {
    array(record(group).get("signals")).iter().filter_map(|entry| {
        let data = entry.get("data")?;
        let last = entry.get("last")?;
        text(last, "eid")?;
        Some(Signal { entry, from: entry.get("from").and_then(|from| text(from, "id"))?, target: text(entry, "target")?,
            kind: text(data, "kind")?, level: LEVELS.iter().position(|level| Some(*level) == text(data, "level"))?,
            title: text(data, "title")?, last_at: timestamp(last.get("at")),
            actor: text(last, "actor").unwrap_or_default() })
    }).collect()
}

/// A person quieted this signal in the App: acknowledged until a newer
/// message, or muted until it ends, broken through when the sender escalates.
fn signal_quiet(signal: &Signal, now: u64) -> bool {
    let entry = signal.entry;
    if truthy(entry.get("acknowledged")) {
        return true;
    }
    let Some(muted) = entry.get("muted").filter(|value| value.is_object()) else { return false };
    let until = text(muted, "until").map_or(0, parse_time);
    let level = text(muted, "level").and_then(|level| LEVELS.iter().position(|known| *known == level));
    until > now && level.is_some_and(|level| signal.level <= level)
}

// ---------- assembly ----------

/// What the producers share: the projection, the kits and placement.
struct Context<'a> {
    projection: &'a Value,
    kits: &'a [&'a dyn Kit],
}

impl Context<'_> {
    fn locate(&self, object: &str) -> Option<Located> {
        self.kits.iter().find_map(|kit| kit.locate(self.projection, object))
    }
    /// The object and its own area, when a kit owns it.
    fn place(&self, object: &str) -> (Vec<String>, Vec<String>) {
        match self.locate(object) {
            Some(located) => (vec![object.to_owned()], vec![located.area]),
            None => (vec![], vec![]),
        }
    }
}

fn alerts(context: &Context, items: &mut Vec<Item>) {
    let group = record(context.projection.get("Alert"));
    for (id, value) in group {
        if value.get("resolved") == Some(&Value::Bool(true)) || text(value, "status") == Some("resolved") || text(value, "state") == Some("resolved") {
            continue;
        }
        let data = value.get("data").unwrap_or(&Value::Null);
        let detail = data.get("detail").unwrap_or(&Value::Null);
        let reason = text(data, "reason").unwrap_or_default();
        if context.kits.iter().any(|kit| kit.folded(context.projection, reason, detail)) {
            continue;
        }
        let protected = reason == "unapproved_protected_change";
        // FR-03: a commit that changed only process configuration is a minor change, registered rather than versioned.
        let minor = protected && text(detail, "class") == Some("minor");
        let incomplete = reason == "inspection_incomplete";
        let owned = context.kits.iter().find_map(|kit| kit.alert(reason));
        let (kind, title, action) = if minor {
            (Kind::Blocked, "主干含未登记的流程配置变更（次要变更）".to_owned(), "审阅该提交后转提案登记；流程配置不能认领或普通撤销。".to_owned())
        } else if protected {
            (Kind::Blocked, "主干含未经批准的人定内容变更".to_owned(), "审阅该提交后选择认领、撤销或转提案。".to_owned())
        } else if incomplete {
            (Kind::Blocked, "主干历史尚不能完整核验".to_owned(), "补全 Git 历史或恢复合法的主干，再等待重新核验。".to_owned())
        } else {
            let title = || [value.get("title"), data.get("reason"), data.get("question")].into_iter().flatten()
                .find(|value| !value.is_null()).map_or_else(|| id.clone(), js_string);
            match owned {
                Some(owned) => (owned.kind, owned.title.unwrap_or_else(title), owned.action),
                None => (Kind::Alert, title(), "查看原始告警并核对项目状态。".to_owned()),
            }
        };
        let mut item = Item::new(id.clone(), kind, title, timestamp(value.get("at")), action);
        if let Some(task) = text(detail, "task") {
            if let Some(located) = context.locate(task) {
                let mut areas = vec![located.area];
                for area in located.reaches {
                    if !areas.contains(&area) { areas.push(area) }
                }
                item.areas = areas;
                item.objects = vec![task.to_owned()];
            }
        }
        item.detail = text(detail, "error").map(str::to_owned);
        // The held object's own panel offers the choices.
        if kind == Kind::Interrupt {
            item.delegation = item.objects.first().cloned();
        }
        items.push(item);
    }
}

/// FR-58: open reviews, oldest first by the list's order; they end only by a person's verdict.
fn reviews(context: &Context, items: &mut Vec<Item>) {
    for (key, value) in record(context.projection.get(crate::reviews::GROUP)) {
        let request = value.get("request").unwrap_or(&Value::Null);
        let resolved = value.get("resolution").is_some_and(|resolution| !resolution.is_null());
        let (Some(id), Some(object), Some(stage), Some(summary)) = (text(value, "id"), text(request, "object"), text(request, "stage"), text(request, "summary")) else { continue };
        if resolved || id != key {
            continue;
        }
        let stage = context.kits.iter().find_map(|kit| kit.review_stage(stage)).unwrap_or(stage);
        let mut item = Item::new(format!("review:{id}"), Kind::Review, format!("{stage}：{summary}"),
            timestamp(value.get("requested").and_then(|requested| requested.get("at"))), "读评审材料后给出结论：通过、要求修改或驳回，后两者写意见。");
        (item.objects, item.areas) = context.place(object);
        if item.objects.is_empty() {
            item.object = Some(object.to_owned());
        }
        item.delegation = Some(object.to_owned());
        items.push(item);
    }
}

/// FR-72: a waiver in its last `SOON_HOURS`. It is never extended: once it
/// lapses the gate blocks again, so a person decides now whether the fix lands in time.
fn waivers(context: &Context, now: u64, items: &mut Vec<Item>) {
    const SOON_MS: u64 = crate::waivers::SOON_HOURS as u64 * 60 * 60 * 1000;
    for value in record(context.projection.get(crate::waivers::GROUP)).values() {
        let (Some(id), Some(object), Some(gate), Some(until)) = (text(value, "id"), text(value, "object"), text(value, "gate"), text(value, "until")) else { continue };
        let until = parse_time(until);
        if until == 0 || until <= now || until - now > SOON_MS {
            continue;
        }
        let at = timestamp(value.get("granted").and_then(|granted| granted.get("at")));
        let mut item = Item::new(format!("waiver:{id}"), Kind::WaiverExpiring, format!("门禁 {gate} 的豁免将于 {} 到期", when(until)), at,
            "到期前修好让检查通过；仍需放行就等到期后另行豁免。");
        (item.objects, item.areas) = context.place(object);
        if item.objects.is_empty() {
            item.object = Some(object.to_owned());
        }
        item.delegation = Some(object.to_owned());
        item.detail = Some(format!("豁免理由：{}；到期后不续期，门禁恢复拦截", value.get("reason").filter(|reason| !reason.is_null()).map(js_string).unwrap_or_default()));
        items.push(item);
    }
}

/// Open questions, valid as the App reads them, keyed by hold ID.
fn questions(projection: &Value) -> BTreeMap<&str, &Value> {
    record(projection.get("Hold")).values().filter_map(|question| {
        let draft = question.get("draft")?;
        let binding = question.get("binding")?;
        let valid = text(binding, "module").is_some() && draft.get("options").is_some_and(Value::is_array)
            && draft.get("context").is_some_and(Value::is_array) && question.get("targets").is_some_and(Value::is_array)
            && text(draft, "text").is_some();
        valid.then_some((text(draft, "id")?, question))
    }).collect()
}
fn answers(projection: &Value) -> BTreeMap<&str, &Value> {
    record(projection.get("Answer")).iter().filter(|(_, answer)| {
        let input = answer.get("input").unwrap_or(&Value::Null);
        text(input, "hold").is_some() && text(input, "text").is_some() && text(answer, "event").is_some()
    }).map(|(id, answer)| (id.as_str(), answer)).collect()
}

fn holds(context: &Context, items: &mut Vec<Item>) {
    let answered = answers(context.projection);
    for (id, question) in questions(context.projection) {
        if answered.contains_key(id) { continue }
        let binding = &question["binding"];
        let mut item = Item::new(id, Kind::Question, text(&question["draft"], "text").unwrap_or_default(),
            timestamp(question.get("opened_at")), "回答问题；涉及人定内容的回答仍须经提案批准。");
        item.areas = vec![text(binding, "module").unwrap_or_default().to_owned()];
        item.objects = text(binding, "task").map(|task| vec![task.to_owned()]).unwrap_or_default();
        items.push(item);
    }
}

fn proposals(context: &Context, items: &mut Vec<Item>) {
    let questions = questions(context.projection);
    let answered = answers(context.projection);
    for (id, entry) in record(context.projection.get("Proposal")) {
        let status = entry.get("status").map(js_string).unwrap_or_else(|| "undefined".into());
        if ["approved", "rejected", "superseded"].contains(&status.as_str()) { continue }
        let proposal = entry.get("proposal").unwrap_or(&Value::Null);
        let impact = proposal.get("impact").unwrap_or(&Value::Null);
        let source = questions.iter().find(|(hold, _)| answered.get(*hold).and_then(|answer| text(answer, "proposal")) == Some(id.as_str()))
            .map(|(_, question)| &question["binding"]);
        let pick = |key: &str, binding_key: &str| {
            let listed = strings(impact.get(key));
            if !listed.is_empty() { return listed }
            source.and_then(|binding| text(binding, binding_key)).map(|value| vec![value.to_owned()]).unwrap_or_default()
        };
        let title = [proposal.get("reason"), entry.get("text")].into_iter().flatten().find(|value| !value.is_null())
            .map_or_else(|| id.clone(), js_string);
        let action = if status == "submitted" { "审阅完整差异与影响后批准或驳回。" } else { "先补全正式提案及验证，再审阅批准。" };
        let mut item = Item::new(id.clone(), Kind::Proposal, title, timestamp(entry.get("at")), action);
        item.areas = pick("modules", "module");
        item.objects = pick("tasks", "task");
        items.push(item);
    }
}

fn delegation(context: &Context, view: Option<&Value>, items: &mut Vec<Item>) {
    let Some(view) = view.filter(|view| view.is_object()) else { return };
    let objects = array(view.get("objects"));
    let title = |id: &str| objects.iter().find(|object| text(object, "id") == Some(id)).and_then(|object| text(object, "title")).unwrap_or(id).to_owned();
    for check in array(view.get("spot_checks")) {
        if truthy(check.get("resolved")) { continue }
        let (Some(object), Some(attempt)) = (text(check, "object"), text(check, "attempt")) else { continue };
        let at = timestamp(check.get("done").and_then(|done| done.get("at")));
        let mut item = Item::new(format!("spot-check:{object}:{attempt}"), Kind::SpotCheck, format!("抽查：{}", title(object)), at,
            "打开验收卡逐条复核证据，再确认或退回。");
        item.areas = context.place(object).1;
        item.objects = vec![object.to_owned()];
        item.delegation = Some(object.to_owned());
        items.push(item);
    }
    for request in array(view.get("requests")) {
        if request.get("open").and_then(Value::as_bool) != Some(true) || !array(request.get("recipients")).is_empty() { continue }
        let (Some(id), Some(data)) = (text(request, "id"), request.get("data")) else { continue };
        let target = text(data, "target").unwrap_or_default();
        let known = objects.iter().any(|object| text(object, "id") == Some(target));
        let areas = context.place(target).1;
        let mut item = Item::new(format!("unanswerable:{id}"), Kind::Unanswerable,
            format!("无人可答：{} · {}", title(target), data.get("text").map(js_string).unwrap_or_default()),
            timestamp(request.get("event").and_then(|event| event.get("at"))),
            "还没有受托者声明能回答这个节点；它会排队到有人声明为止。可以撤回，或在节点上改问别人。");
        item.object = (!known && areas.is_empty()).then(|| target.to_owned());
        item.areas = areas;
        item.objects = if known { vec![target.to_owned()] } else { vec![] };
        item.delegation = known.then(|| target.to_owned());
        items.push(item);
    }
}

/// FR-118: `active` and `time_sensitive` signals not quieted by a person.
fn signal_items(context: &Context, now: u64, items: &mut Vec<Item>) {
    // FR-125: a weakening a person accepted with a reason, until a newer message.
    let accepted = crate::human_decisions::accepted(context.projection);
    for signal in signals(context.projection.get(crate::tower_api::GROUP)) {
        if signal.level < 2 || signal_quiet(&signal, now) { continue }
        let weakening = crate::weakening::reported(signal.kind, signal.actor);
        if weakening && signal.entry["last"].get("eid").and_then(Value::as_str).is_some_and(|eid| accepted.contains(eid)) { continue }
        let data = &signal.entry["data"];
        let (objects, areas) = context.place(signal.target);
        let count = signal.entry.get("count").and_then(Value::as_u64).unwrap_or(1);
        let title = if count > 1 { format!("{} ×{count}", signal.title) } else { signal.title.to_owned() };
        let mut item = Item::new(format!("signal:{}:{}:{}", signal.from, signal.kind, signal.target), Kind::Alert, title, signal.last_at, "");
        item.urgent = signal.level == 3;
        item.signal = Some(signal.target.to_owned());
        let detail = text(data, "detail").map(str::to_owned);
        let unplaced = objects.is_empty().then(|| signal.target.to_owned());
        if weakening {
            item.kind = Kind::Weakening;
            item.object = objects.is_empty().then(|| if signal.target.starts_with("P-") { format!("提案 {}", signal.target) } else { signal.target.to_owned() });
            // A check extension's own subclass is its claim, said as such.
            item.detail = if signal.kind == WEAKENING { detail } else {
                Some(format!("{} 自报 · {}{}", signal.from, signal.kind, detail.map(|detail| format!(" · {detail}")).unwrap_or_default()))
            };
            item.action = "打开对应提案、委托卡或门禁配置核对改动；不同意就驳回或要求恢复。".into();
        } else if (signal.kind == SILENT || signal.kind == STUCK) && signal.actor == TOWER {
            let silent = signal.kind == SILENT;
            item.kind = if silent { Kind::Silent } else { Kind::Stuck };
            item.object = unplaced;
            item.detail = detail;
            item.action = if silent { "请求过了期限仍无人回答；查看后决定插话、换人或撤回请求。" } else { "同一件事反复打转；查看任务后决定插话、改写委托卡或暂停。" }.into();
        } else {
            let measured = match data.get("value") {
                None => String::new(),
                Some(value) => format!(" · {}{}", js_string(value), data.get("threshold").map(|threshold| format!(" / {}", js_string(threshold))).unwrap_or_default()),
            };
            let detail = detail.map(|detail| format!(" · {detail}")).unwrap_or_default();
            item.object = unplaced;
            // A signal Tower.md measured itself, worded by the kit that owns its kind.
            match context.kits.iter().find_map(|kit| kit.alert(signal.kind)).filter(|_| signal.actor == TOWER) {
                Some(owned) => {
                    item.kind = owned.kind;
                    item.detail = Some(format!("Tower.md 测得 · {}{measured}{detail}", signal.kind));
                    item.action = owned.action;
                }
                None => {
                    item.detail = Some(format!("{} 自报 · {}{measured}{detail}", signal.from, signal.kind));
                    item.action = "这是来源自报的提醒，不是证据；核对后再决定是否插话或暂停。".into();
                }
            }
        }
        item.objects = objects;
        item.areas = areas;
        items.push(item);
    }
}

fn mark(value: &Value) -> bool {
    text(value, "at").is_some() && text(value, "eid").is_some() && value.get("seq").is_some_and(|seq| seq.is_u64() || seq.is_i64()) && text(value, "actor").is_some()
}
fn valid_when(value: &Value) -> bool {
    match text(value, "on") {
        Some("date") => text(value, "at").is_some(),
        Some("done" | "activity" | "signal") => text(value, "target").is_some(),
        _ => false,
    }
}
/// Follow-ups as the App reads them.
fn follow_ups(projection: &Value) -> Vec<&Value> {
    record(projection.get("FollowUp")).values().filter(|entry| {
        text(entry, "id").is_some() && text(entry, "text").is_some() && entry.get("when").is_some_and(valid_when) && entry.get("added").is_some_and(mark)
    }).collect()
}
/// The daemon's rhythm, read leniently; `None` from an older daemon.
fn rhythm(value: Option<&Value>) -> Option<(Vec<&Value>, Vec<&str>)> {
    let raw = value?.as_object()?;
    raw.get("dormant_after").filter(|value| value.is_u64() || value.is_i64())?;
    let objects = raw.get("objects")?.as_array()?.iter().filter(|object| {
        text(object, "id").is_some() && object.get("quiet").is_some_and(Value::is_boolean)
            && object.get("last").is_some_and(|last| last.is_null() || mark(last))
    }).collect();
    let due = array(raw.get("due")).iter().filter_map(Value::as_str).collect();
    Some((objects, due))
}

fn pad_date(at: u64) -> Option<String> {
    let date = local(at).filter(|_| at > 0)?;
    Some(format!("{}-{:02}-{:02} {:02}:{:02}", date.year(), date.month(), date.day(), date.hour(), date.minute()))
}
/// The condition in words; a target that is the project reads as "项目".
fn when_label(when: &Value, title: &dyn Fn(&str) -> String) -> String {
    if text(when, "on") == Some("date") {
        let at = text(when, "at").unwrap_or_default();
        return pad_date(parse_time(at)).map_or_else(|| format!("到 {at}"), |date| format!("到 {date}"));
    }
    let target = title(text(when, "target").unwrap_or_default());
    match text(when, "on") {
        Some("done") => format!("{target} 完成时"),
        Some("activity") => format!("{target} 有新动静时"),
        _ => format!("{target} 出现{}信号时", text(when, "kind").filter(|kind| !kind.is_empty()).map(|kind| format!(" {kind} ")).unwrap_or_default()),
    }
}

fn rhythm_items(context: &Context, view: Option<&Value>, quiet: Option<&Value>, items: &mut Vec<Item>) {
    let Some((objects, due)) = rhythm(quiet) else { return };
    let project = objects.first().and_then(|object| text(object, "id"));
    let delegated = view.map(|view| array(view.get("objects"))).unwrap_or(&[]);
    let title = |id: &str| -> String {
        if Some(id) == project { return "项目".into() }
        delegated.iter().find(|object| text(object, "id") == Some(id)).and_then(|object| text(object, "title")).unwrap_or(id).to_owned()
    };
    let place = |item: &mut Item, target: Option<&str>| {
        let (placed, areas) = target.map(|target| context.place(target)).unwrap_or_default();
        let known = target.is_some_and(|target| delegated.iter().any(|object| text(object, "id") == Some(target)));
        item.delegation = if known { target.map(str::to_owned) } else { None };
        item.object = match target {
            Some(target) if Some(target) != project && placed.is_empty() => Some(target.to_owned()),
            _ => None,
        };
        item.objects = placed;
        item.areas = areas;
    };
    let follow = follow_ups(context.projection);
    for id in due {
        let Some(entry) = follow.iter().find(|entry| text(entry, "id") == Some(id) && !truthy(entry.get("done"))) else { continue };
        let target = text(&entry["when"], "target");
        let back = objects.iter().find(|object| {
            let reason = object.get("resurfaced").unwrap_or(&Value::Null);
            text(object, "id") == target && target.is_some() && text(reason, "by") == Some("follow_up") && text(reason, "follow_up") == Some(id)
        });
        let mut item = Item::new(format!("follow-up:{id}"), Kind::FollowUp, text(entry, "text").unwrap_or_default(),
            timestamp(entry["added"].get("at")), "回头看一眼；看完点「完成跟进」。");
        let back_text = back.map(|object| format!("；{} 已休眠，现在浮现", title(text(object, "id").unwrap_or_default()))).unwrap_or_default();
        item.detail = Some(format!("你设的跟进：{}{back_text}", when_label(&entry["when"], &title)));
        place(&mut item, target);
        items.push(item);
    }
    for object in &objects {
        let Some(reason) = object.get("resurfaced").filter(|reason| reason.is_object()) else { continue };
        let by = text(reason, "by").unwrap_or_default();
        if by == "follow_up" { continue }
        let id = text(object, "id").unwrap_or_default();
        let at = if by == "connector" { timestamp(reason.get("event").and_then(|event| event.get("at"))) }
            else { timestamp(object.get("last").and_then(|last| last.get("at"))) };
        let detail = match by {
            "check_every" => format!("到了 {} 天的复查节奏", reason.get("days").map(js_string).unwrap_or_default()),
            _ => format!("沉寂后有新动静（{} 自报）", reason.get("event").and_then(|event| text(event, "actor")).unwrap_or_default()),
        };
        let mut item = Item::new(format!("resurfaced:{id}"), Kind::Resurfaced, format!("{} 重新浮现", title(id)), at,
            "看一眼上次到哪了，再决定接着做还是放下。");
        item.detail = Some(detail);
        place(&mut item, Some(id));
        items.push(item);
    }
}

/// FR-186: answered decisions whose review day has come, unless superseded
/// or settled. The card carries the decision and its reason; the options
/// stay with the question.
fn decision_reviews(context: &Context, now: u64, items: &mut Vec<Item>) {
    use crate::decision_review::{due, next_review, Review, DECISIONS, GROUP};
    let decisions = record(context.projection.get(DECISIONS));
    let superseded: BTreeSet<&str> = decisions.values().flat_map(|decision| array(decision.get("supersedes"))).filter_map(Value::as_str).collect();
    let reviews: BTreeMap<String, Review> = context.projection.get(GROUP).and_then(|group| serde_json::from_value(group.clone()).ok()).unwrap_or_default();
    let (asked, answered) = (questions(context.projection), answers(context.projection));
    for (id, decision) in decisions {
        if superseded.contains(id.as_str()) { continue }
        let Some(date) = next_review(text(decision, "review_on"), reviews.get(id)).filter(|date| due(date, now)) else { continue };
        let Some(question) = text(decision, "question") else { continue };
        let mut item = Item::new(format!("decision-review:{id}"), Kind::DecisionReview, question, parse_time(&format!("{date}T00:00:00Z")),
            "看当时的决定和理由再定");
        let reason = text(decision, "rationale").map(|reason| format!("，理由：{reason}")).unwrap_or_default();
        // The options come from the question this decision answered, when it is still in the projection.
        let options = decision.get("source").and_then(|source| text(source, "eid"))
            .and_then(|eid| answered.iter().find(|(_, answer)| text(answer, "event") == Some(eid)).map(|(hold, _)| *hold))
            .and_then(|hold| asked.get(hold)).map(|question| array(question["draft"].get("options")).iter().filter_map(Value::as_str).collect::<Vec<_>>().join("、"))
            .filter(|options| !options.is_empty()).map(|options| format!("当时的选项：{options}；")).unwrap_or_default();
        item.detail = Some(format!("{date} 到期复查；{options}当时定为“{}”{reason}", text(decision, "decision").unwrap_or_default()));
        item.object = Some(format!("决策 {id}"));
        item.areas = text(decision, "module").filter(|scope| !scope.is_empty()).map(|scope| vec![scope.to_owned()]).unwrap_or_default();
        items.push(item);
    }
}

/// FR-186: decisions a person recorded in the App and decision documents on
/// main with `review_on` come back like answered ones, under the same ID form.
fn recorded_decision_reviews(context: &Context, documents: Option<&Value>, now: u64, items: &mut Vec<Item>) {
    use crate::decision_review::{due, next_review, Document, Review, GROUP};
    let reviews: BTreeMap<String, Review> = context.projection.get(GROUP).and_then(|group| serde_json::from_value(group.clone()).ok()).unwrap_or_default();
    let mut push = |id: &str, own: Option<&str>, title: &str, detail: String| {
        let Some(date) = next_review(own, reviews.get(id)).filter(|date| due(date, now)) else { return };
        let mut item = Item::new(format!("decision-review:{id}"), Kind::DecisionReview, title, parse_time(&format!("{date}T00:00:00Z")),
            "看当时的决定和理由再定");
        item.detail = Some(format!("{date} 到期复查；{detail}"));
        item.object = Some(format!("决策 {id}"));
        items.push(item);
    };
    for (id, decision) in crate::human_decisions::read(context.projection).unwrap_or_default() {
        push(&id, decision.data.review_on.as_deref(), &decision.question, format!("当时定为“{}”", decision.data.text));
    }
    let documents: Vec<Document> = documents.and_then(|documents| serde_json::from_value(documents.clone()).ok()).unwrap_or_default();
    for document in documents {
        push(&document.id, Some(&document.review_on), &document.title, format!("见 {}", document.file));
    }
}

/// FR-135: quiet objects take no attention slot. An item is quiet when every
/// object it concerns is quiet; an item about no object concerns the project.
/// Class 1 and class 4 are never put aside.
fn quiet_item(item: &Item, quiet: Option<&Value>) -> bool {
    let Some((objects, _)) = rhythm(quiet) else { return false };
    if matches!(class(item), 1 | 4) { return false }
    let asleep: BTreeSet<&str> = objects.iter().filter(|object| object["quiet"] == Value::Bool(true)).filter_map(|object| text(object, "id")).collect();
    let concerns: BTreeSet<&str> = item.objects.iter().map(String::as_str).chain(item.delegation.as_deref()).chain(item.signal.as_deref()).collect();
    if concerns.is_empty() {
        return objects.first().and_then(|object| text(object, "id")).is_some_and(|project| asleep.contains(project));
    }
    concerns.iter().all(|id| asleep.contains(id))
}

/// FR-128: the person's budget as the daemon read it; never above the first screen.
pub fn budget(settings: Option<&Value>) -> usize {
    settings.and_then(|settings| settings.get("budget")).and_then(Value::as_u64).filter(|budget| *budget > 0)
        .map_or(FIRST_SCREEN, |budget| (budget as usize).min(FIRST_SCREEN))
}

/// Every candidate, unordered, from a ready state body.
pub fn candidates(state: &Value, kits: &[&dyn Kit], now: u64) -> Vec<Item> {
    let projection = state.get("projection").unwrap_or(&Value::Null);
    let context = Context { projection, kits };
    let view = state.get("delegation_view").filter(|view| !view.is_null());
    let mut items = Vec::new();
    alerts(&context, &mut items);
    holds(&context, &mut items);
    proposals(&context, &mut items);
    reviews(&context, &mut items);
    delegation(&context, view, &mut items);
    signal_items(&context, now, &mut items);
    waivers(&context, now, &mut items);
    rhythm_items(&context, view, state.get("quiet"), &mut items);
    decision_reviews(&context, now, &mut items);
    recorded_decision_reviews(&context, state.get(crate::decision_review::DOCUMENTS), now, &mut items);
    items
}

/// The ordered attention list of a state body; empty unless the state is ready.
pub fn list(state: &Value, kits: &[&dyn Kit], now: u64) -> List {
    let budget = budget(state.get("attention"));
    if state.get("status").and_then(Value::as_str) != Some("ready") {
        return List { items: vec![], shown: 0, asleep: 0, budget };
    }
    let mut items = candidates(state, kits, now);
    items.sort_by(compare);
    let quiet = state.get("quiet");
    let (awake, asleep): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|item| !quiet_item(item, quiet));
    let shown = budget.min(awake.len());
    let asleep_count = asleep.len();
    let entry = |item: Item, quiet: bool| {
        let card = card_text(&card(&item), CARD_CHARS);
        Entry { label: item.kind.label(), symbol: item.kind.symbol(), class: class(&item), timing: timing(&item), quiet, card, item }
    };
    let items = awake.into_iter().map(|item| entry(item, false)).chain(asleep.into_iter().map(|item| entry(item, true))).collect();
    List { items, shown, asleep: asleep_count, budget }
}

// ---------- digest (FR-131) ----------

/// FR-119, FR-130: passive signals in the window, for the digest only. Three
/// or more senders on one target become one card.
pub fn passive_cards(group: Option<&Value>, since: u64, now: u64) -> Vec<(String, String)> {
    let mut by_target: BTreeMap<&str, Vec<Signal>> = BTreeMap::new();
    for signal in signals(group) {
        if signal.level == 1 && !signal_quiet(&signal, now) && signal.last_at >= since && signal.last_at > 0 {
            by_target.entry(signal.target).or_default().push(signal);
        }
    }
    by_target.into_iter().flat_map(|(target, entries)| {
        let senders: BTreeSet<&str> = entries.iter().map(|entry| entry.from).collect();
        if senders.len() >= 3 {
            let titles = entries.iter().take(2).map(|entry| entry.title).collect::<Vec<_>>().join("、");
            let more = if entries.len() > 2 { " 等" } else { "" };
            return vec![(target.to_owned(), format!("{target}：{} 个来源的 {} 条提醒（{titles}{more}）", senders.len(), entries.len()))];
        }
        entries.iter().map(|entry| {
            let by = if entry.actor == TOWER { "Tower.md 判断".to_owned() } else { format!("{} 自报", entry.from) };
            (target.to_owned(), format!("{target}：{}（{by}）", entry.title))
        }).collect()
    }).collect()
}

/// FR-139: one change summary that arrived in the window, as sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Change {
    pub target: String,
    pub from: String,
    pub at: String,
    /// `label：value`, at most seven, in the sender's order.
    pub lines: Vec<String>,
    /// The whole summary on one line, cut for the digest.
    pub text: String,
}

/// FR-139: the latest change summary per sender and target received since
/// `since`, in arrival order. The core shows them and never interprets them.
pub fn changes(group: Option<&Value>, since: u64) -> Vec<Change> {
    use crate::tower_api::{summary_line, ChangeSummary, Merged};
    array(group.and_then(|group| group.get("summaries"))).iter()
        .filter_map(|entry| serde_json::from_value::<Merged<ChangeSummary>>(entry.clone()).ok())
        .filter(|entry| { let at = parse_time(&entry.last.at); at > 0 && at >= since })
        .map(|entry| Change { target: entry.target.clone(), from: entry.from.id.clone(), at: entry.last.at.clone(),
            lines: entry.data.texts(), text: summary_line(&entry) })
        .collect()
}

/// An object that cannot move on: a kit's own stuck objects, then objects
/// whose delegate answered a report request with `blocked`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stuck {
    pub id: String,
    pub title: String,
}

/// Stuck objects of a state body, for the digest (FR-131) and health (FR-39).
pub fn stuck(state: &Value, kits: &[&dyn Kit]) -> Vec<Stuck> {
    let projection = state.get("projection").unwrap_or(&Value::Null);
    let view = state.get("delegation_view").filter(|view| view.is_object());
    let mut found: Vec<Stuck> = kits.iter().flat_map(|kit| kit.stuck(projection)).collect();
    for request in view.map(|view| array(view.get("report_requests"))).unwrap_or(&[]) {
        let blocked = request.get("open").and_then(Value::as_bool) == Some(true)
            && request.get("reply").and_then(|reply| reply.get("data")).and_then(|data| text(data, "state")) == Some("blocked");
        if let Some(object) = text(request, "object").filter(|_| blocked) {
            found.push(Stuck { id: object.to_owned(), title: object.to_owned() });
        }
    }
    found
}

/// Pending proposals a delegate waits on: they came from answering its question.
pub fn blocking_proposals(projection: &Value) -> BTreeSet<String> {
    answers(projection).values().filter_map(|answer| text(answer, "proposal").map(str::to_owned)).collect()
}

/// FR-39 yellow: targets with unhandled `passive` signals from three or more senders.
pub fn crowded_targets(group: Option<&Value>, now: u64) -> Vec<String> {
    let mut by_target: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for signal in signals(group) {
        if signal.level == 1 && !signal_quiet(&signal, now) {
            by_target.entry(signal.target).or_default().insert(signal.from);
        }
    }
    by_target.into_iter().filter(|(_, senders)| senders.len() >= 3).map(|(target, _)| target.to_owned()).collect()
}

/// FR-133: one item of the cross-project list, tagged with its project.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProjectEntry {
    pub project: String,
    #[serde(flatten)]
    pub entry: Entry,
}

/// FR-133: every project's items in one FR-128 order, awake before quiet;
/// still at most `FIRST_SCREEN` on the first screen, never above the
/// smallest budget a project set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Across {
    pub items: Vec<ProjectEntry>,
    pub shown: usize,
    pub budget: usize,
}

pub fn across(lists: &[(String, List)]) -> Across {
    let budget = lists.iter().map(|(_, list)| list.budget).min().unwrap_or(FIRST_SCREEN).min(FIRST_SCREEN);
    let mut items: Vec<ProjectEntry> = lists.iter().flat_map(|(project, list)| list.items.iter()
        .map(|entry| ProjectEntry { project: project.clone(), entry: entry.clone() })).collect();
    items.sort_by(|a, b| a.entry.quiet.cmp(&b.entry.quiet).then_with(|| compare(&a.entry.item, &b.entry.item)).then_with(|| a.project.cmp(&b.project)));
    let shown = budget.min(items.iter().filter(|item| !item.entry.quiet).count());
    Across { items, shown, budget }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Digest {
    pub heading: String,
    /// What finished, where things are stuck, what needs you.
    pub lines: [String; 3],
    pub details: Vec<String>,
    /// FR-139: change summaries in the window; absent when there are none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<Change>,
    pub since: u64,
    pub fallback: bool,
}

fn names(items: &[String], coarse: bool, unit: &str) -> String {
    if coarse || items.len() > 2 { format!("{} {unit}", items.len()) } else { items.join("、") }
}

/// FR-131: three lines; the longer you were away, the coarser (counts instead of names).
pub fn digest(state: &Value, kits: &[&dyn Kit], list: &List, since: u64, fallback: bool, now: u64) -> Digest {
    let coarse = now.saturating_sub(since) > COARSE_AFTER_MS;
    let projection = state.get("projection").unwrap_or(&Value::Null);
    let view = state.get("delegation_view").filter(|view| view.is_object());
    let finished: Vec<String> = view.map(|view| array(view.get("objects"))).unwrap_or(&[]).iter().filter(|object| {
        array(object.get("attempts")).iter().any(|attempt| attempt.get("done").is_some_and(|done| {
            let at = timestamp(done.get("at"));
            at > 0 && at >= since
        }))
    }).filter_map(|object| text(object, "title").map(str::to_owned)).collect();
    let mut stuck: Vec<String> = stuck(state, kits).into_iter().map(|entry| entry.title).collect();
    let mut seen = BTreeSet::new();
    stuck.retain(|name| seen.insert(name.clone()));
    let passive = passive_cards(projection.get(crate::tower_api::GROUP), since, now);
    let changes = changes(projection.get(crate::tower_api::GROUP), since);
    let shown = list.shown;
    let later = list.items.len() - shown;
    let heading = if fallback { "最近 7 天".to_owned() } else { format!("自上次查看以来（{} 起）", when(since)) };
    let first = if finished.is_empty() { "完成了：没有新完成的事".to_owned() } else { format!("完成了：{}", names(&finished, coarse, "件")) };
    let second = if stuck.is_empty() && passive.is_empty() { "卡在：没有卡住的事".to_owned() } else {
        let names = if stuck.is_empty() { "没有卡住的事".to_owned() } else { names(&stuck, coarse, "件") };
        let extra = if passive.is_empty() { String::new() } else { format!("；另有 {} 条提醒", passive.len()) };
        format!("卡在：{names}{extra}")
    };
    let third = if shown == 0 { "需要你：暂时没有".to_owned() } else {
        format!("需要你：{shown} 件{}", if later > 0 { format!("，另有 {later} 条已延后") } else { String::new() })
    };
    let details = finished.iter().map(|title| format!("完成 · {title}"))
        .chain(stuck.iter().map(|title| format!("卡住 · {title}")))
        .chain(passive.iter().map(|(_, text)| format!("提醒 · {text}")))
        .chain(changes.iter().map(|change| format!("变化 · {}", change.text))).collect();
    Digest { heading, lines: [first, second, third], details, changes, since, fallback }
}

#[cfg(test)]
mod tests {
    //! Ported from the App's `attention.test.ts`, which this module replaces as the source of order.
    use super::*;
    use serde_json::json;

    fn item(id: &str, kind: Kind) -> Item {
        Item::new(id, kind, id, 0, "处理")
    }
    fn with(mut item: Item, change: impl FnOnce(&mut Item)) -> Item {
        change(&mut item);
        item
    }
    fn length(text: &str) -> usize {
        text.chars().count()
    }
    fn utc(text: &str) -> u64 {
        parse_time(text)
    }

    /// Places `T-*` objects of a `Task` group, as a kit would.
    struct Places;
    impl Kit for Places {
        fn locate(&self, projection: &Value, object: &str) -> Option<Located> {
            let input = projection.get("Task")?.get(object)?.get("input")?;
            Some(Located { area: input["area"].as_str()?.into(), reaches: strings(input.get("reaches")) })
        }
        fn alert(&self, reason: &str) -> Option<AlertText> {
            (reason == "kit_check").then(|| AlertText { kind: Kind::CheckFailed, title: Some("检查未通过".into()), action: "看证据。".into() })
        }
        fn stuck(&self, projection: &Value) -> Vec<Stuck> {
            record(projection.get("Task")).iter().filter(|(_, task)| task["state"] == "held")
                .filter_map(|(id, task)| Some(Stuck { id: id.clone(), title: task["input"]["title"].as_str()?.into() })).collect()
        }
    }
    const KITS: &[&dyn Kit] = &[&Places];

    #[test]
    fn cards_keep_six_segments_in_order_within_100_characters() {
        let question = with(item("q", Kind::Question), |item| {
            item.title = "是否沿用旧接口".into();
            item.objects = vec!["T-1".into()];
            item.at = parse_time("2026-10-23T08:05:00");
            item.action = "打开问题回答。".into();
        });
        let built = card(&question);
        assert_eq!((built.when.as_str(), built.object.as_str(), built.why.as_str(), built.ask.as_str()), ("10-23 08:05", "任务 T-1", "受托者等你回答才能继续", "回答"));
        assert_eq!(card_text(&built, CARD_CHARS), "10-23 08:05 · 任务 T-1：是否沿用旧接口。因为受托者等你回答才能继续。建议打开问题回答。需要你：回答。");
        let alert = card(&with(item("x", Kind::Alert), |item| { item.object = Some("ops/disk".into()); item.detail = Some("ci 自报".into()) }));
        assert_eq!((alert.when.as_str(), alert.object.as_str(), alert.why.as_str()), ("时间未知", "ops/disk", "ci 自报"));
        assert_eq!(card(&with(item("m", Kind::Proposal), |item| item.areas = vec!["A".into(), "B".into()])).object, "模块 A、B");
        assert_eq!(card(&item("p", Kind::Alert)).object, "项目");
        assert_eq!(card(&with(item("t", Kind::Alert), |item| item.objects = vec!["T-1".into(), "T-2".into()])).object, "T-1 等 2 个任务");
        let weakened = with(item("w", Kind::Weakening), |item| {
            item.title = "承诺弱化：1 处".into();
            item.object = Some("提案 P-1".into());
            item.detail = Some("门禁 crap（eng.crap）由硬改软".into());
            item.action = "打开提案核对".into();
        });
        assert_eq!(card_text(&card(&weakened), CARD_CHARS), "时间未知 · 提案 P-1：承诺弱化：1 处。因为门禁 crap（eng.crap）由硬改软。建议打开提案核对。需要你：核对后驳回、要求恢复或接受。");
        assert_eq!(card(&item("w", Kind::Weakening)).why, "完成标准、检查或门禁被放宽");
    }

    #[test]
    fn cards_shorten_the_longest_segments_first_and_never_the_time() {
        let built = Card { when: "10-23 08:05".into(), object: "T-1".into(), what: "长".repeat(80), why: "原因".repeat(30), suggest: "建议".into(), ask: "回答".into() };
        let text = card_text(&built, CARD_CHARS);
        assert!(length(&text) <= 100, "{text}");
        assert!(text.starts_with("10-23 08:05 · T-1："));
        assert!(text.contains("建议建议。需要你：回答。"));
        assert!(text.contains("长…。因为原因") && text.contains("…。建议"), "{text}");
        for limit in [60, 80, 100] {
            assert!(length(&card_text(&built, limit)) <= limit);
        }
    }

    #[test]
    fn order_puts_protection_and_time_sensitive_first_then_longest_wait_then_widest_reach() {
        let areas = |names: &[&str]| names.iter().map(|name| name.to_string()).collect::<Vec<_>>();
        let mut items = vec![
            with(item("warn-wide", Kind::Alert), |item| { item.areas = areas(&["A", "B", "C"]); item.at = 5 }),
            with(item("warn-narrow", Kind::Alert), |item| { item.areas = areas(&["A"]); item.at = 1 }),
            with(item("question-new", Kind::Question), |item| { item.at = 300; item.areas = areas(&["A", "B"]) }),
            with(item("proposal-old", Kind::Proposal), |item| item.at = 100),
            item("question-unknown", Kind::Question),
            with(item("urgent", Kind::Alert), |item| item.urgent = true),
            with(item("protected", Kind::Blocked), |item| item.at = 9),
            with(item("weakened", Kind::Weakening), |item| item.at = 20),
        ];
        items.sort_by(compare);
        let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["protected", "weakened", "urgent", "proposal-old", "question-new", "question-unknown", "warn-wide", "warn-narrow"]);
        assert_eq!([class(&item("f", Kind::FollowUp)), class(&item("r", Kind::Resurfaced)), class(&item("s", Kind::SpotCheck))], [4, 4, 3]);
        // A permission request is denied when nobody answers in time.
        assert_eq!(class(&item("k", Kind::Permission)), 1);
        assert_eq!((Kind::Permission.label(), Kind::Permission.symbol()), ("授权", "⚿"));
    }

    fn signal(from: &str, kind: &str, target: &str, at: &str, level: &str) -> Value {
        json!({ "from": { "id": from, "kind": "source" }, "target": target, "count": 1, "data": { "kind": kind, "level": level, "title": format!("{kind} 提醒") },
            "last": { "at": at, "eid": format!("{from}-{kind}"), "seq": 1, "actor": "process:towermd-cli" } })
    }

    #[test]
    fn passive_cards_merge_three_senders_and_skip_old_quiet_and_louder_ones() {
        let now = utc("2026-10-23T12:00:00Z");
        let at = "2026-10-23T10:00:00Z";
        let mut seen = signal("ci", "eng.seen", "T-2", at, "passive");
        seen["acknowledged"] = json!({ "at": "2026-10-23T11:00:00Z", "eid": "a", "actor": "human:local" });
        let group = json!({ "signals": [
            signal("ci", "eng.lint", "T-1", at, "passive"), signal("bot", "eng.size", "T-1", at, "passive"), signal("scan", "eng.deps", "T-1", at, "passive"),
            signal("ci", "eng.lint", "T-2", at, "passive"), signal("ci", "eng.old", "T-2", "2026-10-01T00:00:00Z", "passive"),
            signal("ci", "eng.loud", "T-2", at, "active"), seen,
        ] });
        assert_eq!(passive_cards(Some(&group), utc("2026-10-22T00:00:00Z"), now), [
            ("T-1".to_owned(), "T-1：3 个来源的 3 条提醒（eng.lint 提醒、eng.size 提醒 等）".to_owned()),
            ("T-2".to_owned(), "T-2：eng.lint 提醒（ci 自报）".to_owned()),
        ]);
        let mut own = signal("process:towermd", "core.silent", "T-3", at, "passive");
        own["last"]["actor"] = json!(TOWER);
        assert_eq!(passive_cards(Some(&json!({ "signals": [own] })), utc("2026-10-22T00:00:00Z"), now),
            [("T-3".to_owned(), "T-3：core.silent 提醒（Tower.md 判断）".to_owned())]);
    }

    fn ready(projection: Value, view: Value, budget: u64) -> Value {
        json!({ "status": "ready", "projection": projection, "delegation_view": view, "attention": { "budget": budget, "focus_time": [] } })
    }
    fn alerts_projection(count: usize) -> Value {
        let alerts: Map<String, Value> = (1..=count).map(|n| (format!("a{n}"), json!({ "data": { "reason": "disk" } }))).collect();
        json!({ "Alert": alerts })
    }
    fn finished(id: &str, at: &str) -> Value {
        json!({ "id": id, "title": format!("完成 {id}"), "attempts": [{ "done": { "at": at, "eid": format!("D-{id}"), "seq": 3 } }] })
    }

    #[test]
    fn digest_names_things_after_a_short_absence_and_counts_them_after_a_long_one() {
        let now = utc("2026-10-23T12:00:00Z");
        let view = json!({ "objects": [finished("T-1", "2026-10-23T09:00:00Z"), finished("T-0", "2026-10-01T09:00:00Z")], "report_requests": [] });
        let mut projection = alerts_projection(5);
        projection["Task"] = json!({ "T-2": { "state": "held", "input": { "title": "任务 T-2" } }, "T-3": { "state": "running", "input": { "title": "任务 T-3" } } });
        let state = ready(projection, view.clone(), 3);
        let short = digest(&state, KITS, &list(&state, KITS, now), utc("2026-10-23T08:00:00Z"), false, now);
        assert!(short.heading.starts_with("自上次查看以来（10-23 ") && short.heading.ends_with(":00 起）"), "{}", short.heading);
        assert_eq!(short.lines, ["完成了：完成 T-1", "卡在：任务 T-2", "需要你：3 件，另有 2 条已延后"]);
        assert_eq!(short.details, ["完成 · 完成 T-1", "卡住 · 任务 T-2"]);
        let mut projection = json!({ "Task": { "T-2": { "state": "held", "input": { "title": "任务 T-2" } } } });
        projection[crate::tower_api::GROUP] = json!({ "signals": [{ "from": { "id": "ci", "kind": "source" }, "target": "T-9", "count": 1,
            "data": { "kind": "eng.lint", "level": "passive", "title": "警告" }, "last": { "at": "2026-10-22T00:00:00Z", "eid": "e", "seq": 1, "actor": "process:towermd-cli" } }] });
        let state = ready(projection, view, 3);
        let long = digest(&state, KITS, &list(&state, KITS, now), utc("2026-10-20T08:00:00Z"), false, now);
        assert_eq!(long.lines, ["完成了：1 件", "卡在：1 件；另有 1 条提醒", "需要你：暂时没有"]);
        assert!(long.details.contains(&"提醒 · T-9：警告（ci 自报）".to_owned()));
    }

    #[test]
    fn digest_counts_the_first_screen_against_the_budget_and_admits_the_fallback() {
        let now = utc("2026-10-23T12:00:00Z");
        let state = ready(alerts_projection(5), Value::Null, 1);
        let listed = list(&state, KITS, now);
        assert_eq!((listed.shown, listed.budget, listed.items.len()), (1, 1, 5));
        assert_eq!(digest(&state, KITS, &listed, now - 3_600_000, false, now).lines[2], "需要你：1 件，另有 4 条已延后");
        let empty = ready(json!({}), Value::Null, 3);
        let fallback = digest(&empty, KITS, &list(&empty, KITS, now), now - FALLBACK_MS, true, now);
        assert_eq!((fallback.heading.as_str(), fallback.lines.clone(), fallback.details.len()),
            ("最近 7 天", ["完成了：没有新完成的事".to_owned(), "卡在：没有卡住的事".to_owned(), "需要你：暂时没有".to_owned()], 0));
    }

    #[test]
    fn times_are_local_and_unknown_ones_are_admitted() {
        assert_eq!(when(0), "时间未知");
        let at = Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 0).unwrap().timestamp_millis() as u64;
        assert_eq!(when(at), "01-02 03:04");
    }

    #[test]
    fn budget_is_lenient_and_never_grows_the_first_screen() {
        let budgets = [None, Some(json!({ "budget": 9 })), Some(json!({ "budget": 0 })), Some(json!({ "budget": 1.5 })), Some(json!({ "budget": 2 }))];
        assert_eq!(budgets.iter().map(|value| budget(value.as_ref())).collect::<Vec<_>>(), [3, 3, 3, 3, 2]);
    }

    #[test]
    fn only_time_sensitive_items_interrupt_now() {
        assert_eq!([timing(&with(item("u", Kind::Blocked), |item| item.urgent = true)), timing(&item("q", Kind::Question))], [Timing::Now, Timing::NextVisit]);
    }

    #[test]
    fn list_assembles_core_items_places_them_by_kit_and_puts_quiet_ones_last() {
        let now = utc("2026-10-23T12:00:00Z");
        let projection = json!({
            "Task": { "T-1": { "state": "running", "input": { "title": "一", "area": "A", "reaches": ["B"] } } },
            "Alert": {
                "x1": { "at": "2026-10-23T01:00:00Z", "data": { "reason": "kit_check", "detail": { "task": "T-1", "error": "退出码 1" } } },
                "x2": { "data": { "reason": "unapproved_protected_change" } },
                "x3": { "resolved": true, "data": { "reason": "disk" } },
            },
            "Hold": { "h": { "opened_at": "2026-10-23T02:00:00Z", "draft": { "id": "Q-1", "text": "选哪个", "options": [], "context": [] }, "binding": { "module": "A", "task": "T-1" }, "targets": [] } },
            "Proposal": { "P-1": { "status": "submitted", "at": "2026-10-23T03:00:00Z", "proposal": { "reason": "改接口", "impact": { "modules": ["A"], "tasks": ["T-2"] } } },
                "P-0": { "status": "approved" } },
            "TowerApi": { "signals": [
                { "from": { "id": "process:towermd", "kind": "tower" }, "target": "P-1", "count": 1, "data": { "kind": WEAKENING, "level": "active", "title": "承诺弱化：1 处" },
                  "last": { "at": "2026-10-23T04:00:00Z", "eid": "w", "seq": 9, "actor": TOWER } },
                { "from": { "id": "ci", "kind": "source" }, "target": "T-quiet", "count": 2, "data": { "kind": "eng.lint", "level": "active", "title": "lint", "value": 3, "threshold": 1 },
                  "last": { "at": "2026-10-23T05:00:00Z", "eid": "l", "seq": 10, "actor": "process:towermd-cli" } },
            ] },
        });
        let view = json!({ "objects": [], "spot_checks": [], "requests": [{ "id": "R-1", "open": true, "recipients": [], "data": { "target": "ops", "text": "谁来答" }, "event": { "at": "2026-10-23T06:00:00Z" } }] });
        let mut state = ready(projection, view, 3);
        state["quiet"] = json!({ "dormant_after": 14, "objects": [{ "id": "proj", "quiet": false, "last": null }, { "id": "T-quiet", "quiet": true, "last": null }], "due": [] });
        let listed = list(&state, KITS, now);
        let ids: Vec<&str> = listed.items.iter().map(|entry| entry.item.id.as_str()).collect();
        assert_eq!(ids, ["signal:process:towermd:core.weakening:P-1", "x2", "Q-1", "P-1", "x1", "unanswerable:R-1", "signal:ci:eng.lint:T-quiet"]);
        assert_eq!((listed.shown, listed.asleep), (3, 1));
        let checked = &listed.items[4];
        assert_eq!((checked.label, checked.item.areas.clone(), checked.item.objects.clone(), checked.class), ("检查未通过", vec!["A".to_owned(), "B".to_owned()], vec!["T-1".to_owned()], 3));
        assert_eq!(listed.items[0].item.object.as_deref(), Some("提案 P-1"));
        let lint = listed.items.last().unwrap();
        assert!(lint.quiet && lint.item.title == "lint ×2" && lint.item.detail.as_deref() == Some("ci 自报 · eng.lint · 3 / 1"));
        assert_eq!(listed.items[5].item.object.as_deref(), Some("ops"));
        assert!(listed.items.iter().all(|entry| length(&entry.card) <= CARD_CHARS));
        assert_eq!(list(&json!({ "status": "loading" }), KITS, now).items.len(), 0);
    }

    fn stamp(seq: u64) -> Value {
        json!({ "at": "2026-10-20T10:00:00Z", "eid": format!("e{seq}"), "seq": seq, "actor": "human:local" })
    }
    fn lint(target: &str, title: &str, seq: u64) -> Value {
        json!({ "from": { "id": "ci", "kind": "source" }, "target": target, "count": 1, "data": { "kind": "ci", "level": "active", "title": title }, "last": stamp(seq) })
    }

    /// The state of `follow-ups.test.ts` "keeps items the daemon put aside off the first screen".
    #[test]
    fn quiet_items_wait_under_later() {
        let now = utc("2026-10-23T12:00:00Z");
        let task = |id: &str| json!({ "state": "running", "input": { "id": id, "title": id, "area": "A", "reaches": [] } });
        let projection = json!({
            "Task": { "T-1": task("T-1"), "T-3": task("T-3") },
            "FollowUp": { "F2": { "id": "F2", "text": "跟进 F2", "when": { "on": "date", "at": "2026-10-01T00:00:00Z" }, "added": stamp(1) } },
            "TowerApi": { "signals": [lint("T-3", "休眠任务上的提醒", 8), lint("T-1", "醒着的提醒", 9)] },
        });
        let mut state = ready(projection, Value::Null, 3);
        state["quiet"] = json!({ "dormant_after": 14, "due": ["F2"], "objects": [
            { "id": "proj", "last": stamp(9), "quiet": false, "resurfaced": null }, { "id": "T-3", "last": stamp(1), "quiet": true, "resurfaced": null }] });
        let listed = list(&state, KITS, now);
        let titles: Vec<&str> = listed.items.iter().map(|entry| entry.item.title.as_str()).collect();
        assert_eq!(titles, ["醒着的提醒", "跟进 F2", "休眠任务上的提醒"]);
        let ids: Vec<&str> = listed.items.iter().map(|entry| entry.item.id.as_str()).collect();
        assert_eq!(ids, ["signal:ci:ci:T-1", "follow-up:F2", "signal:ci:ci:T-3"]);
        assert_eq!((listed.shown, listed.asleep), (2, 1));
        assert_eq!(listed.items.iter().map(|entry| entry.quiet).collect::<Vec<_>>(), [false, false, true]);
    }

    /// FR-135: what counts as asleep, item by item.
    #[test]
    fn an_item_is_quiet_only_when_everything_it_concerns_is_quiet() {
        let rhythm = |project_quiet: bool| json!({ "dormant_after": 14, "due": [], "objects": [
            { "id": "proj", "quiet": project_quiet, "last": null }, { "id": "T-3", "quiet": true, "last": null }, { "id": "T-1", "quiet": false, "last": null }] });
        let asleep = rhythm(false);
        let on = |objects: &[&str]| with(item("x", Kind::Alert), |item| item.objects = objects.iter().map(|id| (*id).to_owned()).collect());
        assert!(quiet_item(&on(&["T-3"]), Some(&asleep)));
        assert!(!quiet_item(&on(&["T-3", "T-1"]), Some(&asleep)));
        assert!(quiet_item(&with(item("d", Kind::SpotCheck), |item| item.delegation = Some("T-3".into())), Some(&asleep)));
        assert!(quiet_item(&with(item("s", Kind::Alert), |item| item.signal = Some("T-3".into())), Some(&asleep)));
        // No object: the item is about the project.
        assert!(!quiet_item(&on(&[]), Some(&asleep)));
        assert!(quiet_item(&on(&[]), Some(&rhythm(true))));
        // Class 1 and class 4 are never put aside.
        assert!(!quiet_item(&with(item("b", Kind::Blocked), |item| item.objects = vec!["T-3".into()]), Some(&asleep)));
        assert!(!quiet_item(&with(on(&["T-3"]), |item| item.urgent = true), Some(&asleep)));
        assert!(!quiet_item(&with(item("f", Kind::FollowUp), |item| item.objects = vec!["T-3".into()]), Some(&asleep)));
        // An older daemon sends no rhythm: nothing is quiet.
        assert!(!quiet_item(&on(&["T-3"]), None));
    }

    #[test]
    fn across_projects_keeps_one_order_and_at_most_three_awake_on_the_first_screen() {
        let entry = |item: Item, quiet: bool| Entry { label: item.kind.label(), symbol: item.kind.symbol(), class: class(&item),
            timing: timing(&item), quiet, card: String::new(), item };
        let list = |items: Vec<Entry>, budget: usize| List { shown: budget.min(items.len()), asleep: 0, budget, items };
        let a = list(vec![entry(with(item("q", Kind::Question), |item| item.at = 5), false), entry(item("w", Kind::Alert), false)], 3);
        let b = list(vec![entry(item("x", Kind::Blocked), false), entry(with(item("p", Kind::Proposal), |item| item.at = 1), false),
            entry(item("old", Kind::Blocked), true)], 3);
        let merged = across(&[("a".into(), a.clone()), ("b".into(), b)]);
        let ids: Vec<(&str, &str)> = merged.items.iter().map(|entry| (entry.project.as_str(), entry.entry.item.id.as_str())).collect();
        assert_eq!(ids, [("b", "x"), ("b", "p"), ("a", "q"), ("a", "w"), ("b", "old")], "quiet items wait after every awake one");
        assert_eq!((merged.shown, merged.budget), (3, 3));
        let tight = across(&[("a".into(), a.clone()), ("c".into(), list(vec![], 1))]);
        assert_eq!(tight.shown, 1, "the smallest budget a person set holds across projects");
        assert_eq!(across(&[]).shown, 0);
        let json = serde_json::to_value(&merged.items[0]).unwrap();
        assert_eq!((json["project"].as_str(), json["id"].as_str(), json["kind"].as_str()), (Some("b"), Some("x"), Some("blocked")));
    }
}
