//! Fault injection stays test-only; each Hold still comes from real Task/Hub producers.
use super::*;
use crate::ask_fixture as fixture;
use fixture::*;

#[test]
fn pending_fsync_failure_never_appends_and_original_retry_does_not_change_body() {
    let mut f = claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let command = open("pending failure");
    let before = f.session.lock().unwrap().hub.contiguous_seq();
    f.outbox.fail_next_pending();
    let error = ProjectSession::ask_shared(
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
    // Before Hub.append the sole durable helper preserves an I/O refusal.
    // The shared contract requires no Hub call, not post-acceptance uncertainty.
    assert_eq!(error, "injected outbox write/fsync uncertainty");
    let appended = f
        .session
        .lock()
        .unwrap()
        .hub
        .subscribe(before, NOW + 20)
        .unwrap();
    assert!(appended
        .events
        .iter()
        .all(|event| event.data["p3_request"]["request_id"] != id));
    let pending = f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .iter()
        .find(|event| event.data["p3_request"]["request_id"] == id)
        .unwrap()
        .clone();
    assert!(events(&f)
        .iter()
        .all(|event| event.data["p3_request"]["request_id"] != id));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .values()
        .any(|h| h.id == format!("H-{id}")));
    let recovered = call(&mut f, &id, &command, NOW + 21);
    assert!(recovered.receipt.is_some());
    let confirmed = events(&f)
        .into_iter()
        .find(|event| event.data["p3_request"]["request_id"] == id)
        .unwrap();
    assert_eq!(crate::recovery_outbox::original_of(&confirmed), pending);
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.data["p3_request"]["request_id"] == id)
            .count(),
        1
    );
}
#[test]
fn confirmed_failure_keeps_actual_join_and_recovers_original_after_restart() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("confirm join"),
        NOW + 20,
    );
    let later = NOW + 1_000_000;
    ProjectSession::tick_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        later,
        &mut f.outbox,
    )
    .unwrap();
    claim(&mut f, later + 1);
    let id = uuid::Uuid::now_v7().to_string();
    f.outbox.fail_next_confirmation();
    let error = ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &open("confirm join!"),
        None,
        &mut f.outbox,
        || Ok(later + 2),
    )
    .unwrap_err();
    assert!(error.contains("uncertain"));
    assert_eq!(
        f.session.lock().unwrap().human().unwrap().holds[&first.hold]
            .recipients
            .len(),
        2,
        "Hub accepted projection survives uncertain return"
    );
    f = reopen(f, later + 3);
    let recovered = call(&mut f, &id, &open("confirm join!"), later + 4);
    assert_eq!(recovered.hold, first.hold);
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "hold.recipient_added" && e.obj == first.hold)
            .count(),
        1
    );
}
#[test]
fn decision_confirmed_failure_after_open_and_after_answer_only_repair_the_original_receipt() {
    for confirmations in [0, 2] {
        let mut f = claimed_with_decision(true);
        let id = uuid::Uuid::now_v7().to_string();
        f.outbox.fail_confirmation_after(confirmations);
        let error = ProjectSession::ask_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &open("权威问题!"),
            None,
            &mut f.outbox,
            || Ok(NOW + 20),
        )
        .unwrap_err();
        assert!(error.contains("uncertain") && error.contains(&id));
        let hold = f.session.lock().unwrap().human().unwrap().holds[&format!("H-{id}")].clone();
        assert_eq!(
            hold.reply.is_some(),
            confirmations == 2,
            "fault must occur after the intended actual phase"
        );
        f = reopen(f, NOW + 21);
        let recovered = call(&mut f, &id, &open("权威问题!"), NOW + 22);
        assert_eq!(recovered.current.state, AskState::Answered);
        assert_eq!(
            events(&f)
                .iter()
                .filter(|e| e.kind == "hold.opened" && e.data["p3_request"]["request_id"] == id)
                .count(),
            1
        );
        assert_eq!(
            events(&f)
                .iter()
                .filter(|e| e.kind == "hold.answered" && e.data["p3_request"]["request_id"] == id)
                .count(),
            1
        );
    }
}
#[test]
fn expired_run_can_be_settled_by_system_only_at_deadline_without_lease_revival() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("expire settlement"),
        NOW + 20,
    );
    let id = uuid::Uuid::now_v7().to_string();
    let waiting = call(
        &mut f,
        &id,
        &HumanCommand::AskWait {
            hold: opened.hold.clone(),
        },
        NOW + 21,
    );
    let end = waiting.wait_until.unwrap();
    let before = f.session.lock().unwrap().leases(NOW + 22);
    ProjectSession::human_tick_shared(
        &f.session,
        &f.catalog,
        "host",
        &mut f.outbox,
        || Ok(end - 1),
    )
    .unwrap();
    assert!(!events(&f)
        .iter()
        .any(|e| e.kind == "hold.wait_finished" && e.obj == opened.hold));
    let later = NOW + 1_000_000;
    ProjectSession::tick_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        later,
        &mut f.outbox,
    )
    .unwrap();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || Ok(later))
        .unwrap();
    let finished = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.wait_finished" && e.obj == opened.hold)
        .unwrap();
    assert_eq!(finished.actor, "process:towermd-daemon");
    assert!(finished.guards.is_empty());
    assert_eq!(
        finished.data["request_body"]["actor"],
        "process:towermd-cli"
    );
    assert_eq!(finished.data["slice"]["finished_ms"], end);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .leases(later)
        .contains_key("module:A-alpha"));
    assert!(before.contains_key("module:A-alpha"));
    let read = call(
        &mut f,
        &id,
        &HumanCommand::AskWait { hold: opened.hold },
        later + 1,
    );
    assert_eq!(read.current.state, AskState::Expired);
    assert!(!read.actionable);
}

