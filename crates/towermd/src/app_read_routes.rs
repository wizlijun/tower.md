//! Fixed App-only transport for recorded task progress. No registration or activity prelude.
use super::*;
use towermd_core::project_session::SessionSyncError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProgressQuery {
    directory: PathBuf,
}

pub(super) async fn projection(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Query(query): Query<ProgressQuery>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"code":"daemon_changed","error":"守护进程实例已改变，请重新读取"})),
        );
    }
    let result = tokio::task::spawn_blocking(move || read(&server, &query.directory)).await;
    match result {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => {
            let (status, code) = if matches!(&error, SessionSyncError::SourceChanged(_)) {
                (StatusCode::CONFLICT, "task_progress_source_changed")
            } else {
                (StatusCode::BAD_REQUEST, "task_progress_unavailable")
            };
            (status, Json(json!({"code":code,"error":error.to_string()})))
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code":"task_progress_unavailable","error":error.to_string()})),
        ),
    }
}

fn read(server: &Server, directory: &Path) -> Result<Value, SessionSyncError> {
    // Catalog::load reads the cached authority. Server::catalog/main_diagnostics
    // also schedule fetch activity, which a pure App read must not initiate.
    let catalog = Catalog::load(directory)?;
    let project = server.registered_project(catalog.config.project_id())?;
    if project
        .as_ref()
        .is_some_and(|project| project.common_dir != catalog.common_dir)
    {
        return Err("同一项目 ID 已绑定另一个 clone".into());
    }
    // Reuse the actual producer only when it already exists. In particular,
    // never call project(), Hub::open(), recover_finish_project(), or renew.
    let inputs = {
        let _producer = project
            .as_ref()
            .map(|project| {
                project
                    .producer
                    .lock()
                    .map_err(|_| SessionSyncError::Rejected("项目写入队列锁损坏".into()))
            })
            .transpose()?;
        server.recovery_inputs(&catalog)?
    };
    // The actual producer protects only the physical outbox capture. Git/history
    // proof runs without it; concurrent writes must cause SourceChanged below.
    #[cfg(test)]
    tests::before_proof(&catalog.root);
    let view = ProjectSession::read_task_progress(
        &catalog,
        &inputs,
        (server.clock)()?,
        project.as_ref().map(|project| project.hub.as_ref()),
    )?;
    let after = server.registered_project(catalog.config.project_id())?;
    if match (&project, &after) {
        (None, None) => false,
        (Some(before), Some(after)) => !Arc::ptr_eq(before, after),
        _ => true,
    } {
        return Err(SessionSyncError::SourceChanged(
            "项目在读取期间登记或切换，请刷新任务进度".into(),
        ));
    }
    serde_json::to_value(view).map_err(|error| SessionSyncError::Rejected(error.to_string()))
}

#[cfg(test)]
#[path = "app_read_routes_tests.rs"]
mod tests;
