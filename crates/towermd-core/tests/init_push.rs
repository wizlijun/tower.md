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
fn unreachable_origin_warns_but_initializes_without_local_main() {
    let fixture = Fixture::new();
    git(
        &fixture.root,
        &["remote", "add", "origin", fixture.remote.to_str().unwrap()],
    );
    let report = init(
        &fixture.root,
        &InitOptions {
            docs_dir: Some("docs/tower".into()),
        },
    )
    .unwrap();
    assert!(fixture.root.join(CONFIG_PATH).is_file());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("fetch") && warning.contains("快照写入时会再次检查")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("本地 main")));
    assert!(!fixture.root.join(".git/index").exists());
    assert!(!fixture.remote.exists());
}

#[test]
fn divergent_local_main_negotiates_remote_sha_without_publishing() {
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    let tree = git(&fixture.remote, &["rev-parse", "main^{tree}"]);
    let next = git(
        &fixture.remote,
        &["commit-tree", &tree, "-p", "main", "-m", "remote advance"],
    );
    git(&fixture.remote, &["update-ref", "refs/heads/main", &next]);
    fixture.commit();
    let local_main = git(&fixture.root, &["rev-parse", "main"]);
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("dry-run") && warning.contains("不保证")));
    assert_eq!(
        git(&fixture.root, &["rev-parse", "refs/remotes/origin/main"]),
        next
    );
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), local_main);
    assert_eq!(git(&fixture.remote, &["rev-parse", "main"]), next);
    assert!(!fixture.root.join(".git/FETCH_HEAD").exists());
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
    let local_refs = git(&fixture.root, &["for-each-ref", "refs/heads/"]);
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
    assert_eq!(
        git(&fixture.root, &["for-each-ref", "refs/heads/"]),
        local_refs
    );
    assert_eq!(git(&fixture.remote, &["show-ref"]), remote_refs);
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(fixture.root.join("human.txt")).unwrap(),
        "dirty"
    );
    assert!(!fixture.root.join(".git/FETCH_HEAD").exists());
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
        .any(|warning| warning.contains("fetch") && warning.contains("再次检查")));
    assert!(git(&fixture.remote, &["for-each-ref"]).is_empty());
}

#[test]
fn offline_reinit_preserves_config_and_fills_missing_docs() {
    let fixture = Fixture::new();
    fixture.commit();
    fs::create_dir(fixture.root.join(".towermd")).unwrap();
    let config = "# preserve\nproject_id: p1\ndocs_dir: docs/tower\ncustom: true\n";
    fs::write(fixture.root.join(CONFIG_PATH), config).unwrap();
    git(
        &fixture.root,
        &["remote", "add", "origin", fixture.remote.to_str().unwrap()],
    );
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(!report.warnings.is_empty());
    assert_eq!(
        fs::read_to_string(fixture.root.join(CONFIG_PATH)).unwrap(),
        config
    );
    assert!(fixture.root.join("docs/tower/arch").exists());
    assert!(fixture.root.join(".towermd/AGENTS.md").exists());
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
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(!report.warnings.is_empty());
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
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(!report.warnings.is_empty());
    assert!(report.warnings.iter().any(|w| w.contains("dry-run")));
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
fn push_negotiation_timeout_warns_and_still_initializes() {
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
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("超时") && warning.contains("dry-run")));
    assert!(started.elapsed() < Duration::from_secs(40));
    assert!(fixture.root.join(CONFIG_PATH).exists());
}

#[cfg(unix)]
#[test]
fn explicit_policy_rejection_is_fatal_but_authentication_failure_is_only_warning() {
    for (diagnostic, fatal) in [
        ("remote: error: GH006: Protected branch update failed", true),
        ("fatal: Authentication failed", false),
        ("Permission denied (publickey)", false),
        ("fatal: repository access unavailable", false),
    ] {
        let fixture = Fixture::new();
        fixture.commit();
        fixture.add_origin();
        let receiver = fixture.root.parent().unwrap().join("reject-receive-pack");
        executable(
            &receiver,
            &format!("#!/bin/sh\nprintf '%s\\n' '{}' >&2\nexit 1\n", diagnostic),
        );
        git(
            &fixture.root,
            &[
                "config",
                "remote.origin.receivepack",
                receiver.to_str().unwrap(),
            ],
        );
        let index = fs::read(fixture.root.join(".git/index")).ok();
        let before = git(&fixture.root, &["rev-parse", "main"]);
        let remote_before = git(&fixture.remote, &["show-ref"]);
        let result = init(
            &fixture.root,
            &InitOptions {
                docs_dir: Some("docs/tower".into()),
            },
        );
        if fatal {
            assert!(result.unwrap_err().contains("明确拒绝推送策略"));
            fixture.assert_uninitialized();
        } else {
            let report = result.unwrap();
            assert!(report
                .warnings
                .iter()
                .any(|warning| warning.contains("快照写入时会再次检查")));
            assert!(fixture.root.join(CONFIG_PATH).is_file());
        }
        assert_eq!(git(&fixture.root, &["rev-parse", "main"]), before);
        assert_eq!(git(&fixture.remote, &["show-ref"]), remote_before);
        assert_eq!(fs::read(fixture.root.join(".git/index")).ok(), index);
    }
}

#[test]
fn missing_local_main_warns_and_negotiates_remote_main_without_creating_heads() {
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    git(&fixture.root, &["branch", "-m", "feature"]);
    let heads = git(&fixture.root, &["for-each-ref", "refs/heads/"]);
    let index = fs::read(fixture.root.join(".git/index")).ok();
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("本地 main")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("dry-run")));
    assert_eq!(git(&fixture.root, &["for-each-ref", "refs/heads/"]), heads);
    assert_eq!(fs::read(fixture.root.join(".git/index")).ok(), index);
    assert_eq!(
        git(&fixture.root, &["rev-parse", "refs/remotes/origin/main"]),
        git(&fixture.remote, &["rev-parse", "main"])
    );
    assert!(!fixture.root.join(".git/towermd").exists());
}
