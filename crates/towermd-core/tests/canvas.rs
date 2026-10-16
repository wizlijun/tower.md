use serde_json::{json, Value};
use towermd_core::canvas::CanvasDocument;

fn fixture() -> Value {
    json!({
        "extension": {"nested": [true, null, "保留"]},
        "nodes": [
            {"id":"text", "type":"text", "x":-12, "y":-8, "width":100, "height":80,
             "text":"文字", "extra":{"keep":[1,2]}},
            {"id":"file", "type":"file", "x":20, "y":30, "width":120, "height":90,
             "file":"docs/module.md", "towermd":{"obj":"module-1", "future":true}},
            {"id":"link", "type":"link", "x":0, "y":0, "width":50, "height":40,
             "url":"https://example.com"},
            {"id":"group", "type":"group", "x":-50, "y":-50, "width":400, "height":300,
             "label":"组", "backgroundStyle":"cover"}
        ],
        "edges": [
            {"id":"edge", "fromNode":"text", "toNode":"file", "extension":{"keep":true}},
            {"id":"self", "fromNode":"group", "toNode":"group", "fromEnd":"none", "toEnd":"none"}
        ]
    })
}

#[test]
fn unmodified_canvas_is_byte_identical() {
    let source = serde_json::to_string_pretty(&fixture())
        .unwrap()
        .replace('\n', "\r\n");
    let document = CanvasDocument::decode(&source).unwrap();
    assert_eq!(document.encode().as_bytes(), source.as_bytes());
    assert_eq!(
        document
            .nodes()
            .iter()
            .map(|entry| entry.raw["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["text", "file", "link", "group"]
    );
    assert!(document.nodes().iter().all(|entry| !entry.opaque));
    assert_eq!(document.nodes()[3].index, 3);
    assert_eq!(document.edges().len(), 2);
    assert_eq!(document.edges()[0].raw["id"], "edge");
    assert!(document.edges()[0].raw.get("toEnd").is_none());
    assert_eq!(document.edges()[1].raw["toEnd"], "none");
    assert!(document.diagnostics().is_empty());
}

#[test]
fn empty_and_missing_arrays_are_not_materialized() {
    for source in ["{}", " {\"nodes\":[]}\n", "{\"edges\":[]}"] {
        let document = CanvasDocument::decode(source).unwrap();
        assert!(document.nodes().is_empty());
        assert!(document.edges().is_empty());
        assert!(document.diagnostics().is_empty());
        assert_eq!(document.encode(), source);
    }
}

#[test]
fn invalid_root_never_becomes_empty_canvas() {
    for source in [
        "",
        "{",
        "{}[]",
        "[]",
        "null",
        "42",
        "\"canvas\"",
        "{\"nodes\":{}}",
        "{\"edges\":null}",
        r#"{"nodes":{},"extension":0.1234567890123456789}"#,
        r#"{"edges":null,"extension":1e999}"#,
    ] {
        assert!(CanvasDocument::decode(source).is_err(), "{source}");
    }
}

#[test]
fn generated_canvas_uses_only_document_fields() {
    let mut value = fixture();
    value.as_object_mut().unwrap().remove("extension");
    value["nodes"][0].as_object_mut().unwrap().remove("extra");
    value["nodes"][1]["towermd"]
        .as_object_mut()
        .unwrap()
        .remove("future");
    value["edges"][0]
        .as_object_mut()
        .unwrap()
        .remove("extension");
    let document = CanvasDocument::from_value(value.clone()).unwrap();
    assert_eq!(document.raw(), Some(&value));
    assert_eq!(document.nodes().len(), 4);
    assert_eq!(document.edges().len(), 2);
    assert_eq!(
        serde_json::from_str::<Value>(&document.encode()).unwrap(),
        value
    );
    assert_eq!(
        document.encode(),
        CanvasDocument::from_value(value).unwrap().encode()
    );
    assert!(document.diagnostics().is_empty());
    assert_eq!(
        CanvasDocument::from_value(json!({})).unwrap().encode(),
        "{}"
    );
}

#[test]
fn candidates_reject_runtime_state_and_invalid_graphs() {
    let node = fixture()["nodes"][2].clone();
    let baseline = json!({"nodes":[node],"edges":[]});
    for field in [
        "viewport",
        "selection",
        "selected",
        "hover",
        "health",
        "cost",
        "badges",
        "taskGraph",
        "position",
        "data",
    ] {
        let mut root_value = baseline.clone();
        root_value[field] = json!(true);
        assert!(
            CanvasDocument::from_value(root_value).is_err(),
            "root {field}"
        );
        let mut node_value = baseline.clone();
        node_value["nodes"][0][field] = json!(true);
        assert!(
            CanvasDocument::from_value(node_value).is_err(),
            "node {field}"
        );
        let mut extension_value = baseline.clone();
        extension_value["nodes"][0]["towermd"] = json!({field:true});
        assert!(
            CanvasDocument::from_value(extension_value).is_err(),
            "extension {field}"
        );
        let edge_value = json!({"nodes":[node],"edges":[{"id":"edge","fromNode":"link","toNode":"link",field:true}]});
        assert!(
            CanvasDocument::from_value(edge_value).is_err(),
            "edge {field}"
        );
    }
    for invalid in [
        json!(null),
        json!({"nodes":null}),
        json!({"nodes":[node,node]}),
        json!({"nodes":[42]}),
        json!({"edges":[{"id":"missing","fromNode":"link","toNode":"link"}]}),
    ] {
        assert!(CanvasDocument::from_value(invalid).is_err());
    }
    let mut invalid_optional = baseline.clone();
    invalid_optional["nodes"][0]["color"] = json!(false);
    assert!(CanvasDocument::from_value(invalid_optional).is_err());
}

#[test]
fn raw_projection_is_borrowed_and_unavailable_for_ambiguous_json() {
    let value = fixture();
    assert_eq!(
        CanvasDocument::decode(&value.to_string()).unwrap().raw(),
        Some(&value)
    );
    for source in [r#"{"key":1,"key":2}"#, r#"{"extension":1e999}"#] {
        let document = CanvasDocument::decode(source).unwrap();
        assert!(document.raw().is_none());
        assert_eq!(document.encode(), source);
    }
}

#[test]
fn invalid_entries_keep_position_and_raw_content() {
    let first = fixture()["nodes"][0].clone();
    let mut unknown = first.clone();
    unknown["type"] = json!("future");
    let mut missing_id = first.clone();
    missing_id.as_object_mut().unwrap().remove("id");
    let mut zero_width = first.clone();
    zero_width["width"] = json!(0);
    let mut fractional = first.clone();
    fractional["x"] = json!(0.5);
    let nodes = json!([
        first,
        unknown,
        7,
        missing_id,
        zero_width,
        fractional,
        fixture()["nodes"][1]
    ]);
    let source =
        json!({"nodes":nodes,"edges":[false, {"id":"broken","fromNode":7,"toNode":"file"}]})
            .to_string();
    let document = CanvasDocument::decode(&source).unwrap();
    assert_eq!(document.nodes().len(), 7);
    assert!(!document.nodes()[0].opaque);
    assert!(!document.nodes()[6].opaque);
    for index in 1..6 {
        let entry = &document.nodes()[index];
        assert!(entry.opaque, "index {index}");
        assert_eq!(entry.index, index);
        assert_eq!(entry.raw, nodes[index]);
        assert!(document
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.collection == Some("nodes")
                && diagnostic.index == Some(index)));
    }
    assert!(document.edges().iter().all(|entry| entry.opaque));
    assert_eq!(document.encode(), source);
}

#[test]
fn all_node_types_require_safe_integer_geometry_and_positive_size() {
    for template in fixture()["nodes"].as_array().unwrap() {
        for field in ["x", "y", "width", "height"] {
            for invalid in [
                json!(null),
                json!("1"),
                json!(true),
                json!(0.25),
                json!(9007199254740992_u64),
                json!(-9007199254740992_i64),
            ] {
                let mut node = template.clone();
                node[field] = invalid;
                let document =
                    CanvasDocument::decode(&json!({"nodes":[node]}).to_string()).unwrap();
                assert!(document.nodes()[0].opaque, "{field}: {node}");
            }
        }
        for field in ["width", "height"] {
            for invalid in [0, -1] {
                let mut node = template.clone();
                node[field] = json!(invalid);
                assert!(
                    CanvasDocument::decode(&json!({"nodes":[node]}).to_string())
                        .unwrap()
                        .nodes()[0]
                        .opaque
                );
            }
        }
        let mut boundary = template.clone();
        boundary["x"] = json!(-9007199254740991_i64);
        boundary["y"] = json!(9007199254740991_i64);
        boundary["width"] = json!(9007199254740991_i64);
        boundary["height"] = json!(1);
        let document = CanvasDocument::decode(&json!({"nodes":[boundary]}).to_string()).unwrap();
        assert!(!document.nodes()[0].opaque);
        assert!(document.diagnostics().is_empty());
    }
}

#[test]
fn invalid_required_payloads_are_opaque() {
    for (index, field) in [(0, "text"), (1, "file"), (2, "url")] {
        for value in [None, Some(json!(null)), Some(json!(123))] {
            let mut node = fixture()["nodes"][index].clone();
            node.as_object_mut().unwrap().remove(field);
            if let Some(value) = value {
                node[field] = value;
            }
            let document = CanvasDocument::decode(&json!({"nodes":[node]}).to_string()).unwrap();
            assert!(document.nodes()[0].opaque, "{field}");
        }
    }
}

#[test]
fn optional_invalid_fields_are_not_repaired() {
    let mut value = fixture();
    value["nodes"][0]["color"] = json!(9);
    value["nodes"][1]["subpath"] = json!(false);
    value["nodes"][3]["backgroundStyle"] = json!("future");
    value["nodes"][3]["label"] = json!(null);
    value["nodes"][3]["background"] = json!(3);
    value["edges"][0]["fromSide"] = json!("diagonal");
    value["edges"][0]["toEnd"] = json!("circle");
    value["edges"][0]["label"] = json!({});
    let source = value.to_string();
    let document = CanvasDocument::decode(&source).unwrap();
    assert!(document.nodes().iter().all(|entry| !entry.opaque));
    assert!(document.edges().iter().all(|entry| !entry.opaque));
    assert_eq!(
        document
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code == "invalid_optional_field")
            .count(),
        8
    );
    assert_eq!(document.encode(), source);
}

