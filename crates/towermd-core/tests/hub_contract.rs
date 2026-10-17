use serde_json::json;
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
fn input(kind: &str) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "contract-host".into(),
        actor: "process:towermd-cli".into(),
        kind: kind.into(),
        layer: "L5".into(),
        obj: "object".into(),
        data: json!([null,{"commit":false},17]),
        guards: vec![],
    }
}
fn sequence(receipt: AppendReceipt) -> u64 {
    match receipt {
        AppendReceipt::Event(e) => e.seq,
        AppendReceipt::Duplicate { seq } => seq,
    }
}
#[test]
fn opaque_append_contract() {
    let dir = tempfile::tempdir().unwrap();
    let mut local = Hub::open(dir.path()).unwrap();
    let hub: &mut dyn HubProtocol = &mut local;
    let mut event = input("arbitrary.event");
    event.eid = towermd_core::events::derived_eid(&"a".repeat(40), "arbitrary.event").unwrap();
    let first = sequence(hub.append(event.clone(), 10).unwrap());
    assert_eq!(sequence(hub.append(event, 11).unwrap()), first);
    let batch = hub.subscribe(first - 1, 12).unwrap();
    assert_eq!(batch.events[0].data, json!([null,{"commit":false},17]));
}
#[test]
fn reserved_lease_type_cannot_be_client_appended() {
    let dir = tempfile::tempdir().unwrap();
    let mut local = Hub::open(dir.path()).unwrap();
    let hub: &mut dyn HubProtocol = &mut local;
    for kind in [
        "lease.granted",
        "lease.renewed",
        "lease.expired",
        "lease.released",
    ] {
        assert!(matches!(
            hub.append(input(kind), 10),
            Err(HubError::Invalid(_))
        ));
        assert_eq!(hub.contiguous_seq(), 0);
    }
}
fn acquire(
    hub: &mut dyn HubProtocol,
    names: &[&str],
    holder: &str,
    now: u64,
    ttl: u64,
) -> LeaseReceipt {
    hub.lease_acquire(
        AcquireLease {
            host: "host".into(),
            names: names.iter().map(|s| (*s).into()).collect(),
            holder: holder.into(),
            ttl_ms: ttl,
        },
        now,
    )
    .unwrap()
}
#[test]
fn acquisition_is_atomic_with_structured_owners() {
    let dir = tempfile::tempdir().unwrap();
    let mut local = Hub::open(dir.path()).unwrap();
    let hub: &mut dyn HubProtocol = &mut local;
    let grant = acquire(hub, &["a", "b"], "one", 10, 100);
    let before = hub.subscribe(0, 10).unwrap().head;
    let error = hub
        .lease_acquire(
            AcquireLease {
                host: "host".into(),
                names: vec!["b".into(), "c".into()],
                holder: "two".into(),
                ttl_ms: 100,
            },
            11,
        )
        .unwrap_err();
    assert_eq!(
        error,
        HubError::LeaseConflict {
            owners: vec![grant.leases[1].clone()]
        }
    );
    assert_eq!(
        hub.subscribe(before.contiguous_seq, 11).unwrap().head,
        before
    );
}
#[test]
fn all_guards_are_fenced_and_invalid_guards_do_not_consume_seq() {
    let dir = tempfile::tempdir().unwrap();
    let mut local = Hub::open(dir.path()).unwrap();
    let hub: &mut dyn HubProtocol = &mut local;
    let grant = acquire(hub, &["a", "b"], "one", 10, 100);
    let mut event = input("future.event");
    event.guards = grant.leases.iter().map(Lease::fence).collect();
    event.guards[1].epoch += 1;
    let AppendReceipt::Event(recorded) = hub.append(event.clone(), 11).unwrap() else {
        panic!()
    };
    assert!(recorded.stale);
    assert_eq!(recorded.epoch, Some(grant.leases[0].epoch));
    let seq = hub.contiguous_seq();
    event.eid = uuid::Uuid::now_v7().to_string();
    event.guards[1] = event.guards[0].clone();
    assert!(matches!(hub.append(event, 11), Err(HubError::Invalid(_))));
    assert_eq!(hub.contiguous_seq(), seq);
}
#[test]
fn lease_lifecycle_uses_hub_l5_events_and_preserves_new_holder() {
    let dir = tempfile::tempdir().unwrap();
    let mut local = Hub::open(dir.path()).unwrap();
    let hub: &mut dyn HubProtocol = &mut local;
    let grant = acquire(hub, &["a"], "old", 10, 100);
    let fence = grant.leases[0].fence();
    let renewed = hub
        .lease_renew(
            RenewLease {
                host: "host".into(),
                fences: vec![fence.clone()],
                ttl_ms: 100,
            },
            50,
        )
        .unwrap();
    assert_eq!(renewed.leases[0].epoch, fence.epoch);
    assert!(hub
        .subscribe(0, 110)
        .unwrap()
        .events
        .iter()
        .all(|e| e.kind != "lease.expired"));
    let batch = hub.subscribe(0, 150).unwrap();
    assert_eq!(batch.events.last().unwrap().kind, "lease.expired");
    assert!(matches!(
        hub.lease_renew(
            RenewLease {
                host: "host".into(),
                fences: vec![fence.clone()],
                ttl_ms: 100
            },
            150
        ),
        Err(HubError::FenceExpired { .. })
    ));
    let next = acquire(hub, &["a"], "new", 151, 100);
    assert!(next.leases[0].epoch > fence.epoch);
    assert!(hub
        .lease_release(
            ReleaseLease {
                host: "host".into(),
                fences: vec![fence]
            },
            152
        )
        .is_err());
    for event in hub.subscribe(0, 152).unwrap().events {
        assert_eq!(event.layer, "L5");
        assert_eq!(event.actor, "process:towermd-hub");
        assert!(event.epoch.is_some());
    }
    assert_eq!(
        hub.subscribe(hub.contiguous_seq(), 152)
            .unwrap()
            .head
            .leases["a"]
            .holder,
        "new"
    );
}

