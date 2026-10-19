//! Actual producers and physical-source races at the private postappend boundary.
//! No accepted Event or execution capability is constructed by these fixtures.
use super::*;
use crate::ask_fixture::{self as fixture, git, Fixture, NOW};
use std::{
    cell::{Cell, RefCell},
    fs,
    rc::Rc,
};

struct Boundary<'a> {
    session: &'a Mutex<ProjectSession>,
    catalog: &'a Catalog,
    outbox: &'a mut RecoveryOutbox,
    before: &'a RecoveryInputs,
    own: &'a mut Option<OwnFinishAppend>,
    result: &'a Result<AppendReceipt, HubError>,
    now: u64,
}
type BoundaryCallback = Box<dyn for<'a> FnOnce(Boundary<'a>)>;
thread_local! {
    static BOUNDARY: RefCell<Option<BoundaryCallback>> = RefCell::new(None);
}
struct Hook;
impl Drop for Hook {
    fn drop(&mut self) {
        BOUNDARY.with(|hook| {
            hook.borrow_mut().take();
        });
    }
}
fn hook(call: impl for<'a> FnOnce(Boundary<'a>) + 'static) -> Hook {
    BOUNDARY.with(|hook| {
        assert!(hook.borrow_mut().replace(Box::new(call)).is_none());
    });
    Hook
}
#[allow(clippy::too_many_arguments)]
pub(super) fn at_boundary(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    before: &RecoveryInputs,
    own: &mut Option<OwnFinishAppend>,
    result: &Result<AppendReceipt, HubError>,
    now: u64,
) {
    if own.is_none() {
        return;
    }
    let call = BOUNDARY.with(|hook| hook.borrow_mut().take());
    if let Some(call) = call {
        call(Boundary {
            session,
            catalog,
            outbox,
            before,
            own,
            result,
            now,
        });
    }
}
fn actual(boundary: &Boundary<'_>) -> Event {
    let Ok(AppendReceipt::Event(actual)) = boundary.result else {
        panic!("fixture needs the real complete Hub receipt")
    };
    actual.as_ref().clone()
}
fn begin(f: &mut Fixture, id: &str) -> Result<FinishStart, String> {
    ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        id,
        None,
        &mut f.outbox,
        || Ok(NOW + 20),
    )
}
fn queued(start: FinishStart) -> FinishTicket {
    let FinishStart::Queued(ticket) = start else {
        panic!("new actual job must be queued")
    };
    ticket
}
fn reads(f: &Fixture) -> u64 {
    f.session.lock().unwrap().authority_reads()
}
fn saved_read_count(count: Rc<Cell<u64>>) -> Hook {
    hook(move |b| {
        assert!(b.own.is_some());
        count.set(b.session.lock().unwrap().authority_reads());
    })
}
fn assert_original(f: &Fixture, id: &str) -> Event {
    let inputs = f.outbox.read_inputs().unwrap();
    let eid = request_event_eid(id, "finish.started").unwrap();
    let originals: Vec<_> = inputs
        .confirmed()
        .iter()
        .filter(|event| event.eid == eid)
        .collect();
    assert_eq!(originals.len(), 1);
    assert!(!originals[0].stale);
    originals[0].clone()
}

