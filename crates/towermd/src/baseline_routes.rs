//! Fixed baseline adapters. Domain producers own request identity and durability.
use super::*;
use towermd_core::baseline_review::{review_baseline_shared, review_view, ReviewAction};
use towermd_core::recovery_contract::recorded::RequestBody;
use towermd_core::recovery_outbox::{RecoveryContext, RecoveryOutbox};
use towermd_core::request_ledger::{DraftSaveAction, PreparedDraftSaveRequest};

type Reply = (StatusCode, Json<Value>);

fn uncertain(request_id: &str, reason: impl std::fmt::Display) -> String {
    format!("baseline request {request_id} uncertain: {reason}")
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
            } else if error == "repository writer is busy" {
                (StatusCode::SERVICE_UNAVAILABLE, "writer_busy")
            } else if error.contains("conflict")
                || error.contains("reused")
                || error.contains("expected revision")
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
            Json(json!({
                "error":error.to_string(),"code":"request_uncertain","request_id":request_id
            })),
        ),
    }
}

fn save(
    server: &Server,
    directory: &Path,
    run: Option<String>,
    request_id: &str,
    actor: String,
    action: DraftSaveAction,
) -> Result<Value, String> {
    if *server.shutdown.borrow() {
        return Err(uncertain(request_id, "守护进程正在停止"));
    }
    let accepted = server
        .catalog(directory)
        .map_err(|e| uncertain(request_id, e))?;
    let request = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: accepted.config.project_id().into(),
            host: server.endpoint.host.clone(),
            actor,
            run,
            action,
        },
        request_id,
    )?;
    let known = original_request(server, &accepted, request_id)?;
    if let Some(known) = &known {
        let known = serde_json::to_value(known).map_err(|error| error.to_string())?;
        if known["p3_request"]["body_hash"] != request.metadata().body_hash {
            return Err("request_id reused with a different original body".into());
        }
    } else if request.body().actor == "process:towermd-cli" {
        // An accepted original is coordinated before current run checks. App
        // editing has no implicit task run or lease renewal.
        server.renew_existing_read(&accepted, request.body().run.as_deref())?;
    }
    produce_save(server, &accepted, &request)
}

fn produce_save(
    server: &Server,
    accepted: &Catalog,
    request: &PreparedDraftSaveRequest,
) -> Result<Value, String> {
    let request_id = &request.metadata().request_id;
    let project = server
        .project(accepted)
        .map_err(|e| uncertain(request_id, e))?;
    let _producer = project
        .producer
        .lock()
        .map_err(|_| uncertain(request_id, "项目写入队列锁损坏"))?;
    for attempt in 0..3 {
        let catalog = current_scan_catalog(accepted).map_err(|e| uncertain(request_id, e))?;
        let mut outbox = RecoveryOutbox::open_with_uncertainty(
            &server
                .runtime
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            RecoveryContext::from_catalog(&catalog).map_err(|e| uncertain(request_id, e))?,
            project.outbox_uncertainty.clone(),
        )
        .map_err(|error| {
            format!(
                "baseline request {} uncertain: outbox unavailable: {error}",
                request.metadata().request_id
            )
        })?;
        match towermd_core::request_ledger::save_draft_shared(
            &project.hub,
            &catalog,
            &mut outbox,
            &project.writer,
            request,
            || (server.clock)(),
        ) {
            Err(error)
                if attempt < 2
                    && (error == "authority changed; reload Catalog"
                        || error
                            == format!(
                                "baseline request {} uncertain: authority changed; reload Catalog",
                                request.metadata().request_id
                            )) =>
            {
                // Keep the exact original request. Only reload a concurrently
                // advanced authority; core still checks its complete protected
                // map, fixed source and expected draft revision on every pass.
            }
            result => return serde_json::to_value(result?).map_err(|error| error.to_string()),
        }
    }
    unreachable!("the final attempt always returns")
}

