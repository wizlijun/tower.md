use serde_json::{json, Value};
use towermd_core::hub::{Event, Hub};
use towermd_core::hub_protocol::{AcquireLease, HubProtocol, NewEvent, RenewLease};
use towermd_core::snapshot::Snapshot;

fn reduce(events: &[Event]) -> Result<Value, String> {
    Ok(
        json!({"tasks": events.iter().filter(|e| !e.stale && e.kind == "task.created")
        .map(|e| e.obj.clone()).collect::<Vec<_>>()}),
    )
}

#[test]
fn contiguous_prefix_and_heartbeat_ranges_are_exact_and_projection_is_checked() {
    let temp = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temp.path()).unwrap();
    let grant = hub
        .lease_acquire(
            AcquireLease {
                host: "local".into(),
                names: vec!["main".into()],
                holder: "daemon".into(),
                ttl_ms: 120_000,
            },
            1000,
        )
        .unwrap();
    let fences = grant.leases.iter().map(|l| l.fence()).collect::<Vec<_>>();
    hub.lease_renew(
        RenewLease {
            host: "local".into(),
            fences: fences.clone(),
            ttl_ms: 120_000,
        },
        2000,
    )
    .unwrap();
    hub.lease_renew(
        RenewLease {
            host: "local".into(),
            fences,
            ttl_ms: 120_000,
        },
        3000,
    )
    .unwrap();
    hub.append(
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "local".into(),
            actor: "process:towermd-cli".into(),
            kind: "task.created".into(),
            layer: "L4".into(),
            obj: "T-1".into(),
            data: json!({}),
            guards: vec![],
        },
        4000,
    )
    .unwrap();
    let snapshot = Snapshot::build("project", None, &hub.events(0), 4, 1, 1, reduce).unwrap();
    assert_eq!(snapshot.events.len(), 2);
    assert_eq!(snapshot.state.omitted_heartbeats.len(), 1);
    assert_eq!(snapshot.state.omitted_heartbeats[0].first, 2);
    assert_eq!(snapshot.state.omitted_heartbeats[0].last, 3);
    assert_eq!(snapshot.receipts().len(), 2);
    let files = snapshot.files("docs/tower").unwrap();
    let state: Value =
        serde_json::from_slice(files["docs/tower/state/state.json"].as_ref().unwrap()).unwrap();
    assert!(state.get("receipts").is_none());
    assert!(state.get("events").is_none());
    let mut tampered = snapshot.clone();
    tampered.state.projection = json!({"tasks":["forged"]});
    assert!(tampered.validate(reduce).is_err());
    tampered = snapshot.clone();
    tampered.state.omitted_heartbeats[0].last = 4;
    assert!(tampered.validate(reduce).is_err());
    let all = hub.events(0);
    assert!(Snapshot::build(
        "project",
        None,
        &[all[0].clone(), all[3].clone()],
        4,
        1,
        1,
        reduce
    )
    .is_err());
    assert!(Snapshot::build("project", None, &all[..3], 4, 1, 1, reduce).is_err());
    // A received later event cannot move a snapshot beyond the supplied continuous watermark.
    let prefix = Snapshot::build("project", None, &all, 3, 1, 1, reduce).unwrap();
    assert_eq!(prefix.state.projection, json!({"tasks":[]}));
}

#[test]
fn recovery_skips_epochs_and_sequences_without_duplicating_business_events() {
    let empty = Snapshot::build("p", None, &[], 0, 0, 0, reduce).unwrap();
    let recovered = empty.recovery_boundary(1 << 32, 1 << 32, reduce).unwrap();
    assert!(recovered.events.is_empty());
    assert!(recovered.receipts().is_empty());
    assert_eq!(recovered.state.projection, empty.state.projection);
    assert!(empty
        .recovery_boundary((1 << 32) - 1, 1 << 32, reduce)
        .is_err());
    let twice = recovered
        .recovery_boundary(2 << 32, 2 << 32, reduce)
        .unwrap();
    assert_eq!(twice.state.recovery_jumps.len(), 2);
    twice.validate(reduce).unwrap();
}

#[test]
fn recovery_completion_is_bound_to_the_exact_archive_boundary() {
    let empty = Snapshot::build("p", None, &[], 0, 0, 0, reduce).unwrap();
    let boundary = empty.recovery_boundary(1 << 32, 1 << 32, reduce).unwrap();
    let epoch = (1 << 32) + 1;
    let event: Event = serde_json::from_value(json!({
        "seq": (1u64 << 32) + 1, "eid": uuid::Uuid::now_v7().to_string(),
        "host": "host", "actor": "process:towermd-recovery",
        "type": "recovery.completed", "layer": "L5", "obj": "p",
        "data": boundary.state.recovery_jumps[0],
        "guards": [{"name":"main","holder":"daemon","epoch":epoch}],
        "epoch":epoch,"stale":false,"t":"2026-10-21T00:00:00Z","cost_usd":null,
    }))
    .unwrap();
    let snapshot = Snapshot::build(
        "p",
        Some(&boundary),
        &[event.clone()],
        event.seq,
        epoch,
        epoch,
        reduce,
    )
    .unwrap();
    let mut wrong_actor = snapshot.clone();
    wrong_actor.events[0].actor = "process:towermd-cli".into();
    assert!(wrong_actor.validate(reduce).is_err());
    let mut wrong_data = snapshot.clone();
    wrong_data.events[0].data["source_seq"] = json!(1);
    assert!(wrong_data.validate(reduce).is_err());
    let mut expired = snapshot.clone();
    expired.events[0].guards[0].epoch = 1;
    expired.events[0].epoch = Some(1);
    assert!(expired.validate(reduce).is_err());
    let mut duplicated = event.clone();
    duplicated.seq += 1;
    duplicated.eid = uuid::Uuid::now_v7().to_string();
    assert!(Snapshot::build(
        "p",
        Some(&snapshot),
        &[duplicated.clone()],
        duplicated.seq,
        epoch,
        epoch,
        reduce
    )
    .is_err());
}

#[test]
fn git_roundtrip_checks_original_archive_not_only_the_projection() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    let h = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(h.path()).unwrap();
    hub.append(
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "local".into(),
            actor: "process:towermd-cli".into(),
            kind: "task.created".into(),
            layer: "L4".into(),
            obj: "T-1".into(),
            data: json!({}),
            guards: vec![],
        },
        1000,
    )
    .unwrap();
    let snapshot = Snapshot::build("p", None, &hub.events(0), 1, 0, 0, reduce).unwrap();
    for (path, bytes) in snapshot.files("docs/tower").unwrap() {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes.unwrap()).unwrap();
    }
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=QA",
        "-c",
        "user.email=qa@local",
        "commit",
        "-qm",
        "snapshot",
    ]);
    let loaded = Snapshot::read(root, "HEAD", "docs/tower", "p", reduce).unwrap();
    assert_eq!(loaded.state, snapshot.state);
    assert_eq!(loaded.events, snapshot.events);
    assert!(Snapshot::read(root, "HEAD", "docs/tower", "other", reduce).is_err());
    let archive = root.join("docs/tower/state/events/1970-01.jsonl");
    std::fs::write(archive, b"").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=QA",
        "-c",
        "user.email=qa@local",
        "commit",
        "-qm",
        "tampered",
    ]);
    assert!(Snapshot::read(root, "HEAD", "docs/tower", "p", reduce).is_err());
}
