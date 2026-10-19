//! Real B0 publication, Add/Claim, immutable Git inputs and the production writer.
//! Faults stop the actual producer; no accepted Proposal or Run is fabricated.
use super::*;
use crate::{
    ask_fixture::{self as fixture, git, Fixture, NOW},
    hub_protocol::Event,
    proposals::{Alternative, ProposalSource},
    request_ledger::ReviewValidity,
    tasks::TaskCommand,
};
use std::{cell::Cell, fs, sync::Arc};

thread_local! {
    static STOP_AFTER_SAVE: Cell<bool> = const { Cell::new(false) };
}
pub(super) fn stop_after_materialization() -> bool {
    STOP_AFTER_SAVE.with(|stop| stop.replace(false))
}
struct Stop;
impl Stop {
    fn arm() -> Self {
        STOP_AFTER_SAVE.with(|stop| assert!(!stop.replace(true)));
        Self
    }
}
impl Drop for Stop {
    fn drop(&mut self) {
        STOP_AFTER_SAVE.with(|stop| stop.set(false));
    }
}
fn proposed() -> (Fixture, ProposalInput) {
    let mut f = fixture::claimed();
    let base = f.catalog.revision.clone();
    let path = f.catalog.object_paths["A-alpha"].clone();
    let source = fs::read_to_string(f.root.path().join(&path)).unwrap();
    fs::write(
        f.root.path().join(&path),
        format!("{source}\nactual proposed module body\n"),
    )
    .unwrap();
    git(f.root.path(), &["add", &path]);
    git(
        f.root.path(),
        &[
            "commit",
            "-qm",
            "actual protected proposal source, never main",
        ],
    );
    let revision = git(f.root.path(), &["rev-parse", "HEAD"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let input = ProposalInput {
        source: ProposalSource::Branch {
            revision,
            base_revision: base,
        },
        reason: "调整模块正文".into(),
        affected_intents: f
            .catalog
            .objects
            .iter()
            .filter(|(_, doc)| doc.doc_type() == Some("Intent"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_modules: vec!["A-alpha".into()],
        affected_interfaces: f
            .catalog
            .objects
            .iter()
            .filter(|(_, doc)| doc.doc_type() == Some("Interface"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_tasks: vec!["T-ask".into()],
        affected_paths: vec!["alpha/**".into(), "docs/tower/**".into()],
        alternatives: vec![Alternative {
            name: "保留".into(),
            consequences: "保留原正文".into(),
        }],
        recommendation: "采用明确模块说明".into(),
    };
    (f, input)
}
fn call(
    f: &mut Fixture,
    input: &ProposalInput,
    id: &str,
    now: u64,
) -> Result<ProposalOutcome, String> {
    f.writer.save_proposal(
        &f.session,
        &f.catalog,
        input,
        None,
        id,
        &mut f.outbox,
        "host",
        || Ok(now),
    )
}
fn read_saved(f: &Fixture, id: &str) -> crate::snapshot_service::SavedDraft {
    read_draft(&f.catalog, id).unwrap().unwrap()
}
fn watermark(f: &Fixture) -> u64 {
    f.session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq()
}
fn tail(f: &Fixture, after: u64, now: u64) -> Vec<Event> {
    f.session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(after, now)
        .unwrap()
        .events
}
fn original(f: &Fixture, id: &str) -> Event {
    let eid = request_event_eid(id, "proposal.created").unwrap();
    f.outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| event.eid == eid)
        .unwrap()
        .clone()
}
fn stop_after_real_save(
    f: &mut Fixture,
    input: &ProposalInput,
    id: &str,
) -> crate::snapshot_service::SavedDraft {
    let _stop = Stop::arm();
    let error = call(f, input, id, NOW + 20).unwrap_err();
    assert!(
        error.contains(id) && error.contains("before proposal pending"),
        "{error}"
    );
    let saved = read_saved(f, id);
    assert!(validate_proposal_subject(saved.frozen())
        .unwrap()
        .unwrap()
        .binding
        .is_some());
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(!inputs
        .pending()
        .iter()
        .any(|event| event.kind == "proposal.created"));
    assert!(!inputs
        .confirmed()
        .iter()
        .any(|event| event.kind == "proposal.created"));
    assert!(ProjectSession::read_proposal_shared(
        &f.session,
        &f.catalog,
        &inputs,
        id,
        saved.revision(),
        NOW + 20
    )
    .unwrap()
    .is_none());
    saved
}
fn checkpoint(f: &mut Fixture, now: u64) {
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(now),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
}
fn rebuild(mut f: Fixture, now: u64) -> Fixture {
    drop(f.session);
    fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let prepared = crate::recovery::prepare(
        &f.catalog,
        &inputs,
        now,
        now + 300_000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (report, session) = f
        .writer
        .confirm_rebuild(
            None,
            &f.catalog,
            &prepared,
            "human:owner",
            prepared.expires_at,
            &inputs,
            "host",
            || Ok(now + 1),
        )
        .unwrap();
    assert_eq!(report.status, "rebuilt");
    f.session = session;
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    f
}

#[test]
fn actual_propose_is_durable_source_bound_and_same_uuid_never_writes_draft_saved_or_approval() {
    let (mut f, input) = proposed();
    let main = git(f.root.path(), &["rev-parse", "main"]);
    let tags = git(f.root.path(), &["tag", "--list"]);
    let id = uuid::Uuid::now_v7().to_string();
    let result = call(&mut f, &input, &id, NOW + 20).unwrap();
    assert!(matches!(result.current_validity, ReviewValidity::Current));
    let saved = read_saved(&f, &id);
    assert_eq!(
        saved.request().body().action,
        DraftSaveAction::Propose {
            input: input.clone()
        }
    );
    assert_eq!(saved.revision(), result.receipt.result_ref.draft.revision);
    assert_eq!(
        saved.frozen().content_hash,
        result.receipt.result_ref.draft.content_hash
    );
    let accepted = original(&f, &id);
    assert!(!accepted.stale && !accepted.legacy);
    assert_eq!(accepted.guards, result.receipt.result_ref.binding.fences);
    assert_eq!(accepted.guards.len(), 1);
    assert_eq!(accepted.guards[0].name, "module:A-alpha");
    assert_eq!(accepted.actor, "process:towermd-cli");
    assert_eq!(accepted.layer, "L4");
    assert_eq!(accepted.obj, id);
    assert_eq!(
        accepted.data["request_body"]["action"]["input"],
        serde_json::to_value(&input).unwrap()
    );
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.receipt_confirmed(&accepted));
    // A genuine confirmed Event still cannot stand in for live actual outer
    // scheduling evidence when a caller only has the Git source reader.
    let source_only = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    let source_ledger = crate::request_ledger::BaselineRequestLedger::from_history(
        &f.catalog,
        &inputs,
        &source_only,
    )
    .unwrap();
    assert!(source_ledger
        .accepted_proposal(&id, saved.revision())
        .unwrap()
        .is_none());
    assert_eq!(
        ProjectSession::read_proposal_shared(
            &f.session,
            &f.catalog,
            &inputs,
            &id,
            saved.revision(),
            NOW + 21
        )
        .unwrap(),
        Some(accepted.clone())
    );
    assert!(!inputs.confirmed().iter().any(|event| event.obj == id
        && matches!(
            event.kind.as_str(),
            "draft.saved" | "baseline.approved" | "baseline.created"
        )));
    let before = watermark(&f);
    let retry = call(&mut f, &input, &id, NOW + 22).unwrap();
    assert_eq!(retry.receipt, result.receipt);
    assert!(tail(&f, before, NOW + 22)
        .iter()
        .all(|event| event.kind != "proposal.created" && event.kind != "task.claimed"));
    assert_eq!(read_saved(&f, &id).revision(), saved.revision());
    assert_eq!(git(f.root.path(), &["rev-parse", "main"]), main);
    assert_eq!(git(f.root.path(), &["tag", "--list"]), tags);
    let mut changed = input.clone();
    changed.reason.push_str(" changed");
    assert!(call(&mut f, &changed, &id, NOW + 23).is_err());
    let review =
        crate::baseline_review::review_view(&f.catalog, &f.outbox.read_inputs().unwrap(), &id)
            .unwrap()
            .unwrap();
    assert!(review
        .publish_blockers
        .iter()
        .any(|reason| reason.contains("后续基线")));
}

#[test]
fn actual_saved_ref_before_pending_reopens_same_original_and_never_itself_proves_acceptance() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    let before = stop_after_real_save(&mut f, &input, &id);
    let revision = before.revision().to_owned();
    let frozen = before.frozen().clone();
    let binding = validate_proposal_subject(before.frozen())
        .unwrap()
        .unwrap()
        .binding
        .unwrap();
    f = fixture::reopen(f, NOW + 21);
    let result = call(&mut f, &input, &id, NOW + 22).unwrap();
    assert_eq!(result.receipt.result_ref.binding, binding);
    assert_eq!(result.receipt.result_ref.draft.revision, revision);
    assert_eq!(read_saved(&f, &id).frozen(), &frozen);
    assert!(matches!(result.current_validity, ReviewValidity::Current));
    assert_eq!(original(&f, &id).guards, binding.fences);
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "proposal.created")
            .count(),
        1
    );
}

#[test]
fn pending_fsync_never_calls_hub_and_confirm_failure_repairs_only_original_actual_event() {
    for confirmed in [false, true] {
        let (mut f, input) = proposed();
        let id = uuid::Uuid::now_v7().to_string();
        // Isolate the ordinary append boundary after the real writer and its
        // lease-only preparation. A new proposal normally renews its Run first.
        let materialized = stop_after_real_save(&mut f, &input, &id);
        if confirmed {
            f.outbox.fail_next_confirmation();
        } else {
            f.outbox.fail_next_pending();
        }
        let before = watermark(&f);
        let error = call(&mut f, &input, &id, NOW + 20).unwrap_err();
        assert!(error.contains(&id), "{error}");
        assert!(error.contains("uncertain"), "{error}");
        let inputs = f.outbox.read_inputs().unwrap();
        let eid = request_event_eid(&id, "proposal.created").unwrap();
        let actual = tail(&f, before, NOW + 20)
            .into_iter()
            .filter(|event| event.kind == "proposal.created")
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), if confirmed { 1 } else { 0 });
        let pending = if confirmed {
            // A complete confirmed row exists, but its fsync is uncertain. Its
            // exact original is no longer in the physical pending-only set.
            let receipt = inputs
                .confirmed()
                .iter()
                .find(|event| event.eid == eid)
                .unwrap();
            assert_eq!(receipt, &actual[0]);
            assert!(!inputs.pending().iter().any(|event| event.eid == eid));
            assert!(!inputs.receipt_confirmed(receipt));
            assert!(ProjectSession::read_proposal_shared(
                &f.session,
                &f.catalog,
                &inputs,
                &id,
                read_saved(&f, &id).revision(),
                NOW + 20
            )
            .unwrap()
            .is_some());
            crate::recovery_outbox::original_of(receipt)
        } else {
            assert_eq!(
                watermark(&f),
                before,
                "pending fsync failure must not call Hub"
            );
            assert!(tail(&f, before, NOW + 20).is_empty());
            assert!(!inputs.confirmed().iter().any(|event| event.eid == eid));
            inputs
                .pending()
                .iter()
                .find(|event| event.eid == eid)
                .unwrap()
                .clone()
        };
        assert_eq!(pending.eid, eid);
        let saved = read_saved(&f, &id);
        assert_eq!(saved.revision(), materialized.revision());
        assert_eq!(saved.frozen(), materialized.frozen());
        let revision = saved.revision().to_owned();
        let frozen = saved.frozen().clone();
        let before_retry = watermark(&f);
        let result = call(&mut f, &input, &id, NOW + 21).unwrap();
        let receipt = original(&f, &id);
        assert_eq!(crate::recovery_outbox::original_of(&receipt), pending);
        let repaired = f.outbox.read_inputs().unwrap();
        assert!(repaired.receipt_confirmed(&receipt));
        assert!(!repaired.pending().iter().any(|event| event.eid == eid));
        assert_eq!(result.receipt.result_ref.draft.revision, revision);
        assert_eq!(read_saved(&f, &id).revision(), revision);
        assert_eq!(read_saved(&f, &id).frozen(), &frozen);
        let retry_events = tail(&f, before_retry, NOW + 21);
        assert_eq!(
            retry_events
                .iter()
                .filter(|event| event.kind == "proposal.created")
                .count(),
            if confirmed { 0 } else { 1 }
        );
        assert!(retry_events
            .iter()
            .all(|event| !matches!(event.kind.as_str(), "lease.granted" | "task.claimed")));
        if confirmed {
            assert_eq!(receipt, actual[0]);
            assert_eq!(watermark(&f), before_retry);
            assert!(
                retry_events.is_empty(),
                "confirmation repair must not append to Hub"
            );
        }
    }
}

