//! Ideas and commitments are kept apart (core spec FR-181–183). An idea is a
//! one-sentence lead hanging on the canvas: it is not in the attention list,
//! nothing can claim it and no delegate receives it. Only a person turns it
//! into a commitment (`idea.promoted`, signed `human:<id>`, sent from the App
//! only). Promoting as `project` makes the sentence the project's goal, once
//! per project; any other kind belongs to a kit (`Kit::promote`), which must
//! create its own object in the same event. The `Idea` group is created on
//! first use, so older projections stay byte-identical.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation_projection::Mark,
    hub::Event,
    kit::Kit,
    question_model::{human_actor, valid_id},
};

pub const GROUP: &str = "Idea";
/// The registered object type (`crate::types::TYPES`).
pub const TYPE: &str = "Idea";
/// An idea is an unpromoted lead; it lives in the commitment layer's
/// waiting room and protects nothing.
pub const LAYER: &str = "L4";
pub const ADDED: &str = "idea.added";
pub const PROMOTED: &str = "idea.promoted";
/// One sentence: at most this many characters.
pub const TEXT_CHARS: usize = 200;
/// The core's own promotion kind: the sentence becomes the project's goal.
pub const AS_PROJECT: &str = "project";

/// `idea.added`. Anyone may jot an idea; the daemon decides the signer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Added {
    pub text: String,
}

/// `idea.promoted`. `target` and `object` are the kit's new commitment;
/// promoting as `project` carries neither.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Promote {
    #[serde(rename = "as")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Promotion {
    #[serde(rename = "as")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub event: Mark,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Idea {
    pub id: String,
    pub text: String,
    pub added: Mark,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promoted: Option<Promotion>,
}

/// Ideas by ID. IDs are UUIDv7 request IDs, so the order is arrival order.
pub type Ideas = BTreeMap<String, Idea>;

pub fn read(state: &Value) -> Result<Ideas, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(Ideas::new()),
    }
}

/// The idea a person promoted as the project's goal, if any.
pub fn goal(ideas: &Ideas) -> Option<&Idea> {
    ideas.values().find(|idea| idea.promoted.as_ref().is_some_and(|p| p.kind == AS_PROJECT))
}

pub fn check_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > TEXT_CHARS || text.chars().any(char::is_control) {
        return Err(format!("想法须是一句话：非空、不换行、不超过 {TEXT_CHARS} 字"));
    }
    Ok(())
}

fn mark(event: &Event) -> Mark {
    Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() }
}

