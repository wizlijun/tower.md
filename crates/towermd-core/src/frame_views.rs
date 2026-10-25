//! FR-159: views derived from time. A template names the views it offers
//! (`views` in its manifest); the core computes each one from the canonical
//! events on every read. Nothing here is stored or written, so there is
//! nothing to rebuild: the events are the only input.
//!
//! Three views so far, all over a frame's own cards in the events:
//! - the cumulative flow: for each day, how many cards sat in each of the
//!   frame's states at the end of that day;
//! - the plan deviation: on a time axis, each card's first planned date, how
//!   that date moved, and the day it actually reached a done state;
//! - the evolution: when each card was put forward, renamed, moved, linked,
//!   changed state or was archived, oldest first.
//!
//! The last two read free cards only: a bound card follows a core object,
//! whose history is that object's own.
use std::collections::BTreeMap;

use chrono::{DateTime, Days, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::{
    cards::{self, Card},
    frames::{self, Frame},
    hub::Event,
    kit::Kit,
    project_runtime::{self, Reducer},
};

pub const CUMULATIVE_FLOW: &str = "cumulative_flow";
pub const PLAN_DEVIATION: &str = "plan_deviation";
pub const EVOLUTION: &str = "argument_evolution";
/// The evolution keeps the latest steps; older ones are counted only.
pub const STEPS_MAX: usize = 1000;
/// The flow covers at most the last year, today included.
pub const DAYS_MAX: u64 = 366;

/// One day: the count in each state, in the order of `Flow::states`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Day {
    pub date: String,
    pub counts: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Flow {
    pub frame: String,
    pub states: Vec<String>,
    pub done_states: Vec<String>,
    /// Every day from the frame's first day (or a year ago) to today, oldest
    /// first; a day without events carries the day before.
    pub days: Vec<Day>,
}

/// The views `frame` offers: the names its frozen template declares that
/// this build knows and that fit the frame. Unknown names are ignored, so a
/// template written for a later build still reads.
pub fn offered(frame: &Frame) -> Vec<&'static str> {
    let declared = frames::Template::parse(&frame.template.definition).map(|template| template.views).unwrap_or_default();
    let mut out = Vec::new();
    // A text-source frame's cards live in its file, not in the events.
    if frame.source.is_some() {
        return out;
    }
    let declares = |name: &str| declared.iter().any(|declared| declared == name);
    if declares(CUMULATIVE_FLOW) && !frame.states.is_empty() {
        out.push(CUMULATIVE_FLOW);
    }
    if declares(PLAN_DEVIATION) && time_field(frame).is_some() && !frame.done_states.is_empty() {
        out.push(PLAN_DEVIATION);
    }
    if declares(EVOLUTION) {
        out.push(EVOLUTION);
    }
    out
}

/// The date field a frame lays out on a time axis.
fn time_field(frame: &Frame) -> Option<&str> {
    [&frame.layout.x, &frame.layout.y].into_iter().find_map(|axis| match axis {
        frames::Axis::Time { field } => Some(field.as_str()),
        _ => None,
    })
}

fn date_of(t: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(t).ok().map(|at| at.with_timezone(&Utc).date_naive())
}

