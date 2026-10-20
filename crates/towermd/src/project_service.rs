//! Local read and rebuildable S1–S4 scan service. This module never emits a
//! business event, approves a baseline, or writes a worktree/index/main ref.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Path as RoutePath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use towermd_core::catalog::Catalog;
use towermd_core::cold_start::{self, ScanReport};
use towermd_core::{project_view, repo};
use uuid::Uuid;

use crate::daemon::{authorized_client, Server};

type ApiError = (StatusCode, Json<Value>);
fn bad(error: impl ToString) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": error.to_string()})),
    )
}
fn denied() -> ApiError {
    (
        StatusCode::CONFLICT,
        Json(json!({"error": "守护进程实例或客户端类型不匹配"})),
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanJob {
    pub id: String,
    pub status: String,
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
pub async fn scan(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
) -> Result<Json<ScanJob>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    if project.project_id.is_none() {
        return Err(bad(
            "项目尚未初始化：先运行 towermd init 并将配置提交到权威 main",
        ));
    }
    let job = {
        let mut jobs = project.jobs.lock().map_err(|_| bad("扫描状态不可用"))?;
        if let Some(running) = jobs.values().find(|job| job.status == "running") {
            return Ok(Json(running.clone()));
        }
        let job = ScanJob {
            id: Uuid::now_v7().to_string(),
            status: "running".into(),
            report: None,
            error: None,
        };
        jobs.insert(job.id.clone(), job.clone());
        job
    };
    let id = job.id.clone();
    tokio::task::spawn_blocking(move || {
        let result = (|| {
            let catalog = current_catalog(&project)?;
            if !catalog.matches_current_source()? {
                return Err("扫描来源已改变，请重试".into());
            }
            let report = cold_start::scan(&catalog, &id)?;
            if !catalog.matches_current_source()? {
                return Err("扫描期间权威 main 或项目配置已改变，请重新扫描".into());
            }
            save_report(&project, &report)?;
            if !catalog.matches_current_source()? {
                let _ = fs::remove_dir_all(project.common.join("towermd/drafts").join(&id));
                return Err("扫描发布期间项目来源已改变，请重新扫描".into());
            }
            Ok::<_, String>(report)
        })();
        let finished = match result {
            Ok(report) => ScanJob {
                id: id.clone(),
                status: "ready".into(),
                report: Some(report),
                error: None,
            },
            Err(error) => ScanJob {
                id: id.clone(),
                status: "failed".into(),
                report: None,
                error: Some(error),
            },
        };
        if let Ok(mut jobs) = project.jobs.lock() {
            jobs.insert(id, finished);
        };
    });
    Ok(Json(job))
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
            return Ok(Json(job));
        }
        let report = load_report(&project, &id)?;
        Ok::<_, String>(Json(ScanJob {
            id,
            status: "ready".into(),
            report: Some(report),
            error: None,
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
