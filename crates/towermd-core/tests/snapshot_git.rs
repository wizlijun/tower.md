use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use towermd_core::snapshot_git::{
    read_trailer, write_snapshot, SnapshotTrailer, WriteOutcome, WriteRequest,
};

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let output = command
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap().join("repo");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        fs::write(root.join("human.txt"), "original\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "initial"]);
        Self {
            _temporary: temporary,
            root,
        }
    }

    fn request(&self) -> WriteRequest {
        WriteRequest {
            base: git(&self.root, &["rev-parse", "main"]),
            docs_dir: "docs/tower".into(),
            state_json: r#"{"through_seq":7,"writer_epoch":2}"#.into(),
            archives: BTreeMap::from([("2026-10".into(), "{\"seq\":7}\n".into())]),
            through_seq: 7,
            writer_epoch: 2,
        }
    }

    fn remote(&self) -> PathBuf {
        let remote = self.root.parent().unwrap().join("remote.git");
        git(
            self.root.parent().unwrap(),
            &["init", "-q", "--bare", remote.to_str().unwrap()],
        );
        git(
            &self.root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&self.root, &["push", "-q", "origin", "main"]);
        remote
    }
}

fn written(outcome: WriteOutcome) -> String {
    match outcome {
        WriteOutcome::Written { commit } => commit,
        other => panic!("expected Written, got {other:?}"),
    }
}

#[test]
fn remote_snapshot_preserves_local_refs_index_and_worktree() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let remote = fixture.remote();
    fs::write(fixture.root.join("human.txt"), "staged\n").unwrap();
    git(&fixture.root, &["add", "human.txt"]);
    fs::write(fixture.root.join("human.txt"), "unstaged\n").unwrap();
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let mut checks = 0;
    let commit = written(
        write_snapshot(&fixture.root, &request, || {
            checks += 1;
            Ok(())
        })
        .unwrap(),
    );
    assert!(checks >= 2);
    assert_eq!(git(&remote, &["rev-parse", "main"]), commit);
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
    assert_eq!(
        git(&fixture.root, &["rev-parse", "origin/main"]),
        request.base,
        "only fresh fetch may move the tracking ref"
    );
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(fixture.root.join("human.txt")).unwrap(),
        "unstaged\n"
    );
    assert!(!fixture.root.join("docs").exists());
    assert!(!fixture.root.join(".git/FETCH_HEAD").exists());
    assert_eq!(
        git(
            &fixture.root,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", &commit]
        ),
        "docs/tower/state/events/2026-10.jsonl\ndocs/tower/state/state.json"
    );
    assert_eq!(
        read_trailer(&fixture.root, &commit).unwrap(),
        Some(SnapshotTrailer { seq: 7, epoch: 2 })
    );
    assert_eq!(
        git(&fixture.root, &["show", "-s", "--format=%an|%cn", &commit]),
        "process:towermd-snapshot|process:towermd-snapshot"
    );
}

#[test]
fn unchecked_main_uses_cas_without_changing_current_branch() {
    let fixture = Fixture::new();
    let request = fixture.request();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let commit = written(write_snapshot(&fixture.root, &request, || Ok(())).unwrap());
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), commit);
    assert_eq!(git(&fixture.root, &["rev-parse", "HEAD"]), request.base);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert!(!fixture.root.join("docs").exists());
}

#[test]
fn lease_rejection_does_not_write_even_temporary_files() {
    let fixture = Fixture::new();
    fixture.remote();
    let request = fixture.request();
    let error =
        write_snapshot(&fixture.root, &request, || Err("lease expired".into())).unwrap_err();
    assert!(error.contains("lease expired"), "{error}");
    assert!(!fixture.root.join(".git/towermd").exists());
    assert!(
        !fixture.root.join(".git/refs/remotes/origin/main").exists()
            || git(&fixture.root, &["rev-parse", "origin/main"]) == request.base
    );
}

#[test]
fn trailer_requires_an_unambiguous_footer() {
    let fixture = Fixture::new();
    let tree = git(&fixture.root, &["rev-parse", "HEAD^{tree}"]);
    for (message, expected) in [
        (
            "snapshot\n\nTowermd-Snapshot: 7\nTowermd-Epoch: 2",
            Ok(Some(SnapshotTrailer { seq: 7, epoch: 2 })),
        ),
        (
            "body\n\nTowermd-Snapshot: 7\nTowermd-Epoch: 2\n\nordinary final paragraph",
            Ok(None),
        ),
        (
            "snapshot\n\nTowermd-Snapshot: 7\nTowermd-Epoch: 2\nTowermd-Epoch: 3",
            Err(()),
        ),
        (
            "snapshot\n\nTowermd-Snapshot: nope\nTowermd-Epoch: 2",
            Err(()),
        ),
        ("snapshot\n\nTowermd-Snapshot: 7", Err(())),
    ] {
        let commit = git(&fixture.root, &["commit-tree", &tree, "-m", message]);
        assert_eq!(
            read_trailer(&fixture.root, &commit).map_err(|_| ()),
            expected,
            "{message}"
        );
    }
}

