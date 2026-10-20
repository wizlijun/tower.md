use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Barrier};

use serde_json::{json, Value};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    runtime: TempDir,
}
impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            runtime: tempfile::tempdir().unwrap(),
        };
        git(fixture.root.path(), &["init", "-b", "main"]);
        fs::create_dir_all(fixture.root.path().join(".towermd")).unwrap();
        fs::write(
            fixture.root.path().join(".towermd/config.yaml"),
            "project_id: scan-integration-project\ndocs_dir: docs/tower\nhub: local\n",
        )
        .unwrap();
        for name in ["alpha", "beta", "gamma"] {
            fs::create_dir_all(fixture.root.path().join(format!("src/{name}"))).unwrap();
            fs::write(
                fixture.root.path().join(format!("src/{name}/mod.rs")),
                format!("// committed {name}\npub fn run() {{}}\n"),
            )
            .unwrap();
        }
        git(fixture.root.path(), &["add", "."]);
        git(fixture.root.path(), &["commit", "-m", "fixture"]);
        fixture
    }
    fn output(&self, args: &[&str]) -> Output {
        cli(self.root.path(), self.runtime.path(), args)
    }
    fn run(&self, args: &[&str]) -> Value {
        let output = self.output(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn ready(&self, id: &str) -> Value {
        let value = self.run(&["baseline", "scan", "--wait", id]);
        assert_eq!(value["status"], "ready", "{value}");
        value
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(&["daemon", "stop"]);
    }
}
fn cli(root: &Path, runtime: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_towermd"))
        .args(args)
        .current_dir(root)
        .env("TOWERMD_RUNTIME_DIR", runtime)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Scan Test",
            "-c",
            "user.email=scan@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn endpoint(fixture: &Fixture) -> Value {
    fixture.run(&["daemon", "ensure"])
}
fn post(endpoint: &Value, path: &str, data: Value) -> reqwest::blocking::Response {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .header("x-towermd-client", "app")
        .json(&data)
        .send()
        .unwrap()
}

#[test]
fn scan_uses_committed_source_survives_restart_and_rebuilds_deleted_cache() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.path().join("src/alpha/mod.rs"),
        "DIRTY_ONLY_SECRET",
    )
    .unwrap();
    fs::write(
        fixture.root.path().join("untracked.rs"),
        "UNTRACKED_ONLY_SECRET",
    )
    .unwrap();
    let before = git(fixture.root.path(), &["status", "--porcelain=v1"]);
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let initial = fixture.run(&["baseline", "scan"]);
    let id = initial["id"].as_str().unwrap();
    let ready = fixture.ready(id);
    assert_eq!(ready["report"]["revision"], head);
    assert!(!ready.to_string().contains("DIRTY_ONLY_SECRET"));
    assert!(!ready.to_string().contains("untracked.rs"));
    let draft = fixture.root.path().join(".git/towermd/drafts").join(id);
    assert!(draft.join("tower.canvas").is_file());
    assert!(draft.join("arch").is_dir());
    assert_eq!(
        git(fixture.root.path(), &["status", "--porcelain=v1"]),
        before
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    fixture.run(&["daemon", "stop"]);
    assert_eq!(fixture.run(&["baseline", "show", id]), ready);
    fixture.run(&["daemon", "stop"]);
    fs::remove_dir_all(&draft).unwrap();
    assert!(!fixture.output(&["baseline", "show", id]).status.success());
    let rebuilt = fixture.run(&["baseline", "scan"]);
    let rebuilt = fixture.ready(rebuilt["id"].as_str().unwrap());
    assert_eq!(rebuilt["report"]["revision"], head);
    assert_eq!(
        rebuilt["report"]["source_file_count"],
        ready["report"]["source_file_count"]
    );
    assert_eq!(
        git(fixture.root.path(), &["status", "--porcelain=v1"]),
        before
    );
}

#[test]
fn concurrent_scans_share_job_and_project_lock_rejects_another_clone() {
    let fixture = Fixture::new();
    let endpoint = endpoint(&fixture);
    let opened: Value = post(
        &endpoint,
        "/projects/open",
        json!({"path": fixture.root.path()}),
    )
    .error_for_status()
    .unwrap()
    .json()
    .unwrap();
    let path = format!("/projects/{}/scans", opened["handle"].as_str().unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let endpoint = endpoint.clone();
        let path = path.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            post(&endpoint, &path, Value::Null)
                .error_for_status()
                .unwrap()
                .json::<Value>()
                .unwrap()
        }));
    }
    barrier.wait();
    let a = threads.remove(0).join().unwrap();
    let b = threads.remove(0).join().unwrap();
    assert_eq!(a["id"], b["id"], "同一项目的并行扫描必须复用正在执行的扫描");
    fixture.ready(a["id"].as_str().unwrap());
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &[
            "clone",
            "--no-local",
            fixture.root.path().to_str().unwrap(),
            "other",
        ],
    );
    let rejected = cli(
        &clone.path().join("other"),
        fixture.runtime.path(),
        &["project", "view"],
    );
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("另一 clone"));
    let worktree = clone.path().join("worktree");
    git(
        fixture.root.path(),
        &[
            "worktree",
            "add",
            "--detach",
            worktree.to_str().unwrap(),
            "HEAD",
        ],
    );
    let shared: Value = post(&endpoint, "/projects/open", json!({"path": worktree}))
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(opened, shared);
}

#[test]
fn routes_require_known_client_and_instance_and_cli_has_no_approval() {
    let fixture = Fixture::new();
    let endpoint = endpoint(&fixture);
    let url = format!("http://127.0.0.1:{}/projects/open", endpoint["port"]);
    for (instance, client) in [
        ("wrong", "app"),
        (endpoint["instance"].as_str().unwrap(), "agent"),
    ] {
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(&url)
            .header("x-towermd-instance", instance)
            .header("x-towermd-client", client)
            .json(&json!({"path": fixture.root.path()}))
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), 409);
    }
    assert!(!fixture
        .output(&["baseline", "approve", "B0"])
        .status
        .success());
    let injected = post(
        &endpoint,
        "/projects/open",
        json!({"path": fixture.root.path(), "actor": "human:fake"}),
    );
    assert!(!injected.status().is_success());
}

#[cfg(unix)]
#[test]
fn derived_cache_symlink_is_rejected_without_touching_target() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep"), "untouched").unwrap();
    fs::create_dir(fixture.root.path().join(".git/towermd")).unwrap();
    symlink(
        outside.path(),
        fixture.root.path().join(".git/towermd/drafts"),
    )
    .unwrap();
    let started = fixture.run(&["baseline", "scan"]);
    let result = fixture.run(&[
        "baseline",
        "scan",
        "--wait",
        started["id"].as_str().unwrap(),
    ]);
    assert_eq!(result["status"], "failed");
    assert_eq!(
        fs::read_to_string(outside.path().join("keep")).unwrap(),
        "untouched"
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
}
