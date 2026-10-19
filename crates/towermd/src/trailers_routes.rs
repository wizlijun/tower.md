//! Thin Task merge adapter. Candidates, current authorization, terminal G5,
//! proof and durable facts are owned by the core. No client supplies evidence.
use super::*;
use towermd_core::recovery_outbox::RecoveryOutbox;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrailersRequest {
    pub directory: PathBuf,
    pub task: String,
    pub run: Option<String>,
}

// Entirely derived scheduling state. Session + actual Git/outbox reconstruct all
// authority; deleting this state only schedules another complete observation.
pub(super) struct MainObserverSchedule {
    retry_at_mono: Option<u64>,
    retry_pending: bool,
}
impl Default for MainObserverSchedule {
    fn default() -> Self {
        // Registration must also reconcile an original canonical receipt whose
        // actual merge already made its Run ended before daemon restart.
        Self {
            retry_at_mono: None,
            retry_pending: true,
        }
    }
}
impl MainObserverSchedule {
    pub(super) fn pending(&self) -> bool {
        self.retry_pending
    }
    pub(super) fn admitted(&self, hint: bool, mono_ms: u64) -> bool {
        (hint || self.retry_pending) && self.retry_at_mono.is_none_or(|at| mono_ms >= at)
    }
    fn finished(&mut self, success: bool, mono_ms: u64) {
        self.retry_pending = !success;
        self.retry_at_mono = Some(mono_ms.saturating_add(5_000));
    }
}
pub(super) fn project_hint(project: &Project) -> Result<bool, String> {
    project
        .hub
        .lock()
        .map_err(|_| "main 观察 Session 锁损坏")?
        .has_task_merge_candidates()
}
pub(super) fn finish_observation(
    project: &Project,
    success: bool,
    mono_ms: u64,
) -> Result<(), String> {
    project
        .main_observer
        .lock()
        .map_err(|_| "main 观察调度锁损坏")?
        .finished(success, mono_ms);
    Ok(())
}
// Caller owns this project's producer and explicit same-project outbox. The
// core runs long proof outside Session locks and checks the final exact CAS.
pub(super) fn observe(
    project: &Project,
    catalog: &Catalog,
    host: &str,
    outbox: &mut RecoveryOutbox,
    clock: impl Fn() -> Result<u64, String>,
) -> Result<(), String> {
    if catalog.config.project_id() != project.project_id || catalog.common_dir != project.common_dir
    {
        return Err("main observer project/clone changed".into());
    }
    let pending = project
        .main_observer
        .lock()
        .map_err(|_| "main 观察调度锁损坏")?
        .pending();
    if !pending && !project_hint(project)? {
        return Ok(());
    }
    let observation =
        ProjectSession::observe_task_merges_shared(&project.hub, catalog, host, outbox, clock)?;
    for diagnostic in observation.blocked {
        eprintln!(
            "towermd main merge rejected commit={} task={:?}: {}",
            diagnostic.commit, diagnostic.task, diagnostic.reason
        );
    }
    Ok(())
}
impl Server {
    pub(super) fn observe_task_merges(
        &self,
        project: &Project,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
    ) -> Result<(), String> {
        let observed = observe(project, catalog, &self.endpoint.host, outbox, || {
            (self.clock)()
        });
        finish_observation(project, observed.is_ok(), (self.mono_clock)())?;
        observed
    }
    fn trailers(&self, request: TrailersRequest) -> Result<Value, String> {
        if *self.shutdown.borrow() {
            return Err("守护进程正在停止，不接受 trailers 请求".into());
        }
        let catalog = self.catalog(&request.directory)?;
        let project = self.project(&catalog)?;
        let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
        let mut outbox = self.task_outbox(&catalog)?;
        let inputs = outbox.read_inputs()?;
        Self::reconcile_task_intents(
            &project.hub,
            &catalog,
            &inputs,
            (self.clock)()?,
            &mut outbox,
        )?;
        ProjectSession::task_trailers_shared(
            &project.hub,
            &catalog,
            &self.endpoint.host,
            &request.task,
            request.run.as_deref(),
            &mut outbox,
            || (self.clock)(),
        )
        .and_then(|outcome| serde_json::to_value(outcome).map_err(|error| error.to_string()))
    }
}
pub(super) async fn trailers(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<TrailersRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    match tokio::task::spawn_blocking(move || server.trailers(request)).await {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

#[cfg(test)]
#[path = "trailers_routes_tests.rs"]
mod tests;