#[test]
fn saved_intent_expiry_or_replacement_run_cannot_substitute_new_fences_or_return_success() {
    for replace in [false, true] {
        let (mut f, input) = proposed();
        let id = uuid::Uuid::now_v7().to_string();
        let before = stop_after_real_save(&mut f, &input, &id);
        let old_binding = validate_proposal_subject(before.frozen())
            .unwrap()
            .unwrap()
            .binding
            .unwrap();
        let when = if replace {
            ProjectSession::execute_shared(
                &f.session,
                &f.catalog,
                &f.outbox.read_inputs().unwrap(),
                "host",
                TaskCommand::Release,
                None,
                NOW + 21,
                &mut f.outbox,
            )
            .unwrap();
            fixture::claim(&mut f, NOW + 22);
            NOW + 23
        } else {
            NOW + 1_000_000
        };
        let after_claim = watermark(&f);
        let error = call(&mut f, &input, &id, when).unwrap_err();
        assert!(error.contains(&id), "{error}");
        assert_eq!(read_saved(&f, &id).revision(), before.revision());
        assert_eq!(read_saved(&f, &id).frozen(), before.frozen());
        let stale = original(&f, &id);
        assert!(stale.stale);
        assert_eq!(stale.guards, old_binding.fences);
        assert!(ProjectSession::read_proposal_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            &id,
            before.revision(),
            when
        )
        .unwrap()
        .is_none());
        assert!(tail(&f, after_claim, when)
            .iter()
            .all(|event| event.kind != "task.claimed"));
        let after = watermark(&f);
        assert!(call(&mut f, &input, &id, when + 1).is_err());
        assert!(tail(&f, after, when + 1)
            .iter()
            .all(|event| event.kind != "proposal.created" && event.kind != "task.claimed"));
    }
}

