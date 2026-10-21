mod app_launch;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
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
    #[arg(
        long,
        global = true,
        help = "显式指定本机运行，覆盖工作树与分支自动定位"
    )]
    run: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "启动已构建的 App 和本机守护进程")]
    Up,
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
    #[command(about = "管理任务与工作树运行（只有 App 能批准基线）")]
    Task {
        #[command(subcommand)]
        command: TaskArgs,
    },
    #[command(about = "提交真实草稿的慢层变更提案（JSON）：理由、备选方案和建议；App 才能批准")]
    Propose { json:PathBuf, #[arg(long)] request_id:Option<String> },
    #[command(about = "暂停项目新任务准入，并向运行中的 Agent 送达检查点指令")]
    Pause { #[arg(long,required=true)] all:bool, #[arg(long)] request_id:Option<String> },
    #[command(about = "请求 App 中的人解除项目暂停；命令行不会自行恢复")]
    Resume {
        #[arg(long,required=true)] all:bool,
        #[arg(long)] request_id:Option<String>,
        #[arg(long,help="精确重试时沿用原暂停事件 ID；省略时读取当前暂停")]
        pause_event:Option<String>,
    },
    #[command(about = "查询权威 main 上的项目状态与当前运行（JSON），并续租")]
    Status,
    #[command(about = "检查当前任务的路径、慢层保护和主干门禁")]
    Check,
    #[command(about = "冻结提交并在后台运行真实验证；不把自述完成算作进度")]
    Finish {
        #[arg(long,value_name="EVIDENCE_ID",help="等待已有验证，每次最多90秒")]
        wait:Option<String>,
        #[arg(long)]
        request_id:Option<String>,
    },
    #[command(about = "通过 G5 后输出提交 trailer；终态证据先耐久保存")]
    Trailers {
        task:String,
        #[arg(long)]
        request_id:Option<String>,
    },
    #[command(about = "向 App 提问并暂停受影响任务；不自动选择答案")]
    Ask {
        #[arg(required_unless_present="wait",conflicts_with="wait")]
        text:Option<String>,
        #[arg(long,value_name="HOLD_ID",conflicts_with_all=["text","options","recommended","context","target","parked","request_id"])]
        wait:Option<String>,
        #[arg(long="option")]
        options:Vec<String>,
        #[arg(long,help="推荐选项的零起始下标，仅为建议")]
        recommended:Option<usize>,
        #[arg(long)]
        context:Vec<String>,
        #[arg(long)]
        target:Vec<String>,
        #[arg(long)]
        parked:bool,
        #[arg(long)]
        request_id:Option<String>,
    },
    #[command(about = "等待问题答复，单次最多90秒；十分钟累计上限后挂起")]
    Wait { hold:String },
    #[command(about = "转述人在会话中的本次回答；不能记为决策、规则或批准慢层")]
    Answer {
        hold:String,
        text:String,
        #[arg(long)] request_id:Option<String>,
        #[arg(long,requires="harness_version")] harness:Option<String>,
        #[arg(long,requires="harness")] harness_version:Option<String>,
    },
    #[command(about = "由唯一主干写入器生成可恢复快照")]
    Snapshot,
    #[command(about = "请求重建 hub，必须随后在 App 中明确确认")]
    Hub {
        #[command(subcommand)]
        command:HubArgs,
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
enum TaskArgs {
    #[command(about = "从 JSON 新建任务，路径必须落在已批准模块授权上限内")]
    Add {
        json: PathBuf,
        #[arg(long)]
        request_id: Option<String>,
    },
    #[command(about = "在 t/<task-id> 工作树领取任务并取得模块租约")]
    Claim {
        id: String,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long, requires = "harness_version")]
        harness: Option<String>,
        #[arg(long, requires = "harness")]
        harness_version: Option<String>,
    },
    #[command(about = "查看指定任务；省略 ID 时按当前工作树或 --run 查找")]
    Show { id: Option<String> },
    #[command(about = "释放当前或 --run 指定的运行；任务回到就绪")]
    Release {
        #[arg(long)]
        request_id: Option<String>,
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
#[derive(Subcommand)]
enum HubArgs { Rebuild }

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
    let selected_run = cli.run;
    if !matches!(&cli.command,Command::Up|Command::Init{..}|Command::Daemon{..}|Command::Hub{..}
        |Command::VerificationWorker|Command::Baseline{command:BaselineArgs::Schema})
    {
        let cwd=std::env::current_dir().map_err(|e|e.to_string())?;
        if selected_run.is_some() || Catalog::load_optional(&cwd)?.is_some() {
            daemon::deliver_messages(selected_run.as_deref())?;
        }
    }
    // Even read-oriented project/scan commands renew an already bound run.
    // Schema, init and daemon lifecycle operations remain independent of a repo.
    if matches!(
        &cli.command,
        Command::Project { .. } | Command::Baseline { .. }
    ) && !matches!(
        &cli.command,
        Command::Baseline {
            command: BaselineArgs::Schema
        }
    ) {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        if selected_run.is_some() || Catalog::load_optional(&cwd)?.is_some() {
            daemon::task_status(selected_run.as_deref())?;
        }
    }
    match cli.command {
        Command::VerificationWorker => worker::serve(),
        Command::Baseline {
            command: BaselineArgs::Schema,
        } => print_json(towermd_devkit::baseline::schema()),
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
        Command::Up=>print_json(app_launch::up()?),
        Command::Propose{json,request_id}=>{
            let input:serde_json::Value=serde_json::from_slice(&std::fs::read(json).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            let request_id=request_id.unwrap_or_else(||uuid::Uuid::now_v7().to_string());
            print_json(daemon::propose(input,&request_id,selected_run.as_deref())?)
        }
        Command::Pause{all:_,request_id}=>print_json(daemon::pause_all(&request_id.unwrap_or_else(||uuid::Uuid::now_v7().to_string()))?),
        Command::Resume{all:_,request_id,pause_event}=>print_json(daemon::request_resume_all(&request_id.unwrap_or_else(||uuid::Uuid::now_v7().to_string()),pause_event.as_deref())?),
        Command::Status => print_json(daemon::task_status(selected_run.as_deref())?),
        Command::Check => print_json(daemon::task_check(selected_run.as_deref())?),
        Command::Finish{wait,request_id} => print_json(daemon::task_finish(
            &request_id.unwrap_or_else(||uuid::Uuid::now_v7().to_string()),selected_run.as_deref(),wait.as_deref())?),
        Command::Trailers{task,request_id} => {
            let value=daemon::task_trailers(&task,&request_id.unwrap_or_else(||uuid::Uuid::now_v7().to_string()),selected_run.as_deref())?;
            let text=value["trailers"].as_str().ok_or("trailer 响应无效")?;
            let mut stdout=io::stdout().lock();
            stdout.write_all(text.as_bytes()).and_then(|()|stdout.flush()).map_err(|e|e.to_string())
        }
        Command::Ask{text,wait,options,recommended,context,target,parked,request_id} => {
            if let Some(hold)=wait{
                print_json(daemon::wait_for_answer(&hold,selected_run.as_deref())?)?;
                return daemon::deliver_messages(selected_run.as_deref());
            }
            let text=text.ok_or("ask 缺少问题正文")?;
            let id=request_id.unwrap_or_else(||uuid::Uuid::now_v7().to_string());
            let value=daemon::ask(serde_json::json!({"request_id":id,"text":text,"options":options,"recommended":recommended,
                "context":context,"targets":target,"mode":if parked{"parked"}else{"live"}}),selected_run.as_deref())
                .map_err(|error|format!("{error}；未知结果请沿用 ask --request-id {id}"))?;
            print_json(value)?;
            daemon::deliver_messages(selected_run.as_deref())
        }
        Command::Wait{hold} => {
            print_json(daemon::wait_for_answer(&hold,selected_run.as_deref())?)?;
            daemon::deliver_messages(selected_run.as_deref())
        }
        Command::Answer{hold,text,request_id,harness,harness_version} => {
            let id=request_id.unwrap_or_else(||uuid::Uuid::now_v7().to_string());
            let source=harness.zip(harness_version).map(|(name,version)|towermd_devkit::task_model::HarnessSource{name,version});
            let value=daemon::relay_answer(serde_json::json!({"request_id":id,"hold":hold,"text":text,"self_reported":source}),selected_run.as_deref())
                .map_err(|error|format!("{error}；未知结果请沿用 answer --request-id {id}"))?;
            print_json(value)?;
            daemon::deliver_messages(selected_run.as_deref())
        }
        Command::Snapshot => print_json(daemon::snapshot()?),
        Command::Hub{command:HubArgs::Rebuild} => print_json(daemon::rebuild_request()?),
        Command::Task {
            command:
                TaskArgs::Add {
                    json: path,
                    request_id,
                },
        } => {
            let input = read_json(&path)?;
            print_json(daemon::task_add(
                input,
                &request_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                selected_run.as_deref(),
            )?)
        }
        Command::Task {
            command:
                TaskArgs::Claim {
                    id,
                    request_id,
                    harness,
                    harness_version,
                },
        } => {
            if selected_run.is_some() {
                daemon::task_status(selected_run.as_deref())?;
            }
            let source = harness
                .zip(harness_version)
                .map(|(name, version)| towermd_devkit::task_model::HarnessSource { name, version });
            print_json(daemon::task_claim(
                &id,
                &request_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                source,
            )?)
        }
        Command::Task {
            command: TaskArgs::Show { id },
        } => print_json(daemon::task_show(id.as_deref(), selected_run.as_deref())?),
        Command::Task {
            command: TaskArgs::Release { request_id },
        } => print_json(daemon::task_release(
            &request_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
            selected_run.as_deref(),
        )?),
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

fn read_json(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("输入超过2 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
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
