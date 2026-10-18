use serde_json::{json, Value};
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

    fn snapshot_when_idle(&self, directory: &Path, run: Option<&str>) -> Value {
        let mut args = vec!["snapshot"];
        if let Some(run) = run {
            args.extend(["--run", run]);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut attempts = 0;
        loop {
            attempts += 1;
            let output = self.command(directory, &args).output().unwrap();
            if !output.status.success() {
                eprintln!(
                    "snapshot request {attempts} stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    plain_snapshot_source_change(output.status.code(), &output.stderr),
                    "{args:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    std::time::Instant::now() < deadline,
                    "snapshot source kept changing, request {attempts}"
                );
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            if report["status"] != "deferred" {
                assert_eq!(report["status"], "written", "attempt {attempts}: {report}");
                eprintln!("snapshot written by request {attempts}");
                return report;
            }
            assert_eq!(
                report["reason"], "快照写入正在进行",
                "attempt {attempts}: {report}"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "snapshot writer remained busy, attempt {attempts}: {report}"
            );
            eprintln!("snapshot waits for active writer, attempt {attempts}: {report}");
            thread::sleep(Duration::from_millis(50));
        }
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
        if thread::panicking() {
            if let Ok(log) = fs::read_to_string(self.runtime.path().join("daemon.log")) {
                eprintln!(
                    "daemon failure log ({}):\n{log}",
                    self.root.path().display()
                );
            }
        }
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

#[cfg(unix)]
#[test]
fn cold_project_archive_does_not_block_registered_project_reads() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;
    use std::time::Instant;

    let fixture = Fixture::new();
    let cold = Fixture::new();
    cold.success(cold.root.path(), &["status"]);
    cold.snapshot_when_idle(cold.root.path(), None);
    cold.success(cold.root.path(), &["daemon", "stop"]);
    let archive = cold.git(&[
        "ls-tree",
        "-r",
        "--name-only",
        "refs/towermd/snapshot",
        "--",
        "docs/tower/state/events",
    ]);
    let archive = archive.lines().next().expect("real snapshot month");
    let blob = cold.git(&["rev-parse", &format!("refs/towermd/snapshot:{archive}")]);
    let wrapper = fixture.runtime.path().join("git-wrapper");
    fs::create_dir(&wrapper).unwrap();
    let real_git = Command::new("which").arg("git").output().unwrap();
    assert!(real_git.status.success());
    let program = wrapper.join("git");
    fs::write(
        &program,
        r#"#!/bin/sh
cat_file=0
blob=0
month=0
for argument in "$@"; do
    if test "$argument" = cat-file; then cat_file=1; fi
    if test "$argument" = blob; then blob=1; fi
    if test "$argument" = "$TOWERMD_REGISTRATION_BLOB"; then month=1; fi
done
if test "$cat_file$blob$month" = 111 && test "$(pwd -P)" = "$TOWERMD_REGISTRATION_ROOT"; then
    printf 'read\n' >> "$TOWERMD_REGISTRATION_GATES/archive-count"
    if mkdir "$TOWERMD_REGISTRATION_GATES/archive-once" 2>/dev/null; then
        touch "$TOWERMD_REGISTRATION_GATES/archive-entered"
        attempts=0
        while ! test -f "$TOWERMD_REGISTRATION_GATES/archive-release"; do
            attempts=$((attempts + 1))
            if test "$attempts" -ge 1000; then exit 98; fi
            sleep 0.01
        done
    fi
fi
if test -f "$TOWERMD_REGISTRATION_GATES/registered-trace-armed" && test "$(pwd -P)" != "$TOWERMD_REGISTRATION_ROOT"; then
    printf '%s\n' "$*" >> "$TOWERMD_REGISTRATION_GATES/registered-git-log"
fi
exec "$TOWERMD_REGISTRATION_REAL_GIT" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let output = fixture
        .command(fixture.root.path(), &["status"])
        .env(
            "PATH",
            format!("{}:{}", wrapper.display(), std::env::var("PATH").unwrap()),
        )
        .env("TOWERMD_REGISTRATION_GATES", fixture.runtime.path())
        .env("TOWERMD_REGISTRATION_BLOB", blob)
        .env(
            "TOWERMD_REGISTRATION_ROOT",
            fs::canonicalize(cold.root.path()).unwrap(),
        )
        .env(
            "TOWERMD_REGISTRATION_REAL_GIT",
            String::from_utf8(real_git.stdout).unwrap().trim(),
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let mut cold_status = fixture.command(cold.root.path(), &["status"]);
    let cold_worker = thread::spawn(move || cold_status.output().unwrap());
    let entered = fixture.runtime.path().join("archive-entered");
    let release = fixture.runtime.path().join("archive-release");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !entered.exists() {
        assert!(
            Instant::now() < deadline,
            "archive preparation never entered"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let trace_registered = std::env::var_os("TOWERMD_TEST_TRACE_REGISTERED_GIT").is_some();
    if trace_registered {
        fs::write(fixture.runtime.path().join("registered-trace-armed"), "").unwrap();
    }
    let phase_start = Instant::now();
    let mut status = fixture.command(fixture.root.path(), &["status"]);
    let mut renew = fixture.command(&worktree, &["task", "show"]);
    let (status_sender, status_receiver) = mpsc::channel();
    let status_worker = thread::spawn(move || {
        let output = status.output().unwrap();
        eprintln!("registered status elapsed {:?}", phase_start.elapsed());
        status_sender.send(output).unwrap();
    });
    let (renew_sender, renew_receiver) = mpsc::channel();
    let renew_worker = thread::spawn(move || {
        let output = renew.output().unwrap();
        eprintln!("registered bound Show elapsed {:?}", phase_start.elapsed());
        renew_sender.send(output).unwrap();
    });
    let mut active_read = fixture.command(
        &worktree,
        &["baseline", "scan", "--run", run["id"].as_str().unwrap()],
    );
    let (active_read_sender, active_read_receiver) = mpsc::channel();
    let active_read_worker = thread::spawn(move || {
        let output = active_read.output().unwrap();
        eprintln!("registered bound scan elapsed {:?}", phase_start.elapsed());
        active_read_sender.send(output).unwrap()
    });
    // A second cold request must share the first registration and its ProjectSession.
    let mut same_project = fixture.command(cold.root.path(), &["status"]);
    let same_project_worker = thread::spawn(move || same_project.output().unwrap());
    let mut cold_read = fixture.command(cold.root.path(), &["baseline", "scan"]);
    let (read_sender, read_receiver) = mpsc::channel();
    let read_worker = thread::spawn(move || read_sender.send(cold_read.output().unwrap()).unwrap());
    let status_result = status_receiver.recv_timeout(Duration::from_secs(2));
    let renew_result = renew_receiver.recv_timeout(Duration::from_secs(2));
    let active_read_result = active_read_receiver.recv_timeout(Duration::from_secs(2));
    if trace_registered {
        fs::remove_file(fixture.runtime.path().join("registered-trace-armed")).unwrap();
        let trace = fs::read_to_string(fixture.runtime.path().join("registered-git-log")).unwrap();
        eprintln!(
            "registered requests Git calls={} blob_reads={} ref_reads={} original_deadline=2s",
            trace.lines().count(),
            trace
                .lines()
                .filter(|line| line.contains("cat-file blob"))
                .count(),
            trace
                .lines()
                .filter(|line| line.contains("for-each-ref"))
                .count()
        );
    }
    let read_before_release = read_receiver.recv_timeout(Duration::from_secs(2));
    let cold_blocked = !cold_worker.is_finished();
    fs::write(&release, "release").unwrap();
    status_worker.join().unwrap();
    renew_worker.join().unwrap();
    active_read_worker.join().unwrap();
    read_worker.join().unwrap();
    let first = cold_worker.join().unwrap();
    let second = same_project_worker.join().unwrap();
    assert!(first.status.success(), "{first:?}");
    assert!(second.status.success(), "{second:?}");
    assert!(
        matches!(read_before_release, Err(mpsc::RecvTimeoutError::Timeout)),
        "unregistered read opened another session during cold registration: {read_before_release:?}"
    );
    let read = read_receiver.recv().unwrap();
    assert!(read.status.success(), "{read:?}");
    let read: Value = serde_json::from_slice(&read.stdout).unwrap();
    assert!(read.get("warnings").is_none(), "{read}");
    assert!(
        cold_blocked,
        "cold archive escaped the barrier before release"
    );
    let status =
        status_result.expect("registered project status waited for another project's archive");
    let renew = renew_result.expect("bound Run read waited for another project's archive");
    let active_read = active_read_result
        .expect("registered project read renewal waited for another project's archive");
    assert!(status.status.success(), "{status:?}");
    assert!(renew.status.success(), "{renew:?}");
    assert!(active_read.status.success(), "{active_read:?}");
    let shown: Value = serde_json::from_slice(&renew.stdout).unwrap();
    assert_eq!(shown["id"], run["id"]);
    assert_eq!(shown["state"], "running");
    assert_eq!(
        fs::read_to_string(fixture.runtime.path().join("archive-count"))
            .unwrap()
            .lines()
            .count(),
        1,
        "concurrent first calls opened more than one ProjectSession"
    );
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

#[test]
fn failed_first_project_open_releases_registration_and_clone_lock() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    let journal = fixture.root.path().join(".git/towermd/journal.jsonl");
    let original = fs::read(&journal).unwrap();
    fs::write(&journal, "invalid durable journal\n").unwrap();
    let output = fixture
        .command(fixture.root.path(), &["status"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "corrupt hub unexpectedly opened");
    let endpoint: Value =
        serde_json::from_slice(&fs::read(fixture.runtime.path().join("endpoint.json")).unwrap())
            .unwrap();
    fs::write(&journal, original).unwrap();
    fixture.success(fixture.root.path(), &["status"]);
    let shown = fixture.json(&worktree, &["task", "show"]);
    assert_eq!(shown["id"], run["id"]);
    assert_eq!(shown["state"], "running");
    assert_eq!(
        fixture.json(fixture.root.path(), &["daemon", "status"])["instance"],
        endpoint["instance"],
        "retry must open successfully in the same daemon"
    );
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

    for args in [vec!["daemon", "status"], vec!["daemon", "stop"]] {
        let before = journal(&fixture);
        fixture.success(&worktree, &args);
        let after = journal(&fixture);
        assert!(
            after["events"].as_array().unwrap().len() > before["events"].as_array().unwrap().len(),
            "{args:?} 未续租"
        );
        let latest = after["events"].as_array().unwrap().last().unwrap();
        assert_eq!(latest["type"], "lease.renewed");
        assert_eq!(
            latest["lease_delta"]["Renew"]["leases"][0]["holder"],
            run["id"]
        );
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
    let journal = journal(&fixture);
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
    let text = fs::read_to_string(fixture.root.path().join(".git/towermd/journal.jsonl")).unwrap();
    let mut lines = text.lines();
    let header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    let events = lines
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    json!({"snapshot_ack":header["checkpoint"]["ack"],"checkpoint":header["checkpoint"],"events":events,"header":header})
}
fn run_leases(journal: &Value, holder: &Value) -> Vec<Value> {
    let mut leases = std::collections::BTreeMap::new();
    if let Some(values) = journal["checkpoint"]["leases"].as_object() {
        for (name, lease) in values {
            if &lease["holder"] == holder {
                leases.insert(name.clone(), lease.clone());
            }
        }
    }
    for event in journal["events"].as_array().unwrap() {
        for kind in ["Grant", "Renew"] {
            if let Some(values) = event["lease_delta"][kind]["leases"].as_array() {
                for lease in values {
                    if &lease["holder"] == holder {
                        leases.insert(lease["name"].as_str().unwrap().to_string(), lease.clone());
                    }
                }
            }
        }
    }
    leases.into_values().collect()
}

// Fold the immutable journal facts without opening another Hub or taking a lease.
fn current_main_lease(journal: &Value) -> Option<towermd_core::hub::Lease> {
    let mut current = journal["checkpoint"]["leases"].get("main").cloned();
    for event in journal["events"].as_array().unwrap() {
        if event["stale"] == true {
            continue;
        }
        for kind in ["Grant", "Renew"] {
            if let Some(leases) = event["lease_delta"][kind]["leases"].as_array() {
                for lease in leases {
                    if lease["name"] == "main" {
                        current = Some(lease.clone());
                    }
                }
            }
        }
        for kind in ["Release", "Expire"] {
            if let Some(fences) = event["lease_delta"][kind]["fences"].as_array() {
                for fence in fences {
                    if current.as_ref().is_some_and(|lease| {
                        ["name", "holder", "epoch"]
                            .iter()
                            .all(|key| lease[key] == fence[key])
                    }) {
                        current = None;
                    }
                }
            }
        }
    }
    current.map(|lease| serde_json::from_value(lease).unwrap())
}

fn main_lease_renewed(journal: &Value, original: &towermd_core::hub::Lease, now: u64) -> bool {
    current_main_lease(journal).is_some_and(|lease| {
        lease.fence() == original.fence()
            && lease.expires_at > original.expires_at
            && lease.expires_at > now
    })
}

#[test]
fn main_lease_observation_rejects_expiry_release_and_changed_fences() {
    let lease = json!({"name":"main","holder":"snapshot:host","epoch":7,"expires_at":100});
    let original = serde_json::from_value(lease.clone()).unwrap();
    let mut journal = json!({"checkpoint":{"leases":{"main":lease}},"events":[]});
    assert!(!main_lease_renewed(&journal, &original, 50));
    journal["checkpoint"]["leases"]["main"]["expires_at"] = json!(200);
    assert!(main_lease_renewed(&journal, &original, 199));
    assert!(!main_lease_renewed(&journal, &original, 200));
    for key in ["holder", "epoch"] {
        let mut changed = journal.clone();
        changed["checkpoint"]["leases"]["main"][key] = if key == "epoch" {
            json!(8)
        } else {
            json!("other")
        };
        assert!(!main_lease_renewed(&changed, &original, 150));
    }
    for kind in ["Release", "Expire"] {
        let mut removed = journal.clone();
        removed["events"] = json!([{"lease_delta":{kind:{"fences":[{"name":"main","holder":"snapshot:host","epoch":7}]}}}]);
        assert!(!main_lease_renewed(&removed, &original, 150));
        removed["events"][0]["lease_delta"][kind]["fences"][0]["epoch"] = json!(6);
        assert!(main_lease_renewed(&removed, &original, 150));
        removed["events"][0]["lease_delta"][kind]["fences"][0]["epoch"] = json!(7);
        removed["events"][0]["stale"] = json!(true);
        assert!(main_lease_renewed(&removed, &original, 150));
    }
    journal["checkpoint"]["leases"]["main"]["expires_at"] = json!(100);
    journal["events"] = json!([{"lease_delta":{"Renew":{"leases":[{"name":"main","holder":"snapshot:host","epoch":7,"expires_at":200}]}}}]);
    assert!(main_lease_renewed(&journal, &original, 150));
}

fn write_journal(fixture: &Fixture, value: &Value) {
    let mut header = value["header"].clone();
    header["checkpoint"]["ack"] = value["snapshot_ack"].clone();
    let mut text = serde_json::to_string(&header).unwrap();
    text.push('\n');
    for event in value["events"].as_array().unwrap() {
        text.push_str(&serde_json::to_string(event).unwrap());
        text.push('\n');
    }
    fs::write(fixture.root.path().join(".git/towermd/journal.jsonl"), text).unwrap();
}

fn plain_snapshot_source_change(code: Option<i32>, stderr: &[u8]) -> bool {
    code == Some(1)
        && matches!(
            stderr,
            b"towermd: authority changed; reload Catalog\n"
                | b"towermd: authority changed during preparation\n"
                | b"towermd: authority changed during cache probe\n"
        )
}

#[test]
fn snapshot_helper_never_retries_uncertain_or_other_failures() {
    for plain in [
        b"towermd: authority changed; reload Catalog\n".as_slice(),
        b"towermd: authority changed during preparation\n".as_slice(),
        b"towermd: authority changed during cache probe\n".as_slice(),
    ] {
        assert!(plain_snapshot_source_change(Some(1), plain));
        for extra in [b"\n".as_slice(), b"extra line\n".as_slice()] {
            assert!(!plain_snapshot_source_change(
                Some(1),
                &[plain, extra].concat()
            ));
        }
        for code in [None, Some(0), Some(2)] {
            assert!(!plain_snapshot_source_change(code, plain));
        }
        for prefix in [
            "towermd: Git 快照 abc 已写入，但 hub 确认失败；下次从 main/private 补确认：",
            "towermd: uncertain: ",
            "towermd: accepted request requires synchronization: ",
        ] {
            let wrapped = format!(
                "{prefix}{}",
                std::str::from_utf8(plain)
                    .unwrap()
                    .trim_start_matches("towermd: ")
            );
            assert!(!plain_snapshot_source_change(Some(1), wrapped.as_bytes()));
        }
    }
    for stderr in [
        "towermd: Git 快照 abc 已写入，但 hub 确认失败；下次从 main/private 补确认：authority changed; reload Catalog\n",
        "towermd: uncertain: authority changed; reload Catalog\n",
        "towermd: other authority error\n",
        "towermd: SourceChanged: source changed before recovery Git boundary\n",
    ] {
        assert!(!plain_snapshot_source_change(Some(1), stderr.as_bytes()));
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(std::time::Instant::now() < deadline, "等待条件超时");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn snapshot_checked_main_writes_private_without_worktree_writes() {
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
        let report = fixture.snapshot_when_idle(fixture.root.path(), None);
        assert_eq!(report["status"], "written", "linked main: {linked}");
        assert_eq!(
            report["commit"],
            fixture.git(&["rev-parse", "refs/towermd/snapshot"])
        );
        assert!(report["reason"].is_null());
        assert_eq!(journal(&fixture)["snapshot_ack"]["seq"], report["seq"]);
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
        assert_eq!(saved["snapshot_ack"]["commit"], report["commit"]);
        assert!(saved["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["type"] == "snapshot.written"
                && event["data"]["commit"] == report["commit"]));

        let private_commit = report["commit"].clone();
        fixture.git(&["-C", checked_path, "switch", "--detach"]);
        let report = fixture.snapshot_when_idle(fixture.root.path(), None);
        assert_eq!(report["status"], "written");
        assert_eq!(fixture.git(&["rev-parse", "main"]), report["commit"]);
        assert_eq!(
            journal(&fixture)["snapshot_ack"]["commit"],
            report["commit"]
        );
        assert_eq!(
            fixture.git(&["rev-parse", "refs/towermd/snapshot"]),
            private_commit
        );
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
    let report = fixture.snapshot_when_idle(fixture.root.path(), None);
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
fn snapshot_dirty_path_writes_private_and_preserves_user_bytes() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let first = fixture.snapshot_when_idle(fixture.root.path(), None);
    assert_eq!(first["status"], "written");
    fixture.git(&["switch", "main"]);
    let path = fixture.root.path().join("docs/tower/state/state.json");
    fs::write(&path, "user edits\n").unwrap();
    fixture.add("T-2", "B");
    let report = fixture.snapshot_when_idle(fixture.root.path(), None);
    assert_eq!(report["status"], "written");
    assert_eq!(
        report["commit"],
        fixture.git(&["rev-parse", "refs/towermd/snapshot"])
    );
    assert!(report["reason"].is_null());
    assert_eq!(fixture.git(&["rev-parse", "main"]), first["commit"]);
    assert_eq!(journal(&fixture)["snapshot_ack"]["seq"], report["seq"]);
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
    let report = fixture.snapshot_when_idle(fixture.root.path(), run["id"].as_str());
    let saved = journal(&fixture);
    let leases = run_leases(&saved, &run["id"]);
    assert!(!leases.is_empty());
    for lease in leases {
        assert_eq!(lease["epoch"], run["leases"][0]["epoch"]);
        assert_eq!(lease["holder"], run["id"]);
        assert!(lease["expires_at"].as_u64().unwrap() > 0);
    }
    let state: Value =
        serde_json::from_str(&fixture.git(&["show", "main:docs/tower/state/state.json"])).unwrap();
    assert!(!state["omitted"].as_array().unwrap().is_empty());
    assert_eq!(saved["checkpoint"]["floor_seq"], report["seq"]);
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
    let first_wait = std::time::Instant::now();
    eprintln!("periodic phase: first business snapshot ack");
    wait_until(|| journal(&fixture)["snapshot_ack"]["commit"].is_string());
    eprintln!("periodic first ack after {:?}", first_wait.elapsed());
    let first = fixture.git(&["rev-parse", "main"]);
    assert_ne!(first, before);
    thread::sleep(Duration::from_millis(2200));
    assert_eq!(fixture.git(&["rev-parse", "main"]), first);
    fixture.add("T-2", "B");
    let second_wait = std::time::Instant::now();
    eprintln!("periodic phase: second business snapshot main commit");
    wait_until(|| fixture.git(&["rev-parse", "main"]) != first);
    eprintln!("periodic second commit after {:?}", second_wait.elapsed());
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
    let original_header = journal(&fixture)["header"].clone();
    assert_eq!(original_header["checkpoint"]["floor_seq"], 0);
    let report = fixture.snapshot_when_idle(fixture.root.path(), None);
    assert_eq!(report["status"], "written");
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    // Crash-boundary fixture: actual accepted archive prefix before ack, never an ack-less compacted floor.
    let catalog = towermd_core::catalog::Catalog::load(fixture.root.path()).unwrap();
    let source = towermd_core::snapshot_service::read_snapshot(&catalog)
        .unwrap()
        .unwrap();
    assert!(source.snapshot.omitted.is_empty());
    let events: Vec<Value> = source
        .archives
        .values()
        .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
        .collect();
    let mut saved = json!({"header":original_header,"snapshot_ack":null,"events":events});
    saved["header"]["checkpoint"]["ack"] = Value::Null;
    write_journal(&fixture, &saved);
    fixture.success(fixture.root.path(), &["status"]);
    wait_until(|| journal(&fixture)["snapshot_ack"]["commit"] == report["commit"]);
    assert_eq!(fixture.git(&["rev-parse", "main"]), report["commit"]);
}

#[test]
fn missing_hub_with_main_snapshot_refuses_new_task() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let report = fixture.snapshot_when_idle(fixture.root.path(), None);
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
    use towermd_core::project_session::ProjectSession as Hub;
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
        let inputs = towermd_core::recovery_outbox::RecoveryInputs::read(
            &project
                .runtime
                .path()
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            &catalog,
        )
        .unwrap();
        let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open(
            &project
                .runtime
                .path()
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        let hub = Mutex::new(
            Hub::open(&catalog.common_dir.join("towermd"), &catalog, &inputs, 0).unwrap(),
        );
        let earlier = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 25_000;
        tasks::execute(
            &hub,
            &catalog,
            &outbox.read_inputs().unwrap(),
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
            &mut outbox,
        )
        .unwrap();
        let report = SnapshotWriter::default()
            .write(
                &hub,
                &catalog,
                "snapshot-test-host",
                true,
                || Ok(earlier),
                &mut outbox,
            )
            .unwrap();
        assert_eq!(report.status, "written");
    }
    let originals = [&fixture, &other].map(|project| {
        let lease = current_main_lease(&journal(project)).unwrap();
        assert_eq!(lease.holder, "snapshot:snapshot-test-host");
        lease
    });
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
    struct DrainPushBeforeTempRemoval<'a> {
        fixture: &'a Fixture,
        remote: &'a Path,
    }
    impl Drop for DrainPushBeforeTempRemoval<'_> {
        fn drop(&mut self) {
            let _ = fs::write(self.remote.join("snapshot-release"), "");
            let _ = self
                .fixture
                .command(self.fixture.root.path(), &["daemon", "stop"])
                .output();
        }
    }
    let _drain = DrainPushBeforeTempRemoval {
        fixture: &fixture,
        remote: remote.path(),
    };
    fixture.success(other.root.path(), &["status"]);
    fixture.add("T-2", "B");
    eprintln!("BLOCKED_PERIODIC_PHASE waiting actual remote hook");
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
    assert_eq!(response["reason"], "快照写入正在进行", "{response}");
    eprintln!("real blocked periodic writer returns: {response}");
    // A periodic heartbeat snapshot can already have compacted lease.renewed
    // into the header. Require the same live fence and an actual expiry advance
    // from the seeded journal, using checkpoint plus the entire system tail.
    wait_until(|| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        [&fixture, &other]
            .iter()
            .zip(&originals)
            .all(|(project, original)| main_lease_renewed(&journal(project), original, now))
    });
    for (project, original) in [&fixture, &other].iter().zip(&originals) {
        let value = journal(project);
        eprintln!(
            "BLOCKED_PERIODIC_LIVE_LEASE original={original:?} current={:?} checkpoint={} tail={}",
            current_main_lease(&value),
            value["checkpoint"]["leases"],
            value["events"]
        );
    }
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
    let snapshot = fixture.snapshot_when_idle(fixture.root.path(), None);
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
    fs::write(hub_dir.join("journal.jsonl"), "broken journal").unwrap();
    let output = fixture.success(fixture.root.path(), &["hub", "rebuild"]);
    let corrupted: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(corrupted.get("warnings").is_none());
    for key in [
        "status",
        "project_id",
        "main_revision",
        "source_commit",
        "source_seq",
        "seq_boundary",
        "epoch_boundary",
        "source_epoch_high_water",
        "source_reference",
        "inputs_digest",
        "main",
    ] {
        assert_eq!(corrupted[key], request[key], "stable recovery field {key}");
    }
    assert_ne!(corrupted["nonce"], request["nonce"]);
    assert_ne!(corrupted["request_id"], request["request_id"]);
    assert!(
        corrupted["expires_at"].as_u64().unwrap() > corrupted["requested_at"].as_u64().unwrap()
    );
    assert_eq!(
        fs::read_to_string(hub_dir.join("journal.jsonl")).unwrap(),
        "broken journal"
    );
    assert_eq!(fixture.git(&["rev-parse", "main"]), snapshot["commit"]);
    let response = recovery_call(
        &fixture,
        &corrupted,
        Some("app"),
        r#"{"actor":"human:fixture"}"#,
    );
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let error: Value = response.json().unwrap();
    assert!(error["error"].as_str().unwrap().contains("journal"));
    let status = fixture
        .command(fixture.root.path(), &["status"])
        .output()
        .unwrap();
    assert!(!status.status.success());
    assert!(String::from_utf8_lossy(&status.stderr).contains("journal"));
    fixture.success(
        fixture.root.path(),
        &["hub", "rebuild", "--run", "missing-run"],
    );
    assert_eq!(
        fs::read_to_string(hub_dir.join("journal.jsonl")).unwrap(),
        "broken journal"
    );
}

#[test]
fn rebuild_request_never_renews_even_with_explicit_run() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let snapshot = fixture.snapshot_when_idle(fixture.root.path(), None);
    for restart in [false, true] {
        if restart {
            fixture.success(fixture.root.path(), &["daemon", "stop"]);
        }
        let before = fs::read(fixture.root.path().join(".git/towermd/journal.jsonl")).unwrap();
        let previous = run_leases(&journal(&fixture), &run["id"]);
        for args in [
            vec!["hub", "rebuild"],
            vec!["hub", "rebuild", "--run", run["id"].as_str().unwrap()],
            vec!["hub", "rebuild", "--run", "missing-run"],
        ] {
            let request = fixture.json(&worktree, &args);
            assert_eq!(request["status"], "awaiting_app_confirmation");
            assert_eq!(
                fs::read(fixture.root.path().join(".git/towermd/journal.jsonl")).unwrap(),
                before
            );
            assert_eq!(run_leases(&journal(&fixture), &run["id"]), previous);
            let saved: Value = serde_json::from_slice(
                &fs::read(
                    fixture
                        .runtime
                        .path()
                        .join("rebuild-requests")
                        .join(format!("{}.json", request["request_id"].as_str().unwrap())),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(saved["request"]["expires_at"], request["expires_at"]);
            assert!(saved.get("approved").is_none());
        }
        assert_eq!(fixture.git(&["rev-parse", "main"]), snapshot["commit"]);
    }
}

#[test]
fn hub_rebuild_explicit_run_with_missing_journal_does_not_create_empty_hub() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "--detach"]);
    fixture.add("T-1", "A");
    let report = fixture.snapshot_when_idle(fixture.root.path(), None);
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
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!hub_dir.exists(), "恢复请求不能为了校验 Run 创建空 hub");
}

#[test]
fn rebuild_does_not_expose_cli_approval_commands() {
    let fixture = Fixture::new();
    let help = fixture.success(fixture.root.path(), &["hub", "rebuild", "--help"]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("App"));
    for args in [
        vec!["hub", "rebuild", "--approve"],
        vec!["hub", "rebuild", "--yes"],
        vec!["hub", "rebuild", "--confirm"],
        vec!["hub", "rebuild", "--actor", "human:fixture"],
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
        .any(|event| event["type"] == "lease.renewed"
            && event["lease_delta"]["Renew"]["leases"][0]["holder"] == run["id"]));
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
        .join(".git/towermd/journal.jsonl")
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
    let unblocked = fixture.runtime.path().join("fetch-unblocked");
    fs::write(&upload, format!(
        "#!/bin/sh\nprintf 'fetch\\n' >> '{}'\ni=0\nwhile [ ! -f '{}' ] && [ \"$i\" -lt 300 ]; do sleep 0.01; i=$((i+1)); done\nprintf 'unblocked\\n' > '{}'\nexec git-upload-pack \"$@\"\n",
        counter.display(), release.display(), unblocked.display())).unwrap();
    fs::set_permissions(&upload, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.git(&[
        "config",
        "remote.origin.uploadpack",
        upload.to_str().unwrap(),
    ]);
    struct ReleaseFetchOnDrop<'a>(&'a Path);
    impl Drop for ReleaseFetchOnDrop<'_> {
        fn drop(&mut self) {
            let _ = fs::write(self.0, "release");
        }
    }
    let _release_before_fixture_stop = ReleaseFetchOnDrop(&release);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(10));
    let started = std::time::Instant::now();
    let workers: Vec<_> = (0..10)
        .map(|_| {
            let barrier = barrier.clone();
            let mut command = fixture.command(fixture.root.path(), &["status"]);
            thread::spawn(move || {
                barrier.wait();
                let began = std::time::Instant::now();
                let output = command.output().unwrap();
                (output, began.elapsed())
            })
        })
        .collect();
    let outputs: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let responses_elapsed = started.elapsed();
    let worker_elapsed = outputs
        .iter()
        .map(|(_, elapsed)| *elapsed)
        .collect::<Vec<_>>();
    for (output, _) in outputs {
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["main"]["reference"], "refs/remotes/origin/main");
    }
    let elapsed = started.elapsed();
    eprintln!("TEN_STATUS_ACTUAL total={elapsed:?} responses={responses_elapsed:?} worker_elapsed={worker_elapsed:?} upload_entered={} upload_unblocked={} release_exists={}", counter.exists(),unblocked.exists(),release.exists());
    assert!(
        elapsed < Duration::from_secs(2),
        "status 等待了 fetch: total={elapsed:?}, upload_entered={}, upload_unblocked={}, release_exists={}",counter.exists(),unblocked.exists(),release.exists()
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !counter.exists() {
        assert!(std::time::Instant::now() < deadline, "后台 fetch 未启动");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
    assert!(
        !unblocked.exists(),
        "all ten responses must precede actual upload-pack unblock"
    );
    assert!(!release.exists());
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

#[test]
fn remote_snapshot_remains_readable_and_renewable_immediately_offline() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let remote = fixture.runtime.path().join("snapshot-origin.git");
    fixture.git(&["clone", "--bare", ".", remote.to_str().unwrap()]);
    fixture.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let fetched_at = loop {
        let status = fixture.json(&worktree, &["status"]);
        if let Some(at) = status["main"]["fetched_at"].as_u64() {
            break at;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "background fetch did not succeed: {status}"
        );
        thread::sleep(Duration::from_millis(10));
    };
    let report = fixture.snapshot_when_idle(&worktree, None);
    assert_eq!(report["status"], "written");
    fixture.git(&["remote", "set-url", "origin", "/nonexistent/after-snapshot"]);
    let before = journal(&fixture)["events"].as_array().unwrap().len();
    let status = fixture.json(&worktree, &["status"]);
    assert_eq!(status["main"]["fetched_at"].as_u64(), Some(fetched_at));
    let shown = fixture.json(&worktree, &["task", "show"]);
    assert_eq!(shown["id"], run["id"]);
    assert_eq!(shown["state"], "running");
    assert_eq!(
        fixture.git(&["rev-parse", "refs/remotes/origin/main"]),
        report["commit"].as_str().unwrap()
    );
    assert!(journal(&fixture)["events"].as_array().unwrap()[before..]
        .iter()
        .any(|event| event["type"] == "lease.renewed"
            && event["lease_delta"]["Renew"]["leases"][0]["holder"] == run["id"]));
}

#[cfg(unix)]
#[test]
fn read_loaded_before_snapshot_reloads_cache_once_and_still_rejects_real_rollback() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let reader = fixture.worktree("source-reader");
    let wrapper = fixture.runtime.path().join("git-wrapper");
    fs::create_dir(&wrapper).unwrap();
    let original = Command::new("which").arg("git").output().unwrap();
    assert!(original.status.success());
    let program = wrapper.join("git");
    fs::write(&program, r#"#!/bin/sh
scan=0
config=0
revision=0
for argument in "$@"; do
    if test "$argument" = ls-tree; then scan=1; fi
    if test "$argument" = .towermd/config.yaml; then config=1; fi
done
if test "$scan$config" = 11 && test -f "$TOWERMD_READ_GATES/old-revision"; then
    old_revision=$(cat "$TOWERMD_READ_GATES/old-revision")
    for argument in "$@"; do
        if test "$argument" = "$old_revision"; then revision=1; fi
    done
fi
if test "$scan$config$revision" = 111 && test "$(pwd -P)" = "$TOWERMD_READ_ROOT" && mkdir "$TOWERMD_READ_GATES/read-once" 2>/dev/null; then
    touch "$TOWERMD_READ_GATES/read-entered"
    attempts=0
    while ! test -f "$TOWERMD_READ_GATES/read-release"; do
        attempts=$((attempts + 1))
        if test "$attempts" -ge 1000; then
            touch "$TOWERMD_READ_GATES/read-timed-out"
            exit 98
        fi
        sleep 0.01
    done
fi
exec "$TOWERMD_READ_REAL_GIT" "$@"
"#).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let output = fixture
        .command(fixture.root.path(), &["status"])
        .env(
            "PATH",
            format!("{}:{}", wrapper.display(), std::env::var("PATH").unwrap()),
        )
        .env("TOWERMD_READ_GATES", fixture.runtime.path())
        .env("TOWERMD_READ_ROOT", fs::canonicalize(&reader).unwrap())
        .env(
            "TOWERMD_READ_REAL_GIT",
            String::from_utf8(original.stdout).unwrap().trim(),
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let remote = fixture.runtime.path().join("read-race.git");
    fixture.git(&["clone", "--bare", ".", remote.to_str().unwrap()]);
    fixture.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let status = fixture.json(&worktree, &["status"]);
        if status["main"]["fetched_at"].is_u64() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "initial fetch did not finish"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let old = fixture.git(&["rev-parse", "origin/main"]);
    fs::write(fixture.runtime.path().join("old-revision"), &old).unwrap();
    let phases = std::time::Instant::now();
    let mut reading = fixture.command(&reader, &["status"]);
    let reading = thread::spawn(move || reading.output().unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !fixture.runtime.path().join("read-entered").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "read did not load the old revision"
        );
        thread::sleep(Duration::from_millis(10));
    }
    eprintln!(
        "read-race old Catalog barrier entered after {:?}",
        phases.elapsed()
    );
    let report = fixture.snapshot_when_idle(&worktree, None);
    eprintln!("read-race snapshot persisted after {:?}", phases.elapsed());
    assert_eq!(report["status"], "written");
    fixture.git(&[
        "remote",
        "set-url",
        "origin",
        "/nonexistent/read-race-offline",
    ]);
    fs::write(fixture.runtime.path().join("read-release"), "").unwrap();
    let output = reading.join().unwrap();
    eprintln!("read-race reader completed after {:?}", phases.elapsed());
    assert!(
        !fixture.runtime.path().join("read-timed-out").exists(),
        "fixture barrier timed out after {:?}",
        phases.elapsed()
    );
    assert!(
        !output.status.success(),
        "expired Catalog must end this request"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("SourceChanged"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status = fixture.json(&worktree, &["status"]);
    // The next request reloads actual Catalog and still only exposes a verified prefix.

    assert!(status["runs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value["id"] == run["id"] && value["state"] == "running"));
    fixture.git(&["update-ref", "refs/remotes/origin/main", &old]);
    let pending = fixture.json(&worktree, &["status"]);
    assert_eq!(pending["pending_verification"], true);
    let rejected = fixture
        .command(&worktree, &["task", "show"])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(1));
    let error = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        error.contains("persistent snapshot") || error.contains("ack commit is not reachable"),
        "{error}"
    );
}
#[test]
fn command_error_exposes_structured_lease_owners_and_keeps_cli_message() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    fixture.add("T-2", "A");
    let first = fixture.worktree("T-1");
    let second = fixture.worktree("T-2");
    let run = fixture.json(&first, &["task", "claim", "T-1"]);
    let endpoint = fixture.json(fixture.root.path(), &["daemon", "status"]);
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{}/command", endpoint["port"]))
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .json(&json!({"directory":second,"run":null,"command":{"action":"claim","id":"T-2"}}))
        .send()
        .unwrap();
    assert_eq!(response.status(), 400);
    let body: Value = response.json().unwrap();
    assert!(body["error"].as_str().unwrap().contains("occupied"));
    let owners = body["owners"].as_array().unwrap();
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0]["name"], "module:A");
    assert_eq!(owners[0]["holder"], run["id"]);
    assert_eq!(owners[0]["epoch"], run["leases"][0]["epoch"]);
    assert!(owners[0]["expires_at"].as_u64().unwrap() > 0);
    let output = fixture
        .command(&second, &["task", "claim", "T-2"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("occupied"));
}
#[test]
fn websocket_observes_distinct_l5_lease_claim_and_release_events() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let status = fixture.json(fixture.root.path(), &["status"]);
    let mut socket = event_socket(&fixture, status["seq"].as_u64().unwrap());
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    let granted = receive_event(&mut socket);
    let claimed = receive_event(&mut socket);
    assert_eq!(granted["type"], "lease.granted");
    assert_eq!(granted["actor"], "process:towermd-hub");
    assert_eq!(granted["layer"], "L5");
    assert_eq!(claimed["type"], "task.claimed");
    assert_eq!(claimed["layer"], "L5");
    assert_eq!(claimed["guards"], run["leases"]);
    assert_eq!(claimed["data"]["run"]["leases"], run["leases"]);
    assert_eq!(claimed["epoch"], run["leases"][0]["epoch"]);
    fixture.success(&worktree, &["task", "release"]);
    let mut released = false;
    for _ in 0..3 {
        let event = receive_event(&mut socket);
        assert_eq!(event["layer"], "L5");
        if event["type"] == "lease.released" {
            released = true;
            assert_eq!(event["actor"], "process:towermd-hub");
        }
        if event["type"] == "task.released" {
            assert!(released);
            assert_eq!(event["actor"], "process:towermd-cli");
            return;
        }
    }
    panic!("missing task.released");
}

#[cfg(unix)]
#[test]
fn snapshot_cli_recovers_one_remote_race_and_cached_reads_survive_offline() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let remote = fixture.runtime.path().join("retry-remote.git");
    fixture.git(&["clone", "--bare", ".", remote.to_str().unwrap()]);
    fixture.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    towermd_core::origin::refresh_origin_main(fixture.root.path()).unwrap();
    fixture.add("T-1", "A");
    let run_tree = fixture.worktree("T-1");
    let run = fixture.json(&run_tree, &["task", "claim", "T-1"]);
    // Finish the daemon's initial fetch and archive pending business before arming
    // the race; neither background job may take the upload-pack barrier.
    wait_until(|| {
        let status = fixture.json(&run_tree, &["status"]);
        assert!(status["main"]["last_fetch_error"].is_null(), "{status}");
        status["main"]["fetched_at"].as_u64().is_some()
    });
    let prepared = fixture.snapshot_when_idle(&run_tree, run["id"].as_str());
    let initial = fixture.git(&["rev-parse", "origin/main"]);
    assert_eq!(prepared["commit"], initial);
    assert_eq!(journal(&fixture)["snapshot_ack"]["commit"], initial);
    fixture.git(&["merge", "--ff-only", "origin/main"]);
    let marker = fixture.runtime.path().join("snapshot-fetch-complete");
    let release = fixture.runtime.path().join("snapshot-fetch-release");
    let counter = fixture.runtime.path().join("snapshot-fetch-count");
    let once = fixture.runtime.path().join("snapshot-fetch-once");
    let upload = fixture.runtime.path().join("snapshot-upload-pack");
    fs::write(&upload, format!(
        "#!/bin/sh\nprintf 'fetch\\n' >> '{}'\ngit-upload-pack \"$@\"\nresult=$?\nif mkdir '{}' 2>/dev/null; then\n  touch '{}'\n  i=0\n  while [ ! -f '{}' ] && [ \"$i\" -lt 1000 ]; do sleep 0.01; i=$((i+1)); done\nfi\nexit \"$result\"\n",
        counter.display(), once.display(), marker.display(), release.display())).unwrap();
    fs::set_permissions(&upload, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.git(&[
        "config",
        "remote.origin.uploadpack",
        upload.to_str().unwrap(),
    ]);
    let mut command = fixture.command(&run_tree, &["snapshot"]);
    let worker = thread::spawn(move || command.output().unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        if worker.is_finished() {
            let output = worker.join().unwrap();
            panic!(
                "snapshot exited before fetch barrier: status={} stdout={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(
            std::time::Instant::now() < deadline,
            "snapshot fetch barrier was not entered"
        );
        thread::sleep(Duration::from_millis(50));
    }
    // The old authority has already been advertised and packed to the real fetch;
    // the wrapper only delays its exit, before the snapshot's push advertisement.
    assert_eq!(fixture.git(&["rev-parse", "origin/main"]), initial);
    fs::create_dir_all(fixture.root.path().join("src/a")).unwrap();
    fs::write(fixture.root.path().join("src/a/winner.rs"), "winner code\n").unwrap();
    fixture.git(&["add", "src/a/winner.rs"]);
    fixture.git(&["commit", "-qm", "remote competitor code"]);
    let winner = fixture.git(&["rev-parse", "main"]);
    fixture.git(&["push", "origin", "main"]);
    let index = fs::read(fixture.root.path().join(".git/index")).unwrap();
    fs::write(&release, "release").unwrap();
    let output = worker.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "written");
    let commit = report["commit"].as_str().unwrap();
    assert_eq!(fixture.git(&["rev-parse", &format!("{commit}^1")]), winner);
    assert_eq!(
        fixture.git(&["show", &format!("{commit}:src/a/winner.rs")]),
        "winner code"
    );
    assert_eq!(fixture.git(&["rev-parse", "main"]), winner);
    assert_eq!(fixture.git(&["rev-parse", "origin/main"]), commit);
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 2);
    assert_eq!(journal(&fixture)["snapshot_ack"]["commit"], commit);
    fs::rename(&remote, fixture.runtime.path().join("offline-remote.git")).unwrap();
    fixture.success(&run_tree, &["status"]);
    let shown = fixture.json(&run_tree, &["task", "show"]);
    assert_eq!(shown["id"], run["id"]);
    let leases = run_leases(&journal(&fixture), &run["id"]);
    assert!(!leases.is_empty());
}