#[test]
fn actual_join_replay_rejects_changed_opening_members_budget_reply_and_unknown_metadata() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("strict membership"),
        NOW + 20,
    );
    let later = NOW + 1_000_000;
    ProjectSession::tick_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        later,
        &mut f.outbox,
    )
    .unwrap();
    claim(&mut f, later + 1);
    call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("strict membership!"),
        later + 2,
    );
    let recorded = events(&f);
    let added = recorded
        .iter()
        .find(|e| e.kind == "hold.recipient_added" && e.obj == first.hold)
        .unwrap();
    let mut before = human_intervention::HumanState::default();
    for event in recorded.iter().filter(|e| e.seq < added.seq) {
        before.apply(event).unwrap();
    }
    let original = before.projection().0.clone();
    for which in 0..7 {
        // Tampered clones are negative input only, never producer evidence.
        let mut bad = added.clone();
        match which {
            0 => bad.data["opening_eid"] = json!("git:forged"),
            1 => {
                let duplicate = bad.data["added"].clone();
                bad.data["hold"]["recipients"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            2 => bad.data["hold"]["recipients"][0]["host"] = json!("other-host"),
            3 => bad.data["hold"]["attention_request_id"] = json!(uuid::Uuid::now_v7().to_string()),
            4 => bad.data["hold"]["opened_seq"] = json!(added.seq),
            5 => {
                bad.data["hold"]["waits"] = json!([{ "id":uuid::Uuid::now_v7().to_string(),"recipient_run":"invented","started_ms":later,"deadline_ms":later+1,"finished_ms":null}])
            }
            6 => bad.data["hold"]["body"]["context"] = json!("changed original context"),
            _ => unreachable!(),
        }
        let mut state = before.clone();
        assert!(state.apply(&bad).is_err(), "case {which}");
        assert_eq!(
            state.projection().0,
            &original,
            "rejected transition installs nothing"
        );
    }
    before.apply(added).unwrap();
    assert_eq!(before.holds[&first.hold].recipients.len(), 2);
}
#[test]
fn actual_finished_replay_rejects_early_wrong_actor_and_changed_original_wait_identity() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("strict settlement"),
        NOW + 20,
    );
    let id = uuid::Uuid::now_v7().to_string();
    let start = call(
        &mut f,
        &id,
        &HumanCommand::AskWait {
            hold: first.hold.clone(),
        },
        NOW + 21,
    );
    let end = start.wait_until.unwrap();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || Ok(end))
        .unwrap();
    let recorded = events(&f);
    let finished = recorded
        .iter()
        .find(|e| e.kind == "hold.wait_finished" && e.obj == first.hold)
        .unwrap();
    let started = recorded
        .iter()
        .find(|e| e.kind == "hold.wait_started" && e.obj == first.hold)
        .unwrap();
    let mut before = human_intervention::HumanState::default();
    for event in recorded.iter().filter(|e| e.seq < finished.seq) {
        before.apply(event).unwrap();
    }
    for which in 0..4 {
        let mut bad = finished.clone();
        match which {
            0 => bad.t = started.t.clone(),
            1 => bad.actor = "process:towermd-cli".into(),
            2 => bad.guards = started.guards.clone(),
            3 => bad.data["slice"]["recipient_run"] = json!("other-run"),
            _ => unreachable!(),
        }
        assert!(before.clone().apply(&bad).is_err(), "case {which}");
    }
    before.apply(finished).unwrap();
    assert_eq!(before.holds[&first.hold].waits[0].finished_ms, Some(end));
}
#[test]
fn attention_confirmation_failure_retries_frozen_identity_after_restart_without_renewing() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("attention durability"),
        NOW + 20,
    );
    let raw = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.opened" && e.obj == opened.hold)
        .unwrap();
    let attention_id = raw.data["hold"]["attention_request_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let colliding = ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        &attention_id,
        &open("different body"),
        None,
        &mut f.outbox,
        || Ok(NOW + 21),
    )
    .unwrap_err();
    assert!(colliding.contains("reused"));
    let now = NOW + 1_800_020;
    f.outbox.fail_next_confirmation();
    assert!(ProjectSession::human_tick_shared(
        &f.session,
        &f.catalog,
        "host",
        &mut f.outbox,
        || Ok(now)
    )
    .unwrap_err()
    .contains("uncertain"));
    assert!(f.session.lock().unwrap().human().unwrap().holds[&opened.hold].attention);
    f = reopen(f, now + 1);
    ProjectSession::human_tick_shared(
        &f.session,
        &f.catalog,
        "host",
        &mut f.outbox,
        || Ok(now + 2),
    )
    .unwrap();
    let recorded = events(&f);
    let attention = recorded
        .iter()
        .filter(|e| e.kind == "hold.attention" && e.obj == opened.hold)
        .collect::<Vec<_>>();
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0].data["p3_request"]["request_id"], attention_id);
    assert!(attention[0].guards.is_empty());
    assert!(!f
        .session
        .lock()
        .unwrap()
        .leases(now + 2)
        .contains_key("module:A-alpha"));
    let mut early = (*attention[0]).clone();
    early.t = raw.t.clone();
    let mut state = human_intervention::HumanState::default();
    state.apply(&raw).unwrap();
    assert!(state.apply(&early).is_err());
}

