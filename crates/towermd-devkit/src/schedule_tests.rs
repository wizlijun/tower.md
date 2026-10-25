use super::*;
use crate::run_report::{Outcome, ReportStatus, RunFinished, RunReport, Stop, Tokens, RUNNER};
use crate::schedule::{self, plan, Settings, Stage, Step, INTEGRATOR, SCHEDULER};
use crate::task_model::Dispatch;
use std::collections::BTreeSet;

const CLI: &str = "process:towermd-cli";
const AGENT: &str = "claude-code/2.1.308";

fn main_fence() -> Fence {
    Fence { name: "main".into(), holder: "h".into(), epoch: 1 }
}

fn queue(journal: &mut Journal, task: &str) -> Result<Event, String> {
    journal.send(schedule::QUEUED, CLI, "L5", task, json!({"task": task}))
}

fn dispatched(journal: &Journal, task: &str, run: &str) -> Run {
    let mut run = journal.run(task, run, None);
    run.dispatch = Some(Dispatch { harness: "claude-code".into(), model: None });
    run
}

fn claim_as(journal: &mut Journal, run: &Run, actor: &str) -> Result<(), String> {
    let event = journal.event("task.claimed", actor, "L4", &run.task, json!(TaskEvent::Claimed { run: run.clone() }), &run.fences);
    journal.apply(&event)
}

fn started(journal: &mut Journal, run: &Run) {
    journal.send("run.started", RUNNER, "L5", &run.task, json!({"run":run.binding.id,"task":run.task,"agent":AGENT,
        "adapter":"claude-code","budget_usd":null,"task_card":"/g/card.md","raw":"/g/output.jsonl"})).unwrap();
}

fn finished(journal: &mut Journal, run: &Run, outcome: Outcome, status: ReportStatus) {
    finished_in(journal, run, outcome, status, None);
}

fn finished_in(journal: &mut Journal, run: &Run, outcome: Outcome, status: ReportStatus, session: Option<&str>) {
    let report = (outcome == Outcome::Reported).then(|| RunReport { status, summary: "说明".into(), checkpoints: vec![],
        changed_paths: vec![], questions: vec![], suspected_class: None, proposal: None });
    let mut data = RunFinished { run: run.binding.id.clone(), task: run.task.clone(), agent: AGENT.into(), outcome,
        status, stop: (outcome == Outcome::Interrupted).then_some(Stop::Interrupted),
        exit_code: Some(0), signal: None, crashed: false, report, summary: "说明".into(), error_class: None,
        error: (outcome == Outcome::Failed).then(|| "坏了".into()), cost_usd: None, tokens: Tokens::default(),
        session: session.map(str::to_owned), attempts: 1, raw: None };
    data.classify(false);
    journal.send("run.finished", RUNNER, "L5", &run.task, json!(data)).unwrap();
}

fn integrator(journal: &mut Journal, kind: &str, task: &str, data: Value) -> Result<Event, String> {
    let event = journal.event(kind, INTEGRATOR, "L5", task, data, &[main_fence()]);
    journal.apply(&event).map(|_| event)
}

fn steps(journal: &Journal, live: &[&str]) -> Vec<Step> {
    let live = live.iter().map(|run| run.to_string()).collect::<BTreeSet<_>>();
    plan(&journal.state, "h", &live, &Settings::default()).unwrap()
}

const BASE: &str = "1111111111111111111111111111111111111111";
const COMMIT: &str = "2222222222222222222222222222222222222222";

