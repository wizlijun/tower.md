//! Claude Code: `claude --bare -p` with `--output-format stream-json
//! --verbose` (FR-25, FR-26). Shapes checked against Claude Code 2.1.308:
//! `system/init`, `system/api_retry`, `assistant`, and a last `result` line
//! carrying `total_cost_usd`, `usage` and, with `--json-schema`,
//! `structured_output`.
use serde_json::{json, Value};

use super::{count, looks_external, text, Adapter, Capabilities, Channels, Final, Gateway, Launch, Permission, PermissionAsk, RunSpec, Signal, Skills};
use crate::run_report::Tokens;

pub struct ClaudeCode;

impl Adapter for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude-code"
    }
    fn program(&self) -> &'static str {
        "claude"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            channels: Channels { mcp: true, permission_route: true, live_input: true, skills: Skills::PluginDir },
            reports_cost: true,
            enforces_budget: true,
            structured_report: true,
        }
    }
    fn launch(&self, spec: &RunSpec) -> Launch {
        let mut args: Vec<String> = ["--bare", "-p", "--output-format", "stream-json", "--verbose"]
            .map(String::from).to_vec();
        if let Some(model) = &spec.model {
            args.extend(["--model".into(), model.clone()]);
        }
        if let Some(budget) = spec.budget_usd {
            args.extend(["--max-budget-usd".into(), format!("{budget}")]);
        }
        args.extend(["--json-schema".into(), crate::run_report::schema().to_string()]);
        let mode = match spec.permission {
            Permission::ReadOnly => "plan",
            Permission::WorkspaceWrite => "acceptEdits",
        };
        args.extend(["--permission-mode".into(), mode.into()]);
        if let Some(config) = &spec.mcp_config {
            args.extend(["--mcp-config".into(), config.display().to_string(), "--strict-mcp-config".into()]);
        }
        match &spec.permission_tool {
            Some(tool) => args.extend(["--permission-prompt-tool".into(), tool.clone()]),
            None => args.extend(["--permission-prompts".into(), "none".into()]),
        }
        for dir in &spec.skill_dirs {
            args.extend(["--plugin-dir".into(), dir.display().to_string()]);
        }
        args.push(spec.prompt.clone());
        Launch { args, stdin: None }
    }
    /// The same flags, `--resume <session_id>`, and the message as the prompt.
    fn resume(&self, spec: &RunSpec, session: &str, message: &str) -> Option<Launch> {
        let mut launch = self.launch(spec);
        launch.args.pop();
        launch.args.extend(["--resume".into(), session.into(), message.into()]);
        Some(launch)
    }
    fn mcp_config(&self, gateway: &Gateway) -> Option<Value> {
        Some(json!({"mcpServers": {gateway.server.as_str(): {"type": "stdio", "command": gateway.command, "args": gateway.args}}}))
    }
    fn permission_tool(&self, server: &str, tool: &str) -> Option<String> {
        Some(format!("mcp__{server}__{tool}"))
    }
    /// `--permission-prompt-tool` calls the tool with `tool_name` and `input`.
    fn permission_request(&self, arguments: &Value) -> Option<PermissionAsk> {
        let tool = arguments["tool_name"].as_str().filter(|tool| !tool.trim().is_empty())?;
        Some(PermissionAsk { tool: tool.to_owned(), input: arguments.get("input").cloned().unwrap_or(Value::Null) })
    }
    /// It expects `{"behavior":"allow","updatedInput":…}` or `{"behavior":"deny","message":…}` as text.
    fn permission_reply(&self, ask: &PermissionAsk, allow: bool, message: &str) -> Value {
        if allow {
            json!({"behavior": "allow", "updatedInput": ask.input})
        } else {
            json!({"behavior": "deny", "message": message})
        }
    }
    fn parse(&self, line: &str) -> Vec<Signal> {
        let Ok(value) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        match (value["type"].as_str(), value["subtype"].as_str()) {
            (Some("system"), Some("init")) => text(&value["session_id"]).map(Signal::Session).into_iter().collect(),
            (Some("system"), Some("api_retry")) => {
                let error = text(&value["error"]).or_else(|| value["error_status"].as_u64().map(|s| s.to_string()));
                vec![Signal::Retry { attempt: value["attempt"].as_u64().unwrap_or(0) as u32, error }]
            }
            (Some("assistant"), _) => {
                let parts: Vec<&str> = value["message"]["content"].as_array().into_iter().flatten()
                    .filter(|part| part["type"] == "text").filter_map(|part| part["text"].as_str()).collect();
                if parts.is_empty() { Vec::new() } else { vec![Signal::Message(parts.join("\n"))] }
            }
            (Some("result"), subtype) => vec![Signal::Final(result(&value, subtype.unwrap_or_default()))],
            _ => Vec::new(),
        }
    }
}

fn result(value: &Value, subtype: &str) -> Final {
    let is_error = value["is_error"].as_bool().unwrap_or(subtype != "success");
    let usage = &value["usage"];
    let tokens = usage.is_object().then(|| Tokens {
        input: count(&usage["input_tokens"]),
        output: count(&usage["output_tokens"]),
        cache_read: count(&usage["cache_read_input_tokens"]),
        cache_write: count(&usage["cache_creation_input_tokens"]),
    });
    let errors: Vec<String> = value["errors"].as_array().into_iter().flatten()
        .filter_map(|error| error.as_str().map(str::to_owned).or_else(|| text(&error["message"]))).collect();
    let error = is_error.then(|| if errors.is_empty() {
        text(&value["result"]).filter(|text| !text.is_empty()).unwrap_or_else(|| subtype.to_owned())
    } else {
        errors.join("; ")
    });
    Final {
        text: text(&value["result"]),
        report: value.get("structured_output").filter(|report| report.is_object()).cloned(),
        cost_usd: value["total_cost_usd"].as_f64().filter(|cost| cost.is_finite() && *cost >= 0.0),
        tokens,
        external: error.as_deref().is_some_and(looks_external),
        error,
        budget_hit: subtype == "error_max_budget_usd",
    }
}
