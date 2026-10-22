#[path = "support/envelope.rs"]
mod envelope;

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
        envelope::data_checked(args, &output)
    }
    fn ready(&self, id: &str) -> Value {
        // Exit 5 means the scan is still running after one wait window.
        let value = loop {
            let args = ["dev", "scan", "wait", id];
            let output = self.output(&args);
            if output.status.code() != Some(5) {
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                break envelope::data_checked(&args, &output);
            }
        };
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
    Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
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

fn synthesis_input(fixture: &Fixture, scan: &str) -> std::path::PathBuf {
    let mut input = fixture.run(&["dev", "draft", "template", scan]);
    input["risks"] = json!(["测试夹具：模块边界需要人工确认"]);
    input["questions"] = json!(["是否保留当前三个模块？"]);
    input["intents"] = json!([{
        "id": "I-goal", "path": "intents/I-goal.md",
        "text": "---\ntype: Intent\nid: I-goal\nlayer: L0\nstatus: draft\ntitle: 可测试的三个模块\ntext: 验证草稿主干\nallocated_to: [A-scan-1]\nx-preserved: keep\n---\n意图原文。\n"
    }]);
    let path = fixture.runtime.path().join("synthesis.json");
    fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
    path
}

#[test]
fn synthesis_is_durable_idempotent_and_never_changes_user_main_or_index() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.path().join("Makefile"),
        "test:\n\t@echo actual-proof\n",
    )
    .unwrap();
    git(fixture.root.path(), &["add", "Makefile"]);
    git(fixture.root.path(), &["commit", "-m", "test command"]);
    // Existing staged and unstaged user changes must remain untouched.
    fs::write(
        fixture.root.path().join("src/alpha/mod.rs"),
        "// user dirty\n",
    )
    .unwrap();
    fs::write(
        fixture.root.path().join("src/beta/mod.rs"),
        "// user staged\n",
    )
    .unwrap();
    git(fixture.root.path(), &["add", "src/beta/mod.rs"]);
    let before = git(fixture.root.path(), &["status", "--porcelain"]);
    let index = fs::read(fixture.root.path().join(".git/index")).unwrap();
    let main = git(fixture.root.path(), &["rev-parse", "main"]);
    let scan = fixture.run(&["dev", "scan", "start"]);
    let scan_id = scan["id"].as_str().unwrap();
    fixture.ready(scan_id);
    let path = synthesis_input(&fixture, scan_id);
    let args = [
        "dev",
        "draft",
        "submit",
        path.to_str().unwrap(),
        "--producer",
        "codex/test",
    ];
    let first = fixture.run(&args);
    let id = first["id"].as_str().unwrap();
    assert!(id.starts_with("d-"), "{first}");
    assert_eq!(first["self_reported"], true);
    assert_eq!(first["stale"], false);
    assert!(first["evidence_error"].is_null());
    assert_eq!(first["view"]["nodes"].as_array().unwrap().len(), 5);
    assert_eq!(first["view"]["nodes"].as_array().unwrap().iter().filter(|node| node["region"] == "project").count(), 1);
    assert_eq!(fixture.run(&args), first);
    assert_eq!(fixture.run(&["dev", "draft", "ls"])[0]["id"], id);
    let reference = format!("refs/towermd/drafts/{id}");
    let private = git(fixture.root.path(), &["rev-parse", &reference]);
    assert_ne!(private, main);
    assert!(git(fixture.root.path(), &["tag", "--list", "towermd/B*"]).is_empty());
    fs::remove_dir_all(fixture.root.path().join(".git/towermd/drafts")).unwrap();
    fixture.run(&["daemon", "stop"]);
    assert_eq!(fixture.run(&["dev", "draft", "show", id]), first);
    assert_eq!(fixture.run(&args), first);
    assert_eq!(git(fixture.root.path(), &["rev-parse", "main"]), main);
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(git(fixture.root.path(), &["status", "--porcelain"]), before);
    // The original is durable; missing local logs invalidate proof, not text.
    fs::remove_dir_all(fixture.root.path().join(".git/towermd/verify")).unwrap();
    let missing = fixture.run(&["dev", "draft", "show", id]);
    assert!(missing["evidence_error"].is_string(), "{missing}");
    assert_eq!(missing["submission"], first["submission"]);
}

