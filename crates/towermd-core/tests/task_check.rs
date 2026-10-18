use std::{collections::BTreeMap, fs, path::Path};
use towermd_core::{
    catalog::Catalog,
    hub::Hub,
    hub_protocol::{HubProtocol, Lease},
    init::{init, InitOptions},
    recovery_outbox::{RecoveryContext, RecoveryInputs, RecoveryOutbox},
    task_gates::{check, check_shared, CheckReport, GateError},
    tasks::{Run, Task, TaskCommand, TaskService, TaskSpec},
};

fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@invalid",
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
struct Fixture {
    temp: tempfile::TempDir,
    task: Task,
    run: Run,
    leases: BTreeMap<String, Lease>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_allowed("src/a/**")
    }
    fn with_allowed(allowed: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "-qb", "main"]);
        init(
            &root,
            &InitOptions {
                docs_dir: Some("docs/tower".into()),
            },
        )
        .unwrap();
        fs::create_dir_all(root.join("src/a")).unwrap();
        fs::write(root.join("src/a/code.rs"), "original\n").unwrap();
        fs::write(
            root.join("docs/tower/arch/A.md"),
            format!("---\ntype: Module\nid: A\npaths: [src/**, '{allowed}']\n---\n"),
        )
        .unwrap();
        fs::write(
            root.join("docs/tower/arch/B.md"),
            "---\ntype: Module\nid: B\npaths: [src/b/**]\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/tower/specs/AC.md"),
            "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "actual authority"]);
        git(&root, &["switch", "-qc", "t/T"]);
        let c = Catalog::load(&root).unwrap();
        let mut hub = Hub::open(&temp.path().join("hub")).unwrap();
        let mut outbox = RecoveryOutbox::open(
            &temp.path().join("outbox"),
            RecoveryContext::from_catalog(&c).unwrap(),
        )
        .unwrap();
        let mut service = TaskService::default();
        service
            .execute(
                &mut hub,
                &c,
                "host",
                TaskCommand::Add {
                    task: TaskSpec {
                        id: "T".into(),
                        title: "Check actual commit".into(),
                        module: "A".into(),
                        touches: vec!["B".into()],
                        allowed_paths: vec![allowed.into()],
                        depends_on: vec![],
                        acceptance: vec!["AC".into()],
                        decisions: vec![],
                    },
                },
                None,
                1,
                &mut outbox,
            )
            .unwrap();
        let added = hub.subscribe(0, 1).unwrap();
        for event in &added.events {
            service.apply(event).unwrap();
        }
        let run = serde_json::from_value(
            service
                .execute(
                    &mut hub,
                    &c,
                    "host",
                    TaskCommand::Claim {
                        id: "T".into(),
                        harness: None,
                        harness_version: None,
                    },
                    None,
                    1,
                    &mut outbox,
                )
                .unwrap(),
        )
        .unwrap();
        for event in &hub.subscribe(added.head.contiguous_seq, 1).unwrap().events {
            service.apply(event).unwrap();
        }
        let status = service
            .execute(
                &mut hub,
                &c,
                "host",
                TaskCommand::Status,
                None,
                1,
                &mut outbox,
            )
            .unwrap();
        let task = serde_json::from_value(status["tasks"][0].clone()).unwrap();
        let leases = hub.subscribe(0, 1).unwrap().head.leases;
        Self {
            temp,
            task,
            run,
            leases,
        }
    }
    fn root(&self) -> std::path::PathBuf {
        self.temp.path().join("repo")
    }
    fn commit(&self, path: &str, text: &str) {
        let file = self.root().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        git(&self.root(), &["add", "--", path]);
        git(&self.root(), &["commit", "-qm", "actual task change"]);
    }
    fn check(&self) -> Result<CheckReport, GateError> {
        check(
            &Catalog::load(&self.root()).unwrap(),
            &self.task,
            &self.run,
            "host",
            &self.leases,
            2,
        )
    }
}

