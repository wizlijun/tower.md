//! TCP transport fixtures exercise the native adapter, not an actual daemon or GUI.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
};
use towermd_core::recovery_contract::recorded::{RequestReceipt, TaskReviewResult};
use towermd_core::recovery_contract::task_review::ReviewFact;

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
fn view(project_id: &str) -> Value {
    let binding = json!({"run":"run-1","task":"task-1","baseline":"towermd/B0","fences":[{"name":"module:A","holder":"run-1","epoch":1}],"subject":{"kind":"plan","sha256":"a".repeat(64)}});
    let hold = json!({"type":"Hold","id":"hold-1","body":{"kind":"review","binding":binding,"verdict":null,"plan_content":"原计划\r\n末行","source_commit":null,"source_path":"plan.md","comment":null},"opened_ms":10,"opened_seq":0,"reply":null,"future":{"keep":true}});
    json!({"project_id":project_id,"opening":{"seq":7,"eid":"opening-1","host":"fixture-host","t":"2026-10-18T00:00:00Z","actor":"process:towermd-cli","type":"hold.opened","layer":"L5","obj":"hold-1","data":{"hold":hold},"epoch":1,"stale":false,"cost_usd":null,"guards":binding["fences"]},"current_hold":hold,"binding":binding,"current_validity":{"state":"current"}})
}
fn response(stored: &QueuedReview) -> Value {
    let (hold, binding, verdict, comment) = action(stored);
    json!(TaskReviewOutcome {
        receipt: RequestReceipt {
            request_id: stored.metadata.request_id.clone(),
            body_hash: stored.metadata.body_hash.clone(),
            result_ref: TaskReviewResult {
                hold: hold.into(),
                fact: ReviewFact {
                    binding: binding.clone(),
                    verdict: verdict.clone(),
                    actor: stored.request.actor.clone(),
                    answer_eid: request_event_eid(&stored.metadata.request_id, "hold.reviewed")
                        .unwrap()
                },
                verdict: verdict.clone(),
                comment: comment.into()
            }
        },
        current_validity: ReviewValidity::Current
    })
}
fn load_fixture(
    registry: &ProjectRegistry,
    requests: &TaskReviewRequests,
    token: &str,
    app_data: &Path,
    value: Value,
) -> Value {
    let (endpoint, _, worker) = server(
        "instance-1",
        "fixture-host",
        vec![(200, Value::Null), (200, value), (200, Value::Null)],
    );
    let loaded = load_with(registry, requests, token, "hold-1", app_data, || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    loaded
}
fn prepare_fixture(
    registry: &ProjectRegistry,
    requests: &TaskReviewRequests,
    token: &str,
    app_data: &Path,
    ticket: &str,
    value: Value,
) -> PreparedTaskReview {
    let (endpoint, trace, worker) = server(
        "instance-1",
        "fixture-host",
        vec![(200, Value::Null), (200, value), (200, Value::Null)],
    );
    let prepared = prepare_with(
        registry,
        requests,
        token,
        ticket,
        ReviewVerdict::Approved,
        "已读原计划\r\n确认".into(),
        app_data,
        || daemon_client::DaemonClient::connect(endpoint),
    )
    .unwrap();
    worker.join().unwrap();
    assert!(trace
        .lock()
        .unwrap()
        .iter()
        .all(|request| !request.starts_with("POST ")));
    prepared
}
#[test]
fn native_task_review_load_prepare_submit_uses_fixed_routes_and_fsynced_original() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let app_data = tempfile::tempdir().unwrap();
    let requests = TaskReviewRequests::default();
    let original_view = view(catalog.config.project_id());
    let (endpoint, trace, worker) = server(
        "instance-1",
        "fixture-host",
        vec![
            (200, Value::Null),
            (200, original_view.clone()),
            (200, Value::Null),
        ],
    );
    let loaded = load_with(
        &registry,
        &requests,
        &token,
        "hold-1",
        app_data.path(),
        || daemon_client::DaemonClient::connect(endpoint),
    )
    .unwrap();
    worker.join().unwrap();
    for key in ["opening", "current_hold", "binding", "current_validity"] {
        assert_eq!(loaded[key], original_view[key]);
    }
    assert!(
        trace.lock().unwrap()[1].starts_with("GET /app/interventions/reviews/hold-1?directory=")
    );
    let prepared = prepare_fixture(
        &registry,
        &requests,
        &token,
        app_data.path(),
        loaded["review_ticket"].as_str().unwrap(),
        original_view,
    );
    let id = &prepared.request_id;
    let file = path(app_data.path(), id).unwrap();
    let bytes = fs::read(&file).unwrap();
    let stored = read(app_data.path(), id).unwrap();
    assert_eq!(stored.request.actor, draft_requests::identity().unwrap());
    assert!(stored.request.run.is_none());
    assert_eq!(summary(&stored).comment, "已读原计划\r\n确认");
    assert!(write_new(&file, &stored).is_err());
    let (endpoint, trace, worker) = server(
        "restarted-instance",
        "fixture-host",
        vec![
            (200, Value::Null),
            (200, response(&stored)),
            (200, Value::Null),
        ],
    );
    let outcome = submit_with(&registry, &requests, &token, id, app_data.path(), || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    assert_eq!(outcome.state, "confirmed");
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert!(file.with_extension("resolved").exists());
    let trace = trace.lock().unwrap();
    assert!(trace[1].starts_with("POST /app/interventions/action "));
    assert!(trace[1].contains("x-towermd-client: app"));
    assert!(trace[1].contains("x-towermd-instance: restarted-instance"));
    let body: Value = serde_json::from_str(trace[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"directory":catalog.root,"request_id":id,"human_id":stored.request.actor.strip_prefix("human:").unwrap(),"action":stored.request.action})
    );
    assert!(body.get("actor").is_none() && body.get("host").is_none() && body.get("run").is_none());
}
#[test]
fn historical_changed_binding_and_changed_instance_cannot_prepare_an_answer() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let app_data = tempfile::tempdir().unwrap();
    let requests = TaskReviewRequests::default();
    let mut historical = view(catalog.config.project_id());
    historical["current_validity"] = json!({"state":"historical_only","reason":"run expired"});
    let loaded = load_fixture(&registry, &requests, &token, app_data.path(), historical);
    let error = prepare_with(
        &registry,
        &requests,
        &token,
        loaded["review_ticket"].as_str().unwrap(),
        ReviewVerdict::Approved,
        String::new(),
        app_data.path(),
        || panic!("historical review must not send"),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "request_conflict");
    let original = view(catalog.config.project_id());
    let loaded = load_fixture(
        &registry,
        &requests,
        &token,
        app_data.path(),
        original.clone(),
    );
    let ticket = loaded["review_ticket"].as_str().unwrap();
    let (endpoint, trace, worker) = server("instance-2", "fixture-host", vec![(200, Value::Null)]);
    let error = prepare_with(
        &registry,
        &requests,
        &token,
        ticket,
        ReviewVerdict::Approved,
        String::new(),
        app_data.path(),
        || daemon_client::DaemonClient::connect(endpoint),
    )
    .err()
    .unwrap();
    worker.join().unwrap();
    assert_eq!(error.code, "request_context_changed");
    assert_eq!(trace.lock().unwrap().len(), 1);
    let mut changed = original;
    changed["binding"]["subject"]["sha256"] = json!("b".repeat(64));
    changed["opening"]["data"]["hold"]["body"]["binding"] = changed["binding"].clone();
    changed["current_hold"]["body"]["binding"] = changed["binding"].clone();
    let (endpoint, _, worker) = server(
        "instance-1",
        "fixture-host",
        vec![(200, Value::Null), (200, changed), (200, Value::Null)],
    );
    let error = prepare_with(
        &registry,
        &requests,
        &token,
        ticket,
        ReviewVerdict::Approved,
        String::new(),
        app_data.path(),
        || daemon_client::DaemonClient::connect(endpoint),
    )
    .err()
    .unwrap();
    worker.join().unwrap();
    assert_eq!(error.code, "request_conflict");
    assert!(
        pending(&registry, &requests, &token, "hold-1", app_data.path())
            .unwrap()
            .is_empty()
    );
}
#[test]
fn uncertain_delivery_survives_restart_and_retries_identical_request() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let app_data = tempfile::tempdir().unwrap();
    let requests = TaskReviewRequests::default();
    let value = view(catalog.config.project_id());
    let loaded = load_fixture(&registry, &requests, &token, app_data.path(), value.clone());
    let prepared = prepare_fixture(
        &registry,
        &requests,
        &token,
        app_data.path(),
        loaded["review_ticket"].as_str().unwrap(),
        value,
    );
    let id = &prepared.request_id;
    let stored = read(app_data.path(), id).unwrap();
    let file = path(app_data.path(), id).unwrap();
    let bytes = fs::read(&file).unwrap();
    for bad_response in [
        (503, json!({"error":"confirmed fsync failed"})),
        (200, json!({"receipt":"incomplete"})),
    ] {
        let (endpoint, _, worker) = server(
            "instance-1",
            "fixture-host",
            vec![(200, Value::Null), bad_response, (200, Value::Null)],
        );
        let outcome = submit_with(&registry, &requests, &token, id, app_data.path(), || {
            daemon_client::DaemonClient::connect(endpoint)
        })
        .unwrap();
        worker.join().unwrap();
        assert_eq!(outcome.state, "uncertain");
        assert!(!file.with_extension("resolved").exists());
        assert_eq!(fs::read(&file).unwrap(), bytes);
    }
    let restarted = TaskReviewRequests::default();
    assert_eq!(
        pending(&registry, &restarted, &token, "hold-1", app_data.path()).unwrap()[0].request_id,
        *id
    );
    let mut historical = response(&stored);
    historical["current_validity"] =
        json!({"state":"historical_only","reason":"original result; lease expired"});
    let (endpoint, trace, worker) = server(
        "instance-2",
        "fixture-host",
        vec![(200, Value::Null), (200, historical), (200, Value::Null)],
    );
    let outcome = submit_with(&registry, &restarted, &token, id, app_data.path(), || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    assert_eq!(outcome.state, "confirmed");
    assert_eq!(fs::read(file).unwrap(), bytes);
    let trace = trace.lock().unwrap();
    let body: Value = serde_json::from_str(trace[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["request_id"], *id);
    assert_eq!(body["action"], json!(stored.request.action));
    // Historical receipt confirms durability only; it does not create current approval.
    assert_eq!(
        outcome.response.unwrap()["current_validity"]["state"],
        "historical_only"
    );
}
#[test]
fn receipt_requires_every_original_binding_and_answer_field() {
    let binding: ReviewBinding =
        serde_json::from_value(view("project")["binding"].clone()).unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let prepared = TaskReviewRequest::prepare(
        "project",
        "fixture-host",
        "human:fixture",
        &id,
        &TaskReviewAction::Review {
            hold: "hold-1".into(),
            binding,
            verdict: ReviewVerdict::Approved,
            comment: "original".into(),
        },
    )
    .unwrap();
    let stored = QueuedReview {
        schema: 1,
        directory: "/fixture".into(),
        common_dir: "/fixture/.git".into(),
        request: prepared.body().clone(),
        metadata: prepared.metadata().clone(),
    };
    let correct = response(&stored);
    assert!(confirmed(&stored, &correct));
    for pointer in [
        "/receipt/request_id",
        "/receipt/body_hash",
        "/receipt/result_ref/hold",
        "/receipt/result_ref/comment",
        "/receipt/result_ref/fact/actor",
        "/receipt/result_ref/fact/answer_eid",
        "/receipt/result_ref/fact/binding/run",
        "/receipt/result_ref/fact/binding/task",
        "/receipt/result_ref/fact/binding/baseline",
        "/receipt/result_ref/fact/binding/subject/sha256",
        "/receipt/result_ref/fact/binding/fences/0/holder",
    ] {
        let mut changed = correct.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("replacement");
        assert!(!confirmed(&stored, &changed), "{pointer}");
    }
    for pointer in [
        "/receipt/result_ref/verdict",
        "/receipt/result_ref/fact/verdict",
    ] {
        let mut changed = correct.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("rejected");
        assert!(!confirmed(&stored, &changed));
    }
    let mut changed = correct;
    changed["receipt"]["result_ref"]["fact"]["binding"]["fences"][0]["epoch"] = json!(2);
    assert!(!confirmed(&stored, &changed));
}
#[test]
fn host_actor_project_queue_tamper_and_late_instance_changes_preserve_original() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let app_data = tempfile::tempdir().unwrap();
    let requests = TaskReviewRequests::default();
    let value = view(catalog.config.project_id());
    let loaded = load_fixture(&registry, &requests, &token, app_data.path(), value.clone());
    let prepared = prepare_fixture(
        &registry,
        &requests,
        &token,
        app_data.path(),
        loaded["review_ticket"].as_str().unwrap(),
        value,
    );
    let id = &prepared.request_id;
    let stored = read(app_data.path(), id).unwrap();
    let file = path(app_data.path(), id).unwrap();
    let bytes = fs::read(&file).unwrap();
    let (endpoint, trace, worker) =
        server("instance-1", "different-host", vec![(200, Value::Null)]);
    let error = submit_with(&registry, &requests, &token, id, app_data.path(), || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .err()
    .unwrap();
    worker.join().unwrap();
    assert_eq!(error.code, "request_context_changed");
    assert_eq!(trace.lock().unwrap().len(), 1);
    let mut altered = serde_json::to_value(&stored).unwrap();
    altered["request"]["action"]["comment"] = json!("tampered");
    fs::write(&file, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(read(app_data.path(), id).is_err());
    fs::write(&file, &bytes).unwrap();
    let mut changed_actor = stored.clone();
    let fake = TaskReviewRequest::prepare(
        &stored.request.project_id,
        &stored.request.host,
        "human:another-owner",
        id,
        &stored.request.action,
    )
    .unwrap();
    changed_actor.request = fake.body().clone();
    changed_actor.metadata = fake.metadata().clone();
    fs::write(&file, serde_json::to_vec(&changed_actor).unwrap()).unwrap();
    let (endpoint, _, worker) = server("instance-1", "fixture-host", vec![(200, Value::Null)]);
    let error = submit_with(&registry, &requests, &token, id, app_data.path(), || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .err()
    .unwrap();
    worker.join().unwrap();
    assert_eq!(error.code, "request_context_changed");
    fs::write(&file, &bytes).unwrap();
    let changed_health = json!({"address":"127.0.0.1","port":1,"pid":42,"instance":"replacement","host":"fixture-host"});
    let (endpoint, _, worker) = server(
        "instance-1",
        "fixture-host",
        vec![
            (200, Value::Null),
            (200, response(&stored)),
            (200, changed_health),
        ],
    );
    let outcome = submit_with(&registry, &requests, &token, id, app_data.path(), || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    assert_eq!(outcome.state, "uncertain");
    assert!(!file.with_extension("resolved").exists());
    assert_eq!(fs::read(&file).unwrap(), bytes);
    let new_token = registry
        .select(Some(catalog.root.clone()))
        .unwrap()
        .unwrap()
        .token;
    assert_ne!(new_token, token);
    assert!(submit_with(
        &registry,
        &requests,
        &new_token,
        id,
        app_data.path(),
        || panic!("changed token must not send")
    )
    .is_err());
    let (_other, other_registry, other_token, _catalog) = crate::draft_requests::tests::project();
    assert!(pending(
        &other_registry,
        &TaskReviewRequests::default(),
        &other_token,
        "hold-1",
        app_data.path()
    )
    .unwrap()
    .is_empty());
    let error = submit_with(
        &other_registry,
        &requests,
        &other_token,
        id,
        app_data.path(),
        || panic!("other project must not send"),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "request_context_changed");
}

#[test]
fn ticket_actor_selection_and_original_project_are_checked_before_preparing() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let app_data = tempfile::tempdir().unwrap();
    let requests = TaskReviewRequests::default();
    let value = view(catalog.config.project_id());
    let loaded = load_fixture(&registry, &requests, &token, app_data.path(), value.clone());
    let ticket = loaded["review_ticket"].as_str().unwrap();
    requests
        .0
        .lock()
        .unwrap()
        .views
        .get_mut(ticket)
        .unwrap()
        .actor = "human:different-owner".into();
    let error = prepare_with(
        &registry,
        &requests,
        &token,
        ticket,
        ReviewVerdict::Approved,
        String::new(),
        app_data.path(),
        || panic!("changed actor must not send"),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "request_context_changed");
    let mut wrong_project = value.clone();
    wrong_project["project_id"] = json!("other-project");
    assert!(checked_view(&catalog, "hold-1", wrong_project).is_err());
    for pointer in [
        "/opening/obj",
        "/opening/type",
        "/opening/layer",
        "/opening/data/hold/id",
        "/current_hold/id",
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("replacement");
        assert!(
            checked_view(&catalog, "hold-1", changed).is_err(),
            "{pointer}"
        );
    }
    let loaded = load_fixture(&registry, &requests, &token, app_data.path(), value);
    let current_token = registry.select(Some(catalog.root)).unwrap().unwrap().token;
    let error = prepare_with(
        &registry,
        &requests,
        &current_token,
        loaded["review_ticket"].as_str().unwrap(),
        ReviewVerdict::Approved,
        String::new(),
        app_data.path(),
        || panic!("changed project selection must not send"),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "request_context_changed");
}

#[test]
fn stale_archive_review_loads_only_as_history_and_cannot_prepare() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let app_data = tempfile::tempdir().unwrap();
    let requests = TaskReviewRequests::default();
    for terminal in ["verdict", "reply"] {
        let mut answered = view(catalog.config.project_id());
        if terminal == "verdict" {
            answered["current_hold"]["body"]["verdict"] = json!("approved");
        } else {
            answered["current_hold"]["reply"] = json!({"id":"answer-1","text":"already answered",
                "actor":"human:owner","at_ms":11});
        }
        let loaded = load_fixture(&registry, &requests, &token, app_data.path(), answered);
        let error = prepare_with(
            &registry,
            &requests,
            &token,
            loaded["review_ticket"].as_str().unwrap(),
            ReviewVerdict::Approved,
            String::new(),
            app_data.path(),
            || panic!("terminal Review must reject before daemon connection"),
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "request_conflict", "{terminal}");
        assert!(!draft_requests::queue(app_data.path())
            .unwrap()
            .join("task-reviews")
            .exists());
    }
    let mut historical = view(catalog.config.project_id());
    historical["opening"]["stale"] = json!(true);
    assert!(checked_view(&catalog, "hold-1", historical.clone()).is_err());
    historical["current_validity"] =
        json!({"state":"historical_only","reason":"actual rebuilt archive"});
    for raw_seq in [0, 999] {
        historical["opening"]["data"]["hold"]["opened_seq"] = json!(raw_seq);
        historical["current_hold"]["opened_seq"] = json!(raw_seq);
        let decoded = checked_view(&catalog, "hold-1", historical.clone()).unwrap();
        assert_eq!(decoded.current_hold["opened_seq"], raw_seq);
        assert!(decoded.opening.stale);
    }
    let loaded = load_fixture(
        &registry,
        &requests,
        &token,
        app_data.path(),
        historical.clone(),
    );
    assert_eq!(loaded["opening"], historical["opening"]);
    assert_eq!(loaded["current_hold"], historical["current_hold"]);
    let error = prepare_with(
        &registry,
        &requests,
        &token,
        loaded["review_ticket"].as_str().unwrap(),
        ReviewVerdict::Approved,
        String::new(),
        app_data.path(),
        || panic!("historical Review must reject before daemon connection"),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "request_conflict");
    assert!(
        pending(&registry, &requests, &token, "hold-1", app_data.path())
            .unwrap()
            .is_empty()
    );
    assert!(!draft_requests::queue(app_data.path())
        .unwrap()
        .join("task-reviews")
        .exists());
    historical["opening"]["guards"][0]["epoch"] = json!(2);
    assert!(checked_view(&catalog, "hold-1", historical).is_err());
}
