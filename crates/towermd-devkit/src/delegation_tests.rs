use super::*;
use crate::delegation_kit::delegations;
use towermd_core::delegation::{Brief, Checkpoint, Phase, SourceKind};
use towermd_core::delegation_projection::{self as log, overdue_eid};

const HUMAN: &str = "human:local";
const CLI: &str = "process:towermd-cli";

fn item(journal: &Journal, id: &str) -> towermd_core::delegation::KitDelegation {
    delegations(&journal.state).unwrap().into_iter().find(|item| item.id == id).unwrap()
}

fn source() -> HarnessSource {
    HarnessSource { name: "claude-code".into(), version: "2.1".into(), model: Some("m".into()), ..HarnessSource::default() }
}

/// A task taken through claim, a finish with one claim, and the merge.
fn merged(journal: &mut Journal, task: &str, run: &str) -> Run {
    let run = journal.claim(task, run, Some(source())).unwrap();
    let evidence = format!("E-{}", run.binding.id);
    journal.finish(&run, &evidence, vec![Claim { criterion: 0, note: "tested".into() }]).unwrap();
    journal.merge_by_hand(&run, &evidence, &"a".repeat(40));
    run
}

#[test]
fn clarifying_tasks_take_a_restatement_then_a_human_final_card() {
    let mut journal = Journal::new();
    journal.create(TaskDraft { clarify: true, acceptance: vec![], ..draft("T-1") });
    assert_eq!(item(&journal, "T-1").phase, Phase::Clarifying);
    let restated = json!({"restated":"do x","done_when":["x works"],"questions":["a?","b?"]});
    assert!(journal.send("delegation.clarified", HUMAN, "L4", "T-1", restated.clone()).is_err());
    let four = json!({"restated":"do x","done_when":[],"questions":["1","2","3","4"]});
    assert!(journal.send("delegation.clarified", CLI, "L4", "T-1", four).unwrap_err().contains("3"));
    journal.send("delegation.clarified", CLI, "L4", "T-1", restated.clone()).unwrap();
    let brief = Brief { why: Some("w".into()), done_when: vec!["x works".into(), "y".into()], ..Brief::default() };
    let card = json!({"brief":brief,"answers":["yes",""]});
    assert!(journal.send("delegation.finalized", CLI, "L4", "T-1", card.clone()).is_err());
    assert!(journal.send("delegation.finalized", HUMAN, "L4", "T-1", json!({"brief":{"done_when":[]}}))
        .unwrap_err().contains("G0"));
    assert!(journal.send("delegation.finalized", HUMAN, "L4", "T-1",
        json!({"brief":brief,"answers":["1","2","3"]})).is_err());
    journal.send("delegation.finalized", HUMAN, "L4", "T-1", card).unwrap();
    let task = &journal.state["Task"]["T-1"];
    assert_eq!((task["state"].as_str(), task["input"]["acceptance"][1].as_str(), task["input"].get("clarify")),
        (Some("ready"), Some("y"), None));
    assert_eq!(item(&journal, "T-1").brief, brief);
    assert!(journal.send("delegation.clarified", CLI, "L4", "T-1", restated).unwrap_err().contains("待澄清"));
    assert!(journal.send("delegation.finalized", HUMAN, "L4", "T-1", json!({"brief":brief})).is_err());
    let log = log::read(&journal.state).unwrap();
    assert_eq!((log.objects["T-1"].clarifications.len(), log.objects["T-1"].finalized.len()), (1, 1));
}

