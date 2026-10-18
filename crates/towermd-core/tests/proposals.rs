use std::{collections::BTreeMap, fs};
use towermd_core::proposals::{validate_proposal, Alternative, ProposalInput, ProposalSource};
use towermd_core::{
    baseline_draft::DraftEdit,
    baseline_review::review_baseline_shared,
    catalog::Catalog,
    proposals::{analyze_proposal, attach_proposal, validate_proposal_subject},
    recovery_contract::{baseline::Verdict, recorded::RequestBody},
    request_ledger::{save_draft_shared, DraftSaveAction, PreparedDraftSaveRequest},
    snapshot_service::PublicationReport,
    tasks::{Task, TaskSpec},
};
#[path = "support/baseline_review.rs"]
mod fixture;
use fixture::{git, NOW};

fn input() -> ProposalInput {
    ProposalInput {
        source: ProposalSource::Draft {
            id: "D".into(),
            revision: "1".repeat(40),
        },
        reason: "remove shared coupling".into(),
        affected_intents: vec![],
        affected_modules: vec!["A".into()],
        affected_interfaces: vec![],
        affected_tasks: vec![],
        affected_paths: vec!["src/api/**".into()],
        alternatives: vec![Alternative {
            name: "keep".into(),
            consequences: "coupling stays".into(),
        }],
        recommendation: "split the interface".into(),
    }
}

#[test]
fn proposal_requires_a_reason_alternative_and_recommendation() {
    let mut p = input();
    assert!(validate_proposal(&p).is_ok());
    p.reason.clear();
    assert!(validate_proposal(&p).is_err());
    p = input();
    p.alternatives.clear();
    assert!(validate_proposal(&p).is_err());
    p = input();
    p.recommendation.clear();
    assert!(validate_proposal(&p).is_err());
}

#[test]
fn proposal_wire_and_path_validation_share_the_existing_rules() {
    let p = input();
    for key in ["actor", "approved", "output_ref"] {
        let mut value = serde_json::to_value(&p).unwrap();
        value[key] = true.into();
        assert!(serde_json::from_value::<ProposalInput>(value).is_err());
    }
    let mut value = serde_json::to_value(&p).unwrap();
    value["source"]["extra"] = true.into();
    assert!(serde_json::from_value::<ProposalInput>(value).is_err());
    for path in [
        "../escape",
        "/root",
        "foo/**x",
        "src/[ab]",
        "a\\b",
        "x:y",
        "",
        ".git/../foo",
    ] {
        let mut p = p.clone();
        p.affected_paths = vec![path.into()];
        assert!(validate_proposal(&p).is_err(), "{path}");
    }
    for replacement in ["HEAD", "a1", "A".repeat(40).as_str()] {
        let mut p = p.clone();
        p.source = ProposalSource::Branch {
            revision: replacement.into(),
            base_revision: "b".repeat(40),
        };
        assert!(validate_proposal(&p).is_err());
    }
    let mut p = p;
    p.alternatives[0].consequences = " ".into();
    assert!(validate_proposal(&p).is_err());
}

#[test]
fn neither_an_unapproved_main_nor_a_bare_tag_is_a_proposal_baseline() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(
        root.path(),
        &towermd_core::init::InitOptions { docs_dir: None },
    )
    .unwrap();
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &["commit", "-qm", "unapproved configured main"],
    );
    let catalog = Catalog::load(root.path()).unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let inputs = towermd_core::recovery_outbox::RecoveryInputs::read(
        &runtime.path().join("absent-outbox"),
        &catalog,
    )
    .unwrap();
    let p = declared(ProposalSource::Branch {
        revision: catalog.revision.clone(),
        base_revision: catalog.revision.clone(),
    });
    let refs = git(root.path(), &["show-ref"]);
    assert!(
        analyze_proposal(&catalog, &inputs, &p, &BTreeMap::new(), "P-no-baseline")
            .unwrap_err()
            .contains("approved current baseline")
    );
    assert_eq!(git(root.path(), &["show-ref"]), refs);
    assert!(!runtime.path().join("absent-outbox").exists());
    git(root.path(), &["tag", "towermd/B0"]);
    assert!(analyze_proposal(&catalog, &inputs, &p, &BTreeMap::new(), "P-bare").is_err());
}

