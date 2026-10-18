use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    storage: tempfile::TempDir,
    runtime: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            storage: tempfile::tempdir().unwrap(),
            runtime: tempfile::tempdir().unwrap(),
        };
        fs::create_dir(f.root()).unwrap();
        f.git(&f.root(), &["init", "-qb", "main"]);
        f.success(&f.root(), &["init", "--docs-dir", "docs/tower"]);
        fs::create_dir_all(f.root().join("src/a")).unwrap();
        fs::write(f.root().join("src/a/code.rs"), "original\n").unwrap();
        // Explicit isolated authority fixture, not a human approval producer.
        let verify = format!("touch '{}'; exit 91", f.marker().display());
        fs::write(
            f.root().join("docs/tower/arch/A.md"),
            format!("---\ntype: Module\nid: A\npaths: [src/a/**]\nverify: [{{cwd: '.', argv: ['/bin/sh', '-c', {}]}}]\n---\n", serde_json::to_string(&verify).unwrap()),
        ).unwrap();
        fs::write(
            f.root().join("docs/tower/arch/B.md"),
            "---\ntype: Module\nid: B\npaths: [src/b/**]\nverify: []\n---\n",
        )
        .unwrap();
        fs::write(
            f.root().join("docs/tower/specs/AC-A.md"),
            "---\ntype: Acceptance\nid: AC-A\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        f.git(&f.root(), &["add", "."]);
        f.git(
            &f.root(),
            &["commit", "-qm", "isolated authoritative Task fixture"],
        );
        f.success(
            &f.root(),
            &[
                "task",
                "add",
                "--id",
                "T",
                "--title",
                "Check real commit",
                "--module",
                "A",
                "--touches",
                "B",
                "--allow",
                "src/a/**",
                "--acceptance",
                "AC-A",
            ],
        );
        f
    }
    fn root(&self) -> PathBuf {
        self.storage.path().join("repo")
    }
    fn marker(&self) -> PathBuf {
        self.runtime.path().join("verify-must-not-run")
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
    fn git(&self, directory: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(directory)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .into()
    }
    fn claim(&self) -> (PathBuf, Value) {
        let worktree = self.storage.path().join("task-worktree");
        self.git(
            &self.root(),
            &[
                "worktree",
                "add",
                "-qb",
                "t/T",
                worktree.to_str().unwrap(),
                "main",
            ],
        );
        let run = self.json(&worktree, &["task", "claim", "T"]);
        assert_eq!(run["leases"].as_array().unwrap().len(), 2);
        (worktree, run)
    }
    fn commit(&self, worktree: &Path, path: &str, text: &str) {
        let file = worktree.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        self.git(worktree, &["add", "--", path]);
        self.git(worktree, &["commit", "-qm", "actual committed change"]);
    }
    fn checkpoint(&self, worktree: &Path) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let value = self.json(worktree, &["snapshot"]);
            if value["status"] != "deferred" {
                assert!(
                    value["status"] == "written" || value["status"] == "unchanged",
                    "{value}"
                );
                return;
            }
            assert_eq!(value["reason"], "快照写入正在进行");
            assert!(
                Instant::now() < deadline,
                "writer did not become idle: {value}"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn outbox(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(base: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    visit(base, &path, files);
                } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                    files.insert(
                        path.strip_prefix(base).unwrap().into(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }
        let base = self.runtime.path().join("recovery-outbox");
        let mut files = BTreeMap::new();
        visit(&base, &base, &mut files);
        assert!(
            !files.is_empty(),
            "real add/claim must have a durable outbox"
        );
        files
    }
    fn reject(&self, worktree: &Path, args: &[&str], diagnostic: &str) {
        let refs = self.git(&self.root(), &["show-ref"]);
        let outbox = self.outbox();
        let status = self.git(
            worktree,
            &["status", "--porcelain=v2", "--untracked-files=all"],
        );
        let index_path = worktree.join(self.git(worktree, &["rev-parse", "--git-path", "index"]));
        let index = fs::read(&index_path).unwrap();
        let output = self.command(worktree, args).output().unwrap();
        assert!(
            !output.status.success(),
            "rejected check unexpectedly succeeded"
        );
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(self.git(&self.root(), &["show-ref"]), refs);
        assert_eq!(self.outbox(), outbox);
        assert_eq!(
            self.git(
                worktree,
                &["status", "--porcelain=v2", "--untracked-files=all"]
            ),
            status
        );
        assert_eq!(fs::read(index_path).unwrap(), index);
        assert!(!self.marker().exists());
    }
    fn assert_no_check_evidence(&self) {
        // Stop first so reading raw journal bytes cannot race an append.
        self.success(&self.root(), &["daemon", "stop"]);
        let text = fs::read_to_string(self.root().join(".git/towermd/journal.jsonl")).unwrap();
        for (n, line) in text.lines().enumerate() {
            let value: Value = serde_json::from_str(line).unwrap();
            if n == 0 {
                let evidence =
                    &value["checkpoint"]["checkpoint"]["projection"]["objects"]["Evidence"];
                assert!(evidence.is_null() || evidence.as_object().is_some_and(|v| v.is_empty()));
                continue;
            }
            let kind = value["type"].as_str().unwrap();
            assert!(
                !kind.starts_with("check.")
                    && !kind.starts_with("finish.")
                    && kind != "evidence.recorded",
                "{value}"
            );
            assert!(!value["actor"].as_str().unwrap().starts_with("human:"));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if thread::panicking() {
            if let Ok(log) = fs::read_to_string(self.runtime.path().join("daemon.log")) {
                eprintln!("daemon log:\n{log}");
            }
        }
        let _ = self.command(&self.root(), &["daemon", "stop"]).output();
    }
}

#[test]
fn cli_check_reports_actual_commit_without_verification_or_business_write() {
    let f = Fixture::new();
    let (worktree, run) = f.claim();
    f.commit(&worktree, "src/a/code.rs", "actual changed code\n");
    f.checkpoint(&worktree);
    let refs = f.git(&f.root(), &["show-ref"]);
    let outbox = f.outbox();
    let index_path = worktree.join(f.git(&worktree, &["rev-parse", "--git-path", "index"]));
    let index = fs::read(&index_path).unwrap();
    let report = f.json(&worktree, &["check", "--run", run["id"].as_str().unwrap()]);
    assert_eq!(report["commit"], f.git(&worktree, &["rev-parse", "HEAD"]));
    assert_eq!(
        report["tree"],
        f.git(&worktree, &["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(report["base"], f.git(&f.root(), &["rev-parse", "main"]));
    assert_eq!(
        report["gates"],
        json!({"G0":true,"G2":true,"G3":false,"G5":true})
    );
    assert_eq!(report["changed"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["changed"][0]["path"],
        json!(b"src/a/code.rs".to_vec())
    );
    assert_eq!(report["changed"][0]["status"], b'M');
    assert_eq!(report["changed"][0]["old_mode"], 0o100644);
    assert_eq!(report["changed"][0]["new_mode"], 0o100644);
    assert_eq!(
        report["changed"][0]["old_oid"],
        f.git(&worktree, &["rev-parse", "main:src/a/code.rs"])
    );
    assert_eq!(
        report["changed"][0]["new_oid"],
        f.git(&worktree, &["rev-parse", "HEAD:src/a/code.rs"])
    );
    assert_eq!(f.json(&worktree, &["check"]), report);
    assert_eq!(f.git(&f.root(), &["show-ref"]), refs);
    assert_eq!(f.outbox(), outbox);
    assert_eq!(fs::read(index_path).unwrap(), index);
    assert_eq!(
        f.git(
            &worktree,
            &["status", "--porcelain=v2", "--untracked-files=all"]
        ),
        ""
    );
    assert!(!f.marker().exists());
    f.assert_no_check_evidence();
}

#[test]
fn cli_check_rejects_dirty_protected_and_outside_task_commits() {
    for case in ["dirty", "protected", "outside"] {
        let f = Fixture::new();
        let (worktree, _) = f.claim();
        match case {
            "dirty" => fs::write(worktree.join("src/a/untracked.rs"), "keep dirty\n").unwrap(),
            "protected" => f.commit(
                &worktree,
                "docs/tower/arch/A.md",
                "---\ntype: Module\nid: A\npaths: [src/a/**]\n---\nchanged slow layer\n",
            ),
            _ => f.commit(&worktree, "src/b/outside.rs", "outside Task allow\n"),
        }
        f.checkpoint(&worktree);
        f.reject(
            &worktree,
            &["check"],
            match case {
                "dirty" => "Dirty",
                "protected" => "Protected",
                _ => "OutsideTask",
            },
        );
        f.assert_no_check_evidence();
    }
}

#[test]
fn cli_check_rejects_unbound_runs_and_client_authority_fields() {
    let f = Fixture::new();
    f.checkpoint(&f.root());
    f.reject(&f.root(), &["check"], "no active run");
    let (worktree, _) = f.claim();
    f.checkpoint(&worktree);
    f.reject(&worktree, &["check", "--run", "not-this-run"], "local runs");
    let endpoint: Value =
        serde_json::from_slice(&fs::read(f.runtime.path().join("endpoint.json")).unwrap()).unwrap();
    let address = format!(
        "http://127.0.0.1:{}/check",
        endpoint["port"].as_u64().unwrap()
    );
    let outbox = f.outbox();
    let refs = f.git(&f.root(), &["show-ref"]);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    for key in ["actor", "fences", "commands", "evidence"] {
        let mut body = json!({"directory":worktree,"run":null});
        body[key] = json!("human:injected");
        let response = client
            .post(&address)
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .json(&body)
            .send()
            .unwrap();
        assert!(
            response.status().is_client_error(),
            "accepted unexpected field {key}"
        );
    }
    let response = client
        .post(&address)
        .header("x-towermd-instance", "another-instance")
        .json(&json!({"directory":worktree,"run":null}))
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(f.outbox(), outbox);
    assert_eq!(f.git(&f.root(), &["show-ref"]), refs);
    assert!(!f.marker().exists());
    f.assert_no_check_evidence();
}
