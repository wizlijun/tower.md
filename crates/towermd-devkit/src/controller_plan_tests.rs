//! FR-67 controller action plans, spec 12.5 and J4 step 4.
use super::*;
use crate::controller_plan::{self, PlanEvent, Scope, Status, Step, CHANGED};
use crate::task_model::{ModuleScope, PublishedBaseline, VerifiedBaseline};
use std::collections::BTreeSet;
use towermd_core::question_model::CONTROLLER;

const HUMAN: &str = "human:local";

fn change(journal: &mut Journal, actor: &str, plan: &str, change: PlanEvent) -> Result<Event, String> {
    journal.send(CHANGED, actor, "L4", plan, serde_json::to_value(change).unwrap())
}

fn plan(journal: &Journal, id: &str) -> controller_plan::Plan {
    controller_plan::plans(&journal.state).unwrap().remove(id).unwrap()
}

fn task_state(journal: &Journal, id: &str) -> String {
    journal.state["Task"][id]["state"].as_str().unwrap().to_owned()
}

fn open_alerts(journal: &Journal) -> usize {
    journal.state["Alert"].as_object().unwrap().values()
        .filter(|alert| alert["data"]["reason"] == controller_plan::ALERT && alert["resolved"] == false).count()
}

fn previewed(steps: Vec<Step>) -> PlanEvent {
    PlanEvent::Previewed { summary: "暂停两项任务".into(), steps, drafts: vec![], notes: vec![], agent: Some("claude-code/2.1".into()),
        cost_usd: None }
}

fn baseline(tag: &str, modules: &[&str]) -> VerifiedBaseline {
    VerifiedBaseline::from_published(PublishedBaseline { tag: tag.into(), commit: "a".repeat(40),
        approval_eid: uuid::Uuid::now_v7().to_string(), content_hash: "b".repeat(64),
        modules: modules.iter().map(|id| ((*id).to_owned(), ModuleScope { paths: vec!["src/**".into()], high_risk: false })).collect(),
        decisions: BTreeSet::new() }).unwrap()
}

#[test]
fn a_plan_runs_only_when_a_person_confirms_its_preview() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.create(draft("T-2"));
    let global = PlanEvent::Requested { scope: Scope::Global, text: "先停下 A 模块的两项任务".into() };
    assert!(change(&mut journal, "process:towermd-cli", "plan-1", global.clone()).is_err(), "the command line asks nothing of the controller");
    change(&mut journal, HUMAN, "plan-1", global.clone()).unwrap();
    assert!(change(&mut journal, HUMAN, "plan-1", global).is_err(), "one request per plan");
    let pause = vec![Step::Pause { task: "T-1".into() }, Step::Pause { task: "T-2".into() }];
    assert!(change(&mut journal, HUMAN, "plan-1", previewed(pause.clone())).is_err(), "only the controller previews");
    assert!(change(&mut journal, CONTROLLER, "plan-1", PlanEvent::Confirmed { note: None }).is_err(), "the controller never confirms");
    change(&mut journal, CONTROLLER, "plan-1", previewed(pause)).unwrap();
    assert_eq!(plan(&journal, "plan-1").status, Status::Previewed);
    assert_eq!((task_state(&journal, "T-1"), task_state(&journal, "T-2")), ("ready".into(), "ready".into()), "a preview changes nothing");
    assert_eq!(open_alerts(&journal), 1, "the preview waits for a person, with no timeout");
    assert_eq!(plan(&journal, "plan-1").tasks().len(), 2);
    assert!(change(&mut journal, "process:towermd-cli", "plan-1", PlanEvent::Confirmed { note: None }).is_err());
    change(&mut journal, HUMAN, "plan-1", PlanEvent::Confirmed { note: Some("好".into()) }).unwrap();
    assert_eq!((task_state(&journal, "T-1"), task_state(&journal, "T-2")), ("paused".into(), "paused".into()));
    let done = plan(&journal, "plan-1");
    assert_eq!((done.status, done.resolved_by.as_deref()), (Status::Confirmed, Some(HUMAN)));
    assert_eq!(open_alerts(&journal), 0);
    assert!(change(&mut journal, HUMAN, "plan-1", PlanEvent::Confirmed { note: None }).is_err(), "a plan runs once");
    assert!(change(&mut journal, HUMAN, "plan-1", PlanEvent::TakenOver { note: None }).is_err());
}

