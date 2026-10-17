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
        assert_eq!(scan["candidate_tasks"].as_array().unwrap().len(), 1);
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
        assert_eq!(scan["candidate_tasks"].as_array().unwrap().len(), 1);
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

fn plan_for_root<'a>(scan: &'a serde_json::Value, root: &str) -> &'a serde_json::Value {
    let module = scan["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|module| {
            module["roots"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r == root)
        })
        .unwrap();
    scan["validation_plans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plan| plan["module"] == module["id"])
        .unwrap()
}

#[test]
fn own_discoveries_exclude_root_fallback_even_when_not_executable() {
    let root = fixture();
    write(root.path(), "Cargo.toml", "[workspace]\nmembers = []\n");
    write(root.path(), "crates/core/lib.rs", "pub fn core() {}\n");
    write(
        root.path(),
        "crates/core/Cargo.toml",
        "[package]\nname='core'\nversion='0.1.0'\n",
    );
    write(
        root.path(),
        "packages/ui/index.js",
        "export const ui = 1;\n",
    );
    write(
        root.path(),
        "packages/ui/package.json",
        r#"{"packageManager":"pnpm@11.7.0","scripts":{"test":"true"}}"#,
    );
    write(root.path(), "tools/check.sh", "true\n");
    write(root.path(), "src/a/test.py", "pass\n");
    write(
        root.path(),
        "src/a/pyproject.toml",
        "[tool.pytest.ini_options]\n",
    );
    write(root.path(), "src/a2/test.py", "pass\n");
    write(root.path(), "packages/pending/index.js", "export {};\n");
    write(
        root.path(),
        "packages/pending/package.json",
        r#"{"scripts":{"test":"touch NEVER"}}"#,
    );
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    for (directory, cwd, shared) in [
        ("crates/core", "crates/core", false),
        ("packages/ui", "packages/ui", false),
        ("tools", ".", true),
        ("src/a2", ".", true),
        ("src/a", "src/a", false),
        ("packages/pending", "packages/pending", false),
    ] {
        let plan = plan_for_root(&scan, directory);
        assert_eq!(
            plan["commands"].as_array().unwrap().len(),
            1,
            "{directory}: {plan}"
        );
        assert_eq!(plan["commands"][0]["cwd"], cwd, "{directory}");
        assert_eq!(plan["shared"], shared, "{directory}");
    }
    assert!(plan_for_root(&scan, "src/a")["commands"][0]["argv"].is_null());
    assert!(plan_for_root(&scan, "packages/pending")["commands"][0]["argv"].is_null());
    assert_eq!(scan["candidate_tasks"].as_array().unwrap().len(), 2);
    assert!(!root.path().join("NEVER").exists());
}

