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
    actual_b1_ready_with_registration(true)
}
fn actual_b1_ready_with_registration(
    register: bool,
) -> (tempfile::TempDir, tempfile::TempDir, Arc<Server>) {
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
    if register {
        server.project(&catalog).unwrap();
    }
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

// Reads must work after a daemon restart, before any CLI command registers its
// producer. This uses the actual B1 publication and the original HTTP mutation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_existing_recheck_read_is_pure_and_human_submit_registers_without_creating_missing_hub(
) {
    let (root, runtime, server) =
        tokio::task::spawn_blocking(|| actual_b1_ready_with_registration(false))
            .await
            .unwrap();
    let catalog = Catalog::load(root.path()).unwrap();
    assert!(server.projects.lock().unwrap().is_empty());
    let storage = catalog.common_dir.join("towermd");
    let journal = storage.join("journal.jsonl");
    let bytes = fs::read(&journal).unwrap();
    let refs = git(
        root.path(),
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    );
    let inputs = server
        .recovery_inputs(&catalog)
        .unwrap()
        .digest()
        .to_owned();
    fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, path: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    walk(root, &path, found);
                } else {
                    found.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(root, root, &mut found);
        found
    }
    let runtime_before = files(runtime.path());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = router(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let get_url = format!("{base}/app/tasks/T-app-recheck/recheck");
    let post_url = format!("{get_url}/resolve");
    let response = client
        .get(&get_url)
        .headers(app_headers())
        .query(&[("directory", root.path().to_str().unwrap())])
        .send()
        .await
        .unwrap();
    let code = response.status();
    let view: Value = response.json().await.unwrap();
    assert_eq!(code, StatusCode::OK, "{view}");
    assert_eq!(view["binding"]["running"], Value::Null);
    assert_eq!(view["reasons"].as_array().unwrap().len(), 1);
    assert!(
        server.projects.lock().unwrap().is_empty(),
        "GET never registers a producer"
    );
    assert_eq!(fs::read(&journal).unwrap(), bytes);
    assert_eq!(
        git(
            root.path(),
            &["for-each-ref", "--format=%(refname) %(objectname)"]
        ),
        refs
    );
    assert_eq!(server.recovery_inputs(&catalog).unwrap().digest(), inputs);
    assert_eq!(
        files(runtime.path()),
        runtime_before,
        "GET never creates owner/outbox/runtime files"
    );
    let body = json!({"directory":root.path(),"human_id":"owner","action":{
        "request_id":Uuid::now_v7().to_string(),"binding":view["binding"],"comment":"原请求确认当前基线"}});
    let denied = client
        .post(&post_url)
        .header("x-towermd-instance", "snapshot-schedule-test")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(server.projects.lock().unwrap().is_empty());
    let mut malformed = body.clone();
    malformed["action"]["request_id"] = json!("not-a-request-uuid");
    let denied = client
        .post(&post_url)
        .headers(app_headers())
        .json(&malformed)
        .send()
        .await
        .unwrap();
    assert!(!denied.status().is_success());
    assert!(
        server.projects.lock().unwrap().is_empty(),
        "syntax-invalid requests cannot activate a producer"
    );
    assert_eq!(fs::read(&journal).unwrap(), bytes);
    assert_eq!(files(runtime.path()), runtime_before);
    // A valid cold subject is not permission to recreate a now-missing Hub.
    fs::remove_file(&journal).unwrap();
    for read in [true, false] {
        let response = if read {
            client
                .get(&get_url)
                .headers(app_headers())
                .query(&[("directory", root.path().to_str().unwrap())])
                .send()
                .await
                .unwrap()
        } else {
            client
                .post(&post_url)
                .headers(app_headers())
                .json(&body)
                .send()
                .await
                .unwrap()
        };
        assert!(!response.status().is_success());
        assert!(!journal.exists(), "missing journal must never be recreated");
        assert!(server.projects.lock().unwrap().is_empty());
    }
    fs::write(&journal, b"complete corrupt journal\n").unwrap();
    for read in [true, false] {
        let response = if read {
            client
                .get(&get_url)
                .headers(app_headers())
                .query(&[("directory", root.path().to_str().unwrap())])
                .send()
                .await
                .unwrap()
        } else {
            client
                .post(&post_url)
                .headers(app_headers())
                .json(&body)
                .send()
                .await
                .unwrap()
        };
        assert!(!response.status().is_success());
        assert_eq!(fs::read(&journal).unwrap(), b"complete corrupt journal\n");
        assert!(server.projects.lock().unwrap().is_empty());
    }
    fs::write(&journal, &bytes).unwrap();
    assert_eq!(
        files(runtime.path()),
        runtime_before,
        "failed mutations do not bind a missing/corrupt project"
    );
    let resolved = client
        .post(&post_url)
        .headers(app_headers())
        .json(&body)
        .send()
        .await
        .unwrap();
    let code = resolved.status();
    let result: Value = resolved.json().await.unwrap();
    assert_eq!(code, StatusCode::OK, "{result}");
    assert_eq!(
        result["receipt"]["request_id"],
        body["action"]["request_id"]
    );
    assert_eq!(result["receipt"]["result_ref"]["actor"], "human:owner");
    assert!(server
        .registered_project(catalog.config.project_id())
        .unwrap()
        .is_some());
    let retried = client
        .post(&post_url)
        .headers(app_headers())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(retried.status(), StatusCode::OK);
    let retried: Value = retried.json().await.unwrap();
    assert_eq!(retried["receipt"], result["receipt"]);
    let claim = json!({"directory":root.path(),"run":null,"command":{"action":"claim","id":"T-app-recheck"}});
    let claimed = client
        .post(format!("{base}/command"))
        .header("x-towermd-instance", "snapshot-schedule-test")
        .json(&claim)
        .send()
        .await
        .unwrap();
    assert_eq!(claimed.status(), StatusCode::OK);
    let events = server
        .recovery_inputs(&Catalog::load(root.path()).unwrap())
        .unwrap();
    let resolutions = events
        .confirmed()
        .iter()
        .filter(|event| event.kind == "task.recheck_resolved")
        .collect::<Vec<_>>();
    assert_eq!(resolutions.len(), 1);
    assert!(resolutions[0].guards.is_empty());
    // The same project id in another real clone cannot steal this producer.
    let clone_parent = tempfile::tempdir().unwrap();
    let clone_root = clone_parent.path().join("clone");
    git(
        root.path(),
        &[
            "clone",
            "--quiet",
            "--no-hardlinks",
            "--branch",
            "main",
            root.path().to_str().unwrap(),
            clone_root.to_str().unwrap(),
        ],
    );
    let clone_catalog = Catalog::load(&clone_root).unwrap();
    let clone_store = clone_catalog.common_dir.join("towermd");
    fs::create_dir(&clone_store).unwrap();
    fs::copy(&journal, clone_store.join("journal.jsonl")).unwrap();
    fs::copy(storage.join("hub.lock"), clone_store.join("hub.lock")).unwrap();
    let clone_bytes = files(&clone_store);
    let denied = client
        .get(&get_url)
        .headers(app_headers())
        .query(&[("directory", clone_root.to_str().unwrap())])
        .send()
        .await
        .unwrap();
    assert!(!denied.status().is_success());
    let mut clone_body = body.clone();
    clone_body["directory"] = json!(clone_root);
    let denied = client
        .post(&post_url)
        .headers(app_headers())
        .json(&clone_body)
        .send()
        .await
        .unwrap();
    assert!(!denied.status().is_success());
    assert_eq!(files(&clone_store), clone_bytes);
    assert_eq!(server.projects.lock().unwrap().len(), 1);
    serving.abort();
    let _ = serving.await;
}