#[test]
fn confirming_is_all_or_nothing_and_taking_over_runs_nothing() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.create(draft("T-2"));
    journal.send("task.paused", HUMAN, "L4", "T-2", json!(TaskEvent::Paused { task_id: "T-2".into() })).unwrap();
    change(&mut journal, HUMAN, "plan-2", PlanEvent::Requested { scope: Scope::Global, text: "T-1 提前，T-2 继续".into() }).unwrap();
    let steps = vec![Step::Priority { task: "T-1".into(), priority: 9 }, Step::Resume { task: "T-2".into() }];
    assert!(change(&mut journal, CONTROLLER, "plan-2", previewed(vec![Step::Pause { task: "T-9".into() }])).is_err(),
        "a preview that cannot run is refused");
    assert!(change(&mut journal, CONTROLLER, "plan-2", previewed(vec![Step::Pause { task: "T-1".into() }, Step::Resume { task: "T-1".into() }])).is_err(),
        "one action per task");
    change(&mut journal, CONTROLLER, "plan-2", previewed(steps)).unwrap();
    // The person resumed T-2 by hand meanwhile: the plan no longer holds.
    journal.send("task.resumed", HUMAN, "L4", "T-2", json!(TaskEvent::Resumed { task_id: "T-2".into() })).unwrap();
    let error = change(&mut journal, HUMAN, "plan-2", PlanEvent::Confirmed { note: None }).unwrap_err();
    assert_eq!(error, "任务当前不能继续");
    assert_eq!(journal.state["Task"]["T-1"]["input"]["priority"], 0, "nothing of a refused confirmation runs");
    assert_eq!(plan(&journal, "plan-2").status, Status::Previewed);
    change(&mut journal, HUMAN, "plan-2", PlanEvent::TakenOver { note: Some("我自己排".into()) }).unwrap();
    assert_eq!(journal.state["Task"]["T-1"]["input"]["priority"], 0);
    assert_eq!((plan(&journal, "plan-2").status, open_alerts(&journal)), (Status::TakenOver, 0));
}

#[test]
fn a_module_plan_touches_only_its_module_and_a_failure_is_handed_to_a_person() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.create(TaskDraft { module: "B".into(), ..draft("T-2") });
    let scope = Scope::Module { module: "B".into() };
    change(&mut journal, HUMAN, "plan-3", PlanEvent::Requested { scope: scope.clone(), text: "B 模块先停".into() }).unwrap();
    assert!(change(&mut journal, CONTROLLER, "plan-3", previewed(vec![Step::Pause { task: "T-1".into() }])).unwrap_err().contains("模块 B"));
    assert!(change(&mut journal, CONTROLLER, "plan-3", PlanEvent::Requested { scope: Scope::Baseline { baseline: "B0".into() }, text: "x".into() }).is_err());
    let request = plan(&journal, "plan-3");
    let reply = json!({"summary": "x", "steps": [{"action": "delete", "task": "T-2"}], "drafts": [], "notes": []});
    let failed = controller_plan::preview(&journal.state, &request, None, Some(&reply), None, Some("claude-code/2.1".into()), None);
    assert!(matches!(&failed, PlanEvent::Failed { reason, .. } if reason.contains("不支持的动作")), "{failed:?}");
    let timeout = controller_plan::preview(&journal.state, &request, None, None, Some("总控超时"), None, None);
    assert!(matches!(&timeout, PlanEvent::Failed { reason, cost_usd: None, .. } if reason == "总控超时"), "a timeout passes nothing");
    change(&mut journal, CONTROLLER, "plan-3", timeout).unwrap();
    assert_eq!((plan(&journal, "plan-3").status, open_alerts(&journal)), (Status::Failed, 1));
    assert!(change(&mut journal, HUMAN, "plan-3", PlanEvent::Confirmed { note: None }).is_err(), "nothing to confirm");
    change(&mut journal, HUMAN, "plan-3", PlanEvent::TakenOver { note: None }).unwrap();
    assert_eq!(open_alerts(&journal), 0);
}

