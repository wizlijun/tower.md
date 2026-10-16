use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use uuid::Uuid;

use crate::config::{Config, CONFIG_PATH, DEFAULT_DOCS_DIR};
use crate::repo;

pub const AGENTS_PATH: &str = ".towermd/AGENTS.md";
const AGENTS_TEMPLATE: &str = include_str!("../templates/agents.md");
const PROTECTED_SUBDIRS: &[&str] = &["intents", "arch", "specs", "baselines", "state"];
const SKELETON: &[&str] = &["intents", "arch", "specs", "baselines", "decisions"];

#[derive(Debug, Default)]
pub struct InitOptions {
    pub docs_dir: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct InitReport {
    pub root: PathBuf,
    pub project_id: String,
    pub docs_dir: String,
    pub reused_config: bool,
    pub created: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub hints: Vec<String>,
}

pub fn render_agents_md(docs_dir: &str) -> String {
    AGENTS_TEMPLATE.replace("{docs_dir}", docs_dir)
}

pub fn normalize_docs_dir(raw: &str) -> Result<String, String> {
    let invalid = || {
        format!("docs_dir `{raw}` 无效：须为仓库内的相对目录，不能占用根 AGENTS.md，不能包含空路径段、..、保留目录 .towermd/.git、盘符或控制字符")
    };
    if raw.chars().any(char::is_control) || raw.contains(['\\', ':']) {
        return Err(invalid());
    }
    let trimmed = raw.trim();
    let relative = trimmed.strip_prefix("./").unwrap_or(trimmed);
    let normalized = relative.strip_suffix('/').unwrap_or(relative);
    if normalized
        .split('/')
        .next()
        .is_some_and(|component| component.eq_ignore_ascii_case("AGENTS.md"))
    {
        return Err(invalid());
    }
    if normalized.split('/').any(|component| {
        component.is_empty()
            || component == "."
            || component == ".."
            || component.eq_ignore_ascii_case(".towermd")
            || component.eq_ignore_ascii_case(".git")
    }) {
        return Err(invalid());
    }
    Ok(normalized.to_string())
}

pub fn init(dir: &Path, opts: &InitOptions) -> Result<InitReport, String> {
    let root = repo::toplevel(dir)?;
    let requested = opts
        .docs_dir
        .as_deref()
        .map(normalize_docs_dir)
        .transpose()?;
    check_path(&root, CONFIG_PATH, false)?;
    let existing = Config::load(&root)?;
    let reused_config = existing.is_some();
    let docs_dir = match &existing {
        Some(config) => {
            let normalized = normalize_docs_dir(config.docs_dir())?;
            if requested
                .as_ref()
                .is_some_and(|requested| requested != &normalized)
            {
                return Err(format!(
                    "docs_dir 已设为 `{normalized}`；init 不修改已有配置，改动请走提案"
                ));
            }
            normalized
        }
        None => requested.unwrap_or_else(|| DEFAULT_DOCS_DIR.to_string()),
    };
    check_path(&root, &docs_dir, true)?;
    let mut missing_directories = Vec::new();
    for subdirectory in SKELETON {
        let relative = format!("{docs_dir}/{subdirectory}");
        if !check_path(&root, &relative, true)? {
            missing_directories.push(relative);
        }
    }
    let current_agents = read_optional(&root, AGENTS_PATH)?;
    let root_agents = read_optional(&root, "AGENTS.md")?;
    let mut warnings = if reused_config {
        Vec::new()
    } else {
        protected_name_collisions(&root, &docs_dir)
    };
    let config = match existing {
        Some(config) => config,
        None => Config::new(&Uuid::now_v7().to_string(), &docs_dir)?,
    };
    if let Some(diagnostic) = push_preflight(&root)? {
        warnings.push(diagnostic);
    }
    let mut created = Vec::new();
    if !reused_config {
        write_new(&root, CONFIG_PATH, &config.to_yaml(), &mut created)?;
    }
    fs::create_dir_all(root.join(&docs_dir))
        .map_err(|error| format!("无法创建 {docs_dir}：{error}"))?;
    for relative in missing_directories {
        fs::create_dir(root.join(&relative))
            .map_err(|error| format!("无法创建 {relative}，可重试 init：{error}"))?;
        write_new(&root, &format!("{relative}/.gitkeep"), "", &mut created)?;
    }
    let agents = render_agents_md(&docs_dir);
    match current_agents {
        Some(current) if current == agents => {}
        Some(_) => warnings.push(format!("{AGENTS_PATH} 已存在且与生成内容不同，未覆盖")),
        None => write_new(&root, AGENTS_PATH, &agents, &mut created)?,
    }
    let mut hints = Vec::new();
    if !root_agents
        .as_deref()
        .unwrap_or_default()
        .contains(AGENTS_PATH)
    {
        hints.push(format!("请在仓库根目录的 AGENTS.md 中自行加一行：`Tower.md 的规矩见 {AGENTS_PATH}`（init 不修改该文件）"));
    }
    Ok(InitReport {
        root,
        project_id: config.project_id().to_string(),
        docs_dir,
        reused_config,
        created,
        warnings,
        hints,
    })
}

fn push_preflight(root: &Path) -> Result<Option<String>, String> {
    let timeout = Duration::from_secs(30);
    let remotes = repo::git_timeout(root, &["remote"], timeout)?;
    if !remotes.lines().any(|remote| remote == "origin") {
        return Ok(None);
    }
    let main = repo::git_timeout(
        root,
        &["rev-parse", "--verify", "refs/heads/main^{commit}"],
        timeout,
    )
    .map_err(|error| {
        format!("origin 推送预检需要已有的本地 main 提交；请先准备 main，再运行 init：{error}")
    })?;
    let hooks =
        tempfile::tempdir().map_err(|error| format!("无法创建预检 hooks 隔离目录：{error}"))?;
    let hooks_path = hooks.path().to_str().ok_or("预检 hooks 路径不是 UTF-8")?;
    repo::git_timeout(
        root,
        &[
            "-c", &format!("core.hooksPath={hooks_path}"),
            "-c", "remote.origin.mirror=false",
            "-c", "push.followTags=false",
            "-c", "push.gpgSign=false",
            "-c", "core.fsmonitor=false",
            "-c", "gc.auto=0",
            "-c", "maintenance.auto=false",
            "push", "--dry-run", "--porcelain", "--no-verify",
            "--no-follow-tags", "--no-signed", "--recurse-submodules=no",
            "origin", &format!("{main}:refs/heads/main"),
        ],
        timeout,
    )
    .map_err(|error| format!("origin/main 的 push --dry-run 预检失败；请检查连接、认证与 main 更新条件后重试 init（未实际推送）：{error}"))?;
    Ok(Some("origin/main 的 push --dry-run 预检通过：当前连接可完成推送协商，未实际推送；不保证服务端全部写入策略、保护规则或 hooks 会允许后续真实推送。".into()))
}

fn check_path(root: &Path, relative: &str, directory: bool) -> Result<bool, String> {
    let mut path = root.to_path_buf();
    let mut components = Path::new(relative).components().peekable();
    while let Some(component) = components.next() {
        path.push(component);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("无法检查 {}：{error}", path.display())),
        };
        if metadata.file_type().is_symlink() {
            return Err(format!("拒绝符号链接：{}", path.display()));
        }
        let needs_directory = components.peek().is_some() || directory;
        if (needs_directory && !metadata.is_dir()) || (!needs_directory && !metadata.is_file()) {
            return Err(format!(
                "{} 应为{}",
                path.display(),
                if needs_directory {
                    "目录"
                } else {
                    "普通文件"
                }
            ));
        }
    }
    Ok(true)
}

