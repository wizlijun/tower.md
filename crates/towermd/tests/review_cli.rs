#[path = "support/envelope.rs"]
mod envelope;

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    runtime: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            runtime: tempfile::tempdir().unwrap(),
        };
        git(fixture.root.path(), &["init", "-b", "main"]);
        fs::create_dir_all(fixture.root.path().join(".towermd")).unwrap();
        fs::write(
            fixture.root.path().join(".towermd/config.yaml"),
            "project_id: review-integration-project\ndocs_dir: docs/tower\nhub: local\n",
        )
        .unwrap();
        for module in ["alpha", "beta", "gamma"] {
            let directory = fixture.root.path().join(format!("src/{module}"));
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("mod.rs"), "pub mod helpers;\n").unwrap();
            fs::write(directory.join("helpers.rs"), "pub fn run() {}\n").unwrap();
        }
        fs::write(
            fixture.root.path().join("README.md"),
            "# Committed reference\nOnly this committed text is authoritative.\n",
        )
        .unwrap();
        fs::write(
            fixture.root.path().join("Makefile"),
            "test:\n\t@echo real-review-proof\n",
        )
        .unwrap();
        git(fixture.root.path(), &["add", "."]);
        git(fixture.root.path(), &["commit", "-m", "review fixture"]);
        fixture
    }

    fn output(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
            .args(args)
            .current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn run(&self, args: &[&str]) -> Value {
        let output = self.output(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        envelope::data_checked(args, &output)
    }

    fn synthesis(&self) -> Value {
        let scan = self.run(&["dev", "scan", "start"]);
        let scan_id = scan["id"].as_str().unwrap();
        // Exit 5 means the scan is still running after one wait window.
        let ready = loop {
            let args = ["dev", "scan", "wait", scan_id];
            let output = self.output(&args);
            if output.status.code() != Some(5) {
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                break envelope::data_checked(&args, &output);
            }
        };
        assert_eq!(ready["status"], "ready", "{ready}");
        let input = self.run(&["dev", "draft", "template", scan_id]);
        let path = self.runtime.path().join("synthesis.json");
        fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
        self.run(&[
            "dev",
            "draft",
            "submit",
            path.to_str().unwrap(),
            "--producer",
            "codex/test",
        ])
    }

    fn connection(&self) -> (Value, String) {
        let endpoint = self.run(&["daemon", "ensure"]);
        let (status, opened) = post(
            &endpoint,
            "/projects/open",
            Some("app"),
            None,
            json!({"path": self.root.path()}),
        );
        assert_eq!(status, 200, "{opened}");
        (endpoint, opened["handle"].as_str().unwrap().to_owned())
    }

    fn edit(&self, parent: &Value, action: Value) -> Value {
        let (endpoint, handle) = self.connection();
        let (status, draft) = post(
            &endpoint,
            &review_path(&handle, parent),
            Some("app"),
            None,
            json!({"action": action}),
        );
        assert_eq!(status, 200, "{draft}");
        assert_eq!(draft["review"]["actor"], "human:local");
        draft
    }

    fn refs(&self) -> String {
        git(
            self.root.path(),
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/towermd/drafts/",
            ],
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(&["daemon", "stop"]);
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Review Test",
            "-c",
            "user.email=review@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn post(
    endpoint: &Value,
    path: &str,
    client: Option<&str>,
    instance: Option<&str>,
    body: Value,
) -> (u16, Value) {
    let mut request = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
        .header(
            "x-towermd-instance",
            instance.unwrap_or_else(|| endpoint["instance"].as_str().unwrap()),
        );
    if let Some(client) = client {
        request = request.header("x-towermd-client", client);
    }
    let response = request.json(&body).send().unwrap();
    let status = response.status().as_u16();
    let text = response.text().unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or_else(|_| json!(text)),
    )
}

fn review_path(handle: &str, draft: &Value) -> String {
    format!(
        "/projects/{handle}/baselines/{}/review",
        draft["id"].as_str().unwrap()
    )
}

fn node<'a>(draft: &'a Value, id: &str) -> &'a Value {
    draft["view"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == id)
        .unwrap()
}

