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
