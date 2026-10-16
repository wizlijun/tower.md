use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use towermd_core::config::CONFIG_PATH;
use towermd_core::init::{init, InitOptions};

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let output = command
        .current_dir(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgSign=false",
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
    _temporary: tempfile::TempDir,
    root: PathBuf,
    remote: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("repo");
        let remote = temporary.path().join("remote.git");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        Self {
            _temporary: temporary,
            root,
            remote,
        }
    }

    fn commit(&self) {
        git(&self.root, &["commit", "--allow-empty", "-qm", "fixture"]);
    }

    fn add_origin(&self) {
        git(
            &self.root,
            &["clone", "--bare", ".", self.remote.to_str().unwrap()],
        );
        git(
            &self.root,
            &["remote", "add", "origin", self.remote.to_str().unwrap()],
        );
    }

    fn assert_uninitialized(&self) {
        assert!(!self.root.join(".towermd").exists());
        assert!(!self.root.join("docs").exists());
    }
}

#[test]
fn no_origin_preserves_unborn_repository_behavior() {
    let fixture = Fixture::new();
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(report.warnings.is_empty());
    assert!(fixture.root.join(CONFIG_PATH).exists());
    assert!(!fixture.root.join(".git/index").exists());
}

#[test]
fn init_report_defaults_missing_warnings_to_empty() {
    let report: towermd_core::init::InitReport = serde_json::from_value(serde_json::json!({
        "root": "/fixture", "project_id": "fixture", "docs_dir": "docs/tower",
        "reused_config": false, "created": [], "hints": []
    }))
    .unwrap();
    assert!(report.warnings.is_empty());
    assert!(serde_json::to_value(report)
        .unwrap()
        .get("push_preflight")
        .is_none());
}

#[test]
fn origin_requires_local_main_before_creating_files() {
    for committed in [false, true] {
        let fixture = Fixture::new();
        if committed {
            fixture.commit();
            git(&fixture.root, &["branch", "-m", "feature"]);
        }
        fixture.add_origin();
        let error = init(&fixture.root, &InitOptions::default()).unwrap_err();
        assert!(error.contains("main"), "{error}");
        fixture.assert_uninitialized();
    }
}

#[test]
fn unreachable_origin_fails_without_initializing() {
    let fixture = Fixture::new();
    fixture.commit();
    git(
        &fixture.root,
        &["remote", "add", "origin", fixture.remote.to_str().unwrap()],
    );
    let error = init(&fixture.root, &InitOptions::default()).unwrap_err();
    assert!(
        error.contains("dry-run") && error.contains("origin"),
        "{error}"
    );
    fixture.assert_uninitialized();
}

#[test]
fn dry_run_negotiates_without_publishing_local_commits_or_changing_git_state() {
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    fixture.commit();
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    fs::write(fixture.root.join("human.txt"), "staged").unwrap();
    git(&fixture.root, &["add", "human.txt"]);
    fs::write(fixture.root.join("human.txt"), "dirty").unwrap();
    let local_refs = git(&fixture.root, &["show-ref"]);
    let remote_refs = git(&fixture.remote, &["show-ref"]);
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    let diagnostic = report
        .warnings
        .iter()
        .find(|warning| warning.contains("dry-run"))
        .unwrap();
    assert!(diagnostic.contains("dry-run"), "{diagnostic}");
    assert!(
        diagnostic.contains("不保证") && diagnostic.contains("服务端"),
        "{diagnostic}"
    );
    assert_eq!(git(&fixture.root, &["show-ref"]), local_refs);
    assert_eq!(git(&fixture.remote, &["show-ref"]), remote_refs);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(fixture.root.join("human.txt")).unwrap(),
        "dirty"
    );
    assert!(!fixture.root.join(".git/FETCH_HEAD").exists());
}

#[test]
fn remote_non_fast_forward_rejection_precedes_all_init_writes() {
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    let tree = git(&fixture.remote, &["rev-parse", "main^{tree}"]);
    let remote_commit = git(
        &fixture.remote,
        &["commit-tree", &tree, "-p", "main", "-m", "remote change"],
    );
    git(
        &fixture.remote,
        &["update-ref", "refs/heads/main", &remote_commit],
    );
    fixture.commit();
    let error = init(&fixture.root, &InitOptions::default()).unwrap_err();
    assert!(error.contains("dry-run"), "{error}");
    fixture.assert_uninitialized();
    assert_eq!(git(&fixture.remote, &["rev-parse", "main"]), remote_commit);
}