#[test]
fn real_new_started_and_running_install_exact_suffix_and_reopen_identically() {
    let mut f = fixture::claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let prior_evidence = f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .get("Evidence")
        .cloned();
    let prior_holds = f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .get("Hold")
        .cloned();
    let count = Rc::new(Cell::new(u64::MAX));
    let _hook = saved_read_count(count.clone());
    let ticket = queued(begin(&mut f, &id).unwrap());
    assert_ne!(count.get(), u64::MAX);
    assert_eq!(
        reads(&f),
        count.get(),
        "postappend must not rebuild old history"
    );
    let started = assert_original(&f, &id);
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&started));
    let _hook = saved_read_count(count.clone());
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 21)
        })
        .unwrap();
    assert_eq!(
        reads(&f),
        count.get(),
        "Running must install only its actual suffix"
    );
    assert_eq!(execution.status.phase, JobPhase::Running);
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    let evidence = projection.objects.get("Evidence").unwrap();
    let prior_evidence = prior_evidence.unwrap_or_default();
    for (id, original) in &prior_evidence {
        assert_eq!(evidence.get(id), Some(original));
    }
    let added = evidence
        .iter()
        .filter(|(id, _)| !prior_evidence.contains_key(*id))
        .collect::<Vec<_>>();
    assert_eq!(
        added.len(),
        1,
        "first real successful check retains one Passed GateEvidence"
    );
    let (diagnostic_id, diagnostic) = added[0];
    assert_eq!(diagnostic["kind"], "gate");
    assert_eq!(diagnostic["gate"], "g2");
    assert_eq!(diagnostic["result"], "passed");
    assert!(diagnostic.get("task_check").is_some());
    let run_id = started.data["run"]["id"].as_str().unwrap();
    assert_eq!(
        projection.objects["Run"][run_id]["last_check"],
        json!(diagnostic_id)
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let actual_diagnostic = inputs
        .confirmed()
        .iter()
        .find(|event| event.obj == *diagnostic_id)
        .unwrap();
    assert!(crate::task_check_recorded::is_event(actual_diagnostic));
    assert!(!actual_diagnostic.stale);
    assert!(actual_diagnostic.seq < started.seq);
    assert_eq!(projection.objects.get("Hold"), prior_holds.as_ref());
    drop(execution);
    let f = fixture::reopen(f, NOW + 22);
    assert_eq!(f.session.lock().unwrap().projection().unwrap(), &projection);
}

#[test]
fn actual_renewal_between_receipt_and_install_uses_finite_system_replay() {
    let mut f = fixture::claimed();
    let count = Rc::new(Cell::new(u64::MAX));
    let seen = count.clone();
    let _hook = hook(move |b| {
        let mut live = b.session.lock().unwrap();
        seen.set(live.authority_reads());
        live.renew_binding(b.catalog, "host", None, b.now + 1)
            .unwrap();
    });
    let id = uuid::Uuid::now_v7().to_string();
    let _ticket = queued(begin(&mut f, &id).unwrap());
    assert_eq!(reads(&f), count.get());
    let live = f.session.lock().unwrap();
    assert_eq!(live.applied_seq(), live.head.contiguous_seq);
    assert!(live.tail.iter().any(|event| matches!(
        event.lease_delta,
        Some(crate::hub::SystemDelta::Renew { .. })
    )));
}

#[test]
fn same_eid_altered_body_metadata_and_stale_are_never_installed() {
    for fault in ["body", "metadata", "stale"] {
        let mut f = fixture::claimed();
        let count = Rc::new(Cell::new(u64::MAX));
        let seen = count.clone();
        let _hook = hook(move |b| {
            let real = actual(&b);
            let mut changed = real.clone();
            match fault {
                "body" => changed.data["input"]["commands_hash"] = json!("different"),
                "metadata" => changed.cost_usd = Some(0.0),
                "stale" => changed.stale = true,
                _ => unreachable!(),
            }
            let cursor = b.session.lock().unwrap().applied_seq();
            let own = b.own.take().unwrap();
            let fresh = b.outbox.read_inputs().unwrap();
            assert!(
                !own.install(b.session, b.catalog, b.before, &fresh, &changed, b.now)
                    .unwrap_or(false),
                "{fault}"
            );
            assert_eq!(b.session.lock().unwrap().applied_seq(), cursor);
            seen.set(b.session.lock().unwrap().authority_reads());
        });
        let id = uuid::Uuid::now_v7().to_string();
        queued(begin(&mut f, &id).unwrap());
        assert!(
            reads(&f) > count.get(),
            "declined scope must use ordinary fullsync"
        );
        assert_original(&f, &id);
    }
}