#[test]
fn merged_tasks_show_check_and_claimed_evidence_and_take_one_verdict_per_delegation() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    assert!(journal.send("acceptance.used", HUMAN, "L4", "T-1", json!({"criteria":[0]})).is_err());
    merged(&mut journal, "T-1", "R-1");
    let found = item(&journal, "T-1");
    assert_eq!(found.phase, Phase::Done);
    let attempt = &found.attempts[0];
    assert_eq!((attempt.delegate.as_ref().unwrap().model.as_deref(), attempt.active, attempt.version.clone()),
        (Some("m"), false, Some("a".repeat(40))));
    assert!(attempt.delivered.is_some() && attempt.done.is_some());
    let kinds: Vec<_> = found.evidence.iter().map(|entry| (entry.source_kind, entry.criteria.clone(), entry.actor.clone())).collect();
    assert_eq!(kinds, vec![(SourceKind::Check, vec![], "process:towermd-verifier".into()),
        (SourceKind::Claimed, vec![0], "claude-code/2.1".into())]);
    for criteria in [json!([]), json!([2]), json!([1, 1])] {
        assert!(journal.send("acceptance.used", HUMAN, "L4", "T-1", json!({"criteria":criteria})).is_err());
    }
    assert!(journal.send("acceptance.confirmed", HUMAN, "L4", "T-1", json!({"review_ms":5,"unproven":[]}))
        .unwrap_err().contains("未证实"));
    journal.send("acceptance.used", HUMAN, "L4", "T-1", json!({"criteria":[1],"note":"ran it"})).unwrap();
    assert!(journal.send("acceptance.confirmed", CLI, "L4", "T-1", json!({"review_ms":5,"unproven":[]})).is_err());
    journal.send("acceptance.confirmed", HUMAN, "L4", "T-1",
        json!({"review_ms":5,"unproven":[],"takeaway":"tests were enough"})).unwrap();
    assert!(journal.send("acceptance.sent_back", HUMAN, "L4", "T-1", json!({"reason":"no","review_ms":1}))
        .unwrap_err().contains("验收过"));
    assert_eq!(journal.state["Task"]["T-1"]["state"], "merged");
}

#[test]
fn a_send_back_reopens_the_task_and_can_become_a_rule() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    merged(&mut journal, "T-1", "R-1");
    assert!(journal.send("acceptance.sent_back", HUMAN, "L4", "T-1", json!({"reason":" ","review_ms":1})).is_err());
    let back = journal.send("acceptance.sent_back", HUMAN, "L4", "T-1", json!({"reason":"wrong api","review_ms":9})).unwrap();
    assert_eq!(journal.state["Task"]["T-1"]["state"], "ready");
    assert_eq!(item(&journal, "T-1").phase, Phase::Ready);
    journal.claim("T-1", "R-2", None).unwrap();
    let found = item(&journal, "T-1");
    assert_eq!((found.attempts.len(), found.attempts[1].id.as_str(), found.phase), (2, "R-2", Phase::Running));
    let rule = json!({"kind":"A","text":"use the v2 api","source":back.eid});
    assert!(journal.send("rule.recorded", HUMAN, "L4", "RULE-1", rule.clone()).is_err());
    assert!(journal.send("rule.recorded", HUMAN, "L3", "RULE-1",
        json!({"kind":"A","text":"x","source":"e-1"})).unwrap_err().contains("退回"));
    journal.send("rule.recorded", HUMAN, "L3", "RULE-1", rule.clone()).unwrap();
    assert!(journal.send("rule.recorded", HUMAN, "L3", "RULE-1", rule).is_err());
}

#[test]
fn work_kinds_rename_without_cycles_and_levels_are_set_by_people() {
    let mut journal = Journal::new();
    journal.send("work_kind.renamed", HUMAN, "L3", "A", json!({"from":"A","to":"B"})).unwrap();
    journal.send("work_kind.renamed", HUMAN, "L3", "B", json!({"from":"B","to":"C"})).unwrap();
    assert!(journal.send("work_kind.renamed", HUMAN, "L3", "C", json!({"from":"C","to":"A"})).unwrap_err().contains("成环"));
    assert!(journal.send("work_kind.renamed", HUMAN, "L3", "X", json!({"from":"C","to":"D"})).is_err());
    assert!(journal.send("work_kind.renamed", CLI, "L3", "C", json!({"from":"C","to":"D"})).is_err());
    assert_eq!(log::read(&journal.state).unwrap().work_kind("A"), "C");
    let level = json!({"config":"*","kind":"C","level":"approve_minor"});
    assert!(journal.send("delegation.level.set", CLI, "L3", "C", level.clone()).is_err());
    assert!(journal.send("delegation.level.set", HUMAN, "L3", "A", level.clone()).is_err());
    journal.send("delegation.level.set", HUMAN, "L3", "C", level).unwrap();
    assert_eq!(log::read(&journal.state).unwrap().levels[0].data.level, "approve_minor");
}

