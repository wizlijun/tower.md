//! Negative source races, not fabricated publication/approval evidence.
use super::*;
use crate::test_support::{catalog, git, inputs};

#[test]
fn final_live_probe_binds_baseline_tag_name_oid_and_symref() {
    let root = tempfile::tempdir().unwrap();
    let catalog = catalog(root.path());
    let inputs = inputs(&catalog);
    let private = crate::snapshot_git::private_revision(&catalog.root).unwrap();
    let session = std::sync::Mutex::new(
        ProjectSession::open(
            &catalog.common_dir.join("ref-probe-hub"),
            &catalog,
            &inputs,
            1,
        )
        .unwrap(),
    );
    ProjectSession::probe_session_binding(&session, &catalog, &inputs, &private).unwrap();
    let original = crate::snapshot_service::draft_ref_fingerprint(&catalog).unwrap();
    let reference = "refs/tags/towermd/B0";
    git(root.path(), &["update-ref", reference, "main"]);
    let named = crate::snapshot_service::draft_ref_fingerprint(&catalog).unwrap();
    assert_ne!(named, original);
    assert!(ProjectSession::probe_session_binding(&session, &catalog, &inputs, &private).is_err());
    // Same authority, private source and input digest: a ref-only race must fail.
    git(root.path(), &["update-ref", "-d", reference]);
    ProjectSession::probe_session_binding(&session, &catalog, &inputs, &private).unwrap();
    git(root.path(), &["update-ref", "refs/tags/towermd/B1", "main"]);
    let renamed = crate::snapshot_service::draft_ref_fingerprint(&catalog).unwrap();
    assert_ne!(named, renamed, "tag names are part of the source proof");
    git(root.path(), &["update-ref", "-d", "refs/tags/towermd/B1"]);
    git(root.path(), &["update-ref", reference, "main"]);
    git(
        root.path(),
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "another actual commit on task branch",
        ],
    );
    git(root.path(), &["update-ref", reference, "HEAD"]);
    assert_ne!(
        named,
        crate::snapshot_service::draft_ref_fingerprint(&catalog).unwrap(),
        "tag OIDs are bound even while main is unchanged"
    );
    assert!(ProjectSession::probe_session_binding(&session, &catalog, &inputs, &private).is_err());
    git(root.path(), &["update-ref", reference, "main"]);
    assert_eq!(
        named,
        crate::snapshot_service::draft_ref_fingerprint(&catalog).unwrap()
    );
    git(root.path(), &["symbolic-ref", reference, "refs/heads/main"]);
    assert_ne!(
        named,
        crate::snapshot_service::draft_ref_fingerprint(&catalog).unwrap(),
        "same peeled OID does not hide a symbolic tag"
    );
    assert!(ProjectSession::probe_session_binding(&session, &catalog, &inputs, &private).is_err());
}
