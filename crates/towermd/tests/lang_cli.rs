//! FR-197: daemon errors follow the caller's language; without a language the daemon keeps Chinese.
#[path = "support/envelope.rs"]
mod envelope;

use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::Duration,
};

fn towermd(cwd: &Path, runtime: &Path, lang: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_towermd"))
        .args(args)
        .current_dir(cwd)
        .env("TOWERMD_RUNTIME_DIR", runtime)
        .env("TOWERMD_LANG", lang)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn project() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    fs::create_dir(repo.path().join(".towermd")).unwrap();
    fs::write(repo.path().join(".towermd/config.yaml"), "project_id: lang-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "root"]);
    repo
}

struct Stop<'a>(&'a Path, &'a Path);
impl Drop for Stop<'_> {
    fn drop(&mut self) {
        let _ = towermd(self.0, self.1, "en", &["daemon", "stop"]);
    }
}

#[test]
fn daemon_error_bodies_follow_accept_language() {
    let repo = project();
    let runtime = tempfile::tempdir().unwrap();
    let endpoint = envelope::data(&towermd(repo.path(), runtime.path(), "en", &["daemon", "ensure"]));
    let _stop = Stop(repo.path(), runtime.path());
    // Right instance, no client type: the daemon refuses with its own message.
    let refuse = |language: Option<&str>| -> Value {
        let mut request = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap()
            .post(format!("http://127.0.0.1:{}/projects/open", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .json(&serde_json::json!({"path": repo.path()}));
        if let Some(language) = language {
            request = request.header("accept-language", language);
        }
        let response = request.send().unwrap();
        assert_eq!(response.status(), 409);
        response.json().unwrap()
    };
    let chinese = serde_json::json!({"error": "守护进程实例或客户端类型不匹配", "code": "conflict"});
    assert_eq!(refuse(None), chinese, "no Accept-Language keeps the App's Chinese");
    assert_eq!(refuse(Some("zh-CN")), chinese);
    let english = refuse(Some("en"));
    assert_eq!(english["code"], "conflict", "codes never change");
    assert_eq!(english["error"], "daemon instance or client type does not match");
}

#[test]
fn cli_envelope_errors_from_the_daemon_follow_the_language() {
    let repo = project();
    let runtime = tempfile::tempdir().unwrap();
    let _stop = Stop(repo.path(), runtime.path());
    let english = envelope::parse(&towermd(repo.path(), runtime.path(), "en", &["task", "show", "T-nope"]));
    assert_eq!(english["error"]["code"], "not_found");
    assert_eq!(english["error"]["message"], "task not found");
    let chinese = envelope::parse(&towermd(repo.path(), runtime.path(), "zh_CN.UTF-8", &["task", "show", "T-nope"]));
    assert_eq!(chinese["error"]["code"], "not_found");
    assert_eq!(chinese["error"]["message"], "任务不存在");
    // A message composed from several Chinese parts comes out English as a whole.
    let composed = envelope::parse(&towermd(repo.path(), runtime.path(), "en", &["dev", "check"]));
    let message = composed["error"]["message"].as_str().unwrap();
    assert!(message.starts_with("no active run for the current worktree and branch"), "{message}");
    assert!(!message.chars().any(|c| matches!(c, '\u{3000}'..='\u{303f}' | '\u{4e00}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}')), "{message}");
}
