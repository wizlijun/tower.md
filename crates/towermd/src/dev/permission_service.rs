//! Permission requests (FR-63, spec 16.4). The interaction gateway asks on
//! behalf of its run's harness and waits; only a person answers, from the
//! App, and only once. An unanswered request expires into a denial, recorded
//! by the daemon: nothing here allows a tool on a timer.
use super::business::{now, ready, uncertain, open_failure};
use super::reviews::{append, same_or_new};
use super::*;
use towermd_core::{hub_protocol::NewEvent, project_runtime::ProjectRuntime, question_model, tower_api};
use towermd_core::gates::{GateFile, GATES_PATH};
use towermd_devkit::permissions::{self, Decision, Forbidden, Requested, Resolved};

const TOWER: &str = "process:towermd";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask { request_id: String, run: String, tool: String, input: Value }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wait { run: String, permission: String, #[serde(default)] wait_ms: Option<u64> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer { request_id: String, #[serde(flatten)] data: Resolved }

fn gateway(headers: &HeaderMap, server: &Server) -> Result<tasks::Delegate, ApiError> {
    match tasks::require_delegate(headers, server)? {
        tasks::Delegate::Gateway => Ok(tasks::Delegate::Gateway),
        tasks::Delegate::Cli => Err(rejected_as(ErrorCode::Usage)("授权请求只经交互网关")),
    }
}

/// Extra dangerous commands from `gates.yaml` on main. Unreadable means
/// none extra: the built-in list still applies and a person still answers.
fn extra_commands(project: &Project) -> Vec<String> {
    current_catalog(project).ok().and_then(|catalog| catalog.read_file(GATES_PATH).ok())
        .map(|text| GateFile::from_text(text.as_deref()).dangerous_commands).unwrap_or_default()
}

/// The gateway asks for one tool use of its run, signed as the run's agent.
/// G2 reads the whole input first: a dangerous command or a sensitive file
/// is denied at once, recorded by the daemon, and holds the task for a
/// person (12.1 禁止); nobody is asked to allow it.
pub async fn ask(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Ask>) -> Result<Json<Value>, ApiError> {
    let delegate = gateway(&headers, &server)?;
    valid_scan_id(&input.request_id).map_err(bad)?;
    if input.tool.trim().is_empty() { return Err(rejected_as(ErrorCode::Usage)("授权请求须指明工具")); }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        let signer = delegate.signer(runtime, Some(&input.run))?;
        let task = towermd_devkit::run_record::records(runtime.projection()).map_err(bad)?.remove(&input.run)
            .map(|record| record.task).ok_or_else(|| bad("运行不存在"))?;
        let obj = format!("{}{}", permissions::PREFIX, input.request_id);
        if let Some(rule) = permissions::dangerous(&input.tool, &input.input, &extra_commands(&project)) {
            let data = Forbidden { run: input.run, task, tool: input.tool, input: permissions::summary(&input.input), rule };
            let request = same_or_new(runtime, NewEvent { eid: input.request_id.clone(), host: server.endpoint.host.clone(), actor: TOWER.into(),
                kind: permissions::FORBIDDEN.into(), layer: permissions::LAYER.into(), obj: obj.clone(),
                data: serde_json::to_value(&data).map_err(bad)?, guards: vec![] })?;
            let event = append(runtime, request)?;
            runtime.snapshot(now).map_err(uncertain)?;
            return Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"permission":obj,"status":"denied",
                "decision":Decision::Deny,"closed":permissions::Closed::Forbidden,"note":format!("G2 拦截，已交人处理：{}", data.rule)})));
        }
        let data = Requested { run: input.run, task, tool: input.tool, input: permissions::summary(&input.input) };
        let request = same_or_new(runtime, NewEvent { eid: input.request_id.clone(), host: server.endpoint.host.clone(), actor: signer,
            kind: permissions::REQUESTED.into(), layer: permissions::LAYER.into(), obj,
            data: serde_json::to_value(data).map_err(bad)?, guards: vec![] })?;
        let event = append(runtime, request)?;
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"permission":event.obj,"status":"pending"})))
    }).await.map_err(uncertain)?
}

