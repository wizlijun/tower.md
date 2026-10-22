#[path = "support/envelope.rs"]
mod envelope;

use serde_json::{json, Value};
use std::{fs, path::Path, process::{Command, Stdio}, time::{Duration, Instant}};

struct Fixture { root: tempfile::TempDir, runtime: tempfile::TempDir }
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Resolution Test", "-c", "user.email=resolution@example.invalid",
            "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().into()
}
impl Fixture {
    fn new() -> (Self, String) {
        let fixture = Self { root: tempfile::tempdir().unwrap(), runtime: tempfile::tempdir().unwrap() };
        let root = fixture.root.path();
        git(root, &["init", "-qb", "main"]);
        fs::create_dir_all(root.join(".towermd")).unwrap();
        fs::create_dir_all(root.join("docs/tower/arch")).unwrap();
        fs::write(root.join(".towermd/config.yaml"),
            "project_id: resolution-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
        fs::write(root.join("docs/tower/arch/A.md"), "---\ntype: Module\nid: A\n---\nOriginal\n").unwrap();
        fs::write(root.join("source.txt"), "original\n").unwrap();
        git(root, &["add", "."]); git(root, &["commit", "-qm", "import project"]);
        fs::write(root.join("docs/tower/arch/A.md"), "---\ntype: Module\nid: A\n---\nManual edit\n").unwrap();
        git(root, &["add", "."]); git(root, &["commit", "-qm", "manual slow edit"]);
        let target = git(root, &["rev-parse", "HEAD"]);
        (fixture, target)
    }
    fn cli(&self, args: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN").current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null()).args(args).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        envelope::data_checked(args, &output)
    }
    fn request(&self, endpoint: &Value, path: &str, client: &str, body: Option<Value>) -> (reqwest::StatusCode, Value) {
        let client_http = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap();
        let url = format!("http://127.0.0.1:{}{path}", endpoint["port"]);
        let request = match body { Some(body) => client_http.post(url).json(&body), None => client_http.get(url) };
        let response = request.header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", client).send().unwrap();
        let status = response.status(); let result = response.json().unwrap();
        (status, result)
    }
    fn app(&self, endpoint: &Value, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.request(endpoint, path, "app", body);
        assert!(status.is_success(), "{path}: {status} {value}"); value
    }
    fn open(&self) -> (Value, String) {
        let endpoint = self.cli(&["daemon", "ensure"]);
        let project = self.app(&endpoint, "/projects/open", Some(json!({"path":self.root.path()})));
        (endpoint, project["handle"].as_str().unwrap().into())
    }
    fn wait_blocked(&self, endpoint: &Value, handle: &str) {
        self.app(endpoint, &format!("/projects/{handle}/snapshot"), Some(json!({})));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = self.app(endpoint, &format!("/projects/{handle}/state"), None);
            if state["projection"]["MainInspection"]["complete"] == true {
                assert_eq!(state["projection"]["MainInspection"]["passed"], false); return;
            }
            assert!(Instant::now() < deadline, "{state}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn wait_inspection(&self, endpoint: &Value, handle: &str, main: &str, complete: bool) -> Value {
        let deadline=Instant::now()+Duration::from_secs(15);
        loop {
            let state=self.app(endpoint,&format!("/projects/{handle}/state"),None);
            if state["projection"]["MainInspection"]["main"]==main
                && state["projection"]["MainInspection"]["complete"]==complete { return state; }
            assert!(Instant::now()<deadline,"{state}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN").current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path()).args(["daemon", "stop"]).output();
    }
}

#[test]
fn app_claim_updates_same_sha_inspection_and_retries_original_without_creating_baseline() {
    let (fixture, target) = Fixture::new(); let root = fixture.root.path();
    let (endpoint, handle) = fixture.open();
    fixture.wait_blocked(&endpoint, &handle);
    let path = format!("/projects/{handle}/main-resolutions");
    let prepare = json!({"operation":"claim","commit":target});
    assert!(!fixture.request(&endpoint, &format!("{path}/prepare"), "cli", Some(prepare.clone())).0.is_success());
    let preview = fixture.app(&endpoint, &format!("{path}/prepare"), Some(prepare));
    assert!(preview["diff"].as_str().unwrap().contains("+Manual edit"));
    let input = json!({"preview":preview});
    assert!(!fixture.request(&endpoint, &format!("{path}/confirm"), "cli", Some(input.clone())).0.is_success());
    let first = fixture.app(&endpoint, &format!("{path}/confirm"), Some(input.clone()));
    let retry = fixture.app(&endpoint, &format!("{path}/confirm"), Some(input.clone()));
    for key in ["request_id", "event", "seq", "resolution_commit"] { assert_eq!(first[key], retry[key]); }
    assert_eq!(first["resolution_commit"], target);
    assert_eq!(git(root, &["rev-parse", "main"]), target);
    assert_eq!(git(root, &["tag", "--list", "towermd/*"]), "");
    let state = fixture.app(&endpoint, &format!("/projects/{handle}/state"), None);
    assert_eq!(state["projection"]["MainInspection"]["main"], target);
    assert_eq!(state["projection"]["MainInspection"]["passed"], true, "{state}");
    assert!(state["projection"]["Baseline"].as_object().unwrap().is_empty());
    assert_eq!(state["projection"]["MainResolution"][&target]["resolution"]["approval_eid"], preview["request_id"]);
    let mut changed = input; changed["preview"]["diff"] = json!("another input");
    assert!(!fixture.request(&endpoint, &format!("{path}/confirm"), "app", Some(changed)).0.is_success());
}

#[test]
fn app_revert_preserves_index_and_worktree_and_does_not_repeat_inverse() {
    let (fixture, target) = Fixture::new(); let root = fixture.root.path();
    let (endpoint, handle) = fixture.open();
    fixture.wait_blocked(&endpoint, &handle);
    git(root, &["checkout", "--detach", "-q"]);
    fs::write(root.join("source.txt"), "staged user content\n").unwrap();
    git(root, &["add", "source.txt"]);
    fs::write(root.join("source.txt"), "unstaged user content\n").unwrap();
    let index = git(root, &["diff", "--cached", "--binary"]);
    let unstaged = git(root, &["diff", "--binary"]);
    let path = format!("/projects/{handle}/main-resolutions");
    let preview = fixture.app(&endpoint, &format!("{path}/prepare"), Some(json!({"operation":"revert","commit":target})));
    let input = json!({"preview":preview});
    let first = fixture.app(&endpoint, &format!("{path}/confirm"), Some(input.clone()));
    let retry = fixture.app(&endpoint, &format!("{path}/confirm"), Some(input));
    assert_eq!(first["resolution_commit"], retry["resolution_commit"]);
    assert_eq!(first["event"], retry["event"]);
    let inverse = first["resolution_commit"].as_str().unwrap();
    assert_ne!(inverse, target);
    assert_eq!(git(root, &["show", &format!("{inverse}:docs/tower/arch/A.md")]),
        "---\ntype: Module\nid: A\n---\nOriginal");
    assert_eq!(git(root, &["rev-parse", "HEAD"]), target);
    assert_eq!(git(root, &["diff", "--cached", "--binary"]), index);
    assert_eq!(git(root, &["diff", "--binary"]), unstaged);
    let state = fixture.app(&endpoint, &format!("/projects/{handle}/state"), None);
    assert_eq!(state["projection"]["MainInspection"]["passed"], true, "{state}");
    assert!(state["projection"]["Baseline"].as_object().unwrap().is_empty());
}

#[test]
fn incomplete_history_is_visible_and_only_its_alert_clears_after_restoring_history() {
    let (fixture,target)=Fixture::new();let root=fixture.root.path();
    let (endpoint,handle)=fixture.open();
    fixture.wait_blocked(&endpoint,&handle);
    fixture.app(&endpoint,&format!("/projects/{handle}/snapshot"),Some(json!({})));
    fixture.cli(&["daemon","stop"]);
    fs::write(root.join("source.txt"),"ordinary source advancement\n").unwrap();
    git(root,&["add","source.txt"]);git(root,&["commit","-qm","ordinary source advancement"]);
    let tip=git(root,&["rev-parse","HEAD"]);
    fs::write(root.join(".git/shallow"),format!("{tip}\n")).unwrap();
    let (endpoint,handle)=fixture.open();
    let shallow=fixture.wait_inspection(&endpoint,&handle,&tip,false);
    let shallow_id=towermd_core::events::derived_eid(&tip,"alert.raised.inspection_incomplete").unwrap();
    assert_eq!(shallow["projection"]["Alert"][&shallow_id]["resolved"],false);
    assert!(shallow["projection"]["MainInspection"]["error"].as_str().is_some_and(|error|!error.is_empty()));
    assert_eq!(shallow["projection"]["Alert"][&shallow_id]["data"]["detail"]["error"],
        shallow["projection"]["MainInspection"]["error"]);
    fs::remove_file(root.join(".git/shallow")).unwrap();
    let restored=fixture.wait_inspection(&endpoint,&handle,&tip,true);
    assert_eq!(restored["projection"]["Alert"][&shallow_id]["resolved"],true);
    assert!(restored["projection"]["MainInspection"]["error"].is_null());
    let protected_id=towermd_core::events::derived_eid(&target,"alert.raised.unapproved_protected_change").unwrap();
    assert_eq!(restored["projection"]["Alert"][&protected_id]["resolved"],false);
    assert_eq!(restored["projection"]["MainInspection"]["passed"],false);

    // Rewrite only this disposable fixture's ref; main's real previous
    // observation is retained, so the backwards move must remain visible.
    git(root,&["update-ref","refs/heads/main",&target,&tip]);
    let backwards=fixture.wait_inspection(&endpoint,&handle,&target,false);
    let backwards_id=towermd_core::events::derived_eid(&target,"alert.raised.inspection_incomplete").unwrap();
    assert_eq!(backwards["projection"]["Alert"][&backwards_id]["resolved"],false);
    git(root,&["update-ref","refs/heads/main",&tip,&target]);
    let restored=fixture.wait_inspection(&endpoint,&handle,&tip,true);
    assert_eq!(restored["projection"]["Alert"][&backwards_id]["resolved"],true);
    assert_eq!(restored["projection"]["Alert"][&protected_id]["resolved"],false);
    git(root,&["update-ref","refs/heads/main",&target,&tip]);
    let repeated=fixture.wait_inspection(&endpoint,&handle,&target,false);
    assert_eq!(repeated["projection"]["Alert"][&backwards_id]["resolved"],false);
    assert_eq!(repeated["projection"]["Alert"][&backwards_id]["event"],
        backwards["projection"]["Alert"][&backwards_id]["event"],"do not replace the raised original");
    git(root,&["update-ref","refs/heads/main",&tip,&target]);
    let restored=fixture.wait_inspection(&endpoint,&handle,&tip,true);
    assert_eq!(restored["projection"]["Alert"][&backwards_id]["resolved"],true);
    assert_eq!(restored["projection"]["Alert"][&protected_id]["resolved"],false);
}
