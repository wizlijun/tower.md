//! M3-D controller, M3-H cross review and WP-X archive records (FR-23/24/30/34/65/66, FR-75–78).
use super::*;
use crate::controller::{self, Mode, Outcome, Policy, Step, Triage, OVERRIDE, TRIAGED};
use crate::cross_review::{self, Verdict, CROSS_REVIEWED, REVIEWER};
use crate::proposal_model::{Impact, Kind, Proposal};
use crate::run_report::{Outcome as RunOutcome, ReportStatus, RunFinished, Tokens, RUNNER};
use crate::schedule::SCHEDULER;
use crate::task_model::Dispatch;
use towermd_core::delegation::DelegationSettings;
use towermd_core::delegation_projection::Mark;
use towermd_core::delegation_view::{self, DelegationView, LevelRow};
use towermd_core::gates::GateFile;
use towermd_core::question_model::{prepare_question, ChangeTarget, QuestionDraft, QuestionEvent, RunContext, WaitMode, CONTROLLER};

const CLI: &str = "process:towermd-cli";
const AGENT: &str = "claude-code/2.1.308";

fn dispatched(journal: &mut Journal, task: &str, run: &str) -> Run {
    let mut run = journal.run(task, run, None);
    run.dispatch = Some(Dispatch { harness: "claude-code".into(), model: None });
    let event = journal.event("task.claimed", SCHEDULER, "L4", task, json!(TaskEvent::Claimed { run: run.clone() }), &run.fences);
    journal.apply(&event).unwrap();
    run
}

fn ask(journal: &mut Journal, hold: &str, run: &Run, slow: bool) {
    let mut question = prepare_question(QuestionDraft { id: hold.into(), text: "输出用什么格式？".into(),
        options: vec!["JSON".into(), "YAML".into()], recommended: Some(0), context: vec![], mode: WaitMode::Parked },
        RunContext { run: run.binding.id.clone(), task: run.task.clone(), scope: run.module.clone() }, vec![], 1000).unwrap();
    if slow { question.targets = vec![ChangeTarget { id: "A".into(), layer: "L2".into() }] }
    journal.send("hold.opened", CLI, "L5", hold, json!(QuestionEvent::Opened { question })).unwrap();
}

fn view(journal: &Journal, level: Option<&str>) -> DelegationView {
    let mut view = delegation_view::view(&journal.state, KITS, &DelegationSettings::default(), &GateFile::default()).unwrap();
    if let Some(level) = level {
        view.levels.push(LevelRow { config: "*".into(), work_kind: "*".into(), level: level.into(),
            event: Mark { at: "2026-10-22T09:00:00Z".into(), eid: "lv".into(), seq: 0, actor: "human:local".into() } });
    }
    view
}

fn plan(journal: &Journal, policy: &Policy, level: Option<&str>) -> Vec<Step> {
    controller::plan(&journal.state, policy, &view(journal, level)).unwrap()
}

fn send(journal: &mut Journal, event: towermd_core::hub_protocol::NewEvent) -> Result<Event, String> {
    journal.send(&event.kind, &event.actor, &event.layer, &event.obj, event.data)
}

fn yaml(text: &str) -> serde_yaml::Value {
    serde_yaml::from_str(text).unwrap()
}

#[test]
fn the_policy_reads_only_the_question_mode_and_the_controller_role() {
    assert_eq!(Policy::from_config(None, None), Policy::default());
    assert_eq!(Policy::default().question, Mode::AutoNotify, "FR-65 default");
    let controller = yaml("policy: {question: human, review: auto}\ntimeout_minutes: 2\n");
    let runner = yaml("roles: {controller: {harness: codex, model: gpt-5}}\n");
    let policy = Policy::from_config(Some(&controller), Some(&runner));
    assert_eq!(policy.question, Mode::Human);
    assert_eq!((policy.role.harness.as_str(), policy.role.model.as_deref()), ("codex", Some("gpt-5")));
    assert_eq!(policy.timeout.as_secs(), 120);
    let odd = Policy::from_config(Some(&yaml("policy: {question: sometimes}\n")), None);
    assert_eq!(odd.question, Mode::AutoNotify, "an unknown mode keeps the default");
}

