//! Tower API (core spec §5, FR-114–119): one envelope for every message, the
//! `report`, `signal` and `summary` (FR-139) verbs, four signal levels and
//! signal merging. The envelope's `from` is self-reported; the event's signer is the entry that
//! accepted it (`process:<id>` or `<producer>/<version>`), never `human:`.
//! A report never completes anything: `claimed_done` is only a claim. The
//! `TowerApi` group is created on first use, so older projections stay
//! byte-identical. A person quiets a merged signal only from the App
//! (`signal.dismissed`, signed `human:<id>`); the CLI has no such verb.
use std::collections::BTreeMap;

use chrono::{DateTime, Duration, SecondsFormat};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    delegation::parse_time,
    delegation_projection::{Mark, ReportState, SUMMARY_CHARS},
    hub::Event,
    question_model::{human_actor, nonempty, valid_id},
    requests, stuck,
    weakening::{self, Raised},
};

pub const GROUP: &str = "TowerApi";
pub const VERSION: u32 = 1;
/// Fast-layer progress: no person sets it, nothing is protected by it.
pub const LAYER: &str = "L5";
/// FR-115: a signal title has at most this many characters.
pub const TITLE_CHARS: usize = 30;
/// FR-115: a signal detail has at most this many characters.
pub const DETAIL_CHARS: usize = 100;
/// Tower.md itself, as the sender of outbound messages.
pub const TOWER: &str = "process:towermd";
/// Signal kinds in this namespace are raised by Tower.md only (FR-125–137).
pub const CORE_NAMESPACE: &str = "core.";
/// A person's "知道了" or "静音" on one merged signal.
pub const DISMISSED: &str = "signal.dismissed";
/// A mute ends at most this many days after the person set it.
pub const MUTE_DAYS: i64 = 30;
/// FR-139: a change summary has at most this many lines.
pub const SUMMARY_LINES: usize = 7;
/// FR-139: a summary line's label has at most this many characters.
pub const LABEL_CHARS: usize = 20;
/// FR-139: a summary line's text value has at most this many characters.
pub const VALUE_CHARS: usize = 60;
/// Where a summary is shown on one line (the digest), it is cut to this.
pub const SUMMARY_LINE_CHARS: usize = 120;

/// Inbound verbs this core records, and the outbound `request_report`.
/// `card` (FR-147) is recorded by `cards`, one event kind per operation;
/// `summary` (FR-139) is an object's change summary. `reply` (FR-115)
/// answers one open request and is recorded by `requests` as
/// `request.replied`, its body rewritten to [`requests::Reply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verb { Report, Signal, RequestReport, Card, Summary, Reply }

impl Verb {
    /// The event kind an accepted inbound message is recorded as; `card`
    /// depends on the operation (`Envelope::kind`).
    pub fn event_kind(self) -> Option<&'static str> {
        match self {
            Verb::Report => Some("report.received"),
            Verb::Signal => Some("signal.received"),
            Verb::Summary => Some("summary.received"),
            Verb::RequestReport | Verb::Card => None,
            Verb::Reply => Some(requests::REPLIED),
        }
    }
    fn of_event(kind: &str) -> Option<Self> {
        [Verb::Report, Verb::Signal, Verb::Summary].into_iter().find(|verb| verb.event_kind() == Some(kind))
    }
}

/// `delegate`: a person, AI agent or automation; `source`: a check or a
/// connector; `tower`: Tower.md itself, on outbound messages only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromKind { Delegate, Source, Tower }

/// Who says they sent the message. Self-reported, never a trusted signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct From {
    pub id: String,
    pub kind: FromKind,
}

/// FR-114. `data` is the verb's body; `at` is when the sender says it happened.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub v: u32,
    pub project: String,
    pub from: From,
    pub target: String,
    pub verb: Verb,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// FR-115 `report`. There is no `done`: completion is the core's judgement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub state: ReportState,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
    /// FR-127: the sender declares it answers questions on the target
    /// (`true`) or stops doing so (`false`). Sticky until changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<bool>,
}

/// FR-115 `reply`: the request it answers plus a report's fields. The
/// envelope's `target` is the request's object, its `refs` the answer's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub request_id: String,
    pub state: ReportState,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

impl Answer {
    pub fn check(&self) -> Result<(), String> {
        valid_id(&self.request_id)?;
        if self.request_id.starts_with(requests::CHECKPOINT_PREFIX) {
            return Err("检查点汇报请求只经命令行回应".into());
        }
        Report { state: self.state, summary: self.summary.clone(), next: self.next.clone(), progress: None, answers: None }.check()
    }
}

/// FR-118: the sender only declares a level; the core decides how to
/// interrupt. Names follow Apple's interruption levels; critical is unused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level { Log, Passive, Active, TimeSensitive }

/// FR-115 `signal`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signal {
    /// Namespaced, e.g. `eng.crap`.
    pub kind: String,
    pub level: Level,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<Value>,
    /// Declared by the sender; whether it gates anything is the person's
    /// `gates.yaml` (FR-122), never this flag alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deterministic: bool,
}

/// One line of a change summary: a label and a number or a short text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryLine {
    pub label: String,
    pub value: Value,
}

