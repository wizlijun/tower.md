use std::{fs, path::Path, process::Command};
use towermd_core::{
    config::Config,
    path_guard::{self, Rule},
};

fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Guard Test",
            "-c",
            "user.email=guard@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}
fn write(root: &Path, path: &str, value: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, value).unwrap();
}
fn commit(root: &Path, name: &str) -> String {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", name]);
    git(root, &["rev-parse", "HEAD"])
}
fn fixture() -> (tempfile::TempDir, String) {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-qb", "main"]);
    write(temp.path(), "src/app.rs", "fn main() {}\n");
    write(temp.path(), "README.md", "# Ordinary Markdown\n");
    let base = commit(temp.path(), "initial");
    (temp, base)
}
fn enable(root: &Path, message: &str) -> String {
    write(
        root,
        ".towermd/config.yaml",
        &Config::new("guard-project", "docs/tower")
            .unwrap()
            .to_yaml(),
    );
    commit(root, message)
}
fn strings(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| path.to_string()).collect()
}

#[test]
fn fixed_diff_enforces_task_scope_without_rejecting_ordinary_markdown() {
    let (temp, base) = fixture();
    let root = temp.path();
    write(root, "src/app.rs", "fn main() { println!(\"new\"); }\n");
    write(root, "src2/out.rs", "fn outside() {}\n");
    write(
        root,
        "README.md",
        "---\ntitle: Ordinary YAML metadata\n---\n# ordinary prose\n",
    );
    let head = commit(root, "changes");
    let index = fs::read(root.join(".git/index")).unwrap();
    let report = path_guard::check(
        root,
        &base,
        &head,
        "docs/tower",
        &strings(&["src/**", "README.md"]),
    )
    .unwrap();
    assert!(!report.passed);
    assert_eq!(report.changes.len(), 3);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].path, "src2/out.rs");
    assert_eq!(report.violations[0].rule, Rule::OutsideAllowedPaths);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert!(path_guard::check(
        root,
        &base,
        &head,
        "docs/tower",
        &strings(&["src/**", "../escape"])
    )
    .is_err());
    assert!(path_guard::check(root, "main", &head, "docs/tower", &strings(&["src/**"])).is_err());
}

#[test]
fn both_old_and_new_layers_default_types_and_invalid_layers_are_protected() {
    let (temp, _) = fixture();
    let root = temp.path();
    write(
        root,
        "notes/default.md",
        "---\ntype: Decision\n---\nSlow by default.\n",
    );
    write(
        root,
        "notes/changed.md",
        "---\ntype: Task\nlayer: L1\n---\nExplicit slow.\n",
    );
    let base = commit(root, "slow objects outside docs");
    write(root, "notes/default.md", "# Removed all object metadata\n");
    write(
        root,
        "notes/changed.md",
        "---\ntype: Task\nlayer: L4\n---\nDowngraded.\n",
    );
    write(
        root,
        "notes/invalid.md",
        "---\ntype: Task\nlayer: [L4]\n---\nInvalid layer.\n",
    );
    write(root, "notes/new.md", "---\ntype: Module\n---\nNew slow.\n");
    write(
        root,
        "notes/fast.md",
        "---\ntype: Task\nlayer: L4\n---\nFast.\n",
    );
    let head = commit(root, "attempt bypasses");
    let report =
        path_guard::check(root, &base, &head, "docs/tower", &strings(&["notes/**"])).unwrap();
    for path in ["notes/default.md", "notes/changed.md", "notes/new.md"] {
        assert!(
            report
                .violations
                .iter()
                .any(|violation| violation.path == path && violation.rule == Rule::SlowObject),
            "{report:?}"
        );
    }
    assert!(report
        .violations
        .iter()
        .any(|violation| violation.path == "notes/invalid.md"
            && violation.rule == Rule::InvalidObject));
    assert!(!report
        .violations
        .iter()
        .any(|violation| violation.path == "notes/fast.md"));
}

#[test]
fn rename_remains_two_checked_paths_and_all_reserved_directories_are_protected() {
    let (temp, _) = fixture();
    let root = temp.path();
    write(
        root,
        "docs/tower/arch/old.md",
        "# Existing ordinary document in protected folder\n",
    );
    let base = commit(root, "protected input");
    git(root, &["mv", "docs/tower/arch/old.md", "outside.md"]);
    for path in [
        "docs/tower/intents/raw.txt",
        "docs/tower/specs/raw.txt",
        "docs/tower/baselines/raw.txt",
        "docs/tower/state/raw.bin",
        "docs/tower/tower.canvas",
        ".towermd/extra.json",
    ] {
        write(root, path, "changed\n");
    }
    let head = commit(root, "move out and change reserved files");
    let report = path_guard::check(
        root,
        &base,
        &head,
        "docs/tower",
        &strings(&["outside.md", "docs/**", ".towermd/**"]),
    )
    .unwrap();
    assert!(report
        .changes
        .iter()
        .any(|change| change.path == "docs/tower/arch/old.md" && change.status == "D"));
    assert!(report
        .changes
        .iter()
        .any(|change| change.path == "outside.md" && change.status == "A"));
    assert_eq!(
        report
            .violations
            .iter()
            .filter(|violation| violation.rule == Rule::ProtectedPath)
            .count(),
        7
    );
}

