//! Follow-ups (core spec FR-134): a person's reminder for later, with a
//! condition that makes it due. At most `MAX_OPEN` are open at a time. A
//! follow-up is a hub event, never a file: `follow_up.added` and
//! `follow_up.done` are signed `human:<id>` and sent from the App only, so
//! the open list is rebuilt from a snapshot plus the outbox like any other
//! group. `resurfaced.seen` records that a person looked at an object that
//! came back from quiet (FR-136); it changes no group, its time is read from
//! the event history (`crate::quiet`). The `FollowUp` group is created on
//! first use, so older projections stay byte-identical.
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation_projection::Mark,
    hub::Event,
    question_model::{human_actor, valid_id},
    tower_api::{Merged, Signal},
};

pub const GROUP: &str = "FollowUp";
/// A person's attention bookkeeping, like `signal.dismissed`.
pub const LAYER: &str = "L5";
pub const ADDED: &str = "follow_up.added";
pub const DONE: &str = "follow_up.done";
/// FR-136: a person looked at an object that resurfaced.
pub const SEEN: &str = "resurfaced.seen";
/// FR-134: at most this many open follow-ups.
pub const MAX_OPEN: usize = 5;
/// One sentence: at most this many characters.
pub const TEXT_CHARS: usize = 200;

/// The condition that makes a follow-up due.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "snake_case", deny_unknown_fields)]
pub enum When {
    /// At or after this RFC 3339 time.
    Date { at: String },
    /// Once the object is complete (its derived phase is done).
    Done { target: String },
    /// Once something other than a person records an event on the object.
    Activity { target: String },
    /// Once a signal on the object arrives, of this kind if one is given.
    Signal {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
}

impl When {
    /// The object it watches; a date watches none.
    pub fn target(&self) -> Option<&str> {
        match self {
            When::Date { .. } => None,
            When::Done { target } | When::Activity { target } | When::Signal { target, .. } => Some(target),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if let When::Date { at } = self {
            time(at).ok_or("跟进日期须是 RFC 3339 时间")?;
        }
        if let Some(target) = self.target() {
            valid_id(target)?;
        }
        if let When::Signal { kind: Some(kind), .. } = self {
            if kind.trim().is_empty() || kind.len() > 128 || kind.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err("跟进的信号类别无效".into());
            }
        }
        Ok(())
    }
}

/// `follow_up.added`. The event's `obj` is the follow-up's own ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Added {
    pub text: String,
    pub when: When,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowUp {
    pub id: String,
    pub text: String,
    pub when: When,
    pub added: Mark,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<Mark>,
}

/// Follow-ups by ID. IDs are UUIDv7 request IDs, so the order is arrival order.
pub type FollowUps = BTreeMap<String, FollowUp>;

pub fn read(state: &Value) -> Result<FollowUps, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(FollowUps::new()),
    }
}

pub fn open(items: &FollowUps) -> impl Iterator<Item = &FollowUp> {
    items.values().filter(|item| item.done.is_none())
}

pub fn check_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > TEXT_CHARS || text.chars().any(char::is_control) {
        return Err(format!("跟进须是一句话：非空、不换行、不超过 {TEXT_CHARS} 字"));
    }
    Ok(())
}

pub fn time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text).ok().map(|at| at.with_timezone(&Utc))
}

fn mark(event: &Event) -> Mark {
    Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() }
}

pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if ![ADDED, DONE, SEEN].contains(&event.kind.as_str()) {
        return Ok(false);
    }
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err(format!("{} 的层或 fences 无效", event.kind));
    }
    if !human_actor(&event.actor) {
        return Err(format!("{} 只能由人在 App 中发出", event.kind));
    }
    valid_id(&event.obj)?;
    if event.kind == SEEN {
        return if event.data == Value::Object(Default::default()) { Ok(true) } else { Err("浮现已读不带正文".into()) };
    }
    let mut items = read(state)?;
    if event.kind == ADDED {
        let body: Added = serde_json::from_value(event.data.clone()).map_err(|e| format!("跟进正文无效：{e}"))?;
        check_text(&body.text)?;
        body.when.check()?;
        if items.contains_key(&event.obj) {
            return Err("跟进ID重复".into());
        }
        if open(&items).count() >= MAX_OPEN {
            return Err(format!("跟进最多 {MAX_OPEN} 条；先完成一条再加"));
        }
        items.insert(event.obj.clone(), FollowUp { id: event.obj.clone(), text: body.text, when: body.when, added: mark(event), done: None });
    } else {
        if event.data != Value::Object(Default::default()) {
            return Err("完成跟进不带正文".into());
        }
        let item = items.get_mut(&event.obj).ok_or("要完成的跟进不存在")?;
        if item.done.is_some() {
            return Err("这条跟进已经完成".into());
        }
        item.done = Some(mark(event));
    }
    state[GROUP] = serde_json::to_value(&items).map_err(|e| e.to_string())?;
    Ok(true)
}

