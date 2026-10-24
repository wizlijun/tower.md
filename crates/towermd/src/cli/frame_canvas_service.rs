//! FR-154: a person approves writing a frame into `tower.canvas`. Only the
//! App reaches these routes; the CLI has no command for them. The App first
//! asks for a preview (the exact new file), then confirms that preview. The
//! daemon recomputes it, records the person's approval, has the single main
//! writer commit it, proves the commit and records the receipt, so
//! `main_inspection` can prove it later from the hub alone.
use super::*;
use super::business::{config, envelope, now, ready, uncertain, open_failure, runtime_failure};
use super::main_resolutions::authoritative_main;
use towermd_core::{
    events, frames, frame_canvas::{self, Materialized, Preview}, hub::Event, hub_protocol::NewEvent,
    main_writer::WriteOutcome, project_runtime::ProjectRuntime,
};

const HUMAN: &str = "human:local";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm { preview: Preview }

fn frame(runtime: &ProjectRuntime, id: &str) -> Result<frames::Frame, ApiError> {
    frames::read(runtime.projection()).map_err(bad)?.remove(id)
        .ok_or_else(|| rejected_as(ErrorCode::NotFound)(format!("没有容器 {id}")))
}

/// Main must carry no unapproved protected change before we add to it.
fn clean(project: &Project, server: &Server, runtime: &ProjectRuntime, main: &str) -> Result<(), ApiError> {
    let config = config(server, project).map_err(bad)?;
    let inspection = towermd_core::main_inspection::inspect(&project.root, main, &config.project_id, &config.docs_dir,
        None, &config.host, |eid| runtime.receipt(eid).cloned(), &towermd_devkit::dev_state::ASSEMBLY);
    if !inspection.complete || !inspection.blocked_commits.is_empty() {
        return Err(bad(inspection.error.unwrap_or_else(|| "main 含未获批准的受保护变更，须先处理禁止中断".into())));
    }
    Ok(())
}

pub async fn prepare(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let main = authoritative_main(&project).map_err(bad)?;
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let config = config(&server, &project).map_err(bad)?;
        clean(&project, &server, runtime, &main)?;
        let frame = frame(runtime, &id)?;
        let preview = frame_canvas::prepare(&project.root, &main, &config.project_id, &config.docs_dir,
            &frame, runtime.contiguous_seq(), &Uuid::now_v7().to_string()).map_err(bad)?;
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
        if event.kind != frame_canvas::APPROVED || event.actor != HUMAN || event.layer != frame_canvas::LAYER
            || event.obj != preview.frame || !event.guards.is_empty()
            || event.data != serde_json::to_value(preview).map_err(|error| error.to_string())?
        { return Err("原确认请求已绑定其他输入".into()); }
    }
    Ok((original, pending))
}

/// The receipt already recorded for this approval, if any.
fn recorded(runtime: &ProjectRuntime, approval: &str) -> Option<Event> {
    runtime.canonical_receipts().find(|event| event.kind == frame_canvas::MATERIALIZED
        && event.data["approval_eid"].as_str() == Some(approval)).cloned()
}

/// A commit the writer already made for this approval before a crash.
fn written(project: &Project, preview: &Preview, main: &str) -> Result<Option<String>, String> {
    let range = format!("{}..{main}", preview.main_revision);
    let commits = repo::git(&project.root, &["rev-list", "--reverse", &range])?;
    Ok(commits.lines().find(|commit| frame_canvas::prove_commit(&project.root, preview, commit).is_ok()).map(str::to_owned))
}