#[test]
fn the_controller_considers_only_fast_questions_of_dispatched_runs_that_its_level_allows() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    journal.create(draft("T-2"));
    journal.create(draft("T-3"));
    let run = dispatched(&mut journal, "T-1", "R-1");
    let slow_run = dispatched(&mut journal, "T-2", "R-2");
    let session = journal.claim("T-3", "R-3", None).unwrap();
    ask(&mut journal, "H-1", &run, false);
    ask(&mut journal, "H-2", &slow_run, true);
    ask(&mut journal, "H-3", &session, false);
    let policy = Policy::default();
    let steps = plan(&journal, &policy, None);
    assert_eq!(steps.len(), 2, "an interactive session's question is the person's at that session");
    let reasons: Vec<_> = steps.iter().map(|step| match step {
        Step::HandOver { hold, reason, .. } => (hold.as_str(), reason.clone()),
        Step::Consult(_) => panic!("the default level asks a person"),
    }).collect();
    assert_eq!(reasons[0].0, "H-1");
    assert!(reasons[0].1.contains("须问人"), "{}", reasons[0].1);
    assert!(reasons[1].1.contains("慢层"));
    let steps = plan(&journal, &policy, Some("answer_by_decision"));
    let Step::Consult(candidate) = &steps[0] else { panic!("the level allows answering") };
    assert_eq!((candidate.question.draft.id.as_str(), candidate.task.as_str(), candidate.notify), ("H-1", "T-1", true));
    assert!(matches!(&steps[1], Step::HandOver { hold, permit: None, .. } if hold == "H-2"), "no level reaches a slow layer");
    let human = Policy { question: Mode::Human, ..Policy::default() };
    assert!(plan(&journal, &human, Some("approve_minor")).iter().all(|step| matches!(step, Step::HandOver { .. })));
    let brief = controller::brief(&journal.state, candidate).unwrap();
    assert!(brief.contains("输出用什么格式？") && brief.contains("T-1") && brief.contains("first"), "{brief}");
}

#[test]
fn a_controller_answer_needs_its_basis_and_each_question_is_triaged_once() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = dispatched(&mut journal, "T-1", "R-1");
    ask(&mut journal, "H-1", &run, false);
    ask(&mut journal, "H-2", &run, false);
    let steps = plan(&journal, &Policy::default(), Some("answer_by_decision"));
    let Step::Consult(candidate) = steps[0].clone() else { panic!() };
    assert!(controller::verdict(None, Some("超时")).unwrap_err().contains("超时"), "a timeout is handed over, never a pass");
    assert!(controller::verdict(Some(&json!({"in_scope":false,"answer":null,"basis":[],"reason":"要改接口"})), None).is_err());
    assert!(controller::verdict(Some(&json!({"in_scope":true,"answer":"JSON","basis":[],"reason":null})), None).is_err());
    let (text, basis) = controller::verdict(Some(&json!({"in_scope":true,"answer":"JSON","basis":["验收标准 first"],"reason":null})), None).unwrap();
    let answered = send(&mut journal, controller::answer_event("h", &candidate, text, basis).unwrap()).unwrap();
    let triage = |outcome, answer: Option<&str>, reason: Option<&str>, hold: &str| Triage { hold: hold.into(), task: "T-1".into(),
        outcome, answer: answer.map(str::to_owned), reason: reason.map(str::to_owned), level: Some("answer_by_decision".into()),
        agent: Some("codex/0.50".into()), cost_usd: None };
    let forged = triage(Outcome::Answered, Some("e-1"), None, "H-1");
    assert!(send(&mut journal, controller::triage_event("h", &forged).unwrap()).is_err(), "answered means the controller's own answer");
    let done = triage(Outcome::Answered, Some(&answered.eid), None, "H-1");
    send(&mut journal, controller::triage_event("h", &done).unwrap()).unwrap();
    let again = triage(Outcome::HandedOver, None, Some("改主意"), "H-1");
    assert!(send(&mut journal, controller::triage_event("h", &again).unwrap()).is_err(), "triaged once");
    let silent = triage(Outcome::HandedOver, None, None, "H-2");
    assert!(send(&mut journal, controller::triage_event("h", &silent).unwrap()).is_err(), "a hand-over says why");
    let mut stranger = controller::triage_event("h", &triage(Outcome::HandedOver, None, Some("范围外"), "H-2")).unwrap();
    stranger.actor = CLI.into();
    assert!(send(&mut journal, stranger).is_err());
    send(&mut journal, controller::triage_event("h", &triage(Outcome::HandedOver, None, Some("范围外"), "H-2")).unwrap()).unwrap();
    let triages = controller::triages(&journal.state).unwrap();
    assert_eq!(triages["H-1"].cost_usd, None, "unknown cost stays null");
    assert!(plan(&journal, &Policy::default(), Some("answer_by_decision")).is_empty());
    assert!(journal.state["Answer"].get("H-2").is_none(), "a handed-over question stays open for a person");
}

