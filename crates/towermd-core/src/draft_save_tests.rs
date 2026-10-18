use super::*;
use crate::baseline_review_fixture as fixture;
use fixture::*;

#[test]
fn actual_saved_request_stays_uncertain_through_pending_and_confirmation_fsync_failures() {
    let mut f = fixture();
    let saved = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let request = saved.request();
    f.outbox.fail_next_pending();
    let failed = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        request,
        || Ok(NOW + 2),
    )
    .unwrap_err();
    assert!(failed.contains("uncertain"));
    assert!(!f
        .session
        .lock()
        .unwrap()
        .events(0)
        .iter()
        .any(|e| e.kind == "draft.saved"));
    for now in [NOW + 3, NOW + 4] {
        f.outbox.fail_next_confirmation();
        let failed = save_draft_shared(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &f.writer,
            request,
            || Ok(now),
        )
        .unwrap_err();
        assert!(failed.contains("uncertain"));
        assert!(failed.contains(&request.metadata().request_id));
        assert!(matches!(
            read_baseline_request(
                &f.catalog,
                &f.outbox.read_inputs().unwrap(),
                &request.metadata().request_id
            )
            .unwrap(),
            Some(BaselineRequestStatus::DraftSave { receipt: None, .. })
        ));
        assert_eq!(
            f.session
                .lock()
                .unwrap()
                .events(0)
                .iter()
                .filter(|e| e.kind == "draft.saved")
                .count(),
            1
        );
    }
    let original = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        request,
        || Ok(NOW + 5),
    )
    .unwrap();
    assert_eq!(original.receipt.result_ref.revision, saved.revision());
    let repeated = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        request,
        || Ok(NOW + 6),
    )
    .unwrap();
    assert_eq!(repeated.receipt, original.receipt);
    let inputs = f.outbox.read_inputs().unwrap();
    let event = inputs
        .confirmed()
        .iter()
        .find(|e| e.kind == "draft.saved")
        .unwrap();
    assert_eq!(
        event.data["request_body"],
        serde_json::to_value(request.body()).unwrap()
    );
    assert_eq!(
        event.data["p3_receipt"],
        serde_json::to_value(original.receipt).unwrap()
    );
    for field in [
        "layer",
        "actor",
        "host",
        "obj",
        "eid",
        "guards",
        "request",
        "receipt",
        "reference",
        "revision",
        "hash",
    ] {
        let mut wrong = recovery_outbox::original_of(event);
        match field {
            "layer" => wrong.layer = "L5".into(),
            "actor" => wrong.actor = "human:other".into(),
            "host" => wrong.host = "other".into(),
            "obj" => wrong.obj = "other".into(),
            "eid" => wrong.eid = "a".repeat(64),
            "guards" => wrong.guards.push(crate::hub_protocol::Fence {
                name: "main".into(),
                holder: "writer".into(),
                epoch: 1,
            }),
            "request" => wrong.data["request_body"]["run"] = serde_json::json!("different"),
            "receipt" => {
                wrong.data["p3_receipt"]["result_ref"]["revision"] =
                    serde_json::json!("a".repeat(40))
            }
            "reference" => wrong.data["result"]["reference"] = serde_json::json!("refs/heads/main"),
            "revision" => wrong.data["result"]["revision"] = serde_json::json!("bad"),
            _ => wrong.data["result"]["content_hash"] = serde_json::json!("bad"),
        }
        assert!(
            decode_saved(f.catalog.config.project_id(), &wrong).is_err(),
            "{field}"
        );
    }
}

