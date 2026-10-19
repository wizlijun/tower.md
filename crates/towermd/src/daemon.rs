use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::finish_jobs::FinishJobs;
use crate::main_refresh::MainRefresh;
use crate::scan_jobs::{Record, Resume, ScanJobs};
use crate::snapshot_schedule::{ClockReading, SnapshotSchedule};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as RoutePath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use towermd_core::canvas_view::CanvasView;
use towermd_core::catalog::Catalog;
use towermd_core::coldstart::{capture_scan_input, ScanOptions};
use towermd_core::hub::{inspect_store, probe_store, Hub, StoreState};
use towermd_core::hub_protocol::HubError;
use towermd_core::project_session::ProjectSession;
use towermd_core::snapshot_service::{self, SnapshotWriter};
use towermd_core::tasks::{self, TaskCommand, TaskFailure};
use uuid::Uuid;

#[path = "app_answer_routes.rs"]
mod app_answer_routes;
#[path = "app_gate.rs"]
mod app_gate;
#[path = "app_read_routes.rs"]
mod app_read_routes;
#[path = "ask_tick.rs"]
mod ask_tick;
#[path = "baseline_routes.rs"]
mod baseline_routes;
#[path = "finish_routes.rs"]
mod finish_routes;
#[path = "human_routes.rs"]
mod human_routes;
#[path = "inbox_routes.rs"]
mod inbox_routes;
#[path = "trailers_routes.rs"]
mod trailers_routes;
pub use finish_routes::FinishRequest;
pub use trailers_routes::TrailersRequest;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineSubmitRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
    pub request_id: String,
    pub input: towermd_core::baseline_schema::BaselineSubmit,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
    pub request_id: String,
    pub input: towermd_core::proposals::ProposalInput,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Endpoint {
    address: String,
    port: u16,
    pid: u32,
    instance: String,
    host: String,
}

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub directory: PathBuf,
    pub run: Option<String>,
    pub command: TaskCommand,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RebuildRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
}

struct RebuildFailure {
    status: StatusCode,
    message: String,
}
impl From<towermd_core::recovery::ConfirmError> for RebuildFailure {
    fn from(error: towermd_core::recovery::ConfirmError) -> Self {
        use towermd_core::recovery::ConfirmError;
        Self {
            status: match &error {
                ConfirmError::Conflict(_) => StatusCode::CONFLICT,
                ConfirmError::ReadOrRefused(_) | ConfirmError::Persisted { .. } => {
                    StatusCode::BAD_REQUEST
                }
            },
            message: error.to_string(),
        }
    }
}
impl From<towermd_core::recovery::RequestSourceError> for RebuildFailure {
    fn from(error: towermd_core::recovery::RequestSourceError) -> Self {
        towermd_core::recovery::ConfirmError::from(error).into()
    }
}
impl From<String> for RebuildFailure {
    fn from(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }
}
impl From<&str> for RebuildFailure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRebuildRequest {
    request: towermd_core::recovery::RecoveryRequest,
    directory: PathBuf,
    common_dir: PathBuf,
    expires_at: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmRebuildBody {
    actor: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColdstartRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
    #[serde(default)]
    pub options: Option<ScanOptions>,
    #[serde(default)]
    pub wait: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadQuery {
    directory: PathBuf,
    run: Option<String>,
}

#[derive(Deserialize)]
struct Subscription {
    directory: PathBuf,
    #[serde(default)]
    after: u64,
    instance: String,
}

struct Project {
    project_id: String,
    finish_registered: AtomicBool,
    finish_recovery_active: AtomicBool,
    finish_retry_after: AtomicU64,
    finish_recovery: AtomicBool,
    finish_maintenance_at: Mutex<Option<ClockReading>>,
    producer: Arc<Mutex<()>>,
    outbox_uncertainty: towermd_core::recovery_outbox::OutboxUncertainty,
    publication_reconcile: AtomicBool,
    common_dir: PathBuf,
    root: Mutex<PathBuf>,
    hub: Arc<Mutex<ProjectSession>>,
    writer: Arc<SnapshotWriter>,
    snapshot_interval: AtomicU64,
    schedule: Mutex<SnapshotSchedule>,
    ask_tick: Mutex<ask_tick::AskTickSchedule>,
    main_observer: Mutex<trailers_routes::MainObserverSchedule>,
    snapshot_context: Mutex<Option<Arc<SnapshotContext>>>,
    #[cfg(test)]
    background_catalog_loads: AtomicU64,
    reconcile: AtomicBool,
    recovery_error: Mutex<Option<String>>,
    background: Mutex<Option<tokio::task::JoinHandle<()>>>,
    _lock: ProjectLock,
}

struct SnapshotContext {
    catalog: Catalog,
    inputs: towermd_core::recovery_outbox::RecoveryInputs,
}
#[cfg(test)]
fn installed_summary(
    project: &Project,
) -> Result<towermd_core::project_session::SnapshotSummary, String> {
    project
        .hub
        .lock()
        .map_err(|_| "项目 hub 锁损坏")?
        .snapshot_summary()
}
struct SnapshotPreparation {
    summary: towermd_core::project_session::SnapshotSummary,
    session: towermd_core::project_session::SnapshotSessionId,
    order: snapshot_service::SnapshotObservationOrder,
}
impl std::ops::Deref for SnapshotPreparation {
    type Target = towermd_core::project_session::SnapshotSummary;
    fn deref(&self) -> &Self::Target {
        &self.summary
    }
}
fn begin_snapshot_preparation(project: &Project) -> Result<SnapshotPreparation, String> {
    let live = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
    Ok(SnapshotPreparation {
        summary: live.snapshot_summary()?,
        session: live.snapshot_session_id(),
        order: project.writer.observation_order()?,
    })
}
// Current Session identity, summary capture and schedule consumption share one
// short Hub -> schedule boundary. Neither lock is held over Git or proof work.
fn consume_snapshot_schedule<T>(
    project: &Project,
    now: ClockReading,
    consume: impl FnOnce(
        &mut SnapshotSchedule,
        &towermd_core::project_session::SnapshotSummary,
        &towermd_core::project_session::SnapshotSessionId,
    ) -> Result<(T, Option<towermd_core::hub_protocol::NewEvent>), String>,
) -> Result<T, String> {
    let mut live = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
    let summary = live.snapshot_summary()?;
    let session = live.snapshot_session_id();
    let interval = project.snapshot_interval.load(Ordering::Relaxed);
    let mut schedule = project.schedule.lock().map_err(|_| "快照调度锁损坏")?;
    if schedule.bind_session(&session) {
        *project
            .snapshot_context
            .lock()
            .map_err(|_| "快照上下文锁损坏")? = None;
        project.reconcile.store(true, Ordering::Relaxed);
    }
    schedule.install(&summary, now, interval);
    let (value, event) = consume(&mut schedule, &summary, &session)?;
    drop(schedule);
    if let Some(event) = event {
        // The same live Hub remains locked, so a retired Session's alert cannot
        // cross activation into its replacement. This is the original append.
        live.protocol()?
            .append(event, now.unix_ms)
            .map_err(|error| error.to_string())?;
    }
    Ok(value)
}
fn accept_snapshot_result(
    project: &Project,
    schedule: &mut SnapshotSchedule,
    preparation: &SnapshotPreparation,
    attempt: Option<&snapshot_service::SnapshotAttempt>,
    session: &towermd_core::project_session::SnapshotSessionId,
) -> bool {
    let source = attempt
        .and_then(snapshot_service::SnapshotAttempt::session_id)
        .unwrap_or(&preparation.session);
    let order = attempt
        .and_then(snapshot_service::SnapshotAttempt::observation_order)
        .unwrap_or(&preparation.order);
    source == session
        && project.writer.observation_is_current(order)
        && schedule.accept_result(order)
}
fn record_snapshot_failure(
    project: &Project,
    preparation: &SnapshotPreparation,
    reason: String,
    attempt: Option<&snapshot_service::SnapshotAttempt>,
    host: &str,
    now: ClockReading,
) -> Result<(), String> {
    let interval = project.snapshot_interval.load(Ordering::Relaxed);
    consume_snapshot_schedule(project, now, |schedule, summary, session| {
        if !accept_snapshot_result(project, schedule, preparation, attempt, session) {
            return Ok(((), None));
        }
        let before = if &preparation.session == session {
            &preparation.summary
        } else {
            summary
        };
        schedule.failed(reason, before, attempt, now, interval);
        Ok(((), schedule.alert_event(host, interval)))
    })
}
fn finish_snapshot_attempt(
    project: &Project,
    preparation: &SnapshotPreparation,
    attempt: &snapshot_service::SnapshotAttempt,
    host: &str,
    now: ClockReading,
) -> Result<(), String> {
    let interval = project.snapshot_interval.load(Ordering::Relaxed);
    consume_snapshot_schedule(project, now, |schedule, summary, session| {
        if !accept_snapshot_result(project, schedule, preparation, Some(attempt), session) {
            return Ok(((), None));
        }
        if let Some(completed) = attempt.completed() {
            schedule.written(
                (&completed.last).into(),
                summary.fast_pending.as_ref().map(|pending| pending.max_seq),
            );
            schedule.install(summary, now, interval);
            project.reconcile.store(false, Ordering::Relaxed);
            return Ok(((), None));
        }
        let failure = match attempt.result() {
            Err(error) => Some(error.clone()),
            Ok(report) if report.status == "deferred" || report.status == "superseded" => Some(
                report
                    .reason
                    .clone()
                    .unwrap_or_else(|| report.status.clone()),
            ),
            _ => None,
        };
        if let Some(reason) = failure {
            let before = if &preparation.session == session {
                &preparation.summary
            } else {
                summary
            };
            schedule.failed(reason, before, Some(attempt), now, interval);
            Ok(((), schedule.alert_event(host, interval)))
        } else {
            if schedule.next_attempt_at().is_none() {
                project.reconcile.store(false, Ordering::Relaxed);
            }
            Ok(((), None))
        }
    })
}

#[cfg(test)]
fn trace_i2_phase(root: &Path, phase: &str) {
    if std::env::var_os("TOWERMD_I2_CHILD").is_some() {
        eprintln!(
            "I2_ACTUAL_PHASE unix_ms={} root={} phase={phase}",
            now_ms().unwrap_or(0),
            root.display()
        );
    }
}

struct Server {
    finish_jobs: Arc<FinishJobs>,
    scan_jobs: Arc<ScanJobs>,
    source_status_cache: towermd_core::recovery::SourceStatusCache,
    main_refresh: MainRefresh,
    clock: Arc<dyn Fn() -> Result<u64, String> + Send + Sync>,
    mono_clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    endpoint: Endpoint,
    runtime: PathBuf,
    projects: Mutex<BTreeMap<String, Arc<Project>>>,
    registration: Mutex<()>,
    delivery_tokens: Mutex<inbox_routes::DeliveryTokens>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

fn monotonic_clock() -> Arc<dyn Fn() -> u64 + Send + Sync> {
    let origin = Instant::now();
    Arc::new(move || u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX))
}

fn runtime_dir() -> Result<PathBuf, String> {
    let directory = match std::env::var_os("TOWERMD_RUNTIME_DIR") {
        Some(directory) => PathBuf::from(directory),
        None => dirs::data_local_dir()
            .ok_or("无法定位用户运行目录")?
            .join("towermd/runtime"),
    };
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    fs::canonicalize(directory).map_err(|error| error.to_string())
}

fn lock_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn wait_lock(file: &File) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match FileExt::try_lock_exclusive(file) {
            Ok(()) => return Ok(()),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(format!("无法取得守护进程启动锁：{error}")),
        }
    }
}

#[derive(Debug)]
struct ProjectLock(File);
impl Drop for ProjectLock {
    fn drop(&mut self) {
        // Closing our descriptor alone leaves a forked/duplicated descriptor's
        // flock alive. The registered project owns this lock's lifetime.
        let _ = FileExt::unlock(&self.0);
    }
}

