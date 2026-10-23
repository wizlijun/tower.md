//! FR-80 Note.md writeback through the real daemon: nothing without a vault
//! on main, a silent baseline when one is connected, then one plain `.md`
//! summary per newly accepted decision, never overwritten.
#[path = "support/envelope.rs"]
mod envelope;

use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Note Test", "-c", "user.email=note@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().into()
}

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
    vault: tempfile::TempDir,
}
impl Fixture {
    fn output(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
            .args(args).current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null()).output().unwrap()
    }
    fn post(&self, endpoint: &Value, path: &str, body: Value) -> Value {
        let response = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap()
            .post(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", "app").json(&body).send().unwrap();
        let status = response.status();
        let result: Value = response.json().unwrap();
        assert!(status.is_success(), "{path}: {status} {result}");
        result
    }
    fn commit(&self, path: &str, text: &str) -> String {
        let target = self.root.path().join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, text).unwrap();
        git(self.root.path(), &["add", "."]);
        git(self.root.path(), &["commit", "-qm", path]);
        git(self.root.path(), &["rev-parse", "HEAD"])
    }
    fn vault_files(&self) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![self.vault.path().to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.file_name().unwrap() == ".git" { continue; }
                if path.is_dir() { stack.push(path) } else {
                    found.push(path.strip_prefix(self.vault.path()).unwrap().to_string_lossy().into_owned());
                }
            }
        }
        found.sort();
        found
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(&["daemon", "stop"]);
    }
}

fn decision(id: &str) -> String {
    format!("---\ntype: Decision\nid: {id}\nlayer: L4\nstatus: accepted\nquestion: 用哪种共识？\ndecision: 用 Raft\nsources:\n  - {{ id: n1, resource: kb/raft.md, okf_id: kb/raft, block: b-00ff00 }}\n---\n用 Raft\n")
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(40);
    while !path.exists() {
        assert!(Instant::now() < deadline, "daemon never wrote {}", path.display());
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Longer than one 10 s maintenance tick.
fn one_tick() {
    std::thread::sleep(Duration::from_secs(12));
}

#[test]
fn daemon_writes_plain_md_summaries_only_into_a_configured_vault() {
    let fixture = Fixture { root: tempfile::tempdir().unwrap(), runtime: tempfile::tempdir().unwrap(), vault: tempfile::tempdir().unwrap() };
    let root = fixture.root.path();
    git(root, &["init", "-qb", "main"]);
    fixture.commit(".towermd/config.yaml", "project_id: note-project\ndocs_dir: docs/tower\nhub: local\n");
    fixture.commit("docs/tower/decisions/D-old.md", &decision("D-old"));
    let vault = fixture.vault.path();
    fs::write(vault.join("existing.md"), "keep me\n").unwrap();
    git(vault, &["init", "-qb", "main"]);
    git(vault, &["add", "."]);
    git(vault, &["commit", "-qm", "vault"]);
    let vault_head = git(vault, &["rev-parse", "HEAD"]);

    let ensured = fixture.output(&["daemon", "ensure"]);
    assert!(ensured.status.success(), "{}", String::from_utf8_lossy(&ensured.stderr));
    let endpoint = envelope::data_checked(&["daemon", "ensure"], &ensured);
    let opened = fixture.post(&endpoint, "/projects/open", json!({"path": root}));
    fixture.post(&endpoint, &format!("/projects/{}/snapshot", opened["handle"].as_str().unwrap()), json!({}));

    // Not connected: no ledger, nothing in the vault.
    one_tick();
    let ledgers = fixture.runtime.path().join("notemd");
    assert!(!ledgers.exists());
    assert_eq!(fixture.vault_files(), vec!["existing.md"]);

    // Connecting starts a silent baseline: the old decision is not dumped.
    fixture.commit(".towermd/config.yaml", &format!("project_id: note-project\ndocs_dir: docs/tower\nhub: local\nnotemd:\n  vault: {}\n", vault.display()));
    let deadline = Instant::now() + Duration::from_secs(40);
    while fs::read_dir(&ledgers).map(|entries| entries.count()).unwrap_or(0) == 0 {
        assert!(Instant::now() < deadline, "writeback never started");
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(fixture.vault_files(), vec!["existing.md"]);

    // A newly accepted decision becomes one plain OKF summary.
    let commit = fixture.commit("docs/tower/decisions/D-new.md", &decision("D-new"));
    let summary: PathBuf = vault.canonicalize().unwrap().join("inbox/towermd/decision-D-new.md");
    wait_for(&summary);
    std::thread::sleep(Duration::from_millis(200));
    let text = fs::read_to_string(&summary).unwrap();
    let document = towermd_core::okf::Document::parse(&text).unwrap();
    let field = |key: &str| document.get(key).unwrap().clone();
    assert_eq!(document.doc_type(), Some("Decision Summary"));
    assert_eq!(field("generated")["by"].as_str(), Some("process:towermd"));
    let sources = field("sources");
    assert_eq!(sources[0]["resource"].as_str(), Some(format!("git:{commit}").as_str()));
    assert_eq!(sources[1]["resource"].as_str(), Some("towermd:note-project/D-new"));
    assert_eq!(sources[2]["okf_id"].as_str(), Some("kb/raft"));
    assert_eq!(sources[2]["block"].as_str(), Some("b-00ff00"));
    assert!(document.body().contains("`((kb/raft.md#b-00ff00))`"));
    assert_eq!(fixture.vault_files(), vec!["existing.md", "inbox/towermd/decision-D-new.md"]);

    // Idempotent and hands-off: a person's edit stays, nothing else appears,
    // existing notes and the vault's git are untouched.
    fs::write(&summary, "triaged by a person\n").unwrap();
    one_tick();
    assert_eq!(fs::read_to_string(&summary).unwrap(), "triaged by a person\n");
    assert_eq!(fixture.vault_files(), vec!["existing.md", "inbox/towermd/decision-D-new.md"]);
    assert_eq!(fs::read_to_string(vault.join("existing.md")).unwrap(), "keep me\n");
    assert_eq!(git(vault, &["rev-parse", "HEAD"]), vault_head);
    assert_eq!(git(vault, &["status", "--porcelain"]), "?? inbox/");
}
