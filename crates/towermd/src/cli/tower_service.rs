//! Tower API entry for the CLI (core spec §5, FR-114–119): `towermd report`
//! and `towermd signal`. The daemon wraps the body in the unified envelope,
//! dates it and records it signed `process:towermd-cli`. The envelope's
//! `from` is self-reported; inside a run it defaults to the run's delegate.
//! Nothing here completes anything: a report is a claim, never evidence.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{hub_protocol::NewEvent, project_runtime::RuntimeError, tower_api::{self, Envelope, From as Sender, FromKind, Verb}};

const CLI: &str = "process:towermd-cli";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    request_id:String, cwd:PathBuf, #[serde(default)] run:Option<String>,
    verb:Verb, #[serde(default)] target:Option<String>, #[serde(default)] from:Option<Sender>,
    data:Value, #[serde(default)] refs:Vec<String>,
}

/// The run's delegate as `name/version`, or the run ID when it said nothing.
fn delegate(run:&towermd_devkit::task_model::Run)->Sender {
    let id=run.binding.self_reported.as_ref().map_or_else(||run.binding.id.clone(),|source|format!("{}/{}",source.name,source.version));
    Sender{id,kind:FromKind::Delegate}
}

/// A retry with the same request ID returns the original event; the daemon's
/// own date and defaults are not part of the comparison.
fn same(old:&NewEvent,host:&str,kind:&str,input:&Message)->bool {
    let Ok(envelope)=serde_json::from_value::<Envelope>(old.data.clone()) else {return false};
    old.host==host && old.actor==CLI && old.kind==kind && envelope.data==input.data && envelope.refs==input.refs
        && input.target.as_ref().is_none_or(|target|*target==old.obj)
        && input.from.as_ref().is_none_or(|from|*from==envelope.from)
}

pub async fn record(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Message>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let kind=input.verb.event_kind().ok_or_else(||bad("只接受 report 或 signal"))?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let request=if let Some(old)=human_tasks::original(runtime,&input.request_id).map_err(bad)? {
            if !same(&old,&server.endpoint.host,kind,&input) {
                return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他消息"));
            }
            old
        } else {
            // Outside a worktree or on a detached HEAD there is simply no run;
            // an explicitly named run must exist.
            let run=match tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),false) {
                Ok(run)=>run,
                Err(error) if input.run.is_some()=>return Err(bad(error)),
                Err(_)=>None,
            };
            let from=input.from.or_else(||run.as_ref().map(delegate))
                .ok_or_else(||rejected_as(ErrorCode::Usage)("不在运行中时须用 --from 说明来源"))?;
            let project_id=project.project_id.clone().ok_or_else(||bad("项目尚未初始化"))?;
            let target=input.target.or_else(||run.as_ref().map(|run|run.task.clone())).unwrap_or_else(||project_id.clone());
            let envelope=Envelope{v:tower_api::VERSION,project:project_id,from,target,verb:input.verb,data:input.data,
                refs:input.refs,at:tower_api::timestamp(now()).map_err(uncertain)?};
            envelope.check().map_err(bad)?;
            NewEvent{eid:input.request_id,host:server.endpoint.host.clone(),actor:CLI.into(),kind:kind.into(),
                layer:tower_api::LAYER.into(),obj:envelope.target.clone(),data:serde_json::to_value(envelope).map_err(bad)?,guards:vec![]}
        };
        // No eager snapshot: signals can be frequent, and maintenance
        // publishes the fast layer when it is due.
        let event=runtime.append(request,now).map_err(|error|match error {RuntimeError::Invalid(message)=>bad(message),other=>uncertain(other)})?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","obj":event.obj,"envelope":event.data})))
    }).await.map_err(uncertain)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_bodies_cannot_sign_or_date_the_envelope() {
        let body=json!({"request_id":"r","cwd":"/tmp","verb":"signal","data":{"kind":"eng.crap","level":"passive","title":"t"}});
        assert!(serde_json::from_value::<Message>(body.clone()).is_ok());
        for (key,value) in [("actor",json!("human:local")),("at",json!("2026-10-23T00:00:00Z")),("layer",json!("L2")),("v",json!(1))] {
            let mut forged=body.clone();
            forged[key]=value;
            assert!(serde_json::from_value::<Message>(forged).is_err(),"{key}");
        }
        let mut tower=body;
        tower["verb"]=json!("request_report");
        let message:Message=serde_json::from_value(tower).unwrap();
        assert!(message.verb.event_kind().is_none());
    }
}
