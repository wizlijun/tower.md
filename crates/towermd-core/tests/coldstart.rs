use std::{fs, path::Path, process::Command};
use towermd_core::catalog::Catalog;
use towermd_core::coldstart::{prepare_coldstart, ScanOptions};
use towermd_core::config::Config;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
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

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
}

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    write(
        root.path(),
        ".towermd/config.yaml",
        &Config::new("project", "docs/tower").unwrap().to_yaml(),
    );
    root
}

#[test]
fn scan_is_deterministic_and_reads_main_not_dirty_worktree() {
    let root = fixture();
    write(root.path(), "src/alpha/main.rs", "fn main() {}\n");
    write(root.path(), "src/beta/view.ts", "export const view = 1;\n");
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = 'sample'\nversion = '0.1.0'\n",
    );
    commit(root.path());
    git(root.path(), &["switch", "-qc", "task"]);
    write(
        root.path(),
        "src/alpha/untracked.py",
        "raise RuntimeError\n",
    );
    write(root.path(), "Cargo.toml", "malicious dirty content");
    let status = git(root.path(), &["status", "--porcelain"]);
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let catalog = Catalog::load(root.path()).unwrap();
    let scan = prepare_coldstart(&catalog, &ScanOptions::default()).unwrap();
    assert_eq!(
        scan,
        prepare_coldstart(&catalog, &ScanOptions::default()).unwrap()
    );
    assert_eq!(scan["revision"], catalog.revision);
    assert_eq!(scan["coverage"]["source_files"], 2);
    assert_eq!(scan["coverage"]["assigned"], 2);
    assert_eq!(scan["coverage"]["ratio"], 1.0);
    assert_eq!(scan["modules"].as_array().unwrap().len(), 2);
    assert_eq!(scan["stages"]["S5"], "not_run");
    assert!(scan["validation_plans"]
        .as_array()
        .unwrap()
        .iter()
        .all(|plan| plan["status"] == "not_run"));
    assert_eq!(git(root.path(), &["status", "--porcelain"]), status);
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    assert!(!root.path().join("docs/tower/tower.canvas").exists());
    assert!(!catalog.common_dir.join("towermd").exists());
}

#[test]
fn empty_source_is_not_a_fake_full_coverage_and_missing_tests_are_explicit() {
    let root = fixture();
    write(root.path(), "README.md", "hello\n");
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert!(scan["coverage"]["ratio"].is_null());
    assert_eq!(scan["coverage"]["status"], "not_applicable");
    write(root.path(), "src/run.py", "print('hello')\n");
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_eq!(scan["validation_plans"][0]["status"], "no_tests_discovered");
    assert_eq!(scan["candidate_tasks"].as_array().unwrap().len(), 1);
    assert_eq!(scan["candidate_tasks"][0]["status"], "candidate");
}