#[test]
fn shared_and_equivalent_manifest_ci_commands_have_one_execution_per_cwd_argv() {
    let root = fixture();
    write(root.path(), "Cargo.toml", "[workspace]\nmembers = []\n");
    for path in ["one/check.rs", "two/check.rs", "crates/core/lib.rs"] {
        write(root.path(), path, "// source\n");
    }
    write(
        root.path(),
        "crates/core/Cargo.toml",
        "[package]\nname='core'\nversion='0.1.0'\n",
    );
    write(root.path(), ".github/workflows/ci.yml", "jobs:\n  tests:\n    steps:\n      - run: cargo test\n      - run: cargo test\n        working-directory: crates/core\n      - run: cargo test && touch NEVER\n        working-directory: crates/core\n");
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    let own = plan_for_root(&scan, "crates/core");
    assert_eq!(own["commands"].as_array().unwrap().len(), 2);
    let runnable = own["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["argv"].is_array())
        .unwrap();
    assert_eq!(
        runnable["sources"],
        serde_json::json!([".github/workflows/ci.yml", "crates/core/Cargo.toml"])
    );
    assert_eq!(
        scan["verification_commands"],
        serde_json::json!([
            {"cwd":".","argv":["cargo","test"]},
            {"cwd":"crates/core","argv":["cargo","test"]}
        ])
    );
    assert_eq!(plan_for_root(&scan, "one")["shared"], true);
    assert_eq!(plan_for_root(&scan, "two")["shared"], true);
    assert!(!root.path().join("NEVER").exists());
}

#[test]
fn ci_shell_syntax_and_multiple_commands_never_become_executable_argv() {
    let root = fixture();
    write(root.path(), "src/check.rs", "// source\n");
    let scripts = [
        "cargo\ntest",
        "cargo test && touch NEVER",
        "cargo test; touch NEVER",
        "cargo test $OPTIONS",
        "env cargo test",
    ];
    let steps: Vec<_> = scripts
        .iter()
        .map(|script| serde_json::json!({"run":script}))
        .collect();
    write(
        root.path(),
        ".github/workflows/ci.yml",
        &serde_json::json!({"jobs":{"check":{"steps":steps}}}).to_string(),
    );
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert!(scan["verification_commands"].as_array().unwrap().is_empty());
    let commands = scan["validation_plans"][0]["commands"].as_array().unwrap();
    assert!(commands
        .iter()
        .any(|command| command["script"] == scripts[0]));
    assert!(commands
        .iter()
        .all(|command| command["argv"].is_null() && command["requires_confirmation"] == true));
    assert_eq!(scan["candidate_tasks"].as_array().unwrap().len(), 1);
}

#[test]
fn stable_readable_slugs_resolve_collisions_without_group_identity() {
    let root = fixture();
    for directory in [
        "crates/core",
        "packages/ui",
        "foo-bar",
        "foo_bar",
        "foo-bar-1",
    ] {
        write(root.path(), &format!("{directory}/main.rs"), "// source\n");
    }
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    let scan = prepare_coldstart(&catalog, &ScanOptions::default()).unwrap();
    assert_eq!(
        scan,
        prepare_coldstart(&catalog, &ScanOptions::default()).unwrap()
    );
    let ids: Vec<_> = scan["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|module| module["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"A-crates-core"));
    assert!(ids.contains(&"A-packages-ui"));
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        5
    );
    assert!(ids.iter().all(|id| id.len() < 40 && id.starts_with("A-")));
    assert!(ids.contains(&"A-foo-bar-1"));
    assert!(ids.iter().filter(|id| id.starts_with("A-foo-bar-")).count() == 3);
}