#[test]
fn a_person_overriding_the_controller_marks_the_finishes_since_its_answer() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = dispatched(&mut journal, "T-1", "R-1");
    journal.send("run.started", RUNNER, "L5", "T-1", json!({"run":"R-1","task":"T-1","agent":AGENT,
        "adapter":"claude-code","budget_usd":null,"task_card":"/g/card.md","raw":"/g/output.jsonl"})).unwrap();
    ask(&mut journal, "H-1", &run, false);
    let steps = plan(&journal, &Policy::default(), Some("answer_by_decision"));
    let Step::Consult(candidate) = steps[0].clone() else { panic!() };
    let answered = send(&mut journal, controller::answer_event("h", &candidate, "JSON".into(), vec!["验收标准 first".into()]).unwrap()).unwrap();
    let mut data = RunFinished { run: "R-1".into(), task: "T-1".into(), agent: AGENT.into(), outcome: RunOutcome::Failed,
        status: ReportStatus::Failed, stop: None, exit_code: Some(1), signal: None, crashed: false, report: None,
        summary: "说明".into(), error_class: None, error: Some("坏了".into()), cost_usd: None, tokens: Tokens::default(),
        session: None, attempts: 1, raw: None };
    data.classify(false);
    let finished = journal.send("run.finished", RUNNER, "L5", "T-1", json!(data)).unwrap();
    assert!(journal.state.get(OVERRIDE).is_none(), "the group appears only after an override");
    let human = journal.send("hold.answered", "human:local", "L5", "H-1", json!(QuestionEvent::Answered {
        answer: towermd_core::question_model::AnswerInput { hold: "H-1".into(), text: "YAML".into(),
            remember_as: towermd_core::question_model::RememberAs::Temporary, targets: vec![], assumption: None } })).unwrap();
    let overrides = controller::overrides(&journal.state).unwrap();
    let mark = &overrides["H-1"];
    assert_eq!((mark.replaced.as_str(), mark.answer.as_str(), mark.task.as_str()), (answered.eid.as_str(), human.eid.as_str(), "T-1"));
    assert_eq!(mark.affected, vec![finished.eid.clone()]);
    assert_eq!(journal.state["Answer"]["H-1"]["actor"], "human:local");
    let _ = CONTROLLER;
}

fn proposal(id: &str) -> Proposal {
    let mut proposal = Proposal { id: id.into(), project_id: "p".into(), source_revision: "1".repeat(40),
        baseline_tag: "towermd/B0".into(), draft_id: None, source_proposal_id: None, source_commit: None, source_parent: None,
        kind: Kind::Interface, reason: "更新调用契约".into(), alternatives: vec!["保留原契约".into()],
        recommendation: "采用新契约".into(), impact: Impact::default(), changes: vec![], sources: vec![],
        impact_notes: Default::default(), content_hash: String::new() };
    proposal.content_hash = proposal.hash().unwrap();
    proposal
}

