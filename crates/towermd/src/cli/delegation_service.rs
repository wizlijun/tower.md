//! Delegation actions (FR-185, FR-201–211). People finalize cards, accept
//! work, record rules, rename work kinds and set levels only from the App; the
//! delegate clarifies and answers report requests from the CLI. Nothing here
//! completes an object: the kits derive phases from checks and merges.
use super::business::{now, ready, uncertain, open_failure, runtime_failure};
use super::*;
use serde::de::DeserializeOwned;
use towermd_core::{config::Config, delegation::{DelegationSettings, Phase}, delegation_projection::{self as log, Clarified, Confirmed, Finalized, LevelSet, Renamed, Replied, ReportState, RuleRecorded, SentBack, Used}, delegation_view::{self, DelegationView, VerdictKind}, gates::{GateFile, GateStatus, Verdict as GateVerdict}, hub::Event, hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}, question_model::SelfReported, quiet::instant, requests::{self, Ask}, tower_api, waivers::{self, Granted}, weakening};
use super::weakenings::{self,Author};
use towermd_devkit::dev_state::KITS;

const HUMAN: &str = "human:local";
const CLI: &str = "process:towermd-cli";
const DAEMON: &str = "process:towermd";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clarify {request_id:String,clarification:Clarified}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    request_id:String, cwd:PathBuf, #[serde(default)] run:Option<String>,
    state:ReportState, summary:String, #[serde(default)] next:Option<String>,
    /// Generic requests only (FR-116, FR-127): evidence for the answer, and
    /// the sender when no run is current.
    #[serde(default)] refs:Vec<String>, #[serde(default)] from:Option<String>,
}

/// Ledger settings from the project's config; missing or unreadable config
/// keeps the defaults, since the view is display-only.
pub(super) fn settings(project:&Project)->DelegationSettings {
    Config::load(&project.root).ok().flatten().map(|config|config.delegation_settings()).unwrap_or_default()
}

/// Gates come from the working tree here: display only, enforcement reads
/// the authoritative main (`gates::enforce`).
pub(super) fn view(runtime:&ProjectRuntime,project:&Project)->Result<DelegationView,String> {
    let gates=GateFile::load(&project.root).waive(gates::in_force(runtime.projection())?);
    delegation_view::view(runtime.projection(),KITS,&settings(project),&gates)
}

/// Derived for the App on every state read; never persisted.
pub(super) fn view_value(runtime:&ProjectRuntime,project:&Project)->Value {
    match view(runtime,project) {
        Ok(view)=>serde_json::to_value(view).unwrap_or(Value::Null),
        Err(error)=>{eprintln!("委托读模型暂不可用：{error}");Value::Null}
    }
}

/// Open report requests of this run, then open generic requests and node
/// questions addressed to its delegate, as `delivered[]` items. They are
/// never acknowledged: they repeat until `towermd reply`. `envelope` is the
/// same request in the Tower API envelope (FR-114, FR-116, FR-127).
pub(super) fn report_messages(projection:&Value,run:&str,source:Option<&SelfReported>,project_id:&str)->Result<Vec<Value>,String> {
    let mut requests:Vec<_>=log::report_requests(&log::delegations(projection,KITS)?,&log::read(projection)?)
        .into_iter().filter(|request|request.open && request.attempt==run).collect();
    requests.sort_by(|a,b|(&a.due,&a.id).cmp(&(&b.due,&b.id)));
    let at=tower_api::timestamp(now())?;
    let mut messages:Vec<Value>=requests.into_iter().map(|request| json!({
        "message":{"kind":"request_report","request":request.id},"run":run,"actor":DAEMON,
        "request":request.id,"due":request.due,
        "envelope":tower_api::request_report(project_id,&request.object,&request.id,&request.due,&request.text,&at,false),
        "text":format!("请汇报检查点「{}」（截止 {}）：towermd reply {} --state <状态> --summary <摘要>",request.text,request.due,request.id),
    })).collect();
    let sender=requests::delegate_id(run,source);
    let mut generic=requests::addressed_to(projection,&sender)?;
    if sender!=run {generic.extend(requests::addressed_to(projection,run)?);}
    generic.sort_by(|a,b|(&a.request.data.due,&a.request.id).cmp(&(&b.request.data.due,&b.request.id)));
    generic.dedup_by(|a,b|a.request.id==b.request.id);
    for view in generic {
        let (request,data)=(&view.request,&view.request.data);
        let question=data.ask==Ask::Question;
        let text=match data.ask {
            Ask::Question=>format!("请回答「{}」上的提问「{}」（截止 {}）：towermd reply {} --state <状态> --summary <摘要> --ref <依据>",data.target,data.text,data.due,request.id),
            Ask::Report=>format!("请汇报「{}」：{}（截止 {}）：towermd reply {} --state <状态> --summary <摘要>",data.target,data.text,data.due,request.id),
            Ask::Update=>format!("请按规则更新容器「{0}」：{1}（截止 {2}）。先 towermd frame show {0} 读规则和现有卡片，再用 towermd frame add/update/move 按规则改，改完 towermd reply {3} --state <状态> --summary <摘要>",data.target,data.text,data.due,request.id),
        };
        let mut envelope=tower_api::request_report(project_id,&data.target,&request.id,&data.due,&data.text,&at,question);
        if data.ask==Ask::Update {envelope.data["update"]=json!(true);}
        messages.push(json!({
            "message":{"kind":"request_report","request":request.id},"run":run,"actor":DAEMON,
            "request":request.id,"due":data.due,"target":data.target,
            "envelope":envelope,
            "text":text,
        }));
    }
    Ok(messages)
}

