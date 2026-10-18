//! Real publication, Hub and Task producers; no injected Review facts.
#[path = "support/baseline_review.rs"]
mod fixture;
use fixture::*;
use std::sync::Arc;
use towermd_core::{
    baseline_review::review_baseline_shared,
    catalog::Catalog,
    human_intervention::{HumanCommand, ReviewGate, ReviewSubject, ReviewVerdict},
    project_session::ProjectSession,
    recovery_contract::{baseline::Verdict, recorded::TaskReviewAction},
    request_ledger::{ReviewValidity, TaskReviewRequestOutcome},
    snapshot_service::PublicationReport,
    tasks::{TaskCommand, TaskSpec},
};

fn claimed() -> Fixture {
    claimed_with_linked_worktree(false)
}
fn claimed_with_linked_worktree(linked: bool) -> Fixture {
    let mut f = fixture_with_source_files(&[("docs/tower/specs/AC-review.md",
        "---\ntype: Acceptance\nid: AC-review\nmodule: A-alpha\ncriteria: [real review gate]\n---\n")]);
    let baseline_action = action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &baseline_action,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap();
    let published = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 5),
        )
        .unwrap();
    assert!(matches!(published, PublicationReport::Recorded { .. }));
    let task_root = if linked {
        let path = f.runtime.path().join("task-worktree");
        git(
            f.root.path(),
            &[
                "worktree",
                "add",
                "-b",
                "t/T-review",
                path.to_str().unwrap(),
                "main",
            ],
        );
        path
    } else {
        git(f.root.path(), &["switch", "-qc", "t/T-review", "main"]);
        f.root.path().to_owned()
    };
    f.catalog = Arc::new(Catalog::load(&task_root).unwrap());
    assert_eq!(f.catalog.objects["A-alpha"].doc_type(), Some("Module"));
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-review".into(),
                title: "真实评审任务".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-review".into()],
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
            id: "T-review".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 11,
        &mut f.outbox,
    )
    .unwrap();
    f
}

fn plan(text: &str) -> HumanCommand {
    HumanCommand::ReviewRequest {
        gate: ReviewGate::G1,
        content: Some(text.into()),
        // These are provenance, and deliberately do not name a readable file/commit.
        source_commit: Some("f".repeat(40)),
        source_path: Some("outside/plan.md".into()),
    }
}
fn request(f: &mut Fixture, id: &str, command: &HumanCommand) -> TaskReviewRequestOutcome {
    ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        id,
        command,
        None,
        &mut f.outbox,
        || Ok(NOW + 20),
    )
    .unwrap()
}
fn approve(open: &TaskReviewRequestOutcome) -> TaskReviewAction {
    TaskReviewAction::Review {
        hold: open.receipt.result_ref.hold.clone(),
        binding: open.receipt.result_ref.binding.clone(),
        verdict: ReviewVerdict::Approved,
        comment: "已检查原计划内容".into(),
    }
}

#[test]
fn real_plan_request_reuse_answer_and_a_b_a_keep_raw_identity() {
    let mut f = claimed();
    let command = plan("第一步\r\n第二步\n");
    let first_id = uuid::Uuid::now_v7().to_string();
    let first = request(&mut f, &first_id, &command);
    assert_eq!(first.current_validity, ReviewValidity::Current);
    let second_id = uuid::Uuid::now_v7().to_string();
    let second = request(&mut f, &second_id, &command);
    assert_eq!(first.receipt.result_ref, second.receipt.result_ref);
    assert_ne!(first.receipt.request_id, second.receipt.request_id);
    let inputs = f.outbox.read_inputs().unwrap();
    let opening = inputs
        .confirmed()
        .iter()
        .find(|e| e.eid == first.receipt.result_ref.opening_eid)
        .unwrap();
    assert_eq!(opening.kind, "hold.opened");
    assert_eq!(opening.data["hold"]["opened_seq"], 0);
    assert_eq!(
        opening.data["hold"]["body"]["plan_content"],
        "第一步\r\n第二步\n"
    );
    use sha2::Digest;
    assert_eq!(
        first.receipt.result_ref.binding.subject,
        ReviewSubject::Plan {
            sha256: format!(
                "{:x}",
                sha2::Sha256::digest("第一步\r\n第二步\n".as_bytes())
            )
        }
    );
    assert!(inputs
        .confirmed()
        .iter()
        .any(|e| e.kind == "hold.requested" && e.data["p3_request"]["request_id"] == second_id));
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&first.receipt.result_ref.binding)
        .is_err());
    let answer_id = uuid::Uuid::now_v7().to_string();
    let answer = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &approve(&first),
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
    assert_eq!(answer.current_validity, ReviewValidity::Current);
    let input = f.outbox.read_inputs().unwrap();
    let event = input
        .confirmed()
        .iter()
        .find(|e| e.eid == answer.receipt.result_ref.fact.answer_eid)
        .unwrap();
    assert_eq!(event.kind, "hold.reviewed");
    assert_eq!(
        event.data["hold"]["opened_seq"], 0,
        "raw proof is not projected Event.seq"
    );
    assert!(event.data["request_body"]["run"].is_null());
    assert_eq!(event.guards, first.receipt.result_ref.binding.fences);
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&first.receipt.result_ref.binding)
        .is_ok());
    let same_answer = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &approve(&first),
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
    assert_eq!(same_answer.receipt, answer.receipt);
    let mut conflicting = approve(&first);
    let TaskReviewAction::Review { verdict, .. } = &mut conflicting;
    *verdict = ReviewVerdict::Rejected;
    assert!(ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &conflicting,
        &mut f.outbox,
        || Ok(NOW + 21)
    )
    .is_err());
    let after_answer = request(&mut f, &uuid::Uuid::now_v7().to_string(), &command);
    assert_eq!(after_answer.receipt.result_ref, first.receipt.result_ref);
    assert!(
        f.session
            .lock()
            .unwrap()
            .human()
            .unwrap()
            .require_human_review(&first.receipt.result_ref.binding)
            .is_ok(),
        "a new request receipt must not reset the existing effective human answer"
    );
    let b = request(&mut f, &uuid::Uuid::now_v7().to_string(), &plan("不同计划"));
    assert_ne!(b.receipt.result_ref.hold, first.receipt.result_ref.hold);
    let historical = request(&mut f, &first_id, &command);
    assert_eq!(historical.receipt, first.receipt);
    assert!(matches!(
        historical.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    let a_again = request(&mut f, &uuid::Uuid::now_v7().to_string(), &command);
    assert_ne!(
        a_again.receipt.result_ref.hold,
        first.receipt.result_ref.hold
    );
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&a_again.receipt.result_ref.binding)
        .is_err());
}

