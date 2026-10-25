//! M3 runner (10.3, 10.4): starts one headless harness for a claimed run,
//! reads its output through the adapter, keeps the raw output on this
//! machine, and records `run.started` / `run.finished` (6.3).
//!
//! - The run directory is `<git-common-dir>/towermd/runs/<run>/`: `task.md`,
//!   the report schema, `output.jsonl` (raw stdout) and `stderr.log`. None of
//!   it is state; replay never reads it (FR-08).
//! - Everything the harness needs is passed on its command line; the runner
//!   never reads or writes the user's global harness configuration.
//! - Stopping sends SIGINT to the process group, waits up to the grace
//!   period (60 s by default, FR-27), then SIGTERM, then SIGKILL, using the
//!   system `kill` command.
//! - External failures are retried within the run; every other failure,
//!   timeout, crash or budget stop ends the run.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::{sleep_until, Instant};

use crate::budget::{self, Budget, Refusal, Spend};
use crate::harness::{self, Adapter, Final, Permission, RunSpec, Signal};
use crate::run_report::{
    RunCheckpoint, CHECKPOINT,
    self, clip, Outcome, RawOutput, ReportStatus, RunFinished, RunReport, RunRetry, RunStarted, Stop, Tokens, FINISHED,
    RETRY, RUNNER, STARTED,
};
use towermd_core::hub_protocol::NewEvent;

pub const TASK_CARD: &str = "task.md";
pub const SCHEMA: &str = "report.schema.json";
pub const RAW: &str = "output.jsonl";
pub const STDERR: &str = "stderr.log";
/// FR-27: wait this long after SIGINT before SIGTERM.
pub const GRACE: Duration = Duration::from_secs(60);
const TERM_GRACE: Duration = Duration::from_secs(5);
/// After the harness exits, how long to wait for its pipes to close.
const DRAIN: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Wake-up when nothing is scheduled, while waiting for a killed harness.
const IDLE: Duration = Duration::from_millis(200);
const STDERR_TAIL: usize = 4096;

/// Run rules every headless task card ends with (10.4).
const RUN_RULES: &str = "\n\n## 无头运行约定\n\n\
- 不得修改慢层对象、`tower.canvas` 与快照；不得修改 `allowed_paths` 以外的文件；未通过 `towermd finish` 不得合并。\n\
- 不得自行创建分支、工作树或拉取请求，这些由运行器和集成器管理。\n\
- 每完成一个可验证的小步就提交一次检查点。\n";
/// How to reach a person: through the interaction gateway's tools, or,
/// without an MCP channel, parked (FR-60).
const ASK_LIVE: &str = "- 需要人时调用交互网关的工具；返回 `deferred` 时结束回合，并说明在等什么。\n";
const ASK_PARKED: &str = "- 需要人时运行 `towermd ask --parked \"问题\"` 记下问题，然后结束回合：运行报告的 `status` 写 `needs_input`，并说明在等什么。\n";
const REPORT_RULE: &str = "- 结束前写出运行报告：最后一条消息只含一个符合下列 JSON Schema 的对象。报告只是自述，进度仍只认证据。\n\n";

fn run_rules(gateway: bool) -> String {
    format!("{RUN_RULES}{}{REPORT_RULE}", if gateway { ASK_LIVE } else { ASK_PARKED })
}

/// `<git-common-dir>/towermd/runs/<run>/`.
pub fn run_dir(git_common_dir: &Path, run: &str) -> PathBuf {
    git_common_dir.join("towermd").join("runs").join(run)
}

/// The one prompt template every harness gets: read the task card.
pub fn prompt(task_card: &Path) -> String {
    format!("读取任务卡 {}，按卡中的全部约定完成任务。结束前按卡中的报告要求，以 JSON 输出运行报告作为最后一条消息。",
        task_card.display())
}

/// A probed harness: the adapter, the program it runs and its version.
pub struct Harness {
    pub adapter: &'static dyn Adapter,
    pub program: PathBuf,
    pub version: String,
}
impl Harness {
    /// `<harness>/<version>`, the delegate's signature.
    pub fn agent(&self) -> String {
        harness::signature(self.adapter, &self.version)
    }
}

