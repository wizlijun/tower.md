mod support;
use serde_json::{json, Value};
use std::fs;
use towermd_core::events::derived_eid;
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
use towermd_core::snapshot::archive_events;

fn input(kind: &str, data: Value) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:test".into(),
        kind: kind.into(),
        layer: "L5".into(),
        obj: "project".into(),
        data,
        guards: vec![],
    }
}
fn grant(hub: &mut dyn HubProtocol, name: &str, holder: &str, now: u64) -> Fence {
    hub.lease_acquire(
        AcquireLease {
            host: "host".into(),
            names: vec![name.into()],
            holder: holder.into(),
            ttl_ms: 100,
        },
        now,
    )
    .unwrap()
    .leases[0]
        .fence()
}
fn ack(seq: u64, epoch: u64) -> SnapshotAck {
    SnapshotAck {
        commit: "a".repeat(40),
        seq,
        epoch,
    }
}
fn recorded(receipt: AppendReceipt) -> Event {
    match receipt {
        AppendReceipt::Event(event) => *event,
        AppendReceipt::Duplicate { .. } => panic!("event body expected"),
    }
}
fn read_lines(path: &std::path::Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn write_lines(path: &std::path::Path, lines: &[Value]) -> Vec<u8> {
    let bytes = lines
        .iter()
        .map(|line| format!("{}\n", serde_json::to_string(line).unwrap()))
        .collect::<String>()
        .into_bytes();
    fs::write(path, &bytes).unwrap();
    bytes
}

#[test]
fn checkpoint_uses_current_main_fence_and_hub_watermark() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = support::catalog(temporary.path());
    let mut session = support::open(&temporary.path().join("session-hub"), &catalog, 0);
    let writer = grant(session.protocol().unwrap(), "main", "writer", 1);
    session
        .protocol().unwrap()
        .append(
            input(
                "task.state",
                json!({"task": {"type":"Task", "id":"T-1", "title":"Example", "module":"A", "state":"ready", "future":null}}),
            ),
            2,
        )
        .unwrap();
    session = support::sync(session, &catalog, 2);
    let first = session.checkpoint("project", &writer, 2).unwrap();
    session
        .protocol()
        .unwrap()
        .lease_renew(
            RenewLease {
                host: "host".into(),
                fences: vec![writer.clone()],
                ttl_ms: 100,
            },
            3,
        )
        .unwrap();
    session = support::sync(session, &catalog, 3);
    let checkpoint = session.checkpoint("project", &writer, 3).unwrap();
    assert_eq!(first.through_seq, 2);
    assert_eq!(checkpoint.through_seq, 3);
    assert!(serde_json::to_value(&checkpoint)
        .unwrap()
        .get("receipts")
        .is_none());
    assert_eq!(checkpoint.epoch_high_water, session.epoch_high_water());
    assert_eq!(
        checkpoint.projection.objects["Task"]["T-1"]["future"],
        Value::Null
    );
    checkpoint
        .validate_archive(
            "project",
            &archive_events(&session.events(0), 0, 3).unwrap(),
        )
        .unwrap();
    assert_eq!(session.last_seq(), 3);
}

#[test]
fn checkpoint_rejects_module_wrong_holder_expired_and_replaced_fences() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = support::catalog(temporary.path());
    let mut session = support::open(&temporary.path().join("session-hub"), &catalog, 0);
    let writer = grant(session.protocol().unwrap(), "main", "writer", 1);
    let module = grant(session.protocol().unwrap(), "module:A", "writer", 2);
    session = support::sync(session, &catalog, 2);
    assert!(session.checkpoint("project", &module, 2).is_err());
    let mut wrong = writer.clone();
    wrong.holder = "imposter".into();
    assert!(session.checkpoint("project", &wrong, 2).is_err());
    wrong = writer.clone();
    wrong.epoch = module.epoch;
    assert!(session.checkpoint("project", &wrong, 2).is_err());
    assert!(session.checkpoint("project", &writer, 101).is_err());
    let next = grant(session.protocol().unwrap(), "main", "next", 101);
    session = support::sync(session, &catalog, 101);
    assert!(session.checkpoint("project", &writer, 101).is_err());
    assert_eq!(
        session
            .checkpoint("project", &next, 101)
            .unwrap()
            .writer_epoch,
        next.epoch
    );
}

