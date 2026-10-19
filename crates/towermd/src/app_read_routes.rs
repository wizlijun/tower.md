//! Fixed App-only transport for recorded task progress. No registration or activity prelude.
use super::*;
use towermd_core::app_read_model::{TaskProgressState, TaskProgressView};
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
    let (view, _) = read_view(server, directory)?;
    serde_json::to_value(view).map_err(|error| SessionSyncError::Rejected(error.to_string()))
}

fn read_view(
    server: &Server,
    directory: &Path,
) -> Result<(TaskProgressView, Option<Arc<Project>>), SessionSyncError> {
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
    Ok((view, project))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventQuery {
    directory: PathBuf,
    instance: String,
    #[serde(default)]
    after: u64,
}

// Metadata is only an invalidation hint for an unowned cold journal. Its bytes
// never become acceptance, projected state or a changed notification.
type JournalStamp = Option<(u64, std::time::SystemTime)>;

fn journal_stamp(common_dir: &Path) -> Result<JournalStamp, String> {
    match fs::metadata(common_dir.join("towermd/journal.jsonl")) {
        Ok(metadata) => Ok(Some((
            metadata.len(),
            metadata.modified().map_err(|error| error.to_string())?,
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

struct AppEventSource {
    project_id: String,
    common_dir: PathBuf,
    revision: String,
    project: Option<Arc<Project>>,
    session_id: Option<towermd_core::project_session::SnapshotSessionId>,
    stamp: JournalStamp,
    floor: u64,
    head: u64,
}

impl AppEventSource {
    fn capture(server: &Server, directory: &Path) -> Result<Self, String> {
        let catalog = Catalog::load(directory)?;
        let before = journal_stamp(&catalog.common_dir)?;
        let previous = server.registered_project(catalog.config.project_id())?;
        let session_id = previous
            .as_ref()
            .map(|project| {
                project
                    .hub
                    .lock()
                    .map(|session| session.snapshot_session_id())
                    .map_err(|_| "session lock poisoned".to_owned())
            })
            .transpose()?;
        let (view, project) = read_view(server, directory).map_err(|error| error.to_string())?;
        if match (&previous, &project) {
            (None, None) => false,
            (Some(before), Some(after)) => !Arc::ptr_eq(before, after),
            _ => true,
        } {
            return Err("项目实例在建立只读通知期间切换，请重新读取".into());
        }
        if view.state != TaskProgressState::Recorded {
            return Err("尚无当前已核实的 Hub 记录；请重新读取项目".into());
        }
        let common_dir = PathBuf::from(&view.common_dir);
        let stamp = journal_stamp(&common_dir)?;
        if common_dir != catalog.common_dir || (project.is_none() && before != stamp) {
            return Err("任务记录来源在建立只读通知期间变化，请重新读取".into());
        }
        let source = Self {
            project_id: view.project_id,
            common_dir,
            revision: view.revision,
            project,
            session_id,
            stamp,
            floor: view.floor_seq.ok_or("任务记录缺少 Hub floor")?,
            head: view.through_seq.ok_or("任务记录缺少连续水位")?,
        };
        // Reject registration/recovery/retirement races across initial proof.
        source.cursor(server)?;
        Ok(source)
    }

    fn cursor(&self, server: &Server) -> Result<(u64, u64), String> {
        let current = server.registered_project(&self.project_id)?;
        match (&self.project, current) {
            (Some(before), Some(current)) if Arc::ptr_eq(before, &current) => {
                let session = current
                    .hub
                    .lock()
                    .map_err(|_| "session lock poisoned".to_owned())?;
                if self.session_id.as_ref() != Some(&session.snapshot_session_id()) {
                    return Err("项目 Session 已重建，请重新读取".into());
                }
                session.task_progress_cursor(&self.project_id, &self.common_dir)
            }
            (None, None) => {
                if journal_stamp(&self.common_dir)? != self.stamp {
                    return Err("未登记的 Hub 来源已改变，请重新读取".into());
                }
                Ok((self.floor, self.head))
            }
            _ => Err("项目实例已登记、恢复或切换，请重新读取".into()),
        }
    }
}

pub(super) async fn events(
    State(server): State<Arc<Server>>,
    Query(query): Query<EventQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if query.instance != server.endpoint.instance {
        return (StatusCode::CONFLICT, "守护进程实例已改变").into_response();
    }
    let reader = server.clone();
    let source =
        tokio::task::spawn_blocking(move || AppEventSource::capture(&reader, &query.directory))
            .await;
    let source = match source {
        Ok(source) => source,
        Err(error) => Err(error.to_string()),
    };
    upgrade.on_upgrade(move |socket| stream(socket, server, source, query.after))
}

fn validate_cursor(after: u64, floor: u64, head: u64) -> Result<(), String> {
    if after < floor {
        return Err(format!("订阅游标早于 Hub 快照水位 {floor}，请重新读取"));
    }
    if after > head {
        return Err("订阅游标超过 Hub 连续水位，请重新读取".into());
    }
    Ok(())
}

async fn send_frame(socket: &mut WebSocket, frame: Value) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            socket.send(Message::Text(frame.to_string().into()))
        )
        .await,
        Ok(Ok(()))
    )
}

async fn reset(socket: &mut WebSocket, reason: String) {
    let _ = send_frame(socket, json!({"kind":"reset","reason":reason})).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Close(None))).await;
}

async fn stream(
    mut socket: WebSocket,
    server: Arc<Server>,
    source: Result<AppEventSource, String>,
    mut after: u64,
) {
    let source = match source {
        Ok(source) => Arc::new(source),
        Err(error) => {
            reset(&mut socket, error).await;
            return;
        }
    };
    let mut shutdown = server.shutdown.subscribe();
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = shutdown.changed() => return,
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                    _ => {}
                }
            }
            _ = interval.tick() => {
                if *shutdown.borrow() { return; }
                let observed = source.clone();
                let reader = server.clone();
                let cursor = tokio::task::spawn_blocking(move || observed.cursor(&reader)).await;
                let cursor = match cursor {
                    Ok(cursor) => cursor,
                    Err(error) => Err(error.to_string()),
                };
                let (floor, head) = match cursor.and_then(|(floor, head)| {
                    validate_cursor(after, floor, head)?;
                    Ok((floor, head))
                }) {
                    Ok(cursor) => cursor,
                    Err(error) => {
                        reset(&mut socket, error).await;
                        return;
                    }
                };
                if head > after {
                    if !send_frame(&mut socket, json!({
                        "kind":"changed", "through_seq":head,
                        "floor_seq":floor, "revision":source.revision
                    })).await { return; }
                    after = head;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "app_read_routes_tests.rs"]
mod tests;
