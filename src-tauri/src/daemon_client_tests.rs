use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};
const ID: &str = "0192f0aa-0000-7000-8000-000000000001";
fn fixture_endpoint(port: u16) -> Endpoint {
    Endpoint {
        address: "127.0.0.1".into(),
        port,
        pid: 42,
        instance: "fixture-instance".into(),
        host: "fixture-host".into(),
    }
}
pub(crate) fn server(
    responses: Vec<(u16, Value)>,
) -> (Endpoint, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = fixture_endpoint(listener.local_addr().unwrap().port());
    let responses: Vec<_> = responses
        .into_iter()
        .map(|(status, value)| {
            (
                status,
                if value.is_null() {
                    health(endpoint.port)
                } else {
                    value
                },
            )
        })
        .collect();
    let trace = Arc::new(Mutex::new(vec![]));
    let captured = trace.clone();
    let handle = thread::spawn(move || {
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut header = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                header.push_str(&line);
            }
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            header.push_str(&String::from_utf8(request).unwrap());
            captured.lock().unwrap().push(header);
            let body = body.to_string();
            let location = if status == 302 {
                "Location: http://127.0.0.1:1/forbidden\r\n"
            } else {
                ""
            };
            write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nConnection: close\r\n{location}Content-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (endpoint, trace, handle)
}
fn health(port: u16) -> Value {
    serde_json::to_value(fixture_endpoint(port)).unwrap()
}
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
fn project() -> (tempfile::TempDir, ProjectRegistry, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(
        dir.path(),
        &towermd_core::init::InitOptions { docs_dir: None },
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "fixture"]);
    let registry = ProjectRegistry::default();
    let token = registry
        .select(Some(dir.path().into()))
        .unwrap()
        .unwrap()
        .token;
    (dir, registry, token)
}

#[test]
fn endpoint_rejects_remote_zero_port_and_missing_identity() {
    for address in ["localhost", "0.0.0.0", "::1", "192.168.1.2"] {
        let mut endpoint = fixture_endpoint(1234);
        endpoint.address = address.into();
        assert!(DaemonClient::connect(endpoint).is_err());
    }
    let mut endpoint = fixture_endpoint(0);
    assert!(endpoint.validate().is_err());
    endpoint.port = 1234;
    endpoint.instance.clear();
    assert!(endpoint.validate().is_err());
}
#[test]
fn health_mismatch_redirect_and_invalid_json_cannot_be_used_as_a_daemon() {
    for (status, body, expected) in [
        (
            200,
            json!({"address":"127.0.0.1","port":1,"pid":42,"instance":"wrong","host":"fixture-host"}),
            "daemon_changed",
        ),
        (302, json!({}), "protocol_error"),
        (200, json!({"not":"endpoint"}), "protocol_error"),
    ] {
        let (endpoint, trace, worker) = server(vec![(status, body)]);
        assert_eq!(
            DaemonClient::connect(endpoint).err().unwrap().code,
            expected
        );
        worker.join().unwrap();
        assert_eq!(trace.lock().unwrap().len(), 1);
    }
}
#[test]
fn fixed_start_wait_and_draft_routes_preserve_project_and_instance() {
    let (root, _, _) = project();
    let (endpoint, trace, worker) = server_with_health(vec![
        json!({"scan_id":ID,"state":"queued"}),
        json!({"scan_id":ID,"state":"pending"}),
        json!({"scan_id":ID,"state":"pending"}),
    ]);
    let client = DaemonClient::connect(endpoint).unwrap();
    client.scan(root.path(), None).unwrap();
    client.scan(root.path(), Some(ID)).unwrap();
    client.draft(root.path(), ID).unwrap();
    worker.join().unwrap();
    let requests = trace.lock().unwrap();
    assert!(requests[0].starts_with("GET /health "));
    assert!(requests[1].starts_with("POST /baseline/coldstart "));
    assert!(requests[1].contains("x-towermd-instance: fixture-instance"));
    assert!(requests[1].contains("\"wait\":null"));
    assert!(requests[2].contains(&format!("\"wait\":\"{ID}\"")));
    assert!(requests[3].starts_with(&format!("GET /drafts/{ID}?directory=")));
}
pub(crate) fn server_with_health(
    values: Vec<Value>,
) -> (Endpoint, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    server(
        std::iter::once((200, Value::Null))
            .chain(values.into_iter().map(|value| (200, value)))
            .collect(),
    )
}

