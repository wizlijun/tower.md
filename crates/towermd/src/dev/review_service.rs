//! G1/G4 reviews and held tasks (FR-58, spec 10.2, 12.1). A delegate asks
//! for a review from the CLI and waits on it; only a person resolves a review
//! or a held task, from the App. Nothing here passes a review on a timer or
//! completes a task: a decision only lets the delegate go on, ends the run,
//! or sends the card back.
use super::business::{now, ready, uncertain, open_failure, runtime_failure};
use super::*;
use towermd_core::{hub::Event, hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError}, question_model, reviews::{self, Requested, Resolved}};
use towermd_devkit::{dev_state, task_interrupts, task_model::{InterruptResolved, RunState}, task_reviews};

const HUMAN: &str = "human:local";
const CLI: &str = "process:towermd-cli";
/// Review IDs are `V-<request ID>`, as questions are `H-…`.
pub(crate) const PREFIX: &str = "V-";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    request_id: String, cwd: PathBuf, #[serde(default)] run: Option<String>,
    stage: String, summary: String, #[serde(default)] artifacts: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wait { cwd: PathBuf, #[serde(default)] run: Option<String>, review: String, #[serde(default)] wait_ms: Option<u64> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision { request_id: String, #[serde(flatten)] data: InterruptResolved }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict { request_id: String, #[serde(flatten)] data: Resolved }

fn append(runtime: &mut ProjectRuntime, request: NewEvent) -> Result<Event, ApiError> {
    runtime.append(request, now).map_err(|error| match error { RuntimeError::Invalid(message) => bad(message), other => runtime_failure(other) })
}
fn same_or_new(runtime: &ProjectRuntime, request: NewEvent) -> Result<NewEvent, ApiError> {
    match human_tasks::original(runtime, &request.eid).map_err(bad)? {
        Some(old) if old == request => Ok(old),
        Some(_) => Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容或来源")),
        None => Ok(request),
    }
}
fn accepted(event: &Event) -> Json<Value> {
    Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted","obj":event.obj}))
}

/// `towermd ask --review plan|code`: G1 binds the plan review to this
/// attempt, G4 the code review to the tree the latest passed finish tested.
pub async fn ask(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Ask>) -> Result<Json<Value>, ApiError> {
    tasks::require_cli(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    if !task_reviews::STAGES.contains(&input.stage.as_str()) {
        return Err(rejected_as(ErrorCode::Usage)(format!("评审阶段须是 {}", task_reviews::STAGES.join(" 或 "))));
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let id = format!("{PREFIX}{}", input.request_id);
        let request = if let Some(old) = human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            let asked: Option<Requested> = serde_json::from_value(old.data.clone()).ok();
            if old.kind != reviews::REQUESTED || old.actor != CLI || old.host != server.endpoint.host || old.obj != id
                || asked.is_none_or(|asked| (asked.stage, asked.summary, asked.artifacts) != (input.stage, input.summary, input.artifacts))
            { return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他评审或操作")); }
            old
        } else {
            let run = tasks::renew(runtime, &server, &project, &input.cwd, input.run.as_deref(), true).map_err(bad)?.ok_or_else(|| bad("缺少活动运行"))?;
            let version = if input.stage == task_reviews::CODE {
                Some(finish::tested_tree(runtime, &run.binding.id)
                    .ok_or_else(|| rejected_as(ErrorCode::GateFailed)("代码评审绑定已验证的 tree：先 towermd dev finish 通过真实验证，再请求评审"))?)
            } else { None };
            let data = Requested { object: run.task.clone(), stage: input.stage, summary: input.summary, artifacts: input.artifacts,
                version, attempt: Some(run.binding.id.clone()) };
            data.check().map_err(rejected_as(ErrorCode::Usage))?;
            NewEvent { eid: input.request_id, host: server.endpoint.host.clone(), actor: CLI.into(), kind: reviews::REQUESTED.into(),
                layer: reviews::LAYER.into(), obj: id, data: serde_json::to_value(data).map_err(bad)?, guards: run.fences }
        };
        let event = append(runtime, request)?;
        if event.stale { return Err(rejected_as(ErrorCode::StaleEpoch)("评审请求的运行租约已失效，未请求评审")); }
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"review":event.obj,"status":"pending"})))
    }).await.map_err(uncertain)?
}

fn outcome(runtime: &ProjectRuntime, id: &str) -> Result<Value, ApiError> {
    let review = reviews::read(runtime.projection()).map_err(uncertain)?.remove(id).ok_or_else(|| rejected_as(ErrorCode::NotFound)("评审不存在"))?;
    Ok(match review.resolution {
        Some(resolution) => json!({"status":"resolved","review":id,"stage":review.request.stage,"verdict":resolution.verdict,"comment":resolution.comment}),
        None => json!({"status":"pending","review":id,"stage":review.request.stage}),
    })
}

