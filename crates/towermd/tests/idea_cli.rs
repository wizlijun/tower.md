//! Ideas and commitments are kept apart (FR-181–183): anyone jots an idea,
//! the CLI signs it `process:towermd-cli` and the App `human:local`; only a
//! person in the App promotes one, and the CLI has no such verb.
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

fn code(output: &Output) -> Value {
    envelope::parse(output)["error"]["code"].clone()
}

fn request_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

#[test]
fn anyone_jots_an_idea_but_only_a_person_in_the_app_promotes_it() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let (_, opened) = fixture.request(&endpoint, "/projects/open", "app", Some(json!({"path":fixture.root.path()})));
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let state = || fixture.request(&endpoint, &format!("/projects/{handle}/state"), "app", None).1;

    // The CLI signs its idea as a process; a retry is the same event.
    let id = request_id();
    let args = ["--request-id", id.as_str(), "idea", "add", "给周报加 CSV 导出"];
    let jotted = fixture.run(&args);
    assert_eq!((jotted["idea"]["id"].as_str(), jotted["idea"]["text"].as_str(), jotted["idea"]["added"]["actor"].as_str()),
        (Some(id.as_str()), Some("给周报加 CSV 导出"), Some("process:towermd-cli")), "{jotted}");
    assert_eq!(fixture.run(&args)["seq"], jotted["seq"]);
    let changed = ["--request-id", id.as_str(), "idea", "add", "另一句"];
    assert_eq!(code(&fixture.output(&changed)), "conflict");
    assert_eq!(code(&fixture.output(&["idea", "add", &"长".repeat(201)])), "too_long");
    assert_eq!(code(&fixture.output(&["idea", "add", " "])), "usage");
    assert!(!fixture.output(&["idea", "promote", id.as_str()]).status.success(), "the CLI has no promote");

    // The App jots one too, signed by the person; the body cannot sign itself.
    let ideas = format!("/projects/{handle}/ideas");
    let mine = request_id();
    let (status, from_app) = fixture.request(&endpoint, &ideas, "app", Some(json!({"request_id":mine,"text":"做一个周报工具"})));
    assert_eq!((status, from_app["idea"]["added"]["actor"].as_str()), (200, Some("human:local")), "{from_app}");
    let (status, _) = fixture.request(&endpoint, &ideas, "app", Some(json!({"request_id":request_id(),"text":"x","actor":"human:other"})));
    assert!(status >= 400);
    let projection = state()["projection"].clone();
    assert_eq!(projection["Idea"].as_object().unwrap().len(), 2, "{projection}");
    assert!(projection["Task"].get(&id).is_none(), "an idea is not a task and cannot be claimed");

    // Promoting is the App's alone, and only as a project or a task.
    let promote = |idea: &str| format!("/projects/{handle}/ideas/{idea}/promote");
    let as_project = json!({"request_id":request_id(),"as":"project"});
    let (status, refused) = fixture.request(&endpoint, &promote(&mine), "cli", Some(as_project.clone()));
    assert_eq!((status, refused["code"].as_str()), (409, Some("conflict")), "{refused}");
    let (status, _) = fixture.request(&endpoint, &promote(&mine), "app", Some(json!({"request_id":request_id(),"as":"rule"})));
    assert_eq!(status, 400);
    let (status, _) = fixture.request(&endpoint, &promote(&mine), "app", Some(json!({"request_id":request_id(),"as":"project","actor":"human:other"})));
    assert!(status >= 400);
    let draft = json!({"id":"T-one","title":"周报","module":"A-one","allowed_paths":["src"],"acceptance":["能导出"]});
    let (status, no_baseline) = fixture.request(&endpoint, &promote(&id), "app", Some(json!({"request_id":request_id(),"as":"task","task":draft})));
    assert_eq!(status, 400, "a task needs a published baseline: {no_baseline}");
    assert!(state()["projection"]["Idea"][&id].get("promoted").is_none());

    let (status, goal) = fixture.request(&endpoint, &promote(&mine), "app", Some(as_project.clone()));
    assert_eq!(status, 200, "{goal}");
    assert_eq!((goal["idea"]["promoted"]["as"].as_str(), goal["idea"]["promoted"]["event"]["actor"].as_str()),
        (Some("project"), Some("human:local")));
    // A retry of the same promotion returns it; the project keeps one goal.
    assert_eq!(fixture.request(&endpoint, &promote(&mine), "app", Some(as_project)).1["seq"], goal["seq"]);
    let (status, second) = fixture.request(&endpoint, &promote(&id), "app", Some(json!({"request_id":request_id(),"as":"project"})));
    assert_eq!(status, 400, "{second}");
    assert_eq!(state()["projection"]["Idea"][&mine]["promoted"]["as"], "project");

    // FR-135: an unpromoted lead is judged for quietness like any object; a
    // promoted one is the commitment's now and drops out.
    let judged: Vec<String> = state()["quiet"]["objects"].as_array().unwrap().iter()
        .map(|object| object["id"].as_str().unwrap().to_owned()).collect();
    assert!(judged.contains(&id), "{judged:?}");
    assert!(!judged.contains(&mine), "{judged:?}");
}
