//! Every interrupt transition of the task state machine (10.2, 12.1).
use super::*;
use crate::run_record::{BUDGET_EXHAUSTED, RUN_ERROR};
use crate::run_report::{Outcome, ReportStatus, RunFinished, Tokens, RUNNER};
use crate::task_interrupts::{INFO_REQUESTED, RESOLVED, VERIFY};
use crate::task_model::{InterruptKind, ReleaseReason, GATE_EXHAUSTED, FORBIDDEN, INFO_MISSING};

fn task(journal: &Journal, id: &str) -> crate::task_model::Task {
    tasks(&journal.state).unwrap().tasks[id].clone()
}

fn interrupt(journal: &Journal, id: &str) -> Option<(InterruptKind, String)> {
    task(journal, id).interrupt.map(|interrupt| (interrupt.kind, interrupt.opened))
}

fn alert(journal: &Journal, id: &str) -> (String, bool) {
    let alert = &journal.state["Alert"][id];
    (alert["data"]["reason"].as_str().unwrap_or("none").to_owned(), alert["resolved"] == true)
}

fn decide(journal: &mut Journal, task: &str, choice: &str, opened: &str) -> Result<Event, String> {
    journal.send(RESOLVED, "human:local", "L4", task, json!({"choice": choice, "interrupt": opened}))
}

fn blocked(journal: &mut Journal, task: &str, gate: &str, builtin: bool) -> Event {
    journal.send("evidence.recorded", "process:towermd", "L5", task, json!({"kind":"gate","object":task,"version":"tree-1",
        "stage":"finish","revision":null,"verdict":"blocked","gates":[{"gate":gate,"check":"ci.lint","hard":true,
        "builtin":builtin,"status":"failed","reason":"lint 未过"}]})).unwrap()
}

fn review(journal: &mut Journal, id: &str, task: &str, stage: &str) -> Event {
    journal.send("review.requested", "process:towermd-cli", "L5", id,
        json!({"object":task,"stage":stage,"summary":"计划见 docs/plan.md","attempt":"R-1"})).unwrap()
}

#[test]
fn a_review_holds_a_working_task_until_a_person_resolves_it() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.claim("T-1", "R-1", None).unwrap();
    let asked = review(&mut journal, "V-1", "T-1", "plan");
    assert_eq!(task(&journal, "T-1").state, TaskState::Held, "运行中 → 中断中");
    assert_eq!(interrupt(&journal, "T-1"), Some((InterruptKind::Review, asked.eid.clone())));
    assert!(decide(&mut journal, "T-1", "retry", &asked.eid).unwrap_err().contains("评审"), "评审只由评审结论结束");
    journal.send("review.resolved", "human:local", "L5", "V-1", json!({"verdict":"changes_requested","comment":"补回滚方案"})).unwrap();
    let state = task(&journal, "T-1");
    assert!(state.interrupt.is_none());
    assert_eq!(state.state, TaskState::Running, "中断中 → 运行中：评审有结论即恢复，结论本身不是完成");
}

#[test]
fn hard_gate_failures_get_two_fixes_then_hold_and_a_waiver_ends_the_hold() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.claim("T-1", "R-1", None).unwrap();
    blocked(&mut journal, "T-1", "ci.lint", true);
    assert_eq!(task(&journal, "T-1").gate_failures, 0, "内置检查在验证器里计数");
    blocked(&mut journal, "T-1", "lint", false);
    blocked(&mut journal, "T-1", "lint", false);
    assert_eq!((task(&journal, "T-1").state, task(&journal, "T-1").gate_failures), (TaskState::Running, 2));
    let third = blocked(&mut journal, "T-1", "lint", false);
    let held = task(&journal, "T-1");
    assert_eq!((held.state, held.gate_failures), (TaskState::Held, 3), "第 3 次未通过 → 中断中");
    assert_eq!(held.interrupt.as_ref().unwrap().gate.as_deref(), Some("lint"));
    assert_eq!(alert(&journal, &third.eid), (GATE_EXHAUSTED.to_owned(), false));
    // A waiver for another gate changes nothing; the named gate's ends the hold.
    let waive = |gate: &str| json!({"gate":gate,"reason":"上游 lint 规则误报","until":"2026-10-25T00:00:00Z"});
    journal.send("waiver.granted", "human:local", "L3", "T-1", waive("other")).unwrap();
    assert_eq!(task(&journal, "T-1").state, TaskState::Held);
    let granted = journal.send("waiver.granted", "human:local", "L3", "T-1", waive("lint")).unwrap();
    let state = task(&journal, "T-1");
    assert_eq!((state.state, state.gate_failures, state.interrupt.is_none()), (TaskState::Running, 0, true));
    assert_eq!(alert(&journal, &third.eid), (GATE_EXHAUSTED.to_owned(), true));
    assert_eq!(journal.state["Alert"][&third.eid]["resolved_by"], granted.eid.as_str());
}

