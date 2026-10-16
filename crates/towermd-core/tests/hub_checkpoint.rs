use std::fs;

use serde_json::{json, Value};
use towermd_core::hub::{
    derived_eid, AppendReceipt, Fence, Hub, LeaseChange, NewEvent, SnapshotAck,
};
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
        fence: None,
    }
}

fn grant(hub: &mut Hub, name: &str, holder: &str, now: u64) -> Fence {
    hub.append(
        input("lease.acquired", json!({"lease": name})),
        Some(LeaseChange::Acquire {
            names: vec![name.into()],
            holder: holder.into(),
            ttl_ms: 100,
        }),
        now,
    )
    .unwrap()
    .updated_leases()[0]
        .fence()
}

fn ack(seq: u64, epoch: u64) -> SnapshotAck {
    SnapshotAck {
        commit: "a".repeat(40),
        seq,
        epoch,
    }
}

#[test]
fn checkpoint_uses_current_main_fence_and_hub_watermark() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let writer = grant(&mut hub, "main", "writer", 1);
    hub.append(
        input(
            "task.state",
            json!({"task": {"type":"Task", "id":"T-1", "state":"ready", "future":null}}),
        ),
        None,
        2,
    )
    .unwrap();
    let first = hub.checkpoint("project", &writer, 2).unwrap();
    hub.append(
        input("lease.renewed", json!({"lease":"main"})),
        Some(LeaseChange::Renew {
            fences: vec![writer.clone()],
            ttl_ms: 100,
        }),
        3,
    )
    .unwrap();
    let checkpoint = hub.checkpoint("project", &writer, 3).unwrap();
    assert_eq!(first.through_seq, 2);
    assert_eq!(checkpoint.through_seq, 3);
    assert_eq!(checkpoint.receipts, hub.receipts());
    assert_eq!(checkpoint.epoch_high_water, hub.epoch_high_water());
    assert_eq!(
        checkpoint.projection.objects["Task"]["T-1"]["future"],
        Value::Null
    );
    checkpoint
        .validate_archive("project", &archive_events(&hub.events(0), 0, 3).unwrap())
        .unwrap();
    assert_eq!(hub.last_seq(), 3);
}

#[test]
fn checkpoint_rejects_module_wrong_holder_expired_and_replaced_fences() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let writer = grant(&mut hub, "main", "writer", 1);
    let module = grant(&mut hub, "module:A", "writer", 2);
    assert!(hub.checkpoint("project", &module, 2).is_err());
    let mut wrong = writer.clone();
    wrong.holder = "imposter".into();
    assert!(hub.checkpoint("project", &wrong, 2).is_err());
    wrong = writer.clone();
    wrong.epoch = module.epoch;
    assert!(hub.checkpoint("project", &wrong, 2).is_err());
    assert!(hub.checkpoint("project", &writer, 101).is_err());
    let next = grant(&mut hub, "main", "next", 101);
    assert!(hub.checkpoint("project", &writer, 101).is_err());
    assert_eq!(
        hub.checkpoint("project", &next, 101).unwrap().writer_epoch,
        next.epoch
    );
}

#[test]
fn ack_persists_without_event_allocation_or_compaction_and_accepts_predecessor_epoch() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let old = grant(&mut hub, "main", "old", 1);
    let new = grant(&mut hub, "main", "new", 101);
    let events = hub.events(0);
    let receipts = hub.receipts();
    let previous = ack(1, old.epoch);
    hub.acknowledge(previous.clone()).unwrap();
    hub.acknowledge(previous.clone()).unwrap();
    assert_eq!(hub.snapshot_ack(), Some(&previous));
    assert_eq!(hub.events(0), events);
    assert_eq!(hub.receipts(), receipts);
    assert_eq!(hub.epoch_high_water(), new.epoch);
    drop(hub);
    let mut hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.snapshot_ack(), Some(&previous));
    assert_eq!(hub.events(0), events);
    assert_eq!(hub.receipts(), receipts);
    hub.append(input("future.event", json!({})), None, 102)
        .unwrap();
    drop(hub);
    let mut hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.snapshot_ack(), Some(&previous));
    let latest = SnapshotAck {
        commit: "B".repeat(64),
        seq: 3,
        epoch: new.epoch,
    };
    hub.acknowledge(latest.clone()).unwrap();
    assert_eq!(hub.snapshot_ack(), Some(&latest));
    assert!(hub.acknowledge(previous).is_err());
    assert_eq!(hub.last_seq(), 3);
}

#[test]
fn ack_rejects_bad_hash_epoch_and_unconfirmed_sequence_without_mutating_disk() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    grant(&mut hub, "main", "writer", 1);
    hub.acknowledge(ack(1, 1)).unwrap();
    let original = fs::read(temporary.path().join("journal.json")).unwrap();
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
        assert!(hub.acknowledge(candidate).is_err());
        assert_eq!(hub.snapshot_ack(), Some(&ack(1, 1)));
        assert_eq!(
            fs::read(temporary.path().join("journal.json")).unwrap(),
            original
        );
    }
}

