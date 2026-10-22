use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::output::{lang, ErrorCode, Failure, Next};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    #[serde(default)]
    api_version: u32,
    address: String,
    port: u16,
    pid: u32,
    instance: String,
    pub(crate) host: String,
}

pub(crate) struct Server {
    pub(crate) endpoint: Endpoint,
    shutdown: tokio::sync::watch::Sender<bool>,
    pub(crate) runtime: PathBuf,
    pub(crate) shutting_down: std::sync::atomic::AtomicBool,
    pub(crate) verification_lane: std::sync::Mutex<()>,
    pub(crate) projects: std::sync::Mutex<crate::project_service::Projects>,
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
        if start {
            require_current_api(&endpoint)?;
        }
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

pub fn ensure() -> Result<Value, Failure> {
    let endpoint = connect(true).map_err(unavailable)?.ok_or_else(|| unavailable("守护进程未启动"))?;
    Ok(serde_json::to_value(endpoint).map_err(|error| error.to_string())?)
}

pub fn status() -> Result<Value, Failure> {
    match connect(false).map_err(unavailable)? {
        Some(endpoint) => Ok(serde_json::to_value(endpoint).map_err(|error| error.to_string())?),
        None => Ok(json!({"running": false})),
    }
}

pub fn stop() -> Result<Value, Failure> {
    let runtime = runtime_dir().map_err(unavailable)?;
    if let Some(endpoint) = connect(false).map_err(unavailable)? {
        let status = client(Duration::from_secs(5))?
            .post(url(&endpoint, "/shutdown"))
            .header("x-towermd-instance", &endpoint.instance)
            .send()
            .map_err(|error| unavailable(error.to_string()))?
            .status();
        if !status.is_success() {
            return Err(daemon_failure(status, &json!({"error": format!("守护进程拒绝停止请求（{status}）")})));
        }
        let daemon_lock = lock_file(&runtime.join("daemon.lock"))?;
        wait_lock(&daemon_lock)
            .map_err(|error| format!("守护进程仍在停止，请稍后检查：{error}"))?;
    }
    Ok(json!({"running": false}))
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
    let result = daemon_runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| error.to_string())?;
        let endpoint = Endpoint {
            api_version: 7,
            address: "127.0.0.1".into(),
            port: listener
                .local_addr()
                .map_err(|error| error.to_string())?
                .port(),
            pid: std::process::id(),
            instance: Uuid::now_v7().to_string(),
            host,
        };
        let (shutdown, mut receiver) = tokio::sync::watch::channel(false);
        let server = Arc::new(Server {
            endpoint,
            shutdown,
            runtime: runtime.clone(),
            shutting_down: std::sync::atomic::AtomicBool::new(false),
            verification_lane: std::sync::Mutex::new(()),
            projects: std::sync::Mutex::new(crate::project_service::Projects::default()),
        });
        write_json(&runtime.join("endpoint.json"), &server.endpoint)?;
        let maintenance_server = Arc::clone(&server);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                if maintenance_server
                    .shutting_down
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    break;
                }
                let server = Arc::clone(&maintenance_server);
                let _ = tokio::task::spawn_blocking(move || {
                    crate::project_service::business::maintain(&server);
                })
                .await;
            }
        });
        axum::serve(listener, router(Arc::clone(&server)))
            .with_graceful_shutdown(async move {
                while !*receiver.borrow() {
                    if receiver.changed().await.is_err() {
                        break;
                    }
                }
                crate::project_service::cancel_all(&server);
            })
            .await
            .map_err(|error| error.to_string())
    });
    drop(daemon_runtime);
    drop(daemon_lock);
    result
}

