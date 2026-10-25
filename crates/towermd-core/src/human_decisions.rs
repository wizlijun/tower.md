//! Decisions a person records in the App (core spec FR-125, FR-127,
//! FR-206). One `decision.recorded` event, signed `human:<id>`, keeps what
//! the person decided and the message it came from:
//!
//! - the answer to a question asked on a node (FR-127), "记为决策";
//! - a commitment weakening the person accepts, with a one-line reason
//!   (FR-125); the card leaves the list until a newer message arrives.
//!
//! The layer is L3: what a person sets, like the rules they record from a
//! send-back (FR-188 puts L0 to L3 with people). Decisions answered on a
//! held question stay L4 objects of their own group, and a takeaway written
//! when confirming an acceptance (FR-206) is already a person's record in
//! that event, so neither is written again here.
//!
//! An optional `review_on` brings the decision back on that day (FR-186).
//! The group is created on the first record, so older projections stay
//! byte-identical.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation_projection::Mark,
    hub::Event,
    question_model::{check_review_on, human_actor, valid_id},
    requests::{self, Ask},
    tower_api, weakening,
};

pub const GROUP: &str = "HumanDecision";
pub const KIND: &str = "decision.recorded";
/// Set by a person (FR-188).
pub const LAYER: &str = "L3";
/// One sentence, like a proposal's reason.
pub const TEXT_CHARS: usize = 200;

/// What the decision was made on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    /// An answered node question, by request ID.
    Reply { request: String },
    /// An accepted weakening, by the event ID of the latest message the person saw.
    Weakening { event: String },
}

/// The body of `decision.recorded`. The event's `obj` is the decision ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recorded {
    pub text: String,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_on: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub event: Mark,
    pub data: Recorded,
    /// What was asked or raised, copied when recorded so a later merge of
    /// the source does not rewrite it.
    pub question: String,
    /// The object the source was about.
    pub target: String,
}

pub fn read(state: &Value) -> Result<BTreeMap<String, Decision>, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}

/// Event IDs of the weakening messages a person accepted.
pub fn accepted(state: &Value) -> BTreeSet<String> {
    read(state).unwrap_or_default().into_values().filter_map(|decision| match decision.data.source {
        Source::Weakening { event } => Some(event),
        Source::Reply { .. } => None,
    }).collect()
}

/// The question and target a source stands for, if it may be decided on.
fn origin(state: &Value, source: &Source) -> Result<(String, String), String> {
    match source {
        Source::Reply { request } => {
            let found = requests::read(state)?.remove(request)
                .filter(|found| found.data.ask == Ask::Question && found.reply.is_some())
                .ok_or("要记为决策的回答不存在")?;
            Ok((found.data.text, found.data.target))
        }
        Source::Weakening { event } => {
            let log = tower_api::read(state)?;
            let found = log.signals.into_iter()
                .find(|entry| entry.last.eid == *event && weakening::reported(&entry.data.kind, &entry.last.actor))
                .ok_or("要接受的承诺弱化不存在")?;
            Ok((found.data.title, found.target))
        }
    }
}

