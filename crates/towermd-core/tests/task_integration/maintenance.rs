//! Task5 uses the actual Task4 fixture/producers, not constructed proof DTOs.
use super::*;
use std::collections::BTreeSet;
use towermd_core::project_session::{FinishExpiry, FinishMaintenance};

fn maintenance(f: &mut Fixture, owned: &BTreeSet<String>, now: u64) -> FinishMaintenance {
    ProjectSession::maintain_finishes_shared(
        &f.session,
        &f.catalog,
        "host",
        owned,
        Some("actual test remote unavailable"),
        &mut f.outbox,
        || Ok(now),
    )
    .unwrap()
}
fn expire(
    f: &mut Fixture,
    plan: FinishExpiry,
    now: u64,
) -> towermd_core::project_session::FinishExpiryOutcome {
    ProjectSession::expire_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        plan,
        &mut f.outbox,
        || Ok(now),
    )
    .unwrap()
    .expect("actual durable expiry")
}
fn queued() -> (Fixture, finish::FinishStatus) {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let id = uuid::Uuid::now_v7().to_string();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 30),
    )
    .unwrap() else {
        panic!("actual new queued start");
    };
    let status = ticket.status().clone();
    // This core test has no executor; dropping the unconsumed ticket proves no
    // process needs draining. Daemon tests exercise real owned cancellation.
    drop(ticket);
    (f, status)
}

#[test]
fn actual_queued_minutes_keep_all_original_fences_and_fixed_deadline_then_expire() {
    let (mut f, status) = queued();
    let owned = BTreeSet::from([status.input.evidence_id.clone()]);
    let entered = status.input.entered_verifying_at;
    for minute in [14_u64, 28, 42, 56, 59] {
        let at = entered + minute * 60_000;
        let pass = maintenance(&mut f, &owned, at);
        assert!(pass.expirations.is_empty());
        assert_eq!(pass.renewed.len(), 1);
        assert_eq!(pass.renewed[0].kind, "lease.renewed");
        let leases = f.session.lock().unwrap().leases(at);
        for fence in &status.input.fences {
            let lease = &leases[&fence.name];
            assert_eq!(lease.fence(), *fence);
            assert_eq!(lease.expires_at, at + 900_000);
        }
        let state = projection(&f);
        assert_eq!(
            state.objects["Run"][&status.input.run_id]["finish"]["merge_deadline"],
            status.input.merge_deadline
        );
        assert_eq!(
            state.objects["Run"][&status.input.run_id]["finish"]["entered_verifying_at"],
            entered
        );
    }
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind.starts_with("lease.")));
    let before = projection(&f);
    let mut due = maintenance(&mut f, &owned, status.input.merge_deadline);
    assert!(due.renewed.is_empty());
    assert_eq!(due.expirations.len(), 1);
    let plan = due.expirations.remove(0);
    assert_eq!(plan.run_id(), status.input.run_id);
    assert_eq!(
        plan.evidence_ids(),
        std::slice::from_ref(&status.input.evidence_id)
    );
    assert_eq!(plan.deadline(), status.input.merge_deadline);
    let result = expire(&mut f, plan, status.input.merge_deadline);
    assert_eq!(result.actual.kind, "task.released");
    assert_eq!(result.actual.data["run"]["state"], "expired");
    assert_eq!(result.actual.data["task"]["state"], "ready");
    assert_eq!(
        result.actual.data["finish_expiry"]["authority_unavailable"],
        "actual test remote unavailable"
    );
    let after = projection(&f);
    let mut restored = after.objects["Run"][&status.input.run_id].clone();
    restored["state"] = before.objects["Run"][&status.input.run_id]["state"].clone();
    assert_eq!(
        restored, before.objects["Run"][&status.input.run_id],
        "all other Run fields remain exact"
    );
    assert_eq!(
        after.objects["Run"][&status.input.run_id]["finish"]["failures"],
        0
    );
    assert!(!after
        .objects
        .get("Evidence")
        .into_iter()
        .flat_map(|all| all.values())
        .any(|raw| raw["kind"] == "verification"));
    for fence in &status.input.fences {
        assert!(!f
            .session
            .lock()
            .unwrap()
            .leases(status.input.merge_deadline)
            .contains_key(&fence.name));
    }
    assert!(f
        .session
        .lock()
        .unwrap()
        .renew_binding(
            &f.catalog,
            "host",
            Some(&status.input.run_id),
            status.input.merge_deadline
        )
        .is_err());
    let view = ProjectSession::finish_view_shared(
        &f.session,
        &f.catalog,
        "host",
        &status.request_id,
        &mut f.outbox,
        || Ok(status.input.merge_deadline + 1),
    )
    .unwrap()
    .unwrap();
    assert_eq!(view.run_state, "expired");
    assert!(view.evidence.is_none());
    assert_eq!(view.expiry_actual.as_ref(), Some(&result.actual));
    assert!(
        view.actual.is_none(),
        "expiry is not a verification terminal receipt"
    );
    let again = maintenance(&mut f, &BTreeSet::new(), status.input.merge_deadline + 2);
    assert!(again.expirations.is_empty());
    assert!(again
        .expired
        .iter()
        .any(|observed| observed.actual == result.actual));
}

