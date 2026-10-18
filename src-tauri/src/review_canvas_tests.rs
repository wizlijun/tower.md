use super::*;
use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn canvas(x: i64) -> String {
    json!({"nodes":[{"id":"A","type":"file","file":"docs/tower/arch/A.md","towermd":{"obj":"A"},"x":x,"y":2,"width":320,"height":180,"future":{"keep":true}},{"id":"opaque","type":"future","x":5,"y":6,"width":30,"height":40,"custom":"原样"}],"edges":[],"future":"root"}).to_string()
}
fn review(catalog: &Catalog) -> Value {
    json!({"main_revision":catalog.revision,"current_baseline":null,"binding":{"project_id":catalog.config.project_id(),"object_id":"draft-1","content_hash":"c".repeat(64),"target_baseline":"towermd/B0","draft_revision":"d".repeat(40)},"draft":{"bundle":{"id":"draft-1","project_id":catalog.config.project_id(),"docs_dir":catalog.config.docs_dir(),"source_revision":catalog.revision,"target_baseline":"towermd/B0","objects":{"A":{"id":"A","path":"docs/tower/arch/A.md","text":"---\ntype: Module\nid: A\ntitle: 精确审阅版本\nfuture: 原样\n---\nreview body\r\n"}},"canvas_text":canvas(71),"deletions":[],"questions":[],"risks":[]},"files":{},"base_directory":{},"content_hash":"c".repeat(64)},"diff":[],"interface_modules":[],"publish_blockers":[]})
}
fn assert_unavailable(value: &Value) {
    assert_eq!(value["state"], "unavailable");
    assert!(!value["reason"].as_str().unwrap().is_empty());
    assert!(value.get("baseline_canvas").is_none());
    assert!(value.get("draft_canvas").is_none());
}

#[test]
fn comparison_reads_exact_main_git_bytes_and_review_revision_without_worktree_writes() {
    let (root, _, _, initial) = crate::draft_requests::tests::project();
    let docs = root.path().join(initial.config.docs_dir());
    fs::create_dir_all(docs.join("arch")).unwrap();
    let main_text = "---\ntype: Module\nid: A\ntitle: 主干\nfuture: 保留\n---\nmain body\r\n";
    fs::write(docs.join("arch/A.md"), main_text).unwrap();
    fs::write(docs.join("tower.canvas"), canvas(11)).unwrap();
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-qm", "main canvas"]);
    let catalog = Catalog::load(root.path()).unwrap();
    let original = review(&catalog);
    // A later private ref and uncommitted worktree must not replace fetched review bytes.
    git(
        root.path(),
        &[
            "update-ref",
            "refs/towermd/drafts/draft-1",
            &catalog.revision,
        ],
    );
    fs::write(docs.join("tower.canvas"), canvas(999)).unwrap();
    fs::write(docs.join("arch/A.md"), "uncommitted user worktree").unwrap();
    let main_before = git(root.path(), &["rev-parse", "main"]);
    let status_before = git(root.path(), &["status", "--porcelain=v1"]);
    let index_before = fs::read(root.path().join(".git/index")).unwrap();
    let result = comparison(&catalog, &original);
    assert_eq!(result["state"], "available");
    assert_eq!(result["main_revision"], catalog.revision);
    assert_eq!(result["baseline"], Value::Null); // A main SHA never invents B0 approval.
    assert_eq!(
        result["baseline_canvas"]["canvas_view"]["nodes"][0]["raw"]["x"],
        11
    );
    assert_eq!(
        result["draft_canvas"]["canvas_view"]["nodes"][0]["raw"]["x"],
        71
    );
    assert_eq!(
        result["baseline_canvas"]["object_views"][0]["text"],
        main_text
    );
    assert_eq!(
        result["draft_canvas"]["object_views"][0]["text"],
        original["draft"]["bundle"]["objects"]["A"]["text"]
    );
    assert_eq!(
        result["draft_canvas"]["canvas_view"]["nodes"][1]["opaque"],
        true
    );
    assert_eq!(
        result["draft_canvas"]["canvas_view"]["nodes"][1]["raw"]["custom"],
        "原样"
    );
    assert_eq!(
        result["draft_canvas"]["canvas_view"]["nodes"][0]["raw"]["future"],
        json!({"keep":true})
    );
    assert_eq!(git(root.path(), &["rev-parse", "main"]), main_before);
    assert_eq!(
        git(root.path(), &["status", "--porcelain=v1"]),
        status_before
    );
    assert_eq!(
        fs::read(root.path().join(".git/index")).unwrap(),
        index_before
    );
    assert_eq!(
        fs::read_to_string(docs.join("tower.canvas")).unwrap(),
        canvas(999)
    );
}