pub async fn confirm(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Confirm>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let preview = input.preview;
        if preview.frame != id { return Err(bad("确认不属于这个容器")); }
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let config = config(&server, &project).map_err(bad)?;
        if preview.project_id != config.project_id || preview.docs_dir != config.docs_dir {
            return Err(bad("确认不属于当前项目"));
        }
        if let Some(event) = recorded(runtime, &preview.request_id) {
            return Ok(Json(response(&preview, &event, None)));
        }
        let (original, pending) = original_or_pending(runtime, &preview).map_err(bad)?;
        let main = authoritative_main(&project).map_err(uncertain)?;
        let earlier = if original.is_some() || pending.is_some() { written(&project, &preview, &main).map_err(uncertain)? } else { None };
        if earlier.is_none() {
            if main != preview.main_revision { return Err(bad("main 已变化，请重新预览后确认")); }
            clean(&project, &server, runtime, &main)?;
            // The App cannot supply bytes of its own: the daemon must arrive
            // at exactly the same preview from the frame as it is now.
            let again = frame_canvas::prepare(&project.root, &main, &config.project_id, &config.docs_dir,
                &frame(runtime, &preview.frame)?, preview.seq, &preview.request_id).map_err(bad)?;
            if again != preview { return Err(bad("容器或画布已变化，请重新预览后确认")); }
        }
        let approval = match original {
            Some(event) => event,
            None => {
                let mut request = pending.unwrap_or(envelope(&server, preview.request_id.clone(), HUMAN,
                    frame_canvas::APPROVED, &preview.frame, serde_json::to_value(&preview).map_err(bad)?));
                request.layer = frame_canvas::LAYER.into();
                runtime.append(request, now).map_err(uncertain)?
            }
        };
        preview.validate_approval(&approval).map_err(bad)?;
        let commit = match earlier {
            Some(commit) => commit,
            None => match runtime.publish(&frame_canvas::write_request(&preview, runtime.main_fence().map_err(runtime_failure)?.epoch), now)
                .map_err(uncertain)? {
                WriteOutcome::Written { commit, .. } => commit,
                _ => return Err(uncertain("容器落盘被其他主干水位替代，请重试原确认")),
            },
        };
        frame_canvas::prove_commit(&project.root, &preview, &commit).map_err(uncertain)?;
        let eid = events::derived_eid(&commit, frame_canvas::MATERIALIZED).map_err(uncertain)?;
        let data = serde_json::to_value(Materialized { approval_eid: approval.eid.clone(), frame: preview.frame.clone(),
            commit: commit.clone(), sha256: preview.sha256.clone() }).map_err(uncertain)?;
        let event = match runtime.pending().map_err(uncertain)?.into_iter().find(|event| event.eid == eid) {
            Some(pending) if pending.kind == frame_canvas::MATERIALIZED && pending.data == data => runtime.append(pending, now).map_err(runtime_failure)?,
            Some(_) => return Err(uncertain("原容器落盘回执正文冲突")),
            None => {
                let mut event = envelope(&server, eid, frame_canvas::WRITER, frame_canvas::MATERIALIZED, &preview.frame, data);
                event.guards = vec![runtime.main_fence().map_err(runtime_failure)?];
                runtime.append(event, now).map_err(runtime_failure)?
            }
        };
        frame_canvas::prove(&project.root, &config.project_id, &config.docs_dir, &commit, &|eid| runtime.receipt(eid).cloned())
            .map_err(uncertain)?;
        // The new commit is proven by the receipt just recorded; re-inspect
        // before publishing so the snapshot carries the proof.
        *project.finish_observed_main.lock().map_err(|_| uncertain("主干观察游标不可用"))? = None;
        super::finish::observe_authority(&server, &project, runtime).map_err(uncertain)?;
        let snapshot = runtime.snapshot(now).map_err(uncertain)?;
        super::finish::observe_authority(&server, &project, runtime).map_err(uncertain)?;
        Ok(Json(response(&preview, &event, Some(&snapshot))))
    }).await.map_err(uncertain)?
}

fn response(preview: &Preview, event: &Event, snapshot: Option<&str>) -> Value {
    json!({"request_id":preview.request_id,"event":event.eid,"seq":event.seq,"status":"accepted",
        "frame":preview.frame,"commit":event.data["commit"],"snapshot_commit":snapshot})
}
