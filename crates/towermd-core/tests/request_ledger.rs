mod support;
use serde_json::json;
use std::{fs, sync::Mutex};
use towermd_core::{
    catalog::Catalog,
    hub_protocol::NewEvent,
    project_session::ProjectSession,
    recovery_contract::{
        baseline::{ApprovalFact, ReviewBinding, Verdict},
        recorded::{
            request_event_eid, BaselineReviewAction, BaselineReviewRecorded, RequestReceipt,
        },
    },
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
    request_ledger::BaselineRequest,
    snapshot_service::SnapshotWriter,
};
const NOW: u64 = 1_800_000_000_000;

// These programmatic records exercise the request durability contract. They do
// not exercise AppGate/native UI and do not certify a real human approval.
fn review_fixture(catalog: &Catalog, id: &str, comment: &str) -> (BaselineRequest, NewEvent) {
    let binding = ReviewBinding {
        project_id: catalog.config.project_id().into(),
        object_id: "draft-scan".into(),
        content_hash: "a".repeat(64),
        target_baseline: "towermd/B0".into(),
        draft_revision: catalog.revision.clone(),
    };
    let action = BaselineReviewAction::ReviewBaseline {
        binding: binding.clone(),
        verdict: Verdict::Approved,
        comment: comment.into(),
    };
    let request = BaselineRequest::prepare(
        catalog.config.project_id(),
        "host",
        "human:owner",
        id,
        action.clone(),
    )
    .unwrap();
    let body = request.body().clone();
    let meta = request.metadata().clone();
    let hash = meta.body_hash.clone();
    let eid = request_event_eid(id, "baseline.approved").unwrap();
    let approval = ApprovalFact {
        approval_eid: eid.clone(),
        binding,
        actor: "human:owner".into(),
        verdict: Verdict::Approved,
        comment: comment.into(),
    };
    let recorded = BaselineReviewRecorded {
        approval: approval.clone(),
        request_body: body,
        p3_request: meta,
        p3_receipt: RequestReceipt {
            request_id: id.into(),
            body_hash: hash,
            result_ref: approval,
        },
    };
    let event = NewEvent {
        eid,
        host: "host".into(),
        actor: "human:owner".into(),
        kind: "baseline.approved".into(),
        layer: "L1".into(),
        obj: "draft-scan".into(),
        guards: vec![],
        data: serde_json::to_value(recorded).unwrap(),
    };
    (request, event)
}

#[test]
fn actual_pending_retry_reopens_and_returns_original_receipt_without_new_decision() {
    let root = tempfile::tempdir().unwrap();
    let catalog = support::catalog(root.path());
    let runtime = tempfile::tempdir().unwrap();
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            NOW,
        )
        .unwrap(),
    );
    let id = uuid::Uuid::now_v7().to_string();
    let (request, event) = review_fixture(&catalog, &id, "original");
    assert!(ProjectSession::retry_baseline_review_shared(
        &session,
        &catalog,
        &mut outbox,
        &request,
        NOW
    )
    .unwrap()
    .is_none());
    assert!(outbox.read_inputs().unwrap().confirmed().is_empty());
    outbox.append_pending(&event).unwrap();
    let received = ProjectSession::retry_baseline_review_shared(
        &session,
        &catalog,
        &mut outbox,
        &request,
        NOW,
    )
    .unwrap()
    .unwrap();
    assert_eq!(received.original.eid, event.eid);
    assert_eq!(received.original.actor, "human:owner");
    assert_eq!(received.receipt.result_ref.comment, "original");
    assert_eq!(
        outbox.read_inputs().unwrap().confirmed(),
        std::slice::from_ref(&received.original)
    );
    let before = session.lock().unwrap().applied_count();
    let (different, _) = review_fixture(&catalog, &id, "replaced");
    assert!(ProjectSession::retry_baseline_review_shared(
        &session,
        &catalog,
        &mut outbox,
        &different,
        NOW + 1
    )
    .is_err());
    assert_eq!(session.lock().unwrap().applied_count(), before);
    drop(session);
    drop(outbox);
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            NOW + 2,
        )
        .unwrap(),
    );
    let repeated = ProjectSession::retry_baseline_review_shared(
        &session,
        &catalog,
        &mut outbox,
        &request,
        NOW + 2,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&repeated.receipt).unwrap(),
        serde_json::to_vec(&received.receipt).unwrap()
    );
    assert_eq!(repeated.original, received.original);
    assert!(!session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .contains_key("Baseline"));
    assert!(towermd_core::repo::git(
        root.path(),
        &["rev-parse", "--verify", "refs/tags/towermd/B0"]
    )
    .is_err());
}

