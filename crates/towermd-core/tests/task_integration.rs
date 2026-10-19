//! Real core Task4 producers: actual B0, Run, verifier and Git main entries.
#[path = "support/finish_fixture.rs"]
mod fixture;
use fixture::*;
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use towermd_core::{
    baseline_review::review_baseline_shared,
    catalog::Catalog,
    finish::{self, EvidenceResult, FinishStart},
    human_intervention::{HumanCommand, ReviewGate, ReviewVerdict},
    project_session::ProjectSession,
    recovery_contract::{baseline::Verdict, recorded::TaskReviewAction},
    snapshot_service::PublicationReport,
    tasks::{TaskCommand, TaskSpec},
};
fn ready(commands: &[&str], high: bool) -> Fixture {
    let mut f=configured_fixture(&[("docs/tower/specs/AC-finish.md","---\ntype: Acceptance\nid: AC-finish\nmodule: A-alpha\ncriteria: [actual sealed verification]\n---\n")], commands, high);
    let approve = action(&f, Verdict::Approved);
    let receipt = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &approve,
        "human:owner",
        "host",
        NOW + 3,
    )
    .unwrap();
    assert!(matches!(
        f.writer
            .publish_baseline(
                &f.session,
                &f.catalog,
                &mut f.outbox,
                &receipt.receipt.result_ref.approval_eid,
                "host",
                || Ok(NOW + 5)
            )
            .unwrap(),
        PublicationReport::Recorded { .. }
    ));
    git(f.root.path(), &["switch", "-qc", "t/T-finish", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-finish".into(),
                title: "actual finish".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-finish".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 10,
        &mut f.outbox,
    )
    .unwrap();
    f
}
fn claim(f: &mut Fixture, now: u64) -> Result<serde_json::Value, towermd_core::tasks::TaskFailure> {
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-finish".into(),
            harness: None,
            harness_version: None,
        },
        None,
        now,
        &mut f.outbox,
    )
}
fn claimed(commands: &[&str], high: bool) -> Fixture {
    let mut f = ready(commands, high);
    claim(&mut f, NOW + 11).unwrap();
    f
}
fn completed(f: &mut Fixture, id: &str, now: u64) -> finish::FinishOutcome {
    let queued = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        id,
        None,
        &mut f.outbox,
        || Ok(now),
    )
    .unwrap();
    let FinishStart::Queued(ticket) = queued else {
        panic!("new real finish must be queued")
    };
    let ticket =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(now + 1)
        })
        .unwrap();
    let executed = finish::execute(
        ticket,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    ProjectSession::record_finish_shared(&f.session, &f.catalog, executed, &mut f.outbox, || {
        Ok(now + 2)
    })
    .unwrap()
}
#[derive(Clone)]
struct InterruptedFinishConfirmation {
    original: towermd_core::hub_protocol::NewEvent,
    actual: towermd_core::hub_protocol::Event,
    path: std::path::PathBuf,
    backup: std::path::PathBuf,
}
type FinishConfirmationCapture = Arc<std::sync::Mutex<Option<InterruptedFinishConfirmation>>>;
struct InterruptFinishConfirmation {
    inner: towermd_core::hub::Hub,
    kind: String,
    directory: std::path::PathBuf,
    backup: std::path::PathBuf,
    capture: FinishConfirmationCapture,
    release_failed: bool,
}
impl towermd_core::hub_protocol::HubProtocol for InterruptFinishConfirmation {
    fn append(
        &mut self,
        original: towermd_core::hub_protocol::NewEvent,
        now: u64,
    ) -> Result<towermd_core::hub_protocol::AppendReceipt, towermd_core::hub_protocol::HubError>
    {
        use towermd_core::hub_protocol::AppendReceipt;
        let receipt = self.inner.append(original.clone(), now)?;
        if original.kind == self.kind && self.capture.lock().unwrap().is_none() {
            let AppendReceipt::Event(actual) = &receipt else {
                panic!("first actual Task acceptance")
            };
            let path = std::fs::read_dir(&self.directory)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "jsonl")
                })
                .unwrap();
            std::fs::rename(&path, &self.backup).unwrap();
            std::fs::create_dir(&path).unwrap();
            *self.capture.lock().unwrap() = Some(InterruptedFinishConfirmation {
                original,
                actual: actual.as_ref().clone(),
                path,
                backup: self.backup.clone(),
            });
        }
        Ok(receipt)
    }
    fn subscribe(
        &mut self,
        after: u64,
        now: u64,
    ) -> Result<towermd_core::hub_protocol::SubscriptionBatch, towermd_core::hub_protocol::HubError>
    {
        self.inner.subscribe(after, now)
    }
    fn lease_acquire(
        &mut self,
        request: towermd_core::hub_protocol::AcquireLease,
        now: u64,
    ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
    {
        self.inner.lease_acquire(request, now)
    }
    fn lease_renew(
        &mut self,
        request: towermd_core::hub_protocol::RenewLease,
        now: u64,
    ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
    {
        self.inner.lease_renew(request, now)
    }
    fn lease_release(
        &mut self,
        request: towermd_core::hub_protocol::ReleaseLease,
        now: u64,
    ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
    {
        if self.kind == "lease_release" && !self.release_failed {
            self.release_failed = true;
            return Err(towermd_core::hub_protocol::HubError::Io(
                "injected canonical cleanup release failure".into(),
            ));
        }
        self.inner.lease_release(request, now)
    }
    fn snapshot_ack(
        &mut self,
        ack: towermd_core::hub_protocol::SnapshotAck,
        checkpoint: serde_json::Value,
    ) -> Result<(), towermd_core::hub_protocol::HubError> {
        towermd_core::hub_protocol::HubProtocol::snapshot_ack(&mut self.inner, ack, checkpoint)
    }
    fn rebuild(
        &mut self,
        seed: towermd_core::hub_protocol::RebuildSeed,
    ) -> Result<towermd_core::hub_protocol::RebuildReceipt, towermd_core::hub_protocol::HubError>
    {
        self.inner.rebuild(seed)
    }
    fn contiguous_seq(&self) -> u64 {
        self.inner.contiguous_seq()
    }
}
fn interrupt_finish_confirmation(
    mut f: Fixture,
    kind: &str,
    now: u64,
) -> (Fixture, FinishConfirmationCapture) {
    let capture = Arc::new(std::sync::Mutex::new(None));
    // Release this fixture's actual Hub owner before opening its replacement.
    drop(f.session);
    let protocol = InterruptFinishConfirmation {
        inner: towermd_core::hub::Hub::open(&f.catalog.common_dir.join("towermd")).unwrap(),
        kind: kind.into(),
        directory: f.runtime.path().join("outbox"),
        backup: f.runtime.path().join("original-task-outbox-backup"),
        capture: capture.clone(),
        release_failed: false,
    };
    f.session = Arc::new(std::sync::Mutex::new(
        ProjectSession::new(
            Box::new(protocol),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            now,
        )
        .unwrap(),
    ));
    (f, capture)
}
fn restore_interrupted_finish(
    capture: &FinishConfirmationCapture,
) -> InterruptedFinishConfirmation {
    let interrupted = capture.lock().unwrap().as_ref().unwrap().clone();
    std::fs::remove_dir(&interrupted.path).unwrap();
    std::fs::rename(&interrupted.backup, &interrupted.path).unwrap();
    interrupted
}

fn commit_code(f: &Fixture) {
    fs::write(
        f.root.path().join("alpha/result.py"),
        "# actual verified result\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/result.py"]);
    git(f.root.path(), &["commit", "-qm", "actual Task code"]);
}
fn trailers(
    f: &mut Fixture,
    now: u64,
) -> Result<towermd_core::project_session::TaskTrailersOutcome, String> {
    ProjectSession::task_trailers_shared(
        &f.session,
        &f.catalog,
        "host",
        "T-finish",
        None,
        &mut f.outbox,
        || Ok(now),
    )
}
fn observe(
    f: &mut Fixture,
    now: u64,
) -> Result<towermd_core::project_session::MainObservation, String> {
    ProjectSession::observe_task_merges_shared(
        &f.session,
        &f.catalog,
        "host",
        &mut f.outbox,
        || Ok(now),
    )
}
fn amend_and_ff(f: &mut Fixture, footer: &str) -> String {
    git(
        f.root.path(),
        &[
            "commit",
            "--amend",
            "-qm",
            &format!("actual result\n\n{footer}"),
        ],
    );
    let commit = git(f.root.path(), &["rev-parse", "HEAD"]);
    git(f.root.path(), &["update-ref", "refs/heads/main", &commit]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    commit
}
fn projection(f: &Fixture) -> towermd_core::snapshot::StateProjection {
    f.session.lock().unwrap().projection().unwrap().clone()
}
#[test]
fn actual_passed_trailers_are_durable_and_only_unique_main_proof_completes() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let immutable = done.actual.clone();
    let result = trailers(&mut f, NOW + 40).unwrap();
    assert_eq!(result.integration_gate.kind, "evidence.recorded");
    assert_eq!(
        result.integration_gate.data["completed_eid"],
        done.actual.eid
    );
    assert_eq!(result.integration_gate.guards, done.status.input.fences);
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .contains(&result.integration_gate));
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "verifying"
    );
    let repeat = trailers(&mut f, NOW + 41).unwrap();
    assert_eq!(repeat.integration_gate, result.integration_gate);
    assert_eq!(repeat.trailers, result.trailers);
    let commit = amend_and_ff(&mut f, &result.trailers);
    let observed = observe(&mut f, NOW + 42).unwrap();
    assert_eq!(observed.merged.len(), 1, "{:?}", observed.blocked);
    let canonical = &observed.merged[0];
    assert_eq!(
        canonical.eid,
        towermd_core::events::derived_eid(&commit, "merge.done").unwrap()
    );
    assert_eq!(
        canonical.data["proof"]["completed"],
        serde_json::to_value(&immutable).unwrap()
    );
    assert_eq!(
        canonical.data["proof"]["integration_gate"],
        serde_json::to_value(&result.integration_gate).unwrap()
    );
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "merged"
    );
    assert_eq!(projection(&f).objects["Run"][&result.run]["state"], "ended");
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "merge.done")
            .count(),
        1
    );
    assert!(observe(&mut f, NOW + 43).unwrap().merged.is_empty());
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "merge.done")
            .count(),
        1
    );
}
#[test]
fn no_finish_forged_footer_records_one_deterministic_non_human_hold() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let run = projection(&f).objects["Run"]
        .values()
        .next()
        .unwrap()
        .clone();
    assert!(f
        .session
        .lock()
        .unwrap()
        .has_task_merge_candidates()
        .unwrap());
    let fences: Vec<towermd_core::hub_protocol::Fence> =
        serde_json::from_value(run["leases"].clone()).unwrap();
    let epoch = fences
        .iter()
        .find(|f| f.name == "module:A-alpha")
        .unwrap()
        .epoch;
    let footer = format!("Towermd-Task: T-finish\nTowermd-Run: {}\nTowermd-Module: A-alpha\nTowermd-Evidence: E-forged\nTowermd-Epoch: {epoch}\n", run["id"].as_str().unwrap());
    let commit = amend_and_ff(&mut f, &footer);
    let first = observe(&mut f, NOW + 30).unwrap();
    assert!(first.merged.is_empty());
    assert_eq!(first.blocked.len(), 1);
    assert_eq!(first.blocked[0].commit, commit);
    let actual = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|e| e.data.get("main_gate_failure").is_some())
        .unwrap()
        .clone();
    assert_eq!(
        actual.data["main_gate_failure"]["rejection"],
        "missing_started"
    );
    assert_eq!(actual.data["main_gate_failure"]["candidate"], commit);
    assert_eq!(actual.guards, fences);
    assert!(actual.data["evidence"]["baseline"].is_null());
    assert!(actual.data["evidence"]["commands_hash"].is_null());
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "running"
    );
    assert_eq!(projection(&f).objects["Hold"].len(), 1);
    assert!(f.session.lock().unwrap().human().unwrap().holds.is_empty());
    let second = observe(&mut f, NOW + 31).unwrap();
    assert!(second.merged.is_empty());
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.data.get("main_gate_failure").is_some())
            .collect::<Vec<_>>(),
        vec![&actual]
    );
    assert_eq!(projection(&f).objects["Hold"].len(), 1);
    assert!(!projection(&f).objects["Evidence"].contains_key("E-forged"));
    assert_eq!(actual.data["evidence"]["result"], "failed");
}
#[test]
fn terminal_confirmation_fault_retries_the_same_actual_g5_without_reverification() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let (mut f, capture) = interrupt_finish_confirmation(f, "evidence.recorded", NOW + 35);
    let error = trailers(&mut f, NOW + 40).unwrap_err();
    assert!(error.contains("uncertain"), "{error}");
    let interrupted = restore_interrupted_finish(&capture);
    assert_eq!(interrupted.actual.data["completed_eid"], done.actual.eid);
    assert_eq!(interrupted.original.data, interrupted.actual.data);
    assert_eq!(interrupted.original.guards, interrupted.actual.guards);
    let mut conflict = interrupted.original.clone();
    conflict.data["parents"] = serde_json::json!([]);
    assert!(
        f.outbox.append_pending(&conflict).is_err(),
        "same ID cannot replace original terminal parents"
    );
    assert_eq!(interrupted.actual.data["evidence"]["gate"], "g5");
    let result = trailers(&mut f, NOW + 41).unwrap();
    assert_eq!(result.integration_gate, interrupted.actual);
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.confirmed().contains(&interrupted.actual));
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.eid == interrupted.actual.eid)
            .count(),
        1
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.kind == "finish.completed")
            .count(),
        1
    );
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "verifying"
    );
}
#[test]
fn accepted_merge_confirmation_fault_repairs_original_receipt_after_run_has_ended() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let _done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let result = trailers(&mut f, NOW + 40).unwrap();
    let commit = amend_and_ff(&mut f, &result.trailers);
    let (mut f, capture) = interrupt_finish_confirmation(f, "merge.done", NOW + 41);
    let error = observe(&mut f, NOW + 42).unwrap_err();
    assert!(error.contains("uncertain"), "{error}");
    let interrupted = restore_interrupted_finish(&capture);
    assert_eq!(
        interrupted.actual.eid,
        towermd_core::events::derived_eid(&commit, "merge.done").unwrap()
    );
    let retried = observe(&mut f, NOW + 43).unwrap();
    assert_eq!(retried.merged, vec![interrupted.actual.clone()]);
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .contains(&interrupted.actual));
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "merged"
    );
    assert_eq!(projection(&f).objects["Run"][&result.run]["state"], "ended");
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "merge.done")
            .count(),
        1
    );
}
fn review(
    f: &mut Fixture,
    gate: ReviewGate,
    content: Option<String>,
    opened: u64,
) -> towermd_core::request_ledger::TaskReviewRequestOutcome {
    ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::ReviewRequest {
            gate,
            content,
            source_commit: None,
            source_path: None,
        },
        None,
        &mut f.outbox,
        || Ok(opened),
    )
    .unwrap()
}
fn approve(
    f: &mut Fixture,
    open: &towermd_core::request_ledger::TaskReviewRequestOutcome,
    now: u64,
) {
    ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &TaskReviewAction::Review {
            hold: open.receipt.result_ref.hold.clone(),
            binding: open.receipt.result_ref.binding.clone(),
            verdict: ReviewVerdict::Approved,
            comment: "actual core App-authority fixture, not GUI acceptance".into(),
        },
        &mut f.outbox,
        || Ok(now),
    )
    .unwrap();
}
#[test]
fn high_risk_late_g4_is_a_real_independent_gate_and_never_rewrites_g3() {
    let mut f = claimed(&["exit 0"], true);
    commit_code(&f);
    let plan = review(
        &mut f,
        ReviewGate::G1,
        Some("actual immutable Plan\n".into()),
        NOW + 20,
    );
    approve(&mut f, &plan, NOW + 21);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let immutable = done.actual.clone();
    assert_eq!(
        done.evidence.as_ref().unwrap().result,
        EvidenceResult::Passed
    );
    let error = trailers(&mut f, NOW + 35).unwrap_err();
    assert!(error.contains("ReviewBlocked"), "{error}");
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "verifying"
    );
    let g4 = review(&mut f, ReviewGate::G4, None, NOW + 40);
    approve(&mut f, &g4, NOW + 41);
    let result = trailers(&mut f, NOW + 42).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let gate = inputs
        .confirmed()
        .iter()
        .find(|e| e.kind == "evidence.recorded" && e.data["evidence"]["gate"] == "g4")
        .unwrap();
    assert_eq!(
        gate.data["evidence"]["review"]["completed_eid"],
        done.actual.eid
    );
    assert_eq!(
        gate.data["evidence"]["review"]["opening"]["eid"],
        g4.receipt.result_ref.opening_eid
    );
    assert!(done.actual.seq < gate.seq && gate.seq < result.integration_gate.seq);
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .find(|e| e.eid == immutable.eid)
            .unwrap(),
        &immutable
    );
    assert_eq!(
        projection(&f).objects["Evidence"][&done.status.input.evidence_id],
        serde_json::to_value(done.evidence.as_ref().unwrap()).unwrap()
    );
    let _commit = amend_and_ff(&mut f, &result.trailers);
    let observed = observe(&mut f, NOW + 43).unwrap();
    assert_eq!(observed.merged.len(), 1, "{:?}", observed.blocked);
    assert_eq!(
        observed.merged[0].data["proof"]["completed"],
        serde_json::to_value(immutable).unwrap()
    );
    assert_eq!(
        observed.merged[0].data["proof"]["review_contexts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
#[test]
fn no_tests_and_changed_code_or_expired_deadline_never_emit_trailers() {
    for commands in [vec![], vec!["exit 7"]] {
        let mut f = claimed(&commands, false);
        commit_code(&f);
        let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
        assert_ne!(
            done.evidence.as_ref().unwrap().result,
            EvidenceResult::Passed
        );
        assert!(trailers(&mut f, NOW + 40).is_err());
        assert!(!f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .any(|e| e.kind == "evidence.recorded" && e.data["evidence"]["gate"] == "g5"));
    }
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    fs::write(
        f.root.path().join("alpha/result.py"),
        "# changed after sealed verifier\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/result.py"]);
    git(f.root.path(), &["commit", "-qm", "different code"]);
    assert!(trailers(&mut f, NOW + 40).is_err());
    git(
        f.root.path(),
        &["reset", "--hard", &done.status.input.commit],
    );
    assert!(trailers(&mut f, done.status.input.merge_deadline).is_err());
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "verifying"
    );
}
#[test]
fn failed_main_footer_snapshot_reopen_keeps_original_prefix_hold_and_no_human_approval() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let run = projection(&f).objects["Run"]
        .values()
        .next()
        .unwrap()
        .clone();
    let fences: Vec<towermd_core::hub_protocol::Fence> =
        serde_json::from_value(run["leases"].clone()).unwrap();
    let epoch = fences
        .iter()
        .find(|f| f.name == "module:A-alpha")
        .unwrap()
        .epoch;
    let footer = format!("Towermd-Task: T-finish\nTowermd-Run: {}\nTowermd-Module: A-alpha\nTowermd-Evidence: E-forged\nTowermd-Epoch: {epoch}\n", run["id"].as_str().unwrap());
    amend_and_ff(&mut f, &footer);
    observe(&mut f, NOW + 30).unwrap();
    let original = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|e| e.data.get("main_gate_failure").is_some())
        .unwrap()
        .clone();
    let expected = projection(&f);
    let written = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 40),
            &mut f.outbox,
        )
        .unwrap();
    assert_eq!(written.status, "written");
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    drop(f.session);
    f.session = Arc::new(std::sync::Mutex::new(
        ProjectSession::open(
            &f.catalog.common_dir.join("towermd"),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            NOW + 41,
        )
        .unwrap(),
    ));
    assert_eq!(projection(&f).objects["Hold"], expected.objects["Hold"]);
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "running"
    );
    assert!(f.session.lock().unwrap().human().unwrap().holds.is_empty());
    assert!(observe(&mut f, NOW + 42).unwrap().merged.is_empty());
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.data.get("main_gate_failure").is_some())
            .collect::<Vec<_>>(),
        vec![&original]
    );
    assert_eq!(projection(&f).objects["Hold"].len(), 1);
    assert_eq!(
        original.data["main_gate_failure"]["observed_through_seq"],
        expected.objects["Evidence"][original.obj.as_str()]["main_failure"]["observed_through_seq"]
    );
}
#[test]
fn accepted_terminal_receipt_can_be_repaired_after_ttl_without_renewal_or_merge_permission() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let _done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let (mut f, capture) = interrupt_finish_confirmation(f, "evidence.recorded", NOW + 35);
    let expiry = f.session.lock().unwrap().leases(NOW + 36)["module:A-alpha"].expires_at;
    assert!(trailers(&mut f, NOW + 40).is_err());
    let interrupted = restore_interrupted_finish(&capture);
    let before = f.session.lock().unwrap().leases(expiry + 1);
    assert!(!before.contains_key("module:A-alpha"));
    let result = observe(&mut f, expiry + 1).unwrap();
    assert!(result.merged.is_empty());
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .contains(&interrupted.actual));
    assert_eq!(f.session.lock().unwrap().leases(expiry + 1), before);
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "verifying"
    );
    assert!(trailers(&mut f, expiry + 2).is_err());
}
#[test]
fn finite_failure_projection_rejects_changed_guards_and_a_future_prefix_without_partial_hold() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let run = projection(&f).objects["Run"]
        .values()
        .next()
        .unwrap()
        .clone();
    let fences: Vec<towermd_core::hub_protocol::Fence> =
        serde_json::from_value(run["leases"].clone()).unwrap();
    let epoch = fences
        .iter()
        .find(|f| f.name == "module:A-alpha")
        .unwrap()
        .epoch;
    let footer = format!("Towermd-Task: T-finish\nTowermd-Run: {}\nTowermd-Module: A-alpha\nTowermd-Evidence: E-forged\nTowermd-Epoch: {epoch}\n", run["id"].as_str().unwrap());
    amend_and_ff(&mut f, &footer);
    observe(&mut f, NOW + 30).unwrap();
    let actual = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|e| e.data.get("main_gate_failure").is_some())
        .unwrap()
        .clone();
    let expected = projection(&f);
    let mut wrong = actual.clone();
    wrong.guards[0].epoch += 1;
    let mut replay = expected.clone();
    assert!(replay.apply_recorded(&wrong).is_err());
    assert_eq!(replay, expected);
    let mut future = actual.clone();
    future.data["main_gate_failure"]["observed_through_seq"] = serde_json::json!(actual.seq);
    future.data["main_gate_failure"]["evidence"]["main_failure"]["observed_through_seq"] =
        serde_json::json!(actual.seq);
    future.data["evidence"]["main_failure"]["observed_through_seq"] = serde_json::json!(actual.seq);
    assert!(replay.apply_recorded(&future).is_err());
    assert_eq!(replay, expected);
}
#[test]
fn state_snapshot_rebase_reuses_original_g3_but_captures_a_new_terminal_g5() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let original = done.actual.clone();
    let first = trailers(&mut f, NOW + 35).unwrap();
    let written = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 40),
            &mut f.outbox,
        )
        .unwrap();
    assert_eq!(written.status, "written");
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert!(
        trailers(&mut f, NOW + 41).is_err(),
        "actual Task must first include the new authoritative state commit"
    );
    git(f.root.path(), &["rebase", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let second = trailers(&mut f, NOW + 42).unwrap();
    assert_ne!(first.integration_gate.eid, second.integration_gate.eid);
    assert_eq!(second.integration_gate.data["completed_eid"], original.eid);
    assert_eq!(first.evidence, second.evidence);
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "finish.completed")
            .cloned()
            .collect::<Vec<_>>(),
        vec![original.clone()]
    );
    let actual = amend_and_ff(&mut f, &second.trailers);
    let result = observe(&mut f, NOW + 43).unwrap();
    assert_eq!(result.merged.len(), 1, "{:?}", result.blocked);
    assert_eq!(result.merged[0].data["commit"], actual);
    assert_eq!(
        result.merged[0].data["proof"]["completed"],
        serde_json::to_value(original).unwrap()
    );
    assert_eq!(
        result.merged[0].data["proof"]["integration_gate"],
        serde_json::to_value(second.integration_gate).unwrap()
    );
}
#[test]
fn code_main_advance_then_revert_cannot_reuse_old_pass_even_when_tree_matches() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let main_worktree = f.runtime.path().join("real-main-code-worktree");
    git(
        f.root.path(),
        &["worktree", "add", main_worktree.to_str().unwrap(), "main"],
    );
    let old = fs::read(main_worktree.join("alpha/a.py")).unwrap();
    fs::write(
        main_worktree.join("alpha/a.py"),
        "# independent main code\n",
    )
    .unwrap();
    git(&main_worktree, &["add", "alpha/a.py"]);
    git(
        &main_worktree,
        &["commit", "-qm", "actual independent main code"],
    );
    fs::write(main_worktree.join("alpha/a.py"), old).unwrap();
    git(&main_worktree, &["add", "alpha/a.py"]);
    git(
        &main_worktree,
        &["commit", "-qm", "actual main code revert"],
    );
    git(f.root.path(), &["rebase", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_eq!(
        git(f.root.path(), &["rev-parse", "HEAD^{tree}"]),
        done.status.input.tree
    );
    let error = trailers(&mut f, NOW + 40).unwrap_err();
    assert!(error.contains("G5"), "{error}");
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "verifying"
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|e| e.kind == "evidence.recorded" && e.data["evidence"]["gate"] == "g5"));
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|e| e.eid == done.actual.eid)
            .unwrap(),
        &done.actual
    );
}
#[test]
fn two_parent_main_entry_is_the_canonical_commit_even_when_task_head_is_its_side_tip() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let _done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let result = trailers(&mut f, NOW + 40).unwrap();
    git(
        f.root.path(),
        &[
            "commit",
            "--amend",
            "-qm",
            &format!("actual side Task tip\n\n{}", result.trailers),
        ],
    );
    let tip = git(f.root.path(), &["rev-parse", "HEAD"]);
    let main_worktree = f.runtime.path().join("real-main-merge-worktree");
    git(
        f.root.path(),
        &["worktree", "add", main_worktree.to_str().unwrap(), "main"],
    );
    git(
        &main_worktree,
        &[
            "merge",
            "--no-ff",
            "-m",
            &format!("actual main merge\n\n{}", result.trailers),
            "t/T-finish",
        ],
    );
    let entry = git(&main_worktree, &["rev-parse", "HEAD"]);
    assert_ne!(entry, tip);
    assert_eq!(git(f.root.path(), &["rev-parse", "HEAD"]), tip);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let observed = observe(&mut f, NOW + 42).unwrap();
    assert_eq!(observed.merged.len(), 1, "{:?}", observed.blocked);
    assert_eq!(
        observed.merged[0].eid,
        towermd_core::events::derived_eid(&entry, "merge.done").unwrap()
    );
    assert_eq!(observed.merged[0].data["commit"], entry);
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "merged"
    );
    assert_eq!(projection(&f).objects["Run"][&result.run]["state"], "ended");
}
#[test]
fn no_run_observer_is_a_read_only_noop_without_requiring_a_frozen_finish() {
    let mut f = fixture();
    assert!(!f
        .session
        .lock()
        .unwrap()
        .has_task_merge_candidates()
        .unwrap());
    let before = f.outbox.read_inputs().unwrap();
    let seq = f.session.lock().unwrap().last_seq();
    let result = observe(&mut f, NOW + 10).unwrap();
    assert!(result.merged.is_empty());
    assert!(result.blocked.is_empty());
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before.digest());
    assert_eq!(f.session.lock().unwrap().last_seq(), seq);
    assert!(projection(&f)
        .objects
        .get("Run")
        .is_none_or(|runs| runs.is_empty()));
}
#[test]
fn a_snapshot_after_git_merge_cannot_hide_the_unobserved_task_entry() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let result = trailers(&mut f, NOW + 35).unwrap();
    let actual_entry = amend_and_ff(&mut f, &result.trailers);
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "verifying"
    );
    let written = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 40),
            &mut f.outbox,
        )
        .unwrap();
    assert_eq!(written.status, "written");
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_ne!(f.catalog.revision, actual_entry);
    let observed = observe(&mut f, NOW + 42).unwrap();
    assert_eq!(observed.merged.len(), 1, "{:?}", observed.blocked);
    assert_eq!(observed.merged[0].data["commit"], actual_entry);
    assert_eq!(
        observed.merged[0].eid,
        towermd_core::events::derived_eid(&actual_entry, "merge.done").unwrap()
    );
    assert_eq!(
        observed.merged[0].data["proof"]["completed"],
        serde_json::to_value(done.actual).unwrap()
    );
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "merged"
    );
}

