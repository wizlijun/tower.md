use std::fs;
use std::path::Path;
use std::process::Command;

use towermd_core::catalog::{validate_paths, validate_protected_paths, Catalog};
use towermd_core::config::{Config, CONFIG_PATH};

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
        .args([
            "-c",
            "user.name=Catalog Test",
            "-c",
            "user.email=catalog@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = git_command(root).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .to_string()
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn commit(root: &Path) {
    git(root, &["add", "--all"]);
    git(root, &["commit", "-qm", "fixture"]);
}

fn repository() -> tempfile::TempDir {
    let temporary = tempfile::tempdir().unwrap();
    git(temporary.path(), &["init", "-q", "-b", "main"]);
    write(
        temporary.path(),
        CONFIG_PATH,
        &Config::new("project", "docs/tower").unwrap().to_yaml(),
    );
    write(
        temporary.path(),
        "docs/tower/arch/module.md",
        "---\ntype: Module\nid: module\npaths: ['src/**']\n---\nOriginal.\n",
    );
    commit(temporary.path());
    temporary
}

#[test]
fn ordinary_frontmatter_is_not_mistaken_for_a_tower_object() {
    let temporary = repository();
    for path in ["website/post.md", "docs/tower/readme.md"] {
        write(
            temporary.path(),
            path,
            "---\ntitle: Existing article\ndate: 2026-10-16\n---\nBody\n",
        );
    }
    commit(temporary.path());
    let catalog = Catalog::load(temporary.path()).unwrap();
    assert_eq!(catalog.objects.len(), 1);
    assert_eq!(catalog.protected_paths, ["docs/tower/arch/module.md"]);
}

#[test]
fn fixed_protected_non_object_paths_are_not_parsed_as_objects() {
    let temporary = repository();
    for path in [".towermd/AGENTS.md", "docs/tower/state/import.md"] {
        write(
            temporary.path(),
            path,
            "---\nunstructured state or agent instructions\n",
        );
    }
    commit(temporary.path());
    assert!(Catalog::load(temporary.path()).is_ok());
}

#[test]
fn ignores_worktree_and_index_tampering() {
    let temporary = repository();
    let root = temporary.path();
    let revision = git(root, &["rev-parse", "main"]);
    git(root, &["checkout", "-qb", "t/task"]);
    write(root, CONFIG_PATH, "invalid: [");
    write(root, "docs/tower/arch/module.md", "invalid");
    git(root, &["add", "--all"]);
    let before_index = fs::read(root.join(".git/index")).unwrap();
    let catalog = Catalog::load(root).unwrap();
    assert_eq!(catalog.root, root.canonicalize().unwrap());
    assert_eq!(catalog.branch, "t/task");
    assert_eq!(catalog.revision, revision);
    assert_eq!(catalog.config.project_id(), "project");
    assert_eq!(catalog.objects["module"].body(), "Original.\n");
    assert_eq!(before_index, fs::read(root.join(".git/index")).unwrap());
    assert_eq!(git(root, &["rev-parse", "HEAD"]), revision);
}

#[test]
fn linked_worktrees_share_canonical_common_directory() {
    let temporary = repository();
    let invalid =
        "---\ntype: FutureThing\nlayer: null\nfuture: {retain: [one, 0.5]}\n---\nAuthoritative.\n";
    write(temporary.path(), "outside/invalid.md", invalid);
    commit(temporary.path());
    let worktrees = tempfile::tempdir().unwrap();
    let linked = worktrees.path().join("linked space \n");
    git(
        temporary.path(),
        &[
            "worktree",
            "add",
            "-qb",
            "t/linked",
            linked.to_str().unwrap(),
        ],
    );
    write(
        &linked,
        "outside/invalid.md",
        "---\ntype: Task\nlayer: L4\n---\nTampered.\n",
    );
    git(&linked, &["add", "outside/invalid.md"]);
    let index = git(&linked, &["rev-parse", "--git-path", "index"]);
    let before = (
        git(&linked, &["rev-parse", "HEAD"]),
        git(&linked, &["status", "--porcelain"]),
        fs::read(&index).unwrap(),
    );
    let original = Catalog::load(temporary.path()).unwrap();
    let catalog = Catalog::load(&linked).unwrap();
    assert_eq!(catalog.revision, original.revision);
    assert_eq!(catalog.protected_paths, original.protected_paths);
    assert_eq!(
        serde_json::to_value(&catalog.diagnostics).unwrap(),
        serde_json::to_value(&original.diagnostics).unwrap()
    );
    assert_eq!(
        catalog.read_file("outside/invalid.md").unwrap().unwrap(),
        invalid
    );
    assert_eq!(
        before,
        (
            git(&linked, &["rev-parse", "HEAD"]),
            git(&linked, &["status", "--porcelain"]),
            fs::read(&index).unwrap(),
        )
    );
    assert_eq!(catalog.root, linked.canonicalize().unwrap());
    assert_eq!(catalog.common_dir, original.common_dir);
    assert_eq!(
        catalog.common_dir,
        temporary.path().join(".git").canonicalize().unwrap()
    );
    assert_eq!(catalog.branch, "t/linked");
}

#[test]
fn detached_reads_main_but_uncommitted_init_is_rejected() {
    let temporary = repository();
    git(temporary.path(), &["checkout", "--detach", "-q"]);
    assert_eq!(Catalog::load(temporary.path()).unwrap().branch, "HEAD");
    git(temporary.path(), &["checkout", "main", "-q"]);
    git(temporary.path(), &["rm", "--cached", CONFIG_PATH]);
    git(temporary.path(), &["commit", "-qm", "remove config"]);
    let error = Catalog::load(temporary.path()).unwrap_err();
    assert!(error.contains("init") && error.contains("提交"), "{error}");
}

#[test]
fn preserves_unknown_documents_and_skips_non_objects() {
    let temporary = repository();
    let root = temporary.path();
    let text = "---\ntype: FutureObject\nid: future\ncustom: { nested: [1, two] }\n---\nbody\n\n";
    write(root, "docs/tower/arch/空 格\t\n.md", text);
    write(
        root,
        "docs/tower/arch/no-id.md",
        "---\ntype: FutureObject\n---\n",
    );
    write(
        root,
        "docs/tower/guide.md",
        "# Existing ordinary Markdown\n",
    );
    for path in [
        "state/bad.md",
        "artifacts/bad.md",
        ".towermd/bad.md",
        "AGENTS.md",
        "arch/ignored.txt",
    ] {
        write(root, &format!("docs/tower/{path}"), "invalid");
    }
    commit(root);
    let catalog = Catalog::load(root).unwrap();
    assert_eq!(catalog.objects.len(), 2);
    assert_eq!(catalog.objects["future"].to_text(), text);
}

#[test]
fn reports_duplicate_ids_and_invalid_documents() {
    let temporary = repository();
    let root = temporary.path();
    write(
        root,
        "docs/tower/arch/duplicate.md",
        "---\ntype: FutureObject\nid: module\n---\n",
    );
    commit(root);
    let error = Catalog::load(root).unwrap_err();
    assert!(
        error.contains("重复") && error.contains("module"),
        "{error}"
    );
    write(
        root,
        "docs/tower/arch/duplicate.md",
        "---\ntype: [invalid\n---\n",
    );
    commit(root);
    assert!(Catalog::load(root).unwrap_err().contains("duplicate.md"));
}

#[test]
fn validates_committed_config_and_normalizes_docs_directory() {
    let temporary = repository();
    let root = temporary.path();
    for directory in [
        "../outside",
        "/tmp/outside",
        ":(glob)**",
        "docs/*",
        "docs//tower",
        ".git",
        "docs/../tower",
        "docs\\tower",
    ] {
        write(
            root,
            CONFIG_PATH,
            &Config::new("project", directory).unwrap().to_yaml(),
        );
        commit(root);
        assert!(Catalog::load(root).is_err(), "{directory}");
    }
    write(
        root,
        CONFIG_PATH,
        &Config::new("project", "./docs/tower/").unwrap().to_yaml(),
    );
    commit(root);
    assert!(Catalog::load(root).unwrap().objects.contains_key("module"));
    for hub in ["remote", "{url: 'wss://example.invalid'}", "null"] {
        write(
            root,
            CONFIG_PATH,
            &format!("project_id: project\ndocs_dir: docs/tower\nhub: {hub}\n"),
        );
        commit(root);
        assert!(Catalog::load(root).unwrap_err().contains("hub"));
    }
}

#[cfg(unix)]
#[test]
fn ignores_document_symlinks_and_rejects_config_symlinks() {
    let temporary = repository();
    let root = temporary.path();
    std::os::unix::fs::symlink("/etc/passwd", root.join("docs/tower/arch/link.md")).unwrap();
    commit(root);
    assert_eq!(Catalog::load(root).unwrap().objects.len(), 1);
    fs::remove_file(root.join(CONFIG_PATH)).unwrap();
    std::os::unix::fs::symlink("../other.yaml", root.join(CONFIG_PATH)).unwrap();
    commit(root);
    assert!(Catalog::load(root).is_err());
}

fn validate(allowed: &[&str], modules: &[&str]) -> Result<(), String> {
    validate_paths(
        &allowed
            .iter()
            .map(|path| path.to_string())
            .collect::<Vec<_>>(),
        &modules
            .iter()
            .map(|path| path.to_string())
            .collect::<Vec<_>>(),
        "docs/tower",
    )
}

#[test]
fn proves_only_safe_path_subsets() {
    for (allowed, module) in [
        ("src/a/file.rs", "src/a/**"),
        ("src/a/nested/**", "src/a/**"),
        ("src/a/**", "src/a/**"),
        ("src/*.rs", "src/*.rs"),
        ("src/**/file?.rs", "src/**/file?.rs"),
        ("src/file.rs", "src/file.rs"),
    ] {
        validate(&[allowed], &[module]).unwrap();
    }
    for (allowed, module) in [
        ("src/a2/file.rs", "src/a/**"),
        ("src/a2/**", "src/a/**"),
        ("src/**", "src/a/**"),
        ("src/a.rs", "src/*.rs"),
        ("src/a/*.rs", "src/**"),
        ("src/file.rs/more", "src/file.rs"),
    ] {
        assert!(
            validate(&[allowed], &[module]).is_err(),
            "{allowed} vs {module}"
        );
    }
    assert!(validate(&["src/a", "tests/a"], &["src/**"]).is_err());
    assert!(validate(&["src/a"], &[]).is_err());
}

#[test]
fn rejects_invalid_paths_and_protected_overlaps_even_when_identical() {
    for path in [
        "",
        "/src",
        "../src",
        "src/../a",
        "src\\a",
        "src//a",
        "src/",
        "./src",
        "C:/src",
        "src/./a",
        ".towermd/config.yaml",
        ".git/config",
        "docs/tower/intents/a.md",
        "docs/tower/arch/**",
        "docs/tower/specs/x",
        "docs/tower/baselines/x",
        "docs/tower/state/**",
        "docs/tower/tower.canvas",
        "docs/**",
        "**",
        "*/**",
        "docs/tower/*",
        "docs/tower/a*/**",
        "src/[",
        "src/{foo",
    ] {
        assert!(validate(&[path], &[path]).is_err(), "{path}");
    }
    validate(
        &["docs/tower/architecture/a"],
        &["docs/tower/architecture/**"],
    )
    .unwrap();
    validate(&["docs/tower/tasks/a"], &["docs/tower/tasks/**"]).unwrap();
    validate(&["src/a/**"], &["**"]).unwrap();
    validate(&["docs/guides/**"], &["docs/**"]).unwrap();
    validate(&["src/a/**"], &[".git/**", "src/**"]).unwrap();
    assert!(validate(&["docs/tower/arch/**"], &["**"]).is_err());
}

#[cfg(unix)]
#[test]
fn cached_catalog_does_not_wait_for_slow_upload_pack() {
    let origin = repository();
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &["clone", "-q", origin.path().to_str().unwrap(), "."],
    );
    git(
        clone.path(),
        &[
            "config",
            "remote.origin.uploadpack",
            "sleep 45; git-upload-pack",
        ],
    );
    let start = std::time::Instant::now();
    let result = Catalog::load(clone.path());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert!(result.is_ok());
}

#[test]
fn reports_unreadable_blob_and_invalid_id() {
    let temporary = repository();
    let root = temporary.path();
    write(
        root,
        "docs/tower/arch/bad-id.md",
        "---\ntype: Module\nid: [invalid]\n---\n",
    );
    commit(root);
    assert!(Catalog::load(root).unwrap_err().contains("bad-id.md"));
    fs::remove_file(root.join("docs/tower/arch/bad-id.md")).unwrap();
    commit(root);
    let oid = git(root, &["rev-parse", "main:docs/tower/arch/module.md"]);
    fs::remove_file(root.join(".git/objects").join(&oid[..2]).join(&oid[2..])).unwrap();
    assert!(Catalog::load(root).unwrap_err().contains("module.md"));
}

#[cfg(unix)]
#[test]
fn rejects_non_utf8_paths_and_document_contents() {
    use std::os::unix::ffi::OsStrExt;
    let temporary = repository();
    let root = temporary.path();
    let oid = git(root, &["rev-parse", "main:docs/tower/arch/module.md"]);
    assert!(git_command(root)
        .args(["update-index", "--add", "--cacheinfo", "100644", &oid])
        .arg(std::ffi::OsStr::from_bytes(b"docs/tower/arch/bad-\xff.md"))
        .status()
        .unwrap()
        .success());
    git(root, &["commit", "-qm", "non-UTF-8 path"]);
    assert!(Catalog::load(root).unwrap_err().contains("UTF-8"));
    git(root, &["reset", "--hard", "HEAD~1"]);
    fs::write(
        root.join("docs/tower/arch/bad.md"),
        b"---\ntype: Module\n---\n\xff",
    )
    .unwrap();
    commit(root);
    let error = Catalog::load(root).unwrap_err();
    assert!(
        error.contains("bad.md") && error.contains("UTF-8"),
        "{error}"
    );
}

#[test]
fn inherited_git_environment_cannot_redirect_catalog() {
    if let Some(root) = std::env::var_os("TOWERMD_CATALOG_TEST_ROOT") {
        let root = std::path::PathBuf::from(root);
        let catalog = Catalog::load(&root).unwrap();
        assert_eq!(catalog.root, root.canonicalize().unwrap());
        assert!(catalog.objects.contains_key("module"));
        return;
    }
    let origin = repository();
    let clone = tempfile::tempdir().unwrap();
    let other = repository();
    git(
        clone.path(),
        &["clone", "-q", origin.path().to_str().unwrap(), "."],
    );
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inherited_git_environment_cannot_redirect_catalog",
            "--nocapture",
        ])
        .env("TOWERMD_CATALOG_TEST_ROOT", clone.path())
        .env("GIT_DIR", other.path().join(".git"))
        .env("GIT_WORK_TREE", other.path())
        .env("GIT_COMMON_DIR", other.path().join(".git"))
        .env("GIT_INDEX_FILE", other.path().join("wrong-index"))
        .env("GIT_OBJECT_DIRECTORY", other.path().join(".git/objects"))
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "remote.origin.url")
        .env("GIT_CONFIG_VALUE_0", "/nonexistent/catalog-origin")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("1 passed"));
}