#[test]
fn actual_g4_check_rejects_dirty_tree_and_old_tree_answer() {
    let mut f = claimed();
    let command = HumanCommand::ReviewRequest {
        gate: ReviewGate::G4,
        content: None,
        source_commit: None,
        source_path: None,
    };
    std::fs::write(f.root.path().join("alpha/a.py"), "# dirty\n").unwrap();
    let dirty = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 20),
    )
    .unwrap_err();
    assert!(
        dirty.contains("Dirty") && !dirty.contains("uncertain"),
        "{dirty}"
    );
    git(f.root.path(), &["add", "alpha/a.py"]);
    git(f.root.path(), &["commit", "-qm", "actual task change"]);
    let id = uuid::Uuid::now_v7().to_string();
    let first = request(&mut f, &id, &command);
    assert_eq!(
        first.receipt.result_ref.binding.subject,
        ReviewSubject::Tree {
            tree: git(f.root.path(), &["rev-parse", "HEAD^{tree}"])
        }
    );
    std::fs::write(f.root.path().join("alpha/a.py"), "# next tree\n").unwrap();
    git(f.root.path(), &["add", "alpha/a.py"]);
    git(f.root.path(), &["commit", "-qm", "new task tree"]);
    let old_tree = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &approve(&first),
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap_err();
    assert!(
        !old_tree.contains("uncertain"),
        "stable old-tree binding refusal: {old_tree}"
    );
    let historical = request(&mut f, &id, &command);
    assert_eq!(historical.receipt, first.receipt);
    assert!(matches!(
        historical.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    let next = request(&mut f, &uuid::Uuid::now_v7().to_string(), &command);
    assert_ne!(
        next.receipt.result_ref.binding.subject,
        first.receipt.result_ref.binding.subject
    );
}

#[test]
fn request_identity_conflicts_precede_live_run_and_process_cannot_answer() {
    let mut f = claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let first = request(&mut f, &id, &plan("原计划"));
    let conflict = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &id,
        &approve(&first),
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap_err();
    assert!(
        conflict.contains("request_id reused"),
        "one namespace across CLI/App: {conflict}"
    );
    assert!(ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "process:towermd-cli",
        &uuid::Uuid::now_v7().to_string(),
        &approve(&first),
        &mut f.outbox,
        || Ok(NOW + 21)
    )
    .is_err());
    let mut wrong = approve(&first);
    let TaskReviewAction::Review { binding, .. } = &mut wrong;
    binding.fences[0].epoch += 1;
    assert!(ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &wrong,
        &mut f.outbox,
        || Ok(NOW + 21)
    )
    .is_err());
    let mut extra = approve(&first);
    let TaskReviewAction::Review { binding, .. } = &mut extra;
    binding.fences.push(towermd_core::hub_protocol::Fence {
        name: "module:not-the-task".into(),
        holder: binding.run.clone(),
        epoch: 1,
    });
    assert!(ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &extra,
        &mut f.outbox,
        || Ok(NOW + 21)
    )
    .is_err());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 22,
        &mut f.outbox,
    )
    .unwrap();
    let error = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &plan("异体请求"),
        None,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap_err();
    assert!(error.contains("request_id reused"), "{error}");
    let original = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &plan("原计划"),
        None,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap();
    assert_eq!(original.receipt, first.receipt);
    assert!(matches!(
        original.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
}

#[test]
fn snapshot_compaction_restart_restores_actual_request_receipts_and_review() {
    let mut f = claimed();
    let command = plan("持久计划");
    let first = request(&mut f, &uuid::Uuid::now_v7().to_string(), &command);
    let reused_id = uuid::Uuid::now_v7().to_string();
    let reused = request(&mut f, &reused_id, &command);
    let action = approve(&first);
    let answer_id = uuid::Uuid::now_v7().to_string();
    let answered = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
    f.writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 30),
            &mut f.outbox,
        )
        .unwrap();
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    drop(f.session);
    f.session = Arc::new(std::sync::Mutex::new(
        ProjectSession::open(
            &f.catalog.common_dir.join("towermd"),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            NOW + 31,
        )
        .unwrap(),
    ));
    let retried = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &reused_id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 32),
    )
    .unwrap();
    assert_eq!(retried.receipt, reused.receipt);
    let answer_retry = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 32),
    )
    .unwrap();
    assert_eq!(answer_retry.receipt, answered.receipt);
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&first.receipt.result_ref.binding)
        .is_ok());
}

#[test]
fn app_load_preserves_original_and_projected_sequence_and_does_not_renew() {
    let mut f = claimed();
    let first = request(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &plan("只读原计划"),
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let before_digest = inputs.digest().to_owned();
    let cursor = f.session.lock().unwrap().applied_seq();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 21)
        .unwrap();
    let view = ProjectSession::load_task_review_shared(
        &f.session,
        &f.catalog,
        &inputs,
        "host",
        &first.receipt.result_ref.hold,
        || Ok(NOW + 21),
    )
    .unwrap();
    assert_eq!(view.project_id, f.catalog.config.project_id());
    assert_eq!(view.opening.eid, first.receipt.result_ref.opening_eid);
    assert_eq!(view.opening.data["hold"]["opened_seq"], 0);
    assert_eq!(view.current_hold["opened_seq"], view.opening.seq);
    assert_eq!(view.current_validity, ReviewValidity::Current);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before_digest);
    let after = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 21)
        .unwrap();
    assert_eq!(
        after, before,
        "reading must neither append business events nor renew system leases"
    );
}

