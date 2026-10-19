use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use towermd_core::coldstart::ScanOptions;
use towermd_core::config::{Config, DEFAULT_DOCS_DIR};
use towermd_core::init::{init, InitOptions};
use towermd_core::repo;
use towermd_core::tasks::{TaskCommand, TaskSpec};

mod ask_cli;
mod daemon;
mod finish_jobs;
mod main_refresh;
mod review_cli;
mod scan_jobs;
mod snapshot_schedule;

#[derive(Parser)]
#[command(
    name = "towermd",
    version,
    about = "Tower.md：让 Agent 去做，只在关键处放行",
    disable_help_subcommand = true
)]
struct Cli {
    #[arg(long, global = true, help = "显式选择运行，覆盖工作树与分支自动判定")]
    run: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "查询当前项目与任务状态（JSON）")]
    Status,
    #[command(about = "检查当前运行的已提交变更与路径门禁（JSON，不执行验证）")]
    Check,
    #[command(about = "输出当前任务的已核合并 trailers（不提交、不合并）")]
    Trailers { task: String },
    #[command(about = "提问、分段等待或转述回答（不批准）")]
    Ask(ask_cli::AskArgs),
    #[command(about = "后台验证当前任务；用 Evidence ID 有界等待原结果（JSON）")]
    Finish {
        #[arg(
            long,
            conflicts_with = "report",
            help = "等待同一 Evidence ID，最多90秒，不重复验证"
        )]
        wait: Option<String>,
        #[arg(
            long,
            conflicts_with = "wait",
            help = "只读 UTF-8 报告（最多1 MiB），仅作线索，不作为完成证据"
        )]
        report: Option<PathBuf>,
    },
    #[command(about = "请求任务计划或提交树的人工评审")]
    Review {
        #[command(subcommand)]
        command: review_cli::ReviewArgs,
    },
    #[command(about = "请求守护进程写入项目快照（JSON，无远端且 main 已检出时写私有引用）")]
    Snapshot,
    #[command(about = "请求本地 hub 恢复（不执行恢复）")]
    Hub {
        #[command(subcommand)]
        command: HubArgs,
    },
    #[command(about = "扫描并验证候选基线（不批准或发布）")]
    Baseline {
        #[command(subcommand)]
        command: BaselineArgs,
    },
    #[command(about = "新增、领取、查看或释放任务（JSON）")]
    Task {
        #[command(subcommand)]
        command: TaskArgs,
    },
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
enum DaemonArgs {
    Ensure,
    Status,
    Stop,
    #[command(hide = true)]
    Serve,
}

#[derive(Subcommand)]
enum HubArgs {
    #[command(about = "仅生成恢复请求（JSON），等待 App 人工确认")]
    Rebuild,
}

#[derive(Subcommand)]
enum BaselineArgs {
    #[command(about = "输出 S6 提交的唯一 JSON Schema（离线，不启动守护进程）")]
    Schema,
    #[command(about = "提交已有会话综合的 S6 JSON，保存私有草稿（不批准）")]
    Submit {
        #[arg(help = "符合 baseline schema 的本地 JSON 文件")]
        json: PathBuf,
        #[arg(long, help = "重试原 UUIDv7 请求；必须保留完全相同的输入与运行")]
        request_id: Option<String>,
    },
    #[command(about = "后台扫描权威 main 并执行验证；用 --wait 等待同一任务（JSON）")]
    Scan {
        #[arg(long, conflicts_with = "wait", value_parser = clap::value_parser!(u16).range(1..=200))]
        history_limit: Option<u16>,
        #[arg(long, help = "等待已有 scan ID，最多等待90秒；不会重新执行验证")]
        wait: Option<String>,
    },
}

#[derive(Subcommand)]
enum TaskArgs {
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        module: String,
        #[arg(long)]
        touches: Vec<String>,
        #[arg(long = "allow", required = true)]
        allowed_paths: Vec<String>,
        #[arg(long)]
        depends_on: Vec<String>,
        #[arg(long, required = true)]
        acceptance: Vec<String>,
        #[arg(long = "decision")]
        decisions: Vec<String>,
    },
    Claim {
        id: String,
        #[arg(
            long,
            requires = "harness_version",
            help = "技能自报的 harness 名称，不作为可信署名"
        )]
        harness: Option<String>,
        #[arg(long, requires = "harness")]
        harness_version: Option<String>,
    },
    Show {
        id: Option<String>,
    },
    Release,
}