fn assert_hierarchy(scan: &serde_json::Value, count: usize) {
    use std::collections::{BTreeMap, BTreeSet};
    let hierarchy = &scan["hierarchy"];
    let groups: BTreeMap<_, _> = hierarchy["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| (group["id"].as_str().unwrap(), group))
        .collect();
    let modules: BTreeSet<_> = scan["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|module| module["id"].as_str().unwrap())
        .collect();
    let mut pending = vec![hierarchy["children"].as_array().unwrap()];
    let mut seen = BTreeSet::new();
    let mut seen_groups = BTreeSet::new();
    while let Some(children) = pending.pop() {
        assert!(
            (3..=9).contains(&children.len()),
            "fanout {} for {count}",
            children.len()
        );
        for child in children {
            let id = child.as_str().unwrap();
            if modules.contains(id) {
                assert!(seen.insert(id), "duplicate module {id}");
            } else {
                assert!(seen_groups.insert(id), "duplicate group {id}");
                pending.push(groups[id]["children"].as_array().unwrap());
            }
        }
    }
    assert_eq!(seen, modules);
    assert_eq!(seen.len(), count);
    assert_eq!(seen_groups.len(), groups.len());
    let canvas = scan["candidate_canvas"]["nodes"].as_array().unwrap();
    for group in groups.values() {
        let parent = canvas
            .iter()
            .find(|node| node["id"] == group["id"])
            .unwrap();
        assert_eq!(parent["type"], "group");
        for child in group["children"].as_array().unwrap() {
            let child = canvas.iter().find(|node| node["id"] == *child).unwrap();
            let n = |value: &serde_json::Value, key: &str| value[key].as_u64().unwrap();
            assert!(n(child, "x") >= n(parent, "x") && n(child, "y") >= n(parent, "y"));
            assert!(n(child, "x") + n(child, "width") <= n(parent, "x") + n(parent, "width"));
            assert!(n(child, "y") + n(child, "height") <= n(parent, "y") + n(parent, "height"));
        }
    }
    let assigned: usize = scan["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|module| module["members"].as_array().unwrap().len())
        .sum();
    let source_files = scan["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|file| !file["language"].is_null() && file["excluded"] == false)
        .count();
    assert_eq!(scan["coverage"]["source_files"], source_files);
    assert_eq!(scan["coverage"]["assigned"], assigned);
}

#[test]
fn wide_hierarchy_has_balanced_three_to_nine_children_at_every_level() {
    for count in [10, 18, 25, 81, 82, 730] {
        let root = fixture();
        for index in 0..count {
            write(root.path(), &format!("m{index:04}/main.rs"), "// source\n");
        }
        commit(root.path());
        let scan = prepare_coldstart(
            &Catalog::load(root.path()).unwrap(),
            &ScanOptions::default(),
        )
        .unwrap();
        assert_hierarchy(&scan, count);
        let groups = scan["hierarchy"]["groups"].as_array().unwrap();
        assert!(scan["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "virtual_module_group"));
        if count == 25 {
            let mut sizes: Vec<_> = groups
                .iter()
                .map(|group| group["children"].as_array().unwrap().len())
                .collect();
            sizes.sort();
            assert_eq!(sizes, vec![8, 8, 9]);
        }
        if count == 82 {
            let roots = scan["hierarchy"]["children"].as_array().unwrap();
            let mut sizes: Vec<_> = roots
                .iter()
                .map(|id| {
                    groups.iter().find(|g| g["id"] == *id).unwrap()["children"]
                        .as_array()
                        .unwrap()
                        .len()
                })
                .collect();
            sizes.sort();
            assert_eq!(sizes, vec![3, 3, 4]);
        }
        assert!(scan["verification_commands"].as_array().unwrap().is_empty());
    }
}

#[test]
fn small_partitions_drill_down_or_diagnose_actual_unachievable_fanout() {
    let root = fixture();
    for directory in ["code/alpha", "code/beta", "code/gamma"] {
        write(root.path(), &format!("{directory}/main.rs"), "// source\n");
    }
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_eq!(scan["modules"].as_array().unwrap().len(), 3);
    assert_hierarchy(&scan, 3);
    for count in [0, 1, 2] {
        let root = fixture();
        for index in 0..count {
            write(root.path(), &format!("m{index}/main.rs"), "// source\n");
        }
        commit(root.path());
        let scan = prepare_coldstart(
            &Catalog::load(root.path()).unwrap(),
            &ScanOptions::default(),
        )
        .unwrap();
        assert_eq!(scan["modules"].as_array().unwrap().len(), count);
        let expected = if count == 0 {
            "no_source_files"
        } else {
            "module_fanout_unachievable"
        };
        assert!(scan["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == expected));
    }
}

#[test]
fn semantic_parents_group_modules_without_counting_groups_as_sources() {
    let root = fixture();
    for parent in ["crates", "packages", "apps"] {
        for child in ["alpha", "beta", "gamma"] {
            write(
                root.path(),
                &format!("{parent}/{child}/main.rs"),
                "// source\n",
            );
        }
    }
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_hierarchy(&scan, 9);
    let groups = scan["hierarchy"]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 3);
    assert!(groups.iter().all(|group| group["virtual"] == false));
    assert_eq!(scan["hierarchy"]["children"].as_array().unwrap().len(), 3);
}

#[test]
fn directory_normalization_collisions_and_drilled_paths_are_stable() {
    let root = fixture();
    for directory in ["foo-bar", "foo/bar"] {
        write(root.path(), &format!("{directory}/main.rs"), "// source\n");
    }
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    let scan = prepare_coldstart(&catalog, &ScanOptions::default()).unwrap();
    assert_eq!(
        scan,
        prepare_coldstart(&catalog, &ScanOptions::default()).unwrap()
    );
    assert_eq!(scan["modules"][0]["id"], "A-foo-bar-1");
    assert_eq!(scan["modules"][1]["id"], "A-foo-bar-2");
    assert_eq!(
        scan["modules"][0]["paths"],
        serde_json::json!(["foo-bar/**"])
    );
    assert_eq!(
        scan["modules"][1]["paths"],
        serde_json::json!(["foo/bar/**"])
    );
}

#[test]
fn history_refinement_rebuilds_wide_hierarchy_and_command_ownership() {
    let root = fixture();
    for index in 0..25 {
        write(root.path(), &format!("m{index:04}/main.rs"), "// initial\n");
    }
    write(
        root.path(),
        "m0000/Cargo.toml",
        "[package]\nname='m0'\nversion='0.1.0'\n",
    );
    commit(root.path());
    for version in 0..3 {
        for index in 0..2 {
            write(
                root.path(),
                &format!("m{index:04}/main.rs"),
                &format!("// together {version}\n"),
            );
        }
        commit(root.path());
    }
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert_eq!(scan["history"]["merges"].as_array().unwrap().len(), 1);
    assert_hierarchy(&scan, 24);
    assert_eq!(scan["coverage"]["source_files"], 25);
    assert_eq!(scan["coverage"]["assigned"], 25);
    let plan = plan_for_root(&scan, "m0000");
    assert_eq!(plan["commands"].as_array().unwrap().len(), 1);
    assert_eq!(plan["commands"][0]["cwd"], "m0000");
    assert_eq!(plan["shared"], false);
    let canvas =
        towermd_core::canvas::CanvasDocument::from_value(scan["candidate_canvas"].clone()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&canvas.encode()).unwrap(),
        scan["candidate_canvas"]
    );
}

#[test]
fn unparsed_ci_shell_test_is_own_discovery_and_blocks_root_fallback() {
    let root = fixture();
    write(root.path(), "Cargo.toml", "[workspace]\nmembers = []\n");
    for child in ["a", "b", "c"] {
        write(root.path(), &format!("src/{child}/main.rs"), "// source\n");
    }
    write(root.path(),".github/workflows/ci.yml","jobs:\n  test:\n    steps:\n      - run: cargo test; touch NEVER\n        working-directory: src/a\n");
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    let plan = plan_for_root(&scan, "src/a");
    assert_eq!(plan["shared"], false);
    assert_eq!(plan["commands"].as_array().unwrap().len(), 1);
    assert_eq!(plan["commands"][0]["cwd"], "src/a");
    assert!(plan["commands"][0]["argv"].is_null());
    assert_eq!(plan["commands"][0]["requires_confirmation"], true);
}

#[test]
fn spec_probe_non_ascii_directory_ids_remain_readable() {
    let root = fixture();
    for directory in ["核心", "界面", "工具"] {
        write(root.path(), &format!("{directory}/main.rs"), "// source\n");
    }
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    println!("NON_ASCII_MODULES={}", scan["modules"]);
    assert!(
        scan["modules"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| !m["id"].as_str().unwrap().starts_with("A-root")),
        "Non-root directory identity was discarded from readable slug"
    );
    for module in scan["modules"].as_array().unwrap() {
        assert_eq!(
            module["id"],
            format!("A-{}", module["roots"][0].as_str().unwrap())
        );
    }
}

#[test]
fn spec_probe_manifest_in_module_without_colocated_source_is_owned() {
    let root = fixture();
    write(root.path(), "Cargo.toml", "[workspace]\nmembers = []\n");
    write(root.path(), "engine/main.js", "export const x = 1;\n");
    write(root.path(), "tools/check.sh", "true\n");
    write(root.path(), "worker/main.rs", "// source\n");
    write(
        root.path(),
        "engine/qa/package.json",
        r#"{"packageManager":"pnpm@11.7.0","scripts":{"test":"node ../main.js"}}"#,
    );
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    let plan = plan_for_root(&scan, "engine");
    println!(
        "MANIFEST_MODULES={}\nMANIFEST_PLAN={}",
        scan["modules"], plan
    );
    assert_eq!(
        plan["shared"], false,
        "engine/** contains engine/qa manifest; root cargo must not replace its own pnpm command"
    );
    assert_eq!(plan["commands"][0]["cwd"], "engine/qa");
}

#[test]
fn non_root_ancestor_manifest_is_owned_by_its_module_not_descendant_modules() {
    let root = fixture();
    write(root.path(), "Cargo.toml", "[workspace]\nmembers=[]\n");
    write(
        root.path(),
        "src/Cargo.toml",
        "[package]\nname='parent'\nversion='0.1.0'\n",
    );
    for path in ["src/lib.rs", "src/a/main.rs", "src/b/main.rs"] {
        write(root.path(), path, "// source\n");
    }
    commit(root.path());
    let scan = prepare_coldstart(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    let parent = plan_for_root(&scan, "src");
    assert_eq!(parent["shared"], false);
    assert_eq!(parent["commands"][0]["cwd"], "src");
    for path in ["src/a", "src/b"] {
        let child = plan_for_root(&scan, path);
        assert_eq!(child["shared"], true, "{child}");
        assert_eq!(child["commands"][0]["cwd"], ".");
    }
}
