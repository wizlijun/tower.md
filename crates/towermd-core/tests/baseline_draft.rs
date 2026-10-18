use serde_json::{json, Value};
use std::collections::BTreeMap;
use towermd_core::{baseline_draft::*, okf::Document, repo::git};

fn object(id: &str, kind: &str, fields: &str) -> DraftObject {
    DraftObject { id: id.into(), path: format!("docs/tower/arch/{id}.md"),
        text: format!("---\ntype: {kind}\nid: {id}\ntitle: {id}\nx-future: {{values: [2, 1]}}\n{fields}---\nbody {id}") }
}
fn command(cwd: &str) -> VerifyCommand {
    VerifyCommand {
        cwd: cwd.into(),
        argv: vec!["cargo".into(), "test".into()],
    }
}
fn bundle() -> DraftBundle {
    let objects = [
        object(
            "A",
            "Module",
            "paths: ['src/a/**']\nverify: [{cwd: '.', argv: [cargo, test]}]\n",
        ),
        object(
            "B",
            "Module",
            "paths: ['src/b/**']\nverify: [{cwd: src/b, argv: [cargo, test]}]\n",
        ),
        object("C", "Module", "paths: ['src/c/**']\nverify: []\n"),
        object("I", "Interface", "from: B\nto: C\ncontract: api.md\n"),
        object("AC", "Acceptance", "module: B\ncriteria: ['all pass']\n"),
        object("future", "FutureType", "opaque: yes\n"),
    ]
    .into_iter()
    .map(|o| (o.id.clone(), o))
    .collect();
    DraftBundle { id: "draft".into(), project_id: "project".into(), docs_dir: "docs/tower".into(),
        source_revision: "a".repeat(40), target_baseline: "towermd/B0".into(), objects,
        canvas_text: json!({"x-root": {"version":3},"nodes": [
            {"id":"A","type":"file","file":"docs/tower/arch/A.md","towermd":{"obj":"A","future":1},"x":10,"y":20,"width":320,"height":180},
            {"id":"B","type":"file","file":"docs/tower/arch/B.md","towermd":{"obj":"B"},"x":400,"y":20,"width":320,"height":180},
            {"id":"C","type":"file","file":"docs/tower/arch/C.md","towermd":{"obj":"C"},"x":800,"y":20,"width":320,"height":180},
            {"id":"opaque","type":"future","raw":{"nested":[2,1]}},
        ],"edges":[{"id":"edge-I","fromNode":"B","toNode":"C","towermd":{"obj":"I","future":true},"custom":{"preserved":true}}]}).to_string(),
        deletions: vec![], questions: vec!["keep".into()], risks: vec![] }
}
fn doc(bundle: &DraftBundle, id: &str) -> Document {
    Document::parse(&bundle.objects[id].text).unwrap()
}
fn canvas(bundle: &DraftBundle) -> Value {
    serde_json::from_str(&bundle.canvas_text).unwrap()
}

#[test]
fn rename_and_verify_change_real_objects_and_preserve_unedited_bytes() {
    let before = bundle();
    let renamed = apply_edit(
        &before,
        &DraftEdit::Rename {
            object: "A".into(),
            title: "API".into(),
        },
    )
    .unwrap();
    assert_eq!(doc(&renamed, "A").get_str("title"), Some("API"));
    assert_eq!(doc(&renamed, "A").get_str("id"), Some("A"));
    assert_eq!(renamed.objects["A"].path, before.objects["A"].path);
    assert_eq!(
        doc(&renamed, "A").get("x-future"),
        doc(&before, "A").get("x-future")
    );
    assert_eq!(
        renamed.objects["A"].text.split("---\n").last(),
        before.objects["A"].text.split("---\n").last()
    );
    assert_eq!(renamed.objects["future"], before.objects["future"]);
    assert_eq!(renamed.canvas_text, before.canvas_text);
    let verified = apply_edit(
        &renamed,
        &DraftEdit::ConfirmVerify {
            module: "A".into(),
            commands: vec![command("src/a")],
        },
    )
    .unwrap();
    let verify: Vec<VerifyCommand> =
        serde_yaml::from_value(doc(&verified, "A").get("verify").unwrap().clone()).unwrap();
    assert_eq!(verify, vec![command("src/a")]);
    assert_eq!(verified.objects["B"], before.objects["B"]);
    assert!(apply_edit(
        &before,
        &DraftEdit::Rename {
            object: "future".into(),
            title: "bad".into()
        }
    )
    .is_err());
    for invalid in [
        VerifyCommand {
            cwd: "../outside".into(),
            argv: vec!["test".into()],
        },
        VerifyCommand {
            cwd: ".".into(),
            argv: vec![],
        },
    ] {
        assert!(apply_edit(
            &before,
            &DraftEdit::ConfirmVerify {
                module: "A".into(),
                commands: vec![invalid]
            }
        )
        .is_err());
    }
}

