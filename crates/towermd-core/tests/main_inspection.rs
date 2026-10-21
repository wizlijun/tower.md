use std::{collections::BTreeMap, fs, path::Path, process::Command};
use serde_json::json;
use towermd_core::{baseline_publication::Publication, config::Config, events,
    hub::Hub, hub_protocol::*, main_inspection, main_writer::{self, WriteKind, WriteOutcome, WriteRequest},
    project_runtime, dev_state, snapshot::Snapshot, snapshot_publication};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Inspection Test", "-c", "user.email=qa@localhost", "-c", "commit.gpgsign=false"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().into()
}
fn write(root: &Path, path: &str, bytes: impl AsRef<[u8]>) {
    let file = root.join(path); fs::create_dir_all(file.parent().unwrap()).unwrap(); fs::write(file, bytes).unwrap();
}
fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "."]); git(root, &["commit", "-qm", message]); git(root, &["rev-parse", "HEAD"])
}
fn fixture() -> (tempfile::TempDir, String, Hub, Fence) {
    let temp = tempfile::tempdir().unwrap(); let root = temp.path();
    git(root, &["init", "-qb", "main"]);
    write(root, "README.md", "# Imported project\n");
    write(root, ".towermd/config.yaml", Config::new("inspection-project", "docs/tower").unwrap().to_yaml());
    write(root, "docs/tower/arch/import.md", "# Imported structure\n");
    let anchor = commit(root, "whole enable commit");
    let mut hub = Hub::open(&root.join(".git/test-hub")).unwrap();
    let fence = hub.lease_acquire(AcquireLease {
        host: "test".into(), names: vec!["main".into()], holder: "daemon".into(), ttl_ms: 900_000,
    }, 1000).unwrap().leases[0].fence();
    (temp, anchor, hub, fence)
}
fn accepted(hub: &mut Hub, input: NewEvent) -> Event {
    match HubProtocol::append(hub, input, 1001).unwrap() {
        AppendReceipt::Event(event) => *event,
        other => panic!("unexpected {other:?}"),
    }
}
fn written(outcome: WriteOutcome) -> String {
    match outcome { WriteOutcome::Written { commit, .. } => commit, other => panic!("{other:?}") }
}
fn receipts(hub: &Hub) -> BTreeMap<String, Event> {
    project_runtime::canonical_events(&hub.events(0)).unwrap()
}
fn check(root: &Path, main: &str, known: &BTreeMap<String, Event>) -> main_inspection::Inspection {
    main_inspection::inspect(root, main, "inspection-project", "docs/tower", None, "test",
        |eid| known.get(eid).cloned())
}

#[test]
fn original_import_is_context_but_bare_tags_and_reverts_never_authorize_changes() {
    let (temp, anchor, _, _) = fixture(); let root = temp.path();
    assert!(check(root, &anchor, &BTreeMap::new()).passed);
    write(root, "docs/tower/arch/import.md", "# Unauthorized\n");
    let edit = commit(root, "edit protected structure\n\nTowermd-Baseline: B0");
    git(root, &["tag", "towermd/B0", &edit]);
    git(root, &["revert", "--no-edit", &edit]);
    let tip = git(root, &["rev-parse", "HEAD"]);
    let first = check(root, &tip, &BTreeMap::new());
    let second = check(root, &tip, &BTreeMap::new());
    assert!(first.complete); assert!(!first.passed);
    assert_eq!(first.import_anchor, Some(anchor));
    assert_eq!(first.blocked_commits.len(), 2);
    assert_eq!(first.alerts, second.alerts);
    assert!(first.blocked_commits.iter().any(|entry| entry.commit == edit));
}