fn text(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// How many live cards, free and bound, sit in each state; `None` while the
/// frame does not exist.
fn tally(state: &Value, frame: &str, kits: &[&dyn Kit], date: NaiveDate) -> Result<Option<(Frame, BTreeMap<String, u64>)>, String> {
    if state.get(frames::GROUP).and_then(|group| group.get(frame)).is_none() {
        return Ok(None);
    }
    let Some(view) = cards::view(state, kits, &text(date))?.into_iter().find(|view| view.frame.id == frame) else {
        return Ok(None);
    };
    let mut counts = BTreeMap::new();
    let free = view.frame.cards.iter().filter(|card| !card.archived).filter_map(|card| card.state.clone());
    for name in free.chain(view.bound.iter().map(|card| card.state.clone())) {
        *counts.entry(name).or_insert(0) += 1;
    }
    Ok(Some((view.frame, counts)))
}

/// The cumulative flow of `frame`, replaying `events` from `initial` once.
/// A day closes when the first event of a later UTC day arrives; `today`
/// (`YYYY-MM-DD`) closes the last one. `None` when the frame does not exist
/// at the end.
pub fn cumulative_flow<'a>(events: impl IntoIterator<Item = &'a Event>, frame: &str, initial: &Value, reduce: Reducer,
    kits: &[&dyn Kit], today: &str) -> Result<Option<Flow>, String> {
    let today = NaiveDate::parse_from_str(today, "%Y-%m-%d").map_err(|error| format!("today 须是 YYYY-MM-DD：{error}"))?;
    let mut events: Vec<Event> = events.into_iter().cloned().collect();
    events.sort_by_key(|event| event.seq);
    let mut state = initial.clone();
    let mut tallies: BTreeMap<NaiveDate, BTreeMap<String, u64>> = BTreeMap::new();
    let mut day: Option<NaiveDate> = None;
    for event in &events {
        // A timestamp that does not parse, or runs behind, stays in the open day.
        if let Some(date) = date_of(&event.t).filter(|date| day.map_or(true, |open| *date > open)) {
            if let Some(open) = day {
                if let Some((_, counts)) = tally(&state, frame, kits, open)? {
                    tallies.insert(open, counts);
                }
            }
            day = Some(date);
        }
        state = project_runtime::replay(&state, std::slice::from_ref(event), reduce)?;
    }
    let Some(last) = day else { return Ok(None) };
    let end = today.max(last);
    let Some((current, counts)) = tally(&state, frame, kits, end)? else { return Ok(None) };
    tallies.insert(last, counts);
    let Some(first) = tallies.keys().next().copied() else { return Ok(None) };
    let start = end.checked_sub_days(Days::new(DAYS_MAX - 1)).map_or(first, |earliest| first.max(earliest));
    let mut days = Vec::new();
    let mut date = start;
    while date <= end {
        let counts = tallies.range(..=date).next_back().map(|(_, counts)| counts);
        days.push(Day { date: text(date), counts: current.states.iter()
            .map(|name| counts.and_then(|counts| counts.get(name)).copied().unwrap_or(0)).collect() });
        date = date.checked_add_days(Days::new(1)).ok_or("日期越界")?;
    }
    Ok(Some(Flow { frame: frame.to_owned(), states: current.states.clone(), done_states: current.done_states.clone(), days }))
}

type Cards = BTreeMap<String, Card>;

fn cards_of<'a>(state: &'a Value, frame: &str) -> Option<&'a Value> {
    state.get(frames::GROUP)?.get(frame).map(|frame| frame.get("cards").unwrap_or(&Value::Null))
}

/// Replays `events` from `initial` once, calling `step` with each event
/// after which the frame's free cards differ, the frame as stored then, and
/// the cards before and after. The frame as it ends, or `None` when it does
/// not exist then.
fn card_steps<'a>(events: impl IntoIterator<Item = &'a Event>, frame: &str, initial: &Value, reduce: Reducer,
    mut step: impl FnMut(&Event, &Value, &Cards, &Cards)) -> Result<Option<Frame>, String> {
    let mut events: Vec<Event> = events.into_iter().cloned().collect();
    events.sort_by_key(|event| event.seq);
    let mut state = initial.clone();
    let parse = |value: Option<&Value>| -> Result<Cards, String> {
        let list: Vec<Card> = match value {
            Some(value) if !value.is_null() => serde_json::from_value(value.clone()).map_err(|e| e.to_string())?,
            _ => Vec::new(),
        };
        Ok(list.into_iter().map(|card| (card.id.clone(), card)).collect())
    };
    let mut raw = cards_of(&state, frame).cloned();
    let mut before = parse(raw.as_ref())?;
    for event in &events {
        state = project_runtime::replay(&state, std::slice::from_ref(event), reduce)?;
        let now = cards_of(&state, frame);
        if now != raw.as_ref() {
            let after = parse(now)?;
            step(event, &state[frames::GROUP][frame], &before, &after);
            raw = now.cloned();
            before = after;
        }
    }
    Ok(frames::read(&state)?.remove(frame))
}