fn assert_replacements_are_ignored(replace_blob: bool) {
    let temporary = repository();
    let root = temporary.path();
    let original = Catalog::load(root).unwrap();
    let before = git(root, &["rev-parse", "main"]);
    let blob = git(root, &["rev-parse", "main:docs/tower/arch/module.md"]);
    git(root, &["checkout", "-qb", "t/replace"]);
    write(
        root,
        "docs/tower/arch/module.md",
        "---\ntype: Module\nid: module\npaths: ['**']\n---\nTampered.\n",
    );
    commit(root);
    let (source, replacement) = if replace_blob {
        (
            blob,
            git(root, &["rev-parse", "HEAD:docs/tower/arch/module.md"]),
        )
    } else {
        (before.clone(), git(root, &["rev-parse", "HEAD"]))
    };
    git(root, &["replace", &source, &replacement]);
    assert_eq!(git(root, &["rev-parse", "main"]), before);
    assert!(git(root, &["show", "main:docs/tower/arch/module.md"]).contains("Tampered"));
    let catalog = Catalog::load(root).unwrap();
    assert_eq!(catalog.revision, before);
    assert_eq!(
        catalog.objects["module"].to_text(),
        original.objects["module"].to_text()
    );
    let expected = original.objects["module"].to_text();
    let args = ["show", "main:docs/tower/arch/module.md"];
    assert_eq!(
        towermd_core::repo::git(root, &args).unwrap(),
        expected.strip_suffix('\n').unwrap()
    );
    assert_eq!(
        towermd_core::repo::git_timeout(root, &args, std::time::Duration::from_secs(2)).unwrap(),
        expected.strip_suffix('\n').unwrap()
    );
}