#[test]
fn unknown_trailers_keep_raw_text_and_are_nonblocking_but_incomplete_history_blocks() {
    let (temp, anchor, _, _) = fixture(); let root = temp.path();
    write(root, "README.md", "# ordinary change\n");
    git(root, &["add", "."]);
    git(root, &["commit", "--cleanup=verbatim", "-qm", "ordinary\n\nTowermd-Future: opaque:value  with spaces  \n"]);
    let tip = git(root, &["rev-parse", "HEAD"]);
    let result = check(root, &tip, &BTreeMap::new());
    assert!(result.complete && result.passed);
    assert_eq!(result.warning_commits, vec![tip.clone()]);
    assert_eq!(result.alerts[0].data["detail"]["trailers"], json!(["Towermd-Future: opaque:value  with spaces  "]));
    let backwards = main_inspection::inspect(root, &anchor, "inspection-project", "docs/tower",
        Some(&tip), "test", |_| None);
    assert!(!backwards.complete && !backwards.passed);
    assert!(backwards.error.as_deref().unwrap().starts_with("inspection_incomplete:"));
    assert_eq!(backwards.alerts[0].eid, events::derived_eid(&anchor,"alert.raised.inspection_incomplete").unwrap());
    assert_eq!(backwards.alerts[0].data["detail"]["error"], json!(backwards.error));
    fs::write(root.join(".git/shallow"), format!("{tip}\n")).unwrap();
    let shallow=check(root, &tip, &BTreeMap::new());
    assert!(!shallow.complete);
    assert_eq!(shallow.alerts[0].data["reason"], "inspection_incomplete");
    assert_eq!(shallow.alerts[0].data["detail"]["error"], json!(shallow.error));
    assert_eq!(shallow.alerts,check(root,&tip,&BTreeMap::new()).alerts);
    fs::remove_file(root.join(".git/shallow")).unwrap();
    assert!(main_inspection::inspect(root,&tip,"inspection-project","docs/tower",
        Some(&tip),"test",|_|None).complete);
}

#[test]
fn canonical_approval_and_exact_git_publication_authorize_only_their_own_commit() {
    let (temp, anchor, mut hub, fence) = fixture(); let root = temp.path();
    let mut publication = Publication {
        removals: vec![],
        request_id: uuid::Uuid::now_v7().to_string(), draft_id: "reviewed".into(),
        project_id: "inspection-project".into(), source_revision: anchor,
        target: "B0".into(), prepared_at: "2026-10-21T00:00:00Z".into(), content_hash: String::new(),
        files: BTreeMap::from([("docs/tower/arch/A.md".into(), b"---\ntype: Module\n---\nApproved\n".to_vec())]),
    };
    publication.content_hash = publication.hash().unwrap();
    let approval = accepted(&mut hub, NewEvent {
        eid: publication.request_id.clone(), host: "test".into(), actor: "human:local".into(),
        kind: "baseline.approved".into(), layer: "L1".into(), obj: "B0".into(),
        data: serde_json::to_value(&publication).unwrap(), guards: vec![],
    });
    git(root, &["checkout", "--detach", "-q"]);
    let commit = written(main_writer::write(root, &WriteRequest {
        docs_dir: "docs/tower".into(), epoch: fence.epoch,
        kind: WriteKind::Slow { expected_base: publication.source_revision.clone(), tag: Some("refs/tags/towermd/B0".into()) },
        changes: publication.files.iter().map(|(key, value)| (key.clone(), Some(value.clone()))).collect(),
        message: format!("approve B0\n\nTowermd-Baseline: B0\nTowermd-Approval: {}\nTowermd-Content: {}",
            approval.eid, publication.content_hash),
    }, || Ok(())).unwrap());
    let created = accepted(&mut hub, NewEvent {
        eid: events::derived_eid(&commit, "baseline.created").unwrap(), host: "test".into(),
        actor: "process:towermd".into(), kind: "baseline.created".into(), layer: "L1".into(), obj: "B0".into(),
        data: json!({"commit":commit,"approval_eid":approval.eid,"content_hash":publication.content_hash}), guards: vec![fence],
    });
    let mut known = receipts(&hub);
    assert!(check(root, &commit, &known).passed);
    known.remove(&approval.eid);
    assert!(!check(root, &commit, &known).passed);
    known.insert(approval.eid.clone(), approval);
    known.get_mut(&created.eid).unwrap().data["content_hash"] = json!("forged");
    assert!(!check(root, &commit, &known).passed);
}

