use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            runtime: tempfile::tempdir().unwrap(),
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.success(fixture.root.path(), &["init", "--docs-dir", "docs/tower"]);
        let docs = fixture.root.path().join("docs/tower");
        fs::write(
            docs.join("arch/A.md"),
            "---\ntype: Module\nid: A\npaths: [src/a/**]\n---\n",
        )
        .unwrap();
        fs::write(
            docs.join("arch/B.md"),
            "---\ntype: Module\nid: B\npaths: [src/b/**]\n---\n",
        )
        .unwrap();
        fs::write(
            docs.join("specs/AC-A.md"),
            "---\ntype: Acceptance\nid: AC-A\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        fs::write(
            docs.join("specs/AC-B.md"),
            "---\ntype: Acceptance\nid: AC-B\nmodule: B\ncriteria: [works]\n---\n",
        )
        .unwrap();
        fixture.git(&["add", "."]);
        fixture.git(&["commit", "-qm", "fixture"]);
        fixture
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(self.root.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    fn command(&self, directory: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_towermd"));
        command
            .args(args)
            .current_dir(directory)
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null());
        command
    }

    fn success(&self, directory: &Path, args: &[&str]) -> Output {
        let output = self.command(directory, args).output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn json(&self, directory: &Path, args: &[&str]) -> Value {
        serde_json::from_slice(&self.success(directory, args).stdout).unwrap()
    }

    fn add(&self, id: &str, module: &str) {
        self.success(
            self.root.path(),
            &[
                "task",
                "add",
                "--id",
                id,
                "--title",
                "test",
                "--module",
                module,
                "--acceptance",
                &format!("AC-{module}"),
                "--allow",
                &format!("src/{}/**", module.to_lowercase()),
            ],
        );
    }

    fn worktree(&self, id: &str) -> PathBuf {
        let path = self.root.path().join(format!("wt-{id}"));
        self.git(&[
            "worktree",
            "add",
            "-qb",
            &format!("t/{id}"),
            path.to_str().unwrap(),
            "main",
        ]);
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.command(self.root.path(), &["daemon", "stop"]).output();
    }
}

#[test]
fn simultaneous_first_calls_start_one_daemon() {
    let fixture = Fixture::new();
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let mut command = fixture.command(fixture.root.path(), &["status"]);
            thread::spawn(move || command.output().unwrap())
        })
        .collect();
    let responses: Vec<Value> = workers
        .into_iter()
        .map(|worker| {
            let output = worker.join().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        })
        .collect();
    assert!(responses
        .iter()
        .all(|value| value["project_id"] == responses[0]["project_id"]));
    let endpoint: Value =
        serde_json::from_slice(&fs::read(fixture.runtime.path().join("endpoint.json")).unwrap())
            .unwrap();
    assert!(endpoint["port"].as_u64().unwrap() > 0);
    let status = fixture.json(fixture.root.path(), &["daemon", "status"]);
    assert_eq!(status["pid"], endpoint["pid"]);
    assert_eq!(status["address"], "127.0.0.1");
    assert_eq!(fixture.git(&["status", "--porcelain"]), "");
}