#[test]
fn move_only_touches_named_valid_node_and_keeps_opaque_entries() {
    let before = bundle();
    let old = canvas(&before);
    let moved = apply_edit(
        &before,
        &DraftEdit::Move {
            node: "A".into(),
            x: -99,
            y: 123,
        },
    )
    .unwrap();
    let new = canvas(&moved);
    let mut expected = old.clone();
    expected["nodes"][0]["x"] = json!(-99);
    expected["nodes"][0]["y"] = json!(123);
    assert_eq!(new, expected);
    assert_eq!(moved.objects, before.objects);
    for node in ["opaque", "absent"] {
        assert!(apply_edit(
            &before,
            &DraftEdit::Move {
                node: node.into(),
                x: 1,
                y: 2
            }
        )
        .is_err());
    }
    assert!(apply_edit(
        &before,
        &DraftEdit::Move {
            node: "A".into(),
            x: i64::MAX,
            y: 2
        }
    )
    .is_err());
    assert_eq!(canvas(&before), old);
}

#[test]
fn unrelated_crlf_objects_and_unknown_canvas_values_survive_all_local_edits() {
    let mut before = bundle();
    before.objects.get_mut("future").unwrap().text =
        before.objects["future"].text.replace('\n', "\r\n");
    let after = apply_edit(
        &before,
        &DraftEdit::Rename {
            object: "A".into(),
            title: "API".into(),
        },
    )
    .unwrap();
    assert_eq!(
        after.objects["future"].text.as_bytes(),
        before.objects["future"].text.as_bytes()
    );
    assert_eq!(after.canvas_text.as_bytes(), before.canvas_text.as_bytes());
    let mut duplicate = before.clone();
    let mut raw = canvas(&duplicate);
    let node = raw["nodes"][0].clone();
    raw["nodes"].as_array_mut().unwrap().push(node);
    duplicate.canvas_text = raw.to_string();
    assert!(apply_edit(
        &duplicate,
        &DraftEdit::Move {
            node: "A".into(),
            x: 1,
            y: 2
        }
    )
    .is_err());
    assert!(apply_edit(
        &duplicate,
        &DraftEdit::Merge {
            modules: vec!["A".into(), "B".into()],
            keep: "A".into(),
            title: "AB".into()
        }
    )
    .is_err());
}