fn router(server: Arc<Server>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/shutdown", post(shutdown))
        .route("/projects/open", post(crate::project_service::open))
        .route("/projects/{handle}/view", get(crate::project_service::view))
        .route(
            "/projects/{handle}/state",
            get(crate::project_service::business::state),
        )
        .route(
            "/projects/{handle}/main-resolutions/prepare",
            post(crate::project_service::main_resolutions::prepare),
        )
        .route(
            "/projects/{handle}/main-resolutions/confirm",
            post(crate::project_service::main_resolutions::confirm),
        )
        .route(
            "/projects/{handle}/runs/{id}/events",
            get(crate::project_service::run_details::events),
        )
        .route(
            "/projects/{handle}/tasks",
            get(crate::project_service::tasks::show).post(crate::project_service::tasks::add),
        )
        .route(
            "/projects/{handle}/tasks/{id}/claim",
            post(crate::project_service::tasks::claim),
        )
        .route(
            "/projects/{handle}/task-release",
            post(crate::project_service::tasks::release),
        )
        .route(
            "/projects/{handle}/task-status",
            post(crate::project_service::tasks::status),
        )
        .route("/projects/{handle}/main-proposals",post(crate::project_service::proposals::capture_main))
        .route("/projects/{handle}/proposals",get(crate::project_service::proposals::list).post(crate::project_service::proposals::submit))
        .route("/projects/{handle}/proposals/{id}",get(crate::project_service::proposals::get))
        .route("/projects/{handle}/proposals/{id}/publication/prepare",post(crate::project_service::proposals::prepare))
        .route("/projects/{handle}/proposals/{id}/publication/approve",post(crate::project_service::proposals::approve))
        .route("/projects/{handle}/proposals/{id}/reject",post(crate::project_service::proposals::reject))
        .route("/projects/{handle}/task-check",post(crate::project_service::finish::check))
        .route("/projects/{handle}/task-finish",post(crate::project_service::finish::start))
        .route("/projects/{handle}/task-finish/{id}",get(crate::project_service::finish::get))
        .route("/projects/{handle}/task-trailers",post(crate::project_service::finish::trailers))
        .route("/projects/{handle}/ask",post(crate::project_service::questions::ask))
        .route("/projects/{handle}/wait",post(crate::project_service::questions::wait))
        .route("/projects/{handle}/answers",post(crate::project_service::questions::answer))
        .route("/projects/{handle}/answers/relay",post(crate::project_service::questions::relay))
        .route("/projects/{handle}/prompts",post(crate::project_service::questions::prompt))
        .route("/projects/{handle}/pause",post(crate::project_service::project_controls::pause))
        .route("/projects/{handle}/resume-request",post(crate::project_service::project_controls::resume_request))
        .route("/projects/{handle}/resume",post(crate::project_service::project_controls::resume))
        .route("/projects/{handle}/tasks/human",post(crate::project_service::human_tasks::add))
        .route("/projects/{handle}/tasks/options",get(crate::project_service::human_tasks::options))
        .route("/projects/{handle}/events",get(crate::project_service::events::subscribe))
        .route("/projects/{handle}/messages",post(crate::project_service::questions::messages))
        .route("/projects/{handle}/messages/ack",post(crate::project_service::questions::ack))
        .route(
            "/projects/{handle}/baselines/{id}/publication/prepare",
            post(crate::project_service::business::prepare),
        )
        .route(
            "/projects/{handle}/baselines/{id}/publication/approve",
            post(crate::project_service::business::approve),
        )
        .route(
            "/projects/{handle}/snapshot",
            post(crate::project_service::business::snapshot),
        )
        .route(
            "/projects/{handle}/hub/rebuild",
            post(crate::project_service::business::request_rebuild),
        )
        .route(
            "/projects/{handle}/hub/rebuild/{id}/confirm",
            post(crate::project_service::business::confirm_rebuild),
        )
        .route(
            "/projects/{handle}/scans",
            post(crate::project_service::scan),
        )
        .route(
            "/projects/{handle}/scans/{id}",
            get(crate::project_service::get_scan),
        )
        .route(
            "/projects/{handle}/scans/{id}/cancel",
            post(crate::project_service::cancel),
        )
        .route("/drafts/{id}", get(crate::project_service::draft))
        .route(
            "/projects/{handle}/baselines",
            get(crate::project_service::baselines::list)
                .post(crate::project_service::baselines::submit),
        )
        .route(
            "/projects/{handle}/baselines/{id}",
            get(crate::project_service::baselines::get),
        )
        .route(
            "/projects/{handle}/baselines/{id}/review",
            post(crate::project_service::baselines::review),
        )
        .route(
            "/projects/{handle}/baselines/{id}/verify",
            post(crate::project_service::baselines::verify),
        )
        .route(
            "/projects/{handle}/baseline-verifications/{id}",
            get(crate::project_service::baselines::poll_verification),
        )
        .route(
            "/projects/{handle}/baseline-verifications/{id}/cancel",
            post(crate::project_service::baselines::cancel_verification),
        )
        .route(
            "/projects/{handle}/scans/{id}/template",
            get(crate::project_service::baselines::template),
        )
        .with_state(server)
}

