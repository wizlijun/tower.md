use super::*;
use crate::run_record::{records, BUDGET_EXHAUSTED, BUDGET_UNKNOWN, GROUP, RUN_ERROR};
use crate::run_report::{ErrorClass, Outcome, ReportStatus, RunFinished, RunReport, Stop, Tokens, RUNNER};

const AGENT: &str = "mock/1.0";

fn started(journal: &mut Journal, task: &str, run: &str, budget: Option<f64>) -> Result<Event, String> {
    journal.send("run.started", RUNNER, "L5", task, json!({"run":run,"task":task,"agent":AGENT,"adapter":"mock",
        "budget_usd":budget,"task_card":"/g/towermd/runs/R/task.md","raw":"/g/towermd/runs/R/output.jsonl"}))
}

fn report(status: ReportStatus, class: Option<ErrorClass>) -> RunReport {
    RunReport { status, summary: "说明".into(), checkpoints: vec![], changed_paths: vec![], questions: vec![],
        suspected_class: class, proposal: None }
}

fn outcome(task: &str, run: &str, outcome: Outcome, report: Option<RunReport>) -> RunFinished {
    let reported = outcome == Outcome::Reported;
    let mut finished = RunFinished { run: run.into(), task: task.into(), agent: AGENT.into(), outcome,
        status: report.as_ref().map_or(ReportStatus::Failed, |r| r.status),
        stop: match outcome { Outcome::Budget => Some(Stop::Budget), Outcome::Interrupted => Some(Stop::Interrupted), _ => None },
        exit_code: Some(if reported { 0 } else { 1 }), signal: None, crashed: false, report, summary: "说明".into(),
        error_class: None, error: (!reported).then(|| "坏了".into()), cost_usd: Some(0.5), tokens: Tokens::default(),
        session: None, attempts: 1, raw: None };
    finished.classify(false);
    finished
}

fn finish(journal: &mut Journal, data: &RunFinished) -> Result<Event, String> {
    journal.send("run.finished", RUNNER, "L5", &data.task.clone(), json!(data))
}

fn state_of(journal: &Journal, task: &str) -> TaskState {
    tasks(&journal.state).unwrap().tasks[task].state
}

