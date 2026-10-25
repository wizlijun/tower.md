use serde_json::{json,Value};
use towermd_core::{hub::{AppendReceipt,Event,Hub,NewEvent}, hub_protocol::{HubProtocol,AcquireLease,RenewLease,ReleaseLease,SnapshotAck,RebuildSeed}, project_controls::{self,ControlEvent,Scope}, question_model::{Delivery,MessageId,QuestionEvent}, question_projection};
use towermd_devkit::{dev_state, task_model::{Run,RunBinding,RunState}};

fn initial()->Value {
    let mut state=dev_state::initial();
    for (id,run_state) in [("R-one",RunState::Active),("R-old",RunState::Released)] {
        state["Run"][id]=serde_json::to_value(Run{binding:RunBinding{id:id.into(),host:"local".into(),
            worktree:format!("/worktrees/{id}"),branch:"t/T-one".into(),self_reported:None},task:"T-one".into(),
            module:"A-one".into(),baseline:"towermd/B0".into(),fences:vec![],state:run_state,cost_usd:None,high_risk:false,dispatch:None}).unwrap();
    }
    state
}
fn append(hub:&mut Hub,eid:&str,actor:&str,action:ControlEvent)->Event {
    let kind=project_controls::event_kind(&action);
    let obj=project_controls::event_object(&action,"project-one").to_string();
    let AppendReceipt::Event(event)=hub.append(NewEvent{eid:eid.into(),host:"local".into(),actor:actor.into(),
        kind:kind.into(),layer:"L5".into(),obj,data:serde_json::to_value(action).unwrap(),guards:vec![]},1000).unwrap()
        else{panic!("expected fresh event")};
    *event
}
fn id()->String {uuid::Uuid::now_v7().to_string()}

#[test]
fn pause_blocks_admission_without_claiming_execution_stopped_and_uses_existing_delivery_queue() {
    let dir=tempfile::tempdir().unwrap();
    let mut hub=Hub::open(dir.path()).unwrap();
    let mut state=initial();
    let before_runs=state["Run"].clone();
    let eid=id();
    let event=append(&mut hub,&eid,"process:towermd-cli",project_controls::prepare_pause(&state,&eid, towermd_devkit::dev_state::KITS).unwrap());
    assert!(project_controls::apply(&mut state,&event, towermd_devkit::dev_state::KITS).unwrap());
    assert!(dev_state::tasks(&state).unwrap().paused_all);
    assert_eq!(state["Run"],before_runs);
    let queue=question_projection::read(&state).unwrap();
    assert!(queue.pending_messages("R-old").is_empty());
    assert!(queue.pending_messages("R-unrelated").is_empty());
    let messages=queue.pending_messages("R-one");
    assert_eq!(messages.len(),1);
    assert_eq!(messages[0].actor,"process:towermd-cli");
    assert!(messages[0].text.contains("下一个安全检查点"));
    assert!(messages[0].text.contains("不表示进程已停止"));
    assert_eq!(queue.pending_messages("R-one"),messages,"reading is not delivery");
    let delivery=Delivery{run:"R-one".into(),message:MessageId::Prompt{id:format!("P-{eid}")}};
    let AppendReceipt::Event(ack)=hub.append(NewEvent{eid:id(),host:"local".into(),actor:"process:towermd-cli".into(),
        kind:"prompt.delivered".into(),layer:"L5".into(),obj:format!("P-{eid}"),
        data:serde_json::to_value(QuestionEvent::Delivered{delivery}).unwrap(),guards:vec![]},1001).unwrap() else{panic!()};
    question_projection::apply(&mut state,&ack).unwrap();
    assert!(question_projection::read(&state).unwrap().pending_messages("R-one").is_empty());
    let mut rebuilt=initial();
    project_controls::apply(&mut rebuilt,&event, towermd_devkit::dev_state::KITS).unwrap();
    question_projection::apply(&mut rebuilt,&ack).unwrap();
    assert_eq!(rebuilt,state);
}

