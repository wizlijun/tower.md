mod support;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use towermd_core::catalog::Catalog;
use towermd_core::init::{init, InitOptions};
use towermd_core::project_session::ProjectSession as Hub;
use towermd_core::snapshot::{Snapshot, StateProjection};
use towermd_core::snapshot_service::{due, interval_ms, SnapshotWriter};
use towermd_core::tasks::{self, TaskCommand, TaskSpec};

const NOW: u64 = 1_800_000_000_000;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}

fn fixture() -> (tempfile::TempDir, Catalog, Mutex<Hub>) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    git(root, &["init", "-qb", "main"]);
    init(root, &InitOptions { docs_dir: None }).unwrap();
    fs::write(
        root.join("docs/tower/arch/A.md"),
        "---\ntype: Module\nid: A\npaths: [src/**]\n---\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/tower/specs/AC.md"),
        "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root, &["switch", "-qc", "snapshot-fixture"]);
    let catalog = Catalog::load(root).unwrap();
    let hub = Mutex::new(support::open(
        &catalog.common_dir.join("towermd"),
        &catalog,
        0,
    ));
    (temporary, catalog, hub)
}

fn add(hub: &Mutex<Hub>, catalog: &Catalog, id: &str, now: u64) {
    tasks::execute(
        hub,
        catalog,
        &support::inputs(catalog),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: id.into(),
                title: "test".into(),
                module: "A".into(),
                touches: Vec::new(),
                allowed_paths: vec!["src/**".into()],
                depends_on: Vec::new(),
                acceptance: vec!["AC".into()],
                decisions: Vec::new(),
            },
        },
        None,
        now,
    )
    .unwrap();
}

#[test]
fn snapshot_commits_checkpoint_then_acknowledges_and_survives_restart() {
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    let report = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    assert_eq!(report.status, "written");
    assert_eq!(
        report.commit.as_deref(),
        Some(git(temporary.path(), &["rev-parse", "main"]).as_str())
    );
    let state: Snapshot = serde_json::from_str(&git(
        temporary.path(),
        &["show", "main:docs/tower/state/state.json"],
    ))
    .unwrap();
    assert_eq!(state.through_seq, report.seq);
    assert_eq!(state.projection.objects["Task"]["T-1"]["state"], "ready");
    assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().seq, report.seq);
    assert_eq!(git(temporary.path(), &["status", "--porcelain"]), "");
    drop(hub);
    let catalog = Catalog::load(temporary.path()).unwrap();
    let reopened = support::open(&catalog.common_dir.join("towermd"), &catalog, 0);
    assert_eq!(reopened.snapshot_ack().unwrap().seq, report.seq);
}

#[test]
fn subsequent_snapshot_appends_archive_without_heartbeats() {
    let (temporary, catalog, hub) = fixture();
    let writer = SnapshotWriter::default();
    add(&hub, &catalog, "T-1", NOW);
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let catalog = Catalog::load(temporary.path()).unwrap();
    add(&hub, &catalog, "T-2", NOW + 1000);
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW + 1000))
        .unwrap();
    let state: Snapshot = serde_json::from_str(&git(
        temporary.path(),
        &["show", "main:docs/tower/state/state.json"],
    ))
    .unwrap();
    let mut archive = Vec::new();
    let paths = git(
        temporary.path(),
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "main",
            "--",
            "docs/tower/state/events",
        ],
    );
    for path in paths.lines() {
        for line in git(temporary.path(), &["show", &format!("main:{path}")]).lines() {
            let event: towermd_core::hub::Event = serde_json::from_str(line).unwrap();
            assert!(!event.is_pure_heartbeat());
            archive.push(event);
        }
    }
    archive.sort_by_key(|event| event.seq);
    state
        .validate_archive(catalog.config.project_id(), &archive)
        .unwrap();
    let mut replay = StateProjection::default();
    for event in archive {
        replay.apply_recorded(&event).unwrap();
    }
    assert_eq!(replay, state.projection);
    assert_eq!(replay.objects["Task"].len(), 2);
}

#[test]
fn checked_main_persists_private_and_acknowledges_only_written_sequence() {
    for dirty in [false, true] {
        let (temporary, catalog, hub) = fixture();
        let writer = SnapshotWriter::default();
        add(&hub, &catalog, "T-1", NOW);
        let first = writer
            .write(&hub, &catalog, "host", true, || Ok(NOW))
            .unwrap();
        assert_eq!(first.status, "written");
        git(temporary.path(), &["switch", "main"]);
        let catalog = Catalog::load(temporary.path()).unwrap();
        let path = temporary.path().join("docs/tower/state/state.json");
        if dirty {
            fs::write(&path, "user edits").unwrap();
        }
        let before = fs::read(&path).unwrap();
        add(&hub, &catalog, "T-2", NOW + 1000);
        let report = writer
            .write(&hub, &catalog, "host", true, || Ok(NOW + 1000))
            .unwrap();
        assert_eq!(report.status, "written");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().seq, report.seq);
        assert_eq!(
            git(temporary.path(), &["rev-parse", "refs/towermd/snapshot"]),
            report.commit.unwrap()
        );
        assert_eq!(
            git(temporary.path(), &["rev-parse", "main"]),
            first.commit.unwrap()
        );
    }
}

#[test]
fn periodic_snapshot_requires_business_changes_and_elapsed_interval() {
    let (temporary, catalog, hub) = fixture();
    assert_eq!(interval_ms(&catalog).unwrap(), 30 * 60 * 1000);
    assert!(!due(&hub.lock().unwrap(), 1000, NOW).unwrap());
    add(&hub, &catalog, "T-1", NOW);
    assert!(!due(&hub.lock().unwrap(), 1000, NOW + 999).unwrap());
    assert!(due(&hub.lock().unwrap(), 1000, NOW + 1000).unwrap());
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW + 1000))
        .unwrap();
    assert!(!due(&hub.lock().unwrap(), 1000, NOW + 100_000).unwrap());
    let catalog = Catalog::load(temporary.path()).unwrap();
    add(&hub, &catalog, "T-2", NOW + 1500);
    assert!(due(&hub.lock().unwrap(), 1000, NOW + 2000).unwrap());
}

#[test]
fn lost_hub_cannot_start_a_new_history_over_an_existing_snapshot() {
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let catalog = Catalog::load(temporary.path()).unwrap();
    drop(hub);
    fs::remove_file(catalog.common_dir.join("towermd/journal.jsonl")).unwrap();
    let result = Hub::open(
        &catalog.common_dir.join("towermd"),
        &catalog,
        &support::inputs(&catalog),
        0,
    );
    assert!(
        result.is_err(),
        "constructor must not publish a fake valid empty Session over saved Git history"
    );
    let raw = towermd_core::hub::Hub::open(&catalog.common_dir.join("towermd")).unwrap();
    use towermd_core::hub_protocol::HubProtocol;
    assert_eq!(raw.contiguous_seq(), 0);
}