fn open_alerts(journal: &Journal) -> Vec<(String, String)> {
    journal.state["Alert"].as_object().unwrap().values().filter(|alert| alert["resolved"] == false)
        .map(|alert| (alert["data"]["reason"].as_str().unwrap().to_owned(), alert["data"]["detail"]["task"].as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn a_report_is_recorded_and_moves_nothing() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.claim("T-1", "R-1", None).unwrap();
    started(&mut journal, "T-1", "R-1", None).unwrap();
    assert!(started(&mut journal, "T-1", "R-1", None).unwrap_err().contains("活动进程"));
    finish(&mut journal, &outcome("T-1", "R-1", Outcome::Reported, Some(report(ReportStatus::Done, None)))).unwrap();
    assert_eq!(state_of(&journal, "T-1"), TaskState::Running, "报告 done 不是证据");
    let record = &records(&journal.state).unwrap()["R-1"];
    assert_eq!((record.starts, record.failures, record.known_cost_usd, record.running.is_none()), (1, 0, 0.5, true));
    assert_eq!(tasks(&journal.state).unwrap().runs["R-1"].cost_usd, Some(0.5));
    // A blocked or needs_input report is also only a report.
    started(&mut journal, "T-1", "R-1", None).unwrap();
    finish(&mut journal, &outcome("T-1", "R-1", Outcome::Reported, Some(report(ReportStatus::NeedsInput, None)))).unwrap();
    assert_eq!(state_of(&journal, "T-1"), TaskState::Running);
    assert!(open_alerts(&journal).is_empty());
    // A finish request still goes through the normal path, the cost included.
    let run = tasks(&journal.state).unwrap().runs["R-1"].clone();
    assert_eq!(run.cost_usd, Some(1.0));
    journal.finish(&run, "E-1", vec![]).unwrap();
    assert_eq!(state_of(&journal, "T-1"), TaskState::Verifying);
}

#[test]
fn runner_events_need_the_runner_a_live_run_and_a_consistent_record() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.create(draft("T-2"));
    assert!(started(&mut journal, "T-1", "R-1", None).is_err(), "未领取");
    journal.claim("T-1", "R-1", None).unwrap();
    let data = json!({"run":"R-1","task":"T-1","agent":AGENT,"adapter":"mock","budget_usd":null,"task_card":"c","raw":"r"});
    for (actor, layer, obj) in [("claude-code/2", "L5", "T-1"), (RUNNER, "L4", "T-1"), (RUNNER, "L5", "T-2")] {
        assert!(journal.send("run.started", actor, layer, obj, data.clone()).is_err(), "{actor} {layer} {obj}");
    }
    let done = outcome("T-1", "R-1", Outcome::Reported, Some(report(ReportStatus::Done, None)));
    assert!(finish(&mut journal, &done).unwrap_err().contains("运行开始"));
    started(&mut journal, "T-1", "R-1", None).unwrap();
    let mut lying = done.clone();
    lying.exit_code = Some(1);
    assert!(finish(&mut journal, &lying).is_err(), "退出码非零不能算报告");
    let mut no_report = done.clone();
    no_report.report = None;
    assert!(finish(&mut journal, &no_report).is_err(), "无报告时状态必须 failed");
    // Retries: the harness's own under its signature, the runner's under the runner.
    let retry = |source: &str| json!({"run":"R-1","source":source,"attempt":1,"error":"rate_limit"});
    journal.send("run.retry", AGENT, "L5", "T-1", retry("harness")).unwrap();
    journal.send("run.retry", RUNNER, "L5", "T-1", retry("runner")).unwrap();
    assert!(journal.send("run.retry", AGENT, "L5", "T-1", retry("runner")).is_err());
    assert!(journal.send("run.retry", "other/1", "L5", "T-1", retry("harness")).is_err());
    assert_eq!(records(&journal.state).unwrap()["R-1"].retries, 2);
    // Checkpoints (6.3): the runner's, a full commit ID, once each, only while live.
    let oid = "a".repeat(40);
    let point = |commit: &str| json!({"run":"R-1","task":"T-1","commit":commit});
    assert!(journal.send("run.checkpoint", AGENT, "L5", "T-1", point(&oid)).is_err(), "须运行器署名");
    assert!(journal.send("run.checkpoint", RUNNER, "L5", "T-1", point("abc123")).is_err(), "须完整提交号");
    journal.send("run.checkpoint", RUNNER, "L5", "T-1", point(&oid)).unwrap();
    assert!(journal.send("run.checkpoint", RUNNER, "L5", "T-1", point(&oid)).is_err(), "同一提交只记一次");
    let marks = &records(&journal.state).unwrap()["R-1"].checkpoints;
    assert_eq!(marks.iter().map(|mark| mark.commit.as_str()).collect::<Vec<_>>(), [oid.as_str()]);
    // The harness process: the runner's, a plain program name, replaced by a retry's.
    let process = |pid: u32, program: &str| json!({"run":"R-1","task":"T-1","pid":pid,"program":program});
    assert!(journal.send("run.process", AGENT, "L5", "T-1", process(4242, "claude")).is_err(), "须运行器署名");
    assert!(journal.send("run.process", RUNNER, "L5", "T-1", process(1, "claude")).is_err());
    assert!(journal.send("run.process", RUNNER, "L5", "T-1", process(4242, "/bin/sh")).is_err(), "只记程序名");
    journal.send("run.process", RUNNER, "L5", "T-1", process(4242, "claude")).unwrap();
    journal.send("run.process", RUNNER, "L5", "T-1", process(4343, "claude")).unwrap();
    assert_eq!(records(&journal.state).unwrap()["R-1"].process.as_ref().map(|mark| mark.pid), Some(4343));
    finish(&mut journal, &done).unwrap();
    assert!(journal.send("run.checkpoint", RUNNER, "L5", "T-1", point(&"b".repeat(40))).is_err(), "进程结束后不再记");
    assert_eq!(records(&journal.state).unwrap()["R-1"].process, None, "运行结束即清除进程");
    assert!(journal.send("run.process", RUNNER, "L5", "T-1", process(4444, "claude")).is_err());
}

#[test]
fn failures_make_the_task_failed_then_held_on_the_third() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    for (round, run) in ["R-1", "R-2", "R-3"].into_iter().enumerate() {
        let claimed = journal.claim("T-1", run, None).unwrap();
        started(&mut journal, "T-1", run, None).unwrap();
        finish(&mut journal, &outcome("T-1", run, Outcome::Failed, None)).unwrap();
        if round < 2 {
            assert_eq!(state_of(&journal, "T-1"), TaskState::Failed);
            assert!(open_alerts(&journal).is_empty());
            assert!(journal.finish(&claimed, &format!("E-{run}"), vec![]).is_err(), "失败的任务不能 finish");
            // Releasing the run is the way back to ready (Failed -> Ready).
            let event = journal.event("task.released", "process:towermd-cli", "L4", "T-1",
                json!(TaskEvent::Released { run_id: run.into(), reason: ReleaseReason::Voluntary }), &claimed.fences);
            journal.apply(&event).unwrap();
            assert_eq!(state_of(&journal, "T-1"), TaskState::Ready);
        }
    }
    assert_eq!(state_of(&journal, "T-1"), TaskState::Held);
    assert_eq!(open_alerts(&journal), [(RUN_ERROR.to_owned(), "T-1".to_owned())]);
    assert_eq!(crate::run_record::task_failures(&records(&journal.state).unwrap(), "T-1"), 3);
}

