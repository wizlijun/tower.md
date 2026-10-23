//! Human questions and interruption messages. Waiting never holds the project
//! mutex; reading a queue never records delivery.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{hub::{Event, Fence}, hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}, question_model::{self, AnswerInput, ChangeTarget, DecisionCatalog, Delivery, MessageId, PromptScope, QuestionDraft, QuestionEvent, RememberAs, RunContext, WaitMode, WaitPlan}, question_projection};
use towermd_devkit::{dev_state, task_model::{Run, RunState}};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum Destination { Temporary, ModuleDecision, ProcessRule, SlowProposal }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerRequest {
    request_id:String, hold:String, text:String, remember_as:Destination,
    #[serde(default)] targets:Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayRequest {
    request_id:String, cwd:PathBuf, #[serde(default)] run:Option<String>, hold:String, text:String,
    #[serde(default)] self_reported:Option<towermd_devkit::task_model::HarnessSource>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptRequest {
    request_id:String, scope:PromptScope, text:String,
    #[serde(default)] targets:Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskRequest {
    request_id:String, cwd:PathBuf, #[serde(default)] run:Option<String>, text:String,
    #[serde(default)] options:Vec<String>, #[serde(default)] recommended:Option<usize>,
    #[serde(default)] context:Vec<String>, #[serde(default)] mode:WaitMode,
    #[serde(default)] targets:Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitRequest {
    cwd:PathBuf, #[serde(default)] run:Option<String>, hold:String,
    #[serde(default)] wait_ms:Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context { cwd:PathBuf, #[serde(default)] run:Option<String> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AckRequest {request_id:String,cwd:PathBuf,run:String,message:MessageId}

fn binding(run:&Run)->RunContext {
    RunContext{run:run.binding.id.clone(),task:run.task.clone(),scope:run.module.clone()}
}
fn targets(project:&Project, ids:&[String])->Result<Vec<ChangeTarget>,String> {
    let catalog=current_catalog(project)?;
    let mut result=Vec::new();
    for id in ids {
        if result.iter().any(|target:&ChangeTarget| target.id==*id) {return Err("影响对象不能重复".into());}
        let object=catalog.objects.get(id).ok_or_else(||format!("影响对象不存在：{id}"))?;
        let layer=object.get_str("layer").ok_or("影响对象缺少明确层级，不能推测为快层")?;
        result.push(ChangeTarget{id:id.clone(),layer:layer.into()});
    }
    Ok(result)
}
fn same_targets(actual:&[ChangeTarget],input:&[String])->bool {
    actual.iter().map(|target|&target.id).eq(input.iter())
}
fn destination(input:&AnswerRequest)->Result<RememberAs,String> {
    match input.remember_as {
        Destination::Temporary=>Ok(RememberAs::Temporary),
        Destination::ModuleDecision=>Ok(RememberAs::ModuleDecision{id:format!("D-{}",input.request_id),supersedes:vec![]}),
        Destination::ProcessRule|Destination::SlowProposal=>Err("该回答去向尚未开放；请选择仅本次或模块决策。涉及慢层时系统自动保存待批准提案".into()),
    }
}
fn envelope(server:&Server,eid:String,action:&QuestionEvent,guards:Vec<Fence>)->Result<NewEvent,String> {
    let (kind,actor,obj)=match action {
        QuestionEvent::Opened{question}=>("hold.opened","process:towermd-cli",question.draft.id.clone()),
        QuestionEvent::Answered{answer}=>("hold.answered","human:local",answer.hold.clone()),
        QuestionEvent::AnswerRelayed{hold,..}=>("hold.answered","process:towermd-cli",hold.clone()),
        QuestionEvent::DecisionReused{question,..}=>("hold.decision_reused","process:towermd-dedupe",question.draft.id.clone()),
        QuestionEvent::PromptQueued{prompt}=>("prompt.queued","human:local",prompt.id.clone()),
        QuestionEvent::Delivered{delivery}=>("prompt.delivered","process:towermd-cli",match &delivery.message {MessageId::Answer{hold,..}=>hold.clone(),MessageId::Prompt{id}=>id.clone()}),
    };
    Ok(NewEvent{eid,host:server.endpoint.host.clone(),actor:actor.into(),kind:kind.into(),layer:"L5".into(),obj,
        data:serde_json::to_value(action).map_err(|e|e.to_string())?,guards})
}
// Return the original envelope, including its original recipients, timestamp,
// layer resolution and fences. Retries never reconstruct these from new state.
fn original(runtime:&ProjectRuntime,eid:&str)->Result<Option<NewEvent>,String> {
    if let Some(event)=runtime.receipt(eid) {
        if event.stale {return Err("原请求已过期，不能冒充接受成功".into());}
        return Ok(Some(NewEvent{eid:event.eid.clone(),host:event.host.clone(),actor:event.actor.clone(),kind:event.kind.clone(),
            layer:event.layer.clone(),obj:event.obj.clone(),data:event.data.clone(),guards:event.guards.clone()}));
    }
    Ok(runtime.pending().map_err(|e|e.to_string())?.into_iter().find(|event|event.eid==eid))
}
fn append(runtime:&mut ProjectRuntime,input:NewEvent)->Result<Event,ApiError> {
    runtime.append(input,now).map_err(|error|match error {RuntimeError::Invalid(message)=>bad(message),other=>uncertain(other)})
}
fn accepted(event:&Event,proposal:Option<String>)->Json<Value> {
    Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","proposal":proposal}))
}
fn ensure_original(input:&NewEvent,server:&Server,actor:&str,kind:&str)->Result<(),ApiError> {
    if input.host!=server.endpoint.host || input.actor!=actor || input.kind!=kind || input.layer!="L5" {
        return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已属于不同操作或来源"));
    }
    Ok(())
}

pub async fn answer(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<AnswerRequest>)->Result<Json<Value>,ApiError> {
    if !crate::daemon::authorized_app(&headers,&server) {return Err(denied());}
    valid_scan_id(&input.request_id).map_err(bad)?;
    let remember_as=destination(&input).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let request=if let Some(old)=original(runtime,&input.request_id).map_err(bad)? {
            ensure_original(&old,&server,"human:local","hold.answered")?;
            let QuestionEvent::Answered{answer}=serde_json::from_value(old.data.clone()).map_err(bad)? else {return Err(bad("原回答请求无效"));};
            if answer.hold!=input.hold || answer.text!=input.text || answer.remember_as!=remember_as || !same_targets(&answer.targets,&input.targets) {return Err(rejected_as(ErrorCode::Conflict)("同一回答请求内容不同"));}
            old
        } else {
            let answer=AnswerInput{hold:input.hold.clone(),text:input.text,remember_as,targets:targets(&project,&input.targets).map_err(bad)?};
            envelope(&server,input.request_id,&QuestionEvent::Answered{answer},vec![]).map_err(bad)?
        };
        let event=append(runtime,request)?;
        // Snapshot failure remains uncertain, so the caller retries this exact
        // request and the accepted original is reused before snapshot retry.
        runtime.snapshot(now).map_err(uncertain)?;
        let projection=question_projection::read(runtime.projection()).map_err(uncertain)?;
        Ok(accepted(&event,projection.answers.get(&input.hold).and_then(|answer|answer.proposal.clone())))
    }).await.map_err(uncertain)?
}

