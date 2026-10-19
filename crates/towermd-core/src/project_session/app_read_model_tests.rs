use super::*;
use crate::hub_protocol::NewEvent;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
type Hook = Box<dyn FnOnce() + Send>;
fn hooks() -> &'static Mutex<std::collections::BTreeMap<PathBuf, Hook>> {
    static HOOKS: OnceLock<Mutex<std::collections::BTreeMap<PathBuf, Hook>>> = OnceLock::new();
    HOOKS.get_or_init(Default::default)
}
pub(super) fn before_final(root: &Path) {
    let hook = hooks().lock().unwrap().remove(root);
    if let Some(hook) = hook {
        hook();
    }
}
fn original() -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:test".into(),
        kind: "future.audit".into(),
        layer: "L5".into(),
        obj: "audit".into(),
        data: serde_json::json!({}),
        guards: vec![],
    }
}
#[test]
fn actual_complete_and_partial_hub_fsync_failures_are_not_progress_acceptance() {
    for partial in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(root.path());
        let inputs = crate::test_support::inputs(&catalog);
        let path = catalog.common_dir.join("towermd");
        let mut hub = crate::hub::Hub::open(&path).unwrap();
        hub.fail_next_append(partial);
        let live = Mutex::new(ProjectSession::new(Box::new(hub), &catalog, &inputs, 1).unwrap());
        assert!(matches!(
            live.lock()
                .unwrap()
                .protocol()
                .unwrap()
                .append(original(), 2),
            Err(crate::hub_protocol::HubError::Uncertain(_))
        ));
        assert_eq!(live.lock().unwrap().protocol().unwrap().contiguous_seq(), 0);
        assert_eq!(
            live.lock()
                .unwrap()
                .task_progress_cursor(catalog.config.project_id(), &catalog.common_dir)
                .unwrap()
                .1,
            0,
            "physical bytes before successful Hub fsync never become App notifications"
        );
        let bytes = fs::read(path.join("journal.jsonl")).unwrap();
        let error =
            ProjectSession::read_task_progress(&catalog, &inputs, 3, Some(&live)).unwrap_err();
        if !partial {
            assert!(
                error.to_string().contains("尚未得到实际 Hub 确认"),
                "{error}"
            );
        }
        assert_eq!(fs::read(path.join("journal.jsonl")).unwrap(), bytes);
    }
}
#[test]
fn actual_journal_authority_and_outbox_changes_after_proof_require_manual_refresh() {
    for change in ["journal", "authority", "outbox"] {
        let root = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(root.path());
        let mut outbox = crate::test_support::outbox(&catalog);
        let inputs = outbox.read_inputs().unwrap();
        let live = Arc::new(Mutex::new(
            ProjectSession::open(&catalog.common_dir.join("towermd"), &catalog, &inputs, 1)
                .unwrap(),
        ));
        let current = live.clone();
        let directory = catalog.root.clone();
        hooks().lock().unwrap().insert(
            catalog.root.clone(),
            Box::new(move || {
                match change {
                    "journal" => {
                        current
                            .lock()
                            .unwrap()
                            .protocol()
                            .unwrap()
                            .append(original(), 2)
                            .unwrap();
                    }
                    "authority" => {
                        let old_main = crate::catalog::authority_revision(&directory).unwrap();
                        // The shared fixture checks out t/T. A task-branch commit
                        // alone does not change authority; advance real main by CAS.
                        crate::test_support::git(
                            &directory,
                            &[
                                "commit",
                                "--allow-empty",
                                "-qm",
                                "actual new main during read",
                            ],
                        );
                        let new_main = crate::test_support::git(&directory, &["rev-parse", "HEAD"]);
                        assert_ne!(
                            new_main, old_main,
                            "the new real commit must differ from original authority"
                        );
                        crate::test_support::git(
                            &directory,
                            &["update-ref", "refs/heads/main", &new_main, &old_main],
                        );
                        assert_eq!(
                            crate::catalog::authority_revision(&directory).unwrap(),
                            new_main,
                            "the hook must move actual authority, not only worktree HEAD"
                        );
                    }
                    "outbox" => {
                        outbox.append_pending(&original()).unwrap();
                    }
                    _ => unreachable!(),
                }
            }),
        );
        let error =
            ProjectSession::read_task_progress(&catalog, &inputs, 3, Some(&live)).unwrap_err();
        assert!(
            matches!(error, SessionSyncError::SourceChanged(_)),
            "{change}: {error}"
        );
        assert_eq!(
            live.lock().unwrap().applied_seq(),
            0,
            "read never installs the changed record"
        );
    }
}
#[test]
fn retired_or_foreign_session_cannot_supply_warm_read_acceptance() {
    let root = tempfile::tempdir().unwrap();
    let catalog = crate::test_support::catalog(root.path());
    let inputs = crate::test_support::inputs(&catalog);
    let live = Mutex::new(
        ProjectSession::open(&catalog.common_dir.join("towermd"), &catalog, &inputs, 1).unwrap(),
    );
    let foreign = tempfile::tempdir().unwrap();
    let other = crate::test_support::catalog(foreign.path());
    let other_inputs = crate::test_support::inputs(&other);
    assert!(ProjectSession::read_task_progress(&other, &other_inputs, 2, Some(&live)).is_err());
    live.lock().unwrap().retire_empty(&catalog, 2).unwrap();
    assert!(live
        .lock()
        .unwrap()
        .task_progress_cursor(catalog.config.project_id(), &catalog.common_dir)
        .is_err());
    assert!(
        ProjectSession::read_task_progress(&catalog, &inputs, 3, Some(&live))
            .unwrap_err()
            .to_string()
            .contains("retired")
    );
}
