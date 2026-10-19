//! Native protocol fixtures only. These do not claim daemon business or GUI acceptance.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
};
fn view(project: &str) -> Value {
    json!({"project_id":project,"paused":false,"pause_revision":null,"active_runs":[],"resume_request":null,"messages":[]})
}
fn queued(catalog: &Catalog, action: ControlAction) -> QueuedControl {
    let actor = if matches!(&action, ControlAction::ResumeAll) {
        "process:towermd-cli".into()
    } else {
        draft_requests::identity().unwrap()
    };
    let original = ControlRequest::prepare(
        catalog.config.project_id(),
        "fixture-host",
        &actor,
        &uuid::Uuid::now_v7().to_string(),
        action,
    )
    .unwrap();
    QueuedControl {
        schema: 1,
        directory: catalog.root.clone(),
        common_dir: catalog.common_dir.clone(),
        request: original.body().clone(),
        metadata: original.metadata().clone(),
    }
}
fn response(stored: &QueuedControl) -> Value {
    let pause = match &stored.request.action {
        ControlAction::PauseAll => Some(
            ControlRequest::from_recorded(stored.request.clone(), stored.metadata.clone())
                .unwrap()
                .pause_revision()
                .unwrap(),
        ),
        ControlAction::ResumeAll => Some("a".repeat(64)),
        ControlAction::ConfirmResume { pause_revision, .. } => Some(pause_revision.clone()),
        _ => None,
    };
    let runs = if let ControlAction::ConfirmResume { runs, .. } = &stored.request.action {
        runs.clone()
    } else {
        vec![]
    };
    json!({"receipt":{"request_id":stored.metadata.request_id,"body_hash":stored.metadata.body_hash,"result_ref":{"hold":format!("H-{}",stored.metadata.request_id),"opening_eid":request_event_eid(&stored.metadata.request_id,"hold.opened").unwrap(),"pause_revision":pause,"runs":runs}},"historical_only":false})
}
fn server(
    instance: &str,
    responses: Vec<(u16, Value)>,
) -> (
    daemon_client::Endpoint,
    Arc<Mutex<Vec<String>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let health = json!({"address":"127.0.0.1","port":listener.local_addr().unwrap().port(),"pid":42,"instance":instance,"host":"fixture-host"});
    let endpoint = serde_json::from_value(health.clone()).unwrap();
    let trace = Arc::new(Mutex::new(vec![]));
    let captured = trace.clone();
    let worker = thread::spawn(move || {
        for (status, value) in responses {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(!line.is_empty());
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            assert!(length <= 4 * 1024 * 1024);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            request.push_str(&String::from_utf8(body).unwrap());
            captured.lock().unwrap().push(request);
            let body = if value.is_null() {
                health.clone()
            } else {
                value
            }
            .to_string();
            write!(socket,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (endpoint, trace, worker)
}
#[test]
fn original_queue_is_fsynced_reopened_and_rejects_changed_body() {
    let (_root, _registry, _token, catalog) = crate::draft_requests::tests::project();
    let data = tempfile::tempdir().unwrap();
    let stored = queued(
        &catalog,
        ControlAction::Interject {
            scope: towermd_core::human_intervention::Scope::Project,
            text: "原文保持".into(),
        },
    );
    let id = &stored.metadata.request_id;
    let file = path(data.path(), id).unwrap();
    write_new(&file, &stored).unwrap();
    let reopened = read(data.path(), id).unwrap();
    assert_eq!(reopened.request, stored.request);
    assert_eq!(reopened.metadata, stored.metadata);
    assert!(write_new(&file, &stored).is_err());
    let mut altered = serde_json::to_value(&stored).unwrap();
    altered["request"]["action"]["text"] = json!("被替换原文");
    fs::write(file, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(read(data.path(), id).is_err());
}
#[test]
fn confirmed_requires_exact_uuid_hash_opening_and_resume_scope() {
    let (_root, _registry, _token, catalog) = crate::draft_requests::tests::project();
    let stored = queued(
        &catalog,
        ControlAction::ConfirmResume {
            request: "H-process".into(),
            pause_revision: "a".repeat(64),
            runs: vec!["R-1".into(), "R-2".into()],
        },
    );
    let expected = response(&stored);
    assert!(confirmed(&stored, &expected));
    for (key, value) in [
        ("hold", json!("other")),
        ("opening_eid", json!("other")),
        ("pause_revision", json!("b".repeat(64))),
        ("runs", json!(["R-1"])),
    ] {
        let mut changed = expected.clone();
        changed["receipt"]["result_ref"][key] = value;
        assert!(!confirmed(&stored, &changed), "{key}");
    }
    let mut changed = expected.clone();
    changed["receipt"]["body_hash"] = json!("0".repeat(64));
    assert!(!confirmed(&stored, &changed));
    let mut historical = expected;
    historical["historical_only"] = json!(true);
    assert!(confirmed(&stored, &historical));
}
#[test]
fn read_uses_fixed_app_route_and_cannot_supply_actor_or_directory_from_page() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let requests = InterventionRequests::default();
    let data = tempfile::tempdir().unwrap();
    let (endpoint, trace, worker) = server(
        "one",
        vec![
            (200, Value::Null),
            (200, view(catalog.config.project_id())),
            (200, Value::Null),
        ],
    );
    let loaded = load_with(&registry, &requests, &token, data.path(), || {
        daemon_client::DaemonClient::connect(endpoint)
    })
    .unwrap();
    worker.join().unwrap();
    assert_eq!(loaded["project_id"], catalog.config.project_id());
    assert_eq!(loaded["human_actor"], draft_requests::identity().unwrap());
    let trace = trace.lock().unwrap();
    assert!(trace[1].starts_with("GET /app/interventions?directory="));
    assert!(trace[1].contains("x-towermd-client: app"));
    assert!(trace[1].contains("x-towermd-instance: one"));
    assert!(trace.iter().all(|request| !request.starts_with("POST ")));
}
#[test]
fn changed_daemon_rejects_ticket_before_new_queue_or_post() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let requests = InterventionRequests::default();
    let data = tempfile::tempdir().unwrap();
    let ticket = uuid::Uuid::now_v7().to_string();
    requests.0.lock().unwrap().views.insert(
        ticket.clone(),
        Viewed {
            token: token.clone(),
            directory: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            instance: "old".into(),
            host: "fixture-host".into(),
            actor: draft_requests::identity().unwrap(),
            source: CatalogSource::from(&catalog).unwrap(),
            view: serde_json::from_value(view(catalog.config.project_id())).unwrap(),
        },
    );
    let (endpoint, trace, worker) = server("new", vec![(200, Value::Null)]);
    let error = prepare_with(
        &registry,
        &requests,
        &token,
        &ticket,
        AppControlAction::PauseAll {},
        data.path(),
        || daemon_client::DaemonClient::connect(endpoint),
    )
    .err()
    .unwrap();
    worker.join().unwrap();
    assert_eq!(error.code, "request_context_changed");
    assert!(pending(&registry, &requests, &token, data.path())
        .unwrap()
        .is_empty());
    assert_eq!(trace.lock().unwrap().len(), 1);
}
#[test]
fn process_resume_retry_executes_original_durable_uuid_and_never_confirms() {
    let (_root, registry, token, catalog) = crate::draft_requests::tests::project();
    let requests = InterventionRequests::default();
    let data = tempfile::tempdir().unwrap();
    let stored = queued(&catalog, ControlAction::ResumeAll);
    let id = stored.metadata.request_id.clone();
    write_new(&path(data.path(), &id).unwrap(), &stored).unwrap();
    let (endpoint, _trace, worker) = server("one", vec![(200, Value::Null), (200, Value::Null)]);
    let first = request_resume_with(
        &registry,
        &requests,
        &token,
        data.path(),
        || daemon_client::DaemonClient::connect(endpoint),
        |_, actual| {
            assert_eq!(actual, id);
            Err(AppError::new("cli_failed", "回包丢失"))
        },
    );
    worker.join().unwrap();
    assert!(first.is_err());
    assert!(!path(data.path(), &id)
        .unwrap()
        .with_extension("resolved")
        .exists());
    let (endpoint, _trace, worker) = server("two", vec![(200, Value::Null), (200, Value::Null)]);
    let output = response(&stored).to_string();
    request_resume_with(
        &registry,
        &requests,
        &token,
        data.path(),
        || daemon_client::DaemonClient::connect(endpoint),
        |_, actual| {
            assert_eq!(actual, id);
            Ok(crate::cli::CliOutput {
                stdout: output,
                stderr: String::new(),
                exit_code: Some(0),
            })
        },
    )
    .unwrap();
    worker.join().unwrap();
    assert!(path(data.path(), &id)
        .unwrap()
        .with_extension("resolved")
        .exists());
    assert!(matches!(
        read(data.path(), &id).unwrap().request.action,
        ControlAction::ResumeAll
    ));
    assert!(pending(&registry, &requests, &token, data.path())
        .unwrap()
        .is_empty());
}
#[test]
fn stdout_accepts_only_valid_delivery_prefix_and_one_complete_result() {
    let message=json!({"towermd_message":{"id":"H/prompt/R","hold":"H","run":"R","text":"message","kind":"prompt","actionable":false}}).to_string();
    assert_eq!(
        crate::cli::parse_result_stdout(&format!("{message}\n{{\"ok\":true}}\n")).unwrap(),
        json!({"ok":true})
    );
    for invalid in [
        "diagnostic\n{\"ok\":true}",
        "{}\n{}",
        "{\"towermd_message\":{}}\n{}",
        "{\"towermd_message\":{},\"extra\":true}\n{}",
        "{\"ok\":",
    ] {
        assert!(
            crate::cli::parse_result_stdout(invalid).is_err(),
            "{invalid}"
        );
    }
}
#[test]
fn finite_action_does_not_accept_process_resume_or_page_supplied_authority_fields() {
    assert!(serde_json::from_value::<AppControlAction>(json!({"action":"pause_all"})).is_ok());
    for key in [
        "actor",
        "human_id",
        "directory",
        "url",
        "request_id",
        "body_hash",
        "run",
    ] {
        let mut value = json!({"action":"pause_all"});
        value[key] = json!("page-supplied");
        assert!(
            serde_json::from_value::<AppControlAction>(value).is_err(),
            "{key}"
        );
    }
    for value in [
        json!({"action":"resume_all"}),
        json!({"action":"pause_all","actor":"human:other"}),
        json!({"action":"interject","scope":{"kind":"project"},"text":"x","directory":"/other"}),
        json!({"action":"confirm_resume","request":"H","pause_revision":"a".repeat(64),"runs":[],"url":"http://example.invalid"}),
    ] {
        assert!(serde_json::from_value::<AppControlAction>(value).is_err());
    }
}

#[test]
fn control_pause_receipt_uses_original_revision_and_resume_requires_lower_hex() {
    let (_root, _registry, _token, catalog) = crate::draft_requests::tests::project();
    let pause = queued(&catalog, ControlAction::PauseAll);
    let actual = response(&pause);
    assert!(confirmed(&pause, &actual));
    let mut changed = actual;
    changed["receipt"]["result_ref"]["pause_revision"] = json!("0".repeat(64));
    assert!(!confirmed(&pause, &changed));
    let resume = queued(&catalog, ControlAction::ResumeAll);
    let actual = response(&resume);
    assert!(confirmed(&resume, &actual));
    assert!(
        ControlRequest::from_recorded(resume.request.clone(), resume.metadata.clone())
            .unwrap()
            .pause_revision()
            .is_err()
    );
    for invalid in [
        Value::Null,
        json!("A".repeat(64)),
        json!("a".repeat(63)),
        json!("a".repeat(65)),
        json!("g".repeat(64)),
    ] {
        let mut changed = actual.clone();
        changed["receipt"]["result_ref"]["pause_revision"] = invalid;
        assert!(!confirmed(&resume, &changed));
    }
}
