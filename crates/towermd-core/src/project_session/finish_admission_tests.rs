//! Real accepted admission is an immutable response observation, never current
//! state or execution authority. No ticket or receipt is fabricated here.
use super::*;
use crate::{
    ask_fixture::{self as fixture, git, Fixture, NOW},
    hub_protocol::*,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

fn begin(f: &mut Fixture, id: &str) -> Result<FinishStart, String> {
    begin_at(f, id, NOW + 20)
}
fn begin_at(f: &mut Fixture, id: &str, now: u64) -> Result<FinishStart, String> {
    ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        id,
        None,
        &mut f.outbox,
        || Ok(now),
    )
}
fn queued(result: FinishStart) -> FinishTicket {
    let FinishStart::Queued(ticket) = result else {
        panic!("expected actual Queued producer")
    };
    ticket
}
fn full(f: &mut Fixture, id: &str) -> Result<Option<finish::FinishView>, String> {
    full_at(f, id, NOW + 22)
}
fn full_at(f: &mut Fixture, id: &str, now: u64) -> Result<Option<finish::FinishView>, String> {
    ProjectSession::finish_view_shared(&f.session, &f.catalog, "host", id, &mut f.outbox, || {
        Ok(now)
    })
}
fn started(f: &Fixture, id: &str) -> Event {
    let eid = request_event_eid(id, "finish.started").unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let events: Vec<_> = inputs
        .confirmed()
        .iter()
        .filter(|event| event.eid == eid)
        .collect();
    assert_eq!(events.len(), 1);
    events[0].clone()
}
fn fields(view: &finish::FinishView) -> Value {
    json!({"status":view.status,"run_state":view.run_state,"actual":view.actual,
        "evidence":view.evidence,"report":view.report})
}
fn advance_main(root: &Path) {
    let old = git(root, &["rev-parse", "main"]);
    let tree = git(root, &["rev-parse", "main^{tree}"]);
    let next = git(
        root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &old,
            "-m",
            "actual accepted source advance",
        ],
    );
    git(root, &["update-ref", "refs/heads/main", &next, &old]);
}

#[test]
fn new_admission_uses_actual_recorded_run_and_has_no_terminal_evidence_or_read_side_effects() {
    let mut f = fixture::claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let ticket = queued(begin(&mut f, &id).unwrap());
    let actual = started(&f, &id);
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
    let observed: Started = serde_json::from_value(actual.data.clone()).unwrap();
    let admission = ticket.admission_view().unwrap();
    assert_eq!(
        admission.status,
        finish::status_in(&observed.run, &observed.input.evidence_id).unwrap()
    );
    assert_eq!(admission.run_state, observed.run.state);
    assert_eq!(admission.status, *ticket.status());
    assert_eq!(admission.status.phase, JobPhase::Queued);
    assert!(
        admission.actual.is_none() && admission.evidence.is_none() && admission.report.is_none()
    );
    let (projection, head, reads) = {
        let live = f.session.lock().unwrap();
        (
            live.projection().unwrap().clone(),
            live.head.clone(),
            live.authority_reads(),
        )
    };
    assert_eq!(
        admission.run_state,
        projection.objects["Run"][&observed.run.id]["state"]
            .as_str()
            .unwrap()
    );
    let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
    let original = fields(admission);
    assert_eq!(fields(ticket.admission_view().unwrap()), original);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    let live = f.session.lock().unwrap();
    assert_eq!(live.projection().unwrap(), &projection);
    assert_eq!(live.head, head);
    assert_eq!(live.authority_reads(), reads);
    drop(live);
    assert_eq!(fields(&full(&mut f, &id).unwrap().unwrap()), original);
    assert_eq!(started(&f, &id), actual);
}

