#![cfg(unix)]
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use towermd_core::{
    cold_start::{ScanCommand, ScanModule},
    verifier::{run, Limits, VerificationPlan, VerificationStatus as Status},
};

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let output = command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Verifier Test",
            "-c",
            "user.email=verifier@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn module(id: &str, ids: &[&str]) -> ScanModule {
    ScanModule {
        id: id.into(),
        title: id.into(),
        paths: vec!["source.rs".into()],
        files: vec!["source.rs".into()],
        confidence: 0.5,
        rationale: "fixture".into(),
        command_ids: ids.iter().map(|id| (*id).into()).collect(),
        suggested_task: None,
    }
}
fn fixture(script: &str) -> (tempfile::TempDir, VerificationPlan) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    write(root, "verify.sh", script);
    write(root, "source.rs", "fn initial() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    let plan = VerificationPlan {
        root: root.into(),
        id: "verify-fixture".into(),
        revision: git(root, &["rev-parse", "HEAD"]),
        tree: git(root, &["rev-parse", "HEAD^{tree}"]),
        commands: vec![ScanCommand {
            id: "command-1".into(),
            directory: String::new(),
            command: "sh verify.sh".into(),
            modules: vec!["M1".into()],
        }],
        modules: vec![module("M1", &["command-1"])],
        limits: Limits {
            command_timeout_ms: 2_000,
            total_timeout_ms: 5_000,
            output_limit_bytes: 256,
        },
    };
    (dir, plan)
}
fn assert_clean(root: &Path, plan: &VerificationPlan) {
    assert!(!root
        .join(format!(".git/towermd/verify/{}/worktree", plan.id))
        .exists());
    assert_eq!(git(root, &["rev-parse", "HEAD"]), plan.revision);
    assert_eq!(git(root, &["status", "--porcelain"]), "");
    assert_eq!(
        git(root, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
}

#[test]
fn success_binds_tree_preserves_user_files_and_keeps_bounded_hashed_logs() {
    let (dir, plan) = fixture("test -n \"$PATH\" || exit 8\nprintf 'inherited-home=%s\\n' \"$HOME\"\ni=0; while [ $i -lt 300 ]; do printf abcdefgh; i=$((i+1)); done\nprintf err >&2\n");
    let original_index = fs::read(dir.path().join(".git/index")).unwrap();
    let report = run(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.actor, "process:towermd-verifier");
    assert_eq!(report.tree, plan.tree);
    assert_eq!(report.commands[0].status, Status::Passed);
    assert_eq!(report.modules[0].status, Status::Passed);
    assert!(report.cleanup_complete);
    let result = &report.commands[0];
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.argv, vec!["/bin/sh", "-c", "sh verify.sh"]);
    assert!(result.log_truncated);
    assert!(result.log_excerpt.contains(&format!(
        "inherited-home={}",
        std::env::var("HOME").unwrap_or_default()
    )));
    let log = result.log.as_ref().unwrap();
    assert!(!Path::new(log).is_absolute());
    let bytes = fs::read(dir.path().join(".git").join(log)).unwrap();
    assert_eq!(bytes.len(), 256);
    assert_eq!(
        result.log_sha256.as_deref(),
        Some(hex::encode(Sha256::digest(&bytes)).as_str())
    );
    assert_eq!(
        original_index,
        fs::read(dir.path().join(".git/index")).unwrap()
    );
    assert_clean(dir.path(), &plan);
}

#[test]
fn failed_command_is_real_failure_and_untested_module_remains_no_tests() {
    let (dir, mut plan) = fixture("printf expected-failure >&2\nexit 7\n");
    plan.modules.push(module("M-no-tests", &[]));
    let report = run(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.commands[0].status, Status::Failed);
    assert_eq!(report.commands[0].exit_code, Some(7));
    assert!(report.commands[0].log_excerpt.contains("expected-failure"));
    assert_eq!(report.modules[0].status, Status::Failed);
    assert_eq!(report.modules[1].status, Status::NoTests);
    assert!(report.cleanup_complete);
    assert_clean(dir.path(), &plan);
}

#[test]
fn identical_shared_commands_run_once_and_orphan_commands_do_not_run() {
    let (dir, mut plan) = fixture("test ! -e ran-once || exit 9\ntouch ran-once\nprintf once\n");
    plan.commands[0].modules.push("M2".into());
    let mut alias = plan.commands[0].clone();
    alias.id = "alias".into();
    plan.commands.push(alias);
    plan.modules.push(module("M2", &["alias"]));
    plan.commands.push(ScanCommand {
        id: "orphan".into(),
        directory: String::new(),
        command: "exit 99".into(),
        modules: Vec::new(),
    });
    let report = run(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.commands.len(), 2);
    assert_eq!(report.commands[0].status, Status::Passed);
    assert_eq!(report.commands[1].status, Status::NotRun);
    assert!(report.commands[1].log.is_none());
    assert!(report
        .modules
        .iter()
        .all(|module| module.status == Status::Passed && module.shared));
    assert_clean(dir.path(), &plan);
}

#[test]
fn source_pollution_fails_and_stops_later_commands_without_touching_user_checkout() {
    let (dir, mut plan) = fixture("printf pollution > source.rs\nexit 0\n");
    plan.commands.push(ScanCommand {
        id: "later".into(),
        directory: String::new(),
        command: "printf should-not-run".into(),
        modules: vec!["M1".into()],
    });
    plan.modules[0].command_ids.push("later".into());
    let report = run(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.commands[0].status, Status::Failed);
    assert!(report.commands[0]
        .error
        .as_ref()
        .unwrap()
        .contains("已跟踪文件"));
    assert_eq!(report.commands[1].status, Status::NotRun);
    assert_eq!(
        fs::read_to_string(dir.path().join("source.rs")).unwrap(),
        "fn initial() {}\n"
    );
    assert_clean(dir.path(), &plan);
}

#[test]
fn index_and_head_changes_cannot_be_counted_as_passed() {
    for script in [
        "git update-index --assume-unchanged source.rs\n",
        "git checkout -qb accidental-branch\n",
    ] {
        let (dir, plan) = fixture(script);
        let report = run(&plan, &AtomicBool::new(false)).unwrap();
        assert_eq!(report.commands[0].status, Status::Failed);
        assert_eq!(report.modules[0].status, Status::Failed);
        assert_clean(dir.path(), &plan);
    }
}
fn is_alive(pid: &str) -> bool {
    Command::new("/bin/kill")
        .args(["-0", pid])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}
fn wait_dead(pid: &str) {
    let started = Instant::now();
    while is_alive(pid) && started.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!is_alive(pid), "后台验证进程仍在运行：{pid}");
}

