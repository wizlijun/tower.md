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
        Command::new(env!("CARGO_BIN_EXE_towermd"))
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
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn synthesis(&self) -> Value {
        let scan = self.run(&["baseline", "scan"]);
        let scan_id = scan["id"].as_str().unwrap();
        let ready = self.run(&["baseline", "scan", "--wait", scan_id]);
        assert_eq!(ready["status"], "ready", "{ready}");
        let input = self.run(&["baseline", "template", scan_id]);
        let path = self.runtime.path().join("synthesis.json");
        fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
        self.run(&[
            "baseline",
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
        fixture.run(&["baseline", "show", referenced["id"].as_str().unwrap()]),
        referenced
    );
    assert_eq!(
        fixture.run(&["baseline", "show", original["id"].as_str().unwrap()]),
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
        fixture.run(&["baseline", "show", original["id"].as_str().unwrap()]),
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
    let drafts = fixture.run(&["baseline", "list"]);
    let saved = drafts
        .as_array()
        .unwrap()
        .iter()
        .find(|draft| draft["reviewed"] == true)
        .unwrap();
    let saved_id = saved["id"].as_str().unwrap().to_owned();
    let local = fixture.run(&["baseline", "show", &saved_id]);
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
