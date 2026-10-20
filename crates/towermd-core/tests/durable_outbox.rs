use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use towermd_core::durable_outbox::{DurableOutbox, OutboxError};
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
use uuid::Uuid;

fn input() -> NewEvent {
    NewEvent {
        eid: Uuid::now_v7().to_string(),
        host: "host-a".into(),
        actor: "human:local".into(),
        kind: "baseline.approved".into(),
        layer: "L2".into(),
        obj: "B0".into(),
        data: json!({"draft": "draft-a", "approved": true}),
        guards: vec![],
    }
}

fn path(common: &Path, kind: &str, eid: &str) -> PathBuf {
    common
        .join("towermd/outbox")
        .join(kind)
        .join(format!("{}.json", hex::encode(Sha256::digest(eid))))
}

fn lookup(hub: &mut Hub, seq: u64, eid: &str) -> Result<Option<Event>, HubError> {
    Ok(hub
        .events(seq.saturating_sub(1))
        .into_iter()
        .find(|e| e.seq == seq && e.eid == eid))
}

#[derive(Clone, Copy)]
enum Behavior {
    Normal,
    Reject,
    UncertainAfterAppend,
    Duplicate,
}

struct TestHub {
    inner: Hub,
    common: PathBuf,
    behavior: Behavior,
    calls: usize,
}

impl TestHub {
    fn new(common: &Path, hub_dir: &Path, behavior: Behavior) -> Self {
        Self {
            inner: Hub::open(hub_dir).unwrap(),
            common: common.to_owned(),
            behavior,
            calls: 0,
        }
    }
}

impl HubProtocol for TestHub {
    fn append(&mut self, event: NewEvent, now: u64) -> Result<AppendReceipt, HubError> {
        self.calls += 1;
        // The complete pending intent must exist before crossing the hub boundary.
        let bytes = fs::read(path(&self.common, "pending", &event.eid)).unwrap();
        let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(record["payload"], serde_json::to_value(&event).unwrap());
        match self.behavior {
            Behavior::Normal => self.inner.append(event, now),
            Behavior::Reject => Err(HubError::Invalid("rejected".into())),
            Behavior::UncertainAfterAppend => {
                self.inner.append(event, now)?;
                Err(HubError::Uncertain(
                    "connection lost after durable append".into(),
                ))
            }
            Behavior::Duplicate => {
                let seq = match self.inner.append(event, now)? {
                    AppendReceipt::Event(e) => e.seq,
                    AppendReceipt::Duplicate { seq } => seq,
                };
                Ok(AppendReceipt::Duplicate { seq })
            }
        }
    }
    fn subscribe(&mut self, after: u64, now: u64) -> Result<SubscriptionBatch, HubError> {
        self.inner.subscribe(after, now)
    }
    fn lease_acquire(&mut self, r: AcquireLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_acquire(r, now)
    }
    fn lease_renew(&mut self, r: RenewLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_renew(r, now)
    }
    fn lease_release(&mut self, r: ReleaseLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_release(r, now)
    }
    fn snapshot_ack(
        &mut self,
        ack: SnapshotAck,
        checkpoint: serde_json::Value,
    ) -> Result<(), HubError> {
        HubProtocol::snapshot_ack(&mut self.inner, ack, checkpoint)
    }
    fn rebuild(&mut self, seed: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        self.inner.rebuild(seed)
    }
    fn contiguous_seq(&self) -> u64 {
        self.inner.contiguous_seq()
    }
}

fn test_lookup(hub: &mut TestHub, seq: u64, eid: &str) -> Result<Option<Event>, HubError> {
    lookup(&mut hub.inner, seq, eid)
}

#[test]
fn complete_receipt_survives_restart_and_retries_do_not_append_or_rewrite() {
    let common = TempDir::new().unwrap();
    let hub_dir = TempDir::new().unwrap();
    let mut hub = TestHub::new(common.path(), hub_dir.path(), Behavior::Normal);
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    let input = input();
    let event = outbox
        .deliver(&mut hub, input.clone(), 1000, test_lookup)
        .unwrap();
    let receipt_path = path(common.path(), "confirmed", &input.eid);
    let bytes = fs::read(&receipt_path).unwrap();
    assert_eq!(outbox.pending().unwrap(), vec![]);
    assert_eq!(outbox.confirmed().unwrap(), vec![event.clone()]);
    drop(outbox);
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    assert_eq!(
        outbox
            .deliver(&mut hub, input.clone(), 9000, |_, _, _| panic!("no lookup"))
            .unwrap(),
        event
    );
    assert_eq!(hub.calls, 1);
    assert_eq!(fs::read(receipt_path).unwrap(), bytes);
    assert_eq!(outbox.confirmed_by_eid(&input.eid).unwrap(), Some(event));
}

