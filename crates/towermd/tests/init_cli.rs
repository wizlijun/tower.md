use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn git_repo() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(directory.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    directory
}

fn towermd_command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_towermd"));
    command
        .arg("--text")
        .args(args)
        .env("TOWERMD_LANG", "zh-CN")
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null());
    command
}

fn towermd(cwd: &Path, args: &[&str]) -> Output {
    towermd_command(cwd, args).output().unwrap()
}

fn towermd_with_open_stdin(cwd: &Path, args: &[&str]) -> Output {
    let mut child = towermd_command(cwd, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            drop(stdin);
            return output;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            drop(stdin);
            let output = child.wait_with_output().unwrap();
            panic!("init 等待非交互 stdin 超时：{output:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn init_then_reinit_keeps_project_id() {
    let repo = git_repo();
    let first = towermd(repo.path(), &["init", "--docs-dir", "docs/plan"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let output = stdout(&first);
    assert!(output.contains("新生成"), "{output}");
    assert!(output.contains("docs/plan"), "{output}");
    assert!(output.contains(".towermd/config.yaml"), "{output}");
    assert_eq!(
        output.lines().last(),
        Some("以上文件未提交，请检查后自行提交。")
    );
    assert!(output.contains("项目 ID："), "{output}");
    assert!(output.contains("新建："), "{output}");

    let config = fs::read(repo.path().join(".towermd/config.yaml")).unwrap();
    let again = towermd(repo.path(), &["init"]);
    assert!(again.status.success());
    assert!(stdout(&again).contains("沿用"), "{}", stdout(&again));
    assert_eq!(
        fs::read(repo.path().join(".towermd/config.yaml")).unwrap(),
        config
    );
    assert_eq!(
        output
            .lines()
            .find(|line| line.starts_with("项目 ID："))
            .unwrap()
            .split('（')
            .next(),
        stdout(&again)
            .lines()
            .find(|line| line.starts_with("项目 ID："))
            .unwrap()
            .split('（')
            .next()
    );
}

#[test]
fn path_argument_selects_the_repo() {
    let repo = git_repo();
    let elsewhere = tempfile::tempdir().unwrap();
    let output = towermd(
        elsewhere.path(),
        &[
            "init",
            repo.path().to_str().unwrap(),
            "--docs-dir",
            "docs/tower",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(repo.path().join(".towermd/config.yaml").is_file());
    assert!(stdout(&output).contains("docs/tower"));
}

#[test]
fn non_git_directory_fails_with_reason() {
    let directory = tempfile::tempdir().unwrap();
    let output = towermd(directory.path(), &["init"]);
    assert_eq!(output.status.code(), Some(3));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.starts_with("towermd: "), "{error}");
    assert!(error.contains("不在 git 仓库中"), "{error}");
    assert!(output.stdout.is_empty());
}

#[test]
fn changing_docs_dir_on_reinit_fails() {
    let repo = git_repo();
    assert!(towermd(repo.path(), &["init", "--docs-dir", "docs/a"])
        .status
        .success());
    let config = fs::read(repo.path().join(".towermd/config.yaml")).unwrap();
    let output = towermd(repo.path(), &["init", "--docs-dir", "docs/b"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("towermd: "));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read(repo.path().join(".towermd/config.yaml")).unwrap(),
        config
    );
}

#[test]
fn help_lists_only_implemented_commands() {
    let directory = tempfile::tempdir().unwrap();
    let output = towermd(directory.path(), &["--help"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = stdout(&output);
    // Grouped as in spec §2.1–2.7: command lines are indented two spaces under a group heading.
    let commands: Vec<_> = help
        .lines()
        .filter(|line| line.starts_with("  ") && line.as_bytes().get(2).is_some_and(u8::is_ascii_lowercase))
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(
        commands,
        ["status", "needs", "since", "overview", "show", "ls", "history", "watch", "open",
            "report", "signal", "ask", "wait", "reply", "propose", "frame",
            "answer", "pause", "resume", "hub",
            "task", "idea", "decision", "rule",
            "ext", "template",
            "init", "up", "daemon", "snapshot", "rebuild-index", "doctor", "whoami", "schema", "completion", "help",
            "dev"]
    );
    let output = towermd(directory.path(), &["init", "--help"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("--docs-dir"));
    assert!(!directory.path().join(".towermd").exists());
}

#[test]
fn approval_commands_are_rejected() {
    let repo = git_repo();
    for command in ["approve", "authorize", "review", "waive", "accept", "confirm", "finalize", "send-back"] {
        let output = towermd(repo.path(), &[command]);
        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("towermd: "));
        assert!(output.stdout.is_empty());
    }
    assert!(!repo.path().join(".towermd").exists());
}

#[test]
fn invalid_arguments_fail_on_stderr() {
    let repo = git_repo();
    for args in [
        vec!["init", "--docs-dir"],
        vec!["init", "--approve"],
    ] {
        let output = towermd(repo.path(), &args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("towermd: "));
        assert!(output.stdout.is_empty());
    }
    assert!(!repo.path().join(".towermd").exists());
}

#[test]
fn noninteractive_new_init_without_docs_dir_fails_without_any_writes() {
    let repo = git_repo();
    let before: Vec<_> = fs::read_dir(repo.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    let output = towermd_with_open_stdin(repo.path(), &["init"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("非交互模式须显式传 --docs-dir，请先与人确认目录"));
    let after: Vec<_> = fs::read_dir(repo.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(after, before);
    assert!(!repo.path().join(".towermd").exists());
    assert!(!repo.path().join("docs").exists());
    assert!(!repo.path().join(".git/index").exists());
    assert!(!repo.path().join(".git/towermd").exists());
}

#[test]
fn explicit_init_does_not_wait_for_open_stdin() {
    let repo = git_repo();
    let output = towermd_with_open_stdin(repo.path(), &["init", "--docs-dir", "docs/plan"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("用户文档目录：docs/plan"), "{text}");
    assert!(!text.contains("用户文档目录（相对仓库根）"), "{text}");
    assert!(repo.path().join(".towermd/config.yaml").is_file());
}

#[test]
fn reinit_does_not_wait_for_open_stdin() {
    let repo = git_repo();
    assert!(towermd(repo.path(), &["init", "--docs-dir", "docs/plan"])
        .status
        .success());
    let output = towermd_with_open_stdin(repo.path(), &["init"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("沿用已有"), "{text}");
    assert!(text.contains("用户文档目录：docs/plan"), "{text}");
    assert!(!text.contains("用户文档目录（相对仓库根）"), "{text}");
}

#[test]
fn existing_agents_files_emit_warning_and_hint_without_overwrite() {
    let repo = git_repo();
    fs::create_dir(repo.path().join(".towermd")).unwrap();
    fs::write(repo.path().join("AGENTS.md"), "# 我的规矩\n").unwrap();
    fs::write(repo.path().join(".towermd/AGENTS.md"), "# 已有项目规矩\n").unwrap();
    let output = towermd(repo.path(), &["init", "--docs-dir", "docs/tower"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(
        text.lines()
            .any(|line| line.starts_with("警告：") && line.contains(".towermd/AGENTS.md")),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.starts_with("提示：") && line.contains(".towermd/AGENTS.md")),
        "{text}"
    );
    assert_eq!(
        text.lines().last(),
        Some("以上文件未提交，请检查后自行提交。")
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("AGENTS.md")).unwrap(),
        "# 我的规矩\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join(".towermd/AGENTS.md")).unwrap(),
        "# 已有项目规矩\n"
    );
}

#[test]
fn existing_config_is_preserved_byte_for_byte() {
    let repo = git_repo();
    fs::create_dir(repo.path().join(".towermd")).unwrap();
    let config = "# 人工配置\nproject_id: existing-project\ndocs_dir: docs/custom\nhub: local\ncustom: { enabled: true }\n";
    fs::write(repo.path().join(".towermd/config.yaml"), config).unwrap();
    let output = towermd(repo.path(), &["init"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(
        text.contains("项目 ID：existing-project（沿用已有）"),
        "{text}"
    );
    assert!(text.contains("用户文档目录：docs/custom"), "{text}");
    assert_eq!(
        fs::read(repo.path().join(".towermd/config.yaml")).unwrap(),
        config.as_bytes()
    );
}

#[test]
fn malformed_config_fails_without_overwrite() {
    let repo = git_repo();
    fs::create_dir(repo.path().join(".towermd")).unwrap();
    let config = "project_id: [\n";
    fs::write(repo.path().join(".towermd/config.yaml"), config).unwrap();
    let output = towermd(repo.path(), &["init"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("towermd: "));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read(repo.path().join(".towermd/config.yaml")).unwrap(),
        config.as_bytes()
    );
}

#[test]
fn closed_stdout_reports_error_without_panicking() {
    let repo = git_repo();
    for args in [
        vec!["init", "--docs-dir", "docs/tower"],
        vec!["--help"],
        vec!["init", "--help"],
    ] {
        let (reader, writer) = io::pipe().unwrap();
        drop(reader);
        let output = towermd_command(repo.path(), &args)
            .stdout(writer)
            .output()
            .unwrap();
        let error = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {error}");
        assert!(error.starts_with("towermd: "), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}

#[test]
fn closed_stderr_keeps_the_exit_code() {
    let directory = tempfile::tempdir().unwrap();
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);
    let output = towermd_command(directory.path(), &["init"])
        .stderr(writer)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
}