#[test]
fn passed_wait_does_not_gain_background_renewal_or_change_the_first_deadline() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let deadline = done.status.input.merge_deadline;
    let pass = maintenance(&mut f, &BTreeSet::new(), deadline - 60_000);
    assert!(pass.renewed.is_empty());
    assert!(pass.expirations.is_empty());
    let wait = ProjectSession::finish_view_shared(
        &f.session,
        &f.catalog,
        "host",
        &done.status.request_id,
        &mut f.outbox,
        || Ok(deadline - 1),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        wait.evidence.as_ref().unwrap(),
        done.evidence.as_ref().unwrap()
    );
    assert_eq!(wait.status.input.merge_deadline, deadline);
    assert!(
        wait.expiry_actual.is_none(),
        "an unexpired finish has no expiry audit"
    );
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    let expiry = expire(&mut f, due.expirations.remove(0), deadline);
    let expired_view = ProjectSession::finish_view_shared(
        &f.session,
        &f.catalog,
        "host",
        &done.status.request_id,
        &mut f.outbox,
        || Ok(deadline + 1),
    )
    .unwrap()
    .unwrap();
    assert_eq!(expired_view.run_state, "expired");
    assert_eq!(expired_view.expiry_actual, Some(expiry.actual));
    assert_eq!(expired_view.actual, wait.actual);
    assert_eq!(expired_view.evidence, wait.evidence);
    assert_eq!(expired_view.report, wait.report);
    let state = projection(&f);
    assert_eq!(
        state.objects["Run"][&done.status.input.run_id]["state"],
        "expired"
    );
    assert_eq!(
        state.objects["Evidence"][&done.status.input.evidence_id],
        serde_json::to_value(done.evidence.unwrap()).unwrap()
    );
    assert_eq!(
        state.objects["Run"][&done.status.input.run_id]["finish"]["failures"],
        0
    );
}

#[test]
fn real_main_merge_first_observed_at_deadline_wins_before_expiry() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let output = trailers(&mut f, NOW + 40).unwrap();
    let commit = amend_and_ff(&mut f, &output.trailers);
    let result = maintenance(&mut f, &BTreeSet::new(), done.status.input.merge_deadline);
    assert!(result.expirations.is_empty());
    let state = projection(&f);
    assert_eq!(state.objects["Task"]["T-finish"]["state"], "merged");
    assert_eq!(
        state.objects["Run"][&done.status.input.run_id]["state"],
        "ended"
    );
    let inputs = f.outbox.read_inputs().unwrap();
    let merged = inputs
        .confirmed()
        .iter()
        .filter(|event| event.kind == "merge.done")
        .collect::<Vec<_>>();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].data["commit"], commit);
    assert!(!inputs
        .confirmed()
        .iter()
        .any(|event| event.data.get("finish_expiry").is_some()));
}

