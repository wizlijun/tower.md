#[path = "support/envelope.rs"]
mod envelope;

#[path = "support/golden.rs"]
mod golden;
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Task Test",
            "-c",
            "user.email=task@example.invalid",
            "-c",
            "commit.gpgsign=false",
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
impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            runtime: tempfile::tempdir().unwrap(),
        };
        let root = fixture.root.path();
        git(root, &["init", "-qb", "main"]);
        fs::create_dir(root.join(".towermd")).unwrap();
        fs::write(
            root.join(".towermd/config.yaml"),
            "project_id: tasks-project\ndocs_dir: docs/tower\nhub: local\n",
        )
        .unwrap();
        for name in ["alpha", "beta", "gamma"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::write(root.join(name).join("code.rs"), "fn task() {}\n").unwrap();
        }
        fs::write(
            root.join("Makefile"),
            "test:\n\t@printf 'real task fixture verification\\n'\n",
        )
        .unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "fixture"]);
        fixture
    }
    fn output(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
            .args(args)
            .current_dir(cwd)
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
    fn run(&self, cwd: &Path, args: &[&str]) -> Value {
        let output = self.output(cwd, args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        envelope::data_checked(args, &output)
    }
    fn post(&self, endpoint: &Value, path: &str, body: Value) -> Value {
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap()
            .post(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", "app")
            .json(&body)
            .send()
            .unwrap();
        let status = response.status();
        let result: Value = response.json().unwrap();
        assert!(status.is_success(), "{path}: {status} {result}");
        result
    }
    fn get(&self, endpoint: &Value, path: &str) -> Value {
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", "app")
            .send()
            .unwrap();
        let status = response.status();
        let result: Value = response.json().unwrap();
        assert!(status.is_success(), "{path}: {status} {result}");
        result
    }
    fn publish(&self) -> Value {
        let root = self.root.path();
        let endpoint = self.run(root, &["daemon", "ensure"]);
        let opened = self.post(&endpoint, "/projects/open", json!({"path":root}));
        let handle = opened["handle"].as_str().unwrap();
        self.post(
            &endpoint,
            &format!("/projects/{handle}/snapshot"),
            json!({}),
        );
        let scan = self.run(root, &["dev", "scan", "start"]);
        let scan_id = scan["id"].as_str().unwrap();
        // Exit 5 means the scan is still running after one wait window.
        let ready = loop {
            let output = self.output(root, &["dev", "scan", "wait", scan_id]);
            if output.status.code() != Some(5) {
                break output;
            }
        };
        assert!(ready.status.success(), "{}", String::from_utf8_lossy(&ready.stderr));
        let template = self.run(root, &["dev", "draft", "template", scan_id]);
        let path = self.runtime.path().join("input.json");
        fs::write(&path, serde_json::to_vec(&template).unwrap()).unwrap();
        let mut draft = self.run(
            root,
            &[
                "dev",
                "draft",
                "submit",
                path.to_str().unwrap(),
                "--producer",
                "test/1",
            ],
        );
        for module in draft["scan"]["modules"].as_array().unwrap().clone() {
            draft=self.post(&endpoint,&format!("/projects/{handle}/baselines/{}/review",draft["id"].as_str().unwrap()),json!({"action":{"kind":"commands","module_id":module["id"],"commands":[{"directory":"","command":"printf 'task baseline proof\\n'"}]}}));
        }
        let request = uuid::Uuid::now_v7().to_string();
        self.post(
            &endpoint,
            &format!(
                "/projects/{handle}/baselines/{}/verify",
                draft["id"].as_str().unwrap()
            ),
            json!({"request_id":request}),
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        let ready = loop {
            let job = self.get(
                &endpoint,
                &format!("/projects/{handle}/baseline-verifications/{request}"),
            );
            if job["status"] != "running" {
                break job;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(ready["status"], "ready", "{ready}");
        draft = self.get(
            &endpoint,
            &format!(
                "/projects/{handle}/baselines/{}",
                ready["result_id"].as_str().unwrap()
            ),
        );
        git(root, &["checkout", "-qb", "qa-owner"]);
        let path = format!(
            "/projects/{handle}/baselines/{}/publication",
            draft["id"].as_str().unwrap()
        );
        let prepared = self.post(&endpoint, &format!("{path}/prepare"), json!({}));
        self.post(&endpoint,&format!("{path}/approve"),json!({"request_id":prepared["request_id"],"target":prepared["target"],"prepared_at":prepared["prepared_at"],"content_hash":prepared["content_hash"]}));
        draft
    }
    fn task(&self, id: &str, module: &str, path: &str) -> std::path::PathBuf {
        let target = self.runtime.path().join(format!("{id}.json"));
        fs::write(&target,serde_json::to_vec(&json!({"id":id,"title":format!("Task {id}"),"module":module,"allowed_paths":[path],"acceptance":["existing smoke test passes"]})).unwrap()).unwrap();
        target
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(self.root.path(), &["daemon", "stop"]);
    }
}

#[test]
fn task_lifecycle_binds_worktrees_renews_and_serializes_module_ownership() {
    let fixture = Fixture::new();
    let draft = fixture.publish();
    let root = fixture.root.path();
    let module = draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path = draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let one = fixture.task("T-one", module, path);
    let two = fixture.task("T-two", module, path);
    let create_id = uuid::Uuid::now_v7().to_string();
    let created = fixture.run(
        root,
        &[
            "task",
            "add",
            one.to_str().unwrap(),
            "--request-id",
            &create_id,
        ],
    );
    assert_eq!(created["event"]["actor"], "process:towermd-cli");
    assert_eq!(created["task"]["state"], "ready");
    let retried = fixture.run(
        root,
        &[
            "task",
            "add",
            one.to_str().unwrap(),
            "--request-id",
            &create_id,
        ],
    );
    assert_eq!(retried["event"]["eid"], created["event"]["eid"]);
    fixture.run(root, &["task", "add", two.to_str().unwrap()]);
    let bad = fixture.task("T-outside", module, "unowned.rs");
    assert!(!fixture
        .output(root, &["task", "add", bad.to_str().unwrap()])
        .status
        .success());
    let trees = tempfile::tempdir().unwrap();
    let first = trees.path().join("first");
    let second = trees.path().join("second");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "t/T-one",
            first.to_str().unwrap(),
            "main",
        ],
    );
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "t/T-two",
            second.to_str().unwrap(),
            "main",
        ],
    );
    let request = uuid::Uuid::now_v7().to_string();
    let claimed = fixture.run(
        &first,
        &["task", "claim", "T-one", "--request-id", &request],
    );
    assert_eq!(claimed["event"]["actor"], "process:towermd-cli");
    assert_eq!(claimed["run"]["cost_usd"], Value::Null);
    let run_id = claimed["run"]["binding"]["id"].as_str().unwrap();
    assert_eq!(
        fixture.run(
            &first,
            &["task", "claim", "T-one", "--request-id", &request]
        )["event"]["eid"],
        claimed["event"]["eid"]
    );
    assert_eq!(
        fixture.run(&first, &["task", "show"])["task"]["input"]["id"],
        "T-one"
    );
    assert_eq!(
        fixture.run(&first, &["status"])["current_run"]["binding"]["id"],
        run_id
    );
    assert_eq!(
        fixture.run(root, &["status", "--run", run_id])["current_run"]["binding"]["id"],
        run_id
    );
    assert!(!fixture
        .output(&second, &["task", "claim", "T-two"])
        .status
        .success());
    let release = uuid::Uuid::now_v7().to_string();
    let released = fixture.run(&first, &["task", "release", "--request-id", &release]);
    assert_eq!(released["task"]["state"], "ready");
    assert_eq!(
        fixture.run(&first, &["task", "release", "--request-id", &release])["event"]["eid"],
        released["event"]["eid"]
    );
    let second_claim = fixture.run(&second, &["task", "claim", "T-two"]);
    fixture.run(root, &["daemon", "stop"]);
    assert_eq!(
        fixture.run(&second, &["task", "show"])["current_run"]["binding"]["id"],
        second_claim["run"]["binding"]["id"]
    );
    fixture.run(&second, &["task", "release"]);
    assert_eq!(
        fixture.run(&second, &["status"])["current_run"],
        Value::Null
    );
}

#[test]
fn a_bare_baseline_tag_does_not_admit_a_task_or_offer_cli_approval() {
    let fixture = Fixture::new();
    let root = fixture.root.path();
    git(root, &["tag", "towermd/B0"]);
    let path = fixture.task("T-unapproved", "A-alpha", "alpha/code.rs");
    let failed = fixture.output(root, &["task", "add", path.to_str().unwrap()]);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("G0"));
    assert_eq!(envelope::code(&failed), "gate_failed");
    for args in [&["task", "approve"][..], &["dev", "draft", "approve"]] {
        let output = fixture.output(root, args);
        assert!(!output.status.success());
        assert_eq!(envelope::code(&output), "usage");
    }
}