#[test]
fn duplicate_ids_and_dangling_edges_are_diagnosed_without_removal() {
    let mut value = fixture();
    let duplicate_node = value["nodes"][0].clone();
    value["nodes"].as_array_mut().unwrap().push(duplicate_node);
    let duplicate_edge = value["edges"][0].clone();
    value["edges"].as_array_mut().unwrap().push(duplicate_edge);
    value["edges"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"dangling","fromNode":"absent","toNode":"file"}));
    let source = value.to_string();
    let document = CanvasDocument::decode(&source).unwrap();
    assert_eq!(document.nodes().len(), 5);
    assert_eq!(document.edges().len(), 4);
    for (collection, indices) in [("nodes", vec![0, 4]), ("edges", vec![0, 2])] {
        for index in indices {
            assert!(document
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == "duplicate_id"
                    && diagnostic.collection == Some(collection)
                    && diagnostic.index == Some(index)));
        }
    }
    assert!(document
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "ambiguous_endpoint" && diagnostic.index == Some(0)));
    assert!(document
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "dangling_edge" && diagnostic.index == Some(3)));
    assert_eq!(document.encode(), source);
}

#[test]
fn node_and_edge_ids_have_separate_namespaces() {
    let mut value = fixture();
    value["edges"][0]["id"] = json!("text");
    let document = CanvasDocument::decode(&value.to_string()).unwrap();
    assert!(document.diagnostics().is_empty());
}

