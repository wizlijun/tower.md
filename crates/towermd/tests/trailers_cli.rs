//! Actual CLI -> daemon -> core terminal G5 -> real Git main integration.
//! No report, raw projection, caller proof or fake passed Event authorizes a merge.
#[allow(dead_code)]
#[path = "support/finish_cli_fixture.rs"]
mod cli_fixture;
#[path = "support/cli_output.rs"]
mod cli_output;
#[allow(dead_code)]
#[path = "support/finish_fixture.rs"]
mod support;
use cli_fixture::Fixture;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs, thread,
    time::{Duration, Instant},
};
use towermd_core::{
    hub_protocol::Event,
    recovery_contract::recorded::{GateEvidence, GateResult},
    recovery_git::{parse_trailer_context, TrailerContext},
    task_gates::Gate,
};

fn endpoint(f: &Fixture) -> Value {
    serde_json::from_slice(&fs::read(f.runtime.path().join("endpoint.json")).unwrap()).unwrap()
}
fn passed(f: &Fixture) -> Value {
    f.snapshot_before_new_finish("T-finish");
    support::git(f.root.path(), &["rebase", "main"]);
    fs::write(f.root.path().join("alpha/a.py"), "# actual Task change\n").unwrap();
    support::git(f.root.path(), &["add", "alpha/a.py"]);
    support::git(f.root.path(), &["commit", "-qm", "actual Task code"]);
    let queued = f.json(&["finish"]);
    let done = f.json(&["finish", "--wait", queued["evidence_id"].as_str().unwrap()]);
    assert_eq!(done["evidence"]["result"], "passed");
    assert_eq!(done["run_state"], "verifying");
    done
}
fn trailers(f: &Fixture, done: &Value) -> String {
    let output = f.success(&["trailers", "T-finish"]);
    let (_, command_stdout) = cli_output::split(&output.stdout).unwrap();
    let text = std::str::from_utf8(command_stdout).unwrap().to_owned();
    assert_eq!(
        text.lines().count(),
        5,
        "stdout contains only the core renderer"
    );
    let TrailerContext::Task(parsed) = parse_trailer_context(&text).unwrap() else {
        panic!("not actual Task trailers");
    };
    assert_eq!(parsed.task, "T-finish");
    assert_eq!(parsed.module, "A-alpha");
    assert_eq!(parsed.run, done["input"]["run_id"].as_str().unwrap());
    assert_eq!(parsed.evidence, done["evidence_id"].as_str().unwrap());
    assert!(parsed.epoch > 0);
    text
}
fn ff_main(f: &Fixture, footer: &str) -> String {
    let tree = support::git(f.root.path(), &["rev-parse", "HEAD^{tree}"]);
    support::git(
        f.root.path(),
        &[
            "commit",
            "--amend",
            "-qm",
            &format!("actual integration\n\n{footer}"),
        ],
    );
    assert_eq!(
        support::git(f.root.path(), &["rev-parse", "HEAD^{tree}"]),
        tree
    );
    let head = support::git(f.root.path(), &["rev-parse", "HEAD"]);
    let old = support::git(f.root.path(), &["rev-parse", "main"]);
    support::git(f.root.path(), &["merge-base", "--is-ancestor", &old, &head]);
    support::git(
        f.root.path(),
        &["update-ref", "refs/heads/main", &head, &old],
    );
    head
}
fn wait_actual(f: &Fixture, predicate: impl Fn(&Event) -> bool) -> Event {
    wait_actual_until(f, predicate, Instant::now() + Duration::from_secs(90))
}
fn wait_actual_until(f: &Fixture, predicate: impl Fn(&Event) -> bool, deadline: Instant) -> Event {
    loop {
        // Physical actual confirmation is observed without a command that can
        // schedule/authorize main certification. The existing daemon ticker
        // must do the normal background work on its own.
        if let Some(actual) = f
            .outbox()
            .confirmed()
            .iter()
            .find(|e| predicate(e))
            .cloned()
        {
            return actual;
        }
        assert!(
            Instant::now() < deadline,
            "normal main observer did not durably record its actual result"
        );
        thread::sleep(Duration::from_millis(100));
    }
}
#[test]
fn actual_cli_trailers_and_background_main_observer_complete_only_real_evidence() {
    let f =
        Fixture::new(&["printf x >> \"$TOWERMD_RUNTIME_DIR/finish-count\"; test -f alpha/a.py"]);
    let done = passed(&f);
    let before = f.outbox();
    let completed: Event = serde_json::from_value(done["actual"].clone()).unwrap();
    assert_eq!(completed.kind, "finish.completed");
    assert!(before.confirmed().contains(&completed));
    let footer = trailers(&f, &done);
    let gates = f
        .outbox()
        .confirmed()
        .iter()
        .filter(|e| e.kind == "evidence.recorded" && e.data.get("completed_eid").is_some())
        .count();
    assert!(gates > 0, "trailers must first durably record terminal G5");
    assert_eq!(trailers(&f, &done), footer);
    assert_eq!(
        f.outbox()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "evidence.recorded" && e.data.get("completed_eid").is_some())
            .count(),
        gates,
        "same actual context repairs/reuses its original gate"
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    let integrated = ff_main(&f, &footer);
    let merged = wait_actual_until(
        &f,
        |e| e.kind == "merge.done" && e.data["task"]["id"] == "T-finish",
        deadline,
    );
    assert!(!merged.stale);
    assert_eq!(merged.data["commit"], integrated);
    assert_eq!(merged.data["task"]["state"], "merged");
    assert_eq!(merged.data["run"]["id"], done["input"]["run_id"]);
    assert_eq!(merged.data["run"]["state"], "ended");
    let after = f.outbox();
    assert_eq!(
        after
            .confirmed()
            .iter()
            .filter(|e| e.kind == "merge.done")
            .count(),
        1
    );
    assert!(
        after.confirmed().contains(&completed),
        "G3 receipt is immutable"
    );
    assert_eq!(
        fs::read(f.runtime.path().join("finish-count")).unwrap(),
        b"x"
    );
    // Confirmed bytes precede the observer's final Session installation. Status
    // reads only the last verified prefix and must not schedule that installation.
    // Both phases share the original absolute budget, starting before main moves.
    loop {
        let status = f.json(&["status"]);
        let task_merged = status["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "T-finish" && t["state"] == "merged");
        let run_ended = status["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == done["input"]["run_id"] && r["state"] == "ended");
        if task_merged && run_ended && status["pending_verification"] != true {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "normal main observer did not install its verified canonical Task/Run: {status}"
        );
        thread::sleep(Duration::from_millis(100));
    }
    f.json(&[
        "task",
        "add",
        "--id",
        "T-next",
        "--title",
        "dependent",
        "--module",
        "A-alpha",
        "--allow",
        "alpha/**",
        "--acceptance",
        "AC-finish",
        "--depends-on",
        "T-finish",
    ]);
    support::git(f.root.path(), &["switch", "-qc", "t/T-next", "main"]);
    let next = f.json(&["task", "claim", "T-next"]);
    assert_ne!(next["id"], done["input"]["run_id"]);
    assert_eq!(next["state"], "running");
}
#[test]
fn actual_claim_without_finish_cannot_mint_evidence_via_main_footer() {
    let f = Fixture::new(&["test -f alpha/a.py"]);
    f.snapshot_before_new_finish("T-finish");
    support::git(f.root.path(), &["rebase", "main"]);
    let run = f.json(&["task", "show"]);
    let main_fence = run["leases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fence| fence["name"] == "module:A-alpha")
        .unwrap();
    fs::write(f.root.path().join("alpha/a.py"), "# unverified change\n").unwrap();
    support::git(f.root.path(), &["add", "alpha/a.py"]);
    support::git(f.root.path(), &["commit", "-qm", "unverified code"]);
    let footer = format!("Towermd-Task: T-finish\nTowermd-Run: {}\nTowermd-Module: A-alpha\nTowermd-Evidence: E-{}\nTowermd-Epoch: {}\n", run["id"].as_str().unwrap(), uuid::Uuid::now_v7(), main_fence["epoch"]);
    let candidate = ff_main(&f, &footer);
    let failure = wait_actual(&f, |e| {
        e.kind == "evidence.recorded" && e.data["main_gate_failure"]["candidate"] == candidate
    });
    assert!(!failure.stale);
    assert_eq!(failure.actor, "process:towermd-gates");
    assert_eq!(
        failure.data["main_gate_failure"]["rejection"],
        "missing_started"
    );
    assert_eq!(
        failure.data["main_gate_failure"]["claim"]["data"]["run"]["id"],
        run["id"]
    );
    assert_eq!(
        failure.data["main_gate_failure"]["hold"]["kind"],
        "gatefailed"
    );
    assert!(f
        .outbox()
        .confirmed()
        .iter()
        .all(|e| e.kind != "merge.done" && e.kind != "finish.completed"));
    let status = f.json(&["status"]);
    assert!(status["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["id"] != "T-finish" || t["state"] != "merged"));
}
#[test]
fn trailers_http_rejects_caller_authority_and_cli_has_no_merge_or_actor_bypass() {
    let f = Fixture::new(&["test -f alpha/a.py"]);
    let endpoint = endpoint(&f);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{}/trailers", endpoint["port"]);
    let body = json!({"directory":f.root.path(),"task":"T-finish","run":null});
    let before = f.outbox().confirmed().to_vec();
    for key in [
        "actor",
        "proof",
        "evidence",
        "candidate",
        "request_id",
        "skip",
    ] {
        let mut forged = body.clone();
        forged[key] = json!("caller-controlled");
        let response = client
            .post(&url)
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .json(&forged)
            .send()
            .unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::UNPROCESSABLE_ENTITY,
            "{key}"
        );
    }
    let response = client
        .post(&url)
        .header("x-towermd-instance", "old-instance")
        .json(&body)
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    for args in [
        vec!["trailers", "T-finish", "--actor", "human:owner"],
        vec!["trailers", "T-finish", "--skip-g4"],
        vec!["trailers", "T-finish", "--evidence", "E-fake"],
        vec!["task", "merged", "T-finish"],
    ] {
        assert!(!f.command(&args).output().unwrap().status.success());
    }
    assert_eq!(f.outbox().confirmed(), before);
    let no_evidence = f.command(&["trailers", "T-finish"]).output().unwrap();
    assert!(!no_evidence.status.success());
    assert!(cli_output::split(&no_evidence.stdout).unwrap().1.is_empty());
}

fn high_risk_fixture() -> Fixture {
    use towermd_core::{
        baseline_review::review_baseline_shared, recovery_contract::baseline::Verdict,
        snapshot_service::PublicationReport,
    };
    let mut original = support::configured_fixture(&[("docs/tower/specs/AC-finish.md", "---\ntype: Acceptance\nid: AC-finish\nmodule: A-alpha\ncriteria: [actual high risk App review]\n---\n")], &["printf x >> \"$TOWERMD_RUNTIME_DIR/finish-count\"; test -f alpha/a.py"], true);
    let action = support::action(&original, Verdict::Approved);
    let approval = review_baseline_shared(
        &original.session,
        &original.catalog,
        &mut original.outbox,
        &action,
        "human:owner",
        "host",
        support::now() + 3,
    )
    .unwrap();
    assert!(matches!(
        original
            .writer
            .publish_baseline(
                &original.session,
                &original.catalog,
                &mut original.outbox,
                &approval.receipt.result_ref.approval_eid,
                "host",
                || Ok(support::now() + 5)
            )
            .unwrap(),
        PublicationReport::Recorded { .. }
    ));
    support::git(
        original.root.path(),
        &["switch", "-qc", "t/T-finish", "main"],
    );
    let support::Fixture {
        root,
        runtime,
        session,
        outbox,
        ..
    } = original;
    drop(session);
    drop(outbox);
    fs::write(runtime.path().join("host.json"), "\"host\"").unwrap();
    let f = Fixture { root, runtime };
    f.json(&[
        "task",
        "add",
        "--id",
        "T-finish",
        "--title",
        "high risk actual trailers",
        "--module",
        "A-alpha",
        "--allow",
        "alpha/**",
        "--acceptance",
        "AC-finish",
    ]);
    f.json(&["task", "claim", "T-finish"]);
    f
}
// This is the supported fixed App program boundary, not a GUI/manual review
// claim. It submits only an actual producer's binding; no human event is forged.
fn approve_actual_review(f: &Fixture, opening: &Value) -> Value {
    let endpoint = endpoint(f);
    let binding = &opening["receipt"]["result_ref"]["binding"];
    let hold = opening["receipt"]["result_ref"]["hold"].as_str().unwrap();
    let body = json!({"directory":f.root.path(),"request_id":uuid::Uuid::now_v7().to_string(),"human_id":"owner","action":{"kind":"review","hold":hold,"binding":binding,"verdict":"approved","comment":"actual App program boundary reviewed exact subject"}});
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!(
            "http://127.0.0.1:{}/app/interventions/action",
            endpoint["port"]
        ))
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .header("x-towermd-client", "app")
        .json(&body)
        .send()
        .unwrap();
    let code = response.status();
    let outcome: Value = response.json().unwrap();
    assert_eq!(code, reqwest::StatusCode::OK, "{outcome}");
    assert_eq!(outcome["current_validity"]["state"], "current");
    outcome
}
#[test]
fn high_risk_trailers_requires_actual_app_g4_without_rewriting_sealed_g3() {
    let f = high_risk_fixture();
    let refused = f.command(&["finish"]).output().unwrap();
    assert!(!refused.status.success(), "G1 is not automatic");
    assert!(!f.runtime.path().join("finish-count").exists());
    f.snapshot_before_new_finish("T-finish");
    support::git(f.root.path(), &["rebase", "main"]);
    let plan = f.runtime.path().join("original-plan.md");
    fs::write(
        &plan,
        "Actual original high risk plan\nKeep the frozen gates intact.\n",
    )
    .unwrap();
    let g1 = f.json(&[
        "review",
        "request",
        "--gate",
        "G1",
        "--content",
        plan.to_str().unwrap(),
    ]);
    approve_actual_review(&f, &g1);
    let done = passed(&f);
    // The sealed report records only gates actually checked by finish. G4 is
    // a later independent Event, not a false/nullable entry in this map.
    let sealed_gates: BTreeMap<String, bool> =
        serde_json::from_value(done["evidence"]["gates"].clone()).unwrap();
    let expected_gates = BTreeMap::from([
        ("G0".to_owned(), true),
        ("G2".to_owned(), true),
        ("G3".to_owned(), true),
        ("G5".to_owned(), true),
    ]);
    assert_eq!(sealed_gates, expected_gates);
    assert!(!sealed_gates.contains_key("G4"));
    let original_g3: Event = serde_json::from_value(done["actual"].clone()).unwrap();
    let no_g4 = f.command(&["trailers", "T-finish"]).output().unwrap();
    assert!(!no_g4.status.success());
    assert!(cli_output::split(&no_g4.stdout).unwrap().1.is_empty());
    let g4 = f.json(&["review", "request", "--gate", "G4"]);
    let g4_answer = approve_actual_review(&f, &g4);
    let footer = trailers(&f, &done);
    assert!(f.outbox().confirmed().contains(&original_g3));
    let original_gates: BTreeMap<String, bool> =
        serde_json::from_value(original_g3.data["evidence"]["gates"].clone()).unwrap();
    assert_eq!(original_gates, expected_gates);
    let confirmed = f.outbox();
    let g4_events = confirmed
        .confirmed()
        .iter()
        .filter(|event| {
            !event.stale
                && event.kind == "evidence.recorded"
                && event.data["evidence"]["gate"] == "g4"
                && event.data["evidence"]["review"]["completed_eid"] == original_g3.eid
        })
        .collect::<Vec<_>>();
    assert_eq!(
        g4_events.len(),
        1,
        "G4 must be its own unique actual durable terminal gate"
    );
    let g4_event = g4_events[0];
    let gate: GateEvidence = serde_json::from_value(g4_event.data["evidence"].clone()).unwrap();
    assert_eq!(gate.gate, Gate::G4);
    assert_eq!(gate.result, GateResult::Passed);
    assert_eq!(g4_event.actor, "process:towermd-gates");
    assert_eq!(g4_event.guards, original_g3.guards);
    let review = gate.review.unwrap();
    assert_eq!(
        review.completed_eid.as_deref(),
        Some(original_g3.eid.as_str())
    );
    assert!(!review.opening.stale && !review.reviewed.stale);
    assert_eq!(
        review.opening.eid,
        g4["receipt"]["result_ref"]["opening_eid"]
    );
    assert_eq!(
        review.reviewed.eid,
        g4_answer["receipt"]["result_ref"]["fact"]["answer_eid"]
    );
    assert_eq!(review.opening.obj, g4["receipt"]["result_ref"]["hold"]);
    assert_eq!(review.reviewed.obj, review.opening.obj);
    assert_eq!(review.reviewed.actor, "human:owner");
    assert_eq!(
        review.reviewed.data["fact"],
        g4_answer["receipt"]["result_ref"]["fact"]
    );
    assert_eq!(
        review.reviewed.data["fact"]["binding"],
        g4["receipt"]["result_ref"]["binding"]
    );
    assert!(confirmed.confirmed().contains(&review.opening));
    assert!(confirmed.confirmed().contains(&review.reviewed));
    let candidate = ff_main(&f, &footer);
    let merged = wait_actual(&f, |e| {
        e.kind == "merge.done" && e.data["commit"] == candidate
    });
    assert_eq!(merged.data["run"]["state"], "ended");
    assert_eq!(
        merged.data["proof"]["completed"],
        serde_json::to_value(&original_g3).unwrap()
    );
    assert_eq!(
        fs::read(f.runtime.path().join("finish-count")).unwrap(),
        b"x"
    );
}
