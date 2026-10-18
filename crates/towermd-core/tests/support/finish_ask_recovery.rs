//! Combined actual producers and core App-confirm recovery. This is neither a
//! daemon/AppGate transport test nor GUI/human acceptance evidence.
use super::*;
use towermd_core::{
    hub_protocol::Event,
    human_intervention::{AskState, HumanState},
    project_session::ask::AskProgress,
    recovery, snapshot_service,
};

fn ask(f: &mut Fixture, id: &str, action: &HumanCommand, now: u64) -> AskProgress {
    ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        id,
        action,
        None,
        &mut f.outbox,
        || Ok(now),
    )
    .unwrap()
}
fn question(text: &str) -> HumanCommand {
    HumanCommand::AskOpen {
        question: text.into(),
        options: vec![],
        recommended: None,
        context: "mixed actual Ask/Finish recovery".into(),
    }
}
fn checkpoint(f: &mut Fixture, now: u64) {
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(now),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
}
fn reopen(mut f: Fixture, now: u64) -> Fixture {
    drop(f.session);
    f.session = Arc::new(std::sync::Mutex::new(
        ProjectSession::open(
            &f.catalog.common_dir.join("towermd"),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            now,
        )
        .unwrap(),
    ));
    f
}
fn rebuild(mut f: Fixture, now: u64) -> Fixture {
    // Exercise the existing actual core recovery boundary after losing Hub
    // storage, never a hand-built seed, frozen proof or approval Event.
    drop(f.session);
    fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
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
fn archive(f: &Fixture) -> Vec<Event> {
    snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap()
        .archives
        .values()
        .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
        .collect()
}
fn assert_audit(f: &Fixture, original: &Event) {
    let outer = archive(f);
    let audits = outer
        .iter()
        .filter(|event| {
            event.kind == "request.receipt.restored"
                && event.data["original"]["eid"] == original.eid
        })
        .collect::<Vec<_>>();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].data["original"], serde_json::json!(original));
    assert!(!outer.iter().any(|event| event.eid == original.eid));
    let mut human = HumanState::default();
    human.apply(audits[0]).unwrap();
    assert!(
        human.holds.is_empty(),
        "the inner receipt grants no Human state"
    );
    // The public reader proves the complete persisted Event; original producer
    // retries below additionally exercise its private durability/uncertainty guard.
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .contains(original));
}

