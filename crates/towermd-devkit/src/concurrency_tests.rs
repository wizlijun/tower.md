//! FR-31: the concurrency hint is computed, never stored, never applied.
use super::*;
use crate::concurrency::{hint, CALIBRATED};
use crate::schedule::SCHEDULER;
use crate::task_model::Dispatch;
use towermd_core::attention_list::parse_time;
use towermd_core::question_model::{prepare_question, QuestionDraft, QuestionEvent, RunContext, WaitMode};

fn dispatched(journal: &mut Journal, task: &str, run: &str) -> Run {
    let mut run = journal.run(task, run, None);
    run.dispatch = Some(Dispatch { harness: "claude-code".into(), model: None });
    let event = journal.event("task.claimed", SCHEDULER, "L4", task, json!(TaskEvent::Claimed { run: run.clone() }), &run.fences);
    journal.apply(&event).unwrap();
    run
}

fn ask(journal: &mut Journal, hold: &str, run: &Run, opened_at: u64) {
    let question = prepare_question(QuestionDraft { id: hold.into(), text: "用哪个格式？".into(), options: vec![], recommended: None,
        context: vec![], mode: WaitMode::Parked },
        RunContext { run: run.binding.id.clone(), task: run.task.clone(), scope: run.module.clone() }, vec![], opened_at).unwrap();
    journal.send("hold.opened", "process:towermd-cli", "L5", hold, json!(QuestionEvent::Opened { question })).unwrap();
}

#[test]
fn frequent_human_interrupts_suggest_a_lower_concurrency_without_changing_it() {
    let mut journal = Journal::new();
    for id in ["T-1", "T-2", "T-3"] { journal.create(draft(id)); }
    let one = dispatched(&mut journal, "T-1", "R-1");
    let two = dispatched(&mut journal, "T-2", "R-2");
    let session = journal.claim("T-3", "R-3", None).unwrap();
    let now = parse_time("2026-10-22T12:00:00Z");
    let quiet = hint(&journal.state, 4, now).unwrap();
    assert_eq!((quiet.active, quiet.interrupts, quiet.suggest, quiet.longest_wait_minutes), (2, 0, None, None));
    // An interactive session's question is not the dispatched work's.
    ask(&mut journal, "H-3", &session, now - 90 * 60_000);
    assert_eq!(hint(&journal.state, 4, now).unwrap().interrupts, 0);
    ask(&mut journal, "H-1", &one, now - 40 * 60_000);
    let slow = hint(&journal.state, 4, now).unwrap();
    assert_eq!((slow.interrupts, slow.longest_wait_minutes, slow.suggest), (1, Some(40), Some(CALIBRATED)),
        "one interrupt waiting 40 minutes is a bottleneck");
    assert!(slow.reason.unwrap().contains("不改配置"));
    let before = journal.state.clone();
    hint(&journal.state, 4, now).unwrap();
    assert_eq!(journal.state, before, "nothing is stored");
    assert_eq!(hint(&journal.state, CALIBRATED, now).unwrap().suggest, None, "already at the calibrated limit");
    for (hold, run) in [("H-2", &two), ("H-4", &two), ("H-5", &one)] { ask(&mut journal, hold, run, now - 60_000); }
    let crowded = hint(&journal.state, 8, now).unwrap();
    assert_eq!((crowded.interrupts, crowded.suggest), (4, Some(CALIBRATED - 1)), "two interrupts per task");
}