#[test]
fn worktrees_bind_independently_and_restart_replays() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    fixture.add("T-2", "B");
    let first = fixture.worktree("T-1");
    let second = fixture.worktree("T-2");
    let first_run = fixture.json(&first, &["task", "claim", "T-1"]);
    let second_run = fixture.json(&second, &["task", "claim", "T-2"]);
    assert_ne!(first_run["id"], second_run["id"]);
    assert_eq!(
        fixture.json(&first, &["task", "show"])["id"],
        first_run["id"]
    );
    assert_eq!(
        fixture.json(&second, &["task", "show"])["id"],
        second_run["id"]
    );
    assert!(first_run["cost_usd"].is_null());
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    assert_eq!(
        fixture.json(&first, &["task", "show"])["id"],
        first_run["id"]
    );
    assert_eq!(
        fixture.json(&second, &["task", "show"])["id"],
        second_run["id"]
    );
    assert_eq!(fixture.json(&first, &["task", "release"])["state"], "ready");
    let output = fixture.command(&first, &["task", "show"]).output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn competing_claims_have_one_winner_and_no_worktree_writes() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    fixture.add("T-2", "A");
    let first = fixture.worktree("T-1");
    let second = fixture.worktree("T-2");
    let mut first_command = fixture.command(&first, &["task", "claim", "T-1"]);
    let mut second_command = fixture.command(&second, &["task", "claim", "T-2"]);
    let first_thread = thread::spawn(move || first_command.output().unwrap());
    let second_thread = thread::spawn(move || second_command.output().unwrap());
    let outputs = [first_thread.join().unwrap(), second_thread.join().unwrap()];
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    for directory in [&first, &second] {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(directory)
            .output()
            .unwrap();
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn duplicate_clone_identity_is_rejected_but_other_project_works() {
    let fixture = Fixture::new();
    fixture.success(fixture.root.path(), &["status"]);
    let duplicate = tempfile::tempdir().unwrap();
    fixture.git(&[
        "clone",
        "-q",
        fixture.root.path().to_str().unwrap(),
        duplicate.path().to_str().unwrap(),
    ]);
    let output = fixture
        .command(duplicate.path(), &["status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("clone"));
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    let output = fixture
        .command(duplicate.path(), &["status"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "重启不能在第二个 clone 建立分叉 hub"
    );
    let other = Fixture::new();
    let status = fixture.json(other.root.path(), &["status"]);
    let original = fixture.json(fixture.root.path(), &["status"]);
    assert_ne!(status["project_id"], original["project_id"]);
}

#[cfg(unix)]
#[test]
fn abrupt_daemon_exit_preserves_acknowledged_events_and_host() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let endpoint = fixture.json(fixture.root.path(), &["daemon", "status"]);
    let pid = endpoint["pid"].as_u64().unwrap().to_string();
    assert!(Command::new("kill")
        .args(["-KILL", &pid])
        .status()
        .unwrap()
        .success());
    let recovered = fixture.json(&worktree, &["task", "show"]);
    assert_eq!(recovered["id"], run["id"]);
    assert_eq!(recovered["host"], run["host"]);
    let restarted = fixture.json(fixture.root.path(), &["daemon", "status"]);
    assert_ne!(restarted["instance"], endpoint["instance"]);
    assert_eq!(restarted["host"], endpoint["host"]);
}

#[test]
fn no_human_or_completion_cli_and_main_claim_rejected() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    for args in [
        vec!["task", "claim", "T-1"],
        vec!["approve"],
        vec!["task", "complete", "T-1"],
        vec!["status", "--actor", "human:test"],
    ] {
        let output = fixture
            .command(fixture.root.path(), &args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
    }
    let status = fixture.json(fixture.root.path(), &["status"]);
    assert!(status["runs"].as_array().unwrap().is_empty());
}

#[test]
fn daemon_management_commands_renew_the_calling_worktree() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let journal = fixture.root.path().join(".git/towermd/journal.json");
    for args in [vec!["daemon", "status"], vec!["daemon", "stop"]] {
        let before: Value = serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
        fixture.success(&worktree, &args);
        let after: Value = serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
        assert!(
            after["events"].as_array().unwrap().len() > before["events"].as_array().unwrap().len(),
            "{args:?} 未续租"
        );
        let latest = after["events"].as_array().unwrap().last().unwrap();
        assert_eq!(latest["type"], "lease.renewed");
        assert_eq!(latest["obj"], run["id"]);
    }
}

#[test]
fn unbound_lifecycle_commands_do_not_restart_or_require_a_branch() {
    let fixture = Fixture::new();
    fixture.success(fixture.root.path(), &["status"]);
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    assert_eq!(
        fixture.json(fixture.root.path(), &["daemon", "status"])["running"],
        false
    );
    fixture.success(fixture.root.path(), &["status"]);
    fixture.git(&["checkout", "--detach"]);
    fixture.success(fixture.root.path(), &["init"]);
    fixture.success(fixture.root.path(), &["daemon", "status"]);
    let output = fixture
        .command(fixture.root.path(), &["init", "--run", "missing"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn invalid_explicit_run_does_not_stop_the_daemon() {
    let fixture = Fixture::new();
    fixture.success(fixture.root.path(), &["status"]);
    let before = fixture.json(fixture.root.path(), &["daemon", "status"]);
    let output = fixture
        .command(fixture.root.path(), &["daemon", "stop", "--run", "missing"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "无效显式运行不得停机成功");
    assert_eq!(
        fixture.json(fixture.root.path(), &["daemon", "status"])["instance"],
        before["instance"]
    );
}

#[cfg(unix)]
#[test]
fn daemon_survives_interrupt_to_the_first_clients_process_group() {
    use std::os::unix::process::CommandExt;
    let fixture = Fixture::new();
    let mut command = fixture.command(fixture.root.path(), &["status"]);
    command
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().unwrap();
    let client_group = child.id();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let before = fixture.json(fixture.root.path(), &["daemon", "status"]);
    let _ = Command::new("/bin/kill")
        .args(["-INT", "--", &format!("-{client_group}")])
        .output()
        .unwrap();
    thread::sleep(Duration::from_millis(50));
    let after = fixture.json(fixture.root.path(), &["daemon", "status"]);
    assert_eq!(
        after["instance"], before["instance"],
        "Ctrl-C 不得终止共享 daemon"
    );
}

#[test]
fn claim_accepts_harness_self_report_without_changing_actor() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    fixture.success(
        &worktree,
        &[
            "task",
            "claim",
            "T-1",
            "--harness",
            "example",
            "--harness-version",
            "1.0",
        ],
    );
    let journal: Value = serde_json::from_slice(
        &fs::read(fixture.root.path().join(".git/towermd/journal.json")).unwrap(),
    )
    .unwrap();
    let claimed = journal["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "task.claimed")
        .unwrap();
    assert_eq!(claimed["actor"], "process:towermd-cli");
    assert_eq!(claimed["data"]["self_reported"]["harness"], "example");
    assert_eq!(claimed["data"]["self_reported"]["version"], "1.0");
}

fn event_socket(fixture: &Fixture, after: u64) -> TcpStream {
    let endpoint = fixture.json(fixture.root.path(), &["daemon", "status"]);
    let port = endpoint["port"].as_u64().unwrap();
    let mut url = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/events")).unwrap();
    url.query_pairs_mut()
        .append_pair("directory", fixture.root.path().to_str().unwrap())
        .append_pair("after", &after.to_string())
        .append_pair("instance", endpoint["instance"].as_str().unwrap());
    let mut socket = TcpStream::connect(("127.0.0.1", port as u16)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(socket, "GET {}?{} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", url.path(), url.query().unwrap()).unwrap();
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        socket.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
        assert!(header.len() < 8192);
    }
    assert!(
        String::from_utf8_lossy(&header).starts_with("HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&header)
    );
    socket
}

fn receive_event(socket: &mut TcpStream) -> Value {
    let mut header = [0; 2];
    socket.read_exact(&mut header).unwrap();
    assert_eq!(header[0], 0x81);
    assert_eq!(header[1] & 0x80, 0);
    let length = match header[1] & 0x7f {
        126 => {
            let mut bytes = [0; 2];
            socket.read_exact(&mut bytes).unwrap();
            u16::from_be_bytes(bytes) as usize
        }
        127 => {
            let mut bytes = [0; 8];
            socket.read_exact(&mut bytes).unwrap();
            usize::try_from(u64::from_be_bytes(bytes)).unwrap()
        }
        length => length as usize,
    };
    assert!(length < 1_000_000);
    let mut bytes = vec![0; length];
    socket.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn websocket_replays_since_cursor_and_continues_with_new_events() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let mut socket = event_socket(&fixture, 0);
    let first = receive_event(&mut socket);
    assert_eq!(first["seq"], 1);
    assert_eq!(first["type"], "task.state");
    fixture.add("T-2", "B");
    let second = receive_event(&mut socket);
    assert_eq!(second["seq"], 2);
    assert_eq!(second["obj"], "T-2");
    drop(socket);
    let mut resumed = event_socket(&fixture, 1);
    assert_eq!(receive_event(&mut resumed)["seq"], 2);
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
}

fn journal(fixture: &Fixture) -> Value {
    serde_json::from_slice(
        &fs::read(fixture.root.path().join(".git/towermd/journal.json")).unwrap(),
    )
    .unwrap()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(std::time::Instant::now() < deadline, "等待条件超时");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn snapshot_checked_main_reports_deferred_without_worktree_writes() {
    for linked in [false, true] {
        let fixture = Fixture::new();
        let linked_root = tempfile::tempdir().unwrap();
        let checked_root = if linked {
            fixture.git(&["switch", "--detach"]);
            fixture.git(&[
                "worktree",
                "add",
                linked_root.path().to_str().unwrap(),
                "main",
            ]);
            linked_root.path()
        } else {
            fixture.root.path()
        };
        let checked_path = checked_root.to_str().unwrap();
        fixture.add("T-1", "A");
        let before = fixture.git(&["rev-parse", "main"]);
        let index = fixture.git(&["-C", checked_path, "ls-files", "--stage"]);
        assert_eq!(fixture.git(&["remote"]), "");
        let report = fixture.json(fixture.root.path(), &["snapshot"]);
        assert_eq!(report["status"], "deferred", "linked main: {linked}");
        assert!(report["commit"].is_null());
        assert!(report["reason"].as_str().unwrap().contains("main"));
        assert_eq!(fixture.git(&["rev-parse", "main"]), before);
        assert_eq!(
            fixture.git(&["-C", checked_path, "symbolic-ref", "HEAD"]),
            "refs/heads/main"
        );
        assert_eq!(
            fixture.git(&["-C", checked_path, "ls-files", "--stage"]),
            index
        );
        assert_eq!(
            fixture.git(&["-C", checked_path, "status", "--porcelain"]),
            ""
        );
        assert!(!checked_root.join("docs/tower/state/state.json").exists());
        let saved = journal(&fixture);
        assert!(saved["snapshot_ack"].is_null());
        assert!(saved["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["type"] != "snapshot.written"));

        fixture.git(&["-C", checked_path, "switch", "--detach"]);
        let report = fixture.json(fixture.root.path(), &["snapshot"]);
        assert_eq!(report["status"], "written");
        assert_eq!(fixture.git(&["rev-parse", "main"]), report["commit"]);
        let state: Value =
            serde_json::from_str(&fixture.git(&["show", "main:docs/tower/state/state.json"]))
                .unwrap();
        assert_eq!(state["through_seq"], report["seq"]);
        assert_eq!(
            state["projection"]["objects"]["Task"]["T-1"]["state"],
            "ready"
        );
        assert!(!checked_root.join("docs/tower/state/state.json").exists());
    }
}

#[test]
fn snapshot_cli_writes_through_daemon_with_process_actor() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let report = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(report["status"], "written");
    assert_eq!(report["commit"], fixture.git(&["rev-parse", "main"]));
    let state: Value =
        serde_json::from_str(&fixture.git(&["show", "main:docs/tower/state/state.json"])).unwrap();
    assert_eq!(state["through_seq"], report["seq"]);
    assert_eq!(
        state["projection"]["objects"]["Task"]["T-1"]["state"],
        "ready"
    );
    let saved = journal(&fixture);
    assert_eq!(saved["snapshot_ack"]["commit"], report["commit"]);
    assert!(saved["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| { event["actor"].as_str().unwrap().starts_with("process:") }));
    assert_eq!(fixture.git(&["status", "--porcelain"]), "");
}

#[test]
fn snapshot_dirty_path_reports_deferred_and_preserves_ack() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let first = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(first["status"], "written");
    fixture.git(&["switch", "main"]);
    let path = fixture.root.path().join("docs/tower/state/state.json");
    fs::write(&path, "user edits\n").unwrap();
    fixture.add("T-2", "B");
    let report = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(report["status"], "deferred");
    assert!(report["commit"].is_null());
    assert!(!report["reason"].as_str().unwrap().is_empty());
    assert_eq!(fixture.git(&["rev-parse", "main"]), first["commit"]);
    assert_eq!(journal(&fixture)["snapshot_ack"]["seq"], first["seq"]);
    assert_eq!(fs::read_to_string(path).unwrap(), "user edits\n");
}

#[test]
fn snapshot_rejects_invalid_explicit_run_before_writing_git() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let before = fixture.git(&["rev-parse", "main"]);
    let output = fixture
        .command(fixture.root.path(), &["snapshot", "--run", "missing-run"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing-run"));
    assert_eq!(fixture.git(&["rev-parse", "main"]), before);
    assert!(!fixture
        .root
        .path()
        .join("docs/tower/state/state.json")
        .exists());
    assert!(journal(&fixture)["snapshot_ack"].is_null());
}

#[test]
fn snapshot_renews_selected_run_before_checkpoint() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let report = fixture.json(
        fixture.root.path(),
        &["snapshot", "--run", run["id"].as_str().unwrap()],
    );
    assert_eq!(report["status"], "written");
    let saved = journal(&fixture);
    let renewed = saved["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "lease.renewed" && event["obj"] == run["id"])
        .expect("快照前必须续租显式 Run");
    assert!(renewed["seq"].as_u64().unwrap() <= report["seq"].as_u64().unwrap());
}

#[test]
fn periodic_snapshot_uses_main_interval_and_only_business_changes() {
    let fixture = Fixture::new();
    let config_path = fixture.root.path().join(".towermd/config.yaml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(&config_path, format!("{config}\nsnapshot_interval: 1\n")).unwrap();
    fixture.git(&["add", ".towermd/config.yaml"]);
    fixture.git(&["commit", "-qm", "one second snapshots"]);
    fixture.git(&["switch", "--detach"]);
    fixture.success(fixture.root.path(), &["status"]);
    let before = fixture.git(&["rev-parse", "main"]);
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(fixture.git(&["rev-parse", "main"]), before);
    fixture.add("T-1", "A");
    wait_until(|| journal(&fixture)["snapshot_ack"]["commit"].is_string());
    let first = fixture.git(&["rev-parse", "main"]);
    assert_ne!(first, before);
    thread::sleep(Duration::from_millis(2200));
    assert_eq!(fixture.git(&["rev-parse", "main"]), first);
    fixture.add("T-2", "B");
    wait_until(|| fixture.git(&["rev-parse", "main"]) != first);
}

#[test]
fn periodic_snapshot_uses_latest_worktree_after_registered_worktree_is_removed() {
    let fixture = Fixture::new();
    let config_path = fixture.root.path().join(".towermd/config.yaml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(&config_path, format!("{config}\nsnapshot_interval: 1\n")).unwrap();
    fixture.git(&["add", ".towermd/config.yaml"]);
    fixture.git(&["commit", "-qm", "one second snapshots"]);
    fixture.git(&["switch", "--detach"]);
    let first = fixture.worktree("old");
    let second = fixture.worktree("current");
    fixture.success(&first, &["status"]);
    fixture.git(&["worktree", "remove", first.to_str().unwrap()]);
    assert!(!first.exists());
    let before = fixture.git(&["rev-parse", "main"]);
    fixture.success(
        &second,
        &[
            "task",
            "add",
            "--id",
            "T-1",
            "--title",
            "test",
            "--module",
            "A",
            "--acceptance",
            "AC-A",
            "--allow",
            "src/a/**",
        ],
    );
    wait_until(|| journal(&fixture)["snapshot_ack"]["commit"].is_string());
    let commit = fixture.git(&["rev-parse", "main"]);
    assert_ne!(commit, before);
    assert_eq!(journal(&fixture)["snapshot_ack"]["commit"], commit);
    let state: Value =
        serde_json::from_str(&fixture.git(&["show", "main:docs/tower/state/state.json"])).unwrap();
    assert_eq!(
        state["projection"]["objects"]["Task"]["T-1"]["state"],
        "ready"
    );
}

#[test]
fn restart_reconciles_git_snapshot_without_ack_before_serving_tasks() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let report = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(report["status"], "written");
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    let mut saved = journal(&fixture);
    saved["snapshot_ack"] = Value::Null;
    saved["events"]
        .as_array_mut()
        .unwrap()
        .retain(|event| event["type"] != "snapshot.written");
    fs::write(
        fixture.root.path().join(".git/towermd/journal.json"),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
    fixture.success(fixture.root.path(), &["status"]);
    wait_until(|| journal(&fixture)["snapshot_ack"]["commit"] == report["commit"]);
    assert_eq!(fixture.git(&["rev-parse", "main"]), report["commit"]);
}

#[test]
fn missing_hub_with_main_snapshot_refuses_new_task() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let report = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(report["status"], "written");
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    fs::remove_dir_all(fixture.root.path().join(".git/towermd")).unwrap();
    let output = fixture
        .command(
            fixture.root.path(),
            &[
                "task",
                "add",
                "--id",
                "T-2",
                "--title",
                "new",
                "--module",
                "B",
                "--acceptance",
                "AC-B",
                "--allow",
                "src/b/**",
            ],
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("恢复"));
}

#[cfg(unix)]
#[test]
fn blocked_periodic_push_keeps_all_main_leases_alive_and_shutdown_waits() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};
    use towermd_core::catalog::Catalog;
    use towermd_core::hub::Hub;
    use towermd_core::snapshot_service::SnapshotWriter;
    use towermd_core::tasks::{self, TaskCommand, TaskSpec};

    let fixture = Fixture::new();
    let other = Fixture::new();
    fs::write(
        fixture.runtime.path().join("host.json"),
        "\"snapshot-test-host\"",
    )
    .unwrap();
    for project in [&fixture, &other] {
        let config_path = project.root.path().join(".towermd/config.yaml");
        let config = fs::read_to_string(&config_path).unwrap();
        fs::write(&config_path, format!("{config}\nsnapshot_interval: 1\n")).unwrap();
        project.git(&["add", ".towermd/config.yaml"]);
        project.git(&["commit", "-qm", "short interval"]);
        project.git(&["switch", "--detach"]);
        let catalog = Catalog::load(project.root.path()).unwrap();
        let hub = Mutex::new(Hub::open(&catalog.common_dir.join("towermd")).unwrap());
        let earlier = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 25_000;
        tasks::execute(
            &mut hub.lock().unwrap(),
            &catalog,
            "snapshot-test-host",
            TaskCommand::Add {
                task: TaskSpec {
                    id: "T-1".into(),
                    title: "test".into(),
                    module: "A".into(),
                    touches: Vec::new(),
                    allowed_paths: vec!["src/a/**".into()],
                    depends_on: Vec::new(),
                    acceptance: vec!["AC-A".into()],
                    decisions: Vec::new(),
                },
            },
            None,
            earlier,
        )
        .unwrap();
        let report = SnapshotWriter::default()
            .write(&hub, &catalog, "snapshot-test-host", true, || Ok(earlier))
            .unwrap();
        assert_eq!(report.status, "written");
    }
    let remote = tempfile::tempdir().unwrap();
    fixture.git(&[
        "clone",
        "--bare",
        "-q",
        fixture.root.path().to_str().unwrap(),
        remote.path().to_str().unwrap(),
    ]);
    fixture.git(&["remote", "add", "origin", remote.path().to_str().unwrap()]);
    towermd_core::origin::refresh_origin_main(fixture.root.path()).unwrap();
    let hook = remote.path().join("hooks/pre-receive");
    fs::write(&hook, "#!/bin/sh\nprintf 'push\\n' >> snapshot-pushes\ntouch snapshot-blocked\nwhile [ ! -e snapshot-release ]; do sleep 0.05; done\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.success(other.root.path(), &["status"]);
    fixture.add("T-2", "B");
    wait_until(|| remote.path().join("snapshot-blocked").exists());
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let endpoint = fixture.json(fixture.root.path(), &["daemon", "status"]);
    let base = format!("http://127.0.0.1:{}", endpoint["port"]);
    let response = client.post(format!("{base}/command"))
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .json(&serde_json::json!({"directory":fixture.root.path(), "run":null, "command":{"action":"status"}}))
        .send().unwrap();
    assert!(response.status().is_success());
    let response: Value = client
        .post(format!("{base}/snapshot"))
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .json(&serde_json::json!({"directory":fixture.root.path(), "run":null}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(response["status"], "deferred");
    wait_until(|| {
        [&fixture, &other].iter().all(|project| {
            journal(project)["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["type"] == "lease.renewed" && event["obj"] == "main")
        })
    });
    assert_eq!(
        fs::read_to_string(remote.path().join("snapshot-pushes")).unwrap(),
        "push\n"
    );
    let mut stop = fixture
        .command(fixture.root.path(), &["daemon", "stop"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(300));
    assert!(
        stop.try_wait().unwrap().is_none(),
        "停止不能丢下后台 Git 写入"
    );
    fs::write(remote.path().join("snapshot-release"), "").unwrap();
    assert!(stop.wait().unwrap().success());
    let remote_head = fixture.git(&[
        "--git-dir",
        remote.path().to_str().unwrap(),
        "rev-parse",
        "main",
    ]);
    assert_eq!(journal(&fixture)["snapshot_ack"]["commit"], remote_head);
}

#[test]
fn snapshot_endpoint_requires_instance_and_rejects_approval_fields() {
    let fixture = Fixture::new();
    fixture.success(fixture.root.path(), &["status"]);
    let endpoint = fixture.json(fixture.root.path(), &["daemon", "status"]);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{}/snapshot", endpoint["port"]);
    let response = client
        .post(&url)
        .json(&serde_json::json!({"directory":fixture.root.path(), "run":null}))
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let response = client
        .post(&url)
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .json(&serde_json::json!({"directory":fixture.root.path(), "run":null, "approve":true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn hub_rebuild_only_prepares_request_with_missing_or_corrupt_journal() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let snapshot = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(snapshot["status"], "written");
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    let hub_dir = fixture.root.path().join(".git/towermd");
    fs::remove_dir_all(&hub_dir).unwrap();
    let request = fixture.json(fixture.root.path(), &["hub", "rebuild"]);
    assert_eq!(request["status"], "awaiting_app_confirmation");
    assert_eq!(request["source_commit"], snapshot["commit"]);
    assert_eq!(request["main_revision"], snapshot["commit"]);
    assert_eq!(request["source_seq"], snapshot["seq"]);
    assert_eq!(
        request["seq_boundary"].as_u64().unwrap(),
        snapshot["seq"].as_u64().unwrap() + (1 << 32)
    );
    assert!(request["epoch_boundary"].as_u64().unwrap() > (1 << 32));
    assert!(!request["request_id"].as_str().unwrap().is_empty());
    assert!(!request["project_id"].as_str().unwrap().is_empty());
    assert!(!hub_dir.exists(), "请求恢复不能创建空 hub");
    fs::create_dir_all(&hub_dir).unwrap();
    fs::write(hub_dir.join("journal.json"), "broken journal").unwrap();
    let output = fixture.success(fixture.root.path(), &["hub", "rebuild"]);
    let mut corrupted: Value = serde_json::from_slice(&output.stdout).unwrap();
    let warnings = corrupted
        .as_object_mut()
        .unwrap()
        .remove("warnings")
        .unwrap();
    assert!(warnings
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| { warning.as_str().unwrap().contains("隐式续租失败") }));
    assert!(String::from_utf8_lossy(&output.stderr).contains("隐式续租失败"));
    assert_eq!(corrupted, request);
    assert_eq!(
        fs::read_to_string(hub_dir.join("journal.json")).unwrap(),
        "broken journal"
    );
    assert_eq!(fixture.git(&["rev-parse", "main"]), snapshot["commit"]);
    let output = fixture
        .command(
            fixture.root.path(),
            &["hub", "rebuild", "--run", "missing-run"],
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(hub_dir.join("journal.json")).unwrap(),
        "broken journal"
    );
}

#[test]
fn hub_rebuild_implicitly_extends_bound_run_expiry_including_after_restart() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let snapshot = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(snapshot["status"], "written");
    for restart in [false, true] {
        if restart {
            fixture.success(fixture.root.path(), &["daemon", "stop"]);
        }
        let before = journal(&fixture);
        let previous = before["events"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|event| event["lease_delta"]["updated"][0]["holder"] == run["id"])
            .unwrap()["lease_delta"]["updated"]
            .as_array()
            .unwrap();
        thread::sleep(Duration::from_millis(20));
        let request = fixture.json(&worktree, &["hub", "rebuild"]);
        assert_eq!(request["status"], "awaiting_app_confirmation");
        assert!(request.get("warnings").is_none());
        let after = journal(&fixture);
        let renewed = after["events"].as_array().unwrap()
            [before["events"].as_array().unwrap().len()..]
            .iter()
            .find(|event| event["obj"] == run["id"] && event["type"] == "lease.renewed")
            .expect("隐式绑定的 Run 必须续租");
        assert_eq!(renewed["actor"], "process:towermd-cli");
        let leases = renewed["lease_delta"]["updated"].as_array().unwrap();
        assert_eq!(leases.len(), previous.len());
        assert!(!leases.is_empty());
        for old in previous {
            let current = leases
                .iter()
                .find(|lease| lease["name"] == old["name"])
                .unwrap();
            assert!(current["expires_at"].as_u64().unwrap() > old["expires_at"].as_u64().unwrap());
        }
        assert_eq!(fixture.git(&["rev-parse", "main"]), snapshot["commit"]);
    }
}

#[test]
fn hub_rebuild_explicit_run_with_missing_journal_does_not_create_empty_hub() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let report = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(report["status"], "written");
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    let hub_dir = fixture.root.path().join(".git/towermd");
    fs::remove_dir_all(&hub_dir).unwrap();
    let output = fixture
        .command(
            fixture.root.path(),
            &["hub", "rebuild", "--run", "missing-run"],
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!hub_dir.exists(), "恢复请求不能为了校验 Run 创建空 hub");
}

#[test]
fn hub_rebuild_validates_and_renews_explicit_run() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let snapshot = fixture.json(fixture.root.path(), &["snapshot"]);
    assert_eq!(snapshot["status"], "written");
    let before = journal(&fixture)["events"].as_array().unwrap().len();
    let request = fixture.json(
        fixture.root.path(),
        &["hub", "rebuild", "--run", run["id"].as_str().unwrap()],
    );
    assert_eq!(request["status"], "awaiting_app_confirmation");
    assert!(journal(&fixture)["events"].as_array().unwrap()[before..]
        .iter()
        .any(|event| event["type"] == "lease.renewed" && event["obj"] == run["id"]));
    let output = fixture
        .command(
            fixture.root.path(),
            &["hub", "rebuild", "--run", "missing-run"],
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing-run"));
    assert_eq!(fixture.git(&["rev-parse", "main"]), snapshot["commit"]);
}

#[test]
fn hub_rebuild_has_no_approval_or_execution_switches() {
    let fixture = Fixture::new();
    let help = fixture.success(fixture.root.path(), &["hub", "rebuild", "--help"]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("App"));
    for args in [
        vec!["hub", "rebuild", "--approve"],
        vec!["hub", "rebuild", "--yes"],
        vec!["hub", "rebuild", "--execute"],
    ] {
        let output = fixture
            .command(fixture.root.path(), &args)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    fixture.success(fixture.root.path(), &["status"]);
    let endpoint = fixture.json(fixture.root.path(), &["daemon", "status"]);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{}/rebuild-request", endpoint["port"]);
    let response = client
        .post(&url)
        .json(&serde_json::json!({"directory":fixture.root.path(), "run":null}))
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    for extra in ["approve", "yes", "execute", "actor"] {
        let mut body = serde_json::json!({"directory":fixture.root.path(), "run":null});
        body[extra] = serde_json::json!(true);
        let response = client
            .post(&url)
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .json(&body)
            .send()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    }
}

#[test]
fn init_does_not_validate_run_or_renew_when_daemon_is_live() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    fixture.json(&worktree, &["task", "claim", "T-1"]);
    let before = journal(&fixture);
    fixture.success(
        &worktree,
        &["--run", "missing-run", "init", "--docs-dir", "docs/tower"],
    );
    let after = journal(&fixture);
    let old_count = before["events"].as_array().unwrap().len();
    assert!(!after["events"].as_array().unwrap()[old_count..]
        .iter()
        .any(|event| event["type"] == "lease.renewed" && event["obj"] != "main"));
}

fn offline_cached_origin(fixture: &Fixture) {
    let main = fixture.git(&["rev-parse", "main"]);
    fixture.git(&["update-ref", "refs/remotes/origin/main", &main]);
    fixture.git(&[
        "remote",
        "add",
        "origin",
        "/nonexistent/towermd-offline-daemon",
    ]);
}

#[test]
fn offline_cached_main_keeps_status_show_and_implicit_renewal_available() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    offline_cached_origin(&fixture);
    let before = journal(&fixture)["events"].as_array().unwrap().len();
    let start = std::time::Instant::now();
    let status = fixture.json(&worktree, &["status"]);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(status["main"]["reference"], "refs/remotes/origin/main");
    let shown = fixture.json(&worktree, &["task", "show"]);
    assert_eq!(shown["id"], run["id"]);
    let after = journal(&fixture);
    assert!(after["events"].as_array().unwrap()[before..]
        .iter()
        .any(|event| event["type"] == "lease.renewed" && event["obj"] == run["id"]));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let value = fixture.json(&worktree, &["status"]);
        if value["main"]["last_fetch_error"].is_string() {
            assert!(value["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning.as_str().unwrap().contains("fetch")));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fetch 失败诊断未出现：{value}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn remote_project_without_cached_authority_reports_status_but_refuses_task_reads() {
    let fixture = Fixture::new();
    fixture.git(&[
        "remote",
        "add",
        "origin",
        "/nonexistent/towermd-first-fetch",
    ]);
    let status = fixture.json(fixture.root.path(), &["status"]);
    assert_eq!(status["status"], "authority_unavailable");
    assert_eq!(status["main"]["reference"], "refs/remotes/origin/main");
    assert!(status["main"]["fetched_at"].is_null());
    let output = fixture
        .command(fixture.root.path(), &["task", "show", "T-1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("权威 main"));
    assert!(!fixture
        .root
        .path()
        .join(".git/towermd/journal.json")
        .exists());
}

#[cfg(unix)]
#[test]
fn ten_status_requests_share_one_nonblocking_background_fetch() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    // 预热实际项目命令，daemon status 本身不会启动 daemon。
    fixture.success(fixture.root.path(), &["status"]);
    let remote = fixture.runtime.path().join("remote.git");
    fixture.git(&["clone", "--bare", ".", remote.to_str().unwrap()]);
    let cached = fixture.git(&["rev-parse", "main"]);
    fixture.git(&["update-ref", "refs/remotes/origin/main", &cached]);
    fixture.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let counter = fixture.runtime.path().join("fetch-count");
    let release = fixture.runtime.path().join("fetch-release");
    let upload = fixture.runtime.path().join("count-upload-pack");
    fs::write(&upload, format!(
        "#!/bin/sh\nprintf 'fetch\\n' >> '{}'\ni=0\nwhile [ ! -f '{}' ] && [ \"$i\" -lt 300 ]; do sleep 0.01; i=$((i+1)); done\nexec git-upload-pack \"$@\"\n",
        counter.display(), release.display())).unwrap();
    fs::set_permissions(&upload, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.git(&[
        "config",
        "remote.origin.uploadpack",
        upload.to_str().unwrap(),
    ]);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(10));
    let started = std::time::Instant::now();
    let workers: Vec<_> = (0..10)
        .map(|_| {
            let barrier = barrier.clone();
            let mut command = fixture.command(fixture.root.path(), &["status"]);
            thread::spawn(move || {
                barrier.wait();
                command.output().unwrap()
            })
        })
        .collect();
    for worker in workers {
        let output = worker.join().unwrap();
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["main"]["reference"], "refs/remotes/origin/main");
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "status 等待了 fetch"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !counter.exists() {
        assert!(std::time::Instant::now() < deadline, "后台 fetch 未启动");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
    fs::write(&release, "release").unwrap();
    fixture.success(fixture.root.path(), &["status"]);
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
}

#[cfg(unix)]
#[test]
fn daemon_does_not_retain_inherited_client_output_descriptors() {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    let fixture = Fixture::new();
    let (mut reader, writer) = std::io::pipe().unwrap();
    let inherited = writer.as_raw_fd();
    let mut client = fixture.command(fixture.root.path(), &["status"]);
    // Deliberately reproduce a descriptor inherited during concurrent pipe creation.
    // Only the forked CLI changes its descriptor flags.
    unsafe {
        client.pre_exec(move || {
            let flags = fcntl(inherited, 1);
            if flags == -1 || fcntl(inherited, 2, flags & !1) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = client.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    drop(writer);
    let (sender, receiver) = std::sync::mpsc::channel();
    let reading = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = sender.send(reader.read_to_end(&mut bytes));
    });
    let eof = receiver.recv_timeout(Duration::from_secs(2));
    assert!(fixture.json(fixture.root.path(), &["daemon", "status"])["pid"].is_u64());
    // Clean up before asserting, including when the legacy daemon retained the pipe.
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    reading.join().unwrap();
    assert_eq!(
        eof.expect("daemon retained client pipe after CLI exit")
            .unwrap(),
        0
    );
}
