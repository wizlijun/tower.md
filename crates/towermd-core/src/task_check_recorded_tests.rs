//! Actual B0/Add/Claim and immutable Task commits; never fake a failure Event.
use super::*;
use crate::{
    ask_fixture::{self as fixture, git, Fixture, NOW},
    project_session::ProjectSession,
    task_gates::{self, GateError},
};
use std::{cell::Cell, fs, rc::Rc};
fn committed(protected: bool) -> (Fixture, String) {
    let f = fixture::claimed();
    let before = git(f.root.path(), &["rev-parse", "HEAD"]);
    let path = if protected {
        f.catalog.object_paths["A-alpha"].clone()
    } else {
        "outside.txt".into()
    };
    let text = fs::read_to_string(f.root.path().join(&path)).unwrap_or_default();
    fs::write(
        f.root.path().join(&path),
        format!("{text}\nactual rejected change\n"),
    )
    .unwrap();
    git(f.root.path(), &["add", &path]);
    git(f.root.path(), &["commit", "-qm", "actual G2 violation"]);
    (f, before)
}
fn check(f: &mut Fixture, now: u64) -> Result<task_gates::CheckReport, GateError> {
    task_gates::check_recorded_shared(&f.session, &f.catalog, "host", None, &mut f.outbox, || {
        Ok(now)
    })
}
// Callers have no prior Passed diagnostic. The existing clock callback
// sees its first real confirmed Passed only after append has released Session.
// Record the existing read count there; do not add a production counter/hook.
fn observed_passed_clock(
    f: &Fixture,
    now: u64,
) -> (
    Rc<Cell<Option<u64>>>,
    impl Fn() -> Result<u64, String> + 'static,
) {
    observed_passed_clock_with(f, now, || {})
}
fn observed_passed_clock_with(
    f: &Fixture,
    now: u64,
    after_acceptance: impl Fn() + 'static,
) -> (
    Rc<Cell<Option<u64>>>,
    impl Fn() -> Result<u64, String> + 'static,
) {
    assert!(!diagnostic_events(f)
        .iter()
        .any(|event| event.data["evidence"]["result"] == "passed"));
    let count = Rc::new(Cell::new(None));
    let observed = count.clone();
    let directory = f.runtime.path().join("outbox");
    let catalog = f.catalog.clone();
    let session = f.session.clone();
    let clock = move || {
        if observed.get().is_none() {
            let inputs = recovery_outbox::RecoveryInputs::read(&directory, &catalog)?;
            if inputs
                .confirmed()
                .iter()
                .any(|event| is_event(event) && event.data["evidence"]["result"] == "passed")
            {
                observed.set(Some(session.lock().unwrap().authority_reads()));
                after_acceptance();
            }
        }
        Ok(now)
    };
    (count, clock)
}
fn diagnostic_events(f: &Fixture) -> Vec<Event> {
    f.outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .filter(|event| is_event(event))
        .cloned()
        .collect()
}
#[test]
fn actual_protected_and_outside_checks_keep_hard_errors_and_publish_one_typed_forbidden() {
    for protected in [true, false] {
        let (mut f, _) = committed(protected);
        let first = check(&mut f, NOW + 20).unwrap_err();
        assert!(if protected {
            matches!(first, GateError::Protected(_))
        } else {
            matches!(first, GateError::OutsideTask(_))
        });
        let events = diagnostic_events(&f);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        let record = decode(&recovery_outbox::original_of(event)).unwrap();
        assert_eq!(event.actor, "process:towermd-gates");
        assert_eq!(event.guards, record.context.run.leases);
        assert_eq!(record.evidence.result, GateResult::Failed);
        assert_eq!(record.evidence.gate, Gate::G2);
        let hold = record.hold.as_ref().unwrap();
        assert_eq!(hold["kind"], "forbidden");
        assert_eq!(hold["scope"], "TaskBranch");
        assert_eq!(hold["run"], record.context.run.id);
        assert!(hold["path_bytes"]
            .as_array()
            .is_some_and(|bytes| !bytes.is_empty()));
        let projected = f.session.lock().unwrap().projection().unwrap().clone();
        assert_eq!(projected.objects["Task"]["T-ask"]["state"], "running");
        assert_eq!(
            projected.objects["Run"][&record.context.run.id]["last_check"],
            record.evidence.id
        );
        let inputs = f.outbox.read_inputs().unwrap();
        let run: Run =
            serde_json::from_value(projected.objects["Run"][&record.context.run.id].clone())
                .unwrap();
        assert!(inputs.confirmed().contains(event));
        assert!(f.session.lock().unwrap().tail.contains(event));
        let known = known_originals(&f.session, &f.catalog, &inputs, &run, NOW + 21).unwrap();
        assert_eq!(known.len(), 1);
        assert_eq!(known.get(&event.eid), Some(event));
        assert_eq!(f.outbox.read_inputs().unwrap().digest(), inputs.digest());
        assert_eq!(check(&mut f, NOW + 21).unwrap_err(), first);
        assert_eq!(
            diagnostic_events(&f),
            events,
            "same tree reuses its immutable first actual record"
        );
        let view = ProjectSession::read_task_progress(
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            NOW + 22,
            Some(&f.session),
        )
        .unwrap();
        assert!(view.holds.iter().any(|raw| raw["kind"] == "forbidden"));
    }
}
#[test]
fn actual_fix_requires_new_tree_and_true_check_before_prior_card_becomes_history() {
    let (mut f, _) = committed(true);
    assert!(matches!(
        check(&mut f, NOW + 20),
        Err(GateError::Protected(_))
    ));
    let rejected = diagnostic_events(&f)[0].clone();
    let id = decode(&recovery_outbox::original_of(&rejected))
        .unwrap()
        .hold
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    git(f.root.path(), &["revert", "--no-edit", "HEAD"]);
    // Merely changing Git cannot resolve or approve an old diagnosis.
    assert!(
        f.session.lock().unwrap().projection().unwrap().objects["Hold"][&id]
            .get("resolved_by")
            .is_none()
    );
    let (postappend_reads, clock) = observed_passed_clock(&f, NOW + 21);
    let passed = task_gates::check_recorded_shared(
        &f.session,
        &f.catalog,
        "host",
        None,
        &mut f.outbox,
        clock,
    )
    .unwrap();
    assert_eq!(
        Some(f.session.lock().unwrap().authority_reads()),
        postappend_reads.get(),
        "the newly accepted Passed check installs its own exact suffix",
    );
    assert!(!passed.gates["G3"], "a check never fabricates verification");
    let events = diagnostic_events(&f);
    assert_eq!(events.len(), 2);
    let fixed = decode(&recovery_outbox::original_of(&events[1])).unwrap();
    assert_eq!(fixed.evidence.result, GateResult::Passed);
    assert!(fixed.hold.is_none());
    assert_ne!(
        fixed.context.tree,
        decode(&recovery_outbox::original_of(&rejected))
            .unwrap()
            .context
            .tree
    );
    let state = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(state.objects["Hold"][&id]["resolved_by"], fixed.evidence.id);
    assert!(state.objects["Evidence"].contains_key(
        &decode(&recovery_outbox::original_of(&rejected))
            .unwrap()
            .evidence
            .id
    ));
    assert_eq!(state.objects["Task"]["T-ask"]["state"], "running");
    check(&mut f, NOW + 22).unwrap();
    assert_eq!(
        diagnostic_events(&f),
        events,
        "same passing tree also deduplicates"
    );
}
#[test]
fn changed_tree_or_expired_fences_cannot_append_a_current_diagnostic() {
    for expire in [false, true] {
        let (mut f, _) = committed(true);
        let root = f.root.path().to_owned();
        let count = Cell::new(0u32);
        let result = task_gates::check_recorded_shared(
            &f.session,
            &f.catalog,
            "host",
            None,
            &mut f.outbox,
            || {
                let call = count.get();
                count.set(call + 1);
                if call == 1 {
                    if expire {
                        return Ok(NOW + 1_000_000);
                    }
                    fs::write(
                        root.join("alpha/after-check.txt"),
                        "changed after actual inspection",
                    )
                    .unwrap();
                    git(&root, &["add", "alpha/after-check.txt"]);
                    git(&root, &["commit", "-qm", "source raced check"]);
                }
                Ok(NOW + 20)
            },
        );
        assert!(
            matches!(result, Err(GateError::Protected(_))),
            "the original hard refusal is unchanged: {result:?}"
        );
        assert!(diagnostic_events(&f).is_empty());
        assert!(!f
            .session
            .lock()
            .unwrap()
            .projection()
            .unwrap()
            .objects
            .get("Hold")
            .is_some_and(|holds| holds.values().any(|hold| hold["scope"] == "TaskBranch")));
    }
}
#[test]
fn passed_postappend_main_advance_refuses_install_without_losing_actual() {
    let mut f = fixture::claimed();
    let previous = f.catalog.revision.clone();
    let tree = git(f.root.path(), &["rev-parse", "main^{tree}"]);
    let next = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &previous,
            "-m",
            "real main advance after Passed",
        ],
    );
    let root = f.root.path().to_owned();
    let old = previous.clone();
    let changed = next.clone();
    let (postappend_reads, clock) = observed_passed_clock_with(&f, NOW + 20, move || {
        git(&root, &["update-ref", "refs/heads/main", &changed, &old]);
    });
    let result = task_gates::check_recorded_shared(
        &f.session,
        &f.catalog,
        "host",
        None,
        &mut f.outbox,
        clock,
    );
    // Restore only our actual ref update even if the refusal assertion fails.
    // The mutation occurs after confirmation, outside the original G2 proof.
    if postappend_reads.get().is_some() {
        git(
            f.root.path(),
            &["update-ref", "refs/heads/main", &previous, &next],
        );
    }
    assert!(
        postappend_reads.get().is_some(),
        "reach the new Passed phase: {result:?}"
    );
    assert!(
        matches!(result, Err(GateError::Infrastructure(_))),
        "{result:?}"
    );
    let checks = diagnostic_events(&f);
    assert_eq!(checks.len(), 1);
    let actual = &checks[0];
    assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(actual));
    let record = decode(&recovery_outbox::original_of(actual)).unwrap();
    assert_eq!(record.evidence.result, GateResult::Passed);
    assert!(record.hold.is_none());
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| { matches!(event.kind.as_str(), "finish.started" | "finish.running") }));
    // A real complete synchronization after restoring the exact original source
    // must recover the accepted original; it must not append a replacement.
    ProjectSession::sync_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        NOW + 21,
    )
    .unwrap();
    assert_eq!(diagnostic_events(&f), checks);
    let projection = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(
        projection.objects["Evidence"][&record.evidence.id]["result"],
        "passed"
    );
    assert_eq!(projection.objects["Task"]["T-ask"]["state"], "running");
}
#[test]
fn task_check_cards_are_read_only_and_missing_original_git_stays_unresolved_history() {
    let (mut f, base) = committed(true);
    check(&mut f, NOW + 20).unwrap_err();
    let event = diagnostic_events(&f)[0].clone();
    let record = decode(&recovery_outbox::original_of(&event)).unwrap();
    let hold_id = record.hold.as_ref().unwrap()["id"].as_str().unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let digest = inputs.digest().to_owned();
    let main = git(f.root.path(), &["rev-parse", "main"]);
    let index = fs::read(f.catalog.common_dir.join("index")).unwrap();
    let cursor = f.session.lock().unwrap().applied_seq();
    for _ in 0..2 {
        let view =
            ProjectSession::read_task_progress(&f.catalog, &inputs, NOW + 21, Some(&f.session))
                .unwrap();
        assert!(view.holds.iter().any(|hold| hold["id"] == hold_id));
    }
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    assert_eq!(git(f.root.path(), &["rev-parse", "main"]), main);
    assert_eq!(fs::read(f.catalog.common_dir.join("index")).unwrap(), index);
    assert_eq!(f.session.lock().unwrap().applied_seq(), cursor);
    git(f.root.path(), &["reset", "--hard", &base]);
    // Remove only this disposable loose Task commit to model a cold clone or
    // pruned amended branch; no gc subprocess/background maintenance is used.
    let missing = &record.context.commit;
    fs::remove_file(
        f.catalog
            .common_dir
            .join("objects")
            .join(&missing[..2])
            .join(&missing[2..]),
    )
    .unwrap();
    let view = ProjectSession::read_task_progress(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        NOW + 22,
        Some(&f.session),
    )
    .unwrap();
    let raw = view
        .holds
        .iter()
        .find(|hold| hold["id"] == hold_id)
        .unwrap();
    assert_eq!(raw["source_state"], "historical_only");
    assert!(
        raw.get("resolved_by").is_none(),
        "missing provenance is never a pass or resolution"
    );
    check(&mut f, NOW + 23).unwrap();
    let state = f.session.lock().unwrap().projection().unwrap().clone();
    assert!(
        state.objects["Hold"][hold_id].get("resolved_by").is_some(),
        "only a new actual check resolves the old card"
    );
}
#[test]
fn actual_not_latest_main_is_a_g5_diagnostic_and_never_a_terminal_g5_capability() {
    let mut f = fixture::claimed();
    let old = f.catalog.revision.clone();
    let tree = git(f.root.path(), &["rev-parse", "main^{tree}"]);
    let next = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &old,
            "-m",
            "actual independent main advance",
        ],
    );
    git(
        f.root.path(),
        &["update-ref", "refs/heads/main", &next, &old],
    );
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_eq!(
        check(&mut f, NOW + 20).unwrap_err(),
        GateError::NotLatestMain
    );
    let events = diagnostic_events(&f);
    assert_eq!(events.len(), 1);
    let record = decode(&recovery_outbox::original_of(&events[0])).unwrap();
    assert_eq!(record.evidence.gate, Gate::G5);
    assert_eq!(record.evidence.result, GateResult::Failed);
    assert_eq!(record.hold.unwrap()["kind"], "gatefailed");
    assert!(events[0].data.get("integration_gate").is_none());
}