fn bind_project(
    runtime: &Path,
    project_id: &str,
    common_dir: &Path,
) -> Result<ProjectLock, String> {
    let lock_name = hex::encode(Sha256::digest(project_id.as_bytes()));
    let project_lock = lock_file(&runtime.join(format!("project-{lock_name}.lock")))?;
    FileExt::try_lock_exclusive(&project_lock)
        .map_err(|error| format!("项目已被其他 clone 占用：{error}"))?;
    let project_lock = ProjectLock(project_lock);
    let owner_path = runtime.join(format!("project-{lock_name}.json"));
    match fs::read(&owner_path) {
        Ok(bytes) => validate_project_owner(&bytes, common_dir)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_json(&owner_path, &common_dir)?;
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(project_lock)
}

fn validate_project_owner(bytes: &[u8], common_dir: &Path) -> Result<(), String> {
    let owner: PathBuf =
        serde_json::from_slice(bytes).map_err(|error| format!("项目 clone 绑定损坏：{error}"))?;
    if owner != common_dir {
        return Err("同一项目 ID 已绑定另一个 clone；请使用 git worktree 或云 hub".into());
    }
    Ok(())
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(timeout)
        .build()
        .map_err(|error| error.to_string())
}

fn url(endpoint: &Endpoint, path: &str) -> String {
    format!("http://127.0.0.1:{}{path}", endpoint.port)
}

fn live_endpoint(runtime: &Path) -> Option<Endpoint> {
    let endpoint: Endpoint =
        serde_json::from_slice(&fs::read(runtime.join("endpoint.json")).ok()?).ok()?;
    if endpoint.address != "127.0.0.1" || endpoint.port == 0 {
        return None;
    }
    let response: Endpoint = client(Duration::from_millis(300))
        .ok()?
        .get(url(&endpoint, "/health"))
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()?;
    (response.instance == endpoint.instance && response.pid == endpoint.pid).then_some(endpoint)
}

#[cfg(unix)]
fn prepare_daemon_spawn(command: &mut Command) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    extern "C" {
        fn getdtablesize() -> i32;
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    const F_GETFD: i32 = 1;
    const F_SETFD: i32 = 2;
    const FD_CLOEXEC: i32 = 1;
    const EBADF: i32 = 9;
    // Query the limit in the parent; only the forked child's flags are changed.
    let limit = unsafe { getdtablesize() };
    if limit <= 0 {
        return Err(format!(
            "无法确定守护进程文件描述符上限：{}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: after fork use only async-signal-safe fcntl and errno reads, without
    // allocations or locks. Mark, do not close: Rust's exec-error pipe must remain
    // writable until exec fails or successful exec closes it through CLOEXEC.
    // Command already redirected stdio; descriptors 0..=2 remain untouched.
    unsafe {
        command.pre_exec(move || {
            for fd in 3..limit {
                let flags = fcntl(fd, F_GETFD);
                if flags == -1 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(EBADF) {
                        continue;
                    }
                    return Err(error);
                }
                if fcntl(fd, F_SETFD, flags | FD_CLOEXEC) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    Ok(())
}

fn connect(start: bool) -> Result<Option<Endpoint>, String> {
    let runtime = runtime_dir()?;
    let startup = lock_file(&runtime.join("startup.lock"))?;
    wait_lock(&startup)?;
    if let Some(endpoint) = live_endpoint(&runtime) {
        return Ok(Some(endpoint));
    }
    if !start {
        return Ok(None);
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(runtime.join("daemon.log"))
        .map_err(|error| error.to_string())?;
    let mut daemon_command =
        Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    daemon_command
        .args(["daemon", "serve"])
        .env("TOWERMD_RUNTIME_DIR", &runtime)
        .current_dir(&runtime)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        daemon_command.process_group(0);
        prepare_daemon_spawn(&mut daemon_command)?;
    }
    let mut child = daemon_command
        .spawn()
        .map_err(|error| format!("无法启动守护进程：{error}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(endpoint) = live_endpoint(&runtime) {
            return Ok(Some(endpoint));
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!(
                "守护进程启动失败（{status}），请检查 {}",
                runtime.join("daemon.log").display()
            ));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "守护进程启动超时，请检查 {}",
                runtime.join("daemon.log").display()
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub fn request(request: Request) -> Result<Value, String> {
    post_request("/command", &request)
}

pub fn check_request(request: CheckRequest) -> Result<Value, String> {
    post_request("/check", &request)
}

pub fn trailers_request(request: TrailersRequest) -> Result<String, String> {
    let result = post_request("/trailers", &request)?;
    result["trailers"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "trailers response lacks the core renderer output".into())
}

pub fn finish_request(request: FinishRequest) -> Result<Value, String> {
    post_request_with_timeout("/finish", &request, Duration::from_secs(100))
}

pub fn request_task_review(
    directory: PathBuf,
    run: Option<String>,
    request_id: String,
    command: towermd_core::human_intervention::HumanCommand,
) -> Result<Value, String> {
    request_interaction(directory, run, request_id, command)
}

pub fn request_interaction(
    directory: PathBuf,
    run: Option<String>,
    request_id: String,
    command: towermd_core::human_intervention::HumanCommand,
) -> Result<Value, String> {
    post_request(
        "/interaction",
        &human_routes::InteractionRequest {
            directory,
            run,
            request_id,
            command,
        },
    )
}

pub fn snapshot_request(request: SnapshotRequest) -> Result<Value, String> {
    post_request("/snapshot", &request)
}

pub fn rebuild_request(request: RebuildRequest) -> Result<Value, String> {
    post_request("/rebuild-request", &request)
}

pub fn coldstart_request(request: ColdstartRequest) -> Result<Value, String> {
    post_request_with_timeout("/baseline/coldstart", &request, Duration::from_secs(100))
}

pub fn baseline_submit_request(request: BaselineSubmitRequest) -> Result<Value, String> {
    post_request_with_timeout("/baseline/submit", &request, Duration::from_secs(100))
}

pub fn propose_request(request: ProposeRequest) -> Result<Value, String> {
    post_request_with_timeout("/proposals", &request, Duration::from_secs(100))
}

fn post_request(path: &str, request: &impl Serialize) -> Result<Value, String> {
    post_request_with_timeout(path, request, Duration::from_secs(120))
}

fn post_request_with_timeout(
    path: &str,
    request: &impl Serialize,
    timeout: Duration,
) -> Result<Value, String> {
    let endpoint = connect(true)?.ok_or("守护进程未启动")?;
    let response = client(timeout)?
        .post(url(&endpoint, path))
        .header("x-towermd-instance", &endpoint.instance)
        .json(&request)
        .send()
        .map_err(|error| format!("守护进程请求失败（不自动重试写入）：{error}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .map_err(|error| format!("守护进程响应无效：{error}"))?;
    if !status.is_success() {
        return Err(body["error"].as_str().unwrap_or("守护进程请求失败").into());
    }
    Ok(body)
}

pub fn renew_bound(run: Option<String>) -> Result<(), String> {
    let directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let root = match towermd_core::repo::toplevel(&directory) {
        Ok(root) => root,
        Err(_) if run.is_none() => return Ok(()),
        Err(error) => return Err(error),
    };
    let common = towermd_core::repo::git(&root, &["rev-parse", "--git-common-dir"])?;
    if !root.join(common).join("towermd").is_dir() && run.is_none() {
        return Ok(());
    }
    if run.is_none() && connect(false)?.is_none() {
        return Ok(());
    }
    request(Request {
        directory,
        run,
        command: TaskCommand::Status,
    })
    .map(|_| ())
}

pub fn ensure() -> Result<Value, String> {
    let endpoint = connect(true)?.ok_or("守护进程未启动")?;
    serde_json::to_value(endpoint).map_err(|error| error.to_string())
}

pub fn status() -> Result<Value, String> {
    match connect(false)? {
        Some(endpoint) => serde_json::to_value(endpoint).map_err(|error| error.to_string()),
        None => Ok(json!({"running": false})),
    }
}

pub fn stop() -> Result<Value, String> {
    let runtime = runtime_dir()?;
    if let Some(endpoint) = connect(false)? {
        client(Duration::from_secs(5))?
            .post(url(&endpoint, "/shutdown"))
            .header("x-towermd-instance", &endpoint.instance)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| error.to_string())?;
        let daemon_lock = lock_file(&runtime.join("daemon.lock"))?;
        wait_lock(&daemon_lock).map_err(|error| {
            format!("守护进程仍在停止，快照写入结果不确定；未终止在途 Git，请稍后检查：{error}")
        })?;
    }
    Ok(json!({"running": false}))
}

fn now_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis()
        .try_into()
        .map_err(|_| "时钟溢出".into())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", Uuid::now_v7()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        File::open(path.parent().ok_or("运行文件缺少父目录")?)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("运行文件已替换，但目录同步失败：{error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn start_ticker(
    server: Arc<Server>,
) -> (
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    let tick_server = server;
    let (ticker_stop, mut tick_shutdown) = tokio::sync::watch::channel(false);
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _=tick_shutdown.changed()=>{if *tick_shutdown.borrow(){break;}},
                _=interval.tick()=>{
                    if *tick_shutdown.borrow(){break;}
                    let server=tick_server.clone();
                    let result=tokio::task::spawn_blocking(move||server.tick()).await;
                    if let Err(error)=result.unwrap_or_else(|error|Err(error.to_string())) {eprintln!("towermd 租约到期检查失败：{error}");}
                }
            }
        }
    });
    (ticker_stop, ticker)
}

pub fn serve() -> Result<(), String> {
    let runtime = runtime_dir()?;
    let daemon_lock = lock_file(&runtime.join("daemon.lock"))?;
    FileExt::try_lock_exclusive(&daemon_lock)
        .map_err(|error| format!("守护进程已启动或无法加锁：{error}"))?;
    let host_path = runtime.join("host.json");
    let host: String = if host_path.exists() {
        serde_json::from_slice(&fs::read(&host_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("主机 ID 无效：{error}"))?
    } else {
        let host = Uuid::now_v7().to_string();
        write_json(&host_path, &host)?;
        host
    };
    if host.trim().is_empty() {
        return Err("主机 ID 为空".into());
    }
    let daemon_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let result = daemon_runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| error.to_string())?;
        let endpoint = Endpoint {
            address: "127.0.0.1".into(),
            port: listener
                .local_addr()
                .map_err(|error| error.to_string())?
                .port(),
            pid: std::process::id(),
            instance: Uuid::now_v7().to_string(),
            host,
        };
        let (shutdown_sender, mut receiver) = tokio::sync::watch::channel(false);
        let server = Arc::new(Server {
            finish_jobs: Arc::new(FinishJobs::default()),
            scan_jobs: ScanJobs::new(&runtime)?,
            main_refresh: MainRefresh::default(),
            source_status_cache: Default::default(),
            mono_clock: monotonic_clock(),
            clock: Arc::new(now_ms),
            endpoint,
            runtime: runtime.clone(),
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
            delivery_tokens: Mutex::new(inbox_routes::DeliveryTokens::default()),
            shutdown: shutdown_sender,
        });
        let (ticker_stop, ticker) = start_ticker(server.clone());
        let app = router(server.clone());
        write_json(&runtime.join("endpoint.json"), &server.endpoint)?;
        let closing_scans = server.scan_jobs.clone();
        let closing_finishes = server.finish_jobs.clone();
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                while !*receiver.borrow() {
                    if receiver.changed().await.is_err() {
                        break;
                    }
                }
                // Cancel commands before Axum waits for outstanding scan waiters.
                let _ = closing_scans.cancel_all();
                let _ = closing_finishes.cancel_all();
            })
            .await
            .map_err(|error| error.to_string());
        let _ = server.shutdown.send(true);
        let scans = server.scan_jobs.shutdown().await;
        let finishes = server.finish_jobs.shutdown().await;
        // Keep system leases alive while existing Git writers drain; shutdown prevents new work.
        let fetched = server.main_refresh.drain().await;
        let drained = server.drain_snapshots().await;
        let _ = ticker_stop.send(true);
        let ticked = ticker
            .await
            .map_err(|error| format!("项目维护退出失败：{error}"));
        result
            .and(scans)
            .and(finishes)
            .and(fetched)
            .and(drained)
            .and(ticked)
    });
    // Runtime waits for all blocking work while the daemon/file ownership is still held.
    drop(daemon_runtime);
    drop(daemon_lock);
    result
}

fn router(server: Arc<Server>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/app/projection", get(app_read_routes::projection))
        .route("/app/events", get(app_read_routes::events))
        .route("/command", post(command))
        .route("/inbox", post(inbox_routes::inbox))
        .route("/inbox/delivered", post(inbox_routes::delivered))
        .route("/check", post(check))
        .route("/trailers", post(trailers_routes::trailers))
        .route(
            "/finish",
            post(finish_routes::finish).layer(axum::extract::DefaultBodyLimit::max(
                6 * 1024 * 1024 + 16 * 1024,
            )),
        )
        .route("/interaction", post(human_routes::interaction))
        .route(
            "/app/interventions/reviews/{hold}",
            get(human_routes::review),
        )
        .route("/app/interventions", get(human_routes::interventions))
        .route("/app/interventions/action", post(human_routes::action))
        .route(
            "/app/interventions/asks/{hold}",
            get(app_answer_routes::load),
        )
        .route("/snapshot", post(snapshot))
        .route("/rebuild-request", post(rebuild_request_handler))
        .route("/hub/rebuild/{request_id}", get(saved_rebuild_request))
        .route("/hub/rebuild/{request_id}/confirm", post(confirm_rebuild))
        .route("/baseline/coldstart", post(coldstart))
        .route("/baseline/schema", get(baseline_routes::schema))
        .route("/baseline/submit", post(baseline_routes::submit))
        .route("/proposals", post(baseline_routes::propose))
        .route("/drafts", post(baseline_routes::edit))
        .route("/app/drafts/{id}", get(baseline_routes::saved))
        .route("/app/baselines/current", get(baseline_routes::current))
        .route("/app/baselines/{id}/review", get(baseline_routes::review))
        .route("/app/baselines/{id}/action", post(baseline_routes::action))
        .route("/app/requests/{id}", get(baseline_routes::request_status))
        .route(
            "/app/requests/{id}/retry",
            post(baseline_routes::retry_request),
        )
        .route("/drafts/{id}", get(coldstart_draft))
        .route("/canvas", get(canvas))
        .route("/objects/{id}", get(object))
        .route("/events", get(subscribe))
        .route("/shutdown", post(shutdown))
        .with_state(server)
}

fn validated_catalog(
    hub: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &towermd_core::recovery_outbox::RecoveryInputs,
    now: u64,
) -> Result<Option<Catalog>, String> {
    ProjectSession::sync_shared(hub, catalog, inputs, now).map_err(|e| e.to_string())?;
    Ok(None)
}

fn current_scan_catalog(accepted: &Catalog) -> Result<Catalog, String> {
    use towermd_core::recovery_outbox::RecoveryContext;
    let current = Catalog::load(&accepted.root)?;
    if current.common_dir != accepted.common_dir
        || RecoveryContext::from_catalog(accepted)? != RecoveryContext::from_catalog(&current)?
    {
        return Err("扫描期间项目或 hub 配置改变，须重新规划".into());
    }
    Ok(current)
}

impl Server {
    fn prepare_coldstart_context(
        &self,
        directory: &Path,
        run: Option<&str>,
    ) -> Result<Arc<Catalog>, String> {
        if *self.shutdown.borrow() {
            return Err("守护进程正在停止，不接受扫描请求".into());
        }
        let catalog = Arc::new(self.catalog(directory)?);
        // Reject an explicit invalid run before creating a project hub.
        self.renew_existing_read(&catalog, run)?;
        Ok(catalog)
    }

    fn coldstart_callbacks(
        &self,
        catalog: Arc<Catalog>,
        project: Arc<Project>,
    ) -> (Record, Resume) {
        use towermd_core::recovery_outbox::{RecoveryContext, RecoveryOutbox};
        let directory = self
            .runtime
            .join("recovery-outbox")
            .join(catalog.config.project_id());
        let record_catalog = catalog.clone();
        let record_project = project.clone();
        let record_directory = directory.clone();
        let clock = self.clock.clone();
        let record: Record = Arc::new(move |frozen| {
            let _producer = record_project
                .producer
                .lock()
                .map_err(|_| "项目写入队列锁损坏")?;
            let catalog = current_scan_catalog(&record_catalog)?;
            let mut outbox = RecoveryOutbox::open_with_uncertainty(
                &record_directory,
                RecoveryContext::from_catalog(&catalog)?,
                record_project.outbox_uncertainty.clone(),
            )?;
            ProjectSession::record_coldstart_shared(
                &record_project.hub,
                &catalog,
                &mut outbox,
                frozen,
                clock()?,
            )
        });
        let clock = self.clock.clone();
        let resume: Resume = Arc::new(move |scan_id| {
            let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
            let catalog = current_scan_catalog(&catalog)?;
            let mut outbox = RecoveryOutbox::open_with_uncertainty(
                &directory,
                RecoveryContext::from_catalog(&catalog)?,
                project.outbox_uncertainty.clone(),
            )?;
            ProjectSession::resume_coldstart_shared(
                &project.hub,
                &catalog,
                &mut outbox,
                scan_id,
                clock()?,
            )
        });
        (record, resume)
    }

    fn coldstart_resume(self: &Arc<Self>, catalog: Arc<Catalog>) -> Resume {
        let server = self.clone();
        Arc::new(move |scan_id| {
            let catalog = Arc::new(current_scan_catalog(&catalog)?);
            let inputs = server.recovery_inputs(&catalog)?;
            if !towermd_core::coldstart_evidence::recording_exists(&catalog, &inputs, scan_id)? {
                return Ok(None);
            }
            let project = server.project(&catalog)?;
            let (_, resume) = server.coldstart_callbacks(catalog.clone(), project);
            resume(scan_id)
        })
    }

    fn canvas(&self, query: ReadQuery) -> Result<Value, String> {
        let catalog = self.catalog(&query.directory)?;
        let view = CanvasView::load(&catalog)?;
        self.finish_read(&catalog, query.run.as_deref(), view)
    }

    fn object(&self, query: ReadQuery, id: &str) -> Result<Option<Value>, String> {
        let catalog = self.catalog(&query.directory)?;
        let Some(document) = catalog.objects.get(id) else {
            return Ok(None);
        };
        let value = json!({
            "project_id":catalog.config.project_id(), "revision":catalog.revision,
            "id":id, "path":catalog.object_paths.get(id),
            "type":document.get_str("type"), "text":document.to_text(),
        });
        self.finish_read(&catalog, query.run.as_deref(), value)
            .map(Some)
    }

    fn finish_read(
        &self,
        catalog: &Catalog,
        run: Option<&str>,
        mut value: Value,
    ) -> Result<Value, String> {
        if let Err(error) = self.renew_existing_read(catalog, run) {
            if run.is_some() {
                return Err(error);
            }
            value["warnings"] = json!([format!(
                "已有 hub 无法确认隐式续租；仅返回读取结果：{error}"
            )]);
        }
        self.main_diagnostics(&catalog.root, value)
    }

    fn renew_existing_read(&self, catalog: &Catalog, run: Option<&str>) -> Result<(), String> {
        if !probe_store(&catalog.common_dir.join("towermd"))? {
            return if run.is_none() {
                Ok(())
            } else {
                Err(format!(
                    "run {} 无法续租：hub journal 不存在",
                    run.unwrap_or("隐式绑定")
                ))
            };
        }
        let mut project = self.registered_project(catalog.config.project_id())?;
        // A temporary read session must not overlap a cold registration of the same hub.
        let registration = if project.is_none() {
            let guard = self.registration.lock().map_err(|_| "项目注册锁损坏")?;
            project = self.registered_project(catalog.config.project_id())?;
            Some(guard)
        } else {
            None
        };
        if let Some(project) = project {
            drop(registration);
            if project.common_dir != catalog.common_dir {
                return Err("项目 ID 已绑定另一个 clone".into());
            }
            let hub = project.hub.clone();
            self.renew_read_hub(&hub, catalog, run)
        } else {
            let lock_name = hex::encode(Sha256::digest(catalog.config.project_id().as_bytes()));
            let owner = fs::read(self.runtime.join(format!("project-{lock_name}.json")))
                .map_err(|error| format!("无法确认已有项目 clone 绑定，不续租：{error}"))?;
            validate_project_owner(&owner, &catalog.common_dir)?;
            let hub = Mutex::new(
                ProjectSession::new(
                    Box::new(Hub::open(&catalog.common_dir.join("towermd"))?),
                    catalog,
                    &self.recovery_inputs(catalog)?,
                    (self.clock)()?,
                )
                .map_err(|e| e.to_string())?,
            );
            self.renew_read_hub(&hub, catalog, run)
        }
    }

    fn renew_read_hub(
        &self,
        hub: &Mutex<ProjectSession>,
        catalog: &Catalog,
        run: Option<&str>,
    ) -> Result<(), String> {
        tasks::renew_binding(
            &mut *hub.lock().map_err(|_| "项目 hub 锁损坏")?,
            catalog,
            &self.endpoint.host,
            run,
            (self.clock)()?,
        )
    }

    fn task_outbox(
        &self,
        catalog: &Catalog,
    ) -> Result<towermd_core::recovery_outbox::RecoveryOutbox, String> {
        let uncertainty = self
            .registered_project(catalog.config.project_id())?
            .map(|project| project.outbox_uncertainty.clone())
            .unwrap_or_default();
        towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(
            &self
                .runtime
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            towermd_core::recovery_outbox::RecoveryContext::from_catalog(catalog)?,
            uncertainty,
        )
    }
    fn reconcile_task_intents(
        hub: &Mutex<ProjectSession>,
        catalog: &Catalog,
        inputs: &towermd_core::recovery_outbox::RecoveryInputs,
        now: u64,
        outbox: &mut towermd_core::recovery_outbox::RecoveryOutbox,
    ) -> Result<(), String> {
        if ProjectSession::task_reconciliation_needed_shared(hub, catalog, inputs)? {
            ProjectSession::reconcile_pending_shared(hub, catalog, inputs, now, outbox)?;
        }
        Ok(())
    }
    fn recovery_inputs(
        &self,
        catalog: &Catalog,
    ) -> Result<towermd_core::recovery_outbox::RecoveryInputs, String> {
        let uncertainty = self
            .registered_project(catalog.config.project_id())?
            .filter(|project| project.common_dir == catalog.common_dir)
            .map(|project| project.outbox_uncertainty.clone())
            .unwrap_or_default();
        towermd_core::recovery_outbox::RecoveryInputs::read_with_uncertainty(
            &self
                .runtime
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            catalog,
            uncertainty,
        )
    }
    fn catalog(&self, directory: &Path) -> Result<Catalog, String> {
        self.main_refresh.observe(directory)?;
        Catalog::load(directory)
    }

    fn main_diagnostics(&self, directory: &Path, mut value: Value) -> Result<Value, String> {
        let main = self.main_refresh.observe(directory)?;
        if let Some(error) = &main.last_fetch_error {
            let warning = json!(format!(
                "后台 fetch 失败：{error}；已有缓存时继续读取与续租"
            ));
            let mut warnings = value
                .get("warnings")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
            value["warnings"] = json!(warnings);
        }
        value["main"] = serde_json::to_value(main).map_err(|error| error.to_string())?;
        Ok(value)
    }

    fn check(&self, request: CheckRequest) -> Result<Value, String> {
        let catalog = self.catalog(&request.directory)?;
        let project = self.project(&catalog)?;
        // CLI activity renews only its actual binding. This path does not
        // expire Tasks, reconcile producer intents, or append business facts.
        self.renew_read_hub(&project.hub, &catalog, request.run.as_deref())?;
        let inputs = self.recovery_inputs(&catalog)?;
        let report = towermd_core::task_gates::check_shared(
            &project.hub,
            &catalog,
            &inputs,
            &self.endpoint.host,
            request.run.as_deref(),
            || (self.clock)(),
        )
        .map_err(|error| format!("check rejected: {error:?}"))?;
        // RawChange paths/status retain the core's exact byte representation.
        // A successful check does not run verify or certify G3/Evidence.
        let changed = report
            .changed
            .iter()
            .map(|change| {
                json!({
                    "old_mode":change.old_mode,"new_mode":change.new_mode,
                    "old_oid":change.old_oid,"new_oid":change.new_oid,
                    "status":change.status,"path":change.path,
                })
            })
            .collect::<Vec<_>>();
        Ok(
            json!({"commit":report.commit,"tree":report.tree,"base":report.base,
            "gates":report.gates,"changed":changed}),
        )
    }

    fn execute(&self, request: Request) -> Result<Value, TaskFailure> {
        self.main_refresh.observe(&request.directory)?;
        let catalog = match Catalog::load_optional(&request.directory)? {
            Some(catalog) => catalog,
            None if matches!(&request.command, TaskCommand::Status) => {
                return self.main_diagnostics(&request.directory, json!({
                "status": "authority_unavailable",
                "snapshot": {"last":null,"deferred_reason":null,"deferred_since":null},
                "warnings": ["权威 main 不可读；请先将 init 结果提交到 main，有 origin 时须首次成功 fetch origin/main；未读取工作树或任务分支，也未续租"]
            })).map_err(Into::into);
            }
            None => return Err(
                "权威 main 不可读；须完成初始化提交及首次成功 fetch，不能据工作树或任务分支执行"
                    .into(),
            ),
        };
        if matches!(request.command, TaskCommand::Status) {
            let project = self.registered_project(catalog.config.project_id())?;
            let failure = project
                .as_ref()
                .map(|project| {
                    if project.common_dir != catalog.common_dir {
                        return Err("项目 ID 已绑定另一个 clone".to_string());
                    }
                    project
                        .recovery_error
                        .lock()
                        .map_err(|_| "恢复诊断锁损坏".to_string())
                        .map(|error| error.clone())
                })
                .transpose()?
                .flatten();
            if failure.is_some()
                || inspect_store(&catalog.common_dir.join("towermd"))? != StoreState::Nonempty
            {
                if let Some(mut value) = self
                    .source_status_cache
                    .status_source(&catalog, &self.recovery_inputs(&catalog)?)?
                {
                    if let Some(error) = failure {
                        value["error"] = json!(error);
                    }
                    return self
                        .main_diagnostics(&request.directory, value)
                        .map_err(Into::into);
                }
            }
        }
        let status_requested = matches!(request.command, TaskCommand::Status);
        let hub = self.hub(&catalog)?;
        let status_time;
        let mut value = if status_requested {
            let inputs = self.recovery_inputs(&catalog)?;
            // CLI activity renews only actual complete fences, independently of the pure view.
            self.renew_read_hub(&hub, &catalog, request.run.as_deref())?;
            status_time = (self.clock)()?;
            ProjectSession::read_status_shared(
                &hub,
                &catalog,
                &inputs,
                &self.endpoint.host,
                request.run.as_deref(),
                status_time,
            )?
        } else {
            let project = self.project(&catalog)?;
            let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
            let mut outbox = self.task_outbox(&catalog)?;
            let inputs = outbox.read_inputs()?;
            let preparation_time = (self.clock)()?;
            Self::reconcile_task_intents(&hub, &catalog, &inputs, preparation_time, &mut outbox)?;
            self.observe_task_merges(&project, &catalog, &mut outbox)?;
            let inputs = outbox.read_inputs()?;
            status_time = (self.clock)()?;
            ProjectSession::execute_shared(
                &hub,
                &catalog,
                &inputs,
                &self.endpoint.host,
                request.command,
                request.run.as_deref(),
                status_time,
                &mut outbox,
            )?
        };
        if status_requested {
            #[cfg(test)]
            tests::review_after_status_summary(&catalog.root);
            if let Some(project) = self.registered_project(catalog.config.project_id())? {
                value["snapshot"] = consume_snapshot_schedule(
                    &project,
                    ClockReading {
                        mono_ms: (self.mono_clock)(),
                        unix_ms: status_time,
                    },
                    |schedule, _, _| Ok((schedule.status(), None)),
                )?;
            }
        }
        self.main_diagnostics(&request.directory, value)
            .map_err(Into::into)
    }

    fn hub(&self, catalog: &Catalog) -> Result<Arc<Mutex<ProjectSession>>, String> {
        Ok(self.project(catalog)?.hub.clone())
    }

    fn registered_project(&self, project_id: &str) -> Result<Option<Arc<Project>>, String> {
        Ok(self
            .projects
            .lock()
            .map_err(|_| "项目注册表锁损坏")?
            .get(project_id)
            .cloned())
    }

    fn project(&self, catalog: &Catalog) -> Result<Arc<Project>, String> {
        let snapshot_interval = snapshot_service::interval_ms(catalog)?;
        let project_id = catalog.config.project_id();
        let existing = self.registered_project(project_id)?;
        let project = match existing {
            Some(project) => project,
            None => {
                // Serialize cold opens without holding up registered projects or the ticker.
                let _registration = self.registration.lock().map_err(|_| "项目注册锁损坏")?;
                let existing = self.registered_project(project_id)?;
                if let Some(project) = existing {
                    project
                } else {
                    let project_lock =
                        bind_project(&self.runtime, project_id, &catalog.common_dir)?;
                    let protocol = Box::new(Hub::open(&catalog.common_dir.join("towermd"))?);
                    let inputs = self.recovery_inputs(catalog)?;
                    let time = (self.clock)()?;
                    let hub = Mutex::new(ProjectSession::new(protocol, catalog, &inputs, time)?);
                    let mut schedule = SnapshotSchedule::default();
                    schedule.bind_session(
                        &hub.lock()
                            .map_err(|_| "项目 hub 锁损坏")?
                            .snapshot_session_id(),
                    );
                    schedule.install(
                        &hub.lock()
                            .map_err(|_| "项目 hub 锁损坏")?
                            .snapshot_summary()?,
                        ClockReading {
                            mono_ms: (self.mono_clock)(),
                            unix_ms: time,
                        },
                        snapshot_interval,
                    );
                    schedule.prepare_required();
                    let project = Arc::new(Project {
                        project_id: project_id.to_owned(),
                        finish_registered: AtomicBool::new(false),
                        finish_recovery_active: AtomicBool::new(false),
                        finish_retry_after: AtomicU64::new(0),
                        finish_recovery: AtomicBool::new(true),
                        finish_maintenance_at: Mutex::new(None),
                        producer: Arc::new(Mutex::new(())),
                        outbox_uncertainty: inputs.outbox_uncertainty(),
                        publication_reconcile: AtomicBool::new(true),
                        common_dir: catalog.common_dir.clone(),
                        root: Mutex::new(catalog.root.clone()),
                        hub: Arc::new(hub),
                        writer: Arc::new(SnapshotWriter::default()),
                        snapshot_interval: AtomicU64::new(snapshot_interval),
                        schedule: Mutex::new(schedule),
                        ask_tick: Mutex::new(ask_tick::AskTickSchedule::default()),
                        main_observer: Mutex::new(trailers_routes::MainObserverSchedule::default()),
                        snapshot_context: Mutex::new(None),
                        #[cfg(test)]
                        background_catalog_loads: AtomicU64::new(0),
                        reconcile: AtomicBool::new(true),
                        recovery_error: Mutex::new(None),
                        background: Mutex::new(None),
                        _lock: project_lock,
                    });
                    self.projects
                        .lock()
                        .map_err(|_| "项目注册表锁损坏")?
                        .insert(project_id.into(), project.clone());
                    project
                }
            }
        };
        if project.common_dir != catalog.common_dir {
            return Err("同一项目 ID 已由另一个 clone 占用；请使用 git worktree 或云 hub".into());
        }
        *project.root.lock().map_err(|_| "项目路径锁损坏")? = catalog.root.clone();
        project
            .snapshot_interval
            .store(snapshot_interval, Ordering::Relaxed);
        self.recover_finish_project(project.clone(), catalog)?;
        Ok(project)
    }

    fn snapshot(&self, request: SnapshotRequest) -> Result<Value, String> {
        if *self.shutdown.borrow() {
            return Err("守护进程正在停止，不接受新的快照".into());
        }
        let mut catalog = self.catalog(&request.directory)?;
        let project = self.project(&catalog)?;
        let before = begin_snapshot_preparation(&project)?;
        let prepared =
            (|| -> Result<towermd_core::snapshot_service::PreparedSnapshotWrite, String> {
                if let Some(current) = validated_catalog(
                    &project.hub,
                    &catalog,
                    &self.recovery_inputs(&catalog)?,
                    (self.clock)()?,
                )? {
                    catalog = current;
                }
                self.renew_read_hub(&project.hub, &catalog, request.run.as_deref())?;
                ProjectSession::read_status_shared(
                    &project.hub,
                    &catalog,
                    &self.recovery_inputs(&catalog)?,
                    &self.endpoint.host,
                    request.run.as_deref(),
                    (self.clock)()?,
                )
                .map_err(|error| error.to_string())?;
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
                self.observe_task_merges(&project, &catalog, &mut outbox)?;
                project.writer.prepare_write(
                    &project.hub,
                    &catalog,
                    &self.endpoint.host,
                    (self.clock)()?,
                    &mut outbox,
                )
            })();
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                if let Ok(unix_ms) = (self.clock)() {
                    let _ = record_snapshot_failure(
                        &project,
                        &before,
                        error.clone(),
                        None,
                        &self.endpoint.host,
                        ClockReading {
                            mono_ms: (self.mono_clock)(),
                            unix_ms,
                        },
                    );
                }
                return Err(error);
            }
        };
        let attempt = project.writer.write_prepared_observed(
            &project.hub,
            &catalog,
            &self.endpoint.host,
            true,
            || (self.clock)(),
            prepared,
        );
        // Use a fresh trusted pair for elapsed-time accounting after actual work.
        finish_snapshot_attempt(
            &project,
            &before,
            &attempt,
            &self.endpoint.host,
            ClockReading {
                mono_ms: (self.mono_clock)(),
                unix_ms: (self.clock)()?,
            },
        )?;
        let report = attempt.into_result()?;
        self.main_diagnostics(
            &catalog.root,
            serde_json::to_value(report).map_err(|error| error.to_string())?,
        )
    }

    fn projects(&self) -> Result<Vec<Arc<Project>>, String> {
        Ok(self
            .projects
            .lock()
            .map_err(|_| "项目注册表锁损坏")?
            .values()
            .cloned()
            .collect())
    }

    fn rebuild_request(&self, request: RebuildRequest) -> Result<Value, String> {
        let catalog = self.catalog(&request.directory)?;
        let time = (self.clock)()?;
        let prepared = towermd_core::recovery::prepare(
            &catalog,
            &self.recovery_inputs(&catalog)?,
            time,
            time.checked_add(300_000).ok_or("expiry overflow")?,
            &Uuid::now_v7().to_string(),
        )?;
        let stored = StoredRebuildRequest {
            expires_at: prepared.expires_at,
            request: prepared,
            directory: request.directory,
            common_dir: catalog.common_dir.clone(),
        };
        let directory = self.runtime.join("rebuild-requests");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let path = directory.join(format!("{}.json", stored.request.request_id));
        // A previously saved request is immutable, including its original expiry.
        let saved = match self.saved_request(&stored.request.request_id)? {
            Some(saved) => saved,
            None => {
                write_json(&path, &stored)?;
                stored
            }
        };
        self.main_diagnostics(
            &catalog.root,
            serde_json::to_value(saved.request).map_err(|e| e.to_string())?,
        )
    }

    fn saved_request(&self, id: &str) -> Result<Option<StoredRebuildRequest>, String> {
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Ok(None);
        }
        let bytes = match fs::read(
            self.runtime
                .join("rebuild-requests")
                .join(format!("{id}.json")),
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let saved: StoredRebuildRequest =
            serde_json::from_slice(&bytes).map_err(|e| format!("恢复请求记录损坏：{e}"))?;
        if saved.request.request_id != id || saved.request.expires_at != saved.expires_at {
            return Err("恢复请求记录绑定不匹配".into());
        }
        Ok(Some(saved))
    }

    fn project_for_rebuild(
        &self,
        saved: &StoredRebuildRequest,
        actor: &str,
    ) -> Result<Value, RebuildFailure> {
        use towermd_core::recovery::check_request_sources;
        #[cfg(test)]
        eprintln!(
            "REBUILD_PHASE real_ms={} root={} phase=start",
            now_ms().unwrap_or(0),
            saved.directory.display()
        );
        check_request_sources(&saved.directory, &saved.request)?;
        let catalog =
            self.catalog(&saved.directory).map_err(|error| {
                match check_request_sources(&saved.directory, &saved.request) {
                    Ok(()) => RebuildFailure::from(error),
                    Err(source) => {
                        let mut failure = RebuildFailure::from(source);
                        failure.message = format!("{error}；{}", failure.message);
                        failure
                    }
                }
            })?;
        if catalog.common_dir != saved.common_dir
            || catalog.config.project_id() != saved.request.project_id
        {
            return Err(towermd_core::recovery::ConfirmError::Conflict(
                "恢复请求项目或 clone 已变化，须新请求".into(),
            )
            .into());
        }
        // Hub::open repairs incomplete tails. Recovery must preserve damaged bytes instead.
        if inspect_store(&saved.common_dir.join("towermd"))? == StoreState::Nonempty {
            return Err("hub journal 仍有可读历史，拒绝将其作为丢失数据覆盖".into());
        }
        let inputs = self.recovery_inputs(&catalog)?;
        #[cfg(test)]
        eprintln!(
            "REBUILD_PHASE real_ms={} root={} phase=inputs-ready",
            now_ms().unwrap_or(0),
            saved.directory.display()
        );
        let interval = snapshot_service::interval_ms(&catalog)?;
        let mut project = self.registered_project(catalog.config.project_id())?;
        let registration = if project.is_none() {
            let guard = self.registration.lock().map_err(|_| "项目注册锁损坏")?;
            project = self.registered_project(catalog.config.project_id())?;
            Some(guard)
        } else {
            None
        };
        let mut installed_at = saved.request.requested_at;
        let report = if let Some(project) = project {
            drop(registration);
            if project.common_dir != saved.common_dir {
                return Err("项目 ID 已绑定另一个 clone".into());
            }
            #[cfg(test)]
            eprintln!(
                "REBUILD_PHASE real_ms={} root={} phase=some-writer-start",
                now_ms().unwrap_or(0),
                saved.directory.display()
            );
            let result = project.writer.confirm_rebuild_classified(
                Some(project.hub.clone()),
                &catalog,
                &saved.request,
                actor,
                saved.expires_at,
                &inputs,
                &self.endpoint.host,
                || {
                    installed_at = (self.clock)()?;
                    Ok(installed_at)
                },
            );
            #[cfg(test)]
            eprintln!(
                "REBUILD_PHASE real_ms={} root={} phase=some-writer-done ok={}",
                now_ms().unwrap_or(0),
                saved.directory.display(),
                result.is_ok()
            );
            let (report, hub) = match result {
                Ok(value) => value,
                Err(error) => {
                    if matches!(
                        &error,
                        towermd_core::recovery::ConfirmError::Persisted { .. }
                    ) {
                        *project
                            .recovery_error
                            .lock()
                            .map_err(|_| "恢复诊断锁损坏")? = Some(error.to_string());
                    }
                    return Err(error.into());
                }
            };
            debug_assert!(Arc::ptr_eq(&hub, &project.hub));
            project.finish_recovery.store(true, Ordering::Release);
            #[cfg(test)]
            tests::q1_after_rebuild_activation(&project.common_dir);
            *project.root.lock().map_err(|_| "项目路径锁损坏")? = catalog.root.clone();
            *project
                .recovery_error
                .lock()
                .map_err(|_| "恢复诊断锁损坏")? = None;
            project.snapshot_interval.store(interval, Ordering::Relaxed);
            consume_snapshot_schedule(
                &project,
                ClockReading {
                    mono_ms: (self.mono_clock)(),
                    unix_ms: installed_at,
                },
                |_, _, _| Ok(((), None)),
            )?;
            report
        } else {
            let project_lock = bind_project(
                &self.runtime,
                catalog.config.project_id(),
                &saved.common_dir,
            )?;
            let writer = Arc::new(SnapshotWriter::default());
            #[cfg(test)]
            eprintln!(
                "REBUILD_PHASE real_ms={} root={} phase=none-writer-start",
                now_ms().unwrap_or(0),
                saved.directory.display()
            );
            let (report, hub) = writer.confirm_rebuild_classified(
                None,
                &catalog,
                &saved.request,
                actor,
                saved.expires_at,
                &inputs,
                &self.endpoint.host,
                || {
                    installed_at = (self.clock)()?;
                    Ok(installed_at)
                },
            )?;
            #[cfg(test)]
            eprintln!(
                "REBUILD_PHASE real_ms={} root={} phase=none-writer-done",
                now_ms().unwrap_or(0),
                saved.directory.display()
            );
            let summary = hub
                .lock()
                .map_err(|_| "项目 hub 锁损坏")?
                .snapshot_summary()?;
            let mut schedule = SnapshotSchedule::default();
            schedule.bind_session(
                &hub.lock()
                    .map_err(|_| "项目 hub 锁损坏")?
                    .snapshot_session_id(),
            );
            schedule.install(
                &summary,
                ClockReading {
                    mono_ms: (self.mono_clock)(),
                    unix_ms: installed_at,
                },
                interval,
            );
            schedule.prepare_required();
            let project = Arc::new(Project {
                project_id: catalog.config.project_id().to_owned(),
                finish_registered: AtomicBool::new(false),
                finish_recovery_active: AtomicBool::new(false),
                finish_retry_after: AtomicU64::new(0),
                finish_recovery: AtomicBool::new(true),
                finish_maintenance_at: Mutex::new(None),
                producer: Arc::new(Mutex::new(())),
                outbox_uncertainty: inputs.outbox_uncertainty(),
                publication_reconcile: AtomicBool::new(true),
                common_dir: saved.common_dir.clone(),
                root: Mutex::new(catalog.root.clone()),
                hub,
                writer,
                snapshot_interval: AtomicU64::new(interval),
                schedule: Mutex::new(schedule),
                ask_tick: Mutex::new(ask_tick::AskTickSchedule::default()),
                main_observer: Mutex::new(trailers_routes::MainObserverSchedule::default()),
                snapshot_context: Mutex::new(None),
                #[cfg(test)]
                background_catalog_loads: AtomicU64::new(0),
                reconcile: AtomicBool::new(true),
                recovery_error: Mutex::new(None),
                background: Mutex::new(None),
                _lock: project_lock,
            });
            self.projects
                .lock()
                .map_err(|_| "项目注册表锁损坏")?
                .insert(catalog.config.project_id().into(), project);
            drop(registration);
            report
        };
        serde_json::to_value(report).map_err(|error| error.to_string().into())
    }

    fn tick(&self) -> Result<(), String> {
        if !*self.shutdown.borrow() {
            self.main_refresh.tick()?;
        }
        let projects = self.projects()?;
        // This sole ticker maintains actual system fences without awaiting business proof.
        for project in &projects {
            let result = (|| -> Result<(), String> {
                let mut hub = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
                snapshot_service::maintain_main(&mut hub, &self.endpoint.host, (self.clock)()?)
            })();
            if let Err(error) = result {
                eprintln!(
                    "towermd 项目租约维护失败（{}）：{error}",
                    project.common_dir.display()
                );
            }
        }
        for project in projects {
            // Only derived scheduling state is read here. All Catalog/history
            // proof and original receipt coordination stay in the worker.
            if let Err(error) = self.schedule_finish_recovery(project.clone()) {
                eprintln!(
                    "towermd finish recovery scheduling ({}): {error}",
                    project.common_dir.display()
                );
            }
            let mut background = project.background.lock().map_err(|_| "快照后台锁损坏")?;
            if *self.shutdown.borrow()
                || background.as_ref().is_some_and(|job| !job.is_finished())
                || project
                    .recovery_error
                    .lock()
                    .map_err(|_| "恢复诊断锁损坏")?
                    .is_some()
            {
                continue;
            }
            let now = ClockReading {
                mono_ms: (self.mono_clock)(),
                unix_ms: (self.clock)()?,
            };
            // Installed projection is only a scheduling hint. In particular a
            // claimed Run without finish/Evidence must still reject fake footers.
            let main_hint = trailers_routes::project_hint(&project).unwrap_or(true);
            let (main_pending, main_admitted) = {
                let schedule = project
                    .main_observer
                    .lock()
                    .map_err(|_| "main 观察调度锁损坏")?;
                (
                    schedule.pending(),
                    schedule.admitted(main_hint, now.mono_ms),
                )
            };
            let context = project
                .snapshot_context
                .lock()
                .map_err(|_| "快照上下文锁损坏")?
                .clone();
            if main_hint || main_pending {
                // The worker must prove main before any expiry-capable tick.
                // This does not force a snapshot or bypass its existing backoff.
            } else if let Some(context) = context {
                let maintenance = (|| -> Result<(), String> {
                    let _producer = project.producer.try_lock().map_err(|_| "项目写入队列忙")?;
                    // A claim may have installed after the earlier pure hint.
                    // Recheck while owning its producer, before any expiry.
                    if trailers_routes::project_hint(&project)? {
                        return Err("Task main observation requires background proof".into());
                    }
                    if ProjectSession::idle_maintenance_shared(
                        &project.hub,
                        &context.catalog,
                        &context.inputs,
                        now.unix_ms,
                    )? {
                        return Ok(());
                    }
                    let mut outbox = self.task_outbox(&context.catalog)?;
                    let inputs = outbox.read_inputs()?;
                    if ProjectSession::task_reconciliation_needed_shared(
                        &project.hub,
                        &context.catalog,
                        &inputs,
                    )? {
                        return Err(
                            "Task original intent requires background reconciliation".into()
                        );
                    }
                    ProjectSession::tick_incremental_shared(
                        &project.hub,
                        &context.catalog,
                        &context.inputs,
                        &self.endpoint.host,
                        now.unix_ms,
                        &mut outbox,
                    )
                    .map_err(|error| error.to_string())
                })();
                if maintenance.is_err() {
                    project
                        .schedule
                        .lock()
                        .map_err(|_| "快照调度锁损坏")?
                        .prepare_required();
                }
            } else {
                project
                    .schedule
                    .lock()
                    .map_err(|_| "快照调度锁损坏")?
                    .prepare_required();
            }
            let admitted = consume_snapshot_schedule(&project, now, |schedule, _, _| {
                #[cfg(test)]
                if std::env::var_os("TOWERMD_I2_CHILD").is_some() {
                    eprintln!("I2_TICK real_unix_ms={} project={} mono={} unix={} eligible={} preparation={} reconcile={} can_attempt={}",now_ms().unwrap_or(0),project.common_dir.display(),now.mono_ms,now.unix_ms,schedule.eligible(now.mono_ms),schedule.preparation_required(),project.reconcile.load(Ordering::Relaxed),schedule.can_attempt(now.mono_ms));
                }
                Ok((
                    schedule.can_attempt(now.mono_ms)
                        && (schedule.preparation_required()
                            || schedule.eligible(now.mono_ms)
                            || project.reconcile.load(Ordering::Relaxed)),
                    None,
                ))
            });
            let snapshot_admitted = matches!(admitted, Ok(true));
            // This is a pure in-memory hint. It never probes Git or authorizes
            // append, and Ask deadlines do not inherit snapshot writer backoff.
            let (ask_hint, hint_error) = match ask_tick::project_hint(&project, now.unix_ms) {
                Ok(hint) => (hint, None),
                Err(error) => (ask_tick::Hint::needs_proof(), Some(error)),
            };
            let ask_admitted = match project.ask_tick.lock() {
                Ok(schedule) => schedule.admitted(&ask_hint, now.mono_ms),
                Err(_) => {
                    eprintln!(
                        "towermd Ask schedule unavailable ({}): poisoned project scheduling lock",
                        project.common_dir.display()
                    );
                    continue;
                }
            };
            if ask_admitted {
                if let Some(error) = hint_error {
                    eprintln!("towermd Ask projection unavailable ({}), scheduling full background proof: {error}",project.common_dir.display());
                }
            }
            if !snapshot_admitted && !ask_admitted && !main_admitted {
                continue;
            }
            let worker = project.clone();
            let host = self.endpoint.host.clone();
            let runtime = self.runtime.clone();
            let clock = self.clock.clone();
            let mono = self.mono_clock.clone();
            let shutdown = self.shutdown.subscribe();
            *background = Some(tokio::task::spawn_blocking(move || {
                let mut attempted = false;
                let mut preparation = None;
                let mut ask_handled = false;
                let mut main_handled = false;
                let mut ask_requested = ask_admitted || ask_hint.present;
                let result = (|| -> Result<(), String> {
                    if *shutdown.borrow() {
                        return Ok(());
                    }
                    let time = ClockReading {
                        mono_ms: mono(),
                        unix_ms: clock()?,
                    };
                    let snapshot_admitted = snapshot_admitted
                        && worker
                            .schedule
                            .lock()
                            .map_err(|_| "快照调度锁损坏")?
                            .can_attempt(time.mono_ms);
                    if !snapshot_admitted && !ask_admitted && !main_admitted {
                        return Ok(());
                    }
                    if snapshot_admitted {
                        preparation = Some(begin_snapshot_preparation(&worker)?);
                    }
                    let root = worker.root.lock().map_err(|_| "项目路径锁损坏")?.clone();
                    // The heavy boundary is after shutdown, single flight and retry eligibility.
                    #[cfg(test)]
                    worker
                        .background_catalog_loads
                        .fetch_add(1, Ordering::SeqCst);
                    #[cfg(test)]
                    trace_i2_phase(&root, "worker-catalog-start");
                    let catalog = Catalog::load(&root)?;
                    #[cfg(test)]
                    trace_i2_phase(&root, "worker-catalog-done");
                    let (prepared, inputs) = {
                        let _producer = worker.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
                        let mut outbox =
                            towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(
                                &runtime
                                    .join("recovery-outbox")
                                    .join(catalog.config.project_id()),
                                towermd_core::recovery_outbox::RecoveryContext::from_catalog(
                                    &catalog,
                                )?,
                                worker.outbox_uncertainty.clone(),
                            )?;
                        let inputs = outbox.read_inputs()?;
                        if snapshot_admitted || main_admitted {
                            Self::reconcile_task_intents(
                                &worker.hub,
                                &catalog,
                                &inputs,
                                time.unix_ms,
                                &mut outbox,
                            )?;
                        }
                        // Normal main certification always precedes the Task
                        // expiry in prepare_write. Core owns candidates/proof,
                        // actual Run binding and the only durable merge fact.
                        let observed =
                            trailers_routes::observe(&worker, &catalog, &host, &mut outbox, || {
                                clock()
                            });
                        trailers_routes::finish_observation(&worker, observed.is_ok(), mono())?;
                        main_handled = true;
                        observed?;
                        // This existing background worker owns the project
                        // producer only for each finite write. It settles Ask
                        // deadlines without extending inactive module leases.
                        // Historical accepted terminals may need only receipt
                        // confirmation, with no current effective Ask Hold. The
                        // already-read records are a scheduling hint; core still
                        // proves the complete original and actual acceptance.
                        ask_requested |= ask_tick::has_request_hint(&inputs);
                        if ask_requested {
                            let ask_result = ProjectSession::human_tick_shared(
                                &worker.hub,
                                &catalog,
                                &host,
                                &mut outbox,
                                || clock(),
                            );
                            ask_tick::finish(&worker, ask_result.is_ok(), mono(), time.unix_ms)?;
                            ask_handled = true;
                            ask_result?;
                        }
                        // Main-only maintenance must expire Tasks after the
                        // successful observation, independently of snapshot
                        // due/backoff. Use the same producer/outbox and a fresh
                        // physical prefix/clock; never continue after a source
                        // failure or manufacture a new snapshot attempt.
                        if !snapshot_admitted {
                            if main_admitted || main_hint || main_pending {
                                let fresh = outbox.read_inputs()?;
                                ProjectSession::tick_shared(
                                    &worker.hub,
                                    &catalog,
                                    &fresh,
                                    &host,
                                    clock()?,
                                    &mut outbox,
                                )?;
                            }
                            return Ok(());
                        }
                        let prepared = worker.writer.prepare_write(
                            &worker.hub,
                            &catalog,
                            &host,
                            clock()?,
                            &mut outbox,
                        )?;
                        (prepared, outbox.read_inputs()?)
                    };
                    if worker.publication_reconcile.swap(false, Ordering::AcqRel) {
                        let reconcile = (|| -> Result<bool, String> {
                            let _producer =
                                worker.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
                            let current = current_scan_catalog(&catalog)?;
                            let directory = runtime
                                .join("recovery-outbox")
                                .join(current.config.project_id());
                            let inputs = towermd_core::recovery_outbox::RecoveryInputs::read_with_uncertainty(
                                &directory, &current, worker.outbox_uncertainty.clone(),
                            )?;
                            let pending =
                                snapshot_service::pending_baseline_publications(&current, &inputs)?;
                            if pending.is_empty() {
                                return Ok(false);
                            }
                            let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(
                                &directory,
                                towermd_core::recovery_outbox::RecoveryContext::from_catalog(&current)?,
                                worker.outbox_uncertainty.clone(),
                            )?;
                            // A restart can retry existing human approvals, but
                            // cannot create one. Every attempt rechecks authority.
                            for approval in pending {
                                let current = Catalog::load(&current.root)?;
                                match worker.writer.publish_baseline(
                                    &worker.hub,
                                    &current,
                                    &mut outbox,
                                    &approval,
                                    &host,
                                    || clock(),
                                ) {
                                    Err(snapshot_service::PublicationError::Busy) => {
                                        worker.publication_reconcile.store(true, Ordering::Release);
                                    }
                                    Err(error) => eprintln!("towermd 基线出版恢复未完成：{error}"),
                                    Ok(_) => {}
                                }
                            }
                            // Publication may have advanced main. Let the next
                            // existing worker prepare a fresh snapshot context.
                            *worker
                                .snapshot_context
                                .lock()
                                .map_err(|_| "快照上下文锁损坏")? = None;
                            worker
                                .schedule
                                .lock()
                                .map_err(|_| "快照调度锁损坏")?
                                .prepare_required();
                            Ok(true)
                        })();
                        match reconcile {
                            Ok(true) => return Ok(()),
                            Ok(false) => {}
                            Err(error) => {
                                worker.publication_reconcile.store(true, Ordering::Release);
                                return Err(error);
                            }
                        }
                    }
                    #[cfg(test)]
                    trace_i2_phase(&root, "worker-tick-shared-done");
                    let interval = snapshot_service::interval_ms(&catalog)?;
                    worker.snapshot_interval.store(interval, Ordering::Relaxed);
                    let (force, summary, session) = consume_snapshot_schedule(
                        &worker,
                        ClockReading {
                            mono_ms: mono(),
                            unix_ms: time.unix_ms,
                        },
                        |schedule, summary, session| {
                            schedule.prepared();
                            Ok((
                                (schedule.eligible(mono()), summary.clone(), session.clone()),
                                None,
                            ))
                        },
                    )?;
                    let before = SnapshotPreparation {
                        summary,
                        session,
                        order: preparation
                            .as_ref()
                            .expect("worker preparation")
                            .order
                            .clone(),
                    };
                    let context = Arc::new(SnapshotContext { catalog, inputs });
                    *worker
                        .snapshot_context
                        .lock()
                        .map_err(|_| "快照上下文锁损坏")? = Some(context.clone());
                    if *shutdown.borrow() || (!force && !worker.reconcile.load(Ordering::Relaxed)) {
                        return Ok(());
                    }
                    #[cfg(test)]
                    trace_i2_phase(&root, "worker-writer-start");
                    let attempt = worker.writer.write_prepared_observed(
                        &worker.hub,
                        &context.catalog,
                        &host,
                        force,
                        || {
                            #[cfg(test)]
                            trace_i2_phase(&root, "worker-writer-clock");
                            clock()
                        },
                        prepared,
                    );
                    #[cfg(test)]
                    trace_i2_phase(&root, "worker-writer-done");
                    #[cfg(test)]
                    if std::env::var_os("TOWERMD_I2_CHILD").is_some() {
                        eprintln!(
                            "I2_WRITER_RESULT root={} result={:?}",
                            root.display(),
                            attempt.result()
                        );
                    }
                    attempted = true;
                    finish_snapshot_attempt(
                        &worker,
                        &before,
                        &attempt,
                        &host,
                        ClockReading {
                            mono_ms: mono(),
                            unix_ms: clock()?,
                        },
                    )?;
                    attempt.into_result().map(|_| ())
                })();
                if let Err(error) = result {
                    if !main_handled && (main_hint || main_pending || main_admitted) {
                        let _ = trailers_routes::finish_observation(&worker, false, mono());
                    }
                    if !ask_handled && ask_requested {
                        let _ = ask_tick::finish(&worker, false, mono(), 0);
                    }
                    if !attempted {
                        if let Some(preparation) = &preparation {
                            let time = clock().map(|unix_ms| ClockReading {
                                mono_ms: mono(),
                                unix_ms,
                            });
                            if let Ok(time) = time {
                                let _ = record_snapshot_failure(
                                    &worker,
                                    preparation,
                                    error.clone(),
                                    None,
                                    &host,
                                    time,
                                );
                            }
                        }
                    }
                    eprintln!(
                        "towermd 后台项目维护失败（{}）：{error}",
                        worker.common_dir.display()
                    );
                }
            }));
        }
        Ok(())
    }

    async fn drain_snapshots(&self) -> Result<(), String> {
        let mut failure = None;
        for project in self.projects()? {
            let job = project
                .background
                .lock()
                .map_err(|_| "快照后台锁损坏")?
                .take();
            if let Some(job) = job {
                if let Err(error) = job.await {
                    failure = Some(format!("后台快照写入结果不确定：{error}"));
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

async fn health(State(server): State<Arc<Server>>) -> Json<Endpoint> {
    Json(server.endpoint.clone())
}

#[cfg(test)]
fn validate_subscription_cursor(after: u64, floor: u64, last_seq: u64) -> Result<(), String> {
    if after < floor {
        return Err(format!("订阅游标早于 hub 快照水位 {floor}，请从快照加载"));
    }
    if after > last_seq {
        return Err("订阅游标超过 hub 连续水位".into());
    }
    Ok(())
}

async fn subscribe(
    State(server): State<Arc<Server>>,
    Query(subscription): Query<Subscription>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if subscription.instance != server.endpoint.instance {
        return (StatusCode::CONFLICT, "守护进程实例已改变").into_response();
    }
    let loader = server.clone();
    let after = subscription.after;
    let result = tokio::task::spawn_blocking(move || {
        let catalog = loader.catalog(&subscription.directory)?;
        let project = loader.project(&catalog)?;
        let hub = project.hub.clone();
        let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
        ProjectSession::subscribe_shared(
            &hub,
            &catalog,
            &loader.recovery_inputs(&catalog)?,
            after,
            (loader.clock)()?,
        )
        .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))?;
        Ok::<_, String>((
            hub,
            catalog.root,
            project.outbox_uncertainty.clone(),
            project.producer.clone(),
        ))
    })
    .await;
    match result {
        Ok(Ok((hub, root, uncertainty, producer))) => upgrade.on_upgrade(move |socket| {
            stream_events(
                socket,
                hub,
                after,
                server.shutdown.subscribe(),
                server.clock.clone(),
                StreamRecovery {
                    root,
                    runtime: server.runtime.clone(),
                    uncertainty,
                    producer,
                },
            )
        }),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, error).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

#[derive(Clone)]
struct StreamRecovery {
    producer: Arc<Mutex<()>>,
    root: PathBuf,
    runtime: PathBuf,
    uncertainty: towermd_core::recovery_outbox::OutboxUncertainty,
}

async fn stream_events(
    mut socket: WebSocket,
    hub: Arc<Mutex<ProjectSession>>,
    mut after: u64,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    clock: Arc<dyn Fn() -> Result<u64, String> + Send + Sync>,
    recovery: StreamRecovery,
) {
    let mut interval = tokio::time::interval(Duration::from_millis(100));
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
                let hub = hub.clone();let clock=clock.clone();let StreamRecovery { root, runtime, uncertainty, producer }=recovery.clone();
                let batch = tokio::task::spawn_blocking(move || {
                    // Same boundary as claim/release: never validate a half-written outbox.
                    // The guard is dropped before any socket send/await.
                    let _producer=producer.lock().map_err(|_|HubError::Invalid("项目写入队列锁损坏".into()))?;
                    let now=clock().map_err(HubError::Invalid)?;
                    let catalog=Catalog::load(&root).map_err(HubError::Invalid)?;
                    let inputs=towermd_core::recovery_outbox::RecoveryInputs::read_with_uncertainty(&runtime.join("recovery-outbox").join(catalog.config.project_id()),&catalog,uncertainty).map_err(HubError::Invalid)?;
                    ProjectSession::subscribe_shared(&hub,&catalog,&inputs,after,now)
                }).await;
                let events=match batch {Ok(Ok(batch))=>batch.events,Ok(Err(error @ HubError::SnapshotRequired{..}))=>{if let Ok(text)=serde_json::to_string(&json!({"error":error})){let _=tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Text(text.into()))).await;}let _=tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Close(None))).await;return;},Ok(Err(error))=>{eprintln!("towermd event stream rejected: {error}");let _=tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Close(None))).await;return;},Err(error)=>{eprintln!("towermd event stream worker failed: {error}");let _=tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Close(None))).await;return;}};
                for event in events {
                    let Ok(text) = serde_json::to_string(&event) else { return; };
                    let sent = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Text(text.into()))).await;
                    if !matches!(sent, Ok(Ok(()))) { return; }
                    after = event.seq;
                }
            }
        }
    }
}

