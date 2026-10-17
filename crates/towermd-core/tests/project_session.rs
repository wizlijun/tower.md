mod support;
use serde_json::json;
use std::sync::Mutex;
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
use towermd_core::project_session::ProjectSession;
use towermd_core::{catalog::Catalog, recovery_outbox::RecoveryInputs};
fn context(root: &std::path::Path) -> (Catalog, RecoveryInputs) {
    fn git(root: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    git(root, &["init", "-qb", "main"]);
    towermd_core::init::init(root, &towermd_core::init::InitOptions { docs_dir: None }).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "session fixture"]);
    let catalog = Catalog::load(root).unwrap();
    let inputs = RecoveryInputs::read(&root.join("runtime-outbox"), &catalog).unwrap();
    (catalog, inputs)
}
#[test]
fn no_change_status_and_tick_apply_nothing_and_reopen_restores_tail() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let mut hub = Hub::open(&dir.path().join("hub")).unwrap();
    HubProtocol::append(&mut hub,NewEvent{eid:uuid::Uuid::now_v7().to_string(),host:"host".into(),actor:"process:test".into(),kind:"task.state".into(),layer:"L4".into(),obj:"T".into(),data:json!({"task":{"type":"Task","id":"T","title":"Task","module":"A","state":"ready"}}),guards:vec![]},1).unwrap();
    let session = Mutex::new(ProjectSession::new(Box::new(hub), &catalog, &inputs, 1).unwrap());
    ProjectSession::sync_shared(&session, &catalog, &inputs, 1).unwrap();
    let count = session.lock().unwrap().applied_count();
    for _ in 0..200 {
        ProjectSession::sync_shared(&session, &catalog, &inputs, 1).unwrap();
    }
    assert_eq!(session.lock().unwrap().applied_count(), count);
    let projection = session.lock().unwrap().projection().unwrap().clone();
    drop(session);
    let session = Mutex::new(
        ProjectSession::new(
            Box::new(Hub::open(&dir.path().join("hub")).unwrap()),
            &catalog,
            &inputs,
            1,
        )
        .unwrap(),
    );
    ProjectSession::sync_shared(&session, &catalog, &inputs, 1).unwrap();
    assert_eq!(*session.lock().unwrap().projection().unwrap(), projection);
}
#[test]
fn invalid_typed_business_event_leaves_both_projection_and_cursor_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let session =
        Mutex::new(ProjectSession::open(&dir.path().join("hub"), &catalog, &inputs, 1).unwrap());
    let before = session.lock().unwrap().projection().unwrap().clone();
    session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:test".into(),
                kind: "task.state".into(),
                layer: "L4".into(),
                obj: "T".into(),
                data: json!({"task":{"type":"Task","id":"T","state":"running"}}),
                guards: vec![],
            },
            1,
        )
        .unwrap();
    assert!(ProjectSession::sync_shared(&session, &catalog, &inputs, 1).is_err());
    assert_eq!(*session.lock().unwrap().projection().unwrap(), before);
    assert_eq!(session.lock().unwrap().applied_seq(), 0);
    assert_eq!(session.lock().unwrap().applied_count(), 0);
}

#[test]
fn constructor_rejects_unproved_canonical_business_before_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let mut hub = Hub::open(&dir.path().join("hub")).unwrap();
    hub.append(NewEvent {
        eid: uuid::Uuid::now_v7().to_string(), host: "host".into(),
        actor: "process:towermd-recovery".into(), kind: "merge.done".into(),
        layer: "L5".into(), obj: "T".into(), guards: vec![],
        data: json!({"task":{"type":"Task","id":"T","title":"Task","module":"A","state":"merged"},"commit":"a".repeat(40),"proof":{"schema":1}}),
    }, 1).unwrap();
    assert!(
        ProjectSession::new(Box::new(hub), &catalog, &inputs, 1).is_err(),
        "unproved canonical must not produce a valid Session"
    );
}

