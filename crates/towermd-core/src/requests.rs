//! Report requests on any object and node questions (core spec FR-116,
//! FR-127, FR-137). A person sends one from the App (`request.sent`, signed
//! `human:<id>`); the recipient answers through any Tower API entry (`request.replied`,
//! signed by the CLI, HTTP or file entry); the person may withdraw an open one
//! (`request.withdrawn`). A question goes to the senders that declared
//! `answers: true` on the node; with none it waits in the queue (open question
//! 1, option A) and is delivered to whoever declares it later.
//!
//! A request past its deadline without a reply makes Tower.md raise
//! `core.silent` (`passive`; the second in a row on the same object is
//! `active`). Checkpoint requests (`R-…`, FR-211) are judged by the same
//! rule; their old `report.overdue` events still replay and count as silent.
//! The `Requests` group is created on first use.
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation::parse_time,
    delegation_projection::{self as delegations, Entry, Mark, OverdueLevel, ReportState, SUMMARY_CHARS},
    hub::Event,
    kit::Kit,
    question_model::{human_actor, nonempty, valid_id, SelfReported},
    tower_api::{self, clip, Envelope, Level, Signal, TowerLog, DETAIL_CHARS, TITLE_CHARS},
};

pub const GROUP: &str = "Requests";
pub const SENT: &str = "request.sent";
pub const REPLIED: &str = "request.replied";
pub const WITHDRAWN: &str = "request.withdrawn";
pub const KINDS: [&str; 3] = [SENT, REPLIED, WITHDRAWN];
/// Fast layer: asking and answering change no commitment.
pub const LAYER: &str = tower_api::LAYER;
/// The signal kind for silence; only Tower.md raises it.
pub const SILENT: &str = "core.silent";
/// Ref naming the request a `core.silent` is about.
pub const REF_REQUEST: &str = "request:";
/// Checkpoint request IDs (`R-<attempt>-<n>`) are reserved.
pub const CHECKPOINT_PREFIX: &str = "R-";
/// A request's text has at most this many characters.
pub const TEXT_CHARS: usize = 1000;
/// The three entries (FR-117) that may record a reply: the CLI's
/// `towermd reply`, and the `reply` verb through `POST /tower` or the inbox.
pub const REPLY_SIGNERS: [&str; 3] = ["process:towermd-cli", "process:towermd-http", "process:towermd-file"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ask {
    /// "要求汇报": a status report on the object.
    Report,
    /// FR-127 node question: answered with refs.
    Question,
    /// FR-156 "要求按规则更新": the target is a frame; recipients update it
    /// by its rules, then reply.
    Update,
}

/// `request.sent`. `to` is fixed by the daemon when it records the request;
/// an empty list means "whoever declares `answers` on the target".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sent {
    pub target: String,
    pub ask: Ask,
    pub text: String,
    pub due: String,
    #[serde(default)]
    pub to: Vec<String>,
}

/// `request.replied`: the answer, from one of the recipients.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub from: String,
    pub state: ReportState,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_reported: Option<SelfReported>,
}

/// `request.withdrawn` has no body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Withdrawn {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub event: Mark,
    pub data: Sent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Entry<Reply>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawn: Option<Mark>,
}

impl Request {
    pub fn open(&self) -> bool {
        self.reply.is_none() && self.withdrawn.is_none()
    }
}

pub fn read(state: &Value) -> Result<BTreeMap<String, Request>, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}

/// How a run's delegate is named as a sender: `name/version` when it said,
/// else the run ID. The CLI uses the same for `from` (FR-114).
pub fn delegate_id(run: &str, source: Option<&SelfReported>) -> String {
    source.map_or_else(|| run.to_owned(), |source| format!("{}/{}", source.name, source.version))
}

/// Who may answer: the fixed recipients, or, for a queued request, whoever
/// declares `answers` on the target now.
pub fn recipients(request: &Request, tower: &TowerLog) -> Vec<String> {
    if request.data.to.is_empty() { tower.answerers(&request.data.target) } else { request.data.to.clone() }
}

fn mark(event: &Event) -> Mark {
    Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() }
}

fn parse<T: serde::de::DeserializeOwned>(event: &Event) -> Result<T, String> {
    serde_json::from_value(event.data.clone()).map_err(|e| format!("{} 正文无效：{e}", event.kind))
}

