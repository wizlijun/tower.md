use serde_json::{json, Value};
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
use towermd_core::snapshot::{archive_events, SequenceRange, Snapshot, StateProjection};

fn input(kind: &str, data: Value) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:test".into(),
        kind: kind.into(),
        layer: "L5".into(),
        obj: "T-1".into(),
        data,
        guards: vec![],
    }
}
fn acquire(name: &str, holder: &str) -> AcquireLease {
    AcquireLease {
        host: "host".into(),
        names: vec![name.into()],
        holder: holder.into(),
        ttl_ms: 1000,
    }
}
fn recorded(receipt: AppendReceipt) -> Event {
    match receipt {
        AppendReceipt::Event(event) => *event,
        AppendReceipt::Duplicate { .. } => panic!("event body expected"),
    }
}
fn state(task: &str, run: &str) -> Value {
    json!({
        "task": {"type": "Task", "id": "T-1", "title":"Task", "module":"A", "touches":[], "state": task, "future": {"nested": [null, {"opaque": true}]}},
        "run": {"type": "Run", "id": "R-1", "task": "T-1", "state": run, "leases": [], "cost_usd": null, "future": [1, null]}
    })
}
fn fixture() -> Vec<Event> {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    hub.lease_acquire(acquire("main", "writer"), 1).unwrap();
    let grant = hub.lease_acquire(acquire("module:A", "R-1"), 2).unwrap();
    let fences: Vec<_> = grant.leases.iter().map(Lease::fence).collect();
    let mut claim = input("task.claimed", state("running", "running"));
    claim.data["run"]["leases"] = json!(fences);
    claim.guards = fences.clone();
    hub.append(claim, 2).unwrap();
    for now in [3, 4] {
        hub.lease_renew(
            RenewLease {
                host: "host".into(),
                fences: fences.clone(),
                ttl_ms: 1000,
            },
            now,
        )
        .unwrap();
    }
    hub.lease_release(
        ReleaseLease {
            host: "host".into(),
            fences,
        },
        5,
    )
    .unwrap();
    hub.append(input("task.released", state("ready", "released")), 5)
        .unwrap();
    let grant = hub.lease_acquire(acquire("module:A", "R-1"), 6).unwrap();
    let fences: Vec<_> = grant.leases.iter().map(Lease::fence).collect();
    let mut claim = input("task.claimed", state("running", "running"));
    claim.data["run"]["leases"] = json!(fences);
    claim.guards = fences.clone();
    hub.append(claim, 6).unwrap();
    hub.lease_renew(
        RenewLease {
            host: "host".into(),
            fences,
            ttl_ms: 1000,
        },
        7,
    )
    .unwrap();
    hub.append(input("task.state", state("ready", "released")), 2000)
        .unwrap();
    // Independent lease events own seqs: grants 1/2/8, claims 3/9,
    // renewals 4/5/10, lease release 6, business release/state 7/11.
    hub.events(0)
}
fn snapshot(events: &[Event]) -> Snapshot {
    Snapshot::from_events("project", events, events.last().unwrap().seq, 1).unwrap()
}

#[test]
fn claim_renew_release_and_expire_replay_across_archive_windows() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let mut archive = archive_events(&events, 0, 5).unwrap();
    archive.extend(archive_events(&events, 5, 11).unwrap());
    assert_eq!(
        archive.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [1, 2, 3, 6, 7, 8, 9, 11]
    );
    snapshot.validate_archive("project", &archive).unwrap();
    assert_eq!(
        snapshot.omitted,
        [
            SequenceRange { start: 4, end: 5 },
            SequenceRange { start: 10, end: 10 }
        ]
    );
    let mut replay = StateProjection::default();
    for event in &events[..7] {
        replay.apply_recorded(event).unwrap();
    }
    assert!(!replay.lease_summaries.contains_key("module:A"));
    for event in &events[7..] {
        replay.apply_recorded(event).unwrap();
    }
    assert_eq!(replay, snapshot.projection);
    assert!(replay.lease_summaries.contains_key("module:A"));
    assert_eq!(replay.objects["Task"]["T-1"]["state"], "ready");
    assert_eq!(replay.objects["Run"]["R-1"]["state"], "released");
}