#[test]
fn dated_checkpoints_become_report_requests_with_replies_and_overdue_signals() {
    let mut journal = Journal::new();
    let checkpoints = vec![
        Checkpoint { text: "plan".into(), due: Some("2026-10-22T09:00:00Z".into()) },
        Checkpoint { text: "stop".into(), due: None },
        Checkpoint { text: "draft".into(), due: Some("2026-10-22T09:30:00Z".into()) },
        Checkpoint { text: "later".into(), due: Some("2026-10-23T09:00:00Z".into()) },
    ];
    journal.create(TaskDraft { checkpoints, ..draft("T-1") });
    journal.claim("T-1", "R-1", None).unwrap();
    let requests = log::report_requests(&delegations(&journal.state).unwrap(), &log::read(&journal.state).unwrap());
    let ids: Vec<_> = requests.iter().map(|request| (request.id.as_str(), request.open)).collect();
    assert_eq!(ids, [("R-R-1-1", true), ("R-R-1-3", true), ("R-R-1-4", true)]);
    let reply = json!({"state":"in_progress","summary":"half way","next":"finish the parser"});
    assert!(journal.send("report.replied", HUMAN, "L5", "R-R-1-1", reply.clone()).is_err());
    assert!(journal.send("report.replied", CLI, "L5", "R-R-1-1",
        json!({"state":"in_progress","summary":"x".repeat(101)})).unwrap_err().contains("100"));
    assert!(journal.send("report.replied", CLI, "L5", "R-R-1-1", json!({"state":"done","summary":"x"})).is_err());
    assert!(journal.send("report.replied", CLI, "L5", "R-R-1-2", reply.clone()).is_err());
    let overdue = |journal: &mut Journal, request: &str, level: &str, eid: Option<String>| {
        let mut event = journal.event("report.overdue", "process:towermd", "L5", request, json!({"level":level}), &[]);
        event.eid = eid.unwrap_or_else(|| overdue_eid(request));
        journal.apply(&event)
    };
    assert!(overdue(&mut journal, "R-R-1-1", "passive", Some("e-x".into())).unwrap_err().contains("派生"));
    assert!(overdue(&mut journal, "R-R-1-4", "passive", None).unwrap_err().contains("到期"));
    assert!(overdue(&mut journal, "R-R-1-1", "active", None).unwrap_err().contains("级别"));
    overdue(&mut journal, "R-R-1-1", "passive", None).unwrap();
    assert!(overdue(&mut journal, "R-R-1-1", "passive", None).is_err());
    overdue(&mut journal, "R-R-1-3", "active", None).unwrap();
    journal.send("report.replied", CLI, "L5", "R-R-1-1", reply.clone()).unwrap();
    assert!(journal.send("report.replied", CLI, "L5", "R-R-1-1", reply).unwrap_err().contains("已关闭"));
    let log = log::read(&journal.state).unwrap();
    assert!(log.reports["R-R-1-1"].replied.is_some() && log.reports["R-R-1-1"].overdue.is_some());
}

#[test]
fn next_overdue_names_the_earliest_due_request_with_the_level_the_reducer_demands() {
    let mut journal = Journal::new();
    let checkpoints = vec![
        Checkpoint { text: "draft".into(), due: Some("2026-10-22T09:30:00Z".into()) },
        Checkpoint { text: "plan".into(), due: Some("2026-10-22T09:00:00Z".into()) },
        Checkpoint { text: "later".into(), due: Some("2026-10-23T09:00:00Z".into()) },
    ];
    journal.create(TaskDraft { checkpoints, ..draft("T-1") });
    journal.claim("T-1", "R-1", None).unwrap();
    let now = towermd_core::delegation::parse_time("2026-10-22T10:00:00Z").unwrap().timestamp_millis() as u64;
    let mut written = Vec::new();
    while let Some((request, overdue)) = log::next_overdue(&journal.state, crate::dev_state::KITS, now).unwrap() {
        let level = serde_json::to_value(overdue.level).unwrap();
        let mut event = journal.event("report.overdue", "process:towermd", "L5", &request.id, json!({"level":level}), &[]);
        event.eid = overdue_eid(&request.id);
        journal.apply(&event).unwrap();
        written.push((request.id, level));
    }
    assert_eq!(written, [("R-R-1-2".to_string(), json!("passive")), ("R-R-1-1".to_string(), json!("active"))]);
}