#[test]
fn commit_replacement_cannot_change_authoritative_objects() {
    assert_replacements_are_ignored(false);
}

#[test]
fn blob_replacement_cannot_change_authoritative_objects() {
    assert_replacements_are_ignored(true);
}

#[test]
fn collects_slow_object_paths_across_the_authoritative_tree() {
    let temporary = repository();
    let root = temporary.path();
    let protected = [
        (
            "docs/tower/decisions/slow.md",
            "---\ntype: Decision\nid: decision\n---\n",
        ),
        (
            "docs/tower/notes/no-id.md",
            "---\ntype: FutureObject\nlayer: L2\n---\n",
        ),
        (
            "src/abc/slow.md",
            "---\ntype: FutureObject\nlayer: L1\nid: module\n---\n",
        ),
        ("outside/intent.md", "---\ntype: Intent\n---\n"),
        ("outside/module.md", "---\ntype: Module\n---\n"),
        ("outside/acceptance.md", "---\ntype: Acceptance\n---\n"),
        (
            "docs/tower/state/slow.md",
            "---\ntype: FutureObject\nlayer: L0\n---\n",
        ),
    ];
    for (path, text) in protected {
        write(root, path, text);
    }
    for (path, text) in [
        ("outside/readme.md", "# Ordinary Markdown\n"),
        ("outside/fast.md", "---\ntype: Decision\nlayer: L4\n---\n"),
        ("outside/unknown.md", "---\ntype: FutureObject\n---\n"),
    ] {
        write(root, path, text);
    }
    commit(root);
    write(root, "src/abc/slow.md", "---\ntype: Task\nlayer: L4\n---\n");
    let catalog = Catalog::load(root).unwrap();
    let mut expected = protected
        .iter()
        .filter(|(path, _)| !path.starts_with("docs/tower/state/"))
        .map(|(path, _)| path.to_string())
        .collect::<Vec<_>>();
    expected.push("docs/tower/arch/module.md".into());
    expected.sort();
    assert_eq!(catalog.protected_paths, expected);
    assert_eq!(catalog.objects.len(), 2);
    assert!(catalog.objects.contains_key("decision"));
    assert!(catalog.objects.contains_key("module"));
    assert!(validate_paths(
        &["docs/tower/state/slow.md".into()],
        &["docs/tower/state/**".into()],
        "docs/tower"
    )
    .is_err());
}

