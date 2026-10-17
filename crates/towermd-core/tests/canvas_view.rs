use serde_json::json;
use std::{fs, path::Path, process::Command};
use towermd_core::{canvas_view::CanvasView, catalog::Catalog, config::Config};

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

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    write(
        root.path(),
        ".towermd/config.yaml",
        &Config::new("project", "docs/tower").unwrap().to_yaml(),
    );
    for id in ["A", "B"] {
        write(
            root.path(),
            &format!("docs/tower/arch/{id}.md"),
            &format!(
                "---\r\ntype: Module\r\nid: {id}\r\npaths: [src/**]\r\n---\r\nBody without newline"
            ),
        );
    }
    write(root.path(), "README.md", "ordinary reference\r\n\r\n");
    root
}

fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
}

fn node(id: &str, file: &str, object: Option<&str>) -> serde_json::Value {
    let mut node = json!({"id":id,"type":"file","file":file,"x":0,"y":0,"width":100,"height":100});
    if let Some(object) = object {
        node["towermd"] = json!({"obj":object});
    }
    node
}

#[test]
fn view_reads_exact_main_bytes_and_reports_bindings_without_relayout() {
    let root = fixture();
    let canvas = format!("{}\r\n", serde_json::to_string(&json!({"nodes":[node("a", "docs/tower/arch/A.md", Some("A")),node("ref", "README.md", None),node("lost", "docs/tower/arch/lost.md", Some("lost")),node("wrong", "docs/tower/arch/B.md", Some("A"))],"edges":[],"future":42})).unwrap());
    write(root.path(), "docs/tower/tower.canvas", &canvas);
    commit(root.path());
    git(root.path(), &["switch", "-qc", "task"]);
    write(
        root.path(),
        "docs/tower/tower.canvas",
        "dirty invalid canvas",
    );
    write(root.path(), "README.md", "dirty reference");
    let catalog = Catalog::load(root.path()).unwrap();
    let view = CanvasView::load(&catalog).unwrap();
    assert_eq!(view["canvas_text"], canvas);
    assert_eq!(view["revision"], catalog.revision);
    assert_eq!(view["nodes"][0]["status"], "bound");
    assert_eq!(view["nodes"][1]["text"], "ordinary reference\r\n\r\n");
    assert_eq!(view["nodes"][2]["status"], "dangling");
    assert_eq!(view["nodes"][3]["status"], "mismatch");
    assert_eq!(view["unplaced"], json!(["B"]));
    assert_eq!(
        view["objects"]["A"]["text"],
        fs::read_to_string(root.path().join("docs/tower/arch/A.md")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(root.path().join("docs/tower/tower.canvas")).unwrap(),
        "dirty invalid canvas"
    );
}

#[test]
fn references_never_read_outside_tree_or_follow_symlinks() {
    let root = fixture();
    #[cfg(unix)]
    std::os::unix::fs::symlink("README.md", root.path().join("link.md")).unwrap();
    let nodes: Vec<_> = [
        "../outside.md",
        "/tmp/outside.md",
        "C:\\outside.md",
        "link.md",
        "./sub/../README.md",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, path)| node(&index.to_string(), path, None))
    .collect();
    write(
        root.path(),
        "docs/tower/tower.canvas",
        &json!({"nodes":nodes,"edges":[]}).to_string(),
    );
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    for index in 0..4 {
        assert!(view["nodes"][index]["text"].is_null());
    }
    assert_eq!(view["nodes"][4]["text"], "ordinary reference\r\n\r\n");
}

#[test]
fn missing_or_invalid_canvas_is_visible_and_never_materialized() {
    let root = fixture();
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["status"], "missing");
    assert_eq!(view["unplaced"], json!(["A", "B"]));
    assert!(!root.path().join("docs/tower/tower.canvas").exists());
    write(root.path(), "docs/tower/tower.canvas", "{broken");
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["status"], "invalid");
    assert_eq!(view["canvas_text"], "{broken");
    assert_eq!(view["diagnostics"][0]["code"], "invalid_canvas");
}