#[test]
fn legacy_or_changed_carrier_cannot_be_an_accepted_record_or_enter_generic_draft_saved() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let actual = original(&f, &id);
    let before = f.outbox.read_inputs().unwrap();
    let record: ProposalCreated = serde_json::from_value(actual.data.clone()).unwrap();
    for missing in [false, true] {
        let mut changed = record.clone();
        if missing {
            changed.result.carrier.binding = None;
        } else {
            changed.result.carrier.input.reason.push_str(" changed");
        }
        changed.p3_receipt.result_ref = changed.result.clone();
        let mut original = crate::recovery_outbox::original_of(&actual);
        original.data = serde_json::to_value(changed).unwrap();
        assert!(
            crate::proposals::decode_created(f.catalog.config.project_id(), &original).is_err()
        );
    }
    let typed = read_saved(&f, &id).request().clone();
    let mut human = typed.body().clone();
    human.actor = "human:owner".into();
    assert!(PreparedDraftSaveRequest::prepare(human, &uuid::Uuid::now_v7().to_string()).is_err());
    assert!(crate::request_ledger::save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &typed,
        || Ok(NOW + 21)
    )
    .is_err());
    assert!(f
        .writer
        .save_draft(
            &f.session,
            &f.catalog,
            read_saved(&f, &id).frozen(),
            &typed,
            || Ok(NOW + 21)
        )
        .is_err());
    before
        .validate_task_writes(&f.outbox.read_inputs().unwrap(), &[])
        .unwrap();
}