fn plain(text: &str, label: &str) -> Result<(), String> {
    nonempty(text, label)?;
    if text.trim() != text || text.chars().any(char::is_control) || text.len() > 256 {
        return Err(format!("{label}不能含首尾空白或控制字符，且不超过 256 字节"));
    }
    Ok(())
}

fn sent(event: &Event, log: &BTreeMap<String, Request>, tower: &TowerLog) -> Result<Request, String> {
    if !human_actor(&event.actor) {
        return Err("请求汇报或提问只能由 App 中的人发出".into());
    }
    let data: Sent = parse(event)?;
    valid_id(&event.obj)?;
    if event.obj.starts_with(CHECKPOINT_PREFIX) {
        return Err("R- 开头的请求 ID 留给检查点".into());
    }
    if log.contains_key(&event.obj) {
        return Err(format!("请求已存在：{}", event.obj));
    }
    valid_id(&data.target)?;
    nonempty(&data.text, "请求内容")?;
    if data.text.chars().count() > TEXT_CHARS {
        return Err(format!("请求内容不能超过 {TEXT_CHARS} 字"));
    }
    if parse_time(&data.due)? <= parse_time(&event.t)? {
        return Err("截止时间须晚于发出时间".into());
    }
    let unique: BTreeSet<&String> = data.to.iter().collect();
    if unique.len() != data.to.len() {
        return Err("收件人重复".into());
    }
    for id in &data.to {
        plain(id, "收件人")?;
    }
    if data.ask == Ask::Question && data.to != tower.answerers(&data.target) {
        return Err("节点提问只发给在该节点声明了 answers 的受托者或来源".into());
    }
    Ok(Request { id: event.obj.clone(), event: mark(event), data, reply: None, withdrawn: None })
}

fn replied(event: &Event, request: &mut Request, tower: &TowerLog) -> Result<(), String> {
    if !REPLY_SIGNERS.contains(&event.actor.as_str()) {
        return Err("回应须经命令行、本机 HTTP 或文件入口".into());
    }
    let data: Reply = parse(event)?;
    if !recipients(request, tower).contains(&data.from) {
        return Err(format!("{} 不是这条请求的收件人", data.from));
    }
    nonempty(&data.summary, "汇报摘要")?;
    if data.summary.chars().count() > SUMMARY_CHARS {
        return Err(format!("汇报摘要不能超过 {SUMMARY_CHARS} 字"));
    }
    if let Some(next) = &data.next {
        nonempty(next, "下一步")?;
    }
    for entry in &data.refs {
        nonempty(entry, "引用")?;
        if entry.chars().any(char::is_control) {
            return Err("引用不能含控制字符".into());
        }
    }
    if request.data.ask == Ask::Question && data.refs.is_empty() {
        return Err("回答节点提问须带 refs".into());
    }
    if let Some(source) = &data.self_reported {
        source.check()?;
    }
    request.reply = Some(Entry { event: mark(event), data });
    Ok(())
}

/// Validates and records one request event. Returns `Ok(false)` for others.
pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if !KINDS.contains(&event.kind.as_str()) {
        return Ok(false);
    }
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err(format!("{} 的层级无效", event.kind));
    }
    let mut log = read(state)?;
    let tower = tower_api::read(state)?;
    if event.kind == SENT {
        let request = sent(event, &log, &tower)?;
        if request.data.ask == Ask::Update && state.get(crate::frames::GROUP).and_then(|all| all.get(&request.data.target)).is_none() {
            return Err(format!("要求按规则更新只能发给容器：{}", request.data.target));
        }
        log.insert(request.id.clone(), request);
    } else {
        let request = log.get_mut(&event.obj).filter(|request| request.open()).ok_or("请求不存在或已关闭")?;
        if event.kind == REPLIED {
            replied(event, request, &tower)?;
        } else {
            if !human_actor(&event.actor) {
                return Err("撤回请求只能由 App 中的人发出".into());
            }
            let _: Withdrawn = parse(event)?;
            request.withdrawn = Some(mark(event));
        }
    }
    state[GROUP] = serde_json::to_value(&log).map_err(|e| e.to_string())?;
    Ok(true)
}

/// One request that has gone silent and owes its `core.silent`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Silence {
    pub request: String,
    pub target: String,
    pub text: String,
    pub due: String,
    pub level: Level,
    pub recipients: Vec<String>,
}

