//! Actual Hub suffixes racing the complete source proof, without changing its
//! original three attempts or treating a renewed observed head as business state.
use super::*;
use crate::recovery_outbox::RecoveryOutbox;
use crate::tasks::TaskSpec;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
};

thread_local! {
    static AFTER_PREPARE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

pub(super) fn after_complete_prepare() {
    AFTER_PREPARE.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook();
        }
    });
}

struct HookReset;
impl Drop for HookReset {
    fn drop(&mut self) {
        AFTER_PREPARE.with(|slot| *slot.borrow_mut() = None);
    }
}
fn observe(hook: impl FnMut() + 'static) -> HookReset {
    AFTER_PREPARE.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    HookReset
}

struct Fixture {
    root: tempfile::TempDir,
    catalog: Arc<Catalog>,
    session: Arc<Mutex<ProjectSession>>,
    outbox: RecoveryOutbox,
    run: String,
    fences: Vec<Fence>,
    audit: Event,
}
fn actual_audit(live: &mut ProjectSession, at: u64) -> Event {
    let receipt = live
        .hub
        .append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:audit".into(),
                kind: "future.audit".into(),
                layer: "L4".into(),
                obj: "actual-full-prepare".into(),
                data: serde_json::json!({"actual": true}),
                guards: vec![],
            },
            at,
        )
        .unwrap();
    let AppendReceipt::Event(event) = receipt else {
        panic!("new actual audit receipt");
    };
    *event
}
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let catalog = Arc::new(crate::test_support::catalog(root.path()));
    let mut outbox = crate::test_support::outbox(&catalog);
    let session = Arc::new(Mutex::new(
        ProjectSession::open(
            &root.path().join("hub"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            0,
        )
        .unwrap(),
    ));
    ProjectSession::execute_shared(
        &session,
        &catalog,
        &outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T".into(),
                title: "actual complete-preparation renewal race".into(),
                module: "A".into(),
                touches: vec![],
                allowed_paths: vec!["src/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC".into()],
                decisions: vec![],
            },
        },
        None,
        1,
        &mut outbox,
    )
    .unwrap();
    let claimed = ProjectSession::execute_shared(
        &session,
        &catalog,
        &outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T".into(),
            harness: None,
            harness_version: None,
        },
        None,
        2,
        &mut outbox,
    )
    .unwrap();
    let run = claimed["id"].as_str().unwrap().to_string();
    let mut live = session.lock().unwrap();
    let fences = live
        .head
        .leases
        .values()
        .map(|lease| Fence {
            name: lease.name.clone(),
            holder: lease.holder.clone(),
            epoch: lease.epoch,
        })
        .collect::<Vec<_>>();
    assert!(!fences.is_empty(), "real claimed Run owns its full fences");
    // An unknown actual audit cannot take the Task-only delta shortcut, so each
    // observation below occurs after the real complete history proof.
    let audit = actual_audit(&mut live, 3);
    drop(live);
    Fixture {
        root,
        catalog,
        session,
        outbox,
        run,
        fences,
        audit,
    }
}

#[derive(Clone, Copy)]
enum Renewal {
    Status,
    Unrefreshed,
    Intermediate,
}
fn assert_actual_renewal_installs_once(mode: Renewal) {
    let f = fixture();
    let before = f.session.lock().unwrap().projection.clone();
    let count = Rc::new(Cell::new(0));
    let visits = count.clone();
    let session = f.session.clone();
    let catalog = f.catalog.clone();
    let run = f.run.clone();
    let fences = f.fences.clone();
    let _hook = observe(move || {
        visits.set(visits.get() + 1);
        let at = 10 + visits.get() as u64 * 2;
        let mut live = session.lock().unwrap();
        if matches!(mode, Renewal::Status | Renewal::Intermediate) {
            live.renew_binding(&catalog, "host", Some(&run), at)
                .unwrap();
        }
        if matches!(mode, Renewal::Unrefreshed | Renewal::Intermediate) {
            live.hub
                .lease_renew(
                    RenewLease {
                        host: "host".into(),
                        fences: fences.clone(),
                        ttl_ms: 900_000,
                    },
                    at + 1,
                )
                .unwrap();
        }
        assert_eq!(
            live.projection, before,
            "renew never installs business state"
        );
    });
    let inputs = f.outbox.read_inputs().unwrap();
    ProjectSession::sync_shared(&f.session, &f.catalog, &inputs, 100).unwrap();
    assert_eq!(
        count.get(),
        1,
        "one actual complete proof, without extra attempts"
    );
    let mut live = f.session.lock().unwrap();
    let current = live.capture(100).unwrap();
    assert!(current.fresh.is_empty());
    assert_eq!(
        live.head, current.head,
        "exact actual system state installed"
    );
    assert_eq!(live.cursor, current.head.contiguous_seq);
    assert_eq!(live.summary.cursor, live.cursor);
    assert_eq!(live.prepared_head.as_ref(), Some(&current.head));
    assert_eq!(live.events(f.audit.seq - 1)[0], f.audit);
    assert_eq!(live.projection.objects["Run"][&f.run]["state"], "running");
    for fence in &f.fences {
        assert_eq!(live.projection.lease_summaries[&fence.name], *fence);
        assert_eq!(live.head.leases[&fence.name].holder, fence.holder);
        assert_eq!(live.head.leases[&fence.name].epoch, fence.epoch);
        assert!(live.head.leases[&fence.name].expires_at > 100);
    }
    let installed = live.capture(100).unwrap();
    let private = crate::snapshot_git::private_revision(f.root.path()).unwrap();
    let drafts = crate::snapshot_service::draft_ref_fingerprint(&f.catalog).unwrap();
    assert_eq!(
        live.prepared_key.as_ref(),
        Some(&ProjectSession::key(&installed, &f.catalog, &inputs, &private, &drafts).unwrap())
    );
}