#[test]
fn cross_review_is_heterogeneous_reference_only_and_unfinished_is_never_a_pass() {
    assert_eq!(cross_review::from_config(None).unwrap(), None, "off unless configured");
    assert!(cross_review::from_config(Some(&yaml("roles: {cross_reviewer: {harness: claude-code}}\n"))).is_err(),
        "the executor cannot review itself");
    assert!(cross_review::from_config(Some(&yaml("roles: {cross_reviewer: {harness: nope}}\n"))).is_err());
    let settings = cross_review::from_config(Some(&yaml("roles: {cross_reviewer: {harness: codex, model: m, timeout_minutes: 3}}\n")))
        .unwrap().unwrap();
    assert_eq!((settings.role.harness.as_str(), settings.timeout.as_secs()), ("codex", 180));
    let mut journal = Journal::new();
    let id = format!("P-{}", uuid::Uuid::now_v7());
    let p = proposal(&id);
    journal.state["Proposal"][&id] = json!({"type":"Proposal","id":id,"status":"submitted","proposal":p});
    assert_eq!(cross_review::plan(&journal.state).unwrap().len(), 1);
    assert!(cross_review::brief(&p).contains("更新调用契约"));
    let before = journal.state["Proposal"].clone();
    let timed_out = cross_review::record(&p, &settings, None, None, Some("评审超时".into()), None);
    assert_eq!((timed_out.verdict, timed_out.error.as_deref()), (Verdict::Unfinished, Some("评审超时")));
    let mut forged = timed_out.clone();
    forged.verdict = Verdict::Agree;
    assert!(send(&mut journal, cross_review::event("h", &forged).unwrap()).is_err(), "no opinion is not agreement");
    let mut stale = cross_review::record(&p, &settings, Some("codex/0.50".into()),
        Some(&json!({"verdict":"concerns","summary":"缺回滚","findings":[{"severity":"high","text":"没有回滚方案"}]})), None, Some(0.02));
    assert_eq!((stale.verdict, stale.findings.len(), stale.error.is_none()), (Verdict::Concerns, 1, true));
    let review = stale.clone();
    stale.proposal_hash = "0".repeat(64);
    assert!(send(&mut journal, cross_review::event("h", &stale).unwrap()).is_err(), "bound to the proposal's content");
    let mut stranger = cross_review::event("h", &review).unwrap();
    stranger.actor = CLI.into();
    assert!(send(&mut journal, stranger).is_err());
    let event = cross_review::event("h", &review).unwrap();
    assert_eq!((event.actor.as_str(), event.kind.as_str()), (REVIEWER, CROSS_REVIEWED));
    send(&mut journal, event).unwrap();
    assert!(send(&mut journal, cross_review::event("h", &timed_out).unwrap()).is_err(), "one review per proposal");
    assert_eq!(journal.state["Proposal"], before, "the review never changes the proposal or its status");
    assert_eq!(cross_review::reviews(&journal.state).unwrap()[&id].cost_usd, Some(0.02));
    assert!(cross_review::plan(&journal.state).unwrap().is_empty());
    let _ = TRIAGED;
}

#[test]
fn archive_records_are_immutable_and_bound_to_a_started_run() {
    use crate::artifacts::{self, Archive, File, Store};
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    dispatched(&mut journal, "T-1", "R-1");
    let file = File { name: "output.jsonl".into(), path: "towermd/runs/R-1/1/output.jsonl".into(), sha256: "a".repeat(64),
        bytes: 3, arca: None };
    let archive = Archive { run: "R-1".into(), task: "T-1".into(), attempt: 1, store: Store::Local, files: vec![file.clone()],
        note: Some("没有安装 Arca，原始输出只留在本机".into()) };
    assert!(send(&mut journal, artifacts::event("h", &archive).unwrap()).is_err(), "no start, nothing to archive");
    journal.send("run.started", RUNNER, "L5", "T-1", json!({"run":"R-1","task":"T-1","agent":AGENT,
        "adapter":"claude-code","budget_usd":null,"task_card":"/g/card.md","raw":"/g/output.jsonl"})).unwrap();
    let mut escape = archive.clone();
    escape.files[0].path = "../outside".into();
    assert!(send(&mut journal, artifacts::event("h", &escape).unwrap()).is_err());
    send(&mut journal, artifacts::event("h", &archive).unwrap()).unwrap();
    let mut changed = archive.clone();
    changed.files[0].sha256 = "b".repeat(64);
    assert!(send(&mut journal, artifacts::event("h", &changed).unwrap()).is_err(), "immutable");
    assert_eq!(artifacts::local_only(&journal.state).unwrap(), 1);
}