#[test]
fn ack_compacts_preserves_eid_and_requires_snapshot_for_old_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let mut local = Hub::open(dir.path()).unwrap();
    let hub: &mut dyn HubProtocol = &mut local;
    let grant = acquire(hub, &["main"], "writer", 1, 100);
    let event = input("arbitrary.event");
    let seq = sequence(hub.append(event.clone(), 2).unwrap());
    let ack = SnapshotAck {
        commit: "a".repeat(40),
        seq,
        epoch: grant.leases[0].epoch,
    };
    let checkpoint = json!(["opaque", false]);
    hub.snapshot_ack(ack.clone(), checkpoint.clone()).unwrap();
    assert_eq!(
        hub.append(event, 3).unwrap(),
        AppendReceipt::Duplicate { seq }
    );
    assert!(matches!(
        hub.subscribe(0, 3),
        Err(HubError::SnapshotRequired { .. })
    ));
    assert!(hub.subscribe(seq, 3).unwrap().events.is_empty());
    let mut changed = ack;
    changed.commit = "b".repeat(40);
    hub.snapshot_ack(changed.clone(), checkpoint).unwrap();
    assert!(hub
        .snapshot_ack(changed, json!({"different":true}))
        .is_err());
    drop(local);
    let mut local = Hub::open(dir.path()).unwrap();
    assert_eq!(local.subscribe(seq, 3).unwrap().head.floor_seq, seq);
}
#[test]
fn ack_epoch_belongs_to_its_prefix_and_floor_never_regresses() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    let first = acquire(&mut hub, &["main"], "writer", 1, 100);
    acquire(&mut hub, &["module:A"], "run", 2, 100);
    let bytes = std::fs::read(dir.path().join("journal.jsonl")).unwrap();
    assert!(matches!(
        HubProtocol::snapshot_ack(
            &mut hub,
            SnapshotAck {
                commit: "a".repeat(40),
                seq: 1,
                epoch: 2
            },
            json!({})
        ),
        Err(HubError::Invalid(_))
    ));
    assert_eq!(
        std::fs::read(dir.path().join("journal.jsonl")).unwrap(),
        bytes
    );
    assert!(HubProtocol::snapshot_ack(
        &mut hub,
        SnapshotAck {
            commit: "a".repeat(40),
            seq: 0,
            epoch: first.leases[0].epoch
        },
        json!({})
    )
    .is_err());
    let dir2 = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(dir2.path()).unwrap();
    hub.rebuild(RebuildSeed {
        final_snapshot: None,
        source_seq: 0,
        source_epoch: 0,
        seq_boundary: 1 << 32,
        epoch_boundary: 1 << 32,
        checkpoint: json!([]),
        receipts: std::collections::BTreeMap::new(),
        tail: vec![],
    })
    .unwrap();
    assert!(HubProtocol::snapshot_ack(
        &mut hub,
        SnapshotAck {
            commit: "a".repeat(40),
            seq: 1,
            epoch: 1
        },
        json!([])
    )
    .is_err());
}
#[test]
fn rebuild_contract_keeps_opaque_checkpoint_receipts_and_invalidates_old_fences() {
    use std::collections::BTreeMap;
    let dir = tempfile::tempdir().unwrap();
    let mut local = Hub::open(dir.path()).unwrap();
    let hub: &mut dyn HubProtocol = &mut local;
    let grant = acquire(hub, &["main"], "old", 1, 100);
    let fence = grant.leases[0].fence();
    let mut original = input("arbitrary.event");
    original.guards = vec![fence.clone()];
    let original_seq = sequence(hub.append(original.clone(), 2).unwrap());
    let seq_boundary = original_seq + (1 << 32);
    let epoch_boundary = grant.leases[0].epoch + (1 << 32);
    let checkpoint = json!([null,{"uninterpreted":false}]);
    let seed = RebuildSeed {
        final_snapshot: None,
        source_seq: original_seq,
        source_epoch: grant.leases[0].epoch,
        seq_boundary,
        epoch_boundary,
        checkpoint: checkpoint.clone(),
        receipts: BTreeMap::from([(grant.event.eid, 1), (original.eid.clone(), original_seq)]),
        tail: vec![],
    };
    assert_eq!(
        hub.rebuild(seed.clone()).unwrap(),
        RebuildReceipt {
            contiguous_seq: seq_boundary,
            epoch_high_water: epoch_boundary
        }
    );
    assert_eq!(
        hub.append(original, 3).unwrap(),
        AppendReceipt::Duplicate { seq: original_seq }
    );
    assert!(
        matches!(hub.subscribe(0,3),Err(HubError::SnapshotRequired{checkpoint:found,..})if found==checkpoint)
    );
    assert!(hub
        .subscribe(seq_boundary, 3)
        .unwrap()
        .head
        .leases
        .is_empty());
    let mut stale = input("arbitrary.event");
    stale.guards = vec![fence.clone()];
    let AppendReceipt::Event(event) = hub.append(stale, 3).unwrap() else {
        panic!()
    };
    assert!(event.stale);
    assert_eq!(event.epoch, Some(fence.epoch));
    let new = acquire(hub, &["main"], "new", 3, 100);
    assert_eq!(new.leases[0].epoch, epoch_boundary + 1);
    let head = hub.subscribe(hub.contiguous_seq(), 3).unwrap().head;
    assert!(hub.rebuild(seed).is_err());
    assert_eq!(hub.subscribe(head.contiguous_seq, 3).unwrap().head, head);
}