#[test]
fn constructor_rejects_raw_task_merged_declaration() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let mut hub = Hub::open(&dir.path().join("hub")).unwrap();
    hub.append(NewEvent {
        eid: uuid::Uuid::now_v7().to_string(), host: "host".into(),
        actor: "process:test".into(), kind: "task.state".into(),
        layer: "L5".into(), obj: "T".into(), guards: vec![],
        data: json!({"task":{"type":"Task","id":"T","title":"Task","module":"A","state":"merged"}}),
    }, 1).unwrap();
    assert!(
        ProjectSession::new(Box::new(hub), &catalog, &inputs, 1).is_err(),
        "raw merged cannot certify completion"
    );
}

#[test]
fn canonical_merge_applies_task_run_and_evidence_identically() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, _) = context(dir.path());
    let mut hub = Hub::open(&dir.path().join("hub")).unwrap();
    hub.append(NewEvent {eid:uuid::Uuid::now_v7().to_string(),host:"host".into(),actor:"process:towermd-recovery".into(),kind:"merge.done".into(),layer:"L5".into(),obj:"T".into(),guards:vec![],data:json!({"task":{"type":"Task","id":"T","title":"Task","module":"A","state":"merged"},"run":{"type":"Run","id":"R","task":"T","host":"host","worktree":"/tmp/task","branch":"t/T","state":"ended","leases":[],"cost_usd":null,"task_card":{}},"evidence":{"type":"Evidence","id":"E","task":"T","run":"R","tree":"a".repeat(40)}})},1).unwrap();
    let event = hub.subscribe(0, 1).unwrap().events.remove(0);
    let mut projection = towermd_core::snapshot::StateProjection::default();
    projection.apply_recorded(&event).unwrap();
    assert_eq!(projection.objects["Task"]["T"]["state"], "merged");
    assert_eq!(projection.objects["Run"]["R"]["state"], "ended");
    assert_eq!(projection.objects["Evidence"]["E"]["tree"], "a".repeat(40));
    let mut tasks = towermd_core::tasks::TaskService::default();
    tasks.apply(&event).unwrap();
    let mut restored = towermd_core::tasks::TaskService::default();
    restored.restore(&projection).unwrap();
    let status = tasks
        .execute(
            &mut hub,
            &catalog,
            "host",
            towermd_core::tasks::TaskCommand::Status,
            None,
            1,
        )
        .unwrap();
    let replay = restored
        .execute(
            &mut hub,
            &catalog,
            "host",
            towermd_core::tasks::TaskCommand::Status,
            None,
            1,
        )
        .unwrap();
    assert_eq!(status, replay);
}