/// FR-25 `probe()`: run the version command and read the version.
pub async fn probe(adapter: &'static dyn Adapter, program: Option<PathBuf>) -> Result<Harness, String> {
    let program = program.unwrap_or_else(|| PathBuf::from(adapter.program()));
    let output = tokio::time::timeout(PROBE_TIMEOUT, Command::new(&program).args(adapter.version_args())
        .stdin(Stdio::null()).kill_on_drop(true).output()).await
        .map_err(|_| format!("{} 版本探测超时", adapter.id()))?
        .map_err(|error| format!("无法运行 {}：{error}", program.display()))?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let version = output.status.success().then(|| adapter.parse_version(&text)).flatten()
        .ok_or_else(|| format!("{} 没有报告可识别的版本", adapter.id()))?;
    Ok(Harness { adapter, program, version })
}

/// Where the runner's events go: the hub, through the daemon, in M3-B.
pub trait RunSink {
    fn emit(&mut self, event: NewEvent) -> Result<(), String>;
}

/// Everything one run needs, passed explicitly.
#[derive(Clone, Debug)]
pub struct RunConfig {
    pub host: String,
    pub run: String,
    pub task: String,
    pub module: String,
    pub worktree: PathBuf,
    pub run_dir: PathBuf,
    /// Task card text; the runner appends the headless run rules.
    pub task_card: String,
    pub model: Option<String>,
    pub permission: Permission,
    /// The limit `budget::admit` gave this run.
    pub budget_usd: Option<f64>,
    pub timeout: Duration,
    pub grace: Duration,
    /// Launches in total, including retries after external failures.
    pub max_attempts: u32,
    pub mcp_config: Option<PathBuf>,
    pub permission_tool: Option<String>,
    pub skill_dirs: Vec<PathBuf>,
    /// Extra environment for the harness; the rest is inherited without `GIT_*`.
    pub env: Vec<(String, String)>,
    /// FR-25: continue this parked session instead of starting afresh, when
    /// the adapter can.
    pub resume: Option<Resume>,
}

/// A parked session and what it is told when it continues.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resume {
    pub session: String,
    pub message: String,
}

fn event(host: &str, actor: &str, kind: &str, obj: &str, data: Value) -> NewEvent {
    NewEvent { eid: uuid::Uuid::now_v7().to_string(), host: host.into(), actor: actor.into(), kind: kind.into(),
        layer: "L5".into(), obj: obj.into(), data, guards: Vec::new() }
}

