//! FR-157/158: a frame's history, replayed read-only from the canonical
//! events. A step is any accepted event after which the frame reads
//! differently; each step says which cards became new, changed state or
//! changed text. The view at a step is the ordinary frame view over the
//! state replayed up to that step. Nothing here is stored or written.
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::{cards::Card, frames, hub::Event, project_runtime::{self, Reducer}};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    New,
    State,
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Step {
    pub seq: u64,
    pub at: String,
    pub kind: String,
    pub actor: String,
    /// Cards this step added or changed, and how.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub cards: BTreeMap<String, Vec<Change>>,
}

fn sorted<'a>(events: impl IntoIterator<Item = &'a Event>) -> Vec<Event> {
    let mut events: Vec<Event> = events.into_iter().cloned().collect();
    events.sort_by_key(|event| event.seq);
    events
}

fn cards(frame: Option<&Value>) -> BTreeMap<String, Card> {
    frame.and_then(|frame| frame.get("cards")).cloned()
        .and_then(|cards| serde_json::from_value::<Vec<Card>>(cards).ok())
        .unwrap_or_default().into_iter().filter(|card| !card.archived)
        .map(|card| (card.id.clone(), card)).collect()
}

/// How each card differs between two readings of the frame.
pub fn changes(before: Option<&Value>, after: Option<&Value>) -> BTreeMap<String, Vec<Change>> {
    let (old, new) = (cards(before), cards(after));
    let mut out = BTreeMap::new();
    for (id, card) in &new {
        let mut found = Vec::new();
        match old.get(id) {
            None => found.push(Change::New),
            Some(was) => {
                if was.state != card.state {
                    found.push(Change::State);
                }
                if (&was.title, &was.body, &was.fields, &was.links, &was.parent) != (&card.title, &card.body, &card.fields, &card.links, &card.parent) {
                    found.push(Change::Text);
                }
            }
        }
        if !found.is_empty() {
            out.insert(id.clone(), found);
        }
    }
    out
}

/// Every step of `frame`, oldest first, replaying `events` from `initial`.
pub fn steps<'a>(events: impl IntoIterator<Item = &'a Event>, frame: &str, initial: &Value, reduce: Reducer) -> Result<Vec<Step>, String> {
    let mut state = initial.clone();
    let mut before: Option<Value> = None;
    let mut out = Vec::new();
    for event in sorted(events) {
        state = project_runtime::replay(&state, std::slice::from_ref(&event), reduce)?;
        let after = state.get(frames::GROUP).and_then(|group| group.get(frame)).cloned();
        if after != before {
            out.push(Step { seq: event.seq, at: event.t.clone(), kind: event.kind.clone(), actor: event.actor.clone(),
                cards: changes(before.as_ref(), after.as_ref()) });
            before = after;
        }
    }
    Ok(out)
}

/// The whole state as it was right after `seq`.
pub fn state_at<'a>(events: impl IntoIterator<Item = &'a Event>, seq: u64, initial: &Value, reduce: Reducer) -> Result<Value, String> {
    let events: Vec<Event> = sorted(events).into_iter().filter(|event| event.seq <= seq).collect();
    project_runtime::replay(initial, &events, reduce)
}
