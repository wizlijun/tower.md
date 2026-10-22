//! clap syntax of the towermd command line; no behaviour lives here.
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "towermd",
    version,
    about = "Tower.md：让 Agent 去做，只在关键处放行",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        help = "显式指定本机运行，覆盖工作树与分支自动定位"
    )]
    pub run: Option<String>,
    #[arg(long, global = true, conflicts_with = "text", help = "输出 JSON 信封（stdout 不是终端时的默认）")]
    pub json: bool,
    #[arg(long, global = true, help = "输出给人看的文本（stdout 是终端时的默认）")]
    pub text: bool,
    #[arg(short, long, global = true, help = "文本模式下只输出消息与错误")]
    pub quiet: bool,
    /// Read before parsing by `output::lang::init`; declared for validation and help.
    #[allow(dead_code)]
    #[arg(long, global = true, value_name = "LANG", value_parser = ["en", "zh-CN"],
        help = "本地文字的语言；省略时依次看 TOWERMD_LANG、LC_ALL、LANG 与系统语言")]
    pub lang: Option<String>,
    #[arg(long, global = true, value_name = "UUIDV7",
        help = "写命令的请求 ID（UUID v7）；省略时自动生成，失败后沿用它可安全重试")]
    pub request_id: Option<String>,
    #[arg(long, global = true, value_name = "PROJECT_ID", help = "项目 ID；C0 只接受当前仓库的项目")]
    pub project: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "启动已构建的 App 和本机守护进程")]
    Up,
    #[command(hide = true)]
    VerificationWorker,
    #[command(about = "读取项目画布（经本机守护进程）")]
    Show {
        #[arg(value_name = "PROJECT", help = "项目 ID；省略时为当前仓库")]
        project: Option<String>,
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
    Propose { #[arg(value_name = "FILE", help = "JSON 文件；- 表示标准输入")] input:PathBuf },
    #[command(about = "暂停项目新任务准入，并向运行中的 Agent 送达检查点指令")]
    Pause {
        #[arg(value_name = "SCOPE", value_parser = ["all"], help = "范围；C0 只支持 all")]
        scope: String,
    },
    #[command(about = "请求 App 中的人解除项目暂停；命令行不会自行恢复")]
    Resume {
        #[arg(value_name = "SCOPE", value_parser = ["all"], help = "范围；C0 只支持 all")]
        scope: String,
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
    },
    #[command(about = "通过 G5 后输出提交 trailer；终态证据先耐久保存")]
    Trailers {
        task:String,
    },
    #[command(about = "向 App 提问并暂停受影响任务；不自动选择答案")]
    Ask {
        // `body` keeps the positional apart from the global `--text` flag.
        #[arg(id="body",value_name="TEXT")]
        text:String,
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
    },
    #[command(about = "等待问题答复，单次最多90秒；十分钟累计上限后挂起")]
    Wait { hold:String },
    #[command(about = "转述人在会话中的本次回答；不能记为决策、规则或批准慢层")]
    Answer {
        hold:String,
        #[arg(id="body",value_name="TEXT")]
        text:String,
        #[arg(long, help = "确认这是人在当前会话中亲口给出的回答；只做转述，不记为决策、规则或批准")]
        relayed: bool,
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
pub enum TaskArgs {
    #[command(about = "从 JSON 新建任务，路径必须落在已批准模块授权上限内")]
    Add {
        #[arg(value_name = "FILE", help = "JSON 文件；- 表示标准输入")]
        input: PathBuf,
    },
    #[command(about = "在 t/<task-id> 工作树领取任务并取得模块租约")]
    Claim {
        id: String,
        #[arg(long, requires = "harness_version")]
        harness: Option<String>,
        #[arg(long, requires = "harness")]
        harness_version: Option<String>,
    },
    #[command(about = "查看指定任务；省略 ID 时按当前工作树或 --run 查找")]
    Show { id: Option<String> },
    #[command(about = "释放当前或 --run 指定的运行；任务回到就绪")]
    Release,
}

#[derive(Subcommand)]
pub enum BaselineArgs {
    #[command(about = "输出与提交校验相同的 S6 JSON Schema")]
    Schema,
    #[command(about = "从已完成 S5 扫描导出综合模板；内容尚未由 Agent 综合")]
    Template {
        id: String,
    },
    #[command(about = "提交 S6 JSON 为未批准草稿；有 origin 时推送私有草稿引用")]
    Submit {
        #[arg(value_name = "FILE", help = "JSON 文件；- 表示标准输入")]
        input: PathBuf,
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
pub enum DaemonArgs {
    Ensure,
    Status,
    Stop,
    #[command(hide = true)]
    Serve,
}
#[derive(Subcommand)]
pub enum HubArgs { Rebuild }

impl Command {
    /// Dotted leaf name; also the schema name (T9) and the text renderer key.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Up => "up",
            Command::VerificationWorker => "verification-worker",
            Command::Show { .. } => "show",
            Command::Baseline { command } => match command {
                BaselineArgs::Schema => "baseline.schema",
                BaselineArgs::Template { .. } => "baseline.template",
                BaselineArgs::Submit { .. } => "baseline.submit",
                BaselineArgs::List => "baseline.list",
                BaselineArgs::Scan { .. } => "baseline.scan",
                BaselineArgs::Show { .. } => "baseline.show",
                BaselineArgs::Cancel { .. } => "baseline.cancel",
            },
            Command::Task { command } => match command {
                TaskArgs::Add { .. } => "task.add",
                TaskArgs::Claim { .. } => "task.claim",
                TaskArgs::Show { .. } => "task.show",
                TaskArgs::Release => "task.release",
            },
            Command::Propose { .. } => "propose",
            Command::Pause { .. } => "pause",
            Command::Resume { .. } => "resume",
            Command::Status => "status",
            Command::Check => "check",
            Command::Finish { .. } => "finish",
            Command::Trailers { .. } => "trailers",
            Command::Ask { .. } => "ask",
            Command::Wait { .. } => "wait",
            Command::Answer { .. } => "answer",
            Command::Snapshot => "snapshot",
            Command::Hub { .. } => "hub.rebuild",
            Command::Daemon { command } => match command {
                DaemonArgs::Ensure => "daemon.ensure",
                DaemonArgs::Status => "daemon.status",
                DaemonArgs::Stop => "daemon.stop",
                DaemonArgs::Serve => "daemon.serve",
            },
            Command::Init { .. } => "init",
        }
    }

    /// Writes always carry a request ID so a retry is deduplicated by the hub.
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            Command::Task { command: TaskArgs::Add { .. } | TaskArgs::Claim { .. } | TaskArgs::Release }
                | Command::Propose { .. }
                | Command::Pause { .. }
                | Command::Resume { .. }
                | Command::Finish { .. }
                | Command::Trailers { .. }
                | Command::Ask { .. }
                | Command::Answer { .. }
                | Command::Snapshot
                | Command::Hub { .. }
                | Command::Baseline { command: BaselineArgs::Scan { wait: None } }
        )
    }

    /// Commands that deliver pending answers and interjections first.
    pub fn repo_bound(&self) -> bool {
        !matches!(
            self,
            Command::Up | Command::Init { .. } | Command::Daemon { .. } | Command::Hub { .. } | Command::VerificationWorker
                | Command::Baseline { command: BaselineArgs::Schema }
        )
    }

    /// Read-oriented project and scan commands still renew a bound run.
    pub fn renews_run(&self) -> bool {
        matches!(self, Command::Show { .. } | Command::Baseline { .. }) && self.repo_bound()
    }
}
