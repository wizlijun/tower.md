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
    assert!(!path.exists());
    drop(Hub::open(&path).unwrap());
    let bytes = fs::read(path.join("journal.jsonl")).unwrap();
    let view = ProjectSession::read_task_progress(&catalog, &inputs, 1000, None).unwrap();
    assert_eq!(view.state, TaskProgressState::Unavailable);
    assert_eq!(view.through_seq, None);
    assert_eq!(fs::read(path.join("journal.jsonl")).unwrap(), bytes);
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
