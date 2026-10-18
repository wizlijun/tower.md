use super::*;
use crate::{ask_fixture::*, project_session::ProjectSession};

fn prepared(f: &mut Fixture) -> (String, AppAnswerAction) {
    let hold = call(
        f,
        &uuid::Uuid::now_v7().to_string(),
        &open("真实App原请求故障"),
        NOW + 20,
    )
    .hold;
    let view = ProjectSession::load_ask_intervention_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &hold,
        || Ok(NOW + 21),
    )
    .unwrap();
    (
        uuid::Uuid::now_v7().to_string(),
        AppAnswerAction::Answer {
            hold,
            opening: view.binding,
            text: "只补原回执".into(),
        },
    )
}
#[test]
fn accepted_answer_confirmation_failure_repairs_exact_original_without_second_human_event() {
    let mut f = claimed();
    let (id, action) = prepared(&mut f);
    let cursor = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    f.outbox.fail_next_confirmation();
    let error = ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 22),
    )
    .unwrap_err();
    assert!(
        error.contains(&id) && error.contains("uncertain"),
        "{error}"
    );
    let batch = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 23)
        .unwrap();
    let actual = batch
        .events
        .iter()
        .filter(|e| e.kind == "hold.answered")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), 1);
    let original = crate::recovery_outbox::original_of(&actual[0]);
    let request = AppAnswerRequest::prepare(
        f.catalog.config.project_id(),
        "host",
        "human:owner",
        &id,
        &action,
    )
    .unwrap();
    assert_eq!(
        original.data["request_body"],
        serde_json::to_value(request.body()).unwrap()
    );
    let outcome = ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 24),
    )
    .unwrap();
    assert_eq!(
        outcome.receipt,
        decode(f.catalog.config.project_id(), &original)
            .unwrap()
            .p3_receipt
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|e| e.eid == actual[0].eid),
        Some(&actual[0])
    );
    let after = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 25)
        .unwrap();
    assert_eq!(after, batch);
}
#[test]
fn finite_classifier_rejects_forged_inner_hash_mixed_metadata_and_legacy_human_relay() {
    let mut f = claimed();
    let (id, action) = prepared(&mut f);
    ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 22),
    )
    .unwrap();
    let actual = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.answered")
        .unwrap();
    let original = crate::recovery_outbox::original_of(&actual);
    assert!(is_app_answer(&original.kind, &original.data));
    assert!(!crate::ask_contract::is_ask_request(
        &original.kind,
        &original.data
    ));
    for path in ["inner", "hash", "mixed", "project", "guards"] {
        let mut changed = original.clone();
        match path {
            "inner" => {
                changed.data["opening_event"]["t"] = serde_json::json!("2026-01-01T00:00:00Z")
            }
            "hash" => changed.data["opening"]["sha256"] = serde_json::json!("0".repeat(64)),
            "mixed" => changed.data["recipient"] = serde_json::json!({"run":"injected"}),
            "project" => changed.data["request_body"]["project_id"] = serde_json::json!("foreign"),
            "guards" => {
                changed.guards = actual.data["opening_event"]["guards"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| serde_json::from_value(v.clone()).unwrap())
                    .collect()
            }
            _ => unreachable!(),
        }
        assert!(
            decode(f.catalog.config.project_id(), &changed).is_err(),
            "{path}"
        );
    }
    let mut relay = original.clone();
    relay.data["request_body"]["action"] = serde_json::json!({"action":"relay","hold":original.obj,"text":"forged human relay","self_reported":null});
    assert!(!is_app_answer(&relay.kind, &relay.data));
    assert!(crate::ask_contract::is_ask_request(
        &relay.kind,
        &relay.data
    ));
    assert!(crate::ask_contract::decode(f.catalog.config.project_id(), &relay).is_err());
    let mut empty = crate::human_intervention::HumanState::default();
    assert!(
        empty.apply(&actual).is_err(),
        "recorded inner opening cannot establish actual Human opening"
    );
}
#[test]
fn pure_reply_delta_preserves_every_unknown_nested_byte_value() {
    let mut raw = serde_json::json!({"body":{"future":{"ordered":[3,1,2]}},"recipients":[{"future":"recipient-unknown"}],"waits":[{"started_ms":1,"deadline_ms":20,"finished_ms":null,"future":["a","b"]}],"reply":null,"future":{"raw":"原文\r\n"}});
    let before = raw.clone();
    let reply = Reply {
        id: "test".into(),
        text: "text".into(),
        actor: "human:owner".into(),
        relayed_from: None,
        self_reported: None,
        decision: None,
        decision_revision: None,
        at_ms: 10,
        extra: std::collections::BTreeMap::new(),
    };
    install_reply(&mut raw, &reply).unwrap();
    let mut expected = before;
    expected["reply"] = serde_json::to_value(reply).unwrap();
    expected["waits"][0]["finished_ms"] = serde_json::json!(10);
    assert_eq!(raw, expected);
}