#[test]
fn app_review_replays_after_restart_without_scan_cache_or_user_file_changes() {
    let fixture = Fixture::new();
    let original = fixture.synthesis();
    let module = original["scan"]["modules"][0]["id"].as_str().unwrap();
    fs::write(
        fixture.root.path().join("README.md"),
        "DIRTY_REFERENCE_SECRET\n",
    )
    .unwrap();
    fs::write(
        fixture.root.path().join("src/alpha/helpers.rs"),
        "// staged user edit\n",
    )
    .unwrap();
    git(fixture.root.path(), &["add", "src/alpha/helpers.rs"]);
    let status = git(fixture.root.path(), &["status", "--porcelain"]);
    let index = fs::read(fixture.root.path().join(".git/index")).unwrap();
    let main = git(fixture.root.path(), &["rev-parse", "main"]);
    let renamed = fixture.edit(
        &original,
        json!({
            "kind": "rename", "node_id": module, "title": "人工审阅后的模块"
        }),
    );
    let moved = fixture.edit(
        &renamed,
        json!({
            "kind": "move", "node_id": module, "x": 1234, "y": -567
        }),
    );
    let referenced = fixture.edit(
        &moved,
        json!({
            "kind": "reference", "path": "README.md", "x": -120, "y": 890
        }),
    );
    assert_eq!(node(&referenced, module)["title"], "人工审阅后的模块");
    assert_eq!(node(&referenced, module)["x"], 1234);
    assert_eq!(node(&referenced, module)["y"], -567);
    let reference = referenced["view"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["file"] == "README.md")
        .unwrap();
    assert_eq!(
        reference["content"],
        "# Committed reference\nOnly this committed text is authoritative.\n"
    );
    assert!(!referenced.to_string().contains("DIRTY_REFERENCE_SECRET"));
    assert_eq!(referenced["submission"], original["submission"]);
    assert_eq!(referenced["scan"], original["scan"]);
    assert_eq!(referenced["review"]["pending_modules"], json!([]));
    assert_eq!(referenced["review"]["actions"].as_array().unwrap().len(), 3);
    fs::remove_dir_all(fixture.root.path().join(".git/towermd/drafts")).unwrap();
    fixture.run(&["daemon", "stop"]);
    assert_eq!(
        fixture.run(&["dev", "draft", "show", referenced["id"].as_str().unwrap()]),
        referenced
    );
    assert_eq!(
        fixture.run(&["dev", "draft", "show", original["id"].as_str().unwrap()]),
        original
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "main"]), main);
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(git(fixture.root.path(), &["status", "--porcelain"]), status);
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("README.md")).unwrap(),
        "DIRTY_REFERENCE_SECRET\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("src/alpha/helpers.rs")).unwrap(),
        "// staged user edit\n"
    );
    assert!(git(fixture.root.path(), &["tag", "--list", "towermd/B*"]).is_empty());
}

