//! Source-only regression coverage for reuse, not replacement of final CAS.
use super::*;
use crate::{
    ask_fixture::git,
    config::{Config, CONFIG_PATH},
};
use std::fs;

fn source_fixture() -> (tempfile::TempDir, Catalog) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    fs::create_dir(root.path().join(".towermd")).unwrap();
    fs::write(
        root.path().join(CONFIG_PATH),
        Config::new("source-probe", "docs/tower").unwrap().to_yaml(),
    )
    .unwrap();
    git(root.path(), &["add", "-f", CONFIG_PATH]);
    git(root.path(), &["commit", "-qm", "actual authority config"]);
    let catalog = Catalog::load(root.path()).unwrap();
    (root, catalog)
}
fn check_loads(catalog: &Catalog) -> (Result<(), String>, usize) {
    ACTUAL_WORKTREE_FULL_LOADS.with(|count| count.set(0));
    let result = actual_worktree(catalog);
    let loads = ACTUAL_WORKTREE_FULL_LOADS.with(|count| count.get());
    (result, loads)
}

#[test]
fn unchanged_actual_source_eliminates_the_original_one_full_catalog_load() {
    let (_root, catalog) = source_fixture();
    // Before this change actual_worktree unconditionally invoked Catalog::load
    // once. This counter counts only that call site, not continue_shared reads.
    assert!(catalog.matches_current_source().unwrap());
    let (result, loads) = check_loads(&catalog);
    result.unwrap();
    assert_eq!(loads, 0);
}

#[test]
fn branch_and_authority_drift_take_original_full_check_and_refuse_stale_catalog() {
    let (root, catalog) = source_fixture();
    git(root.path(), &["switch", "-qc", "actual-other-branch"]);
    let (result, loads) = check_loads(&catalog);
    assert!(result.unwrap_err().contains("source changed"));
    assert_eq!(loads, 1);
    git(root.path(), &["switch", "-q", "main"]);
    git(
        root.path(),
        &["commit", "--allow-empty", "-qm", "new actual authority"],
    );
    let (result, loads) = check_loads(&catalog);
    assert!(result.unwrap_err().contains("source changed"));
    assert_eq!(loads, 1);
}

#[test]
fn full_config_root_and_common_coordinates_are_required_even_when_oid_matches() {
    let (root, mut catalog) = source_fixture();
    let original = catalog.config.to_yaml();
    catalog.config = Config::parse(&format!("{original}unknown_probe_key: retained\n")).unwrap();
    assert!(!catalog.matches_current_source().unwrap());
    let (result, loads) = check_loads(&catalog);
    assert!(result.is_err());
    assert_eq!(loads, 1);
    catalog.config = Config::parse(&original).unwrap();
    let common = catalog.common_dir.clone();
    catalog.common_dir = root.path().join("different-common");
    let (result, loads) = check_loads(&catalog);
    assert!(result.is_err());
    assert_eq!(loads, 1);
    catalog.common_dir = common;
    fs::create_dir(root.path().join("subdir")).unwrap();
    catalog.root = root.path().join("subdir");
    let (result, loads) = check_loads(&catalog);
    assert!(result.is_err());
    assert_eq!(loads, 1);
}

#[test]
fn changed_physical_linked_worktree_source_is_not_reused() {
    let (root, _catalog) = source_fixture();
    let worktrees = tempfile::tempdir().unwrap();
    let linked = worktrees.path().join("actual-linked");
    git(
        root.path(),
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "main",
        ],
    );
    let catalog = Catalog::load(&linked).unwrap();
    assert!(catalog.matches_current_source().unwrap());
    let (other, _) = source_fixture();
    fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", other.path().join(".git").display()),
    )
    .unwrap();
    let (result, loads) = check_loads(&catalog);
    assert!(result.is_err());
    assert_eq!(loads, 1);
}

#[test]
fn missing_authority_probe_still_runs_original_reload_instead_of_silent_success() {
    let (root, catalog) = source_fixture();
    git(
        root.path(),
        &["remote", "add", "origin", "/unavailable-source-probe"],
    );
    let (result, loads) = check_loads(&catalog);
    assert!(result.is_err());
    assert_eq!(loads, 1);
}

#[test]
fn only_the_first_typed_preclosure_head_race_is_read_changed() {
    use crate::project_session::SessionSyncError;
    let exact = "head changed before continuation";
    let before = InboxReadFailure::before_poll(SessionSyncError::SourceChanged(exact.into()));
    assert!(before.is_read_changed());
    assert_eq!(before.to_string(), exact);
    for error in [
        SessionSyncError::Rejected(exact.into()),
        SessionSyncError::PreparationRequired,
        SessionSyncError::SourceChanged("authority changed; reload Catalog".into()),
        SessionSyncError::SourceChanged(format!("{exact} ")),
    ] {
        assert!(!InboxReadFailure::before_poll(error).is_read_changed());
    }
}

#[test]
fn after_coordination_and_poll_errors_with_identical_text_never_grant_retry() {
    use crate::project_session::SessionSyncError;
    let exact = "head changed before continuation";
    let after = InboxReadFailure::after_coordination(SessionSyncError::SourceChanged(exact.into()));
    assert!(!after.is_read_changed());
    assert_eq!(after.to_string(), exact);
    // poll's inner String result and all I/O/source/context failures use From,
    // independently of whether their display happens to equal the race text.
    assert!(!InboxReadFailure::from(exact).is_read_changed());
}