/// `towermd wait V-…`: polls the projection in short critical sections, at
/// most one ask-wait per call; an open review is pending, never passed.
pub async fn wait(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Wait>) -> Result<Json<Value>, ApiError> {
    tasks::require_cli(&headers, &server)?;
    let project = project(&server, &handle).map_err(bad)?;
    let budget = input.wait_ms.unwrap_or(question_model::ASK_WAIT_MS).min(question_model::ASK_WAIT_MS);
    let (first_server, first_project, review) = (server.clone(), project.clone(), input.review.clone());
    tokio::task::spawn_blocking(move || {
        let mut slot = first_project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &first_server, &first_project).map_err(open_failure)?;
        let run = tasks::renew(runtime, &first_server, &first_project, &input.cwd, input.run.as_deref(), true).map_err(bad)?.ok_or_else(|| bad("缺少活动运行"))?;
        let found = reviews::read(runtime.projection()).map_err(bad)?;
        let asked = found.get(&input.review).ok_or_else(|| rejected_as(ErrorCode::NotFound)("评审不存在"))?;
        if asked.request.attempt.as_deref() != Some(run.binding.id.as_str()) { return Err(bad("评审不属于当前运行")); }
        Ok::<(), ApiError>(())
    }).await.map_err(uncertain)??;
    let started = tokio::time::Instant::now();
    loop {
        let poll = project.clone();
        let id = review.clone();
        let value = tokio::task::spawn_blocking(move || {
            let slot = poll.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
            outcome(slot.as_ref().ok_or_else(|| uncertain("项目会话在等待期间关闭"))?, &id)
        }).await.map_err(uncertain)??;
        let elapsed = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        if value["status"] != "pending" || elapsed >= budget { return Ok(Json(value)); }
        tokio::time::sleep(Duration::from_millis((budget - elapsed).min(250))).await;
    }
}

/// A person's verdict on a review (App only).
pub async fn resolve(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Verdict>) -> Result<Json<Value>, ApiError> {
    delegations::app(&headers, &server)?;
    input.data.check().map_err(rejected_as(ErrorCode::Usage))?;
    let data = serde_json::to_value(&input.data).map_err(bad)?;
    delegations::record(server, handle, input.request_id, reviews::RESOLVED, reviews::LAYER, id, data).await
}

/// A person's way out of a held task (App only): let the delegate try again,
/// send the card back to be rewritten, or end the run for someone else. An
/// ended run gives its module leases back.
pub async fn interrupt(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Decision>) -> Result<Json<Value>, ApiError> {
    delegations::app(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let request = same_or_new(runtime, NewEvent { eid: input.request_id, host: server.endpoint.host.clone(), actor: HUMAN.into(),
            kind: task_interrupts::RESOLVED.into(), layer: "L4".into(), obj: id, data: serde_json::to_value(&input.data).map_err(bad)?, guards: vec![] })?;
        let run = dev_state::tasks(runtime.projection()).map_err(bad)?.tasks.get(&request.obj).and_then(|task| task.active_run.clone());
        let event = append(runtime, request)?;
        let projection = dev_state::tasks(runtime.projection()).map_err(bad)?;
        if let Some(run) = run.and_then(|run| projection.runs.get(&run)).filter(|run| run.state != RunState::Active) {
            match runtime.release_work_leases(run.fences.clone(), now) {
                Ok(_) | Err(RuntimeError::Fenced(_)) => {}
                Err(error) => return Err(uncertain(error)),
            }
        }
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(accepted(&event))
    }).await.map_err(uncertain)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_carry_no_signer_and_only_known_choices() {
        let decision = json!({"request_id":"r","choice":"retry","interrupt":"e-1","note":"再试一次"});
        assert!(serde_json::from_value::<Decision>(decision.clone()).is_ok());
        for (key, value) in [("actor", json!("human:local")), ("choice", json!("pass")), ("choice", json!("complete"))] {
            let mut forged = decision.clone();
            forged[key] = value;
            assert!(serde_json::from_value::<Decision>(forged).is_err(), "{key}");
        }
        assert!(serde_json::from_value::<Verdict>(json!({"request_id":"r","verdict":"passed"})).is_ok());
        assert!(serde_json::from_value::<Verdict>(json!({"request_id":"r","verdict":"timeout"})).is_err());
        let ask = json!({"request_id":"r","cwd":"/tmp","stage":"plan","summary":"s"});
        assert!(serde_json::from_value::<Ask>(ask.clone()).is_ok());
        for key in ["version", "attempt", "object"] {
            let mut forged = ask.clone();
            forged[key] = json!("x");
            assert!(serde_json::from_value::<Ask>(forged).is_err(), "{key}");
        }
    }
}