fn original_request(
    server: &Server,
    catalog: &Catalog,
    request_id: &str,
) -> Result<Option<towermd_core::request_ledger::BaselineRequestStatus>, String> {
    // An unreadable journal cannot establish that a previous attempt was
    // refused. Keep the original request available for coordination.
    let read = (|| {
        towermd_core::request_ledger::read_baseline_request(
            catalog,
            &server.recovery_inputs(catalog)?,
            request_id,
        )
    })();
    read.map_err(|reason| uncertain(request_id, reason))
}

fn reviewed_response(
    server: &Server,
    project: &Project,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    result: towermd_core::baseline_review::ReviewResult,
) -> Result<Value, String> {
    use towermd_core::{baseline_review::Verdict, snapshot_service::PublicationError};
    let mut value = serde_json::to_value(&result).map_err(|error| error.to_string())?;
    if result.receipt.result_ref.verdict == Verdict::Approved {
        let approval = &result.receipt.result_ref;
        let publication = project.writer.publish_baseline(
            &project.hub,
            catalog,
            outbox,
            &approval.approval_eid,
            &server.endpoint.host,
            || (server.clock)(),
        );
        value["publication"] = match publication {
            Ok(report) => serde_json::to_value(report).map_err(|error| error.to_string())?,
            Err(PublicationError::Busy) => {
                project.publication_reconcile.store(true, Ordering::Release);
                json!({
                    "state":"pending","publication_id":approval.approval_eid,
                    "target":approval.binding.target_baseline,"commit":null,
                    "stage":"preparation","reason":"repository writer is busy"
                })
            }
            Err(error) => json!({"state":"blocked","publication_id":approval.approval_eid,
                "target":approval.binding.target_baseline,"reason":error.to_string()}),
        };
    }
    // An accepted review stays accepted even if publication remains pending or
    // needs a new review. Never replace its original receipt with a Git outcome.
    Ok(value)
}

pub(super) async fn submit(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<BaselineSubmitRequest>,
) -> Reply {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    let request_id = request.request_id.clone();
    mutation_response(
        tokio::task::spawn_blocking(move || {
            save(
                &server,
                &request.directory,
                request.run,
                &request.request_id,
                "process:towermd-cli".into(),
                DraftSaveAction::SubmitBaseline {
                    submit: request.input,
                },
            )
        })
        .await,
        &request_id,
    )
}