struct FailAck {
    inner: Hub,
    fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl HubProtocol for FailAck {
    fn append(&mut self, e: NewEvent, n: u64) -> Result<AppendReceipt, HubError> {
        self.inner.append(e, n)
    }
    fn subscribe(&mut self, a: u64, n: u64) -> Result<SubscriptionBatch, HubError> {
        self.inner.subscribe(a, n)
    }
    fn lease_acquire(&mut self, r: AcquireLease, n: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_acquire(r, n)
    }
    fn lease_renew(&mut self, r: RenewLease, n: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_renew(r, n)
    }
    fn lease_release(&mut self, r: ReleaseLease, n: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_release(r, n)
    }
    fn snapshot_ack(&mut self, a: SnapshotAck, c: serde_json::Value) -> Result<(), HubError> {
        if a.seq > 101 && self.fail.swap(false, std::sync::atomic::Ordering::SeqCst) {
            Err(HubError::Io("actual ack failure".into()))
        } else {
            HubProtocol::snapshot_ack(&mut self.inner, a, c)
        }
    }
    fn rebuild(&mut self, s: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        self.inner.rebuild(s)
    }
    fn contiguous_seq(&self) -> u64 {
        self.inner.contiguous_seq()
    }
}
#[test]
fn older_ack_floor_is_proved_from_reachable_snapshot_history() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let path = catalog.common_dir.join("towermd");
    let mut raw = Hub::open(&path).unwrap();
    for _ in 0..100 {
        raw.append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:test".into(),
                kind: "future.audit".into(),
                layer: "L5".into(),
                obj: "T".into(),
                data: json!({}),
                guards: vec![],
            },
            1,
        )
        .unwrap();
    }
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let session = Mutex::new(
        ProjectSession::new(
            Box::new(FailAck {
                inner: raw,
                fail: fail.clone(),
            }),
            &catalog,
            &inputs,
            1,
        )
        .unwrap(),
    );
    let writer = towermd_core::snapshot_service::SnapshotWriter::default();
    let first = writer
        .write(&session, &catalog, "host", true, || Ok(1))
        .unwrap();
    assert_eq!(first.seq, 101);
    session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:test".into(),
                kind: "future.audit".into(),
                layer: "L5".into(),
                obj: "T".into(),
                data: json!({"seq":103}),
                guards: vec![],
            },
            2,
        )
        .unwrap();
    fail.store(true, std::sync::atomic::Ordering::SeqCst);
    let error = writer
        .write(&session, &catalog, "host", true, || Ok(2))
        .unwrap_err();
    assert!(error.contains("actual ack failure"));
    assert_eq!(session.lock().unwrap().snapshot_ack().unwrap().seq, 101);
    let saved = towermd_core::snapshot_service::read_snapshot(&catalog)
        .unwrap()
        .unwrap();
    assert_eq!(saved.snapshot.through_seq, 103);
    drop(session);
    let reopened = ProjectSession::open(&path, &catalog, &inputs, 2).unwrap();
    assert_eq!(reopened.snapshot_ack().unwrap().seq, 101);
    assert_eq!(reopened.applied_seq(), 104);
    drop(reopened);
    let tree = support::git(
        &catalog.root,
        &["rev-parse", &format!("{}^{{tree}}", saved.commit)],
    );
    let unreachable = support::git(
        &catalog.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &catalog.revision,
            "-m",
            "replacement without old ack ancestor\n\nTowermd-Snapshot: 103\nTowermd-Epoch: 1",
        ],
    );
    support::git(
        &catalog.root,
        &["update-ref", "refs/towermd/snapshot", &unreachable],
    );
    assert!(
        ProjectSession::open(&path, &catalog, &inputs, 2).is_err(),
        "unreachable old ack cannot mint a floor proof"
    );
}

