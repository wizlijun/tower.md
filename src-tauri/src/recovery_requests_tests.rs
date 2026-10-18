use super::*;
use serde_json::{json, Value};
fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@invalid",
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
fn project() -> (tempfile::TempDir, ProjectRegistry, String, Catalog) {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(
        temp.path(),
        &towermd_core::init::InitOptions { docs_dir: None },
    )
    .unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "recovery adapter fixture"]);
    let registry = ProjectRegistry::default();
    let token = registry
        .select(Some(temp.path().into()))
        .unwrap()
        .unwrap()
        .token;
    let catalog = Catalog::load(temp.path()).unwrap();
    (temp, registry, token, catalog)
}
fn saved(catalog: &Catalog) -> Value {
    let mut request = RecoveryRequest {
        status: "awaiting_app_confirmation".into(),
        request_id: String::new(),
        project_id: catalog.config.project_id().into(),
        main_revision: catalog.revision.clone(),
        source_commit: "b".repeat(40),
        source_seq: 42,
        seq_boundary: 4294967338,
        epoch_boundary: 4294967310,
        source_epoch_high_water: 14,
        source_reference: "main;refs/towermd/snapshot:missing".into(),
        inputs_digest: "c".repeat(64),
        requested_at: 1000,
        expires_at: 301000,
        nonce: uuid::Uuid::now_v7().to_string(),
    };
    request.request_id = "a".repeat(64);
    json!({"expires_at":request.expires_at,"request":request,"directory":catalog.root,"common_dir":catalog.common_dir})
}
#[test]
fn saved_request_from_another_project_cannot_be_confirmed() {
    let (_temp, _registry, _token, catalog) = project();
    let mut value = saved(&catalog);
    let id = value["request"]["request_id"].as_str().unwrap().to_string();
    value["request"]["project_id"] = json!("another-project");
    assert!(
        checked_saved(&catalog, &id, value).is_err(),
        "a foreign saved project must not become a recovery confirmation"
    );
}
#[test]
fn strict_id_and_saved_context_refuse_paths_and_replacement_expiry() {
    for id in [
        "",
        "../source",
        &"A".repeat(64),
        &"a".repeat(63),
        &"a".repeat(65),
    ] {
        assert!(safe_id(id).is_err());
    }
    let (_temp, _registry, _token, catalog) = project();
    let original = saved(&catalog);
    let id = original["request"]["request_id"].as_str().unwrap();
    for key in [
        "common_dir",
        "directory",
        "expires_at",
        "request_id",
        "status",
    ] {
        let mut value = original.clone();
        match key {
            "common_dir" => value[key] = json!("/not/selected/.git"),
            "directory" => value[key] = json!("/missing/recovery/project"),
            "expires_at" => value[key] = json!(301001),
            "request_id" => value["request"][key] = json!("b".repeat(64)),
            _ => value["request"][key] = json!("ready"),
        };
        assert!(
            checked_saved(&catalog, id, value).is_err(),
            "bad saved {key} must be rejected"
        );
    }
}
fn report(value: &Value) -> Value {
    json!({"status":"rebuilt","commit":"d".repeat(40),"seq":value["request"]["seq_boundary"].as_u64().unwrap()+3,"epoch":value["request"]["epoch_boundary"].as_u64().unwrap()+1,"reason":null})
}
#[test]
fn saved_get_and_explicit_confirm_use_exact_fixed_id_and_native_actor_only() {
    let (_temp, registry, token, catalog) = project();
    let requests = RebuildRequests::default();
    let value = saved(&catalog);
    let id = value["request"]["request_id"].as_str().unwrap();
    let (endpoint, trace, worker) = crate::daemon_client::tests::server(vec![
        (200, Value::Null),
        (200, value.clone()),
        (200, Value::Null),
        (200, value.clone()),
        (200, report(&value)),
        (200, Value::Null),
        (200, value.clone()),
    ]);
    let first = load_with(&registry, &requests, &token, id, || {
        daemon_client::DaemonClient::connect(endpoint.clone())
    })
    .unwrap();
    assert_eq!(first.request.expires_at, 301000);
    assert!(first.can_confirm);
    assert_eq!(trace.lock().unwrap().len(), 2);
    let result = confirm_with(&registry, &requests, &token, id, || {
        daemon_client::DaemonClient::connect(endpoint.clone())
    })
    .unwrap();
    assert_eq!(result.status, "rebuilt");
    assert_eq!(
        result.commit.as_deref(),
        Some("dddddddddddddddddddddddddddddddddddddddd")
    );
    assert_eq!(
        confirm_with(&registry, &requests, &token, id, || panic!(
            "never rediscover for attempted ID"
        ))
        .err()
        .unwrap()
        .code,
        "recovery_already_attempted"
    );
    let refreshed = load_with(&registry, &requests, &token, id, || {
        daemon_client::DaemonClient::connect(endpoint.clone())
    })
    .unwrap();
    assert!(!refreshed.can_confirm);
    assert_eq!(refreshed.request, first.request);
    worker.join().unwrap();
    let trace = trace.lock().unwrap();
    assert_eq!(trace.len(), 7);
    assert!(trace[1].starts_with(&format!("GET /hub/rebuild/{id} HTTP/")));
    assert!(trace[3].starts_with(&format!("GET /hub/rebuild/{id} HTTP/")));
    assert!(trace[4].starts_with(&format!("POST /hub/rebuild/{id}/confirm HTTP/")));
    assert!(trace[4].contains("x-towermd-client: app"));
    assert!(trace[4].contains("x-towermd-instance: fixture-instance"));
    let body: Value = serde_json::from_str(trace[4].split("\r\n").last().unwrap()).unwrap();
    assert_eq!(body, json!({"actor":draft_requests::identity().unwrap()}));
    assert!(!trace
        .iter()
        .any(|request| request.starts_with("POST /rebuild-request ")));
}
#[test]
fn unviewed_invalid_and_stale_project_requests_never_submit() {
    let (_temp, registry, token, catalog) = project();
    let requests = RebuildRequests::default();
    let value = saved(&catalog);
    let id = value["request"]["request_id"].as_str().unwrap();
    assert_eq!(
        confirm_with(&registry, &requests, &token, id, || panic!(
            "no read admission"
        ))
        .err()
        .unwrap()
        .code,
        "recovery_not_viewed"
    );
    assert_eq!(
        load_with(&registry, &requests, &token, "../source", || panic!(
            "unsafe ID"
        ))
        .err()
        .unwrap()
        .code,
        "recovery_id_invalid"
    );
    let (endpoint, trace, worker) = crate::daemon_client::tests::server(vec![(200, Value::Null)]);
    let result = load_with(&registry, &requests, &token, id, || {
        let client = daemon_client::DaemonClient::connect(endpoint)?;
        registry.select(Some(catalog.root.clone())).unwrap();
        Ok(client)
    });
    assert_eq!(result.err().unwrap().code, "project_missing");
    worker.join().unwrap();
    assert_eq!(trace.lock().unwrap().len(), 1);
}
#[test]
fn changed_instance_and_changed_saved_body_never_restage_the_old_request() {
    for change in ["instance", "body"] {
        let (_temp, registry, token, catalog) = project();
        let requests = RebuildRequests::default();
        let value = saved(&catalog);
        let id = value["request"]["request_id"].as_str().unwrap();
        let mut changed = value.clone();
        changed["expires_at"] = json!(301100);
        changed["request"]["requested_at"] = json!(1100);
        changed["request"]["expires_at"] = json!(301100);
        let mut replies = vec![(200, Value::Null), (200, value.clone()), (200, Value::Null)];
        if change == "body" {
            replies.push((200, changed));
        }
        let (endpoint, trace, worker) = crate::daemon_client::tests::server(replies);
        load_with(&registry, &requests, &token, id, || {
            daemon_client::DaemonClient::connect(endpoint.clone())
        })
        .unwrap();
        if change == "instance" {
            requests
                .0
                .lock()
                .unwrap()
                .views
                .get_mut(id)
                .unwrap()
                .instance = "previous-instance".into();
        }
        let error = confirm_with(&registry, &requests, &token, id, || {
            daemon_client::DaemonClient::connect(endpoint.clone())
        })
        .err()
        .unwrap();
        assert_eq!(error.code, "recovery_conflict");
        worker.join().unwrap();
        assert!(!trace
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.starts_with("POST ")));
        assert!(!requests.0.lock().unwrap().attempted.contains(id));
    }
}
#[test]
fn exact_http_failures_and_persisted_boundary_keep_old_card_audit_without_repeating_post() {
    for (status, message, code) in [
        (409, "恢复请求已过期；须新请求", "recovery_conflict"),
        (
            409,
            "main/private 来源已变化，须新请求",
            "recovery_conflict",
        ),
        (403, "恢复只能由 App 确认", "recovery_forbidden"),
        (
            404,
            "恢复请求不存在；请运行 hub rebuild",
            "recovery_missing",
        ),
        (
            400,
            "恢复边界 eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee 已持久化，本地安装失败；须新请求",
            "recovery_failed",
        ),
        (503, "恢复送达状态不确定", "recovery_uncertain"),
    ] {
        let (_temp, registry, token, catalog) = project();
        let requests = RebuildRequests::default();
        let value = saved(&catalog);
        let id = value["request"]["request_id"].as_str().unwrap();
        let (endpoint, trace, worker) = crate::daemon_client::tests::server(vec![
            (200, Value::Null),
            (200, value.clone()),
            (200, Value::Null),
            (200, value.clone()),
            (status, json!({"error":message})),
        ]);
        load_with(&registry, &requests, &token, id, || {
            daemon_client::DaemonClient::connect(endpoint.clone())
        })
        .unwrap();
        let error = confirm_with(&registry, &requests, &token, id, || {
            daemon_client::DaemonClient::connect(endpoint.clone())
        })
        .err()
        .unwrap();
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(
            serde_json::from_str::<Value>(error.detail.as_deref().unwrap()).unwrap(),
            json!({"error":message})
        );
        assert_eq!(
            confirm_with(&registry, &requests, &token, id, || panic!(
                "old ID must not repeat"
            ))
            .err()
            .unwrap()
            .code,
            "recovery_already_attempted"
        );
        worker.join().unwrap();
        assert_eq!(
            trace
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.starts_with("POST "))
                .count(),
            1
        );
    }
}
#[test]
fn missing_saved_request_does_not_issue_a_new_request_and_invalid_success_is_uncertain() {
    let (_temp, registry, token, catalog) = project();
    let requests = RebuildRequests::default();
    let value = saved(&catalog);
    let id = value["request"]["request_id"].as_str().unwrap();
    let (endpoint, trace, worker) = crate::daemon_client::tests::server(vec![
        (200, Value::Null),
        (404, json!({"error":"恢复请求不存在；请运行 hub rebuild"})),
    ]);
    assert_eq!(
        load_with(&registry, &requests, &token, id, || {
            daemon_client::DaemonClient::connect(endpoint)
        })
        .err()
        .unwrap()
        .code,
        "recovery_missing"
    );
    worker.join().unwrap();
    assert_eq!(trace.lock().unwrap().len(), 2);
    let (endpoint, trace, worker) = crate::daemon_client::tests::server(vec![
        (200, Value::Null),
        (200, value.clone()),
        (200, Value::Null),
        (200, value.clone()),
        (
            200,
            json!({"status":"rebuilt","commit":"fake","seq":1,"epoch":1,"reason":null}),
        ),
    ]);
    load_with(&registry, &requests, &token, id, || {
        daemon_client::DaemonClient::connect(endpoint.clone())
    })
    .unwrap();
    assert_eq!(
        confirm_with(&registry, &requests, &token, id, || {
            daemon_client::DaemonClient::connect(endpoint.clone())
        })
        .err()
        .unwrap()
        .code,
        "recovery_uncertain"
    );
    worker.join().unwrap();
    assert_eq!(
        trace
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.starts_with("POST "))
            .count(),
        1
    );
}

