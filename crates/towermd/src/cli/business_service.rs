//! App-only publication/recovery and the daemon-owned project runtime.
use super::*;
use towermd_core::{config::HubSetting, events::derived_eid, hub::Event, hub_protocol::NewEvent, main_writer::{WriteKind, WriteOutcome, WriteRequest}, project_runtime::{ProjectRuntime, RecoveryEvidence, RuntimeConfig, RuntimeError}};
use towermd_devkit::{baseline_publication, dev_state};

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub(super) fn config(server: &Server, project: &Project) -> Result<RuntimeConfig, String> {
    let catalog = current_catalog(project)?;
    Ok(RuntimeConfig {
        root: project.root.clone(),
        common_dir: project.common.clone(),
        project_id: catalog.config.project_id().into(),
        docs_dir: catalog.config.docs_dir().into(),
        host: server.endpoint.host.clone(),
    })
}
/// FR-84: the configured hub decides the sequencer. A remote hub is never
/// replaced by a local one, so an unreachable hub fails rather than forks.
fn open_runtime(server: &Server, project: &Project) -> Result<ProjectRuntime, RuntimeError> {
    let catalog = current_catalog(project).map_err(RuntimeError::Invalid)?;
    let config = config(server, project).map_err(RuntimeError::Invalid)?;
    match catalog.config.hub().map_err(RuntimeError::Invalid)? {
        HubSetting::Local => ProjectRuntime::open(config, dev_state::initial(), dev_state::apply, now),
        HubSetting::Remote { url } => {
            let hub = crate::remote_hub::RemoteHub::new(&url, catalog.config.project_id())
                .map_err(RuntimeError::Invalid)?;
            ProjectRuntime::open_remote(config, dev_state::initial(), dev_state::apply, Box::new(hub), now)
        }
    }
}
pub(super) fn ready<'a>(
    slot: &'a mut Option<ProjectRuntime>,
    server: &Server,
    project: &Project,
) -> Result<&'a mut ProjectRuntime, RuntimeError> {
    if slot.is_none() {
        *slot = Some(open_runtime(server, project)?);
    }
    Ok(slot.as_mut().unwrap())
}
pub(super) fn uncertain(error: impl ToString) -> ApiError {
    coded(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal, error)
}
fn hub_offline(error: impl ToString) -> ApiError {
    coded(StatusCode::SERVICE_UNAVAILABLE, ErrorCode::HubOffline, error)
}
/// A runtime failure after the project opened. An offline remote hub is a
/// transient `hub_offline`, never an internal error.
pub(super) fn runtime_failure(error: RuntimeError) -> ApiError {
    match error {
        RuntimeError::Offline(_) => hub_offline(error),
        other => uncertain(other),
    }
}
/// Opening a project failed; same classification as before, except that an
/// unreachable remote hub is reported as `hub_offline`.
pub(super) fn open_failure(error: RuntimeError) -> ApiError {
    match error {
        RuntimeError::Offline(_) => hub_offline(error),
        other => bad(other),
    }
}
pub(super) fn envelope(
    server: &Server,
    eid: String,
    actor: &str,
    kind: &str,
    obj: &str,
    data: Value,
) -> NewEvent {
    NewEvent {
        eid,
        host: server.endpoint.host.clone(),
        actor: actor.into(),
        kind: kind.into(),
        layer: "L1".into(),
        obj: obj.into(),
        data,
        guards: vec![],
    }
}

