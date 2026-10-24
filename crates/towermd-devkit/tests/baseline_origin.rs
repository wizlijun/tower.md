#![cfg(unix)]
//! J4: a scan seeded from an approved baseline keeps its modules, carries the
//! approved objects into a draft and compares the reviewed draft against them.
use std::collections::BTreeMap;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
use towermd_core::{catalog::Catalog, config::{Config, CONFIG_PATH}, okf::Document};
use towermd_devkit::baseline_origin::{compare, DiffKind, Origin, OriginModule, OriginObject};
use towermd_devkit::review::{self, ReviewAction, ReviewedCommand};
use towermd_devkit::{baseline, cold_start, verifier::{self, Limits, VerificationPlan}};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Origin Test", "-c", "user.email=qa@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn approved(id: &str, kind: &str, path: &str, front: &str) -> OriginObject {
    OriginObject {
        id: id.into(),
        object_type: kind.into(),
        path: path.into(),
        text: format!(
            "---\ntype: {kind}\nid: {id}\n{front}\nstatus: stable\nbaseline: B0\nverified:\n  by: human:local\n  at: 2026-10-20T00:00:00Z\ngenerated:\n  by: qa/1\n  at: 2026-10-19T00:00:00Z\n---\n{id} 的已批准正文。\n"
        ),
    }
}
fn module(id: &str, title: &str, paths: &[&str]) -> (OriginModule, OriginObject) {
    let list = paths.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n");
    (
        OriginModule {
            id: id.into(),
            title: title.into(),
            paths: paths.iter().map(|p| p.to_string()).collect(),
            commands: vec![ReviewedCommand { directory: String::new(), command: "make test".into() }],
        },
        approved(
            id,
            "Module",
            &format!("arch/{id}.md"),
            &format!("title: {title}\nlayer: L1\npaths:\n{list}\nverify:\n- make test\nverification_commands:\n- directory: ''\n  command: make test\nreview_verification_pending: true"),
        ),
    )
}

fn origin() -> Origin {
    let (a1, d1) = module("A-1", "客户端", &["client/main.rs", "client/view.rs"]);
    let (a2, d2) = module("A-2", "引擎", &["engine/main.rs", "engine/gone.rs"]);
    let (a3, d3) = module("A-3", "存储", &["storage/main.rs"]);
    let objects = vec![
        d1,
        d2,
        d3,
        approved("I-1", "Interface", "arch/I-1.md", "title: 查询\nlayer: L1\nfrom: A-1\nto: A-2\ncontract: 返回结果"),
        approved("AC-1", "Acceptance", "specs/AC-1.md", "title: 引擎可用\nlayer: L2\nmodule: A-2\ncriteria:\n- 能跑\nverify:\n- make test\n- cargo test"),
        approved("IN-1", "Intent", "intents/IN-1.md", "title: 目标\nlayer: L0\nallocated_to:\n- A-1"),
    ];
    Origin {
        baseline: "towermd/B0".into(),
        commit: "0".repeat(40),
        modules: vec![a1, a2, a3],
        objects,
        positions: BTreeMap::from([
            ("A-1".into(), [10, 20, 300, 180]),
            ("I-1".into(), [900, 900, 300, 180]),
            ("unassigned".into(), [-900, 40, 320, 200]),
        ]),
    }
}

fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-qb", "main"]);
    fs::create_dir(root.join(".towermd")).unwrap();
    fs::write(root.join(CONFIG_PATH), Config::new("qa", "docs/tower").unwrap().to_yaml()).unwrap();
    // engine/gone.rs was deleted since B0; engine/extra.rs and tools/new.rs are new.
    for path in ["client/main.rs", "client/view.rs", "engine/main.rs", "engine/extra.rs", "storage/main.rs", "tools/new.rs"] {
        fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        fs::write(root.join(path), "fn main() {}\n").unwrap();
    }
    fs::write(root.join("Makefile"), "test:\n\t@printf 'origin proof\\n'\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    directory
}

