use serde_json::{json, Value};
use towermd_core::hub::{Event, Fence, Hub, LeaseChange, NewEvent};
use towermd_core::snapshot::{
    archive_events, EventBuffer, SequenceRange, Snapshot, StateProjection,
};

fn input(kind: &str, data: Value) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:test".into(),
        kind: kind.into(),
        layer: "L4".into(),
        obj: "T-1".into(),
        data,
        fence: None,
    }
}

fn acquire(name: &str, holder: &str) -> Option<LeaseChange> {
    Some(LeaseChange::Acquire {
        names: vec![name.into()],
        holder: holder.into(),
        ttl_ms: 1000,
    })
}

fn state(task: &str, run: &str) -> Value {
    json!({
        "task": {"type": "Task", "id": "T-1", "state": task,
                 "future": {"nested": [null, {"opaque": true}]}},
        "run": {"type": "Run", "id": "R-1", "task": "T-1", "state": run,
                "leases": [], "cost_usd": null, "future": [1, null]}
    })
}

fn fixture() -> Vec<Event> {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    hub.append(
        input("lease.acquired", json!({"lease": "main"})),
        acquire("main", "writer"),
        1,
    )
    .unwrap();
    let claim = hub
        .append(
            input("task.claimed", state("running", "running")),
            acquire("module:A", "R-1"),
            2,
        )
        .unwrap();
    let fences: Vec<_> = claim
        .updated_leases()
        .iter()
        .map(|lease| lease.fence())
        .collect();
    for now in [3, 4] {
        hub.append(
            input("lease.renewed", json!({"run": "R-1"})),
            Some(LeaseChange::Renew {
                fences: fences.clone(),
                ttl_ms: 1000,
            }),
            now,
        )
        .unwrap();
    }
    hub.append(
        input("task.state", state("ready", "released")),
        Some(LeaseChange::Release { fences }),
        5,
    )
    .unwrap();
    let claim = hub
        .append(
            input("task.claimed", state("running", "running")),
            acquire("module:A", "R-1"),
            6,
        )
        .unwrap();
    hub.append(
        input("lease.renewed", json!({"run": "R-1"})),
        Some(LeaseChange::Renew {
            fences: claim
                .updated_leases()
                .iter()
                .map(|lease| lease.fence())
                .collect(),
            ttl_ms: 1000,
        }),
        7,
    )
    .unwrap();
    hub.append(input("task.state", state("ready", "released")), None, 2000)
        .unwrap();
    hub.events(0)
}

fn snapshot(events: &[Event]) -> Snapshot {
    Snapshot::from_events("project", events, events.last().unwrap().seq, 1).unwrap()
}

