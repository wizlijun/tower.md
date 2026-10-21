//! Read-only run timeline. Select canonical originals, never pending envelopes,
//! projection-only receipts or arbitrary paths supplied by the caller.
use super::business::{ready, uncertain};
use super::*;
use towermd_core::{hub::Event, project_controls::ControlEvent, question_model::QuestionEvent};
use towermd_devkit::{dev_state, task_finish::{self, FinishRequest, MergeProof}, task_model::{Run, TaskEvent}, verifier::VerificationReport};
use std::collections::BTreeSet;

#[derive(Serialize)]
pub struct TimelineEvent {
    eid:String, seq:u64, at:String, actor:String, kind:String,
    object:String, stale:bool, evidence_id:Option<String>,
}
#[derive(Serialize)]
pub struct EvidenceDetail {
    id:String, completion_eid:String, revision:String, tree:String,
    passed:bool, report_sha256:String, report:VerificationReport,
}
#[derive(Serialize)]
pub struct RunMessage {
    kind: &'static str, id:String, text:String, at:String, actor:String,
}
#[derive(Serialize)]
pub struct RunDetails {
    run_id:String, task_id:String, seq:u64,
    events:Vec<TimelineEvent>, evidence:Vec<EvidenceDetail>, messages:Vec<RunMessage>, harness_output:Option<String>,
}