#[test]
fn actual_non_approval_verdicts_remain_blocking_and_missing_baseline_is_not_invented() {
    let mut f = claimed();
    for (content, verdict) in [
        ("需要修改", ReviewVerdict::ChangesRequested),
        ("拒绝计划", ReviewVerdict::Rejected),
    ] {
        let open = request(&mut f, &uuid::Uuid::now_v7().to_string(), &plan(content));
        let action = TaskReviewAction::Review {
            hold: open.receipt.result_ref.hold.clone(),
            binding: open.receipt.result_ref.binding.clone(),
            verdict: verdict.clone(),
            comment: content.into(),
        };
        let result = ProjectSession::answer_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            "human:owner",
            &uuid::Uuid::now_v7().to_string(),
            &action,
            &mut f.outbox,
            || Ok(NOW + 21),
        )
        .unwrap();
        assert_eq!(result.current_validity, ReviewValidity::Current);
        assert_eq!(result.receipt.result_ref.verdict, verdict);
        assert!(f
            .session
            .lock()
            .unwrap()
            .human()
            .unwrap()
            .require_human_review(&open.receipt.result_ref.binding)
            .is_err());
    }
    git(f.root.path(), &["tag", "-d", "towermd/B0"]);
    let before = f.outbox.read_inputs().unwrap().digest().to_owned();
    assert!(ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        &plan("无真实基线"),
        None,
        &mut f.outbox,
        || Ok(NOW + 22)
    )
    .is_err());
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), before);
    assert!(git(f.root.path(), &["tag", "--list", "towermd/B0"]).is_empty());
}

fn rebuild_uncheckpointed_review(keep_original_confirmed: bool) {
    use towermd_core::{hub_protocol::Event, recovery, snapshot_service};
    let mut f = claimed();
    // Freeze the real active Run before either Review producer executes.
    f.writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 15),
            &mut f.outbox,
        )
        .unwrap();
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let source = snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let command = plan("恢复前原文\r\n保留末行与来源\n");
    let request_id = uuid::Uuid::now_v7().to_string();
    let opened = request(&mut f, &request_id, &command);
    let answer_id = uuid::Uuid::now_v7().to_string();
    let action = approve(&opened);
    let answered = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let opening = inputs
        .confirmed()
        .iter()
        .find(|event| event.eid == opened.receipt.result_ref.opening_eid)
        .unwrap()
        .clone();
    let answer = inputs
        .confirmed()
        .iter()
        .find(|event| event.eid == answered.receipt.result_ref.fact.answer_eid)
        .unwrap()
        .clone();
    assert!(opening.seq > source.snapshot.through_seq && answer.seq > source.snapshot.through_seq);
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&opened.receipt.result_ref.binding)
        .is_ok());
    assert!(
        !source
            .archives
            .values()
            .any(|text| text.lines().any(|line| {
                let event: Event = serde_json::from_str(line).unwrap();
                event.eid == opening.eid || event.eid == answer.eid
            })),
        "the test must lose uncheckpointed Review receipts, not replay an existing Human snapshot"
    );
    if !keep_original_confirmed {
        // Retain exact pending intents but remove their only original accepted
        // receipt records. A later stale outer Event must not invent old success.
        for entry in std::fs::read_dir(f.runtime.path().join("outbox")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let lines = text
                .lines()
                .filter(|line| {
                    let row: serde_json::Value = serde_json::from_str(line).unwrap();
                    !(row["state"] == "confirmed"
                        && (row["original"]["eid"] == opening.eid
                            || row["original"]["eid"] == answer.eid))
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(path, format!("{lines}\n")).unwrap();
        }
    }
    drop(f.session);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let prepared = recovery::prepare(
        &f.catalog,
        &inputs,
        NOW + 30,
        NOW + 300_000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (report, rebuilt) = f
        .writer
        .confirm_rebuild(
            None,
            &f.catalog,
            &prepared,
            "human:owner",
            prepared.expires_at,
            &inputs,
            "host",
            || Ok(NOW + 40),
        )
        .unwrap();
    assert_eq!(report.status, "rebuilt");
    f.session = rebuilt;
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let saved = snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let events: Vec<Event> = saved
        .archives
        .values()
        .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
        .collect();
    let stale_opening = events
        .iter()
        .find(|event| event.eid == opening.eid)
        .unwrap();
    let stale_answer = events.iter().find(|event| event.eid == answer.eid).unwrap();
    assert!(stale_opening.stale && stale_answer.stale);
    assert!(stale_opening.seq > prepared.seq_boundary && stale_answer.seq > prepared.seq_boundary);
    assert_eq!(stale_opening.data, opening.data);
    assert_eq!(stale_answer.data, answer.data);
    assert_eq!(stale_opening.guards, opening.guards);
    assert_eq!(stale_answer.guards, answer.guards);
    {
        let live = f.session.lock().unwrap();
        assert!(live
            .human()
            .unwrap()
            .current_plan_subject(&opened.receipt.result_ref.binding.run)
            .is_err());
        assert!(live
            .human()
            .unwrap()
            .require_human_review(&opened.receipt.result_ref.binding)
            .is_err());
        assert!(!live
            .human()
            .unwrap()
            .holds
            .contains_key(&opened.receipt.result_ref.hold));
        assert_eq!(
            live.projection().unwrap().objects["Run"][&opened.receipt.result_ref.binding.run]
                ["state"],
            "released"
        );
    }
    let inputs = f.outbox.read_inputs().unwrap();
    let digest = inputs.digest().to_owned();
    let cursor = f.session.lock().unwrap().applied_seq();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 41)
        .unwrap();
    let view = ProjectSession::load_task_review_shared(
        &f.session,
        &f.catalog,
        &inputs,
        "host",
        &opened.receipt.result_ref.hold,
        || Ok(NOW + 41),
    )
    .unwrap();
    assert!(matches!(
        view.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    assert_eq!(view.opening, *stale_opening);
    assert_eq!(view.current_hold, answer.data["hold"]);
    assert_eq!(
        view.current_hold["opened_seq"], 0,
        "historical raw Hold is not a new effective projection"
    );
    assert_eq!(
        view.opening.data["hold"]["body"]["plan_content"],
        "恢复前原文\r\n保留末行与来源\n"
    );
    let retried = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &request_id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 41),
    );
    let answer_retry = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 41),
    );
    if keep_original_confirmed {
        let retried = retried.unwrap();
        assert_eq!(retried.receipt, opened.receipt);
        assert!(matches!(
            retried.current_validity,
            ReviewValidity::HistoricalOnly { .. }
        ));
        let retried_answer = answer_retry.unwrap();
        assert_eq!(retried_answer.receipt, answered.receipt);
        assert!(matches!(
            retried_answer.current_validity,
            ReviewValidity::HistoricalOnly { .. }
        ));
        let confirmed = f.outbox.read_inputs().unwrap();
        assert_eq!(
            confirmed
                .confirmed()
                .iter()
                .find(|event| event.eid == opening.eid),
            Some(&opening)
        );
        assert_eq!(
            confirmed
                .confirmed()
                .iter()
                .find(|event| event.eid == answer.eid),
            Some(&answer)
        );
    } else {
        for error in [retried.unwrap_err(), answer_retry.unwrap_err()] {
            assert!(
                error.contains("uncertain") && error.contains("only a stale outer replay"),
                "{error}"
            );
        }
        let pending = f.outbox.read_inputs().unwrap();
        assert!(pending
            .pending()
            .iter()
            .any(|event| event.eid == opening.eid
                && event.data == opening.data
                && event.guards == opening.guards));
        assert!(pending.pending().iter().any(|event| event.eid == answer.eid
            && event.data == answer.data
            && event.guards == answer.guards));
        assert!(!pending
            .confirmed()
            .iter()
            .any(|event| event.eid == opening.eid || event.eid == answer.eid));
    }
    assert_eq!(
        f.outbox.read_inputs().unwrap().digest(),
        digest,
        "retry fsyncs original confirmations without replacing them with replay receipts"
    );
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(cursor, NOW + 41)
            .unwrap(),
        before,
        "historical load/retry must not append, renew, grant or revive the original Run"
    );
    let conflict = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &request_id,
        &plan("同 UUID 异正文"),
        None,
        &mut f.outbox,
        || Ok(NOW + 41),
    )
    .unwrap_err();
    assert!(conflict.contains("request_id reused"), "{conflict}");
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
}

