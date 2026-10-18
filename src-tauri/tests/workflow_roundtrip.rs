//! Actual native adapters and the staged CLI/daemon, without a WebView or mock server.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
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
    baseline_draft::{
        AnnotationContent, AnnotationNode, AnnotationRelation, DraftEdit, DraftObject, SplitPart,
        VerifyCommand,
    },
    baseline_review::Verdict,
    okf::Document,
};

const TEST: &str = "native_edits_and_reviews_roundtrip_through_the_real_daemon";
const CHILD: &str = "TOWERMD_NATIVE_WORKFLOW_CHILD";
const ACCEPTANCE: &str = "native-workflow-acceptance";
const INTERFACE: &str = "native-workflow-interface";
const PART: &str = "native-workflow-split-part";

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
        // Root Module paths enumerate source files rather than the manifest.
        // Two real roots give Split scopes without inventing scan coverage.
        f.write("fixture_probe.rs", "pub const SCANNED_ROOT: bool = true;\n");
        f.write("fixture_scope.rs", "pub const SECOND_SCOPE: u8 = 2;\n");
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
        let splittable = modules
            .iter()
            .find(|module| {
                let document = Document::parse(module["text"].as_str().unwrap()).unwrap();
                serde_json::to_value(document.get("paths").unwrap())
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .len()
                    >= 2
            })
            .expect("actual scan must provide a Module with at least two paths");
        assert!(modules.len() >= 2, "Interface needs another actual Module");
        let acceptance = json!({"id": ACCEPTANCE,
            "path":format!("specs/{ACCEPTANCE}.md"),
            "text":format!("---\ntype: Acceptance\nid: {ACCEPTANCE}\ntitle: Native workflow criterion\nmodule: {}\ncriteria: ['The actual fixture test passes']\n---\n", splittable["id"].as_str().unwrap())});
        let input = json!({"scan_id":id, "project_id":candidate["input"]["project_id"],
            "source_revision":candidate["input"]["revision"], "intents":[], "modules":modules,
            "interfaces":[], "acceptances":[acceptance], "questions":["Review this actual native fixture"], "risks":[]});
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
            // A fast-state snapshot may move the proof source between probes.
            // Retry only this exact original request; the next proof must still
            // validate the fixed S6 source and the full protected map.
            let source_race = error.contains(&format!(
                "request_id={request}：recovery sources changed during proof；"
            ));
            assert!(
                (error.contains("repository writer is busy") || source_race)
                    && Instant::now() < deadline,
                "{error}"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn edit(
        &self,
        registry: &ProjectRegistry,
        requests: &DraftRequests,
        token: &str,
        before: &Value,
        edit: DraftEdit,
    ) -> Value {
        eprintln!("native typed edit: {edit:?}");
        let id = before["id"].as_str().unwrap();
        let request = draft_requests::prepare(
            registry,
            requests,
            token,
            id,
            &text(before, "revision"),
            edit,
            self.app_data.path(),
        )
        .unwrap();
        let outcome = submit_edit(self, registry, requests, token, &request.request_id);
        let saved = draft_requests::load(registry, token, id, self.app_data.path()).unwrap();
        assert_ne!(saved["revision"], before["revision"]);
        assert_eq!(saved["current_validity"]["state"], "current");
        assert_eq!(
            saved["revision"],
            outcome.response.unwrap()["receipt"]["result_ref"]["revision"]
        );
        let reference = format!("refs/towermd/drafts/{id}");
        assert_eq!(
            self.git(&["rev-parse", &reference]),
            text(&saved, "revision")
        );
        assert_eq!(
            self.git(&["show", &format!("{reference}:docs/tower/tower.canvas")]),
            saved["draft"]["bundle"]["canvas_text"]
                .as_str()
                .unwrap()
                .trim_end_matches('\n')
        );
        saved
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
fn document(saved: &Value, id: &str) -> Document {
    Document::parse(
        saved["draft"]["bundle"]["objects"][id]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}
fn strings(document: &Document, field: &str) -> Vec<String> {
    serde_json::to_value(document.get(field).unwrap())
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}
fn object_node(saved: &Value, id: &str) -> String {
    canvas(saved)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["towermd"]["obj"] == id)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn structural_edits(
    f: &Fixture,
    registry: &ProjectRegistry,
    requests: &DraftRequests,
    token: &str,
    initial: &Value,
) -> Value {
    let module = document(initial, ACCEPTANCE)
        .get_str("module")
        .unwrap()
        .to_owned();
    let paths = strings(&document(initial, &module), "paths");
    assert!(paths.len() >= 2);
    let other = initial["draft"]["bundle"]["objects"]
        .as_object()
        .unwrap()
        .keys()
        .find(|id| id.as_str() != module && document(initial, id).doc_type() == Some("Module"))
        .unwrap()
        .to_owned();
    let original_node = object_node(initial, &module);
    let renamed = f.edit(
        registry,
        requests,
        token,
        initial,
        DraftEdit::Rename {
            object: module.clone(),
            title: "Renamed actual module".into(),
        },
    );
    assert_eq!(
        document(&renamed, &module).get_str("title"),
        Some("Renamed actual module")
    );
    let moved = f.edit(
        registry,
        requests,
        token,
        &renamed,
        DraftEdit::Move {
            node: original_node.clone(),
            x: -360,
            y: 280,
        },
    );
    assert!(canvas(&moved)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["id"] == original_node && node["x"] == -360 && node["y"] == 280));
    let commands = vec![VerifyCommand {
        cwd: ".".into(),
        argv: vec!["cargo".into(), "test".into(), "--locked".into()],
    }];
    let verified = f.edit(
        registry,
        requests,
        token,
        &moved,
        DraftEdit::ConfirmVerify {
            module: module.clone(),
            commands: commands.clone(),
        },
    );
    assert_eq!(
        serde_json::to_value(document(&verified, &module).get("verify").unwrap()).unwrap(),
        serde_json::to_value(&commands).unwrap()
    );
    let linked = f.edit(registry, requests, token, &verified,
        DraftEdit::PutInterface { object:DraftObject {
            id:INTERFACE.into(), path:format!("docs/tower/arch/{INTERFACE}.md"),
            text:format!("---\ntype: Interface\nid: {INTERFACE}\ntitle: Actual native contract\ncontract: README.md\n---\n") },
            from:module.clone(), to:other.clone() });
    assert_eq!(
        document(&linked, INTERFACE).get_str("from"),
        Some(module.as_str())
    );
    assert_eq!(
        document(&linked, INTERFACE).get_str("to"),
        Some(other.as_str())
    );
    assert!(
        canvas(&linked)["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["towermd"]["obj"] == INTERFACE
                && edge["label"] == "Actual native contract"),
        "FR-101 requires the actual Interface title on its relation edge"
    );
    for endpoint in [&module, &other] {
        assert!(strings(&document(&linked, endpoint), "interfaces").contains(&INTERFACE.into()));
    }
    let split = f.edit(
        registry,
        requests,
        token,
        &linked,
        DraftEdit::Split {
            module: module.clone(),
            parts: vec![
                SplitPart {
                    id: module.clone(),
                    title: "First actual scope".into(),
                    paths: vec![paths[0].clone()],
                    verify: commands.clone(),
                    acceptance_ids: vec![],
                    x: -360,
                    y: 280,
                },
                SplitPart {
                    id: PART.into(),
                    title: "Second actual scope".into(),
                    paths: paths[1..].to_vec(),
                    verify: commands.clone(),
                    acceptance_ids: vec![ACCEPTANCE.into()],
                    x: 40,
                    y: 280,
                },
            ],
            interface_endpoints: BTreeMap::from([(INTERFACE.into(), [PART.into(), other.clone()])]),
        },
    );
    assert_eq!(strings(&document(&split, &module), "paths"), paths[..1]);
    assert_eq!(strings(&document(&split, PART), "paths"), paths[1..]);
    assert_eq!(document(&split, ACCEPTANCE).get_str("module"), Some(PART));
    assert_eq!(document(&split, INTERFACE).get_str("from"), Some(PART));
    assert!(strings(&document(&split, &module), "interfaces").is_empty());
    assert_eq!(strings(&document(&split, PART), "interfaces"), [INTERFACE]);
    assert!(canvas(&split)["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["towermd"]["obj"] == INTERFACE
            && edge["fromNode"] == object_node(&split, PART)
            && edge["toNode"] == object_node(&split, &other)));
    let removed = f.edit(
        registry,
        requests,
        token,
        &split,
        DraftEdit::RemoveInterface {
            interface: INTERFACE.into(),
        },
    );
    assert!(removed["draft"]["bundle"]["objects"]
        .get(INTERFACE)
        .is_none());
    assert!(canvas(&removed)["edges"]
        .as_array()
        .unwrap()
        .iter()
        .all(|edge| edge["towermd"]["obj"] != INTERFACE));
    for endpoint in [PART, other.as_str()] {
        assert!(strings(&document(&removed, endpoint), "interfaces").is_empty());
    }
    assert!(
        !removed["draft"]["bundle"]["deletions"]
            .as_array()
            .unwrap()
            .contains(&json!(format!("docs/tower/arch/{INTERFACE}.md"))),
        "removing a newly drafted Interface cancels creation, not an actual source deletion"
    );
    let merged = f.edit(
        registry,
        requests,
        token,
        &removed,
        DraftEdit::Merge {
            modules: vec![module.clone(), PART.into()],
            keep: module.clone(),
            title: "Merged actual scopes".into(),
        },
    );
    assert_eq!(
        document(&merged, &module).get_str("title"),
        Some("Merged actual scopes")
    );
    let mut merged_paths = strings(&document(&merged, &module), "paths");
    let mut expected_paths = paths;
    merged_paths.sort();
    expected_paths.sort();
    assert_eq!(merged_paths, expected_paths);
    assert!(merged["draft"]["bundle"]["objects"].get(PART).is_none());
    assert!(
        !merged["draft"]["bundle"]["deletions"]
            .as_array()
            .unwrap()
            .contains(&json!(format!("docs/tower/arch/{PART}.md"))),
        "merging a newly split part cancels creation without a fictitious source deletion"
    );
    assert_eq!(
        document(&merged, ACCEPTANCE).get_str("module"),
        Some(module.as_str())
    );
    assert!(canvas(&merged)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node["towermd"]["obj"] != PART));
    let referenced = f.edit(
        registry,
        requests,
        token,
        &merged,
        DraftEdit::AddDocref {
            node: "native-readme-reference".into(),
            path: "README.md".into(),
            x: 480,
            y: -100,
        },
    );
    assert!(canvas(&referenced)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["id"] == "native-readme-reference"
            && node["file"] == "README.md"
            && node.get("towermd").is_none()));
    assert_eq!(
        referenced["draft"]["bundle"]["objects"],
        merged["draft"]["bundle"]["objects"]
    );
    referenced
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
    submit_draft_request(f, registry, requests, token, request, false)
}

