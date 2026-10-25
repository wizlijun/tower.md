//! FR-25 harness adapters. This module is harness-neutral: the runner, the
//! scheduler and the App see only these types. Everything a particular
//! harness needs (program name, flags, output shapes) lives in its own file
//! below, guarded by `tests/harness_boundary.rs` (NFR-07).
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use crate::run_report::Tokens;

mod claude_code;
mod codex;
mod cursor;
mod gemini;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    ReadOnly,
    WorkspaceWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Skills {
    PluginDir,
    SkillsDir,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Channels {
    pub mcp: bool,
    pub permission_route: bool,
    pub live_input: bool,
    pub skills: Skills,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub channels: Channels,
    /// The harness reports a cost; without it a budget cannot be enforced.
    pub reports_cost: bool,
    /// The harness stops itself at a given budget.
    pub enforces_budget: bool,
    /// The harness constrains its final output with a JSON Schema.
    pub structured_report: bool,
}

/// FR-25 `RunSpec`, with the paths the runner prepared.
#[derive(Clone, Debug, PartialEq)]
pub struct RunSpec {
    pub run_id: String,
    pub task_id: String,
    pub worktree: PathBuf,
    /// `<git-common-dir>/towermd/runs/<run>/`: task card, schema, raw output.
    pub run_dir: PathBuf,
    /// The fixed template pointing at `task.md`.
    pub prompt: String,
    pub model: Option<String>,
    pub permission: Permission,
    pub budget_usd: Option<f64>,
    /// The FR-28 report schema, already written to disk.
    pub output_schema: PathBuf,
    pub mcp_config: Option<PathBuf>,
    /// The interaction gateway's permission tool, when it routes approvals.
    pub permission_tool: Option<String>,
    pub skill_dirs: Vec<PathBuf>,
}

/// How to start the harness: arguments after the program, and what to feed
/// on standard input. The runner owns the process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

/// The end of one harness session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Final {
    pub text: Option<String>,
    pub report: Option<Value>,
    pub cost_usd: Option<f64>,
    pub tokens: Option<Tokens>,
    pub error: Option<String>,
    /// The error looks external (network, rate limit, authentication).
    pub external: bool,
    /// The harness stopped itself at the budget it was given.
    pub budget_hit: bool,
}

/// One line of raw output in harness-neutral terms (FR-25 `parse`).
#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    Session(String),
    /// A whole assistant message; the last one is the FR-29 fallback.
    Message(String),
    /// A piece of the current assistant message.
    Delta(String),
    /// The harness retries its own API call (6.3 `run.retry`).
    Retry { attempt: u32, error: Option<String> },
    /// Usage of one turn; `cost_usd` only when the harness says so.
    Usage { tokens: Tokens, cost_usd: Option<f64> },
    Failure { message: String, external: bool },
    Final(Final),
}

pub trait Adapter: Send + Sync {
    /// FR-25 id, also the first half of the agent signature.
    fn id(&self) -> &'static str;
    /// The program looked up on `PATH` unless the caller names another.
    fn program(&self) -> &'static str;
    fn version_args(&self) -> &'static [&'static str] {
        &["--version"]
    }
    fn parse_version(&self, output: &str) -> Option<String> {
        first_version(output)
    }
    fn capabilities(&self) -> Capabilities;
    fn launch(&self, spec: &RunSpec) -> Launch;
    /// FR-25 `resume`: continue a parked session with `message` as the new
    /// prompt. `None` when this harness cannot: the runner starts a new
    /// attempt, whose task card carries the same message.
    fn resume(&self, _spec: &RunSpec, _session: &str, _message: &str) -> Option<Launch> {
        None
    }
    fn parse(&self, line: &str) -> Vec<Signal>;
    /// The MCP client configuration that starts the interaction gateway, in
    /// this harness's own format; `None` without an MCP channel (FR-60).
    fn mcp_config(&self, _gateway: &Gateway) -> Option<Value> {
        None
    }
    /// The name this harness calls the gateway's permission tool by, when it
    /// routes permission prompts to a tool (FR-63).
    fn permission_tool(&self, _server: &str, _tool: &str) -> Option<String> {
        None
    }
    /// The tool asked for and its input, out of a permission prompt's arguments.
    fn permission_request(&self, _arguments: &Value) -> Option<PermissionAsk> {
        None
    }
    /// The permission tool's answer, in this harness's protocol.
    fn permission_reply(&self, _ask: &PermissionAsk, allow: bool, message: &str) -> Value {
        serde_json::json!({"allow": allow, "message": message})
    }
}

/// The interaction gateway as a local stdio MCP server (16.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gateway {
    pub server: String,
    pub command: String,
    pub args: Vec<String>,
}

/// A permission prompt in neutral terms.
#[derive(Clone, Debug, PartialEq)]
pub struct PermissionAsk {
    pub tool: String,
    pub input: Value,
}

/// `<harness>/<version>`, the delegate's actor form.
pub fn signature(adapter: &dyn Adapter, version: &str) -> String {
    format!("{}/{}", adapter.id(), version)
}

pub fn adapters() -> [&'static dyn Adapter; 4] {
    [&claude_code::ClaudeCode, &codex::Codex, &cursor::Cursor, &gemini::Gemini]
}

pub fn find(id: &str) -> Option<&'static dyn Adapter> {
    adapters().into_iter().find(|adapter| adapter.id() == id)
}

/// The first dotted version token, kept to characters an actor allows.
pub fn first_version(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ','))
        .map(|token| token.trim_start_matches(['v', 'V']).trim_end_matches(|c: char| !c.is_ascii_alphanumeric()))
        .find(|token| {
            token.starts_with(|c: char| c.is_ascii_digit()) && token.contains('.')
                && token.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
        })
        .map(str::to_owned)
}

/// Errors outside the run's control: network, rate limits, outages and
/// authentication (14.3). Harness-neutral wording only.
pub fn looks_external(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "rate limit", "rate_limit", "ratelimit", "429", "overloaded", "529", "500 ", "502", "503", "504",
        "timed out", "timeout", "network", "connection", "econnreset", "enotfound", "dns", "stream disconnected",
        "unauthorized", "401", "403", "authentication", "api key", "api_key", "quota", "service unavailable",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

pub(crate) fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

pub(crate) fn count(value: &Value) -> Option<u64> {
    value.as_u64()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