#[test]
fn rejected_main_footer_never_blocks_existing_expiry_or_reauthorizes_a_hold() {
    for first_seen_after_expiry in [false, true] {
        let mut f = claimed(&["exit 0"], false);
        commit_code(&f);
        let run = projection(&f).objects["Run"]
            .values()
            .next()
            .unwrap()
            .clone();
        let fences: Vec<towermd_core::hub_protocol::Fence> =
            serde_json::from_value(run["leases"].clone()).unwrap();
        let epoch = fences
            .iter()
            .find(|f| f.name == "module:A-alpha")
            .unwrap()
            .epoch;
        let footer = format!("Towermd-Task: T-finish\nTowermd-Run: {}\nTowermd-Module: A-alpha\nTowermd-Evidence: E-forged\nTowermd-Epoch: {epoch}\n", run["id"].as_str().unwrap());
        amend_and_ff(&mut f, &footer);
        if !first_seen_after_expiry {
            observe(&mut f, NOW + 30).unwrap();
        }
        let before = f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.data.get("main_gate_failure").is_some())
            .cloned()
            .collect::<Vec<_>>();
        let expiry = f.session.lock().unwrap().leases(NOW + 31)["module:A-alpha"].expires_at;
        let leases = f.session.lock().unwrap().leases(expiry + 1);
        let result = observe(&mut f, expiry + 1).unwrap();
        assert!(result.merged.is_empty());
        assert_eq!(result.blocked.len(), 1);
        assert_eq!(f.session.lock().unwrap().leases(expiry + 1), leases);
        assert_eq!(
            f.outbox
                .read_inputs()
                .unwrap()
                .confirmed()
                .iter()
                .filter(|e| e.data.get("main_gate_failure").is_some())
                .cloned()
                .collect::<Vec<_>>(),
            before
        );
        let inputs = f.outbox.read_inputs().unwrap();
        ProjectSession::tick_shared(
            &f.session,
            &f.catalog,
            &inputs,
            "host",
            expiry + 1,
            &mut f.outbox,
        )
        .unwrap();
        assert_eq!(projection(&f).objects["Task"]["T-finish"]["state"], "ready");
        assert_eq!(
            projection(&f).objects["Run"][run["id"].as_str().unwrap()]["state"],
            "released"
        );
        assert_eq!(
            projection(&f)
                .objects
                .get("Hold")
                .map_or(0, |holds| holds.len()),
            usize::from(!first_seen_after_expiry)
        );
        assert!(f.session.lock().unwrap().human().unwrap().holds.is_empty());
    }
}
#[test]
fn actual_failure_outer_metadata_cannot_install_an_automatic_hold() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let run = projection(&f).objects["Run"]
        .values()
        .next()
        .unwrap()
        .clone();
    let fences: Vec<towermd_core::hub_protocol::Fence> =
        serde_json::from_value(run["leases"].clone()).unwrap();
    let epoch = fences
        .iter()
        .find(|f| f.name == "module:A-alpha")
        .unwrap()
        .epoch;
    let footer = format!("Towermd-Task: T-finish\nTowermd-Run: {}\nTowermd-Module: A-alpha\nTowermd-Evidence: E-forged\nTowermd-Epoch: {epoch}\n", run["id"].as_str().unwrap());
    amend_and_ff(&mut f, &footer);
    observe(&mut f, NOW + 30).unwrap();
    let actual = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|e| e.data.get("main_gate_failure").is_some())
        .unwrap()
        .clone();
    let expected = projection(&f);
    for (field, bad) in [
        ("legacy", serde_json::json!(true)),
        ("epoch", serde_json::json!(epoch + 1)),
        ("heartbeat", serde_json::json!(true)),
        ("cost_usd", serde_json::json!(0)),
    ] {
        let mut changed = serde_json::to_value(&actual).unwrap();
        changed[field] = bad;
        let changed: towermd_core::hub_protocol::Event = serde_json::from_value(changed).unwrap();
        let mut replay = expected.clone();
        assert!(replay.apply_recorded(&changed).is_err(), "{field}");
        assert_eq!(replay, expected, "{field}");
    }
}