#[test]
fn git_success_without_hub_ack_is_reconciled_without_duplicate_fact() {
    for checked_main in [false, true] {
        let (temporary, catalog, hub) = fixture();
        if checked_main {
            git(temporary.path(), &["switch", "main"]);
        }
        add(&hub, &catalog, "T-1", NOW);
        let writer = SnapshotWriter::default();
        let journal = catalog.common_dir.join("towermd/journal.jsonl");
        let backup = catalog.common_dir.join("towermd/saved.jsonl");
        fs::rename(&journal, &backup).unwrap();
        fs::create_dir(&journal).unwrap();
        let error = writer
            .write(&hub, &catalog, "host", true, || Ok(NOW))
            .unwrap_err();
        assert!(
            error.contains("Git 快照") && error.contains("hub 确认失败"),
            "{error}"
        );
        assert!(hub.lock().unwrap().snapshot_ack().is_none());
        fs::remove_dir(&journal).unwrap();
        fs::rename(&backup, &journal).unwrap();
        drop(hub);
        let catalog = Catalog::load(temporary.path()).unwrap();
        let hub = Mutex::new(support::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            0,
        ));
        let report = writer
            .write(&hub, &catalog, "host", false, || Ok(NOW + 1000))
            .unwrap();
        assert_eq!(report.status, "unchanged");
        assert_eq!(
            hub.lock().unwrap().snapshot_ack().unwrap().commit,
            towermd_core::snapshot_service::read_snapshot(&catalog)
                .unwrap()
                .unwrap()
                .commit
        );
        writer
            .write(&hub, &catalog, "host", false, || Ok(NOW + 2000))
            .unwrap();
        assert_eq!(
            hub.lock()
                .unwrap()
                .events(0)
                .iter()
                .filter(|event| event.kind == "snapshot.written")
                .count(),
            1
        );
    }
}

#[test]
fn main_lease_renewal_keeps_epoch_but_expiry_never_revives_it() {
    let (_temporary, catalog, hub) = fixture();
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let mut hub = hub.lock().unwrap();
    let original = hub.leases(NOW)["main"].clone();
    towermd_core::snapshot_service::maintain_main(&mut hub, "host", NOW + 30_000).unwrap();
    let renewed = hub.leases(NOW + 30_000)["main"].clone();
    assert_eq!(original.epoch, renewed.epoch);
    assert_eq!(renewed.expires_at, NOW + 150_000);
    towermd_core::snapshot_service::maintain_main(&mut hub, "host", NOW + 150_000).unwrap();
    assert!(hub.leases(NOW + 150_000).is_empty());
}