fn decode<T:serde::de::DeserializeOwned>(event:&Event)->Result<T,String> {
    serde_json::from_value(event.data.clone()).map_err(|error|format!("原件 {} 的结构无效：{error}",event.eid))
}
fn collect<'a>(run:&Run,seq:u64,originals:impl Iterator<Item=&'a Event>)->Result<RunDetails,String> {
    let mut originals=originals.collect::<Vec<_>>();
    originals.sort_by(|a,b|(a.seq,&a.eid).cmp(&(b.seq,&b.eid)));
    let mut holds=BTreeSet::new();
    let mut evidence_ids=BTreeSet::new();
    let mut claim_seq=None;
    for event in &originals {
        if event.stale {continue;}
        match event.kind.as_str() {
            "task.claimed"=>if let TaskEvent::Claimed{run:claimed}=decode(event)? {
                if claimed.binding.id==run.binding.id {
                    if claimed.binding!=run.binding || claimed.task!=run.task || claimed.module!=run.module || claimed.fences!=run.fences {
                        return Err("运行投影与实际领取原件不同".into());
                    }
                    claim_seq=Some(event.seq);
                }
            },
            "hold.opened"|"hold.decision_reused"=>match decode(event)? {
                QuestionEvent::Opened{question}|QuestionEvent::DecisionReused{question,..} if question.binding.run==run.binding.id=>{
                    if question.binding.task!=run.task || question.binding.scope!=run.module {return Err("问题绑定与运行不同".into());}
                    holds.insert(question.draft.id);
                },
                _=>{},
            },
            "task.finish.requested"=>{
                let request:FinishRequest=decode(event)?;
                if request.run.binding.id==run.binding.id {evidence_ids.insert(request.evidence_id);}
            },
            _=>{},
        }
    }
    let claim_seq=claim_seq.ok_or("运行缺少 canonical 领取原件")?;
    let mut result=RunDetails{run_id:run.binding.id.clone(),task_id:run.task.clone(),seq,
        events:vec![],evidence:vec![],messages:vec![],harness_output:None};
    let mut active=false;
    for event in originals {
        if event.is_pure_heartbeat() {continue;}
        let related=if event.stale {
            // Invalid stale bodies never become accepted evidence. A complete
            // matching old fence still permits displaying their stale metadata.
            event.guards.iter().any(|fence|run.fences.contains(fence))
                || holds.contains(&event.obj) || evidence_ids.contains(&event.obj)
        } else {
            match event.kind.as_str() {
                "task.created"=>matches!(decode(event)?,TaskEvent::Created{task} if task.input.id==run.task),
                "task.claimed"=>{
                    let same=event.seq==claim_seq;
                    if same {active=true;} same
                },
                "task.released"=>{
                    let same=matches!(decode(event)?,TaskEvent::Released{run_id,..} if run_id==run.binding.id);
                    if same {active=false;} same
                },
                "task.finish.requested"|"task.finish.started"|"task.finish.completed"|"task.finish.interrupted"|"task.g5.completed"=>evidence_ids.contains(&event.obj),
                "task.merged"=>{
                    let proof:MergeProof=decode(event)?;
                    let same=proof.run_id==run.binding.id && proof.task_id==run.task;
                    if same {active=false;} same
                },
                "hold.opened"|"hold.answered"|"hold.decision_reused"=>holds.contains(&event.obj),
                "prompt.queued"=>matches!(decode(event)?,QuestionEvent::PromptQueued{prompt} if prompt.recipients.iter().any(|recipient|recipient.run==run.binding.id && recipient.task==run.task && recipient.scope==run.module)),
                "project.paused"|"project.resumed"=>matches!(decode(event)?,ControlEvent::Paused{prompt,..}|ControlEvent::Resumed{prompt,..} if prompt.recipients.iter().any(|recipient|recipient.run==run.binding.id && recipient.task==run.task && recipient.scope==run.module)),
                "prompt.delivered"=>matches!(decode(event)?,QuestionEvent::Delivered{delivery} if delivery.run==run.binding.id),
                "lease.granted"|"lease.renewed"=>event.updated_leases().iter().any(|lease|run.fences.contains(&lease.fence())),
                "lease.released"|"lease.expired"=>{
                    let same=event.released_fences().iter().any(|fence|run.fences.contains(fence));
                    if same {active=false;} same
                },
                "recovery.completed"=>{
                    let affected=active && event.seq>claim_seq;
                    if affected {active=false;} affected
                },
                _=>false,
            }
        };
        if !related {continue;}
        if !event.stale && matches!(event.kind.as_str(),"hold.opened"|"hold.answered"|"hold.decision_reused"|"prompt.queued") {
            let mut push=|kind,id,text|result.messages.push(RunMessage{kind,id,text,at:event.t.clone(),actor:event.actor.clone()});
            match decode(event)? {
                QuestionEvent::Opened{question}=>push("question",question.draft.id,question.draft.text),
                QuestionEvent::Answered{answer}=>push("answer",event.eid.clone(),answer.text),
                QuestionEvent::AnswerRelayed{text,..}=>push("answer",event.eid.clone(),text),
                QuestionEvent::DecisionReused{question,decision}=>{
                    push("question",question.draft.id,question.draft.text);
                    push("answer",event.eid.clone(),decision.decision);
                },
                QuestionEvent::PromptQueued{prompt}=>push("prompt",prompt.id,prompt.text),
                _=>return Err("消息原件类型与信封不同".into()),
            }
        }
        if !event.stale && event.kind==task_finish::FINISH_EVENT {
            let evidence=task_finish::completion_from_event(event)?;
            if evidence.input.run.binding.id!=run.binding.id || evidence.input.task.input.id!=run.task {
                return Err("验证原件与选定运行不同".into());
            }
            result.evidence.push(EvidenceDetail{id:evidence.input.evidence_id,completion_eid:event.eid.clone(),
                revision:evidence.input.revision,tree:evidence.input.tree,passed:evidence.passed,
                report_sha256:evidence.report_sha256,report:evidence.report});
        }
        if !event.stale && matches!(event.kind.as_str(),"project.paused"|"project.resumed") {
            match decode(event)? {
                ControlEvent::Paused{prompt,..}|ControlEvent::Resumed{prompt,..}=>result.messages.push(RunMessage{
                    kind:"prompt",id:prompt.id,text:prompt.text,at:event.t.clone(),actor:event.actor.clone()}),
                _=>return Err("控制消息原件类型与信封不同".into()),
            }
        }
        result.events.push(TimelineEvent{eid:event.eid.clone(),seq:event.seq,at:event.t.clone(),actor:event.actor.clone(),
            kind:event.kind.clone(),object:event.obj.clone(),stale:event.stale,
            evidence_id:if evidence_ids.contains(&event.obj){Some(event.obj.clone())}else{None}});
    }
    Ok(result)
}