#[test]
fn expired_run_remains_a_historical_candidate_and_never_reverifies() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let output = trailers(&mut f, NOW + 40).unwrap();
    let deadline = done.status.input.merge_deadline;
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    let expired = expire(&mut f, due.expirations.remove(0), deadline);
    assert!(f
        .session
        .lock()
        .unwrap()
        .has_task_merge_candidates()
        .unwrap());
    amend_and_ff(&mut f, &output.trailers);
    assert_eq!(observe(&mut f, deadline + 1).unwrap().merged.len(), 1);
    assert!(observe(&mut f, deadline + 2).unwrap().merged.is_empty());
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "finish.completed")
            .count(),
        1
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "merge.done")
            .count(),
        1
    );
    assert!(inputs.confirmed().contains(&expired.actual));
    assert_eq!(
        projection(&f).objects["Run"][&done.status.input.run_id]["state"],
        "ended"
    );
}

#[test]
fn late_historical_fact_waits_for_a_new_active_run_without_touching_its_epoch() {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    let done = completed(&mut f, &uuid::Uuid::now_v7().to_string(), NOW + 30);
    let output = trailers(&mut f, NOW + 40).unwrap();
    let deadline = done.status.input.merge_deadline;
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    expire(&mut f, due.expirations.remove(0), deadline);
    let next = claim(&mut f, deadline + 1).unwrap();
    let new_id = next["id"].as_str().unwrap().to_string();
    assert_ne!(new_id, done.status.input.run_id);
    let new_fences = next["leases"].clone();
    amend_and_ff(&mut f, &output.trailers);
    let blocked = observe(&mut f, deadline + 2).unwrap();
    assert!(blocked.merged.is_empty());
    assert!(blocked
        .blocked
        .iter()
        .any(|diagnostic| diagnostic.reason.contains("historical_merge_conflict")));
    let current = projection(&f);
    assert_eq!(current.objects["Task"]["T-finish"]["state"], "running");
    assert_eq!(current.objects["Run"][&new_id]["leases"], new_fences);
    for fence in
        serde_json::from_value::<Vec<towermd_core::hub_protocol::Fence>>(new_fences).unwrap()
    {
        assert_eq!(
            f.session.lock().unwrap().leases(deadline + 2)[&fence.name].fence(),
            fence
        );
    }
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        Some(&new_id),
        deadline + 3,
        &mut f.outbox,
    )
    .unwrap();
    assert_eq!(observe(&mut f, deadline + 4).unwrap().merged.len(), 1);
    assert_eq!(projection(&f).objects["Run"][&new_id]["state"], "released");
}

#[test]
fn actual_expiry_acceptance_with_confirmation_failure_repairs_one_original_receipt() {
    let (f, status) = queued();
    let deadline = status.input.merge_deadline;
    let (mut f, capture) = interrupt_finish_confirmation(f, "task.released", deadline - 1);
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    let error = ProjectSession::expire_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        due.expirations.remove(0),
        &mut f.outbox,
        || Ok(deadline),
    )
    .unwrap_err();
    assert!(error.contains("uncertain"), "{error}");
    let accepted = restore_interrupted_finish(&capture);
    assert_eq!(accepted.actual.data["run"]["state"], "expired");
    let actual_before = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let restored = maintenance(&mut f, &BTreeSet::new(), deadline + 1);
    assert!(restored
        .expired
        .iter()
        .any(|outcome| outcome.actual == accepted.actual));
    assert_eq!(
        f.session
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq(),
        actual_before
    );
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.eid == accepted.actual.eid)
            .collect::<Vec<_>>(),
        vec![&accepted.actual]
    );
    let mut changed = accepted.original;
    changed.data["finish_expiry"]["merge_deadline"] = serde_json::json!(deadline + 1);
    assert!(
        f.outbox.append_pending(&changed).is_err(),
        "same original identity cannot change its deadline/body"
    );
    assert_eq!(
        projection(&f).objects["Run"][&status.input.run_id]["finish"]["failures"],
        0
    );
}

