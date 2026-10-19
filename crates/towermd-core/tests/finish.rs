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

const SOURCE_GATE_COMMAND:&str="gate=\"$(git rev-parse --git-common-dir)/towermd/cache/finish-source-gate\"; mkdir -p \"$gate\"; : > \"$gate/entered\"; while [ ! -f \"$gate/release\" ]; do sleep 0.01; done; exit 0";
struct SourceGate {
    path: std::path::PathBuf,
}
impl Drop for SourceGate {
    fn drop(&mut self) {
        let _ = fs::write(self.path.join("release"), "");
    }
}
fn execute_at_source_barrier(
    f: &mut Fixture,
) -> (
    finish::FinishStatus,
    SourceGate,
    std::thread::JoinHandle<finish::ExecutedFinish>,
) {
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
    let status = ticket.status().clone();
    let ticket =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    let gate = SourceGate {
        path: f
            .catalog
            .common_dir
            .join("towermd/cache/finish-source-gate"),
    };
    let worker = std::thread::spawn(move || {
        finish::execute(
            ticket,
            Duration::from_secs(30),
            Arc::new(AtomicBool::new(false)),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !gate.path.join("entered").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        gate.path.join("entered").exists(),
        "actual sealed verifier never reached its owned execution barrier"
    );
    assert!(!worker.is_finished());
    (status, gate, worker)
}
#[test]
fn actual_verifier_accepts_only_state_snapshot_main_advance_without_rewriting_frozen_evidence() {
    let mut f = claimed(&[SOURCE_GATE_COMMAND], false);
    let (status, gate, worker) = execute_at_source_barrier(&mut f);
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 32),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    assert!(
        !worker.is_finished(),
        "main must advance during real sealed execution"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_ne!(f.catalog.revision, status.input.authority);
    let ordinary = towermd_core::task_gates::check_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        None,
        || Ok(NOW + 33),
    );
    assert!(
        matches!(
            ordinary,
            Err(towermd_core::task_gates::GateError::NotLatestMain)
        ),
        "ordinary new checks still require latest main: {ordinary:?}"
    );
    fs::write(gate.path.join("release"), "").unwrap();
    let executed = worker.join().unwrap();
    let done = ProjectSession::record_finish_shared(
        &f.session,
        &f.catalog,
        executed,
        &mut f.outbox,
        || Ok(NOW + 34),
    )
    .unwrap();
    assert_eq!(done.status.input, status.input);
    let evidence = done.evidence.unwrap();
    assert_eq!(evidence.result, EvidenceResult::Passed);
    assert_eq!(evidence.authority, status.input.authority);
    assert_eq!(evidence.tree, status.input.tree);
    assert_eq!(done.actual.data["report"]["tree"], status.input.tree);
    assert_eq!(
        evidence.gates,
        status
            .checked_gates
            .into_iter()
            .chain([("G3".into(), true)])
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    assert_eq!(run(&f, &status.input.run_id)["state"], "verifying");
}
#[test]
fn frozen_no_tests_uses_same_current_main_state_proof_and_keeps_original_input() {
    let mut f = claimed(&[], false);
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
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 32),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_ne!(f.catalog.revision, input.authority);
    let executed = finish::execute(
        execution,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let done = ProjectSession::record_finish_shared(
        &f.session,
        &f.catalog,
        executed,
        &mut f.outbox,
        || Ok(NOW + 33),
    )
    .unwrap();
    assert_eq!(done.status.input, input);
    assert_eq!(done.evidence.unwrap().result, EvidenceResult::NoTests);
    assert!(done.actual.data["report"]["executions"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn frozen_verification_rejects_actual_code_config_slow_rollback_and_reverted_merge_side_history() {
    for fault in ["code", "config", "slow", "rollback", "merge-side"] {
        let mut f = claimed(&[SOURCE_GATE_COMMAND], false);
        let (status, gate, worker) = execute_at_source_barrier(&mut f);
        let main = f.runtime.path().join("actual-main");
        git(
            f.root.path(),
            &["worktree", "add", main.to_str().unwrap(), "main"],
        );
        match fault {
            "code" => {
                fs::write(main.join("alpha/a.py"), "# unrelated actual main code\n").unwrap();
                git(&main, &["add", "alpha/a.py"]);
                git(&main, &["commit", "-qm", "advance code, not state"]);
            }
            "config" => {
                let path = main.join(".towermd/config.yaml");
                let mut text = fs::read_to_string(&path).unwrap();
                text.push_str("\nsnapshot_interval: 91\n");
                fs::write(&path, text).unwrap();
                git(&main, &["add", ".towermd/config.yaml"]);
                git(&main, &["commit", "-qm", "advance actual policy"]);
            }
            "slow" => {
                let path = &f.catalog.object_paths["A-alpha"];
                let mut text = fs::read_to_string(main.join(path)).unwrap();
                text.push_str("\nChanged main architecture.\n");
                fs::write(main.join(path), text).unwrap();
                git(&main, &["add", path]);
                git(&main, &["commit", "-qm", "advance actual slow Module"]);
            }
            "rollback" => {
                let parent = git(&main, &["rev-parse", "HEAD^"]);
                git(&main, &["reset", "--hard", &parent]);
            }
            "merge-side" => {
                git(&main, &["switch", "-qc", "forbidden-side"]);
                fs::write(main.join("alpha/a.py"), "# forbidden side code\n").unwrap();
                git(&main, &["add", "alpha/a.py"]);
                git(
                    &main,
                    &["commit", "-qm", "side code must remain visible to G5"],
                );
                git(&main, &["revert", "--no-edit", "HEAD"]);
                git(&main, &["switch", "-q", "main"]);
                git(
                    &main,
                    &[
                        "merge",
                        "--no-ff",
                        "forbidden-side",
                        "-m",
                        "merge reverted code side history",
                    ],
                );
                assert_eq!(
                    git(&main, &["rev-parse", "HEAD^{tree}"]),
                    git(
                        &main,
                        &["rev-parse", &format!("{}^{{tree}}", status.input.authority)]
                    )
                );
            }
            _ => unreachable!(),
        }
        assert!(
            !worker.is_finished(),
            "fault must occur during real sealed verification"
        );
        f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
        let actual_authority = f.catalog.revision.clone();
        assert_ne!(actual_authority, status.input.authority);
        fs::write(gate.path.join("release"), "").unwrap();
        let executed = worker.join().unwrap();
        let before = f.outbox.read_inputs().unwrap();
        let cursor = f
            .session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq();
        let rejected = ProjectSession::record_finish_shared(
            &f.session,
            &f.catalog,
            executed,
            &mut f.outbox,
            || Ok(NOW + 34),
        );
        assert!(rejected.is_err(), "{fault} accepted a changed authority");
        assert_eq!(git(f.root.path(), &["rev-parse", "main"]), actual_authority);
        assert_eq!(
            f.outbox.read_inputs().unwrap().digest(),
            before.digest(),
            "{fault}"
        );
        assert_eq!(
            f.session
                .lock()
                .unwrap()
                .protocol()
                .unwrap()
                .contiguous_seq(),
            cursor,
            "{fault}"
        );
        assert!(
            !f.session
                .lock()
                .unwrap()
                .projection()
                .unwrap()
                .objects
                .get("Evidence")
                .is_some_and(|evidence| evidence.contains_key(&status.input.evidence_id)),
            "{fault}"
        );
    }
}
#[test]
fn frozen_current_main_moving_again_after_g5_cannot_cross_final_source_cas() {
    let mut f = claimed(&[SOURCE_GATE_COMMAND], false);
    let (status, gate, worker) = execute_at_source_barrier(&mut f);
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 32),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let checked_authority = f.catalog.revision.clone();
    let main = f.runtime.path().join("racing-main");
    git(
        f.root.path(),
        &["worktree", "add", main.to_str().unwrap(), "main"],
    );
    fs::write(gate.path.join("release"), "").unwrap();
    let executed = worker.join().unwrap();
    let cursor = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let before = f.outbox.read_inputs().unwrap();
    let calls = std::cell::Cell::new(0);
    let result = ProjectSession::record_finish_shared(
        &f.session,
        &f.catalog,
        executed,
        &mut f.outbox,
        || {
            let count = calls.get() + 1;
            calls.set(count);
            if count == 3 {
                fs::write(
                    main.join("docs/tower/state/check-race.txt"),
                    "actual second state advance\n",
                )
                .unwrap();
                git(&main, &["add", "docs/tower/state/check-race.txt"]);
                git(&main, &["commit", "-qm", "advance actual main after G5"]);
            }
            Ok(NOW + 34)
        },
    );
    let error = result.unwrap_err();
    assert!(calls.get() >= 3);
    assert!(
        error.contains("uncertain") && error.contains(&status.request_id),
        "{error}"
    );
    assert_ne!(git(&main, &["rev-parse", "HEAD"]), checked_authority);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before.digest());
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
#[test]
fn frozen_no_tests_rejects_code_advance_without_recording_evidence() {
    let mut f = claimed(&[], false);
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
    let evidence_id = ticket.status().input.evidence_id.clone();
    let ticket =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    let main = f.runtime.path().join("no-tests-main");
    git(
        f.root.path(),
        &["worktree", "add", main.to_str().unwrap(), "main"],
    );
    fs::write(main.join("alpha/a.py"), "# actual code change\n").unwrap();
    git(&main, &["add", "alpha/a.py"]);
    git(
        &main,
        &["commit", "-qm", "code is not a state-only exception"],
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let executed = finish::execute(
        ticket,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let before = f.outbox.read_inputs().unwrap();
    let cursor = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    assert!(ProjectSession::record_finish_shared(
        &f.session,
        &f.catalog,
        executed,
        &mut f.outbox,
        || Ok(NOW + 32)
    )
    .is_err());
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before.digest());
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq(),
        cursor
    );
    assert!(!f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .get("Evidence")
        .is_some_and(|evidence| evidence.contains_key(&evidence_id)));
}

#[test]
fn actual_executed_output_survives_source_refusal_and_records_once_with_same_input() {
    let command = format!(
        "printf . >> \"$(git rev-parse --git-common-dir)/towermd/cache/finish-execution-count\"; {SOURCE_GATE_COMMAND}"
    );
    let mut f = claimed(&[&command], false);
    let (status, gate, worker) = execute_at_source_barrier(&mut f);
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 32),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    fs::write(gate.path.join("release"), "").unwrap();
    let executed = worker.join().unwrap();
    let before = f.outbox.read_inputs().unwrap();
    let refused = ProjectSession::record_borrowed_finish_shared(
        &f.session,
        &f.catalog,
        &executed,
        &mut f.outbox,
        || Ok(NOW + 33),
    );
    assert!(
        refused.is_err(),
        "the old source cannot authorize completion"
    );
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before.digest());
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_ne!(f.catalog.revision, status.input.authority);
    let done = ProjectSession::record_borrowed_finish_shared(
        &f.session,
        &f.catalog,
        &executed,
        &mut f.outbox,
        || Ok(NOW + 34),
    )
    .unwrap();
    assert_eq!(done.status.input, status.input);
    assert_eq!(
        done.evidence.as_ref().unwrap().result,
        EvidenceResult::Passed
    );
    let again = ProjectSession::record_borrowed_finish_shared(
        &f.session,
        &f.catalog,
        &executed,
        &mut f.outbox,
        || Ok(NOW + 35),
    )
    .unwrap();
    assert_eq!(again.actual, done.actual);
    assert_eq!(
        fs::read(
            f.catalog
                .common_dir
                .join("towermd/cache/finish-execution-count")
        )
        .unwrap(),
        b"."
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.eid == status.completed_eid)
            .count(),
        1
    );
    assert_eq!(run(&f, &status.input.run_id)["state"], "verifying");
}

#[test]
fn actual_cancellation_after_verifier_cleanup_records_interrupted_without_failure() {
    use std::sync::atomic::Ordering;
    let mut f = claimed(&[SOURCE_GATE_COMMAND], false);
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
    let status = ticket.status().clone();
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    let gate = SourceGate {
        path: f
            .catalog
            .common_dir
            .join("towermd/cache/finish-source-gate"),
    };
    let before_worktrees = git(f.root.path(), &["worktree", "list", "--porcelain"]);
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let worker =
        std::thread::spawn(move || finish::execute(execution, Duration::from_secs(30), signal));
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !gate.path.join("entered").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        gate.path.join("entered").exists(),
        "actual verifier did not enter its owned command"
    );
    assert!(!worker.is_finished());
    cancelled.store(true, Ordering::Release);
    let executed = worker.join().unwrap();
    assert_eq!(
        git(f.root.path(), &["worktree", "list", "--porcelain"]),
        before_worktrees,
        "interruption is produced only after owned worktree/process cleanup"
    );
    let interrupted = ProjectSession::record_borrowed_finish_shared(
        &f.session,
        &f.catalog,
        &executed,
        &mut f.outbox,
        || Ok(NOW + 32),
    )
    .unwrap();
    assert_eq!(interrupted.status.phase, JobPhase::Interrupted);
    assert!(interrupted.evidence.is_none());
    assert_eq!(interrupted.actual.kind, "finish.interrupted");
    assert_eq!(interrupted.status.input, status.input);
    assert_eq!(run(&f, &status.input.run_id)["finish"]["failures"], 0);
    assert_eq!(run(&f, &status.input.run_id)["state"], "running");
    assert!(!f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .get("Evidence")
        .is_some_and(|objects| objects.contains_key(&status.input.evidence_id)));
    let repeated = ProjectSession::record_borrowed_finish_shared(
        &f.session,
        &f.catalog,
        &executed,
        &mut f.outbox,
        || Ok(NOW + 33),
    )
    .unwrap();
    assert_eq!(repeated.actual, interrupted.actual);
}

