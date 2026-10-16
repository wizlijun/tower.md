use serde_json::json;
use std::fs;
use tempfile::TempDir;
use towermd_core::hub::{Fence, Hub, LeaseChange, NewEvent};
use uuid::Uuid;

fn input() -> NewEvent {
    NewEvent {
        eid: Uuid::now_v7().to_string(),
        host: "host-a".into(),
        actor: "process:towermd-cli".into(),
        kind: "task.claimed".into(),
        layer: "L4".into(),
        obj: "T-example".into(),
        data: json!({"run": "run-a"}),
        fence: None,
    }
}

fn acquire(names: &[&str], holder: &str, ttl_ms: u64) -> Option<LeaseChange> {
    Some(LeaseChange::Acquire {
        names: names.iter().map(|name| (*name).into()).collect(),
        holder: holder.into(),
        ttl_ms,
    })
}

#[test]
fn append_restart_and_duplicate_return_original_without_any_changes() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    let original = input();
    let event = hub
        .append(original.clone(), acquire(&["module:a"], "run-a", 100), 1000)
        .unwrap();
    assert_eq!(event.seq, 1);
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
        hub.append(duplicate, acquire(&["other"], "other", 0), u64::MAX)
            .unwrap(),
        event
    );
    assert_eq!(hub.events(0), vec![event]);
    assert!(hub.events(1).is_empty());
    assert_eq!(hub.last_seq(), 1);
    assert_eq!(hub.leases(1000), leases);
}

#[test]
fn acquisition_is_atomic_and_epochs_are_global_and_survive_release_and_restart() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), acquire(&["b"], "owner", 100), 10)
        .unwrap();
    assert!(hub
        .append(input(), acquire(&["a", "b"], "other", 100), 11)
        .is_err());
    assert_eq!(hub.last_seq(), 1);
    assert_eq!(hub.leases(11).len(), 1);
    let first = hub.leases(11)["b"].fence();
    hub.append(
        input(),
        Some(LeaseChange::Release {
            fences: vec![first.clone()],
        }),
        12,
    )
    .unwrap();
    drop(hub);
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), acquire(&["a", "b"], "other", 100), 13)
        .unwrap();
    let leases = hub.leases(13);
    assert!(leases["a"].epoch > first.epoch);
    assert!(leases["b"].epoch > leases["a"].epoch);
}

#[test]
fn stale_fence_is_recorded_and_skips_even_invalid_lease_changes() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), acquire(&["a"], "owner", 10), 10)
        .unwrap();
    let old = hub.leases(10)["a"].fence();
    hub.append(input(), acquire(&["a"], "new-owner", 100), 20)
        .unwrap();
    let current = hub.leases(20);
    let mut stale = input();
    stale.fence = Some(old.clone());
    let event = hub
        .append(
            stale,
            Some(LeaseChange::Release {
                fences: vec![old.clone()],
            }),
            21,
        )
        .unwrap();
    assert!(event.stale);
    assert_eq!(event.epoch, Some(old.epoch));
    assert_eq!(hub.leases(21), current);
    let mut stale = input();
    stale.fence = Some(old);
    assert!(hub.append(stale, acquire(&[], "", 0), 22).unwrap().stale);
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
    hub.append(input(), acquire(&["a", "b"], "owner", 100), 10)
        .unwrap();
    let fences: Vec<_> = hub.leases(10).values().map(|lease| lease.fence()).collect();
    let mut invalid = fences.clone();
    invalid[1].holder = "imposter".into();
    assert!(hub
        .append(
            input(),
            Some(LeaseChange::Renew {
                fences: invalid.clone(),
                ttl_ms: 200
            }),
            20
        )
        .is_err());
    assert!(hub
        .append(input(), Some(LeaseChange::Release { fences: invalid }), 20)
        .is_err());
    assert_eq!(hub.leases(20)["a"].expires_at, 110);
    assert_eq!(hub.last_seq(), 1);
    hub.append(
        input(),
        Some(LeaseChange::Renew {
            fences: fences.clone(),
            ttl_ms: 200,
        }),
        20,
    )
    .unwrap();
    drop(hub);
    let mut hub = Hub::open(dir.path()).unwrap();
    assert_eq!(hub.leases(219)["a"].expires_at, 220);
    assert!(hub.leases(220).is_empty());
    assert!(hub
        .append(
            input(),
            Some(LeaseChange::Renew {
                fences: fences.clone(),
                ttl_ms: 100
            }),
            220
        )
        .is_err());
    assert!(hub
        .append(input(), Some(LeaseChange::Release { fences }), 220)
        .is_err());
    assert_eq!(hub.last_seq(), 2);
}