#[test]
fn shared_uncertainty_survives_writer_reopen_and_hides_confirmation_until_sync_finishes() {
    use crate::recovery_outbox::{OutboxUncertainty, RecoveryContext, RecoveryInputs};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let mut f = fixture();
    let uncertainty = OutboxUncertainty::default();
    let path = f.runtime.path().join("outbox");
    let context = RecoveryContext::from_catalog(&f.catalog).unwrap();
    drop(f.outbox);
    f.outbox =
        RecoveryOutbox::open_with_uncertainty(&path, context.clone(), uncertainty.clone()).unwrap();
    let saved = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let request = saved.request();
    f.outbox.fail_next_confirmation();
    assert!(save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        request,
        || Ok(NOW + 2)
    )
    .unwrap_err()
    .contains("uncertain"));
    drop(f.outbox);
    let inputs =
        RecoveryInputs::read_with_uncertainty(&path, &f.catalog, uncertainty.clone()).unwrap();
    assert!(matches!(
        read_baseline_request(&f.catalog, &inputs, &request.metadata().request_id).unwrap(),
        Some(BaselineRequestStatus::DraftSave { receipt: None, .. })
    ));
    assert!(matches!(
        read_baseline_request(
            &f.catalog,
            &inputs.reload(&f.catalog).unwrap(),
            &request.metadata().request_id
        )
        .unwrap(),
        Some(BaselineRequestStatus::DraftSave { receipt: None, .. })
    ));
    f.outbox = RecoveryOutbox::open_with_uncertainty(&path, context, uncertainty.clone()).unwrap();
    let observed = Arc::new(AtomicBool::new(false));
    let witness = observed.clone();
    let catalog = f.catalog.clone();
    let id = request.metadata().request_id.clone();
    crate::recovery_outbox::CONFIRMATION_OBSERVER.with(|observer| {
        *observer.borrow_mut() = Some(Box::new(move || {
            // Run the read while the writer is paused before the final fsync.
            // The query takes no producer or Session lock.
            std::thread::spawn(move || {
                let inputs =
                    RecoveryInputs::read_with_uncertainty(&path, &catalog, uncertainty).unwrap();
                assert!(matches!(
                    read_baseline_request(&catalog, &inputs, &id).unwrap(),
                    Some(BaselineRequestStatus::DraftSave { receipt: None, .. })
                ));
                witness.store(true, Ordering::SeqCst);
            })
            .join()
            .unwrap();
        }));
    });
    let receipt = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        request,
        || Ok(NOW + 3),
    )
    .unwrap();
    assert!(observed.load(Ordering::SeqCst));
    assert!(
        matches!(read_baseline_request(&f.catalog,&f.outbox.read_inputs().unwrap(),&request.metadata().request_id).unwrap(),Some(BaselineRequestStatus::DraftSave{receipt:Some(actual),..}) if actual==receipt.receipt)
    );
}

#[test]
fn actual_cas_followup_clock_failure_is_uncertain_and_reuses_the_saved_commit() {
    let mut f = fixture();
    let (_, request) = next_save(&f);
    let previous = f.revision.clone();
    let reference = f
        .catalog
        .common_dir
        .join("refs/towermd/drafts")
        .join(request.draft_id());
    let error = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || {
            // This test clock fails only after the real update-ref CAS has replaced
            // the tip. It never substitutes a materialization/approval proof.
            if std::fs::read_to_string(&reference).unwrap().trim() != previous {
                Err("injected clock failure after actual draft CAS".into())
            } else {
                Ok(NOW + 2)
            }
        },
    )
    .unwrap_err();
    assert!(error.contains("uncertain"), "{error}");
    assert!(error.contains(&request.metadata().request_id), "{error}");
    let saved = read_draft(&f.catalog, request.draft_id()).unwrap().unwrap();
    assert_ne!(saved.revision(), previous);
    assert_eq!(saved.request(), &request);
    let original = saved.revision().to_owned();
    let result = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 3),
    )
    .unwrap();
    assert_eq!(result.receipt.result_ref.revision, original);
    assert_eq!(
        read_draft(&f.catalog, request.draft_id())
            .unwrap()
            .unwrap()
            .revision(),
        original
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "draft.saved"
                && event.data["p3_request"]["request_id"] == request.metadata().request_id)
            .count(),
        1
    );
}
