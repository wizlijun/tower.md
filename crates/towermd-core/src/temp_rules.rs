//! Temporary rules (core spec FR-69): a person's standing instruction such as
//! "no dependency bumps tonight", with a scope and an expiry. A rule is a hub
//! event, never a file, so it is archived with the snapshot and rebuilt from
//! a snapshot plus the outbox like any other group. `rule.temp.added` is a
//! person's act, signed `human:<id>` and sent from the App only;
//! `rule.temp.expired` is the daemon's record that the expiry passed, signed
//! `process:towermd`. A rule stops applying at `until` whether or not the
//! expiry was recorded yet. The `TempRule` group is created on first use, so
//! older projections stay byte-identical.
use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation_projection::Mark,
    hub::Event,
    question_model::{human_actor, valid_id, PromptScope, RunContext},
};

pub const GROUP: &str = "TempRule";
/// Set by a person, like a frame.
pub const LAYER: &str = "L3";
pub const ADDED: &str = "rule.temp.added";
pub const EXPIRED: &str = "rule.temp.expired";
/// The only signer of an expiry.
pub const EXPIRER: &str = "process:towermd";
/// One sentence: at most this many characters.
pub const TEXT_CHARS: usize = 200;
/// A temporary rule lasts at most this long.
pub const MAX_DAYS: i64 = 30;

/// `rule.temp.added`. The event's `obj` is the rule's own ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Added {
    pub text: String,
    pub scope: PromptScope,
    /// RFC 3339; the rule applies strictly before it.
    pub until: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempRule {
    pub id: String,
    pub text: String,
    pub scope: PromptScope,
    pub until: String,
    pub added: Mark,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expired: Option<Mark>,
}

impl TempRule {
    /// In force at `now`: before its expiry, which is never extended.
    pub fn active(&self, now: DateTime<Utc>) -> bool {
        self.expired.is_none() && time(&self.until).is_some_and(|until| now < until)
    }

    /// Past its expiry but not yet recorded as expired.
    pub fn lapsed(&self, now: DateTime<Utc>) -> bool {
        self.expired.is_none() && !self.active(now)
    }
}

/// Rules by ID. IDs carry UUIDv7 request IDs, so the order is arrival order.
pub type TempRules = BTreeMap<String, TempRule>;

pub fn read(state: &Value) -> Result<TempRules, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(TempRules::new()),
    }
}

/// The rules in force at `now` that cover this run, oldest first.
pub fn covering<'a>(rules: &'a TempRules, run: &RunContext, now: DateTime<Utc>) -> Vec<&'a TempRule> {
    rules.values().filter(|rule| rule.active(now) && rule.scope.covers(run)).collect()
}

pub fn check_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > TEXT_CHARS || text.chars().any(char::is_control) {
        return Err(format!("临时规则须是一句话：非空、不换行、不超过 {TEXT_CHARS} 字"));
    }
    Ok(())
}

pub fn check_scope(scope: &PromptScope) -> Result<(), String> {
    match scope {
        PromptScope::Global => Ok(()),
        PromptScope::Scoped { scope } => valid_id(scope),
        PromptScope::Task { task } => valid_id(task),
    }
}

/// `until` is after `at` and at most `MAX_DAYS` later.
pub fn check_until(until: &str, at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    let until = time(until).ok_or("临时规则的到期时间须是 RFC 3339 时间")?;
    if until <= at {
        return Err("临时规则的到期时间须晚于现在".into());
    }
    if until > at + Duration::days(MAX_DAYS) {
        return Err(format!("临时规则最长 {MAX_DAYS} 天"));
    }
    Ok(until)
}

pub fn time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text).ok().map(|at| at.with_timezone(&Utc))
}

fn mark(event: &Event) -> Mark {
    Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() }
}

pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if ![ADDED, EXPIRED].contains(&event.kind.as_str()) {
        return Ok(false);
    }
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err(format!("{} 的层或 fences 无效", event.kind));
    }
    valid_id(&event.obj)?;
    let at = time(&event.t).ok_or("事件时间无效")?;
    let mut rules = read(state)?;
    if event.kind == ADDED {
        if !human_actor(&event.actor) {
            return Err("临时规则只能由人在 App 中创建".into());
        }
        let body: Added = serde_json::from_value(event.data.clone()).map_err(|e| format!("临时规则正文无效：{e}"))?;
        check_text(&body.text)?;
        check_scope(&body.scope)?;
        check_until(&body.until, at)?;
        if rules.contains_key(&event.obj) {
            return Err("临时规则ID重复".into());
        }
        rules.insert(event.obj.clone(), TempRule { id: event.obj.clone(), text: body.text, scope: body.scope,
            until: body.until, added: mark(event), expired: None });
    } else {
        if event.actor != EXPIRER {
            return Err("临时规则的到期只能由守护进程记录".into());
        }
        if event.data != Value::Object(Default::default()) {
            return Err("临时规则到期不带正文".into());
        }
        let rule = rules.get_mut(&event.obj).ok_or("要到期的临时规则不存在")?;
        if rule.expired.is_some() {
            return Err("这条临时规则已经到期".into());
        }
        if rule.active(at) {
            return Err("临时规则尚未到期".into());
        }
        rule.expired = Some(mark(event));
    }
    state[GROUP] = serde_json::to_value(&rules).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "TR-0192c2d0-0000-7000-8000-000000000001";

    fn event(seq: u64, t: &str, kind: &str, actor: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":t,"actor":actor,
            "type":kind,"layer":LAYER,"obj":ID,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn add(until: &str) -> Event {
        event(1, "2026-10-24T10:00:00.000Z", ADDED, "human:local",
            json!({"text":"今晚不要改依赖版本","scope":{"kind":"module","module":"A"},"until":until}))
    }

    fn run(task: &str, module: &str) -> RunContext {
        RunContext { run: format!("R-{task}"), task: task.into(), scope: module.into() }
    }

    #[test]
    fn other_events_are_not_rules_and_an_untouched_projection_has_no_group() {
        let mut state = json!({});
        assert!(!apply(&mut state, &event(1, "2026-10-24T10:00:00Z", "follow_up.added", "human:local", json!({}))).unwrap());
        assert_eq!(state, json!({}));
        assert!(read(&state).unwrap().is_empty());
    }

    #[test]
    fn a_rule_applies_strictly_before_its_expiry_and_only_in_scope() {
        let mut state = json!({});
        assert!(apply(&mut state, &add("2026-10-25T06:00:00+08:00")).unwrap());
        let rules = read(&state).unwrap();
        let before = time("2026-10-24T21:59:59.999Z").unwrap();
        let at = time("2026-10-24T22:00:00Z").unwrap();
        assert_eq!(covering(&rules, &run("T-1", "A"), before).len(), 1);
        assert!(covering(&rules, &run("T-2", "B"), before).is_empty(), "another module is out of scope");
        assert!(covering(&rules, &run("T-1", "A"), at).is_empty(), "the rule lapses at its expiry");
        assert!(rules[ID].lapsed(at) && !rules[ID].lapsed(before));
    }

    #[test]
    fn only_a_person_adds_and_only_the_daemon_records_the_expiry_after_it_passed() {
        for actor in ["process:towermd-cli", "process:towermd", "claude-code/2.1"] {
            let mut forged = add("2026-10-25T00:00:00Z");
            forged.actor = actor.into();
            assert!(apply(&mut json!({}), &forged).unwrap_err().contains("只能由人"), "{actor}");
        }
        let mut state = json!({});
        apply(&mut state, &add("2026-10-25T00:00:00Z")).unwrap();
        let early = event(2, "2026-10-24T23:59:59Z", EXPIRED, EXPIRER, json!({}));
        assert_eq!(apply(&mut state, &early).unwrap_err(), "临时规则尚未到期");
        let by_person = event(3, "2026-10-25T00:00:00Z", EXPIRED, "human:local", json!({}));
        assert!(apply(&mut state, &by_person).is_err());
        let expired = event(4, "2026-10-25T00:00:00Z", EXPIRED, EXPIRER, json!({}));
        assert!(apply(&mut state, &expired).unwrap());
        assert_eq!(read(&state).unwrap()[ID].expired.as_ref().unwrap().seq, 4);
        assert_eq!(apply(&mut state, &event(5, "2026-10-25T01:00:00Z", EXPIRED, EXPIRER, json!({}))).unwrap_err(), "这条临时规则已经到期");
    }

    #[test]
    fn the_expiry_is_after_the_event_and_at_most_thirty_days_later() {
        for until in ["2026-10-24T10:00:00Z", "2026-10-24T09:00:00Z", "2026-11-23T10:00:00.001Z", "tonight"] {
            assert!(apply(&mut json!({}), &add(until)).is_err(), "{until}");
        }
        assert!(apply(&mut json!({}), &add("2026-11-23T10:00:00Z")).unwrap(), "exactly thirty days is allowed");
    }

    #[test]
    fn malformed_rules_are_rejected() {
        let mut state = json!({});
        for data in [
            json!({"text":"","scope":{"kind":"global"},"until":"2026-10-25T00:00:00Z"}),
            json!({"text":"两行\n文本","scope":{"kind":"global"},"until":"2026-10-25T00:00:00Z"}),
            json!({"text":"x".repeat(TEXT_CHARS + 1),"scope":{"kind":"global"},"until":"2026-10-25T00:00:00Z"}),
            json!({"text":"x","scope":{"kind":"task","task":"../T"},"until":"2026-10-25T00:00:00Z"}),
            json!({"text":"x","scope":{"kind":"global"},"until":"2026-10-25T00:00:00Z","layer":"L0"}),
        ] {
            assert!(apply(&mut state, &event(1, "2026-10-24T10:00:00Z", ADDED, "human:local", data.clone())).is_err(), "{data}");
        }
        let mut wrong = add("2026-10-25T00:00:00Z");
        wrong.layer = "L0".into();
        assert!(apply(&mut state, &wrong).is_err());
        assert_eq!(state, json!({}));
        apply(&mut state, &add("2026-10-25T00:00:00Z")).unwrap();
        assert_eq!(apply(&mut state, &add("2026-10-25T00:00:00Z")).unwrap_err(), "临时规则ID重复");
    }
}
