//! End-to-end runner tests against the mock harnesses in
//! `tests/fixtures/harness/`. They print the same shape of output as the real
//! CLIs; the real harnesses are never run. Every event the runner emits is
//! applied to a devkit projection, so the reducer checks them too.
#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::watch;
use towermd_core::hub::{Event, Fence};
use towermd_core::hub_protocol::NewEvent;
use towermd_devkit::dev_state::{self, tasks};
use towermd_devkit::harness::{self, Permission};
use towermd_devkit::run_record::{self, BUDGET_EXHAUSTED, BUDGET_UNKNOWN, RUN_ERROR};
use towermd_devkit::run_report::{ErrorClass, Outcome, ReportStatus, RunFinished, Stop, FINISHED, RETRY, RUNNER, STARTED};
use towermd_devkit::runner::{self, Harness, RunConfig, RunSink};
use towermd_devkit::task_model::{Run, RunBinding, RunState, Task, TaskDraft, TaskEvent, TaskOrigin, TaskState};

/// A one-task projection that takes the runner's events in order.
struct Journal {
    state: Value,
    seq: u64,
    events: Vec<NewEvent>,
}

impl Journal {
    fn new() -> Self {
        let mut journal = Self { state: dev_state::initial(), seq: 0, events: Vec::new() };
        journal.state["Baseline"]["B0"] = json!({"type":"Baseline","id":"B0"});
        let input = TaskDraft { id: "T-1".into(), title: "T-1".into(), module: "A".into(),
            allowed_paths: vec!["src/a/**".into()], acceptance: vec!["first".into()], ..TaskDraft::default() };
        let task = Task { input, origin: TaskOrigin::Cli, baseline: "towermd/B0".into(), state: TaskState::Ready,
            needs_review: false, active_run: None, gate_failures: 0, interrupt: None };
        journal.apply("process:towermd-cli", "task.created", "L4", "T-1", json!(TaskEvent::Created { task }), vec![]).unwrap();
        let fences = vec![Fence { name: "module:A".into(), holder: "R-1".into(), epoch: 1 }];
        let run = Run { binding: RunBinding { id: "R-1".into(), host: "h".into(), worktree: "/tmp/R-1".into(),
                branch: "t/T-1".into(), self_reported: None },
            task: "T-1".into(), module: "A".into(), baseline: "towermd/B0".into(), fences: fences.clone(),
            state: RunState::Active, cost_usd: None, high_risk: false, dispatch: None };
        journal.apply("process:towermd-cli", "task.claimed", "L4", "T-1", json!(TaskEvent::Claimed { run }), fences).unwrap();
        journal
    }
    fn apply(&mut self, actor: &str, kind: &str, layer: &str, obj: &str, data: Value, guards: Vec<Fence>) -> Result<(), String> {
        self.seq += 1;
        let event: Event = serde_json::from_value(json!({"seq":self.seq,"eid":format!("e-{}",self.seq),"host":"h",
            "t":format!("2026-10-24T10:{:02}:00Z",self.seq),"actor":actor,"type":kind,"layer":layer,"obj":obj,
            "data":data,"epoch":null,"stale":false,"cost_usd":null,"guards":guards})).unwrap();
        let mut next = self.state.clone();
        dev_state::apply(&mut next, &event)?;
        self.state = next;
        Ok(())
    }
    fn kinds(&self) -> Vec<(&str, &str)> {
        self.events.iter().map(|event| (event.kind.as_str(), event.actor.as_str())).collect()
    }
    fn task(&self) -> TaskState {
        tasks(&self.state).unwrap().tasks["T-1"].state
    }
    fn alerts(&self) -> Vec<String> {
        self.state["Alert"].as_object().unwrap().values()
            .map(|alert| alert["data"]["reason"].as_str().unwrap().to_owned()).collect()
    }
}

impl RunSink for Journal {
    fn emit(&mut self, event: NewEvent) -> Result<(), String> {
        let NewEvent { actor, kind, layer, obj, data, guards, .. } = event.clone();
        self.events.push(event);
        self.apply(&actor, &kind, &layer, &obj, data, guards)
    }
}

fn mock(script: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/harness").join(format!("{script}.sh"))
}

async fn probe(id: &str) -> Harness {
    runner::probe(harness::find(id).unwrap(), Some(mock(id))).await.unwrap()
}

fn config(dir: &Path, mode: &str) -> RunConfig {
    let path = |name: &str| dir.join(name).display().to_string();
    RunConfig {
        host: "h".into(), run: "R-1".into(), task: "T-1".into(), module: "A".into(), worktree: dir.to_path_buf(),
        run_dir: runner::run_dir(&dir.join(".git"), "R-1"), task_card: "# T-1\n\n改 src/a。\n".into(),
        model: None, permission: Permission::WorkspaceWrite, budget_usd: None, timeout: Duration::from_secs(30),
        grace: Duration::from_secs(5), max_attempts: 2, mcp_config: None, permission_tool: None, skill_dirs: vec![],
        env: vec![("MOCK_MODE".into(), mode.into()), ("MOCK_ARGS".into(), path("args")),
            ("MOCK_STATE".into(), path("state")), ("MOCK_PROMPT".into(), path("prompt"))],
    }
}

