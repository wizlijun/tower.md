//! Real B0 publication, Add/Claim, immutable Git inputs and the production writer.
//! Faults stop the actual producer; no accepted Proposal or Run is fabricated.
use super::*;
use crate::snapshot_service::PublicationReport;
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
    assert_eq!(review.binding.target_baseline, "towermd/B1");
    assert!(
        review.publish_blockers.is_empty(),
        "{:?}",
        review.publish_blockers
    );
    assert!(!git(f.root.path(), &["tag", "--list"])
        .lines()
        .any(|tag| tag == "towermd/B1"));
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

#[test]
fn bn_population_uses_actual_prefix_before_and_after_selected_checkpoint() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let accepted = original(&f, &id);
    let inputs = f.outbox.read_inputs().unwrap();
    let confirmed_only = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    assert!(
        confirmed_only
            .task_population_at(&f.catalog, accepted.seq)
            .is_err(),
        "a confirmed Proposal/Task receipt cannot extend the actual source boundary"
    );
    checkpoint(&mut f, NOW + 21);
    let inputs = f.outbox.read_inputs().unwrap();
    let saved_history = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    let before = saved_history
        .task_population_at(&f.catalog, accepted.seq)
        .unwrap();
    assert_eq!(before["T-ask"].state, "running");
    assert_eq!(before.len(), 1);
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: crate::tasks::TaskSpec {
                id: "T-after-population".into(),
                title: "actual later task".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-ask".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 22,
        &mut f.outbox,
    )
    .unwrap();
    let later = watermark(&f);
    checkpoint(&mut f, NOW + 23);
    let inputs = f.outbox.read_inputs().unwrap();
    let history = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    let original_prefix = history
        .task_population_at(&f.catalog, accepted.seq)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&original_prefix).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    let after = history.task_population_at(&f.catalog, later).unwrap();
    assert_eq!(after["T-after-population"].state, "ready");
    assert_eq!(after.len(), 2);
    assert!(history
        .task_population_at(&f.catalog, watermark(&f) + 1)
        .is_err());
}

#[test]
fn bn_review_subject_needs_actual_saved_proposal_and_rejects_deleted_ref() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    let unaccepted = stop_after_real_save(&mut f, &input, &id);
    let binding =
        crate::baseline_review::review_binding(unaccepted.frozen(), unaccepted.revision());
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(
        crate::project_session::VerifiedProposalSubject::capture_shared(
            &f.session,
            &f.catalog,
            &inputs,
            &binding,
            NOW + 20
        )
        .is_err()
    );
    call(&mut f, &input, &id, NOW + 21).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let subject = crate::project_session::VerifiedProposalSubject::capture_shared(
        &f.session,
        &f.catalog,
        &inputs,
        &binding,
        NOW + 22,
    )
    .unwrap();
    subject
        .check_shared(&f.session, &f.catalog, &inputs, NOW + 22)
        .unwrap();
    let view =
        crate::baseline_review::review_view_shared(&f.session, &f.catalog, &inputs, &id, NOW + 22)
            .unwrap()
            .unwrap();
    assert_eq!(view.binding, binding);
    assert!(
        view.publish_blockers.is_empty(),
        "actual accepted Proposal should qualify for exact App review: {:?}",
        view.publish_blockers
    );
    let accepted = original(&f, &id);
    git(
        f.root.path(),
        &[
            "update-ref",
            "-d",
            &format!("refs/towermd/drafts/{id}"),
            unaccepted.revision(),
        ],
    );
    assert!(subject
        .check_shared(&f.session, &f.catalog, &inputs, NOW + 23)
        .is_err());
    assert_eq!(
        original(&f, &id),
        accepted,
        "read refusal never edits the original receipt"
    );
}

fn approve_proposal(f: &mut Fixture, id: &str, now: u64) -> crate::baseline_review::ReviewResult {
    let view = crate::baseline_review::review_view_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        id,
        now,
    )
    .unwrap()
    .unwrap();
    assert!(
        view.publish_blockers.is_empty(),
        "{:?}",
        view.publish_blockers
    );
    crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &crate::baseline_review::ReviewAction {
            request_id: uuid::Uuid::now_v7().to_string(),
            binding: view.binding,
            verdict: crate::baseline_review::Verdict::Approved,
            comment: "programmatic core test review; not GUI/human acceptance evidence".into(),
        },
        "human:owner",
        "host",
        now,
    )
    .unwrap()
}

#[test]
fn actual_bn_publication_marks_full_s_population_and_claim_check_finish_stay_blocked() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let approval = approve_proposal(&mut f, &id, NOW + 21);
    // This Task did not exist in the Proposal's declared affected_tasks.
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: crate::tasks::TaskSpec {
                id: "T-late-S".into(),
                title: "actual late Task before publication".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-ask".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 22,
        &mut f.outbox,
    )
    .unwrap();
    let result = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 25),
        )
        .unwrap();
    let PublicationReport::Recorded { created } = result else {
        panic!("real B1 publication incomplete: {result:?}")
    };
    assert_eq!(created.result.target, "towermd/B1");
    assert_eq!(
        git(f.root.path(), &["rev-parse", "refs/tags/towermd/B1"]),
        created.result.commit
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    for id in ["B0", "B1"] {
        let target = format!("towermd/{id}");
        let path = format!("{}/baselines/{id}.md", f.catalog.config.docs_dir());
        assert_eq!(f.catalog.object_paths[id], path);
        assert_eq!(f.catalog.objects[id].doc_type(), Some("Baseline"));
        assert_eq!(f.catalog.objects[id].get_str("id"), Some(id));
        assert_eq!(
            f.catalog.objects[id].get_str("git_tag"),
            Some(target.as_str())
        );
        assert_eq!(
            git(f.root.path(), &["show", "-s", "--format=%s", &target]),
            format!("Tower.md approved {id}")
        );
    }
    let saved = read_saved(&f, &id);
    let parent = git(
        f.root.path(),
        &["rev-parse", &format!("{}^", created.result.commit)],
    );
    let before = Catalog::at_revision(f.root.path(), &parent)
        .unwrap()
        .unwrap();
    let population = std::collections::BTreeMap::new();
    assert!(
        crate::proposals::analyze_publication_tasks(&before, saved.frozen(), &population)
            .unwrap_err()
            .contains("draft target baseline already exists")
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let history = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    let proof =
        crate::recovery_git::prove_publication(&f.catalog, &history, &created.result.commit)
            .unwrap();
    crate::proposals::analyze_verified_publication_tasks(
        &before,
        saved.frozen(),
        &population,
        &proof,
    )
    .unwrap();
    assert!(
        crate::proposals::analyze_verified_publication_tasks(
            &f.catalog,
            saved.frozen(),
            &population,
            &proof
        )
        .is_err(),
        "a proved publication cannot be rebound to the published child as parent"
    );
    let mut another = saved.frozen().clone();
    another.bundle.target_baseline = "towermd/B2".into();
    assert!(
        crate::proposals::analyze_verified_publication_tasks(
            &before,
            &another,
            &population,
            &proof
        )
        .is_err(),
        "existing B1 proof cannot audit another target"
    );
    let mut wrong_hash = saved.frozen().clone();
    wrong_hash.content_hash = "0".repeat(64);
    assert!(crate::proposals::analyze_verified_publication_tasks(
        &before,
        &wrong_hash,
        &population,
        &proof
    )
    .is_err());
    let reference = "refs/tags/towermd/B1";
    let tag_oid = git(f.root.path(), &["rev-parse", reference]);
    git(f.root.path(), &["update-ref", "-d", reference, &tag_oid]);
    assert!(
        crate::proposals::analyze_verified_publication_tasks(
            &before,
            saved.frozen(),
            &population,
            &proof
        )
        .is_err(),
        "a previously proved publication cannot substitute a missing current target tag"
    );
    git(f.root.path(), &["update-ref", reference, &tag_oid]);
    let markers = inputs
        .confirmed()
        .iter()
        .filter(|event| event.kind == "task.recheck_required")
        .collect::<Vec<_>>();
    assert_eq!(
        markers.len(),
        2,
        "both the original running Task and later ready Task need review"
    );
    let actual = inputs
        .confirmed()
        .iter()
        .find(|event| {
            event.eid
                == crate::events::derived_eid(&created.result.commit, "baseline.created").unwrap()
        })
        .unwrap();
    for marker in &markers {
        let record = crate::proposals::required_record(marker).unwrap();
        assert_eq!(&record.created, actual);
        let q = crate::snapshot_service::read_snapshot_commit(&f.catalog, &record.source_commit)
            .unwrap();
        assert_eq!(
            q.snapshot.through_seq + 1,
            actual.seq,
            "S must immediately follow its actual durable Q"
        );
        assert_eq!(record.fact.new_baseline, "towermd/B1");
    }
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    for task in ["T-ask", "T-late-S"] {
        assert_eq!(projection.objects["Task"][task]["needs_recheck"], true);
    }
    assert!(f
        .session
        .lock()
        .unwrap()
        .tasks
        .check_context(&f.catalog, "host", None)
        .unwrap_err()
        .contains("recheck"));
    assert!(f
        .session
        .lock()
        .unwrap()
        .tasks
        .finish_context(&f.catalog, "host", None)
        .unwrap_err()
        .contains("recheck"));
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 26,
        &mut f.outbox,
    )
    .unwrap();
    git(f.root.path(), &["switch", "-qc", "t/T-late-S", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let refusal = ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-late-S".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 27,
        &mut f.outbox,
    )
    .unwrap_err();
    assert!(refusal.to_string().contains("recheck"), "{refusal}");
    let count = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .filter(|event| event.kind == "task.recheck_required")
        .count();
    let repeated = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 28),
        )
        .unwrap();
    assert!(matches!(repeated, PublicationReport::Recorded { .. }));
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "task.recheck_required")
            .count(),
        count
    );
    assert_eq!(
        git(f.root.path(), &["tag", "--list", "towermd/B*"])
            .lines()
            .collect::<Vec<_>>(),
        vec!["towermd/B0", "towermd/B1"]
    );
}

