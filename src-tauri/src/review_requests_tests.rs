use super::*;

#[test]
fn load_keeps_complete_review_and_original_ticket_when_comparison_is_unavailable() {
    for with_revision in [false, true] {
        let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
        let app_data = tempfile::tempdir().unwrap();
        let requests = ReviewRequests::default();
        let original = json!({
            "binding":{"project_id":catalog.config.project_id(),"object_id":"draft-1","content_hash":"c".repeat(64),"target_baseline":"towermd/B0","draft_revision":"a".repeat(40)},
            "draft":{"bundle":{"id":"draft-1","project_id":catalog.config.project_id(),"docs_dir":catalog.config.docs_dir(),"source_revision":catalog.revision,"target_baseline":"towermd/B0","objects":{},"canvas_text":"{\"nodes\":[],\"edges\":[]}","deletions":[],"questions":[],"risks":[]},"files":{},"base_directory":{},"content_hash":"c".repeat(64)},
            "current_baseline":null,"diff":[{"path":"docs/tower/arch/A.md","before":"exact before\r\n","after":"exact after\r\n"}],"interface_modules":["A","B"],"publish_blockers":["original blocker"],"verification":{"state":"unavailable","reason":"original reason"}
        });
        let mut wire = original.clone();
        if with_revision {
            wire["main_revision"] = json!(catalog.revision);
        }
        let (endpoint, trace, worker) = crate::daemon_client::tests::server_with_health(vec![wire]);
        let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
        let loaded = load_with(
            &registry,
            &requests,
            &token,
            "draft-1",
            app_data.path(),
            || Ok(client),
        )
        .unwrap();
        worker.join().unwrap();
        for field in [
            "draft",
            "binding",
            "diff",
            "interface_modules",
            "publish_blockers",
            "verification",
        ] {
            assert_eq!(loaded[field], original[field]);
        }
        assert_eq!(
            loaded["canvas_comparison"]["state"],
            if with_revision {
                "available"
            } else {
                "unavailable"
            }
        );
        let ticket = loaded["review_ticket"].as_str().unwrap();
        let state = requests.0.lock().unwrap();
        assert_eq!(json!(state.views[ticket].binding), original["binding"]);
        assert_eq!(state.views[ticket].actor, loaded["human_actor"]);
        assert!(trace
            .lock()
            .unwrap()
            .iter()
            .all(|request| !request.starts_with("POST ")));
    }
}