pub(super) async fn propose(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<ProposeRequest>,
) -> Reply {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    let request_id = request.request_id.clone();
    mutation_response(
        tokio::task::spawn_blocking(move || {
            // Validate shape and UUID before registering a project or creating an
            // outbox. Core still owns source, impact and full Run authorization.
            towermd_core::proposals::validate_proposal(&request.input)?;
            towermd_core::recovery_contract::recorded::request_event_eid(
                &request.request_id,
                "proposal.created",
            )?;
            if *server.shutdown.borrow() {
                return Err(uncertain(&request.request_id, "守护进程正在停止"));
            }
            let accepted = server
                .catalog(&request.directory)
                .map_err(|error| uncertain(&request.request_id, error))?;
            let project = server
                .project(&accepted)
                .map_err(|error| uncertain(&request.request_id, error))?;
            let _producer = project
                .producer
                .lock()
                .map_err(|_| uncertain(&request.request_id, "项目写入队列锁损坏"))?;
            if *server.shutdown.borrow() {
                return Err(uncertain(&request.request_id, "守护进程正在停止"));
            }
            let catalog = current_scan_catalog(&accepted)
                .map_err(|error| uncertain(&request.request_id, error))?;
            let mut outbox = RecoveryOutbox::open_with_uncertainty(
                &server
                    .runtime
                    .join("recovery-outbox")
                    .join(catalog.config.project_id()),
                RecoveryContext::from_catalog(&catalog)
                    .map_err(|error| uncertain(&request.request_id, error))?,
                project.outbox_uncertainty.clone(),
            )
            .map_err(|error| uncertain(&request.request_id, error))?;
            // Do not use generic draft.saved or renew/current-Run preflight here:
            // the finite producer repairs an accepted original UUID first.
            let outcome = project.writer.save_proposal(
                &project.hub,
                &catalog,
                &request.input,
                request.run.as_deref(),
                &request.request_id,
                &mut outbox,
                &server.endpoint.host,
                || (server.clock)(),
            )?;
            serde_json::to_value(outcome).map_err(|error| uncertain(&request.request_id, error))
        })
        .await,
        &request_id,
    )
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum EditRequest {
    Create {
        directory: PathBuf,
        human_id: String,
        request_id: String,
        baseline: String,
        main_revision: String,
    },
    Edit {
        directory: PathBuf,
        human_id: String,
        request_id: String,
        id: String,
        expected_revision: String,
        edit: towermd_core::baseline_draft::DraftEdit,
    },
}

pub(super) async fn edit(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<EditRequest>,
) -> Reply {
    let permit = match gate(&headers, &server) {
        Ok(permit) => permit,
        Err(reply) => return reply,
    };
    let (directory, human_id, request_id, action) = match request {
        EditRequest::Create {
            directory,
            human_id,
            request_id,
            baseline,
            main_revision,
        } => (
            directory,
            human_id,
            request_id,
            DraftSaveAction::CreateDraft {
                baseline,
                main_revision,
            },
        ),
        EditRequest::Edit {
            directory,
            human_id,
            request_id,
            id,
            expected_revision,
            edit,
        } => (
            directory,
            human_id,
            request_id,
            DraftSaveAction::EditDraft {
                id,
                expected_revision,
                edit,
            },
        ),
    };
    let actor = match permit.human_actor(&human_id) {
        Ok(actor) => actor,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":error.to_string()})),
            )
        }
    };
    let original_id = request_id.clone();
    mutation_response(
        tokio::task::spawn_blocking(move || {
            save(&server, &directory, None, &request_id, actor, action)
        })
        .await,
        &original_id,
    )
}

fn gate(headers: &HeaderMap, server: &Server) -> Result<app_gate::AppGate, Reply> {
    app_gate::validate_app(headers, &server.endpoint.instance).map_err(|error| {
        let status = if error == app_gate::AppGateError::InstanceChanged {
            StatusCode::CONFLICT
        } else {
            StatusCode::FORBIDDEN
        };
        (status, Json(json!({"error":error.to_string()})))
    })
}

pub(super) async fn schema(State(server): State<Arc<Server>>, headers: HeaderMap) -> Reply {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    match serde_json::from_str(towermd_core::baseline_schema::schema()) {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

pub(super) fn saved_draft(
    catalog: &Catalog,
    id: &str,
    inputs: &towermd_core::recovery_outbox::RecoveryInputs,
) -> Result<Option<Value>, String> {
    snapshot_service::read_draft(catalog, id)
        .map_err(|error| error.to_string())?
        .map(|saved| {
            let current = towermd_core::baseline_current::validate_draft_target(
                catalog,
                inputs,
                &saved.frozen().bundle.target_baseline,
            )
            .and_then(|()| towermd_core::baseline_draft::freeze(catalog, &saved.frozen().bundle));
            let current_validity = match current {
                Ok(frozen) if &frozen == saved.frozen() => json!({"state":"current"}),
                Ok(_) => json!({"state":"historical_only","reason":"当前冻结内容已变化"}),
                Err(reason) => json!({"state":"historical_only","reason":reason}),
            };
            Ok(json!({
                "state":"saved", "project_id":catalog.config.project_id(),
                "id":id, "revision":saved.revision(), "request_id":saved.request_id(),
                "draft":saved.frozen(), "current_validity":current_validity
            }))
        })
        .transpose()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewQuery {
    directory: PathBuf,
}

/// Resolve only actual published baseline proofs. This read does not create a
/// project session, recover scan jobs, renew leases, or deliver an inbox.
pub(super) async fn current(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Query(query): Query<ReviewQuery>,
) -> Reply {
    if let Err(reply) = gate(&headers, &server) {
        return reply;
    }
    match tokio::task::spawn_blocking(move || {
        let catalog = server.catalog(&query.directory)?;
        towermd_core::baseline_current::current_baseline(
            &catalog,
            &server.recovery_inputs(&catalog)?,
        )
    })
    .await
    {
        Ok(Ok(current)) => (StatusCode::OK, Json(json!(current))),
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error,"code":"current_baseline_unavailable"})),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string(),"code":"current_baseline_unavailable"})),
        ),
    }
}

