//! Actual Task/Hub/Session Ask producers. These are core integration fixtures,
//! not daemon/CLI/App acceptance or human approval evidence.
#[path = "support/baseline_review.rs"]
mod baseline_fixture;
#[path = "support/ask.rs"]
mod fixture;
use fixture::*;
use std::sync::Arc;
use towermd_core::{
    catalog::Catalog,
    human_intervention::{AskState, HumanCommand},
    project_session::ProjectSession,
};

#[test]
fn actual_open_reuse_relay_and_original_retry_preserve_immutable_response() {
    let mut f = claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let command = open("是否写入？");
    let first = call(&mut f, &id, &command, NOW + 20);
    assert_eq!(first.current.state, AskState::Pending);
    assert!(first.receipt.is_some());
    assert!(first.actionable);
    let again = call(&mut f, &id, &command, NOW + 21);
    assert_eq!(again.receipt, first.receipt);
    let reused = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("是否写入!"),
        NOW + 22,
    );
    assert_eq!(reused.hold, first.hold);
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "hold.opened" && e.obj == first.hold)
            .count(),
        1
    );
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "hold.requested" && e.obj == first.hold)
            .count(),
        1
    );
    let relay = HumanCommand::Relay {
        hold: first.hold.clone(),
        text: "人说先停止\r\n保留原文".into(),
        self_reported: None,
    };
    let answered = call(&mut f, &uuid::Uuid::now_v7().to_string(), &relay, NOW + 30);
    assert_eq!(answered.current.state, AskState::Answered);
    let reply = answered.current.reply.unwrap();
    assert_eq!(reply.actor, "process:towermd-cli");
    assert_eq!(reply.relayed_from.as_deref(), Some("human"));
    let original = call(&mut f, &id, &command, NOW + 31);
    assert_eq!(
        original.receipt, first.receipt,
        "receipt preserves original pending result"
    );
    assert_eq!(original.current.state, AskState::Answered);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .contains_key("Decision"));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .contains_key("Rule"));
    assert!(ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &open("different"),
        None,
        &mut f.outbox,
        || Ok(NOW + 32)
    )
    .unwrap_err()
    .contains("reused"));
}

#[test]
fn actual_wait_reuses_one_slice_and_spends_at_most_ten_minutes_per_run() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("等待预算"),
        NOW + 20,
    );
    let wait = HumanCommand::AskWait {
        hold: first.hold.clone(),
    };
    let id = uuid::Uuid::now_v7().to_string();
    let running = call(&mut f, &id, &wait, NOW + 21);
    let slice = running.slice.clone().unwrap();
    assert_eq!(slice.id, id);
    assert_eq!(slice.deadline_ms, NOW + 90_021);
    assert!(running.receipt.is_none());
    let repeat = call(&mut f, &id, &wait, NOW + 22);
    assert_eq!(repeat.slice, Some(slice));
    let overlap = call(&mut f, &uuid::Uuid::now_v7().to_string(), &wait, NOW + 23);
    assert_eq!(overlap.slice, running.slice);
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(NOW + 90_021)
    })
    .unwrap();
    let done = call(&mut f, &id, &wait, NOW + 90_021);
    assert!(done.receipt.is_some());
    assert_eq!(done.slice.as_ref().unwrap().finished_ms, Some(NOW + 90_021));
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "hold.wait_started" && e.data["p3_request"]["request_id"] == id)
            .count(),
        1
    );
    let mut now = NOW + 90_021;
    for duration in [90_000, 90_000, 90_000, 90_000, 90_000, 60_000] {
        let id = uuid::Uuid::now_v7().to_string();
        let waiting = call(&mut f, &id, &wait, now);
        assert_eq!(waiting.wait_until, Some(now + duration));
        now += duration;
        ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
            Ok(now)
        })
        .unwrap();
        let done = call(&mut f, &id, &wait, now);
        assert!(done.receipt.is_some());
    }
    let deferred = call(&mut f, &uuid::Uuid::now_v7().to_string(), &wait, now);
    assert_eq!(deferred.current.state, AskState::Deferred);
    assert_eq!(deferred.wait_until, None);
    let hold = f.session.lock().unwrap().human().unwrap().holds[&first.hold].clone();
    assert_eq!(
        towermd_core::human_intervention::wait_budget(&hold, &hold.recipients[0].run, now),
        600_000
    );
}