#[test]
fn original_uuid_and_reopened_job_never_mint_a_new_admission() {
    let mut f = fixture::claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let ticket = queued(begin(&mut f, &id).unwrap());
    let accepted = fields(ticket.admission_view().unwrap());
    let original = started(&f, &id);
    let resumed = queued(begin(&mut f, &id).unwrap());
    assert!(resumed.admission_view().is_none());
    assert_eq!(fields(&full(&mut f, &id).unwrap().unwrap()), accepted);
    let mut f = fixture::reopen(f, NOW + 23);
    let reopened = queued(begin_at(&mut f, &id, NOW + 24).unwrap());
    assert!(reopened.admission_view().is_none());
    assert_eq!(started(&f, &id), original);
    assert_eq!(
        fields(&full_at(&mut f, &id, NOW + 25).unwrap().unwrap()),
        accepted
    );
}

#[test]
fn failed_started_confirmation_returns_no_ticket_then_same_original_repairs_without_admission() {
    let mut f = fixture::claimed();
    let id = uuid::Uuid::now_v7().to_string();
    f.outbox.fail_next_confirmation();
    let error = begin(&mut f, &id)
        .expect_err("failed confirmed fsync must not return any ticket/admission");
    assert!(error.contains(&id), "{error}");
    let original = started(&f, &id);
    assert!(!f.outbox.read_inputs().unwrap().receipt_confirmed(&original));
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Run"]
            [original.data["run"]["id"].as_str().unwrap()]["state"],
        original.data["run"]["state"],
        "actual accepted Hub fact is still visible"
    );
    let repaired = queued(begin(&mut f, &id).unwrap());
    assert!(
        repaired.admission_view().is_none(),
        "repair success is not a new admission"
    );
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&original));
    assert_eq!(started(&f, &id), original);
}

// Delegate every method to the sole actual owned Hub. Only after its genuine
// started append has succeeded, advance main to force the real final sync gate.
struct AdvanceAfterStarted {
    inner: Box<dyn HubProtocol>,
    root: PathBuf,
    fired: bool,
}
impl HubProtocol for AdvanceAfterStarted {
    fn append(&mut self, event: NewEvent, now: u64) -> Result<AppendReceipt, HubError> {
        let starting = event.kind == "finish.started";
        let receipt = self.inner.append(event, now)?;
        if starting && !self.fired && matches!(&receipt, AppendReceipt::Event(_)) {
            self.fired = true;
            advance_main(&self.root);
        }
        Ok(receipt)
    }
    fn subscribe(&mut self, after: u64, now: u64) -> Result<SubscriptionBatch, HubError> {
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
    fn snapshot_ack(&mut self, ack: SnapshotAck, checkpoint: Value) -> Result<(), HubError> {
        self.inner.snapshot_ack(ack, checkpoint)
    }
    fn rebuild(&mut self, seed: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        self.inner.rebuild(seed)
    }
    fn contiguous_seq(&self) -> u64 {
        self.inner.contiguous_seq()
    }
}

#[test]
fn accepted_started_with_failed_final_sync_has_no_ticket_or_admission() {
    let mut f = fixture::claimed();
    // The temporary Hub holds only the field during a same-lock move of the
    // real owner; it never supplies an event, source or positive assertion.
    let temporary = tempfile::tempdir().unwrap();
    {
        let mut live = f.session.lock().unwrap();
        let inner = std::mem::replace(
            &mut live.hub,
            Box::new(crate::hub::Hub::open(temporary.path()).unwrap()),
        );
        live.hub = Box::new(AdvanceAfterStarted {
            inner,
            root: f.root.path().to_path_buf(),
            fired: false,
        });
    }
    let id = uuid::Uuid::now_v7().to_string();
    let error = begin(&mut f, &id)
        .expect_err("accepted but unsynchronized request must not return a ticket/admission");
    assert!(
        error.contains(&id) && error.contains("authority changed"),
        "{error}"
    );
    let original = started(&f, &id);
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&original));
    assert!(
        full(&mut f, &id).is_err(),
        "old source cannot be read as current"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let resumed = queued(begin(&mut f, &id).unwrap());
    assert!(resumed.admission_view().is_none());
    assert_eq!(started(&f, &id), original);
}

