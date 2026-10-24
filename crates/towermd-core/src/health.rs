//! Health colour and aggregates (core spec §9.3: FR-39, FR-133). Derived on
//! every read from the state body and its ordered attention list; nothing is
//! stored. The colour always travels with a shape and a text label, so it
//! never rests on colour alone.
//!
//! Red: an unhandled protected change, failed check, loop, interrupt, run error or
//! spent budget, or a pending proposal a delegate waits on. Yellow: an open
//! question or review (including a review a delegate asked for), a
//! stuck object, a running delegation with no confirmed progress past
//! `health.no_progress_hours`, or `passive` signals from three or more
//! senders on one target. Grey: nothing delegated. Green otherwise. A red or
//! yellow reason wins over grey, so a question on an empty project still shows.
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::attention_list::{self, parse_time, Entry, Kind, List};
use crate::kit::Kit;

/// `config.yaml` `health:`; see `Config::health`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Settings {
    pub no_progress_hours: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { no_progress_hours: 24 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    Green,
    Grey,
    Yellow,
    Red,
}

impl Color {
    pub fn label(self) -> &'static str {
        match self { Color::Red => "需处理", Color::Yellow => "需留意", Color::Green => "正常", Color::Grey => "没有任务" }
    }
    pub fn symbol(self) -> &'static str {
        match self { Color::Red => "▲", Color::Yellow => "◆", Color::Green => "●", Color::Grey => "○" }
    }
}

/// One project's or one area's colour and FR-39 aggregate fields.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Health {
    pub color: Color,
    pub label: &'static str,
    pub symbol: &'static str,
    /// Why it is red or yellow, worst first; empty when green or grey.
    pub reasons: Vec<String>,
    pub done: usize,
    pub running: usize,
    pub blocked: usize,
    /// Unhandled attention items per kind.
    pub interrupts: BTreeMap<String, usize>,
    /// The worst current check verdict over delegated objects: `failed`,
    /// `pending` or `passed`; absent when no check applies.
    pub check: Option<&'static str>,
}

/// The project's health with each area's (a container on the canvas).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    #[serde(flatten)]
    pub project: Health,
    pub areas: BTreeMap<String, Health>,
}

struct Scope<'a> {
    objects: Vec<&'a Value>,
    entries: Vec<&'a Entry>,
    stuck: usize,
    crowded: usize,
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn kind_name(kind: Kind) -> String {
    serde_json::to_value(kind).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default()
}

/// A running delegation whose latest stamp is older than the threshold.
fn idle(object: &Value, now: u64, hours: u64) -> bool {
    if text(object, "phase") != Some("running") { return false }
    let attempts = object.get("attempts").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let Some(attempt) = attempts.iter().rev().find(|attempt| attempt.get("active").and_then(Value::as_bool) == Some(true)) else { return false };
    let latest = ["delegated", "delivered"].iter().filter_map(|key| attempt.get(*key).and_then(|stamp| text(stamp, "at")))
        .map(parse_time).max().unwrap_or(0);
    latest > 0 && now.saturating_sub(latest) > hours * 60 * 60 * 1000
}