#[test]
fn new_uuid_reanalyzes_actual_task_population_instead_of_reusing_old_impact() {
    let (mut f, input) = proposed();
    let first = uuid::Uuid::now_v7().to_string();
    let original = call(&mut f, &input, &first, NOW + 20).unwrap();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: crate::tasks::TaskSpec {
                id: "T-later".into(),
                title: "new actual impacted Task".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-ask".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 21,
        &mut f.outbox,
    )
    .unwrap();
    assert_eq!(
        call(&mut f, &input, &first, NOW + 22).unwrap().receipt,
        original.receipt
    );
    let missing = uuid::Uuid::now_v7().to_string();
    assert!(call(&mut f, &input, &missing, NOW + 23).is_err());
    assert!(read_draft(&f.catalog, &missing).unwrap().is_none());
    let mut fresh = input.clone();
    fresh.affected_tasks.push("T-later".into());
    let id = uuid::Uuid::now_v7().to_string();
    let result = call(&mut f, &fresh, &id, NOW + 24).unwrap();
    assert!(result
        .receipt
        .result_ref
        .carrier
        .impact
        .tasks
        .contains(&"T-later".into()));
    assert_ne!(
        result.receipt.result_ref.draft.draft_id,
        original.receipt.result_ref.draft.draft_id
    );
}