/// The `core.silent` already raised for a request, if any.
pub fn raised<'a>(tower: &'a TowerLog, request: &str) -> Option<&'a Mark> {
    let name = format!("{REF_REQUEST}{request}");
    tower.silences.iter().find(|raised| raised.refs.contains(&name)).map(|raised| &raised.event)
}

struct Candidate {
    id: String,
    target: String,
    text: String,
    due: DateTime<FixedOffset>,
    due_text: String,
    open: bool,
    recipients: Vec<String>,
    overdue: Option<OverdueLevel>,
}

/// FR-137: every open, addressed request past its deadline that has no
/// `core.silent` yet, oldest first, with its level. A request is `active`
/// when the previous request on the same object (by deadline) was silent
/// too. A request nobody can receive yet never goes silent: it is the
/// person's queue, shown as "无人可答" instead.
pub fn silences(state: &Value, kits: &[&dyn Kit], now_ms: u64) -> Result<Vec<Silence>, String> {
    let now = i64::try_from(now_ms).ok().and_then(DateTime::from_timestamp_millis).ok_or("时间无效")?.fixed_offset();
    let tower = tower_api::read(state)?;
    let log = delegations::read(state)?;
    let items = delegations::delegations(state, kits)?;
    let mut all = Vec::new();
    for request in delegations::report_requests(&items, &log) {
        let source = items.iter().filter(|item| item.id == request.object).flat_map(|item| &item.attempts)
            .find(|attempt| attempt.id == request.attempt).and_then(|attempt| attempt.delegate.as_ref());
        all.push(Candidate {
            recipients: vec![delegate_id(&request.attempt, source)],
            overdue: log.reports.get(&request.id).and_then(|report| report.overdue.as_ref()).map(|entry| entry.data.level),
            due: parse_time(&request.due)?, due_text: request.due, open: request.open,
            id: request.id, target: request.object, text: request.text,
        });
    }
    for request in read(state)?.into_values() {
        all.push(Candidate {
            recipients: recipients(&request, &tower), overdue: None, due: parse_time(&request.data.due)?,
            open: request.open(), id: request.id, target: request.data.target, text: request.data.text,
            due_text: request.data.due,
        });
    }
    all.sort_by(|a, b| (a.due, &a.id).cmp(&(b.due, &b.id)));
    let mut silent: BTreeSet<String> = all.iter()
        .filter(|item| item.overdue.is_some() || raised(&tower, &item.id).is_some()).map(|item| item.id.clone()).collect();
    let mut previous: BTreeMap<String, String> = BTreeMap::new();
    let mut found = Vec::new();
    for item in all {
        let before = previous.insert(item.target.clone(), item.id.clone());
        if !item.open || item.recipients.is_empty() || item.due > now || raised(&tower, &item.id).is_some() {
            continue;
        }
        let level = match item.overdue {
            // A checkpoint already marked overdue keeps the level it had.
            Some(OverdueLevel::Active) => Level::Active,
            Some(OverdueLevel::Passive) => Level::Passive,
            None if before.is_some_and(|id| silent.contains(&id)) => Level::Active,
            None => Level::Passive,
        };
        silent.insert(item.id.clone());
        found.push(Silence { request: item.id, target: item.target, text: item.text, due: item.due_text, level,
            recipients: item.recipients });
    }
    Ok(found)
}

/// Hub ID of the `core.silent` for one request.
pub fn silence_eid(request: &str) -> String {
    tower_api::signal_eid(SILENT, request)
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn silence_signal(silence: &Silence) -> Signal {
    Signal {
        kind: SILENT.into(), level: silence.level,
        title: clip(&format!("失联：{}", one_line(&silence.text)), TITLE_CHARS),
        detail: Some(clip(&format!("{} 未在截止 {} 前回应汇报请求 {}", silence.recipients.join("、"), silence.due, silence.request),
            DETAIL_CHARS)),
        value: None, threshold: None, deterministic: true,
    }
}

/// The envelope Tower.md records as `signal.received` signed `process:towermd`.
pub fn silence_envelope(project: &str, silence: &Silence, at: String) -> Result<Envelope, String> {
    tower_api::tower_signal(project, &silence.target, vec![format!("{REF_REQUEST}{}", silence.request)], at, &silence_signal(silence))
}

/// A request as the App and the CLI see it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestView {
    #[serde(flatten)]
    pub request: Request,
    pub open: bool,
    pub recipients: Vec<String>,
    pub silence: Option<Mark>,
}

