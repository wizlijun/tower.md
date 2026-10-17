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
use towermd_core::hub::{probe_store, Hub};
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
    let result=daemon_runtime.block_on(async {
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
            let tick_server = server.clone();
            let (ticker_stop,mut tick_shutdown)=tokio::sync::watch::channel(false);
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
            let app = Router::new()
                .route("/health", get(health))
                .route("/command", post(command))
                .route("/snapshot", post(snapshot))
                .route("/rebuild-request", post(rebuild_request_handler))
                .route("/baseline/coldstart", post(coldstart))
                .route("/canvas", get(canvas))
                .route("/objects/{id}", get(object))
                .route("/events", get(subscribe))
                .route("/shutdown", post(shutdown))
                .with_state(server.clone());
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
            let _=ticker_stop.send(true);
            let ticked=ticker.await.map_err(|error|format!("项目维护退出失败：{error}"));
            result.and(fetched).and(drained).and(ticked)
        });
    // Runtime waits for all blocking work while the daemon/file ownership is still held.
    drop(daemon_runtime);
    drop(daemon_lock);
    result
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
        let current = validated_catalog(
            hub,
            catalog,
            &self.recovery_inputs(catalog)?,
            (self.clock)()?,
        )?;
        let catalog = current.as_ref().unwrap_or(catalog);
        let result = tasks::renew_binding(
            &mut *hub.lock().map_err(|_| "项目 hub 锁损坏")?,
            catalog,
            &self.endpoint.host,
            run,
            (self.clock)()?,
        );
        if result.is_err() {
            ProjectSession::sync_shared(
                hub,
                catalog,
                &self.recovery_inputs(catalog)?,
                (self.clock)()?,
            )
            .map_err(|e| e.to_string())?;
        }
        result
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
        let hub = self.hub(&catalog)?;
        let inputs = self.recovery_inputs(&catalog)?;
        let value = if matches!(request.command, TaskCommand::Status) {
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
        {
            let mut hub = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
            tasks::execute(
                &mut hub,
                &catalog,
                &self.endpoint.host,
                TaskCommand::Status,
                request.run.as_deref(),
                (self.clock)()?,
            )
            .map_err(|e| e.to_string())?;
        }
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
        let mut catalog = self.catalog(&request.directory)?;
        let renewal = (|| -> Result<(), String> {
            if !probe_store(&catalog.common_dir.join("towermd"))? {
                return if request.run.is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "run {} 无法续租：hub journal 不存在",
                        request.run.as_deref().unwrap_or("隐式绑定")
                    ))
                };
            }
            let hub = self.hub(&catalog)?;
            if let Some(current) = validated_catalog(
                &hub,
                &catalog,
                &self.recovery_inputs(&catalog)?,
                (self.clock)()?,
            )? {
                catalog = current;
            }
            let mut hub = hub.lock().map_err(|_| "项目 hub 锁损坏")?;
            tasks::execute(
                &mut hub,
                &catalog,
                &self.endpoint.host,
                TaskCommand::Status,
                request.run.as_deref(),
                (self.clock)()?,
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })();
        let warning = match renewal {
            Ok(()) => None,
            Err(error) if request.run.is_some() => return Err(error),
            Err(error) => Some(format!(
                "隐式续租失败，未确认租约延长；仅生成待 App 确认的恢复请求：{error}"
            )),
        };
        let mut response = serde_json::to_value(towermd_core::recovery::prepare(
            &catalog,
            &self.recovery_inputs(&catalog)?,
            (self.clock)()?,
            (self.clock)()?
                .checked_add(300_000)
                .ok_or("expiry overflow")?,
            &Uuid::now_v7().to_string(),
        )?)
        .map_err(|error| error.to_string())?;
        if let Some(warning) = warning {
            response["warnings"] = json!([warning]);
        }
        self.main_diagnostics(&catalog.root, response)
    }

    fn tick(&self) -> Result<(), String> {
        if !*self.shutdown.borrow() {
            self.main_refresh.tick()?;
        }
        let now = (self.clock)()?;
        for project in self.projects()? {
            let result = (|| -> Result<(), String> {
                {
                    let mut hub = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
                    snapshot_service::maintain_main(&mut hub, &self.endpoint.host, now)?;
                }
                if *self.shutdown.borrow() {
                    return Ok(());
                }
                let root = project.root.lock().map_err(|_| "项目路径锁损坏")?.clone();
                let catalog = Catalog::load(&root)?;
                let inputs = self.recovery_inputs(&catalog)?;
                ProjectSession::sync_shared(&project.hub, &catalog, &inputs, now)
                    .map_err(|e| e.to_string())?;
                let due = {
                    let mut hub = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
                    tasks::expire(&mut hub, &self.endpoint.host, now)?;
                    snapshot_service::due(
                        &hub,
                        project.snapshot_interval.load(Ordering::Relaxed),
                        now,
                    )?
                };
                let mut background = project.background.lock().map_err(|_| "快照后台锁损坏")?;
                if *self.shutdown.borrow()
                    || background.as_ref().is_some_and(|job| !job.is_finished())
                    || (!due && !project.reconcile.load(Ordering::Relaxed))
                {
                    return Ok(());
                }
                let root = project.root.lock().map_err(|_| "项目路径锁损坏")?.clone();
                let worker = project.clone();
                let host = self.endpoint.host.clone();
                let clock = self.clock.clone();
                *background = Some(tokio::task::spawn_blocking(move || {
                    let result = (|| -> Result<(), String> {
                        let catalog = Catalog::load(&root)?;
                        worker
                            .snapshot_interval
                            .store(snapshot_service::interval_ms(&catalog)?, Ordering::Relaxed);
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
                        eprintln!("towermd 后台快照失败（{}）：{error}", root.display());
                    }
                }));
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!(
                    "towermd 项目维护失败（{}）：{error}",
                    project.common_dir.display()
                );
            }
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
        validated_catalog(
            &hub,
            &catalog,
            &loader.recovery_inputs(&catalog)?,
            (loader.clock)()?,
        )?;
        {
            let mut hub = hub.lock().map_err(|_| "项目 hub 锁损坏")?;
            hub.subscribe(after, (loader.clock)()?)
                .map_err(|e| serde_json::to_string(&e).unwrap_or_else(|_| e.to_string()))?;
        }
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
                    ProjectSession::sync_shared(&hub,&catalog,&inputs,now).map_err(|e|HubError::Invalid(e.to_string()))?;
                    hub.lock().map_err(|_|HubError::Invalid("session lock poisoned".into()))?.subscribe(after,now)
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
        server.tick().unwrap();
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
}
