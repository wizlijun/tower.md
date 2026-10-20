//! App-only resolution of unauthorized main commits. No CLI approval route.
use super::*;
use super::business::{config, envelope, now, ready, uncertain};
use towermd_core::{events, hub::Event, hub_protocol::NewEvent, main_resolution::{self, Operation, Preview, Resolution},
    main_writer::WriteOutcome, path_guard, project_runtime::ProjectRuntime};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepare {
    operation: Operation,
    commit: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm { preview: Preview }

fn authoritative_main(project: &Project) -> Result<String, String> {
    let remotes = repo::git(&project.root, &["remote"])?;
    let reference = if remotes.lines().any(|remote| remote == "origin") {
        repo::git(&project.root, &["fetch", "--no-tags", "origin", "+refs/heads/main:refs/remotes/origin/main"])?;
        "refs/remotes/origin/main"
    } else { "refs/heads/main" };
    repo::git(&project.root, &["rev-parse", "--verify", reference])
}

pub async fn prepare(
    State(server): State<Arc<Server>>, headers: HeaderMap,
    RoutePath(handle): RoutePath<String>, Json(input): Json<Prepare>,
) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let main = authoritative_main(&project).map_err(bad)?;
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let config = config(&server, &project).map_err(bad)?;
        let eid = events::derived_eid(&input.commit, "main.change.resolved").map_err(bad)?;
        if runtime.receipt(&eid).is_some() { return Err(bad("该提交已有处理原件，请刷新项目态势")); }
        let inspection = towermd_core::main_inspection::inspect(&project.root, &main, &config.project_id,
            &config.docs_dir, None, &config.host, |eid| runtime.receipt(eid).cloned());
        if !inspection.complete { return Err(bad(inspection.error.unwrap_or("主干检查不完整".into()))); }
        if !inspection.blocked_commits.iter().any(|entry| entry.commit == input.commit) {
            return Err(bad("目标不是当前 main 的待处理受保护提交"));
        }
        let preview = main_resolution::prepare(&project.root, &main, &config.project_id, &config.docs_dir,
            &input.commit, input.operation, &Uuid::now_v7().to_string()).map_err(bad)?;
        Ok(Json(serde_json::to_value(preview).map_err(bad)?))
    }).await.map_err(uncertain)?
}

fn original_or_pending(runtime: &ProjectRuntime, preview: &Preview) -> Result<(Option<Event>, Option<NewEvent>), String> {
    let original = runtime.receipt(&preview.request_id).cloned();
    if let Some(event) = original.as_ref() {
        preview.validate_approval(event)?;
    }
    let pending = runtime.pending().map_err(|error| error.to_string())?.into_iter()
        .find(|event| event.eid == preview.request_id);
    if let Some(event) = pending.as_ref() {
        if event.kind != "main.resolution.approved" || event.actor != "human:local"
            || event.layer != "L1" || event.obj != preview.target_commit || !event.guards.is_empty()
            || event.data != serde_json::to_value(preview).map_err(|error| error.to_string())?
        { return Err("原确认请求已绑定其他输入".into()); }
    }
    Ok((original, pending))
}

