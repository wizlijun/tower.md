//! Generic report requests and node questions (FR-116, FR-127), silence
//! (FR-137) and stuck detection (FR-126). A person asks only from the App;
//! the daemon fixes who receives it and later raises `core.silent` and
//! `core.stuck` signed `process:towermd`. Raising never approves, blocks or
//! completes anything; each signal's hub ID derives from its source, so a
//! retry or a second host raises it once.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{config::Config, delegation_projection as log, hub::Event, hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError},
    requests::{self, Ask, Sent}, stuck, tower_api::{self, Envelope}};
use towermd_devkit::dev_state::KITS;

const HUMAN: &str = "human:local";
/// Signals written per kind and maintenance pass; the next pass continues.
const BATCH: usize = 16;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskBody {request_id:String,target:String,ask:Ask,text:String,due:String}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Withdraw {request_id:String}

/// Who receives a new request on `target`. A question goes only to those
/// who declared `answers` there (FR-127); a report request also reaches
/// whoever reported on it and the delegate of its active attempt. Empty
/// means queued: whoever declares `answers` later receives it.
fn recipients(projection:&Value,target:&str,ask:Ask)->Result<Vec<String>,String> {
    let tower=tower_api::read(projection)?;
    let mut to=tower.answerers(target);
    if ask==Ask::Report {
        to.extend(tower.reports.iter().filter(|report|report.target==target).map(|report|report.from.id.clone()));
        let items=log::delegations(projection,KITS)?;
        to.extend(items.iter().filter(|item|item.id==target).flat_map(|item|&item.attempts).filter(|attempt|attempt.active)
            .map(|attempt|requests::delegate_id(&attempt.id,attempt.delegate.as_ref())));
        to.sort();
        to.dedup();
    }
    Ok(to)
}

/// Body `{request_id, target, ask, text, due}`; only the App sends it. A
/// retry with the same request ID and body returns the original even when
/// the recipients changed since.
pub async fn ask(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<AskBody>)->Result<Json<Value>,ApiError> {
    super::delegations::app(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let request=if let Some(old)=human_tasks::original(runtime,&input.request_id).map_err(bad)? {
            let data:Sent=serde_json::from_value(old.data.clone()).map_err(|_|rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容"))?;
            if old.host!=server.endpoint.host || old.actor!=HUMAN || old.kind!=requests::SENT || old.obj!=input.request_id
                || (&data.target,data.ask,&data.text,&data.due)!=(&input.target,input.ask,&input.text,&input.due)
            {return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容"));}
            old
        } else {
            let to=recipients(runtime.projection(),&input.target,input.ask).map_err(bad)?;
            let data=Sent{target:input.target,ask:input.ask,text:input.text,due:input.due,to};
            NewEvent{eid:input.request_id.clone(),host:server.endpoint.host.clone(),actor:HUMAN.into(),kind:requests::SENT.into(),
                layer:requests::LAYER.into(),obj:input.request_id,data:serde_json::to_value(data).map_err(bad)?,guards:vec![]}
        };
        let event=runtime.append(request,now).map_err(|error|match error {RuntimeError::Invalid(message)=>bad(message),other=>uncertain(other)})?;
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","obj":event.obj,"to":event.data["to"]})))
    }).await.map_err(uncertain)?
}

/// The person withdraws an open request; it stops being delivered and never
/// goes silent.
pub async fn withdraw(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Withdraw>)->Result<Json<Value>,ApiError> {
    super::delegations::app(&headers,&server)?;
    super::delegations::record(server,handle,input.request_id,requests::WITHDRAWN,requests::LAYER,id,json!({})).await
}

/// Appends one `signal.received` signed `process:towermd` unless it is
/// already recorded; a queued copy is resent as is.
fn raise(runtime:&mut ProjectRuntime,host:&str,eid:String,envelope:impl FnOnce()->Result<Envelope,String>)->Result<(),String> {
    if runtime.receipt(&eid).is_some() {return Ok(());}
    let request=match runtime.pending().map_err(|e|e.to_string())?.into_iter().find(|event|event.eid==eid) {
        Some(queued)=>queued,
        None=>{
            let envelope=envelope()?;
            NewEvent{eid,host:host.into(),actor:tower_api::TOWER.into(),kind:"signal.received".into(),layer:tower_api::LAYER.into(),
                obj:envelope.target.clone(),data:serde_json::to_value(envelope).map_err(|e|e.to_string())?,guards:vec![]}
        }
    };
    let event=runtime.append(request,now).map_err(|e|e.to_string())?;
    if event.stale {return Err("信号事件未生效".into());}
    Ok(())
}

/// Maintenance (FR-137): one `core.silent` per request past its deadline
/// without an answer, passive at first and active when the previous request
/// on the same object went silent too.
pub(super) fn write_silences(server:&Server,project:&Project,runtime:&mut ProjectRuntime)->Result<usize,String> {
    let Some(project_id)=project.project_id.as_deref() else {return Ok(0)};
    let silences=requests::silences(runtime.projection(),KITS,now())?;
    let mut written=0;
    for silence in silences.into_iter().take(BATCH) {
        raise(runtime,&server.endpoint.host,requests::silence_eid(&silence.request),
            ||requests::silence_envelope(project_id,&silence,tower_api::timestamp(now())?))?;
        written+=1;
    }
    Ok(written)
}

/// Maintenance (FR-126): one active `core.stuck` per trigger, with thresholds
/// from `config.yaml` (defaults when missing or unreadable).
pub(super) fn write_stuck(server:&Server,project:&Project,runtime:&mut ProjectRuntime)->Result<usize,String> {
    let Some(project_id)=project.project_id.as_deref() else {return Ok(0)};
    let thresholds=Config::load(&project.root).ok().flatten().map(|config|config.stuck()).unwrap_or_default();
    let mut events:Vec<&Event>=runtime.canonical_receipts().collect();
    events.sort_by_key(|event|event.seq);
    let triggers:Vec<_>=stuck::detect(&events,runtime.projection(),KITS,&thresholds,now())?.into_iter()
        .filter(|trigger|runtime.receipt(&trigger.eid()).is_none()).take(BATCH).collect();
    let mut written=0;
    for trigger in triggers {
        raise(runtime,&server.endpoint.host,trigger.eid(),||trigger.envelope(project_id,tower_api::timestamp(now())?))?;
        written+=1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bodies_cannot_sign_or_address_a_request() {
        let body=json!({"request_id":"q1","target":"T-1","ask":"question","text":"为什么？","due":"2026-10-24T00:00:00Z"});
        assert!(serde_json::from_value::<AskBody>(body.clone()).is_ok());
        for (key,value) in [("to",json!(["a/1"])),("actor",json!("human:x")),("layer",json!("L2"))] {
            let mut forged=body.clone();
            forged[key]=value;
            assert!(serde_json::from_value::<AskBody>(forged).is_err(),"{key}");
        }
        let mut wrong=body;
        wrong["ask"]=json!("approve");
        assert!(serde_json::from_value::<AskBody>(wrong).is_err());
    }
}