#[test]
fn ask_historical_audit_and_live_ask_coexist_with_third_finish_hold_across_snapshot_and_rebuild() {
    let mut f = claimed(&["exit 7"], false);
    checkpoint(&mut f, NOW + 15);
    let old_open_id = uuid::Uuid::now_v7().to_string();
    let old = ask(
        &mut f,
        &old_open_id,
        &question("historical slice"),
        NOW + 20,
    );
    let wait_id = uuid::Uuid::now_v7().to_string();
    let wait = HumanCommand::AskWait {
        hold: old.hold.clone(),
    };
    let waiting = ask(&mut f, &wait_id, &wait, NOW + 21);
    let deadline = waiting.wait_until.unwrap();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(deadline)
    })
    .unwrap();
    let terminal = ask(&mut f, &wait_id, &wait, deadline + 1);
    let original = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| {
            event.kind == "hold.wait_finished" && event.data["p3_request"]["request_id"] == wait_id
        })
        .unwrap()
        .clone();
    assert!(!original.stale);

    // This first actual rebuild creates the finite historical audit. The old
    // guarded opening is stale; neither it nor its inner terminal revives Run.
    f = rebuild(f, deadline + 10);
    assert_audit(&f, &original);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .leases(deadline + 12)
        .contains_key("module:A-alpha"));
    let historical = ask(&mut f, &wait_id, &wait, deadline + 12);
    assert_eq!(historical.receipt, terminal.receipt);
    assert_eq!(historical.current.state, AskState::Expired);
    assert!(!historical.actionable);

    // An actual new claim and explicit Ask create a current Human Hold beside
    // the existing audit. A relay remains process testimony, not G1/G4 approval.
    // Recovery advanced main; sync this owned task branch before a new Run.
    // Keep the real G5 latest-main requirement rather than bypassing it.
    git(f.root.path(), &["rebase", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    claim(&mut f, deadline + 20).unwrap();
    let current_id = uuid::Uuid::now_v7().to_string();
    let current_action = question("current independent question");
    let current = ask(&mut f, &current_id, &current_action, deadline + 21);
    let answer = ask(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::Relay {
            hold: current.hold.clone(),
            text: "批准仅是转述文字".into(),
            self_reported: None,
        },
        deadline + 22,
    );
    assert_eq!(answer.current.state, AskState::Answered);
    assert_eq!(
        answer.current.reply.as_ref().unwrap().actor,
        "process:towermd-cli"
    );
    let mut outcomes = Vec::new();
    for n in 0..3 {
        let outcome = completed(
            &mut f,
            &uuid::Uuid::now_v7().to_string(),
            deadline + 30 + n * 10,
        );
        assert_eq!(
            outcome.evidence.as_ref().unwrap().result,
            EvidenceResult::Failed
        );
        outcomes.push(outcome);
    }
    let last = outcomes.last().unwrap();
    let run_id = last.status.input.run_id.clone();
    let expected = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(expected.objects["Run"][&run_id]["state"], "held");
    assert_eq!(expected.objects["Run"][&run_id]["finish"]["failures"], 3);
    assert_eq!(expected.objects["Hold"].len(), 2);
    let gate = expected.objects["Hold"]
        .values()
        .find(|hold| hold["kind"] == "gatefailed")
        .unwrap()
        .clone();
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&current.hold));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(gate["id"].as_str().unwrap()));
    assert!(!expected.objects.contains_key("Decision"));
    assert!(!expected.objects.contains_key("Rule"));
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "hold.reviewed"));

    checkpoint(&mut f, deadline + 80);
    f = reopen(f, deadline + 81);
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects,
        expected.objects
    );
    assert_audit(&f, &original);
    let before_tick = f.session.lock().unwrap().leases(deadline + 82);
    let before_seq = f.session.lock().unwrap().last_seq();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(deadline + 82)
    })
    .unwrap();
    assert_eq!(f.session.lock().unwrap().leases(deadline + 82), before_tick);
    assert_eq!(f.session.lock().unwrap().last_seq(), before_seq);

    // The second core confirmation must split only validated automatic Holds
    // from Human restore while retaining both complete raw Hold objects.
    let before_finish_events = archive(&f)
        .into_iter()
        .filter(|event| event.kind.starts_with("finish.") || event.kind == "evidence.recorded")
        .collect::<Vec<_>>();
    f = rebuild(f, deadline + 90);
    assert_audit(&f, &original);
    // FR-93 invalidates every old active Run, including held/verifying. Only
    // these two lifecycle state fields change; full frozen finish history,
    // deadline/failure accounting, Evidence and both raw Holds remain exact.
    let task_id = last.status.input.task_id.clone();
    let mut released_expected = expected.clone();
    released_expected
        .objects
        .get_mut("Task")
        .unwrap()
        .get_mut(&task_id)
        .unwrap()["state"] = serde_json::json!("ready");
    released_expected
        .objects
        .get_mut("Run")
        .unwrap()
        .get_mut(&run_id)
        .unwrap()["state"] = serde_json::json!("released");
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(projection.objects, released_expected.objects);
    assert_eq!(
        projection.objects["Run"][&run_id]["finish"],
        expected.objects["Run"][&run_id]["finish"]
    );
    let rebuilt_archive = archive(&f); // selected real Git archive passes validate_archive
    let released = rebuilt_archive
        .iter()
        .filter(|event| {
            event.kind == "task.released"
                && event.obj == task_id
                && event.data["task"]["id"] == task_id
                && event.data["run"]["id"] == run_id
        })
        .collect::<Vec<_>>();
    assert_eq!(
        released.len(),
        1,
        "one actual recovery release for this exact old Run and Task"
    );
    assert!(!released[0].stale);
    // Event keeps compatibility flags private; its canonical wire encoding
    // omits legacy only when false.
    assert!(serde_json::to_value(released[0])
        .unwrap()
        .get("legacy")
        .is_none());
    assert_eq!(released[0].actor, "process:towermd-daemon");
    assert_eq!(released[0].layer, "L5");
    assert!(released[0].guards.is_empty());
    assert_eq!(
        released[0].data["task"],
        released_expected.objects["Task"][&task_id]
    );
    assert_eq!(
        released[0].data["run"],
        released_expected.objects["Run"][&run_id]
    );
    let after_finish_events = rebuilt_archive
        .into_iter()
        .filter(|event| event.kind.starts_with("finish.") || event.kind == "evidence.recorded")
        .collect::<Vec<_>>();
    assert_eq!(
        after_finish_events, before_finish_events,
        "recovery must not reverify or create/rewrite finish or gate Evidence events"
    );
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&current.hold));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(gate["id"].as_str().unwrap()));
    assert_eq!(
        projection.objects["Run"][&run_id]["finish"]["merge_deadline"],
        last.status.input.merge_deadline
    );
    // The recovery writer owns main; no execution Module lease survives.
    let restored_leases = f.session.lock().unwrap().leases(deadline + 92);
    assert_eq!(restored_leases.len(), 1, "{restored_leases:?}");
    assert_eq!(restored_leases["main"].holder, "snapshot:host");
    let cursor = f.session.lock().unwrap().last_seq();
    let historical = ask(&mut f, &wait_id, &wait, deadline + 92);
    assert_eq!(historical.receipt, terminal.receipt);
    assert!(!historical.actionable);
    let current_history = ask(&mut f, &current_id, &current_action, deadline + 93);
    assert_eq!(current_history.receipt, current.receipt);
    assert_eq!(current_history.current.state, AskState::Expired);
    assert!(!current_history.actionable);
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(deadline + 94)
    })
    .unwrap();
    assert_eq!(f.session.lock().unwrap().last_seq(), cursor);
    assert_eq!(
        f.session.lock().unwrap().leases(deadline + 94),
        restored_leases
    );
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects,
        released_expected.objects
    );
    assert!(!archive(&f)
        .iter()
        .any(|event| event.kind == "hold.reviewed"));
}