#[test]
fn checked_main_preserves_other_staged_and_dirty_changes() {
    let fixture = Fixture::new();
    let request = fixture.request();
    fs::write(fixture.root.join("human.txt"), "staged\n").unwrap();
    git(&fixture.root, &["add", "human.txt"]);
    fs::write(fixture.root.join("human.txt"), "dirty\n").unwrap();
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    assert!(matches!(
        write_snapshot(&fixture.root, &request, || Ok(())).unwrap(),
        WriteOutcome::Deferred { .. }
    ));
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert_eq!(git(&fixture.root, &["show", "HEAD:human.txt"]), "original");
    assert_eq!(
        fs::read_to_string(fixture.root.join("human.txt")).unwrap(),
        "dirty\n"
    );
    assert_eq!(
        git(&fixture.root, &["status", "--porcelain"]),
        "MM human.txt"
    );
    assert!(!fixture.root.join("docs").exists());
}

#[test]
fn checked_main_defers_user_files_and_intermediate_states() {
    for condition in [
        "untracked",
        "ignored",
        "staged",
        "merge",
        "rebase",
        "cherry-pick",
        "symlink",
    ] {
        let fixture = Fixture::new();
        let request = fixture.request();
        let state = fixture.root.join("docs/tower/state/state.json");
        fs::create_dir_all(state.parent().unwrap()).unwrap();
        match condition {
            "untracked" | "ignored" | "staged" => {
                fs::write(&state, "user contents").unwrap();
                if condition == "ignored" {
                    fs::write(fixture.root.join(".git/info/exclude"), "docs/\n").unwrap();
                }
                if condition == "staged" {
                    git(&fixture.root, &["add", "docs"]);
                }
            }
            "merge" => fs::write(fixture.root.join(".git/MERGE_HEAD"), &request.base).unwrap(),
            "rebase" => fs::create_dir(fixture.root.join(".git/rebase-merge")).unwrap(),
            "cherry-pick" => {
                fs::write(fixture.root.join(".git/CHERRY_PICK_HEAD"), &request.base).unwrap()
            }
            "symlink" => {
                #[cfg(unix)]
                {
                    fs::remove_dir_all(fixture.root.join("docs")).unwrap();
                    std::os::unix::fs::symlink(
                        fixture.root.parent().unwrap(),
                        fixture.root.join("docs"),
                    )
                    .unwrap();
                }
            }
            _ => unreachable!(),
        }
        let index = fs::read(fixture.root.join(".git/index")).unwrap();
        let outcome = write_snapshot(&fixture.root, &request, || Ok(())).unwrap();
        assert!(
            matches!(outcome, WriteOutcome::Deferred { .. }),
            "{condition}: {outcome:?}"
        );
        assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
        if matches!(condition, "untracked" | "ignored" | "staged") {
            assert_eq!(fs::read_to_string(state).unwrap(), "user contents");
        }
    }
}

#[test]
fn fresh_fetch_fences_new_epoch_and_stale_base() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let remote = fixture.remote();
    let tree = git(&fixture.root, &["rev-parse", "HEAD^{tree}"]);
    let next = git(
        &fixture.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &request.base,
            "-m",
            "other change",
        ],
    );
    git(&fixture.root, &["push", "origin", &format!("{next}:main")]);
    assert!(write_snapshot(&fixture.root, &request, || Ok(()))
        .unwrap_err()
        .contains("main 已推进"));
    let next = git(
        &fixture.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &next,
            "-m",
            "snapshot\n\nTowermd-Snapshot: 1\nTowermd-Epoch: 3",
        ],
    );
    git(&fixture.root, &["push", "origin", &format!("{next}:main")]);
    assert_eq!(
        write_snapshot(&fixture.root, &request, || Ok(())).unwrap(),
        WriteOutcome::Superseded {
            commit: next.clone(),
            epoch: 3,
            seq: 1
        }
    );
    assert_eq!(git(&remote, &["rev-parse", "main"]), next);
}