#[test]
fn claim_renew_release_and_expire_replay_across_archive_windows() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let mut archive = archive_events(&events, 0, 4).unwrap();
    archive.extend(archive_events(&events, 4, 8).unwrap());
    assert_eq!(
        archive.iter().map(|event| event.seq).collect::<Vec<_>>(),
        [1, 2, 5, 6, 8]
    );
    snapshot.validate_archive("project", &archive).unwrap();
    assert_eq!(
        snapshot.omitted,
        [
            SequenceRange { start: 3, end: 4 },
            SequenceRange { start: 7, end: 7 }
        ]
    );
    let mut replay = StateProjection::default();
    for event in &events[..5] {
        replay.apply_recorded(event).unwrap();
    }
    assert!(!replay.lease_summaries.contains_key("module:A"));
    for event in &events[5..] {
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
    let snapshot = Snapshot::from_events("project", &events, 2, 1).unwrap();
    let run = &snapshot.projection.objects["Run"]["R-1"];
    assert_eq!(
        run["leases"],
        json!(events[1]
            .updated_leases()
            .iter()
            .map(|lease| lease.fence())
            .collect::<Vec<_>>())
    );
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
fn heartbeat_requires_renew_delta_and_only_generated_data() {
    let events = fixture();
    assert!(events[2].is_pure_heartbeat());
    for data in [
        state("running", "running"),
        json!({"run": {"id": "R-1", "state": "released"}}),
        json!({"run": "R-1", "evidence": "new"}),
    ] {
        let mut changed = events.clone();
        changed[2].data = data;
        assert!(!changed[2].is_pure_heartbeat());
        assert!(archive_events(&changed, 0, 8)
            .unwrap()
            .iter()
            .any(|event| event.seq == 3));
    }
    let mut acquire = events[0].clone();
    acquire.kind = "lease.renewed".into();
    assert!(!acquire.is_pure_heartbeat());
    let mut no_delta = events[7].clone();
    no_delta.kind = "lease.renewed".into();
    no_delta.data = json!({"run": "R-1"});
    assert!(!no_delta.is_pure_heartbeat());
    let mut cost = events[2].clone();
    cost.cost_usd = Some(0.5);
    assert!(!cost.is_pure_heartbeat());
    let mut business = events.clone();
    business[2].kind = "task.state".into();
    business[2].data = state("verifying", "running");
    let snapshot = Snapshot::from_events("project", &business, 3, 1).unwrap();
    assert!(snapshot.omitted.is_empty());
    assert_eq!(
        snapshot.projection.objects["Task"]["T-1"]["state"],
        "verifying"
    );
    snapshot
        .validate_archive("project", &archive_events(&business, 0, 3).unwrap())
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
    stale.fence = Some(Fence {
        name: "main".into(),
        holder: "old".into(),
        epoch: u64::MAX,
    });
    let stale = hub.append(stale, None, 1).unwrap();
    assert!(stale.stale);
    projection.apply_recorded(&stale).unwrap();
    let future = hub
        .append(input("future.event", state("bad", "bad")), None, 2)
        .unwrap();
    projection.apply_recorded(&future).unwrap();
    assert_eq!(projection, before);
    let rebuilt = hub
        .append(input("hub.rebuilt", json!({})), None, 3)
        .unwrap();
    projection.apply_recorded(&rebuilt).unwrap();
    assert!(projection.lease_summaries.is_empty());
    assert_eq!(projection.objects, before.objects);
    let mut resumed = events[7].clone();
    resumed.data = state("ready", "released");
    projection.apply_recorded(&resumed).unwrap();
    assert_eq!(projection.objects["Run"]["R-1"]["state"], "released");
}

#[test]
fn malformed_task_and_run_are_rejected_atomically() {
    let events = fixture();
    let mut projection = StateProjection::default();
    projection.apply_recorded(&events[0]).unwrap();
    let before = projection.clone();
    for (field, key, value) in [
        ("task", "type", json!("FutureType")),
        ("task", "type", json!("Module")),
        ("run", "type", json!("Task")),
        ("run", "id", json!(null)),
        ("task", "id", json!("")),
    ] {
        let mut event = events[1].clone();
        event.data[field][key] = value;
        assert!(projection.apply_recorded(&event).is_err());
        assert_eq!(projection, before);
    }
    let mut claim_without_leases = events[7].clone();
    claim_without_leases.kind = "task.claimed".into();
    assert!(projection.apply_recorded(&claim_without_leases).is_err());
    assert_eq!(projection, before);
}

#[test]
fn prefix_high_water_ignores_stale_incoming_epochs_and_requires_main_summary() {
    let events = fixture();
    let prefix = Snapshot::from_events("project", &events, 5, 1).unwrap();
    assert_eq!(prefix.epoch_high_water, 2);
    assert_eq!(snapshot(&events).epoch_high_water, 3);
    assert!(Snapshot::from_events("project", &events, 8, 3).is_err());
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temporary.path()).unwrap();
    hub.append(
        input("lease.acquired", json!({})),
        acquire("main", "writer"),
        1,
    )
    .unwrap();
    let mut stale = input("task.state", json!({"invalid": true}));
    stale.fence = Some(Fence {
        name: "main".into(),
        holder: "old".into(),
        epoch: u64::MAX,
    });
    hub.append(stale, None, 2).unwrap();
    let events = hub.events(0);
    let snapshot = snapshot(&events);
    assert_eq!(snapshot.epoch_high_water, 1);
    snapshot.validate_archive("project", &events).unwrap();
    hub.append(
        input("lease.released", json!({})),
        Some(LeaseChange::Release {
            fences: vec![events[0].updated_leases()[0].fence()],
        }),
        3,
    )
    .unwrap();
    assert!(Snapshot::from_events("project", &hub.events(0), 3, 1).is_err());
}