#[test]
fn actual_legacy_outer_opening_without_attention_metadata_is_readable_but_never_gets_a_random_tick_id(
) {
    let mut f = claimed();
    let modern = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("legacy seed"),
        NOW + 20,
    );
    let opening = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.opened" && e.obj == modern.hold)
        .unwrap();
    let mut raw = opening.data["hold"].clone();
    raw["id"] = json!("H-imported-without-attention");
    raw.as_object_mut().unwrap().remove("attention_request_id");
    let original = NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:towermd-cli".into(),
        kind: "hold.opened".into(),
        layer: "L5".into(),
        obj: "H-imported-without-attention".into(),
        data: json!({"hold":raw}),
        guards: opening.guards.clone(),
    };
    {
        let mut live = f.session.lock().unwrap();
        recovery_outbox::append_durable(
            live.protocol().unwrap(),
            &mut f.outbox,
            original,
            NOW + 21,
        )
        .unwrap();
    }
    let inputs = f.outbox.read_inputs().unwrap();
    ProjectSession::sync_shared(&f.session, &f.catalog, &inputs, NOW + 22).unwrap();
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key("H-imported-without-attention"));
    let now = NOW + 1_800_050;
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || Ok(now))
        .unwrap();
    assert!(!events(&f)
        .iter()
        .any(|e| e.kind == "hold.attention" && e.obj == "H-imported-without-attention"));
    assert!(
        !f.session.lock().unwrap().human().unwrap().holds["H-imported-without-attention"].attention
    );
}

