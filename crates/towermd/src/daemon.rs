use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::main_refresh::MainRefresh;
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
use towermd_core::coldstart::{prepare_coldstart, ScanOptions};
use towermd_core::hub::{inspect_store, probe_store, Hub, StoreState};
use towermd_core::hub_protocol::HubError;
use towermd_core::project_session::ProjectSession;
use towermd_core::snapshot_service::{self, SnapshotWriter};
use towermd_core::tasks::{self, TaskCommand, TaskFailure};
use uuid::Uuid;

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
impl From<towermd_core::recovery::RequestSourceError> for RebuildFailure {
    fn from(error: towermd_core::recovery::RequestSourceError) -> Self {
        use towermd_core::recovery::RequestSourceError;
        Self {
            status: match &error {
                RequestSourceError::Changed => StatusCode::CONFLICT,
                RequestSourceError::Read(_) => StatusCode::BAD_REQUEST,
            },
            message: error.to_string(),
        }
    }
}
impl From<String> for RebuildFailure {
    fn from(message: String) -> Self {
        let status = if !message.contains("已持久化")
            && (message.contains("已变化")
                || message.contains("changed")
                || message.contains("未过期"))
        {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_REQUEST
        };
        Self { status, message }
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
    pub options: ScanOptions,
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
    common_dir: PathBuf,
    root: Mutex<PathBuf>,
    hub: Arc<Mutex<ProjectSession>>,
    writer: Arc<SnapshotWriter>,
    snapshot_interval: AtomicU64,
    reconcile: AtomicBool,
    recovery_error: Mutex<Option<String>>,
    background: Mutex<Option<tokio::task::JoinHandle<()>>>,
    _lock: File,
}

struct Server {
    main_refresh: MainRefresh,
    clock: Arc<dyn Fn() -> Result<u64, String> + Send + Sync>,
    endpoint: Endpoint,
    runtime: PathBuf,
    projects: Mutex<BTreeMap<String, Arc<Project>>>,
    registration: Mutex<()>,
    shutdown: tokio::sync::watch::Sender<bool>,
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

fn bind_project(runtime: &Path, project_id: &str, common_dir: &Path) -> Result<File, String> {
    let lock_name = hex::encode(Sha256::digest(project_id.as_bytes()));
    let project_lock = lock_file(&runtime.join(format!("project-{lock_name}.lock")))?;
    FileExt::try_lock_exclusive(&project_lock)
        .map_err(|error| format!("项目已被其他 clone 占用：{error}"))?;
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

pub fn snapshot_request(request: SnapshotRequest) -> Result<Value, String> {
    post_request("/snapshot", &request)
}

pub fn rebuild_request(request: RebuildRequest) -> Result<Value, String> {
    post_request("/rebuild-request", &request)
}

pub fn coldstart_request(request: ColdstartRequest) -> Result<Value, String> {
    post_request("/baseline/coldstart", &request)
}

fn post_request(path: &str, request: &impl Serialize) -> Result<Value, String> {
    let endpoint = connect(true)?.ok_or("守护进程未启动")?;
    let response = client(Duration::from_secs(120))?
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
            main_refresh: MainRefresh::default(),
            clock: Arc::new(now_ms),
            endpoint,
            runtime: runtime.clone(),
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
            shutdown: shutdown_sender,
        });
        let (ticker_stop, ticker) = start_ticker(server.clone());
        let app = router(server.clone());
        write_json(&runtime.join("endpoint.json"), &server.endpoint)?;
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                while !*receiver.borrow() {
                    if receiver.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
            .map_err(|error| error.to_string());
        let _ = server.shutdown.send(true);
        // Keep system leases alive while existing Git writers drain; shutdown prevents new work.
        let fetched = server.main_refresh.drain().await;
        let drained = server.drain_snapshots().await;
        let _ = ticker_stop.send(true);
        let ticked = ticker
            .await
            .map_err(|error| format!("项目维护退出失败：{error}"));
        result.and(fetched).and(drained).and(ticked)
    });
    // Runtime waits for all blocking work while the daemon/file ownership is still held.
    drop(daemon_runtime);
    drop(daemon_lock);
    result
}