#[test]
fn a_person_retries_rewrites_or_reassigns_a_held_task() {
    for (choice, state, run_kept) in [("retry", TaskState::Running, true), ("rewrite", TaskState::Proposed, false),
        ("reassign", TaskState::Ready, false)]
    {
        let mut journal = Journal::new();
        journal.create(draft("T-1"));
        journal.claim("T-1", "R-1", None).unwrap();
        for _ in 0..3 {
            blocked(&mut journal, "T-1", "lint", false);
        }
        let opened = task(&journal, "T-1").interrupt.unwrap().opened;
        assert!(journal.send(RESOLVED, "process:towermd-cli", "L4", "T-1", json!({"choice":choice,"interrupt":opened})).is_err(),
            "只有人能处理中断");
        assert!(decide(&mut journal, "T-1", choice, "e-0").unwrap_err().contains("刷新"));
        assert!(decide(&mut journal, "T-1", "pass", &opened).is_err(), "没有放行选项");
        decide(&mut journal, "T-1", choice, &opened).unwrap();
        let after = task(&journal, "T-1");
        assert_eq!((after.state, after.gate_failures, after.interrupt.is_none()), (state, 0, true), "{choice}");
        assert_eq!(after.active_run.is_some(), run_kept, "{choice}");
        let run = &tasks(&journal.state).unwrap().runs["R-1"];
        assert_eq!(run.state == RunState::Active, run_kept, "{choice}");
        assert_eq!(alert(&journal, &opened).1, true);
        assert!(decide(&mut journal, "T-1", choice, &opened).unwrap_err().contains("没有待处理"));
    }
}

#[test]
fn a_released_run_leaves_a_gate_failure_held_until_a_person_decides() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = journal.claim("T-1", "R-1", None).unwrap();
    for _ in 0..3 {
        blocked(&mut journal, "T-1", "lint", false);
    }
    let event = journal.event("task.released", "process:towermd-cli", "L4", "T-1",
        json!(TaskEvent::Released { run_id: "R-1".into(), reason: ReleaseReason::Voluntary }), &run.fences);
    journal.apply(&event).unwrap();
    assert_eq!(task(&journal, "T-1").state, TaskState::Held, "放弃运行不放掉中断");
    assert!(journal.claim("T-1", "R-2", None).is_err());
    let opened = task(&journal, "T-1").interrupt.unwrap().opened;
    decide(&mut journal, "T-1", "retry", &opened).unwrap();
    assert_eq!(task(&journal, "T-1").state, TaskState::Ready, "中断中 → 就绪：原运行已结束");
    journal.claim("T-1", "R-2", None).unwrap();
}

#[test]
fn a_g2_stop_at_finish_holds_at_once_as_forbidden() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = journal.claim("T-1", "R-1", None).unwrap();
    let request = journal.finish(&run, "E-1", vec![]).unwrap();
    let mut stop = journal.event("task.finish.interrupted", "process:towermd-verifier", "L4", "E-1",
        json!({"request_eid":request.eid,"error":"G2 路径守卫失败：docs/x.md：越界","guard":"G3"}), &[]);
    stop.eid = "i".into();
    assert!(journal.apply(&stop).is_err(), "只认 G2");
    stop.data["guard"] = json!("G2");
    journal.apply(&stop).unwrap();
    let held = task(&journal, "T-1");
    assert_eq!((held.state, held.interrupt.as_ref().map(|i| i.kind)), (TaskState::Held, Some(InterruptKind::Forbidden)),
        "验证中 → 中断中：禁止不给自动修复次数");
    assert_eq!(alert(&journal, "i"), (FORBIDDEN.to_owned(), false));
    decide(&mut journal, "T-1", "retry", "i").unwrap();
    assert_eq!(task(&journal, "T-1").state, TaskState::Running);
    assert_eq!(alert(&journal, "i").1, true);
}

#[test]
fn an_ordinary_verifier_interruption_does_not_hold() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = journal.claim("T-1", "R-1", None).unwrap();
    let request = journal.finish(&run, "E-1", vec![]).unwrap();
    let mut stop = journal.event("task.finish.interrupted", "process:towermd-verifier", "L4", "E-1",
        json!({"request_eid":request.eid,"error":"验证超时"}), &[]);
    stop.eid = "i".into();
    journal.apply(&stop).unwrap();
    assert_eq!((task(&journal, "T-1").state, task(&journal, "T-1").gate_failures), (TaskState::Running, 0));
}

#[test]
fn missing_information_asks_the_creator_and_a_claim_or_final_card_answers() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let reason = json!({"reason":"G0：缺少允许路径或验收标准"});
    assert!(journal.send(INFO_REQUESTED, "claude-code/2.1", "L4", "T-1", reason.clone()).is_err());
    assert!(journal.send(INFO_REQUESTED, "process:towermd-cli", "L4", "T-1", json!({"reason":" "})).is_err());
    let asked = journal.send(INFO_REQUESTED, "process:towermd-cli", "L4", "T-1", reason.clone()).unwrap();
    assert_eq!(task(&journal, "T-1").state, TaskState::Ready, "不派出，也不改状态");
    assert_eq!(interrupt(&journal, "T-1"), Some((InterruptKind::MissingInfo, asked.eid.clone())));
    assert_eq!(alert(&journal, &asked.eid), (INFO_MISSING.to_owned(), false));
    assert!(journal.send(INFO_REQUESTED, "process:towermd-cli", "L4", "T-1", reason.clone()).is_err(), "一次一问");
    journal.claim("T-1", "R-1", None).unwrap();
    assert!(task(&journal, "T-1").interrupt.is_none(), "领取通过 G0 即已补齐");
    assert_eq!(alert(&journal, &asked.eid).1, true);
    assert!(journal.send(INFO_REQUESTED, "process:towermd-cli", "L4", "T-1", reason).is_err(), "已委托的任务不问");
}

