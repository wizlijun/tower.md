use serde_json::{json, Value};
use towermd_core::{
    hub::Event,
    kit::{Kit, KitRun},
    project_controls, project_state,
    question_model::RunContext,
};

struct Widget;
impl Kit for Widget {
    fn state_groups(&self) -> &'static [&'static str] {
        &["Widget"]
    }
}

#[test]
fn core_initial_holds_only_core_groups() {
    assert_eq!(
        project_state::core_initial(),
        json!({"Hold":{}, "Proposal":{}, "Prompt":{}, "Alert":{}})
    );
}

#[test]
fn kit_groups_join_the_initial_projection() {
    let state = project_state::initial_with(&[&Widget]);
    assert_eq!(state["Widget"], json!({}));
    assert_eq!(state.as_object().unwrap().len(), 5);
}

struct Counter;
impl Kit for Counter {
    fn state_groups(&self) -> &'static [&'static str] {
        &["Counter"]
    }
    fn reduce(&self, state: &mut Value, event: &Event) -> Result<bool, String> {
        if event.kind != "counter.bumped" {
            return Ok(false);
        }
        state["Counter"] = json!(state["Counter"].as_u64().unwrap_or(0) + 1);
        Ok(true)
    }
    fn runs(&self, _: &Value) -> Result<Vec<KitRun>, String> {
        Ok(vec![KitRun { active: true, context: RunContext{run:"R-one".into(),task:"T-one".into(),module:"A-one".into()} }])
    }
}

fn bumped() -> Event {
    serde_json::from_value(json!({"seq":1,"eid":"e-1","host":"test","t":"2026-10-21T12:00:00Z",
        "actor":"process:test","type":"counter.bumped","layer":"L4","obj":"C-1","data":{},
        "epoch":null,"stale":false,"cost_usd":null,"guards":[]}))
    .unwrap()
}

#[test]
fn unknown_business_events_reach_the_kits() {
    let mut state = project_state::initial_with(&[&Counter]);
    project_state::apply_with(&[&Counter], &mut state, &bumped()).unwrap();
    assert_eq!(state["Counter"], json!(1));
}

#[test]
fn core_alone_rejects_kit_events() {
    let mut state = project_state::core_initial();
    assert_eq!(
        project_state::core_apply(&mut state, &bumped()).unwrap_err(),
        "尚未支持的业务事件：counter.bumped"
    );
}

#[test]
fn pause_recipients_come_from_kit_runs() {
    let state = project_state::initial_with(&[&Counter]);
    let recipients = |kits: &[&dyn Kit]| match project_controls::prepare_pause(&state, "E-pause", kits).unwrap() {
        project_controls::ControlEvent::Paused { prompt, .. } => {
            prompt.recipients.into_iter().map(|recipient| recipient.run).collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(recipients(&[&Counter]), ["R-one"]);
    assert!(recipients(&[]).is_empty());
}