#[test]
fn original_accepted_receipt_survives_actual_rebuild_and_second_loss_without_restoring_authority() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    let result = call(&mut f, &input, &id, NOW + 20).unwrap();
    let accepted = original(&f, &id);
    checkpoint(&mut f, NOW + 30);
    f = rebuild(f, NOW + 40);
    let retry = call(&mut f, &input, &id, NOW + 42).unwrap();
    assert_eq!(retry.receipt, result.receipt);
    assert!(matches!(
        retry.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    assert_eq!(original(&f, &id), accepted);
    checkpoint(&mut f, NOW + 50);
    let directory = f.runtime.path().join("outbox");
    drop(f.outbox);
    fs::remove_dir_all(&directory).unwrap();
    f.outbox = RecoveryOutbox::open(
        &directory,
        crate::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
    )
    .unwrap();
    f = rebuild(f, NOW + 60);
    let before = watermark(&f);
    let retry = call(&mut f, &input, &id, NOW + 62).unwrap();
    assert_eq!(retry.receipt, result.receipt);
    assert!(matches!(
        retry.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    assert_eq!(original(&f, &id), accepted);
    assert!(tail(&f, before, NOW + 62)
        .iter()
        .all(|event| event.kind != "proposal.created" && event.kind != "task.claimed"));
    assert!(ProjectSession::read_proposal_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &id,
        &result.receipt.result_ref.draft.revision,
        NOW + 62
    )
    .unwrap()
    .is_none());
}

#[test]
fn actual_source_change_is_hard_refusal_after_saved_intent_without_rewriting_ref_or_request() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    let before = stop_after_real_save(&mut f, &input, &id);
    let main = git(f.root.path(), &["rev-parse", "main"]);
    let tree = git(f.root.path(), &["rev-parse", "main^{tree}"]);
    let next = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &main,
            "-m",
            "actual source changes after save",
        ],
    );
    git(
        f.root.path(),
        &["update-ref", "refs/heads/main", &next, &main],
    );
    assert!(call(&mut f, &input, &id, NOW + 21).is_err());
    assert_eq!(
        git(
            f.root.path(),
            &["rev-parse", &format!("refs/towermd/drafts/{id}")]
        ),
        before.revision()
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "proposal.created"));
}

