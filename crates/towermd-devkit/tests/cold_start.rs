use std::{fs, path::Path, process::Command};

use towermd_core::{canvas::CanvasDocument, catalog::Catalog, config::{Config, CONFIG_PATH}, okf::Document};
use towermd_devkit::{cold_start::{canvas_json, scan}};

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
            "user.name=Scan Test",
            "-c",
            "user.email=scan@example.invalid",
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
fn write(root: &Path, path: &str, body: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}
fn commit(root: &Path) -> String {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root, &["rev-parse", "HEAD"])
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-qb", "main"]);
    write(
        dir.path(),
        CONFIG_PATH,
        &Config::new("scan-test", "docs/tower").unwrap().to_yaml(),
    );
    dir
}

#[test]
fn fixed_commit_inventory_ignores_dirty_files_and_resolves_all_candidate_objects() {
    let dir = fixture();
    let root = dir.path();
    for (path, body) in [
        ("src/client/app.ts", "export const n = 1;"),
        ("src/server/app.rs", "fn main() {}"),
        ("src/storage/db.py", "pass"),
        (
            "src/client/package.json",
            r#"{"packageManager":"pnpm@11.7.0","scripts":{"test":"vitest run","check":"tsc"}}"#,
        ),
        (
            "src/server/Cargo.toml",
            "[package]\nname = \"server\"\nversion = \"0.1.0\"",
        ),
        ("node_modules/dependency/index.ts", "dependency"),
        ("target/generated.rs", "generated"),
        ("docs/tower/existing.ts", "protected document"),
        ("README.md", "# Project"),
    ] {
        write(root, path, body);
    }
    let revision = commit(root);
    let catalog = Catalog::load(root).unwrap();
    write(root, "src/client/package.json", "invalid dirty manifest");
    write(root, "src/new/dirty.ts", "not in committed inventory");
    git(root, &["add", "."]);
    let before = (
        git(root, &["status", "--porcelain"]),
        fs::read(root.join(".git/index")).unwrap(),
    );
    let report = scan(&catalog, "scan-fixture").unwrap();
    assert_eq!(report.revision, revision);
    assert_eq!(report.reference, "refs/heads/main");
    assert_eq!(report.tree, git(root, &["rev-parse", "HEAD^{tree}"]));
    assert_eq!(report.source_file_count, 3);
    assert_eq!(report.assigned_file_count, 3);
    assert_eq!(report.coverage, 1.0);
    assert!(report.unassigned.is_empty());
    assert_eq!(report.modules.len(), 3);
    assert!(report
        .commands
        .iter()
        .any(|command| command.command == "pnpm run test" && command.directory == "src/client"));
    let storage = report
        .modules
        .iter()
        .find(|module| module.files.contains(&"src/storage/db.py".into()))
        .unwrap();
    assert!(storage.command_ids.is_empty());
    assert!(storage
        .suggested_task
        .as_ref()
        .unwrap()
        .contains("补充测试"));
    let canvas = CanvasDocument::decode(&canvas_json(&report).unwrap()).unwrap();
    assert!(
        canvas.diagnostics().is_empty(),
        "{:?}",
        canvas.diagnostics()
    );
    assert_eq!(canvas.nodes().len(), 4);
    let project = report.view.nodes.iter().find(|node| node.region.as_deref() == Some("project")).unwrap();
    assert_eq!(project.kind, "group");
    assert!(project.file.is_none() && project.object_id.is_none() && project.object_type.is_none());
    assert_eq!((project.x, project.y, project.width, project.height), (-380, 0, 320, 240));
    let raw = &canvas.nodes().iter().find(|node| node.raw["id"] == project.id).unwrap().raw;
    assert_eq!(raw["type"], "group");
    assert_eq!(raw["towermd"]["region"], "project");
    assert!(raw.get("file").is_none());
    for node in report.view.nodes.iter().filter(|node| node.object_type.as_deref() == Some("Module")) {
        let document = Document::parse(&node.content).unwrap();
        assert_eq!(document.doc_type(), Some("Module"));
        assert_eq!(document.get_str("status"), Some("draft"));
        assert_eq!(document.get_str("verification"), Some("not_run"));
        assert_eq!(document.get_str("id"), node.object_id.as_deref());
        assert_eq!(
            node.file.as_deref(),
            Some(format!("arch/{}.md", node.id).as_str())
        );
        assert!(!node.content.contains("verified:"));
    }
    assert!(report
        .diagnostics
        .iter()
        .any(|message| message.contains("S5 尚未执行")));
    assert_eq!(
        before,
        (
            git(root, &["status", "--porcelain"]),
            fs::read(root.join(".git/index")).unwrap()
        )
    );
    assert!(
        !root.join(".git/towermd").exists(),
        "纯扫描不可写入缓存或 hub"
    );
    let again = scan(&catalog, "scan-fixture").unwrap();
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        serde_json::to_value(again).unwrap()
    );
}