#[test]
fn expired_first_run_does_not_block_only_explicit_second_run_joining_pending_ask() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("共用问题"),
        NOW + 20,
    );
    let old = f.session.lock().unwrap().human().unwrap().holds[&first.hold].recipients[0].clone();
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
    let wait = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::AskWait {
            hold: first.hold.clone(),
        },
        later + 2,
    );
    assert_eq!(wait.current.state, AskState::Expired);
    assert!(!wait.actionable);
    assert_eq!(
        f.session.lock().unwrap().human().unwrap().holds[&first.hold].recipients,
        vec![old.clone()]
    );
    let second = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("共用问题!"),
        later + 3,
    );
    assert_eq!(second.hold, first.hold);
    let added = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.recipient_added")
        .unwrap();
    assert_eq!(
        added.guards,
        serde_json::from_value::<Vec<towermd_core::hub_protocol::Fence>>(
            added.data["added"]["fences"].clone()
        )
        .unwrap()
    );
    assert_ne!(added.guards, old.fences);
    assert_eq!(
        added.data["hold"]["recipients"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        added.data["hold"]["opened_seq"],
        f.session.lock().unwrap().human().unwrap().holds[&first.hold].opened_seq
    );
    assert_eq!(
        added.data["p3_receipt"]["result_ref"]
            .as_object()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(added.data["p3_receipt"]["result_ref"]["state"], "pending");
    assert!(f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .inbox(&[old], &added.guards)
        .iter()
        .all(|m| !m.actionable));
}

#[test]
fn attention_is_once_at_thirty_minutes_and_does_not_renew_expired_recipient() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("请关注"),
        NOW + 20,
    );
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(NOW + 1_800_019)
    })
    .unwrap();
    assert!(!f.session.lock().unwrap().human().unwrap().holds[&opened.hold].attention);
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(NOW + 1_800_020)
    })
    .unwrap();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(NOW + 1_800_021)
    })
    .unwrap();
    assert!(f.session.lock().unwrap().human().unwrap().holds[&opened.hold].attention);
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "hold.attention" && e.obj == opened.hold)
            .count(),
        1
    );
    let leases = f.session.lock().unwrap().leases(NOW + 1_800_021);
    assert!(
        !leases.contains_key("module:A-alpha"),
        "attention does not prolong the Task"
    );
}

#[test]
fn actual_decision_open_then_answer_uses_one_original_and_never_worktree_text() {
    let mut f = claimed_with_decision(true);
    std::fs::write(
        f.root.path().join("docs/tower/decisions/D-ask.md"),
        "uncommitted false answer",
    )
    .unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let first = call(&mut f, &id, &open("权威问题!"), NOW + 20);
    assert_eq!(first.current.state, AskState::Answered);
    let reply = first.current.reply.as_ref().unwrap();
    assert_eq!(reply.text, "使用仓库答案");
    assert_eq!(reply.actor, "process:towermd-dedupe");
    assert_eq!(reply.decision.as_deref(), Some("D-ask"));
    let recorded = events(&f);
    let opened = recorded
        .iter()
        .find(|e| e.kind == "hold.opened" && e.obj == first.hold)
        .unwrap();
    let answered = recorded
        .iter()
        .find(|e| e.kind == "hold.answered" && e.obj == first.hold)
        .unwrap();
    assert!(opened.seq < answered.seq);
    assert!(opened.data["hold"]["reply"].is_null());
    assert!(opened.data.get("p3_receipt").is_none());
    assert_eq!(opened.data["planned_reply"], answered.data["hold"]["reply"]);
    assert_eq!(opened.data["p3_request"], answered.data["p3_request"]);
    assert_eq!(opened.data["request_body"], answered.data["request_body"]);
    f = reopen(f, NOW + 21);
    let retry = call(&mut f, &id, &open("权威问题!"), NOW + 22);
    assert_eq!(retry.receipt, first.receipt);
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.obj == first.hold && e.kind == "hold.answered")
            .count(),
        1
    );
}

