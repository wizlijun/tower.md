//! Project registry and rebuildable scan service. Business adapters below
//! route accepted events and protected publications through ProjectRuntime.
#[path = "dev/baseline_service.rs"]
pub(crate) mod baselines;
#[path = "cli/business_service.rs"]
pub(crate) mod business;
#[path = "cli/task_service.rs"]
pub(crate) mod tasks;
#[path = "dev/finish_service.rs"]
pub(crate) mod finish;
#[path = "cli/question_service.rs"]
pub(crate) mod questions;
#[path = "cli/proposal_service.rs"]
pub(crate) mod proposals;
#[path = "cli/main_resolution_service.rs"]
pub(crate) mod main_resolutions;
#[path = "cli/run_details_service.rs"]
pub(crate) mod run_details;
#[path = "cli/project_control_service.rs"]
pub(crate) mod project_controls;
#[path = "cli/human_task_service.rs"]
pub(crate) mod human_tasks;
#[path = "cli/event_service.rs"]
pub(crate) mod events;
#[path = "cli/delegation_service.rs"]
pub(crate) mod delegations;
#[path = "cli/tower_service.rs"]
pub(crate) mod tower;
#[path = "cli/gate_service.rs"]
pub(crate) mod gates;
#[path = "cli/weakening_service.rs"]
pub(crate) mod weakenings;
#[path = "cli/idea_service.rs"]
pub(crate) mod ideas;
#[path = "cli/frame_service.rs"]
pub(crate) mod frames;
#[path = "cli/request_service.rs"]
pub(crate) mod requests;
#[path = "cli/notemd_service.rs"]
pub(crate) mod notemd;
#[path = "cli/follow_up_service.rs"]
pub(crate) mod follow_ups;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use towermd_devkit::verifier::{Limits, VerificationPlan, VerificationStatus};

