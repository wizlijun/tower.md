//! clap syntax of the towermd command line; no behaviour lives here.
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "towermd", version)]
pub struct Cli {
    #[arg(long, global = true)]
    pub run: Option<String>,
    #[arg(long, global = true, conflicts_with = "text")]
    pub json: bool,
    #[arg(long, global = true)]
    pub text: bool,
    #[arg(short, long, global = true)]
    pub quiet: bool,
    // Read before parsing by `output::lang::init`; declared for validation and help.
    #[allow(dead_code)]
    #[arg(long, global = true, value_name = "LANG", value_parser = ["en", "zh-CN"])]
    pub lang: Option<String>,
    #[arg(long, global = true, value_name = "UUIDV7")]
    pub request_id: Option<String>,
    #[arg(long, global = true, value_name = "PROJECT_ID")]
    pub project: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Up,
    #[command(hide = true)]
    VerificationWorker,
    Show {
        #[arg(value_name = "PROJECT")]
        project: Option<String>,
    },
    Task {
        #[command(subcommand)]
        command: TaskArgs,
    },
    Propose { #[arg(value_name = "FILE")] input:PathBuf },
    Pause {
        #[arg(value_name = "SCOPE", value_parser = ["all"])]
        scope: String,
    },
    Resume {
        #[arg(value_name = "SCOPE", value_parser = ["all"])]
        scope: String,
        #[arg(long)]
        pause_event:Option<String>,
    },
    Status,
    Ask {
        // `body` keeps the positional apart from the global `--text` flag.
        #[arg(id="body",value_name="TEXT")]
        text:String,
        #[arg(long="option")]
        options:Vec<String>,
        #[arg(long)]
        recommended:Option<usize>,
        #[arg(long)]
        context:Vec<String>,
        #[arg(long)]
        target:Vec<String>,
        #[arg(long)]
        parked:bool,
    },
    Wait { hold:String },
    Answer {
        hold:String,
        #[arg(id="body",value_name="TEXT")]
        text:String,
        #[arg(long)]
        relayed: bool,
        #[arg(long,requires="harness_version")] harness:Option<String>,
        #[arg(long,requires="harness")] harness_version:Option<String>,
    },
    Snapshot,
    Hub {
        #[command(subcommand)]
        command:HubArgs,
    },
    Daemon {
        #[command(subcommand)]
        command: DaemonArgs,
    },
    Init {
        path: Option<PathBuf>,
        #[arg(long)]
        docs_dir: Option<String>,
    },
    Dev {
        #[command(subcommand)]
        command: DevArgs,
    },
    Schema {
        #[arg(value_name = "COMMAND")]
        name: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum TaskArgs {
    Add {
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
    Claim {
        id: String,
        #[arg(long, requires = "harness_version")]
        harness: Option<String>,
        #[arg(long, requires = "harness")]
        harness_version: Option<String>,
    },
    Show { id: Option<String> },
    Release,
}

#[derive(Subcommand)]
pub enum DevArgs {
    Check,
    Finish {
        #[arg(long, value_name = "EVIDENCE_ID")]
        wait: Option<String>,
    },
    Trailers { task: String },
    Scan {
        #[command(subcommand)]
        command: ScanArgs,
    },
    Draft {
        #[command(subcommand)]
        command: DraftArgs,
    },
}

#[derive(Subcommand)]
pub enum ScanArgs {
    Start,
    Wait { #[arg(value_name = "SCAN_ID")] id: String },
    Show { #[arg(value_name = "SCAN_ID")] id: String },
    Cancel { #[arg(value_name = "SCAN_ID")] id: String },
}

#[derive(Subcommand)]
pub enum DraftArgs {
    Template { #[arg(value_name = "SCAN_ID")] scan: String },
    Submit {
        #[arg(value_name = "FILE")]
        input: PathBuf,
        #[arg(long)]
        producer: String,
    },
    Show { #[arg(value_name = "DRAFT_ID")] id: String },
    Ls,
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
            Command::Schema { .. } => "schema",
            Command::Dev { command } => match command {
                DevArgs::Check => "dev.check",
                DevArgs::Finish { .. } => "dev.finish",
                DevArgs::Trailers { .. } => "dev.trailers",
                DevArgs::Scan { command } => match command {
                    ScanArgs::Start => "dev.scan.start",
                    ScanArgs::Wait { .. } => "dev.scan.wait",
                    ScanArgs::Show { .. } => "dev.scan.show",
                    ScanArgs::Cancel { .. } => "dev.scan.cancel",
                },
                DevArgs::Draft { command } => match command {
                    DraftArgs::Template { .. } => "dev.draft.template",
                    DraftArgs::Submit { .. } => "dev.draft.submit",
                    DraftArgs::Show { .. } => "dev.draft.show",
                    DraftArgs::Ls => "dev.draft.ls",
                },
            },
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
                | Command::Ask { .. }
                | Command::Answer { .. }
                | Command::Snapshot
                | Command::Hub { .. }
                | Command::Dev { command: DevArgs::Finish { wait: None } | DevArgs::Trailers { .. } }
                | Command::Dev { command: DevArgs::Scan { command: ScanArgs::Start } }
        )
    }

    /// Commands that deliver pending answers and interjections first.
    pub fn repo_bound(&self) -> bool {
        !matches!(
            self,
            Command::Up | Command::Init { .. } | Command::Daemon { .. } | Command::Hub { .. } | Command::VerificationWorker
                | Command::Schema { .. }
        )
    }

    /// Read-oriented project and scan commands still renew a bound run.
    pub fn renews_run(&self) -> bool {
        matches!(self, Command::Show { .. } | Command::Dev { command: DevArgs::Scan { .. } | DevArgs::Draft { .. } })
            && self.repo_bound()
    }
}