fn published() -> fixture::Fixture {
    let mut f = fixture::fixture_with_source_files(&[
        ("docs/tower/intents/I.md", "---\ntype: Intent\nid: I\nallocated_to: [A]\nfuture: preserved\n---\nintent\n"),
        ("docs/tower/arch/A.md", "---\ntype: Module\nid: A\npaths: ['alpha/**']\ninterfaces: [IF]\nverify: []\nfuture: preserved\n---\nA\n"),
        ("docs/tower/arch/B.md", "---\ntype: Module\nid: B\npaths: ['beta/**']\ninterfaces: [IF]\nverify: []\n---\nB\n"),
        ("docs/tower/arch/C.md", "---\ntype: Module\nid: C\npaths: ['gamma/**']\ninterfaces: []\nverify: []\n---\nC\n"),
        ("docs/tower/arch/U.md", "---\ntype: Module\nid: U\npaths: ['unrelated/**']\ninterfaces: []\nverify: []\n---\nU\n"),
        ("docs/tower/arch/IF.md", "---\ntype: Interface\nid: IF\nfrom: A\nto: B\nfuture: preserved\n---\ncontract\n"),
        ("docs/tower/specs/AC.md", "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [actual]\n---\nacceptance\n"),
    ]);
    let action = fixture::action(&f, Verdict::Approved);
    let result = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap();
    let publication = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &result.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 5),
        )
        .unwrap();
    assert!(
        matches!(publication, PublicationReport::Recorded { .. }),
        "{publication:?}"
    );
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    f
}

fn task(id: &str, module: &str, state: &str, paths: &[&str]) -> Task {
    Task {
        object_type: "Task".into(),
        spec: TaskSpec {
            id: id.into(),
            title: id.into(),
            module: module.into(),
            touches: vec![],
            allowed_paths: paths.iter().map(|p| (*p).into()).collect(),
            depends_on: vec![],
            acceptance: vec![],
            decisions: vec![],
        },
        state: state.into(),
        extra: BTreeMap::new(),
    }
}

fn tasks() -> BTreeMap<String, Task> {
    let mut acceptance = task("T-acceptance", "U", "ready", &["unrelated/future.txt"]);
    acceptance.spec.acceptance = vec!["AC".into()];
    [
        task("T-A", "A", "ready", &["alpha/a.py"]),
        task("T-B", "B", "running", &["beta/**"]),
        task("T-uncertain", "U", "ready", &["unrelated/future.txt"]),
        task("T-done", "A", "merged", &["alpha/a.py"]),
        acceptance,
    ]
    .into_iter()
    .map(|t| (t.spec.id.clone(), t))
    .collect()
}

fn declared(source: ProposalSource) -> ProposalInput {
    ProposalInput {
        source,
        reason: "move interface boundary".into(),
        affected_intents: vec!["I".into()],
        affected_modules: vec!["A".into(), "B".into(), "C".into()],
        affected_interfaces: vec!["IF".into()],
        affected_tasks: vec![
            "T-A".into(),
            "T-B".into(),
            "T-acceptance".into(),
            "T-uncertain".into(),
        ],
        affected_paths: vec![
            "docs/tower/**".into(),
            "alpha/**".into(),
            "beta/**".into(),
            "gamma/**".into(),
        ],
        alternatives: vec![Alternative {
            name: "keep".into(),
            consequences: "keep coupling".into(),
        }],
        recommendation: "move".into(),
    }
}