use axum::extract::{Path as RoutePath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use towermd_core::catalog::Catalog;
use towermd_devkit::cold_start::{self, ScanReport};
use towermd_core::{project_view, repo};
use uuid::Uuid;

use crate::daemon::{authorized_client, Server};

use crate::output::ErrorCode;

type ApiError = (StatusCode, Json<Value>);
/// Error body: `error` stays for the App (D2), `code` is the CLI C0 code.
fn coded(status: StatusCode, code: ErrorCode, error: impl ToString) -> ApiError {
    (status, Json(json!({"error": error.to_string(), "code": code.as_str()})))
}
/// A refusal the daemon has not classified further (D3).
fn bad(error: impl ToString) -> ApiError {
    coded(StatusCode::BAD_REQUEST, ErrorCode::Rejected, error)
}
fn denied() -> ApiError {
    coded(StatusCode::CONFLICT, ErrorCode::Conflict, "守护进程实例或客户端类型不匹配")
}
/// `map_err(rejected_as(ErrorCode::GateFailed))` where the daemon knows why.
/// The HTTP status stays 400 so the App's wording is unchanged.
fn rejected_as<E: ToString>(code: ErrorCode) -> impl FnOnce(E) -> ApiError {
    move |error| coded(StatusCode::BAD_REQUEST, code, error)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanJob {
    pub id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<ScanReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
struct Project {
    root: PathBuf,
    common: PathBuf,
    project_id: Option<String>,
    _project_lock: Option<File>,
    jobs: Mutex<BTreeMap<String, ScanJob>>,
    review_jobs: Mutex<BTreeMap<String, baselines::ReviewJob>>,
    finish_jobs: Mutex<BTreeMap<String, finish::FinishJob>>,
    finish_observed_main: Mutex<Option<String>>,
    controls: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
    runtime: Mutex<Option<towermd_core::project_runtime::ProjectRuntime>>,
}
#[derive(Default)]
pub struct Projects {
    handles: BTreeMap<String, Arc<Project>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenProject {
    path: PathBuf,
}
#[derive(Deserialize)]
pub struct DraftQuery {
    project: String,
}

fn directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(()),
        Ok(_) => Err(format!("派生目录不是普通目录：{}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| format!("{}: {error}", path.display()))
        }
        Err(error) => Err(error.to_string()),
    }
}
fn regular_bytes(path: &Path) -> Result<Vec<u8>, String> {
    if !fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_file()
    {
        return Err(format!("派生文件不是普通文件：{}", path.display()));
    }
    fs::read(path).map_err(|error| error.to_string())
}
fn new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}
fn valid_scan_id(id: &str) -> Result<(), String> {
    let uuid = Uuid::parse_str(id).map_err(|_| "扫描 ID 无效")?;
    if uuid.get_version_num() != 7 || uuid.to_string() != id {
        return Err("扫描 ID 无效".into());
    }
    Ok(())
}
fn draft_directory(project: &Project) -> Result<PathBuf, String> {
    let state = project.common.join("towermd");
    directory(&state)?;
    let drafts = state.join("drafts");
    directory(&drafts)?;
    Ok(drafts)
}
fn module_files(report: &ScanReport) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    for node in &report.view.nodes {
        if node.kind != "file" {
            continue;
        }
        let file = node.file.as_deref().ok_or("草稿文件节点缺少路径")?;
        let name = file.strip_prefix("arch/").ok_or("草稿文件必须位于 arch/")?;
        if name.is_empty()
            || name.contains(['/', '\\'])
            || !name.ends_with(".md")
            || name.starts_with('.')
        {
            return Err("草稿文件路径无效".into());
        }
        if files
            .insert(file.to_owned(), node.content.clone())
            .is_some()
        {
            return Err("草稿文件重复".into());
        }
    }
    Ok(files)
}
fn validate_report(project: &Project, id: &str, report: &ScanReport) -> Result<(), String> {
    if report.id != id
        || project.project_id.as_deref() != Some(report.project_id.as_str())
        || report.view.revision != report.revision
    {
        return Err("草稿与项目或源提交不匹配，请重新扫描".into());
    }
    if !matches!(report.revision.len(), 40 | 64)
        || !report.revision.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("草稿源提交无效".into());
    }
    let tree = repo::git(
        &project.root,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{tree}}", report.revision),
        ],
    )?;
    if tree != report.tree {
        return Err("草稿源 tree 不匹配".into());
    }
    module_files(report)?;
    cold_start::validate_verification_logs(report, &project.common)?;
    Ok(())
}
fn save_report(project: &Project, report: &ScanReport) -> Result<(), String> {
    valid_scan_id(&report.id)?;
    validate_report(project, &report.id, report)?;
    let drafts = draft_directory(project)?;
    let temporary = drafts.join(format!(".{}-{}.tmp", report.id, Uuid::now_v7()));
    fs::create_dir(&temporary).map_err(|e| e.to_string())?;
    let result = (|| {
        fs::create_dir(temporary.join("arch")).map_err(|e| e.to_string())?;
        for (file, content) in module_files(report)? {
            new_file(&temporary.join(file), content.as_bytes())?;
        }
        new_file(
            &temporary.join("tower.canvas"),
            cold_start::canvas_json(report)?.as_bytes(),
        )?;
        new_file(
            &temporary.join("report.json"),
            &serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?,
        )?;
        new_file(
            &temporary.join("unassigned.json"),
            &serde_json::to_vec_pretty(&report.unassigned).map_err(|e| e.to_string())?,
        )?;
        new_file(
            &temporary.join("source.json"),
            &serde_json::to_vec(
                &json!({"common_dir": project.common, "project_id": project.project_id}),
            )
            .map_err(|e| e.to_string())?,
        )?;
        File::open(temporary.join("arch"))
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        File::open(&temporary)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        let target = drafts.join(&report.id);
        if target.try_exists().map_err(|e| e.to_string())? {
            return Err("扫描结果目录已存在".into());
        }
        fs::rename(&temporary, target).map_err(|e| e.to_string())?;
        File::open(&drafts)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(temporary);
    }
    result
}
fn load_report(project: &Project, id: &str) -> Result<ScanReport, String> {
    valid_scan_id(id)?;
    let drafts = project.common.join("towermd/drafts");
    // A read never creates a missing cache, and never follows cache symlinks.
    for path in [
        project.common.join("towermd"),
        drafts.clone(),
        drafts.join(id),
        drafts.join(id).join("arch"),
    ] {
        if !fs::symlink_metadata(path)
            .map_err(|_| "扫描结果不存在，请重新扫描")?
            .file_type()
            .is_dir()
        {
            return Err("草稿目录无效".into());
        }
    }
    let path = drafts.join(id);
    let source: Value = serde_json::from_slice(&regular_bytes(&path.join("source.json"))?)
        .map_err(|e| e.to_string())?;
    if source != json!({"common_dir": project.common, "project_id": project.project_id}) {
        return Err("草稿缓存属于其他项目".into());
    }
    let report: ScanReport = serde_json::from_slice(&regular_bytes(&path.join("report.json"))?)
        .map_err(|e| e.to_string())?;
    validate_report(project, id, &report)?;
    if regular_bytes(&path.join("tower.canvas"))? != cold_start::canvas_json(&report)?.as_bytes() {
        return Err("草稿画布与扫描报告不一致，请重新扫描".into());
    }
    for (file, content) in module_files(&report)? {
        if regular_bytes(&path.join(file))? != content.as_bytes() {
            return Err("草稿对象与扫描报告不一致，请重新扫描".into());
        }
    }
    Ok(report)
}
fn project(server: &Server, handle: &str) -> Result<Arc<Project>, String> {
    server
        .projects
        .lock()
        .map_err(|_| "项目注册不可用")?
        .handles
        .get(handle)
        .cloned()
        .ok_or_else(|| "项目会话无效，请重新打开项目".into())
}
fn current_catalog(project: &Project) -> Result<Catalog, String> {
    let catalog = Catalog::load(&project.root)?;
    if catalog.common_dir != project.common
        || Some(catalog.config.project_id()) != project.project_id.as_deref()
    {
        return Err("项目配置已改变，请重新打开项目".into());
    }
    Ok(catalog)
}

pub async fn open(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(input): Json<OpenProject>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    tokio::task::spawn_blocking(move || {
        let root = repo::toplevel(&input.path)?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let common = root
            .join(repo::git(&root, &["rev-parse", "--git-common-dir"])?)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let catalog = Catalog::load_optional(&root)?;
        let project_id = catalog.as_ref().map(|c| c.config.project_id().to_string());
        let mut registry = server.projects.lock().map_err(|_| "项目注册不可用")?;
        for (handle, existing) in &registry.handles {
            if existing.common == common && existing.project_id == project_id {
                return Ok(Json(json!({"handle": handle})));
            }
            if project_id.is_some()
                && existing.project_id == project_id
                && existing.common != common
            {
                return Err(
                    "同一 project_id 的另一 clone 已在本机打开；请使用工作树或云端 hub".into(),
                );
            }
        }
        let project_lock = if let Some(id) = &project_id {
            let locks = server.runtime.join("projects");
            directory(&locks)?;
            let path = locks.join(format!("{:x}.lock", Sha256::digest(id.as_bytes())));
            if fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file()) {
                return Err("项目锁文件无效".into());
            }
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            FileExt::try_lock_exclusive(&file).map_err(|_| "同一项目已由其他本地服务占用")?;
            Some(file)
        } else {
            None
        };
        let handle = Uuid::now_v7().to_string();
        registry.handles.insert(
            handle.clone(),
            Arc::new(Project {
                root,
                common,
                project_id,
                _project_lock: project_lock,
                jobs: Mutex::new(BTreeMap::new()),
                review_jobs: Mutex::new(BTreeMap::new()),
                finish_jobs: Mutex::new(BTreeMap::new()),
                finish_observed_main: Mutex::new(None),
                controls: Mutex::new(BTreeMap::new()),
                runtime: Mutex::new(None),
            }),
        );
        Ok::<_, String>(Json(json!({"handle": handle})))
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}
pub async fn view(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
) -> Result<Json<project_view::ProjectView>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || project_view::load(&project.root).map(Json))
        .await
        .map_err(bad)?
        .map_err(bad)
}
fn marker_path(project: &Project, id: &str) -> PathBuf {
    project
        .common
        .join("towermd/drafts")
        .join(format!(".running-{id}.json"))
}
fn write_marker(project: &Project, job: &ScanJob) -> Result<(), String> {
    draft_directory(project)?;
    new_file(
        &marker_path(project, &job.id),
        &serde_json::to_vec(&json!({
            "common_dir": project.common, "project_id": project.project_id, "job": job,
        }))
        .map_err(|e| e.to_string())?,
    )?;
    File::open(project.common.join("towermd/drafts"))
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn finish_marker(project: &Project, job: &ScanJob) -> Result<(), String> {
    draft_directory(project)?;
    let path = marker_path(project, &job.id);
    let temporary = path.with_extension(format!("{}.tmp", Uuid::now_v7()));
    let result = (|| {
        new_file(&temporary, &serde_json::to_vec(&json!({"common_dir": project.common, "project_id": project.project_id, "job": job})).map_err(|e| e.to_string())?)?;
        fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        File::open(project.common.join("towermd/drafts"))
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn read_interrupted(project: &Project, id: &str) -> Result<ScanJob, String> {
    for path in [
        project.common.join("towermd"),
        project.common.join("towermd/drafts"),
    ] {
        if !fs::symlink_metadata(path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_dir()
        {
            return Err("草稿目录无效".into());
        }
    }
    let value: Value = serde_json::from_slice(&regular_bytes(&marker_path(project, id))?)
        .map_err(|e| e.to_string())?;
    if value["common_dir"] != json!(project.common)
        || value["project_id"] != json!(project.project_id)
    {
        return Err("运行记录属于另一项目".into());
    }
    let mut job: ScanJob =
        serde_json::from_value(value["job"].clone()).map_err(|e| e.to_string())?;
    if job.id != id {
        return Err("运行记录 ID 不匹配".into());
    }
    if let Some(report) = &job.report {
        validate_report(project, id, report)?;
        if job.status == "running" && report.verification.is_some() {
            return Err("运行记录不能提供已完成验证报告".into());
        }
    }
    if matches!(job.status.as_str(), "failed" | "cancelled") {
        return Ok(job);
    }
    if job.status != "running" {
        return Err("运行记录状态无效".into());
    }
    job.status = "failed".into();
    job.phase = Some("interrupted".into());
    job.error = Some(
        "守护进程在验证完成前退出。候选保留，验证结果未知；原 worker 将取消并清理，不会自动重跑。"
            .into(),
    );
    Ok(job)
}
fn publish_job(project: &Project, job: ScanJob) {
    if let Ok(mut jobs) = project.jobs.lock() {
        jobs.insert(job.id.clone(), job);
    }
}
fn terminal_status(report: &ScanReport) -> (&'static str, Option<String>) {
    if let Some(verification) = &report.verification {
        if !verification.cleanup_complete {
            return (
                "failed",
                Some("验证清理未完成，请检查报告；不能认证通过".into()),
            );
        }
        if verification
            .commands
            .iter()
            .any(|command| command.status == VerificationStatus::Cancelled)
            || verification
                .modules
                .iter()
                .any(|module| module.status == VerificationStatus::Cancelled)
        {
            return ("cancelled", None);
        }
    }
    ("ready", None)
}
pub fn cancel_all(server: &Server) {
    server.shutting_down.store(true, Ordering::SeqCst);
    if let Ok(projects) = server.projects.lock() {
        for project in projects.handles.values() {
            if let Ok(controls) = project.controls.lock() {
                for control in controls.values() {
                    control.store(true, Ordering::SeqCst);
                }
            }
        }
    }
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ScanStart {
    #[serde(default)]
    request_id: Option<String>,
}
pub async fn scan(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
    // `null` is accepted too: older clients posted it as an empty body.
    body: Option<Json<Option<ScanStart>>>,
) -> Result<Json<ScanJob>, ApiError> {
    let request_id = body.and_then(|Json(body)| body).and_then(|body| body.request_id);
    if let Some(id) = &request_id {
        valid_scan_id(id).map_err(rejected_as(ErrorCode::Usage))?;
        // A retry of a scan that already finished is answered from disk.
        if let Ok(found) = read_scan(Arc::clone(&server), headers.clone(), handle.clone(), id.clone()).await {
            return Ok(found);
        }
    }
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    if server.shutting_down.load(Ordering::SeqCst) {
        return Err(coded(StatusCode::SERVICE_UNAVAILABLE, ErrorCode::DaemonUnavailable, "守护进程正在关闭，不能启动验证"));
    }
    let project = project(&server, &handle).map_err(bad)?;
    if project.project_id.is_none() {
        return Err(coded(StatusCode::BAD_REQUEST, ErrorCode::NotInitialized,
            "项目尚未初始化：先运行 towermd init 并将配置提交到权威 main"));
    }
    let control = Arc::new(AtomicBool::new(false));
    let job = {
        let mut jobs = project.jobs.lock().map_err(|_| bad("扫描状态不可用"))?;
        if let Some(job) = request_id.as_ref().and_then(|id| jobs.get(id)) {
            return Ok(Json(job.clone()));
        }
        if let Some(running) = jobs.values().find(|job| job.status == "running") {
            return Ok(Json(running.clone()));
        }
        let job = ScanJob {
            id: request_id.clone().unwrap_or_else(|| Uuid::now_v7().to_string()),
            status: "running".into(),
            phase: Some("inventory".into()),
            cancel_requested: false,
            report: None,
            error: None,
        };
        project
            .controls
            .lock()
            .map_err(|_| bad("扫描控制不可用"))?
            .insert(job.id.clone(), Arc::clone(&control));
        jobs.insert(job.id.clone(), job.clone());
        job
    };
    let mut progress = job.clone();
    tokio::task::spawn_blocking(move || {
        let mut worker_started = false;
        let result = (|| {
            if server.shutting_down.load(Ordering::SeqCst) {
                control.store(true, Ordering::SeqCst);
            }
            let catalog = current_catalog(&project)?;
            if !catalog.matches_current_source()? {
                return Err("扫描来源已改变，请重试".into());
            }
            let mut report = cold_start::scan(&catalog, &progress.id)?;
            if !catalog.matches_current_source()? {
                return Err("扫描期间权威 main 或项目配置已改变，请重新扫描".into());
            }
            progress.report = Some(report.clone());
            progress.phase = Some("queued".into());
            write_marker(&project, &progress)?;
            publish_job(&project, progress.clone());
            let lane = loop {
                if control.load(Ordering::SeqCst) || server.shutting_down.load(Ordering::SeqCst) {
                    control.store(true, Ordering::SeqCst);
                    return Err("验证开始前已取消，未执行候选命令".into());
                }
                match server.verification_lane.try_lock() {
                    Ok(lane) => break lane,
                    Err(std::sync::TryLockError::WouldBlock) => {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(_) => return Err("验证调度不可用".into()),
                }
            };
            progress.phase = Some("verifying".into());
            publish_job(&project, progress.clone());
            let plan = VerificationPlan {
                root: project.root.clone(),
                id: progress.id.clone(),
                revision: report.revision.clone(),
                tree: report.tree.clone(),
                commands: report.commands.clone(),
                modules: report.modules.clone(),
                limits: Limits::default(),
            };
            if control.load(Ordering::SeqCst) || server.shutting_down.load(Ordering::SeqCst) {
                control.store(true, Ordering::SeqCst);
                return Err("验证启动前已取消，未执行候选命令".into());
            }
            worker_started = true;
            let verified = crate::worker::run(&plan, &control, &server.runtime)?;
            cold_start::apply_verification(&mut report, verified)?;
            progress.report = Some(report.clone());
            drop(lane);
            if !catalog.matches_current_source()? {
                return Err("验证期间权威来源已改变，保留结果供查看；请重新扫描当前提交".into());
            }
            save_report(&project, &report)?;
            if !catalog.matches_current_source()? {
                let _ =
                    fs::remove_dir_all(project.common.join("towermd/drafts").join(&progress.id));
                return Err("扫描发布期间项目来源已改变，请重新扫描".into());
            }
            Ok::<_, String>(report)
        })();
        progress.cancel_requested = control.load(Ordering::SeqCst);
        match result {
            Ok(report) => {
                let (status, error) = terminal_status(&report);
                progress.status = status.into();
                progress.phase = Some("complete".into());
                progress.error = error;
                progress.report = Some(report);
                let _ = fs::remove_file(marker_path(&project, &progress.id));
            }
            Err(error) => {
                progress.status = if progress.cancel_requested && !worker_started {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                progress.phase = Some("complete".into());
                progress.error = Some(error);
                if let Err(marker_error) = finish_marker(&project, &progress) {
                    progress.error = Some(format!(
                        "{}；无法保存运行结束状态：{marker_error}",
                        progress.error.as_deref().unwrap_or("扫描失败")
                    ));
                }
            }
        }
        publish_job(&project, progress.clone());
        if let Ok(mut controls) = project.controls.lock() {
            controls.remove(&progress.id);
        };
    });
    Ok(Json(job))
}
pub async fn cancel(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<ScanJob>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    let mut jobs = project.jobs.lock().map_err(|_| bad("扫描状态不可用"))?;
    let job = jobs
        .get_mut(&id)
        .ok_or_else(|| bad("扫描未在当前守护进程运行，请先查询结果"))?;
    if job.status == "running" {
        let controls = project.controls.lock().map_err(|_| bad("扫描控制不可用"))?;
        if let Some(control) = controls.get(&id) {
            control.store(true, Ordering::SeqCst);
        }
        job.cancel_requested = true;
        job.phase = Some("cancelling".into());
    }
    // Running remains running until the worker has exited and cleanup reports.
    Ok(Json(job.clone()))
}
async fn read_scan(
    server: Arc<Server>,
    headers: HeaderMap,
    handle: String,
    id: String,
) -> Result<Json<ScanJob>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        current_catalog(&project)?;
        if let Some(job) = project
            .jobs
            .lock()
            .map_err(|_| "扫描状态不可用")?
            .get(&id)
            .cloned()
        {
            if let Some(report) = &job.report {
                cold_start::validate_verification_logs(report, &project.common)?;
            }
            return Ok(Json(job));
        }
        let report = match load_report(&project, &id) {
            Ok(report) => report,
            Err(_error)
                if !project.common.join("towermd/drafts").join(&id).exists()
                    && marker_path(&project, &id).exists() =>
            {
                return read_interrupted(&project, &id).map(Json)
            }
            Err(error) => return Err(error),
        };
        let (status, error) = terminal_status(&report);
        Ok::<_, String>(Json(ScanJob {
            id,
            status: status.into(),
            phase: Some("complete".into()),
            cancel_requested: status == "cancelled",
            report: Some(report),
            error,
        }))
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}
pub async fn get_scan(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<ScanJob>, ApiError> {
    read_scan(server, headers, handle, id).await
}
pub async fn draft(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<DraftQuery>,
) -> Result<Json<ScanJob>, ApiError> {
    read_scan(server, headers, query.project, id).await
}
