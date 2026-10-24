//! Shared checks for the C0 stdout envelope; included with #[path].
#![allow(dead_code)]
use serde_json::Value;
use std::process::Output;

pub fn exit_for(code: &str) -> i32 {
    match code {
        "internal" => 1,
        "usage" | "too_long" => 2,
        "not_found" | "not_initialized" => 3,
        "lease_busy" | "stale_epoch" | "conflict" => 4,
        "pending" | "deferred" => 5,
        "guard_blocked" | "gate_failed" | "needs_proposal" | "rejected" => 6,
        "needs_human" => 7,
        "daemon_unavailable" | "hub_offline" => 8,
        other => panic!("unknown error code {other}"),
    }
}

/// stdout must hold exactly one envelope whose code matches the exit status.
pub fn parse(output: &Output) -> Value {
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not one JSON envelope ({error}): {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(envelope["v"], 1, "{envelope}");
    for key in ["delivered", "next", "warnings"] {
        assert!(envelope[key].is_array(), "{key}: {envelope}");
    }
    let ok = envelope["ok"].as_bool().expect("ok is a boolean");
    let exit = if ok { 0 } else { exit_for(envelope["error"]["code"].as_str().expect("error.code")) };
    assert_eq!(output.status.code(), Some(exit), "{envelope}\nstderr: {}", String::from_utf8_lossy(&output.stderr));
    validate(&load("envelope"), &envelope, "envelope");
    envelope
}

pub fn data(output: &Output) -> Value {
    let envelope = parse(output);
    assert_eq!(envelope["ok"], true, "{envelope}\nstderr: {}", String::from_utf8_lossy(&output.stderr));
    envelope["data"].clone()
}

pub fn code(output: &Output) -> String {
    let envelope = parse(output);
    assert_eq!(envelope["ok"], false, "{envelope}");
    envelope["error"]["code"].as_str().unwrap().to_owned()
}

const SCHEMA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/schema");

fn load(name: &str) -> Value {
    let text = std::fs::read_to_string(format!("{SCHEMA_DIR}/{name}.json")).unwrap_or_else(|error| panic!("schema {name}: {error}"));
    serde_json::from_str(&text).unwrap()
}

/// Checks the subset of JSON Schema used by crates/towermd/schema.
pub fn validate(schema: &Value, value: &Value, at: &str) {
    // A nullable type (`["object", "null"]`): null is valid and has nothing more to check.
    if let Some(kinds) = schema["type"].as_array() {
        if value.is_null() && kinds.contains(&Value::from("null")) {
            return;
        }
        let kinds: Vec<&Value> = kinds.iter().filter(|kind| *kind != "null").collect();
        if let [kind] = kinds.as_slice() {
            let mut single = schema.clone();
            single["type"] = (*kind).clone();
            return validate(&single, value, at);
        }
    }
    if let Some(kind) = schema["type"].as_str() {
        let ok = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "null" => value.is_null(),
            other => panic!("{at}: unsupported type {other}"),
        };
        assert!(ok, "{at}: expected {kind}, got {value}");
    }
    if let Some(allowed) = schema["enum"].as_array() {
        assert!(allowed.contains(value), "{at}: {value} not in {allowed:?}");
    }
    for key in schema["required"].as_array().into_iter().flatten() {
        let key = key.as_str().unwrap();
        assert!(value.get(key).is_some(), "{at}: missing {key} in {value}");
    }
    if let Some(properties) = schema["properties"].as_object() {
        for (key, sub) in properties {
            if let Some(child) = value.get(key) {
                validate(sub, child, &format!("{at}.{key}"));
            }
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, child) in array.iter().enumerate() {
            validate(items, child, &format!("{at}[{index}]"));
        }
    }
}

/// Dotted leaf name of a towermd argv, skipping global options.
pub fn command_name(args: &[&str]) -> String {
    let mut words = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "--run" | "--lang" | "--request-id" | "--project" => { iter.next(); }
            "--json" | "--text" | "-q" | "--quiet" => {}
            word if word.starts_with('-') => {}
            word => words.push(word),
        }
    }
    let depth = match words.first().copied() {
        Some("task" | "hub" | "daemon" | "idea" | "ext" | "template" | "frame" | "decision" | "rule") => 2,
        Some("dev") if matches!(words.get(1).copied(), Some("scan" | "draft")) => 3,
        Some("dev") => 2,
        _ => 1,
    };
    words.into_iter().take(depth).collect::<Vec<_>>().join(".")
}

/// Envelope plus per-command data check; returns `data`.
pub fn data_checked(args: &[&str], output: &Output) -> Value {
    let data = data(output);
    let name = match command_name(args) {
        name if name.is_empty() => "towermd".to_owned(),
        name => name,
    };
    validate(&load(&name), &data, &name);
    let read = args.contains(&"--wait") || matches!(name.as_str(), "show" | "task.show" | "status" | "wait" | "schema" | "dev.check" | "dev.scan.wait" | "dev.scan.show" | "dev.draft.template" | "dev.draft.show" | "dev.draft.ls" | "dev.draft.compare" | "daemon.ensure" | "daemon.status" | "daemon.stop" | "up" | "ext.ls" | "ext.show" | "template.ls" | "frame.show" | "frame.export"
        | "needs" | "since" | "overview" | "ls" | "task.ls" | "history" | "watch" | "open" | "decision.show" | "decision.ls" | "rule.ls"
        | "rebuild-index" | "doctor" | "whoami" | "towermd");
    if !read {
        assert!(data["request_id"].is_string(), "{name}: write without request_id: {data}");
    }
    data
}