#[cfg(unix)]
#[test]
fn published_snapshot_cache_failure_does_not_ack_and_can_be_reconciled() {
    use std::os::unix::fs::PermissionsExt;
    let (temporary, original, hub) = fixture();
    add(&hub, &original, "T-1", NOW);
    let remote = tempfile::tempdir().unwrap();
    git(
        temporary.path(),
        &["clone", "--bare", ".", remote.path().to_str().unwrap()],
    );
    git(
        temporary.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    towermd_core::origin::refresh_origin_main(temporary.path()).unwrap();
    let catalog = Catalog::load(temporary.path()).unwrap();
    let lock = catalog.common_dir.join("refs/remotes/origin/main.lock");
    let hook = remote.path().join("hooks/post-receive");
    fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", lock.display())).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let error = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap_err();
    let published = git(remote.path(), &["rev-parse", "main"]);
    assert!(
        error.contains(&published) && error.contains("缓存未协调"),
        "{error}"
    );
    assert!(hub.lock().unwrap().snapshot_ack().is_none());
    assert!(!hub
        .lock()
        .unwrap()
        .events(0)
        .iter()
        .any(|event| event.kind == "snapshot.written"));
    assert_eq!(
        Catalog::load(temporary.path()).unwrap().revision,
        catalog.revision
    );
    fs::remove_file(lock).unwrap();
    towermd_core::origin::refresh_origin_main(temporary.path()).unwrap();
    let current = Catalog::load(temporary.path()).unwrap();
    assert_eq!(current.revision, published);
    SnapshotWriter::default()
        .write(&hub, &current, "host", false, || Ok(NOW + 1))
        .unwrap();
    assert_eq!(
        hub.lock().unwrap().snapshot_ack().unwrap().commit,
        published
    );
    assert_eq!(
        hub.lock()
            .unwrap()
            .events(0)
            .iter()
            .filter(|event| event.kind == "snapshot.written")
            .count(),
        1
    );
}

struct PrefixHub {
    inner: std::sync::Arc<std::sync::Mutex<towermd_core::hub::Hub>>,
    confirmed: std::sync::Arc<std::sync::atomic::AtomicU64>,
    deliver: std::sync::Arc<std::sync::atomic::AtomicBool>,
    future: towermd_core::hub_protocol::Event,
    delivered: bool,
}
impl towermd_core::hub_protocol::HubProtocol for PrefixHub {
    fn append(
        &mut self,
        e: towermd_core::hub_protocol::NewEvent,
        n: u64,
    ) -> Result<towermd_core::hub_protocol::AppendReceipt, towermd_core::hub_protocol::HubError>
    {
        self.inner.lock().unwrap().append(e, n)
    }
    fn subscribe(
        &mut self,
        a: u64,
        n: u64,
    ) -> Result<towermd_core::hub_protocol::SubscriptionBatch, towermd_core::hub_protocol::HubError>
    {
        use std::sync::atomic::Ordering;
        use towermd_core::hub_protocol::*;
        if self.deliver.load(Ordering::SeqCst) && !self.delivered {
            let e = &self.future;
            self.inner.lock().unwrap().append(
                NewEvent {
                    eid: e.eid.clone(),
                    host: e.host.clone(),
                    actor: e.actor.clone(),
                    kind: e.kind.clone(),
                    layer: e.layer.clone(),
                    obj: e.obj.clone(),
                    data: e.data.clone(),
                    guards: e.guards.clone(),
                },
                n,
            )?;
            self.delivered = true;
        }
        let mut batch = self.inner.lock().unwrap().subscribe(a, n)?;
        if !self.delivered && a < 103 {
            batch.events.push(self.future.clone());
        }
        Ok(batch)
    }
    fn lease_acquire(
        &mut self,
        r: towermd_core::hub_protocol::AcquireLease,
        n: u64,
    ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
    {
        self.inner.lock().unwrap().lease_acquire(r, n)
    }
    fn lease_renew(
        &mut self,
        r: towermd_core::hub_protocol::RenewLease,
        n: u64,
    ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
    {
        self.inner.lock().unwrap().lease_renew(r, n)
    }
    fn lease_release(
        &mut self,
        r: towermd_core::hub_protocol::ReleaseLease,
        n: u64,
    ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
    {
        self.inner.lock().unwrap().lease_release(r, n)
    }
    fn snapshot_ack(
        &mut self,
        a: towermd_core::hub_protocol::SnapshotAck,
        c: serde_json::Value,
    ) -> Result<(), towermd_core::hub_protocol::HubError> {
        towermd_core::hub_protocol::HubProtocol::snapshot_ack(
            &mut *self.inner.lock().unwrap(),
            a,
            c,
        )
    }
    fn rebuild(
        &mut self,
        r: towermd_core::hub_protocol::RebuildSeed,
    ) -> Result<towermd_core::hub_protocol::RebuildReceipt, towermd_core::hub_protocol::HubError>
    {
        self.inner.lock().unwrap().rebuild(r)
    }
    fn contiguous_seq(&self) -> u64 {
        use std::sync::atomic::Ordering;
        self.inner
            .lock()
            .unwrap()
            .contiguous_seq()
            .min(self.confirmed.load(Ordering::SeqCst))
    }
}
#[test]
fn actual_snapshot_writer_waits_for_102_before_publishing_visible_103() {
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    };
    use towermd_core::hub_protocol::*;
    let (temporary, catalog, _) = fixture();
    let mut inner = towermd_core::hub::Hub::open(&temporary.path().join("ordered-hub")).unwrap();
    inner
        .lease_acquire(
            AcquireLease {
                host: "host".into(),
                names: vec!["main".into()],
                holder: "snapshot:host".into(),
                ttl_ms: 120000,
            },
            NOW,
        )
        .unwrap();
    for _ in 0..100 {
        inner
            .append(
                NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "host".into(),
                    actor: "process:test".into(),
                    kind: "arbitrary.event".into(),
                    layer: "L5".into(),
                    obj: "object".into(),
                    data: serde_json::json!([null, false]),
                    guards: vec![],
                },
                NOW,
            )
            .unwrap();
    }
    let mut future = inner.events(100)[0].clone();
    future.seq = 103;
    future.eid = uuid::Uuid::now_v7().to_string();
    future.data = serde_json::json!(["future", 103]);
    let future_eid = future.eid.clone();
    let confirmed = Arc::new(AtomicU64::new(101));
    let deliver = Arc::new(AtomicBool::new(false));
    let hub = Mutex::new(
        Hub::new(
            Box::new(PrefixHub {
                inner: Arc::new(Mutex::new(inner)),
                confirmed: confirmed.clone(),
                deliver: deliver.clone(),
                future,
                delivered: false,
            }),
            &catalog,
            &support::inputs(&catalog),
            NOW,
        )
        .unwrap(),
    );
    let writer = SnapshotWriter::default();
    let first = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    assert_eq!(first.status, "written");
    assert_eq!(first.seq, 101);
    let saved =
        towermd_core::snapshot_service::read_snapshot(&Catalog::load(temporary.path()).unwrap())
            .unwrap()
            .unwrap();
    assert_eq!(saved.snapshot.through_seq, 101);
    assert!(!saved.archives.values().any(|s| s.contains(&future_eid)));
    confirmed.store(103, Ordering::SeqCst);
    deliver.store(true, Ordering::SeqCst);
    let catalog = Catalog::load(temporary.path()).unwrap();
    let second = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW + 1))
        .unwrap();
    assert_eq!(second.status, "written");
    assert_eq!(second.seq, 103);
    let saved =
        towermd_core::snapshot_service::read_snapshot(&Catalog::load(temporary.path()).unwrap())
            .unwrap()
            .unwrap();
    assert_eq!(saved.snapshot.through_seq, 103);
    assert!(saved.archives.values().any(|s| s.contains(&future_eid)));
}
#[test]
fn unchanged_authority_and_idle_commands_do_not_reread_archive_or_reapply_history() {
    let (_temporary, catalog, hub) = fixture();
    let mut session = hub.lock().unwrap();
    for _ in 0..100 {
        towermd_core::snapshot_service::validate_authority(&mut session, &catalog).unwrap();
    }
    assert_eq!(session.authority_reads(), 1);
    let applied = session.applied_count();
    drop(session);
    for _ in 0..100 {
        Hub::execute_shared(
            &hub,
            &catalog,
            &support::inputs(&catalog),
            "host",
            TaskCommand::Status,
            None,
            NOW,
        )
        .unwrap();
        Hub::tick_shared(&hub, &catalog, &support::inputs(&catalog), "host", NOW).unwrap();
    }
    let session = hub.lock().unwrap();
    assert_eq!(session.applied_count(), applied);
    assert_eq!(session.authority_reads(), 1);
    drop(session);
    {
        let live = hub.lock().unwrap();
        eprintln!(
            "idle stage before Add: reads={} applied={} head={} ack={:?}",
            live.authority_reads(),
            live.applied_count(),
            live.last_seq(),
            live.snapshot_ack()
        );
    }
    add(&hub, &catalog, "T-1", NOW);
    let session = hub.lock().unwrap();
    eprintln!(
        "idle stage after Add: reads={} applied={} head={} ack={:?}",
        session.authority_reads(),
        session.applied_count(),
        session.last_seq(),
        session.snapshot_ack()
    );
    assert_eq!(session.applied_count(), applied + 1);
    drop(session);
    for _ in 0..100 {
        Hub::execute_shared(
            &hub,
            &catalog,
            &support::inputs(&catalog),
            "host",
            TaskCommand::Status,
            None,
            NOW,
        )
        .unwrap();
        Hub::tick_shared(&hub, &catalog, &support::inputs(&catalog), "host", NOW).unwrap();
    }
    let session = hub.lock().unwrap();
    eprintln!(
        "idle stage after 100 unchanged requests: reads={} applied={} head={} ack={:?}",
        session.authority_reads(),
        session.applied_count(),
        session.last_seq(),
        session.snapshot_ack()
    );
    assert_eq!(session.applied_count(), applied + 1);
    assert_eq!(session.authority_reads(), 1);
}
#[test]
fn snapshot_writer_rejects_a_changed_project_identity_in_the_same_git_directory() {
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    towermd_core::snapshot_service::validate_authority(&mut hub.lock().unwrap(), &catalog).unwrap();
    let mut changed = Catalog::load(temporary.path()).unwrap();
    changed.config =
        towermd_core::config::Config::new("different-project", catalog.config.docs_dir()).unwrap();
    let before = git(temporary.path(), &["rev-parse", "main"]);
    let error = SnapshotWriter::default()
        .write(&hub, &changed, "host", true, || Ok(NOW))
        .unwrap_err();
    assert!(error.contains("identity"));
    assert_eq!(git(temporary.path(), &["rev-parse", "main"]), before);
    assert!(hub.lock().unwrap().snapshot_ack().is_none());
}
#[test]
fn git_write_rechecks_protocol_main_fence_after_network_stage() {
    use std::sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc,
    };
    use towermd_core::hub_protocol::*;
    let (temporary, catalog, _) = fixture();
    let mut local = towermd_core::hub::Hub::open(&temporary.path().join("fenced-hub")).unwrap();
    let grant = local
        .lease_acquire(
            AcquireLease {
                host: "host".into(),
                names: vec!["main".into()],
                holder: "snapshot:host".into(),
                ttl_ms: 120000,
            },
            NOW,
        )
        .unwrap();
    let fence = grant.leases[0].fence();
    let shared = Arc::new(Mutex::new(local));
    let protocol = PrefixHub {
        inner: shared.clone(),
        confirmed: Arc::new(AtomicU64::new(u64::MAX)),
        deliver: Arc::new(AtomicBool::new(false)),
        future: grant.event,
        delivered: true,
    };
    let hub = Mutex::new(
        Hub::new(
            Box::new(protocol),
            &catalog,
            &support::inputs(&catalog),
            NOW,
        )
        .unwrap(),
    );
    let before = git(temporary.path(), &["rev-parse", "main"]);
    let mut calls = 0;
    let result = SnapshotWriter::default().write(&hub, &catalog, "host", true, || {
        calls += 1;
        if calls == 2 {
            let mut remote = shared.lock().unwrap();
            remote
                .lease_release(
                    ReleaseLease {
                        host: "other".into(),
                        fences: vec![fence.clone()],
                    },
                    NOW + 1,
                )
                .unwrap();
            remote
                .lease_acquire(
                    AcquireLease {
                        host: "other".into(),
                        names: vec!["main".into()],
                        holder: "replacement".into(),
                        ttl_ms: 120000,
                    },
                    NOW + 1,
                )
                .unwrap();
        }
        Ok(NOW + 1)
    });
    let error = result.unwrap_err();
    assert!(error.contains("main 租约已失效"), "{error}");
    assert_eq!(git(temporary.path(), &["rev-parse", "main"]), before);
    assert_eq!(
        shared.lock().unwrap().leases(NOW + 1)["main"].holder,
        "replacement"
    );
}

