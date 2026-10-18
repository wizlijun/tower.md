use super::*;
fn queued() -> QueuedEdit {
    let id = uuid::Uuid::now_v7().to_string();
    let prepared = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: "fixture-project".into(),
            host: "fixture-host".into(),
            actor: "human:fixture".into(),
            run: None,
            action: DraftSaveAction::EditDraft {
                id: "draft-1".into(),
                expected_revision: "a".repeat(40),
                edit: DraftEdit::Move {
                    node: "A".into(),
                    x: -12,
                    y: 34,
                },
            },
        },
        &id,
    )
    .unwrap();
    QueuedEdit {
        schema: 1,
        directory: "/fixture".into(),
        common_dir: "/fixture/.git".into(),
        request: prepared.body().clone(),
        metadata: prepared.metadata().clone(),
    }
}
#[test]
fn native_identity_and_ids_reject_actor_paths_and_missing_user() {
    for invalid in ["", "human:owner", "a/b", "with space", "a\nb"] {
        assert!(human(invalid).is_err());
    }
    assert_eq!(human("本机-owner").unwrap(), "本机-owner");
    for invalid in ["", ".", "..", "../../main", "a?directory=elsewhere", "a/b"] {
        assert!(safe_id(invalid).is_err());
    }
    assert!(safe_id("draft-1.some_id").is_ok());
}
#[test]
fn fsynced_original_queue_reopens_with_identical_uuid_body_and_shared_hash() {
    let temp = tempfile::tempdir().unwrap();
    let stored = queued();
    let id = &stored.metadata.request_id;
    let path = request_path(temp.path(), id).unwrap();
    write_new(&path, &stored).unwrap();
    let reopened = read_queue(temp.path(), id).unwrap();
    assert_eq!(reopened.request, stored.request);
    assert_eq!(reopened.metadata, stored.metadata);
    assert!(write_new(&path, &stored).is_err());
    let mut altered: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    altered["request"]["action"]["edit"]["x"] = json!(999);
    fs::write(&path, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(read_queue(temp.path(), id).is_err());
}
#[test]
fn typed_http_body_has_no_actor_host_or_replacement_payload() {
    let stored = queued();
    let (id, revision, edit) = edit_fields(&stored).unwrap();
    let body = serde_json::to_value(EditHttpRequest {
        action: "edit",
        directory: &stored.directory,
        human_id: "fixture",
        request_id: &stored.metadata.request_id,
        id,
        expected_revision: revision,
        edit,
    })
    .unwrap();
    assert_eq!(
        body["edit"],
        json!({"action":"move","node":"A","x":-12,"y":34})
    );
    assert!(body.get("actor").is_none());
    assert!(body.get("host").is_none());
    assert!(body.get("url").is_none());
    assert_eq!(body["expected_revision"], "a".repeat(40));
}
#[test]
fn queue_rejects_symlinks_and_non_v7_request_ids() {
    let temp = tempfile::tempdir().unwrap();
    assert!(request_path(temp.path(), "0192f0aa-0000-4000-8000-000000000001").is_err());
    #[cfg(unix)]
    {
        let stored = queued();
        let original = temp.path().join("original");
        fs::write(&original, "{}").unwrap();
        let path = request_path(temp.path(), &stored.metadata.request_id).unwrap();
        std::os::unix::fs::symlink(original, path).unwrap();
        assert!(read_queue(temp.path(), &stored.metadata.request_id).is_err());
    }
}

#[test]
fn fixed_native_edit_and_original_retry_routes_never_accept_a_replacement_body() {
    let stored = queued();
    let (id, revision, edit) = edit_fields(&stored).unwrap();
    let request_id = &stored.metadata.request_id;
    let root = Path::new("/fixture root/中文");
    let (endpoint, trace, worker) = crate::daemon_client::tests::server_with_health(vec![
        json!({"state":"saved"}),
        json!({"receipt":"actual"}),
        json!({"kind":"draft_save","request_body":stored.request}),
        json!({"receipt":"same"}),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    client.saved(root, id).unwrap();
    client
        .edit(&EditHttpRequest {
            action: "edit",
            directory: root,
            human_id: "fixture",
            request_id,
            id,
            expected_revision: revision,
            edit,
        })
        .unwrap();
    assert!(client.original_request(root, request_id).unwrap().is_some());
    client.retry_original(root, request_id).unwrap();
    worker.join().unwrap();
    let trace = trace.lock().unwrap();
    assert!(trace[1].starts_with("GET /app/drafts/draft-1?directory="));
    assert!(trace[1].contains("%E4%B8%AD%E6%96%87"));
    assert!(trace[1].contains("x-towermd-client: app"));
    assert!(trace[2].starts_with("POST /drafts "));
    assert!(trace[2].contains("x-towermd-client: app"));
    let body: Value = serde_json::from_str(trace[2].split("\r\n").last().unwrap()).unwrap();
    assert_eq!(body["request_id"], *request_id);
    assert_eq!(body["edit"], serde_json::to_value(edit).unwrap());
    assert!(body.get("actor").is_none());
    assert!(trace[3].starts_with(&format!("GET /app/requests/{request_id}?directory=")));
    assert!(trace[4].starts_with(&format!("POST /app/requests/{request_id}/retry ")));
    assert!(trace[4].contains("x-towermd-client: app"));
    let retry: Value = serde_json::from_str(trace[4].split("\r\n").last().unwrap()).unwrap();
    assert_eq!(retry, json!({"directory":root}));
}
#[test]
fn mutation_busy_conflict_and_missing_original_keep_their_real_transport_state() {
    let stored = queued();
    let (id, revision, edit) = edit_fields(&stored).unwrap();
    let (endpoint, _, worker) = crate::daemon_client::tests::server(vec![
        (200, Value::Null),
        (404, json!({"error":"unknown request"})),
        (
            503,
            json!({"code":"writer_busy","request_id":stored.metadata.request_id,"error":"repository writer is busy"}),
        ),
        (
            409,
            json!({"code":"request_conflict","error":"expected revision changed"}),
        ),
        (
            400,
            json!({"code":"request_refused","error":"invalid edit"}),
        ),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    assert!(client
        .original_request(Path::new("/fixture"), &stored.metadata.request_id)
        .unwrap()
        .is_none());
    let body = EditHttpRequest {
        action: "edit",
        directory: Path::new("/fixture"),
        human_id: "fixture",
        request_id: &stored.metadata.request_id,
        id,
        expected_revision: revision,
        edit,
    };
    assert_eq!(client.edit(&body).unwrap_err().code, "request_uncertain");
    assert_eq!(client.edit(&body).unwrap_err().code, "request_conflict");
    assert_eq!(client.edit(&body).unwrap_err().code, "request_refused");
    worker.join().unwrap();
}
#[test]
fn original_request_read_uses_request_uncertainty_instead_of_scan_errors() {
    let stored = queued();
    let (endpoint, _, worker) = crate::daemon_client::tests::server(vec![
        (200, Value::Null),
        (
            503,
            json!({"code":"request_uncertain","request_id":stored.metadata.request_id,"error":"recovery sources changed during proof"}),
        ),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    let error = client
        .original_request(Path::new("/fixture"), &stored.metadata.request_id)
        .unwrap_err();
    assert_eq!(error.code, "request_uncertain");
    assert!(error.detail.unwrap().contains(&stored.metadata.request_id));
    worker.join().unwrap();
}
fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
pub(crate) fn project() -> (
    tempfile::TempDir,
    ProjectRegistry,
    String,
    towermd_core::catalog::Catalog,
) {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(
        temp.path(),
        &towermd_core::init::InitOptions { docs_dir: None },
    )
    .unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "fixture"]);
    let registry = ProjectRegistry::default();
    let token = registry
        .select(Some(temp.path().into()))
        .unwrap()
        .unwrap()
        .token;
    let catalog = context(&registry, &token).unwrap();
    (temp, registry, token, catalog)
}
#[test]
fn saved_projection_uses_core_opaque_classification_and_keeps_original_unknown_text() {
    let (_root, _registry, _token, catalog) = project();
    let text = "---\ntype: Module\nid: A\ntitle: 模块\nfuture: 原样\n---\n正文\r\n";
    let canvas = r#"{"nodes":[{"id":"A","type":"file","file":"docs/tower/arch/A.md","towermd":{"obj":"A"},"x":1,"y":2,"width":320,"height":180,"future":{"keep":true}},{"id":"opaque","type":"future","x":5,"y":6,"width":30,"height":40}],"edges":[],"future":"root"}"#;
    let value = json!({"state":"saved","id":"draft-1","project_id":catalog.config.project_id(),"revision":"a".repeat(40),"request_id":uuid::Uuid::now_v7().to_string(),"current_validity":{"state":"historical_only","reason":"target changed"},"unknown":"keep","draft":{"bundle":{"id":"draft-1","project_id":catalog.config.project_id(),"docs_dir":"docs/tower","source_revision":"b".repeat(40),"target_baseline":"towermd/B0","objects":{"A":{"id":"A","path":"docs/tower/arch/A.md","text":text}},"canvas_text":canvas,"deletions":[],"questions":[],"risks":[]},"files":{},"base_directory":{},"content_hash":"c".repeat(64)}});
    let view = project_saved(&catalog, "draft-1", value.clone()).unwrap();
    assert_eq!(view["draft"], value["draft"]);
    assert_eq!(view["unknown"], "keep");
    assert_eq!(view["object_views"][0]["text"], text);
    assert_eq!(view["object_views"][0]["type"], "Module");
    assert_eq!(view["canvas_view"]["nodes"][0]["opaque"], false);
    assert_eq!(view["canvas_view"]["nodes"][1]["opaque"], true);
    assert_eq!(view["current_validity"]["state"], "historical_only");
    for (field, expected) in [
        ("", None),
        ("verify: null\n", Some(Value::Null)),
        ("verify: legacy-command\n", Some(json!("legacy-command"))),
        ("verify: []\n", Some(json!([]))),
        ("verify:\n  ? [one, two]\n  : invalid\n", Some(Value::Null)),
    ] {
        let original = format!("---\ntype: Module\nid: A\n{field}---\nexact original body\n");
        let mut variant = value.clone();
        variant["draft"]["bundle"]["objects"]["A"]["text"] = json!(original);
        let projected = project_saved(&catalog, "draft-1", variant).unwrap();
        assert_eq!(
            projected["object_views"][0]["fields"].get("verify"),
            expected.as_ref()
        );
        assert_eq!(projected["object_views"][0]["text"], original);
    }
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("current_validity");
    assert!(project_saved(&catalog, "draft-1", missing).is_err());
    let mut wrong = value;
    wrong["project_id"] = json!("another");
    assert!(project_saved(&catalog, "draft-1", wrong).is_err());
}
#[test]
fn restart_reloads_original_pending_request_and_requires_current_token_registration() {
    let (_root, registry, token, catalog) = project();
    let app_data = tempfile::tempdir().unwrap();
    let mut stored = queued();
    stored.directory = catalog.root.clone();
    stored.common_dir = catalog.common_dir.clone();
    stored.request.project_id = catalog.config.project_id().into();
    stored.metadata =
        PreparedDraftSaveRequest::prepare(stored.request.clone(), &stored.metadata.request_id)
            .unwrap()
            .metadata()
            .clone();
    write_new(
        &request_path(app_data.path(), &stored.metadata.request_id).unwrap(),
        &stored,
    )
    .unwrap();
    let restarted = DraftRequests::default();
    let found = pending(&registry, &restarted, &token, "draft-1", app_data.path()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].request_id, stored.metadata.request_id);
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
            &stored.metadata.request_id,
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
            &stored.metadata.request_id,
            app_data.path()
        )
        .unwrap_err()
        .code,
        "request_not_prepared"
    );
    let recovered = pending(&registry, &restarted, &next, "draft-1", app_data.path()).unwrap();
    assert_eq!(recovered[0].request_id, stored.metadata.request_id);
    assert_eq!(
        read_queue(app_data.path(), &stored.metadata.request_id)
            .unwrap()
            .request,
        stored.request
    );
}

#[test]
fn original_read_failures_keep_edit_queue_and_explicit_retry_uses_same_body_and_uuid() {
    for transport_failure in [false, true] {
        let (_root, registry, token, catalog) = project();
        let app_data = tempfile::tempdir().unwrap();
        let mut stored = queued();
        stored.directory = catalog.root.clone();
        stored.common_dir = catalog.common_dir.clone();
        stored.request.project_id = catalog.config.project_id().into();
        stored.metadata =
            PreparedDraftSaveRequest::prepare(stored.request.clone(), &stored.metadata.request_id)
                .unwrap()
                .metadata()
                .clone();
        let id = &stored.metadata.request_id;
        let file = request_path(app_data.path(), id).unwrap();
        write_new(&file, &stored).unwrap();
        let bytes = fs::read(&file).unwrap();
        let requests = DraftRequests::default();
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

        let response = json!({"receipt":{"request_id":id,"body_hash":stored.metadata.body_hash,"result_ref":{"draft_id":"draft-1","revision":"b".repeat(40),"content_hash":"c".repeat(64),"reference":"refs/towermd/drafts/draft-1","delivery":{"state":"local_saved"}}},"current_validity":{"state":"current"}});
        let (endpoint, trace, worker) = crate::daemon_client::tests::server_with_health(vec![
            json!({"kind":"draft_save","p3_request":stored.metadata,"request_body":stored.request}),
            response,
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
            json!({"directory":stored.directory})
        );
    }
}

fn creation(catalog: &towermd_core::catalog::Catalog) -> QueuedEdit {
    let id = uuid::Uuid::now_v7().to_string();
    let prepared = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: catalog.config.project_id().into(),
            host: "fixture-host".into(),
            actor: "human:fixture".into(),
            run: None,
            action: DraftSaveAction::CreateDraft {
                baseline: "towermd/B0".into(),
                main_revision: catalog.revision.clone(),
            },
        },
        &id,
    )
    .unwrap();
    QueuedEdit {
        schema: 1,
        directory: catalog.root.clone(),
        common_dir: catalog.common_dir.clone(),
        request: prepared.body().clone(),
        metadata: prepared.metadata().clone(),
    }
}
#[test]
fn native_creation_queue_restarts_with_original_binding_without_breaking_edit_pending() {
    let (_root, registry, token, catalog) = project();
    let app_data = tempfile::tempdir().unwrap();
    let create = creation(&catalog);
    write_new(
        &request_path(app_data.path(), &create.metadata.request_id).unwrap(),
        &create,
    )
    .unwrap();
    let mut edit = queued();
    edit.directory = catalog.root.clone();
    edit.common_dir = catalog.common_dir.clone();
    edit.request.project_id = catalog.config.project_id().into();
    edit.metadata =
        PreparedDraftSaveRequest::prepare(edit.request.clone(), &edit.metadata.request_id)
            .unwrap()
            .metadata()
            .clone();
    write_new(
        &request_path(app_data.path(), &edit.metadata.request_id).unwrap(),
        &edit,
    )
    .unwrap();
    let requests = DraftRequests::default();
    let originals = pending_creations(&registry, &requests, &token, app_data.path()).unwrap();
    assert_eq!(originals.len(), 1);
    assert_eq!(originals[0].request_id, create.metadata.request_id);
    assert_eq!(originals[0].draft_id, create.metadata.request_id);
    assert_eq!(originals[0].main_revision, catalog.revision);
    let edits = pending(&registry, &requests, &token, "draft-1", app_data.path()).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].request_id, edit.metadata.request_id);
    assert_eq!(
        read_queue(app_data.path(), &create.metadata.request_id)
            .unwrap()
            .request,
        create.request
    );
    assert!(submit_with_kind(
        &registry,
        &requests,
        &token,
        &edit.metadata.request_id,
        app_data.path(),
        true,
        || panic!("wrong action must not connect")
    )
    .is_err());
    assert!(submit_with(
        &registry,
        &requests,
        &token,
        &create.metadata.request_id,
        app_data.path(),
        || panic!("wrong action must not connect")
    )
    .is_err());
}
#[test]
fn current_baseline_and_create_use_fixed_app_routes_and_exact_original_fields() {
    let (_root, _registry, _token, catalog) = project();
    let stored = creation(&catalog);
    let (endpoint, trace, worker) = crate::daemon_client::tests::server_with_health(vec![
        json!({"baseline":"towermd/B0","main_revision":catalog.revision}),
        json!({"actual":"receipt"}),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    let root = Path::new("/fixture root/中文");
    assert_eq!(
        client.current_baseline(root).unwrap()["baseline"],
        "towermd/B0"
    );
    client
        .create(&CreateHttpRequest {
            action: "create",
            directory: root,
            human_id: "fixture",
            request_id: &stored.metadata.request_id,
            baseline: "towermd/B0",
            main_revision: &catalog.revision,
        })
        .unwrap();
    worker.join().unwrap();
    let trace = trace.lock().unwrap();
    assert!(trace[1].starts_with("GET /app/baselines/current?directory="));
    assert!(trace[1].contains("%E4%B8%AD%E6%96%87"));
    assert!(trace[1].contains("x-towermd-client: app"));
    assert!(trace[2].starts_with("POST /drafts "));
    assert!(trace[2].contains("x-towermd-client: app"));
    let body: Value = serde_json::from_str(trace[2].split("\r\n").last().unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"action":"create","directory":root,"human_id":"fixture","request_id":stored.metadata.request_id,"baseline":"towermd/B0","main_revision":catalog.revision})
    );
}
#[test]
fn native_creation_uncertain_read_keeps_original_queue_and_retry_coordinates_same_uuid() {
    let (_root, registry, token, catalog) = project();
    let app_data = tempfile::tempdir().unwrap();
    let stored = creation(&catalog);
    let id = &stored.metadata.request_id;
    let path = request_path(app_data.path(), id).unwrap();
    write_new(&path, &stored).unwrap();
    let original_bytes = fs::read(&path).unwrap();
    let requests = DraftRequests::default();
    pending_creations(&registry, &requests, &token, app_data.path()).unwrap();
    let (endpoint, trace, worker) = crate::daemon_client::tests::server(vec![
        (200, Value::Null),
        (
            503,
            json!({"code":"request_uncertain","request_id":id,"error":"source read uncertain"}),
        ),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    let outcome = submit_with_kind(
        &registry,
        &requests,
        &token,
        id,
        app_data.path(),
        true,
        || Ok(client),
    )
    .unwrap();
    worker.join().unwrap();
    assert_eq!(outcome.state, "uncertain");
    assert_eq!(outcome.request_id, *id);
    assert!(!path.with_extension("resolved").exists());
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    assert!(trace
        .lock()
        .unwrap()
        .iter()
        .all(|line| !line.starts_with("POST ")));
    let response = json!({"receipt":{"request_id":id,"body_hash":stored.metadata.body_hash,"result_ref":{"draft_id":id,"revision":"a".repeat(40),"reference":format!("refs/towermd/drafts/{id}"),"content_hash":"b".repeat(64),"delivery":{"state":"local_saved"}}},"current_validity":{"state":"current"}});
    let (endpoint, trace, worker) = crate::daemon_client::tests::server_with_health(vec![
        json!({"kind":"draft_save","request_body":stored.request,"p3_request":stored.metadata}),
        response,
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    let outcome = submit_with_kind(
        &registry,
        &requests,
        &token,
        id,
        app_data.path(),
        true,
        || Ok(client),
    )
    .unwrap();
    worker.join().unwrap();
    assert_eq!(outcome.state, "confirmed");
    assert_eq!(outcome.request_id, *id);
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    assert!(
        pending_creations(&registry, &requests, &token, app_data.path())
            .unwrap()
            .is_empty()
    );
    let trace = trace.lock().unwrap();
    assert!(trace[2].starts_with(&format!("POST /app/requests/{id}/retry ")));
    let body: Value = serde_json::from_str(trace[2].split("\r\n").last().unwrap()).unwrap();
    assert_eq!(body, json!({"directory":catalog.root}));
}
#[test]
fn native_create_prepare_requires_exact_fresh_baseline_and_main_before_queue_write() {
    let (_root, registry, token, catalog) = project();
    let app_data = tempfile::tempdir().unwrap();
    let requests = DraftRequests::default();
    let (endpoint, _, worker) = crate::daemon_client::tests::server_with_health(vec![
        json!({"baseline":"towermd/B0","main_revision":catalog.revision}),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    let result = prepare_create_with(
        &registry,
        &requests,
        &token,
        "towermd/B0",
        &"a".repeat(40),
        app_data.path(),
        || Ok(client),
    );
    worker.join().unwrap();
    assert_eq!(result.err().unwrap().code, "request_conflict");
    assert!(
        pending_creations(&registry, &requests, &token, app_data.path())
            .unwrap()
            .is_empty()
    );
    let (endpoint, _, worker) = crate::daemon_client::tests::server_with_health(vec![
        json!({"baseline":"towermd/B0","main_revision":catalog.revision}),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    let result = prepare_create_with(
        &registry,
        &requests,
        &token,
        "towermd/B0",
        &catalog.revision,
        app_data.path(),
        || Ok(client),
    )
    .unwrap();
    worker.join().unwrap();
    let stored = read_queue(app_data.path(), &result.request_id).unwrap();
    assert_eq!(result.draft_id, result.request_id);
    assert_eq!(stored.request.actor, identity().unwrap());
    assert_eq!(stored.request.run, None);
    assert_eq!(
        pending_creations(&registry, &requests, &token, app_data.path()).unwrap()[0].request_id,
        result.request_id
    );
}
#[test]
fn native_creation_first_send_keeps_prepared_body_and_changed_host_cannot_replace_it() {
    for changed_host in [false, true] {
        let (_root, registry, token, catalog) = project();
        let app_data = tempfile::tempdir().unwrap();
        let mut stored = creation(&catalog);
        stored.request.actor = identity().unwrap();
        if changed_host {
            stored.request.host = "old-host".into();
        }
        stored.metadata =
            PreparedDraftSaveRequest::prepare(stored.request.clone(), &stored.metadata.request_id)
                .unwrap()
                .metadata()
                .clone();
        let id = &stored.metadata.request_id;
        let path = request_path(app_data.path(), id).unwrap();
        write_new(&path, &stored).unwrap();
        let bytes = fs::read(&path).unwrap();
        let requests = DraftRequests::default();
        pending_creations(&registry, &requests, &token, app_data.path()).unwrap();
        let mut replies = vec![
            (200, Value::Null),
            (404, json!({"error":"unknown request"})),
        ];
        if !changed_host {
            replies.push((200,json!({"receipt":{"request_id":id,"body_hash":stored.metadata.body_hash,"result_ref":{"draft_id":id,"revision":"a".repeat(40),"reference":format!("refs/towermd/drafts/{id}"),"content_hash":"b".repeat(64),"delivery":{"state":"local_saved_remote_pending","reason":"offline"}}},"current_validity":{"state":"current"}})));
        }
        let (endpoint, trace, worker) = crate::daemon_client::tests::server(replies);
        let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
        let outcome = submit_with_kind(
            &registry,
            &requests,
            &token,
            id,
            app_data.path(),
            true,
            || Ok(client),
        );
        worker.join().unwrap();
        let trace = trace.lock().unwrap();
        if changed_host {
            assert_eq!(outcome.unwrap_err().code, "request_context_changed");
            assert!(trace.iter().all(|line| !line.starts_with("POST ")));
            assert!(!path.with_extension("resolved").exists());
        } else {
            assert_eq!(outcome.unwrap().state, "confirmed");
            assert!(trace[2].starts_with("POST /drafts "));
            let body: Value = serde_json::from_str(trace[2].split("\r\n").last().unwrap()).unwrap();
            assert_eq!(
                body,
                json!({"action":"create","directory":catalog.root,"human_id":stored.request.actor.strip_prefix("human:").unwrap(),"request_id":id,"baseline":"towermd/B0","main_revision":catalog.revision})
            );
        }
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
#[test]
fn native_creation_original_body_mismatch_is_not_retried_or_marked_resolved() {
    let (_root, registry, token, catalog) = project();
    let app_data = tempfile::tempdir().unwrap();
    let stored = creation(&catalog);
    let id = &stored.metadata.request_id;
    let path = request_path(app_data.path(), id).unwrap();
    write_new(&path, &stored).unwrap();
    let requests = DraftRequests::default();
    pending_creations(&registry, &requests, &token, app_data.path()).unwrap();
    let mut body = serde_json::to_value(&stored.request).unwrap();
    body["action"]["main_revision"] = json!("d".repeat(40));
    let (endpoint, trace, worker) = crate::daemon_client::tests::server_with_health(vec![
        json!({"kind":"draft_save","request_body":body,"p3_request":stored.metadata}),
    ]);
    let client = daemon_client::DaemonClient::connect(endpoint).unwrap();
    let error = submit_with_kind(
        &registry,
        &requests,
        &token,
        id,
        app_data.path(),
        true,
        || Ok(client),
    )
    .unwrap_err();
    worker.join().unwrap();
    assert_eq!(error.code, "request_conflict");
    assert!(!path.with_extension("resolved").exists());
    assert!(trace
        .lock()
        .unwrap()
        .iter()
        .all(|line| !line.starts_with("POST ")));
}