fn stored() -> QueuedReview {
    let id = uuid::Uuid::now_v7().to_string();
    let request = BaselineRequest::prepare(
        "project",
        "fixture-host",
        "human:fixture",
        &id,
        BaselineReviewAction::ReviewBaseline {
            binding: ReviewBinding {
                project_id: "project".into(),
                object_id: "draft-1".into(),
                content_hash: "c".repeat(64),
                target_baseline: "towermd/B0".into(),
                draft_revision: "a".repeat(40),
            },
            verdict: Verdict::Approved,
            comment: "原文已审阅".into(),
        },
    )
    .unwrap();
    QueuedReview {
        schema: 1,
        directory: "/fixture root".into(),
        common_dir: "/fixture root/.git".into(),
        request: request.body().clone(),
        metadata: request.metadata().clone(),
    }
}
fn response(stored: &QueuedReview) -> Value {
    let (binding, verdict, comment) = action(stored);
    json!({"receipt":{"request_id":stored.metadata.request_id,"body_hash":stored.metadata.body_hash,"result_ref":{"approval_eid":request_event_eid(&stored.metadata.request_id,match verdict{Verdict::Approved=>"baseline.approved",Verdict::Rejected=>"baseline.rejected"}).unwrap(),"binding":binding,"actor":stored.request.actor,"verdict":verdict,"comment":comment}},"current_validity":{"state":"current"},"publication":{"state":"pending","reason":"main checked out"}})
}
#[test]
fn fsynced_review_reopens_original_actor_binding_verdict_and_hash() {
    let dir = tempfile::tempdir().unwrap();
    let original = stored();
    let id = &original.metadata.request_id;
    let file = path(dir.path(), id).unwrap();
    write_new(&file, &original).unwrap();
    let reopened = read(dir.path(), id).unwrap();
    assert_eq!(reopened.request, original.request);
    assert_eq!(reopened.metadata, original.metadata);
    let mut altered = serde_json::to_value(&original).unwrap();
    altered["request"]["action"]["binding"]["draft_revision"] = json!("b".repeat(40));
    fs::write(file, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(read(dir.path(), id).is_err());
}
#[test]
fn exact_receipt_is_required_and_publication_pending_does_not_invalidate_approval() {
    let original = stored();
    let valid = response(&original);
    assert!(confirmed(&original, &valid));
    for field in ["actor", "comment", "verdict"] {
        let mut invalid = valid.clone();
        invalid["receipt"]["result_ref"][field] = json!("replacement");
        assert!(!confirmed(&original, &invalid));
    }
    for field in [
        "project_id",
        "object_id",
        "content_hash",
        "target_baseline",
        "draft_revision",
    ] {
        let mut invalid = valid.clone();
        invalid["receipt"]["result_ref"]["binding"][field] = json!("replacement");
        assert!(!confirmed(&original, &invalid));
    }
    let mut wrong_event = valid.clone();
    wrong_event["receipt"]["result_ref"]["approval_eid"] =
        json!(request_event_eid(&original.metadata.request_id, "baseline.rejected").unwrap());
    assert!(!confirmed(&original, &wrong_event));
    let mut invalid = valid.clone();
    invalid["receipt"]["body_hash"] = json!("different");
    assert!(!confirmed(&original, &invalid));
    let mut historical = valid;
    historical["current_validity"] = json!({"state":"historical_only","reason":"old binding"});
    assert!(confirmed(&original, &historical));
}
#[test]
fn review_fixed_routes_preserve_typed_binding_and_retry_has_only_directory() {
    let original = stored();
    let (binding, verdict, comment) = action(&original);
    let (endpoint, trace, worker) = crate::daemon_client::tests::server_with_health(vec![
        json!({"binding":binding}),
        response(&original),
        json!({"kind":"review","request_body":original.request}),
        response(&original),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    client
        .review(&original.directory, &binding.object_id)
        .unwrap();
    client
        .review_action(
            &binding.object_id,
            &ReviewHttpRequest {
                directory: &original.directory,
                human_id: "fixture",
                action: ReviewAction {
                    request_id: original.metadata.request_id.clone(),
                    binding: binding.clone(),
                    verdict: verdict.clone(),
                    comment: comment.into(),
                },
            },
        )
        .unwrap();
    client
        .original_request(&original.directory, &original.metadata.request_id)
        .unwrap();
    client
        .retry_original(&original.directory, &original.metadata.request_id)
        .unwrap();
    worker.join().unwrap();
    let trace = trace.lock().unwrap();
    assert!(trace[1].starts_with("GET /app/baselines/draft-1/review?directory="));
    assert!(trace[1].contains("x-towermd-client: app"));
    assert!(trace[2].starts_with("POST /app/baselines/draft-1/action "));
    let body: Value = serde_json::from_str(trace[2].split("\r\n").last().unwrap()).unwrap();
    assert_eq!(body["action"]["binding"], json!(binding));
    assert!(body.get("actor").is_none());
    assert!(body.get("host").is_none());
    let retry: Value = serde_json::from_str(trace[4].split("\r\n").last().unwrap()).unwrap();
    assert_eq!(retry, json!({"directory":original.directory}));
}

#[test]
fn restart_restores_review_uuid_and_binding_but_revoked_tokens_cannot_submit() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let app_data = tempfile::tempdir().unwrap();
    let mut original = stored();
    original.directory = catalog.root.clone();
    original.common_dir = catalog.common_dir.clone();
    original.request.project_id = catalog.config.project_id().into();
    let BaselineReviewAction::ReviewBaseline { binding, .. } = &mut original.request.action;
    binding.project_id = original.request.project_id.clone();
    original.metadata = BaselineRequest::prepare(
        &original.request.project_id,
        &original.request.host,
        &original.request.actor,
        &original.metadata.request_id,
        original.request.action.clone(),
    )
    .unwrap()
    .metadata()
    .clone();
    write_new(
        &path(app_data.path(), &original.metadata.request_id).unwrap(),
        &original,
    )
    .unwrap();
    let restarted = ReviewRequests::default();
    let found = pending(&registry, &restarted, &token, "draft-1", app_data.path()).unwrap();
    assert_eq!(found[0].request_id, original.metadata.request_id);
    assert_eq!(found[0].binding, action(&original).0.clone());
    let next = registry
        .select(Some(catalog.root.clone()))
        .unwrap()
        .unwrap()
        .token;
    assert_eq!(
        submit(
            &registry,
            &restarted,
            &token,
            &original.metadata.request_id,
            app_data.path()
        )
        .unwrap_err()
        .code,
        "project_missing"
    );
    assert_eq!(
        submit(
            &registry,
            &restarted,
            &next,
            &original.metadata.request_id,
            app_data.path()
        )
        .unwrap_err()
        .code,
        "request_not_prepared"
    );
    pending(&registry, &restarted, &next, "draft-1", app_data.path()).unwrap();
    assert_eq!(
        read(app_data.path(), &original.metadata.request_id)
            .unwrap()
            .request,
        original.request
    );
}

#[test]
fn original_read_failures_keep_review_queue_and_explicit_retry_uses_exact_original() {
    for transport_failure in [false, true] {
        let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
        let app_data = tempfile::tempdir().unwrap();
        let mut original = stored();
        original.directory = catalog.root.clone();
        original.common_dir = catalog.common_dir.clone();
        original.request.project_id = catalog.config.project_id().into();
        let BaselineReviewAction::ReviewBaseline { binding, .. } = &mut original.request.action;
        binding.project_id = original.request.project_id.clone();
        original.metadata = BaselineRequest::prepare(
            &original.request.project_id,
            &original.request.host,
            &original.request.actor,
            &original.metadata.request_id,
            original.request.action.clone(),
        )
        .unwrap()
        .metadata()
        .clone();
        let id = &original.metadata.request_id;
        let file = path(app_data.path(), id).unwrap();
        write_new(&file, &original).unwrap();
        let bytes = fs::read(&file).unwrap();
        let requests = ReviewRequests::default();
        pending(&registry, &requests, &token, "draft-1", app_data.path()).unwrap();
        let mut responses = vec![(200, Value::Null)];
        if !transport_failure {
            responses.push((503, json!({"code":"request_uncertain","request_id":id,"error":"recovery sources changed during proof"})));
        }
        let (endpoint, trace, worker) = crate::daemon_client::tests::server(responses);
        let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
        let outcome = submit_with(&registry, &requests, &token, id, app_data.path(), || {
            Ok(client)
        })
        .unwrap();
        worker.join().unwrap();
        assert_eq!(outcome.state, "uncertain");
        assert_eq!(outcome.request_id, *id);
        let error = outcome.error.unwrap();
        assert_eq!(error.code, "request_uncertain");
        assert!(error.message.contains(id));
        let detail: Value = serde_json::from_str(error.detail.as_ref().unwrap()).unwrap();
        assert_eq!(detail["code"], "request_uncertain");
        assert_eq!(detail["request_id"], *id);
        assert!(!file.with_extension("resolved").exists());
        assert_eq!(fs::read(&file).unwrap(), bytes);
        assert_eq!(
            pending(&registry, &requests, &token, "draft-1", app_data.path()).unwrap()[0]
                .request_id,
            *id
        );
        assert!(trace
            .lock()
            .unwrap()
            .iter()
            .all(|request| !request.starts_with("POST ")));

        let (endpoint, trace, worker) = crate::daemon_client::tests::server_with_health(vec![
            json!({"kind":"review","p3_request":original.metadata,"request_body":original.request}),
            response(&original),
        ]);
        let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
        let confirmed = submit_with(&registry, &requests, &token, id, app_data.path(), || {
            Ok(client)
        })
        .unwrap();
        worker.join().unwrap();
        assert_eq!(confirmed.state, "confirmed");
        assert_eq!(confirmed.request_id, *id);
        assert_eq!(fs::read(&file).unwrap(), bytes);
        let trace = trace.lock().unwrap();
        assert!(trace[1].starts_with(&format!("GET /app/requests/{id}?")));
        assert!(trace[2].starts_with(&format!("POST /app/requests/{id}/retry ")));
        assert_eq!(
            serde_json::from_str::<Value>(trace[2].split("\r\n").last().unwrap()).unwrap(),
            json!({"directory":original.directory})
        );
    }
}
