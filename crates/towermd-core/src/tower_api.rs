//! Tower API (core spec §5, FR-114–119): one envelope for every message, the
//! `report` and `signal` verbs, four signal levels and signal merging. The
//! envelope's `from` is self-reported; the event's signer is the entry that
//! accepted it (`process:<id>` or `<producer>/<version>`), never `human:`.
//! A report never completes anything: `claimed_done` is only a claim. The
//! `TowerApi` group is created on first use, so older projections stay
//! byte-identical. A person quiets a merged signal only from the App
//! (`signal.dismissed`, signed `human:<id>`); the CLI has no such verb.
use chrono::{DateTime, Duration, SecondsFormat};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    delegation::parse_time,
    delegation_projection::{Mark, ReportState, SUMMARY_CHARS},
    hub::Event,
    question_model::{human_actor, nonempty, valid_id},
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

/// Inbound verbs this core records, and the outbound `request_report`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verb { Report, Signal, RequestReport }

impl Verb {
    /// The event kind an accepted inbound message is recorded as.
    pub fn event_kind(self) -> Option<&'static str> {
        match self {
            Verb::Report => Some("report.received"),
            Verb::Signal => Some("signal.received"),
            Verb::RequestReport => None,
        }
    }
    fn of_event(kind: &str) -> Option<Self> {
        [Verb::Report, Verb::Signal].into_iter().find(|verb| verb.event_kind() == Some(kind))
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
}

pub fn read(state: &Value) -> Result<TowerLog, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(TowerLog::default()),
    }
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
fn signal_kind(kind: &str) -> Result<(), String> {
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
        }
    }
    pub fn report(&self) -> Result<Report, String> {
        serde_json::from_value(self.data.clone()).map_err(|e| format!("汇报正文无效：{e}"))
    }
    pub fn signal(&self) -> Result<Signal, String> {
        serde_json::from_value(self.data.clone()).map_err(|e| format!("信号正文无效：{e}"))
    }
}

/// FR-116: an outbound report request in the same envelope.
pub fn request_report(project: &str, target: &str, request_id: &str, due: &str, text: &str, at: &str) -> Envelope {
    Envelope {
        v: VERSION, project: project.into(), from: From { id: TOWER.into(), kind: FromKind::Tower },
        target: target.into(), verb: Verb::RequestReport,
        data: json!({"request_id": request_id, "due": due, "text": text}), refs: Vec::new(), at: at.into(),
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

/// Validates and merges one `report.received` or `signal.received`, or
/// records a person's `signal.dismissed`. Returns `Ok(false)` for other events.
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
    let mut log = read(state)?;
    let mark = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
    let (from, target) = (envelope.from.id.clone(), envelope.target.clone());
    match verb {
        Verb::Report => {
            let data = envelope.report()?;
            merge(&mut log.reports, |entry| entry.from.id == from && entry.target == target, envelope, data, mark);
        }
        _ => {
            let data = envelope.signal()?;
            if data.kind.starts_with(CORE_NAMESPACE) && event.actor != TOWER {
                return Err("core. 命名空间的信号只能由 Tower.md 发出".into());
            }
            let kind = data.kind.clone();
            merge(&mut log.signals, |entry| entry.from.id == from && entry.data.kind == kind && entry.target == target, envelope, data, mark);
        }
    }
    state[GROUP] = serde_json::to_value(&log).map_err(|e| e.to_string())?;
    Ok(true)
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
        let outbound = request_report("p-1", "T-1", "R-1", "2026-10-24T00:00:00Z", "plan", "2026-10-23T00:00:00.000Z");
        assert_eq!(serde_json::to_value(&outbound).unwrap()["verb"], "request_report");
        assert!(outbound.check().is_err(), "outbound messages are not accepted inbound");
        assert_eq!(timestamp(0).unwrap(), "1970-01-01T00:00:00.000Z");
    }
}