#[test]
fn observer_same_original_only_renews_frozen_pending_slice_and_never_after_answer() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("observer renewal"),
        NOW + 20,
    );
    let wait = HumanCommand::AskWait {
        hold: first.hold.clone(),
    };
    let origin = call(&mut f, &uuid::Uuid::now_v7().to_string(), &wait, NOW + 21);
    let id = uuid::Uuid::now_v7().to_string();
    let observer = call(&mut f, &id, &wait, NOW + 22);
    let before = f.session.lock().unwrap().leases(NOW + 22)["module:A-alpha"].clone();
    let same = call(&mut f, &id, &wait, NOW + 37);
    let renewed = f.session.lock().unwrap().leases(NOW + 37)["module:A-alpha"].clone();
    assert_eq!(same.receipt, observer.receipt);
    assert_eq!(same.slice, origin.slice);
    assert_eq!(before.holder, renewed.holder);
    assert_eq!(before.epoch, renewed.epoch);
    assert!(renewed.expires_at > before.expires_at);
    call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::Relay {
            hold: first.hold,
            text: "stop waiting".into(),
            self_reported: None,
        },
        NOW + 40,
    );
    let before = f.session.lock().unwrap().leases(NOW + 40)["module:A-alpha"].clone();
    let completed = call(&mut f, &id, &wait, NOW + 50);
    assert_eq!(completed.current.state, AskState::Answered);
    assert_eq!(completed.receipt, observer.receipt);
    let after = f.session.lock().unwrap().leases(NOW + 50)["module:A-alpha"].clone();
    assert_eq!(
        before, after,
        "readable answered observer does not perpetually renew"
    );
}

#[test]
fn new_recipient_added_kind_cannot_extend_an_actual_review_hold() {
    let mut f = claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let command = HumanCommand::ReviewRequest {
        gate: human_intervention::ReviewGate::G1,
        content: Some("real immutable Review subject".into()),
        source_commit: None,
        source_path: None,
    };
    let review = ProjectSession::request_task_review_shared(
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
    let recorded = events(&f);
    let opening = recorded
        .iter()
        .find(|event| event.eid == review.receipt.result_ref.opening_eid)
        .unwrap();
    let to = {
        let live = f.session.lock().unwrap();
        recipient(
            &context(&live, &f.catalog, "host", None, NOW + 20)
                .unwrap()
                .run,
        )
    };
    let mut state = human_intervention::HumanState::default();
    for event in &recorded {
        state.apply(event).unwrap();
    }
    let original = state.projection().0.clone();
    // A negative clone of a real Review is not accepted producer evidence.
    let mut forbidden = opening.clone();
    forbidden.kind = "hold.recipient_added".into();
    forbidden.seq += 1;
    forbidden.data["hold"]["recipients"] = json!([to]);
    assert!(state
        .apply(&forbidden)
        .unwrap_err()
        .contains("limited to Ask"));
    assert_eq!(state.projection().0, &original);
}

#[test]
fn wait_finished_confirmation_failure_is_repaired_by_tick_after_restart_without_resettlement() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("wait terminal durability"),
        NOW + 20,
    );
    let id = uuid::Uuid::now_v7().to_string();
    let waiting = call(
        &mut f,
        &id,
        &HumanCommand::AskWait {
            hold: opened.hold.clone(),
        },
        NOW + 21,
    );
    let deadline = waiting.wait_until.unwrap();
    // human_tick first confirms the already accepted started phase in retry,
    // then settle_wait retries that same phase. Fail only terminal confirmation.
    f.outbox.fail_confirmation_after(2);
    let error =
        ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
            Ok(deadline)
        })
        .unwrap_err();
    assert!(
        error.contains("uncertain") && error.contains(&id),
        "{error}"
    );
    let actual = f
        .session
        .lock()
        .unwrap()
        .tail
        .iter()
        .find(|event| {
            event.kind == "hold.wait_finished" && event.data["p3_request"]["request_id"] == id
        })
        .cloned()
        .expect("the fault must follow actual terminal Hub acceptance");
    assert!(!actual.stale);
    assert!(actual.guards.is_empty());
    assert_eq!(actual.actor, "process:towermd-daemon");
    assert_eq!(actual.data["slice"]["finished_ms"], deadline);
    assert!(!f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
    assert_eq!(
        f.session.lock().unwrap().human().unwrap().holds[&opened.hold].waits[0].finished_ms,
        Some(deadline)
    );
    f = reopen(f, deadline + 1);
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    let leases = f.session.lock().unwrap().leases(deadline + 1);
    let cursor = f.session.lock().unwrap().last_seq();
    let records = events(&f).len();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(deadline + 2)
    })
    .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.receipt_confirmed(&actual));
    assert!(!inputs
        .pending()
        .iter()
        .any(|original| original.eid == actual.eid));
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.eid == actual.eid)
            .collect::<Vec<_>>(),
        vec![&actual]
    );
    assert_eq!(
        inputs.confirmed().len(),
        records,
        "repair must not append another receipt or phase"
    );
    assert_eq!(
        f.session.lock().unwrap().last_seq(),
        cursor,
        "repair must not append a Hub event"
    );
    assert_eq!(f.session.lock().unwrap().projection().unwrap(), &projection);
    assert_eq!(
        f.session.lock().unwrap().leases(deadline + 2),
        leases,
        "receipt repair never renews Module leases"
    );
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(deadline + 3)
    })
    .unwrap();
    assert_eq!(
        events(&f)
            .iter()
            .filter(|event| event.kind == "hold.wait_finished"
                && event.data["p3_request"]["request_id"] == id)
            .count(),
        1
    );
}

