// Included as core unit tests to exercise private producer/recovery boundaries.
use super::*;
use crate::{
    project_session::ProjectSession,
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
    snapshot_service::SnapshotWriter,
    test_support,
    verifier::{verify_sealed, VerificationRequest},
};
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};
const NOW: u64 = 1_800_000_000_000;
struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
    catalog: Catalog,
    frozen: FrozenColdstart,
    session: Mutex<ProjectSession>,
    outbox: RecoveryOutbox,
}
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    test_support::catalog(root.path());
    test_support::git(root.path(), &["switch", "main"]);
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.py"), "# actual source\n").unwrap();
    test_support::git(root.path(), &["add", "src"]);
    test_support::git(root.path(), &["commit", "-qm", "coldstart source"]);
    test_support::git(root.path(), &["switch", "-qc", "coldstart-case"]);
    let catalog = Catalog::load(root.path()).unwrap();
    let input =
        crate::coldstart::capture_scan_input(&catalog, &crate::coldstart::ScanOptions::default())
            .unwrap();
    let prepared = PreparedColdstart::prepare(input, "scan-real").unwrap();
    let sealed = verify_sealed(&VerificationRequest {
        root: prepared.input().root.clone(),
        common_dir: prepared.input().common_dir.clone(),
        revision: prepared.input().revision.clone(),
        commands: prepared.commands().unwrap(),
        timeout: Duration::from_secs(5),
        log_root: prepared
            .input()
            .common_dir
            .join("towermd/cache/verification"),
        cancelled: Arc::new(AtomicBool::new(false)),
    })
    .unwrap();
    let frozen = prepared.freeze(&sealed, "actual-host", NOW).unwrap();
    assert_eq!(frozen.evidence_events.len(), 1);
    let runtime = tempfile::tempdir().unwrap();
    let outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &runtime.path().join("hub"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            NOW,
        )
        .unwrap(),
    );
    Fixture {
        root,
        runtime,
        catalog,
        frozen,
        session,
        outbox,
    }
}
fn strip_unpersisted_confirmed(runtime: &std::path::Path) {
    for file in fs::read_dir(runtime.join("outbox")).unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let lines = text
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).unwrap()["state"] != "confirmed")
            .collect::<Vec<_>>();
        fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
    }
}

#[test]
fn incomplete_report_and_pending_evidence_cannot_mint_completed_and_resume_keeps_extensions() {
    let mut f = fixture();
    let report = f.frozen.report_event.clone();
    crate::recovery_outbox::append_durable(
        &mut *f.session.lock().unwrap().hub,
        &mut f.outbox,
        report,
        NOW,
    )
    .unwrap();
    ProjectSession::sync_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        NOW,
    )
    .unwrap();
    assert!(
        read_completed(&f.catalog, &f.outbox.read_inputs().unwrap(), "scan-real")
            .unwrap()
            .is_none()
    );
    let mut pending = f.frozen.evidence_events[0].clone();
    pending.data["evidence"]["unknown_extension"] = json!({"future":[1,null,{"nested":true}]});
    f.outbox.append_pending(&pending).unwrap();
    assert!(
        read_completed(&f.catalog, &f.outbox.read_inputs().unwrap(), "scan-real")
            .unwrap()
            .is_none()
    );
    let restored = ProjectSession::resume_coldstart_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        "scan-real",
        NOW + 1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(restored.evidence[0], pending.data["evidence"]);
    assert!(
        read_completed(&f.catalog, &f.outbox.read_inputs().unwrap(), "scan-real")
            .unwrap()
            .is_some()
    );
    let confirmed = f.outbox.read_inputs().unwrap();
    assert_eq!(confirmed.confirmed().len(), 2);
    assert_eq!(
        crate::recovery_outbox::original_of(&confirmed.confirmed()[1]),
        pending
    );
}

