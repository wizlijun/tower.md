//! Actual B0/Task/Ask producers plus real loopback HTTP. These tests exercise
//! the CLI interaction wire, not an installed CLI binary or GUI acceptance.
use super::super::tests::{snapshot_fixture, snapshot_git, snapshot_server};
use super::*;
#[path = "../../towermd-core/tests/support/baseline_review.rs"]
mod baseline_fixture;
#[path = "../../towermd-core/tests/support/ask.rs"]
pub(super) mod fixture;
use towermd_core::{
    ask_answer_contract::{AskAnswerView, AskOpeningBinding},
    human_intervention::{HumanCommand, ReviewGate},
};

// A one-shot, physical-root-bound fault after the adapter has pinned Catalog.
// Clock callbacks remain pure and cannot accidentally move the fault into the
// project-registration/recovery prelude before current_scan_catalog.
type BeforeAnswerHook = Box<dyn FnOnce(&Catalog) + Send>;
fn answer_hooks() -> &'static Mutex<BTreeMap<PathBuf, BeforeAnswerHook>> {
    static HOOKS: std::sync::OnceLock<Mutex<BTreeMap<PathBuf, BeforeAnswerHook>>> =
        std::sync::OnceLock::new();
    HOOKS.get_or_init(Default::default)
}
pub(super) fn before_answer(catalog: &Catalog) {
    let hook = answer_hooks().lock().unwrap().remove(&catalog.root);
    if let Some(hook) = hook {
        hook(catalog);
    }
}
struct AnswerHookGuard(PathBuf);
impl Drop for AnswerHookGuard {
    fn drop(&mut self) {
        answer_hooks().lock().unwrap().remove(&self.0);
    }
}
fn before_answer_once(root: &Path, hook: BeforeAnswerHook) -> AnswerHookGuard {
    let guard = AnswerHookGuard(root.to_owned());
    let previous = answer_hooks().lock().unwrap().insert(root.to_owned(), hook);
    assert!(previous.is_none());
    guard
}

fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-towermd-instance",
        "snapshot-schedule-test".parse().unwrap(),
    );
    headers.insert("x-towermd-client", "app".parse().unwrap());
    headers
}
async fn response(request: reqwest::RequestBuilder) -> (StatusCode, Value) {
    let response = request.send().await.unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"error":String::from_utf8_lossy(&bytes)})),
    )
}
fn adopt(f: fixture::Fixture) -> (tempfile::TempDir, tempfile::TempDir, Catalog, Arc<Server>) {
    let fixture::Fixture {
        root,
        runtime,
        catalog,
        session,
        outbox,
        writer,
        ..
    } = f;
    let project_id = catalog.config.project_id().to_owned();
    // Release the actual old owner before opening a daemon Session. The same
    // complete real durable records move once into the daemon's normal runtime
    // location; no cached Session is rebound and no receipt is manufactured.
    drop(session);
    drop(outbox);
    drop(writer);
    let directory = runtime.path().join("recovery-outbox");
    fs::create_dir(&directory).unwrap();
    fs::rename(runtime.path().join("outbox"), directory.join(project_id)).unwrap();
    let mut server = snapshot_server(
        root.path(),
        runtime.path().to_owned(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::NOW + 40)),
    );
    Arc::get_mut(&mut server).unwrap().endpoint.host = "host".into();
    let catalog = Catalog::load(root.path()).unwrap();
    (root, runtime, catalog, server)
}

#[test]
fn finite_app_action_keeps_review_wire_and_rejects_answer_review_or_actor_mixtures() {
    let answer = json!({"directory":"/owned/project","request_id":Uuid::now_v7().to_string(),"human_id":"owner","action":{"action":"answer","hold":"H","opening":{"eid":Uuid::now_v7().to_string(),"sha256":"a".repeat(64)},"text":"answer"}});
    assert!(serde_json::from_value::<human_routes::ReviewActionRequest>(answer.clone()).is_ok());
    let mut review = answer.clone();
    review["action"] = json!({"kind":"review","hold":"H","binding":{"run":"R","task":"T","baseline":"B0","fences":[],"subject":{"kind":"plan","sha256":"a".repeat(64)}},"verdict":"approved","comment":"original wire"});
    assert!(serde_json::from_value::<human_routes::ReviewActionRequest>(review.clone()).is_ok());
    for (outer, key, value) in [
        (true, "actor", json!("human:forged")),
        (false, "kind", json!("review")),
        (false, "binding", review["action"]["binding"].clone()),
        (false, "guards", json!([])),
        (false, "raw_hold", json!({})),
    ] {
        let mut mixed = answer.clone();
        if outer {
            mixed[key] = value;
        } else {
            mixed["action"][key] = value;
        }
        assert!(
            serde_json::from_value::<human_routes::ReviewActionRequest>(mixed).is_err(),
            "{key}"
        );
    }
    review["action"]["action"] = json!("answer");
    assert!(serde_json::from_value::<human_routes::ReviewActionRequest>(review).is_err());
}