#[cfg(unix)]
#[test]
fn symlinks_and_gitlinks_cannot_hide_unauthorized_files() {
    let (temp, base) = fixture();
    let root = temp.path();
    std::os::unix::fs::symlink("../README.md", root.join("src/link")).unwrap();
    git(root, &["add", "src/link"]);
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{base},src/vendor"),
        ],
    );
    git(root, &["commit", "-qm", "special entries"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let report =
        path_guard::check(root, &base, &head, "docs/tower", &strings(&["src/**"])).unwrap();
    assert_eq!(
        report
            .violations
            .iter()
            .filter(|violation| violation.rule == Rule::UnsupportedEntry)
            .count(),
        2
    );
}

#[test]
fn inspection_uses_the_original_import_commit_and_detects_reverted_slow_changes() {
    let (temp, _) = fixture();
    let root = temp.path();
    write(
        root,
        "docs/tower/arch/imported.md",
        "# Included in whole import commit\n",
    );
    let anchor = enable(root, "first enable");
    write(root, "docs/tower/arch/imported.md", "# Unauthorized edit\n");
    let unauthorized = commit(root, "slow edit");
    git(root, &["revert", "--no-edit", &unauthorized]);
    fs::remove_file(root.join(".towermd/config.yaml")).unwrap();
    commit(root, "remove config");
    let tip = enable(root, "re-add same project id");
    git(root, &["tag", "towermd/B0", &unauthorized]);
    let inspection =
        path_guard::inspect_main(root, &tip, "guard-project", "docs/tower", Some(&anchor)).unwrap();
    assert_eq!(inspection.import_anchor, anchor);
    assert_eq!(inspection.commits.len(), 4);
    assert!(inspection
        .commits
        .iter()
        .find(|commit| commit.commit == unauthorized)
        .unwrap()
        .violations
        .iter()
        .any(|violation| violation.rule == Rule::ProtectedPath));
    assert!(inspection
        .commits
        .iter()
        .all(|commit| !commit.violations.is_empty()));
    assert!(!inspection
        .commits
        .iter()
        .any(|commit| commit.commit == anchor));
    assert!(
        path_guard::inspect_main(root, &anchor, "guard-project", "docs/tower", Some(&tip))
            .unwrap_err()
            .starts_with("inspection_incomplete:")
    );
    fs::write(root.join(".git/shallow"), format!("{tip}\n")).unwrap();
    assert!(
        path_guard::inspect_main(root, &tip, "guard-project", "docs/tower", None)
            .unwrap_err()
            .contains("浅历史")
    );
}

#[test]
fn independently_introduced_same_project_ids_are_ambiguous() {
    let (temp, base) = fixture();
    let root = temp.path();
    enable(root, "main enable");
    git(root, &["checkout", "-qb", "other", &base]);
    enable(root, "independent enable");
    git(root, &["checkout", "-q", "main"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "other",
            "-m",
            "merge independent identities",
        ],
    );
    let tip = git(root, &["rev-parse", "HEAD"]);
    assert!(
        path_guard::inspect_main(root, &tip, "guard-project", "docs/tower", None)
            .unwrap_err()
            .contains("不是唯一")
    );
}

#[test]
fn path_rules_live_in_path_guard() {
    use towermd_core::path_guard::{path_is_allowed, path_is_within};
    assert!(path_is_within("src/a/**", "src/**").unwrap());
    assert!(!path_is_within("src/**", "src/a/**").unwrap());
    assert!(path_is_allowed("src/a/b.rs", &["src/a/".into()]).unwrap());
    assert!(!path_is_allowed("src/b.rs", &["src/a/**".into()]).unwrap());
    assert!(path_is_allowed("src/", &["src/**".into()]).is_err());
    assert!(path_is_within("../x", "src/**").is_err());
}

#[test]
fn only_process_configuration_alone_is_a_minor_change() {
    use towermd_core::path_guard::{change_class, process_config, Violation};
    let touched = |paths: &[&str]| -> Vec<Violation> {
        paths.iter().map(|path| Violation { path: (*path).into(), rule: Rule::ProtectedPath, reason: String::new() }).collect()
    };
    for path in [".towermd/gates.yaml", ".towermd/config.yaml", ".towermd/extensions/eng.yaml", ".TowerMD/Gates.yaml",
        ".towermd/skillpacks.yaml", ".towermd/skillpacks/superpowers@1.2.0/skills/tdd/SKILL.md"] {
        assert!(process_config(path), "{path}");
    }
    for path in [".towermd/state/snapshot.json", ".towermd/extensions", ".towermd/gates.yaml.bak", "docs/tower/intents/a.md",
        ".towermd/skillpacks", ".towermd/skillpacks.yaml.bak"] {
        assert!(!process_config(path), "{path}");
    }
    assert_eq!(change_class(&touched(&[".towermd/gates.yaml", ".towermd/extensions/x.yaml"])), "minor");
    assert_eq!(change_class(&touched(&[".towermd/skillpacks.yaml", ".towermd/skillpacks/p@1/skills/a/SKILL.md"])), "minor");
    assert_eq!(change_class(&touched(&[".towermd/gates.yaml", "docs/tower/intents/a.md"])), "major");
    assert_eq!(change_class(&[]), "major");
}