#[test]
fn bn_approval_rejects_missing_exact_proposal_and_changed_parent_tag() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let saved = read_saved(&f, &id);
    let view = crate::baseline_review::review_view_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &id,
        NOW + 21,
    )
    .unwrap()
    .unwrap();
    let action = crate::baseline_review::ReviewAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding,
        verdict: crate::baseline_review::Verdict::Approved,
        comment: "original viewed subject".into(),
    };
    let before = f.outbox.read_inputs().unwrap().confirmed().to_vec();
    let reference = format!("refs/towermd/drafts/{id}");
    git(
        f.root.path(),
        &["update-ref", "-d", &reference, saved.revision()],
    );
    assert!(crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 22
    )
    .is_err());
    git(f.root.path(), &["update-ref", &reference, saved.revision()]);
    // A bare extra label is not a new parent or an automatic next-number choice.
    git(f.root.path(), &["tag", "towermd/B1", "main"]);
    assert!(crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 23
    )
    .is_err());
    assert_eq!(f.outbox.read_inputs().unwrap().confirmed(), before);
}

// Real producer matrix. These programmatic core actors are not GUI acceptance.
fn published_recheck_fixture() -> Fixture {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let approval = approve_proposal(&mut f, &id, NOW + 21);
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: crate::tasks::TaskSpec {
                id: "T-recheck-ready".into(),
                title: "Ready Task for exact App recheck".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-ask".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 22,
        &mut f.outbox,
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
            || Ok(NOW + 25),
        )
        .unwrap();
    assert!(matches!(publication, PublicationReport::Recorded { .. }));
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    f
}
fn read_recheck(f: &Fixture, task: &str, now: u64) -> crate::proposals::TaskRecheckView {
    ProjectSession::load_task_recheck_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        task,
        now,
    )
    .unwrap()
}
fn resolve_recheck(
    f: &mut Fixture,
    action: &crate::proposals::TaskRecheckAction,
    now: u64,
) -> Result<crate::proposals::TaskRecheckOutcome, String> {
    let result = f.writer.resolve_task_recheck(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        action,
        &mut f.outbox,
        || Ok(now),
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    result
}
fn recheck_with_self_consistent_subject(
    f: &Fixture,
    actual: &crate::hub_protocol::Event,
    subject: crate::proposals::TaskRecheckView,
) -> crate::hub_protocol::Event {
    let record = crate::proposals::decode_resolved(
        f.catalog.config.project_id(),
        &crate::recovery_outbox::original_of(actual),
    )
    .unwrap();
    let crate::proposals::ResolveRecheckAction::ResolveTaskRecheck { comment, .. } =
        record.request_body.action;
    let request = crate::proposals::RecheckRequest::prepare(
        f.catalog.config.project_id(),
        &actual.host,
        &actual.actor,
        &crate::proposals::TaskRecheckAction {
            request_id: record.p3_request.request_id,
            binding: subject.binding.clone(),
            comment,
        },
    )
    .unwrap();
    let original = crate::proposals::resolved_event(
        &request,
        &record.source_commit,
        record.source_seq,
        subject,
    )
    .unwrap();
    let mut changed = actual.clone();
    changed.data = original.data;
    changed.guards = original.guards;
    assert_eq!(changed.eid, actual.eid);
    crate::proposals::decode_resolved(
        f.catalog.config.project_id(),
        &crate::recovery_outbox::original_of(&changed),
    )
    .unwrap();
    crate::recovery_outbox::validate_actual(
        &crate::recovery_outbox::original_of(&changed),
        &changed,
    )
    .unwrap();
    changed
}

fn ready_recheck_fixture() -> Fixture {
    let mut f = published_recheck_fixture();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        NOW + 26,
        &mut f.outbox,
    )
    .unwrap();
    git(
        f.root.path(),
        &["switch", "-qc", "t/T-recheck-ready", "main"],
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    f
}
#[test]
fn task_recheck_ready_resolves_before_claim_and_same_uuid_is_one_original() {
    let mut f = ready_recheck_fixture();
    let view = read_recheck(&f, "T-recheck-ready", NOW + 30);
    assert_eq!(view.binding.running, None);
    assert_eq!(view.reasons.len(), 1);
    let refusal = ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-recheck-ready".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 30,
        &mut f.outbox,
    )
    .unwrap_err();
    assert!(refusal.to_string().contains("recheck"));
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding,
        comment: "确认此任务适用于当前基线；core test only".into(),
    };
    let first = resolve_recheck(&mut f, &action, NOW + 31).unwrap();
    assert_eq!(first.current_validity, ReviewValidity::Current);
    let original = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| event.eid == first.receipt.result_ref.resolved_eid)
        .unwrap()
        .clone();
    assert_eq!(original.actor, "human:owner");
    assert!(original.guards.is_empty());
    let record = crate::proposals::decode_resolved(
        f.catalog.config.project_id(),
        &crate::recovery_outbox::original_of(&original),
    )
    .unwrap();
    let q =
        crate::snapshot_service::read_snapshot_commit(&f.catalog, &record.source_commit).unwrap();
    assert_eq!(q.snapshot.through_seq + 1, original.seq);
    let before = watermark(&f);
    let retry = resolve_recheck(&mut f, &action, NOW + 32).unwrap();
    assert_eq!(retry.receipt, first.receipt);
    assert_eq!(watermark(&f), before);
    let mut changed = action.clone();
    changed.comment.push_str(" changed");
    assert!(resolve_recheck(&mut f, &changed, NOW + 32)
        .unwrap_err()
        .contains("request_id"));
    assert!(read_recheck(&f, "T-recheck-ready", NOW + 32)
        .reasons
        .is_empty());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-recheck-ready".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 33,
        &mut f.outbox,
    )
    .unwrap();
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Task"]["T-recheck-ready"]
            ["needs_recheck"],
        false
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "task.recheck_resolved")
            .count(),
        1
    );
}
#[test]
fn task_recheck_running_binds_renewal_expiry_and_rejects_agent() {
    let mut f = published_recheck_fixture();
    let old = read_recheck(&f, "T-ask", NOW + 30);
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: old.binding.clone(),
        comment: "review original Run".into(),
    };
    assert!(f
        .writer
        .resolve_task_recheck(
            &f.session,
            &f.catalog,
            "host",
            "process:towermd-cli",
            &action,
            &mut f.outbox,
            || Ok(NOW + 30)
        )
        .is_err());
    let fences = old.binding.running.as_ref().unwrap().fences.clone();
    let renewal = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .lease_renew(
            crate::hub_protocol::RenewLease {
                host: "host".into(),
                fences: fences.clone(),
                ttl_ms: 900_000,
            },
            NOW + 31,
        )
        .unwrap();
    let fresh = read_recheck(&f, "T-ask", NOW + 32);
    assert_ne!(fresh.binding.running, old.binding.running);
    let before = watermark(&f);
    assert!(resolve_recheck(&mut f, &action, NOW + 32)
        .unwrap_err()
        .contains("binding changed"));
    assert_eq!(watermark(&f), before);
    // A real ordinary checkpoint acknowledges/compacts the renewal before the
    // click. Its archive omits pure heartbeat; no new renewal may be invented.
    f.writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 33),
            &mut f.outbox,
        )
        .unwrap();
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let compacted = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(renewal.event.seq - 1, NOW + 34)
        .unwrap_err();
    assert!(matches!(
        compacted,
        crate::hub_protocol::HubError::SnapshotRequired { .. }
    ));
    let still_fresh = read_recheck(&f, "T-ask", NOW + 34);
    assert_eq!(still_fresh.binding, fresh.binding);
    let fresh_action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: still_fresh.binding,
        comment: "review renewed exact Run after real Q compaction".into(),
    };
    let result = resolve_recheck(&mut f, &fresh_action, NOW + 35).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let actual = inputs
        .confirmed()
        .iter()
        .find(|event| event.eid == result.receipt.result_ref.resolved_eid)
        .unwrap();
    assert_eq!(actual.guards, fences);
    let record = crate::proposals::decode_resolved(
        f.catalog.config.project_id(),
        &crate::recovery_outbox::original_of(actual),
    )
    .unwrap();
    let mut changed_expiry = record.subject.clone();
    *changed_expiry
        .binding
        .running
        .as_mut()
        .unwrap()
        .lease_expires_at
        .values_mut()
        .next()
        .unwrap() += 1;
    let changed = recheck_with_self_consistent_subject(&f, actual, changed_expiry);
    assert!(
        crate::recovery_git::validate_recheck_resolution_rows_for_test(
            &f.catalog,
            &[actual.clone(), changed],
        )
        .is_err(),
        "same eid cannot reuse a different complete expiry's Q proof"
    );
    let q =
        crate::snapshot_service::read_snapshot_commit(&f.catalog, &record.source_commit).unwrap();
    let proof = q.snapshot.recheck_lease_proof.as_ref().unwrap();
    assert_eq!(proof.through_seq, q.snapshot.through_seq);
    assert_eq!(proof.subject.binding, fresh_action.binding);
    assert_eq!(proof.request.request_id, fresh_action.request_id);
    assert_eq!(proof.generation, q.snapshot.recovery_generation());
    let mut forged_expiry = proof.clone();
    *forged_expiry
        .subject
        .binding
        .running
        .as_mut()
        .unwrap()
        .lease_expires_at
        .values_mut()
        .next()
        .unwrap() += 1;
    assert!(
        forged_expiry.validate(&q.snapshot).is_err(),
        "caller expiry cannot replace the bound original subject"
    );
    assert_eq!(
        proof
            .subject
            .binding
            .running
            .as_ref()
            .unwrap()
            .lease_expires_at,
        renewal
            .leases
            .iter()
            .map(|lease| (lease.name.clone(), lease.expires_at))
            .collect()
    );
    assert!(
        !q.archives
            .values()
            .any(|text| text.contains(&renewal.event.eid)),
        "finite audit proof must not promote omitted renewal into actual archives"
    );
    assert_eq!(read_recheck(&f, "T-ask", NOW + 36).reasons.len(), 0);
    // Original running approval remains historical across rebuild, with no
    // permission to replace/reacquire any Run fence.
    f = rebuild(f, NOW + 40);
    let projected = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(projected.objects["Task"]["T-ask"]["needs_recheck"], true);
    let retry = resolve_recheck(&mut f, &fresh_action, NOW + 42).unwrap();
    assert_eq!(retry.receipt, result.receipt);
    assert!(matches!(
        retry.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
}
#[test]
fn task_recheck_ready_actual_receipt_recovers_without_post_resolution_checkpoint() {
    let mut f = ready_recheck_fixture();
    let view = read_recheck(&f, "T-recheck-ready", NOW + 30);
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding,
        comment: "ready review, actual Q before S".into(),
    };
    let first = resolve_recheck(&mut f, &action, NOW + 31).unwrap();
    // No checkpoint here: only the producer's pre-resolution Q survives in Git.
    f = rebuild(f, NOW + 40);
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Task"]["T-recheck-ready"]
            ["needs_recheck"],
        false
    );
    let recovered = resolve_recheck(&mut f, &action, NOW + 42).unwrap();
    assert_eq!(recovered.receipt, first.receipt);
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-recheck-ready".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 43,
        &mut f.outbox,
    )
    .unwrap();
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "task.recheck_resolved")
            .count(),
        1
    );
}
#[test]
fn task_recheck_confirmation_uncertainty_repairs_original_uuid_without_second_clear() {
    let mut f = ready_recheck_fixture();
    let view = read_recheck(&f, "T-recheck-ready", NOW + 30);
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding,
        comment: "confirm fsync repair".into(),
    };
    f.outbox.fail_next_confirmation();
    let error = resolve_recheck(&mut f, &action, NOW + 31).unwrap_err();
    assert!(
        error.contains("uncertain") && error.contains(&action.request_id),
        "{error}"
    );
    let actual = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| event.kind == "task.recheck_resolved")
        .unwrap()
        .clone();
    let result = resolve_recheck(&mut f, &action, NOW + 32).unwrap();
    assert_eq!(result.receipt.result_ref.resolved_eid, actual.eid);
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "task.recheck_resolved")
            .count(),
        1
    );
}
#[test]
fn task_recheck_pending_fsync_retries_only_exact_unchanged_q() {
    let mut f = ready_recheck_fixture();
    let view = read_recheck(&f, "T-recheck-ready", NOW + 30);
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding,
        comment: "original pending Q".into(),
    };
    f.outbox.fail_next_pending();
    let error = resolve_recheck(&mut f, &action, NOW + 31).unwrap_err();
    assert!(
        error.contains(&action.request_id) && error.contains("uncertain"),
        "{error}"
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let pending = inputs
        .pending()
        .iter()
        .find(|e| e.kind == "task.recheck_resolved")
        .unwrap()
        .clone();
    assert!(!inputs.confirmed().iter().any(|e| e.eid == pending.eid));
    let result = resolve_recheck(&mut f, &action, NOW + 32).unwrap();
    assert_eq!(result.receipt.result_ref.resolved_eid, pending.eid);
    let actual = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|e| e.eid == pending.eid)
        .unwrap()
        .clone();
    assert_eq!(crate::recovery_outbox::original_of(&actual), pending);
    // A distinct fresh request has a distinct own no-op receipt, never a new
    // permission for the old nonempty subject under a silently upgraded UUID.
    let fresh = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: read_recheck(&f, "T-recheck-ready", NOW + 33).binding,
        comment: "independent reread".into(),
    };
    assert!(fresh.binding.publication_ids.is_empty());
    let second = resolve_recheck(&mut f, &fresh, NOW + 33).unwrap();
    assert_ne!(second.receipt.request_id, result.receipt.request_id);
}
#[test]
fn task_recheck_old_ready_click_is_not_migrated_to_new_run() {
    let mut f = ready_recheck_fixture();
    let old = read_recheck(&f, "T-recheck-ready", NOW + 30);
    let accepted = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: old.binding.clone(),
        comment: "ready review".into(),
    };
    resolve_recheck(&mut f, &accepted, NOW + 31).unwrap();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-recheck-ready".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 32,
        &mut f.outbox,
    )
    .unwrap();
    let old_click = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: old.binding,
        comment: "old ready subject after actual claim".into(),
    };
    let before = watermark(&f);
    assert!(resolve_recheck(&mut f, &old_click, NOW + 33).is_err());
    assert_eq!(watermark(&f), before);
    assert!(read_recheck(&f, "T-recheck-ready", NOW + 33)
        .binding
        .running
        .is_some());
}
#[test]
fn task_recheck_retry_never_clears_a_later_real_b2_publication() {
    let mut f = published_recheck_fixture();
    let running = read_recheck(&f, "T-ask", NOW + 30);
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: running.binding,
        comment: "only first actual B1".into(),
    };
    let first = resolve_recheck(&mut f, &action, NOW + 31).unwrap();
    // The disposable test repository stays on the exact original Task branch.
    // Rebase its next genuine Proposal input onto actual current main.
    git(f.root.path(), &["reset", "--hard", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let base = f.catalog.revision.clone();
    let path = f.catalog.object_paths["A-alpha"].clone();
    let text = fs::read_to_string(f.root.path().join(&path)).unwrap();
    fs::write(
        f.root.path().join(&path),
        format!("{text}\nsecond real baseline change\n"),
    )
    .unwrap();
    git(f.root.path(), &["add", &path]);
    git(
        f.root.path(),
        &["commit", "-qm", "second genuine Task Proposal"],
    );
    let revision = git(f.root.path(), &["rev-parse", "HEAD"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let input = ProposalInput {
        source: ProposalSource::Branch {
            revision,
            base_revision: base,
        },
        reason: "second change".into(),
        affected_intents: f
            .catalog
            .objects
            .iter()
            .filter(|(_, d)| d.doc_type() == Some("Intent"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_modules: vec!["A-alpha".into()],
        affected_interfaces: f
            .catalog
            .objects
            .iter()
            .filter(|(_, d)| d.doc_type() == Some("Interface"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_tasks: vec!["T-ask".into(), "T-recheck-ready".into()],
        affected_paths: vec!["alpha/**".into(), "docs/tower/**".into()],
        alternatives: vec![Alternative {
            name: "retain".into(),
            consequences: "keep B1".into(),
        }],
        recommendation: "second change".into(),
    };
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 40).unwrap();
    let approved = approve_proposal(&mut f, &id, NOW + 41);
    let published = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approved.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 45),
        )
        .unwrap();
    assert!(matches!(published, PublicationReport::Recorded { .. }));
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    // Actual B0/B1/B2 publications and their fixed Proposal sources. Count
    // physical immutable misses, not wall time or a presumed performance budget.
    // Each independent complete read creates a fresh finite owner.
    for _ in 0..2 {
        let counts = crate::recovery_git::read_publication_counts(
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
        )
        .unwrap();
        for label in ["towermd/B0", "towermd/B1", "towermd/B2"] {
            let commit = git(f.root.path(), &["rev-parse", label]);
            for kind in ["tree", "catalog", "message", "parents"] {
                assert_eq!(counts.get(&(commit.clone(), kind)), Some(&1), "{label}: {kind} must be physically read once per full OID in this complete read");
            }
        }
        assert!(counts.values().all(|count| *count == 1));
    }
    let current = read_recheck(&f, "T-ask", NOW + 46);
    assert_eq!(current.binding.target_baseline, "towermd/B2");
    assert_eq!(current.reasons.len(), 1);
    assert_eq!(current.reasons[0].new_baseline, "towermd/B2");
    let retry = resolve_recheck(&mut f, &action, NOW + 47).unwrap();
    assert_eq!(retry.receipt, first.receipt);
    assert!(matches!(
        retry.current_validity,
        ReviewValidity::HistoricalOnly { .. }
    ));
    assert_eq!(read_recheck(&f, "T-ask", NOW + 47).reasons, current.reasons);
    assert!(f
        .session
        .lock()
        .unwrap()
        .tasks
        .check_context(&f.catalog, "host", None)
        .unwrap_err()
        .contains("recheck"));
}

#[test]
fn task_recheck_native_view_validator_binds_raw_task_and_complete_reasons() {
    let f = published_recheck_fixture();
    let original = read_recheck(&f, "T-ask", NOW + 30);
    original.validate().unwrap();
    let mut text = original.clone();
    text.task_text.push_str("title: replaced title\n");
    assert!(text.validate().is_err());
    let mut facts = original.clone();
    facts.reasons.clear();
    assert!(facts.validate().is_err());
    let mut expiry = original.clone();
    expiry
        .binding
        .running
        .as_mut()
        .unwrap()
        .lease_expires_at
        .clear();
    assert!(expiry.validate().is_err());
    let mut state = original.clone();
    state.binding.running = None;
    assert!(state.validate().is_err());
}

// These regressions exercise actual producer writes, not hand-authored S or
// task.recheck_required records. Core test actors are not GUI acceptance.
fn audit_add_task(f: &mut Fixture, id: &str, now: u64) {
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: crate::tasks::TaskSpec {
                id: id.into(),
                title: "publication audit task".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-ask".into()],
                decisions: vec![],
            },
        },
        None,
        now,
        &mut f.outbox,
    )
    .unwrap();
}

#[test]
fn bn_after_tag_fault_invalidates_cached_admission_and_original_writer_can_finish() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    audit_add_task(&mut f, "T-audit-before-S", NOW + 21);
    let approval = approve_proposal(&mut f, &id, NOW + 22);
    super::super::baseline_publish::set_test_boundary(
        f.root.path(),
        "AfterTag",
        Box::new(|| Err("audit stop after actual tag".into())),
    );
    let first = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 25),
        )
        .unwrap();
    assert!(
        matches!(first, PublicationReport::Pending { .. }),
        "{first:?}"
    );
    assert!(!git(f.root.path(), &["rev-parse", "refs/tags/towermd/B1"]).is_empty());
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "baseline.created" && event.data["target"] == "towermd/B1"));
    git(
        f.root.path(),
        &["switch", "-qc", "t/T-audit-before-S", "main"],
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let refusal = ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-audit-before-S".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 26,
        &mut f.outbox,
    )
    .unwrap_err();
    assert!(refusal.to_string().contains("recheck"), "{refusal}");
    // No second approval/UUID: our own tag must invalidate and reprepare, not
    // permanently lock the single writer out of its exact Q2/S continuation.
    let retry = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 27),
        )
        .unwrap();
    assert!(
        matches!(retry, PublicationReport::Recorded { .. }),
        "{retry:?}"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let view = read_recheck(&f, "T-audit-before-S", NOW + 28);
    assert_eq!(view.reasons.len(), 1);
    assert_eq!(view.binding.target_baseline, "towermd/B1");
}

