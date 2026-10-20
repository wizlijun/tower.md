//! Project admission controls. A pause is a request to checkpoint, never proof
//! that a process stopped. Control messages use the existing prompt queue.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::{hub::Event, project_state, question_model::{self, Prompt, PromptScope, RunContext},
    question_projection, task_model::RunState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Scope { #[default] All }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeRequest {
    pub event:String,
    pub pause_event:String,
    pub actor:String,
    pub at:String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlState {
    pub paused:bool,
    pub pause_event:Option<String>,
    pub scope:Scope,
    pub resume_requests:Vec<ResumeRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum ControlEvent {
    Paused {scope:Scope,prompt:Prompt},
    ResumeRequested {scope:Scope,pause_event:String},
    Resumed {scope:Scope,pause_event:String,prompt:Prompt},
}

pub fn read(state:&Value)->Result<ControlState,String> {
    let control:ControlState=match state.get("ProjectControl") {
        Some(value)=>serde_json::from_value(value.clone()).map_err(|e|e.to_string())?,
        None=>ControlState::default(),
    };
    if control.paused!=state["tasks_paused"].as_bool().unwrap_or(false)
        || control.paused!=control.pause_event.is_some() {
        return Err("项目暂停投影与准入门禁不一致".into());
    }
    Ok(control)
}

pub fn active(state:&Value)->Result<Vec<RunContext>,String> {
    Ok(project_state::tasks(state)?.runs.values().filter(|run|run.state==RunState::Active)
        .map(|run|RunContext{run:run.binding.id.clone(),task:run.task.clone(),module:run.module.clone()}).collect())
}

fn pause_text(eid:&str)->String {
    format!("项目已暂停接收新任务（暂停 ID：{eid}）。请在下一个安全检查点保存进展并暂停推进，汇报检查点与未完成事项。执行前通过 towermd status 确认当前仍是这次暂停；已解除时忽略此旧指令。本消息不表示进程已停止，不更改或延长租约。")
}
fn resume_text(pause:&str)->String {
    format!("App 中的人已解除项目暂停 {pause}。请先通过 towermd status 确认项目当前未再次暂停，再按当前有效租约和门禁继续。本消息不恢复过期租约，不证明任何任务完成。")
}
fn prompt(eid:&str,text:String,recipients:&[RunContext])->Result<Prompt,String> {
    question_model::prepare_prompt(format!("P-{eid}"),PromptScope::Global,text,recipients,vec![])
}

pub fn prepare_pause(state:&Value,eid:&str)->Result<ControlEvent,String> {
    if read(state)?.paused {return Err("项目已暂停；请确认现有暂停后再恢复".into());}
    Ok(ControlEvent::Paused{scope:Scope::All,prompt:prompt(eid,pause_text(eid),&active(state)?)?})
}
pub fn prepare_resume_request(state:&Value,pause_event:String)->Result<ControlEvent,String> {
    require_pause(&read(state)?,&pause_event)?;
    Ok(ControlEvent::ResumeRequested{scope:Scope::All,pause_event})
}
pub fn prepare_resume(state:&Value,eid:&str,pause_event:String)->Result<ControlEvent,String> {
    require_pause(&read(state)?,&pause_event)?;
    Ok(ControlEvent::Resumed{scope:Scope::All,prompt:prompt(eid,resume_text(&pause_event),&active(state)?)?,pause_event})
}
fn require_pause(control:&ControlState,pause:&str)->Result<(),String> {
    if !control.paused || control.pause_event.as_deref()!=Some(pause) {
        return Err("待确认的暂停已变化；请读取当前项目状态后重新确认".into());
    }
    Ok(())
}

pub fn event_kind(action:&ControlEvent)->&'static str {
    match action {ControlEvent::Paused{..}=>"project.paused",
        ControlEvent::ResumeRequested{..}=>"project.resume.requested",ControlEvent::Resumed{..}=>"project.resumed"}
}
pub fn event_object<'a>(action:&'a ControlEvent,project:&'a str)->&'a str {
    match action {ControlEvent::Paused{..}=>project,
        ControlEvent::ResumeRequested{pause_event,..}|ControlEvent::Resumed{pause_event,..}=>pause_event}
}

/// Strict typed consumer, also used when rebuilding a snapshot. No lease or
/// execution state changes occur here; the original canonical event owns both
/// the admission change and the frozen prompt.
pub fn apply(state:&mut Value,event:&Event)->Result<bool,String> {
    if !matches!(event.kind.as_str(),"project.paused"|"project.resume.requested"|"project.resumed") {return Ok(false);}
    if event.stale {return Ok(true);}
    let action:ControlEvent=serde_json::from_value(event.data.clone()).map_err(|e|e.to_string())?;
    if event.kind!=event_kind(&action) || event.layer!="L5" || event.obj.trim().is_empty() || !event.guards.is_empty() {
        return Err("项目控制事件信封无效".into());
    }
    if event.obj!=event_object(&action,&event.obj) {return Err("项目恢复信封未绑定原暂停".into());}
    let mut control=read(state)?;
    let delivery=match action {
        ControlEvent::Paused{scope,prompt}=>{
            if !matches!(event.actor.as_str(),"human:local"|"process:towermd-cli") {return Err("暂停来源无效".into());}
            if control.paused {return Err("不能覆盖尚未解除的暂停".into());}
            validate_prompt(state,event,&prompt,pause_text(&event.eid))?;
            control=ControlState{paused:true,pause_event:Some(event.eid.clone()),scope,resume_requests:vec![]};
            Some(prompt)
        }
        ControlEvent::ResumeRequested{scope:_,pause_event}=>{
            if event.actor!="process:towermd-cli" {return Err("恢复请求须保留命令行过程来源".into());}
            require_pause(&control,&pause_event)?;
            if control.resume_requests.iter().any(|request|request.event==event.eid) {return Err("重复恢复请求原件".into());}
            control.resume_requests.push(ResumeRequest{event:event.eid.clone(),pause_event,
                actor:event.actor.clone(),at:event.t.clone()});
            None
        }
        ControlEvent::Resumed{scope:_,pause_event,prompt}=>{
            if event.actor!="human:local" {return Err("解除暂停必须由 App 中的人确认".into());}
            require_pause(&control,&pause_event)?;
            validate_prompt(state,event,&prompt,resume_text(&pause_event))?;
            control=ControlState::default();
            Some(prompt)
        }
    };
    let mut next=state.clone();
    if let Some(prompt)=delivery {
        let queue=question_projection::read(&next)?;
        if queue.prompts.contains_key(&prompt.id)||queue.prompt_actors.contains_key(&prompt.id) {
            return Err("项目控制消息 ID 已存在".into());
        }
        for group in ["Prompt","PromptActor"] {
            if next.get(group).is_none() {next[group]=serde_json::json!({});}
            if !next[group].is_object() {return Err("插话投影不是对象".into());}
        }
        next["Prompt"][&prompt.id]=serde_json::to_value(&prompt).map_err(|e|e.to_string())?;
        next["PromptActor"][&prompt.id]=Value::String(event.actor.clone());
    }
    next["tasks_paused"]=Value::Bool(control.paused);
    next["ProjectControl"]=serde_json::to_value(control).map_err(|e|e.to_string())?;
    *state=next;
    Ok(true)
}

fn validate_prompt(state:&Value,event:&Event,actual:&Prompt,text:String)->Result<(),String> {
    if *actual!=prompt(&event.eid,text,&actual.recipients)? {return Err("项目控制指令或冻结收件人无效".into());}
    let tasks=project_state::tasks(state)?;
    for recipient in &actual.recipients {
        let run=tasks.runs.get(&recipient.run).ok_or("控制指令收件运行不存在")?;
        if run.binding.id!=recipient.run || run.task!=recipient.task || run.module!=recipient.module {
            return Err("控制指令收件运行绑定不符".into());
        }
    }
    Ok(())
}
