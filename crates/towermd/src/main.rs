use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::json;
use towermd_core::catalog::Catalog;
use towermd_core::config::{Config, DEFAULT_DOCS_DIR};
use towermd_core::init::{init, InitOptions};
use towermd_core::repo;

mod daemon;
mod project_service;

#[derive(Parser)]
#[command(
    name = "towermd",
    version,
    about = "Tower.md：让 Agent 去做，只在关键处放行",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "读取项目画布（经本机守护进程）")]
    Project {
        #[command(subcommand)]
        command: ProjectArgs,
    },
    #[command(about = "生成与查询可重建的 S1–S4 候选草稿；不批准基线、不运行验证命令")]
    Baseline {
        #[command(subcommand)]
        command: BaselineArgs,
    },
    #[command(about = "查询权威 main 上的项目状态（JSON）")]
    Status,
    #[command(about = "管理本用户守护进程（JSON）")]
    Daemon {
        #[command(subcommand)]
        command: DaemonArgs,
    },
    #[command(about = "初始化：生成项目 ID、.towermd/ 配置与 AGENTS.md、文档骨架（不提交）")]
    Init {
        #[arg(help = "仓库内任意路径，默认当前目录")]
        path: Option<PathBuf>,
        #[arg(long, help = "用户文档目录，相对仓库根；非交互新配置必须显式指定")]
        docs_dir: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProjectArgs {
    View,
}

#[derive(Subcommand)]
enum BaselineArgs {
    Scan {
        #[arg(
            long,
            value_name = "SCAN_ID",
            help = "等待已有扫描，每次最多 90 秒；仍 running 时再次等待"
        )]
        wait: Option<String>,
    },
    Show {
        id: String,
    },
}

#[derive(Subcommand)]
enum DaemonArgs {
    Ensure,
    Status,
    Stop,
    #[command(hide = true)]
    Serve,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "towermd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if !error.use_stderr() => {
            return error.print().map_err(|error| error.to_string());
        }
        Err(error) => return Err(error.to_string()),
    };
    match cli.command {
        Command::Project {
            command: ProjectArgs::View,
        } => print_json(daemon::project_view()?),
        Command::Baseline {
            command: BaselineArgs::Scan { wait },
        } => print_json(daemon::baseline_scan(wait.as_deref())?),
        Command::Baseline {
            command: BaselineArgs::Show { id },
        } => print_json(daemon::baseline_show(&id)?),
        Command::Init { path, docs_dir } => run_init(path, docs_dir),
        Command::Status => run_status(),
        Command::Daemon {
            command: DaemonArgs::Serve,
        } => daemon::serve(),
        Command::Daemon {
            command: DaemonArgs::Ensure,
        } => print_json(daemon::ensure()?),
        Command::Daemon {
            command: DaemonArgs::Status,
        } => print_json(daemon::status()?),
        Command::Daemon {
            command: DaemonArgs::Stop,
        } => print_json(daemon::stop()?),
    }
}

fn run_status() -> Result<(), String> {
    let directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let catalog = Catalog::load(&directory)?;
    print_json(json!({
        "project_id": catalog.config.project_id(),
        "branch": catalog.branch,
        "revision": catalog.revision,
        "docs_dir": catalog.config.docs_dir(),
        "objects": catalog.objects.len(),
        "diagnostics": catalog.diagnostics,
    }))
}

fn print_json(value: serde_json::Value) -> Result<(), String> {
    let mut text = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    text.push(b'\n');
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&text)
        .and_then(|()| stdout.flush())
        .map_err(|error| error.to_string())
}

fn run_init(path: Option<PathBuf>, docs_dir: Option<String>) -> Result<(), String> {
    let directory = match path {
        Some(path) => path,
        None => std::env::current_dir().map_err(|error| format!("无法读取当前目录：{error}"))?,
    };
    let existing = Config::load(&repo::toplevel(&directory)?)?;
    let docs_dir = match docs_dir {
        Some(docs_dir) => Some(docs_dir),
        None if existing.is_some() => None,
        None if io::stdin().is_terminal() => ask_docs_dir()?,
        None => return Err("非交互模式须显式传 --docs-dir，请先与人确认目录".into()),
    };

    let report = init(&directory, &InitOptions { docs_dir })?;
    let origin = if report.reused_config {
        "沿用已有"
    } else {
        "新生成"
    };
    let mut output = format!(
        "项目 ID：{}（{origin}）\n用户文档目录：{}\n",
        report.project_id, report.docs_dir
    );
    if !report.created.is_empty() {
        output.push_str("新建：\n");
        for file in &report.created {
            output.push_str(&format!("  {file}\n"));
        }
    }
    for warning in &report.warnings {
        output.push_str(&format!("警告：{warning}\n"));
    }
    for hint in &report.hints {
        output.push_str(&format!("提示：{hint}\n"));
    }
    output.push_str("以上文件未提交，请检查后自行提交。\n");
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(output.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| error.to_string())
}

fn ask_docs_dir() -> Result<Option<String>, String> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "用户文档目录（相对仓库根）[{DEFAULT_DOCS_DIR}]: ")
        .and_then(|()| stdout.flush())
        .map_err(|error| error.to_string())?;
    read_docs_dir(&mut io::stdin().lock())
}

fn read_docs_dir(reader: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .map_err(|error| error.to_string())?
        == 0
    {
        return Err("未收到用户文档目录输入，已取消初始化".into());
    }
    let line = line.trim();
    Ok((!line.is_empty()).then(|| line.to_string()))
}

#[cfg(test)]
mod tests {
    use super::read_docs_dir;
    use std::io::Cursor;

    #[test]
    fn eof_cancels_instead_of_accepting_default() {
        let result = read_docs_dir(&mut Cursor::new(b""));
        assert!(result.is_err(), "EOF 不应批准默认目录：{result:?}");
        assert!(result.unwrap_err().contains("取消"));
    }

    #[test]
    fn enter_accepts_default() {
        assert_eq!(read_docs_dir(&mut Cursor::new(b"\n")).unwrap(), None);
    }

    #[test]
    fn input_selects_trimmed_docs_dir() {
        assert_eq!(
            read_docs_dir(&mut Cursor::new(b" docs/plan \n")).unwrap(),
            Some("docs/plan".into())
        );
    }
}
