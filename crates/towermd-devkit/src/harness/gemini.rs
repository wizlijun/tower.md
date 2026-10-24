//! Gemini CLI: `gemini -p <prompt> --output-format stream-json` (FR-25
//! table): `init`, streamed assistant `message` deltas, `error`, and a last
//! `result` with `status` and token `stats`, no cost. Not installed on the
//! development machine; only the mock harness exercises this adapter.
use serde_json::Value;

use super::{count, looks_external, text, Adapter, Capabilities, Channels, Final, Launch, Permission, RunSpec, Signal, Skills};
use crate::run_report::Tokens;

pub struct Gemini;

impl Adapter for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }
    fn program(&self) -> &'static str {
        "gemini"
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
        let approval = match spec.permission {
            Permission::ReadOnly => "default",
            Permission::WorkspaceWrite => "auto_edit",
        };
        let mut args: Vec<String> = vec!["--output-format".into(), "stream-json".into(),
            "--approval-mode".into(), approval.into()];
        if let Some(model) = &spec.model {
            args.extend(["--model".into(), model.clone()]);
        }
        args.extend(["-p".into(), spec.prompt.clone()]);
        Launch { args, stdin: None }
    }
    fn parse(&self, line: &str) -> Vec<Signal> {
        let Ok(value) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        match value["type"].as_str().unwrap_or_default() {
            "init" => text(&value["session_id"]).map(Signal::Session).into_iter().collect(),
            "message" if value["role"] == "assistant" => {
                let Some(content) = text(&value["content"]) else { return Vec::new() };
                vec![if value["delta"] == true { Signal::Delta(content) } else { Signal::Message(content) }]
            }
            "error" => {
                let message = text(&value["message"]).unwrap_or_else(|| "error".into());
                if value["severity"] == "warning" {
                    return Vec::new();
                }
                vec![Signal::Failure { external: looks_external(&message), message }]
            }
            "result" => {
                let stats = &value["stats"];
                let error = (value["status"] != "success").then(|| {
                    text(&value["error"]["message"]).unwrap_or_else(|| "error".into())
                });
                vec![Signal::Final(Final {
                    tokens: stats.is_object().then(|| Tokens {
                        input: count(&stats["input_tokens"]), output: count(&stats["output_tokens"]),
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
