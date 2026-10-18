use super::*;
use crate::daemon::tests::{snapshot_fixture, snapshot_git, snapshot_server};
use towermd_core::{
    baseline_draft::{freeze, from_scan, DraftObject},
    baseline_schema::BaselineSubmit,
    coldstart_evidence::{read_completed, PreparedColdstart},
    recovery_contract::recorded::RequestBody,
    request_ledger::{DraftSaveAction, PreparedDraftSaveRequest},
    verifier::{verify_sealed, VerificationRequest},
};

const NOW: u64 = 1_800_000_000_000;

#[tokio::test]
async fn request_reads_and_unknown_retries_do_not_create_a_project_hub_or_action() {
    let (dir, root) = snapshot_fixture(600);
    let server = snapshot_server(
        &root,
        dir.path().join("runtime"),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(NOW)),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-towermd-instance",
        server.endpoint.instance.parse().unwrap(),
    );
    headers.insert("x-towermd-client", "app".parse().unwrap());
    let id = Uuid::now_v7().to_string();
    let draft = saved(
        State(server.clone()),
        headers.clone(),
        RoutePath(id.clone()),
        Query(ReviewQuery {
            directory: root.clone(),
        }),
    )
    .await;
    assert_eq!(draft.0, StatusCode::NOT_FOUND);
    let status = request_status(
        State(server.clone()),
        headers.clone(),
        RoutePath(id.clone()),
        Query(ReviewQuery {
            directory: root.clone(),
        }),
    )
    .await;
    assert_eq!(status.0, StatusCode::NOT_FOUND);
    let retry = retry_request(
        State(server.clone()),
        headers,
        RoutePath(id),
        Json(RetryRequest {
            directory: root.clone(),
        }),
    )
    .await;
    assert!(retry.0.is_client_error());
    assert!(server.projects.lock().unwrap().is_empty());
    assert!(!root.join(".git/towermd/journal.jsonl").exists());
    assert!(!server.runtime.join("recovery-outbox").exists());
    assert!(!snapshot_git(&root, &["show-ref"]).contains("refs/towermd/drafts/"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_saved_draft_routes_keep_app_boundary_and_exact_original_review() {
    let (dir, root) = snapshot_fixture(600);
    fs::create_dir(root.join("alpha")).unwrap();
    fs::write(root.join("alpha/a.py"), "# actual module\n").unwrap();
    snapshot_git(&root, &["add", "."]);
    snapshot_git(&root, &["commit", "-qm", "module"]);
    let main_before = snapshot_git(&root, &["rev-parse", "main"]);
    let index_before = fs::read(root.join(".git/index")).unwrap();
    let server = snapshot_server(
        &root,
        dir.path().join("runtime"),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(NOW)),
    );
    let setup_server = server.clone();
    let setup_root = root.clone();
    let (id, revision) = tokio::task::spawn_blocking(move || {
        let catalog = Catalog::load(&setup_root).unwrap();
        let project = setup_server.project(&catalog).unwrap();
        let prepared = PreparedColdstart::prepare(
            capture_scan_input(&catalog, &ScanOptions::default()).unwrap(),
            "scan-app-review",
        ).unwrap();
        let input = prepared.input();
        let sealed = verify_sealed(&VerificationRequest {
            root: input.root.clone(), common_dir: input.common_dir.clone(),
            revision: input.revision.clone(), commands: prepared.commands().unwrap(),
            timeout: Duration::from_secs(5),
            log_root: input.common_dir.join("towermd/cache/verification"),
            cancelled: Arc::new(AtomicBool::new(false)),
        }).unwrap();
        let coldstart = prepared.freeze(&sealed, "snapshot-host", NOW).unwrap();
        let mut outbox = RecoveryOutbox::open(
            &setup_server.runtime.join("recovery-outbox").join(catalog.config.project_id()),
            RecoveryContext::from_catalog(&catalog).unwrap(),
        ).unwrap();
        ProjectSession::record_coldstart_shared(&project.hub, &catalog, &mut outbox, &coldstart, NOW).unwrap();
        let completed = read_completed(&catalog, &outbox.read_inputs().unwrap(), "scan-app-review").unwrap().unwrap();
        let modules = completed.scan()["modules"].as_array().unwrap().iter().map(|module| {
            let id = module["id"].as_str().unwrap();
            DraftObject { id: id.into(), path: format!("arch/{id}.md"),
                text: format!("---\ntype: Module\nid: {id}\npaths: [alpha/**]\nverify: []\ninterfaces: []\n---\nactual text") }
        }).collect();
        let submit = BaselineSubmit {
            scan_id: completed.scan_id().into(), project_id: catalog.config.project_id().into(),
            source_revision: completed.input().revision.clone(), intents: vec![], modules,
            interfaces: vec![], acceptances: vec![], questions: vec!["review me".into()], risks: vec![],
        };
        let bundle = from_scan(&catalog, &completed, submit.clone()).unwrap();
        let frozen = freeze(&catalog, &bundle).unwrap();
        let request = PreparedDraftSaveRequest::prepare(RequestBody {
            project_id: catalog.config.project_id().into(), host: "snapshot-host".into(),
            actor: "process:towermd-cli".into(), run: None,
            action: DraftSaveAction::SubmitBaseline { submit },
        }, &Uuid::now_v7().to_string()).unwrap();
        let saved = project.writer.save_draft(&project.hub, &catalog, &frozen, &request, || Ok(NOW)).unwrap();
        (bundle.id, saved.revision().to_owned())
    }).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = router(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let instance = &server.endpoint.instance;
    let schema_url = format!("{base}/baseline/schema");
    assert_eq!(
        client.get(&schema_url).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    let schema: Value = client
        .get(&schema_url)
        .header("x-towermd-instance", instance)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        schema,
        serde_json::from_str::<Value>(towermd_core::baseline_schema::schema()).unwrap()
    );
    assert_eq!(
        client
            .get(format!("{base}/app/drafts/{id}"))
            .header("x-towermd-instance", instance)
            .query(&[("directory", &root)])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let saved: Value = client
        .get(format!("{base}/app/drafts/{id}"))
        .header("x-towermd-instance", instance)
        .header("x-towermd-client", "app")
        .query(&[("directory", &root)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["state"], "saved");
    assert_eq!(saved["revision"], revision);
    let review_url = format!("{base}/app/baselines/{id}/review");
    assert_eq!(
        client
            .get(&review_url)
            .header("x-towermd-instance", instance)
            .query(&[("directory", &root)])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let view: Value = client
        .get(&review_url)
        .header("x-towermd-instance", instance)
        .header("x-towermd-client", "app")
        .query(&[("directory", &root)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(view["binding"]["draft_revision"], revision);
    assert!(view["diff"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["path"].as_str().unwrap().ends_with("tower.canvas")));
    let request_id = Uuid::now_v7().to_string();
    let body = json!({"directory":root,"human_id":"owner","action":{
        "request_id":request_id,"binding":view["binding"],"verdict":"approved","comment":"exact content"
    }});
    let action_url = format!("{base}/app/baselines/{id}/action");
    assert_eq!(
        client
            .post(&action_url)
            .header("x-towermd-instance", instance)
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut injection = body.clone();
    injection["actor"] = json!("human:injected");
    assert_eq!(
        client
            .post(&action_url)
            .header("x-towermd-instance", instance)
            .header("x-towermd-client", "app")
            .json(&injection)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut first = None;
    for _ in 0..2 {
        let response = client
            .post(&action_url)
            .header("x-towermd-instance", instance)
            .header("x-towermd-client", "app")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().await.unwrap()
        );
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["receipt"]["result_ref"]["actor"], "human:owner");
        assert_eq!(value["current_validity"]["state"], "current");
        if let Some(first) = &first {
            assert_eq!(&value, first);
        } else {
            first = Some(value);
        }
    }
    // The original decision may already be accepted when another writer holds
    // the outbox. A retry must retain its identity and remain uncertain, rather
    // than telling the App to resolve its queue as a rejected decision.
    let catalog = Catalog::load(&root).unwrap();
    let held = RecoveryOutbox::open(
        &server
            .runtime
            .join("recovery-outbox")
            .join(catalog.config.project_id()),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let retry = client
        .post(format!("{base}/app/requests/{request_id}/retry"))
        .header("x-towermd-instance", instance)
        .header("x-towermd-client", "app")
        .json(&json!({"directory":root}))
        .send()
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::SERVICE_UNAVAILABLE);
    let retry: Value = retry.json().await.unwrap();
    assert_eq!(retry["code"], "request_uncertain");
    assert_eq!(retry["request_id"], request_id);
    drop(held);
    // Reading an accepted original can fail before opening the producer's
    // outbox. That failure must not resolve the App's durable request as refused.
    let outbox_path = server
        .runtime
        .join("recovery-outbox")
        .join(catalog.config.project_id());
    let retained = outbox_path.with_extension("retained");
    fs::rename(&outbox_path, &retained).unwrap();
    fs::write(&outbox_path, "temporarily unreadable outbox directory").unwrap();
    let unreadable = client
        .post(format!("{base}/app/requests/{request_id}/retry"))
        .header("x-towermd-instance", instance)
        .header("x-towermd-client", "app")
        .json(&json!({"directory":root}))
        .send()
        .await
        .unwrap();
    assert_eq!(unreadable.status(), StatusCode::SERVICE_UNAVAILABLE);
    let unreadable: Value = unreadable.json().await.unwrap();
    assert_eq!(unreadable["code"], "request_uncertain");
    assert_eq!(unreadable["request_id"], request_id);
    fs::remove_file(&outbox_path).unwrap();
    fs::rename(&retained, &outbox_path).unwrap();
    let mut changed = body;
    changed["action"]["comment"] = json!("different body");
    assert!(client
        .post(&action_url)
        .header("x-towermd-instance", instance)
        .header("x-towermd-client", "app")
        .json(&changed)
        .send()
        .await
        .unwrap()
        .status()
        .is_client_error());
    assert_eq!(snapshot_git(&root, &["rev-parse", "main"]), main_before);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index_before);
    assert!(!root.join("docs/tower/tower.canvas").exists());
    let catalog = Catalog::load(&root).unwrap();
    let inputs = server.recovery_inputs(&catalog).unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "baseline.approved")
            .count(),
        1
    );
    assert!(!inputs
        .confirmed()
        .iter()
        .any(|event| event.kind == "baseline.created"));
    assert_eq!(first.as_ref().unwrap()["publication"]["state"], "pending");
    assert_eq!(first.as_ref().unwrap()["publication"]["stage"], "main");
    let original_receipt = first.unwrap()["receipt"].clone();
    drop(client);
    serving.abort();
    let _ = serving.await;

    // Reopening the project must discover the already accepted approval; a
    // second click or a newly invented human action is not needed for recovery.
    snapshot_git(&root, &["switch", "-qc", "user-working"]);
    let switched_index = fs::read(root.join(".git/index")).unwrap();
    let worktree_head = snapshot_git(&root, &["rev-parse", "HEAD"]);
    server.projects.lock().unwrap().clear();
    let journal = root.join(".git/towermd/journal.jsonl");
    let journal_before = fs::read(&journal).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-towermd-instance",
        server.endpoint.instance.parse().unwrap(),
    );
    headers.insert("x-towermd-client", "app".parse().unwrap());
    let read = super::saved(
        State(server.clone()),
        headers,
        RoutePath(id),
        Query(ReviewQuery {
            directory: root.clone(),
        }),
    )
    .await;
    assert_eq!(read.0, StatusCode::OK);
    assert!(server.projects.lock().unwrap().is_empty());
    assert_eq!(fs::read(&journal).unwrap(), journal_before);
    let catalog = Catalog::load(&root).unwrap();
    let project = server.project(&catalog).unwrap();
    server.tick().unwrap();
    server.drain_snapshots().await.unwrap();
    let baseline = snapshot_git(&root, &["rev-parse", "refs/tags/towermd/B0"]);
    assert_ne!(baseline, main_before);
    assert_eq!(snapshot_git(&root, &["rev-parse", "main"]), baseline);
    assert_eq!(snapshot_git(&root, &["rev-parse", "HEAD"]), worktree_head);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), switched_index);
    let catalog = Catalog::load(&root).unwrap();
    let inputs = server.recovery_inputs(&catalog).unwrap();
    let original =
        towermd_core::request_ledger::read_baseline_request(&catalog, &inputs, &request_id)
            .unwrap()
            .unwrap();
    assert_eq!(
        serde_json::to_value(original).unwrap()["receipt"],
        original_receipt
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "baseline.created")
            .count(),
        1
    );
    assert_eq!(
        project
            .hub
            .lock()
            .unwrap()
            .events(0)
            .iter()
            .filter(|event| event.kind == "baseline.created")
            .count(),
        1
    );
    assert!(!project.publication_reconcile.load(Ordering::Acquire));
}
