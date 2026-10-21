//! M1 proposal admission: clients give reasons, while immutable Git/draft bytes
//! determine differences and impact. Only the App can approve or reject.
use super::*;
use super::business::{now,ready,uncertain};
use towermd_core::{hub_protocol::NewEvent, project_runtime::ProjectRuntime};
use towermd_devkit::{dev_state, proposal_model::{self,Proposal}};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submit {request_id:String,cwd:PathBuf,#[serde(default)]run:Option<String>,input:proposal_model::Input}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reject {request_id:String,proposal_hash:String,reason:String}
fn record(runtime:&ProjectRuntime,id:&str)->Result<Value,String>{
    proposal_model::valid_id(id)?;
    runtime.projection()["Proposal"].get(id).cloned().ok_or("提案不存在".into())
}
fn submitted_input_matches(proposal:&Proposal,input:&proposal_model::Input)->bool{
    proposal.draft_id.as_deref()==Some(input.draft_id.as_str())&&proposal.source_proposal_id==input.source_proposal_id
        &&proposal.kind==input.kind&&proposal.reason==input.reason&&proposal.alternatives==input.alternatives
        &&proposal.recommendation==input.recommendation
}
pub async fn submit(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Submit>)->Result<Json<Value>,ApiError>{
    super::tasks::require_cli(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move||{
        super::tasks::coordinates(&project,&input.cwd).map_err(bad)?;
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let id=format!("P-{}",input.request_id);
        let existing=runtime.receipt(&input.request_id).map(|event|(event.actor.clone(),event.kind.clone(),event.obj.clone(),event.data.clone(),None))
            .or(runtime.pending().map_err(uncertain)?.into_iter().find(|event|event.eid==input.request_id)
                .map(|event|(event.actor.clone(),event.kind.clone(),event.obj.clone(),event.data.clone(),Some(event))));
        if let Some((actor,kind,obj,data,pending))=existing{
            let proposal:Proposal=serde_json::from_value(data).map_err(bad)?;
            if actor!="process:towermd-cli"||kind!="proposal.submitted"||obj!=id||!submitted_input_matches(&proposal,&input.input){return Err(bad("请求 ID 已绑定其他提案内容"));}
            if let Some(event)=pending{runtime.append(event,now).map_err(uncertain)?;}
            return Ok(Json(record(runtime,&id).map_err(bad)?));
        }
        super::tasks::renew(runtime,&server,&project,&input.cwd,input.run.as_deref(),false).map_err(bad)?;
        let catalog=super::finish::inspect_authority(&server,&project,runtime).map_err(bad)?;
        let baseline=super::tasks::baseline(&project,runtime).map_err(bad)?;
        let draft=towermd_devkit::draft_store::load(&catalog,&input.input.draft_id).map_err(bad)?;
        let tasks=dev_state::tasks(runtime.projection()).map_err(bad)?;
        let proposal=proposal_model::prepare(&catalog,&draft,&tasks,&baseline,&id,input.input).map_err(bad)?;
        runtime.append(NewEvent{eid:input.request_id,host:server.endpoint.host.clone(),actor:"process:towermd-cli".into(),
            kind:"proposal.submitted".into(),layer:"L4".into(),obj:id.clone(),data:serde_json::to_value(&proposal).map_err(bad)?,guards:vec![]},now).map_err(uncertain)?;
        Ok(Json(record(runtime,&id).map_err(bad)?))
    }).await.map_err(uncertain)?
}
pub async fn list(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>)->Result<Json<Value>,ApiError>{
    if !authorized_client(&headers,&server){return Err(denied());}
    let project=project(&server,&handle).map_err(bad)?;
    let slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
    let entries=slot.as_ref().and_then(|runtime|runtime.projection()["Proposal"].as_object()).map(|group|group.values().cloned().collect::<Vec<_>>()).unwrap_or_default();
    Ok(Json(json!({"proposals":entries})))
}
pub async fn get(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>)->Result<Json<Value>,ApiError>{
    if !authorized_client(&headers,&server){return Err(denied());}
    let project=project(&server,&handle).map_err(bad)?;
    let slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
    Ok(Json(record(slot.as_ref().ok_or_else(||bad("项目尚未激活"))?,&id).map_err(bad)?))
}
pub async fn prepare(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>)->Result<Json<Value>,ApiError>{
    if !crate::daemon::authorized_app(&headers,&server){return Err(denied());}
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move||{
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        super::finish::inspect_authority(&server,&project,runtime).map_err(bad)?;
        let proposal=proposal_model::from_state(runtime.projection(),&id).map_err(bad)?;
        let draft=proposal.draft_id.as_deref().ok_or_else(||bad("捕获草稿须先补全真实审阅基线，不能直接批准"))?;
        super::business::prepare_publication(&project,runtime,draft,Some(&id))
    }).await.map_err(uncertain)?
}
pub async fn approve(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<super::business::Approval>)->Result<Json<Value>,ApiError>{
    if !crate::daemon::authorized_app(&headers,&server){return Err(denied());}
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move||{
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        // Accepted approval retries must first prove the original publication;
        // they cannot be rejected merely because their own commit advanced main.
        super::finish::replay_pending(runtime).map_err(uncertain)?;
        if runtime.receipt(&input.request_id).is_none(){super::finish::inspect_authority(&server,&project,runtime).map_err(bad)?;}
        let proposal=proposal_model::from_state(runtime.projection(),&id).map_err(bad)?;
        let draft=proposal.draft_id.as_deref().ok_or_else(||bad("捕获草稿不能直接批准"))?;
        super::business::approve_publication(&server,&project,runtime,draft,Some(&id),input)
    }).await.map_err(uncertain)?
}
pub async fn reject(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Reject>)->Result<Json<Value>,ApiError>{
    if !crate::daemon::authorized_app(&headers,&server){return Err(denied());}
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move||{
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let data=json!({"proposal_hash":input.proposal_hash,"reason":input.reason});
        if let Some(event)=runtime.receipt(&input.request_id){
            if event.actor!="human:local"||event.kind!="proposal.rejected"||event.obj!=id||event.data!=data{return Err(bad("原请求与驳回内容不同"));}
        }else{
            let proposal=proposal_model::from_state(runtime.projection(),&id).map_err(bad)?;
            if proposal.content_hash!=input.proposal_hash||input.reason.trim().is_empty(){return Err(bad("驳回须绑定原提案摘要和原因"));}
            if runtime.projection()["Proposal"][&id]["status"]=="approved"{return Err(bad("已出版提案不能驳回"));}
            runtime.append(NewEvent{eid:input.request_id,host:server.endpoint.host.clone(),actor:"human:local".into(),kind:"proposal.rejected".into(),layer:"L4".into(),obj:id.clone(),data,guards:vec![]},now).map_err(uncertain)?;
        }
        Ok(Json(record(runtime,&id).map_err(bad)?))
    }).await.map_err(uncertain)?
}
/// Used by G2 and explicit main-resolution capture. This only stores original
/// evidence; it never accepts the changed commit as approved structure.
pub(super) fn capture(server:&Server,project:&Project,runtime:&mut ProjectRuntime,catalog:&Catalog,commit:&str,parent:&str,changes:&[towermd_core::path_guard::ChangedPath])->Result<Proposal,String>{
    let baseline=super::tasks::baseline(project,runtime)?;
    let tasks=dev_state::tasks(runtime.projection())?;
    let proposal=proposal_model::prepare_from_changes(catalog,&tasks,baseline.tag(),commit,parent,changes)?;
    if runtime.projection()["Proposal"].get(&proposal.id).is_some(){
        let original=proposal_model::from_state(runtime.projection(),&proposal.id)?;
        if original.project_id!=proposal.project_id||original.source_revision!=proposal.source_revision
            ||original.source_commit!=proposal.source_commit||original.source_parent!=proposal.source_parent
            ||original.changes!=proposal.changes{return Err("同一捕获提案的原始差异不一致".into());}
        return Ok(original);
    }
    let eid=towermd_core::events::derived_eid(&proposal.content_hash,"proposal.captured")?;
    if runtime.receipt(&eid).is_none(){
        runtime.append(NewEvent{eid,host:server.endpoint.host.clone(),actor:"process:towermd".into(),kind:"proposal.captured".into(),layer:"L4".into(),obj:proposal.id.clone(),data:serde_json::to_value(&proposal).map_err(|e|e.to_string())?,guards:vec![]},now).map_err(|e|e.to_string())?;
    }
    Ok(proposal)
}