#[test]
fn review_rejects_non_app_identity_and_invalid_actions_before_saving() {
    let fixture = Fixture::new();
    // The fixed source has a real Markdown file at a generated object's target
    // path. A reference must not display old text while its canvas path resolves
    // to the synthesized replacement in the private draft commit.
    let collision_path = "docs/tower/arch/A-scan-1.md";
    fs::create_dir_all(fixture.root.path().join("docs/tower/arch")).unwrap();
    fs::write(
        fixture.root.path().join(collision_path),
        "# Existing source document\nMust not alias a synthesized module.\n",
    )
    .unwrap();
    git(fixture.root.path(), &["add", collision_path]);
    git(
        fixture.root.path(),
        &["commit", "-m", "existing reference target"],
    );
    let original = fixture.synthesis();
    assert_eq!(node(&original, "A-scan-1")["file"], "arch/A-scan-1.md");
    let module = original["scan"]["modules"][0]["id"].as_str().unwrap();
    let (endpoint, handle) = fixture.connection();
    let path = review_path(&handle, &original);
    let action = json!({"kind": "rename", "node_id": module, "title": "Reviewed"});
    let refs = fixture.refs();
    for (client, instance) in [
        (Some("cli"), None),
        (None, None),
        (Some("app"), Some("old-instance")),
    ] {
        let (status, response) = post(
            &endpoint,
            &path,
            client,
            instance,
            json!({"action": action}),
        );
        assert_eq!(status, 409, "{response}");
        assert_eq!(fixture.refs(), refs);
    }
    for body in [
        json!({"action": action, "actor": "human:owner"}),
        json!({"action": {"kind":"rename", "node_id":module, "title":"Forged", "actor":"human:owner"}}),
    ] {
        let (status, response) = post(&endpoint, &path, Some("app"), None, body);
        assert_eq!(status, 422, "{response}");
        assert_eq!(fixture.refs(), refs);
    }
    for invalid in [
        json!({"kind":"split", "module_id":module, "new_id":"A-new", "title":"Invalid", "files":["src/absent.rs"]}),
        json!({"kind":"reference", "path":"../README.md", "x":0, "y":0}),
        json!({"kind":"reference", "path":"missing.md", "x":0, "y":0}),
        json!({"kind":"reference", "path":collision_path, "x":0, "y":0}),
    ] {
        let (status, response) = post(
            &endpoint,
            &path,
            Some("app"),
            None,
            json!({"action":invalid}),
        );
        assert_eq!(status, 400, "{response}");
        assert_eq!(fixture.refs(), refs);
    }
    let edited = fixture.edit(&original, action.clone());
    assert_eq!(fixture.edit(&original, action), edited);
    assert_eq!(fixture.refs().lines().count(), 2);
}

#[test]
fn structure_and_command_reviews_mark_pending_without_rewriting_original_evidence() {
    let fixture = Fixture::new();
    let original = fixture.synthesis();
    let modules = original["scan"]["modules"].as_array().unwrap();
    assert_eq!(modules.len(), 3, "{original}");
    let first = modules[0]["id"].as_str().unwrap();
    let second = modules[1]["id"].as_str().unwrap();
    let merged = fixture.edit(
        &original,
        json!({
            "kind":"merge", "module_ids":[first,second], "target_id":first, "title":"Combined"
        }),
    );
    assert_eq!(merged["review"]["pending_modules"], json!([first]));
    assert_eq!(merged["review"]["modules"].as_array().unwrap().len(), 2);
    let split = fixture.edit(
        &original,
        json!({
            "kind":"split", "module_id":first, "new_id":"A-split", "title":"Extracted",
            "files":[modules[0]["files"][0]]
        }),
    );
    let pending = split["review"]["pending_modules"].as_array().unwrap();
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&json!(first)) && pending.contains(&json!("A-split")));
    let commands = fixture.edit(
        &original,
        json!({
            "kind":"commands", "module_id":first,
            "commands":[{"directory":".", "command":"printf changed-validation"}]
        }),
    );
    assert_eq!(commands["review"]["pending_modules"], json!([first]));
    assert_eq!(commands["review"]["confirmed_modules"], json!([first]));
    for draft in [&merged, &split, &commands] {
        assert_eq!(draft["scan"], original["scan"]);
        assert_eq!(draft["submission"], original["submission"]);
        let document =
            towermd_core::okf::Document::parse(node(draft, first)["content"].as_str().unwrap())
                .unwrap();
        assert_eq!(document.get_str("verification"), Some("notRun"));
        assert!(document.get("initial_evidence").is_none());
    }
    assert_eq!(
        fixture.run(&["dev", "draft", "show", original["id"].as_str().unwrap()]),
        original
    );
}