/// App display reads never enter the CLI/scan activity prelude or reconstruct
/// scan jobs. Only the actual durable draft ref can supply this response.
pub(super) async fn saved(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<ReviewQuery>,
) -> Reply {
    if let Err(reply) = gate(&headers, &server) {
        return reply;
    }
    match tokio::task::spawn_blocking(move || {
        let catalog = server.catalog(&query.directory)?;
        saved_draft(&catalog, &id, &server.recovery_inputs(&catalog)?)
    })
    .await
    {
        Ok(Ok(Some(value))) => (StatusCode::OK, Json(value)),
        Ok(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"已保存草稿不存在"})),
        ),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

pub(super) async fn request_status(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<ReviewQuery>,
) -> Reply {
    if let Err(reply) = gate(&headers, &server) {
        return reply;
    }
    if let Err(error) =
        towermd_core::recovery_contract::recorded::request_event_eid(&id, "draft.saved")
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error,"code":"request_refused","request_id":id})),
        );
    }
    let request_id = id.clone();
    match tokio::task::spawn_blocking(move || {
        let catalog = server
            .catalog(&query.directory)
            .map_err(|e| uncertain(&id, e))?;
        original_request(&server, &catalog, &id)
    })
    .await
    {
        Ok(Ok(Some(value))) => (StatusCode::OK, Json(json!(value))),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, Json(json!({"error":"原请求不存在"}))),
        Ok(Err(error)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":error,"code":"request_uncertain","request_id":request_id})),
        ),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(
                json!({"error":uncertain(&request_id, error),"code":"request_uncertain","request_id":request_id}),
            ),
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RetryRequest {
    directory: PathBuf,
}

pub(super) async fn retry_request(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Json(request): Json<RetryRequest>,
) -> Reply {
    if let Err(reply) = gate(&headers, &server) {
        return reply;
    }
    let request_id = id.clone();
    mutation_response(
        tokio::task::spawn_blocking(move || {
            use towermd_core::recovery_contract::recorded::BaselineReviewAction;
            use towermd_core::request_ledger::BaselineRequestStatus;
            if *server.shutdown.borrow() {
                return Err(uncertain(&id, "守护进程正在停止"));
            }
            let accepted = server
                .catalog(&request.directory)
                .map_err(|e| uncertain(&id, e))?;
            let original = original_request(&server, &accepted, &id)?
                .ok_or("原请求不存在；不能由重试端点创造新动作")?;
            match original {
                BaselineRequestStatus::DraftSave {
                    request_body,
                    p3_request,
                    ..
                } => {
                    let original =
                        PreparedDraftSaveRequest::prepare(request_body, &p3_request.request_id)?;
                    if original.metadata() != &p3_request {
                        return Err("原请求摘要不一致".into());
                    }
                    produce_save(&server, &accepted, &original)
                }
                BaselineRequestStatus::Review {
                    request_body,
                    p3_request,
                    ..
                } => {
                    let BaselineReviewAction::ReviewBaseline {
                        binding,
                        verdict,
                        comment,
                    } = request_body.action;
                    let action = ReviewAction {
                        request_id: p3_request.request_id,
                        binding,
                        verdict,
                        comment,
                    };
                    let project = server
                        .project(&accepted)
                        .map_err(|e| uncertain(&action.request_id, e))?;
                    let _producer = project
                        .producer
                        .lock()
                        .map_err(|_| uncertain(&action.request_id, "项目写入队列锁损坏"))?;
                    let catalog = current_scan_catalog(&accepted)
                        .map_err(|e| uncertain(&action.request_id, e))?;
                    let mut outbox = RecoveryOutbox::open_with_uncertainty(
                        &server
                            .runtime
                            .join("recovery-outbox")
                            .join(catalog.config.project_id()),
                        RecoveryContext::from_catalog(&catalog)
                            .map_err(|e| uncertain(&action.request_id, e))?,
                        project.outbox_uncertainty.clone(),
                    )
                    .map_err(|error| {
                        format!(
                            "baseline request {} uncertain: outbox unavailable: {error}",
                            action.request_id
                        )
                    })?;
                    let result = review_baseline_shared(
                        &project.hub,
                        &catalog,
                        &mut outbox,
                        &action,
                        &request_body.actor,
                        &request_body.host,
                        (server.clock)().map_err(|e| uncertain(&action.request_id, e))?,
                    )?;
                    reviewed_response(&server, &project, &catalog, &mut outbox, result)
                }
            }
        })
        .await,
        &request_id,
    )
}