fn recovery_endpoint(f: &Fixture) -> Value {
    serde_json::from_slice(&fs::read(f.runtime.path().join("endpoint.json")).unwrap()).unwrap()
}

fn recovery_call(
    f: &Fixture,
    request: &Value,
    client: Option<&str>,
    body: &str,
) -> reqwest::blocking::Response {
    let endpoint = recovery_endpoint(f);
    let call = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!(
            "http://127.0.0.1:{}/hub/rebuild/{}/confirm",
            endpoint["port"],
            request["request_id"].as_str().unwrap()
        ))
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .header("content-type", "application/json")
        .body(body.to_owned());
    let call = match client {
        Some(kind) => call.header("x-towermd-client", kind),
        None => call,
    };
    call.send().unwrap()
}

#[test]
fn app_confirmation_restores_private_snapshot_after_real_hub_loss() {
    let f = Fixture::new();
    f.add("T-1", "A");
    let saved = f.snapshot_when_idle(f.root.path(), None);
    assert_eq!(
        f.git(&["rev-parse", "refs/towermd/snapshot"]),
        saved["commit"]
    );
    f.success(f.root.path(), &["daemon", "stop"]);
    let hub_dir = f.root.path().join(".git/towermd");
    fs::remove_dir_all(&hub_dir).unwrap();
    let status = f.json(f.root.path(), &["status"]);
    assert_eq!(status["status"], "recovery_required");
    assert_eq!(status["source_commit"], saved["commit"]);
    assert!(!hub_dir.exists(), "只读 status 不创建 hub");
    let request = f.json(f.root.path(), &["hub", "rebuild"]);
    assert!(!hub_dir.exists(), "CLI 请求不创建 hub");
    for kind in [None, Some("cli"), Some("agent"), Some("unknown")] {
        assert_eq!(
            recovery_call(&f, &request, kind, r#"{"actor":"human:fixture"}"#).status(),
            reqwest::StatusCode::FORBIDDEN
        );
        assert!(!hub_dir.exists());
    }
    let response = recovery_call(&f, &request, Some("app"), r#"{"actor":"human:fixture"}"#);
    let code = response.status();
    let rebuilt: Value = response.json().unwrap();
    assert_eq!(code, reqwest::StatusCode::OK, "{rebuilt}");
    assert_eq!(rebuilt["status"], "rebuilt");
    assert!(rebuilt["seq"].as_u64().unwrap() > request["seq_boundary"].as_u64().unwrap());
    let status = f.json(f.root.path(), &["status"]);
    assert!(status["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == "T-1"));
}

fn lost_hub_fixture(empty: bool) -> (Fixture, Value) {
    let f = Fixture::new();
    f.add("T-1", "A");
    let snapshot = f.snapshot_when_idle(f.root.path(), None);
    f.success(f.root.path(), &["daemon", "stop"]);
    let dir = f.root.path().join(".git/towermd");
    fs::remove_dir_all(&dir).unwrap();
    if empty {
        drop(towermd_core::hub::Hub::open(&dir).unwrap());
    }
    let request = f.json(f.root.path(), &["hub", "rebuild"]);
    assert_eq!(request["source_commit"], snapshot["commit"]);
    (f, request)
}

#[test]
fn recovery_status_is_read_only_and_task_writes_are_rejected() {
    for empty in [false, true] {
        let (f, request) = lost_hub_fixture(empty);
        let path = f.root.path().join(".git/towermd/journal.jsonl");
        let before = fs::read(&path).ok();
        for _ in 0..3 {
            let status = f.json(f.root.path(), &["status"]);
            assert_eq!(status["status"], "recovery_required");
            assert!(status["message"].as_str().unwrap().contains("需恢复"));
            assert_eq!(status["source_commit"], request["source_commit"]);
            assert_eq!(fs::read(&path).ok(), before);
        }
        for args in [
            vec![
                "task",
                "add",
                "--id",
                "T-new",
                "--title",
                "new",
                "--module",
                "A",
                "--acceptance",
                "AC-A",
                "--allow",
                "src/a/**",
            ],
            vec!["task", "claim", "T-1"],
            vec!["snapshot"],
        ] {
            let output = f.command(f.root.path(), &args).output().unwrap();
            assert!(!output.status.success(), "{args:?}");
            if empty {
                assert_eq!(fs::read(&path).ok(), before);
            }
        }
    }
}

#[test]
fn confirm_rejects_process_actor_expiry_and_unknown_fields() {
    let (f, request) = lost_hub_fixture(false);
    let before_main = f.git(&["rev-parse", "main"]);
    let before_private = f.git(&["rev-parse", "refs/towermd/snapshot"]);
    for body in [
        r#"{"actor":"process:towermd-cli"}"#,
        r#"{"actor":"human:"}"#,
        r#"{"actor":"human:two words"}"#,
        r#"{"actor":"human:fixture","approve":true}"#,
        r#"{"actor":"human:fixture","expiry":9999999999999}"#,
        r#"{"actor":"human:fixture","directory":"/tmp"}"#,
        r#"{"actor":"human:fixture","source_commit":"fake"}"#,
        "broken json",
    ] {
        assert_eq!(
            recovery_call(&f, &request, Some("app"), body).status(),
            reqwest::StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
    }
    let saved_path = f
        .runtime
        .path()
        .join("rebuild-requests")
        .join(format!("{}.json", request["request_id"].as_str().unwrap()));
    let mut saved: Value = serde_json::from_slice(&fs::read(&saved_path).unwrap()).unwrap();
    saved["expires_at"] = json!(1);
    saved["request"]["expires_at"] = json!(1);
    fs::write(&saved_path, serde_json::to_vec(&saved).unwrap()).unwrap();
    assert_eq!(
        recovery_call(&f, &request, Some("app"), r#"{"actor":"human:fixture"}"#).status(),
        reqwest::StatusCode::CONFLICT
    );
    assert!(!f.root.path().join(".git/towermd").exists());
    assert_eq!(f.git(&["rev-parse", "main"]), before_main);
    assert_eq!(
        f.git(&["rev-parse", "refs/towermd/snapshot"]),
        before_private
    );
}

#[test]
fn confirm_requires_saved_request_and_current_instance() {
    let (f, request) = lost_hub_fixture(false);
    let endpoint = recovery_endpoint(&f);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let url = format!(
        "http://127.0.0.1:{}/hub/rebuild/{}",
        endpoint["port"],
        request["request_id"].as_str().unwrap()
    );
    let path = f
        .runtime
        .path()
        .join("rebuild-requests")
        .join(format!("{}.json", request["request_id"].as_str().unwrap()));
    let original = fs::read(&path).unwrap();
    for _ in 0..3 {
        let saved: Value = client
            .get(&url)
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(saved["request"], {
            let mut v = request.clone();
            v.as_object_mut().unwrap().remove("main");
            v
        });
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!f.root.path().join(".git/towermd").exists());
    }
    for kind in [
        None,
        Some("cli"),
        Some("agent"),
        Some("unknown"),
        Some("app"),
    ] {
        let call = client
            .post(format!("{url}/confirm"))
            .header("x-towermd-instance", "old")
            .body("invalid json");
        let call = match kind {
            Some(kind) => call.header("x-towermd-client", kind),
            None => call,
        };
        assert_eq!(call.send().unwrap().status(), reqwest::StatusCode::CONFLICT);
    }
    for kind in [None, Some("cli"), Some("agent"), Some("unknown")] {
        assert_eq!(
            recovery_call(&f, &request, kind, "invalid json").status(),
            reqwest::StatusCode::FORBIDDEN
        );
    }
    fs::remove_file(&path).unwrap();
    assert_eq!(
        recovery_call(&f, &request, Some("app"), r#"{"actor":"human:fixture"}"#).status(),
        reqwest::StatusCode::NOT_FOUND
    );
    let mut unknown = request.clone();
    unknown["request_id"] = json!("0".repeat(64));
    assert_eq!(
        recovery_call(&f, &unknown, Some("app"), r#"{"actor":"human:fixture"}"#).status(),
        reqwest::StatusCode::NOT_FOUND
    );
    assert!(!f.root.path().join(".git/towermd").exists());
}

#[test]
fn confirm_rejects_changed_main_private_source_or_outbox() {
    use towermd_core::recovery_outbox::{RecoveryContext, RecoveryOutbox};
    for change in ["main", "private", "outbox"] {
        let (f, request) = lost_hub_fixture(false);
        match change {
            "main" => {
                f.git(&["commit", "--allow-empty", "-qm", "source changed"]);
            }
            "private" => {
                let source = request["source_commit"].as_str().unwrap();
                let tree = f.git(&["rev-parse", &format!("{source}^{{tree}}")]);
                let next = f.git(&[
                    "commit-tree",
                    &tree,
                    "-p",
                    source,
                    "-m",
                    "private source changed",
                ]);
                f.git(&["update-ref", "refs/towermd/snapshot", &next]);
            }
            _ => {
                let catalog = towermd_core::catalog::Catalog::load(f.root.path()).unwrap();
                let host: String =
                    serde_json::from_slice(&fs::read(f.runtime.path().join("host.json")).unwrap())
                        .unwrap();
                let mut outbox = RecoveryOutbox::open(
                    &f.runtime
                        .path()
                        .join("recovery-outbox")
                        .join(catalog.config.project_id()),
                    RecoveryContext::from_catalog(&catalog).unwrap(),
                )
                .unwrap();
                outbox
                    .append_pending(&towermd_core::hub_protocol::NewEvent {
                        eid: uuid::Uuid::now_v7().to_string(),
                        host,
                        actor: "process:fixture".into(),
                        kind: "future.audit".into(),
                        layer: "L5".into(),
                        obj: "fixture".into(),
                        data: json!({}),
                        guards: vec![],
                    })
                    .unwrap();
            }
        }
        let response = recovery_call(&f, &request, Some("app"), r#"{"actor":"human:fixture"}"#);
        assert_eq!(
            response.status(),
            reqwest::StatusCode::CONFLICT,
            "{change}: {}",
            response.text().unwrap()
        );
        // A rejected source must not create a target journal.
        assert!(!f.root.path().join(".git/towermd/journal.jsonl").exists());
        let fresh = f.json(f.root.path(), &["hub", "rebuild"]);
        assert_ne!(fresh["request_id"], request["request_id"]);
    }
}

#[test]
fn hub_rebuild_confirm_preserves_incomplete_journal() {
    let (fixture, request) = lost_hub_fixture(true);
    let path = fixture.root.path().join(".git/towermd/journal.jsonl");
    let mut original = fs::read(&path).unwrap();
    original.extend(br#"{"partial":"#);
    fs::write(&path, &original).unwrap();
    let before_main = fixture.git(&["rev-parse", "main"]);
    let before_private = fixture.git(&["rev-parse", "refs/towermd/snapshot"]);
    let status = fixture
        .command(fixture.root.path(), &["status"])
        .output()
        .unwrap();
    assert!(!status.status.success());
    assert_eq!(fs::read(&path).unwrap(), original);
    let response = recovery_call(
        &fixture,
        &request,
        Some("app"),
        r#"{"actor":"human:fixture"}"#,
    );
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fixture.git(&["rev-parse", "main"]), before_main);
    assert_eq!(
        fixture.git(&["rev-parse", "refs/towermd/snapshot"]),
        before_private
    );
}

#[test]
fn snapshot_status_reports_verified_last_and_deferral() {
    let fixture = Fixture::new();
    let empty = fixture.json(fixture.root.path(), &["status"]);
    assert!(empty["snapshot"]["last"].is_null());
    assert!(empty["snapshot"]["deferred_reason"].is_null());
    assert!(empty["snapshot"]["deferred_since"].is_null());
    fixture.add("T-status", "A");
    let first = fixture.snapshot_when_idle(fixture.root.path(), None);
    let status = fixture.json(fixture.root.path(), &["status"]);
    let last = status["snapshot"]["last"].clone();
    for field in ["commit", "seq", "epoch"] {
        assert_eq!(last[field], first[field]);
    }
    assert!(last["written_at"].as_u64().is_some());
    assert_eq!(
        fixture.json(fixture.root.path(), &["status"])["snapshot"]["last"],
        last,
        "query time must not replace the original matching Event time"
    );
    let lock = fixture.root.path().join(".git/refs/towermd/snapshot.lock");
    fs::write(&lock, "actual Git failure").unwrap();
    let failure = fixture
        .command(fixture.root.path(), &["snapshot"])
        .output()
        .unwrap();
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("snapshot.lock"));
    let failed = fixture.json(fixture.root.path(), &["status"]);
    assert!(failed["snapshot"]["deferred_reason"]
        .as_str()
        .unwrap()
        .contains("snapshot.lock"));
    let since = failed["snapshot"]["deferred_since"].as_u64().unwrap();
    let again = fixture
        .command(fixture.root.path(), &["snapshot"])
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert_eq!(
        fixture.json(fixture.root.path(), &["status"])["snapshot"]["deferred_since"],
        since
    );
    fs::remove_file(lock).unwrap();
    let success = fixture.snapshot_when_idle(fixture.root.path(), None);
    let cleared = fixture.json(fixture.root.path(), &["status"]);
    assert_eq!(cleared["snapshot"]["last"]["commit"], success["commit"]);
    assert!(cleared["snapshot"]["deferred_reason"].is_null());
    assert!(cleared["snapshot"]["deferred_since"].is_null());
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    let catalog = towermd_core::catalog::Catalog::load(fixture.root.path()).unwrap();
    let inputs = towermd_core::recovery_outbox::RecoveryInputs::read(
        &fixture
            .runtime
            .path()
            .join("recovery-outbox")
            .join(catalog.config.project_id()),
        &catalog,
    )
    .unwrap();
    let verified = towermd_core::project_session::ProjectSession::open(
        &catalog.common_dir.join("towermd"),
        &catalog,
        &inputs,
        0,
    )
    .unwrap();
    let original = verified.snapshot_summary().unwrap().last.unwrap();
    assert_eq!(
        cleared["snapshot"]["last"]["written_at"].as_u64(),
        original.written_at
    );
    drop(verified);
    fs::remove_dir_all(catalog.common_dir.join("towermd")).unwrap();
    let recovery = fixture.json(fixture.root.path(), &["status"]);
    assert_eq!(recovery["status"], "recovery_required");
    assert_eq!(recovery["snapshot"]["last"]["commit"], success["commit"]);
    assert!(
        recovery["snapshot"]["last"]["written_at"].is_null(),
        "source has no matching current written Event after real Hub loss"
    );
    for _ in 0..3 {
        assert_eq!(
            fixture.json(fixture.root.path(), &["status"])["snapshot"],
            recovery["snapshot"]
        );
        assert!(!catalog.common_dir.join("towermd").exists());
    }
    fixture.git(&["update-ref", "-d", "refs/heads/main"]);
    let unavailable = fixture.json(fixture.root.path(), &["status"]);
    assert_eq!(unavailable["status"], "authority_unavailable");
    assert_eq!(
        unavailable["snapshot"],
        json!({"last":null,"deferred_reason":null,"deferred_since":null})
    );
}

// Actual CLI/HTTP transport coverage. The existing repository fixture is an
// explicitly imported authority, not P7 human approval or B0 acceptance.
#[test]
fn ask_cli_wait_does_not_block_another_module_and_relay_is_process_authored() {
    let fixture = Fixture::new();
    fixture.add("T-ask", "A");
    fixture.add("T-other", "B");
    let first = fixture.worktree("T-ask");
    let other = fixture.worktree("T-other");
    let run = fixture.json(&first, &["task", "claim", "T-ask"]);
    fixture.json(&other, &["task", "claim", "T-other"]);
    let id = uuid::Uuid::now_v7().to_string();
    let hold = format!("H-{id}");
    let mut asking = fixture.command(&first, &["ask", "是否继续？", "--request-id", &id]);
    let waiter = thread::spawn(move || asking.output().unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let actual = journal(&fixture);
        if actual["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "hold.wait_started" && e["obj"] == hold)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Ask did not reach its durable slice: {actual}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    // The route is still waiting. This same project must remain usable from
    // another bound worktree and the reply must enter through its real CLI.
    let begin = std::time::Instant::now();
    let observed = fixture.json(&other, &["task", "show"]);
    assert_eq!(observed["task"], "T-other");
    assert!(begin.elapsed() < Duration::from_secs(10));
    let reply = fixture.json(
        &first,
        &[
            "ask",
            "--reply",
            &hold,
            "--answer",
            "先停止\r\n保留原文",
            "--harness",
            "fixture",
            "--harness-version",
            "1",
        ],
    );
    assert_eq!(reply["current"]["state"], "answered");
    let output = waiter.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let finished: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(finished["current"]["reply"]["actor"], "process:towermd-cli");
    assert_eq!(finished["current"]["reply"]["relayed_from"], "human");
    assert_eq!(
        finished["current"]["reply"]["self_reported"]["harness"],
        "fixture"
    );
    assert_eq!(finished["current"]["reply"]["text"], "先停止\r\n保留原文");
    assert_eq!(finished["slice"]["recipient_run"], run["id"]);
    assert!(finished["receipt"].is_object());
    let wait_id = finished["receipt"]["request_id"].as_str().unwrap();
    fixture.success(fixture.root.path(), &["daemon", "stop"]);
    let replay = fixture.json(&first, &["ask", "--wait", &hold, "--request-id", wait_id]);
    assert_eq!(replay["receipt"], finished["receipt"]);
    let conflict = fixture
        .command(&first, &["ask", "different body", "--request-id", wait_id])
        .output()
        .unwrap();
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains(wait_id));
}

#[test]
fn ask_http_refuses_human_actions_and_invalid_input_cannot_create_hold() {
    let f = Fixture::new();
    f.add("T-ask", "A");
    let tree = f.worktree("T-ask");
    f.json(&tree, &["task", "claim", "T-ask"]);
    for args in [
        vec!["ask", "？！"],
        vec!["ask", "问题", "--recommended", "0"],
        vec!["ask", "--reply", "H-unknown", "--answer", "答复"],
        vec!["ask", "问题", "--actor", "human:owner"],
    ] {
        assert!(!f.command(&tree, &args).output().unwrap().status.success());
    }
    let endpoint = recovery_endpoint(&f);
    let response = reqwest::blocking::Client::builder().no_proxy().build().unwrap()
        .post(format!("http://127.0.0.1:{}/interaction", endpoint["port"]))
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .json(&json!({"directory":tree,"run":null,"request_id":uuid::Uuid::now_v7().to_string(),"command":{"action":"pause_all"}}))
        .send().unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(journal(&f)["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| !e["type"].as_str().unwrap_or_default().starts_with("hold.")));
}

#[test]
fn ask_cli_unanswered_slice_has_a_real_terminal_receipt_within_one_wait() {
    let f = Fixture::new();
    f.add("T-timeout", "A");
    let tree = f.worktree("T-timeout");
    f.json(&tree, &["task", "claim", "T-timeout"]);
    let begin = std::time::Instant::now();
    let result = f.json(&tree, &["ask", "本次保持未回答"]);
    assert!(begin.elapsed() >= Duration::from_secs(89));
    assert!(begin.elapsed() < Duration::from_secs(110));
    assert_eq!(result["current"]["state"], "pending");
    assert!(result["current"]["reply"].is_null());
    assert!(result["receipt"].is_object());
    assert_eq!(
        result["slice"]["finished_ms"],
        result["slice"]["deadline_ms"]
    );
    assert_eq!(
        result["slice"]["deadline_ms"].as_u64().unwrap()
            - result["slice"]["started_ms"].as_u64().unwrap(),
        90_000
    );
    let again = std::time::Instant::now();
    let retry = f.json(
        &tree,
        &[
            "ask",
            "--wait",
            result["hold"].as_str().unwrap(),
            "--request-id",
            result["receipt"]["request_id"].as_str().unwrap(),
        ],
    );
    assert!(again.elapsed() < Duration::from_secs(10));
    assert_eq!(retry["receipt"], result["receipt"]);
}

#[test]
fn ask_cli_shutdown_preserves_explicit_run_in_the_original_wait_retry() {
    let f = Fixture::new();
    f.add("T-explicit", "A");
    let tree = f.worktree("T-explicit");
    let run = f.json(&tree, &["task", "claim", "T-explicit"]);
    let run = run["id"].as_str().unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let hold = format!("H-{id}");
    let mut call = f.command(
        &tree,
        &["ask", "显式运行的等待", "--request-id", &id, "--run", run],
    );
    let waiter = thread::spawn(move || call.output().unwrap());
    let end = std::time::Instant::now() + Duration::from_secs(30);
    let wait_id = loop {
        let actual = journal(&f);
        if let Some(event) = actual["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["type"] == "hold.wait_started" && e["obj"] == hold)
        {
            assert_eq!(event["data"]["request_body"]["run"], run);
            break event["data"]["p3_request"]["request_id"]
                .as_str()
                .unwrap()
                .to_owned();
        }
        assert!(
            std::time::Instant::now() < end,
            "waiting intent not accepted: {actual}"
        );
        thread::sleep(Duration::from_millis(50));
    };
    f.success(f.root.path(), &["daemon", "stop"]);
    let output = waiter.join().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains(&format!(
            "towermd ask --wait {hold} --request-id {wait_id} --run {run}"
        )),
        "{error}"
    );
    f.json(
        &tree,
        &[
            "ask",
            "--reply",
            &hold,
            "--answer",
            "重新启动后转述",
            "--run",
            run,
        ],
    );
    let retried = f.json(
        &tree,
        &[
            "ask",
            "--wait",
            &hold,
            "--request-id",
            &wait_id,
            "--run",
            run,
        ],
    );
    assert_eq!(retried["receipt"]["request_id"], wait_id);
    assert_eq!(retried["current"]["state"], "answered");
    assert_eq!(
        journal(&f)["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "hold.wait_started" && e["obj"] == hold)
            .count(),
        1
    );
}