#[test]
fn missing_information_can_be_settled_on_the_card() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let asked = journal.send(INFO_REQUESTED, "process:towermd-cli", "L4", "T-1", json!({"reason":"缺决策 D-1"})).unwrap();
    decide(&mut journal, "T-1", "rewrite", &asked.eid).unwrap();
    assert_eq!((task(&journal, "T-1").state, task(&journal, "T-1").interrupt.is_none()), (TaskState::Proposed, true));
    assert_eq!(alert(&journal, &asked.eid).1, true);
}

fn started(journal: &mut Journal, run: &str) {
    journal.send("run.started", RUNNER, "L5", "T-1", json!({"run":run,"task":"T-1","agent":"mock/1.0","adapter":"mock",
        "budget_usd":1.0,"task_card":"c","raw":"r"})).unwrap();
}

fn ended(journal: &mut Journal, run: &str, outcome: Outcome, crashed: bool) -> Event {
    let mut data = RunFinished { run: run.into(), task: "T-1".into(), agent: "mock/1.0".into(), outcome, status: ReportStatus::Failed,
        stop: (outcome == Outcome::Budget).then_some(crate::run_report::Stop::Budget), exit_code: Some(1), signal: None, crashed,
        report: None, summary: "说明".into(), error_class: None, error: Some("坏了".into()), cost_usd: Some(0.5),
        tokens: Tokens::default(), session: None, attempts: 1, raw: None };
    data.classify(false);
    journal.send("run.finished", RUNNER, "L5", "T-1", json!(data)).unwrap()
}

#[test]
fn the_runners_error_and_budget_holds_carry_the_same_options() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.claim("T-1", "R-1", None).unwrap();
    started(&mut journal, "R-1");
    let crash = ended(&mut journal, "R-1", Outcome::Failed, true);
    assert_eq!(interrupt(&journal, "T-1"), Some((InterruptKind::RunError, crash.eid.clone())));
    assert_eq!(alert(&journal, &crash.eid), (RUN_ERROR.to_owned(), false));
    decide(&mut journal, "T-1", "retry", &crash.eid).unwrap();
    assert_eq!(task(&journal, "T-1").state, TaskState::Running, "重试：同一运行继续");
    assert_eq!(alert(&journal, &crash.eid).1, true);
    // The crash a person saw no longer counts toward the next hold.
    started(&mut journal, "R-1");
    ended(&mut journal, "R-1", Outcome::Failed, false);
    assert_eq!(task(&journal, "T-1").state, TaskState::Failed, "运行中 → 失败");

    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.claim("T-1", "R-1", None).unwrap();
    started(&mut journal, "R-1");
    let budget = ended(&mut journal, "R-1", Outcome::Budget, false);
    assert_eq!(interrupt(&journal, "T-1"), Some((InterruptKind::Budget, budget.eid.clone())));
    decide(&mut journal, "T-1", "reassign", &budget.eid).unwrap();
    assert_eq!(task(&journal, "T-1").state, TaskState::Ready, "停止：运行结束，任务回到就绪");
    assert_eq!(alert(&journal, &budget.eid), (BUDGET_EXHAUSTED.to_owned(), true));
}

#[test]
fn the_verify_check_is_the_counted_builtin_gate() {
    assert_eq!(VERIFY, crate::delegation_kit::VERIFY_CHECK);
}

#[test]
fn a_person_reassigns_a_running_task_from_the_app_without_fences() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = journal.claim("T-1", "R-1", None).unwrap();
    let release = |reason| json!(TaskEvent::Released { run_id: "R-1".into(), reason });
    for (actor, reason, guards) in [
        ("human:local", ReleaseReason::Voluntary, &[][..]),
        ("human:local", ReleaseReason::Reassigned, &run.fences[..]),
        ("process:towermd", ReleaseReason::Reassigned, &[][..]),
    ] {
        let event = journal.event("task.released", actor, "L4", "T-1", release(reason), guards);
        assert!(journal.apply(&event).is_err(), "{actor} {reason:?}");
    }
    let event = journal.event("task.released", "human:local", "L4", "T-1", release(ReleaseReason::Reassigned), &[]);
    journal.apply(&event).unwrap();
    assert_eq!(task(&journal, "T-1").state, TaskState::Ready, "重派：运行结束，任务回到就绪");
    let again = journal.event("task.released", "human:local", "L4", "T-1", release(ReleaseReason::Reassigned), &[]);
    assert!(journal.apply(&again).is_err(), "已结束的运行不能再重派");
    journal.claim("T-1", "R-2", None).unwrap();
}