#[test]
fn hub_invalid_and_uncertain_are_distinct_and_neither_certifies_pending() {
    let common = TempDir::new().unwrap();
    let hub_dir = TempDir::new().unwrap();
    let mut hub = TestHub::new(common.path(), hub_dir.path(), Behavior::Reject);
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    let input = input();
    assert!(matches!(
        outbox.deliver(&mut hub, input.clone(), 1000, test_lookup),
        Err(OutboxError::Hub(HubError::Invalid(_)))
    ));
    assert_eq!(outbox.pending().unwrap(), vec![input.clone()]);
    assert!(outbox.confirmed().unwrap().is_empty());
    hub.behavior = Behavior::UncertainAfterAppend;
    assert!(matches!(
        outbox.deliver(&mut hub, input.clone(), 1000, test_lookup),
        Err(OutboxError::Hub(HubError::Uncertain(_)))
    ));
    let original = hub.inner.events(0)[0].clone();
    assert!(outbox.confirmed().unwrap().is_empty());
    drop(outbox);
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    hub.behavior = Behavior::Duplicate;
    assert_eq!(
        outbox.deliver(&mut hub, input, 9000, test_lookup).unwrap(),
        original
    );
    assert_eq!(hub.inner.events(0), vec![original]);
}

#[test]
fn duplicate_requires_original_full_envelope_and_preserves_unknown_fields() {
    let common = TempDir::new().unwrap();
    let hub_dir = TempDir::new().unwrap();
    let mut hub = TestHub::new(common.path(), hub_dir.path(), Behavior::Duplicate);
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    let input = input();
    assert!(matches!(
        outbox.deliver(&mut hub, input.clone(), 1000, |_, _, _| Ok(None)),
        Err(OutboxError::Uncertain(_))
    ));
    assert_eq!(outbox.pending().unwrap(), vec![input.clone()]);
    assert!(outbox.confirmed().unwrap().is_empty());
    let mut raw = serde_json::to_value(&hub.inner.events(0)[0]).unwrap();
    raw["future_receipt_field"] = json!({"preserved": true});
    let archive: Event = serde_json::from_value(raw.clone()).unwrap();
    let event = outbox
        .deliver(&mut hub, input, 5000, |_, _, _| Ok(Some(archive.clone())))
        .unwrap();
    assert_eq!(serde_json::to_value(event).unwrap(), raw);
    drop(outbox);
    assert_eq!(
        serde_json::to_value(
            DurableOutbox::open(common.path())
                .unwrap()
                .confirmed()
                .unwrap()[0]
                .clone()
        )
        .unwrap(),
        raw
    );
}

#[test]
fn acknowledged_hub_compaction_recovers_original_from_archive_without_new_event() {
    let common = TempDir::new().unwrap();
    let hub_dir = TempDir::new().unwrap();
    let mut hub = TestHub::new(
        common.path(),
        hub_dir.path(),
        Behavior::UncertainAfterAppend,
    );
    let lease = hub
        .lease_acquire(
            AcquireLease {
                host: "host-a".into(),
                names: vec!["main".into()],
                holder: "daemon-a".into(),
                ttl_ms: 100,
            },
            1000,
        )
        .unwrap();
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    let mut input = input();
    input.guards = vec![lease.leases[0].fence()];
    assert!(outbox
        .deliver(&mut hub, input.clone(), 1001, test_lookup)
        .is_err());
    let original = hub.inner.events(1)[0].clone();
    hub.snapshot_ack(
        SnapshotAck {
            commit: "a".repeat(40),
            seq: original.seq,
            epoch: lease.leases[0].epoch,
        },
        json!({"archive": "external"}),
    )
    .unwrap();
    assert!(hub.inner.events(0).is_empty());
    drop(outbox);
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    hub.behavior = Behavior::Normal;
    assert!(matches!(
        outbox.deliver(&mut hub, input.clone(), 5000, test_lookup),
        Err(OutboxError::Uncertain(_))
    ));
    assert_eq!(hub.inner.last_seq(), original.seq);
    let recovered = outbox
        .deliver(&mut hub, input, 5000, |_, seq, eid| {
            assert_eq!(seq, original.seq);
            assert_eq!(eid, original.eid);
            Ok(Some(original.clone()))
        })
        .unwrap();
    assert_eq!(recovered, original);
    assert!(!recovered.stale);
    assert_eq!(hub.inner.last_seq(), original.seq);
    assert_eq!(
        outbox.confirmed_by_eid(&original.eid).unwrap(),
        Some(original)
    );
}

#[test]
fn same_id_different_intent_is_rejected_before_hub_even_after_confirmation() {
    let common = TempDir::new().unwrap();
    let hub_dir = TempDir::new().unwrap();
    let mut hub = TestHub::new(common.path(), hub_dir.path(), Behavior::Reject);
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    let input = input();
    let _ = outbox.deliver(&mut hub, input.clone(), 1000, test_lookup);
    let mut changed = input.clone();
    changed.data["draft"] = json!("other");
    assert!(matches!(
        outbox.deliver(&mut hub, changed.clone(), 1000, test_lookup),
        Err(OutboxError::Invalid(_))
    ));
    assert_eq!(hub.calls, 1);
    hub.behavior = Behavior::Normal;
    outbox.deliver(&mut hub, input, 1000, test_lookup).unwrap();
    assert!(matches!(
        outbox.deliver(&mut hub, changed, 1000, test_lookup),
        Err(OutboxError::Invalid(_))
    ));
    assert_eq!(hub.calls, 2);
}

