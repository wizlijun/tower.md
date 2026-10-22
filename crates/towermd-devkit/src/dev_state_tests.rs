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
            needs_review: false, active_run: None };
        let event = self.event("task.created", "process:towermd-cli", "L4", &id,
            json!(TaskEvent::Created { task }), &[]);
        self.apply(&event).unwrap();
    }
    pub fn run(&self, task: &str, run: &str, source: Option<HarnessSource>) -> Run {
        Run { binding: RunBinding { id: run.into(), host: "h".into(), worktree: format!("/tmp/{run}"),
                branch: format!("t/{task}"), self_reported: source },
            task: task.into(), module: "A".into(), baseline: "towermd/B0".into(),
            fences: vec![Fence { name: "module:A".into(), holder: run.into(), epoch: 3 }],
            state: RunState::Active, cost_usd: None }
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
            interrupted_eid: "i".into(), claims };
        let event = self.event("task.finish.requested", "process:towermd-cli", "L4", evidence,
            json!(request), &run.fences);
        self.apply(&event)?;
        Ok(event)
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
        deadline_ms: 2, started_eid: "s".into(), completion_eid: "c".into(), interrupted_eid: "i".into(), claims: vec![] };
    assert!(json!(request).get("claims").is_none());
}
