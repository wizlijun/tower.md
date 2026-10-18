#[path = "support/baseline_review.rs"]
mod fixture;
use fixture::*;
use std::{fs, sync::Mutex};
use towermd_core::{
    baseline_review::review_baseline_shared,
    catalog::Catalog,
    project_session::ProjectSession,
    recovery_contract::baseline::Verdict,
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
    request_ledger::{
        read_baseline_request, save_draft_shared, BaselineRequestStatus, DraftDelivery,
        DraftSaveAction, DraftSaveValidity, PreparedDraftSaveRequest,
    },
    snapshot_service::{read_draft, SnapshotWriter},
};

#[test]
fn actual_cas_gap_is_read_only_recoverable_and_conflicts_across_actions() {
    let mut f = fixture();
    let saved = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let request = saved.request().clone();
    let main = f.catalog.revision.clone();
    let status = read_baseline_request(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &request.metadata().request_id,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        status,
        BaselineRequestStatus::DraftSave { receipt: None, .. }
    ));
    let mut review = action(&f, Verdict::Approved);
    review.request_id = request.metadata().request_id.clone();
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &review,
        "human:owner",
        "host",
        NOW + 2
    )
    .unwrap_err()
    .contains("request_id"));
    let (_, edit) = next_save(&f);
    let changed =
        PreparedDraftSaveRequest::prepare(edit.body().clone(), &request.metadata().request_id)
            .unwrap();
    assert!(save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &changed,
        || Ok(NOW + 2)
    )
    .unwrap_err()
    .contains("request_id"));
    let original = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 3),
    )
    .unwrap();
    assert_eq!(original.receipt.result_ref.revision, f.revision);
    assert_eq!(
        original.receipt.result_ref.delivery,
        DraftDelivery::LocalSaved
    );
    assert_eq!(original.current_validity, DraftSaveValidity::Current);
    assert_eq!(Catalog::load(f.root.path()).unwrap().revision, main);
    let inputs = f.outbox.read_inputs().unwrap();
    let event = inputs
        .confirmed()
        .iter()
        .find(|e| e.kind == "draft.saved")
        .unwrap();
    assert_eq!(event.layer, "L1");
    assert_eq!(event.actor, request.body().actor);
    assert_eq!(event.host, request.body().host);
    assert_eq!(event.obj, request.draft_id());
    assert!(event.guards.is_empty());
    let again = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 4),
    )
    .unwrap();
    assert_eq!(again.receipt, original.receipt);
    assert!(
        matches!(read_baseline_request(&f.catalog, &f.outbox.read_inputs().unwrap(), &request.metadata().request_id).unwrap(), Some(BaselineRequestStatus::DraftSave { receipt: Some(receipt), .. }) if receipt == original.receipt)
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "draft.saved")
            .count(),
        1
    );
    let review = action(&f, Verdict::Approved);
    let approved = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &review,
        "human:owner",
        "host",
        NOW + 5,
    )
    .unwrap();
    for body in [request.body().clone(), edit.body().clone()] {
        let conflicting = PreparedDraftSaveRequest::prepare(body, &review.request_id).unwrap();
        assert!(save_draft_shared(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &f.writer,
            &conflicting,
            || Ok(NOW + 6)
        )
        .unwrap_err()
        .contains("request_id"));
    }
    let repeated = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &review,
        "human:owner",
        "host",
        NOW + 7,
    )
    .unwrap();
    assert_eq!(repeated.receipt, approved.receipt);
}

#[test]
fn real_submit_and_edit_own_derivation_and_finish_old_save_before_replacing_tip() {
    let mut f = fixture();
    let saved = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let request = PreparedDraftSaveRequest::prepare(
        saved.request().body().clone(),
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    git(
        f.root.path(),
        &[
            "update-ref",
            "-d",
            &format!("refs/towermd/drafts/{}", request.draft_id()),
        ],
    );
    let original = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 2),
    )
    .unwrap();
    assert_eq!(
        original.receipt.result_ref.content_hash,
        f.frozen.content_hash
    );
    f.revision = original.receipt.result_ref.revision.clone();
    let (expected, edit) = next_save(&f);
    let edited = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &edit,
        || Ok(NOW + 3),
    )
    .unwrap();
    assert_eq!(
        edited.receipt.result_ref.content_hash,
        expected.content_hash
    );
    let actual = read_draft(&f.catalog, edit.draft_id()).unwrap().unwrap();
    assert_eq!(actual.frozen(), &expected);
    assert_eq!(actual.request(), &edit);
    let old = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 4),
    )
    .unwrap();
    assert_eq!(old.receipt, original.receipt);
    assert!(matches!(
        old.current_validity,
        DraftSaveValidity::HistoricalOnly { .. }
    ));
    let mut review = action(&f, Verdict::Rejected);
    review.request_id = edit.metadata().request_id.clone();
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &review,
        "human:owner",
        "host",
        NOW + 5
    )
    .unwrap_err()
    .contains("request_id"));
    let mut wrong = edit.body().clone();
    wrong.action = DraftSaveAction::EditDraft {
        id: edit.draft_id().into(),
        expected_revision: actual.revision().into(),
        edit: towermd_core::baseline_draft::DraftEdit::Move {
            node: f.frozen.bundle.objects.keys().next().unwrap().clone(),
            x: 1,
            y: 2,
        },
    };
    assert!(save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &PreparedDraftSaveRequest::prepare(wrong, &edit.metadata().request_id).unwrap(),
        || Ok(NOW + 6)
    )
    .is_err());
}