#[test]
fn finish_persists_real_report_survives_restart_and_only_main_admission_merges_task() {
    let fixture=Fixture::new();
    let draft=fixture.publish();
    let root=fixture.root.path();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let input=fixture.task("T-finish",module,path);
    fixture.run(root,&["task","add",input.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();
    let worktree=trees.path().join("task");
    git(root,&["worktree","add","-b","t/T-finish",worktree.to_str().unwrap(),"main"]);
    fixture.run(&worktree,&["task","claim","T-finish"]);
    fs::write(worktree.join(path),"fn task() { /* implemented */ }\n").unwrap();
    git(&worktree,&["add",path]);
    git(&worktree,&["commit","-qm","task implementation"]);
    assert_eq!(fixture.run(&worktree,&["dev","check"])["passed"],true);
    let request=uuid::Uuid::now_v7().to_string();
    let queued=fixture.run(&worktree,&["dev","finish","--request-id",&request]);
    let evidence=queued["evidence_id"].as_str().unwrap();
    assert_eq!(queued["status"],"queued");
    let finished=fixture.run(&worktree,&["dev","finish","--wait",evidence]);
    assert_eq!(finished["status"],"passed","{finished}");
    assert_eq!(finished["evidence"]["report"]["actor"],"process:towermd-verifier");
    assert!(finished["evidence"]["report"]["commands"][0]["argv"].as_array().unwrap().len()>=3);
    assert_eq!(fixture.run(&worktree,&["task","show"])["task"]["state"],"verifying");
    fixture.run(root,&["daemon","stop"]);
    let restored=fixture.run(&worktree,&["dev","finish","--wait",evidence]);
    assert_eq!(restored["evidence"],finished["evidence"]);
    assert_eq!(fixture.run(&worktree,&["dev","finish","--request-id",&request])["evidence_id"],evidence);
    let gate_request=uuid::Uuid::now_v7().to_string();
    let trailers=fixture.output(&worktree,&["--text","dev","trailers","T-finish","--request-id",&gate_request]);
    assert!(trailers.status.success(),"{}",String::from_utf8_lossy(&trailers.stderr));
    let text=String::from_utf8(trailers.stdout).unwrap();
    assert_eq!(text.lines().count(),5);
    assert!(text.contains(&format!("Towermd-Evidence: {evidence}")));
    git(&worktree,&["commit","--amend","-qm",&format!("task implementation\n\n{text}")]);
    let tip=git(&worktree,&["rev-parse","HEAD"]);
    git(root,&["update-ref","refs/heads/main",&tip]);
    let deadline=Instant::now()+Duration::from_secs(35);
    loop{
        let task=fixture.run(root,&["task","show","T-finish"]);
        if task["task"]["state"]=="merged"{break;}
        assert!(Instant::now()<deadline,"main admission did not certify task: {task}");
        std::thread::sleep(Duration::from_millis(500));
    }
    let accepted=fixture.run(&worktree,&["dev","finish","--wait",evidence]);
    assert_eq!(accepted["status"],"merged");
    assert_eq!(accepted["merge"]["admission_commit"],tip);
    assert_eq!(accepted["merge"]["tree"],git(&worktree,&["rev-parse","HEAD^{tree}"]));
    golden::record(root, "finish");
}

#[test]
fn finish_rejects_dirty_and_protected_edits_and_keeps_task_unmerged() {
    let fixture=Fixture::new();
    let draft=fixture.publish();
    let root=fixture.root.path();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let input=fixture.task("T-guarded",module,path);
    fixture.run(root,&["task","add",input.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();
    let worktree=trees.path().join("task");
    git(root,&["worktree","add","-b","t/T-guarded",worktree.to_str().unwrap(),"main"]);
    fixture.run(&worktree,&["task","claim","T-guarded"]);
    fs::write(worktree.join(path),"fn task() { /* dirty */ }\n").unwrap();
    assert!(!fixture.output(&worktree,&["dev","check"]).status.success());
    let dirty=fixture.run(&worktree,&["dev","finish"]);
    let interrupted=fixture.output(&worktree,&["dev","finish","--wait",dirty["evidence_id"].as_str().unwrap()]);
    assert_eq!(envelope::code(&interrupted),"gate_failed");
    let interrupted=envelope::parse(&interrupted)["data"].clone();
    assert_eq!(interrupted["status"],"interrupted");
    assert!(interrupted["error"].as_str().unwrap().contains("未提交"));
    git(&worktree,&["add",path]);
    git(&worktree,&["commit","-qm","allowed checkpoint"]);
    fs::write(worktree.join("docs/tower/tower.canvas"),"{}\n").unwrap();
    git(&worktree,&["add","docs/tower/tower.canvas"]);
    git(&worktree,&["commit","-qm","forbidden canvas edit"]);
    assert!(!fixture.output(&worktree,&["dev","check"]).status.success());
    let forbidden=fixture.run(&worktree,&["dev","finish"]);
    let rejected=fixture.output(&worktree,&["dev","finish","--wait",forbidden["evidence_id"].as_str().unwrap()]);
    assert_eq!(envelope::code(&rejected),"gate_failed");
    let rejected=envelope::parse(&rejected)["data"].clone();
    assert_eq!(rejected["status"],"interrupted");
    assert!(rejected["error"].as_str().unwrap().contains("G2"));
    assert_eq!(fixture.run(&worktree,&["task","show"])["task"]["state"],"running");
    assert!(!fixture.output(&worktree,&["dev","trailers","T-guarded"]).status.success());
    assert!(!fixture.output(&worktree,&["dev","finish","--state","done"]).status.success());
}
