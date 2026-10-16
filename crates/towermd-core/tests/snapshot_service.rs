use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use towermd_core::catalog::Catalog;
use towermd_core::hub::Hub;
use towermd_core::init::{init, InitOptions};
use towermd_core::snapshot::{Snapshot, StateProjection};
use towermd_core::snapshot_service::{due, interval_ms, SnapshotWriter};
use towermd_core::tasks::{self, TaskCommand, TaskSpec};

const NOW: u64 = 1_800_000_000_000;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}

fn fixture() -> (tempfile::TempDir, Catalog, Mutex<Hub>) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    git(root, &["init", "-qb", "main"]);
    init(root, &InitOptions { docs_dir: None }).unwrap();
    fs::write(
        root.join("docs/tower/arch/A.md"),
        "---\ntype: Module\nid: A\npaths: [src/**]\n---\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/tower/specs/AC.md"),
        "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root, &["switch", "-qc", "snapshot-fixture"]);
    let catalog = Catalog::load(root).unwrap();
    let hub = Mutex::new(Hub::open(&catalog.common_dir.join("towermd")).unwrap());
    (temporary, catalog, hub)
}

fn add(hub: &Mutex<Hub>, catalog: &Catalog, id: &str, now: u64) {
    tasks::execute(
        &mut hub.lock().unwrap(),
        catalog,
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: id.into(),
                title: "test".into(),
                module: "A".into(),
                touches: Vec::new(),
                allowed_paths: vec!["src/**".into()],
                depends_on: Vec::new(),
                acceptance: vec!["AC".into()],
                decisions: Vec::new(),
            },
        },
        None,
        now,
    )
    .unwrap();
}

#[test]
fn snapshot_commits_checkpoint_then_acknowledges_and_survives_restart() {
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    let report = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    assert_eq!(report.status, "written");
    assert_eq!(
        report.commit.as_deref(),
        Some(git(temporary.path(), &["rev-parse", "main"]).as_str())
    );
    let state: Snapshot = serde_json::from_str(&git(
        temporary.path(),
        &["show", "main:docs/tower/state/state.json"],
    ))
    .unwrap();
    assert_eq!(state.through_seq, report.seq);
    assert_eq!(state.projection.objects["Task"]["T-1"]["state"], "ready");
    assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().seq, report.seq);
    assert_eq!(git(temporary.path(), &["status", "--porcelain"]), "");
    drop(hub);
    let reopened = Hub::open(&catalog.common_dir.join("towermd")).unwrap();
    assert_eq!(reopened.snapshot_ack().unwrap().seq, report.seq);
}

#[test]
fn subsequent_snapshot_appends_archive_without_heartbeats() {
    let (temporary, catalog, hub) = fixture();
    let writer = SnapshotWriter::default();
    add(&hub, &catalog, "T-1", NOW);
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let catalog = Catalog::load(temporary.path()).unwrap();
    add(&hub, &catalog, "T-2", NOW + 1000);
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW + 1000))
        .unwrap();
    let state: Snapshot = serde_json::from_str(&git(
        temporary.path(),
        &["show", "main:docs/tower/state/state.json"],
    ))
    .unwrap();
    let mut archive = Vec::new();
    let paths = git(
        temporary.path(),
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "main",
            "--",
            "docs/tower/state/events",
        ],
    );
    for path in paths.lines() {
        for line in git(temporary.path(), &["show", &format!("main:{path}")]).lines() {
            let event: towermd_core::hub::Event = serde_json::from_str(line).unwrap();
            assert!(!event.is_pure_heartbeat());
            archive.push(event);
        }
    }
    archive.sort_by_key(|event| event.seq);
    state
        .validate_archive(catalog.config.project_id(), &archive)
        .unwrap();
    let mut replay = StateProjection::default();
    for event in archive {
        replay.apply_recorded(&event).unwrap();
    }
    assert_eq!(replay, state.projection);
    assert_eq!(replay.objects["Task"].len(), 2);
}