#[test]
fn rebuilt_review_original_receipts_remain_durable_but_only_raw_history_is_readable() {
    rebuild_uncheckpointed_review(true);
}

#[test]
fn rebuilt_review_with_only_new_stale_outer_remains_uncertain_without_original_receipt() {
    rebuild_uncheckpointed_review(false);
}

#[test]
fn app_viewer_worktree_resolves_the_actual_run_without_renewing_or_accepting_client_paths() {
    let mut f = claimed_with_linked_worktree(true);
    let viewer_root = f.runtime.path().join("viewer-worktree");
    git(
        f.root.path(),
        &[
            "worktree",
            "add",
            "-b",
            "review-viewer",
            viewer_root.to_str().unwrap(),
            "main",
        ],
    );
    let viewer = Catalog::load(&viewer_root).unwrap();
    assert_ne!(viewer.root, f.catalog.root);
    assert_eq!(viewer.common_dir, f.catalog.common_dir);
    let command = plan("来自 linked task 工作树的计划\r\n原文\n");
    let request_id = uuid::Uuid::now_v7().to_string();
    let opened = request(&mut f, &request_id, &command);
    let inputs = f.outbox.read_inputs().unwrap();
    let digest = inputs.digest().to_owned();
    let cursor = f.session.lock().unwrap().applied_seq();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 21)
        .unwrap();
    let view = ProjectSession::load_task_review_shared(
        &f.session,
        &viewer,
        &inputs,
        "host",
        &opened.receipt.result_ref.hold,
        || Ok(NOW + 21),
    )
    .unwrap();
    assert_eq!(view.current_validity, ReviewValidity::Current);
    assert_eq!(view.binding, opened.receipt.result_ref.binding);
    assert_eq!(view.current_hold["opened_seq"], view.opening.seq);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(cursor, NOW + 21)
            .unwrap(),
        before,
        "App viewing from another worktree must not renew the Run"
    );
    let cli_error = ProjectSession::request_task_review_shared(
        &f.session,
        &viewer,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        &plan("CLI must stay in task worktree"),
        Some(&opened.receipt.result_ref.binding.run),
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap_err();
    assert!(cli_error.contains("worktree/branch"), "{cli_error}");
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    let action = approve(&opened);
    let answer_id = uuid::Uuid::now_v7().to_string();
    let answered = ProjectSession::answer_task_review_shared(
        &f.session,
        &viewer,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 22),
    )
    .unwrap();
    assert_eq!(answered.current_validity, ReviewValidity::Current);
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&opened.receipt.result_ref.binding)
        .is_ok());
    let tree_request = HumanCommand::ReviewRequest {
        gate: ReviewGate::G4,
        content: None,
        source_commit: None,
        source_path: None,
    };
    let tree = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        &tree_request,
        None,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap();
    let tree_view = ProjectSession::load_task_review_shared(
        &f.session,
        &viewer,
        &f.outbox.read_inputs().unwrap(),
        "host",
        &tree.receipt.result_ref.hold,
        || Ok(NOW + 24),
    )
    .unwrap();
    assert_eq!(tree_view.current_validity, ReviewValidity::Current);
    assert!(matches!(
        tree_view.binding.subject,
        ReviewSubject::Tree { .. }
    ));
    let tree_answer = ProjectSession::answer_task_review_shared(
        &f.session,
        &viewer,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &approve(&tree),
        &mut f.outbox,
        || Ok(NOW + 24),
    )
    .unwrap();
    assert_eq!(tree_answer.current_validity, ReviewValidity::Current);
    // An original request/answer retry still uses its original receipt before
    // attempting to resolve a now missing execution worktree.
    std::fs::remove_dir_all(&f.catalog.root).unwrap();
    let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
    let retried = ProjectSession::request_task_review_shared(
        &f.session,
        &viewer,
        "host",
        &request_id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 25),
    )
    .unwrap();
    assert_eq!(retried.receipt, opened.receipt);
    assert!(matches!(
        retried.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    let retried = ProjectSession::answer_task_review_shared(
        &f.session,
        &viewer,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 25),
    )
    .unwrap();
    assert_eq!(retried.receipt, answered.receipt);
    assert!(matches!(
        retried.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
}

#[test]
fn app_viewer_refuses_changed_missing_foreign_or_wrong_host_run_coordinates() {
    let mut f = claimed_with_linked_worktree(true);
    let viewer_root = f.runtime.path().join("viewer-worktree");
    git(
        f.root.path(),
        &[
            "worktree",
            "add",
            "-b",
            "review-viewer",
            viewer_root.to_str().unwrap(),
            "main",
        ],
    );
    let viewer = Catalog::load(&viewer_root).unwrap();
    let opened = request(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &plan("严格工作树绑定"),
    );
    let action = approve(&opened);
    let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
    let cursor = f.session.lock().unwrap().applied_seq();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 21)
        .unwrap();
    let answer = |f: &mut Fixture, selected: &Catalog, host: &str| {
        ProjectSession::answer_task_review_shared(
            &f.session,
            selected,
            host,
            "human:owner",
            &uuid::Uuid::now_v7().to_string(),
            &action,
            &mut f.outbox,
            || Ok(NOW + 21),
        )
    };
    assert!(answer(&mut f, &viewer, "other-host").is_err());
    git(
        &f.catalog.root,
        &["switch", "-qc", "unexpected-task-branch"],
    );
    assert!(answer(&mut f, &viewer, "host").is_err());
    let historical = ProjectSession::load_task_review_shared(
        &f.session,
        &viewer,
        &f.outbox.read_inputs().unwrap(),
        "host",
        &opened.receipt.result_ref.hold,
        || Ok(NOW + 21),
    )
    .unwrap();
    assert!(matches!(
        historical.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    git(&f.catalog.root, &["switch", "t/T-review"]);
    let foreign_root = f.runtime.path().join("foreign-clone");
    git(
        f.root.path(),
        &[
            "clone",
            "--no-hardlinks",
            f.root.path().to_str().unwrap(),
            foreign_root.to_str().unwrap(),
        ],
    );
    let foreign = Catalog::load(&foreign_root).unwrap();
    assert_eq!(foreign.config.project_id(), viewer.config.project_id());
    assert_ne!(foreign.common_dir, viewer.common_dir);
    assert!(answer(&mut f, &foreign, "host").is_err());
    assert!(ProjectSession::load_task_review_shared(
        &f.session,
        &foreign,
        &f.outbox.read_inputs().unwrap(),
        "host",
        &opened.receipt.result_ref.hold,
        || Ok(NOW + 21)
    )
    .is_err());
    let moved = f.runtime.path().join("moved-task-worktree");
    std::fs::rename(&f.catalog.root, &moved).unwrap();
    assert!(answer(&mut f, &viewer, "host").is_err());
    // Restore the Run's exact path, then replace it with a foreign clone at the
    // same name. Matching project_id alone must never approve that clone.
    std::fs::rename(&moved, &f.catalog.root).unwrap();
    std::fs::remove_dir_all(&f.catalog.root).unwrap();
    std::fs::rename(&foreign_root, &f.catalog.root).unwrap();
    assert!(answer(&mut f, &viewer, "host").is_err());
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(cursor, NOW + 21)
            .unwrap(),
        before,
        "failed coordinates must neither append nor renew any fence"
    );
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&opened.receipt.result_ref.binding)
        .is_err());
}