fn unproved_merge() -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:towermd-recovery".into(),
        kind: "merge.done".into(),
        layer: "L5".into(),
        obj: "T".into(),
        guards: vec![],
        data: json!({"task":{"type":"Task","id":"T","title":"Task","module":"A","state":"merged"},"commit":"a".repeat(40),"proof":{"schema":1}}),
    }
}
#[test]
fn canonical_tail_is_rejected_by_shared_sync_status_subscription_writer_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let path = catalog.common_dir.join("towermd");
    let session = Mutex::new(ProjectSession::open(&path, &catalog, &inputs, 1).unwrap());
    session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(unproved_merge(), 2)
        .unwrap();
    let before = session.lock().unwrap().projection().unwrap().clone();
    assert!(ProjectSession::sync_shared(&session, &catalog, &inputs, 2).is_err());
    assert_eq!(*session.lock().unwrap().projection().unwrap(), before);
    assert_eq!(session.lock().unwrap().applied_seq(), 0);
    let status =
        ProjectSession::read_status_shared(&session, &catalog, &inputs, "host", None, 2).unwrap();
    assert_eq!(status["seq"], 0);
    assert_eq!(status["pending_verification"], true);
    assert_eq!(status["tasks"], json!([]));
    assert!(ProjectSession::subscribe_shared(&session, &catalog, &inputs, 0, 2).is_err());
    let refs = support::git(&catalog.root, &["show-ref"]);
    assert!(towermd_core::snapshot_service::SnapshotWriter::default()
        .write(&session, &catalog, "host", true, || Ok(2))
        .is_err());
    assert_eq!(support::git(&catalog.root, &["show-ref"]), refs);
    drop(session);
    assert!(ProjectSession::open(&path, &catalog, &inputs, 2).is_err());
}
#[test]
fn opaque_snapshot_required_cannot_restore_unproved_projection() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let path = catalog.common_dir.join("towermd");
    let session = Mutex::new(ProjectSession::open(&path, &catalog, &inputs, 1).unwrap());
    session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .lease_acquire(
            AcquireLease {
                host: "host".into(),
                names: vec!["main".into()],
                holder: "snapshot:host".into(),
                ttl_ms: 1000,
            },
            1,
        )
        .unwrap();
    let events = session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(0, 1)
        .unwrap()
        .events;
    let mut checkpoint =
        towermd_core::snapshot::Snapshot::from_events(catalog.config.project_id(), &events, 1, 1)
            .unwrap();
    checkpoint.through_seq = 1 + (1 << 32);
    checkpoint.epoch_high_water = 1 + (1 << 32);
    checkpoint
        .recovery_gaps
        .push(towermd_core::snapshot::SequenceRange {
            start: 2,
            end: 1 + (1 << 32),
        });
    checkpoint
        .projection
        .objects
        .entry("Task".into())
        .or_default()
        .insert("T".into(), json!({"type":"Task","id":"T","state":"merged"}));
    session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .rebuild(RebuildSeed {
            final_snapshot: None,
            source_seq: 1,
            source_epoch: 1,
            seq_boundary: 1 + (1 << 32),
            epoch_boundary: 1 + (1 << 32),
            checkpoint: serde_json::to_value(checkpoint).unwrap(),
            receipts: Default::default(),
            tail: vec![],
        })
        .unwrap();
    assert!(ProjectSession::sync_shared(&session, &catalog, &inputs, 2).is_err());
    assert_eq!(session.lock().unwrap().applied_seq(), 0);
    assert!(session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .is_empty());
    drop(session);
    assert!(ProjectSession::open(&path, &catalog, &inputs, 2).is_err());
}
#[test]
fn preparation_rejects_actual_changed_inputs_and_authority_without_advancing() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let path = catalog.common_dir.join("towermd");
    let session = Mutex::new(ProjectSession::open(&path, &catalog, &inputs, 1).unwrap());
    let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open(
        &catalog.root.join("runtime-outbox"),
        towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let mut event = unproved_merge();
    event.kind = "future.audit".into();
    event.data = json!({"actual":true});
    outbox.append_pending(&event).unwrap();
    drop(outbox);
    assert!(matches!(
        ProjectSession::sync_shared(&session, &catalog, &inputs, 2),
        Err(towermd_core::project_session::SessionSyncError::SourceChanged(_))
    ));
    assert_eq!(session.lock().unwrap().applied_seq(), 0);
    let fresh = RecoveryInputs::read(&catalog.root.join("runtime-outbox"), &catalog).unwrap();
    let status =
        ProjectSession::read_status_shared(&session, &catalog, &fresh, "host", None, 2).unwrap();
    assert_eq!(status["seq"], 0);
    assert_eq!(status["pending_verification"], true);
    assert_eq!(status["tasks"], json!([]));
    support::git(
        &catalog.root,
        &["commit", "--allow-empty", "-qm", "advance actual authority"],
    );
    assert!(matches!(
        ProjectSession::sync_shared(&session, &catalog, &fresh, 2),
        Err(towermd_core::project_session::SessionSyncError::SourceChanged(_))
    ));
    assert_eq!(session.lock().unwrap().applied_seq(), 0);
}

