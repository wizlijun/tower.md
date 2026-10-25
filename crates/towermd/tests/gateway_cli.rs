//! The interaction gateway's daemon side (spec 16.4, FR-60, FR-63). The
//! gateway signs as its run's agent, so it serves only a run the runner
//! started and is still running; a permission request goes only through it,
//! and only a person answers one, from the App.
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
        fs::write(root.join(".towermd/config.yaml"), "project_id: gateway-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
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
    fn request(&self, endpoint: &Value, path: &str, client: &str, body: Value) -> (u16, Value) {
        let http = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap();
        let response = http.post(format!("http://127.0.0.1:{}{path}", endpoint["port"])).json(&body)
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", client).send().unwrap();
        let status = response.status().as_u16();
        (status, response.json().unwrap_or(Value::Null))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(&["daemon", "stop"]);
    }
}

#[test]
fn the_gateway_serves_only_a_live_runner_run_and_only_a_person_answers_a_permission() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let path = json!({"path":fixture.root.path()});
    let (status, opened) = fixture.request(&endpoint, "/projects/open", "mcp", path.clone());
    assert_eq!(status, 200, "the gateway opens its run's project: {opened}");
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let (status, _) = fixture.request(&endpoint, "/projects/open", "agent", path);
    assert!(status >= 400, "an unknown client is refused");

    // Without a run, or for a run the runner is not running, it signs nothing.
    let cwd = fixture.root.path().to_str().unwrap();
    let question = |run: Option<&str>| {
        let mut body = json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":cwd,"text":"用哪个库？",
            "options":["a","b"]});
        if let Some(run) = run { body["run"] = json!(run); }
        fixture.request(&endpoint, &format!("/projects/{handle}/ask"), "mcp", body)
    };
    let (status, refused) = question(None);
    assert_eq!((status >= 400, refused["code"].as_str()), (true, Some("usage")), "{refused}");
    let (status, refused) = question(Some("R-none"));
    assert_eq!((status >= 400, refused["code"].as_str()), (true, Some("conflict")), "{refused}");
    let (status, refused) = fixture.request(&endpoint, &format!("/projects/{handle}/tower"), "mcp",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":cwd,"run":"R-none","verb":"report","data":{"state":"in_progress","summary":"进行中"}}));
    assert!(status >= 400, "{refused}");

    // A permission request goes only through the gateway, for a live run.
    let permissions = format!("/projects/{handle}/permissions");
    let ask = json!({"request_id":uuid::Uuid::now_v7().to_string(),"run":"R-none","tool":"Bash","input":{"command":"ls"}});
    let (status, refused) = fixture.request(&endpoint, &permissions, "cli", ask.clone());
    assert_eq!((status >= 400, refused["code"].as_str()), (true, Some("usage")), "{refused}");
    let (status, _) = fixture.request(&endpoint, &permissions, "app", ask.clone());
    assert!(status >= 400, "the App does not ask");
    let (status, refused) = fixture.request(&endpoint, &permissions, "mcp", ask);
    assert_eq!((status >= 400, refused["code"].as_str()), (true, Some("conflict")), "{refused}");
    let (status, _) = fixture.request(&endpoint, &format!("{permissions}/wait"), "mcp", json!({"run":"R-none","permission":"K-none"}));
    assert!(status >= 400);

    // Answering is the App's alone, and the body cannot sign or invent a choice.
    let resolve = format!("{permissions}/K-none/resolve");
    for client in ["cli", "mcp"] {
        let (status, _) = fixture.request(&endpoint, &resolve, client, json!({"request_id":uuid::Uuid::now_v7().to_string(),"decision":"allow"}));
        assert!(status >= 400, "{client}");
    }
    let (status, _) = fixture.request(&endpoint, &resolve, "app", json!({"request_id":uuid::Uuid::now_v7().to_string(),"decision":"always"}));
    assert!(status >= 400);
    let (status, refused) = fixture.request(&endpoint, &resolve, "app", json!({"request_id":uuid::Uuid::now_v7().to_string(),"decision":"allow"}));
    assert!(status >= 400, "no such request: {refused}");
}

#[test]
fn towermd_mcp_speaks_stdio_json_rpc_and_reaches_the_daemon_as_the_gateway() {
    let fixture = Fixture::new();
    let lines = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"towermd_note","arguments":{"summary":"进行中"}}}),
    ];
    let speak = || {
        use std::io::Write;
        let mut child = Command::new(env!("CARGO_BIN_EXE_towermd"))
            .args(["mcp", "--run", "R-none", "--worktree", fixture.root.path().to_str().unwrap(), "--harness", "claude-code"])
            .env("TOWERMD_RUNTIME_DIR", fixture.runtime.path()).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        for line in &lines {
            writeln!(stdin, "{line}").unwrap();
        }
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).collect::<Vec<_>>()
    };

    // Without a daemon it still lists its tools; a call says why it failed
    // and never starts a daemon.
    let replies = speak();
    assert_eq!(replies.len(), 3, "a notification gets no reply: {replies:?}");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    let names: Vec<&str> = replies[1]["result"]["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["towermd_ask", "towermd_request_review", "towermd_permission", "towermd_note"]);
    assert_eq!(replies[2]["result"]["isError"], true, "{}", replies[2]);
    assert!(!fixture.runtime.path().join("endpoint.json").exists(), "the gateway never starts a daemon");

    // With a daemon, a run the runner is not running gets nothing signed.
    fixture.run(&["daemon", "ensure"]);
    let replies = speak();
    let refused = replies[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(replies[2]["result"]["isError"], true);
    assert!(refused.contains("运行器"), "{refused}");
}
