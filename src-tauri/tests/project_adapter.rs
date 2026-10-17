use std::{fs, path::Path, process::Command};
use towermd_app::project::ProjectRegistry;

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    dir
}
#[test]
fn selected_subdirectory_binds_canonical_root_and_revokes_previous_token() {
    let first = repository();
    let second = repository();
    let sub = first.path().join("中文 空格");
    fs::create_dir(&sub).unwrap();
    let registry = ProjectRegistry::default();
    let selected = registry
        .select(Some(sub))
        .unwrap()
        .expect("选择 Git 项目应返回令牌");
    assert_eq!(
        selected.display_path,
        first.path().canonicalize().unwrap().to_string_lossy()
    );
    assert_eq!(
        registry.resolve(&selected.token).unwrap(),
        first.path().canonicalize().unwrap()
    );
    assert!(registry.select(None).unwrap().is_none());
    assert!(registry.resolve(&selected.token).is_ok());
    let next = registry
        .select(Some(second.path().to_path_buf()))
        .unwrap()
        .unwrap();
    assert_ne!(selected.token, next.token);
    assert_eq!(
        registry.resolve(&selected.token).unwrap_err().code,
        "project_missing"
    );
    assert_eq!(
        registry.resolve("unknown").unwrap_err().code,
        "project_missing"
    );
}
#[test]
fn non_git_selection_reports_real_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let error = ProjectRegistry::default()
        .select(Some(dir.path().to_path_buf()))
        .unwrap_err();
    assert_eq!(error.code, "not_git_project");
    assert!(error.detail.unwrap().contains("not a git repository"));
}
#[test]
fn config_marker_must_be_a_regular_file_and_symlinks_are_rejected() {
    let dir = repository();
    let registry = ProjectRegistry::default();
    assert!(
        !registry
            .select(Some(dir.path().to_path_buf()))
            .unwrap()
            .unwrap()
            .has_config
    );
    fs::create_dir(dir.path().join(".towermd")).unwrap();
    let marker = dir.path().join(".towermd/config.yaml");
    fs::write(&marker, "unparsed marker").unwrap();
    assert!(
        registry
            .select(Some(dir.path().to_path_buf()))
            .unwrap()
            .unwrap()
            .has_config
    );
    fs::remove_file(&marker).unwrap();
    std::os::unix::fs::symlink("missing", &marker).unwrap();
    assert_eq!(
        registry
            .select(Some(dir.path().to_path_buf()))
            .unwrap_err()
            .code,
        "config_marker_invalid"
    );
    fs::remove_file(&marker).unwrap();
    fs::create_dir(&marker).unwrap();
    assert_eq!(
        registry
            .select(Some(dir.path().to_path_buf()))
            .unwrap_err()
            .code,
        "config_marker_invalid"
    );
}
#[test]
fn symlink_selection_resolves_to_git_root() {
    let dir = repository();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("项目链接");
    std::os::unix::fs::symlink(dir.path(), &link).unwrap();
    let selected = ProjectRegistry::default()
        .select(Some(link))
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.display_path,
        dir.path().canonicalize().unwrap().to_string_lossy()
    );
}

