use serde_json::json;
use std::fs;
use tempfile::TempDir;
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
use uuid::Uuid;

fn input() -> NewEvent {
    NewEvent {
        eid: Uuid::now_v7().to_string(),
        host: "host-a".into(),
        actor: "process:towermd-cli".into(),
        kind: "task.claimed".into(),
        layer: "L5".into(),
        obj: "T-example".into(),
        data: json!({"run": "run-a"}),
        guards: vec![],
    }
}

fn acquire(names: &[&str], holder: &str, ttl_ms: u64) -> AcquireLease {
    AcquireLease {
        host: "host-a".into(),
        names: names.iter().map(|name| (*name).into()).collect(),
        holder: holder.into(),
        ttl_ms,
    }
}
fn recorded(receipt: AppendReceipt) -> Event {
    match receipt {
        AppendReceipt::Event(event) => *event,
        AppendReceipt::Duplicate { .. } => panic!("event body expected"),
    }
}
fn renew(fences: Vec<Fence>, ttl_ms: u64) -> RenewLease {
    RenewLease {
        host: "host-a".into(),
        fences,
        ttl_ms,
    }
}
fn release(fences: Vec<Fence>) -> ReleaseLease {
    ReleaseLease {
        host: "host-a".into(),
        fences,
    }
}

#[test]
fn append_restart_and_duplicate_return_original_without_any_changes() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    let granted = hub
        .lease_acquire(acquire(&["module:a"], "run-a", 100), 1000)
        .unwrap();
    let original = input();
    let event = recorded(hub.append(original.clone(), 1000).unwrap());
    // The independent grant occupies seq 1; the client event occupies seq 2.
    assert_eq!(event.seq, 2);
    assert_eq!(event.cost_usd, None);
    assert!(!event.stale);
    assert_eq!(
        serde_json::to_value(&event).unwrap()["type"],
        "task.claimed"
    );
    let leases = hub.leases(1000);
    drop(hub);
    let mut hub = Hub::open(dir.path()).unwrap();
    let mut duplicate = original;
    duplicate.actor.clear();
    assert_eq!(
        hub.append(duplicate, u64::MAX).unwrap(),
        AppendReceipt::Event(Box::new(event.clone()))
    );
    assert_eq!(hub.events(0), vec![granted.event, event]);
    assert!(hub.events(2).is_empty());
    assert_eq!(hub.last_seq(), 2);
    assert_eq!(hub.leases(1000), leases);
}

#[test]
fn acquisition_is_atomic_and_epochs_are_global_and_survive_release_and_restart() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.lease_acquire(acquire(&["b"], "owner", 100), 10)
        .unwrap();
    assert!(hub
        .lease_acquire(acquire(&["a", "b"], "other", 100), 11)
        .is_err());
    assert_eq!(hub.last_seq(), 1);
    assert_eq!(hub.leases(11).len(), 1);
    let first = hub.leases(11)["b"].fence();
    hub.lease_release(release(vec![first.clone()]), 12).unwrap();
    drop(hub);
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.lease_acquire(acquire(&["a", "b"], "other", 100), 13)
        .unwrap();
    let leases = hub.leases(13);
    assert!(leases["a"].epoch > first.epoch);
    assert!(leases["b"].epoch > leases["a"].epoch);
}

#[test]
fn stale_fence_is_recorded_without_mutating_current_leases() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.lease_acquire(acquire(&["a"], "owner", 10), 10).unwrap();
    let old = hub.leases(10)["a"].fence();
    hub.lease_acquire(acquire(&["a"], "new-owner", 100), 20)
        .unwrap();
    let current = hub.leases(20);
    let mut stale = input();
    stale.guards = vec![old.clone()];
    let event = recorded(hub.append(stale, 21).unwrap());
    assert!(event.stale);
    assert_eq!(event.epoch, Some(old.epoch));
    assert_eq!(hub.leases(21), current);
    // Release is now a separate operation: an old fence must be rejected atomically.
    assert!(hub.lease_release(release(vec![old.clone()]), 22).is_err());
    let mut stale = input();
    stale.guards = vec![old];
    assert!(recorded(hub.append(stale, 22).unwrap()).stale);
    drop(hub);
    let hub = Hub::open(dir.path()).unwrap();
    assert!(!hub.events(0)[0].stale);
    assert!(hub.events(0)[2].stale);
    assert_eq!(hub.leases(21), current);
}

