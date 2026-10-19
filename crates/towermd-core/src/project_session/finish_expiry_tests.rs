//! Actual finite expiry/rebuild receipts. Forged variants below are negatives;
//! no test-only producer capability or public proof API is introduced.
use super::*;
use crate::ask_fixture::{claimed, git, Fixture, NOW};
use crate::finish::expiry_receipt::{self as wire, Record};
use std::{collections::BTreeSet, fs, sync::Arc};

fn rebuild(mut f: Fixture, now: u64, erase_outbox: bool) -> Fixture {
    drop(f.session);
    fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    if erase_outbox {
        drop(f.outbox);
        fs::remove_dir_all(f.runtime.path().join("outbox")).unwrap();
        f.outbox = RecoveryOutbox::open(
            &f.runtime.path().join("outbox"),
            recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
        )
        .unwrap();
    }
    let inputs = f.outbox.read_inputs().unwrap();
    let request = crate::recovery::prepare(
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
fn audit_fixture() -> (Fixture, FinishStatus, Event, Event) {
    let mut f = claimed();
    assert_eq!(
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 15),
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
        || Ok(NOW + 20),
    )
    .unwrap() else {
        panic!("actual new queued producer");
    };
    let status = ticket.status().clone();
    drop(ticket);
    let deadline = status.input.merge_deadline;
    let mut due = ProjectSession::maintain_finishes_shared(
        &f.session,
        &f.catalog,
        "host",
        &BTreeSet::new(),
        None,
        &mut f.outbox,
        || Ok(deadline),
    )
    .unwrap();
    let expiry = ProjectSession::expire_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        due.expirations.remove(0),
        &mut f.outbox,
        || Ok(deadline),
    )
    .unwrap()
    .unwrap()
    .actual;
    let f = rebuild(f, deadline + 1, false);
    let saved = crate::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let audit = saved
        .archives
        .values()
        .flat_map(|text| text.lines())
        .map(|line| crate::hub_legacy::decode_archive_line(line).unwrap())
        .find(wire::is_wrapper)
        .unwrap();
    assert_eq!(audit.data["original"], json!(expiry));
    (f, status, expiry, audit)
}
fn recanonicalize(template: &Event, record: &Record) -> Event {
    let mut event = template.clone();
    let canonical = wire::canonical(record, &event.host).unwrap();
    event.eid = canonical.eid;
    event.data = canonical.data;
    event
}
fn append_invalid_audit(f: &mut Fixture, record: &Record, now: u64) -> String {
    let original = wire::canonical(record, "host").unwrap();
    let before = f.session.lock().unwrap().projection().unwrap().clone();
    let receipt = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(original, now)
        .unwrap();
    assert!(matches!(receipt, AppendReceipt::Event(_)));
    let error = ProjectSession::sync_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        now,
    )
    .unwrap_err();
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap(),
        &before,
        "an invalid historical audit never partially installs Task/Run state"
    );
    error.to_string()
}

#[test]
fn historical_expiry_family_is_strict_and_never_a_generic_original_or_actual_token() {
    let (mut f, status, expiry, audit) = audit_fixture();
    let record = wire::decode(f.catalog.config.project_id(), &audit)
        .unwrap()
        .unwrap();
    for field in ["request_body", "p3_request", "unknown_audit_key"] {
        let mut changed = audit.clone();
        changed.data[field] = json!({});
        assert!(
            crate::request_ledger::restored_original(f.catalog.config.project_id(), &changed)
                .is_err(),
            "{field}"
        );
    }
    let mut ordinary = record.clone();
    ordinary
        .original
        .data
        .as_object_mut()
        .unwrap()
        .remove("finish_expiry");
    assert!(wire::decode(
        f.catalog.config.project_id(),
        &recanonicalize(&audit, &ordinary)
    )
    .is_err());
    let mut mixed = record.clone();
    mixed.original.data["p3_request"] = json!({});
    assert!(wire::decode(
        f.catalog.config.project_id(),
        &recanonicalize(&audit, &mixed)
    )
    .is_err());
    for field in ["seq", "t", "stale", "guards"] {
        let mut changed = audit.clone();
        changed.data["original"][field] = match field {
            "seq" => json!(expiry.seq + 1),
            "t" => json!("2099-01-01T00:00:00Z"),
            "stale" => json!(true),
            _ => json!([{"name":"module:A-alpha","holder":"wrong","epoch":1}]),
        };
        assert!(
            wire::decode(f.catalog.config.project_id(), &changed).is_err(),
            "{field}"
        );
    }
    let inputs = f.outbox.read_inputs().unwrap();
    let history = history_shared(
        &f.session,
        &f.catalog,
        &inputs,
        status.input.merge_deadline + 4,
    )
    .unwrap();
    let outer = history.actual_outer_events(&f.catalog).unwrap();
    assert!(!outer.iter().any(|event| event.eid == expiry.eid));
    assert!(history
        .task_receipts(
            &f.catalog,
            outer.iter().map(|event| event.seq).max().unwrap(),
            &[recovery_outbox::original_of(&expiry)]
        )
        .unwrap()
        .is_empty());
    let old_start = record
        .finish_expiry
        .context
        .iter()
        .find(|event| event.eid == status.started_eid)
        .unwrap();
    let start_original = recovery_outbox::original_of(old_start);
    let actual_receipt = history
        .actual_finish_receipt(&f.catalog, &start_original)
        .unwrap()
        .unwrap();
    let start_actual = actual_receipt
        .event_for(f.catalog.config.project_id(), &start_original)
        .unwrap();
    assert!(start_actual.stale);
    assert_ne!(
        start_actual, old_start,
        "actual token preserves the new stale replay, not the audit's original role"
    );
    let actual_start = outer
        .iter()
        .find(|event| event.eid == status.started_eid)
        .unwrap();
    assert!(actual_start.stale);
    assert!(
        history
            .main_task_claims(
                &f.catalog,
                "host",
                &BTreeSet::from([status.input.run_id.clone()])
            )
            .unwrap()
            .iter()
            .all(|event| event.eid
                != record
                    .finish_expiry
                    .context
                    .iter()
                    .find(|event| event.kind == "task.claimed")
                    .unwrap()
                    .eid
                || outer.contains(event)),
        "the audit does not add a new generic claim role"
    );
    let view = ProjectSession::finish_view_shared(
        &f.session,
        &f.catalog,
        "host",
        &status.request_id,
        &mut f.outbox,
        || Ok(status.input.merge_deadline + 4),
    )
    .unwrap()
    .unwrap();
    assert_eq!(view.run_state, "expired");
    assert!(view.evidence.is_none());
    assert_eq!(view.expiry_actual.as_ref(), Some(&expiry));
    assert!(view.actual.is_none());
}