#[test]
fn checked_main_snapshot_is_readable_and_extends_after_restart() {
    let (temporary, catalog, hub) = fixture();
    git(temporary.path(), &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let first = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let first_commit = first.commit.unwrap();
    assert_eq!(
        git(temporary.path(), &["rev-parse", "main"]),
        catalog.revision
    );
    let saved = towermd_core::snapshot_service::read_snapshot(&catalog)
        .unwrap()
        .expect("private checkpoint is a recovery source");
    assert_eq!(saved.commit, first_commit);
    assert_eq!(saved.snapshot.through_seq, first.seq);
    drop(hub);
    let hub = Mutex::new(support::open(
        &catalog.common_dir.join("towermd"),
        &catalog,
        0,
    ));
    add(&hub, &catalog, "T-2", NOW + 1000);
    let second = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW + 1000))
        .unwrap();
    let saved = towermd_core::snapshot_service::read_snapshot(&catalog)
        .unwrap()
        .unwrap();
    assert_eq!(saved.commit, second.commit.unwrap());
    assert_eq!(saved.snapshot.projection.objects["Task"].len(), 2);
    assert_eq!(git(temporary.path(), &["status", "--porcelain"]), "");
}

#[test]
fn private_snapshot_keeps_all_archive_months_when_main_has_no_checkpoint() {
    let (temporary, catalog, hub) = fixture();
    git(temporary.path(), &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let later = NOW + 40 * 24 * 60 * 60 * 1000;
    add(&hub, &catalog, "T-2", later);
    writer
        .write(&hub, &catalog, "host", true, || Ok(later))
        .unwrap();
    let latest = later + 40 * 24 * 60 * 60 * 1000;
    add(&hub, &catalog, "T-3", latest);
    writer
        .write(&hub, &catalog, "host", true, || Ok(latest))
        .unwrap();
    let saved = towermd_core::snapshot_service::read_snapshot(&catalog)
        .unwrap()
        .unwrap();
    assert_eq!(saved.archives.len(), 3);
    assert_eq!(saved.snapshot.projection.objects["Task"].len(), 3);
}

#[test]
fn same_boundary_different_state_commit_cannot_silently_replace_ack() {
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    git(temporary.path(), &["switch", "main"]);
    let path = temporary.path().join("docs/tower/state/state.json");
    let snapshot: Snapshot = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();
    git(temporary.path(), &["add", "docs/tower/state/state.json"]);
    git(
        temporary.path(),
        &[
            "commit",
            "-qm",
            &format!(
                "same checkpoint\n\nTowermd-Snapshot: {}\nTowermd-Epoch: {}",
                snapshot.through_seq, snapshot.writer_epoch
            ),
        ],
    );
    let catalog = Catalog::load(temporary.path()).unwrap();
    let ack = hub.lock().unwrap().snapshot_ack().unwrap().clone();
    let result =
        towermd_core::snapshot_service::validate_authority(&mut hub.lock().unwrap(), &catalog);
    assert!(
        result.is_err(),
        "a different commit needs an explicit verified promotion proof"
    );
    assert_eq!(hub.lock().unwrap().snapshot_ack(), Some(&ack));
}

fn snapshot_variant(
    root: &Path,
    source: &str,
    snapshot: &Snapshot,
    archives: &std::collections::BTreeMap<String, String>,
) -> String {
    git(root, &["switch", "--detach", source]);
    fs::write(
        root.join("docs/tower/state/state.json"),
        serde_json::to_string(snapshot).unwrap(),
    )
    .unwrap();
    for (month, text) in archives {
        fs::write(
            root.join(format!("docs/tower/state/events/{month}.jsonl")),
            text,
        )
        .unwrap();
    }
    git(root, &["add", "docs/tower/state"]);
    git(
        root,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            &format!(
                "snapshot variant\n\nTowermd-Snapshot: {}\nTowermd-Epoch: {}",
                snapshot.through_seq, snapshot.writer_epoch
            ),
        ],
    );
    git(root, &["rev-parse", "HEAD"])
}