struct DeliveryChange {
    armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    root: std::path::PathBuf,
    previous: Option<String>,
}
struct ChurningHub {
    inner: Hub,
    enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    captures: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    delivery_change: Option<DeliveryChange>,
}
impl HubProtocol for ChurningHub {
    fn append(&mut self, e: NewEvent, n: u64) -> Result<AppendReceipt, HubError> {
        self.inner.append(e, n)
    }
    fn subscribe(&mut self, a: u64, n: u64) -> Result<SubscriptionBatch, HubError> {
        if self.enabled.load(std::sync::atomic::Ordering::SeqCst) {
            self.captures
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut event = unproved_merge();
            event.kind = "future.audit".into();
            event.data = json!({});
            self.inner.append(event, n)?;
        }
        let batch = self.inner.subscribe(a, n)?;
        if let Some(change) = &self.delivery_change {
            if a < self.inner.contiguous_seq()
                && change
                    .armed
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                if let Some(previous) = &change.previous {
                    support::git(&change.root, &["update-ref", "refs/heads/main", previous]);
                } else {
                    let catalog = Catalog::load(&change.root).unwrap();
                    let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open(
                        &change.root.join("runtime-outbox"),
                        towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog)
                            .unwrap(),
                    )
                    .unwrap();
                    let mut event = unproved_merge();
                    event.kind = "future.audit".into();
                    event.data = json!({"delivery":"changed"});
                    outbox.append_pending(&event).unwrap();
                }
            }
        }
        Ok(batch)
    }
    fn lease_acquire(&mut self, r: AcquireLease, n: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_acquire(r, n)
    }
    fn lease_renew(&mut self, r: RenewLease, n: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_renew(r, n)
    }
    fn lease_release(&mut self, r: ReleaseLease, n: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_release(r, n)
    }
    fn snapshot_ack(&mut self, a: SnapshotAck, c: serde_json::Value) -> Result<(), HubError> {
        HubProtocol::snapshot_ack(&mut self.inner, a, c)
    }
    fn rebuild(&mut self, s: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        self.inner.rebuild(s)
    }
    fn contiguous_seq(&self) -> u64 {
        self.inner.contiguous_seq()
    }
}
#[test]
fn actual_head_churn_discards_three_preparations_without_installing() {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let enabled = Arc::new(AtomicBool::new(false));
    let captures = Arc::new(AtomicUsize::new(0));
    let session = Mutex::new(
        ProjectSession::new(
            Box::new(ChurningHub {
                inner: Hub::open(&dir.path().join("hub")).unwrap(),
                enabled: enabled.clone(),
                captures: captures.clone(),
                delivery_change: None,
            }),
            &catalog,
            &inputs,
            1,
        )
        .unwrap(),
    );
    enabled.store(true, Ordering::SeqCst);
    assert!(matches!(
        ProjectSession::sync_shared(&session, &catalog, &inputs, 2),
        Err(towermd_core::project_session::SessionSyncError::SourceChanged(_))
    ));
    assert_eq!(captures.load(Ordering::SeqCst), 6);
    assert_eq!(session.lock().unwrap().applied_seq(), 0);
    enabled.store(false, Ordering::SeqCst);
    ProjectSession::sync_shared(&session, &catalog, &inputs, 2).unwrap();
    assert_eq!(session.lock().unwrap().applied_seq(), 6);
    enabled.store(true, Ordering::SeqCst);
    captures.store(0, Ordering::SeqCst);
    let failed = ProjectSession::new(
        Box::new(ChurningHub {
            inner: Hub::open(&dir.path().join("constructor-hub")).unwrap(),
            enabled,
            captures: captures.clone(),
            delivery_change: None,
        }),
        &catalog,
        &inputs,
        2,
    );
    assert!(matches!(
        failed,
        Err(towermd_core::project_session::SessionSyncError::SourceChanged(_))
    ));
    assert_eq!(captures.load(Ordering::SeqCst), 7);
}

#[test]
fn malformed_merge_evidence_leaves_pure_business_projections_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, _) = context(dir.path());
    let mut hub = Hub::open(&dir.path().join("hub")).unwrap();
    let mut event = unproved_merge();
    event.data["run"] = json!({"type":"Run","id":"R","task":"T","host":"host","worktree":"/tmp/task","branch":"t/T","state":"ended","leases":[],"cost_usd":null,"task_card":{}});
    hub.append(event, 1).unwrap();
    let event = hub.subscribe(0, 1).unwrap().events.remove(0);
    let mut projection = towermd_core::snapshot::StateProjection::default();
    let before = projection.clone();
    assert!(projection.apply_recorded(&event).is_err());
    assert_eq!(
        projection, before,
        "malformed Evidence must not leave a partially merged Task/ended Run"
    );
    let mut tasks = towermd_core::tasks::TaskService::default();
    assert!(tasks.apply(&event).is_err());
    assert_eq!(
        tasks
            .execute(
                &mut hub,
                &catalog,
                "host",
                towermd_core::tasks::TaskCommand::Status,
                None,
                1
            )
            .unwrap()["tasks"],
        json!([])
    );
}

