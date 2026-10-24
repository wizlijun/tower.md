use serde_json::json;
use towermd_core::{hub::{AppendReceipt,Hub,NewEvent},hub_protocol::HubProtocol,
    question_model::*,question_projection};

#[test]
fn canonical_question_replay_rebuilds_canvas_groups_without_losing_other_proposals() {
    let dir=tempfile::tempdir().unwrap();
    let mut hub=Hub::open(dir.path()).unwrap();
    let question=prepare_question(QuestionDraft{id:"H-one".into(),text:"Format?".into(),options:vec![],recommended:None,context:vec![],mode:WaitMode::Live},
        RunContext{run:"R-one".into(),task:"T-one".into(),scope:"A-one".into()},vec![],1000).unwrap();
    let actions=[("hold.opened","process:towermd-cli",QuestionEvent::Opened{question}),
        ("hold.answered","human:local",QuestionEvent::Answered{answer:AnswerInput{hold:"H-one".into(),text:"JSON".into(),remember_as:RememberAs::ModuleDecision{id:"D-one".into(),supersedes:vec![]},targets:vec![],assumption:None}})];
    let initial=json!({"Hold":{},"Prompt":{},"Proposal":{"P-other":{"id":"P-other","status":"accepted"}}});
    let mut state=initial.clone();
    let mut originals=vec![];
    for (index,(kind,actor,action)) in actions.into_iter().enumerate() {
        let AppendReceipt::Event(event)=hub.append(NewEvent{eid:uuid::Uuid::now_v7().to_string(),host:"local".into(),actor:actor.into(),kind:kind.into(),layer:"L5".into(),obj:"H-one".into(),data:serde_json::to_value(action).unwrap(),guards:vec![]},1001+index as u64).unwrap() else {panic!()};
        question_projection::apply(&mut state,&event).unwrap();
        assert_eq!(question_projection::blocks_run(&state,"R-one").unwrap(),index==0);
        assert_eq!(question_projection::blocks_task(&state,"T-one").unwrap(),index==0);
        assert!(!question_projection::blocks_run(&state,"R-other").unwrap());
        originals.push(event);
    }
    assert_eq!(state["Hold"]["H-one"]["binding"]["module"],"A-one");
    assert_eq!(state["Answer"]["H-one"]["input"]["text"],"JSON");
    assert_eq!(state["QuestionDecision"]["D-one"]["accepted_by"],"human:local");
    assert_eq!(state["Proposal"],initial["Proposal"]);
    let restored:serde_json::Value=serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert_eq!(question_projection::read(&restored).unwrap().pending_messages("R-one").len(),1);
    let mut replay=initial;
    for original in originals {question_projection::apply(&mut replay,&original).unwrap();}
    assert_eq!(replay,state);
}

#[test]
fn relay_checkpoint_and_full_replay_agree_without_remembered_authority() {
    let dir=tempfile::tempdir().unwrap();let mut hub=Hub::open(dir.path()).unwrap();
    let question=prepare_question(QuestionDraft{id:"H-relay".into(),text:"Format?".into(),options:vec![],recommended:None,context:vec![],mode:WaitMode::Live},
        RunContext{run:"R-one".into(),task:"T-one".into(),scope:"A-one".into()},vec![],1000).unwrap();
    let actions=[("hold.opened",QuestionEvent::Opened{question}),
        ("hold.answered",QuestionEvent::AnswerRelayed{hold:"H-relay".into(),text:"JSON".into(),relayed_from:"human".into(),self_reported:None})];
    let mut state=json!({});let mut originals=vec![];
    for (index,(kind,action)) in actions.into_iter().enumerate() {
        let AppendReceipt::Event(event)=hub.append(NewEvent{eid:uuid::Uuid::now_v7().to_string(),host:"local".into(),actor:"process:towermd-cli".into(),
            kind:kind.into(),layer:"L5".into(),obj:"H-relay".into(),data:serde_json::to_value(action).unwrap(),guards:vec![]},1001+index as u64).unwrap() else {panic!()};
        towermd_core::project_state::core_apply(&mut state,&event).unwrap();
        assert_eq!(question_projection::blocks_task(&state,"T-one").unwrap(),index==0);
        originals.push(event);
    }
    let checkpoint:serde_json::Value=serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    let restored=question_projection::read(&checkpoint).unwrap();
    assert_eq!(restored.pending_messages("R-one").len(),1);
    assert_eq!(restored.answers["H-relay"].actor,"process:towermd-cli");
    assert!(restored.decisions.is_empty()&&restored.rules.is_empty()&&restored.proposals.is_empty());
    let mut replay=json!({});
    for original in originals {towermd_core::project_state::core_apply(&mut replay,&original).unwrap();}
    assert_eq!(checkpoint,replay);
}