#[test]
fn bn_own_q_preparation_composes_actual_main_renewal_before_capture() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let approval = approve_proposal(&mut f, &id, NOW + 21);
    let clock = Arc::new(AtomicU64::new(NOW + 25));
    let changed = clock.clone();
    let renewal_seq = Arc::new(AtomicU64::new(0));
    let observed_seq = renewal_seq.clone();
    let session = f.session.clone();
    super::super::baseline_publish::set_test_boundary(
        f.root.path(),
        "BeforeOwnQ",
        Box::new(move || {
            let at = NOW + 60_025;
            let mut live = session.lock().unwrap();
            let before = live.hub.contiguous_seq();
            crate::snapshot_service::maintain_main(&mut live, "host", at)?;
            assert_eq!(
                live.hub.contiguous_seq(),
                before + 1,
                "actual main renewal must append"
            );
            observed_seq.store(before + 1, Ordering::SeqCst);
            changed.store(at, Ordering::SeqCst);
            Ok(())
        }),
    );
    let report = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(clock.load(Ordering::SeqCst)),
        )
        .unwrap();
    assert!(
        matches!(report, PublicationReport::Recorded { .. }),
        "{report:?}"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let inputs = f.outbox.read_inputs().unwrap();
    let marker = inputs
        .confirmed()
        .iter()
        .find(|event| event.kind == "task.recheck_required")
        .unwrap();
    let marker = crate::proposals::required_record(marker).unwrap();
    let saved =
        crate::snapshot_service::read_snapshot_commit(&f.catalog, &marker.source_commit).unwrap();
    assert_eq!(saved.snapshot.through_seq + 1, marker.created.seq);
    let seq = renewal_seq.load(Ordering::SeqCst);
    assert!(seq > 0 && seq <= saved.snapshot.through_seq);
    assert!(
        saved
            .snapshot
            .omitted
            .iter()
            .any(|range| range.start <= seq && seq <= range.end),
        "Q must account for the real renewal using FR-89 heartbeat coverage"
    );
    assert!(
        saved
            .archives
            .values()
            .flat_map(|text| text.lines())
            .all(|line| {
                let event: Event = serde_json::from_str(line).unwrap();
                !event.is_pure_heartbeat()
            }),
        "pure renewal heartbeats must not be archived"
    );
}