#[test]
fn merge_unions_paths_keeps_cwd_rewrites_references_and_deletes_only_removed_module() {
    let before = bundle();
    let merged = apply_edit(
        &before,
        &DraftEdit::Merge {
            modules: vec!["A".into(), "B".into()],
            keep: "A".into(),
            title: "AB".into(),
        },
    )
    .unwrap();
    assert!(!merged.objects.contains_key("B"));
    assert_eq!(merged.deletions, vec!["docs/tower/arch/B.md"]);
    let a = doc(&merged, "A");
    assert_eq!(
        a.get("paths").unwrap(),
        &serde_yaml::to_value(vec!["src/a/**", "src/b/**"]).unwrap()
    );
    let commands: Vec<VerifyCommand> =
        serde_yaml::from_value(a.get("verify").unwrap().clone()).unwrap();
    assert_eq!(commands, vec![command("."), command("src/b")]);
    assert_eq!(doc(&merged, "I").get_str("from"), Some("A"));
    assert_eq!(doc(&merged, "AC").get_str("module"), Some("A"));
    let c = canvas(&merged);
    assert_eq!(c["edges"][0]["fromNode"], "A");
    assert_eq!(c["nodes"][0]["x"], 10);
    assert_eq!(c["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(c["nodes"][2], canvas(&before)["nodes"][3]);
    assert_eq!(merged.objects["C"], before.objects["C"]);
    assert_eq!(merged.objects["future"], before.objects["future"]);
    assert!(before.objects.contains_key("B"));
    assert!(before.deletions.is_empty());
}

#[test]
fn merge_rejects_internal_interface_atomically() {
    let mut before = bundle();
    before.objects.get_mut("I").unwrap().text = object("I", "Interface", "from: A\nto: B\n").text;
    let copy = before.clone();
    let error = apply_edit(
        &before,
        &DraftEdit::Merge {
            modules: vec!["A".into(), "B".into()],
            keep: "A".into(),
            title: "AB".into(),
        },
    )
    .unwrap_err();
    assert!(error.contains("RemoveInterface"));
    assert_eq!(before, copy);
}

fn parts() -> Vec<SplitPart> {
    vec![
        SplitPart {
            id: "B1".into(),
            title: "B1".into(),
            paths: vec!["src/b/**".into()],
            verify: vec![],
            acceptance_ids: vec!["AC".into()],
            x: 1,
            y: 2,
        },
        SplitPart {
            id: "B2".into(),
            title: "B2".into(),
            paths: vec!["extra/**".into()],
            verify: vec![command(".")],
            acceptance_ids: vec![],
            x: 3,
            y: 4,
        },
    ]
}
fn splittable() -> DraftBundle {
    let mut before = bundle();
    before.objects.get_mut("B").unwrap().text = object(
        "B",
        "Module",
        "paths: ['src/b/**', 'extra/**']\nverify: []\n",
    )
    .text;
    before
}
#[test]
fn split_requires_explicit_complete_assignments_and_clones_unknown_fields() {
    let before = splittable();
    let endpoints = BTreeMap::from([("I".into(), ["B2".into(), "C".into()])]);
    let edit = DraftEdit::Split {
        module: "B".into(),
        parts: parts(),
        interface_endpoints: endpoints.clone(),
    };
    let split = apply_edit(&before, &edit).unwrap();
    assert!(!split.objects.contains_key("B"));
    assert_eq!(split.deletions, vec!["docs/tower/arch/B.md"]);
    assert_eq!(doc(&split, "I").get_str("from"), Some("B2"));
    assert_eq!(doc(&split, "AC").get_str("module"), Some("B1"));
    for id in ["B1", "B2"] {
        assert_eq!(
            doc(&split, id).get("x-future"),
            doc(&before, "B").get("x-future")
        );
        assert_eq!(doc(&split, id).body(), "body B");
    }
    assert_eq!(canvas(&split)["edges"][0]["fromNode"], "B2");
    assert_eq!(split.objects["A"], before.objects["A"]);
    let mut broken = parts();
    broken[1].paths = broken[0].paths.clone();
    assert!(apply_edit(
        &before,
        &DraftEdit::Split {
            module: "B".into(),
            parts: broken,
            interface_endpoints: endpoints.clone()
        }
    )
    .is_err());
    assert!(apply_edit(
        &before,
        &DraftEdit::Split {
            module: "B".into(),
            parts: parts(),
            interface_endpoints: BTreeMap::new()
        }
    )
    .is_err());
    let mut broken = parts();
    broken[0].acceptance_ids.clear();
    assert!(apply_edit(
        &before,
        &DraftEdit::Split {
            module: "B".into(),
            parts: broken,
            interface_endpoints: endpoints
        }
    )
    .is_err());
    assert_eq!(before, splittable());
}

#[test]
fn split_can_keep_original_id_without_deleting_its_path_and_rejects_unassigned_edges() {
    let before = splittable();
    let mut p = parts();
    p[0].id = "B".into();
    let mappings = BTreeMap::from([("I".into(), ["B2".into(), "C".into()])]);
    let after = apply_edit(
        &before,
        &DraftEdit::Split {
            module: "B".into(),
            parts: p.clone(),
            interface_endpoints: mappings.clone(),
        },
    )
    .unwrap();
    assert!(after.deletions.is_empty());
    assert_eq!(after.objects["B"].path, before.objects["B"].path);
    assert_eq!(doc(&after, "AC").get_str("module"), Some("B"));
    let mut blocked = before.clone();
    let mut raw = canvas(&blocked);
    raw["edges"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"annotation","fromNode":"A","toNode":"B","label":"unassigned"}));
    blocked.canvas_text = raw.to_string();
    assert!(apply_edit(
        &blocked,
        &DraftEdit::Split {
            module: "B".into(),
            parts: p,
            interface_endpoints: mappings
        }
    )
    .unwrap_err()
    .contains("未明确分配"));
}

#[test]
fn fixed_edit_envelope_rejects_unknown_fields() {
    assert!(serde_json::from_value::<DraftEdit>(
        json!({"action":"move","node":"A","x":1,"y":2,"actor":"human:any"})
    )
    .is_err());
    assert!(serde_json::from_value::<DraftEdit>(json!({"action":"confirm_verify","module":"A","commands":[{"cwd":".","argv":["test"],"unchecked":true}]})).is_err());
}

#[test]
fn put_remove_interface_build_real_object_and_edge_with_exact_deletion() {
    let before = bundle();
    let interface = object("I2", "Interface", "contract: actual.md\n");
    let added = apply_edit(
        &before,
        &DraftEdit::PutInterface {
            object: interface,
            from: "A".into(),
            to: "C".into(),
        },
    )
    .unwrap();
    assert_eq!(doc(&added, "I2").get_str("from"), Some("A"));
    assert_eq!(canvas(&added)["edges"][1]["towermd"]["obj"], "I2");
    let removed = apply_edit(
        &added,
        &DraftEdit::RemoveInterface {
            interface: "I".into(),
        },
    )
    .unwrap();
    assert!(!removed.objects.contains_key("I"));
    assert!(removed.objects.contains_key("I2"));
    assert!(removed.objects.contains_key("A"));
    assert_eq!(removed.deletions, vec!["docs/tower/arch/I.md"]);
    assert_eq!(canvas(&removed)["edges"].as_array().unwrap().len(), 1);
    assert!(apply_edit(
        &before,
        &DraftEdit::RemoveInterface {
            interface: "A".into()
        }
    )
    .is_err());
}