#[test]
fn claim_keeps_unknown_fields_null_and_recorded_fences() {
    let events = fixture();
    let snapshot = Snapshot::from_events("project", &events, 3, 1).unwrap();
    let run = &snapshot.projection.objects["Run"]["R-1"];
    assert_eq!(run["leases"], json!(events[2].guards));
    assert!(events[2].updated_leases().is_empty());
    assert_eq!(run["future"], json!([1, null]));
    assert_eq!(run["cost_usd"], Value::Null);
    assert_eq!(
        snapshot.projection.objects["Task"]["T-1"]["future"],
        state("running", "running")["task"]["future"]
    );
    assert_eq!(
        serde_json::from_str::<Snapshot>(&serde_json::to_string(&snapshot).unwrap()).unwrap(),
        snapshot
    );
}

#[test]
fn heartbeat_is_hub_control_metadata_and_business_events_are_preserved() {
    let events = fixture();
    assert!(events[3].is_pure_heartbeat());
    assert_eq!(events[3].data, Value::Null);
    assert_eq!(events[3].actor, "process:towermd-hub");
    assert_eq!(events[3].layer, "L5");
    assert!(!events[0].is_pure_heartbeat());
    assert!(!events[10].is_pure_heartbeat());
    assert!(archive_events(&events, 0, 11)
        .unwrap()
        .iter()
        .all(|event| !event.is_pure_heartbeat()));
    // New business data does not control heartbeat classification. Legacy renew
    // bodies are classified conservatively by the isolated legacy decoder.
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    hub.lease_acquire(acquire("main", "writer"), 1).unwrap();
    let business = recorded(
        hub.append(input("task.state", state("verifying", "running")), 2)
            .unwrap(),
    );
    assert!(!business.is_pure_heartbeat());
    let snapshot = Snapshot::from_events("project", &hub.events(0), 2, 1).unwrap();
    assert!(snapshot.omitted.is_empty());
    assert_eq!(
        snapshot.projection.objects["Task"]["T-1"]["state"],
        "verifying"
    );
    snapshot
        .validate_archive("project", &archive_events(&hub.events(0), 0, 2).unwrap())
        .unwrap();
}

#[test]
fn recorded_stale_future_and_rebuilt_do_not_invent_business_changes() {
    let events = fixture();
    let mut projection = snapshot(&events).projection;
    let before = projection.clone();
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    let mut stale = input("task.state", json!({"invalid": true}));
    stale.guards = vec![Fence {
        name: "main".into(),
        holder: "old".into(),
        epoch: u64::MAX,
    }];
    let stale = recorded(hub.append(stale, 1).unwrap());
    assert!(stale.stale);
    projection.apply_recorded(&stale).unwrap();
    let future = recorded(
        hub.append(input("future.event", state("bad", "bad")), 2)
            .unwrap(),
    );
    projection.apply_recorded(&future).unwrap();
    assert_eq!(projection, before);
    let rebuilt = recorded(hub.append(input("hub.rebuilt", json!({})), 3).unwrap());
    projection.apply_recorded(&rebuilt).unwrap();
    assert!(projection.lease_summaries.is_empty());
    assert_eq!(projection.objects, before.objects);
    let mut resumed = events[10].clone();
    resumed.data = state("ready", "released");
    projection.apply_recorded(&resumed).unwrap();
    assert_eq!(projection.objects["Run"]["R-1"]["state"], "released");
}

#[test]
fn malformed_task_and_run_are_rejected_atomically() {
    let events = fixture();
    let mut projection = StateProjection::default();
    for event in &events[..2] {
        projection.apply_recorded(event).unwrap();
    }
    let before = projection.clone();
    for (field, key, value) in [
        ("task", "type", json!("FutureType")),
        ("task", "type", json!("Module")),
        ("run", "type", json!("Task")),
        ("run", "id", json!(null)),
        ("task", "id", json!("")),
    ] {
        let mut event = events[2].clone();
        event.data[field][key] = value;
        assert!(projection.apply_recorded(&event).is_err());
        assert_eq!(projection, before);
    }
    let mut missing_run = events[2].clone();
    missing_run.data.as_object_mut().unwrap().remove("run");
    assert!(projection.apply_recorded(&missing_run).is_err());
    assert_eq!(projection, before);
}

#[test]
fn malformed_claim_fences_are_rejected_atomically() {
    let events = fixture();
    let mut projection = StateProjection::default();
    for event in &events[..2] {
        projection.apply_recorded(event).unwrap();
    }
    let before = projection.clone();
    // The business core must require the recorded run fences, even though the
    // hub intentionally does not interpret the opaque business data.
    for leases in [
        Value::Null,
        json!([]),
        json!([{ "name":"module:A", "holder":"R-1", "epoch":99 }]),
    ] {
        let mut claim = events[2].clone();
        claim.data["run"]["leases"] = leases;
        assert!(projection.apply_recorded(&claim).is_err());
        assert_eq!(projection, before);
    }
}

