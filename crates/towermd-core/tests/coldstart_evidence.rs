mod support;
use serde_json::json;
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};
use tempfile::TempDir;
use towermd_core::{
    catalog::Catalog,
    coldstart::{capture_scan_input, ScanOptions},
    coldstart_evidence::{FrozenColdstart, PreparedColdstart},
    hub_protocol::{AppendReceipt, Event, NewEvent},
    project_session::ProjectSession,
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
    snapshot::{Snapshot, StateProjection},
    snapshot_service::SnapshotWriter,
    verifier::{verify_sealed, VerificationRequest},
};
const NOW: u64 = 1_800_000_000_000;

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, content).unwrap();
}
fn fixture(sources: bool) -> (TempDir, TempDir, Catalog, FrozenColdstart) {
    let root = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    support::git(root.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(
        root.path(),
        &towermd_core::init::InitOptions { docs_dir: None },
    )
    .unwrap();
    if sources {
        for path in ["alpha/a.py", "beta/b.py", "good/g.py", "worker/w.py"] {
            write(root.path(), path, "# source\n");
        }
        write(
            root.path(),
            "Makefile",
            "test:\n\t@printf 'root-real-execution\\n'\n\t@false\n",
        );
        write(
            root.path(),
            "good/Makefile",
            "test:\n\t@printf 'independent-real-execution\\n'\n\t@test -f g.py\n",
        );
        write(
            root.path(),
            "worker/pyproject.toml",
            "[project]\nname='worker'\n",
        );
    }
    support::git(root.path(), &["add", "."]);
    support::git(root.path(), &["commit", "-qm", "coldstart fixture"]);
    support::git(root.path(), &["switch", "-qc", "scan-fixture"]);
    let catalog = Catalog::load(root.path()).unwrap();
    let input = capture_scan_input(&catalog, &ScanOptions::default()).unwrap();
    let prepared = PreparedColdstart::prepare(input, "scan-fixture").unwrap();
    let input = prepared.input();
    let sealed = verify_sealed(&VerificationRequest {
        root: input.root.clone(),
        common_dir: input.common_dir.clone(),
        revision: input.revision.clone(),
        commands: prepared.commands().unwrap(),
        timeout: Duration::from_secs(5),
        log_root: input.common_dir.join("towermd/cache/verification"),
        cancelled: Arc::new(AtomicBool::new(false)),
    })
    .unwrap();
    let frozen = prepared.freeze(&sealed, "actual-host", NOW).unwrap();
    (root, runtime, catalog, frozen)
}
fn open(catalog: &Catalog, runtime: &Path) -> (Mutex<ProjectSession>, RecoveryOutbox) {
    let outbox = RecoveryOutbox::open(
        &runtime.join("outbox"),
        RecoveryContext::from_catalog(catalog).unwrap(),
    )
    .unwrap();
    let session = ProjectSession::open(
        &runtime.join("hub"),
        catalog,
        &outbox.read_inputs().unwrap(),
        NOW,
    )
    .unwrap();
    (Mutex::new(session), outbox)
}
fn append(session: &Mutex<ProjectSession>, event: NewEvent) -> Event {
    match session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(event, NOW)
        .unwrap()
    {
        AppendReceipt::Event(e) => *e,
        _ => panic!("new fixture event must have original receipt"),
    }
}

#[test]
fn real_report_is_once_durable_for_shared_failed_independent_passed_and_no_tests() {
    let (_root, runtime, catalog, frozen) = fixture(true);
    assert_eq!(frozen.report().executions.len(), 2);
    let evidence = frozen.evidence();
    assert_eq!(evidence.len(), 4);
    assert_eq!(
        evidence.iter().filter(|e| e["result"] == "failed").count(),
        2
    );
    assert_eq!(
        evidence.iter().filter(|e| e["result"] == "passed").count(),
        1
    );
    assert_eq!(
        evidence
            .iter()
            .filter(|e| e["result"] == "no_tests")
            .count(),
        1
    );
    let (session, mut outbox) = open(&catalog, runtime.path());
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW).unwrap();
    let originals = outbox.read_inputs().unwrap();
    assert_eq!(originals.confirmed().len(), 5);
    assert_eq!(
        originals
            .confirmed()
            .iter()
            .filter(|e| e.kind == "coldstart.reported")
            .count(),
        1
    );
    let report = originals
        .confirmed()
        .iter()
        .find(|e| e.kind == "coldstart.reported")
        .unwrap();
    for event in originals
        .confirmed()
        .iter()
        .filter(|e| e.kind == "evidence.recorded")
    {
        assert!(event.seq > report.seq);
        assert_eq!(event.data["evidence"]["log"]["report_eid"], report.eid);
        assert!(event.data.get("coldstart_report").is_none());
        assert!(event.data["evidence"].get("validation_plan").is_none());
    }
    let before = session.lock().unwrap().applied_count();
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW + 1)
        .unwrap();
    assert_eq!(session.lock().unwrap().applied_count(), before);
    assert_eq!(
        outbox.read_inputs().unwrap().confirmed(),
        originals.confirmed()
    );
    let state = session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(state.objects["Evidence"].len(), 4);
    assert!(!state.objects.contains_key("Task"));
    assert!(!state.objects.contains_key("Run"));
    for evidence in frozen.evidence() {
        assert_eq!(
            state.objects["Evidence"][evidence["id"].as_str().unwrap()],
            evidence
        );
    }
}

