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
    /// Extra environment for every call; the daemon inherits it at start.
    env: Vec<(&'static str, String)>,
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
            env: Vec::new(),
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
            .envs(self.env.iter().map(|(key, value)| (*key, value)))
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
    let card = claimed["task_card"]["markdown"].as_str().unwrap();
    assert!(card.starts_with("# 任务卡 T-one") && card.contains("## 禁止事项") && card.contains("towermd dev finish --report"), "{card}");
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
    assert_eq!(fixture.run(&first, &["task", "show"])["task_card"]["markdown"], claimed["task_card"]["markdown"]);
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
fn tasks_are_listed_edited_shown_and_claimed_without_an_id_only_when_unambiguous() {
    let fixture = Fixture::new();
    let draft = fixture.publish();
    let root = fixture.root.path();
    let module = draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path = draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    for id in ["T-one", "T-two"] {
        fixture.run(root, &["task", "add", fixture.task(id, module, path).to_str().unwrap()]);
    }
    let listed = fixture.run(root, &["task", "ls"]);
    assert_eq!((listed["kind"].as_str(), listed["total"].as_u64()), (Some("tasks"), Some(2)), "{listed}");
    assert_eq!(fixture.run(root, &["ls", "tasks", "--state", "ready"])["items"].as_array().unwrap().len(), 2);
    assert_eq!(fixture.run(root, &["ls", "tasks", "--state", "merged"])["items"], json!([]));

    // Two candidates: no guessing, the candidates come back with the error.
    let ambiguous = envelope::parse(&fixture.output(root, &["task", "claim"]));
    assert_eq!(ambiguous["error"]["code"], "usage", "{ambiguous}");
    assert_eq!(ambiguous["data"]["candidates"].as_array().unwrap().len(), 2, "{ambiguous}");
    assert_eq!(ambiguous["next"][0]["cmd"], "towermd task claim T-one");

    // Rewriting T-two to need clarification leaves exactly one candidate.
    let edit = fixture.runtime.path().join("edit.json");
    fs::write(&edit, serde_json::to_vec(&json!({"id":"T-two","title":"Task T-two, reworded","module":module,
        "allowed_paths":[path],"acceptance":["existing smoke test passes"],"clarify":true})).unwrap()).unwrap();
    let edited = fixture.run(root, &["task", "edit", edit.to_str().unwrap()]);
    assert_eq!((edited["event"]["actor"].as_str(), edited["task"]["state"].as_str()), (Some("process:towermd-cli"), Some("proposed")), "{edited}");
    let shown = fixture.run(root, &["show", "T-two"]);
    assert_eq!((shown["kind"].as_str(), shown["object"]["input"]["title"].as_str()), (Some("task"), Some("Task T-two, reworded")), "{shown}");
    let history = fixture.run(root, &["history", "T-two"]);
    let kinds: Vec<_> = history["events"].as_array().unwrap().iter().map(|event| event["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["task.created", "task.edited"], "{history}");
    assert!(history["events"].as_array().unwrap().iter().all(|event| event.get("data").is_none()), "metadata only");
    assert_eq!(envelope::code(&fixture.output(root, &["history", "T-nobody"])), "not_found");
    assert_eq!(envelope::code(&fixture.output(root, &["show", "T-nobody"])), "not_found");

    let trees = tempfile::tempdir().unwrap();
    let first = trees.path().join("first");
    git(root, &["worktree", "add", "-b", "t/T-one", first.to_str().unwrap(), "main"]);
    let claimed = fixture.run(&first, &["task", "claim"]);
    assert_eq!(claimed["task"]["input"]["id"], "T-one", "{claimed}");
    // A claimed task is no longer the delegate's to rewrite.
    fs::write(&edit, serde_json::to_vec(&json!({"id":"T-one","title":"late","module":module,
        "allowed_paths":[path],"acceptance":["x"]})).unwrap()).unwrap();
    assert!(!fixture.output(root, &["task", "edit", edit.to_str().unwrap()]).status.success());
    // Nothing claimable now: not_found, pointing at the list.
    let none = envelope::parse(&fixture.output(root, &["task", "claim"]));
    assert_eq!((none["error"]["code"].as_str(), none["next"][0]["cmd"].as_str()), (Some("not_found"), Some("towermd ls tasks")), "{none}");
    fixture.run(&first, &["task", "release"]);
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

/// FR-11/14/31/32 end to end with the mock harness: `dev run` queues, the
/// scheduler claims and dispatches, the mock commits on the task branch, the
/// integrator verifies the branch and fast-forwards main with the trailers.
#[test]
fn dispatched_run_is_claimed_verified_and_merged_by_the_integrator() {
    let mut fixture=Fixture::new();
    let mock=Path::new(env!("CARGO_MANIFEST_DIR")).join("../towermd-devkit/tests/fixtures/harness/claude-code.sh");
    fixture.env.push(("TOWERMD_HARNESS_CLAUDE_CODE",mock.to_str().unwrap().to_owned()));
    fixture.env.push(("MOCK_MODE","commit".into()));
    // The daemon starts in publish and inherits this; the module is checked below.
    fixture.env.push(("MOCK_COMMIT","alpha/code.rs".into()));
    let args=tempfile::NamedTempFile::new().unwrap();
    fixture.env.push(("MOCK_ARGS",args.path().to_str().unwrap().to_owned()));
    // FR-51–54: a locked pack in the import commit (later `.towermd/` edits
    // on main need approval); only the executor's skill is handed over.
    let pack=fixture.root.path().join(".towermd/skillpacks/flow@1.0.0");
    for skill in ["plan","explore"]{
        fs::create_dir_all(pack.join("skills").join(skill)).unwrap();
        fs::write(pack.join("skills").join(skill).join("SKILL.md"),format!("---\nname: {skill}\n---\n{skill}\n")).unwrap();
    }
    let hash=Command::new("sh").current_dir(&pack)
        .args(["-c","find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256"]).output().unwrap();
    let hash=String::from_utf8(hash.stdout).unwrap().split_whitespace().next().unwrap().to_owned();
    fs::write(fixture.root.path().join(".towermd/skillpacks.yaml"),format!("packs:\n  - type: SkillPack\n    name: flow\n    version: 1.0.0\n    hash: sha256:{hash}\n    enabled_for:\n      executor: [plan]\n      controller: [explore]\n")).unwrap();
    git(fixture.root.path(),&["add",".towermd/skillpacks.yaml",".towermd/skillpacks"]);
    git(fixture.root.path(),&["commit","--amend","-qm","fixture with skill pack"]);
    let draft=fixture.publish();
    let root=fixture.root.path();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    assert_eq!(path,"alpha/code.rs");
    let adapters=fixture.run(root,&["dev","adapters"]);
    let mocked=adapters["items"].as_array().unwrap().iter().find(|item|item["id"]=="claude-code").unwrap();
    assert_eq!(mocked["installed"],true,"{adapters}");
    let input=fixture.task("T-run",module,path);
    fixture.run(root,&["task","add",input.to_str().unwrap()]);
    let main_before=git(root,&["rev-parse","main"]);
    let request=uuid::Uuid::now_v7().to_string();
    let queued=fixture.run(root,&["dev","run","T-run","--harness","claude-code","--request-id",&request]);
    assert_eq!(queued["request_id"],request);
    // A retry with the same request ID adds nothing.
    fixture.run(root,&["dev","run","T-run","--harness","claude-code","--request-id",&request]);
    let deadline=Instant::now()+Duration::from_secs(90);
    loop{
        let task=fixture.run(root,&["task","show","T-run"]);
        if task["task"]["state"]=="merged"{break;}
        let queue=fixture.run(root,&["dev","queue"]);
        assert!(Instant::now()<deadline,"dispatched task did not merge: {task} {queue}");
        std::thread::sleep(Duration::from_millis(500));
    }
    let tip=git(root,&["rev-parse","main"]);
    assert_ne!(tip,main_before);
    // FR-60: the dispatched run got the gateway in its own run directory.
    let runs=root.join(".git/towermd/runs");
    let configs:Vec<_>=fs::read_dir(&runs).unwrap().flatten()
        .flat_map(|run|fs::read_dir(run.path()).unwrap().flatten().map(|attempt|attempt.path().join("mcp.json")))
        .filter(|path|path.exists()).collect();
    assert_eq!(configs.len(),1,"{runs:?}");
    let gateway=fs::read_to_string(&configs[0]).unwrap();
    assert!(gateway.contains("\"mcp\"")&&gateway.contains("claude-code"),"{gateway}");
    let attempt=configs[0].parent().unwrap();
    let argv=fs::read_to_string(args.path()).unwrap();
    assert!(argv.contains(configs[0].to_str().unwrap()),"{argv}");
    assert!(argv.contains(attempt.join("skillpacks/flow").to_str().unwrap()),"{argv}");
    assert!(attempt.join("skillpacks/flow/skills/plan/SKILL.md").exists());
    assert!(!attempt.join("skillpacks/flow/skills/explore").exists(),"其他角色的技能不复制");
    let run=attempt.parent().unwrap().file_name().unwrap().to_str().unwrap();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let handle=fixture.post(&endpoint,"/projects/open",json!({"path":root}))["handle"].as_str().unwrap().to_owned();
    let details=fixture.get(&endpoint,&format!("/projects/{handle}/runs/{run}/events"));
    let loaded=details["events"].as_array().unwrap().iter().filter(|event|event["kind"]=="skill.loaded").count();
    assert_eq!(loaded,1,"{details}");
    let message=git(root,&["show","-s","--format=%B","main"]);
    assert!(message.starts_with("mock change"),"{message}");
    assert!(message.contains("Towermd-Evidence: "),"{message}");
    assert_eq!(git(root,&["show","-s","--format=%cn <%ce>|%an","main"]),"Tower.md integrator <towermd@localhost>|mock");
    assert!(git(root,&["show","--format=","--name-only","main"]).lines().any(|line|line==path));
    let queue=fixture.run(root,&["dev","queue"]);
    assert_eq!(queue["queue"],json!([]),"{queue}");
    // Nothing left to take off the queue: refused, not silently accepted.
    assert!(!fixture.output(root,&["dev","dequeue","T-run","--reason","done already"]).status.success());
}

/// M3-D and WP-X: a dispatched delegate parks a question. Within the level a
/// person set, the controller answers it from a read-only consultation, as its
/// own process and with its basis; the attempt's raw output stays on this
/// machine and says so.
#[test]
fn the_controller_answers_within_the_level_and_raw_output_is_archived_locally() {
    let mut fixture=Fixture::new();
    let mock=Path::new(env!("CARGO_MANIFEST_DIR")).join("../towermd-devkit/tests/fixtures/harness/claude-code.sh");
    fixture.env.push(("TOWERMD_HARNESS_CLAUDE_CODE",mock.to_str().unwrap().to_owned()));
    fixture.env.push(("MOCK_MODE","ask".into()));
    fixture.env.push(("MOCK_COMMIT","alpha/code.rs".into()));
    fixture.env.push(("MOCK_TOWERMD",env!("CARGO_BIN_EXE_towermd").into()));
    fixture.env.push(("MOCK_REPLY",r#"{"in_scope":true,"answer":"JSON","basis":["任务卡：验收标准"],"reason":null}"#.into()));
    fixture.env.push(("TOWERMD_ARCA",String::new()));
    let draft=fixture.publish();
    let root=fixture.root.path();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let handle=fixture.post(&endpoint,"/projects/open",json!({"path":root}))["handle"].as_str().unwrap().to_owned();
    // The default level only reports; a person raises it in the App.
    let level=json!({"request_id":uuid::Uuid::now_v7().to_string(),"config":"*","kind":"*","level":"answer_by_decision"});
    fixture.post(&endpoint,&format!("/projects/{handle}/delegation-levels"),level);
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let input=fixture.task("T-asks",module,path);
    fixture.run(root,&["task","add",input.to_str().unwrap()]);
    fixture.run(root,&["dev","run","T-asks","--harness","claude-code","--request-id",&uuid::Uuid::now_v7().to_string()]);
    let deadline=Instant::now()+Duration::from_secs(120);
    let projection=loop{
        let state=fixture.get(&endpoint,&format!("/projects/{handle}/state"));
        let projection=state["projection"].clone();
        let triaged=projection["ControllerTriage"].as_object().is_some_and(|group|!group.is_empty());
        let archived=projection["Artifact"].as_object().is_some_and(|group|!group.is_empty());
        let merged=fixture.run(root,&["task","show","T-asks"])["task"]["state"]=="merged";
        if triaged && archived && merged {break projection;}
        assert!(Instant::now()<deadline,"controller or archive did not run: {projection}");
        std::thread::sleep(Duration::from_millis(500));
    };
    let (hold,triage)=projection["ControllerTriage"].as_object().unwrap().iter().next().unwrap();
    assert_eq!(triage["outcome"],"answered","{triage}");
    assert_eq!(triage["level"],"answer_by_decision");
    assert!(triage["cost_usd"].is_null(),"the mock reports no cost: unknown, not 0");
    let answer=&projection["Answer"][hold];
    assert_eq!(answer["actor"],"process:towermd-controller","{answer}");
    assert_eq!(answer["input"]["text"],"JSON","{answer}");
    assert_eq!(answer["basis"],json!(["任务卡：验收标准"]));
    let (_,archive)=projection["Artifact"].as_object().unwrap().iter().next().unwrap();
    assert_eq!(archive["store"],"local","{archive}");
    assert!(archive["files"].as_array().unwrap().iter().all(|file|file["path"].as_str().unwrap().starts_with("towermd/runs/")),"{archive}");
    let queue=fixture.run(root,&["dev","queue"]);
    assert_eq!(queue["archive"]["installed"],false,"{queue}");
    assert!(queue["archive"]["local_only"].as_u64().unwrap()>=1,"{queue}");
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
    // D3: a G2 stop holds the task until a person decides in the App.
    let shown=fixture.run(&worktree,&["task","show"])["task"].clone();
    assert_eq!(shown["state"],"held");
    assert_eq!(shown["interrupt"]["kind"],"forbidden");
    assert!(!fixture.output(&worktree,&["dev","trailers","T-guarded"]).status.success());
    assert!(!fixture.output(&worktree,&["dev","finish","--state","done"]).status.success());
}

/// FR-120–124: gates declared on main block `dev finish` and `dev trailers`
/// while an active deterministic signal stands; the judgement is recorded as
/// `kind: gate` evidence and shown on the task card.
#[test]
fn declared_gates_block_finish_and_trailers_until_the_signal_clears() {
    let fixture=Fixture::new();
    let root=fixture.root.path();
    fs::write(root.join(".towermd/gates.yaml"),"owner: qa\ngates:\n  crap:\n    check: eng.crap\n    stage: finish\n  style: { check: eng.style, hard: false }\n").unwrap();
    // Part of the import commit: later `.towermd/` edits on main need approval.
    git(root,&["add",".towermd/gates.yaml"]);
    git(root,&["commit","--amend","-qm","fixture with gates"]);
    let draft=fixture.publish();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let input=fixture.task("T-gated",module,path);
    fixture.run(root,&["task","add",input.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();
    let worktree=trees.path().join("task");
    git(root,&["worktree","add","-b","t/T-gated",worktree.to_str().unwrap(),"main"]);
    fixture.run(&worktree,&["task","claim","T-gated"]);
    fs::write(worktree.join(path),"fn task() { /* gated */ }\n").unwrap();
    git(&worktree,&["add",path]);
    git(&worktree,&["commit","-qm","task implementation"]);
    let signal=|level:&str|fixture.run(root,&["signal","eng.crap","--level",level,"--deterministic","--title","CRAP 偏高",
        "--from","ci/nightly","--target","T-gated"]);
    signal("active");
    let blocked=fixture.output(&worktree,&["dev","finish"]);
    assert_eq!(envelope::code(&blocked),"gate_failed");
    let message=envelope::parse(&blocked)["error"]["message"].as_str().unwrap().to_owned();
    assert!(message.contains("检查未通过") && message.contains("crap"),"{message}");
    let card=&fixture.run(root,&["task","show","T-gated"])["delegation"]["card"];
    assert_eq!(card["completion"]["verdict"],"blocked","{card}");
    let crap=card["completion"]["gates"].as_array().unwrap().iter().find(|gate|gate["gate"]=="crap").unwrap().clone();
    assert_eq!((crap["status"].as_str(),crap["hard"].as_bool()),(Some("failed"),Some(true)));
    // A soft gate never blocks; a gate without a trusted check program stays clear.
    let style=card["completion"]["gates"].as_array().unwrap().iter().find(|gate|gate["gate"]=="style").unwrap().clone();
    assert_eq!((style["status"].as_str(),style["hard"].as_bool()),(Some("clear"),Some(false)));
    // Lowering the signal clears the gate; finish and trailers proceed.
    signal("passive");
    let queued=fixture.run(&worktree,&["dev","finish"]);
    let finished=fixture.run(&worktree,&["dev","finish","--wait",queued["evidence_id"].as_str().unwrap()]);
    assert_eq!(finished["status"],"passed","{finished}");
    let card=&fixture.run(root,&["task","show","T-gated"])["delegation"]["card"];
    // Every applicable check passed on the current version; the task still
    // completes only by its merge.
    assert_eq!((card["completion"]["verdict"].as_str(),card["phase"].as_str()),(Some("passed"),Some("claimed_done")),"{card}");
    let verify=card["completion"]["gates"].as_array().unwrap().iter().find(|gate|gate["gate"]=="core.verify").unwrap().clone();
    assert_eq!((verify["builtin"].as_bool(),verify["status"].as_str()),(Some(true),Some("passed")),"{card}");
    assert!(card["card"]["whole"].as_array().unwrap().iter().any(|entry|entry["check"]=="core.verify"),"{card}");
    signal("active");
    assert_eq!(envelope::code(&fixture.output(&worktree,&["dev","trailers","T-gated"])),"gate_failed");
    signal("passive");
    let gate_request=uuid::Uuid::now_v7().to_string();
    fixture.run(&worktree,&["dev","trailers","T-gated","--request-id",&gate_request]);
    let recorded:Vec<Value>=golden::history(root).iter().map(|line|serde_json::from_str::<Value>(line).unwrap())
        .filter(|event|event["type"]=="evidence.recorded").collect();
    let stages:Vec<_>=recorded.iter().map(|event|(event["data"]["stage"].as_str().unwrap().to_owned(),event["data"]["verdict"].as_str().unwrap().to_owned())).collect();
    assert_eq!(stages,[("finish".into(),"blocked".into()),("trailers".into(),"blocked".into()),("trailers".into(),"passed".into())],"{recorded:?}");
    let head=git(root,&["rev-parse","main"]);
    assert!(recorded.iter().all(|event|event["actor"]=="process:towermd" && event["layer"]=="L5" && event["obj"]=="T-gated"
        && event["data"]["kind"]=="gate" && event["data"]["revision"]==head.as_str()),"{recorded:?}");
}

/// FR-72: only a person in the App waives a declared hard gate, for one
/// object, until a time at most 30 days away. While it applies the gate
/// reads `waived` and passes, and the gate evidence names the waiver; at its
/// expiry the gate blocks again, and it is never extended.
#[test]
fn a_waiver_lets_one_blocking_gate_through_until_it_lapses() {
    let fixture=Fixture::new();
    let root=fixture.root.path();
    fs::write(root.join(".towermd/gates.yaml"),"gates:\n  crap:\n    check: eng.crap\n    stage: finish\n  style: { check: eng.style, hard: false }\n").unwrap();
    git(root,&["add",".towermd/gates.yaml"]);
    git(root,&["commit","--amend","-qm","fixture with gates"]);
    let draft=fixture.publish();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let handle=fixture.post(&endpoint,"/projects/open",json!({"path":root}))["handle"].as_str().unwrap().to_owned();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let input=fixture.task("T-waived",module,path);
    fixture.run(root,&["task","add",input.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();
    let worktree=trees.path().join("task");
    git(root,&["worktree","add","-b","t/T-waived",worktree.to_str().unwrap(),"main"]);
    fixture.run(&worktree,&["task","claim","T-waived"]);
    fs::write(worktree.join(path),"fn task() { /* waived */ }\n").unwrap();
    git(&worktree,&["add",path]);
    git(&worktree,&["commit","-qm","task implementation"]);
    fixture.run(root,&["signal","eng.crap","--level","active","--deterministic","--title","CRAP 偏高","--from","ci/nightly","--target","T-waived"]);
    assert_eq!(envelope::code(&fixture.output(&worktree,&["dev","finish"])),"gate_failed");

    let now=||std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let at=|ms:u64|towermd_core::tower_api::timestamp(ms).unwrap();
    let waive=format!("/projects/{handle}/delegations/T-waived/waive");
    let send=|client:&str,body:Value|{
        let response=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap()
            .post(format!("http://127.0.0.1:{}{waive}",endpoint["port"]))
            .header("x-towermd-instance",endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client",client).json(&body).send().unwrap();
        (response.status().as_u16(),response.json::<Value>().unwrap())
    };
    let body=|gate:&str,until:String|json!({"request_id":uuid::Uuid::now_v7().to_string(),"gate":gate,"reason":"上游修复前先放行","until":until});
    let hour=now()+3_600_000;
    // A CLI client is refused before anything is read: waivers are human-only.
    assert_eq!(send("cli",body("crap",at(hour))).0,409);
    let (status,refused)=send("app",body("crap",at(now()+31*86_400_000)));
    assert_eq!((status,refused["code"].as_str()),(400,Some("usage")),"{refused}");
    assert!(refused["error"].to_string().contains("30 天") || refused.to_string().contains("30 天"),"{refused}");
    assert_eq!(send("app",body("crap",at(now()-1000))).0,400,"an expiry in the past");
    let (status,builtin)=send("app",body("core.verify",at(hour)));
    assert_eq!((status,builtin["code"].as_str()),(400,Some("not_found")),"the kit's own check is never waived: {builtin}");
    let (status,soft)=send("app",body("style",at(hour)));
    assert_eq!(status,400,"a soft gate blocks nothing: {soft}");
    let mut forged=body("crap",at(hour));
    forged["actor"]=json!("human:forged");
    assert_eq!(send("app",forged).0,400,"bodies cannot carry signer fields");

    // A short waiver: the gate reads waived and finish passes.
    let lapse=now()+12_000;
    let first=body("crap",at(lapse));
    let (status,granted)=send("app",first.clone());
    assert_eq!((status,granted["status"].as_str(),granted["obj"].as_str()),(200,Some("accepted"),Some("T-waived")),"{granted}");
    let waiver=first["request_id"].as_str().unwrap().to_owned();
    assert_eq!(granted["event"],waiver.as_str(),"the waiver's ID is its request ID");
    let (_,again)=send("app",body("crap",at(hour)));
    assert!(again.to_string().contains("不续期"),"no second waiver while one applies: {again}");
    let card=&fixture.run(root,&["task","show","T-waived"])["delegation"]["card"];
    let crap=card["completion"]["gates"].as_array().unwrap().iter().find(|gate|gate["gate"]=="crap").unwrap().clone();
    assert_eq!((crap["status"].as_str(),crap["evidence"][0].as_str()),(Some("waived"),Some(waiver.as_str())),"{card}");
    // The grant's snapshot advanced main; finish wants a fast-forward.
    git(&worktree,&["rebase","-q","main"]);
    let queued=fixture.run(&worktree,&["dev","finish"]);
    assert_eq!(fixture.run(&worktree,&["dev","finish","--wait",queued["evidence_id"].as_str().unwrap()])["status"],"passed");

    // At its expiry the gate blocks again; a retry of the grant still returns the original.
    while now()<=lapse {std::thread::sleep(Duration::from_millis(200));}
    assert_eq!(envelope::code(&fixture.output(&worktree,&["dev","trailers","T-waived"])),"gate_failed");
    let (status,replayed)=send("app",first);
    assert_eq!((status,replayed["seq"].clone()),(200,granted["seq"].clone()),"{replayed}");
    // Never extended, but a person may grant a new one once it lapsed.
    let second=body("crap",at(now()+3_600_000));
    assert_eq!(send("app",second.clone()).0,200);
    git(&worktree,&["rebase","-q","main"]);
    let gate_request=uuid::Uuid::now_v7().to_string();
    fixture.run(&worktree,&["dev","trailers","T-waived","--request-id",&gate_request]);

    let history:Vec<Value>=golden::history(root).iter().map(|line|serde_json::from_str::<Value>(line).unwrap()).collect();
    let grants:Vec<_>=history.iter().filter(|event|event["type"]=="waiver.granted").collect();
    assert_eq!(grants.len(),2,"{grants:?}");
    assert!(grants.iter().all(|event|event["actor"]=="human:local" && event["layer"]=="L3" && event["obj"]=="T-waived"),"{grants:?}");
    let cited=|event:&Value,id:&str|event["data"]["gates"].as_array().unwrap().iter()
        .any(|gate|gate["gate"]=="crap" && gate["status"]=="waived" && gate["evidence"][0]==id);
    let gates:Vec<_>=history.iter().filter(|event|event["type"]=="evidence.recorded" && event["data"]["kind"]=="gate").collect();
    let verdicts:Vec<_>=gates.iter().map(|event|(event["data"]["stage"].as_str().unwrap(),event["data"]["verdict"].as_str().unwrap())).collect();
    // As before waivers: a passing finish records nothing of its own.
    assert_eq!(verdicts,[("finish","blocked"),("trailers","blocked"),("trailers","passed")],"{gates:?}");
    assert!(cited(gates[2],second["request_id"].as_str().unwrap()),"the evidence names the waiver it relied on: {gates:?}");
    assert!(!cited(gates[1],&waiver),"a lapsed waiver is not applied");
}

/// FR-125: a run dropping its module's check and main loosening gates and
/// thresholds each raise one active `core.weakening` signed by Tower.md. The
/// run's is attributed to its task and configuration, main's to nobody; the
/// changes themselves still wait for a person.
#[test]
fn weakened_commitments_raise_one_signal_per_source_and_stay_unapproved() {
    let fixture=Fixture::new();
    let root=fixture.root.path();
    fs::write(root.join(".towermd/gates.yaml"),"gates:\n  crap: { check: eng.crap }\n  style: { check: eng.style }\n").unwrap();
    git(root,&["add",".towermd/gates.yaml"]);
    git(root,&["commit","--amend","-qm","fixture with gates"]);
    let draft=fixture.publish();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let handle=fixture.post(&endpoint,"/projects/open",json!({"path":root}))["handle"].as_str().unwrap().to_owned();
    let state=||fixture.get(&endpoint,&format!("/projects/{handle}/state"))["projection"]["TowerApi"].clone();
    let raised=|target:&str|state()["signals"].as_array().into_iter().flatten()
        .find(|entry|entry["data"]["kind"]=="core.weakening" && entry["target"]==target).cloned();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let input=fixture.task("T-weak",module,path);
    fixture.run(root,&["task","add",input.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();
    let worktree=trees.path().join("task");
    git(root,&["worktree","add","-b","t/T-weak",worktree.to_str().unwrap(),"main"]);
    fixture.run(&worktree,&["task","claim","T-weak","--harness","claude-code","--harness-version","2.1"]);

    // The run drops its module's verify command: G2 captures it as a draft.
    let doc=git(&worktree,&["ls-files","docs/tower"]).lines().map(str::to_owned).find(|file|{
        let text=fs::read_to_string(worktree.join(file)).unwrap_or_default();
        towermd_core::okf::Document::parse(&text).is_ok_and(|doc|doc.doc_type()==Some("Module") && doc.get_str("id")==Some(module))
    }).expect("module document");
    let mut document=towermd_core::okf::Document::parse(&fs::read_to_string(worktree.join(&doc)).unwrap()).unwrap();
    assert!(document.get("verify").is_some_and(|verify|verify.as_sequence().is_some_and(|list|!list.is_empty())),"{doc}");
    document.set("verify",Default::default()).unwrap();
    fs::write(worktree.join(&doc),document.to_text()).unwrap();
    git(&worktree,&["add",&doc]);
    git(&worktree,&["commit","-qm","drop the module check"]);
    assert_eq!(envelope::code(&fixture.output(&worktree,&["dev","check"])),"guard_blocked");
    let proposals=fixture.get(&endpoint,&format!("/projects/{handle}/proposals"));
    let captured=proposals["proposals"].as_array().unwrap().iter().find(|entry|entry["proposal"]["changes"].as_array()
        .is_some_and(|changes|changes.iter().any(|change|change["path"]==doc.as_str()))).unwrap().clone();
    assert_eq!(captured["status"],"draft","captured, never approved");
    assert_eq!(captured["weakening"][0]["change"],"check_removed","{captured}");
    let id=captured["proposal"]["id"].as_str().unwrap();
    let signal=raised(id).unwrap_or_else(||panic!("no signal for {id}: {}",state()));
    assert_eq!((signal["data"]["level"].as_str(),signal["from"]["id"].as_str(),signal["last"]["actor"].as_str()),
        (Some("active"),Some("process:towermd"),Some("process:towermd")),"{signal}");
    let refs:Vec<&str>=signal["refs"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(refs.contains(&format!("proposal:{id}").as_str()) && refs.contains(&"delegation:T-weak")
        && refs.iter().any(|entry|entry.starts_with("config:") && entry.contains("claude-code")),"{refs:?}");
    // A retry captures the same draft and raises nothing new.
    assert_eq!(envelope::code(&fixture.output(&worktree,&["dev","check"])),"guard_blocked");
    assert_eq!(raised(id).unwrap()["count"],signal["count"]);

    // Main softens one gate, drops another and loosens a threshold.
    let edit=trees.path().join("main-edit");
    git(root,&["worktree","add","--detach",edit.to_str().unwrap(),"main"]);
    fs::write(edit.join(".towermd/gates.yaml"),"gates:\n  crap: { check: eng.crap, hard: false }\n").unwrap();
    let config=fs::read_to_string(edit.join(".towermd/config.yaml")).unwrap();
    fs::write(edit.join(".towermd/config.yaml"),format!("{config}review_backlog: 9\n")).unwrap();
    git(&edit,&["commit","-qam","loosen the bar"]);
    let tip=git(&edit,&["rev-parse","HEAD"]);
    git(root,&["update-ref","refs/heads/main",&tip]);
    let _=fixture.output(&worktree,&["dev","check"]);
    let deadline=Instant::now()+Duration::from_secs(35);
    let signal=loop {
        if let Some(signal)=raised("tasks-project") {break signal;}
        assert!(Instant::now()<deadline,"main weakening not raised: {}",state());
        std::thread::sleep(Duration::from_millis(500));
    };
    let detail=signal["data"]["detail"].as_str().unwrap();
    assert!(detail.contains("删除门禁 style") && detail.contains("由硬改软") && detail.contains("由 5 放宽到 9"),"{detail}");
    assert_eq!(signal["data"]["value"],3);
    assert_eq!(signal["refs"],json!([format!("main:{tip}")]),"main changes are attributed to nobody");
    let weakenings=state()["weakenings"].as_array().unwrap().len();
    let _=fixture.output(&worktree,&["dev","check"]);
    assert_eq!(state()["weakenings"].as_array().unwrap().len(),weakenings,"each source raises once");
    // The protected change is still unapproved: main inspection blocks it.
    let inspected=&fixture.get(&endpoint,&format!("/projects/{handle}/state"))["projection"]["MainInspection"];
    assert_eq!(inspected["passed"],false,"{inspected}");
    // FR-03: only process configuration changed, so it is a minor change.
    let deadline=Instant::now()+Duration::from_secs(35);
    loop {
        let alerts=fixture.get(&endpoint,&format!("/projects/{handle}/state"))["projection"]["Alert"].clone();
        let raised=alerts.as_object().into_iter().flatten().find(|(_,alert)|alert["data"]["reason"]=="unapproved_protected_change"
            && alert["data"]["detail"]["commit"]==tip.as_str()).map(|(_,alert)|alert.clone());
        if let Some(alert)=raised {assert_eq!(alert["data"]["detail"]["class"],"minor","{alert}");break;}
        assert!(Instant::now()<deadline,"no protected change alert: {alerts}");
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Spec 15.5 and 16.2: the App drills into a task and a module, lists what
/// waits on a person, asks for a pause or a report as prompts, and reassigns
/// a running task; the CLI cannot reassign, and nothing here completes work.
#[test]
fn the_app_drills_down_prompts_and_reassigns_a_running_task() {
    let fixture=Fixture::new();
    let root=fixture.root.path();
    let draft=fixture.publish();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let handle=fixture.post(&endpoint,"/projects/open",json!({"path":root}))["handle"].as_str().unwrap().to_owned();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let input=fixture.task("T-app",module,path);
    fixture.run(root,&["task","add",input.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();
    let worktree=trees.path().join("task");
    git(root,&["worktree","add","-b","t/T-app",worktree.to_str().unwrap(),"main"]);
    let run=fixture.run(&worktree,&["task","claim","T-app"])["run"]["binding"]["id"].as_str().unwrap().to_owned();

    let shown=fixture.get(&endpoint,&format!("/projects/{handle}/tasks/T-app"));
    assert_eq!((shown["task"]["state"].as_str(),shown["runs"][0]["binding"]["id"].as_str()),(Some("running"),Some(run.as_str())),"{shown}");
    assert!(shown["task_card"]["markdown"].as_str().unwrap().contains("# 任务卡 T-app"),"{shown}");
    let drilled=fixture.get(&endpoint,&format!("/projects/{handle}/modules/{module}"));
    assert_eq!(drilled["module"]["front"]["type"],"Module","{drilled}");
    assert_eq!(drilled["tasks"][0]["input"]["id"],"T-app","{drilled}");
    let holds=fixture.get(&endpoint,&format!("/projects/{handle}/holds"));
    assert_eq!((holds["questions"].as_array().map(Vec::len),holds["interrupts"].as_array().map(Vec::len)),(Some(0),Some(0)),"{holds}");

    let paused=fixture.post(&endpoint,&format!("/projects/{handle}/tasks/T-app/pause"),json!({"request_id":uuid::Uuid::now_v7().to_string()}));
    fixture.post(&endpoint,&format!("/projects/{handle}/tasks/T-app/report"),json!({"request_id":uuid::Uuid::now_v7().to_string()}));
    assert_eq!(fixture.get(&endpoint,&format!("/projects/{handle}/tasks/T-app"))["task"]["state"],"running","a pause is a prompt, not a state change: {paused}");
    let card=fixture.run(&worktree,&["task","show"])["task_card"]["markdown"].as_str().unwrap().to_owned();
    assert!(card.contains("人工插话：请在下一个安全检查点暂停") && card.contains("人工插话：请在下一个回合结束后汇报"),"{card}");

    let release=format!("/projects/{handle}/tasks/T-app/release");
    let send=|client:&str,body:Value|{
        let response=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap()
            .post(format!("http://127.0.0.1:{}{release}",endpoint["port"]))
            .header("x-towermd-instance",endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client",client).json(&body).send().unwrap();
        (response.status().as_u16(),response.json::<Value>().unwrap_or(Value::Null))
    };
    let request=uuid::Uuid::now_v7().to_string();
    let denied=send("cli",json!({"request_id":request,"run":run}));
    assert!(denied.0==409 && denied.1.to_string().contains("客户端类型"),"reassigning is the person's action: {denied:?}");
    let stale=send("app",json!({"request_id":uuid::Uuid::now_v7().to_string(),"run":"R-stale"}));
    assert!(stale.1["code"]=="conflict" && stale.1.to_string().contains("当前运行已变化"),"{stale:?}");
    let (status,accepted)=send("app",json!({"request_id":request,"run":run}));
    assert_eq!(status,200,"{accepted}");
    assert_eq!(send("app",json!({"request_id":request,"run":run})).1["event"],accepted["event"],"retries are idempotent");
    let shown=fixture.get(&endpoint,&format!("/projects/{handle}/tasks/T-app"));
    assert_eq!((shown["task"]["state"].as_str(),shown["runs"][0]["state"].as_str()),(Some("ready"),Some("released")),"{shown}");
    // The module lease is free again: the task can be claimed anew.
    fixture.run(&worktree,&["task","claim","T-app"]);
}

/// Spec 9.3: once B0 is approved, Tower.md measures `main` against the
/// approved modules and raises a growth signal over its threshold, signed
/// `process:towermd`; when main falls back under it the signal drops to log.
/// Neither changes any structure.
#[test]
fn growth_signals_follow_main_against_the_approved_modules() {
    let fixture=Fixture::new();
    let root=fixture.root.path();
    let config=fs::read_to_string(root.join(".towermd/config.yaml")).unwrap();
    fs::write(root.join(".towermd/config.yaml"),format!("{config}signals:\n  module_size: {{lines: 2}}\n")).unwrap();
    fs::write(root.join("alpha/code.rs"),"fn task() {}\nfn more() {}\nfn most() {}\n").unwrap();
    git(root,&["commit","-qam","a large alpha"]);
    let draft=fixture.publish();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let handle=fixture.post(&endpoint,"/projects/open",json!({"path":root}))["handle"].as_str().unwrap().to_owned();
    let alpha=draft["review"]["modules"].as_array().unwrap().iter()
        .find(|module|module["files"].as_array().unwrap().iter().any(|file|file=="alpha/code.rs")).unwrap()["id"].as_str().unwrap().to_owned();
    let state=||fixture.get(&endpoint,&format!("/projects/{handle}/state"));
    let growth=|level:&str|state()["projection"]["TowerApi"]["signals"].as_array().into_iter().flatten()
        .find(|entry|entry["data"]["kind"]=="growth.module_too_large" && entry["target"]==alpha.as_str() && entry["data"]["level"]==level).cloned();
    let deadline=Instant::now()+Duration::from_secs(35);
    let signal=loop {
        if let Some(signal)=growth("active") {break signal;}
        assert!(Instant::now()<deadline,"no growth signal: {}",state()["projection"]["TowerApi"]);
        std::thread::sleep(Duration::from_millis(500));
    };
    assert_eq!((signal["from"]["id"].as_str(),signal["data"]["value"].as_u64(),signal["data"]["threshold"].as_u64()),
        (Some("process:towermd"),Some(3),Some(2)),"{signal}");
    let listed=state()["attention_list"].clone();
    let item=listed["items"].as_array().or(listed.as_array()).into_iter().flatten()
        .find(|item|item["id"].as_str().is_some_and(|id|id.contains("growth.module_too_large"))).cloned()
        .unwrap_or_else(||panic!("not listed: {listed}"));
    assert!(item["detail"].as_str().unwrap().starts_with("Tower.md 测得"),"{item}");
    assert!(item["action"].as_str().unwrap().contains("towermd propose"),"{item}");
    // A standing finding is not resent.
    std::thread::sleep(Duration::from_secs(11));
    assert_eq!(growth("active").unwrap()["count"],1);

    // Main shrinks alpha under the threshold: the signal drops to log.
    let trees=tempfile::tempdir().unwrap();
    let edit=trees.path().join("main-edit");
    git(root,&["worktree","add","--detach",edit.to_str().unwrap(),"main"]);
    fs::write(edit.join("alpha/code.rs"),"fn task() {}\n").unwrap();
    git(&edit,&["commit","-qam","split alpha"]);
    let tip=git(&edit,&["rev-parse","HEAD"]);
    git(root,&["update-ref","refs/heads/main",&tip]);
    let deadline=Instant::now()+Duration::from_secs(45);
    let cleared=loop {
        if let Some(signal)=growth("log") {break signal;}
        assert!(Instant::now()<deadline,"growth signal not cleared: {}",state()["projection"]["TowerApi"]);
        std::thread::sleep(Duration::from_millis(500));
    };
    assert_eq!(cleared["count"],2);
    assert_eq!(cleared["refs"],json!([format!("main:{tip}")]));
    let listed=state()["attention_list"].to_string();
    assert!(!listed.contains("growth.module_too_large"),"{listed}");
}

#[test]
fn a_sentence_seeds_only_the_first_version() {
    let fixture=Fixture::new();
    let root=fixture.root.path();
    let seed=fixture.run(root,&["dev","seed","做一个把会议录音转成待办的命令行工具"]);
    assert_eq!((seed["origin"].as_str(),seed["intent"]["id"].as_str(),seed["intent"]["path"].as_str()),
        (Some("argument"),Some("I-1"),Some("intents/I-1.md")),"{seed}");
    let text=seed["intent"]["text"].as_str().unwrap();
    assert!(text.contains("type: Intent")&&text.contains("layer: L0")&&text.contains("status: draft"),"{text}");
    assert_eq!((seed["limits"]["questions"].as_u64(),seed["limits"]["tasks"].as_u64()),(Some(3),Some(7)));
    // Seeding writes nothing and needs a sentence or the promoted goal.
    let missing=fixture.output(root,&["dev","seed"]);
    assert!(!missing.status.success());
    assert_eq!(envelope::code(&missing),"usage");
    assert!(String::from_utf8_lossy(&missing.stderr).contains("立项"));
    assert_eq!(git(root,&["status","--porcelain"]),"");
    // Once a version is approved, growth goes through proposals.
    fixture.publish();
    let later=fixture.output(root,&["dev","seed","再做一个"]);
    assert!(!later.status.success());
    assert_eq!(envelope::code(&later),"needs_proposal");
}