#[test]
fn unrelated_physical_pending_row_cannot_be_absorbed_as_our_write() {
    let mut f = fixture::claimed();
    let outside_eid = uuid::Uuid::now_v7().to_string();
    let expected = outside_eid.clone();
    let _hook = hook(move |b| {
        let real = actual(&b);
        b.outbox
            .append_pending(&NewEvent {
                eid: outside_eid,
                host: "host".into(),
                actor: "process:outside".into(),
                kind: "future.audit".into(),
                layer: "L4".into(),
                obj: "outside".into(),
                data: json!({"outside":true}),
                guards: vec![],
            })
            .unwrap();
        let fresh = b.outbox.read_inputs().unwrap();
        assert!(b
            .own
            .take()
            .unwrap()
            .install(b.session, b.catalog, b.before, &fresh, &real, b.now,)
            .is_err());
    });
    let id = uuid::Uuid::now_v7().to_string();
    queued(begin(&mut f, &id).unwrap());
    assert_original(&f, &id);
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .iter()
        .any(|e| e.eid == expected));
}

#[test]
fn actual_unrelated_business_and_human_tail_force_fullsync_without_losing_hold() {
    for human in [false, true] {
        let mut f = fixture::claimed();
        let count = Rc::new(Cell::new(u64::MAX));
        let seen = count.clone();
        let _hook = hook(move |b| {
            let real = actual(&b);
            if human {
                ProjectSession::ask_shared(
                    b.session,
                    b.catalog,
                    "host",
                    &uuid::Uuid::now_v7().to_string(),
                    &fixture::open("Actual Human tail must survive finish synchronization?"),
                    None,
                    b.outbox,
                    || Ok(b.now + 1),
                )
                .unwrap();
            } else {
                // Actual outer business tail with no accompanying local outbox
                // row: physical-record equality alone cannot authorize reuse.
                let receipt = b
                    .session
                    .lock()
                    .unwrap()
                    .hub
                    .append(
                        NewEvent {
                            eid: uuid::Uuid::now_v7().to_string(),
                            host: "host".into(),
                            actor: "process:outside".into(),
                            kind: "future.audit".into(),
                            layer: "L4".into(),
                            obj: "outside".into(),
                            data: json!({"actual_outer":true}),
                            guards: vec![],
                        },
                        b.now + 1,
                    )
                    .unwrap();
                assert!(matches!(receipt, AppendReceipt::Event(_)));
            }
            let fresh = b.outbox.read_inputs().unwrap();
            let own = b.own.take().unwrap();
            assert!(!own
                .install(b.session, b.catalog, b.before, &fresh, &real, b.now)
                .unwrap_or(false));
            seen.set(b.session.lock().unwrap().authority_reads());
        });
        let id = uuid::Uuid::now_v7().to_string();
        queued(begin(&mut f, &id).unwrap());
        assert_original(&f, &id);
        let live = f.session.lock().unwrap();
        assert_eq!(live.applied_seq(), live.head.contiguous_seq);
        if human {
            assert_eq!(live.projection().unwrap().objects["Hold"].len(), 1);
            assert_eq!(
                live.projection().unwrap().objects["Task"]["T-ask"]["state"],
                "verifying"
            );
        } else {
            assert!(live.authority_reads() > count.get());
        }
    }
}

