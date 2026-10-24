//! Quiet objects and resurfacing (core spec FR-135, FR-136), derived on every
//! read from the event history and never stored. An object with no event for
//! longer than `dormant_after` days is quiet: the App fades it and it takes no
//! first-screen slot. A quiet object resurfaces when one of its follow-ups is
//! due, when its `check_every` rhythm runs out, or when a connector reports
//! on it after a quiet gap; a person's own event on it (any, or the App's
//! `resurfaced.seen`) settles that. Machine bookkeeping such as leases and
//! snapshots is not activity. Settings are read leniently from `quiet:` in
//! `.towermd/config.yaml`.
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_yaml::Value as Yaml;

use crate::{
    delegation_projection::Mark,
    follow_ups::{self, Facts, FollowUps},
    hub::Event,
    question_model::human_actor,
    tower_api::{Merged, Signal},
};

/// FR-135 default, in days.
pub const DORMANT_AFTER: u32 = 14;
/// Longest accepted rhythm, in days; anything above is ignored as a typo.
pub const MAX_DAYS: u64 = 3650;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Settings {
    /// Days without any event after which an object is quiet.
    pub dormant_after: u32,
    /// Per object ID: days after its last event at which it resurfaces.
    pub check_every: BTreeMap<String, u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { dormant_after: DORMANT_AFTER, check_every: BTreeMap::new() }
    }
}

fn days(value: &Yaml) -> Option<u32> {
    value.as_u64().filter(|days| (1..=MAX_DAYS).contains(days)).map(|days| days as u32)
}

/// A missing, non-mapping or malformed section keeps the defaults; an
/// unreadable rhythm is skipped rather than making anything quiet sooner.
pub fn settings(section: Option<&Yaml>) -> Settings {
    let defaults = Settings::default();
    let Some(map) = section.and_then(Yaml::as_mapping) else { return defaults };
    let dormant_after = map.get("dormant_after").and_then(days).unwrap_or(defaults.dormant_after);
    let check_every = map.get("check_every").and_then(Yaml::as_mapping).map(|entries| entries.iter()
        .filter_map(|(id, value)| Some((id.as_str()?.to_owned(), days(value)?))).collect()).unwrap_or_default();
    Settings { dormant_after, check_every }
}

/// Leases, snapshots, hub upkeep and main inspection keep running while
/// nothing happens; they are not activity.
fn bookkeeping(kind: &str) -> bool {
    kind.starts_with("lease.") || kind.starts_with("snapshot.") || kind.starts_with("hub.") || kind == "main.inspected"
}

/// A report or signal a connector (`from.kind: source`) sent.
fn connector(event: &Event) -> bool {
    matches!(event.kind.as_str(), "report.received" | "signal.received") && event.data["from"]["kind"] == "source"
}

fn time(at: &str) -> Option<DateTime<Utc>> {
    follow_ups::time(at)
}

/// The judgement time from Unix milliseconds, as the daemon's clock gives it.
pub fn instant(ms: u64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(i64::try_from(ms).unwrap_or(i64::MAX)).unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// One object's activity, in hub order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trail {
    pub last: Option<Mark>,
    /// The latest event a person signed.
    pub human: Option<Mark>,
    /// The latest event anyone else signed.
    pub other: Option<Mark>,
    /// The latest connector event that arrived after a quiet gap.
    pub woke: Option<Mark>,
}

impl Trail {
    fn record(&mut self, event: &Event, gap: Duration) {
        let mark = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
        let quiet_before = self.last.as_ref().and_then(|last| Some(time(&event.t)? - time(&last.at)?)).is_some_and(|since| since > gap);
        if connector(event) && quiet_before {
            self.woke = Some(mark.clone());
        }
        if human_actor(&event.actor) { self.human = Some(mark.clone()) } else { self.other = Some(mark.clone()) }
        self.last = Some(mark);
    }
}

/// Per object by `obj`, plus the project as a whole, where every event counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub project: Trail,
    pub objects: BTreeMap<String, Trail>,
}