#[test]
fn duplicate_properties_disable_projection_without_data_loss() {
    for source in [
        r#"{"nodes":[],"nodes":[]}"#,
        r#"{"extension":{"name":1,"name":2}}"#,
        r#"{"nodes":[{"id":"first","\u0069d":"last"}]}"#,
        r#"{"edges":[{"extra":[{"key":true,"key":false}]}]}"#,
    ] {
        let document = CanvasDocument::decode(source).unwrap();
        assert_eq!(document.encode(), source);
        assert!(document.nodes().is_empty());
        assert!(document.edges().is_empty());
        assert!(document
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "duplicate_property"));
    }
}

#[test]
fn large_unknown_numbers_round_trip_verbatim() {
    for token in [
        "184467440737095516160001",
        "0.12345678901234567890123456789",
        "1.234567890123456789e80",
        "1e999",
    ] {
        let source = format!("{{\"nodes\":[],\"extension\":{token}}}");
        let document = CanvasDocument::decode(&source).unwrap();
        assert_eq!(document.encode(), source);
        assert!(
            document
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == "unrepresentable_number"),
            "{token}"
        );
    }
}

#[test]
fn imported_runtime_fields_are_preserved_but_diagnosed() {
    let mut value = fixture();
    value["viewport"] = json!({"x":0,"y":0,"zoom":1});
    value["nodes"][0]["selected"] = json!(true);
    value["edges"][0]["towermd"] = json!({"health":"red", "cost":null});
    let source = value.to_string();
    let document = CanvasDocument::decode(&source).unwrap();
    assert_eq!(document.encode(), source);
    assert!(document
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "runtime_field" && diagnostic.collection.is_none()));
    assert!(document
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "runtime_field"
            && diagnostic.collection == Some("nodes")
            && diagnostic.index == Some(0)));
    assert!(document
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "runtime_field"
            && diagnostic.collection == Some("edges")
            && diagnostic.index == Some(0)));
}

