//! Real binary common prelude before both success and refusal. Actual B0 and
//! fixed process/App routes supply the facts; no injected Reply/Review facts.
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
    fs,
    process::Stdio,
    time::{Duration, Instant},
};
use towermd_core::ask_answer_contract::{AppAnswerAction, AskAnswerView};

fn endpoint(f: &Fixture) -> Value {
    serde_json::from_slice(&fs::read(f.runtime.path().join("endpoint.json")).unwrap()).unwrap()
}
fn url(endpoint: &Value, path: &str) -> String {
    format!(
        "http://127.0.0.1:{}{path}",
        endpoint["port"].as_u64().unwrap()
    )
}
fn http(endpoint: &Value) -> reqwest::blocking::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-towermd-instance",
        endpoint["instance"].as_str().unwrap().parse().unwrap(),
    );
    headers.insert("x-towermd-client", "app".parse().unwrap());
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(120))
        .default_headers(headers)
        .build()
        .unwrap()
}
fn source_changed(error: &str) -> bool {
    [
        "authority changed; reload Catalog",
        "authority changed during cache probe",
        "authority changed during preparation",
        "private snapshot source changed during request",
    ]
    .iter()
    .any(|suffix| error.ends_with(suffix))
}
fn post_original(
    client: &reqwest::blocking::Client,
    endpoint: &Value,
    path: &str,
    body: &Value,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(120);
    for attempt in 0..3 {
        assert!(
            Instant::now() < deadline,
            "original HTTP request budget exhausted"
        );
        let response = client
            .post(url(endpoint, path))
            .json(body)
            .timeout(deadline.saturating_duration_since(Instant::now()))
            .send()
            .unwrap();
        let code = response.status();
        let value: Value = response.json().unwrap();
        if code.is_success() {
            return value;
        }
        assert!(
            attempt < 2
                && value["code"] == "request_uncertain"
                && value["request_id"] == body["request_id"]
                && source_changed(value["error"].as_str().unwrap_or("")),
            "original request {:?}, attempt {}: {code}: {value}",
            body["request_id"],
            attempt + 1
        );
    }
    unreachable!()
}
fn answer_view(
    client: &reqwest::blocking::Client,
    endpoint: &Value,
    f: &Fixture,
    hold: &str,
) -> AskAnswerView {
    let deadline = Instant::now() + Duration::from_secs(120);
    for attempt in 0..3 {
        assert!(
            Instant::now() < deadline,
            "original HTTP read budget exhausted"
        );
        let response = client
            .get(url(endpoint, &format!("/app/interventions/asks/{hold}")))
            .query(&[("directory", f.root.path().to_string_lossy())])
            .timeout(deadline.saturating_duration_since(Instant::now()))
            .send()
            .unwrap();
        let code = response.status();
        let value: Value = response.json().unwrap();
        if code.is_success() {
            return serde_json::from_value(value).unwrap();
        }
        assert!(
            attempt < 2 && source_changed(value["error"].as_str().unwrap_or("")),
            "attempt {}: {code}: {value}",
            attempt + 1
        );
    }
    unreachable!()
}
// Only this already pure Task read is retried. A message may have been printed
// and acknowledged before the body encountered source drift, so retain every
// actual stdout byte and validate the combined stable IDs, never just the last
// invocation. Unknown errors, partial bodies and duplicate delivery still fail.
fn task_show(f: &Fixture, expected_ids: &[&str]) -> std::process::Output {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut ids = Vec::new();
    for attempt in 0..3 {
        let mut output = f.command(&["task", "show"]).output().unwrap();
        stdout.extend_from_slice(&output.stdout);
        stderr.extend_from_slice(&output.stderr);
        let (messages, body) = cli_output::split(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid actual stdout: {error}; all stdout={} all stderr={}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            )
        });
        ids.extend(messages.iter().map(|message| message.id.clone()));
        assert!(
            ids.iter().all(|id| expected_ids.contains(&id.as_str())),
            "unexpected actual message IDs: {ids:?}; all stdout={}",
            String::from_utf8_lossy(&stdout)
        );
        if output.status.success() {
            assert_eq!(
                ids.iter().map(String::as_str).collect::<Vec<_>>(),
                expected_ids,
                "all stdout from every attempt is retained: {}",
                String::from_utf8_lossy(&stdout)
            );
            output.stdout = stdout;
            output.stderr = stderr;
            return output;
        }
        assert!(
            attempt < 2
                && body.is_empty()
                && output.status.code() == Some(1)
                && output.stderr == b"towermd: authority changed during cache probe\n",
            "task show attempt {}: status={} all stdout={} all stderr={}",
            attempt + 1,
            output.status,
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
    }
    unreachable!()
}
fn actual_open_answer(f: &Fixture, question: &str, text: &str) -> String {
    let endpoint = endpoint(f);
    let http = http(&endpoint);
    let open_body = json!({"directory":f.root.path(),"run":null,"request_id":uuid::Uuid::now_v7().to_string(),
        "command":{"action":"ask_open","question":question,"options":[],"recommended":null,"context":"actual inbox CLI regression"}});
    let opened = post_original(&http, &endpoint, "/interaction", &open_body);
    let hold = opened["hold"].as_str().unwrap();
    let view = answer_view(&http, &endpoint, f, hold);
    let answer_body = json!({"directory":f.root.path(),"request_id":uuid::Uuid::now_v7().to_string(),"human_id":"delivery-cli-owner",
        "action":AppAnswerAction::Answer{hold:hold.into(),opening:view.binding,text:text.into()}});
    let answered = post_original(&http, &endpoint, "/app/interventions/action", &answer_body);
    format!(
        "{}/{}/{}",
        hold,
        answered["receipt"]["result_ref"]["reply"]["id"]
            .as_str()
            .unwrap(),
        view.recipients[0].recipient.run
    )
}
#[test]
fn next_non_status_cli_and_even_refused_command_print_then_confirm_stable_ids_before_body() {
    let f = Fixture::new(&[]);
    let first = actual_open_answer(
        &f,
        "next non-status command",
        "first exact answer\nsecond line",
    );
    let output = task_show(&f, &[first.as_str()]);
    let (messages, rest) = cli_output::split(&output.stdout).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, first);
    assert_eq!(messages[0].text, "first exact answer\nsecond line");
    let body: Value = serde_json::from_slice(rest).unwrap();
    assert_eq!(body["task"], "T-finish");
    let deliveries: Vec<_> = f
        .outbox()
        .confirmed()
        .iter()
        .filter(|e| e.kind == "prompt.delivered")
        .cloned()
        .collect();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].actor, "process:towermd-cli");
    assert_eq!(deliveries[0].data["message_ids"], json!([first]));
    let second = actual_open_answer(
        &f,
        "message precedes command refusal",
        "still deliver before unknown task",
    );
    let refused = f.command(&["task", "show", "T-missing"]).output().unwrap();
    assert!(!refused.status.success());
    let (messages, rest) = cli_output::split(&refused.stdout).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, second);
    assert!(rest.is_empty());
    assert!(!refused.stderr.is_empty());
    let before = f
        .outbox()
        .confirmed()
        .iter()
        .filter(|e| e.kind == "prompt.delivered")
        .count();
    assert_eq!(before, 2);
    f.success(&["daemon", "stop"]);
    let after = task_show(&f, &[]);
    assert!(cli_output::split(&after.stdout).unwrap().0.is_empty());
    assert_eq!(
        f.outbox()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "prompt.delivered")
            .count(),
        before
    );
}
#[test]
fn actual_cli_broken_stdout_leaves_message_unconfirmed_and_next_cli_repeats_same_id() {
    let f = Fixture::new(&[]);
    let id = actual_open_answer(&f, "broken stdout", "must not disappear before flush");
    let mut child = f
        .command(&["task", "show"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(f
        .outbox()
        .confirmed()
        .iter()
        .all(|event| event.kind != "prompt.delivered"));
    let next = task_show(&f, &[id.as_str()]);
    let (messages, body) = cli_output::split(&next.stdout).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, id);
    assert!(serde_json::from_slice::<Value>(body).is_ok());
    assert_eq!(
        f.outbox()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "prompt.delivered")
            .count(),
        1
    );
}
#[test]
fn missing_hub_rebuild_warns_unavailable_without_creating_hub_and_offline_schema_never_starts_daemon(
) {
    let f = Fixture::new(&[]);
    f.snapshot_before_new_finish("T-finish");
    f.success(&["daemon", "stop"]);
    let catalog = towermd_core::catalog::Catalog::load(f.root.path()).unwrap();
    let store = catalog.common_dir.join("towermd");
    fs::remove_dir_all(&store).unwrap();
    let result = f.success(&["hub", "rebuild"]);
    assert!(String::from_utf8_lossy(&result.stderr).contains("inbox unavailable"));
    let (_, body) = cli_output::split(&result.stdout).unwrap();
    let request: Value = serde_json::from_slice(body).unwrap();
    assert!(request["request_id"].is_string());
    assert!(
        !store.exists(),
        "prelude and request cannot implicitly rebuild/create Hub"
    );
    f.success(&["daemon", "stop"]);
    let before = fs::read(f.runtime.path().join("endpoint.json")).ok();
    let schema = f.success(&["baseline", "schema"]);
    assert!(cli_output::split(&schema.stdout).unwrap().0.is_empty());
    assert!(serde_json::from_slice::<Value>(&schema.stdout)
        .unwrap()
        .get("$schema")
        .is_some());
    assert_eq!(
        fs::read(f.runtime.path().join("endpoint.json")).ok(),
        before
    );
}