#[test]
fn live_check_binds_actual_clean_commit_without_running_verification() {
    let f = Fixture::new();
    f.commit("src/a/code.rs", "changed\n");
    let index = fs::read(f.root().join(".git/index")).unwrap();
    let refs = git(&f.root(), &["show-ref"]);
    let report = f.check().unwrap();
    assert_eq!(report.commit, git(&f.root(), &["rev-parse", "HEAD"]));
    assert_eq!(report.tree, git(&f.root(), &["rev-parse", "HEAD^{tree}"]));
    assert_eq!(report.base, git(&f.root(), &["rev-parse", "main"]));
    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].path, b"src/a/code.rs");
    assert_eq!(
        report.gates,
        BTreeMap::from([
            ("G0".into(), true),
            ("G2".into(), true),
            ("G3".into(), false),
            ("G5".into(), true)
        ])
    );
    assert_eq!(fs::read(f.root().join(".git/index")).unwrap(), index);
    assert_eq!(git(&f.root(), &["show-ref"]), refs);
}

#[cfg(unix)]
#[test]
fn live_check_never_mixes_head_commit_and_another_tree() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    const CHILD: &str = "TOWERMD_CHECK_OID_RACE_CHILD";
    const NAME: &str = "live_check_never_mixes_head_commit_and_another_tree";
    if std::env::var_os(CHILD).is_none() {
        assert!(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture"])
            .env(CHILD, "1")
            .status()
            .unwrap()
            .success());
        return;
    }
    let f = Fixture::new();
    let a = git(&f.root(), &["rev-parse", "HEAD"]);
    git(&f.root(), &["switch", "-qc", "race-other-tree"]);
    f.commit("src/a/code.rs", "another actual tree\n");
    let b = git(&f.root(), &["rev-parse", "HEAD"]);
    git(&f.root(), &["switch", "t/T"]);
    let catalog = Catalog::load(&f.root()).unwrap();
    let real_git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("git"))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
    let real = quote(real_git.to_str().unwrap());
    let shim = f.temp.path().join("race-bin");
    fs::create_dir(&shim).unwrap();
    fs::write(
        shim.join("git"),
        format!(
            r#"#!/bin/sh
case "$*" in
  'rev-parse --verify HEAD^{{commit}}')
    {real} "$@"
    result=$?
    {real} update-ref refs/heads/t/T {b}
    exit "$result" ;;
  'rev-parse --verify HEAD^{{tree}}')
    {real} "$@"
    result=$?
    {real} update-ref refs/heads/t/T {a}
    exit "$result" ;;
esac
exec {real} "$@"
"#
        ),
    )
    .unwrap();
    fs::set_permissions(shim.join("git"), fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(shim).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    // Only this isolated child changes PATH. The real working tree remains A;
    // the shim deterministically moves the owned branch between each HEAD read.
    std::env::set_var("PATH", path);
    assert_eq!(
        check(&catalog, &f.task, &f.run, "host", &f.leases, 2),
        Err(GateError::NotLatestMain)
    );
}

#[test]
fn live_check_rejects_every_binding_mismatch_and_another_branch() {
    let f = Fixture::new();
    for field in ["task", "host", "worktree", "branch"] {
        let mut run = f.run.clone();
        match field {
            "task" => run.task = "another".into(),
            "host" => run.host = "another".into(),
            "worktree" => run.worktree = f.temp.path().display().to_string(),
            _ => run.branch = "t/another".into(),
        }
        assert_eq!(
            check(
                &Catalog::load(&f.root()).unwrap(),
                &f.task,
                &run,
                "host",
                &f.leases,
                2
            ),
            Err(GateError::WrongBinding),
            "{field}"
        );
    }
    git(&f.root(), &["switch", "-qc", "t/another"]);
    assert_eq!(f.check(), Err(GateError::WrongBinding));
}

#[test]
fn live_check_requires_exact_all_module_fences_and_unexpired_leases() {
    let f = Fixture::new();
    let catalog = Catalog::load(&f.root()).unwrap();
    for failure in [
        "touches_epoch",
        "expired",
        "missing",
        "duplicate",
        "wrong_holder",
        "missing_fence",
    ] {
        let mut leases = f.leases.clone();
        let mut run = f.run.clone();
        match failure {
            "touches_epoch" => leases.get_mut("module:B").unwrap().epoch += 1,
            "expired" => leases.get_mut("module:B").unwrap().expires_at = 2,
            "missing" => {
                leases.remove("module:B");
            }
            "duplicate" => run.leases.push(run.leases[0].clone()),
            "wrong_holder" => leases.get_mut("module:B").unwrap().holder = "another".into(),
            _ => {
                run.leases.pop();
            }
        }
        assert_eq!(
            check(&catalog, &f.task, &run, "host", &leases, 2),
            Err(GateError::StaleFence),
            "{failure}"
        );
    }
}