fn public_source_boundary(operation: &str, changed: &str) {
    use towermd_core::tasks::{TaskCommand, TaskSpec};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let s0 = support::catalog(root).revision;
    support::git(root, &["switch", "main"]);
    support::git(
        root,
        &["commit", "--allow-empty", "-qm", "S1 actual authority"],
    );
    support::git(root, &["switch", "t/T"]);
    let catalog = Catalog::load(root).unwrap();
    let inputs = support::inputs(&catalog);
    let session =
        Mutex::new(ProjectSession::open(&root.join("hub"), &catalog, &inputs, 0).unwrap());
    ProjectSession::execute_shared(
        &session,
        &catalog,
        &inputs,
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T".into(),
                title: "Task".into(),
                module: "A".into(),
                touches: vec![],
                allowed_paths: vec!["src/file.rs".into()],
                depends_on: vec![],
                acceptance: vec!["AC".into()],
                decisions: vec![],
            },
        },
        None,
        1,
    )
    .unwrap();
    if operation == "expire" {
        ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            2,
        )
        .unwrap();
    }
    let before = session.lock().unwrap().projection().unwrap().clone();
    let applied = session.lock().unwrap().applied_seq();
    if changed == "main" {
        support::git(
            root,
            &["update-ref", "refs/heads/main", &s0, &catalog.revision],
        );
    } else {
        let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open(
            &root.join("test-outbox"),
            towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        outbox
            .append_pending(&NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:test".into(),
                kind: "alert.raised".into(),
                layer: "L3".into(),
                obj: "audit".into(),
                data: json!({"reason":"changed actual inputs"}),
                guards: vec![],
            })
            .unwrap();
    }
    let result = match operation {
        "show" => ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Show {
                id: Some("T".into()),
            },
            None,
            3,
        )
        .map(|_| ())
        .map_err(|e| e.to_string()),
        "claim" => towermd_core::tasks::execute(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            3,
        )
        .map(|_| ())
        .map_err(|e| e.to_string()),
        "expire" => {
            towermd_core::tasks::expire(&session, &catalog, &inputs, "host", 15 * 60 * 1000 + 3)
        }
        "reconcile" => ProjectSession::reconcile_pending_shared(&session, &catalog, &inputs, 3),
        "subscribe" => ProjectSession::subscribe_shared(&session, &catalog, &inputs, 0, 3)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        _ => unreachable!(),
    };
    assert!(
        result.is_err(),
        "public {operation} accepted stale actual {changed}: {result:?}"
    );
    assert_eq!(*session.lock().unwrap().projection().unwrap(), before);
    assert_eq!(session.lock().unwrap().applied_seq(), applied);
    assert!(!session
        .lock()
        .unwrap()
        .events(applied)
        .iter()
        .any(|event| event.kind.starts_with("task.")));
}
macro_rules! public_source_test {
    ($name:ident, $operation:literal, $source:literal) => {
        #[test]
        fn $name() {
            public_source_boundary($operation, $source);
        }
    };
}
public_source_test!(public_show_rejects_main_rollback, "show", "main");
public_source_test!(public_claim_rejects_main_rollback, "claim", "main");
public_source_test!(public_expire_rejects_main_rollback, "expire", "main");
public_source_test!(public_reconcile_rejects_main_rollback, "reconcile", "main");
public_source_test!(public_subscribe_rejects_main_rollback, "subscribe", "main");
public_source_test!(public_show_rejects_changed_inputs, "show", "inputs");
public_source_test!(public_claim_rejects_changed_inputs, "claim", "inputs");
public_source_test!(public_expire_rejects_changed_inputs, "expire", "inputs");
public_source_test!(
    public_reconcile_rejects_changed_inputs,
    "reconcile",
    "inputs"
);
public_source_test!(
    public_subscribe_rejects_changed_inputs,
    "subscribe",
    "inputs"
);

