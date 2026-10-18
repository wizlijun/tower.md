#[path = "support/baseline_review.rs"]
mod fixture;
use fixture::*;
use towermd_core::{
    baseline_current::current_baseline, baseline_review::review_baseline_shared, catalog::Catalog,
    recovery_contract::baseline::Verdict, snapshot_service::PublicationReport,
};

fn published() -> Fixture {
    let mut f = fixture_with_source_files(&[("docs/tower/intents/I-existing.md",
        "---\r\ntype: Intent\r\nid: I-existing\r\n# original comment\r\nfuture: {values: [2, 1]}\r\n---\r\noriginal body without final newline")]);
    let action = action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap();
    let publication = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 5),
        )
        .unwrap();
    assert!(
        matches!(publication, PublicationReport::Recorded { .. }),
        "{publication:?}"
    );
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    f
}

#[test]
fn current_baseline_comes_from_real_publication_and_rejects_unproved_labels() {
    let f = published();
    let current = current_baseline(&f.catalog, &f.outbox.read_inputs().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(current.baseline, "towermd/B0");
    assert_eq!(
        current.main_revision,
        git(f.root.path(), &["rev-parse", "main"])
    );
    for label in ["towermd/B01", "towermd/B1", "towermd/B2"] {
        git(f.root.path(), &["tag", label, "main"]);
        assert!(
            current_baseline(&f.catalog, &f.outbox.read_inputs().unwrap()).is_err(),
            "{label}"
        );
        git(f.root.path(), &["tag", "-d", label]);
    }
    git(
        f.root.path(),
        &[
            "symbolic-ref",
            "refs/tags/towermd/B1",
            "refs/tags/towermd/B0",
        ],
    );
    assert!(
        current_baseline(&f.catalog, &f.outbox.read_inputs().unwrap())
            .unwrap_err()
            .contains("symbolic")
    );
    git(
        f.root.path(),
        &["symbolic-ref", "--delete", "refs/tags/towermd/B1"],
    );
    let blob = git(f.root.path(), &["rev-parse", "main:alpha/a.py"]);
    git(
        f.root.path(),
        &["update-ref", "refs/tags/towermd/B1", &blob],
    );
    assert!(current_baseline(&f.catalog, &f.outbox.read_inputs().unwrap()).is_err());
    git(f.root.path(), &["update-ref", "-d", "refs/tags/towermd/B1"]);
    let before = current.main_revision;
    git(f.root.path(), &["switch", "-qc", "code-advance", "main"]);
    std::fs::write(
        f.root.path().join("alpha/a.py"),
        "# unrelated code advancement\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/a.py"]);
    git(f.root.path(), &["commit", "-qm", "code advancement"]);
    let advanced = git(f.root.path(), &["rev-parse", "HEAD"]);
    git(
        f.root.path(),
        &["update-ref", "refs/heads/main", &advanced, &before],
    );
    assert!(
        current_baseline(&f.catalog, &f.outbox.read_inputs().unwrap())
            .unwrap_err()
            .contains("conflict")
    );
    let fresh = Catalog::load(f.root.path()).unwrap();
    let fresh_current = current_baseline(&fresh, &f.outbox.read_inputs().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(fresh_current.baseline, "towermd/B0");
    assert_eq!(fresh_current.main_revision, advanced);
    let id = uuid::Uuid::now_v7().to_string();
    assert!(towermd_core::baseline_draft::from_current_baseline(
        &fresh,
        &f.outbox.read_inputs().unwrap(),
        &id,
        "towermd/B0",
        &before
    )
    .unwrap_err()
    .contains("conflict"));
    let bundle = towermd_core::baseline_draft::from_current_baseline(
        &fresh,
        &f.outbox.read_inputs().unwrap(),
        &id,
        "towermd/B0",
        &advanced,
    )
    .unwrap();
    assert_eq!(bundle.source_revision, advanced);
}

#[test]
fn saved_current_draft_preserves_raw_bytes_and_reopens_after_edit_without_publishing() {
    use towermd_core::{
        baseline_draft::DraftEdit,
        baseline_review::{review_view, ReviewAction},
        recovery_contract::recorded::RequestBody,
        request_ledger::{
            save_draft_shared, DraftSaveAction, DraftSaveValidity, PreparedDraftSaveRequest,
        },
        snapshot_service::read_draft,
    };
    let mut f = published();
    let before_heads = git(
        f.root.path(),
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads/",
            "refs/tags/",
        ],
    );
    let before_status = git(f.root.path(), &["status", "--porcelain"]);
    let index = git(
        f.root.path(),
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    let before_index = std::fs::read(&index).unwrap();
    let request_id = uuid::Uuid::now_v7().to_string();
    let request = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: f.catalog.config.project_id().into(),
            actor: "human:owner".into(),
            host: "host".into(),
            run: None,
            action: DraftSaveAction::CreateDraft {
                baseline: "towermd/B0".into(),
                main_revision: f.catalog.revision.clone(),
            },
        },
        &request_id,
    )
    .unwrap();
    assert_eq!(request.draft_id(), request_id);
    assert_eq!(request.expected_revision(), None);
    for (baseline, main_revision) in [
        ("towermd/B1", f.catalog.revision.as_str()),
        ("towermd/B0", f.frozen.bundle.source_revision.as_str()),
    ] {
        let mut bad_body = request.body().clone();
        bad_body.action = DraftSaveAction::CreateDraft {
            baseline: baseline.into(),
            main_revision: main_revision.into(),
        };
        let bad =
            PreparedDraftSaveRequest::prepare(bad_body, &uuid::Uuid::now_v7().to_string()).unwrap();
        let error = save_draft_shared(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &f.writer,
            &bad,
            || Ok(NOW + 7),
        )
        .unwrap_err();
        assert!(error.contains("conflict"), "{error}");
        assert!(read_draft(&f.catalog, bad.draft_id()).unwrap().is_none());
    }
    let saved = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 8),
    )
    .unwrap();
    assert_eq!(saved.current_validity, DraftSaveValidity::Current);
    let first = read_draft(&f.catalog, &request_id).unwrap().unwrap();
    assert_eq!(first.frozen().bundle.target_baseline, "towermd/B1");
    assert_eq!(first.frozen().bundle.source_revision, f.catalog.revision);
    for object in first.frozen().bundle.objects.values() {
        assert_eq!(
            object.text,
            f.catalog.read_file(&object.path).unwrap().unwrap()
        );
    }
    assert_eq!(first.frozen().bundle.objects["I-existing"].text,
        "---\r\ntype: Intent\r\nid: I-existing\r\n# original comment\r\nfuture: {values: [2, 1]}\r\n---\r\noriginal body without final newline");
    assert_eq!(
        first.frozen().bundle.canvas_text,
        f.catalog
            .read_file("docs/tower/tower.canvas")
            .unwrap()
            .unwrap()
    );
    assert!(first
        .frozen()
        .files
        .keys()
        .all(|path| !path.contains("/baselines/") && !path.contains("/decisions/")));
    let repeated = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 9),
    )
    .unwrap();
    assert_eq!(saved.receipt, repeated.receipt);
    let mut changed = request.body().clone();
    changed.action = DraftSaveAction::CreateDraft {
        baseline: "towermd/B1".into(),
        main_revision: f.catalog.revision.clone(),
    };
    let changed = PreparedDraftSaveRequest::prepare(changed, &request_id).unwrap();
    assert!(save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &changed,
        || Ok(NOW + 9)
    )
    .is_err());
    assert_eq!(
        first.revision(),
        read_draft(&f.catalog, &request_id)
            .unwrap()
            .unwrap()
            .revision()
    );
    let old_b0 = read_draft(&f.catalog, &f.frozen.bundle.id)
        .unwrap()
        .unwrap();
    assert_eq!(old_b0.frozen(), &f.frozen);

    let edit_id = uuid::Uuid::now_v7().to_string();
    let edit = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: f.catalog.config.project_id().into(),
            actor: "human:owner".into(),
            host: "host".into(),
            run: None,
            action: DraftSaveAction::EditDraft {
                id: request_id.clone(),
                expected_revision: first.revision().into(),
                edit: DraftEdit::Rename {
                    object: "I-existing".into(),
                    title: "updated intent".into(),
                },
            },
        },
        &edit_id,
    )
    .unwrap();
    let edited = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &edit,
        || Ok(NOW + 10),
    )
    .unwrap();
    assert_ne!(
        saved.receipt.result_ref.revision,
        edited.receipt.result_ref.revision
    );
    let latest = read_draft(&f.catalog, &request_id).unwrap().unwrap();
    assert!(latest.frozen().bundle.objects["I-existing"]
        .text
        .contains("updated intent"));
    drop(f.session);
    f.session = std::sync::Arc::new(std::sync::Mutex::new(
        towermd_core::project_session::ProjectSession::open(
            &f.catalog.common_dir.join("towermd"),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            NOW + 10,
        )
        .unwrap(),
    ));
    let view = review_view(&f.catalog, &f.outbox.read_inputs().unwrap(), &request_id)
        .unwrap()
        .unwrap();
    assert!(view
        .publish_blockers
        .iter()
        .any(|blocker| blocker.contains("提案")));
    let approval = ReviewAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding.clone(),
        verdict: Verdict::Approved,
        comment: "cannot approve unfinished Bn flow".into(),
    };
    assert!(review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &approval,
        "human:owner",
        "host",
        NOW + 11
    )
    .unwrap_err()
    .contains("提案"));
    let rejected = ReviewAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        verdict: Verdict::Rejected,
        ..approval
    };
    review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &rejected,
        "human:owner",
        "host",
        NOW + 12,
    )
    .unwrap();
    let old_receipt = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &request,
        || Ok(NOW + 13),
    )
    .unwrap();
    assert_eq!(saved.receipt, old_receipt.receipt);
    assert!(matches!(
        old_receipt.current_validity,
        DraftSaveValidity::HistoricalOnly { .. }
    ));
    // A new/invalid target must not reinterpret existing private bytes or the
    // original receipt. It only makes the current target unusable.
    git(f.root.path(), &["tag", "towermd/B1", "main"]);
    assert_eq!(
        read_draft(&f.catalog, &f.frozen.bundle.id)
            .unwrap()
            .unwrap()
            .frozen(),
        &f.frozen
    );
    assert_eq!(
        read_draft(&f.catalog, &request_id)
            .unwrap()
            .unwrap()
            .frozen(),
        latest.frozen()
    );
    let historical = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &edit,
        || Ok(NOW + 14),
    )
    .unwrap();
    assert_eq!(historical.receipt, edited.receipt);
    assert!(matches!(
        historical.current_validity,
        DraftSaveValidity::HistoricalOnly { .. }
    ));
    git(f.root.path(), &["tag", "-d", "towermd/B1"]);
    assert_eq!(
        before_heads,
        git(
            f.root.path(),
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads/",
                "refs/tags/"
            ]
        )
    );
    assert_eq!(
        before_status,
        git(f.root.path(), &["status", "--porcelain"])
    );
    assert_eq!(before_index, std::fs::read(index).unwrap());
}