/// FR-139 `summary`: what changed in an object, in at most seven structured
/// lines. The core shows it in fixed places and never interprets it; the
/// full text and the diff stay with the object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeSummary {
    pub lines: Vec<SummaryLine>,
}

/// FR-119: one entry per sender, kind and target, keeping the latest
/// message and how often it arrived.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Merged<T> {
    pub from: From,
    pub target: String,
    pub data: T,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    /// The sender's own time of the latest message.
    pub at: String,
    pub count: u64,
    pub first: Mark,
    pub last: Mark,
    /// "知道了": the person saw the latest message. A newer one clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged: Option<Mark>,
    /// "静音": quiet until `until` while the level stays at or below `level`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<Muted>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Muted {
    pub until: String,
    /// The level when muted; a sender escalating above it breaks through.
    pub level: Level,
    pub event: Mark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quiet { Ack, Mute }

/// `signal.dismissed`: one merged signal by sender, kind and target, and the
/// latest message the person saw. Only the App sends it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dismissed {
    pub from: String,
    pub kind: String,
    pub target: String,
    pub mode: Quiet,
    /// Event ID of the latest message the person saw.
    pub seen: String,
    /// Mute only: when the mute ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TowerLog {
    /// Latest report per sender and target.
    #[serde(default)]
    pub reports: Vec<Merged<Report>>,
    /// Latest signal per sender, kind and target.
    #[serde(default)]
    pub signals: Vec<Merged<Signal>>,
    /// Every `core.weakening` in hub order, for the track record (FR-202).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weakenings: Vec<Raised>,
    /// FR-127: per target, the senders that declared `answers: true`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub answerers: BTreeMap<String, Vec<String>>,
    /// Every `core.silent` in hub order (FR-137).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub silences: Vec<Raised>,
    /// Every `core.stuck` in hub order, for the track record (FR-126, FR-202).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stuck: Vec<Raised>,
    /// FR-139: the latest change summary per sender and target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summaries: Vec<Merged<ChangeSummary>>,
}

impl TowerLog {
    /// FR-127: who declared they answer questions on `target`, sorted.
    pub fn answerers(&self, target: &str) -> Vec<String> {
        self.answerers.get(target).cloned().unwrap_or_default()
    }
}

pub fn read(state: &Value) -> Result<TowerLog, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(TowerLog::default()),
    }
}

/// Hub ID of a signal Tower.md raises for one source, so a retry or another
/// host raises it once.
pub fn signal_eid(kind: &str, source: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(kind.as_bytes());
    hash.update([0]);
    hash.update(source.as_bytes());
    format!("git:{}", hex::encode(hash.finalize()))
}

/// At most `chars` characters, the last one `…` when cut.
pub fn clip(text: &str, chars: usize) -> String {
    let all: Vec<char> = text.chars().collect();
    if all.len() <= chars {
        return text.to_owned();
    }
    all[..chars - 1].iter().chain(['…'].iter()).collect()
}

/// A signal raised by Tower.md itself, in the envelope it records.
pub fn tower_signal(project: &str, target: &str, refs: Vec<String>, at: String, signal: &Signal) -> Result<Envelope, String> {
    Ok(Envelope {
        v: VERSION, project: project.into(), from: From { id: TOWER.into(), kind: FromKind::Source },
        target: target.into(), verb: Verb::Signal, data: serde_json::to_value(signal).map_err(|e| e.to_string())?, refs, at,
    })
}

/// RFC 3339 in UTC with milliseconds, for envelopes the daemon dates.
pub fn timestamp(ms: u64) -> Result<String, String> {
    let at = i64::try_from(ms).ok().and_then(DateTime::from_timestamp_millis).ok_or("时间无效")?;
    Ok(at.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// The envelope signer: an automated entry or a producer, never a person.
pub fn check_signer(actor: &str) -> Result<(), String> {
    let part = |text: &str| !text.is_empty() && !text.contains([':', '/']) && !text.chars().any(|c| c.is_whitespace() || c.is_control());
    let valid = match actor.strip_prefix("process:") {
        Some(id) => part(id),
        None => actor.split_once('/').is_some_and(|(name, version)| part(name) && part(version)),
    };
    if !valid {
        return Err("接入接口的署名只能是 process:<id> 或 <producer>/<version>，不能是人".into());
    }
    Ok(())
}

fn token(text: &str, label: &str) -> Result<(), String> {
    if text.is_empty() || text.len() > 256 || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("{label}须是不含空白和控制字符的非空标识，且不超过 256 字节"));
    }
    Ok(())
}

fn bounded(text: &str, label: &str, chars: usize) -> Result<(), String> {
    nonempty(text, label)?;
    if text.chars().count() > chars {
        return Err(format!("{label}不能超过 {chars} 字"));
    }
    Ok(())
}

fn optional(text: &Option<String>, label: &str) -> Result<(), String> {
    text.as_deref().map_or(Ok(()), |text| nonempty(text, label))
}

fn scalar(value: &Option<Value>, label: &str) -> Result<(), String> {
    match value {
        None | Some(Value::Number(_) | Value::Bool(_)) => Ok(()),
        Some(Value::String(text)) => nonempty(text, label),
        Some(_) => Err(format!("{label}须是数字、字符串或布尔值")),
    }
}

