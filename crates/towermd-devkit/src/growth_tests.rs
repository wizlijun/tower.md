use serde_json::json;

use super::*;
use crate::dev_state::Stamp;
use crate::run_record::FinishMark;
use crate::run_report::{Outcome, ReportStatus};

fn now() -> u64 {
    chrono::DateTime::parse_from_rfc3339("2026-10-24T12:00:00Z").unwrap().timestamp_millis() as u64
}

fn module(id: &str, files: usize, lines: usize) -> ModuleSize {
    ModuleSize { id: id.into(), title: format!("{id} 模块"), files, lines }
}

fn layout(modules: Vec<ModuleSize>, unassigned: usize, unallocated: &[&str]) -> Layout {
    Layout {
        revision: "r".into(), modules,
        unassigned: (0..unassigned).map(|index| format!("loose/{index}.rs")).collect(),
        unallocated: unallocated.iter().map(|id| ((*id).into(), format!("{id} 意图"))).collect(),
    }
}

fn kinds(found: &[Finding]) -> Vec<(&str, &str)> {
    found.iter().map(|finding| (finding.signal.kind.as_str(), finding.target.as_str())).collect()
}

fn failed(at: &str, class: ErrorClass) -> RunRecord {
    RunRecord { task: "T-1".into(), module: "A".into(), agent: "a/1".into(), budget_usd: None, running: None, forgiven: 0,
        starts: 1, retries: 0, failures: 1, known_cost_usd: 0.0, unknown_cost: 0,
        finishes: vec![FinishMark { at: Stamp { at: at.into(), eid: format!("e-{at}"), seq: 1 }, outcome: Outcome::Failed,
            status: ReportStatus::Failed, stop: None, error_class: Some(class), cost_usd: None, session: None }], checkpoints: Vec::new(), process: None }
}

#[test]
fn defaults_follow_spec_9_3_and_config_overrides_them() {
    let defaults = Settings::from_config(None, None);
    assert_eq!(defaults, Settings::default());
    assert_eq!((defaults.module_lines, defaults.module_files, defaults.cross_window, defaults.cross_percent, defaults.unassigned_files),
        (3000, 40, 10, 30, 10));
    // No default count settles a project; only a person's config does.
    assert_eq!(defaults.mode(100), Mode::Growing);
    let signals: Yaml = serde_yaml::from_str("module_size: {lines: 100, files: -1}\ncross_module: {window: 4, ratio: 0.5}\nunassigned: {files: 2}\nrepeated_error: {count: 2}").unwrap();
    let growth: Yaml = serde_yaml::from_str("settle_at_modules: 3").unwrap();
    let settings = Settings::from_config(Some(&signals), Some(&growth));
    assert_eq!((settings.module_lines, settings.module_files, settings.cross_window, settings.cross_percent, settings.unassigned_files, settings.repeated.count),
        (100, 40, 4, 50, 2, 2));
    assert_eq!((settings.mode(2), settings.mode(3)), (Mode::Growing, Mode::Settled));
    let manual: Yaml = serde_yaml::from_str("mode: growing\nsettle_at_modules: 1").unwrap();
    assert_eq!(Settings::from_config(None, Some(&manual)).mode(5), Mode::Growing);
    let bad: Yaml = serde_yaml::from_str("cross_module: {ratio: 3}").unwrap();
    assert_eq!(Settings::from_config(Some(&bad), None).cross_percent, 30);
}

