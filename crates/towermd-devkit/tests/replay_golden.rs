//! v0.1.1 replay lock: journals recorded from real CLI flows must replay to
//! the recorded projection, and every event line must round-trip unchanged.
use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_json::Value;
use towermd_core::{hub::Event, project_runtime};
use towermd_devkit::dev_state;

const CASES: [&str; 4] = ["finish", "app_pause", "proposal", "controls"];
const REQUIRED_KINDS: [&str; 12] = [
    "baseline.approved",
    "baseline.created",
    "task.created",
    "task.claimed",
    "task.finish.requested",
    "task.finish.started",
    "task.finish.completed",
    "task.g5.completed",
    "task.merged",
    "hold.opened",
    "project.paused",
    "proposal.submitted",
];

fn case_dir(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replay-v0.1.1").join(case)
}

fn lines(case: &str) -> Vec<String> {
    fs::read_to_string(case_dir(case).join("events.jsonl"))
        .unwrap_or_else(|error| panic!("{case}: {error}"))
        .lines()
        .map(str::to_owned)
        .collect()
}

fn events(case: &str) -> Vec<Event> {
    lines(case).iter().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn recorded(case: &str, name: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(case_dir(case).join(name)).unwrap()).unwrap()
}

#[test]
fn v0_1_1_journals_replay_to_the_recorded_projection() {
    for case in CASES {
        let projection =
            project_runtime::replay(&dev_state::initial(), &events(case), dev_state::apply)
                .unwrap_or_else(|error| panic!("{case}: {error}"));
        assert_eq!(projection, recorded(case, "projection.json"), "{case}");
    }
}

#[test]
fn v0_1_1_event_lines_round_trip_byte_for_byte() {
    for case in CASES {
        for line in lines(case) {
            let event: Event = serde_json::from_str(&line).unwrap();
            assert_eq!(serde_json::to_string(&event).unwrap(), line, "{case}");
        }
    }
}

#[test]
fn initial_projection_is_unchanged() {
    for case in CASES {
        assert_eq!(dev_state::initial(), recorded(case, "initial.json"), "{case}");
    }
}

#[test]
fn golden_covers_the_required_event_kinds() {
    let kinds: BTreeSet<String> =
        CASES.iter().flat_map(|case| events(case)).map(|event| event.kind).collect();
    for kind in REQUIRED_KINDS {
        assert!(kinds.contains(kind), "missing {kind}");
    }
}