fn safe_segment(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.starts_with('.')
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// One harness process.
#[derive(Default)]
struct Session {
    status: Option<ExitStatus>,
    /// The process ran and was waited for.
    exited: bool,
    signalled: bool,
    stop: Option<Stop>,
    last: Option<Final>,
    message: Option<String>,
    session: Option<String>,
    tokens: Tokens,
    usage_cost: Option<f64>,
    failure: Option<(String, bool)>,
    stderr: String,
}

impl Session {
    fn cost(&self) -> Option<f64> {
        self.last.as_ref().and_then(|last| last.cost_usd).or(self.usage_cost)
    }
    fn exit_code(&self) -> Option<i32> {
        self.status.and_then(|status| status.code())
    }
    fn signal(&self) -> Option<i32> {
        #[cfg(unix)]
        { use std::os::unix::process::ExitStatusExt; self.status.and_then(|status| status.signal()) }
        #[cfg(not(unix))]
        { None }
    }
    /// The harness's last words: the final text, else the last message.
    fn last_text(&self) -> Option<&str> {
        self.last.as_ref().and_then(|last| last.text.as_deref()).filter(|text| !text.trim().is_empty())
            .or(self.message.as_deref())
    }
    /// The report, and why a report that was given is not valid.
    fn report(&self) -> (Option<RunReport>, Option<String>) {
        if let Some(value) = self.last.as_ref().and_then(|last| last.report.as_ref()) {
            return match RunReport::parse(value) {
                Ok(report) => (Some(report), None),
                Err(error) => (None, Some(error)),
            };
        }
        (self.last_text().and_then(RunReport::extract), None)
    }
    fn external(&self) -> bool {
        self.failure.as_ref().is_some_and(|(_, external)| *external)
            || self.last.as_ref().is_some_and(|last| last.external)
            || (self.exit_code() != Some(0) && harness::looks_external(&self.stderr))
    }
}

/// Run one harness to its end. Emits `run.started` and `run.finished`
/// (and `run.retry` on retries); returns what `run.finished` recorded.
/// `control` set to `true` interrupts the run.
pub async fn run(
    harness: &Harness,
    config: &RunConfig,
    sink: &mut dyn RunSink,
    mut control: watch::Receiver<bool>,
) -> Result<RunFinished, String> {
    if !safe_segment(&config.run) || !safe_segment(&config.task) {
        return Err("运行或任务 ID 不能用作目录名".into());
    }
    let capabilities = harness.adapter.capabilities();
    if config.budget_usd.is_some() && !capabilities.reports_cost {
        // Unknown cost is never counted as 0: ask a person instead of running.
        sink.emit(budget::refusal_event(&config.host, &config.task, &config.module, Refusal::Unknown,
            Budget { run_usd: config.budget_usd, module_usd: None }, Spend::default()))?;
        return Err(format!("{} 不报告费用，设了预算时不能运行", harness.adapter.id()));
    }
    std::fs::create_dir_all(&config.run_dir).map_err(|e| e.to_string())?;
    let card = config.run_dir.join(TASK_CARD);
    let schema = config.run_dir.join(SCHEMA);
    let schema_text = serde_json::to_string_pretty(&run_report::schema()).map_err(|e| e.to_string())?;
    let rules = run_rules(config.mcp_config.is_some());
    std::fs::write(&card, format!("{}{rules}```json\n{schema_text}\n```\n", config.task_card.trim_end()))
        .map_err(|e| e.to_string())?;
    std::fs::write(&schema, &schema_text).map_err(|e| e.to_string())?;
    let raw_path = config.run_dir.join(RAW);
    let agent = harness.agent();
    let spec = RunSpec {
        run_id: config.run.clone(), task_id: config.task.clone(), worktree: config.worktree.clone(),
        run_dir: config.run_dir.clone(), prompt: prompt(&card), model: config.model.clone(),
        permission: config.permission, budget_usd: config.budget_usd, output_schema: schema,
        mcp_config: config.mcp_config.clone(), permission_tool: config.permission_tool.clone(),
        skill_dirs: config.skill_dirs.clone(),
    };
    let started = RunStarted {
        run: config.run.clone(), task: config.task.clone(), agent: agent.clone(), adapter: harness.adapter.id().into(),
        budget_usd: config.budget_usd, task_card: card.display().to_string(), raw: raw_path.display().to_string(),
    };
    let mut points = Some(Checkpoints::new(&config.worktree));
    sink.emit(event(&config.host, RUNNER, STARTED, &config.task, json!(started)))?;

    let deadline = Instant::now() + config.timeout;
    let mut attempts = 0;
    let mut known = 0.0;
    let mut unknown = false;
    let mut tokens = Tokens::default();
    let mut sessions = Vec::new();
    loop {
        attempts += 1;
        let session = if *control.borrow_and_update() {
            Session { stop: Some(Stop::Interrupted), ..Session::default() }
        } else {
            launch(harness, &spec, config, &raw_path, deadline, known, &agent, sink, &mut control, &mut points).await?
        };
        match session.cost() {
            Some(cost) => known += cost,
            None => unknown |= session.exited,
        }
        tokens.add(&session.tokens);
        let failed = session.exit_code() != Some(0) || session.report().0.is_none();
        let retry = session.stop.is_none() && failed && session.external() && attempts < config.max_attempts.max(1)
            && !session.last.as_ref().is_some_and(|last| last.budget_hit);
        if retry {
            let error = session.failure.as_ref().map(|(message, _)| message.clone())
                .or_else(|| session.last.as_ref().and_then(|last| last.error.clone()));
            let retry = RunRetry { run: config.run.clone(), source: "runner".into(), attempt: attempts, error };
            sink.emit(event(&config.host, RUNNER, RETRY, &config.task, json!(retry)))?;
            sessions.push(session);
            continue;
        }
        sessions.push(session);
        break;
    }
    let last = sessions.last().expect("at least one session");
    let (report, invalid) = last.report();
    let budget_hit = last.stop == Some(Stop::Budget) || last.last.as_ref().is_some_and(|last| last.budget_hit);
    let outcome = match last.stop {
        Some(Stop::Interrupted) => Outcome::Interrupted,
        _ if budget_hit => Outcome::Budget,
        Some(Stop::Timeout) => Outcome::Failed,
        _ if last.exit_code() == Some(0) && report.is_some() => Outcome::Reported,
        _ => Outcome::Failed,
    };
    let status = match (&report, outcome) {
        (Some(report), Outcome::Reported | Outcome::Budget | Outcome::Interrupted) => report.status,
        _ => ReportStatus::Failed,
    };
    let crashed = last.signal().is_some() && !last.signalled;
    let error = (outcome != Outcome::Reported).then(|| {
        last.last.as_ref().and_then(|last| last.error.clone())
            .or_else(|| last.failure.as_ref().map(|(message, _)| message.clone()))
            .or(invalid.clone())
            .or_else(|| match last.stop {
                Some(Stop::Timeout) => Some(format!("运行超过 {} 秒", config.timeout.as_secs())),
                Some(Stop::Budget) => Some("运行达到预算上限".into()),
                Some(Stop::Interrupted) => Some("运行被中断".into()),
                None => None,
            })
            .or_else(|| (!last.stderr.trim().is_empty()).then(|| clip(&last.stderr)))
            .unwrap_or_else(|| match (last.exit_code(), last.signal()) {
                (Some(0), _) => "运行没有写出报告".into(),
                (Some(code), _) => format!("harness 以退出码 {code} 结束，没有写出报告"),
                (None, Some(signal)) => format!("harness 被信号 {signal} 终止"),
                (None, None) => "harness 没有启动".into(),
            })
    });
    let summary = report.as_ref().map(|report| report.summary.clone())
        .or_else(|| last.last_text().map(clip).filter(|text| !text.is_empty()))
        .or_else(|| error.as_deref().map(clip))
        .unwrap_or_else(|| "运行没有输出".into());
    let mut finished = RunFinished {
        run: config.run.clone(), task: config.task.clone(), agent, outcome, status,
        stop: last.stop, exit_code: last.exit_code(), signal: last.signal(), crashed,
        report: if outcome == Outcome::Failed && last.exit_code() != Some(0) { None } else { report },
        summary, error_class: None, error,
        cost_usd: (!unknown && sessions.iter().any(|s| s.exited)).then_some(known),
        tokens, session: sessions.iter().rev().find_map(|s| s.session.clone()), attempts,
        raw: raw_output(&raw_path),
    };
    if finished.report.is_none() {
        finished.status = ReportStatus::Failed;
    }
    finished.classify(last.external());
    sink.emit(event(&config.host, RUNNER, FINISHED, &config.task, json!(finished)))?;
    Ok(finished)
}

/// A read-only consultation: the controller answering a question (FR-23),
/// a cross reviewer judging a proposal (FR-34). One session, no retries, no
/// run events; the brief and the reply schema are the caller's.
#[derive(Clone, Debug)]
pub struct Consultation {
    pub host: String,
    /// Directory name and the spec's run ID.
    pub id: String,
    /// Where the harness starts; it may read, never write (`ReadOnly`).
    pub cwd: PathBuf,
    /// `<git-common-dir>/towermd/consults/<id>/`: brief, schema, raw output.
    pub dir: PathBuf,
    pub brief: String,
    pub schema: Value,
    pub model: Option<String>,
    pub timeout: Duration,
    /// Extra environment, as in `RunConfig::env`.
    pub env: Vec<(String, String)>,
}

/// What a consultation returned. `reply` is `None` when nothing valid came back.
#[derive(Clone, Debug, PartialEq)]
pub struct Consulted {
    pub agent: String,
    pub reply: Option<Value>,
    /// Unknown stays `None`, never 0.
    pub cost_usd: Option<f64>,
    pub error: Option<String>,
}

/// `<git-common-dir>/towermd/consults/<id>/`.
pub fn consult_dir(git_common_dir: &Path, id: &str) -> PathBuf {
    git_common_dir.join("towermd").join("consults").join(id)
}

struct Silent;
impl RunSink for Silent {
    fn emit(&mut self, _: NewEvent) -> Result<(), String> {
        Ok(())
    }
}

/// The last JSON object in a reply's text, for harnesses without structured output.
fn json_object(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str::<Value>(text.get(start..=end)?).ok().filter(Value::is_object)
}

pub async fn consult(harness: &Harness, consultation: &Consultation) -> Result<Consulted, String> {
    if !safe_segment(&consultation.id) {
        return Err("咨询 ID 不能用作目录名".into());
    }
    let dir = &consultation.dir;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let card = dir.join(TASK_CARD);
    let schema = dir.join(SCHEMA);
    let schema_text = serde_json::to_string_pretty(&consultation.schema).map_err(|e| e.to_string())?;
    std::fs::write(&card, format!("{}\n\n只读，不改任何文件。最后一条消息只含一个符合下列 JSON Schema 的对象：\n\n```json\n{schema_text}\n```\n",
        consultation.brief.trim_end())).map_err(|e| e.to_string())?;
    std::fs::write(&schema, &schema_text).map_err(|e| e.to_string())?;
    let config = RunConfig {
        host: consultation.host.clone(), run: consultation.id.clone(), task: consultation.id.clone(), module: String::new(),
        worktree: consultation.cwd.clone(), run_dir: dir.clone(), task_card: String::new(), model: consultation.model.clone(),
        permission: Permission::ReadOnly, budget_usd: None, timeout: consultation.timeout, grace: TERM_GRACE,
        max_attempts: 1, mcp_config: None, permission_tool: None, skill_dirs: Vec::new(), env: consultation.env.clone(),
        resume: None,
    };
    let spec = RunSpec {
        run_id: config.run.clone(), task_id: config.task.clone(), worktree: config.worktree.clone(), run_dir: dir.clone(),
        prompt: format!("读取 {}，按其中要求作答，最后一条消息只输出要求的 JSON 对象。", card.display()),
        model: config.model.clone(), permission: Permission::ReadOnly, budget_usd: None, output_schema: schema,
        mcp_config: None, permission_tool: None, skill_dirs: Vec::new(),
    };
    let agent = harness.agent();
    let (_control, mut receiver) = watch::channel(false);
    let session = launch(harness, &spec, &config, &dir.join(RAW), Instant::now() + consultation.timeout, 0.0, &agent,
        &mut Silent, &mut receiver, &mut None).await?;
    let reply = session.last.as_ref().and_then(|last| last.report.clone()).filter(Value::is_object)
        .or_else(|| session.last_text().and_then(json_object));
    let error = if session.stop == Some(Stop::Timeout) {
        Some(format!("咨询超过 {} 秒", consultation.timeout.as_secs()))
    } else if reply.is_none() {
        Some(session.last.as_ref().and_then(|last| last.error.clone())
            .or_else(|| session.failure.as_ref().map(|(message, _)| message.clone()))
            .unwrap_or_else(|| "没有给出可读的 JSON 回复".into()))
    } else {
        None
    };
    // A timed-out session's half reply is not a reply.
    let reply = if session.stop.is_some() { None } else { reply };
    Ok(Consulted { agent, reply, cost_usd: session.cost(), error })
}

fn raw_output(path: &Path) -> Option<RawOutput> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 { break }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Some(RawOutput { path: path.display().to_string(), sha256: hex::encode(hasher.finalize()), bytes })
}

