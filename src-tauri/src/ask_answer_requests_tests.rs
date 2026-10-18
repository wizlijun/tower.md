//! Actual TCP adapter fixtures, not daemon business authorization or GUI evidence.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
};
use towermd_core::{
    hub::Hub,
    hub_protocol::{AcquireLease, AppendReceipt, HubProtocol, NewEvent},
};

type Trace = Arc<Mutex<Vec<String>>>;
fn server(
    instance: &str,
    host: &str,
    responses: Vec<(u16, Value)>,
) -> (daemon_client::Endpoint, Trace, thread::JoinHandle<()>) {
    server_with_gate(instance, host, responses, None)
}
fn server_with_gate(
    instance: &str,
    host: &str,
    responses: Vec<(u16, Value)>,
    gate: Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
) -> (daemon_client::Endpoint, Trace, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let health = json!({"address":"127.0.0.1","port":listener.local_addr().unwrap().port(),"pid":42,"instance":instance,"host":host});
    let endpoint = serde_json::from_value(health.clone()).unwrap();
    let trace = Arc::new(Mutex::new(vec![]));
    let captured = trace.clone();
    let worker = thread::spawn(move || {
        for (index, (status, value)) in responses.into_iter().enumerate() {
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
            if index == 1 {
                if let Some((entered, release)) = &gate {
                    entered.send(()).unwrap();
                    release
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                }
            }
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

fn view(project: &str) -> Value {
    let directory = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(directory.path()).unwrap();
    let granted = hub
        .lease_acquire(
            AcquireLease {
                host: "fixture-host".into(),
                holder: "run-1".into(),
                names: vec!["module:A".into()],
                ttl_ms: 900000,
            },
            10,
        )
        .unwrap();
    let fences = granted
        .leases
        .iter()
        .map(|lease| lease.fence())
        .collect::<Vec<_>>();
    let recipient = json!({"run":"run-1","task":"task-1","host":"fixture-host","worktree":"/actual/fixture","branch":"t/task-1","fences":fences});
    let hold = json!({"type":"Hold","id":"H-1","body":{"kind":"ask","fingerprint":towermd_core::human_intervention::fingerprint("A","How?"),"question":"How?","module":"A","options":["yes","no"],"recommended":0,"context":"original context","future":{"order":[2,1]}},"recipients":[recipient],"opened_ms":10,"opened_seq":0,"reply":null,"waits":[],"delivered":{},"attention":false,"future":{"keep":true}});
    let AppendReceipt::Event(opening) = hub
        .append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "fixture-host".into(),
                actor: "process:towermd-cli".into(),
                kind: "hold.opened".into(),
                layer: "L5".into(),
                obj: "H-1".into(),
                guards: fences,
                data: json!({"hold":hold}),
            },
            10,
        )
        .unwrap()
    else {
        panic!("real opening receipt")
    };
    assert!(!opening.stale);
    let binding = AskOpeningBinding::from_opening(&opening).unwrap();
    let mut current = hold.clone();
    current["opened_seq"] = json!(opening.seq);
    json!({"project_id":project,"opening":opening,"current_hold":current,"binding":binding,"current_validity":{"state":"current"},"recipients":[{"recipient":recipient,"actionable":true}],"actions":["answer"]})
}
fn response(stored: &QueuedAnswer, historical: bool) -> Value {
    let (hold, binding, text) = action(stored);
    json!({"receipt":{"request_id":stored.metadata.request_id,"body_hash":stored.metadata.body_hash,"result_ref":{"hold":hold,"opening_eid":binding.eid,"reply":{"id":request_event_eid(&stored.metadata.request_id,"hold.answered").unwrap(),"text":text,"actor":stored.request.actor,"relayed_from":null,"self_reported":null,"decision":null,"decision_revision":null,"at_ms":11}}},"current_validity":if historical {json!({"state":"historical_only","reason":"hub rebuilt"})}else{json!({"state":"current"})}})
}
fn load_fixture(
    registry: &ProjectRegistry,
    requests: &AskAnswerRequests,
    token: &str,
    data: &Path,
    value: Value,
) -> Value {
    let (endpoint, trace, worker) = server(
        "one",
        "fixture-host",
        vec![(200, Value::Null), (200, value), (200, Value::Null)],
    );
    let loaded = load_with(registry, requests, token, "H-1", data, || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    assert!(trace.lock().unwrap()[1].starts_with("GET /app/interventions/asks/H-1?directory="));
    loaded
}
fn prepare_fixture(
    registry: &ProjectRegistry,
    requests: &AskAnswerRequests,
    token: &str,
    data: &Path,
    ticket: &str,
    value: Value,
) -> PreparedAskAnswer {
    let (endpoint, trace, worker) = server(
        "one",
        "fixture-host",
        vec![(200, Value::Null), (200, value), (200, Value::Null)],
    );
    let prepared = prepare_with(
        registry,
        requests,
        token,
        ticket,
        "actual human text".into(),
        data,
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
fn ask_answer_fixed_http_queue_reopens_and_retries_original_uuid_body_as_historical_receipt() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = AskAnswerRequests::default();
    let original = view(catalog.config.project_id());
    let loaded = load_fixture(&registry, &requests, &token, data.path(), original.clone());
    let prepared = prepare_fixture(
        &registry,
        &requests,
        &token,
        data.path(),
        loaded["answer_ticket"].as_str().unwrap(),
        original,
    );
    let file = path(data.path(), &prepared.request_id).unwrap();
    let bytes = fs::read(&file).unwrap();
    let stored = read(data.path(), &prepared.request_id).unwrap();
    assert_eq!(stored.request.actor, draft_requests::identity().unwrap());
    assert!(stored.request.run.is_none());
    assert!(write_new(&file, &stored).is_err());
    let mut posted = Vec::new();
    for accepted in [false, true] {
        let requests = AskAnswerRequests::default();
        let originals = pending(&registry, &requests, &token, "H-1", data.path()).unwrap();
        assert_eq!(originals.len(), 1);
        assert_eq!(originals[0].request_id, prepared.request_id);
        let result = if accepted {
            (200, response(&stored, true))
        } else {
            (
                503,
                json!({"code":"request_uncertain","error":"actual receipt not confirmed","request_id":prepared.request_id}),
            )
        };
        let (endpoint, trace, worker) = server(
            "restarted",
            "fixture-host",
            vec![(200, Value::Null), result, (200, Value::Null)],
        );
        let outcome = submit_with(
            &registry,
            &requests,
            &token,
            &prepared.request_id,
            data.path(),
            || daemon_client::DaemonClient::connect(endpoint),
        )
        .unwrap();
        worker.join().unwrap();
        assert_eq!(
            outcome.state,
            if accepted { "confirmed" } else { "uncertain" }
        );
        if accepted {
            assert_eq!(
                outcome.response.as_ref().unwrap()["current_validity"]["state"],
                "historical_only"
            );
        }
        assert_eq!(fs::read(&file).unwrap(), bytes);
        assert_eq!(file.with_extension("resolved").exists(), accepted);
        let trace = trace.lock().unwrap();
        assert!(trace[1].starts_with("POST /app/interventions/action "));
        assert!(trace[1].contains("x-towermd-client: app"));
        assert!(trace[1].contains("x-towermd-instance: restarted"));
        let body: Value = serde_json::from_str(trace[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"directory":catalog.root,"request_id":prepared.request_id,"human_id":stored.request.actor.strip_prefix("human:").unwrap(),"action":stored.request.action})
        );
        assert!(
            body.get("actor").is_none()
                && body.get("run").is_none()
                && body.get("fences").is_none()
        );
        posted.push(body);
    }
    assert_eq!(
        posted[0], posted[1],
        "uncertainty never generates another UUID/body"
    );
}
#[test]
fn ask_answer_expired_current_can_prepare_but_history_and_answered_cannot() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = AskAnswerRequests::default();
    let mut expired = view(catalog.config.project_id());
    expired["recipients"][0]["actionable"] = json!(false);
    let loaded = load_fixture(&registry, &requests, &token, data.path(), expired.clone());
    prepare_fixture(
        &registry,
        &requests,
        &token,
        data.path(),
        loaded["answer_ticket"].as_str().unwrap(),
        expired.clone(),
    );
    for history in [false, true] {
        let requests = AskAnswerRequests::default();
        let mut blocked = expired.clone();
        blocked["actions"] = json!([]);
        if history {
            blocked["current_validity"] = json!({"state":"historical_only","reason":"rebuild"});
            blocked["opening"]["stale"] = json!(true);
            let event = serde_json::from_value(blocked["opening"].clone()).unwrap();
            blocked["binding"] = json!(AskOpeningBinding::from_opening(&event).unwrap());
        } else {
            blocked["current_hold"]["reply"] = json!({"id":"answer-old","text":"already answered","actor":"human:owner","at_ms":11});
        }
        let loaded = load_fixture(&registry, &requests, &token, data.path(), blocked);
        assert!(prepare_with(
            &registry,
            &requests,
            &token,
            loaded["answer_ticket"].as_str().unwrap(),
            "new answer".into(),
            data.path(),
            || panic!("blocked prepare must not contact daemon")
        )
        .is_err());
    }
}
#[test]
fn ask_answer_rejects_binding_project_reply_mismatch_and_tampered_saved_identity() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let original = view(catalog.config.project_id());
    for which in 0..4 {
        let mut bad = original.clone();
        match which {
            0 => bad["project_id"] = json!("other-project"),
            1 => bad["binding"]["sha256"] = json!("f".repeat(64)),
            2 => bad["current_hold"]["body"]["question"] = json!("different"),
            3 => bad["opening"]["stale"] = json!(true),
            _ => unreachable!(),
        }
        assert!(checked_view(&catalog, "H-1", bad).is_err());
    }
    let data = tempfile::tempdir().unwrap();
    let requests = AskAnswerRequests::default();
    let loaded = load_fixture(&registry, &requests, &token, data.path(), original.clone());
    let prepared = prepare_fixture(
        &registry,
        &requests,
        &token,
        data.path(),
        loaded["answer_ticket"].as_str().unwrap(),
        original,
    );
    let mut stored = read(data.path(), &prepared.request_id).unwrap();
    let mut wrong = response(&stored, false);
    wrong["receipt"]["result_ref"]["reply"]["actor"] = json!("human:other");
    assert!(!confirmed(&stored, &wrong));
    stored.metadata.body_hash = "f".repeat(64);
    fs::write(
        path(data.path(), &prepared.request_id).unwrap(),
        serde_json::to_vec(&stored).unwrap(),
    )
    .unwrap();
    assert!(pending(&registry, &requests, &token, "H-1", data.path()).is_err());
}
#[test]
fn ask_answer_instance_replaced_after_post_retains_original_unresolved_request() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = AskAnswerRequests::default();
    let original = view(catalog.config.project_id());
    let loaded = load_fixture(&registry, &requests, &token, data.path(), original.clone());
    let prepared = prepare_fixture(
        &registry,
        &requests,
        &token,
        data.path(),
        loaded["answer_ticket"].as_str().unwrap(),
        original,
    );
    let stored = read(data.path(), &prepared.request_id).unwrap();
    let file = path(data.path(), &prepared.request_id).unwrap();
    let bytes = fs::read(&file).unwrap();
    let changed = json!({"address":"127.0.0.1","port":1,"pid":42,"instance":"different","host":"fixture-host"});
    let (endpoint, _, worker) = server(
        "one",
        "fixture-host",
        vec![
            (200, Value::Null),
            (200, response(&stored, false)),
            (200, changed),
        ],
    );
    let outcome = submit_with(
        &registry,
        &requests,
        &token,
        &prepared.request_id,
        data.path(),
        || daemon_client::DaemonClient::connect(endpoint),
    )
    .unwrap();
    worker.join().unwrap();
    assert_eq!(outcome.state, "uncertain");
    assert_eq!(outcome.error.unwrap().code, "daemon_changed");
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert!(!file.with_extension("resolved").exists());
}