#[test]
fn journal_and_archive_bounds_require_contiguous_unique_input() {
    let events = fixture();
    assert!(Snapshot::from_events("", &events, 8, 1).is_err());
    assert!(Snapshot::from_events("project", &events, 9, 1).is_err());
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
    assert!(Snapshot::from_events("project", &duplicate, 8, 1).is_err());
    assert!(archive_events(&duplicate, 0, 8).is_err());
    assert!(archive_events(&events, 8, 7).is_err());
    assert!(archive_events(&events, 0, 9).is_err());
    assert!(archive_events(&events, 8, 8).unwrap().is_empty());
    assert_eq!(
        archive_events(&events[4..], 4, 8).unwrap(),
        archive_events(&events, 4, 8).unwrap()
    );
}

#[test]
fn receipts_include_heartbeats_but_state_json_has_no_event_bodies() {
    let events = fixture();
    let snapshot = snapshot(&events);
    assert_eq!(snapshot.receipts.len(), events.len());
    for event in &events {
        assert_eq!(snapshot.receipts[&event.eid], event.seq);
    }
    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert!(encoded.get("events").is_none());
    let text = encoded.to_string();
    assert!(!text.contains("lease_delta"));
    assert!(!text.contains("task.claimed"));
    assert!(!text.contains("expires_at"));
}

#[test]
fn archive_rejects_missing_business_duplicates_reordering_and_projection_mismatch() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let archive = archive_events(&events, 0, 8).unwrap();
    let mut missing = archive.clone();
    missing.remove(2);
    let mut duplicate = archive.clone();
    duplicate.insert(1, duplicate[0].clone());
    let mut reordered = archive.clone();
    reordered.swap(1, 2);
    let mut wrong_receipt = archive.clone();
    wrong_receipt[1].eid = uuid::Uuid::now_v7().to_string();
    let mut altered = archive.clone();
    altered.last_mut().unwrap().data["task"]["state"] = json!("merged");
    for bad in [missing, duplicate, reordered, wrong_receipt, altered] {
        assert!(snapshot.validate_archive("project", &bad).is_err());
    }
    assert!(snapshot.validate_archive("other", &archive).is_err());
}

#[test]
fn metadata_rejects_duplicate_receipts_unknown_format_and_invalid_epochs() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let archive = archive_events(&events, 0, 8).unwrap();
    for change in 0..6 {
        let mut invalid = snapshot.clone();
        match change {
            0 => invalid.format_version += 1,
            1 => {
                invalid.receipts.insert("duplicate".into(), 1);
            }
            2 => {
                invalid.receipts.remove(&events[2].eid);
            }
            3 => {
                invalid.receipts.insert("beyond".into(), 9);
            }
            4 => invalid.epoch_high_water = 1,
            _ => invalid.writer_epoch = 2,
        }
        assert!(invalid.validate_archive("project", &archive).is_err());
    }
    let encoded = serde_json::to_string(&snapshot).unwrap();
    let duplicate_eid = encoded.replace(
        "\"receipts\":{",
        &format!("\"receipts\":{{\"{}\":1,", events[0].eid),
    );
    assert!(serde_json::from_str::<Snapshot>(&duplicate_eid).is_err());
    let overflow = encoded.replace("\"through_seq\":8", "\"through_seq\":18446744073709551616");
    assert!(serde_json::from_str::<Snapshot>(&overflow).is_err());
}

