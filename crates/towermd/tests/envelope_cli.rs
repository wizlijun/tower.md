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
    let repo = empty_repo();
    let runtime = tempfile::tempdir().unwrap();
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

fn towermd_with_stdin(cwd: &Path, runtime: &Path, args: &[&str], input: &[u8]) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_towermd"))
        .args(args)
        .current_dir(cwd)
        .env("TOWERMD_RUNTIME_DIR", runtime)
        .env("TOWERMD_LANG", "zh-CN")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn empty_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "--allow-empty", "-m", "root"]);
    repo
}

#[test]
fn another_project_is_a_usage_error() {
    let repo = empty_repo();
    let runtime = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join(".towermd")).unwrap();
    fs::write(repo.path().join(".towermd/config.yaml"), "project_id: here\ndocs_dir: docs/tower\nhub: local\n").unwrap();
    for args in [vec!["--project", "elsewhere", "status"], vec!["show", "elsewhere"]] {
        let output = towermd(repo.path(), runtime.path(), &args);
        assert_eq!(envelope::code(&output), "usage", "{args:?}");
        assert!(envelope::parse(&output)["error"]["message"].as_str().unwrap().contains("elsewhere"));
    }
    // Rejected locally: no daemon was started for a foreign project.
    assert!(!runtime.path().join("endpoint.json").exists());
}

#[test]
fn input_dash_reads_standard_input() {
    let repo = empty_repo();
    let runtime = tempfile::tempdir().unwrap();
    let output = towermd_with_stdin(repo.path(), runtime.path(), &["task", "add", "-"], b"{");
    assert_eq!(envelope::code(&output), "usage");
    assert!(envelope::parse(&output)["error"]["message"].as_str().unwrap().contains("stdin"));
}

#[test]
fn answer_without_relayed_needs_a_human() {
    let repo = empty_repo();
    let runtime = tempfile::tempdir().unwrap();
    let output = towermd(repo.path(), runtime.path(), &["answer", "H-1", "yes"]);
    assert_eq!(envelope::code(&output), "needs_human");
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(envelope::parse(&output)["next"][0]["cmd"], "towermd up");
    // Repeating the same call cannot succeed, so no retry is offered.
    assert!(envelope::parse(&output)["error"].get("retry").is_none());
}

#[test]
fn v0_1_1_command_names_are_usage_errors() {
    let repo = empty_repo();
    let runtime = tempfile::tempdir().unwrap();
    for args in [
        vec!["baseline", "scan"],
        vec!["check"],
        vec!["finish"],
        vec!["trailers", "T-1"],
        vec!["project", "view"],
        vec!["pause", "--all"],
        vec!["ask", "--wait", "H-1"],
        // The command line never approves anything.
        vec!["dev", "draft", "approve", "d-1"],
        // Malformed IDs are rejected locally as usage, not internal.
        vec!["dev", "draft", "show", "d-not-a-digest"],
        vec!["dev", "draft", "show", "0190a0b0-0000-7000-8000-000000000000"],
        vec!["dev", "scan", "show", "d-1"],
    ] {
        let output = towermd(repo.path(), runtime.path(), &args);
        assert_eq!(envelope::code(&output), "usage", "{args:?}");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    assert!(!runtime.path().join("endpoint.json").exists());
}

#[test]
fn schema_lists_commands_and_describes_input_and_output() {
    let repo = empty_repo();
    let runtime = tempfile::tempdir().unwrap();
    let list = envelope::data(&towermd(repo.path(), runtime.path(), &["schema"]));
    let names: Vec<&str> = list["commands"].as_array().unwrap().iter().map(|name| name.as_str().unwrap()).collect();
    assert_eq!(names.len(), 31);
    assert!(names.contains(&"dev.draft.submit") && names.contains(&"task.claim"));
    let claim = envelope::data(&towermd(repo.path(), runtime.path(), &["schema", "task.claim"]));
    assert!(claim["output"]["required"].as_array().unwrap().contains(&serde_json::json!("run")));
    let submit = envelope::data(&towermd(repo.path(), runtime.path(), &["schema", "dev.draft.submit"]));
    assert!(submit["input"].is_object() && submit["output"].is_object());
    let envelope_schema = envelope::data(&towermd(repo.path(), runtime.path(), &["schema", "envelope"]));
    assert!(envelope_schema["output"]["properties"]["error"]["properties"]["code"]["enum"].as_array().unwrap().contains(&serde_json::json!("rejected")));
    assert_eq!(envelope::code(&towermd(repo.path(), runtime.path(), &["schema", "nope"])), "usage");
    assert!(!runtime.path().join("endpoint.json").exists(), "schema never needs the daemon");
}

#[test]
fn help_follows_the_language_and_shows_examples() {
    let repo = empty_repo();
    let runtime = tempfile::tempdir().unwrap();
    let english = towermd(repo.path(), runtime.path(), &["--lang", "en", "help", "task", "add"]);
    assert!(english.status.success());
    let english = String::from_utf8(english.stdout).unwrap();
    assert!(english.contains("Examples:") && english.contains("towermd task add"), "{english}");
    let chinese = towermd(repo.path(), runtime.path(), &["--lang", "zh-CN", "task", "add", "--help"]);
    let chinese = String::from_utf8(chinese.stdout).unwrap();
    assert!(chinese.contains("示例：") && chinese.contains("新建任务"), "{chinese}");
    assert!(!runtime.path().join("endpoint.json").exists());
}