fn router(server: Arc<Server>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/command", post(command))
        .route("/snapshot", post(snapshot))
        .route("/rebuild-request", post(rebuild_request_handler))
        .route("/hub/rebuild/{request_id}", get(saved_rebuild_request))
        .route("/hub/rebuild/{request_id}/confirm", post(confirm_rebuild))
        .route("/baseline/coldstart", post(coldstart))
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

impl Server {
    fn coldstart(&self, request: ColdstartRequest) -> Result<Value, String> {
        let catalog = self.catalog(&request.directory)?;
        let prepared = prepare_coldstart(&catalog, &request.options)?;
        self.finish_read(&catalog, request.run.as_deref(), prepared)
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

    fn recovery_inputs(
        &self,
        catalog: &Catalog,
    ) -> Result<towermd_core::recovery_outbox::RecoveryInputs, String> {
        towermd_core::recovery_outbox::RecoveryInputs::read(
            &self
                .runtime
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            catalog,
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

    fn execute(&self, request: Request) -> Result<Value, TaskFailure> {
        self.main_refresh.observe(&request.directory)?;
        let catalog = match Catalog::load_optional(&request.directory)? {
            Some(catalog) => catalog,
            None if matches!(&request.command, TaskCommand::Status) => {
                return self.main_diagnostics(&request.directory, json!({
                "status": "authority_unavailable",
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
                if let Some(mut value) = towermd_core::recovery::status_source(
                    &catalog,
                    &self.recovery_inputs(&catalog)?,
                )? {
                    if let Some(error) = failure {
                        value["error"] = json!(error);
                    }
                    return self
                        .main_diagnostics(&request.directory, value)
                        .map_err(Into::into);
                }
            }
        }
        let hub = self.hub(&catalog)?;
        let inputs = self.recovery_inputs(&catalog)?;
        let value = if matches!(request.command, TaskCommand::Status) {
            // CLI activity renews only actual complete fences, independently of the pure view.
            self.renew_read_hub(&hub, &catalog, request.run.as_deref())?;
            ProjectSession::read_status_shared(
                &hub,
                &catalog,
                &inputs,
                &self.endpoint.host,
                request.run.as_deref(),
                (self.clock)()?,
            )?
        } else {
            ProjectSession::execute_shared(
                &hub,
                &catalog,
                &inputs,
                &self.endpoint.host,
                request.command,
                request.run.as_deref(),
                (self.clock)()?,
            )?
        };
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
                    let hub = Mutex::new(ProjectSession::new(
                        Box::new(Hub::open(&catalog.common_dir.join("towermd"))?),
                        catalog,
                        &self.recovery_inputs(catalog)?,
                        (self.clock)()?,
                    )?);
                    let project = Arc::new(Project {
                        common_dir: catalog.common_dir.clone(),
                        root: Mutex::new(catalog.root.clone()),
                        hub: Arc::new(hub),
                        writer: Arc::new(SnapshotWriter::default()),
                        snapshot_interval: AtomicU64::new(snapshot_interval),
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
        Ok(project)
    }

    fn snapshot(&self, request: SnapshotRequest) -> Result<Value, String> {
        let mut catalog = self.catalog(&request.directory)?;
        let project = self.project(&catalog)?;
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
        let report =
            project
                .writer
                .write(&project.hub, &catalog, &self.endpoint.host, true, || {
                    (self.clock)()
                })?;
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
            return Err("恢复请求项目或 clone 已变化，须新请求".into());
        }
        // Hub::open repairs incomplete tails. Recovery must preserve damaged bytes instead.
        if inspect_store(&saved.common_dir.join("towermd"))? == StoreState::Nonempty {
            return Err("hub journal 仍有可读历史，拒绝将其作为丢失数据覆盖".into());
        }
        let inputs = self.recovery_inputs(&catalog)?;
        let interval = snapshot_service::interval_ms(&catalog)?;
        let mut project = self.registered_project(catalog.config.project_id())?;
        let registration = if project.is_none() {
            let guard = self.registration.lock().map_err(|_| "项目注册锁损坏")?;
            project = self.registered_project(catalog.config.project_id())?;
            Some(guard)
        } else {
            None
        };
        let report = if let Some(project) = project {
            drop(registration);
            if project.common_dir != saved.common_dir {
                return Err("项目 ID 已绑定另一个 clone".into());
            }
            let result = project.writer.confirm_rebuild(
                Some(project.hub.clone()),
                &catalog,
                &saved.request,
                actor,
                saved.expires_at,
                &inputs,
                &self.endpoint.host,
                || (self.clock)(),
            );
            let (report, hub) = match result {
                Ok(value) => value,
                Err(error) => {
                    if error.contains("已持久化") {
                        *project
                            .recovery_error
                            .lock()
                            .map_err(|_| "恢复诊断锁损坏")? = Some(error.clone());
                    }
                    return Err(error.into());
                }
            };
            debug_assert!(Arc::ptr_eq(&hub, &project.hub));
            *project.root.lock().map_err(|_| "项目路径锁损坏")? = catalog.root.clone();
            *project
                .recovery_error
                .lock()
                .map_err(|_| "恢复诊断锁损坏")? = None;
            project.snapshot_interval.store(interval, Ordering::Relaxed);
            project.reconcile.store(true, Ordering::Relaxed);
            report
        } else {
            let project_lock = bind_project(
                &self.runtime,
                catalog.config.project_id(),
                &saved.common_dir,
            )?;
            let writer = Arc::new(SnapshotWriter::default());
            let (report, hub) = writer.confirm_rebuild(
                None,
                &catalog,
                &saved.request,
                actor,
                saved.expires_at,
                &inputs,
                &self.endpoint.host,
                || (self.clock)(),
            )?;
            let project = Arc::new(Project {
                common_dir: saved.common_dir.clone(),
                root: Mutex::new(catalog.root.clone()),
                hub,
                writer,
                snapshot_interval: AtomicU64::new(interval),
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
            let mut background = project.background.lock().map_err(|_| "快照后台锁损坏")?;
            if *self.shutdown.borrow() || background.as_ref().is_some_and(|job| !job.is_finished())
            {
                continue;
            }
            let worker = project.clone();
            let host = self.endpoint.host.clone();
            let runtime = self.runtime.clone();
            let clock = self.clock.clone();
            let shutdown = self.shutdown.subscribe();
            // The existing per-project slot owns the entire business maintenance lifecycle.
            // Busy projects do not queue or spawn another worker; shutdown drains this same slot.
            *background = Some(tokio::task::spawn_blocking(move || {
                let result = (|| -> Result<(), String> {
                    if *shutdown.borrow() {
                        return Ok(());
                    }
                    let root = worker.root.lock().map_err(|_| "项目路径锁损坏")?.clone();
                    let catalog = Catalog::load(&root)?;
                    let inputs = towermd_core::recovery_outbox::RecoveryInputs::read(
                        &runtime
                            .join("recovery-outbox")
                            .join(catalog.config.project_id()),
                        &catalog,
                    )?;
                    ProjectSession::tick_shared(&worker.hub, &catalog, &inputs, &host, clock()?)?;
                    worker
                        .snapshot_interval
                        .store(snapshot_service::interval_ms(&catalog)?, Ordering::Relaxed);
                    let due = {
                        let hub = worker.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
                        snapshot_service::due(
                            &hub,
                            worker.snapshot_interval.load(Ordering::Relaxed),
                            clock()?,
                        )?
                    };
                    if *shutdown.borrow() || (!due && !worker.reconcile.load(Ordering::Relaxed)) {
                        return Ok(());
                    }
                    let report =
                        worker
                            .writer
                            .write(&worker.hub, &catalog, &host, false, || clock())?;
                    if report.status != "deferred" {
                        worker.reconcile.store(false, Ordering::Relaxed);
                    }
                    Ok(())
                })();
                if let Err(error) = result {
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
        let hub = loader.hub(&catalog)?;
        ProjectSession::subscribe_shared(
            &hub,
            &catalog,
            &loader.recovery_inputs(&catalog)?,
            after,
            (loader.clock)()?,
        )
        .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))?;
        Ok::<_, String>((hub, catalog.root))
    })
    .await;
    match result {
        Ok(Ok((hub, root))) => upgrade.on_upgrade(move |socket| {
            stream_events(
                socket,
                hub,
                after,
                server.shutdown.subscribe(),
                server.clock.clone(),
                root,
                server.runtime.clone(),
            )
        }),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, error).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn stream_events(
    mut socket: WebSocket,
    hub: Arc<Mutex<ProjectSession>>,
    mut after: u64,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    clock: Arc<dyn Fn() -> Result<u64, String> + Send + Sync>,
    root: PathBuf,
    runtime: PathBuf,
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
                let hub = hub.clone();let clock=clock.clone();let root=root.clone();let runtime=runtime.clone();
                let batch = tokio::task::spawn_blocking(move || {
                    let now=clock().map_err(HubError::Invalid)?;
                    let catalog=Catalog::load(&root).map_err(HubError::Invalid)?;
                    let inputs=towermd_core::recovery_outbox::RecoveryInputs::read(&runtime.join("recovery-outbox").join(catalog.config.project_id()),&catalog).map_err(HubError::Invalid)?;
                    ProjectSession::subscribe_shared(&hub,&catalog,&inputs,after,now)
                }).await;
                let events=match batch {Ok(Ok(batch))=>batch.events,Ok(Err(error @ HubError::SnapshotRequired{..}))=>{if let Ok(text)=serde_json::to_string(&json!({"error":error})){let _=tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Text(text.into()))).await;}let _=tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Close(None))).await;return;},_=>return};
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
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    if headers
        .get("x-towermd-client")
        .and_then(|v| v.to_str().ok())
        != Some("app")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"恢复只能由 App 确认"})),
        );
    }
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
    if !body.actor.strip_prefix("human:").is_some_and(|id| {
        !id.is_empty()
            && !id.contains([':', '/'])
            && !id.chars().any(|c| c.is_whitespace() || c.is_control())
    }) {
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
    read_response(tokio::task::spawn_blocking(move || server.coldstart(request)).await)
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

#[cfg(test)]
mod tests {
    use super::*;

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
            fs::write(&program, r#"#!/bin/sh
if test -f "$TOWERMD_I2_GATES/a-armed" && test "$(pwd -P)" = "$(cat "$TOWERMD_I2_GATES/a-root")"; then
    cat_file=0
    blob=0
    month=0
    target=$(cat "$TOWERMD_I2_GATES/a-blob")
    for argument in "$@"; do
        if test "$argument" = cat-file; then cat_file=1; fi
        if test "$argument" = blob; then blob=1; fi
        if test "$argument" = "$target"; then month=1; fi
    done
    if test "$cat_file$blob$month" = 111 && mkdir "$TOWERMD_I2_GATES/a-once" 2>/dev/null; then
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
"#).unwrap();
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
            let config = root.join(".towermd/config.yaml");
            fs::write(
                &config,
                format!(
                    "{}\nsnapshot_interval: 1\n",
                    fs::read_to_string(&config).unwrap()
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
            let runtime = temporary.path().join("runtime"); fs::create_dir(&runtime).unwrap();
            let (shutdown, _receiver) = tokio::sync::watch::channel(false);
            let server = Arc::new(Server { main_refresh: MainRefresh::default(), clock: Arc::new(move||Ok(clock.load(Ordering::SeqCst))), endpoint: Endpoint { address:"127.0.0.1".into(),port:0,pid:std::process::id(),instance:"ticker-proof".into(),host:"ticker-host".into() },runtime,projects:Mutex::new(BTreeMap::new()),registration:Mutex::new(()),shutdown });
            for (root, id) in [(&a,"T-A"),(&b,"T-B")] {
                server.execute(Request { directory:root.clone(),run:None,command:serde_json::from_value(json!({"action":"add","task":{"id":id,"title":id,"module":"A","allowed_paths":["src/**"],"acceptance":["AC"]}})).unwrap() }).unwrap();
            }
            let a_run = temporary.path().join("a-run"); git(&a, &["worktree","add","-qb","t/T-A",a_run.to_str().unwrap(),"main"]);
            let run = server.execute(Request { directory:a_run.clone(),run:None,command:TaskCommand::Claim {id:"T-A".into(),harness:None,harness_version:None} }).unwrap();
            let ac = Catalog::load(&a_run).unwrap(); let bc=Catalog::load(&b).unwrap();
            let ap=server.project(&ac).unwrap(); let bp=server.project(&bc).unwrap();
            assert_eq!(ap.writer.write(&ap.hub,&ac,&server.endpoint.host,true,||Ok(start)).unwrap().status,"written");
            let applied=ap.hub.lock().unwrap().applied_seq();
            let run_before=ap.hub.lock().unwrap().leases(start)["module:A"].clone();
            let month=git(&a_run,&["ls-tree","-r","--name-only","refs/towermd/snapshot","--","docs/tower/state/events"]);
            let blob=git(&a_run,&["rev-parse",&format!("refs/towermd/snapshot:{}",month.lines().next().unwrap())]);
            fs::write(gates.join("a-blob"),blob).unwrap(); fs::write(gates.join("a-root"),a_run.canonicalize().unwrap().to_str().unwrap()).unwrap();
            time.store(start+2000,Ordering::SeqCst);
            let (stop,ticker)=start_ticker(server.clone());
            let b_entered=wait_file(&gates.join("b-entered")).await;
            if !b_entered { fs::write(gates.join("b-release"),"").unwrap(); server.shutdown.send(true).unwrap(); server.main_refresh.drain().await.unwrap(); server.drain_snapshots().await.unwrap(); stop.send(true).unwrap(); ticker.await.unwrap(); panic!("actual B background writer did not reach remote push"); }
            let old_a=ap.hub.lock().unwrap().leases(start+2000)["main"].clone();
            let old_b=bp.hub.lock().unwrap().leases(start+2000)["main"].clone();
            git(&a,&["commit","--allow-empty","-qm","advance A source before slow proof"]);
            fs::write(gates.join("a-armed"),"").unwrap();
            let a_entered=wait_file(&gates.join("a-entered")).await;
            time.store(start+33_000,Ordering::SeqCst);
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
                let crossed=old_b.expires_at+1000; time.store(crossed,Ordering::SeqCst);
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
            let drain_now=time.load(Ordering::SeqCst)+33_000; time.store(drain_now,Ordering::SeqCst);
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
            runtime,
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
            shutdown,
            main_refresh: MainRefresh::default(),
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
                            root,
                            runtime,
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
            .write(&session, &catalog, "host", true, || Ok(2))
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
            clock: Arc::new(|| Ok(1000)),
            endpoint: Endpoint {
                address: "127.0.0.1".into(),
                port: 0,
                pid: std::process::id(),
                instance: "proof-matrix".into(),
                host: "host".into(),
            },
            runtime: runtime.clone(),
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
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
        let route_root = root.clone();
        let route_runtime = runtime.clone();
        let route_shutdown = server.shutdown.clone();
        let app = Router::new().route(
            "/events",
            get(move |upgrade: WebSocketUpgrade| {
                let hub = route_hub.clone();
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
                            root,
                            runtime,
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
            root.join("docs/tower/arch/A.md"),
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
            clock: Arc::new(|| Ok(2000)),
            endpoint: Endpoint {
                address: "127.0.0.1".into(),
                port: 0,
                pid: std::process::id(),
                instance: Uuid::now_v7().to_string(),
                host: "fixture-host".into(),
            },
            runtime,
            projects: Mutex::new(BTreeMap::new()),
            registration: Mutex::new(()),
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
            .write(&history, &catalog, "fixture-host", true, || Ok(1000))
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
            Arc::get_mut(&mut server).unwrap().clock = Arc::new(move || {
                let catalog = Catalog::load(&fault_root)?;
                if !observed.load(Ordering::SeqCst)
                    && snapshot_service::read_snapshot(&catalog)?
                        .unwrap()
                        .snapshot
                        .through_seq
                        > boundary
                {
                    fs::rename(&fault_journal, &fault_backup).map_err(|e| e.to_string())?;
                    fs::create_dir(&fault_journal).map_err(|e| e.to_string())?;
                    observed.store(true, Ordering::SeqCst);
                }
                Ok(2000)
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
                    let project_file = project._lock.metadata().unwrap();
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
                        let project_file = project._lock.metadata().unwrap();
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
        ] {
            let (_temp, mut server, run_tree, _old) = rebuild_fixture(true);
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
                "late-delete-private" | "late-delete-main" => {}
                _ => {
                    rebuild_git(&root, &["update-ref", "-d", removed]);
                }
            }
            let mut refs_after_removal = ["refs/heads/main", "refs/towermd/snapshot"]
                .map(|name| fs::read(catalog.common_dir.join(name)).ok());
            if removed.starts_with("late-delete-") {
                let reference = if removed == "late-delete-main" {
                    "refs/heads/main"
                } else {
                    "refs/towermd/snapshot"
                };
                refs_after_removal[usize::from(reference == "refs/towermd/snapshot")] = None;
                let source_root = root.clone();
                let calls = AtomicU64::new(0);
                Arc::get_mut(&mut server).unwrap().clock = Arc::new(move || {
                    // First clock is HTTP expiry; second is core confirmation after daemon's source check/Catalog.
                    if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                        rebuild_git(&source_root, &["update-ref", "-d", reference]);
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
            let observation = json!({"removed":removed,"saved_source":request["source_commit"],"http_status":code.as_u16(),"response":response,"target_exists_after":directory.exists(),"project_published_after":server.registered_project(catalog.config.project_id()).unwrap().is_some(),"expected_status":if removed == "read-io" {400} else {409},"refs_equal_after_confirm":refs_after_removal==["refs/heads/main", "refs/towermd/snapshot"].map(|name| fs::read(catalog.common_dir.join(name)).ok()),"saved_request_equal":saved_before==fs::read(saved_path).unwrap()});
            eprintln!("SPEC_DISAPPEARED_SOURCE_OBSERVATION={observation}");
            observations.push(observation);
            stop.send(()).unwrap();
            serving.await.unwrap();
            server.shutdown.send_replace(true);
            server.main_refresh.drain().await.unwrap();
            server.drain_snapshots().await.unwrap();
        }
        assert!(observations.iter().all(|o|o["http_status"]==o["expected_status"] && o["target_exists_after"]==false && o["project_published_after"]==false && o["refs_equal_after_confirm"]==true && o["saved_request_equal"]==true),"a saved recovery source/authority that disappeared must conflict without target changes: {observations:?}");
    }
}