#[test]
fn a_crash_holds_at_once_and_a_design_suspicion_only_raises_an_error() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.create(draft("T-2"));
    journal.claim("T-1", "R-1", None).unwrap();
    started(&mut journal, "T-1", "R-1", None).unwrap();
    let mut crash = outcome("T-1", "R-1", Outcome::Failed, None);
    (crash.crashed, crash.exit_code, crash.signal) = (true, None, Some(11));
    finish(&mut journal, &crash).unwrap();
    assert_eq!(state_of(&journal, "T-1"), TaskState::Held);

    let mut other = journal.run("T-2", "R-2", None);
    other.binding.worktree = "/tmp/other".into();
    let event = journal.event("task.claimed", "process:towermd-cli", "L4", "T-2", json!(TaskEvent::Claimed { run: other.clone() }), &other.fences);
    journal.apply(&event).unwrap();
    started(&mut journal, "T-2", "R-2", None).unwrap();
    let design = outcome("T-2", "R-2", Outcome::Reported, Some(report(ReportStatus::Failed, Some(ErrorClass::Design))));
    assert_eq!(design.error_class, Some(ErrorClass::Design));
    finish(&mut journal, &design).unwrap();
    assert_eq!(state_of(&journal, "T-2"), TaskState::Running, "自述不改变状态");
    let mut alerts = open_alerts(&journal);
    alerts.sort();
    assert_eq!(alerts, [(RUN_ERROR.to_owned(), "T-1".to_owned()), (RUN_ERROR.to_owned(), "T-2".to_owned())]);
}

#[test]
fn a_budget_stop_holds_and_an_unknown_cost_under_a_budget_asks_a_person() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.claim("T-1", "R-1", None).unwrap();
    started(&mut journal, "T-1", "R-1", Some(1.0)).unwrap();
    let mut unknown = outcome("T-1", "R-1", Outcome::Reported, Some(report(ReportStatus::Done, None)));
    unknown.cost_usd = None;
    finish(&mut journal, &unknown).unwrap();
    assert_eq!(state_of(&journal, "T-1"), TaskState::Running);
    assert_eq!(open_alerts(&journal), [(BUDGET_UNKNOWN.to_owned(), "T-1".to_owned())]);
    assert_eq!(tasks(&journal.state).unwrap().runs["R-1"].cost_usd, None, "未知费用不能记为 0");
    let record = &records(&journal.state).unwrap()["R-1"];
    assert_eq!((record.known_cost_usd, record.unknown_cost), (0.0, 1));

    started(&mut journal, "T-1", "R-1", Some(1.0)).unwrap();
    let budget = outcome("T-1", "R-1", Outcome::Budget, None);
    finish(&mut journal, &budget).unwrap();
    assert_eq!(state_of(&journal, "T-1"), TaskState::Held);
    assert!(open_alerts(&journal).contains(&(BUDGET_EXHAUSTED.to_owned(), "T-1".to_owned())));
    assert!(journal.state[GROUP]["R-1"]["finishes"][1]["stop"] == "budget");
}

#[test]
fn an_interrupted_run_moves_nothing() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.claim("T-1", "R-1", None).unwrap();
    started(&mut journal, "T-1", "R-1", None).unwrap();
    let mut stopped = outcome("T-1", "R-1", Outcome::Interrupted, None);
    stopped.stop = None;
    assert!(finish(&mut journal, &stopped).is_err(), "中断须对应运行器的停止");
    stopped.stop = Some(Stop::Interrupted);
    finish(&mut journal, &stopped).unwrap();
    assert_eq!(state_of(&journal, "T-1"), TaskState::Running);
    assert!(open_alerts(&journal).is_empty());
}

#[test]
fn a_finish_report_is_validated_and_recorded_but_proves_nothing() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = journal.claim("T-1", "R-1", None).unwrap();
    let request = |report: RunReport| {
        let mut request = crate::task_finish::FinishRequest { evidence_id: "E-1".into(), run: run.clone(),
            requested_at_ms: 1_000, deadline_ms: 1_000 + crate::task_finish::VERIFYING_TTL_MS, started_eid: "s".into(),
            completion_eid: "c".into(), interrupted_eid: "i".into(), claims: vec![], report: None };
        request.report = Some(report);
        json!(request)
    };
    let mut bad = report(ReportStatus::Done, None);
    bad.summary = " ".into();
    let event = journal.event("task.finish.requested", "process:towermd-cli", "L4", "E-1", request(bad), &run.fences);
    assert!(journal.apply(&event).is_err(), "无效报告拒收");
    let event = journal.event("task.finish.requested", "process:towermd-cli", "L4", "E-1",
        request(report(ReportStatus::Done, None)), &run.fences);
    journal.apply(&event).unwrap();
    assert_eq!(state_of(&journal, "T-1"), TaskState::Verifying, "与无报告的 finish 相同，等验证");
    assert_eq!(journal.state["Evidence"]["E-1"]["request"]["report"]["status"], "done");
    assert_eq!(journal.state["Evidence"]["E-1"]["status"], "queued");
}
