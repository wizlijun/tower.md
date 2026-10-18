//! Background scans. Runtime files are derived indexes, never verifier capabilities.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{oneshot, Notify},
    task::JoinHandle,
};
use towermd_core::{
    coldstart::ScanInput,
    coldstart_draft::render_draft,
    coldstart_evidence::{FrozenColdstart, PreparedColdstart, RecordedColdstart},
    verifier::{self, VerificationRequest},
};

pub(crate) type Record = Arc<dyn Fn(&FrozenColdstart) -> Result<(), String> + Send + Sync>;
pub(crate) type Resume =
    Arc<dyn Fn(&str) -> Result<Option<RecordedColdstart>, String> + Send + Sync>;
pub(crate) const WAIT_BUDGET: Duration = Duration::from_secs(90);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_INDEX_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct Index {
    schema: u32,
    scan_id: String,
    input: ScanInput,
    state: String,
    stages: Vec<String>,
    evidence_ids: Vec<String>,
    diagnostic: Option<String>,
}
struct Job {
    index: Mutex<Index>,
    published: Mutex<Option<Value>>,
    notify: Notify,
    cancelled: Arc<AtomicBool>,
    active: AtomicBool,
}
impl Job {
    fn new(index: Index) -> Self {
        Self {
            index: Mutex::new(index),
            published: Mutex::new(None),
            notify: Notify::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
            active: AtomicBool::new(false),
        }
    }
}
struct Active<'a>(&'a Job);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.store(false, Ordering::Release);
        self.0.notify.notify_waiters();
    }
}
#[derive(Default)]
struct Registry {
    jobs: BTreeMap<String, Arc<Job>>,
    workers: Vec<JoinHandle<()>>,
    stopping: bool,
    recoveries: BTreeMap<String, Arc<tokio::sync::Mutex<()>>>,
}
pub(crate) struct ScanJobs {
    indexes: PathBuf,
    drafts: PathBuf,
    registry: Mutex<Registry>,
}
impl ScanJobs {
    pub(crate) fn new(runtime: &Path) -> Result<Arc<Self>, String> {
        directory(runtime)?;
        let runtime = runtime.canonicalize().map_err(|e| e.to_string())?;
        let indexes = runtime.join("scan-jobs");
        let drafts = runtime.join("drafts");
        directory(&indexes)?;
        directory(&drafts)?;
        Ok(Arc::new(Self {
            indexes,
            drafts,
            registry: Mutex::new(Registry::default()),
        }))
    }