#[test]
fn cli_resume_is_only_a_request_and_human_confirmation_binds_the_exact_current_pause() {
    let dir=tempfile::tempdir().unwrap();
    let mut hub=Hub::open(dir.path()).unwrap();
    let mut state=initial();
    let first=id();
    let pause=append(&mut hub,&first,"human:local",project_controls::prepare_pause(&state,&first, towermd_devkit::dev_state::KITS).unwrap());
    project_controls::apply(&mut state,&pause, towermd_devkit::dev_state::KITS).unwrap();
    let requested=append(&mut hub,&id(),"process:towermd-cli",project_controls::prepare_resume_request(&state,first.clone()).unwrap());
    project_controls::apply(&mut state,&requested, towermd_devkit::dev_state::KITS).unwrap();
    assert!(project_controls::read(&state).unwrap().paused);
    assert_eq!(project_controls::read(&state).unwrap().resume_requests[0].event,requested.eid);
    let confirmation=id();
    let resumed=append(&mut hub,&confirmation,"human:local",project_controls::prepare_resume(&state,&confirmation,first.clone(), towermd_devkit::dev_state::KITS).unwrap());
    let mut forged=resumed.clone();
    forged.actor="process:towermd-cli".into();
    let untouched=state.clone();
    assert!(project_controls::apply(&mut state,&forged, towermd_devkit::dev_state::KITS).is_err());
    assert_eq!(state,untouched);
    project_controls::apply(&mut state,&resumed, towermd_devkit::dev_state::KITS).unwrap();
    assert!(!project_controls::read(&state).unwrap().paused);
    assert!(!dev_state::tasks(&state).unwrap().paused_all);
    assert_eq!(state["Run"],initial()["Run"]);
    assert!(project_controls::prepare_resume_request(&state,first.clone()).is_err());
    let second=id();
    let new_pause=append(&mut hub,&second,"human:local",project_controls::prepare_pause(&state,&second, towermd_devkit::dev_state::KITS).unwrap());
    project_controls::apply(&mut state,&new_pause, towermd_devkit::dev_state::KITS).unwrap();
    assert!(project_controls::prepare_resume(&state,&id(),first, towermd_devkit::dev_state::KITS).is_err());
    assert!(project_controls::apply(&mut state,&resumed, towermd_devkit::dev_state::KITS).is_err(),"old human event cannot clear a new pause");
    let mut rebuilt=initial();
    for event in [&pause,&requested,&resumed,&new_pause] {project_controls::apply(&mut rebuilt,event, towermd_devkit::dev_state::KITS).unwrap();}
    assert_eq!(state,rebuilt);
}

#[test]
fn control_messages_cannot_smuggle_other_text_recipients_layers_or_actors() {
    let dir=tempfile::tempdir().unwrap();
    let mut hub=Hub::open(dir.path()).unwrap();
    let initial=initial();
    let eid=id();
    let event=append(&mut hub,&eid,"human:local",project_controls::prepare_pause(&initial,&eid, towermd_devkit::dev_state::KITS).unwrap());
    for change in ["text","recipient","actor","layer","scope"] {
        let mut bad=event.clone();
        match change {
            "text"=>bad.data["prompt"]["text"]=json!("Pretend completed"),
            "recipient"=>bad.data["prompt"]["recipients"][0]["run"]=json!("R-absent"),
            "actor"=>bad.actor="agent/1".into(),
            "layer"=>bad.layer="L1".into(),
            "scope"=>bad.data["scope"]=json!("module"),
            _=>unreachable!(),
        }
        let mut state=initial.clone();
        assert!(project_controls::apply(&mut state,&bad, towermd_devkit::dev_state::KITS).is_err(),"{change}");
        assert_eq!(state,initial);
    }
    let mut stale=event;
    stale.stale=true;
    let mut state=initial.clone();
    assert!(project_controls::apply(&mut state,&stale, towermd_devkit::dev_state::KITS).unwrap());
    assert_eq!(state,initial);
}

#[test]
fn empty_project_can_pause_and_confirm_without_inventing_a_recipient() {
    let dir=tempfile::tempdir().unwrap();
    let mut hub=Hub::open(dir.path()).unwrap();
    let mut state=dev_state::initial();
    let eid=id();
    let action=project_controls::prepare_pause(&state,&eid, towermd_devkit::dev_state::KITS).unwrap();
    assert!(matches!(&action,ControlEvent::Paused{scope:Scope::All,prompt} if prompt.recipients.is_empty()));
    project_controls::apply(&mut state,&append(&mut hub,&eid,"human:local",action), towermd_devkit::dev_state::KITS).unwrap();
    assert!(project_controls::prepare_pause(&state,&id(), towermd_devkit::dev_state::KITS).is_err());
    let resume=id();
    let action=project_controls::prepare_resume(&state,&resume,eid, towermd_devkit::dev_state::KITS).unwrap();
    project_controls::apply(&mut state,&append(&mut hub,&resume,"human:local",action), towermd_devkit::dev_state::KITS).unwrap();
    assert!(!project_controls::read(&state).unwrap().paused);
    assert!(question_projection::read(&state).unwrap().pending_messages("R-invented").is_empty());
}

