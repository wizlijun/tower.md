//! Actual B0/Task/Run/Ask/Answer producers; no forged current human fact.
use super::*;
use crate::{
    ask_answer_contract::AppAnswerAction,
    ask_fixture::*,
    catalog::Catalog,
    human_intervention::HumanCommand,
    project_session::ProjectSession,
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
};
use std::sync::Arc;

fn inbox(f: &mut Fixture, activity: InboxActivity, selector: Option<&str>, now: u64) -> InboxBatch {
    ProjectSession::inbox_shared(
        &f.session,
        &f.catalog,
        "host",
        selector,
        activity,
        &mut f.outbox,
        || Ok(now),
    )
    .unwrap()
}
fn answer(f: &mut Fixture, hold: &str, text: &str, now: u64) {
    let view = ProjectSession::load_ask_intervention_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        hold,
        || Ok(now),
    )
    .unwrap();
    ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &AppAnswerAction::Answer {
            hold: hold.into(),
            opening: view.binding,
            text: text.into(),
        },
        &mut f.outbox,
        || Ok(now + 1),
    )
    .unwrap();
}
fn ids(batch: &InboxBatch) -> Vec<String> {
    batch
        .messages()
        .iter()
        .map(|m| m.id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn ack(f: &mut Fixture, batch: &InboxBatch, id: &str, now: u64) -> Result<DeliveryOutcome, String> {
    ProjectSession::acknowledge_inbox_shared(
        &f.session,
        &f.catalog,
        "host",
        batch,
        id,
        &ids(batch),
        &mut f.outbox,
        || Ok(now),
    )
}
fn after(f: &Fixture, cursor: u64, now: u64) -> Vec<Event> {
    f.session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, now)
        .unwrap()
        .events
}
fn later_run(f: &mut Fixture, now: u64) {
    ProjectSession::tick_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        now,
        &mut f.outbox,
    )
    .unwrap();
    claim(f, now + 1);
}

#[test]
fn actual_answer_delivery_is_source_bound_idempotent_and_expired_run_never_renews() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("delivery original"),
        NOW + 20,
    );
    answer(&mut f, &opened.hold, "exact reply\r\nsecond line", NOW + 21);
    let before = f.session.lock().unwrap().last_seq();
    let batch = inbox(&mut f, InboxActivity::Recovery, None, NOW + 23);
    assert_eq!(batch.messages.len(), 1);
    assert_eq!(batch.messages[0].text, "exact reply\r\nsecond line");
    assert!(
        after(&f, before, NOW + 24).is_empty(),
        "read does not ack or renew"
    );
    let request = uuid::Uuid::now_v7().to_string();
    let receipt = ack(&mut f, &batch, &request, NOW + 25).unwrap();
    assert_eq!(receipt.receipt.result_ref.message_ids, ids(&batch));
    let original = events(&f)
        .into_iter()
        .find(|e| e.kind == "prompt.delivered")
        .unwrap();
    let cursor = f.session.lock().unwrap().last_seq();
    let expired = NOW + 1_000_000;
    assert_eq!(ack(&mut f, &batch, &request, expired).unwrap(), receipt);
    assert!(after(&f, cursor, expired).iter().all(|e| !matches!(
        e.kind.as_str(),
        "prompt.delivered" | "lease.granted" | "lease.renewed" | "task.claimed"
    )));
    assert_eq!(
        events(&f).iter().find(|e| e.eid == original.eid),
        Some(&original)
    );
    assert!(inbox(&mut f, InboxActivity::Project, None, expired + 1)
        .messages
        .is_empty());
    let current = f.session.lock().unwrap().last_seq();
    let empty = inbox(&mut f, InboxActivity::Recovery, None, expired + 2);
    assert!(empty.messages.is_empty());
    assert_eq!(
        f.session.lock().unwrap().last_seq(),
        current,
        "zero poll appends no empty ack"
    );
}