/// 6.3 `run.checkpoint`: commits the harness makes in the worktree, noticed
/// while its process is live. A worktree without git yields none.
struct Checkpoints {
    worktree: PathBuf,
    head: Option<String>,
    seen: std::collections::BTreeSet<String>,
    due: Instant,
}

const CHECKPOINT_EVERY: Duration = Duration::from_secs(2);
const CHECKPOINTS_PER_SCAN: usize = 50;

impl Checkpoints {
    fn new(worktree: &Path) -> Self {
        Checkpoints { worktree: worktree.to_path_buf(), head: Self::head(worktree), seen: Default::default(),
            due: Instant::now() + CHECKPOINT_EVERY }
    }

    fn head(worktree: &Path) -> Option<String> {
        towermd_core::repo::git_timeout(worktree, &["rev-parse", "HEAD"], Duration::from_secs(5)).ok()
            .map(|head| head.trim().to_owned())
    }

    /// Emit one `run.checkpoint` per commit since the last scan, oldest first.
    fn scan(&mut self, config: &RunConfig, sink: &mut dyn RunSink, force: bool) {
        let now = Instant::now();
        if !force && now < self.due {
            return;
        }
        self.due = now + CHECKPOINT_EVERY;
        let Some(head) = Self::head(&self.worktree) else { return };
        if self.head.as_deref() == Some(head.as_str()) {
            return;
        }
        let range = match &self.head {
            Some(last) => format!("{last}..{head}"),
            None => head.clone(),
        };
        let listed = towermd_core::repo::git_timeout(&self.worktree, &["rev-list", "--reverse", &range], Duration::from_secs(5))
            .unwrap_or_default();
        let commits: Vec<&str> = listed.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
        for commit in commits.iter().skip(commits.len().saturating_sub(CHECKPOINTS_PER_SCAN)) {
            if !self.seen.insert((*commit).to_owned()) {
                continue;
            }
            let point = RunCheckpoint { run: config.run.clone(), task: config.task.clone(), commit: (*commit).to_owned() };
            // An observation, not the run's result: a refused mark never ends the run.
            let _ = sink.emit(event(&config.host, RUNNER, CHECKPOINT, &config.task, json!(point)));
        }
        self.head = Some(head);
    }
}