#[test]
fn actual_no_command_cancellation_repairs_original_interrupted_after_confirmation_fault() {
    let (mut f, capture) =
        interrupt_finish_confirmation(claimed(&[], false), "finish.interrupted", NOW + 12);
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
    let status = ticket.status().clone();
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    let executed = finish::execute(
        execution,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(true)),
    );
    let failed = ProjectSession::record_borrowed_finish_shared(
        &f.session,
        &f.catalog,
        &executed,
        &mut f.outbox,
        || Ok(NOW + 32),
    )
    .unwrap_err();
    assert!(
        failed.contains("uncertain") && failed.contains(&id),
        "{failed}"
    );
    let original = restore_interrupted_finish(&capture);
    assert_eq!(original.actual.kind, "finish.interrupted");
    assert_eq!(original.original.guards, status.input.fences);
    assert_eq!(original.actual.data["run"]["finish"]["failures"], 0);
    // Explicit --wait-style receipt coordination may finish before the retained
    // owner retries. It must still return the same terminal, not reject its phase.
    let repaired = ProjectSession::finish_view_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &mut f.outbox,
        || Ok(NOW + 33),
    )
    .unwrap()
    .unwrap();
    assert_eq!(repaired.status.phase, JobPhase::Interrupted);
    assert_eq!(repaired.actual.as_ref(), Some(&original.actual));
    let retained = ProjectSession::record_borrowed_finish_shared(
        &f.session,
        &f.catalog,
        &executed,
        &mut f.outbox,
        || Ok(NOW + 34),
    )
    .unwrap();
    assert_eq!(retained.actual, original.actual);
    assert!(retained.evidence.is_none());
    assert_eq!(retained.status.input, status.input);
    assert_eq!(run(&f, &status.input.run_id)["finish"]["failures"], 0);
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.eid == original.actual.eid)
            .count(),
        1
    );
    assert!(!inputs
        .pending()
        .iter()
        .any(|event| event.eid == original.actual.eid));
    assert!(!inputs
        .confirmed()
        .iter()
        .any(|event| event.eid == status.completed_eid));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .get("Evidence")
        .is_some_and(|objects| objects.contains_key(&status.input.evidence_id)));
}