#[test]
fn ack_compacts_without_event_allocation_and_accepts_predecessor_epoch() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let old = grant(&mut hub, "main", "old", 1);
    let new = grant(&mut hub, "main", "new", 101);
    let events = hub.events(0);
    let receipts = hub.receipts();
    let previous = ack(1, old.epoch);
    let checkpoint = json!(["opaque", null]);
    HubProtocol::snapshot_ack(&mut hub, previous.clone(), checkpoint.clone()).unwrap();
    let journal = temporary.path().join("journal.jsonl");
    let bytes = fs::read(&journal).unwrap();
    HubProtocol::snapshot_ack(&mut hub, previous.clone(), checkpoint.clone()).unwrap();
    assert_eq!(fs::read(&journal).unwrap(), bytes);
    assert_eq!(hub.snapshot_ack(), Some(&previous));
    assert_eq!(hub.events(1), events[1..]);
    assert_eq!(hub.receipts(), receipts);
    assert_eq!(hub.epoch_high_water(), new.epoch);
    assert_eq!(hub.last_seq(), 2);
    assert!(
        matches!(hub.subscribe(0, 101), Err(HubError::SnapshotRequired { checkpoint: found, .. }) if found == checkpoint)
    );
    let lines = read_lines(&journal);
    assert_eq!(lines[0]["version"], 2);
    assert_eq!(lines[0]["checkpoint"]["checkpoint"], checkpoint);
    assert_eq!(lines[0]["checkpoint"]["leases"]["main"]["holder"], "old");
    assert_eq!(lines[1]["type"], "lease.granted");
    drop(hub);
    let mut hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.snapshot_ack(), Some(&previous));
    assert_eq!(hub.events(1), events[1..]);
    assert_eq!(hub.receipts(), receipts);
    hub.append(input("future.event", json!({})), 102).unwrap();
    drop(hub);
    let mut hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.snapshot_ack(), Some(&previous));
    let latest = SnapshotAck {
        commit: "B".repeat(64),
        seq: 3,
        epoch: new.epoch,
    };
    HubProtocol::snapshot_ack(&mut hub, latest.clone(), json!({"new":true})).unwrap();
    assert_eq!(hub.snapshot_ack(), Some(&latest));
    assert!(HubProtocol::snapshot_ack(&mut hub, previous, checkpoint).is_err());
    assert_eq!(hub.last_seq(), 3);
}

#[test]
fn ack_rejects_bad_hash_epoch_and_unconfirmed_sequence_without_mutating_disk() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    grant(&mut hub, "main", "writer", 1);
    HubProtocol::snapshot_ack(&mut hub, ack(1, 1), json!([null])).unwrap();
    let journal = temporary.path().join("journal.jsonl");
    let original = fs::read(&journal).unwrap();
    let mut invalid = vec![ack(2, 1), ack(1, 0), ack(1, 2), ack(0, 1)];
    for commit in [
        "a".repeat(39),
        "g".repeat(40),
        "a".repeat(65),
        String::new(),
    ] {
        invalid.push(SnapshotAck {
            commit,
            ..ack(1, 1)
        });
    }
    for candidate in invalid {
        assert!(HubProtocol::snapshot_ack(&mut hub, candidate, json!([null])).is_err());
        assert_eq!(hub.snapshot_ack(), Some(&ack(1, 1)));
        assert_eq!(fs::read(&journal).unwrap(), original);
    }
    assert!(HubProtocol::snapshot_ack(&mut hub, ack(1, 1), json!(["changed"])).is_err());
    assert_eq!(fs::read(&journal).unwrap(), original);
}

