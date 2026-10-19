mod support;
use std::fs;
use towermd_core::{
    app_read_model::TaskProgressState, catalog::Catalog, hub::Hub, project_session::ProjectSession,
    recovery_outbox::RecoveryInputs,
};
fn fixture() -> (tempfile::TempDir, Catalog, RecoveryInputs) {
    let root = tempfile::tempdir().unwrap();
    support::git(root.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(root.path(), &Default::default()).unwrap();
    support::git(root.path(), &["add", "."]);
    support::git(root.path(), &["commit", "-qm", "real read-only fixture"]);
    let catalog = Catalog::load(root.path()).unwrap();
    let inputs = RecoveryInputs::read(&root.path().join("not-created-outbox"), &catalog).unwrap();
    (root, catalog, inputs)
}
#[test]
fn missing_and_empty_hub_read_never_create_storage_or_claim_current_empty_state() {
    let (_root, catalog, inputs) = fixture();
    let path = catalog.common_dir.join("towermd");
    let view = ProjectSession::read_task_progress(&catalog, &inputs, 1000, None).unwrap();
    assert_eq!(view.state, TaskProgressState::Unavailable);
    assert_eq!(view.through_seq, None);
    assert!(view.holds.is_empty() && view.events.is_empty());
    assert_eq!(view.events_floor_seq, None);
    assert!(!path.exists());
    drop(Hub::open(&path).unwrap());
    let bytes = fs::read(path.join("journal.jsonl")).unwrap();
    let view = ProjectSession::read_task_progress(&catalog, &inputs, 1000, None).unwrap();
    assert_eq!(view.state, TaskProgressState::Unavailable);
    assert_eq!(view.through_seq, None);
    assert_eq!(fs::read(path.join("journal.jsonl")).unwrap(), bytes);
}

#[test]
fn recorded_event_window_is_bounded_actual_tail_and_old_dtos_remain_readable() {
    use std::sync::Mutex;
    use towermd_core::hub_protocol::NewEvent;
    let (_root, catalog, inputs) = fixture();
    let session = Mutex::new(
        ProjectSession::open(&catalog.common_dir.join("towermd"), &catalog, &inputs, 1).unwrap(),
    );
    for index in 0..260 {
        session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .append(
                NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "actual-audit-host".into(),
                    actor: "process:test".into(),
                    kind: "future.audit".into(),
                    layer: "L5".into(),
                    obj: "audit".into(),
                    data: serde_json::json!({"index":index}),
                    guards: vec![],
                },
                index + 2,
            )
            .unwrap();
    }
    let path = catalog.common_dir.join("towermd/journal.jsonl");
    let before = fs::read(&path).unwrap();
    let view = ProjectSession::read_task_progress(&catalog, &inputs, 1000, Some(&session)).unwrap();
    assert_eq!(view.state, TaskProgressState::Recorded);
    assert_eq!(view.events.len(), 256);
    assert_eq!(view.floor_seq, Some(0));
    assert_eq!(view.events_floor_seq, Some(4));
    assert_eq!(view.through_seq, Some(260));
    assert_eq!(view.events.first().unwrap().seq, 5);
    assert_eq!(view.events.last().unwrap().seq, 260);
    assert!(view.events.iter().all(|event| event.kind == "future.audit"));
    assert!(view.holds.is_empty());
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(session.lock().unwrap().applied_seq(), 0);
    let mut legacy = serde_json::to_value(view).unwrap();
    for key in ["holds", "events", "events_floor_seq"] {
        legacy.as_object_mut().unwrap().remove(key);
    }
    let compatible: towermd_core::app_read_model::TaskProgressView =
        serde_json::from_value(legacy).unwrap();
    assert!(compatible.holds.is_empty() && compatible.events.is_empty());
    assert_eq!(compatible.events_floor_seq, None);
}
#[test]
fn cold_foreign_owner_and_missing_lock_cannot_make_recorded_bytes_accepted() {
    let (_root, catalog, inputs) = fixture();
    let path = catalog.common_dir.join("towermd");
    let owner = Hub::open(&path).unwrap();
    let bytes = fs::read(path.join("journal.jsonl")).unwrap();
    assert!(ProjectSession::read_task_progress(&catalog, &inputs, 1000, None).is_err());
    assert_eq!(fs::read(path.join("journal.jsonl")).unwrap(), bytes);
    drop(owner);
    fs::remove_file(path.join("hub.lock")).unwrap();
    assert!(ProjectSession::read_task_progress(&catalog, &inputs, 1000, None).is_err());
    assert!(!path.join("hub.lock").exists());
}
#[test]
fn corrupt_complete_line_and_partial_tail_are_rejected_without_repair() {
    use std::io::Write;
    let (_root, catalog, inputs) = fixture();
    let path = catalog.common_dir.join("towermd");
    drop(Hub::open(&path).unwrap());
    let original = fs::read(path.join("journal.jsonl")).unwrap();
    for tail in [
        b"bad complete line\n".as_slice(),
        b"{\"partial\":".as_slice(),
    ] {
        fs::write(path.join("journal.jsonl"), &original).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(path.join("journal.jsonl"))
            .unwrap()
            .write_all(tail)
            .unwrap();
        let bytes = fs::read(path.join("journal.jsonl")).unwrap();
        assert!(ProjectSession::read_task_progress(&catalog, &inputs, 1000, None).is_err());
        assert_eq!(fs::read(path.join("journal.jsonl")).unwrap(), bytes);
    }
}