#[test]
fn create_request_is_human_only_and_preserves_old_wire_shape() {
    use towermd_core::{
        recovery_contract::recorded::RequestBody,
        request_ledger::{DraftSaveAction, PreparedDraftSaveRequest},
    };
    let id = uuid::Uuid::now_v7().to_string();
    let body = RequestBody {
        project_id: "p".into(),
        actor: "human:owner".into(),
        host: "host".into(),
        run: None,
        action: DraftSaveAction::CreateDraft {
            baseline: "towermd/B0".into(),
            main_revision: "a".repeat(40),
        },
    };
    let request = PreparedDraftSaveRequest::prepare(body.clone(), &id).unwrap();
    assert_eq!(request.draft_id(), id);
    assert_eq!(
        serde_json::to_value(request.body()).unwrap()["action"],
        serde_json::json!({
            "kind":"create_draft", "baseline":"towermd/B0", "main_revision":"a".repeat(40)
        })
    );
    for actor in ["process:towermd-cli", "codex/1", "human:"] {
        let mut bad = body.clone();
        bad.actor = actor.into();
        assert!(PreparedDraftSaveRequest::prepare(bad, &id).is_err());
    }
    let mut bad = body.clone();
    bad.run = Some("R".into());
    assert!(PreparedDraftSaveRequest::prepare(bad, &id).is_err());
    let mut bad = body.clone();
    bad.action = DraftSaveAction::CreateDraft {
        baseline: "towermd/B01".into(),
        main_revision: "a".repeat(40),
    };
    assert!(PreparedDraftSaveRequest::prepare(bad, &id).is_err());
    assert!(PreparedDraftSaveRequest::prepare(body, "not-an-original-uuid").is_err());
    let old = serde_json::json!({"kind":"edit_draft","id":"D","expected_revision":"b".repeat(40),"edit":{"action":"rename","object":"A","title":"title"}});
    let parsed: DraftSaveAction = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), old);
}

#[test]
fn no_approved_baseline_never_opens_a_current_draft_from_private_s6_ref() {
    let f = fixture();
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(current_baseline(&f.catalog, &inputs).unwrap().is_none());
    assert!(towermd_core::baseline_draft::from_current_baseline(
        &f.catalog,
        &inputs,
        &uuid::Uuid::now_v7().to_string(),
        "towermd/B0",
        &f.catalog.revision
    )
    .unwrap_err()
    .contains("no approved"));
    git(f.root.path(), &["tag", "towermd/B0", "main"]);
    assert!(current_baseline(&f.catalog, &inputs).is_err());
}
