//! CLI task admission and run binding. Human approval is deliberately absent.
use super::business::{now, ready, uncertain, open_failure, runtime_failure};
use super::*;
use towermd_core::{hub::Event, hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}};
use towermd_devkit::{baseline_publication, dev_state, task_model::{ self, HarnessSource, ReleaseReason, Run, RunBinding, RunState, TaskDraft, TaskEvent, TaskOrigin, TaskProjection, VerifiedBaseline, }};

pub(super) const MODULE_TTL: u64 = 15 * 60 * 1000;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Add {
    request_id: String,
    task: TaskDraft,
    cwd: PathBuf,
    #[serde(default)]
    run: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    request_id: String,
    cwd: PathBuf,
    #[serde(default)]
    self_reported: Option<HarnessSource>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    cwd: PathBuf,
    #[serde(default)]
    run: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    request_id: String,
    cwd: PathBuf,
    #[serde(default)]
    run: Option<String>,
}
#[derive(Deserialize)]
pub struct ShowQuery {
    #[serde(default)]
    id: Option<String>,
    cwd: PathBuf,
    #[serde(default)]
    run: Option<String>,
}

pub(super) fn require_cli(headers: &HeaderMap, server: &Server) -> Result<(), ApiError> {
    if !authorized_client(headers, server)
        || headers
            .get("x-towermd-client")
            .and_then(|v| v.to_str().ok())
            != Some("cli")
    {
        return Err(denied());
    }
    Ok(())
}
pub(super) fn coordinates(project: &Project, cwd: &Path) -> Result<(String, String), String> {
    let root = repo::toplevel(cwd)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let common = root
        .join(repo::git(&root, &["rev-parse", "--git-common-dir"])?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if common != project.common {
        return Err("调用目录不属于该项目的工作树".into());
    }
    let branch = repo::git(&root, &["symbolic-ref", "--short", "HEAD"])?;
    Ok((root.to_str().ok_or("工作树路径不是 UTF-8")?.into(), branch))
}
pub(super) fn baseline(project: &Project, runtime: &ProjectRuntime) -> Result<VerifiedBaseline, String> {
    let records = runtime.projection()["Baseline"]
        .as_object()
        .ok_or("基线投影无效")?;
    let (_, record) = records
        .iter()
        .filter_map(|(id, value)| {
            id.strip_prefix('B')?
                .parse::<u64>()
                .ok()
                .map(|number| (number, value))
        })
        .max_by_key(|(number, _)| *number)
        .ok_or("G0：尚无经 App 批准且实际出版的基线")?;
    let commit = record["commit"].as_str().ok_or("基线缺少提交证明")?;
    let approval = runtime
        .receipt(record["approval_eid"].as_str().ok_or("基线缺少批准原件")?)
        .ok_or("无法读取已接受的人工批准原件")?;
    let tag = record["git_tag"].as_str().ok_or("基线缺少标签引用")?;
    let readback =
        towermd_core::main_writer::read_publication(&project.root, &format!("refs/tags/{tag}"))?
            .ok_or("已记录基线在权威远端缺少实际出版标签")?;
    if readback.commit != commit {
        return Err("权威基线标签与已接受出版回执不同".into());
    }
    repo::git(
        &project.root,
        &[
            "merge-base",
            "--is-ancestor",
            commit,
            &readback.main_revision,
        ],
    )
    .map_err(|_| "批准基线不在当前权威 main 上")?;
    baseline_publication::verified_baseline(&project.root, commit, approval)
}
pub(super) fn locate(
    projection: &TaskProjection,
    host: &str,
    worktree: &str,
    branch: &str,
    explicit: Option<&str>,
    required: bool,
) -> Result<Option<Run>, String> {
    if let Some(id) = explicit {
        let run = projection.runs.get(id).ok_or("指定运行不存在")?;
        if run.binding.host != host || run.state != RunState::Active {
            return Err("指定运行不属于本机或已结束".into());
        }
        return Ok(Some(run.clone()));
    }
    let matches: Vec<_> = projection
        .runs
        .values()
        .filter(|run| {
            run.state == RunState::Active
                && run.binding.host == host
                && run.binding.worktree == worktree
                && run.binding.branch == branch
        })
        .collect();
    if matches.len() > 1 {
        return Err("工作树存在多个活动运行，投影不一致".into());
    }
    if let Some(run) = matches.first() {
        return Ok(Some((*run).clone()));
    }
    if required {
        let active: Vec<_> = projection
            .runs
            .values()
            .filter(|run| run.state == RunState::Active && run.binding.host == host)
            .map(|run| {
                format!(
                    "{} ({} · {})",
                    run.binding.id, run.task, run.binding.worktree
                )
            })
            .collect();
        return Err(format!(
            "当前工作树和分支没有活动运行；请先 task claim，或显式 --run。 本机活动运行：{}",
            if active.is_empty() {
                "无".into()
            } else {
                active.join("；")
            }
        ));
    }
    Ok(None)
}
pub(super) fn renew(
    runtime: &mut ProjectRuntime,
    server: &Server,
    project: &Project,
    cwd: &Path,
    explicit: Option<&str>,
    required: bool,
) -> Result<Option<Run>, String> {
    let (worktree, branch) = coordinates(project, cwd)?;
    runtime.tick(now).map_err(|e| e.to_string())?;
    let projection = dev_state::tasks(runtime.projection())?;
    let run = locate(
        &projection,
        &server.endpoint.host,
        &worktree,
        &branch,
        explicit,
        required,
    )?;
    if let Some(run) = &run {
        runtime
            .renew_work_leases(run.fences.clone(), MODULE_TTL, now)
            .map_err(|e| e.to_string())?;
    }
    Ok(run)
}
fn envelope(server: &Server, request_id: String, action: &TaskEvent) -> Result<NewEvent, String> {
    let (kind, obj, guards) = match action {
        TaskEvent::Created { task } => ("task.created", task.input.id.clone(), Vec::new()),
        TaskEvent::Claimed { run } => ("task.claimed", run.task.clone(), run.fences.clone()),
        _ => return Err("释放须显式绑定原运行 fences".into()),
    };
    Ok(NewEvent {
        eid: request_id,
        host: server.endpoint.host.clone(),
        actor: "process:towermd-cli".into(),
        kind: kind.into(),
        layer: "L4".into(),
        obj,
        data: serde_json::to_value(action).map_err(|e| e.to_string())?,
        guards,
    })
}
fn known(runtime: &ProjectRuntime, eid: &str) -> Result<Option<(Value, Option<NewEvent>)>, String> {
    if let Some(event) = runtime.receipt(eid) {
        if event.stale || event.actor != "process:towermd-cli" {
            return Err("原请求已经过期或来源不匹配；不能重新执行".into());
        }
        return Ok(Some((event.data.clone(), None)));
    }
    Ok(runtime
        .pending()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|event| event.eid == eid)
        .map(|event| (event.data.clone(), Some(event))))
}
fn redeliver(
    runtime: &mut ProjectRuntime,
    eid: &str,
    pending: Option<NewEvent>,
) -> Result<Event, ApiError> {
    let event = match pending {
        Some(input) => runtime.append(input, now).map_err(runtime_failure)?,
        None => runtime
            .receipt(eid)
            .cloned()
            .ok_or_else(|| bad("原事件缺失"))?,
    };
    if event.stale {
        return Err(rejected_as(ErrorCode::StaleEpoch)("原请求的租约已过期，事件未生效"));
    }
    Ok(event)
}