pub(super) async fn review(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<ReviewQuery>,
) -> Reply {
    if let Err(reply) = gate(&headers, &server) {
        return reply;
    }
    match tokio::task::spawn_blocking(move || {
        let catalog = server.catalog(&query.directory)?;
        let inputs = server.recovery_inputs(&catalog)?;
        review_view(&catalog, &inputs, &id)
    })
    .await
    {
        Ok(Ok(Some(view))) => (StatusCode::OK, Json(json!(view))),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, Json(json!({"error":"草稿不存在"}))),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionRequest {
    directory: PathBuf,
    human_id: String,
    action: ReviewAction,
}

pub(super) async fn action(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Json(request): Json<ActionRequest>,
) -> Reply {
    let permit = match gate(&headers, &server) {
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
    if id != request.action.binding.object_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"URL 与审阅对象不一致"})),
        );
    }
    let request_id = request.action.request_id.clone();
    mutation_response(
        tokio::task::spawn_blocking(move || {
            if *server.shutdown.borrow() {
                return Err(uncertain(&request.action.request_id, "守护进程正在停止"));
            }
            let accepted = server
                .catalog(&request.directory)
                .map_err(|e| uncertain(&request.action.request_id, e))?;
            let project = server
                .project(&accepted)
                .map_err(|e| uncertain(&request.action.request_id, e))?;
            let _producer = project
                .producer
                .lock()
                .map_err(|_| uncertain(&request.action.request_id, "项目写入队列锁损坏"))?;
            let catalog = current_scan_catalog(&accepted)
                .map_err(|e| uncertain(&request.action.request_id, e))?;
            let mut outbox = RecoveryOutbox::open_with_uncertainty(
                &server
                    .runtime
                    .join("recovery-outbox")
                    .join(catalog.config.project_id()),
                RecoveryContext::from_catalog(&catalog)
                    .map_err(|e| uncertain(&request.action.request_id, e))?,
                project.outbox_uncertainty.clone(),
            )
            .map_err(|error| {
                format!(
                    "baseline request {} uncertain: outbox unavailable: {error}",
                    request.action.request_id
                )
            })?;
            let result = review_baseline_shared(
                &project.hub,
                &catalog,
                &mut outbox,
                &request.action,
                &actor,
                &server.endpoint.host,
                (server.clock)().map_err(|e| uncertain(&request.action.request_id, e))?,
            )?;
            reviewed_response(&server, &project, &catalog, &mut outbox, result)
        })
        .await,
        &request_id,
    )
}

#[cfg(test)]
#[path = "baseline_routes_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "proposal_routes_tests.rs"]
mod proposal_tests;
