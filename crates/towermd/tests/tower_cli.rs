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

#[test]
fn a_change_summary_has_at_most_seven_lines_and_shows_as_sent_in_the_digest() {
    let fixture = Fixture::new();
    let summary = |from: &str, lines: &[&str]| {
        let mut args = vec!["summary", "--from", from, "--target", "T-1", "--ref", "abc1234"];
        for line in lines { args.extend(["--line", line]); }
        fixture.output(&args)
    };
    let first = summary("diff/1", &["新增函数=3", "新增依赖=1", "结尾=改了"]);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stdout));
    let first = envelope::data_checked(&["summary"], &first);
    assert_eq!((first["obj"].as_str(), first["envelope"]["verb"].as_str()), (Some("T-1"), Some("summary")));
    assert_eq!(first["envelope"]["data"]["lines"][0], json!({"label":"新增函数","value":3}));
    // Eight lines, a missing `=`, an over-long value or an uninstalled extension are refused, never cut.
    let eight: Vec<String> = (1..=8).map(|n| format!("第{n}项={n}")).collect();
    assert!(!summary("diff/1", &eight.iter().map(String::as_str).collect::<Vec<_>>()).status.success());
    assert_eq!(code(&summary("diff/1", &["没有等号"])), "usage");
    assert!(!summary("diff/1", &[&format!("结尾={}", "长".repeat(61))]).status.success());
    assert!(!summary("pkg:eng-diff", &["新增函数=1"]).status.success());
    let latest = fixture.run(&["summary", "--from", "diff/1", "--target", "T-1", "--line", "新增函数=4"]);
    assert!(latest["seq"].as_u64() > first["seq"].as_u64());

    let since = fixture.run(&["since"]);
    let changes = since["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "{since}");
    assert_eq!((changes[0]["lines"].clone(), changes[0]["text"].as_str()),
        (json!(["新增函数：4"]), Some("T-1：新增函数：4（diff/1 提供）")));
    assert!(since["details"].as_array().unwrap().contains(&json!("变化 · T-1：新增函数：4（diff/1 提供）")));
    let text = fixture.output(&["--text", "since"]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("变化 · T-1：新增函数：4（diff/1 提供）"));
}

