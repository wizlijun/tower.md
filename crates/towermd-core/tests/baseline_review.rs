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
fn review_identifies_actual_main_without_replacing_the_fixed_draft_source() {
    let f = fixture();
    let source_revision = f.catalog.revision.clone();
    git(f.root.path(), &["switch", "main"]);
    std::fs::write(f.root.path().join("alpha/a.py"), "# newer main code\n").unwrap();
    git(f.root.path(), &["add", "alpha/a.py"]);
    git(f.root.path(), &["commit", "-qm", "advance only code"]);
    let catalog = Catalog::load(f.root.path()).unwrap();
    assert_ne!(catalog.revision, source_revision);
    let refs = git(f.root.path(), &["show-ref"]);
    let index = std::fs::read(f.root.path().join(".git/index")).unwrap();
    let status = git(f.root.path(), &["status", "--porcelain=v1"]);
    let view = review_view(
        &catalog,
        &f.outbox.read_inputs().unwrap(),
        &f.frozen.bundle.id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_value(&view).unwrap()["main_revision"],
        catalog.revision
    );
    assert_eq!(view.draft.bundle.source_revision, source_revision);
    assert_eq!(view.binding.draft_revision, f.revision);
    assert_eq!(git(f.root.path(), &["show-ref"]), refs);
    assert_eq!(
        std::fs::read(f.root.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(git(f.root.path(), &["status", "--porcelain=v1"]), status);
}

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
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 2),
            &mut f.outbox,
        )
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
        .write(
            &session,
            &current,
            "host",
            true,
            || Ok(NOW + 6),
            &mut outbox,
        )
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

