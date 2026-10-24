//! Codex: `codex exec --json` writes JSON Lines (`thread.started`,
//! `turn.completed` with `usage`, `turn.failed`, `item.*`, `error`). Shapes
//! checked against codex-cli 0.163.0. Codex reports tokens, not cost, so the
//! cost stays unknown. The final message is the report, constrained by
//! `--output-schema` and also written with `-o`.
use serde_json::Value;

use super::{count, looks_external, text, Adapter, Capabilities, Channels, Launch, Permission, RunSpec, Signal, Skills};
use crate::run_report::Tokens;

pub struct Codex;

/// File the last message is written to, inside the run directory.
pub const LAST_MESSAGE: &str = "last-message.txt";

impl Adapter for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }
    fn program(&self) -> &'static str {
        "codex"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            channels: Channels { mcp: false, permission_route: false, live_input: false, skills: Skills::None },
            reports_cost: false,
            enforces_budget: false,
            structured_report: true,
        }
    }
    fn launch(&self, spec: &RunSpec) -> Launch {
        let sandbox = match spec.permission {
            Permission::ReadOnly => "read-only",
            Permission::WorkspaceWrite => "workspace-write",
        };
        let mut args: Vec<String> = vec![
            "exec".into(), "--json".into(), "--sandbox".into(), sandbox.into(), "--skip-git-repo-check".into(),
            "--output-schema".into(), spec.output_schema.display().to_string(),
            "-o".into(), spec.run_dir.join(LAST_MESSAGE).display().to_string(),
        ];
        if let Some(model) = &spec.model {
            args.extend(["-m".into(), model.clone()]);
        }
        // The prompt comes on standard input so it is never read as a flag.
        args.push("-".into());
        Launch { args, stdin: Some(spec.prompt.clone()) }
    }
    fn parse(&self, line: &str) -> Vec<Signal> {
        let Ok(value) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        match value["type"].as_str().unwrap_or_default() {
            "thread.started" => text(&value["thread_id"]).map(Signal::Session).into_iter().collect(),
            "item.completed" if value["item"]["type"] == "agent_message" => {
                text(&value["item"]["text"]).map(Signal::Message).into_iter().collect()
            }
            "turn.completed" => {
                let usage = &value["usage"];
                vec![Signal::Usage { cost_usd: None, tokens: Tokens {
                    input: count(&usage["input_tokens"]),
                    output: count(&usage["output_tokens"]),
                    cache_read: count(&usage["cached_input_tokens"]),
                    cache_write: None,
                } }]
            }
            "turn.failed" => {
                let message = text(&value["error"]["message"]).unwrap_or_else(|| "turn failed".into());
                vec![Signal::Failure { external: looks_external(&message), message }]
            }
            "error" => {
                let message = text(&value["message"]).unwrap_or_default();
                // Codex reports its own reconnects as errors while it retries.
                match reconnect_attempt(&message) {
                    Some(attempt) => vec![Signal::Retry { attempt, error: Some(message) }],
                    None => vec![Signal::Failure { external: looks_external(&message), message }],
                }
            }
            _ => Vec::new(),
        }
    }
}

/// `Reconnecting... 2/5` gives attempt 2.
fn reconnect_attempt(message: &str) -> Option<u32> {
    let rest = message.strip_prefix("Reconnecting")?;
    let fraction = rest.split_whitespace().find(|token| token.contains('/'))?;
    fraction.split('/').next()?.parse().ok()
}