async fn run(harness: &Harness, config: &RunConfig, journal: &mut Journal, interrupt_after: Option<Duration>) -> RunFinished {
    let (sender, control) = watch::channel(false);
    if let Some(delay) = interrupt_after {
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = sender.send(true);
        });
        runner::run(harness, config, journal, control).await.unwrap()
    } else {
        let finished = runner::run(harness, config, journal, control).await.unwrap();
        drop(sender);
        finished
    }
}

#[tokio::test]
async fn a_reported_run_records_cost_tokens_and_raw_output_and_moves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let harness = probe("claude-code").await;
    assert_eq!(harness.agent(), "claude-code/2.1.308");
    let config = config(dir.path(), "done");
    let mut journal = Journal::new();
    let finished = run(&harness, &config, &mut journal, None).await;
    assert_eq!((finished.outcome, finished.status, finished.cost_usd), (Outcome::Reported, ReportStatus::Done, Some(0.25)));
    assert_eq!((finished.tokens.input, finished.tokens.cache_read), (Some(10), Some(2)));
    assert_eq!((finished.session.as_deref(), finished.attempts, finished.error_class), (Some("s-1"), 1, None));
    assert_eq!(journal.kinds(), [(STARTED, RUNNER), (FINISHED, RUNNER)]);
    assert_eq!(journal.task(), TaskState::Running, "报告 done 不是证据");
    assert_eq!(tasks(&journal.state).unwrap().runs["R-1"].cost_usd, Some(0.25));

    let card = std::fs::read_to_string(config.run_dir.join(runner::TASK_CARD)).unwrap();
    assert!(card.starts_with("# T-1") && card.contains("无头运行约定") && card.contains("\"suspected_class\""));
    // FR-60: without an MCP channel the delegate parks its question and ends the turn.
    assert!(card.contains("towermd ask --parked") && card.contains("needs_input") && !card.contains("交互网关"));
    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    assert!(args.starts_with("--bare\n") && args.contains(&config.run_dir.join(runner::TASK_CARD).display().to_string()));
    let raw = finished.raw.unwrap();
    let text = std::fs::read_to_string(&raw.path).unwrap();
    assert_eq!((text.lines().count(), raw.bytes), (3, text.len() as u64));
    assert!(raw.path.ends_with("towermd/runs/R-1/output.jsonl"));
}

#[tokio::test]
async fn no_report_fails_the_task_with_the_last_message_as_summary() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Journal::new();
    let finished = run(&probe("claude-code").await, &config(dir.path(), "noreport"), &mut journal, None).await;
    assert_eq!((finished.outcome, finished.status, finished.report.is_none()), (Outcome::Failed, ReportStatus::Failed, true));
    assert_eq!(finished.summary, "做了一半，没写报告");
    assert_eq!((finished.error.as_deref(), finished.error_class), (Some("运行没有写出报告"), Some(ErrorClass::Execution)));
    assert_eq!(journal.task(), TaskState::Failed);
    assert!(journal.alerts().is_empty());
}

#[tokio::test]
async fn an_external_failure_is_retried_within_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let harness = probe("claude-code").await;
    let mut journal = Journal::new();
    let finished = run(&harness, &config(dir.path(), "external"), &mut journal, None).await;
    assert_eq!((finished.outcome, finished.attempts), (Outcome::Reported, 2));
    assert!((finished.cost_usd.unwrap() - 0.30).abs() < 1e-9, "两次会话的费用相加");
    let agent = harness.agent();
    assert_eq!(journal.kinds(), [(STARTED, RUNNER), (RETRY, agent.as_str()), (RETRY, RUNNER), (FINISHED, RUNNER)]);
    assert_eq!(run_record::records(&journal.state).unwrap()["R-1"].retries, 2);
    assert_eq!(journal.task(), TaskState::Running);
}

#[tokio::test]
async fn a_design_suspicion_raises_an_error_interrupt_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Journal::new();
    let finished = run(&probe("claude-code").await, &config(dir.path(), "design"), &mut journal, None).await;
    assert_eq!((finished.outcome, finished.status, finished.error_class), (Outcome::Reported, ReportStatus::Failed, Some(ErrorClass::Design)));
    assert_eq!(finished.report.unwrap().proposal.unwrap().kind, "interface");
    assert_eq!(journal.task(), TaskState::Running);
    assert_eq!(journal.alerts(), [RUN_ERROR]);
}

#[tokio::test]
async fn the_harness_budget_stop_holds_the_task() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path(), "budget");
    config.budget_usd = Some(1.0);
    let mut journal = Journal::new();
    let finished = run(&probe("claude-code").await, &config, &mut journal, None).await;
    assert_eq!((finished.outcome, finished.cost_usd, finished.error_class), (Outcome::Budget, Some(1.2), None));
    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    assert!(args.contains("--max-budget-usd\n1\n"));
    assert_eq!(journal.task(), TaskState::Held);
    assert_eq!(journal.alerts(), [BUDGET_EXHAUSTED]);
}