#[test]
fn same_pending_question_adds_second_actual_recipient_then_cross_run_batch_repairs_partial_confirmation(
) {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("shared pending delivery"),
        NOW + 20,
    );
    let later = NOW + 1_000_000;
    later_run(&mut f, later);
    let joined = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("shared pending delivery!"),
        later + 2,
    );
    assert_eq!(joined.hold, first.hold);
    let added = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.recipient_added")
        .unwrap();
    assert_eq!(
        added.data["hold"]["recipients"].as_array().unwrap().len(),
        2
    );
    answer(
        &mut f,
        &first.hold,
        "one answer, two frozen recipients",
        later + 3,
    );
    let batch = inbox(&mut f, InboxActivity::Recovery, None, later + 5);
    assert_eq!(batch.messages.len(), 2);
    assert_eq!(batch.action.groups.len(), 2);
    assert_ne!(
        batch.action.groups[0].recipient_run,
        batch.action.groups[1].recipient_run
    );
    assert_eq!(
        batch.messages.iter().filter(|m| m.actionable).count(),
        1,
        "old Run is archive only"
    );
    let request = uuid::Uuid::now_v7().to_string();
    let cursor = f.session.lock().unwrap().last_seq();
    f.outbox.fail_confirmation_after(1);
    assert!(ack(&mut f, &batch, &request, later + 6)
        .unwrap_err()
        .contains("uncertain"));
    let accepted: Vec<_> = after(&f, cursor, later + 7)
        .into_iter()
        .filter(|e| e.kind == "prompt.delivered")
        .collect();
    assert_eq!(
        accepted.len(),
        2,
        "fault is after one group durably confirms and next actual acceptance"
    );
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.receipt_confirmed(&accepted[0]));
    assert!(!inputs.receipt_confirmed(&accepted[1]));
    f = reopen(f, later + 8);
    let before = f.session.lock().unwrap().last_seq();
    let repaired = ack(&mut f, &batch, &request, later + 9).unwrap();
    assert_eq!(repaired.receipt.result_ref.message_ids, ids(&batch));
    assert!(
        after(&f, before, later + 10).is_empty(),
        "repair is original confirmations only, no Hub append/renew"
    );
    for actual in &accepted {
        assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(actual));
    }
    assert!(inbox(&mut f, InboxActivity::Recovery, None, later + 11)
        .messages
        .is_empty());
    let mut changed = batch.clone();
    changed.action.recipients.reverse();
    assert!(ack(&mut f, &changed, &request, later + 12).is_err());
    let mut changed = batch.clone();
    changed.action.groups.pop();
    changed.messages.pop();
    assert!(ack(&mut f, &changed, &request, later + 12)
        .unwrap_err()
        .contains("reused"));
}

#[test]
fn answered_question_is_not_reused_for_a_later_run_without_decision() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("answered is not new knowledge"),
        NOW + 20,
    );
    answer(&mut f, &first.hold, "old Run only", NOW + 21);
    let later = NOW + 1_000_000;
    later_run(&mut f, later);
    let next = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("answered is not new knowledge"),
        later + 2,
    );
    assert_ne!(first.hold, next.hold);
    assert!(next.current.reply.is_none());
    let run = f.session.lock().unwrap().human().unwrap().holds[&next.hold].recipients[0]
        .run
        .clone();
    assert!(
        inbox(&mut f, InboxActivity::Recovery, Some(&run), later + 3)
            .messages
            .is_empty()
    );
    let old = inbox(&mut f, InboxActivity::Recovery, None, later + 4);
    assert_eq!(old.messages.len(), 1);
    assert_eq!(old.messages[0].hold, first.hold);
    assert!(!old.messages[0].actionable);
    answer(&mut f, &next.hold, "new explicit answer", later + 5);
    let both = inbox(&mut f, InboxActivity::Recovery, None, later + 7);
    assert_eq!(
        both.action.groups.len(),
        2,
        "actual cross-Hold/Run batch is allowed"
    );
    ack(&mut f, &both, &uuid::Uuid::now_v7().to_string(), later + 8).unwrap();
    assert!(inbox(&mut f, InboxActivity::Recovery, None, later + 9)
        .messages
        .is_empty());
}