pub fn maintain(server: &Server) {
    let projects = match server.projects.lock() {
        Ok(registry) => registry.handles.values().cloned().collect::<Vec<_>>(),
        Err(_) => return,
    };
    for project in projects {
        let Ok(mut slot) = project.runtime.try_lock() else {
            continue;
        };
        if let Some(runtime) = slot.as_mut() {
            if let Err(error) = runtime.tick(now) {
                eprintln!("项目维护停止写入：{error}");
                // A remote mirror is derived state: rebuild it from Git and
                // the hub on the next use instead of staying stuck.
                if runtime.is_remote() && !matches!(error, RuntimeError::Offline(_)) {
                    *slot = None;
                }
                continue;
            }
            // Only the host holding `main` maintains the project: one committer.
            if !runtime.holds_main() {
                continue;
            }
            if let Err(error) = super::finish::maintain_project(server, &project, runtime) {
                eprintln!("任务维护待处理：{error}");
            }
            if let Err(error) = super::requests::write_silences(server, &project, runtime) {
                eprintln!("失联信号待重试：{error}");
            }
            if let Err(error) = super::requests::write_stuck(server, &project, runtime) {
                eprintln!("兜圈子信号待重试：{error}");
            }
            if let Err(error) = super::growth::write(server, &project, runtime) {
                eprintln!("生长信号待重试：{error}");
            }
            if let Err(error) = super::temp_rules::expire(server, runtime) {
                eprintln!("临时规则到期待记录：{error}");
            }
            if let Err(error) = super::notemd::writeback(server, &project, runtime) {
                eprintln!("Note.md 回写待重试：{error}");
            }
            if runtime.snapshot_due(now()) || runtime.maintenance_due(now()) {
                if let Err(error) = runtime.snapshot(now) {
                    eprintln!("项目定时快照待重试：{error}");
                }
            }
        }
        if let Err(error) = super::tower::drain(server, &project, &mut slot) {
            eprintln!("文件入口待重试：{error}");
        }
    }
}

/// Waiting time is display metadata. Resolve only referenced canonical
/// originals, without walking or changing durable business projection/history.
fn display_projection<'a>(projection:&Value,lookup:impl Fn(&str)->Option<&'a Event>)->Value {
    let mut display=projection.clone();
    for group in ["Alert","Proposal"] {
        let Some(entries)=display.get_mut(group).and_then(Value::as_object_mut) else {continue;};
        for (id,entry) in entries {
            let Some(fields)=entry.as_object_mut() else {continue;};
            // Never let a cached display timestamp outrank the actual original.
            fields.remove("at");
            let direct=fields.get("event").and_then(Value::as_str);
            let eid=direct.or_else(||fields.get("source_event").and_then(Value::as_str));
            let Some(event)=eid.and_then(&lookup).filter(|event|!event.stale) else {continue;};
            let matches=match (group,direct.is_some(),event.kind.as_str()) {
                ("Alert",true,"alert.raised")=>event.obj==*id,
                ("Proposal",true,"proposal.submitted"|"proposal.captured")=>event.obj==*id,
                ("Proposal",false,"hold.answered"|"prompt.queued")=>true,
                _=>false,
            };
            if matches {fields.insert("at".into(),json!(event.t));}
        }
    }
    display
}

pub async fn state(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || read_state(&server,&project).map(Json))
        .await.map_err(uncertain)?
}

pub(super) fn state_value(runtime:&ProjectRuntime,project:&Project)->Value {
    let projection=display_projection(runtime.projection(),|eid|runtime.receipt(eid));
    let mut state=json!({"status":"ready","projection":projection,
        "seq":runtime.contiguous_seq(),"snapshot_commit":runtime.snapshot_commit(),
        "delegation_view":super::delegations::view_value(runtime,project),
        "attention":attention_settings(project),
        "quiet":super::follow_ups::rhythm_value(runtime,project),
        "frames":frames_view(runtime,project),
        "starters":super::starters::view_value(runtime)});
    // FR-128: the ordered attention list, derived from this same body on every read.
    let now=now();
    let list=towermd_core::attention_list::list(&state,dev_state::KITS,now);
    // FR-39, FR-133: health colour, aggregates and the latest activity, derived on every read.
    let settings=towermd_core::config::Config::load(&project.root).ok().flatten().map(|config|config.health()).unwrap_or_default();
    let health=towermd_core::health::report(&state,dev_state::KITS,&list,settings,now);
    state["health"]=serde_json::to_value(health).unwrap_or(Value::Null);
    state["last_activity"]=json!(runtime.canonical_receipts().filter(|event|!event.stale).map(|event|event.t.as_str()).max());
    state["attention_list"]=serde_json::to_value(list).unwrap_or(Value::Null);
    state
}