#[test]
fn rejects_malformed_external_okf_with_path_context() {
    let temporary = repository();
    let root = temporary.path();
    for text in [
        "---\nlayer: L1\n---\n",
        "---\ntype: ''\nlayer: L1\n---\n",
        "---\ntype: Module\nlayer: [\n---\n",
    ] {
        write(root, "outside/broken.md", text);
        commit(root);
        let error = Catalog::load(root).unwrap_err();
        assert!(error.contains("outside/broken.md"), "{error}");
    }
}

#[test]
fn protects_known_slow_paths_from_literal_and_uncertain_glob_overlap() {
    let protected = vec!["src/abc/slow.md".into(), "docs/tower/decisions/a.md".into()];
    for pattern in [
        "src/abc/slow.md",
        "src/abc/**",
        "src/**",
        "src/a*/**",
        "src/**/slow.md",
        "**/slow.md",
        "src/a?c/slow.md",
        "docs/tower/decisions/*.md",
        "SRC/ABC/slow.md",
    ] {
        let error = validate_protected_paths(&[pattern.into()], &protected).unwrap_err();
        assert!(error.contains("保护") && error.contains(pattern), "{error}");
    }
    for pattern in ["src/abcd/**", "src/xyz/**", "src/abc/fast.md", "tests/*.rs"] {
        validate_protected_paths(&[pattern.into()], &protected).unwrap();
    }
    assert!(
        validate_protected_paths(&["src/xyz/**".into(), "src/a*/**".into()], &protected).is_err()
    );
    assert!(validate_protected_paths(&["../src/**".into()], &[]).is_err());
}

