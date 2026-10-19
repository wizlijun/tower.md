use super::*;
use crate::daemon_client::{tests::server, DaemonClient};
use serde_json::{json, Value};
fn project() -> (tempfile::TempDir, ProjectRegistry, String) {
    let root = tempfile::tempdir().unwrap();
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
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    git(root.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(root.path(), &Default::default()).unwrap();
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-qm", "actual selected project"]);
    let registry = ProjectRegistry::default();
    let token = registry
        .select(Some(root.path().into()))
        .unwrap()
        .unwrap()
        .token;
    (root, registry, token)
}
fn view(registry: &ProjectRegistry, token: &str) -> Value {
    let catalog = draft_requests::context(registry, token).unwrap();
    json!({"project_id":catalog.config.project_id(),"common_dir":catalog.common_dir,"revision":catalog.revision,"captured_at":1000,"through_seq":1,"floor_seq":0,"state":"recorded","source":{"kind":"hub_journal"},"tasks":[{"type":"Task","id":"T","state":"ready","future":{"keep":[2,1]}}],"runs":[],"evidence":[],"diagnostics":[]})
}
#[test]
fn fixed_native_progress_read_checks_health_project_and_preserves_unknown_fields() {
    let (_root, registry, token) = project();
    let expected = view(&registry, &token);
    let (endpoint, trace, worker) = server(vec![
        (200, Value::Null),
        (200, expected.clone()),
        (200, Value::Null),
    ]);
    let result = load_with(&registry, &token, || DaemonClient::connect(endpoint)).unwrap();
    worker.join().unwrap();
    assert_eq!(serde_json::to_value(result).unwrap(), expected);
    let requests = trace.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].starts_with("GET /app/projection?directory="));
    assert!(requests[1].contains("x-towermd-instance: fixture-instance"));
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    assert!(!requests
        .iter()
        .any(|request| request.contains("/command") || request.contains("/inbox")));
}
#[test]
fn foreign_project_common_and_incoherent_recovery_response_are_rejected() {
    let (_root, registry, token) = project();
    for key in ["project_id", "common_dir", "revision", "state"] {
        let mut body = view(&registry, &token);
        body[key] = json!(if key == "state" {
            "recovery_required"
        } else {
            "foreign"
        });
        let (endpoint, _, worker) =
            server(vec![(200, Value::Null), (200, body), (200, Value::Null)]);
        assert!(
            load_with(&registry, &token, || DaemonClient::connect(endpoint)).is_err(),
            "{key}"
        );
        worker.join().unwrap();
    }
}
#[test]
fn redirected_and_changed_instance_reads_are_not_used_or_retried() {
    let (_root, registry, token) = project();
    let (endpoint, trace, worker) = server(vec![(200, Value::Null), (302, json!({}))]);
    assert_eq!(
        load_with(&registry, &token, || DaemonClient::connect(endpoint))
            .unwrap_err()
            .code,
        "protocol_error"
    );
    worker.join().unwrap();
    assert_eq!(trace.lock().unwrap().len(), 2);
    let (endpoint, _, worker) = server(vec![
        (200, Value::Null),
        (200, view(&registry, &token)),
        (
            200,
            json!({"address":"127.0.0.1","port":1,"pid":42,"instance":"other","host":"fixture-host"}),
        ),
    ]);
    assert_eq!(
        load_with(&registry, &token, || DaemonClient::connect(endpoint))
            .unwrap_err()
            .code,
        "daemon_changed"
    );
    worker.join().unwrap();
}
#[test]
fn project_switch_during_discovery_rejects_the_old_token_before_get() {
    let (_root, registry, token) = project();
    let (other, _, _) = project();
    let (endpoint, trace, worker) = server(vec![(200, Value::Null)]);
    let result = load_with(&registry, &token, || {
        let client = DaemonClient::connect(endpoint)?;
        registry.select(Some(other.path().into()))?;
        Ok(client)
    });
    assert_eq!(result.unwrap_err().code, "project_missing");
    worker.join().unwrap();
    assert_eq!(trace.lock().unwrap().len(), 1);
}