#[test]
fn live_project_poll_renews_only_original_epoch_and_recovery_poll_never_renews() {
    let mut f = claimed();
    let leases = f.session.lock().unwrap().leases(NOW + 12);
    let cursor = f.session.lock().unwrap().last_seq();
    let empty = inbox(&mut f, InboxActivity::Recovery, None, NOW + 20);
    assert!(empty.messages.is_empty());
    assert!(after(&f, cursor, NOW + 20).is_empty());
    inbox(&mut f, InboxActivity::Project, None, NOW + 30);
    let renewed = after(&f, cursor, NOW + 31);
    assert!(renewed.iter().any(|e| e.kind == "lease.renewed"));
    assert!(renewed.iter().all(|e| e.kind == "lease.renewed"));
    let current = f.session.lock().unwrap().leases(NOW + 31);
    for (name, lease) in leases
        .into_iter()
        .filter(|(name, _)| name.starts_with("module:"))
    {
        assert_eq!(current[&name].fence(), lease.fence());
        assert!(current[&name].expires_at > lease.expires_at);
    }
    assert!(ProjectSession::inbox_shared(
        &f.session,
        &f.catalog,
        "other-host",
        Some("not-my-run"),
        InboxActivity::Project,
        &mut f.outbox,
        || Ok(NOW + 32)
    )
    .is_err());
}

#[test]
fn delivery_reducer_merges_only_ids_preserving_later_reply_attention_and_unknown_fields() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("reduce exact IDs"),
        NOW + 20,
    );
    answer(&mut f, &opened.hold, "real accepted reply", NOW + 21);
    let batch = inbox(&mut f, InboxActivity::Recovery, None, NOW + 23);
    ack(&mut f, &batch, &uuid::Uuid::now_v7().to_string(), NOW + 24).unwrap();
    let actual = events(&f)
        .into_iter()
        .find(|e| e.kind == "prompt.delivered")
        .unwrap();
    let record = decode(
        f.catalog.config.project_id(),
        &recovery_outbox::original_of(&actual),
    )
    .unwrap();
    // Reducer-only late-state negative control. These altered raw bytes are
    // never submitted as an accepted human fact or used as a provenance proof.
    let mut late = record.hold.clone();
    late["reply"]["text"] = json!("later reply bytes");
    late["attention"] = json!(true);
    late["unknown_future"] = json!({"nested":[3,2,1]});
    let mut holds = BTreeMap::from([(opened.hold.clone(), late.clone())]);
    let mut canonical = crate::human_intervention::HumanProjection::default();
    reduce_ids(&mut canonical, &mut holds, &record).unwrap();
    let mut expected = late;
    expected["delivered"][&record.recipient_run] = json!(record.message_ids);
    assert_eq!(holds[&opened.hold], expected);
    assert_eq!(
        canonical.delivered_ids,
        record.message_ids.into_iter().collect()
    );
    let mut absent = BTreeMap::new();
    reduce_ids(
        &mut canonical,
        &mut absent,
        &decode(
            f.catalog.config.project_id(),
            &recovery_outbox::original_of(&actual),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(absent.is_empty(), "delivery never manufactures a Hold");
}

fn rebuild(mut f: Fixture, now: u64, delete_logs: bool) -> Fixture {
    drop(f.session);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    if delete_logs {
        drop(f.outbox);
        let directory = f.runtime.path().join("outbox");
        std::fs::remove_dir_all(&directory).unwrap();
        f.outbox = RecoveryOutbox::open(
            &directory,
            RecoveryContext::from_catalog(&f.catalog).unwrap(),
        )
        .unwrap();
    }
    let inputs = f.outbox.read_inputs().unwrap();
    let prepared = crate::recovery::prepare(
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
fn delivered_after_snapshot_survives_actual_rebuild_then_log_deletion_and_second_rebuild_without_revival(
) {
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
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("delivery after Q"),
        NOW + 20,
    );
    answer(
        &mut f,
        &opened.hold,
        "must not be reprinted after rebuild",
        NOW + 21,
    );
    let batch = inbox(&mut f, InboxActivity::Recovery, None, NOW + 23);
    let request = uuid::Uuid::now_v7().to_string();
    let receipt = ack(&mut f, &batch, &request, NOW + 24).unwrap();
    let original = events(&f)
        .into_iter()
        .find(|e| e.kind == "prompt.delivered")
        .unwrap();
    for pass in 0..2 {
        f = rebuild(f, NOW + 30 + pass * 20, pass == 1);
        let cursor = f.session.lock().unwrap().last_seq();
        assert!(
            inbox(&mut f, InboxActivity::Recovery, None, NOW + 32 + pass * 20)
                .messages
                .is_empty()
        );
        assert!(!f
            .session
            .lock()
            .unwrap()
            .human()
            .unwrap()
            .holds
            .contains_key(&opened.hold));
        assert_eq!(
            ack(&mut f, &batch, &request, NOW + 33 + pass * 20).unwrap(),
            receipt
        );
        let tail = after(&f, cursor, NOW + 34 + pass * 20);
        assert!(tail.iter().all(|e| !matches!(
            e.kind.as_str(),
            "prompt.delivered"
                | "hold.answered"
                | "task.claimed"
                | "lease.granted"
                | "lease.renewed"
        )));
        assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&original));
    }
}

