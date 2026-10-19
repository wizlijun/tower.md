//! Source-only protocol tests; Root runs these after the actual producer is integrated.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
};
type Trace = Arc<Mutex<Vec<String>>>;
fn server(
    instance: &str,
    host: &str,
    responses: Vec<(u16, Value)>,
) -> (daemon_client::Endpoint, Trace, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let health = json!({"address":"127.0.0.1","port":listener.local_addr().unwrap().port(),"pid":42,"instance":instance,"host":host});
    let endpoint = serde_json::from_value(health.clone()).unwrap();
    let trace = Arc::new(Mutex::new(vec![]));
    let captured = trace.clone();
    let worker = thread::spawn(move || {
        for (status, value) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut text = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    text.push_str(&line);
                    break;
                }
                assert!(!line.is_empty(), "request ended before headers");
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                text.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            text.push_str(&String::from_utf8(body).unwrap());
            captured.lock().unwrap().push(text);
            let body = if value.is_null() {
                health.clone()
            } else {
                value
            }
            .to_string();
            write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    (endpoint, trace, worker)
}
fn view() -> Value {
    let task = json!({"type":"Task","id":"task-1","title":"read this <script> literally","module":"A","touches":["B"],"state":"running","needs_recheck":true});
    let parsed: towermd_core::tasks::Task = serde_json::from_value(task.clone()).unwrap();
    json!({"binding":{"task_id":"task-1","target_baseline":"towermd/B2","task_content_hash":towermd_core::proposals::task_content_hash(&parsed).unwrap(),"publication_ids":["publication-1","publication-2"],"running":{"run":"run-1","fences":[{"name":"module:A","holder":"run-1","epoch":1},{"name":"module:B","holder":"run-1","epoch":2}],"lease_expires_at":{"module:A":1800000100000u64,"module:B":1800000200000u64}}},"task_text":task.to_string(),"reasons":[{"task":"task-1","old_baseline":"towermd/B0","new_baseline":"towermd/B1","publication_id":"publication-1","reasons":["module contract changed"]},{"task":"task-1","old_baseline":"towermd/B1","new_baseline":"towermd/B2","publication_id":"publication-2","reasons":["new acceptance"]}]})
}
fn load_fixture(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    data: &Path,
) -> Value {
    let (endpoint, trace, worker) = server(
        "instance-1",
        "fixture-host",
        vec![(200, Value::Null), (200, view()), (200, Value::Null)],
    );
    let loaded = load_with(registry, requests, token, "task-1", data, || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    assert!(trace.lock().unwrap()[1].starts_with("GET /app/tasks/task-1/recheck?directory="));
    loaded
}
fn prepare_fixture(
    registry: &ProjectRegistry,
    requests: &TaskRecheckRequests,
    token: &str,
    data: &Path,
) -> PreparedTaskRecheck {
    let loaded = load_fixture(registry, requests, token, data);
    let (endpoint, trace, worker) = server(
        "instance-1",
        "fixture-host",
        vec![(200, Value::Null), (200, view()), (200, Value::Null)],
    );
    let prepared = prepare_with(
        registry,
        requests,
        token,
        loaded["recheck_ticket"].as_str().unwrap(),
        "complete original reviewed".into(),
        data,
        || daemon_client::DaemonClient::connect(endpoint),
    )
    .unwrap();
    worker.join().unwrap();
    assert!(trace
        .lock()
        .unwrap()
        .iter()
        .all(|r| !r.starts_with("POST ")));
    prepared
}
fn receipt(stored: &QueuedRecheck) -> Value {
    json!({"receipt":{"request_id":stored.metadata.request_id,"body_hash":stored.metadata.body_hash,"result_ref":{"request_id":stored.metadata.request_id,"binding":action(stored).0,"actor":stored.request.actor,"resolved_eid":request_event_eid(&stored.metadata.request_id,"task.recheck_resolved").unwrap()}},"current_validity":{"state":"current"}})
}
#[test]
fn native_fixed_routes_save_original_before_send_and_accept_only_exact_receipt() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = TaskRecheckRequests::default();
    let prepared = prepare_fixture(&registry, &requests, &token, data.path());
    let id = &prepared.request_id;
    let file = path(data.path(), id).unwrap();
    let bytes = fs::read(&file).unwrap();
    let stored = read(data.path(), id).unwrap();
    assert!(write_new(&file, &stored).is_err());
    assert_eq!(stored.request.run, Some("run-1".into()));
    assert_eq!(stored.request.actor, draft_requests::identity().unwrap());
    let (endpoint, trace, worker) = server(
        "restarted-instance",
        "fixture-host",
        vec![
            (200, Value::Null),
            (200, receipt(&stored)),
            (200, Value::Null),
        ],
    );
    let result = submit_with(&registry, &requests, &token, id, data.path(), || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    assert_eq!(result.state, "confirmed");
    assert_eq!(fs::read(file).unwrap(), bytes);
    let trace = trace.lock().unwrap();
    assert!(trace[1].starts_with("POST /app/tasks/task-1/recheck/resolve "));
    assert!(trace[1].contains("x-towermd-instance: restarted-instance"));
    let body: Value = serde_json::from_str(trace[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"directory":catalog.root,"human_id":stored.request.actor.strip_prefix("human:").unwrap(),"action":original_action(&stored)})
    );
    assert!(body.get("actor").is_none() && body.get("run").is_none() && body.get("host").is_none());
}
#[test]
fn changed_displayed_task_publications_or_expiry_never_saves_a_replacement() {
    for pointer in [
        "/binding/publication_ids",
        "/binding/task_content_hash",
        "/binding/running/lease_expires_at/module:A",
        "/reasons/0/reasons/0",
    ] {
        let (_root, registry, token, _) = crate::draft_requests::tests::project();
        let data = tempfile::tempdir().unwrap();
        let requests = TaskRecheckRequests::default();
        let loaded = load_fixture(&registry, &requests, &token, data.path());
        let mut changed = view();
        match pointer {
            "/binding/publication_ids" => {
                changed["binding"]["publication_ids"] = json!(["publication-1"]);
                changed["reasons"].as_array_mut().unwrap().pop();
            }
            "/binding/task_content_hash" => {
                changed["binding"]["task_content_hash"] = json!("a".repeat(64));
            }
            "/reasons/0/reasons/0" => {
                changed["reasons"][0]["reasons"][0] = json!("changed reason");
            }
            _ => {
                changed["binding"]["running"]["lease_expires_at"]["module:A"] =
                    json!(1800000999999u64);
            }
        }
        let (endpoint, trace, worker) = server(
            "instance-1",
            "fixture-host",
            vec![(200, Value::Null), (200, changed), (200, Value::Null)],
        );
        assert!(prepare_with(
            &registry,
            &requests,
            &token,
            loaded["recheck_ticket"].as_str().unwrap(),
            "read".into(),
            data.path(),
            || daemon_client::DaemonClient::connect(endpoint)
        )
        .is_err());
        worker.join().unwrap();
        assert!(pending(&registry, &requests, &token, "task-1", data.path())
            .unwrap()
            .is_empty());
        assert!(trace
            .lock()
            .unwrap()
            .iter()
            .all(|r| !r.starts_with("POST ")));
    }
}
#[test]
fn full_receipt_binding_including_each_expiry_actor_and_derived_event_is_required() {
    let (_root, registry, token, _) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = TaskRecheckRequests::default();
    let prepared = prepare_fixture(&registry, &requests, &token, data.path());
    let stored = read(data.path(), &prepared.request_id).unwrap();
    let original = receipt(&stored);
    assert!(confirmed(&stored, &original));
    for pointer in [
        "/receipt/request_id",
        "/receipt/body_hash",
        "/receipt/result_ref/request_id",
        "/receipt/result_ref/actor",
        "/receipt/result_ref/resolved_eid",
        "/receipt/result_ref/binding/task_content_hash",
        "/receipt/result_ref/binding/target_baseline",
        "/receipt/result_ref/binding/running/run",
        "/receipt/result_ref/binding/running/lease_expires_at/module:A",
    ] {
        let mut wrong = original.clone();
        *wrong.pointer_mut(pointer).unwrap() = if pointer.contains("expires_at") {
            json!(1)
        } else {
            json!("different")
        };
        assert!(!confirmed(&stored, &wrong), "{pointer}");
    }
    for pointer in [
        "/receipt/result_ref/binding/publication_ids",
        "/receipt/result_ref/binding/running/fences",
    ] {
        let mut wrong = original.clone();
        wrong
            .pointer_mut(pointer)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(!confirmed(&stored, &wrong), "{pointer}");
    }
    let mut wrong = original.clone();
    wrong["receipt"]["result_ref"]["binding"]["running"]["lease_expires_at"]
        .as_object_mut()
        .unwrap()
        .remove("module:B");
    assert!(!confirmed(&stored, &wrong));
    let mut old = original;
    old["current_validity"] =
        json!({"state":"historical_only","reason":"later publication remains pending"});
    assert!(
        confirmed(&stored, &old),
        "authentic historical receipt is retained, not promoted to current"
    );
}
#[test]
fn uncertain_keeps_same_uuid_bytes_then_original_retry_can_confirm() {
    let (_root, registry, token, _) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = TaskRecheckRequests::default();
    let prepared = prepare_fixture(&registry, &requests, &token, data.path());
    let id = &prepared.request_id;
    let file = path(data.path(), id).unwrap();
    let bytes = fs::read(&file).unwrap();
    let stored = read(data.path(), id).unwrap();
    let (endpoint, _, worker) = server(
        "instance-1",
        "fixture-host",
        vec![
            (200, Value::Null),
            (200, json!({"not":"receipt"})),
            (200, Value::Null),
        ],
    );
    assert_eq!(
        submit_with(&registry, &requests, &token, id, data.path(), || {
            daemon_client::DaemonClient::connect(endpoint)
        })
        .unwrap()
        .state,
        "uncertain"
    );
    worker.join().unwrap();
    assert!(!file.with_extension("resolved").exists());
    assert_eq!(fs::read(&file).unwrap(), bytes);
    let fresh = TaskRecheckRequests::default();
    let pending = pending(&registry, &fresh, &token, "task-1", data.path()).unwrap();
    assert_eq!(pending[0].request_id, *id);
    let (endpoint, _, worker) = server(
        "next-instance",
        "fixture-host",
        vec![
            (200, Value::Null),
            (200, receipt(&stored)),
            (200, Value::Null),
        ],
    );
    assert_eq!(
        submit_with(&registry, &fresh, &token, id, data.path(), || {
            daemon_client::DaemonClient::connect(endpoint)
        })
        .unwrap()
        .state,
        "confirmed"
    );
    worker.join().unwrap();
    assert_eq!(fs::read(file).unwrap(), bytes);
}
#[test]
fn path_injection_and_unbound_original_are_refused_before_network() {
    let (_root, registry, token, _) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = TaskRecheckRequests::default();
    assert!(load_with(
        &registry,
        &requests,
        &token,
        "../task",
        data.path(),
        || panic!("invalid task must not connect")
    )
    .is_err());
    let prepared = prepare_fixture(&registry, &requests, &token, data.path());
    let fresh = TaskRecheckRequests::default();
    assert!(submit_with(
        &registry,
        &fresh,
        &token,
        &prepared.request_id,
        data.path(),
        || panic!("unbound UUID must not connect")
    )
    .is_err());
    let mut stored = read(data.path(), &prepared.request_id).unwrap();
    stored.metadata.request_id = uuid::Uuid::now_v7().to_string();
    fs::write(
        path(data.path(), &prepared.request_id).unwrap(),
        serde_json::to_vec(&stored).unwrap(),
    )
    .unwrap();
    assert!(read(data.path(), &prepared.request_id).is_err());
}
#[test]
fn writer_checkpoint_may_advance_head_but_cannot_change_project_or_authority_context() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&catalog.root)
        .args([
            "commit",
            "--allow-empty",
            "-m",
            "simulated writer checkpoint",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        check_source(&registry, &token, &catalog).is_err(),
        "a displayed ticket cannot prepare from a changed source"
    );
    assert!(
        check_source_identity(&registry, &token, &catalog, false).is_ok(),
        "an exact original receipt may follow its own writer checkpoint"
    );
    assert!(registry.select(None).unwrap().is_none());
    assert!(
        check_source_identity(&registry, &token, &catalog, false).is_ok(),
        "cancelling the folder picker preserves the selected project"
    );
    let selected = registry
        .select(Some(catalog.root.clone()))
        .unwrap()
        .unwrap();
    assert_ne!(selected.token, token);
    assert!(
        check_source_identity(&registry, &token, &catalog, false).is_err(),
        "checkpoint allowance does not revive a stale project token"
    );
    assert!(check_source_identity(&registry, &selected.token, &catalog, false).is_ok());
}