#[test]
fn expired_release_without_input_fence_cannot_damage_new_holder() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), acquire(&["a"], "old", 10), 10).unwrap();
    let fence = hub.leases(10)["a"].fence();
    hub.append(input(), acquire(&["a"], "new", 10), 20).unwrap();
    assert!(hub
        .append(
            input(),
            Some(LeaseChange::Release {
                fences: vec![fence]
            }),
            21
        )
        .is_err());
    assert_eq!(hub.leases(21)["a"].holder, "new");
    assert_eq!(hub.last_seq(), 2);
}

#[test]
fn current_fence_is_historical_even_after_expiry() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), acquire(&["a"], "owner", 10), 10)
        .unwrap();
    let mut event = input();
    event.fence = Some(hub.leases(10)["a"].fence());
    let accepted = hub.append(event.clone(), None, 11).unwrap();
    assert!(!accepted.stale);
    event.eid = Uuid::now_v7().to_string();
    assert!(hub.append(event, None, 20).unwrap().stale);
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
        assert!(hub.append(event, None, 10).is_err(), "{field}");
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
        assert!(hub.append(event, None, 10).is_err(), "{actor}");
    }
    let mut event = input();
    event.eid = "00000000-0000-4000-8000-000000000000".into();
    assert!(hub.append(event, None, 10).is_err());
    assert!(hub.append(input(), None, u64::MAX).is_err());
    assert_eq!(hub.last_seq(), 0);
    for actor in ["human:bruce", "process:towermd-cli", "codex/1.0"] {
        let mut event = input();
        event.actor = actor.into();
        hub.append(event, None, 10).unwrap();
    }
    assert_eq!(hub.last_seq(), 3);
}

#[test]
fn invalid_lease_batches_and_overflow_are_rejected_atomically() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    for change in [
        acquire(&[], "owner", 10),
        acquire(&["a", "a"], "owner", 10),
        acquire(&[""], "owner", 10),
        acquire(&["a"], "", 10),
        acquire(&["a"], "owner", 0),
        acquire(&["a"], "owner", u64::MAX),
        Some(LeaseChange::Renew {
            fences: vec![],
            ttl_ms: 10,
        }),
        Some(LeaseChange::Release { fences: vec![] }),
    ] {
        assert!(hub.append(input(), change, 10).is_err());
        assert_eq!(hub.last_seq(), 0);
        assert!(hub.leases(10).is_empty());
    }
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
fn accepted_events_are_durable_and_disk_failure_leaves_memory_unchanged() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("hub");
    let mut hub = Hub::open(&path).unwrap();
    let accepted = hub
        .append(input(), acquire(&["a"], "owner", 100), 10)
        .unwrap();
    let before = hub.leases(10);
    let saved = root.path().join("saved");
    fs::rename(&path, &saved).unwrap();
    fs::write(&path, "not a directory").unwrap();
    assert!(hub
        .append(input(), acquire(&["b"], "other", 100), 11)
        .is_err());
    assert_eq!(hub.events(0), vec![accepted.clone()]);
    assert_eq!(hub.leases(10), before);
    assert_eq!(hub.last_seq(), 1);
    fs::remove_file(&path).unwrap();
    fs::rename(&saved, &path).unwrap();
    drop(hub);
    let mut hub = Hub::open(&path).unwrap();
    assert_eq!(hub.events(0), vec![accepted]);
    assert_eq!(hub.append(input(), None, 12).unwrap().seq, 2);
}

#[test]
fn corrupt_journal_is_rejected_without_reset() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), None, 10).unwrap();
    drop(hub);
    let journal = dir.path().join("journal.json");
    fs::write(&journal, b"{broken").unwrap();
    assert!(Hub::open(dir.path()).is_err());
    assert_eq!(fs::read(&journal).unwrap(), b"{broken");
}

#[test]
fn unknown_fence_is_stale_without_creating_a_lease() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    let mut event = input();
    event.fence = Some(Fence {
        name: "missing".into(),
        holder: "owner".into(),
        epoch: 1,
    });
    assert!(
        hub.append(event, acquire(&["a"], "owner", 10), 10)
            .unwrap()
            .stale
    );
    assert!(hub.leases(10).is_empty());
}