#[test]
fn zero_module_scan_still_durably_records_one_empty_report() {
    let (_root, runtime, catalog, frozen) = fixture(false);
    assert!(frozen.evidence().is_empty());
    assert!(frozen.report().executions.is_empty());
    let (session, mut outbox) = open(&catalog, runtime.path());
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW).unwrap();
    assert_eq!(outbox.read_inputs().unwrap().confirmed().len(), 1);
    assert!(session
        .lock()
        .unwrap()
        .projection()
        .unwrap()
        .objects
        .is_empty());
}

#[test]
fn projection_preserves_extensions_rejects_conflicts_atomically_and_ignores_stale() {
    let (_root, runtime, catalog, frozen) = fixture(true);
    let (session, mut outbox) = open(&catalog, runtime.path());
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW).unwrap();
    let inputs = outbox.read_inputs().unwrap();
    let mut event = inputs
        .confirmed()
        .iter()
        .find(|e| e.kind == "evidence.recorded")
        .unwrap()
        .clone();
    event.data["evidence"]["extension"] = json!({"unrecognized":[1,2,3]});
    let mut state = StateProjection::default();
    state.apply_recorded(&event).unwrap();
    assert_eq!(
        state.objects["Evidence"][&event.obj],
        event.data["evidence"]
    );
    let before = state.clone();
    for field in ["type", "id"] {
        let mut invalid = event.clone();
        invalid.data["evidence"][field] = json!("wrong");
        assert!(state.apply_recorded(&invalid).is_err());
        assert_eq!(state, before);
    }
    let mut conflict = event.clone();
    conflict.data["evidence"]["extension"] = json!(false);
    assert!(state.apply_recorded(&conflict).is_err());
    assert_eq!(state, before);
    event.stale = true;
    let mut empty = StateProjection::default();
    empty.apply_recorded(&event).unwrap();
    assert!(empty.objects.is_empty());
    event.stale = false;
    event.data["evidence"]["coldstart"]["schema"] = json!(99);
    empty.apply_recorded(&event).unwrap();
    assert!(empty.objects.is_empty());
}

#[test]
fn snapshot_archive_and_reopen_keep_evidence_without_cache_job_draft_or_outbox() {
    let (root, runtime, catalog, frozen) = fixture(true);
    let (session, mut outbox) = open(&catalog, runtime.path());
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW).unwrap();
    // Ordinary Task and Run declarations coexist without gaining a completed state.
    append(
        &session,
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "actual-host".into(),
            actor: "process:test".into(),
            kind: "task.state".into(),
            layer: "L4".into(),
            obj: "T".into(),
            guards: vec![],
            data: json!({
                "task":{"type":"Task","id":"T","title":"Task","module":"A","state":"ready"},
                "run":{"type":"Run","id":"R","task":"T","module":"A","state":"released", "host":"actual-host", "worktree":"/fixture", "branch":"fixture", "leases":[], "cost_usd":null, "task_card":{}}
            }),
        },
    );
    ProjectSession::sync_shared(&session, &catalog, &outbox.read_inputs().unwrap(), NOW).unwrap();
    let before = session.lock().unwrap().projection().unwrap().clone();
    let report = SnapshotWriter::default()
        .write(
            &session,
            &catalog,
            "actual-host",
            true,
            || Ok(NOW + 2),
            &mut support::outbox(&catalog),
        )
        .unwrap();
    assert_eq!(report.status, "written");
    let snapshot: Snapshot = serde_json::from_str(&support::git(
        root.path(),
        &["show", "main:docs/tower/state/state.json"],
    ))
    .unwrap();
    assert_eq!(snapshot.projection.objects, before.objects);
    drop(outbox);
    drop(session);
    fs::remove_dir_all(runtime.path().join("outbox")).unwrap();
    fs::remove_dir_all(catalog.common_dir.join("towermd/cache")).unwrap();
    let current = Catalog::load(root.path()).unwrap();
    let (session, mut outbox) = open(&current, runtime.path());
    assert_eq!(
        session.lock().unwrap().projection().unwrap().objects,
        before.objects
    );
    let restored = ProjectSession::resume_coldstart_shared(
        &session,
        &current,
        &mut outbox,
        "scan-fixture",
        NOW + 3,
    )
    .unwrap()
    .unwrap();
    assert_eq!(restored.report_eid, frozen.report_eid());
    assert_eq!(restored.report, frozen.report().clone());
    assert_eq!(restored.evidence, frozen.evidence());
}

