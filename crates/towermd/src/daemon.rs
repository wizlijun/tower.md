use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
use towermd_core::hub::Hub;
use towermd_core::snapshot_service::{self, SnapshotWriter};
use towermd_core::tasks::{self, TaskCommand};
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
    hub: Arc<Mutex<Hub>>,
    writer: Arc<SnapshotWriter>,
    snapshot_interval: AtomicU64,
    reconcile: AtomicBool,
    background: Mutex<Option<tokio::task::JoinHandle<()>>>,
    _lock: File,
}

struct Server {
    endpoint: Endpoint,
    runtime: PathBuf,
    projects: Mutex<BTreeMap<String, Arc<Project>>>,
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
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?
        .block_on(async {
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
                endpoint,
                runtime: runtime.clone(),
                projects: Mutex::new(BTreeMap::new()),
                shutdown: shutdown_sender,
            });
            let tick_server = server.clone();
            let ticker = tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    let server = tick_server.clone();
                    let result = tokio::task::spawn_blocking(move || server.tick()).await;
                    if let Err(error) = result.unwrap_or_else(|error| Err(error.to_string())) {
                        eprintln!("towermd 租约到期检查失败：{error}");
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
            let drained = server.drain_snapshots().await;
            ticker.abort();
            result.and(drained)
        })
}

impl Server {
    fn coldstart(&self, request: ColdstartRequest) -> Result<Value, String> {
        let catalog = Catalog::load(&request.directory)?;
        let prepared = prepare_coldstart(&catalog, &request.options)?;
        self.finish_read(&catalog, request.run.as_deref(), prepared)
    }

    fn canvas(&self, query: ReadQuery) -> Result<Value, String> {
        let catalog = Catalog::load(&query.directory)?;
        let view = CanvasView::load(&catalog)?;
        self.finish_read(&catalog, query.run.as_deref(), view)
    }

    fn object(&self, query: ReadQuery, id: &str) -> Result<Option<Value>, String> {
        let catalog = Catalog::load(&query.directory)?;
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
        Ok(value)
    }

