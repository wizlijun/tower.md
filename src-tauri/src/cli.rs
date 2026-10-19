use crate::commands::AppError;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn resolve_cli(exe: &Path, development: bool) -> Result<PathBuf, AppError> {
    let cli = if development {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("binaries/towermd-{}", env!("TOWERMD_HOST_TRIPLE")))
    } else {
        exe.parent()
            .ok_or_else(|| AppError::new("tool_missing", "无法确定 App 可执行文件目录"))?
            .join("towermd")
    };
    validate_cli(&cli)?;
    Ok(cli)
}

fn validate_cli(cli: &Path) -> Result<(), AppError> {
    let executable = cli.metadata().is_ok_and(|metadata| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            metadata.is_file()
        }
    });
    if executable {
        Ok(())
    } else {
        Err(AppError::new(
            "tool_missing",
            format!(
                "缺少随包 Tower CLI 或文件不可执行：{}。请重新安装完整 App。",
                cli.display()
            ),
        ))
    }
}

#[derive(Debug)]
pub enum CliAction {
    Ensure,
    Init { docs_dir: String },
    RequestResume { request_id: String },
}

#[derive(Debug)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

// Native-only boundary. Callers supply an app-owned non-Git directory for Ensure,
// or a canonical worktree resolved from a ProjectToken for Init. This is not an invoke.
pub fn run_cli(cli: &Path, cwd: &Path, action: CliAction) -> Result<CliOutput, AppError> {
    validate_cli(cli)?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new(cli);
    match action {
        CliAction::Ensure => {
            command.args(["daemon", "ensure"]);
        }
        CliAction::Init { docs_dir } => {
            command.args(["init", "--docs-dir", &docs_dir]);
        }
        CliAction::RequestResume { request_id } => {
            towermd_core::recovery_contract::recorded::request_event_eid(
                &request_id,
                "hold.opened",
            )
            .map_err(|error| AppError::new("request_invalid", error))?;
            command.args(["resume", "--all", "--request-id", &request_id]);
        }
    }
    let output = command
        .current_dir(cwd)
        .env("PATH", &path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            AppError::new(
                "cli_spawn_failed",
                format!("无法执行随包 Tower CLI：{error}"),
            )
        })?;
    let result = CliOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code(),
    };
    if !output.status.success() {
        let mut error = AppError::new(
            "cli_failed",
            "Tower CLI 执行失败，请检查工具诊断与 App 查找环境",
        );
        error.detail = Some(format!(
            "exit_code={:?}\nstdout:\n{}\nstderr:\n{}\nPATH={}\n若诊断提示缺少验证工具，请安装该工具，并从包含它的 PATH 环境重新启动 App。App 不修改 shell rc 或 harness 配置；已运行的 daemon 不会随 App 重启而更新 PATH。",
            result.exit_code, result.stdout, result.stderr, path.to_string_lossy()
        ));
        return Err(error);
    }
    Ok(result)
}

// A CLI result may follow complete, compact Delivery JSONL records. Never scan
// for the last JSON value or accept arbitrary diagnostics before the result.
pub(crate) fn parse_result_stdout(stdout: &str) -> Result<serde_json::Value, AppError> {
    let mut remaining = stdout;
    while let Some((line, rest)) = remaining.split_once('\n') {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            break;
        };
        let Some(fields) = value.as_object() else {
            break;
        };
        if fields.len() != 1 || !fields.contains_key("towermd_message") {
            break;
        }
        serde_json::from_value::<towermd_core::human_intervention::Message>(
            fields["towermd_message"].clone(),
        )
        .map_err(|error| {
            AppError::new(
                "protocol_error",
                format!("CLI Delivery 原件不完整：{error}"),
            )
        })?;
        remaining = rest;
    }
    serde_json::from_str(remaining).map_err(|error| {
        AppError::new(
            "protocol_error",
            format!("CLI 缺少唯一完整操作回执：{error}"),
        )
    })
}

// No WebView command is registered here; project/app-data ownership is established
// by the native callers before they use this adapter.
pub async fn execute_cli(cwd: PathBuf, action: CliAction) -> Result<CliOutput, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe = std::env::current_exe()
            .map_err(|error| AppError::new("tool_missing", format!("无法定位 App：{error}")))?;
        let cli = resolve_cli(&exe, cfg!(debug_assertions))?;
        run_cli(&cli, &cwd, action)
    })
    .await
    .map_err(|error| AppError::new("cli_task_failed", format!("CLI 执行任务失败：{error}")))?
}