fn authorized(headers: &HeaderMap, server: &Server) -> bool {
    headers
        .get("x-towermd-instance")
        .and_then(|value| value.to_str().ok())
        == Some(server.endpoint.instance.as_str())
}

async fn command(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    match tokio::task::spawn_blocking(move || server.execute(request)).await {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error.to_string(),"owners":error.owners})),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

async fn check(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<CheckRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    match tokio::task::spawn_blocking(move || server.check(request)).await {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

async fn snapshot(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<SnapshotRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    match tokio::task::spawn_blocking(move || server.snapshot(request)).await {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

async fn rebuild_request_handler(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<RebuildRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    match tokio::task::spawn_blocking(move || server.rebuild_request(request)).await {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

async fn saved_rebuild_request(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    match server.saved_request(&id) {
        Ok(Some(saved)) => (
            StatusCode::OK,
            Json(serde_json::to_value(saved).expect("saved request serialization")),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"恢复请求不存在；请运行 hub rebuild"})),
        ),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
    }
}

async fn confirm_rebuild(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    request: axum::extract::Request,
) -> (StatusCode, Json<Value>) {
    let gate = match app_gate::validate_app(&headers, &server.endpoint.instance) {
        Ok(gate) => gate,
        Err(app_gate::AppGateError::InstanceChanged) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error":"守护进程实例已改变"})),
            )
        }
        Err(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"恢复只能由 App 确认"})),
            )
        }
    };
    let bytes = match axum::body::to_bytes(request.into_body(), 16_384).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error":error.to_string()})),
            )
        }
    };
    let body: ConfirmRebuildBody = match serde_json::from_slice(&bytes) {
        Ok(body) => body,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error":error.to_string()})),
            )
        }
    };
    if body
        .actor
        .strip_prefix("human:")
        .and_then(|id| gate.human_actor(id).ok())
        .as_deref()
        != Some(body.actor.as_str())
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error":"人工 actor 格式非法"})),
        );
    }
    let saved = match server.saved_request(&id) {
        Ok(Some(saved)) => saved,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"恢复请求不存在；请运行 hub rebuild"})),
            )
        }
        Err(error) => return (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
    };
    match (server.clock)() {
        Ok(time) if saved.expires_at > time => {}
        Ok(_) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error":"恢复请求已过期；须新请求"})),
            )
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":error})),
            )
        }
    }
    match tokio::task::spawn_blocking(move || server.project_for_rebuild(&saved, &body.actor)).await
    {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => (error.status, Json(json!({"error":error.message}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

async fn coldstart(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<ColdstartRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    let wait_id = request.wait.clone();
    let operation = async {
        if request.wait.is_some() && request.options.is_some() {
            return Err("wait 与扫描 options 不能同时使用".into());
        }
        if request
            .options
            .as_ref()
            .is_some_and(|options| !(1..=200).contains(&options.history_limit))
        {
            return Err("history_limit 必须为 1–200".into());
        }
        if let Some(id) = &request.wait {
            Uuid::parse_str(id).map_err(|_| "scan ID 非法")?;
        }
        let context_server = server.clone();
        let wait = request.wait;
        let context = tokio::task::spawn_blocking(move || {
            let catalog = context_server
                .prepare_coldstart_context(&request.directory, request.run.as_deref())?;
            let (input, record) = if wait.is_none() {
                let input = capture_scan_input(&catalog, &request.options.unwrap_or_default())?;
                let project = context_server.project(&catalog)?;
                let (record, _) = context_server.coldstart_callbacks(catalog.clone(), project);
                (Some(input), Some(record))
            } else {
                (None, None)
            };
            let resume = context_server.coldstart_resume(catalog.clone());
            Ok::<_, String>((catalog, input, wait, record, resume))
        })
        .await
        .map_err(|e| e.to_string())??;
        let (catalog, input, wait, record, resume) = context;
        if let Some(id) = wait {
            server
                .scan_jobs
                .recover(
                    catalog.config.project_id(),
                    &catalog.common_dir,
                    &id,
                    resume,
                )
                .await?;
            server
                .scan_jobs
                .wait(catalog.config.project_id(), &catalog.common_dir, &id)
                .await
        } else {
            server.scan_jobs.enqueue(
                input.ok_or("缺少冻结扫描输入")?,
                server.endpoint.host.clone(),
                record.ok_or("缺少扫描记录器")?,
            )
        }
    };
    let result = if let Some(id) = wait_id {
        match tokio::time::timeout(crate::scan_jobs::WAIT_BUDGET, operation).await {
            Ok(result) => result,
            Err(_) => Ok(json!({"scan_id":id,"state":"pending","stages":["waiting"],
                "diagnostic":"等待已满90秒；请使用同一 scan ID 再次等待，后台工作不会重跑"})),
        }
    } else {
        operation.await
    };
    match result {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
    }
}

async fn coldstart_draft(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<ReadQuery>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    let context_server = server.clone();
    let context = tokio::task::spawn_blocking(move || {
        let catalog =
            context_server.prepare_coldstart_context(&query.directory, query.run.as_deref())?;
        let saved = baseline_routes::saved_draft(
            &catalog,
            &id,
            &context_server.recovery_inputs(&catalog)?,
        )?;
        let resume = context_server.coldstart_resume(catalog.clone());
        Ok::<_, String>((catalog, resume, saved, id))
    })
    .await;
    let result = async {
        let (catalog, resume, saved, id) = context.map_err(|e| e.to_string())??;
        if let Some(saved) = saved {
            return Ok(saved);
        }
        if Uuid::parse_str(&id).is_err() {
            return Err("scan ID 非法".into());
        }
        server
            .scan_jobs
            .recover(
                catalog.config.project_id(),
                &catalog.common_dir,
                &id,
                resume,
            )
            .await?;
        let status =
            server
                .scan_jobs
                .status(catalog.config.project_id(), &catalog.common_dir, &id)?;
        if status["state"] != "completed" {
            return Ok(status);
        }
        server
            .scan_jobs
            .draft(catalog.config.project_id(), &catalog.common_dir, &id)
    }
    .await;
    match result {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) if error == "scan not found" => {
            (StatusCode::NOT_FOUND, Json(json!({"error":error})))
        }
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
    }
}

async fn canvas(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Query(query): Query<ReadQuery>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    read_response(tokio::task::spawn_blocking(move || server.canvas(query)).await)
}

async fn object(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<ReadQuery>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    match tokio::task::spawn_blocking(move || server.object(query, &id)).await {
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, Json(json!({"error":"对象不存在"}))),
        Ok(Ok(Some(value))) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => read_response(Ok(Err(error))),
        Err(error) => read_response(Err(error)),
    }
}

fn read_response(
    result: Result<Result<Value, String>, tokio::task::JoinError>,
) -> (StatusCode, Json<Value>) {
    match result {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        ),
    }
}

async fn shutdown(State(server): State<Arc<Server>>, headers: HeaderMap) -> StatusCode {
    if !authorized(&headers, &server) {
        return StatusCode::CONFLICT;
    }
    let _ = server.shutdown.send(true);
    StatusCode::OK
}

fn read_inbox(
    http: &reqwest::blocking::Client,
    endpoint: &Endpoint,
    directory: &Path,
    run: Option<String>,
    recovery: bool,
) -> Result<Option<inbox_routes::InboxResponse>, String> {
    // At most three identical read requests; no output, ack or business command
    // has been sent. Only the server's typed first-continuation code is eligible.
    for attempt in 0..3 {
        let response = http
            .post(url(endpoint, "/inbox"))
            .header("x-towermd-instance", &endpoint.instance)
            .header(
                "x-towermd-inbox-activity",
                if recovery { "recovery" } else { "project" },
            )
            .json(&inbox_routes::InboxRequest {
                directory: directory.to_owned(),
                run: run.clone(),
            })
            .send()
            .map_err(|error| format!("inbox read failed: {error}"))?;
        let status = response.status();
        let value: Value = response
            .json()
            .map_err(|error| format!("invalid inbox response: {error}"))?;
        if !status.is_success() {
            if recovery
                && status.as_u16() == 409
                && value["code"] == "inbox_unavailable_for_recovery"
            {
                eprintln!(
                    "towermd: inbox unavailable before App recovery; no messages read or acknowledged"
                );
                return Ok(None);
            }
            if status.as_u16() == 409
                && value["code"] == "inbox_read_changed"
                && value["error"] == "head changed before continuation"
                && value.get("request_id").is_some_and(Value::is_null)
                && attempt < 2
            {
                continue;
            }
            return Err(value["error"]
                .as_str()
                .unwrap_or("inbox read rejected")
                .into());
        }
        let inbox: inbox_routes::InboxResponse =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        if !inbox.messages.is_empty() && inbox.delivery_token.is_empty() {
            return Err("inbox messages lack delivery token".into());
        }
        return Ok(Some(inbox));
    }
    Err("inbox read retry budget exhausted".into())
}

/// One finite CLI prelude, with the same daemon instance captured for read and ack.
pub fn deliver_inbox<W: std::io::Write>(
    directory: PathBuf,
    run: Option<String>,
    recovery: bool,
    out: &mut W,
) -> Result<(), String> {
    let endpoint = connect(true)?.ok_or("daemon unavailable")?;
    let http = client(Duration::from_secs(120))?;
    deliver_inbox_at(&http, &endpoint, directory, run, recovery, out)
}

fn deliver_inbox_at<W: std::io::Write>(
    http: &reqwest::blocking::Client,
    endpoint: &Endpoint,
    directory: PathBuf,
    run: Option<String>,
    recovery: bool,
    out: &mut W,
) -> Result<(), String> {
    let Some(inbox) = read_inbox(http, endpoint, &directory, run, recovery)? else {
        return Ok(());
    };
    let request_id = Uuid::now_v7().to_string();
    crate::cli_delivery::print_then_ack(out, &inbox.messages, |ids| {
        let mut ids = ids.to_vec();
        ids.sort();
        ids.dedup();
        let response = http
            .post(url(endpoint, "/inbox/delivered"))
            .header("x-towermd-instance", &endpoint.instance)
            .header(
                "x-towermd-inbox-activity",
                if recovery { "recovery" } else { "project" },
            )
            .json(&inbox_routes::DeliveredRequest {
                directory,
                request_id: request_id.clone(),
                delivery_token: inbox.delivery_token,
                message_ids: ids.clone(),
            })
            .send()
            .map_err(|error| {
                format!(
                    "request_id={request_id}: delivery acknowledgement transport failed: {error}"
                )
            })?;
        let status = response.status();
        let value: Value = response.json().map_err(|error| {
            format!("request_id={request_id}: invalid acknowledgement response: {error}")
        })?;
        if !status.is_success() {
            return Err(format!(
                "request_id={request_id}: {}",
                value["error"]
                    .as_str()
                    .unwrap_or("delivery acknowledgement rejected")
            ));
        }
        let outcome: towermd_core::cli_delivery::DeliveryOutcome =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        if outcome.receipt.request_id != request_id || outcome.receipt.result_ref.message_ids != ids
        {
            return Err("delivery acknowledgement differs from exact printed IDs/request".into());
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "inbox_retry_tests.rs"]
mod inbox_retry_tests;

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn snapshot_git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().into()
    }
    fn snapshot_private(root: &Path) -> Option<String> {
        let out = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "--verify", "--quiet", "refs/towermd/snapshot"])
            .output()
            .unwrap();
        if out.status.success() {
            Some(String::from_utf8(out.stdout).unwrap().trim().into())
        } else {
            assert_eq!(out.status.code(), Some(1), "{out:?}");
            None
        }
    }
    pub(super) fn snapshot_server(
        root: &Path,
        runtime: PathBuf,
        mono: Arc<AtomicU64>,
        unix: Arc<AtomicU64>,
    ) -> Arc<Server> {
        fs::create_dir_all(&runtime).unwrap();
        let (shutdown, _receiver) = tokio::sync::watch::channel(false);
        let _ = root;
        Arc::new(Server {
            source_status_cache: Default::default(),
            main_refresh: MainRefresh::default(),
            mono_clock: Arc::new(move || mono.load(Ordering::SeqCst)),
            clock: Arc::new(move || Ok(unix.load(Ordering::SeqCst))),
            endpoint: Endpoint {
                address: "127.0.0.1".into(),
                port: 0,
                pid: std::process::id(),
                instance: "snapshot-schedule-test".into(),
                host: "snapshot-host".into(),
            },
            finish_jobs: Arc::new(FinishJobs::default()),
            scan_jobs: ScanJobs::new(&runtime).unwrap(),
            runtime,
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
            delivery_tokens: Mutex::new(inbox_routes::DeliveryTokens::default()),
            shutdown,
        })
    }
    pub(super) fn snapshot_fixture(interval: u64) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir(&root).unwrap();
        snapshot_git(&root, &["init", "-qb", "main"]);
        towermd_core::init::init(&root, &towermd_core::init::InitOptions { docs_dir: None })
            .unwrap();
        let config = root.join(".towermd/config.yaml");
        fs::write(
            &config,
            format!(
                "{}\nsnapshot_interval: {interval}\n",
                fs::read_to_string(&config).unwrap()
            ),
        )
        .unwrap();
        snapshot_git(&root, &["add", "."]);
        snapshot_git(&root, &["commit", "-qm", "snapshot scheduling fixture"]);
        (dir, root)
    }
    #[test]
    fn cold_project_shares_bound_outbox_uncertainty_with_its_writers() {
        use towermd_core::hub_protocol::HubProtocol;
        let (dir, root) = snapshot_fixture(60);
        let catalog = Catalog::load(&root).unwrap();
        let time = 1_800_000_000_000;
        let mut hub = Hub::open(&catalog.common_dir.join("towermd")).unwrap();
        hub.append(
            towermd_core::hub_protocol::NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:test".into(),
                kind: "future.audit".into(),
                layer: "L5".into(),
                obj: "audit".into(),
                data: json!({}),
                guards: vec![],
            },
            time,
        )
        .unwrap();
        drop(hub);
        let server = snapshot_server(
            &root,
            dir.path().join("runtime"),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(time)),
        );
        let project = server.project(&catalog).unwrap();
        let inputs = server.recovery_inputs(&catalog).unwrap();
        // No sync/replay after registration may replace the original binding.
        assert_eq!(
            project
                .hub
                .lock()
                .unwrap()
                .snapshot_summary()
                .unwrap()
                .periodic_pending_since,
            Some(time)
        );
        assert!(
            ProjectSession::idle_maintenance_shared(&project.hub, &catalog, &inputs, time).unwrap()
        );
    }
    /// Real local Hub acceptance followed by a filesystem confirmation failure.
    /// This fixture only interrupts the outbox, never fabricates an actual receipt.
    struct InterruptTaskConfirmation {
        inner: towermd_core::hub::Hub,
        directory: PathBuf,
        backup: PathBuf,
        interrupted: Arc<Mutex<Option<(towermd_core::hub_protocol::NewEvent, PathBuf)>>>,
    }
    impl towermd_core::hub_protocol::HubProtocol for InterruptTaskConfirmation {
        fn append(
            &mut self,
            event: towermd_core::hub_protocol::NewEvent,
            now: u64,
        ) -> Result<towermd_core::hub_protocol::AppendReceipt, towermd_core::hub_protocol::HubError>
        {
            let actual = self.inner.append(event.clone(), now)?;
            if event.kind == "task.claimed" && self.interrupted.lock().unwrap().is_none() {
                let path = fs::read_dir(&self.directory)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.extension()
                            .is_some_and(|extension| extension == "jsonl")
                    })
                    .unwrap();
                fs::rename(&path, &self.backup).unwrap();
                fs::create_dir(&path).unwrap();
                *self.interrupted.lock().unwrap() = Some((event, path));
            }
            Ok(actual)
        }
        fn subscribe(
            &mut self,
            after: u64,
            now: u64,
        ) -> Result<
            towermd_core::hub_protocol::SubscriptionBatch,
            towermd_core::hub_protocol::HubError,
        > {
            self.inner.subscribe(after, now)
        }
        fn lease_acquire(
            &mut self,
            request: towermd_core::hub_protocol::AcquireLease,
            now: u64,
        ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
        {
            self.inner.lease_acquire(request, now)
        }
        fn lease_renew(
            &mut self,
            request: towermd_core::hub_protocol::RenewLease,
            now: u64,
        ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
        {
            self.inner.lease_renew(request, now)
        }
        fn lease_release(
            &mut self,
            request: towermd_core::hub_protocol::ReleaseLease,
            now: u64,
        ) -> Result<towermd_core::hub_protocol::LeaseReceipt, towermd_core::hub_protocol::HubError>
        {
            self.inner.lease_release(request, now)
        }
        fn snapshot_ack(
            &mut self,
            ack: towermd_core::hub_protocol::SnapshotAck,
            checkpoint: Value,
        ) -> Result<(), towermd_core::hub_protocol::HubError> {
            towermd_core::hub_protocol::HubProtocol::snapshot_ack(&mut self.inner, ack, checkpoint)
        }
        fn rebuild(
            &mut self,
            seed: towermd_core::hub_protocol::RebuildSeed,
        ) -> Result<towermd_core::hub_protocol::RebuildReceipt, towermd_core::hub_protocol::HubError>
        {
            self.inner.rebuild(seed)
        }
        fn contiguous_seq(&self) -> u64 {
            self.inner.contiguous_seq()
        }
    }
    #[test]
    fn next_daemon_business_repairs_only_the_original_accepted_task_confirmation() {
        let (dir, root) = snapshot_fixture(600);
        fs::write(
            root.join("docs/tower/A.md"),
            "---\ntype: Module\nid: A\npaths: ['src/**']\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/tower/AC.md"),
            "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        snapshot_git(&root, &["add", "docs"]);
        snapshot_git(&root, &["commit", "-qm", "task confirmation fixture"]);
        snapshot_git(&root, &["switch", "-qc", "t/T"]);
        let server = snapshot_server(
            &root,
            dir.path().join("runtime"),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(1_800_000_000_000)),
        );
        let command = |command| Request {
            directory: root.clone(),
            run: None,
            command,
        };
        server
            .execute(command(TaskCommand::Add {
                task: towermd_core::tasks::TaskSpec {
                    id: "T".into(),
                    title: "Task".into(),
                    module: "A".into(),
                    touches: vec![],
                    allowed_paths: vec!["src/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC".into()],
                    decisions: vec![],
                },
            }))
            .unwrap();
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        let inputs = server.recovery_inputs(&catalog).unwrap();
        let interrupted = Arc::new(Mutex::new(None));
        let backup = server.runtime.join("interrupted-task-confirmation.jsonl");
        let temporary_hub = tempfile::tempdir().unwrap();
        let placeholder = ProjectSession::new(
            Box::new(towermd_core::hub::Hub::open(temporary_hub.path()).unwrap()),
            &catalog,
            &inputs,
            1_800_000_000_000,
        )
        .unwrap();
        {
            // Keep the same registered Session mutex while releasing only the
            // previous owned protocol. Never open the real directory twice.
            let mut live = project.hub.lock().unwrap();
            let previous = std::mem::replace(&mut *live, placeholder);
            drop(previous);
            let protocol = InterruptTaskConfirmation {
                inner: towermd_core::hub::Hub::open(&catalog.common_dir.join("towermd")).unwrap(),
                directory: server
                    .runtime
                    .join("recovery-outbox")
                    .join(catalog.config.project_id()),
                backup: backup.clone(),
                interrupted: interrupted.clone(),
            };
            *live = ProjectSession::new(Box::new(protocol), &catalog, &inputs, 1_800_000_000_000)
                .unwrap();
        }
        let refs = snapshot_git(&root, &["show-ref"]);
        let index = fs::read(catalog.common_dir.join("index")).unwrap();
        let status = snapshot_git(&root, &["status", "--porcelain=v1"]);
        assert!(server
            .execute(command(TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None
            }))
            .is_err());
        let (original, path) = interrupted
            .lock()
            .unwrap()
            .clone()
            .expect("real Hub accepted the original before confirmation failed");
        fs::remove_dir(&path).unwrap();
        fs::rename(backup, &path).unwrap();
        let pending = server.recovery_inputs(&catalog).unwrap();
        assert!(pending.pending().iter().any(|event| event == &original));
        assert!(!pending
            .confirmed()
            .iter()
            .any(|event| event.eid == original.eid));
        let shown = server
            .execute(command(TaskCommand::Show {
                id: Some("T".into()),
            }))
            .unwrap();
        assert!(shown.to_string().contains("running"));
        let confirmed = server.recovery_inputs(&catalog).unwrap();
        let actual = confirmed
            .confirmed()
            .iter()
            .find(|event| event.eid == original.eid)
            .unwrap();
        assert_eq!(actual.data, original.data);
        assert_eq!(actual.guards, original.guards);
        assert_eq!(actual.host, original.host);
        assert_eq!(actual.actor, original.actor);
        assert!(!actual.stale);
        assert!(!confirmed
            .pending()
            .iter()
            .any(|event| event.eid == original.eid));
        assert_eq!(
            project
                .hub
                .lock()
                .unwrap()
                .events(0)
                .iter()
                .filter(|event| event.kind == "task.claimed")
                .count(),
            1
        );
        assert!(project
            .hub
            .lock()
            .unwrap()
            .pending_claim()
            .unwrap()
            .is_none());
        assert_eq!(snapshot_git(&root, &["show-ref"]), refs);
        assert_eq!(fs::read(catalog.common_dir.join("index")).unwrap(), index);
        assert_eq!(snapshot_git(&root, &["status", "--porcelain=v1"]), status);
    }
    fn snapshot_hold(project: &Project, now: u64) -> String {
        let eid = Uuid::now_v7().to_string();
        project
            .hub
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .append(
                towermd_core::hub_protocol::NewEvent {
                    eid: eid.clone(),
                    host: "snapshot-host".into(),
                    actor: "human:test".into(),
                    kind: "hold.resolved".into(),
                    layer: "L4".into(),
                    obj: "H".into(),
                    data: json!({}),
                    guards: vec![],
                },
                now,
            )
            .unwrap();
        eid
    }
    #[tokio::test]
    async fn background_hold_resolved_writes_real_git_snapshot_within_thirty_seconds() {
        let (dir, root) = snapshot_fixture(600);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(1_800_000_000_000));
        let server = snapshot_server(
            &root,
            dir.path().join("runtime"),
            mono.clone(),
            unix.clone(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}/command", listener.local_addr().unwrap());
        let mut stop = server.shutdown.subscribe();
        let app = router(server.clone());
        let serving = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    while !*stop.borrow() {
                        if stop.changed().await.is_err() {
                            break;
                        }
                    }
                })
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let first: Value = client
            .post(&address)
            .header("x-towermd-instance", "snapshot-schedule-test")
            .json(&json!({"directory":root,"run":null,"command":{"action":"status"}}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(first["snapshot"]["last"].is_null());
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        let eid = snapshot_hold(&project, unix.load(Ordering::SeqCst));
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        for offset in (2_000..25_000).step_by(2_000) {
            mono.store(offset, Ordering::SeqCst);
            unix.store(1_800_000_000_000 + offset, Ordering::SeqCst);
            snapshot_hold(&project, unix.load(Ordering::SeqCst));
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
        }
        assert!(snapshot_private(&root).is_none());
        mono.store(25_000, Ordering::SeqCst);
        unix.store(1_800_000_025_000, Ordering::SeqCst);
        let begin = Instant::now();
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let commit = snapshot_private(&root)
            .expect("actual tick must write at the first 25-second deadline");
        let completed_at = 25_000 + begin.elapsed().as_millis();
        eprintln!("actual Router hold first_deadline_ms=25000 Git_ack_completed_at_ms={completed_at} commit={commit}");
        assert!(
            completed_at <= 30_000,
            "actual Git+ack must finish within the remaining five seconds"
        );
        let saved = snapshot_service::read_snapshot(&Catalog::load(&root).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(saved.commit, commit);
        assert!(saved.archives.values().any(|text| text.contains(&eid)));
        assert_eq!(
            project.hub.lock().unwrap().snapshot_ack().unwrap().commit,
            commit
        );
        let status: Value = client
            .post(&address)
            .header("x-towermd-instance", "snapshot-schedule-test")
            .json(&json!({"directory":root,"run":null,"command":{"action":"status"}}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(status["snapshot"]["last"]["commit"], commit);
        assert_eq!(
            status["snapshot"]["last"]["seq"],
            saved.snapshot.through_seq
        );
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), serving)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn restored_alert_without_business_pending_retries_actual_writer() {
        const START: u64 = 1_800_000_000_000;
        let (dir, root) = snapshot_fixture(60);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(START));
        let runtime = dir.path().join("runtime");
        let server = snapshot_server(&root, runtime.clone(), mono.clone(), unix.clone());
        let first = server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        let old = first["commit"].as_str().unwrap().to_string();
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        let lock = project.common_dir.join("refs/towermd/snapshot.lock");
        fs::write(&lock, "actual CAS failure").unwrap();
        server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap_err();
        // Elapsed time crosses the alert threshold while Unix stays fixed. No
        // actual heartbeat or business input can accidentally make this due.
        mono.store(61_001, Ordering::SeqCst);
        server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap_err();
        let before = installed_summary(&project).unwrap();
        assert!(before.fast_pending.is_none() && before.periodic_pending_since.is_none());
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(project);
        drop(server);
        fs::remove_file(lock).unwrap();
        let server = snapshot_server(
            &root,
            runtime.clone(),
            Arc::new(AtomicU64::new(0)),
            unix.clone(),
        );
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        let restored = installed_summary(&project).unwrap();
        let eid = restored.alert.as_ref().unwrap().eid.clone();
        assert!(restored.fast_pending.is_none() && restored.periodic_pending_since.is_none());
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let new = snapshot_private(&root).unwrap();
        assert_ne!(
            new, old,
            "restored failure itself must bypass the old normal-due gate"
        );
        assert_eq!(
            project.hub.lock().unwrap().snapshot_ack().unwrap().commit,
            new
        );
        assert!(project.schedule.lock().unwrap().status()["deferred_since"].is_null());
        let alerts = project
            .hub
            .lock()
            .unwrap()
            .events(0)
            .into_iter()
            .filter(|e| e.kind == "alert.raised" && e.data["component"] == "snapshot")
            .collect::<Vec<_>>();
        assert!(alerts.len() <= 1);
        if let Some(alert) = alerts.first() {
            assert_eq!(alert.eid, eid);
        }
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(project);
        drop(server);
        let server = snapshot_server(&root, runtime, Arc::new(AtomicU64::new(0)), unix);
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(installed_summary(&project).unwrap().alert.is_none());
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    #[tokio::test]
    async fn completing_older_checkpoint_preserves_later_hold_original_deadline() {
        const START: u64 = 1_800_000_000_000;
        let (dir, root) = snapshot_fixture(60);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(START));
        let server = snapshot_server(
            &root,
            dir.path().join("runtime"),
            mono.clone(),
            unix.clone(),
        );
        server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        let before = begin_snapshot_preparation(&project).unwrap();
        let mut calls = 0;
        let mut hold_seq = None;
        let mut original_deadline_due = false;
        let attempt = project.writer.write_prepared_observed(
            &project.hub,
            &catalog,
            &server.endpoint.host,
            true,
            || {
                calls += 1;
                if calls == 2 {
                    // The sole writer has frozen Q and published its actual intent.
                    // A real later hold is installed by the existing full command
                    // boundary, then observed by the ordinary light ticker.
                    mono.store(10_000, Ordering::SeqCst);
                    unix.store(START + 10_000, Ordering::SeqCst);
                    snapshot_hold(&project, START + 10_000);
                    let _ = server.execute(Request {
                        directory: root.clone(),
                        run: None,
                        command: TaskCommand::Show { id: None },
                    });
                    server.tick().unwrap();
                    hold_seq = installed_summary(&project)
                        .unwrap()
                        .fast_pending
                        .map(|p| p.seq);
                    original_deadline_due = !project.schedule.lock().unwrap().eligible(34_999)
                        && project.schedule.lock().unwrap().eligible(35_000);
                }
                Ok(unix.load(Ordering::SeqCst))
            },
            {
                let mut outbox = server.task_outbox(&catalog).unwrap();
                project
                    .writer
                    .prepare_write(
                        &project.hub,
                        &catalog,
                        &server.endpoint.host,
                        (server.clock)().unwrap(),
                        &mut outbox,
                    )
                    .unwrap()
            },
        );
        server.drain_snapshots().await.unwrap();
        let hold_seq = hold_seq.expect("light ticker must install the real later hold");
        assert!(original_deadline_due);
        let completed = attempt
            .completed()
            .expect("actual older Git write and exact ack");
        assert!(hold_seq > completed.last.seq);
        mono.store(20_000, Ordering::SeqCst);
        unix.store(START + 20_000, Ordering::SeqCst);
        finish_snapshot_attempt(
            &project,
            &before,
            &attempt,
            &server.endpoint.host,
            ClockReading {
                mono_ms: 20_000,
                unix_ms: START + 20_000,
            },
        )
        .unwrap();
        let pending = installed_summary(&project).unwrap().fast_pending.unwrap();
        assert_eq!(pending.seq, hold_seq);
        let eligible = project.schedule.lock().unwrap().eligible(35_000);
        if !eligible {
            server.shutdown.send_replace(true);
            server.drain_snapshots().await.unwrap();
            panic!("older Q completion postponed the already granted later-hold deadline");
        }
        mono.store(35_000, Ordering::SeqCst);
        unix.store(START + 35_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        assert!(project.hub.lock().unwrap().snapshot_ack().unwrap().seq >= hold_seq);
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    #[tokio::test]
    async fn completed_prefix_keeps_window_when_only_first_fast_event_is_covered() {
        const START: u64 = 1_800_000_000_000;
        let (dir, root) = snapshot_fixture(60);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(START));
        let server = snapshot_server(
            &root,
            dir.path().join("runtime"),
            mono.clone(),
            unix.clone(),
        );
        server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        snapshot_hold(&project, START);
        let _ = server.execute(Request {
            directory: root.clone(),
            run: None,
            command: TaskCommand::Show { id: None },
        });
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let before = begin_snapshot_preparation(&project).unwrap();
        let first_seq = before.fast_pending.as_ref().unwrap().seq;
        let mut calls = 0;
        let mut hold_seq = None;
        let mut original_deadline_due = false;
        let attempt = project.writer.write_prepared_observed(
            &project.hub,
            &catalog,
            &server.endpoint.host,
            true,
            || {
                calls += 1;
                if calls == 2 {
                    // The sole writer has frozen Q and published its actual intent.
                    // A real later hold is installed by the existing full command
                    // boundary, then observed by the ordinary light ticker.
                    mono.store(10_000, Ordering::SeqCst);
                    unix.store(START + 10_000, Ordering::SeqCst);
                    let later_eid = snapshot_hold(&project, START + 10_000);
                    let _ = server.execute(Request {
                        directory: root.clone(),
                        run: None,
                        command: TaskCommand::Show { id: None },
                    });
                    server.tick().unwrap();
                    let observed = installed_summary(&project).unwrap();
                    hold_seq = project
                        .hub
                        .lock()
                        .unwrap()
                        .events(0)
                        .into_iter()
                        .find(|event| event.eid == later_eid)
                        .map(|event| event.seq);
                    assert_eq!(
                        observed.fast_pending.as_ref().map(|p| p.seq),
                        Some(first_seq)
                    );
                    assert_eq!(observed.fast_pending.as_ref().map(|p| p.max_seq), hold_seq);
                    assert!(observed.cursor >= hold_seq.unwrap());
                    original_deadline_due = !project.schedule.lock().unwrap().eligible(24_999)
                        && project.schedule.lock().unwrap().eligible(25_000);
                }
                Ok(unix.load(Ordering::SeqCst))
            },
            {
                let mut outbox = server.task_outbox(&catalog).unwrap();
                project
                    .writer
                    .prepare_write(
                        &project.hub,
                        &catalog,
                        &server.endpoint.host,
                        (server.clock)().unwrap(),
                        &mut outbox,
                    )
                    .unwrap()
            },
        );
        server.drain_snapshots().await.unwrap();
        let hold_seq = hold_seq.expect("light ticker must install the real later hold");
        assert!(original_deadline_due);
        let completed = attempt
            .completed()
            .expect("actual older Git write and exact ack");
        assert!(first_seq <= completed.last.seq && hold_seq > completed.last.seq);
        eprintln!(
            "actual installed window first={first_seq} max_fast={hold_seq} completed_Q={}",
            completed.last.seq
        );
        mono.store(20_000, Ordering::SeqCst);
        unix.store(START + 20_000, Ordering::SeqCst);
        finish_snapshot_attempt(
            &project,
            &before,
            &attempt,
            &server.endpoint.host,
            ClockReading {
                mono_ms: 20_000,
                unix_ms: START + 20_000,
            },
        )
        .unwrap();
        let pending = installed_summary(&project).unwrap().fast_pending.unwrap();
        assert_eq!(pending.seq, hold_seq);
        let eligible = project.schedule.lock().unwrap().eligible(25_000);
        if !eligible {
            server.shutdown.send_replace(true);
            server.drain_snapshots().await.unwrap();
            panic!("Q covered the first fast event but postponed the original window for its already installed higher fast event");
        }
        mono.store(25_000, Ordering::SeqCst);
        unix.store(START + 25_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        assert!(project.hub.lock().unwrap().snapshot_ack().unwrap().seq >= hold_seq);
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    #[tokio::test]
    async fn heartbeat_only_tail_is_compacted_at_periodic_deadline() {
        let (dir, root) = snapshot_fixture(60);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(1_800_000_000_000));
        let server = snapshot_server(
            &root,
            dir.path().join("runtime"),
            mono.clone(),
            unix.clone(),
        );
        let first = server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        let first_commit = first["commit"].as_str().unwrap().to_string();
        for offset in [0, 25_000, 30_000, 59_999] {
            mono.store(offset, Ordering::SeqCst);
            unix.store(1_800_000_000_000 + offset, Ordering::SeqCst);
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
            assert_eq!(
                snapshot_private(&root).as_deref(),
                Some(first_commit.as_str()),
                "heartbeat must not start the fast 25-second path"
            );
        }
        let heart = project
            .hub
            .lock()
            .unwrap()
            .events(0)
            .into_iter()
            .find(|event| event.is_pure_heartbeat())
            .expect("real maintenance must produce a confirmed renewal");
        mono.store(60_000, Ordering::SeqCst);
        unix.store(1_800_000_060_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let second = snapshot_private(&root).unwrap();
        assert_ne!(second, first_commit);
        {
            let live = project.hub.lock().unwrap();
            assert!(live.snapshot_ack().unwrap().seq >= heart.seq);
            let batch = live.events(0);
            assert!(!batch.iter().any(|event| event.eid == heart.eid));
        }
        let saved = snapshot_service::read_snapshot(&Catalog::load(&root).unwrap())
            .unwrap()
            .unwrap();
        assert!(
            !saved
                .archives
                .values()
                .any(|text| text.contains(&heart.eid)),
            "heartbeat receipt must not become permanent archive state"
        );
        // Keep Unix fixed so no new actual renew input is produced; move only
        // elapsed scheduling time through multiple intervals.
        for offset in [120_000, 180_000, 240_000] {
            mono.store(offset, Ordering::SeqCst);
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
            assert_eq!(snapshot_private(&root).as_deref(), Some(second.as_str()));
        }
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }
    #[tokio::test]
    async fn snapshot_and_heartbeat_events_do_not_create_snapshot_loop() {
        let (dir, root) = snapshot_fixture(60);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(1_800_000_000_000));
        let runtime = dir.path().join("runtime");
        let server = snapshot_server(&root, runtime.clone(), mono.clone(), unix.clone());
        let report = server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        let commit = report["commit"].as_str().unwrap().to_string();
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(server);
        let mono = Arc::new(AtomicU64::new(0));
        let server = snapshot_server(&root, runtime, mono.clone(), unix);
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(installed_summary(&project).unwrap().fast_pending.is_none());
        assert_eq!(
            installed_summary(&project).unwrap().periodic_pending_since,
            None
        );
        for now in [0, 25_000, 60_000, 120_000, 600_000] {
            mono.store(now, Ordering::SeqCst);
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
            assert_eq!(snapshot_private(&root).as_deref(), Some(commit.as_str()));
        }
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }
    #[tokio::test]
    async fn snapshot_restart_restores_uncovered_first_deadline_and_discards_covered_hold() {
        let (dir, root) = snapshot_fixture(600);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(1_800_000_000_000));
        let runtime = dir.path().join("runtime");
        let server = snapshot_server(&root, runtime.clone(), mono, unix.clone());
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        let eid = snapshot_hold(&project, unix.load(Ordering::SeqCst));
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(project);
        drop(server);
        unix.store(1_800_000_020_000, Ordering::SeqCst);
        let mono = Arc::new(AtomicU64::new(0));
        let server = snapshot_server(&root, runtime.clone(), mono.clone(), unix.clone());
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        for now in [0, 4_999] {
            mono.store(now, Ordering::SeqCst);
            unix.store(1_800_000_020_000 + now, Ordering::SeqCst);
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
            assert!(snapshot_private(&root).is_none());
        }
        mono.store(5_000, Ordering::SeqCst);
        unix.store(1_800_000_025_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let commit =
            snapshot_private(&root).expect("restart must retain only the remaining five seconds");
        let saved = snapshot_service::read_snapshot(&Catalog::load(&root).unwrap())
            .unwrap()
            .unwrap();
        assert!(saved.archives.values().any(|text| text.contains(&eid)));
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(project);
        drop(server);
        let mono = Arc::new(AtomicU64::new(0));
        let server = snapshot_server(&root, runtime, mono.clone(), unix);
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(installed_summary(&project).unwrap().fast_pending.is_none());
        for now in [0, 25_000, 120_000] {
            mono.store(now, Ordering::SeqCst);
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
            assert_eq!(snapshot_private(&root).as_deref(), Some(commit.as_str()));
        }
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_failure_episode_survives_alert_crash_window_and_successful_archive_restart() {
        const START: u64 = 1_800_000_000_000;
        let (dir, root) = snapshot_fixture(60);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(START));
        let runtime = dir.path().join("runtime");
        let server = snapshot_server(&root, runtime.clone(), mono.clone(), unix.clone());
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        let lock = project.common_dir.join("refs/towermd/snapshot.lock");
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        fs::write(&lock, "real Git CAS failure").unwrap();
        snapshot_hold(&project, START);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        mono.store(25_000, Ordering::SeqCst);
        unix.store(START + 25_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let first = project.schedule.lock().unwrap().status();
        assert_eq!(first["deferred_since"], START + 25_000);
        mono.store(30_000, Ordering::SeqCst);
        unix.store(START + 30_000, Ordering::SeqCst);
        assert!(server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None
            })
            .is_err());
        assert_eq!(
            project.schedule.lock().unwrap().status()["deferred_since"],
            first["deferred_since"],
            "manual failure must not reset since"
        );
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(project);
        drop(server);
        unix.store(START + 31_000, Ordering::SeqCst);
        let mono = Arc::new(AtomicU64::new(0));
        let server = snapshot_server(&root, runtime.clone(), mono.clone(), unix.clone());
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(
            project.schedule.lock().unwrap().status()["deferred_since"].is_null(),
            "restart without durable alert may discard prior failure observation"
        );
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        assert_eq!(
            project.schedule.lock().unwrap().status()["deferred_since"],
            START + 31_000
        );
        mono.store(61_001, Ordering::SeqCst);
        unix.store(START + 92_001, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let alerts = project
            .hub
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(0, START + 92_001)
            .unwrap()
            .events
            .into_iter()
            .filter(|event| event.kind == "alert.raised" && event.data["component"] == "snapshot")
            .collect::<Vec<_>>();
        assert_eq!(alerts.len(), 1);
        let alert = alerts[0].clone();
        assert_eq!(alert.data["since"], START + 31_000);
        // Crash window: the Hub accepted the alert, but the live Session has not
        // installed it and schedule has not recorded its durable eid.
        assert!(installed_summary(&project).unwrap().alert.is_none());
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(project);
        drop(server);
        let mono = Arc::new(AtomicU64::new(0));
        let server = snapshot_server(&root, runtime.clone(), mono.clone(), unix.clone());
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert_eq!(
            installed_summary(&project)
                .unwrap()
                .alert
                .as_ref()
                .unwrap()
                .eid,
            alert.eid
        );
        for now in [0, 61_000, 121_000, 181_000] {
            mono.store(now, Ordering::SeqCst);
            unix.store(START + 92_001 + now, Ordering::SeqCst);
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
            assert_eq!(
                project.schedule.lock().unwrap().status()["deferred_since"],
                START + 31_000
            );
            let events = project
                .hub
                .lock()
                .unwrap()
                .protocol()
                .unwrap()
                .subscribe(0, START + 92_001 + now)
                .unwrap()
                .events;
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == "alert.raised"
                        && event.data["component"] == "snapshot")
                    .count(),
                1
            );
        }
        fs::remove_file(&lock).unwrap();
        mono.store(241_000, Ordering::SeqCst);
        unix.store(START + 333_001, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let status = project.schedule.lock().unwrap().status();
        assert!(status["deferred_since"].is_null());
        assert!(status["deferred_reason"].is_null());
        let saved = snapshot_service::read_snapshot(&Catalog::load(&root).unwrap())
            .unwrap()
            .unwrap();
        assert!(saved
            .archives
            .values()
            .any(|text| text.contains(&alert.eid)));
        assert!(!project
            .hub
            .lock()
            .unwrap()
            .events(0)
            .iter()
            .any(|event| event.eid == alert.eid));
        assert_eq!(status["last"]["commit"], saved.commit);
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(project);
        drop(server);
        let mono = Arc::new(AtomicU64::new(0));
        let server = snapshot_server(&root, runtime, mono.clone(), unix.clone());
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(installed_summary(&project).unwrap().alert.is_none());
        assert!(project.schedule.lock().unwrap().status()["deferred_since"].is_null());
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        // A new episode can have exactly the same Unix since after a wall-clock
        // correction. Identity comes from the accepted eid and proven success.
        unix.store(START + 31_000, Ordering::SeqCst);
        snapshot_hold(&project, START + 31_000);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        fs::write(&lock, "new real Git CAS failure").unwrap();
        mono.store(25_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        mono.store(86_001, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let after = project
            .hub
            .lock()
            .unwrap()
            .events(0)
            .last()
            .map(|event| event.seq)
            .unwrap_or(0);
        let batch = project
            .hub
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(after, START + 31_000)
            .unwrap();
        let new = batch
            .events
            .iter()
            .find(|event| event.kind == "alert.raised")
            .expect("new episode must accept its own alert");
        assert_ne!(new.eid, alert.eid);
        assert_eq!(new.data["since"], alert.data["since"]);
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_backoff_skips_catalog_git_before_deadline() {
        use std::os::unix::fs::PermissionsExt;
        const CHILD: &str = "TOWERMD_H5_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let (dir, root) = snapshot_fixture(60);
            let gates = dir.path().join("gates");
            let bin = gates.join("bin");
            fs::create_dir_all(&bin).unwrap();
            let actual = Command::new("which").arg("git").output().unwrap();
            assert!(actual.status.success());
            let program = bin.join("git");
            fs::write(&program,r#"#!/bin/sh
if test -f "$TOWERMD_H5_GATES/armed" && test . -ef "$TOWERMD_H5_ROOT"; then
    cmd= sub= object= skip=0
    for argument in "$@"; do
        if test "$skip" = 1; then skip=0; continue; fi
        if test -z "$cmd" && test "$argument" = -c; then skip=1; continue; fi
        if test -z "$cmd"; then cmd=$argument
        elif test -z "$sub"; then sub=$argument
        elif test -z "$object"; then object=$argument
        fi
    done
    if test "$cmd" = worktree && test "$sub" = list; then echo worktree >> "$TOWERMD_H5_GATES/worktree"; fi
    if test "$cmd" = rev-parse; then echo probe >> "$TOWERMD_H5_GATES/probe"; fi
    if test "$cmd" = cat-file && test "$sub" = blob && test -f "$TOWERMD_H5_GATES/archive-oid"; then
        target=$(cat "$TOWERMD_H5_GATES/archive-oid")
        if test "$object" = "$target"; then echo archive >> "$TOWERMD_H5_GATES/archive"; fi
    fi
fi
exec "$TOWERMD_H5_GIT" "$@"
"#).unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "daemon::tests::snapshot_backoff_skips_catalog_git_before_deadline",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("TOWERMD_H5_ROOT", root.canonicalize().unwrap())
                .env("TOWERMD_H5_GATES", &gates)
                .env(
                    "TOWERMD_H5_GIT",
                    String::from_utf8(actual.stdout).unwrap().trim(),
                )
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        bin.display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(
                output.status.success(),
                "actual isolated tick subprocess failed"
            );
            return;
        }
        fn count(path: &Path) -> usize {
            fs::read_to_string(path).unwrap_or_default().lines().count()
        }
        const START: u64 = 1_800_000_000_000;
        let root = PathBuf::from(std::env::var_os("TOWERMD_H5_ROOT").unwrap());
        let gates = PathBuf::from(std::env::var_os("TOWERMD_H5_GATES").unwrap());
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(START));
        let server = snapshot_server(&root, gates.join("runtime"), mono.clone(), unix.clone());
        let report = server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        let original = report["commit"].as_str().unwrap().to_string();
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        let tree = snapshot_git(
            &root,
            &["ls-tree", "-r", &original, "--", "docs/tower/state/events"],
        );
        let oid = tree
            .split_whitespace()
            .nth(2)
            .expect("fixture must contain an actual archive blob");
        fs::write(gates.join("archive-oid"), oid).unwrap();
        fs::write(gates.join("armed"), "").unwrap();
        let lock = project.common_dir.join("refs/towermd/snapshot.lock");
        fs::write(&lock, "persistent actual Git CAS failure").unwrap();
        let error = server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap_err();
        assert!(
            error.contains("snapshot.lock"),
            "must reach actual failing Git CAS: {error}"
        );
        assert!(
            count(&gates.join("worktree")) > 0,
            "must count actual worktree list"
        );
        assert!(
            count(&gates.join("archive")) > 0,
            "must count actual archive blob reads"
        );
        let business = project
            .hub
            .lock()
            .unwrap()
            .projection()
            .unwrap()
            .objects
            .clone();
        let mut last_attempt = None;
        let mut attempts = 0;
        for second in 0..=3_600 {
            let now = second * 1_000;
            mono.store(now, Ordering::SeqCst);
            unix.store(START + now, Ordering::SeqCst);
            let next = project
                .schedule
                .lock()
                .unwrap()
                .next_attempt_at()
                .expect("real failure must establish a retry deadline");
            let before = (
                project.background_catalog_loads.load(Ordering::SeqCst),
                count(&gates.join("worktree")),
                count(&gates.join("archive")),
            );
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
            let after = (
                project.background_catalog_loads.load(Ordering::SeqCst),
                count(&gates.join("worktree")),
                count(&gates.join("archive")),
            );
            if now < next {
                assert_eq!(
                    after, before,
                    "actual heavy I/O before retry deadline at {now}, next={next}"
                );
            }
            if after.0 > before.0 {
                assert_eq!(
                    after.0,
                    before.0 + 1,
                    "one slot may invoke only one actual Catalog load"
                );
                attempts += 1;
                if let Some(previous) = last_attempt {
                    if now > 123_000 {
                        assert_eq!(
                            now - previous,
                            60_000,
                            "steady backoff must be capped by configured interval"
                        );
                    }
                }
                last_attempt = Some(now);
                eprintln!(
                    "actual retry mono={now} next={next} catalog={} worktree={} archive={}",
                    after.0, after.1, after.2
                );
            }
            assert_eq!(
                project.hub.lock().unwrap().projection().unwrap().objects,
                business,
                "unchanged business projection must never gain replayed state"
            );
        }
        assert!(
            (60..=65).contains(&attempts),
            "actual one-hour attempt count {attempts}"
        );
        let events = project.hub.lock().unwrap().events(0);
        let alerts = events
            .iter()
            .filter(|event| event.kind == "alert.raised" && event.data["component"] == "snapshot")
            .collect::<Vec<_>>();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].data["since"], START);
        assert_eq!(snapshot_private(&root).as_deref(), Some(original.as_str()));
        assert!(
            count(&gates.join("probe")) > 3_600,
            "lightweight actual source probes must continue through backoff"
        );
        let leases = project.hub.lock().unwrap().leases(START + 3_600_000);
        assert!(
            leases["main"].expires_at > START + 3_600_000,
            "actual main lease must remain live"
        );
        eprintln!("one-hour actual tick totals: attempts={attempts} catalog={} worktree={} archive={} source_probes={} alerts=1",project.background_catalog_loads.load(Ordering::SeqCst),count(&gates.join("worktree")),count(&gates.join("archive")),count(&gates.join("probe")));
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_busy_manual_uses_same_episode_and_shutdown_drains_actual_writer() {
        use std::os::unix::fs::PermissionsExt;
        const CHILD: &str = "TOWERMD_BUSY_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let (dir, root) = snapshot_fixture(60);
            let gates = dir.path().join("gates");
            let bin = gates.join("bin");
            fs::create_dir_all(&bin).unwrap();
            let git = Command::new("which").arg("git").output().unwrap();
            assert!(git.status.success());
            let wrapper = bin.join("git");
            fs::write(
                &wrapper,
                r#"#!/bin/sh
if test -f "$TOWERMD_BUSY_GATES/armed" && test . -ef "$TOWERMD_BUSY_ROOT"; then
    cmd= skip=0 snapshot_ref=0
    for argument in "$@"; do
        if test "$skip" = 1; then skip=0; continue; fi
        if test -z "$cmd" && test "$argument" = -c; then skip=1; continue; fi
        if test -z "$cmd"; then cmd=$argument; fi
        if test "$argument" = refs/towermd/snapshot; then snapshot_ref=1; fi
    done
    if test "$cmd" = update-ref && test "$snapshot_ref" = 1; then
        "$TOWERMD_BUSY_GIT" "$@" || exit "$?"
        touch "$TOWERMD_BUSY_GATES/entered"
        attempts=0
        while ! test -f "$TOWERMD_BUSY_GATES/release"; do
            attempts=$((attempts+1));if test "$attempts" -ge 1000; then exit 98; fi;sleep 0.01
        done
        exit 0
    fi
fi
exec "$TOWERMD_BUSY_GIT" "$@"
"#,
            )
            .unwrap();
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
            let output=Command::new(std::env::current_exe().unwrap()).args(["--exact","daemon::tests::snapshot_busy_manual_uses_same_episode_and_shutdown_drains_actual_writer","--nocapture"])
                .env(CHILD,"1").env("TOWERMD_BUSY_ROOT",root.canonicalize().unwrap()).env("TOWERMD_BUSY_GATES",&gates).env("TOWERMD_BUSY_GIT",String::from_utf8(git.stdout).unwrap().trim())
                .env("PATH",format!("{}:{}",bin.display(),std::env::var("PATH").unwrap_or_default())).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1").output().unwrap();
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(output.status.success());
            return;
        }
        const START: u64 = 1_800_000_000_000;
        let root = PathBuf::from(std::env::var_os("TOWERMD_BUSY_ROOT").unwrap());
        let gates = PathBuf::from(std::env::var_os("TOWERMD_BUSY_GATES").unwrap());
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(START));
        let runtime = gates.join("runtime");
        let server = snapshot_server(&root, runtime.clone(), mono.clone(), unix.clone());
        let first = server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        let old = first["commit"].as_str().unwrap().to_string();
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        snapshot_hold(&project, START);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        fs::write(gates.join("armed"), "").unwrap();
        mono.store(25_000, Ordering::SeqCst);
        unix.store(START + 25_000, Ordering::SeqCst);
        server.tick().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !gates.join("entered").exists() && Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if !gates.join("entered").exists() {
            fs::write(gates.join("release"), "").unwrap();
            server.shutdown.send_replace(true);
            server.drain_snapshots().await.unwrap();
            panic!("fixture did not reach actual published private CAS stage");
        }
        let published = snapshot_private(&root).unwrap();
        assert_ne!(published, old);
        // Status deliberately shows the last proved prefix. A real Show
        // command uses the shared full preparation path even if no Run exists.
        let _ = server.execute(Request {
            directory: root.clone(),
            run: None,
            command: TaskCommand::Show { id: None },
        });
        let visible = installed_summary(&project).unwrap();
        if visible.last.as_ref().map(|last| &last.commit) != Some(&published) {
            fs::write(gates.join("release"), "").unwrap();
            server.shutdown.send_replace(true);
            server.drain_snapshots().await.unwrap();
            panic!("real Show preparation did not install the published source");
        }
        let first_busy = server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        assert_eq!(first_busy["status"], "deferred");
        let since = project.schedule.lock().unwrap().status()["deferred_since"].clone();
        mono.store(86_001, Ordering::SeqCst);
        unix.store(START + 86_001, Ordering::SeqCst);
        server.tick().unwrap();
        let again = server
            .snapshot(SnapshotRequest {
                directory: root.clone(),
                run: None,
            })
            .unwrap();
        assert_eq!(again["status"], "deferred");
        assert_eq!(
            project.schedule.lock().unwrap().status()["deferred_since"],
            since
        );
        let active = project.background_catalog_loads.load(Ordering::SeqCst);
        assert_eq!(snapshot_private(&root).as_deref(), Some(published.as_str()));
        server.shutdown.send_replace(true);
        let drained = server.clone();
        let drain = tokio::spawn(async move { drained.drain_snapshots().await });
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            !drain.is_finished(),
            "shutdown must await the actual writer held at Git, not just a scheduling task"
        );
        server.tick().unwrap();
        assert_eq!(
            project.background_catalog_loads.load(Ordering::SeqCst),
            active,
            "shutdown cannot launch another heavy worker"
        );
        fs::write(gates.join("release"), "").unwrap();
        tokio::time::timeout(Duration::from_secs(5), drain)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let new = snapshot_private(&root).unwrap();
        assert_ne!(new, old);
        assert_eq!(
            project.hub.lock().unwrap().snapshot_ack().unwrap().commit,
            new
        );
        assert!(project.schedule.lock().unwrap().status()["deferred_since"].is_null());
        drop(project);
        drop(server);
        let server = snapshot_server(&root, runtime, Arc::new(AtomicU64::new(0)), unix);
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(
            installed_summary(&project).unwrap().alert.is_none(),
            "actual post-alert completion must remain closed after full restart"
        );
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn actual_ticker_maintains_fences_while_archive_proof_and_writer_are_blocked() {
        use std::os::unix::fs::PermissionsExt;
        const CHILD: &str = "TOWERMD_I2_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let temporary = tempfile::tempdir().unwrap();
            let gates = temporary.path().canonicalize().unwrap();
            let wrapper = gates.join("bin");
            fs::create_dir(&wrapper).unwrap();
            let real = Command::new("which").arg("git").output().unwrap();
            assert!(real.status.success());
            let program = wrapper.join("git");
            fs::write(
                &program,
                r#"#!/bin/sh
if test -f "$TOWERMD_I2_GATES/a-armed"; then
    cat_file=0
    blob=0
    month=0
    for argument in "$@"; do
        if test "$argument" = cat-file; then cat_file=1; fi
        if test "$argument" = blob; then blob=1; fi
    done
    if test "$cat_file$blob" = 11 && test . -ef "$TOWERMD_I2_GATES/a-directory"; then
        IFS= read -r target < "$TOWERMD_I2_GATES/a-blob"
        for argument in "$@"; do
            if test "$argument" = "$target"; then month=1; fi
        done
    fi
    if test "$month" = 1 && mkdir "$TOWERMD_I2_GATES/a-once" 2>/dev/null; then
        touch "$TOWERMD_I2_GATES/a-entered"
        attempts=0
        while ! test -f "$TOWERMD_I2_GATES/a-release"; do
            attempts=$((attempts + 1))
            if test "$attempts" -ge 1000; then exit 98; fi
            sleep 0.01
        done
    fi
fi
exec "$TOWERMD_I2_REAL_GIT" "$@"
"#,
            )
            .unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "daemon::tests::actual_ticker_maintains_fences_while_archive_proof_and_writer_are_blocked", "--nocapture"])
                .env(CHILD, "1").env("TOWERMD_I2_GATES", &gates)
                .env("TOWERMD_I2_REAL_GIT", String::from_utf8(real.stdout).unwrap().trim())
                .env("PATH", format!("{}:{}", wrapper.display(), std::env::var("PATH").unwrap()))
                .output().unwrap();
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(
                output.status.success(),
                "child status={:?}\n{}\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        fn git(root: &Path, args: &[&str]) -> String {
            let output = Command::new("git")
                .current_dir(root)
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout)
                .unwrap()
                .trim_end_matches('\n')
                .into()
        }
        fn project(root: &Path) {
            fs::create_dir(root).unwrap();
            git(root, &["init", "-qb", "main"]);
            towermd_core::init::init(root, &towermd_core::init::InitOptions { docs_dir: None })
                .unwrap();
            if root.file_name().unwrap() != "b" {
                fs::write(
                    root.join("docs/tower/arch/A.md"),
                    "---\ntype: Module\nid: A\npaths: [src/**]\n---\n",
                )
                .unwrap();
                fs::write(
                    root.join("docs/tower/specs/AC.md"),
                    "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
                )
                .unwrap();
            }
            let config = root.join(".towermd/config.yaml");
            fs::write(
                &config,
                format!(
                    "{}\nsnapshot_interval: 1\n",
                    towermd_core::config::Config::new(
                        if root.file_name().unwrap() == "b" {
                            "01-ticker-b"
                        } else {
                            "00-ticker-a"
                        },
                        "docs/tower",
                    )
                    .unwrap()
                    .to_yaml()
                ),
            )
            .unwrap();
            git(root, &["add", "."]);
            git(root, &["commit", "-qm", "ticker project"]);
        }
        async fn wait_file(path: &Path) -> bool {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !path.exists() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            path.exists()
        }
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(async {
            let temporary = tempfile::tempdir().unwrap();
            let gates = PathBuf::from(std::env::var_os("TOWERMD_I2_GATES").unwrap());
            let a = temporary.path().join("a"); let b = temporary.path().join("b"); project(&a); project(&b);
            let remote = temporary.path().join("remote.git");
            git(temporary.path(), &["clone", "-q", "--bare", b.to_str().unwrap(), remote.to_str().unwrap()]);
            git(&b, &["remote", "add", "origin", remote.to_str().unwrap()]); git(&b, &["fetch", "-q", "origin"]);
            fs::write(remote.join("hooks/pre-receive"), r#"#!/bin/sh
touch "$TOWERMD_I2_GATES/b-entered"
attempts=0
while ! test -f "$TOWERMD_I2_GATES/b-release"; do
    attempts=$((attempts + 1))
    if test "$attempts" -ge 1000; then exit 98; fi
    sleep 0.01
done
"#).unwrap();
            fs::set_permissions(remote.join("hooks/pre-receive"), fs::Permissions::from_mode(0o755)).unwrap();
            let start = now_ms().unwrap(); let time = Arc::new(AtomicU64::new(start)); let clock=time.clone();
            let mono=Arc::new(AtomicU64::new(0));let mono_clock=mono.clone();
            let trace_start=Instant::now(); let trace=Arc::new(Mutex::new(Vec::new()));let clock_trace=trace.clone();
            let runtime = temporary.path().join("runtime"); fs::create_dir(&runtime).unwrap();
            let (shutdown, _receiver) = tokio::sync::watch::channel(false);
            let server = Arc::new(Server { main_refresh: MainRefresh::default(),
            source_status_cache: Default::default(),
            mono_clock: Arc::new(move||mono_clock.load(Ordering::SeqCst)), clock: Arc::new(move||{let value=clock.load(Ordering::SeqCst);clock_trace.lock().unwrap().push((trace_start.elapsed().as_millis(),value));Ok(value)}), endpoint: Endpoint { address:"127.0.0.1".into(),port:0,pid:std::process::id(),instance:"ticker-proof".into(),host:"ticker-host".into() },finish_jobs: Arc::new(FinishJobs::default()),
            scan_jobs: ScanJobs::new(&runtime).unwrap(),runtime,projects:Mutex::new(BTreeMap::new()),registration:Mutex::new(()),delivery_tokens:Mutex::new(inbox_routes::DeliveryTokens::default()),shutdown });
            // A needs the original claimed Run and canonical archive proof.
            // B needs a real business tail and a blocked main writer, without
            // unrelated module/acceptance objects in every Git policy proof.
            server.execute(Request { directory:a.clone(),run:None,command:serde_json::from_value(json!({"action":"add","task":{"id":"T-A","title":"T-A","module":"A","allowed_paths":["src/**"],"acceptance":["AC"]}})).unwrap() }).unwrap();
            let b_catalog=Catalog::load(&b).unwrap();
            let b_project=server.project(&b_catalog).unwrap();
            let b_audit_eid=Uuid::now_v7().to_string();
            let b_audit=b_project.hub.lock().unwrap().protocol().unwrap().append(towermd_core::hub_protocol::NewEvent {
                eid:b_audit_eid.clone(),host:"host".into(),actor:"process:test".into(),kind:"future.audit".into(),layer:"L5".into(),obj:"audit".into(),data:json!({"safe":true}),guards:vec![]
            },start).unwrap();
            let towermd_core::hub_protocol::AppendReceipt::Event(b_audit)=b_audit else {panic!("actual audit append")};
            assert!(b_catalog.objects.is_empty());
            drop(b_project);
            let a_run = temporary.path().join("a-run"); git(&a, &["worktree","add","-qb","t/T-A",a_run.to_str().unwrap(),"main"]);
            let run = server.execute(Request { directory:a_run.clone(),run:None,command:TaskCommand::Claim {id:"T-A".into(),harness:None,harness_version:None} }).unwrap();
            let ac = Catalog::load(&a_run).unwrap(); let bc=Catalog::load(&b).unwrap();
            let ap=server.project(&ac).unwrap(); let bp=server.project(&bc).unwrap();
            assert_eq!(ap.writer.write_prepared(&ap.hub,&ac,&server.endpoint.host,true,||Ok(start), { let mut outbox=server.task_outbox(&ac).unwrap(); ap.writer.prepare_write(&ap.hub,&ac,&server.endpoint.host, (server.clock)().unwrap(), &mut outbox).unwrap() }).unwrap().status,"written");
            let applied=ap.hub.lock().unwrap().applied_seq();
            let run_before=ap.hub.lock().unwrap().leases(start)["module:A"].clone();
            let month=git(&a_run,&["ls-tree","-r","--name-only","refs/towermd/snapshot","--","docs/tower/state/events"]);
            let blob=git(&a_run,&["rev-parse",&format!("refs/towermd/snapshot:{}",month.lines().next().unwrap())]);
            fs::write(gates.join("a-blob"),blob).unwrap(); fs::write(gates.join("a-root"),a_run.canonicalize().unwrap().to_str().unwrap()).unwrap(); std::os::unix::fs::symlink(a_run.canonicalize().unwrap(), gates.join("a-directory")).unwrap();
            // Finish only the registered context/reconciliation phase while
            // the original clock is still before the periodic deadline. The
            // first real B snapshot and push must still enter via the ticker.
            assert_eq!(time.load(Ordering::SeqCst),start);
            assert!(snapshot_service::read_snapshot(&bc).unwrap().is_none());
            assert!(bp.hub.lock().unwrap().snapshot_ack().is_none());
            assert!(!gates.join("b-entered").exists());
            eprintln!("I2_ORIGIN before_warm_real={} mono={} unix={}",now_ms().unwrap(),(server.mono_clock)(),time.load(Ordering::SeqCst));
            server.tick().unwrap();
            server.drain_snapshots().await.unwrap();
            server.main_refresh.drain().await.unwrap();
            assert_eq!(time.load(Ordering::SeqCst),start);
            assert!(ap.snapshot_context.lock().unwrap().is_some());
            assert!(bp.snapshot_context.lock().unwrap().is_some());
            assert!(snapshot_service::read_snapshot(&bc).unwrap().is_none());
            assert!(bp.hub.lock().unwrap().snapshot_ack().is_none());
            assert!(!gates.join("b-entered").exists());
            assert!(installed_summary(&bp).unwrap().fast_pending.is_none(), "B exercises the one-second periodic path, never a synthetic fast event");
            eprintln!("I2_WARM_CONTEXT unix_unchanged=true B_snapshot=false B_ack=false B_push=false catalogA={} catalogB={}",ap.background_catalog_loads.load(Ordering::SeqCst),bp.background_catalog_loads.load(Ordering::SeqCst));
            time.store(start+2000,Ordering::SeqCst);mono.store(2000,Ordering::SeqCst);
            eprintln!("I2_ORIGIN timed_start_real={} mono={} unix={}",now_ms().unwrap(),(server.mono_clock)(),time.load(Ordering::SeqCst));
            let (stop,ticker)=start_ticker(server.clone());
            assert!(Arc::ptr_eq(&server.projects().unwrap()[0], &ap));
            assert!(bp.schedule.lock().unwrap().eligible(2000), "t0 represents two elapsed seconds in both trusted clocks");
            let barrier_start=Instant::now();
            let b_entered=wait_file(&gates.join("b-entered")).await;
            eprintln!("I2_PHASE B_entered={b_entered} barrier_ms={} catalogA={} catalogB={} clock_calls={:?}",barrier_start.elapsed().as_millis(),ap.background_catalog_loads.load(Ordering::SeqCst),bp.background_catalog_loads.load(Ordering::SeqCst),trace.lock().unwrap());
            if !b_entered { fs::write(gates.join("b-release"),"").unwrap(); server.shutdown.send(true).unwrap(); server.main_refresh.drain().await.unwrap(); server.drain_snapshots().await.unwrap(); stop.send(true).unwrap(); ticker.await.unwrap(); panic!("actual B background writer did not reach remote push"); }
            let old_a=ap.hub.lock().unwrap().leases(start+2000)["main"].clone();
            let old_b=bp.hub.lock().unwrap().leases(start+2000)["main"].clone();
            git(&a,&["commit","--allow-empty","-qm","advance A source before slow proof"]);
            fs::write(gates.join("a-armed"),"").unwrap();
            // Warm main observation is admitted at mono=5000. Only advance
            // the injected clocks after the real source change and arming;
            // the original real five-second proof watchdog is unchanged.
            {
                let hint=trailers_routes::project_hint(&ap).unwrap();
                assert!(hint);
                let schedule=ap.main_observer.lock().unwrap();
                assert!(!schedule.admitted(hint,2000));
                assert!(schedule.admitted(hint,5000));
            }
            assert_eq!(time.load(Ordering::SeqCst),start+2000);
            time.store(start+5000,Ordering::SeqCst);mono.store(5000,Ordering::SeqCst);
            let a_entered=wait_file(&gates.join("a-entered")).await;
            time.store(start+33_000,Ordering::SeqCst);mono.store(33_000,Ordering::SeqCst);
            let deadline=Instant::now()+Duration::from_secs(3);
            let mut renewed=false;
            while Instant::now()<deadline {
                let a_now=ap.hub.lock().unwrap().leases(start+33_000).get("main").cloned();
                let b_now=bp.hub.lock().unwrap().leases(start+33_000).get("main").cloned();
                renewed=a_now.as_ref().is_some_and(|l|l.fence()==old_a.fence()&&l.expires_at>old_a.expires_at)&&b_now.as_ref().is_some_and(|l|l.fence()==old_b.fence()&&l.expires_at>old_b.expires_at);
                if renewed {break;} tokio::time::sleep(Duration::from_millis(10)).await;
            }
            eprintln!("actual archive_entered={a_entered} ticker_same_epoch_renewed={renewed} oldA={} oldB={}",old_a.expires_at,old_b.expires_at);
            let mut twice=false;
            let mut run_renewed=false;
            if renewed {
                let crossed=old_b.expires_at+1000; time.store(crossed,Ordering::SeqCst);mono.store(crossed-start,Ordering::SeqCst);
                let deadline=Instant::now()+Duration::from_secs(3);
                while Instant::now()<deadline {
                    let leases=bp.hub.lock().unwrap().leases(crossed); twice=leases.get("main").is_some_and(|l|l.fence()==old_b.fence()&&l.expires_at>crossed+90_000);
                    if twice {break;} tokio::time::sleep(Duration::from_millis(10)).await;
                }
                eprintln!("actual original_ttl_crossed={crossed} second_ticker_renewal={twice}");
                let status=server.execute(Request {directory:a_run.clone(),run:run["id"].as_str().map(str::to_owned),command:TaskCommand::Status}).unwrap();
                assert_eq!(status["seq"],applied); assert_eq!(status["pending_verification"],true);
                assert_eq!(status["runs"][0]["leases"],run["leases"]);
                let run_after=ap.hub.lock().unwrap().leases(crossed)["module:A"].clone();
                run_renewed=run_after.fence()==run_before.fence()&&run_after.expires_at>run_before.expires_at;
                eprintln!("actual run_fence_preserved={run_renewed} expiry_before={} expiry_after={} last_verified_seq={applied}",run_before.expires_at,run_after.expires_at);
            }
            let drain_before=bp.hub.lock().unwrap().leases(time.load(Ordering::SeqCst)).get("main").cloned();
            server.shutdown.send(true).unwrap();
            let drain_server=server.clone(); let drain=tokio::spawn(async move {drain_server.main_refresh.drain().await.unwrap();drain_server.drain_snapshots().await.unwrap();});
            fs::write(gates.join("a-release"),"").unwrap();
            let drain_now=time.load(Ordering::SeqCst)+33_000; time.store(drain_now,Ordering::SeqCst);mono.store(drain_now-start,Ordering::SeqCst);
            let deadline=Instant::now()+Duration::from_secs(3); let mut drain_renewed=false;
            while Instant::now()<deadline {
                let lease=bp.hub.lock().unwrap().leases(drain_now).get("main").cloned();
                drain_renewed=lease.as_ref().zip(drain_before.as_ref()).is_some_and(|(new,old)|new.fence()==old.fence()&&new.expires_at>old.expires_at);
                if drain_renewed {break;} tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let waiting=!drain.is_finished();
            let owns=Hub::open(&bp.common_dir.join("towermd")).is_err();
            eprintln!("actual shutdown_drain_waiting={waiting} owner_held={owns} ticker_renewed_during_drain={drain_renewed}");
            fs::write(gates.join("b-release"),"").unwrap();
            tokio::time::timeout(Duration::from_secs(10),drain).await.unwrap().unwrap();
            stop.send(true).unwrap(); ticker.await.unwrap();
            let b_saved=snapshot_service::read_snapshot(&Catalog::load(&b).unwrap()).unwrap().unwrap();
            let b_ack=bp.hub.lock().unwrap().snapshot_ack().unwrap().clone();
            assert_ne!(b_saved.commit,bc.revision,"B must really publish its first new Git snapshot");
            assert_eq!(b_ack.commit,b_saved.commit);
            assert_eq!(b_ack.seq,b_saved.snapshot.through_seq);
            assert_eq!(b_ack.epoch,b_saved.snapshot.writer_epoch);
            assert!(b_ack.seq>=b_audit.seq);
            assert_eq!(git(&remote,&["rev-parse","refs/heads/main"]),b_saved.commit);
            assert!(b_saved.archives.values().flat_map(|text|text.lines()).any(|line|serde_json::from_str::<towermd_core::hub_protocol::Event>(line).is_ok_and(|event|event.eid==b_audit_eid&&event.kind=="future.audit"&&event.actor=="process:test")));
            assert!(a_entered,"real registered A archive proof never entered");
            assert!(renewed,"unique real ticker stopped automatic main renewal behind slow A proof");
            assert!(twice,"real ticker did not renew beyond original120s boundary");
            assert!(run_renewed,"same-project complete Run fences must renew without business proof");
            assert!(waiting,"shutdown must wait for actual blocked B writer");
            assert!(owns,"owner released during drain");
            assert!(drain_renewed,"ticker stopped lease maintenance while actual writer was draining");
        });
    }

    #[test]
    fn subscription_cursor_before_recovery_floor_requires_snapshot() {
        let floor = (1_u64 << 32) + 12;
        for after in [0, 12, floor - 1] {
            let error = validate_subscription_cursor(after, floor, floor + 3).unwrap_err();
            assert!(error.contains("请从快照加载"), "{error}");
        }
    }

    #[test]
    fn subscription_cursor_accepts_floor_and_tail_but_rejects_future() {
        let floor = (1_u64 << 32) + 12;
        for after in floor..=floor + 3 {
            assert!(validate_subscription_cursor(after, floor, floor + 3).is_ok());
        }
        assert!(validate_subscription_cursor(floor + 4, floor, floor + 3)
            .unwrap_err()
            .contains("超过 hub 连续水位"));
        assert!(validate_subscription_cursor(0, 0, 0).is_ok());
        assert!(validate_subscription_cursor(0, 0, 3).is_ok());
    }

    #[test]
    fn project_binding_drop_unlocks_even_with_inherited_handle() {
        let runtime = tempfile::tempdir().unwrap();
        let common = tempfile::tempdir().unwrap();
        let lock = bind_project(runtime.path(), "descriptor-owner", common.path()).unwrap();
        let inherited = lock.0.try_clone().unwrap();
        drop(lock);
        let rebound = bind_project(runtime.path(), "descriptor-owner", common.path());
        assert!(rebound.is_ok(), "{rebound:?}");
        drop(inherited);
    }

    #[test]
    fn clone_identity_binding_survives_daemon_restart() {
        let runtime = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let lock = bind_project(runtime.path(), "same-id", first.path()).unwrap();
        assert!(bind_project(runtime.path(), "same-id", first.path()).is_err());
        drop(lock);
        let error = bind_project(runtime.path(), "same-id", second.path()).unwrap_err();
        assert!(error.contains("clone"), "{error}");
        assert!(bind_project(runtime.path(), "same-id", first.path()).is_ok());
        assert!(bind_project(runtime.path(), "other-id", second.path()).is_ok());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn offline_twenty_minutes_of_http_activity_preserve_run_epoch() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("repo");
        let runtime = temporary.path().join("runtime");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&runtime).unwrap();
        fn git(root: &Path, args: &[&str]) -> String {
            let output = Command::new("git")
                .current_dir(root)
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            String::from_utf8(output.stdout).unwrap().trim().into()
        }
        git(&root, &["init", "-qb", "main"]);
        towermd_core::init::init(
            &root,
            &towermd_core::init::InitOptions {
                docs_dir: Some("docs/tower".into()),
            },
        )
        .unwrap();
        fs::write(
            root.join("docs/tower/arch/A.md"),
            "---\ntype: Module\nid: A\npaths: [src/a/**]\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/tower/specs/AC-A.md"),
            "---\ntype: Acceptance\nid: AC-A\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "fixture"]);
        let revision = git(&root, &["rev-parse", "main"]);
        git(
            &root,
            &["update-ref", "refs/remotes/origin/main", &revision],
        );
        git(
            &root,
            &[
                "remote",
                "add",
                "origin",
                "/nonexistent/towermd-twenty-minutes",
            ],
        );
        let worktree = temporary.path().join("run");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-qb",
                "t/T-1",
                worktree.to_str().unwrap(),
                "main",
            ],
        );
        let start = now_ms().unwrap();
        let time = Arc::new(AtomicU64::new(start));
        let clock = time.clone();
        // Each simulated minute advances both production clock domains.
        let elapsed_clock = time.clone();
        let (shutdown, _shutdown_receiver) = tokio::sync::watch::channel(false);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = Arc::new(Server {
            endpoint: Endpoint {
                address: "127.0.0.1".into(),
                port,
                pid: std::process::id(),
                instance: "twenty-minute-test".into(),
                host: "test-host".into(),
            },
            finish_jobs: Arc::new(FinishJobs::default()),
            scan_jobs: ScanJobs::new(&runtime).unwrap(),
            runtime,
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
            delivery_tokens: Mutex::new(inbox_routes::DeliveryTokens::default()),
            shutdown,
            main_refresh: MainRefresh::default(),
            source_status_cache: Default::default(),
            mono_clock: Arc::new(move || {
                elapsed_clock.load(Ordering::SeqCst).saturating_sub(start)
            }),
            clock: Arc::new(move || Ok(clock.load(Ordering::SeqCst))),
        });
        let app = Router::new()
            .route("/command", post(command))
            .with_state(server.clone());
        let serving = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let address = format!("http://127.0.0.1:{port}/command");
        async fn call(address: &str, directory: &Path, action: Value) -> Value {
            let address = address.to_string();
            let directory = directory.to_path_buf();
            tokio::task::spawn_blocking(move || {
                let response = reqwest::blocking::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(2))
                    .build()
                    .unwrap()
                    .post(address)
                    .header("x-towermd-instance", "twenty-minute-test")
                    .json(&json!({"directory": directory, "run": null, "command": action}))
                    .send()
                    .unwrap();
                let status = response.status();
                let value: Value = response.json().unwrap();
                assert!(status.is_success(), "{value}");
                value
            })
            .await
            .unwrap()
        }
        call(
            &address,
            &root,
            json!({"action":"add", "task": {
                "id":"T-1", "title":"test", "module":"A", "touches":[],
                "allowed_paths":["src/a/**"], "depends_on":[], "acceptance":["AC-A"], "decisions":[]
            }}),
        )
        .await;
        let run = call(
            &address,
            &worktree,
            json!({"action":"claim", "id":"T-1", "harness":null, "harness_version":null}),
        )
        .await;
        let run_id = run["id"].clone();
        let fences = run["leases"].as_array().unwrap().clone();
        assert!(!fences.is_empty());
        for fence in &fences {
            assert!(fence["name"].is_string());
            assert!(fence["epoch"].is_u64());
        }
        for minute in [5_u64, 10, 15, 20] {
            time.store(start + minute * 60_000, Ordering::SeqCst);
            let status = call(&address, &worktree, json!({"action":"status"})).await;
            let active = status["runs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|value| value["id"] == run_id)
                .unwrap();
            assert_eq!(active["state"], "running");
            assert_eq!(active["leases"], run["leases"]);
            let leases = status["leases"].as_array().unwrap();
            for fence in &fences {
                let lease = leases
                    .iter()
                    .find(|lease| lease["name"] == fence["name"] && lease["holder"] == run_id)
                    .unwrap();
                assert_eq!(lease["epoch"], fence["epoch"]);
                assert!(lease["expires_at"].as_u64().unwrap() > start + minute * 60_000);
            }
            let maintenance = server.clone();
            tokio::task::spawn_blocking(move || maintenance.tick())
                .await
                .unwrap()
                .unwrap();
            server.drain_snapshots().await.unwrap();
        }
        let shown = call(&address, &worktree, json!({"action":"show", "id":null})).await;
        assert_eq!(shown["id"], run_id);
        assert_eq!(shown["state"], "running");
        // 不再有活动时仍按生产 TTL 过期，证明注入没有绕过租约硬门禁。
        time.store(start + 36 * 60_000, Ordering::SeqCst);
        let maintenance = server.clone();
        tokio::task::spawn_blocking(move || maintenance.tick())
            .await
            .unwrap()
            .unwrap();
        server.drain_snapshots().await.unwrap();
        {
            let projects = server.projects().unwrap();
            assert_eq!(projects.len(), 1);
            let live = projects[0].hub.lock().unwrap();
            let released = live
                .events(0)
                .into_iter()
                .find(|event| event.kind == "task.released" && event.data["run"]["id"] == run_id)
                .expect(
                    "TTL maintenance must append the actual release while snapshot is backed off",
                );
            let observation = live.snapshot_summary().unwrap();
            assert!(observation.cursor >= released.seq);
            assert!(
                observation
                    .pending_through
                    .is_some_and(|seq| seq >= released.seq),
                "accepted release must be installed and observed before any later status query"
            );
        }
        let status = call(&address, &root, json!({"action":"status"})).await;
        let expired = status["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["id"] == run_id)
            .unwrap();
        assert_eq!(expired["state"], "released");
        server.shutdown.send(true).unwrap();
        server.main_refresh.drain().await.unwrap();
        server.drain_snapshots().await.unwrap();
        serving.abort();
    }
    #[cfg(unix)]
    #[test]
    fn daemon_spawn_descriptor_cleanup_preserves_exec_errors() {
        let mut command = Command::new("/nonexistent/towermd-executable");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        prepare_daemon_spawn(&mut command).unwrap();
        let error = command.spawn().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn running_websocket_reports_snapshot_required_and_closes_after_compaction() {
        use std::io::Read;
        use towermd_core::hub_protocol::{AcquireLease, HubProtocol};
        let directory = tempfile::tempdir().unwrap();
        fn git(root: &Path, args: &[&str]) {
            let out = Command::new("git")
                .current_dir(root)
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
        }
        git(directory.path(), &["init", "-qb", "main"]);
        towermd_core::init::init(
            directory.path(),
            &towermd_core::init::InitOptions { docs_dir: None },
        )
        .unwrap();
        git(directory.path(), &["add", "."]);
        git(directory.path(), &["commit", "-qm", "WS context"]);
        git(directory.path(), &["switch", "-qc", "ws"]);
        let catalog = Catalog::load(directory.path()).unwrap();
        let runtime = directory.path().join("runtime");
        let inputs = towermd_core::recovery_outbox::RecoveryInputs::read(
            &runtime
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            &catalog,
        )
        .unwrap();
        let mut local = Hub::open(&catalog.common_dir.join("towermd")).unwrap();
        let grant = local
            .lease_acquire(
                AcquireLease {
                    host: "host".into(),
                    names: vec!["main".into()],
                    holder: "snapshot:host".into(),
                    ttl_ms: 1000,
                },
                1,
            )
            .unwrap();
        let _fence = grant.leases[0].fence();
        let session = Arc::new(Mutex::new(
            ProjectSession::new(Box::new(local), &catalog, &inputs, 1).unwrap(),
        ));
        let gate = Arc::new(tokio::sync::Notify::new());
        let route_session = session.clone();
        let route_gate = gate.clone();
        let route_root = catalog.root.clone();
        let route_runtime = runtime.clone();
        let (shutdown, _receiver) = tokio::sync::watch::channel(false);
        let app = Router::new().route(
            "/events",
            get(move |upgrade: WebSocketUpgrade| {
                let session = route_session.clone();
                let gate = route_gate.clone();
                let shutdown = shutdown.subscribe();
                let root = route_root.clone();
                let runtime = route_runtime.clone();
                async move {
                    upgrade.on_upgrade(move |socket| async move {
                        gate.notified().await;
                        stream_events(
                            socket,
                            session,
                            0,
                            shutdown,
                            Arc::new(|| Ok(2)),
                            StreamRecovery {
                                root,
                                runtime,
                                uncertainty: Default::default(),
                                producer: Arc::new(Mutex::new(())),
                            },
                        )
                        .await
                    })
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let socket=tokio::task::spawn_blocking(move||{let mut socket=std::net::TcpStream::connect(("127.0.0.1",port)).unwrap();socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();write!(socket,"GET /events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").unwrap();let mut header=vec![];while !header.ends_with(b"\r\n\r\n"){let mut byte=[0];socket.read_exact(&mut byte).unwrap();header.push(byte[0]);}assert!(header.starts_with(b"HTTP/1.1 101"));socket}).await.unwrap();
        let report = SnapshotWriter::default()
            .write(
                &session,
                &catalog,
                "host",
                true,
                || Ok(2),
                &mut towermd_core::recovery_outbox::RecoveryOutbox::open(
                    &runtime
                        .join("recovery-outbox")
                        .join(catalog.config.project_id()),
                    towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        // The short historical lease is truly renewed before checkpoint; its heartbeat is covered.
        assert_eq!(report.seq, 2);
        gate.notify_one();
        let body = tokio::task::spawn_blocking(move || {
            let mut socket = socket;
            let mut header = [0; 2];
            socket.read_exact(&mut header).unwrap();
            assert_eq!(header[0], 0x81);
            let length = if header[1] & 0x7f == 126 {
                let mut bytes = [0; 2];
                socket.read_exact(&mut bytes).unwrap();
                u16::from_be_bytes(bytes) as usize
            } else {
                (header[1] & 0x7f) as usize
            };
            let mut bytes = vec![0; length];
            socket.read_exact(&mut bytes).unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            socket.read_exact(&mut header).unwrap();
            assert_eq!(header[0] & 0x0f, 8);
            body
        })
        .await
        .unwrap();
        assert_eq!(
            body["error"]["SnapshotRequired"]["head"]["floor_seq"],
            report.seq
        );
        assert_eq!(
            body["error"]["SnapshotRequired"]["checkpoint"]["through_seq"],
            report.seq
        );
        serving.abort();
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn background_and_established_websocket_reject_later_unproved_canonical_tail() {
        use std::io::Read;
        use towermd_core::hub_protocol::NewEvent;
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir(&root).unwrap();
        fn git(root: &Path, args: &[&str]) {
            let out = Command::new("git")
                .current_dir(root)
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
        }
        git(&root, &["init", "-qb", "main"]);
        towermd_core::init::init(&root, &towermd_core::init::InitOptions { docs_dir: None })
            .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "daemon proof fixture"]);
        let runtime = temporary.path().join("runtime");
        fs::create_dir(&runtime).unwrap();
        let catalog = Catalog::load(&root).unwrap();
        let (shutdown, _receiver) = tokio::sync::watch::channel(false);
        let server = Arc::new(Server {
            main_refresh: MainRefresh::default(),
            source_status_cache: Default::default(),
            mono_clock: monotonic_clock(),
            clock: Arc::new(|| Ok(1000)),
            endpoint: Endpoint {
                address: "127.0.0.1".into(),
                port: 0,
                pid: std::process::id(),
                instance: "proof-matrix".into(),
                host: "host".into(),
            },
            finish_jobs: Arc::new(FinishJobs::default()),
            scan_jobs: ScanJobs::new(&runtime).unwrap(),
            runtime: runtime.clone(),
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
            delivery_tokens: Mutex::new(inbox_routes::DeliveryTokens::default()),
            shutdown,
        });
        let project = server.project(&catalog).unwrap();
        let audit = NewEvent {
            eid: Uuid::now_v7().to_string(),
            host: "host".into(),
            actor: "process:test".into(),
            kind: "future.audit".into(),
            layer: "L5".into(),
            obj: "audit".into(),
            data: json!({"safe":true}),
            guards: vec![],
        };
        project
            .hub
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .append(audit.clone(), 1000)
            .unwrap();
        let route_hub = project.hub.clone();
        let route_producer = project.producer.clone();
        let route_root = root.clone();
        let route_runtime = runtime.clone();
        let route_shutdown = server.shutdown.clone();
        let app = Router::new().route(
            "/events",
            get(move |upgrade: WebSocketUpgrade| {
                let hub = route_hub.clone();
                let producer = route_producer.clone();
                let root = route_root.clone();
                let runtime = route_runtime.clone();
                let shutdown = route_shutdown.subscribe();
                async move {
                    upgrade.on_upgrade(move |socket| {
                        stream_events(
                            socket,
                            hub,
                            0,
                            shutdown,
                            Arc::new(|| Ok(1000)),
                            StreamRecovery {
                                root,
                                runtime,
                                uncertainty: Default::default(),
                                producer,
                            },
                        )
                    })
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut socket=tokio::task::spawn_blocking(move||{let mut socket=std::net::TcpStream::connect(("127.0.0.1",port)).unwrap();socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();write!(socket,"GET /events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").unwrap();let mut header=vec![];while !header.ends_with(b"\r\n\r\n"){let mut byte=[0];socket.read_exact(&mut byte).unwrap();header.push(byte[0]);}assert!(header.starts_with(b"HTTP/1.1 101"));socket}).await.unwrap();
        let (returned, first) = tokio::task::spawn_blocking(move || {
            let mut header = [0; 2];
            socket.read_exact(&mut header).unwrap();
            assert_eq!(header[0], 0x81);
            let n = if header[1] & 0x7f == 126 {
                let mut b = [0; 2];
                socket.read_exact(&mut b).unwrap();
                u16::from_be_bytes(b) as usize
            } else {
                (header[1] & 0x7f) as usize
            };
            let mut bytes = vec![0; n];
            socket.read_exact(&mut bytes).unwrap();
            (socket, serde_json::from_slice::<Value>(&bytes).unwrap())
        })
        .await
        .unwrap();
        socket = returned;
        assert_eq!(first["eid"], audit.eid);
        let before = project.hub.lock().unwrap().applied_seq();
        assert_eq!(before, 1);
        project.hub.lock().unwrap().protocol().unwrap().append(NewEvent{eid:Uuid::now_v7().to_string(),host:"host".into(),actor:"process:towermd-recovery".into(),kind:"merge.done".into(),layer:"L5".into(),obj:"T".into(),data:json!({"commit":"a".repeat(40),"task":{"type":"Task","id":"T","state":"merged"},"proof":{"schema":1}}),guards:vec![]},1000).unwrap();
        let actual_refs = || {
            let output = Command::new("git")
                .current_dir(&root)
                .args([
                    "for-each-ref",
                    "--format=%(refname):%(objectname)",
                    "refs/heads/main",
                    "refs/towermd/snapshot",
                ])
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        };
        let refs_before = actual_refs();
        server.tick().unwrap();
        // tick schedules one bounded business worker; wait for that actual worker,
        // then preserve the original no-write/no-apply and established-WS checks.
        tokio::time::timeout(Duration::from_secs(5), server.drain_snapshots())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(Catalog::load(&root).unwrap().revision, catalog.revision);
        assert_eq!(actual_refs(), refs_before);
        assert!(snapshot_service::read_snapshot(&catalog).unwrap().is_none());
        assert!(project.hub.lock().unwrap().snapshot_ack().is_none());
        assert!(project.hub.lock().unwrap().leases(1000).is_empty());
        assert_eq!(project.hub.lock().unwrap().applied_seq(), before);
        assert!(project
            .hub
            .lock()
            .unwrap()
            .projection()
            .unwrap()
            .objects
            .is_empty());
        assert!(project.background.lock().unwrap().is_none());
        let closed = tokio::task::spawn_blocking(move || {
            let mut header = [0; 2];
            match socket.read_exact(&mut header) {
                Err(error) => matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ),
                Ok(()) => header[0] & 0xf == 8,
            }
        })
        .await
        .unwrap();
        assert!(
            closed,
            "established subscription must close without delivering unproved merge"
        );
        assert_eq!(project.hub.lock().unwrap().applied_seq(), before);
        serving.abort();
    }
    fn rebuild_git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    fn rebuild_fixture(
        private: bool,
    ) -> (
        tempfile::TempDir,
        Arc<Server>,
        PathBuf,
        towermd_core::hub_protocol::NewEvent,
    ) {
        rebuild_fixture_with_module_path(private, "docs/tower/arch/A.md")
    }

    fn rebuild_fixture_with_module_path(
        private: bool,
        module_path: &str,
    ) -> (
        tempfile::TempDir,
        Arc<Server>,
        PathBuf,
        towermd_core::hub_protocol::NewEvent,
    ) {
        use towermd_core::hub_protocol::NewEvent;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&runtime).unwrap();
        rebuild_git(&root, &["init", "-qb", "main"]);
        towermd_core::init::init(
            &root,
            &towermd_core::init::InitOptions {
                docs_dir: Some("docs/tower".into()),
            },
        )
        .unwrap();
        fs::write(
            root.join(module_path),
            "---\ntype: Module\nid: A\npaths: [src/a/**]\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/tower/specs/AC-A.md"),
            "---\ntype: Acceptance\nid: AC-A\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        rebuild_git(&root, &["add", "."]);
        rebuild_git(&root, &["commit", "-qm", "recovery daemon fixture"]);
        let (shutdown, _receiver) = tokio::sync::watch::channel(false);
        let server = Arc::new(Server {
            main_refresh: MainRefresh::default(),
            source_status_cache: Default::default(),
            mono_clock: monotonic_clock(),
            clock: Arc::new(|| Ok(2000)),
            endpoint: Endpoint {
                address: "127.0.0.1".into(),
                port: 0,
                pid: std::process::id(),
                instance: Uuid::now_v7().to_string(),
                host: "fixture-host".into(),
            },
            finish_jobs: Arc::new(FinishJobs::default()),
            scan_jobs: ScanJobs::new(&runtime).unwrap(),
            runtime,
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
            delivery_tokens: Mutex::new(inbox_routes::DeliveryTokens::default()),
            shutdown,
        });
        let catalog = Catalog::load(&root).unwrap();
        server.project(&catalog).unwrap(); // Original registered empty owner exists before any snapshot.
        let run_tree = temp.path().join("run");
        rebuild_git(
            &root,
            &[
                "worktree",
                "add",
                "-qb",
                "t/T-1",
                run_tree.to_str().unwrap(),
                "main",
            ],
        );
        if !private {
            rebuild_git(&root, &["switch", "--detach"]);
        }
        let catalog = Catalog::load(&run_tree).unwrap();
        let inputs = server.recovery_inputs(&catalog).unwrap();
        let history = Mutex::new(
            ProjectSession::new(
                Box::new(Hub::open(&temp.path().join("historical-hub")).unwrap()),
                &catalog,
                &inputs,
                1000,
            )
            .unwrap(),
        );
        for (id, depends) in [("T-1", vec![]), ("T-2", vec!["T-1".to_string()])] {
            let command: TaskCommand=serde_json::from_value(json!({"action":"add","task":{"id":id,"title":"fixture","module":"A","allowed_paths":["src/a/**"],"acceptance":["AC-A"],"depends_on":depends}})).unwrap();
            ProjectSession::execute_shared(
                &history,
                &catalog,
                &server.recovery_inputs(&catalog).unwrap(),
                "fixture-host",
                command,
                None,
                1000,
                &mut server.task_outbox(&catalog).unwrap(),
            )
            .unwrap();
        }
        let run = ProjectSession::execute_shared(
            &history,
            &catalog,
            &server.recovery_inputs(&catalog).unwrap(),
            "fixture-host",
            TaskCommand::Claim {
                id: "T-1".into(),
                harness: None,
                harness_version: None,
            },
            None,
            1000,
            &mut server.task_outbox(&catalog).unwrap(),
        )
        .unwrap();
        let fences = serde_json::from_value(run["leases"].clone()).unwrap();
        let old = NewEvent {
            eid: Uuid::now_v7().to_string(),
            host: "fixture-host".into(),
            actor: "process:old-fixture".into(),
            kind: "task.state".into(),
            layer: "L5".into(),
            obj: "T-1".into(),
            data: json!({"task":history.lock().unwrap().projection().unwrap().objects["Task"]["T-1"]}),
            guards: fences,
        };
        let report = SnapshotWriter::default()
            .write(
                &history,
                &catalog,
                "fixture-host",
                true,
                || Ok(1000),
                &mut server.task_outbox(&catalog).unwrap(),
            )
            .unwrap();
        assert_eq!(report.status, "written");
        let current = Catalog::load(&root).unwrap();
        let saved = snapshot_service::read_snapshot(&current).unwrap().unwrap();
        assert_eq!(
            saved.source == snapshot_service::SnapshotSource::Private,
            private
        );
        drop(history);
        (temp, server, run_tree, old)
    }

    async fn rebuild_http(
        port: u16,
        instance: String,
        path: String,
        body: Value,
    ) -> (reqwest::StatusCode, Value) {
        tokio::task::spawn_blocking(move || {
            let response = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap()
                .post(format!("http://127.0.0.1:{port}{path}"))
                .header("x-towermd-instance", instance)
                .header("x-towermd-client", "app")
                .json(&body)
                .send()
                .unwrap();
            (response.status(), response.json().unwrap())
        })
        .await
        .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_rebuild_makes_old_epoch_writes_stale() {
        use towermd_core::hub_protocol::AppendReceipt;
        let (_temp, server, run_tree, old) = rebuild_fixture(true);
        let root = run_tree.parent().unwrap().join("repo");
        let catalog = Catalog::load(&root).unwrap();
        let original = server.project(&catalog).unwrap().hub.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = router(server.clone());
        let (stop, done) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = done.await;
                })
                .await
                .unwrap()
        });
        let (code, request) = rebuild_http(
            port,
            server.endpoint.instance.clone(),
            "/rebuild-request".into(),
            json!({"directory":root,"run":null}),
        )
        .await;
        assert_eq!(code, reqwest::StatusCode::OK, "{request}");
        let (code, report) = rebuild_http(
            port,
            server.endpoint.instance.clone(),
            format!(
                "/hub/rebuild/{}/confirm",
                request["request_id"].as_str().unwrap()
            ),
            json!({"actor":"human:fixture"}),
        )
        .await;
        assert_eq!(code, reqwest::StatusCode::OK, "{report}");
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(Arc::ptr_eq(&project.hub, &original));
        let before = server
            .execute(Request {
                directory: root.clone(),
                run: None,
                command: TaskCommand::Status,
            })
            .unwrap();
        let receipt = project
            .hub
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .append(old.clone(), 2000)
            .unwrap();
        let AppendReceipt::Event(event) = receipt else {
            panic!("old event must be sequenced")
        };
        assert_eq!(event.eid, old.eid);
        assert!(event.stale);
        assert!(event.seq > report["seq"].as_u64().unwrap());
        let after = server
            .execute(Request {
                directory: root.clone(),
                run: None,
                command: TaskCommand::Status,
            })
            .unwrap();
        assert_eq!(after["tasks"], before["tasks"]);
        let blocked = server
            .execute(Request {
                directory: run_tree.clone(),
                run: None,
                command: TaskCommand::Claim {
                    id: "T-2".into(),
                    harness: None,
                    harness_version: None,
                },
            })
            .unwrap_err();
        assert!(blocked.to_string().contains("dependencies"), "{blocked}");
        let claimed = server
            .execute(Request {
                directory: run_tree,
                run: None,
                command: TaskCommand::Claim {
                    id: "T-1".into(),
                    harness: None,
                    harness_version: None,
                },
            })
            .unwrap();
        for lease in claimed["leases"].as_array().unwrap() {
            assert!(lease["epoch"].as_u64().unwrap() > request["epoch_boundary"].as_u64().unwrap());
        }
        stop.send(()).unwrap();
        serving.await.unwrap();
        server.shutdown.send_replace(true);
        server.main_refresh.drain().await.unwrap();
        server.drain_snapshots().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_confirm_install_failure_consumes_persisted_boundary() {
        for private in [false, true] {
            let (_temp, mut server, run_tree, _old) = rebuild_fixture(private);
            let root = run_tree.parent().unwrap().join("repo");
            let catalog = Catalog::load(&root).unwrap();
            let request = server
                .rebuild_request(RebuildRequest {
                    directory: root.clone(),
                    run: None,
                })
                .unwrap();
            let boundary = request["seq_boundary"].as_u64().unwrap();
            let injected = Arc::new(AtomicBool::new(false));
            let observed = injected.clone();
            let fault_root = root.clone();
            let journal = catalog.common_dir.join("towermd/journal.jsonl");
            let backup = journal.with_extension("backup");
            let fault_journal = journal.clone();
            let fault_backup = backup.clone();
            let original = fs::read(&journal).unwrap();
            let watched_refs = [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads/main",
                "refs/towermd/snapshot",
            ];
            let original_refs = rebuild_git(&root, &watched_refs);
            let full_reads = Arc::new(AtomicU64::new(0));
            let verified_reads = full_reads.clone();
            let clock_calls = Arc::new(AtomicU64::new(0));
            let calls = clock_calls.clone();
            Arc::get_mut(&mut server).unwrap().clock = Arc::new(move || {
                let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                let began = Instant::now();
                let already_injected = observed.load(Ordering::SeqCst);
                let result = (|| -> Result<u64, String> {
                    if observed.load(Ordering::SeqCst) {
                        return Ok(2000);
                    }
                    // This is only a test fault trigger. Unchanged actual refs need
                    // no archive proof; a change still uses the unique full reader
                    // before moving the real journal at the persisted boundary.
                    if rebuild_git(&fault_root, &watched_refs) == original_refs {
                        return Ok(2000);
                    }
                    verified_reads.fetch_add(1, Ordering::SeqCst);
                    let catalog = Catalog::load(&fault_root)?;
                    let saved = snapshot_service::read_snapshot(&catalog)?.unwrap();
                    if saved.snapshot.through_seq > boundary {
                        eprintln!("INSTALL_FAULT_VERIFIED private={private} commit={} seq={} boundary={boundary}", saved.commit, saved.snapshot.through_seq);
                        fs::rename(&fault_journal, &fault_backup).map_err(|e| e.to_string())?;
                        fs::create_dir(&fault_journal).map_err(|e| e.to_string())?;
                        observed.store(true, Ordering::SeqCst);
                    }
                    Ok(2000)
                })();
                eprintln!("INSTALL_FAULT_CLOCK private={private} call={call} already_injected={already_injected} injected={} elapsed_ms={} result_ok={}", observed.load(Ordering::SeqCst), began.elapsed().as_millis(), result.is_ok());
                result
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let app = router(server.clone());
            let (stop, done) = tokio::sync::oneshot::channel::<()>();
            let serving = tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = done.await;
                    })
                    .await
                    .unwrap()
            });
            let confirm = format!(
                "/hub/rebuild/{}/confirm",
                request["request_id"].as_str().unwrap()
            );
            eprintln!("INSTALL_FAULT_HTTP private={private} phase=first_confirm path={confirm}");
            let (code, error) = rebuild_http(
                port,
                server.endpoint.instance.clone(),
                confirm.clone(),
                json!({"actor":"human:fixture"}),
            )
            .await;
            assert_eq!(code, reqwest::StatusCode::BAD_REQUEST, "{error}");
            assert!(injected.load(Ordering::SeqCst));
            let current = Catalog::load(&root).unwrap();
            let saved = snapshot_service::read_snapshot(&current).unwrap().unwrap();
            assert!(
                error["error"].as_str().unwrap().contains("已持久化"),
                "{error}"
            );
            assert!(error["error"].as_str().unwrap().contains(&saved.commit));
            let project = server.project(&current).unwrap();
            assert!(project.hub.lock().unwrap().projection().is_err());
            let status = server
                .execute(Request {
                    directory: root.clone(),
                    run: None,
                    command: TaskCommand::Status,
                })
                .unwrap();
            assert_eq!(status["status"], "recovery_required");
            assert!(status["error"].as_str().unwrap().contains(&saved.commit));
            assert!(server
                .execute(Request {
                    directory: run_tree.clone(),
                    run: None,
                    command: TaskCommand::Claim {
                        id: "T-1".into(),
                        harness: None,
                        harness_version: None
                    }
                })
                .is_err());
            fs::remove_dir(&journal).unwrap();
            fs::rename(&backup, &journal).unwrap();
            assert_eq!(fs::read(&journal).unwrap(), original);
            eprintln!("INSTALL_FAULT_HTTP private={private} phase=consumed_confirm");
            let (code, _) = rebuild_http(
                port,
                server.endpoint.instance.clone(),
                confirm,
                json!({"actor":"human:fixture"}),
            )
            .await;
            assert_eq!(code, reqwest::StatusCode::CONFLICT);
            assert_eq!(fs::read(&journal).unwrap(), original);
            let (code, fresh) = rebuild_http(
                port,
                server.endpoint.instance.clone(),
                "/rebuild-request".into(),
                json!({"directory":root,"run":null}),
            )
            .await;
            assert_eq!(code, reqwest::StatusCode::OK, "{fresh}");
            assert!(fresh["source_seq"].as_u64().unwrap() > boundary);
            assert!(
                fresh["epoch_boundary"].as_u64().unwrap()
                    > request["epoch_boundary"].as_u64().unwrap()
            );
            eprintln!(
                "INSTALL_FAULT_HTTP private={private} phase=fresh_confirm path={}",
                fresh["request_id"]
            );
            let (code, report) = rebuild_http(
                port,
                server.endpoint.instance.clone(),
                format!(
                    "/hub/rebuild/{}/confirm",
                    fresh["request_id"].as_str().unwrap()
                ),
                json!({"actor":"human:fixture"}),
            )
            .await;
            assert_eq!(code, reqwest::StatusCode::OK, "{report}");
            assert!(project.hub.lock().unwrap().projection().is_ok());
            assert_eq!(
                full_reads.load(Ordering::SeqCst),
                1,
                "only the actual changed boundary needs the fault fixture's full reader"
            );
            eprintln!(
                "INSTALL_FAULT_TOTAL private={private} clock_calls={} verified_full_reads={}",
                clock_calls.load(Ordering::SeqCst),
                full_reads.load(Ordering::SeqCst)
            );
            stop.send(()).unwrap();
            serving.await.unwrap();
            server.shutdown.send_replace(true);
            server.main_refresh.drain().await.unwrap();
            server.drain_snapshots().await.unwrap();
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_control_actor_is_rejected_before_target_changes() {
        let mut observations = vec![];
        for registered in [false, true] {
            for actor in ["human:\u{0}", "human:\u{7f}"] {
                let (_temp, server, run_tree, _old) = rebuild_fixture(true);
                let root = run_tree.parent().unwrap().join("repo");
                let catalog = Catalog::load(&root).unwrap();
                let directory = catalog.common_dir.join("towermd");
                let original = if registered {
                    Some(server.project(&catalog).unwrap())
                } else {
                    server.projects.lock().unwrap().clear();
                    fs::remove_dir_all(&directory).unwrap();
                    None
                };
                #[cfg(unix)]
                let files_before = original.as_ref().map(|project| {
                    use std::os::unix::fs::MetadataExt;
                    let project_file = project._lock.0.metadata().unwrap();
                    let hub_file = fs::metadata(directory.join("hub.lock")).unwrap();
                    (
                        project_file.dev(),
                        project_file.ino(),
                        hub_file.dev(),
                        hub_file.ino(),
                    )
                });
                let request = server
                    .rebuild_request(RebuildRequest {
                        directory: root.clone(),
                        run: None,
                    })
                    .unwrap();
                let saved_path = server
                    .runtime
                    .join("rebuild-requests")
                    .join(format!("{}.json", request["request_id"].as_str().unwrap()));
                let saved_before = fs::read(&saved_path).unwrap();
                let journal_before = fs::read(directory.join("journal.jsonl")).ok();
                let refs_before = rebuild_git(&root, &["show-ref"]);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let app = router(server.clone());
                let (stop, done) = tokio::sync::oneshot::channel::<()>();
                let serving = tokio::spawn(async move {
                    axum::serve(listener, app)
                        .with_graceful_shutdown(async {
                            let _ = done.await;
                        })
                        .await
                        .unwrap()
                });
                let confirm = format!(
                    "/hub/rebuild/{}/confirm",
                    request["request_id"].as_str().unwrap()
                );
                let (code, response) = rebuild_http(
                    port,
                    server.endpoint.instance.clone(),
                    confirm.clone(),
                    json!({"actor":actor}),
                )
                .await;
                let journal_after = fs::read(directory.join("journal.jsonl")).ok();
                let registered_after = server
                    .registered_project(catalog.config.project_id())
                    .unwrap();
                let same_arc = original
                    .as_ref()
                    .map(|p| Arc::ptr_eq(&p.hub, &registered_after.as_ref().unwrap().hub));
                let active_after = original
                    .as_ref()
                    .map(|p| p.hub.lock().unwrap().projection().is_ok());
                let observation = json!({"registered":registered,"actor":actor,"http_status":code.as_u16(),"response":response,
                    "target_exists_after":directory.exists(),"project_published_after":registered_after.is_some(),
                    "same_arc":same_arc,"active_after":active_after,"journal_before":journal_before,"journal_after":journal_after,
                    "bytes_equal":journal_before==journal_after,"refs_equal":refs_before==rebuild_git(&root,&["show-ref"]),
                    "request_equal":saved_before==fs::read(&saved_path).unwrap()});
                eprintln!("HTTP_CONTROL_ACTOR_OBSERVATION={observation}");
                observations.push(observation);
                if let Some(project) = &original {
                    assert!(
                        Hub::open(&directory).is_err(),
                        "original hub file ownership must stay held"
                    );
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        let project_file = project._lock.0.metadata().unwrap();
                        let hub_file = fs::metadata(directory.join("hub.lock")).unwrap();
                        assert_eq!(
                            Some((
                                project_file.dev(),
                                project_file.ino(),
                                hub_file.dev(),
                                hub_file.ino()
                            )),
                            files_before
                        );
                    }
                }
                // A normal human actor still restores through the exact real TCP handler.
                if actor == "human:\u{0}" {
                    let (code, report) = rebuild_http(
                        port,
                        server.endpoint.instance.clone(),
                        confirm,
                        json!({"actor":"human:fixture"}),
                    )
                    .await;
                    assert_eq!(code, reqwest::StatusCode::OK, "{report}");
                    let project = server
                        .registered_project(catalog.config.project_id())
                        .unwrap()
                        .unwrap();
                    if let Some(original) = &original {
                        assert!(Arc::ptr_eq(&original.hub, &project.hub));
                    }
                    assert!(project.hub.lock().unwrap().projection().is_ok());
                }
                stop.send(()).unwrap();
                serving.await.unwrap();
                server.shutdown.send_replace(true);
                server.main_refresh.drain().await.unwrap();
                server.drain_snapshots().await.unwrap();
            }
        }
        assert!(
            observations.iter().all(|o| o["http_status"] == 422
                && o["bytes_equal"] == true
                && o["refs_equal"] == true
                && o["request_equal"] == true
                && (o["registered"] == false
                    && o["target_exists_after"] == false
                    && o["project_published_after"] == false
                    || o["registered"] == true
                        && o["same_arc"] == true
                        && o["active_after"] == true)),
            "control actor must be 422 before target creation or retirement: {observations:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_saved_recovery_source_changes_are_conflicts() {
        let mut observations = Vec::new();
        for removed in [
            "refs/towermd/snapshot",
            "refs/heads/main",
            "bad-config",
            "read-io",
            "late-delete-private",
            "late-delete-main",
            "late-outbox-change",
            "late-outbox-bad",
        ] {
            let (_temp, mut server, run_tree, old) = rebuild_fixture(true);
            let root = run_tree.parent().unwrap().join("repo");
            let catalog = Catalog::load(&root).unwrap();
            let directory = catalog.common_dir.join("towermd");
            server.projects.lock().unwrap().clear();
            fs::remove_dir_all(&directory).unwrap();
            let request = server
                .rebuild_request(RebuildRequest {
                    directory: root.clone(),
                    run: None,
                })
                .unwrap();
            assert_eq!(
                request["source_reference"]
                    .as_str()
                    .unwrap()
                    .split(':')
                    .next()
                    .unwrap(),
                "refs/towermd/snapshot"
            );
            let saved_path = server
                .runtime
                .join("rebuild-requests")
                .join(format!("{}.json", request["request_id"].as_str().unwrap()));
            let saved_before = fs::read(&saved_path).unwrap();
            match removed {
                "bad-config" => {
                    fs::write(root.join(".towermd/config.yaml"), "invalid: [").unwrap();
                    rebuild_git(&root, &["add", ".towermd/config.yaml"]);
                    rebuild_git(&root, &["commit", "-qm", "invalid new authority config"]);
                }
                "read-io" => {
                    let object = catalog
                        .common_dir
                        .join("objects")
                        .join(&catalog.revision[..2])
                        .join(&catalog.revision[2..]);
                    fs::remove_file(&object).unwrap();
                    fs::create_dir(&object).unwrap(); // A directory cannot be read as a Git commit object.
                }
                "late-delete-private"
                | "late-delete-main"
                | "late-outbox-change"
                | "late-outbox-bad" => {}
                _ => {
                    rebuild_git(&root, &["update-ref", "-d", removed]);
                }
            }
            let mut refs_after_removal = ["refs/heads/main", "refs/towermd/snapshot"]
                .map(|name| fs::read(catalog.common_dir.join(name)).ok());
            if removed.starts_with("late-") {
                let reference = if removed == "late-delete-main" {
                    "refs/heads/main"
                } else {
                    "refs/towermd/snapshot"
                };
                if removed.starts_with("late-delete-") {
                    refs_after_removal[usize::from(reference == "refs/towermd/snapshot")] = None;
                }
                let source_root = root.clone();
                let outbox_directory = server
                    .runtime
                    .join("recovery-outbox")
                    .join(catalog.config.project_id());
                let calls = AtomicU64::new(0);
                Arc::get_mut(&mut server).unwrap().clock = Arc::new(move || {
                    // HTTP expiry is first; core confirmation is after daemon's actual source reads.
                    if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                        match removed {
                            "late-outbox-change" => {
                                let current = Catalog::load(&source_root)?;
                                let context =
                                    towermd_core::recovery_outbox::RecoveryContext::from_catalog(
                                        &current,
                                    )?;
                                let mut outbox =
                                    towermd_core::recovery_outbox::RecoveryOutbox::open(
                                        &outbox_directory,
                                        context,
                                    )?;
                                outbox.append_pending(&old)?;
                            }
                            "late-outbox-bad" => {
                                fs::create_dir_all(&outbox_directory).map_err(|e| e.to_string())?;
                                fs::write(outbox_directory.join("changed.jsonl"), "not-json\n")
                                    .map_err(|e| e.to_string())?;
                            }
                            _ => {
                                rebuild_git(&source_root, &["update-ref", "-d", reference]);
                            }
                        }
                    }
                    Ok(2000)
                });
            }
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let app = router(server.clone());
            let (stop, done) = tokio::sync::oneshot::channel::<()>();
            let serving = tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = done.await;
                    })
                    .await
                    .unwrap()
            });
            let (code, response) = rebuild_http(
                port,
                server.endpoint.instance.clone(),
                format!(
                    "/hub/rebuild/{}/confirm",
                    request["request_id"].as_str().unwrap()
                ),
                json!({"actor":"human:fixture"}),
            )
            .await;
            let observation = json!({"removed":removed,"saved_source":request["source_commit"],"http_status":code.as_u16(),"response":response,"target_exists_after":directory.exists(),"project_published_after":server.registered_project(catalog.config.project_id()).unwrap().is_some(),"expected_status":if matches!(removed, "read-io" | "late-outbox-bad") {400} else {409},"bad_outbox_reached":removed != "late-outbox-bad" || (fs::read(server.runtime.join("recovery-outbox").join(catalog.config.project_id()).join("changed.jsonl")).ok()==Some(b"not-json\n".to_vec()) && server.recovery_inputs(&catalog).is_err()),"refs_equal_after_confirm":refs_after_removal==["refs/heads/main", "refs/towermd/snapshot"].map(|name| fs::read(catalog.common_dir.join(name)).ok()),"saved_request_equal":saved_before==fs::read(saved_path).unwrap()});
            eprintln!("SPEC_DISAPPEARED_SOURCE_OBSERVATION={observation}");
            observations.push(observation);
            stop.send(()).unwrap();
            serving.await.unwrap();
            server.shutdown.send_replace(true);
            server.main_refresh.drain().await.unwrap();
            server.drain_snapshots().await.unwrap();
        }
        assert!(observations.iter().all(|o|o["http_status"]==o["expected_status"] && o["bad_outbox_reached"]==true && o["target_exists_after"]==false && o["project_published_after"]==false && o["refs_equal_after_confirm"]==true && o["saved_request_equal"]==true),"a saved recovery source/authority that disappeared must conflict without target changes: {observations:?}");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_late_git_read_io_keeps_failure_category() {
        // The Git fault wrapper changes PATH only in this exact-test child.
        // Concurrent fixtures resolving their own real Git must never capture
        // this temporary wrapper and retain it after its TempDir is removed.
        const CHILD: &str = "TOWERMD_LATE_GIT_IO_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "daemon::tests::app_late_git_read_io_keeps_failure_category",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(
                output.status.success(),
                "isolated actual Git read-I/O fixture failed"
            );
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let mut observations = Vec::new();
        for module_path in [
            "docs/tower/arch/changed-A.md",
            "docs/tower/arch/已变化-A.md",
            "docs/tower/arch/未过期-A.md",
        ] {
            let (temp, mut server, run_tree, _old) =
                rebuild_fixture_with_module_path(true, module_path);
            let root = fs::canonicalize(run_tree.parent().unwrap().join("repo")).unwrap();
            let catalog = Catalog::load(&root).unwrap();
            let directory = catalog.common_dir.join("towermd");
            server.projects.lock().unwrap().clear();
            fs::remove_dir_all(&directory).unwrap();
            let request = server
                .rebuild_request(RebuildRequest {
                    directory: root.clone(),
                    run: None,
                })
                .unwrap();
            let id = request["request_id"].as_str().unwrap();
            let saved = server.saved_request(id).unwrap().unwrap();
            assert!(towermd_core::recovery::check_request_sources(&root, &saved.request).is_ok());
            let saved_path = server
                .runtime
                .join("rebuild-requests")
                .join(format!("{id}.json"));
            let saved_before = fs::read(&saved_path).unwrap();
            let refs_before = ["refs/heads/main", "refs/towermd/snapshot"]
                .map(|n| fs::read(catalog.common_dir.join(n)).ok());
            let blob = rebuild_git(&root, &["rev-parse", &format!("main:{module_path}")]);
            let object = catalog
                .common_dir
                .join("objects")
                .join(&blob[..2])
                .join(&blob[2..]);
            let object_before = fs::read(&object).unwrap();
            let backup = object.with_extension("spec-backup");
            let wrapper = temp.path().join("git-wrapper");
            fs::create_dir(&wrapper).unwrap();
            let real = Command::new("which").arg("git").output().unwrap();
            assert!(real.status.success());
            let real = String::from_utf8(real.stdout).unwrap().trim().to_string();
            let armed = temp.path().join("armed-after-preprobe");
            let injected = temp.path().join("actual-cat-file-io-injected");
            let script = format!(
                r#"#!/bin/sh
if test "$1" = cat-file && test "$2" = blob && test "$3" = '{blob}' && test "$(pwd -P)" = '{root}' && test -f '{armed}'; then
    /bin/mv '{object}' '{backup}' || exit 95
    /bin/mkdir '{object}' || exit 96
    /bin/rm '{armed}' || exit 97
    /usr/bin/touch '{injected}' || exit 98
fi
exec '{real}' "$@"
"#,
                blob = blob,
                root = root.display(),
                armed = armed.display(),
                object = object.display(),
                backup = backup.display(),
                injected = injected.display(),
                real = real
            );
            let program = wrapper.join("git");
            fs::write(&program, script).unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            let calls = Arc::new(AtomicU64::new(0));
            let clock_calls = calls.clone();
            let clock_armed = armed.clone();
            Arc::get_mut(&mut server).unwrap().clock = Arc::new(move || {
                if clock_calls.fetch_add(1, Ordering::SeqCst) == 1 {
                    fs::write(
                        &clock_armed,
                        "core confirmation began after daemon source preprobe and Catalog",
                    )
                    .map_err(|e| e.to_string())?;
                }
                Ok(2000)
            });
            let previous_path = std::env::var_os("PATH").unwrap();
            std::env::set_var(
                "PATH",
                format!("{}:{}", wrapper.display(), previous_path.to_string_lossy()),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let app = router(server.clone());
            let (stop, done) = tokio::sync::oneshot::channel::<()>();
            let serving = tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = done.await;
                    })
                    .await
                    .unwrap()
            });
            let (code, response) = rebuild_http(
                port,
                server.endpoint.instance.clone(),
                format!("/hub/rebuild/{id}/confirm"),
                json!({"actor":"human:fixture"}),
            )
            .await;
            std::env::set_var("PATH", previous_path);
            let observation = json!({"module_path":module_path,"http_status":code.as_u16(),"response":response,"clock_calls":calls.load(Ordering::SeqCst),"actual_git_io_stage_reached":injected.exists(),"object_is_directory":object.is_dir(),"original_object_bytes_preserved":fs::read(&backup).ok()==Some(object_before.clone()),"source_identity_still_matches":towermd_core::recovery::check_request_sources(&root,&saved.request).is_ok(),"refs_equal":refs_before==["refs/heads/main","refs/towermd/snapshot"].map(|n|fs::read(catalog.common_dir.join(n)).ok()),"saved_request_equal":saved_before==fs::read(&saved_path).unwrap(),"target_exists_after":directory.exists(),"project_published_after":server.registered_project(catalog.config.project_id()).unwrap().is_some()});
            eprintln!("SPEC_LATE_READ_IO_OBSERVATION={observation}");
            assert!(
                injected.exists(),
                "fixture must reach actual cat-file I/O stage"
            );
            fs::remove_dir(&object).unwrap();
            fs::rename(&backup, &object).unwrap();
            assert_eq!(fs::read(object).unwrap(), object_before);
            stop.send(()).unwrap();
            serving.await.unwrap();
            server.shutdown.send_replace(true);
            server.main_refresh.drain().await.unwrap();
            server.drain_snapshots().await.unwrap();
            observations.push(observation);
        }
        assert!(observations.iter().all(|o|o["http_status"]==400 && o["source_identity_still_matches"]==true && o["refs_equal"]==true && o["saved_request_equal"]==true && o["target_exists_after"]==false && o["project_published_after"]==false),"ordinary unchanged-source actual Git blob I/O must be 400 regardless of diagnostic words: {observations:?}");
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_confirmation_expiry_after_http_check_is_conflict() {
        for registered in [false, true] {
            let (_temp, mut server, run_tree, _old) = rebuild_fixture(true);
            let root = run_tree.parent().unwrap().join("repo");
            let catalog = Catalog::load(&root).unwrap();
            let directory = catalog.common_dir.join("towermd");
            let original = server
                .registered_project(catalog.config.project_id())
                .unwrap()
                .unwrap();
            if !registered {
                server.projects.lock().unwrap().clear();
                drop(original);
                fs::remove_dir_all(&directory).unwrap();
            } else {
                drop(original);
            }
            let request = server
                .rebuild_request(RebuildRequest {
                    directory: root.clone(),
                    run: None,
                })
                .unwrap();
            let id = request["request_id"].as_str().unwrap();
            let expires_at = request["expires_at"].as_u64().unwrap();
            let saved_path = server
                .runtime
                .join("rebuild-requests")
                .join(format!("{id}.json"));
            let saved = fs::read(&saved_path).unwrap();
            let refs = rebuild_git(&root, &["show-ref"]);
            let journal = fs::read(directory.join("journal.jsonl")).ok();
            let calls = AtomicU64::new(0);
            Arc::get_mut(&mut server).unwrap().clock = Arc::new(move || {
                Ok(if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    2000
                } else {
                    expires_at
                })
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let app = router(server.clone());
            let (stop, done) = tokio::sync::oneshot::channel::<()>();
            let serving = tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = done.await;
                    })
                    .await
                    .unwrap()
            });
            let (code, response) = rebuild_http(
                port,
                server.endpoint.instance.clone(),
                format!("/hub/rebuild/{id}/confirm"),
                json!({"actor":"human:fixture"}),
            )
            .await;
            let active = server
                .registered_project(catalog.config.project_id())
                .unwrap()
                .map(|p| p.hub.lock().unwrap().projection().is_ok());
            let observation = json!({"registered":registered,"http_status":code.as_u16(),"response":response,"active_after":active,"target_exists_after":directory.exists(),"journal_equal":journal==fs::read(directory.join("journal.jsonl")).ok(),"refs_equal":refs==rebuild_git(&root,&["show-ref"]),"saved_equal":saved==fs::read(&saved_path).unwrap()});
            eprintln!("HTTP_CORE_EXPIRY_OBSERVATION={observation}");
            stop.send(()).unwrap();
            serving.await.unwrap();
            server.shutdown.send_replace(true);
            server.main_refresh.drain().await.unwrap();
            server.drain_snapshots().await.unwrap();
            assert_eq!(code, reqwest::StatusCode::CONFLICT);
            assert_eq!(observation["journal_equal"], true);
            assert_eq!(observation["refs_equal"], true);
            assert_eq!(observation["saved_equal"], true);
            assert!(if registered {
                active == Some(true)
            } else {
                !directory.exists()
            });
        }
    }
    #[tokio::test]
    async fn q1_later_actual_results_win_over_delayed_completed_and_failed_tails() {
        const START: u64 = 1_800_000_000_000;
        let (dir, root) = snapshot_fixture(600);
        let server = snapshot_server(
            &root,
            dir.path().join("runtime"),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(START)),
        );
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        let host = &server.endpoint.host;
        let now = ClockReading {
            mono_ms: 0,
            unix_ms: START,
        };
        let old_success_before = begin_snapshot_preparation(&project).unwrap();
        let old_success = project.writer.write_observed(
            &project.hub,
            &catalog,
            host,
            true,
            || Ok(START),
            &mut server.task_outbox(&catalog).unwrap(),
        );
        assert!(
            old_success.completed().is_some(),
            "{:?}",
            old_success.result()
        );
        let lock = project.common_dir.join("refs/towermd/snapshot.lock");
        fs::write(&lock, "actual CAS failure").unwrap();
        let newer_failure_before = begin_snapshot_preparation(&project).unwrap();
        let newer_failure = project.writer.write_observed(
            &project.hub,
            &catalog,
            host,
            true,
            || Ok(START),
            &mut server.task_outbox(&catalog).unwrap(),
        );
        fs::remove_file(&lock).unwrap();
        assert!(newer_failure.result().is_err());
        assert!(newer_failure
            .observation_order()
            .unwrap()
            .follows(old_success.observation_order().unwrap()));
        finish_snapshot_attempt(&project, &newer_failure_before, &newer_failure, host, now)
            .unwrap();
        let failed = project.schedule.lock().unwrap().status();
        finish_snapshot_attempt(&project, &old_success_before, &old_success, host, now).unwrap();
        assert_eq!(
            project.schedule.lock().unwrap().status(),
            failed,
            "old completed tail cleared the newer actual failure"
        );
        let newer_success_before = begin_snapshot_preparation(&project).unwrap();
        let newer_success = project.writer.write_observed(
            &project.hub,
            &catalog,
            host,
            true,
            || Ok(START),
            &mut server.task_outbox(&catalog).unwrap(),
        );
        assert!(newer_success.completed().is_some());
        finish_snapshot_attempt(&project, &newer_success_before, &newer_success, host, now)
            .unwrap();
        let succeeded = project.schedule.lock().unwrap().status();
        finish_snapshot_attempt(&project, &newer_failure_before, &newer_failure, host, now)
            .unwrap();
        assert_eq!(
            project.schedule.lock().unwrap().status(),
            succeeded,
            "old failed tail reopened the newer actual success"
        );
        assert!(succeeded["deferred_since"].is_null());
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    #[tokio::test]
    async fn q1_delayed_busy_after_actual_owner_completion_cannot_reopen_episode() {
        const START: u64 = 1_800_000_000_000;
        let (dir, root) = snapshot_fixture(1);
        let runtime = dir.path().join("runtime");
        let unix = Arc::new(AtomicU64::new(START));
        let server = snapshot_server(
            &root,
            runtime.clone(),
            Arc::new(AtomicU64::new(0)),
            unix.clone(),
        );
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        let host = &server.endpoint.host;
        let owner_before = begin_snapshot_preparation(&project).unwrap();
        let busy_before = begin_snapshot_preparation(&project).unwrap();
        let mut delayed_busy = None;
        let mut calls = 0;
        let prepared = {
            let mut outbox = server.task_outbox(&catalog).unwrap();
            project
                .writer
                .prepare_write(&project.hub, &catalog, host, START, &mut outbox)
                .unwrap()
        };
        let owner = project.writer.write_prepared_observed(
            &project.hub,
            &catalog,
            host,
            true,
            || {
                calls += 1;
                if calls == 2 {
                    let busy = project.writer.write_observed(
                        &project.hub,
                        &catalog,
                        host,
                        true,
                        || Ok(START),
                        &mut server.task_outbox(&catalog).unwrap(),
                    );
                    assert!(
                        busy.inflight().is_some(),
                        "actual frozen owner intent must be visible"
                    );
                    finish_snapshot_attempt(
                        &project,
                        &busy_before,
                        &busy,
                        host,
                        ClockReading {
                            mono_ms: 0,
                            unix_ms: START,
                        },
                    )
                    .unwrap();
                    let busy = project.writer.write_observed(
                        &project.hub,
                        &catalog,
                        host,
                        true,
                        || Ok(START),
                        &mut server.task_outbox(&catalog).unwrap(),
                    );
                    assert!(busy.inflight().is_some());
                    delayed_busy = Some(busy);
                }
                Ok(START)
            },
            prepared,
        );
        let busy = delayed_busy.expect("actual owner preparation callback");
        assert!(owner.completed().is_some());
        assert!(owner
            .observation_order()
            .unwrap()
            .follows(busy.observation_order().unwrap()));
        // This second true Busy result was captured while the owner held its
        // writer, but its callback arrives after Git+exact ack and before the
        // owner's delayed daemon callback. It crosses the original threshold.
        let late = ClockReading {
            mono_ms: 1_001,
            unix_ms: START + 1_001,
        };
        finish_snapshot_attempt(&project, &busy_before, &busy, host, late).unwrap();
        finish_snapshot_attempt(&project, &owner_before, &owner, host, late).unwrap();
        assert!(project.schedule.lock().unwrap().status()["deferred_since"].is_null());
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        drop(project);
        drop(server);
        let restarted = snapshot_server(&root, runtime, Arc::new(AtomicU64::new(0)), unix);
        let project = restarted.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(
            installed_summary(&project).unwrap().alert.is_none(),
            "a late Busy alert must not reopen the completed owner on restart"
        );
        restarted.shutdown.send_replace(true);
        restarted.drain_snapshots().await.unwrap();
    }

    type Q1ActivationHook = Arc<dyn Fn() + Send + Sync>;
    static Q1_ACTIVATION_HOOK: std::sync::OnceLock<Mutex<Option<(PathBuf, Q1ActivationHook)>>> =
        std::sync::OnceLock::new();
    pub(super) fn q1_after_rebuild_activation(common: &Path) {
        let hook = Q1_ACTIVATION_HOOK
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(path, _)| path == common)
            .map(|(_, hook)| hook.clone());
        if let Some(hook) = hook {
            hook();
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn q1_some_activation_invalidates_old_preparation_and_preserves_new_window() {
        // This lifecycle case needs a real private checkpoint and an empty
        // registered owner, but no Module/Task/Claim recovery history.
        let (_temp, root) = snapshot_fixture(600);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(2_000));
        let server = snapshot_server(
            &root,
            _temp.path().join("runtime"),
            mono.clone(),
            unix.clone(),
        );
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        assert_eq!(project.hub.lock().unwrap().last_seq(), 0);
        let run_tree = _temp.path().join("run");
        rebuild_git(
            &root,
            &[
                "worktree",
                "add",
                "-qb",
                "q1-history",
                run_tree.to_str().unwrap(),
                "main",
            ],
        );
        let history_catalog = Catalog::load(&run_tree).unwrap();
        let history = Mutex::new(
            ProjectSession::new(
                Box::new(Hub::open(&_temp.path().join("historical-hub")).unwrap()),
                &history_catalog,
                &server.recovery_inputs(&history_catalog).unwrap(),
                1_000,
            )
            .unwrap(),
        );
        let checkpoint = SnapshotWriter::default()
            .write(
                &history,
                &history_catalog,
                &server.endpoint.host,
                true,
                || Ok(1_000),
                &mut server.task_outbox(&history_catalog).unwrap(),
            )
            .unwrap();
        assert_eq!(checkpoint.status, "written");
        let saved = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
        assert_eq!(saved.source, snapshot_service::SnapshotSource::Private);
        assert!(saved.snapshot.projection.objects.is_empty());
        eprintln!(
            "Q1_SOME_FIXTURE source=private history_events={} projection_objects={}",
            history.lock().unwrap().last_seq(),
            saved.snapshot.projection.objects.len()
        );
        drop(history);
        let original_hub = project.hub.clone();
        let original_writer = project.writer.clone();
        let original_session = project.hub.lock().unwrap().snapshot_session_id();
        #[cfg(unix)]
        let original_inode = {
            use std::os::unix::fs::MetadataExt;
            project._lock.0.metadata().unwrap().ino()
        };
        let old = begin_snapshot_preparation(&project).unwrap();
        // A real failed old-session preparation, without adding any Hub event
        // (Some confirmation legitimately requires an empty registered target).
        let outbox = server
            .runtime
            .join("recovery-outbox")
            .join(catalog.config.project_id());
        fs::create_dir_all(&outbox).unwrap();
        let corrupt = outbox.join("00.json");
        fs::write(&corrupt, "not an outbox envelope").unwrap();
        let old_error = match server.recovery_inputs(&catalog) {
            Err(error) => error,
            Ok(_) => panic!("corrupt actual outbox must fail preparation"),
        };
        fs::remove_file(&corrupt).unwrap();
        let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        *Q1_ACTIVATION_HOOK
            .get_or_init(Default::default)
            .lock()
            .unwrap() = Some((
            project.common_dir.clone(),
            Arc::new(move || {
                entered_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = router(server.clone());
        let (stop, done) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = done.await;
                })
                .await
                .unwrap();
        });
        let (code, request) = rebuild_http(
            port,
            server.endpoint.instance.clone(),
            "/rebuild-request".into(),
            json!({"directory":root,"run":null}),
        )
        .await;
        assert_eq!(code, reqwest::StatusCode::OK, "{request}");
        let confirm_instance = server.endpoint.instance.clone();
        let path = format!(
            "/hub/rebuild/{}/confirm",
            request["request_id"].as_str().unwrap()
        );
        let mut confirming = tokio::spawn(rebuild_http(
            port,
            confirm_instance,
            path,
            json!({"actor":"human:fixture"}),
        ));
        tokio::select! {
            reached = entered_rx.recv() => assert!(reached.is_some()),
            response = &mut confirming => {
                *Q1_ACTIVATION_HOOK.get_or_init(Default::default).lock().unwrap() = None;
                let _ = stop.send(());
                serving.await.unwrap();
                server.shutdown.send_replace(true);
                server.drain_snapshots().await.unwrap();
                panic!("activation fixture returned before reaching barrier: {response:?}");
            }
        }
        let new_session = project.hub.lock().unwrap().snapshot_session_id();
        assert_ne!(new_session, original_session);
        consume_snapshot_schedule(
            &project,
            ClockReading {
                mono_ms: 0,
                unix_ms: 2_000,
            },
            |_, _, _| Ok(((), None)),
        )
        .unwrap();
        record_snapshot_failure(
            &project,
            &old,
            old_error,
            None,
            &server.endpoint.host,
            ClockReading {
                mono_ms: 0,
                unix_ms: 2_000,
            },
        )
        .unwrap();
        assert!(
            project.schedule.lock().unwrap().status()["deferred_since"].is_null(),
            "old preparation entered a replacement Session"
        );
        mono.store(1_000, Ordering::SeqCst);
        unix.store(3_000, Ordering::SeqCst);
        snapshot_hold(&project, 3_000);
        let pause_started = std::time::Instant::now();
        let pause_inputs = server.recovery_inputs(&catalog).unwrap();
        let inputs_elapsed = pause_started.elapsed();
        ProjectSession::sync_shared(&project.hub, &catalog, &pause_inputs, 3_000).unwrap();
        eprintln!(
            "Q1_SOME_PAUSED_PROOF inputs_ms={} verified_sync_total_ms={}",
            inputs_elapsed.as_millis(),
            pause_started.elapsed().as_millis()
        );
        // Observe the real installed Hub while the daemon tail is paused.
        // Full inputs/tick/Status I/O follows the pre-resync tail assertion below.
        consume_snapshot_schedule(
            &project,
            ClockReading {
                mono_ms: 1_000,
                unix_ms: 3_000,
            },
            |_, _, _| Ok(((), None)),
        )
        .unwrap();
        let hold = installed_summary(&project).unwrap().fast_pending.unwrap();
        assert!(project.schedule.lock().unwrap().eligible(26_000));
        assert!(!project.schedule.lock().unwrap().eligible(25_999));
        mono.store(20_000, Ordering::SeqCst);
        unix.store(22_000, Ordering::SeqCst);
        release_tx.send(()).unwrap();
        let (code, report) = confirming.await.unwrap();
        *Q1_ACTIVATION_HOOK
            .get_or_init(Default::default)
            .lock()
            .unwrap() = None;
        assert_eq!(code, reqwest::StatusCode::OK, "{report}");
        assert!(Arc::ptr_eq(&project.hub, &original_hub));
        assert!(Arc::ptr_eq(&project.writer, &original_writer));
        assert_eq!(
            project.hub.lock().unwrap().snapshot_session_id(),
            new_session
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(project._lock.0.metadata().unwrap().ino(), original_inode);
        }
        assert!(
            project.schedule.lock().unwrap().eligible(26_000),
            "late Some daemon tail reset an already observed new-session window"
        );
        assert!(!project.schedule.lock().unwrap().eligible(25_999));
        ProjectSession::tick_shared(
            &project.hub,
            &catalog,
            &server.recovery_inputs(&catalog).unwrap(),
            &server.endpoint.host,
            22_000,
            &mut server.task_outbox(&catalog).unwrap(),
        )
        .unwrap();
        let observed = server
            .execute(Request {
                directory: root.clone(),
                run: None,
                command: TaskCommand::Status,
            })
            .unwrap();
        assert!(observed["snapshot"]["deferred_since"].is_null());
        assert!(!project.schedule.lock().unwrap().eligible(25_999));
        assert!(project.schedule.lock().unwrap().eligible(26_000));
        mono.store(26_000, Ordering::SeqCst);
        unix.store(28_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        assert!(project.hub.lock().unwrap().snapshot_ack().unwrap().seq >= hold.max_seq);
        stop.send(()).unwrap();
        serving.await.unwrap();
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }

    type ReviewStatusHook = Arc<dyn Fn() + Send + Sync>;
    static REVIEW_STATUS_HOOK: std::sync::OnceLock<Mutex<Option<(PathBuf, ReviewStatusHook)>>> =
        std::sync::OnceLock::new();
    pub(super) fn review_after_status_summary(root: &Path) {
        let hook = REVIEW_STATUS_HOOK
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(p, _)| p == root)
            .map(|(_, h)| h.clone());
        if let Some(hook) = hook {
            hook();
        }
    }
    #[tokio::test]
    async fn independent_review_stale_real_status_postpones_real_hold_checkpoint() {
        const START: u64 = 1_800_000_000_000;
        let (dir, root) = snapshot_fixture(600);
        let mono = Arc::new(AtomicU64::new(0));
        let unix = Arc::new(AtomicU64::new(START));
        let server = snapshot_server(
            &root,
            dir.path().join("runtime"),
            mono.clone(),
            unix.clone(),
        );
        server
            .execute(Request {
                directory: root.clone(),
                run: None,
                command: TaskCommand::Status,
            })
            .unwrap();
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let project = server.project(&Catalog::load(&root).unwrap()).unwrap();
        assert!(installed_summary(&project).unwrap().fast_pending.is_none());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let once = Arc::new(AtomicBool::new(true));
        *REVIEW_STATUS_HOOK
            .get_or_init(Default::default)
            .lock()
            .unwrap() = Some((
            root.canonicalize().unwrap(),
            Arc::new(move || {
                if once.swap(false, Ordering::SeqCst) {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }
            }),
        ));
        let old_server = server.clone();
        let old_root = root.clone();
        let pending = std::thread::spawn(move || {
            old_server.execute(Request {
                directory: old_root,
                run: None,
                command: TaskCommand::Status,
            })
        });
        if entered_rx.recv_timeout(Duration::from_secs(3)).is_err() {
            let _ = release_tx.send(());
            let _ = pending.join();
            panic!("review fixture never captured real status summary");
        }
        mono.store(1_000, Ordering::SeqCst);
        unix.store(START + 1_000, Ordering::SeqCst);
        let eid = snapshot_hold(&project, START + 1_000);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let proved = installed_summary(&project).unwrap();
        let fast = proved.fast_pending.unwrap();
        assert!(project.schedule.lock().unwrap().eligible(26_000));
        eprintln!("REVIEW real hold eid={eid} seq={} generation={} cursor={} first_deadline=26000; old status blocked after actual Session snapshot_summary",fast.seq,proved.generation,proved.cursor);
        mono.store(20_000, Ordering::SeqCst);
        unix.store(START + 20_000, Ordering::SeqCst);
        release_tx.send(()).unwrap();
        let response = pending.join().unwrap().unwrap();
        *REVIEW_STATUS_HOOK.get().unwrap().lock().unwrap() = None;
        eprintln!(
            "REVIEW stale real status returned snapshot={}",
            response["snapshot"]
        );
        mono.store(21_000, Ordering::SeqCst);
        unix.store(START + 21_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let original_due = project.schedule.lock().unwrap().eligible(26_000);
        let postponed = project.schedule.lock().unwrap().eligible(46_000);
        mono.store(26_000, Ordering::SeqCst);
        unix.store(START + 26_000, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let at_deadline = snapshot_private(&root);
        mono.store(31_001, Ordering::SeqCst);
        unix.store(START + 31_001, Ordering::SeqCst);
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        let actual = snapshot_private(&root);
        let observed = installed_summary(&project).unwrap();
        eprintln!("REVIEW original_due={original_due} due_at46000={postponed} actual_private_at26000={at_deadline:?} actual_private_at31001={actual:?} real_installed_pending={:?}; since observed=1000 elapsed=30001 > original30000",observed.fast_pending);
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
        assert!(actual.is_some(),"late real status snapshot cancelled the first 25-second deadline: the actual hold is installed but no Git snapshot exists at original deadline");
    }
}
