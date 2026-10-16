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
    let hub = Mutex::new(Hub::open(&catalog.common_dir.join("towermd")).unwrap());
    (temporary, catalog, hub)
}

fn add(hub: &Mutex<Hub>, catalog: &Catalog, id: &str, now: u64) {
    tasks::execute(
        &mut hub.lock().unwrap(),
        catalog,
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
    let reopened = Hub::open(&catalog.common_dir.join("towermd")).unwrap();
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
fn checked_main_defers_without_acknowledging_unwritten_sequence() {
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
        assert_eq!(report.status, "deferred");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(hub.lock().unwrap().snapshot_ack().unwrap().seq, first.seq);
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
    let mut hub = Hub::open(&catalog.common_dir.join("towermd")).unwrap();
    let error = towermd_core::snapshot_service::validate_authority(&mut hub, &catalog).unwrap_err();
    assert!(error.contains("恢复"), "{error}");
    assert_eq!(hub.last_seq(), 0);
}

#[test]
fn git_success_without_hub_ack_is_reconciled_without_duplicate_fact() {
    let (temporary, catalog, hub) = fixture();
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
    let hub = Mutex::new(Hub::open(&catalog.common_dir.join("towermd")).unwrap());
    let catalog = Catalog::load(temporary.path()).unwrap();
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
    for _ in 0..100 {
        session
            .execute(&catalog, "host", TaskCommand::Status, None, NOW)
            .unwrap();
        session.tick("host", NOW).unwrap();
    }
    assert_eq!(session.applied_count(), applied);
    assert_eq!(session.authority_reads(), 1);
    drop(session);
    add(&hub, &catalog, "T-1", NOW);
    let mut session = hub.lock().unwrap();
    assert_eq!(session.applied_count(), applied + 1);
    for _ in 0..100 {
        session
            .execute(&catalog, "host", TaskCommand::Status, None, NOW)
            .unwrap();
        session.tick("host", NOW).unwrap();
    }
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
    let hub = Mutex::new(Hub::new(Box::new(protocol), NOW).unwrap());
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
