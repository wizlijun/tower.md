mod support;
use std::sync::Mutex;
use towermd_core::{
    project_session::ProjectSession,
    snapshot_service::SnapshotWriter,
    tasks::{TaskCommand, TaskSpec},
};
fn private_rollback(operation: &str) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let catalog = support::catalog(&root);
    let checkout = dir.path().join("main");
    support::git(
        &root,
        &["worktree", "add", "-q", checkout.to_str().unwrap(), "main"],
    );
    let inputs = support::inputs(&catalog);
    let session =
        Mutex::new(ProjectSession::open(&root.join("hub"), &catalog, &inputs, 0).unwrap());
    let writer = SnapshotWriter::default();
    assert_eq!(
        writer
            .write(&session, &catalog, "host", true, || Ok(10))
            .unwrap()
            .status,
        "written"
    );
    let s0 = support::git(&root, &["rev-parse", "refs/towermd/snapshot"]);
    ProjectSession::execute_shared(
        &session,
        &catalog,
        &inputs,
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T".into(),
                title: "Task".into(),
                module: "A".into(),
                touches: vec![],
                allowed_paths: vec!["src/file.rs".into()],
                depends_on: vec![],
                acceptance: vec!["AC".into()],
                decisions: vec![],
            },
        },
        None,
        20,
    )
    .unwrap();
    if operation == "expire" {
        ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            21,
        )
        .unwrap();
    }
    assert_eq!(
        writer
            .write(&session, &catalog, "host", true, || Ok(30))
            .unwrap()
            .status,
        "written"
    );
    let s1 = support::git(&root, &["rev-parse", "refs/towermd/snapshot"]);
    assert_ne!(s0, s1);
    ProjectSession::sync_shared(&session, &catalog, &inputs, 31).unwrap();
    assert_eq!(session.lock().unwrap().snapshot_ack().unwrap().commit, s1);
    let before = session.lock().unwrap().projection().unwrap().clone();
    let applied = session.lock().unwrap().applied_seq();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    support::git(&root, &["update-ref", "refs/towermd/snapshot", &s0, &s1]);
    let refs = support::git(&root, &["show-ref"]);
    let result = match operation {
        "show" => ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Show {
                id: Some("T".into()),
            },
            None,
            32,
        )
        .map(|_| ())
        .map_err(|e| e.to_string()),
        "claim" => towermd_core::tasks::execute(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            32,
        )
        .map(|_| ())
        .map_err(|e| e.to_string()),
        "expire" => {
            towermd_core::tasks::expire(&session, &catalog, &inputs, "host", 15 * 60 * 1000 + 22)
        }
        "reconcile" => ProjectSession::reconcile_pending_shared(&session, &catalog, &inputs, 32),
        "subscribe" => ProjectSession::subscribe_shared(&session, &catalog, &inputs, applied, 32)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        _ => unreachable!(),
    };
    assert!(
        result.is_err(),
        "public {operation} accepted actual ack-bound private S1 to S0 rollback: {result:?}"
    );
    let error = result.unwrap_err();
    assert!(
        error.contains("ack") || error.contains("rollback") || error.contains("source changed"),
        "{operation}: {error}"
    );
    assert_eq!(*session.lock().unwrap().projection().unwrap(), before);
    assert_eq!(session.lock().unwrap().applied_seq(), applied);
    assert_eq!(support::git(&root, &["show-ref"]), refs);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert!(!session
        .lock()
        .unwrap()
        .events(applied)
        .iter()
        .any(|event| event.kind.starts_with("task.")));
}
macro_rules! private_source_test {
    ($name:ident,$operation:literal) => {
        #[test]
        fn $name() {
            private_rollback($operation);
        }
    };
}
private_source_test!(public_show_rejects_ack_bound_private_rollback, "show");
private_source_test!(public_claim_rejects_ack_bound_private_rollback, "claim");
private_source_test!(public_expire_rejects_ack_bound_private_rollback, "expire");
private_source_test!(
    public_reconcile_rejects_ack_bound_private_rollback,
    "reconcile"
);
private_source_test!(
    public_subscribe_rejects_ack_bound_private_rollback,
    "subscribe"
);