/// `towermd task show` additions (§5): the card, rules for its work kind,
/// past send-back reasons and open report requests.
pub(super) fn task_details(runtime:&ProjectRuntime,project:&Project,id:&str)->Result<Value,String> {
    let view=view(runtime,project)?;
    let Some(object)=view.objects.iter().find(|object|object.id==id) else {
        return Ok(json!({"card":null,"rules":[],"sent_back":[],"report_requests":[]}));
    };
    let rules:Vec<_>=view.rules.iter().filter(|rule|rule.work_kind==object.work_kind||rule.work_kind=="*").collect();
    let sent_back:Vec<_>=object.attempts.iter().filter_map(|attempt|attempt.verdict.as_ref()
        .filter(|verdict|verdict.kind==VerdictKind::SentBack)
        .map(|verdict|json!({"attempt":attempt.id,"reason":verdict.reason,"at":verdict.event.at}))).collect();
    let requests:Vec<_>=view.report_requests.iter().filter(|request|request.request.object==id&&request.request.open).collect();
    Ok(json!({"card":object,"rules":rules,"sent_back":sent_back,"report_requests":requests}))
}

fn same_or_new(runtime:&ProjectRuntime,request:NewEvent)->Result<NewEvent,ApiError> {
    match human_tasks::original(runtime,&request.eid).map_err(bad)? {
        Some(old) if old==request=>Ok(old),
        Some(_)=>Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容或来源")),
        None=>Ok(request),
    }
}
fn append(runtime:&mut ProjectRuntime,request:NewEvent)->Result<Event,ApiError> {
    runtime.append(request,now).map_err(|error|match error {RuntimeError::Invalid(message)=>bad(message),other=>runtime_failure(other)})
}
fn accepted(event:&Event)->Json<Value> {
    Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","obj":event.obj}))
}

/// `{request_id, …}` where the rest is exactly the event body `T`.
fn split<T:DeserializeOwned+Serialize>(mut input:Value,extra:&[&str])->Result<(String,Vec<String>,Value),ApiError> {
    let fields=input.as_object_mut().ok_or_else(||bad("请求正文须是对象"))?;
    let mut take=|key:&str|fields.remove(key).and_then(|value|value.as_str().map(str::to_owned)).ok_or_else(||bad(format!("缺少 {key}")));
    let request_id=take("request_id")?;
    let extra=extra.iter().map(|key|take(key)).collect::<Result<Vec<_>,_>>()?;
    let data:T=serde_json::from_value(input).map_err(bad)?;
    Ok((request_id,extra,serde_json::to_value(data).map_err(bad)?))
}

/// Runs before a new human event is written, under the runtime lock.
type Precheck=fn(&Server,&Project,&mut ProjectRuntime,&str)->Result<(),ApiError>;
fn no_precheck(_:&Server,_:&Project,_:&mut ProjectRuntime,_:&str)->Result<(),ApiError> {Ok(())}