pub fn activity<'a>(events: impl IntoIterator<Item = &'a Event>, dormant_after: u32) -> Activity {
    let gap = Duration::days(dormant_after.into());
    let mut events: Vec<&Event> = events.into_iter().filter(|event| !event.stale && !bookkeeping(&event.kind)).collect();
    events.sort_by_key(|event| event.seq);
    let mut activity = Activity::default();
    for event in events {
        activity.project.record(event, gap);
        activity.objects.entry(event.obj.clone()).or_default().record(event, gap);
        // FR-156: a write to one of a frame's cards keeps the frame awake.
        if let Ok(frame) = crate::cards::card_frame(&event.obj) {
            activity.objects.entry(frame.to_owned()).or_default().record(event, gap);
        }
    }
    activity
}

/// Why a quiet object came back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum Resurfaced {
    FollowUp { follow_up: String },
    CheckEvery { days: u32 },
    Connector { event: Mark },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObjectRhythm {
    pub id: String,
    pub last: Option<Mark>,
    pub quiet: bool,
    pub resurfaced: Option<Resurfaced>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rhythm {
    pub dormant_after: u32,
    /// The project first, then the objects in the order given.
    pub objects: Vec<ObjectRhythm>,
    /// Open follow-ups that are due now, by ID (FR-134).
    pub due: Vec<String>,
}

pub struct Inputs<'a> {
    pub now: DateTime<Utc>,
    pub project: &'a str,
    /// The objects to judge besides the project, e.g. the kits' delegations.
    pub objects: &'a [String],
    pub done: &'a BTreeSet<String>,
    pub signals: &'a [Merged<Signal>],
    pub follow_ups: &'a FollowUps,
    pub settings: &'a Settings,
}

fn older_than(mark: Option<&Mark>, now: DateTime<Utc>, days: u32) -> bool {
    mark.and_then(|mark| time(&mark.at)).is_some_and(|at| now - at > Duration::days(days.into()))
}

