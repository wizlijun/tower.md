use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use towermd_core::main_writer::{
    self, Boundary, WriteKind, WriteOutcome, WriteRequest, SNAPSHOT_REF,
};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Writer Test",
            "-c",
            "user.email=qa@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim_end().into()
}

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-qb", "main"]);
    fs::write(temp.path().join("source.txt"), "original\n").unwrap();
    fs::create_dir(temp.path().join(".towermd")).unwrap();
    fs::write(temp.path().join(".towermd/config.yaml"),
        "project_id: writer-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "initial"]);
    temp
}

fn snapshot(root: &Path, epoch: u64, seq: u64) -> WriteRequest {
    use towermd_core::{hub::Hub, hub_protocol::{AcquireLease, HubProtocol, NewEvent}};
    let mut hub=Hub::open(&root.join(".git/writer-fixture-hub")).unwrap();
    let now=chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").unwrap().timestamp_millis() as u64;
    if hub.contiguous_seq()==0 {
        let mut names=(1..epoch).map(|id|format!("module:fixture-{id}")).collect::<Vec<_>>();
        names.push("main".into());
        hub.lease_acquire(AcquireLease {host:"writer-test".into(), names,
            holder:"writer".into(),ttl_ms:900_000},now).unwrap();
    }
    while hub.contiguous_seq()<seq {
        let next=hub.contiguous_seq()+1;
        hub.append(NewEvent {eid:uuid::Uuid::now_v7().to_string(),host:"writer-test".into(),
            actor:"process:writer-test".into(),kind:"fixture.observed".into(),layer:"L5".into(),
            obj:format!("event-{next}"),data:serde_json::json!({}),guards:vec![]},now).unwrap();
    }
    let snapshot=towermd_core::snapshot::Snapshot::build("writer-project",None,&hub.events(0),seq,
        epoch,hub.epoch_high_water().max(epoch),|_|Ok(serde_json::Value::Null)).unwrap();
    WriteRequest {
        docs_dir: "docs/tower".into(),
        epoch,
        kind: WriteKind::Snapshot { seq, promote_from: None },
        changes: snapshot.files("docs/tower").unwrap(),
        message: "Tower.md snapshot".into(),
    }
}

fn written(outcome: WriteOutcome) -> (String, String) {
    match outcome {
        WriteOutcome::Written {
            commit, reference, ..
        } => (commit, reference),
        other => panic!("expected write: {other:?}"),
    }
}

fn slow(base: String) -> WriteRequest {
    WriteRequest {
        docs_dir: "docs/tower".into(),
        epoch: 3,
        kind: WriteKind::Slow {
            expected_base: base,
            tag: Some("refs/tags/towermd/B0".into()),
        },
        changes: BTreeMap::from([(
            "docs/tower/arch/A.md".into(),
            Some(b"---\ntype: Module\n---\n".to_vec()),
        )]),
        message: "Approve B0\n\nTowermd-Baseline: B0\nTowermd-Draft: d-test\n".into(),
    }
}

#[test]
fn checked_out_main_snapshot_preserves_dirty_worktree_index_and_branch() {
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("source.txt"), "staged\n").unwrap();
    git(root, &["add", "source.txt"]);
    fs::write(root.join("source.txt"), "unstaged\n").unwrap();
    let head = git(root, &["rev-parse", "HEAD"]);
    let index = fs::read(root.join(".git/index")).unwrap();
    let (commit, reference) =
        written(main_writer::write(root, &snapshot(root, 2, 4), || Ok(())).unwrap());
    assert_eq!(reference, SNAPSHOT_REF);
    assert_eq!(head, git(root, &["rev-parse", "HEAD"]));
    assert_eq!(index, fs::read(root.join(".git/index")).unwrap());
    assert_eq!(
        fs::read_to_string(root.join("source.txt")).unwrap(),
        "unstaged\n"
    );
    assert!(!root.join("docs").exists());
    assert_eq!(
        git(root, &["show", "-s", "--format=%an", &commit]),
        "process:towermd-snapshot"
    );
    let changed = git(
        root,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", &commit],
    );
    assert_eq!(
        changed,
        "docs/tower/state/events/2026-10.jsonl\ndocs/tower/state/state.json"
    );
    assert_eq!(
        main_writer::latest_snapshot(root)
            .unwrap()
            .unwrap()
            .boundary,
        Boundary { epoch: 2, seq: 4 }
    );
    assert!(matches!(
        main_writer::write(root, &snapshot(root, 2, 4), || Ok(())).unwrap(),
        WriteOutcome::Superseded { .. }
    ));
}

