use std::path::PathBuf;

use super::*;

fn spec(permission: Permission) -> RunSpec {
    RunSpec {
        run_id: "R-1".into(), task_id: "T-1".into(), worktree: "/w".into(), run_dir: "/g/towermd/runs/R-1".into(),
        prompt: "读取任务卡 /g/towermd/runs/R-1/task.md".into(), model: Some("m-1".into()), permission,
        budget_usd: Some(1.5), output_schema: "/g/towermd/runs/R-1/report.schema.json".into(), mcp_config: None,
        permission_tool: None, skill_dirs: vec![PathBuf::from("/skills/a")],
    }
}

fn adapter(id: &str) -> &'static dyn Adapter {
    find(id).unwrap()
}

fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2).any(|pair| pair[0] == flag && pair[1] == value)
}

#[test]
fn four_adapters_are_registered_with_their_signatures() {
    let ids: Vec<_> = adapters().iter().map(|adapter| adapter.id()).collect();
    assert_eq!(ids, ["claude-code", "codex", "cursor", "gemini"]);
    assert_eq!(signature(adapter("codex"), "0.163.0"), "codex/0.163.0");
    assert!(find("other").is_none());
}

#[test]
fn versions_are_read_from_each_version_line() {
    assert_eq!(adapter("claude-code").parse_version("2.1.308 (Claude Code)\n").as_deref(), Some("2.1.308"));
    assert_eq!(adapter("codex").parse_version("codex-cli 0.163.0\n").as_deref(), Some("0.163.0"));
    assert_eq!(adapter("gemini").parse_version("v0.9.0").as_deref(), Some("0.9.0"));
    assert_eq!(adapter("cursor").parse_version("2025.10.02-abc1234").as_deref(), Some("2025.10.02-abc1234"));
    assert_eq!(first_version("no version here"), None);
}

#[test]
fn claude_code_runs_bare_with_the_schema_budget_and_explicit_skills() {
    let launch = adapter("claude-code").launch(&spec(Permission::WorkspaceWrite));
    let args = &launch.args;
    assert_eq!(&args[..5], ["--bare", "-p", "--output-format", "stream-json", "--verbose"]);
    assert!(has_pair(args, "--model", "m-1") && has_pair(args, "--max-budget-usd", "1.5"));
    assert!(has_pair(args, "--permission-mode", "acceptEdits") && has_pair(args, "--permission-prompts", "none"));
    assert!(has_pair(args, "--plugin-dir", "/skills/a"));
    let schema = &args[args.iter().position(|arg| arg == "--json-schema").unwrap() + 1];
    assert_eq!(serde_json::from_str::<Value>(schema).unwrap(), crate::run_report::schema());
    assert_eq!(args.last().unwrap(), "读取任务卡 /g/towermd/runs/R-1/task.md");
    assert_eq!(launch.stdin, None);

    let mut gated = spec(Permission::ReadOnly);
    gated.mcp_config = Some("/g/mcp.json".into());
    gated.permission_tool = Some("mcp__gateway__permission".into());
    let args = adapter("claude-code").launch(&gated).args;
    assert!(has_pair(&args, "--permission-mode", "plan") && has_pair(&args, "--mcp-config", "/g/mcp.json"));
    assert!(args.contains(&"--strict-mcp-config".into()) && !args.contains(&"--permission-prompts".into()));
    assert!(has_pair(&args, "--permission-prompt-tool", "mcp__gateway__permission"));
}

/// FR-25 resume: only Claude Code continues a parked session; the others
/// start a new attempt.
#[test]
fn claude_code_resumes_a_parked_session_with_the_message_as_the_prompt() {
    let spec = spec(Permission::WorkspaceWrite);
    let fresh = adapter("claude-code").launch(&spec).args;
    let resumed = adapter("claude-code").resume(&spec, "s-1", "回答：JSON").unwrap().args;
    assert_eq!(&resumed[..fresh.len() - 1], &fresh[..fresh.len() - 1], "the same flags");
    assert_eq!(&resumed[fresh.len() - 1..], ["--resume", "s-1", "回答：JSON"]);
    for id in ["codex", "cursor", "gemini"] {
        assert!(adapter(id).resume(&spec, "s-1", "m").is_none(), "{id}");
    }
}