fn finish(f: &mut Fixture, id: &str, now: u64) -> Result<crate::finish::FinishStart, String> {
    ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        id,
        None,
        &mut f.outbox,
        || Ok(now),
    )
}

#[test]
fn finish_first_g2_refusal_records_the_original_error_without_starting_or_repeating_a_run() {
    for protected in [true, false] {
        let (mut f, _) = committed(protected);
        let id = uuid::Uuid::now_v7().to_string();
        let original = task_gates::check_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            "host",
            None,
            || Ok(NOW + 20),
        )
        .unwrap_err();
        let expected = format!("finish check: {original:?}");
        assert_eq!(finish(&mut f, &id, NOW + 21).unwrap_err(), expected);
        let events = diagnostic_events(&f);
        assert_eq!(events.len(), 1);
        let record = decode(&recovery_outbox::original_of(&events[0])).unwrap();
        assert_eq!(record.hold.as_ref().unwrap()["kind"], "forbidden");
        assert_eq!(record.evidence.gate, Gate::G2);
        assert_eq!(record.evidence.result, GateResult::Failed);
        assert_eq!(events[0].guards, record.context.run.leases);
        let before_retry = f.session.lock().unwrap().projection().unwrap().clone();
        let inputs = f.outbox.read_inputs().unwrap();
        let digest = inputs.digest().to_owned();
        assert_eq!(before_retry.objects["Task"]["T-ask"]["state"], "running");
        assert_eq!(before_retry.objects["Run"].len(), 1);
        assert!(!inputs
            .confirmed()
            .iter()
            .any(|event| event.kind == "finish.started"));
        assert_eq!(finish(&mut f, &id, NOW + 22).unwrap_err(), expected);
        assert_eq!(diagnostic_events(&f), events);
        assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
        assert_eq!(
            f.session.lock().unwrap().projection().unwrap().objects,
            before_retry.objects,
            "a rejected original UUID cannot mint another Run or mutate the same-tree diagnosis",
        );
    }
}

