use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tempfile::TempDir;
use towermd_core::{
    repo,
    verifier::{
        validate_commands, verify, verify_sealed, verify_sealed_with_cleanup, VerificationCommand,
        VerificationRequest,
    },
};
fn git(root: &Path, args: &[&str]) -> String {
    repo::git(root, args).unwrap()
}
fn fixture(script: &str) -> TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    fs::write(root.path().join("check.sh"), script).unwrap();
    fs::write(root.path().join("staged"), "original").unwrap();
    git(root.path(), &["add", "check.sh", "staged"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=codex/gpt-6",
            "-c",
            "user.email=codex@towermd.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ],
    );
    root
}
fn request(root: &Path) -> VerificationRequest {
    let common = fs::canonicalize(root.join(".git")).unwrap();
    VerificationRequest {
        root: fs::canonicalize(root).unwrap(),
        log_root: common.join("towermd/cache/verification"),
        common_dir: common,
        revision: git(root, &["rev-parse", "HEAD"]),
        commands: vec![VerificationCommand {
            cwd: ".".into(),
            argv: vec!["sh".into(), "check.sh".into()],
        }],
        timeout: Duration::from_secs(3),
        cancelled: Arc::new(AtomicBool::new(false)),
    }
}
#[test]
fn selected_commit_preserves_dirty_files_index_status_and_main() {
    let root = fixture("printf 'selected output\n'; printf 'error output\n' >&2; exit 0\n");
    let req = request(root.path());
    let tree = git(root.path(), &["rev-parse", "HEAD^{tree}"]);
    fs::write(root.path().join("staged"), "staged").unwrap();
    git(root.path(), &["add", "staged"]);
    fs::write(root.path().join("staged"), "unstaged").unwrap();
    fs::write(root.path().join("check.sh"), "exit 1\n").unwrap();
    fs::write(root.path().join("untracked"), "untracked").unwrap();
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let status = git(root.path(), &["status", "--porcelain=v1"]);
    let report = verify(&req).expect("selected revision should run");
    assert_eq!(report.commit, req.revision);
    assert_eq!(report.tree, tree);
    assert_eq!(report.executions.len(), 1);
    let execution = &report.executions[0];
    assert_eq!(execution.result, "passed");
    assert_eq!(
        fs::read_to_string(&execution.stdout_log).unwrap(),
        "selected output\n"
    );
    assert_eq!(
        fs::read_to_string(&execution.stderr_log).unwrap(),
        "error output\n"
    );
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    assert_eq!(git(root.path(), &["status", "--porcelain=v1"]), status);
    assert_eq!(git(root.path(), &["rev-parse", "main"]), req.revision);
    assert_eq!(
        fs::read_to_string(root.path().join("staged")).unwrap(),
        "unstaged"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("check.sh")).unwrap(),
        "exit 1\n"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("untracked")).unwrap(),
        "untracked"
    );
    assert_eq!(
        fs::read_dir(req.common_dir.join("towermd/verify"))
            .unwrap()
            .count(),
        0
    );
}

fn command(argv: &[&str]) -> VerificationCommand {
    VerificationCommand {
        cwd: ".".into(),
        argv: argv.iter().map(|v| (*v).into()).collect(),
    }
}
fn digest(path: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(fs::read(path).unwrap()))
}