enum Tick {
    Line(Option<Vec<u8>>),
    Exit(std::io::Result<ExitStatus>),
    Control(Option<bool>),
    Wake,
}

#[derive(Clone, Copy)]
enum Phase {
    Int,
    Term,
    Kill,
}

/// Send a signal to the harness's process group with the system `kill`.
fn kill_group(pid: Option<u32>, signal: &str) -> bool {
    let Some(pid) = pid else { return false };
    if !cfg!(unix) {
        return false;
    }
    std::process::Command::new("kill").args(["-s", signal, "--", &format!("-{pid}")])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()
        .is_ok_and(|status| status.success())
}

#[allow(clippy::too_many_arguments)]
async fn launch(
    harness: &Harness,
    spec: &RunSpec,
    config: &RunConfig,
    raw_path: &Path,
    deadline: Instant,
    spent: f64,
    agent: &str,
    sink: &mut dyn RunSink,
    control: &mut watch::Receiver<bool>,
    points: &mut Option<Checkpoints>,
) -> Result<Session, String> {
    let mut session = Session::default();
    // FR-25: a parked session continues with the message, then the card.
    let launch = config.resume.as_ref()
        .and_then(|resume| harness.adapter.resume(spec, &resume.session, &format!("{}\n\n{}", resume.message.trim_end(), spec.prompt)))
        .unwrap_or_else(|| harness.adapter.launch(spec));
    let mut command = Command::new(&harness.program);
    command.args(&launch.args).current_dir(&config.worktree)
        .stdin(if launch.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command.envs(config.env.iter().map(|(key, value)| (key, value)));
    #[cfg(unix)]
    command.process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            session.failure = Some((format!("无法启动 {}：{error}", harness.program.display()), false));
            return Ok(session);
        }
    };
    let pid = child.id();
    if let (Some(input), Some(mut stdin)) = (launch.stdin, child.stdin.take()) {
        tokio::spawn(async move {
            let _ = stdin.write_all(input.as_bytes()).await;
        });
    }
    let stderr_path = config.run_dir.join(STDERR);
    let stderr_task = child.stderr.take().map(|mut stderr| tokio::spawn(async move {
        let mut tail = Vec::new();
        // Small appends to a local file; tokio's `fs` feature is not enabled.
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(stderr_path).ok();
        let mut buffer = vec![0; 8192];
        while let Ok(read) = stderr.read(&mut buffer).await {
            if read == 0 { break }
            if let Some(file) = file.as_mut() {
                let _ = std::io::Write::write_all(file, &buffer[..read]);
            }
            tail.extend_from_slice(&buffer[..read]);
            if tail.len() > STDERR_TAIL {
                tail.drain(..tail.len() - STDERR_TAIL);
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    }));
    let mut raw = std::io::BufWriter::new(std::fs::OpenOptions::new().create(true).append(true).open(raw_path)
        .map_err(|e| e.to_string())?);
    // Raw bytes per line: a stray invalid byte must not end the stream.
    let mut lines = BufReader::new(child.stdout.take().ok_or("harness 没有标准输出")?).split(b'\n');
    let mut eof = false;
    let mut control_open = true;
    let mut stopping: Option<(Phase, Instant)> = None;
    let mut drain: Option<Instant> = None;
    let mut delta = String::new();
    loop {
        if eof && session.exited {
            break;
        }
        let wake = [stopping.map(|(_, at)| at), session.stop.is_none().then_some(deadline), drain]
            .into_iter().flatten().min().unwrap_or_else(|| Instant::now() + IDLE);
        let tick = tokio::select! {
            line = lines.next_segment(), if !eof => Tick::Line(line.ok().flatten()),
            status = child.wait(), if !session.exited => Tick::Exit(status),
            changed = control.changed(), if control_open && session.stop.is_none() => {
                Tick::Control(changed.ok().map(|_| *control.borrow_and_update()))
            }
            _ = sleep_until(wake) => Tick::Wake,
        };
        let mut request = None;
        match tick {
            Tick::Line(None) => eof = true,
            Tick::Line(Some(bytes)) => {
                raw.write_all(&bytes).and_then(|_| raw.write_all(b"\n")).map_err(|e| e.to_string())?;
                let line = String::from_utf8_lossy(&bytes);
                let line = line.trim_end_matches('\r');
                for signal in harness.adapter.parse(line) {
                    match signal {
                        Signal::Session(id) => session.session = Some(id),
                        Signal::Message(text) => { delta.clear(); session.message = Some(text) }
                        Signal::Delta(text) => { delta.push_str(&text); session.message = Some(delta.clone()) }
                        Signal::Retry { attempt, error } => {
                            let retry = RunRetry { run: config.run.clone(), source: "harness".into(), attempt, error };
                            sink.emit(event(&config.host, agent, RETRY, &config.task, json!(retry)))?;
                        }
                        Signal::Usage { tokens, cost_usd } => {
                            session.tokens.add(&tokens);
                            if let Some(cost) = cost_usd {
                                session.usage_cost = Some(session.usage_cost.unwrap_or(0.0) + cost);
                            }
                        }
                        Signal::Failure { message, external } => session.failure = Some((message, external)),
                        Signal::Final(last) => {
                            if let Some(tokens) = &last.tokens {
                                session.tokens = tokens.clone();
                            }
                            session.last = Some(last);
                        }
                    }
                }
                if let (Some(budget), Some(cost)) = (spec.budget_usd, session.cost()) {
                    if spent + cost >= budget && !session.exited {
                        request = Some(Stop::Budget);
                    }
                }
            }
            Tick::Exit(status) => {
                session.status = status.ok();
                session.exited = true;
                drain = Some(Instant::now() + DRAIN);
            }
            Tick::Control(None) => control_open = false,
            Tick::Control(Some(true)) => request = Some(Stop::Interrupted),
            Tick::Control(Some(false)) => {}
            Tick::Wake => {
                let now = Instant::now();
                if drain.is_some_and(|at| now >= at) {
                    // Something left behind still holds the output open.
                    kill_group(pid, "KILL");
                    break;
                }
                match stopping {
                    Some((Phase::Int, at)) if now >= at => {
                        kill_group(pid, "TERM");
                        stopping = Some((Phase::Term, now + TERM_GRACE));
                    }
                    Some((Phase::Term, at)) if now >= at => {
                        if !kill_group(pid, "KILL") {
                            let _ = child.start_kill();
                        }
                        stopping = Some((Phase::Kill, now + TERM_GRACE));
                    }
                    Some((Phase::Kill, at)) if now >= at => {
                        let _ = child.start_kill();
                        stopping = None;
                    }
                    None if session.stop.is_none() && now >= deadline => request = Some(Stop::Timeout),
                    _ => {}
                }
            }
        }
        if let Some(points) = points.as_mut() {
            points.scan(config, sink, false);
        }
        if let Some(stop) = request.filter(|_| session.stop.is_none() && !session.exited) {
            session.stop = Some(stop);
            session.signalled = true;
            if !kill_group(pid, "INT") {
                let _ = child.start_kill();
            }
            stopping = Some((Phase::Int, Instant::now() + config.grace));
        }
    }
    // Reap whatever the harness left in its process group.
    kill_group(pid, "KILL");
    if let Some(points) = points.as_mut() {
        points.scan(config, sink, true);
    }
    if let Some(task) = stderr_task {
        session.stderr = tokio::time::timeout(DRAIN, task).await.ok().and_then(Result::ok).unwrap_or_default();
    }
    raw.flush().map_err(|e| e.to_string())?;
    Ok(session)
}