/// One human event signed `human:local`; a retry with the same request ID
/// and body returns the original.
pub(super) async fn record(server:Arc<Server>,handle:String,request_id:String,kind:&'static str,layer:&'static str,obj:String,data:Value)->Result<Json<Value>,ApiError> {
    record_checked(server,handle,request_id,kind,layer,obj,data,no_precheck).await
}
#[allow(clippy::too_many_arguments)]
async fn record_checked(server:Arc<Server>,handle:String,request_id:String,kind:&'static str,layer:&'static str,obj:String,data:Value,precheck:Precheck)->Result<Json<Value>,ApiError> {
    valid_scan_id(&request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        let request=same_or_new(runtime,NewEvent{eid:request_id,host:server.endpoint.host.clone(),actor:HUMAN.into(),
            kind:kind.into(),layer:layer.into(),obj,data,guards:vec![]})?;
        if runtime.receipt(&request.eid).is_none() {precheck(&server,&project,runtime,&request.obj)?;}
        let event=append(runtime,request)?;
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event))
    }).await.map_err(uncertain)?
}

pub(super) fn app(headers:&HeaderMap,server:&Server)->Result<(),ApiError> {
    if crate::daemon::authorized_app(headers,server) {Ok(())} else {Err(denied())}
}

pub async fn finalize(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,_,data)=split::<Finalized>(input,&[])?;
    record(server,handle,request_id,"delegation.finalized","L4",id,data).await
}
pub async fn used(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,_,data)=split::<Used>(input,&[])?;
    record(server,handle,request_id,"acceptance.used","L4",id,data).await
}
/// `takeaway` stays in the event: no general human decision record exists
/// outside question answers, and the read model shows it as the person's.
pub async fn confirm(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,_,data)=split::<Confirmed>(input,&[])?;
    record_checked(server,handle,request_id,"acceptance.confirmed","L4",id,data,gate_confirm).await
}
/// FR-121/122: a person completes a claimed object only when no deterministic
/// check applies to it, and never past a blocking gate. Confirming work the
/// kit already proved done is acceptance, not completion.
fn gate_confirm(server:&Server,project:&Project,runtime:&mut ProjectRuntime,id:&str)->Result<(),ApiError> {
    let items=log::delegations(runtime.projection(),KITS).map_err(bad)?;
    if !items.iter().any(|item|item.id==id && item.phase==Phase::ClaimedDone) {return Ok(());}
    let catalog=current_catalog(project).map_err(bad)?;
    let judgement=gates::enforce(server,runtime,&catalog,id,"confirm",true).map_err(uncertain)?;
    gates::require_clear(&judgement)?;
    if judgement.verdict!=GateVerdict::HumanOnly {
        return Err(rejected_as(ErrorCode::GateFailed)("该对象有确定性检查，只能由检查通过来完成"));
    }
    Ok(())
}
/// FR-72: a person waives one declared hard gate on one object until a
/// time, at most 30 days away. Only a gate that holds the object back right
/// now can be waived, judged against main's declaration without waivers: the
/// kit's own checks and broken declarations never can. A retry with the same
/// request ID and body returns the original even after the gate cleared.
pub async fn waive(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,_,data)=split::<Granted>(input,&[])?;
    let body:Granted=serde_json::from_value(data.clone()).map_err(bad)?;
    let usage=rejected_as::<String>(ErrorCode::Usage);
    if let Err(error)=waivers::check_gate(&body.gate).and_then(|_|waivers::check_reason(&body.reason)) {return Err(usage(error));}
    valid_scan_id(&request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        let request=same_or_new(runtime,NewEvent{eid:request_id,host:server.endpoint.host.clone(),actor:HUMAN.into(),
            kind:waivers::GRANTED.into(),layer:waivers::LAYER.into(),obj:id,data,guards:vec![]})?;
        if runtime.receipt(&request.eid).is_none() {
            waivers::check_until(&body.until,instant(now())).map_err(rejected_as(ErrorCode::Usage))?;
            waivable(&project,runtime,&request.obj,&body.gate)?;
        }
        let event=append(runtime,request)?;
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event))
    }).await.map_err(uncertain)?
}
fn waivable(project:&Project,runtime:&ProjectRuntime,object:&str,gate:&str)->Result<(),ApiError> {
    let catalog=current_catalog(project).map_err(bad)?;
    let file=GateFile::from_text(catalog.read_file(towermd_core::gates::GATES_PATH).map_err(bad)?.as_deref());
    let judgement=towermd_core::gates::judge_object(runtime.projection(),KITS,&file,object).map_err(bad)?;
    match judgement.gates.iter().find(|result|result.gate==gate&&!result.builtin) {
        Some(result) if result.hard && matches!(result.status,GateStatus::Failed|GateStatus::Pending)=>Ok(()),
        Some(result) if result.status==GateStatus::Error=>Err(bad(format!("门禁 {gate} 的声明有误，不能豁免；请先修正 gates.yaml"))),
        Some(_)=>Err(bad(format!("门禁 {gate} 当前没有拦住 {object}，无需豁免"))),
        None=>Err(rejected_as(ErrorCode::NotFound)(format!("主干的 gates.yaml 没有门禁 {gate}；内置检查不能豁免"))),
    }
}
pub async fn send_back(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,_,data)=split::<SentBack>(input,&[])?;
    record(server,handle,request_id,"acceptance.sent_back","L4",id,data).await
}
/// Body `{request_id, id, kind, text, source?}`; `id` names the rule.
pub async fn rule(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,mut extra,data)=split::<RuleRecorded>(input,&["id"])?;
    record(server,handle,request_id,"rule.recorded","L3",extra.remove(0),data).await
}
pub async fn rename(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,_,data)=split::<Renamed>(input,&[])?;
    let obj=data["from"].as_str().unwrap_or_default().to_owned();
    record(server,handle,request_id,"work_kind.renamed","L3",obj,data).await
}
pub async fn level(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,_,data)=split::<LevelSet>(input,&[])?;
    // Only a level the project names: a typo must not become a level whose
    // meaning nobody chose (FR-185).
    let level=data["level"].as_str().unwrap_or_default();
    let named=settings(&*project(&server,&handle).map_err(bad)?).delegation_levels;
    if !named.iter().any(|name|name==level) {
        return Err(bad(format!("委托级别 {level} 不在项目配置的级别里：{}",named.join("、"))));
    }
    let obj=data["kind"].as_str().unwrap_or_default().to_owned();
    record(server,handle,request_id,"delegation.level.set","L3",obj,data).await
}
/// A person's "知道了" or "静音" on one merged signal; only the App sends it.
pub async fn dismiss_signal(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Value>)->Result<Json<Value>,ApiError> {
    app(&headers,&server)?;
    let (request_id,_,data)=split::<tower_api::Dismissed>(input,&[])?;
    let obj=data["target"].as_str().unwrap_or_default().to_owned();
    record(server,handle,request_id,tower_api::DISMISSED,tower_api::LAYER,obj,data).await
}

