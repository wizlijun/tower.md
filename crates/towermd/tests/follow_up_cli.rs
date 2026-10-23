//! Follow-ups, quiet objects and resurfacing (FR-134–136): only a person in
//! the App adds or completes a follow-up or settles a resurfaced object; the
//! state read derives which follow-ups are due and which objects are quiet.
#[path = "support/envelope.rs"]
mod envelope;

use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::Duration,
};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Tower Test", "-c", "user.email=tower@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let fixture = Self { root: tempfile::tempdir().unwrap(), runtime: tempfile::tempdir().unwrap() };
        let root = fixture.root.path();
        git(root, &["init", "-qb", "main"]);
        fs::create_dir(root.join(".towermd")).unwrap();
        fs::write(root.join(".towermd/config.yaml"), "project_id: tower-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
        fs::write(root.join("README.md"), "fixture\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "fixture"]);
        fixture
    }
    fn output(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
            .args(args).current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null()).output().unwrap()
    }
    fn run(&self, args: &[&str]) -> Value {
        let output = self.output(args);
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stdout));
        envelope::data_checked(args, &output)
    }
    fn request(&self, endpoint: &Value, path: &str, client: &str, body: Option<Value>) -> (u16, Value) {
        let client_http = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap();
        let url = format!("http://127.0.0.1:{}{path}", endpoint["port"]);
        let request = match body {
            Some(body) => client_http.post(url).json(&body),
            None => client_http.get(url),
        }
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .header("x-towermd-client", client);
        let response = request.send().unwrap();
        let status = response.status().as_u16();
        (status, response.json().unwrap_or(Value::Null))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(&["daemon", "stop"]);
    }
}

fn request_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

#[test]
fn only_a_person_in_the_app_keeps_follow_ups_and_the_state_says_which_are_due() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let handle = fixture.request(&endpoint, "/projects/open", "app", Some(json!({"path":fixture.root.path()}))).1["handle"].as_str().unwrap().to_owned();
    let state = |handle: &str| fixture.request(&endpoint, &format!("/projects/{handle}/state"), "app", None).1;

    let add = format!("/projects/{handle}/follow-ups");
    let past = json!({"on":"date","at":"2026-01-01T00:00:00Z"});
    let first = request_id();
    let body = json!({"request_id":first,"text":"回头看看周报","when":past});
    let (status, refused) = fixture.request(&endpoint, &add, "cli", Some(body.clone()));
    assert_eq!((status, refused["code"].as_str()), (409, Some("conflict")), "the CLI cannot add one: {refused}");
    let (status, added) = fixture.request(&endpoint, &add, "app", Some(body.clone()));
    assert_eq!(status, 200, "{added}");
    assert_eq!((added["follow_up"]["added"]["actor"].as_str(), added["follow_up"]["when"]["on"].as_str()), (Some("human:local"), Some("date")));
    assert_eq!(fixture.request(&endpoint, &add, "app", Some(body)).1["seq"], added["seq"], "a retry is the same event");
    let (status, reused) = fixture.request(&endpoint, &add, "app", Some(json!({"request_id":first,"text":"另一句","when":past})));
    assert_eq!((status, reused["code"].as_str()), (400, Some("conflict")), "{reused}");
    let (status, _) = fixture.request(&endpoint, &add, "app", Some(json!({"request_id":request_id(),"text":"x","when":past,"actor":"human:other"})));
    assert!(status >= 400);
    let (status, missing) = fixture.request(&endpoint, &add, "app", Some(json!({"request_id":request_id(),"text":"x","when":{"on":"done","target":"T-none"}})));
    assert_eq!((status, missing["code"].as_str()), (400, Some("not_found")), "{missing}");
    let (status, _) = fixture.request(&endpoint, &add, "app", Some(json!({"request_id":request_id(),"text":"x","when":{"on":"date","at":"明天"}})));
    assert_eq!(status, 400);
    let quiet = state(&handle)["quiet"].clone();
    assert_eq!(quiet["dormant_after"], 14, "{quiet}");
    assert_eq!((quiet["objects"][0]["id"].as_str(), quiet["objects"][0]["quiet"].as_bool()), (Some("tower-project"), Some(false)));
    assert_eq!(quiet["due"], json!([first]), "a date in the past is due at once");

    // At most five are open; the project itself is a known target.
    let later = json!({"on":"activity","target":"tower-project"});
    for _ in 0..4 {
        let (status, body) = fixture.request(&endpoint, &add, "app", Some(json!({"request_id":request_id(),"text":"等项目有动静","when":later})));
        assert_eq!(status, 200, "{body}");
    }
    let (status, full) = fixture.request(&endpoint, &add, "app", Some(json!({"request_id":request_id(),"text":"第六条","when":later})));
    assert_eq!(status, 400, "{full}");
    assert!(full["error"].as_str().unwrap_or_default().contains("最多 5 条"), "{full}");

    // Completing is the App's too, once.
    let done = format!("/projects/{handle}/follow-ups/{first}/done");
    let finish = json!({"request_id":request_id()});
    assert_eq!(fixture.request(&endpoint, &done, "cli", Some(finish.clone())).0, 409);
    let (status, closed) = fixture.request(&endpoint, &done, "app", Some(finish.clone()));
    assert_eq!(status, 200, "{closed}");
    assert_eq!(closed["follow_up"]["done"]["actor"], "human:local");
    assert_eq!(fixture.request(&endpoint, &done, "app", Some(finish)).1["seq"], closed["seq"]);
    assert_eq!(fixture.request(&endpoint, &done, "app", Some(json!({"request_id":request_id()}))).0, 400);
    assert_eq!(state(&handle)["quiet"]["due"], json!([]));

    // Nothing resurfaced, so there is nothing to settle.
    let seen = format!("/projects/{handle}/resurfaced/tower-project/seen");
    assert_eq!(fixture.request(&endpoint, &seen, "cli", Some(json!({"request_id":request_id()}))).0, 409);
    let (status, body) = fixture.request(&endpoint, &seen, "app", Some(json!({"request_id":request_id()})));
    assert_eq!((status, body["code"].as_str()), (400, Some("conflict")), "{body}");

    // Follow-ups are hub events: a restarted daemon rebuilds them.
    fixture.run(&["daemon", "stop"]);
    let endpoint_again = fixture.run(&["daemon", "ensure"]);
    let handle = fixture.request(&endpoint_again, "/projects/open", "app", Some(json!({"path":fixture.root.path()}))).1["handle"].as_str().unwrap().to_owned();
    let rebuilt = fixture.request(&endpoint_again, &format!("/projects/{handle}/state"), "app", None).1;
    let group = rebuilt["projection"]["FollowUp"].as_object().unwrap();
    assert_eq!(group.len(), 5, "{rebuilt}");
    assert!(group[&first]["done"].is_object());
}