#[test]
fn historical_wait_terminal_confirmation_repairs_original_without_effective_hold_or_lease_revival()
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
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("historical wait receipt"),
        NOW + 20,
    );
    let id = uuid::Uuid::now_v7().to_string();
    let waiting = call(
        &mut f,
        &id,
        &HumanCommand::AskWait {
            hold: opened.hold.clone(),
        },
        NOW + 21,
    );
    let deadline = waiting.wait_until.unwrap();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(deadline)
    })
    .unwrap();
    let actual = events(&f)
        .into_iter()
        .find(|event| {
            event.kind == "hold.wait_finished" && event.data["p3_request"]["request_id"] == id
        })
        .unwrap();
    // Reuse the real App recovery path. The original accepted receipt survives,
    // while the new actual outer schedule makes this Ask historical only.
    drop(f.session);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let prepared = crate::recovery::prepare(
        &f.catalog,
        &inputs,
        deadline + 10,
        deadline + 300_000,
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
            || Ok(deadline + 20),
        )
        .unwrap();
    assert_eq!(report.status, "rebuilt");
    f.session = session;
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&opened.hold));
    assert_historical_system_audit(&mut f, &actual, deadline + 20);
    // A failed fsync of the exact already accepted original creates durability
    // uncertainty, without a new Event or a fabricated current Hold.
    let original = recovery_outbox::original_of(&actual);
    f.outbox.fail_next_confirmation();
    assert!(f.outbox.persist_confirmed(&original, &actual).is_err());
    assert!(!f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
    f = reopen(f, deadline + 21);
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    let leases = f.session.lock().unwrap().leases(deadline + 21);
    let cursor = f.session.lock().unwrap().last_seq();
    let records = events(&f).len();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(deadline + 22)
    })
    .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.receipt_confirmed(&actual));
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .find(|event| event.eid == actual.eid),
        Some(&actual)
    );
    assert!(!inputs
        .pending()
        .iter()
        .any(|pending| pending.eid == actual.eid));
    assert_eq!(inputs.confirmed().len(), records);
    assert_eq!(f.session.lock().unwrap().last_seq(), cursor);
    assert_eq!(f.session.lock().unwrap().projection().unwrap(), &projection);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&opened.hold));
    assert_eq!(f.session.lock().unwrap().leases(deadline + 22), leases);
    assert!(!leases.contains_key("module:A-alpha"));
}

