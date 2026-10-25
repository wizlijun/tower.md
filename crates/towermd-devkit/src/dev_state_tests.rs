use super::*;
use crate::task_finish::{Claim, FinishRequest, VERIFYING_TTL_MS};
use crate::task_model::{HarnessSource, RunBinding, Task, TaskDraft, TaskOrigin, TaskState};
use towermd_core::hub::Fence;

pub(super) struct Journal {
    pub state: Value,
    seq: u64,
}

impl Journal {
    pub fn new() -> Self {
        let mut state = initial();
        state["Baseline"]["B0"] = json!({"type":"Baseline","id":"B0"});
        Self { state, seq: 0 }
    }
    pub fn event(&mut self, kind: &str, actor: &str, layer: &str, obj: &str, data: Value, guards: &[Fence]) -> Event {
        self.seq += 1;
        serde_json::from_value(json!({"seq":self.seq,"eid":format!("e-{}",self.seq),"host":"h",
            "t":format!("2026-10-22T10:{:02}:00Z",self.seq),"actor":actor,"type":kind,"layer":layer,"obj":obj,
            "data":data,"epoch":null,"stale":false,"cost_usd":null,"guards":guards})).unwrap()
    }
    /// Appends an event signed by the given actor; errors leave the state as is.
    pub fn send(&mut self, kind: &str, actor: &str, layer: &str, obj: &str, data: Value) -> Result<Event, String> {
        let event = self.event(kind, actor, layer, obj, data, &[]);
        self.apply(&event)?;
        Ok(event)
    }
    pub fn apply(&mut self, event: &Event) -> Result<(), String> {
        let mut next = self.state.clone();
        apply(&mut next, event)?;
        self.state = next;
        Ok(())
    }
    pub fn create(&mut self, input: TaskDraft) {
        let state = if input.clarify || input.acceptance.is_empty() { TaskState::Proposed } else { TaskState::Ready };
        let id = input.id.clone();
        let task = Task { input, origin: TaskOrigin::Cli, baseline: "towermd/B0".into(), state,
            needs_review: false, active_run: None, gate_failures: 0, interrupt: None };
        let event = self.event("task.created", "process:towermd-cli", "L4", &id,
            json!(TaskEvent::Created { task }), &[]);
        self.apply(&event).unwrap();
    }
    pub fn run(&self, task: &str, run: &str, source: Option<HarnessSource>) -> Run {
        Run { binding: RunBinding { id: run.into(), host: "h".into(), worktree: format!("/tmp/{run}"),
                branch: format!("t/{task}"), self_reported: source },
            task: task.into(), module: "A".into(), baseline: "towermd/B0".into(),
            fences: vec![Fence { name: "module:A".into(), holder: run.into(), epoch: 3 }],
            state: RunState::Active, cost_usd: None, high_risk: false, dispatch: None }
    }
    pub fn claim(&mut self, task: &str, run: &str, source: Option<HarnessSource>) -> Result<Run, String> {
        let run = self.run(task, run, source);
        let event = self.event("task.claimed", "process:towermd-cli", "L4", task,
            json!(TaskEvent::Claimed { run: run.clone() }), &run.fences);
        self.apply(&event)?;
        Ok(run)
    }
    pub fn finish(&mut self, run: &Run, evidence: &str, claims: Vec<Claim>) -> Result<Event, String> {
        let request = FinishRequest { evidence_id: evidence.into(), run: run.clone(), requested_at_ms: 1_000,
            deadline_ms: 1_000 + VERIFYING_TTL_MS, started_eid: "s".into(), completion_eid: "c".into(),
            interrupted_eid: "i".into(), claims, report: None };
        let event = self.event("task.finish.requested", "process:towermd-cli", "L4", evidence,
            json!(request), &run.fences);
        self.apply(&event)?;
        Ok(event)
    }
    /// Stand-in for the verifier and the main admission proof, which need a
    /// real repository: marks the run merged with a passed check.
    pub fn merge_by_hand(&mut self, run: &Run, evidence: &str, tree: &str) {
        self.seq += 1;
        let state = &mut self.state;
        state["Task"][&run.task]["state"] = json!("merged");
        state["Task"][&run.task]["active_run"] = Value::Null;
        state["Run"][&run.binding.id]["state"] = json!("released");
        let entry = &mut state["Evidence"][evidence];
        entry["status"] = json!("merged");
        entry["input"] = json!({"tree": tree});
        entry["evidence"] = json!({"passed": true, "input": {"tree": tree}});
        entry["completion"] = json!({"actor":"process:towermd-verifier","t":"2026-10-22T11:00:00Z","eid":"done-check"});
        entry["merge"] = json!({"tree": tree});
        state[TIMING][&run.binding.id]["done"] = json!({"at":format!("2026-10-22T10:{:02}:00Z",self.seq),
            "eid":format!("e-{}",self.seq),"seq":self.seq});
    }
}

