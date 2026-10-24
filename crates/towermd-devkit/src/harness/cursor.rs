//! Cursor CLI: `agent -p --output-format stream-json`, `--force` to let it
//! change files (FR-25 table). Its stream follows the `system`/`assistant`/
//! `result` shape without a cost. Not checked against an installed CLI: the
//! `agent` on the development machine is another program, so only the mock
//! harness exercises this adapter.
use serde_json::Value;

use super::{count, looks_external, text, Adapter, Capabilities, Channels, Final, Launch, Permission, RunSpec, Signal, Skills};
use crate::run_report::Tokens;

pub struct Cursor;

impl Adapter for Cursor {
    fn id(&self) -> &'static str {
        "cursor"
    }
    fn program(&self) -> &'static str {
        "agent"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            channels: Channels { mcp: false, permission_route: false, live_input: false, skills: Skills::None },
            reports_cost: false,
            enforces_budget: false,
            structured_report: false,
        }
    }
    fn launch(&self, spec: &RunSpec) -> Launch {
        let mut args: Vec<String> = ["-p", "--output-format", "stream-json"].map(String::from).to_vec();
        if spec.permission == Permission::WorkspaceWrite {
            args.push("--force".into());
        }
        if let Some(model) = &spec.model {
            args.extend(["--model".into(), model.clone()]);
        }
        args.push(spec.prompt.clone());
        Launch { args, stdin: None }
    }
    fn parse(&self, line: &str) -> Vec<Signal> {
        let Ok(value) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        match (value["type"].as_str(), value["subtype"].as_str()) {
            (Some("system"), Some("init")) => text(&value["session_id"]).map(Signal::Session).into_iter().collect(),
            (Some("assistant"), _) => {
                let parts: Vec<&str> = value["message"]["content"].as_array().into_iter().flatten()
                    .filter(|part| part["type"] == "text").filter_map(|part| part["text"].as_str()).collect();
                if parts.is_empty() { Vec::new() } else { vec![Signal::Message(parts.concat())] }
            }
            (Some("result"), subtype) => {
                let is_error = value["is_error"].as_bool().unwrap_or(subtype != Some("success"));
                let error = is_error.then(|| text(&value["result"]).unwrap_or_else(|| "error".into()));
                let usage = &value["usage"];
                vec![Signal::Final(Final {
                    text: text(&value["result"]),
                    tokens: usage.is_object().then(|| Tokens {
                        input: count(&usage["input_tokens"]), output: count(&usage["output_tokens"]),
                        ..Tokens::default()
                    }),
                    external: error.as_deref().is_some_and(looks_external),
                    error,
                    ..Final::default()
                })]
            }
            _ => Vec::new(),
        }
    }
}
