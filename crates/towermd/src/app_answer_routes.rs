//! Fixed App Ask reader/Answer adapter. It accepts no raw actor, source, Hold,
//! fences, URL or method. Core owns immutable requests and actual source proof.
use super::*;
use towermd_core::{ask_answer_contract::AppAnswerAction, recovery_outbox::RecoveryOutbox};
type Reply = (StatusCode, Json<Value>);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AskQuery {
    directory: PathBuf,
}

pub(super) async fn load(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(hold): RoutePath<String>,
    Query(query): Query<AskQuery>,
) -> Reply {
    if let Err(reply) = human_routes::app_gate(&headers, &server) {
        return reply;
    }
    match tokio::task::spawn_blocking(move || {
        let catalog = server.catalog(&query.directory)?;
        if !probe_store(&catalog.common_dir.join("towermd"))? {
            return Err("hub journal 不存在；须先完成恢复，读取不会创建 hub".into());
        }
        let project = server.project(&catalog)?;
        let inputs = server.recovery_inputs(&catalog)?;
        ProjectSession::load_ask_intervention_shared(&project.hub, &catalog, &inputs, &hold, || {
            (server.clock)()
        })
        .and_then(|view| serde_json::to_value(view).map_err(|e| e.to_string()))
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
// The shared /action handler already holds the fixed AppGate identity and the
// same project's producer/outbox lock. No alternate append path exists here.
pub(super) fn answer(
    server: &Server,
    project: &Project,
    catalog: &Catalog,
    actor: &str,
    id: &str,
    action: &AppAnswerAction,
    outbox: &mut RecoveryOutbox,
) -> Result<Value, String> {
    #[cfg(test)]
    tests::before_answer(catalog);
    ProjectSession::answer_ask_shared(
        &project.hub,
        catalog,
        &server.endpoint.host,
        actor,
        id,
        action,
        outbox,
        || (server.clock)(),
    )
    .and_then(|outcome| serde_json::to_value(outcome).map_err(|e| e.to_string()))
}

#[cfg(test)]
#[path = "app_answer_routes_tests.rs"]
mod tests;

#[cfg(test)]
pub(super) use tests::fixture::{call, claimed, open, Fixture, NOW};