#[test]
fn ranges_reject_overlap_inversion_zero_overflow_and_uncovered_sequences() {
    let events = fixture();
    let snapshot = snapshot(&events);
    let archive = archive_events(&events, 0, 8).unwrap();
    for range in [
        SequenceRange { start: 0, end: 1 },
        SequenceRange { start: 4, end: 3 },
        SequenceRange { start: 3, end: 5 },
        SequenceRange { start: 9, end: 9 },
        SequenceRange {
            start: 8,
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
        .push(SequenceRange { start: 3, end: 4 });
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
    *snapshot.receipts.get_mut(&archive[1].eid).unwrap() = archive[1].seq;
    snapshot.recovery_gaps.push(SequenceRange {
        start: 2,
        end: jump + 1,
    });
    snapshot.validate_archive("project", &archive).unwrap();
    snapshot.receipts.insert("inside-gap".into(), jump);
    assert!(snapshot.validate_archive("project", &archive).is_err());
}

#[test]
fn buffer_advances_only_contiguous_tail_after_confirmed_base() {
    let mut events = fixture();
    events[0].seq = 102;
    events[1].seq = 103;
    let mut buffer = EventBuffer::new(101);
    buffer.push(events[1].clone()).unwrap();
    assert_eq!(buffer.through_seq(), 101);
    assert!(buffer.events().is_empty());
    buffer.push(events[0].clone()).unwrap();
    assert_eq!(buffer.through_seq(), 103);
    assert_eq!(buffer.events(), events[..2]);
    buffer.push(events[0].clone()).unwrap();
    assert_eq!(buffer.events().len(), 2);
}

#[test]
fn buffer_rejects_seq_eid_conflicts_and_base_replays_without_partial_update() {
    let events = fixture();
    let mut buffer = EventBuffer::new(0);
    buffer.push(events[1].clone()).unwrap();
    let mut seq_conflict = events[0].clone();
    seq_conflict.seq = 2;
    let mut eid_conflict = events[1].clone();
    eid_conflict.seq = 3;
    let mut body_conflict = events[1].clone();
    body_conflict.data = json!({});
    for bad in [seq_conflict, eid_conflict, body_conflict] {
        assert!(buffer.push(bad).is_err());
        assert_eq!(buffer.through_seq(), 0);
    }
    buffer.push(events[0].clone()).unwrap();
    assert_eq!(buffer.through_seq(), 2);
    let mut at_base = EventBuffer::new(1);
    assert!(at_base.push(events[0].clone()).is_err());
    let mut maximum = EventBuffer::new(u64::MAX - 1);
    let mut last = events[0].clone();
    last.seq = u64::MAX;
    maximum.push(last.clone()).unwrap();
    assert_eq!(maximum.through_seq(), u64::MAX);
    assert_eq!(maximum.events(), vec![last]);
}

#[test]
fn extend_preserves_seed_metadata_merges_omissions_and_rejects_invalid_tails() {
    let events = fixture();
    let base = Snapshot::from_events("project", &events, 3, 1).unwrap();
    assert_eq!(
        Snapshot::extend(Some(&base), &events[3..], 8, 1).unwrap(),
        snapshot(&events)
    );
    assert_eq!(Snapshot::extend(Some(&base), &[], 3, 1).unwrap(), base);
    let mut recovered = base.clone();
    let jump = 1_u64 << 32;
    recovered.through_seq += jump;
    recovered.epoch_high_water += jump;
    recovered.recovery_gaps.push(SequenceRange {
        start: 4,
        end: recovered.through_seq,
    });
    let mut tail = events[3..].to_vec();
    for event in &mut tail {
        event.seq += jump;
    }
    let extended = Snapshot::extend(Some(&recovered), &tail, jump + 8, 1).unwrap();
    assert_eq!(extended.epoch_high_water, recovered.epoch_high_water);
    assert_eq!(extended.recovery_gaps, recovered.recovery_gaps);
    assert_eq!(extended.receipts.len(), events.len());
    assert_eq!(extended.projection, snapshot(&events).projection);
    let mut archive = archive_events(&events, 0, 3).unwrap();
    archive.extend(archive_events(&tail, recovered.through_seq, jump + 8).unwrap());
    extended.validate_archive("project", &archive).unwrap();
    assert!(Snapshot::extend(Some(&base), &events[4..], 8, 1).is_err());
    assert!(Snapshot::extend(Some(&base), &events[3..], 2, 1).is_err());
    assert!(Snapshot::extend(Some(&base), &events[3..], 9, 1).is_err());
    let mut duplicate = events[3..].to_vec();
    duplicate[0].eid = events[0].eid.clone();
    assert!(Snapshot::extend(Some(&base), &duplicate, 8, 1).is_err());
    let mut invalid = base.clone();
    invalid.format_version += 1;
    assert!(Snapshot::extend(Some(&invalid), &events[3..], 8, 1).is_err());
    invalid = base.clone();
    invalid.through_seq = u64::MAX;
    assert!(Snapshot::extend(Some(&invalid), &events[3..], u64::MAX, 1).is_err());
    assert!(Snapshot::extend(None, &events, 8, 1).is_err());
}