/// What the due judgement may look at; all of it is derived on read.
pub struct Facts<'a> {
    pub now: DateTime<Utc>,
    /// Objects whose derived phase is done.
    pub done: &'a BTreeSet<String>,
    /// The latest event not signed by a person, per object.
    pub others: &'a BTreeMap<String, Mark>,
    pub signals: &'a [Merged<Signal>],
}

/// FR-134: whether an open follow-up is due now. Activity and signals only
/// count when they arrived after the follow-up was added; a date or a done
/// object counts whenever it holds.
pub fn due(item: &FollowUp, facts: &Facts) -> bool {
    if item.done.is_some() {
        return false;
    }
    let after = item.added.seq;
    match &item.when {
        When::Date { at } => time(at).is_some_and(|at| facts.now >= at),
        When::Done { target } => facts.done.contains(target),
        When::Activity { target } => facts.others.get(target).is_some_and(|last| last.seq > after),
        When::Signal { target, kind } => facts.signals.iter().any(|entry| entry.target == *target
            && kind.as_ref().is_none_or(|kind| entry.data.kind == *kind) && entry.last.seq > after),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "0192c2d0-0000-7000-8000-000000000001";

    fn event(seq: u64, kind: &str, actor: &str, obj: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":"2026-10-23T01:00:00Z","actor":actor,
            "type":kind,"layer":"L5","obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn id(n: u64) -> String {
        format!("0192c2d0-0000-7000-8000-{n:012}")
    }

    fn add(state: &mut Value, seq: u64, obj: &str, when: Value) -> Result<bool, String> {
        apply(state, &event(seq, ADDED, "human:local", obj, json!({"text":"看看导出做完没有","when":when})))
    }

    #[test]
    fn other_events_are_not_follow_ups_and_an_untouched_projection_has_no_group() {
        let mut state = json!({});
        assert!(!apply(&mut state, &event(1, "idea.added", "human:local", ID, json!({"text":"x"}))).unwrap());
        assert_eq!(state, json!({}));
        assert!(read(&state).unwrap().is_empty());
    }

    #[test]
    fn a_person_adds_and_completes_a_follow_up_once() {
        let mut state = json!({});
        assert!(add(&mut state, 1, ID, json!({"on":"done","target":"T-1"})).unwrap());
        let items = read(&state).unwrap();
        assert_eq!(items[ID].when, When::Done { target: "T-1".into() });
        assert_eq!(items[ID].added.actor, "human:local");
        assert_eq!(add(&mut state, 2, ID, json!({"on":"done","target":"T-1"})).unwrap_err(), "跟进ID重复");
        assert!(apply(&mut state, &event(3, DONE, "human:local", ID, json!({}))).unwrap());
        assert_eq!(read(&state).unwrap()[ID].done.as_ref().unwrap().seq, 3);
        assert_eq!(apply(&mut state, &event(4, DONE, "human:local", ID, json!({}))).unwrap_err(), "这条跟进已经完成");
        assert_eq!(apply(&mut state, &event(5, DONE, "human:local", &id(9), json!({}))).unwrap_err(), "要完成的跟进不存在");
        assert!(apply(&mut state, &event(6, DONE, "human:local", ID, json!({"x":1}))).is_err());
    }

    #[test]
    fn only_a_person_on_the_attention_layer_writes_follow_ups() {
        let mut state = json!({});
        for actor in ["process:towermd-cli", "process:towermd", "claude-code/2.1"] {
            let error = apply(&mut state, &event(1, ADDED, actor, ID, json!({"text":"x","when":{"on":"date","at":"2026-11-01T00:00:00Z"}}))).unwrap_err();
            assert!(error.contains("只能由人"), "{actor}: {error}");
            assert!(apply(&mut state, &event(1, SEEN, actor, "T-1", json!({}))).is_err());
        }
        let mut wrong = event(1, ADDED, "human:local", ID, json!({"text":"x","when":{"on":"date","at":"2026-11-01T00:00:00Z"}}));
        wrong.layer = "L4".into();
        assert!(apply(&mut state, &wrong).is_err());
        assert_eq!(state, json!({}));
    }

    #[test]
    fn malformed_follow_ups_are_rejected() {
        let mut state = json!({});
        for when in [
            json!({"on":"date","at":"next week"}),
            json!({"on":"done","target":"../T"}),
            json!({"on":"activity"}),
            json!({"on":"signal","target":"T-1","kind":"a b"}),
            json!({"on":"pin","target":"T-1"}),
            json!({"on":"done","target":"T-1","extra":1}),
        ] {
            assert!(add(&mut state, 1, ID, when.clone()).is_err(), "{when}");
        }
        for text in ["", "  ", "两行\n文本", &"长".repeat(TEXT_CHARS + 1)] {
            assert!(apply(&mut state, &event(1, ADDED, "human:local", ID, json!({"text":text,"when":{"on":"done","target":"T-1"}}))).is_err());
        }
        assert!(apply(&mut state, &event(1, ADDED, "human:local", ID, json!({"text":"x","when":{"on":"done","target":"T-1"},"pin":true}))).is_err());
        assert_eq!(state, json!({}));
    }

    #[test]
    fn at_most_five_are_open_and_completing_one_makes_room() {
        let mut state = json!({});
        for n in 1..=MAX_OPEN as u64 {
            add(&mut state, n, &id(n), json!({"on":"date","at":"2026-11-01T00:00:00+09:00"})).unwrap();
        }
        let error = add(&mut state, 6, &id(6), json!({"on":"date","at":"2026-11-01T00:00:00Z"})).unwrap_err();
        assert!(error.contains("最多 5 条"), "{error}");
        apply(&mut state, &event(7, DONE, "human:local", &id(1), json!({}))).unwrap();
        add(&mut state, 8, &id(6), json!({"on":"date","at":"2026-11-01T00:00:00Z"})).unwrap();
        assert_eq!(open(&read(&state).unwrap()).count(), MAX_OPEN);
        assert_eq!(read(&state).unwrap().len(), 6);
    }

    #[test]
    fn seen_is_accepted_without_a_body_and_changes_no_group() {
        let mut state = json!({});
        assert!(apply(&mut state, &event(1, SEEN, "human:local", "T-1", json!({}))).unwrap());
        assert_eq!(state, json!({}));
        assert!(apply(&mut state, &event(2, SEEN, "human:local", "T-1", json!({"x":1}))).is_err());
    }

    fn follow_up(seq: u64, when: When) -> FollowUp {
        FollowUp { id: ID.into(), text: "x".into(), when,
            added: Mark { at: "2026-10-23T01:00:00Z".into(), eid: format!("e{seq}"), seq, actor: "human:local".into() }, done: None }
    }

    fn signal(target: &str, kind: &str, seq: u64) -> Merged<Signal> {
        serde_json::from_value(json!({"from":{"id":"pkg:eng@1/crap","kind":"source"},"target":target,
            "data":{"kind":kind,"level":"passive","title":"t"},"at":"2026-10-23T01:00:00Z","count":1,
            "first":{"at":"2026-10-23T01:00:00Z","eid":"s","seq":seq,"actor":"process:towermd-cli"},
            "last":{"at":"2026-10-23T01:00:00Z","eid":"s","seq":seq,"actor":"process:towermd-cli"}})).unwrap()
    }

    #[test]
    fn each_condition_decides_when_a_follow_up_is_due() {
        let now = time("2026-11-01T00:00:00Z").unwrap();
        let done = BTreeSet::from(["T-done".to_owned()]);
        let mark = |seq| Mark { at: "x".into(), eid: "o".into(), seq, actor: "claude-code/2.1".into() };
        let others = BTreeMap::from([("T-old".to_owned(), mark(5)), ("T-new".to_owned(), mark(20))]);
        let signals = [signal("T-sig", "eng.crap", 20), signal("T-stale", "eng.crap", 5)];
        let facts = Facts { now, done: &done, others: &others, signals: &signals };
        let when = |when| due(&follow_up(10, when), &facts);
        assert!(when(When::Date { at: "2026-11-01T09:00:00+09:00".into() }));
        assert!(!when(When::Date { at: "2026-11-01T00:00:01Z".into() }));
        assert!(when(When::Done { target: "T-done".into() }));
        assert!(!when(When::Done { target: "T-open".into() }));
        assert!(when(When::Activity { target: "T-new".into() }));
        assert!(!when(When::Activity { target: "T-old".into() }), "activity before the follow-up does not count");
        assert!(when(When::Signal { target: "T-sig".into(), kind: None }));
        assert!(when(When::Signal { target: "T-sig".into(), kind: Some("eng.crap".into()) }));
        assert!(!when(When::Signal { target: "T-sig".into(), kind: Some("eng.lint".into()) }));
        assert!(!when(When::Signal { target: "T-stale".into(), kind: None }), "a signal before the follow-up does not count");
        let mut closed = follow_up(10, When::Done { target: "T-done".into() });
        closed.done = Some(mark(30));
        assert!(!due(&closed, &facts), "a completed follow-up is never due");
    }
}