/// Accepts one `decision.recorded` from a person on a source not decided
/// before. `Ok(false)` for other events.
pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != KIND {
        return Ok(false);
    }
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err(format!("{KIND} 的层或 fences 无效"));
    }
    if !human_actor(&event.actor) {
        return Err("决策只能由人在 App 中记下".into());
    }
    valid_id(&event.obj)?;
    let data: Recorded = serde_json::from_value(event.data.clone()).map_err(|e| format!("决策正文无效：{e}"))?;
    let text = data.text.trim();
    if text.is_empty() || text != data.text || text.chars().count() > TEXT_CHARS {
        return Err(format!("决策须是 1 到 {TEXT_CHARS} 字、首尾无空白的一句话"));
    }
    if let Some(day) = &data.review_on {
        check_review_on(day)?;
    }
    let mut decisions = read(state)?;
    if decisions.contains_key(&event.obj) {
        return Err(format!("决策已存在：{}", event.obj));
    }
    if decisions.values().any(|decision| decision.data.source == data.source) {
        return Err("这条来源已记为决策".into());
    }
    let (question, target) = origin(state, &data.source)?;
    let mark = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
    decisions.insert(event.obj.clone(), Decision { id: event.obj.clone(), event: mark, data, question, target });
    state[GROUP] = serde_json::to_value(&decisions).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(seq: u64, actor: &str, obj: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":"2026-10-23T01:00:00Z","actor":actor,
            "type":KIND,"layer":LAYER,"obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn mark(eid: &str, actor: &str) -> Value {
        json!({"at":"2026-10-22T00:00:00Z","eid":eid,"seq":1,"actor":actor})
    }

    fn signal(kind: &str, eid: &str, actor: &str) -> Value {
        json!({"from":{"id":"lint","kind":"source"},"target":"T-1","data":{"kind":kind,"level":"active","title":format!("{kind} 标题")},
            "at":"2026-10-22T00:00:00Z","count":1,"first":mark(eid, actor),"last":mark(eid, actor)})
    }

    fn state() -> Value {
        let request = |ask: &str, replied: bool| {
            let mut value = json!({"id":"x","event":mark("q","human:local"),
                "data":{"target":"T-1","ask":ask,"text":"用哪种格式？","due":"2026-10-30T00:00:00Z","to":["d"]}});
            if replied {
                value["reply"] = json!({"event":mark("r","process:towermd-http"),"data":{"from":"d","state":"claimed_done","summary":"用 JSON"}});
            }
            value
        };
        json!({
            "Requests": {"Q-1": request("question", true), "Q-2": request("question", false), "Q-3": request("report", true)},
            "TowerApi": {"signals": [signal(weakening::KIND, "w1", tower_api::TOWER), signal(weakening::KIND, "w2", "process:towermd-cli"),
                signal("lint.weakening.tests", "w3", "process:towermd-http"), signal("lint.crap", "w4", "process:towermd-http")]},
        })
    }

    #[test]
    fn other_events_are_ignored_and_no_group_is_created() {
        let mut state = state();
        let mut other = event(1, "human:local", "D-1", json!({}));
        other.kind = "idea.added".into();
        assert!(!apply(&mut state, &other).unwrap());
        assert!(state.get(GROUP).is_none());
    }

    #[test]
    fn a_person_records_an_answer_or_accepts_a_weakening_once() {
        let mut state = state();
        let reply = json!({"text":"统一用 JSON","source":{"kind":"reply","request":"Q-1"},"review_on":"2027-01-01"});
        assert!(apply(&mut state, &event(1, "human:local", "D-1", reply.clone())).unwrap());
        let accept = json!({"text":"这次删测试是因为模块拆分","source":{"kind":"weakening","event":"w1"}});
        assert!(apply(&mut state, &event(2, "human:local", "D-2", accept.clone())).unwrap());
        let own = json!({"text":"扩展自报的也接受","source":{"kind":"weakening","event":"w3"}});
        assert!(apply(&mut state, &event(3, "human:local", "D-3", own)).unwrap());
        let found = read(&state).unwrap();
        assert_eq!((found["D-1"].question.as_str(), found["D-1"].target.as_str()), ("用哪种格式？", "T-1"));
        assert_eq!(found["D-2"].question, "core.weakening 标题");
        assert_eq!(accepted(&state), BTreeSet::from(["w1".to_owned(), "w3".to_owned()]));
        assert_eq!(apply(&mut state, &event(4, "human:local", "D-4", reply)).unwrap_err(), "这条来源已记为决策");
        assert_eq!(apply(&mut state, &event(4, "human:local", "D-1", accept)).unwrap_err(), "决策已存在：D-1");
    }

    #[test]
    fn only_a_person_decides_on_a_real_source() {
        let mut state = state();
        let before = state.clone();
        let body = |source: Value| json!({"text":"定了","source":source});
        for actor in ["process:towermd-cli", "claude-code/2.1", "process:towermd"] {
            assert!(apply(&mut state, &event(1, actor, "D-1", body(json!({"kind":"reply","request":"Q-1"})))).unwrap_err().contains("只能由人"));
        }
        for (source, error) in [
            (json!({"kind":"reply","request":"Q-2"}), "要记为决策的回答不存在"),
            (json!({"kind":"reply","request":"Q-3"}), "要记为决策的回答不存在"),
            (json!({"kind":"reply","request":"Q-9"}), "要记为决策的回答不存在"),
            (json!({"kind":"weakening","event":"w2"}), "要接受的承诺弱化不存在"),
            (json!({"kind":"weakening","event":"w4"}), "要接受的承诺弱化不存在"),
        ] {
            assert_eq!(apply(&mut state, &event(1, "human:local", "D-1", body(source.clone()))).unwrap_err(), error, "{source}");
        }
        for data in [json!({"text":"","source":{"kind":"reply","request":"Q-1"}}), json!({"text":" 定了","source":{"kind":"reply","request":"Q-1"}}),
            json!({"text":"x".repeat(TEXT_CHARS + 1),"source":{"kind":"reply","request":"Q-1"}}),
            json!({"text":"定了","source":{"kind":"reply","request":"Q-1"},"review_on":"next week"}),
            json!({"text":"定了","source":{"kind":"reply","request":"Q-1"},"x":1}),
            json!({"text":"定了","source":{"kind":"acceptance","event":"e"}})] {
            assert!(apply(&mut state, &event(1, "human:local", "D-1", data.clone())).is_err(), "{data}");
        }
        let mut wrong = event(1, "human:local", "D-1", body(json!({"kind":"reply","request":"Q-1"})));
        wrong.layer = "L4".into();
        assert!(apply(&mut state, &wrong).is_err());
        let mut bad_id = event(1, "human:local", "../D", body(json!({"kind":"reply","request":"Q-1"})));
        bad_id.layer = LAYER.into();
        assert!(apply(&mut state, &bad_id).is_err());
        assert_eq!(state, before);
    }
}
