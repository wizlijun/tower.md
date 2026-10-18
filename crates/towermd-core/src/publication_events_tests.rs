use super::*;
use crate::recovery_contract::{
    baseline::ReviewBinding,
    recorded::{BaselineCreatedAction, BaselineCreatedResult},
};

// Wire-contract fixtures only. They cannot construct either opaque producer
// input, certify a real approval, or establish an actual publication.
fn created() -> NewEvent {
    let result = BaselineCreatedResult {
        commit: "a".repeat(40),
        publication_id: "b".repeat(64),
        approval_eid: "b".repeat(64),
        content_hash: "c".repeat(64),
        target: "towermd/B0".into(),
        tag: "towermd/B0".into(),
        tags: vec!["towermd/B0".into()],
    };
    let body = RequestBody {
        project_id: "project".into(),
        actor: "process:towermd-baseline".into(),
        host: "original-host".into(),
        run: None,
        action: BaselineCreatedAction::RecordBaseline {
            result: result.clone(),
        },
    };
    let request_id =
        crate::events::derived_eid(&result.commit, "baseline.created.request").unwrap();
    let hash = recovery_contract::canonical_hash(&body).unwrap();
    let record = BaselineCreated {
        result: result.clone(),
        request_body: body,
        p3_request: RequestMeta {
            request_id: request_id.clone(),
            body_hash: hash.clone(),
        },
        p3_receipt: RequestReceipt {
            request_id,
            body_hash: hash,
            result_ref: result.clone(),
        },
    };
    NewEvent {
        eid: crate::events::derived_eid(&result.commit, "baseline.created").unwrap(),
        host: "original-host".into(),
        actor: "process:towermd-baseline".into(),
        kind: "baseline.created".into(),
        layer: "L1".into(),
        obj: result.publication_id,
        guards: vec![],
        data: serde_json::to_value(record).unwrap(),
    }
}
fn prepared() -> NewEvent {
    prepared_in("project")
}
fn prepared_in(project: &str) -> NewEvent {
    let binding = ReviewBinding {
        project_id: project.into(),
        object_id: "draft".into(),
        content_hash: "c".repeat(64),
        target_baseline: "towermd/B0".into(),
        draft_revision: "d".repeat(40),
    };
    let request_id = uuid::Uuid::now_v7().to_string();
    let eid = recovery_contract::recorded::request_event_eid(
        &request_id,
        "baseline.publication_prepared",
    )
    .unwrap();
    let body = RequestBody {
        project_id: project.into(),
        actor: "process:towermd-baseline".into(),
        host: "original-host".into(),
        run: None,
        action: PublicationPrepareAction::PreparePublication {
            publication_id: "b".repeat(64),
            approval_eid: "b".repeat(64),
            binding: binding.clone(),
            authority_revision: "a".repeat(40),
            content_hash: binding.content_hash.clone(),
        },
    };
    let hash = recovery_contract::canonical_hash(&body).unwrap();
    let result = PublicationPreparedResult {
        prepared_eid: eid.clone(),
        publication_id: "b".repeat(64),
        authority_revision: "a".repeat(40),
        binding: binding.clone(),
        content_hash: binding.content_hash.clone(),
    };
    let record = PublicationPrepared {
        publication_id: "b".repeat(64),
        approval_eid: "b".repeat(64),
        binding: binding.clone(),
        authority_revision: "a".repeat(40),
        content_hash: binding.content_hash,
        request_body: body,
        p3_request: RequestMeta {
            request_id: request_id.clone(),
            body_hash: hash.clone(),
        },
        p3_receipt: RequestReceipt {
            request_id,
            body_hash: hash,
            result_ref: result,
        },
    };
    NewEvent {
        eid,
        host: "original-host".into(),
        actor: "process:towermd-baseline".into(),
        kind: "baseline.publication_prepared".into(),
        layer: "L1".into(),
        obj: "b".repeat(64),
        guards: vec![crate::hub_protocol::Fence {
            name: "main".into(),
            holder: "writer".into(),
            epoch: 7,
        }],
        data: serde_json::to_value(record).unwrap(),
    }
}

#[test]
fn shared_created_uses_derived_commit_request_id_without_app_uuid_validation() {
    let event = created();
    let identity = decode_request("project", &event).unwrap();
    let RequestIdentity::Created { meta, .. } = identity else {
        panic!("typed created")
    };
    assert!(uuid::Uuid::parse_str(&meta.request_id).is_err());
    assert_eq!(
        meta.request_id,
        crate::events::derived_eid(&"a".repeat(40), "baseline.created.request").unwrap()
    );
    let mut changed = event;
    changed.data["p3_request"]["request_id"] = serde_json::json!(uuid::Uuid::now_v7().to_string());
    assert!(decode_request("project", &changed).is_err());
}

#[test]
fn shared_publication_envelopes_require_exact_layer_actor_host_subject_and_guards() {
    for original in [prepared(), created()] {
        assert!(decode_request("project", &original).is_ok());
        for field in [
            "layer", "actor", "host", "obj", "eid", "guards", "body", "receipt",
        ] {
            let mut changed = original.clone();
            match field {
                "layer" => changed.layer = "L5".into(),
                "actor" => changed.actor = "human:owner".into(),
                "host" => changed.host = "another-host".into(),
                "obj" => changed.obj = "another-publication".into(),
                "eid" => changed.eid = "e".repeat(64),
                "guards" => {
                    if changed.kind == "baseline.created" {
                        changed.guards = prepared().guards
                    } else {
                        changed.guards.clear()
                    }
                }
                "body" => changed.data["request_body"]["run"] = serde_json::json!("implicit-run"),
                _ => changed.data["p3_receipt"]["body_hash"] = serde_json::json!("e".repeat(64)),
            }
            assert!(
                decode_request("project", &changed).is_err(),
                "{} {field}",
                original.kind
            );
        }
        assert!(decode_request("other-project", &original).is_err());
    }
    let mut wrong = prepared();
    wrong.guards[0].name = "module:A".into();
    assert!(decode_request("project", &wrong).is_err());
    let mut wrong = prepared();
    wrong.guards.push(wrong.guards[0].clone());
    assert!(decode_request("project", &wrong).is_err());
}