pub(super) fn capture_run_violations(server:&Server,project:&Project,runtime:&mut ProjectRuntime,catalog:&Catalog,run:&towermd_devkit::task_model::Run)->Result<Vec<String>,String>{
    let tasks=dev_state::tasks(runtime.projection())?;
    let task=tasks.tasks.get(&run.task).ok_or("运行任务不存在")?;
    let root=Path::new(&run.binding.worktree);
    let (worktree,branch)=super::tasks::coordinates(project,root)?;
    if worktree!=run.binding.worktree||branch!=run.binding.branch{return Err("捕获运行工作树已改变".into());}
    let head=repo::git(root,&["rev-parse","--verify","HEAD"])?;
    let commits=repo::git(&catalog.root,&["--no-replace-objects","rev-list","--reverse",&format!("{}..{head}",catalog.revision)])?;
    let mut ids=Vec::new();
    for commit in commits.lines(){
        let parents=repo::git(&catalog.root,&["--no-replace-objects","show","-s","--format=%P",commit])?;
        for parent in parents.split_whitespace(){
            let guard=towermd_core::path_guard::check(&catalog.root,parent,commit,catalog.config.docs_dir(),&task.input.allowed_paths)?;
            if !guard.passed{
                ids.push(capture(server,project,runtime,catalog,commit,parent,&guard.changes)?.id);
            }
        }
    }
    Ok(ids)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureMain {commit:String}
pub async fn capture_main(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<CaptureMain>)->Result<Json<Value>,ApiError>{
    if !crate::daemon::authorized_app(&headers,&server){return Err(denied());}
    if !matches!(input.commit.len(),40|64)||!input.commit.bytes().all(|b|b.is_ascii_hexdigit()){return Err(bad("须指定完整固定提交"));}
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move||{
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let (catalog,inspection)=super::finish::observe_authority(&server,&project,runtime).map_err(bad)?;
        if !inspection.complete{return Err(bad("main 历史检查不完整，不能捕获"));}
        if !inspection.blocked_commits.iter().any(|entry|entry.commit==input.commit){return Err(bad("指定提交不是当前 main 的未处理受保护变更"));}
        let parents=repo::git(&catalog.root,&["--no-replace-objects","show","-s","--format=%P",&input.commit]).map_err(bad)?;
        let parent=parents.split_whitespace().next().ok_or_else(||bad("不能把导入边界转换为变更提案"))?;
        let guard=towermd_core::path_guard::check(&catalog.root,parent,&input.commit,catalog.config.docs_dir(),&[format!("{}/",catalog.config.docs_dir())]).map_err(bad)?;
        let proposal=capture(&server,&project,runtime,&catalog,&input.commit,parent,&guard.changes).map_err(uncertain)?;
        Ok(Json(record(runtime,&proposal.id).map_err(bad)?))
    }).await.map_err(uncertain)?
}
