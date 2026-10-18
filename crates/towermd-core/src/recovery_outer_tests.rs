//! Private contract-history tests for outer coordinates. Fake coverage metadata
//! below tests the helper only; it never creates a public trusted-history path.
use super::*;
use crate::hub::Hub;
use crate::hub_protocol::HubProtocol;

fn fixture() -> (
    tempfile::TempDir,
    Catalog,
    VerifiedRecoveryHistory,
    Vec<Event>,
) {
    let root = tempfile::tempdir().unwrap();
    let catalog = crate::test_support::catalog(root.path());
    let mut hub = Hub::open(&root.path().join("isolated-hub")).unwrap();
    let mut events = Vec::new();
    for n in 1..=3 {
        let event = NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "host".into(),
            actor: "process:test-outer".into(),
            kind: "outer.recorded".into(),
            layer: "L5".into(),
            obj: format!("outer-{n}"),
            guards: vec![],
            data: json!({"value":n}),
        };
        let crate::hub_protocol::AppendReceipt::Event(actual) = hub.append(event, n).unwrap()
        else {
            panic!("actual Event expected");
        };
        events.push(*actual);
    }
    let history = read_history(
        &catalog,
        &crate::test_support::inputs(&catalog),
        &events[..2],
        &[],
    )
    .unwrap();
    (root, catalog, history, events)
}

#[test]
fn accepted_outer_schedule_never_uses_confirmed_or_inner_original_coordinates() {
    let (_root, catalog, mut history, events) = fixture();
    history
        .candidate
        .originals
        .insert(events[2].eid.clone(), events[2].clone());
    history.schedule_events.reverse();
    history.schedule_events.push(events[0].clone());
    let result = history.accepted_outer_events(&catalog, 2).unwrap();
    assert_eq!(result, vec![&events[0], &events[1]]);
    assert!(history.accepted_outer_events(&catalog, 3).is_err());
    assert!(history
        .accepted_outer_events(&catalog, 0)
        .unwrap()
        .is_empty());
    let mut changed = events[0].clone();
    changed.data = json!({"different":true});
    history.schedule_events.push(changed);
    assert!(history
        .accepted_outer_events(&catalog, 2)
        .unwrap_err()
        .contains("conflicting"));
}

#[test]
fn accepted_outer_schedule_rejects_missing_sequences_and_changed_authority() {
    let (root, catalog, mut history, events) = fixture();
    history.schedule_events = vec![events[1].clone()];
    assert!(history
        .accepted_outer_events(&catalog, 2)
        .unwrap_err()
        .contains("uncovered"));
    history.schedule_events = events[..2].to_vec();
    crate::test_support::git(root.path(), &["switch", "main"]);
    crate::test_support::git(
        root.path(),
        &["commit", "--allow-empty", "-qm", "changed authority"],
    );
    assert!(history
        .accepted_outer_events(&catalog, 2)
        .unwrap_err()
        .contains("binding changed"));
}

#[test]
fn accepted_outer_schedule_preserves_rebuild_outer_after_a_verified_sequence_jump() {
    let (_root, catalog, mut history, events) = fixture();
    let jump = 1_u64 << 32;
    let mut rebuilt = events[2].clone();
    rebuilt.seq = jump + 1;
    rebuilt.kind = "hub.rebuilt".into();
    history.schedule_events.push(rebuilt.clone());
    history.captured_recovery_gaps = vec![crate::snapshot::SequenceRange {
        start: 3,
        end: jump,
    }];
    let result = history.accepted_outer_events(&catalog, jump + 1).unwrap();
    assert_eq!(result, vec![&events[0], &events[1], &rebuilt]);
    for through in [3, 20, jump] {
        assert!(history
            .accepted_outer_events(&catalog, through)
            .unwrap_err()
            .contains("inside a recovery gap"));
    }
    assert!(history.accepted_outer_events(&catalog, jump + 2).is_err());
}

// The receipts below come from real Hub appends. These tests exercise the
// captured-tail side of the reader. The complete App recovery/archive path
// needs separate integration coverage; no private candidate constructor proves it.
fn stale_task_fixture(
    kind: &str,
) -> (
    tempfile::TempDir,
    Catalog,
    VerifiedRecoveryHistory,
    NewEvent,
    Event,
) {
    let root = tempfile::tempdir().unwrap();
    let catalog = crate::test_support::catalog(root.path());
    let mut hub = Hub::open(&root.path().join("isolated-hub")).unwrap();
    let guards = vec![crate::hub_protocol::Fence {
        name: "module:A".into(),
        holder: "missing-run".into(),
        epoch: 1,
    }];
    let original = NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "host".into(),
        actor: "process:towermd-cli".into(),
        kind: kind.into(),
        layer: "L5".into(),
        obj: "T".into(),
        guards: guards.clone(),
        data: json!({
            "task": {"type":"Task","id":"T","title":"Original Task",
                "module":"A","touches":[],"allowed_paths":["src/file.rs"],
                "depends_on":[],"acceptance":["AC"],"decisions":[],"state":"running"},
            "run": {"type":"Run","id":"missing-run","task":"T","host":"host",
                "worktree":catalog.root,"branch":catalog.branch,"state":"running",
                "leases":guards,"cost_usd":null,"task_card":{}}
        }),
    };
    let crate::hub_protocol::AppendReceipt::Event(actual) =
        hub.append(original.clone(), 1).unwrap()
    else {
        panic!("actual Task receipt expected");
    };
    assert!(
        actual.stale,
        "no lease exists for the actual guarded append"
    );
    let actual = *actual;
    let history = read_history(
        &catalog,
        &crate::test_support::inputs(&catalog),
        std::slice::from_ref(&actual),
        &[],
    )
    .unwrap();
    (root, catalog, history, original, actual)
}

