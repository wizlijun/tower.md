//! Decision review dates (core spec FR-186). A decision recorded from an
//! answer may carry `review_on`, a calendar day. From that day on the
//! decision is an attention candidate ("该复查了"), with the question, the
//! decision and its reason. A person settles it in the App with one
//! `decision.reviewed` event, signed `human:<id>`: keep it (optionally with
//! the next review day) or void it. A voided decision is never cited again
//! (FR-62). Rewriting is answering the question anew with a decision that
//! supersedes this one.
//!
//! The `DecisionReview` group is created on the first review, so older
//! projections stay byte-identical. Whether a decision is due is derived on
//! every read from the date and `now`; nothing about "due" is stored.
use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation_projection::Mark,
    hub::Event,
    question_model::{check_review_on, human_actor, valid_id},
};

pub const GROUP: &str = "DecisionReview";
/// Decisions are L4 objects; settling one is a decision about it.
pub const LAYER: &str = "L4";
pub const REVIEWED: &str = "decision.reviewed";
/// Where answered decisions live in the projection.
pub const DECISIONS: &str = "QuestionDecision";

/// The body of `decision.reviewed`. The event's `obj` is the decision ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reviewed {
    /// The decision stands; `review_on` sets the next review, none ends reviews.
    Keep {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        review_on: Option<String>,
    },
    /// The decision no longer holds and is never cited again. A struct
    /// variant, so unknown fields are refused like on `Keep`.
    Void {},
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub id: String,
    /// The next review day after the latest keep.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<Mark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voided: Option<Mark>,
}

/// Reviews by decision ID.
pub type Reviews = BTreeMap<String, Review>;

pub fn read(state: &Value) -> Result<Reviews, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(Reviews::new()),
    }
}

/// Decisions a person voided on review.
pub fn voided(state: &Value) -> Result<Vec<String>, String> {
    Ok(read(state)?.into_values().filter(|review| review.voided.is_some()).map(|review| review.id).collect())
}

