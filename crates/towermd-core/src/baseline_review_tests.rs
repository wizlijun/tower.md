use super::*;
use crate::baseline_review_fixture as fixture;
use fixture::*;

#[test]
fn real_saved_ref_producer_stays_uncertain_until_original_confirmation_is_durable() {
    let mut f = fixture();
    let action = action(&f, Verdict::Approved);
    for now in [NOW + 2, NOW + 3] {
        f.outbox.fail_next_confirmation();
        let error = review_baseline_shared(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &action,
            "human:owner",
            "host",
            now,
        )
        .unwrap_err();
        assert!(error.contains("uncertain"));
        assert!(error.contains(&action.request_id));
        assert_eq!(
            f.session
                .lock()
                .unwrap()
                .events(0)
                .iter()
                .filter(|event| event.kind == "baseline.approved")
                .count(),
            1
        );
    }
    let result = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 4,
    )
    .unwrap();
    assert_eq!(result.current_validity, ReviewValidity::Current);
    let inputs = f.outbox.read_inputs().unwrap();
    let original = inputs
        .confirmed()
        .iter()
        .find(|event| event.eid == result.receipt.result_ref.approval_eid)
        .unwrap();
    assert_eq!(original.layer, "L1");
    assert_eq!(original.actor, "human:owner");
    assert_eq!(original.host, "host");
    assert_eq!(original.obj, action.binding.object_id);
    assert!(original.guards.is_empty());
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "baseline.approved")
            .count(),
        1
    );
}

#[test]
fn save_between_initial_read_and_final_ref_check_rejects_the_old_view_without_a_fact() {
    let mut f = fixture();
    let action = action(&f, Verdict::Approved);
    let (next, request) = next_save(&f);
    let writer = f.writer.clone();
    let session = f.session.clone();
    let catalog = f.catalog.clone();
    test_hooks::BEFORE.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(move || {
            writer
                .save_draft(&session, &catalog, &next, &request, || Ok(NOW + 2))
                .unwrap();
        }))
    });
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 3
    )
    .unwrap_err()
    .contains("changed before"));
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "baseline.approved"));
}

#[test]
fn save_after_append_preserves_original_receipt_but_new_revision_never_inherits_approval() {
    let mut f = fixture();
    let action = action(&f, Verdict::Approved);
    let (next, request) = next_save(&f);
    let writer = f.writer.clone();
    let session = f.session.clone();
    let catalog = f.catalog.clone();
    test_hooks::AFTER.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(move || {
            writer
                .save_draft(&session, &catalog, &next, &request, || Ok(NOW + 3))
                .unwrap();
        }))
    });
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
    assert!(matches!(
        result.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    assert_eq!(result.receipt.result_ref.binding, action.binding);
    let current = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    assert_ne!(current.revision(), action.binding.draft_revision);
    let repeated = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 4,
    )
    .unwrap();
    assert_eq!(repeated.receipt, result.receipt);
    assert!(matches!(
        repeated.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    let approved = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .filter(|e| e.kind == "baseline.approved")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(approved.len(), 1);
    assert_eq!(
        approved[0].data["approval"]["binding"]["draft_revision"],
        action.binding.draft_revision
    );
    let mut another = action.clone();
    another.request_id = uuid::Uuid::now_v7().to_string();
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &another,
        "human:owner",
        "host",
        NOW + 5
    )
    .is_err());
}

#[test]
fn interface_impact_ignores_unchanged_unknown_and_opaque_diff_entries() {
    let interface = "---\ntype: Interface\nfrom: unchanged-left\nto: unchanged-right\n---\nbody";
    let diffs = vec![
        PathDiff {
            path: "arch/unchanged.md".into(),
            before: Some(interface.into()),
            after: Some(interface.into()),
        },
        PathDiff {
            path: "arch/future.md".into(),
            before: None,
            after: Some(
                "---\ntype: FutureObject\nfrom: future-left\nto: future-right\n---\nopaque".into(),
            ),
        },
        PathDiff {
            path: "ordinary.md".into(),
            before: Some("untyped ordinary markdown".into()),
            after: None,
        },
        PathDiff {
            path: "tower.canvas".into(),
            before: None,
            after: Some(
                r#"{"nodes": [], "future": {"from":"canvas-left", "to":"canvas-right"}}"#.into(),
            ),
        },
        PathDiff {
            path: "arch/opaque.md".into(),
            before: Some("---\ntype: [invalid\nfrom: opaque-left\n---\nopaque".into()),
            after: None,
        },
    ];
    assert!(affected_interface_modules(&diffs).is_empty());
}

#[test]
fn accepted_review_retry_input_failure_is_uncertain_before_confirmation_coordination() {
    use crate::request_ledger::{BaselineRequestLedger, RequestIdentity};
    let mut f = fixture();
    let action = action(&f, Verdict::Approved);
    let original = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap();
    let request = BaselineRequest::prepare(
        f.catalog.config.project_id(),
        "host",
        "human:owner",
        &action.request_id,
        BaselineReviewAction::ReviewBaseline {
            binding: action.binding.clone(),
            verdict: action.verdict.clone(),
            comment: action.comment.clone(),
        },
    )
    .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let history = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    let ledger = BaselineRequestLedger::from_history(&f.catalog, &inputs, &history).unwrap();
    let mut unused_hub = crate::hub::Hub::open(&f.runtime.path().join("unused-hub")).unwrap();
    let path = f.runtime.path().join("outbox");
    let retained = f.runtime.path().join("retained-outbox");
    std::fs::rename(&path, &retained).unwrap();
    std::fs::write(&path, "injected unreadable outbox directory").unwrap();
    let error = ledger
        .retry_original(
            &f.catalog,
            &RequestIdentity::Review(request),
            &mut unused_hub,
            &mut f.outbox,
            NOW + 3,
        )
        .unwrap_err();
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&retained, &path).unwrap();
    assert!(
        matches!(error, crate::hub_protocol::HubError::Uncertain(_)),
        "{error}"
    );
    let repeated = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 4,
    )
    .unwrap();
    assert_eq!(repeated.receipt, original.receipt);
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .events(0)
            .iter()
            .filter(|event| event.kind == "baseline.approved")
            .count(),
        1
    );
}

#[test]
fn accepted_review_followup_read_failure_is_uncertain_and_retries_the_original_receipt() {
    let mut f = fixture();
    let action = action(&f, Verdict::Approved);
    let path = f.runtime.path().join("outbox");
    let retained = f.runtime.path().join("retained-outbox");
    let (blocked_path, moved_path) = (path.clone(), retained.clone());
    crate::recovery_outbox::CONFIRMATION_OBSERVER.with(|observer| {
        *observer.borrow_mut() = Some(Box::new(move || {
            std::fs::rename(&blocked_path, &moved_path).unwrap();
            std::fs::write(&blocked_path, "injected unreadable outbox directory").unwrap();
        }));
    });
    let error = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap_err();
    assert!(error.contains("uncertain"), "{error}");
    assert!(error.contains(&action.request_id), "{error}");
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&retained, &path).unwrap();
    let original = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| event.kind == "baseline.approved")
        .unwrap()
        .clone();
    let result = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 3,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(result.receipt).unwrap(),
        original.data["p3_receipt"]
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "baseline.approved")
            .count(),
        1
    );
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .events(0)
            .iter()
            .filter(|event| event.kind == "baseline.approved")
            .count(),
        1
    );
}
