#![cfg(unix)]
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, process::Command, sync::atomic::AtomicBool};
use towermd_core::{
    baseline::{self, BaselineDraft, DraftObject},
    catalog::Catalog,
    cold_start,
    config::{Config, CONFIG_PATH},
    okf::Document,
    review::{self, ReviewAction, ReviewedCommand},
    verifier::{self, Limits, VerificationPlan},
};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Review Test",
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
fn fixture() -> (tempfile::TempDir, BaselineDraft) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-qb", "main"]);
    fs::create_dir(root.join(".towermd")).unwrap();
    fs::write(
        root.join(CONFIG_PATH),
        Config::new("qa", "docs/tower").unwrap().to_yaml(),
    )
    .unwrap();
    for name in ["client", "engine", "storage"] {
        fs::create_dir(root.join(name)).unwrap();
        for file in ["main.rs", "lib.rs"] {
            fs::write(root.join(name).join(file), "fn example() {}\n").unwrap();
        }
    }
    fs::write(
        root.join("Makefile"),
        "test:\n\t@printf 'real review verification\\n'\n",
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
    let mut input = baseline::template(&scan).unwrap();
    let a = input.modules[0].id.clone();
    let b = input.modules[1].id.clone();
    let mut module = Document::parse(&input.modules[0].text).unwrap();
    module
        .set(
            "x-custom",
            serde_yaml::to_value(json!({"keep":[1,2]})).unwrap(),
        )
        .unwrap();
    input.modules[0].text = module.to_text();
    input.interfaces.push(object(
        "IF-call",
        "arch/IF-call.md",
        &format!("type: Interface\nlayer: L1\nfrom: {a}\nto: {b}\ncontract: 调用契约\ntitle: 调用"),
    ));
    input.intents.push(object(
        "I-purpose",
        "intents/I-purpose.md",
        &format!("type: Intent\nlayer: L0\nallocated_to: [{a}, {b}]\ntitle: 目标"),
    ));
    input.acceptances.push(object(
        "AC-result",
        "specs/AC-result.md",
        &format!("type: Acceptance\nlayer: L2\nmodule: {b}\ncriteria: [结果符合契约]"),
    ));
    let draft = baseline::synthesize(
        &scan,
        input,
        "test/1",
        "2026-10-21T00:00:00Z",
        &format!("d-{}", "a".repeat(64)),
    )
    .unwrap();
    (temp, draft)
}
fn object(id: &str, path: &str, front: &str) -> DraftObject {
    DraftObject {
        id: id.into(),
        path: path.into(),
        text: format!("---\nid: {id}\n{front}\n---\n原始正文。\n"),
    }
}
fn apply(base: &BaselineDraft, actions: &[ReviewAction]) -> Result<BaselineDraft, String> {
    review::replay(
        base,
        actions,
        "human:local",
        &format!("d-{}", "b".repeat(64)),
        "2026-10-21T01:00:00Z",
        &BTreeMap::new(),
    )
}
fn document(draft: &BaselineDraft, id: &str) -> Document {
    Document::parse(
        &draft
            .view
            .nodes
            .iter()
            .find(|n| n.id == id)
            .unwrap()
            .content,
    )
    .unwrap()
}
fn identical_original(base: &BaselineDraft, edited: &BaselineDraft) {
    assert_eq!(
        serde_json::to_value(&base.scan).unwrap(),
        serde_json::to_value(&edited.scan).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&base.submission).unwrap(),
        serde_json::to_value(&edited.submission).unwrap()
    );
}