/// Frames with their bound cards and whether they are frozen today (UTC),
/// derived on every read and never stored.
fn frames_view(runtime:&ProjectRuntime,project:&Project)->Value {
    let today=towermd_core::tower_api::timestamp(now()).unwrap_or_default();
    let mut views=towermd_core::cards::view(runtime.projection(),towermd_devkit::dev_state::KITS,today.get(..10).unwrap_or_default())
        .ok().and_then(|views|serde_json::to_value(views).ok()).unwrap_or(Value::Null);
    // FR-163: a text-source frame shows its file as main has it now.
    super::source_frames::attach(project,runtime,&mut views);
    views
}

/// FR-128, FR-130: the person's budget and focus time; the App derives the
/// three timings from them. A missing or unreadable config keeps the defaults.
fn attention_settings(project:&Project)->Value {
    let settings=towermd_core::config::Config::load(&project.root).ok().flatten().map(|config|config.attention()).unwrap_or_default();
    serde_json::to_value(settings).unwrap_or(Value::Null)
}

/// Shared HTTP/WS state reader. Only an initial read or changed authoritative
/// refs need the cold activation checks; a live subscription reads its existing
/// runtime directly after ticking the Hub.
pub(super) fn read_state(server:&Server,project:&Project)->Result<Value,ApiError> {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        if slot.is_none() {
            // A never-activated project's read-only status must not create a Hub.
            let marker = project.common.join("towermd/runtime.json");
            let latest = towermd_core::main_writer::latest_snapshot(&project.root).map_err(bad)?;
            if !marker.exists() && latest.is_none() {
                return Ok(json!({"status":"not_activated"}));
            }
            match open_runtime(&server, &project) {
                Ok(runtime) => *slot = Some(runtime),
                Err(RuntimeError::Offline(error)) => {
                    return Ok(json!({"status":"hub_offline","error":error}))
                }
                Err(error) => {
                    return Ok(json!({"status":"needs_recovery","error":error.to_string()}))
                }
            }
        }
        let runtime = slot.as_mut().unwrap();
        match runtime.tick(now) {
            Ok(()) => Ok(state_value(runtime,project)),
            // CLI spec 3.7: reads still answer from the last local projection.
            Err(RuntimeError::Offline(error)) => {
                let mut state = state_value(runtime,project);
                state["warnings"] = json!([format!("hub 离线，数据截至 seq {}：{error}", runtime.contiguous_seq())]);
                Ok(state)
            }
            Err(error) => Err(uncertain(error)),
        }
}

#[cfg(test)]
mod display_tests {
    use super::*;
    use towermd_core::{hub::{AppendReceipt,Hub}, hub_protocol::HubProtocol};