#[test]
fn old_expiry_releases_no_replacement_epoch_and_reopen_reconstructs_its_receipt() {
    use towermd_core::hub_protocol::AcquireLease;
    let (mut f, status) = queued();
    let deadline = status.input.merge_deadline;
    let replacement = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .lease_acquire(
            AcquireLease {
                host: "host".into(),
                names: status
                    .input
                    .fences
                    .iter()
                    .map(|fence| fence.name.clone())
                    .collect(),
                holder: uuid::Uuid::now_v7().to_string(),
                ttl_ms: 900_000,
            },
            deadline,
        )
        .unwrap()
        .leases;
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    let expired = expire(&mut f, due.expirations.remove(0), deadline);
    for lease in &replacement {
        assert_eq!(
            f.session.lock().unwrap().leases(deadline)[&lease.name],
            *lease
        );
    }
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(deadline + 1),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    drop(f.session);
    f.session = Arc::new(std::sync::Mutex::new(
        ProjectSession::open(
            &f.catalog.common_dir.join("towermd"),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            deadline + 2,
        )
        .unwrap(),
    ));
    let restored = maintenance(&mut f, &BTreeSet::new(), deadline + 3);
    assert!(restored
        .expired
        .iter()
        .any(|outcome| outcome.actual == expired.actual));
    for lease in &replacement {
        assert_eq!(
            f.session.lock().unwrap().leases(deadline + 3)[&lease.name],
            *lease
        );
    }
}

#[test]
fn prepared_expiry_cannot_cross_actual_main_or_session_identity_changes() {
    for replace_session in [false, true] {
        let (mut f, status) = queued();
        let deadline = status.input.merge_deadline;
        let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
        if replace_session {
            drop(f.session);
            f.session = Arc::new(std::sync::Mutex::new(
                ProjectSession::open(
                    &f.catalog.common_dir.join("towermd"),
                    &f.catalog,
                    &f.outbox.read_inputs().unwrap(),
                    deadline,
                )
                .unwrap(),
            ));
        } else {
            git(
                f.root.path(),
                &[
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "real source changed during owned cleanup",
                ],
            );
            let changed = git(f.root.path(), &["rev-parse", "HEAD"]);
            git(f.root.path(), &["update-ref", "refs/heads/main", &changed]);
        }
        assert!(ProjectSession::expire_finish_shared(
            &f.session,
            &f.catalog,
            "host",
            due.expirations.remove(0),
            &mut f.outbox,
            || Ok(deadline + 1)
        )
        .is_err());
        assert!(!f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .any(|event| event.data.get("finish_expiry").is_some()));
        assert_eq!(
            projection(&f).objects["Run"][&status.input.run_id]["state"],
            "verifying"
        );
    }
}

#[test]
fn app_rebuild_diagnoses_pending_only_expiry_instead_of_replaying_it_as_a_ready_fact() {
    let (mut f, status) = queued();
    // The real started source is durable before the uncertain later release.
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 40),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let deadline = status.input.merge_deadline;
    let (mut f, capture) = interrupt_finish_confirmation(f, "task.released", deadline - 1);
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    assert!(ProjectSession::expire_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        due.expirations.remove(0),
        &mut f.outbox,
        || Ok(deadline)
    )
    .is_err());
    let interrupted = restore_interrupted_finish(&capture);
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .contains(&interrupted.original));
    // Discard only the real uncheckpointed Hub. There is now no complete actual
    // receipt proving acceptance; the frozen pending original cannot supply it.
    drop(f.session);
    fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let request = towermd_core::recovery::prepare(
        &f.catalog,
        &inputs,
        deadline + 1,
        deadline + 100_000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (_, installed) = f
        .writer
        .confirm_rebuild(
            None,
            &f.catalog,
            &request,
            "human:recovery-owner",
            request.expires_at,
            &inputs,
            "host",
            || Ok(deadline + 2),
        )
        .unwrap();
    f.session = installed;
    let events = f.session.lock().unwrap().events(0);
    assert!(!events
        .iter()
        .any(|event| event.eid == interrupted.original.eid));
    // Recovery's own explicit old-Run invalidation can release its old epoch;
    // this assertion specifically forbids the pending expiry from becoming fact.
    let current = f.session.lock().unwrap().projection().unwrap().clone();
    assert_ne!(
        current.objects["Run"][&status.input.run_id]["state"],
        "expired"
    );
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .pending()
        .contains(&interrupted.original));
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.eid == interrupted.original.eid));
    let saved =
        towermd_core::snapshot_service::read_snapshot(&Catalog::load(f.root.path()).unwrap())
            .unwrap()
            .unwrap();
    let archived = saved
        .archives
        .values()
        .flat_map(|text| text.lines())
        .map(|line| serde_json::from_str::<towermd_core::hub_protocol::Event>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(!archived
        .iter()
        .any(|event| event.eid == interrupted.original.eid));
    assert!(archived.iter().any(|event| event.data["code"]
        == "expiry_intent_requires_owned_cleanup"
        && event.data["original_eid"] == interrupted.original.eid));
}