pub async fn events(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>)->Result<Json<Value>,ApiError> {
    if !authorized_client(&headers,&server) {return Err(denied());}
    if id.is_empty() || id.len()>128 || !id.bytes().all(|byte|byte.is_ascii_alphanumeric() || matches!(byte,b'-'|b'_'|b'.')) {
        return Err(bad("运行 ID 无效"));
    }
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        if slot.is_none() && !project.common.join("towermd/runtime.json").exists()
            && towermd_core::main_writer::latest_snapshot(&project.root).map_err(bad)?.is_none() {
            return Err((StatusCode::NOT_FOUND,Json(json!({"error":"项目尚无运行"}))));
        }
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let tasks=dev_state::tasks(runtime.projection()).map_err(uncertain)?;
        let run=tasks.runs.get(&id).ok_or_else(||(StatusCode::NOT_FOUND,Json(json!({"error":"运行不存在"}))))?;
        // Do not tick/renew or open any log path to serve this read.
        let details=collect(run,runtime.contiguous_seq(),runtime.canonical_receipts()).map_err(uncertain)?;
        Ok(Json(serde_json::to_value(details).map_err(uncertain)?))
    }).await.map_err(uncertain)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use towermd_core::{hub::{AppendReceipt,Hub,NewEvent}, hub_protocol::{AcquireLease,HubProtocol}, question_model::{prepare_question,QuestionDraft,RunContext,WaitMode}};
    use towermd_devkit::{task_model::{RunBinding,RunState}};
    fn run(id:&str)->Run {
        Run{binding:RunBinding{id:id.into(),host:"local".into(),worktree:"/not-read".into(),branch:"t/T-one".into(),self_reported:None},
            task:"T-one".into(),module:"A-one".into(),baseline:"towermd/B0".into(),fences:vec![],state:RunState::Active,cost_usd:None}
    }
    fn append(hub:&mut Hub,kind:&str,obj:&str,data:Value,at:u64)->Event {
        let AppendReceipt::Event(event)=hub.append(NewEvent{eid:Uuid::now_v7().to_string(),host:"local".into(),actor:"process:test".into(),kind:kind.into(),layer:"L5".into(),obj:obj.into(),data,guards:vec![]},at).unwrap() else {panic!()};
        *event
    }
    fn claim(hub:&mut Hub,run:&Run,at:u64)->Event {
        append(hub,"task.claimed",&run.task,serde_json::to_value(TaskEvent::Claimed{run:run.clone()}).unwrap(),at)
    }
    #[test]
    fn selects_only_original_events_of_the_run_and_omits_private_payloads() {
        let dir=tempfile::tempdir().unwrap();let mut hub=Hub::open(dir.path()).unwrap();
        let one=run("R-one");let two=run("R-two");
        let claimed=claim(&mut hub,&one,10);claim(&mut hub,&two,11);
        let question=prepare_question(QuestionDraft{id:"H-one".into(),text:"Question?".into(),options:vec![],recommended:None,context:vec![],mode:WaitMode::Live},
            RunContext{run:"R-one".into(),task:"T-one".into(),scope:"A-one".into()},vec![],12).unwrap();
        let hold=append(&mut hub,"hold.opened","H-one",serde_json::to_value(QuestionEvent::Opened{question}).unwrap(),12);
        append(&mut hub,"alert.raised","unrelated",json!({"token":"never-expose","path":"/private/file"}),13);
        let events=hub.events(0);
        let result=collect(&one,hub.contiguous_seq(),events.iter().rev()).unwrap();
        assert_eq!(result.events.iter().map(|event|&event.eid).collect::<Vec<_>>(),vec![&claimed.eid,&hold.eid]);
        assert!(result.evidence.is_empty());assert!(result.harness_output.is_none());
        let serialized=serde_json::to_string(&result).unwrap();
        assert!(!serialized.contains("never-expose"));assert!(!serialized.contains("/private/file"));assert!(!serialized.contains("/not-read"));
        let mut wrong=one.clone();wrong.binding.branch="t/T-other".into();
        assert!(collect(&wrong,hub.contiguous_seq(),events.iter()).is_err());
        assert!(collect(&run("R-unknown"),hub.contiguous_seq(),events.iter()).is_err());
    }
    #[test]
    fn lease_expiration_uses_full_original_fence_not_shared_module_name() {
        let dir=tempfile::tempdir().unwrap();let mut hub=Hub::open(dir.path()).unwrap();
        let mut one=run("R-one");
        hub.lease_acquire(AcquireLease{host:"local".into(),holder:"R-one".into(),names:vec!["module:A-one".into()],ttl_ms:100},1000).unwrap();
        one.fences=hub.leases(1000).values().map(|lease|lease.fence()).collect();
        claim(&mut hub,&one,1001);
        hub.subscribe(hub.contiguous_seq(),1101).unwrap();
        let original_expired=hub.events(0).into_iter().find(|event|event.kind=="lease.expired").unwrap();
        hub.lease_acquire(AcquireLease{host:"local".into(),holder:"R-two".into(),names:vec!["module:A-one".into()],ttl_ms:100},1200).unwrap();
        hub.subscribe(hub.contiguous_seq(),1301).unwrap();
        let events=hub.events(0);let result=collect(&one,hub.contiguous_seq(),events.iter()).unwrap();
        let expiration=result.events.iter().filter(|event|event.kind=="lease.expired").collect::<Vec<_>>();
        assert_eq!(expiration.len(),1);assert_eq!(expiration[0].eid,original_expired.eid);
    }
    #[test]
    fn message_text_requires_original_run_binding_and_excludes_stale_and_other_runs() {
        use towermd_core::question_model::{AnswerInput,RememberAs,Prompt,PromptScope};
        use towermd_core::hub::Fence;
        let dir=tempfile::tempdir().unwrap();let mut hub=Hub::open(dir.path()).unwrap();
        let one=run("R-one");let two=run("R-two");claim(&mut hub,&one,1);claim(&mut hub,&two,2);
        for (id,run_id,text) in [("H-one","R-one","Original question?"),("H-two","R-two","Other private question?")] {
            let question=prepare_question(QuestionDraft{id:id.into(),text:text.into(),options:vec![],recommended:None,context:vec![],mode:WaitMode::Live},
                RunContext{run:run_id.into(),task:"T-one".into(),scope:"A-one".into()},vec![],10).unwrap();
            append(&mut hub,"hold.opened",id,serde_json::to_value(QuestionEvent::Opened{question}).unwrap(),10);
        }
        let answer=append(&mut hub,"hold.answered","H-one",serde_json::to_value(QuestionEvent::Answered{answer:AnswerInput{
            hold:"H-one".into(),text:"Original answer\nSecond line".into(),remember_as:RememberAs::Temporary,targets:vec![],
        }}).unwrap(),11);
        append(&mut hub,"hold.answered","H-two",serde_json::to_value(QuestionEvent::Answered{answer:AnswerInput{
            hold:"H-two".into(),text:"Other private answer".into(),remember_as:RememberAs::Temporary,targets:vec![],
        }}).unwrap(),12);
        for (id,run_id,text) in [("P-one","R-one","Original prompt"),("P-two","R-two","Other private prompt")] {
            append(&mut hub,"prompt.queued",id,serde_json::to_value(QuestionEvent::PromptQueued{prompt:Prompt{
                id:id.into(),scope:PromptScope::Global,text:text.into(),recipients:vec![RunContext{run:run_id.into(),task:"T-one".into(),scope:"A-one".into()}],targets:vec![],
            }}).unwrap(),13);
        }
        let stale=hub.append(NewEvent{eid:Uuid::now_v7().to_string(),host:"local".into(),actor:"human:local".into(),kind:"hold.answered".into(),layer:"L5".into(),obj:"H-one".into(),
            data:serde_json::to_value(QuestionEvent::Answered{answer:AnswerInput{hold:"H-one".into(),text:"Stale answer must not be delivered".into(),remember_as:RememberAs::Temporary,targets:vec![]}}).unwrap(),
            guards:vec![Fence{name:"module:A-one".into(),holder:"R-one".into(),epoch:999}]},14).unwrap();
        assert!(matches!(stale,AppendReceipt::Event(event) if event.stale));
        let events=hub.events(0);let details=collect(&one,hub.contiguous_seq(),events.iter()).unwrap();
        assert_eq!(details.messages.iter().map(|message|(message.kind,message.text.as_str())).collect::<Vec<_>>(),vec![
            ("question","Original question?"),("answer","Original answer\nSecond line"),("prompt","Original prompt")]);
        assert_eq!(details.messages[1].id,answer.eid);assert_eq!(details.messages[1].at,answer.t);
        assert_eq!(details.messages[1].actor,answer.actor);
        assert!(details.events.iter().any(|event|event.stale));
    }

}
