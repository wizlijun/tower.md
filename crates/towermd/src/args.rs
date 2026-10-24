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
    #[arg(long, global = true, value_name = "PATH|ID")]
    pub project: Option<String>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    Up,
    #[command(hide = true)]
    VerificationWorker,
    // C1 reads (§2.1): read-only, never renew a run, never approve.
    Needs {
        #[arg(long)]
        all: bool,
    },
    Since {
        #[arg(long, value_name = "TIME")]
        from: Option<String>,
    },
    Overview,
    Show {
        #[arg(value_name = "ID")]
        id: Option<String>,
    },
    Ls {
        #[arg(value_name = "KIND", value_parser = clap::builder::PossibleValuesParser::new(crate::read::KINDS))]
        kind: String,
        #[arg(long)]
        state: Option<String>,
        #[arg(long, value_name = "TIME")]
        since: Option<String>,
    },
    History {
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "TIME")]
        since: Option<String>,
    },
    Watch {
        #[arg(long, value_name = "ID")]
        scope: Option<String>,
        /// Stop after this many seconds (tests and scripts).
        #[arg(long, hide = true, value_name = "SECONDS")]
        exit_after: Option<u64>,
    },
    Open {
        #[arg(value_name = "ID")]
        id: Option<String>,
    },
    Decision {
        #[command(subcommand)]
        command: DecisionArgs,
    },
    Rule {
        #[command(subcommand)]
        command: RuleArgs,
    },
    RebuildIndex,
    Doctor,
    Whoami,
    Completion {
        #[arg(value_name = "SHELL")]
        shell: clap_complete::Shell,
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
    Wait {
        #[arg(value_name="ID")]
        hold:String,
        #[arg(long, value_name="N", value_parser=clap::value_parser!(u64).range(1..=85))]
        seconds:Option<u64>,
    },
    /// FR-68: say a delivered prompt was acted on; not progress, not approval.
    Handled {
        #[arg(value_name = "PROMPT_ID")]
        prompt: String,
        #[arg(long)]
        note: Option<String>,
    },
    Reply {
        #[arg(value_name = "REQUEST_ID")]
        request: String,
        #[arg(long, value_parser = ["not_started", "in_progress", "waiting", "blocked", "claimed_done"])]
        state: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        next: Option<String>,
        /// Evidence for the answer; a node question requires at least one.
        #[arg(long = "ref", value_name = "REF")]
        refs: Vec<String>,
        /// The sender, for a request answered outside a run.
        #[arg(long, value_name = "ID")]
        from: Option<String>,
    },
    // Tower API `report` (FR-115): progress is a claim, never completion.
    Report {
        #[arg(long, value_parser = ["not_started", "in_progress", "waiting", "blocked", "claimed_done"])]
        state: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        next: Option<String>,
        #[arg(long, value_name = "DONE/TOTAL")]
        progress: Option<String>,
        #[arg(long, requires = "progress")]
        unit: Option<String>,
        /// FR-127: declare that this sender answers questions on the target;
        /// sticky until `--no-answers`.
        #[arg(long, conflicts_with = "no_answers")]
        answers: bool,
        #[arg(long)]
        no_answers: bool,
        #[command(flatten)]
        about: TowerArgs,
    },
    // Tower API `signal` (FR-115, FR-118).
    Signal {
        #[arg(value_name = "KIND")]
        kind: String,
        #[arg(long, value_parser = ["log", "passive", "active", "time_sensitive"])]
        level: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        detail: Option<String>,
        #[arg(long)]
        value: Option<String>,
        #[arg(long)]
        threshold: Option<String>,
        #[arg(long)]
        deterministic: bool,
        #[command(flatten)]
        about: TowerArgs,
    },
    // Tower API `summary` (FR-139): an object's change summary, at most 7 lines.
    Summary {
        #[arg(long = "line", value_name = "LABEL=VALUE", required = true)]
        lines: Vec<String>,
        #[command(flatten)]
        about: TowerArgs,
    },
    // An unpromoted idea (FR-183); promoting it is the App's alone.
    Idea {
        #[command(subcommand)]
        command: IdeaArgs,
    },
    // Installed extensions (FR-120), read-only; installing is a proposal.
    Ext {
        #[command(subcommand)]
        command: ExtArgs,
    },
    // Templates a person can place in the App (FR-142, FR-143), read-only.
    Template {
        #[command(subcommand)]
        command: TemplateArgs,
    },
    // Frames (FR-147): read one, and write its cards through the Tower API
    // `card` verb under the frame's lease. Creating, moving or changing the
    // rules of a frame is the App's alone.
    Frame {
        #[command(subcommand)]
        command: FrameArgs,
    },
    Answer {
        #[arg(value_name="QUESTION_ID")]
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

/// The delegate's self-reported configuration (FR-203 fingerprint).
#[derive(clap::Args, Default)]
pub struct SourceArgs {
    #[arg(long, requires = "harness_version")]
    pub harness: Option<String>,
    #[arg(long, requires = "harness")]
    pub harness_version: Option<String>,
    #[arg(long, requires = "harness")]
    pub vendor: Option<String>,
    #[arg(long, requires = "harness")]
    pub model: Option<String>,
    #[arg(long, requires = "harness")]
    pub skills_version: Option<String>,
}

/// What a Tower API message is about and who says they sent it. Inside a
/// run both default to the run's task and delegate.
#[derive(clap::Args)]
pub struct TowerArgs {
    #[arg(long, value_name = "OBJECT_ID")]
    pub target: Option<String>,
    #[arg(long, value_name = "ID")]
    pub from: Option<String>,
    #[arg(long, value_parser = ["delegate", "source"], requires = "from")]
    pub from_kind: Option<String>,
    #[arg(long = "ref", value_name = "REF")]
    pub refs: Vec<String>,
}

#[derive(Subcommand)]
pub enum TaskArgs {
    Add {
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
    Edit {
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
    Claim {
        id: Option<String>,
        #[command(flatten)]
        source: SourceArgs,
    },
    Show { id: Option<String> },
    Ls {
        #[arg(long)]
        state: Option<String>,
    },
    Release,
}

#[derive(Subcommand)]
pub enum DevArgs {
    Check,
    Finish {
        #[arg(long, value_name = "EVIDENCE_ID", conflicts_with = "claims")]
        wait: Option<String>,
        /// `[{"criterion": 0, "note": "…"}]`; recorded as self-report only.
        #[arg(long, value_name = "FILE")]
        claims: Option<PathBuf>,
    },
    Clarify {
        #[arg(value_name = "TASK")]
        task: String,
        #[arg(value_name = "FILE")]
        input: PathBuf,
        #[command(flatten)]
        source: SourceArgs,
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
    Start {
        #[arg(long)]
        from_baseline: bool,
    },
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
    Compare { #[arg(value_name = "DRAFT_ID")] id: String },
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
#[derive(Subcommand)]
pub enum ExtArgs {
    Ls,
    Show {
        #[arg(value_name = "ID")]
        id: String,
    },
}
#[derive(Subcommand)]
pub enum TemplateArgs { Ls }
#[derive(Subcommand)]
pub enum DecisionArgs {
    Show {
        #[arg(value_name = "ID")]
        id: String,
    },
    Ls,
}
#[derive(Subcommand)]
pub enum RuleArgs { Ls }
#[derive(Subcommand)]
pub enum FrameArgs {
    Show {
        #[arg(value_name = "FRAME_ID")]
        frame: String,
    },
    Add {
        #[arg(value_name = "FRAME_ID")]
        frame: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        state: Option<String>,
        /// `KEY=VALUE`; a number or boolean value stays one.
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,
        #[arg(long, value_name = "CARD_ID")]
        parent: Option<String>,
        #[command(flatten)]
        from: CardSender,
    },
    Update {
        #[arg(value_name = "CARD_ID")]
        card: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,
        #[arg(long = "clear", value_name = "KEY")]
        clear: Vec<String>,
        #[command(flatten)]
        from: CardSender,
    },
    Move {
        #[arg(value_name = "CARD_ID")]
        card: String,
        #[arg(value_name = "STATE")]
        state: String,
        #[command(flatten)]
        from: CardSender,
    },
    Link {
        #[arg(value_name = "CARD_ID")]
        card: String,
        #[arg(value_name = "TO_CARD_ID")]
        to: String,
        #[command(flatten)]
        from: CardSender,
    },
    Archive {
        #[arg(value_name = "CARD_ID")]
        card: String,
        #[command(flatten)]
        from: CardSender,
    },
    /// Ask a person to change the frame's rules; only the App applies them.
    Propose {
        #[arg(value_name = "FRAME_ID")]
        frame: String,
        /// A JSON file of the rules to change, `-` for stdin.
        #[arg(value_name = "FILE")]
        rules: PathBuf,
        #[command(flatten)]
        from: CardSender,
    },
}

/// Who writes a card: inside a run, the run's delegate by default.
#[derive(clap::Args)]
pub struct CardSender {
    #[arg(long, value_name = "ID")]
    pub from: Option<String>,
    #[arg(long, value_parser = ["delegate", "source"], requires = "from")]
    pub from_kind: Option<String>,
}
#[derive(Subcommand)]
pub enum IdeaArgs {
    Add {
        #[arg(id = "body", value_name = "TEXT")]
        text: String,
    },
}

impl Command {
    /// Dotted leaf name; also the schema name (T9) and the text renderer key.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Up => "up",
            Command::VerificationWorker => "verification-worker",
            Command::Needs { .. } => "needs",
            Command::Since { .. } => "since",
            Command::Overview => "overview",
            Command::Show { .. } => "show",
            Command::Ls { .. } => "ls",
            Command::History { .. } => "history",
            Command::Watch { .. } => "watch",
            Command::Open { .. } => "open",
            Command::Decision { command: DecisionArgs::Show { .. } } => "decision.show",
            Command::Decision { command: DecisionArgs::Ls } => "decision.ls",
            Command::Rule { command: RuleArgs::Ls } => "rule.ls",
            Command::RebuildIndex => "rebuild-index",
            Command::Doctor => "doctor",
            Command::Whoami => "whoami",
            Command::Completion { .. } => "completion",
            Command::Task { command } => match command {
                TaskArgs::Add { .. } => "task.add",
                TaskArgs::Edit { .. } => "task.edit",
                TaskArgs::Ls { .. } => "task.ls",
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
            Command::Handled { .. } => "handled",
            Command::Reply { .. } => "reply",
            Command::Report { .. } => "report",
            Command::Signal { .. } => "signal",
            Command::Summary { .. } => "summary",
            Command::Idea { command: IdeaArgs::Add { .. } } => "idea.add",
            Command::Ext { command: ExtArgs::Ls } => "ext.ls",
            Command::Ext { command: ExtArgs::Show { .. } } => "ext.show",
            Command::Template { command: TemplateArgs::Ls } => "template.ls",
            Command::Frame { command } => match command {
                FrameArgs::Show { .. } => "frame.show",
                FrameArgs::Add { .. } => "frame.add",
                FrameArgs::Update { .. } => "frame.update",
                FrameArgs::Move { .. } => "frame.move",
                FrameArgs::Link { .. } => "frame.link",
                FrameArgs::Archive { .. } => "frame.archive",
                FrameArgs::Propose { .. } => "frame.propose",
            },
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
                DevArgs::Clarify { .. } => "dev.clarify",
                DevArgs::Trailers { .. } => "dev.trailers",
                DevArgs::Scan { command } => match command {
                    ScanArgs::Start { .. } => "dev.scan.start",
                    ScanArgs::Wait { .. } => "dev.scan.wait",
                    ScanArgs::Show { .. } => "dev.scan.show",
                    ScanArgs::Cancel { .. } => "dev.scan.cancel",
                },
                DevArgs::Draft { command } => match command {
                    DraftArgs::Template { .. } => "dev.draft.template",
                    DraftArgs::Submit { .. } => "dev.draft.submit",
                    DraftArgs::Show { .. } => "dev.draft.show",
                    DraftArgs::Ls => "dev.draft.ls",
                    DraftArgs::Compare { .. } => "dev.draft.compare",
                },
            },
        }
    }

    /// Writes always carry a request ID so a retry is deduplicated by the hub.
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            Command::Task { command: TaskArgs::Add { .. } | TaskArgs::Edit { .. } | TaskArgs::Claim { .. } | TaskArgs::Release }
                | Command::Propose { .. }
                | Command::Pause { .. }
                | Command::Resume { .. }
                | Command::Ask { .. }
                | Command::Answer { .. }
                | Command::Handled { .. }
                | Command::Reply { .. }
                | Command::Report { .. }
                | Command::Signal { .. }
                | Command::Summary { .. }
                | Command::Idea { .. }
                | Command::Frame { command: FrameArgs::Add { .. } | FrameArgs::Update { .. } | FrameArgs::Move { .. } | FrameArgs::Link { .. } | FrameArgs::Archive { .. } | FrameArgs::Propose { .. } }
                | Command::Snapshot
                | Command::Hub { .. }
                | Command::Dev { command: DevArgs::Finish { wait: None, .. } | DevArgs::Trailers { .. } | DevArgs::Clarify { .. } }
                | Command::Dev { command: DevArgs::Scan { command: ScanArgs::Start { .. } | ScanArgs::Cancel { .. } } }
                | Command::Dev { command: DevArgs::Draft { command: DraftArgs::Submit { .. } } }
                | Command::Init { .. }
        )
    }

    /// Commands that never touch a repository, so `--project` is not checked.
    pub fn repo_free(&self) -> bool {
        matches!(
            self,
            Command::Up | Command::Daemon { .. } | Command::VerificationWorker | Command::Schema { .. } | Command::Completion { .. } | Command::Overview
        )
    }

    /// Commands that deliver pending answers and interjections first.
    /// `watch` changes nothing, delivery included; `doctor` and `whoami`
    /// must work when the daemon or the project does not.
    pub fn repo_bound(&self) -> bool {
        !self.repo_free()
            && !matches!(self, Command::Init { .. } | Command::Hub { .. } | Command::Watch { .. } | Command::Doctor | Command::Whoami | Command::RebuildIndex)
    }

    /// `show` and the scan commands still renew a bound run, as in C0. The
    /// other C1 reads (§2.1) do not: looking is not working.
    pub fn renews_run(&self) -> bool {
        matches!(self, Command::Show { .. } | Command::Dev { command: DevArgs::Scan { .. } | DevArgs::Draft { .. } })
            && self.repo_bound()
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use crate::output::help::shell_words;
    use clap::{error::ErrorKind, Parser};

    const DOCS: &[(&str, &str)] = &[
        ("skills/README.md", include_str!("../../../skills/README.md")),
        ("skills/towermd-ask/SKILL.md", include_str!("../../../skills/towermd-ask/SKILL.md")),
        ("skills/towermd-baseline/SKILL.md", include_str!("../../../skills/towermd-baseline/SKILL.md")),
        ("skills/towermd-claim/SKILL.md", include_str!("../../../skills/towermd-claim/SKILL.md")),
        ("skills/towermd-finish/SKILL.md", include_str!("../../../skills/towermd-finish/SKILL.md")),
        ("skills/towermd-init/SKILL.md", include_str!("../../../skills/towermd-init/SKILL.md")),
        ("skills/towermd-propose/SKILL.md", include_str!("../../../skills/towermd-propose/SKILL.md")),
        ("towermd-core/templates/agents.md", include_str!("../../towermd-core/templates/agents.md")),
        ("towermd-devkit/templates/agents-dev.md", include_str!("../../towermd-devkit/templates/agents-dev.md")),
    ];
    /// Named in the skills as not yet provided (M2); must stay unparsable.
    const NOT_YET: &[&str] = &["towermd review request"];

    /// Inline `towermd …` spans plus `towermd …` lines inside fenced blocks.
    fn command_lines(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut fenced = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                if line.trim_start().starts_with("towermd ") {
                    found.push(line.trim().to_owned());
                }
                continue;
            }
            for (index, span) in line.split('`').enumerate() {
                if index % 2 == 1 && span.starts_with("towermd ") {
                    found.push(span.to_owned());
                }
            }
        }
        found
    }

    fn fill_placeholders(line: &str) -> String {
        let mut out = String::new();
        let mut depth = 0;
        for ch in line.chars() {
            match ch {
                '<' => { if depth == 0 { out.push('x'); } depth += 1; }
                '>' if depth > 0 => depth -= 1,
                _ if depth > 0 => {}
                ch => out.push(ch),
            }
        }
        out
    }

    #[test]
    fn every_documented_command_parses() {
        let mut checked = 0;
        for (file, text) in DOCS {
            for line in command_lines(text) {
                if NOT_YET.iter().any(|prefix| line.starts_with(prefix)) {
                    continue;
                }
                let words = shell_words(&fill_placeholders(&line));
                match Cli::try_parse_from(&words) {
                    Ok(_) => {}
                    Err(error) if matches!(error.kind(), ErrorKind::MissingRequiredArgument | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand) => {}
                    Err(error) => panic!("{file}: `{line}`: {error}"),
                }
                checked += 1;
            }
        }
        assert!(checked >= 20, "only {checked} command lines found");
    }
}
