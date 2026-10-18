#[path = "support/baseline_review.rs"]
mod fixture;
use fixture::*;
use towermd_core::{
    baseline_review::{review_baseline_shared, review_view, ReviewValidity},
    catalog::Catalog,
    recovery_contract::baseline::Verdict,
    snapshot_service::read_draft,
};

#[test]
fn real_private_ref_review_is_exact_durable_idempotent_and_never_publishes() {
    let mut f = fixture();
    let view = review_view(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &f.frozen.bundle.id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(view.binding.draft_revision, f.revision);
    assert_eq!(view.draft, f.frozen);
    assert!(view.current_baseline.is_none());
    assert!(view.diff.iter().any(|d| d.path.ends_with("tower.canvas")));
    let action = action(&f, Verdict::Approved);
    let before = git(f.root.path(), &["show-ref"]);
    let result = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap();
    assert_eq!(result.current_validity, ReviewValidity::Current);
    assert_eq!(result.receipt.result_ref.binding, action.binding);
    assert_eq!(result.receipt.result_ref.actor, "human:owner");
    let first = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|e| e.kind == "baseline.approved")
        .unwrap()
        .clone();
    assert_eq!(first.guards.len(), 0);
    assert_eq!(first.epoch, None);
    assert_eq!(first.eid, result.receipt.result_ref.approval_eid);
    let repeated = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 3,
    )
    .unwrap();
    assert_eq!(repeated, result);
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "baseline.approved")
            .count(),
        1
    );
    assert_eq!(git(f.root.path(), &["show-ref"]), before);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .contains_key("Baseline"));
    let mut changed = action.clone();
    changed.comment.push_str(" replaced");
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &changed,
        "human:owner",
        "host",
        NOW + 4
    )
    .is_err());
}

#[test]
fn rejection_is_terminal_for_that_exact_revision_and_no_timeout_or_new_request_reverses_it() {
    let mut f = fixture();
    let rejected = action(&f, Verdict::Rejected);
    let result = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &rejected,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap();
    assert_eq!(result.receipt.result_ref.verdict, Verdict::Rejected);
    let mut approved = rejected.clone();
    approved.request_id = uuid::Uuid::now_v7().to_string();
    approved.verdict = Verdict::Approved;
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &approved,
        "human:owner",
        "host",
        NOW + 100_000
    )
    .unwrap_err()
    .contains("terminal"));
    let repeated = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &rejected,
        "human:owner",
        "host",
        NOW + 200_000,
    )
    .unwrap();
    assert_eq!(repeated.receipt, result.receipt);
    assert_eq!(
        read_draft(&f.catalog, &f.frozen.bundle.id)
            .unwrap()
            .unwrap()
            .revision(),
        f.revision
    );
}

