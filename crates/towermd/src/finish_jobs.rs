//! Process-local scheduling only. Core original events remain the only Job and
//! completion authority; no JSON index can mint an execution ticket.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{sync::Notify, task::JoinHandle};

pub(crate) const WAIT_LIMIT: Duration = Duration::from_secs(90);
pub(crate) const VERIFY_TIMEOUT: Duration = Duration::from_secs(300);
type Work = Box<dyn FnOnce(Arc<AtomicBool>) -> Result<Value, String> + Send>;
struct Job {
    common_dir: PathBuf,
    view: Mutex<Value>,
    executed: Mutex<Option<towermd_core::finish::ExecutedFinish>>,
    notify: Notify,
    active: AtomicBool,
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
impl FinishJobs {
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
        *retained = None;
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
        if registry.stopping {
            return Err(format!("finish request {id} uncertain: daemon is stopping"));
        }
        if job.active.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
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
            job.active.store(false, Ordering::Release);
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
        Some("completed" | "interrupted" | "uncertain")
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
}