fn day_of(event: &Event) -> String {
    date_of(&event.t).map(text).unwrap_or_else(|| event.t.chars().take(10).collect())
}

/// Who made a change: the person, or the delegate a delegate write names.
fn author(card: &Card) -> String {
    if card.changed.actor.starts_with("human:") {
        return card.changed.actor.clone();
    }
    card.by.clone().unwrap_or_else(|| card.changed.actor.clone())
}

/// One move of a card's planned date.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Replan {
    /// The UTC day of the change.
    pub date: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// One card on the time axis, planned against actual.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Planned {
    pub card: String,
    pub title: String,
    /// The first date the card was given.
    pub planned: Option<String>,
    /// The date it has now.
    pub plan: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replans: Vec<Replan>,
    /// The UTC day it last entered a done state, while it is done.
    pub done: Option<String>,
    /// `plan` minus `planned`, in days.
    pub slip_days: Option<i64>,
    /// `done` minus `planned`, in days, once done.
    pub deviation_days: Option<i64>,
    /// Today minus `plan` for an open card past its date.
    pub overdue_days: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deviation {
    pub frame: String,
    pub field: String,
    pub done_states: Vec<String>,
    pub today: String,
    /// Live cards, by their current date, undated last.
    pub cards: Vec<Planned>,
}

fn date_field(card: &Card, field: &str) -> Option<String> {
    card.fields.get(field).and_then(Value::as_str).filter(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()).map(str::to_owned)
}

fn days_between(from: Option<&str>, to: Option<&str>) -> Option<i64> {
    let parse = |value: Option<&str>| value.and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok());
    Some((parse(to)? - parse(from)?).num_days())
}

