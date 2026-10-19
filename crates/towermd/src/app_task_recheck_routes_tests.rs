//! Real B1 producers and loopback fixed AppGate/CLI claim wire. Not GUI evidence.
use super::*;
use crate::daemon::app_answer_routes::{claimed, Fixture, NOW};
use crate::daemon::tests::{snapshot_fixture, snapshot_server};
use towermd_core::{
    baseline_review::{self, ReviewAction, Verdict},
    proposals::{Alternative, ProposalInput, ProposalSource},
    snapshot_service::PublicationReport,
    tasks::TaskSpec,
};
fn git(root: &Path, args: &[&str]) -> String {
    crate::daemon::tests::snapshot_git(root, args)
}
fn actual_b1_ready() -> (tempfile::TempDir, tempfile::TempDir, Arc<Server>) {
    let mut f = claimed();
    let base = f.catalog.revision.clone();
    let path = f.catalog.object_paths["A-alpha"].clone();
    let source = fs::read_to_string(f.root.path().join(&path)).unwrap();
    fs::write(
        f.root.path().join(&path),
        format!("{source}\nactual B1 Task recheck source\n"),
    )
    .unwrap();
    git(f.root.path(), &["add", &path]);
    git(
        f.root.path(),
        &["commit", "-qm", "actual protected Proposal"],
    );
    let revision = git(f.root.path(), &["rev-parse", "HEAD"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-app-recheck".into(),
                title: "App ready review".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-ask".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 19,
        &mut f.outbox,
    )
    .unwrap();
    let input = ProposalInput {
        source: ProposalSource::Branch {
            revision,
            base_revision: base,
        },
        reason: "actual App recheck".into(),
        affected_intents: f
            .catalog
            .objects
            .iter()
            .filter(|(_, d)| d.doc_type() == Some("Intent"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_modules: vec!["A-alpha".into()],
        affected_interfaces: f
            .catalog
            .objects
            .iter()
            .filter(|(_, d)| d.doc_type() == Some("Interface"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_tasks: vec!["T-app-recheck".into(), "T-ask".into()],
        affected_paths: vec!["alpha/**".into(), "docs/tower/**".into()],
        alternatives: vec![Alternative {
            name: "retain".into(),
            consequences: "retain original".into(),
        }],
        recommendation: "new content".into(),
    };
    let id = Uuid::now_v7().to_string();
    f.writer
        .save_proposal(
            &f.session,
            &f.catalog,
            &input,
            None,
            &id,
            &mut f.outbox,
            "host",
            || Ok(NOW + 20),
        )
        .unwrap();
    let view = baseline_review::review_view_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &id,
        NOW + 21,
    )
    .unwrap()
    .unwrap();
    let approval = baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &ReviewAction {
            request_id: Uuid::now_v7().to_string(),
            binding: view.binding,
            verdict: Verdict::Approved,
            comment: "programmatic setup only".into(),
        },
        "human:owner",
        "host",
        NOW + 21,
    )
    .unwrap();
    assert!(matches!(
        f.writer
            .publish_baseline(
                &f.session,
                &f.catalog,
                &mut f.outbox,
                &approval.receipt.result_ref.approval_eid,
                "host",
                || Ok(NOW + 25)
            )
            .unwrap(),
        PublicationReport::Recorded { .. }
    ));
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 26,
        &mut f.outbox,
    )
    .unwrap();
    git(f.root.path(), &["switch", "-qc", "t/T-app-recheck", "main"]);
    let Fixture {
        root,
        runtime,
        session,
        outbox,
        writer,
        catalog,
        ..
    } = f;
    let project_id = catalog.config.project_id().to_owned();
    drop(session);
    drop(outbox);
    drop(writer);
    fs::create_dir(runtime.path().join("recovery-outbox")).unwrap();
    fs::rename(
        runtime.path().join("outbox"),
        runtime.path().join("recovery-outbox").join(project_id),
    )
    .unwrap();
    let mut server = snapshot_server(
        root.path(),
        runtime.path().to_path_buf(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(NOW + 40)),
    );
    Arc::get_mut(&mut server).unwrap().endpoint.host = "host".into();
    let catalog = Catalog::load(root.path()).unwrap();
    server.project(&catalog).unwrap();
    (root, runtime, server)
}
fn app_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-towermd-instance",
        "snapshot-schedule-test".parse().unwrap(),
    );
    headers.insert("x-towermd-client", "app".parse().unwrap());
    headers
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_app_ready_recheck_http_is_finite_and_unblocks_cli_claim() {
    let (root, _runtime, server) = tokio::task::spawn_blocking(actual_b1_ready).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = router(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let claim = json!({"directory":root.path(),"run":null,"command":{"action":"claim","id":"T-app-recheck"}});
    let before = client
        .post(format!("{base}/command"))
        .header("x-towermd-instance", "snapshot-schedule-test")
        .json(&claim)
        .send()
        .await
        .unwrap();
    assert!(!before.status().is_success());
    let response = client
        .get(format!("{base}/app/tasks/T-app-recheck/recheck"))
        .headers(app_headers())
        .query(&[("directory", root.path().to_str().unwrap())])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let view: Value = response.json().await.unwrap();
    assert_eq!(view["binding"]["running"], Value::Null);
    assert_eq!(view["reasons"].as_array().unwrap().len(), 1);
    let body = json!({"directory":root.path(),"human_id":"owner","action":{"request_id":Uuid::now_v7().to_string(),"binding":view["binding"],"comment":"确认此任务适用于当前基线"}});
    let url = format!("{base}/app/tasks/T-app-recheck/recheck/resolve");
    let denied = client
        .post(&url)
        .header("x-towermd-instance", "snapshot-schedule-test")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let mut forged = body.clone();
    forged["actor"] = json!("human:forged");
    assert_eq!(
        client
            .post(&url)
            .headers(app_headers())
            .json(&forged)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let resolved = client
        .post(&url)
        .headers(app_headers())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resolved.status(), StatusCode::OK);
    let result: Value = resolved.json().await.unwrap();
    assert_eq!(result["receipt"]["result_ref"]["actor"], "human:owner");
    let retry: Value = client
        .post(&url)
        .headers(app_headers())
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(retry["receipt"], result["receipt"]);
    let claimed = client
        .post(format!("{base}/command"))
        .header("x-towermd-instance", "snapshot-schedule-test")
        .json(&claim)
        .send()
        .await
        .unwrap();
    assert_eq!(claimed.status(), StatusCode::OK);
    let catalog = Catalog::load(root.path()).unwrap();
    let events = server.recovery_inputs(&catalog).unwrap();
    assert_eq!(
        events
            .confirmed()
            .iter()
            .filter(|e| e.kind == "task.recheck_resolved")
            .count(),
        1
    );
    assert!(events
        .confirmed()
        .iter()
        .find(|e| e.kind == "task.recheck_resolved")
        .unwrap()
        .guards
        .is_empty());
    serving.abort();
    let _ = serving.await;
}
#[tokio::test]
async fn task_recheck_read_missing_project_does_not_register_or_create() {
    let (temp, root) = snapshot_fixture(7200);
    let catalog = Catalog::load(&root).unwrap();
    let server = snapshot_server(
        &root,
        temp.path().join("runtime"),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(NOW)),
    );
    let result = load(
        State(server.clone()),
        app_headers(),
        RoutePath("T-missing".into()),
        Query(RecheckQuery { directory: root }),
    )
    .await;
    assert_eq!(result.0, StatusCode::BAD_REQUEST);
    assert!(server.projects.lock().unwrap().is_empty());
    assert!(!catalog.common_dir.join("towermd").exists());
    assert!(!server.runtime.join("recovery-outbox").exists());
}