#[test]
fn docref_reads_fixed_git_blob_and_never_copies_or_claims_document() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]).unwrap();
    std::fs::write(root.join("guide.md"), "# ordinary\n").unwrap();
    std::fs::write(root.join("object.md"), "---\ntype: Module\nid: X\n---\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("guide.md", root.join("link.md")).unwrap();
    git(root, &["add", "."]).unwrap();
    git(
        root,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    )
    .unwrap();
    let mut before = bundle();
    before.source_revision = git(root, &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(
        root.join("guide.md"),
        "---\ntype: Module\n---\nworktree replaced\n",
    )
    .unwrap();
    let edit = DraftEdit::AddDocref {
        node: "guide".into(),
        path: "guide.md".into(),
        x: 10,
        y: 20,
    };
    let after = apply_edit_at(root, &before, &edit).unwrap();
    let node = canvas(&after)["nodes"][4].clone();
    assert_eq!(node["file"], "guide.md");
    assert!(node.get("towermd").is_none());
    assert_eq!(after.objects, before.objects);
    assert_eq!(
        std::fs::read_to_string(root.join("guide.md")).unwrap(),
        "---\ntype: Module\n---\nworktree replaced\n"
    );
    assert!(apply_edit(&before, &edit).is_err());
    for path in [
        "missing.md",
        "object.md",
        "../guide.md",
        ".git/config",
        "link.md",
    ] {
        assert!(
            apply_edit_at(
                root,
                &before,
                &DraftEdit::AddDocref {
                    node: "guide".into(),
                    path: path.into(),
                    x: 1,
                    y: 2
                }
            )
            .is_err(),
            "{path}"
        );
    }
    assert!(apply_edit_at(root, &after, &edit).is_err());
}

#[test]
fn interface_edits_keep_all_module_memberships_and_existing_unknown_fields() {
    let mut before = bundle();
    for module in ["B", "C"] {
        let mut module_doc = doc(&before, module);
        module_doc
            .set("interfaces", serde_yaml::to_value(vec!["I"]).unwrap())
            .unwrap();
        before.objects.get_mut(module).unwrap().text = module_doc.to_text();
    }
    let incoming = DraftObject {
        id: "I".into(),
        path: before.objects["I"].path.clone(),
        text: "---\ntype: Interface\nid: I\n---\nupdated body".into(),
    };
    let changed = apply_edit(
        &before,
        &DraftEdit::PutInterface {
            object: incoming,
            from: "A".into(),
            to: "C".into(),
        },
    )
    .unwrap();
    assert_eq!(
        doc(&changed, "I").get("x-future"),
        doc(&before, "I").get("x-future")
    );
    assert_eq!(doc(&changed, "I").get_str("contract"), Some("api.md"));
    assert_eq!(doc(&changed, "I").body(), "updated body");
    for module in ["A", "C"] {
        assert_eq!(
            doc(&changed, module).get("interfaces"),
            Some(&serde_yaml::to_value(vec!["I"]).unwrap())
        );
    }
    assert_eq!(
        doc(&changed, "B").get("interfaces"),
        Some(&serde_yaml::to_value(Vec::<String>::new()).unwrap())
    );
    let removed = apply_edit(
        &changed,
        &DraftEdit::RemoveInterface {
            interface: "I".into(),
        },
    )
    .unwrap();
    for module in ["A", "B", "C"] {
        assert!(!doc(&removed, module)
            .get("interfaces")
            .and_then(|v| v.as_sequence())
            .is_some_and(|v| v.contains(&serde_yaml::Value::from("I"))));
    }
    assert_eq!(doc(&before, "I").get_str("from"), Some("B"));
}

#[test]
fn putting_unchanged_interface_keeps_existing_raw_text_byte_identical() {
    let mut before = bundle();
    before.objects.get_mut("I").unwrap().text="---\r\ntype: Interface\r\nid: I\r\n# keep exact formatting\r\nx-future: { values: [2, 1] }\r\nfrom: B\r\nto: C\r\n---\r\nbody without final newline".into();
    let after = apply_edit(
        &before,
        &DraftEdit::PutInterface {
            object: before.objects["I"].clone(),
            from: "B".into(),
            to: "C".into(),
        },
    )
    .unwrap();
    assert_eq!(
        after.objects["I"].text.as_bytes(),
        before.objects["I"].text.as_bytes()
    );
}