#[test]
fn main_worktree_without_bound_run_keeps_status_add_and_existing_init_outputs_empty_of_messages() {
    let f = Fixture::new(&[]);
    let id = actual_open_answer(&f, "old task branch reply", "not for main branch");
    support::git(f.root.path(), &["switch", "main"]);
    for args in [
        vec!["status"],
        vec![
            "task",
            "add",
            "--id",
            "T-next",
            "--title",
            "no bound Run",
            "--module",
            "A-alpha",
            "--allow",
            "alpha/**",
            "--acceptance",
            "AC-finish",
        ],
        vec!["init", "--docs-dir", "docs/tower"],
    ] {
        let output = f.success(&args);
        let (messages, body) = cli_output::split(&output.stdout).unwrap();
        assert!(
            messages.is_empty(),
            "no Run in this worktree/branch can consume another scope's reply"
        );
        if args[0] == "init" {
            let catalog = towermd_core::catalog::Catalog::load(f.root.path()).unwrap();
            assert!(std::str::from_utf8(body)
                .unwrap()
                .contains(catalog.config.project_id()));
        } else {
            assert!(serde_json::from_slice::<Value>(body).is_ok());
        }
    }
    assert!(f
        .outbox()
        .confirmed()
        .iter()
        .all(|event| event.kind != "prompt.delivered"));
    support::git(f.root.path(), &["switch", "t/T-finish"]);
    let output = task_show(&f, &[id.as_str()]);
    assert_eq!(cli_output::split(&output.stdout).unwrap().0.len(), 1);
}

#[test]
fn existing_init_stays_offline_and_never_consumes_a_bound_runs_answer() {
    let f = Fixture::new(&[]);
    let id = actual_open_answer(
        &f,
        "offline init must not deliver",
        "pending until a project command",
    );
    f.success(&["daemon", "stop"]);
    // Remove only this fixture's stale endpoint marker. Re-entry must not
    // connect, start a daemon, create an endpoint or append a delivery fact.
    fs::remove_file(f.runtime.path().join("endpoint.json")).unwrap();
    let cursor = f.outbox().confirmed().len();
    let output = f.success(&["init", "--docs-dir", "docs/tower"]);
    assert!(cli_output::split(&output.stdout).unwrap().0.is_empty());
    assert!(!f.runtime.path().join("endpoint.json").exists());
    assert_eq!(f.outbox().confirmed().len(), cursor);
    let output = task_show(&f, &[id.as_str()]);
    let (messages, body) = cli_output::split(&output.stdout).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, id);
    assert!(serde_json::from_slice::<Value>(body).is_ok());
}