#[test]
fn actual_expiry_audit_source_and_barrier_changes_reject_even_with_a_new_canonical_hash() {
    for change in ["source", "barrier", "context", "body", "epoch"] {
        let (mut f, status, _, audit) = audit_fixture();
        let mut record = wire::decode(f.catalog.config.project_id(), &audit)
            .unwrap()
            .unwrap();
        match change {
            "source" => {
                git(
                    f.root.path(),
                    &[
                        "commit",
                        "--allow-empty",
                        "-qm",
                        "actual commit is not the selected source snapshot",
                    ],
                );
                record.finish_expiry.source_commit = git(f.root.path(), &["rev-parse", "HEAD"]);
            }
            "barrier" => record.finish_expiry.rebuild_eid = uuid::Uuid::now_v7().to_string(),
            "context" => {
                record
                    .finish_expiry
                    .context
                    .retain(|event| event.eid != status.started_eid);
            }
            "body" => record.original.data["task"]["title"] = json!("forged historical title"),
            "epoch" => {
                record.original.data["run"]["leases"][0]["epoch"] = json!(u64::MAX);
            }
            _ => unreachable!(),
        }
        let error = append_invalid_audit(&mut f, &record, status.input.merge_deadline + 4);
        assert!(
            error.contains("expiry")
                || error.contains("source")
                || error.contains("rebuild")
                || (change == "source"
                    && error.contains("snapshot trailer is not the actual state commit")),
            "{change}: {error}"
        );
    }
}

#[test]
fn historical_expired_lookup_rejects_a_conflicting_original_phase_pending() {
    let (f, status, _, audit) = audit_fixture();
    let mut f = rebuild(f, status.input.merge_deadline + 4, true);
    let record = wire::decode(f.catalog.config.project_id(), &audit)
        .unwrap()
        .unwrap();
    let started = record
        .finish_expiry
        .context
        .iter()
        .find(|event| event.eid == status.started_eid)
        .unwrap();
    let mut original = recovery_outbox::original_of(started);
    original.data["unknown_pending_field"] = json!("same eid, different full original");
    f.outbox.append_pending(&original).unwrap();
    let error = ProjectSession::finish_view_shared(
        &f.session,
        &f.catalog,
        "host",
        &status.request_id,
        &mut f.outbox,
        || Ok(status.input.merge_deadline + 6),
    )
    .unwrap_err();
    assert!(error.contains("UUID/body conflict"), "{error}");
    assert!(f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .all(|event| event.eid != original.eid));
    assert!(f
        .session
        .lock()
        .unwrap()
        .renew_binding(
            &f.catalog,
            "host",
            Some(&status.input.run_id),
            status.input.merge_deadline + 6
        )
        .is_err());
}

#[test]
fn pending_only_expiry_audit_cannot_be_promoted_by_app_rebuild() {
    let (mut f, status, _, audit) = audit_fixture();
    let mut record = wire::decode(f.catalog.config.project_id(), &audit)
        .unwrap()
        .unwrap();
    record.original.data["task"]["title"] = json!("pending inner is not actual acceptance");
    let pending = wire::canonical(&record, "host").unwrap();
    assert_ne!(pending.eid, audit.eid);
    f.outbox.append_pending(&pending).unwrap();
    drop(f.session);
    fs::remove_dir_all(f.catalog.common_dir.join("towermd")).unwrap();
    let now = status.input.merge_deadline + 4;
    let inputs = f.outbox.read_inputs().unwrap();
    let request = crate::recovery::prepare(
        &f.catalog,
        &inputs,
        now,
        now + 100_000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let error = f
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
        .err()
        .expect("pending audit must refuse");
    assert!(
        error
            .to_string()
            .contains("pending expiry audit cannot establish original acceptance"),
        "{error}"
    );
    assert!(f.outbox.read_inputs().unwrap().pending().contains(&pending));
    let saved = crate::snapshot_service::read_snapshot(&Catalog::load(f.root.path()).unwrap())
        .unwrap()
        .unwrap();
    assert!(!saved
        .archives
        .values()
        .flat_map(|text| text.lines())
        .map(|line| crate::hub_legacy::decode_archive_line(line).unwrap())
        .any(|event| event.eid == pending.eid));
}
