//! Real B0 publication, claims, Git guards and sealed verifier. No client-built
//! FrozenFinish, Run or VerificationReport is used to establish completion.
#[path = "support/finish_ask_recovery.rs"]
mod ask_recovery;
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
    finish::{self, EvidenceResult, FailureAction, FinishStart, JobPhase},
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
fn run(f: &Fixture, id: &str) -> serde_json::Value {
    f.session.lock().unwrap().projection().unwrap().objects["Run"][id].clone()
}
#[test]
fn third_failure_is_hold_and_deadline_has_one_origin() {
    assert_eq!(finish::failure_action(0), (1, FailureAction::Running));
    assert_eq!(finish::failure_action(1), (2, FailureAction::Running));
    assert_eq!(
        finish::failure_action(2),
        (3, FailureAction::GatefailedHold)
    );
    assert_eq!(finish::fixed_deadline(123).unwrap(), 3_600_123);
    assert!(finish::fixed_deadline(u64::MAX).is_err());
}
#[test]
fn actual_sealed_pass_remains_verifying_with_exact_report_and_complete_receipt() {
    let mut f = claimed(&["test -f alpha/a.py"], false);
    let id = uuid::Uuid::now_v7().to_string();
    let result = completed(&mut f, &id, NOW + 30);
    let evidence = result.evidence.unwrap();
    assert_eq!(evidence.result, EvidenceResult::Passed);
    assert_eq!(result.status.phase, JobPhase::Completed);
    assert_eq!(evidence.log.report_eid, result.actual.eid);
    assert_eq!(result.actual.data["report"]["commit"], evidence.commit);
    assert_eq!(
        result.actual.data["report"]["executions"][0]["command"]["argv"],
        serde_json::json!(["/bin/sh", "-c", "test -f alpha/a.py"])
    );
    assert_eq!(run(&f, &evidence.run)["state"], "verifying");
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Task"]["T-finish"]["state"],
        "verifying"
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|e| e.eid == result.actual.eid),
        Some(&result.actual)
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .iter()
        .any(|e| e.eid == result.actual.eid));
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let retried = ProjectSession::retry_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &mut f.outbox,
        || Ok(NOW + 40),
    )
    .unwrap()
    .unwrap();
    assert_eq!(retried, result.status);
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq(),
        before
    );
    let mut projection = f.session.lock().unwrap().projection().unwrap().clone();
    let before = projection.clone();
    let mut conflict = result.actual.clone();
    conflict.data["evidence"]["result"] = serde_json::json!("failed");
    assert!(projection.apply_recorded(&conflict).is_err());
    assert_eq!(projection, before);
}
#[test]
fn three_distinct_sealed_failures_create_one_hold_replay_is_idempotent_and_reopen_preserves_deadline(
) {
    let mut f = claimed(&["exit 7", "exit 0"], false);
    let mut outcomes = vec![];
    for n in 0..3 {
        let id = uuid::Uuid::now_v7().to_string();
        let result = completed(&mut f, &id, NOW + 30 + n * 10);
        assert_eq!(
            result.evidence.as_ref().unwrap().result,
            EvidenceResult::Failed
        );
        assert_eq!(
            result.actual.data["report"]["executions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        outcomes.push(result);
    }
    let run_id = outcomes[0].status.input.run_id.clone();
    let final_run = run(&f, &run_id);
    assert_eq!(final_run["state"], "held");
    assert_eq!(final_run["finish"]["failures"], 3);
    assert_eq!(
        final_run["finish"]["consumed_failures"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    for result in &outcomes {
        assert_eq!(
            result.status.input.entered_verifying_at,
            outcomes[0].status.input.entered_verifying_at
        );
        assert_eq!(
            result.status.input.merge_deadline,
            outcomes[0].status.input.merge_deadline
        );
    }
    let last = outcomes.last().unwrap();
    let expected = f.session.lock().unwrap().projection().unwrap().clone();
    let mut replay = expected.clone();
    replay.apply_recorded(&last.actual).unwrap();
    assert_eq!(replay, expected);
    assert_eq!(expected.objects["Hold"].len(), 1);
    let deadline = last.status.input.merge_deadline;
    let written = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 80),
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
            NOW + 81,
        )
        .unwrap(),
    ));
    assert_eq!(run(&f, &run_id)["finish"]["merge_deadline"], deadline);
    assert_eq!(run(&f, &run_id)["finish"]["failures"], 3);
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Hold"].len(),
        1
    );
    assert!(ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        None,
        &mut f.outbox,
        || Ok(NOW + 82)
    )
    .is_err());
}
#[test]
fn authoritative_no_tests_skips_executor_and_three_distinct_results_hold() {
    let mut f = claimed(&[], false);
    fs::write(f.root.path().join("alpha/run.sh"), "exit 0\n").unwrap();
    git(f.root.path(), &["add", "alpha/run.sh"]);
    git(
        f.root.path(),
        &[
            "commit",
            "-qm",
            "branch script does not grant authoritative verification",
        ],
    );
    let before = f.catalog.common_dir.join("towermd/cache/verification");
    let count = fs::read_dir(&before).unwrap().count();
    for n in 0..3 {
        let result = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30 + n * 10);
        assert_eq!(result.evidence.unwrap().result, EvidenceResult::NoTests);
        assert!(result.actual.data["report"]["executions"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(result.actual.data["evidence"]["gates"]["G3"], false);
    }
    assert_eq!(fs::read_dir(&before).unwrap().count(), count);
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Task"]["T-finish"]["state"],
        "held"
    );
}
#[test]
fn actual_worker_interruption_is_durable_without_failed_evidence_or_reset_deadline() {
    let mut f = claimed(&["exit 0"], false);
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
        panic!()
    };
    let first = ticket.status().clone();
    let FinishStart::Queued(duplicate) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 30),
    )
    .unwrap() else {
        panic!("same queued UUID must retain exact input")
    };
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    drop(execution);
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    assert!(ProjectSession::launch_finish_shared(
        &f.session,
        &f.catalog,
        duplicate,
        &mut f.outbox,
        || Ok(NOW + 31)
    )
    .is_err());
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq(),
        before
    );
    let interrupted = ProjectSession::interrupt_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        "worker owner stopped before execution",
        &mut f.outbox,
        || Ok(NOW + 32),
    )
    .unwrap();
    assert_eq!(interrupted.status.phase, JobPhase::Interrupted);
    assert!(interrupted.evidence.is_none());
    assert_eq!(run(&f, &first.input.run_id)["finish"]["failures"], 0);
    let next = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 40);
    assert_eq!(next.status.input.merge_deadline, first.input.merge_deadline);
    assert_eq!(
        ProjectSession::retry_finish_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &mut f.outbox,
            || Ok(NOW + 45)
        )
        .unwrap()
        .unwrap()
        .phase,
        JobPhase::Interrupted
    );
}
fn request_plan(
    f: &mut Fixture,
    text: &str,
) -> towermd_core::request_ledger::TaskReviewRequestOutcome {
    ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::ReviewRequest {
            gate: ReviewGate::G1,
            content: Some(text.into()),
            source_commit: None,
            source_path: None,
        },
        None,
        &mut f.outbox,
        || Ok(NOW + 20),
    )
    .unwrap()
}
fn approve_plan(f: &mut Fixture, open: &towermd_core::request_ledger::TaskReviewRequestOutcome) {
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
            comment: "approved exact actual Plan".into(),
        },
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
}
#[test]
fn high_risk_uses_current_real_plan_approval_and_g4_remains_a_separate_hard_gate() {
    let mut f = claimed(&["exit 0"], true);
    assert!(ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        None,
        &mut f.outbox,
        || Ok(NOW + 25)
    )
    .is_err());
    let first = request_plan(&mut f, "Plan A\n");
    approve_plan(&mut f, &first);
    let completed = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    assert_eq!(
        completed.evidence.as_ref().unwrap().result,
        EvidenceResult::Passed
    );
    assert!(completed.actual.data["review_context"].is_object());
    assert_eq!(
        completed.actual.data["review_context"]["opening"]["eid"],
        first.receipt.result_ref.opening_eid
    );
    assert_eq!(
        run(&f, &completed.status.input.run_id)["state"],
        "verifying"
    );
    assert!(ProjectSession::require_finish_review_shared(
        &f.session,
        &f.catalog,
        &completed.status.input,
        ReviewGate::G4,
        &f.outbox.read_inputs().unwrap(),
        NOW + 33
    )
    .is_err());
    let _second = request_plan(&mut f, "Plan B is different and unapproved\n");
    let refused = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        None,
        &mut f.outbox,
        || Ok(NOW + 35),
    );
    assert!(refused.is_err());
    assert!(ProjectSession::require_finish_review_shared(
        &f.session,
        &f.catalog,
        &completed.status.input,
        ReviewGate::G1,
        &f.outbox.read_inputs().unwrap(),
        NOW + 36
    )
    .is_err());
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Evidence"]
            [&completed.status.input.evidence_id]["result"],
        "passed"
    );
}
#[test]
fn dirty_input_and_foreign_outbox_are_rejected_before_any_finish_intent() {
    let mut f = claimed(&["exit 0"], false);
    fs::write(f.root.path().join("alpha/a.py"), "dirty\n").unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    assert!(ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 30)
    )
    .is_err());
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .iter()
        .any(|e| e.kind.starts_with("finish.")));
    let mut foreign = claimed(&[], false);
    let cursor = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    assert!(ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut foreign.outbox,
        || Ok(NOW + 31)
    )
    .is_err());
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq(),
        cursor
    );
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