#[test]
fn competing_remote_push_is_rejected_without_retry() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let remote = fixture.remote();
    let tree = git(&fixture.root, &["rev-parse", "HEAD^{tree}"]);
    let next = git(
        &fixture.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &request.base,
            "-m",
            "competitor",
        ],
    );
    let mut checks = 0;
    let result = write_snapshot(&fixture.root, &request, || {
        checks += 1;
        if checks == 2 {
            git(&fixture.root, &["push", "origin", &format!("{next}:main")]);
        }
        Ok(())
    });
    assert!(result.unwrap_err().contains("push 未确认成功"));
    assert_eq!(git(&remote, &["rev-parse", "main"]), next);
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
}

#[test]
fn invalid_inputs_do_not_create_git_writer_files() {
    let fixture = Fixture::new();
    for invalid in ["month", "traversal", "docs", "state", "seq", "epoch"] {
        let mut request = fixture.request();
        match invalid {
            "month" => {
                request.archives.insert("2026-13".into(), "".into());
            }
            "traversal" => {
                request.archives.insert("../2026-10".into(), "".into());
            }
            "docs" => request.docs_dir = "../outside".into(),
            "state" => request.state_json = "{".into(),
            "seq" => request.through_seq += 1,
            "epoch" => request.writer_epoch += 1,
            _ => unreachable!(),
        }
        assert!(
            write_snapshot(&fixture.root, &request, || Ok(())).is_err(),
            "{invalid}"
        );
        assert!(!fixture.root.join(".git/towermd").exists());
    }
    fs::create_dir(fixture.root.join("subdir")).unwrap();
    assert!(write_snapshot(&fixture.root.join("subdir"), &fixture.request(), || Ok(())).is_err());
}

#[test]
fn checked_main_defers_clean_repository_until_user_switches_branch() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let references = git(&fixture.root, &["show-ref"]);
    let mut checks = 0;
    let outcome = write_snapshot(&fixture.root, &request, || {
        checks += 1;
        Ok(())
    })
    .unwrap();
    let WriteOutcome::Deferred { reason } = outcome else {
        panic!("checked main must defer: {outcome:?}");
    };
    assert!(
        reason.contains("main") && reason.contains("切换"),
        "{reason}"
    );
    assert_eq!(checks, 1);
    assert_eq!(git(&fixture.root, &["show-ref"]), references);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(fixture.root.join("human.txt")).unwrap(),
        "original\n"
    );
    assert!(!fixture.root.join("docs").exists());

    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let commit = written(write_snapshot(&fixture.root, &request, || Ok(())).unwrap());
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), commit);
    assert_eq!(git(&fixture.root, &["rev-parse", "HEAD"]), request.base);
    assert!(!fixture.root.join("docs").exists());
}

#[test]
fn checked_main_defers_before_stale_base_or_historical_fence_checks() {
    let fixture = Fixture::new();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let request = fixture.request();
    let commit = written(write_snapshot(&fixture.root, &request, || Ok(())).unwrap());
    git(&fixture.root, &["checkout", "-q", "main"]);
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    for epoch in [2, 3] {
        let mut stale = fixture.request();
        stale.base = request.base.clone();
        stale.writer_epoch = epoch;
        stale.state_json = format!(r#"{{"through_seq":7,"writer_epoch":{epoch}}}"#);
        assert!(matches!(
            write_snapshot(&fixture.root, &stale, || Ok(())).unwrap(),
            WriteOutcome::Deferred { .. }
        ));
    }
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), commit);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(fixture.root.join("docs/tower/state/state.json")).unwrap(),
        request.state_json
    );
}

#[test]
fn checked_main_preserves_existing_clean_and_dirty_snapshot() {
    let fixture = Fixture::new();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let first = fixture.request();
    written(write_snapshot(&fixture.root, &first, || Ok(())).unwrap());
    git(&fixture.root, &["checkout", "-q", "main"]);
    let mut next = fixture.request();
    next.through_seq = 8;
    next.state_json = r#"{"through_seq":8,"writer_epoch":2}"#.into();
    let state = fixture.root.join("docs/tower/state/state.json");
    for content in [first.state_json.as_str(), "human dirty"] {
        fs::write(&state, content).unwrap();
        let index = fs::read(fixture.root.join(".git/index")).unwrap();
        assert!(matches!(
            write_snapshot(&fixture.root, &next, || Ok(())).unwrap(),
            WriteOutcome::Deferred { .. }
        ));
        assert_eq!(fs::read_to_string(&state).unwrap(), content);
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
        assert_eq!(git(&fixture.root, &["rev-parse", "main"]), next.base);
    }
}