use crate::baseline_review_fixture as fixture;

fn approved_fixture() -> (fixture::Fixture, String) {
    let mut f = fixture::fixture();
    let action = fixture::action(&f, crate::recovery_contract::baseline::Verdict::Approved);
    let review = crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        fixture::NOW + 2,
    )
    .unwrap();
    (f, review.receipt.result_ref.approval_eid)
}

#[test]
fn actual_prepared_confirmation_failures_retry_the_original_fence_body_and_event() {
    use crate::snapshot_service::PublicationReport;
    let (mut f, eid) = approved_fixture();
    let mut original = None;
    for now in [fixture::NOW + 3, fixture::NOW + 4] {
        f.outbox.fail_next_confirmation();
        let report = f
            .writer
            .publish_baseline(&f.session, &f.catalog, &mut f.outbox, &eid, "host", || {
                Ok(now)
            })
            .unwrap();
        assert!(
            matches!(report, PublicationReport::Pending { commit: None, .. }),
            "{report:?}"
        );
        let records = f.session.lock().unwrap().events(0);
        let prepared: Vec<_> = records
            .iter()
            .filter(|event| event.kind == "baseline.publication_prepared")
            .collect();
        assert_eq!(prepared.len(), 1);
        if let Some(previous) = &original {
            assert_eq!(*prepared[0], *previous);
        } else {
            original = Some(prepared[0].clone());
        }
    }
    let report = f
        .writer
        .publish_baseline(&f.session, &f.catalog, &mut f.outbox, &eid, "host", || {
            Ok(fixture::NOW + 5)
        })
        .unwrap();
    assert!(
        matches!(report, PublicationReport::Recorded { .. }),
        "{report:?}"
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let records: Vec<_> = inputs
        .confirmed()
        .iter()
        .filter(|e| e.kind == "baseline.publication_prepared")
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(*records[0], original.unwrap());
}

#[test]
fn actual_created_confirmation_failure_keeps_canonical_derived_request_and_receipt() {
    use crate::snapshot_service::PublicationReport;
    let (mut f, eid) = approved_fixture();
    // The first confirmation is the real prepared fact; fail only the next
    // actual created confirmation, after main and B0 have really been published.
    f.outbox.fail_confirmation_after(1);
    let report = f
        .writer
        .publish_baseline(&f.session, &f.catalog, &mut f.outbox, &eid, "host", || {
            Ok(fixture::NOW + 3)
        })
        .unwrap();
    assert!(
        matches!(
            report,
            PublicationReport::Pending {
                commit: Some(_),
                ..
            }
        ),
        "{report:?}"
    );
    let current = Catalog::load(f.root.path()).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let original = inputs
        .confirmed()
        .iter()
        .find(|e| e.kind == "baseline.created")
        .unwrap()
        .clone();
    assert!(!inputs.receipt_confirmed(&original));
    assert!(
        crate::snapshot_service::pending_baseline_publications(&current, &inputs)
            .unwrap()
            .contains(&eid)
    );
    assert_eq!(
        original.eid,
        crate::events::derived_eid(
            original.data["commit"].as_str().unwrap(),
            "baseline.created"
        )
        .unwrap()
    );
    let report = f
        .writer
        .publish_baseline(&f.session, &current, &mut f.outbox, &eid, "host", || {
            Ok(fixture::NOW + 4)
        })
        .unwrap();
    let PublicationReport::Recorded { created } = report else {
        panic!("durable canonical completion")
    };
    assert_eq!(serde_json::to_value(created).unwrap(), original.data);
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.receipt_confirmed(&original));
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.kind == "baseline.created")
            .count(),
        1
    );
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .events(0)
            .iter()
            .filter(|e| e.kind == "baseline.created")
            .count(),
        1
    );
}

#[test]
fn typed_internal_pending_request_conflicts_with_app_review_in_the_same_project_index() {
    use crate::{recovery_outbox::RecoveryContext, request_ledger::BaselineRequest};
    let root = tempfile::tempdir().unwrap();
    let catalog = crate::test_support::catalog(root.path());
    let runtime = tempfile::tempdir().unwrap();
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let event = prepared_in(catalog.config.project_id());
    let record: PublicationPrepared = serde_json::from_value(event.data.clone()).unwrap();
    outbox.append_pending(&event).unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            fixture::NOW,
        )
        .unwrap(),
    );
    let request = BaselineRequest::prepare(
        catalog.config.project_id(),
        "original-host",
        "human:owner",
        &record.p3_request.request_id,
        crate::recovery_contract::recorded::BaselineReviewAction::ReviewBaseline {
            binding: record.binding,
            verdict: crate::recovery_contract::baseline::Verdict::Approved,
            comment: "must conflict before creating any human fact".into(),
        },
    )
    .unwrap();
    assert!(ProjectSession::retry_baseline_review_shared(
        &session,
        &catalog,
        &mut outbox,
        &request,
        fixture::NOW + 1
    )
    .unwrap_err()
    .contains("request_id"));
    assert!(outbox.read_inputs().unwrap().confirmed().is_empty());
    assert!(!session.lock().unwrap().events(0).iter().any(|e| matches!(
        e.kind.as_str(),
        "baseline.approved" | "baseline.publication_prepared"
    )));
}
