//! S6 endpoint adapter. Core owns validation, projection and private Git storage.
use super::*;
use towermd_devkit::{baseline, baseline_origin, dev_state, draft_store, proposal_model};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submit {
    producer: String,
    submission: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    action: towermd_devkit::review::ReviewAction,
}

pub async fn review(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
    Json(input): Json<Review>,
) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    draft_store::valid_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project).map_err(bad)?;
        let prepared = draft_store::prepare_review(&catalog, &id, input.action).map_err(bad)?;
        let uncertain = |error: String| coded(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            format!("审阅保存结果未确认，请用原草稿和相同操作重试：{error}"),
        );
        let draft = draft_store::persist_review(&catalog, prepared).map_err(uncertain)?;
        draft_store::push_if_remote(&catalog, &draft.id).map_err(uncertain)?;
        response(&project, &current_catalog(&project).map_err(uncertain)?, draft)
            .map(Json).map_err(uncertain)
    })
    .await
    .map_err(|error| coded(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal, error))?
}

fn response(
    project: &Project,
    catalog: &Catalog,
    draft: baseline::BaselineDraft,
) -> Result<Value, String> {
    let evidence_error = cold_start::validate_verification_logs(&draft.scan, &project.common).err();
    let review_evidence_error = draft
        .review
        .as_ref()
        .and_then(|review| review.verification.as_ref())
        .and_then(|_| {
            towermd_devkit::review_verification::validate_logs(&draft, &project.common).err()
        });
    let stale = catalog.revision != draft.scan.revision;
    let mut value = serde_json::to_value(draft).map_err(|e| e.to_string())?;
    value["evidence_error"] = json!(evidence_error);
    value["review_evidence_error"] = json!(review_evidence_error);
    value["stale"] = json!(stale);
    Ok(value)
}