#[test]
fn equal_boundary_equivalent_snapshots_prefer_main_after_archive_normalization() {
    use towermd_core::snapshot_service::{read_snapshot, SnapshotSource};
    let (temporary, catalog, hub) = fixture();
    git(temporary.path(), &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let private = read_snapshot(&catalog).unwrap().unwrap();
    let reordered = private
        .archives
        .iter()
        .map(|(month, text)| {
            let text = text
                .lines()
                .map(|line| {
                    let value: serde_json::Value = serde_json::from_str(line).unwrap();
                    format!("{}\n", serde_json::to_string(&value).unwrap())
                })
                .collect::<String>();
            (month.clone(), text)
        })
        .collect();
    let main = snapshot_variant(
        temporary.path(),
        &private.commit,
        &private.snapshot,
        &reordered,
    );
    assert_ne!(main, private.commit);
    git(temporary.path(), &["update-ref", "refs/heads/main", &main]);
    let catalog = Catalog::load(temporary.path()).unwrap();
    let chosen = read_snapshot(&catalog).unwrap().unwrap();
    assert_eq!(chosen.source, SnapshotSource::Main);
    assert_eq!(chosen.commit, main);
    assert_eq!(chosen.authority_revision, catalog.revision);
}

#[test]
fn read_snapshot_selects_max_epoch_then_seq_and_tracks_actual_state_commit() {
    use towermd_core::snapshot_service::{read_snapshot, SnapshotSource};
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    let first = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap()
        .commit
        .unwrap();
    git(temporary.path(), &["switch", "main"]);
    let catalog = Catalog::load(temporary.path()).unwrap();
    add(&hub, &catalog, "T-2", NOW + 1000);
    let second = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW + 1000))
        .unwrap()
        .commit
        .unwrap();
    let chosen = read_snapshot(&catalog).unwrap().unwrap();
    assert_eq!(chosen.commit, second);
    assert_eq!(chosen.source, SnapshotSource::Private);
    git(temporary.path(), &["switch", "--detach"]);
    git(
        temporary.path(),
        &["update-ref", "refs/heads/main", &second],
    );
    git(
        temporary.path(),
        &["update-ref", "refs/towermd/snapshot", &first],
    );
    let catalog = Catalog::load(temporary.path()).unwrap();
    let chosen = read_snapshot(&catalog).unwrap().unwrap();
    assert_eq!(chosen.commit, second);
    assert_eq!(chosen.source, SnapshotSource::Main);
    // Generate a valid higher-epoch source from a fresh protocol history with fewer events.
    let low_base = Catalog {
        revision: git(temporary.path(), &["rev-parse", &format!("{first}^1")]),
        ..Catalog::load(temporary.path()).unwrap()
    };
    // No snapshots on either source while this independent valid history is produced.
    git(
        temporary.path(),
        &["update-ref", "refs/heads/main", &low_base.revision],
    );
    git(
        temporary.path(),
        &["update-ref", "-d", "refs/towermd/snapshot"],
    );
    let other_dir = temporary.path().join("second-hub");
    let other = Mutex::new(support::open(&other_dir, &low_base, 0));
    other
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .lease_acquire(
            towermd_core::hub_protocol::AcquireLease {
                host: "host".into(),
                names: vec!["unused".into()],
                holder: "other".into(),
                ttl_ms: 120_000,
            },
            NOW,
        )
        .unwrap();
    let high = writer
        .write(&other, &low_base, "host", true, || Ok(NOW))
        .unwrap()
        .commit
        .unwrap();
    let higher_epoch = read_snapshot(&Catalog::load(temporary.path()).unwrap())
        .unwrap()
        .unwrap();
    assert!(higher_epoch.snapshot.writer_epoch > chosen.snapshot.writer_epoch);
    assert!(higher_epoch.snapshot.through_seq < chosen.snapshot.through_seq);
    git(
        temporary.path(),
        &["update-ref", "refs/heads/main", &second],
    );
    git(
        temporary.path(),
        &["update-ref", "refs/towermd/snapshot", &high],
    );
    let catalog = Catalog::load(temporary.path()).unwrap();
    let chosen = read_snapshot(&catalog).unwrap().unwrap();
    assert_eq!(chosen.commit, high);
    assert_eq!(chosen.source, SnapshotSource::Private);
    // A later code-only head is authority, not the snapshot acknowledgement commit.
    git(temporary.path(), &["update-ref", "refs/heads/main", &high]);
    let tree = git(
        temporary.path(),
        &["rev-parse", &format!("{high}^{{tree}}")],
    );
    let code = git(
        temporary.path(),
        &["commit-tree", &tree, "-p", &high, "-m", "ordinary code"],
    );
    git(temporary.path(), &["update-ref", "refs/heads/main", &code]);
    let chosen = read_snapshot(&Catalog::load(temporary.path()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(chosen.commit, high);
    assert_eq!(chosen.authority_revision, code);
    assert_eq!(chosen.source, SnapshotSource::Main);
}

#[test]
fn equal_boundary_different_state_or_archive_is_rejected() {
    use towermd_core::snapshot_service::read_snapshot;
    for change in ["state", "archive"] {
        let (temporary, catalog, hub) = fixture();
        add(&hub, &catalog, "T-1", NOW);
        SnapshotWriter::default()
            .write(&hub, &catalog, "host", true, || Ok(NOW))
            .unwrap();
        let catalog = Catalog::load(temporary.path()).unwrap();
        let main = read_snapshot(&catalog).unwrap().unwrap();
        let mut snapshot = main.snapshot.clone();
        let mut archives = main.archives.clone();
        if change == "state" {
            snapshot
                .projection
                .objects
                .get_mut("Task")
                .unwrap()
                .get_mut("T-1")
                .unwrap()["title"] = "other title".into();
        }
        for text in archives.values_mut() {
            *text = text
                .lines()
                .map(|line| {
                    let mut event: serde_json::Value = serde_json::from_str(line).unwrap();
                    if change == "archive" {
                        event["host"] = "another-host".into();
                    } else if event["data"]["task"]["id"] == "T-1" {
                        event["data"]["task"]["title"] = "other title".into();
                    }
                    format!("{}\n", serde_json::to_string(&event).unwrap())
                })
                .collect();
        }
        let private = snapshot_variant(temporary.path(), &main.commit, &snapshot, &archives);
        git(
            temporary.path(),
            &["update-ref", "refs/towermd/snapshot", &private],
        );
        let error = read_snapshot(&catalog)
            .err()
            .expect("internally valid sources conflict at the same boundary");
        assert!(error.contains("同水位内容冲突"), "{change}: {error}");
        assert_eq!(git(temporary.path(), &["rev-parse", "main"]), main.commit);
        assert_eq!(
            git(temporary.path(), &["rev-parse", "refs/towermd/snapshot"]),
            private
        );
    }
}

#[test]
fn private_snapshot_context_is_checked_before_current_docs_path_lookup() {
    use towermd_core::snapshot_service::read_snapshot;
    for key in ["project_id", "docs_dir", "hub"] {
        let (temporary, catalog, hub) = fixture();
        git(temporary.path(), &["switch", "main"]);
        add(&hub, &catalog, "T-1", NOW);
        SnapshotWriter::default()
            .write(&hub, &catalog, "host", true, || Ok(NOW))
            .unwrap();
        let private = read_snapshot(&catalog).unwrap().unwrap();
        let ack = hub.lock().unwrap().snapshot_ack().unwrap().clone();
        if key == "hub" {
            git(temporary.path(), &["switch", "--detach", &private.commit]);
        }
        let path = temporary.path().join(".towermd/config.yaml");
        let mut config: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        config[key] = match key {
            "project_id" => "another-project",
            "docs_dir" => "different/docs",
            "hub" => "another-hub",
            _ => unreachable!(),
        }
        .into();
        fs::write(&path, serde_yaml::to_string(&config).unwrap()).unwrap();
        git(temporary.path(), &["add", ".towermd/config.yaml"]);
        git(
            temporary.path(),
            &["commit", "-qm", "change project context"],
        );
        if key == "hub" {
            let changed = git(temporary.path(), &["rev-parse", "HEAD"]);
            git(
                temporary.path(),
                &["update-ref", "refs/towermd/snapshot", &changed],
            );
        }
        let current = Catalog::load(temporary.path()).unwrap();
        let heads = git(temporary.path(), &["show-ref"]);
        let error = read_snapshot(&current)
            .err()
            .expect("private context must not be treated as missing state");
        assert!(
            error.contains("私有快照与当前权威上下文不相容"),
            "{key}: {error}"
        );
        assert!(SnapshotWriter::default()
            .write(&hub, &current, "host", true, || Ok(NOW + 1000))
            .is_err());
        assert_eq!(git(temporary.path(), &["show-ref"]), heads);
        assert_eq!(hub.lock().unwrap().snapshot_ack(), Some(&ack));
    }
}

#[test]
fn authority_cache_tracks_private_head_without_rereading_unchanged_archives() {
    use towermd_core::snapshot_service::validate_authority;
    let (temporary, catalog, hub) = fixture();
    git(temporary.path(), &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let private = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap()
        .commit
        .unwrap();
    validate_authority(&mut hub.lock().unwrap(), &catalog).unwrap();
    let reads = hub.lock().unwrap().authority_reads();
    for _ in 0..10 {
        validate_authority(&mut hub.lock().unwrap(), &catalog).unwrap();
    }
    assert_eq!(hub.lock().unwrap().authority_reads(), reads);
    let tree = git(
        temporary.path(),
        &["rev-parse", &format!("{private}^{{tree}}")],
    );
    let code = git(
        temporary.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &private,
            "-m",
            "ordinary private head",
        ],
    );
    git(
        temporary.path(),
        &["update-ref", "refs/towermd/snapshot", &code],
    );
    validate_authority(&mut hub.lock().unwrap(), &catalog).unwrap();
    assert_eq!(hub.lock().unwrap().authority_reads(), reads + 1);
    assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().commit, private);
    validate_authority(&mut hub.lock().unwrap(), &catalog).unwrap();
    assert_eq!(hub.lock().unwrap().authority_reads(), reads + 1);
}