#[test]
fn review_verification_is_the_exact_recorded_report_and_evidence_and_edits_keep_scan_binding() {
    use towermd_core::{baseline_review::ReviewVerification, coldstart_evidence::read_completed};
    let f = fixture();
    let inputs = f.outbox.read_inputs().unwrap();
    let completed = read_completed(&f.catalog, &inputs, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let view = review_view(&f.catalog, &inputs, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let ReviewVerification::Available {
        scan_id,
        source_revision,
        report_eid,
        report,
        evidence,
    } = view.verification
    else {
        panic!("real completed scan")
    };
    assert_eq!(scan_id, completed.scan_id());
    assert_eq!(source_revision, completed.input().revision);
    assert_eq!(report_eid, completed.report_eid());
    assert_eq!(report.as_ref(), completed.report());
    assert_eq!(evidence, completed.evidence());
    let (next, request) = next_save(&f);
    f.writer
        .save_draft(&f.session, &f.catalog, &next, &request, || Ok(NOW + 2))
        .unwrap();
    let next_view = review_view(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &f.frozen.bundle.id,
    )
    .unwrap()
    .unwrap();
    assert_ne!(next_view.binding, view.binding);
    assert!(
        matches!(next_view.verification,ReviewVerification::Available{report:next_report,evidence:next_evidence,..} if next_report==report && next_evidence==evidence)
    );
}

#[test]
fn a_real_imported_draft_without_completed_coldstart_has_explicit_unavailable_verification() {
    use towermd_core::{
        baseline_draft::freeze,
        baseline_review::ReviewVerification,
        request_ledger::{DraftSaveAction, PreparedDraftSaveRequest},
    };
    let f = fixture();
    // Actual Git materialization fixture for a future/imported draft format;
    // this is not the Submit producer and creates no approval or save receipt.
    let mut bundle = f.frozen.bundle.clone();
    bundle.id = "imported-without-coldstart".into();
    let frozen = freeze(&f.catalog, &bundle).unwrap();
    let saved = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    let mut body = saved.request().body().clone();
    let DraftSaveAction::SubmitBaseline { submit } = &mut body.action else {
        unreachable!()
    };
    submit.scan_id = bundle.id.clone();
    let request =
        PreparedDraftSaveRequest::prepare(body, &uuid::Uuid::now_v7().to_string()).unwrap();
    f.writer
        .save_draft(&f.session, &f.catalog, &frozen, &request, || Ok(NOW + 2))
        .unwrap();
    let view = review_view(&f.catalog, &f.outbox.read_inputs().unwrap(), &bundle.id)
        .unwrap()
        .unwrap();
    assert!(
        matches!(view.verification,ReviewVerification::Unavailable{ref reason} if !reason.is_empty())
    );
    let wire = serde_json::to_value(view).unwrap();
    assert_eq!(wire["verification"]["state"], "unavailable");
    assert!(wire["verification"].get("report").is_none());
    assert!(wire["verification"].get("evidence").is_none());
}

#[test]
fn actual_review_diff_includes_deleted_and_replaced_interface_endpoints() {
    use towermd_core::{
        baseline_draft::{apply_edit, freeze, DraftEdit},
        recovery_contract::recorded::RequestBody,
        request_ledger::{DraftSaveAction, PreparedDraftSaveRequest},
    };
    let f = fixture_with_source_files(&[
        ("docs/tower/arch/left.md", "---\ntype: Module\nid: left\ntitle: Left\npaths: [alpha]\nverify: []\ninterfaces: [api]\n---\nleft\n"),
        ("docs/tower/arch/right.md", "---\ntype: Module\nid: right\ntitle: Right\npaths: [alpha]\nverify: []\ninterfaces: [api]\n---\nright\n"),
        ("docs/tower/arch/replacement.md", "---\ntype: Module\nid: replacement\ntitle: Replacement\npaths: [alpha]\nverify: []\ninterfaces: []\n---\nreplacement\n"),
        ("docs/tower/arch/api.md", "---\ntype: Interface\nid: api\nfrom: left\nto: right\nx-future: {values: [2, 1]}\n---\noriginal interface\n"),
    ]);
    let before = review_view(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &f.frozen.bundle.id,
    )
    .unwrap()
    .unwrap();
    assert!(
        before.interface_modules.is_empty(),
        "unchanged real source interface"
    );
    let edits = [
        (
            "deleted-interface",
            DraftEdit::RemoveInterface {
                interface: "api".into(),
            },
            vec!["left", "right"],
        ),
        (
            "changed-interface",
            DraftEdit::PutInterface {
                object: f.frozen.bundle.objects["api"].clone(),
                from: "left".into(),
                to: "replacement".into(),
            },
            vec!["left", "replacement", "right"],
        ),
    ];
    for (id, edit, expected) in edits {
        let bundle = apply_edit(&f.frozen.bundle, &edit).unwrap();
        let frozen = freeze(&f.catalog, &bundle).unwrap();
        // Real low-level materialization fixture isolates review diff semantics;
        // it does not mint Submit success or an approval fact.
        let expected_revision = read_draft(&f.catalog, &bundle.id)
            .unwrap()
            .unwrap()
            .revision()
            .to_owned();
        let request = PreparedDraftSaveRequest::prepare(
            RequestBody {
                project_id: f.catalog.config.project_id().into(),
                actor: "human:owner".into(),
                host: "host".into(),
                run: None,
                action: DraftSaveAction::EditDraft {
                    id: bundle.id.clone(),
                    expected_revision,
                    edit,
                },
            },
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        f.writer
            .save_draft(&f.session, &f.catalog, &frozen, &request, || Ok(NOW + 2))
            .unwrap();
        let view = review_view(&f.catalog, &f.outbox.read_inputs().unwrap(), &bundle.id)
            .unwrap()
            .unwrap();
        assert_eq!(view.interface_modules, expected, "{id}");
        let diff = view
            .diff
            .iter()
            .find(|diff| diff.path == "docs/tower/arch/api.md")
            .unwrap();
        assert!(diff.before.as_ref().unwrap().contains("to: right"));
        if id == "deleted-interface" {
            assert!(diff.after.is_none());
        } else {
            assert!(diff.after.as_ref().unwrap().contains("to: replacement"));
        }
    }
}