#[test]
fn failed_private_push_preserves_review_and_same_action_retry_publishes_only_that_ref() {
    let fixture = Fixture::new();
    let remote_parent = tempfile::tempdir().unwrap();
    let remote = remote_parent.path().join("remote.git");
    let unavailable = remote_parent.path().join("temporarily-unavailable.git");
    fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--bare", "-b", "main"]);
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(fixture.root.path(), &["push", "-u", "origin", "main"]);
    let main = git(&remote, &["rev-parse", "main"]);
    let original = fixture.synthesis();
    let (endpoint, handle) = fixture.connection();
    let action = json!({
        "kind":"rename", "node_id":original["scan"]["modules"][0]["id"],
        "title":"Survives remote outage"
    });
    fs::rename(&remote, &unavailable).unwrap();
    let (status, response) = post(
        &endpoint,
        &review_path(&handle, &original),
        Some("app"),
        None,
        json!({"action":action}),
    );
    assert_eq!(status, 500, "{response}");
    let drafts = fixture.run(&["dev", "draft", "ls"]);
    let saved = drafts
        .as_array()
        .unwrap()
        .iter()
        .find(|draft| draft["reviewed"] == true)
        .unwrap();
    let saved_id = saved["id"].as_str().unwrap().to_owned();
    let local = fixture.run(&["dev", "draft", "show", &saved_id]);
    assert_eq!(local["review"]["actor"], "human:local");
    assert_eq!(fixture.refs().lines().count(), 2);
    fs::rename(&unavailable, &remote).unwrap();
    let retried = fixture.edit(&original, action);
    assert_eq!(retried, local);
    assert_eq!(retried["id"], saved_id);
    assert_eq!(fixture.refs().lines().count(), 2);
    let reference = format!("refs/towermd/drafts/{saved_id}");
    assert_eq!(
        git(&remote, &["rev-parse", &reference]),
        git(fixture.root.path(), &["rev-parse", &reference])
    );
    assert_eq!(git(&remote, &["rev-parse", "main"]), main);
    assert_eq!(git(fixture.root.path(), &["rev-parse", "main"]), main);
    assert_eq!(
        git(
            &remote,
            &["for-each-ref", "--format=%(refname)", "refs/heads/"]
        ),
        "refs/heads/main"
    );
    assert!(git(&remote, &["tag", "--list"]).is_empty());
}

fn get(endpoint: &Value, path: &str) -> (u16, Value) {
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
        .get(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .header("x-towermd-client", "app")
        .send()
        .unwrap();
    (response.status().as_u16(), response.json().unwrap())
}
fn wait_review(endpoint: &Value, handle: &str, id: &str) -> Value {
    let until = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let (status, job) = get(
            endpoint,
            &format!("/projects/{handle}/baseline-verifications/{id}"),
        );
        assert_eq!(status, 200, "{job}");
        if job["status"] != "running" {
            return job;
        }
        assert!(std::time::Instant::now() < until, "{job}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn app_reverifies_current_commands_once_and_recovers_the_private_result() {
    let fixture = Fixture::new();
    let original = fixture.synthesis();
    let module = original["scan"]["modules"][0]["id"].as_str().unwrap();
    let reviewed = fixture.edit(&original, json!({"kind":"commands","module_id":module,"commands":[{"directory":"","command":"printf 'real revised command\\n'"}]}));
    let before = git(fixture.root.path(), &["rev-parse", "main"]);
    let index = fs::read(fixture.root.path().join(".git/index")).unwrap();
    let (endpoint, handle) = fixture.connection();
    let id = uuid::Uuid::now_v7().to_string();
    let path = format!(
        "/projects/{handle}/baselines/{}/verify",
        reviewed["id"].as_str().unwrap()
    );
    let (denied, _) = post(
        &endpoint,
        &path,
        Some("cli"),
        None,
        json!({"request_id":id}),
    );
    assert_eq!(denied, 409);
    let (status, running) = post(
        &endpoint,
        &path,
        Some("app"),
        None,
        json!({"request_id":id}),
    );
    assert_eq!(status, 200, "{running}");
    assert_eq!(running["id"], id);
    let ready = wait_review(&endpoint, &handle, &id);
    assert_eq!(ready["status"], "ready", "{ready}");
    let result = ready["result_id"].as_str().unwrap();
    let (_, draft) = get(&endpoint, &format!("/projects/{handle}/baselines/{result}"));
    assert_eq!(draft["review"]["verification"]["run_id"], id);
    assert_eq!(draft["review"]["verification"]["parent_id"], reviewed["id"]);
    assert!(draft["review"]["verification"]["report"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["argv"][2] == "printf 'real revised command\\n'"));
    assert_eq!(draft["scan"], original["scan"]);
    assert!(draft["review"]["pending_modules"]
        .as_array()
        .unwrap()
        .is_empty());
    let (_, same) = post(
        &endpoint,
        &path,
        Some("app"),
        None,
        json!({"request_id":id}),
    );
    assert_eq!(same["result_id"], result);
    let other_path = format!(
        "/projects/{handle}/baselines/{}/verify",
        original["id"].as_str().unwrap()
    );
    assert_eq!(
        post(
            &endpoint,
            &other_path,
            Some("app"),
            None,
            json!({"request_id":id})
        )
        .0,
        400
    );
    fixture.run(&["daemon", "stop"]);
    fs::remove_dir_all(fixture.root.path().join(".git/towermd/drafts")).unwrap();
    let (endpoint, handle) = fixture.connection();
    let (_, recovered) = get(
        &endpoint,
        &format!("/projects/{handle}/baseline-verifications/{id}"),
    );
    assert_eq!(recovered["result_id"], result);
    assert_eq!(recovered["phase"], "publishing");
    let path = format!(
        "/projects/{handle}/baselines/{}/verify",
        reviewed["id"].as_str().unwrap()
    );
    let (status, retried) = post(
        &endpoint,
        &path,
        Some("app"),
        None,
        json!({"request_id":id}),
    );
    assert_eq!(status, 200, "{retried}");
    assert_eq!(retried["result_id"], result);
    assert_eq!(retried["status"], "ready");
    assert_eq!(git(fixture.root.path(), &["rev-parse", "main"]), before);
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).unwrap(),
        index
    );
}