#[test]
fn live_check_rejects_dirty_staged_and_untracked_without_cleaning() {
    for state in ["dirty", "staged", "untracked"] {
        let f = Fixture::new();
        let path = if state == "untracked" {
            "src/a/new.rs"
        } else {
            "src/a/code.rs"
        };
        fs::write(f.root().join(path), "uncommitted\n").unwrap();
        if state == "staged" {
            git(&f.root(), &["add", "--", path]);
        }
        let index = fs::read(f.root().join(".git/index")).unwrap();
        assert_eq!(f.check(), Err(GateError::Dirty), "{state}");
        assert_eq!(fs::read(f.root().join(path)).unwrap(), b"uncommitted\n");
        assert_eq!(fs::read(f.root().join(".git/index")).unwrap(), index);
    }
}

#[test]
fn live_check_refuses_all_git_operations_without_removing_markers() {
    let f = Fixture::new();
    for operation in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
    ] {
        let path = f
            .root()
            .join(git(&f.root(), &["rev-parse", "--git-path", operation]));
        fs::write(&path, "owned operation marker").unwrap();
        assert_eq!(
            f.check(),
            Err(GateError::OperationInProgress(operation.into()))
        );
        assert_eq!(fs::read(&path).unwrap(), b"owned operation marker");
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn live_check_authorizes_segments_and_preserves_non_line_paths() {
    for (path, allowed) in [
        ("src/a/new/deep.rs", true),
        ("src/a/line\n name.rs", true),
        ("src/a2/code.rs", false),
        ("src/b/code.rs", false),
    ] {
        let f = Fixture::new();
        f.commit(path, "actual code\n");
        if allowed {
            assert_eq!(
                f.check().unwrap().changed.last().unwrap().path,
                path.as_bytes()
            );
        } else {
            assert_eq!(
                f.check(),
                Err(GateError::OutsideTask(path.as_bytes().to_vec()))
            );
        }
    }
    let f = Fixture::with_allowed("src/a/*?.rs");
    f.commit("src/a/z.rs", "ok\n");
    assert!(f.check().is_ok());
    f.commit("src/a/nested/z.rs", "outside single segment\n");
    assert!(matches!(f.check(), Err(GateError::OutsideTask(_))));
}

#[test]
fn live_check_globstar_matches_descendants_without_authorizing_prefix_replacement() {
    let f = Fixture::with_allowed("src/a/**/x?.rs");
    f.commit("src/a/x1.rs", "zero intermediate directories\n");
    f.commit("src/a/nested/x2.rs", "one intermediate directory\n");
    assert!(f.check().is_ok());
    let f = Fixture::new();
    fs::remove_dir_all(f.root().join("src/a")).unwrap();
    fs::write(
        f.root().join("src/a"),
        "directory prefix replaced by a file\n",
    )
    .unwrap();
    git(&f.root(), &["add", "-A"]);
    git(
        &f.root(),
        &["commit", "-qm", "replace authorized subtree prefix"],
    );
    assert_eq!(f.check(), Err(GateError::OutsideTask(b"src/a".to_vec())));
}

#[test]
fn live_check_checks_both_blob_layers_and_ordinary_frontmatter() {
    for content in [
        "---\ntype: Decision\nid: D\n---\nslow",
        "---\ntype: FutureType\nlayer: L1\n---\nslow",
        "---\ntype: Task\nlayer: malformed\n---\ninvalid",
    ] {
        let f = Fixture::new();
        f.commit("src/a/object.md", content);
        assert_eq!(
            f.check(),
            Err(GateError::Protected(b"src/a/object.md".to_vec()))
        );
    }
    let f = Fixture::new();
    f.commit(
        "src/a/article.md",
        "---\ntitle: Ordinary markdown\n---\ntext\n",
    );
    assert!(f.check().is_ok());
    let f = Fixture::new();
    f.commit(
        "src/a/protected.md",
        "---\ntype: Decision\nid: D\n---\nslow\n",
    );
    git(&f.root(), &["update-ref", "refs/heads/main", "HEAD"]);
    fs::remove_file(f.root().join("src/a/protected.md")).unwrap();
    git(&f.root(), &["add", "-u"]);
    git(&f.root(), &["commit", "-qm", "try deleting slow object"]);
    assert!(matches!(
        f.check(),
        Err(GateError::MissingContext(_)) | Err(GateError::Protected(_))
    ));
}

#[test]
fn live_check_rejects_missing_authority_context_and_unconfirmed_rechecks() {
    let mut f = Fixture::new();
    f.task
        .extra
        .insert("needs_recheck".into(), serde_json::json!(true));
    assert!(matches!(f.check(), Err(GateError::MissingContext(_))));
    f.task.extra.clear();
    f.task.spec.decisions.push("missing".into());
    assert!(matches!(f.check(), Err(GateError::MissingContext(_))));
    f.task.spec.decisions.clear();
    f.commit(
        "docs/tower/specs/AC.md",
        "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: []\n---\n",
    );
    git(&f.root(), &["update-ref", "refs/heads/main", "HEAD"]);
    assert!(matches!(f.check(), Err(GateError::MissingContext(_))));
}

#[test]
fn shared_check_uses_verified_session_and_rechecks_leases_after_git_io() {
    use std::{cell::Cell, sync::Mutex};
    use towermd_core::project_session::ProjectSession;
    let f = Fixture::new();
    f.commit("src/a/code.rs", "actual captured task commit\n");
    let catalog = Catalog::load(&f.root()).unwrap();
    let inputs = RecoveryInputs::read(&f.temp.path().join("outbox"), &catalog).unwrap();
    let session =
        Mutex::new(ProjectSession::open(&f.temp.path().join("hub"), &catalog, &inputs, 2).unwrap());
    let valid = check_shared(&session, &catalog, &inputs, "host", Some(&f.run.id), || {
        Ok(2)
    })
    .unwrap();
    assert_eq!(valid.commit, git(&f.root(), &["rev-parse", "HEAD"]));
    let calls = Cell::new(0);
    let expired = check_shared(&session, &catalog, &inputs, "host", Some(&f.run.id), || {
        let call = calls.get();
        calls.set(call + 1);
        Ok(if call == 0 {
            2
        } else {
            f.leases["module:B"].expires_at
        })
    });
    assert_eq!(
        calls.get(),
        2,
        "must reach both short Session captures around actual Git checks"
    );
    assert_eq!(expired, Err(GateError::StaleFence));
}

#[test]
fn live_check_rejects_diverged_or_stale_authority() {
    let f = Fixture::new();
    let old = Catalog::load(&f.root()).unwrap();
    f.commit("src/a/code.rs", "task\n");
    let task_tip = git(&f.root(), &["rev-parse", "HEAD"]);
    git(&f.root(), &["switch", "main"]);
    f.commit("src/a/code.rs", "authority advanced\n");
    git(&f.root(), &["switch", "t/T"]);
    assert_eq!(
        check(&old, &f.task, &f.run, "host", &f.leases, 2),
        Err(GateError::NotLatestMain)
    );
    assert_eq!(f.check(), Err(GateError::NotLatestMain));
    assert_eq!(git(&f.root(), &["rev-parse", "HEAD"]), task_tip);
}

#[cfg(unix)]
#[test]
fn live_check_rejects_symlink_and_gitlink_modes_but_keeps_executable_files() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = Fixture::new();
    f.commit("src/a/exec", "#!/bin/sh\nexit 0\n");
    fs::set_permissions(
        f.root().join("src/a/exec"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    git(&f.root(), &["add", "src/a/exec"]);
    git(&f.root(), &["commit", "-qm", "executable"]);
    assert_eq!(f.check().unwrap().changed[0].new_mode, 0o100755);
    symlink("code.rs", f.root().join("src/a/link")).unwrap();
    git(&f.root(), &["add", "src/a/link"]);
    git(&f.root(), &["commit", "-qm", "symlink"]);
    assert_eq!(
        f.check(),
        Err(GateError::UnsupportedMode(b"src/a/link".to_vec()))
    );
    let f = Fixture::new();
    let nested = f.root().join("src/a/submodule");
    fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-qb", "main"]);
    fs::write(nested.join("code"), "actual nested commit").unwrap();
    git(&nested, &["add", "."]);
    git(&nested, &["commit", "-qm", "actual nested repository"]);
    let commit = git(&nested, &["rev-parse", "HEAD"]);
    git(
        &f.root(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000",
            &commit,
            "src/a/submodule",
        ],
    );
    git(&f.root(), &["commit", "-qm", "gitlink"]);
    assert_eq!(
        f.check(),
        Err(GateError::UnsupportedMode(b"src/a/submodule".to_vec()))
    );
}