#[test]
fn bn_own_q_rejects_actual_post_q_renewal_and_original_review_uuid_recovers() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let approval = approve_proposal(&mut f, &id, NOW + 21);
    let clock = Arc::new(AtomicU64::new(NOW + 25));
    let changed = clock.clone();
    let session = f.session.clone();
    super::super::baseline_publish::set_test_boundary(
        f.root.path(),
        "AfterOwnQWritten",
        Box::new(move || {
            let at = NOW + 60_025;
            let mut live = session.lock().unwrap();
            let before = live.hub.contiguous_seq();
            crate::snapshot_service::maintain_main(&mut live, "host", at)?;
            assert_eq!(live.hub.contiguous_seq(), before + 1);
            changed.store(at, Ordering::SeqCst);
            Ok(())
        }),
    );
    let first = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(clock.load(Ordering::SeqCst)),
        )
        .unwrap();
    assert!(
        matches!(&first, PublicationReport::Pending { reason, .. }
        if reason.contains("population Q changed")),
        "{first:?}"
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| { event.kind == "baseline.created" && event.data["target"] == "towermd/B1" }));
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let old_q = crate::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let original = crate::baseline_review::ReviewAction {
        request_id: approval.receipt.request_id.clone(),
        binding: approval.receipt.result_ref.binding.clone(),
        verdict: crate::baseline_review::Verdict::Approved,
        comment: approval.receipt.result_ref.comment.clone(),
    };
    let retried = crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &original,
        "human:owner",
        "host",
        clock.load(Ordering::SeqCst),
    )
    .unwrap();
    assert_eq!(
        retried.receipt, approval.receipt,
        "same original review UUID and exact body"
    );
    let report = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &retried.receipt.result_ref.approval_eid,
            "host",
            || Ok(clock.load(Ordering::SeqCst)),
        )
        .unwrap();
    assert!(
        matches!(report, PublicationReport::Recorded { .. }),
        "{report:?}"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let inputs = f.outbox.read_inputs().unwrap();
    let actuals = inputs
        .confirmed()
        .iter()
        .filter(|event| event.kind == "baseline.created" && event.data["target"] == "towermd/B1")
        .collect::<Vec<_>>();
    assert_eq!(actuals.len(), 1);
    let marker = inputs
        .confirmed()
        .iter()
        .find(|event| event.kind == "task.recheck_required")
        .unwrap();
    let marker = crate::proposals::required_record(marker).unwrap();
    let q =
        crate::snapshot_service::read_snapshot_commit(&f.catalog, &marker.source_commit).unwrap();
    assert!(q.snapshot.through_seq > old_q.snapshot.through_seq);
    assert_eq!(q.snapshot.through_seq + 1, actuals[0].seq);
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| {
                event.kind == "baseline.approved"
                    && event.eid == approval.receipt.result_ref.approval_eid
            })
            .count(),
        1
    );
}

