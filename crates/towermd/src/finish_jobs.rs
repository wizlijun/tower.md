//! Process-local scheduling only. Core original events remain the only Job and
//! completion authority; no JSON index can mint an execution ticket.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};
use tokio::{sync::Notify, task::JoinHandle};

pub(crate) const WAIT_LIMIT: Duration = Duration::from_secs(90);
pub(crate) const VERIFY_TIMEOUT: Duration = Duration::from_secs(300);
type Work = Box<dyn FnOnce(Arc<AtomicBool>) -> Result<Value, String> + Send>;
// Process supervision only. Only a real private FinishTicket may install it;
// cached/client JSON cannot nominate a process or deadline for cancellation.
#[derive(PartialEq)]
struct OwnedDeadline {
    run: String,
    evidence: String,
    deadline: u64,
}
struct Job {
    common_dir: PathBuf,
    view: Mutex<Value>,
    executed: Mutex<Option<towermd_core::finish::ExecutedFinish>>,
    notify: Notify,
    active: AtomicBool,
    // Process supervision only; false before real execute, updated only by its
    // non-deserializable output. A worker panic cannot certify child cleanup.
    cleanup_confirmed: AtomicBool,
    expiring: AtomicBool,
    owned_deadline: Mutex<Option<OwnedDeadline>>,
    drained: (Mutex<bool>, Condvar),
    cancelled: Arc<AtomicBool>,
}
#[derive(Default)]
struct Registry {
    jobs: BTreeMap<(String, String), Arc<Job>>,
    workers: Vec<JoinHandle<()>>,
    stopping: bool,
}
#[derive(Default)]
pub(crate) struct FinishJobs {
    registry: Mutex<Registry>,
}
fn note_cleanup_unknown(view: &mut Value) {
    view["cleanup_unknown"] = json!(true);
    view["cleanup_diagnostic"] = json!("previous execution process identity/exit is unknown; no old process was signalled and its verification directory is retained");
}
impl FinishJobs {
    // Diagnostic only: absence from this registry is not proof that a previous
    // daemon's process group exited. Only our original ticket plus actual runner
    // cleanup can establish that local supervision completed.
    fn recovered_running_cleanup_unknown(
        &self,
        project: &str,
        common: &Path,
        status: &towermd_core::finish::FinishStatus,
    ) -> Result<bool, String> {
        if status.phase != towermd_core::finish::JobPhase::Running {
            return Ok(false);
        }
        let job = match self.job(project, common, &status.input.evidence_id) {
            Ok(job) => job,
            Err(error) if error == "finish not found" => return Ok(true),
            Err(error) => return Err(error),
        };
        let owned = job
            .owned_deadline
            .lock()
            .map_err(|_| "finish ownership lock poisoned")?;
        let same_ticket = owned.as_ref().is_some_and(|scope| {
            scope.run == status.input.run_id
                && scope.evidence == status.input.evidence_id
                && scope.deadline == status.input.merge_deadline
        });
        Ok(!same_ticket
            || job.active.load(Ordering::Acquire)
            || !job.cleanup_confirmed.load(Ordering::Acquire))
    }
    pub(crate) fn annotate_expired_cleanup(
        &self,
        project: &str,
        common: &Path,
        status: &towermd_core::finish::FinishStatus,
        view: &mut Value,
    ) -> Result<(), String> {
        // Process supervision diagnostics are independent of the core's
        // typed expiry receipt. Never inherit receipt fields from Job JSON.
        if view["run_state"] == "expired"
            && self.recovered_running_cleanup_unknown(project, common, status)?
        {
            note_cleanup_unknown(view);
        }
        Ok(())
    }
    /// Initial must have just been obtained from the real core producer/reader.
    pub(crate) fn attach(
        &self,
        project: &str,
        common: &Path,
        id: &str,
        initial: Value,
    ) -> Result<(), String> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| "finish registry lock poisoned")?;
        let key = (project.to_owned(), id.to_owned());
        if let Some(job) = registry.jobs.get(&key) {
            if job.common_dir != common {
                return Err("finish belongs to another clone".into());
            }
            if !job.active.load(Ordering::Acquire) {
                *job.view.lock().map_err(|_| "finish view lock poisoned")? = initial;
                job.notify.notify_waiters();
            }
        } else {
            registry.jobs.insert(
                key,
                Arc::new(Job {
                    common_dir: common.to_owned(),
                    view: Mutex::new(initial),
                    executed: Mutex::new(None),
                    notify: Notify::new(),
                    active: AtomicBool::new(false),
                    cleanup_confirmed: AtomicBool::new(true),
                    expiring: AtomicBool::new(false),
                    owned_deadline: Mutex::new(None),
                    drained: (Mutex::new(true), Condvar::new()),
                    cancelled: Arc::new(AtomicBool::new(false)),
                }),
            );
        }
        Ok(())
    }
    fn job(&self, project: &str, common: &Path, id: &str) -> Result<Arc<Job>, String> {
        let job = self
            .registry
            .lock()
            .map_err(|_| "finish registry lock poisoned")?
            .jobs
            .get(&(project.to_owned(), id.to_owned()))
            .cloned()
            .ok_or("finish not found")?;
        if job.common_dir != common {
            return Err("finish belongs to another clone".into());
        }
        Ok(job)
    }
    pub(crate) fn active(&self, project: &str, common: &Path, id: &str) -> Result<bool, String> {
        match self.job(project, common, id) {
            Ok(job) => Ok(job.active.load(Ordering::Acquire)),
            Err(error) if error == "finish not found" => Ok(false),
            Err(error) => Err(error),
        }
    }
    /// Called only immediately before consuming an actual ExecutionTicket.
    pub(crate) fn execution_entered(
        &self,
        project: &str,
        common: &Path,
        id: &str,
    ) -> Result<(), String> {
        self.job(project, common, id)?
            .cleanup_confirmed
            .store(false, Ordering::Release);
        Ok(())
    }
    /// This is real, non-deserializable executor output, never a cached report.
    /// It stays owned until the original terminal receipt is durable.
    pub(crate) fn retain_executed(
        &self,
        project: &str,
        common: &Path,
        id: &str,
        executed: towermd_core::finish::ExecutedFinish,
    ) -> Result<(), String> {
        let job = self.job(project, common, id)?;
        let mut retained = job
            .executed
            .lock()
            .map_err(|_| "finish result lock poisoned")?;
        if retained.is_some() {
            return Err("finish already owns actual execution output".into());
        }
        job.cleanup_confirmed
            .store(executed.cleanup_confirmed(), Ordering::Release);
        *retained = Some(executed);
        Ok(())
    }
    pub(crate) fn has_executed(
        &self,
        project: &str,
        common: &Path,
        id: &str,
    ) -> Result<bool, String> {
        match self.job(project, common, id) {
            Ok(job) => {
                let present = job
                    .executed
                    .lock()
                    .map_err(|_| "finish result lock poisoned")?
                    .is_some();
                Ok(present)
            }
            Err(error) if error == "finish not found" => Ok(false),
            Err(error) => Err(error),
        }
    }
    /// The callback only records the borrowed original output. A failed source
    /// proof keeps it for same-ID coordination and cannot rerun verification.
    pub(crate) fn record_executed(
        &self,
        project: &str,
        common: &Path,
        id: &str,
        record: impl FnOnce(&towermd_core::finish::ExecutedFinish) -> Result<Value, String>,
    ) -> Result<Option<Value>, String> {
        let job = self.job(project, common, id)?;
        let mut retained = job
            .executed
            .lock()
            .map_err(|_| "finish result lock poisoned")?;
        let Some(executed) = retained.as_ref() else {
            return Ok(None);
        };
        let result = record(executed)?;
        if executed.cleanup_confirmed() {
            *retained = None;
        }
        Ok(Some(result))
    }
    pub(crate) fn submit(
        &self,
        project: &str,
        common: &Path,
        id: &str,
        work: Work,
    ) -> Result<(), String> {
        let job = self.job(project, common, id)?;
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| "finish registry lock poisoned")?;
        if !job.cleanup_confirmed.load(Ordering::Acquire) && !job.active.load(Ordering::Acquire) {
            return Err(
                "finish process cleanup unconfirmed; retained original execution output".into(),
            );
        }
        if registry.stopping {
            return Err(format!("finish request {id} uncertain: daemon is stopping"));
        }
        if job.expiring.load(Ordering::Acquire) {
            return Err("finish expiry is pending; original job cannot execute again".into());
        }
        let mut drained = job
            .drained
            .0
            .lock()
            .map_err(|_| "finish drain lock poisoned")?;
        if job.active.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        *drained = false;
        drop(drained);
        job.cancelled.store(false, Ordering::Release);
        let id = id.to_owned();
        registry.workers.retain(|worker| !worker.is_finished());
        registry.workers.push(tokio::task::spawn_blocking(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                work(job.cancelled.clone())
            }))
            .unwrap_or_else(|_| Err("finish worker panicked; retry the original ID".into()));
            let view = match result {
                Ok(view) => view,
                Err(error) => {
                    eprintln!("towermd finish worker {id}: {error}");
                    json!({"evidence_id":id,"job_state":"uncertain","diagnostic":error})
                }
            };
            if let Ok(mut current) = job.view.lock() {
                *current = view;
            }
            // Signal only after execution, child cleanup and recording have all
            // left the worker body. An uncertain cached view is not a drain.
            if let Ok(mut drained) = job.drained.0.lock() {
                *drained = true;
                job.active.store(false, Ordering::Release);
                job.drained.1.notify_all();
            }
            job.notify.notify_waiters();
        }));
        Ok(())
    }
    pub(crate) async fn wait(
        &self,
        project: &str,
        common: &Path,
        id: &str,
        budget: Duration,
    ) -> Result<Value, String> {
        let job = self.job(project, common, id)?;
        wait_job(
            &job.notify,
            || {
                job.view
                    .lock()
                    .map(|view| view.clone())
                    .map_err(|_| "finish view lock poisoned".into())
            },
            budget,
        )
        .await
    }
    pub(crate) fn register_owned_ticket(
        &self,
        project: &str,
        common: &Path,
        ticket: &towermd_core::finish::FinishTicket,
    ) -> Result<(), String> {
        let input = &ticket.status().input;
        if input.project_id != project || input.common_dir != common {
            return Err("owned finish ticket belongs to another project/clone".into());
        }
        let job = self.job(project, common, &input.evidence_id)?;
        let scope = OwnedDeadline {
            run: input.run_id.clone(),
            evidence: input.evidence_id.clone(),
            deadline: input.merge_deadline,
        };
        let mut owned = job
            .owned_deadline
            .lock()
            .map_err(|_| "finish ownership lock poisoned")?;
        if owned.as_ref().is_some_and(|prior| prior != &scope) {
            return Err("owned finish ticket changed its original Run/deadline".into());
        }
        *owned = Some(scope);
        Ok(())
    }
    fn due_owned_ids(&self, project: &str, common: &Path, now: u64) -> Result<Vec<String>, String> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| "finish registry lock poisoned")?;
        let mut due = Vec::new();
        for ((owner, id), job) in &registry.jobs {
            if owner != project {
                continue;
            }
            if job.common_dir != common {
                return Err("finish belongs to another clone".into());
            }
            if job.active.load(Ordering::Acquire)
                && job
                    .owned_deadline
                    .lock()
                    .map_err(|_| "finish ownership lock poisoned")?
                    .as_ref()
                    .is_some_and(|scope| scope.evidence == *id && now >= scope.deadline)
            {
                due.push(id.clone());
            }
        }
        Ok(due)
    }
    pub(crate) fn has_due_owned(
        &self,
        project: &str,
        common: &Path,
        now: u64,
    ) -> Result<bool, String> {
        Ok(!self.due_owned_ids(project, common, now)?.is_empty())
    }
    /// Source failure does not extend the original owned process lifetime.
    /// No business state, receipt or retained report is changed here.
    pub(crate) fn cancel_due_owned(
        &self,
        project: &str,
        common: &Path,
        now: u64,
    ) -> Result<(), String> {
        self.cancel_and_drain(project, common, &self.due_owned_ids(project, common, now)?)
    }
    /// Scheduling hint only; core still proves actual ownership and all fences.
    pub(crate) fn owned_evidence_ids(
        &self,
        project: &str,
        common: &Path,
    ) -> Result<BTreeSet<String>, String> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| "finish registry lock poisoned")?;
        let mut ids = BTreeSet::new();
        for ((owner, id), job) in &registry.jobs {
            if owner == project {
                if job.common_dir != common {
                    return Err("finish belongs to another clone".into());
                }
                if job.active.load(Ordering::Acquire) && !job.expiring.load(Ordering::Acquire) {
                    ids.insert(id.clone());
                }
            }
        }
        Ok(ids)
    }
    /// Called with no producer, Session, outbox or registry lock. It cannot wait
    /// on the registry's global worker list, which includes this maintenance job.
    /// Missing jobs mean no worker is owned here, not that an old process exited.
    pub(crate) fn cancel_and_drain(
        &self,
        project: &str,
        common: &Path,
        ids: &[String],
    ) -> Result<(), String> {
        let jobs = {
            let registry = self
                .registry
                .lock()
                .map_err(|_| "finish registry lock poisoned")?;
            let jobs = ids
                .iter()
                .filter_map(|id| {
                    registry
                        .jobs
                        .get(&(project.to_owned(), id.clone()))
                        .cloned()
                })
                .collect::<Vec<_>>();
            // Validate every identity before signalling any worker.
            if jobs.iter().any(|job| job.common_dir != common) {
                return Err("finish belongs to another clone".into());
            }
            for job in &jobs {
                job.expiring.store(true, Ordering::Release);
                job.cancelled.store(true, Ordering::Release);
            }
            jobs
        };
        for job in jobs {
            let mut drained = job
                .drained
                .0
                .lock()
                .map_err(|_| "finish drain lock poisoned")?;
            while !*drained {
                drained = job
                    .drained
                    .1
                    .wait(drained)
                    .map_err(|_| "finish drain lock poisoned")?;
            }
            if !job.cleanup_confirmed.load(Ordering::Acquire) {
                return Err(
                    "finish process cleanup unconfirmed; retained original execution output".into(),
                );
            }
        }
        Ok(())
    }
    /// The argument must come directly from core's actual durable expiry result.
    /// It is a derived cache update, never an input to core's expiry proof.
    pub(crate) fn expired(
        &self,
        project: &str,
        common: &Path,
        expiry: &towermd_core::project_session::FinishExpiryOutcome,
    ) -> Result<(), String> {
        for id in &expiry.evidence_ids {
            // Core already validated this exact actual expiry and complete Run.
            // Reconstruct only its diagnostic input, never an execution capability.
            let status: towermd_core::finish::FinishStatus =
                serde_json::from_value(expiry.actual.data["run"]["finish"]["jobs"][id].clone())
                    .map_err(|error| format!("expiry diagnostic original status: {error}"))?;
            let cleanup_unknown =
                self.recovered_running_cleanup_unknown(project, common, &status)?;
            if let Ok(job) = self.job(project, common, id) {
                if job.active.load(Ordering::Acquire) {
                    return Err("expired finish worker has not drained".into());
                }
                if !job.cleanup_confirmed.load(Ordering::Acquire) {
                    return Err(
                        "finish process cleanup unconfirmed; retained original execution output"
                            .into(),
                    );
                }
                *job.executed
                    .lock()
                    .map_err(|_| "finish execution lock poisoned")? = None;
                job.expiring.store(true, Ordering::Release);
                let mut current = job.view.lock().map_err(|_| "finish view lock poisoned")?;
                // Expiry ends this Run's authority. It does not rewrite an
                // already-recorded G3 result or its original actual receipt.
                current["run_state"] = json!("expired");
                current["expiry_actual"] = json!(expiry.actual);
                if !matches!(
                    current["job_state"].as_str(),
                    Some("completed" | "interrupted")
                ) {
                    current["job_state"] = json!("expired");
                    // actual is reserved for an accepted completed/interrupted
                    // terminal. Expiry remains the separate full audit above.
                    if let Some(fields) = current.as_object_mut() {
                        fields.remove("actual");
                    }
                }
                if cleanup_unknown {
                    note_cleanup_unknown(&mut current);
                }
                job.notify.notify_waiters();
            } else {
                // No local worker can be drained. Preserve the original sealed
                // input/diagnostic and explicitly leave old process cleanup unknown.
                let mut initial = json!({"evidence_id":id,"job_state":"expired","run_state":"expired","expiry_actual":expiry.actual,"input":status.input,"diagnostic":status.diagnostic});
                if cleanup_unknown {
                    note_cleanup_unknown(&mut initial);
                }
                self.attach(project, common, id, initial)?;
                self.job(project, common, id)?
                    .expiring
                    .store(true, Ordering::Release);
            }
        }
        Ok(())
    }
    /// Finite startup/project-registration discovery. The callback reads the
    /// verified Session, never a cached index, before scheduling any execution.
    pub(crate) fn recover_project(
        &self,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| "finish registry lock poisoned")?;
        if registry.stopping {
            return Err("finish service is stopping".into());
        }
        registry.workers.retain(|worker| !worker.is_finished());
        registry.workers.push(tokio::task::spawn_blocking(move || {
            if let Err(error) = work() {
                eprintln!("towermd finish recovery: {error}");
            }
        }));
        Ok(())
    }
    pub(crate) fn cancel_all(&self) -> Result<(), String> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| "finish registry lock poisoned")?;
        registry.stopping = true;
        for job in registry.jobs.values() {
            job.cancelled.store(true, Ordering::Release);
            job.notify.notify_waiters();
        }
        Ok(())
    }
    pub(crate) async fn shutdown(&self) -> Result<(), String> {
        self.cancel_all()?;
        let workers = std::mem::take(
            &mut self
                .registry
                .lock()
                .map_err(|_| "finish registry lock poisoned")?
                .workers,
        );
        for worker in workers {
            worker
                .await
                .map_err(|e| format!("finish worker drain failed: {e}"))?;
        }
        Ok(())
    }
}
fn terminal(view: &Value) -> bool {
    matches!(
        view["job_state"].as_str(),
        Some("completed" | "interrupted" | "uncertain" | "expired")
    )
}
pub(crate) async fn wait_job(
    notify: &Notify,
    read: impl Fn() -> Result<Value, String>,
    budget: Duration,
) -> Result<Value, String> {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let current = read()?;
        if terminal(&current) {
            return Ok(current);
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return read();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn finish_jobs_wait_is_bounded_and_reads_after_timeout() {
        let notify = Notify::new();
        let count = std::sync::atomic::AtomicUsize::new(0);
        let result = wait_job(
            &notify,
            || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"job_state":"running"}))
            },
            Duration::from_millis(10),
        )
        .await
        .unwrap();
        assert_eq!(result["job_state"], "running");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn finish_jobs_notification_during_read_cannot_be_lost() {
        let notify = Notify::new();
        let reads = std::sync::atomic::AtomicUsize::new(0);
        let result = wait_job(
            &notify,
            || {
                if reads.fetch_add(1, Ordering::SeqCst) == 0 {
                    notify.notify_waiters();
                    Ok(json!({"job_state":"running"}))
                } else {
                    Ok(json!({"job_state":"completed"}))
                }
            },
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(result["job_state"], "completed");
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn expiry_cancel_drains_only_exact_owned_job_and_prevents_same_job_resubmission() {
        // This tests registry lifetime only. These cached values grant no core
        // execution or expiry permission and do not stand in for actual reports.
        let jobs = Arc::new(FinishJobs::default());
        let common = PathBuf::from("registry-owned-common");
        for id in ["first", "second"] {
            jobs.attach("project", &common, id, json!({"job_state":"running"}))
                .unwrap();
        }
        let (ready, arrivals) = std::sync::mpsc::channel();
        let (cancelled, cancellation) = std::sync::mpsc::channel();
        let child_cleanup = Arc::new((Mutex::new(false), Condvar::new()));
        struct Cleanup(Arc<FinishJobs>, Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                *self.1 .0.lock().unwrap() = true;
                self.1 .1.notify_all();
                let _ = self.0.cancel_all();
            }
        }
        let _cleanup = Cleanup(jobs.clone(), child_cleanup.clone());
        let cleanup = child_cleanup.clone();
        let first_ready = ready.clone();
        jobs.submit(
            "project",
            &common,
            "first",
            Box::new(move |stop| {
                first_ready.send(()).unwrap();
                while !stop.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                cancelled.send(()).unwrap();
                let mut released = cleanup.0.lock().unwrap();
                while !*released {
                    released = cleanup.1.wait(released).unwrap();
                }
                Ok(json!({"job_state":"interrupted"}))
            }),
        )
        .unwrap();
        jobs.submit(
            "project",
            &common,
            "second",
            Box::new(move |stop| {
                ready.send(()).unwrap();
                while !stop.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(json!({"job_state":"interrupted"}))
            }),
        )
        .unwrap();
        for _ in 0..2 {
            arrivals.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        assert!(
            !jobs.has_due_owned("project", &common, u64::MAX).unwrap(),
            "cached status cannot register a private-ticket supervisor scope"
        );
        jobs.cancel_due_owned("project", &common, u64::MAX).unwrap();
        assert!(jobs.active("project", &common, "first").unwrap());
        assert!(jobs
            .cancel_and_drain("project", Path::new("foreign-common"), &["first".into()])
            .is_err());
        assert_eq!(
            jobs.owned_evidence_ids("project", &common).unwrap(),
            BTreeSet::from(["first".into(), "second".into()])
        );
        let draining_jobs = jobs.clone();
        let draining_common = common.clone();
        let drain = tokio::task::spawn_blocking(move || {
            draining_jobs.cancel_and_drain("project", &draining_common, &["first".into()])
        });
        cancellation.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            !drain.is_finished(),
            "a cancellation signal is not actual child cleanup/drain"
        );
        assert!(jobs.active("project", &common, "second").unwrap());
        assert_eq!(
            jobs.owned_evidence_ids("project", &common).unwrap(),
            BTreeSet::from(["second".into()])
        );
        *child_cleanup.0.lock().unwrap() = true;
        child_cleanup.1.notify_all();
        tokio::time::timeout(Duration::from_secs(15), drain)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!jobs.active("project", &common, "first").unwrap());
        assert!(jobs
            .submit(
                "project",
                &common,
                "first",
                Box::new(|_| panic!("expired job must not execute twice"))
            )
            .is_err());
        jobs.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exited_worker_without_cleanup_proof_cannot_drain_or_execute_again() {
        // This is a process-lifetime negative, not fake Task/Run authority. A
        // panic before retaining the actual output cannot certify any cleanup.
        let jobs = Arc::new(FinishJobs::default());
        let common = PathBuf::from("registry-unconfirmed-common");
        jobs.attach(
            "project",
            &common,
            "original",
            json!({"job_state":"running"}),
        )
        .unwrap();
        let executing = jobs.clone();
        let worker_common = common.clone();
        jobs.submit(
            "project",
            &common,
            "original",
            Box::new(move |_| {
                executing.execution_entered("project", &worker_common, "original")?;
                panic!("lost execution cleanup observation");
            }),
        )
        .unwrap();
        let job = jobs.job("project", &common, "original").unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(15);
        while job.active.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < until, "worker did not exit");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(
            *job.drained.0.lock().unwrap(),
            "actual Rust worker did exit"
        );
        assert!(!job.cleanup_confirmed.load(Ordering::Acquire));
        assert!(jobs
            .cancel_and_drain("project", &common, &["original".into()])
            .unwrap_err()
            .contains("cleanup unconfirmed"));
        jobs.attach(
            "project",
            &common,
            "original",
            json!({"job_state":"completed","cleanup_confirmed":true}),
        )
        .unwrap();
        assert!(
            !job.cleanup_confirmed.load(Ordering::Acquire),
            "cached JSON cannot mint cleanup proof"
        );
        assert!(jobs
            .submit(
                "project",
                &common,
                "original",
                Box::new(|_| panic!("must not execute again"))
            )
            .unwrap_err()
            .contains("cleanup unconfirmed"));
        jobs.shutdown().await.unwrap();
    }
}