#[test]
fn fabricated_source_event_and_actual_unknown_hold_carrier_do_not_prove_delivery() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("exact sources"),
        NOW + 20,
    );
    answer(&mut f, &opened.hold, "real text", NOW + 21);
    let batch = inbox(&mut f, InboxActivity::Recovery, None, NOW + 23);
    let mut bad = batch.clone();
    bad.action.groups[0].source.data["hold"]["reply"]["text"] = json!("forged");
    bad.messages[0].text = "forged".into();
    let cursor = f.session.lock().unwrap().last_seq();
    assert!(ack(&mut f, &bad, &uuid::Uuid::now_v7().to_string(), NOW + 24).is_err());
    assert!(after(&f, cursor, NOW + 25).is_empty());
    let mut unknown = batch.action.groups[0].source.clone();
    unknown.kind = "future.audit".into();
    assert!(!is_source(&unknown));
    assert!(source_records(f.catalog.config.project_id(), &[&unknown])
        .unwrap()
        .is_empty());
    assert!(batch.selected(&[]).is_err());
    assert!(batch.selected(&["not-offered".into()]).is_err());
}

#[test]
fn actual_g1_and_g4_review_replies_deliver_without_changing_exact_review_authority() {
    use crate::human_intervention::{ReviewGate, ReviewVerdict};
    use crate::recovery_contract::recorded::TaskReviewAction;
    let mut f = claimed();
    let mut bindings = Vec::new();
    for (index, gate) in [ReviewGate::G1, ReviewGate::G4].into_iter().enumerate() {
        let command = HumanCommand::ReviewRequest {
            content: (gate == ReviewGate::G1).then(|| "actual G1 plan bytes".into()),
            gate,
            source_commit: None,
            source_path: None,
        };
        let now = NOW + 20 + index as u64 * 5;
        let opened = ProjectSession::request_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            &uuid::Uuid::now_v7().to_string(),
            &command,
            None,
            &mut f.outbox,
            || Ok(now),
        )
        .unwrap();
        let binding = opened.receipt.result_ref.binding.clone();
        let before = inbox(&mut f, InboxActivity::Recovery, None, now + 1);
        assert!(before
            .messages
            .iter()
            .all(|m| m.hold != opened.receipt.result_ref.hold));
        assert!(
            f.session
                .lock()
                .unwrap()
                .human()
                .unwrap()
                .require_human_review(&binding)
                .is_err(),
            "delivery cannot manufacture a human answer"
        );
        ProjectSession::answer_task_review_shared(
            &f.session,
            &f.catalog,
            "host",
            "human:owner",
            &uuid::Uuid::now_v7().to_string(),
            &TaskReviewAction::Review {
                hold: opened.receipt.result_ref.hold,
                binding: binding.clone(),
                verdict: ReviewVerdict::Approved,
                comment: format!("actual review {index}"),
            },
            &mut f.outbox,
            || Ok(now + 2),
        )
        .unwrap();
        bindings.push(binding);
    }
    let batch = inbox(&mut f, InboxActivity::Recovery, None, NOW + 30);
    assert_eq!(batch.messages.len(), 2);
    assert!(batch
        .action
        .groups
        .iter()
        .all(|g| g.source.kind == "hold.reviewed"));
    assert!(batch
        .action
        .groups
        .iter()
        .all(|g| g.source.eid != g.source.data["hold"]["reply"]["id"].as_str().unwrap()));
    let facts: Vec<_> = bindings
        .iter()
        .map(|binding| {
            f.session
                .lock()
                .unwrap()
                .human()
                .unwrap()
                .require_human_review(binding)
                .unwrap()
                .clone()
        })
        .collect();
    ack(&mut f, &batch, &uuid::Uuid::now_v7().to_string(), NOW + 31).unwrap();
    for (binding, fact) in bindings.iter().zip(facts) {
        assert_eq!(
            f.session
                .lock()
                .unwrap()
                .human()
                .unwrap()
                .require_human_review(binding)
                .unwrap(),
            &fact
        );
    }
    let mut other = bindings[0].clone();
    other.fences[0].epoch += 1;
    assert!(
        f.session
            .lock()
            .unwrap()
            .human()
            .unwrap()
            .require_human_review(&other)
            .is_err(),
        "a delivered reply never substitutes fences"
    );
    assert!(inbox(&mut f, InboxActivity::Recovery, None, NOW + 32)
        .messages
        .is_empty());
}