fn assert_historical_system_audit(f: &mut Fixture, actual: &crate::hub_protocol::Event, now: u64) {
    let saved = crate::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let archive = saved
        .archives
        .values()
        .flat_map(|text| text.lines())
        .map(|line| crate::hub_legacy::decode_archive_line(line).unwrap())
        .collect::<Vec<_>>();
    let audit = archive
        .iter()
        .find(|event| {
            event.kind == "request.receipt.restored" && event.data["original"]["eid"] == actual.eid
        })
        .expect("recovery must archive a finite receipt audit, not reapply the system transition");
    assert_eq!(
        crate::request_ledger::restored_original(f.catalog.config.project_id(), audit)
            .unwrap()
            .as_ref(),
        Some(actual)
    );
    assert!(
        !archive.iter().any(|event| event.eid == actual.eid),
        "the new actual schedule cannot turn the old system result into a fresh transition"
    );
    let mut human = human_intervention::HumanState::default();
    human.apply(audit).unwrap();
    assert!(
        human.holds.is_empty(),
        "an inner original is never Human authority"
    );
    assert!(
        human
            .apply(actual)
            .unwrap_err()
            .contains("actual outer opening"),
        "ordinary new system events still require an actual opening"
    );
    for field in ["sequence", "time", "stale", "body", "actor", "outer_actor"] {
        let mut changed = audit.clone();
        match field {
            "sequence" => changed.data["original"]["seq"] = json!(actual.seq + 1),
            "time" => changed.data["original"]["t"] = json!("2026-01-01T00:00:00.000Z"),
            "stale" => changed.data["original"]["stale"] = json!(true),
            "body" => {
                changed.data["original"]["data"]["hold"]["unknown_original_extension"] =
                    json!("changed")
            }
            "actor" => changed.data["original"]["actor"] = json!("process:other"),
            "outer_actor" => changed.actor = "process:other".into(),
            _ => unreachable!(),
        }
        assert!(
            crate::request_ledger::restored_original(f.catalog.config.project_id(), &changed)
                .is_err(),
            "{field}"
        );
    }
    let inputs = f.outbox.read_inputs().unwrap();
    let capture = f.session.lock().unwrap().capture(now).unwrap();
    let history =
        crate::recovery_git::prepare_session_history(&f.catalog, &inputs, &capture).unwrap();
    let original = recovery_outbox::original_of(actual);
    assert_eq!(
        history.ask_system_original(&f.catalog, &original).unwrap(),
        Some(actual)
    );
    let mut changed = original.clone();
    changed.data["hold"]["unknown_original_extension"] = json!("changed");
    assert!(
        history.ask_system_original(&f.catalog, &changed).is_err(),
        "the same eid cannot substitute a different complete original"
    );
    for field in ["actor", "guards", "kind", "hold_kind"] {
        let mut changed = original.clone();
        match field {
            "actor" => changed.actor = "process:other".into(),
            "guards" => {
                changed.guards = actual.data["hold"]["recipients"][0]["fences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| serde_json::from_value(value.clone()).unwrap())
                    .collect()
            }
            "kind" => changed.kind = "hold.answered".into(),
            "hold_kind" => changed.data["hold"]["body"]["kind"] = json!("review"),
            _ => unreachable!(),
        }
        assert!(
            history.ask_system_original(&f.catalog, &changed).is_err(),
            "{field}"
        );
    }
}

#[test]
fn attention_original_receipt_rebuild_is_history_without_current_hold_or_new_attention() {
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
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("historical attention receipt"),
        NOW + 20,
    );
    let now = NOW + 1_800_020;
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || Ok(now))
        .unwrap();
    let actual = events(&f)
        .into_iter()
        .find(|event| event.kind == "hold.attention" && event.obj == opened.hold)
        .unwrap();
    drop(f.session);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let prepared = crate::recovery::prepare(
        &f.catalog,
        &inputs,
        now + 10,
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
            || Ok(now + 20),
        )
        .unwrap();
    assert_eq!(report.status, "rebuilt");
    f.session = session;
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_historical_system_audit(&mut f, &actual, now + 20);
    f = reopen(f, now + 21);
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    let cursor = f.session.lock().unwrap().last_seq();
    let leases = f.session.lock().unwrap().leases(now + 21);
    let records = events(&f);
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(now + 22)
    })
    .unwrap();
    assert_eq!(f.session.lock().unwrap().last_seq(), cursor);
    assert_eq!(f.session.lock().unwrap().projection().unwrap(), &projection);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&opened.hold));
    assert_eq!(f.session.lock().unwrap().leases(now + 22), leases);
    assert!(!leases.contains_key("module:A-alpha"));
    assert_eq!(
        events(&f),
        records,
        "historical attention does not emit or replace any receipt"
    );
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
    // The archived audit must retain the original receipt without the old
    // producer logs, and a second real rebuild still grants no Human authority.
    drop(f.session);
    drop(f.outbox);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let directory = f.runtime.path().join("outbox");
    std::fs::remove_dir_all(&directory).unwrap();
    f.outbox = recovery_outbox::RecoveryOutbox::open(
        &directory,
        recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
    )
    .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.confirmed().is_empty());
    let prepared = crate::recovery::prepare(
        &f.catalog,
        &inputs,
        now + 30,
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
            || Ok(now + 40),
        )
        .unwrap();
    assert_eq!(report.status, "rebuilt");
    f.session = session;
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_historical_system_audit(&mut f, &actual, now + 40);
    let cursor = f.session.lock().unwrap().last_seq();
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(now + 41)
    })
    .unwrap();
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
    assert_eq!(events(&f), vec![actual]);
    assert_eq!(f.session.lock().unwrap().last_seq(), cursor);
    assert_eq!(f.session.lock().unwrap().projection().unwrap(), &projection);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&opened.hold));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .leases(now + 41)
        .contains_key("module:A-alpha"));
}