#[test]
fn actual_snapshot_tail_and_restart_preserve_slice_budget_and_original_receipts() {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("重启期间预算"),
        NOW + 20,
    );
    let wait = HumanCommand::AskWait {
        hold: opened.hold.clone(),
    };
    let id = uuid::Uuid::now_v7().to_string();
    let started = call(&mut f, &id, &wait, NOW + 21);
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
    f = reopen(f, NOW + 31);
    let resumed = call(&mut f, &id, &wait, NOW + 32);
    assert_eq!(resumed.slice, started.slice);
    assert!(resumed.receipt.is_none());
    let end = started.wait_until.unwrap();
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || Ok(end))
        .unwrap();
    let finished = call(&mut f, &id, &wait, end);
    assert!(finished.receipt.is_some());
    f = reopen(f, end + 1);
    let retried = call(&mut f, &id, &wait, end + 2);
    assert_eq!(retried.receipt, finished.receipt);
    let hold = f.session.lock().unwrap().human().unwrap().holds[&opened.hold].clone();
    assert_eq!(hold.waits.len(), 1);
    assert_eq!(
        towermd_core::human_intervention::wait_budget(&hold, &hold.recipients[0].run, end + 2),
        90_000
    );
}

#[test]
fn answer_before_new_run_open_creates_another_hold_instead_of_inheriting_session_reply() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("会话不是知识"),
        NOW + 20,
    );
    call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::Relay {
            hold: first.hold.clone(),
            text: "只给本次会话".into(),
            self_reported: None,
        },
        NOW + 21,
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
    let second = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("会话不是知识!"),
        later + 2,
    );
    assert_ne!(first.hold, second.hold);
    assert_eq!(second.current.state, AskState::Pending);
    assert!(second.current.reply.is_none());
}

#[test]
fn answer_after_actual_join_preserves_both_recipients_and_only_new_live_run_is_actionable() {
    let mut f = claimed();
    let first = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("加入先于回答"),
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
    let joined = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("加入先于回答!"),
        later + 2,
    );
    let before = f.session.lock().unwrap().human().unwrap().holds[&first.hold]
        .recipients
        .clone();
    assert_eq!(before.len(), 2);
    let replied = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::Relay {
            hold: joined.hold.clone(),
            text: "双方均能归档查看".into(),
            self_reported: None,
        },
        later + 3,
    );
    assert_eq!(replied.current.state, AskState::Answered);
    let session = f.session.lock().unwrap();
    assert_eq!(
        session.human().unwrap().holds[&first.hold].recipients,
        before
    );
    let active = session
        .leases(later + 3)
        .values()
        .map(|l| l.fence())
        .collect::<Vec<_>>();
    let inbox = session.human().unwrap().inbox(&before, &active);
    assert_eq!(inbox.len(), 2);
    assert!(
        !inbox
            .iter()
            .find(|m| m.run == before[0].run)
            .unwrap()
            .actionable
    );
    assert!(
        inbox
            .iter()
            .find(|m| m.run == before[1].run)
            .unwrap()
            .actionable
    );
}

#[test]
fn relay_settles_wait_then_same_original_tick_keeps_receipt_and_concurrent_observer_receipt_immutable(
) {
    let mut f = claimed();
    let opened = call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &open("等待并发"),
        NOW + 20,
    );
    let wait = HumanCommand::AskWait {
        hold: opened.hold.clone(),
    };
    let id = uuid::Uuid::now_v7().to_string();
    let first = call(&mut f, &id, &wait, NOW + 21);
    let other_id = uuid::Uuid::now_v7().to_string();
    let other = call(&mut f, &other_id, &wait, NOW + 22);
    assert!(other.receipt.is_some());
    assert_eq!(other.slice, first.slice);
    call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &HumanCommand::Relay {
            hold: opened.hold.clone(),
            text: "现在结束".into(),
            self_reported: None,
        },
        NOW + 40,
    );
    let finished = call(&mut f, &id, &wait, NOW + 41);
    assert_eq!(finished.slice.unwrap().finished_ms, Some(NOW + 40));
    let observed = call(&mut f, &other_id, &wait, NOW + 42);
    assert_eq!(observed.receipt, other.receipt);
    assert_eq!(observed.current.state, AskState::Answered);
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "hold.wait_finished" && e.obj == opened.hold)
            .count(),
        1
    );
}

