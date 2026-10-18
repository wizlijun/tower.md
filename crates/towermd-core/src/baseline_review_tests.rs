use super::*;
#[path = "../tests/support/baseline_review.rs"]
mod fixture;
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