fn atomic_rebuild_fixture() -> (tempfile::TempDir, Hub, RebuildSeed, NewEvent) {
    let directory = tempfile::tempdir().unwrap();
    let mut source = Hub::open(directory.path()).unwrap();
    let mut seed = RebuildSeed {
        source_seq: 0,
        source_epoch: 0,
        seq_boundary: 1 << 32,
        epoch_boundary: 1 << 32,
        checkpoint: json!(["gap", null]),
        receipts: std::collections::BTreeMap::new(),
        tail: vec![],
        final_snapshot: None,
    };
    source.rebuild(seed.clone()).unwrap();
    let grant = acquire(&mut source, &["main"], "new-owner", 10, 100);
    let original = input("opaque.business");
    let covered = sequence(source.append(original.clone(), 11).unwrap());
    source
        .lease_renew(
            RenewLease {
                host: "host".into(),
                fences: vec![grant.leases[0].fence()],
                ttl_ms: 200,
            },
            12,
        )
        .unwrap();
    source.append(input("opaque.tail"), 13).unwrap();
    seed.tail = source.subscribe(seed.seq_boundary, 13).unwrap().events;
    let ack = SnapshotAck {
        commit: "a".repeat(40),
        seq: covered,
        epoch: grant.leases[0].epoch,
    };
    let checkpoint = json!([null, {"opaque-final": [false, 17]}]);
    seed.final_snapshot = Some(RebuildSnapshot {
        ack: ack.clone(),
        checkpoint: checkpoint.clone(),
    });
    HubProtocol::snapshot_ack(&mut source, ack, checkpoint).unwrap();
    (directory, source, seed, original)
}