/// FR-134–136: which follow-ups are due, which objects are quiet and which
/// came back. A due follow-up resurfaces its quiet target; a rhythm that ran
/// out resurfaces its object whether or not it went quiet first; a connector
/// event after a gap stays resurfaced until a person's own event follows it.
pub fn rhythm<'a>(events: impl IntoIterator<Item = &'a Event>, inputs: &Inputs) -> Rhythm {
    let settings = inputs.settings;
    let activity = activity(events, settings.dormant_after);
    let others: BTreeMap<String, Mark> = activity.objects.iter()
        .filter_map(|(id, trail)| Some((id.clone(), trail.other.clone()?))).collect();
    let facts = Facts { now: inputs.now, done: inputs.done, others: &others, signals: inputs.signals };
    let due: Vec<&follow_ups::FollowUp> = follow_ups::open(inputs.follow_ups).filter(|item| follow_ups::due(item, &facts)).collect();
    let empty = Trail::default();
    let mut seen = BTreeSet::new();
    let ids = std::iter::once(inputs.project).chain(inputs.objects.iter().map(String::as_str)).filter(|id| seen.insert(*id));
    let objects = ids.map(|id| {
        let trail = if id == inputs.project { &activity.project } else { activity.objects.get(id).unwrap_or(&empty) };
        let dormant = older_than(trail.last.as_ref(), inputs.now, settings.dormant_after);
        let by_follow_up = due.iter().find(|item| dormant && item.when.target() == Some(id))
            .map(|item| Resurfaced::FollowUp { follow_up: item.id.clone() });
        let by_rhythm = || settings.check_every.get(id).filter(|days| older_than(trail.last.as_ref(), inputs.now, **days))
            .map(|days| Resurfaced::CheckEvery { days: *days });
        let by_connector = || trail.woke.clone().filter(|woke| !dormant
            && trail.human.as_ref().is_none_or(|human| human.seq < woke.seq)).map(|event| Resurfaced::Connector { event });
        let resurfaced = by_follow_up.or_else(by_rhythm).or_else(by_connector);
        ObjectRhythm { id: id.to_owned(), last: trail.last.clone(), quiet: dormant && resurfaced.is_none(), resurfaced }
    }).collect();
    Rhythm { dormant_after: settings.dormant_after, objects, due: due.iter().map(|item| item.id.clone()).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::follow_ups::{FollowUp, When};
    use serde_json::{json, Value};

    fn event(seq: u64, day: u32, kind: &str, actor: &str, obj: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":format!("2026-10-{day:02}T01:00:00Z"),"actor":actor,
            "type":kind,"layer":"L5","obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn agent(seq: u64, day: u32, obj: &str) -> Event {
        event(seq, day, "delegation.clarified", "claude-code/2.1", obj, json!({}))
    }

    fn note(seq: u64, day: u32, obj: &str, from: &str) -> Event {
        event(seq, day, "report.received", "process:towermd-cli", obj, json!({"from":{"id":"notemd","kind":from},"target":obj}))
    }

    fn read(text: &str) -> Settings {
        settings(serde_yaml::from_str::<Yaml>(text).unwrap().get("quiet"))
    }

    #[test]
    fn missing_or_malformed_settings_keep_the_defaults() {
        assert_eq!(settings(None), Settings { dormant_after: 14, check_every: BTreeMap::new() });
        assert_eq!(read("quiet: 3\n"), Settings::default());
        assert_eq!(read("quiet:\n  dormant_after: 0\n  check_every: [1]\n"), Settings::default());
        assert_eq!(read("quiet:\n  dormant_after: 99999\n").dormant_after, 14);
        let set = read("quiet:\n  dormant_after: 30\n  check_every:\n    T-1: 7\n    T-2: 0\n    T-3: x\n    p-1: 90\n");
        assert_eq!(set.dormant_after, 30);
        assert_eq!(set.check_every, BTreeMap::from([("T-1".into(), 7), ("p-1".into(), 90)]));
    }

    #[test]
    fn a_write_to_a_card_keeps_its_frame_awake() {
        let found = activity(&[agent(1, 1, "C-board"), agent(2, 5, "C-board.3"), agent(3, 6, "T-1.2")], 14);
        assert_eq!(found.objects["C-board"].last.as_ref().unwrap().seq, 2);
        assert_eq!(found.objects["C-board.3"].last.as_ref().unwrap().seq, 2);
        assert!(!found.objects.contains_key("T-1"), "only frame ids count as frames");
    }

    #[test]
    fn activity_skips_bookkeeping_and_stale_events_and_splits_people_from_others() {
        let mut stale = agent(4, 9, "T-1");
        stale.stale = true;
        let events = [agent(1, 1, "T-1"), event(2, 2, "follow_up.added", "human:local", "F-1", json!({})),
            event(3, 8, "lease.renewed", "process:towermd-hub", "main", json!({})), stale,
            event(5, 3, "acceptance.used", "human:local", "T-1", json!({})),
            event(6, 9, "snapshot.written", "process:towermd-snapshot", "p-1", json!({}))];
        let found = activity(&events, 14);
        assert_eq!(found.project.last.as_ref().unwrap().seq, 5);
        let trail = &found.objects["T-1"];
        assert_eq!((trail.last.as_ref().unwrap().seq, trail.human.as_ref().unwrap().seq, trail.other.as_ref().unwrap().seq), (5, 5, 1));
        assert!(!found.objects.contains_key("main"));
    }

    #[test]
    fn only_a_connector_after_a_quiet_gap_wakes_an_object() {
        let events = [agent(1, 1, "T-1"), agent(2, 1, "T-3"), note(3, 5, "T-1", "source"), note(4, 19, "T-3", "source"),
            note(5, 25, "T-1", "delegate"), note(6, 26, "T-2", "source")];
        let found = activity(&events, 14);
        assert!(found.objects["T-1"].woke.is_none(), "4 days is no gap, and a delegate is not a connector");
        assert!(found.objects["T-2"].woke.is_none(), "a first event wakes nothing");
        assert_eq!(found.objects["T-3"].woke.as_ref().unwrap().seq, 4);
        assert!(found.project.woke.is_none(), "the project kept moving");
    }

    fn follow_up(id: &str, seq: u64, when: When) -> (String, FollowUp) {
        (id.into(), FollowUp { id: id.into(), text: "x".into(), when, done: None,
            added: Mark { at: "2026-10-01T01:00:00Z".into(), eid: format!("e{seq}"), seq, actor: "human:local".into() } })
    }

    fn judge(events: &[Event], objects: &[&str], follow_ups: &FollowUps, settings: &Settings, now: &str) -> Rhythm {
        let objects: Vec<String> = objects.iter().map(|id| (*id).to_owned()).collect();
        let done = BTreeSet::from(["T-done".to_owned()]);
        rhythm(events, &Inputs { now: time(now).unwrap(), project: "p-1", objects: &objects, done: &done,
            signals: &[], follow_ups, settings })
    }

    fn by_id<'a>(rhythm: &'a Rhythm, id: &str) -> &'a ObjectRhythm {
        rhythm.objects.iter().find(|object| object.id == id).unwrap()
    }

    #[test]
    fn objects_go_quiet_after_dormant_after_days_without_events() {
        let events = [agent(1, 1, "T-old"), agent(2, 10, "T-new")];
        let found = judge(&events, &["T-old", "T-new", "T-none", "T-old"], &FollowUps::new(), &Settings::default(), "2026-10-16T01:00:01Z");
        assert_eq!(found.objects.iter().map(|object| object.id.as_str()).collect::<Vec<_>>(), ["p-1", "T-old", "T-new", "T-none"]);
        assert!(by_id(&found, "T-old").quiet);
        assert!(!by_id(&found, "T-new").quiet);
        assert!(!by_id(&found, "T-none").quiet, "nothing known is not quiet");
        assert!(!by_id(&found, "p-1").quiet, "the project moved on day 10");
        let later = judge(&events, &[], &FollowUps::new(), &Settings::default(), "2026-10-25T01:00:01Z");
        assert!(by_id(&later, "p-1").quiet);
        assert!(found.due.is_empty());
    }

    #[test]
    fn a_due_follow_up_resurfaces_its_quiet_target_and_is_listed() {
        let events = [agent(1, 1, "T-done"), agent(2, 1, "T-busy")];
        let items = FollowUps::from([follow_up("F-1", 3, When::Done { target: "T-done".into() }),
            follow_up("F-2", 4, When::Date { at: "2026-12-01T00:00:00Z".into() }),
            follow_up("F-3", 5, When::Done { target: "T-busy".into() })]);
        let found = judge(&events, &["T-done", "T-busy"], &items, &Settings::default(), "2026-10-20T00:00:00Z");
        assert_eq!(found.due, ["F-1"]);
        let object = by_id(&found, "T-done");
        assert_eq!((object.quiet, object.resurfaced.clone()), (false, Some(Resurfaced::FollowUp { follow_up: "F-1".into() })));
        assert!(by_id(&found, "T-busy").quiet, "a follow-up that is not due leaves it quiet");
    }

    #[test]
    fn a_rhythm_that_ran_out_resurfaces_until_a_new_event() {
        let settings = Settings { dormant_after: 14, check_every: BTreeMap::from([("T-1".into(), 7)]) };
        let events = [agent(1, 1, "T-1")];
        let found = judge(&events, &["T-1"], &FollowUps::new(), &settings, "2026-10-09T01:00:00Z");
        assert_eq!(by_id(&found, "T-1").resurfaced, Some(Resurfaced::CheckEvery { days: 7 }));
        assert!(!by_id(&found, "T-1").quiet);
        let seen = [agent(1, 1, "T-1"), event(2, 9, follow_ups::SEEN, "human:local", "T-1", json!({}))];
        let found = judge(&seen, &["T-1"], &FollowUps::new(), &settings, "2026-10-09T02:00:00Z");
        assert_eq!(by_id(&found, "T-1").resurfaced, None);
    }

    #[test]
    fn a_connector_after_a_gap_resurfaces_until_a_person_looks() {
        let events = [agent(1, 1, "T-1"), note(2, 20, "T-1", "source")];
        let found = judge(&events, &["T-1"], &FollowUps::new(), &Settings::default(), "2026-10-21T00:00:00Z");
        assert!(matches!(by_id(&found, "T-1").resurfaced, Some(Resurfaced::Connector { ref event }) if event.seq == 2));
        let agent_after = [agent(1, 1, "T-1"), note(2, 20, "T-1", "source"), agent(3, 20, "T-1")];
        let found = judge(&agent_after, &["T-1"], &FollowUps::new(), &Settings::default(), "2026-10-21T00:00:00Z");
        assert!(by_id(&found, "T-1").resurfaced.is_some(), "only a person settles it");
        let seen = [agent(1, 1, "T-1"), note(2, 20, "T-1", "source"), event(3, 20, follow_ups::SEEN, "human:local", "T-1", json!({}))];
        let found = judge(&seen, &["T-1"], &FollowUps::new(), &Settings::default(), "2026-10-21T00:00:00Z");
        assert!(by_id(&found, "T-1").resurfaced.is_none());
        let found = judge(&events, &["T-1"], &FollowUps::new(), &Settings::default(), "2026-11-20T00:00:00Z");
        assert!(by_id(&found, "T-1").quiet && by_id(&found, "T-1").resurfaced.is_none(), "it can go quiet again");
    }
}