#[test]
fn committed_cochange_changes_candidate_boundaries_and_edges_have_history() {
    let dir = fixture();
    let root = dir.path();
    for name in ["alpha", "beta", "gamma", "delta"] {
        write(root, &format!("{name}/main.rs"), "fn initial() {}\n");
    }
    commit(root);
    for iteration in 0..2 {
        write(
            root,
            "alpha/main.rs",
            &format!("fn alpha_{iteration}() {{}}\n"),
        );
        write(
            root,
            "beta/main.rs",
            &format!("fn beta_{iteration}() {{}}\n"),
        );
        commit(root);
    }
    write(root, "gamma/main.rs", "fn gamma_change() {}\n");
    write(root, "delta/main.rs", "fn delta_change() {}\n");
    commit(root);
    let report = scan(&Catalog::load(root).unwrap(), "history").unwrap();
    assert_eq!(report.modules.len(), 3);
    assert!(report
        .modules
        .iter()
        .any(|module| module.files.contains(&"alpha/main.rs".into())
            && module.files.contains(&"beta/main.rs".into())));
    assert!(report
        .diagnostics
        .iter()
        .any(|message| message.contains("历史修正")));
    let gamma = report
        .modules
        .iter()
        .find(|module| module.files.contains(&"gamma/main.rs".into()))
        .unwrap();
    let delta = report
        .modules
        .iter()
        .find(|module| module.files.contains(&"delta/main.rs".into()))
        .unwrap();
    assert!(report
        .couplings
        .iter()
        .any(
            |coupling| ((coupling.from == gamma.id && coupling.to == delta.id)
                || (coupling.from == delta.id && coupling.to == gamma.id))
                && coupling.commits == 2
        ));
    assert_eq!(
        report
            .hotspots
            .iter()
            .find(|hotspot| hotspot.path == "alpha/main.rs")
            .unwrap()
            .commits,
        3
    );
    assert!(report.view.edges.iter().all(|edge| edge
        .label
        .as_ref()
        .unwrap()
        .starts_with("共同修改")));
}

#[test]
fn impossible_module_count_is_diagnosed_and_root_commands_are_shared() {
    let dir = fixture();
    let root = dir.path();
    write(root, "main.rs", "fn main() {}\n");
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"example\"\nversion = \"0.1.0\"\n",
    );
    commit(root);
    let report = scan(&Catalog::load(root).unwrap(), "small").unwrap();
    assert_eq!(report.modules.len(), 1);
    assert!(report
        .diagnostics
        .iter()
        .any(|message| message.contains("无法形成每层 3–9")));
    assert!(report.modules[0].suggested_task.is_none());
    assert_eq!(report.commands.len(), 1);
    assert_eq!(
        report.commands[0].modules,
        vec![report.modules[0].id.clone()]
    );
}

#[test]
fn empty_source_repository_does_not_claim_complete_coverage() {
    let dir = fixture();
    let root = dir.path();
    write(root, "README.md", "# Empty");
    commit(root);
    let report = scan(&Catalog::load(root).unwrap(), "empty").unwrap();
    assert_eq!(report.source_file_count, 0);
    assert_eq!(report.coverage, 0.0);
    assert!(report.modules.is_empty());
    assert!(report.commands.is_empty());
}

#[test]
fn origin_main_not_local_branch_supplies_inventory_and_manifest() {
    let dir = fixture();
    let root = dir.path();
    write(root, "committed/main.rs", "fn original() {}\n");
    let original = commit(root);
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repository.git",
        ],
    );
    git(root, &["update-ref", "refs/remotes/origin/main", &original]);
    write(root, "local/only.ts", "local version");
    commit(root);
    let report = scan(&Catalog::load(root).unwrap(), "origin").unwrap();
    assert_eq!(report.reference, "refs/remotes/origin/main");
    assert_eq!(report.revision, original);
    assert_eq!(report.source_file_count, 1);
    assert_eq!(report.modules[0].files, vec!["committed/main.rs"]);
}

#[test]
fn root_verification_command_is_deduplicated_and_only_used_as_fallback() {
    let dir = fixture();
    let root = dir.path();
    for name in ["client", "engine", "storage"] {
        write(root, &format!("{name}/main.rs"), "fn main() {}\n");
    }
    write(root, "Cargo.toml", "[workspace]\nmembers = []\n");
    write(
        root,
        "client/package.json",
        r#"{"scripts":{"test":"client-only"}}"#,
    );
    commit(root);
    let report = scan(&Catalog::load(root).unwrap(), "shared").unwrap();
    let shared = report
        .commands
        .iter()
        .find(|command| command.directory.is_empty())
        .unwrap();
    let client = report
        .commands
        .iter()
        .find(|command| command.directory == "client")
        .unwrap();
    assert_eq!(shared.command, "cargo test");
    assert_eq!(shared.modules.len(), 2);
    assert_eq!(client.modules.len(), 1);
    assert!(!shared.modules.contains(&client.modules[0]));
    assert!(report
        .modules
        .iter()
        .all(|module| module.command_ids.len() == 1));
}

