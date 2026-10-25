//! Commitment weakening (FR-125): a pure comparison of two versions of what
//! was promised, and the `core.weakening` signal Tower.md raises for it.
//!
//! Reported, whoever made the change:
//!
//! - `gates.yaml`: a gated check no longer gated, a hard gate made soft, a
//!   gate's check declared non-deterministic;
//! - `config.yaml` thresholds loosened in the direction the spec defines
//!   (`review_backlog` up, `spot_check_min` down);
//! - completion criteria: fewer items than before;
//! - checks: a check command that no longer appears anywhere.
//!
//! A change whose direction cannot be decided is never reported: a reworded
//! criterion, an unreadable file on either side, a renamed gate on the same
//! check. Blocking is the safe direction, so a declaration that now fails
//! closed is not a weakening either.
//!
//! Every raised signal is also kept in order in the `TowerApi` group, so the
//! track record (FR-202) can count the ones a delegate configuration caused.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_yaml::Value as Yaml;

use crate::{
    config::{Config, CONFIG_PATH},
    delegation_projection::Mark,
    gates::{GateFile, GATES_PATH},
    kit::Kit,
    okf::Document,
    tower_api::{self, Envelope, From, FromKind, Level, Signal, TowerLog, Verb},
};

/// The signal kind; only Tower.md raises it (`core.` namespace).
pub const KIND: &str = "core.weakening";
/// Ref naming the self-reported configuration that made the change.
pub const REF_CONFIG: &str = "config:";
/// Ref naming the delegated object the change was made for.
pub const REF_DELEGATION: &str = "delegation:";
/// Ref naming the proposal that carries the change.
pub const REF_PROPOSAL: &str = "proposal:";
/// Ref naming the authoritative main revision that carries the change.
pub const REF_MAIN: &str = "main:";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// A gated check is no longer gated.
    GateRemoved,
    /// A hard gate became soft.
    GateSoftened,
    /// A gate's check was declared non-deterministic.
    GateNondeterministic,
    ThresholdLoosened,
    CriteriaReduced,
    CheckRemoved,
}

/// One weakening, with the sentence the App shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub change: Change,
    pub text: String,
}

fn item(change: Change, text: String) -> Item {
    Item { change, text }
}

/// What a kit's list field promises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bar {
    /// Completion criteria: weakened when there are fewer.
    Criteria,
    /// Check commands: weakened when one disappears.
    Checks,
}

/// A list field in a kit's documents of one OKF `type` that states a commitment.
#[derive(Clone, Copy, Debug)]
pub struct Commitment {
    pub doc_type: &'static str,
    pub key: &'static str,
    pub bar: Bar,
}

/// A file before and after; `None` is absent (or not text).
#[derive(Clone, Copy, Debug)]
pub struct FileChange<'a> {
    pub path: &'a str,
    pub before: Option<&'a str>,
    pub after: Option<&'a str>,
}

/// Per gated check: hard if any gate on it is, non-deterministic if any
/// entry says so, and the gate names. Entries without a check gate nothing.
fn gated(file: &GateFile) -> BTreeMap<&str, (bool, bool, Vec<&str>)> {
    let mut checks: BTreeMap<&str, (bool, bool, Vec<&str>)> = BTreeMap::new();
    for gate in file.gates.values() {
        let Some(check) = gate.check.as_deref() else { continue };
        let entry = checks.entry(check).or_default();
        entry.0 |= gate.hard;
        entry.1 |= file.nondeterministic(&gate.name);
        entry.2.push(&gate.name);
    }
    checks
}