    pub(crate) fn enqueue(
        self: &Arc<Self>,
        input: ScanInput,
        host: String,
        record: Record,
    ) -> Result<Value, String> {
        let id = uuid::Uuid::now_v7().to_string();
        let index = Index {
            schema: 1,
            scan_id: id.clone(),
            input,
            state: "queued".into(),
            stages: vec!["queued".into()],
            evidence_ids: vec![],
            diagnostic: None,
        };
        // Reserve the durable input before scheduling any command. No registry/session
        // lock is held across filesystem synchronization or Git execution.
        self.save_index(&index)?;
        let job = Arc::new(Job::new(index));
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| "scan registry lock poisoned")?;
        if registry.stopping {
            return Err("scan service is stopping".into());
        }
        job.active.store(true, Ordering::Release);
        registry.jobs.insert(id.clone(), job.clone());
        registry.workers.retain(|worker| !worker.is_finished());
        let jobs = self.clone();
        registry.workers.push(tokio::task::spawn_blocking(move || {
            let _active = Active(&job);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                jobs.execute(&job, &host, &record)
            }))
            .unwrap_or_else(|_| {
                Err((
                    "uncertain",
                    "scan worker panicked; durable acceptance must be reconciled".into(),
                ))
            });
            if let Err((state, error)) = result {
                jobs.fail(&job, state, error);
            }
        }));
        Ok(json!({"scan_id":id,"state":"queued","stages":["queued"]}))
    }

    fn lookup(&self, project_id: &str, common_dir: &Path, id: &str) -> Result<Arc<Job>, String> {
        scan_id(id)?;
        let job = self
            .registry
            .lock()
            .map_err(|_| "scan registry lock poisoned")?
            .jobs
            .get(id)
            .cloned()
            .ok_or("scan not found")?;
        self.check_context(&job, project_id, common_dir)?;
        Ok(job)
    }
    fn check_context(&self, job: &Job, project_id: &str, common_dir: &Path) -> Result<(), String> {
        let index = job.index.lock().map_err(|_| "scan index lock poisoned")?;
        if index.input.project_id != project_id || index.input.common_dir != common_dir {
            return Err("scan belongs to a different project or clone".into());
        }
        Ok(())
    }
    pub(crate) fn status(
        &self,
        project_id: &str,
        common_dir: &Path,
        id: &str,
    ) -> Result<Value, String> {
        let job = self.lookup(project_id, common_dir, id)?;
        view(&job)
    }
    pub(crate) async fn wait(
        &self,
        project_id: &str,
        common_dir: &Path,
        id: &str,
    ) -> Result<Value, String> {
        self.wait_budget(project_id, common_dir, id, WAIT_BUDGET)
            .await
    }
    async fn wait_budget(
        &self,
        project_id: &str,
        common_dir: &Path,
        id: &str,
        budget: Duration,
    ) -> Result<Value, String> {
        let job = self.lookup(project_id, common_dir, id)?;
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            // Register before reading state, so notify_waiters cannot be lost
            // between the terminal-state check and awaiting the notification.
            let notified = job.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let current = view(&job)?;
            if terminal(current["state"].as_str().unwrap_or("")) {
                return Ok(current);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return view(&job);
            }
        }
    }
    pub(crate) fn draft(
        &self,
        project_id: &str,
        common_dir: &Path,
        id: &str,
    ) -> Result<Value, String> {
        let job = self.lookup(project_id, common_dir, id)?;
        if view(&job)?["state"] != "completed" {
            return view(&job);
        }
        let expected = job
            .published
            .lock()
            .map_err(|_| "scan publication lock poisoned")?
            .clone()
            .ok_or("completed scan lacks verified publication")?;
        checked_directory(&self.drafts)?;
        let final_dir = self.drafts.join(id);
        checked_directory(&final_dir)?;
        let bytes = read_regular(&final_dir.join("response.json"), 32 * MAX_INDEX_BYTES)?;
        if bytes != json_bytes(&expected)? {
            return Err("derived draft cache changed; recovery required".into());
        }
        Ok(expected)
    }

    /// Only the caller's core history/outbox coordinator may restore completion.
    /// Persisted JSON is never deserialized into FrozenColdstart/SealedVerification.
    pub(crate) async fn recover(
        self: &Arc<Self>,
        project_id: &str,
        common_dir: &Path,
        id: &str,
        resume: Resume,
    ) -> Result<(), String> {
        scan_id(id)?;
        let single = {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| "scan registry lock poisoned")?;
            registry
                .recoveries
                .retain(|_, lock| Arc::strong_count(lock) > 1);
            registry
                .recoveries
                .entry(id.to_owned())
                .or_default()
                .clone()
        };
        // This per-id async guard also covers recovery when no runtime index exists.
        let single = single.lock_owned().await;
        let jobs = self.clone();
        let project_id = project_id.to_owned();
        let common_dir = common_dir.to_owned();
        let id = id.to_owned();
        let (send, receive) = oneshot::channel();
        {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| "scan registry lock poisoned")?;
            if registry.stopping {
                return Err("scan service is stopping".into());
            }
            registry.workers.retain(|worker| !worker.is_finished());
            registry.workers.push(tokio::task::spawn_blocking(move || {
                let _single = single;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    jobs.recover_blocking(&project_id, &common_dir, &id, &resume)
                }))
                .unwrap_or_else(|_| {
                    Err("scan recovery worker panicked; retry reconciliation".into())
                });
                let _ = send.send(result);
            }));
        }
        receive
            .await
            .map_err(|_| "scan recovery worker terminated")?
    }
    fn recover_blocking(
        &self,
        project_id: &str,
        common_dir: &Path,
        id: &str,
        resume: &Resume,
    ) -> Result<(), String> {
        directory(&self.indexes)?;
        let existing = self
            .registry
            .lock()
            .map_err(|_| "scan registry lock poisoned")?
            .jobs
            .get(id)
            .cloned();
        let mut from_history = None;
        let job = if let Some(job) = existing {
            self.check_context(&job, project_id, common_dir)?;
            job
        } else {
            let path = self.indexes.join(format!("{id}.json"));
            let index = match fs::symlink_metadata(&path) {
                Ok(_) => {
                    let mut index: Index =
                        serde_json::from_slice(&read_regular(&path, MAX_INDEX_BYTES)?)
                            .map_err(|e| e.to_string())?;
                    if index.schema != 1 || index.scan_id != id {
                        return Err("scan index schema/identity mismatch".into());
                    }
                    index.state = "recovering".into();
                    index
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let recorded = resume(id)?.ok_or("scan not found")?;
                    if recorded.scan_id != id
                        || recorded.input.project_id != project_id
                        || recorded.input.common_dir != common_dir
                    {
                        return Err("scan belongs to a different project or clone".into());
                    }
                    let index = Index {
                        schema: 1,
                        scan_id: id.into(),
                        input: recorded.input.clone(),
                        state: "recovering".into(),
                        stages: vec!["recovering".into()],
                        evidence_ids: vec![],
                        diagnostic: None,
                    };
                    self.save_index(&index)?;
                    from_history = Some(recorded);
                    index
                }
                Err(error) => return Err(error.to_string()),
            };
            let job = Arc::new(Job::new(index));
            self.check_context(&job, project_id, common_dir)?;
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| "scan registry lock poisoned")?;
            if registry.stopping {
                return Ok(());
            }
            registry.jobs.insert(id.into(), job.clone());
            job
        };
        if job
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }
        let _active = Active(&job);
        let state = view(&job)?;
        if (state["state"] == "completed" && !self.publication_missing(&job, id)?)
            || matches!(
                state["state"].as_str(),
                Some("interrupted" | "cancelled" | "failed")
            )
        {
            return Ok(());
        }
        let recorded = match from_history {
            Some(recorded) => Ok(Some(recorded)),
            None => resume(id),
        };
        let result = match recorded {
            Ok(Some(recorded)) => self.publish(&job, &recorded),
            Ok(None) => self.change(&job, "interrupted", Some("interrupted: no durable report; cleanup_identity_unverified; no stale PID or worktree was touched".into()), None, None),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.fail(&job, "uncertain", error);
        }
        Ok(())
    }
    fn publication_missing(&self, job: &Job, id: &str) -> Result<bool, String> {
        match fs::symlink_metadata(&self.drafts) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error.to_string()),
            Ok(_) => checked_directory(&self.drafts)?,
        }
        let target = self.drafts.join(id);
        match fs::symlink_metadata(&target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error.to_string()),
            Ok(_) => checked_directory(&target)?,
        }
        let response = target.join("response.json");
        match fs::symlink_metadata(&response) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error.to_string()),
            Ok(_) => {
                let published = job
                    .published
                    .lock()
                    .map_err(|_| "scan publication lock poisoned")?;
                let expected = published
                    .as_ref()
                    .ok_or("completed scan lacks verified publication")?;
                if read_regular(&response, 32 * MAX_INDEX_BYTES)? != json_bytes(expected)? {
                    return Err("derived draft cache changed; recovery required".into());
                }
                Ok(false)
            }
        }
    }
    pub(crate) fn cancel_all(&self) -> Result<(), String> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| "scan registry lock poisoned")?;
        registry.stopping = true;
        for job in registry.jobs.values() {
            job.cancelled.store(true, Ordering::Release);
        }
        Ok(())
    }
    pub(crate) async fn shutdown(&self) -> Result<(), String> {
        self.cancel_all()?;
        let workers = {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| "scan registry lock poisoned")?;
            std::mem::take(&mut registry.workers)
        };
        let mut errors = Vec::new();
        for worker in workers {
            if let Err(error) = worker.await {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn execute(
        &self,
        job: &Job,
        host: &str,
        record: &Record,
    ) -> Result<(), (&'static str, String)> {
        let run = || -> Result<(), String> {
            if job.cancelled.load(Ordering::Acquire) {
                return Err("scan cancelled".into());
            }
            self.change(job, "scanning", None, None, None)?;
            let input = job
                .index
                .lock()
                .map_err(|_| "scan index lock poisoned")?
                .input
                .clone();
            let id = job
                .index
                .lock()
                .map_err(|_| "scan index lock poisoned")?
                .scan_id
                .clone();
            let prepared = PreparedColdstart::prepare(input.clone(), &id)?;
            self.change(job, "verifying", None, None, None)?;
            let sealed = verifier::verify_sealed(&VerificationRequest {
                root: input.root.clone(),
                common_dir: input.common_dir.clone(),
                revision: input.revision.clone(),
                commands: prepared.commands()?,
                timeout: VERIFY_TIMEOUT,
                log_root: input.common_dir.join("towermd/cache/verification"),
                cancelled: job.cancelled.clone(),
            })?;
            if job.cancelled.load(Ordering::Acquire) {
                return Err("scan cancelled after verifier cleanup".into());
            }
            let frozen = prepared.freeze(&sealed, host, now_ms()?)?;
            self.change(job, "recording", None, None, None)?;
            record(&frozen)?;
            self.publish(job, &frozen.recorded())
        };
        run().map_err(|error| {
            let state = if job.cancelled.load(Ordering::Acquire) {
                "cancelled"
            } else if job
                .index
                .lock()
                .ok()
                .is_some_and(|i| matches!(i.state.as_str(), "recording" | "publishing"))
            {
                "uncertain"
            } else {
                "failed"
            };
            (state, error)
        })
    }
    fn publish(&self, job: &Job, recorded: &RecordedColdstart) -> Result<(), String> {
        let index = job
            .index
            .lock()
            .map_err(|_| "scan index lock poisoned")?
            .clone();
        if recorded.scan_id != index.scan_id
            || serde_json::to_value(&recorded.input).map_err(|e| e.to_string())?
                != serde_json::to_value(&index.input).map_err(|e| e.to_string())?
        {
            return Err("durable report differs from registered scan input".into());
        }
        let evidence_ids = recorded
            .evidence
            .iter()
            .map(|e| {
                e["id"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("recorded Evidence lacks id")
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.change(job, "publishing", None, Some(evidence_ids.clone()), None)?;
        let mut bundle = render_draft(&recorded.input, &recorded.scan)?;
        let response = json!({"scan_id":index.scan_id,"state":"completed", "input":recorded.input,
            "modules":bundle.files.iter().filter(|(p,_)| p.starts_with("arch/") && p.ends_with(".md"))
                .map(|(path,bytes)| Ok(json!({"path":path,"text":std::str::from_utf8(bytes).map_err(|e| e.to_string())?})))
                .collect::<Result<Vec<Value>,String>>()?,
            "canvas":serde_json::from_slice::<Value>(&bundle.files["tower.canvas"]).map_err(|e| e.to_string())?,
            "unassigned":recorded.scan["unassigned"],"candidate_tasks":recorded.scan["candidate_tasks"],
            "evidence":recorded.evidence,"report_eid":recorded.report_eid,"verification_report":recorded.report});
        bundle
            .files
            .insert("response.json".into(), json_bytes(&response)?);
        publish_directory(&self.drafts, &index.scan_id, &bundle.files)?;
        self.change(job, "completed", None, Some(evidence_ids), Some(response))
    }
    fn save_index(&self, index: &Index) -> Result<(), String> {
        scan_id(&index.scan_id)?;
        atomic_json(&self.indexes, &format!("{}.json", index.scan_id), index)
    }
    fn change(
        &self,
        job: &Job,
        state: &str,
        diagnostic: Option<String>,
        evidence_ids: Option<Vec<String>>,
        published: Option<Value>,
    ) -> Result<(), String> {
        let mut index = job.index.lock().map_err(|_| "scan index lock poisoned")?;
        let mut next = index.clone();
        next.state = state.into();
        if !next.stages.iter().any(|stage| stage == state) {
            next.stages.push(state.into());
        }
        next.diagnostic = diagnostic;
        if let Some(ids) = evidence_ids {
            next.evidence_ids = ids;
        }
        self.save_index(&next)?;
        if let Some(published) = published {
            *job.published
                .lock()
                .map_err(|_| "scan publication lock poisoned")? = Some(published);
        }
        *index = next;
        drop(index);
        job.notify.notify_waiters();
        Ok(())
    }
    fn fail(&self, job: &Job, state: &str, error: String) {
        if let Err(persistence) = self.change(job, state, Some(error.clone()), None, None) {
            if let Ok(mut index) = job.index.lock() {
                index.state = "uncertain".into();
                index.diagnostic = Some(format!(
                    "{error}; runtime index persistence failed: {persistence}"
                ));
            }
            job.notify.notify_waiters();
        }
    }
}
fn view(job: &Job) -> Result<Value, String> {
    let index = job.index.lock().map_err(|_| "scan index lock poisoned")?;
    Ok(
        json!({"scan_id":index.scan_id,"state":index.state,"stages":index.stages,
        "evidence_ids":index.evidence_ids,"diagnostic":index.diagnostic}),
    )
}
fn terminal(state: &str) -> bool {
    matches!(
        state,
        "completed" | "failed" | "uncertain" | "interrupted" | "cancelled"
    )
}
fn scan_id(id: &str) -> Result<(), String> {
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| "invalid scan_id")?;
    if uuid.get_version_num() != 7 || uuid.to_string() != id {
        return Err("invalid scan_id".into());
    }
    Ok(())
}
fn now_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis()
        .try_into()
        .map_err(|_| "scan clock overflow".into())
}
fn checked_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("unsafe scan directory: {}", path.display()));
    }
    Ok(())
}
fn directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => checked_directory(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or("scan directory lacks parent")?;
            if !parent.exists() {
                directory(parent)?;
            }
            fs::create_dir(path).map_err(|e| e.to_string())?;
            File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            checked_directory(path)
        }
        Err(e) => Err(e.to_string()),
    }
}
fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err("unsafe or oversized scan file".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("oversized scan file".into());
    }
    Ok(bytes)
}
fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn atomic_json(parent: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    checked_directory(parent)?;
    let target = parent.join(name);
    if fs::symlink_metadata(&target).is_ok() {
        read_regular(&target, MAX_INDEX_BYTES)?;
    }
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::now_v7()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(&json_bytes(value)?)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temporary, &target).map_err(|e| e.to_string())?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn publish_directory(
    parent: &Path,
    id: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    scan_id(id)?;
    directory(parent)?;
    let target = parent.join(id);
    if fs::symlink_metadata(&target).is_ok() {
        checked_directory(&target)?;
        let mut missing = Vec::new();
        for (path, bytes) in files {
            let path = safe_child(&target, path)?;
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.push((path, bytes));
                }
                Err(error) => return Err(error.to_string()),
                Ok(_) => {
                    if read_regular(&path, 32 * MAX_INDEX_BYTES)? != *bytes {
                        return Err("existing derived draft differs from durable report".into());
                    }
                }
            }
        }
        // Validate all surviving bytes before restoring only missing derived files.
        // The job remains publishing until the complete set is durable again.
        for (path, bytes) in missing {
            let parent = path.parent().ok_or("draft file lacks parent")?;
            directory(parent)?;
            let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::now_v7()));
            let result = (|| {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(|e| e.to_string())?;
                file.write_all(bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|e| e.to_string())?;
                // Link is an atomic create-if-absent: a competing file or symlink
                // cannot be overwritten by restoring this missing cache entry.
                fs::hard_link(&temporary, &path).map_err(|e| e.to_string())?;
                File::open(parent)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| e.to_string())
            })();
            let _ = fs::remove_file(&temporary);
            result?;
        }
        return Ok(());
    }
    let temporary = parent.join(format!(".{}-{}", id, uuid::Uuid::now_v7()));
    fs::create_dir(&temporary).map_err(|e| e.to_string())?;
    let result = (|| {
        for (path, bytes) in files {
            let path = safe_child(&temporary, path)?;
            let directory_path = path.parent().ok_or("draft file lacks parent")?;
            directory(directory_path)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            File::open(directory_path)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
        }
        File::open(&temporary)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temporary, &target).map_err(|e| e.to_string())?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}
