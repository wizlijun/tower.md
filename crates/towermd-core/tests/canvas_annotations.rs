use serde_json::{json, Value};
use std::collections::BTreeMap;
use towermd_core::baseline_draft::*;

fn bundle() -> DraftBundle {
    DraftBundle {
        id: "D-1".into(),
        project_id: "p".into(),
        docs_dir: "docs/tower".into(),
        source_revision: "a".repeat(40),
        target_baseline: "towermd/B0".into(),
        objects: BTreeMap::new(),
        deletions: vec![],
        questions: vec![],
        risks: vec![],
        canvas_text: format!(
            "{}\r\n",
            json!({
                "x-root":{"future":1.25},
                "nodes":[
                    {"id":"A","type":"file","file":"docs/tower/arch/A.md","towermd":{"obj":"A","unknown":[2,1]},"x":0,"y":0,"width":300,"height":160},
                    {"id":"B","type":"file","file":"docs/tower/arch/B.md","towermd":{"obj":"B"},"x":400,"y":0,"width":300,"height":160},
                    {"id":"note","type":"text","text":"原文","x":20,"y":250,"width":300,"height":160,"unknown":{"values":[2,1],"fraction":0.125}},
                    {"id":"future","type":"future","raw":[2,1]}
                ],
                "edges":[
                    {"id":"interface","fromNode":"A","toNode":"B","towermd":{"obj":"I"},"label":"API"},
                    {"id":"future-edge","raw":{"future":true}}
                ]
            })
        ),
    }
}
fn raw(bundle: &DraftBundle) -> Value {
    serde_json::from_str(&bundle.canvas_text).unwrap()
}
fn node(id: &str, content: AnnotationContent) -> AnnotationNode {
    AnnotationNode {
        id: id.into(),
        x: 20,
        y: 250,
        width: 300,
        height: 160,
        content,
    }
}
fn put_text(before: &DraftBundle, id: &str, text: &str) -> DraftBundle {
    apply_edit(
        before,
        &DraftEdit::PutAnnotation {
            node: node(id, AnnotationContent::Text { text: text.into() }),
        },
    )
    .unwrap()
}
fn relation(id: &str, from: &str, to: &str) -> DraftEdit {
    DraftEdit::PutRelation {
        relation: AnnotationRelation {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            label: "关联".into(),
        },
    }
}

#[test]
fn isolated_cards_and_non_tree_relations_share_the_object_canvas() {
    let before = bundle();
    let first = put_text(&before, "one", "概念一");
    let second = put_text(&first, "two", "概念二");
    let third = put_text(&second, "isolated", "孤立概念");
    let graph = apply_edit(
        &apply_edit(
            &apply_edit(&third, &relation("r1", "one", "two")).unwrap(),
            &relation("r2", "two", "one"),
        )
        .unwrap(),
        &relation("r3", "A", "one"),
    )
    .unwrap();
    let graph_raw = raw(&graph);
    assert_eq!(graph_raw["nodes"].as_array().unwrap().len(), 7);
    assert_eq!(graph_raw["edges"].as_array().unwrap().len(), 5);
    assert_eq!(graph_raw["nodes"][0], raw(&before)["nodes"][0]);
    assert_eq!(graph_raw["nodes"][3], raw(&before)["nodes"][3]);
    assert_eq!(graph_raw["edges"][0], raw(&before)["edges"][0]);
    assert_eq!(graph_raw["edges"][1], raw(&before)["edges"][1]);
    assert_eq!(graph_raw["x-root"], raw(&before)["x-root"]);
    assert_eq!(graph.objects, before.objects);
    assert_eq!(graph.deletions, before.deletions);
}

#[test]
fn annotation_updates_preserve_extensions_and_no_op_preserves_original_bytes() {
    let before = bundle();
    assert_eq!(
        put_text(&before, "note", "原文").canvas_text,
        before.canvas_text
    );
    let changed = put_text(&before, "note", "修改后的正文");
    assert_eq!(
        raw(&changed)["nodes"][2]["unknown"],
        raw(&before)["nodes"][2]["unknown"]
    );
    let linked = apply_edit(
        &changed,
        &DraftEdit::PutAnnotation {
            node: node(
                "note",
                AnnotationContent::Link {
                    url: "https://example.invalid/reference".into(),
                },
            ),
        },
    )
    .unwrap();
    assert_eq!(raw(&linked)["nodes"][2]["type"], "link");
    assert!(raw(&linked)["nodes"][2].get("text").is_none());
    assert_eq!(
        raw(&linked)["nodes"][2]["unknown"],
        raw(&before)["nodes"][2]["unknown"]
    );
    let mut with_extension = raw(&linked);
    with_extension["edges"].as_array_mut().unwrap().push(json!({"id":"r","fromNode":"note","toNode":"A","label":"旧关系","custom":[2,1],"fromSide":"right"}));
    let linked = DraftBundle {
        canvas_text: with_extension.to_string(),
        ..linked
    };
    let edited = apply_edit(&linked, &relation("r", "note", "B")).unwrap();
    assert_eq!(raw(&edited)["edges"][2]["custom"], json!([2, 1]));
    assert_eq!(raw(&edited)["edges"][2]["fromSide"], "right");
    assert_eq!(raw(&edited)["edges"][2]["toNode"], "B");
}

