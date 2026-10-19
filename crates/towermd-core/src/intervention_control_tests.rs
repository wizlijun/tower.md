//! Real B0, Task and Run producers, durable controls and actual rebuilds.
use super::*;
use crate::catalog::Catalog;
use crate::{
    ask_fixture::*,
    cli_delivery::InboxActivity,
    project_session::ProjectSession,
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
    tasks::TaskCommand,
};
use std::sync::Arc;
fn control(
    f: &mut Fixture,
    id: &str,
    actor: &str,
    action: ControlAction,
    now: u64,
) -> Result<ControlOutcome, String> {
    ProjectSession::control_shared(
        &f.session,
        &f.catalog,
        "host",
        actor,
        id,
        action,
        &mut f.outbox,
        || Ok(now),
    )
}
fn view(f: &Fixture, now: u64) -> ControlView {
    ProjectSession::load_interventions_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        || Ok(now),
    )
    .unwrap()
}
fn inbox(f: &mut Fixture, now: u64) -> crate::cli_delivery::InboxBatch {
    ProjectSession::inbox_shared(
        &f.session,
        &f.catalog,
        "host",
        None,
        InboxActivity::Recovery,
        &mut f.outbox,
        || Ok(now),
    )
    .unwrap()
}
fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}
#[test]
fn exact_module_prompt_freezes_real_recipients_and_printed_delivery_is_not_processed() {
    let mut f = claimed();
    let before = f.session.lock().unwrap().last_seq();
    let action = ControlAction::Interject {
        scope: Scope::Module {
            module: "A-alpha".into(),
        },
        text: "保留原文\r\n请先核对接口".into(),
    };
    let request = id();
    let saved = control(&mut f, &request, "human:owner", action.clone(), NOW + 20).unwrap();
    assert_eq!(saved.receipt.result_ref.runs.len(), 1);
    let batch = inbox(&mut f, NOW + 21);
    assert_eq!(batch.messages().len(), 1);
    assert_eq!(batch.messages()[0].text, "保留原文\r\n请先核对接口");
    assert_eq!(batch.messages()[0].kind, "prompt");
    let cursor = f.session.lock().unwrap().last_seq();
    let read = view(&f, NOW + 22);
    assert_eq!(
        f.session.lock().unwrap().last_seq(),
        cursor,
        "App GET never writes, delivers or renews"
    );
    assert!(!read.messages[0].recipients[0].delivered);
    let delivery = id();
    let ids = vec![batch.messages()[0].id.clone()];
    ProjectSession::acknowledge_inbox_shared(
        &f.session,
        &f.catalog,
        "host",
        &batch,
        &delivery,
        &ids,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap();
    assert!(inbox(&mut f, NOW + 24).messages().is_empty());
    assert!(view(&f, NOW + 24).messages[0].recipients[0].delivered);
    assert_eq!(
        control(&mut f, &request, "human:owner", action, NOW + 25).unwrap(),
        saved
    );
    let tail = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before, NOW + 26)
        .unwrap()
        .events;
    assert_eq!(
        tail.iter()
            .filter(|e| e.kind == "hold.opened" && is_control(&e.kind, &e.data))
            .count(),
        1
    );
    let prompts: Vec<_> = tail.iter().filter(|e| e.kind == "human.prompt").collect();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].actor, "human:owner");
    assert_eq!(prompts[0].data["text"], "保留原文\r\n请先核对接口");
    assert_eq!(
        prompts[0].data["scope"],
        serde_json::json!({"kind":"module","module":"A-alpha"})
    );
    assert_eq!(prompts[0].data["produced_action"]["action"], "interject");
    assert!(tail
        .iter()
        .all(|e| !matches!(e.kind.as_str(), "merge.done" | "baseline.created")));
    assert!(control(
        &mut f,
        &id(),
        "process:towermd-cli",
        ControlAction::Interject {
            scope: Scope::Project,
            text: "cannot impersonate human".into()
        },
        NOW + 27
    )
    .is_err());
}
#[test]
fn empty_task_scope_records_zero_recipients_and_never_broadcasts_to_new_runs() {
    let mut f = claimed();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 20,
        &mut f.outbox,
    )
    .unwrap();
    let prompt = control(
        &mut f,
        &id(),
        "human:owner",
        ControlAction::Interject {
            scope: Scope::Task {
                task: "T-ask".into(),
            },
            text: "old empty scope".into(),
        },
        NOW + 21,
    )
    .unwrap();
    assert!(prompt.receipt.result_ref.runs.is_empty());
    assert!(view(&f, NOW + 22).messages[0].recipients.is_empty());
    claim(&mut f, NOW + 23);
    assert!(inbox(&mut f, NOW + 24).messages().is_empty());
}
#[test]
fn pause_blocks_actual_claim_and_process_resume_requires_exact_app_confirmation() {
    let mut f = claimed();
    let pause_id = id();
    control(
        &mut f,
        &pause_id,
        "process:towermd-cli",
        ControlAction::PauseAll,
        NOW + 20,
    )
    .unwrap();
    assert!(view(&f, NOW + 21).paused);
    assert_eq!(inbox(&mut f, NOW + 21).messages()[0].kind, "pause");
    let result = ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-ask".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 22,
        &mut f.outbox,
    );
    assert!(result.unwrap_err().message.contains("project paused"));
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Status,
        None,
        NOW + 23,
        &mut f.outbox,
    )
    .unwrap();
    let requested = control(
        &mut f,
        &id(),
        "process:towermd-cli",
        ControlAction::ResumeAll,
        NOW + 24,
    )
    .unwrap();
    assert!(view(&f, NOW + 25).paused);
    let binding = view(&f, NOW + 25).resume_request.unwrap();
    assert_eq!(binding.request, requested.receipt.result_ref.hold);
    let action = ControlAction::ConfirmResume {
        request: binding.request.clone(),
        pause_revision: binding.pause_revision.clone(),
        runs: binding.runs.clone(),
    };
    assert!(control(
        &mut f,
        &id(),
        "process:towermd-cli",
        action.clone(),
        NOW + 26
    )
    .is_err());
    let bad = ControlAction::ConfirmResume {
        request: binding.request.clone(),
        pause_revision: binding.pause_revision.clone(),
        runs: vec![],
    };
    assert!(control(&mut f, &id(), "human:owner", bad, NOW + 26).is_err());
    assert!(view(&f, NOW + 27).paused);
    let confirmed = control(&mut f, &id(), "human:owner", action, NOW + 28).unwrap();
    assert!(!confirmed.historical_only);
    assert!(!view(&f, NOW + 29).paused);
    let messages = inbox(&mut f, NOW + 30);
    assert!(messages.messages().iter().any(|m| m.kind == "resume"));
    assert!(messages
        .messages()
        .iter()
        .filter(|m| m.kind == "pause")
        .all(|m| !m.actionable));
    assert!(
        f.session.lock().unwrap().projection().unwrap().objects["Run"]
            .values()
            .all(|run| run["state"] != "ended"),
        "App controls do not kill/finish harness runs"
    );
}
#[test]
fn changed_resume_scope_stays_paused_until_a_new_explicit_request_and_confirmation() {
    let mut f = claimed();
    control(
        &mut f,
        &id(),
        "human:owner",
        ControlAction::PauseAll,
        NOW + 20,
    )
    .unwrap();
    control(
        &mut f,
        &id(),
        "process:towermd-cli",
        ControlAction::ResumeAll,
        NOW + 21,
    )
    .unwrap();
    let old = view(&f, NOW + 22).resume_request.unwrap();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 23,
        &mut f.outbox,
    )
    .unwrap();
    assert!(view(&f, NOW + 24).resume_request.is_none());
    let stale = ControlAction::ConfirmResume {
        request: old.request,
        pause_revision: old.pause_revision,
        runs: old.runs,
    };
    assert!(control(&mut f, &id(), "human:owner", stale, NOW + 25).is_err());
    assert!(view(&f, NOW + 26).paused);
    control(
        &mut f,
        &id(),
        "process:towermd-cli",
        ControlAction::ResumeAll,
        NOW + 27,
    )
    .unwrap();
    let current = view(&f, NOW + 28).resume_request.unwrap();
    assert!(current.runs.is_empty());
    control(
        &mut f,
        &id(),
        "human:owner",
        ControlAction::ConfirmResume {
            request: current.request,
            pause_revision: current.pause_revision,
            runs: current.runs,
        },
        NOW + 29,
    )
    .unwrap();
    assert!(!view(&f, NOW + 30).paused);
    claim(&mut f, NOW + 31);
}
#[test]
fn lost_control_confirmation_repairs_original_uuid_before_current_scope_and_never_reissues_pause() {
    let mut f = claimed();
    let request = id();
    let before = f.session.lock().unwrap().last_seq();
    f.outbox.fail_confirmation_after(1);
    assert!(control(
        &mut f,
        &request,
        "human:owner",
        ControlAction::PauseAll,
        NOW + 20
    )
    .unwrap_err()
    .contains("uncertain"));
    let original = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before, NOW + 21)
        .unwrap()
        .events
        .into_iter()
        .find(|event| event.kind == "hold.opened" && is_control(&event.kind, &event.data))
        .unwrap();
    f = reopen(f, NOW + 21);
    let cursor = f.session.lock().unwrap().last_seq();
    let repaired = control(
        &mut f,
        &request,
        "human:owner",
        ControlAction::PauseAll,
        NOW + 1_000_000,
    )
    .unwrap();
    assert_eq!(repaired.receipt.request_id, request);
    let later = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 1_000_000)
        .unwrap()
        .events;
    // This retry is deliberately beyond the module lease TTL. Hub subscribe
    // expires real leases; those system facts must not be mistaken for a second
    // human control. No other event may be appended while repairing the receipt.
    assert!(
        later.iter().all(|event| {
            event.kind == "lease.expired"
                && event.actor == "process:towermd-hub"
                && matches!(
                    event.lease_delta,
                    Some(crate::hub::SystemDelta::Expire { .. })
                )
        }),
        "receipt repair appended a non-expiry event: {later:?}"
    );
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&original));
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|event| event.eid == original.eid),
        Some(&original)
    );
    assert!(view(&f, NOW + 1_000_000).paused);
    assert!(control(
        &mut f,
        &request,
        "human:other",
        ControlAction::PauseAll,
        NOW + 1_000_001
    )
    .is_err());
}
fn rebuild(mut f: Fixture, now: u64, delete_logs: bool) -> Fixture {
    drop(f.session);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    if delete_logs {
        drop(f.outbox);
        let path = f.runtime.path().join("outbox");
        std::fs::remove_dir_all(&path).unwrap();
        f.outbox = RecoveryOutbox::open(&path, RecoveryContext::from_catalog(&f.catalog).unwrap())
            .unwrap();
    }
    let inputs = f.outbox.read_inputs().unwrap();
    let prepared =
        crate::recovery::prepare(&f.catalog, &inputs, now, now + 300_000, &id()).unwrap();
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
fn actual_rebuild_and_second_rebuild_after_log_deletion_keep_pause_without_restoring_old_runs() {
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
    let prompt_id = id();
    let action = ControlAction::Interject {
        scope: Scope::Project,
        text: "original historical prompt".into(),
    };
    let prompt = control(&mut f, &prompt_id, "human:owner", action.clone(), NOW + 20).unwrap();
    control(
        &mut f,
        &id(),
        "human:owner",
        ControlAction::PauseAll,
        NOW + 21,
    )
    .unwrap();
    control(
        &mut f,
        &id(),
        "process:towermd-cli",
        ControlAction::ResumeAll,
        NOW + 21,
    )
    .unwrap();
    assert!(view(&f, NOW + 21).resume_request.is_some());
    let batch = inbox(&mut f, NOW + 22);
    let mut ids: Vec<_> = batch.messages().iter().map(|m| m.id.clone()).collect();
    ids.sort();
    ProjectSession::acknowledge_inbox_shared(
        &f.session,
        &f.catalog,
        "host",
        &batch,
        &id(),
        &ids,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap();
    for pass in 0..2 {
        f = rebuild(f, NOW + 30 + pass * 20, pass == 1);
        let current = view(&f, NOW + 32 + pass * 20);
        assert!(
            current.paused,
            "rebuild must not silently resume a durable project pause"
        );
        assert!(current.active_runs.is_empty());
        assert!(
            current.resume_request.is_none(),
            "old Resume request cannot confirm the new empty Run scope"
        );
        let blocked = ProjectSession::execute_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            "host",
            TaskCommand::Claim {
                id: "T-ask".into(),
                harness: None,
                harness_version: None,
            },
            None,
            NOW + 32 + pass * 20,
            &mut f.outbox,
        );
        assert!(blocked.unwrap_err().message.contains("project paused"));
        assert!(
            inbox(&mut f, NOW + 33 + pass * 20).messages().is_empty(),
            "printed original IDs remain delivered"
        );
        let old = control(
            &mut f,
            &prompt_id,
            "human:owner",
            action.clone(),
            NOW + 34 + pass * 20,
        )
        .unwrap();
        assert_eq!(old.receipt, prompt.receipt);
        assert!(old.historical_only);
    }
    control(
        &mut f,
        &id(),
        "process:towermd-cli",
        ControlAction::ResumeAll,
        NOW + 80,
    )
    .unwrap();
    let resume = view(&f, NOW + 81).resume_request.unwrap();
    assert!(resume.runs.is_empty());
    control(
        &mut f,
        &id(),
        "human:owner",
        ControlAction::ConfirmResume {
            request: resume.request,
            pause_revision: resume.pause_revision,
            runs: resume.runs,
        },
        NOW + 82,
    )
    .unwrap();
    assert!(!view(&f, NOW + 83).paused);
    claim(&mut f, NOW + 84);
}

#[test]
fn confirmed_project_resume_survives_two_rebuilds_without_restoring_old_run_grants() {
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
    control(
        &mut f,
        &id(),
        "human:owner",
        ControlAction::PauseAll,
        NOW + 20,
    )
    .unwrap();
    control(
        &mut f,
        &id(),
        "process:towermd-cli",
        ControlAction::ResumeAll,
        NOW + 21,
    )
    .unwrap();
    let resume = view(&f, NOW + 22).resume_request.unwrap();
    control(
        &mut f,
        &id(),
        "human:owner",
        ControlAction::ConfirmResume {
            request: resume.request,
            pause_revision: resume.pause_revision,
            runs: resume.runs,
        },
        NOW + 23,
    )
    .unwrap();
    for pass in 0..2 {
        f = rebuild(f, NOW + 30 + pass * 20, pass == 1);
        let current = view(&f, NOW + 32 + pass * 20);
        assert!(!current.paused);
        assert!(current.active_runs.is_empty());
        assert!(current.resume_request.is_none());
    }
}

#[test]
fn accepted_human_input_with_lost_confirmation_retries_one_hold_without_duplicate_input() {
    let mut f = claimed();
    let request = id();
    let before = f.session.lock().unwrap().last_seq();
    f.outbox.fail_next_confirmation();
    assert!(control(
        &mut f,
        &request,
        "human:owner",
        ControlAction::PauseAll,
        NOW + 20
    )
    .unwrap_err()
    .contains("uncertain"));
    let tail = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before, NOW + 21)
        .unwrap()
        .events;
    assert_eq!(tail.iter().filter(|e| e.kind == "human.prompt").count(), 1);
    assert!(tail.iter().all(|e| e.kind != "hold.opened"));
    assert!(!f.session.lock().unwrap().human().unwrap().paused());
    f = reopen(f, NOW + 22);
    let result = control(
        &mut f,
        &request,
        "human:owner",
        ControlAction::PauseAll,
        NOW + 23,
    )
    .unwrap();
    assert_eq!(result.receipt.request_id, request);
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "human.prompt")
            .count(),
        1
    );
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "hold.opened"
                && e.data["request_body"]["action"]["action"] == "pause_all")
            .count(),
        1
    );
    assert!(view(&f, NOW + 24).paused);
}