#[test]
fn private_cas_failure_does_not_acknowledge_an_unpublished_checkpoint() {
    let (temporary, catalog, hub) = fixture();
    git(temporary.path(), &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let tree = git(temporary.path(), &["rev-parse", "HEAD^{tree}"]);
    let winner = git(
        temporary.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &catalog.revision,
            "-m",
            "independent winner",
        ],
    );
    let mut calls = 0;
    let error = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || {
            calls += 1;
            if calls == 4 {
                git(
                    temporary.path(),
                    &["update-ref", "refs/towermd/snapshot", &winner],
                );
            }
            Ok(NOW)
        })
        .unwrap_err();
    assert!(error.contains("CAS"), "{error}");
    assert!(hub.lock().unwrap().snapshot_ack().is_none());
    assert_eq!(
        git(temporary.path(), &["rev-parse", "refs/towermd/snapshot"]),
        winner
    );
    assert_eq!(
        git(temporary.path(), &["rev-parse", "main"]),
        catalog.revision
    );
}

#[test]
fn private_snapshot_promotes_without_new_business_and_keeps_new_main_code() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    git(root, &["switch", "main"]);
    add(&hub, &catalog, "T-old", NOW - 40 * 86_400_000);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    let source = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let saved = towermd_core::snapshot_service::read_snapshot(&catalog)
        .unwrap()
        .unwrap();
    assert_eq!(saved.archives.len(), 2);
    let opaque = hub.lock().unwrap().base_snapshot().unwrap().clone();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/new.rs"), "winner code\n").unwrap();
    git(root, &["add", "src/new.rs"]);
    git(root, &["commit", "-qm", "new main code"]);
    let base = git(root, &["rev-parse", "main"]);
    git(root, &["switch", "snapshot-fixture"]);
    let catalog = Catalog::load(root).unwrap();
    let report = writer
        .write(&hub, &catalog, "host", false, || Ok(NOW + 1))
        .unwrap();
    assert_eq!(report.status, "written");
    let commit = report.commit.unwrap();
    assert_eq!(git(root, &["rev-parse", &format!("{commit}^1")]), base);
    assert_eq!(
        git(root, &["show", &format!("{commit}:src/new.rs")]),
        "winner code"
    );
    let next = towermd_core::snapshot_service::read_snapshot(&Catalog::load(root).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(next.snapshot, saved.snapshot);
    assert_eq!(next.archives, saved.archives);
    assert_eq!(hub.lock().unwrap().base_snapshot(), Some(&opaque));
    assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().commit, commit);
    assert_eq!(
        git(root, &["rev-parse", "refs/towermd/snapshot"]),
        source.commit.unwrap()
    );
    assert_eq!(
        git(
            root,
            &[
                "diff",
                "--name-only",
                &base,
                &commit,
                "--",
                ".",
                ":(exclude)docs/tower/state"
            ]
        ),
        ""
    );
}