#[test]
fn renew_is_atomic_durable_and_expired_lease_cannot_resurrect() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.lease_acquire(acquire(&["a", "b"], "owner", 100), 10)
        .unwrap();
    let fences: Vec<_> = hub.leases(10).values().map(Lease::fence).collect();
    let mut invalid = fences.clone();
    invalid[1].holder = "imposter".into();
    assert!(hub.lease_renew(renew(invalid.clone(), 200), 20).is_err());
    assert!(hub.lease_release(release(invalid), 20).is_err());
    assert_eq!(hub.leases(20)["a"].expires_at, 110);
    assert_eq!(hub.last_seq(), 1);
    hub.lease_renew(renew(fences.clone(), 200), 20).unwrap();
    drop(hub);
    let mut hub = Hub::open(dir.path()).unwrap();
    assert_eq!(hub.leases(219)["a"].expires_at, 220);
    assert!(hub.leases(220).is_empty());
    assert!(hub.lease_renew(renew(fences.clone(), 100), 220).is_err());
    assert!(hub.lease_release(release(fences), 220).is_err());
    assert_eq!(hub.last_seq(), 2);
}

#[test]
fn expired_release_without_input_fence_cannot_damage_new_holder() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.lease_acquire(acquire(&["a"], "old", 10), 10).unwrap();
    let fence = hub.leases(10)["a"].fence();
    hub.lease_acquire(acquire(&["a"], "new", 10), 20).unwrap();
    assert!(hub.lease_release(release(vec![fence]), 21).is_err());
    assert_eq!(hub.leases(21)["a"].holder, "new");
    assert_eq!(hub.last_seq(), 2);
}

#[test]
fn current_fence_is_historical_even_after_expiry() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.lease_acquire(acquire(&["a"], "owner", 10), 10).unwrap();
    let mut event = input();
    event.guards = vec![hub.leases(10)["a"].fence()];
    let accepted = recorded(hub.append(event.clone(), 11).unwrap());
    assert!(!accepted.stale);
    event.eid = Uuid::now_v7().to_string();
    assert!(recorded(hub.append(event, 20).unwrap()).stale);
    drop(hub);
    let hub = Hub::open(dir.path()).unwrap();
    assert_eq!(hub.events(1)[0], accepted);
}

#[test]
fn malformed_envelopes_and_invalid_time_do_not_consume_sequence() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    for field in ["eid", "host", "actor", "kind", "layer", "obj"] {
        let mut event = input();
        match field {
            "eid" => event.eid = "not-a-uuid".into(),
            "host" => event.host.clear(),
            "actor" => event.actor = "human:".into(),
            "kind" => event.kind.clear(),
            "layer" => event.layer = "L6".into(),
            "obj" => event.obj.clear(),
            _ => unreachable!(),
        }
        assert!(hub.append(event, 10).is_err(), "{field}");
    }
    for actor in [
        "agent",
        "human:someone/1",
        "process:",
        "agent/",
        "/1",
        "a/b/c",
        "a /1",
    ] {
        let mut event = input();
        event.actor = actor.into();
        assert!(hub.append(event, 10).is_err(), "{actor}");
    }
    let mut event = input();
    event.eid = "00000000-0000-4000-8000-000000000000".into();
    assert!(hub.append(event, 10).is_err());
    assert!(hub.append(input(), u64::MAX).is_err());
    assert_eq!(hub.last_seq(), 0);
    for actor in ["human:bruce", "process:towermd-cli", "codex/1.0"] {
        let mut event = input();
        event.actor = actor.into();
        hub.append(event, 10).unwrap();
    }
    assert_eq!(hub.last_seq(), 3);
}

#[test]
fn invalid_lease_batches_and_overflow_are_rejected_atomically() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    for request in [
        acquire(&[], "owner", 10),
        acquire(&["a", "a"], "owner", 10),
        acquire(&[""], "owner", 10),
        acquire(&["a"], "", 10),
        acquire(&["a"], "owner", 0),
        acquire(&["a"], "owner", u64::MAX),
    ] {
        assert!(hub.lease_acquire(request, 10).is_err());
        assert_eq!(hub.last_seq(), 0);
        assert!(hub.leases(10).is_empty());
    }
    assert!(hub.lease_renew(renew(vec![], 10), 10).is_err());
    assert!(hub.lease_release(release(vec![]), 10).is_err());
    assert_eq!(hub.last_seq(), 0);
    assert!(hub.leases(10).is_empty());
}