#[test]
fn bn_own_q_written_source_drift_refuses_s_before_consuming_original_approval() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let approval = approve_proposal(&mut f, &id, NOW + 21);
    let root = f.root.path().to_path_buf();
    let b0 = git(&root, &["rev-parse", "refs/tags/towermd/B0"]);
    super::super::baseline_publish::set_test_boundary(
        f.root.path(),
        "AfterOwnQWritten",
        Box::new(move || {
            let b1 = git(&root, &["rev-parse", "refs/tags/towermd/B1"]);
            git(&root, &["update-ref", "refs/tags/towermd/B0", &b1]);
            Ok(())
        }),
    );
    let first = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 25),
        )
        .unwrap();
    assert!(
        matches!(&first, PublicationReport::Pending { reason, .. }
        if reason.contains("source/approval refs changed")),
        "{first:?}"
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| { event.kind == "baseline.created" && event.data["target"] == "towermd/B1" }));
    git(f.root.path(), &["update-ref", "refs/tags/towermd/B0", &b0]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let report = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 26),
        )
        .unwrap();
    assert!(
        matches!(report, PublicationReport::Recorded { .. }),
        "{report:?}"
    );
}

#[test]
fn bn_own_q_rejects_foreign_writer_and_changed_checkpoint_seals() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let approval = approve_proposal(&mut f, &id, NOW + 21);
    super::super::baseline_publish::set_test_boundary(
        f.root.path(),
        "AfterTag",
        Box::new(|| Err("stop before any population Q".into())),
    );
    let first = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 25),
        )
        .unwrap();
    assert!(matches!(first, PublicationReport::Pending { .. }));
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let inputs = f.outbox.read_inputs().unwrap();
    let history = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    let commit = git(f.root.path(), &["rev-parse", "refs/tags/towermd/B1"]);
    let proof = crate::recovery_git::prove_publication(&f.catalog, &history, &commit).unwrap();
    let writer = uuid::Uuid::now_v7();
    let mut own = crate::project_session::OwnPublicationPreparation::prepare(
        writer,
        &f.session,
        &f.catalog,
        inputs,
        &proof,
        NOW + 26,
    )
    .unwrap();
    for foreign_writer in [true, false] {
        let checkpoint = {
            let mut live = f.session.lock().unwrap();
            own.absorb_renewals(&mut live, &f.catalog, NOW + 26)
                .unwrap();
            let fence = live.leases(NOW + 26)["main"].fence();
            let checkpoint = live
                .checkpoint(f.catalog.config.project_id(), &fence, NOW + 26)
                .unwrap();
            own.capture_q(&mut live, &f.catalog, &checkpoint, &fence, NOW + 26)
                .unwrap();
            checkpoint
        };
        // Negative-only construction inside SnapshotWriter's own private module
        // tree. Production callers cannot construct or deserialize this type.
        let mut fake = super::super::WrittenPublicationQ {
            writer: if foreign_writer {
                uuid::Uuid::now_v7()
            } else {
                writer
            },
            commit: commit.clone(),
            snapshot: checkpoint,
        };
        if !foreign_writer {
            fake.snapshot.through_seq += 1;
        }
        let before = f.session.lock().unwrap().hub.contiguous_seq();
        let error = own
            .record_written(&f.session, fake, &mut f.outbox, &mut || Ok(NOW + 26))
            .unwrap_err();
        assert!(error.contains("written seal differs"), "{error}");
        assert_eq!(f.session.lock().unwrap().hub.contiguous_seq(), before);
    }
}