#[test]
fn actual_main_private_and_protected_blob_changes_decline_reuse_and_keep_receipt() {
    for source in ["main", "private", "blob"] {
        let mut f = fixture::claimed();
        let _hook = hook(move |b| {
            let real = actual(&b);
            if source == "blob" {
                let path = &b.catalog.object_paths["A-alpha"];
                let oid = git(
                    &b.catalog.root,
                    &["rev-parse", &format!("{}:{path}", b.catalog.revision)],
                );
                let loose = b
                    .catalog
                    .common_dir
                    .join("objects")
                    .join(&oid[..2])
                    .join(&oid[2..]);
                assert!(
                    loose.is_file(),
                    "fixture requires actual protected loose blob"
                );
                fs::remove_file(loose).unwrap();
                assert_eq!(
                    crate::catalog::authority_revision(&b.catalog.root).unwrap(),
                    b.catalog.revision
                );
            } else {
                let tree = git(
                    &b.catalog.root,
                    &["rev-parse", &format!("{}^{{tree}}", b.catalog.revision)],
                );
                let next = git(
                    &b.catalog.root,
                    &[
                        "commit-tree",
                        &tree,
                        "-p",
                        &b.catalog.revision,
                        "-m",
                        "actual source drift",
                    ],
                );
                let reference = if source == "main" {
                    "refs/heads/main"
                } else {
                    crate::snapshot_git::PRIVATE_REF
                };
                git(&b.catalog.root, &["update-ref", reference, &next]);
            }
            let fresh = b.outbox.read_inputs().unwrap();
            let cursor = b.session.lock().unwrap().applied_seq();
            assert!(
                b.own
                    .take()
                    .unwrap()
                    .install(b.session, b.catalog, b.before, &fresh, &real, b.now,)
                    .is_err(),
                "{source}"
            );
            assert_eq!(b.session.lock().unwrap().applied_seq(), cursor);
        });
        let id = uuid::Uuid::now_v7().to_string();
        let error = match begin(&mut f, &id) {
            Err(error) => error,
            Ok(_) => panic!("actual source drift must retain the ordinary source refusal"),
        };
        assert!(error.contains(&id), "{error}");
        assert_original(&f, &id);
    }
}

#[test]
fn failed_confirmation_uses_original_fullsync_then_same_uuid_repairs_one_receipt() {
    let mut f = fixture::claimed();
    let count = Rc::new(Cell::new(u64::MAX));
    let seen = count.clone();
    f.outbox.fail_next_confirmation();
    let _hook = hook(move |b| {
        assert!(b.result.is_err());
        let own = b.own.take().unwrap();
        let fresh = b.outbox.read_inputs().unwrap();
        let actual = fresh
            .confirmed()
            .iter()
            .find(|e| e.eid == own.original.eid)
            .unwrap();
        assert!(!fresh.receipt_confirmed(actual));
        assert!(!own
            .install(b.session, b.catalog, b.before, &fresh, actual, b.now)
            .unwrap());
        seen.set(b.session.lock().unwrap().authority_reads());
    });
    let id = uuid::Uuid::now_v7().to_string();
    let error = match begin(&mut f, &id) {
        Err(error) => error,
        Ok(_) => panic!("fsync failed"),
    };
    assert!(error.contains(&id));
    assert!(reads(&f) > count.get());
    let original = assert_original(&f, &id);
    queued(begin(&mut f, &id).unwrap());
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.receipt_confirmed(&original));
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.eid == original.eid)
            .count(),
        1
    );
}

#[test]
fn real_reprepared_session_does_not_consume_old_scope_or_authorize_old_ticket() {
    let mut f = fixture::claimed();
    let _hook = hook(move |b| {
        let real = actual(&b);
        let old_identity = b.session.lock().unwrap().snapshot_session_id();
        // Move the sole actual Hub into a genuinely newly prepared Session.
        // The temporary Hub only holds the old Session's field during transfer.
        let temporary = tempfile::tempdir().unwrap();
        let placeholder = Box::new(crate::hub::Hub::open(temporary.path()).unwrap());
        let hub = std::mem::replace(&mut b.session.lock().unwrap().hub, placeholder);
        let fresh = b.outbox.read_inputs().unwrap();
        let replacement = ProjectSession::new(hub, b.catalog, &fresh, b.now).unwrap();
        assert_ne!(replacement.snapshot_session_id(), old_identity);
        *b.session.lock().unwrap() = replacement;
        assert!(!b
            .own
            .take()
            .unwrap()
            .install(b.session, b.catalog, b.before, &fresh, &real, b.now,)
            .unwrap());
    });
    let id = uuid::Uuid::now_v7().to_string();
    let old_ticket = queued(begin(&mut f, &id).unwrap());
    assert!(ProjectSession::launch_finish_shared(
        &f.session,
        &f.catalog,
        old_ticket,
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .is_err());
    let original = assert_original(&f, &id);
    let _fresh_ticket = queued(begin(&mut f, &id).unwrap());
    assert_eq!(assert_original(&f, &id), original);
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|e| e.kind == "finish.running"));
}