#[test]
fn recovered_review_reply_is_historical_delivery_only_never_a_restored_review_fact() {
    use crate::human_intervention::{ReviewGate, ReviewVerdict};
    use crate::recovery_contract::recorded::TaskReviewAction;
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
    let opened = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::ReviewRequest {
            gate: ReviewGate::G1,
            content: Some("historical real plan".into()),
            source_commit: None,
            source_path: None,
        },
        None,
        &mut f.outbox,
        || Ok(NOW + 20),
    )
    .unwrap();
    let binding = opened.receipt.result_ref.binding.clone();
    ProjectSession::answer_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &uuid::Uuid::now_v7().to_string(),
        &TaskReviewAction::Review {
            hold: opened.receipt.result_ref.hold,
            binding: binding.clone(),
            verdict: ReviewVerdict::Approved,
            comment: "old actual verdict".into(),
        },
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap();
    f = rebuild(f, NOW + 30, false);
    let batch = inbox(&mut f, InboxActivity::Recovery, None, NOW + 32);
    assert_eq!(batch.messages.len(), 1);
    assert!(!batch.messages[0].actionable);
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&binding)
        .is_err());
    ack(&mut f, &batch, &uuid::Uuid::now_v7().to_string(), NOW + 33).unwrap();
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&binding)
        .is_err());
    assert!(inbox(&mut f, InboxActivity::Recovery, None, NOW + 34)
        .messages
        .is_empty());
}

#[test]
fn pending_fsync_failure_never_accepts_and_same_original_is_used_after_restart() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("pending delivery fsync"),
        NOW + 20,
    );
    answer(&mut f, &opened.hold, "printed source", NOW + 21);
    let batch = inbox(&mut f, InboxActivity::Recovery, None, NOW + 23);
    let request = uuid::Uuid::now_v7().to_string();
    let cursor = f.session.lock().unwrap().last_seq();
    f.outbox.fail_next_pending();
    let error = ack(&mut f, &batch, &request, NOW + 24).unwrap_err();
    assert!(
        error.contains(&request) && error.contains("uncertain"),
        "{error}"
    );
    assert!(
        after(&f, cursor, NOW + 25).is_empty(),
        "pending fsync failure precedes unique Hub append"
    );
    let original = f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .iter()
        .find(|event| event.kind == "prompt.delivered")
        .unwrap()
        .clone();
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .projection()
        .1
        .delivered_ids
        .is_empty());
    f = reopen(f, NOW + 26);
    ack(&mut f, &batch, &request, NOW + 27).unwrap();
    let actual = events(&f)
        .into_iter()
        .find(|event| event.kind == "prompt.delivered")
        .unwrap();
    assert_eq!(recovery_outbox::original_of(&actual), original);
    assert_eq!(
        after(&f, cursor, NOW + 28)
            .iter()
            .filter(|event| event.kind == "prompt.delivered")
            .count(),
        1
    );
}

