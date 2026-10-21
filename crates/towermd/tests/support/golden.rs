//! Records v0.1.1 replay fixtures from a finished real CLI flow. Inert unless
//! TOWERMD_RECORD_REPLAY_GOLDEN is set, so normal test runs never write files.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn record(root: &Path, case: &str) {
    if std::env::var_os("TOWERMD_RECORD_REPLAY_GOLDEN").is_none() {
        return;
    }
    let journal = fs::read_to_string(root.join(".git/towermd/hub/journal.jsonl")).unwrap();
    // Only newline-terminated lines are durable journal records.
    let complete = &journal[..journal.rfind('\n').map_or(0, |end| end + 1)];
    let mut records = complete.lines();
    let header: serde_json::Value = serde_json::from_str(records.next().unwrap()).unwrap();
    assert_eq!(header["version"], 2, "journal header");
    // Every snapshot ACK compacts the journal, so the full history is the
    // acknowledged Git snapshot archive followed by the journal tail.
    let mut lines = match header["checkpoint"]["ack"]["commit"].as_str() {
        Some(commit) => archived(root, commit),
        None => Vec::new(),
    };
    lines.extend(records.map(str::to_owned));
    let events: Vec<towermd_core::hub::Event> =
        lines.iter().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(events.first().map(|event| event.seq), Some(1), "golden needs the full history");
    assert!(events.windows(2).all(|pair| pair[0].seq < pair[1].seq), "golden history out of order");
    let initial = towermd_devkit::dev_state::initial();
    let projection =
        towermd_core::project_runtime::replay(&initial, &events, towermd_devkit::dev_state::apply)
            .unwrap();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../towermd-devkit/tests/fixtures/replay-v0.1.1")
        .join(case);
    fs::create_dir_all(&dir).unwrap();
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    fs::write(dir.join("events.jsonl"), body).unwrap();
    fs::write(dir.join("projection.json"), serde_json::to_string_pretty(&projection).unwrap() + "\n").unwrap();
    fs::write(dir.join("initial.json"), serde_json::to_string_pretty(&initial).unwrap() + "\n").unwrap();
}

/// Archived event lines of a snapshot commit, ordered by sequence.
fn archived(root: &Path, commit: &str) -> Vec<String> {
    let mut lines: Vec<(u64, String)> = Vec::new();
    for path in git(root, &["ls-tree", "-r", "--name-only", commit]).lines() {
        if !path.contains("/state/events/") || !path.ends_with(".jsonl") {
            continue;
        }
        for line in git(root, &["show", &format!("{commit}:{path}")]).lines() {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            lines.push((event["seq"].as_u64().unwrap(), line.to_owned()));
        }
    }
    lines.sort_by_key(|(seq, _)| *seq);
    lines.into_iter().map(|(_, line)| line).collect()
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}