#[test]
fn bn_private_q2_and_actual_s_recover_exact_population_before_first_marker() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    audit_add_task(&mut f, "T-private-Q", NOW + 21);
    let approval = approve_proposal(&mut f, &id, NOW + 22);
    // Publication CAS finishes with main free; making main checked out only
    // after its real tag forces the existing private-snapshot producer for Q2.
    let root = f.root.path().to_path_buf();
    super::super::baseline_publish::set_test_boundary(
        f.root.path(),
        "AfterTag",
        Box::new(move || {
            git(&root, &["switch", "main"]);
            Ok(())
        }),
    );
    super::super::baseline_publish::set_test_boundary(
        f.root.path(),
        "AfterCreatedBeforeMarkers",
        Box::new(|| Err("audit stop after actual S before marker".into())),
    );
    let first = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 25),
        )
        .unwrap();
    assert!(
        matches!(first, PublicationReport::Pending { .. }),
        "{first:?}"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let q_oid = git(f.root.path(), &["rev-parse", "refs/towermd/snapshot"]);
    let inputs = f.outbox.read_inputs().unwrap();
    let actual = inputs
        .confirmed()
        .iter()
        .find(|event| event.kind == "baseline.created" && event.data["target"] == "towermd/B1")
        .unwrap()
        .clone();
    assert!(!inputs
        .confirmed()
        .iter()
        .any(|event| event.kind == "task.recheck_required"));
    assert!(
        crate::snapshot_service::pending_baseline_publications(&f.catalog, &inputs)
            .unwrap()
            .contains(&approval.receipt.result_ref.approval_eid),
        "an actual S without its exact population markers still needs original continuation"
    );
    let q = crate::snapshot_service::read_snapshot_commit(&f.catalog, &q_oid).unwrap();
    assert_eq!(q.snapshot.through_seq + 1, actual.seq);
    assert!(
        !crate::repo::git_output(
            f.root.path(),
            &["merge-base", "--is-ancestor", &q_oid, "main"]
        )
        .unwrap()
        .status
        .success(),
        "fixture must require private ancestry"
    );
    // A post-S Task is outside the frozen S population, even if later snapshots
    // contain it. Never pick a newest population merely to repair the gap.
    audit_add_task(&mut f, "T-after-private-S", NOW + 26);
    git(
        f.root.path(),
        &["update-ref", "-d", "refs/towermd/snapshot", &q_oid],
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let missing = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 27),
        )
        .unwrap();
    assert!(
        matches!(missing, PublicationReport::Pending { .. }),
        "{missing:?}"
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "task.recheck_required"));
    git(
        f.root.path(),
        &["switch", "-qc", "t/T-after-private-S", "main"],
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert!(
        crate::snapshot_service::pending_baseline_publications(
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
        )
        .unwrap()
        .contains(&approval.receipt.result_ref.approval_eid),
        "missing private Q cannot mark this original publication completed"
    );
    let unknown = ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-after-private-S".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 27,
        &mut f.outbox,
    )
    .unwrap_err();
    assert!(
        unknown.to_string().contains("recheck"),
        "unknown Q cannot admit even a later Task: {unknown}"
    );
    git(
        f.root.path(),
        &["update-ref", "refs/towermd/snapshot", &q_oid],
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let recovered = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 28),
        )
        .unwrap();
    assert!(
        matches!(recovered, PublicationReport::Recorded { .. }),
        "{recovered:?}"
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let creations = inputs
        .confirmed()
        .iter()
        .filter(|event| event.kind == "baseline.created" && event.data["target"] == "towermd/B1")
        .collect::<Vec<_>>();
    assert_eq!(
        creations,
        vec![&actual],
        "repair keeps the actual original S"
    );
    let records = inputs
        .confirmed()
        .iter()
        .filter(|event| event.kind == "task.recheck_required")
        .map(|event| crate::proposals::required_record(event).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record.source_commit == q_oid && record.created == actual));
    assert!(records
        .iter()
        .any(|record| record.fact.task == "T-private-Q"));
    assert!(!records
        .iter()
        .any(|record| record.fact.task == "T-after-private-S"));
    let current = Catalog::load(f.root.path()).unwrap();
    assert!(
        !crate::snapshot_service::pending_baseline_publications(&current, &inputs)
            .unwrap()
            .contains(&approval.receipt.result_ref.approval_eid),
        "the original publication completes only after its exact markers are accepted"
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| {
                event.kind == "baseline.approved"
                    && event.eid == approval.receipt.result_ref.approval_eid
            })
            .count(),
        1,
        "recovery never creates another approval"
    );
    assert_eq!(read_recheck(&f, "T-private-Q", NOW + 29).reasons.len(), 1);
}

#[test]
fn task_recheck_finite_q_proof_rejects_unknown_fields_and_subject_request_source_forgery() {
    let mut f = ready_recheck_fixture();
    let view = read_recheck(&f, "T-recheck-ready", NOW + 30);
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding,
        comment: "finite Q proof negative matrix".into(),
    };
    let result = resolve_recheck(&mut f, &action, NOW + 31).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let actual = inputs
        .confirmed()
        .iter()
        .find(|event| event.eid == result.receipt.result_ref.resolved_eid)
        .unwrap();
    let record = crate::proposals::decode_resolved(
        f.catalog.config.project_id(),
        &crate::recovery_outbox::original_of(actual),
    )
    .unwrap();
    crate::recovery_git::validate_recheck_resolution_rows_for_test(
        &f.catalog,
        &[actual.clone(), actual.clone()],
    )
    .unwrap();
    let mut changed_subject = record.subject.clone();
    let mut task: crate::tasks::Task = serde_yaml::from_str(&changed_subject.task_text).unwrap();
    task.spec.title.push_str(" self-consistent forged subject");
    changed_subject.task_text = serde_yaml::to_string(&task).unwrap();
    changed_subject.binding.task_content_hash = crate::proposals::task_content_hash(&task).unwrap();
    let changed = recheck_with_self_consistent_subject(&f, actual, changed_subject);
    assert!(
        crate::recovery_git::validate_recheck_resolution_rows_for_test(
            &f.catalog,
            &[actual.clone(), changed.clone()],
        )
        .is_err(),
        "same eid cannot reuse a different complete subject's Q proof"
    );
    let q =
        crate::snapshot_service::read_snapshot_commit(&f.catalog, &record.source_commit).unwrap();
    let snapshot = &q.snapshot;
    let proof = snapshot.recheck_lease_proof.as_ref().unwrap();
    proof.validate(snapshot).unwrap();
    let request = crate::proposals::RecheckRequest::prepare(
        f.catalog.config.project_id(),
        "host",
        "human:owner",
        &action,
    )
    .unwrap();
    proof.require_request(&request).unwrap();
    let mut unknown = serde_json::to_value(snapshot).unwrap();
    unknown["recheck_lease_proof"]["grant_permission"] = serde_json::json!(true);
    assert!(serde_json::from_value::<crate::snapshot::Snapshot>(unknown).is_err());
    let mut generation = proof.clone();
    generation.generation += 1;
    assert!(generation.validate(snapshot).is_err());
    let mut source = proof.clone();
    source.through_seq += 1;
    assert!(source.validate(snapshot).is_err());
    let mut task = proof.clone();
    task.subject.task_text.push_str("title: forged\n");
    assert!(task.validate(snapshot).is_err());
    let mut original = proof.clone();
    original.request.request_id = uuid::Uuid::now_v7().to_string();
    assert!(original.require_request(&request).is_err());
    let mut body = proof.clone();
    let crate::proposals::ResolveRecheckAction::ResolveTaskRecheck { comment, .. } =
        &mut body.request_body.action;
    comment.push_str(" forged");
    assert!(body.validate(snapshot).is_err());
    // Optional/default keeps old ordinary sources readable; it grants no finite
    // recheck proof. The reader explicitly refuses None for a recheck original.
    let mut legacy = serde_json::to_value(snapshot).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("recheck_lease_proof");
    let legacy = serde_json::from_value::<crate::snapshot::Snapshot>(legacy).unwrap();
    assert!(legacy.recheck_lease_proof.is_none());
    assert!(legacy.task_recheck_proof().is_err());
    let extended = crate::snapshot::Snapshot::extend(
        Some(snapshot),
        &[],
        snapshot.through_seq,
        snapshot.writer_epoch,
    )
    .unwrap();
    assert!(
        extended.recheck_lease_proof.is_none(),
        "ordinary checkpoints must not carry an older request proof"
    );
    // The same genuine history may return only its sealed complete originals;
    // an internally self-consistent changed row is still not that original.
    let mut history = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    let originals = crate::recovery_git::task_recheck_originals(&f.catalog, &history).unwrap();
    assert!(originals.contains(actual));
    assert!(
        crate::recovery_git::altered_recheck_history_original_for_test(
            &f.catalog,
            &mut history,
            changed,
        )
        .is_err()
    );
    assert_eq!(
        crate::recovery_git::task_recheck_originals(&f.catalog, &history).unwrap(),
        originals
    );
    let tag = "refs/tags/towermd/B1";
    let tag_oid = git(f.root.path(), &["rev-parse", tag]);
    git(f.root.path(), &["update-ref", "-d", tag, &tag_oid]);
    assert!(
        crate::recovery_git::task_recheck_originals(&f.catalog, &history).is_err(),
        "a previously sealed history cannot hide an actual tag deletion"
    );
    git(f.root.path(), &["update-ref", tag, &tag_oid]);
    assert_eq!(
        crate::recovery_git::task_recheck_originals(&f.catalog, &history).unwrap(),
        originals
    );
}