#[test]
fn only_one_writer_holds_lock_until_drop() {
    let dir = TempDir::new().unwrap();
    let hub = Hub::open(dir.path()).unwrap();
    assert!(Hub::open(dir.path()).is_err());
    drop(hub);
    assert!(Hub::open(dir.path()).is_ok());
}

#[test]
fn accepted_events_are_durable_and_parent_path_changes_do_not_fake_append_failure() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("hub");
    let mut hub = Hub::open(&path).unwrap();
    let granted = hub
        .lease_acquire(acquire(&["a"], "owner", 100), 10)
        .unwrap();
    let accepted = recorded(hub.append(input(), 10).unwrap());
    let before = hub.leases(10);
    let saved = root.path().join("saved");
    fs::rename(&path, &saved).unwrap();
    fs::write(&path, "not a directory").unwrap();
    // Append retains its open descriptor. Actual partial-write/fsync failures and
    // poison/reopen behavior are covered by hub::tests using real I/O injection.
    let second = recorded(hub.append(input(), 11).unwrap());
    assert_eq!(
        hub.events(0),
        vec![granted.event.clone(), accepted.clone(), second.clone()]
    );
    assert_eq!(hub.leases(10), before);
    assert_eq!(hub.last_seq(), 3);
    fs::remove_file(&path).unwrap();
    fs::rename(&saved, &path).unwrap();
    drop(hub);
    let mut hub = Hub::open(&path).unwrap();
    assert_eq!(hub.events(0), vec![granted.event, accepted, second]);
    assert_eq!(recorded(hub.append(input(), 12).unwrap()).seq, 4);
}

#[test]
fn corrupt_journal_is_rejected_without_reset() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), 10).unwrap();
    drop(hub);
    let journal = dir.path().join("journal.jsonl");
    fs::write(&journal, b"{broken\n").unwrap();
    assert!(Hub::open(dir.path()).is_err());
    assert_eq!(fs::read(&journal).unwrap(), b"{broken\n");
}

#[test]
fn unknown_fence_is_stale_without_creating_a_lease() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    let mut event = input();
    event.guards = vec![Fence {
        name: "missing".into(),
        holder: "owner".into(),
        epoch: 1,
    }];
    assert!(recorded(hub.append(event, 10).unwrap()).stale);
    assert!(hub.leases(10).is_empty());
}

#[test]
fn valid_json_with_corrupt_sequence_epoch_or_stale_is_rejected() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.lease_acquire(acquire(&["a"], "owner", 100), 10)
        .unwrap();
    drop(hub);
    let path = dir.path().join("journal.jsonl");
    let original: Vec<serde_json::Value> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for field in [
        "seq",
        "stale",
        "epoch",
        "expires_at",
        "cost_usd",
        "type",
        "version",
        "duplicate",
    ] {
        let mut corrupt = original.clone();
        match field {
            "seq" => corrupt[1]["seq"] = json!(2),
            "stale" => corrupt[1]["stale"] = json!(true),
            "epoch" => corrupt[1]["lease_delta"]["Grant"]["leases"][0]["epoch"] = json!(99),
            "expires_at" => {
                corrupt[1]["lease_delta"]["Grant"]["leases"][0]["expires_at"] = json!(9)
            }
            "cost_usd" => corrupt[1]["cost_usd"] = json!(0.5),
            "type" => corrupt[1]["type"] = json!("lease.renewed"),
            "version" => corrupt[0]["version"] = json!(999),
            "duplicate" => {
                let mut duplicate = corrupt[1].clone();
                duplicate["seq"] = json!(2);
                corrupt.push(duplicate);
            }
            _ => unreachable!(),
        }
        let bytes = corrupt
            .iter()
            .map(|line| format!("{}\n", serde_json::to_string(line).unwrap()))
            .collect::<String>();
        fs::write(&path, &bytes).unwrap();
        assert!(Hub::open(dir.path()).is_err(), "{field}");
        assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
    }
}