#[test]
fn finish_fix_uses_fresh_diagnostic_inputs_and_existing_uuid_retries_remain_pure() {
    let (mut f, _) = committed(true);
    let id = uuid::Uuid::now_v7().to_string();
    assert!(finish(&mut f, &id, NOW + 20)
        .unwrap_err()
        .starts_with("finish check: Protected("));
    let rejected = decode(&recovery_outbox::original_of(&diagnostic_events(&f)[0])).unwrap();
    let hold_id = rejected.hold.unwrap()["id"].as_str().unwrap().to_owned();
    git(f.root.path(), &["revert", "--no-edit", "HEAD"]);
    assert!(
        f.session.lock().unwrap().projection().unwrap().objects["Hold"][&hold_id]
            .get("resolved_by")
            .is_none()
    );
    let crate::finish::FinishStart::Queued(ticket) = finish(&mut f, &id, NOW + 21).unwrap() else {
        panic!("the fixed actual tree must retain ordinary queued admission");
    };
    let status = ticket.status().clone();
    assert_eq!(status.phase, crate::finish::JobPhase::Queued);
    assert!(
        !status.checked_gates["G3"],
        "admission is not verifier completion"
    );
    let checks = diagnostic_events(&f);
    assert_eq!(checks.len(), 2);
    let passed = decode(&recovery_outbox::original_of(&checks[1])).unwrap();
    assert_eq!(passed.evidence.result, GateResult::Passed);
    let before_retry = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(
        before_retry.objects["Hold"][&hold_id]["resolved_by"],
        passed.evidence.id
    );
    assert_eq!(before_retry.objects["Run"].len(), 1);
    assert_eq!(before_retry.objects["Task"]["T-ask"]["state"], "verifying");
    let inputs = f.outbox.read_inputs().unwrap();
    let digest = inputs.digest().to_owned();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "finish.started")
            .count(),
        1
    );
    let crate::finish::FinishStart::Queued(retry) = finish(&mut f, &id, NOW + 22).unwrap() else {
        panic!("the original UUID must retain its existing queued receipt");
    };
    assert_eq!(retry.status(), &status);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    assert_eq!(diagnostic_events(&f), checks);
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects,
        before_retry.objects
    );
    // A stale retry is still a pure frozen check; it cannot record another
    // diagnosis, start another job or turn its new tree into the old input.
    fs::write(
        f.root.path().join("alpha/after-queue.txt"),
        "actual later tree",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/after-queue.txt"]);
    git(f.root.path(), &["commit", "-qm", "change after queue"]);
    assert!(finish(&mut f, &id, NOW + 23).is_err());
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), digest);
    assert_eq!(diagnostic_events(&f), checks);
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects,
        before_retry.objects
    );
}