#[test]
fn duplicate_lookup_checks_every_intent_field_and_sequence() {
    for field in [
        "eid", "host", "actor", "type", "layer", "obj", "data", "guards", "seq",
    ] {
        let common = TempDir::new().unwrap();
        let hub_dir = TempDir::new().unwrap();
        let mut hub = TestHub::new(common.path(), hub_dir.path(), Behavior::Duplicate);
        let mut outbox = DurableOutbox::open(common.path()).unwrap();
        let input = input();
        let result = outbox.deliver(&mut hub, input.clone(), 1000, |hub, _, _| {
            let mut raw = serde_json::to_value(&hub.inner.events(0)[0]).unwrap();
            raw[field] = match field {
                "seq" => json!(42),
                "data" => json!({"different": true}),
                "guards" => json!([{"name":"main","holder":"other","epoch":1}]),
                _ => json!("different"),
            };
            Ok(Some(serde_json::from_value(raw).unwrap()))
        });
        assert!(matches!(result, Err(OutboxError::Corrupt(_))), "{field}");
        assert!(outbox.confirmed().unwrap().is_empty());
        assert_eq!(outbox.pending().unwrap(), vec![input]);
    }
}

#[test]
fn stale_receipt_is_durable_but_never_certified_or_replaced_by_retry() {
    let common = TempDir::new().unwrap();
    let hub_dir = TempDir::new().unwrap();
    let mut hub = Hub::open(hub_dir.path()).unwrap();
    let lease = hub
        .lease_acquire(
            AcquireLease {
                host: "host-a".into(),
                names: vec!["main".into()],
                holder: "daemon-a".into(),
                ttl_ms: 10,
            },
            1000,
        )
        .unwrap();
    let mut input = input();
    input.guards = vec![lease.leases[0].fence()];
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    let original = match outbox.deliver(&mut hub, input.clone(), 1011, lookup) {
        Err(OutboxError::Stale(e)) => *e,
        other => panic!("expected stale, got {other:?}"),
    };
    assert!(original.stale);
    assert!(outbox.pending().unwrap().is_empty());
    assert_eq!(outbox.confirmed().unwrap(), vec![original.clone()]);
    let before = hub.last_seq();
    drop(outbox);
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    match outbox.deliver(&mut hub, input, 9999, lookup) {
        Err(OutboxError::Stale(e)) => assert_eq!(*e, original),
        other => panic!("expected original stale, got {other:?}"),
    }
    assert_eq!(hub.last_seq(), before);
}

#[test]
fn owner_lock_and_missing_or_corrupt_records_fail_closed() {
    for corruption in [
        "pending_missing",
        "confirmed_broken",
        "digest",
        "directory_missing",
        "filename",
    ] {
        let common = TempDir::new().unwrap();
        let hub_dir = TempDir::new().unwrap();
        let mut hub = Hub::open(hub_dir.path()).unwrap();
        let mut outbox = DurableOutbox::open(common.path()).unwrap();
        assert!(matches!(
            DurableOutbox::open(common.path()),
            Err(OutboxError::Uncertain(_))
        ));
        let input = input();
        outbox
            .deliver(&mut hub, input.clone(), 1000, lookup)
            .unwrap();
        drop(outbox);
        let pending = path(common.path(), "pending", &input.eid);
        let confirmed = path(common.path(), "confirmed", &input.eid);
        match corruption {
            "pending_missing" => fs::remove_file(pending).unwrap(),
            "confirmed_broken" => fs::write(confirmed, b"{").unwrap(),
            "digest" => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&confirmed).unwrap()).unwrap();
                value["payload"]["t"] = json!("2020-01-01T00:00:00Z");
                fs::write(confirmed, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "directory_missing" => {
                fs::remove_dir_all(common.path().join("towermd/outbox/confirmed")).unwrap()
            }
            "filename" => {
                fs::rename(&confirmed, path(common.path(), "confirmed", "different")).unwrap()
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(
                DurableOutbox::open(common.path()),
                Err(OutboxError::Corrupt(_))
            ),
            "{corruption}"
        );
    }
}

#[test]
fn filesystem_symlink_and_unsafe_ids_cannot_escape_outbox() {
    let common = TempDir::new().unwrap();
    let hub_dir = TempDir::new().unwrap();
    let mut hub = Hub::open(hub_dir.path()).unwrap();
    let mut outbox = DurableOutbox::open(common.path()).unwrap();
    let mut input = input();
    input.eid = "../../escape".into();
    assert!(outbox.deliver(&mut hub, input, 1000, lookup).is_err());
    assert!(outbox.pending().unwrap().is_empty());
    assert_eq!(hub.last_seq(), 0);
    drop(outbox);
    #[cfg(unix)]
    {
        let outside = TempDir::new().unwrap();
        let confirmed = common.path().join("towermd/outbox/confirmed");
        fs::remove_dir(&confirmed).unwrap();
        std::os::unix::fs::symlink(outside.path(), &confirmed).unwrap();
        assert!(matches!(
            DurableOutbox::open(common.path()),
            Err(OutboxError::Corrupt(_))
        ));
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