#[test]
fn slow_publication_requires_unchecked_main_and_creates_tag_atomically() {
    let temp = fixture();
    let root = temp.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    let request = slow(head.clone());
    assert!(main_writer::write(root, &request, || Ok(()))
        .unwrap_err()
        .contains("main 已"));
    git(root, &["checkout", "--detach", "-q"]);
    let index = fs::read(root.join(".git/index")).unwrap();
    let (commit, reference) = written(main_writer::write(root, &request, || Ok(())).unwrap());
    assert_eq!(reference, "refs/heads/main");
    assert_eq!(git(root, &["rev-parse", "main"]), commit);
    assert_eq!(git(root, &["rev-parse", "towermd/B0"]), commit);
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    let trailers = git(
        root,
        &[
            "show",
            "-s",
            "--format=%(trailers:key=Towermd-Baseline,valueonly)",
            &commit,
        ],
    );
    assert_eq!(trailers, "B0");
    assert_eq!(
        git(
            root,
            &[
                "show",
                "-s",
                "--format=%(trailers:key=Towermd-Epoch,valueonly)",
                &commit
            ]
        ),
        "3"
    );
}

#[test]
fn main_in_another_worktree_also_prevents_slow_publication() {
    let temp = fixture();
    let root = temp.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    let other = temp.path().join("other");
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            other.to_str().unwrap(),
            "HEAD",
        ],
    );
    assert!(main_writer::write(&other, &slow(head), || Ok(()))
        .unwrap_err()
        .contains("main 已"));
}