#[test]
fn each_signal_fires_only_over_its_threshold() {
    let settings = Settings::default();
    let quiet = layout(vec![module("A", 40, 3000)], 10, &[]);
    assert_eq!(findings(&settings, "p-1", &quiet, &[true, true, true], &BTreeMap::new(), now()), []);
    let busy = layout(vec![module("A", 41, 10), module("B", 2, 3001), module("C", 1, 1)], 11, &["I-1"]);
    // Three of the last ten tasks crossed modules: exactly 30% is not over.
    let mut recent = vec![true; 5];
    recent.extend([false, false, false, false, false, false, false, true, true, true]);
    let found = findings(&settings, "p-1", &busy, &recent, &BTreeMap::new(), now());
    assert_eq!(kinds(&found), [(MODULE_TOO_LARGE, "A"), (MODULE_TOO_LARGE, "B"), (UNASSIGNED, "p-1"), (UNALLOCATED_INTENT, "I-1")]);
    assert_eq!((found[1].signal.value.clone(), found[1].signal.threshold.clone()), (Some(json!(3001)), Some(json!(3000))));
    assert!(found.iter().all(|finding| finding.signal.level == Level::Active && finding.signal.title.chars().count() <= TITLE_CHARS));
    recent.push(true);
    let found = findings(&settings, "p-1", &busy, &recent, &BTreeMap::new(), now());
    assert!(kinds(&found).contains(&(CROSS_MODULE, "p-1")));
    // Fewer tasks than the window are not judged yet.
    assert!(!kinds(&findings(&settings, "p-1", &busy, &[true; 9], &BTreeMap::new(), now())).contains(&(CROSS_MODULE, "p-1")));
    let mut records = BTreeMap::new();
    for (run, at) in [("R-1", "2026-10-24T01:00:00Z"), ("R-2", "2026-10-24T02:00:00Z"), ("R-3", "2026-10-24T03:00:00Z")] {
        records.insert(run.to_owned(), failed(at, ErrorClass::Execution));
    }
    let found = findings(&settings, "p-1", &quiet, &[], &records, now());
    assert_eq!(kinds(&found), [(REPEATED_ERROR, "p-1")]);
    assert_eq!(found[0].signal.title, "同类错误反复：执行错误");
}

#[test]
fn a_settled_project_allows_no_unassigned_file() {
    let settled: Yaml = serde_yaml::from_str("mode: settled").unwrap();
    let settings = Settings::from_config(None, Some(&settled));
    let found = findings(&settings, "p-1", &layout(vec![module("A", 1, 1)], 1, &[]), &[], &BTreeMap::new(), now());
    assert_eq!(kinds(&found), [(UNASSIGNED, "p-1")]);
    assert_eq!(found[0].signal.threshold, Some(json!(0)));
    assert!(found[0].signal.detail.as_deref().unwrap().starts_with("定型期"));
}

fn logged(kind: &str, target: &str, level: &str, count: u64, from: &str) -> Value {
    let mark = json!({"at":"2026-10-24T00:00:00Z","eid":"e","seq":1,"actor":from});
    json!({"from":{"id":from,"kind":"source"},"target":target,"data":{"kind":kind,"level":level,"title":"t"},
        "at":"2026-10-24T00:00:00Z","count":count,"first":mark,"last":mark})
}

#[test]
fn only_transitions_are_raised_and_each_has_its_own_id() {
    let settings = Settings::default();
    let found = findings(&settings, "p-1", &layout(vec![module("A", 41, 1)], 0, &["I-1"]), &[], &BTreeMap::new(), now());
    let empty = TowerLog::default();
    let first = transitions(&empty, found.clone());
    assert_eq!(first.iter().map(|raise| (raise.signal.kind.as_str(), raise.target.as_str())).collect::<Vec<_>>(),
        [(MODULE_TOO_LARGE, "A"), (UNALLOCATED_INTENT, "I-1")]);
    assert_eq!(first[0].eid, signal_eid(MODULE_TOO_LARGE, "A#1"));
    // Standing findings are not resent; one that went away is cleared once;
    // an agent's self-reported growth signal is not Tower.md's to clear.
    let log: TowerLog = serde_json::from_value(json!({"signals":[
        logged(MODULE_TOO_LARGE, "A", "active", 1, TOWER), logged(UNALLOCATED_INTENT, "I-1", "active", 1, TOWER),
        logged(UNASSIGNED, "p-1", "active", 2, TOWER), logged(UNASSIGNED, "p-1", "active", 1, "claude-code/2"),
        logged(CROSS_MODULE, "p-1", "log", 1, TOWER)]})).unwrap();
    let next = transitions(&log, found.clone());
    assert_eq!(next.len(), 1);
    assert_eq!((next[0].signal.kind.as_str(), next[0].signal.level, next[0].eid.clone()), (UNASSIGNED, Level::Log, signal_eid(UNASSIGNED, "p-1#3")));
    // A cleared signal that comes back is raised again under a new ID.
    let mut back = found;
    back.push(findings(&settings, "p-1", &layout(vec![], 0, &[]), &[true; 10], &BTreeMap::new(), now()).remove(0));
    let again = transitions(&log, back);
    assert_eq!(again.iter().find(|raise| raise.signal.kind == CROSS_MODULE).map(|raise| raise.eid.clone()), Some(signal_eid(CROSS_MODULE, "p-1#2")));
}
