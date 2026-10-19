//! Actual HTTP transport over real B0/Task/protected-source producers.
use super::super::finish_routes::tests::fixture;
use super::*;
use crate::daemon::tests::{snapshot_fixture, snapshot_server};
use towermd_core::{
    baseline_review::review_baseline_shared,
    proposals::{Alternative, ProposalInput, ProposalSource},
    recovery_contract::{baseline::Verdict, recorded::request_event_eid},
    snapshot_service::{read_draft, PublicationReport},
    tasks::TaskSpec,
};

fn request_shape(directory: &Path) -> Value {
    json!({"directory":directory,"run":null,"request_id":Uuid::now_v7().to_string(),
        "input":{"source":{"kind":"branch","revision":"a".repeat(40),"base_revision":"b".repeat(40)},
        "reason":"明确模块变化","affected_intents":[],"affected_modules":[],"affected_interfaces":[],"affected_tasks":[],"affected_paths":[],
        "alternatives":[{"name":"保留","consequences":"保留现状"}],"recommendation":"采用提案"}})
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proposals_http_rejects_foreign_instance_unknown_fields_and_bad_uuid_without_storage() {
    let (dir, root) = snapshot_fixture(600);
    let server = snapshot_server(
        &root,
        dir.path().join("runtime"),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::now() + 10_000)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/proposals", listener.local_addr().unwrap());
    let app = router(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let body = request_shape(&root);
    let before = fixture::git(&root, &["rev-parse", "main"]);
    let response = client
        .post(&base)
        .header("x-towermd-instance", "old-instance")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    for key in [
        "actor",
        "human_id",
        "proof",
        "fences",
        "approval",
        "output_ref",
        "skip",
    ] {
        let mut forged = body.clone();
        forged[key] = json!("caller-authority");
        let response = client
            .post(&base)
            .header("x-towermd-instance", &server.endpoint.instance)
            .json(&forged)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{key}");
    }
    let mut nested = body.clone();
    nested["input"]["actor"] = json!("human:injected");
    assert_eq!(
        client
            .post(&base)
            .header("x-towermd-instance", &server.endpoint.instance)
            .json(&nested)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for fault in ["uuid", "reason"] {
        let mut invalid = body.clone();
        if fault == "uuid" {
            invalid["request_id"] = json!("not-a-v7");
        } else {
            invalid["input"]["reason"] = json!("");
        }
        let response = client
            .post(&base)
            .header("x-towermd-instance", &server.endpoint.instance)
            .json(&invalid)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{fault}");
    }
    assert!(server.projects.lock().unwrap().is_empty());
    assert!(!root.join(".git/towermd/journal.jsonl").exists());
    assert!(!server.runtime.join("recovery-outbox").exists());
    assert_eq!(fixture::git(&root, &["rev-parse", "main"]), before);
    assert!(!fixture::git(&root, &["show-ref"]).contains("refs/towermd/drafts/"));
    serving.abort();
    let _ = serving.await;
}

fn actual_project() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Arc<Server>,
    ProposalInput,
    Value,
) {
    let mut f = fixture::configured_fixture(&[("docs/tower/specs/AC-propose.md", "---\ntype: Acceptance\nid: AC-propose\nmodule: A-alpha\ncriteria: [actual source-bound proposal]\n---\n")], &["test -f alpha/a.py"], false);
    let action = fixture::action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        fixture::now() + 3,
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
                || Ok(fixture::now() + 5)
            )
            .unwrap(),
        PublicationReport::Recorded { .. }
    ));
    fixture::git(f.root.path(), &["switch", "-qc", "t/T-propose", "main"]);
    let fixture::Fixture {
        root,
        runtime,
        session,
        outbox,
        ..
    } = f;
    drop(session);
    drop(outbox);
    let mut server = snapshot_server(
        root.path(),
        runtime.path().to_path_buf(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::now() + 10_000)),
    );
    // Match the actual owned project's host identity before registering it.
    Arc::get_mut(&mut server).unwrap().endpoint.host = "host".into();
    server
        .execute(Request {
            directory: root.path().to_path_buf(),
            run: None,
            command: TaskCommand::Add {
                task: TaskSpec {
                    id: "T-propose".into(),
                    title: "actual HTTP proposal".into(),
                    module: "A-alpha".into(),
                    touches: vec![],
                    allowed_paths: vec!["alpha/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC-propose".into()],
                    decisions: vec![],
                },
            },
        })
        .unwrap();
    let run = server
        .execute(Request {
            directory: root.path().to_path_buf(),
            run: None,
            command: TaskCommand::Claim {
                id: "T-propose".into(),
                harness: None,
                harness_version: None,
            },
        })
        .unwrap();
    let catalog = Catalog::load(root.path()).unwrap();
    let path = catalog.object_paths["A-alpha"].clone();
    let original = catalog.read_file(&path).unwrap().unwrap();
    fs::write(
        root.path().join(&path),
        format!("{original}\nActual proposed module explanation.\n"),
    )
    .unwrap();
    fixture::git(root.path(), &["add", &path]);
    fixture::git(root.path(), &["commit", "-qm", "actual protected source"]);
    let input = ProposalInput {
        source: ProposalSource::Branch {
            revision: fixture::git(root.path(), &["rev-parse", "HEAD"]),
            base_revision: catalog.revision.clone(),
        },
        reason: "调整模块说明，等待人工后续批准".into(),
        affected_intents: catalog
            .objects
            .iter()
            .filter(|(_, doc)| doc.doc_type() == Some("Intent"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_modules: vec!["A-alpha".into()],
        affected_interfaces: catalog
            .objects
            .iter()
            .filter(|(_, doc)| doc.doc_type() == Some("Interface"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_tasks: vec!["T-propose".into()],
        affected_paths: vec!["alpha/**".into(), "docs/tower/**".into()],
        alternatives: vec![Alternative {
            name: "保留".into(),
            consequences: "保留原说明".into(),
        }],
        recommendation: "采用明确说明".into(),
    };
    (root, runtime, server, input, run)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_proposals_http_uses_original_writer_receipt_and_rejects_rebinding() {
    let (root, _runtime, server, input, run) =
        tokio::task::spawn_blocking(actual_project).await.unwrap();
    let catalog = Catalog::load(root.path()).unwrap();
    let main = fixture::git(root.path(), &["rev-parse", "main"]);
    let tag = fixture::git(root.path(), &["rev-parse", "refs/tags/towermd/B0"]);
    let index = fs::read(catalog.common_dir.join("index")).unwrap();
    let before = server
        .recovery_inputs(&catalog)
        .unwrap()
        .confirmed()
        .to_vec();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/proposals", listener.local_addr().unwrap());
    let app = router(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(100))
        .build()
        .unwrap();
    let id = Uuid::now_v7().to_string();
    let body = json!({"directory":root.path(),"run":run["id"],"request_id":id,"input":input});
    let response = client
        .post(&base)
        .header("x-towermd-instance", &server.endpoint.instance)
        .json(&body)
        .send()
        .await
        .unwrap();
    let code = response.status();
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(code, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["current_validity"]["state"], "current");
    assert_eq!(outcome["receipt"]["request_id"], id);
    let revision = outcome["receipt"]["result_ref"]["draft"]["revision"]
        .as_str()
        .unwrap();
    let saved = read_draft(&catalog, &id).unwrap().unwrap();
    assert_eq!(saved.revision(), revision);
    assert_eq!(
        saved.frozen().content_hash,
        outcome["receipt"]["result_ref"]["draft"]["content_hash"]
    );
    let eid = request_event_eid(&id, "proposal.created").unwrap();
    let inputs = server.recovery_inputs(&catalog).unwrap();
    let actual = inputs
        .confirmed()
        .iter()
        .find(|event| event.eid == eid)
        .unwrap()
        .clone();
    assert!(!actual.stale);
    assert_eq!(actual.actor, "process:towermd-cli");
    assert_eq!(actual.host, server.endpoint.host);
    assert_eq!(serde_json::to_value(&actual.guards).unwrap(), run["leases"]);
    assert_eq!(
        actual.data["request_body"]["action"]["input"],
        body["input"]
    );
    assert!(inputs
        .confirmed()
        .iter()
        .filter(|event| !before.contains(event))
        .all(|event| event.kind == "proposal.created"));
    let repeated = client
        .post(&base)
        .header("x-towermd-instance", &server.endpoint.instance)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(repeated.status(), StatusCode::OK);
    assert_eq!(repeated.json::<Value>().await.unwrap(), outcome);
    let accepted = server
        .recovery_inputs(&catalog)
        .unwrap()
        .confirmed()
        .to_vec();
    let mut changed = body.clone();
    changed["input"]["reason"] = json!("same UUID different body");
    let refusal = client
        .post(&base)
        .header("x-towermd-instance", &server.endpoint.instance)
        .json(&changed)
        .send()
        .await
        .unwrap();
    assert!(refusal.status().is_client_error());
    let refusal: Value = refusal.json().await.unwrap();
    assert_eq!(refusal["request_id"], id);
    assert_eq!(
        server.recovery_inputs(&catalog).unwrap().confirmed(),
        accepted
    );
    let unknown_id = Uuid::now_v7().to_string();
    let mut wrong_run = body.clone();
    wrong_run["request_id"] = json!(unknown_id);
    wrong_run["run"] = json!(Uuid::now_v7().to_string());
    assert!(client
        .post(&base)
        .header("x-towermd-instance", &server.endpoint.instance)
        .json(&wrong_run)
        .send()
        .await
        .unwrap()
        .status()
        .is_client_error());
    assert!(read_draft(&catalog, &unknown_id).unwrap().is_none());
    assert_eq!(
        server.recovery_inputs(&catalog).unwrap().confirmed(),
        accepted
    );
    assert_eq!(
        read_draft(&catalog, &id).unwrap().unwrap().frozen(),
        saved.frozen()
    );
    assert_eq!(fixture::git(root.path(), &["rev-parse", "main"]), main);
    assert_eq!(
        fixture::git(root.path(), &["rev-parse", "refs/tags/towermd/B0"]),
        tag
    );
    assert_eq!(fs::read(catalog.common_dir.join("index")).unwrap(), index);
    serving.abort();
    let _ = serving.await;
}