#[test]
fn origin_present_recovery_never_invents_remote_delivery_and_prelude_reserves_before_cas() {
    let mut f = fixture();
    git(
        f.root.path(),
        &[
            "remote",
            "add",
            "origin",
            "/nonexistent/towermd-test-origin",
        ],
    );
    git(
        f.root.path(),
        &[
            "update-ref",
            "refs/remotes/origin/main",
            &f.catalog.revision,
        ],
    );
    let saved = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let request = saved.request().clone();
    let receipt = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 2),
    )
    .unwrap();
    assert_eq!(
        receipt.receipt.result_ref.delivery,
        DraftDelivery::LocalSavedRemotePending {
            reason: "remote delivery not confirmed during recovery".into()
        }
    );
    let (_, edit) = next_save(&f);
    assert!(ProjectSession::retry_draft_save_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &edit,
        NOW + 3
    )
    .unwrap()
    .is_none());
    let mut review = action(&f, Verdict::Approved);
    review.request_id = edit.metadata().request_id.clone();
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &review,
        "human:owner",
        "host",
        NOW + 4
    )
    .unwrap_err()
    .contains("request_id"));
    let result = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &edit,
        || Ok(NOW + 5),
    )
    .unwrap();
    assert!(matches!(
        result.receipt.result_ref.delivery,
        DraftDelivery::LocalSavedRemotePending { .. }
    ));
}

#[test]
fn saved_receipt_survives_actual_hub_rebuild_resnapshot_and_removed_outbox() {
    let mut f = fixture();
    f.writer
        .write(&f.session, &f.catalog, "host", true, || Ok(NOW + 2))
        .unwrap();
    let catalog = Catalog::load(f.root.path()).unwrap();
    let saved = read_draft(&catalog, &f.frozen.bundle.id).unwrap().unwrap();
    let request = saved.request().clone();
    let original = save_draft_shared(
        &f.session,
        &catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 3),
    )
    .unwrap();
    let Fixture {
        root,
        runtime,
        session,
        mut outbox,
        ..
    } = f;
    drop(session);
    fs::remove_dir_all(catalog.common_dir.join("towermd")).unwrap();
    let inputs = outbox.read_inputs().unwrap();
    let recovery = towermd_core::recovery::prepare(
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
            &recovery,
            "human:recovery-tester",
            recovery.expires_at,
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
    fs::remove_dir_all(runtime.path().join("outbox")).unwrap();
    let current = Catalog::load(root.path()).unwrap();
    outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&current).unwrap(),
    )
    .unwrap();
    // Pure lookup must not recreate a missing Hub or write a confirmation.
    let status = read_baseline_request(
        &current,
        &outbox.read_inputs().unwrap(),
        &request.metadata().request_id,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        status,
        BaselineRequestStatus::DraftSave { receipt: None, .. }
    ));
    assert!(outbox.read_inputs().unwrap().confirmed().is_empty());
    let session = Mutex::new(
        ProjectSession::open(
            &current.common_dir.join("towermd"),
            &current,
            &outbox.read_inputs().unwrap(),
            NOW + 7,
        )
        .unwrap(),
    );
    let restored = save_draft_shared(
        &session,
        &current,
        &mut outbox,
        &SnapshotWriter::default(),
        &request,
        || Ok(NOW + 8),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&restored.receipt).unwrap(),
        serde_json::to_vec(&original.receipt).unwrap()
    );
    assert!(!session
        .lock()
        .unwrap()
        .events(0)
        .iter()
        .any(|e| e.kind == "draft.saved"));
}

#[test]
fn pure_request_lookup_does_not_open_or_recreate_a_missing_hub() {
    let mut f = fixture();
    let saved = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let request = saved.request().clone();
    save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 2),
    )
    .unwrap();
    f.writer
        .write(&f.session, &f.catalog, "host", true, || Ok(NOW + 3))
        .unwrap();
    let Fixture {
        root,
        session,
        outbox,
        ..
    } = f;
    drop(session);
    let current = Catalog::load(root.path()).unwrap();
    let hub = current.common_dir.join("towermd");
    fs::remove_dir_all(&hub).unwrap();
    let inputs = outbox.read_inputs().unwrap();
    assert!(matches!(
        read_baseline_request(&current, &inputs, &request.metadata().request_id).unwrap(),
        Some(BaselineRequestStatus::DraftSave {
            receipt: Some(_),
            ..
        })
    ));
    assert!(!hub.exists());
    let unknown = uuid::Uuid::now_v7().to_string();
    assert!(read_baseline_request(&current, &inputs, &unknown)
        .unwrap()
        .is_none());
    assert!(!hub.exists());
}

#[test]
fn materialized_request_recovery_precedes_live_source_validation() {
    let mut f = fixture();
    let saved = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let request = saved.request();
    git(f.root.path(), &["switch", "main"]);
    fs::write(
        f.root.path().join("docs/tower/changed-after-save.md"),
        "ordinary protected source occupancy\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "docs/tower/changed-after-save.md"]);
    git(
        f.root.path(),
        &[
            "commit",
            "-qm",
            "protected directory advanced after actual CAS",
        ],
    );
    git(f.root.path(), &["switch", "review-fixture"]);
    let current = Catalog::load(f.root.path()).unwrap();
    let original = save_draft_shared(
        &f.session,
        &current,
        &mut f.outbox,
        &f.writer,
        request,
        || Ok(NOW + 2),
    )
    .unwrap();
    assert_eq!(original.receipt.result_ref.revision, saved.revision());
    assert!(matches!(
        original.current_validity,
        DraftSaveValidity::HistoricalOnly { .. }
    ));
    let again = save_draft_shared(
        &f.session,
        &current,
        &mut f.outbox,
        &f.writer,
        request,
        || Ok(NOW + 3),
    )
    .unwrap();
    assert_eq!(again.receipt, original.receipt);
    assert_eq!(
        read_draft(&current, request.draft_id())
            .unwrap()
            .unwrap()
            .revision(),
        saved.revision()
    );
}