#[test]
fn source_missing_after_real_git_gc_is_only_a_local_trace_diagnostic() {
    let (root, runtime, catalog, frozen) = fixture(true);
    let old_source = frozen.input().revision.clone();
    let (session, mut outbox) = open(&catalog, runtime.path());
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW).unwrap();
    SnapshotWriter::default()
        .write(
            &session,
            &catalog,
            "actual-host",
            true,
            || Ok(NOW + 2),
            &mut support::outbox(&catalog),
        )
        .unwrap();
    drop(session);
    drop(outbox);
    // Retain the exact snapshot tree on a new root, like a shallow migration.
    let tree = support::git(root.path(), &["rev-parse", "main^{tree}"]);
    let message = support::git(root.path(), &["show", "--no-patch", "--format=%B", "main"]);
    let shallow_root = support::git(root.path(), &["commit-tree", &tree, "-m", &message]);
    support::git(
        root.path(),
        &["update-ref", "refs/heads/main", &shallow_root],
    );
    support::git(root.path(), &["switch", "--detach", "main"]);
    support::git(root.path(), &["branch", "-D", "scan-fixture"]);
    support::git(root.path(), &["reflog", "expire", "--expire=now", "--all"]);
    support::git(root.path(), &["gc", "--prune=now"]);
    assert!(towermd_core::repo::git(root.path(), &["cat-file", "-e", &old_source]).is_err());
    fs::remove_dir_all(runtime.path().join("outbox")).unwrap();
    let current = Catalog::load(root.path()).unwrap();
    let inputs = towermd_core::recovery_outbox::RecoveryInputs::read(
        &runtime.path().join("outbox"),
        &current,
    )
    .unwrap();
    let diagnostics =
        towermd_core::coldstart_evidence::trace_diagnostics(&current, &inputs).unwrap();
    assert_eq!(diagnostics.len(), 4);
    let completed =
        towermd_core::coldstart_evidence::read_completed(&current, &inputs, "scan-fixture")
            .unwrap()
            .unwrap();
    assert_eq!(completed.evidence(), frozen.evidence());
    assert_eq!(completed.evidence().len(), 4);
}

#[test]
fn orphan_or_tampered_module_declarations_are_rejected_by_the_actual_reader() {
    let (_root, runtime, catalog, frozen) = fixture(true);
    let (session, mut outbox) = open(&catalog, runtime.path());
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW).unwrap();
    let mut evidence = frozen.evidence()[0].clone();
    evidence["id"] = json!("orphan");
    append(
        &session,
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "actual-host".into(),
            actor: "process:towermd-verifier".into(),
            kind: "evidence.recorded".into(),
            layer: "L5".into(),
            obj: "orphan".into(),
            guards: vec![],
            data: json!({"evidence":evidence}),
        },
    );
    assert!(ProjectSession::sync_shared(
        &session,
        &catalog,
        &outbox.read_inputs().unwrap(),
        NOW + 1
    )
    .is_err());
    assert_eq!(
        session.lock().unwrap().projection().unwrap().objects["Evidence"].len(),
        4
    );
}

#[test]
fn linked_worktree_can_resume_same_clone_but_another_clone_cannot_produce_old_package() {
    let (root, runtime, catalog, frozen) = fixture(true);
    let (session, mut outbox) = open(&catalog, runtime.path());
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW).unwrap();
    let linked = tempfile::tempdir().unwrap();
    support::git(
        root.path(),
        &[
            "worktree",
            "add",
            "-b",
            "linked-scan",
            linked.path().to_str().unwrap(),
            "main",
        ],
    );
    let linked_catalog = Catalog::load(linked.path()).unwrap();
    assert_ne!(linked_catalog.root, catalog.root);
    assert_eq!(linked_catalog.common_dir, catalog.common_dir);
    let resumed = ProjectSession::resume_coldstart_shared(
        &session,
        &linked_catalog,
        &mut outbox,
        "scan-fixture",
        NOW + 1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(resumed.report_eid, frozen.report_eid());
    let completed = towermd_core::coldstart_evidence::read_completed(
        &linked_catalog,
        &outbox.read_inputs().unwrap(),
        "scan-fixture",
    )
    .unwrap()
    .unwrap();
    assert_eq!(completed.evidence(), frozen.evidence());
    let clone = tempfile::tempdir().unwrap();
    support::git(
        root.path(),
        &[
            "clone",
            "--no-local",
            "-b",
            "main",
            root.path().to_str().unwrap(),
            clone.path().to_str().unwrap(),
        ],
    );
    let clone_catalog = Catalog::load(clone.path()).unwrap();
    assert_eq!(
        clone_catalog.config.project_id(),
        catalog.config.project_id()
    );
    let clone_runtime = tempfile::tempdir().unwrap();
    let (clone_session, mut clone_outbox) = open(&clone_catalog, clone_runtime.path());
    assert!(ProjectSession::record_coldstart_shared(
        &clone_session,
        &clone_catalog,
        &mut clone_outbox,
        &frozen,
        NOW + 2
    )
    .is_err());
    assert!(clone_outbox.read_inputs().unwrap().confirmed().is_empty());
}