#[test]
fn claude_code_stream_yields_session_retry_message_and_final_cost() {
    let claude = adapter("claude-code");
    assert_eq!(claude.parse(r#"{"type":"system","subtype":"init","session_id":"s-1"}"#), [Signal::Session("s-1".into())]);
    assert_eq!(claude.parse(r#"{"type":"system","subtype":"api_retry","attempt":2,"max_retries":10,"retry_delay_ms":500,"error_status":429,"error":"rate_limit"}"#),
        [Signal::Retry { attempt: 2, error: Some("rate_limit".into()) }]);
    assert_eq!(claude.parse(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a"},{"type":"tool_use"},{"type":"text","text":"b"}]}}"#),
        [Signal::Message("a\nb".into())]);
    let [Signal::Final(last)] = &claude.parse(r#"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.5,"usage":{"input_tokens":3,"output_tokens":4,"cache_creation_input_tokens":1,"cache_read_input_tokens":2},"result":"ok","structured_output":{"status":"done"}}"#)[..] else { panic!() };
    assert_eq!((last.cost_usd, last.text.as_deref(), last.error.as_deref(), last.budget_hit), (Some(0.5), Some("ok"), None, false));
    assert_eq!(last.tokens, Some(Tokens { input: Some(3), output: Some(4), cache_read: Some(2), cache_write: Some(1) }));
    assert_eq!(last.report, Some(serde_json::json!({"status":"done"})));
    let [Signal::Final(last)] = &claude.parse(r#"{"type":"result","subtype":"error_max_budget_usd","is_error":true,"total_cost_usd":1.2,"errors":["Reached maximum budget"]}"#)[..] else { panic!() };
    assert!(last.budget_hit && !last.external && last.error.as_deref() == Some("Reached maximum budget"));
    let [Signal::Final(last)] = &claude.parse(r#"{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["API Error: 529 overloaded"]}"#)[..] else { panic!() };
    assert!(last.external && last.cost_usd.is_none());
    assert!(claude.parse("not json").is_empty() && claude.parse(r#"{"type":"user"}"#).is_empty());
}

#[test]
fn claude_code_names_the_tool_requests_it_refused_by_itself() {
    let claude = adapter("claude-code");
    let [Signal::Denied(denied), Signal::Final(_)] = &claude.parse(r#"{"type":"result","subtype":"success","is_error":false,"result":"","permission_denials":[{"tool_name":"Bash","tool_use_id":"t1","tool_input":{"command":"rm -rf build"}},{"tool_name":"","tool_input":{}}]}"#)[..] else { panic!() };
    assert_eq!(denied, &[PermissionAsk { tool: "Bash".into(), input: serde_json::json!({"command": "rm -rf build"}) }]);
    let [Signal::Final(_)] = &claude.parse(r#"{"type":"result","subtype":"success","is_error":false,"result":"","permission_denials":[]}"#)[..] else { panic!() };
}

#[test]
fn codex_runs_exec_json_with_the_schema_file_and_no_cost() {
    let launch = adapter("codex").launch(&spec(Permission::WorkspaceWrite));
    let args = &launch.args;
    assert_eq!(&args[..2], ["exec", "--json"]);
    assert!(has_pair(args, "--sandbox", "workspace-write") && args.contains(&"--skip-git-repo-check".into()));
    assert!(has_pair(args, "--output-schema", "/g/towermd/runs/R-1/report.schema.json"));
    assert!(has_pair(args, "-o", "/g/towermd/runs/R-1/last-message.txt") && has_pair(args, "-m", "m-1"));
    assert_eq!((args.last().unwrap().as_str(), launch.stdin.as_deref()), ("-", Some("读取任务卡 /g/towermd/runs/R-1/task.md")));
    assert!(has_pair(&adapter("codex").launch(&spec(Permission::ReadOnly)).args, "--sandbox", "read-only"));
    assert!(!adapter("codex").capabilities().reports_cost);

    let codex = adapter("codex");
    assert_eq!(codex.parse(r#"{"type":"thread.started","thread_id":"th-1"}"#), [Signal::Session("th-1".into())]);
    assert_eq!(codex.parse(r#"{"type":"item.completed","item":{"id":"i","type":"agent_message","text":"hi"}}"#), [Signal::Message("hi".into())]);
    assert!(codex.parse(r#"{"type":"item.completed","item":{"id":"i","type":"command_execution"}}"#).is_empty());
    assert_eq!(codex.parse(r#"{"type":"turn.completed","usage":{"input_tokens":9,"cached_input_tokens":4,"output_tokens":2}}"#),
        [Signal::Usage { cost_usd: None, tokens: Tokens { input: Some(9), output: Some(2), cache_read: Some(4), cache_write: None } }]);
    assert_eq!(codex.parse(r#"{"type":"turn.failed","error":{"message":"stream disconnected before completion"}}"#),
        [Signal::Failure { message: "stream disconnected before completion".into(), external: true }]);
    assert_eq!(codex.parse(r#"{"type":"error","message":"Reconnecting... 2/5"}"#),
        [Signal::Retry { attempt: 2, error: Some("Reconnecting... 2/5".into()) }]);
}

#[test]
fn cursor_and_gemini_follow_their_documented_streams() {
    let args = adapter("cursor").launch(&spec(Permission::WorkspaceWrite)).args;
    assert_eq!(&args[..3], ["-p", "--output-format", "stream-json"]);
    assert!(args.contains(&"--force".into()) && has_pair(&args, "--model", "m-1"));
    assert!(!adapter("cursor").launch(&spec(Permission::ReadOnly)).args.contains(&"--force".into()));
    let cursor = adapter("cursor");
    assert_eq!(cursor.parse(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"x"}]}}"#), [Signal::Message("x".into())]);
    let [Signal::Final(last)] = &cursor.parse(r#"{"type":"result","subtype":"success","is_error":false,"result":"fin"}"#)[..] else { panic!() };
    assert_eq!((last.text.as_deref(), last.cost_usd), (Some("fin"), None));

    let args = adapter("gemini").launch(&spec(Permission::WorkspaceWrite)).args;
    assert!(has_pair(&args, "--output-format", "stream-json") && has_pair(&args, "--approval-mode", "auto_edit"));
    assert!(has_pair(&args, "-p", "读取任务卡 /g/towermd/runs/R-1/task.md"));
    let gemini = adapter("gemini");
    assert_eq!(gemini.parse(r#"{"type":"init","session_id":"g-1","model":"m"}"#), [Signal::Session("g-1".into())]);
    assert_eq!(gemini.parse(r#"{"type":"message","role":"assistant","content":"pa","delta":true}"#), [Signal::Delta("pa".into())]);
    assert!(gemini.parse(r#"{"type":"message","role":"user","content":"q"}"#).is_empty());
    let [Signal::Final(last)] = &gemini.parse(r#"{"type":"result","status":"error","error":{"message":"quota exceeded"},"stats":{"input_tokens":1,"output_tokens":2}}"#)[..] else { panic!() };
    assert!(last.external && last.cost_usd.is_none() && last.tokens.as_ref().unwrap().output == Some(2));
    for id in ["cursor", "gemini"] {
        let capabilities = adapter(id).capabilities();
        assert!(!capabilities.reports_cost && !capabilities.structured_report, "{id}");
    }
}

#[test]
fn external_errors_are_named_without_harness_words() {
    for text in ["API Error: 429", "Connection reset", "authentication failed", "Request timed out"] {
        assert!(looks_external(text), "{text}");
    }
    assert!(!looks_external("assertion failed: tests do not pass"));
}