#[test]
fn invalid_empty_question_recommended_and_cross_kind_relay_are_refused_without_ask_events() {
    let mut f = claimed();
    let before = events(&f).len();
    for command in [
        open("？！"),
        HumanCommand::AskOpen {
            question: "valid".into(),
            options: vec![],
            recommended: Some(0),
            context: String::new(),
        },
        HumanCommand::Relay {
            hold: "H-missing".into(),
            text: String::new(),
            self_reported: None,
        },
    ] {
        assert!(ProjectSession::ask_shared(
            &f.session,
            &f.catalog,
            "host",
            &uuid::Uuid::now_v7().to_string(),
            &command,
            None,
            &mut f.outbox,
            || Ok(NOW + 20)
        )
        .is_err());
    }
    assert_eq!(events(&f).len(), before);
}

fn interrupt_decision_after_open(f: &mut Fixture, id: &str) {
    let session = f.session.clone();
    let error=ProjectSession::ask_shared(&f.session,&f.catalog,"host",id,&open("权威问题!"),None,&mut f.outbox,|| {
        let live=session.lock().unwrap();
        if live.human().unwrap().holds.values().any(|h|matches!(&h.body,towermd_core::human_intervention::HoldBody::Ask{question,..} if question=="权威问题!")) {Err("injected process interruption after actual opening".into())}else{Ok(NOW+20)}
    }).unwrap_err();
    assert!(error.contains(id) && error.contains("uncertain"));
    let recorded = events(f);
    assert_eq!(
        recorded
            .iter()
            .filter(|e| e.kind == "hold.opened" && e.data["p3_request"]["request_id"] == id)
            .count(),
        1
    );
    assert!(!recorded
        .iter()
        .any(|e| e.kind == "hold.answered" && e.data["p3_request"]["request_id"] == id));
}
#[test]
fn crash_after_actual_decision_opening_recovers_same_plan_and_uuid_without_a_second_opening() {
    let mut f = claimed_with_decision(true);
    let id = uuid::Uuid::now_v7().to_string();
    interrupt_decision_after_open(&mut f, &id);
    let original = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.opened" && e.data["p3_request"]["request_id"] == id)
        .unwrap();
    f = reopen(f, NOW + 21);
    let resumed = call(&mut f, &id, &open("权威问题!"), NOW + 22);
    assert_eq!(resumed.current.state, AskState::Answered);
    let answer = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.answered" && e.data["p3_request"]["request_id"] == id)
        .unwrap();
    assert_eq!(answer.data["hold"]["reply"], original.data["planned_reply"]);
    assert_eq!(answer.data["request_body"], original.data["request_body"]);
    assert_eq!(
        events(&f)
            .iter()
            .filter(|e| e.kind == "hold.opened" && e.data["p3_request"]["request_id"] == id)
            .count(),
        1
    );
}
#[test]
fn changed_or_deprecated_actual_decision_does_not_replan_an_opened_original() {
    for changed in ["decision: 改后的答案", "status: deprecated"] {
        let mut f = claimed_with_decision(true);
        let id = uuid::Uuid::now_v7().to_string();
        interrupt_decision_after_open(&mut f, &id);
        let original = events(&f)
            .into_iter()
            .find(|e| e.kind == "hold.opened" && e.data["p3_request"]["request_id"] == id)
            .unwrap();
        let main = git(f.root.path(), &["rev-parse", "main"]);
        // Deliberate external authority change in a disposable repo; this is
        // a refusal fixture, not a fabricated approved baseline transition.
        git(f.root.path(), &["switch", "main"]);
        let path = f.root.path().join("docs/tower/decisions/D-ask.md");
        let text = std::fs::read_to_string(&path).unwrap();
        let text = if changed.starts_with("decision:") {
            text.replace("decision: 使用仓库答案", changed)
        } else {
            text.replace("status: stable", changed)
        };
        std::fs::write(path, text).unwrap();
        git(f.root.path(), &["add", "docs/tower/decisions/D-ask.md"]);
        git(
            f.root.path(),
            &["commit", "-qm", "external authority race fixture"],
        );
        git(f.root.path(), &["switch", "t/T-ask"]);
        f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
        let error = ProjectSession::ask_shared(
            &f.session,
            &f.catalog,
            "host",
            &id,
            &open("权威问题!"),
            None,
            &mut f.outbox,
            || Ok(NOW + 22),
        )
        .unwrap_err();
        assert!(
            error.contains("Decision") || error.contains("uncertain"),
            "{error}"
        );
        assert!(!events(&f)
            .iter()
            .any(|e| e.kind == "hold.answered" && e.data["p3_request"]["request_id"] == id));
        assert_eq!(
            events(&f).iter().find(|e| e.eid == original.eid).unwrap(),
            &original
        );
        git(f.root.path(), &["update-ref", "refs/heads/main", &main]);
        f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
        let recovered = call(&mut f, &id, &open("权威问题!"), NOW + 23);
        assert_eq!(recovered.current.reply.unwrap().text, "使用仓库答案");
    }
}