#[test]
fn new_holder_advances_private_snapshot_to_main_without_business_changes() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    git(root, &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    let source = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    drop(hub);
    let hub = Mutex::new(support::open(
        &catalog.common_dir.join("towermd"),
        &catalog,
        0,
    ));
    git(root, &["switch", "snapshot-fixture"]);
    let report = writer
        .write(&hub, &catalog, "new-host", false, || Ok(NOW + 120_001))
        .unwrap();
    assert_eq!(report.status, "written");
    assert!(report.seq > source.seq);
    assert!(report.epoch > source.epoch);
    let saved = towermd_core::snapshot_service::read_snapshot(&Catalog::load(root).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(saved.snapshot.writer_epoch, report.epoch);
    assert_eq!(
        saved.source,
        towermd_core::snapshot_service::SnapshotSource::Main
    );
    assert_eq!(
        git(root, &["rev-parse", "refs/towermd/snapshot"]),
        source.commit.unwrap()
    );
}

#[test]
fn promote_rechecks_live_main_lease() {
    let (temporary, catalog, hub) = fixture();
    git(temporary.path(), &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    git(temporary.path(), &["switch", "snapshot-fixture"]);
    let ack = hub.lock().unwrap().snapshot_ack().unwrap().clone();
    let mut calls = 0;
    let error = writer
        .write(&hub, &catalog, "host", false, || {
            calls += 1;
            Ok(if calls == 1 { NOW } else { NOW + 120_001 })
        })
        .unwrap_err();
    assert!(error.contains("租约已失效"), "{error}");
    assert_eq!(hub.lock().unwrap().snapshot_ack(), Some(&ack));
    assert_eq!(
        git(temporary.path(), &["rev-parse", "main"]),
        catalog.revision
    );
}

#[test]
fn promote_stops_at_higher_main_fence() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    git(root, &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let ack = hub.lock().unwrap().snapshot_ack().unwrap().clone();
    git(
        root,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "higher historical fence\n\nTowermd-Snapshot: 100\nTowermd-Epoch: 100",
        ],
    );
    let base = git(root, &["rev-parse", "main"]);
    git(root, &["switch", "snapshot-fixture"]);
    let catalog = Catalog::load(root).unwrap();
    let report = writer
        .write(&hub, &catalog, "host", false, || Ok(NOW + 1))
        .unwrap();
    assert_eq!(report.status, "superseded");
    assert_eq!(git(root, &["rev-parse", "main"]), base);
    assert_eq!(hub.lock().unwrap().snapshot_ack(), Some(&ack));
}

#[test]
fn promote_ack_failure_reconciles_after_restart() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    git(root, &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    let private = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap()
        .commit
        .unwrap();
    git(root, &["switch", "snapshot-fixture"]);
    let journal = catalog.common_dir.join("towermd/journal.jsonl");
    let backup = catalog.common_dir.join("towermd/saved-promotion.jsonl");
    fs::rename(&journal, &backup).unwrap();
    fs::create_dir(&journal).unwrap();
    let error = writer
        .write(&hub, &catalog, "host", false, || Ok(NOW + 1))
        .unwrap_err();
    let published = git(root, &["rev-parse", "main"]);
    assert!(
        error.contains(&published) && error.contains("hub 确认失败"),
        "{error}"
    );
    assert_ne!(published, catalog.revision);
    fs::remove_dir(&journal).unwrap();
    fs::rename(&backup, &journal).unwrap();
    drop(hub);
    let catalog = Catalog::load(root).unwrap();
    let hub = Mutex::new(support::open(
        &catalog.common_dir.join("towermd"),
        &catalog,
        0,
    ));
    for _ in 0..2 {
        let report = writer
            .write(&hub, &catalog, "host", false, || Ok(NOW + 2))
            .unwrap();
        assert_eq!(report.status, "unchanged");
    }
    assert_eq!(
        hub.lock().unwrap().snapshot_ack().unwrap().commit,
        published
    );
    let eid = towermd_core::events::derived_eid(&published, "snapshot.written").unwrap();
    assert_eq!(
        hub.lock()
            .unwrap()
            .events(0)
            .iter()
            .filter(|event| event.eid == eid)
            .count(),
        1
    );
    assert_eq!(git(root, &["rev-parse", "main"]), published);
    assert_eq!(git(root, &["rev-parse", "refs/towermd/snapshot"]), private);
}

#[test]
fn private_snapshot_promotes_to_origin_without_changing_local_heads() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    git(root, &["switch", "main"]);
    add(&hub, &catalog, "T-old", NOW - 40 * 86_400_000);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let saved = towermd_core::snapshot_service::read_snapshot(&catalog)
        .unwrap()
        .unwrap();
    let remote = tempfile::tempdir().unwrap();
    git(
        root,
        &["clone", "--bare", ".", remote.path().to_str().unwrap()],
    );
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    towermd_core::origin::refresh_origin_main(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/new.rs"), "remote winner\n").unwrap();
    git(root, &["add", "src/new.rs"]);
    git(root, &["commit", "-qm", "remote new code"]);
    git(root, &["push", "origin", "main"]);
    let heads = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
        ],
    );
    let base = git(root, &["rev-parse", "origin/main"]);
    let catalog = Catalog::load(root).unwrap();
    let report = writer
        .write(&hub, &catalog, "host", false, || Ok(NOW + 1))
        .unwrap();
    assert_eq!(report.status, "written");
    let commit = report.commit.unwrap();
    assert_eq!(git(root, &["rev-parse", &format!("{commit}^1")]), base);
    assert_eq!(
        git(root, &["show", &format!("{commit}:src/new.rs")]),
        "remote winner"
    );
    assert_eq!(
        git(
            root,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads"
            ]
        ),
        heads
    );
    let next = towermd_core::snapshot_service::read_snapshot(&Catalog::load(root).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(next.snapshot, saved.snapshot);
    assert_eq!(next.archives, saved.archives);
    assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().commit, commit);
}

#[test]
fn ordinary_merge_retains_actual_promoted_snapshot_commit() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    git(root, &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    git(root, &["switch", "snapshot-fixture"]);
    let promoted = writer
        .write(&hub, &catalog, "host", false, || Ok(NOW + 1))
        .unwrap()
        .commit
        .unwrap();
    git(root, &["switch", "-qc", "merge-side", &catalog.revision]);
    fs::write(root.join("side.txt"), "side\n").unwrap();
    git(root, &["add", "side.txt"]);
    git(root, &["commit", "-qm", "side without state"]);
    git(root, &["switch", "main"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-s",
            "ours",
            "merge-side",
            "-m",
            "ordinary merge",
        ],
    );
    let merge = git(root, &["rev-parse", "main"]);
    assert_ne!(merge, promoted);
    let saved = towermd_core::snapshot_service::read_snapshot(&Catalog::load(root).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(saved.commit, promoted);
    fs::write(root.join("later.txt"), "later code\n").unwrap();
    git(root, &["add", "later.txt"]);
    git(root, &["commit", "-qm", "ordinary code after merge"]);
    let saved = towermd_core::snapshot_service::read_snapshot(&Catalog::load(root).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(saved.commit, promoted);
}

#[test]
fn promote_rejects_source_changed_by_final_lease_callback() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    git(root, &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    let private = writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap()
        .commit
        .unwrap();
    git(root, &["switch", "snapshot-fixture"]);
    let tree = git(root, &["rev-parse", &format!("{private}^{{tree}}")]);
    let changed = git(
        root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &private,
            "-m",
            "changed private source",
        ],
    );
    let mut calls = 0;
    let result = writer.write(&hub, &catalog, "host", false, || {
        calls += 1;
        if calls == 3 {
            git(root, &["update-ref", "refs/towermd/snapshot", &changed]);
        }
        Ok(NOW + 1)
    });
    assert!(
        result.is_err(),
        "a changed exact promotion source must not be published"
    );
    assert_eq!(git(root, &["rev-parse", "main"]), catalog.revision);
    assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().commit, private);
}