pub async fn submit(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
    Json(input): Json<Submit>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        let submission = baseline::parse(input.submission)?;
        let id = draft_store::submission_id(&submission, &input.producer)?;
        let draft = if draft_store::private_commit(&project.root, &id)?.is_some() {
            // A lost reply can be retried even after scan caches have been removed.
            draft_store::load(&catalog, &id)?
        } else {
            let scan = load_report(&project, &submission.scan_id)?;
            if scan.revision != catalog.revision || !catalog.matches_current_source()? {
                return Err("扫描已过期，请重新扫描当前权威提交后综合".into());
            }
            draft_store::save(&catalog, &scan, submission, &input.producer)?
        };
        draft_store::push_if_remote(&catalog, &id)?;
        response(&project, &current_catalog(&project)?, draft).map(Json)
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

pub async fn list(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        let drafts = draft_store::list(&catalog)?;
        Ok::<_, String>(Json(json!(drafts
            .into_iter()
            .map(|draft| json!({
                "id": draft.id, "producer": draft.producer, "created_at": draft.created_at,
                "source_revision": draft.scan.revision, "title": draft.view.title,
                "stale": draft.scan.revision != catalog.revision,
                "reviewed": draft.review.is_some(),
            }))
            .collect::<Vec<_>>())))
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

pub async fn get(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    draft_store::valid_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        response(&project, &catalog, draft_store::load(&catalog, &id)?).map(Json)
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

pub async fn template(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        let scan = load_report(&project, &id)?;
        if scan.revision != catalog.revision {
            return Err("扫描已过期，请重新扫描".into());
        }
        serde_json::to_value(baseline::template(&scan)?)
            .map(Json)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

/// J4: turn a from-baseline scan into a draft. The daemon fills the template
/// itself, so the draft is signed by the process, not by any session; it is
/// still unreviewed and unapproved.
pub async fn draft_from_scan(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let scan = load_report(&project, &id)?;
        let origin = scan
            .origin
            .as_ref()
            .ok_or("该扫描不是从基线开始的，请按模板综合后提交草稿")?;
        let current = {
            let mut slot = project.runtime.lock().map_err(|_| "项目会话不可用")?;
            let runtime = business::ready(&mut slot, &server, &project).map_err(|e| e.to_string())?;
            tasks::baseline(&project, runtime)?.tag().to_string()
        };
        if origin.baseline != current {
            return Err(format!(
                "扫描从 {} 开始，当前已批准基线是 {current}，请重新从基线扫描",
                origin.baseline
            ));
        }
        let catalog = current_catalog(&project)?;
        if scan.revision != catalog.revision || !catalog.matches_current_source()? {
            return Err("扫描已过期，请重新从基线扫描当前权威提交".into());
        }
        let submission = baseline::template(&scan)?;
        let producer = format!("towermd/{}", env!("CARGO_PKG_VERSION"));
        let id = draft_store::submission_id(&submission, &producer)?;
        let draft = if draft_store::private_commit(&project.root, &id)?.is_some() {
            draft_store::load(&catalog, &id)?
        } else {
            draft_store::save(&catalog, &scan, submission, &producer)?
        };
        draft_store::push_if_remote(&catalog, &id)?;
        response(&project, &current_catalog(&project)?, draft).map(Json)
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

/// J4: what a draft from a baseline changes against that baseline, and the
/// paths and impact a proposal for it would carry. Read only.
pub async fn compare(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    draft_store::valid_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        let draft = draft_store::load(&catalog, &id)?;
        let comparison = baseline_origin::compare(&draft)?;
        let mut slot = project.runtime.lock().map_err(|_| "项目会话不可用")?;
        let runtime = business::ready(&mut slot, &server, &project).map_err(|e| e.to_string())?;
        let current = tasks::baseline(&project, runtime)?;
        let projection = dev_state::tasks(runtime.projection())?;
        let (changed_paths, impact) = proposal_model::preview(&catalog, &draft, &projection, &current)?;
        Ok::<_, String>(Json(json!({
            "current_baseline": current.tag(),
            "outdated": comparison.baseline != current.tag(),
            "stale": draft.scan.revision != catalog.revision,
            "changed_paths": changed_paths,
            "impact": impact,
            "comparison": comparison,
        })))
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewJob {
    pub id: String,
    pub parent_id: String,
    pub input_sha256: String,
    pub status: String,
    pub phase: String,
    pub cancel_requested: bool,
    pub result_id: Option<String>,
    pub error: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyRequest {
    request_id: String,
}

fn valid_request_id(id: &str) -> Result<(), String> {
    let uuid = Uuid::parse_str(id).map_err(|_| "验证请求 ID 无效")?;
    if !matches!(uuid.get_version_num(), 4 | 7) || uuid.to_string() != id {
        return Err("验证请求 ID 无效".into());
    }
    Ok(())
}
fn job_path(project: &Project, id: &str) -> PathBuf {
    project
        .common
        .join("towermd/drafts")
        .join(format!("verify-{id}.json"))
}
fn save_job(project: &Project, job: &ReviewJob) -> Result<(), String> {
    let directory = draft_directory(project)?;
    let temporary = directory.join(format!(".verify-{}.tmp", Uuid::now_v7()));
    new_file(
        &temporary,
        &serde_json::to_vec(job).map_err(|e| e.to_string())?,
    )?;
    fs::rename(&temporary, job_path(project, &job.id)).map_err(|e| e.to_string())?;
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())
}
fn publish_verification(project: &Project, job: &ReviewJob) {
    if let Ok(mut jobs) = project.review_jobs.lock() {
        jobs.insert(job.id.clone(), job.clone());
    }
}
fn result_job(draft: &baseline::BaselineDraft) -> Option<ReviewJob> {
    let verification = draft.review.as_ref()?.verification.as_ref()?;
    let report = &verification.report;
    let cancelled = report
        .commands
        .iter()
        .any(|command| command.status == VerificationStatus::Cancelled);
    Some(ReviewJob {
        id: verification.run_id.clone(),
        parent_id: verification.parent_id.clone(),
        input_sha256: verification.input_sha256.clone(),
        status: if !report.cleanup_complete {
            "failed"
        } else if cancelled {
            "cancelled"
        } else {
            "ready"
        }
        .into(),
        phase: "complete".into(),
        cancel_requested: cancelled,
        result_id: Some(draft.id.clone()),
        error: (!report.cleanup_complete).then(|| "验证清理未完成，不能认证通过".into()),
    })
}
fn recovered_job(project: &Project, id: &str) -> Result<Option<ReviewJob>, String> {
    let catalog = current_catalog(project)?;
    let path = job_path(project, id);
    let saved = if path.exists() {
        let job: ReviewJob =
            serde_json::from_slice(&regular_bytes(&path)?).map_err(|e| e.to_string())?;
        if job.id != id {
            return Err("验证记录 ID 不匹配".into());
        }
        Some(job)
    } else {
        None
    };
    if let Some(result) = saved.as_ref().and_then(|job| job.result_id.as_ref()) {
        let draft = draft_store::load(&catalog, result)?;
        let rebuilt = result_job(&draft).ok_or("验证原件缺少真实报告")?;
        if rebuilt.id != id
            || saved.as_ref().is_some_and(|job| {
                job.parent_id != rebuilt.parent_id || job.input_sha256 != rebuilt.input_sha256
            })
        {
            return Err("验证记录与耐久原件不匹配".into());
        }
        // A failed publication must remain retryable, even after restart.
        if saved.as_ref().is_some_and(|job| job.phase == "publishing") {
            return Ok(saved);
        }
        return Ok(Some(rebuilt));
    }
    // The result may have become durable immediately before the process exited,
    // or this rebuildable marker may have been removed with the scan cache.
    for draft in draft_store::list(&catalog)? {
        if let Some(mut job) = result_job(&draft).filter(|job| job.id == id) {
            let parent = draft_store::load(&catalog, &job.parent_id)?;
            if serde_json::to_value(&draft.review.as_ref().unwrap().actions)
                .map_err(|e| e.to_string())?
                != serde_json::to_value(
                    &parent.review.as_ref().ok_or("验证原草稿缺少审阅")?.actions,
                )
                .map_err(|e| e.to_string())?
            {
                continue;
            }
            job.status = "failed".into();
            job.phase = "publishing".into();
            job.error = Some("验证结果已保存；请用原请求重试确认私有引用发布".into());
            return Ok(Some(job));
        }
    }
    let Some(mut job) = saved else {
        return Ok(None);
    };
    if job.status == "running" {
        job.status = "failed".into();
        job.phase = "interrupted".into();
        job.error =
            Some("守护进程在完成前退出；结果未知，未自动重跑。原 worker 将取消并清理。".into());
    }
    Ok(Some(job))
}

pub async fn verify(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, parent_id)): RoutePath<(String, String)>,
    Json(input): Json<VerifyRequest>,
) -> Result<Json<ReviewJob>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_request_id(&input.request_id).map_err(bad)?;
    draft_store::valid_id(&parent_id).map_err(bad)?;
    if server.shutting_down.load(Ordering::SeqCst) {
        return Err(bad("守护进程正在关闭"));
    }
    let project = project(&server, &handle).map_err(bad)?;
    let request_id = input.request_id;
    let owner = Arc::clone(&project);
    let (job, prepared, control) = tokio::task::spawn_blocking(move || {
        let mut jobs = owner
            .review_jobs
            .lock()
            .map_err(|_| bad("审阅验证状态不可用"))?;
        let existing = match jobs.get(&request_id).cloned() {
            Some(job) => Some(job),
            None => recovered_job(&owner, &request_id).map_err(bad)?,
        };
        if let Some(mut job) = existing {
            if job.parent_id != parent_id {
                return Err(bad("同一验证请求不能更换草稿"));
            }
            if job.phase == "publishing" {
                let catalog = current_catalog(&owner).map_err(bad)?;
                let result_id = job
                    .result_id
                    .as_ref()
                    .ok_or_else(|| bad("发布记录缺少草稿"))?;
                draft_store::push_if_remote(&catalog, result_id).map_err(|error| {
                    coded(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal, error)
                })?;
                job = result_job(&draft_store::load(&catalog, result_id).map_err(bad)?)
                    .ok_or_else(|| bad("报告缺失"))?;
                save_job(&owner, &job).map_err(bad)?;
                jobs.insert(request_id, job.clone());
            }
            return Ok((job, None, None));
        }
        if jobs.values().any(|job| job.status == "running") {
            return Err(bad("本项目已有审阅验证正在执行，请等待或取消"));
        }
        let catalog = current_catalog(&owner).map_err(bad)?;
        let prepared =
            draft_store::prepare_verification(&catalog, &parent_id, &request_id, Limits::default())
                .map_err(bad)?;
        let job = ReviewJob {
            id: request_id,
            parent_id,
            input_sha256: prepared.input_sha256.clone(),
            status: "running".into(),
            phase: "queued".into(),
            cancel_requested: false,
            result_id: None,
            error: None,
        };
        save_job(&owner, &job).map_err(bad)?;
        let control = Arc::new(AtomicBool::new(false));
        owner
            .controls
            .lock()
            .map_err(|_| bad("验证控制不可用"))?
            .insert(job.id.clone(), Arc::clone(&control));
        jobs.insert(job.id.clone(), job.clone());
        Ok((job, Some(prepared), Some(control)))
    })
    .await
    .map_err(bad)??;
    if let (Some(prepared), Some(control)) = (prepared, control) {
        let mut progress = job.clone();
        tokio::task::spawn_blocking(move || {
            let result = (|| {
                let lane = loop {
                    if control.load(Ordering::SeqCst) || server.shutting_down.load(Ordering::SeqCst)
                    {
                        control.store(true, Ordering::SeqCst);
                        return Err("验证开始前已取消".into());
                    }
                    match server.verification_lane.try_lock() {
                        Ok(lane) => break lane,
                        Err(std::sync::TryLockError::WouldBlock) => {
                            std::thread::sleep(Duration::from_millis(50))
                        }
                        Err(_) => return Err("验证调度不可用".into()),
                    }
                };
                progress.phase = "verifying".into();
                publish_verification(&project, &progress);
                let report = crate::worker::run(&prepared.plan, &control, &server.runtime)?;
                drop(lane);
                let catalog = current_catalog(&project)?;
                let draft = draft_store::persist_verification(&catalog, prepared, report)?;
                progress.result_id = Some(draft.id.clone());
                progress.phase = "publishing".into();
                save_job(&project, &progress)?;
                publish_verification(&project, &progress);
                draft_store::push_if_remote(&catalog, &draft.id)?;
                result_job(&draft).ok_or_else(|| "验证草稿缺少报告".into())
            })();
            progress = match result {
                Ok(job) => job,
                Err(error) => {
                    progress.status =
                        if control.load(Ordering::SeqCst) && progress.result_id.is_none() {
                            "cancelled"
                        } else {
                            "failed"
                        }
                        .into();
                    if progress.phase != "publishing" {
                        progress.phase = "complete".into();
                    }
                    progress.cancel_requested = control.load(Ordering::SeqCst);
                    progress.error = Some(error);
                    progress
                }
            };
            if let Err(error) = save_job(&project, &progress) {
                progress.status = "failed".into();
                progress.error = Some(format!("运行结束记录未确认，请用原请求重试：{error}"));
            }
            publish_verification(&project, &progress);
            if let Ok(mut controls) = project.controls.lock() {
                controls.remove(&progress.id);
            };
        });
    }
    Ok(Json(job))
}

pub async fn poll_verification(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<ReviewJob>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_request_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let job = project
            .review_jobs
            .lock()
            .map_err(|_| bad("验证状态不可用"))?
            .get(&id)
            .cloned();
        match job {
            Some(job) => Ok(Json(job)),
            None => recovered_job(&project, &id)
                .and_then(|job| job.ok_or_else(|| "验证请求不存在".into()))
                .map(Json)
                .map_err(bad),
        }
    })
    .await
    .map_err(bad)?
}
pub async fn cancel_verification(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<ReviewJob>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_request_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    let mut jobs = project
        .review_jobs
        .lock()
        .map_err(|_| bad("验证状态不可用"))?;
    if let Some(job) = jobs.get_mut(&id) {
        if job.status == "running" {
            if let Some(control) = project
                .controls
                .lock()
                .map_err(|_| bad("验证控制不可用"))?
                .get(&id)
            {
                control.store(true, Ordering::SeqCst);
            }
            job.cancel_requested = true;
            job.phase = "cancelling".into();
        }
        return Ok(Json(job.clone()));
    }
    drop(jobs);
    recovered_job(&project, &id)
        .and_then(|job| job.ok_or_else(|| "验证请求不存在".into()))
        .map(Json)
        .map_err(bad)
}