/// FR-159: planned against actual on a frame's time axis. `None` when the
/// frame does not exist at the end or has no time axis.
pub fn plan_deviation<'a>(events: impl IntoIterator<Item = &'a Event>, frame: &str, initial: &Value, reduce: Reducer,
    today: &str) -> Result<Option<Deviation>, String> {
    let today = NaiveDate::parse_from_str(today, "%Y-%m-%d").map_err(|error| format!("today 须是 YYYY-MM-DD：{error}"))?;
    // The field and the done states come from the frozen template, which
    // never changes once the frame exists.
    let mut meta: Option<(String, Vec<String>)> = None;
    let mut first: BTreeMap<String, String> = BTreeMap::new();
    let mut replans: BTreeMap<String, Vec<Replan>> = BTreeMap::new();
    let mut done_since: BTreeMap<String, String> = BTreeMap::new();
    let done = |card: Option<&Card>, states: &[String]| card.and_then(|card| card.state.as_ref()).is_some_and(|state| states.contains(state));
    let end = card_steps(events, frame, initial, reduce, |event, stored, before, after| {
        let (field, done_states) = meta.get_or_insert_with(|| {
            let stored: Option<Frame> = serde_json::from_value(stored.clone()).ok();
            stored.map(|frame| (time_field(&frame).unwrap_or_default().to_owned(), frame.done_states)).unwrap_or_default()
        });
        let (field, done_states) = (field.as_str(), done_states.as_slice());
        for (id, card) in after {
            let old = before.get(id);
            let (was, now) = (old.and_then(|old| date_field(old, field)), date_field(card, field));
            if let Some(date) = &now {
                first.entry(id.clone()).or_insert_with(|| date.clone());
            }
            if old.is_some() && was != now {
                replans.entry(id.clone()).or_default().push(Replan { date: day_of(event), from: was, to: now });
            }
            match (done(old, done_states), done(Some(card), done_states)) {
                (false, true) => {
                    done_since.insert(id.clone(), day_of(event));
                }
                (true, false) => {
                    done_since.remove(id);
                }
                _ => {}
            }
        }
    })?;
    let Some(end) = end else { return Ok(None) };
    let Some(field) = time_field(&end).map(str::to_owned) else { return Ok(None) };
    let done = |card: &Card| done(Some(card), &end.done_states);
    let mut cards: Vec<Planned> = end.cards.iter().filter(|card| !card.archived).map(|card| {
        let planned = first.get(&card.id).cloned();
        let plan = date_field(card, &field);
        let finished = done(card).then(|| done_since.get(&card.id).cloned()).flatten();
        let overdue = (!done(card)).then(|| days_between(plan.as_deref(), Some(&text(today)))).flatten().filter(|days| *days > 0);
        Planned {
            card: card.id.clone(), title: card.title.clone(),
            slip_days: days_between(planned.as_deref(), plan.as_deref()),
            deviation_days: days_between(planned.as_deref(), finished.as_deref()),
            overdue_days: overdue,
            replans: replans.remove(&card.id).unwrap_or_default(),
            planned, plan, done: finished,
        }
    }).collect();
    cards.sort_by(|a, b| (a.plan.is_none(), &a.plan, &a.card).cmp(&(b.plan.is_none(), &b.plan, &b.card)));
    Ok(Some(Deviation { frame: frame.to_owned(), field, done_states: end.done_states.clone(), today: text(today), cards }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// The card appeared.
    Proposed,
    Renamed,
    /// Its state changed; a done state is where a claim stands.
    State,
    /// It moved under another card, or to the top.
    Moved,
    Linked,
    Unlinked,
    Archived,
    Restored,
}

/// One change of one card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Step {
    pub at: String,
    /// The UTC day of `at`.
    pub date: String,
    pub card: String,
    pub title: String,
    pub change: Change,
    /// The state after the change, for `proposed` and `state`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// The parent after `moved`, or the other card of `linked`/`unlinked`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    pub by: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Evolution {
    pub frame: String,
    pub states: Vec<String>,
    pub done_states: Vec<String>,
    /// Oldest first; at most `STEPS_MAX`, the latest kept.
    pub steps: Vec<Step>,
    /// How many older steps were left out.
    pub omitted: usize,
}

/// FR-159: how the frame's cards were put forward and changed, from the
/// events. `None` when the frame does not exist at the end.
pub fn evolution<'a>(events: impl IntoIterator<Item = &'a Event>, frame: &str, initial: &Value, reduce: Reducer) -> Result<Option<Evolution>, String> {
    let mut steps = Vec::new();
    let end = card_steps(events, frame, initial, reduce, |event, _, before, after| {
        let mut push = |card: &Card, change: Change, state: Option<String>, other: Option<String>| steps.push(Step {
            at: event.t.clone(), date: day_of(event), card: card.id.clone(), title: card.title.clone(), change, state, other, by: author(card),
        });
        for (id, card) in after {
            let Some(old) = before.get(id) else {
                push(card, Change::Proposed, card.state.clone(), card.parent.clone());
                continue;
            };
            if old.title != card.title {
                push(card, Change::Renamed, None, None);
            }
            if old.state != card.state {
                push(card, Change::State, card.state.clone(), None);
            }
            if old.parent != card.parent {
                push(card, Change::Moved, None, card.parent.clone());
            }
            for to in card.links.iter().filter(|to| !old.links.contains(to)) {
                push(card, Change::Linked, None, Some(to.clone()));
            }
            for to in old.links.iter().filter(|to| !card.links.contains(to)) {
                push(card, Change::Unlinked, None, Some(to.clone()));
            }
            if old.archived != card.archived {
                push(card, if card.archived { Change::Archived } else { Change::Restored }, None, None);
            }
        }
    })?;
    let Some(end) = end else { return Ok(None) };
    let omitted = steps.len().saturating_sub(STEPS_MAX);
    steps.drain(..omitted);
    Ok(Some(Evolution { frame: frame.to_owned(), states: end.states.clone(), done_states: end.done_states.clone(), steps, omitted }))
}

#[cfg(test)]
#[path = "frame_views_tests.rs"]
mod tests;