fn submit_draft_request(
    f: &Fixture,
    registry: &ProjectRegistry,
    requests: &DraftRequests,
    token: &str,
    request: &str,
    creation: bool,
) -> EditOutcome {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let outcome = if creation {
            draft_requests::submit_create(registry, requests, token, request, f.app_data.path())
        } else {
            draft_requests::submit(registry, requests, token, request, f.app_data.path())
        }
        .unwrap();
        assert_eq!(
            outcome.request_id, request,
            "coordination must keep the original UUID"
        );
        if outcome.state == "confirmed" {
            return outcome;
        }
        let error = outcome
            .error
            .as_ref()
            .expect("nonconfirmed native request needs a diagnostic");
        let original = error
            .detail
            .as_deref()
            .and_then(|detail| serde_json::from_str::<Value>(detail).ok());
        assert!(
            outcome.state == "uncertain"
                && error.code == "request_uncertain"
                && original.as_ref().is_some_and(|body| {
                    body["request_id"] == request
                        && matches!(
                            body["code"].as_str(),
                            Some("writer_busy" | "request_uncertain")
                        )
                })
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
    let initial = structural_edits(&f, &registry, &requests, &token, &initial);
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
    assert_eq!(
        reopened["draft"]["bundle"]["objects"],
        saved["draft"]["bundle"]["objects"]
    );
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

    // Release only this disposable fixture's main, then coordinate the already
    // accepted original approval. No new human decision is invented to publish.
    f.git(&["switch", "-qc", "native-current-draft"]);
    let deadline = Instant::now() + Duration::from_secs(30);
    let published = loop {
        let result = review_requests::submit(
            &registry,
            &reviews,
            &token,
            &approval.request_id,
            f.app_data.path(),
        )
        .unwrap();
        assert_eq!(result.request_id, approval.request_id);
        if result.state == "confirmed" {
            let response = result.response.unwrap();
            assert_eq!(
                response["receipt"],
                approved.response.as_ref().unwrap()["receipt"]
            );
            if response["publication"]["state"] == "recorded" {
                break response;
            }
            assert_eq!(response["publication"]["state"], "pending", "{response}");
        } else {
            assert_eq!(result.state, "uncertain", "{result:?}");
        }
        assert!(
            Instant::now() < deadline,
            "original B0 publication did not settle"
        );
        thread::sleep(Duration::from_millis(50));
    };
    let b0 = f.git(&["rev-parse", "refs/tags/towermd/B0^{commit}"]);
    assert_eq!(published["publication"]["created"]["commit"], b0);
    // Freeze main for the following private-draft assertions. Otherwise the
    // independent fast-state snapshot worker can legitimately advance main.
    f.git(&["switch", "main"]);
    let source_main = f.git(&["rev-parse", "main"]);
    f.git(&["merge-base", "--is-ancestor", &b0, &source_main]);
    let user_head = f.git(&["rev-parse", "HEAD"]);
    let user_index = fs::read(f.root.path().join(".git/index")).unwrap();
    let create_status = f.git(&["status", "--porcelain=v1"]);
    eprintln!("native current draft: actual B0 published, loading authority");
    let current = draft_requests::load_current(&registry, &token, f.app_data.path())
        .unwrap()
        .unwrap();
    assert_eq!(current.baseline, "towermd/B0");
    assert_eq!(current.main_revision, source_main);
    let creation = draft_requests::prepare_create(
        &registry,
        &requests,
        &token,
        &current.baseline,
        &current.main_revision,
        f.app_data.path(),
    )
    .unwrap();
    assert_eq!(creation.draft_id, creation.request_id);
    let queue_path = f
        .app_data
        .path()
        .join("draft-requests")
        .join(format!("{}.json", creation.request_id));
    let queued = fs::read(&queue_path).unwrap();
    drop((registry, requests, reviews));
    f.stop();
    f.start();
    let (registry, token) = f.select();
    let requests = DraftRequests::default();
    let pending =
        draft_requests::pending_creations(&registry, &requests, &token, f.app_data.path()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, creation.request_id);
    assert_eq!(pending[0].main_revision, source_main);
    assert_eq!(fs::read(&queue_path).unwrap(), queued);
    assert!(draft_requests::pending(
        &registry,
        &requests,
        &token,
        &creation.draft_id,
        f.app_data.path()
    )
    .unwrap()
    .is_empty());
    eprintln!("native current draft: original creation restored, submitting same UUID");
    let created =
        submit_draft_request(&f, &registry, &requests, &token, &creation.request_id, true);
    let again = submit_draft_request(&f, &registry, &requests, &token, &creation.request_id, true);
    assert_eq!(
        created.response.as_ref().unwrap()["receipt"],
        again.response.as_ref().unwrap()["receipt"]
    );
    let draft =
        draft_requests::load(&registry, &token, &creation.draft_id, f.app_data.path()).unwrap();
    assert_eq!(draft["draft"]["bundle"]["source_revision"], source_main);
    assert_eq!(draft["draft"]["bundle"]["target_baseline"], "towermd/B1");
    assert_eq!(draft["current_validity"]["state"], "current");
    assert!(
        draft_requests::pending_creations(&registry, &requests, &token, f.app_data.path())
            .unwrap()
            .is_empty()
    );
    let module = draft["object_views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["type"] == "Module")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let edited = f.edit(
        &registry,
        &requests,
        &token,
        &draft,
        DraftEdit::Rename {
            object: module.into(),
            title: "Edited after approved B0".into(),
        },
    );
    assert_eq!(
        document(&edited, module).get_str("title"),
        Some("Edited after approved B0")
    );
    let other = edited["object_views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["type"] == "Module" && object["id"] != module)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let edited = f.edit(
        &registry,
        &requests,
        &token,
        &edited,
        DraftEdit::PutInterface {
            object: DraftObject {
                id: INTERFACE.into(),
                path: format!("docs/tower/arch/{INTERFACE}.md"),
                text: format!("---\ntype: Interface\nid: {INTERFACE}\ntitle: Review canvas contract\ncontract: README.md\n---\n"),
            },
            from: module.into(),
            to: other.clone(),
        },
    );
    drop((registry, requests));
    f.stop();
    f.start();
    let (registry, token) = f.select();
    let reopened =
        draft_requests::load(&registry, &token, &creation.draft_id, f.app_data.path()).unwrap();
    assert_eq!(reopened["draft"], edited["draft"]);
    assert_eq!(reopened["revision"], edited["revision"]);
    let reviews = ReviewRequests::default();
    let view = review_requests::load(
        &registry,
        &reviews,
        &token,
        &creation.draft_id,
        f.app_data.path(),
    )
    .unwrap();
    assert_eq!(view["main_revision"], source_main);
    let comparison = &view["canvas_comparison"];
    assert_eq!(comparison["state"], "available");
    assert_eq!(comparison["main_revision"], source_main);
    assert_eq!(comparison["baseline"], "towermd/B0");
    assert_eq!(
        comparison["draft_canvas"]["canvas_view"],
        reopened["canvas_view"]
    );
    assert_eq!(
        comparison["draft_canvas"]["object_views"],
        reopened["object_views"]
    );
    let actual_main_canvas = towermd_core::canvas::CanvasDocument::decode(
        &f.git(&["show", &format!("{source_main}:docs/tower/tower.canvas")]),
    )
    .unwrap();
    for (key, entries) in [
        ("nodes", actual_main_canvas.nodes()),
        ("edges", actual_main_canvas.edges()),
    ] {
        let expected: Vec<_> = entries
            .iter()
            .map(|entry| json!({"index":entry.index,"opaque":entry.opaque,"raw":entry.raw}))
            .collect();
        assert_eq!(
            comparison["baseline_canvas"]["canvas_view"][key],
            json!(expected)
        );
    }
    assert!(comparison["baseline_canvas"]["object_views"]
        .as_array()
        .unwrap()
        .iter()
        .all(|object| object["id"] != INTERFACE));
    let affected: std::collections::BTreeSet<_> = view["interface_modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    assert_eq!(
        affected,
        std::collections::BTreeSet::from([module, other.as_str()])
    );
    assert!(!view["publish_blockers"].as_array().unwrap().is_empty());
    assert!(review_requests::prepare(
        &registry,
        &reviews,
        &token,
        &text(&view, "review_ticket"),
        Verdict::Approved,
        "must not bypass Proposal".into(),
        f.app_data.path()
    )
    .is_err());
    assert_eq!(f.git(&["rev-parse", "main"]), source_main);
    assert!(f.git(&["tag", "--list", "towermd/B1"]).is_empty());
    assert_eq!(f.git(&["rev-parse", "HEAD"]), user_head);
    assert_eq!(
        fs::read(f.root.path().join(".git/index")).unwrap(),
        user_index
    );
    assert_eq!(f.git(&["status", "--porcelain=v1"]), create_status);
    assert_eq!(fs::read(&queue_path).unwrap(), queued);
    f.stop();
}