#[test]
fn failed_ack_rename_preserves_memory_and_allows_retry() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    grant(&mut hub, "main", "writer", 1);
    hub.acknowledge(ack(1, 1)).unwrap();
    hub.append(input("future.event", json!({})), None, 2)
        .unwrap();
    let receipts = hub.receipts();
    let journal = temporary.path().join("journal.json");
    let saved = temporary.path().join("saved.json");
    fs::rename(&journal, &saved).unwrap();
    fs::create_dir(&journal).unwrap();
    assert!(hub.acknowledge(ack(2, 1)).is_err());
    assert_eq!(hub.snapshot_ack(), Some(&ack(1, 1)));
    assert_eq!(hub.receipts(), receipts);
    assert!(!fs::read_dir(temporary.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
    fs::remove_dir(&journal).unwrap();
    fs::rename(&saved, &journal).unwrap();
    hub.acknowledge(ack(2, 1)).unwrap();
    drop(hub);
    assert_eq!(
        Hub::open(temporary.path()).unwrap().snapshot_ack(),
        Some(&ack(2, 1))
    );
}

#[test]
fn version_one_without_ack_opens_and_invalid_persisted_ack_is_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    grant(&mut hub, "main", "writer", 1);
    drop(hub);
    let journal = temporary.path().join("journal.json");
    let mut old: Value = serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
    old.as_object_mut().unwrap().remove("snapshot_ack");
    fs::write(&journal, serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(Hub::open(temporary.path())
        .unwrap()
        .snapshot_ack()
        .is_none());
    for invalid in [
        ack(2, 1),
        ack(1, 0),
        ack(1, 2),
        SnapshotAck {
            commit: "invalid".into(),
            ..ack(1, 1)
        },
    ] {
        let mut damaged = old.clone();
        damaged["snapshot_ack"] = json!(invalid);
        let bytes = serde_json::to_vec(&damaged).unwrap();
        fs::write(&journal, &bytes).unwrap();
        assert!(Hub::open(temporary.path()).is_err());
        assert_eq!(fs::read(&journal).unwrap(), bytes);
    }
}

#[test]
fn epoch_high_water_and_receipts_survive_stale_events_release_and_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let fence = grant(&mut hub, "main", "writer", 1);
    hub.append(
        input("lease.released", json!({})),
        Some(LeaseChange::Release {
            fences: vec![fence],
        }),
        2,
    )
    .unwrap();
    let mut stale = input("future.event", json!({}));
    stale.fence = Some(Fence {
        name: "main".into(),
        holder: "old".into(),
        epoch: u64::MAX,
    });
    hub.append(stale, None, 3).unwrap();
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
fn derived_events_require_matching_commit_kind_and_valid_actor_even_on_duplicate() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let commit = "a".repeat(40);
    let mut event = input("snapshot.written", json!({"commit": commit}));
    event.eid = derived_eid(&commit, &event.kind).unwrap();
    let accepted = hub.append(event.clone(), None, 1).unwrap();
    assert_eq!(hub.append(event.clone(), None, 2).unwrap(), accepted);
    for field in ["commit", "kind", "actor", "missing", "eid"] {
        let mut invalid = event.clone();
        match field {
            "commit" => invalid.data["commit"] = json!("b".repeat(40)),
            "kind" => invalid.kind = "baseline.created".into(),
            "actor" => invalid.actor = "invalid".into(),
            "missing" => invalid.data = json!({}),
            _ => invalid.eid = format!("git:{}", "0".repeat(64)),
        }
        assert!(hub.append(invalid, None, 3).is_err(), "{field}");
    }
    assert_eq!(hub.last_seq(), 1);
    drop(hub);
    let mut hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.append(event, None, 3).unwrap(), accepted);
    let path = temporary.path().join("journal.json");
    drop(hub);
    let mut damaged: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    damaged["events"][0]["data"]["commit"] = json!("b".repeat(40));
    fs::write(&path, serde_json::to_vec(&damaged).unwrap()).unwrap();
    assert!(Hub::open(temporary.path()).is_err());
}

#[test]
fn receipt_append_retains_full_events_and_existing_idempotency_without_seed() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let event = input("future.event", json!({"unknown": null}));
    let AppendReceipt::Event(recorded) = hub.append_receipt(event.clone(), None, 1).unwrap() else {
        panic!("new event must have a body");
    };
    assert_eq!(
        hub.append_receipt(event.clone(), None, 2).unwrap(),
        AppendReceipt::Event(recorded.clone())
    );
    assert_eq!(hub.append(event, None, 2).unwrap(), *recorded);
    assert_eq!(hub.last_seq(), 1);
    assert!(hub.base_snapshot().is_none());
    drop(hub);
    let hub = Hub::open(temporary.path()).unwrap();
    assert_eq!(hub.events(0), vec![*recorded]);
    assert!(hub.base_snapshot().is_none());
}
