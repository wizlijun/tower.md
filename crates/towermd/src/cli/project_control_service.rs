//! Admission pause and explicitly confirmed resume. Commands enqueue checkpoint
//! instructions through the question delivery path, never signal a process.
use super::business::{now,ready, uncertain, open_failure, runtime_failure};
use super::*;
use towermd_core::{hub::Event, hub_protocol::NewEvent, project_controls::{self,ControlEvent,Scope}, project_runtime::{ProjectRuntime,RuntimeError}};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pause {request_id:String,scope:Scope}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resume {request_id:String,pause_event:String,scope:Scope}

fn original(runtime:&ProjectRuntime,eid:&str)->Result<Option<NewEvent>,String> {
    if let Some(event)=runtime.receipt(eid) {
        if event.stale {return Err("原控制请求未生效".into());}
        return Ok(Some(NewEvent{eid:event.eid.clone(),host:event.host.clone(),actor:event.actor.clone(),
            kind:event.kind.clone(),layer:event.layer.clone(),obj:event.obj.clone(),data:event.data.clone(),guards:event.guards.clone()}));
    }
    Ok(runtime.pending().map_err(|e|e.to_string())?.into_iter().find(|event|event.eid==eid))
}
fn ensure_original(input:&NewEvent,server:&Server,project:&Project,actor:&str,kind:&str)->Result<ControlEvent,ApiError> {
    if input.host!=server.endpoint.host || input.actor!=actor || input.kind!=kind || input.layer!="L5"
        || !input.guards.is_empty() {return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已属于其他控制操作或来源"));}
    let action=serde_json::from_value(input.data.clone()).map_err(bad)?;
    if input.obj!=project_controls::event_object(&action,project.project_id.as_deref().ok_or_else(||rejected_as(ErrorCode::NotInitialized)("项目尚未初始化"))?) {
        return Err(bad("原控制请求对象不符"));
    }
    Ok(action)
}
fn envelope(server:&Server,project:&Project,eid:String,actor:&str,action:ControlEvent)->Result<NewEvent,ApiError> {
    let obj=project_controls::event_object(&action,project.project_id.as_deref().ok_or_else(||rejected_as(ErrorCode::NotInitialized)("项目尚未初始化"))?).to_string();
    Ok(NewEvent{eid,host:server.endpoint.host.clone(),actor:actor.into(),kind:project_controls::event_kind(&action).into(),
        layer:"L5".into(),obj,data:serde_json::to_value(action).map_err(bad)?,guards:vec![]})
}
fn append(runtime:&mut ProjectRuntime,input:NewEvent)->Result<Event,ApiError> {
    runtime.append(input,now).map_err(|error|match error {RuntimeError::Invalid(message)=>bad(message),other=>runtime_failure(other)})
}
fn accepted(event:&Event,runtime:&ProjectRuntime)->Result<Json<Value>,ApiError> {
    let control=project_controls::read(runtime.projection()).map_err(uncertain)?;
    Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","control":control})))
}

pub async fn pause(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Pause>)->Result<Json<Value>,ApiError> {
    let actor=if crate::daemon::authorized_app(&headers,&server) {"human:local"}
        else {tasks::require_cli(&headers,&server)?;"process:towermd-cli"};
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        let request=if let Some(old)=original(runtime,&input.request_id).map_err(bad)? {
            if !matches!(ensure_original(&old,&server,&project,actor,"project.paused")?,ControlEvent::Paused{scope,..} if scope==input.scope) {
                return Err(rejected_as(ErrorCode::Conflict)("同一暂停请求内容不同"));
            }
            old
        } else {
            runtime.tick(now).map_err(runtime_failure)?;
            let action=project_controls::prepare_pause(runtime.projection(),&input.request_id, towermd_devkit::dev_state::KITS).map_err(bad)?;
            envelope(&server,&project,input.request_id,actor,action)?
        };
        let event=append(runtime,request)?;
        runtime.snapshot(now).map_err(uncertain)?;
        accepted(&event,runtime)
    }).await.map_err(uncertain)?
}

pub async fn resume_request(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Resume>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    change_resume(server,handle,input,false).await
}
pub async fn resume(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Resume>)->Result<Json<Value>,ApiError> {
    if !crate::daemon::authorized_app(&headers,&server) {return Err(denied());}
    change_resume(server,handle,input,true).await
}
async fn change_resume(server:Arc<Server>,handle:String,input:Resume,human:bool)->Result<Json<Value>,ApiError> {
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        let (actor,kind)=if human {("human:local","project.resumed")}else{("process:towermd-cli","project.resume.requested")};
        let request=if let Some(old)=original(runtime,&input.request_id).map_err(bad)? {
            let action=ensure_original(&old,&server,&project,actor,kind)?;
            let same=match action {
                ControlEvent::Resumed{scope,pause_event,..} if human=>scope==input.scope&&pause_event==input.pause_event,
                ControlEvent::ResumeRequested{scope,pause_event} if !human=>scope==input.scope&&pause_event==input.pause_event,
                _=>false,
            };
            if !same {return Err(rejected_as(ErrorCode::Conflict)("同一恢复请求内容不同"));}
            old
        } else {
            runtime.tick(now).map_err(runtime_failure)?;
            let action=if human {project_controls::prepare_resume(runtime.projection(),&input.request_id,input.pause_event, towermd_devkit::dev_state::KITS)}
                else{project_controls::prepare_resume_request(runtime.projection(),input.pause_event)}.map_err(bad)?;
            envelope(&server,&project,input.request_id,actor,action)?
        };
        let event=append(runtime,request)?;
        runtime.snapshot(now).map_err(uncertain)?;
        accepted(&event,runtime)
    }).await.map_err(uncertain)?
}