#[test]
fn history_rejects_bad_hash_mapping_indices_identity_and_authorization_fields() {
    let mut f = fixture();
    ProjectSession::record_coldstart_shared(&f.session, &f.catalog, &mut f.outbox, &f.frozen, NOW)
        .unwrap();
    let original: BTreeMap<_, _> = f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .map(|e| (e.eid.clone(), e.clone()))
        .collect();
    let report = f.frozen.report_eid().to_owned();
    let evidence = f.frozen.evidence_events[0].eid.clone();
    let module = f.frozen.evidence_events[0].data["evidence"]["module"]
        .as_str()
        .unwrap()
        .to_owned();
    for (role, pointer, value) in [
        (
            &report,
            "/coldstart_report/input/project_id",
            json!("another-project"),
        ),
        (
            &report,
            &format!("/coldstart_report/evidence_ids/{module}/event_eid"),
            json!("unmapped-event"),
        ),
        (
            &report,
            "/coldstart_report/report/tree",
            json!("f".repeat(40)),
        ),
        (
            &evidence,
            "/evidence/log/report_sha256",
            json!("f".repeat(64)),
        ),
        (
            &evidence,
            "/evidence/log/report_eid",
            json!("missing-report"),
        ),
        (
            &evidence,
            "/evidence/coldstart/execution_indices",
            json!([0]),
        ),
        (&evidence, "/evidence/result", json!("passed")),
        (
            &evidence,
            "/evidence/generated/at",
            json!("2001-01-01T00:00:00Z"),
        ),
    ] {
        let mut bad = original.clone();
        *bad.get_mut(role)
            .unwrap()
            .data
            .pointer_mut(pointer)
            .unwrap() = value;
        assert!(
            validate_history(f.catalog.config.project_id(), &bad).is_err(),
            "{pointer}"
        );
    }
    for key in ["task", "run", "gate", "fences", "authority", "baseline"] {
        let mut bad = original.clone();
        bad.get_mut(&evidence).unwrap().data["evidence"][key] = json!("claimed-authority");
        assert!(
            validate_history(f.catalog.config.project_id(), &bad).is_err(),
            "{key}"
        );
    }
    let mut unknown = original.clone();
    unknown.get_mut(&evidence).unwrap().data["evidence"]["future"] = json!({"opaque":true});
    validate_history(f.catalog.config.project_id(), &unknown).unwrap();
    let mut wrong_order = original;
    wrong_order.get_mut(&evidence).unwrap().seq = wrong_order[&report].seq;
    assert!(validate_history(f.catalog.config.project_id(), &wrong_order).is_err());
}

#[test]
fn failed_confirmation_blocks_followers_then_compacted_actual_archive_recovers_original_receipt() {
    let mut f = fixture();
    f.outbox.fail_next_confirmation();
    let error = ProjectSession::record_coldstart_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.frozen,
        NOW,
    )
    .unwrap_err();
    assert!(error.contains("fsync"), "{error}");
    assert!(!f
        .session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .contains_key("Evidence"));
    let actual = f
        .session
        .lock()
        .unwrap()
        .events(0)
        .into_iter()
        .find(|e| e.kind == "coldstart.reported")
        .unwrap();
    // A crash can lose the unconfirmed fsync tail. Keep only the durable pending.
    strip_unpersisted_confirmed(f.runtime.path());
    assert!(f.outbox.read_inputs().unwrap().confirmed().is_empty());
    SnapshotWriter::default()
        .write(&f.session, &f.catalog, "actual-host", true, || Ok(NOW + 1))
        .unwrap();
    let current = Catalog::load(f.root.path()).unwrap();
    assert!(f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(0, NOW + 2)
        .is_err());
    ProjectSession::record_coldstart_shared(
        &f.session,
        &current,
        &mut f.outbox,
        &f.frozen,
        NOW + 2,
    )
    .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs.confirmed().iter().find(|e| e.eid == actual.eid),
        Some(&actual)
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.kind == "coldstart.reported")
            .count(),
        1
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.kind == "evidence.recorded")
            .count(),
        1
    );
}

