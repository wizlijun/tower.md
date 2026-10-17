use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
use towermd_core::{
    canvas::CanvasDocument,
    catalog::Catalog,
    coldstart::{capture_scan_input, prepare_scan, ScanInput, ScanOptions},
    coldstart_draft::{render_draft, DraftBundle},
    config::Config,
    okf::Document,
};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
}
fn fixture(count: usize) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    write(
        root.path(),
        ".towermd/config.yaml",
        &Config::new("project", "docs/tower").unwrap().to_yaml(),
    );
    for index in 0..count {
        write(
            root.path(),
            &format!("模块{index:02}/main.rs"),
            "fn main() {}\n",
        );
    }
    write(root.path(), "docs/tower/user.txt", "keep user document\n");
    commit(root.path());
    root
}
fn scan(root: &Path) -> (ScanInput, Value) {
    let catalog = Catalog::load(root).unwrap();
    let input = capture_scan_input(&catalog, &ScanOptions::default()).unwrap();
    let result = prepare_scan(&input).unwrap();
    (input, result)
}
fn value(bundle: &DraftBundle, path: &str) -> Value {
    serde_json::from_slice(&bundle.files[path]).unwrap()
}
fn materialize(bundle: &DraftBundle, target: &Path) {
    for (path, bytes) in &bundle.files {
        assert!(!path.starts_with('/') && !path.split('/').any(|part| part == ".." || part == "."));
        let path = target.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

#[test]
fn draft_file_nodes_resolve_real_okf_modules_and_preserve_integer_groups() {
    let root = fixture(25);
    let (input, result) = scan(root.path());
    let bundle = render_draft(&input, &result).unwrap();
    let out = tempfile::tempdir().unwrap();
    materialize(&bundle, out.path());
    let canvas =
        CanvasDocument::decode(&fs::read_to_string(out.path().join("tower.canvas")).unwrap())
            .unwrap();
    assert!(canvas.diagnostics().is_empty());
    let mut files = 0;
    let mut groups = 0;
    for node in canvas.nodes() {
        for key in ["x", "y", "width", "height"] {
            assert!(node.raw[key].is_i64() || node.raw[key].is_u64());
        }
        if node.raw["type"] == "group" {
            groups += 1;
            continue;
        }
        assert_eq!(node.raw["type"], "file");
        files += 1;
        let id = node.raw["towermd"]["obj"].as_str().unwrap();
        let path = node.raw["file"].as_str().unwrap();
        assert_eq!(path, format!("arch/{id}.md"));
        let document =
            Document::parse(&fs::read_to_string(out.path().join(path)).unwrap()).unwrap();
        assert_eq!(document.doc_type(), Some("Module"));
        assert_eq!(document.get_str("id"), Some(id));
        assert_eq!(document.get_str("layer"), Some("L1"));
        assert_eq!(document.get_str("status"), Some("draft"));
        assert_eq!(
            document.get("generated").unwrap()["by"].as_str(),
            Some("process:towermd")
        );
        assert!(document
            .get_str("name")
            .is_some_and(|name| !name.is_empty()));
        assert!(document.get("paths").unwrap().is_sequence());
        assert!(document.get("interfaces").unwrap().is_sequence());
        assert!(document.get("verify").unwrap().is_sequence());
        assert_eq!(document.get_str("commit"), Some(input.revision.as_str()));
        assert_eq!(document.get_str("tree"), Some(input.tree.as_str()));
        for key in ["verified", "baseline", "owner", "approved"] {
            assert!(document.get(key).is_none(), "{key}");
        }
    }
    assert_eq!(files, 25);
    assert_eq!(groups, 3);
    assert_eq!(bundle.files.len(), 30);
    let saved = value(&bundle, "result.json");
    assert_eq!(saved["hierarchy"], result["hierarchy"]);
    assert_eq!(saved["stages"]["S5"], "not_run");
    assert_eq!(saved["candidate_canvas"], value(&bundle, "tower.canvas"));
    for module in saved["modules"].as_array().unwrap() {
        assert_eq!(
            module["candidate_path"],
            format!("arch/{}.md", module["id"].as_str().unwrap())
        );
    }
    assert_eq!(value(&bundle, "input.json"), json!(input));
}

#[test]
fn fixed_input_rebuild_ignores_later_main_and_dirty_worktree() {
    let root = fixture(3);
    let (input, result) = scan(root.path());
    let first = render_draft(&input, &result).unwrap();
    let out = tempfile::tempdir().unwrap();
    materialize(&first, out.path());
    fs::remove_dir_all(out.path()).unwrap();
    write(root.path(), "later/new.ts", "export const later = 1;\n");
    commit(root.path());
    write(root.path(), "模块00/main.rs", "dirty uncommitted\n");
    write(root.path(), "untracked.py", "raise RuntimeError\n");
    let status = git(root.path(), &["status", "--porcelain"]);
    let head = git(root.path(), &["rev-parse", "main"]);
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let rebuilt = prepare_scan(&serde_json::from_value(json!(input)).unwrap()).unwrap();
    assert_eq!(
        rebuilt, result,
        "fixed revision must not follow current main"
    );
    assert_eq!(render_draft(&input, &rebuilt).unwrap().files, first.files);
    assert_eq!(git(root.path(), &["status", "--porcelain"]), status);
    assert_eq!(git(root.path(), &["rev-parse", "main"]), head);
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(root.path().join("docs/tower/user.txt")).unwrap(),
        "keep user document\n"
    );
    assert!(!root.path().join("docs/tower/tower.canvas").exists());
    assert!(!input.common_dir.join("towermd/drafts").exists());
}

#[test]
fn mismatched_fixed_input_or_result_is_rejected() {
    let root = fixture(3);
    let (input, result) = scan(root.path());
    for key in ["project_id", "revision", "tree", "algorithm", "options"] {
        let mut mixed = result.clone();
        mixed[key] = if key == "options" {
            json!({"history_limit":1})
        } else {
            json!("wrong")
        };
        assert!(render_draft(&input, &mixed).is_err(), "result {key}");
    }
    for key in [
        "project_id",
        "revision",
        "tree",
        "algorithm",
        "root",
        "common_dir",
        "options",
    ] {
        let mut mixed = json!(input);
        mixed[key] = match key {
            "revision" | "tree" => json!("0".repeat(40)),
            "root" | "common_dir" => json!("/private/tmp/does-not-exist-towermd-draft"),
            "options" => json!({"history_limit":0}),
            _ => json!("wrong"),
        };
        let mixed: ScanInput = serde_json::from_value(mixed).unwrap();
        assert!(prepare_scan(&mixed).is_err(), "input {key}");
    }
}

#[test]
fn renderer_rejects_unsafe_ids_duplicates_missing_or_mismatched_nodes() {
    let root = fixture(3);
    let (input, result) = scan(root.path());
    for id in [
        "../escape",
        "A-x/../../escape",
        "/absolute",
        "A-x\\escape",
        "A-x\0escape",
    ] {
        let mut bad = result.clone();
        bad["modules"][0]["id"] = json!(id);
        assert!(render_draft(&input, &bad).is_err(), "{id:?}");
    }
    let mut bad = result.clone();
    let duplicate = bad["modules"][0].clone();
    bad["modules"].as_array_mut().unwrap().push(duplicate);
    assert!(render_draft(&input, &bad).is_err());
    let mut bad = result.clone();
    bad["candidate_canvas"]["nodes"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(render_draft(&input, &bad).is_err());
    let mut bad = result.clone();
    bad["candidate_canvas"]["nodes"][0]["towermd"]["obj"] = json!("A-unknown");
    assert!(render_draft(&input, &bad).is_err());
}

#[test]
fn draft_keeps_no_test_candidates_and_unassigned_without_executing_commands() {
    let root = fixture(3);
    write(
        root.path(),
        "模块00/package.json",
        r#"{"packageManager":"pnpm@10","scripts":{"test":"touch SHOULD-NOT-EXIST"}}"#,
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("missing-target", root.path().join("loose.py")).unwrap();
    }
    commit(root.path());
    let (input, result) = scan(root.path());
    let bundle = render_draft(&input, &result).unwrap();
    assert_eq!(value(&bundle, "unassigned.json"), result["unassigned"]);
    assert_eq!(
        value(&bundle, "candidate-tasks.json"),
        result["candidate_tasks"]
    );
    assert_eq!(
        value(&bundle, "candidate-tasks.json")
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let document =
        Document::parse(std::str::from_utf8(&bundle.files["arch/A-模块00.md"]).unwrap()).unwrap();
    assert_eq!(
        document.get("verify").unwrap().as_sequence().unwrap()[0].as_str(),
        Some("cd '模块00' && 'pnpm' 'run' 'test'")
    );
    assert_eq!(
        serde_json::to_value(document.get("verification_plan").unwrap()).unwrap(),
        result["validation_plans"][0]
    );
    assert!(!root.path().join("模块00/SHOULD-NOT-EXIST").exists());
    assert_eq!(value(&bundle, "result.json")["stages"]["S5"], "not_run");
}

#[test]
fn rebuilding_a_shallow_scan_does_not_change_after_local_history_is_deepened() {
    let origin = fixture(4);
    for value in [1, 2] {
        for index in [0, 1] {
            write(
                origin.path(),
                &format!("模块{index:02}/main.rs"),
                &format!("const VALUE:u8={value};\n"),
            );
        }
        commit(origin.path());
    }
    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", origin.path().canonicalize().unwrap().display());
    git(
        origin.path(),
        &[
            "clone",
            "--quiet",
            "--depth",
            "1",
            &url,
            clone.path().to_str().unwrap(),
        ],
    );
    assert_eq!(
        git(clone.path(), &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    let (input, result) = scan(clone.path());
    assert_eq!(result["stages"]["S4"], "unavailable");
    git(clone.path(), &["fetch", "--quiet", "--unshallow", "origin"]);
    assert_eq!(
        git(clone.path(), &["rev-parse", "--is-shallow-repository"]),
        "false"
    );
    let rebuilt = prepare_scan(&input).unwrap();
    assert_eq!(
        rebuilt["modules"], result["modules"],
        "deepening must not change the captured module partition"
    );
    assert_eq!(rebuilt, result);
    let full = capture_scan_input(
        &Catalog::load(clone.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    assert!(full.history_available);
    git(
        clone.path(),
        &["fetch", "--quiet", "--depth", "1", "origin"],
    );
    assert_eq!(
        git(clone.path(), &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    assert!(prepare_scan(&full).unwrap_err().contains("history"));
}

#[test]
fn empty_source_draft_keeps_not_applicable_without_fabricated_modules() {
    let root = fixture(0);
    let (input, result) = scan(root.path());
    let bundle = render_draft(&input, &result).unwrap();
    assert_eq!(bundle.files.len(), 5);
    assert_eq!(
        value(&bundle, "result.json")["coverage"]["status"],
        "not_applicable"
    );
    assert_eq!(value(&bundle, "tower.canvas")["nodes"], json!([]));
    assert_eq!(value(&bundle, "candidate-tasks.json"), json!([]));
}