    fn original(hub:&mut Hub,kind:&str,obj:&str,time:u64)->Event {
        let AppendReceipt::Event(event)=hub.append(NewEvent{eid:Uuid::now_v7().to_string(),host:"local".into(),
            actor:"process:towermd".into(),kind:kind.into(),layer:"L5".into(),obj:obj.into(),
            data:json!({}),guards:vec![]},time).unwrap() else {panic!("fresh original required")};
        *event
    }
    #[test]
    fn waiting_times_come_from_referenced_originals_without_mutating_snapshot_projection() {
        let dir=tempfile::tempdir().unwrap();
        let mut hub=Hub::open(dir.path()).unwrap();
        let alert=original(&mut hub,"alert.raised","A-one",1000);
        let proposal=original(&mut hub,"proposal.submitted","P-one",2000);
        let answer=original(&mut hub,"hold.answered","H-one",3000);
        let prompt=original(&mut hub,"prompt.queued","P-prompt",4000);
        let originals=BTreeMap::from([(alert.eid.clone(),alert.clone()),(proposal.eid.clone(),proposal.clone()),
            (answer.eid.clone(),answer.clone()),(prompt.eid.clone(),prompt.clone())]);
        let projection=json!({"Alert":{"A-one":{"event":alert.eid,"resolved":false}},
            "Proposal":{"P-one":{"event":proposal.eid,"status":"submitted"},
                "P-answer":{"source_event":answer.eid,"status":"draft"},"P-prompt":{"source_event":prompt.eid,"status":"draft"}},
            "Task":{"T-one":{"state":"running"}}});
        let before=serde_json::to_vec(&projection).unwrap();
        let display=display_projection(&projection,|eid|originals.get(eid));
        assert_eq!(display["Alert"]["A-one"]["at"],alert.t);
        assert_eq!(display["Proposal"]["P-one"]["at"],proposal.t);
        assert_eq!(display["Proposal"]["P-answer"]["at"],answer.t);
        assert_eq!(display["Proposal"]["P-prompt"]["at"],prompt.t);
        assert_eq!(display["Task"],projection["Task"]);
        assert_eq!(serde_json::to_vec(&projection).unwrap(),before);
        assert!(projection["Alert"]["A-one"].get("at").is_none());
        assert!(projection["Proposal"]["P-one"].get("at").is_none());
    }
    #[test]
    fn missing_wrong_object_or_stale_originals_do_not_invent_a_waiting_start() {
        let dir=tempfile::tempdir().unwrap();
        let mut hub=Hub::open(dir.path()).unwrap();
        let mut stale=original(&mut hub,"proposal.submitted","P-stale",1000);stale.stale=true;
        let wrong=original(&mut hub,"alert.raised","A-other",2000);
        let originals=BTreeMap::from([(stale.eid.clone(),stale.clone()),(wrong.eid.clone(),wrong.clone())]);
        let projection=json!({"Alert":{"A-one":{"event":wrong.eid,"at":"unproven"},"A-missing":{"event":"absent"}},
            "Proposal":{"P-stale":{"event":stale.eid},"P-wrong-kind":{"source_event":wrong.eid}}});
        let display=display_projection(&projection,|eid|originals.get(eid));
        for group in ["Alert","Proposal"] {
            for entry in display[group].as_object().unwrap().values() {assert!(entry.get("at").is_none());}
        }
        assert_eq!(projection["Alert"]["A-one"]["at"],"unproven");
    }
}