async fn health(State(server): State<Arc<Server>>) -> Json<Endpoint> {
    Json(server.endpoint.clone())
}

fn authorized(headers: &HeaderMap, server: &Server) -> bool {
    headers
        .get("x-towermd-instance")
        .and_then(|value| value.to_str().ok())
        == Some(server.endpoint.instance.as_str())
}

async fn shutdown(State(server): State<Arc<Server>>, headers: HeaderMap) -> StatusCode {
    if !authorized(&headers, &server) {
        return StatusCode::CONFLICT;
    }
    crate::project_service::cancel_all(&server);
    let _ = server.shutdown.send(true);
    StatusCode::OK
}

pub(crate) fn authorized_client(headers: &HeaderMap, server: &Server) -> bool {
    authorized(headers, server)
        && matches!(
            headers
                .get("x-towermd-client")
                .and_then(|value| value.to_str().ok()),
            Some("app" | "cli")
        )
}

/// v0.1 separates App human actions from CLI process actions. This is a local
/// protocol boundary, not cryptographic attestation of the calling process.
pub(crate) fn authorized_app(headers: &HeaderMap, server: &Server) -> bool {
    authorized(headers, server)
        && headers
            .get("x-towermd-client")
            .and_then(|v| v.to_str().ok())
            == Some("app")
}

fn request(
    endpoint: &Endpoint,
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, Failure> {
    request_with_timeout(endpoint, method, path, body, Duration::from_secs(30))
}
fn request_with_timeout(
    endpoint: &Endpoint,
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
    timeout: Duration,
) -> Result<Value, Failure> {
    let mut request = client(timeout)?
        .request(method, url(endpoint, path))
        .header("x-towermd-instance", &endpoint.instance)
        .header("x-towermd-client", "cli");
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().map_err(|error| unavailable(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(daemon_failure(status, &error_body(&body)));
    }
    let value: Value = response.json().map_err(|error| format!("守护进程响应无效：{error}"))?;
    Ok(value)
}

/// axum's own rejections (404, 415, 422) and empty bodies are not JSON; the
/// HTTP status still classifies them (D3) and the text becomes the message.
fn error_body(body: &str) -> Value {
    serde_json::from_str::<Value>(body)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({"error": body.trim()}))
}

/// Nothing reached the daemon: retrying unchanged is safe.
fn unavailable(message: impl Into<String>) -> Failure {
    Failure::new(ErrorCode::DaemonUnavailable, message)
        .next(Next::new("towermd daemon status", lang::pick("check the local daemon", "查看本机守护进程")))
}

/// Error body `{"error", "code"?, "hint"?}`. Without a known code the HTTP
/// status decides (D3), so an old or unconverted path is never `internal` by accident.
fn daemon_failure(status: reqwest::StatusCode, value: &Value) -> Failure {
    let message = value.get("error").and_then(Value::as_str).filter(|message| !message.is_empty()).unwrap_or("守护进程请求失败");
    let code = value.get("code").and_then(Value::as_str).and_then(ErrorCode::parse).unwrap_or(match status.as_u16() {
        400 | 422 => ErrorCode::Rejected,
        404 => ErrorCode::NotFound,
        409 => ErrorCode::Conflict,
        _ => ErrorCode::Internal,
    });
    let failure = Failure::new(code, message);
    match value.get("hint").and_then(Value::as_str) {
        Some(hint) => failure.hint(hint),
        None => failure,
    }
}

fn project_connection() -> Result<(Endpoint, String), Failure> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = towermd_core::repo::toplevel(&cwd).map_err(|error| {
        Failure::new(ErrorCode::NotFound, error).hint(lang::pick("run inside a git repository", "请在 git 仓库内运行"))
    })?;
    let endpoint = connect(true).map_err(unavailable)?.ok_or_else(|| unavailable("守护进程未启动"))?;
    let response = request(
        &endpoint,
        reqwest::Method::POST,
        "/projects/open",
        Some(&json!({"path": root})),
    )?;
    let handle = response
        .get("handle")
        .and_then(Value::as_str)
        .ok_or("项目服务响应无效")?
        .to_string();
    Ok((endpoint, handle))
}

pub fn project_view() -> Result<Value, Failure> {
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        reqwest::Method::GET,
        &format!("/projects/{handle}/view"),
        None,
    )
}