/// The delegate restates the task, drafts done-when and asks ≤3 questions.
pub async fn clarify(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Clarify>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        let request=same_or_new(runtime,NewEvent{eid:input.request_id,host:server.endpoint.host.clone(),actor:CLI.into(),
            kind:"delegation.clarified".into(),layer:"L4".into(),obj:id,
            data:serde_json::to_value(&input.clarification).map_err(bad)?,guards:vec![]})?;
        // Until a person finalizes the card, the brief is the kit's own; a
        // clarification drafting fewer done-when items lowers the bar.
        let brief=log::delegations(runtime.projection(),KITS).map_err(bad)?.into_iter()
            .find(|item|item.id==request.obj).map(|item|item.brief.done_when).unwrap_or_default();
        let event=append(runtime,request)?;
        let drafted=&input.clarification.done_when;
        if !drafted.is_empty() {
            let project_id=project.project_id.clone().ok_or_else(||bad("项目尚未初始化"))?;
            weakenings::raise(runtime,&server.endpoint.host,&project_id,&event.eid,&event.obj,event.eid.clone(),
                Some(Author{object:&event.obj,source:input.clarification.self_reported.as_ref()}),&weakening::criteria(&brief,drafted)).map_err(uncertain)?;
        }
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event))
    }).await.map_err(uncertain)?
}