pub async fn add(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
    Json(input): Json<Add>,
) -> Result<Json<Value>, ApiError> {
    require_cli(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        renew(runtime, &server, &project, &input.cwd, input.run.as_deref(), false).map_err(bad)?;
        let event = if let Some((data, pending)) = known(runtime, &input.request_id).map_err(bad)? {
            let action: TaskEvent = serde_json::from_value(data).map_err(bad)?;
            if !matches!(&action, TaskEvent::Created { task } if task.input == input.task && task.origin == TaskOrigin::Cli) { return Err(rejected_as(ErrorCode::Conflict)("同一请求 ID 的任务内容不同")); }
            redeliver(runtime, &input.request_id, pending)?
        } else {
            let approved = baseline(&project, runtime).map_err(rejected_as(ErrorCode::GateFailed))?;
            let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
            let action = task_model::prepare_add(&approved, &projection, input.task, TaskOrigin::Cli).map_err(rejected_as(ErrorCode::GateFailed))?;
            runtime.append(envelope(&server, input.request_id, &action).map_err(bad)?, now).map_err(runtime_failure)?
        };
        Ok(Json(json!({"event":event,"task":runtime.projection()["Task"][&event.obj]})))
    }).await.map_err(uncertain)?
}

pub async fn claim(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
    Json(input): Json<Claim>,
) -> Result<Json<Value>, ApiError> {
    require_cli(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let (worktree, branch) = coordinates(&project, &input.cwd).map_err(bad)?;
        let binding = RunBinding { id: format!("R-{}", input.request_id), host: server.endpoint.host.clone(), worktree, branch, self_reported: input.self_reported };
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        runtime.tick(now).map_err(runtime_failure)?;
        let event = if let Some((data, pending)) = known(runtime, &input.request_id).map_err(bad)? {
            let action: TaskEvent = serde_json::from_value(data).map_err(bad)?;
            if !matches!(&action, TaskEvent::Claimed { run } if run.task == id && run.binding == binding) { return Err(rejected_as(ErrorCode::Conflict)("同一领取请求的任务或运行绑定不同")); }
            redeliver(runtime, &input.request_id, pending)?
        } else {
            runtime.tick(now).map_err(runtime_failure)?;
            let approved = baseline(&project, runtime).map_err(rejected_as(ErrorCode::GateFailed))?;
            let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
            if towermd_core::question_projection::blocks_task(runtime.projection(),&id).map_err(bad)? {
                return Err(rejected_as(ErrorCode::GuardBlocked)("任务仍有未解决提问；释放再领取不能绕过问答门禁"));
            }
            task_model::check_g0(&approved, &projection, &id).map_err(rejected_as(ErrorCode::GateFailed))?;
            if binding.branch != format!("t/{id}") { return Err(rejected_as(ErrorCode::GuardBlocked)("先创建并进入 t/<task-id> 分支的工作树，再领取任务")); }
            let task = &projection.tasks[&id];
            let names = std::iter::once(&task.input.module).chain(&task.input.touches).map(|module| format!("module:{module}")).collect::<Vec<_>>();
            let leases = runtime.acquire_work_leases(&binding.id, names, MODULE_TTL, now).map_err(|error| match error {
                RuntimeError::Fenced(_) => rejected_as(ErrorCode::LeaseBusy)(error),
                // The hub refuses new work leases while the project is paused: a state conflict, not a busy lease.
                RuntimeError::Invalid(_) => rejected_as(ErrorCode::Conflict)(error),
                _ => bad(error),
            })?;
            let fences = leases.iter().map(|lease| lease.fence()).collect::<Vec<_>>();
            let action = match task_model::prepare_claim(&approved, &projection, &id, binding, &leases, now()) {
                Ok(action) => action,
                Err(error) => { let _ = runtime.release_work_leases(fences.clone(), now); return Err(rejected_as(ErrorCode::GateFailed)(error)); }
            };
            match runtime.append(envelope(&server, input.request_id, &action).map_err(bad)?, now) {
                Ok(event) if !event.stale => event,
                Ok(_) => return Err(rejected_as(ErrorCode::StaleEpoch)("领取请求的租约已失效，任务未开始")),
                Err(error) => {
                    if matches!(error, RuntimeError::Invalid(_)) { let _ = runtime.release_work_leases(fences.clone(), now); }
                    return Err(uncertain(error));
                }
            }
        };
        let TaskEvent::Claimed { run } = serde_json::from_value(event.data.clone()).map_err(bad)? else { return Err(bad("领取回执无效")); };
        if runtime.projection()["Run"][&run.binding.id]["state"] == "active" {
            runtime.renew_work_leases(run.fences.clone(), MODULE_TTL, now).map_err(uncertain)?;
        }
        Ok(Json(json!({"event":event,"run":runtime.projection()["Run"][&run.binding.id],"task":runtime.projection()["Task"][&id]})))
    }).await.map_err(uncertain)?
}