#[test]
fn prefix_high_water_ignores_stale_incoming_epochs_and_requires_main_summary() {
    let events = fixture();
    let prefix = Snapshot::from_events("project", &events, 7, 1).unwrap();
    assert_eq!(prefix.epoch_high_water, 2);
    assert_eq!(snapshot(&events).epoch_high_water, 3);
    assert!(Snapshot::from_events("project", &events, 11, 3).is_err());
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    hub.lease_acquire(acquire("main", "writer"), 1).unwrap();
    let mut stale = input("task.state", json!({"invalid": true}));
    stale.guards = vec![Fence {
        name: "main".into(),
        holder: "old".into(),
        epoch: u64::MAX,
    }];
    hub.append(stale, 2).unwrap();
    let events = hub.events(0);
    let snapshot = snapshot(&events);
    assert_eq!(snapshot.epoch_high_water, 1);
    snapshot.validate_archive("project", &events).unwrap();
    hub.lease_release(
        ReleaseLease {
            host: "host".into(),
            fences: vec![events[0].updated_leases()[0].fence()],
        },
        3,
    )
    .unwrap();
    assert!(Snapshot::from_events("project", &hub.events(0), 3, 1).is_err());
}

#[test]
fn journal_and_archive_bounds_require_contiguous_unique_input() {
    let events = fixture();
    assert!(Snapshot::from_events("", &events, 11, 1).is_err());
    assert!(Snapshot::from_events("project", &events, 12, 1).is_err());
    assert!(Snapshot::from_events("project", &events, 0, 0).is_err());
    for bad in [
        vec![events[1].clone()],
        vec![events[0].clone(), events[0].clone()],
        vec![events[0].clone(), events[2].clone()],
    ] {
        assert!(Snapshot::from_events("project", &bad, 1, 1).is_err());
        assert!(archive_events(&bad, 0, 3).is_err());
    }
    let mut duplicate = events.clone();
    duplicate[2].eid = duplicate[1].eid.clone();
    assert!(Snapshot::from_events("project", &duplicate, 11, 1).is_err());
    assert!(archive_events(&duplicate, 0, 11).is_err());
    assert!(archive_events(&events, 11, 10).is_err());
    assert!(archive_events(&events, 0, 12).is_err());
    assert!(archive_events(&events, 11, 11).unwrap().is_empty());
    assert_eq!(
        archive_events(&events[5..], 5, 11).unwrap(),
        archive_events(&events, 5, 11).unwrap()
    );
}

#[test]
fn state_json_has_no_receipts_and_heartbeat_ranges_are_merged() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["format_version"], 2);
    assert!(encoded.get("receipts").is_none());
    assert!(encoded.get("events").is_none());
    assert_eq!(
        snapshot.omitted,
        [
            SequenceRange { start: 4, end: 5 },
            SequenceRange { start: 10, end: 10 }
        ]
    );
    let text = encoded.to_string();
    assert!(!text.contains("lease_delta"));
    assert!(!text.contains("task.claimed"));
    assert!(!text.contains("expires_at"));
    let base = Snapshot::from_events("project", &events, 4, 1).unwrap();
    let extended = Snapshot::extend(Some(&base), &events[4..], 5, 1).unwrap();
    assert_eq!(extended.omitted, [SequenceRange { start: 4, end: 5 }]);
}

#[test]
fn archive_rejects_missing_business_duplicates_reordering_and_projection_mismatch() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let archive = archive_events(&events, 0, 11).unwrap();
    let mut missing = archive.clone();
    missing.remove(2);
    let mut duplicate = archive.clone();
    duplicate.insert(1, duplicate[0].clone());
    let mut reordered = archive.clone();
    reordered.swap(1, 2);
    // v2 has no eid receipts in state; only a duplicate eid can be disproved
    // from the archive itself. A fresh UUID cannot be authenticated by state.
    let mut duplicate_eid = archive.clone();
    duplicate_eid[1].eid = duplicate_eid[0].eid.clone();
    let mut altered = archive.clone();
    altered.last_mut().unwrap().data["task"]["state"] = json!("merged");
    for bad in [missing, duplicate, reordered, duplicate_eid, altered] {
        assert!(snapshot.validate_archive("project", &bad).is_err());
    }
    assert!(snapshot.validate_archive("other", &archive).is_err());
}