impl From<TaskArgs> for TaskCommand {
    fn from(command: TaskArgs) -> Self {
        match command {
            TaskArgs::Add {
                id,
                title,
                module,
                touches,
                allowed_paths,
                depends_on,
                acceptance,
                decisions,
            } => Self::Add {
                task: TaskSpec {
                    id,
                    title,
                    module,
                    touches,
                    allowed_paths,
                    depends_on,
                    acceptance,
                    decisions,
                },
            },
            TaskArgs::Claim {
                id,
                harness,
                harness_version,
            } => Self::Claim {
                id,
                harness,
                harness_version,
            },
            TaskArgs::Show { id } => Self::Show { id },
            TaskArgs::Release => Self::Release,
        }
    }
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
    if matches!(
        &cli.command,
        Command::Daemon {
            command: DaemonArgs::Status | DaemonArgs::Stop
        }
    ) {
        if let Err(error) = daemon::renew_bound(cli.run.clone()) {
            if cli.run.is_none()
                && matches!(
                    &cli.command,
                    Command::Daemon {
                        command: DaemonArgs::Stop
                    }
                )
            {
                let _ = writeln!(io::stderr().lock(), "towermd: 停止前续租失败：{error}");
            } else {
                return Err(error);
            }
        }
    }
    match cli.command {
        Command::Init { path, docs_dir } => run_init(path, docs_dir),
        Command::Status => run_command(TaskCommand::Status, cli.run),
        Command::Review { command } => review_cli::run(command, cli.run),
        Command::Ask(args) => ask_cli::run(args, cli.run),
        Command::Check => print_json(daemon::check_request(daemon::CheckRequest {
            directory: std::env::current_dir().map_err(|error| error.to_string())?,
            run: cli.run,
        })?),

        Command::Trailers { task } => {
            let trailers = daemon::trailers_request(daemon::TrailersRequest {
                directory: std::env::current_dir().map_err(|e| e.to_string())?,
                task,
                run: cli.run,
            })?;
            io::stdout()
                .lock()
                .write_all(trailers.as_bytes())
                .map_err(|e| e.to_string())
        }
        Command::Finish { wait, report } => {
            let report = report.map(read_finish_report).transpose()?;
            let result = daemon::finish_request(daemon::FinishRequest {
                directory: std::env::current_dir().map_err(|e| e.to_string())?,
                run: cli.run,
                wait,
                report,
            })?;
            print_json(result.clone())?;
            match result["job_state"].as_str() {
                Some("completed") if result["result"] != "passed" => {
                    Err("验证未通过；失败或无测试不能完成任务".into())
                }
                Some("interrupted" | "uncertain") => {
                    Err("验证中断或结果不确定；保留原 Evidence ID 查询".into())
                }
                _ => Ok(()),
            }
        }
        Command::Snapshot => print_json(daemon::snapshot_request(daemon::SnapshotRequest {
            directory: std::env::current_dir().map_err(|error| error.to_string())?,
            run: cli.run,
        })?),
        Command::Hub {
            command: HubArgs::Rebuild,
        } => run_rebuild(cli.run),
        Command::Baseline {
            command: BaselineArgs::Schema,
        } => {
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(towermd_core::baseline_schema::schema().as_bytes())
                .map_err(|error| error.to_string())
        }
        Command::Baseline {
            command: BaselineArgs::Submit { json, request_id },
        } => {
            let bytes = std::fs::read(&json)
                .map_err(|error| format!("无法读取 {}：{error}", json.display()))?;
            let value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            let input = towermd_core::baseline_schema::parse_submit(&value)?;
            let request_id = request_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
            // Fail malformed IDs before contacting the daemon, using the shared derivation.
            towermd_core::recovery_contract::recorded::request_event_eid(
                &request_id,
                "draft.saved",
            )?;
            let request = daemon::BaselineSubmitRequest {
                directory: std::env::current_dir().map_err(|error| error.to_string())?,
                run: cli.run,
                request_id: request_id.clone(),
                input,
            };
            print_json(daemon::baseline_submit_request(request)
                .map_err(|error| format!("request_id={request_id}：{error}；重试须使用 --request-id {request_id} 和原输入"))?)
        }
        Command::Baseline {
            command:
                BaselineArgs::Scan {
                    history_limit,
                    wait,
                },
        } => print_json(daemon::coldstart_request(daemon::ColdstartRequest {
            directory: std::env::current_dir().map_err(|error| error.to_string())?,
            run: cli.run,
            options: wait.is_none().then_some(ScanOptions {
                history_limit: history_limit.unwrap_or(200),
            }),
            wait,
        })?),
        Command::Task { command } => run_command(command.into(), cli.run),
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

fn run_command(command: TaskCommand, run: Option<String>) -> Result<(), String> {
    print_json(daemon::request(daemon::Request {
        directory: std::env::current_dir().map_err(|error| error.to_string())?,
        run,
        command,
    })?)
}

fn run_rebuild(run: Option<String>) -> Result<(), String> {
    let response = daemon::rebuild_request(daemon::RebuildRequest {
        directory: std::env::current_dir().map_err(|error| error.to_string())?,
        run,
    })?;
    if let Some(warnings) = response["warnings"].as_array() {
        for warning in warnings.iter().filter_map(serde_json::Value::as_str) {
            writeln!(io::stderr().lock(), "towermd: 警告：{warning}")
                .map_err(|error| error.to_string())?;
        }
    }
    print_json(response)
}

fn read_finish_report(path: PathBuf) -> Result<String, String> {
    use std::io::Read;
    if !std::fs::metadata(&path)
        .map_err(|e| format!("无法读取 {}：{e}", path.display()))?
        .is_file()
    {
        return Err("finish report must be a regular file".into());
    }
    let file =
        std::fs::File::open(&path).map_err(|e| format!("无法读取 {}：{e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("finish report must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("finish report exceeds 1 MiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "finish report must be UTF-8".into())
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