#[test]
fn repeated_coupling_refines_directory_partition_without_inventing_interfaces() {
    let root = fixture();
    for name in ["alpha", "beta", "gamma", "delta"] {
        write(root.path(), &format!("src/{name}/main.rs"), "initial\n");
    }
    commit(root.path());
    for version in 1..=3 {
        for name in ["alpha", "beta"] {
            write(
                root.path(),
                &format!("src/{name}/main.rs"),
                &format!("change{version}\n"),
            );
        }
        commit(root.path());
    }
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_eq!(scan["modules"].as_array().unwrap().len(), 3);
    assert_eq!(scan["history"]["merges"].as_array().unwrap().len(), 1);
    assert!(scan["candidate_canvas"]["edges"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(scan["coverage"]["assigned"], 4);
}

#[test]
fn candidate_scripts_are_never_executed_or_reported_passed() {
    let root = fixture();
    write(root.path(), "src/main.ts", "export {};\n");
    write(
        root.path(),
        "package.json",
        r#"{"scripts":{"test":"touch SHOULD_NOT_EXIST"},"packageManager":"pnpm@10.0.0"}"#,
    );
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_eq!(scan["validation_plans"][0]["status"], "not_run");
    assert!(!root.path().join("SHOULD_NOT_EXIST").exists());
    assert_eq!(scan["requires_app_approval"], true);
}

#[test]
fn unsupported_or_malformed_manifest_is_not_reported_as_no_tests() {
    for (path, content) in [
        ("pyproject.toml", "[tool.pytest.ini_options]\n"),
        ("package.json", "{broken"),
        (
            "Cargo.toml",
            "[package] # valid TOML comment\nname = 'sample'\nversion = '0.1.0'\n",
        ),
        (
            "Makefile",
            ".PHONY: test check\ntest check:\n\tpython -m pytest\n",
        ),
    ] {
        let root = fixture();
        write(root.path(), "src/run.py", "pass\n");
        write(root.path(), path, content);
        commit(root.path());
        let scan = prepare_coldstart(
            &Catalog::load(root.path()).unwrap(),
            &ScanOptions::default(),
        )
        .unwrap();
        assert_eq!(
            scan["validation_plans"][0]["status"], "unsupported",
            "{scan}"
        );
        assert!(scan["candidate_tasks"].as_array().unwrap().is_empty());
    }
}

#[test]
fn source_symlinks_are_unassigned_and_fast_state_does_not_move_layout() {
    let root = fixture();
    write(root.path(), "src/run.rs", "fn main() {}\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/outside/never-read.rs", root.path().join("src/link.rs")).unwrap();
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    let before = prepare_coldstart(&catalog, &ScanOptions::default()).unwrap();
    #[cfg(unix)]
    assert_eq!(before["unassigned"].as_array().unwrap().len(), 1);
    write(root.path(), "docs/tower/state/runtime.rs", "fast-state\n");
    commit(root.path());
    let after = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_eq!(before["candidate_canvas"], after["candidate_canvas"]);
    assert_eq!(before["coverage"], after["coverage"]);
}

#[test]
fn ci_test_commands_are_reported_as_unexecuted_candidates() {
    let root = fixture();
    write(root.path(), "src/tested.py", "pass\n");
    write(
        root.path(),
        ".github/workflows/ci.yml",
        "jobs:\n  check:\n    steps:\n      - run: python -m pytest\n      - run: echo setup\n",
    );
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_eq!(scan["validation_plans"][0]["status"], "not_run");
    let commands = scan["validation_plans"][0]["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["script"], "python -m pytest");
    assert!(commands[0]["argv"].is_null());
    assert_eq!(commands[0]["requires_confirmation"], true);
}

#[test]
fn equivalent_ci_working_directories_keep_discovered_commands() {
    for cwd in [".", "./", "src", "./src", "src/"] {
        let root = fixture();
        write(root.path(), "src/tested.py", "pass\n");
        write(root.path(), ".github/workflows/ci.yml", &format!("jobs:\n  check:\n    steps:\n      - run: python -m pytest\n        working-directory: {cwd}\n"));
        commit(root.path());
        let scan = prepare_coldstart(
            &Catalog::load(root.path()).unwrap(),
            &ScanOptions::default(),
        )
        .unwrap();
        assert_eq!(
            scan["validation_plans"][0]["status"], "not_run",
            "cwd={cwd}: {scan}"
        );
        let command = &scan["validation_plans"][0]["commands"][0];
        assert_eq!(command["source_cwd"], cwd);
        assert_eq!(
            command["cwd"],
            if cwd.contains("src") { "src" } else { "." }
        );
        assert!(scan["candidate_tasks"].as_array().unwrap().is_empty());
    }
}

#[test]
fn nested_directory_candidates_do_not_claim_each_others_files() {
    let root = fixture();
    write(root.path(), "src/lib.rs", "pub mod foo;\n");
    write(root.path(), "src/foo/mod.rs", "pub fn foo() {}\n");
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    let parent = scan["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|module| module["roots"][0] == "src")
        .unwrap();
    assert_eq!(parent["paths"], serde_json::json!(["src/lib.rs"]));
}

#[test]
fn shallow_history_cannot_change_directory_partition_using_partial_evidence() {
    let root = fixture();
    for name in ["alpha", "beta", "gamma", "delta"] {
        write(root.path(), &format!("src/{name}/main.rs"), "initial\n");
    }
    commit(root.path());
    for version in 0..5 {
        write(
            root.path(),
            "src/alpha/main.rs",
            &format!("only{version}\n"),
        );
        commit(root.path());
    }
    for version in 0..3 {
        for name in ["alpha", "beta"] {
            write(
                root.path(),
                &format!("src/{name}/main.rs"),
                &format!("both{version}\n"),
            );
        }
        commit(root.path());
    }
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &[
            "clone",
            "-q",
            "--depth=3",
            &format!("file://{}", root.path().display()),
            ".",
        ],
    );
    git(clone.path(), &["remote", "remove", "origin"]);
    let scan = prepare_coldstart(
        &Catalog::load(clone.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_eq!(scan["modules"].as_array().unwrap().len(), 4);
    assert_eq!(scan["history"]["status"], "unavailable_shallow");
    assert_eq!(scan["stages"]["S4"], "unavailable");
}