#[test]
fn actual_hub_receipt_with_real_run_fences_cannot_substitute_an_unrelated_task() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    let saved = stop_after_real_save(&mut f, &input, &id);
    let mut carrier = validate_proposal_subject(saved.frozen()).unwrap().unwrap();
    let mut binding = carrier.binding.clone().unwrap();
    binding.task = "T-not-the-original-task".into();
    carrier.binding = Some(binding.clone());
    let request = saved.request().clone();
    let result = ProposalResult {
        draft: crate::request_ledger::DraftSavedResult {
            draft_id: id.clone(),
            revision: saved.revision().into(),
            reference: format!("refs/towermd/drafts/{id}"),
            content_hash: saved.frozen().content_hash.clone(),
            delivery: crate::request_ledger::DraftDelivery::LocalSaved,
        },
        binding: binding.clone(),
        carrier,
    };
    let record = ProposalCreated {
        result: result.clone(),
        request_body: request.body().clone(),
        p3_request: request.metadata().clone(),
        p3_receipt: RequestReceipt {
            request_id: id.clone(),
            body_hash: request.metadata().body_hash.clone(),
            result_ref: result,
        },
    };
    let invalid = NewEvent {
        eid: request_event_eid(&id, "proposal.created").unwrap(),
        host: "host".into(),
        actor: "process:towermd-cli".into(),
        kind: "proposal.created".into(),
        layer: "L4".into(),
        obj: id.clone(),
        guards: binding.fences,
        data: serde_json::to_value(record).unwrap(),
    };
    let before = f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .clone();
    // Negative control: the ordinary Hub only sequences the complete guarded
    // input. Its acceptance cannot replace the canonical Proposal provenance.
    let receipt = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(invalid, NOW + 21)
        .unwrap();
    assert!(
        matches!(receipt, crate::hub_protocol::AppendReceipt::Event(ref actual) if !actual.stale)
    );
    let error = ProjectSession::sync_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        NOW + 22,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("preceding actual Task")
            || error.to_string().contains("actual preceding Task/Run"),
        "{error}"
    );
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects,
        before
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "proposal.created"));
}

fn event_for_saved(saved: &crate::snapshot_service::SavedDraft) -> NewEvent {
    let request = saved.request();
    let id = request.draft_id();
    let carrier = validate_proposal_subject(saved.frozen()).unwrap().unwrap();
    let binding = carrier.binding.clone().unwrap();
    let result = ProposalResult {
        draft: crate::request_ledger::DraftSavedResult {
            draft_id: id.into(),
            revision: saved.revision().into(),
            reference: format!("refs/towermd/drafts/{id}"),
            content_hash: saved.frozen().content_hash.clone(),
            delivery: crate::request_ledger::DraftDelivery::LocalSaved,
        },
        binding: binding.clone(),
        carrier,
    };
    let record = ProposalCreated {
        result: result.clone(),
        request_body: request.body().clone(),
        p3_request: request.metadata().clone(),
        p3_receipt: RequestReceipt {
            request_id: id.into(),
            body_hash: request.metadata().body_hash.clone(),
            result_ref: result,
        },
    };
    NewEvent {
        eid: request_event_eid(id, "proposal.created").unwrap(),
        host: "host".into(),
        actor: "process:towermd-cli".into(),
        kind: "proposal.created".into(),
        layer: "L4".into(),
        obj: id.into(),
        guards: binding.fences,
        data: serde_json::to_value(record).unwrap(),
    }
}