#[test]
fn missing_main_canvas_is_absent_not_a_directory_preview() {
    let (root, _, _, initial) = crate::draft_requests::tests::project();
    let docs = root.path().join(initial.config.docs_dir());
    // init has no committed main canvas; a visible worktree canvas is not main.
    assert!(initial
        .read_file(&format!("{}/tower.canvas", initial.config.docs_dir()))
        .unwrap()
        .is_none());
    fs::create_dir_all(&docs).unwrap();
    fs::write(docs.join("tower.canvas"), canvas(999)).unwrap();
    let result = comparison(&initial, &review(&initial));
    assert_eq!(result["state"], "available");
    assert_eq!(result["baseline_canvas"], Value::Null);
    assert_eq!(
        result["draft_canvas"]["canvas_view"]["nodes"][0]["raw"]["x"],
        71
    );
    assert!(!result["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn changed_or_missing_main_identity_is_unavailable_without_hiding_original_review() {
    let (root, _, _, catalog) = crate::draft_requests::tests::project();
    let original = review(&catalog);
    for replacement in [Value::Null, json!("a".repeat(40)), json!("")] {
        let mut changed = original.clone();
        changed["main_revision"] = replacement;
        assert_unavailable(&comparison(&catalog, &changed));
        assert_eq!(changed["draft"], original["draft"]);
    }
    git(
        root.path(),
        &["commit", "--allow-empty", "-qm", "advanced main"],
    );
    assert_unavailable(&comparison(&catalog, &original));
    let mut other = Catalog::load(root.path()).unwrap();
    other.common_dir = other.common_dir.join("another-repo");
    let current = review(&Catalog::load(root.path()).unwrap());
    assert_unavailable(&comparison(&other, &current));
    let config =
        towermd_core::config::Config::new("another-project", catalog.config.docs_dir()).unwrap();
    fs::write(
        root.path().join(towermd_core::config::CONFIG_PATH),
        config.to_yaml(),
    )
    .unwrap();
    git(root.path(), &["add", towermd_core::config::CONFIG_PATH]);
    git(root.path(), &["commit", "-qm", "change project identity"]);
    let current = review(&Catalog::load(root.path()).unwrap());
    assert_unavailable(&comparison(&catalog, &current));
}

#[test]
fn invalid_canvas_or_unsafe_object_paths_only_disable_comparison() {
    let (_root, _, _, catalog) = crate::draft_requests::tests::project();
    let original = review(&catalog);
    for text in [
        "not JSON",
        r#"{"nodes":{}}"#,
        r#"{"nodes":[{"id":"bad","type":"file","file":"../escape","x":1,"y":2,"width":3,"height":4}],"edges":[]}"#,
    ] {
        let mut changed = original.clone();
        changed["draft"]["bundle"]["canvas_text"] = json!(text);
        assert_unavailable(&comparison(&catalog, &changed));
    }
    for path in [
        "../escape.md",
        "/tmp/escape.md",
        "docs/tower/../escape.md",
        "docs/tower/.git/config",
    ] {
        let mut changed = original.clone();
        changed["draft"]["bundle"]["objects"]["A"]["path"] = json!(path);
        assert_unavailable(&comparison(&catalog, &changed));
    }
    let mut changed = original;
    changed["draft"]["bundle"]["project_id"] = json!("another-project");
    assert_unavailable(&comparison(&catalog, &changed));
}

#[test]
fn invalid_or_symlink_main_canvas_cannot_be_a_current_comparison() {
    for symlink in [false, true] {
        let (root, _, _, initial) = crate::draft_requests::tests::project();
        let docs = root.path().join(initial.config.docs_dir());
        fs::create_dir_all(&docs).unwrap();
        let path = docs.join("tower.canvas");
        if symlink {
            #[cfg(unix)]
            std::os::unix::fs::symlink("../external.canvas", &path).unwrap();
            #[cfg(not(unix))]
            continue;
        } else {
            fs::write(&path, "invalid JSON").unwrap();
        }
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "invalid main canvas"]);
        let catalog = Catalog::load(root.path()).unwrap();
        assert_unavailable(&comparison(&catalog, &review(&catalog)));
    }
}

#[test]
fn comparison_projects_interface_endpoints_and_keeps_core_baseline_label() {
    let (_root, _, _, catalog) = crate::draft_requests::tests::project();
    let mut original = review(&catalog);
    original["current_baseline"] = json!("towermd/B7");
    let text = "---\ntype: Interface\nid: I\ntitle: 契约\nfrom: A\nto: B\nfuture: 保留\n---\nexact interface body\r\n";
    original["draft"]["bundle"]["objects"]["I"] =
        json!({"id":"I","path":"docs/tower/arch/I.md","text":text});
    let result = comparison(&catalog, &original);
    assert_eq!(result["state"], "available");
    assert_eq!(result["baseline"], "towermd/B7");
    assert_eq!(result["main_revision"], catalog.revision);
    let interface = &result["draft_canvas"]["object_views"][1];
    assert_eq!(interface["id"], "I");
    assert_eq!(interface["type"], "Interface");
    assert_eq!(interface["fields"]["from"], "A");
    assert_eq!(interface["fields"]["to"], "B");
    assert_eq!(interface["text"], text);
}

#[test]
fn ordinary_root_agents_docref_remains_available_without_becoming_an_object() {
    let (_root, _, _, catalog) = crate::draft_requests::tests::project();
    let mut original = review(&catalog);
    let mut value: Value =
        serde_json::from_str(original["draft"]["bundle"]["canvas_text"].as_str().unwrap()).unwrap();
    value["nodes"].as_array_mut().unwrap().push(json!({"id":"instructions","type":"file","file":"AGENTS.md","x":1,"y":2,"width":3,"height":4}));
    original["draft"]["bundle"]["canvas_text"] = json!(value.to_string());
    let result = comparison(&catalog, &original);
    assert_eq!(result["state"], "available");
    assert_eq!(
        result["draft_canvas"]["canvas_view"]["nodes"][2]["raw"]["file"],
        "AGENTS.md"
    );
    assert_eq!(
        result["draft_canvas"]["object_views"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