#[test]
fn finish_first_not_latest_main_keeps_g5_refusal_without_terminal_capability() {
    let mut f = fixture::claimed();
    let old = f.catalog.revision.clone();
    let tree = git(f.root.path(), &["rev-parse", "main^{tree}"]);
    let next = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &old,
            "-m",
            "main advances before first finish",
        ],
    );
    git(
        f.root.path(),
        &["update-ref", "refs/heads/main", &next, &old],
    );
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_eq!(
        finish(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 20).unwrap_err(),
        "finish check: NotLatestMain",
    );
    let checks = diagnostic_events(&f);
    assert_eq!(checks.len(), 1);
    let record = decode(&recovery_outbox::original_of(&checks[0])).unwrap();
    assert_eq!(record.evidence.gate, Gate::G5);
    assert_eq!(record.hold.unwrap()["kind"], "gatefailed");
    assert!(checks[0].data.get("integration_gate").is_none());
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "finish.started"));
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Task"]["T-ask"]["state"],
        "running"
    );
}

fn checkpoint_and_reopen(f: Fixture, now: u64) -> Fixture {
    let mut f = f;
    let expected = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(now),
                &mut f.outbox,
            )
            .unwrap()
            .status,
        "written",
        "the real writer must pass its unchanged strict Human/Hold replay comparison",
    );
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    let saved = crate::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    assert_eq!(saved.snapshot.projection.objects, expected.objects);
    assert_eq!(saved.snapshot.projection.human, expected.human);
    let f = fixture::reopen(f, now + 1);
    let reopened = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(reopened.objects, expected.objects);
    assert_eq!(reopened.human, expected.human);
    f
}

