//! Fixed App-only Task recheck reader and human resolution. No raw Event,
//! arbitrary actor, Run override, source commit or bypass flag is accepted.
use super::*;
use towermd_core::proposals::TaskRecheckAction;
type Reply = (StatusCode, Json<Value>);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecheckQuery {
    directory: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecheckRequest {
    directory: PathBuf,
    human_id: String,
    action: TaskRecheckAction,
}
pub(super) async fn load(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(task): RoutePath<String>,
    Query(query): Query<RecheckQuery>,
) -> Reply {
    if let Err(reply) = human_routes::app_gate(&headers, &server) {
        return reply;
    }
    match tokio::task::spawn_blocking(move || {
        let catalog = Catalog::load(&query.directory)?;
        let project = server.registered_project(catalog.config.project_id())?;
        if project
            .as_ref()
            .is_some_and(|project| project.common_dir != catalog.common_dir)
        {
            return Err("Task recheck project ID belongs to another clone".into());
        }
        let inputs = {
            let _producer = project
                .as_ref()
                .map(|project| project.producer.lock().map_err(|_| "项目写入队列锁损坏"))
                .transpose()?;
            server.recovery_inputs(&catalog)?
        };
        let view = ProjectSession::read_task_recheck(
            &catalog,
            &inputs,
            &task,
            (server.clock)()?,
            project.as_ref().map(|project| project.hub.as_ref()),
        )
        .map_err(|e| e.to_string())?;
        let after = server.registered_project(catalog.config.project_id())?;
        if match (&project, &after) {
            (None, None) => false,
            (Some(before), Some(after)) => !Arc::ptr_eq(before, after),
            _ => true,
        } {
            return Err("Task recheck project registered or changed while reading; refresh".into());
        }
        serde_json::to_value(view).map_err(|e| e.to_string())
    })
    .await
    {
        Ok(Ok(view)) => (StatusCode::OK, Json(view)),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}
pub(super) async fn resolve(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(task): RoutePath<String>,
    Json(request): Json<RecheckRequest>,
) -> Reply {
    let permit = match human_routes::app_gate(&headers, &server) {
        Ok(permit) => permit,
        Err(reply) => return reply,
    };
    let actor = match permit.human_actor(&request.human_id) {
        Ok(actor) => actor,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":error.to_string()})),
            )
        }
    };
    let id = request.action.request_id.clone();
    if task != request.action.binding.task_id {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"Task recheck route/binding mismatch","request_id":id})),
        );
    }
    match tokio::task::spawn_blocking(move || {
        if *server.shutdown.borrow() {
            return Err(format!(
                "Task recheck request {} uncertain: daemon stopping",
                request.action.request_id
            ));
        }
        let accepted = Catalog::load(&request.directory)?;
        towermd_core::proposals::RecheckRequest::prepare(
            accepted.config.project_id(),
            &server.endpoint.host,
            &actor,
            &request.action,
        )?;
        // Only this explicit App human mutation may activate the existing
        // producer. Missing storage remains behind the separate recovery gate.
        let project = server.project_existing(&accepted)?;
        let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
        let catalog = current_scan_catalog(&accepted)?;
        let mut outbox = server.task_outbox(&catalog)?;
        project
            .writer
            .resolve_task_recheck(
                &project.hub,
                &catalog,
                &server.endpoint.host,
                &actor,
                &request.action,
                &mut outbox,
                || (server.clock)(),
            )
            .and_then(|outcome| serde_json::to_value(outcome).map_err(|e| e.to_string()))
    })
    .await
    {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => {
            let (status, code) = if error.contains("uncertain") {
                (StatusCode::SERVICE_UNAVAILABLE, "request_uncertain")
            } else if [
                "binding", "changed", "reused", "conflict", "expired", "stale",
            ]
            .iter()
            .any(|word| error.contains(word))
            {
                (StatusCode::CONFLICT, "request_conflict")
            } else {
                (StatusCode::BAD_REQUEST, "request_refused")
            };
            (
                status,
                Json(json!({"error":error,"code":code,"request_id":id})),
            )
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string(),"code":"request_uncertain","request_id":id})),
        ),
    }
}
#[cfg(test)]
#[path = "app_task_recheck_routes_tests.rs"]
mod tests;