#[tokio::test]
async fn ask_read_with_missing_hub_is_readonly_and_does_not_register_or_create_storage() {
    let (temp, root) = snapshot_fixture(7200);
    let catalog = Catalog::load(&root).unwrap();
    let directory = catalog.common_dir.join("towermd");
    assert!(!directory.exists());
    let server = snapshot_server(
        &root,
        temp.path().join("runtime"),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::NOW)),
    );
    let result = load(
        State(server.clone()),
        headers(),
        RoutePath("H-missing".into()),
        Query(AskQuery { directory: root }),
    )
    .await;
    assert_eq!(result.0, StatusCode::BAD_REQUEST);
    assert!(result.1 .0["error"]
        .as_str()
        .unwrap()
        .contains("读取不会创建"));
    assert!(!directory.exists());
    assert!(server.projects.lock().unwrap().is_empty());
    assert!(!server.runtime.join("recovery-outbox").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_app_gate_answer_and_process_wait_use_one_human_receipt_without_review_authority() {
    let mut f = fixture::claimed();
    let opened = fixture::call(
        &mut f,
        &Uuid::now_v7().to_string(),
        &fixture::open("实际App回答HTTP"),
        fixture::NOW + 20,
    );
    let review = ProjectSession::request_task_review_shared(
        &f.session,
        &f.catalog,
        "host",
        &Uuid::now_v7().to_string(),
        &HumanCommand::ReviewRequest {
            gate: ReviewGate::G1,
            content: Some("原计划，不得用Ask回答批准".into()),
            source_commit: None,
            source_path: None,
        },
        None,
        &mut f.outbox,
        || Ok(fixture::NOW + 21),
    )
    .unwrap();
    let review_hold = review.receipt.result_ref.hold;
    let (_root, _runtime, catalog, server) = adopt(f);
    let source_changed = Arc::new(AtomicBool::new(false));
    let project = server.project(&catalog).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let app = router(server.clone());
    let serving = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .default_headers(headers())
        .build()
        .unwrap();
    let read = response(
        client
            .get(format!("{base}/app/interventions/asks/{}", opened.hold))
            .query(&[("directory", catalog.root.to_string_lossy().to_string())]),
    )
    .await;
    assert_eq!(read.0, StatusCode::OK, "{}", read.1);
    let view: AskAnswerView = serde_json::from_value(read.1).unwrap();
    let id = Uuid::now_v7().to_string();
    let body = json!({"directory":catalog.root,"request_id":id,"human_id":"owner","action":{"action":"answer","hold":opened.hold,"opening":view.binding,"text":"保留原文\r\n只答本次"}});
    let cursor = project
        .hub
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let plain = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let no_app = response(
        plain
            .post(format!("{base}/app/interventions/action"))
            .header("x-towermd-instance", "snapshot-schedule-test")
            .json(&body),
    )
    .await;
    assert_eq!(no_app.0, StatusCode::FORBIDDEN);
    let wrong_instance = response(
        client
            .post(format!("{base}/app/interventions/action"))
            .header("x-towermd-instance", "other-instance")
            .json(&body),
    )
    .await;
    assert_eq!(wrong_instance.0, StatusCode::CONFLICT);
    for wrong in [
        "actor",
        "mixed",
        "identity",
        "command",
        "interaction",
        "review",
    ] {
        let mut bad = body.clone();
        bad["request_id"] = json!(Uuid::now_v7().to_string());
        let mut path = "/app/interventions/action";
        match wrong {
            "actor" => bad["actor"] = json!("human:forged"),
            "mixed" => bad["action"]["kind"] = json!("review"),
            "identity" => bad["human_id"] = json!("human:owner"),
            "command" => {
                path = "/command";
                bad = json!({"directory":catalog.root,"run":null,"command":body["action"]});
            }
            "interaction" => {
                path = "/interaction";
                bad = json!({"directory":catalog.root,"run":null,"request_id":Uuid::now_v7().to_string(),"command":body["action"]});
            }
            "review" => {
                let opening = project
                    .hub
                    .lock()
                    .unwrap()
                    .human()
                    .unwrap()
                    .projection()
                    .1
                    .openings[&review_hold]
                    .clone();
                bad["action"]["hold"] = json!(review_hold);
                bad["action"]["opening"] =
                    serde_json::to_value(AskOpeningBinding::from_opening(&opening).unwrap())
                        .unwrap();
            }
            _ => unreachable!(),
        }
        let refused = response(client.post(format!("{base}{path}")).json(&bad)).await;
        assert!(
            refused.0.is_client_error(),
            "{wrong}: {} {}",
            refused.0,
            refused.1
        );
    }
    assert!(project
        .hub
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(cursor, fixture::NOW + 40)
        .unwrap()
        .events
        .is_empty());
    let wait_id = Uuid::now_v7().to_string();
    let wait_body = json!({"directory":catalog.root,"run":null,"request_id":wait_id,"command":{"action":"ask_wait","hold":opened.hold}});
    let waiting_client = plain.clone();
    let waiting_url = format!("{base}/interaction");
    let waiting = tokio::spawn(async move {
        response(
            waiting_client
                .post(waiting_url)
                .header("x-towermd-instance", "snapshot-schedule-test")
                .json(&wait_body),
        )
        .await
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if server
            .recovery_inputs(&catalog)
            .unwrap()
            .confirmed()
            .iter()
            .any(|e| e.kind == "hold.wait_started" && e.data["p3_request"]["request_id"] == wait_id)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual process wait never entered its durable slice"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let answered = response(
        client
            .post(format!("{base}/app/interventions/action"))
            .json(&body),
    )
    .await;
    assert_eq!(answered.0, StatusCode::OK, "{}", answered.1);
    assert_eq!(
        answered.1["receipt"]["result_ref"]["reply"]["actor"],
        "human:owner"
    );
    let wait = waiting.await.unwrap();
    assert_eq!(wait.0, StatusCode::OK, "{}", wait.1);
    assert_eq!(wait.1["current"]["state"], "answered");
    assert_eq!(
        wait.1["current"]["reply"],
        answered.1["receipt"]["result_ref"]["reply"]
    );
    let retry = response(
        client
            .post(format!("{base}/app/interventions/action"))
            .json(&body),
    )
    .await;
    assert_eq!(retry.0, StatusCode::OK, "{}", retry.1);
    assert_eq!(retry.1, answered.1);
    let old_main = snapshot_git(&catalog.root, &["rev-parse", "main"]);
    let expected = old_main.clone();
    let reached = source_changed.clone();
    let race_root = catalog.root.clone();
    let _source_hook = before_answer_once(
        &catalog.root,
        Box::new(move |pinned| {
            assert_eq!(pinned.root, race_root);
            assert_eq!(
                pinned.revision, expected,
                "fault must follow the actual Catalog pin"
            );
            let parent = snapshot_git(&race_root, &["rev-parse", "main"]);
            assert_eq!(parent, pinned.revision);
            let tree = snapshot_git(&race_root, &["rev-parse", "main^{tree}"]);
            let commit = snapshot_git(
                &race_root,
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    &parent,
                    "-m",
                    "owned App Answer source race",
                ],
            );
            snapshot_git(
                &race_root,
                &["update-ref", "refs/heads/main", &commit, &parent],
            );
            reached.store(true, Ordering::SeqCst);
        }),
    );
    let uncertain = response(
        client
            .post(format!("{base}/app/interventions/action"))
            .json(&body),
    )
    .await;
    assert!(source_changed.load(Ordering::SeqCst));
    assert_eq!(
        uncertain.0,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        uncertain.1
    );
    assert_eq!(uncertain.1["request_id"], id);
    assert_eq!(uncertain.1["code"], "request_uncertain");
    snapshot_git(&catalog.root, &["update-ref", "refs/heads/main", &old_main]);
    let repaired = response(
        client
            .post(format!("{base}/app/interventions/action"))
            .json(&body),
    )
    .await;
    assert_eq!(repaired.0, StatusCode::OK, "{}", repaired.1);
    assert_eq!(repaired.1, answered.1);
    let mut changed = body.clone();
    changed["action"]["text"] = json!("changed original");
    assert_eq!(
        response(
            client
                .post(format!("{base}/app/interventions/action"))
                .json(&changed)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let inputs = server.recovery_inputs(&catalog).unwrap();
    let actual = inputs
        .confirmed()
        .iter()
        .filter(|e| e.kind == "hold.answered" && e.obj == opened.hold)
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), 1);
    assert!(project
        .hub
        .lock()
        .unwrap()
        .human()
        .unwrap()
        .require_human_review(&review.receipt.result_ref.binding)
        .is_err());
    stop.send(()).unwrap();
    serving.await.unwrap();
    server.shutdown.send_replace(true);
    server.main_refresh.drain().await.unwrap();
    server.drain_snapshots().await.unwrap();
}