// A real local Hub accepts the original Task event; only its subsequent
// confirmation write is interrupted. No Review facts or receipts are injected.
#[derive(Clone)]
struct InterruptedTaskConfirmation {
    original: towermd_core::hub_protocol::NewEvent,
    actual: towermd_core::hub_protocol::Event,
    path: std::path::PathBuf,
    backup: std::path::PathBuf,
}
type TaskConfirmationCapture = Arc<std::sync::Mutex<Option<InterruptedTaskConfirmation>>>;
struct InterruptTaskConfirmation {
    inner: towermd_core::hub::Hub,
    kind: String,
    directory: std::path::PathBuf,
    backup: std::path::PathBuf,
    capture: TaskConfirmationCapture,
}
impl towermd_core::hub_protocol::HubProtocol for InterruptTaskConfirmation {
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
            *self.capture.lock().unwrap() = Some(InterruptedTaskConfirmation {
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
fn interrupt_task_confirmation(
    mut f: Fixture,
    kind: &str,
    now: u64,
) -> (Fixture, TaskConfirmationCapture) {
    let capture = Arc::new(std::sync::Mutex::new(None));
    // Release this fixture's actual Hub owner before opening its replacement.
    drop(f.session);
    let protocol = InterruptTaskConfirmation {
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
fn restore_interrupted_task(capture: &TaskConfirmationCapture) -> InterruptedTaskConfirmation {
    let interrupted = capture.lock().unwrap().as_ref().unwrap().clone();
    std::fs::remove_dir(&interrupted.path).unwrap();
    std::fs::rename(&interrupted.backup, &interrupted.path).unwrap();
    interrupted
}
fn add_unrelated_task() -> TaskCommand {
    TaskCommand::Add {
        task: TaskSpec {
            id: "T-other".into(),
            title: "Separate Task request".into(),
            module: "A-alpha".into(),
            touches: vec![],
            allowed_paths: vec!["alpha/**".into()],
            depends_on: vec![],
            acceptance: vec!["AC-review".into()],
            decisions: vec![],
        },
    }
}
fn actual_hub_cursor(f: &Fixture) -> u64 {
    f.session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq()
}
fn assert_no_new_grants(f: &Fixture, after: u64, now: u64) {
    let batch = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(after, now)
        .unwrap();
    assert!(
        batch
            .events
            .iter()
            .all(|event| event.kind != "lease.granted"),
        "repair must not acquire any replacement lease: {:?}",
        batch.events
    );
}

#[test]
fn new_review_requests_repair_exact_pending_task_receipts_before_run_resolution() {
    let mut f = claimed_with_linked_worktree(true);
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 12,
        &mut f.outbox,
    )
    .unwrap();
    let (mut f, capture) = interrupt_task_confirmation(f, "task.claimed", NOW + 13);
    assert!(ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-review".into(),
            harness: None,
            harness_version: None
        },
        None,
        NOW + 13,
        &mut f.outbox
    )
    .is_err());
    let original_claim = restore_interrupted_task(&capture);
    assert!(f.session.lock().unwrap().pending_claim().unwrap().is_some());
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.pending().contains(&original_claim.original));
    assert!(!inputs
        .confirmed()
        .iter()
        .any(|event| event.eid == original_claim.actual.eid));
    // Include grants already accepted before the confirmation fault, even
    // when the Session has not yet installed that actual Hub prefix.
    let before_review = actual_hub_cursor(&f);
    let opened = request(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &plan("协调原claim之后开立"),
    );
    assert_eq!(opened.current_validity, ReviewValidity::Current);
    assert_eq!(
        opened.receipt.result_ref.binding.run,
        original_claim.original.data["run"]["id"].as_str().unwrap()
    );
    assert!(f.session.lock().unwrap().pending_claim().unwrap().is_none());
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|event| event.eid == original_claim.actual.eid),
        Some(&original_claim.actual)
    );
    assert_no_new_grants(&f, before_review, NOW + 20);

    let (mut f, capture) = interrupt_task_confirmation(f, "task.state", NOW + 21);
    assert!(ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        add_unrelated_task(),
        None,
        NOW + 21,
        &mut f.outbox
    )
    .is_err());
    let original_state = restore_interrupted_task(&capture);
    let viewer_root = f.runtime.path().join("review-maintenance-viewer");
    git(
        f.root.path(),
        &[
            "worktree",
            "add",
            "--detach",
            viewer_root.to_str().unwrap(),
            "main",
        ],
    );
    let viewer = Catalog::load(&viewer_root).unwrap();
    // Include grants already accepted before the confirmation fault, even
    // when the Session has not yet installed that actual Hub prefix.
    let before_review = actual_hub_cursor(&f);
    let answer = ProjectSession::answer_task_review_shared(
        &f.session,
        &viewer,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &approve(&opened),
        &mut f.outbox,
        || Ok(NOW + 22),
    )
    .unwrap();
    assert_eq!(answer.current_validity, ReviewValidity::Current);
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|event| event.eid == original_state.actual.eid),
        Some(&original_state.actual)
    );
    assert_no_new_grants(&f, before_review, NOW + 22);
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&opened.receipt.result_ref.binding)
        .is_ok());
}

