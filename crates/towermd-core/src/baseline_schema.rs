//! S6 submission shape. Object semantics and scan provenance are checked separately.
use crate::recovery_contract::baseline::DraftObject;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineSubmit {
    pub scan_id: String,
    pub project_id: String,
    pub source_revision: String,
    pub intents: Vec<DraftObject>,
    pub modules: Vec<DraftObject>,
    pub interfaces: Vec<DraftObject>,
    pub acceptances: Vec<DraftObject>,
    pub questions: Vec<String>,
    pub risks: Vec<String>,
}

pub fn schema() -> &'static str {
    include_str!("../schema/baseline-submit.schema.json")
}

pub fn parse_submit(value: &Value) -> Result<BaselineSubmit, String> {
    static COMPILED: OnceLock<Result<Value, String>> = OnceLock::new();
    let root = COMPILED.get_or_init(|| {
        let root: Value =
            serde_json::from_str(schema()).map_err(|error| format!("schema $: {error}"))?;
        check_schema(&root, &root, "$", &mut Vec::new())?;
        Ok(root)
    });
    let root = root.as_ref().map_err(Clone::clone)?;
    validate(root, root, value, "$")?;
    serde_json::from_value(value.clone()).map_err(|error| format!("$: {error}"))
}

fn property_path(path: &str, key: &str) -> String {
    if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        format!("{path}.{key}")
    } else {
        format!("{path}[{}]", serde_json::to_string(key).unwrap())
    }
}

fn referenced<'a>(root: &'a Value, reference: &str) -> Result<&'a Value, String> {
    reference
        .strip_prefix('#')
        .and_then(|pointer| root.pointer(pointer))
        .ok_or_else(|| format!("unsupported or unresolved schema reference {reference}"))
}

// Fail closed before validating any input, including unused definitions. These are
// the only vocabulary and value forms used by the shipped Draft 2020-12 schema.
fn check_schema(
    root: &Value,
    node: &Value,
    path: &str,
    references: &mut Vec<String>,
) -> Result<(), String> {
    let object = node
        .as_object()
        .ok_or_else(|| format!("schema {path}: expected object"))?;
    for (key, value) in object {
        let location = property_path(path, key);
        let malformed = || format!("schema {location}: unsupported keyword value");
        match key.as_str() {
            "$schema" | "$id" => {
                value.as_str().ok_or_else(malformed)?;
            }
            "$ref" => {
                let reference = value.as_str().ok_or_else(malformed)?;
                if references.iter().any(|seen| seen == reference) {
                    return Err(format!("schema {location}: cyclic reference {reference}"));
                }
                references.push(reference.to_owned());
                check_schema(root, referenced(root, reference)?, &location, references)?;
                references.pop();
            }
            "$defs" | "properties" => {
                for (name, child) in value.as_object().ok_or_else(malformed)? {
                    check_schema(root, child, &property_path(&location, name), references)?;
                }
            }
            "type" => {
                if !matches!(value.as_str(), Some("object" | "array" | "string")) {
                    return Err(malformed());
                }
            }
            "required" => {
                let fields = value.as_array().ok_or_else(malformed)?;
                if fields.iter().any(|field| !field.is_string()) {
                    return Err(malformed());
                }
            }
            "additionalProperties" => {
                if value != &Value::Bool(false) {
                    return Err(malformed());
                }
            }
            "items" => check_schema(root, value, &location, references)?,
            "minItems" | "minLength" => {
                value.as_u64().ok_or_else(malformed)?;
            }
            _ => return Err(format!("schema {location}: unsupported keyword")),
        }
    }
    Ok(())
}

fn validate(root: &Value, node: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
        validate(root, referenced(root, reference)?, value, path)?;
    }
    if let Some(expected) = node.get("type").and_then(Value::as_str) {
        let matches = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            _ => false,
        };
        if !matches {
            return Err(format!("{path}: expected {expected}"));
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = node.get("required").and_then(Value::as_array) {
            for key in required {
                let key = key.as_str().unwrap(); // checked once by check_schema
                if !object.contains_key(key) {
                    return Err(format!(
                        "{}: required property is missing",
                        property_path(path, key)
                    ));
                }
            }
        }
        let properties = node.get("properties").and_then(Value::as_object);
        for (key, child) in object {
            let location = property_path(path, key);
            if let Some(rule) = properties.and_then(|properties| properties.get(key)) {
                validate(root, rule, child, &location)?;
            } else if node.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(format!("{location}: additional property is forbidden"));
            }
        }
    }
    if let Some(array) = value.as_array() {
        if let Some(minimum) = node.get("minItems").and_then(Value::as_u64) {
            if (array.len() as u64) < minimum {
                return Err(format!("{path}: expected at least {minimum} items"));
            }
        }
        if let Some(items) = node.get("items") {
            for (index, child) in array.iter().enumerate() {
                validate(root, items, child, &format!("{path}[{index}]"))?;
            }
        }
    }
    if let (Some(text), Some(minimum)) = (
        value.as_str(),
        node.get("minLength").and_then(Value::as_u64),
    ) {
        if (text.chars().count() as u64) < minimum {
            return Err(format!("{path}: expected at least {minimum} characters"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_schema_keywords_are_rejected_even_in_unused_definitions() {
        for location in ["", "/$defs/object", "/properties/modules/items"] {
            let mut root: Value = serde_json::from_str(schema()).unwrap();
            root.pointer_mut(location).unwrap()["maxItems"] = json!(1);
            let error = check_schema(&root, &root, "$", &mut Vec::new()).unwrap_err();
            assert!(error.contains("maxItems"), "{error}");
        }
    }

    #[test]
    fn constraints_come_from_schema_and_unicode_length_counts_characters() {
        let mut root: Value = serde_json::from_str(schema()).unwrap();
        root["properties"]["modules"]["minItems"] = json!(2);
        check_schema(&root, &root, "$", &mut Vec::new()).unwrap();
        let value = json!([{"id":"模", "path":"a", "text":"x"}]);
        let rule = &root["properties"]["modules"];
        assert!(validate(&root, rule, &value, "$.modules").is_err());
        root["properties"]["modules"]["minItems"] = json!(1);
        let rule = &root["properties"]["modules"];
        validate(&root, rule, &value, "$.modules").unwrap();
        root["$defs"]["object"]["properties"]["id"]["minLength"] = json!(2);
        let rule = &root["properties"]["modules"];
        assert!(validate(&root, rule, &value, "$.modules")
            .unwrap_err()
            .contains("$.modules[0].id"));
    }

    #[test]
    fn unsupported_keyword_values_and_cyclic_references_are_rejected() {
        for malformed in [
            json!({"type":"integer"}),
            json!({"required":[3]}),
            json!({"additionalProperties":true}),
            json!({"minLength":-1}),
            json!({"items":false}),
            json!({"$ref":"#/$defs/missing"}),
            json!({"$ref":"#"}),
        ] {
            assert!(check_schema(&malformed, &malformed, "$", &mut Vec::new()).is_err());
        }
    }
}