/// Answers a report request of the caller's own run, or a generic request or
/// node question addressed to its delegate (FR-116, FR-127). The
/// self-reported configuration is the one the run was claimed with.
pub async fn reply(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Reply>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let checkpoint=id.starts_with(requests::CHECKPOINT_PREFIX);
    if checkpoint && (input.from.is_some()||!input.refs.is_empty()) {
        return Err(bad("检查点汇报不接受 --from 或 --ref"));
    }
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        let request=if let Some(old)=human_tasks::original(runtime,&input.request_id).map_err(bad)? {
            let conflict=||rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他汇报");
            if old.host!=server.endpoint.host || old.actor!=CLI || old.obj!=id {return Err(conflict());}
            let same=if checkpoint {
                let data:Replied=serde_json::from_value(old.data.clone()).map_err(bad)?;
                old.kind=="report.replied" && (data.state,&data.summary,&data.next)==(input.state,&input.summary,&input.next)
            } else {
                let data:requests::Reply=serde_json::from_value(old.data.clone()).map_err(bad)?;
                old.kind==requests::REPLIED && (data.state,&data.summary,&data.next,&data.refs)==(input.state,&input.summary,&input.next,&input.refs)
                    && input.from.as_ref().is_none_or(|from|*from==data.from)
            };
            if !same {return Err(conflict());}
            old
        } else if checkpoint {
            let run=tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),true).map_err(bad)?.ok_or_else(||bad("缺少活动运行"))?;
            let projection=runtime.projection();
            let requests=log::report_requests(&log::delegations(projection,KITS).map_err(bad)?,&log::read(projection).map_err(bad)?);
            let request=requests.iter().find(|request|request.id==id).ok_or_else(||rejected_as(ErrorCode::NotFound)("汇报请求不存在"))?;
            if request.attempt!=run.binding.id {return Err(bad("汇报请求不属于当前运行"));}
            let data=Replied{state:input.state,summary:input.summary,next:input.next,self_reported:run.binding.self_reported.clone()};
            NewEvent{eid:input.request_id,host:server.endpoint.host.clone(),actor:CLI.into(),kind:"report.replied".into(),
                layer:"L5".into(),obj:id,data:serde_json::to_value(data).map_err(bad)?,guards:vec![]}
        } else {
            if !requests::read(runtime.projection()).map_err(bad)?.contains_key(&id) {
                return Err(rejected_as(ErrorCode::NotFound)("请求不存在"));
            }
            // Outside a run the sender names itself, as `towermd report` does.
            let run=match tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),false) {
                Ok(run)=>run,
                Err(error) if input.run.is_some()=>return Err(bad(error)),
                Err(_)=>None,
            };
            let from=input.from.or_else(||run.as_ref().map(|run|requests::delegate_id(&run.binding.id,run.binding.self_reported.as_ref())))
                .ok_or_else(||rejected_as(ErrorCode::Usage)("不在运行中时须用 --from 说明来源"))?;
            let data=requests::Reply{from,state:input.state,summary:input.summary,next:input.next,refs:input.refs,
                self_reported:run.and_then(|run|run.binding.self_reported)};
            NewEvent{eid:input.request_id,host:server.endpoint.host.clone(),actor:CLI.into(),kind:requests::REPLIED.into(),
                layer:requests::LAYER.into(),obj:id,data:serde_json::to_value(data).map_err(bad)?,guards:vec![]}
        };
        let event=append(runtime,request)?;
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event))
    }).await.map_err(uncertain)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bodies_carry_only_the_event_fields_and_a_request_id() {
        let (id,extra,data)=split::<RuleRecorded>(json!({"request_id":"r","id":"rule-1","kind":"docs","text":"cite"}),&["id"]).unwrap();
        assert_eq!((id.as_str(),extra,data),("r",vec!["rule-1".to_string()],json!({"kind":"docs","text":"cite"})));
        for key in ["actor","layer","obj","t"] {
            let mut input=json!({"request_id":"r","reason":"x","review_ms":1});
            input[key]=json!("human:forged");
            assert!(split::<SentBack>(input,&[]).is_err(),"{key}");
        }
        assert!(split::<Used>(json!({"criteria":[0]}),&[]).is_err());
        assert!(split::<Used>(json!([1]),&[]).is_err());
    }

    #[test]
    fn cli_bodies_cannot_sign_or_choose_the_run_owner() {
        let clarify=json!({"request_id":"r","clarification":{"restated":"x","done_when":["a"]}});
        assert!(serde_json::from_value::<Clarify>(clarify.clone()).is_ok());
        let mut forged=clarify;
        forged["actor"]=json!("human:local");
        assert!(serde_json::from_value::<Clarify>(forged).is_err());
        let reply=json!({"request_id":"r","cwd":"/tmp","state":"waiting","summary":"s"});
        assert!(serde_json::from_value::<Reply>(reply.clone()).is_ok());
        for (key,value) in [("self_reported",json!({"name":"x","version":"1"})),("actor",json!("human:local"))] {
            let mut forged=reply.clone();
            forged[key]=value;
            assert!(serde_json::from_value::<Reply>(forged).is_err(),"{key}");
        }
        let mut bad_state=reply;
        bad_state["state"]=json!("done");
        assert!(serde_json::from_value::<Reply>(bad_state).is_err());
    }
}