#[test]
fn metadata_rejects_unknown_format_invalid_epochs_and_v2_receipts() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let archive = archive_events(&events, 0, 11).unwrap();
    for change in 0..3 {
        let mut invalid = snapshot.clone();
        match change {
            0 => invalid.format_version += 1,
            1 => invalid.epoch_high_water = 1,
            _ => invalid.writer_epoch = 2,
        }
        assert!(invalid.validate_archive("project", &archive).is_err());
    }
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    encoded["receipts"] = json!({events[0].eid.clone(): 1});
    let invalid: Snapshot = serde_json::from_value(encoded).unwrap();
    assert!(invalid.validate_metadata().is_err());
    let encoded = serde_json::to_string(&snapshot).unwrap();
    let overflow = encoded.replace("\"through_seq\":11", "\"through_seq\":18446744073709551616");
    assert!(serde_json::from_str::<Snapshot>(&overflow).is_err());
}

#[test]
fn legacy_receipts_validate_duplicates_missing_beyond_and_normalize_to_v2() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let archive = archive_events(&events, 0, 11).unwrap();
    let mut legacy = serde_json::to_value(&snapshot).unwrap();
    legacy["format_version"] = json!(1);
    let receipts: std::collections::BTreeMap<_, _> =
        events.iter().map(|e| (e.eid.clone(), e.seq)).collect();
    legacy["receipts"] = json!(receipts);
    let old: Snapshot = serde_json::from_value(legacy.clone()).unwrap();
    old.validate_archive("project", &archive).unwrap();
    let mut normalized = old;
    normalized.normalize();
    assert_eq!(normalized, snapshot);
    assert!(serde_json::to_value(normalized)
        .unwrap()
        .get("receipts")
        .is_none());
    for change in 0..4 {
        let mut invalid = legacy.clone();
        match change {
            0 => invalid["receipts"]["duplicate"] = json!(1),
            1 => {
                invalid["receipts"]
                    .as_object_mut()
                    .unwrap()
                    .remove(&events[3].eid);
            }
            2 => invalid["receipts"]["beyond"] = json!(12),
            _ => {
                invalid["receipts"]
                    .as_object_mut()
                    .unwrap()
                    .remove(&events[2].eid);
            }
        }
        let invalid: Snapshot = serde_json::from_value(invalid).unwrap();
        assert!(invalid.validate_archive("project", &archive).is_err());
    }
    let encoded = serde_json::to_string(&legacy).unwrap();
    let duplicate_eid = encoded.replace(
        "\"receipts\":{",
        &format!("\"receipts\":{{\"{}\":1,", events[0].eid),
    );
    assert!(serde_json::from_str::<Snapshot>(&duplicate_eid).is_err());
}

#[test]
fn ranges_reject_overlap_inversion_zero_overflow_and_uncovered_sequences() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let archive = archive_events(&events, 0, 11).unwrap();
    for range in [
        SequenceRange { start: 0, end: 1 },
        SequenceRange { start: 5, end: 4 },
        SequenceRange { start: 4, end: 6 },
        SequenceRange { start: 12, end: 12 },
        SequenceRange {
            start: 11,
            end: u64::MAX,
        },
    ] {
        let mut invalid = snapshot.clone();
        invalid.omitted.push(range);
        assert!(invalid.validate_archive("project", &archive).is_err());
    }
    let mut overlap = snapshot.clone();
    overlap
        .recovery_gaps
        .push(SequenceRange { start: 4, end: 5 });
    assert!(overlap.validate_archive("project", &archive).is_err());
    let mut uncovered = snapshot.clone();
    uncovered.omitted.clear();
    assert!(uncovered.validate_archive("project", &archive).is_err());
}

#[test]
fn large_recovery_gap_is_validated_without_enumerating_missing_sequences() {
    let events = fixture();
    let mut snapshot = Snapshot::from_events("project", &events, 2, 1).unwrap();
    let mut archive = events[..2].to_vec();
    let jump = 1_u64 << 40;
    archive[1].seq += jump;
    snapshot.through_seq += jump;
    snapshot.recovery_gaps.push(SequenceRange {
        start: 2,
        end: jump + 1,
    });
    snapshot.validate_archive("project", &archive).unwrap();
    let mut inside_gap = archive;
    inside_gap[1].seq = jump;
    assert!(snapshot.validate_archive("project", &inside_gap).is_err());
}

