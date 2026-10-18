//! Finite Task review adapters. The core owns binding, request identity and
//! durability; only the fixed App boundary can construct a human actor.
use super::*;
use towermd_core::{
    human_intervention::HumanCommand, recovery_contract::recorded::TaskReviewAction,
};

type Reply = (StatusCode, Json<Value>);

fn uncertain(request_id: &str, reason: impl std::fmt::Display) -> String {
    format!("Task review request {request_id} uncertain: {reason}")
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InteractionRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
    pub request_id: String,
    pub command: HumanCommand,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewQuery {
    directory: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewActionRequest {
    directory: PathBuf,
    request_id: String,
    human_id: String,
    action: TaskReviewAction,
}

fn app_gate(headers: &HeaderMap, server: &Server) -> Result<app_gate::AppGate, Reply> {
    app_gate::validate_app(headers, &server.endpoint.instance).map_err(|error| {
        let status = match error {
            app_gate::AppGateError::InstanceChanged => StatusCode::CONFLICT,
            _ => StatusCode::FORBIDDEN,
        };
        (status, Json(json!({"error":error.to_string()})))
    })
}

fn mutation_response(
    result: Result<Result<Value, String>, tokio::task::JoinError>,
    request_id: &str,
) -> Reply {
    match result {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => {
            let (status, code) = if error.contains("uncertain") {
                (StatusCode::SERVICE_UNAVAILABLE, "request_uncertain")
            } else if [
                "conflict", "reused", "binding", "changed", "stale", "expired",
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
                Json(json!({"error":error,"code":code,"request_id":request_id})),
            )
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"error":error.to_string(),"code":"request_uncertain","request_id":request_id}),
            ),
        ),
    }
}

pub(super) async fn interaction(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<InteractionRequest>,
) -> Reply {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    if !matches!(&request.command, HumanCommand::ReviewRequest { .. }) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"此入口目前只接受任务评审请求"})),
        );
    }
    let request_id = request.request_id.clone();
    mutation_response(
        tokio::task::spawn_blocking(move || {
            if *server.shutdown.borrow() {
                return Err(format!(
                    "request {} uncertain: daemon is stopping",
                    request.request_id
                ));
            }
            let id = &request.request_id;
            towermd_core::recovery_contract::recorded::request_event_eid(id, "hold.opened")?;
            let accepted = server
                .catalog(&request.directory)
                .map_err(|error| uncertain(id, error))?;
            if !probe_store(&accepted.common_dir.join("towermd"))
                .map_err(|error| uncertain(id, error))?
            {
                return Err(uncertain(id, "hub journal 不存在；须先恢复"));
            }
            let project = server
                .project(&accepted)
                .map_err(|error| uncertain(id, error))?;
            let _producer = project
                .producer
                .lock()
                .map_err(|_| uncertain(id, "项目写入队列锁损坏"))?;
            let catalog = current_scan_catalog(&accepted).map_err(|error| uncertain(id, error))?;
            let mut outbox = server
                .task_outbox(&catalog)
                .map_err(|error| uncertain(id, error))?;
            ProjectSession::request_task_review_shared(
                &project.hub,
                &catalog,
                &server.endpoint.host,
                &request.request_id,
                &request.command,
                request.run.as_deref(),
                &mut outbox,
                || (server.clock)(),
            )
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        })
        .await,
        &request_id,
    )
}

pub(super) async fn review(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(hold): RoutePath<String>,
    Query(query): Query<ReviewQuery>,
) -> Reply {
    if let Err(reply) = app_gate(&headers, &server) {
        return reply;
    }
    match tokio::task::spawn_blocking(move || {
        let catalog = server.catalog(&query.directory)?;
        if !probe_store(&catalog.common_dir.join("towermd"))? {
            return Err("hub journal 不存在；须先完成恢复，读取不会创建 hub".into());
        }
        let project = server.project(&catalog)?;
        let inputs = server.recovery_inputs(&catalog)?;
        ProjectSession::load_task_review_shared(
            &project.hub,
            &catalog,
            &inputs,
            &server.endpoint.host,
            &hold,
            || (server.clock)(),
        )
        .and_then(|view| serde_json::to_value(view).map_err(|error| error.to_string()))
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

pub(super) async fn action(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<ReviewActionRequest>,
) -> Reply {
    let permit = match app_gate(&headers, &server) {
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
    let request_id = request.request_id.clone();
    mutation_response(
        tokio::task::spawn_blocking(move || {
            if *server.shutdown.borrow() {
                return Err(format!(
                    "request {} uncertain: daemon is stopping",
                    request.request_id
                ));
            }
            let id = &request.request_id;
            towermd_core::recovery_contract::recorded::request_event_eid(id, "hold.reviewed")?;
            let accepted = server
                .catalog(&request.directory)
                .map_err(|error| uncertain(id, error))?;
            if !probe_store(&accepted.common_dir.join("towermd"))
                .map_err(|error| uncertain(id, error))?
            {
                return Err(uncertain(id, "hub journal 不存在；须先恢复"));
            }
            let project = server
                .project(&accepted)
                .map_err(|error| uncertain(id, error))?;
            let _producer = project
                .producer
                .lock()
                .map_err(|_| uncertain(id, "项目写入队列锁损坏"))?;
            let catalog = current_scan_catalog(&accepted).map_err(|error| uncertain(id, error))?;
            let mut outbox = server
                .task_outbox(&catalog)
                .map_err(|error| uncertain(id, error))?;
            ProjectSession::answer_task_review_shared(
                &project.hub,
                &catalog,
                &server.endpoint.host,
                &actor,
                &request.request_id,
                &request.action,
                &mut outbox,
                || (server.clock)(),
            )
            .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        })
        .await,
        &request_id,
    )
}