#[test]
fn the_queue_takes_requests_from_the_cli_or_a_person_once() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let initial = journal.state.clone();
    assert!(journal.send(schedule::QUEUED, AGENT, "L5", "T-1", json!({"task":"T-1"})).is_err());
    assert!(journal.send(schedule::QUEUED, CLI, "L5", "T-1", json!({"task":"T-1","harness":"nope"})).is_err());
    assert!(journal.send(schedule::QUEUED, CLI, "L5", "T-9", json!({"task":"T-9"})).is_err());
    assert_eq!(journal.state, initial, "refused requests leave no group behind");
    queue(&mut journal, "T-1").unwrap();
    assert!(queue(&mut journal, "T-1").is_err());
    let queued = schedule::queue(&journal.state).unwrap();
    assert_eq!(queued["T-1"].by, CLI);
    assert!(journal.send(schedule::DEQUEUED, AGENT, "L5", "T-1", json!({"task":"T-1","reason":"x"})).is_err());
    journal.send(schedule::DEQUEUED, SCHEDULER, "L5", "T-1", json!({"task":"T-1","reason":"预算不足"})).unwrap();
    assert!(schedule::queue(&journal.state).unwrap().is_empty());
    journal.send(schedule::QUEUED, "human:local", "L5", "T-1", json!({"task":"T-1","harness":"codex","model":"m"})).unwrap();
}

#[test]
fn only_the_scheduler_claims_releases_and_only_the_integrator_finishes_a_dispatched_run() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = dispatched(&journal, "T-1", "R-1");
    assert!(claim_as(&mut journal, &run, CLI).is_err());
    claim_as(&mut journal, &run, SCHEDULER).unwrap();
    assert!(journal.finish(&run, "EV-1", vec![]).is_err(), "the CLI cannot finish a dispatched run");
    let mut cli_run = journal.run("T-1", "R-2", None);
    cli_run.binding.worktree = "/tmp/other".into();
    let release = |actor: &str, journal: &mut Journal| {
        let event = journal.event("task.released", actor, "L4", "T-1",
            json!(TaskEvent::Released { run_id: "R-1".into(), reason: ReleaseReason::Voluntary }), &run.fences);
        journal.apply(&event)
    };
    assert!(release(CLI, &mut journal).is_err());
    release(SCHEDULER, &mut journal).unwrap();
    assert!(claim_as(&mut journal, &cli_run, SCHEDULER).is_err(), "a CLI claim is still the CLI's");
    claim_as(&mut journal, &cli_run, CLI).unwrap();
}

#[test]
fn rebase_and_verify_rejections_count_toward_the_auto_fix_budget() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = dispatched(&journal, "T-1", "R-1");
    claim_as(&mut journal, &run, SCHEDULER).unwrap();
    let reject = |stage: &str| json!({"task":"T-1","run":"R-1","base":BASE,"stage":stage,"reason":"冲突"});
    let event = journal.event(schedule::MERGE_REJECTED, SCHEDULER, "L5", "T-1", reject("rebase"), &[main_fence()]);
    assert!(journal.apply(&event).is_err(), "only the integrator records merges");
    let event = journal.event(schedule::MERGE_REJECTED, INTEGRATOR, "L5", "T-1", reject("rebase"), &[]);
    assert!(journal.apply(&event).is_err(), "a merge result needs the main lease");
    integrator(&mut journal, schedule::MERGE_REJECTED, "T-1", reject("publish")).unwrap();
    assert_eq!(tasks(&journal.state).unwrap().tasks["T-1"].gate_failures, 0);
    integrator(&mut journal, schedule::MERGE_REJECTED, "T-1", reject("rebase")).unwrap();
    integrator(&mut journal, schedule::MERGE_REJECTED, "T-1", reject("verify")).unwrap();
    assert_eq!(tasks(&journal.state).unwrap().tasks["T-1"].state, TaskState::Running);
    integrator(&mut journal, schedule::MERGE_REJECTED, "T-1", reject("rebase")).unwrap();
    let task = &tasks(&journal.state).unwrap().tasks["T-1"];
    assert_eq!((task.state, task.gate_failures), (TaskState::Held, 3));
    let mark = &schedule::merges(&journal.state).unwrap()["R-1"];
    assert_eq!((mark.status.as_str(), mark.stage), ("rejected", Some(Stage::Rebase)));
}