#[test]
fn final_clock_failure_after_actual_acceptance_keeps_original_uuid_and_retry_never_reappends() {
    use std::cell::Cell;
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("final delivery proof"),
        NOW + 20,
    );
    answer(&mut f, &opened.hold, "already accepted group", NOW + 21);
    let batch = inbox(&mut f, InboxActivity::Recovery, None, NOW + 23);
    let request = uuid::Uuid::now_v7().to_string();
    let calls = Cell::new(0);
    let error = ProjectSession::acknowledge_inbox_shared(
        &f.session,
        &f.catalog,
        "host",
        &batch,
        &request,
        &ids(&batch),
        &mut f.outbox,
        || {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err("injected final clock I/O".into())
            } else {
                Ok(NOW + 24)
            }
        },
    )
    .unwrap_err();
    assert!(
        error.contains(&request) && error.contains("uncertain") && error.contains("final clock"),
        "{error}"
    );
    let original = events(&f)
        .into_iter()
        .find(|event| event.kind == "prompt.delivered")
        .unwrap();
    let cursor = f.session.lock().unwrap().last_seq();
    ack(&mut f, &batch, &request, NOW + 25).unwrap();
    assert!(after(&f, cursor, NOW + 26).is_empty());
    assert_eq!(
        events(&f).iter().find(|event| event.eid == original.eid),
        Some(&original)
    );
}

#[test]
fn actual_other_host_claim_cannot_collect_or_confirm_original_hosts_archived_reply() {
    use crate::tasks::TaskCommand;
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("host scope"),
        NOW + 20,
    );
    answer(&mut f, &opened.hold, "only original recipient", NOW + 21);
    let original_run = f.session.lock().unwrap().human().unwrap().holds[&opened.hold].recipients[0]
        .run
        .clone();
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
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "other-host",
        TaskCommand::Claim {
            id: "T-ask".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 24,
        &mut f.outbox,
    )
    .unwrap();
    let cursor = f.session.lock().unwrap().last_seq();
    assert!(ProjectSession::inbox_shared(
        &f.session,
        &f.catalog,
        "other-host",
        Some(&original_run),
        InboxActivity::Recovery,
        &mut f.outbox,
        || Ok(NOW + 25)
    )
    .is_err());
    assert!(ProjectSession::inbox_shared(
        &f.session,
        &f.catalog,
        "other-host",
        None,
        InboxActivity::Recovery,
        &mut f.outbox,
        || Ok(NOW + 26)
    )
    .unwrap()
    .messages()
    .is_empty());
    let original = inbox(&mut f, InboxActivity::Recovery, None, NOW + 27);
    assert_eq!(original.messages.len(), 1);
    assert!(!original.messages[0].actionable);
    assert!(ProjectSession::acknowledge_inbox_shared(
        &f.session,
        &f.catalog,
        "other-host",
        &original,
        &uuid::Uuid::now_v7().to_string(),
        &ids(&original),
        &mut f.outbox,
        || Ok(NOW + 28)
    )
    .is_err());
    assert!(after(&f, cursor, NOW + 29).is_empty());
    ack(
        &mut f,
        &original,
        &uuid::Uuid::now_v7().to_string(),
        NOW + 30,
    )
    .unwrap();
}

#[test]
fn unclaimed_and_main_viewer_scope_are_empty_without_ack_or_other_worktree_delivery() {
    let mut f = crate::ask_fixture::fixture();
    let cursor = f.session.lock().unwrap().last_seq();
    let empty = inbox(&mut f, InboxActivity::Project, None, NOW + 2);
    assert!(empty.messages.is_empty());
    assert!(after(&f, cursor, NOW + 3).is_empty());
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("stay on original worktree"),
        NOW + 20,
    );
    answer(&mut f, &opened.hold, "not for main viewer", NOW + 21);
    let viewer = f.runtime.path().join("main-viewer");
    git(
        f.root.path(),
        &["worktree", "add", viewer.to_str().unwrap(), "main"],
    );
    let catalog = Catalog::load(&viewer).unwrap();
    let cursor = f.session.lock().unwrap().last_seq();
    let batch = ProjectSession::inbox_shared(
        &f.session,
        &catalog,
        "host",
        None,
        InboxActivity::Project,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap();
    assert!(batch.messages().is_empty());
    assert!(after(&f, cursor, NOW + 24).is_empty());
    let old_run = f.session.lock().unwrap().human().unwrap().holds[&opened.hold].recipients[0]
        .run
        .clone();
    assert!(ProjectSession::inbox_shared(
        &f.session,
        &catalog,
        "host",
        Some(&old_run),
        InboxActivity::Project,
        &mut f.outbox,
        || Ok(NOW + 25)
    )
    .is_err());
    assert_eq!(
        inbox(&mut f, InboxActivity::Recovery, None, NOW + 26)
            .messages
            .len(),
        1
    );
    git(
        f.root.path(),
        &["worktree", "remove", viewer.to_str().unwrap()],
    );
}

