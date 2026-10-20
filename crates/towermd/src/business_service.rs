//! App-only publication/recovery and the daemon-owned project runtime.
use super::*;
use towermd_core::{
    baseline_publication,
    events::derived_eid,
    hub::Event,
    hub_protocol::NewEvent,
    main_writer::{WriteKind, WriteOutcome, WriteRequest},
    project_runtime::{ProjectRuntime, RecoveryEvidence, RuntimeConfig},
    project_state,
};

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
pub(super) fn ready<'a>(
    slot: &'a mut Option<ProjectRuntime>,
    server: &Server,
    project: &Project,
) -> Result<&'a mut ProjectRuntime, String> {
    if slot.is_none() {
        *slot = Some(
            ProjectRuntime::open(
                config(server, project)?,
                project_state::initial(),
                project_state::apply,
                now,
            )
            .map_err(|e| e.to_string())?,
        );
    }
    Ok(slot.as_mut().unwrap())
}
pub(super) fn uncertain(error: impl ToString) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":error.to_string()})),
    )
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
                continue;
            }
            if runtime.snapshot_due(now()) {
                if let Err(error) = runtime.snapshot(now) {
                    eprintln!("项目定时快照待重试：{error}");
                }
            }
        }
    }
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
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        if slot.is_none() {
            // A never-activated project's read-only status must not create a Hub.
            let marker = project.common.join("towermd/runtime.json");
            let latest = towermd_core::main_writer::latest_snapshot(&project.root).map_err(bad)?;
            if !marker.exists() && latest.is_none() {
                return Ok(Json(json!({"status":"not_activated"})));
            }
            match ProjectRuntime::open(
                config(&server, &project).map_err(bad)?,
                project_state::initial(),
                project_state::apply,
                now,
            ) {
                Ok(runtime) => *slot = Some(runtime),
                Err(error) => {
                    return Ok(Json(
                        json!({"status":"needs_recovery","error":error.to_string()}),
                    ))
                }
            }
        }
        let runtime = slot.as_mut().unwrap();
        runtime.tick(now).map_err(uncertain)?;
        Ok(Json(
            json!({"status":"ready","projection":runtime.projection(),
            "seq":runtime.contiguous_seq(),"snapshot_commit":runtime.snapshot_commit()}),
        ))
    })
    .await
    .map_err(uncertain)?
}

pub async fn prepare(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let catalog = current_catalog(&project).map_err(bad)?;
        let draft = towermd_core::draft_store::load(&catalog, &id).map_err(bad)?;
        if draft.scan.revision != catalog.revision {
            return Err(bad("来源 main 已改变，请重新扫描当前提交"));
        }
        let number = runtime.projection()["Baseline"]
            .as_object()
            .map_or(0, |m| m.len());
        let publication = baseline_publication::prepare(
            &draft,
            &project.common,
            &Uuid::now_v7().to_string(),
            &format!("B{number}"),
            &baseline_publication::preview_time(),
        )
        .map_err(bad)?;
        Ok(Json(serde_json::to_value(publication).map_err(bad)?))
    })
    .await
    .map_err(uncertain)?
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    request_id: String,
    target: String,
    prepared_at: String,
    content_hash: String,
}

pub async fn approve(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
    Json(input): Json<Approval>,
) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move|| {
        let mut slot=project.runtime.lock().map_err(|_|bad("项目会话不可用"))?;
        let runtime=ready(&mut slot,&server,&project).map_err(bad)?;
        let old=runtime.receipt(&input.request_id).cloned();
        let publication=if let Some(event)=old.as_ref() {
            baseline_publication::from_approval(event).map_err(bad)?
        } else {
            let catalog=current_catalog(&project).map_err(bad)?;
            let draft=towermd_core::draft_store::load(&catalog,&id).map_err(bad)?;
            if draft.scan.revision != catalog.revision {return Err(bad("批准来源已改变，请重新审阅"));}
            let expected=format!("B{}",runtime.projection()["Baseline"].as_object().map_or(0,|m|m.len()));
            if input.target != expected {return Err(bad("目标基线编号已改变，请重新审阅"));}
            baseline_publication::prepare(&draft,&project.common,&input.request_id,
                &input.target,&input.prepared_at).map_err(bad)?
        };
        if publication.draft_id != id || publication.target != input.target
            || publication.prepared_at != input.prepared_at || publication.content_hash != input.content_hash
        {return Err(bad("批准请求与原预览不一致"));}
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
                epoch:runtime.main_fence().epoch,
                kind:WriteKind::Slow {expected_base:publication.source_revision.clone(),tag:Some(tag)},
                changes:publication.files.iter().map(|(p,b)|(p.clone(),Some(b.clone()))).collect(),
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
            runtime.append(envelope(&server,created_eid,"process:towermd","baseline.created",
                &publication.target,json!({"commit":commit,"approval_eid":approval.eid,
                    "content_hash":publication.content_hash})),now).map_err(uncertain)?;
        }
        let snapshot_commit=runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"target":publication.target,"commit":commit,
            "tag":format!("towermd/{}",publication.target),
            "content_hash":publication.content_hash,"snapshot_commit":snapshot_commit})))
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
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
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
pub async fn request_rebuild(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    let id = Uuid::now_v7().to_string();
    let directory_path = project.common.join("towermd");
    directory(&directory_path).map_err(bad)?;
    new_file(
        &rebuild_path(&project, &id).map_err(bad)?,
        &serde_json::to_vec(&json!({"request_id":id,"project_id":project.project_id}))
            .map_err(bad)?,
    )
    .map_err(uncertain)?;
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
        let runtime = ProjectRuntime::recover_confirmed(
            config(&server, &project).map_err(bad)?,
            project_state::initial(),
            project_state::apply,
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
        let eid = derived_eid(&commit, "baseline.created")?;
        if originals.contains_key(&eid) {
            continue;
        }
        result.push(NewEvent {eid,host:approval.host.clone(),actor:"process:towermd-recovery".into(),
            kind:"baseline.created".into(),layer:"L1".into(),obj:publication.target,
            data:json!({"commit":commit,"approval_eid":approval.eid,"content_hash":publication.content_hash}),
            guards:vec![]});
    }
    Ok(result)
}