#[test]
fn a_merge_binds_passed_evidence_with_g5_and_leaves_the_queue() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    queue(&mut journal, "T-1").unwrap();
    let run = dispatched(&journal, "T-1", "R-1");
    claim_as(&mut journal, &run, SCHEDULER).unwrap();
    let done = json!({"task":"T-1","run":"R-1","evidence_id":"EV-1","base":BASE,"commit":COMMIT});
    assert!(integrator(&mut journal, schedule::MERGE_DONE, "T-1", done.clone()).is_err());
    journal.state["Evidence"]["EV-1"] = json!({"run":"R-1","status":"passed"});
    assert!(integrator(&mut journal, schedule::MERGE_DONE, "T-1", done.clone()).is_err(), "G5 first");
    journal.state["Evidence"]["EV-1"]["g5_eid"] = json!("g5");
    integrator(&mut journal, schedule::MERGE_DONE, "T-1", done).unwrap();
    assert!(schedule::queue(&journal.state).unwrap().is_empty());
    assert_eq!(schedule::merges(&journal.state).unwrap()["R-1"].commit.as_deref(), Some(COMMIT));
}

#[test]
fn a_pass_dispatches_by_priority_up_to_the_concurrency_limit() {
    let mut journal = Journal::new();
    for (id, priority) in [("T-1", 0), ("T-2", 5), ("T-3", 1)] {
        let mut input = draft(id);
        input.priority = priority;
        journal.create(input);
        queue(&mut journal, id).unwrap();
    }
    let settings = Settings { concurrency: 2, ..Settings::default() };
    let steps = plan(&journal.state, "h", &BTreeSet::new(), &settings).unwrap();
    let tasks = steps.iter().map(|step| match step { Step::Dispatch { task, .. } => task.as_str(), _ => "?" }).collect::<Vec<_>>();
    assert_eq!(tasks, ["T-2", "T-3"]);
    assert!(matches!(&steps[0], Step::Dispatch { harness, model: None, .. } if harness == "claude-code"));
}

#[test]
fn a_dispatched_run_launches_integrates_and_relaunches_from_its_records() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    queue(&mut journal, "T-1").unwrap();
    let run = dispatched(&journal, "T-1", "R-1");
    claim_as(&mut journal, &run, SCHEDULER).unwrap();
    let launch = |note: bool| move |steps: &[Step]| steps.iter().any(|step| matches!(step, Step::Launch { run, note: n } if run == "R-1" && n.is_some() == note));
    assert!(launch(false)(&steps(&journal, &[])));
    assert_eq!(steps(&journal, &["R-1"]), [Step::Renew { run: "R-1".into() }]);
    started(&mut journal, &run);
    assert!(steps(&journal, &[]).contains(&Step::Orphaned { run: "R-1".into() }), "a lost process after a restart");
    finished(&mut journal, &run, Outcome::Interrupted, ReportStatus::Failed);
    assert!(launch(false)(&steps(&journal, &[])));
    started(&mut journal, &run);
    finished(&mut journal, &run, Outcome::Reported, ReportStatus::Done);
    assert!(steps(&journal, &[]).contains(&Step::Integrate { run: "R-1".into(), evidence_id: None }));
    integrator(&mut journal, schedule::MERGE_REJECTED, "T-1",
        json!({"task":"T-1","run":"R-1","base":BASE,"stage":"rebase","reason":"冲突"})).unwrap();
    assert!(launch(true)(&steps(&journal, &[])), "the delegate is told why");
    started(&mut journal, &run);
    finished(&mut journal, &run, Outcome::Reported, ReportStatus::Blocked);
    assert!(steps(&journal, &[]).iter().any(|step| matches!(step, Step::GiveUp { status: ReportStatus::Blocked, .. })));
}