#[test]
fn task_progress_reads_real_evidence_then_canonical_merge_and_cold_compacted_history() {
    use towermd_core::app_read_model::TaskProgressState;
    let mut f = claimed(&["exit 0"], false);
    fs::write(
        f.root.path().join("alpha/progress.py"),
        "# real task progress\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/progress.py"]);
    git(f.root.path(), &["commit", "-qm", "actual progress input"]);
    let request = uuid::Uuid::now_v7().to_string();
    let done = completed(&mut f, &request, NOW + 30);
    let input = f.outbox.read_inputs().unwrap();
    let journal = f.catalog.common_dir.join("towermd/journal.jsonl");
    let bytes = fs::read(&journal).unwrap();
    let before = input.digest().to_owned();
    let view =
        ProjectSession::read_task_progress(&f.catalog, &input, NOW + 33, Some(&f.session)).unwrap();
    assert_eq!(view.state, TaskProgressState::Recorded);
    assert_eq!(
        view.tasks
            .iter()
            .find(|task| task["id"] == "T-finish")
            .unwrap()["state"],
        "verifying"
    );
    assert_eq!(
        view.runs
            .iter()
            .find(|run| run["id"] == done.status.input.run_id)
            .unwrap()["state"],
        "verifying"
    );
    assert!(view
        .evidence
        .iter()
        .any(|e| e["id"] == done.evidence.as_ref().unwrap().id && e["result"] == "passed"));
    assert_eq!(fs::read(&journal).unwrap(), bytes);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before);
    let footer = ProjectSession::task_trailers_shared(
        &f.session,
        &f.catalog,
        "host",
        "T-finish",
        None,
        &mut f.outbox,
        || Ok(NOW + 35),
    )
    .unwrap();
    git(
        f.root.path(),
        &[
            "commit",
            "--amend",
            "-qm",
            &format!("real progress main entry\n\n{}", footer.trailers),
        ],
    );
    let commit = git(f.root.path(), &["rev-parse", "HEAD"]);
    git(f.root.path(), &["update-ref", "refs/heads/main", &commit]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let merged = ProjectSession::observe_task_merges_shared(
        &f.session,
        &f.catalog,
        "host",
        &mut f.outbox,
        || Ok(NOW + 36),
    )
    .unwrap();
    assert_eq!(merged.merged.len(), 1);
    let warm = ProjectSession::read_task_progress(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        NOW + 37,
        Some(&f.session),
    )
    .unwrap();
    assert_eq!(
        warm.tasks
            .iter()
            .find(|task| task["id"] == "T-finish")
            .unwrap()["state"],
        "merged"
    );
    assert_eq!(
        warm.runs
            .iter()
            .find(|run| run["id"] == done.status.input.run_id)
            .unwrap()["state"],
        "ended"
    );
    let snapshot = f
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
    assert_eq!(snapshot.status, "written");
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let expected = f.session.lock().unwrap().projection().unwrap().clone();
    let through = f.session.lock().unwrap().applied_seq();
    let input = f.outbox.read_inputs().unwrap();
    drop(f.session);
    let bytes = fs::read(&journal).unwrap();
    let cold = ProjectSession::read_task_progress(&f.catalog, &input, NOW + 41, None).unwrap();
    assert_eq!(
        cold.tasks,
        expected.objects["Task"]
            .values()
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        cold.runs,
        expected.objects["Run"]
            .values()
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        cold.evidence,
        expected.objects["Evidence"]
            .values()
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(cold.through_seq, Some(through));
    assert!(cold.floor_seq.unwrap() > 0);
    assert_eq!(fs::read(&journal).unwrap(), bytes);
    let split = bytes.iter().position(|byte| *byte == b'\n').unwrap();
    for field in ["projection", "ack"] {
        let mut header: serde_json::Value = serde_json::from_slice(&bytes[..split]).unwrap();
        if field == "projection" {
            header["checkpoint"]["checkpoint"]["projection"]["objects"]["Task"]["T-finish"]
                ["state"] = serde_json::json!("ready");
        } else {
            header["checkpoint"]["ack"]["commit"] = serde_json::json!("f".repeat(40));
        }
        let mut changed = serde_json::to_vec(&header).unwrap();
        changed.extend_from_slice(&bytes[split..]);
        fs::write(&journal, &changed).unwrap();
        assert!(
            ProjectSession::read_task_progress(&f.catalog, &input, NOW + 42, None).is_err(),
            "{field}"
        );
        assert_eq!(
            fs::read(&journal).unwrap(),
            changed,
            "read must not repair bad checkpoint"
        );
    }
    fs::write(journal, bytes).unwrap();
}

#[test]
fn task_progress_expired_recorded_run_never_renews_expires_or_releases_it() {
    let f = claimed(&["exit 0"], false);
    let inputs = f.outbox.read_inputs().unwrap();
    let bytes = fs::read(f.catalog.common_dir.join("towermd/journal.jsonl")).unwrap();
    let warm = ProjectSession::read_task_progress(
        &f.catalog,
        &inputs,
        NOW + 16 * 60_000,
        Some(&f.session),
    )
    .unwrap();
    assert_eq!(warm.runs[0]["state"], "running");
    drop(f.session);
    let cold =
        ProjectSession::read_task_progress(&f.catalog, &inputs, NOW + 16 * 60_000, None).unwrap();
    assert_eq!(warm, cold);
    assert_eq!(
        fs::read(f.catalog.common_dir.join("towermd/journal.jsonl")).unwrap(),
        bytes
    );
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), inputs.digest());
}

#[test]
fn task_progress_missing_hub_keeps_snapshot_tasks_explicitly_historical() {
    use towermd_core::app_read_model::TaskProgressState;
    let mut f = claimed(&["exit 0"], false);
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 20),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let inputs = f.outbox.read_inputs().unwrap();
    let path = f.catalog.common_dir.join("towermd");
    drop(f.session);
    fs::remove_dir_all(&path).unwrap();
    let view = ProjectSession::read_task_progress(&f.catalog, &inputs, NOW + 21, None).unwrap();
    assert_eq!(view.state, TaskProgressState::RecoveryRequired);
    assert!(view.tasks.iter().any(|task| task["id"] == "T-finish"));
    assert!(view.runs.is_empty() && view.evidence.is_empty());
    assert!(!path.exists());
}