#[test]
fn offline_catalog_reads_cached_main_without_fetching() {
    let origin = repository();
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &["clone", "-q", origin.path().to_str().unwrap(), "."],
    );
    let cached = git(clone.path(), &["rev-parse", "origin/main"]);
    git(
        clone.path(),
        &[
            "remote",
            "set-url",
            "origin",
            "/nonexistent/towermd-offline",
        ],
    );
    let index = fs::read(clone.path().join(".git/index")).unwrap();
    let catalog = Catalog::load(clone.path()).unwrap();
    assert_eq!(catalog.revision, cached);
    assert!(catalog.objects.contains_key("module"));
    assert_eq!(fs::read(clone.path().join(".git/index")).unwrap(), index);
    assert!(!clone.path().join(".git/FETCH_HEAD").exists());
}

#[test]
fn local_main_is_not_a_remote_project_bootstrap_authority() {
    let root = repository();
    git(
        root.path(),
        &["remote", "add", "origin", "/nonexistent/towermd-origin"],
    );
    assert!(Catalog::load_optional(root.path()).unwrap().is_none());
    assert!(Catalog::load(root.path())
        .unwrap_err()
        .contains("权威 main"));
    git(root.path(), &["checkout", "-qb", "t/untrusted"]);
    write(
        root.path(),
        "docs/tower/arch/module.md",
        "---\ntype: Module\nid: evil\n---\n",
    );
    commit(root.path());
    assert!(Catalog::load_optional(root.path()).unwrap().is_none());
}