#[test]
fn atomic_rebuild_matches_compaction_and_reopens_exact_ack_tail_and_receipts() {
    let (_source_directory, mut source, seed, original) = atomic_rebuild_fixture();
    let directory = tempfile::tempdir().unwrap();
    let mut target = Hub::open(directory.path()).unwrap();
    let snapshot = seed.final_snapshot.clone().unwrap();
    let expected = source.subscribe(snapshot.ack.seq, 13).unwrap();
    target.rebuild(seed.clone()).unwrap();
    assert_eq!(target.subscribe(snapshot.ack.seq, 13).unwrap(), expected);
    assert!(
        matches!(target.subscribe(0, 13), Err(HubError::SnapshotRequired { checkpoint, head }) if checkpoint == snapshot.checkpoint && head == expected.head)
    );
    assert_eq!(
        target.append(original.clone(), 13).unwrap(),
        AppendReceipt::Duplicate {
            seq: snapshot.ack.seq
        }
    );
    let header: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(directory.path().join("journal.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        header["checkpoint"]["ack"],
        serde_json::to_value(&snapshot.ack).unwrap()
    );
    assert_eq!(header["checkpoint"]["checkpoint"], snapshot.checkpoint);
    assert_eq!(header["checkpoint"]["floor_seq"], snapshot.ack.seq);
    drop(target);
    let mut reopened = Hub::open(directory.path()).unwrap();
    assert_eq!(reopened.subscribe(snapshot.ack.seq, 13).unwrap(), expected);
    assert_eq!(
        reopened.append(original, 13).unwrap(),
        AppendReceipt::Duplicate {
            seq: snapshot.ack.seq
        }
    );
    assert_eq!(
        expected.events,
        seed.tail
            .into_iter()
            .filter(|event| event.seq > snapshot.ack.seq)
            .collect::<Vec<_>>()
    );
}

#[test]
fn atomic_rebuild_rejects_invalid_ack_and_tail_without_partial_persistence() {
    let (_source_directory, _source, valid, _) = atomic_rebuild_fixture();
    for variant in 0..8 {
        let directory = tempfile::tempdir().unwrap();
        let mut target = Hub::open(directory.path()).unwrap();
        let before = std::fs::read(directory.path().join("journal.jsonl")).unwrap();
        let mut invalid = valid.clone();
        match variant {
            0 => invalid.final_snapshot.as_mut().unwrap().ack.seq = valid.seq_boundary - 1,
            1 => {
                invalid.final_snapshot.as_mut().unwrap().ack.seq =
                    valid.tail.last().unwrap().seq + 1
            }
            2 => invalid.final_snapshot.as_mut().unwrap().ack.epoch = valid.epoch_boundary + 2,
            3 => invalid.final_snapshot.as_mut().unwrap().ack.commit = "not-a-commit".into(),
            4 => invalid.tail[0].seq += 1,
            5 => invalid.tail[1].eid = invalid.tail[0].eid.clone(),
            6 => {
                invalid.receipts.insert(uuid::Uuid::now_v7().to_string(), 1);
            }
            7 => invalid.seq_boundary -= 1,
            _ => unreachable!(),
        }
        assert!(
            matches!(target.rebuild(invalid), Err(HubError::Invalid(_))),
            "variant {variant}"
        );
        assert_eq!(
            std::fs::read(directory.path().join("journal.jsonl")).unwrap(),
            before
        );
        assert_eq!(target.subscribe(0, 13).unwrap().head.contiguous_seq, 0);
        drop(target);
        assert_eq!(Hub::open(directory.path()).unwrap().contiguous_seq(), 0);
    }
}

#[test]
fn rebuild_seed_without_final_snapshot_remains_wire_compatible() {
    let seed = RebuildSeed {
        source_seq: 0,
        source_epoch: 0,
        seq_boundary: 1 << 32,
        epoch_boundary: 1 << 32,
        checkpoint: json!([false, null]),
        receipts: std::collections::BTreeMap::new(),
        tail: vec![],
        final_snapshot: None,
    };
    let wire = serde_json::to_value(&seed).unwrap();
    assert!(wire.get("final_snapshot").is_none());
    assert_eq!(serde_json::from_value::<RebuildSeed>(wire).unwrap(), seed);
}