#[test]
fn system_wait_pending_without_original_accepted_receipt_refuses_rebuild() {
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
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("unknown original terminal"),
        NOW + 20,
    );
    let id = uuid::Uuid::now_v7().to_string();
    let waiting = call(
        &mut f,
        &id,
        &HumanCommand::AskWait { hold: opened.hold },
        NOW + 21,
    );
    let deadline = waiting.wait_until.unwrap();
    f.outbox.fail_confirmation_after(2);
    assert!(ProjectSession::human_tick_shared(
        &f.session,
        &f.catalog,
        "host",
        &mut f.outbox,
        || Ok(deadline)
    )
    .unwrap_err()
    .contains("uncertain"));
    let actual = f
        .session
        .lock()
        .unwrap()
        .tail
        .iter()
        .find(|event| {
            event.kind == "hold.wait_finished" && event.data["p3_request"]["request_id"] == id
        })
        .cloned()
        .unwrap();
    let original = recovery_outbox::original_of(&actual);
    // A failed fsync may leave a complete, readable confirmed line. Remove
    // only that disposable receipt to model loss of all original acceptance
    // evidence; keep the exact durable pending body, then lose the old Hub.
    for entry in std::fs::read_dir(f.runtime.path().join("outbox")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|part| part.to_str()) != Some("jsonl") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let kept = text
            .lines()
            .filter(|line| {
                let value: Value = serde_json::from_str(line).unwrap();
                !(value["state"] == "confirmed" && value["original"]["eid"] == actual.eid)
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, format!("{kept}\n")).unwrap();
    }
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .contains(&original));
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.eid == original.eid));
    drop(f.session);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let digest = inputs.digest().to_owned();
    let private = crate::snapshot_git::private_revision(f.root.path()).unwrap();
    let authority = f.catalog.revision.clone();
    let prepared = crate::recovery::prepare(
        &f.catalog,
        &inputs,
        deadline + 10,
        deadline + 300_000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let error = match f.writer.confirm_rebuild(
        None,
        &f.catalog,
        &prepared,
        "human:owner",
        prepared.expires_at,
        &inputs,
        "host",
        || Ok(deadline + 20),
    ) {
        Ok(_) => panic!("a raw pending system result cannot become an accepted original"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("uncertain") && error.contains("original accepted receipt"),
        "{error}"
    );
    let after = f.outbox.read_inputs().unwrap();
    assert_eq!(after.digest(), digest);
    assert!(after.pending().contains(&original));
    assert!(!after
        .confirmed()
        .iter()
        .any(|event| event.eid == original.eid));
    assert_eq!(
        crate::snapshot_git::private_revision(f.root.path()).unwrap(),
        private
    );
    assert_eq!(Catalog::load(f.root.path()).unwrap().revision, authority);
}

fn phase_only_original_after_actual_rebuild(decision: bool, keep_original: bool) {
    let mut f = claimed_with_decision(decision);
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
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    let request_id = uuid::Uuid::now_v7().to_string();
    let (command, hold) = if decision {
        let command = open("权威问题!");
        let session = f.session.clone();
        let error = ProjectSession::ask_shared(
            &f.session,
            &f.catalog,
            "host",
            &request_id,
            &command,
            None,
            &mut f.outbox,
            || {
                if session
                    .lock()
                    .unwrap()
                    .human()
                    .unwrap()
                    .holds
                    .contains_key(&format!("H-{request_id}"))
                {
                    Err("injected interruption after actual Decision opening".into())
                } else {
                    Ok(NOW + 20)
                }
            },
        )
        .unwrap_err();
        assert!(
            error.contains("uncertain") && error.contains(&request_id),
            "{error}"
        );
        (command, format!("H-{request_id}"))
    } else {
        let opened = call(
            &mut f,
            &uuid::Uuid::now_v7().to_string(),
            &open("phase-only wait"),
            NOW + 20,
        );
        let command = HumanCommand::AskWait {
            hold: opened.hold.clone(),
        };
        let started = call(&mut f, &request_id, &command, NOW + 21);
        assert!(started.receipt.is_none());
        (command, opened.hold)
    };
    let phase = events(&f)
        .into_iter()
        .find(|event| event.data["p3_request"]["request_id"] == request_id)
        .unwrap();
    assert_eq!(
        phase.kind,
        if decision {
            "hold.opened"
        } else {
            "hold.wait_started"
        }
    );
    assert!(phase.data.get("p3_receipt").is_none());
    assert!(!events(&f)
        .iter()
        .any(|event| event.data["p3_request"]["request_id"] == request_id
            && matches!(event.kind.as_str(), "hold.answered" | "hold.wait_finished")));
    if !keep_original {
        // Remove only this original phase confirmation from the disposable
        // outbox. Recovery must not infer it from a later stale outer replay.
        for entry in std::fs::read_dir(f.runtime.path().join("outbox")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|part| part.to_str()) != Some("jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let kept = text
                .lines()
                .filter(|line| {
                    let value: Value = serde_json::from_str(line).unwrap();
                    !(value["state"] == "confirmed" && value["original"]["eid"] == phase.eid)
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(path, format!("{kept}\n")).unwrap();
        }
    }
    drop(f.session);
    std::fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let prepared = crate::recovery::prepare(
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
            &prepared,
            "human:owner",
            prepared.expires_at,
            &inputs,
            "host",
            || Ok(NOW + 40),
        )
        .unwrap();
    assert_eq!(report.status, "rebuilt");
    f.session = session;
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&hold));
    let cursor = f.session.lock().unwrap().applied_seq();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 41)
        .unwrap();
    let records = events(&f).len();
    let epoch = f.session.lock().unwrap().epoch_high_water();
    let result = ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        &request_id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 41),
    );
    if keep_original {
        let result = result.unwrap();
        assert_eq!(result.hold, hold);
        assert_eq!(result.current.state, AskState::Expired);
        assert!(!result.actionable);
        assert!(
            result.receipt.is_none(),
            "reading a phase must not invent a terminal receipt"
        );
        assert!(
            result.current.reply.is_none(),
            "a planned Decision is not an accepted answer"
        );
        assert!(result.wait_until.is_none());
        assert_eq!(
            result.slice,
            if decision {
                None
            } else {
                Some(serde_json::from_value(phase.data["slice"].clone()).unwrap())
            }
        );
        assert_eq!(
            events(&f).iter().find(|event| event.eid == phase.eid),
            Some(&phase)
        );
    } else {
        let error = result.unwrap_err();
        assert!(
            error.contains("uncertain") && error.contains("stale outer replay"),
            "{error}"
        );
    }
    // This is after the old wait deadline, but no historical phase may restart
    // a slice, answer a Decision, renew a lease, or block unrelated tick work.
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(NOW + 100_000)
    })
    .unwrap();
    let after = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 100_001)
        .unwrap();
    assert_eq!(
        after
            .events
            .iter()
            .filter(|event| event.kind.starts_with("hold."))
            .collect::<Vec<_>>(),
        before
            .events
            .iter()
            .filter(|event| event.kind.starts_with("hold."))
            .collect::<Vec<_>>()
    );
    assert_eq!(events(&f).len(), records);
    assert_eq!(f.session.lock().unwrap().epoch_high_water(), epoch);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .leases(NOW + 100_001)
        .contains_key("module:A-alpha"));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&hold));
}

#[test]
fn decision_opening_only_original_receipt_is_historical_after_actual_rebuild() {
    for keep_original in [true, false] {
        phase_only_original_after_actual_rebuild(true, keep_original);
    }
}

#[test]
fn wait_started_only_original_receipt_is_historical_after_actual_rebuild() {
    for keep_original in [true, false] {
        phase_only_original_after_actual_rebuild(false, keep_original);
    }
}