#[test]
fn duplicate_without_confirmed_or_actual_archive_stays_uncertain_and_no_follower_is_written() {
    let mut f = fixture();
    f.outbox.fail_next_confirmation();
    assert!(ProjectSession::record_coldstart_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.frozen,
        NOW
    )
    .is_err());
    strip_unpersisted_confirmed(f.runtime.path());
    let history =
        crate::recovery_git::read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap())
            .unwrap();
    let mut live = f.session.lock().unwrap();
    let lease = live
        .hub
        .lease_acquire(
            crate::hub_protocol::AcquireLease {
                host: "actual-host".into(),
                names: vec!["main".into()],
                holder: "snapshot:test".into(),
                ttl_ms: 10000,
            },
            NOW + 1,
        )
        .unwrap();
    let seq = live.hub.contiguous_seq();
    live.hub
        .snapshot_ack(
            crate::hub_protocol::SnapshotAck {
                commit: "a".repeat(40),
                seq,
                epoch: lease.leases[0].epoch,
            },
            json!({}),
        )
        .unwrap();
    let error = crate::recovery_outbox::append_coldstart_durable(
        &mut *live.hub,
        &mut f.outbox,
        f.frozen.report_event.clone(),
        NOW + 1,
        &history,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("duplicate has no original receipt"),
        "{error}"
    );
    assert!(f.outbox.read_inputs().unwrap().confirmed().is_empty());
    assert!(!live.projection().unwrap().objects.contains_key("Evidence"));
}

#[test]
fn app_recovery_preserves_original_receipts_after_second_snapshot_and_outbox_deletion() {
    let mut f = fixture();
    f.frozen.evidence_events[0].data["evidence"]["future_extension"] =
        json!({"opaque":[null,{"retained":true}]});
    SnapshotWriter::default()
        .write(&f.session, &f.catalog, "actual-host", true, || Ok(NOW))
        .unwrap();
    let current = Catalog::load(f.root.path()).unwrap();
    ProjectSession::record_coldstart_shared(
        &f.session,
        &current,
        &mut f.outbox,
        &f.frozen,
        NOW + 1,
    )
    .unwrap();
    let originals = f.outbox.read_inputs().unwrap().confirmed().to_vec();
    let inputs = f.outbox.read_inputs().unwrap();
    let request = crate::recovery::prepare(
        &current,
        &inputs,
        NOW + 2,
        NOW + 10000,
        "0192f0aa-0000-7000-8000-000000000001",
    )
    .unwrap();
    let (_, restored) = crate::recovery::confirm(
        None,
        &current,
        &request,
        "human:owner",
        NOW + 10000,
        &inputs,
        "recovered-host",
        || Ok(NOW + 3),
    )
    .unwrap();
    let restored_catalog = Catalog::load(f.root.path()).unwrap();
    SnapshotWriter::default()
        .write(&restored, &restored_catalog, "recovered-host", true, || {
            Ok(NOW + 4)
        })
        .unwrap();
    drop(f.outbox);
    fs::remove_dir_all(f.runtime.path().join("outbox")).unwrap();
    fs::remove_dir_all(f.catalog.common_dir.join("towermd/cache")).unwrap();
    let current = Catalog::load(f.root.path()).unwrap();
    let inputs =
        crate::recovery_outbox::RecoveryInputs::read(&f.runtime.path().join("outbox"), &current)
            .unwrap();
    let history = crate::recovery_git::read_recovery_history(&current, &inputs).unwrap();
    for original in &originals {
        assert_eq!(history.coldstart_original(&original.eid), Some(original));
    }
    let completed = read_completed(&current, &inputs, "scan-real")
        .unwrap()
        .unwrap();
    assert_eq!(completed.evidence(), f.frozen.evidence());
    assert_eq!(completed.report_eid(), f.frozen.report_eid());
}