#[test]
fn branch_analysis_uses_actual_protected_diff_old_and_new_endpoints_without_writes() {
    let f = published();
    git(f.root.path(), &["switch", "-qc", "proposal-branch", "main"]);
    let docs = f.root.path().join(f.catalog.config.docs_dir());
    let before_if = f
        .catalog
        .read_file("docs/tower/arch/IF.md")
        .unwrap()
        .unwrap();
    let after_if = before_if
        .replace("to: B", "to: C")
        .replace('\n', "\r\n")
        .trim_end_matches("\r\n")
        .to_owned();
    fs::write(docs.join("arch/IF.md"), &after_if).unwrap();
    for (id, value) in [("B", "interfaces: []"), ("C", "interfaces: [IF]")] {
        let text = f
            .catalog
            .read_file(&format!("docs/tower/arch/{id}.md"))
            .unwrap()
            .unwrap();
        fs::write(
            docs.join(format!("arch/{id}.md")),
            text.replace(
                if id == "B" {
                    "interfaces: [IF]"
                } else {
                    "interfaces: []"
                },
                value,
            ),
        )
        .unwrap();
    }
    let mut canvas: serde_json::Value = serde_json::from_str(
        &f.catalog
            .read_file("docs/tower/tower.canvas")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    for edge in canvas["edges"].as_array_mut().unwrap() {
        if edge["towermd"]["obj"] == "IF" {
            edge["toNode"] = "C".into();
        }
    }
    fs::write(docs.join("tower.canvas"), canvas.to_string()).unwrap();
    fs::write(
        f.root.path().join("alpha/a.py"),
        "# unrelated branch code\n",
    )
    .unwrap();
    fs::write(
        docs.join("ignored-task.md"),
        "---\ntype: Task\nid: branch-task\nstate: done\n---\nignore\n",
    )
    .unwrap();
    fs::write(f.root.path().join("README.md"), "---\ntype: Article\ncategory: ordinary documentation\n---\nUnrelated Markdown is not a Tower object.\n").unwrap();
    git(f.root.path(), &["add", "."]);
    git(
        f.root.path(),
        &["commit", "-qm", "real branch proposal and unrelated code"],
    );
    let revision = git(f.root.path(), &["rev-parse", "HEAD"]);
    let p = declared(ProposalSource::Branch {
        revision: revision.clone(),
        base_revision: f.catalog.revision.clone(),
    });
    fs::write(
        docs.join("arch/IF.md"),
        "dirty working tree must not replace source\n",
    )
    .unwrap();
    fs::write(
        f.root.path().join("untracked.txt"),
        "keep untracked bytes\n",
    )
    .unwrap();
    let refs = git(f.root.path(), &["show-ref"]);
    let status = git(f.root.path(), &["status", "--porcelain=v1"]);
    let index = fs::read(f.catalog.common_dir.join("index")).unwrap();
    let result = analyze_proposal(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &p,
        &tasks(),
        "P-branch",
    )
    .unwrap();
    assert_eq!(result.baseline, "towermd/B0");
    assert_eq!(result.main_revision, f.catalog.revision);
    assert_eq!(result.bundle.id, "P-branch");
    assert_eq!(result.bundle.source_revision, f.catalog.revision);
    assert_eq!(result.bundle.target_baseline, "towermd/B1");
    assert_eq!(result.impact.interfaces, ["IF"]);
    assert_eq!(result.impact.modules, ["A", "B", "C"]);
    assert_eq!(result.impact.intents, ["I"]);
    assert_eq!(result.impact.acceptances, ["AC"]);
    assert_eq!(
        result.impact.tasks,
        ["T-A", "T-B", "T-acceptance", "T-uncertain"]
    );
    assert!(!result.bundle.objects.contains_key("branch-task"));
    assert!(result.changes.iter().all(|c| c.path != "alpha/a.py"
        && c.path != "README.md"
        && !c.path.ends_with("ignored-task.md")));
    let interface = result
        .changes
        .iter()
        .find(|c| c.path.ends_with("/IF.md"))
        .unwrap();
    assert_eq!(interface.before.as_ref(), Some(&before_if));
    assert_eq!(interface.after.as_ref(), Some(&after_if));
    assert!(result.bundle.objects["A"]
        .text
        .contains("future: preserved"));
    assert_eq!(git(f.root.path(), &["show-ref"]), refs);
    assert_eq!(git(f.root.path(), &["status", "--porcelain=v1"]), status);
    assert_eq!(fs::read(f.catalog.common_dir.join("index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(docs.join("arch/IF.md")).unwrap(),
        "dirty working tree must not replace source\n"
    );
    assert_eq!(
        fs::read_to_string(f.root.path().join("untracked.txt")).unwrap(),
        "keep untracked bytes\n"
    );
    for key in [
        "affected_intents",
        "affected_modules",
        "affected_interfaces",
        "affected_tasks",
        "affected_paths",
    ] {
        let mut value = serde_json::to_value(&p).unwrap();
        value[key] = serde_json::json!([]);
        let incomplete: ProposalInput = serde_json::from_value(value).unwrap();
        let reason = analyze_proposal(
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            &incomplete,
            &tasks(),
            "P-missing",
        )
        .unwrap_err();
        assert!(reason.contains(key), "{key}: {reason}");
    }
    let mut missing_end = p.clone();
    missing_end.affected_modules.retain(|id| id != "B");
    assert!(analyze_proposal(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &missing_end,
        &tasks(),
        "P-old-end"
    )
    .unwrap_err()
    .contains("B"));
    // A bare later tag cannot become the authority used for another proposal.
    git(f.root.path(), &["tag", "towermd/B1", "main"]);
    assert!(analyze_proposal(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &p,
        &tasks(),
        "P-bare"
    )
    .is_err());
}

#[test]
fn draft_analysis_requires_exact_persistent_revision_and_layout_does_not_affect_tasks() {
    let mut f = published();
    let id = uuid::Uuid::now_v7().to_string();
    let create = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: f.catalog.config.project_id().into(),
            actor: "human:owner".into(),
            host: "host".into(),
            run: None,
            action: DraftSaveAction::CreateDraft {
                baseline: "towermd/B0".into(),
                main_revision: f.catalog.revision.clone(),
            },
        },
        &id,
    )
    .unwrap();
    let created = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &create,
        || Ok(NOW + 10),
    )
    .unwrap();
    let revision = created.receipt.result_ref.revision;
    let edit = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: f.catalog.config.project_id().into(),
            actor: "human:owner".into(),
            host: "host".into(),
            run: None,
            action: DraftSaveAction::EditDraft {
                id: id.clone(),
                expected_revision: revision.clone(),
                edit: DraftEdit::Move {
                    node: "A".into(),
                    x: 912,
                    y: -312,
                },
            },
        },
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let saved = save_draft_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &f.writer,
        &edit,
        || Ok(NOW + 11),
    )
    .unwrap();
    let mut p = declared(ProposalSource::Draft {
        id: id.clone(),
        revision: saved.receipt.result_ref.revision,
    });
    p.affected_intents.clear();
    p.affected_modules.clear();
    p.affected_interfaces.clear();
    p.affected_tasks.clear();
    p.affected_paths = vec!["docs/tower/tower.canvas".into()];
    let refs = git(f.root.path(), &["show-ref"]);
    let analysis = analyze_proposal(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &p,
        &tasks(),
        "P-layout",
    )
    .unwrap();
    assert_eq!(analysis.changes.len(), 1);
    assert!(analysis.impact.tasks.is_empty());
    assert!(analysis.impact.modules.is_empty());
    let proposed = attach_proposal(&f.catalog, &analysis, &p, "run-proposal").unwrap();
    let path = format!("docs/tower/proposals/{}.md", analysis.bundle.id);
    assert!(proposed.files.contains_key(&path));
    assert!(proposed.files[&path].text.contains("type: Proposal"));
    assert!(proposed.files[&path].text.contains("layer: L4"));
    let carrier = validate_proposal_subject(&proposed).unwrap().unwrap();
    assert_eq!(carrier.input, p);
    assert_eq!(carrier.impact, analysis.impact);
    assert_eq!(carrier.run, "run-proposal");
    assert_ne!(carrier.payload_hash, proposed.content_hash);
    // A human may save another draft revision, but the old impact cannot
    // authorize its changed payload. The publication consumer uses this same check.
    let changed = towermd_core::baseline_draft::apply_edit(
        &proposed.bundle,
        &DraftEdit::Move {
            node: "A".into(),
            x: 44,
            y: 55,
        },
    )
    .unwrap();
    let changed = towermd_core::baseline_draft::freeze(&f.catalog, &changed).unwrap();
    assert!(validate_proposal_subject(&changed)
        .unwrap_err()
        .contains("payload changed"));
    let original_object = proposed.bundle.objects[&proposed.bundle.id].clone();
    for (key, value) in [
        ("id", "other"),
        ("layer", "L1"),
        ("reason", "swapped reason"),
    ] {
        let mut bundle = proposed.bundle.clone();
        let mut doc = towermd_core::okf::Document::parse(&original_object.text).unwrap();
        doc.set(key, value.into()).unwrap();
        bundle.objects.get_mut(&bundle.id).unwrap().text = doc.to_text();
        assert!(
            towermd_core::baseline_draft::freeze(&f.catalog, &bundle).is_err(),
            "{key}"
        );
    }
    let mut wrong_path = proposed.bundle.clone();
    wrong_path.objects.get_mut(&wrong_path.id).unwrap().path = "docs/tower/arch/wrong.md".into();
    assert!(towermd_core::baseline_draft::freeze(&f.catalog, &wrong_path).is_err());
    let mut unknown = proposed.bundle.clone();
    let mut doc = towermd_core::okf::Document::parse(&original_object.text).unwrap();
    doc.set("future", serde_yaml::from_str("{values: [2, 1]}").unwrap())
        .unwrap();
    unknown.objects.get_mut(&unknown.id).unwrap().text = doc.to_text();
    let unknown = towermd_core::baseline_draft::freeze(&f.catalog, &unknown).unwrap();
    assert_ne!(unknown.content_hash, proposed.content_hash);
    assert!(validate_proposal_subject(&unknown).unwrap().is_some());
    let mut arbitrary_l4 = analysis.bundle.clone();
    arbitrary_l4.objects.insert(
        "other-task".into(),
        towermd_core::baseline_draft::DraftObject {
            id: "other-task".into(),
            path: "docs/tower/proposals/other-task.md".into(),
            text: "---\ntype: Task\nid: other-task\nlayer: L4\n---\nnot a proposal\n".into(),
        },
    );
    assert!(towermd_core::baseline_draft::freeze(&f.catalog, &arbitrary_l4).is_err());
    assert_eq!(
        analysis.bundle.objects["A"].text,
        f.catalog
            .read_file("docs/tower/arch/A.md")
            .unwrap()
            .unwrap()
    );
    assert_eq!(git(f.root.path(), &["show-ref"]), refs);
    p.source = ProposalSource::Draft { id, revision };
    assert!(analyze_proposal(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &p,
        &tasks(),
        "P-stale"
    )
    .unwrap_err()
    .contains("revision changed"));
    if let ProposalSource::Draft { id, .. } = &p.source {
        git(
            f.root.path(),
            &[
                "symbolic-ref",
                &format!("refs/towermd/drafts/{id}"),
                "refs/heads/main",
            ],
        );
        assert!(analyze_proposal(
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            &p,
            &tasks(),
            "P-symbolic"
        )
        .is_err());
    }
}

