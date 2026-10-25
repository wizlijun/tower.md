use super::*;
use crate::permissions::{self, expired_eid, read, Closed, Decision, ALERT, EXPIRED, REQUESTED, RESOLVED};
use crate::run_report::{Outcome, ReportStatus, RunFinished, Tokens, RUNNER};

const AGENT: &str = "mock/1.0";

fn running(journal: &mut Journal, task: &str, run: &str) {
    journal.create(draft(task));
    journal.claim(task, run, None).unwrap();
    journal.send("run.started", RUNNER, "L5", task, json!({"run":run,"task":task,"agent":AGENT,"adapter":"mock",
        "budget_usd":null,"task_card":"/g/towermd/runs/R/task.md","raw":"/g/towermd/runs/R/output.jsonl"})).unwrap();
}

fn ask(journal: &mut Journal, actor: &str, id: &str, run: &str, task: &str) -> Result<Event, String> {
    journal.send(REQUESTED, actor, "L5", id, json!({"run":run,"task":task,"tool":"Bash","input":"{\"command\":\"rm -rf build\"}"}))
}

fn alert(journal: &Journal, id: &str) -> (String, bool) {
    let alert = &journal.state["Alert"][id];
    (alert["data"]["reason"].as_str().unwrap_or("none").to_owned(), alert["resolved"] == true)
}

#[test]
fn only_the_running_delegate_asks_and_only_a_person_allows() {
    let mut journal = Journal::new();
    // Nothing ran: no record, no request.
    journal.create(draft("T-0"));
    journal.claim("T-0", "R-0", None).unwrap();
    assert!(ask(&mut journal, AGENT, "K-0", "R-0", "T-0").unwrap_err().contains("仍在运行"));
    running(&mut journal, "T-1", "R-1");
    for actor in ["other/1.0", "process:towermd-cli", "human:local"] {
        assert!(ask(&mut journal, actor, "K-1", "R-1", "T-1").is_err(), "{actor}");
    }
    assert!(ask(&mut journal, AGENT, "K-1", "R-1", "T-0").is_err(), "另一个任务");
    assert!(journal.send(REQUESTED, AGENT, "L5", "X-1", json!({"run":"R-1","task":"T-1","tool":"Bash","input":""})).is_err());
    ask(&mut journal, AGENT, "K-1", "R-1", "T-1").unwrap();
    assert!(ask(&mut journal, AGENT, "K-1", "R-1", "T-1").is_err(), "同一个请求只记一次");
    assert_eq!(alert(&journal, "K-1"), (ALERT.to_owned(), false));
    let permission = &read(&journal.state).unwrap()["K-1"];
    assert_eq!((permission.agent.as_str(), permission.tool.as_str(), permission.outcome.is_none()), (AGENT, "Bash", true));
    // The delegate, the CLI or the daemon cannot answer for the person.
    for actor in [AGENT, "process:towermd-cli", "process:towermd"] {
        assert!(journal.send(RESOLVED, actor, "L5", "K-1", json!({"decision":"allow"})).is_err(), "{actor}");
    }
    assert!(journal.send(RESOLVED, "human:local", "L5", "K-1", json!({"decision":"maybe"})).is_err());
    journal.send(RESOLVED, "human:local", "L5", "K-1", json!({"decision":"allow"})).unwrap();
    let outcome = read(&journal.state).unwrap()["K-1"].outcome.clone().unwrap();
    assert_eq!((outcome.decision, outcome.closed), (Decision::Allow, Closed::Answered));
    assert!(alert(&journal, "K-1").1);
    assert!(journal.send(RESOLVED, "human:local", "L5", "K-1", json!({"decision":"deny"})).is_err(), "已关闭");
}

#[test]
fn unanswered_requests_are_denied_never_allowed() {
    let mut journal = Journal::new();
    running(&mut journal, "T-1", "R-1");
    ask(&mut journal, AGENT, "K-1", "R-1", "T-1").unwrap();
    let expire = |journal: &mut Journal, eid: &str| {
        let mut event = journal.event(EXPIRED, "process:towermd", "L5", "K-1", json!({}), &[]);
        event.eid = eid.into();
        journal.apply(&event)
    };
    // Before the deadline the request cannot expire.
    assert!(expire(&mut journal, &expired_eid("K-1")).unwrap_err().contains("尚未到期"));
    for _ in 0..10 {
        journal.event("noop", "process:towermd", "L5", "x", json!({}), &[]);
    }
    // Past the deadline a person's answer is too late; expiry is a denial.
    assert!(journal.send(RESOLVED, "human:local", "L5", "K-1", json!({"decision":"allow"})).unwrap_err().contains("超时"));
    assert!(expire(&mut journal, "e-other").is_err(), "派生的事件 ID");
    expire(&mut journal, &expired_eid("K-1")).unwrap();
    let outcome = read(&journal.state).unwrap()["K-1"].outcome.clone().unwrap();
    assert_eq!((outcome.decision, outcome.closed), (Decision::Deny, Closed::Expired));
    assert!(alert(&journal, "K-1").1);
}

#[test]
fn a_run_that_ends_takes_its_open_requests_with_it() {
    let mut journal = Journal::new();
    running(&mut journal, "T-1", "R-1");
    ask(&mut journal, AGENT, "K-1", "R-1", "T-1").unwrap();
    ask(&mut journal, AGENT, "K-2", "R-1", "T-1").unwrap();
    journal.send(RESOLVED, "human:local", "L5", "K-2", json!({"decision":"deny","note":"不要删"})).unwrap();
    let mut finished = RunFinished { run: "R-1".into(), task: "T-1".into(), agent: AGENT.into(), outcome: Outcome::Failed,
        status: ReportStatus::Failed, stop: None, exit_code: Some(1), signal: None, crashed: false, report: None,
        summary: "说明".into(), error_class: None, error: Some("坏了".into()), cost_usd: Some(0.1), tokens: Tokens::default(),
        session: None, attempts: 1, raw: None };
    finished.classify(false);
    journal.send("run.finished", RUNNER, "L5", "T-1", json!(finished)).unwrap();
    let all = read(&journal.state).unwrap();
    assert_eq!(all["K-1"].outcome.as_ref().map(|o| (o.decision, o.closed)), Some((Decision::Deny, Closed::RunEnded)));
    assert_eq!(all["K-2"].outcome.as_ref().map(|o| (o.closed, o.note.clone())), Some((Closed::Answered, Some("不要删".into()))));
    assert!(alert(&journal, "K-1").1);
    // No request can be made for a run that is no longer running.
    assert!(ask(&mut journal, AGENT, "K-3", "R-1", "T-1").is_err());
}

#[test]
fn journals_without_requests_keep_the_old_projection() {
    let mut journal = Journal::new();
    running(&mut journal, "T-1", "R-1");
    assert!(journal.state.get(permissions::GROUP).is_none());
}

#[test]
fn the_input_a_person_sees_is_cut() {
    let long = "x".repeat(permissions::INPUT_CHARS + 10);
    let cut = permissions::summary(&json!(long));
    assert_eq!(cut.chars().count(), permissions::INPUT_CHARS);
    assert!(cut.ends_with('…'));
    assert_eq!(permissions::summary(&json!({"a":1})), "{\"a\":1}");
}