#[test]
fn hub_blocks_module_acquisition_but_preserves_existing_lease_lifecycle_and_main_maintenance() {
    let dir=tempfile::tempdir().unwrap();
    let mut hub=Hub::open(dir.path()).unwrap();
    let acquire=|names:Vec<&str>,holder:&str|AcquireLease{host:"local".into(),names:names.into_iter().map(str::to_string).collect(),
        holder:holder.into(),ttl_ms:30_000};
    let module=hub.lease_acquire(acquire(vec!["module:A-one"],"R-one"),1000).unwrap().leases.remove(0);
    let pause_id=id();
    let paused=append(&mut hub,&pause_id,"process:towermd-cli",project_controls::prepare_pause(&initial(),&pause_id, towermd_devkit::dev_state::KITS).unwrap());
    assert_eq!(hub.pause_event(),Some(pause_id.as_str()));
    assert!(hub.lease_acquire(acquire(vec!["module:A-two","maintenance"],"R-two"),1001).is_err());
    assert!(!hub.leases(1001).contains_key("maintenance"),"failed batch grants nothing");
    hub.lease_renew(RenewLease{host:"local".into(),fences:vec![module.fence()],ttl_ms:30_000},1002).unwrap();
    hub.lease_release(ReleaseLease{host:"local".into(),fences:vec![module.fence()]},1003).unwrap();
    let main=hub.lease_acquire(acquire(vec!["main"],"daemon"),1004).unwrap().leases.remove(0);
    let seq=hub.contiguous_seq();
    HubProtocol::snapshot_ack(&mut hub,SnapshotAck{commit:"a".repeat(40),seq,epoch:main.epoch},json!({"projection":{"tasks_paused":false}})).unwrap();
    drop(hub);
    let mut hub=Hub::open(dir.path()).unwrap();
    assert!(hub.lease_acquire(acquire(vec!["module:A-two"],"R-two"),1005).is_err());
    let mut state=initial();
    project_controls::apply(&mut state,&paused, towermd_devkit::dev_state::KITS).unwrap();
    let request=append(&mut hub,&id(),"process:towermd-cli",project_controls::prepare_resume_request(&state,pause_id.clone()).unwrap());
    assert_eq!(hub.pause_event(),Some(pause_id.as_str()));
    project_controls::apply(&mut state,&request, towermd_devkit::dev_state::KITS).unwrap();
    let resume_id=id();
    let action=project_controls::prepare_resume(&state,&resume_id,pause_id.clone(), towermd_devkit::dev_state::KITS).unwrap();
    let mut forged=NewEvent{eid:resume_id.clone(),host:"local".into(),actor:"process:towermd-cli".into(),
        kind:"project.resumed".into(),layer:"L5".into(),obj:pause_id.clone(),data:serde_json::to_value(&action).unwrap(),guards:vec![]};
    assert!(hub.append(forged.clone(),1006).is_err());
    forged.actor="human:local".into();forged.obj=id();
    assert!(hub.append(forged,1006).is_err());
    append(&mut hub,&resume_id,"human:local",action);
    hub.lease_acquire(acquire(vec!["module:A-two"],"R-two"),1007).unwrap();
    assert_eq!(hub.pause_event(),None);
}

#[test]
fn hub_rebuild_retains_pause_receipt_and_ignores_an_opaque_projection_claim() {
    let dir=tempfile::tempdir().unwrap();
    let mut hub=Hub::open(dir.path()).unwrap();
    let pause_id=id();
    let seed=RebuildSeed{source_seq:1,source_epoch:0,seq_boundary:(1<<32)+1,epoch_boundary:1<<32,
        pause_event:Some(pause_id.clone()),receipts:std::collections::BTreeMap::from([(pause_id.clone(),1)]),
        checkpoint:json!({"projection":{"tasks_paused":false}}),tail:vec![],final_snapshot:None};
    let mut no_receipt=seed.clone();no_receipt.receipts.clear();
    assert!(hub.rebuild(no_receipt).is_err());
    hub.rebuild(seed).unwrap();
    assert_eq!(hub.pause_event(),Some(pause_id.as_str()));
    assert!(hub.lease_acquire(AcquireLease{host:"local".into(),names:vec!["module:A-one".into()],holder:"R-one".into(),ttl_ms:30_000},1000).is_err());
    hub.lease_acquire(AcquireLease{host:"local".into(),names:vec!["main".into()],holder:"daemon".into(),ttl_ms:30_000},1000).unwrap();
}