pub(super) fn draft(id: &str) -> TaskDraft {
    TaskDraft { id: id.into(), title: format!("{id} title"), module: "A".into(),
        allowed_paths: vec!["src/a/**".into()], acceptance: vec!["first".into(), "second".into()],
        ..TaskDraft::default() }
}

#[test]
fn claims_record_times_from_events_and_finish_claims_must_target_criteria() {
    let mut journal = Journal::new();
    journal.create(draft("T-1"));
    let run = journal.claim("T-1", "R-1", None).unwrap();
    let timing = &timings(&journal.state).unwrap()["R-1"];
    assert_eq!((timing.task.as_str(), timing.claimed.eid.as_str(), timing.claimed.seq), ("T-1", "e-2", 2));
    assert_eq!(timing.claimed.at, "2026-10-22T10:02:00Z");
    assert!(timing.delivered.is_none() && timing.done.is_none());
    for claims in [vec![Claim { criterion: 2, note: "x".into() }], vec![Claim { criterion: 0, note: " ".into() }]] {
        assert!(journal.finish(&run, "E-1", claims).unwrap_err().contains("自述"));
    }
    let event = journal.finish(&run, "E-1", vec![Claim { criterion: 1, note: "ran it".into() }]).unwrap();
    assert_eq!(journal.state["Evidence"]["E-1"]["request"]["claims"][0]["note"], "ran it");
    let timing = &timings(&journal.state).unwrap()["R-1"];
    assert_eq!(timing.delivered.as_ref().unwrap().eid, event.eid);
    assert_eq!(timing.finishes["E-1"].at, event.t);
}

#[test]
fn finish_requests_without_claims_keep_the_v0_1_1_shape() {
    let journal = Journal::new();
    let request = FinishRequest { evidence_id: "E".into(), run: journal.run("T-1", "R-1", None), requested_at_ms: 1,
        deadline_ms: 2, started_eid: "s".into(), completion_eid: "c".into(), interrupted_eid: "i".into(), claims: vec![],
        report: None };
    assert!(json!(request).get("claims").is_none() && json!(request).get("report").is_none());
}

fn promoted(origin: TaskOrigin, target: &str) -> Value {
    let task = Task { input: draft("T-9"), origin, baseline: "towermd/B0".into(), state: TaskState::Ready,
        needs_review: false, active_run: None, gate_failures: 0, interrupt: None };
    json!({"as":"task","target":target,"object":TaskEvent::Created { task }})
}

#[test]
fn a_person_promotes_an_idea_as_a_task_in_one_event() {
    let mut journal = Journal::new();
    let idea = "0192c2d0-0000-7000-8000-000000000001";
    journal.send("idea.added", "claude-code/2.1", "L4", idea, json!({"text":"给周报加导出"})).unwrap();
    assert!(tasks(&journal.state).unwrap().tasks.is_empty(), "想法不是任务，不能被领取");
    assert!(journal.claim(idea, "R-0", None).is_err());
    for (actor, data) in [
        ("process:towermd-cli", promoted(TaskOrigin::AppHuman, "T-9")),
        ("human:local", promoted(TaskOrigin::Cli, "T-9")),
        ("human:local", promoted(TaskOrigin::AppHuman, "T-other")),
        ("human:local", json!({"as":"task","target":"T-9"})),
    ] {
        assert!(journal.send("idea.promoted", actor, "L4", idea, data.clone()).is_err(), "{actor} {data}");
    }
    assert!(journal.state["Idea"][idea].get("promoted").is_none());
    let event = journal.send("idea.promoted", "human:local", "L4", idea, promoted(TaskOrigin::AppHuman, "T-9")).unwrap();
    let created = &tasks(&journal.state).unwrap().tasks["T-9"];
    assert_eq!((created.origin, created.state), (TaskOrigin::AppHuman, TaskState::Ready));
    assert_eq!(journal.state["Idea"][idea]["promoted"]["target"], "T-9");
    assert_eq!(promoted_task(&event.kind, &event.data).unwrap().unwrap().input.id, "T-9");
    journal.claim("T-9", "R-9", None).unwrap();
}

#[test]
fn promoting_as_a_task_needs_a_published_baseline() {
    let mut journal = Journal::new();
    journal.state["Baseline"] = json!({});
    let idea = "0192c2d0-0000-7000-8000-000000000001";
    journal.send("idea.added", "human:local", "L4", idea, json!({"text":"x"})).unwrap();
    let error = journal.send("idea.promoted", "human:local", "L4", idea, promoted(TaskOrigin::AppHuman, "T-9")).unwrap_err();
    assert_eq!(error, "任务没有已出版基线");
}

#[path = "delegation_tests.rs"]
mod delegation;

#[path = "run_record_tests.rs"]
mod run_record;

#[path = "schedule_tests.rs"]
mod schedule;
#[path = "permissions_tests.rs"]
mod permissions;

#[path = "task_interrupts_tests.rs"]
mod interrupts;

#[path = "task_reviews_tests.rs"]
mod reviews;