#[test]
fn checked_main_defers_without_acknowledging_unwritten_sequence() {
    for dirty in [false, true] {
        let (temporary, catalog, hub) = fixture();
        let writer = SnapshotWriter::default();
        add(&hub, &catalog, "T-1", NOW);
        let first = writer
            .write(&hub, &catalog, "host", true, || Ok(NOW))
            .unwrap();
        assert_eq!(first.status, "written");
        git(temporary.path(), &["switch", "main"]);
        let catalog = Catalog::load(temporary.path()).unwrap();
        let path = temporary.path().join("docs/tower/state/state.json");
        if dirty {
            fs::write(&path, "user edits").unwrap();
        }
        let before = fs::read(&path).unwrap();
        add(&hub, &catalog, "T-2", NOW + 1000);
        let report = writer
            .write(&hub, &catalog, "host", true, || Ok(NOW + 1000))
            .unwrap();
        assert_eq!(report.status, "deferred");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().seq, first.seq);
        assert_eq!(
            git(temporary.path(), &["rev-parse", "main"]),
            first.commit.unwrap()
        );
    }
}

#[test]
fn periodic_snapshot_requires_business_changes_and_elapsed_interval() {
    let (temporary, catalog, hub) = fixture();
    assert_eq!(interval_ms(&catalog).unwrap(), 30 * 60 * 1000);
    assert!(!due(&hub.lock().unwrap(), 1000, NOW).unwrap());
    add(&hub, &catalog, "T-1", NOW);
    assert!(!due(&hub.lock().unwrap(), 1000, NOW + 999).unwrap());
    assert!(due(&hub.lock().unwrap(), 1000, NOW + 1000).unwrap());
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW + 1000))
        .unwrap();
    assert!(!due(&hub.lock().unwrap(), 1000, NOW + 100_000).unwrap());
    let catalog = Catalog::load(temporary.path()).unwrap();
    add(&hub, &catalog, "T-2", NOW + 1500);
    assert!(due(&hub.lock().unwrap(), 1000, NOW + 2000).unwrap());
}

#[test]
fn lost_hub_cannot_start_a_new_history_over_an_existing_snapshot() {
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let catalog = Catalog::load(temporary.path()).unwrap();
    drop(hub);
    fs::remove_file(catalog.common_dir.join("towermd/journal.json")).unwrap();
    let hub = Hub::open(&catalog.common_dir.join("towermd")).unwrap();
    let error = towermd_core::snapshot_service::validate_authority(&hub, &catalog).unwrap_err();
    assert!(error.contains("恢复"), "{error}");
    assert_eq!(hub.last_seq(), 0);
}

#[test]
fn git_success_without_hub_ack_is_reconciled_without_duplicate_fact() {
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    let first = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    drop(hub);
    let journal = catalog.common_dir.join("towermd/journal.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
    value["events"]
        .as_array_mut()
        .unwrap()
        .retain(|event| event["type"] != "snapshot.written");
    value.as_object_mut().unwrap().remove("snapshot_ack");
    fs::write(&journal, serde_json::to_vec(&value).unwrap()).unwrap();
    let hub = Mutex::new(Hub::open(&catalog.common_dir.join("towermd")).unwrap());
    let catalog = Catalog::load(temporary.path()).unwrap();
    let report = writer
        .write(&hub, &catalog, "host", false, || Ok(NOW + 1000))
        .unwrap();
    assert_eq!(report.status, "unchanged");
    assert_eq!(
        hub.lock().unwrap().snapshot_ack().unwrap().commit,
        first.commit.unwrap()
    );
    writer
        .write(&hub, &catalog, "host", false, || Ok(NOW + 2000))
        .unwrap();
    assert_eq!(
        hub.lock()
            .unwrap()
            .events(0)
            .iter()
            .filter(|event| event.kind == "snapshot.written")
            .count(),
        1
    );
}

#[test]
fn main_lease_renewal_keeps_epoch_but_expiry_never_revives_it() {
    let (_temporary, catalog, hub) = fixture();
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let mut hub = hub.lock().unwrap();
    let original = hub.leases(NOW)["main"].clone();
    towermd_core::snapshot_service::maintain_main(&mut hub, "host", NOW + 30_000).unwrap();
    let renewed = hub.leases(NOW + 30_000)["main"].clone();
    assert_eq!(original.epoch, renewed.epoch);
    assert_eq!(renewed.expires_at, NOW + 150_000);
    towermd_core::snapshot_service::maintain_main(&mut hub, "host", NOW + 150_000).unwrap();
    assert!(hub.leases(NOW + 150_000).is_empty());
}