#[test]
fn removing_origin_uses_local_main_even_with_leftover_tracking_ref() {
    let root = repository();
    let original = git(root.path(), &["rev-parse", "main"]);
    git(
        root.path(),
        &["update-ref", "refs/remotes/origin/main", &original],
    );
    write(
        root.path(),
        "docs/tower/arch/new.md",
        "---\ntype: Module\nid: new\n---\n",
    );
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    assert!(catalog.objects.contains_key("new"));
    assert_ne!(catalog.revision, original);
}

#[test]
fn unborn_repository_has_no_authority_even_after_local_init() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(root.path(), &Default::default(), &[]).unwrap();
    assert!(Catalog::load_optional(root.path()).unwrap().is_none());
    assert!(!root.path().join(".git/towermd").exists());
}

#[test]
fn exact_authority_reference_preserves_commit_and_tag_but_rejects_invalid_refs() {
    for kind in [
        "commit",
        "tag",
        "blob",
        "tree",
        "corrupt-ref",
        "missing-object",
        "corrupt-object",
        "prefix",
        "resolved-symbolic",
        "missing",
    ] {
        let root = repository();
        let original = git(root.path(), &["rev-parse", "HEAD"]);
        git(
            root.path(),
            &["remote", "add", "origin", "/nonexistent/exact-authority"],
        );
        let reference = "refs/remotes/origin/main";
        match kind {
            "missing" => {}
            "prefix" => {
                git(
                    root.path(),
                    &["update-ref", "refs/remotes/origin/main/child", &original],
                );
            }
            "resolved-symbolic" => {
                git(
                    root.path(),
                    &["update-ref", "refs/remotes/origin/other", &original],
                );
                git(
                    root.path(),
                    &["symbolic-ref", reference, "refs/remotes/origin/other"],
                );
            }
            "corrupt-ref" | "missing-object" => {
                let path = root.path().join(".git/refs/remotes/origin/main");
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(
                    path,
                    if kind == "corrupt-ref" {
                        "not-an-oid\n".into()
                    } else {
                        format!("{}\n", "f".repeat(40))
                    },
                )
                .unwrap();
            }
            _ => {
                let target = match kind {
                    "tag" => {
                        git(
                            root.path(),
                            &["tag", "-a", "authority", "-m", "authority tag", &original],
                        );
                        git(root.path(), &["rev-parse", "authority"])
                    }
                    "blob" => git(root.path(), &["rev-parse", "HEAD:.towermd/config.yaml"]),
                    "tree" => git(root.path(), &["rev-parse", "HEAD^{tree}"]),
                    _ => original.clone(),
                };
                git(root.path(), &["update-ref", reference, &target]);
                if kind == "corrupt-object" {
                    let object = root
                        .path()
                        .join(".git/objects")
                        .join(&original[..2])
                        .join(&original[2..]);
                    fs::remove_file(&object).unwrap();
                    fs::write(object, b"corrupt loose object").unwrap();
                }
            }
        }
        let loaded = Catalog::load_optional(root.path());
        match kind {
            "commit" | "tag" => assert_eq!(loaded.unwrap().unwrap().revision, original, "{kind}"),
            "missing" => assert!(loaded.unwrap().is_none()),
            _ => assert!(
                loaded.is_err(),
                "{kind} must be an authority error, not missing or a prefix match"
            ),
        }
    }
}