fn rebuild_uncheckpointed_ask(keep_original: bool) {
    use towermd_core::{hub_protocol::Event, recovery, snapshot_service};
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
    let id = uuid::Uuid::now_v7().to_string();
    let command = open("仅实际outer可恢复");
    let opened = call(&mut f, &id, &command, NOW + 20);
    let wait_id = uuid::Uuid::now_v7().to_string();
    call(
        &mut f,
        &wait_id,
        &HumanCommand::AskWait {
            hold: opened.hold.clone(),
        },
        NOW + 20,
    );
    let answer_id = uuid::Uuid::now_v7().to_string();
    let relay = HumanCommand::Relay {
        hold: opened.hold.clone(),
        text: "保留原回执，不复活运行".into(),
        self_reported: None,
    };
    let answered = call(&mut f, &answer_id, &relay, NOW + 21);
    let original = events(&f)
        .into_iter()
        .filter(|e| e.obj == opened.hold)
        .collect::<Vec<_>>();
    assert_eq!(original.len(), 3);
    assert!(original.iter().all(|e| e.seq > source.snapshot.through_seq));
    if !keep_original {
        for entry in std::fs::read_dir(f.runtime.path().join("outbox")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let kept = text
                .lines()
                .filter(|line| {
                    let value: serde_json::Value = serde_json::from_str(line).unwrap();
                    !(value["state"] == "confirmed"
                        && original.iter().any(|e| value["original"]["eid"] == e.eid))
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(path, format!("{kept}\n")).unwrap();
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
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let saved = snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let outer = saved
        .archives
        .values()
        .flat_map(|text| {
            text.lines()
                .map(|line| serde_json::from_str::<Event>(line).unwrap())
        })
        .collect::<Vec<_>>();
    assert!(original.iter().all(|old| outer
        .iter()
        .any(|e| e.eid == old.eid && e.stale && e.data == old.data && e.guards == old.guards)));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&opened.hold));
    let cursor = f.session.lock().unwrap().applied_seq();
    let before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 41)
        .unwrap();
    let request = ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        &command,
        None,
        &mut f.outbox,
        || Ok(NOW + 41),
    );
    let answer = ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        &answer_id,
        &relay,
        None,
        &mut f.outbox,
        || Ok(NOW + 41),
    );
    if keep_original {
        let request = request.unwrap();
        let answer = answer.unwrap();
        assert_eq!(request.receipt, opened.receipt);
        assert_eq!(answer.receipt, answered.receipt);
        assert_eq!(request.current.state, AskState::Expired);
        assert_eq!(answer.current.state, AskState::Expired);
        assert!(!request.actionable && !answer.actionable);
    } else {
        for error in [request.unwrap_err(), answer.unwrap_err()] {
            assert!(
                error.contains("uncertain") && error.contains("stale outer replay"),
                "{error}"
            );
        }
    }
    ProjectSession::human_tick_shared(&f.session, &f.catalog, "host", &mut f.outbox, || {
        Ok(NOW + 1_800_050)
    })
    .unwrap();
    assert!(!f
        .session
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .holds
        .contains_key(&opened.hold));
    let after = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 1_800_050)
        .unwrap();
    assert_eq!(
        after
            .events
            .iter()
            .filter(|e| e.kind.starts_with("hold."))
            .collect::<Vec<_>>(),
        before
            .events
            .iter()
            .filter(|e| e.kind.starts_with("hold."))
            .collect::<Vec<_>>(),
        "historical own receipt/metadata never grants a new ordinary Ask event"
    );
}
#[test]
fn actual_rebuild_retains_own_receipts_but_never_rehydrates_stale_ask_as_effective() {
    rebuild_uncheckpointed_ask(true);
}
#[test]
fn actual_rebuild_stale_outer_without_original_receipt_keeps_old_uuid_uncertain() {
    rebuild_uncheckpointed_ask(false);
}