#[test]
fn ordinary_merge_can_adopt_a_verified_snapshot_from_second_parent() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    add(&hub, &catalog, "T-1", NOW);
    let source = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap()
        .commit
        .unwrap();
    git(root, &["branch", "published-side", &source]);
    git(root, &["update-ref", "refs/heads/main", &catalog.revision]);
    git(root, &["switch", "main"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "published-side",
            "-m",
            "adopt verified snapshot",
        ],
    );
    let saved = towermd_core::snapshot_service::read_snapshot(&Catalog::load(root).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(saved.commit, source);
    assert_ne!(saved.authority_revision, source);
}

#[test]
fn ordinary_state_change_without_snapshot_footer_is_rejected() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    add(&hub, &catalog, "T-1", NOW);
    SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    git(root, &["switch", "main"]);
    let path = root.join("docs/tower/state/state.json");
    let snapshot: Snapshot = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();
    git(root, &["add", "docs/tower/state/state.json"]);
    git(
        root,
        &["commit", "-qm", "state bytes changed without footer"],
    );
    let error = towermd_core::snapshot_service::read_snapshot(&Catalog::load(root).unwrap())
        .err()
        .unwrap();
    assert!(error.contains("state 变更缺少提交 trailer"), "{error}");
}

#[test]
fn equivalent_main_appearing_during_promotion_only_reconciles_ack() {
    let (temporary, catalog, hub) = fixture();
    let root = temporary.path();
    git(root, &["switch", "main"]);
    add(&hub, &catalog, "T-1", NOW);
    let writer = SnapshotWriter::default();
    writer
        .write(&hub, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let saved = towermd_core::snapshot_service::read_snapshot(&catalog)
        .unwrap()
        .unwrap();
    git(root, &["switch", "snapshot-fixture"]);
    let mut calls = 0;
    let mut winner = String::new();
    let report = writer
        .write(&hub, &catalog, "host", false, || {
            calls += 1;
            if calls == 2 {
                winner = snapshot_variant(root, &saved.commit, &saved.snapshot, &saved.archives);
                git(root, &["update-ref", "refs/heads/main", &winner]);
            }
            Ok(NOW + 1)
        })
        .unwrap();
    assert_eq!(report.status, "unchanged");
    assert_eq!(git(root, &["rev-parse", "main"]), winner);
    assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().commit, winner);
    assert_eq!(hub.lock().unwrap().base_snapshot(), Some(&saved.snapshot));
}

#[test]
fn higher_verified_remote_snapshot_stops_retry_without_acknowledging_loser() {
    use towermd_core::hub_protocol::AcquireLease;
    let (temporary, _, hub) = fixture();
    let root = temporary.path();
    let remote = tempfile::tempdir().unwrap();
    git(
        root,
        &["clone", "--bare", ".", remote.path().to_str().unwrap()],
    );
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    towermd_core::origin::refresh_origin_main(root).unwrap();
    let catalog = Catalog::load(root).unwrap();
    add(&hub, &catalog, "T-1", NOW);
    let other = Mutex::new(support::open(
        &temporary.path().join("other-history"),
        &catalog,
        0,
    ));
    other
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .lease_acquire(
            AcquireLease {
                host: "other-host".into(),
                names: vec!["unused".into()],
                holder: "other".into(),
                ttl_ms: 120_000,
            },
            NOW,
        )
        .unwrap();
    let mut calls = 0;
    let mut winner = None;
    let report = SnapshotWriter::default()
        .write(&hub, &catalog, "host", true, || {
            calls += 1;
            if calls == 3 {
                winner = Some(
                    SnapshotWriter::default()
                        .write(&other, &catalog, "other-host", true, || Ok(NOW))
                        .unwrap(),
                );
            }
            Ok(NOW)
        })
        .unwrap();
    let winner = winner.unwrap();
    assert_eq!(report.status, "superseded");
    assert_eq!(report.commit, winner.commit);
    // The observed higher source stops before a fourth lease callback; no losing push/ack.
    assert_eq!(calls, 3);
    assert!(hub.lock().unwrap().snapshot_ack().is_none());
    let saved = towermd_core::snapshot_service::read_snapshot(&Catalog::load(root).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(saved.commit, winner.commit.unwrap());
    assert_eq!(saved.snapshot.writer_epoch, winner.epoch);
    assert_eq!(git(root, &["rev-parse", "origin/main"]), saved.commit);
}

#[test]
fn writer_callback_rejects_new_canonical_before_publishing_captured_checkpoint() {
    use towermd_core::hub_protocol::NewEvent;
    let (temporary, catalog, hub) = fixture();
    add(&hub, &catalog, "T-1", NOW);
    let before = git(temporary.path(), &["show-ref"]);
    let mut calls = 0;
    let result=SnapshotWriter::default().write(&hub,&catalog,"host",true,||{calls+=1;if calls==2{hub.lock().unwrap().protocol().unwrap().append(NewEvent{eid:uuid::Uuid::now_v7().to_string(),host:"host".into(),actor:"process:towermd-recovery".into(),kind:"merge.done".into(),layer:"L5".into(),obj:"T-1".into(),data:json!({"commit":"a".repeat(40),"task":{"type":"Task","id":"T-1","state":"merged"},"proof":{"schema":1}}),guards:vec![]},NOW).unwrap();}Ok(NOW)});
    let error = result.unwrap_err();
    assert!(error.contains("canonical"), "{error}");
    assert_eq!(git(temporary.path(), &["show-ref"]), before);
    assert!(hub.lock().unwrap().snapshot_ack().is_none());
    assert_ne!(
        hub.lock().unwrap().projection().unwrap().objects["Task"]["T-1"]["state"],
        "merged"
    );
}