#[test]
fn historical_review_retries_and_identity_conflicts_do_not_coordinate_another_task_request() {
    let mut f = claimed_with_linked_worktree(true);
    let command = plan("历史原件优先于其他Task维护");
    let request_id = uuid::Uuid::now_v7().to_string();
    let opened = request(&mut f, &request_id, &command);
    let action = approve(&opened);
    let answer_id = uuid::Uuid::now_v7().to_string();
    let answered = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 22,
        &mut f.outbox,
    )
    .unwrap();
    let (mut f, capture) = interrupt_task_confirmation(f, "task.state", NOW + 23);
    assert!(ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        add_unrelated_task(),
        None,
        NOW + 23,
        &mut f.outbox
    )
    .is_err());
    let unrelated = restore_interrupted_task(&capture);
    let viewer_root = f.runtime.path().join("historical-review-viewer");
    git(
        f.root.path(),
        &[
            "worktree",
            "add",
            "--detach",
            viewer_root.to_str().unwrap(),
            "main",
        ],
    );
    let viewer = Catalog::load(&viewer_root).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let digest = inputs.digest().to_owned();
    assert!(
        ProjectSession::task_reconciliation_needed_shared(&f.session, &viewer, &inputs).unwrap()
    );
    let cursor = f.session.lock().unwrap().applied_seq();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 24)
        .unwrap();
    let retry = ProjectSession::request_task_review_shared(
        &f.session,
        &viewer,
        "host",
        &request_id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 24),
    )
    .unwrap();
    assert_eq!(retry.receipt, opened.receipt);
    assert!(matches!(
        retry.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    let retry = ProjectSession::answer_task_review_shared(
        &f.session,
        &viewer,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 24),
    )
    .unwrap();
    assert_eq!(retry.receipt, answered.receipt);
    assert!(matches!(
        retry.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    let error = ProjectSession::request_task_review_shared(
        &f.session,
        &viewer,
        "host",
        &request_id,
        &plan("同UUID异正文"),
        None,
        &mut f.outbox,
        || Ok(NOW + 24),
    )
    .unwrap_err();
    assert!(error.contains("request_id reused"), "{error}");
    let after = f.outbox.read_inputs().unwrap();
    assert_eq!(after.digest(), digest);
    assert!(after.pending().contains(&unrelated.original));
    assert!(!after
        .confirmed()
        .iter()
        .any(|event| event.eid == unrelated.actual.eid));
    assert!(
        ProjectSession::task_reconciliation_needed_shared(&f.session, &viewer, &after).unwrap()
    );
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(cursor, NOW + 24)
            .unwrap(),
        before,
        "an old Review retry must not coordinate, append, renew or acquire another Task request"
    );
}

#[test]
fn accepted_review_receipt_source_failures_remain_uncertain_and_retry_the_same_original() {
    use std::cell::Cell;
    let mut f = claimed();
    let opened = request(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &plan("原接受计划"),
    );
    let action = approve(&opened);
    let request_id = uuid::Uuid::now_v7().to_string();
    let answered = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &request_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
    let original = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| event.eid == answered.receipt.result_ref.fact.answer_eid)
        .unwrap()
        .clone();
    let host_file = std::fs::read_dir(f.runtime.path().join("outbox"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .unwrap();
    let original_bytes = std::fs::read(&host_file).unwrap();
    let original_main = git(f.root.path(), &["rev-parse", "refs/heads/main"]);
    let tree = git(
        f.root.path(),
        &["rev-parse", &format!("{original_main}^{{tree}}")],
    );
    let advanced_main = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &original_main,
            "-m",
            "actual temporary authority change for request proof",
        ],
    );
    for after_receipt in [false, true] {
        let actual_cursor = actual_hub_cursor(&f);
        let before = f
            .session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(actual_cursor, NOW + 22)
            .unwrap();
        if !after_receipt {
            git(
                f.root.path(),
                &[
                    "update-ref",
                    "refs/heads/main",
                    &advanced_main,
                    &original_main,
                ],
            );
        }
        let calls = Cell::new(0);
        let error = ProjectSession::answer_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            "human:owner",
            &request_id,
            &action,
            &mut f.outbox,
            || {
                calls.set(calls.get() + 1);
                // First clock enters original retry; the second starts the
                // current-validity read after that exact Event was recovered.
                if after_receipt && calls.get() == 2 {
                    git(
                        f.root.path(),
                        &[
                            "update-ref",
                            "refs/heads/main",
                            &advanced_main,
                            &original_main,
                        ],
                    );
                }
                Ok(NOW + 22)
            },
        )
        .unwrap_err();
        assert!(
            error.contains("uncertain") && error.contains(&request_id),
            "after_receipt={after_receipt} calls={}: {error}",
            calls.get()
        );
        assert!(
            error.contains("authority") || error.contains("sources changed"),
            "{error}"
        );
        assert_eq!(std::fs::read(&host_file).unwrap(), original_bytes);
        assert_eq!(
            f.session
                .lock()
                .unwrap()
                .protocol()
                .unwrap()
                .subscribe(actual_cursor, NOW + 22)
                .unwrap(),
            before,
            "source failure cannot append, renew or mint a replacement receipt"
        );
        git(
            f.root.path(),
            &[
                "update-ref",
                "refs/heads/main",
                &original_main,
                &advanced_main,
            ],
        );
        let restored = ProjectSession::answer_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            "human:owner",
            &request_id,
            &action,
            &mut f.outbox,
            || Ok(NOW + 22),
        )
        .unwrap();
        assert_eq!(restored.receipt, answered.receipt);
        assert_eq!(
            f.outbox
                .read_inputs()
                .unwrap()
                .confirmed()
                .iter()
                .find(|event| event.eid == original.eid),
            Some(&original)
        );
        assert_eq!(std::fs::read(&host_file).unwrap(), original_bytes);
        assert_eq!(
            f.session
                .lock()
                .unwrap()
                .protocol()
                .unwrap()
                .subscribe(actual_cursor, NOW + 22)
                .unwrap(),
            before
        );
    }
    let actual_cursor = actual_hub_cursor(&f);
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(actual_cursor, NOW + 23)
        .unwrap();
    std::fs::write(&host_file, b"{invalid actual outbox record}\n").unwrap();
    let error = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &request_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap_err();
    assert!(
        error.contains("uncertain") && error.contains(&request_id),
        "{error}"
    );
    assert_eq!(
        std::fs::read(&host_file).unwrap(),
        b"{invalid actual outbox record}\n"
    );
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(actual_cursor, NOW + 23)
            .unwrap(),
        before
    );
    std::fs::write(&host_file, &original_bytes).unwrap();
    let restored = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &request_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap();
    assert_eq!(restored.receipt, answered.receipt);
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|event| event.eid == original.eid),
        Some(&original)
    );
    assert_eq!(std::fs::read(&host_file).unwrap(), original_bytes);
    let TaskReviewAction::Review {
        hold,
        binding,
        verdict,
        ..
    } = action;
    let conflict = TaskReviewAction::Review {
        hold,
        binding,
        verdict,
        comment: "同原UUID异答复".into(),
    };
    let error = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &request_id,
        &conflict,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap_err();
    assert!(
        error.contains("request_id reused") && !error.contains("uncertain"),
        "{error}"
    );
    assert_eq!(std::fs::read(&host_file).unwrap(), original_bytes);

    // A fresh request that loses its source proof before append also retains
    // its UUID, while the stable G4 gate refusals above remain plain refusals.
    let new_id = uuid::Uuid::now_v7().to_string();
    let command = plan("新请求准备期间源变化");
    let actual_cursor = actual_hub_cursor(&f);
    let calls = Cell::new(0);
    let error = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &new_id,
        &command,
        None,
        &mut f.outbox,
        || {
            calls.set(calls.get() + 1);
            // Original lookup, renewal, then preparation's actual proof.
            if calls.get() == 3 {
                git(
                    f.root.path(),
                    &[
                        "update-ref",
                        "refs/heads/main",
                        &advanced_main,
                        &original_main,
                    ],
                );
            }
            Ok(NOW + 24)
        },
    )
    .unwrap_err();
    assert!(
        error.contains("uncertain") && error.contains(&new_id),
        "{error}"
    );
    assert_eq!(std::fs::read(&host_file).unwrap(), original_bytes);
    let changes = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(actual_cursor, NOW + 24)
        .unwrap();
    assert!(changes
        .events
        .iter()
        .all(|event| !event.kind.starts_with("hold.") && event.kind != "lease.granted"));
    git(
        f.root.path(),
        &[
            "update-ref",
            "refs/heads/main",
            &original_main,
            &advanced_main,
        ],
    );
    let restored = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &new_id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 24),
    )
    .unwrap();
    assert_eq!(restored.receipt.request_id, new_id);
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.data["p3_request"]["request_id"] == new_id)
            .count(),
        1
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .find(|event| event.eid == original.eid),
        Some(&original)
    );
}