    fn renew_existing_read(&self, catalog: &Catalog, run: Option<&str>) -> Result<(), String> {
        let bytes = match fs::read(catalog.common_dir.join("towermd/journal.json")) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && run.is_none() => {
                return Ok(())
            }
            Err(error) => {
                return Err(format!(
                    "run {} 无法续租：读取已有 journal 失败：{error}",
                    run.unwrap_or("隐式绑定")
                ))
            }
        };
        serde_json::from_slice::<Value>(&bytes)
            .map_err(|error| format!("已有 journal 无效：{error}"))?;
        let projects = self.projects.lock().map_err(|_| "项目注册表锁损坏")?;
        if let Some(project) = projects.get(catalog.config.project_id()) {
            if project.common_dir != catalog.common_dir {
                return Err("项目 ID 已绑定另一个 clone".into());
            }
            let mut hub = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
            self.renew_read_hub(&mut hub, catalog, run)
        } else {
            let lock_name = hex::encode(Sha256::digest(catalog.config.project_id().as_bytes()));
            let owner = fs::read(self.runtime.join(format!("project-{lock_name}.json")))
                .map_err(|error| format!("无法确认已有项目 clone 绑定，不续租：{error}"))?;
            validate_project_owner(&owner, &catalog.common_dir)?;
            let mut hub = Hub::open(&catalog.common_dir.join("towermd"))?;
            self.renew_read_hub(&mut hub, catalog, run)
        }
    }

    fn renew_read_hub(
        &self,
        hub: &mut Hub,
        catalog: &Catalog,
        run: Option<&str>,
    ) -> Result<(), String> {
        snapshot_service::validate_authority(hub, catalog)?;
        tasks::renew_binding(hub, catalog, &self.endpoint.host, run, now_ms()?)
    }

    fn execute(&self, request: Request) -> Result<Value, String> {
        let catalog = Catalog::load(&request.directory)?;
        let hub = self.hub(&catalog)?;
        let mut hub = hub.lock().map_err(|_| "项目 hub 锁损坏")?;
        snapshot_service::validate_authority(&hub, &catalog)?;
        tasks::execute(
            &mut hub,
            &catalog,
            &self.endpoint.host,
            request.command,
            request.run.as_deref(),
            now_ms()?,
        )
    }

    fn hub(&self, catalog: &Catalog) -> Result<Arc<Mutex<Hub>>, String> {
        Ok(self.project(catalog)?.hub.clone())
    }

    fn project(&self, catalog: &Catalog) -> Result<Arc<Project>, String> {
        let snapshot_interval = snapshot_service::interval_ms(catalog)?;
        Ok({
            let mut projects = self.projects.lock().map_err(|_| "项目注册表锁损坏")?;
            let project_id = catalog.config.project_id();
            if let Some(existing) = projects.get(project_id) {
                if existing.common_dir != catalog.common_dir {
                    return Err(
                        "同一项目 ID 已由另一个 clone 占用；请使用 git worktree 或云 hub".into(),
                    );
                }
                *existing.root.lock().map_err(|_| "项目路径锁损坏")? = catalog.root.clone();
                existing
                    .snapshot_interval
                    .store(snapshot_interval, Ordering::Relaxed);
                existing.clone()
            } else {
                let project_lock = bind_project(&self.runtime, project_id, &catalog.common_dir)?;
                let hub = Hub::open(&catalog.common_dir.join("towermd"))?;
                snapshot_service::validate_authority(&hub, catalog)?;
                let project = Arc::new(Project {
                    common_dir: catalog.common_dir.clone(),
                    root: Mutex::new(catalog.root.clone()),
                    hub: Arc::new(Mutex::new(hub)),
                    writer: Arc::new(SnapshotWriter::default()),
                    snapshot_interval: AtomicU64::new(snapshot_interval),
                    reconcile: AtomicBool::new(true),
                    background: Mutex::new(None),
                    _lock: project_lock,
                });
                projects.insert(project_id.into(), project.clone());
                project
            }
        })
    }

    fn snapshot(&self, request: SnapshotRequest) -> Result<Value, String> {
        let catalog = Catalog::load(&request.directory)?;
        let project = self.project(&catalog)?;
        {
            let mut hub = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
            snapshot_service::validate_authority(&hub, &catalog)?;
            tasks::execute(
                &mut hub,
                &catalog,
                &self.endpoint.host,
                TaskCommand::Status,
                request.run.as_deref(),
                now_ms()?,
            )?;
        }
        let report =
            project
                .writer
                .write(&project.hub, &catalog, &self.endpoint.host, true, now_ms)?;
        serde_json::to_value(report).map_err(|error| error.to_string())
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
        let catalog = Catalog::load(&request.directory)?;
        let renewal = (|| -> Result<(), String> {
            let journal = match fs::read(catalog.common_dir.join("towermd/journal.json")) {
                Ok(bytes) => bytes,
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound && request.run.is_none() =>
                {
                    return Ok(());
                }
                Err(error) => return Err(format!("无法读取 hub journal，未续租：{error}")),
            };
            serde_json::from_slice::<Value>(&journal)
                .map_err(|error| format!("hub journal 无效，未续租：{error}"))?;
            let hub = self.hub(&catalog)?;
            let mut hub = hub.lock().map_err(|_| "项目 hub 锁损坏")?;
            snapshot_service::validate_authority(&hub, &catalog)?;
            tasks::execute(
                &mut hub,
                &catalog,
                &self.endpoint.host,
                TaskCommand::Status,
                request.run.as_deref(),
                now_ms()?,
            )?;
            Ok(())
        })();
        let warning = match renewal {
            Ok(()) => None,
            Err(error) if request.run.is_some() => return Err(error),
            Err(error) => Some(format!(
                "隐式续租失败，未确认租约延长；仅生成待 App 确认的恢复请求：{error}"
            )),
        };
        let mut response = serde_json::to_value(towermd_core::recovery::prepare(&catalog)?)
            .map_err(|error| error.to_string())?;
        if let Some(warning) = warning {
            response["warnings"] = json!([warning]);
        }
        Ok(response)
    }

    fn tick(&self) -> Result<(), String> {
        let now = now_ms()?;
        for project in self.projects()? {
            let result = (|| -> Result<(), String> {
                let due = {
                    let mut hub = project.hub.lock().map_err(|_| "项目 hub 锁损坏")?;
                    snapshot_service::maintain_main(&mut hub, &self.endpoint.host, now)?;
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
                *background = Some(tokio::task::spawn_blocking(move || {
                    let result = (|| -> Result<(), String> {
                        let catalog = Catalog::load(&root)?;
                        worker
                            .snapshot_interval
                            .store(snapshot_service::interval_ms(&catalog)?, Ordering::Relaxed);
                        let report =
                            worker
                                .writer
                                .write(&worker.hub, &catalog, &host, false, now_ms)?;
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
        let catalog = Catalog::load(&subscription.directory)?;
        let hub = loader.hub(&catalog)?;
        {
            let hub = hub.lock().map_err(|_| "项目 hub 锁损坏")?;
            snapshot_service::validate_authority(&hub, &catalog)?;
            validate_subscription_cursor(
                after,
                hub.base_snapshot()
                    .map_or(0, |snapshot| snapshot.through_seq),
                hub.last_seq(),
            )?;
        }
        Ok::<_, String>(hub)
    })
    .await;
    match result {
        Ok(Ok(hub)) => upgrade.on_upgrade(move |socket| {
            stream_events(socket, hub, after, server.shutdown.subscribe())
        }),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, error).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn stream_events(
    mut socket: WebSocket,
    hub: Arc<Mutex<Hub>>,
    mut after: u64,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
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
                let hub = hub.clone();
                let events = tokio::task::spawn_blocking(move || {
                    hub.lock().map(|hub| hub.events(after)).map_err(|_| ())
                }).await;
                let Ok(Ok(events)) = events else { return; };
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
}
