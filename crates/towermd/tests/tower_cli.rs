//! Tower API from the command line (FR-114–119): a plain shell script reports
//! progress and raises signals without a run, the daemon wraps them in the
//! envelope signed `process:towermd-cli`, signals merge with a count, and the
//! App can read them in `/state` but never send them.
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
    fn request(&self, endpoint: &Value, method: reqwest::Method, path: &str, client: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap()
            .request(method, format!("http://127.0.0.1:{}{path}", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", client);
        if let Some(body) = body {
            request = request.json(&body);
        }
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

#[test]
fn a_shell_script_reports_and_signals_and_signals_merge_with_a_count() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let (_, opened) = fixture.request(&endpoint, reqwest::Method::POST, "/projects/open", "app", Some(json!({"path":fixture.root.path()})));
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let state = || fixture.request(&endpoint, reqwest::Method::GET, &format!("/projects/{handle}/state"), "app", None).1;

    // Outside a run the sender must be named; nothing is guessed.
    let anonymous = fixture.output(&["signal", "eng.crap", "--level", "active", "--title", "CRAP 偏高"]);
    assert_eq!(code(&anonymous), "usage", "{}", String::from_utf8_lossy(&anonymous.stdout));

    let id = uuid::Uuid::now_v7().to_string();
    let args = ["--request-id", id.as_str(), "signal", "eng.crap", "--level", "active", "--title", "CRAP 偏高",
        "--value", "41", "--threshold", "30", "--from", "ci/nightly", "--ref", "src/parser.rs"];
    let first = fixture.run(&args);
    let envelope = &first["envelope"];
    assert_eq!((envelope["project"].as_str(), envelope["target"].as_str(), envelope["from"].clone(), envelope["verb"].as_str()),
        (Some("tower-project"), Some("tower-project"), json!({"id":"ci/nightly","kind":"source"}), Some("signal")), "{first}");
    assert_eq!((envelope["data"]["value"].clone(), envelope["data"]["threshold"].clone()), (json!(41), json!(30)));
    // A retry with the same request ID is the same event; a changed body is a conflict.
    assert_eq!(fixture.run(&args)["seq"], first["seq"]);
    let mut changed = args;
    changed[9] = "42";
    assert_eq!(code(&fixture.output(&changed)), "conflict");

    let second = fixture.run(&["signal", "eng.crap", "--level", "active", "--title", "CRAP 更高", "--value", "44", "--from", "ci/nightly"]);
    assert!(second["seq"].as_u64() > first["seq"].as_u64());
    fixture.run(&["signal", "eng.lint", "--level", "passive", "--title", "两处警告", "--from", "ci/nightly"]);
    let report = fixture.run(&["report", "--state", "blocked", "--summary", "等 API 密钥", "--progress", "3/7", "--unit", "files",
        "--from", "agent/1", "--from-kind", "delegate", "--target", "T-9"]);
    assert_eq!((report["obj"].as_str(), report["envelope"]["data"]["progress"].clone()),
        (Some("T-9"), json!({"done":3,"total":7,"unit":"files"})));

    let tower = state()["projection"]["TowerApi"].clone();
    let signals = tower["signals"].as_array().unwrap();
    assert_eq!(signals.len(), 2, "{tower}");
    let crap = signals.iter().find(|signal| signal["data"]["kind"] == "eng.crap").unwrap();
    assert_eq!((crap["count"].as_u64(), crap["data"]["value"].clone(), crap["data"]["title"].as_str()), (Some(2), json!(44), Some("CRAP 更高")));
    assert_eq!(crap["last"]["actor"], "process:towermd-cli");
    assert_eq!(crap["first"]["seq"], first["seq"]);
    assert_eq!((tower["reports"][0]["data"]["state"].as_str(), tower["reports"][0]["target"].as_str()), (Some("blocked"), Some("T-9")));

    // Over-long text is refused, never cut; there is no `done`, and no person.
    let long = fixture.output(&["signal", "eng.crap", "--level", "active", "--title", &"长".repeat(31), "--from", "ci/nightly"]);
    assert_eq!(code(&long), "too_long");
    let long = fixture.output(&["report", "--state", "in_progress", "--summary", &"长".repeat(101), "--from", "ci/nightly"]);
    assert_eq!(code(&long), "too_long");
    assert!(!fixture.output(&["report", "--state", "done", "--summary", "x", "--from", "ci/nightly"]).status.success());
    assert!(!fixture.output(&["report", "--state", "waiting", "--summary", "x", "--from", "human:local"]).status.success());
    assert!(!fixture.output(&["signal", "core.silent", "--level", "active", "--title", "x", "--from", "ci/nightly"]).status.success());
    assert!(!fixture.output(&["report", "--state", "waiting", "--summary", "x", "--progress", "8/7", "--from", "ci/nightly"]).status.success());
    assert_eq!(state()["projection"]["TowerApi"], tower, "refused messages leave no trace");

    // The App can read signals but not send them; nobody signs the envelope.
    let body = json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":fixture.root.path(),"verb":"signal",
        "from":{"id":"app/1","kind":"source"},"data":{"kind":"eng.crap","level":"active","title":"x"}});
    let (status, refused) = fixture.request(&endpoint, reqwest::Method::POST, &format!("/projects/{handle}/tower"), "app", Some(body.clone()));
    assert_eq!((status, refused["code"].as_str()), (409, Some("conflict")), "{refused}");
    let mut forged = body;
    forged["actor"] = json!("human:local");
    let (status, _) = fixture.request(&endpoint, reqwest::Method::POST, &format!("/projects/{handle}/tower"), "cli", Some(forged));
    assert!(status >= 400);

    // Only a person in the App quiets a signal, and only the message they saw.
    let dismiss = format!("/projects/{handle}/signals/dismiss");
    let ack = |seen: &Value| json!({"request_id":uuid::Uuid::now_v7().to_string(),"from":"ci/nightly","kind":"eng.crap",
        "target":"tower-project","mode":"ack","seen":seen});
    let (status, _) = fixture.request(&endpoint, reqwest::Method::POST, &dismiss, "cli", Some(ack(&crap["last"]["eid"])));
    assert_eq!(status, 409, "the CLI has no dismiss verb");
    let (status, stale) = fixture.request(&endpoint, reqwest::Method::POST, &dismiss, "app", Some(ack(&crap["first"]["eid"])));
    assert_eq!(status, 400, "{stale}");
    let mut forged = ack(&crap["last"]["eid"]);
    forged["actor"] = json!("human:other");
    assert_eq!(fixture.request(&endpoint, reqwest::Method::POST, &dismiss, "app", Some(forged)).0, 400);
    let (status, accepted) = fixture.request(&endpoint, reqwest::Method::POST, &dismiss, "app", Some(ack(&crap["last"]["eid"])));
    assert_eq!((status, accepted["obj"].as_str()), (200, Some("tower-project")), "{accepted}");
    let signals = state()["projection"]["TowerApi"]["signals"].clone();
    let crap = signals.as_array().unwrap().iter().find(|signal| signal["data"]["kind"] == "eng.crap").unwrap();
    assert_eq!((crap["acknowledged"]["actor"].as_str(), crap["acknowledged"]["eid"].clone()), (Some("human:local"), accepted["event"].clone()));
}
