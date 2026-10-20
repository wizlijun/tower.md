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

// Capture names, bytes, kinds and modification times; reads may change atime.
// The project fixture includes its actual Git refs and initialized store paths.
fn history_files(root: &Path) -> BTreeMap<PathBuf, (u8, Vec<u8>, std::time::SystemTime)> {
    fn visit(
        root: &Path,
        path: &Path,
        files: &mut BTreeMap<PathBuf, (u8, Vec<u8>, std::time::SystemTime)>,
    ) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let (kind, bytes) = if metadata.file_type().is_symlink() {
            (
                2,
                fs::read_link(path)
                    .unwrap()
                    .to_string_lossy()
                    .as_bytes()
                    .to_vec(),
            )
        } else if metadata.is_dir() {
            (1, vec![])
        } else {
            (0, fs::read(path).unwrap())
        };
        files.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            (kind, bytes, metadata.modified().unwrap()),
        );
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), files);
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn resolved_history_is_complete_without_current_view_and_does_not_write() {
    let (root, registry, token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = TaskRecheckRequests::default();
    let prepared = prepare_fixture(&registry, &requests, &token, data.path());
    let stored = read(data.path(), &prepared.request_id).unwrap();
    let (endpoint, _, worker) = server(
        "instance-1",
        "fixture-host",
        vec![
            (200, Value::Null),
            (200, receipt(&stored)),
            (200, Value::Null),
        ],
    );
    assert_eq!(
        submit_with(
            &registry,
            &requests,
            &token,
            &prepared.request_id,
            data.path(),
            || { daemon_client::DaemonClient::connect(endpoint) }
        )
        .unwrap()
        .state,
        "confirmed"
    );
    worker.join().unwrap();
    assert!(path(data.path(), &prepared.request_id)
        .unwrap()
        .with_extension("resolved")
        .exists());

    // Keep a real existing Hub journal locked: local history must neither open
    // its producer nor modify the journal, refs or recovery state.
    let store = catalog.common_dir.join("towermd");
    let _owned_hub = towermd_core::hub::Hub::open(&store).unwrap();
    assert!(!fs::read(store.join("journal.jsonl")).unwrap().is_empty());
    // A restart has neither a displayed view nor an approval ticket. All fixture
    // daemon listeners have closed; history cannot request a current Task view.
    let fresh = TaskRecheckRequests::default();
    let before_data = history_files(data.path());
    let before_project = history_files(root.path());
    let before_common = history_files(&catalog.common_dir);
    let originals = history(&registry, &fresh, &token, "task-1", data.path()).unwrap();
    assert_eq!(originals.len(), 1);
    assert_eq!(
        serde_json::to_value(&originals[0]).unwrap(),
        serde_json::to_value(&prepared).unwrap()
    );
    assert!(fresh.0.lock().unwrap().views.is_empty());
    assert_eq!(
        fresh.0.lock().unwrap().bound.get(&prepared.request_id),
        Some(&token)
    );
    assert_eq!(history_files(data.path()), before_data);
    assert_eq!(history_files(root.path()), before_project);
    assert_eq!(history_files(&catalog.common_dir), before_common);
    assert!(pending(&registry, &fresh, &token, "task-1", data.path())
        .unwrap()
        .is_empty());
}

#[test]
fn missing_history_never_creates_queue_app_data_or_ticket() {
    let (root, registry, token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let fresh = TaskRecheckRequests::default();
    for location in [
        data.path().to_path_buf(),
        data.path().join("missing-app-data"),
    ] {
        let before_data = history_files(data.path());
        let before_project = history_files(root.path());
        let before_common = history_files(&catalog.common_dir);
        assert!(history(&registry, &fresh, &token, "task-1", &location)
            .unwrap()
            .is_empty());
        assert_eq!(history_files(data.path()), before_data);
        assert_eq!(history_files(root.path()), before_project);
        assert_eq!(history_files(&catalog.common_dir), before_common);
    }
    // Existing parent queue with no Task queue is also a read-only empty result.
    fs::create_dir(data.path().join("draft-requests")).unwrap();
    let before = history_files(data.path());
    assert!(history(&registry, &fresh, &token, "task-1", data.path())
        .unwrap()
        .is_empty());
    assert_eq!(history_files(data.path()), before);
    assert!(fresh.0.lock().unwrap().views.is_empty());
    assert!(fresh.0.lock().unwrap().bound.is_empty());
}

#[test]
fn history_rejects_tampered_originals_and_never_leaks_other_project_or_task() {
    let (_root, registry, token, _) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = TaskRecheckRequests::default();
    let prepared = prepare_fixture(&registry, &requests, &token, data.path());
    let file = path(data.path(), &prepared.request_id).unwrap();
    let stored = read(data.path(), &prepared.request_id).unwrap();
    let original = serde_json::to_value(&stored).unwrap();
    for pointer in [
        "/metadata/body_hash",
        "/request/actor",
        "/subject/task_text",
        "/subject/binding/task_content_hash",
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = match pointer {
            "/metadata/body_hash" | "/subject/binding/task_content_hash" => json!("f".repeat(64)),
            "/request/actor" => json!("human:another"),
            _ => json!("not the original Task"),
        };
        fs::write(&file, serde_json::to_vec(&changed).unwrap()).unwrap();
        let before = history_files(data.path());
        assert!(
            history(&registry, &requests, &token, "task-1", data.path()).is_err(),
            "{pointer}"
        );
        assert_eq!(history_files(data.path()), before);
    }
    let mut process_actor = original.clone();
    process_actor["request"]["actor"] = json!("process:towermd-cli");
    fs::write(&file, serde_json::to_vec(&process_actor).unwrap()).unwrap();
    let error = history(&registry, &requests, &token, "task-1", data.path())
        .err()
        .expect("process actor must be rejected");
    assert!(error
        .message
        .contains("Task recheck requires an App human actor"));
    let mut other = stored.clone();
    let other_request = RecheckRequest::prepare(
        "other-project",
        &stored.request.host,
        &stored.request.actor,
        &original_action(&stored),
    )
    .unwrap();
    other.request = other_request.body().clone();
    other.metadata = other_request.metadata().clone();
    fs::write(&file, serde_json::to_vec(&other).unwrap()).unwrap();
    let fresh = TaskRecheckRequests::default();
    assert!(history(&registry, &fresh, &token, "task-1", data.path())
        .unwrap()
        .is_empty());
    assert!(fresh.0.lock().unwrap().bound.is_empty());
    let mut other_directory = stored.clone();
    other_directory.directory = data.path().join("another-worktree");
    fs::write(&file, serde_json::to_vec(&other_directory).unwrap()).unwrap();
    assert!(history(&registry, &fresh, &token, "task-1", data.path()).is_err());
    assert!(fresh.0.lock().unwrap().bound.is_empty());
    fs::write(&file, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert!(
        history(&registry, &fresh, &token, "another-task", data.path())
            .unwrap()
            .is_empty()
    );
    assert!(history(&registry, &fresh, &token, "../task-1", data.path()).is_err());
    assert!(history(&registry, &fresh, "stale-token", "task-1", data.path()).is_err());
    assert!(fresh.0.lock().unwrap().bound.is_empty());
}