#[test]
fn rename_move_and_confirmation_preserve_original_evidence_and_unknown_keys() {
    let (_temp, base) = fixture();
    let id = base.scan.modules[0].id.clone();
    let commands: Vec<_> = base
        .scan
        .commands
        .iter()
        .filter(|c| c.modules.contains(&id))
        .map(|c| ReviewedCommand {
            directory: c.directory.clone(),
            command: c.command.clone(),
        })
        .collect();
    let edited = apply(
        &base,
        &[
            ReviewAction::Rename {
                node_id: id.clone(),
                title: "人工名称".into(),
            },
            ReviewAction::Move {
                node_id: id.clone(),
                x: -120,
                y: 370,
            },
            ReviewAction::Commands {
                module_id: id.clone(),
                commands: commands.clone(),
            },
        ],
    )
    .unwrap();
    let dotted: Vec<_> = commands
        .iter()
        .map(|command| ReviewedCommand {
            directory: if command.directory.is_empty() {
                ".".into()
            } else {
                command.directory.clone()
            },
            command: command.command.clone(),
        })
        .collect();
    let confirmed = apply(
        &base,
        &[ReviewAction::Commands {
            module_id: id.clone(),
            commands: dotted,
        }],
    )
    .unwrap();
    assert!(confirmed
        .review
        .as_ref()
        .unwrap()
        .pending_modules
        .is_empty());
    assert!(confirmed.review.as_ref().unwrap().modules[0]
        .commands
        .iter()
        .all(|c| c.directory != "."));
    let duplicate = vec![
        ReviewedCommand {
            directory: String::new(),
            command: "make test".into(),
        },
        ReviewedCommand {
            directory: ".".into(),
            command: "make test".into(),
        },
    ];
    assert!(apply(
        &base,
        &[ReviewAction::Commands {
            module_id: id.clone(),
            commands: duplicate
        }]
    )
    .is_err());
    identical_original(&base, &edited);
    let before = document(&base, &id);
    let after = document(&edited, &id);
    assert_eq!(
        before.get("initial_evidence"),
        after.get("initial_evidence")
    );
    assert_eq!(before.get("x-custom"), after.get("x-custom"));
    assert_eq!(after.get_str("title"), Some("人工名称"));
    assert!(edited.review.as_ref().unwrap().pending_modules.is_empty());
    assert_eq!(
        edited.review.as_ref().unwrap().confirmed_modules,
        vec![id.clone()]
    );
    let node = edited.view.nodes.iter().find(|n| n.id == id).unwrap();
    assert_eq!((node.x, node.y), (-120, 370));
    assert!(apply(
        &edited,
        &[ReviewAction::Move {
            node_id: id,
            x: 0,
            y: 0
        }]
    )
    .is_err());
}

#[test]
fn merge_preserves_source_coverage_remaps_references_and_invalidates_proof() {
    let (_temp, base) = fixture();
    let a = base.scan.modules[0].id.clone();
    let b = base.scan.modules[1].id.clone();
    let edited = apply(
        &base,
        &[ReviewAction::Merge {
            module_ids: vec![a.clone(), b.clone()],
            target_id: a.clone(),
            title: "合并模块".into(),
        }],
    )
    .unwrap();
    identical_original(&base, &edited);
    let state = edited.review.as_ref().unwrap();
    assert_eq!(state.modules.len(), 2);
    assert_eq!(state.pending_modules, vec![a.clone()]);
    assert_eq!(
        state
            .modules
            .iter()
            .find(|m| m.id == a)
            .unwrap()
            .files
            .len(),
        4
    );
    assert_eq!(
        document(&edited, "AC-result").get_str("module"),
        Some(a.as_str())
    );
    assert_eq!(
        document(&edited, "I-purpose")
            .get("allocated_to")
            .unwrap()
            .as_sequence()
            .unwrap()
            .len(),
        1
    );
    assert!(!edited
        .view
        .nodes
        .iter()
        .any(|n| n.id == "IF-call" || n.id == b));
    assert!(!edited.view.edges.iter().any(|e| e.from_node == e.to_node));
    assert!(edited
        .view
        .diagnostics
        .iter()
        .any(|s| s.contains("成为模块内部关系")));
    let module = document(&edited, &a);
    assert!(module.get("initial_evidence").is_none());
    assert_eq!(module.get_str("verification"), Some("notRun"));
    assert_eq!(module.get("x-custom"), document(&base, &a).get("x-custom"));
}