#[test]
fn delegation_events_need_an_empty_fence_list_and_a_known_object() {
    let mut journal = Journal::new();
    journal.create(TaskDraft { clarify: true, ..draft("T-1") });
    let fence = Fence { name: "x".into(), holder: "y".into(), epoch: 1 };
    let event = journal.event("delegation.clarified", CLI, "L4", "T-1",
        json!({"restated":"r","done_when":[]}), &[fence]);
    assert!(journal.apply(&event).unwrap_err().contains("信封"));
    assert!(journal.send("delegation.clarified", CLI, "L4", "T-9", json!({"restated":"r"})).unwrap_err().contains("不存在"));
}

#[test]
fn the_view_reads_task_delegations_end_to_end() {
    use towermd_core::delegation::DelegationSettings;
    use towermd_core::delegation_view::view;
    use towermd_core::gates::GateFile;
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    merged(&mut journal, "T-1", "R-1");
    journal.send("acceptance.used", HUMAN, "L4", "T-1", json!({"criteria":[1]})).unwrap();
    let found = view(&journal.state, KITS, &DelegationSettings::default(), &GateFile::default()).unwrap();
    let card = found.objects[0].card.as_ref().unwrap();
    assert_eq!((card.unproven.len(), card.can_judge, card.whole[0].passed), (0, true, Some(true)));
    assert_eq!((found.track_record[0].work_kind.as_str(), found.track_record[0].records), ("A", 1));
    assert_eq!(found.spot_checks.len(), 1, "a new configuration is always drawn");
    assert!(card.rows[0].evidence[0].version.is_some() && card.whole[0].version == card.version);
}

#[test]
fn tasks_complete_by_core_verify_and_a_declared_gate_blocks_them() {
    use towermd_core::gates::{judge_object, GateFile, GateStatus, Verdict};
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let none = GateFile::default();
    let judgement = judge_object(&journal.state, KITS, &none, "T-1").unwrap();
    assert_eq!((judgement.verdict, judgement.gates[0].gate.as_str()), (Verdict::Pending, "core.verify"));
    merged(&mut journal, "T-1", "R-1");
    let found = item(&journal, "T-1");
    assert_eq!((found.checks.clone(), found.human_can_complete), (vec!["core.verify".to_owned()], false));
    assert_eq!(found.evidence[0].check.as_deref(), Some("core.verify"));
    let judgement = judge_object(&journal.state, KITS, &none, "T-1").unwrap();
    assert_eq!((judgement.verdict, judgement.gates[0].status), (Verdict::Passed, GateStatus::Passed));

    // A delegate cannot forge the built-in check through a signal.
    let forged = json!({"v":1,"project":"p","from":{"id":"me","kind":"source"},"target":"T-1","verb":"signal",
        "data":{"kind":"core.verify","level":"log","title":"ok","deterministic":true},"at":"2026-10-23T10:00:00Z"});
    assert!(journal.send("signal.received", CLI, "L5", "T-1", forged).unwrap_err().contains("core."));
    let loud = json!({"v":1,"project":"p","from":{"id":"ci","kind":"source"},"target":"T-1","verb":"signal",
        "data":{"kind":"eng.crap","level":"active","title":"CRAP 14","deterministic":true},"at":"2026-10-23T10:00:00Z"});
    journal.send("signal.received", CLI, "L5", "T-1", loud).unwrap();
    let gates = GateFile::parse("gates:\n  crap: { check: eng.crap }\n");
    let judgement = judge_object(&journal.state, KITS, &gates, "T-1").unwrap();
    assert_eq!(judgement.verdict, Verdict::Blocked);
    assert!(judgement.declared_blocks().unwrap().contains("CRAP 14"));
    assert_eq!(judge_object(&journal.state, KITS, &none, "T-1").unwrap().verdict, Verdict::Passed);
}
