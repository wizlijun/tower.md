//! JSON-RPC frames and tool schemas of the interaction gateway. No IO, no
//! state. Four tools, none of them an approval: a person approves only in
//! the App.
use serde_json::{json, Value};

use towermd_devkit::gateway::{ASK, NOTE, PERMISSION, REVIEW};

/// Only a fallback: whatever version the client sends is echoed.
pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

pub fn initialize_result(client_version: Option<&str>) -> Value {
    json!({
        "protocolVersion": client_version.unwrap_or(DEFAULT_PROTOCOL_VERSION),
        "capabilities": {"tools": {}},
        "serverInfo": {"name": towermd_devkit::gateway::SERVER, "version": env!("CARGO_PKG_VERSION")},
    })
}

/// Every tool result but a permission's may carry `delivered` (FR-60).
const DELIVERED: &str = " Any result may carry delivered: answers and interjections from the person since your last call; read and follow them.";

pub fn tool_definitions() -> Value {
    let strings = json!({"type": "array", "items": {"type": "string"}});
    json!({"tools": [
        {
            "name": ASK,
            "description": "Ask the person a question and wait for the answer, up to 10 minutes. If nobody answers in time the question is parked: end the turn, report status needs_input and say what you are waiting for; the answer arrives when the run continues.".to_owned() + DELIVERED,
            "inputSchema": {"type": "object", "properties": {
                "question": {"type": "string"},
                "options": strings,
                "recommended": {"type": "integer", "description": "Index of the option you recommend."},
                "context": strings,
            }, "required": ["question"]}
        },
        {
            "name": REVIEW,
            "description": "Ask a person to review the plan (stage plan), the design (stage design) or the code (stage code, after towermd dev finish passed) and wait up to 10 minutes. A review never passes by waiting: if it is still open, end the turn with status needs_input.".to_owned() + DELIVERED,
            "inputSchema": {"type": "object", "properties": {
                "stage": {"type": "string", "enum": towermd_devkit::task_reviews::STAGES},
                "summary": {"type": "string"},
                "artifacts": strings,
            }, "required": ["stage", "summary"]}
        },
        {
            "name": PERMISSION,
            "description": "Called by the harness when a tool needs permission. A person allows it once or denies it in the App; no answer within 10 minutes is a denial.",
            "inputSchema": {"type": "object", "additionalProperties": true}
        },
        {
            "name": NOTE,
            "description": "Tell the person where the run stands, without waiting. Progress counts only with evidence; this is a note, not a completion.".to_owned() + DELIVERED,
            "inputSchema": {"type": "object", "properties": {
                "summary": {"type": "string"},
                "next": {"type": "string"},
            }, "required": ["summary"]}
        }
    ]})
}

pub fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// A failed tool is a result with `isError`, not a protocol error, so the
/// delegate reads why and carries on.
pub fn tool_error(id: &Value, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {"isError": true, "content": [{"type": "text", "text": message}]}})
}

pub fn tool_ok(id: &Value, payload: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": serde_json::to_string(payload).unwrap_or_default()}]}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_tools_and_none_approves() {
        let names: Vec<String> = tool_definitions()["tools"].as_array().unwrap().iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned()).collect();
        assert_eq!(names, [ASK, REVIEW, PERMISSION, NOTE]);
        for name in &names {
            for word in ["approve", "resolve", "answer", "allow", "pass", "merge", "complete"] {
                assert!(!name.contains(word), "{name}");
            }
        }
    }

    #[test]
    fn initialize_echoes_the_client_version() {
        assert_eq!(initialize_result(Some("2025-11-25"))["protocolVersion"], "2025-11-25");
        assert_eq!(initialize_result(None)["protocolVersion"], DEFAULT_PROTOCOL_VERSION);
    }
}