#[test]
fn split_keeps_each_file_once_and_exposes_reference_ambiguity() {
    let (_temp, base) = fixture();
    let a = base.scan.modules[0].id.clone();
    let selected = base.scan.modules[0].files[0].clone();
    let edited = apply(
        &base,
        &[ReviewAction::Split {
            module_id: a.clone(),
            new_id: "A-new".into(),
            title: "拆出部分".into(),
            files: vec![selected.clone()],
        }],
    )
    .unwrap();
    identical_original(&base, &edited);
    let state = edited.review.as_ref().unwrap();
    assert_eq!(state.modules.len(), 4);
    assert_eq!(state.pending_modules.len(), 2);
    assert_eq!(
        state
            .modules
            .iter()
            .filter(|m| m.files.contains(&selected))
            .count(),
        1
    );
    assert_eq!(
        state
            .modules
            .iter()
            .find(|m| m.id == "A-new")
            .unwrap()
            .files,
        vec![selected]
    );
    assert_eq!(
        document(&edited, "IF-call").get_str("from"),
        Some(a.as_str())
    );
    assert!(document(&edited, "I-purpose")
        .get("allocated_to")
        .unwrap()
        .as_sequence()
        .unwrap()
        .iter()
        .any(|id| id.as_str() == Some("A-new")));
    assert!(edited
        .view
        .diagnostics
        .iter()
        .any(|s| s.contains("需要重新审阅归属")));
    for files in [
        vec![],
        base.scan.modules[0].files.clone(),
        vec!["../secret.rs".into()],
        vec!["untracked.rs".into()],
    ] {
        assert!(apply(
            &base,
            &[ReviewAction::Split {
                module_id: a.clone(),
                new_id: "A-other".into(),
                title: "部分".into(),
                files
            }]
        )
        .is_err());
    }
}

#[test]
fn changed_or_removed_commands_require_actual_reverification_never_fabricate_pass() {
    let (_temp, base) = fixture();
    let a = base.scan.modules[0].id.clone();
    for commands in [
        vec![],
        vec![ReviewedCommand {
            directory: "client".into(),
            command: "make test".into(),
        }],
    ] {
        let edited = apply(
            &base,
            &[ReviewAction::Commands {
                module_id: a.clone(),
                commands,
            }],
        )
        .unwrap();
        let state = edited.review.as_ref().unwrap();
        assert_eq!(state.pending_modules, vec![a.clone()]);
        assert_eq!(state.confirmed_modules, vec![a.clone()]);
        assert!(document(&edited, &a).get("initial_evidence").is_none());
        identical_original(&base, &edited);
    }
    for command in [
        ReviewedCommand {
            directory: "../other".into(),
            command: "true".into(),
        },
        ReviewedCommand {
            directory: ".".into(),
            command: " ".into(),
        },
    ] {
        assert!(apply(
            &base,
            &[ReviewAction::Commands {
                module_id: a.clone(),
                commands: vec![command]
            }]
        )
        .is_err());
    }
}