#[test]
fn timeout_cancels_the_process_group_and_removes_detached_worktree() {
    let marker = tempfile::tempdir().unwrap();
    let path = marker.path().join("child.pid");
    let script = format!("sleep 20 &\nchild=$!\nprintf '%s' \"$child\" > '{}'\ntrap 'wait \"$child\" 2>/dev/null; exit 0' TERM\nwait \"$child\"\n", path.display());
    let (dir, mut plan) = fixture(&script);
    plan.limits.command_timeout_ms = 150;
    let started = Instant::now();
    let report = run(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.commands[0].status, Status::TimedOut);
    assert!(report.cleanup_complete);
    assert!(started.elapsed() < Duration::from_secs(4));
    wait_dead(&fs::read_to_string(path).unwrap());
    assert_clean(dir.path(), &plan);
}

#[test]
fn cancellation_interrupts_running_command_and_marks_remaining_not_run() {
    let (dir, mut plan) = fixture(
        "sleep 20 &\nchild=$!\ntrap 'wait \"$child\" 2>/dev/null; exit 0' TERM\nwait \"$child\"\n",
    );
    plan.commands.push(ScanCommand {
        id: "later".into(),
        directory: String::new(),
        command: "echo later".into(),
        modules: vec!["M1".into()],
    });
    plan.modules[0].command_ids.push("later".into());
    let cancel = AtomicBool::new(false);
    let report = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(250));
            cancel.store(true, std::sync::atomic::Ordering::Release);
        });
        run(&plan, &cancel).unwrap()
    });
    assert_eq!(report.commands[0].status, Status::Cancelled);
    assert_eq!(report.commands[1].status, Status::NotRun);
    assert_eq!(report.modules[0].status, Status::Cancelled);
    assert!(report.cleanup_complete);
    assert_clean(dir.path(), &plan);
}

#[test]
fn successful_parent_exit_still_reaps_background_children() {
    let marker = tempfile::tempdir().unwrap();
    let path = marker.path().join("child.pid");
    let script = format!(
        "sleep 20 &\nprintf '%s' \"$!\" > '{}'\nexit 0\n",
        path.display()
    );
    let (dir, plan) = fixture(&script);
    let report = run(&plan, &AtomicBool::new(false)).unwrap();
    assert!(report.cleanup_complete);
    assert_eq!(report.commands[0].status, Status::Passed);
    wait_dead(&fs::read_to_string(path).unwrap());
    assert_clean(dir.path(), &plan);
}

#[test]
fn tree_mismatch_and_path_escape_are_rejected_before_execution() {
    let (dir, mut plan) = fixture("exit 0\n");
    plan.tree = "0".repeat(40);
    assert!(run(&plan, &AtomicBool::new(false))
        .unwrap_err()
        .contains("commit/tree"));
    plan.tree = git(dir.path(), &["rev-parse", "HEAD^{tree}"]);
    plan.commands[0].directory = "../escape".into();
    assert!(run(&plan, &AtomicBool::new(false)).is_err());
    assert!(!dir.path().join(".git/towermd/verify").exists());
}

#[test]
fn symlink_command_directory_is_refused() {
    use std::os::unix::fs::symlink;
    let (dir, mut plan) = fixture("exit 0\n");
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), dir.path().join("linked")).unwrap();
    git(dir.path(), &["add", "linked"]);
    git(dir.path(), &["commit", "-qm", "symlink fixture"]);
    plan.revision = git(dir.path(), &["rev-parse", "HEAD"]);
    plan.tree = git(dir.path(), &["rev-parse", "HEAD^{tree}"]);
    plan.commands[0].directory = "linked".into();
    let report = run(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.commands[0].status, Status::Failed);
    assert!(report.commands[0]
        .error
        .as_ref()
        .unwrap()
        .contains("普通目录"));
    assert_clean(dir.path(), &plan);
}