#[test]
fn empty_origin_is_only_negotiated_and_never_initialized_by_push() {
    let fixture = Fixture::new();
    fixture.commit();
    git(
        &fixture.root,
        &["init", "-q", "--bare", fixture.remote.to_str().unwrap()],
    );
    git(
        &fixture.root,
        &["remote", "add", "origin", fixture.remote.to_str().unwrap()],
    );
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("dry-run")));
    assert!(git(&fixture.remote, &["for-each-ref"]).is_empty());
}

#[test]
fn failed_reinit_preserves_existing_config_and_does_not_fill_missing_docs() {
    let fixture = Fixture::new();
    fixture.commit();
    fs::create_dir(fixture.root.join(".towermd")).unwrap();
    let config = "# preserve\nproject_id: p1\ndocs_dir: docs/tower\ncustom: true\n";
    fs::write(fixture.root.join(CONFIG_PATH), config).unwrap();
    git(
        &fixture.root,
        &["remote", "add", "origin", fixture.remote.to_str().unwrap()],
    );
    assert!(init(&fixture.root, &InitOptions::default()).is_err());
    assert_eq!(
        fs::read_to_string(fixture.root.join(CONFIG_PATH)).unwrap(),
        config
    );
    assert!(!fixture.root.join("docs").exists());
    assert!(!fixture.root.join(".towermd/AGENTS.md").exists());
}

#[test]
fn explicit_main_refspec_ignores_default_push_and_mirror_configuration() {
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    let tree = git(&fixture.remote, &["rev-parse", "main^{tree}"]);
    let next = git(
        &fixture.remote,
        &["commit-tree", &tree, "-p", "main", "-m", "remote advanced"],
    );
    git(&fixture.remote, &["update-ref", "refs/heads/main", &next]);
    git(&fixture.root, &["checkout", "-qb", "feature"]);
    git(
        &fixture.root,
        &["config", "remote.origin.push", "feature:refs/heads/feature"],
    );
    git(&fixture.root, &["config", "remote.origin.mirror", "true"]);
    let refs = git(&fixture.remote, &["show-ref"]);
    assert!(init(&fixture.root, &InitOptions::default()).is_err());
    fixture.assert_uninitialized();
    assert_eq!(git(&fixture.remote, &["show-ref"]), refs);
}

#[test]
fn preflight_uses_origin_push_url() {
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    let unavailable = fixture.root.parent().unwrap().join("no-push-access.git");
    git(
        &fixture.root,
        &[
            "config",
            "remote.origin.pushurl",
            unavailable.to_str().unwrap(),
        ],
    );
    assert!(init(&fixture.root, &InitOptions::default()).is_err());
    fixture.assert_uninitialized();
}

#[cfg(unix)]
fn executable(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn hooks_are_not_executed_and_server_policy_is_not_claimed_as_verified() {
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    fixture.commit();
    executable(
        &fixture.root.join(".git/hooks/pre-push"),
        "#!/bin/sh\necho ran > local-hook-ran\nexit 1\n",
    );
    executable(
        &fixture.remote.join("hooks/pre-receive"),
        "#!/bin/sh\necho ran > remote-hook-ran\nexit 1\n",
    );
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    let diagnostic = report
        .warnings
        .iter()
        .find(|warning| warning.contains("dry-run"))
        .unwrap();
    assert!(
        diagnostic.contains("不保证") && diagnostic.contains("hooks"),
        "{diagnostic}"
    );
    assert!(!fixture.root.join("local-hook-ran").exists());
    assert!(!fixture.remote.join("remote-hook-ran").exists());
}

#[cfg(unix)]
#[test]
fn push_negotiation_has_a_finite_timeout_and_leaves_init_unwritten() {
    use std::time::{Duration, Instant};
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    let receiver = fixture.root.parent().unwrap().join("sleeping-receive-pack");
    executable(&receiver, "#!/bin/sh\nsleep 60\n");
    git(
        &fixture.root,
        &[
            "config",
            "remote.origin.receivepack",
            receiver.to_str().unwrap(),
        ],
    );
    let started = Instant::now();
    let error = init(&fixture.root, &InitOptions::default()).unwrap_err();
    assert!(
        error.contains("超时") && error.contains("dry-run"),
        "{error}"
    );
    assert!(started.elapsed() < Duration::from_secs(40));
    fixture.assert_uninitialized();
}