#[test]
fn passed_finish_admission_checkpoints_without_inventing_an_empty_hold_map() {
    let mut f = fixture::claimed();
    let id = uuid::Uuid::now_v7().to_string();
    let (postappend_reads, clock) = observed_passed_clock(&f, NOW + 20);
    let crate::finish::FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        clock,
    )
    .unwrap() else {
        panic!("actual finish must queue before verification");
    };
    assert_eq!(
        Some(f.session.lock().unwrap().authority_reads()),
        postappend_reads.get(),
        "first Passed and Started retain the already proved prefix",
    );
    let original_status = ticket.status().clone();
    assert!(!original_status.checked_gates["G3"]);
    let checks = diagnostic_events(&f);
    assert_eq!(checks.len(), 1);
    let record = decode(&recovery_outbox::original_of(&checks[0])).unwrap();
    assert_eq!(record.evidence.result, GateResult::Passed);
    assert!(record.hold.is_none());
    let runtime = f.session.lock().unwrap().projection().unwrap().clone();
    assert!(!runtime.objects.contains_key("Hold"));
    // Reproduce the CLI regression: the next real checkpoint follows the
    // first finish's Passed G2 original, without running any verifier.
    let mut f = checkpoint_and_reopen(f, NOW + 21);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .contains_key("Hold"));
    let crate::finish::FinishStart::Queued(retry) = finish(&mut f, &id, NOW + 23).unwrap() else {
        panic!("checkpoint/reopen must preserve the original queued UUID");
    };
    assert_eq!(retry.status(), &original_status);
    assert_eq!(diagnostic_events(&f), checks);
}

