//! End-to-end delegation flow (FR-185, FR-201–211): App-only human actions,
//! CLI clarify/reply/claims, report requests in `delivered[]`, the daemon's
//! overdue signal and the derived `delegation_view`.
#[path = "support/envelope.rs"]
mod envelope;

use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

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
struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
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
    fn request(&self, endpoint: &Value, path: &str, client: &str, body: Value) -> (u16, Value) {
        let response = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap()
            .post(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", client).json(&body).send().unwrap();
        let status = response.status().as_u16();
        (status, response.json().unwrap())
    }
    fn file(&self, name: &str, body: Value) -> String {
        let path = self.runtime.path().join(name);
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        path.to_str().unwrap().to_owned()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(self.root.path(), &["daemon", "stop"]);
    }
}


fn uuid() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn object<'a>(state: &'a Value, id: &str) -> &'a Value {
    state["delegation_view"]["objects"].as_array().unwrap().iter().find(|object| object["id"] == id)
        .unwrap_or_else(|| panic!("no delegation view for {id}: {}", state["delegation_view"]))
}

fn delivered_requests(output: &Output) -> Vec<Value> {
    envelope::parse(output)["delivered"].as_array().unwrap().iter()
        .filter(|message| message["message"]["kind"] == "request_report").cloned().collect()
}