#[test]
fn actual_snapshot_archive_and_main_grant_are_required_even_with_valid_trailers() {
    let (temp, _, mut hub, fence) = fixture(); let root = temp.path();
    let snapshot = Snapshot::build("inspection-project", None, &hub.events(0), hub.contiguous_seq(),
        fence.epoch, hub.epoch_high_water(), |events| project_runtime::replay(
            &dev_state::initial(), events, dev_state::apply)).unwrap();
    git(root, &["checkout", "--detach", "-q"]);
    let commit = written(main_writer::write(root, &WriteRequest {
        docs_dir: "docs/tower".into(), epoch: fence.epoch,
        kind: WriteKind::Snapshot { seq: snapshot.state.seq, promote_from: None },
        changes: snapshot.files("docs/tower").unwrap(), message: "durable snapshot".into(),
    }, || Ok(())).unwrap());
    let proof = snapshot_publication::SnapshotProof {
        commit: commit.clone(), seq: snapshot.state.seq, epoch: fence.epoch,
    };
    let receipt = accepted(&mut hub, NewEvent {
        eid: events::derived_eid(&commit, "snapshot.written").unwrap(), host: "test".into(),
        actor: "process:towermd-snapshot".into(), kind: "snapshot.written".into(),
        layer: "L5".into(), obj: "inspection-project".into(),
        data: serde_json::to_value(&proof).unwrap(), guards: vec![fence],
    });
    let mut known = receipts(&hub);
    assert!(check(root, &commit, &known).passed);
    assert_eq!(snapshot_publication::prove(root, &commit, "docs/tower", "inspection-project",
        &receipt, &|eid| known.get(eid).cloned()).unwrap(), proof);
    known.remove(&snapshot.events[0].eid);
    assert!(!check(root, &commit, &known).passed);
    known = receipts(&hub);
    let mut higher_epoch = receipt.clone();
    higher_epoch.guards[0].epoch += 2; higher_epoch.epoch = Some(higher_epoch.guards[0].epoch);
    higher_epoch.actor = "process:towermd-recovery".into();
    known.insert(higher_epoch.eid.clone(), higher_epoch.clone());
    assert!(snapshot_publication::prove(root, &commit, "docs/tower", "inspection-project",
        &higher_epoch, &|eid| known.get(eid).cloned()).is_ok());
    higher_epoch.data["epoch"] = json!(higher_epoch.guards[0].epoch);
    known.insert(higher_epoch.eid.clone(), higher_epoch.clone());
    assert!(snapshot_publication::prove(root, &commit, "docs/tower", "inspection-project",
        &higher_epoch, &|eid| known.get(eid).cloned()).is_err());

    // A real accepted receipt still cannot bless unrelated changes hidden in
    // a snapshot-shaped commit with matching trailers and author.
    git(root, &["checkout", "--detach", "-q", &commit]);
    write(root, "README.md", "# Smuggled unrelated change\n");
    git(root, &["add", "README.md"]);
    let message = format!("snapshot lookalike\n\nTowermd-Epoch: {}\nTowermd-Snapshot: {}",
        proof.epoch, proof.seq);
    git(root, &["commit", "-qm", &message, "--author=process:towermd-snapshot <towermd@localhost>"]);
    let mixed = git(root, &["rev-parse", "HEAD"]);
    let mut input = NewEvent {
        eid: events::derived_eid(&mixed, "snapshot.written").unwrap(), host: "test".into(),
        actor: receipt.actor.clone(), kind: receipt.kind.clone(), layer: receipt.layer.clone(),
        obj: receipt.obj.clone(), data: receipt.data.clone(), guards: receipt.guards.clone(),
    };
    input.data["commit"] = json!(mixed);
    let mixed_receipt = accepted(&mut hub, input);
    known = receipts(&hub);
    assert!(snapshot_publication::prove(root, &mixed, "docs/tower", "inspection-project",
        &mixed_receipt, &|eid| known.get(eid).cloned()).unwrap_err().contains("非归档改动"));

    // Rewriting the stored projection cannot pass shared-reader replay.
    let mut forged_state = snapshot.state.clone();
    forged_state.projection["Alert"]["fake"] = json!({"resolved":false});
    write(root, "docs/tower/state/state.json", serde_json::to_vec_pretty(&forged_state).unwrap());
    git(root, &["add", "docs/tower/state/state.json"]);
    git(root, &["commit", "-qm", &message, "--author=process:towermd-snapshot <towermd@localhost>"]);
    let forged = git(root, &["rev-parse", "HEAD"]);
    assert!(Snapshot::read(root, &forged, "docs/tower", "inspection-project",
        |events| project_runtime::replay(&dev_state::initial(), events, dev_state::apply)).is_err());
}