pub async fn relay(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<RelayRequest>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let projection=question_projection::read(runtime.projection()).map_err(bad)?;
        let question=projection.holds.get(&input.hold).ok_or_else(||rejected_as(ErrorCode::NotFound)("问题不存在"))?;
        let action=QuestionEvent::AnswerRelayed{hold:input.hold,text:input.text,relayed_from:"human".into(),self_reported:input.self_reported};
        let request=if let Some(old)=original(runtime,&input.request_id).map_err(bad)? {
            ensure_original(&old,&server,"process:towermd-cli","hold.answered")?;
            if old.data!=serde_json::to_value(&action).map_err(bad)? {return Err(rejected_as(ErrorCode::Conflict)("同一转述请求内容不同"));}
            // A confirmed answer is retryable even after its run has expired.
            let (worktree,branch)=tasks::coordinates(&project,&input.cwd).map_err(bad)?;
            let tasks=dev_state::tasks(runtime.projection()).map_err(bad)?;
            let run=tasks.runs.get(&question.binding.run).ok_or_else(||rejected_as(ErrorCode::NotFound)("原问题运行不存在"))?;
            if run.binding.host!=server.endpoint.host || input.run.as_ref().is_some_and(|id|id!=&question.binding.run)
                || (input.run.is_none() && (run.binding.worktree!=worktree || run.binding.branch!=branch))
            {return Err(bad("转述问题不属于当前运行"));}
            old
        } else {
            let run=tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),true).map_err(bad)?.ok_or_else(||bad("缺少活动运行"))?;
            if question.binding.run!=run.binding.id {return Err(bad("转述问题不属于当前运行"));}
            envelope(&server,input.request_id,&action,run.fences).map_err(bad)?
        };
        let event=append(runtime,request)?;
        if event.stale {return Err(rejected_as(ErrorCode::StaleEpoch)("转述回答的运行租约已失效，回答未接受"));}
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event,None))
    }).await.map_err(uncertain)?
}