#[test]
fn expected_project_revision_hash_target_actor_and_ordinary_source_changes_are_checked_before_append(
) {
    let mut f = fixture();
    let original = action(&f, Verdict::Approved);
    let count = f.outbox.read_inputs().unwrap().confirmed().len();
    for field in ["project", "revision", "hash", "target", "id"] {
        let mut changed = original.clone();
        match field {
            "project" => changed.binding.project_id = "other".into(),
            "revision" => changed.binding.draft_revision = "c".repeat(40),
            "hash" => changed.binding.content_hash = "b".repeat(64),
            "target" => changed.binding.target_baseline = "towermd/B1".into(),
            _ => changed.binding.object_id = "missing".into(),
        }
        assert!(
            review_baseline_shared(
                &f.session,
                &f.catalog,
                &mut f.outbox,
                &changed,
                "human:owner",
                "host",
                NOW + 2
            )
            .is_err(),
            "{field}"
        );
    }
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &original,
        "process:towermd-cli",
        "host",
        NOW + 2
    )
    .is_err());
    assert_eq!(f.outbox.read_inputs().unwrap().confirmed().len(), count);
    let other = tempfile::tempdir().unwrap();
    git(
        f.root.path(),
        &[
            "clone",
            "--no-local",
            f.root.path().to_str().unwrap(),
            other.path().to_str().unwrap(),
        ],
    );
    let other_catalog = Catalog::load(other.path()).unwrap();
    assert_ne!(other_catalog.common_dir, f.catalog.common_dir);
    assert!(review_baseline_shared(
        &f.session,
        &other_catalog,
        &mut f.outbox,
        &original,
        "human:owner",
        "host",
        NOW + 2
    )
    .is_err());
    assert_eq!(f.outbox.read_inputs().unwrap().confirmed().len(), count);
    git(f.root.path(), &["switch", "main"]);
    std::fs::write(
        f.root.path().join("docs/tower/new-ordinary.md"),
        "ordinary source occupancy\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "docs/tower/new-ordinary.md"]);
    git(
        f.root.path(),
        &["commit", "-qm", "current protected directory changed"],
    );
    git(f.root.path(), &["switch", "review-fixture"]);
    let current = Catalog::load(f.root.path()).unwrap();
    let saved = read_draft(&current, &f.frozen.bundle.id).unwrap().unwrap();
    assert_eq!(saved.frozen(), &f.frozen);
    let historical_view = review_view(
        &current,
        &f.outbox.read_inputs().unwrap(),
        &f.frozen.bundle.id,
    )
    .unwrap()
    .unwrap();
    assert!(!historical_view.publish_blockers.is_empty());
    assert!(review_baseline_shared(
        &f.session,
        &current,
        &mut f.outbox,
        &original,
        "human:owner",
        "host",
        NOW + 3
    )
    .is_err());
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind.starts_with("baseline."))
            .count(),
        0
    );
}

#[test]
fn actual_saved_review_survives_hub_recovery_resnapshot_and_old_outbox_removal() {
    use std::sync::{Arc, Mutex};
    use towermd_core::{
        project_session::ProjectSession,
        recovery_outbox::{RecoveryContext, RecoveryOutbox},
        snapshot_service::SnapshotWriter,
    };
    let mut f = fixture();
    f.writer
        .write(&f.session, &f.catalog, "host", true, || Ok(NOW + 2))
        .unwrap();
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let action = action(&f, Verdict::Approved);
    let original = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 3,
    )
    .unwrap();
    let Fixture {
        root,
        runtime,
        catalog,
        session,
        mut outbox,
        ..
    } = f;
    drop(session);
    std::fs::remove_dir_all(catalog.common_dir.join("towermd")).unwrap();
    let inputs = outbox.read_inputs().unwrap();
    let request = towermd_core::recovery::prepare(
        &catalog,
        &inputs,
        NOW + 4,
        NOW + 1000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (_, session) = SnapshotWriter::default()
        .confirm_rebuild(
            None,
            &catalog,
            &request,
            "human:recovery-tester",
            request.expires_at,
            &inputs,
            "host",
            || Ok(NOW + 5),
        )
        .unwrap();
    let current = Catalog::load(root.path()).unwrap();
    SnapshotWriter::default()
        .write(&session, &current, "host", true, || Ok(NOW + 6))
        .unwrap();
    drop(session);
    drop(outbox);
    std::fs::remove_dir_all(runtime.path().join("outbox")).unwrap();
    let current = Catalog::load(root.path()).unwrap();
    outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&current).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &current.common_dir.join("towermd"),
            &current,
            &outbox.read_inputs().unwrap(),
            NOW + 7,
        )
        .unwrap(),
    );
    let restored = review_baseline_shared(
        &session,
        &current,
        &mut outbox,
        &action,
        "human:owner",
        "host",
        NOW + 7,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&restored.receipt).unwrap(),
        serde_json::to_vec(&original.receipt).unwrap()
    );
    assert_eq!(restored.current_validity, ReviewValidity::Current);
    assert!(!session
        .lock()
        .unwrap()
        .events(0)
        .iter()
        .any(|e| e.kind == "baseline.approved"));
}