#[test]
fn diagnostic_checkpoint_preserves_real_human_hold_and_resolved_forbidden_audit() {
    let (mut f, _) = committed(true);
    assert!(matches!(
        check(&mut f, NOW + 20),
        Err(GateError::Protected(_))
    ));
    fixture::call(
        &mut f,
        &uuid::Uuid::now_v7().to_string(),
        &fixture::open("保留这个实际人工问答，不能被自动诊断取代"),
        NOW + 21,
    );
    let before = f.session.lock().unwrap().projection().unwrap().clone();
    let canonical = before.human.as_ref().unwrap().clone();
    assert_eq!(canonical.openings.len(), 1);
    let human_id = canonical.openings.keys().next().unwrap().clone();
    let human_hold = before.objects["Hold"][&human_id].clone();
    let rejected = decode(&recovery_outbox::original_of(&diagnostic_events(&f)[0])).unwrap();
    let forbidden_id = rejected.hold.unwrap()["id"].as_str().unwrap().to_owned();
    git(f.root.path(), &["revert", "--no-edit", "HEAD"]);
    assert!(!check(&mut f, NOW + 22).unwrap().gates["G3"]);
    let runtime = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(runtime.objects["Hold"][&human_id], human_hold);
    assert_eq!(runtime.human.as_ref(), Some(&canonical));
    assert!(runtime.objects["Hold"][&forbidden_id]
        .get("resolved_by")
        .is_some());
    assert_eq!(runtime.objects["Hold"].len(), 2);
    let f = checkpoint_and_reopen(f, NOW + 23);
    let reopened = f.session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(reopened.objects["Hold"][&human_id], human_hold);
    assert_eq!(
        reopened.objects["Hold"][&forbidden_id],
        runtime.objects["Hold"][&forbidden_id]
    );
    assert_eq!(reopened.human.as_ref(), Some(&canonical));
}
