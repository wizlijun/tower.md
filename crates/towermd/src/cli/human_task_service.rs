//! App-only task creation. The server derives origin and admission state from
//! the actual published baseline; the caller supplies only a task draft.
use super::business::{now,ready,uncertain};
use super::*;
use towermd_core::{hub_protocol::NewEvent, project_runtime::{ProjectRuntime,RuntimeError}};
use towermd_devkit::{dev_state, task_model::{self,TaskDraft,TaskEvent,TaskOrigin}};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Add {request_id:String,task:TaskDraft}

pub async fn options(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>)->Result<Json<Value>,ApiError> {
    if !crate::daemon::authorized_app(&headers,&server) {return Err(denied());}
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=slot.as_ref().ok_or_else(||bad("项目尚无活动批准基线；请先在 App 出版基线"))?;
        let baseline=tasks::baseline(&project,runtime).map_err(bad)?;
        let modules=baseline.modules().iter().map(|(id,module)|json!({"id":id,"title":id,"paths":module.paths})).collect::<Vec<_>>();
        Ok(Json(json!({"baseline":baseline.tag(),"modules":modules})))
    }).await.map_err(uncertain)?
}

pub(super) fn original(runtime:&ProjectRuntime,eid:&str)->Result<Option<NewEvent>,String> {
    if let Some(event)=runtime.receipt(eid) {
        if event.stale {return Err("原任务请求未生效".into());}
        return Ok(Some(NewEvent{eid:event.eid.clone(),host:event.host.clone(),actor:event.actor.clone(),
            kind:event.kind.clone(),layer:event.layer.clone(),obj:event.obj.clone(),data:event.data.clone(),guards:event.guards.clone()}));
    }
    Ok(runtime.pending().map_err(|e|e.to_string())?.into_iter().find(|event|event.eid==eid))
}

pub async fn add(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Add>)->Result<Json<Value>,ApiError> {
    if !crate::daemon::authorized_app(&headers,&server) {return Err(denied());}
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let request=if let Some(old)=original(runtime,&input.request_id).map_err(bad)? {
            if old.host!=server.endpoint.host || old.actor!="human:local" || old.kind!="task.created"
                || old.layer!="L4" || old.obj!=input.task.id || !old.guards.is_empty() {
                return Err(bad("请求 ID 已属于其他任务或来源"));
            }
            let action:TaskEvent=serde_json::from_value(old.data.clone()).map_err(bad)?;
            if !matches!(action,TaskEvent::Created{task} if task.input==input.task && task.origin==TaskOrigin::AppHuman) {
                return Err(bad("同一人工任务请求内容不同"));
            }
            old
        } else {
            let baseline=tasks::baseline(&project,runtime).map_err(bad)?;
            let projection=dev_state::tasks(runtime.projection()).map_err(bad)?;
            let action=task_model::prepare_add(&baseline,&projection,input.task.clone(),TaskOrigin::AppHuman).map_err(bad)?;
            NewEvent{eid:input.request_id,host:server.endpoint.host.clone(),actor:"human:local".into(),
                kind:"task.created".into(),layer:"L4".into(),obj:input.task.id,
                data:serde_json::to_value(action).map_err(bad)?,guards:vec![]}
        };
        let event=runtime.append(request,now).map_err(|error|match error {
            RuntimeError::Invalid(message)=>bad(message),other=>uncertain(other)})?;
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,
            "status":"accepted","task":runtime.projection()["Task"][&event.obj]})))
    }).await.map_err(uncertain)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn app_task_input_cannot_choose_actor_origin_state_or_run_authority() {
        let input=json!({"request_id":"request","task":{"id":"T-one","title":"Task","module":"A-one","allowed_paths":[]}});
        assert!(serde_json::from_value::<Add>(input.clone()).is_ok());
        for key in ["actor","origin","state","cwd","run"] {
            let mut extra=input.clone();
            extra[key]=json!("human:forged");
            assert!(serde_json::from_value::<Add>(extra).is_err(),"{key}");
        }
        for key in ["actor","origin","state","active_run","baseline","needs_review"] {
            let mut extra=input.clone();
            extra["task"][key]=json!("injected");
            assert!(serde_json::from_value::<Add>(extra).is_err(),"{key}");
        }
    }
}