pub async fn release(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
    Json(input): Json<Release>,
) -> Result<Json<Value>, ApiError> {
    require_cli(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let event = if let Some((data, pending)) = known(runtime, &input.request_id).map_err(bad)? {
            let action: TaskEvent = serde_json::from_value(data).map_err(bad)?;
            let TaskEvent::Released {
                run_id,
                reason: ReleaseReason::Voluntary,
            } = action
            else {
                return Err(bad("请求 ID 不属于自愿释放"));
            };
            if input.run.as_ref().is_some_and(|id| id != &run_id) {
                return Err(bad("同一释放请求指定了另一运行"));
            }
            let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
            let run = projection
                .runs
                .get(&run_id)
                .ok_or_else(|| bad("原释放运行缺失"))?;
            let (worktree, branch) = coordinates(&project, &input.cwd).map_err(bad)?;
            if run.binding.host != server.endpoint.host
                || (input.run.is_none()
                    && (run.binding.worktree != worktree || run.binding.branch != branch))
            {
                return Err(bad("原释放请求来自另一工作树"));
            }
            redeliver(runtime, &input.request_id, pending)?
        } else {
            let run = renew(
                runtime,
                &server,
                &project,
                &input.cwd,
                input.run.as_deref(),
                true,
            )
            .map_err(bad)?
            .ok_or_else(|| bad("没有活动运行"))?;
            let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
            let action =
                task_model::prepare_release(&projection, &run.binding.id, ReleaseReason::Voluntary)
                    .map_err(bad)?;
            let envelope = NewEvent {
                eid: input.request_id,
                host: server.endpoint.host.clone(),
                actor: "process:towermd-cli".into(),
                kind: "task.released".into(),
                layer: "L4".into(),
                obj: run.task,
                data: serde_json::to_value(action).map_err(bad)?,
                guards: run.fences,
            };
            let event = runtime.append(envelope, now).map_err(runtime_failure)?;
            if event.stale {
                return Err(rejected_as(ErrorCode::StaleEpoch)("释放事件的原租约已失效"));
            }
            event
        };
        match runtime.release_work_leases(event.guards.clone(), now) {
            Ok(_) | Err(RuntimeError::Fenced(_)) => {}
            Err(error) => return Err(uncertain(error)),
        }
        Ok(Json(
            json!({"event":event,"task":runtime.projection()["Task"][&event.obj]}),
        ))
    })
    .await
    .map_err(uncertain)?
}