#[test]
fn frozen_g4_after_state_only_rebase_remains_current_and_approves_only_original_tree() {
    use towermd_core::{
        recovery_contract::task_review::ReviewSubject, request_ledger::ReviewValidity,
    };
    let mut f = claimed(&["exit 0"], true);
    commit_code(&f);
    let plan = review(
        &mut f,
        ReviewGate::G1,
        Some("original approved plan\n".into()),
        NOW + 20,
    );
    approve(&mut f, &plan, NOW + 21);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let immutable = done.actual.clone();
    let written = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 35),
            &mut f.outbox,
        )
        .unwrap();
    assert_eq!(written.status, "written");
    git(f.root.path(), &["rebase", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_ne!(
        git(f.root.path(), &["rev-parse", "HEAD^{tree}"]),
        done.status.input.tree
    );
    let error = trailers(&mut f, NOW + 40).unwrap_err();
    assert!(error.contains("ReviewBlocked"), "{error}");
    let hold = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| {
            event.kind == "hold.opened"
                && event.data["hold"]["body"]["binding"]["subject"]["tree"]
                    == done.status.input.tree
        })
        .unwrap()
        .obj
        .clone();
    let inputs = f.outbox.read_inputs().unwrap();
    let loaded = ProjectSession::load_task_review_shared(
        &f.session,
        &f.catalog,
        &inputs,
        "host",
        &hold,
        || Ok(NOW + 41),
    )
    .unwrap();
    assert_eq!(loaded.current_validity, ReviewValidity::Current);
    assert_eq!(
        loaded.binding.subject,
        ReviewSubject::Tree {
            tree: done.status.input.tree.clone()
        }
    );
    let written = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 42),
            &mut f.outbox,
        )
        .unwrap();
    assert_eq!(written.status, "written");
    git(f.root.path(), &["rebase", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    drop(f.session);
    f.session = Arc::new(std::sync::Mutex::new(
        ProjectSession::open(
            &f.catalog.common_dir.join("towermd"),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            NOW + 43,
        )
        .unwrap(),
    ));
    let inputs = f.outbox.read_inputs().unwrap();
    let reopened = ProjectSession::load_task_review_shared(
        &f.session,
        &f.catalog,
        &inputs,
        "host",
        &hold,
        || Ok(NOW + 44),
    )
    .unwrap();
    assert_eq!(reopened.binding, loaded.binding);
    assert_eq!(reopened.current_validity, ReviewValidity::Current);
    let answer_id = uuid::Uuid::now_v7().to_string();
    let action = TaskReviewAction::Review {
        hold: hold.clone(),
        binding: loaded.binding,
        verdict: ReviewVerdict::Approved,
        comment: "actual frozen G4 core fixture; not GUI acceptance".into(),
    };
    ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 45),
    )
    .unwrap();
    let result = trailers(&mut f, NOW + 46).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.kind == "finish.completed")
            .cloned()
            .collect::<Vec<_>>(),
        vec![immutable.clone()]
    );
    let g4 = inputs
        .confirmed()
        .iter()
        .find(|e| e.kind == "evidence.recorded" && e.data["evidence"]["gate"] == "g4")
        .unwrap();
    assert_eq!(g4.data["evidence"]["tree"], done.status.input.tree);
    assert_eq!(g4.data["evidence"]["review"]["opening"]["obj"], hold);
    amend_and_ff(&mut f, &result.trailers);
    let observed = observe(&mut f, NOW + 47).unwrap();
    assert_eq!(observed.merged.len(), 1, "{:?}", observed.blocked);
    assert_eq!(
        observed.merged[0].data["proof"]["completed"],
        serde_json::to_value(immutable).unwrap()
    );
}
#[test]
fn frozen_g4_changed_code_and_expired_original_deadline_never_approve_old_binding() {
    use towermd_core::request_ledger::ReviewValidity;
    let mut f = claimed(&["exit 0"], true);
    commit_code(&f);
    let plan = review(
        &mut f,
        ReviewGate::G1,
        Some("approved original plan\n".into()),
        NOW + 20,
    );
    approve(&mut f, &plan, NOW + 21);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let error = trailers(&mut f, NOW + 35).unwrap_err();
    assert!(error.contains("ReviewBlocked"), "{error}");
    let hold = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| {
            event.kind == "hold.opened"
                && event.data["hold"]["body"]["binding"]["subject"]["tree"]
                    == done.status.input.tree
        })
        .unwrap()
        .obj
        .clone();
    let inputs = f.outbox.read_inputs().unwrap();
    let binding = ProjectSession::load_task_review_shared(
        &f.session,
        &f.catalog,
        &inputs,
        "host",
        &hold,
        || Ok(NOW + 36),
    )
    .unwrap()
    .binding;
    fs::write(
        f.root.path().join("alpha/result.py"),
        "# changed after original G4 opening\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/result.py"]);
    git(
        f.root.path(),
        &["commit", "-qm", "changed original G4 subject"],
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let view = ProjectSession::load_task_review_shared(
        &f.session,
        &f.catalog,
        &inputs,
        "host",
        &hold,
        || Ok(NOW + 37),
    )
    .unwrap();
    assert!(matches!(
        view.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    let action = TaskReviewAction::Review {
        hold: hold.clone(),
        binding,
        verdict: ReviewVerdict::Approved,
        comment: "must be refused".into(),
    };
    assert!(ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &action,
        &mut f.outbox,
        || Ok(NOW + 38)
    )
    .is_err());
    git(
        f.root.path(),
        &["reset", "--hard", &done.status.input.commit],
    );
    assert!(ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &action,
        &mut f.outbox,
        || Ok(done.status.input.merge_deadline)
    )
    .is_err());
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "hold.reviewed" && event.obj == hold));
}
#[test]
fn canonical_merge_releases_only_original_module_fences_without_task_released() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let output = trailers(&mut f, NOW + 40).unwrap();
    amend_and_ff(&mut f, &output.trailers);
    let result = observe(&mut f, NOW + 41).unwrap();
    assert_eq!(result.merged.len(), 1, "{:?}", result.blocked);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .leases(NOW + 41)
        .contains_key("module:A-alpha"));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .has_task_merge_candidates()
        .unwrap());
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "merged"
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "task.released"));
}
#[test]
fn restart_derives_original_canonical_cleanup_and_never_releases_a_new_epoch() {
    use towermd_core::hub_protocol::{AcquireLease, HubProtocol};
    for replace_epoch in [false, true] {
        let mut f = claimed(&["exit 0"], false);
        commit_code(&f);
        completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
        let output = trailers(&mut f, NOW + 40).unwrap();
        amend_and_ff(&mut f, &output.trailers);
        let expiry = f.session.lock().unwrap().leases(NOW + 40)["module:A-alpha"].expires_at;
        let (mut f, _) = interrupt_finish_confirmation(f, "lease_release", NOW + 41);
        let error = observe(&mut f, NOW + 42).unwrap_err();
        assert!(
            error.contains("injected canonical cleanup release failure"),
            "{error}"
        );
        assert_eq!(projection(&f).objects["Run"][&output.run]["state"], "ended");
        assert!(f
            .session
            .lock()
            .unwrap()
            .has_task_merge_candidates()
            .unwrap());
        let canonical = f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "merge.done")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(canonical.len(), 1);
        drop(f.session);
        let now = if replace_epoch { expiry + 1 } else { NOW + 43 };
        let replacement = if replace_epoch {
            let mut hub =
                towermd_core::hub::Hub::open(&f.catalog.common_dir.join("towermd")).unwrap();
            Some(
                hub.lease_acquire(
                    AcquireLease {
                        host: "host".into(),
                        names: vec!["module:A-alpha".into()],
                        holder: uuid::Uuid::now_v7().to_string(),
                        ttl_ms: expiry - (NOW + 40),
                    },
                    now,
                )
                .unwrap()
                .leases[0]
                    .clone(),
            )
        } else {
            None
        };
        f.session = Arc::new(std::sync::Mutex::new(
            ProjectSession::open(
                &f.catalog.common_dir.join("towermd"),
                &f.catalog,
                &f.outbox.read_inputs().unwrap(),
                now,
            )
            .unwrap(),
        ));
        assert!(observe(&mut f, now).unwrap().merged.is_empty());
        let leases = f.session.lock().unwrap().leases(now);
        if let Some(replacement) = replacement {
            assert_eq!(leases.get("module:A-alpha"), Some(&replacement));
        } else {
            assert!(!leases.contains_key("module:A-alpha"));
        }
        assert_eq!(
            f.outbox
                .read_inputs()
                .unwrap()
                .confirmed()
                .iter()
                .filter(|event| event.kind == "merge.done")
                .cloned()
                .collect::<Vec<_>>(),
            canonical
        );
        assert!(!f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .any(|event| event.kind == "task.released"));
    }
}