pub fn scan_start(request_id: &str) -> Result<Value, Failure> {
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        reqwest::Method::POST,
        &format!("/projects/{handle}/scans"),
        Some(&json!({"request_id": request_id})),
    )
}

pub fn scan_wait(id: &str) -> Result<Value, Failure> {
    let deadline = Instant::now() + Duration::from_secs(85);
    validate_scan_id(id)?;
    let (endpoint, handle) = project_connection()?;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let again = format!("towermd dev scan wait {id}");
            return Err(Failure::new(ErrorCode::Pending, format!("等待扫描超时，请再次运行 {again}"))
                .next(Next::new(again, lang::pick("wait again (at most 90 s per call)", "再次等待（每次最多 90 秒）"))));
        }
        let value = request_with_timeout(
            &endpoint,
            reqwest::Method::GET,
            &format!("/projects/{handle}/scans/{id}"),
            None,
            remaining.min(Duration::from_secs(30)),
        )?;
        if value.get("status").and_then(Value::as_str) != Some("running")
            || Instant::now() >= deadline
        {
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub fn scan_show(id: &str) -> Result<Value, Failure> {
    validate_scan_id(id)?;
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        reqwest::Method::GET,
        &format!("/projects/{handle}/scans/{id}"),
        None,
    )
}

pub fn draft_show(id: &str) -> Result<Value, Failure> {
    towermd_devkit::draft_store::valid_id(id).map_err(|error| Failure::new(ErrorCode::Usage, error))?;
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        reqwest::Method::GET,
        &format!("/projects/{handle}/baselines/{id}"),
        None,
    )
}

pub fn draft_list() -> Result<Value, Failure> {
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        reqwest::Method::GET,
        &format!("/projects/{handle}/baselines"),
        None,
    )
}

pub fn draft_template(id: &str) -> Result<Value, Failure> {
    validate_scan_id(id)?;
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        reqwest::Method::GET,
        &format!("/projects/{handle}/scans/{id}/template"),
        None,
    )
}

pub fn draft_submit(submission: Value, producer: &str) -> Result<Value, Failure> {
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        reqwest::Method::POST,
        &format!("/projects/{handle}/baselines"),
        Some(&json!({
            "submission": submission, "producer": producer,
        })),
    )
}
fn validate_scan_id(id: &str) -> Result<(), Failure> {
    let uuid = Uuid::parse_str(id).map_err(|_| Failure::new(ErrorCode::Usage, "扫描 ID 无效"))?;
    if uuid.get_version_num() != 7 || uuid.to_string() != id {
        return Err(Failure::new(ErrorCode::Usage, "扫描 ID 无效"));
    }
    Ok(())
}

pub fn scan_cancel(id: &str) -> Result<Value, Failure> {
    validate_scan_id(id)?;
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        reqwest::Method::POST,
        &format!("/projects/{handle}/scans/{id}/cancel"),
        None,
    )
}