#[test]
fn expired_fence_never_moves_a_reference_and_old_epoch_is_refused() {
    let temp = fixture();
    let root = temp.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    let mut checks = 0;
    let error = main_writer::write(root, &snapshot(root, 5, 7), || {
        checks += 1;
        if checks == 4 {
            Err("lease expired".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error, "lease expired");
    assert_eq!(
        git(
            root,
            &["for-each-ref", "--format=%(refname)", "refs/towermd"]
        ),
        ""
    );
    main_writer::write(root, &snapshot(root, 5, 7), || Ok(())).unwrap();
    assert!(main_writer::write(root, &snapshot(root, 4, 99), || Ok(()))
        .unwrap_err()
        .contains("更高 epoch"));
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
}

#[test]
fn private_snapshot_promotion_preserves_archive_and_does_not_refuse_itself() {
    let temp = fixture();
    let root = temp.path();
    let request = snapshot(root, 2, 4);
    let (private, _) = written(main_writer::write(root, &request, || Ok(())).unwrap());
    git(root, &["checkout", "--detach", "-q"]);
    let mut promotion = request;
    promotion.kind = WriteKind::Snapshot {
        seq: 4,
        promote_from: Some(private.clone()),
    };
    let (commit, reference) = written(main_writer::write(root, &promotion, || Ok(())).unwrap());
    assert_eq!(reference, "refs/heads/main");
    assert_eq!(
        git(
            root,
            &[
                "diff",
                "--name-only",
                &private,
                &commit,
                "--",
                "docs/tower/state"
            ]
        ),
        ""
    );
    assert_eq!(git(root, &["rev-parse", SNAPSHOT_REF]), private);
    assert!(matches!(
        main_writer::write(root, &promotion, || Ok(())).unwrap(),
        WriteOutcome::Superseded { .. }
    ));
}

#[test]
fn snapshot_paths_archives_and_promotion_content_cannot_be_rewritten() {
    let temp = fixture();
    let root = temp.path();
    let mut invalid = snapshot(root, 2, 4);
    invalid
        .changes
        .insert("source.txt".into(), Some(b"bad".to_vec()));
    assert!(main_writer::write(root, &invalid, || Ok(())).is_err());
    invalid.changes.remove("source.txt");
    invalid
        .changes
        .insert("docs/tower/state/events/2026-13.jsonl".into(), Some(vec![]));
    assert!(main_writer::write(root, &invalid, || Ok(())).is_err());
    let (private, _) = written(main_writer::write(root, &snapshot(root, 2, 4), || Ok(())).unwrap());
    let mut next = snapshot(root, 2, 5);
    next.changes.insert(
        "docs/tower/state/events/2026-10.jsonl".into(),
        Some(b"replacement\n".to_vec()),
    );
    assert!(main_writer::write(root, &next, || Ok(()))
        .unwrap_err()
        .contains("只能追加"));
    git(root, &["checkout", "--detach", "-q"]);
    let mut promotion = snapshot(root, 2, 4);
    promotion.kind = WriteKind::Snapshot {
        seq: 4,
        promote_from: Some(private),
    };
    promotion.changes.insert(
        "docs/tower/state/state.json".into(),
        Some(b"changed\n".to_vec()),
    );
    assert!(main_writer::write(root, &promotion, || Ok(()))
        .unwrap_err()
        .contains("不得改变"));
}

fn remote(root: &Path, directory: &Path) {
    git(directory, &["init", "--bare", "-q", "origin.git"]);
    let origin = directory.join("origin.git");
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(root, &["push", "-q", "origin", "main"]);
}

#[test]
fn remote_main_and_tag_are_published_without_moving_local_main_or_index() {
    let temp = fixture();
    let origin_temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    remote(root, origin_temp.path());
    let head = git(root, &["rev-parse", "HEAD"]);
    let index = fs::read(root.join(".git/index")).unwrap();
    let (commit, _) = written(main_writer::write(root, &slow(head.clone()), || Ok(())).unwrap());
    assert_eq!(
        git(
            &origin_temp.path().join("origin.git"),
            &["rev-parse", "main"]
        ),
        commit
    );
    assert_eq!(
        git(
            &origin_temp.path().join("origin.git"),
            &["rev-parse", "towermd/B0"]
        ),
        commit
    );
    assert_eq!(git(root, &["rev-parse", "main"]), head);
    assert_eq!(index, fs::read(root.join(".git/index")).unwrap());
    assert!(!root.join("docs").exists());
}

#[test]
fn snapshot_retries_remote_race_on_new_base_but_slow_approval_does_not_drift() {
    let temp = fixture();
    let origin_temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    remote(root, origin_temp.path());
    let competitor = origin_temp.path().join("competitor");
    git(
        origin_temp.path(),
        &["clone", "-q", "-b", "main", "origin.git", "competitor"],
    );
    let mut checks = 0;
    let (commit, _) = written(
        main_writer::write(root, &snapshot(root, 2, 4), || {
            checks += 1;
            if checks == 4 {
                fs::write(competitor.join("other.txt"), "concurrent\n").unwrap();
                git(&competitor, &["add", "."]);
                git(&competitor, &["commit", "-qm", "concurrent"]);
                git(&competitor, &["push", "-q", "origin", "main"]);
            }
            Ok(())
        })
        .unwrap(),
    );
    assert!(checks > 4);
    assert_eq!(
        git(root, &["show", &format!("{commit}:other.txt")]),
        "concurrent"
    );
    let old = git(root, &["rev-parse", "main"]);
    assert!(main_writer::write(root, &slow(old), || Ok(()))
        .unwrap_err()
        .contains("main 已改变"));
}

#[test]
fn malformed_or_duplicate_snapshot_trailer_is_not_a_fencing_proof() {
    let temp = fixture();
    let root = temp.path();
    git(
        root,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "invalid\n\nTowermd-Epoch: 2\nTowermd-Snapshot: 1\nTowermd-Snapshot: 2",
        ],
    );
    assert!(main_writer::latest_snapshot(root).unwrap().is_none());
    let (commit,_) = written(main_writer::write(root, &snapshot(root, 3, 9), || Ok(())).unwrap());
    assert_eq!(main_writer::latest_snapshot(root).unwrap().unwrap().commit, commit);
}

#[test]
fn forged_snapshot_producer_and_high_epoch_without_archived_main_grant_do_not_select_or_fence() {
    let temp=fixture();let root=temp.path();
    let request=snapshot(root,2,4);
    let (real,_)=written(main_writer::write(root,&request,||Ok(())).unwrap());
    for (path,bytes) in &request.changes {
        let target=root.join(path);fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target,bytes.as_ref().unwrap()).unwrap();
    }
    let state_path=root.join("docs/tower/state/state.json");
    let mut state:towermd_core::snapshot::SnapshotState=serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    // The copied archive is well formed and still has the real epoch-2 grant.
    // A spoofed author plus matching positive trailers cannot invent epoch 99.
    state.epoch=99;
    state.epoch_high_water=99;
    fs::write(&state_path,serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    git(root,&["add","docs/tower/state"]);
    git(root,&["commit","-qm","forged high snapshot\n\nTowermd-Epoch: 99\nTowermd-Snapshot: 4",
        "--author=process:towermd-snapshot <towermd@localhost>"]);
    let forged=git(root,&["rev-parse","HEAD"]);
    assert_ne!(forged,real);
    let archive=towermd_core::snapshot::Snapshot::read(root,&forged,"docs/tower","writer-project",
        |_|Ok(serde_json::Value::Null)).unwrap();
    let originals=towermd_core::project_runtime::canonical_events(&archive.events).unwrap();
    assert!(towermd_core::snapshot_publication::prove_checked(root,&forged,"docs/tower","writer-project",
        &archive,&|id|originals.get(id).cloned()).unwrap_err().contains("历史主干写入租约"));
    assert_eq!(main_writer::latest_snapshot(root).unwrap().unwrap().commit,real);
    let (next,_)=written(main_writer::write(root,&snapshot(root,2,5),||Ok(())).unwrap());
    assert_eq!(main_writer::latest_snapshot(root).unwrap().unwrap().commit,next);
    assert!(main_writer::write(root,&snapshot(root,1,9),||Ok(())).unwrap_err().contains("更高 epoch"));
}

#[test]
fn ordinary_fake_snapshot_without_epoch_is_ignored_but_real_unarchived_slow_epoch_still_fences() {
    let temp=fixture();let root=temp.path();
    git(root,&["commit","--allow-empty","-qm","ordinary\n\nTowermd-Snapshot: not-a-snapshot"]);
    assert!(main_writer::latest_snapshot(root).unwrap().is_none());
    git(root,&["checkout","--detach","-q"]);
    let base=git(root,&["rev-parse","main"]);
    main_writer::write(root,&slow(base),||Ok(())).unwrap();
    assert!(main_writer::latest_snapshot(root).unwrap().is_none());
    assert!(main_writer::write(root,&snapshot(root,2,4),||Ok(())).unwrap_err().contains("更高 epoch"));
}

#[test]
#[cfg(unix)]
fn repository_hooks_and_trailer_commands_do_not_execute_during_publication() {
    use std::os::unix::fs::PermissionsExt;
    let temp = fixture();
    let root = temp.path();
    for name in [
        "pre-commit",
        "commit-msg",
        "reference-transaction",
        "pre-push",
    ] {
        let path = root.join(".git/hooks").join(name);
        fs::write(&path, "#!/bin/sh\nexit 99\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(
        root,
        &[
            "config",
            "trailer.towermd-epoch.cmd",
            "touch trailer-command-ran",
        ],
    );
    git(root, &["config", "trailer.separators", "="]);
    let (commit, _) = written(main_writer::write(root, &snapshot(root, 2, 4), || Ok(())).unwrap());
    assert!(!root.join("trailer-command-ran").exists());
    assert_eq!(
        main_writer::latest_snapshot(root).unwrap().unwrap().commit,
        commit
    );
}

#[test]
fn task_module_epoch_does_not_fence_an_older_still_live_main_lease() {
    let temp = fixture();
    let root = temp.path();
    git(root, &["commit", "--allow-empty", "-qm",
        "task work\n\nTowermd-Task: T-1\nTowermd-Run: R-1\nTowermd-Module: A-test\nTowermd-Evidence: E-1\nTowermd-Epoch: 99"]);
    let (_, reference) = written(main_writer::write(root, &snapshot(root, 2, 4), || Ok(())).unwrap());
    assert_eq!(reference, SNAPSHOT_REF);
}

#[test]
fn cached_tags_do_not_prove_remote_publication_and_conflicting_tags_are_not_replaced() {
    let temp = fixture();
    let origin_temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    remote(root, origin_temp.path());
    let old = git(root, &["rev-parse", "HEAD"]);
    git(root, &["tag", "towermd/B0"]);
    assert_eq!(
        main_writer::read_publication(root, "refs/tags/towermd/B0").unwrap(),
        None
    );
    git(root, &["tag", "-d", "towermd/B0"]);
    let (commit, _) = written(main_writer::write(root, &slow(old.clone()), || Ok(())).unwrap());
    git(root, &["tag", "towermd/B0", &old]);
    assert!(main_writer::read_publication(root, "refs/tags/towermd/B0")
        .unwrap_err()
        .contains("冲突"));
    assert_eq!(git(root, &["rev-parse", "towermd/B0"]), old);
    assert_eq!(
        git(
            &origin_temp.path().join("origin.git"),
            &["rev-parse", "towermd/B0"]
        ),
        commit
    );
}

#[test]
fn a_remote_tag_outside_main_is_not_a_publication() {
    let temp = fixture();
    let origin_temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    remote(root, origin_temp.path());
    let original = git(root, &["rev-parse", "main"]);
    git(root, &["checkout", "-qb", "side"]);
    fs::write(root.join("side.txt"), "not merged").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "unmerged"]);
    git(root, &["tag", "towermd/B0"]);
    git(root, &["push", "-q", "origin", "refs/tags/towermd/B0"]);
    assert!(main_writer::read_publication(root, "refs/tags/towermd/B0")
        .unwrap_err()
        .contains("尚未进入"));
    assert_eq!(git(root, &["rev-parse", "main"]), original);
}

fn advance(base: &str, commit: &str) -> WriteRequest {
    WriteRequest {
        docs_dir: "docs/tower".into(),
        epoch: 3,
        kind: WriteKind::Advance { expected_base: base.into(), commit: commit.into() },
        changes: BTreeMap::new(),
        message: String::new(),
    }
}

/// A side branch in a second worktree, one commit ahead of `main`.
fn side_commit(root: &Path, path: &str) -> String {
    let side = root.join("side");
    git(root, &["worktree", "add", "-q", "-b", "side", side.to_str().unwrap(), "main"]);
    let file = side.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "side\n").unwrap();
    git(&side, &["add", "."]);
    git(&side, &["commit", "-qm", "side work"]);
    git(&side, &["rev-parse", "HEAD"])
}