#[test]
fn changing_annotation_kind_preserves_fields_unknown_to_the_original_kind() {
    let mut value = raw(&bundle());
    value["nodes"][2]["label"] = json!("future label extension");
    let before = DraftBundle {
        canvas_text: value.to_string(),
        ..bundle()
    };
    let after = apply_edit(
        &before,
        &DraftEdit::PutAnnotation {
            node: node(
                "note",
                AnnotationContent::Link {
                    url: "https://example.invalid/reference".into(),
                },
            ),
        },
    )
    .unwrap();
    assert_eq!(raw(&after)["nodes"][2]["label"], "future label extension");
    assert!(raw(&after)["nodes"][2].get("text").is_none());
}

#[test]
fn deleting_annotation_removes_only_its_annotation_relations() {
    let before = apply_edit(&bundle(), &relation("r", "note", "A")).unwrap();
    let after = apply_edit(
        &before,
        &DraftEdit::RemoveAnnotation {
            node: "note".into(),
        },
    )
    .unwrap();
    assert_eq!(raw(&after)["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(raw(&after)["edges"], raw(&bundle())["edges"]);
    let before = apply_edit(&bundle(), &relation("r", "note", "A")).unwrap();
    let after = apply_edit(&before, &DraftEdit::RemoveRelation { edge: "r".into() }).unwrap();
    assert_eq!(raw(&after)["nodes"], raw(&before)["nodes"]);
    assert_eq!(raw(&after)["edges"], raw(&bundle())["edges"]);
}

#[test]
fn group_creation_and_removal_never_moves_or_deletes_other_nodes() {
    let before = bundle();
    let grouped = apply_edit(
        &before,
        &DraftEdit::PutAnnotation {
            node: AnnotationNode {
                id: "group".into(),
                x: -50,
                y: -50,
                width: 1000,
                height: 800,
                content: AnnotationContent::Group {
                    label: "概念分组".into(),
                },
            },
        },
    )
    .unwrap();
    let removed = apply_edit(
        &grouped,
        &DraftEdit::RemoveAnnotation {
            node: "group".into(),
        },
    )
    .unwrap();
    assert_eq!(raw(&removed), raw(&before));
}

#[test]
fn object_bindings_opaque_targets_and_invalid_geometry_cannot_be_overwritten() {
    let before = bundle();
    for edit in [
        DraftEdit::PutAnnotation {
            node: node(
                "A",
                AnnotationContent::Text {
                    text: "覆盖对象".into(),
                },
            ),
        },
        DraftEdit::RemoveAnnotation { node: "A".into() },
        DraftEdit::RemoveAnnotation {
            node: "future".into(),
        },
        DraftEdit::RemoveRelation {
            edge: "interface".into(),
        },
        DraftEdit::RemoveRelation {
            edge: "future-edge".into(),
        },
        relation("interface", "A", "note"),
        relation("future-edge", "A", "note"),
        relation("bad", "missing", "note"),
        relation("bad", "future", "note"),
        DraftEdit::PutAnnotation {
            node: AnnotationNode {
                width: 0,
                ..node("new", AnnotationContent::Text { text: "".into() })
            },
        },
        DraftEdit::PutAnnotation {
            node: AnnotationNode {
                x: 9_007_199_254_740_992,
                ..node("new", AnnotationContent::Text { text: "".into() })
            },
        },
    ] {
        assert!(apply_edit(&before, &edit).is_err(), "{edit:?}");
        assert_eq!(before.canvas_text, bundle().canvas_text);
    }
    for edge in [
        json!({"id":"bound","fromNode":"note","toNode":"A","towermd":{"obj":"I"}}),
        json!({"id":"opaque","fromNode":"note"}),
    ] {
        let mut value = raw(&before);
        value["edges"].as_array_mut().unwrap().push(edge);
        let protected = DraftBundle {
            canvas_text: value.to_string(),
            ..before.clone()
        };
        assert!(apply_edit(
            &protected,
            &DraftEdit::RemoveAnnotation {
                node: "note".into()
            }
        )
        .is_err());
    }
}

#[test]
fn annotation_edit_wire_is_finite_and_rejects_actor_binding_or_fractional_coordinates() {
    let valid = json!({"action":"put_annotation","node":{"id":"x","x":0,"y":0,"width":300,"height":160,"content":{"type":"text","text":""}}});
    assert!(serde_json::from_value::<DraftEdit>(valid.clone()).is_ok());
    for (location, key, value) in [
        ("", "actor", json!("human:owner")),
        ("/node", "towermd", json!({"obj":"A"})),
        ("/node", "x", json!(0.5)),
        ("/node/content", "type", json!("file")),
        ("/node/content", "unknown", json!(true)),
    ] {
        let mut invalid = valid.clone();
        invalid.pointer_mut(location).unwrap()[key] = value;
        assert!(serde_json::from_value::<DraftEdit>(invalid).is_err());
    }
}