#[test]
fn task_recheck_normal_checkpoint_drops_finite_proof_and_real_rebuild_does_not_create_leases() {
    let mut f = ready_recheck_fixture();
    let view = read_recheck(&f, "T-recheck-ready", NOW + 30);
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding,
        comment: "ordinary Q must not inherit proof".into(),
    };
    resolve_recheck(&mut f, &action, NOW + 31).unwrap();
    let report = f
        .writer
        .write(
            &f.session,
            &f.catalog,
            "host",
            true,
            || Ok(NOW + 33),
            &mut f.outbox,
        )
        .unwrap();
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let source = crate::snapshot_service::read_snapshot_commit(
        &f.catalog,
        report.commit.as_deref().unwrap(),
    )
    .unwrap();
    assert!(source.snapshot.recheck_lease_proof.is_none());
    f = rebuild(f, NOW + 40);
    assert!(
        f.session
            .lock()
            .unwrap()
            .leases(NOW + 41)
            .keys()
            .all(|name| !name.starts_with("module:")),
        "finite Q proof cannot restore Run leases"
    );
    let retry = resolve_recheck(&mut f, &action, NOW + 42).unwrap();
    assert_eq!(retry.receipt.request_id, action.request_id);
    assert!(f
        .session
        .lock()
        .unwrap()
        .leases(NOW + 43)
        .keys()
        .all(|name| !name.starts_with("module:")));
}

// Source-only attention integration: real Proposal producers, not raw-tail cards.
fn attention_progress(f: &Fixture, now: u64) -> crate::app_read_model::TaskProgressView {
    ProjectSession::read_task_progress(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        now,
        Some(&f.session),
    )
    .unwrap()
}
#[test]
fn proposal_attention_uses_complete_proved_prefix_beyond_event_tail_without_writing() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    for index in 0..300 {
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .append(
                crate::hub_protocol::NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "host".into(),
                    actor: "process:test".into(),
                    kind: "future.audit".into(),
                    layer: "L5".into(),
                    obj: "audit".into(),
                    data: serde_json::json!({}),
                    guards: vec![],
                },
                NOW + 30 + index,
            )
            .unwrap();
    }
    let journal = f.catalog.common_dir.join("towermd/journal.jsonl");
    let bytes = fs::read(&journal).unwrap();
    let seq = watermark(&f);
    let main = git(f.root.path(), &["rev-parse", "main"]);
    let refs = crate::snapshot_service::draft_ref_fingerprint(&f.catalog).unwrap();
    let view = attention_progress(&f, NOW + 400);
    assert!(
        !view.events.iter().any(|e| e.kind == "proposal.created"),
        "actual Proposal is older than bounded tail"
    );
    assert_eq!(view.proposals.len(), 1);
    let item = &view.proposals[0];
    let saved = read_saved(&f, &id);
    assert_eq!(
        item.binding,
        crate::baseline_review::review_binding(saved.frozen(), saved.revision())
    );
    assert_eq!(item.reason, input.reason);
    assert_eq!(item.recommendation, input.recommendation);
    assert_eq!(item.created_seq, original(&f, &id).seq);
    assert_eq!(fs::read(journal).unwrap(), bytes);
    assert_eq!(watermark(&f), seq);
    assert_eq!(git(f.root.path(), &["rev-parse", "main"]), main);
    assert_eq!(
        crate::snapshot_service::draft_ref_fingerprint(&f.catalog).unwrap(),
        refs
    );
}
#[test]
fn terminal_decisions_remove_new_review_items_but_approved_original_stays_diagnostic_only() {
    for verdict in [
        crate::baseline_review::Verdict::Approved,
        crate::baseline_review::Verdict::Rejected,
    ] {
        let (mut f, input) = proposed();
        let id = uuid::Uuid::now_v7().to_string();
        call(&mut f, &input, &id, NOW + 20).unwrap();
        assert_eq!(attention_progress(&f, NOW + 21).proposals.len(), 1);
        let saved = read_saved(&f, &id);
        let action = crate::baseline_review::ReviewAction {
            request_id: uuid::Uuid::now_v7().to_string(),
            binding: crate::baseline_review::review_binding(saved.frozen(), saved.revision()),
            verdict: verdict.clone(),
            comment: "core protocol test only, not GUI human acceptance".into(),
        };
        let decided = crate::baseline_review::review_baseline_shared(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &action,
            "human:owner",
            "host",
            NOW + 22,
        )
        .unwrap();
        let view = attention_progress(&f, NOW + 23);
        assert!(view.proposals.is_empty());
        if verdict == crate::baseline_review::Verdict::Approved {
            assert!(view.diagnostics.iter().any(|text| text
                .contains(&decided.receipt.result_ref.approval_eid)
                && text.contains("do not prepare another approval")));
        }
    }
}
#[test]
fn materialized_only_or_changed_saved_ref_is_diagnostic_never_a_pending_card() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    let _stop = Stop::arm();
    assert!(call(&mut f, &input, &id, NOW + 20).is_err());
    let view = attention_progress(&f, NOW + 21);
    assert!(view.proposals.is_empty());
    assert!(view
        .diagnostics
        .iter()
        .any(|text| text.contains(&id) && text.contains("不推断拒绝")));
    drop(_stop);
    call(&mut f, &input, &id, NOW + 22).unwrap();
    assert_eq!(attention_progress(&f, NOW + 23).proposals.len(), 1);
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
    let view = attention_progress(&f, NOW + 24);
    assert!(view.proposals.is_empty());
    assert!(view
        .diagnostics
        .iter()
        .any(|text| text.contains(&id) && text.contains("historical")));
}
#[test]
fn actually_published_target_and_rebuilt_historical_proposal_never_offer_new_review() {
    let (mut f, input) = proposed();
    let id = uuid::Uuid::now_v7().to_string();
    call(&mut f, &input, &id, NOW + 20).unwrap();
    let saved = read_saved(&f, &id);
    let decision = crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &crate::baseline_review::ReviewAction {
            request_id: uuid::Uuid::now_v7().to_string(),
            binding: crate::baseline_review::review_binding(saved.frozen(), saved.revision()),
            verdict: crate::baseline_review::Verdict::Approved,
            comment: "core test only".into(),
        },
        "human:owner",
        "host",
        NOW + 21,
    )
    .unwrap();
    let publication = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &decision.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 25),
        )
        .unwrap();
    assert!(matches!(
        publication,
        crate::snapshot_service::PublicationReport::Recorded { .. }
    ));
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    assert!(attention_progress(&f, NOW + 26).proposals.is_empty());
    let (mut historical, input) = proposed();
    let old = uuid::Uuid::now_v7().to_string();
    call(&mut historical, &input, &old, NOW + 20).unwrap();
    checkpoint(&mut historical, NOW + 21);
    let historical = rebuild(historical, NOW + 22);
    let view = attention_progress(&historical, NOW + 24);
    assert!(view.proposals.is_empty());
    assert!(view
        .diagnostics
        .iter()
        .any(|text| text.contains(&old) && text.contains("未核实当前实际接受资格")));
}