fn judge(scope: &Scope, blocking: &BTreeSet<String>, settings: Settings, now: u64) -> Health {
    let count = |test: &dyn Fn(&Entry) -> bool| scope.entries.iter().filter(|entry| test(entry)).count();
    let mut red = vec![];
    for kind in [Kind::Blocked, Kind::CheckFailed, Kind::Stuck, Kind::Interrupt, Kind::Error, Kind::Budget] {
        let n = count(&|entry| entry.item.kind == kind);
        if n > 0 { red.push(format!("{n} 条{}", kind.label())) }
    }
    let waited = count(&|entry| entry.item.kind == Kind::Proposal && blocking.contains(&entry.item.id));
    if waited > 0 { red.push(format!("{waited} 个提案阻塞受托者")) }
    let mut yellow = vec![];
    let questions = count(&|entry| matches!(entry.item.kind, Kind::Question | Kind::Unanswerable));
    if questions > 0 { yellow.push(format!("{questions} 条待回答")) }
    let reviews = count(&|entry| matches!(entry.item.kind, Kind::SpotCheck | Kind::Weakening | Kind::Review)
        || (entry.item.kind == Kind::Proposal && !blocking.contains(&entry.item.id)));
    if reviews > 0 { yellow.push(format!("{reviews} 条待评审")) }
    if scope.stuck > 0 { yellow.push(format!("{} 件受阻", scope.stuck)) }
    let idle = scope.objects.iter().filter(|object| idle(object, now, settings.no_progress_hours)).count();
    if idle > 0 { yellow.push(format!("{idle} 件超过 {} 小时没有已确认的进展", settings.no_progress_hours)) }
    if scope.crowded > 0 { yellow.push(format!("{} 处被 3 个以上来源提醒", scope.crowded)) }
    let color = if !red.is_empty() { Color::Red } else if !yellow.is_empty() { Color::Yellow }
        else if scope.objects.is_empty() { Color::Grey } else { Color::Green };
    let phase = |names: &[&str]| scope.objects.iter().filter(|object| text(object, "phase").is_some_and(|phase| names.contains(&phase))).count();
    let mut interrupts = BTreeMap::new();
    for entry in &scope.entries {
        *interrupts.entry(kind_name(entry.item.kind)).or_insert(0) += 1;
    }
    let verdicts: BTreeSet<&str> = scope.objects.iter().filter_map(|object| object.get("completion").and_then(|completion| text(completion, "verdict"))).collect();
    let check = if verdicts.contains("blocked") { Some("failed") } else if verdicts.contains("pending") { Some("pending") }
        else if verdicts.contains("passed") { Some("passed") } else { None };
    Health { color, label: color.label(), symbol: color.symbol(), reasons: red.into_iter().chain(yellow).collect(),
        done: phase(&["done"]), running: phase(&["running", "claimed_done"]), blocked: scope.stuck, interrupts, check }
}