#[test]
fn sealed_execution_binds_the_real_report_and_runs_only_once() {
    let counter = tempfile::tempdir().unwrap();
    let count_path = counter.path().join("executions");
    let root = fixture("printf 'sealed output\\n'\n");
    let mut req = request(root.path());
    req.commands = vec![command(&[
        "sh",
        "-c",
        "printf x >> \"$1\"; printf 'sealed output\\n'",
        "verify",
        count_path.to_str().unwrap(),
    ])];
    let sealed = verify_sealed(&req).unwrap();
    assert_eq!(fs::read(count_path).unwrap(), b"x");
    assert_eq!(sealed.root(), req.root);
    assert_eq!(sealed.common_dir(), req.common_dir);
    assert_eq!(sealed.commands(), req.commands);
    assert_eq!(sealed.report().commit, req.revision);
    let execution = &sealed.report().executions[0];
    assert_eq!(execution.result, "passed");
    assert_eq!(digest(&execution.stdout_log), execution.stdout_sha256);
    let report_path = Path::new(&execution.stdout_log)
        .parent()
        .unwrap()
        .join("report.json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(report_path).unwrap()).unwrap(),
        serde_json::to_value(sealed.report()).unwrap()
    );
    assert_eq!(
        fs::read_dir(req.common_dir.join("towermd/verify"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn shared_command_validation_rejects_bad_commands_before_execution() {
    let root = fixture("exit 0\n");
    for (cwd, argv) in [
        ("../escape", vec!["sh"]),
        ("a//b", vec!["sh"]),
        (".", vec![]),
        (".", vec![""]),
        (".", vec!["sh", "bad\0argument"]),
    ] {
        let mut req = request(root.path());
        req.commands = vec![VerificationCommand {
            cwd: cwd.into(),
            argv: argv.into_iter().map(str::to_owned).collect(),
        }];
        assert!(validate_commands(&req.commands).is_err());
        assert!(verify_sealed(&req).is_err());
        assert!(!req.log_root.exists());
        assert!(!req.common_dir.join("towermd/verify").exists());
    }
}
#[test]
fn failures_continue_with_distinct_diagnostics_and_sealed_logs() {
    let root = fixture("exit 0\n");
    let mut req = request(root.path());
    req.timeout = Duration::from_millis(150);
    req.commands = vec![
        command(&["sh", "-c", "printf bad >&2; exit 7"]),
        command(&["towermd-nonexistent-verifier-tool"]),
        command(&["sh", "-c", "sleep 20"]),
        command(&["sh", "-c", "printf good"]),
    ];
    let report = verify(&req).unwrap();
    assert_eq!(
        report
            .executions
            .iter()
            .map(|e| e.result.as_str())
            .collect::<Vec<_>>(),
        vec!["failed", "failed", "failed", "passed"]
    );
    assert_eq!(report.executions[0].exit_code, Some(7));
    assert!(report.executions[0]
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("exit"));
    assert!(report.executions[1]
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("spawn"));
    assert!(report.executions[2].timed_out);
    assert!(report.executions[2]
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("timeout"));
    for e in &report.executions {
        assert_eq!(digest(&e.stdout_log), e.stdout_sha256);
        assert_eq!(digest(&e.stderr_log), e.stderr_sha256);
    }
    let directory = Path::new(&report.executions[0].stdout_log)
        .parent()
        .unwrap();
    let bytes = fs::read(directory.join("report.json")).unwrap();
    assert_eq!(
        fs::read_to_string(directory.join("report.sha256"))
            .unwrap()
            .trim(),
        digest(directory.join("report.json").to_str().unwrap())
    );
    assert_eq!(
        towermd_core::verifier::report_hash(&report).unwrap(),
        digest(directory.join("report.json").to_str().unwrap())
    );
    assert_eq!(
        serde_json::from_slice::<towermd_core::verifier::VerificationReport>(&bytes).unwrap(),
        report
    );
    let again = verify(&req).unwrap();
    assert_ne!(
        again.executions[0].stdout_log,
        report.executions[0].stdout_log
    );
    assert_eq!(fs::read(directory.join("report.json")).unwrap(), bytes);
}
#[test]
fn invalid_bindings_and_inputs_fail_before_execution() {
    let root = fixture("exit 0\n");
    let req = request(root.path());
    let other = fixture("exit 0\n");
    let mut wrong = request(root.path());
    wrong.common_dir = other.path().join(".git");
    assert!(verify(&wrong).unwrap_err().contains("common"));
    for revision in [
        "HEAD".to_string(),
        req.revision[..12].to_string(),
        format!("{}^{{tree}}", req.revision),
    ] {
        let mut bad = request(root.path());
        bad.revision = revision;
        assert!(verify(&bad).unwrap_err().contains("commit"));
    }
    for argv in [vec![], vec!["".into()], vec!["sh".into(), "a\0b".into()]] {
        let mut bad = request(root.path());
        bad.commands[0].argv = argv;
        assert!(verify(&bad).unwrap_err().contains("argv"));
    }
    for cwd in ["", "..", "a/../b", "/tmp", "./a", "a//b"] {
        let mut bad = request(root.path());
        bad.commands[0].cwd = cwd.into();
        assert!(verify(&bad).unwrap_err().contains("cwd"));
    }
    let mut bad = request(root.path());
    bad.log_root = root.path().join("docs");
    assert!(verify(&bad).unwrap_err().contains("log_root"));
    assert!(!root.path().join("docs").exists());
}
#[test]
fn cancellation_never_marks_unexecuted_commands_passed() {
    let root = fixture("exit 0\n");
    let mut req = request(root.path());
    req.cancelled
        .store(true, std::sync::atomic::Ordering::SeqCst);
    req.commands = vec![
        command(&["sh", "-c", "exit 0"]),
        command(&["sh", "-c", "exit 0"]),
    ];
    let report = verify(&req).unwrap();
    assert_eq!(report.executions.len(), 2);
    assert!(report
        .executions
        .iter()
        .all(|e| e.result == "failed" && e.diagnostic.as_ref().unwrap().contains("cancel")));
}
#[test]
fn empty_commands_are_an_empty_report_not_an_invented_pass() {
    let root = fixture("exit 0\n");
    let mut req = request(root.path());
    req.commands.clear();
    assert!(verify(&req).unwrap().executions.is_empty());
}
#[cfg(unix)]
#[test]
fn symlink_cwd_and_cache_escape_are_rejected_and_other_worktree_survives() {
    use std::os::unix::fs::symlink;
    let root = fixture("exit 0\n");
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    git(root.path(), &["add", "escape"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=codex/gpt-6",
            "-c",
            "user.email=codex@towermd.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "symlink",
        ],
    );
    let mut req = request(root.path());
    req.commands[0].cwd = "escape".into();
    let other = req
        .common_dir
        .join("towermd/verify/other-owned-by-someone-else");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    git(
        root.path(),
        &[
            "worktree",
            "add",
            "--detach",
            other.to_str().unwrap(),
            &req.revision,
        ],
    );
    assert!(verify(&req).unwrap_err().contains("cwd"));
    assert!(other.join("check.sh").exists());
    let verify_dir = req.common_dir.join("towermd/verify");
    assert_eq!(fs::read_dir(verify_dir).unwrap().count(), 1);
    fs::remove_dir_all(req.common_dir.join("towermd/cache")).unwrap();
    symlink(outside.path(), req.common_dir.join("towermd/cache")).unwrap();
    req.commands[0].cwd = ".".into();
    assert!(verify(&req).unwrap_err().contains("symlink"));
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    git(
        root.path(),
        &["worktree", "remove", "--force", other.to_str().unwrap()],
    );
}
#[test]
fn both_streams_are_drained_without_a_pipe_deadlock() {
    let root = fixture("exit 0\n");
    let mut req = request(root.path());
    req.commands = vec![command(&[
        "python3",
        "-c",
        "import os; [(os.write(1,b'a'*8192),os.write(2,b'b'*8192)) for _ in range(128)]",
    ])];
    let report = verify(&req).unwrap();
    let e = &report.executions[0];
    assert_eq!(e.result, "passed");
    assert_eq!(fs::metadata(&e.stdout_log).unwrap().len(), 1048576);
    assert_eq!(fs::metadata(&e.stderr_log).unwrap().len(), 1048576);
}
#[test]
fn inherited_environment_and_loopback_use_a_separate_test_process() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::Command,
    };
    if std::env::var_os("TOWERMD_VERIFIER_ENV_CHILD").is_some() {
        let root = fixture("exit 0\n");
        let mut req = request(root.path());
        req.commands=vec![command(&["python3","-c", "import os,socket; assert os.environ['TOWERMD_VERIFIER_MARKER']=='inherited'; assert 'GIT_DIR' not in os.environ; s=socket.create_connection(('127.0.0.1',int(os.environ['TOWERMD_VERIFIER_PORT'])),timeout=2); s.sendall(b'hello'); assert s.recv(5)==b'world'"])];
        let report = verify(&req).unwrap();
        println!(
            "actual command stderr: {}",
            fs::read_to_string(&report.executions[0].stderr_log).unwrap()
        );
        assert_eq!(report.executions[0].result, "passed", "{:?}", report);
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inherited_environment_and_loopback_use_a_separate_test_process",
            "--nocapture",
        ])
        .env("TOWERMD_VERIFIER_ENV_CHILD", "1")
        .env("TOWERMD_VERIFIER_MARKER", "inherited")
        .env(
            "TOWERMD_VERIFIER_PORT",
            listener.local_addr().unwrap().port().to_string(),
        )
        .env("GIT_DIR", "/nonexistent/towermd-test-redirect")
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut msg = [0; 5];
                stream.read_exact(&mut msg).unwrap();
                assert_eq!(&msg, b"hello");
                stream.write_all(b"world").unwrap();
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("child exited before loopback: {status}");
                }
                if started.elapsed() > Duration::from_secs(10) {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("loopback timeout");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!(child.wait().unwrap().success());
}
struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(unix)]
struct PidCleanup {
    path: std::path::PathBuf,
    armed: bool,
}
#[cfg(unix)]
impl Drop for PidCleanup {
    fn drop(&mut self) {
        if self.armed {
            if let Ok(pid) = fs::read_to_string(self.path.join("parent")) {
                let _ = std::process::Command::new("/bin/kill")
                    .args(["-KILL", "--", &format!("-{}", pid.trim())])
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
    }
}
#[cfg(unix)]
fn assert_dead(pid: &str) {
    let started = std::time::Instant::now();
    while std::process::Command::new("/bin/kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
    {
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "process still alive: {pid}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("/bin/kill -0 {pid}: exited (nonzero)");
}
#[cfg(unix)]
#[test]
fn timeout_cancellation_and_exited_leader_kill_real_process_groups() {
    for mode in ["timeout", "cancel", "background"] {
        let pids = tempfile::tempdir().unwrap();
        let mut cleanup = PidCleanup {
            path: pids.path().to_path_buf(),
            armed: true,
        };
        let script=format!("echo $$ > '{}/parent'; sh -c 'trap \"\" TERM; echo $$ > \"{}/child\"; while :; do sleep 1; done' &\n{}\n",pids.path().display(),pids.path().display(),if mode=="background" {"while [ ! -s \"CHILD_FILE\" ]; do sleep 0.01; done; exit 0"}else{"wait"});
        let script = script.replace("CHILD_FILE", &pids.path().join("child").to_string_lossy());
        let root = fixture(&script);
        let mut req = request(root.path());
        req.timeout = Duration::from_millis(500);
        let cancellation = req.cancelled.clone();
        let child_file = pids.path().join("child");
        let cancel_thread = if mode == "cancel" {
            Some(std::thread::spawn(move || {
                let start = std::time::Instant::now();
                while !child_file.exists() {
                    assert!(start.elapsed() < Duration::from_secs(3));
                    std::thread::sleep(Duration::from_millis(5));
                }
                cancellation.store(true, std::sync::atomic::Ordering::SeqCst);
            }))
        } else {
            None
        };
        let unrelated = std::process::Command::new("sleep")
            .arg("20")
            .spawn()
            .unwrap();
        let mut unrelated = OwnedChild(unrelated);
        let started = std::time::Instant::now();
        let result = verify(&req);
        assert!(started.elapsed() < Duration::from_secs(5));
        if let Some(t) = cancel_thread {
            t.join().unwrap();
        }
        for file in ["parent", "child"] {
            let pid = fs::read_to_string(pids.path().join(file))
                .expect("fixture reached actual child stage");
            println!("actual {mode} {file} PID={}", pid.trim());
            assert_dead(pid.trim());
        }
        cleanup.armed = false;
        let report = result.unwrap();
        assert_eq!(
            report.executions[0].result,
            if mode == "background" {
                "passed"
            } else {
                "failed"
            }
        );
        assert!(unrelated.0.try_wait().unwrap().is_none());
        unrelated.0.kill().unwrap();
        unrelated.0.wait().unwrap();
        assert_eq!(
            fs::read_dir(req.common_dir.join("towermd/verify"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn canonical_paths_are_required() {
    let root = fixture("exit 0\n");
    for field in ["root", "common"] {
        let mut req = request(root.path());
        if field == "root" {
            req.root.push(".");
        } else {
            req.common_dir.push(".");
        }
        assert!(verify(&req).unwrap_err().contains("canonical"));
    }
}
#[test]
fn failed_add_releases_its_empty_reservation() {
    let root = fixture("exit 0\n");
    let req = request(root.path());
    fs::write(
        req.common_dir.join("worktrees"),
        "reserved unrelated sentinel",
    )
    .unwrap();
    assert!(verify(&req).is_err());
    assert_eq!(
        fs::read_dir(req.common_dir.join("towermd/verify"))
            .unwrap()
            .count(),
        0,
        "failed add must release its exact empty reservation"
    );
    assert_eq!(
        fs::read_to_string(req.common_dir.join("worktrees")).unwrap(),
        "reserved unrelated sentinel"
    );
}
#[cfg(unix)]
#[test]
fn infrastructure_failures_never_overwrite_logs_or_publish_a_passed_report() {
    use std::os::unix::fs::PermissionsExt;
    for failure in ["add", "stdout_collision", "report_collision", "seal"] {
        let root = fixture(if failure == "seal" {
            "common=$(git rev-parse --path-format=absolute --git-common-dir)\nfor log in \"$common\"/towermd/cache/verification/*/0-stdout.log; do mv \"$log\" \"$log.saved\"; done\nprintf success\n"
        } else {
            "exit 0\n"
        });
        let req = request(root.path());
        let hook = req.common_dir.join("hooks/post-checkout");
        if failure != "seal" {
            let body = if failure == "add" {
                "exit 19\n".to_string()
            } else {
                format!("common=$(git rev-parse --path-format=absolute --git-common-dir)\nfor dir in \"$common\"/towermd/cache/verification/*; do printf sentinel > \"$dir/{}\"; done\n",if failure=="stdout_collision"{"0-stdout.log"}else{"report.json"})
            };
            fs::write(&hook, format!("#!/bin/sh\n{body}")).unwrap();
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let error = verify(&req).unwrap_err();
        println!("actual infrastructure {failure}: {error}");
        assert_eq!(
            fs::read_dir(req.common_dir.join("towermd/verify"))
                .unwrap()
                .count(),
            0
        );
        let entry = fs::read_dir(&req.log_root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        if failure == "report_collision" {
            assert_eq!(
                fs::read_to_string(entry.join("report.json")).unwrap(),
                "sentinel"
            );
        } else {
            assert!(!entry.join("report.json").exists());
        }
        if failure == "stdout_collision" {
            assert_eq!(
                fs::read_to_string(entry.join("0-stdout.log")).unwrap(),
                "sentinel"
            );
        }
    }
}

#[test]
fn argv_preserves_empty_arguments_and_literal_shell_metacharacters() {
    let root = fixture("exit 0\n");
    let mut req = request(root.path());
    let literal = format!(
        "$(touch {}); literal",
        root.path().join("must-not-exist").display()
    );
    req.commands = vec![command(&["/usr/bin/printf", "[%s]", &literal, ""])];
    let report = verify(&req).unwrap();
    assert_eq!(report.executions[0].result, "passed");
    assert_eq!(
        fs::read_to_string(&report.executions[0].stdout_log).unwrap(),
        format!("[{literal}][]")
    );
    assert!(!root.path().join("must-not-exist").exists());
    assert_eq!(
        towermd_core::verifier::commands_hash(&req.commands).unwrap(),
        towermd_core::recovery_contract::verification::commands_hash(&req.commands).unwrap()
    );
    assert_eq!(
        towermd_core::verifier::report_hash(&report).unwrap(),
        towermd_core::recovery_contract::verification::report_hash(&report).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn later_commands_revalidate_cwd_after_an_earlier_command_changes_it() {
    let root = fixture("exit 0\n");
    fs::create_dir(root.path().join("safe")).unwrap();
    fs::write(root.path().join("safe/keep"), "tracked").unwrap();
    git(root.path(), &["add", "safe"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=codex/gpt-6",
            "-c",
            "user.email=codex@towermd.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "safe cwd",
        ],
    );
    let outside = tempfile::tempdir().unwrap();
    let mut req = request(root.path());
    req.commands = vec![
        command(&[
            "sh",
            "-c",
            &format!("rm -rf safe; ln -s '{}' safe", outside.path().display()),
        ]),
        VerificationCommand {
            cwd: "safe".into(),
            argv: vec!["sh".into(), "-c".into(), "echo escaped > marker".into()],
        },
    ];
    let result = verify(&req);
    assert!(
        !outside.path().join("marker").exists(),
        "later cwd followed a symlink outside its owned worktree"
    );
    assert!(result.unwrap_err().contains("cwd"));
    assert_eq!(
        fs::read_dir(req.common_dir.join("towermd/verify"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn typed_cleanup_is_independent_of_validation_spawn_and_post_cleanup_infrastructure_errors() {
    let root = fixture("exit 0\n");
    let mut req = request(root.path());
    req.revision = "not-an-object".into();
    let outcome = verify_sealed_with_cleanup(&req);
    assert!(
        outcome.cleanup_confirmed(),
        "validation failed before any process existed"
    );
    assert!(outcome.into_result().is_err());

    req = request(root.path());
    req.commands = vec![command(&["towermd-nonexistent-verifier-tool"])];
    let outcome = verify_sealed_with_cleanup(&req);
    assert!(
        outcome.cleanup_confirmed(),
        "spawn failure creates no child to drain"
    );
    let report = outcome.into_result().unwrap();
    assert_eq!(report.report().executions[0].result, "failed");
    assert!(report.report().executions[0]
        .diagnostic
        .as_ref()
        .unwrap()
        .contains("spawn failed"));

    // The real command removes its own opened stdout path. Log sealing then
    // fails after the actual process group was killed/reaped; Err is not a
    // claim that an owned process remains alive.
    req.commands = vec![command(&[
        "sh",
        "-c",
        "for entry in \"$1\"/*/0-stdout.log; do rm -- \"$entry\"; done",
        "verify",
        req.log_root.to_str().unwrap(),
    ])];
    let outcome = verify_sealed_with_cleanup(&req);
    assert!(
        outcome.cleanup_confirmed(),
        "post-process IO error cannot erase actual cleanup"
    );
    assert!(outcome.into_result().is_err());
}
