//! Actual native adapters and the staged CLI/daemon, without a WebView or mock server.
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use towermd_app::{
    draft_requests::{self, DraftRequests, EditOutcome},
    project::ProjectRegistry,
    review_requests::{self, ReviewRequests},
};
use towermd_core::{
    baseline_draft::{AnnotationContent, AnnotationNode, AnnotationRelation, DraftEdit},
    baseline_review::Verdict,
    okf::Document,
};

const TEST: &str = "native_edits_and_reviews_roundtrip_through_the_real_daemon";
const CHILD: &str = "TOWERMD_NATIVE_WORKFLOW_CHILD";

struct Fixture {
    root: tempfile::TempDir,
    app_data: tempfile::TempDir,
    runtime: PathBuf,
    cli: PathBuf,
    daemon: Option<Child>,
}
impl Fixture {
    fn new() -> Self {
        let mut f = Self {
            root: tempfile::tempdir().unwrap(),
            app_data: tempfile::tempdir().unwrap(),
            runtime: std::env::var_os("TOWERMD_RUNTIME_DIR").unwrap().into(),
            cli: towermd_app::cli::resolve_cli(&std::env::current_exe().unwrap(), true)
                .expect("stage the current real CLI before running native workflow acceptance"),
            daemon: None,
        };
        f.git(&["init", "-qb", "main"]);
        f.success(&["init", "--docs-dir", "docs/tower"]);
        f.write("README.md", "# Native workflow fixture\n");
        f.write("Cargo.toml", "[package]\nname = \"native-workflow-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n");
        f.write(
            "Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"native-workflow-fixture\"\nversion = \"0.1.0\"\n",
        );
        f.write("src/lib.rs", "pub fn answer() -> u8 { 42 }\n#[test]\nfn actual_test() { assert_eq!(answer(), 42); }\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-qm", "native workflow fixture"]);
        f.start();
        f
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(self.root.path())
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
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.cli);
        command
            .args(args)
            .current_dir(self.root.path())
            .stdin(Stdio::null());
        command
    }
    fn success(&self, args: &[&str]) -> std::process::Output {
        let output = self.command(args).output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    fn cli(&self, args: &[&str]) -> Value {
        serde_json::from_slice(&self.success(args).stdout).unwrap()
    }
    fn start(&mut self) {
        fs::create_dir_all(&self.runtime).unwrap();
        let previous = fs::read(self.runtime.join("endpoint.json")).ok();
        self.daemon = Some(
            self.command(&["daemon", "serve"])
                .stdout(Stdio::null())
                .stderr(fs::File::create(self.runtime.join("workflow-daemon.log")).unwrap())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(bytes) = fs::read(self.runtime.join("endpoint.json")) {
                if Some(&bytes) != previous.as_ref() {
                    break;
                }
            }
            assert!(
                self.daemon.as_mut().unwrap().try_wait().unwrap().is_none(),
                "own daemon exited before its endpoint was ready"
            );
            assert!(Instant::now() < deadline, "own daemon did not start");
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn stop(&mut self) {
        self.success(&["daemon", "stop"]);
        assert!(self.daemon.take().unwrap().wait().unwrap().success());
    }
    fn candidate(&self, id: &str) -> Value {
        let endpoint: Value =
            serde_json::from_slice(&fs::read(self.runtime.join("endpoint.json")).unwrap()).unwrap();
        reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{}/drafts/{id}", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .query(&[("directory", self.root.path())])
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn save_s6(&self) -> String {
        let scan = self.cli(&["baseline", "scan"]);
        let id = scan["scan_id"].as_str().unwrap();
        assert_eq!(
            self.cli(&["baseline", "scan", "--wait", id])["state"],
            "completed"
        );
        let candidate = self.candidate(id);
        let executions = candidate["verification_report"]["executions"]
            .as_array()
            .unwrap();
        assert!(
            !executions.is_empty(),
            "fixture must execute real cargo tests"
        );
        assert!(
            executions
                .iter()
                .all(|execution| execution["result"] == "passed" && execution["exit_code"] == 0),
            "{executions:?}"
        );
        let modules: Vec<_> = candidate["modules"].as_array().unwrap().iter().map(|module| {
            let document = Document::parse(module["text"].as_str().unwrap()).unwrap();
            json!({"id":document.get_str("id").unwrap(), "path":module["path"], "text":module["text"]})
        }).collect();
        let input = json!({"scan_id":id, "project_id":candidate["input"]["project_id"],
            "source_revision":candidate["input"]["revision"], "intents":[], "modules":modules,
            "interfaces":[], "acceptances":[], "questions":["Review this actual native fixture"], "risks":[]});
        let file = self.app_data.path().join("s6.json");
        fs::write(&file, serde_json::to_vec(&input).unwrap()).unwrap();
        let request = uuid::Uuid::now_v7().to_string();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let output = self
                .command(&[
                    "baseline",
                    "submit",
                    file.to_str().unwrap(),
                    "--request-id",
                    &request,
                ])
                .output()
                .unwrap();
            if output.status.success() {
                let saved: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(saved["receipt"]["request_id"], request);
                assert_eq!(
                    saved["receipt"]["result_ref"]["delivery"]["state"],
                    "local_saved"
                );
                return id.into();
            }
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains("repository writer is busy") && Instant::now() < deadline,
                "{error}"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn select(&self) -> (ProjectRegistry, String) {
        let registry = ProjectRegistry::default();
        let token = registry
            .select(Some(self.root.path().to_path_buf()))
            .unwrap()
            .unwrap()
            .token;
        (registry, token)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut daemon) = self.daemon.take() {
            // Only this fixture's directly spawned child, never an inferred PID.
            let _ = daemon.kill();
            let _ = daemon.wait();
        }
    }
}
fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap().into()
}
fn canvas(saved: &Value) -> Value {
    serde_json::from_str(saved["draft"]["bundle"]["canvas_text"].as_str().unwrap()).unwrap()
}
fn put_text(content: &str) -> DraftEdit {
    DraftEdit::PutAnnotation {
        node: AnnotationNode {
            id: "concept".into(),
            x: -120,
            y: 40,
            width: 300,
            height: 180,
            content: AnnotationContent::Text {
                text: content.into(),
            },
        },
    }
}
fn submit_edit(
    f: &Fixture,
    registry: &ProjectRegistry,
    requests: &DraftRequests,
    token: &str,
    request: &str,
) -> EditOutcome {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let outcome =
            draft_requests::submit(registry, requests, token, request, f.app_data.path()).unwrap();
        if outcome.state == "confirmed" {
            return outcome;
        }
        let error = outcome
            .error
            .as_ref()
            .expect("nonconfirmed native request needs a diagnostic");
        assert!(
            outcome.state == "uncertain"
                && error.message.contains("repository writer is busy")
                && Instant::now() < deadline,
            "{outcome:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn native_edits_and_reviews_roundtrip_through_the_real_daemon() {
    if std::env::var_os(CHILD).is_none() {
        let runtime = tempfile::tempdir().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(Path::new(env!("CARGO")).parent().unwrap().to_path_buf())
                .chain(std::env::split_paths(&path)),
        )
        .unwrap();
        command
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD, "1")
            .env("TOWERMD_RUNTIME_DIR", runtime.path())
            .env("USER", "native-workflow-fixture")
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
        ] {
            command.env_remove(name);
        }
        assert!(
            command.status().unwrap().success(),
            "isolated native workflow failed"
        );
        return;
    }
    let mut f = Fixture::new();
    let main = f.git(&["rev-parse", "main"]);
    let status = f.git(&["status", "--porcelain=v1"]);
    let index = fs::read(f.root.path().join(".git/index")).unwrap();
    let id = f.save_s6();
    let (registry, token) = f.select();
    let requests = DraftRequests::default();
    let initial = draft_requests::load(&registry, &token, &id, f.app_data.path()).unwrap();
    let revision = text(&initial, "revision");
    let target = canvas(&initial)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["type"] == "file")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let queued = draft_requests::prepare(
        &registry,
        &requests,
        &token,
        &id,
        &revision,
        put_text("独立概念卡片"),
        f.app_data.path(),
    )
    .unwrap();
    assert_eq!(queued.human_actor, "human:native-workflow-fixture");
    let queued_bytes = fs::read(
        f.app_data
            .path()
            .join(format!("draft-requests/{}.json", queued.request_id)),
    )
    .unwrap();
    drop((registry, requests));
    let (registry, token) = f.select();
    let requests = DraftRequests::default();
    let pending =
        draft_requests::pending(&registry, &requests, &token, &id, f.app_data.path()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, queued.request_id);
    let first = submit_edit(&f, &registry, &requests, &token, &queued.request_id);
    let repeated = submit_edit(&f, &registry, &requests, &token, &queued.request_id);
    assert_eq!(first.response, repeated.response);
    assert_eq!(
        fs::read(
            f.app_data
                .path()
                .join(format!("draft-requests/{}.json", queued.request_id))
        )
        .unwrap(),
        queued_bytes
    );
    let saved = draft_requests::load(&registry, &token, &id, f.app_data.path()).unwrap();
    assert_ne!(saved["revision"], revision);
    let relation = draft_requests::prepare(
        &registry,
        &requests,
        &token,
        &id,
        &text(&saved, "revision"),
        DraftEdit::PutRelation {
            relation: AnnotationRelation {
                id: "ordinary-relation".into(),
                from: "concept".into(),
                to: target,
                label: "relates to".into(),
            },
        },
        f.app_data.path(),
    )
    .unwrap();
    submit_edit(&f, &registry, &requests, &token, &relation.request_id);
    let saved = draft_requests::load(&registry, &token, &id, f.app_data.path()).unwrap();
    let expected_canvas = canvas(&saved);
    assert!(expected_canvas["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["id"] == "concept"
            && node["text"] == "独立概念卡片"
            && node.get("towermd").is_none()));
    assert!(expected_canvas["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["id"] == "ordinary-relation"
            && edge["label"] == "relates to"
            && edge.get("towermd").is_none()));
    assert_eq!(
        saved["draft"]["bundle"]["objects"],
        initial["draft"]["bundle"]["objects"]
    );
    drop((registry, requests));
    f.stop();
    f.start();
    let (registry, token) = f.select();
    let requests = DraftRequests::default();
    let reopened = draft_requests::load(&registry, &token, &id, f.app_data.path()).unwrap();
    assert_eq!(reopened["revision"], saved["revision"]);
    assert_eq!(canvas(&reopened), expected_canvas);
    let reviews = ReviewRequests::default();
    let view = review_requests::load(&registry, &reviews, &token, &id, f.app_data.path()).unwrap();
    assert_eq!(view["verification"]["state"], "available");
    let rejection = review_requests::prepare(
        &registry,
        &reviews,
        &token,
        &text(&view, "review_ticket"),
        Verdict::Rejected,
        "Actual fixture rejection; no UI acceptance claim".into(),
        f.app_data.path(),
    )
    .unwrap();
    drop((registry, reviews));
    let (registry, token) = f.select();
    let reviews = ReviewRequests::default();
    let pending =
        review_requests::pending(&registry, &reviews, &token, &id, f.app_data.path()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, rejection.request_id);
    let rejected = review_requests::submit(
        &registry,
        &reviews,
        &token,
        &rejection.request_id,
        f.app_data.path(),
    )
    .unwrap();
    assert_eq!(rejected.state, "confirmed", "{rejected:?}");
    assert_eq!(
        rejected.response.as_ref().unwrap()["receipt"]["result_ref"]["verdict"],
        "rejected"
    );
    assert_eq!(
        review_requests::submit(
            &registry,
            &reviews,
            &token,
            &rejection.request_id,
            f.app_data.path()
        )
        .unwrap()
        .response,
        rejected.response
    );
    // Rejection is terminal for its binding: a distinct explicit edit and view
    // are needed before the fixture can approve a different revision.
    let edit = draft_requests::prepare(
        &registry,
        &requests,
        &token,
        &id,
        &text(&reopened, "revision"),
        put_text("新修订再审阅"),
        f.app_data.path(),
    )
    .unwrap();
    submit_edit(&f, &registry, &requests, &token, &edit.request_id);
    let view = review_requests::load(&registry, &reviews, &token, &id, f.app_data.path()).unwrap();
    assert_ne!(
        view["binding"],
        serde_json::to_value(&rejection.binding).unwrap()
    );
    let approval = review_requests::prepare(
        &registry,
        &reviews,
        &token,
        &text(&view, "review_ticket"),
        Verdict::Approved,
        "Programmatic approval of this disposable fixture only".into(),
        f.app_data.path(),
    )
    .unwrap();
    let approved = review_requests::submit(
        &registry,
        &reviews,
        &token,
        &approval.request_id,
        f.app_data.path(),
    )
    .unwrap();
    assert_eq!(approved.state, "confirmed", "{approved:?}");
    let response = approved.response.as_ref().unwrap();
    assert_eq!(response["receipt"]["result_ref"]["verdict"], "approved");
    assert_eq!(response["publication"]["state"], "pending");
    assert_eq!(f.git(&["rev-parse", "main"]), main);
    assert!(f.git(&["tag", "--list", "towermd/*"]).is_empty());
    assert_eq!(f.git(&["status", "--porcelain=v1"]), status);
    assert_eq!(fs::read(f.root.path().join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(f.root.path().join("README.md")).unwrap(),
        "# Native workflow fixture\n"
    );
    f.stop();
}