#[test]
fn accepted_admission_survives_later_source_change_but_launch_and_wait_still_check_actual_source() {
    for source in ["code", "blob"] {
        let mut f = fixture::claimed();
        let id = uuid::Uuid::now_v7().to_string();
        let ticket = queued(begin(&mut f, &id).unwrap());
        let admission = fields(ticket.admission_view().unwrap());
        let original = started(&f, &id);
        let mut restore = None;
        if source == "code" {
            let changed = tempfile::tempdir().unwrap();
            let directory = changed.path().join("main-change");
            git(
                f.root.path(),
                &[
                    "worktree",
                    "add",
                    "--detach",
                    directory.to_str().unwrap(),
                    "main",
                ],
            );
            fs::write(directory.join("alpha/a.py"), "# actual new main code\n").unwrap();
            git(&directory, &["add", "alpha/a.py"]);
            git(
                &directory,
                &[
                    "commit",
                    "-qm",
                    "actual code authority advance after admission",
                ],
            );
            let next = git(&directory, &["rev-parse", "HEAD"]);
            git(f.root.path(), &["update-ref", "refs/heads/main", &next]);
            git(
                f.root.path(),
                &["worktree", "remove", directory.to_str().unwrap()],
            );
        } else {
            let path = &f.catalog.object_paths["A-alpha"];
            let oid = git(
                f.root.path(),
                &["rev-parse", &format!("{}:{path}", f.catalog.revision)],
            );
            let loose = f
                .catalog
                .common_dir
                .join("objects")
                .join(&oid[..2])
                .join(&oid[2..]);
            let bytes = fs::read(&loose).unwrap();
            fs::remove_file(&loose).unwrap();
            restore = Some((loose, bytes));
        }
        // A past actual acceptance is not revoked by the later observation gap.
        assert_eq!(fields(ticket.admission_view().unwrap()), admission);
        let error = full(&mut f, &id).expect_err("wait must still prove its actual source");
        assert!(error.contains(&id), "{source}: {error}");
        if source == "code" {
            f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
        }
        assert!(
            ProjectSession::launch_finish_shared(
                &f.session,
                &f.catalog,
                ticket,
                &mut f.outbox,
                || Ok(NOW + 24)
            )
            .is_err(),
            "{source} cannot execute by reusing admission"
        );
        assert!(!f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .any(|event| event.kind == "finish.running"));
        assert_eq!(started(&f, &id), original);
        if let Some((path, bytes)) = restore {
            fs::write(path, bytes).unwrap();
        }
    }
}

#[test]
fn actual_rebuild_restores_original_history_but_never_new_admission_or_old_execution_authority() {
    let mut f = fixture::claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let ticket = queued(begin(&mut f, &id).unwrap());
    let admission = fields(ticket.admission_view().unwrap());
    let original = started(&f, &id);
    let report = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 25),
            &mut f.outbox,
        )
        .unwrap();
    assert!(matches!(report.status.as_str(), "written" | "unchanged"));
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    drop(f.session);
    fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let request = crate::recovery::prepare(
        &f.catalog,
        &inputs,
        NOW + 30,
        NOW + 300_000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (report, session) = f
        .writer
        .confirm_rebuild(
            None,
            &f.catalog,
            &request,
            "human:owner",
            request.expires_at,
            &inputs,
            "host",
            || Ok(NOW + 31),
        )
        .unwrap();
    assert_eq!(report.status, "rebuilt");
    f.session = session;
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_eq!(
        fields(ticket.admission_view().unwrap()),
        admission,
        "past acceptance stays an observation only"
    );
    let view = full_at(&mut f, &id, NOW + 32).unwrap().unwrap();
    assert_eq!(view.status.request_id, id);
    assert_eq!(view.status.input, ticket.status.input);
    assert_eq!(view.run_state, "released");
    assert!(view.actual.is_none() && view.evidence.is_none() && view.report.is_none());
    assert!(
        begin_at(&mut f, &id, NOW + 33).is_err(),
        "released old Run must not mint a new Queued ticket/admission"
    );
    assert!(ProjectSession::launch_finish_shared(
        &f.session,
        &f.catalog,
        ticket,
        &mut f.outbox,
        || Ok(NOW + 34)
    )
    .is_err());
    assert_eq!(started(&f, &id), original);
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "finish.running"));
}