#[test]
fn fullprepare_installs_actual_status_renewal_without_three_attempt_starvation() {
    assert_actual_renewal_installs_once(Renewal::Status);
}
#[test]
fn fullprepare_accepts_exact_old_cached_head_after_actual_unrefreshed_renewal() {
    assert_actual_renewal_installs_once(Renewal::Unrefreshed);
}
#[test]
fn fullprepare_accepts_only_an_actual_intermediate_renewal_head() {
    assert_actual_renewal_installs_once(Renewal::Intermediate);
}

fn assert_actual_nonrenew_suffix_is_not_installed(new_epoch: bool) {
    let f = fixture();
    let before = RecordedPrefix::new(&f.session.lock().unwrap());
    let count = Rc::new(Cell::new(0));
    let visits = count.clone();
    let session = f.session.clone();
    let _hook = observe(move || {
        visits.set(visits.get() + 1);
        let mut live = session.lock().unwrap();
        if new_epoch {
            live.hub
                .lease_acquire(
                    AcquireLease {
                        host: "host".into(),
                        names: vec![format!("module:new-{}", visits.get())],
                        holder: "other-run".into(),
                        ttl_ms: 900_000,
                    },
                    10 + visits.get() as u64,
                )
                .unwrap();
        } else {
            actual_audit(&mut live, 10 + visits.get() as u64);
        }
    });
    let error = ProjectSession::sync_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        100,
    )
    .unwrap_err();
    assert_eq!(count.get(), 3, "original finite retry budget remains exact");
    assert_eq!(
        error,
        SessionSyncError::SourceChanged(
            "session sources/head changed during three preparations".into()
        )
    );
    let live = f.session.lock().unwrap();
    assert!(before.business_matches(&live));
    assert_eq!(live.cursor, before.session.cursor);
    assert_eq!(live.tail, before.session.tail);
    assert!(live.hub.contiguous_seq() > live.cursor);
}
#[test]
fn fullprepare_never_installs_an_actual_new_business_suffix() {
    assert_actual_nonrenew_suffix_is_not_installed(false);
}
#[test]
fn fullprepare_never_installs_an_actual_grant_or_new_epoch_suffix() {
    assert_actual_nonrenew_suffix_is_not_installed(true);
}

#[test]
fn fullprepare_does_not_accept_an_unproved_cached_head_between_actual_renewals() {
    let f = fixture();
    let before = RecordedPrefix::new(&f.session.lock().unwrap());
    let count = Rc::new(Cell::new(0));
    let visits = count.clone();
    let session = f.session.clone();
    let catalog = f.catalog.clone();
    let run = f.run.clone();
    let _hook = observe(move || {
        visits.set(visits.get() + 1);
        let mut live = session.lock().unwrap();
        live.renew_binding(&catalog, "host", Some(&run), 10 + visits.get() as u64)
            .unwrap();
        // Fault only the derived observed head. The actual Hub still has the
        // complete real renewal, but this cache is neither the saved anchor nor
        // any actual intermediate replay head and must never close the CAS.
        live.head.leases.values_mut().next().unwrap().expires_at -= 1;
    });
    assert_eq!(
        ProjectSession::sync_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            100,
        )
        .unwrap_err(),
        SessionSyncError::SourceChanged(
            "session sources/head changed during three preparations".into()
        )
    );
    assert_eq!(count.get(), 3);
    let live = f.session.lock().unwrap();
    assert!(before.business_matches(&live));
    assert_eq!(live.cursor, before.session.cursor);
    assert_eq!(live.tail, before.session.tail);
}

#[test]
fn fullprepare_actual_main_change_after_proof_keeps_original_projection() {
    let f = fixture();
    let before = RecordedPrefix::new(&f.session.lock().unwrap());
    let root = f.root.path().to_path_buf();
    let _hook = observe(move || {
        crate::test_support::git(
            &root,
            &["commit", "--allow-empty", "-qm", "new actual main"],
        );
        let head = crate::test_support::git(&root, &["rev-parse", "HEAD"]);
        crate::test_support::git(&root, &["update-ref", "refs/heads/main", &head]);
    });
    let error = ProjectSession::sync_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        100,
    )
    .unwrap_err();
    assert_eq!(
        error,
        SessionSyncError::SourceChanged("authority changed during preparation".into())
    );
    let live = f.session.lock().unwrap();
    assert!(before.business_matches(&live));
    assert_eq!(live.cursor, before.session.cursor);
    assert_eq!(live.tail, before.session.tail);
}

#[test]
fn fullprepare_actual_private_change_cannot_rebind_the_original_request() {
    let f = fixture();
    let before = RecordedPrefix::new(&f.session.lock().unwrap());
    assert!(crate::snapshot_git::private_revision(f.root.path())
        .unwrap()
        .is_none());
    let root = f.root.path().to_path_buf();
    let _hook = observe(move || {
        let head = crate::test_support::git(&root, &["rev-parse", "HEAD"]);
        crate::test_support::git(
            &root,
            &["update-ref", crate::snapshot_git::PRIVATE_REF, &head],
        );
    });
    let error = ProjectSession::sync_shared_bound(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        100,
        Some(&None),
    )
    .unwrap_err();
    assert_eq!(
        error,
        SessionSyncError::SourceChanged("private snapshot source changed during request".into())
    );
    let live = f.session.lock().unwrap();
    assert!(before.business_matches(&live));
    assert_eq!(live.cursor, before.session.cursor);
    assert_eq!(live.tail, before.session.tail);
}