pub(super) fn publication_input(project:&Project,runtime:&ProjectRuntime,id:&str,proposal_id:Option<&str>,request_id:&str,target:&str,prepared_at:&str)->Result<baseline_publication::Publication,String>{
    let catalog=current_catalog(project)?;
    let draft=towermd_devkit::draft_store::load(&catalog,id)?;
    if draft.scan.revision!=catalog.revision {return Err("批准来源已改变，请重新审阅".into());}
    let expected=format!("B{}",runtime.projection()["Baseline"].as_object().map_or(0,|m|m.len()));
    if target!=expected {return Err("目标基线编号已改变，请重新审阅".into());}
    let mut publication=baseline_publication::prepare(&draft,&project.common,request_id,target,prepared_at)?;
    if let Some(proposal_id)=proposal_id {
        let proposal=towermd_devkit::proposal_model::from_state(runtime.projection(),proposal_id)?;
        if runtime.projection()["Proposal"][proposal_id]["status"]!="submitted" {
            return Err("提案不是待批准状态".into());
        }
        towermd_devkit::proposal_model::validate_source(runtime.projection(),&proposal)?;
        let baseline=super::tasks::baseline(project,runtime)?;
        if proposal.baseline_tag!=baseline.tag() {
            return Err("提案基线已改变，请重新补全提案".into());
        }
        towermd_devkit::proposal_model::inherit_decisions(&mut publication,&catalog,&baseline)?;
        towermd_devkit::proposal_model::attach_to_publication(&mut publication,&proposal,catalog.config.docs_dir())?;
    } else if target!="B0" {return Err("后续基线必须通过提案提交差异、影响和方案".into());}
    Ok(publication)
}
pub(super) fn prepare_publication(project:&Project,runtime:&ProjectRuntime,id:&str,proposal_id:Option<&str>)->Result<Json<Value>,ApiError>{
    let target=format!("B{}",runtime.projection()["Baseline"].as_object().map_or(0,|m|m.len()));
    let publication=publication_input(project,runtime,id,proposal_id,&Uuid::now_v7().to_string(),&target,&baseline_publication::preview_time()).map_err(bad)?;
    Ok(Json(serde_json::to_value(publication).map_err(bad)?))
}
pub async fn prepare(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>)->Result<Json<Value>,ApiError>{
    if !crate::daemon::authorized_app(&headers,&server){return Err(denied());}
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move||{
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        prepare_publication(&project,runtime,&id,None)
    }).await.map_err(uncertain)?
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {pub(super) request_id:String,pub(super) target:String,pub(super) prepared_at:String,pub(super) content_hash:String}
pub(super) fn approve_publication(server:&Server,project:&Project,runtime:&mut ProjectRuntime,id:&str,proposal_id:Option<&str>,input:Approval)->Result<Json<Value>,ApiError>{
        super::finish::replay_pending(runtime).map_err(uncertain)?;
        let old=runtime.receipt(&input.request_id).cloned();
        let publication=if let Some(event)=old.as_ref() {
            baseline_publication::from_approval(event).map_err(bad)?
        } else {
            publication_input(project,runtime,&id,proposal_id,&input.request_id,&input.target,&input.prepared_at).map_err(bad)?
        };
        if publication.draft_id != id || publication.target != input.target
            || publication.prepared_at != input.prepared_at || publication.content_hash != input.content_hash
        {return Err(bad("批准请求与原预览不一致"));}
        let link=towermd_devkit::proposal_model::publication_link(&publication).map_err(bad)?;
        if link.as_ref().map(|link|link.id.as_str())!=proposal_id {return Err(bad("批准请求不属于本提案入口"));}
        let approval=match old {
            Some(event)=>event,
            None=>runtime.append(envelope(&server,input.request_id.clone(),"human:local",
                "baseline.approved",&input.target,serde_json::to_value(&publication).map_err(bad)?),
                now).map_err(uncertain)?,
        };
        let tag=format!("refs/tags/towermd/{}",publication.target);
        let existing=towermd_core::main_writer::read_publication(&project.root,&tag)
            .map_err(uncertain)?.map(|readback|readback.commit);
        let commit=if let Some(commit)=existing {
            publication.prove_commit(&project.root,&commit,&approval).map_err(uncertain)?;
            commit
        } else {
            let request=WriteRequest {
                docs_dir:current_catalog(&project).map_err(uncertain)?.config.docs_dir().into(),
                epoch:runtime.main_fence().map_err(runtime_failure)?.epoch,
                kind:WriteKind::Slow {expected_base:publication.source_revision.clone(),tag:Some(tag)},
                changes:publication.files.iter().map(|(p,b)|(p.clone(),Some(b.clone())))
                    .chain(publication.removals.iter().map(|path|(path.clone(),None))).collect(),
                message:format!("towermd: approve {}\n\nTowermd-Baseline: {}\nTowermd-Approval: {}\nTowermd-Content: {}",
                    publication.target,publication.target,approval.eid,publication.content_hash),
            };
            let outcome=runtime.publish(&request,now).map_err(uncertain)?;
            let WriteOutcome::Written {commit,..}=outcome else {return Err(uncertain("基线出版被更高水位取代"))};
            publication.prove_commit(&project.root,&commit,&approval).map_err(uncertain)?;
            commit
        };
        let created_eid=derived_eid(&commit,"baseline.created").map_err(uncertain)?;
        if runtime.receipt(&created_eid).is_none() {
            let mut created=envelope(&server,created_eid,"process:towermd","baseline.created",
                &publication.target,json!({"commit":commit,"approval_eid":approval.eid,
                    "content_hash":publication.content_hash}));
            created.guards=vec![runtime.main_fence().map_err(runtime_failure)?];
            runtime.append(created,now).map_err(runtime_failure)?;
        }
        let snapshot_commit=runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"target":publication.target,"commit":commit,
            "tag":format!("towermd/{}",publication.target),
            "content_hash":publication.content_hash,"snapshot_commit":snapshot_commit})))
}
pub async fn approve(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath((handle,id)):RoutePath<(String,String)>,Json(input):Json<Approval>)->Result<Json<Value>,ApiError>{
    if !crate::daemon::authorized_app(&headers,&server){return Err(denied());}
    let project=project(&server,&handle).map_err(bad)?;
    tokio::task::spawn_blocking(move||{
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(open_failure)?;
        approve_publication(&server,&project,runtime,&id,None,input)
    }).await.map_err(uncertain)?
}

