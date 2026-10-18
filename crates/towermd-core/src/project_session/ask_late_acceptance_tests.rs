//! A pending original keeps its timestamps even when the actual Hub accepts it
//! later. These fixtures use real Task/Ask producers; mutated Event clones below
//! are rejection inputs only and are never used as accepted source evidence.
use crate::ask_fixture as fixture;
use crate::{
    hub_protocol::Event,
    human_intervention::{AskState, HumanCommand, HumanState},
    project_session::ProjectSession,
    recovery_outbox,
};
use fixture::*;

fn millis(event: &Event) -> u64 {
    u64::try_from(
        chrono::DateTime::parse_from_rfc3339(&event.t)
            .unwrap()
            .timestamp_millis(),
    )
    .unwrap()
}
fn stamp(now: u64) -> String {
    chrono::DateTime::from_timestamp_millis(i64::try_from(now).unwrap())
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn no_actual_event(f: &Fixture, after: u64, eid: &str, now: u64) {
    let tail = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(after, now)
        .unwrap();
    assert!(
        tail.events.iter().all(|event| event.eid != eid),
        "pending fsync failure must not invoke Hub append"
    );
}

#[test]
fn opening_pending_fsync_failure_accepts_later_with_original_time_body_and_guards() {
    let mut f = claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let command = open("late original opening");
    let before = f.session.lock().unwrap().applied_seq();
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
    assert!(
        error.contains("injected outbox write/fsync uncertainty"),
        "{error}"
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let frozen = inputs
        .pending()
        .iter()
        .find(|event| event.kind == "hold.opened" && event.data["p3_request"]["request_id"] == id)
        .unwrap()
        .clone();
    assert_eq!(frozen.data["hold"]["opened_ms"], NOW + 20);
    no_actual_event(&f, before, &frozen.eid, NOW + 20);
    let accepted = call(&mut f, &id, &command, NOW + 21);
    assert_eq!(accepted.current.state, AskState::Pending);
    let actual = events(&f)
        .into_iter()
        .find(|event| event.eid == frozen.eid)
        .unwrap();
    assert_eq!(recovery_outbox::original_of(&actual), frozen);
    assert_eq!(millis(&actual), NOW + 21);
    assert!(!actual.stale);
    assert_eq!(
        f.session.lock().unwrap().human().unwrap().holds[&accepted.hold].opened_ms,
        NOW + 20
    );
    let mut future = actual.clone();
    future.t = stamp(NOW + 19);
    let mut state = HumanState::default();
    assert!(state.apply(&future).is_err());
    assert!(state.holds.is_empty());
    let mut wrong_guard = actual.clone();
    wrong_guard.guards[0].epoch += 1;
    assert!(HumanState::default().apply(&wrong_guard).is_err());
    assert!(ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &open("changed original body"),
        None,
        &mut f.outbox,
        || Ok(NOW + 22)
    )
    .unwrap_err()
    .contains("reused"));
}

#[test]
fn wait_pending_fsync_failure_accepts_later_without_resetting_start_deadline_or_budget() {
    for accepted_at in [NOW + 22, NOW + 90_022] {
        let mut f = claimed();
        let opened = call(
            &mut f,
            &uuid::Uuid::now_v7().to_string(),
            &open("late original slice"),
            NOW + 20,
        );
        let id = uuid::Uuid::now_v7().to_string();
        let command = HumanCommand::AskWait {
            hold: opened.hold.clone(),
        };
        let before = f.session.lock().unwrap().applied_seq();
        f.outbox.fail_next_pending();
        let error = ProjectSession::ask_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &command,
            None,
            &mut f.outbox,
            || Ok(NOW + 21),
        )
        .unwrap_err();
        assert!(
            error.contains("injected outbox write/fsync uncertainty"),
            "{error}"
        );
        let inputs = f.outbox.read_inputs().unwrap();
        let frozen = inputs
            .pending()
            .iter()
            .find(|event| {
                event.kind == "hold.wait_started" && event.data["p3_request"]["request_id"] == id
            })
            .unwrap()
            .clone();
        let deadline = NOW + 90_021;
        assert_eq!(frozen.data["slice"]["started_ms"], NOW + 21);
        assert_eq!(frozen.data["slice"]["deadline_ms"], deadline);
        no_actual_event(&f, before, &frozen.eid, NOW + 21);
        assert!(
            f.session.lock().unwrap().human().unwrap().holds[&opened.hold]
                .waits
                .is_empty()
        );
        let accepted = call(&mut f, &id, &command, accepted_at);
        let started = events(&f)
            .into_iter()
            .find(|event| event.eid == frozen.eid)
            .unwrap();
        assert_eq!(recovery_outbox::original_of(&started), frozen);
        assert_eq!(millis(&started), accepted_at);
        assert!(!started.stale);
        assert_eq!(accepted.slice.as_ref().unwrap().started_ms, NOW + 21);
        assert_eq!(accepted.slice.as_ref().unwrap().deadline_ms, deadline);
        if accepted_at < deadline {
            assert!(accepted.receipt.is_none());
            assert_eq!(accepted.wait_until, Some(deadline));
        } else {
            assert!(accepted.receipt.is_some());
            assert_eq!(accepted.slice.as_ref().unwrap().finished_ms, Some(deadline));
        }
        let all = events(&f);
        let mut before_start = HumanState::default();
        for event in all.iter().filter(|event| event.seq < started.seq) {
            before_start.apply(event).unwrap();
        }
        let mut future = started.clone();
        future.t = stamp(NOW + 20);
        assert!(before_start.clone().apply(&future).is_err());
        let mut wrong_guard = started.clone();
        wrong_guard.guards[0].epoch += 1;
        assert!(before_start.apply(&wrong_guard).is_err());
        let settled_at = accepted_at.max(deadline);
        ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
            Ok(settled_at)
        })
        .unwrap();
        let final_result = call(&mut f, &id, &command, settled_at);
        assert!(final_result.receipt.is_some());
        assert_eq!(
            final_result.slice.as_ref().unwrap().finished_ms,
            Some(deadline)
        );
        let hold = f.session.lock().unwrap().human().unwrap().holds[&opened.hold].clone();
        assert_eq!(hold.waits.len(), 1);
        assert_eq!(
            crate::human_intervention::wait_budget(&hold, &hold.recipients[0].run, settled_at),
            90_000
        );
        assert_eq!(
            events(&f)
                .iter()
                .filter(|event| event.kind == "hold.wait_started"
                    && event.data["p3_request"]["request_id"] == id)
                .count(),
            1
        );
        assert_eq!(
            events(&f)
                .iter()
                .filter(|event| event.kind == "hold.wait_finished"
                    && event.data["p3_request"]["request_id"] == id)
                .count(),
            1
        );
    }
}

#[test]
fn a_late_original_still_uses_expired_guards_and_never_rehydrates_an_active_hold() {
    let mut f = claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let command = open("late past actual lease");
    f.outbox.fail_next_pending();
    assert!(ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 20)
    )
    .is_err());
    let inputs = f.outbox.read_inputs().unwrap();
    let original = inputs
        .pending()
        .iter()
        .find(|event| event.data["p3_request"]["request_id"] == id)
        .unwrap()
        .clone();
    let accepted = call(&mut f, &id, &command, NOW + 1_000_000);
    let actual = events(&f)
        .into_iter()
        .find(|event| event.eid == original.eid)
        .unwrap();
    assert_eq!(recovery_outbox::original_of(&actual), original);
    assert!(actual.stale);
    assert_eq!(accepted.current.state, AskState::Expired);
    assert!(!accepted.actionable);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&accepted.hold));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .leases(NOW + 1_000_000)
        .contains_key("module:A-alpha"));
}
