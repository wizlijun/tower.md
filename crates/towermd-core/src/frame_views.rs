//! FR-159: views derived from time. A template names the views it offers
//! (`views` in its manifest); the core computes each one from the canonical
//! events on every read. Nothing here is stored or written, so there is
//! nothing to rebuild: the events are the only input.
//!
//! The one view so far is the cumulative flow: for each day, how many cards
//! sat in each of the frame's states at the end of that day.
use std::collections::BTreeMap;

use chrono::{DateTime, Days, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::{
    cards,
    frames::{self, Frame},
    hub::Event,
    kit::Kit,
    project_runtime::{self, Reducer},
};

pub const CUMULATIVE_FLOW: &str = "cumulative_flow";
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
    if declared.iter().any(|name| name == CUMULATIVE_FLOW) && !frame.states.is_empty() {
        out.push(CUMULATIVE_FLOW);
    }
    out
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

#[cfg(test)]
#[path = "frame_views_tests.rs"]
mod tests;