fn require_current_api(endpoint: &Endpoint) -> Result<(), String> {
    if endpoint.api_version != 7 {
        return Err("旧版守护进程不支持当前草稿与验证，请先 towermd daemon stop 再重试".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_endpoint_remains_readable_but_cannot_silently_run_new_verification() {
        let legacy = json!({"address": "127.0.0.1", "port": 1234, "pid": 1, "instance": "legacy", "host": "host"});
        let mut endpoint: Endpoint = serde_json::from_value(legacy).unwrap();
        assert_eq!(endpoint.api_version, 0);
        assert!(require_current_api(&endpoint)
            .unwrap_err()
            .contains("daemon stop"));
        endpoint.api_version = 1;
        assert!(require_current_api(&endpoint).is_err());
        endpoint.api_version = 2;
        assert!(require_current_api(&endpoint).is_err());
        endpoint.api_version = 3;
        assert!(require_current_api(&endpoint).is_err());
        endpoint.api_version = 4;
        assert!(require_current_api(&endpoint).is_err());
        endpoint.api_version = 5;
        assert!(require_current_api(&endpoint).is_err());
        endpoint.api_version = 6;
        assert!(require_current_api(&endpoint).is_err());
        endpoint.api_version = 7;
        assert!(require_current_api(&endpoint).is_ok());
    }

    #[test]
    fn daemon_failures_prefer_the_explicit_code_then_the_http_status() {
        use reqwest::StatusCode;
        let explicit = daemon_failure(StatusCode::BAD_REQUEST, &json!({"error": "lease held", "code": "lease_busy", "hint": "wait"}));
        assert_eq!(explicit.code, ErrorCode::LeaseBusy);
        assert_eq!(explicit.message, "lease held");
        assert_eq!(explicit.hint.as_deref(), Some("wait"));
        for (status, code) in [
            (StatusCode::BAD_REQUEST, ErrorCode::Rejected),
            (StatusCode::NOT_FOUND, ErrorCode::NotFound),
            (StatusCode::CONFLICT, ErrorCode::Conflict),
            (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal),
        ] {
            assert_eq!(daemon_failure(status, &json!({"error": "x"})).code, code, "{status}");
        }
        // An unknown code from a newer daemon must not be trusted blindly.
        assert_eq!(daemon_failure(StatusCode::BAD_REQUEST, &json!({"error": "x", "code": "approved"})).code, ErrorCode::Rejected);
    }

    #[test]
    fn non_json_error_bodies_fall_back_to_the_http_status() {
        use reqwest::StatusCode;
        let empty = daemon_failure(StatusCode::NOT_FOUND, &error_body(""));
        assert_eq!(empty.code, ErrorCode::NotFound);
        assert_eq!(empty.message, "守护进程请求失败");
        let axum = daemon_failure(StatusCode::UNPROCESSABLE_ENTITY, &error_body("Failed to deserialize the JSON body\n"));
        assert_eq!(axum.code, ErrorCode::Rejected);
        assert_eq!(axum.message, "Failed to deserialize the JSON body");
        assert_eq!(daemon_failure(StatusCode::CONFLICT, &error_body("busy")).code, ErrorCode::Conflict);
        assert_eq!(daemon_failure(StatusCode::BAD_REQUEST, &error_body("[1]")).code, ErrorCode::Rejected);
        assert_eq!(daemon_failure(StatusCode::BAD_GATEWAY, &error_body("<html>")).code, ErrorCode::Internal);
    }
}

fn task_request(
    method: reqwest::Method,
    suffix: &str,
    body: Option<&Value>,
) -> Result<Value, Failure> {
    let (endpoint, handle) = project_connection()?;
    request(
        &endpoint,
        method,
        &format!("/projects/{handle}{suffix}"),
        body,
    )
}
pub fn task_status(run: Option<&str>) -> Result<Value, Failure> {
    task_request(
        reqwest::Method::POST,
        "/task-status",
        Some(&json!({"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"run":run})),
    )
}
pub fn task_add(task: Value, request_id: &str, run: Option<&str>) -> Result<Value, Failure> {
    task_request(reqwest::Method::POST, "/tasks", Some(&json!({"request_id":request_id,"task":task,"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"run":run})))
}
pub fn task_claim(
    id: &str,
    request_id: &str,
    source: Option<towermd_devkit::task_model::HarnessSource>,
) -> Result<Value, Failure> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Failure::new(ErrorCode::Usage, "任务 ID 无效"));
    }
    task_request(reqwest::Method::POST, &format!("/tasks/{id}/claim"), Some(&json!({"request_id":request_id,"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"self_reported":source})))
}
pub fn task_show(id: Option<&str>, run: Option<&str>) -> Result<Value, Failure> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut url = reqwest::Url::parse("http://localhost/tasks").map_err(|e| e.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("cwd", cwd.to_str().ok_or("工作树路径不是UTF-8")?);
        if let Some(id) = id {
            query.append_pair("id", id);
        }
        if let Some(run) = run {
            query.append_pair("run", run);
        }
    }
    task_request(
        reqwest::Method::GET,
        &format!("/tasks?{}", url.query().unwrap_or_default()),
        None,
    )
}
pub fn task_release(request_id: &str, run: Option<&str>) -> Result<Value, Failure> {
    task_request(reqwest::Method::POST, "/task-release", Some(&json!({"request_id":request_id,"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"run":run})))
}

pub fn task_check(run:Option<&str>)->Result<Value, Failure>{
    task_request(reqwest::Method::POST,"/task-check",Some(&json!({"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"run":run})))
}
pub fn finish_start(request_id:&str,run:Option<&str>)->Result<Value, Failure>{
    let (endpoint,handle)=project_connection()?;
    request(&endpoint,reqwest::Method::POST,&format!("/projects/{handle}/task-finish"),
        Some(&json!({"request_id":request_id,"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"run":run})))
}
pub fn finish_wait(id:&str)->Result<Value, Failure>{
    let deadline=Instant::now()+Duration::from_secs(85);
    validate_scan_id(id.strip_prefix("E-").ok_or_else(|| Failure::new(ErrorCode::Usage, "验证 ID 须为 E-<UUIDv7>"))?)?;
    let (endpoint,handle)=project_connection()?;
    loop{
        let remaining=deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero(){
            let again=format!("towermd dev finish --wait {id}");
            return Err(Failure::new(ErrorCode::Pending, format!("等待达到 90 秒上限，请再次运行 {again}"))
                .next(Next::new(again, lang::pick("wait again (at most 90 s per call)", "再次等待（每次最多 90 秒）"))));
        }
        let value=request_with_timeout(&endpoint,reqwest::Method::GET,&format!("/projects/{handle}/task-finish/{id}"),None,remaining.min(Duration::from_secs(30)))?;
        if !matches!(value["status"].as_str(),Some("queued"|"running")) || Instant::now()>=deadline{return Ok(value);}
        std::thread::sleep(Duration::from_millis(250));
    }
}
pub fn task_trailers(task:&str,request_id:&str,run:Option<&str>)->Result<Value, Failure>{
    task_request(reqwest::Method::POST,"/task-trailers",Some(&json!({
        "request_id":request_id,"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"run":run,"task":task,
    })))
}
/// The daemon's snapshot is idempotent by state; the request ID is only echoed (D7).
pub fn snapshot(request_id:&str)->Result<Value, Failure>{task_request(reqwest::Method::POST,"/snapshot",Some(&json!({"request_id":request_id})))}
pub fn rebuild_request(request_id:&str)->Result<Value, Failure>{task_request(reqwest::Method::POST,"/hub/rebuild",Some(&json!({"request_id":request_id})))}

pub fn ask(mut input:Value,run:Option<&str>)->Result<Value, Failure>{
    input["cwd"]=json!(std::env::current_dir().map_err(|e|e.to_string())?);
    input["run"]=json!(run);
    task_request(reqwest::Method::POST,"/ask",Some(&input))
}
pub fn relay_answer(mut input:Value,run:Option<&str>)->Result<Value, Failure>{
    input["cwd"]=json!(std::env::current_dir().map_err(|e|e.to_string())?);
    input["run"]=json!(run);
    task_request(reqwest::Method::POST,"/answers/relay",Some(&input))
}
pub fn wait_for_answer(hold:&str,run:Option<&str>)->Result<Value, Failure>{
    let (endpoint,handle)=project_connection()?;
    request_with_timeout(&endpoint,reqwest::Method::POST,&format!("/projects/{handle}/wait"),
        Some(&json!({"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"run":run,"hold":hold,"wait_ms":85_000})),Duration::from_secs(89))
}

pub fn fetch_messages(run: Option<&str>) -> Result<Value, Failure> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    task_request(reqwest::Method::POST, "/messages", Some(&json!({"cwd": cwd, "run": run})))
}

pub fn ack_message(message: &Value) -> Result<(), Failure> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    task_request(
        reqwest::Method::POST,
        "/messages/ack",
        Some(&json!({"request_id": Uuid::now_v7().to_string(), "cwd": cwd, "run": message["run"], "message": message["message"]})),
    )
    .map(drop)
}

pub fn propose(input:Value,request_id:&str,run:Option<&str>)->Result<Value, Failure>{
    task_request(reqwest::Method::POST,"/proposals",Some(&json!({"request_id":request_id,"cwd":std::env::current_dir().map_err(|e|e.to_string())?,"run":run,"input":input})))
}

pub fn pause_all(request_id:&str)->Result<Value, Failure>{
    task_request(reqwest::Method::POST,"/pause",Some(&json!({"request_id":request_id,"scope":"all"})))
}
pub fn request_resume_all(request_id:&str,pause_event:Option<&str>)->Result<Value, Failure>{
    let (endpoint,handle)=project_connection()?;
    let pause=match pause_event {
        Some(id)=>id.to_owned(),
        None=>request(&endpoint,reqwest::Method::GET,&format!("/projects/{handle}/state"),None)?["projection"]["ProjectControl"]["pause_event"]
            .as_str().ok_or("当前没有待解除暂停；精确重试旧请求时请沿用 --pause-event")?.to_owned(),
    };
    request(&endpoint,reqwest::Method::POST,&format!("/projects/{handle}/resume-request"),Some(&json!({"request_id":request_id,"pause_event":pause,"scope":"all"})))
}
