mod support;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};
use tempfile::TempDir;
use towermd_core::{
    baseline_draft::{
        apply_edit, freeze, from_scan, publication_content_hash, publication_freeze, DraftBundle,
        DraftEdit, DraftObject, VerifyCommand,
    },
    baseline_schema::BaselineSubmit,
    catalog::Catalog,
    coldstart::{capture_scan_input, ScanOptions},
    coldstart_evidence::{read_completed, CompletedColdstart, PreparedColdstart},
    okf::Document,
    project_session::ProjectSession,
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
    verifier::{verify_sealed, VerificationRequest},
};
const NOW: u64 = 1_800_000_000_000;
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn fixture() -> (TempDir, TempDir, Catalog, CompletedColdstart) {
    fixture_with_retained_references(false)
}

#[test]
fn annotation_cards_and_relations_freeze_in_the_same_canvas_without_changing_objects_or_source() {
    use towermd_core::baseline_draft::{AnnotationContent, AnnotationNode, AnnotationRelation};
    let (root, _runtime, catalog, completed) = fixture();
    let before = from_scan(&catalog, &completed, submit(&completed)).unwrap();
    let original = freeze(&catalog, &before).unwrap();
    let refs = support::git(root.path(), &["show-ref"]);
    let status = support::git(root.path(), &["status", "--porcelain"]);
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let mut edited = before.clone();
    for (id, content) in [
        (
            "free-note",
            AnnotationContent::Text {
                text: "自由概念".into(),
            },
        ),
        (
            "free-link",
            AnnotationContent::Link {
                url: "https://example.invalid/reference".into(),
            },
        ),
        (
            "free-group",
            AnnotationContent::Group {
                label: "概念分组".into(),
            },
        ),
    ] {
        edited = apply_edit(
            &edited,
            &DraftEdit::PutAnnotation {
                node: AnnotationNode {
                    id: id.into(),
                    x: -400,
                    y: 300,
                    width: 300,
                    height: 160,
                    content,
                },
            },
        )
        .unwrap();
    }
    edited = apply_edit(
        &edited,
        &DraftEdit::PutRelation {
            relation: AnnotationRelation {
                id: "note-relation".into(),
                from: "free-note".into(),
                to: "free-link".into(),
                label: "引用".into(),
            },
        },
    )
    .unwrap();
    let frozen = freeze(&catalog, &edited).unwrap();
    assert_ne!(frozen.content_hash, original.content_hash);
    assert_eq!(edited.objects, before.objects);
    assert_eq!(
        frozen.files.keys().collect::<Vec<_>>(),
        original.files.keys().collect::<Vec<_>>()
    );
    for (path, file) in &original.files {
        if !path.ends_with("/tower.canvas") {
            assert_eq!(&frozen.files[path], file);
        }
    }
    let canvas: Value =
        serde_json::from_str(&frozen.files["docs/tower/tower.canvas"].text).unwrap();
    assert!(canvas["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["id"] == "free-note" && node["type"] == "text"));
    assert_eq!(support::git(root.path(), &["show-ref"]), refs);
    assert_eq!(
        support::git(root.path(), &["status", "--porcelain"]),
        status
    );
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
}
fn fixture_with_retained_references(
    retained: bool,
) -> (TempDir, TempDir, Catalog, CompletedColdstart) {
    let root = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    support::git(root.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(
        root.path(),
        &towermd_core::init::InitOptions { docs_dir: None },
    )
    .unwrap();
    write(root.path(), "alpha/a.py", "# alpha\n");
    write(root.path(), "beta/b.py", "# beta\n");
    write(
        root.path(),
        "docs/tower/arch/occupied.md",
        "# ordinary, never take over\n",
    );
    write(
        root.path(),
        "docs/tower/extra.md",
        "# ordinary, protect directory occupancy\n",
    );
    write(
        root.path(),
        "outside.md",
        "---\ntype: Intent\nid: outside\n---\nsource outside docs_dir\n",
    );
    write(root.path(),"docs/tower/arch/Existing.md","---\ntype: Module\nid: Existing\npaths: ['alpha/**']\nx-existing: {keep: [2, 1]}\n---\nold body\n");
    write(
        root.path(),
        "docs/tower/arch/future.md",
        "---\ntype: FutureThing\nid: future\nx-future: true\n---\nuntouched\r\n",
    );
    write(root.path(), "docs/tower/artifacts/derived.txt", "derived\n");
    if retained {
        write(root.path(),"docs/tower/arch/OldPeer.md","---\r\ntype: Module\r\nid: OldPeer\r\npaths: ['beta/**']\r\nx-old: retained\r\n---\r\nold peer");
        write(root.path(),"docs/tower/specs/OldAC.md","---\ntype: Acceptance\nid: OldAC\nmodule: Existing\ncriteria: [works]\nx-old: retained\n---\nold acceptance");
        write(root.path(),"docs/tower/arch/OldIF.md","---\ntype: Interface\nid: OldIF\nfrom: Existing\nto: OldPeer\nx-old: retained\n---\nold interface");
        write(
            root.path(),
            "docs/tower/intents/OldIntent.md",
            "---\r\ntype: Intent\r\nid: OldIntent\r\nx-old: retained\r\n---\r\nold intent",
        );
        write(
            root.path(),
            "docs/tower/evidence/Historical.md",
            "---\ntype: Evidence\nid: Historical\n---\naudit only\n",
        );
    }
    support::git(root.path(), &["add", "."]);
    support::git(root.path(), &["commit", "-qm", "actual fixed scan source"]);
    support::git(root.path(), &["switch", "-qc", "scan-task"]);
    let catalog = Catalog::load(root.path()).unwrap();
    let input = capture_scan_input(&catalog, &ScanOptions::default()).unwrap();
    let prepared = PreparedColdstart::prepare(input, "scan-fixture").unwrap();
    let input = prepared.input();
    let sealed = verify_sealed(&VerificationRequest {
        root: input.root.clone(),
        common_dir: input.common_dir.clone(),
        revision: input.revision.clone(),
        commands: prepared.commands().unwrap(),
        timeout: Duration::from_secs(5),
        log_root: input.common_dir.join("towermd/cache/verification"),
        cancelled: Arc::new(AtomicBool::new(false)),
    })
    .unwrap();
    let frozen = prepared.freeze(&sealed, "actual-host", NOW).unwrap();
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &runtime.path().join("hub"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            NOW,
        )
        .unwrap(),
    );
    assert!(
        read_completed(&catalog, &outbox.read_inputs().unwrap(), "scan-fixture")
            .unwrap()
            .is_none()
    );
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &frozen, NOW).unwrap();
    let completed = read_completed(&catalog, &outbox.read_inputs().unwrap(), "scan-fixture")
        .unwrap()
        .unwrap();
    (root, runtime, catalog, completed)
}
fn submit(completed: &CompletedColdstart) -> BaselineSubmit {
    let modules=completed.scan()["modules"].as_array().unwrap().iter().map(|m|{
        let id=m["id"].as_str().unwrap();
        let fields=serde_yaml::to_string(&json!({"type":"Module","id":id,"title":id,"paths":m["paths"],"verify":[],"interfaces":[],"x-future":{"values":[2,1]}})).unwrap();
        DraftObject{id:id.into(),path:format!("arch/{id}.md"),text:format!("---\n{fields}---\nbody without final newline")}
    }).collect::<Vec<_>>();
    assert!(modules.len() >= 2);
    let interface=DraftObject{id:"IF".into(),path:"arch/IF.md".into(),text:format!("---\ntype: Interface\nid: IF\nfrom: {}\nto: {}\ncontract: actual-api.md\n---\ninterface body",modules[0].id,modules[1].id)};
    let acceptance = DraftObject {
        id: "AC".into(),
        path: "specs/AC.md".into(),
        text: format!(
            "---\ntype: Acceptance\nid: AC\nmodule: {}\ncriteria: [works]\n---\nacceptance body",
            modules[0].id
        ),
    };
    BaselineSubmit {
        scan_id: completed.scan_id().into(),
        project_id: completed.input().project_id.clone(),
        source_revision: completed.input().revision.clone(),
        intents: vec![],
        modules,
        interfaces: vec![interface],
        acceptances: vec![acceptance],
        questions: vec!["second".into(), "first".into()],
        risks: vec!["risk".into()],
    }
}
fn hash(catalog: &Catalog, bundle: &DraftBundle) -> String {
    freeze(catalog, bundle).unwrap().content_hash
}

#[test]
fn actual_completed_scan_is_required_and_b0_freeze_preserves_exact_bytes_and_trace() {
    let (root, _runtime, catalog, completed) = fixture();
    let mut submitted = submit(&completed);
    submitted.modules[0].text = submitted.modules[0].text.replace('\n', "\r\n");
    let original = submitted.modules[0].text.clone();
    let id = submitted.modules[0].id.clone();
    let status = support::git(root.path(), &["status", "--porcelain"]);
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let refs = support::git(root.path(), &["show-ref"]);
    let bundle = from_scan(&catalog, &completed, submitted).unwrap();
    assert_eq!(bundle.id, completed.scan_id());
    assert_eq!(bundle.target_baseline, "towermd/B0");
    assert!(completed
        .evidence()
        .iter()
        .all(|e| e["result"] == "no_tests"));
    assert!(!completed.scan()["candidate_tasks"]
        .as_array()
        .unwrap()
        .is_empty());
    let frozen = freeze(&catalog, &bundle).unwrap();
    assert_eq!(
        frozen.files[&format!("docs/tower/arch/{id}.md")]
            .text
            .as_bytes(),
        original.as_bytes()
    );
    assert_eq!(frozen.files.len(), bundle.objects.len() + 1);
    assert!(frozen.files.keys().all(|path| !path.contains("state/")
        && !path.contains("manifest")
        && !path.contains("Evidence")));
    for path in [
        "docs/tower/arch/occupied.md",
        "docs/tower/extra.md",
        "outside.md",
        ".towermd/config.yaml",
    ] {
        assert!(frozen.base_directory.contains_key(path), "{path}");
    }
    assert!(!frozen
        .base_directory
        .contains_key("docs/tower/state/derived.txt"));
    assert!(!frozen
        .base_directory
        .contains_key("docs/tower/artifacts/derived.txt"));
    let canvas: Value = serde_json::from_str(&bundle.canvas_text).unwrap();
    for object in bundle
        .objects
        .values()
        .filter(|o| Document::parse(&o.text).unwrap().doc_type() == Some("Module"))
    {
        let node = canvas["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["towermd"]["obj"] == object.id)
            .unwrap();
        assert_eq!(node["file"], object.path);
    }
    assert!(publication_freeze(&frozen).is_ok());
    assert_eq!(
        support::git(root.path(), &["status", "--porcelain"]),
        status
    );
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    assert_eq!(support::git(root.path(), &["show-ref"]), refs);
}

#[test]
fn completed_scan_submit_accepts_linked_worktree_and_rejects_another_clone() {
    let (root, _runtime, catalog, completed) = fixture();
    let linked = tempfile::tempdir().unwrap();
    support::git(
        root.path(),
        &[
            "worktree",
            "add",
            "-b",
            "linked-submit",
            linked.path().to_str().unwrap(),
            "main",
        ],
    );
    let linked_catalog = Catalog::load(linked.path()).unwrap();
    assert_ne!(linked_catalog.root, catalog.root);
    assert_eq!(linked_catalog.common_dir, catalog.common_dir);
    assert!(from_scan(&linked_catalog, &completed, submit(&completed)).is_ok());
    let clone = tempfile::tempdir().unwrap();
    support::git(
        root.path(),
        &[
            "clone",
            "--no-local",
            "-b",
            "main",
            root.path().to_str().unwrap(),
            clone.path().to_str().unwrap(),
        ],
    );
    let clone_catalog = Catalog::load(clone.path()).unwrap();
    assert_eq!(
        clone_catalog.config.project_id(),
        catalog.config.project_id()
    );
    assert!(from_scan(&clone_catalog, &completed, submit(&completed)).is_err());
}

#[test]
fn freeze_rejects_deletion_of_protected_slow_layer_outside_configured_directory() {
    let (_root, _runtime, catalog, completed) = fixture();
    let mut bundle = from_scan(&catalog, &completed, submit(&completed)).unwrap();
    bundle.deletions = vec!["outside.md".into()];
    assert!(freeze(&catalog, &bundle).is_err());
}

#[test]
fn removing_interface_keeps_a_valid_freezable_bundle() {
    let (_root, _runtime, catalog, completed) = fixture();
    let mut submitted = submit(&completed);
    for module in &mut submitted.modules {
        let mut doc = Document::parse(&module.text).unwrap();
        doc.set("interfaces", serde_yaml::to_value(vec!["IF"]).unwrap())
            .unwrap();
        module.text = doc.to_text();
    }
    let bundle = from_scan(&catalog, &completed, submitted).unwrap();
    let removed = apply_edit(
        &bundle,
        &DraftEdit::RemoveInterface {
            interface: "IF".into(),
        },
    )
    .unwrap();
    assert!(freeze(&catalog, &removed).is_ok());
}

#[test]
fn retained_source_references_are_rewritten_when_a_submitted_module_is_merged() {
    let (_root, _runtime, catalog, completed) = fixture_with_retained_references(true);
    let mut submitted = submit(&completed);
    let old = submitted.modules[0].id.clone();
    submitted.modules[0].id = "Existing".into();
    submitted.modules[0].path = "arch/Existing.md".into();
    submitted.modules[0].text = submitted.modules[0].text.replace(&old, "Existing");
    submitted.interfaces.clear();
    submitted.acceptances[0].text = submitted.acceptances[0].text.replace(&old, "Existing");
    let keep = submitted.modules[1].id.clone();
    let bundle = from_scan(&catalog, &completed, submitted).unwrap();
    let merged = apply_edit(
        &bundle,
        &DraftEdit::Merge {
            modules: vec!["Existing".into(), keep.clone()],
            keep: keep.clone(),
            title: "Merged".into(),
        },
    )
    .unwrap();
    let frozen = freeze(&catalog, &merged).unwrap();
    // Apply only the frozen patch to the retained source view: an omitted OldAC
    // must not survive with a reference to the now-deleted Existing module.
    let effective_ac = frozen
        .files
        .get("docs/tower/specs/OldAC.md")
        .map(|file| file.text.clone())
        .unwrap_or_else(|| catalog.objects["OldAC"].to_text());
    assert_eq!(
        Document::parse(&effective_ac).unwrap().get_str("module"),
        Some(keep.as_str())
    );
    assert_eq!(
        Document::parse(&merged.objects["OldIF"].text)
            .unwrap()
            .get_str("from"),
        Some(keep.as_str())
    );
    for id in ["OldPeer", "OldIntent"] {
        assert_eq!(
            bundle.objects[id].text.as_bytes(),
            catalog.objects[id].to_text().as_bytes()
        );
    }
    assert!(!bundle.objects.contains_key("Historical"));
    assert!(!bundle.objects.contains_key("future"));
    assert!(!frozen
        .files
        .contains_key("docs/tower/evidence/Historical.md"));
}

#[test]
fn manual_bundle_cannot_delete_a_module_still_referenced_by_retained_source() {
    let (_root, _runtime, catalog, completed) = fixture_with_retained_references(true);
    let mut bundle = from_scan(&catalog, &completed, submit(&completed)).unwrap();
    for id in ["Existing", "OldPeer", "OldAC", "OldIF", "OldIntent"] {
        bundle.objects.remove(id);
    }
    bundle.deletions = vec!["docs/tower/arch/Existing.md".into()];
    assert!(freeze(&catalog, &bundle).is_err());
}

#[test]
fn submit_rejects_wrong_identity_shape_type_layer_paths_and_ordinary_document_takeover() {
    let (_root, _runtime, catalog, completed) = fixture();
    let good = submit(&completed);
    for field in ["scan", "project", "revision"] {
        let mut bad = good.clone();
        match field {
            "scan" => bad.scan_id = "other".into(),
            "project" => bad.project_id = uuid::Uuid::now_v7().to_string(),
            _ => bad.source_revision = "b".repeat(40),
        }
        assert!(from_scan(&catalog, &completed, bad).is_err(), "{field}");
    }
    for path in [
        "../escape.md",
        "/absolute.md",
        "arch/../escape.md",
        ".git/secret.md",
        "docs/tower/arch/double-prefix.md",
        "arch/occupied.md",
    ] {
        let mut bad = good.clone();
        bad.modules[0].path = path.into();
        assert!(from_scan(&catalog, &completed, bad).is_err(), "{path}");
    }
    let mut bad = good.clone();
    bad.modules[0].text = bad.modules[0]
        .text
        .replace("type: Module", "type: FutureThing");
    assert!(from_scan(&catalog, &completed, bad).is_err());
    let mut bad = good.clone();
    bad.modules[0].text = bad.modules[0].text.replacen("---\n", "---\nlayer: L4\n", 1);
    assert!(from_scan(&catalog, &completed, bad).is_err());
    let mut bad = good.clone();
    bad.modules.push(bad.modules[0].clone());
    assert!(from_scan(&catalog, &completed, bad).is_err());
    let mut bad = good.clone();
    bad.interfaces[0].text = bad.interfaces[0]
        .text
        .replace(&format!("from: {}", good.modules[0].id), "from: absent");
    assert!(from_scan(&catalog, &completed, bad).is_err());
    let mut bad = good.clone();
    bad.acceptances[0].text = bad.acceptances[0]
        .text
        .replace(&format!("module: {}", good.modules[0].id), "module: absent");
    assert!(from_scan(&catalog, &completed, bad).is_err());
}

#[test]
fn shared_freeze_hash_covers_semantic_edits_deletions_directory_and_ordered_questions() {
    let (_root, _runtime, catalog, completed) = fixture();
    let bundle = from_scan(&catalog, &completed, submit(&completed)).unwrap();
    let original = hash(&catalog, &bundle);
    let first = bundle
        .objects
        .iter()
        .find(|(_, o)| Document::parse(&o.text).unwrap().doc_type() == Some("Module"))
        .unwrap()
        .0
        .clone();
    let reverse: BTreeMap<_, _> = bundle
        .objects
        .iter()
        .rev()
        .map(|(id, obj)| (id.clone(), obj.clone()))
        .collect();
    let mut reordered = bundle.clone();
    reordered.objects = reverse;
    assert_eq!(hash(&catalog, &reordered), original);
    let mut edited = bundle.clone();
    edited
        .objects
        .get_mut(&first)
        .unwrap()
        .text
        .push_str("body changed");
    assert_ne!(hash(&catalog, &edited), original);
    let mut edited = bundle.clone();
    edited.objects.get_mut(&first).unwrap().text = edited.objects[&first]
        .text
        .replace("values:", "new-values:");
    assert_ne!(hash(&catalog, &edited), original);
    let edited = apply_edit(
        &bundle,
        &DraftEdit::Move {
            node: first.clone(),
            x: 999,
            y: 888,
        },
    )
    .unwrap();
    assert_ne!(hash(&catalog, &edited), original);
    let edited = apply_edit(
        &bundle,
        &DraftEdit::ConfirmVerify {
            module: first,
            commands: vec![VerifyCommand {
                cwd: ".".into(),
                argv: vec!["cargo".into(), "test".into(), "--locked".into()],
            }],
        },
    )
    .unwrap();
    assert_ne!(hash(&catalog, &edited), original);
    let mut edited = bundle.clone();
    edited
        .deletions
        .push("docs/tower/arch/not-published.md".into());
    assert_ne!(hash(&catalog, &edited), original);
    let mut edited = bundle.clone();
    edited.questions.reverse();
    assert_ne!(hash(&catalog, &edited), original);
    let frozen = freeze(&catalog, &bundle).unwrap();
    let mut publication = publication_freeze(&frozen).unwrap();
    let texts = frozen
        .files
        .iter()
        .map(|(p, f)| (p.clone(), f.text.clone()))
        .collect();
    publication
        .base_directory
        .get_mut("docs/tower/extra.md")
        .unwrap()
        .replace_range(7.., &"b".repeat(40));
    assert_ne!(
        publication_content_hash(&publication, &texts).unwrap(),
        original
    );
}

#[test]
fn freeze_enforces_initial_target_and_refuses_arbitrary_deletion_and_existing_b0() {
    let (root, _runtime, catalog, completed) = fixture();
    let bundle = from_scan(&catalog, &completed, submit(&completed)).unwrap();
    let mut bad = bundle.clone();
    bad.target_baseline = "towermd/B1".into();
    assert!(freeze(&catalog, &bad).is_err());
    let mut bad = bundle.clone();
    bad.deletions = vec!["docs/tower/extra.md".into()];
    assert!(freeze(&catalog, &bad).is_err());
    let mut bad = bundle.clone();
    bad.docs_dir = "hijacked".into();
    assert!(freeze(&catalog, &bad).is_err());
    let mut bad = bundle.clone();
    bad.source_revision = "b".repeat(40);
    assert!(freeze(&catalog, &bad).is_err());
    support::git(root.path(), &["tag", "towermd/B0"]);
    assert!(freeze(&catalog, &bundle).is_err());
    assert!(from_scan(&catalog, &completed, submit(&completed)).is_err());
}

#[test]
fn existing_unknown_fields_and_opaque_canvas_survive_without_publishing_unknown_objects() {
    let (_root, _runtime, catalog, completed) = fixture();
    let mut submitted = submit(&completed);
    let old_id = submitted.modules[0].id.clone();
    submitted.modules[0].id = "Existing".into();
    submitted.modules[0].path = "arch/Existing.md".into();
    submitted.modules[0].text = submitted.modules[0].text.replace(&old_id, "Existing");
    submitted.interfaces[0].text = submitted.interfaces[0].text.replace(&old_id, "Existing");
    submitted.acceptances[0].text = submitted.acceptances[0].text.replace(&old_id, "Existing");
    let mut bundle = from_scan(&catalog, &completed, submitted).unwrap();
    let existing = Document::parse(&bundle.objects["Existing"].text).unwrap();
    assert_eq!(
        existing.get("x-existing"),
        catalog.objects["Existing"].get("x-existing")
    );
    assert_eq!(existing.body(), "body without final newline");
    let future = DraftObject {
        id: "future".into(),
        path: "docs/tower/arch/future.md".into(),
        text: catalog.objects["future"].to_text(),
    };
    bundle.objects.insert("future".into(), future.clone());
    let mut canvas: Value = serde_json::from_str(&bundle.canvas_text).unwrap();
    let opaque =
        json!({"id":"future-node","type":"not-yet-understood","future":{"values":[3,2,1]}});
    canvas["nodes"].as_array_mut().unwrap().push(opaque.clone());
    canvas["x-unknown"] = json!({"keep":true});
    bundle.canvas_text = canvas.to_string();
    let frozen = freeze(&catalog, &bundle).unwrap();
    assert_eq!(frozen.bundle.objects["future"], future);
    assert!(!frozen.files.contains_key("docs/tower/arch/future.md"));
    assert!(frozen
        .base_directory
        .contains_key("docs/tower/arch/future.md"));
    let frozen_canvas: Value =
        serde_json::from_str(&frozen.files["docs/tower/tower.canvas"].text).unwrap();
    assert!(frozen_canvas["nodes"].as_array().unwrap().contains(&opaque));
    assert_eq!(frozen_canvas["x-unknown"], json!({"keep":true}));
    bundle
        .objects
        .get_mut("future")
        .unwrap()
        .text
        .push_str("not allowed");
    assert!(freeze(&catalog, &bundle).is_err());
}

#[test]
fn fixed_source_survives_actual_snapshot_and_code_only_main_advancement() {
    let (root, runtime, catalog, completed) = fixture();
    let submitted = submit(&completed);
    let bundle = from_scan(&catalog, &completed, submitted.clone()).unwrap();
    let original = freeze(&catalog, &bundle).unwrap();
    let outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let session = Mutex::new(
        ProjectSession::open(
            &runtime.path().join("hub"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            NOW + 1,
        )
        .unwrap(),
    );
    towermd_core::snapshot_service::SnapshotWriter::default()
        .write(&session, &catalog, "actual-host", true, || Ok(NOW + 2))
        .unwrap();
    let current = Catalog::load(root.path()).unwrap();
    assert_ne!(current.revision, catalog.revision);
    assert_eq!(freeze(&current, &bundle).unwrap(), original);
    assert_eq!(
        from_scan(&current, &completed, submitted.clone()).unwrap(),
        bundle
    );
    drop(session);
    drop(outbox);
    support::git(root.path(), &["switch", "main"]);
    write(
        root.path(),
        "alpha/a.py",
        "# code advanced after fixed scan\n",
    );
    support::git(root.path(), &["add", "alpha/a.py"]);
    support::git(root.path(), &["commit", "-qm", "pure code advance"]);
    support::git(root.path(), &["switch", "scan-task"]);
    let current = Catalog::load(root.path()).unwrap();
    assert_eq!(freeze(&current, &bundle).unwrap(), original);
    assert_eq!(from_scan(&current, &completed, submitted).unwrap(), bundle);
    assert_eq!(bundle.source_revision, completed.input().revision);
}

#[test]
fn current_protected_content_ordinary_occupancy_external_l0_and_config_invalidate_new_freeze() {
    for (path, text) in [
        ("docs/tower/extra.md", "# changed ordinary docs occupancy\n"),
        ("docs/tower/new-ordinary.md", "# new ordinary document\n"),
        (
            "new-protected.md",
            "---\ntype: Intent\nid: new-outside\n---\nnew L0\n",
        ),
        (".towermd/config.yaml", ""),
    ] {
        let (root, _runtime, catalog, completed) = fixture();
        let submitted = submit(&completed);
        let bundle = from_scan(&catalog, &completed, submitted.clone()).unwrap();
        support::git(root.path(), &["switch", "main"]);
        if path == ".towermd/config.yaml" {
            let mut value: serde_yaml::Value =
                serde_yaml::from_str(&catalog.config.to_yaml()).unwrap();
            value["future-policy"] = serde_yaml::Value::from("changed");
            write(root.path(), path, &serde_yaml::to_string(&value).unwrap());
        } else {
            write(root.path(), path, text);
        }
        support::git(root.path(), &["add", path]);
        support::git(root.path(), &["commit", "-qm", "protected source advance"]);
        support::git(root.path(), &["switch", "scan-task"]);
        let current = Catalog::load(root.path()).unwrap();
        assert!(freeze(&current, &bundle).is_err(), "{path}");
        assert!(
            from_scan(&current, &completed, submitted).is_err(),
            "{path}"
        );
    }
}