/// `<namespace>.<name>`: lowercase ASCII letters, digits, `_` and `-` per part.
pub(crate) fn signal_kind(kind: &str) -> Result<(), String> {
    let part = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'));
    if kind.len() > 128 || !kind.contains('.') || !kind.split('.').all(part) {
        return Err(format!("信号种类须带命名空间，如 eng.crap：{kind}"));
    }
    Ok(())
}

impl Report {
    pub fn check(&self) -> Result<(), String> {
        bounded(&self.summary, "汇报摘要", SUMMARY_CHARS)?;
        optional(&self.next, "下一步")?;
        if let Some(progress) = &self.progress {
            if progress.total == 0 || progress.done > progress.total {
                return Err("进度须满足 0 ≤ done ≤ total 且 total > 0".into());
            }
            optional(&progress.unit, "进度单位")?;
        }
        Ok(())
    }
}

impl Signal {
    pub fn check(&self) -> Result<(), String> {
        signal_kind(&self.kind)?;
        bounded(&self.title, "信号标题", TITLE_CHARS)?;
        if let Some(detail) = &self.detail {
            bounded(detail, "信号说明", DETAIL_CHARS)?;
        }
        scalar(&self.value, "信号值")?;
        scalar(&self.threshold, "阈值")
    }
}

impl ChangeSummary {
    /// Over seven lines is refused, never cut: the sender knows what matters.
    pub fn check(&self) -> Result<(), String> {
        if self.lines.is_empty() || self.lines.len() > SUMMARY_LINES {
            return Err(format!("变化摘要须有 1 到 {SUMMARY_LINES} 行"));
        }
        for line in &self.lines {
            bounded(&line.label, "摘要标签", LABEL_CHARS)?;
            if line.label.chars().any(char::is_control) {
                return Err("摘要标签不能含控制字符".into());
            }
            match &line.value {
                Value::Number(_) => {}
                Value::String(text) => {
                    bounded(text, "摘要值", VALUE_CHARS)?;
                    if text.chars().any(char::is_control) {
                        return Err("摘要值不能含控制字符".into());
                    }
                }
                _ => return Err("摘要值须是数字或一段短文字".into()),
            }
        }
        Ok(())
    }

    /// One line per entry, `label：value`, in the sender's order.
    pub fn texts(&self) -> Vec<String> {
        self.lines.iter().map(|line| match &line.value {
            Value::String(text) => format!("{}：{text}", line.label),
            value => format!("{}：{value}", line.label),
        }).collect()
    }
}

/// FR-139: the summary on one line for the digest, cut at
/// [`SUMMARY_LINE_CHARS`]; the full lines stay in the summary.
pub fn summary_line(entry: &Merged<ChangeSummary>) -> String {
    clip(&format!("{}：{}（{} 提供）", entry.target, entry.data.texts().join("；"), entry.from.id), SUMMARY_LINE_CHARS)
}

impl Envelope {
    /// Validates an inbound message and its verb body.
    pub fn check(&self) -> Result<(), String> {
        if self.v != VERSION {
            return Err(format!("信封版本须为 {VERSION}"));
        }
        token(&self.project, "项目")?;
        token(&self.from.id, "来源标识")?;
        if self.from.id.to_ascii_lowercase().starts_with("human:") || self.from.kind == FromKind::Tower {
            return Err("接入方不能自称为人或 Tower.md".into());
        }
        valid_id(&self.target)?;
        for entry in &self.refs {
            nonempty(entry, "引用")?;
            if entry.chars().any(char::is_control) {
                return Err("引用不能含控制字符".into());
            }
        }
        parse_time(&self.at)?;
        match self.verb {
            Verb::Report => self.report()?.check(),
            Verb::Signal => self.signal()?.check(),
            Verb::RequestReport => Err("request_report 只能由 Tower.md 发出".into()),
            Verb::Card => crate::cards::check(&self.card()?, &self.target),
            Verb::Summary => self.summary()?.check(),
            Verb::Reply => self.answer()?.check(),
        }
    }
    /// The event kind this inbound message is recorded as.
    pub fn kind(&self) -> Result<&'static str, String> {
        match self.verb {
            Verb::Card => Ok(self.card()?.kind()),
            verb => verb.event_kind().ok_or_else(|| "request_report 只能由 Tower.md 发出".into()),
        }
    }
    pub fn card(&self) -> Result<crate::cards::Op, String> {
        serde_json::from_value(self.data.clone()).map_err(|e| format!("卡片操作无效：{e}"))
    }
    pub fn report(&self) -> Result<Report, String> {
        serde_json::from_value(self.data.clone()).map_err(|e| format!("汇报正文无效：{e}"))
    }
    pub fn signal(&self) -> Result<Signal, String> {
        serde_json::from_value(self.data.clone()).map_err(|e| format!("信号正文无效：{e}"))
    }
    pub fn summary(&self) -> Result<ChangeSummary, String> {
        serde_json::from_value(self.data.clone()).map_err(|e| format!("变化摘要无效：{e}"))
    }
    pub fn answer(&self) -> Result<Answer, String> {
        serde_json::from_value(self.data.clone()).map_err(|e| format!("回应正文无效：{e}"))
    }
}

