//! G1 and G4 (FR-58): a person's verdict on the attempt or tree, never a timer.
use super::*;
use crate::task_reviews::{checks, g1, g4};

fn ask(journal: &mut Journal, id: &str, stage: &str, version: Option<&str>) {
    let mut data = json!({"object":"T-1","stage":stage,"summary":"见 docs/plan.md","attempt":"R-1"});
    if let Some(version) = version {
        data["version"] = json!(version);
    }
    journal.send("review.requested", "process:towermd-cli", "L5", id, data).unwrap();
}

fn verdict(journal: &mut Journal, id: &str, verdict: &str) {
    journal.send("review.resolved", "human:local", "L5", id, json!({"verdict":verdict,"comment":"意见"})).unwrap();
}

#[test]
fn a_high_risk_run_needs_a_passed_plan_and_code_review() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let mut run = journal.claim("T-1", "R-1", None).unwrap();
    assert!(g1(&journal.state, &run).is_ok() && g4(&journal.state, &run, "tree-1").is_ok(), "普通模块不强制评审");
    run.high_risk = true;
    assert!(g1(&journal.state, &run).unwrap_err().contains("--review plan"));
    assert!(g4(&journal.state, &run, "tree-1").unwrap_err().contains("--review code"));
    assert_eq!(checks(true), ["core.verify", "human.review.plan", "human.review.code"]);
    assert_eq!(checks(false), ["core.verify"]);

    ask(&mut journal, "V-1", "plan", None);
    assert!(g1(&journal.state, &run).unwrap_err().contains("尚待人处理"), "评审不会超时通过");
    verdict(&mut journal, "V-1", "changes_requested");
    assert!(g1(&journal.state, &run).unwrap_err().contains("要求修改"));
    ask(&mut journal, "V-2", "plan", None);
    journal.send("review.resolved", "human:local", "L5", "V-2", json!({"verdict":"passed"})).unwrap();
    g1(&journal.state, &run).unwrap();

    // G4 binds to the tested tree: a pass on another tree does not count.
    ask(&mut journal, "V-3", "code", Some("tree-0"));
    journal.send("review.resolved", "human:local", "L5", "V-3", json!({"verdict":"passed"})).unwrap();
    assert!(g4(&journal.state, &run, "tree-1").unwrap_err().contains("G4"));
    ask(&mut journal, "V-4", "code", Some("tree-1"));
    verdict(&mut journal, "V-4", "rejected");
    assert!(g4(&journal.state, &run, "tree-1").unwrap_err().contains("驳回"));
    ask(&mut journal, "V-5", "code", Some("tree-1"));
    journal.send("review.resolved", "human:local", "L5", "V-5", json!({"verdict":"passed"})).unwrap();
    g4(&journal.state, &run, "tree-1").unwrap();
}

#[test]
fn an_asked_review_binds_even_an_ordinary_run() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = journal.claim("T-1", "R-1", None).unwrap();
    ask(&mut journal, "V-1", "plan", None);
    assert!(g1(&journal.state, &run).is_err());
    verdict(&mut journal, "V-1", "rejected");
    assert!(g1(&journal.state, &run).is_err(), "驳回后须重新请求评审");
}