pub async fn prompt(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<PromptRequest>)->Result<Json<Value>,ApiError> {
    if !crate::daemon::authorized_app(&headers,&server) {return Err(denied());}
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let request=if let Some(old)=original(runtime,&input.request_id).map_err(bad)? {
            ensure_original(&old,&server,"human:local","prompt.queued")?;
            let QuestionEvent::PromptQueued{prompt}=serde_json::from_value(old.data.clone()).map_err(bad)? else {return Err(bad("原插话请求无效"));};
            if prompt.scope!=input.scope || prompt.text!=input.text || !same_targets(&prompt.targets,&input.targets) {return Err(rejected_as(ErrorCode::Conflict)("同一插话请求内容不同"));}
            old
        } else {
            runtime.tick(now).map_err(uncertain)?;
            let tasks=dev_state::tasks(runtime.projection()).map_err(bad)?;
            let active=tasks.runs.values().filter(|run|run.state==RunState::Active).map(binding).collect::<Vec<_>>();
            let prompt=question_model::prepare_prompt(format!("P-{}",input.request_id),input.scope,input.text,&active,targets(&project,&input.targets).map_err(bad)?).map_err(bad)?;
            if prompt.recipients.is_empty() {return Err(bad("此范围内没有活动运行，插话未排队"));}
            envelope(&server,input.request_id,&QuestionEvent::PromptQueued{prompt},vec![]).map_err(bad)?
        };
        let event=append(runtime,request)?;
        runtime.snapshot(now).map_err(uncertain)?;
        let projection=question_projection::read(runtime.projection()).map_err(uncertain)?;
        let proposal=projection.proposals.values().find(|proposal|proposal.source_event==event.eid).map(|proposal|proposal.id.clone());
        Ok(accepted(&event,proposal))
    }).await.map_err(uncertain)?
}

pub async fn ask(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<AskRequest>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let draft=QuestionDraft{id:format!("H-{}",input.request_id),text:input.text,options:input.options,
            recommended:input.recommended,context:input.context,mode:input.mode};
        let request=if let Some(old)=original(runtime,&input.request_id).map_err(bad)? {
            if old.host!=server.endpoint.host || old.layer!="L5" {return Err(bad("原问题来源不同"));}
            let question=match serde_json::from_value::<QuestionEvent>(old.data.clone()).map_err(bad)? {
                QuestionEvent::Opened{question} if old.kind=="hold.opened" && old.actor=="process:towermd-cli"=>question,
                QuestionEvent::DecisionReused{question,..} if old.kind=="hold.decision_reused" && old.actor=="process:towermd-dedupe"=>question,
                _=>return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他操作")),
            };
            // A confirmed question remains the same question after lease expiry.
            // Coordinate validation still prevents accidental cross-project use.
            let (worktree,branch)=tasks::coordinates(&project,&input.cwd).map_err(bad)?;
            let tasks=dev_state::tasks(runtime.projection()).map_err(bad)?;
            let run=tasks.runs.get(&question.binding.run).ok_or_else(||rejected_as(ErrorCode::NotFound)("原问题运行不存在"))?;
            if run.binding.host!=server.endpoint.host || input.run.as_ref().is_some_and(|id|id!=&question.binding.run)
                || (input.run.is_none() && (run.binding.worktree!=worktree || run.binding.branch!=branch))
                || question.draft!=draft || !same_targets(&question.targets,&input.targets) {return Err(rejected_as(ErrorCode::Conflict)("同一提问请求的内容或绑定不同"));}
            old
        } else {
            let run=tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),true).map_err(bad)?.ok_or_else(||bad("缺少活动运行"))?;
            let question=question_model::prepare_question(draft,binding(&run),targets(&project,&input.targets).map_err(bad)?,now()).map_err(bad)?;
            let projection=question_projection::read(runtime.projection()).map_err(bad)?;
            let mut decisions=DecisionCatalog::from_projection(&projection);
            let catalog=current_catalog(&project).map_err(bad)?;
            decisions.read_main(&project.root,catalog.config.docs_dir()).map_err(bad)?;
            let action=decisions.reuse_event(&question).map_err(bad)?.unwrap_or(QuestionEvent::Opened{question});
            envelope(&server,input.request_id,&action,run.fences).map_err(bad)?
        };
        let event=append(runtime,request)?;
        let projection=question_projection::read(runtime.projection()).map_err(uncertain)?;
        let wait=projection.wait_plan(&event.obj,now(),Some(0)).map_err(uncertain)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"hold":event.obj,"wait":wait})))
    }).await.map_err(uncertain)?
}