#[test]
fn actual_expiry_body_preserves_complete_task_and_run_preimages() {
    let (mut f, status) = queued();
    let before = projection(&f);
    let deadline = status.input.merge_deadline;
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    let actual = expire(&mut f, due.expirations.remove(0), deadline).actual;
    let mut accepted = before.clone();
    accepted.apply_recorded(&actual).unwrap();
    assert_eq!(
        accepted.objects["Run"][&status.input.run_id]["state"],
        "expired"
    );
    for (object, field, changed) in [
        ("task", "title", serde_json::json!("different task title")),
        ("task", "allowed_paths", serde_json::json!(["**"])),
        (
            "task",
            "unknown_expiry_field",
            serde_json::json!({"preserve":"not a new field"}),
        ),
        (
            "run",
            "worktree",
            serde_json::json!("/different/actual/worktree"),
        ),
        ("run", "branch", serde_json::json!("different-task-branch")),
        ("run", "unknown_expiry_field", serde_json::json!(true)),
    ] {
        let mut changed_event = actual.clone();
        changed_event.data[object][field] = changed;
        let mut state = before.clone();
        let error = state.apply_recorded(&changed_event).unwrap_err();
        assert!(
            error.contains("changed fields other than"),
            "{object}.{field}: {error}"
        );
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            serde_json::to_value(&before).unwrap(),
            "a rejected complete expiry body must not partially change the projection"
        );
    }
}

#[test]
fn accepted_old_expiry_never_overwrites_a_new_actual_claim() {
    let (mut f, status) = queued();
    let deadline = status.input.merge_deadline;
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    let actual = expire(&mut f, due.expirations.remove(0), deadline).actual;
    let next = claim(&mut f, deadline + 1).unwrap();
    let before = projection(&f);
    let mut state = before.clone();
    let error = state.apply_recorded(&actual).unwrap_err();
    assert!(error.contains("another active Run"), "{error}");
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    let restored = maintenance(&mut f, &BTreeSet::new(), deadline + 2);
    assert!(
        restored
            .expired
            .iter()
            .any(|outcome| outcome.actual == actual),
        "the original historical receipt still exists without becoming a new Ready fact"
    );
    assert_eq!(
        projection(&f).objects["Task"]["T-finish"]["state"],
        "running"
    );
    let next_id = next["id"].as_str().unwrap();
    assert_eq!(
        projection(&f).objects["Run"][next_id]["leases"],
        next["leases"]
    );
    for fence in
        serde_json::from_value::<Vec<towermd_core::hub_protocol::Fence>>(next["leases"].clone())
            .unwrap()
    {
        assert_eq!(
            f.session.lock().unwrap().leases(deadline + 2)[&fence.name].fence(),
            fence
        );
    }
}

fn queued_after_actual_prestart_snapshot() -> (Fixture, finish::FinishStatus) {
    let mut f = claimed(&["exit 0"], false);
    commit_code(&f);
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 21),
                &mut f.outbox
            )
            .unwrap()
            .status,
        "written"
    );
    git(f.root.path(), &["rebase", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        None,
        &mut f.outbox,
        || Ok(NOW + 30),
    )
    .unwrap() else {
        panic!("actual start after source snapshot");
    };
    let status = ticket.status().clone();
    drop(ticket);
    let source = towermd_core::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let started = inputs
        .confirmed()
        .iter()
        .find(|event| event.eid == status.started_eid)
        .unwrap();
    assert!(source.snapshot.through_seq < started.seq);
    (f, status)
}