#[test]
fn a_reply_becomes_task_steps_and_slow_layer_drafts_that_never_run() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    change(&mut journal, HUMAN, "plan-4", PlanEvent::Requested { scope: Scope::Global, text: "加一项导出任务".into() }).unwrap();
    let request = plan(&journal, "plan-4");
    let reply = json!({"summary": "新增导出任务，T-1 让路", "notes": [],
        "steps": [{"action": "create", "task": "T-5", "title": "导出 CSV", "module": "A", "allowed_paths": ["src/export.rs"],
            "acceptance": ["导出含表头"], "priority": 3}, {"action": "priority", "task": "T-1", "priority": -1}],
        "drafts": [{"target": "模块 A 的接口", "text": "加一个 export()"}]});
    let b0 = baseline("towermd/B0", &["A"]);
    let change_event = controller_plan::preview(&journal.state, &request, Some(&b0), Some(&reply), None, None, None);
    let PlanEvent::Previewed { steps, drafts, cost_usd, .. } = &change_event else { panic!("{change_event:?}") };
    assert!(matches!(&steps[0], Step::Create { task } if task.origin == crate::task_model::TaskOrigin::AppHuman && task.input.priority == 3));
    assert_eq!((drafts.len(), *cost_usd), (1, None), "unknown cost stays null");
    change(&mut journal, CONTROLLER, "plan-4", change_event).unwrap();
    assert!(journal.state["Task"].get("T-5").is_none(), "the preview creates nothing");
    change(&mut journal, HUMAN, "plan-4", PlanEvent::Confirmed { note: None }).unwrap();
    assert_eq!((task_state(&journal, "T-5"), journal.state["Task"]["T-1"]["input"]["priority"].clone()), ("ready".into(), json!(-1)));
    assert!(journal.state["Proposal"].as_object().is_none_or(|group| group.is_empty()), "a slow-layer draft is never applied");
}

#[test]
fn a_new_baseline_gets_a_plan_that_rebinds_the_tasks_still_in_it() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.create(TaskDraft { module: "Z".into(), ..draft("T-2") });
    journal.create(draft("T-3"));
    let _run = journal.claim("T-1", "R-1", None).unwrap();
    journal.state["Baseline"]["B1"] = json!({"type": "Baseline", "id": "B1"});
    for id in ["T-1", "T-2"] { journal.state["Task"][id]["needs_review"] = json!(true) }
    let b1 = baseline("towermd/B1", &["A"]);
    let (id, requested, preview) = controller_plan::regenerate(&journal.state, &b1).unwrap().unwrap();
    assert_eq!(id, "plan-J4-B1");
    let PlanEvent::Previewed { steps, notes, .. } = &preview else { panic!() };
    assert_eq!(steps, &vec![Step::Rebase { task: "T-1".into(), baseline: "B1".into() }]);
    assert!(notes.iter().any(|note| note.contains("t/T-1") && note.contains("参考")), "{notes:?}");
    assert!(notes.iter().any(|note| note.starts_with("T-2") && note.contains("改写")), "{notes:?}");
    assert!(change(&mut journal, HUMAN, &id, requested.clone()).is_err(), "the baseline plan is the controller's to raise");
    change(&mut journal, CONTROLLER, &id, requested).unwrap();
    change(&mut journal, CONTROLLER, &id, preview).unwrap();
    assert!(controller_plan::regenerate(&journal.state, &b1).unwrap().is_none(), "once per baseline");
    assert_eq!(journal.state["Task"]["T-1"]["needs_review"], true, "still waiting for the person");
    change(&mut journal, HUMAN, &id, PlanEvent::Confirmed { note: None }).unwrap();
    let task = &journal.state["Task"]["T-1"];
    assert_eq!((task["needs_review"].clone(), task["baseline"].clone()), (json!(false), json!("towermd/B1")));
    assert_eq!(journal.state["Run"]["R-1"]["baseline"], "towermd/B1");
    assert_eq!(journal.state["Task"]["T-2"]["needs_review"], true, "a task outside the baseline stays for the person");
    assert_eq!(journal.state["Task"]["T-3"]["baseline"], "towermd/B0", "an untouched task keeps its binding");
}