#[test]
fn duplicate_fences_and_zero_ttl_do_not_mutate_leases() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.lease_acquire(acquire(&["a"], "owner", 100), 10)
        .unwrap();
    let before = hub.leases(10);
    let fence = before["a"].fence();
    for request in [
        renew(vec![fence.clone(), fence.clone()], 100),
        renew(vec![fence.clone()], 0),
        renew(vec![fence.clone()], u64::MAX),
    ] {
        assert!(hub.lease_renew(request, 11).is_err());
        assert_eq!(hub.leases(11), before);
        assert_eq!(hub.last_seq(), 1);
    }
    assert!(hub
        .lease_release(release(vec![fence.clone(), fence]), 11)
        .is_err());
    assert_eq!(hub.leases(11), before);
    assert_eq!(hub.last_seq(), 1);
}

#[test]
fn failed_rename_preserves_previous_journal_and_can_retry() {
    let root = TempDir::new().unwrap();
    let mut hub = Hub::open(root.path()).unwrap();
    hub.lease_acquire(acquire(&["main"], "writer", 100), 9)
        .unwrap();
    let first = recorded(hub.append(input(), 10).unwrap());
    let before = hub.events(0);
    let leases = hub.leases(11);
    let journal = root.path().join("journal.jsonl");
    let saved = root.path().join("saved.jsonl");
    let bytes = fs::read(&journal).unwrap();
    fs::rename(&journal, &saved).unwrap();
    fs::create_dir(&journal).unwrap();
    let ack = SnapshotAck {
        commit: "a".repeat(40),
        seq: first.seq,
        epoch: 1,
    };
    assert!(HubProtocol::snapshot_ack(&mut hub, ack.clone(), json!(["opaque"])).is_err());
    assert_eq!(hub.events(0), before);
    assert_eq!(hub.leases(11), leases);
    assert_eq!(hub.snapshot_ack(), None);
    assert_eq!(fs::read(&saved).unwrap(), bytes);
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
    fs::remove_dir(&journal).unwrap();
    fs::rename(&saved, &journal).unwrap();
    HubProtocol::snapshot_ack(&mut hub, ack.clone(), json!(["opaque"])).unwrap();
    let next = recorded(hub.append(input(), 12).unwrap());
    assert_eq!(next.seq, 3);
    drop(hub);
    let mut hub = Hub::open(root.path()).unwrap();
    assert_eq!(hub.snapshot_ack(), Some(&ack));
    assert_eq!(hub.events(2), vec![next]);
    assert!(matches!(
        hub.subscribe(1, 12),
        Err(HubError::SnapshotRequired { .. })
    ));
}

#[test]
fn updated_leases_exposes_atomic_grants_and_renewals_for_replay() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    let granted = hub
        .lease_acquire(acquire(&["a", "b"], "owner", 100), 10)
        .unwrap()
        .event;
    let expected: Vec<_> = hub.leases(10).into_values().collect();
    assert_eq!(granted.updated_leases(), expected.as_slice());
    assert!(granted.updated_leases()[1].epoch > granted.updated_leases()[0].epoch);
    let fences: Vec<_> = granted.updated_leases().iter().map(Lease::fence).collect();
    let renewed = hub
        .lease_renew(renew(fences.clone(), 200), 20)
        .unwrap()
        .event;
    assert_eq!(renewed.updated_leases().len(), 2);
    assert_eq!(renewed.updated_leases()[0].expires_at, 220);
    assert_eq!(renewed.updated_leases()[0].epoch, expected[0].epoch);
    let plain = recorded(hub.append(input(), 21).unwrap());
    assert!(plain.updated_leases().is_empty());
    let released = hub
        .lease_release(release(fences.clone()), 22)
        .unwrap()
        .event;
    assert!(released.updated_leases().is_empty());
    let mut stale = input();
    stale.guards = vec![fences[0].clone()];
    assert!(recorded(hub.append(stale, 23).unwrap())
        .updated_leases()
        .is_empty());
    drop(hub);
    let reopened = Hub::open(dir.path()).unwrap();
    assert_eq!(reopened.events(0)[0].updated_leases(), expected.as_slice());
    assert_eq!(
        reopened.events(0)[1].updated_leases(),
        renewed.updated_leases()
    );
}

#[test]
fn work_lease_names_are_kind_colon_id() {
    use towermd_core::hub::is_work_lease;
    assert!(is_work_lease("module:A-one"));
    assert!(is_work_lease("ticket:42"));
    for name in ["main", "maintenance", ":x", "module:", ""] {
        assert!(!is_work_lease(name), "{name}");
    }
}