#[test]
fn branch_sources_reject_unknown_layer_configuration_symlink_and_unrelated_ancestry() {
    let f = published();
    git(
        f.root.path(),
        &["switch", "-qc", "invalid-proposal", "main"],
    );
    let base = f.catalog.revision.clone();
    let analyze = |revision: String, base_revision: String| {
        analyze_proposal(
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            &declared(ProposalSource::Branch {
                revision,
                base_revision,
            }),
            &tasks(),
            "P-invalid",
        )
    };
    for text in [
        "---\ntype: Mystery\nid: new\n---\nunknown\n",
        "---\ntype: Module\nid: new\nlayer: wrong\n---\ninvalid\n",
        "---\ntype: Module\nid: new\nlayer: L5\n---\nwrong default\n",
        "---\ntype: Module\nid: A\npaths: ['alpha/**']\n---\nduplicate ID\n",
    ] {
        git(f.root.path(), &["reset", "--hard", &base]);
        fs::write(f.root.path().join("docs/tower/arch/new.md"), text).unwrap();
        git(f.root.path(), &["add", "."]);
        git(f.root.path(), &["commit", "-qm", "invalid object"]);
        assert!(analyze(git(f.root.path(), &["rev-parse", "HEAD"]), base.clone()).is_err());
    }
    // Location cannot downgrade a document that explicitly declares a slow
    // layer; ordinary metadata is ignored only when it is actually unprotected.
    git(f.root.path(), &["reset", "--hard", &base]);
    fs::write(
        f.root.path().join("README.md"),
        "---\ntype: Mystery\nlayer: L1\n---\nprotected despite location\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "."]);
    git(
        f.root.path(),
        &["commit", "-qm", "unknown external slow document"],
    );
    assert!(
        analyze(git(f.root.path(), &["rev-parse", "HEAD"]), base.clone())
            .unwrap_err()
            .contains("protected")
    );
    git(f.root.path(), &["reset", "--hard", &base]);
    let config = fs::read_to_string(f.root.path().join(".towermd/config.yaml")).unwrap();
    fs::write(
        f.root.path().join(".towermd/config.yaml"),
        format!("{config}\nfuture: changed\n"),
    )
    .unwrap();
    git(f.root.path(), &["add", "."]);
    git(f.root.path(), &["commit", "-qm", "config changed"]);
    assert!(
        analyze(git(f.root.path(), &["rev-parse", "HEAD"]), base.clone())
            .unwrap_err()
            .contains("configuration")
    );
    #[cfg(unix)]
    {
        git(f.root.path(), &["reset", "--hard", &base]);
        std::os::unix::fs::symlink("A.md", f.root.path().join("docs/tower/arch/linked.md"))
            .unwrap();
        git(f.root.path(), &["add", "."]);
        git(f.root.path(), &["commit", "-qm", "symlink"]);
        assert!(
            analyze(git(f.root.path(), &["rev-parse", "HEAD"]), base.clone())
                .unwrap_err()
                .contains("UnsupportedMode")
        );
        git(f.root.path(), &["reset", "--hard", &base]);
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            f.root.path().join("docs/tower/arch/A.md"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        git(f.root.path(), &["add", "."]);
        git(f.root.path(), &["commit", "-qm", "protected mode changed"]);
        assert!(
            analyze(git(f.root.path(), &["rev-parse", "HEAD"]), base.clone())
                .unwrap_err()
                .contains("file mode")
        );
    }
    let unrelated = git(f.root.path(), &["rev-parse", "HEAD"]);
    assert!(analyze(base.clone(), unrelated)
        .unwrap_err()
        .contains("ancestor"));
    let blob = git(f.root.path(), &["rev-parse", &format!("{base}:alpha/a.py")]);
    assert!(analyze(blob, base).unwrap_err().contains("actual commit"));
}
