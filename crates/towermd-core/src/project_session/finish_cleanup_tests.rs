//! Actual runner output with withheld process-cleanup confirmation. The fault
//! is installed only after real kill/reap; no synthetic execution ticket/flag.
use super::voluntary_fixture::{self as fixture, git, NOW};
use super::*;
use crate::{
    baseline_review::review_baseline_shared,
    recovery_contract::baseline::Verdict,
    snapshot_service::PublicationReport,
    tasks::{TaskCommand, TaskSpec},
};
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

#[test]
fn actual_unconfirmed_cleanup_never_records_a_new_terminal_or_reexecutes() {
    let marker = tempfile::tempdir().unwrap();
    let marker_path = marker.path().join("actual-executions");
    let command = format!(
        "printf x >> '{}'",
        marker_path.display().to_string().replace('\'', "'\\''")
    );
    let mut f = fixture::configured_fixture(&[("docs/tower/specs/AC-cleanup.md",
        "---\ntype: Acceptance\nid: AC-cleanup\nmodule: A-alpha\ncriteria: [actual process cleanup]\n---\n")], &[command.as_str()], false);
    let approval = fixture::action(&f, Verdict::Approved);
    let receipt = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &approval,
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
    git(f.root.path(), &["switch", "-qc", "t/T-cleanup", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-cleanup".into(),
                title: "actual cleanup".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-cleanup".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 10,
        &mut f.outbox,
    )
    .unwrap();
    let run = ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-cleanup".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 11,
        &mut f.outbox,
    )
    .unwrap();
    fs::create_dir_all(f.root.path().join("alpha")).unwrap();
    fs::write(
        f.root.path().join("alpha/cleanup.txt"),
        "real selected code\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/cleanup.txt"]);
    git(
        f.root.path(),
        &["commit", "-qm", "actual finish cleanup subject"],
    );
    let id = uuid::Uuid::now_v7().to_string();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 20),
    )
    .unwrap() else {
        panic!("new actual ticket");
    };
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 21)
        })
        .unwrap();
    let before = fs::read(&marker_path).unwrap_or_default();
    let executed = crate::verifier::with_unconfirmed_cleanup_for_test(|| {
        finish::execute(
            execution,
            Duration::from_secs(10),
            Arc::new(AtomicBool::new(false)),
        )
    });
    assert!(!executed.cleanup_confirmed());
    assert!(executed.result.is_err());
    let after = fs::read(&marker_path).unwrap();
    assert_eq!(
        after.len(),
        before.len() + 1,
        "real selected command executes once before cleanup is withheld"
    );
    for now in [NOW + 22, NOW + 23] {
        let error = ProjectSession::record_borrowed_finish_shared(
            &f.session,
            &f.catalog,
            &executed,
            &mut f.outbox,
            || Ok(now),
        )
        .unwrap_err();
        assert!(
            error.contains(&id) && error.contains("cleanup is unconfirmed"),
            "{error}"
        );
        let inputs = f.outbox.read_inputs().unwrap();
        assert!(!inputs.pending().iter().any(|event| matches!(
            event.kind.as_str(),
            "finish.completed" | "finish.interrupted"
        )));
        assert!(!inputs.confirmed().iter().any(|event| matches!(
            event.kind.as_str(),
            "finish.completed" | "finish.interrupted"
        )));
    }
    let replay = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 24),
    )
    .unwrap();
    let FinishStart::Existing(existing) = replay else {
        panic!("unconfirmed cleanup must not mint another execution ticket");
    };
    assert_eq!(existing.phase, JobPhase::Running);
    let state = f.session.lock().unwrap().projection().unwrap().clone();
    let run_id = run["id"].as_str().unwrap();
    assert_eq!(state.objects["Run"][run_id]["finish"]["failures"], 0);
    assert!(state
        .objects
        .get("Evidence")
        .is_none_or(|all| !all.contains_key(&existing.input.evidence_id)));
    assert_eq!(
        fs::read(&marker_path).unwrap(),
        after,
        "record/retry never reexecutes the verifier"
    );
}