#[test]
fn public_sync_rejects_ordinary_task_state_merged_without_canonical_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, inputs) = context(dir.path());
    let session =
        Mutex::new(ProjectSession::open(&dir.path().join("hub"), &catalog, &inputs, 0).unwrap());
    let before = session.lock().unwrap().projection().unwrap().clone();
    session.lock().unwrap().protocol().unwrap().append(NewEvent {
        eid: uuid::Uuid::now_v7().to_string(), host: "host".into(), actor: "process:self-reported".into(), kind: "task.state".into(), layer: "L4".into(), obj: "T".into(), data: json!({"task":{"type":"Task","id":"T","title":"self report","module":"A","state":"merged"}}), guards: vec![],
    }, 1).unwrap();
    assert!(ProjectSession::sync_shared(&session, &catalog, &inputs, 1).is_err());
    assert_eq!(*session.lock().unwrap().projection().unwrap(), before);
    assert_eq!(session.lock().unwrap().applied_seq(), 0);
}

#[test]
fn subscription_does_not_deliver_batch_when_actual_source_or_inputs_change_during_read() {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    for change_main in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (old, _) = context(dir.path());
        support::git(
            dir.path(),
            &["commit", "--allow-empty", "-qm", "delivery S1"],
        );
        let catalog = Catalog::load(dir.path()).unwrap();
        let inputs = RecoveryInputs::read(&dir.path().join("runtime-outbox"), &catalog).unwrap();
        let armed = Arc::new(AtomicBool::new(false));
        let session = Mutex::new(
            ProjectSession::new(
                Box::new(ChurningHub {
                    inner: Hub::open(&dir.path().join("hub")).unwrap(),
                    enabled: Arc::new(AtomicBool::new(false)),
                    captures: Arc::new(AtomicUsize::new(0)),
                    delivery_change: Some(DeliveryChange {
                        armed: armed.clone(),
                        root: dir.path().into(),
                        previous: change_main.then_some(old.revision),
                    }),
                }),
                &catalog,
                &inputs,
                1,
            )
            .unwrap(),
        );
        let mut audit = unproved_merge();
        audit.kind = "future.audit".into();
        audit.data = json!({"verified":"ordinary audit"});
        session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .append(audit, 2)
            .unwrap();
        ProjectSession::sync_shared(&session, &catalog, &inputs, 2).unwrap();
        let before = session.lock().unwrap().projection().unwrap().clone();
        let applied = session.lock().unwrap().applied_seq();
        assert_eq!(applied, 1);
        armed.store(true, Ordering::SeqCst);
        let error = ProjectSession::subscribe_shared(&session, &catalog, &inputs, 0, 3)
            .unwrap_err()
            .to_string();
        assert!(
            !armed.load(Ordering::SeqCst),
            "actual delivery read must trigger source change"
        );
        assert_eq!(
            error,
            if change_main {
                "authority changed; reload Catalog"
            } else {
                "recovery inputs changed during preparation"
            }
        );
        assert_eq!(session.lock().unwrap().applied_seq(), applied);
        assert_eq!(*session.lock().unwrap().projection().unwrap(), before);
        assert_eq!(session.lock().unwrap().events(0).len(), 1);
    }
}