#[test]
fn interfaces_are_editable_removable_and_never_human_approval() {
    let (_temp, base) = fixture();
    let a = base.scan.modules[0].id.clone();
    let c = base.scan.modules[2].id.clone();
    let actions = [
        ReviewAction::Interface {
            id: "IF-call".into(),
            from: a.clone(),
            to: c.clone(),
            title: "新契约".into(),
            contract: "必须返回结构化结果".into(),
        },
        ReviewAction::Rename {
            node_id: "IF-call".into(),
            title: "改名接口".into(),
        },
    ];
    let edited = apply(&base, &actions).unwrap();
    assert_eq!(document(&edited, "IF-call").get_str("to"), Some(c.as_str()));
    assert!(edited.view.edges.iter().any(|e| e.id == "interface-IF-call"
        && e.to_node == c
        && e.label.as_deref() == Some("接口：改名接口")));
    assert_eq!(edited.review.as_ref().unwrap().pending_modules.len(), 3);
    let removed = apply(
        &base,
        &[ReviewAction::RemoveInterface {
            id: "IF-call".into(),
        }],
    )
    .unwrap();
    assert!(!removed.view.nodes.iter().any(|n| n.id == "IF-call"));
    assert!(!removed
        .view
        .edges
        .iter()
        .any(|e| e.id == "interface-IF-call"));
    assert!(review::replay(
        &base,
        &actions,
        "process:towermd-cli",
        &format!("d-{}", "b".repeat(64)),
        "2026-10-21T00:00:00Z",
        &BTreeMap::new()
    )
    .is_err());
    for node in edited.view.nodes.iter().filter(|node| node.object_id.is_some()) {
        let doc = Document::parse(&node.content).unwrap();
        assert_ne!(doc.get_str("status"), Some("approved"));
        assert!(doc.get("approved_by").is_none());
    }
    assert!(serde_json::from_value::<ReviewAction>(
        json!({"kind":"rename","node_id":a,"title":"x","approved":true})
    )
    .is_err());
}

#[test]
fn existing_markdown_reference_preserves_exact_bytes_without_creating_object() {
    let (_temp, base) = fixture();
    let raw = "# Existing project doc\r\nNo frontmatter required.\r\n";
    let references = BTreeMap::from([("README.md".into(), raw.into())]);
    let action = ReviewAction::Reference {
        path: "README.md".into(),
        x: 45,
        y: 90,
    };
    let edited = review::replay(
        &base,
        &[action.clone(), action],
        "human:local",
        &format!("d-{}", "b".repeat(64)),
        "2026-10-21T00:00:00Z",
        &references,
    )
    .unwrap();
    let nodes: Vec<_> = edited
        .view
        .nodes
        .iter()
        .filter(|n| n.file.as_deref() == Some("README.md"))
        .collect();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].content, raw);
    assert!(nodes[0].object_id.is_none());
    assert!(nodes[0].object_type.is_none());
    assert_eq!(nodes[0].kind, "file");
    identical_original(&base, &edited);
    for path in [
        "../README.md",
        "/README.md",
        ".git/config.md",
        "missing.md",
        "README.txt",
    ] {
        assert!(apply(
            &base,
            &[ReviewAction::Reference {
                path: path.into(),
                x: 0,
                y: 0
            }]
        )
        .is_err());
    }
}

#[test]
fn new_object_paths_cannot_overwrite_another_id() {
    let (_temp, mut base) = fixture();
    let a = base.scan.modules[0].id.clone();
    let b = base.scan.modules[1].id.clone();
    base.view
        .nodes
        .iter_mut()
        .find(|node| node.id == "IF-call")
        .unwrap()
        .file = Some("arch/A-new.md".into());
    assert!(apply(
        &base,
        &[ReviewAction::Split {
            module_id: a.clone(),
            new_id: "A-new".into(),
            title: "新模块".into(),
            files: vec![base.scan.modules[0].files[0].clone()]
        }]
    )
    .unwrap_err()
    .contains("路径"));
    assert!(apply(
        &base,
        &[ReviewAction::Interface {
            id: "A-new".into(),
            from: a,
            to: b,
            title: "新接口".into(),
            contract: "契约".into()
        }]
    )
    .unwrap_err()
    .contains("路径"));
}

fn stored_review(root: &Path, base: &BaselineDraft) -> (Catalog, BaselineDraft) {
    use towermd_core::draft_store;
    let catalog = Catalog::load(root).unwrap();
    let original = draft_store::save(
        &catalog,
        &base.scan,
        base.submission.clone(),
        &base.producer,
    )
    .unwrap();
    let first = &base.scan.modules[0];
    let reviewed = draft_store::save_review(
        &catalog,
        &original.id,
        ReviewAction::Commands {
            module_id: first.id.clone(),
            commands: vec![ReviewedCommand {
                directory: String::new(),
                command: "printf 'S7 actual verification\\n'".into(),
            }],
        },
    )
    .unwrap();
    (catalog, reviewed)
}