#[test]
fn advance_fast_forwards_remote_main_to_an_existing_commit_without_rewriting_it() {
    let temp = fixture();
    let origin_temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    remote(root, origin_temp.path());
    let head = git(root, &["rev-parse", "HEAD"]);
    let commit = side_commit(root, "src/a.txt");
    let (written_commit, reference) = written(main_writer::write(root, &advance(&head, &commit), || Ok(())).unwrap());
    assert_eq!((written_commit.as_str(), reference.as_str()), (commit.as_str(), "refs/heads/main"));
    assert_eq!(git(&origin_temp.path().join("origin.git"), &["rev-parse", "main"]), commit);
    assert_eq!(git(root, &["rev-parse", "main"]), head, "local main is not moved");
    // A stale base is refused: the integrator must rebase again.
    let error = main_writer::write(root, &advance(&head, &commit), || Ok(())).unwrap_err();
    assert!(error.contains("main 已改变"), "{error}");
}

#[test]
fn advance_refuses_non_descendants_snapshot_paths_and_checked_out_main() {
    let temp = fixture();
    let origin_temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    // Without origin and with main checked out, nothing moves.
    let commit = side_commit(root, "src/a.txt");
    assert!(main_writer::write(root, &advance(&head, &commit), || Ok(())).unwrap_err().contains("main 已在工作树检出"));
    remote(root, origin_temp.path());
    // Snapshot paths are only written by snapshots.
    let side = root.join("side");
    fs::create_dir_all(side.join("docs/tower/state")).unwrap();
    fs::write(side.join("docs/tower/state/state.json"), "{}").unwrap();
    git(&side, &["add", "."]);
    git(&side, &["commit", "-qm", "forged state"]);
    let forged = git(&side, &["rev-parse", "HEAD"]);
    assert!(main_writer::write(root, &advance(&head, &forged), || Ok(())).unwrap_err().contains("state/"));
    // A commit that does not descend from main is refused.
    git(root, &["checkout", "-q", "--orphan", "lonely"]);
    git(root, &["commit", "-qm", "lonely", "--allow-empty"]);
    let lonely = git(root, &["rev-parse", "HEAD"]);
    assert!(main_writer::write(root, &advance(&head, &lonely), || Ok(())).unwrap_err().contains("变基"));
    // Files or a message are not part of an advance.
    let mut request = advance(&head, &commit);
    request.message = "x".into();
    assert!(main_writer::write(root, &request, || Ok(())).is_err());
    // A failed fence check moves nothing.
    assert_eq!(main_writer::write(root, &advance(&head, &commit), || Err("lease expired".into())).unwrap_err(), "lease expired");
    assert_eq!(git(&origin_temp.path().join("origin.git"), &["rev-parse", "main"]), head);
}