pub async fn snapshot(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        Ok(Json(
            json!({"commit":runtime.snapshot(now).map_err(uncertain)?}),
        ))
    })
    .await
    .map_err(uncertain)?
}

fn rebuild_path(project: &Project, id: &str) -> Result<PathBuf, String> {
    valid_scan_id(id)?;
    Ok(project
        .common
        .join("towermd")
        .join(format!("rebuild-{id}.json")))
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RebuildRequest {
    #[serde(default)]
    request_id: Option<String>,
}
pub async fn request_rebuild(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
    body: Option<Json<RebuildRequest>>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    let id = body.and_then(|Json(body)| body.request_id).unwrap_or_else(|| Uuid::now_v7().to_string());
    let path = rebuild_path(&project, &id).map_err(rejected_as(ErrorCode::Usage))?;
    let content = serde_json::to_vec(&json!({"request_id":id,"project_id":project.project_id})).map_err(bad)?;
    let directory_path = project.common.join("towermd");
    directory(&directory_path).map_err(bad)?;
    match fs::read(&path) {
        Ok(existing) if existing == content => return Ok(Json(json!({"request_id":id}))),
        Ok(_) => return Err(rejected_as(ErrorCode::Conflict)("同一请求 ID 的重建请求内容不同")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => new_file(&path, &content).map_err(uncertain)?,
        Err(error) => return Err(uncertain(error)),
    }
    File::open(directory_path)
        .and_then(|file| file.sync_all())
        .map_err(uncertain)?;
    Ok(Json(json!({"request_id":id})))
}

pub async fn confirm_rebuild(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let path = rebuild_path(&project, &id).map_err(bad)?;
        let request: Value =
            serde_json::from_slice(&regular_bytes(&path).map_err(bad)?).map_err(bad)?;
        if request["request_id"] != id || request["project_id"] != json!(project.project_id) {
            return Err(bad("恢复请求不属于当前项目"));
        }
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        if let Some(runtime) = slot.as_ref() {
            let original = runtime
                .receipt(&id)
                .ok_or_else(|| bad("当前 Hub 正常运行，无需灾难恢复"))?;
            if original.kind != "hub.rebuild.confirmed"
                || original.actor != "human:local"
                || original.stale
            {
                return Err(bad("原请求不是有效的人工恢复确认"));
            }
            return Ok(Json(
                json!({"status":"ready","snapshot_commit":runtime.snapshot_commit(),
                "seq":runtime.contiguous_seq()}),
            ));
        }
        let mut confirmation = envelope(
            &server,
            id.clone(),
            "human:local",
            "hub.rebuild.confirmed",
            project
                .project_id
                .as_deref()
                .ok_or_else(|| bad("项目缺少 ID"))?,
            json!({"request_id":id}),
        );
        confirmation.layer = "L5".into();
        let catalog = current_catalog(&project).map_err(bad)?;
        if catalog.config.hub().map_err(bad)? != HubSetting::Local {
            return Err(bad("远端 hub 不在本机重建；v0.1 只支持本地 hub 的恢复"));
        }
        let runtime = ProjectRuntime::recover_confirmed(
            config(&server, &project).map_err(bad)?,
            dev_state::initial(),
            dev_state::apply,
            confirmation,
            now,
            recover_main,
        )
        .map_err(uncertain)?;
        let response = json!({"status":"ready","snapshot_commit":runtime.snapshot_commit(),
            "seq":runtime.contiguous_seq()});
        *slot = Some(runtime);
        Ok(Json(response))
    })
    .await
    .map_err(uncertain)?
}