pub async fn show(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
    Query(input): Query<ShowQuery>,
) -> Result<Json<Value>, ApiError> {
    require_cli(&headers, &server)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let run = renew(
            runtime,
            &server,
            &project,
            &input.cwd,
            input.run.as_deref(),
            input.id.is_none(),
        )
        .map_err(bad)?;
        let id = input
            .id
            .or_else(|| run.as_ref().map(|run| run.task.clone()))
            .ok_or_else(|| rejected_as(ErrorCode::Usage)("未指定任务"))?;
        let task = runtime.projection()["Task"]
            .get(&id)
            .ok_or_else(|| rejected_as(ErrorCode::NotFound)("任务不存在"))?;
        let delegation = super::delegations::task_details(runtime, &project, &id).map_err(bad)?;
        let module = task["input"]["module"].as_str().unwrap_or_default();
        let temp_rules = super::temp_rules::for_task(runtime.projection(), &id, module).map_err(bad)?;
        Ok(Json(json!({"task":task,"current_run":run,"delegation":delegation,"temp_rules":temp_rules})))
    })
    .await
    .map_err(uncertain)?
}

pub async fn status(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
    Json(input): Json<Context>,
) -> Result<Json<Value>, ApiError> {
    require_cli(&headers, &server)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project).map_err(bad)?;
        let (_, branch) = coordinates(&project, &input.cwd).map_err(bad)?;
        let mut result = json!({"project_id":catalog.config.project_id(),"branch":branch,"revision":catalog.revision,"docs_dir":catalog.config.docs_dir(),"objects":catalog.objects.len(),"diagnostics":catalog.diagnostics,"current_run":null});
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        if slot.is_none() && !project.common.join("towermd/runtime.json").exists() && towermd_core::main_writer::latest_snapshot(&project.root).map_err(bad)?.is_none() {
            if input.run.is_some() { return Err(rejected_as(ErrorCode::NotFound)("当前项目尚无活动运行")); }
            result["runtime_status"] = json!("not_activated"); return Ok(Json(result));
        }
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let run = renew(runtime, &server, &project, &input.cwd, input.run.as_deref(), input.run.is_some()).map_err(bad)?;
        result["runtime_status"] = json!("ready"); result["current_run"] = json!(run); result["projection"] = runtime.projection().clone();
        Ok(Json(result))
    }).await.map_err(uncertain)?
}