#[test]
fn actual_finish_deadline_keeps_project_inbox_prelude_readonly_before_maintenance() {
    let mut f = claimed();
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 15),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    git(f.root.path(), &["rebase", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("deadline preserves actual reply"),
        NOW + 20,
    );
    answer(
        &mut f,
        &opened.hold,
        "read after deadline, never revive the Run",
        NOW + 21,
    );
    let request = uuid::Uuid::now_v7().to_string();
    let crate::finish::FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &request,
        None,
        &mut f.outbox,
        || Ok(NOW + 30),
    )
    .unwrap() else {
        panic!("actual first queued finish");
    };
    let input = ticket.status().input.clone();
    let run = input.run_id.clone();
    let deadline = input.merge_deadline;
    assert_eq!(deadline, input.entered_verifying_at + 3_600_000);
    drop(ticket); // No executor or maintenance producer is invoked by this test.
    for minute in [14, 28, 42, 56, 59] {
        let batch = inbox(
            &mut f,
            InboxActivity::Project,
            Some(&run),
            input.entered_verifying_at + minute * 60_000,
        );
        assert_eq!(batch.messages.len(), 1);
        assert!(batch.messages[0].actionable);
    }
    let before = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(before.objects["Run"][&run]["state"], "verifying");
    let fences = input.fences.clone();
    let leases = f.session.lock().unwrap().leases(deadline);
    for fence in &fences {
        assert_eq!(leases[&fence.name].fence(), *fence);
        assert!(
            leases[&fence.name].expires_at > deadline,
            "59 minute actual renew intentionally still covers 60 minute boundary"
        );
    }
    // Renewals are accepted directly by Hub before the next Session replay.
    // Observe the actual accepted head so that the preceding 59-minute renewal
    // cannot be mistaken for a new write at the deadline.
    let cursor = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    // These commands all enter this same project prelude. Exercise implicit
    // status scope and explicit release/finish--wait Run selectors without
    // running their business bodies or installing early maintenance.
    let mut last = None;
    for selector in [None, Some(run.as_str()), Some(run.as_str())] {
        let batch = inbox(&mut f, InboxActivity::Project, selector, deadline);
        assert_eq!(batch.messages.len(), 1);
        assert!(!batch.messages[0].actionable);
        last = Some(batch);
    }
    let boundary_events = after(&f, cursor, deadline);
    assert!(
        boundary_events.is_empty(),
        "deadline prelude never renews, reacquires, expires or confirms a new delivery: {:?}",
        boundary_events
            .iter()
            .map(|event| (&event.kind, event.seq))
            .collect::<Vec<_>>()
    );
    assert_eq!(f.session.lock().unwrap().leases(deadline), leases);
    assert_eq!(f.session.lock().unwrap().projection().unwrap(), &before);
    let batch = last.unwrap();
    let delivery_id = uuid::Uuid::now_v7().to_string();
    let receipt = ack(&mut f, &batch, &delivery_id, deadline).unwrap();
    let accepted = after(&f, cursor, deadline);
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].kind, "prompt.delivered");
    let repaired_cursor = f.session.lock().unwrap().last_seq();
    assert_eq!(
        ack(&mut f, &batch, &delivery_id, deadline + 1).unwrap(),
        receipt
    );
    assert!(after(&f, repaired_cursor, deadline + 1).is_empty());
    assert_eq!(f.session.lock().unwrap().leases(deadline + 1), leases);
    let current = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(current.objects["Run"][&run], before.objects["Run"][&run]);
    assert_eq!(current.objects["Task"], before.objects["Task"]);
    assert!(
        inbox(&mut f, InboxActivity::Project, Some(&run), deadline + 2)
            .messages
            .is_empty()
    );
}