#[test]
fn real_review_verification_is_private_durable_and_bound_to_current_input() {
    use towermd_core::verifier::VerificationStatus;
    use towermd_core::{draft_store, review_verification};
    let (temp, base) = fixture();
    let root = temp.path();
    let (catalog, reviewed) = stored_review(root, &base);
    let first = base.scan.modules[0].id.clone();
    let before_index = fs::read(catalog.common_dir.join("index")).unwrap();
    assert!(reviewed
        .review
        .as_ref()
        .unwrap()
        .pending_modules
        .contains(&first));
    let prepared = draft_store::prepare_verification(
        &catalog,
        &reviewed.id,
        &uuid::Uuid::now_v7().to_string(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(prepared.plan.revision, base.scan.revision);
    assert_eq!(
        prepared.plan.commands.len(),
        2,
        "the two untouched modules share one actual command"
    );
    let report = verifier::run(&prepared.plan, &AtomicBool::new(false)).unwrap();
    assert!(report
        .modules
        .iter()
        .all(|module| module.status == VerificationStatus::Passed));
    let verified =
        draft_store::persist_verification(&catalog, prepared.clone(), report.clone()).unwrap();
    assert_ne!(reviewed.id, verified.id);
    assert!(verified.review.as_ref().unwrap().pending_modules.is_empty());
    identical_original(&base, &verified);
    assert!(document(&verified, &first)
        .get("initial_evidence")
        .is_none());
    assert!(document(&verified, &first).get("review_evidence").is_some());
    assert_eq!(
        document(&verified, &first).get_str("verification"),
        Some("passed")
    );
    assert_eq!(
        fs::read(catalog.common_dir.join("index")).unwrap(),
        before_index
    );
    assert!(catalog.matches_current_source().unwrap());
    assert_eq!(
        draft_store::persist_verification(&catalog, prepared.clone(), report.clone())
            .unwrap()
            .id,
        verified.id
    );

    let moved = draft_store::save_review(
        &catalog,
        &verified.id,
        ReviewAction::Move {
            node_id: first.clone(),
            x: 700,
            y: 800,
        },
    )
    .unwrap();
    assert!(moved.review.as_ref().unwrap().verification.is_some());
    let changed = draft_store::save_review(
        &catalog,
        &moved.id,
        ReviewAction::Commands {
            module_id: first.clone(),
            commands: vec![ReviewedCommand {
                directory: String::new(),
                command: "false".into(),
            }],
        },
    )
    .unwrap();
    assert!(changed.review.as_ref().unwrap().verification.is_none());
    assert!(changed
        .review
        .as_ref()
        .unwrap()
        .pending_modules
        .contains(&first));
    assert_eq!(
        document(&changed, &first).get_str("verification"),
        Some("notRun")
    );

    let run_id = verified
        .review
        .as_ref()
        .unwrap()
        .verification
        .as_ref()
        .unwrap()
        .run_id
        .clone();
    fs::remove_dir_all(catalog.common_dir.join("towermd/verify").join(run_id)).unwrap();
    let rebuilt = draft_store::load(&catalog, &verified.id).unwrap();
    assert_eq!(
        serde_json::to_value(&verified).unwrap(),
        serde_json::to_value(&rebuilt).unwrap()
    );
    assert!(review_verification::validate_logs(&rebuilt, &catalog.common_dir).is_err());
    // An already saved result remains idempotent after local cache eviction.
    assert_eq!(
        draft_store::persist_verification(&catalog, prepared, report)
            .unwrap()
            .id,
        verified.id
    );
}

#[test]
fn reviewed_verification_rejects_wrong_commands_digest_and_changed_source() {
    use towermd_core::{draft_store, review_verification};
    let (temp, base) = fixture();
    let root = temp.path();
    let (catalog, reviewed) = stored_review(root, &base);
    let prepared = draft_store::prepare_verification(
        &catalog,
        &reviewed.id,
        &uuid::Uuid::now_v7().to_string(),
        Limits::default(),
    )
    .unwrap();
    let report = verifier::run(&prepared.plan, &AtomicBool::new(false)).unwrap();
    let mut forged = report.clone();
    forged.commands[0].argv.push("unrequested argument".into());
    assert!(draft_store::persist_verification(&catalog, prepared.clone(), forged).is_err());
    let mut draft = reviewed.clone();
    let forged_input = review_verification::ReviewVerification {
        parent_id: reviewed.id.clone(),
        input_sha256: "0".repeat(64),
        run_id: prepared.plan.id.clone(),
        limits: prepared.plan.limits.clone(),
        report: report.clone(),
    };
    assert!(review_verification::apply(&mut draft, forged_input).is_err());
    assert_eq!(
        serde_json::to_value(&reviewed).unwrap(),
        serde_json::to_value(&draft).unwrap()
    );
    fs::write(root.join("README.md"), "source moved\n").unwrap();
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-qm", "move source"]);
    assert!(draft_store::persist_verification(&catalog, prepared, report).is_err());
    assert!(draft_store::prepare_verification(
        &Catalog::load(root).unwrap(),
        &reviewed.id,
        &uuid::Uuid::now_v7().to_string(),
        Limits::default()
    )
    .is_err());
}

#[test]
fn real_review_failures_cancellation_and_no_tests_never_become_passed() {
    use towermd_core::draft_store;
    use towermd_core::verifier::VerificationStatus;
    let (temp, base) = fixture();
    let (catalog, reviewed) = stored_review(temp.path(), &base);
    let first = base.scan.modules[0].id.clone();
    let second = base.scan.modules[1].id.clone();
    let failing = draft_store::save_review(
        &catalog,
        &reviewed.id,
        ReviewAction::Commands {
            module_id: first.clone(),
            commands: vec![ReviewedCommand {
                directory: String::new(),
                command: "exit 7".into(),
            }],
        },
    )
    .unwrap();
    let no_tests = draft_store::save_review(
        &catalog,
        &failing.id,
        ReviewAction::Commands {
            module_id: second.clone(),
            commands: vec![],
        },
    )
    .unwrap();
    let prepared = draft_store::prepare_verification(
        &catalog,
        &no_tests.id,
        &uuid::Uuid::now_v7().to_string(),
        Limits::default(),
    )
    .unwrap();
    let report = verifier::run(&prepared.plan, &AtomicBool::new(false)).unwrap();
    let failed = draft_store::persist_verification(&catalog, prepared, report).unwrap();
    assert_eq!(
        document(&failed, &first).get_str("verification"),
        Some("failed")
    );
    assert_eq!(
        document(&failed, &second).get_str("verification"),
        Some("no_tests")
    );
    let prepared = draft_store::prepare_verification(
        &catalog,
        &failed.id,
        &uuid::Uuid::now_v7().to_string(),
        Limits::default(),
    )
    .unwrap();
    let report = verifier::run(&prepared.plan, &AtomicBool::new(true)).unwrap();
    assert!(!report
        .commands
        .iter()
        .any(|command| command.status == VerificationStatus::Passed));
    let cancelled = draft_store::persist_verification(&catalog, prepared, report).unwrap();
    assert!(cancelled
        .review
        .as_ref()
        .unwrap()
        .pending_modules
        .contains(&first));
    assert_ne!(
        document(&cancelled, &first).get_str("verification"),
        Some("passed")
    );
}

#[test]
fn baseline_publication_requires_real_evidence_and_exact_human_approved_bytes() {
    use towermd_core::hub::{AppendReceipt, Hub};
    use towermd_core::hub_protocol::{HubProtocol, NewEvent};
    use towermd_core::{baseline_publication, draft_store, main_writer};
    let (temp, base) = fixture();
    let root = temp.path();
    let (catalog, mut reviewed) = stored_review(root, &base);
    let request_id = uuid::Uuid::now_v7().to_string();
    assert!(baseline_publication::prepare(
        &reviewed,
        &catalog.common_dir,
        &request_id,
        "B0",
        "2026-10-21T01:00:00Z"
    )
    .is_err());
    for module in reviewed.review.clone().unwrap().modules {
        reviewed = draft_store::save_review(
            &catalog,
            &reviewed.id,
            ReviewAction::Commands {
                module_id: module.id,
                commands: module.commands,
            },
        )
        .unwrap();
    }
    let prepared = draft_store::prepare_verification(
        &catalog,
        &reviewed.id,
        &uuid::Uuid::now_v7().to_string(),
        Limits::default(),
    )
    .unwrap();
    let report = verifier::run(&prepared.plan, &AtomicBool::new(false)).unwrap();
    let verified = draft_store::persist_verification(&catalog, prepared, report).unwrap();
    let publication = baseline_publication::prepare(
        &verified,
        &catalog.common_dir,
        &request_id,
        "B0",
        "2026-10-21T01:00:00Z",
    )
    .unwrap();
    let canvas: serde_json::Value = serde_json::from_slice(&publication.files["docs/tower/tower.canvas"]).unwrap();
    let project = canvas["nodes"].as_array().unwrap().iter().find(|node| node["towermd"]["region"] == "project").unwrap();
    assert_eq!(project["type"], "group");
    assert!(project.get("file").is_none());
    assert!(project["towermd"].get("obj").is_none());
    assert_eq!(project["x"], -380);
    let mut hub = Hub::open(&catalog.common_dir.join("publication-test-hub")).unwrap();
    let AppendReceipt::Event(approval) = hub
        .append(
            NewEvent {
                eid: request_id,
                host: "local".into(),
                actor: "human:local".into(),
                kind: "baseline.approved".into(),
                layer: "L1".into(),
                obj: "B0".into(),
                data: serde_json::to_value(&publication).unwrap(),
                guards: vec![],
            },
            1000,
        )
        .unwrap()
    else {
        panic!()
    };
    publication.check_approval(&approval).unwrap();
    let mut bad = publication.clone();
    bad.files.values_mut().next().unwrap().push(b'!');
    assert!(bad.check_approval(&approval).is_err());
    let mut fake = (*approval).clone();
    fake.actor = "process:towermd-cli".into();
    assert!(publication.check_approval(&fake).is_err());
    git(root, &["switch", "-qc", "qa"]);
    let request = main_writer::WriteRequest {
        docs_dir: "docs/tower".into(),
        epoch: 1,
        kind: main_writer::WriteKind::Slow {
            expected_base: publication.source_revision.clone(),
            tag: Some("refs/tags/towermd/B0".into()),
        },
        changes: publication
            .files
            .iter()
            .map(|(p, b)| (p.clone(), Some(b.clone())))
            .collect(),
        message: "Approve B0\n\nTowermd-Baseline: B0".into(),
    };
    let main_writer::WriteOutcome::Written { commit, .. } =
        main_writer::write(root, &request, || Ok(())).unwrap()
    else {
        panic!()
    };
    publication.prove_commit(root, &commit, &approval).unwrap();
    let published = towermd_core::project_view::load(root).unwrap();
    let project = published.nodes.iter().find(|node| node.region.as_deref() == Some("project")).unwrap();
    assert_eq!(project.kind, "group");
    assert_eq!((project.x, project.y, project.width, project.height), (-380, 0, 320, 240));

    assert!(publication
        .prove_commit(root, &publication.source_revision, &approval)
        .is_err());
    git(
        root,
        &["tag", "-f", "towermd/B0", &publication.source_revision],
    );
    assert!(publication.prove_commit(root, &commit, &approval).is_err());
}
