use std::io::{self, BufRead, IsTerminal, Read, Write};
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
mod worker;

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
    #[command(hide = true)]
    VerificationWorker,
    #[command(about = "读取项目画布（经本机守护进程）")]
    Project {
        #[command(subcommand)]
        command: ProjectArgs,
    },
    #[command(about = "生成 S1–S5 候选并真实运行仓库验证命令（继承环境、允许网络）；不批准基线")]
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
    #[command(about = "输出与提交校验相同的 S6 JSON Schema")]
    Schema,
    #[command(about = "从已完成 S5 扫描导出综合模板；内容尚未由 Agent 综合")]
    Template {
        id: String,
    },
    #[command(about = "提交 S6 JSON 为未批准草稿；有 origin 时推送私有草稿引用")]
    Submit {
        json: PathBuf,
        #[arg(long, help = "自报 harness/version 来源，不代表可信身份")]
        producer: String,
    },
    #[command(about = "列出本项目耐久综合草稿")]
    List,
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
    #[command(about = "取消扫描；等待验证进程和工作树清理完毕后才结束")]
    Cancel {
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
        Command::VerificationWorker => worker::serve(),
        Command::Baseline {
            command: BaselineArgs::Schema,
        } => print_json(towermd_core::baseline::schema()),
        Command::Baseline {
            command: BaselineArgs::Template { id },
        } => print_json(daemon::baseline_template(&id)?),
        Command::Baseline {
            command: BaselineArgs::List,
        } => print_json(daemon::baseline_list()?),
        Command::Baseline {
            command:
                BaselineArgs::Submit {
                    json: path,
                    producer,
                },
        } => {
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 2 * 1024 * 1024 {
                return Err("综合输入超过 2 MiB".into());
            }
            let input =
                serde_json::from_slice(&bytes).map_err(|e| format!("综合输入 JSON 无效：{e}"))?;
            print_json(daemon::baseline_submit(input, &producer)?)
        }
        Command::Baseline {
            command: BaselineArgs::Cancel { id },
        } => print_json(daemon::baseline_cancel(&id)?),
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