#[test]
fn actual_guarded_proposal_cannot_replace_saved_impact_revision_or_hashes() {
    for field in ["impact", "revision", "content_hash", "payload_hash"] {
        let (mut f, input) = proposed();
        let id = uuid::Uuid::now_v7().to_string();
        let saved = stop_after_real_save(&mut f, &input, &id);
        let mut event = event_for_saved(&saved);
        match field {
            "impact" => event.data["result"]["carrier"]["impact"]["tasks"] = serde_json::json!([]),
            "revision" => {
                event.data["result"]["draft"]["revision"] =
                    serde_json::json!(input_source_revision(&input))
            }
            "content_hash" => {
                event.data["result"]["draft"]["content_hash"] = serde_json::json!("0".repeat(64))
            }
            "payload_hash" => {
                event.data["result"]["carrier"]["payload_hash"] = serde_json::json!("0".repeat(64))
            }
            _ => unreachable!(),
        }
        event.data["p3_receipt"]["result_ref"] = event.data["result"].clone();
        crate::proposals::decode_created(f.catalog.config.project_id(), &event).unwrap();
        let before = f
            .session
            .lock()
            .unwrap()
            .projection()
            .unwrap()
            .objects
            .clone();
        let receipt = f
            .session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .append(event, NOW + 21)
            .unwrap();
        assert!(
            matches!(receipt, crate::hub_protocol::AppendReceipt::Event(ref actual) if !actual.stale)
        );
        let error = ProjectSession::sync_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            NOW + 22,
        )
        .unwrap_err();
        assert!(!error.to_string().is_empty(), "{field}");
        assert_eq!(
            f.session.lock().unwrap().projection().unwrap().objects,
            before,
            "{field}"
        );
        assert!(
            ProjectSession::read_proposal_shared(
                &f.session,
                &f.catalog,
                &f.outbox.read_inputs().unwrap(),
                &id,
                saved.revision(),
                NOW + 22
            )
            .is_err(),
            "{field}"
        );
        assert!(!f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .any(|event| event.kind == "proposal.created"));
    }
}
fn input_source_revision(input: &ProposalInput) -> &str {
    match &input.source {
        ProposalSource::Branch { revision, .. } | ProposalSource::Draft { revision, .. } => {
            revision
        }
    }
}

#[test]
fn actual_saved_carrier_must_match_recomputed_preceding_task_impact() {
    let (mut f, input) = proposed();
    let original_id = uuid::Uuid::now_v7().to_string();
    let saved = stop_after_real_save(&mut f, &input, &original_id);
    let binding = validate_proposal_subject(saved.frozen())
        .unwrap()
        .unwrap()
        .binding
        .unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let inputs = f.outbox.read_inputs().unwrap();
    let context = ProjectSession::prepare_proposal_shared(
        &f.session,
        &f.catalog,
        &inputs,
        "host",
        Some(&binding.run),
        NOW + 21,
        false,
    )
    .unwrap();
    let analysis = analyze_proposal(&f.catalog, &inputs, &input, &context.tasks, &id).unwrap();
    assert!(analysis.impact.tasks.contains(&"T-ask".to_string()));
    let mut frozen =
        crate::proposals::attach_bound_proposal(&f.catalog, &analysis, &input, &binding).unwrap();
    let mut carrier = validate_proposal_subject(&frozen).unwrap().unwrap();
    carrier.impact.tasks.clear();
    let object = frozen.bundle.objects.get_mut(&id).unwrap();
    let mut doc = crate::okf::Document::parse(&object.text).unwrap();
    doc.set("impact", serde_yaml::to_value(&carrier.impact).unwrap())
        .unwrap();
    doc.set("proposal", serde_yaml::to_value(&carrier).unwrap())
        .unwrap();
    object.text = doc.to_text();
    frozen = crate::baseline_draft::freeze(&f.catalog, &frozen.bundle).unwrap();
    let request = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: f.catalog.config.project_id().into(),
            actor: "process:towermd-cli".into(),
            host: "host".into(),
            run: Some(binding.run.clone()),
            action: DraftSaveAction::Propose {
                input: input.clone(),
            },
        },
        &id,
    )
    .unwrap();
    {
        // Negative control uses the real sole writer to save a deliberately
        // wrong analysis. A genuine manifest/ref still cannot authorize it.
        let writing = f.writer.writing.try_lock().unwrap();
        f.writer
            .save_draft_locked(&writing, &f.session, &f.catalog, &frozen, &request, || {
                Ok(NOW + 22)
            })
            .unwrap();
    }
    let wrong_saved = read_saved(&f, &id);
    let event = event_for_saved(&wrong_saved);
    let record = crate::proposals::decode_created(f.catalog.config.project_id(), &event).unwrap();
    crate::proposals::require_saved(&record, &wrong_saved).unwrap();
    let before = f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .clone();
    let actual = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(event, NOW + 23)
        .unwrap();
    assert!(matches!(actual, crate::hub_protocol::AppendReceipt::Event(ref event) if !event.stale));
    let error = ProjectSession::sync_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        NOW + 24,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("original actual analysis"),
        "{error}"
    );
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects,
        before
    );
    assert!(ProjectSession::read_proposal_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &id,
        wrong_saved.revision(),
        NOW + 24
    )
    .is_err());
}