#[test]
fn linked_worktree_is_its_own_canonical_project_root() {
    let dir = repository();
    git(
        dir.path(),
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@localhost",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
    let outside = tempfile::tempdir().unwrap();
    let linked = outside.path().join("链接工作树 空格");
    git(
        dir.path(),
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    let selected = ProjectRegistry::default()
        .select(Some(linked.clone()))
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.display_path,
        linked.canonicalize().unwrap().to_string_lossy()
    );
}

#[test]
fn invalid_or_revoked_tokens_and_empty_docs_are_rejected_before_cli() {
    let registry = ProjectRegistry::default();
    let error = tauri::async_runtime::block_on(towermd_app::project::initialize(
        registry.clone(),
        "unknown".into(),
        "docs/tower".into(),
    ))
    .unwrap_err();
    assert_eq!(error.code, "project_missing");
    let first = repository();
    let token = registry
        .select(Some(first.path().to_path_buf()))
        .unwrap()
        .unwrap()
        .token;
    let error = tauri::async_runtime::block_on(towermd_app::project::initialize(
        registry.clone(),
        token.clone(),
        "  ".into(),
    ))
    .unwrap_err();
    assert_eq!(error.code, "docs_dir_empty");
    let second = repository();
    registry.select(Some(second.path().to_path_buf())).unwrap();
    let error = tauri::async_runtime::block_on(towermd_app::project::initialize(
        registry,
        token,
        "docs/tower".into(),
    ))
    .unwrap_err();
    assert_eq!(error.code, "project_missing");
    assert!(!first.path().join(".towermd").exists());
    assert!(!second.path().join(".towermd").exists());
}

#[test]
fn init_rechecks_git_root_before_executing_cli() {
    let first = repository();
    let registry = ProjectRegistry::default();
    let token = registry
        .select(Some(first.path().to_path_buf()))
        .unwrap()
        .unwrap()
        .token;
    fs::remove_dir_all(first.path().join(".git")).unwrap();
    let error = tauri::async_runtime::block_on(towermd_app::project::initialize(
        registry,
        token,
        "docs/tower".into(),
    ))
    .unwrap_err();
    assert_eq!(error.code, "not_git_project");
    assert!(!first.path().join(".towermd").exists());
}

#[test]
fn real_native_cli_init_preserves_head_index_markdown_and_repeated_config() {
    let dir = repository();
    let markdown = dir.path().join("已有 文档.md");
    fs::write(&markdown, "# 原文\n提交版\n").unwrap();
    git(dir.path(), &["add", "--", "已有 文档.md"]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@localhost",
            "commit",
            "-m",
            "fixture",
        ],
    );
    fs::write(&markdown, "# 原文\n已暂存，须逐字保留\n").unwrap();
    git(dir.path(), &["add", "--", "已有 文档.md"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]).stdout;
    let index = fs::read(dir.path().join(".git/index")).unwrap();
    let staged = git(dir.path(), &["ls-files", "--stage"]).stdout;
    let original = fs::read(&markdown).unwrap();
    assert!(git(dir.path(), &["remote"]).stdout.is_empty());
    let registry = ProjectRegistry::default();
    let token = registry
        .select(Some(dir.path().to_path_buf()))
        .unwrap()
        .unwrap()
        .token;
    let docs = "文档 空格 '单引号' \"双引号\" $(touch 不应出现)";
    let output = tauri::async_runtime::block_on(towermd_app::project::initialize(
        registry.clone(),
        token.clone(),
        docs.into(),
    ))
    .unwrap();
    assert_eq!(output.exit_code, Some(0));
    assert!(dir.path().join(docs).is_dir());
    assert!(!dir.path().join("不应出现").exists());
    let config = fs::read(dir.path().join(".towermd/config.yaml")).unwrap();
    assert!(String::from_utf8_lossy(&config).contains("project_id:"));
    tauri::async_runtime::block_on(towermd_app::project::initialize(
        registry,
        token,
        docs.into(),
    ))
    .unwrap();
    assert_eq!(
        config,
        fs::read(dir.path().join(".towermd/config.yaml")).unwrap()
    );
    assert_eq!(head, git(dir.path(), &["rev-parse", "HEAD"]).stdout);
    assert_eq!(index, fs::read(dir.path().join(".git/index")).unwrap());
    assert_eq!(staged, git(dir.path(), &["ls-files", "--stage"]).stdout);
    assert_eq!(original, fs::read(markdown).unwrap());
}

#[test]
fn real_native_cli_init_in_empty_repository_does_not_create_head_or_index() {
    let dir = repository();
    let registry = ProjectRegistry::default();
    let token = registry
        .select(Some(dir.path().to_path_buf()))
        .unwrap()
        .unwrap()
        .token;
    tauri::async_runtime::block_on(towermd_app::project::initialize(
        registry,
        token,
        "docs/tower".into(),
    ))
    .unwrap();
    assert!(!Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .unwrap()
        .status
        .success());
    assert!(!dir.path().join(".git/index").exists());
}

#[test]
fn stale_native_selection_does_not_revoke_newer_project_token() {
    let first = repository();
    let second = repository();
    let registry = ProjectRegistry::default();
    let old_request = registry.begin_selection().unwrap();
    let new_request = registry.begin_selection().unwrap();
    let current = registry
        .finish_selection(new_request, Some(second.path().to_path_buf()))
        .unwrap()
        .unwrap();
    assert!(registry
        .finish_selection(old_request, Some(first.path().to_path_buf()))
        .unwrap()
        .is_none());
    assert_eq!(
        registry.resolve(&current.token).unwrap(),
        second.path().canonicalize().unwrap()
    );
}

#[test]
fn broken_or_redirected_config_directory_is_not_reported_as_uninitialized() {
    let dir = repository();
    let registry = ProjectRegistry::default();
    let config_dir = dir.path().join(".towermd");
    std::os::unix::fs::symlink("missing-directory", &config_dir).unwrap();
    assert_eq!(
        registry
            .select(Some(dir.path().to_path_buf()))
            .unwrap_err()
            .code,
        "config_marker_invalid"
    );
    fs::remove_file(&config_dir).unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("config.yaml"), "other project").unwrap();
    std::os::unix::fs::symlink(outside.path(), &config_dir).unwrap();
    assert_eq!(
        registry
            .select(Some(dir.path().to_path_buf()))
            .unwrap_err()
            .code,
        "config_marker_invalid"
    );
}

#[test]
fn inaccessible_marker_directory_reports_diagnostic_instead_of_missing_config() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repository();
    let config_dir = dir.path().join(".towermd");
    fs::create_dir(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "marker").unwrap();
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o000)).unwrap();
    let result = ProjectRegistry::default().select(Some(dir.path().to_path_buf()));
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(result.unwrap_err().code, "config_marker_invalid");
}

#[test]
fn cancelling_native_selection_keeps_token_and_does_not_create_files() {
    let dir = repository();
    let registry = ProjectRegistry::default();
    let selected = registry
        .select(Some(dir.path().to_path_buf()))
        .unwrap()
        .unwrap();
    let request = registry.begin_selection().unwrap();
    assert!(registry.finish_selection(request, None).unwrap().is_none());
    assert!(registry.resolve(&selected.token).is_ok());
    assert!(!dir.path().join(".towermd").exists());
    assert!(!dir.path().join("docs").exists());
}