#[test]
fn app_cancels_review_worker_and_restart_does_not_claim_passed() {
    let fixture = Fixture::new();
    let original = fixture.synthesis();
    let module = original["scan"]["modules"][0]["id"].as_str().unwrap();
    let reviewed = fixture.edit(&original, json!({"kind":"commands","module_id":module,"commands":[{"directory":"","command":"sleep 30"}]}));
    let (endpoint, handle) = fixture.connection();
    let id = uuid::Uuid::now_v7().to_string();
    let path = format!(
        "/projects/{handle}/baselines/{}/verify",
        reviewed["id"].as_str().unwrap()
    );
    assert_eq!(
        post(
            &endpoint,
            &path,
            Some("app"),
            None,
            json!({"request_id":id})
        )
        .0,
        200
    );
    let cancel_path = format!("/projects/{handle}/baseline-verifications/{id}/cancel");
    let (status, cancelling) = post(&endpoint, &cancel_path, Some("app"), None, json!({}));
    assert_eq!(status, 200, "{cancelling}");
    let cancelled = wait_review(&endpoint, &handle, &id);
    assert_eq!(cancelled["status"], "cancelled", "{cancelled}");
    if let Some(result) = cancelled["result_id"].as_str() {
        let (_, draft) = get(&endpoint, &format!("/projects/{handle}/baselines/{result}"));
        assert!(draft["review"]["pending_modules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == module));
    }
    fixture.run(&["daemon", "stop"]);
    let (endpoint, handle) = fixture.connection();
    let (status, recovered) = get(
        &endpoint,
        &format!("/projects/{handle}/baseline-verifications/{id}"),
    );
    assert_eq!(status, 200, "{recovered}");
    assert_eq!(recovered["status"], "cancelled", "{recovered}");
}

fn publication_fixture(fixture: &Fixture) -> (Value, String, Value) {
    let (endpoint, handle) = fixture.connection();
    let mut draft = fixture.synthesis();
    let modules = draft["scan"]["modules"].as_array().unwrap().clone();
    for module in modules {
        draft = fixture.edit(&draft, json!({"kind":"commands", "module_id":module["id"],
            "commands":[{"directory":"","command":"printf 'actual approved baseline validation\\n'"}]}));
    }
    let id = uuid::Uuid::now_v7().to_string();
    let path = format!(
        "/projects/{handle}/baselines/{}/verify",
        draft["id"].as_str().unwrap()
    );
    assert_eq!(
        post(
            &endpoint,
            &path,
            Some("app"),
            None,
            json!({"request_id":id})
        )
        .0,
        200
    );
    let ready = wait_review(&endpoint, &handle, &id);
    assert_eq!(ready["status"], "ready", "{ready}");
    let (_, result) = get(
        &endpoint,
        &format!(
            "/projects/{handle}/baselines/{}",
            ready["result_id"].as_str().unwrap()
        ),
    );
    (endpoint, handle, result)
}

fn approval_body(preview: &Value) -> Value {
    json!({"request_id":preview["request_id"], "target":preview["target"],
        "prepared_at":preview["prepared_at"],"content_hash":preview["content_hash"]})
}

#[test]
fn app_publication_is_exact_idempotent_and_recovery_requires_the_same_human_request() {
    let fixture = Fixture::new();
    let (endpoint, handle, draft) = publication_fixture(&fixture);
    // A local slow-layer writer must not move a checked-out main.
    git(fixture.root.path(), &["checkout", "-b", "qa-working"]);
    let original_index = fs::read(fixture.root.path().join(".git/index")).unwrap();
    let original_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let prefix = format!(
        "/projects/{handle}/baselines/{}/publication",
        draft["id"].as_str().unwrap()
    );
    assert_eq!(
        post(
            &endpoint,
            &format!("{prefix}/prepare"),
            Some("cli"),
            None,
            json!({})
        )
        .0,
        409
    );
    let (status, preview) = post(
        &endpoint,
        &format!("{prefix}/prepare"),
        Some("app"),
        None,
        json!({}),
    );
    assert_eq!(status, 200, "{preview}");
    assert_eq!(preview["target"], "B0");
    let body = approval_body(&preview);
    assert_eq!(
        post(
            &endpoint,
            &format!("{prefix}/approve"),
            Some("cli"),
            None,
            body.clone()
        )
        .0,
        409
    );
    let mut forged = body.clone();
    forged["actor"] = json!("human:owner");
    assert_eq!(
        post(
            &endpoint,
            &format!("{prefix}/approve"),
            Some("app"),
            None,
            forged
        )
        .0,
        422
    );
    let (status, published) = post(
        &endpoint,
        &format!("{prefix}/approve"),
        Some("app"),
        None,
        body.clone(),
    );
    assert_eq!(status, 200, "{published}");
    let commit = published["commit"].as_str().unwrap();
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "towermd/B0"]),
        commit
    );
    let (status, retry) = post(
        &endpoint,
        &format!("{prefix}/approve"),
        Some("app"),
        None,
        body.clone(),
    );
    assert_eq!(status, 200, "{retry}");
    assert_eq!(retry["commit"], published["commit"]);
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).unwrap(),
        original_index
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        original_head
    );
    let main=git(fixture.root.path(),&["rev-parse","main"]);
    let tree=git(fixture.root.path(),&["rev-parse","main^{tree}"]);
    let unknown=git(fixture.root.path(),&["commit-tree",&tree,"-p",&main,"-m",
        "future extension\n\nTowermd-Future: preserve trailing spaces  \n"]);
    let fake_snapshot=git(fixture.root.path(),&["commit-tree",&tree,"-p",&unknown,"-m",
        "ordinary empty commit, not a snapshot\n\nTowermd-Snapshot: not-a-snapshot"]);
    git(fixture.root.path(),&["update-ref","refs/heads/main",&fake_snapshot,&main]);
    fixture.run(&["daemon", "stop"]);
    fs::remove_dir_all(fixture.root.path().join(".git/towermd/hub")).unwrap();
    let (endpoint, handle) = fixture.connection();
    let (_, state) = get(&endpoint, &format!("/projects/{handle}/state"));
    assert_eq!(state["status"], "needs_recovery", "{state}");
    let (status, request) = post(
        &endpoint,
        &format!("/projects/{handle}/hub/rebuild"),
        Some("cli"),
        None,
        json!({}),
    );
    assert_eq!(status, 200, "{request}");
    let confirm = format!(
        "/projects/{handle}/hub/rebuild/{}/confirm",
        request["request_id"].as_str().unwrap()
    );
    assert_eq!(
        post(&endpoint, &confirm, Some("cli"), None, json!({})).0,
        409
    );
    let (status, recovered) = post(&endpoint, &confirm, Some("app"), None, json!({}));
    assert_eq!(status, 200, "{recovered}");
    assert!(recovered["seq"].as_u64().unwrap() > 1 << 32);
    let (status, retried) = post(&endpoint, &confirm, Some("app"), None, json!({}));
    assert_eq!(status, 200, "{retried}");
    let (_, state) = get(&endpoint, &format!("/projects/{handle}/state"));
    assert_eq!(
        state["projection"]["Baseline"]["B0"]["commit"],
        published["commit"]
    );
    assert!(state["projection"]["Alert"].as_object().unwrap().values().any(|alert|
        alert["data"]["detail"]["trailers"]==json!(["Towermd-Future: preserve trailing spaces  "])));
    assert!(state["projection"]["Alert"].as_object().unwrap().values().any(|alert|
        alert["data"]["reason"]=="invalid_snapshot_publication"
            && alert["data"]["detail"]["commit"]==fake_snapshot
            && alert["data"]["detail"]["severity"]=="warning"));
    let archive=towermd_core::snapshot::Snapshot::read(fixture.root.path(),
        recovered["snapshot_commit"].as_str().unwrap(),"docs/tower","review-integration-project",
        |events|towermd_core::project_runtime::replay(&towermd_devkit::dev_state::initial(),
            events,towermd_devkit::dev_state::apply)).unwrap();
    let originals=towermd_core::project_runtime::canonical_events(&archive.events).unwrap();
    assert!(!originals.contains_key(&towermd_core::events::derived_eid(&fake_snapshot,"snapshot.written").unwrap()));
    fixture.run(&["daemon", "stop"]);
    let (endpoint, handle) = fixture.connection();
    let confirm = format!(
        "/projects/{handle}/hub/rebuild/{}/confirm",
        request["request_id"].as_str().unwrap()
    );
    let (status, resumed) = post(&endpoint, &confirm, Some("app"), None, json!({}));
    assert_eq!(status, 200, "{resumed}");
    assert!(
        resumed["seq"].as_u64().unwrap() < 2 << 32,
        "retry must not allocate another recovery interval"
    );
    let path = format!(
        "/projects/{handle}/baselines/{}/publication/approve",
        draft["id"].as_str().unwrap()
    );
    let (status, approval_retry) = post(&endpoint, &path, Some("app"), None, body);
    assert_eq!(status, 200, "{approval_retry}");
    assert_eq!(approval_retry["commit"], published["commit"]);
}