#[test]
fn archive_then_restart_without_old_outbox_rebuilds_the_same_request_receipt() {
    let root = tempfile::tempdir().unwrap();
    let catalog = support::catalog(root.path());
    let runtime = tempfile::tempdir().unwrap();
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            NOW,
        )
        .unwrap(),
    );
    let (request, event) = review_fixture(
        &catalog,
        &uuid::Uuid::now_v7().to_string(),
        "archive original",
    );
    outbox.append_pending(&event).unwrap();
    let original = ProjectSession::retry_baseline_review_shared(
        &session,
        &catalog,
        &mut outbox,
        &request,
        NOW,
    )
    .unwrap()
    .unwrap();
    SnapshotWriter::default()
        .write(&session, &catalog, "host", true, || Ok(NOW + 1))
        .unwrap();
    drop(session);
    drop(outbox);
    fs::remove_dir_all(runtime.path().join("outbox")).unwrap();
    let current = Catalog::load(root.path()).unwrap();
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&current).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &current.common_dir.join("towermd"),
            &current,
            &outbox.read_inputs().unwrap(),
            NOW + 2,
        )
        .unwrap(),
    );
    let before = session.lock().unwrap().applied_count();
    let repeated = ProjectSession::retry_baseline_review_shared(
        &session,
        &current,
        &mut outbox,
        &request,
        NOW + 2,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&repeated.receipt).unwrap(),
        serde_json::to_vec(&original.receipt).unwrap()
    );
    assert_eq!(repeated.original, original.original);
    assert_eq!(session.lock().unwrap().applied_count(), before);
}

#[test]
fn accepted_original_is_restored_as_audit_then_resnapshotted_without_old_outbox() {
    let root = tempfile::tempdir().unwrap();
    let catalog = support::catalog(root.path());
    let runtime = tempfile::tempdir().unwrap();
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            NOW,
        )
        .unwrap(),
    );
    SnapshotWriter::default()
        .write(&session, &catalog, "host", true, || Ok(NOW))
        .unwrap();
    let catalog = Catalog::load(root.path()).unwrap();
    let (request, event) = review_fixture(
        &catalog,
        &uuid::Uuid::now_v7().to_string(),
        "recover exact request",
    );
    outbox.append_pending(&event).unwrap();
    let original = ProjectSession::retry_baseline_review_shared(
        &session,
        &catalog,
        &mut outbox,
        &request,
        NOW + 1,
    )
    .unwrap()
    .unwrap();
    drop(session);
    fs::remove_dir_all(catalog.common_dir.join("towermd")).unwrap();
    let inputs = outbox.read_inputs().unwrap();
    let recovery = towermd_core::recovery::prepare(
        &catalog,
        &inputs,
        NOW + 2,
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
            || Ok(NOW + 3),
        )
        .unwrap();
    let recovered_catalog = Catalog::load(root.path()).unwrap();
    let snapshot = towermd_core::snapshot_service::read_snapshot(&recovered_catalog)
        .unwrap()
        .unwrap();
    let restored: Vec<towermd_core::hub_protocol::Event> = snapshot
        .archives
        .values()
        .flat_map(|text| text.lines())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!restored
        .iter()
        .any(|event| event.kind == "baseline.approved"));
    let wrapper = restored
        .iter()
        .find(|event| event.kind == "request.receipt.restored")
        .unwrap();
    assert_eq!(
        wrapper.data["original"],
        serde_json::to_value(&original.original).unwrap()
    );
    assert_eq!(wrapper.actor, "process:towermd-daemon");
    let current = Catalog::load(root.path()).unwrap();
    SnapshotWriter::default()
        .write(&session, &current, "host", true, || Ok(NOW + 4))
        .unwrap();
    drop(session);
    drop(outbox);
    fs::remove_dir_all(runtime.path().join("outbox")).unwrap();
    let current = Catalog::load(root.path()).unwrap();
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&current).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &current.common_dir.join("towermd"),
            &current,
            &outbox.read_inputs().unwrap(),
            NOW + 5,
        )
        .unwrap(),
    );
    let repeated = ProjectSession::retry_baseline_review_shared(
        &session,
        &current,
        &mut outbox,
        &request,
        NOW + 5,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&repeated.receipt).unwrap(),
        serde_json::to_vec(&original.receipt).unwrap()
    );
    assert_eq!(repeated.original, original.original);
    assert_eq!(repeated.receipt.result_ref.comment, "recover exact request");
    assert_eq!(json!(repeated.original.guards), json!([]));
}