fn read_optional(root: &Path, relative: &str) -> Result<Option<String>, String> {
    if !check_path(root, relative, false)? {
        return Ok(None);
    }
    fs::read_to_string(root.join(relative))
        .map(Some)
        .map_err(|error| format!("无法读取 {relative}：{error}"))
}

fn protected_name_collisions(root: &Path, docs_dir: &str) -> Vec<String> {
    let base = root.join(docs_dir);
    let mut hits: Vec<&str> = PROTECTED_SUBDIRS
        .iter()
        .copied()
        .filter(|subdirectory| base.join(subdirectory).is_dir())
        .collect();
    if base.join("tower.canvas").exists() {
        hits.push("tower.canvas");
    }
    if hits.is_empty() {
        return Vec::new();
    }
    vec![format!("`{docs_dir}` 下已有 {}，它们会整体成为受保护路径（只能经人批准修改）；建议另选一个专用目录", hits.join("、"))]
}

fn write_new(
    root: &Path,
    relative: &str,
    content: &str,
    created: &mut Vec<String>,
) -> Result<(), String> {
    check_path(root, relative, false)?;
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 {}：{error}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("无法新建 {relative}，可重试 init：{error}"))?;
    if let Err(error) = file.write_all(content.as_bytes()) {
        drop(file);
        fs::remove_file(&path).map_err(|cleanup| {
            format!("无法写入 {relative}：{error}；清理本次新建文件失败：{cleanup}")
        })?;
        return Err(format!("无法写入 {relative}：{error}"));
    }
    created.push(relative.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::{Arc, Barrier};

    fn fixture_git(directory: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        let output = command
            .args(args)
            .current_dir(directory)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    fn git_repo() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        fixture_git(directory.path(), &["init", "-q"]);
        directory
    }

    fn opts(docs_dir: Option<&str>) -> InitOptions {
        InitOptions {
            docs_dir: docs_dir.map(str::to_string),
        }
    }

    #[test]
    fn first_run_writes_config_agents_and_skeleton() {
        let repository = git_repo();
        let report = init(repository.path(), &opts(None)).unwrap();
        assert!(!report.reused_config);
        assert_eq!(report.docs_dir, DEFAULT_DOCS_DIR);
        assert_eq!(
            Uuid::parse_str(&report.project_id)
                .unwrap()
                .get_version_num(),
            7
        );
        let config = Config::load(repository.path()).unwrap().unwrap();
        assert_eq!(config.project_id(), report.project_id);
        let agents = fs::read_to_string(repository.path().join(AGENTS_PATH)).unwrap();
        assert!(agents.contains("`docs/tower/intents/`"));
        assert!(!agents.contains("{docs_dir}"));
        for subdirectory in ["intents", "arch", "specs", "baselines", "decisions"] {
            let placeholder = repository
                .path()
                .join(format!("docs/tower/{subdirectory}/.gitkeep"));
            assert_eq!(fs::read(placeholder).unwrap(), b"");
        }
        assert!(!repository.path().join("docs/tower/state").exists());
        assert!(report.created.contains(&CONFIG_PATH.to_string()));
        assert!(report.created.contains(&AGENTS_PATH.to_string()));
        assert_eq!(report.created.len(), 7);
    }

    #[test]
    fn second_run_keeps_project_id_and_config_bytes() {
        let repository = git_repo();
        let first = init(repository.path(), &opts(Some("docs/plan"))).unwrap();
        let before = fs::read(repository.path().join(CONFIG_PATH)).unwrap();
        let second = init(repository.path(), &opts(None)).unwrap();
        assert!(second.reused_config);
        assert_eq!(second.project_id, first.project_id);
        assert_eq!(second.docs_dir, "docs/plan");
        assert!(second.created.is_empty(), "{:?}", second.created);
        assert_eq!(
            fs::read(repository.path().join(CONFIG_PATH)).unwrap(),
            before
        );
    }

    #[test]
    fn normalizes_existing_and_requested_paths_without_rewriting_config() {
        let repository = git_repo();
        fs::create_dir(repository.path().join(".towermd")).unwrap();
        let original =
            "# keep this comment\nproject_id: existing\ndocs_dir: ./docs/plan/\ncustom: yes\n";
        fs::write(repository.path().join(CONFIG_PATH), original).unwrap();
        let report = init(repository.path(), &opts(Some("docs/plan/"))).unwrap();
        assert_eq!(report.docs_dir, "docs/plan");
        assert_eq!(report.project_id, "existing");
        assert!(report.reused_config);
        assert_eq!(
            fs::read_to_string(repository.path().join(CONFIG_PATH)).unwrap(),
            original
        );
        let error = init(repository.path(), &opts(Some("docs/other"))).unwrap_err();
        assert!(error.contains("docs/plan"), "{error}");
    }

    #[test]
    fn invalid_existing_docs_dir_is_rejected_without_writes() {
        for invalid in [
            "../outside",
            "/outside",
            ".GIT/objects",
            ".ToWeRmD/x",
            "C:/outside",
            "a/../../b",
            "a\nb",
        ] {
            let repository = git_repo();
            fs::create_dir(repository.path().join(".towermd")).unwrap();
            let original = Config::new("existing", invalid).unwrap().to_yaml();
            fs::write(repository.path().join(CONFIG_PATH), &original).unwrap();
            assert!(init(repository.path(), &opts(None)).is_err(), "{invalid:?}");
            assert_eq!(
                fs::read_to_string(repository.path().join(CONFIG_PATH)).unwrap(),
                original
            );
            assert!(!repository.path().join(AGENTS_PATH).exists());
            assert!(!repository.path().join("docs").exists());
            assert_eq!(
                fs::read_dir(repository.path().join(".towermd"))
                    .unwrap()
                    .count(),
                1
            );
        }
    }

    #[test]
    fn invalid_request_is_rejected_before_creating_files() {
        let repository = git_repo();
        assert!(init(repository.path(), &opts(Some("../outside"))).is_err());
        assert!(!repository.path().join(".towermd").exists());
    }

    #[test]
    fn docs_dir_cannot_occupy_root_agents_path() {
        for docs_dir in ["AGENTS.md", "AGENTS.md/nested", "agents.MD/nested"] {
            let repository = git_repo();
            assert!(
                init(repository.path(), &opts(Some(docs_dir))).is_err(),
                "{docs_dir}"
            );
            assert!(!repository.path().join("AGENTS.md").exists());
            assert!(!repository.path().join(CONFIG_PATH).exists());
        }
    }

    #[test]
    fn runs_from_subdirectory_write_at_toplevel() {
        let repository = git_repo();
        let subdirectory = repository.path().join("src/deep");
        fs::create_dir_all(&subdirectory).unwrap();
        let report = init(&subdirectory, &opts(None)).unwrap();
        assert_eq!(
            report.root.canonicalize().unwrap(),
            repository.path().canonicalize().unwrap()
        );
        assert!(repository.path().join(CONFIG_PATH).is_file());
        assert!(!subdirectory.join(".towermd").exists());
    }

    #[test]
    fn warns_about_protected_names_and_leaves_existing_directories_alone() {
        let repository = git_repo();
        fs::create_dir_all(repository.path().join("docs/specs")).unwrap();
        fs::write(repository.path().join("docs/specs/old.md"), "旧规格\n").unwrap();
        fs::write(repository.path().join("docs/tower.canvas"), "canvas\n").unwrap();
        let report = init(repository.path(), &opts(Some("docs"))).unwrap();
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("specs") && warning.contains("tower.canvas")));
        assert_eq!(
            fs::read_to_string(repository.path().join("docs/specs/old.md")).unwrap(),
            "旧规格\n"
        );
        assert!(!repository.path().join("docs/specs/.gitkeep").exists());
    }

    #[test]
    fn root_agents_md_is_never_modified() {
        let repository = git_repo();
        fs::write(repository.path().join("AGENTS.md"), "# 我的规矩\n").unwrap();
        let report = init(repository.path(), &opts(None)).unwrap();
        assert_eq!(
            fs::read_to_string(repository.path().join("AGENTS.md")).unwrap(),
            "# 我的规矩\n"
        );
        assert!(report.hints.iter().any(|hint| hint.contains(AGENTS_PATH)));
        fs::write(
            repository.path().join("AGENTS.md"),
            "见 .towermd/AGENTS.md\n",
        )
        .unwrap();
        assert!(init(repository.path(), &opts(None))
            .unwrap()
            .hints
            .is_empty());
    }

    #[test]
    fn edited_towermd_agents_md_is_not_overwritten() {
        let repository = git_repo();
        init(repository.path(), &opts(None)).unwrap();
        fs::write(repository.path().join(AGENTS_PATH), "人改过\n").unwrap();
        let report = init(repository.path(), &opts(None)).unwrap();
        assert_eq!(
            fs::read_to_string(repository.path().join(AGENTS_PATH)).unwrap(),
            "人改过\n"
        );
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains(AGENTS_PATH)));
    }

    #[test]
    fn file_in_directory_position_does_not_create_config() {
        for blocked in [
            "docs",
            "docs/tower",
            "docs/tower/intents",
            "docs/tower/decisions",
            ".towermd",
        ] {
            let repository = git_repo();
            let path = repository.path().join(blocked);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "keep\n").unwrap();
            assert!(init(repository.path(), &opts(None)).is_err(), "{blocked}");
            assert!(!repository.path().join(CONFIG_PATH).exists());
            assert_eq!(fs::read_to_string(path).unwrap(), "keep\n");
        }
    }

    #[test]
    fn directory_in_file_position_is_rejected_before_writes() {
        for blocked in [CONFIG_PATH, AGENTS_PATH, "AGENTS.md"] {
            let repository = git_repo();
            fs::create_dir_all(repository.path().join(blocked)).unwrap();
            assert!(init(repository.path(), &opts(None)).is_err(), "{blocked}");
            assert!(!repository.path().join("docs").exists());
            if blocked != CONFIG_PATH {
                assert!(!repository.path().join(CONFIG_PATH).exists());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn parent_and_leaf_symlinks_are_rejected_without_touching_external_files() {
        use std::os::unix::fs::symlink;

        for linked in [
            ".towermd",
            CONFIG_PATH,
            AGENTS_PATH,
            "AGENTS.md",
            "docs",
            "docs/tower",
            "docs/tower/intents",
        ] {
            for dangling in [false, true] {
                let repository = git_repo();
                let external = tempfile::tempdir().unwrap();
                let target = external.path().join("target");
                if !dangling {
                    if linked.ends_with(".md") || linked == CONFIG_PATH {
                        fs::write(&target, "external content\n").unwrap();
                    } else {
                        fs::create_dir(&target).unwrap();
                        fs::write(target.join("sentinel"), "external content\n").unwrap();
                    }
                }
                let link = repository.path().join(linked);
                fs::create_dir_all(link.parent().unwrap()).unwrap();
                symlink(&target, &link).unwrap();
                assert!(
                    init(repository.path(), &opts(None)).is_err(),
                    "{linked}, dangling={dangling}"
                );
                assert!(fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink());
                if dangling {
                    assert!(!target.exists());
                } else if target.is_dir() {
                    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
                    assert_eq!(
                        fs::read_to_string(target.join("sentinel")).unwrap(),
                        "external content\n"
                    );
                } else {
                    assert_eq!(fs::read_to_string(&target).unwrap(), "external content\n");
                }
                if linked != CONFIG_PATH {
                    assert!(!repository.path().join(CONFIG_PATH).exists());
                }
            }
        }
    }

    #[test]
    fn exclusive_creation_does_not_overwrite_or_remove_existing_content() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("human.md"), "human\n").unwrap();
        let mut created = Vec::new();
        assert!(write_new(directory.path(), "human.md", "replacement", &mut created).is_err());
        assert_eq!(
            fs::read_to_string(directory.path().join("human.md")).unwrap(),
            "human\n"
        );
        assert!(created.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn partial_write_failure_removes_only_its_new_file() {
        if let Some(root) = std::env::var_os("TOWERMD_TEST_WRITE_FAILURE") {
            let root = PathBuf::from(root);
            let mut created = Vec::new();
            let result = write_new(&root, "partial.md", &"x".repeat(65536), &mut created);
            assert!(result.unwrap_err().contains("无法写入 partial.md"));
            assert!(!root.join("partial.md").exists());
            assert!(created.is_empty());
            assert_eq!(fs::read_to_string(root.join("human.md")).unwrap(), "keep\n");
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("human.md"), "keep\n").unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", "trap '' XFSZ; ulimit -f 1; exec \"$1\" --exact init::tests::partial_write_failure_removes_only_its_new_file --nocapture", "write-failure-test"])
            .arg(std::env::current_exe().unwrap())
            .env("TOWERMD_TEST_WRITE_FAILURE", temporary.path())
            .output().unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    fn concurrent_init_keeps_one_project_id() {
        let repository = git_repo();
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let root = repository.path().to_path_buf();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    init(&root, &opts(None))
                })
            })
            .collect();
        let reports: Vec<_> = workers
            .into_iter()
            .filter_map(|worker| worker.join().unwrap().ok())
            .collect();
        let config = Config::load(repository.path()).unwrap().unwrap();
        for report in reports {
            assert_eq!(report.project_id, config.project_id());
        }
        let before = fs::read(repository.path().join(CONFIG_PATH)).unwrap();
        assert_eq!(
            init(repository.path(), &opts(None)).unwrap().project_id,
            config.project_id()
        );
        assert_eq!(
            fs::read(repository.path().join(CONFIG_PATH)).unwrap(),
            before
        );
    }

    #[test]
    fn linked_worktree_and_detached_head_only_change_target_files() {
        let repository = git_repo();
        fixture_git(
            repository.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        );
        let checkout = tempfile::tempdir().unwrap();
        let linked = checkout.path().join("linked ");
        fixture_git(
            repository.path(),
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        fs::write(linked.join("staged.txt"), "staged\n").unwrap();
        fixture_git(&linked, &["add", "staged.txt"]);
        let head = repo::git(&linked, &["rev-parse", "HEAD"]).unwrap();
        let index_path = repo::git(&linked, &["rev-parse", "--git-path", "index"]).unwrap();
        let index = fs::read(linked.join(&index_path)).unwrap();
        let primary_index = fs::read(repository.path().join(".git/index")).ok();
        let report = init(&linked, &opts(None)).unwrap();
        assert_eq!(report.root, linked.canonicalize().unwrap());
        assert!(linked.join(CONFIG_PATH).is_file());
        assert!(!repository.path().join(".towermd").exists());
        assert!(!repository.path().join("docs").exists());
        assert_eq!(repo::git(&linked, &["rev-parse", "HEAD"]).unwrap(), head);
        assert!(repo::git(&linked, &["symbolic-ref", "-q", "HEAD"]).is_err());
        assert_eq!(fs::read(linked.join(index_path)).unwrap(), index);
        assert_eq!(
            fs::read(repository.path().join(".git/index")).ok(),
            primary_index
        );
    }

    #[test]
    fn does_not_commit_or_create_an_index() {
        let repository = git_repo();
        init(repository.path(), &opts(None)).unwrap();
        assert!(repo::git(repository.path(), &["rev-parse", "--verify", "HEAD"]).is_err());
        assert!(!repository.path().join(".git/index").exists());
    }

    #[test]
    fn outside_git_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let error = init(directory.path(), &opts(None)).unwrap_err();
        assert!(error.contains("不在 git 仓库中"), "{error}");
        assert!(!directory.path().join(".towermd").exists());
    }

    #[test]
    fn docs_dir_validation() {
        assert_eq!(normalize_docs_dir("./docs/tower/").unwrap(), "docs/tower");
        assert_eq!(normalize_docs_dir("docs/计划").unwrap(), "docs/计划");
        for invalid in [
            "",
            ".",
            "/abs",
            "../up",
            "a/../b",
            ".towermd/x",
            ".git",
            ".GIT/x",
            ".TowerMD/x",
            "a//b",
            "a\\b",
            "C:/outside",
            "c:relative",
            "a/C:/outside",
            "a\0b",
            "a\nb",
            "\tdocs",
            "docs\r",
            "a\u{7f}b",
        ] {
            assert!(normalize_docs_dir(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn agents_template_distinguishes_layers_and_current_capabilities() {
        let rendered = render_agents_md("docs/custom");
        assert!(rendered.contains("L0–L2"));
        assert!(rendered.contains("L3"));
        assert!(rendered.contains("快照专用"));
        assert!(rendered.contains("`status` 与 `task add|claim|show|release`"));
        assert!(rendered.contains("本用户守护进程访问持久化本地 hub"));
        assert!(rendered.contains("尚未提供"));
        assert!(!rendered.contains("凡是触及上述路径的 diff 都会被拒绝"));
        assert!(rendered.contains("docs/custom/state/"));
    }
}