#[test]
fn remote_publication_reads_authoritative_tag_without_moving_local_checkout() {
    let fixture = Fixture::new();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-b", "main"]);
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(fixture.root.path(), &["push", "-u", "origin", "main"]);
    let (endpoint, handle, draft) = publication_fixture(&fixture);
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let index = fs::read(fixture.root.path().join(".git/index")).unwrap();
    let path = format!(
        "/projects/{handle}/baselines/{}/publication",
        draft["id"].as_str().unwrap()
    );
    let (status, preview) = post(
        &endpoint,
        &format!("{path}/prepare"),
        Some("app"),
        None,
        json!({}),
    );
    assert_eq!(status, 200, "{preview}");
    let body = approval_body(&preview);
    let (status, result) = post(
        &endpoint,
        &format!("{path}/approve"),
        Some("app"),
        None,
        body.clone(),
    );
    assert_eq!(status, 200, "{result}");
    assert_eq!(
        git(remote.path(), &["rev-parse", "towermd/B0"]),
        result["commit"].as_str().unwrap()
    );
    let (status, retry) = post(
        &endpoint,
        &format!("{path}/approve"),
        Some("app"),
        None,
        body,
    );
    assert_eq!(status, 200, "{retry}");
    assert_eq!(retry["commit"], result["commit"]);
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).unwrap(),
        index
    );
}