fn verified(root: &Path) -> cold_start::ScanReport {
    let mut scan = cold_start::scan_from_baseline(&Catalog::load(root).unwrap(), &uuid::Uuid::now_v7().to_string(), origin()).unwrap();
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
    scan
}

#[test]
fn a_seeded_scan_keeps_approved_modules_commands_and_layout() {
    let directory = repository();
    let scan = cold_start::scan_from_baseline(&Catalog::load(directory.path()).unwrap(), "seeded", origin()).unwrap();
    let ids: Vec<_> = scan.modules.iter().map(|m| (m.id.as_str(), m.title.as_str())).collect();
    assert_eq!(ids, [("A-1", "客户端"), ("A-2", "引擎"), ("A-3", "存储")]);
    // Deleted files drop out, a new file joins its directory's module, and a
    // file in no approved directory stays unassigned.
    assert_eq!(scan.modules[1].files, ["engine/extra.rs", "engine/main.rs"]);
    assert_eq!(scan.unassigned, ["tools/new.rs"]);
    assert!(scan.diagnostics.iter().any(|d| d.contains("engine/extra.rs → 引擎")), "{:?}", scan.diagnostics);
    assert!(scan.diagnostics.iter().any(|d| d.starts_with("S2 按 ") && d.contains("从 towermd/B0 进入草稿")));
    assert!(scan.modules.iter().all(|m| m.rationale.starts_with("沿用 towermd/B0 的已批准边界")));
    // Only the approved command, shared, no detection.
    assert_eq!(scan.commands.len(), 1);
    assert_eq!(scan.commands[0].command, "make test");
    assert_eq!(scan.commands[0].modules, ["A-1", "A-2", "A-3"]);
    let node = |id: &str| scan.view.nodes.iter().find(|n| n.id == id).unwrap();
    assert_eq!((node("A-1").x, node("A-1").y), (10, 20));
    assert_eq!((node("unassigned").x, node("unassigned").y), (-900, 40));
    assert_eq!(scan.origin, Some(origin()));
    // Same commit and origin, same report.
    let again = cold_start::scan_from_baseline(&Catalog::load(directory.path()).unwrap(), "seeded", origin()).unwrap();
    assert_eq!(serde_json::to_value(&again).unwrap(), serde_json::to_value(&scan).unwrap());
    // A plain scan never carries an origin.
    let plain = cold_start::scan(&Catalog::load(directory.path()).unwrap(), "plain").unwrap();
    assert!(plain.origin.is_none());
    assert!(serde_json::to_value(&plain).unwrap().get("origin").is_none());
}

