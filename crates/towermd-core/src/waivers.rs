//! Waivers (core spec FR-03, FR-72): the only way past a hard gate. A
//! person grants one in the App, signed `human:<id>`, for one declared gate
//! on one object, with a reason and an expiry of at most 30 days. It is a
//! temporary deviation: it changes no version and no declaration, and it
//! lapses at `until` with nothing written, since every judgement is made at
//! a given time. It is never extended; once it lapsed, a person may grant a
//! new one. The waiver event is the decision on record, and every gate
//! evidence written while it applies names it. The `Waiver` group is created
//! on first use, so older projections stay byte-identical.
use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation_projection::Mark,
    hub::Event,
    question_model::{human_actor, valid_id},
    temp_rules::time,
};

pub const GROUP: &str = "Waiver";
/// Set by a person, like a temporary rule.
pub const LAYER: &str = "L3";
pub const GRANTED: &str = "waiver.granted";
/// One sentence: at most this many characters.
pub const REASON_CHARS: usize = 200;
/// What the App proposes when a person does not choose.
pub const DEFAULT_DAYS: i64 = 7;
/// A waiver lasts at most this long.
pub const MAX_DAYS: i64 = 30;
/// Within this long of its expiry a waiver joins the attention list.
pub const SOON_HOURS: i64 = 24;

/// `waiver.granted`. The event's `obj` is the waived object; its eid is the
/// waiver's ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Granted {
    /// The `gates.yaml` entry it lets through.
    pub gate: String,
    pub reason: String,
    /// RFC 3339; the waiver applies strictly before it.
    pub until: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiver {
    pub id: String,
    pub object: String,
    pub gate: String,
    pub reason: String,
    pub until: String,
    pub granted: Mark,
}

impl Waiver {
    /// In force at `now`: before its expiry, which is never extended.
    pub fn active(&self, now: DateTime<Utc>) -> bool {
        time(&self.until).is_some_and(|until| now < until)
    }

    /// In force and lapsing within `SOON_HOURS`.
    pub fn expiring(&self, now: DateTime<Utc>) -> bool {
        self.active(now) && time(&self.until).is_some_and(|until| until - now <= Duration::hours(SOON_HOURS))
    }
}

/// Waivers by ID (the granting event's UUIDv7), so in arrival order.
pub type Waivers = BTreeMap<String, Waiver>;

pub fn read(state: &Value) -> Result<Waivers, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(Waivers::new()),
    }
}

/// The waivers in force at `now`, oldest first.
pub fn active(waivers: &Waivers, now: DateTime<Utc>) -> Vec<Waiver> {
    waivers.values().filter(|waiver| waiver.active(now)).cloned().collect()
}

pub fn check_reason(reason: &str) -> Result<(), String> {
    if reason.trim().is_empty() || reason.chars().count() > REASON_CHARS || reason.chars().any(char::is_control) {
        return Err(format!("豁免理由须是一句话：非空、不换行、不超过 {REASON_CHARS} 字"));
    }
    Ok(())
}

pub fn check_gate(gate: &str) -> Result<(), String> {
    if gate.trim().is_empty() || gate.chars().count() > REASON_CHARS || gate.chars().any(char::is_control) {
        return Err("门禁名无效".into());
    }
    Ok(())
}

/// `until` is after `at` and at most `MAX_DAYS` later.
pub fn check_until(until: &str, at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    let until = time(until).ok_or("豁免的到期时间须是 RFC 3339 时间")?;
    if until <= at {
        return Err("豁免的到期时间须晚于现在".into());
    }
    if until > at + Duration::days(MAX_DAYS) {
        return Err(format!("豁免最长 {MAX_DAYS} 天"));
    }
    Ok(until)
}

pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != GRANTED {
        return Ok(false);
    }
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err(format!("{} 的层或 fences 无效", event.kind));
    }
    if !human_actor(&event.actor) {
        return Err("豁免只能由人在 App 中授予".into());
    }
    valid_id(&event.obj)?;
    let at = time(&event.t).ok_or("事件时间无效")?;
    let body: Granted = serde_json::from_value(event.data.clone()).map_err(|e| format!("豁免正文无效：{e}"))?;
    check_gate(&body.gate)?;
    check_reason(&body.reason)?;
    check_until(&body.until, at)?;
    let mut waivers = read(state)?;
    if waivers.contains_key(&event.eid) {
        return Err("豁免ID重复".into());
    }
    if waivers.values().any(|waiver| waiver.object == event.obj && waiver.gate == body.gate && waiver.active(at)) {
        return Err("这道门禁对该对象已有生效中的豁免；豁免不续期，到期后再另行授予".into());
    }
    let granted = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
    waivers.insert(event.eid.clone(), Waiver { id: event.eid.clone(), object: event.obj.clone(), gate: body.gate,
        reason: body.reason, until: body.until, granted });
    state[GROUP] = serde_json::to_value(&waivers).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn grant(seq: u64, t: &str, actor: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("0192c2d0-0000-7000-8000-00000000000{seq}"),"host":"h","t":t,
            "actor":actor,"type":GRANTED,"layer":LAYER,"obj":"T-1","data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn body(until: &str) -> Value {
        json!({"gate":"crap","reason":"上游修复前先放行","until":until})
    }

    #[test]
    fn other_events_are_not_waivers_and_an_untouched_projection_has_no_group() {
        let mut state = json!({});
        let mut other = grant(1, "2026-10-24T10:00:00Z", "human:local", body("2026-10-25T00:00:00Z"));
        other.kind = "rule.temp.added".into();
        assert!(!apply(&mut state, &other).unwrap());
        assert_eq!(state, json!({}));
        assert!(read(&state).unwrap().is_empty());
    }

    #[test]
    fn a_waiver_applies_strictly_before_its_expiry_and_joins_attention_in_its_last_day() {
        let mut state = json!({});
        assert!(apply(&mut state, &grant(1, "2026-10-24T10:00:00Z", "human:local", body("2026-10-31T10:00:00+08:00"))).unwrap());
        let waivers = read(&state).unwrap();
        let waiver = waivers.values().next().unwrap();
        assert_eq!((waiver.object.as_str(), waiver.gate.as_str(), waiver.granted.actor.as_str()), ("T-1", "crap", "human:local"));
        let before = time("2026-10-31T01:59:59.999Z").unwrap();
        let at = time("2026-10-31T02:00:00Z").unwrap();
        assert!(waiver.active(before) && !waiver.active(at), "it lapses at its expiry");
        assert_eq!(active(&waivers, before).len(), 1);
        assert!(active(&waivers, at).is_empty());
        assert!(waiver.expiring(time("2026-10-30T02:00:00Z").unwrap()));
        assert!(!waiver.expiring(time("2026-10-30T01:59:59Z").unwrap()));
        assert!(!waiver.expiring(at));
    }

    #[test]
    fn only_a_person_grants_and_never_twice_for_the_same_gate_while_one_applies() {
        for actor in ["process:towermd-cli", "process:towermd", "claude-code/2.1", "human:"] {
            let forged = grant(1, "2026-10-24T10:00:00Z", actor, body("2026-10-25T00:00:00Z"));
            assert!(apply(&mut json!({}), &forged).is_err(), "{actor}");
        }
        let mut state = json!({});
        apply(&mut state, &grant(1, "2026-10-24T10:00:00Z", "human:local", body("2026-10-25T00:00:00Z"))).unwrap();
        let extend = grant(2, "2026-10-24T23:59:59Z", "human:local", body("2026-11-20T00:00:00Z"));
        assert!(apply(&mut state, &extend).unwrap_err().contains("不续期"));
        let other_gate = grant(3, "2026-10-24T11:00:00Z", "human:local", json!({"gate":"lint","reason":"r","until":"2026-10-25T00:00:00Z"}));
        assert!(apply(&mut state, &other_gate).unwrap());
        let after = grant(4, "2026-10-25T00:00:00Z", "human:local", body("2026-10-26T00:00:00Z"));
        assert!(apply(&mut state, &after).unwrap(), "a new waiver once the old one lapsed");
        assert_eq!(read(&state).unwrap().len(), 3);
        assert_eq!(apply(&mut state, &after).unwrap_err(), "豁免ID重复");
    }

    #[test]
    fn the_expiry_is_after_the_grant_and_at_most_thirty_days_later() {
        for until in ["2026-10-24T10:00:00Z", "2026-10-24T09:00:00Z", "2026-11-23T10:00:00.001Z", "next week"] {
            assert!(apply(&mut json!({}), &grant(1, "2026-10-24T10:00:00Z", "human:local", body(until))).is_err(), "{until}");
        }
        assert!(apply(&mut json!({}), &grant(1, "2026-10-24T10:00:00Z", "human:local", body("2026-11-23T10:00:00Z"))).unwrap());
    }

    #[test]
    fn malformed_waivers_are_rejected() {
        let mut state = json!({});
        for data in [
            json!({"gate":"crap","reason":"","until":"2026-10-25T00:00:00Z"}),
            json!({"gate":"crap","reason":"两行\n理由","until":"2026-10-25T00:00:00Z"}),
            json!({"gate":"crap","reason":"x".repeat(REASON_CHARS + 1),"until":"2026-10-25T00:00:00Z"}),
            json!({"gate":" ","reason":"r","until":"2026-10-25T00:00:00Z"}),
            json!({"gate":"crap","reason":"r"}),
            json!({"gate":"crap","reason":"r","until":"2026-10-25T00:00:00Z","layer":"L0"}),
        ] {
            assert!(apply(&mut state, &grant(1, "2026-10-24T10:00:00Z", "human:local", data.clone())).is_err(), "{data}");
        }
        let mut wrong = grant(1, "2026-10-24T10:00:00Z", "human:local", body("2026-10-25T00:00:00Z"));
        wrong.layer = "L2".into();
        assert!(apply(&mut state, &wrong).is_err());
        let mut path = grant(1, "2026-10-24T10:00:00Z", "human:local", body("2026-10-25T00:00:00Z"));
        path.obj = "../T-1".into();
        assert!(apply(&mut state, &path).is_err());
        assert_eq!(state, json!({}));
    }
}