#[test]
fn invalid_layer_remains_readable_and_protected() {
    let root = repository();
    let text = "---\ntype: Task\nid: invalid\nlayer: L9\nfuture: {nested: [unknown, 0.5]}\n---\nOriginal bytes.\n";
    write(root.path(), "src/a/invalid.md", text);
    commit(root.path());
    let catalog = Catalog::load(root.path()).expect("invalid layer is readable");
    assert!(catalog.protected_paths.contains(&"src/a/invalid.md".into()));
    assert_eq!(
        serde_json::to_value(&catalog.diagnostics).unwrap(),
        serde_json::json!([{
            "code":"invalid_layer", "path":"src/a/invalid.md", "message":"非法 layer；按慢层保护，不采用 type 默认层"
        }])
    );
    assert!(!catalog.objects.contains_key("invalid"));
    assert_eq!(
        catalog.read_file("src/a/invalid.md").unwrap().unwrap(),
        text
    );
}

#[test]
fn invalid_layer_diagnostics_cover_all_values_inside_and_outside_documents() {
    let root = repository();
    let mut expected = Vec::new();
    let mut texts = Vec::new();
    for (index, value) in [
        "null",
        "''",
        "'  '",
        "42",
        "true",
        "[L1]",
        "{value: L1}",
        "L9",
    ]
    .iter()
    .enumerate()
    {
        for (base, kind, id) in [
            ("outside", "Task", ""),
            ("docs/tower/notes", "FutureThing", ""),
            ("docs/tower/decisions", "Decision", "id: invalid-unique\n"),
        ] {
            let path = format!("{base}/invalid-{index}.md");
            let id = id.replace("unique", &index.to_string());
            let text = format!("---\ntype: {kind}\n{id}layer: {value}\nfuture: {{nested: [unknown, 0.5]}}\n---\nKeep.\n");
            write(root.path(), &path, &text);
            expected.push(path.clone());
            texts.push((path, text));
        }
    }
    for (name, layer) in [
        ("missing", ""),
        ("slow", "layer: L2\n"),
        ("fast", "layer: L4\n"),
    ] {
        write(
            root.path(),
            &format!("outside/{name}.md"),
            &format!("---\ntype: FutureThing\n{layer}---\n"),
        );
    }
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    expected.sort();
    assert_eq!(
        catalog
            .diagnostics
            .iter()
            .map(|d| d.path.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(catalog
        .diagnostics
        .iter()
        .all(|d| d.code == "invalid_layer"));
    assert_eq!(catalog.objects.len(), 9);
    for (path, text) in texts {
        assert!(catalog.protected_paths.contains(&path), "{path}");
        assert_eq!(catalog.read_file(&path).unwrap().unwrap(), text);
        assert_eq!(
            towermd_core::okf::Document::parse(&text).unwrap().to_text(),
            text
        );
        assert!(validate_protected_paths(&[path], &catalog.protected_paths).is_err());
    }
    for pattern in ["outside/**", "out*/**", "**/invalid-0.md"] {
        assert!(
            validate_protected_paths(&[pattern.into()], &catalog.protected_paths).is_err(),
            "{pattern}"
        );
    }
    assert!(catalog.protected_paths.contains(&"outside/slow.md".into()));
    assert!(!catalog
        .protected_paths
        .contains(&"outside/missing.md".into()));
    assert!(!catalog.protected_paths.contains(&"outside/fast.md".into()));
    validate_protected_paths(&["src/safe.rs".into()], &catalog.protected_paths).unwrap();
    assert!(validate(&["docs/tower/arch/fast.md"], &["docs/tower/arch/**"]).is_err());
}