fn actual_app_rebuild(mut f: Fixture, now: u64, delete_outbox: bool) -> Fixture {
    drop(f.session);
    fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    if delete_outbox {
        drop(f.outbox);
        fs::remove_dir_all(f.runtime.path().join("outbox")).unwrap();
        f.outbox = towermd_core::recovery_outbox::RecoveryOutbox::open(
            &f.runtime.path().join("outbox"),
            towermd_core::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
        )
        .unwrap();
    }
    let inputs = f.outbox.read_inputs().unwrap();
    let request = towermd_core::recovery::prepare(
        &f.catalog,
        &inputs,
        now,
        now + 100_000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (_, installed) = f
        .writer
        .confirm_rebuild(
            None,
            &f.catalog,
            &request,
            "human:recovery-owner",
            request.expires_at,
            &inputs,
            "host",
            || Ok(now + 1),
        )
        .unwrap();
    f.session = installed;
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    f
}

#[test]
fn twice_actual_app_rebuild_without_old_outbox_preserves_only_historical_expiry() {
    let (mut f, status) = queued_after_actual_prestart_snapshot();
    let deadline = status.input.merge_deadline;
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline);
    let expiry = expire(&mut f, due.expirations.remove(0), deadline).actual;
    let mut f = actual_app_rebuild(f, deadline + 1, false);
    let first = ProjectSession::finish_view_shared(
        &f.session,
        &f.catalog,
        "host",
        &status.request_id,
        &mut f.outbox,
        || Ok(deadline + 3),
    )
    .unwrap()
    .unwrap();
    assert_eq!(first.run_state, "expired");
    assert_eq!(first.status.input, status.input);
    assert!(first.evidence.is_none());
    assert_eq!(first.expiry_actual.as_ref(), Some(&expiry));
    assert!(
        first.actual.is_none(),
        "historical expiry never occupies terminal actual"
    );
    let saved = towermd_core::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let archive = saved
        .archives
        .values()
        .flat_map(|text| text.lines())
        .map(|line| serde_json::from_str::<towermd_core::hub_protocol::Event>(line).unwrap())
        .collect::<Vec<_>>();
    let audit = archive
        .iter()
        .find(|event| {
            event.kind == "request.receipt.restored" && event.data.get("finish_expiry").is_some()
        })
        .unwrap()
        .clone();
    assert_eq!(
        audit.data["original"],
        serde_json::to_value(&expiry).unwrap()
    );
    assert!(archive
        .iter()
        .any(|event| event.eid == status.started_eid && event.stale));
    assert!(
        !archive.iter().any(|event| event.eid == expiry.eid),
        "old Ready is not a new outer fact"
    );
    let before = projection(&f);
    assert_ne!(
        before.objects["Run"][&status.input.run_id]["state"],
        "expired"
    );
    assert!(f
        .session
        .lock()
        .unwrap()
        .renew_binding(&f.catalog, "host", Some(&status.input.run_id), deadline + 3)
        .is_err());

    f = actual_app_rebuild(f, deadline + 4, true);
    assert!(f.outbox.read_inputs().unwrap().confirmed().is_empty());
    let second = ProjectSession::finish_view_shared(
        &f.session,
        &f.catalog,
        "host",
        &status.request_id,
        &mut f.outbox,
        || Ok(deadline + 6),
    )
    .unwrap()
    .unwrap();
    assert_eq!(second.run_state, "expired");
    assert_eq!(second.status.input, status.input);
    assert_eq!(second.expiry_actual.as_ref(), Some(&expiry));
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&second).unwrap(),
        "the complete history view is independent of an erased outbox and repeated rebuild"
    );
    let restored = maintenance(&mut f, &BTreeSet::new(), deadline + 7);
    assert!(restored
        .expired
        .iter()
        .any(|result| result.actual == expiry));
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.eid == expiry.eid)
            .collect::<Vec<_>>(),
        vec![&expiry]
    );
    let after = projection(&f);
    assert_ne!(
        after.objects["Run"][&status.input.run_id]["state"],
        "expired"
    );
    assert_ne!(after.objects["Task"]["T-finish"]["state"], "merged");
    assert!(after
        .objects
        .get("Evidence")
        .is_none_or(|all| !all.contains_key(&status.input.evidence_id)));
    let saved = towermd_core::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let audits = saved
        .archives
        .values()
        .flat_map(|text| text.lines())
        .map(|line| serde_json::from_str::<towermd_core::hub_protocol::Event>(line).unwrap())
        .filter(|event| {
            event.kind == "request.receipt.restored" && event.data.get("finish_expiry").is_some()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        audits,
        vec![audit],
        "second rebuild preserves the same first audit/barrier/body without nesting"
    );
}