#[test]
fn accepted_completion_confirmation_failure_compacts_and_reopens_without_reexecuting_or_changing_original(
) {
    let (mut f, capture) =
        interrupt_finish_confirmation(claimed(&["exit 0"], false), "finish.completed", NOW + 12);
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
        panic!()
    };
    let input = ticket.status().input.clone();
    let launched =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    let executed = finish::execute(
        launched,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let error = ProjectSession::record_finish_shared(
        &f.session,
        &f.catalog,
        executed,
        &mut f.outbox,
        || Ok(NOW + 32),
    )
    .unwrap_err();
    assert!(
        error.contains("uncertain") && error.contains(&id),
        "{error}"
    );
    let original = restore_interrupted_finish(&capture);
    assert_eq!(original.original.eid, original.actual.eid);
    assert_eq!(original.original.guards, input.fences);
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .contains(&original.original));
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .contains(&original.actual));
    ProjectSession::sync_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        NOW + 33,
    )
    .unwrap();
    assert_eq!(
        run(&f, &input.run_id)["finish"]["jobs"][&input.evidence_id]["phase"],
        "recording"
    );
    let log_root = f.catalog.common_dir.join("towermd/cache/verification");
    let executed_count = fs::read_dir(&log_root).unwrap().count();
    let written = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 34),
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
            NOW + 35,
        )
        .unwrap(),
    ));
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let result = ProjectSession::retry_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &mut f.outbox,
        || Ok(NOW + 36),
    )
    .unwrap()
    .unwrap();
    assert_eq!(result.phase, JobPhase::Completed);
    assert_eq!(result.input, input);
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq(),
        before
    );
    assert_eq!(fs::read_dir(&log_root).unwrap().count(), executed_count);
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|e| e.eid == original.actual.eid),
        Some(&original.actual)
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .iter()
        .any(|e| e.eid == original.original.eid));
    assert_eq!(run(&f, &input.run_id)["state"], "verifying");
}
#[test]
fn raw_pending_started_and_projection_cannot_cross_the_third_failure_hold() {
    use towermd_core::{hub_protocol::NewEvent, recovery_contract::recorded::request_event_eid};
    let mut f = claimed(&["exit 9"], false);
    let mut results = vec![];
    for n in 0..3 {
        results.push(completed(
            &mut f,
            &uuid::Uuid::now_v7().to_string(),
            NOW + 30 + n * 10,
        ));
    }
    let last = results.last().unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let evidence_id = format!("E-{id}");
    let mut job = last.status.clone();
    job.request_id = id.clone();
    job.input.evidence_id = evidence_id.clone();
    job.phase = JobPhase::Queued;
    job.started_eid = request_event_eid(&id, "finish.started").unwrap();
    job.completed_eid = request_event_eid(&id, "finish.completed").unwrap();
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    let mut task = projection.objects["Task"]["T-finish"].clone();
    task["state"] = serde_json::json!("verifying");
    let mut forged_run = projection.objects["Run"][&job.input.run_id].clone();
    forged_run["state"] = serde_json::json!("verifying");
    forged_run["finish"]["jobs"][&evidence_id] = serde_json::json!(job);
    let original = NewEvent {
        eid: job.started_eid.clone(),
        host: "host".into(),
        actor: "process:towermd-verifier".into(),
        kind: "finish.started".into(),
        layer: "L4".into(),
        obj: "T-finish".into(),
        data: serde_json::json!({"task":task,"run":forged_run,"input":job.input,"review_context":null}),
        guards: job.input.fences.clone(),
    };
    f.outbox.append_pending(&original).unwrap();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let error = ProjectSession::retry_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &mut f.outbox,
        || Ok(NOW + 70),
    )
    .unwrap_err();
    assert!(error.contains("Hold"), "{error}");
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq(),
        before
    );
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .contains(&original));
    assert_eq!(f.session.lock().unwrap().projection().unwrap(), &projection);
    // Pure negative only: a deliberately forged copy of an actual envelope is
    // never appended and cannot mutate the replay projection either.
    let mut forged = last.actual.clone();
    forged.kind = original.kind;
    forged.eid = original.eid;
    forged.data = original.data;
    let mut copy = projection.clone();
    assert!(copy.apply_recorded(&forged).unwrap_err().contains("Hold"));
    assert_eq!(copy, projection);
}
#[test]
fn claim_confirmation_is_repaired_before_freezing_the_actual_original_run() {
    let (mut f, capture) =
        interrupt_finish_confirmation(ready(&["exit 0"], false), "task.claimed", NOW + 10);
    let error = claim(&mut f, NOW + 11).unwrap_err();
    let original = restore_interrupted_finish(&capture);
    assert!(
        error.message.contains("claim 结果不确定，重开后协调"),
        "{error}"
    );
    assert!(error.message.contains(&original.original.eid), "{error}");
    assert!(
        error
            .message
            .contains(original.actual.data["run"]["id"].as_str().unwrap()),
        "{error}"
    );
    assert!(
        error.message.contains("outbox path is not a regular file"),
        "{error}"
    );
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .contains(&original.original));
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
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
        panic!()
    };
    assert_eq!(
        ticket.status().input.run_id,
        original.actual.data["run"]["id"]
    );
    assert_eq!(ticket.status().input.fences, original.original.guards);
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|e| e.eid == original.actual.eid),
        Some(&original.actual)
    );
    let events = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before, NOW + 31)
        .unwrap()
        .events;
    assert!(!events.iter().any(|e| e.kind == "lease.granted"));
    assert_eq!(
        events.iter().filter(|e| e.kind == "finish.started").count(),
        1
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .iter()
        .any(|e| e.eid == original.original.eid));
}
#[test]
fn started_confirmation_retries_the_same_frozen_ticket_without_another_start_or_grant() {
    let (mut f, capture) =
        interrupt_finish_confirmation(claimed(&["exit 0"], false), "finish.started", NOW + 12);
    let id = uuid::Uuid::now_v7().to_string();
    let error = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 30),
    )
    .unwrap_err();
    assert!(
        error.contains("uncertain") && error.contains(&id),
        "{error}"
    );
    let original = restore_interrupted_finish(&capture);
    let expected: towermd_core::finish::FrozenFinish =
        serde_json::from_value(original.actual.data["input"].clone()).unwrap();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 31),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(ticket.status().input, expected);
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq(),
        before
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|e| e.eid == original.actual.eid),
        Some(&original.actual)
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "finish.started" && e.eid == original.actual.eid)
            .count(),
        1
    );
    let executing =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 32)
        })
        .unwrap();
    let result = ProjectSession::record_finish_shared(
        &f.session,
        &f.catalog,
        finish::execute(
            executing,
            Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        ),
        &mut f.outbox,
        || Ok(NOW + 33),
    )
    .unwrap();
    assert_eq!(result.evidence.unwrap().result, EvidenceResult::Passed);
    assert_eq!(result.status.input, expected);
}
#[cfg(unix)]
#[test]
fn actual_verifier_infrastructure_error_records_interrupted_without_failure_evidence() {
    let mut f = claimed(&["exit 0"], false);
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
        panic!()
    };
    let input = ticket.status().input.clone();
    let executing =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    // The exact owned verifier directory is empty after the fixture's real scan.
    // A symlink fault must stop the real executor before any command/log result.
    let verify = f.catalog.common_dir.join("towermd/verify");
    fs::remove_dir(&verify).unwrap();
    let foreign = f.runtime.path().join("foreign-verifier-root");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("keep"), b"owned elsewhere").unwrap();
    std::os::unix::fs::symlink(&foreign, &verify).unwrap();
    let log_root = f.catalog.common_dir.join("towermd/cache/verification");
    let before = fs::read_dir(&log_root).unwrap().count();
    let executed = finish::execute(
        executing,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let result = ProjectSession::record_finish_shared(
        &f.session,
        &f.catalog,
        executed,
        &mut f.outbox,
        || Ok(NOW + 32),
    )
    .unwrap();
    assert_eq!(result.status.phase, JobPhase::Interrupted);
    assert!(result.evidence.is_none());
    assert!(result
        .status
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("symlink"));
    assert_eq!(run(&f, &input.run_id)["state"], "running");
    assert_eq!(run(&f, &input.run_id)["finish"]["failures"], 0);
    assert!(
        !f.session.lock().unwrap().projection().unwrap().objects["Evidence"]
            .contains_key(&input.evidence_id)
    );
    assert_eq!(fs::read_dir(&log_root).unwrap().count(), before);
    assert_eq!(fs::read(foreign.join("keep")).unwrap(), b"owned elsewhere");
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|e| e.eid == result.actual.eid),
        Some(&result.actual)
    );
    fs::remove_file(&verify).unwrap();
    fs::create_dir(&verify).unwrap();
    let next = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 40);
    assert_eq!(next.status.input.merge_deadline, input.merge_deadline);
}
#[test]
fn amended_task_commit_new_evidence_keeps_the_first_verifying_deadline() {
    let mut f = claimed(&["exit 4"], false);
    fs::write(f.root.path().join("alpha/a.py"), "# task commit one\n").unwrap();
    git(f.root.path(), &["add", "alpha/a.py"]);
    git(
        f.root.path(),
        &["commit", "-qm", "first Task implementation"],
    );
    let first = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    fs::write(
        f.root.path().join("alpha/a.py"),
        "# amended Task implementation\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/a.py"]);
    git(f.root.path(), &["commit", "--amend", "--no-edit"]);
    let second = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 50);
    assert_ne!(first.status.input.commit, second.status.input.commit);
    assert_ne!(first.status.input.tree, second.status.input.tree);
    assert_ne!(
        first.status.input.evidence_id,
        second.status.input.evidence_id
    );
    assert_eq!(first.status.input.run_id, second.status.input.run_id);
    assert_eq!(
        first.status.input.entered_verifying_at,
        second.status.input.entered_verifying_at
    );
    assert_eq!(
        first.status.input.merge_deadline,
        second.status.input.merge_deadline
    );
    assert_eq!(
        second.status.input.merge_deadline,
        first.status.input.entered_verifying_at + 3_600_000
    );
    assert_eq!(
        run(&f, &second.status.input.run_id)["finish"]["failures"],
        2
    );
}
#[test]
fn final_launch_cas_rejects_an_actual_run_change_that_preserves_job_and_fences() {
    use std::{cell::Cell, sync::Mutex};
    use towermd_core::hub_protocol::{AppendReceipt, NewEvent};
    let mut f = claimed(&["exit 0"], false);
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
        panic!()
    };
    let input = ticket.status().input.clone();
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    let task = projection.objects["Task"]["T-finish"].clone();
    let mut changed = projection.objects["Run"][&input.run_id].clone();
    changed["remote-observation"] = serde_json::json!({"value":"must not be overwritten"});
    let live = f.session.clone();
    let calls = Cell::new(0);
    let accepted = Mutex::new(None);
    let original = NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:towermd-cli".into(),
        kind: "task.state".into(),
        layer: "L5".into(),
        obj: "T-finish".into(),
        data: serde_json::json!({"task":task,"run":changed}),
        guards: input.fences.clone(),
    };
    // A second real Hub writer may advance business state after Git checking.
    // This is an actual durable Hub event; it is not an injected Review fact.
    let error =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 3 {
                let AppendReceipt::Event(event) = live
                    .lock()
                    .unwrap()
                    .protocol()
                    .unwrap()
                    .append(original.clone(), NOW + 31)
                    .unwrap()
                else {
                    panic!()
                };
                *accepted.lock().unwrap() = Some(*event);
            }
            Ok(NOW + 31)
        })
        .unwrap_err();
    assert_eq!(calls.get(), 3);
    assert!(error.contains("CAS changed"), "{error}");
    let accepted = accepted.lock().unwrap().clone().unwrap();
    assert_eq!(
        accepted.data["run"]["finish"]["jobs"][&input.evidence_id]["phase"],
        "queued"
    );
    assert_eq!(
        run(&f, &input.run_id)["remote-observation"],
        serde_json::json!({"value":"must not be overwritten"})
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .iter()
        .any(|e| e.kind == "finish.running"));
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|e| e.kind == "finish.running"));
    assert!(
        !f.session.lock().unwrap().projection().unwrap().objects["Evidence"]
            .contains_key(&input.evidence_id)
    );
}