#[test]
fn accepted_history_uses_original_immutable_draft_after_ref_deleted_and_tasks_change() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    let result = call(&mut f, &input, &id, NOW + 20).unwrap();
    let accepted = original(&f, &id);
    let saved = read_saved(&f, &id);
    git(
        f.root.path(),
        &[
            "update-ref",
            "-d",
            &format!("refs/towermd/drafts/{id}"),
            saved.revision(),
        ],
    );
    assert!(read_draft(&f.catalog, &id).unwrap().is_none());
    // The current Task/Run may be released; the old accepted analysis is still
    // checked with its actual population before proposal.created, never today.
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 21,
        &mut f.outbox,
    )
    .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        ProjectSession::read_proposal_shared(
            &f.session,
            &f.catalog,
            &inputs,
            &id,
            saved.revision(),
            NOW + 22
        )
        .unwrap(),
        Some(accepted.clone())
    );
    let retry = call(&mut f, &input, &id, NOW + 23).unwrap();
    assert_eq!(retry.receipt, result.receipt);
    assert!(matches!(
        retry.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    assert_eq!(original(&f, &id), accepted);
    assert!(read_draft(&f.catalog, &id).unwrap().is_none());
}

#[test]
fn historical_draft_source_does_not_follow_a_deleted_mutable_source_ref() {
    let (mut f, mut input) = proposed();
    let source_id = uuid::Uuid::now_v7().to_string();
    let source = stop_after_real_save(&mut f, &input, &source_id);
    input.source = ProposalSource::Draft {
        id: source_id.clone(),
        revision: source.revision().into(),
    };
    let id = uuid::Uuid::now_v7().to_string();
    let result = call(&mut f, &input, &id, NOW + 21).unwrap();
    let accepted = original(&f, &id);
    git(
        f.root.path(),
        &[
            "update-ref",
            "-d",
            &format!("refs/towermd/drafts/{source_id}"),
            source.revision(),
        ],
    );
    git(
        f.root.path(),
        &[
            "update-ref",
            "-d",
            &format!("refs/towermd/drafts/{id}"),
            &result.receipt.result_ref.draft.revision,
        ],
    );
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 22,
        &mut f.outbox,
    )
    .unwrap();
    let retry = call(&mut f, &input, &id, NOW + 23).unwrap();
    assert_eq!(retry.receipt, result.receipt);
    assert!(matches!(
        retry.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    assert_eq!(
        ProjectSession::read_proposal_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            &id,
            &result.receipt.result_ref.draft.revision,
            NOW + 24
        )
        .unwrap(),
        Some(accepted)
    );
    assert!(read_draft(&f.catalog, &source_id).unwrap().is_none());
    assert!(read_draft(&f.catalog, &id).unwrap().is_none());
}