/// `gates.yaml`, compared per gated check. An unreadable file on either side
/// decides nothing: a broken file already blocks every completion.
pub fn gates(before: Option<&str>, after: Option<&str>) -> Vec<Item> {
    let (before, after) = (GateFile::from_text(before), GateFile::from_text(after));
    if before.error.is_some() || after.error.is_some() {
        return Vec::new();
    }
    let mut items = Vec::new();
    // The project's own G2 lists only add; dropping an entry loosens.
    for (label, was, now) in [("敏感路径", &before.sensitive_paths, &after.sensitive_paths),
        ("危险命令", &before.dangerous_commands, &after.dangerous_commands)] {
        for entry in was.iter().filter(|entry| !now.contains(entry)) {
            items.push(item(Change::GateRemoved, format!("删除 G2 {label}：{entry}")));
        }
    }
    let (before, after) = (gated(&before), gated(&after));
    for (check, (hard, nondeterministic, names)) in &before {
        let names = names.join("、");
        match after.get(check) {
            None => items.push(item(Change::GateRemoved, format!("删除门禁 {names}（{check}）"))),
            Some((now_hard, now_nondeterministic, _)) => {
                if *hard && !now_hard {
                    items.push(item(Change::GateSoftened, format!("门禁 {names}（{check}）由硬改软")));
                }
                if !nondeterministic && *now_nondeterministic {
                    items.push(item(Change::GateNondeterministic, format!("门禁 {names}（{check}）改为非确定性")));
                }
            }
        }
    }
    items
}

fn percent(rate: f64) -> String {
    format!("{}%", (rate * 10000.0).round() / 100.0)
}

/// `config.yaml` thresholds, by their effective values. An unreadable file
/// on either side decides nothing.
pub fn thresholds(before: Option<&str>, after: Option<&str>) -> Vec<Item> {
    let (Some(Ok(before)), Some(Ok(after))) = (before.map(Config::parse), after.map(Config::parse)) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    if after.review_backlog() > before.review_backlog() {
        items.push(item(Change::ThresholdLoosened,
            format!("待验收积压阈值由 {} 放宽到 {}", before.review_backlog(), after.review_backlog())));
    }
    if after.spot_check_min() < before.spot_check_min() {
        items.push(item(Change::ThresholdLoosened,
            format!("最低抽查比例由 {} 降到 {}", percent(before.spot_check_min()), percent(after.spot_check_min()))));
    }
    // FR-126: a stuck threshold is looser when more repeats, a shorter ask
    // window or a longer run are needed before Tower.md speaks.
    let (was, now) = (before.stuck(), after.stuck());
    let looser = [
        ("连续相同下一步阈值", was.same_next as f64, now.same_next as f64, true),
        ("连续检查失败阈值", was.check_failures as f64, now.check_failures as f64, true),
        ("提问次数阈值", was.asks as f64, now.asks as f64, true),
        ("提问统计窗口（小时）", was.ask_window_hours as f64, now.ask_window_hours as f64, false),
        ("时长倍数阈值", was.duration_factor, now.duration_factor, true),
    ];
    for (name, before, after, up) in looser {
        if (up && after > before) || (!up && after < before) {
            items.push(item(Change::ThresholdLoosened, format!("兜圈子{name}由 {before} 放宽到 {after}")));
        }
    }
    items
}

/// Completion criteria: weakened only when there are fewer. A rewording
/// cannot be judged and is not reported; the dropped wording is listed.
pub fn criteria(before: &[String], after: &[String]) -> Vec<Item> {
    if after.len() >= before.len() {
        return Vec::new();
    }
    let kept: BTreeSet<&str> = after.iter().map(|text| text.trim()).collect();
    let dropped: Vec<String> = before.iter().map(|text| text.trim()).filter(|text| !kept.contains(text))
        .map(|text| format!("「{text}」")).collect();
    let mut text = format!("完成标准由 {} 条减为 {} 条", before.len(), after.len());
    if !dropped.is_empty() {
        text.push_str(&format!("，删去{}", dropped.join("")));
    }
    vec![item(Change::CriteriaReduced, text)]
}

/// Check commands: each one that no longer appears is a removed check.
pub fn checks(before: &[String], after: &[String]) -> Vec<Item> {
    let kept: BTreeSet<&str> = after.iter().map(|text| text.trim()).collect();
    let mut seen = BTreeSet::new();
    before.iter().map(|text| text.trim()).filter(|text| !kept.contains(text) && seen.insert(*text))
        .map(|text| item(Change::CheckRemoved, format!("删除检查「{text}」"))).collect()
}

fn strings(document: &Document, key: &str) -> Vec<String> {
    document.get(key).and_then(Yaml::as_sequence).into_iter().flatten()
        .filter_map(Yaml::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned).collect()
}