// Wrap the real Hub, injecting only genuine accepted audit events. This matches
// the independent Q1 probe: two discarded inner rounds, a successful third,
// then a new tail at the short continuation boundary must end this request.
struct ContinuationRaceHub {
    inner: Hub,
    armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
fn continuation_audit(number: usize) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:audit".into(),
        kind: "future.audit".into(),
        layer: "L4".into(),
        obj: format!("audit-{number}"),
        data: json!({"index":number}),
        guards: vec![],
    }
}
impl HubProtocol for ContinuationRaceHub {
    fn append(&mut self, event: NewEvent, now: u64) -> Result<AppendReceipt, HubError> {
        eprintln!(
            "actual command append {} at subscribe {}",
            event.kind,
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        );
        self.inner.append(event, now)
    }
    fn subscribe(&mut self, after: u64, now: u64) -> Result<SubscriptionBatch, HubError> {
        use std::sync::atomic::Ordering;
        if self.armed.load(Ordering::SeqCst) {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if [2, 4, 7, 10].contains(&call) {
                self.inner.append(continuation_audit(call), now)?;
                eprintln!("actual Hub audit appended at subscribe {call}");
            }
        }
        self.inner.subscribe(after, now)
    }
    fn lease_acquire(&mut self, request: AcquireLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_acquire(request, now)
    }
    fn lease_renew(&mut self, request: RenewLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_renew(request, now)
    }
    fn lease_release(&mut self, request: ReleaseLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_release(request, now)
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
#[test]
fn one_public_request_does_not_restart_preparation_budget_before_its_continuation() {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    use towermd_core::tasks::{TaskCommand, TaskSpec};
    let dir = tempfile::tempdir().unwrap();
    let catalog = support::catalog(dir.path());
    let inputs = support::inputs(&catalog);
    let armed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let session = Mutex::new(
        ProjectSession::new(
            Box::new(ContinuationRaceHub {
                inner: Hub::open(&dir.path().join("hub")).unwrap(),
                armed: armed.clone(),
                calls: calls.clone(),
            }),
            &catalog,
            &inputs,
            0,
        )
        .unwrap(),
    );
    let initial = session.lock().unwrap().authority_reads();
    let refs = support::git(dir.path(), &["show-ref"]);
    let index = std::fs::read(dir.path().join(".git/index")).unwrap();
    session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(continuation_audit(0), 1)
        .unwrap();
    let add = || TaskCommand::Add {
        task: TaskSpec {
            id: "T".into(),
            title: "one request".into(),
            module: "A".into(),
            touches: vec![],
            allowed_paths: vec!["src/**".into()],
            depends_on: vec![],
            acceptance: vec!["AC".into()],
            decisions: vec![],
        },
    };
    armed.store(true, Ordering::SeqCst);
    let result =
        ProjectSession::execute_shared(&session, &catalog, &inputs, "host", add(), None, 2);
    armed.store(false, Ordering::SeqCst);
    let raw = session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(0, 2)
        .unwrap();
    let task_events = raw
        .events
        .iter()
        .filter(|event| event.obj == "T" && event.kind == "task.state")
        .count();
    eprintln!(
        "result={result:?}; proof installs={}; subscribes={}; actual Task events={task_events}",
        session.lock().unwrap().authority_reads() - initial,
        calls.load(Ordering::SeqCst)
    );
    assert_eq!(
        Catalog::load(dir.path()).unwrap().revision,
        catalog.revision
    );
    assert_eq!(support::inputs(&catalog).digest(), inputs.digest());
    assert_eq!(support::git(dir.path(), &["show-ref"]), refs);
    assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index);
    let error = result.expect_err(
        "same public request must stop instead of obtaining fourth/fifth preparation rounds",
    );
    assert!(
        error
            .to_string()
            .contains("head changed before continuation"),
        "{error}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 7);
    assert_eq!(session.lock().unwrap().authority_reads() - initial, 1);
    assert_eq!(task_events, 0, "business FnOnce must not have run");
    assert!(session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .get("Task")
        .is_none_or(|tasks| !tasks.contains_key("T")));
    assert!(session.lock().unwrap().pending_claim().unwrap().is_none());
    assert_eq!(session.lock().unwrap().applied_seq(), 3);
    assert_eq!(raw.head.contiguous_seq, 4);
    // A distinct fresh request may prove the new tail and execute once normally.
    ProjectSession::execute_shared(&session, &catalog, &inputs, "host", add(), None, 3).unwrap();
    let raw = session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(0, 3)
        .unwrap();
    assert_eq!(
        raw.events
            .iter()
            .filter(|event| event.obj == "T" && event.kind == "task.state")
            .count(),
        1
    );
}