#[tokio::test]
async fn a_crash_holds_the_task_with_an_error_interrupt() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Journal::new();
    let finished = run(&probe("claude-code").await, &config(dir.path(), "crash"), &mut journal, None).await;
    assert!(finished.crashed);
    assert_eq!((finished.signal, finished.outcome, finished.session.as_deref()), (Some(11), Outcome::Failed, Some("s-1")));
    assert_eq!(journal.task(), TaskState::Held);
    assert_eq!(journal.alerts(), [RUN_ERROR]);
}

#[tokio::test]
async fn an_interrupt_sends_sigint_and_the_harness_ends_its_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Journal::new();
    let started = std::time::Instant::now();
    let finished = run(&probe("claude-code").await, &config(dir.path(), "hang"), &mut journal, Some(Duration::from_millis(300))).await;
    assert!(started.elapsed() < Duration::from_secs(4), "SIGINT 后不必等宽限期");
    assert_eq!((finished.outcome, finished.stop, finished.exit_code), (Outcome::Interrupted, Some(Stop::Interrupted), Some(130)));
    assert_eq!((finished.crashed, finished.cost_usd), (false, Some(0.3)));
    assert_eq!(journal.task(), TaskState::Running);
}

#[tokio::test]
async fn a_harness_ignoring_sigint_gets_sigterm_after_the_grace_period() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path(), "stubborn");
    config.grace = Duration::from_millis(300);
    let mut journal = Journal::new();
    let finished = run(&probe("claude-code").await, &config, &mut journal, Some(Duration::from_millis(200))).await;
    assert_eq!((finished.outcome, finished.signal, finished.crashed), (Outcome::Interrupted, Some(15), false));
    assert_eq!(finished.cost_usd, None, "没有报告费用的会话不能记为 0");
    assert_eq!(journal.task(), TaskState::Running);
}

#[tokio::test]
async fn a_timeout_stops_the_harness_and_fails_the_task() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path(), "hang");
    config.timeout = Duration::from_millis(500);
    let mut journal = Journal::new();
    let finished = run(&probe("claude-code").await, &config, &mut journal, None).await;
    assert_eq!((finished.outcome, finished.stop, finished.attempts), (Outcome::Failed, Some(Stop::Timeout), 1));
    assert_eq!(journal.task(), TaskState::Failed);
}

#[tokio::test]
async fn codex_reads_the_prompt_on_stdin_and_reports_no_cost() {
    let dir = tempfile::tempdir().unwrap();
    let harness = probe("codex").await;
    assert_eq!(harness.agent(), "codex/0.163.0");
    let config = config(dir.path(), "done");
    let mut journal = Journal::new();
    let finished = run(&harness, &config, &mut journal, None).await;
    assert_eq!((finished.outcome, finished.status, finished.cost_usd), (Outcome::Reported, ReportStatus::Blocked, None));
    assert_eq!((finished.tokens.input, finished.session.as_deref()), (Some(9), Some("th-1")));
    assert!(std::fs::read_to_string(dir.path().join("prompt")).unwrap().starts_with("读取任务卡 "));
    assert!(std::fs::read_to_string(dir.path().join("args")).unwrap().starts_with("exec\n--json\n"));
    assert!(config.run_dir.join(runner::SCHEMA).is_file());
    assert_eq!(tasks(&journal.state).unwrap().runs["R-1"].cost_usd, None);
    assert_eq!(journal.task(), TaskState::Running);
    assert!(journal.alerts().is_empty(), "没设预算时未知费用不打扰");
}

#[tokio::test]
async fn a_budget_on_a_harness_without_cost_asks_a_person_before_running() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path(), "done");
    config.budget_usd = Some(1.0);
    let mut journal = Journal::new();
    let (_sender, control) = watch::channel(false);
    let error = runner::run(&probe("codex").await, &config, &mut journal, control).await.unwrap_err();
    assert!(error.contains("不报告费用"), "{error}");
    assert_eq!(journal.kinds(), [("alert.raised", RUNNER)]);
    assert_eq!(journal.alerts(), [BUDGET_UNKNOWN]);
    assert!(!dir.path().join("args").exists(), "没有启动 harness");
    assert_eq!(journal.task(), TaskState::Running);
}

#[tokio::test]
async fn gemini_deltas_build_the_report_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Journal::new();
    let finished = run(&probe("gemini").await, &config(dir.path(), "done"), &mut journal, None).await;
    assert_eq!((finished.outcome, finished.status, finished.cost_usd), (Outcome::Reported, ReportStatus::NeedsInput, None));
    assert_eq!(finished.report.unwrap().questions, ["用哪个库？"]);
}

#[tokio::test]
async fn probing_a_missing_program_fails_and_names_the_harness() {
    let error = runner::probe(harness::find("cursor").unwrap(), Some(PathBuf::from("/nonexistent/agent"))).await.err().unwrap();
    assert!(error.contains("/nonexistent/agent"), "{error}");
}