#[test]
fn diagnostics_serialize_with_original_entry_indices() {
    let document = CanvasDocument::decode(r#"{"nodes":[false,null]}"#).unwrap();
    let diagnostics = serde_json::to_value(document.diagnostics()).unwrap();
    assert_eq!(diagnostics[0]["code"], "invalid_entry");
    assert_eq!(diagnostics[0]["collection"], "nodes");
    assert_eq!(diagnostics[0]["index"], 0);
    assert_eq!(diagnostics[1]["index"], 1);
    assert!(!diagnostics[0]["message"].as_str().unwrap().is_empty());
}

#[test]
fn generated_canvas_accepts_coldstart_region_metadata() {
    let mut node = fixture()["nodes"][3].clone();
    node["towermd"] = json!({"region":"unassigned"});
    let value = json!({"nodes":[node], "edges":[]});
    let document = CanvasDocument::from_value(value.clone()).unwrap();
    assert_eq!(document.raw(), Some(&value));
    assert_eq!(document.nodes()[0].raw["towermd"]["region"], "unassigned");
}

#[test]
fn duplicate_property_diagnostics_keep_nested_entry_index() {
    let source = r#"{"nodes":[{}, {"extra":[{"key":1,"key":2}]}]}"#;
    let document = CanvasDocument::decode(source).unwrap();
    let diagnostic = &document.diagnostics()[0];
    assert_eq!(diagnostic.code, "duplicate_property");
    assert_eq!(diagnostic.collection, Some("nodes"));
    assert_eq!(diagnostic.index, Some(1));
    assert!(document.raw().is_none());
    assert_eq!(document.encode(), source);
}