#[test]
fn valid_json_with_corrupt_sequence_epoch_or_stale_is_rejected() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), acquire(&["a"], "owner", 100), 10)
        .unwrap();
    drop(hub);
    let path = dir.path().join("journal.json");
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for field in [
        "seq",
        "stale",
        "epoch",
        "expires_at",
        "version",
        "duplicate",
    ] {
        let mut corrupt = original.clone();
        match field {
            "seq" => corrupt["events"][0]["seq"] = json!(2),
            "stale" => corrupt["events"][0]["stale"] = json!(true),
            "epoch" => corrupt["events"][0]["lease_delta"]["updated"][0]["epoch"] = json!(99),
            "expires_at" => {
                corrupt["events"][0]["lease_delta"]["updated"][0]["expires_at"] = json!(999)
            }
            "version" => corrupt["version"] = json!(999),
            "duplicate" => {
                let mut duplicate = corrupt["events"][0].clone();
                duplicate["seq"] = json!(2);
                corrupt["events"].as_array_mut().unwrap().push(duplicate);
            }
            _ => unreachable!(),
        }
        fs::write(&path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        assert!(Hub::open(dir.path()).is_err(), "{field}");
    }
}

#[test]
fn duplicate_fences_and_zero_ttl_do_not_mutate_leases() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    hub.append(input(), acquire(&["a"], "owner", 100), 10)
        .unwrap();
    let before = hub.leases(10);
    let fence = before["a"].fence();
    for change in [
        LeaseChange::Renew {
            fences: vec![fence.clone(), fence.clone()],
            ttl_ms: 100,
        },
        LeaseChange::Renew {
            fences: vec![fence.clone()],
            ttl_ms: 0,
        },
        LeaseChange::Renew {
            fences: vec![fence.clone()],
            ttl_ms: u64::MAX,
        },
        LeaseChange::Release {
            fences: vec![fence.clone(), fence],
        },
    ] {
        assert!(hub.append(input(), Some(change), 11).is_err());
        assert_eq!(hub.leases(11), before);
        assert_eq!(hub.last_seq(), 1);
    }
}

#[test]
fn failed_rename_preserves_previous_journal_and_can_retry() {
    let root = TempDir::new().unwrap();
    let mut hub = Hub::open(root.path()).unwrap();
    let first = hub.append(input(), None, 10).unwrap();
    let journal = root.path().join("journal.json");
    let saved = root.path().join("saved.json");
    fs::rename(&journal, &saved).unwrap();
    fs::create_dir(&journal).unwrap();
    let event = input();
    assert!(hub
        .append(event.clone(), acquire(&["a"], "owner", 100), 11)
        .is_err());
    assert_eq!(hub.events(0), vec![first]);
    assert!(hub.leases(11).is_empty());
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
    fs::remove_dir(&journal).unwrap();
    fs::rename(&saved, &journal).unwrap();
    assert_eq!(
        hub.append(event, acquire(&["a"], "owner", 100), 12)
            .unwrap()
            .seq,
        2
    );
    drop(hub);
    assert_eq!(Hub::open(root.path()).unwrap().last_seq(), 2);
}

#[test]
fn updated_leases_exposes_atomic_grants_and_renewals_for_replay() {
    let dir = TempDir::new().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    let granted = hub
        .append(input(), acquire(&["a", "b"], "owner", 100), 10)
        .unwrap();
    let expected: Vec<_> = hub.leases(10).into_values().collect();
    assert_eq!(granted.updated_leases(), expected.as_slice());
    assert!(granted.updated_leases()[1].epoch > granted.updated_leases()[0].epoch);
    let fences: Vec<_> = granted
        .updated_leases()
        .iter()
        .map(|lease| lease.fence())
        .collect();
    let renewed = hub
        .append(
            input(),
            Some(LeaseChange::Renew {
                fences: fences.clone(),
                ttl_ms: 200,
            }),
            20,
        )
        .unwrap();
    assert_eq!(renewed.updated_leases().len(), 2);
    assert_eq!(renewed.updated_leases()[0].expires_at, 220);
    assert_eq!(renewed.updated_leases()[0].epoch, expected[0].epoch);
    let plain = hub.append(input(), None, 21).unwrap();
    assert!(plain.updated_leases().is_empty());
    let released = hub
        .append(
            input(),
            Some(LeaseChange::Release {
                fences: fences.clone(),
            }),
            22,
        )
        .unwrap();
    assert!(released.updated_leases().is_empty());
    let mut stale = input();
    stale.fence = Some(fences[0].clone());
    assert!(hub
        .append(stale, acquire(&["c"], "other", 100), 23)
        .unwrap()
        .updated_leases()
        .is_empty());
    drop(hub);
    let reopened = Hub::open(dir.path()).unwrap();
    assert_eq!(reopened.events(0)[0].updated_leases(), expected.as_slice());
    assert_eq!(
        reopened.events(1)[0].updated_leases(),
        renewed.updated_leases()
    );
}