#[test]
fn failed_ack_rename_preserves_memory_and_allows_retry() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    grant(&mut hub, "main", "writer", 1);
    HubProtocol::snapshot_ack(&mut hub, ack(1, 1), json!(null)).unwrap();
    hub.append(input("future.event", json!({})), 2).unwrap();
    let receipts = hub.receipts();
    let tail = hub.events(1);
    let journal = temporary.path().join("journal.jsonl");
    let saved = temporary.path().join("saved.jsonl");
    let bytes = fs::read(&journal).unwrap();
    fs::rename(&journal, &saved).unwrap();
    fs::create_dir(&journal).unwrap();
    assert!(HubProtocol::snapshot_ack(&mut hub, ack(2, 1), json!(17)).is_err());
    assert_eq!(hub.snapshot_ack(), Some(&ack(1, 1)));
    assert_eq!(hub.receipts(), receipts);
    assert_eq!(hub.events(1), tail);
    assert_eq!(fs::read(&saved).unwrap(), bytes);
    assert!(!fs::read_dir(temporary.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
    fs::remove_dir(&journal).unwrap();
    fs::rename(&saved, &journal).unwrap();
    HubProtocol::snapshot_ack(&mut hub, ack(2, 1), json!(17)).unwrap();
    drop(hub);
    assert_eq!(
        Hub::open(temporary.path()).unwrap().snapshot_ack(),
        Some(&ack(2, 1))
    );
}

fn legacy_grant() -> Value {
    json!({"version":1,"events":[{
        "seq":1,"eid":uuid::Uuid::now_v7().to_string(),"host":"host","t":"1970-01-01T00:00:00.001Z",
        "actor":"process:test","type":"lease.acquired","layer":"L4","obj":"project","data":{"lease":"main"},
        "epoch":null,"stale":false,"cost_usd":null,"fence":null,
        "lease_delta":{"change":{"Acquire":{"names":["main"],"holder":"writer","ttl_ms":100}},
            "updated":[{"name":"main","holder":"writer","epoch":1,"expires_at":101}]}
    }]})
}

#[test]
fn version_one_without_ack_opens_and_invalid_persisted_ack_is_rejected() {
    let old = legacy_grant();
    let temporary = tempfile::tempdir().unwrap();
    let journal = temporary.path().join("journal.json");
    let original = serde_json::to_vec(&old).unwrap();
    fs::write(&journal, &original).unwrap();
    let hub = Hub::open(temporary.path()).unwrap();
    assert!(hub.snapshot_ack().is_none());
    assert_eq!(hub.events(0)[0].layer, "L4");
    assert_eq!(hub.events(0)[0].seq, 1);
    assert_eq!(hub.events(0)[0].eid, old["events"][0]["eid"]);
    assert_eq!(fs::read(&journal).unwrap(), original);
    assert!(temporary.path().join("journal.jsonl").exists());
    drop(hub);
    // Invalid v1 fixtures must each migrate in a fresh directory: after v2 exists,
    // it is authoritative and the old file is intentionally ignored.
    for invalid in [
        ack(2, 1),
        ack(1, 0),
        ack(1, 2),
        SnapshotAck {
            commit: "invalid".into(),
            ..ack(1, 1)
        },
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let journal = temporary.path().join("journal.json");
        let mut damaged = old.clone();
        damaged["snapshot_ack"] = json!(invalid);
        let bytes = serde_json::to_vec(&damaged).unwrap();
        fs::write(&journal, &bytes).unwrap();
        assert!(Hub::open(temporary.path()).is_err());
        assert_eq!(fs::read(&journal).unwrap(), bytes);
        assert!(!temporary.path().join("journal.jsonl").exists());
    }
}

#[test]
fn invalid_persisted_jsonl_ack_is_rejected_without_reset() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    grant(&mut hub, "main", "writer", 1);
    HubProtocol::snapshot_ack(&mut hub, ack(1, 1), json!(["opaque"])).unwrap();
    drop(hub);
    let path = temporary.path().join("journal.jsonl");
    let original = read_lines(&path);
    for invalid in [
        ack(2, 1),
        ack(1, 0),
        ack(1, 2),
        SnapshotAck {
            commit: "invalid".into(),
            ..ack(1, 1)
        },
    ] {
        let mut damaged = original.clone();
        damaged[0]["checkpoint"]["ack"] = json!(invalid);
        let bytes = write_lines(&path, &damaged);
        assert!(Hub::open(temporary.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn epoch_high_water_and_receipts_survive_stale_events_release_and_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let fence = grant(&mut hub, "main", "writer", 1);
    hub.lease_release(
        ReleaseLease {
            host: "host".into(),
            fences: vec![fence],
        },
        2,
    )
    .unwrap();
    let mut stale = input("future.event", json!({}));
    stale.guards = vec![Fence {
        name: "main".into(),
        holder: "old".into(),
        epoch: u64::MAX,
    }];
    hub.append(stale, 3).unwrap();
    assert_eq!(hub.epoch_high_water(), 1);
    let receipts = hub.receipts();
    assert_eq!(receipts.len(), 3);
    for event in hub.events(0) {
        assert_eq!(receipts[&event.eid], event.seq);
    }
    drop(hub);
    let hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.epoch_high_water(), 1);
    assert_eq!(hub.receipts(), receipts);
}

#[test]
fn derived_ids_are_stable_domain_separated_and_validate_arguments() {
    let commit = "a".repeat(40);
    let eid = derived_eid(&commit, "snapshot.written").unwrap();
    assert_eq!(
        eid,
        "git:b3e9d7ac1855d4aab30f95f74476e1223e4fb823f352d0e7e2cb5c714f6e987c"
    );
    assert_eq!(eid, derived_eid(&commit, "snapshot.written").unwrap());
    assert_eq!(eid.len(), 68);
    assert!(eid.starts_with("git:"));
    assert_ne!(eid, derived_eid(&commit, "baseline.created").unwrap());
    assert_ne!(
        eid,
        derived_eid(&"b".repeat(40), "snapshot.written").unwrap()
    );
    assert!(derived_eid(&"A".repeat(64), "snapshot.written").is_ok());
    for kind in ["", "two words", "a\0b", "line\nbreak"] {
        assert!(derived_eid(&commit, kind).is_err());
    }
    for commit in ["abc".into(), "z".repeat(40), "a".repeat(41)] {
        assert!(derived_eid(&commit, "snapshot.written").is_err());
    }
}

#[test]
fn derived_events_are_opaque_and_envelope_validation_still_rejects_new_invalid_events() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let commit = "a".repeat(40);
    let mut event = input("snapshot.written", json!([null, {"commit":false}]));
    event.eid = derived_eid(&commit, &event.kind).unwrap();
    let accepted = hub.append(event.clone(), 1).unwrap();
    assert_eq!(hub.append(event.clone(), 2).unwrap(), accepted);
    let mut duplicate = event.clone();
    duplicate.kind = "future.event".into();
    duplicate.data = json!({"commit": "b".repeat(40)});
    assert_eq!(hub.append(duplicate, 3).unwrap(), accepted);
    for field in ["actor", "eid"] {
        let mut invalid = event.clone();
        invalid.eid = uuid::Uuid::now_v7().to_string();
        match field {
            "actor" => invalid.actor = "invalid".into(),
            _ => invalid.eid = "git:invalid".into(),
        }
        assert!(hub.append(invalid, 3).is_err(), "{field}");
    }
    assert_eq!(hub.last_seq(), 1);
    drop(hub);
    let mut hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.append(event, 3).unwrap(), accepted);
    drop(hub);
    // Physical v2 data remains opaque even on reopen; binding commit to a
    // derived eid belongs to the caller, with v1 strictness retained below.
    let path = temporary.path().join("journal.jsonl");
    let mut lines = read_lines(&path);
    lines[1]["data"] = json!({"commit":"b".repeat(40)});
    write_lines(&path, &lines);
    let hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.events(0)[0].data, lines[1]["data"]);
}