/// A change set as a whole. `gates.yaml` and `config.yaml` by their own
/// rules; kit documents pooled across the set, so a criterion or check that
/// moves to another document in the same change is not lost.
pub fn files(kits: &[&dyn Kit], changes: &[FileChange]) -> Vec<Item> {
    let commitments: Vec<Commitment> = kits.iter().flat_map(|kit| kit.commitments().iter().copied()).collect();
    let mut items = Vec::new();
    let mut pools: BTreeMap<(Bar, &str), (Vec<String>, Vec<String>)> = BTreeMap::new();
    for change in changes {
        match change.path {
            GATES_PATH => items.extend(gates(change.before, change.after)),
            CONFIG_PATH => items.extend(thresholds(change.before, change.after)),
            _ => {
                let sides = [change.before, change.after].map(|text| text.and_then(|text| Document::parse(text).ok()));
                for (side, document) in sides.iter().enumerate() {
                    let Some(document) = document else { continue };
                    for commitment in commitments.iter().filter(|entry| document.doc_type() == Some(entry.doc_type)) {
                        // Checks pool across types: one command is one check.
                        let pool = if commitment.bar == Bar::Checks { "" } else { commitment.doc_type };
                        let lists = pools.entry((commitment.bar, pool)).or_default();
                        let list = if side == 0 { &mut lists.0 } else { &mut lists.1 };
                        list.extend(strings(document, commitment.key));
                    }
                }
            }
        }
    }
    for ((bar, _), (before, after)) in &pools {
        items.extend(match bar {
            Bar::Criteria => criteria(before, after),
            Bar::Checks => checks(before, after),
        });
    }
    items
}

/// Hub ID of the signal raised for one source (an event ID or a revision
/// pair), so a retry or another host raises it once.
pub fn eid(source: &str) -> String {
    tower_api::signal_eid(KIND, source)
}

/// `refs` for a signal: where the change came from and, when known, the
/// delegated object and configuration it is attributed to.
pub fn refs(source: String, delegation: Option<&str>, config: Option<&str>) -> Vec<String> {
    let mut refs = vec![source];
    refs.extend(delegation.map(|id| format!("{REF_DELEGATION}{id}")));
    refs.extend(config.map(|config| format!("{REF_CONFIG}{config}")));
    refs
}

/// The `active` signal listing every item, or `None` when nothing weakened.
pub fn signal(items: &[Item]) -> Option<Signal> {
    if items.is_empty() {
        return None;
    }
    let detail: Vec<char> = items.iter().map(|item| item.text.as_str()).collect::<Vec<_>>().join("；").chars().collect();
    let detail = if detail.len() > tower_api::DETAIL_CHARS {
        detail[..tower_api::DETAIL_CHARS - 1].iter().chain(['…'].iter()).collect()
    } else {
        detail.into_iter().collect()
    };
    Some(Signal {
        kind: KIND.into(), level: Level::Active, title: format!("承诺弱化：{} 处", items.len()),
        detail: Some(detail), value: Some(json!(items.len())), threshold: None, deterministic: true,
    })
}

/// The envelope Tower.md records as `signal.received` signed `process:towermd`.
pub fn envelope(project: &str, target: &str, refs: Vec<String>, at: String, items: &[Item]) -> Option<Envelope> {
    let signal = signal(items)?;
    Some(Envelope {
        v: tower_api::VERSION, project: project.into(),
        from: From { id: tower_api::TOWER.into(), kind: FromKind::Source },
        target: target.into(), verb: Verb::Signal, data: serde_json::to_value(signal).ok()?, refs, at,
    })
}

/// One raised `core.weakening`, in hub order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Raised {
    pub target: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    pub event: Mark,
}

impl Raised {
    fn names(&self, prefix: &str, value: &str) -> bool {
        self.refs.iter().any(|entry| entry.strip_prefix(prefix) == Some(value))
    }
}

/// FR-202: weakenings attributed to `config` working on `object`, raised
/// after `after` and up to `until` (hub sequence numbers).
pub fn count(log: &TowerLog, config: &str, object: &str, after: u64, until: u64) -> u32 {
    count_in(&log.weakenings, config, object, after, until)
}