#[test]
fn bn_required_plan_reuse_rejects_other_q_s_fact_and_next_call_tag_drift() {
    // Two actual markers from the production B1 writer; no accepted event or
    // history proof is fabricated for the positive population.
    let f = published_recheck_fixture();
    let inputs = f.outbox.read_inputs().unwrap();
    let history = crate::recovery_git::read_recovery_history(&f.catalog, &inputs).unwrap();
    let markers = inputs
        .confirmed()
        .iter()
        .filter(|event| event.kind == "task.recheck_required")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 2);
    let first = crate::proposals::required_record(&markers[0]).unwrap();
    let second = crate::proposals::required_record(&markers[1]).unwrap();
    assert_eq!(first.created, second.created);
    assert_eq!(first.source_commit, second.source_commit);
    crate::recovery_git::validate_required_markers_for_test(&f.catalog, &history, &markers)
        .unwrap();

    // Every negative remains a self-consistent typed request/receipt envelope.
    // Put a genuine marker first so its shared plan already exists when the
    // second row is inspected. No negative is appended to the Hub or outbox.
    let changed = |source: &str, created: &Event, fact: &crate::proposals::RecheckFact| {
        let mut marker = markers[1].clone();
        let original =
            crate::proposals::required_event(f.catalog.config.project_id(), fact, source, created)
                .unwrap();
        marker.data = original.data;
        crate::proposals::required_record(&marker).unwrap();
        marker
    };
    let mut fact = second.fact.clone();
    fact.reasons = vec!["self-consistent but unproved impact reason".into()];
    let bad_fact = changed(&second.source_commit, &second.created, &fact);
    let mut another_s = second.created.clone();
    another_s.seq += 1;
    assert!(
        markers[1].seq > another_s.seq,
        "the S negative must reach the population proof, not just fail marker order"
    );
    let bad_s = changed(&second.source_commit, &another_s, &second.fact);
    let listing = git(
        f.root.path(),
        &["log", "--format=%H", &second.source_commit],
    );
    let another_q = listing
        .lines()
        .find(|commit| {
            crate::snapshot_service::read_snapshot_commit(&f.catalog, commit).is_ok_and(|saved| {
                saved.snapshot.through_seq.checked_add(1) != Some(second.created.seq)
            })
        })
        .expect("the actual B0/B1 history must contain an earlier different Q");
    let bad_q = changed(another_q, &second.created, &second.fact);
    for bad in [bad_fact, bad_s, bad_q] {
        assert!(crate::recovery_git::validate_required_markers_for_test(
            &f.catalog,
            &history,
            &[markers[0].clone(), bad],
        )
        .is_err());
    }
    let mut wrong_order = markers[1].clone();
    wrong_order.seq = second.created.seq;
    assert!(crate::recovery_git::validate_required_markers_for_test(
        &f.catalog,
        &history,
        &[markers[0].clone(), wrong_order],
    )
    .unwrap_err()
    .contains("project/order"));

    // A completed prior call cannot retain a plan as current authority. Use
    // the same already-proved history and markers after a real tag deletion.
    let reference = "refs/tags/towermd/B1";
    let tag = git(f.root.path(), &["rev-parse", reference]);
    git(f.root.path(), &["update-ref", "-d", reference, &tag]);
    let missing_tag =
        crate::recovery_git::validate_required_markers_for_test(&f.catalog, &history, &markers);
    git(f.root.path(), &["update-ref", reference, &tag]);
    assert!(
        missing_tag.is_err(),
        "an earlier same-group proof cannot substitute a current missing B1 tag"
    );
    crate::recovery_git::validate_required_markers_for_test(&f.catalog, &history, &markers)
        .unwrap();
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), inputs.digest());
}

#[test]
fn finite_completed_proposal_node_keeps_original_binding_refs_and_owner_boundaries() {
    let f = published_recheck_fixture();
    let commit = git(f.root.path(), &["rev-parse", "refs/tags/towermd/B1"]);
    let before = git(
        f.root.path(),
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let seq = f.session.lock().unwrap().hub.contiguous_seq();
    crate::recovery_git::exercise_completed_proposal_context_for_test(&f.catalog, &commit);
    assert_eq!(
        git(
            f.root.path(),
            &["for-each-ref", "--format=%(refname) %(objectname)"]
        ),
        before
    );
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), inputs.digest());
    assert_eq!(f.session.lock().unwrap().hub.contiguous_seq(), seq);
}

fn task_recheck_clock_only_expiry_at(boundary: &'static str) {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let mut f = published_recheck_fixture();
    let initial = read_recheck(&f, "T-ask", NOW + 30);
    let running = initial
        .binding
        .running
        .as_ref()
        .expect("actual running Task");
    let expiry = *running.lease_expires_at.values().min().unwrap();
    // Start one millisecond before the existing real Run expiry. Normal writer
    // preparation acquires/renews main for its full TTL, isolating Run expiry
    // from main expiry without extending the Run or manufacturing an Event.
    let start = expiry.checked_sub(1).unwrap();
    let viewed = read_recheck(&f, "T-ask", start);
    assert_eq!(viewed.binding.running, initial.binding.running);
    let action = crate::proposals::TaskRecheckAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: viewed.binding,
        comment: format!("clock-only expiry at {boundary}"),
    };
    let fences = action.binding.running.as_ref().unwrap().fences.clone();
    let clock = Arc::new(AtomicU64::new(start));
    let changed = clock.clone();
    let reached = Arc::new(AtomicBool::new(false));
    let observed = reached.clone();
    super::super::baseline_publish::set_test_boundary(
        f.root.path(),
        boundary,
        Box::new(move || {
            assert_eq!(changed.load(Ordering::SeqCst), start);
            changed.store(expiry, Ordering::SeqCst);
            observed.store(true, Ordering::SeqCst);
            // Clock only: no Event, tick, subscribe, release, renewal or lease
            // mutation. The actual final producer boundary must detect expiry.
            Ok(())
        }),
    );
    let result = f.writer.resolve_task_recheck(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &action,
        &mut f.outbox,
        || Ok(clock.load(Ordering::SeqCst)),
    );
    assert!(
        reached.load(Ordering::SeqCst),
        "actual Q/proof boundary was not reached: {result:?}"
    );
    assert!(result.is_err(), "a physically expired Run must not receive S, including an Ok historical outcome: {result:?}");
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(
        !inputs.confirmed().iter().any(|event| {
            event.kind == "task.recheck_resolved"
                && event.data["p3_request"]["request_id"] == action.request_id
        }),
        "no original resolution may be accepted/confirmed after Run expiry"
    );
    let current = Catalog::load(f.root.path()).unwrap();
    let q = crate::snapshot_service::read_snapshot(&current)
        .unwrap()
        .expect("actual durable Q remains after refused S");
    assert_eq!(
        q.snapshot.task_recheck_proof().unwrap().subject.binding,
        action.binding
    );
    let live = f.session.lock().unwrap();
    assert!(
        q.snapshot.through_seq < live.hub.contiguous_seq(),
        "actual expiry advances the durable Q boundary before S"
    );
    assert_eq!(
        live.projection().unwrap().objects["Task"]["T-ask"]["needs_recheck"],
        true
    );
    assert!(
        live.leases(expiry)
            .get("main")
            .is_some_and(|lease| lease.expires_at > expiry),
        "main is still live; this negative isolates the Run fence expiry"
    );
    for fence in &fences {
        assert!(!live.leases(expiry).contains_key(&fence.name));
    }
    let events = live.events(0);
    assert!(!events
        .iter()
        .any(|event| event.kind == "task.recheck_resolved"
            && event.data["p3_request"]["request_id"] == action.request_id));
    assert!(
        events.iter().any(|event| event.kind == "lease.expired"
            && fences
                .iter()
                .all(|fence| event.released_lease_names().contains(&fence.name))),
        "the producer's fresh head read settles actual expiry"
    );
}

#[test]
fn task_recheck_rejects_clock_only_run_expiry_after_source_proof() {
    task_recheck_clock_only_expiry_at("AfterTaskRecheckSourceProof");
}

#[test]
fn task_recheck_rejects_clock_only_run_expiry_after_final_slow_sync() {
    task_recheck_clock_only_expiry_at("BeforeTaskRecheckFinalAppend");
}
