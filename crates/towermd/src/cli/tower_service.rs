//! Tower API entries (core spec §5, FR-114–120): three equivalent ways in,
//! one write path ([`admit`] then [`commit`]).
//!
//! - CLI, `towermd report` and `towermd signal`: the daemon wraps the body in
//!   the unified envelope, dates it and signs it `process:towermd-cli`; inside
//!   a run `from` defaults to the run's delegate.
//! - HTTP, `POST /tower`: a bare envelope from any local client but the App,
//!   signed `process:towermd-http`.
//! - File: one bare envelope per line in `*.jsonl` files dropped into the
//!   project's inbox under the daemon's runtime directory, outside every
//!   repository; maintenance turns them into events signed
//!   `process:towermd-file` and deletes them. Files are never state.
//!
//! `from` is always self-reported. Installed extensions bound what a
//! `pkg:<id>` sender may claim (FR-123). Nothing here completes anything: a
//! report is a claim, never evidence.
//!
//! The `card` verb (FR-147) writes a frame's cards. The first write takes
//! the frame's lease `frame:<id>` for its sender, each further write renews
//! it, and another sender is refused while it is held (FR-187). The reply
//! says whether the write took effect or now waits for a person.
use super::business::{now, ready, uncertain, open_failure, runtime_failure};
use super::*;
use towermd_core::{cards, extensions::Registry, hub::Fence, hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}, tower_api::{self, Envelope, From as Sender, FromKind, Verb}};

const CLI: &str = "process:towermd-cli";
const HTTP: &str = "process:towermd-http";
const FILE: &str = "process:towermd-file";
/// One inbox line holds one envelope; longer lines are refused unread.
const LINE_BYTES: usize = 64 * 1024;
/// A file still being written is left alone until it has been quiet this long.
const QUIET: Duration = Duration::from_secs(2);
const PROCESSING: &str = ".processing";
const REJECTED: &str = "rejected";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    request_id:String, cwd:PathBuf, #[serde(default)] run:Option<String>,
    verb:Verb, #[serde(default)] target:Option<String>, #[serde(default)] from:Option<Sender>,
    data:Value, #[serde(default)] refs:Vec<String>,
}

/// The run's delegate as `name/version`, or the run ID when it said nothing.
fn delegate(run:&towermd_devkit::task_model::Run)->Sender {
    Sender{id:towermd_core::requests::delegate_id(&run.binding.id,run.binding.self_reported.as_ref()),kind:FromKind::Delegate}
}

/// A retry with the same request ID returns the original event; the daemon's
/// own date and defaults are not part of the comparison.
fn same(old:&NewEvent,host:&str,kind:&str,input:&Message)->bool {
    let Ok(envelope)=serde_json::from_value::<Envelope>(old.data.clone()) else {return false};
    old.host==host && old.actor==CLI && old.kind==kind && envelope.data==input.data && envelope.refs==input.refs
        && input.target.as_ref().is_none_or(|target|*target==old.obj)
        && input.from.as_ref().is_none_or(|from|*from==envelope.from)
}

/// The event a message of this verb becomes; a card's from its operation.
fn kind_of(verb:Verb,data:&Value)->Result<&'static str,ApiError> {
    if verb==Verb::Card {
        return serde_json::from_value::<cards::Op>(data.clone()).map(|op|op.kind()).map_err(|error|bad(format!("卡片操作无效：{error}")));
    }
    verb.event_kind().ok_or_else(||bad("只接受 report、signal 或 card"))
}

/// FR-187: the sender's lease on the frame it writes, taken or renewed.
fn frame_lease(runtime:&mut ProjectRuntime,envelope:&Envelope)->Result<Vec<Fence>,ApiError> {
    let op=envelope.card().map_err(bad)?;
    let frame=cards::frame_of(&op,&envelope.target).map_err(bad)?;
    let name=cards::lease_name(frame);
    let holder=envelope.from.id.as_str();
    let leases=match runtime.work_leases(now()).get(&name) {
        Some(lease) if lease.holder==holder=>runtime.renew_work_leases(vec![lease.fence()],cards::LEASE_TTL_MS,now),
        Some(lease)=>return Err(rejected_as(ErrorCode::LeaseBusy)(format!("容器 {frame} 正由 {} 写入，租约到期或对方写完后再试",lease.holder))),
        None=>runtime.acquire_work_leases(holder,vec![name],cards::LEASE_TTL_MS,now),
    }.map_err(|error|match error {
        RuntimeError::Fenced(_)=>rejected_as(ErrorCode::LeaseBusy)(error),
        // Paused projects hand out no new work leases.
        RuntimeError::Invalid(_)=>rejected_as(ErrorCode::Conflict)(error),
        other=>runtime_failure(other),
    })?;
    Ok(leases.iter().map(|lease|lease.fence()).collect())
}

