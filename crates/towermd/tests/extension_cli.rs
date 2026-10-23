//! Extensions and the other two Tower API entries (FR-117, FR-120, FR-123).
//!
//! An extension is installed when its manifest is on main; the CLI only
//! reads it. What a `pkg:<id>` sender may send is bounded by that manifest
//! at every entry. `POST /tower` and the runtime-directory inbox feed the
//! same write path as `towermd signal`, signed as their own process.
#[path = "support/envelope.rs"]
mod envelope;

use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime},
};

const MANIFEST: &str = "id: eng-clean\nversion: 1.2.0\ntype: check\nemits:\n  - kind: eng.crap\n    label: CRAP\n    icon: gauge\n    unit: 分\n    deterministic: true\n  - kind: eng.hint\n    max_level: passive\nconfig:\n  crap_max: { type: number, default: 12 }\ncommands:\n  crap: \"cargo run -p probe -- functions\"\n";

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Tower Test", "-c", "user.email=tower@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
}
impl Fixture {
    /// A project with `eng-clean` installed and a `broken` manifest on main.
    fn new() -> Self {
        let fixture = Self { root: tempfile::tempdir().unwrap(), runtime: tempfile::tempdir().unwrap() };
        let root = fixture.root.path();
        git(root, &["init", "-qb", "main"]);
        fs::create_dir_all(root.join(".towermd/extensions/eng-clean")).unwrap();
        fs::create_dir_all(root.join(".towermd/extensions/broken")).unwrap();
        fs::write(root.join(".towermd/config.yaml"), "project_id: tower-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
        fs::write(root.join(".towermd/extensions/eng-clean/towermd-plugin.yaml"), MANIFEST).unwrap();
        fs::write(root.join(".towermd/extensions/broken/towermd-plugin.yaml"), "id: other\nversion: 1\ntype: check\nemits: [ { kind: lint.x } ]\n").unwrap();
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
    fn code(&self, args: &[&str]) -> String {
        envelope::code(&self.output(args))
    }
    fn request(&self, endpoint: &Value, method: reqwest::Method, path: &str, headers: &[(&str, &str)], body: Option<Value>) -> (u16, Value) {
        let mut request = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap()
            .request(method, format!("http://127.0.0.1:{}{path}", endpoint["port"]));
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().unwrap();
        let status = response.status().as_u16();
        (status, response.json().unwrap_or(Value::Null))
    }
    /// Starts the daemon and opens the project as the App does.
    fn open(&self) -> (Value, String) {
        let endpoint = self.run(&["daemon", "ensure"]);
        let instance = endpoint["instance"].as_str().unwrap().to_owned();
        let (_, opened) = self.request(&endpoint, reqwest::Method::POST, "/projects/open",
            &[("x-towermd-instance", &instance), ("x-towermd-client", "app")], Some(json!({"path":self.root.path()})));
        (endpoint, opened["handle"].as_str().unwrap().to_owned())
    }
    fn tower(&self, endpoint: &Value, handle: &str) -> Value {
        let instance = endpoint["instance"].as_str().unwrap();
        self.request(endpoint, reqwest::Method::GET, &format!("/projects/{handle}/state"),
            &[("x-towermd-instance", instance), ("x-towermd-client", "app")], None).1["projection"]["TowerApi"].clone()
    }
    fn inbox(&self) -> PathBuf {
        self.runtime.path().join("inbox/tower-project")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(&["daemon", "stop"]);
    }
}

fn message(from: &str, verb: &str, data: Value) -> Value {
    json!({"v":1,"project":"tower-project","from":{"id":from,"kind":"source"},"target":"tower-project","verb":verb,
        "data":data,"at":"2026-10-23T10:00:00+09:00"})
}

fn signal(from: &str, kind: &str, level: &str, deterministic: bool) -> Value {
    message(from, "signal", json!({"kind":kind,"level":level,"title":"CRAP 偏高","deterministic":deterministic}))
}

#[test]
fn extensions_are_read_from_main_and_bound_what_a_pkg_sender_may_send() {
    let fixture = Fixture::new();
    // Only main counts: a manifest in the working tree is not installed.
    fs::create_dir_all(fixture.root.path().join(".towermd/extensions/local")).unwrap();
    fs::write(fixture.root.path().join(".towermd/extensions/local/towermd-plugin.yaml"), "id: local\nversion: 1\ntype: check\n").unwrap();

    let listed = fixture.run(&["ext", "ls"]);
    let head = git(fixture.root.path(), &["rev-parse", "main"]);
    assert_eq!(listed["revision"].as_str(), Some(head.trim()));
    let ids: Vec<_> = listed["extensions"].as_array().unwrap().iter().map(|entry| entry["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["broken", "eng-clean"]);
    assert_eq!(listed["extensions"][1]["kinds"], json!(["eng.crap", "eng.hint"]));
    assert!(listed["extensions"][0]["problems"][0].as_str().unwrap().contains("安装目录"), "{listed}");

    let shown = fixture.run(&["ext", "show", "eng-clean"]);
    assert_eq!((shown["version"].as_str(), shown["type"].as_str()), (Some("1.2.0"), Some("check")));
    assert_eq!(shown["emits"][0], json!({"kind":"eng.crap","label":"CRAP","icon":"gauge","unit":"分","deterministic":true,"max_level":"active"}));
    assert_eq!(shown["emits"][1]["max_level"], "passive");
    assert_eq!(shown["manifest"]["config"]["crap_max"]["default"], 12);
    assert_eq!(shown["commands"]["crap"], "cargo run -p probe -- functions");
    assert_eq!(fixture.code(&["ext", "show", "local"]), "not_found");
    // Read-only: there is no way to install or enable from the command line.
    assert_eq!(fixture.code(&["ext", "add", "x"]), "usage");

    let accepted = fixture.run(&["signal", "eng.crap", "--level", "active", "--title", "CRAP 偏高", "--deterministic",
        "--from", "pkg:eng-clean@1.2/crap"]);
    assert_eq!(accepted["envelope"]["from"]["id"], "pkg:eng-clean@1.2/crap");
    fixture.run(&["report", "--state", "in_progress", "--summary", "扫描中", "--from", "pkg:eng-clean"]);
    for (args, reason) in [
        (vec!["signal", "eng.crap", "--level", "active", "--title", "x", "--from", "ci/nightly"], "属于扩展 eng-clean"),
        (vec!["signal", "eng.crap", "--level", "time_sensitive", "--title", "x", "--from", "pkg:eng-clean"], "最高只能是 active"),
        (vec!["signal", "eng.hint", "--level", "active", "--title", "x", "--from", "pkg:eng-clean"], "最高只能是 passive"),
        (vec!["signal", "eng.hint", "--level", "passive", "--title", "x", "--deterministic", "--from", "pkg:eng-clean"], "确定性"),
        (vec!["signal", "eng.other", "--level", "passive", "--title", "x", "--from", "pkg:eng-clean"], "未声明信号种类"),
        (vec!["signal", "lint.x", "--level", "passive", "--title", "x", "--from", "pkg:broken"], "清单有问题"),
        (vec!["signal", "eng.crap", "--level", "passive", "--title", "x", "--from", "pkg:local"], "未安装"),
        (vec!["report", "--state", "in_progress", "--summary", "x", "--from", "pkg:local"], "未安装"),
    ] {
        let refused = envelope::parse(&fixture.output(&args));
        assert_eq!(refused["error"]["code"], "rejected", "{args:?}: {refused}");
        assert!(refused["error"]["message"].as_str().unwrap().contains(reason), "{args:?}: {refused}");
    }
    // Reading and refusing never touched the repository.
    assert_eq!(git(fixture.root.path(), &["status", "--porcelain"]), "?? .towermd/extensions/local/\n");
}

#[test]
fn the_http_entry_records_bare_envelopes_through_the_same_checks() {
    let fixture = Fixture::new();
    let (endpoint, handle) = fixture.open();
    assert_eq!(endpoint["api_version"], 13);
    let instance = endpoint["instance"].as_str().unwrap().to_owned();
    let script = [("x-towermd-instance", instance.as_str())];
    let post = |headers: &[(&str, &str)], body: Value| fixture.request(&endpoint, reqwest::Method::POST, "/tower", headers, Some(body));

    let body = signal("pkg:eng-clean", "eng.crap", "active", true);
    let (status, first) = post(&script, body.clone());
    assert_eq!(status, 200, "{first}");
    assert_eq!((first["status"].as_str(), first["envelope"].clone()), (Some("accepted"), body.clone()));
    // Resending the same envelope is the same message.
    let (_, again) = post(&script, body.clone());
    assert_eq!((again["event"].clone(), again["seq"].clone()), (first["event"].clone(), first["seq"].clone()));
    let (status, report) = post(&[script[0], ("x-towermd-client", "cli")],
        message("ci/nightly", "report", json!({"state":"blocked","summary":"等密钥","progress":{"done":3,"total":7}})));
    assert_eq!(status, 200, "{report}");

    let tower = fixture.tower(&endpoint, &handle);
    let crap = &tower["signals"][0];
    assert_eq!((crap["count"].as_u64(), crap["last"]["actor"].as_str()), (Some(1), Some("process:towermd-http")), "{tower}");
    assert_eq!(tower["reports"][0]["last"]["actor"], "process:towermd-http");

    let mut wrong_project = body.clone();
    wrong_project["project"] = json!("elsewhere");
    let mut human = message("human:local", "report", json!({"state":"waiting","summary":"x"}));
    human["from"]["kind"] = json!("delegate");
    let mut forged = body.clone();
    forged["actor"] = json!("human:local");
    let mut long = body.clone();
    long["data"]["title"] = json!("长".repeat(31));
    for (headers, body, expected) in [
        (vec![], body.clone(), 409),
        (vec![("x-towermd-instance", "stale")], body.clone(), 409),
        (vec![script[0], ("x-towermd-client", "app")], body.clone(), 409),
        (script.to_vec(), wrong_project, 404),
        (script.to_vec(), human, 400),
        (script.to_vec(), long, 400),
        (script.to_vec(), signal("ci/nightly", "core.silent", "active", false), 400),
        (script.to_vec(), signal("ci/nightly", "eng.crap", "active", true), 400),
        (script.to_vec(), signal("pkg:eng-clean", "eng.crap", "time_sensitive", true), 400),
        (script.to_vec(), message("ci/nightly", "request_report", json!({})), 400),
    ] {
        let (status, refused) = post(&headers, body);
        assert_eq!(status, expected, "{headers:?}: {refused}");
    }
    let (status, _) = post(&script, forged);
    assert!(status >= 400, "unknown envelope keys such as a signer are refused");
    assert_eq!(fixture.tower(&endpoint, &handle), tower, "refused messages leave no trace");
}

#[test]
fn the_file_entry_drains_the_runtime_inbox_and_never_writes_the_repository() {
    let fixture = Fixture::new();
    let (endpoint, handle) = fixture.open();
    let inbox = fixture.inbox();
    fs::create_dir_all(&inbox).unwrap();
    let lines = [
        signal("pkg:eng-clean", "eng.crap", "active", true).to_string(),
        "{not json".to_owned(),
        String::new(),
        signal("pkg:eng-clean", "eng.other", "passive", false).to_string(),
        message("cron/disk", "report", json!({"state":"in_progress","summary":"磁盘 85%"})).to_string(),
    ];
    let drop_file = |name: &str| {
        let path = inbox.join(name);
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        // Already quiet: skip the wait for a writer that might still append.
        fs::File::options().write(true).open(&path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(10)).unwrap();
        path
    };
    let drained = |path: &Path| {
        let deadline = Instant::now() + Duration::from_secs(60);
        while path.exists() || inbox.join(".processing").read_dir().is_ok_and(|mut entries| entries.next().is_some()) {
            assert!(Instant::now() < deadline, "the inbox was not drained");
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    // Not *.jsonl: never read.
    fs::write(inbox.join("notes.txt"), "x").unwrap();
    let first = drop_file("a.jsonl");
    drained(&first);

    let tower = fixture.tower(&endpoint, &handle);
    let crap = &tower["signals"][0];
    assert_eq!((crap["count"].as_u64(), crap["last"]["actor"].as_str()), (Some(1), Some("process:towermd-file")), "{tower}");
    assert_eq!((tower["signals"].as_array().unwrap().len(), tower["reports"][0]["data"]["summary"].as_str()), (1, Some("磁盘 85%")));

    let rejected: Vec<Value> = fs::read_dir(inbox.join("rejected")).unwrap()
        .flat_map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect::<Vec<_>>())
        .collect();
    assert_eq!(rejected.iter().map(|entry| entry["line"].as_u64().unwrap()).collect::<Vec<_>>(), [2, 4], "{rejected:?}");
    assert!(rejected[1]["error"].as_str().unwrap().contains("未声明信号种类"), "{rejected:?}");
    assert_eq!(rejected[0]["text"], "{not json");

    // The same file again (a crash before deletion) records nothing twice.
    let second = drop_file("b.jsonl");
    drained(&second);
    assert_eq!(fixture.tower(&endpoint, &handle), tower);
    assert!(inbox.join("notes.txt").exists());
    // Files are never state and never land in a working tree.
    assert_eq!(git(fixture.root.path(), &["status", "--porcelain", "--ignored"]), "");
}
