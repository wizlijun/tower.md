use std::{path::Path, time::Duration};
use towermd_core::{repo, task_gates::*};
#[test]
fn raw_z_preserves_deletions_newlines_and_sha256_oids() {
    for width in [40, 64] {
        let raw = format!(
            ":100644 000000 {} {} D\0src/a\n b.rs\0",
            "a".repeat(width),
            "0".repeat(width)
        );
        let changes = parse_raw_z(raw.as_bytes()).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, b"src/a\n b.rs");
        assert_eq!((changes[0].old_mode, changes[0].new_mode), (0o100644, 0));
        assert_eq!(changes[0].old_oid, "a".repeat(width));
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=codex/gpt-6",
            "-c",
            "user.email=codex@towermd.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
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
#[test]
fn bounded_git_bytes_retains_non_utf8_and_all_trailing_bytes() {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(t.path().join("bytes"), [0xff, b'\n', b'\0', b'\n']).unwrap();
    git(t.path(), &["add", "bytes"]);
    git(t.path(), &["commit", "-qm", "fixture"]);
    assert_eq!(
        repo::git_bytes_timeout(t.path(), &["show", "HEAD:bytes"], Duration::from_secs(2)).unwrap(),
        [0xff, b'\n', b'\0', b'\n']
    );
    assert!(
        repo::git_bytes_timeout(t.path(), &["show", "HEAD:missing"], Duration::from_secs(2))
            .is_err()
    );
}

#[test]
fn raw_z_rejects_unsupported_modes() {
    for mode in ["120000", "160000", "100664", "0100644"] {
        let raw = format!(
            ":{mode} 100644 {} {} M\0src/x\0",
            "a".repeat(40),
            "b".repeat(40)
        );
        assert!(parse_raw_z(raw.as_bytes()).is_err(), "{mode}");
    }
}
#[test]
fn raw_z_rejects_invalid_and_mismatched_oids() {
    for oid in [
        format!("g{}", "a".repeat(39)),
        "a".repeat(39),
        "a".repeat(64),
        "0".repeat(40),
    ] {
        let raw = format!(":100644 100644 {oid} {} M\0src/x\0", "b".repeat(40));
        assert!(parse_raw_z(raw.as_bytes()).is_err(), "{oid}");
    }
}
#[test]
fn raw_z_rejects_illegal_or_non_utf8_paths() {
    for path in [
        "../x",
        "/x",
        "a//b",
        "a/./b",
        "a/../b",
        "a\\b",
        ".git/config",
        "",
    ] {
        let raw = format!(
            ":100644 100644 {} {} M\0{path}\0",
            "a".repeat(40),
            "b".repeat(40)
        );
        assert!(parse_raw_z(raw.as_bytes()).is_err(), "{path:?}");
    }
    let mut raw = format!(
        ":100644 100644 {} {} M\0src/",
        "a".repeat(40),
        "b".repeat(40)
    )
    .into_bytes();
    raw.extend([0xff, 0]);
    assert!(parse_raw_z(&raw).is_err());
}
#[test]
fn raw_z_rejects_invalid_status_or_missing_side() {
    for (old, new, status) in [
        ("100644", "100644", "X"),
        ("100644", "100644", "A"),
        ("000000", "100644", "M"),
        ("000000", "000000", "D"),
    ] {
        let raw = format!(
            ":{old} {new} {} {} {status}\0src/x\0",
            "a".repeat(40),
            "b".repeat(40)
        );
        assert!(parse_raw_z(raw.as_bytes()).is_err());
    }
}
#[test]
fn raw_z_rejects_duplicate_paths() {
    let row = format!(
        ":100644 100644 {} {} M\0src/x\0",
        "a".repeat(40),
        "b".repeat(40)
    );
    assert!(parse_raw_z((row.clone() + &row).as_bytes()).is_err());
}
#[test]
fn raw_z_rejects_truncated_rename_and_bad_header_formats() {
    for raw in [
        b":bad\0x\0".as_slice(),
        b":100644 100644 a b M\0src/a",
        b":100644 100644 a b R100\0old\0new\0",
    ] {
        assert!(matches!(
            parse_raw_z(raw),
            Err(GateError::Infrastructure(_))
        ));
    }
}

use towermd_core::{
    catalog::Catalog,
    init::{init, InitOptions},
    recovery_contract::finish::FrozenFinish,
};
struct Fixture {
    root: tempfile::TempDir,
    frozen: FrozenFinish,
}
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
fn commit(root: &Path) -> String {
    git(root, &["add", "--all"]);
    git(root, &["commit", "-qm", "contract Git fixture"]);
    git(root, &["rev-parse", "HEAD"])
}
fn fixture(sha256: bool) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let mut args = vec!["init", "-q", "-b", "main"];
    if sha256 {
        args.push("--object-format=sha256");
    }
    git(root.path(), &args);
    init(
        root.path(),
        &InitOptions {
            docs_dir: Some("docs/tower".into()),
        },
    )
    .unwrap();
    write(root.path(), "src/code", "old\n");
    let base = commit(root.path());
    git(root.path(), &["checkout", "-qb", "t/T"]);
    write(root.path(), "src/code", "task\n");
    let task = commit(root.path());
    let tree = git(root.path(), &["rev-parse", "HEAD^{tree}"]);
    let c = Catalog::load(root.path()).unwrap();
    let frozen = FrozenFinish {
        evidence_id: "E".into(),
        project_id: c.config.project_id().into(),
        host: "oldhost".into(),
        root: "/old/root".into(),
        common_dir: "/old/common".into(),
        task_id: "T".into(),
        run_id: "R".into(),
        module_id: "M".into(),
        commit: task,
        tree,
        base: base.clone(),
        authority: base,
        baseline: "towermd/B0".into(),
        plan_hash: None,
        commands: vec![],
        commands_hash: "a".repeat(64),
        fences: vec![],
        entered_verifying_at: 1,
        merge_deadline: 2,
    };
    Fixture { root, frozen }
}
#[test]
fn g5_accepts_real_same_tree_and_sha256_repository_without_mutating_worktree() {
    for sha256 in [false, true] {
        let f = fixture(sha256);
        let c = Catalog::load(f.root.path()).unwrap();
        let index = std::fs::read(f.root.path().join(".git/index")).unwrap();
        assert!(check_g5(&c, &f.frozen, &f.frozen.commit, &f.frozen.base).is_ok());
        assert_eq!(
            std::fs::read(f.root.path().join(".git/index")).unwrap(),
            index
        );
        assert_eq!(git(f.root.path(), &["rev-parse", "HEAD"]), f.frozen.commit);
    }
}
#[test]
fn g5_accepts_only_state_canvas_base_advance_after_actual_rebase() {
    let f = fixture(false);
    git(f.root.path(), &["checkout", "-q", "main"]);
    write(f.root.path(), "docs/tower/state/example", "state\n");
    write(f.root.path(), "docs/tower/tower.canvas", "canvas\n");
    let base = commit(f.root.path());
    git(f.root.path(), &["checkout", "-q", "t/T"]);
    git(f.root.path(), &["rebase", "main"]);
    let candidate = git(f.root.path(), &["rev-parse", "HEAD"]);
    assert_ne!(candidate, f.frozen.commit);
    let c = Catalog::load(f.root.path()).unwrap();
    let result = check_g5(&c, &f.frozen, &candidate, &base);
    assert!(result.is_ok(), "{result:?}");
}
fn advance_and_revert(f: &Fixture, path: &str) -> String {
    git(f.root.path(), &["checkout", "-q", "main"]);
    let old = std::fs::read(f.root.path().join(path)).unwrap();
    let mut changed = old.clone();
    changed.extend_from_slice(if path.ends_with("yaml") {
        b"snapshot_interval: 99\n"
    } else {
        b"advance\n"
    });
    std::fs::write(f.root.path().join(path), changed).unwrap();
    commit(f.root.path());
    std::fs::write(f.root.path().join(path), old).unwrap();
    let base = commit(f.root.path());
    git(f.root.path(), &["checkout", "-q", "t/T"]);
    git(f.root.path(), &["rebase", "main"]);
    base
}
#[test]
fn g5_rejects_code_advance_then_revert_even_when_final_tree_coincides() {
    let f = fixture(false);
    let base = advance_and_revert(&f, "src/code");
    let candidate = git(f.root.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        git(f.root.path(), &["rev-parse", "HEAD^{tree}"]),
        f.frozen.tree
    );
    assert!(matches!(
        check_g5(
            &Catalog::load(f.root.path()).unwrap(),
            &f.frozen,
            &candidate,
            &base
        ),
        Err(GateError::ProofRejected(_))
    ));
}
#[test]
fn g5_rejects_config_advance_then_revert_even_when_final_tree_coincides() {
    let f = fixture(false);
    let base = advance_and_revert(&f, ".towermd/config.yaml");
    let candidate = git(f.root.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        git(f.root.path(), &["rev-parse", "HEAD^{tree}"]),
        f.frozen.tree
    );
    assert!(matches!(
        check_g5(
            &Catalog::load(f.root.path()).unwrap(),
            &f.frozen,
            &candidate,
            &base
        ),
        Err(GateError::ProofRejected(_))
    ));
}
#[test]
fn g5_checks_every_merge_parent_edge_in_base_advance() {
    let f = fixture(false);
    git(f.root.path(), &["checkout", "-qb", "side", "main"]);
    write(f.root.path(), "src/side", "code\n");
    commit(f.root.path());
    std::fs::remove_file(f.root.path().join("src/side")).unwrap();
    commit(f.root.path());
    git(f.root.path(), &["checkout", "-q", "main"]);
    write(f.root.path(), "docs/tower/state/state-fixture", "state\n");
    commit(f.root.path());
    git(
        f.root.path(),
        &[
            "merge",
            "--no-ff",
            "-qm",
            "merge reverted code side",
            "side",
        ],
    );
    let base = git(f.root.path(), &["rev-parse", "HEAD"]);
    git(f.root.path(), &["checkout", "-q", "t/T"]);
    git(f.root.path(), &["rebase", "main"]);
    let candidate = git(f.root.path(), &["rev-parse", "HEAD"]);
    assert!(matches!(
        check_g5(
            &Catalog::load(f.root.path()).unwrap(),
            &f.frozen,
            &candidate,
            &base
        ),
        Err(GateError::ProofRejected(_))
    ));
}
#[test]
fn g5_state_exception_never_allows_task_itself_to_write_state_or_canvas() {
    for path in ["docs/tower/state/task", "docs/tower/tower.canvas"] {
        let mut f = fixture(false);
        write(f.root.path(), path, "task write\n");
        f.frozen.commit = commit(f.root.path());
        f.frozen.tree = git(f.root.path(), &["rev-parse", "HEAD^{tree}"]);
        assert!(matches!(
            check_g5(
                &Catalog::load(f.root.path()).unwrap(),
                &f.frozen,
                &f.frozen.commit,
                &f.frozen.base
            ),
            Err(GateError::Protected(_))
        ));
    }
}
#[test]
fn g5_rejects_fake_tree_base_context_or_task_code_as_base_advance() {
    let f = fixture(false);
    let c = Catalog::load(f.root.path()).unwrap();
    let mut bad = f.frozen.clone();
    bad.tree = git(f.root.path(), &["rev-parse", "main^{tree}"]);
    assert!(check_g5(&c, &bad, &f.frozen.commit, &f.frozen.base).is_err());
    let mut bad = f.frozen.clone();
    bad.project_id = "fake".into();
    assert!(check_g5(&c, &bad, &f.frozen.commit, &f.frozen.base).is_err());
    assert!(check_g5(&c, &f.frozen, &f.frozen.commit, &f.frozen.commit).is_err());
    assert!(check_g5(&c, &f.frozen, &f.frozen.commit, &"f".repeat(40)).is_err());
}
#[cfg(unix)]
#[test]
fn g5_rejects_real_symlink_or_gitlink_changes() {
    for gitlink in [false, true] {
        let mut f = fixture(false);
        if gitlink {
            git(
                f.root.path(),
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("160000,{},src/sub", f.frozen.base),
                ],
            );
            git(f.root.path(), &["commit", "-qm", "gitlink fixture"]);
        } else {
            std::os::unix::fs::symlink("code", f.root.path().join("src/link")).unwrap();
            commit(f.root.path());
        }
        f.frozen.commit = git(f.root.path(), &["rev-parse", "HEAD"]);
        f.frozen.tree = git(f.root.path(), &["rev-parse", "HEAD^{tree}"]);
        assert!(matches!(
            check_g5(
                &Catalog::load(f.root.path()).unwrap(),
                &f.frozen,
                &f.frozen.commit,
                &f.frozen.base
            ),
            Err(GateError::UnsupportedMode(_))
        ));
    }
}