fn safe_child(parent: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
        || relative.is_empty()
    {
        return Err("unsafe derived draft path".into());
    }
    let mut current = parent.to_owned();
    for component in path.components() {
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("unsafe derived draft parent".into());
            }
        }
        current.push(component);
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use towermd_core::{
        catalog::Catalog,
        coldstart::{capture_scan_input, ScanOptions},
        config::Config,
        repo,
    };

    fn git(root: &Path, args: &[&str]) -> String {
        let mut options = vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=t@invalid",
            "-c",
            "commit.gpgsign=false",
        ];
        options.extend_from_slice(args);
        repo::git(root, &options).unwrap()
    }
    fn fixture(makefile: Option<&str>) -> (tempfile::TempDir, ScanInput) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-qb", "main"]);
        fs::create_dir(dir.path().join(".towermd")).unwrap();
        fs::write(
            dir.path().join(".towermd/config.yaml"),
            Config::new("test", "docs/tower").unwrap().to_yaml(),
        )
        .unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.py"), "print('fixture')\n").unwrap();
        if let Some(text) = makefile {
            fs::write(dir.path().join("Makefile"), text).unwrap();
        }
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "fixture"]);
        let catalog = Catalog::load(dir.path()).unwrap();
        let input = capture_scan_input(&catalog, &ScanOptions::default()).unwrap();
        (dir, input)
    }
    fn queued(jobs: &ScanJobs, input: ScanInput) -> Arc<Job> {
        let index = Index {
            schema: 1,
            scan_id: uuid::Uuid::now_v7().to_string(),
            input,
            state: "queued".into(),
            stages: vec!["queued".into()],
            evidence_ids: vec![],
            diagnostic: None,
        };
        jobs.save_index(&index).unwrap();
        let id = index.scan_id.clone();
        let job = Arc::new(Job::new(index));
        jobs.registry.lock().unwrap().jobs.insert(id, job.clone());
        job
    }
    #[tokio::test]
    async fn wait_budget_is_bounded_and_notification_wakes_all_repeat_waiters() {
        let (_repo, input) = fixture(None);
        let runtime = tempfile::tempdir().unwrap();
        let jobs = ScanJobs::new(runtime.path()).unwrap();
        let job = queued(&jobs, input.clone());
        let id = job.index.lock().unwrap().scan_id.clone();
        let start = tokio::time::Instant::now();
        let response = jobs
            .wait_budget(
                &input.project_id,
                &input.common_dir,
                &id,
                Duration::from_millis(15),
            )
            .await
            .unwrap();
        assert_eq!(response["state"], "queued");
        assert!(start.elapsed() < Duration::from_secs(1));
        let change = jobs.clone();
        let waiter1 = jobs.wait_budget(
            &input.project_id,
            &input.common_dir,
            &id,
            Duration::from_secs(2),
        );
        let waiter2 = jobs.wait_budget(
            &input.project_id,
            &input.common_dir,
            &id,
            Duration::from_secs(2),
        );
        let finish = async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            change
                .change(&job, "failed", Some("fixture".into()), None, None)
                .unwrap();
        };
        let (first, second, _) = tokio::join!(waiter1, waiter2, finish);
        assert_eq!(first.unwrap()["state"], "failed");
        assert_eq!(second.unwrap()["state"], "failed");
        assert_eq!(
            jobs.wait(&input.project_id, &input.common_dir, &id)
                .await
                .unwrap()["state"],
            "failed"
        );
    }
    #[tokio::test]
    async fn real_scan_records_once_before_atomic_publication_and_never_holds_registry() {
        let (_repo, input) = fixture(None);
        let runtime = tempfile::tempdir().unwrap();
        let jobs = ScanJobs::new(runtime.path()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (entered, received) = oneshot::channel();
        let entered = Mutex::new(Some(entered));
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let worker_gate = gate.clone();
        let count = calls.clone();
        let queued = jobs
            .enqueue(
                input.clone(),
                "test-host".into(),
                Arc::new(move |frozen| {
                    assert!(!frozen.evidence().is_empty());
                    count.fetch_add(1, Ordering::SeqCst);
                    entered.lock().unwrap().take().unwrap().send(()).unwrap();
                    let (lock, changed) = &*worker_gate;
                    let mut released = lock.lock().unwrap();
                    while !*released {
                        released = changed.wait(released).unwrap();
                    }
                    Ok(())
                }),
            )
            .unwrap();
        let id = queued["scan_id"].as_str().unwrap();
        tokio::time::timeout(Duration::from_secs(10), received)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            jobs.status(&input.project_id, &input.common_dir, id)
                .unwrap()["state"],
            "recording"
        );
        assert!(!jobs.drafts.join(id).exists());
        assert_eq!(
            jobs.draft(&input.project_id, &input.common_dir, id)
                .unwrap()["state"],
            "recording"
        );
        // An active worker cannot be resumed or declared interrupted by concurrent GET/wait.
        jobs.recover(
            &input.project_id,
            &input.common_dir,
            id,
            Arc::new(|_| panic!("active scan resumed")),
        )
        .await
        .unwrap();
        let (lock, changed) = &*gate;
        *lock.lock().unwrap() = true;
        changed.notify_all();
        assert_eq!(
            jobs.wait(&input.project_id, &input.common_dir, id)
                .await
                .unwrap()["state"],
            "completed"
        );
        let draft = jobs
            .draft(&input.project_id, &input.common_dir, id)
            .unwrap();
        assert!(!draft["modules"].as_array().unwrap().is_empty());
        assert_eq!(draft["evidence"][0]["result"], "no_tests");
        for _ in 0..2 {
            jobs.recover(
                &input.project_id,
                &input.common_dir,
                id,
                Arc::new(|_| panic!("complete live scan resumed")),
            )
            .await
            .unwrap();
            assert_eq!(
                jobs.wait(&input.project_id, &input.common_dir, id)
                    .await
                    .unwrap()["state"],
                "completed"
            );
            assert_eq!(
                jobs.draft(&input.project_id, &input.common_dir, id)
                    .unwrap(),
                draft
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(jobs.drafts.join(id).join("tower.canvas").is_file());
        assert!(fs::read_dir(&jobs.drafts).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with('.')));
        jobs.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn accepted_report_recovers_same_evidence_after_result_was_not_published() {
        let (_repo, input) = fixture(None);
        let runtime = tempfile::tempdir().unwrap();
        let jobs = ScanJobs::new(runtime.path()).unwrap();
        let durable = Arc::new(Mutex::new(None));
        let saved = durable.clone();
        let queued = jobs
            .enqueue(
                input.clone(),
                "test-host".into(),
                Arc::new(move |frozen| {
                    *saved.lock().unwrap() = Some(frozen.recorded());
                    Err("fixture: durable accepted, response uncertain".into())
                }),
            )
            .unwrap();
        let id = queued["scan_id"].as_str().unwrap().to_owned();
        assert_eq!(
            jobs.wait(&input.project_id, &input.common_dir, &id)
                .await
                .unwrap()["state"],
            "uncertain"
        );
        assert!(!jobs.drafts.join(&id).exists());
        jobs.shutdown().await.unwrap();
        drop(jobs);
        let restored = ScanJobs::new(runtime.path()).unwrap();
        let resume_calls = Arc::new(AtomicUsize::new(0));
        let count = resume_calls.clone();
        let recorded = durable.lock().unwrap().clone().unwrap();
        let evidence = recorded.evidence.clone();
        restored
            .recover(
                &input.project_id,
                &input.common_dir,
                &id,
                Arc::new(move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(recorded.clone()))
                }),
            )
            .await
            .unwrap();
        assert_eq!(
            restored
                .wait(&input.project_id, &input.common_dir, &id)
                .await
                .unwrap()["state"],
            "completed"
        );
        assert_eq!(
            restored
                .draft(&input.project_id, &input.common_dir, &id)
                .unwrap()["evidence"],
            json!(evidence)
        );
        restored
            .recover(
                &input.project_id,
                &input.common_dir,
                &id,
                Arc::new(|_| panic!("duplicate recovery")),
            )
            .await
            .unwrap();
        assert_eq!(resume_calls.load(Ordering::SeqCst), 1);
        restored.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn deleted_runtime_indexes_and_drafts_rebuild_only_from_history_without_rerun() {
        let counter = tempfile::tempdir().unwrap();
        let count_path = counter.path().join("runs");
        let command = format!("test:\n\t@echo run >> '{}'\n", count_path.display());
        let (_repo, input) = fixture(Some(&command));
        let runtime = tempfile::tempdir().unwrap();
        let jobs = ScanJobs::new(runtime.path()).unwrap();
        let saved = Arc::new(Mutex::new(None));
        let captured = saved.clone();
        let queued = jobs
            .enqueue(
                input.clone(),
                "test-host".into(),
                Arc::new(move |frozen| {
                    *captured.lock().unwrap() = Some(frozen.recorded());
                    Ok(())
                }),
            )
            .unwrap();
        let id = queued["scan_id"].as_str().unwrap().to_owned();
        assert_eq!(
            jobs.wait(&input.project_id, &input.common_dir, &id)
                .await
                .unwrap()["state"],
            "completed"
        );
        let original = jobs
            .draft(&input.project_id, &input.common_dir, &id)
            .unwrap();
        let recorded = saved.lock().unwrap().clone().unwrap();
        let live_resume: Resume = Arc::new(move |_| Ok(Some(recorded.clone())));
        fs::remove_file(jobs.drafts.join(&id).join("response.json")).unwrap();
        jobs.recover(
            &input.project_id,
            &input.common_dir,
            &id,
            live_resume.clone(),
        )
        .await
        .unwrap();
        assert_eq!(
            jobs.draft(&input.project_id, &input.common_dir, &id)
                .unwrap(),
            original
        );
        fs::remove_dir_all(jobs.drafts.join(&id)).unwrap();
        jobs.recover(
            &input.project_id,
            &input.common_dir,
            &id,
            live_resume.clone(),
        )
        .await
        .unwrap();
        assert_eq!(
            jobs.draft(&input.project_id, &input.common_dir, &id)
                .unwrap(),
            original
        );
        fs::remove_dir_all(&jobs.indexes).unwrap();
        fs::remove_dir_all(&jobs.drafts).unwrap();
        jobs.recover(
            &input.project_id,
            &input.common_dir,
            &id,
            live_resume.clone(),
        )
        .await
        .unwrap();
        assert_eq!(
            jobs.draft(&input.project_id, &input.common_dir, &id)
                .unwrap(),
            original
        );
        let response = jobs.drafts.join(&id).join("response.json");
        fs::write(&response, "tampered").unwrap();
        assert!(jobs
            .recover(&input.project_id, &input.common_dir, &id, live_resume)
            .await
            .is_err());
        assert_eq!(fs::read(&response).unwrap(), b"tampered");
        fs::write(&response, json_bytes(&original).unwrap()).unwrap();
        assert_eq!(fs::read_to_string(&count_path).unwrap(), "run\n");
        jobs.shutdown().await.unwrap();
        drop(jobs);
        fs::remove_dir_all(runtime.path().join("scan-jobs")).unwrap();
        fs::remove_dir_all(runtime.path().join("drafts")).unwrap();
        let rebuilt = ScanJobs::new(runtime.path()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let call_count = calls.clone();
        let recorded = saved.lock().unwrap().clone().unwrap();
        let resume: Resume = Arc::new(move |_| {
            call_count.fetch_add(1, Ordering::SeqCst);
            Ok(Some(recorded.clone()))
        });
        let (one, two) = tokio::join!(
            rebuilt.recover(&input.project_id, &input.common_dir, &id, resume.clone()),
            rebuilt.recover(&input.project_id, &input.common_dir, &id, resume),
        );
        one.unwrap();
        two.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            rebuilt
                .draft(&input.project_id, &input.common_dir, &id)
                .unwrap(),
            original
        );
        assert_eq!(fs::read_to_string(&count_path).unwrap(), "run\n");
        assert_eq!(
            rebuilt
                .recover(
                    &input.project_id,
                    &input.common_dir,
                    &uuid::Uuid::now_v7().to_string(),
                    Arc::new(|_| Ok(None))
                )
                .await
                .unwrap_err(),
            "scan not found"
        );
        #[cfg(unix)]
        {
            let response = rebuilt.drafts.join(&id).join("response.json");
            fs::remove_file(&response).unwrap();
            std::os::unix::fs::symlink(&count_path, &response).unwrap();
            assert!(rebuilt
                .draft(&input.project_id, &input.common_dir, &id)
                .is_err());
            assert_eq!(fs::read_to_string(&count_path).unwrap(), "run\n");
        }
        rebuilt.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn dropped_http_recovery_future_keeps_worker_single_flight_until_publication() {
        let (_repo, input) = fixture(None);
        let runtime = tempfile::tempdir().unwrap();
        let jobs = ScanJobs::new(runtime.path()).unwrap();
        let saved = Arc::new(Mutex::new(None));
        let captured = saved.clone();
        let queued = jobs
            .enqueue(
                input.clone(),
                "test-host".into(),
                Arc::new(move |frozen| {
                    *captured.lock().unwrap() = Some(frozen.recorded());
                    Ok(())
                }),
            )
            .unwrap();
        let id = queued["scan_id"].as_str().unwrap().to_owned();
        assert_eq!(
            jobs.wait(&input.project_id, &input.common_dir, &id)
                .await
                .unwrap()["state"],
            "completed"
        );
        jobs.shutdown().await.unwrap();
        drop(jobs);
        fs::remove_dir_all(runtime.path().join("scan-jobs")).unwrap();
        fs::remove_dir_all(runtime.path().join("drafts")).unwrap();
        let rebuilt = ScanJobs::new(runtime.path()).unwrap();
        let recorded = saved.lock().unwrap().clone().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (entered, received) = oneshot::channel();
        let entered = Mutex::new(Some(entered));
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let worker_gate = gate.clone();
        let resume: Resume = Arc::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            if let Some(entered) = entered.lock().unwrap().take() {
                entered.send(()).unwrap();
            }
            let (lock, changed) = &*worker_gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = changed.wait(released).unwrap();
            }
            Ok(Some(recorded.clone()))
        });
        let start = |resume: Resume| {
            let jobs = rebuilt.clone();
            let input = input.clone();
            let id = id.clone();
            tokio::spawn(async move {
                jobs.recover(&input.project_id, &input.common_dir, &id, resume)
                    .await
            })
        };
        let first = start(resume.clone());
        received.await.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        let second = start(resume);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!second.is_finished());
        let (lock, changed) = &*gate;
        *lock.lock().unwrap() = true;
        changed.notify_all();
        tokio::time::timeout(Duration::from_secs(5), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            rebuilt
                .draft(&input.project_id, &input.common_dir, &id)
                .unwrap()["state"],
            "completed"
        );
        rebuilt.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn persisted_completed_without_real_report_is_interrupted_and_old_pid_is_untouched() {
        let (_repo, input) = fixture(None);
        let runtime = tempfile::tempdir().unwrap();
        let jobs = ScanJobs::new(runtime.path()).unwrap();
        let job = queued(&jobs, input.clone());
        let mut value = serde_json::to_value(&*job.index.lock().unwrap()).unwrap();
        value["state"] = json!("completed");
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        value["pgid"] = json!(child.id()); // ignored historical runtime metadata is never authority to kill
        let owned = runtime.path().join("unverified-old-worktree");
        fs::create_dir(&owned).unwrap();
        value["worktree"] = json!(owned);
        let id = value["scan_id"].as_str().unwrap().to_owned();
        atomic_json(&jobs.indexes, &format!("{id}.json"), &value).unwrap();
        drop(jobs);
        let restored = ScanJobs::new(runtime.path()).unwrap();
        restored
            .recover(
                &input.project_id,
                &input.common_dir,
                &id,
                Arc::new(|_| Ok(None)),
            )
            .await
            .unwrap();
        let state = restored
            .wait(&input.project_id, &input.common_dir, &id)
            .await
            .unwrap();
        assert_eq!(state["state"], "interrupted");
        assert!(state["diagnostic"]
            .as_str()
            .unwrap()
            .contains("cleanup_identity_unverified"));
        assert!(child.try_wait().unwrap().is_none());
        assert!(owned.is_dir());
        assert!(!restored.drafts.join(&id).exists());
        child.kill().unwrap(); // test-owned child, never a runtime supplied PID
        child.wait().unwrap();
        restored.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn context_ids_and_runtime_symlinks_are_rejected() {
        let (_repo, input) = fixture(None);
        let runtime = tempfile::tempdir().unwrap();
        let jobs = ScanJobs::new(runtime.path()).unwrap();
        let job = queued(&jobs, input.clone());
        let id = job.index.lock().unwrap().scan_id.clone();
        assert!(jobs
            .status("other-project", &input.common_dir, &id)
            .is_err());
        assert!(jobs
            .status(&input.project_id, &runtime.path().join("other-clone"), &id)
            .is_err());
        assert!(jobs
            .status(&input.project_id, &input.common_dir, "../escape")
            .is_err());
        assert_eq!(
            jobs.status(
                &input.project_id,
                &input.common_dir,
                &uuid::Uuid::now_v7().to_string()
            )
            .unwrap_err(),
            "scan not found"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let path = jobs.indexes.join(format!("{id}.json"));
            fs::remove_file(&path).unwrap();
            symlink(runtime.path().join("outside"), &path).unwrap();
            jobs.registry.lock().unwrap().jobs.remove(&id);
            assert!(jobs
                .recover(
                    &input.project_id,
                    &input.common_dir,
                    &id,
                    Arc::new(|_| panic!("symlink was read"))
                )
                .await
                .is_err());
            let other = tempfile::tempdir().unwrap();
            let runtime2 = tempfile::tempdir().unwrap();
            symlink(other.path(), runtime2.path().join("drafts")).unwrap();
            assert!(ScanJobs::new(runtime2.path()).is_err());
        }
        jobs.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn shutdown_cancels_running_verifier_and_drains_owned_worktree() {
        let marker = tempfile::tempdir().unwrap();
        let command = format!(
            "test:\n\t@echo started > '{}'\n\t@sleep 300\n",
            marker.path().join("started").display()
        );
        let (_repo, input) = fixture(Some(&command));
        let runtime = tempfile::tempdir().unwrap();
        let jobs = ScanJobs::new(runtime.path()).unwrap();
        let before = git(&input.root, &["worktree", "list", "--porcelain"]);
        let queued = jobs
            .enqueue(
                input.clone(),
                "test-host".into(),
                Arc::new(|_| panic!("cancelled job must not record")),
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(15), async {
            while !marker.path().join("started").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), jobs.shutdown())
            .await
            .unwrap()
            .unwrap();
        let id = queued["scan_id"].as_str().unwrap();
        assert_eq!(
            jobs.status(&input.project_id, &input.common_dir, id)
                .unwrap()["state"],
            "cancelled"
        );
        assert_eq!(
            git(&input.root, &["worktree", "list", "--porcelain"]),
            before
        );
        assert!(jobs
            .enqueue(input.clone(), "test-host".into(), Arc::new(|_| Ok(())))
            .is_err());
    }
}