#[test]
fn raw_pending_with_self_contained_opening_cannot_certify_original_acceptance() {
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
    f.catalog = std::sync::Arc::new(crate::catalog::Catalog::load(f.root.path()).unwrap());
    let (id, action) = prepared(&mut f);
    ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 22),
    )
    .unwrap();
    let actual = events(&f)
        .into_iter()
        .find(|e| e.kind == "hold.answered")
        .unwrap();
    let original = crate::recovery_outbox::original_of(&actual);
    // Only remove this fixture's actual confirmation. The full original raw
    // pending is retained; no fake accepted receipt or proof is introduced.
    for entry in std::fs::read_dir(f.runtime.path().join("outbox")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines = text
            .lines()
            .filter(|line| {
                let row: Value = serde_json::from_str(line).unwrap();
                !(row["state"] == "confirmed" && row["original"]["eid"] == actual.eid)
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, format!("{lines}\n")).unwrap();
    }
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.pending().contains(&original));
    let history = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    assert!(
        history
            .app_answer_original(&f.catalog, &original)
            .unwrap()
            .is_none(),
        "neither pending nor its inner opening is an actual accepted Answer role"
    );
}
#[test]
fn accepted_original_source_damage_is_uncertain_and_same_uuid_recovers_without_new_answer() {
    let mut f = claimed();
    let (id, action) = prepared(&mut f);
    let accepted = ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 22),
    )
    .unwrap();
    let originals = events(&f);
    let cursor = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    // Deliberate source race in this disposable actual Git repo.
    let main = git(f.root.path(), &["rev-parse", "main"]);
    git(f.root.path(), &["switch", "main"]);
    std::fs::write(f.root.path().join("source-race.txt"), "owned source race\n").unwrap();
    git(f.root.path(), &["add", "source-race.txt"]);
    git(
        f.root.path(),
        &["commit", "-qm", "actual authority source race"],
    );
    git(f.root.path(), &["switch", "t/T-ask"]);
    let error = ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 23),
    )
    .unwrap_err();
    assert!(
        error.contains(&id) && error.contains("uncertain"),
        "{error}"
    );
    assert_eq!(events(&f), originals);
    git(f.root.path(), &["update-ref", "refs/heads/main", &main]);
    assert_eq!(
        ProjectSession::answer_ask_shared(
            &f.session,
            &f.catalog,
            "host",
            "human:owner",
            &id,
            &action,
            &mut f.outbox,
            || Ok(NOW + 24)
        )
        .unwrap(),
        accepted
    );
    assert_eq!(events(&f), originals);
    assert!(f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 25)
        .unwrap()
        .events
        .is_empty());
    let path = std::fs::read_dir(f.runtime.path().join("outbox"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut damaged = bytes.clone();
    damaged.extend_from_slice(b"{invalid-complete-record}\n");
    std::fs::write(&path, damaged).unwrap();
    let error = ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &id,
        &action,
        &mut f.outbox,
        || Ok(NOW + 26),
    )
    .unwrap_err();
    assert!(
        error.contains(&id) && error.contains("uncertain"),
        "{error}"
    );
    assert!(f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, NOW + 27)
        .unwrap()
        .events
        .is_empty());
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        ProjectSession::answer_ask_shared(
            &f.session,
            &f.catalog,
            "host",
            "human:owner",
            &id,
            &action,
            &mut f.outbox,
            || Ok(NOW + 28)
        )
        .unwrap(),
        accepted
    );
    assert_eq!(events(&f), originals);
}