fn word(decision: Decision) -> &'static str {
    match decision { Decision::Allow => "allowed", Decision::Deny => "denied" }
}

/// Reads the request; at or past its deadline an open request expires into a
/// denial first.
fn settle(runtime: &mut ProjectRuntime, server: &Server, run: &str, id: &str) -> Result<Value, ApiError> {
    let found = |runtime: &ProjectRuntime| permissions::read(runtime.projection()).map_err(uncertain);
    let permission = found(runtime)?.remove(id).ok_or_else(|| rejected_as(ErrorCode::NotFound)("授权请求不存在"))?;
    if permission.run != run { return Err(bad("授权请求不属于当前运行")); }
    let permission = match permission.outcome {
        None if permissions::due(&permission, &tower_api::timestamp(now()).map_err(uncertain)?).map_err(uncertain)? => {
            let request = same_or_new(runtime, NewEvent { eid: permissions::expired_eid(id), host: server.endpoint.host.clone(), actor: TOWER.into(),
                kind: permissions::EXPIRED.into(), layer: permissions::LAYER.into(), obj: id.into(), data: json!({}), guards: vec![] })?;
            append(runtime, request)?;
            runtime.snapshot(now).map_err(uncertain)?;
            found(runtime)?.remove(id).ok_or_else(|| uncertain("授权请求在到期后消失"))?
        }
        _ => permission,
    };
    Ok(match permission.outcome {
        Some(outcome) => json!({"status":word(outcome.decision),"permission":id,"decision":outcome.decision,"closed":outcome.closed,"note":outcome.note}),
        None => json!({"status":"pending","permission":id,"deadline":permission.deadline}),
    })
}

/// Polls in short critical sections, at most one ask-wait per call; an open
/// request is pending, never allowed.
pub async fn wait(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Wait>) -> Result<Json<Value>, ApiError> {
    gateway(&headers, &server)?;
    let project = project(&server, &handle).map_err(bad)?;
    let budget = input.wait_ms.unwrap_or(question_model::ASK_WAIT_MS).min(question_model::ASK_WAIT_MS);
    let started = tokio::time::Instant::now();
    loop {
        let (poll, server, run, id) = (project.clone(), server.clone(), input.run.clone(), input.permission.clone());
        let value = tokio::task::spawn_blocking(move || {
            let mut slot = poll.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
            let runtime = ready(&mut slot, &server, &poll).map_err(open_failure)?;
            settle(runtime, &server, &run, &id)
        }).await.map_err(uncertain)??;
        let elapsed = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        if value["status"] != "pending" || elapsed >= budget { return Ok(Json(value)); }
        tokio::time::sleep(Duration::from_millis((budget - elapsed).min(250))).await;
    }
}

/// A person's answer (App only): allow this once, or deny.
pub async fn resolve(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Answer>) -> Result<Json<Value>, ApiError> {
    delegations::app(&headers, &server)?;
    input.data.check().map_err(rejected_as(ErrorCode::Usage))?;
    let data = serde_json::to_value(&input.data).map_err(bad)?;
    delegations::record(server, handle, input.request_id, permissions::RESOLVED, permissions::LAYER, id, data).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_carry_no_signer_deadline_or_decision_of_their_own() {
        let ask = json!({"request_id":"r","run":"R-1","tool":"Bash","input":{"command":"ls"}});
        assert!(serde_json::from_value::<Ask>(ask.clone()).is_ok());
        for key in ["actor", "agent", "task", "deadline", "decision"] {
            let mut forged = ask.clone();
            forged[key] = json!("x");
            assert!(serde_json::from_value::<Ask>(forged).is_err(), "{key}");
        }
        assert!(serde_json::from_value::<Answer>(json!({"request_id":"r","decision":"allow"})).is_ok());
        for decision in ["timeout", "always", "rule"] {
            assert!(serde_json::from_value::<Answer>(json!({"request_id":"r","decision":decision})).is_err(), "{decision}");
        }
        assert!(serde_json::from_value::<Answer>(json!({"request_id":"r","decision":"deny","actor":"human:local"})).is_err());
    }
}