#[test]
fn ask_answer_second_preparation_with_another_ticket_preserves_original_queue_uuid() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = AskAnswerRequests::default();
    let original = view(catalog.config.project_id());
    let first = load_fixture(&registry, &requests, &token, data.path(), original.clone());
    let second = load_fixture(&registry, &requests, &token, data.path(), original.clone());
    assert_ne!(first["answer_ticket"], second["answer_ticket"]);
    let prepared = prepare_fixture(
        &registry,
        &requests,
        &token,
        data.path(),
        first["answer_ticket"].as_str().unwrap(),
        original,
    );
    let file = path(data.path(), &prepared.request_id).unwrap();
    let bytes = fs::read(&file).unwrap();
    for text in ["actual human text", "a different second answer"] {
        let error = prepare_with(
            &registry,
            &requests,
            &token,
            second["answer_ticket"].as_str().unwrap(),
            text.into(),
            data.path(),
            || panic!("an unresolved original must reject another prepare before HTTP"),
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "request_pending");
    }
    let queued = pending(&registry, &requests, &token, "H-1", data.path()).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].request_id, prepared.request_id);
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert!(
        requests.0.lock().unwrap().preparing.is_empty(),
        "failed prepares release only their slot"
    );
}

#[test]
fn ask_answer_concurrent_same_ticket_preparation_is_single_flight_during_actual_http() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let requests = AskAnswerRequests::default();
    let original = view(catalog.config.project_id());
    let loaded = load_fixture(&registry, &requests, &token, data.path(), original.clone());
    let ticket = loaded["answer_ticket"].as_str().unwrap().to_owned();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (endpoint, trace, http) = server_with_gate(
        "one",
        "fixture-host",
        vec![(200, Value::Null), (200, original), (200, Value::Null)],
        Some((entered_tx, release_rx)),
    );
    let other_registry = registry.clone();
    let other_requests = requests.clone();
    let other_token = token.clone();
    let other_ticket = ticket.clone();
    let directory = data.path().to_owned();
    let preparing = thread::spawn(move || {
        prepare_with(
            &other_registry,
            &other_requests,
            &other_token,
            &other_ticket,
            "actual human text".into(),
            &directory,
            || daemon_client::DaemonClient::connect(endpoint),
        )
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let error = prepare_with(
        &registry,
        &requests,
        &token,
        &ticket,
        "actual human text".into(),
        data.path(),
        || panic!("single flight must reject a second prepare without another HTTP call"),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "request_busy");
    assert_eq!(
        trace.lock().unwrap().len(),
        2,
        "the first real GET is paused before its response"
    );
    release_tx.send(()).unwrap();
    let prepared = preparing.join().unwrap().unwrap();
    http.join().unwrap();
    let queued = pending(&registry, &requests, &token, "H-1", data.path()).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].request_id, prepared.request_id);
    assert!(requests.0.lock().unwrap().preparing.is_empty());
    assert!(
        trace
            .lock()
            .unwrap()
            .iter()
            .all(|request| !request.starts_with("POST ")),
        "preparation must never submit an answer"
    );
}