pub fn apply(state: &mut Value, event: &Event, kits: &[&dyn Kit]) -> Result<bool, String> {
    let added = match event.kind.as_str() {
        ADDED => true,
        PROMOTED => false,
        _ => return Ok(false),
    };
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err(format!("{} 的层或 fences 无效", event.kind));
    }
    valid_id(&event.obj)?;
    let mut ideas = read(state)?;
    if added {
        let body: Added = serde_json::from_value(event.data.clone()).map_err(|e| format!("想法正文无效：{e}"))?;
        check_text(&body.text)?;
        if ideas.contains_key(&event.obj) {
            return Err("想法ID重复".into());
        }
        ideas.insert(event.obj.clone(), Idea { id: event.obj.clone(), text: body.text, added: mark(event), promoted: None });
    } else {
        if !human_actor(&event.actor) {
            return Err("只有人能立项".into());
        }
        let request: Promote = serde_json::from_value(event.data.clone()).map_err(|e| format!("立项正文无效：{e}"))?;
        let idea = ideas.get(&event.obj).ok_or("立项的想法不存在")?;
        if idea.promoted.is_some() {
            return Err("这个想法已经立项".into());
        }
        if request.kind == AS_PROJECT {
            if request.target.is_some() || request.object.is_some() {
                return Err("立为项目不带其他对象".into());
            }
            if goal(&ideas).is_some() {
                return Err("项目已有目标".into());
            }
        } else {
            let target = request.target.as_deref().ok_or("立项缺少目标对象")?;
            valid_id(target)?;
            let mut handled = false;
            for kit in kits {
                if kit.promote(state, event, &request)? {
                    handled = true;
                    break;
                }
            }
            if !handled {
                return Err(format!("不支持的立项类别：{}", request.kind));
            }
        }
        let idea = ideas.get_mut(&event.obj).ok_or("立项的想法不存在")?;
        idea.promoted = Some(Promotion { kind: request.kind, target: request.target, event: mark(event) });
    }
    state[GROUP] = serde_json::to_value(&ideas).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "0192c2d0-0000-7000-8000-000000000001";

    fn event(seq: u64, kind: &str, actor: &str, obj: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":"2026-10-23T01:00:00Z","actor":actor,
            "type":kind,"layer":"L4","obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn added(state: &mut Value) {
        assert!(apply(state, &event(1, ADDED, "process:towermd-cli", ID, json!({"text":"给周报加导出"})), &[]).unwrap());
    }

    struct Tickets;
    impl Kit for Tickets {
        fn promote(&self, state: &mut Value, event: &Event, request: &Promote) -> Result<bool, String> {
            if request.kind != "ticket" { return Ok(false) }
            if !human_actor(&event.actor) { return Err("kit 也要求人".into()) }
            state["Ticket"][request.target.as_deref().unwrap()] = request.object.clone().unwrap_or_default();
            Ok(true)
        }
    }

    #[test]
    fn other_events_are_not_ideas_and_an_untouched_projection_has_no_group() {
        let mut state = json!({});
        assert!(!apply(&mut state, &event(1, "signal.dismissed", "human:local", ID, json!({})), &[]).unwrap());
        assert_eq!(state, json!({}));
        assert!(read(&state).unwrap().is_empty());
    }

    #[test]
    fn an_idea_is_recorded_once_with_its_signer() {
        let mut state = json!({});
        added(&mut state);
        let ideas = read(&state).unwrap();
        assert_eq!(ideas[ID].text, "给周报加导出");
        assert_eq!(ideas[ID].added.actor, "process:towermd-cli");
        assert!(ideas[ID].promoted.is_none());
        let again = event(2, ADDED, "human:local", ID, json!({"text":"再来一次"}));
        assert_eq!(apply(&mut state, &again, &[]).unwrap_err(), "想法ID重复");
    }

    #[test]
    fn a_malformed_idea_is_rejected() {
        let too_long = "想".repeat(TEXT_CHARS + 1);
        for (layer, obj, data) in [
            ("L4", ID, json!({"text":""})),
            ("L4", ID, json!({"text":"  "})),
            ("L4", ID, json!({"text":"两行\n想法"})),
            ("L4", ID, json!({"text":too_long})),
            ("L4", ID, json!({"text":"x","actor":"human:forged"})),
            ("L4", "../x", json!({"text":"x"})),
            ("L1", ID, json!({"text":"x"})),
        ] {
            let mut bad = event(1, ADDED, "process:towermd-cli", obj, data.clone());
            bad.layer = layer.into();
            assert!(apply(&mut json!({}), &bad, &[]).is_err(), "{layer} {obj} {data}");
        }
        assert!(check_text(&"想".repeat(TEXT_CHARS)).is_ok());
    }

    #[test]
    fn only_a_person_promotes_and_only_once() {
        let mut state = json!({});
        added(&mut state);
        for actor in ["process:towermd-cli", "process:towermd", "claude-code/2.1", "human:"] {
            let forged = event(2, PROMOTED, actor, ID, json!({"as":"project"}));
            assert_eq!(apply(&mut state.clone(), &forged, &[]).unwrap_err(), "只有人能立项", "{actor}");
        }
        let missing = event(2, PROMOTED, "human:local", "0192c2d0-0000-7000-8000-000000000009", json!({"as":"project"}));
        assert_eq!(apply(&mut state.clone(), &missing, &[]).unwrap_err(), "立项的想法不存在");
        let promote = event(2, PROMOTED, "human:local", ID, json!({"as":"project"}));
        assert!(apply(&mut state, &promote, &[]).unwrap());
        let ideas = read(&state).unwrap();
        assert_eq!(goal(&ideas).unwrap().id, ID);
        assert_eq!(ideas[ID].promoted.as_ref().unwrap().event.actor, "human:local");
        let twice = event(3, PROMOTED, "human:local", ID, json!({"as":"project"}));
        assert_eq!(apply(&mut state, &twice, &[]).unwrap_err(), "这个想法已经立项");
    }

    #[test]
    fn a_project_has_one_goal_and_promoting_carries_no_foreign_object() {
        let mut state = json!({});
        added(&mut state);
        let other = "0192c2d0-0000-7000-8000-000000000002";
        assert!(apply(&mut state, &event(2, ADDED, "human:local", other, json!({"text":"第二个"})), &[]).unwrap());
        let smuggled = event(3, PROMOTED, "human:local", ID, json!({"as":"project","target":"X"}));
        assert!(apply(&mut state.clone(), &smuggled, &[]).is_err());
        assert!(apply(&mut state, &event(3, PROMOTED, "human:local", ID, json!({"as":"project"})), &[]).unwrap());
        let second = event(4, PROMOTED, "human:local", other, json!({"as":"project"}));
        assert_eq!(apply(&mut state, &second, &[]).unwrap_err(), "项目已有目标");
    }

    #[test]
    fn other_kinds_belong_to_a_kit_and_need_a_target() {
        let mut state = json!({});
        added(&mut state);
        let unknown = event(2, PROMOTED, "human:local", ID, json!({"as":"ticket","target":"K-1","object":{"n":1}}));
        assert_eq!(apply(&mut state.clone(), &unknown, &[]).unwrap_err(), "不支持的立项类别：ticket");
        let untargeted = event(2, PROMOTED, "human:local", ID, json!({"as":"ticket"}));
        assert_eq!(apply(&mut state.clone(), &untargeted, &[&Tickets]).unwrap_err(), "立项缺少目标对象");
        assert!(apply(&mut state, &unknown, &[&Tickets]).unwrap());
        assert_eq!(state["Ticket"]["K-1"], json!({"n":1}));
        let promoted = read(&state).unwrap()[ID].promoted.clone().unwrap();
        assert_eq!((promoted.kind.as_str(), promoted.target.as_deref()), ("ticket", Some("K-1")));
        assert!(goal(&read(&state).unwrap()).is_none());
    }
}