#[test]
fn a_person_asks_from_the_app_answerers_reply_with_refs_and_silence_and_loops_are_raised() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let (_, opened) = fixture.request(&endpoint, reqwest::Method::POST, "/projects/open", "app", Some(json!({"path":fixture.root.path()})));
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let state = || fixture.request(&endpoint, reqwest::Method::GET, &format!("/projects/{handle}/state"), "app", None).1;
    let post = |path: &str, client: &str, body: Value| fixture.request(&endpoint, reqwest::Method::POST, &format!("/projects/{handle}{path}"), client, Some(body));
    let due = |seconds: u64| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        towermd_core::tower_api::timestamp(now + seconds * 1000).unwrap()
    };
    let new_id = || uuid::Uuid::now_v7().to_string();
    let (q1, q2) = (new_id(), new_id());
    let ask = |id: &str, target: &str, ask: &str, seconds: u64| json!({"request_id":id,"target":target,"ask":ask,"text":"解析器为什么慢？","due":due(seconds)});

    // A source declares it answers questions on T-q; the declaration sticks.
    fixture.run(&["report", "--state", "in_progress", "--summary", "在看", "--from", "src/1", "--target", "T-q", "--answers"]);
    fixture.run(&["report", "--state", "in_progress", "--summary", "还在看", "--from", "src/1", "--target", "T-q"]);
    assert_eq!(state()["projection"]["TowerApi"]["answerers"]["T-q"], json!(["src/1"]));

    // Asking is a person's action: the CLI cannot, and the body cannot address it.
    let (status, refused) = post("/requests", "cli", ask(&q1, "T-q", "question", 3600));
    assert_eq!((status, refused["code"].as_str()), (409, Some("conflict")), "{refused}");
    let mut forged = ask(&q1, "T-q", "question", 3600);
    forged["to"] = json!(["someone/1"]);
    assert!(post("/requests", "app", forged).0 >= 400, "the daemon fixes the recipients");
    let question = ask(&q1, "T-q", "question", 3600);
    let (status, sent) = post("/requests", "app", question.clone());
    assert_eq!((status, sent["to"].clone()), (200, json!(["src/1"])), "{sent}");
    assert_eq!(post("/requests", "app", question.clone()).1["seq"], sent["seq"], "a retry is the same request");
    let mut changed = question;
    changed["text"] = json!("别的问题");
    assert_eq!(post("/requests", "app", changed).1["code"], "conflict");

    // Nobody answers on T-none: the question is queued, never dropped.
    let (_, queued) = post("/requests", "app", ask(&q2, "T-none", "question", 3600));
    assert_eq!(queued["to"], json!([]));
    let requests = state()["delegation_view"]["requests"].clone();
    let queued = requests.as_array().unwrap().iter().find(|view| view["id"] == q2.as_str()).unwrap();
    assert_eq!((queued["open"].as_bool(), queued["recipients"].clone()), (Some(true), json!([])), "{requests}");
    let withdraw = format!("/requests/{q2}/withdraw");
    let body = json!({"request_id":new_id()});
    assert_eq!(post(&withdraw, "cli", body.clone()).0, 409);
    assert_eq!(post(&withdraw, "app", body).0, 200);

    // A question is answered from the CLI by a recipient, with refs.
    let no_ref = fixture.output(&["reply", &q1, "--state", "in_progress", "--summary", "正则回溯", "--from", "src/1"]);
    assert!(!no_ref.status.success(), "a question needs refs");
    assert!(!fixture.output(&["reply", &q1, "--state", "in_progress", "--summary", "x", "--from", "other/1", "--ref", "a"]).status.success());
    let answered = fixture.run(&["reply", &q1, "--state", "in_progress", "--summary", "正则回溯", "--from", "src/1", "--ref", "src/parser.rs"]);
    assert_eq!(answered["obj"], q1.as_str());
    let reply = &state()["projection"]["Requests"][&q1]["reply"];
    assert_eq!((reply["data"]["from"].as_str(), reply["data"]["refs"].clone(), reply["event"]["actor"].as_str()),
        (Some("src/1"), json!(["src/parser.rs"]), Some("process:towermd-cli")), "{reply}");

    // Two report requests on T-q go unanswered: passive, then active.
    let reports = [new_id(), new_id()];
    for id in &reports {
        let (status, sent) = post("/requests", "app", ask(id, "T-q", "report", 2));
        assert_eq!((status, sent["to"].clone()), (200, json!(["src/1"])), "{sent}");
    }
    // The same next step three times in a row is a loop.
    for _ in 0..3 {
        fixture.run(&["report", "--state", "in_progress", "--summary", "重试", "--next", "修测试", "--from", "src/1", "--target", "T-loop"]);
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    let tower = loop {
        let tower = state()["projection"]["TowerApi"].clone();
        let raised = |key: &str| tower[key].as_array().map_or(0, Vec::len);
        if raised("silences") == 2 && raised("stuck") == 1 {break tower;}
        assert!(std::time::Instant::now() < deadline, "not raised: {tower}");
        std::thread::sleep(Duration::from_millis(500));
    };
    let signal = |kind: &str| tower["signals"].as_array().unwrap().iter().find(|entry| entry["data"]["kind"] == kind).unwrap().clone();
    let silent = signal("core.silent");
    assert_eq!((silent["target"].as_str(), silent["count"].as_u64(), silent["data"]["level"].as_str(), silent["last"]["actor"].as_str()),
        (Some("T-q"), Some(2), Some("active"), Some("process:towermd")), "{silent}");
    assert_eq!(tower["silences"][0]["refs"], json!([format!("request:{}", reports[0])]));
    let stuck = signal("core.stuck");
    assert_eq!((stuck["target"].as_str(), stuck["data"]["level"].as_str(), stuck["refs"][0].as_str()),
        (Some("T-loop"), Some("active"), Some("stuck:same_next")), "{stuck}");
    // Raised once: later passes find the same IDs recorded.
    std::thread::sleep(Duration::from_secs(11));
    let again = state()["projection"]["TowerApi"].clone();
    assert_eq!((again["silences"].as_array().unwrap().len(), again["stuck"].as_array().unwrap().len()), (2, 1));
}