#[test]
fn the_template_carries_approved_objects_back_as_unapproved_drafts() {
    let directory = repository();
    let scan = verified(directory.path());
    let submission = baseline::template(&scan).unwrap();
    assert_eq!(submission.modules.len(), 3);
    assert_eq!((submission.intents.len(), submission.interfaces.len(), submission.acceptances.len()), (1, 1, 1));
    for object in submission.modules.iter().chain(&submission.interfaces).chain(&submission.acceptances).chain(&submission.intents) {
        let document = Document::parse(&object.text).unwrap();
        assert_eq!(document.get_str("status"), Some("draft"), "{}", object.text);
        for key in ["verified", "baseline", "generated", "review_verification_pending"] {
            assert!(document.get(key).is_none(), "{key}: {}", object.text);
        }
        assert!(document.body().contains("已批准正文"));
    }
    let engine = Document::parse(&submission.modules[1].text).unwrap();
    let paths: Vec<String> = serde_yaml::from_value(engine.get("paths").unwrap().clone()).unwrap();
    assert_eq!(paths, ["engine/extra.rs", "engine/main.rs"]);
    // The acceptance keeps only commands its module still runs.
    let acceptance = Document::parse(&submission.acceptances[0].text).unwrap();
    let verify: Vec<String> = serde_yaml::from_value(acceptance.get("verify").unwrap().clone()).unwrap();
    assert_eq!(verify, ["make test"]);
    assert!(submission.risks.iter().any(|risk| risk.contains("AC-1")), "{:?}", submission.risks);

    let draft = baseline::synthesize(&scan, submission, "towermd/0.1.0", "2026-10-24T00:00:00Z", &format!("d-{}", "b".repeat(64))).unwrap();
    let interface = draft.view.nodes.iter().find(|n| n.id == "I-1").unwrap();
    assert_eq!((interface.x, interface.y), (900, 900));
    assert!(draft.view.diagnostics.iter().any(|d| d.starts_with("从 towermd/B0 进入草稿")), "{:?}", draft.view.diagnostics);

    // Unreviewed, the draft differs from B0 only where the commit did.
    let comparison = compare(&draft).unwrap();
    let kinds: BTreeMap<_, _> = comparison.objects.iter().map(|o| (o.id.as_str(), o.kinds.clone())).collect();
    assert_eq!(kinds, BTreeMap::from([("A-2", vec![DiffKind::Files]), ("AC-1", vec![DiffKind::Commands])]));
    let engine = comparison.objects.iter().find(|o| o.id == "A-2").unwrap();
    assert_eq!(engine.files_added, ["engine/extra.rs"]);
    assert_eq!(engine.files_removed, ["engine/gone.rs"]);
    assert_eq!(comparison.unchanged, ["A-1", "A-3", "I-1", "IN-1"]);
    assert_eq!(comparison.unassigned, ["tools/new.rs"]);
    assert_eq!(comparison.baseline, "towermd/B0");
}

#[test]
fn the_comparison_names_renames_moves_merges_and_splits() {
    let directory = repository();
    let scan = verified(directory.path());
    let draft = baseline::synthesize(&scan, baseline::template(&scan).unwrap(), "towermd/0.1.0", "2026-10-24T00:00:00Z", &format!("d-{}", "c".repeat(64))).unwrap();
    let actions = vec![
        ReviewAction::Rename { node_id: "A-1".into(), title: "前端".into() },
        ReviewAction::Move { node_id: "I-1".into(), x: 0, y: 1200 },
        ReviewAction::Merge { module_ids: vec!["A-2".into(), "A-3".into()], target_id: "A-2".into(), title: "引擎".into() },
        ReviewAction::Split { module_id: "A-1".into(), new_id: "A-4".into(), title: "视图".into(), files: vec!["client/view.rs".into()] },
    ];
    let reviewed = review::replay(&draft, &actions, "human:local", &format!("d-{}", "d".repeat(64)), "2026-10-24T00:01:00Z", &BTreeMap::new()).unwrap();
    let comparison = compare(&reviewed).unwrap();
    let find = |id: &str| comparison.objects.iter().find(|o| o.id == id).unwrap_or_else(|| panic!("{id}: {comparison:?}"));
    let client = find("A-1");
    assert!(client.kinds.contains(&DiffKind::Renamed) && client.kinds.contains(&DiffKind::Files), "{client:?}");
    assert_eq!(client.before_title.as_deref(), Some("客户端"));
    assert_eq!(client.files_removed, ["client/view.rs"]);
    assert_eq!(find("A-4").kinds, [DiffKind::Added, DiffKind::SplitFrom]);
    assert_eq!(find("A-4").related.as_deref(), Some("A-1"));
    assert_eq!(find("A-3").kinds, [DiffKind::Removed, DiffKind::MergedInto]);
    assert_eq!(find("A-3").related.as_deref(), Some("A-2"));
    assert!(find("A-2").files_added.contains(&"storage/main.rs".to_string()));
    assert_eq!(find("I-1").kinds, [DiffKind::Moved]);
    // A draft from a fresh scan has nothing to compare against.
    let mut plain = reviewed.clone();
    plain.scan.origin = None;
    assert!(compare(&plain).is_err());
}