/// 12.2, FR-25: a run parked on `needs_input` waits while its question is
/// open, then continues its session with the answer as the new prompt. Once
/// that is delivered, a second park with nothing new goes to a person.
#[test]
fn a_parked_run_waits_for_its_answer_and_then_resumes_its_session() {
    use towermd_core::question_model::{prepare_question, AnswerInput, Delivery, MessageId, QuestionDraft, QuestionEvent,
        RememberAs, RunContext, WaitMode, RESUME_DELIVERY};
    assert_eq!(RESUME_DELIVERY, SCHEDULER);
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    queue(&mut journal, "T-1").unwrap();
    let run = dispatched(&journal, "T-1", "R-1");
    claim_as(&mut journal, &run, SCHEDULER).unwrap();
    started(&mut journal, &run);
    let question = prepare_question(QuestionDraft { id: "H-1".into(), text: "用哪种格式？".into(), options: vec![],
        recommended: None, context: vec![], mode: WaitMode::Parked },
        RunContext { run: "R-1".into(), task: "T-1".into(), scope: run.module.clone() }, vec![], 1000).unwrap();
    journal.send("hold.opened", CLI, "L5", "H-1", json!(QuestionEvent::Opened { question })).unwrap();
    finished_in(&mut journal, &run, Outcome::Reported, ReportStatus::NeedsInput, Some("s-1"));
    let resumed = |steps: Vec<Step>| steps.into_iter().find(|step| matches!(step, Step::Resume { .. }));
    let gave_up = |steps: &[Step]| steps.iter().any(|step| matches!(step, Step::GiveUp { .. }));
    let waiting = steps(&journal, &[]);
    assert!(resumed(waiting.clone()).is_none() && !gave_up(&waiting), "an open question holds the parked run: {waiting:?}");

    journal.send("hold.answered", "human:local", "L5", "H-1", json!(QuestionEvent::Answered { answer: AnswerInput {
        hold: "H-1".into(), text: "JSON".into(), remember_as: RememberAs::Temporary, targets: vec![], assumption: None } })).unwrap();
    let Some(Step::Resume { run: id, session, deliveries, message }) = resumed(steps(&journal, &[])) else { panic!("no resume") };
    assert_eq!((id.as_str(), session.as_deref(), deliveries.len()), ("R-1", Some("s-1"), 1));
    assert!(message.contains("用哪种格式？") && message.contains("JSON") && message.contains("human:local"), "{message}");

    // The scheduler records the delivery; no other process may.
    let delivered = json!(QuestionEvent::Delivered { delivery: Delivery { run: "R-1".into(), message: deliveries[0].clone() } });
    assert!(journal.send("prompt.delivered", INTEGRATOR, "L5", "H-1", delivered.clone()).is_err());
    journal.send("prompt.delivered", SCHEDULER, "L5", "H-1", delivered).unwrap();
    assert!(matches!(&deliveries[0], MessageId::Answer { hold, .. } if hold == "H-1"));
    started(&mut journal, &run);
    finished_in(&mut journal, &run, Outcome::Reported, ReportStatus::NeedsInput, Some("s-1"));
    let after = steps(&journal, &[]);
    assert!(resumed(after.clone()).is_none() && gave_up(&after), "nothing new came: a person decides: {after:?}");
}

#[test]
fn cancelled_or_paused_runs_stop_and_cancelled_ones_are_released() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    queue(&mut journal, "T-1").unwrap();
    let run = dispatched(&journal, "T-1", "R-1");
    claim_as(&mut journal, &run, SCHEDULER).unwrap();
    journal.state["tasks_paused"] = json!(true);
    assert_eq!(steps(&journal, &["R-1"]), [Step::Stop { run: "R-1".into() }]);
    assert!(steps(&journal, &[]).is_empty(), "a paused run waits for resume");
    journal.state["tasks_paused"] = json!(false);
    journal.send(schedule::DEQUEUED, CLI, "L5", "T-1", json!({"task":"T-1","reason":"不要了"})).unwrap();
    assert_eq!(steps(&journal, &["R-1"]), [Step::Stop { run: "R-1".into() }]);
    assert_eq!(steps(&journal, &[]), [Step::Release { run: "R-1".into(), reason: ReleaseReason::Voluntary }]);
}