#[test]
fn legacy_derived_event_requires_matching_commit_and_kind() {
    let commit = "a".repeat(40);
    let mut old = legacy_grant();
    old["events"][0]["type"] = json!("snapshot.written");
    old["events"][0]["eid"] = json!(derived_eid(&commit, "snapshot.written").unwrap());
    old["events"][0]["data"] = json!({"commit":commit});
    old["events"][0]["lease_delta"] = Value::Null;
    for field in ["valid", "commit", "kind", "actor", "missing", "eid"] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("journal.json");
        let mut fixture = old.clone();
        match field {
            "commit" => fixture["events"][0]["data"]["commit"] = json!("b".repeat(40)),
            "kind" => fixture["events"][0]["type"] = json!("baseline.created"),
            "actor" => fixture["events"][0]["actor"] = json!("invalid"),
            "missing" => fixture["events"][0]["data"] = json!({}),
            "eid" => fixture["events"][0]["eid"] = json!(format!("git:{}", "0".repeat(64))),
            _ => {}
        }
        let bytes = serde_json::to_vec(&fixture).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            Hub::open(temporary.path()).is_ok(),
            field == "valid",
            "{field}"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn receipt_append_retains_full_events_and_existing_idempotency_without_seed() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let event = input("future.event", json!({"unknown": null}));
    let recorded = recorded(hub.append(event.clone(), 1).unwrap());
    assert_eq!(
        hub.append(event.clone(), 2).unwrap(),
        AppendReceipt::Event(Box::new(recorded.clone()))
    );
    assert_eq!(
        hub.append(event, 2).unwrap(),
        AppendReceipt::Event(Box::new(recorded.clone()))
    );
    assert_eq!(hub.last_seq(), 1);
    assert_eq!(hub.subscribe(0, 2).unwrap().head.floor_seq, 0);
    drop(hub);
    let mut hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.events(0), vec![recorded]);
    assert_eq!(hub.subscribe(0, 2).unwrap().head.floor_seq, 0);
}

#[test]
fn legacy_grant_ttl_mismatch_is_rejected_without_rewriting_history() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("journal.json");
    let mut old = legacy_grant();
    old["events"][0]["lease_delta"]["updated"][0]["expires_at"] = json!(999);
    let bytes = serde_json::to_vec(&old).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(Hub::open(temporary.path()).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(!temporary.path().join("journal.jsonl").exists());
}