#[test]
fn g4_not_latest_main_is_refused_only_after_the_same_source_and_head_stay_stable() {
    use std::cell::Cell;
    let mut f = claimed();
    let task_head = git(f.root.path(), &["rev-parse", "HEAD^{commit}"]);
    let tree = git(f.root.path(), &["rev-parse", "HEAD^{tree}"]);
    let main = git(f.root.path(), &["rev-parse", "refs/heads/main"]);
    let advanced = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &main,
            "-m",
            "main ahead of task",
        ],
    );
    let newer_main = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &advanced,
            "-m",
            "main changed during G4 refusal",
        ],
    );
    let other_head = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &task_head,
            "-m",
            "HEAD changed during G4 refusal",
        ],
    );
    git(
        f.root.path(),
        &["update-ref", "refs/heads/main", &advanced, &main],
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let command = HumanCommand::ReviewRequest {
        gate: ReviewGate::G4,
        content: None,
        source_commit: None,
        source_path: None,
    };
    for change in ["none", "head", "authority"] {
        let id = uuid::Uuid::now_v7().to_string();
        let inputs = f.outbox.read_inputs().unwrap();
        let digest = inputs.digest().to_owned();
        let cursor = actual_hub_cursor(&f);
        let calls = Cell::new(0);
        let error = ProjectSession::request_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &command,
            None,
            &mut f.outbox,
            || {
                calls.set(calls.get() + 1);
                // The actual check has already rejected a real behind-main
                // tree. Move a real ref at its final refusal-proof boundary.
                if calls.get() == 5 {
                    match change {
                        "head" => {
                            git(
                                f.root.path(),
                                &[
                                    "update-ref",
                                    "refs/heads/t/T-review",
                                    &other_head,
                                    &task_head,
                                ],
                            );
                        }
                        "authority" => {
                            git(
                                f.root.path(),
                                &["update-ref", "refs/heads/main", &newer_main, &advanced],
                            );
                        }
                        _ => {}
                    }
                }
                Ok(NOW + 20)
            },
        )
        .unwrap_err();
        assert!(
            calls.get() >= 5,
            "actual NotLatestMain refusal proof must run: {error}"
        );
        if change == "none" {
            assert!(
                error.contains("NotLatestMain") && !error.contains("uncertain"),
                "{error}"
            );
        } else {
            assert!(
                error.contains("uncertain") && error.contains(&id),
                "{change}: {error}"
            );
            if change == "head" {
                assert!(error.contains("HEAD changed"), "{error}");
                git(
                    f.root.path(),
                    &[
                        "update-ref",
                        "refs/heads/t/T-review",
                        &task_head,
                        &other_head,
                    ],
                );
            } else {
                git(
                    f.root.path(),
                    &["update-ref", "refs/heads/main", &advanced, &newer_main],
                );
            }
            let stable = ProjectSession::request_task_review_shared(
                &f.session,
                &f.catalog,
                "host",
                &id,
                &command,
                None,
                &mut f.outbox,
                || Ok(NOW + 20),
            )
            .unwrap_err();
            assert!(
                stable.contains("NotLatestMain") && !stable.contains("uncertain"),
                "{stable}"
            );
        }
        assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
        let added = f
            .session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(cursor, NOW + 20)
            .unwrap();
        assert!(added
            .events
            .iter()
            .all(|event| !event.kind.starts_with("hold.") && event.kind != "lease.granted"));
    }
}

