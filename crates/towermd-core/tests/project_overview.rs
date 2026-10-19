use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use towermd_core::{config::Config, project_overview};

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

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-qb", "main"]);
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, text).unwrap();
}

fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
}

fn bytes(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn walk(root: &Path, current: &Path, output: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                walk(root, &path, output);
            } else if metadata.file_type().is_symlink() {
                output.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read_link(&path)
                        .unwrap()
                        .to_string_lossy()
                        .as_bytes()
                        .to_vec(),
                );
            } else {
                output.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut output = BTreeMap::new();
    walk(root, root, &mut output);
    output
}

#[test]
fn ordinary_repository_uses_fixed_head_paths_without_writes_or_symlink_reads() {
    let dir = repository();
    write(dir.path(), "README.md", "committed root");
    write(dir.path(), "src/nested/main.rs", "fn main() {}");
    write(dir.path(), "中文 空格/原文.md", "committed document");
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "secret.md", "MUST NOT READ OUTSIDE");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        outside.path().join("secret.md"),
        dir.path().join("src/external.md"),
    )
    .unwrap();
    commit(dir.path());
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    write(dir.path(), "src/dirty-untracked.rs", "not committed");
    write(dir.path(), "src/nested/main.rs", "worktree content");
    fs::remove_file(dir.path().join("中文 空格/原文.md")).unwrap();
    let before = bytes(dir.path());
    let overview = project_overview::load(dir.path()).unwrap();
    assert_eq!(overview.source, "repository");
    assert_eq!(overview.revision, head);
    assert_eq!(overview.nodes.len(), 3);
    let root = &overview.nodes[0];
    assert!(root.text.contains("README.md"));
    assert!(root.text.contains("src/nested/main.rs"));
    assert!(root.text.contains("中文 空格/原文.md"));
    assert!(!root.text.contains("dirty-untracked"));
    assert!(!root.text.contains("MUST NOT READ OUTSIDE"));
    #[cfg(unix)]
    assert!(root.text.contains("src/external.md"));
    assert!(overview.nodes.iter().all(|node| node.status == "draft"));
    assert_eq!(overview.edges.len(), 2);
    assert_eq!(bytes(dir.path()), before);
    assert!(!dir.path().join(".towermd").exists());
    let again = project_overview::load(dir.path()).unwrap();
    assert_eq!(
        serde_json::to_value(overview).unwrap(),
        serde_json::to_value(again).unwrap()
    );
}

#[test]
fn unborn_repository_has_an_explicit_empty_overview_and_creates_nothing() {
    let dir = repository();
    write(dir.path(), "untracked.md", "not a commit");
    let before = bytes(dir.path());
    let overview = project_overview::load(dir.path()).unwrap();
    assert_eq!(overview.source, "repository");
    assert_eq!(overview.revision, "");
    assert!(overview.nodes.is_empty());
    assert!(overview.edges.is_empty());
    assert!(overview
        .diagnostics
        .iter()
        .any(|text| text.contains("尚无提交")));
    assert_eq!(bytes(dir.path()), before);
}

fn configured(root: &Path) {
    write(
        root,
        ".towermd/config.yaml",
        &Config::new("project", "docs/tower").unwrap().to_yaml(),
    );
    write(
        root,
        "docs/tower/arch/A.md",
        "---\ntype: Module\nid: A\ntitle: 权威模块\n---\nCommitted body\n",
    );
}

#[test]
fn authoritative_canvas_preserves_geometry_and_reads_committed_plain_text() {
    let dir = repository();
    configured(dir.path());
    write(
        dir.path(),
        "README.md",
        "<script>plain text only</script>\n",
    );
    let canvas = json!({
        "nodes": [
            {"id":"module","type":"file","file":"docs/tower/arch/A.md","x":-123,"y":45,"width":333,"height":177,"towermd":{"obj":"A"}},
            {"id":"readme","type":"file","file":"README.md","x":500,"y":70,"width":210,"height":110},
            {"id":"link","type":"link","url":"https://example.invalid/do-not-open","x":800,"y":70,"width":210,"height":110}
        ],
        "edges":[{"id":"e","fromNode":"module","toNode":"readme"}]
    });
    write(dir.path(), "docs/tower/tower.canvas", &canvas.to_string());
    commit(dir.path());
    let authority = git(dir.path(), &["rev-parse", "main"]);
    git(dir.path(), &["switch", "-qc", "task"]);
    write(dir.path(), "README.md", "DIRTY CONTENT");
    write(
        dir.path(),
        "docs/tower/tower.canvas",
        "dirty invalid canvas",
    );
    let before = bytes(dir.path());
    let overview = project_overview::load(dir.path()).unwrap();
    assert_eq!(overview.source, "baseline");
    assert_eq!(overview.revision, authority);
    let module = &overview.nodes[0];
    assert_eq!(
        (module.x, module.y, module.width, module.height),
        (-123.0, 45.0, 333.0, 177.0)
    );
    assert_eq!(module.title, "权威模块");
    assert_eq!(module.status, "bound");
    assert_eq!(module.obj.as_deref(), Some("A"));
    assert_eq!(module.object_type.as_deref(), Some("Module"));
    assert!(module.text.contains("Committed body"));
    assert_eq!(overview.nodes[1].text, "<script>plain text only</script>\n");
    assert_eq!(overview.nodes[1].status, "reference");
    assert_eq!(overview.nodes[2].kind, "link");
    assert_eq!(
        overview.nodes[2].text,
        "https://example.invalid/do-not-open"
    );
    assert_eq!(overview.edges[0].from, "module");
    assert_eq!(overview.edges[0].to, "readme");
    assert_eq!(bytes(dir.path()), before);
}

#[test]
fn missing_canvas_previews_head_without_materializing_candidates() {
    let dir = repository();
    configured(dir.path());
    commit(dir.path());
    git(dir.path(), &["switch", "-qc", "task"]);
    write(dir.path(), "src/current.rs", "current branch");
    commit(dir.path());
    let before = bytes(dir.path());
    let overview = project_overview::load(dir.path()).unwrap();
    assert_eq!(overview.source, "repository");
    assert_eq!(overview.revision, git(dir.path(), &["rev-parse", "HEAD"]));
    assert_eq!(
        overview.authority_revision.as_deref(),
        Some(git(dir.path(), &["rev-parse", "main"]).as_str())
    );
    assert_ne!(
        overview.authority_revision.as_deref(),
        Some(overview.revision.as_str())
    );
    assert!(overview.nodes[0].text.contains("src/current.rs"));
    assert!(!overview.nodes[0].text.contains(".towermd/"));
    assert!(overview
        .diagnostics
        .iter()
        .any(|text| text.contains("权威 main 尚无画布")));
    assert_eq!(bytes(dir.path()), before);
}

#[test]
fn damaged_authority_is_reported_instead_of_replaced_by_a_repository_graph() {
    let dir = repository();
    configured(dir.path());
    write(dir.path(), "docs/tower/tower.canvas", "{broken");
    commit(dir.path());
    let before = bytes(dir.path());
    assert!(project_overview::load(dir.path())
        .unwrap_err()
        .contains("权威画布"));
    assert_eq!(bytes(dir.path()), before);
    write(dir.path(), ".towermd/config.yaml", "project_id: [");
    commit(dir.path());
    assert!(project_overview::load(dir.path())
        .unwrap_err()
        .contains("合法 YAML"));
}