#[test]
fn duplicate_json_properties_are_explicitly_opaque_not_an_empty_loaded_map() {
    let root = fixture();
    let text = "{\"nodes\":[],\"nodes\":[],\"edges\":[]}";
    write(root.path(), "docs/tower/tower.canvas", text);
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["status"], "opaque");
    assert_eq!(view["canvas_text"], text);
    assert!(!view["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn edges_require_interface_objects_and_newline_references_stay_literal() {
    let root = fixture();
    write(root.path(), "line\nbreak.md", "legal filename\n");
    let mut value = json!({"nodes":[node("a","docs/tower/arch/A.md",Some("A")),node("ref","line\nbreak.md",None)],"edges":[{"id":"edge","fromNode":"a","toNode":"ref","towermd":{"obj":"A"}}]});
    write(root.path(), "docs/tower/tower.canvas", &value.to_string());
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    assert_eq!(
        catalog.read_file("line\nbreak.md").unwrap().as_deref(),
        Some("legal filename\n")
    );
    let view = CanvasView::load(&catalog).unwrap();
    assert_eq!(view["edges"][0]["status"], "mismatch");
    assert_eq!(view["nodes"][1]["text"], "legal filename\n");
    write(
        root.path(),
        "docs/tower/arch/interface.md",
        "---\ntype: Interface\nid: IF-1\n---\n",
    );
    value["edges"][0]["towermd"]["obj"] = "IF-1".into();
    write(root.path(), "docs/tower/tower.canvas", &value.to_string());
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["edges"][0]["status"], "bound");
}

