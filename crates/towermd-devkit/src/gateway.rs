//! Wiring a headless run to the interaction gateway (spec 16.4, FR-60,
//! FR-63). The gateway is `towermd mcp`, a local stdio server the harness
//! starts for this run only; its configuration is written into the run's own
//! directory and passed explicitly, never into the user's harness settings.
use std::path::Path;

use crate::harness::{Adapter, Gateway};
use crate::runner::RunConfig;

/// The server name the harness knows the gateway by.
pub const SERVER: &str = "towermd";
pub const ASK: &str = "towermd_ask";
pub const REVIEW: &str = "towermd_request_review";
pub const PERMISSION: &str = "towermd_permission";
pub const NOTE: &str = "towermd_note";
/// `<run_dir>/mcp.json`.
pub const CONFIG: &str = "mcp.json";

/// The gateway command for one run: the hidden `towermd mcp`.
pub fn gateway(towermd: &Path, config: &RunConfig, adapter: &dyn Adapter) -> Gateway {
    Gateway {
        server: SERVER.into(),
        command: towermd.display().to_string(),
        args: vec!["mcp".into(), "--run".into(), config.run.clone(), "--worktree".into(), config.worktree.display().to_string(),
            "--harness".into(), adapter.id().into()],
    }
}

/// Gives the run the gateway as far as its harness can take it. A harness
/// without an MCP channel is left to park its questions (FR-60); one that
/// cannot route permission prompts to a tool keeps none.
pub fn wire(config: &mut RunConfig, adapter: &dyn Adapter, towermd: &Path) -> Result<(), String> {
    let channels = adapter.capabilities().channels;
    if !channels.mcp {
        return Ok(());
    }
    let Some(file) = adapter.mcp_config(&gateway(towermd, config, adapter)) else { return Ok(()) };
    std::fs::create_dir_all(&config.run_dir).map_err(|e| e.to_string())?;
    let path = config.run_dir.join(CONFIG);
    std::fs::write(&path, serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    config.mcp_config = Some(path);
    if channels.permission_route {
        config.permission_tool = adapter.permission_tool(SERVER, PERMISSION);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{self, Permission};
    use std::time::Duration;

    fn config(dir: &Path) -> RunConfig {
        RunConfig { host: "h".into(), run: "R-1".into(), task: "T-1".into(), module: "A".into(), worktree: dir.join("wt"),
            run_dir: dir.join("runs/R-1"), task_card: "# T-1".into(), model: None, permission: Permission::WorkspaceWrite,
            budget_usd: None, timeout: Duration::from_secs(60), grace: Duration::from_secs(5), max_attempts: 1,
            mcp_config: None, permission_tool: None, skill_dirs: vec![], env: vec![] }
    }

    #[test]
    fn only_a_harness_with_an_mcp_channel_gets_the_gateway_in_its_run_directory() {
        let dir = tempfile::tempdir().unwrap();
        for adapter in harness::adapters() {
            let mut run = config(dir.path());
            wire(&mut run, adapter, Path::new("/opt/towermd")).unwrap();
            let channels = adapter.capabilities().channels;
            assert_eq!(run.mcp_config.is_some(), channels.mcp, "{}", adapter.id());
            assert_eq!(run.permission_tool.is_some(), channels.mcp && channels.permission_route, "{}", adapter.id());
            if let Some(path) = &run.mcp_config {
                assert!(path.starts_with(dir.path().join("runs/R-1")), "运行目录之外不写");
                let text = std::fs::read_to_string(path).unwrap();
                for part in ["/opt/towermd", "\"mcp\"", "R-1", adapter.id()] {
                    assert!(text.contains(part), "{part}: {text}");
                }
            }
        }
    }

    #[test]
    fn a_permission_prompt_reads_and_answers_in_the_harness_protocol() {
        let adapter = harness::find("claude-code").unwrap();
        let ask = adapter.permission_request(&serde_json::json!({"tool_name":"Bash","input":{"command":"ls"}})).unwrap();
        assert_eq!(ask.tool, "Bash");
        assert_eq!(adapter.permission_reply(&ask, true, "")["updatedInput"]["command"], "ls");
        assert_eq!(adapter.permission_reply(&ask, false, "超时")["message"], "超时");
        assert!(adapter.permission_request(&serde_json::json!({"input":{}})).is_none());
        assert_eq!(harness::find("codex").unwrap().permission_tool(SERVER, PERMISSION), None);
    }
}