#[test]
fn extend_preserves_seed_metadata_merges_omissions_and_rejects_invalid_tails() {
    let events = fixture();
    let base = Snapshot::from_events("project", &events, 4, 1).unwrap();
    assert_eq!(
        Snapshot::extend(Some(&base), &events[4..], 11, 1).unwrap(),
        snapshot(&events)
    );
    assert_eq!(Snapshot::extend(Some(&base), &[], 4, 1).unwrap(), base);
    let mut recovered = base.clone();
    let jump = 1_u64 << 32;
    recovered.through_seq += jump;
    recovered.epoch_high_water += jump;
    recovered.recovery_gaps.push(SequenceRange {
        start: 5,
        end: recovered.through_seq,
    });
    let mut tail = events[4..].to_vec();
    for event in &mut tail {
        event.seq += jump;
    }
    let extended = Snapshot::extend(Some(&recovered), &tail, jump + 11, 1).unwrap();
    assert_eq!(extended.epoch_high_water, recovered.epoch_high_water);
    assert_eq!(extended.recovery_gaps, recovered.recovery_gaps);
    assert_eq!(extended.projection, snapshot(&events).projection);
    let mut archive = archive_events(&events, 0, 4).unwrap();
    archive.extend(archive_events(&tail, recovered.through_seq, jump + 11).unwrap());
    extended.validate_archive("project", &archive).unwrap();
    assert!(Snapshot::extend(Some(&base), &events[5..], 11, 1).is_err());
    assert!(Snapshot::extend(Some(&base), &events[4..], 3, 1).is_err());
    assert!(Snapshot::extend(Some(&base), &events[4..], 12, 1).is_err());
    // Cross-window eid uniqueness is now established by the full archive.
    let mut duplicate = events[4..].to_vec();
    duplicate[1].eid = events[0].eid.clone();
    let duplicated = Snapshot::extend(Some(&base), &duplicate, 11, 1).unwrap();
    let mut archive = archive_events(&events, 0, 4).unwrap();
    archive.extend(archive_events(&duplicate, 4, 11).unwrap());
    assert!(duplicated.validate_archive("project", &archive).is_err());
    let mut invalid = base.clone();
    invalid.format_version += 1;
    assert!(Snapshot::extend(Some(&invalid), &events[4..], 11, 1).is_err());
    invalid = base;
    invalid.through_seq = u64::MAX;
    assert!(Snapshot::extend(Some(&invalid), &events[4..], u64::MAX, 1).is_err());
    assert!(Snapshot::extend(None, &events, 11, 1).is_err());
}

#[test]
fn legacy_renewals_with_business_data_are_preserved_in_archive() {
    for data in [
        json!({"run":"R-1"}),
        state("running", "running"),
        json!({"run":{"id":"R-1","state":"released"}}),
        json!({"run":"R-1","evidence":"new"}),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let fence = json!({"name":"main","holder":"writer","epoch":1});
        let event = |seq, kind, data, delta| {
            json!({
                "seq":seq,"eid":uuid::Uuid::now_v7().to_string(),"host":"host",
                "t":format!("1970-01-01T00:00:00.{seq:03}Z"),"actor":"process:test",
                "type":kind,"layer":"L4","obj":"project","data":data,
                "epoch":null,"stale":false,"cost_usd":null,"fence":null,"lease_delta":delta
            })
        };
        let grant = event(
            1,
            "lease.acquired",
            json!({"lease":"main"}),
            json!({
                "change":{"Acquire":{"names":["main"],"holder":"writer","ttl_ms":1000}},
                "updated":[{"name":"main","holder":"writer","epoch":1,"expires_at":1001}]
            }),
        );
        let renewal = event(
            2,
            "lease.renewed",
            data.clone(),
            json!({
                "change":{"Renew":{"fences":[fence],"ttl_ms":1000}},
                "updated":[{"name":"main","holder":"writer","epoch":1,"expires_at":1002}]
            }),
        );
        let legacy = json!({"version":1,"events":[grant,renewal]});
        let path = temporary.path().join("journal.json");
        let bytes = serde_json::to_vec(&legacy).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let hub = Hub::open(temporary.path()).unwrap();
        let events = hub.events(0);
        assert_eq!(events[1].data, data);
        let pure = data == json!({"run":"R-1"});
        assert_eq!(events[1].is_pure_heartbeat(), pure);
        let archive = archive_events(&events, 0, 2).unwrap();
        assert_eq!(archive.iter().any(|event| event.seq == 2), !pure);
        snapshot(&events)
            .validate_archive("project", &archive)
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