#[test]
fn active_running_finish_without_any_main_run_id_has_a_read_only_observer_noop() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let id = uuid::Uuid::now_v7().to_string();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 30),
    )
    .unwrap() else {
        panic!("new actual queued finish");
    };
    let _execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    let expected = projection(&f);
    let before = f.outbox.read_inputs().unwrap().digest().to_owned();
    let cursor = f.session.lock().unwrap().last_seq();
    let observed = observe(&mut f, NOW + 32).unwrap();
    assert!(observed.merged.is_empty() && observed.blocked.is_empty());
    assert_eq!(projection(&f), expected);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before);
    assert_eq!(f.session.lock().unwrap().last_seq(), cursor);
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "verifying"
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "finish.completed" || event.kind == "merge.done"));
}
#[test]
fn fixed_run_scan_keeps_no_space_footer_and_body_matches_on_the_full_proof_path() {
    for actual_footer in [false, true] {
        let mut f = claimed(&["exit 0"], false);
        commit_code(&f);
        let run = projection(&f).objects["Run"]
            .values()
            .next()
            .unwrap()
            .clone();
        let id = run["id"].as_str().unwrap();
        let fences: Vec<towermd_core::hub_protocol::Fence> =
            serde_json::from_value(run["leases"].clone()).unwrap();
        let epoch = fences
            .iter()
            .find(|f| f.name == "module:A-alpha")
            .unwrap()
            .epoch;
        let footer = if actual_footer {
            format!("Towermd-Task:T-finish\nTowermd-Run:\t{id}\nTowermd-Module:A-alpha\nTowermd-Evidence:E-forged\nTowermd-Epoch:{epoch}\n")
        } else {
            format!("ordinary body mentions the actual Run {id}; no Task footer")
        };
        if actual_footer {
            assert!(matches!(
                towermd_core::recovery_git::parse_trailer_context(&footer).unwrap(),
                towermd_core::recovery_git::TrailerContext::Task(_)
            ));
        }
        amend_and_ff(&mut f, &footer);
        let before = f.outbox.read_inputs().unwrap().digest().to_owned();
        let observed = observe(&mut f, NOW + 30).unwrap();
        assert!(observed.merged.is_empty());
        if actual_footer {
            assert_eq!(observed.blocked.len(), 1);
            let inputs = f.outbox.read_inputs().unwrap();
            let failure = inputs
                .confirmed()
                .iter()
                .find(|event| event.data.get("main_gate_failure").is_some())
                .unwrap();
            assert_eq!(
                failure.data["main_gate_failure"]["rejection"],
                "missing_started"
            );
            assert_eq!(failure.guards, fences);
            assert_eq!(projection(&f).objects["Hold"].len(), 1);
        } else {
            assert!(observed.blocked.is_empty());
            assert_eq!(f.outbox.read_inputs().unwrap().digest(), before);
            assert!(!projection(&f).objects.contains_key("Hold"));
        }
        assert_eq!(
            projection(&f).objects["Task"]["T-finish"]["state"],
            "running"
        );
        assert!(f.session.lock().unwrap().human().unwrap().holds.is_empty());
    }
}
#[test]
fn empty_run_scan_cannot_absorb_a_new_main_footer_during_final_clock() {
    use std::cell::Cell;
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let run = projection(&f).objects["Run"]
        .values()
        .next()
        .unwrap()
        .clone();
    let fences: Vec<towermd_core::hub_protocol::Fence> =
        serde_json::from_value(run["leases"].clone()).unwrap();
    let epoch = fences
        .iter()
        .find(|f| f.name == "module:A-alpha")
        .unwrap()
        .epoch;
    let footer = format!("Towermd-Task:T-finish\nTowermd-Run:{}\nTowermd-Module:A-alpha\nTowermd-Evidence:E-forged\nTowermd-Epoch:{epoch}\n", run["id"].as_str().unwrap());
    git(
        f.root.path(),
        &[
            "commit",
            "--amend",
            "-qm",
            &format!("actual candidate not yet on main\n\n{footer}"),
        ],
    );
    let candidate = git(f.root.path(), &["rev-parse", "HEAD"]);
    let root = f.root.path().to_path_buf();
    let calls = Cell::new(0);
    let before = f.outbox.read_inputs().unwrap().digest().to_owned();
    let result = ProjectSession::observe_task_merges_shared(
        &f.session,
        &f.catalog,
        "host",
        &mut f.outbox,
        || {
            calls.set(calls.get() + 1);
            // Initial clock, actual active projection, cleanup's negative guard,
            // then the final NoOp clock after the one full-main grep.
            if calls.get() == 4 {
                git(&root, &["update-ref", "refs/heads/main", &candidate]);
            }
            Ok(NOW + 30)
        },
    );
    assert_eq!(calls.get(), 4);
    let error = result.unwrap_err();
    assert!(error.contains("authority changed"), "{error}");
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before);
    assert!(!projection(&f).objects.contains_key("Hold"));
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let observed = observe(&mut f, NOW + 31).unwrap();
    assert!(observed.merged.is_empty());
    assert_eq!(observed.blocked.len(), 1);
    assert_eq!(projection(&f).objects["Hold"].len(), 1);
}

#[path = "task_integration/maintenance.rs"]
mod maintenance;