#[test]
fn interface_binding_is_unresolved_when_interface_catalog_is_not_loaded() {
    let root = fixture();
    let canvas = json!({
        "nodes": [node("a", "docs/tower/arch/A.md", Some("A")),
                  node("b", "docs/tower/arch/B.md", Some("B"))],
        "edges": [
            {"id":"edge", "fromNode":"a", "toNode":"b", "towermd":{"obj":"IF-1"}},
            {"id":"wrong", "fromNode":"a", "toNode":"b", "towermd":{"obj":"A"}},
            {"id":"annotation", "fromNode":"a", "toNode":"b"},
            {"id":"invalid", "fromNode":"a", "towermd":{"obj":"IF-1"}}
        ]
    })
    .to_string();
    write(root.path(), "docs/tower/tower.canvas", &canvas);
    commit(root.path());
    for yaml_exists in [false, true] {
        if yaml_exists {
            write(
                root.path(),
                "docs/tower/arch/interfaces.yaml",
                "- type: Interface\n  id: IF-1\n",
            );
            commit(root.path());
        }
        let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
        assert_eq!(view["edges"][0]["status"], "unresolved");
        let diagnostics = view["diagnostics"].as_array().unwrap();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "interface_unresolved"
                && diagnostic["collection"] == "edges"
                && diagnostic["index"] == 0
                && diagnostic["object"] == "IF-1"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic["collection"] == "edges" && diagnostic["index"] == 3));
        assert_eq!(view["edges"][1]["status"], "mismatch");
        assert_eq!(view["edges"][2]["status"], "annotation");
        assert_eq!(view["edges"][3]["status"], "opaque");
        assert_eq!(view["canvas_text"], canvas);
    }
    write(
        root.path(),
        "docs/tower/arch/interface.md",
        "---\ntype: Interface\nid: IF-1\n---\n",
    );
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["edges"][0]["status"], "bound");
    assert_eq!(view["edges"][1]["status"], "mismatch");
    assert_eq!(view["edges"][2]["status"], "annotation");
    assert_eq!(view["edges"][3]["status"], "opaque");
    assert!(!view["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "interface_unresolved"));
    assert_eq!(view["canvas_text"], canvas);
}

#[cfg(unix)]
#[test]
fn unsafe_canvas_keeps_its_error_diagnostic() {
    let root = fixture();
    std::os::unix::fs::symlink(
        "../../../README.md",
        root.path().join("docs/tower/tower.canvas"),
    )
    .unwrap();
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["status"], "invalid");
    assert!(view["canvas_text"].is_null());
    assert_eq!(view["diagnostics"][0]["code"], "unsafe_canvas");
    assert!(view["diagnostics"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("拒绝读取"));
}

fn write_invalid_layer(root: &Path) {
    write(
        root,
        "outside/invalid.md",
        "---\ntype: FutureThing\nlayer: null\nfuture: {retain: [unknown, 0.5]}\n---\nKeep.\n",
    );
}

fn assert_catalog_diagnostic(view: &serde_json::Value) {
    assert!(view["diagnostics"].as_array().unwrap().iter().any(|d| d == &json!({
        "code":"invalid_layer", "path":"outside/invalid.md", "message":"非法 layer；按慢层保护，不采用 type 默认层"
    })), "Catalog layer diagnostic missing: {}", view["diagnostics"]);
}

#[test]
fn catalog_diagnostics_survive_loaded_canvas_with_existing_diagnostics() {
    let root = fixture();
    write_invalid_layer(root.path());
    let canvas = format!(
        "{}\r\n",
        json!({"nodes":[node("a", "docs/tower/arch/A.md", Some("A")),node("b", "docs/tower/arch/B.md", Some("B")),{"id":"opaque","type":"text","text":"bad geometry","x":"bad","y":0,"width":100,"height":100}],"edges":[{"id":"unresolved","fromNode":"a","toNode":"b","towermd":{"obj":"IF-missing"}}],"future":{"keep":0.5}})
    );
    write(root.path(), "docs/tower/tower.canvas", &canvas);
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    assert_eq!(catalog.diagnostics.len(), 1);
    let view = CanvasView::load(&catalog).unwrap();
    assert_eq!(view["status"], "loaded");
    assert_eq!(view["canvas_text"], canvas);
    assert_eq!(view["nodes"][0]["status"], "bound");
    assert_eq!(view["nodes"][2]["status"], "opaque");
    assert_eq!(view["edges"][0]["status"], "unresolved");
    assert_catalog_diagnostic(&view);
    assert!(view["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "interface_unresolved"
            && d["collection"] == "edges"
            && d["index"] == 0));
    assert!(view["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["collection"] == "nodes" && d["index"] == 2));
}

#[test]
fn catalog_diagnostics_survive_missing_canvas() {
    let root = fixture();
    write_invalid_layer(root.path());
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["status"], "missing");
    assert!(view["canvas_text"].is_null());
    assert_eq!(view["unplaced"], json!(["A", "B"]));
    assert!(!root.path().join("docs/tower/tower.canvas").exists());
    assert_catalog_diagnostic(&view);
}

#[test]
fn catalog_diagnostics_survive_invalid_canvas() {
    let root = fixture();
    write_invalid_layer(root.path());
    write(root.path(), "docs/tower/tower.canvas", "{broken\r\n");
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["status"], "invalid");
    assert_eq!(view["canvas_text"], "{broken\r\n");
    assert_eq!(view["unplaced"], json!(["A", "B"]));
    assert!(view["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "invalid_canvas" && d["reason"].is_string()));
    assert_catalog_diagnostic(&view);
}

#[cfg(unix)]
#[test]
fn catalog_diagnostics_survive_unsafe_canvas() {
    let root = fixture();
    write_invalid_layer(root.path());
    std::os::unix::fs::symlink(
        "../../../README.md",
        root.path().join("docs/tower/tower.canvas"),
    )
    .unwrap();
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["status"], "invalid");
    assert!(view["canvas_text"].is_null());
    assert_eq!(view["unplaced"], json!(["A", "B"]));
    assert!(view["diagnostics"].as_array().unwrap().iter().any(
        |d| d["code"] == "unsafe_canvas" && d["reason"].as_str().unwrap().contains("拒绝读取")
    ));
    assert_catalog_diagnostic(&view);
}

#[test]
fn catalog_diagnostics_survive_opaque_canvas() {
    let root = fixture();
    write_invalid_layer(root.path());
    let text = "{\"nodes\":[],\"nodes\":[],\"edges\":[]}";
    write(root.path(), "docs/tower/tower.canvas", text);
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["status"], "opaque");
    assert_eq!(view["canvas_text"], text);
    assert!(view["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] != "invalid_layer"));
    assert_catalog_diagnostic(&view);
}