#[test]
fn g5_rejects_commit_object_hidden_under_regular_file_mode() {
    let f = fixture(false);
    // Actual Git permits this tree: mode 100644 with a commit OID instead of a blob OID.
    // Build a state-only main advance and its rebased task candidate without checking out that invalid entry.
    let cache = format!("100644,{},docs/tower/state/invalid-object", f.frozen.base);
    git(f.root.path(), &["read-tree", &f.frozen.base]);
    git(
        f.root.path(),
        &["update-index", "--add", "--cacheinfo", &cache],
    );
    let base_tree = git(f.root.path(), &["write-tree"]);
    let base = git(
        f.root.path(),
        &[
            "commit-tree",
            &base_tree,
            "-p",
            &f.frozen.base,
            "-m",
            "invalid blob fixture",
        ],
    );
    git(f.root.path(), &["update-ref", "refs/heads/main", &base]);
    git(f.root.path(), &["read-tree", &f.frozen.commit]);
    git(
        f.root.path(),
        &["update-index", "--add", "--cacheinfo", &cache],
    );
    let tree = git(f.root.path(), &["write-tree"]);
    let candidate = git(
        f.root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &base,
            "-m",
            "rebased task fixture",
        ],
    );
    let result = check_g5(
        &Catalog::load(f.root.path()).unwrap(),
        &f.frozen,
        &candidate,
        &base,
    );
    assert!(
        matches!(result, Err(GateError::InvalidObject(_))),
        "{result:?}"
    );
}
