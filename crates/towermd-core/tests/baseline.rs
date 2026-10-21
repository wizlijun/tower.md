#![cfg(unix)]
use serde_json::json;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
use towermd_core::{
    baseline::{self, DraftObject, Submission},
    catalog::Catalog,
    cold_start::{self, ScanReport},
    config::{Config, CONFIG_PATH},
    okf::Document,
    verifier::{self, Limits, VerificationPlan},
};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Baseline Test",
            "-c",
            "user.email=qa@example.invalid",
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
}
fn fixture() -> (tempfile::TempDir, ScanReport) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-qb", "main"]);
    fs::create_dir(root.join(".towermd")).unwrap();
    fs::write(
        root.join(CONFIG_PATH),
        Config::new("qa", "docs/tower").unwrap().to_yaml(),
    )
    .unwrap();
    for name in ["client", "engine", "storage"] {
        fs::create_dir(root.join(name)).unwrap();
        fs::write(root.join(name).join("main.rs"), "fn main() {}\n").unwrap();
    }
    fs::write(
        root.join("Makefile"),
        "test:\n\t@printf 'real failed baseline verification\\n'\n\t@exit 1\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    let mut scan = cold_start::scan(
        &Catalog::load(root).unwrap(),
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let result = verifier::run(
        &VerificationPlan {
            root: root.into(),
            id: scan.id.clone(),
            revision: scan.revision.clone(),
            tree: scan.tree.clone(),
            commands: scan.commands.clone(),
            modules: scan.modules.clone(),
            limits: Limits::default(),
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    cold_start::apply_verification(&mut scan, result).unwrap();
    (directory, scan)
}
fn synthesize(
    scan: &ScanReport,
    submission: Submission,
) -> Result<baseline::BaselineDraft, String> {
    baseline::synthesize(
        scan,
        submission,
        "test-harness/1",
        "2026-10-21T00:00:00Z",
        &format!("d-{}", "a".repeat(64)),
    )
}
fn edit(object: &mut DraftObject, key: &str, value: serde_yaml::Value) {
    let mut document = Document::parse(&object.text).unwrap();
    document.set(key, value).unwrap();
    object.text = document.to_text();
}
fn object(id: &str, path: &str, front: &str) -> DraftObject {
    DraftObject {
        id: id.into(),
        path: path.into(),
        text: format!("---\nid: {id}\n{front}\n---\n草稿正文。\n"),
    }
}

#[test]
fn schema_is_shared_and_validates_nested_shapes() {
    let minimal = json!({"scan_id":"s","project_id":"p","source_revision":"r","intents":[],"modules":[{"id":"A","path":"arch/A.md","text":"x"}],"interfaces":[],"acceptances":[],"questions":[],"risks":[]});
    assert_eq!(baseline::schema()["$id"], "towermd:baseline-submit:v1");
    assert!(baseline::parse(minimal.clone()).is_ok());
    for (pointer, value) in [
        ("/modules/0/path", json!(1)),
        ("/modules/0/text", json!("")),
        ("/questions", json!([3])),
        ("/modules", json!([])),
    ] {
        let mut input = minimal.clone();
        *input.pointer_mut(pointer).unwrap() = value;
        assert!(baseline::parse(input).is_err(), "{pointer}");
    }
    let mut input = minimal;
    input["approved"] = json!(true);
    assert!(baseline::parse(input).is_err());
}

#[test]
fn actual_failed_scan_keeps_evidence_coordinates_and_exact_input_with_new_semantic_nodes() {
    let (_directory, scan) = fixture();
    let mut input = baseline::template(&scan).unwrap();
    edit(&mut input.modules[0], "title", "会话综合的名称".into());
    edit(
        &mut input.modules[0],
        "x-custom",
        serde_yaml::to_value(json!({"keep":[1,2]})).unwrap(),
    );
    edit(&mut input.modules[0], "verification", "passed".into());
    edit(
        &mut input.modules[0],
        "initial_evidence",
        serde_yaml::to_value(json!({"actor":"human:fake","status":"passed"})).unwrap(),
    );
    let a = input.modules[0].id.clone();
    let b = input.modules[1].id.clone();
    input.intents.push(object(
        "I-purpose",
        "intents/I-purpose.md",
        &format!("type: Intent\nlayer: L0\ntitle: 项目意图\nallocated_to: [{a}]"),
    ));
    input.interfaces.push(object("IF-api", "arch/IF-api.md", &format!("type: Interface\nlayer: L1\ntitle: 调用约定\nfrom: {a}\nto: {b}\ncontract: 请求与响应约定待人确认")));
    input.acceptances.push(object(
        "AC-client",
        "specs/AC-client.md",
        &format!("type: Acceptance\nlayer: L2\nmodule: {a}\ncriteria: [调用返回预期结果]"),
    ));
    let original = serde_json::to_vec(&input).unwrap();
    let draft = synthesize(&scan, input).unwrap();
    assert_eq!(serde_json::to_vec(&draft.submission).unwrap(), original);
    for source in &scan.view.nodes {
        let node = draft.view.nodes.iter().find(|n| n.id == source.id).unwrap();
        assert_eq!(
            (node.x, node.y, node.width, node.height),
            (source.x, source.y, source.width, source.height)
        );
    }
    let node = draft
        .view
        .nodes
        .iter()
        .find(|n| n.object_id.as_deref() == Some(&a))
        .unwrap();
    let document = Document::parse(&node.content).unwrap();
    assert_eq!(node.title, "会话综合的名称");
    assert_eq!(document.get_str("verification"), Some("failed"));
    assert_eq!(
        document.get("initial_evidence").unwrap()["actor"].as_str(),
        Some("process:towermd-verifier")
    );
    assert_eq!(
        document.get("x-custom").unwrap()["keep"]
            .as_sequence()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        document.get("generated").unwrap()["by"].as_str(),
        Some("test-harness/1")
    );
    assert_eq!(draft.view.nodes.len(), scan.view.nodes.len() + 3);
    assert!(draft.view.title.contains("综合草稿"));
    assert!(!draft
        .view
        .diagnostics
        .iter()
        .any(|message| message.contains("尚未完成 S6")));
    assert!(draft.view.edges.iter().any(
        |edge| edge.id == "interface-IF-api" && edge.label.as_deref() == Some("接口：调用约定")
    ));
    let again = synthesize(&scan, draft.submission.clone()).unwrap();
    assert_eq!(
        serde_json::to_vec(&again).unwrap(),
        serde_json::to_vec(&draft).unwrap()
    );
}

#[test]
fn invalid_binding_paths_types_references_approval_and_modified_verification_are_rejected() {
    let (_directory, scan) = fixture();
    let input = baseline::template(&scan).unwrap();
    let mut bad = input.clone();
    bad.scan_id = "another".into();
    assert!(synthesize(&scan, bad).is_err());
    let mut bad = input.clone();
    bad.project_id = "another".into();
    assert!(synthesize(&scan, bad).is_err());
    let mut bad = input.clone();
    bad.source_revision = "another".into();
    assert!(synthesize(&scan, bad).is_err());
    let mut bad = input.clone();
    bad.modules.push(bad.modules[0].clone());
    assert!(synthesize(&scan, bad).is_err());
    for path in [
        "../arch/A.md",
        "/arch/A.md",
        "arch/../A.md",
        "arch/.git.md",
        "specs/A.md",
        "arch/a\\b.md",
    ] {
        let mut bad = input.clone();
        bad.modules[0].path = path.into();
        assert!(synthesize(&scan, bad).is_err(), "{path}");
    }
    for (key, value) in [
        ("id", json!("other")),
        ("type", json!("Intent")),
        ("layer", json!("L5")),
        ("status", json!("stable")),
        ("verified", json!({"by":"human:fake"})),
        ("baseline", json!("B0")),
        ("owner", json!("human:fake")),
        ("paths", json!(["other/**"])),
        ("verify", json!(["echo fake"])),
        ("confidence", json!(1.1)),
        ("rationale", json!("")),
        ("interfaces", json!(["missing"])),
    ] {
        let mut bad = input.clone();
        edit(
            &mut bad.modules[0],
            key,
            serde_yaml::to_value(value).unwrap(),
        );
        assert!(synthesize(&scan, bad).is_err(), "{key}");
    }
    let mut bad = input.clone();
    bad.interfaces.push(object(
        "IF-bad",
        "arch/IF-bad.md",
        "type: Interface\nlayer: L1\nfrom: missing\nto: missing\ncontract: test",
    ));
    assert!(synthesize(&scan, bad).is_err());
    let mut bad = input.clone();
    bad.acceptances.push(object(
        "AC-bad",
        "specs/AC-bad.md",
        "type: Acceptance\nlayer: L2\nmodule: missing\ncriteria: [test]",
    ));
    assert!(synthesize(&scan, bad).is_err());
    let mut bad = input.clone();
    bad.acceptances.push(object(
        "AC-command",
        "specs/AC-command.md",
        &format!(
            "type: Acceptance\nlayer: L2\nmodule: {}\ncriteria: [test]\nverify: [echo invented]",
            input.modules[0].id
        ),
    ));
    assert!(synthesize(&scan, bad).is_err());
    for producer in [
        "human:fake",
        "process:towermd-cli",
        "human:fake/1",
        "test",
        "a/b/c",
    ] {
        assert!(baseline::synthesize(
            &scan,
            input.clone(),
            producer,
            "2026-10-21T00:00:00Z",
            &format!("d-{}", "a".repeat(64))
        )
        .is_err());
    }
    let mut unfinished = scan.clone();
    unfinished.verification = None;
    assert!(synthesize(&unfinished, input.clone()).is_err());
    let mut unfinished = scan.clone();
    unfinished.verification.as_mut().unwrap().cleanup_complete = false;
    assert!(synthesize(&unfinished, input).is_err());
}

#[test]
fn completed_no_tests_can_be_synthesized_but_interrupted_verification_cannot() {
    let (_directory, mut scan) = fixture();
    for status in [
        verifier::VerificationStatus::TimedOut,
        verifier::VerificationStatus::Cancelled,
        verifier::VerificationStatus::NotRun,
    ] {
        let mut interrupted = scan.clone();
        for result in &mut interrupted.verification.as_mut().unwrap().commands {
            result.status = status;
        }
        for result in &mut interrupted.verification.as_mut().unwrap().modules {
            result.status = status;
        }
        assert!(baseline::template(&interrupted).is_err());
    }
    // A no-test candidate has no commands, and no success evidence is invented.
    scan.commands.clear();
    for module in &mut scan.modules {
        module.command_ids.clear();
    }
    let report = scan.verification.as_mut().unwrap();
    report.commands.clear();
    for result in &mut report.modules {
        result.command_ids.clear();
        result.shared = false;
        result.status = verifier::VerificationStatus::NoTests;
    }
    for node in &mut scan.view.nodes {
        if node.object_type.as_deref() != Some("Module") {
            continue;
        }
        let mut doc = Document::parse(&node.content).unwrap();
        doc.set(
            "verify",
            serde_yaml::to_value(Vec::<String>::new()).unwrap(),
        )
        .unwrap();
        doc.set(
            "verification_commands",
            serde_yaml::to_value(Vec::<String>::new()).unwrap(),
        )
        .unwrap();
        node.content = doc.to_text();
    }
    let report = scan.verification.clone().unwrap();
    cold_start::apply_verification(&mut scan, report).unwrap();
    let draft = synthesize(&scan, baseline::template(&scan).unwrap()).unwrap();
    for node in &draft.view.nodes {
        if node.object_type.as_deref() == Some("Module") {
            assert_eq!(
                Document::parse(&node.content)
                    .unwrap()
                    .get_str("verification"),
                Some("no_tests")
            );
        }
    }
}

#[test]
fn unassigned_not_run_command_does_not_block_completed_module_results() {
    let (_directory, mut scan) = fixture();
    let mut command = scan.commands[0].clone();
    command.id = "unassigned-command".into();
    command.modules.clear();
    scan.commands.push(command);
    let mut result = scan.verification.as_ref().unwrap().commands[0].clone();
    result.id = "unassigned-command".into();
    result.modules.clear();
    result.status = verifier::VerificationStatus::NotRun;
    result.log = None;
    result.log_sha256 = None;
    result.log_excerpt.clear();
    result.exit_code = None;
    scan.verification.as_mut().unwrap().commands.push(result);
    assert!(synthesize(&scan, baseline::template(&scan).unwrap()).is_ok());
}

#[test]
fn reloading_a_legacy_scan_does_not_add_a_project_region_to_its_synthesis() {
    let (_directory, mut scan) = fixture();
    scan.view.nodes.retain(|node| node.region.is_none());
    let bytes = serde_json::to_vec(&scan).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("\"region\""));
    let legacy: ScanReport = serde_json::from_slice(&bytes).unwrap();
    let submission = baseline::template(&legacy).unwrap();
    let draft = synthesize(&legacy, submission).unwrap();
    assert_eq!(draft.view.nodes.len(), legacy.view.nodes.len());
    assert!(draft.view.nodes.iter().all(|node| node.region.is_none()));
}
