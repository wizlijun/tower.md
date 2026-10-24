//! FR-189: starter kits, a set of recommended templates, checks and display
//! names for nouns. Kits are first-party manifests shipped outside the core;
//! the core only records a person's choice. A kit is recommended once, after
//! the project has its first content; a person adopts or dismisses it in the
//! App, and either answer ends the recommendation. Kits never change a core
//! rule: adopting one only records a preference the App may show.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{delegation_projection::Mark, frames, hub::Event, question_model::{human_actor, valid_id}};

pub const GROUP: &str = "Starter";
pub const LAYER: &str = "L3";
pub const ADOPTED: &str = "starter.adopted";
pub const DISMISSED: &str = "starter.dismissed";

/// One kit's manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kit {
    pub id: String,
    pub title: String,
    /// Template ids worth placing first.
    #[serde(default)]
    pub templates: Vec<String>,
    /// Checks the kit suggests, in words.
    #[serde(default)]
    pub checks: Vec<String>,
    /// Core noun → the name this kit shows for it, e.g. `Task: 章节`.
    #[serde(default)]
    pub names: BTreeMap<String, String>,
}

impl Kit {
    pub fn parse(text: &str) -> Result<Kit, String> {
        let kit: Kit = serde_yaml::from_str(text).map_err(|e| format!("起步套装清单无效：{e}"))?;
        valid_id(&kit.id)?;
        if kit.title.trim().is_empty() || kit.names.values().any(|name| name.trim().is_empty()) {
            return Err(format!("起步套装 {} 缺少标题或显示名称", kit.id));
        }
        Ok(kit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    Adopted,
    Dismissed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chosen {
    pub choice: Choice,
    pub at: Mark,
}

pub fn read(state: &Value) -> Result<BTreeMap<String, Chosen>, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}

/// Only a person, from the App, adopts or dismisses a kit.
pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    let choice = match event.kind.as_str() {
        ADOPTED => Choice::Adopted,
        DISMISSED => Choice::Dismissed,
        _ => return Ok(false),
    };
    if event.layer != LAYER || !event.guards.is_empty() || !human_actor(&event.actor) {
        return Err("起步套装只由人在 App 中采用或忽略".into());
    }
    valid_id(&event.obj)?;
    let mut chosen = read(state)?;
    chosen.insert(event.obj.clone(), Chosen { choice, at: frames::mark(event) });
    state[GROUP] = serde_json::to_value(chosen).map_err(|e| e.to_string())?;
    Ok(true)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct View {
    /// Kits nobody answered yet; empty until the project has content.
    pub recommended: Vec<Kit>,
    pub adopted: Vec<Kit>,
    /// Display names from the adopted kits; the first adopted kit wins.
    pub names: BTreeMap<String, String>,
}

pub fn view(state: &Value, kits: &[Kit], has_content: bool) -> Result<View, String> {
    let chosen = read(state)?;
    let recommended = if has_content { kits.iter().filter(|kit| !chosen.contains_key(&kit.id)).cloned().collect() } else { Vec::new() };
    let mut adopted: Vec<Kit> = kits.iter().filter(|kit| chosen.get(&kit.id).is_some_and(|c| c.choice == Choice::Adopted)).cloned().collect();
    adopted.sort_by_key(|kit| chosen[&kit.id].at.seq);
    let mut names = BTreeMap::new();
    for kit in &adopted {
        for (noun, name) in &kit.names {
            names.entry(noun.clone()).or_insert_with(|| name.clone());
        }
    }
    Ok(View { recommended, adopted, names })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(seq: u64, kind: &str, actor: &str, obj: &str) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":"2026-10-24T00:00:00.000Z",
            "actor":actor,"type":kind,"layer":LAYER,"obj":obj,"data":{},"guards":[],"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn kits() -> Vec<Kit> {
        vec![
            Kit::parse("id: research\ntitle: 长期研究\ntemplates: [kanban]\nnames:\n  Task: 阅读\n").unwrap(),
            Kit::parse("id: writing\ntitle: 写作\nnames:\n  Task: 章节\n").unwrap(),
        ]
    }

    #[test]
    fn a_kit_is_recommended_once_after_the_first_content() {
        let mut state = json!({});
        assert!(view(&state, &kits(), false).unwrap().recommended.is_empty());
        assert_eq!(view(&state, &kits(), true).unwrap().recommended.len(), 2);
        assert!(apply(&mut state, &event(1, DISMISSED, "human:local", "research")).unwrap());
        let shown = view(&state, &kits(), true).unwrap();
        assert_eq!(shown.recommended.iter().map(|kit| kit.id.as_str()).collect::<Vec<_>>(), ["writing"]);
        apply(&mut state, &event(2, ADOPTED, "human:local", "writing")).unwrap();
        let shown = view(&state, &kits(), true).unwrap();
        assert!(shown.recommended.is_empty());
        assert_eq!(shown.names["Task"], "章节");
        apply(&mut state, &event(3, ADOPTED, "human:local", "research")).unwrap();
        assert_eq!(view(&state, &kits(), true).unwrap().names["Task"], "章节", "kits mix; the first adopted wins");
    }

    #[test]
    fn only_a_person_chooses_a_kit() {
        let mut state = json!({});
        for actor in ["process:towermd-cli", "process:towermd", "claude-code/2.0"] {
            assert!(apply(&mut state, &event(1, ADOPTED, actor, "research")).is_err(), "{actor}");
        }
        assert!(state.get(GROUP).is_none());
        assert!(!apply(&mut state, &event(1, "frame.created", "human:local", "research")).unwrap());
        assert!(Kit::parse("id: x\ntitle: X\nrules: {}\n").is_err(), "a kit has no rules of its own");
    }
}