#[test]
fn foreign_saved_http_body_and_changed_human_identity_cannot_reach_confirmation() {
    let (_temp, registry, token, catalog) = project();
    let requests = RebuildRequests::default();
    let value = saved(&catalog);
    let id = value["request"]["request_id"].as_str().unwrap();
    let mut foreign = value.clone();
    foreign["request"]["project_id"] = json!("another-project");
    let (endpoint, trace, worker) =
        crate::daemon_client::tests::server(vec![(200, Value::Null), (200, foreign)]);
    assert_eq!(
        load_with(&registry, &requests, &token, id, || {
            daemon_client::DaemonClient::connect(endpoint)
        })
        .err()
        .unwrap()
        .code,
        "recovery_context_changed"
    );
    worker.join().unwrap();
    assert_eq!(trace.lock().unwrap().len(), 2);
    assert!(requests.0.lock().unwrap().views.is_empty());
    let (endpoint, trace, worker) =
        crate::daemon_client::tests::server(vec![(200, Value::Null), (200, value.clone())]);
    load_with(&registry, &requests, &token, id, || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    requests.0.lock().unwrap().views.get_mut(id).unwrap().actor = "human:previous-user".into();
    assert_eq!(
        confirm_with(&registry, &requests, &token, id, || panic!(
            "changed native identity cannot discover or submit"
        ))
        .err()
        .unwrap()
        .code,
        "recovery_context_changed"
    );
    assert_eq!(trace.lock().unwrap().len(), 2);
}

#[test]
fn changing_project_after_read_invalidates_both_old_and_new_token_confirmation() {
    let (_temp, registry, token, catalog) = project();
    let requests = RebuildRequests::default();
    let value = saved(&catalog);
    let id = value["request"]["request_id"].as_str().unwrap();
    let (endpoint, trace, worker) =
        crate::daemon_client::tests::server(vec![(200, Value::Null), (200, value.clone())]);
    load_with(&registry, &requests, &token, id, || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    let new_token = registry
        .select(Some(catalog.root.clone()))
        .unwrap()
        .unwrap()
        .token;
    assert_eq!(
        confirm_with(&registry, &requests, &token, id, || panic!(
            "old token cannot discover"
        ))
        .err()
        .unwrap()
        .code,
        "project_missing"
    );
    assert_eq!(
        confirm_with(&registry, &requests, &new_token, id, || panic!(
            "new token has not reviewed this request"
        ))
        .err()
        .unwrap()
        .code,
        "recovery_context_changed"
    );
    assert_eq!(trace.lock().unwrap().len(), 2);
}