#[test]
fn synthesis_rejects_stale_sources_and_human_approval_but_keeps_old_drafts_readable() {
    let fixture = Fixture::new();
    let scan = fixture.run(&["dev", "scan", "start"]);
    let scan_id = scan["id"].as_str().unwrap();
    fixture.ready(scan_id);
    let path = synthesis_input(&fixture, scan_id);
    let args = [
        "dev",
        "draft",
        "submit",
        path.to_str().unwrap(),
        "--producer",
        "codex/test",
    ];
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut invalid = original.clone();
    let text = invalid["modules"][0]["text"].as_str().unwrap().replace(
        "status: draft",
        "status: draft\nverified: {by: 'human:owner', at: '2026-10-21T00:00:00Z'}",
    );
    invalid["modules"][0]["text"] = json!(text);
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(!fixture.output(&args).status.success());
    assert!(fixture
        .run(&["dev", "draft", "ls"])
        .as_array()
        .unwrap()
        .is_empty());
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let first = fixture.run(&args);
    fs::write(
        fixture.root.path().join("src/alpha/mod.rs"),
        "// changed main\n",
    )
    .unwrap();
    git(fixture.root.path(), &["add", "."]);
    git(fixture.root.path(), &["commit", "-m", "advance source"]);
    let old = fixture.run(&["dev", "draft", "show", first["id"].as_str().unwrap()]);
    assert_eq!(old["stale"], true);
    assert_eq!(fixture.run(&args)["id"], first["id"]); // Lost-reply retry.
    invalid = original;
    invalid["risks"] = json!(["new synthesis from stale input"]);
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(!fixture.output(&args).status.success());
}

