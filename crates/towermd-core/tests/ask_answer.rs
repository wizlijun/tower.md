//! Real B0/Task/Ask producers. These core tests are not AppGate, GUI, or
//! daemon/native acceptance evidence.
#[path = "support/baseline_review.rs"]
mod baseline_fixture;
#[path = "support/ask.rs"]
mod fixture;
use fixture::*;
use std::sync::Arc;
use towermd_core::{
    ask_answer_contract::{
        AppAnswerAction, AppAnswerRequest, AskAnswerValidity, AskAnswerView, AskOpeningBinding,
    },
    catalog::Catalog,
    human_intervention::{AskState, HumanCommand},
    project_session::ProjectSession,
    recovery, snapshot_service,
};
fn load(f: &Fixture, hold: &str, now: u64) -> AskAnswerView {
    ProjectSession::load_ask_intervention_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        hold,
        || Ok(now),
    )
    .unwrap()
}
fn answer(
    f: &mut Fixture,
    id: &str,
    action: &AppAnswerAction,
    now: u64,
) -> Result<towermd_core::ask_answer_contract::AskAnswerOutcome, String> {
    ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        id,
        action,
        &mut f.outbox,
        || Ok(now),
    )
}
fn action(view: &AskAnswerView, text: &str) -> AppAnswerAction {
    AppAnswerAction::Answer {
        hold: view.current_hold["id"].as_str().unwrap().into(),
        opening: view.binding.clone(),
        text: text.into(),
    }
}
#[test]
fn actual_current_app_answer_wait_and_original_retry_preserve_full_receipt() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("App本次回答"),
        NOW + 20,
    );
    let wait_id = uuid::Uuid::now_v7().to_string();
    let wait = HumanCommand::AskWait {
        hold: opened.hold.clone(),
    };
    let started = call(&mut f, &wait_id, &wait, NOW + 21);
    assert!(started.receipt.is_none());
    let before = events(&f);
    let view = load(&f, &opened.hold, NOW + 22);
    assert_eq!(view.current_validity, AskAnswerValidity::Current);
    assert_eq!(
        view.binding,
        AskOpeningBinding::from_opening(&view.opening).unwrap()
    );
    assert_eq!(events(&f), before, "view never renews or writes");
    assert!(view.recipients.iter().all(|r| r.actionable));
    let id = uuid::Uuid::now_v7().to_string();
    let act = action(&view, "保留原文\r\n仅本次答复");
    let receipt = answer(&mut f, &id, &act, NOW + 30).unwrap();
    assert_eq!(receipt.receipt.result_ref.reply.actor, "human:owner");
    assert_eq!(receipt.receipt.result_ref.reply.relayed_from, None);
    assert_eq!(receipt.receipt.result_ref.reply.at_ms, NOW + 30);
    let read = load(&f, &opened.hold, NOW + 31);
    let mut expected = view.current_hold.clone();
    expected["reply"] = serde_json::to_value(&receipt.receipt.result_ref.reply).unwrap();
    expected["waits"][0]["finished_ms"] = serde_json::json!(NOW + 30);
    assert_eq!(read.current_hold, expected);
    assert!(read.actions.is_empty());
    let finished = call(&mut f, &wait_id, &wait, NOW + 32);
    assert_eq!(finished.current.state, AskState::Answered);
    assert_eq!(
        finished.current.reply,
        Some(receipt.receipt.result_ref.reply.clone())
    );
    let actual = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.answered")
        .unwrap();
    let before_retry = events(&f);
    assert_eq!(answer(&mut f, &id, &act, NOW + 33).unwrap(), receipt);
    assert_eq!(events(&f), before_retry, "retry only confirms original");
    assert_eq!(
        events(&f).iter().find(|e| e.eid == actual.eid),
        Some(&actual)
    );
    assert!(answer(&mut f, &id, &action(&view, "changed"), NOW + 34)
        .unwrap_err()
        .contains("reused"));
    assert!(
        answer(&mut f, &uuid::Uuid::now_v7().to_string(), &act, NOW + 34)
            .unwrap_err()
            .contains("already answered")
    );
    assert_eq!(events(&f), before_retry);
}
#[test]
fn current_expired_recipient_answer_records_without_renewal_or_old_run_revival() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("过期仍保存本次原问题答复"),
        NOW + 20,
    );
    let view = load(&f, &opened.hold, NOW + 21);
    let old_run = view.recipients[0].recipient.run.clone();
    let now = NOW + 1_000_000;
    let expired = load(&f, &opened.hold, now);
    assert_eq!(expired.current_validity, AskAnswerValidity::Current);
    assert!(expired.recipients.iter().all(|r| !r.actionable));
    let cursor = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let outcome = answer(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &action(&expired, "仅归档"),
        now + 1,
    )
    .unwrap();
    assert_eq!(outcome.current_validity, AskAnswerValidity::Current);
    let batch = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, now + 2)
        .unwrap();
    assert!(!batch.events.iter().any(|e| matches!(
        e.kind.as_str(),
        "lease.granted" | "lease.renewed" | "task.claimed"
    )));
    assert!(batch
        .head
        .leases
        .values()
        .all(|l| l.holder != old_run || l.expires_at <= now));
    let after = load(&f, &opened.hold, now + 2);
    assert!(after.recipients.iter().all(|r| !r.actionable));
    assert_eq!(after.current_hold["reply"]["text"], "仅归档");
}
#[test]
fn app_viewer_worktree_is_only_project_selection_and_cannot_supply_arbitrary_binding() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("viewer项目选择"),
        NOW + 20,
    );
    let view = load(&f, &opened.hold, NOW + 21);
    let viewer = f
        .root
        .path()
        .parent()
        .unwrap()
        .join(format!("viewer-{}", uuid::Uuid::now_v7()));
    git(
        f.root.path(),
        &[
            "worktree",
            "add",
            "-qb",
            "viewer-answer",
            viewer.to_str().unwrap(),
            "main",
        ],
    );
    let catalog = Catalog::load(&viewer).unwrap();
    let read = ProjectSession::load_ask_intervention_shared(
        &f.session,
        &catalog,
        &f.outbox.read_inputs().unwrap(),
        &opened.hold,
        || Ok(NOW + 22),
    )
    .unwrap();
    assert_eq!(read, view);
    let mut bad = action(&read, "bad binding");
    let AppAnswerAction::Answer { opening, .. } = &mut bad;
    opening.sha256 = "0".repeat(64);
    let before = events(&f);
    assert!(ProjectSession::answer_ask_shared(
        &f.session,
        &catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &bad,
        &mut f.outbox,
        || Ok(NOW + 23)
    )
    .is_err());
    assert_eq!(events(&f), before);
    for actor in ["process:towermd-cli", "human:", "human:a:b"] {
        assert!(AppAnswerRequest::prepare(
            catalog.config.project_id(),
            "host",
            actor,
            &uuid::Uuid::now_v7().to_string(),
            &action(&read, "answer")
        )
        .is_err());
    }
    ProjectSession::answer_ask_shared(
        &f.session,
        &catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &action(&read, "viewer实际答复"),
        &mut f.outbox,
        || Ok(NOW + 24),
    )
    .unwrap();
    assert_eq!(
        load(&f, &opened.hold, NOW + 25).current_hold["reply"]["text"],
        "viewer实际答复"
    );
    git(
        f.root.path(),
        &["worktree", "remove", viewer.to_str().unwrap()],
    );
}
fn rebuild_owned(mut f: Fixture, now: u64) -> Fixture {
    drop(f.session);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let prepared = recovery::prepare(
        &f.catalog,
        &inputs,
        now,
        now + 300_000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (report, session) = f
        .writer
        .confirm_rebuild(
            None,
            &f.catalog,
            &prepared,
            "human:owner",
            prepared.expires_at,
            &inputs,
            "host",
            || Ok(now + 1),
        )
        .unwrap();
    assert_eq!(report.status, "rebuilt");
    f.session = session;
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    f
}

#[test]
fn task_progress_actual_rebuild_does_not_rediscover_checkpointed_ask_as_current() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("保存在检查点的未答问题不能在重建后重新打扰"),
        NOW + 20,
    );
    let before = ProjectSession::read_task_progress(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        NOW + 21,
        Some(&f.session),
    )
    .unwrap();
    assert!(before.holds.iter().any(|hold| hold["id"] == opened.hold));
    let original = load(&f, &opened.hold, NOW + 22).opening;
    f.writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 25),
            &mut f.outbox,
        )
        .unwrap();
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert!(snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap()
        .snapshot
        .projection
        .objects["Hold"]
        .contains_key(&opened.hold));
    let f = rebuild_owned(f, NOW + 30);
    let journal = f.catalog.common_dir.join("towermd/journal.jsonl");
    let bytes = std::fs::read(&journal).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let after = ProjectSession::read_task_progress(&f.catalog, &inputs, NOW + 32, Some(&f.session))
        .unwrap();
    assert_eq!(
        after.state,
        towermd_core::app_read_model::TaskProgressState::Recorded
    );
    assert!(!after.holds.iter().any(|hold| hold["id"] == opened.hold));
    assert_eq!(std::fs::read(journal).unwrap(), bytes);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), inputs.digest());
    let historical = load(&f, &opened.hold, NOW + 33);
    assert!(matches!(
        historical.current_validity,
        AskAnswerValidity::HistoricalOnly { .. }
    ));
    assert_eq!(historical.opening.eid, original.eid);
    assert!(
        historical.actions.is_empty(),
        "discovery cannot grant a new late-answer path"
    );
}
#[test]
fn accepted_app_answer_before_checkpoint_survives_two_actual_rebuilds_without_new_answer_authority()
{
    let mut f = claimed();
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
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("两次重建后的原问答"),
        NOW + 20,
    );
    let view = load(&f, &opened.hold, NOW + 21);
    let id = uuid::Uuid::now_v7().to_string();
    let act = action(&view, "实际已接受原答复，不恢复旧运行");
    let outcome = answer(&mut f, &id, &act, NOW + 22).unwrap();
    let original = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.answered")
        .unwrap();
    assert!(original.seq > source.snapshot.through_seq);
    for pass in 0..2 {
        f = rebuild_owned(f, NOW + 30 + pass * 10);
        let history = load(&f, &opened.hold, NOW + 32 + pass * 10);
        assert!(matches!(
            history.current_validity,
            AskAnswerValidity::HistoricalOnly { .. }
        ));
        assert!(history.actions.is_empty() && history.recipients.iter().all(|r| !r.actionable));
        assert_eq!(
            history.current_hold["reply"],
            original.data["hold"]["reply"]
        );
        assert_eq!(history.binding, view.binding);
        assert!(!f
            .session
            .lock()
            .unwrap()
            .human()
            .unwrap()
            .holds
            .contains_key(&opened.hold));
        let cursor = f
            .session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq();
        let retry = answer(&mut f, &id, &act, NOW + 33 + pass * 10).unwrap();
        assert_eq!(retry.receipt, outcome.receipt);
        assert!(matches!(
            retry.current_validity,
            AskAnswerValidity::HistoricalOnly { .. }
        ));
        assert!(answer(
            &mut f,
            &uuid::Uuid::now_v7().to_string(),
            &act,
            NOW + 33 + pass * 10
        )
        .unwrap_err()
        .contains("historical"));
        let after = f
            .session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(cursor, NOW + 34 + pass * 10)
            .unwrap();
        assert!(
            after.events.is_empty(),
            "history/retry never appends or grants"
        );
        let inputs = f.outbox.read_inputs().unwrap();
        assert_eq!(
            inputs.confirmed().iter().find(|e| e.eid == original.eid),
            Some(&original)
        );
        assert!(after.head.leases.values().all(|l| !history
            .recipients
            .iter()
            .any(|r| r.recipient.run == l.holder)));
        if pass == 0 {
            // The persisted selected checkpoint/audit archive must be sufficient
            // when the original outbox is deleted before the second rebuild.
            drop(inputs);
            let runtime = f.runtime.path().join("outbox");
            drop(f.outbox);
            std::fs::remove_dir_all(&runtime).unwrap();
            f.outbox = towermd_core::recovery_outbox::RecoveryOutbox::open(
                &runtime,
                towermd_core::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
            )
            .unwrap();
        }
    }
}