pub fn view(state: &Value) -> Result<Vec<RequestView>, String> {
    let tower = tower_api::read(state)?;
    let mut all: Vec<RequestView> = read(state)?.into_values().map(|request| RequestView {
        open: request.open(), recipients: recipients(&request, &tower), silence: raised(&tower, &request.id).cloned(), request,
    }).collect();
    all.sort_by_key(|view| view.request.event.seq);
    Ok(all)
}

/// Open requests a sender may answer, oldest first.
pub fn addressed_to(state: &Value, sender: &str) -> Result<Vec<RequestView>, String> {
    Ok(view(state)?.into_iter().filter(|view| view.open && view.recipients.iter().any(|id| id == sender)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CLI: &str = "process:towermd-cli";
    fn event(seq: u64, kind: &str, actor: &str, obj: &str, t: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":t,"actor":actor,
            "type":kind,"layer":"L5","obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }
    const T0: &str = "2026-10-23T00:00:00Z";
    fn sent(seq: u64, id: &str, target: &str, ask: &str, due: &str, to: &[&str]) -> Event {
        event(seq, SENT, "human:local", id, T0, json!({"target":target,"ask":ask,"text":"进展如何？","due":due,"to":to}))
    }
    fn reply(seq: u64, id: &str, from: &str, refs: &[&str]) -> Event {
        event(seq, REPLIED, CLI, id, "2026-10-23T01:00:00Z",
            json!({"from":from,"state":"in_progress","summary":"一半","refs":refs}))
    }
    fn declare(state: &mut Value, seq: u64, from: &str, target: &str, answers: bool) {
        let envelope = json!({"v":1,"project":"p","from":{"id":from,"kind":"delegate"},"target":target,"verb":"report",
            "data":{"state":"in_progress","summary":"s","answers":answers},"at":T0});
        let event = event(seq, "report.received", "process:towermd-cli", target, T0, envelope);
        assert!(tower_api::apply(state, &event).unwrap());
    }

    #[test]
    fn only_a_person_sends_and_withdraws_and_only_the_cli_replies() {
        let mut state = json!({});
        for actor in ["process:towermd-cli", "process:towermd", "claude-code/2.1"] {
            let mut forged = sent(1, "Q1", "T-1", "report", "2026-10-24T00:00:00Z", &["a/1"]);
            forged.actor = actor.into();
            assert!(apply(&mut state, &forged).is_err(), "{actor}");
        }
        assert!(apply(&mut state, &sent(1, "Q1", "T-1", "report", "2026-10-24T00:00:00Z", &["a/1"])).unwrap());
        let mut human = reply(2, "Q1", "a/1", &[]);
        human.actor = "human:local".into();
        assert!(apply(&mut state, &human).is_err());
        let mut withdraw = event(2, WITHDRAWN, CLI, "Q1", T0, json!({}));
        assert!(apply(&mut state, &withdraw).is_err());
        withdraw.actor = "human:local".into();
        assert!(apply(&mut state, &withdraw).unwrap());
        assert!(!read(&state).unwrap()["Q1"].open());
        assert!(apply(&mut state, &reply(3, "Q1", "a/1", &[])).unwrap_err().contains("已关闭"));
        assert!(!apply(&mut state, &event(4, "report.replied", CLI, "R-x-1", T0, json!({}))).unwrap());
    }

    /// FR-117: the `reply` verb through HTTP or the inbox answers like the
    /// CLI; any other signer, including another process, is refused.
    #[test]
    fn a_reply_comes_through_any_of_the_three_entries_only() {
        for (actor, ok) in [("process:towermd-http", true), ("process:towermd-file", true), ("process:towermd", false), ("a/1", false)] {
            let mut state = json!({});
            assert!(apply(&mut state, &sent(1, "Q1", "T-1", "report", "2026-10-24T00:00:00Z", &["a/1"])).unwrap());
            let mut answer = reply(2, "Q1", "a/1", &[]);
            answer.actor = actor.into();
            assert_eq!(apply(&mut state, &answer).is_ok(), ok, "{actor}");
            assert_eq!(read(&state).unwrap()["Q1"].open(), !ok);
        }
    }

    #[test]
    fn invalid_requests_are_rejected_and_leave_the_state_alone() {
        let mut state = json!({});
        assert!(apply(&mut state, &sent(1, "Q1", "T-1", "report", "2026-10-24T00:00:00Z", &[])).unwrap());
        let before = state.clone();
        let mut long = sent(2, "Q2", "T-1", "report", "2026-10-24T00:00:00Z", &[]);
        long.data["text"] = json!("长".repeat(TEXT_CHARS + 1));
        let mut extra = sent(2, "Q2", "T-1", "report", "2026-10-24T00:00:00Z", &[]);
        extra.data["actor"] = json!("human:x");
        let mut guarded = sent(2, "Q2", "T-1", "report", "2026-10-24T00:00:00Z", &[]);
        guarded.layer = "L2".into();
        for bad in [
            sent(2, "Q1", "T-1", "report", "2026-10-24T00:00:00Z", &[]),
            sent(2, "R-a-1", "T-1", "report", "2026-10-24T00:00:00Z", &[]),
            sent(2, "Q2", "bad id", "report", "2026-10-24T00:00:00Z", &[]),
            sent(2, "Q2", "T-1", "report", T0, &[]),
            sent(2, "Q2", "T-1", "report", "soon", &[]),
            sent(2, "Q2", "T-1", "report", "2026-10-24T00:00:00Z", &["a/1", "a/1"]),
            sent(2, "Q2", "T-1", "report", "2026-10-24T00:00:00Z", &[" a"]),
            sent(2, "Q2", "T-1", "verdict", "2026-10-24T00:00:00Z", &[]),
            long, extra, guarded,
        ] {
            assert!(apply(&mut state, &bad).is_err(), "{}", bad.data);
        }
        assert_eq!(state, before);
    }

    #[test]
    fn an_update_by_the_rules_goes_only_to_a_frame() {
        let mut state = json!({"Frame":{"C-board":{}}});
        let error = apply(&mut state, &sent(1, "Q1", "T-1", "update", "2026-10-24T00:00:00Z", &[])).unwrap_err();
        assert!(error.contains("只能发给容器"), "{error}");
        assert!(apply(&mut state, &sent(1, "Q1", "C-board", "update", "2026-10-24T00:00:00Z", &["a/1"])).unwrap_or_else(|e| panic!("{e}")));
        assert_eq!(read(&state).unwrap()["Q1"].data.ask, Ask::Update);
    }

    #[test]
    fn questions_go_to_declared_answerers_and_their_answers_carry_refs() {
        let mut state = json!({});
        // Nobody declared: the question is queued with no recipients.
        assert!(apply(&mut state, &sent(1, "Q1", "N-1", "question", "2026-10-24T00:00:00Z", &[])).unwrap());
        let tower = tower_api::read(&state).unwrap();
        assert!(recipients(&read(&state).unwrap()["Q1"], &tower).is_empty());
        assert!(apply(&mut state, &reply(2, "Q1", "a/1", &["src/a.rs"])).unwrap_err().contains("收件人"));
        declare(&mut state, 3, "a/1", "N-1", true);
        declare(&mut state, 4, "b/1", "N-1", true);
        assert_eq!(addressed_to(&state, "a/1").unwrap().len(), 1, "a queued question reaches a later answerer");
        // A new question must name exactly the declared answerers.
        assert!(apply(&mut state, &sent(5, "Q2", "N-1", "question", "2026-10-24T00:00:00Z", &["a/1"])).is_err());
        assert!(apply(&mut state, &sent(5, "Q2", "N-1", "question", "2026-10-24T00:00:00Z", &["a/1", "b/1"])).unwrap());
        assert!(apply(&mut state, &reply(6, "Q1", "a/1", &[])).unwrap_err().contains("refs"));
        assert!(apply(&mut state, &reply(6, "Q1", "c/1", &["x"])).is_err());
        assert!(apply(&mut state, &reply(6, "Q1", "a/1", &["src/a.rs#L1"])).unwrap());
        declare(&mut state, 7, "b/1", "N-1", false);
        assert_eq!(tower_api::read(&state).unwrap().answerers("N-1"), ["a/1"]);
        // Fixed recipients stay the ones it was sent to.
        assert!(apply(&mut state, &reply(8, "Q2", "b/1", &["doc.md"])).unwrap());
    }

    #[test]
    fn a_report_request_accepts_a_reply_without_refs_from_a_recipient() {
        let mut state = json!({});
        apply(&mut state, &sent(1, "Q1", "T-1", "report", "2026-10-24T00:00:00Z", &["a/1"])).unwrap();
        let mut long = reply(2, "Q1", "a/1", &[]);
        long.data["summary"] = json!("长".repeat(SUMMARY_CHARS + 1));
        assert!(apply(&mut state, &long).is_err());
        assert!(apply(&mut state, &reply(2, "Q1", "a/1", &[])).unwrap());
        let view = view(&state).unwrap();
        assert_eq!((view[0].open, view[0].recipients.clone()), (false, vec!["a/1".to_owned()]));
    }

    fn silent_signal(state: &mut Value, seq: u64, silence: &Silence) {
        let envelope = silence_envelope("p", silence, T0.into()).unwrap();
        let mut raised = event(seq, "signal.received", tower_api::TOWER, &silence.target, T0, serde_json::to_value(envelope).unwrap());
        raised.eid = silence_eid(&silence.request);
        assert!(tower_api::apply(state, &raised).unwrap());
    }
    fn ms(at: &str) -> u64 {
        parse_time(at).unwrap().timestamp_millis() as u64
    }

    #[test]
    fn silence_is_passive_then_active_on_the_second_in_a_row() {
        let mut state = json!({});
        apply(&mut state, &sent(1, "Q1", "T-1", "report", "2026-10-23T02:00:00Z", &["a/1"])).unwrap();
        apply(&mut state, &sent(2, "Q2", "T-1", "report", "2026-10-23T04:00:00Z", &["a/1"])).unwrap();
        apply(&mut state, &sent(3, "Q3", "T-2", "question", "2026-10-23T02:00:00Z", &[])).unwrap();
        assert!(silences(&state, &[], ms("2026-10-23T01:59:59Z")).unwrap().is_empty(), "not due yet");
        let found = silences(&state, &[], ms("2026-10-23T03:00:00Z")).unwrap();
        assert_eq!(found.iter().map(|s| (s.request.as_str(), s.level)).collect::<Vec<_>>(), [("Q1", Level::Passive)],
            "the queued question has nobody to be silent");
        silent_signal(&mut state, 4, &found[0]);
        assert!(silences(&state, &[], ms("2026-10-23T03:00:00Z")).unwrap().is_empty(), "raised once");
        let found = silences(&state, &[], ms("2026-10-23T05:00:00Z")).unwrap();
        assert_eq!(found.iter().map(|s| (s.request.as_str(), s.level)).collect::<Vec<_>>(), [("Q2", Level::Active)]);
        let signal = silence_signal(&found[0]);
        assert!(signal.check().is_ok());
        assert!(signal.title.starts_with("失联："));
        let log = tower_api::read(&state).unwrap();
        assert_eq!(log.silences[0].refs, ["request:Q1"]);
        assert_eq!(raised(&log, "Q1").map(|mark| mark.seq), Some(4));
    }

    #[test]
    fn a_reply_between_two_requests_resets_the_streak_and_both_found_at_once_escalate() {
        let mut state = json!({});
        for (seq, id, due) in [(1, "Q1", "2026-10-23T02:00:00Z"), (2, "Q2", "2026-10-23T03:00:00Z"), (3, "Q3", "2026-10-23T04:00:00Z")] {
            apply(&mut state, &sent(seq, id, "T-1", "report", due, &["a/1"])).unwrap();
        }
        apply(&mut state, &reply(4, "Q2", "a/1", &[])).unwrap();
        let found = silences(&state, &[], ms("2026-10-23T05:00:00Z")).unwrap();
        assert_eq!(found.iter().map(|s| (s.request.as_str(), s.level)).collect::<Vec<_>>(),
            [("Q1", Level::Passive), ("Q3", Level::Passive)]);
        let mut fresh = json!({});
        apply(&mut fresh, &sent(1, "Q1", "T-1", "report", "2026-10-23T02:00:00Z", &["a/1"])).unwrap();
        apply(&mut fresh, &sent(2, "Q2", "T-1", "report", "2026-10-23T03:00:00Z", &["a/1"])).unwrap();
        let found = silences(&fresh, &[], ms("2026-10-23T05:00:00Z")).unwrap();
        assert_eq!(found.iter().map(|s| s.level).collect::<Vec<_>>(), [Level::Passive, Level::Active]);
    }

    #[test]
    fn delegates_are_named_like_the_cli_names_them() {
        let source = SelfReported { name: "claude-code".into(), version: "2.1".into(), vendor: None, model: None, skills: None };
        assert_eq!(delegate_id("A-1", Some(&source)), "claude-code/2.1");
        assert_eq!(delegate_id("A-1", None), "A-1");
    }
}
