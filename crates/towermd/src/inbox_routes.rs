//! Fixed process-only inbox boundary. Tokens never enter request identity.
use super::*;
use towermd_core::{
    cli_delivery::{InboxActivity, InboxBatch, InboxReadFailure},
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
};

#[derive(Default)]
pub(super) struct DeliveryTokens {
    batches: BTreeMap<String, InboxBatch>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InboxRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InboxResponse {
    pub messages: Vec<towermd_core::human_intervention::Message>,
    pub delivery_token: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeliveredRequest {
    pub directory: PathBuf,
    pub request_id: String,
    pub delivery_token: String,
    pub message_ids: Vec<String>,
}
type Reply = (StatusCode, Json<Value>);
fn activity(headers: &HeaderMap) -> Result<InboxActivity, String> {
    match headers.get("x-towermd-inbox-activity") {
        None => Ok(InboxActivity::Project),
        Some(value) => match value.to_str().map_err(|_| "invalid inbox activity")? {
            "project" => Ok(InboxActivity::Project),
            "recovery" => Ok(InboxActivity::Recovery),
            _ => Err("unknown inbox activity".into()),
        },
    }
}
fn denied() -> Reply {
    (
        StatusCode::CONFLICT,
        Json(json!({"error":"daemon instance changed","code":"instance_changed"})),
    )
}
fn failure(error: String, id: Option<&str>) -> Reply {
    let uncertain = error.contains("uncertain");
    (
        if uncertain {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::BAD_REQUEST
        },
        Json(
            json!({"error":error,"code":if uncertain{"request_uncertain"}else{"request_refused"},"request_id":id}),
        ),
    )
}
#[cfg(test)]
thread_local! {
    static INBOX_REFRESH_FULL_LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn inbox_catalog(accepted: &Catalog) -> Result<Option<Catalog>, String> {
    if accepted.matches_current_source().unwrap_or(false) {
        return Ok(None);
    }
    // A newer authority can legitimately be accepted by the old independent
    // current_scan_catalog check. Do not turn reuse equality into a new gate.
    #[cfg(test)]
    INBOX_REFRESH_FULL_LOADS.with(|count| count.set(count.get() + 1));
    current_scan_catalog(accepted).map(Some)
}

pub(super) async fn inbox(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<InboxRequest>,
) -> Reply {
    if !authorized(&headers, &server) {
        return denied();
    }
    let activity = match activity(&headers) {
        Ok(activity) => activity,
        Err(error) => return failure(error, None),
    };
    let result =
        tokio::task::spawn_blocking(move || -> Result<Option<InboxResponse>, InboxReadFailure> {
            if *server.shutdown.borrow() {
                return Err("daemon shutting down".into());
            }
            let accepted = server.catalog(&request.directory)?;
            if activity == InboxActivity::Recovery
                && matches!(
                    inspect_store(&accepted.common_dir.join("towermd"))?,
                    StoreState::Missing | StoreState::Empty
                )
            {
                // This is an explicitly unavailable read, not an empty successful
                // inbox. No project, Hub, token, ack or system lease is created.
                return Ok(None);
            }
            let project = server.project(&accepted)?;
            let _producer = project
                .producer
                .lock()
                .map_err(|_| "project producer lock poisoned")?;
            let refreshed = inbox_catalog(&accepted)?;
            let catalog = refreshed.as_ref().unwrap_or(&accepted);
            let mut outbox = RecoveryOutbox::open_with_uncertainty(
                &server
                    .runtime
                    .join("recovery-outbox")
                    .join(catalog.config.project_id()),
                RecoveryContext::from_catalog(catalog)?,
                project.outbox_uncertainty.clone(),
            )?;
            let batch = ProjectSession::inbox_shared(
                &project.hub,
                catalog,
                &server.endpoint.host,
                request.run.as_deref(),
                activity,
                &mut outbox,
                || (server.clock)(),
            )?;
            let messages = batch.messages().to_vec();
            let token = if messages.is_empty() {
                String::new()
            } else {
                let token = Uuid::now_v7().to_string();
                server
                    .delivery_tokens
                    .lock()
                    .map_err(|_| "delivery token lock poisoned")?
                    .batches
                    .insert(token.clone(), batch);
                token
            };
            Ok(Some(InboxResponse {
                messages,
                delivery_token: token,
            }))
        })
        .await;
    match result {
        Ok(Ok(Some(value))) => (StatusCode::OK, Json(json!(value))),
        Ok(Ok(None)) => (
            StatusCode::CONFLICT,
            Json(
                json!({"error":"inbox unavailable until explicit App recovery; no messages acknowledged","code":"inbox_unavailable_for_recovery"}),
            ),
        ),
        Ok(Err(error)) if error.is_read_changed() => (
            StatusCode::CONFLICT,
            Json(json!({"error":error.to_string(),"code":"inbox_read_changed","request_id":null})),
        ),
        Ok(Err(error)) => failure(error.to_string(), None),
        Err(error) => failure(format!("inbox task failed: {error}"), None),
    }
}
pub(super) async fn delivered(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<DeliveredRequest>,
) -> Reply {
    if !authorized(&headers, &server) {
        return denied();
    }
    if let Err(error) = activity(&headers) {
        return failure(error, Some(&request.request_id));
    }
    let id = request.request_id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, String> {
        if *server.shutdown.borrow() {
            return Err(format!(
                "delivery request {} uncertain: daemon shutting down",
                request.request_id
            ));
        }
        let batch = server
            .delivery_tokens
            .lock()
            .map_err(|_| "delivery token lock poisoned")?
            .batches
            .get(&request.delivery_token)
            .cloned()
            .ok_or("delivery token is unknown for this daemon instance")?;
        let accepted = server.catalog(&request.directory)?;
        if accepted.root != batch.directory() {
            return Err("delivery token belongs to another canonical worktree".into());
        }
        let project = server.project(&accepted)?;
        let _producer = project
            .producer
            .lock()
            .map_err(|_| "project producer lock poisoned")?;
        let catalog = current_scan_catalog(&accepted)?;
        let mut outbox = RecoveryOutbox::open_with_uncertainty(
            &server
                .runtime
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            RecoveryContext::from_catalog(&catalog)?,
            project.outbox_uncertainty.clone(),
        )?;
        let result = ProjectSession::acknowledge_inbox_shared(
            &project.hub,
            &catalog,
            &server.endpoint.host,
            &batch,
            &request.request_id,
            &request.message_ids,
            &mut outbox,
            || (server.clock)(),
        )?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
    .await;
    match result {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => failure(error, Some(&id)),
        Err(error) => failure(
            format!("delivery request {id} uncertain: {error}"),
            Some(&id),
        ),
    }
}

#[cfg(test)]
#[path = "inbox_routes_tests.rs"]
mod tests;