/// The one write path of the three entries: the envelope must be this
/// project's and valid, and an extension may only send what it declared.
fn admit(server:&Server,project:&Project,runtime:&mut ProjectRuntime,extensions:&Registry,eid:String,actor:&str,envelope:Envelope)->Result<NewEvent,ApiError> {
    let project_id=project.project_id.as_deref().ok_or_else(||bad("项目尚未初始化"))?;
    if envelope.project!=project_id {
        return Err(bad(format!("信封的项目 {} 不是 {project_id}",envelope.project)));
    }
    envelope.check().map_err(bad)?;
    let kind=envelope.kind().map_err(bad)?;
    extensions.permit(&envelope).map_err(bad)?;
    let guards=if cards::is_write(kind) {frame_lease(runtime,&envelope)?} else {vec![]};
    Ok(NewEvent{eid,host:server.endpoint.host.clone(),actor:actor.into(),kind:kind.into(),
        layer:tower_api::LAYER.into(),obj:envelope.target.clone(),data:serde_json::to_value(envelope).map_err(bad)?,guards})
}

fn commit(runtime:&mut ProjectRuntime,request:NewEvent)->Result<Value,ApiError> {
    // No eager snapshot: signals can be frequent, and maintenance
    // publishes the fast layer when it is due.
    let event=runtime.append(request,now).map_err(|error|match error {RuntimeError::Invalid(message)=>bad(message),other=>runtime_failure(other)})?;
    let mut reply=json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","obj":event.obj,"envelope":event.data});
    if cards::is_write(&event.kind) {
        if event.stale {
            return Err(rejected_as(ErrorCode::StaleEpoch)("容器租约已失效，这次写入没有生效；重试会重新取得租约"));
        }
        let op=serde_json::from_value::<Envelope>(event.data.clone()).map_err(bad)?.card().map_err(bad)?;
        let frame=cards::frame_of(&op,&event.obj).map_err(bad)?;
        reply["card"]=cards::outcome(runtime.projection(),frame,event.seq).map_err(bad)?;
    }
    Ok(reply)
}

/// Entries without request IDs name a message by its content: the same
/// envelope through the same entry is the same message, so a resent HTTP
/// call or a file read again after a crash is recorded once.
fn content_eid(actor:&str,envelope:&Envelope)->Result<String,ApiError> {
    let mut hash=Sha256::new();
    hash.update(actor.as_bytes());
    hash.update([0]);
    hash.update(serde_json::to_vec(envelope).map_err(bad)?);
    Ok(format!("git:{:x}",hash.finalize()))
}

fn record_envelope(server:&Server,project:&Project,runtime:&mut ProjectRuntime,extensions:&Registry,actor:&str,envelope:Envelope)->Result<Value,ApiError> {
    let eid=content_eid(actor,&envelope)?;
    let request=match human_tasks::original(runtime,&eid).map_err(bad)? {
        Some(old)=>{
            if old.actor!=actor || old.data!=serde_json::to_value(&envelope).map_err(bad)? {
                return Err(rejected_as(ErrorCode::Conflict)("消息 ID 已用于其他消息"));
            }
            old
        }
        None=>admit(server,project,runtime,extensions,eid,actor,envelope)?,
    };
    commit(runtime,request)
}

pub async fn record(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,Json(input):Json<Message>)->Result<Json<Value>,ApiError> {
    tasks::require_cli(&headers,&server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let kind=kind_of(input.verb,&input.data)?;
    if input.verb==Verb::Card && input.target.is_none() {
        return Err(rejected_as(ErrorCode::Usage)("card 须指明容器或卡片"));
    }
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
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
            let extensions=Registry::installed(&project.root).map_err(uncertain)?;
            admit(&server,&project,runtime,&extensions,input.request_id,CLI,envelope)?
        };
        commit(runtime,request).map(Json)
    }).await.map_err(uncertain)?
}

/// The open project an envelope names. The HTTP entry has no handle, so the
/// project must already be open in this daemon.
fn opened(server:&Server,project_id:&str)->Result<Arc<Project>,ApiError> {
    server.projects.lock().map_err(|_|uncertain("项目注册不可用"))?.handles.values()
        .find(|project|project.project_id.as_deref()==Some(project_id)).cloned()
        .ok_or_else(||coded(StatusCode::NOT_FOUND,ErrorCode::NotFound,format!(
            "项目 {project_id} 未在守护进程中打开；在该仓库内运行任一 towermd 命令或在 App 中打开它")))
}

/// `POST /tower` (FR-117). The instance header keeps out web pages and
/// scripts aimed at an old daemon; the App records people through its own
/// routes. Authentication waits for security hardening.
pub async fn http(State(server):State<Arc<Server>>,headers:HeaderMap,Json(envelope):Json<Envelope>)->Result<Json<Value>,ApiError> {
    if !crate::daemon::authorized(&headers,&server)
        || headers.get("x-towermd-client").and_then(|value|value.to_str().ok())==Some("app") {
        return Err(denied());
    }
    let project=opened(&server,&envelope.project)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        let extensions=Registry::installed(&project.root).map_err(uncertain)?;
        record_envelope(&server,&project,runtime,&extensions,HTTP,envelope).map(Json)
    }).await.map_err(uncertain)?
}

/// The file entry's inbox for a project, outside every repository.
pub(crate) fn inbox(runtime:&Path,project_id:&str)->Option<PathBuf> {
    let safe=!project_id.is_empty() && project_id.len()<=128 && !project_id.starts_with('.')
        && project_id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'));
    safe.then(||runtime.join("inbox").join(project_id))
}

