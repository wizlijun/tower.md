//! Real high-risk verifier/Review producers with a clock-driven HEAD race.
use super::super::finish::voluntary_fixture as fixture;
use super::*;
use crate::{
    baseline_review::review_baseline_shared,
    finish::{self, FinishStart},
    human_intervention::{HumanCommand, ReviewVerdict},
    recovery_contract::{baseline::Verdict, recorded::TaskReviewAction},
    snapshot_service::PublicationReport,
    tasks::{TaskCommand, TaskSpec},
};
use fixture::*;
use std::{
    cell::Cell,
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

thread_local! { static CLOCK_BOUNDARY: Cell<Option<&'static str>> = const { Cell::new(None) }; }
pub(super) struct ClockBoundary(Option<&'static str>);
impl ClockBoundary {
    pub(super) fn enter(boundary: &'static str) -> Self {
        Self(CLOCK_BOUNDARY.with(|current| current.replace(Some(boundary))))
    }
}
impl Drop for ClockBoundary {
    fn drop(&mut self) {
        CLOCK_BOUNDARY.with(|current| current.set(self.0));
    }
}
fn review(
    f: &mut Fixture,
    gate: ReviewGate,
    content: Option<String>,
    at: u64,
) -> TaskReviewRequestOutcome {
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
        || Ok(at),
    )
    .unwrap()
}
fn approved_plan(f: &mut Fixture) {
    let opening = review(
        f,
        ReviewGate::G1,
        Some("actual immutable approved plan\n".into()),
        NOW + 20,
    );
    ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &TaskReviewAction::Review {
            hold: opening.receipt.result_ref.hold,
            binding: opening.receipt.result_ref.binding,
            verdict: ReviewVerdict::Approved,
            comment: "actual App-authority fixture, not GUI acceptance".into(),
        },
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
}
fn passed_fixture(
    early_g4: bool,
) -> (
    Fixture,
    finish::FinishOutcome,
    Option<TaskReviewRequestOutcome>,
) {
    let mut f = configured_fixture(&[("docs/tower/specs/AC-head.md", "---\ntype: Acceptance\nid: AC-head\nmodule: A-alpha\ncriteria: [actual final HEAD proof]\n---\n")], &["exit 0"], true);
    let action = action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
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
                &approval.receipt.result_ref.approval_eid,
                "host",
                || Ok(NOW + 5)
            )
            .unwrap(),
        PublicationReport::Recorded { .. }
    ));
    git(f.root.path(), &["switch", "-qc", "t/T-head", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-head".into(),
                title: "actual HEAD race".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-head".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 10,
        &mut f.outbox,
    )
    .unwrap();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-head".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 11,
        &mut f.outbox,
    )
    .unwrap();
    fs::write(
        f.root.path().join("alpha/result.py"),
        "# sealed original tree\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/result.py"]);
    git(f.root.path(), &["commit", "-qm", "actual verified code"]);
    approved_plan(&mut f);
    let opening = early_g4.then(|| review(&mut f, ReviewGate::G4, None, NOW + 22));
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
        panic!("new finish must be queued");
    };
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
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
        || Ok(NOW + 32),
    )
    .unwrap();
    assert_eq!(
        done.evidence.as_ref().unwrap().result,
        finish::EvidenceResult::Passed
    );
    (f, done, opening)
}
fn changed_clock(root: &Path, target: &'static str, changed: &Cell<bool>) -> Result<u64, String> {
    if CLOCK_BOUNDARY.with(|phase| phase.get()) == Some(target) && !changed.replace(true) {
        fs::write(
            root.join("alpha/result.py"),
            "# real code commit after final G5\n",
        )
        .unwrap();
        git(root, &["add", "alpha/result.py"]);
        git(
            root,
            &["commit", "-qm", "clock callback changed actual HEAD"],
        );
    }
    Ok(NOW + 40)
}
fn assert_no_human_append(f: &Fixture, before: u64, digest: &str, done: &finish::FinishOutcome) {
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    let added = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before, NOW + 40)
        .unwrap();
    assert!(added.events.iter().all(|event| !matches!(
        event.kind.as_str(),
        "hold.opened" | "hold.requested" | "hold.reviewed"
    )));
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .contains(&done.actual));
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Task"]["T-head"]["state"],
        "verifying"
    );
}
#[test]
fn frozen_g4_opening_rejects_head_change_from_final_g5_or_append_clock() {
    for boundary in ["after_g5", "before_append"] {
        let (mut f, done, _) = passed_fixture(false);
        let subject = prove_frozen_g4_subject(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            "host",
            &done.status.input.run_id,
            &done.status.input.tree,
            &|| Ok(NOW + 39),
        )
        .map_err(|_| "actual frozen proof failed")
        .unwrap()
        .expect("actual passed verifier subject");
        let root = f.root.path().to_path_buf();
        let id = uuid::Uuid::now_v7().to_string();
        let before = f
            .session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq();
        let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
        let authority = f.catalog.revision.clone();
        let changed = Cell::new(false);
        let error = ProjectSession::request_finish_g4_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            subject,
            &mut f.outbox,
            || changed_clock(&root, boundary, &changed),
        )
        .unwrap_err();
        assert!(changed.get(), "{boundary}");
        assert!(
            error.contains("uncertain") && error.contains(&id) && error.contains("HEAD changed"),
            "{boundary}: {error}"
        );
        assert_eq!(git(&root, &["rev-parse", "main"]), authority);
        assert_no_human_append(&f, before, &digest, &done);
    }
}
#[test]
fn frozen_g4_answer_rejects_head_change_from_final_g5_or_append_clock() {
    for boundary in ["after_g5", "before_append"] {
        let (mut f, done, opening) = passed_fixture(true);
        let opening = opening.unwrap();
        let action = TaskReviewAction::Review {
            hold: opening.receipt.result_ref.hold.clone(),
            binding: opening.receipt.result_ref.binding,
            verdict: ReviewVerdict::Approved,
            comment: "must not approve a changed HEAD".into(),
        };
        let root = f.root.path().to_path_buf();
        let id = uuid::Uuid::now_v7().to_string();
        let before = f
            .session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq();
        let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
        let authority = f.catalog.revision.clone();
        let changed = Cell::new(false);
        let error = ProjectSession::answer_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            "human:owner",
            &id,
            &action,
            &mut f.outbox,
            || changed_clock(&root, boundary, &changed),
        )
        .unwrap_err();
        assert!(changed.get(), "{boundary}");
        assert!(
            error.contains("uncertain") && error.contains(&id) && error.contains("HEAD changed"),
            "{boundary}: {error}"
        );
        assert_eq!(git(&root, &["rev-parse", "main"]), authority);
        assert_no_human_append(&f, before, &digest, &done);
        assert!(
            f.session.lock().unwrap().projection().unwrap().objects["Hold"]
                [&opening.receipt.result_ref.hold]["reply"]
                .is_null()
        );
    }
}