#[test]
fn task_receipts_preserve_real_outer_stale_receipts_for_finite_originals() {
    for kind in ["task.state", "task.claimed", "task.released"] {
        let (_root, catalog, mut history, original, actual) = stale_task_fixture(kind);
        history.schedule_events.push(actual.clone());
        let receipts = history
            .task_receipts(&catalog, actual.seq, &[original.clone(), original.clone()])
            .unwrap();
        assert_eq!(receipts.len(), 1);
        let receipt = receipts[&original.eid].clone();
        let restored = receipt
            .event_for(catalog.config.project_id(), &original)
            .unwrap();
        assert_eq!(restored, &actual);
        assert!(restored.stale);
        assert_eq!(crate::recovery_outbox::original_of(restored), original);
    }
}

#[test]
fn task_receipts_do_not_use_pending_or_private_inner_candidates() {
    let (_task_root, _task_catalog, _task_history, original, actual) =
        stale_task_fixture("task.claimed");
    let (_root, catalog, _history, events) = fixture();
    let mut outbox = crate::test_support::outbox(&catalog);
    outbox.append_pending(&original).unwrap();
    let inputs = outbox.read_inputs().unwrap();
    assert!(inputs.pending().contains(&original));
    let mut history = read_history(&catalog, &inputs, &events[..2], &[]).unwrap();
    assert!(history
        .task_receipts(&catalog, 2, std::slice::from_ref(&original))
        .unwrap()
        .is_empty());
    // A private contract fixture models an inner or confirmed-only role. Its
    // actual receipt seq is within the requested range, but it has no outer
    // occurrence in this source. It must never become a repair token.
    assert!(actual.seq <= 2);
    history
        .candidate
        .originals
        .insert(original.eid.clone(), actual);
    assert!(history
        .task_receipts(&catalog, 2, std::slice::from_ref(&original))
        .unwrap()
        .is_empty());
}

#[test]
fn task_receipts_reject_changed_originals_project_and_source_binding() {
    let (_root, mut catalog, mut history, original, actual) = stale_task_fixture("task.state");
    let receipts = history
        .task_receipts(&catalog, actual.seq, std::slice::from_ref(&original))
        .unwrap();
    let receipt = &receipts[&original.eid];
    assert!(receipt.event_for("another-project", &original).is_err());
    for change in [
        "body", "guard", "actor", "host", "kind", "object", "layer", "eid",
    ] {
        let mut changed = original.clone();
        match change {
            "body" => changed.data["task"]["title"] = json!("Changed Task"),
            "guard" => changed.guards[0].epoch += 1,
            "actor" => changed.actor = "process:another-producer".into(),
            "host" => changed.host = "another-host".into(),
            "kind" => changed.kind = "task.released".into(),
            "object" => changed.obj = "another-task".into(),
            "layer" => changed.layer = "L4".into(),
            _ => changed.eid = uuid::Uuid::now_v7().to_string(),
        }
        assert!(
            receipt
                .event_for(catalog.config.project_id(), &changed)
                .is_err(),
            "{change}"
        );
        if change != "eid" {
            assert!(
                history
                    .task_receipts(&catalog, actual.seq, &[original.clone(), changed.clone()])
                    .is_err(),
                "conflicting {change}"
            );
            assert!(
                history
                    .task_receipts(&catalog, actual.seq, std::slice::from_ref(&changed))
                    .is_err(),
                "mismatched {change}"
            );
        } else {
            assert!(history
                .task_receipts(&catalog, actual.seq, std::slice::from_ref(&changed))
                .unwrap()
                .is_empty());
        }
    }
    let mut unsupported = original.clone();
    unsupported.kind = "merge.done".into();
    assert!(history
        .task_receipts(&catalog, actual.seq, &[unsupported])
        .is_err());
    history.authority = "f".repeat(catalog.revision.len());
    assert!(history
        .task_receipts(&catalog, actual.seq, std::slice::from_ref(&original))
        .is_err());
    history.authority = catalog.revision.clone();
    catalog.config =
        crate::config::Config::new("another-project", catalog.config.docs_dir()).unwrap();
    assert!(history
        .task_receipts(&catalog, actual.seq, std::slice::from_ref(&original))
        .is_err());
}