#[test]
fn a_delegation_runs_from_clarify_to_send_back_with_reports_overdue_and_app_only_human_actions() {
    let fixture = Fixture::new();
    let draft = fixture.publish();
    let root = fixture.root.path();
    let endpoint = fixture.run(root, &["daemon", "ensure"]);
    let opened = fixture.post(&endpoint, "/projects/open", json!({"path":root}));
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let project = |suffix: &str| format!("/projects/{handle}{suffix}");
    let state = || fixture.get(&endpoint, &project("/state"));
    let module = draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path = draft["review"]["modules"][0]["files"][0].as_str().unwrap();

    // The App's task form takes the new brief fields; clarify keeps it proposed.
    let created = fixture.post(&endpoint, &project("/tasks/human"), json!({"request_id":uuid(),"task":{
        "id":"T-del","title":"Delegated","module":module,"allowed_paths":[path],"why":"users need it",
        "limits":"no new dependencies","checkpoints":[{"text":"plan","due":"2026-01-01T00:00:00Z"},{"text":"stop and show"}],
        "self_check":"run the tests","clarify":true}}));
    assert_eq!(created["task"]["input"]["clarify"], true, "{created}");
    assert_eq!(object(&state(), "T-del")["phase"], "clarifying");

    // The delegate clarifies from the CLI, with its self-reported configuration.
    let too_many = fixture.file("many.json", json!({"restated":"x","done_when":["a"],"questions":["1","2","3","4"]}));
    assert!(!fixture.output(root, &["dev", "clarify", "T-del", &too_many]).status.success());
    let clarification = fixture.file("clarify.json", json!({"restated":"Make the parser accept X",
        "done_when":["parser accepts X"],"questions":["Which format?"]}));
    let clarified = fixture.run(root, &["dev", "clarify", "T-del", &clarification, "--harness", "claude-code", "--harness-version", "2.1"]);
    assert_eq!(clarified["obj"], "T-del");
    let view = state();
    let draft_card = &object(&view, "T-del")["clarification"]["draft"];
    assert_eq!((draft_card["restated"].as_str(), draft_card["self_reported"]["name"].as_str()),
        (Some("Make the parser accept X"), Some("claude-code")));
    assert_eq!(view["delegation_view"]["settings"]["review_backlog"], 5);

    // Only the App finalizes; the CLI client is refused before anything is read.
    let finalize = json!({"request_id":uuid(),"answers":["JSON"],"brief":{"why":"users need it",
        "done_when":["parser accepts X","old inputs still parse"],"limits":"no new dependencies",
        "checkpoints":[{"text":"plan","due":"2026-01-01T00:00:00Z"},{"text":"stop and show"}],"self_check":"run the tests"}});
    let (status, refused) = fixture.request(&endpoint, &project("/delegations/T-del/finalize"), "cli", finalize.clone());
    assert_eq!((status, refused["code"].as_str()), (409, Some("conflict")), "{refused}");
    let finalized = fixture.post(&endpoint, &project("/delegations/T-del/finalize"), finalize.clone());
    assert_eq!(fixture.post(&endpoint, &project("/delegations/T-del/finalize"), finalize.clone())["seq"], finalized["seq"]);
    let mut changed = finalize.clone();
    changed["answers"] = json!(["YAML"]);
    let (status, conflict) = fixture.request(&endpoint, &project("/delegations/T-del/finalize"), "app", changed);
    assert_eq!((status, conflict["code"].as_str()), (400, Some("conflict")), "{conflict}");
    let view = state();
    assert_eq!(object(&view, "T-del")["phase"], "ready");
    assert_eq!(object(&view, "T-del")["brief_set_by"], "human");
    assert!(!fixture.output(root, &["dev", "clarify", "T-del", &clarification]).status.success(), "clarify after the final card");

    // Claim with the full fingerprint; the dated checkpoint becomes a request.
    let trees = tempfile::tempdir().unwrap();
    let worktree = trees.path().join("task");
    git(root, &["worktree", "add", "-b", "t/T-del", worktree.to_str().unwrap(), "main"]);
    let claimed = fixture.run(&worktree, &["task", "claim", "T-del", "--harness", "claude-code", "--harness-version", "2.1",
        "--vendor", "anthropic", "--model", "opus", "--skills-version", "0.2"]);
    let source = &claimed["run"]["binding"]["self_reported"];
    assert_eq!((source["vendor"].as_str(), source["model"].as_str(), source["skills"].as_str()),
        (Some("anthropic"), Some("opus"), Some("0.2")), "{claimed}");
    let run = claimed["run"]["binding"]["id"].as_str().unwrap().to_owned();
    let request = format!("R-{run}-1");
    let shown = fixture.output(&worktree, &["task", "show"]);
    let delivered = delivered_requests(&shown);
    assert_eq!(delivered.len(), 1, "{}", String::from_utf8_lossy(&shown.stdout));
    assert_eq!((delivered[0]["request"].as_str(), delivered[0]["run"].as_str(), delivered[0]["actor"].as_str()),
        (Some(request.as_str()), Some(run.as_str()), Some("process:towermd")));
    assert!(delivered[0]["text"].as_str().unwrap().contains("towermd reply"));
    let outbound = &delivered[0]["envelope"];
    assert_eq!((outbound["verb"].as_str(), outbound["from"]["kind"].as_str(), outbound["target"].as_str(), outbound["data"]["request_id"].as_str()),
        (Some("request_report"), Some("tower"), Some("T-del"), Some(request.as_str())), "{outbound}");
    // Inside a run a report defaults to the run's task and self-reported delegate.
    let phase = object(&state(), "T-del")["phase"].clone();
    let reported = fixture.run(&worktree, &["report", "--state", "claimed_done", "--summary", "自认为完成", "--progress", "7/7"]);
    assert_eq!((reported["obj"].as_str(), reported["envelope"]["from"].clone()),
        (Some("T-del"), json!({"id":"claude-code/2.1","kind":"delegate"})), "{reported}");
    assert_eq!(object(&state(), "T-del")["phase"], phase, "a claimed done changes nothing");
    let details = envelope::data_checked(&["task", "show"], &shown);
    assert_eq!(details["delegation"]["report_requests"][0]["id"], request.as_str());
    assert_eq!(details["delegation"]["card"]["brief"]["why"], "users need it");
    // Not acknowledged: the request comes back on the next call.
    assert_eq!(delivered_requests(&fixture.output(&worktree, &["status"])).len(), 1);

    // The due date is past: maintenance writes one passive overdue, once.
    let deadline = Instant::now() + Duration::from_secs(30);
    let overdue = loop {
        let view = state();
        let entry = view["delegation_view"]["report_requests"][0]["overdue"].clone();
        if !entry.is_null() {break entry;}
        assert!(Instant::now() < deadline, "no overdue written: {}", view["delegation_view"]["report_requests"]);
        std::thread::sleep(Duration::from_millis(500));
    };
    assert_eq!((overdue["data"]["level"].as_str(), overdue["event"]["actor"].as_str()), (Some("passive"), Some("process:towermd")));
    std::thread::sleep(Duration::from_secs(11));
    let view = state();
    assert_eq!(view["delegation_view"]["report_requests"][0]["overdue"], overdue);
    assert_eq!(view["projection"]["Delegation"]["reports"].as_object().unwrap().len(), 1);

    // The delegate replies; the request stops being delivered at once.
    let long = "长".repeat(101);
    assert!(!fixture.output(&worktree, &["reply", &request, "--state", "in_progress", "--summary", &long]).status.success());
    assert!(!fixture.output(&worktree, &["reply", &request, "--state", "done", "--summary", "x"]).status.success());
    let reply_args = ["reply", request.as_str(), "--state", "in_progress", "--summary", "解析器完成一半", "--next", "补测试"];
    let replied = fixture.output(&worktree, &reply_args);
    let data = envelope::data_checked(&reply_args, &replied);
    assert_eq!(data["obj"], request.as_str());
    assert!(delivered_requests(&replied).is_empty());
    assert!(delivered_requests(&fixture.output(&worktree, &["status"])).is_empty());
    let reply = &state()["delegation_view"]["report_requests"][0]["reply"];
    assert_eq!((reply["data"]["state"].as_str(), reply["data"]["self_reported"]["model"].as_str(), reply["event"]["actor"].as_str()),
        (Some("in_progress"), Some("opus"), Some("process:towermd-cli")));

    // finish carries per-criterion claims; they never complete anything.
    fs::write(worktree.join(path), "fn task() { /* delegated */ }\n").unwrap();
    git(&worktree, &["add", path]);
    git(&worktree, &["commit", "-qm", "delegated work"]);
    // Snapshots of the events above advanced main; finish wants a fast-forward.
    git(&worktree, &["rebase", "-q", "main"]);
    let wrong = fixture.file("wrong.json", json!([{"criterion":2,"note":"x"}]));
    assert_eq!(envelope::code(&fixture.output(&worktree, &["dev", "finish", "--claims", &wrong])), "rejected");
    let claims = fixture.file("claims.json", json!([{"criterion":0,"note":"tested with X"}]));
    let finish_id = uuid();
    let queued = fixture.run(&worktree, &["dev", "finish", "--claims", &claims, "--request-id", &finish_id]);
    assert_eq!(envelope::code(&fixture.output(&worktree, &["dev", "finish", "--request-id", &finish_id])), "conflict");
    let evidence = queued["evidence_id"].as_str().unwrap();
    assert_eq!(fixture.run(&worktree, &["dev", "finish", "--wait", evidence])["status"], "passed");
    let view = state();
    assert_eq!(object(&view, "T-del")["phase"], "claimed_done");
    let (status, early) = fixture.request(&endpoint, &project("/delegations/T-del/confirm"), "app",
        json!({"request_id":uuid(),"review_ms":10,"unproven":[]}));
    assert_eq!(status, 400, "a claim alone cannot be accepted: {early}");
    // core.verify applies to every task: only the check completes it (FR-121).
    assert_eq!(early["code"], "gate_failed", "{early}");
    assert_eq!(object(&view, "T-del")["completion"]["gates"][0]["check"], "core.verify");
    let gate = fixture.output(&worktree, &["--text", "dev", "trailers", "T-del"]);
    assert!(gate.status.success(), "{}", String::from_utf8_lossy(&gate.stderr));
    git(&worktree, &["commit", "--amend", "-qm", &format!("delegated work\n\n{}", String::from_utf8(gate.stdout).unwrap())]);
    let tip = git(&worktree, &["rev-parse", "HEAD"]);
    git(root, &["update-ref", "refs/heads/main", &tip]);
    let deadline = Instant::now() + Duration::from_secs(35);
    let view = loop {
        let view = state();
        if object(&view, "T-del")["phase"] == "done" {break view;}
        assert!(Instant::now() < deadline, "task was not merged: {}", object(&view, "T-del"));
        std::thread::sleep(Duration::from_millis(500));
    };
    let card = &object(&view, "T-del")["card"];
    assert!(card["rows"][0]["evidence"].as_array().unwrap().iter().any(|evidence| evidence["source_kind"] == "claimed"), "{card}");
    assert_eq!(card["can_judge"], true);

    // Every human action is App-only; each is accepted from the App.
    let human = [
        ("/delegations/T-del/used", json!({"criteria":[1],"note":"ran it on old inputs"})),
        ("/delegations/T-del/send-back", json!({"reason":"X still fails on empty input","review_ms":4200})),
    ];
    for (suffix, body) in &human {
        let mut body = body.clone();
        body["request_id"] = json!(uuid());
        let (status, _) = fixture.request(&endpoint, &project(suffix), "cli", body.clone());
        assert_eq!(status, 409, "{suffix}");
        let accepted = fixture.post(&endpoint, &project(suffix), body);
        assert_eq!(accepted["status"], "accepted");
    }
    let view = state();
    let object_view = object(&view, "T-del");
    assert_eq!(object_view["phase"], "ready", "a send-back reopens the task");
    let verdict = &object_view["attempts"][0]["verdict"];
    assert_eq!(verdict["kind"], "sent_back");
    let kind = object_view["work_kind"].as_str().unwrap().to_owned();
    let config = object_view["attempts"][0]["config"].as_str().unwrap().to_owned();
    let rules = json!({"request_id":uuid(),"id":"rule-empty-input","kind":kind,"text":"cover empty input","source":verdict["event"]["eid"]});
    let levels = json!({"request_id":uuid(),"config":config,"kind":"parsing","level":"approve_minor"});
    let rename = json!({"request_id":uuid(),"from":kind,"to":"parsing"});
    for (suffix, body) in [("/rules", rules), ("/work-kinds/rename", rename), ("/delegation-levels", levels)] {
        assert_eq!(fixture.request(&endpoint, &project(suffix), "cli", body.clone()).0, 409, "{suffix}");
        assert_eq!(fixture.post(&endpoint, &project(suffix), body)["status"], "accepted", "{suffix}");
    }
    let (status, _) = fixture.request(&endpoint, &project("/delegations/T-del/confirm"), "app",
        json!({"request_id":uuid(),"review_ms":10,"unproven":[],"actor":"human:forged"}));
    assert_eq!(status, 400, "bodies cannot carry signer fields");
    let view = state();
    let object_view = object(&view, "T-del");
    assert_eq!(object_view["work_kind"], "parsing");
    assert_eq!(view["delegation_view"]["rules"][0]["work_kind"], "parsing");
    assert_eq!(view["delegation_view"]["levels"][0]["level"], "approve_minor");

    // `task show` gives the delegate the card, rules and past send-backs.
    let shown = fixture.run(root, &["task", "show", "T-del"]);
    let details = &shown["delegation"];
    assert_eq!(details["rules"][0]["text"], "cover empty input");
    assert_eq!(details["sent_back"][0]["reason"], "X still fails on empty input");
    assert!(details["report_requests"].as_array().unwrap().is_empty());
}