#[test]
fn late_cleaned_cancellation_cannot_interrupt_back_to_running_past_fixed_deadline() {
    let (mut f, status) = queued();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &status.request_id,
        None,
        &mut f.outbox,
        || Ok(NOW + 31),
    )
    .unwrap() else {
        panic!("original queued ticket only");
    };
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 32)
        })
        .unwrap();
    // Real explicit CLI-style activity keeps the same original fences alive;
    // this deliberately makes TTL expiry unable to hide the fixed-deadline bug.
    for minute in [14_u64, 28, 42, 56, 59] {
        f.session
            .lock()
            .unwrap()
            .renew_binding(
                &f.catalog,
                "host",
                Some(&status.input.run_id),
                status.input.entered_verifying_at + minute * 60_000,
            )
            .unwrap();
    }
    let deadline = status.input.merge_deadline;
    for fence in &status.input.fences {
        assert!(f.session.lock().unwrap().leases(deadline)[&fence.name].expires_at > deadline);
    }
    let cancelled = Arc::new(AtomicBool::new(true));
    let executed = finish::execute(execution, Duration::from_secs(10), cancelled);
    assert!(executed.cleanup_confirmed());
    let error = ProjectSession::record_borrowed_finish_shared(
        &f.session,
        &f.catalog,
        &executed,
        &mut f.outbox,
        || Ok(deadline),
    )
    .unwrap_err();
    assert!(
        error.contains(&status.request_id) && error.contains("deadline"),
        "{error}"
    );
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(!inputs.pending().iter().any(|event| matches!(
        event.kind.as_str(),
        "finish.completed" | "finish.interrupted"
    )));
    assert!(!inputs.confirmed().iter().any(|event| matches!(
        event.kind.as_str(),
        "finish.completed" | "finish.interrupted"
    )));
    assert_eq!(
        projection(&f).objects["Run"][&status.input.run_id]["finish"]["failures"],
        0
    );
    let mut due = maintenance(&mut f, &BTreeSet::new(), deadline + 1);
    expire(&mut f, due.expirations.remove(0), deadline + 1);
    assert_eq!(
        projection(&f).objects["Run"][&status.input.run_id]["state"],
        "expired"
    );
}

#[test]
fn already_accepted_interruption_keeps_its_original_receipt_after_deadline() {
    let (mut f, status) = queued();
    let interrupted = ProjectSession::interrupt_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &status.request_id,
        "actual queued worker was never executed",
        &mut f.outbox,
        || Ok(NOW + 31),
    )
    .unwrap();
    let original_actual = interrupted.actual;
    let retried = ProjectSession::interrupt_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &status.request_id,
        "later caller cannot rewrite the original diagnostic",
        &mut f.outbox,
        || Ok(status.input.merge_deadline + 1),
    )
    .unwrap();
    assert_eq!(retried.actual, original_actual);
    assert!(retried.evidence.is_none());
    assert_eq!(
        retried.status.input.merge_deadline,
        status.input.merge_deadline
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "finish.interrupted")
            .count(),
        1
    );
    assert_eq!(
        projection(&f).objects["Run"][&status.input.run_id]["finish"]["failures"],
        0
    );
}
