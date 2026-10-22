#[path = "support/envelope.rs"]
mod envelope;

use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
};

fn towermd(cwd: &Path, runtime: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_towermd"))
        .args(args)
        .current_dir(cwd)
        .env("TOWERMD_RUNTIME_DIR", runtime)
        .env("TOWERMD_LANG", "zh-CN")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn json_success_is_one_envelope() {
    let cwd = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let output = towermd(cwd.path(), runtime.path(), &["daemon", "status"]);
    assert_eq!(envelope::data(&output)["running"], false);
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn usage_errors_exit_two_without_retry() {
    let cwd = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    for args in [vec!["status", "--no-such-flag"], vec![]] {
        let output = towermd(cwd.path(), runtime.path(), &args);
        let envelope = envelope::parse(&output);
        assert_eq!(envelope["error"]["code"], "usage", "{args:?}");
        assert!(envelope["error"]["hint"].as_str().unwrap().contains("--help"));
        assert!(envelope["error"].get("retry").is_none());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.starts_with("towermd: usage: "), "{stderr}");
    }
}

#[test]
fn text_failures_write_only_to_stderr() {
    let cwd = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let output = towermd(cwd.path(), runtime.path(), &["--text", "task", "release", "--request-id", "not-a-uuid"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("towermd: ") && stderr.contains("--request-id"), "{stderr}");
}

#[test]
fn oversized_input_is_too_long() {
    let repo = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "--allow-empty", "-m", "root"]);
    let big = repo.path().join("big.json");
    fs::write(&big, format!("\"{}\"", "x".repeat(2 * 1024 * 1024))).unwrap();
    let output = towermd(repo.path(), runtime.path(), &["task", "add", big.to_str().unwrap()]);
    assert_eq!(envelope::code(&output), "too_long");
    assert!(envelope::parse(&output)["error"].get("retry").is_none());
}

#[test]
fn unreachable_daemon_exits_eight() {
    let cwd = tempfile::tempdir().unwrap();
    let blocker = cwd.path().join("not-a-directory");
    fs::write(&blocker, "").unwrap();
    let output = towermd(cwd.path(), &blocker.join("runtime"), &["daemon", "ensure"]);
    let envelope = envelope::parse(&output);
    assert_eq!(envelope["error"]["code"], "daemon_unavailable");
    assert_eq!(envelope["error"]["retry"]["safe"], true);
    assert_eq!(envelope["next"][0]["cmd"], "towermd daemon status");
}