fn regular_files(directory:&Path)->Result<Vec<(PathBuf,fs::Metadata)>,String> {
    let mut files=Vec::new();
    for entry in fs::read_dir(directory).map_err(|e|format!("{}: {e}",directory.display()))? {
        let entry=entry.map_err(|e|e.to_string())?;
        let path=entry.path();
        let name=entry.file_name();
        let name=name.to_string_lossy();
        let meta=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;
        if meta.file_type().is_file() && !name.starts_with('.') && name.ends_with(".jsonl") {
            files.push((path,meta));
        }
    }
    files.sort_by(|a,b|a.0.cmp(&b.0));
    Ok(files)
}

/// Maintenance: moves quiet inbox files aside, records each line and deletes
/// the file. Refused lines go to `rejected/` with the reason; a transient
/// failure leaves the file for the next round, where recorded lines dedupe.
pub(super) fn drain(server:&Server,project:&Project,slot:&mut Option<ProjectRuntime>)->Result<(),String> {
    let Some(inbox)=project.project_id.as_deref().and_then(|id|inbox(&server.runtime,id)) else {return Ok(())};
    fs::create_dir_all(&inbox).map_err(|e|format!("{}: {e}",inbox.display()))?;
    let processing=inbox.join(PROCESSING);
    directory(&processing)?;
    for (path,meta) in regular_files(&inbox)? {
        if meta.modified().ok().and_then(|at|at.elapsed().ok()).is_none_or(|age|age<QUIET) {continue;}
        let name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        // Claimed names sort in claim order and never collide with a leftover.
        fs::rename(&path,processing.join(format!("{}-{name}",Uuid::now_v7()))).map_err(|e|e.to_string())?;
    }
    let files=regular_files(&processing)?;
    if files.is_empty() {return Ok(())}
    let runtime=ready(slot,server,project).map_err(|e|e.to_string())?;
    let extensions=Registry::installed(&project.root)?;
    for (path,_) in files {
        consume(server,project,runtime,&extensions,&inbox,&path)?;
    }
    Ok(())
}

fn consume(server:&Server,project:&Project,runtime:&mut ProjectRuntime,extensions:&Registry,inbox:&Path,path:&Path)->Result<(),String> {
    let bytes=regular_bytes(path)?;
    let mut rejected=Vec::new();
    for (index,line) in bytes.split(|byte|*byte==b'\n').enumerate() {
        let line=line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) {continue;}
        let outcome=if line.len()>LINE_BYTES {
            Err(bad(format!("一行不能超过 {LINE_BYTES} 字节")))
        } else {
            serde_json::from_slice::<Envelope>(line).map_err(|e|bad(format!("信封无效：{e}")))
                .and_then(|envelope|record_envelope(server,project,runtime,extensions,FILE,envelope))
        };
        match outcome {
            Ok(_)=>{}
            Err((status,body)) if status.is_server_error()=>return Err(body["error"].as_str().unwrap_or("写入失败").to_owned()),
            Err((_,body))=>{
                let text=String::from_utf8_lossy(&line[..line.len().min(LINE_BYTES)]).into_owned();
                rejected.push(json!({"line":index+1,"error":body["error"],"code":body["code"],"text":text}));
            }
        }
    }
    if !rejected.is_empty() {
        let directory=inbox.join(REJECTED);
        fs::create_dir_all(&directory).map_err(|e|e.to_string())?;
        let name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let lines=rejected.iter().map(|entry|format!("{entry}\n")).collect::<String>();
        fs::write(directory.join(name),lines).map_err(|e|e.to_string())?;
        eprintln!("文件入口拒收 {} 行，原因见 {}",rejected.len(),directory.display());
    }
    fs::remove_file(path).map_err(|e|e.to_string())
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

    #[test]
    fn entries_sign_as_processes_and_name_messages_by_content() {
        for actor in [CLI,HTTP,FILE] {
            tower_api::check_signer(actor).unwrap();
        }
        let envelope:Envelope=serde_json::from_value(json!({"v":1,"project":"p","from":{"id":"ci/1","kind":"source"},
            "target":"p","verb":"signal","data":{"kind":"eng.x","level":"log","title":"t"},"at":"2026-10-23T00:00:00Z"})).unwrap();
        let eid=content_eid(HTTP,&envelope).unwrap();
        assert_eq!(eid,content_eid(HTTP,&envelope).unwrap());
        assert_ne!(eid,content_eid(FILE,&envelope).unwrap());
        assert!(eid.starts_with("git:") && eid.len()==68);
    }

    #[test]
    fn the_inbox_lives_under_the_runtime_directory_for_safe_project_ids_only() {
        let runtime=Path::new("/rt");
        assert_eq!(inbox(runtime,"tower-project"),Some(PathBuf::from("/rt/inbox/tower-project")));
        for bad in ["","..",".hidden","a/b","a\\b","a b"] {
            assert_eq!(inbox(runtime,bad),None,"{bad}");
        }
    }
}
