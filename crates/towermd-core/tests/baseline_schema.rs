use serde_json::{json, Value};
use towermd_core::baseline_schema::{parse_submit, schema};

fn minimal() -> Value {
    json!({"scan_id":"scan", "project_id":"project",
        "source_revision":"1111111111111111111111111111111111111111",
        "intents":[], "modules":[{"id":"A-api","path":"arch/A-api.md",
        "text":"---\ntype: Module\nid: A-api\n---\nAPI\n"}],
        "interfaces":[], "acceptances":[], "questions":[], "risks":[]})
}

#[test]
fn submit_rejects_wrong_shape_before_reading_objects() {
    let mut value = minimal();
    assert!(serde_json::from_str::<Value>(schema()).is_ok());
    assert!(parse_submit(&value).is_ok());
    value["modules"][0]["path"] = json!(3);
    assert!(parse_submit(&value)
        .unwrap_err()
        .contains("modules[0].path"));
    value["actor"] = json!("human:owner");
    assert!(parse_submit(&value).is_err());
}

#[test]
fn modules_require_at_least_one_object() {
    let mut value = minimal();
    value["modules"] = json!([]);
    assert!(parse_submit(&value).is_err());
}

#[test]
fn schema_is_the_single_embedded_source() {
    assert_eq!(
        schema(),
        include_str!("../schema/baseline-submit.schema.json")
    );
    let schema: Value = serde_json::from_str(schema()).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["$id"], "towermd:baseline-submit:v1");
}

#[test]
fn every_field_is_required_and_root_must_be_an_object() {
    let valid = minimal();
    for field in valid.as_object().unwrap().keys() {
        let mut value = valid.clone();
        value.as_object_mut().unwrap().remove(field);
        let error = parse_submit(&value).unwrap_err();
        assert!(error.contains(&format!("$.{field}")), "{error}");
    }
    for value in [Value::Null, json!([]), json!("submit"), json!(true)] {
        assert!(parse_submit(&value)
            .unwrap_err()
            .contains("$: expected object"));
    }
}

#[test]
fn each_collection_and_identifier_has_the_schema_type() {
    for field in ["scan_id", "project_id", "source_revision"] {
        let mut value = minimal();
        value[field] = json!(3);
        let error = parse_submit(&value).unwrap_err();
        assert!(
            error.contains(&format!("$.{field}: expected string")),
            "{error}"
        );
    }
    for field in [
        "intents",
        "modules",
        "interfaces",
        "acceptances",
        "questions",
        "risks",
    ] {
        let mut value = minimal();
        value[field] = json!({});
        let error = parse_submit(&value).unwrap_err();
        assert!(
            error.contains(&format!("$.{field}: expected array")),
            "{error}"
        );
    }
}

#[test]
fn object_envelopes_reject_missing_empty_wrong_type_and_unknown_fields() {
    for collection in ["intents", "modules", "interfaces", "acceptances"] {
        let mut base = minimal();
        base[collection] = minimal()["modules"].clone();
        for field in ["id", "path", "text"] {
            for invalid in [Value::Null, json!(7), json!(""), json!([])] {
                let mut value = base.clone();
                value[collection][0][field] = invalid;
                let error = parse_submit(&value).unwrap_err();
                assert!(
                    error.contains(&format!("$.{collection}[0].{field}")),
                    "{error}"
                );
            }
            let mut value = base.clone();
            value[collection][0].as_object_mut().unwrap().remove(field);
            let error = parse_submit(&value).unwrap_err();
            assert!(
                error.contains(&format!("$.{collection}[0].{field}")),
                "{error}"
            );
        }
        let mut value = base.clone();
        value[collection][0]["actor"] = json!("human:owner");
        let error = parse_submit(&value).unwrap_err();
        assert!(
            error.contains(&format!("$.{collection}[0].actor")),
            "{error}"
        );
        value[collection] = json!(["raw text"]);
        let error = parse_submit(&value).unwrap_err();
        assert!(
            error.contains(&format!("$.{collection}[0]: expected object")),
            "{error}"
        );
    }
}

#[test]
fn questions_and_risks_items_are_strings_with_full_index_paths() {
    for field in ["questions", "risks"] {
        let mut value = minimal();
        value[field] = json!(["", "需要确认"]);
        assert!(parse_submit(&value).is_ok());
        value[field][1] = json!({"text":"invalid"});
        let error = parse_submit(&value).unwrap_err();
        assert!(
            error.contains(&format!("$.{field}[1]: expected string")),
            "{error}"
        );
    }
}

#[test]
fn actor_and_other_unknown_keys_are_rejected_with_unambiguous_paths() {
    for (key, path) in [("actor", "$.actor"), ("unknown.key", "$[\"unknown.key\"]")] {
        let mut value = minimal();
        value[key] = json!("human:owner");
        let error = parse_submit(&value).unwrap_err();
        assert!(error.contains(path), "{error}");
    }
}

#[test]
fn shape_parsing_preserves_raw_text_for_the_later_semantic_check() {
    let mut value = minimal();
    let text = "---\r\ntype: UnknownType\r\ncustom: retained\r\n---\r\n正文";
    value["modules"][0]["text"] = json!(text);
    let parsed = parse_submit(&value).unwrap();
    assert_eq!(parsed.modules[0].text, text);
    assert_eq!(serde_json::to_value(parsed).unwrap(), value);
}