/// FR-116: an outbound report request in the same envelope; a node
/// question (FR-127) is marked `question` and its answer must carry refs.
pub fn request_report(project: &str, target: &str, request_id: &str, due: &str, text: &str, at: &str, question: bool) -> Envelope {
    let mut data = json!({"request_id": request_id, "due": due, "text": text});
    if question {
        data["question"] = json!(true);
    }
    Envelope {
        v: VERSION, project: project.into(), from: From { id: TOWER.into(), kind: FromKind::Tower },
        target: target.into(), verb: Verb::RequestReport, data, refs: Vec::new(), at: at.into(),
    }
}

fn merge<T>(entries: &mut Vec<Merged<T>>, same: impl Fn(&Merged<T>) -> bool, envelope: Envelope, data: T, mark: Mark) {
    match entries.iter_mut().find(|entry| same(entry)) {
        Some(entry) => {
            entry.from = envelope.from;
            entry.data = data;
            entry.refs = envelope.refs;
            entry.at = envelope.at;
            entry.count += 1;
            entry.last = mark;
            entry.acknowledged = None;
        }
        None => entries.push(Merged {
            from: envelope.from, target: envelope.target, data, refs: envelope.refs, at: envelope.at,
            count: 1, first: mark.clone(), last: mark, acknowledged: None, muted: None,
        }),
    }
}

/// A person quiets the merged signal they saw. A message newer than `seen`
/// rejects the action, so nothing unseen is ever acknowledged or muted.
fn dismiss(state: &mut Value, event: &Event) -> Result<(), String> {
    if !human_actor(&event.actor) || event.layer != LAYER || !event.guards.is_empty() {
        return Err("只有人能在 App 中处理提醒".into());
    }
    let input: Dismissed = serde_json::from_value(event.data.clone()).map_err(|e| format!("提醒处理无效：{e}"))?;
    if input.target != event.obj {
        return Err("提醒处理的对象与事件不符".into());
    }
    let mut log = read(state)?;
    let entry = log.signals.iter_mut()
        .find(|entry| entry.from.id == input.from && entry.data.kind == input.kind && entry.target == input.target)
        .ok_or("提醒不存在")?;
    if entry.last.eid != input.seen {
        return Err("这条提醒已有新的上报，请重新查看后再处理".into());
    }
    let mark = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
    match (input.mode, input.until) {
        (Quiet::Ack, None) => entry.acknowledged = Some(mark),
        (Quiet::Mute, Some(until)) => {
            let (from, to) = (parse_time(&event.t)?, parse_time(&until)?);
            if to <= from || to > from + Duration::days(MUTE_DAYS) {
                return Err(format!("静音须在之后 {MUTE_DAYS} 天内结束"));
            }
            entry.muted = Some(Muted { until, level: entry.data.level, event: mark });
        }
        (Quiet::Ack, Some(_)) => return Err("“知道了”不带结束时间".into()),
        (Quiet::Mute, None) => return Err("静音须写明结束时间".into()),
    }
    state[GROUP] = serde_json::to_value(&log).map_err(|e| e.to_string())?;
    Ok(())
}

/// Validates and merges one `report.received`, `signal.received` or
/// `summary.received`, or records a person's `signal.dismissed`. Returns `Ok(false)` for other events.
pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind == DISMISSED {
        dismiss(state, event)?;
        return Ok(true);
    }
    let Some(verb) = Verb::of_event(&event.kind) else { return Ok(false) };
    if !event.guards.is_empty() || event.layer != LAYER {
        return Err(format!("{} 信封无效", event.kind));
    }
    check_signer(&event.actor)?;
    let envelope: Envelope = serde_json::from_value(event.data.clone()).map_err(|e| format!("{} 信封无效：{e}", event.kind))?;
    if envelope.verb != verb || envelope.target != event.obj {
        return Err(format!("{} 的动词或对象与信封不符", event.kind));
    }
    envelope.check()?;
    // The merged lists keep an entry per sender and target, so a whole-log
    // round trip per message made replay quadratic. They stay as stored and
    // only the entry this message lands on is read and written; the bytes
    // equal the round trip's. Nothing changes until every check has passed.
    let group = state.get(GROUP);
    let mut log: TowerLog = match group {
        Some(Value::Object(group)) => serde_json::from_value(Value::Object(group.iter()
            .filter(|(name, _)| !LISTS.contains(&name.as_str())).map(|(name, value)| (name.clone(), value.clone())).collect()))
            .map_err(|e| e.to_string())?,
        Some(_) => return Err("TowerApi 投影无效".into()),
        None => TowerLog::default(),
    };
    let mark = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
    let (from, target) = (envelope.from.id.clone(), envelope.target.clone());
    let landed = match verb {
        Verb::Report => {
            let data = envelope.report()?;
            if let Some(answers) = data.answers {
                let entry = log.answerers.entry(target.clone()).or_default();
                entry.retain(|id| *id != from);
                if answers {
                    entry.push(from.clone());
                    entry.sort();
                }
                if entry.is_empty() {
                    log.answerers.remove(&target);
                }
            }
            Some(merged_entry(group, "reports", |entry| entry["from"]["id"] == from.as_str() && entry["target"] == target.as_str(),
                envelope, data, mark)?)
        }
        Verb::Summary => {
            let data = envelope.summary()?;
            merge(&mut log.summaries, |entry| entry.from.id == from && entry.target == target, envelope, data, mark);
            None
        }
        _ => {
            let data = envelope.signal()?;
            if data.kind.starts_with(CORE_NAMESPACE) && event.actor != TOWER {
                return Err("core. 命名空间的信号只能由 Tower.md 发出".into());
            }
            let kind = data.kind.clone();
            let kept = match kind.as_str() {
                weakening::KIND => Some(&mut log.weakenings),
                requests::SILENT => Some(&mut log.silences),
                stuck::KIND => Some(&mut log.stuck),
                _ => None,
            };
            if let Some(kept) = kept {
                kept.push(Raised { target: target.clone(), refs: envelope.refs.clone(), event: mark.clone() });
            }
            Some(merged_entry(group, "signals", |entry| entry["from"]["id"] == from.as_str() && entry["data"]["kind"] == kind.as_str()
                && entry["target"] == target.as_str(), envelope, data, mark)?)
        }
    };
    let mut value = serde_json::to_value(&log).map_err(|e| e.to_string())?;
    if let Some(group) = state.get_mut(GROUP).and_then(Value::as_object_mut) {
        for name in LISTS {
            if let Some(stored) = group.remove(name) {
                value[name] = stored;
            }
        }
    }
    if let Some((list, index, merged)) = landed {
        let entries = value[list].as_array_mut().ok_or("TowerApi 投影无效")?;
        match index {
            Some(index) => entries[index] = merged,
            None => entries.push(merged),
        }
    }
    state[GROUP] = value;
    Ok(true)
}