fn recover_main(evidence: RecoveryEvidence<'_>) -> Result<Vec<NewEvent>, String> {
    let mut originals: BTreeMap<String, Event> = BTreeMap::new();
    for event in evidence.snapshot.events.iter().chain(evidence.confirmed) {
        let original =
            towermd_core::snapshot::recovered_original(event)?.unwrap_or_else(|| event.clone());
        originals.insert(original.eid.clone(), original);
    }
    let catalog = Catalog::load(evidence.root)?;
    let mut result = Vec::new();
    let mut proven_publications = std::collections::BTreeSet::new();
    for approval in originals
        .values()
        .filter(|e| !e.stale && e.kind == "baseline.approved")
    {
        let publication = baseline_publication::from_approval(approval)?;
        let reference = format!("refs/tags/towermd/{}", publication.target);
        let Some(readback) =
            towermd_core::main_writer::read_publication(evidence.root, &reference)?
        else {
            continue;
        };
        let commit = readback.commit;
        publication.prove_commit(evidence.root, &commit, approval)?;
        repo::git(
            evidence.root,
            &["merge-base", "--is-ancestor", &commit, &catalog.revision],
        )?;
        proven_publications.insert(commit.clone());
        let eid = derived_eid(&commit, "baseline.created")?;
        if originals.contains_key(&eid) {
            continue;
        }
        result.push(NewEvent {eid,host:approval.host.clone(),actor:"process:towermd-recovery".into(),
            kind:"baseline.created".into(),layer:"L1".into(),obj:publication.target,
            data:json!({"commit":commit,"approval_eid":approval.eid,"content_hash":publication.content_hash}),
            guards:vec![]});
    }
    for approval in originals.values().filter(|e| !e.stale && e.kind == "main.resolution.approved") {
        use towermd_core::main_resolution::{self, Operation};
        let preview = main_resolution::from_approval(approval)?;
        if preview.project_id != catalog.config.project_id() || preview.docs_dir != catalog.config.docs_dir() {
            return Err("恢复中的主干处理确认不属于当前项目".into());
        }
        let eid = derived_eid(&preview.target_commit, "main.change.resolved")?;
        let proof = if let Some(resolved) = originals.get(&eid) {
            main_resolution::prove_resolution(evidence.root, catalog.config.project_id(),
                catalog.config.docs_dir(), resolved, approval)?
        } else {
            let commit = match preview.operation {
                Operation::Claim => preview.target_commit.clone(),
                Operation::Revert => match main_resolution::find_revert(evidence.root, &catalog.revision, &preview, approval)? {
                    Some(commit) => commit,
                    None => continue,
                },
            };
            main_resolution::prove_commit(evidence.root, &preview, approval, &commit)?
        };
        if !main_resolution::is_ancestor(evidence.root, &proof.resolution_commit, &catalog.revision)? {
            continue;
        }
        proven_publications.insert(proof.target_commit.clone());
        proven_publications.insert(proof.resolution_commit.clone());
        if originals.contains_key(&eid) { continue; }
        result.push(NewEvent {
            eid, host: "recovery".into(), actor: "process:towermd-recovery".into(),
            kind: "main.change.resolved".into(), layer: "L1".into(), obj: proof.target_commit.clone(),
            data: serde_json::to_value(proof).map_err(|e|e.to_string())?, guards: vec![],
        });
    }
    let inspected = match towermd_core::path_guard::inspect_main(evidence.root,&catalog.revision,
        catalog.config.project_id(),catalog.config.docs_dir(),None) {
        Ok(value)=>value,
        Err(error)=>{
            let eid=derived_eid(&catalog.revision,"alert.raised.inspection_incomplete")?;
            if !originals.contains_key(&eid) {
                result.push(NewEvent {eid:eid.clone(),host:"recovery".into(),actor:"process:towermd-recovery".into(),
                    kind:"alert.raised".into(),layer:"L5".into(),obj:eid,
                    data:json!({"reason":"inspection_incomplete","detail":{"commit":catalog.revision,
                        "severity":"blocking","error":error}}),guards:vec![]});
            }
            return Ok(result);
        }
    };
    for entry in &inspected.commits {
        // The sentinel protects meaningful trailing spaces from the general
        // text reader's trim_end; the commit body remains byte-for-byte intact.
        let raw_message=repo::git(evidence.root,&["show","-s","--format=%B%x00",&entry.commit])?;
        let message=raw_message.strip_suffix('\0').ok_or("恢复提交说明缺少读取终止符")?;
        let unknown:Vec<_>=message.lines().filter(|line|line.starts_with("Towermd-"))
            .filter(|line|!["Towermd-Epoch:","Towermd-Snapshot:","Towermd-Baseline:","Towermd-Approval:",
                "Towermd-Content:","Towermd-Task:","Towermd-Run:","Towermd-Module:","Towermd-Evidence:","Towermd-Resolution:"]
                .iter().any(|prefix|line.starts_with(prefix))).collect();
        if !unknown.is_empty() {
            let eid=derived_eid(&entry.commit,"alert.raised.unknown_towermd_trailer")?;
            if !originals.contains_key(&eid) {
                result.push(NewEvent {eid:eid.clone(),host:"recovery".into(),actor:"process:towermd-recovery".into(),
                    kind:"alert.raised".into(),layer:"L5".into(),obj:eid,
                    data:json!({"reason":"unknown_towermd_trailer","detail":{"commit":entry.commit,
                        "severity":"warning","trailers":unknown}}),guards:vec![]});
            }
        }
        if !message.lines().any(|line|line.starts_with("Towermd-Snapshot:")) {continue;}
        let eid=derived_eid(&entry.commit,"snapshot.written")?;
        let proof = (|| {
            let snapshot=towermd_core::snapshot::Snapshot::read(evidence.root,&entry.commit,
                catalog.config.docs_dir(),catalog.config.project_id(),|events|
                    towermd_core::project_runtime::replay(&dev_state::initial(),events,dev_state::apply))?;
            towermd_core::snapshot_publication::prove_checked(evidence.root,&entry.commit,
                catalog.config.docs_dir(),catalog.config.project_id(),&snapshot,&|id|originals.get(id).cloned())
        })();
        let proof = match proof {
            Ok(proof) => proof,
            Err(error) if !originals.contains_key(&eid) => {
                // A trailer only locates a candidate. An unproved candidate
                // remains diagnostic data; it cannot prevent Hub reconstruction.
                let alert_id = derived_eid(&entry.commit, "alert.raised.invalid_snapshot_publication")?;
                if !originals.contains_key(&alert_id) {
                    result.push(NewEvent {
                        eid: alert_id.clone(), host: "recovery".into(), actor: "process:towermd-recovery".into(),
                        kind: "alert.raised".into(), layer: "L5".into(), obj: alert_id,
                        data: json!({"reason":"invalid_snapshot_publication",
                            "detail":{"commit":entry.commit,"severity":"warning","error":error}}),
                        guards: vec![],
                    });
                }
                continue;
            }
            Err(error) => return Err(format!("已接受快照原件无法重新证明：{error}")),
        };
        if let Some(written) = originals.get(&eid) {
            proof.check_receipt(catalog.config.project_id(), written)?;
        }
        proven_publications.insert(entry.commit.clone());
        if originals.contains_key(&eid) {continue;}
        result.push(NewEvent {eid,host:"recovery".into(),actor:"process:towermd-recovery".into(),
            kind:"snapshot.written".into(),layer:"L5".into(),obj:catalog.config.project_id().into(),
            data:serde_json::to_value(proof).map_err(|e|e.to_string())?,guards:vec![]});
    }
    let mut protected_history_passed = true;
    for entry in &inspected.commits {
        if entry.violations.is_empty() || proven_publications.contains(&entry.commit) {
            continue;
        }
        protected_history_passed = false;
        let eid = derived_eid(&entry.commit, "alert.raised.unapproved_protected_change")?;
        if !originals.contains_key(&eid) {
            result.push(NewEvent {
                eid: eid.clone(), host: "recovery".into(), actor: "process:towermd-recovery".into(),
                kind: "alert.raised".into(), layer: "L5".into(), obj: eid,
                data: json!({"reason":"unapproved_protected_change",
                    "detail":{"commit":entry.commit,"severity":"blocking",
                        "class":towermd_core::path_guard::change_class(&entry.violations),"violations":entry.violations}}),
                guards: vec![],
            });
        }
    }
    if protected_history_passed {
        result.extend(super::finish::recover_merges(&evidence)?);
    }
    Ok(result)
}