/// Validate caller binding once, then poll the pure projection in independent
/// short critical sections. No lock, subprocess or renewed inactivity timeout
/// is held throughout the user's thinking time.
pub async fn wait(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<WaitRequest>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    let project=project(&server,&handle).map_err(bad)?;
    let first_project=project.clone();
    let first_server=server.clone();
    let hold=input.hold.clone();
    let budget=input.wait_ms.unwrap_or(question_model::ASK_WAIT_MS).min(question_model::ASK_WAIT_MS);
    tokio::task::spawn_blocking(move|| {
        let mut slot=first_project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&first_server,&first_project).map_err(bad)?;
        let run=tasks::renew(runtime,&first_server,&first_project,&input.cwd,input.run.as_deref(),true).map_err(bad)?.ok_or_else(||bad("缺少活动运行"))?;
        let projection=question_projection::read(runtime.projection()).map_err(bad)?;
        let question=projection.holds.get(&input.hold).ok_or_else(||rejected_as(ErrorCode::NotFound)("问题不存在"))?;
        if question.binding.run!=run.binding.id {return Err(bad("问题不属于当前运行"));}
        Ok::<(),ApiError>(())
    }).await.map_err(uncertain)??;
    let started=tokio::time::Instant::now();
    loop {
        let remaining=budget.saturating_sub(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        let poll_project=project.clone();
        let poll_hold=hold.clone();
        let result=tokio::task::spawn_blocking(move|| {
            let slot=poll_project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
            let runtime=slot.as_ref().ok_or_else(||uncertain("项目会话在等待期间关闭"))?;
            question_projection::read(runtime.projection()).map_err(uncertain)?.wait_plan(&poll_hold,now(),Some(remaining)).map_err(bad)
        }).await.map_err(uncertain)??;
        let WaitPlan::Pending{wait_ms,..}=result else {return Ok(Json(serde_json::to_value(result).map_err(uncertain)?));};
        if wait_ms==0 {return Ok(Json(serde_json::to_value(result).map_err(uncertain)?));}
        tokio::time::sleep(Duration::from_millis(wait_ms.min(250))).await;
    }
}

pub async fn messages(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Context>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        if slot.is_none() && !project.common.join("towermd/runtime.json").exists()
            && towermd_core::main_writer::latest_snapshot(&project.root).map_err(bad)?.is_none() {
            return Ok(Json(json!({"run":null,"messages":[]})));
        }
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let run=tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),false).map_err(bad)?;
        let Some(run)=run else {return Ok(Json(json!({"run":null,"messages":[]})));};
        let projection=question_projection::read(runtime.projection()).map_err(bad)?;
        let tasks=dev_state::tasks(runtime.projection()).map_err(bad)?;
        let task=tasks.tasks.get(&run.task).ok_or_else(||rejected_as(ErrorCode::NotFound)("运行任务不存在"))?;
        let needs_review=task.needs_review.then(||json!({"task":task.input.id,"baseline":task.baseline}));
        let mut messages=serde_json::to_value(projection.pending_messages(&run.binding.id)).map_err(bad)?;
        if let Some(list)=messages.as_array_mut() {
            list.extend(super::delegations::report_messages(runtime.projection(),&run.binding.id,run.binding.self_reported.as_ref(),project.project_id.as_deref().unwrap_or_default()).map_err(bad)?);
        }
        Ok(Json(json!({"run":run.binding.id,"messages":messages,"needs_review":needs_review})))
    }).await.map_err(uncertain)?
}

pub async fn ack(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<AckRequest>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        tasks::coordinates(&project,&input.cwd).map_err(bad)?;
        let task_projection=dev_state::tasks(runtime.projection()).map_err(bad)?;
        let run=task_projection.runs.get(&input.run).ok_or_else(||rejected_as(ErrorCode::NotFound)("送达确认的运行不存在"))?;
        if run.binding.host!=server.endpoint.host {return Err(bad("不能替其他主机确认送达"));}
        let delivery=Delivery{run:input.run,message:input.message};
        // Output may have succeeded just before a lease expired. Delivery is an
        // observation, not new authority; recording it needs no module fence.
        let request=if let Some(old)=original(runtime,&input.request_id).map_err(bad)? {
            ensure_original(&old,&server,"process:towermd-cli","prompt.delivered")?;
            if old.data!=serde_json::to_value(QuestionEvent::Delivered{delivery:delivery.clone()}).map_err(bad)? {return Err(rejected_as(ErrorCode::Conflict)("同一送达确认内容不同"));}
            old
        } else {
            let projection=question_projection::read(runtime.projection()).map_err(bad)?;
            if let Some(receipt)=projection.deliveries.values().find(|receipt|receipt.delivery==delivery) {
                let event=runtime.receipt(&receipt.event).ok_or_else(||uncertain("送达投影缺少完整原件"))?;
                return Ok(Json(json!({"request_id":input.request_id,"event":event.eid,"seq":event.seq,"status":"already_delivered","proposal":null})));
            }
            envelope(&server,input.request_id,&QuestionEvent::Delivered{delivery},vec![]).map_err(bad)?
        };
        let event=append(runtime,request)?;
        Ok(accepted(&event,None))
    }).await.map_err(uncertain)?
}