#[test]
fn synthesis_pushes_only_private_draft_ref_to_origin() {
    let fixture = Fixture::new();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-b", "main"]);
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(fixture.root.path(), &["push", "-u", "origin", "main"]);
    let before = git(remote.path(), &["rev-parse", "main"]);
    let scan = fixture.run(&["dev", "scan", "start"]);
    let scan_id = scan["id"].as_str().unwrap();
    fixture.ready(scan_id);
    let path = synthesis_input(&fixture, scan_id);
    let draft = fixture.run(&[
        "dev",
        "draft",
        "submit",
        path.to_str().unwrap(),
        "--producer",
        "codex/test",
    ]);
    let reference = format!("refs/towermd/drafts/{}", draft["id"].as_str().unwrap());
    assert_eq!(
        git(remote.path(), &["rev-parse", &reference]),
        git(fixture.root.path(), &["rev-parse", &reference])
    );
    assert_eq!(git(remote.path(), &["rev-parse", "main"]), before);
    assert_eq!(git(fixture.root.path(), &["rev-parse", "main"]), before);
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
    let initial = fixture.run(&["dev", "scan", "start"]);
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
    assert_eq!(fixture.run(&["dev", "scan", "show", id]), ready);
    fixture.run(&["daemon", "stop"]);
    fs::remove_dir_all(&draft).unwrap();
    assert!(!fixture.output(&["dev", "scan", "show", id]).status.success());
    let rebuilt = fixture.run(&["dev", "scan", "start"]);
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
        &["show"],
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
        .output(&["dev", "draft", "approve", "B0"])
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
    let started = fixture.run(&["dev", "scan", "start"]);
    let result = fixture.run(&[
        "dev",
        "scan",
        "wait",
        started["id"].as_str().unwrap(),
    ]);
    assert_eq!(result["status"], "failed");
    assert_eq!(
        fs::read_to_string(outside.path().join("keep")).unwrap(),
        "untouched"
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
}

fn add_makefile(fixture: &Fixture, recipe: &str) {
    fs::write(
        fixture.root.path().join("Makefile"),
        format!(".PHONY: test\ntest:\n\t{recipe}\n"),
    )
    .unwrap();
    git(fixture.root.path(), &["add", "Makefile"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "verification fixture"],
    );
}
fn wait_until(mut condition: impl FnMut() -> bool, message: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !condition() {
        assert!(std::time::Instant::now() < deadline, "{message}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
#[cfg(unix)]
fn process_running(pid: &str) -> bool {
    assert!(pid.bytes().all(|b| b.is_ascii_digit()));
    let result = Command::new("ps")
        .args(["-p", pid, "-o", "stat="])
        .output()
        .unwrap();
    let status = String::from_utf8_lossy(&result.stdout);
    result.status.success() && !status.trim().is_empty() && !status.trim().starts_with('Z')
}

#[test]
fn verification_reports_real_success_failure_and_no_tests_and_rejects_lost_logs() {
    for (recipe, expected) in [
        (Some("printf 'real verification\\n'"), "passed"),
        (Some("exit 7"), "failed"),
        (None, "no_tests"),
    ] {
        let fixture = Fixture::new();
        if let Some(recipe) = recipe {
            add_makefile(&fixture, recipe);
        }
        let before = git(fixture.root.path(), &["status", "--porcelain=v1"]);
        let initial = fixture.run(&["dev", "scan", "start"]);
        let id = initial["id"].as_str().unwrap();
        let ready = fixture.ready(id);
        let verification = &ready["report"]["verification"];
        assert_eq!(verification["actor"], "process:towermd-verifier");
        assert_eq!(verification["cleanup_complete"], true);
        assert!(
            verification["modules"]
                .as_array()
                .unwrap()
                .iter()
                .all(|module| module["status"] == expected),
            "{ready}"
        );
        if recipe.is_some() {
            assert_eq!(
                verification["commands"].as_array().unwrap().len(),
                1,
                "共享根命令只执行一次"
            );
            assert!(verification["modules"]
                .as_array()
                .unwrap()
                .iter()
                .all(|module| module["shared"] == true));
        }
        assert_eq!(
            git(fixture.root.path(), &["status", "--porcelain=v1"]),
            before
        );
        assert!(!fixture
            .root
            .path()
            .join(format!(".git/towermd/verify/{id}/worktree"))
            .exists());
        fixture.run(&["daemon", "stop"]);
        assert_eq!(fixture.run(&["dev", "scan", "show", id]), ready);
        if expected == "passed" {
            let log = verification["commands"][0]["log"].as_str().unwrap();
            fs::remove_file(fixture.root.path().join(".git").join(log)).unwrap();
            assert!(
                !fixture.output(&["dev", "scan", "show", id]).status.success(),
                "日志删除后不能沿用 passed"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn cancelling_scan_waits_for_child_group_and_worktree_cleanup() {
    let fixture = Fixture::new();
    // Test-owned marker paths sit outside the detached worktree, so the command
    // itself does not modify tracked source and interfere with this assertion.
    let pid_path = fixture.runtime.path().join("child.pid");
    add_makefile(
        &fixture,
        &format!("sh -c 'sleep 60 & echo $$! > {}; wait'", pid_path.display()),
    );
    let initial = fixture.run(&["dev", "scan", "start"]);
    let id = initial["id"].as_str().unwrap();
    wait_until(|| pid_path.exists(), "验证子进程没有启动");
    let pid = fs::read_to_string(&pid_path).unwrap().trim().to_owned();
    assert!(process_running(&pid));
    let cancelled = fixture.run(&["dev", "scan", "cancel", id]);
    assert_eq!(cancelled["status"], "running");
    assert_eq!(cancelled["cancel_requested"], true);
    let complete = fixture.run(&["dev", "scan", "wait", id]);
    assert_eq!(complete["status"], "cancelled", "{complete}");
    assert_eq!(complete["report"]["verification"]["cleanup_complete"], true);
    wait_until(|| !process_running(&pid), "取消后验证子进程仍存活");
    assert!(!fixture
        .root
        .path()
        .join(format!(".git/towermd/verify/{id}/worktree"))
        .exists());
    fixture.run(&["daemon", "stop"]);
    let restored = fixture.run(&["dev", "scan", "show", id]);
    assert_eq!(restored["status"], "cancelled");
}

#[cfg(unix)]
#[test]
fn daemon_death_closes_worker_pipe_and_cleans_its_children() {
    let fixture = Fixture::new();
    let pid_path = fixture.runtime.path().join("orphan-child.pid");
    add_makefile(
        &fixture,
        &format!("sh -c 'sleep 60 & echo $$! > {}; wait'", pid_path.display()),
    );
    let daemon = endpoint(&fixture);
    let initial = fixture.run(&["dev", "scan", "start"]);
    let id = initial["id"].as_str().unwrap();
    wait_until(|| pid_path.exists(), "验证子进程没有启动");
    let child_pid = fs::read_to_string(&pid_path).unwrap().trim().to_owned();
    let daemon_pid = daemon["pid"].as_u64().unwrap().to_string();
    // This PID came from the daemon started in this fixture's private runtime.
    assert!(Command::new("kill")
        .args(["-KILL", &daemon_pid])
        .status()
        .unwrap()
        .success());
    wait_until(
        || !process_running(&child_pid),
        "daemon 退出后 worker 未清理子进程",
    );
    let worktree = fixture
        .root
        .path()
        .join(format!(".git/towermd/verify/{id}/worktree"));
    wait_until(|| !worktree.exists(), "daemon 退出后 worker 未清理工作树");
    let restored = fixture.run(&["dev", "scan", "show", id]);
    assert_eq!(restored["status"], "failed");
    assert_eq!(restored["phase"], "interrupted");
    assert!(
        restored["report"]["verification"].is_null(),
        "中断不能恢复为通过"
    );
}

#[cfg(unix)]
#[test]
fn worker_eof_cancels_and_inherits_environment_without_joining_stdin_thread() {
    use std::io::Write;
    use towermd_core::catalog::Catalog;
    use towermd_devkit::cold_start;
    use towermd_devkit::verifier::{Limits, VerificationPlan};
    let fixture = Fixture::new();
    let pid_path = fixture.runtime.path().join("direct-worker.pid");
    add_makefile(&fixture, "sh -c 'test \"$$TOWERMD_WORKER_TEST\" = inherited || exit 9; sleep 60 & echo $$! > \"$$TOWERMD_WORKER_PID\"; wait'");
    let id = uuid::Uuid::now_v7().to_string();
    let report = cold_start::scan(&Catalog::load(fixture.root.path()).unwrap(), &id).unwrap();
    let plan = VerificationPlan {
        root: fixture.root.path().to_path_buf(),
        id: id.clone(),
        revision: report.revision,
        tree: report.tree,
        commands: report.commands,
        modules: report.modules,
        limits: Limits::default(),
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
        .arg("verification-worker")
        .env("TOWERMD_RUNTIME_DIR", fixture.runtime.path())
        .env("TOWERMD_WORKER_TEST", "inherited")
        .env("TOWERMD_WORKER_PID", &pid_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    serde_json::to_writer(&mut input, &plan).unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    wait_until(|| pid_path.exists(), "worker 未继承环境或命令未启动");
    let pid = fs::read_to_string(&pid_path).unwrap().trim().to_owned();
    drop(input);
    wait_until(
        || child.try_wait().unwrap().is_some(),
        "worker EOF 后退出超时",
    );
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["Ok"]["cleanup_complete"], true, "{reply}");
    assert_eq!(reply["Ok"]["commands"][0]["status"], "cancelled", "{reply}");
    wait_until(|| !process_running(&pid), "worker EOF 后子进程仍存活");
}

#[test]
fn scanning_an_uninitialized_repository_is_not_initialized() {
    let root = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["commit", "--allow-empty", "-m", "empty"]);
    let output = cli(root.path(), runtime.path(), &["dev", "scan", "start"]);
    let _ = cli(root.path(), runtime.path(), &["daemon", "stop"]);
    let envelope = envelope::parse(&output);
    assert_eq!(envelope["error"]["code"], "not_initialized", "{envelope}");
    assert_eq!(output.status.code(), Some(3));
    assert!(envelope["error"]["message"].as_str().unwrap().contains("towermd init"));
}

#[test]
fn scan_start_retry_with_the_same_request_id_returns_the_same_job() {
    let fixture = Fixture::new();
    let request = uuid::Uuid::now_v7().to_string();
    let first = fixture.run(&["dev", "scan", "start", "--request-id", &request]);
    assert_eq!(first["id"], request, "{first}");
    fixture.ready(&request);
    let again = fixture.run(&["dev", "scan", "start", "--request-id", &request]);
    assert_eq!(again["id"], request);
    assert_eq!(again["status"], "ready");
    // A finished scan is on disk, so the same ID survives a daemon restart.
    fixture.run(&["daemon", "stop"]);
    let restored = fixture.run(&["dev", "scan", "start", "--request-id", &request]);
    assert_eq!(restored["id"], request);
    assert_eq!(restored["status"], "ready");
}