#[test]
fn final_review_clock_regression_is_uncertain_without_appending_a_new_fact() {
    use std::cell::Cell;
    let mut f = claimed();
    let command = plan("最终append前的时钟证明");
    let id = uuid::Uuid::now_v7().to_string();
    let cursor = actual_hub_cursor(&f);
    let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
    let calls = Cell::new(0);
    let error = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &command,
        None,
        &mut f.outbox,
        || {
            calls.set(calls.get() + 1);
            Ok(if calls.get() == 5 { NOW + 19 } else { NOW + 20 })
        },
    )
    .unwrap_err();
    assert_eq!(calls.get(), 5);
    assert!(
        error.contains("uncertain")
            && error.contains(&id)
            && error.contains("clock moved backwards before Review append"),
        "{error}"
    );
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    let added = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 20)
        .unwrap();
    assert!(added
        .events
        .iter()
        .all(|event| !event.kind.starts_with("hold.") && event.kind != "lease.granted"));
    let opened = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 20),
    )
    .unwrap();
    assert_eq!(opened.receipt.request_id, id);
    let answer_id = uuid::Uuid::now_v7().to_string();
    let action = approve(&opened);
    let cursor = actual_hub_cursor(&f);
    let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
    let calls = Cell::new(0);
    let error = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || {
            calls.set(calls.get() + 1);
            Ok(if calls.get() == 7 { NOW + 19 } else { NOW + 21 })
        },
    )
    .unwrap_err();
    assert_eq!(calls.get(), 7);
    assert!(
        error.contains("uncertain")
            && error.contains(&answer_id)
            && error.contains("clock moved backwards before Review append"),
        "{error}"
    );
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(cursor, NOW + 21)
            .unwrap()
            .events,
        vec![]
    );
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&opened.receipt.result_ref.binding)
        .is_err());
    let answered = ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &answer_id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
    assert_eq!(answered.receipt.request_id, answer_id);
    assert_eq!(answered.current_validity, ReviewValidity::Current);
    let inputs = f.outbox.read_inputs().unwrap();
    for request_id in [&id, &answer_id] {
        assert_eq!(
            inputs
                .confirmed()
                .iter()
                .filter(|event| event.data["p3_request"]["request_id"] == request_id.as_str())
                .count(),
            1
        );
    }
}