/// The UTC calendar day of a time in milliseconds.
pub fn day(ms: u64) -> String {
    i64::try_from(ms).ok().and_then(DateTime::<Utc>::from_timestamp_millis)
        .map(|at| at.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// The day a decision comes back: the date set at the latest keep, else
/// its own; none once voided or kept without a new date.
pub fn next_review<'a>(own: Option<&'a str>, review: Option<&'a Review>) -> Option<&'a str> {
    match review {
        Some(review) if review.voided.is_some() => None,
        Some(review) if review.kept.is_some() => review.review_on.as_deref(),
        _ => own,
    }
}

/// Due from the review day on (UTC).
pub fn due(review_on: &str, now: u64) -> bool {
    check_review_on(review_on).is_ok() && day(now).as_str() >= review_on
}

/// Accepts one `decision.reviewed` from a person on an answered decision
/// that is neither superseded nor already voided. `Ok(false)` for other events.
pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != REVIEWED {
        return Ok(false);
    }
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err(format!("{REVIEWED} 的层或 fences 无效"));
    }
    if !human_actor(&event.actor) {
        return Err("复查决策只能由人在 App 中完成".into());
    }
    valid_id(&event.obj)?;
    let decisions = state.get(DECISIONS).and_then(Value::as_object);
    if !decisions.is_some_and(|decisions| decisions.contains_key(&event.obj)) {
        return Err("要复查的决策不存在".into());
    }
    let superseded = decisions.into_iter().flat_map(|decisions| decisions.values())
        .flat_map(|decision| decision.get("supersedes").and_then(Value::as_array).into_iter().flatten())
        .any(|replaced| replaced.as_str() == Some(event.obj.as_str()));
    if superseded {
        return Err("这条决策已被新决策替代".into());
    }
    let body: Reviewed = serde_json::from_value(event.data.clone()).map_err(|e| format!("复查结果无效：{e}"))?;
    let mut reviews = read(state)?;
    let review = reviews.entry(event.obj.clone()).or_insert_with(|| Review { id: event.obj.clone(), review_on: None, kept: None, voided: None });
    if review.voided.is_some() {
        return Err("这条决策已作废".into());
    }
    let mark = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
    match body {
        Reviewed::Keep { review_on } => {
            if let Some(next) = &review_on {
                check_review_on(next)?;
                let today = DateTime::parse_from_rfc3339(&event.t).map(|at| at.with_timezone(&Utc).format("%Y-%m-%d").to_string())
                    .map_err(|_| "事件时间无效")?;
                if *next <= today {
                    return Err("下次复查日期须晚于今天".into());
                }
            }
            review.review_on = review_on;
            review.kept = Some(mark);
        }
        Reviewed::Void {} => review.voided = Some(mark),
    }
    state[GROUP] = serde_json::to_value(&reviews).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(seq: u64, actor: &str, obj: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":"2026-10-23T01:00:00Z","actor":actor,
            "type":REVIEWED,"layer":LAYER,"obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn decisions() -> Value {
        json!({DECISIONS: {
            "D-1": {"id":"D-1","module":"A","question":"格式？","decision":"JSON","supersedes":[],"review_on":"2026-10-20"},
            "D-2": {"id":"D-2","module":"A","question":"格式？","decision":"YAML","supersedes":["D-old"]},
            "D-old": {"id":"D-old","module":"A","question":"格式？","decision":"CSV","supersedes":[]}}})
    }

    #[test]
    fn other_events_are_ignored_and_no_group_is_created() {
        let mut state = decisions();
        let mut other = event(1, "human:local", "D-1", json!({"outcome":"void"}));
        other.kind = "idea.added".into();
        assert!(!apply(&mut state, &other).unwrap());
        assert_eq!(state, decisions());
        assert!(read(&state).unwrap().is_empty());
    }

    #[test]
    fn keep_moves_the_next_review_and_void_retires_the_decision() {
        let mut state = decisions();
        assert_eq!(next_review(Some("2026-10-20"), None), Some("2026-10-20"));
        assert!(apply(&mut state, &event(1, "human:local", "D-1", json!({"outcome":"keep","review_on":"2027-01-01"}))).unwrap());
        let reviews = read(&state).unwrap();
        assert_eq!(next_review(Some("2026-10-20"), reviews.get("D-1")), Some("2027-01-01"));
        apply(&mut state, &event(2, "human:local", "D-1", json!({"outcome":"keep"}))).unwrap();
        assert_eq!(next_review(Some("2026-10-20"), read(&state).unwrap().get("D-1")), None, "kept without a date: no more reviews");
        apply(&mut state, &event(3, "human:local", "D-1", json!({"outcome":"void"}))).unwrap();
        assert_eq!(voided(&state).unwrap(), vec!["D-1".to_owned()]);
        assert_eq!(apply(&mut state, &event(4, "human:local", "D-1", json!({"outcome":"keep"}))).unwrap_err(), "这条决策已作废");
    }

    #[test]
    fn only_a_person_reviews_an_existing_current_decision() {
        let mut state = decisions();
        for actor in ["process:towermd-cli", "claude-code/2.1"] {
            assert!(apply(&mut state, &event(1, actor, "D-1", json!({"outcome":"void"}))).unwrap_err().contains("只能由人"));
        }
        assert_eq!(apply(&mut state, &event(1, "human:local", "D-9", json!({"outcome":"void"}))).unwrap_err(), "要复查的决策不存在");
        assert_eq!(apply(&mut state, &event(1, "human:local", "D-old", json!({"outcome":"void"}))).unwrap_err(), "这条决策已被新决策替代");
        for data in [json!({"outcome":"keep","review_on":"2026-10-23"}), json!({"outcome":"keep","review_on":"next week"}),
            json!({"outcome":"rewrite"}), json!({"outcome":"void","x":1})] {
            assert!(apply(&mut state, &event(1, "human:local", "D-1", data.clone())).is_err(), "{data}");
        }
        let mut wrong = event(1, "human:local", "D-1", json!({"outcome":"void"}));
        wrong.layer = "L5".into();
        assert!(apply(&mut state, &wrong).is_err());
        assert_eq!(state, decisions());
    }

    #[test]
    fn a_review_day_is_due_from_its_utc_morning() {
        let at = |text: &str| u64::try_from(DateTime::parse_from_rfc3339(text).unwrap().timestamp_millis()).unwrap();
        assert!(!due("2026-10-24", at("2026-10-23T23:59:59Z")));
        assert!(due("2026-10-24", at("2026-10-24T00:00:00Z")));
        assert!(due("2026-10-01", at("2026-10-24T00:00:00Z")));
        assert!(!due("soon", at("2026-10-24T00:00:00Z")));
    }
}