#[test]
fn duplicate_start_returns_issued_id_and_arbitrary_ids_or_old_tokens_cannot_request() {
    let (_root, registry, token) = project();
    let scans = CandidateScans::default();
    let (endpoint, trace, worker) =
        server_with_health(vec![json!({"scan_id":ID,"state":"queued"})]);
    let first = start_with(&registry, &scans, &token, || {
        DaemonClient::connect(endpoint)
    })
    .unwrap();
    assert_eq!(
        start_with(&registry, &scans, &token, || panic!(
            "duplicate start must not rediscover or rerun"
        ))
        .unwrap(),
        first
    );
    assert!(scans.check(&token, "../../shutdown").is_err());
    assert!(scans
        .check(&token, "0192f0aa-0000-7000-8000-000000000099")
        .is_err());
    assert_eq!(
        start_with(&registry, &scans, "unknown", || panic!())
            .unwrap_err()
            .code,
        "project_missing"
    );
    worker.join().unwrap();
    assert_eq!(trace.lock().unwrap().len(), 2);
}
#[test]
fn obsolete_synchronous_output_is_incompatible_not_completed() {
    assert_eq!(
        validate_status(&json!({"state":"prepared","candidate_canvas":{}}), None)
            .unwrap_err()
            .code,
        "daemon_incompatible"
    );
    assert!(validate_status(&json!({"scan_id":ID,"state":"pending"}), Some("other")).is_err());
}

#[test]
fn completed_draft_requires_selected_project_and_fixed_source() {
    let (root, _, _) = project();
    let catalog = towermd_core::catalog::Catalog::load(root.path()).unwrap();
    let good = json!({"scan_id":ID,"state":"completed", "input":{
        "project_id":catalog.config.project_id(),"common_dir":catalog.common_dir,
        "root":root.path(),"revision":"a".repeat(40),"tree":"b".repeat(40)},
        "modules":[],"canvas":{"nodes":[],"edges":[]},"evidence":[],"report_eid":"report-id",
        "verification_report":{"commit":"a".repeat(40),"tree":"b".repeat(40),"executions":[]}});
    let mut wrong_project = good.clone();
    wrong_project["input"]["project_id"] = json!("different-project");
    let mut wrong_clone = good.clone();
    wrong_clone["input"]["common_dir"] = json!("/different/clone/.git");
    let mut wrong_commit = good.clone();
    wrong_commit["verification_report"]["commit"] = json!("c".repeat(40));
    let (endpoint, _, worker) =
        server_with_health(vec![good.clone(), wrong_project, wrong_clone, wrong_commit]);
    let client = DaemonClient::connect(endpoint).unwrap();
    assert_eq!(client.draft(root.path(), ID).unwrap(), good);
    for _ in 0..3 {
        assert_eq!(
            client.draft(root.path(), ID).unwrap_err().code,
            "protocol_error"
        );
    }
    worker.join().unwrap();
}

#[test]
fn changing_project_while_connecting_prevents_old_project_post() {
    let (_root, registry, token) = project();
    let other = tempfile::tempdir().unwrap();
    git(other.path(), &["init", "-qb", "main"]);
    let scans = CandidateScans::default();
    let (endpoint, trace, worker) = server_with_health(vec![]);
    let result = start_with(&registry, &scans, &token, || {
        let client = DaemonClient::connect(endpoint)?;
        registry.select(Some(other.path().into())).unwrap();
        Ok(client)
    });
    assert_eq!(result.unwrap_err().code, "project_missing");
    worker.join().unwrap();
    assert_eq!(trace.lock().unwrap().len(), 1);
}

#[test]
fn start_without_valid_issued_id_is_uncertain_and_cannot_be_replayed() {
    let (_root, registry, token) = project();
    let scans = CandidateScans::default();
    let (endpoint, trace, worker) =
        server_with_health(vec![json!({"state":"prepared","candidate_canvas":{}})]);
    assert_eq!(
        start_with(&registry, &scans, &token, || DaemonClient::connect(
            endpoint
        ))
        .unwrap_err()
        .code,
        "daemon_incompatible"
    );
    assert_eq!(
        start_with(&registry, &scans, &token, || panic!(
            "uncertain start must not repeat"
        ))
        .unwrap_err()
        .code,
        "scan_start_uncertain"
    );
    worker.join().unwrap();
    assert_eq!(trace.lock().unwrap().len(), 2);
}

#[test]
fn uncommitted_configuration_does_not_start_a_native_scan() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    git(
        root.path(),
        &["commit", "--allow-empty", "-qm", "plain Git"],
    );
    towermd_core::init::init(
        root.path(),
        &towermd_core::init::InitOptions { docs_dir: None },
    )
    .unwrap();
    let registry = ProjectRegistry::default();
    let token = registry
        .select(Some(root.path().into()))
        .unwrap()
        .unwrap()
        .token;
    assert_eq!(
        start_with(&registry, &CandidateScans::default(), &token, || panic!(
            "uncommitted configuration must not contact daemon"
        ))
        .unwrap_err()
        .code,
        "project_not_initialized"
    );
}