/// The same count over any kept list of raised signals (`core.stuck` too).
pub fn count_in(raised: &[Raised], config: &str, object: &str, after: u64, until: u64) -> u32 {
    let found = raised.iter().filter(|raised| raised.event.seq > after && raised.event.seq <= until
        && raised.names(REF_CONFIG, config) && raised.names(REF_DELEGATION, object)).count();
    u32::try_from(found).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(items: &[Item]) -> Vec<(Change, &str)> {
        items.iter().map(|item| (item.change, item.text.as_str())).collect()
    }
    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    const GATES: &str = "gates:\n  crap: { check: eng.crap }\n  lint: { check: eng.lint, hard: false }\n  review: { check: ai.review, hard: false, deterministic: false }\n";

    #[test]
    fn gates_report_a_removed_gate_a_softened_one_and_a_check_made_nondeterministic() {
        assert!(gates(Some(GATES), Some(GATES)).is_empty());
        let removed = "gates:\n  lint: { check: eng.lint, hard: false }\n  review: { check: ai.review, hard: false, deterministic: false }\n";
        assert_eq!(texts(&gates(Some(GATES), Some(removed))), [(Change::GateRemoved, "删除门禁 crap（eng.crap）")]);
        let softened = GATES.replace("crap: { check: eng.crap }", "crap: { check: eng.crap, hard: false }");
        assert_eq!(texts(&gates(Some(GATES), Some(&softened))), [(Change::GateSoftened, "门禁 crap（eng.crap）由硬改软")]);
        let guessing = GATES.replace("lint: { check: eng.lint, hard: false }", "lint: { check: eng.lint, hard: false, deterministic: false }");
        assert_eq!(texts(&gates(Some(GATES), Some(&guessing))), [(Change::GateNondeterministic, "门禁 lint（eng.lint）改为非确定性")]);
        // Deleting the whole file removes every gated check.
        assert_eq!(gates(Some(GATES), None).iter().map(|item| item.change).collect::<Vec<_>>(), [Change::GateRemoved; 3]);
        assert_eq!(gates(Some(GATES), Some("")).len(), 3);
        // G2's project lists only add: removing an entry loosens, adding one does not.
        let listed = format!("{GATES}sensitive_paths: ['secrets/**']\ndangerous_commands: ['terraform destroy']\n");
        assert!(gates(Some(GATES), Some(&listed)).is_empty());
        assert_eq!(texts(&gates(Some(&listed), Some(GATES))), [(Change::GateRemoved, "删除 G2 敏感路径：secrets/**"),
            (Change::GateRemoved, "删除 G2 危险命令：terraform destroy")]);
    }

    #[test]
    fn gates_do_not_report_what_cannot_be_judged_or_only_strengthens() {
        // Added, hardened, made deterministic, or a renamed gate on the same check.
        assert!(gates(None, Some(GATES)).is_empty());
        let hardened = GATES.replace("lint: { check: eng.lint, hard: false }", "lint: { check: eng.lint }");
        assert!(gates(Some(GATES), Some(&hardened)).is_empty());
        let renamed = GATES.replace("crap:", "complexity:");
        assert!(gates(Some(GATES), Some(&renamed)).is_empty());
        // Another hard gate still guards the check.
        let doubled = format!("{GATES}  crap2: {{ check: eng.crap }}\n");
        let softened = doubled.replace("crap: { check: eng.crap }", "crap: { check: eng.crap, hard: false }");
        assert!(gates(Some(&doubled), Some(&softened)).is_empty());
        // A broken file blocks every completion: neither direction is a weakening.
        assert!(gates(Some(GATES), Some("gates: [")).is_empty());
        assert!(gates(Some("gates: ["), Some("")).is_empty());
        // A problem entry still gates its check and fails closed.
        let problem = GATES.replace("crap: { check: eng.crap }", "crap: { check: eng.crap, hard: maybe }");
        assert!(gates(Some(GATES), Some(&problem)).is_empty());
        // Unknown keys are not commitments.
        assert!(gates(Some(GATES), Some(&format!("owner: me\n{GATES}"))).is_empty());
    }

    #[test]
    fn thresholds_loosen_only_in_the_direction_the_spec_defines() {
        let base = "project_id: p\ndocs_dir: d\nreview_backlog: 5\nspot_check_min: 0.05\n";
        assert!(thresholds(Some(base), Some(base)).is_empty());
        let looser = base.replace("review_backlog: 5", "review_backlog: 8").replace("0.05", "0.025");
        assert_eq!(texts(&thresholds(Some(base), Some(&looser))), [
            (Change::ThresholdLoosened, "待验收积压阈值由 5 放宽到 8"),
            (Change::ThresholdLoosened, "最低抽查比例由 5% 降到 2.5%"),
        ]);
        // Tighter values, or a missing key that keeps the default, are not reported.
        assert!(thresholds(Some(&looser), Some(base)).is_empty());
        assert!(thresholds(Some(base), Some("project_id: p\ndocs_dir: d\n")).is_empty());
        // Removing a stricter value falls back to the looser default.
        let strict = base.replace("review_backlog: 5", "review_backlog: 2");
        assert_eq!(thresholds(Some(&strict), Some("project_id: p\ndocs_dir: d\n")).len(), 1);
        // Unreadable or missing on either side decides nothing.
        assert!(thresholds(Some(base), Some("project_id: [")).is_empty());
        assert!(thresholds(None, Some(base)).is_empty());
        // FR-126 stuck thresholds: more repeats or a shorter window is looser.
        let stuck = format!("{base}stuck:\n  same_next: 5\n  ask_window_hours: 12\n  duration_factor: 2\n");
        assert_eq!(texts(&thresholds(Some(base), Some(&stuck))), [
            (Change::ThresholdLoosened, "兜圈子连续相同下一步阈值由 3 放宽到 5"),
            (Change::ThresholdLoosened, "兜圈子提问统计窗口（小时）由 24 放宽到 12"),
        ]);
        assert_eq!(thresholds(Some(&stuck), Some(base)).len(), 1, "back to the default factor 3 is looser");
        assert!(thresholds(Some(base), None).is_empty());
    }

    #[test]
    fn criteria_weaken_only_when_there_are_fewer() {
        let before = list(&["tests pass", "docs updated", "no new warnings"]);
        assert_eq!(texts(&criteria(&before, &list(&["tests pass"]))),
            [(Change::CriteriaReduced, "完成标准由 3 条减为 1 条，删去「docs updated」「no new warnings」")]);
        assert_eq!(texts(&criteria(&before, &list(&["all fine", "ok"]))), [(Change::CriteriaReduced, "完成标准由 3 条减为 2 条，删去「tests pass」「docs updated」「no new warnings」")]);
        // A rewording or an addition cannot be judged as weaker.
        assert!(criteria(&before, &list(&["tests pass", "docs updated", "warnings ok"])).is_empty());
        assert!(criteria(&before, &list(&["tests pass", "docs updated", "no new warnings", "bench"])).is_empty());
        assert!(criteria(&[], &list(&["x"])).is_empty());
    }

    #[test]
    fn checks_report_each_command_that_no_longer_appears() {
        let before = list(&["cargo test", "cargo clippy", "cargo test"]);
        assert_eq!(texts(&checks(&before, &list(&["cargo test"]))), [(Change::CheckRemoved, "删除检查「cargo clippy」")]);
        assert_eq!(checks(&before, &[]).len(), 2);
        assert!(checks(&before, &list(&["cargo clippy", "cargo test", "cargo doc"])).is_empty());
    }

    struct Sample;
    impl Kit for Sample {
        fn commitments(&self) -> &'static [Commitment] {
            &[Commitment { doc_type: "Promise", key: "criteria", bar: Bar::Criteria },
              Commitment { doc_type: "Promise", key: "verify", bar: Bar::Checks },
              Commitment { doc_type: "Unit", key: "verify", bar: Bar::Checks }]
        }
    }
    fn doc(kind: &str, criteria: &[&str], verify: &[&str]) -> String {
        format!("---\ntype: {kind}\ncriteria: {}\nverify: {}\n---\n", json!(criteria), json!(verify))
    }

    #[test]
    fn files_route_gates_and_config_and_pool_kit_documents() {
        let kits: [&dyn Kit; 1] = [&Sample];
        let a = doc("Promise", &["one", "two"], &["make test"]);
        let fewer = doc("Promise", &["one"], &[]);
        let config = "project_id: p\ndocs_dir: d\n";
        let changes = [
            FileChange { path: "docs/a.md", before: Some(&a), after: Some(&fewer) },
            FileChange { path: GATES_PATH, before: Some(GATES), after: None },
            FileChange { path: CONFIG_PATH, before: Some(config), after: Some("project_id: p\ndocs_dir: d\nreview_backlog: 9\n") },
        ];
        let found = files(&kits, &changes);
        let kinds: Vec<_> = found.iter().map(|item| item.change).collect();
        assert_eq!(kinds, [Change::GateRemoved, Change::GateRemoved, Change::GateRemoved, Change::ThresholdLoosened,
            Change::CriteriaReduced, Change::CheckRemoved]);
        // A deleted document loses its criteria and checks.
        assert_eq!(files(&kits, &[FileChange { path: "docs/a.md", before: Some(&a), after: None }]).len(), 2);
        // Moving a criterion or a check to another document in the same change is not a loss.
        let b = doc("Unit", &[], &["make test"]);
        let moved = [
            FileChange { path: "docs/a.md", before: Some(&a), after: Some(&doc("Promise", &["one"], &[])) },
            FileChange { path: "docs/c.md", before: None, after: Some(&doc("Promise", &["two"], &[])) },
            FileChange { path: "docs/b.md", before: None, after: Some(&b) },
        ];
        assert!(files(&kits, &moved).is_empty());
        // Documents no kit declares, and unreadable ones, decide nothing.
        let other = "---\ntype: Note\ncriteria: [a, b]\n---\n";
        assert!(files(&kits, &[FileChange { path: "docs/n.md", before: Some(other), after: None }]).is_empty());
        assert!(files(&[], &[FileChange { path: "docs/a.md", before: Some(&a), after: None }]).is_empty());
        assert!(files(&kits, &[FileChange { path: "docs/a.md", before: Some("no frontmatter"), after: None }]).is_empty());
    }

    #[test]
    fn the_signal_is_active_names_every_item_and_fits_the_detail_limit() {
        assert!(signal(&[]).is_none());
        let items: Vec<_> = (0..20).map(|n| item(Change::CheckRemoved, format!("删除检查「cmd {n}」"))).collect();
        let signal = signal(&items).unwrap();
        assert_eq!((signal.kind.as_str(), signal.level, signal.title.as_str(), signal.value.clone()), (KIND, Level::Active, "承诺弱化：20 处", Some(json!(20))));
        let detail = signal.detail.clone().unwrap();
        assert_eq!(detail.chars().count(), tower_api::DETAIL_CHARS);
        assert!(detail.starts_with("删除检查「cmd 0」；") && detail.ends_with('…'));
        signal.check().unwrap();
        let envelope = envelope("p-1", "T-1", refs("proposal:P-1".into(), Some("T-1"), Some("unknown")),
            "2026-10-23T00:00:00.000Z".into(), &items).unwrap();
        envelope.check().unwrap();
        assert_eq!(envelope.refs, ["proposal:P-1", "delegation:T-1", "config:unknown"]);
        assert_eq!(eid("e-1"), eid("e-1"));
        assert_ne!(eid("e-1"), eid("e-2"));
    }

    #[test]
    fn count_attributes_only_to_the_named_configuration_and_object_in_range() {
        let raised = |seq: u64, refs: Vec<String>| Raised { target: "T-1".into(), refs,
            event: Mark { at: "2026-10-23T00:00:00Z".into(), eid: format!("e{seq}"), seq, actor: tower_api::TOWER.into() } };
        let log = TowerLog { weakenings: vec![
            raised(3, refs("proposal:P-1".into(), Some("T-1"), Some("cfg-a"))),
            raised(5, refs("proposal:P-2".into(), Some("T-1"), Some("cfg-b"))),
            raised(7, refs("main:abc".into(), None, None)),
            raised(9, refs("proposal:P-3".into(), Some("T-1"), Some("cfg-a"))),
        ], ..TowerLog::default() };
        assert_eq!(count(&log, "cfg-a", "T-1", 0, u64::MAX), 2);
        assert_eq!(count(&log, "cfg-a", "T-1", 3, 8), 0);
        assert_eq!(count(&log, "cfg-a", "T-2", 0, u64::MAX), 0);
        assert_eq!(count(&log, "cfg-b", "T-1", 0, 5), 1);
    }
}
