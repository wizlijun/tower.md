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