#[test]
fn a_task_the_person_paused_is_stopped_and_not_dispatched_until_resumed() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.create(draft("T-2"));
    queue(&mut journal, "T-1").unwrap();
    queue(&mut journal, "T-2").unwrap();
    let run = dispatched(&journal, "T-1", "R-1");
    claim_as(&mut journal, &run, SCHEDULER).unwrap();
    let person = |journal: &mut Journal, kind: &str, data: Value| {
        let task = data["task_id"].as_str().unwrap().to_owned();
        let event = journal.event(kind, "human:local", "L4", &task, data, &[]);
        journal.apply(&event)
    };
    person(&mut journal, "task.paused", json!(TaskEvent::Paused { task_id: "T-1".into() })).unwrap();
    person(&mut journal, "task.paused", json!(TaskEvent::Paused { task_id: "T-2".into() })).unwrap();
    assert_eq!(steps(&journal, &["R-1"]), [Step::Stop { run: "R-1".into() }], "the live run stops; T-2 is not dispatched");
    assert!(steps(&journal, &[]).is_empty(), "a paused run waits; a paused queued task is not dispatched");
    person(&mut journal, "task.resumed", json!(TaskEvent::Resumed { task_id: "T-2".into() })).unwrap();
    assert!(steps(&journal, &[]).iter().any(|step| matches!(step, Step::Dispatch { task, .. } if task == "T-2")), "{:?}", steps(&journal, &[]));
    person(&mut journal, "task.resumed", json!(TaskEvent::Resumed { task_id: "T-1".into() })).unwrap();
    assert!(steps(&journal, &[]).contains(&Step::Launch { run: "R-1".into(), note: None }));
}

#[test]
fn settings_read_the_runner_section_with_per_module_overrides() {
    let config: serde_yaml::Value = serde_yaml::from_str("concurrency: 2\ntimeout_minutes: 5\nroles:\n  executor:\n    harness: codex\n    model: gpt\nmodules:\n  A:\n    harness: gemini\n  B:\n    model: other\n").unwrap();
    let settings = Settings::from_config(Some(&config));
    assert_eq!(settings.concurrency, 2);
    assert_eq!(settings.timeout.as_secs(), 300);
    assert_eq!((settings.role("A").harness.as_str(), settings.role("A").model.as_deref()), ("gemini", Some("gpt")));
    assert_eq!((settings.role("B").harness.as_str(), settings.role("B").model.as_deref()), ("codex", Some("other")));
    assert_eq!(settings.role("C").harness, "codex");
    let defaults = Settings::from_config(Some(&serde_yaml::from_str("concurrency: 0").unwrap()));
    assert_eq!(defaults, Settings::default());
    assert_eq!(defaults.concurrency, 4);
}

#[test]
fn the_card_carries_paths_criteria_and_why_it_runs_again() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let task = &tasks(&journal.state).unwrap().tasks["T-1"];
    let card = schedule::card(task, Some("集成器拒绝合并：冲突"));
    for part in ["# T-1 T-1 title", "`src/a/**`", "1. first", "2. second", "集成器拒绝合并：冲突"] {
        assert!(card.contains(part), "{part}");
    }
}

/// FR-51–54: only the scheduler records what a dispatched run loaded, per run.
#[test]
fn only_the_scheduler_records_the_skills_a_run_loaded() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.claim("T-1", "R-1", None).unwrap();
    let loaded = |run: &str, task: &str| json!({"task": task, "run": run, "attempt": 1, "role": "executor", "revision": "abc",
        "packs": [{"name": "flow", "version": "1.0.0", "hash": "sha256:00", "skills": ["plan"]}]});
    for actor in [CLI, AGENT, "human:local", RUNNER] {
        assert!(journal.send(crate::skillpacks::LOADED, actor, "L5", "R-1", loaded("R-1", "T-1")).is_err(), "{actor}");
    }
    assert!(journal.send(crate::skillpacks::LOADED, SCHEDULER, "L5", "R-2", loaded("R-2", "T-1")).is_err(), "运行不存在");
    assert!(journal.send(crate::skillpacks::LOADED, SCHEDULER, "L5", "R-1", loaded("R-1", "T-2")).is_err(), "另一个任务");
    assert!(journal.send(crate::skillpacks::LOADED, SCHEDULER, "L5", "R-1",
        json!({"task": "T-1", "run": "R-1", "attempt": 1, "role": "executor", "revision": "abc", "packs": []})).is_err(), "没加载就不记");
    journal.send(crate::skillpacks::LOADED, SCHEDULER, "L5", "R-1", loaded("R-1", "T-1")).unwrap();
    assert_eq!(journal.state[crate::skillpacks::PROJECTION]["R-1"]["packs"][0]["skills"], json!(["plan"]));
}