/// FR-39 for a ready state body and its ordered list.
pub fn report(state: &Value, kits: &[&dyn Kit], list: &List, settings: Settings, now: u64) -> Report {
    let projection = state.get("projection").unwrap_or(&Value::Null);
    let objects: Vec<&Value> = state.get("delegation_view").and_then(|view| view.get("objects")).and_then(Value::as_array)
        .map(|objects| objects.iter().collect()).unwrap_or_default();
    let area_of = |id: &str| kits.iter().find_map(|kit| kit.locate(projection, id)).map(|located| located.area);
    let stuck: BTreeSet<String> = attention_list::stuck(state, kits).into_iter().map(|entry| entry.id).collect();
    let crowded = attention_list::crowded_targets(projection.get(crate::tower_api::GROUP), now);
    let blocking = attention_list::blocking_proposals(projection);
    let whole = Scope { objects: objects.clone(), entries: list.items.iter().collect(), stuck: stuck.len(), crowded: crowded.len() };
    let mut areas: BTreeSet<String> = list.items.iter().flat_map(|entry| entry.item.areas.iter().cloned()).filter(|area| !area.is_empty()).collect();
    areas.extend(objects.iter().filter_map(|object| text(object, "id")).filter_map(area_of));
    let areas = areas.into_iter().map(|area| {
        let here = |id: &str| area_of(id).as_deref() == Some(area.as_str());
        let scope = Scope {
            objects: objects.iter().copied().filter(|object| text(object, "id").is_some_and(here)).collect(),
            entries: list.items.iter().filter(|entry| entry.item.areas.contains(&area)).collect(),
            stuck: stuck.iter().filter(|id| here(id)).count(),
            crowded: crowded.iter().filter(|target| here(target)).count(),
        };
        let health = judge(&scope, &blocking, settings, now);
        (area, health)
    }).collect();
    Report { project: judge(&whole, &blocking, settings, now), areas }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention_list::Located;
    use serde_json::json;

    struct Places;
    impl Kit for Places {
        fn locate(&self, projection: &Value, object: &str) -> Option<Located> {
            let area = projection.get("Area")?.get(object)?.as_str()?;
            Some(Located { area: area.into(), reaches: vec![] })
        }
    }
    const KITS: &[&dyn Kit] = &[&Places];
    const NOW: &str = "2026-10-24T12:00:00Z";

    fn object(id: &str, phase: &str, verdict: &str, delegated: &str) -> Value {
        json!({"id": id, "title": id, "phase": phase, "completion": {"verdict": verdict},
            "attempts": [{"delegated": {"at": delegated}, "active": phase == "running"}]})
    }
    fn state(objects: Vec<Value>, projection: Value) -> Value {
        json!({"status": "ready", "projection": projection, "delegation_view": {"objects": objects}})
    }
    fn health(state: &Value) -> Report {
        let now = parse_time(NOW);
        let list = attention_list::list(state, KITS, now);
        report(state, KITS, &list, Settings::default(), now)
    }

    #[test]
    fn colour_follows_the_spec_table_with_shape_and_text() {
        let empty = health(&state(vec![], json!({})));
        assert_eq!((empty.project.color, empty.project.symbol, empty.project.label), (Color::Grey, "○", "没有任务"));
        let fresh = object("T-1", "running", "pending", "2026-10-24T10:00:00Z");
        let calm = health(&state(vec![fresh.clone(), object("T-2", "done", "passed", "2026-10-20T00:00:00Z")], json!({"Area": {"T-1": "A", "T-2": "B"}})));
        assert_eq!((calm.project.color, calm.project.done, calm.project.running, calm.project.check), (Color::Green, 1, 1, Some("pending")));
        assert_eq!(calm.areas.keys().collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(calm.areas["B"].check, Some("passed"));

        let stale = object("T-1", "running", "pending", "2026-10-23T08:00:00Z");
        let idle = health(&state(vec![stale], json!({"Area": {"T-1": "A"}})));
        assert_eq!(idle.project.color, Color::Yellow);
        assert_eq!(idle.project.reasons, ["1 件超过 24 小时没有已确认的进展"]);
        assert_eq!(idle.areas["A"].color, Color::Yellow);

        let alert = json!({"A-1": {"data": {"reason": "unapproved_protected_change", "detail": {"task": "T-3"}}, "at": "2026-10-24T09:00:00Z"}});
        let red = health(&state(vec![fresh, object("T-3", "proposed", "blocked", "2026-10-24T10:00:00Z")], json!({"Alert": alert, "Area": {"T-1": "A", "T-3": "B"}})));
        assert_eq!((red.project.color, red.project.symbol, red.project.check), (Color::Red, "▲", Some("failed")));
        assert_eq!(red.project.reasons, ["1 条禁止"]);
        assert_eq!(red.project.interrupts.get("blocked"), Some(&1));
        assert_eq!((red.areas["A"].color, red.areas["B"].color), (Color::Green, Color::Red), "an area only takes its own items");
    }

    #[test]
    fn three_passive_senders_on_one_target_turn_it_yellow_until_handled() {
        let signal = |from: &str| json!({"from": {"id": from, "kind": "source"}, "target": "T-1", "count": 1,
            "data": {"kind": "x.y", "level": "passive", "title": "t"}, "last": {"at": "2026-10-24T11:00:00Z", "eid": from, "seq": 1, "actor": "process:towermd-cli"}});
        let fresh = object("T-1", "running", "pending", "2026-10-24T10:00:00Z");
        let two = health(&state(vec![fresh.clone()], json!({"TowerApi": {"signals": [signal("a"), signal("b")]}, "Area": {"T-1": "A"}})));
        assert_eq!(two.project.color, Color::Green);
        let three = health(&state(vec![fresh], json!({"TowerApi": {"signals": [signal("a"), signal("b"), signal("c")]}, "Area": {"T-1": "A"}})));
        assert_eq!((three.project.color, three.areas["A"].color), (Color::Yellow, Color::Yellow));
        assert_eq!(three.project.reasons, ["1 处被 3 个以上来源提醒"]);
    }
}