#[test]
fn recovery_wrapper_preserves_exact_original_and_rejects_forgery_nesting_and_stale_original() {
    let mut f = fixture();
    ProjectSession::record_coldstart_shared(&f.session, &f.catalog, &mut f.outbox, &f.frozen, NOW)
        .unwrap();
    let originals = f.outbox.read_inputs().unwrap().confirmed().to_vec();
    for original in originals {
        let wrapper = recovery_receipt(&original, "recovered-host").unwrap();
        let actual = match f
            .session
            .lock()
            .unwrap()
            .hub
            .append(wrapper, NOW + 1)
            .unwrap()
        {
            crate::hub_protocol::AppendReceipt::Event(e) => *e,
            _ => panic!(),
        };
        assert_eq!(restored_original(&actual).unwrap(), Some(original.clone()));
        for (field, value) in [
            ("actor", json!("process:forged")),
            ("obj", json!("wrong")),
            ("host", json!("wrong-host")),
            ("seq", json!(9000)),
        ] {
            let mut bad = actual.clone();
            bad.data["original"][field] = value;
            assert!(restored_original(&bad).is_err(), "{field}");
        }
        let mut stale = original.clone();
        stale.stale = true;
        assert!(recovery_receipt(&stale, "recovered-host").is_err());
        let mut nested = actual.clone();
        nested.data["original"] = serde_json::to_value(&actual).unwrap();
        assert!(restored_original(&nested).is_err());
        let mut stale_outer = actual;
        stale_outer.stale = true;
        assert!(restored_original(&stale_outer).unwrap().is_none());
    }
}

#[test]
fn recording_probe_is_readonly_and_bad_pending_package_is_not_appended() {
    let mut f = fixture();
    let absent = f.runtime.path().join("not-created");
    let inputs = crate::recovery_outbox::RecoveryInputs::read(&absent, &f.catalog).unwrap();
    assert!(!recording_exists(&f.catalog, &inputs, "missing-scan").unwrap());
    assert!(!absent.exists());
    let mut report = f.frozen.report_event.clone();
    report.data["coldstart_report"]["generated_at"] = json!("invalid-timestamp");
    f.outbox.append_pending(&report).unwrap();
    let before = f.session.lock().unwrap().last_seq();
    assert!(ProjectSession::resume_coldstart_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        "scan-real",
        NOW + 1
    )
    .is_err());
    assert_eq!(f.session.lock().unwrap().last_seq(), before);
    assert!(f.outbox.read_inputs().unwrap().confirmed().is_empty());
}

#[test]
fn authority_change_before_first_append_retains_frozen_report_intent_for_fresh_resume() {
    let mut f = fixture();
    let old_source = f.frozen.input().revision.clone();
    let expected = f.frozen.report_event.clone();
    let parent = f.catalog.revision.clone();
    let tree = test_support::git(f.root.path(), &["rev-parse", "main^{tree}"]);
    let advanced = test_support::git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &parent,
            "-m",
            "concurrent main advance",
        ],
    );
    test_support::git(
        f.root.path(),
        &["update-ref", "refs/heads/main", &advanced, &parent],
    );
    let error = ProjectSession::record_coldstart_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.frozen,
        NOW,
    )
    .unwrap_err();
    assert!(error.contains("authority changed"), "{error}");
    let pending = f.outbox.read_inputs().unwrap();
    assert_eq!(pending.pending(), std::slice::from_ref(&expected));
    assert!(pending.confirmed().is_empty());
    let current = Catalog::load(f.root.path()).unwrap();
    let restored = ProjectSession::resume_coldstart_shared(
        &f.session,
        &current,
        &mut f.outbox,
        "scan-real",
        NOW + 1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(restored.report_eid, expected.eid);
    assert_eq!(restored.input.revision, old_source);
    let accepted = f.outbox.read_inputs().unwrap();
    assert_eq!(
        crate::recovery_outbox::original_of(&accepted.confirmed()[0]),
        expected
    );
    assert_eq!(
        accepted
            .confirmed()
            .iter()
            .filter(|e| e.kind == "coldstart.reported")
            .count(),
        1
    );
    assert_eq!(restored.report, f.frozen.report().clone());
}
