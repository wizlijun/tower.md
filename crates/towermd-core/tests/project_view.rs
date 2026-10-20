use std::{fs, path::Path, process::Command};

use serde_json::json;
use towermd_core::{
    config::{Config, CONFIG_PATH},
    project_view::load,
};

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let output = command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=View Test",
            "-c",
            "user.email=view@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn commit(root: &Path) -> String {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root, &["rev-parse", "HEAD"])
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    write(
        root,
        CONFIG_PATH,
        &Config::new("view-test", "docs/tower").unwrap().to_yaml(),
    );
    write(
        root,
        "docs/tower/arch/module.md",
        "---\ntype: Module\nid: module\ntitle: 已提交模块\n---\n原始正文\n",
    );
    write(
        root,
        "docs/tower/arch/unplaced.md",
        "---\ntype: Module\nid: unplaced\ntitle: 未摆放\n---\n独立模块\n",
    );
    let canvas = json!({
        "nodes": [
            {"id":"module","type":"file","file":"docs/tower/arch/module.md","towermd":{"obj":"module"},"x":-250,"y":20,"width":300,"height":180},
            {"id":"idea","type":"text","text":"# 想法\n普通文本 <script>alert(1)</script>","x":200,"y":20,"width":260,"height":180},
            {"id":"link","type":"link","url":"https://example.invalid","x":200,"y":300,"width":260,"height":100},
            {"id":"group","type":"group","label":"概念","x":-300,"y":-50,"width":800,"height":500}
        ],
        "edges":[
            {"id":"a","fromNode":"module","toNode":"idea","label":"解释","fromSide":"bottom","toSide":"top","toEnd":"none"},
            {"id":"b","fromNode":"idea","toNode":"module","label":"反馈","fromEnd":"arrow","color":"#aabbcc"}
        ]
    });
    write(root, "docs/tower/tower.canvas", &canvas.to_string());
    commit(root);
    dir
}

#[test]
fn renders_general_graph_from_one_commit_without_writing_or_reading_dirty_files() {
    let dir = fixture();
    let root = dir.path();
    let sha = git(root, &["rev-parse", "HEAD"]);
    write(root, "docs/tower/tower.canvas", "invalid dirty canvas");
    write(root, "docs/tower/arch/module.md", "untrusted working copy");
    git(root, &["add", "."]);
    let before = (
        git(root, &["status", "--porcelain"]),
        fs::read(root.join(".git/index")).unwrap(),
    );
    let view = load(root).unwrap();
    assert_eq!(view.revision, sha);
    assert_eq!(view.nodes.len(), 4);
    assert_eq!(view.edges.len(), 2);
    assert_eq!(view.nodes[0].x, -250);
    assert_eq!(view.nodes[0].title, "已提交模块");
    assert!(view.nodes[0].content.contains("原始正文"));
    assert_eq!(view.edges[0].from_side.as_deref(), Some("bottom"));
    assert_eq!(view.edges[0].to_end.as_deref(), Some("none"));
    assert_eq!(view.unplaced.len(), 1);
    assert_eq!(view.unplaced[0].object_id.as_deref(), Some("unplaced"));
    assert_eq!(view.edges[1].to_node, "module");
    assert_eq!(
        before,
        (
            git(root, &["status", "--porcelain"]),
            fs::read(root.join(".git/index")).unwrap()
        )
    );
    assert_eq!(git(root, &["rev-parse", "HEAD"]), sha);
    assert!(!root.join(".git/towermd").exists());
}

#[test]
fn configured_origin_reads_cached_remote_main_and_never_local_main_or_network() {
    let dir = fixture();
    let root = dir.path();
    let old = git(root, &["rev-parse", "HEAD"]);
    git(
        root,
        &["remote", "add", "origin", "/nonexistent/offline-origin"],
    );
    assert!(load(root).unwrap_err().contains("origin/main"));
    git(root, &["update-ref", "refs/remotes/origin/main", &old]);
    write(
        root,
        "docs/tower/arch/module.md",
        "---\ntype: Module\nid: module\ntitle: local-only\n---\n",
    );
    commit(root);
    let view = load(root).unwrap();
    assert_eq!(view.revision, old);
    assert_eq!(view.reference, "refs/remotes/origin/main");
    assert_eq!(view.nodes[0].title, "已提交模块");
    assert!(!root.join(".git/FETCH_HEAD").exists());
}

#[test]
fn missing_configuration_canvas_and_invalid_canvas_are_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    assert!(load(root).unwrap_err().contains("main"));
    write(root, "README.md", "project");
    commit(root);
    let empty = load(root).unwrap();
    assert!(empty.docs_dir.is_none());
    assert!(empty.nodes.is_empty());
    write(
        root,
        CONFIG_PATH,
        &Config::new("view-test", "docs/tower").unwrap().to_yaml(),
    );
    write(
        root,
        "docs/tower/arch/module.md",
        "---\ntype: Module\nid: module\n---\n",
    );
    commit(root);
    let unplaced = load(root).unwrap();
    assert_eq!(unplaced.unplaced.len(), 1);
    assert!(unplaced
        .diagnostics
        .iter()
        .any(|d| d.contains("tower.canvas")));
    write(root, "docs/tower/tower.canvas", "{ broken");
    commit(root);
    assert!(load(root).is_err());
}

#[test]
fn dangling_unsafe_and_duplicate_entries_do_not_hide_the_valid_graph() {
    let dir = fixture();
    let root = dir.path();
    let path = root.join("docs/tower/tower.canvas");
    let mut canvas: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let nodes = canvas["nodes"].as_array_mut().unwrap();
    for (id, file) in [("unsafe", "../outside"), ("missing", "missing.md")] {
        nodes.push(json!({"id":id,"type":"file","file":file,"x":0,"y":0,"width":200,"height":100}));
    }
    nodes.push(nodes[1].clone());
    let edges = canvas["edges"].as_array_mut().unwrap();
    edges.push(json!({"id":"dup","fromNode":"module","toNode":"link"}));
    edges.push(json!({"id":"dup","fromNode":"module","toNode":"link"}));
    fs::write(path, canvas.to_string()).unwrap();
    commit(root);
    let view = load(root).unwrap();
    assert!(!view.nodes.iter().any(|n| n.id == "idea"));
    assert!(view
        .nodes
        .iter()
        .find(|n| n.id == "unsafe")
        .unwrap()
        .content
        .contains("无法读取"));
    assert!(view
        .nodes
        .iter()
        .find(|n| n.id == "missing")
        .unwrap()
        .title
        .starts_with("悬空引用"));
    assert!(view.edges.is_empty());
    assert!(!view.diagnostics.is_empty());
}

#[cfg(unix)]
#[test]
fn referenced_symlink_is_not_followed() {
    let dir = fixture();
    let root = dir.path();
    fs::remove_file(root.join("docs/tower/arch/module.md")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", root.join("docs/tower/arch/module.md")).unwrap();
    commit(root);
    let view = load(root).unwrap();
    assert!(view.nodes[0].content.starts_with("无法读取引用"));
    assert!(view.nodes[0].object_id.is_none());
}