pub async fn confirm(
    State(server): State<Arc<Server>>, headers: HeaderMap,
    RoutePath(handle): RoutePath<String>, Json(input): Json<Confirm>,
) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let preview = input.preview;
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let config = config(&server, &project).map_err(bad)?;
        if preview.project_id != config.project_id || preview.docs_dir != config.docs_dir {
            return Err(bad("确认不属于当前项目"));
        }
        let (original, pending) = original_or_pending(runtime, &preview).map_err(bad)?;
        main_resolution::verify_preview(&project.root, &preview).map_err(bad)?;
        let main = authoritative_main(&project).map_err(uncertain)?;
        let resolution_eid = events::derived_eid(&preview.target_commit, "main.change.resolved").map_err(bad)?;
        let existing = runtime.receipt(&resolution_eid).cloned();
        if let Some(existing) = &existing {
            if existing.data["approval_eid"] != preview.request_id {
                return Err(bad("该提交已有另一项人工处理，不能覆盖"));
            }
        }
        if original.is_none() && pending.is_none() {
            if main != preview.main_revision { return Err(bad("main 已变化，请重新读取实际 diff 后确认")); }
            // A forged preview cannot manufacture a new admission path for a
            // previously approved snapshot/baseline publication.
            let inspection = towermd_core::main_inspection::inspect(&project.root, &main,
                &config.project_id, &config.docs_dir, None, &config.host, |eid| runtime.receipt(eid).cloned());
            if !inspection.complete || !inspection.blocked_commits.iter().any(|entry| entry.commit == preview.target_commit) {
                return Err(bad("目标不是可处理的 main 禁止中断"));
            }
        }
        path_guard::inspect_main(&project.root, &main, &config.project_id, &config.docs_dir,
            Some(&preview.main_revision)).map_err(uncertain)?;
        let approval = match original {
            Some(event) => event,
            None => runtime.append(pending.unwrap_or(envelope(&server, preview.request_id.clone(), "human:local",
                "main.resolution.approved", &preview.target_commit, serde_json::to_value(&preview).map_err(bad)?)), now)
                .map_err(uncertain)?,
        };
        let (resolution, event) = if let Some(event) = existing {
            let proof = main_resolution::prove_resolution(&project.root, &config.project_id, &config.docs_dir, &event, &approval)
                .map_err(uncertain)?;
            if !main_resolution::is_ancestor(&project.root, &proof.resolution_commit, &main).map_err(uncertain)? {
                return Err(uncertain("已处理提交不再属于当前 main"));
            }
            (proof, event)
        } else {
            let commit = match preview.operation {
                Operation::Claim => preview.target_commit.clone(),
                Operation::Revert => match main_resolution::find_revert(&project.root, &main, &preview, &approval).map_err(uncertain)? {
                    Some(commit) => commit,
                    None => {
                        let request = main_resolution::revert_request(&project.root, &preview, &approval, runtime.main_fence().epoch)
                            .map_err(uncertain)?;
                        match runtime.publish(&request, now).map_err(uncertain)? {
                            WriteOutcome::Written { commit, .. } => commit,
                            _ => return Err(uncertain("撤销写入被其他主干水位替代，请重试原确认")),
                        }
                    }
                },
            };
            let proof = main_resolution::prove_commit(&project.root, &preview, &approval, &commit).map_err(uncertain)?;
            let data = serde_json::to_value(&proof).map_err(uncertain)?;
            let pending_resolution = runtime.pending().map_err(uncertain)?.into_iter().find(|event| event.eid == resolution_eid);
            let event = if let Some(pending) = pending_resolution {
                if pending.kind != "main.change.resolved" || pending.actor != "process:towermd"
                    || pending.layer != "L1" || pending.obj != preview.target_commit || pending.data != data {
                    return Err(uncertain("原主干处理回执正文冲突"));
                }
                runtime.append(pending, now).map_err(uncertain)?
            } else {
                let mut event = envelope(&server, resolution_eid, "process:towermd", "main.change.resolved", &preview.target_commit, data);
                event.guards = vec![runtime.main_fence()];
                runtime.append(event, now).map_err(uncertain)?
            };
            main_resolution::prove_resolution(&project.root, &config.project_id, &config.docs_dir, &event, &approval)
                .map_err(uncertain)?;
            (proof, event)
        };
        // A claim changes the proof set without changing main's SHA. Invalidate
        // the derived cursor and force inspection before persisting the result.
        *project.finish_observed_main.lock().map_err(|_| uncertain("主干观察游标不可用"))? = None;
        super::finish::observe_authority(&server, &project, runtime).map_err(uncertain)?;
        let snapshot_commit = runtime.snapshot(now).map_err(uncertain)?;
        // A public snapshot can itself advance main. Its canonical receipt is
        // now available, so the immediately returned projection can prove it.
        super::finish::observe_authority(&server, &project, runtime).map_err(uncertain)?;
        Ok(Json(response(&preview, &resolution, &event, &snapshot_commit)))
    }).await.map_err(uncertain)?
}

fn response(preview: &Preview, resolution: &Resolution, event: &Event, snapshot: &str) -> Value {
    json!({"request_id":preview.request_id,"event":event.eid,"seq":event.seq,
        "operation":resolution.operation,"target_commit":resolution.target_commit,
        "resolution_commit":resolution.resolution_commit,"snapshot_commit":snapshot})
}