#[cfg(unix)]
#[test]
fn tracked_symbolic_links_are_excluded_without_following_them() {
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "secret.ts", "must not be scanned");
    symlink(outside.path().join("secret.ts"), root.join("link.ts")).unwrap();
    write(root, "src/main.rs", "fn main() {}\n");
    commit(root);
    let report = scan(&Catalog::load(root).unwrap(), "symlink").unwrap();
    assert_eq!(report.source_file_count, 1);
    assert!(!report
        .modules
        .iter()
        .any(|module| module.files.contains(&"link.ts".into())));
    assert!(report
        .diagnostics
        .iter()
        .any(|message| message.contains("link.ts")));
}

#[test]
fn three_module_grid_uses_two_columns_and_serializes_matching_edge_ports() {
    let dir = fixture();
    let root = dir.path();
    for name in ["alpha", "beta", "gamma"] {
        write(root, &format!("{name}/main.rs"), "fn main() {}\n");
    }
    commit(root);
    let report = scan(&Catalog::load(root).unwrap(), "layout").unwrap();
    let nodes: Vec<_> = report.view.nodes.iter().filter(|node| node.object_type.as_deref() == Some("Module")).collect();
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0].y, nodes[1].y);
    assert!(nodes[1].x > nodes[0].x);
    assert_eq!(nodes[0].x, nodes[2].x);
    assert!(nodes[2].y > nodes[0].y);
    let serialized: serde_json::Value =
        serde_json::from_str(&canvas_json(&report).unwrap()).unwrap();
    assert_eq!(report.view.edges.len(), 3);
    for (index, edge) in report.view.edges.iter().enumerate() {
        let from = nodes.iter().find(|node| node.id == edge.from_node).unwrap();
        let to = nodes.iter().find(|node| node.id == edge.to_node).unwrap();
        let sides = if from.y == to.y {
            ("right", "left")
        } else {
            ("bottom", "top")
        };
        assert_eq!(edge.from_side.as_deref(), Some(sides.0));
        assert_eq!(edge.to_side.as_deref(), Some(sides.1));
        assert_eq!(serialized["edges"][index]["fromSide"], sides.0);
        assert_eq!(serialized["edges"][index]["toSide"], sides.1);
    }
    for node in nodes {
        let document = Document::parse(&node.content).unwrap();
        assert!(document
            .body()
            .starts_with(&format!("1 个源码文件 · {}", node.title)));
        assert!(document.body().contains("未发现验证命令"));
        assert!(!document.body().contains("无可识别测试"));
    }
}

#[test]
fn merged_native_and_frontend_module_keeps_root_commands_for_uncovered_files() {
    let dir = fixture();
    let root = dir.path();
    for (path, content) in [
        ("src/App.svelte", "<p>first</p>\n"),
        ("src-tauri/main.rs", "fn first() {}\n"),
        ("tools/script.py", "pass\n"),
        ("tests/check.rs", "fn check() {}\n"),
        (
            "src-tauri/Cargo.toml",
            "[package]\nname = \"native\"\nversion = \"0.1.0\"\n",
        ),
        (
            "package.json",
            r#"{"packageManager":"pnpm@11.7.0","scripts":{"test":"vitest run","check":"svelte-check"}}"#,
        ),
    ] {
        write(root, path, content);
    }
    commit(root);
    for iteration in 0..2 {
        write(root, "src/App.svelte", &format!("<p>{iteration}</p>\n"));
        write(
            root,
            "src-tauri/main.rs",
            &format!("fn change_{iteration}() {{}}\n"),
        );
        commit(root);
    }
    let report = scan(&Catalog::load(root).unwrap(), "mixed-commands").unwrap();
    let merged = report
        .modules
        .iter()
        .find(|module| {
            module.files.contains(&"src/App.svelte".into())
                && module.files.contains(&"src-tauri/main.rs".into())
        })
        .expect("共同修改应合并前端与原生候选");
    let assigned: Vec<_> = report
        .commands
        .iter()
        .filter(|command| merged.command_ids.contains(&command.id))
        .map(|command| (command.directory.as_str(), command.command.as_str()))
        .collect();
    assert_eq!(assigned.len(), 3);
    assert!(assigned.contains(&("src-tauri", "cargo test")));
    assert!(assigned.contains(&("", "pnpm run test")));
    assert!(assigned.contains(&("", "pnpm run check")));
    let node = report
        .view
        .nodes
        .iter()
        .find(|node| node.id == merged.id)
        .unwrap();
    let document = Document::parse(&node.content).unwrap();
    assert_eq!(document.get_str("verification"), Some("not_run"));
    assert_eq!(
        document
            .get("verification_commands")
            .unwrap()
            .as_sequence()
            .unwrap()
            .len(),
        3
    );
}