#[test]
fn linked_main_is_located_without_touching_callers_worktree() {
    let fixture = Fixture::new();
    let request = fixture.request();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let main = fixture
        .root
        .parent()
        .unwrap()
        .join("main worktree\nwith newline");
    git(
        &fixture.root,
        &["worktree", "add", main.to_str().unwrap(), "main"],
    );
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let main_index_path = git(
        &main,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    let main_index = fs::read(&main_index_path).unwrap();
    assert!(matches!(
        write_snapshot(&fixture.root, &request, || Ok(())).unwrap(),
        WriteOutcome::Deferred { .. }
    ));
    assert_eq!(git(&main, &["rev-parse", "HEAD"]), request.base);
    assert_eq!(git(&fixture.root, &["rev-parse", "HEAD"]), request.base);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert!(!fixture.root.join("docs").exists());
    assert!(!main.join("docs").exists());
    assert_eq!(fs::read(&main_index_path).unwrap(), main_index);
    git(&main, &["checkout", "-qb", "main-user-feature"]);
    let commit = written(write_snapshot(&fixture.root, &request, || Ok(())).unwrap());
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), commit);
    assert_eq!(git(&main, &["rev-parse", "HEAD"]), request.base);
    assert_eq!(fs::read(&main_index_path).unwrap(), main_index);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert!(!main.join("docs").exists());
    assert!(!fixture.root.join("docs").exists());
    assert_eq!(
        fs::read_dir(fixture.root.join(".git/towermd"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn unchecked_main_cas_rejects_concurrent_update() {
    let fixture = Fixture::new();
    let request = fixture.request();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let tree = git(&fixture.root, &["rev-parse", "HEAD^{tree}"]);
    let next = git(
        &fixture.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &request.base,
            "-m",
            "competitor",
        ],
    );
    let mut checks = 0;
    let error = write_snapshot(&fixture.root, &request, || {
        checks += 1;
        if checks == 2 {
            git(
                &fixture.root,
                &["update-ref", "refs/heads/main", &next, &request.base],
            );
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.contains("CAS"), "{error}");
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), next);
}

#[cfg(unix)]
fn executable(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn hostile_hooks_identity_and_filters_are_isolated() {
    if std::env::var_os("TOWERMD_SNAPSHOT_HOSTILE_CHILD").is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "hostile_hooks_identity_and_filters_are_isolated",
                "--nocapture",
            ])
            .env("TOWERMD_SNAPSHOT_HOSTILE_CHILD", "1")
            .env("GIT_AUTHOR_NAME", "Human")
            .env("GIT_AUTHOR_EMAIL", "human@example.invalid")
            .env("GIT_COMMITTER_NAME", "Human")
            .env("GIT_COMMITTER_EMAIL", "human@example.invalid")
            .env("GIT_AUTHOR_DATE", "invalid date")
            .env("GIT_COMMITTER_DATE", "invalid date")
            .env("GIT_INDEX_FILE", "/must-not-create-snapshot-index")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    for mode in ["checked", "unchecked", "remote"] {
        let fixture = Fixture::new();
        fs::write(
            fixture.root.join(".gitattributes"),
            "*.json filter=hostile\n*.jsonl filter=hostile\n",
        )
        .unwrap();
        git(&fixture.root, &["add", ".gitattributes"]);
        git(&fixture.root, &["commit", "-qm", "attributes"]);
        let request = fixture.request();
        if mode == "remote" {
            fixture.remote();
        }
        if mode == "unchecked" {
            git(&fixture.root, &["checkout", "-qb", "feature"]);
        }
        let hooks = fixture.root.parent().unwrap().join("hostile-hooks");
        fs::create_dir(&hooks).unwrap();
        for name in [
            "pre-commit",
            "prepare-commit-msg",
            "commit-msg",
            "post-commit",
            "reference-transaction",
            "pre-push",
            "post-rewrite",
        ] {
            executable(
                &hooks.join(name),
                "#!/bin/sh\necho ran > hook-ran\nexit 1\n",
            );
        }
        git(
            &fixture.root,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
        git(&fixture.root, &["config", "commit.gpgSign", "true"]);
        git(&fixture.root, &["config", "user.name", "Wrong identity"]);
        git(
            &fixture.root,
            &[
                "config",
                "filter.hostile.clean",
                "echo ran > filter-ran; exit 1",
            ],
        );
        git(
            &fixture.root,
            &[
                "config",
                "filter.hostile.process",
                "echo ran > filter-ran; exit 1",
            ],
        );
        git(
            &fixture.root,
            &["config", "filter.hostile.required", "true"],
        );
        let outcome = write_snapshot(&fixture.root, &request, || Ok(())).unwrap();
        if mode == "checked" {
            assert!(matches!(outcome, WriteOutcome::Deferred { .. }));
            assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
            assert!(!fixture.root.join("docs").exists());
        } else {
            let commit = written(outcome);
            assert_eq!(git(&fixture.root, &["show", "-s", "--format=%an|%ae|%cn|%ce", &commit]), "process:towermd-snapshot|towermd-snapshot@localhost|process:towermd-snapshot|towermd-snapshot@localhost");
        }
        assert!(!fixture.root.join("hook-ran").exists(), "{mode}");
        assert!(!fixture.root.join("filter-ran").exists(), "{mode}");
    }
}

#[test]
fn checked_main_preserves_unstaged_changes_hidden_by_index_flags() {
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let fixture = Fixture::new();
        git(&fixture.root, &["checkout", "-qb", "feature"]);
        let first = fixture.request();
        written(write_snapshot(&fixture.root, &first, || Ok(())).unwrap());
        git(&fixture.root, &["checkout", "-q", "main"]);
        let mut next = fixture.request();
        next.through_seq = 8;
        next.state_json = r#"{"through_seq":8,"writer_epoch":2}"#.into();
        git(
            &fixture.root,
            &["update-index", flag, "docs/tower/state/state.json"],
        );
        fs::write(
            fixture.root.join("docs/tower/state/state.json"),
            "hidden human change",
        )
        .unwrap();
        let index = fs::read(fixture.root.join(".git/index")).unwrap();
        assert!(matches!(
            write_snapshot(&fixture.root, &next, || Ok(())).unwrap(),
            WriteOutcome::Deferred { .. }
        ));
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
        assert_eq!(
            fs::read_to_string(fixture.root.join("docs/tower/state/state.json")).unwrap(),
            "hidden human change"
        );
    }
}

#[test]
fn main_checked_out_before_cas_is_deferred() {
    let fixture = Fixture::new();
    let request = fixture.request();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let mut checks = 0;
    let mut index = Vec::new();
    let outcome = write_snapshot(&fixture.root, &request, || {
        checks += 1;
        if checks == 3 {
            git(&fixture.root, &["checkout", "-q", "main"]);
            index = fs::read(fixture.root.join(".git/index")).unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert!(matches!(outcome, WriteOutcome::Deferred { .. }));
    assert_eq!(checks, 3);
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert!(!fixture.root.join("docs").exists());
}

#[test]
fn second_lease_check_prevents_remote_and_unchecked_publication() {
    for mode in ["remote", "unchecked"] {
        let fixture = Fixture::new();
        let request = fixture.request();
        let remote = if mode == "remote" {
            Some(fixture.remote())
        } else {
            git(&fixture.root, &["checkout", "-qb", "feature"]);
            None
        };
        let index = fs::read(fixture.root.join(".git/index")).unwrap();
        let mut checks = 0;
        let error = write_snapshot(&fixture.root, &request, || {
            checks += 1;
            if checks == 2 {
                Err("lease lost".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.contains("lease lost"), "{error}");
        assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
        if let Some(remote) = remote {
            assert_eq!(git(&remote, &["rev-parse", "main"]), request.base);
        }
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
        assert!(!fixture.root.join("docs").exists());
        assert_eq!(
            fs::read_dir(fixture.root.join(".git/towermd"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn state_paths_are_literal_and_month_archives_are_complete_replacements() {
    let fixture = Fixture::new();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let mut request = fixture.request();
    request.docs_dir = "./docs/[tower]*/".into();
    let first = written(write_snapshot(&fixture.root, &request, || Ok(())).unwrap());
    request.base = first;
    request.through_seq = 8;
    request.state_json = r#"{"through_seq":8,"writer_epoch":2}"#.into();
    request
        .archives
        .insert("2026-10".into(), "{\"seq\":7}\n{\"seq\":8}\n".into());
    request
        .archives
        .insert("2026-11".into(), "{\"seq\":9}\n".into());
    let commit = written(write_snapshot(&fixture.root, &request, || Ok(())).unwrap());
    assert_eq!(
        git(
            &fixture.root,
            &[
                "show",
                &format!("{commit}:docs/[tower]*/state/events/2026-10.jsonl")
            ]
        ),
        "{\"seq\":7}\n{\"seq\":8}"
    );
    assert_eq!(
        read_trailer(&fixture.root, &commit).unwrap(),
        Some(SnapshotTrailer { seq: 8, epoch: 2 })
    );
}

#[cfg(unix)]
#[test]
fn linked_main_filter_configuration_is_also_isolated() {
    let fixture = Fixture::new();
    let request = fixture.request();
    git(
        &fixture.root,
        &["config", "extensions.worktreeConfig", "true"],
    );
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let main = fixture.root.parent().unwrap().join("main");
    git(
        &fixture.root,
        &["worktree", "add", main.to_str().unwrap(), "main"],
    );
    fs::write(main.join(".gitattributes"), "*.json filter=worktree\n").unwrap();
    git(
        &main,
        &[
            "config",
            "--worktree",
            "filter.worktree.clean",
            "echo ran > filter-ran; cat",
        ],
    );
    git(
        &main,
        &["config", "--worktree", "filter.worktree.required", "true"],
    );
    assert!(matches!(
        write_snapshot(&fixture.root, &request, || Ok(())).unwrap(),
        WriteOutcome::Deferred { .. }
    ));
    assert!(!main.join("docs").exists());
    assert!(!main.join("filter-ran").exists());
}

#[test]
fn trailer_footer_accepts_crlf_and_blank_line_whitespace() {
    let fixture = Fixture::new();
    let tree = git(&fixture.root, &["rev-parse", "HEAD^{tree}"]);
    for message in [
        "snapshot\r\n\r\nTowermd-Snapshot: 7\r\nTowermd-Epoch: 2\r\n",
        "snapshot\n \nTowermd-Snapshot: 7\nTowermd-Epoch: 2\n \n",
    ] {
        let commit = git(&fixture.root, &["commit-tree", &tree, "-m", message]);
        assert_eq!(
            read_trailer(&fixture.root, &commit).unwrap(),
            Some(SnapshotTrailer { seq: 7, epoch: 2 })
        );
    }
}

#[test]
fn historical_fence_survives_ordinary_main_commits() {
    for mode in ["remote", "unchecked"] {
        let fixture = Fixture::new();
        let remote = if mode == "remote" {
            Some(fixture.remote())
        } else {
            None
        };
        if mode == "unchecked" {
            git(&fixture.root, &["checkout", "-qb", "feature"]);
        }
        let mut newer = fixture.request();
        newer.writer_epoch = 100;
        newer.through_seq = 100;
        newer.state_json = r#"{"through_seq":100,"writer_epoch":100}"#.into();
        let newer_commit = written(write_snapshot(&fixture.root, &newer, || Ok(())).unwrap());
        let tree = git(
            &fixture.root,
            &["rev-parse", &format!("{newer_commit}^{{tree}}")],
        );
        let ordinary = git(
            &fixture.root,
            &[
                "commit-tree",
                &tree,
                "-p",
                &newer_commit,
                "-m",
                "ordinary slow-layer change",
            ],
        );
        if mode == "remote" {
            git(
                &fixture.root,
                &["push", "origin", &format!("{ordinary}:refs/heads/main")],
            );
        } else {
            git(
                &fixture.root,
                &["update-ref", "refs/heads/main", &ordinary, &newer_commit],
            );
        }
        let mut stale = fixture.request();
        stale.base = ordinary.clone();
        assert_eq!(
            write_snapshot(&fixture.root, &stale, || Ok(())).unwrap(),
            WriteOutcome::Superseded {
                commit: newer_commit,
                epoch: 100,
                seq: 100
            },
            "{mode}"
        );
        assert_eq!(
            git(
                remote.as_deref().unwrap_or(&fixture.root),
                &["rev-parse", "main"]
            ),
            ordinary
        );
    }
}

#[cfg(unix)]
#[test]
fn checked_main_never_enters_index_or_worktree_write_commands() {
    if std::env::var_os("TOWERMD_CHECKED_MAIN_READONLY_CHILD").is_none() {
        let wrapper = tempfile::tempdir().unwrap();
        executable(
            &wrapper.path().join("git"),
            r#"#!/bin/sh
if test -f .review-arm; then
    for argument in "$@"; do
        case "$argument" in
            add|commit|read-tree|hash-object|update-index|write-tree|commit-tree|update-ref)
                printf '%s\n' "$argument" > .review-write-attempt
                exit 97;;
        esac
    done
fi
exec /usr/bin/git "$@"
"#,
        );
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "checked_main_never_enters_index_or_worktree_write_commands",
                "--nocapture",
            ])
            .env("TOWERMD_CHECKED_MAIN_READONLY_CHILD", "1")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    wrapper.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let fixture = Fixture::new();
    let request = fixture.request();
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    fs::write(fixture.root.join(".review-arm"), "").unwrap();
    let outcome = write_snapshot(&fixture.root, &request, || Ok(()));
    assert!(
        !fixture.root.join(".review-write-attempt").exists(),
        "{outcome:?}"
    );
    assert!(matches!(outcome.unwrap(), WriteOutcome::Deferred { .. }));
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert!(!fixture.root.join("docs").exists());
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
}

#[test]
fn symbolic_tracking_ref_cannot_redirect_fetch_into_local_branch() {
    for dangling in [false, true] {
        let fixture = Fixture::new();
        let mut request = fixture.request();
        fixture.remote();
        if !dangling {
            git(&fixture.root, &["branch", "human-feature"]);
        }
        let old_heads = git(
            &fixture.root,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads/",
            ],
        );
        let tree = git(&fixture.root, &["rev-parse", "HEAD^{tree}"]);
        let next = git(
            &fixture.root,
            &[
                "commit-tree",
                &tree,
                "-p",
                &request.base,
                "-m",
                "ordinary remote change",
            ],
        );
        git(
            &fixture.root,
            &["push", "origin", &format!("{next}:refs/heads/main")],
        );
        git(
            &fixture.root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/main",
                "refs/heads/human-feature",
            ],
        );
        request.base = next;
        let _ = write_snapshot(&fixture.root, &request, || Ok(()));
        assert_eq!(
            git(
                &fixture.root,
                &[
                    "for-each-ref",
                    "--format=%(refname) %(objectname)",
                    "refs/heads/"
                ]
            ),
            old_heads,
            "dangling={dangling}"
        );
        assert_eq!(
            git(&fixture.root, &["symbolic-ref", "refs/remotes/origin/main"]),
            "refs/heads/human-feature"
        );
    }
}

#[test]
fn fencing_uses_historical_tuple_maximum_including_merged_ancestors() {
    let fixture = Fixture::new();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    let mut request = fixture.request();
    let tree = git(&fixture.root, &["rev-parse", "HEAD^{tree}"]);
    let maximum = git(
        &fixture.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &request.base,
            "-m",
            "snapshot\n\nTowermd-Snapshot: 100\nTowermd-Epoch: 100",
        ],
    );
    let first_parent = git(
        &fixture.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &request.base,
            "-m",
            "snapshot\n\nTowermd-Snapshot: 1000\nTowermd-Epoch: 99",
        ],
    );
    let merge = git(
        &fixture.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &first_parent,
            "-p",
            &maximum,
            "-m",
            "merge",
        ],
    );
    let tip = git(
        &fixture.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &merge,
            "-m",
            "snapshot\n\nTowermd-Snapshot: 99\nTowermd-Epoch: 100",
        ],
    );
    git(
        &fixture.root,
        &["update-ref", "refs/heads/main", &tip, &request.base],
    );
    request.base = tip;
    assert_eq!(
        write_snapshot(&fixture.root, &request, || Ok(())).unwrap(),
        WriteOutcome::Superseded {
            commit: maximum,
            epoch: 100,
            seq: 100
        }
    );
    request.writer_epoch = 101;
    request.through_seq = 1;
    request.state_json = r#"{"through_seq":1,"writer_epoch":101}"#.into();
    written(write_snapshot(&fixture.root, &request, || Ok(())).unwrap());
}

#[test]
fn temporary_fetch_refs_are_removed_after_success_and_stale_base_failure() {
    let fixture = Fixture::new();
    let request = fixture.request();
    fixture.remote();
    let refs = git(&fixture.root, &["show-ref"]);
    written(write_snapshot(&fixture.root, &request, || Ok(())).unwrap());
    assert_eq!(git(&fixture.root, &["show-ref"]), refs);
    let mut newer = fixture.request();
    newer.writer_epoch = 3;
    newer.state_json = r#"{"through_seq":7,"writer_epoch":3}"#.into();
    assert!(write_snapshot(&fixture.root, &newer, || Ok(()))
        .unwrap_err()
        .contains("main 已推进"));
    assert_eq!(git(&fixture.root, &["show-ref"]), refs);
    git(
        &fixture.root,
        &[
            "remote",
            "set-url",
            "origin",
            "/missing-towermd-fixture-remote",
        ],
    );
    assert!(write_snapshot(&fixture.root, &newer, || Ok(())).is_err());
    assert_eq!(git(&fixture.root, &["show-ref"]), refs);
}

#[test]
fn incomplete_or_invalid_history_cannot_bypass_fencing() {
    for condition in ["shallow", "malformed-footer"] {
        let fixture = Fixture::new();
        git(&fixture.root, &["checkout", "-qb", "feature"]);
        let mut request = fixture.request();
        if condition == "shallow" {
            fs::write(
                fixture.root.join(".git/shallow"),
                format!("{}\n", request.base),
            )
            .unwrap();
        } else {
            let tree = git(&fixture.root, &["rev-parse", "HEAD^{tree}"]);
            let malformed = git(
                &fixture.root,
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    &request.base,
                    "-m",
                    "snapshot\n\nTowermd-Snapshot: 100\nTowermd-Epoch: invalid",
                ],
            );
            let ordinary = git(
                &fixture.root,
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    &malformed,
                    "-m",
                    "ordinary commit",
                ],
            );
            git(
                &fixture.root,
                &["update-ref", "refs/heads/main", &ordinary, &request.base],
            );
            request.base = ordinary;
        }
        assert!(
            write_snapshot(&fixture.root, &request, || Ok(())).is_err(),
            "{condition}"
        );
        assert_eq!(git(&fixture.root, &["rev-parse", "main"]), request.base);
        assert!(!fixture.root.join("docs").exists());
    }
}

#[test]
fn symbolic_local_main_is_rejected_without_moving_human_branch() {
    let fixture = Fixture::new();
    git(&fixture.root, &["switch", "-qc", "human-branch"]);
    git(
        &fixture.root,
        &["symbolic-ref", "refs/heads/main", "refs/heads/human-branch"],
    );
    let request = fixture.request();
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let result = write_snapshot(&fixture.root, &request, || Ok(()));
    assert_eq!(
        git(&fixture.root, &["rev-parse", "human-branch"]),
        request.base
    );
    assert_eq!(
        git(&fixture.root, &["symbolic-ref", "refs/heads/main"]),
        "refs/heads/human-branch"
    );
    assert!(result.unwrap_err().contains("符号引用"));
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert!(!fixture.root.join("docs").exists());
}

#[test]
fn main_changed_to_symref_during_lease_check_is_rejected() {
    let fixture = Fixture::new();
    let request = fixture.request();
    git(&fixture.root, &["switch", "-qc", "human-branch"]);
    let mut checks = 0;
    let result = write_snapshot(&fixture.root, &request, || {
        checks += 1;
        if checks == 3 {
            git(
                &fixture.root,
                &["symbolic-ref", "refs/heads/main", "refs/heads/human-branch"],
            );
        }
        Ok(())
    });
    assert_eq!(
        git(&fixture.root, &["rev-parse", "human-branch"]),
        request.base
    );
    assert!(result.unwrap_err().contains("符号引用"));
}

#[test]
fn dangling_symbolic_main_is_rejected_without_creating_its_target() {
    let fixture = Fixture::new();
    let request = fixture.request();
    git(&fixture.root, &["switch", "-qc", "human-branch"]);
    git(
        &fixture.root,
        &[
            "symbolic-ref",
            "refs/heads/main",
            "refs/heads/missing-human-branch",
        ],
    );
    let error = write_snapshot(&fixture.root, &request, || Ok(())).unwrap_err();
    assert!(error.contains("悬空符号引用"), "{error}");
    assert_eq!(
        git(&fixture.root, &["symbolic-ref", "refs/heads/main"]),
        "refs/heads/missing-human-branch"
    );
    assert!(git(
        &fixture.root,
        &["for-each-ref", "refs/heads/missing-human-branch"]
    )
    .is_empty());
    assert_eq!(
        git(&fixture.root, &["rev-parse", "human-branch"]),
        request.base
    );
    assert!(!fixture.root.join("docs").exists());
}

#[cfg(unix)]
#[test]
fn local_cas_never_dereferences_a_symref_inserted_after_validation() {
    if std::env::var_os("TOWERMD_MAIN_SYMREF_CHILD").is_none() {
        let wrapper = tempfile::tempdir().unwrap();
        executable(
            &wrapper.path().join("git"),
            r#"#!/bin/sh
update_ref=0
for argument in "$@"; do
    if test "$argument" = update-ref; then update_ref=1; fi
done
if test -f .review-main-symref && test "$update_ref" = 1; then
    rm .review-main-symref
    /usr/bin/git symbolic-ref refs/heads/main refs/heads/human-branch || exit $?
fi
exec /usr/bin/git "$@"
"#,
        );
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local_cas_never_dereferences_a_symref_inserted_after_validation",
                "--nocapture",
            ])
            .env("TOWERMD_MAIN_SYMREF_CHILD", "1")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    wrapper.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let fixture = Fixture::new();
    let request = fixture.request();
    git(&fixture.root, &["switch", "-qc", "human-branch"]);
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    fs::write(fixture.root.join(".review-main-symref"), "").unwrap();
    let _ = write_snapshot(&fixture.root, &request, || Ok(()));
    assert!(!fixture.root.join(".review-main-symref").exists());
    assert_eq!(
        git(&fixture.root, &["rev-parse", "human-branch"]),
        request.base
    );
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert!(!fixture.root.join("docs").exists());
}