/// The merged lists, kept as stored by `apply`.
const LISTS: [&str; 2] = ["reports", "signals"];

/// `merge` on the one stored entry of `list` that `same` picks out, if any:
/// where it goes and what it becomes.
fn merged_entry<T: Serialize + serde::de::DeserializeOwned>(group: Option<&Value>, list: &'static str,
    same: impl Fn(&Value) -> bool, envelope: Envelope, data: T, mark: Mark) -> Result<(&'static str, Option<usize>, Value), String> {
    let entries = match group.and_then(|group| group.get(list)) {
        Some(entries) => entries.as_array().ok_or("TowerApi 投影无效")?.as_slice(),
        None => &[],
    };
    let index = entries.iter().position(same);
    let mut one: Vec<Merged<T>> = match index {
        Some(index) => vec![serde_json::from_value(entries[index].clone()).map_err(|e| e.to_string())?],
        None => Vec::new(),
    };
    merge(&mut one, |_| true, envelope, data, mark);
    Ok((list, index, serde_json::to_value(&one[0]).map_err(|e| e.to_string())?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(verb: &str, from: &str, target: &str, data: Value) -> Value {
        json!({"v":1,"project":"p-1","from":{"id":from,"kind":"source"},"target":target,"verb":verb,
            "data":data,"refs":["src/a.rs#L1"],"at":"2026-10-23T10:00:00+09:00"})
    }

    fn event(seq: u64, kind: &str, actor: &str, data: Value) -> Event {
        let obj = data["target"].clone();
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":"2026-10-23T01:00:00Z","actor":actor,
            "type":kind,"layer":"L5","obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn signal(seq: u64, from: &str, kind: &str, target: &str, value: u64) -> Event {
        event(seq, "signal.received", "process:towermd-cli", envelope("signal", from, target,
            json!({"kind":kind,"level":"passive","title":"CRAP 偏高","value":value,"threshold":30})))
    }

    #[test]
    fn signals_merge_by_sender_kind_and_target_keeping_the_latest_value_and_a_count() {
        let mut state = json!({});
        for (seq, from, kind, target, value) in [
            (1, "pkg:eng@1/crap", "eng.crap", "T-1", 41), (2, "pkg:eng@1/crap", "eng.crap", "T-1", 42),
            (3, "pkg:eng@1/crap", "eng.crap", "T-2", 50), (4, "pkg:eng@1/crap", "eng.lint", "T-1", 1),
            (5, "other/1", "eng.crap", "T-1", 43), (6, "pkg:eng@1/crap", "eng.crap", "T-1", 44),
        ] {
            assert!(apply(&mut state, &signal(seq, from, kind, target, value)).unwrap());
        }
        let log = read(&state).unwrap();
        assert_eq!(log.signals.len(), 4);
        let merged = &log.signals[0];
        assert_eq!((merged.count, merged.data.value.clone(), merged.first.seq, merged.last.seq), (3, Some(json!(44)), 1, 6));
        assert_eq!(merged.last.actor, "process:towermd-cli");
        assert!(log.signals[1..].iter().all(|entry| entry.count == 1));
    }

    #[test]
    fn reports_keep_the_latest_per_sender_and_target_and_never_mean_done() {
        let mut state = json!({});
        let report = |seq, state: &str| event(seq, "report.received", "claude-code/2.1", envelope("report", "claude-code/2.1", "T-1",
            json!({"state":state,"summary":"一半","progress":{"done":3,"total":7,"unit":"files"}})));
        apply(&mut state, &report(1, "in_progress")).unwrap();
        apply(&mut state, &report(2, "claimed_done")).unwrap();
        let log = read(&state).unwrap();
        assert_eq!((log.reports.len(), log.reports[0].count, log.reports[0].data.state), (1, 2, ReportState::ClaimedDone));
        let done = report(3, "done");
        assert!(apply(&mut state, &done).is_err(), "there is no done state");
    }

    #[test]
    fn merging_one_entry_stores_the_same_bytes_as_a_whole_log_round_trip() {
        let mut state = json!({});
        for seq in 1..=40u64 {
            let (from, target) = (format!("src/{}", seq % 3), format!("T-{}", seq % 5));
            let event = if seq % 4 == 0 {
                signal(seq, &from, if seq % 8 == 0 { weakening::KIND } else { "eng.crap" }, &target, seq)
            } else {
                super::tests::event(seq, "report.received", "process:towermd-cli", envelope("report", &from, &target,
                    json!({"state":"in_progress","summary":"s","answers":seq % 3 != 1})))
            };
            let actor = if event.data["data"]["kind"] == weakening::KIND { TOWER } else { "process:towermd-cli" };
            let event = Event { actor: actor.into(), ..event };
            assert!(apply(&mut state, &event).unwrap(), "{seq}");
            assert_eq!(state[GROUP], serde_json::to_value(read(&state).unwrap()).unwrap(), "{seq}");
        }
        let log = read(&state).unwrap();
        assert_eq!((log.reports.len(), log.signals.len()), (15, 10));
        assert!(!log.weakenings.is_empty() && !log.answerers.is_empty());
    }

    #[test]
    fn people_cannot_sign_or_be_claimed_through_the_tower_api() {
        let body = envelope("signal", "x/1", "T-1", json!({"kind":"eng.crap","level":"active","title":"t"}));
        for actor in ["human:local", "human:bruce"] {
            assert!(apply(&mut json!({}), &event(1, "signal.received", actor, body.clone())).unwrap_err().contains("不能是人"));
        }
        let forged = envelope("signal", "human:local", "T-1", json!({"kind":"eng.crap","level":"active","title":"t"}));
        assert!(apply(&mut json!({}), &event(1, "signal.received", "process:towermd-cli", forged)).is_err());
        let mut tower = body.clone();
        tower["from"]["kind"] = json!("tower");
        assert!(apply(&mut json!({}), &event(1, "signal.received", "process:towermd-cli", tower)).is_err());
        for actor in ["process:towermd-cli", "claude-code/2.1"] {
            assert!(apply(&mut json!({}), &event(1, "signal.received", actor, body.clone())).unwrap());
        }
        for actor in ["process:", "a/b/c", "plain"] {
            assert!(check_signer(actor).is_err(), "{actor}");
        }
    }

    #[test]
    fn invalid_messages_are_rejected_and_leave_the_state_alone() {
        let base = |data: Value| envelope("signal", "x/1", "T-1", data);
        let ok = json!({"kind":"eng.crap","level":"time_sensitive","title":"t","detail":"d","deterministic":true});
        let mut state = json!({});
        assert!(apply(&mut state, &event(1, "signal.received", "x/1", base(ok.clone()))).unwrap());
        let before = state.clone();
        for (key, value) in [("kind", json!("crap")), ("kind", json!("Eng.crap")), ("kind", json!("core.silent")),
            ("level", json!("critical")), ("title", json!("长".repeat(31))), ("detail", json!("长".repeat(101))),
            ("value", json!({"a":1})), ("extra", json!(1))] {
            let mut data = ok.clone();
            data[key] = value;
            assert!(apply(&mut state, &event(2, "signal.received", "x/1", base(data))).is_err(), "{key}");
        }
        let mut wrong_obj = event(2, "signal.received", "x/1", base(ok.clone()));
        wrong_obj.obj = "T-2".into();
        let mut wrong_verb = event(2, "report.received", "x/1", base(ok.clone()));
        wrong_verb.kind = "report.received".into();
        let mut guarded = event(2, "signal.received", "x/1", base(ok.clone()));
        guarded.layer = "L2".into();
        let mut old = base(ok.clone());
        old["v"] = json!(2);
        let mut bad_progress = envelope("report", "x/1", "T-1", json!({"state":"in_progress","summary":"s","progress":{"done":8,"total":7}}));
        bad_progress["at"] = json!("yesterday");
        for bad in [wrong_obj, wrong_verb, guarded, event(2, "signal.received", "x/1", old), event(2, "report.received", "x/1", bad_progress)] {
            assert!(apply(&mut state, &bad).is_err(), "{}", bad.data);
        }
        assert_eq!(state, before);
        assert!(!apply(&mut state, &event(3, "report.replied", "x/1", base(ok))).unwrap());
    }

    fn dismissed(seq: u64, actor: &str, mode: &str, seen: &str, until: Option<&str>) -> Event {
        let mut data = json!({"from":"pkg:eng@1/crap","kind":"eng.crap","target":"T-1","mode":mode,"seen":seen});
        if let Some(until) = until { data["until"] = json!(until); }
        serde_json::from_value(json!({"seq":seq,"eid":format!("d{seq}"),"host":"h","t":"2026-10-23T02:00:00Z","actor":actor,
            "type":DISMISSED,"layer":"L5","obj":"T-1","data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    #[test]
    fn a_person_acknowledges_only_what_they_saw_and_a_newer_message_reopens_it() {
        let mut state = json!({});
        apply(&mut state, &signal(1, "pkg:eng@1/crap", "eng.crap", "T-1", 41)).unwrap();
        apply(&mut state, &signal(2, "pkg:eng@1/crap", "eng.crap", "T-1", 42)).unwrap();
        let before = state.clone();
        assert!(apply(&mut state, &dismissed(3, "human:local", "ack", "e1", None)).unwrap_err().contains("新的上报"));
        assert_eq!(state, before);
        assert!(apply(&mut state, &dismissed(3, "human:local", "ack", "e2", None)).unwrap());
        let entry = &read(&state).unwrap().signals[0];
        assert_eq!(entry.acknowledged.as_ref().map(|mark| (mark.eid.as_str(), mark.actor.as_str())), Some(("d3", "human:local")));
        apply(&mut state, &signal(4, "pkg:eng@1/crap", "eng.crap", "T-1", 43)).unwrap();
        assert!(read(&state).unwrap().signals[0].acknowledged.is_none(), "a newer message is unseen");
        let reports = read(&state).unwrap();
        assert!(serde_json::to_value(&reports.reports).unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn a_person_mutes_with_an_end_time_and_only_from_the_app() {
        let mut state = json!({});
        apply(&mut state, &signal(1, "pkg:eng@1/crap", "eng.crap", "T-1", 41)).unwrap();
        let before = state.clone();
        for actor in ["process:towermd-cli", "process:towermd", "claude-code/2.1", "human:"] {
            assert!(apply(&mut state, &dismissed(2, actor, "ack", "e1", None)).is_err(), "{actor}");
        }
        for until in [None, Some("2026-10-23T01:00:00Z"), Some("2026-12-01T00:00:00Z"), Some("tomorrow")] {
            assert!(apply(&mut state, &dismissed(2, "human:local", "mute", "e1", until)).is_err(), "{until:?}");
        }
        assert!(apply(&mut state, &dismissed(2, "human:local", "ack", "e1", Some("2026-10-24T00:00:00Z"))).is_err());
        let mut elsewhere = dismissed(2, "human:local", "ack", "e1", None);
        elsewhere.obj = "T-2".into();
        let mut unknown = dismissed(2, "human:local", "ack", "e1", None);
        unknown.data["kind"] = json!("eng.lint");
        let mut extra = dismissed(2, "human:local", "ack", "e1", None);
        extra.data["actor"] = json!("human:other");
        for bad in [elsewhere, unknown, extra] {
            assert!(apply(&mut state, &bad).is_err(), "{}", bad.data);
        }
        assert_eq!(state, before);
        assert!(apply(&mut state, &dismissed(2, "human:local", "mute", "e1", Some("2026-10-30T02:00:00Z"))).unwrap());
        apply(&mut state, &signal(3, "pkg:eng@1/crap", "eng.crap", "T-1", 42)).unwrap();
        let muted = read(&state).unwrap().signals[0].muted.clone().unwrap();
        assert_eq!((muted.until.as_str(), muted.level, muted.event.eid.as_str()), ("2026-10-30T02:00:00Z", Level::Passive, "d2"));
    }

    #[test]
    fn only_tower_raises_core_signals_and_sends_report_requests() {
        let core = envelope("signal", "process:towermd", "T-1", json!({"kind":"core.silent","level":"active","title":"t"}));
        assert!(apply(&mut json!({}), &event(1, "signal.received", TOWER, core)).unwrap());
        let outbound = request_report("p-1", "T-1", "R-1", "2026-10-24T00:00:00Z", "plan", "2026-10-23T00:00:00.000Z", false);
        assert!(outbound.data.get("question").is_none());
        let question = request_report("p-1", "T-1", "Q-1", "2026-10-24T00:00:00Z", "why", "2026-10-23T00:00:00.000Z", true);
        assert_eq!(question.data["question"], true);
        assert_eq!(serde_json::to_value(&outbound).unwrap()["verb"], "request_report");
        assert!(outbound.check().is_err(), "outbound messages are not accepted inbound");
        assert_eq!(timestamp(0).unwrap(), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn weakenings_merge_like_any_signal_and_are_also_kept_in_order() {
        let mut state = json!({});
        let quiet = envelope("signal", "process:towermd", "T-1", json!({"kind":"core.silent","level":"active","title":"t"}));
        apply(&mut state, &event(1, "signal.received", TOWER, quiet)).unwrap();
        assert!(!serde_json::to_string(&state).unwrap().contains("weakenings"), "absent until first used");
        for seq in [2, 3] {
            let raised = envelope("signal", "process:towermd", "T-1", json!({"kind":weakening::KIND,"level":"active","title":"承诺弱化：1 处"}));
            apply(&mut state, &event(seq, "signal.received", TOWER, raised)).unwrap();
        }
        let log = read(&state).unwrap();
        let merged = log.signals.iter().find(|entry| entry.data.kind == weakening::KIND).unwrap();
        assert_eq!(merged.count, 2);
        assert_eq!(log.weakenings.iter().map(|raised| (raised.event.seq, raised.target.as_str(), raised.refs.clone()))
            .collect::<Vec<_>>(), [(2, "T-1", vec!["src/a.rs#L1".to_owned()]), (3, "T-1", vec!["src/a.rs#L1".to_owned()])]);
        let forged = envelope("signal", "ci", "T-1", json!({"kind":weakening::KIND,"level":"active","title":"t"}));
        assert!(apply(&mut state, &event(4, "signal.received", "process:towermd-cli", forged)).is_err());
    }

    #[test]
    fn answers_declarations_are_sticky_per_target_until_withdrawn() {
        let mut state = json!({});
        let report = |seq, from: &str, target: &str, answers: Option<bool>| {
            let mut data = json!({"state":"in_progress","summary":"s"});
            if let Some(answers) = answers { data["answers"] = json!(answers); }
            event(seq, "report.received", "process:towermd-cli", envelope("report", from, target, data))
        };
        apply(&mut state, &report(1, "b/1", "T-1", Some(true))).unwrap();
        apply(&mut state, &report(2, "a/1", "T-1", Some(true))).unwrap();
        apply(&mut state, &report(3, "a/1", "T-1", None)).unwrap();
        apply(&mut state, &report(4, "a/1", "T-2", Some(true))).unwrap();
        let log = read(&state).unwrap();
        assert_eq!((log.answerers("T-1"), log.answerers("T-2"), log.answerers("T-3")),
            (vec!["a/1".to_owned(), "b/1".to_owned()], vec!["a/1".to_owned()], vec![]));
        apply(&mut state, &report(5, "a/1", "T-2", Some(false))).unwrap();
        assert!(!read(&state).unwrap().answerers.contains_key("T-2"));
        assert!(apply(&mut state, &report(6, "a/1", "T-1", Some(true))).is_ok());
        let mut bad = report(7, "a/1", "T-1", None);
        bad.data["data"]["answers"] = json!("yes");
        assert!(apply(&mut state, &bad).is_err());
    }

    #[test]
    fn silences_and_stuck_signals_are_kept_in_order_like_weakenings() {
        let mut state = json!({});
        for (seq, kind) in [(1, crate::requests::SILENT), (2, crate::stuck::KIND), (3, crate::requests::SILENT)] {
            let raised = envelope("signal", "process:towermd", "T-1", json!({"kind":kind,"level":"active","title":"t"}));
            apply(&mut state, &event(seq, "signal.received", TOWER, raised)).unwrap();
        }
        let log = read(&state).unwrap();
        assert_eq!(log.silences.iter().map(|raised| raised.event.seq).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(log.stuck.iter().map(|raised| raised.event.seq).collect::<Vec<_>>(), [2]);
        assert!(log.weakenings.is_empty());
        assert_eq!(clip("一二三四", 3), "一二…");
        assert_eq!(clip("一二三", 3), "一二三");
        assert_ne!(signal_eid("a", "b"), signal_eid("a", "c"));
    }

    #[test]
    fn change_summaries_hold_at_most_seven_structured_lines_and_keep_the_latest_per_sender() {
        let summary = |seq, from: &str, lines: Value| event(seq, "summary.received", "process:towermd-cli",
            envelope("summary", from, "T-1", json!({"lines": lines})));
        let line = |label: &str, value: Value| json!({"label": label, "value": value});
        let mut state = json!({});
        apply(&mut state, &summary(1, "pkg:eng/diff", json!([line("新增函数", json!(3)), line("删除测试", json!(1))]))).unwrap();
        apply(&mut state, &summary(2, "pkg:eng/diff", json!([line("新增函数", json!(4)), line("结尾", json!("改了"))]))).unwrap();
        apply(&mut state, &summary(3, "other/1", json!([line("新增主张", json!(2))]))).unwrap();
        let log = read(&state).unwrap();
        assert_eq!(log.summaries.len(), 2);
        assert_eq!((log.summaries[0].count, log.summaries[0].data.texts()), (2, vec!["新增函数：4".to_owned(), "结尾：改了".to_owned()]));
        assert_eq!(summary_line(&log.summaries[0]), "T-1：新增函数：4；结尾：改了（pkg:eng/diff 提供）");
        let eight: Vec<Value> = (0..8).map(|n| line(&format!("第 {n} 项"), json!(n))).collect();
        for (lines, error) in [
            (json!([]), "1 到 7 行"), (json!(eight), "1 到 7 行"), (json!([line("", json!(1))]), "摘要标签"),
            (json!([line(&"标".repeat(LABEL_CHARS + 1), json!(1))]), "摘要标签"), (json!([line("a", json!("x".repeat(VALUE_CHARS + 1)))]), "摘要值"),
            (json!([line("a", json!("一\n二"))]), "控制字符"), (json!([line("a", json!(true))]), "数字或"), (json!([line("a", json!({"n":1}))]), "数字或"),
            (json!([{"label":"a","value":1,"diff":"..."}]), "变化摘要无效"),
        ] {
            let error_text = apply(&mut state.clone(), &summary(9, "x/1", lines.clone())).unwrap_err();
            assert!(error_text.contains(error), "{lines}: {error_text}");
        }
        let long: Vec<Value> = (0..7).map(|n| line(&"标".repeat(LABEL_CHARS), json!("值".repeat(VALUE_CHARS - n)))).collect();
        apply(&mut state, &summary(10, "long/1", json!(long))).unwrap();
        let shown = summary_line(&read(&state).unwrap().summaries[2]);
        assert_eq!((shown.chars().count(), shown.ends_with('…')), (SUMMARY_LINE_CHARS, true));
    }
}
