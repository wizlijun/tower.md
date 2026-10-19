//! Task finish transport. All authorization, execution capabilities and durable
//! result recording remain in the core; report text is only a caller hint.
use super::*;
use crate::finish_jobs::{VERIFY_TIMEOUT, WAIT_LIMIT};
use towermd_core::finish::{
    self, FinishOutcome, FinishStart, FinishStatus, FinishTicket, FinishView, JobPhase,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishRequest {
    pub directory: PathBuf,
    pub run: Option<String>,
    pub wait: Option<String>,
    pub report: Option<String>,
}
const MAX_REPORT_BYTES: usize = 1024 * 1024;
fn request_id(evidence_id: &str) -> Result<&str, String> {
    let id = evidence_id
        .strip_prefix("E-")
        .ok_or("finish requires an Evidence ID (E-<UUIDv7>)")?;
    towermd_core::recovery_contract::recorded::request_event_eid(id, "finish.started")?;
    Ok(id)
}
fn view(value: FinishView) -> Result<Value, String> {
    let expired = value.run_state == "expired"
        && !matches!(
            value.status.phase,
            JobPhase::Completed | JobPhase::Interrupted
        );
    let mut result = json!({"evidence_id":value.status.input.evidence_id,"job_state":value.status.phase,"run_state":value.run_state,"input":value.status.input,"diagnostic":value.status.diagnostic});
    if expired {
        result["job_state"] = json!("expired");
    }
    if let Some(evidence) = value.evidence {
        result["result"] = serde_json::to_value(&evidence.result).map_err(|e| e.to_string())?;
        result["evidence"] = serde_json::to_value(evidence).map_err(|e| e.to_string())?;
    }
    if let Some(report) = value.report {
        result["report"] = serde_json::to_value(report).map_err(|e| e.to_string())?;
    }
    if let Some(actual) = value.actual {
        result["actual"] = serde_json::to_value(actual).map_err(|e| e.to_string())?;
    }
    // Only the core's same-history proof supplies an expiry audit. Job cache
    // presence cannot create or remove this receipt from a full reader result.
    if let Some(actual) = value.expiry_actual {
        result["expiry_actual"] = serde_json::to_value(actual).map_err(|e| e.to_string())?;
    }
    Ok(result)
}
fn view_with_cleanup_diagnostic(
    jobs: &crate::finish_jobs::FinishJobs,
    project: &str,
    common: &Path,
    value: FinishView,
) -> Result<Value, String> {
    let status = value.status.clone();
    let mut result = view(value)?;
    jobs.annotate_expired_cleanup(project, common, &status, &mut result)?;
    Ok(result)
}
fn outcome(value: FinishOutcome) -> Result<Value, String> {
    let report = if value.evidence.is_some() {
        Some(
            serde_json::from_value(value.actual.data["report"].clone())
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let run_state = value.actual.data["run"]["state"]
        .as_str()
        .ok_or("finish actual receipt lacks Run state")?
        .to_owned();
    view(FinishView {
        status: value.status,
        run_state,
        actual: Some(value.actual),
        expiry_actual: None,
        evidence: value.evidence,
        report,
    })
}
fn run_catalog(accepted: &Catalog, status: &FinishStatus) -> Result<Catalog, String> {
    let actual = Catalog::load(&status.input.root)?;
    if actual.root != status.input.root
        || actual.common_dir != accepted.common_dir
        || actual.config.project_id() != accepted.config.project_id()
        || towermd_core::recovery_outbox::RecoveryContext::from_catalog(&actual)?
            != towermd_core::recovery_outbox::RecoveryContext::from_catalog(accepted)?
    {
        return Err("finish actual worktree project/clone/config changed".into());
    }
    Ok(actual)
}
// A retry is allowed only after reading an actually changed authority, with
// the same project/clone/config. The normal producer still proves that source
// and every business gate. Stable refusals never spin or become success.
fn at_current_authority<T>(
    accepted: &Catalog,
    id: &str,
    mut operation: impl FnMut(&Catalog) -> Result<T, String>,
) -> Result<T, String> {
    let mut current = current_scan_catalog(accepted)
        .map_err(|error| format!("finish request {id} uncertain: {error}"))?;
    for attempt in 0..3 {
        match operation(&current) {
            Ok(result) => return Ok(result),
            Err(error) => {
                let fresh = match current_scan_catalog(&current) {
                    Ok(fresh) => fresh,
                    Err(_) => return Err(format!("finish request {id} uncertain: {error}")),
                };
                if fresh.revision == current.revision {
                    return Err(error);
                }
                if attempt == 2 {
                    return Err(format!(
                        "finish request {id} uncertain: authority kept changing; {error}"
                    ));
                }
                current = fresh;
            }
        }
    }
    unreachable!("bounded authority attempts return")
}
fn record_retained(
    jobs: &crate::finish_jobs::FinishJobs,
    project: &Project,
    accepted: &Catalog,
    status: &FinishStatus,
    directory: &std::path::Path,
    clock: &dyn Fn() -> Result<u64, String>,
) -> Result<Option<Value>, String> {
    let id = &status.input.evidence_id;
    if !jobs.has_executed(accepted.config.project_id(), &accepted.common_dir, id)? {
        return Ok(None);
    }
    at_current_authority(accepted, id, |catalog| {
        record_retained_at(jobs, project, catalog, status, directory, clock)
    })
}
fn record_retained_at(
    jobs: &crate::finish_jobs::FinishJobs,
    project: &Project,
    catalog: &Catalog,
    status: &FinishStatus,
    directory: &std::path::Path,
    clock: &dyn Fn() -> Result<u64, String>,
) -> Result<Option<Value>, String> {
    let catalog = run_catalog(catalog, status)?;
    let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(
        directory,
        towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog)?,
        project.outbox_uncertainty.clone(),
    )?;
    jobs.record_executed(
        catalog.config.project_id(),
        &catalog.common_dir,
        &status.input.evidence_id,
        |executed| {
            outcome(ProjectSession::record_borrowed_finish_shared(
                &project.hub,
                &catalog,
                executed,
                &mut outbox,
                clock,
            )?)
        },
    )
}

fn wait_context(catalog: &Catalog, run: Option<&str>, status: &FinishStatus) -> Result<(), String> {
    if status.input.root != catalog.root
        || catalog.branch != format!("t/{}", status.input.task_id)
        || run.is_some_and(|run| run != status.input.run_id)
    {
        return Err("finish wait requires its original Task worktree/branch and Run".into());
    }
    Ok(())
}
fn schedule_work(
    jobs: Arc<crate::finish_jobs::FinishJobs>,
    project: Arc<Project>,
    accepted: Arc<Catalog>,
    host: String,
    clock: Arc<dyn Fn() -> Result<u64, String> + Send + Sync>,
    directory: PathBuf,
    ticket: FinishTicket,
) -> Result<(), String> {
    let status = ticket.status().clone();
    let id = status.input.evidence_id.clone();
    let initial = json!({"evidence_id":id,"job_state":"queued","run_state":"verifying","input":status.input,"diagnostic":null});
    jobs.attach(
        accepted.config.project_id(),
        &accepted.common_dir,
        &id,
        initial,
    )?;
    let project_id = accepted.config.project_id().to_owned();
    let common = accepted.common_dir.clone();
    let recording_jobs = jobs.clone();
    jobs.register_owned_ticket(accepted.config.project_id(), &accepted.common_dir, &ticket)?;
    jobs.submit(
        &project_id,
        &common,
        &id,
        Box::new(move |cancelled| {
            let result = (|| {
                let execution = {
                    let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
                    let catalog = run_catalog(&accepted, &status)?;
                    let mut outbox =
                        towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(
                            &directory,
                            towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog)?,
                            project.outbox_uncertainty.clone(),
                        )?;
                    if cancelled.load(Ordering::Acquire) {
                        return outcome(ProjectSession::interrupt_finish_shared(
                            &project.hub,
                            &catalog,
                            &host,
                            &status.request_id,
                            "daemon stopped before verification",
                            &mut outbox,
                            || clock(),
                        )?);
                    }
                    drop(outbox);
                    let mut original_ticket = Some(ticket);
                    at_current_authority(&catalog, &status.input.evidence_id, |catalog| {
                        let catalog = run_catalog(catalog, &status)?;
                        let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(
                            &directory,
                            towermd_core::recovery_outbox::RecoveryContext::from_catalog(&catalog)?,
                            project.outbox_uncertainty.clone(),
                        )?;
                        let ticket = if let Some(ticket) = original_ticket.take() {
                            ticket
                        } else {
                            // If Running was actually accepted but its capability
                            // was not returned, it is an orphan, never executable.
                            match ProjectSession::begin_finish_shared(&project.hub, &catalog,
                                &host, &status.request_id, Some(&status.input.run_id),
                                &mut outbox, || clock())? {
                                FinishStart::Queued(ticket) => ticket,
                                FinishStart::Existing(actual) => return Err(format!(
                                    "finish request {} uncertain: actual {:?} requires original receipt coordination",
                                    status.request_id, actual.phase)),
                            }
                        };
                        ProjectSession::launch_finish_shared(&project.hub, &catalog, ticket,
                            &mut outbox, || clock())
                    })?
                };
                // No producer, outbox, Session or registry lock spans the real owned
                // worktree execution/process cleanup.
                recording_jobs.execution_entered(accepted.config.project_id(), &accepted.common_dir,
                    &status.input.evidence_id)?;
                let executed = finish::execute(execution, VERIFY_TIMEOUT, cancelled.clone());
                // Install the real output before another fallible source read.
                // A source race can defer recording but cannot erase execution.
                recording_jobs.retain_executed(
                    accepted.config.project_id(), &accepted.common_dir,
                    &status.input.evidence_id, executed,
                )?;
                #[cfg(test)]
                tests::finish_boundary(&accepted.common_dir, "executed_retained");
                let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
                record_retained(&recording_jobs, &project, &accepted, &status, &directory,
                    &*clock)?.ok_or_else(|| "finish actual execution output vanished".to_owned())
            })();
            if result.is_err() {
                project.finish_recovery.store(true, Ordering::Release);
            }
            result
        }),
    )
}
// Rebuildable scheduling state is released even if a recovery callback panics.
// The core capability is minted before cancellation; it is consumed only
// after the exact owned workers have exited and all source inputs are fresh.
fn maintain_project_finishes(
    jobs: &crate::finish_jobs::FinishJobs,
    project: &Project,
    catalog: &Catalog,
    host: &str,
    directory: &Path,
    authority_unavailable: Option<&str>,
    clock: &dyn Fn() -> Result<u64, String>,
) -> Result<std::collections::BTreeSet<String>, String> {
    let maintained = {
        let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
        let owned = jobs.owned_evidence_ids(&project.project_id, &project.common_dir)?;
        let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(
            directory,
            towermd_core::recovery_outbox::RecoveryContext::from_catalog(catalog)?,
            project.outbox_uncertainty.clone(),
        )?;
        ProjectSession::maintain_finishes_shared(
            &project.hub,
            catalog,
            host,
            &owned,
            authority_unavailable,
            &mut outbox,
            clock,
        )?
    };
    let mut expired_ids = std::collections::BTreeSet::new();
    for expired in maintained.expired {
        jobs.cancel_and_drain(
            &project.project_id,
            &project.common_dir,
            &expired.evidence_ids,
        )?;
        jobs.expired(&project.project_id, &project.common_dir, &expired)?;
        expired_ids.extend(expired.evidence_ids);
    }
    for expiry in maintained.expirations {
        // No business lock survives into child cancellation/process-group drain.
        jobs.cancel_and_drain(
            &project.project_id,
            &project.common_dir,
            expiry.evidence_ids(),
        )?;
        let fresh = Catalog::load(&catalog.root)?;
        if fresh.common_dir != project.common_dir || fresh.config.project_id() != project.project_id
        {
            return Err("finish expiry project identity changed".into());
        }
        let actual = {
            let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
            let mut outbox = towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(
                directory,
                towermd_core::recovery_outbox::RecoveryContext::from_catalog(&fresh)?,
                project.outbox_uncertainty.clone(),
            )?;
            ProjectSession::expire_finish_shared(
                &project.hub,
                &fresh,
                host,
                expiry,
                &mut outbox,
                clock,
            )?
        };
        if let Some(expired) = actual {
            jobs.expired(&project.project_id, &project.common_dir, &expired)?;
            expired_ids.extend(expired.evidence_ids);
        }
        // None is canonical/terminal priority, never permission to throw away
        // the original sealed output. Normal original-receipt recovery follows.
    }
    Ok(expired_ids)
}
fn finish_maintenance_due(last: Option<ClockReading>, now: ClockReading) -> bool {
    last.is_none_or(|last| {
        now.mono_ms.saturating_sub(last.mono_ms) >= 60_000
            && now.unix_ms.saturating_sub(last.unix_ms) >= 60_000
    })
}
struct FinishRecoveryFlight {
    project: Arc<Project>,
    mono: Arc<dyn Fn() -> u64 + Send + Sync>,
    succeeded: bool,
    maintenance_at: ClockReading,
}
impl Drop for FinishRecoveryFlight {
    fn drop(&mut self) {
        if self.succeeded {
            if let Ok(mut last) = self.project.finish_maintenance_at.lock() {
                *last = Some(self.maintenance_at);
            }
            self.project.finish_retry_after.store(0, Ordering::Release);
        } else {
            self.project
                .finish_retry_after
                .store((self.mono)().saturating_add(5_000), Ordering::Release);
            self.project.finish_recovery.store(true, Ordering::Release);
        }
        self.project
            .finish_recovery_active
            .store(false, Ordering::Release);
    }
}
impl Server {
    fn schedule_finish(
        &self,
        project: Arc<Project>,
        accepted: Arc<Catalog>,
        ticket: FinishTicket,
    ) -> Result<(), String> {
        let directory = self
            .runtime
            .join("recovery-outbox")
            .join(accepted.config.project_id());
        schedule_work(
            self.finish_jobs.clone(),
            project,
            accepted,
            self.endpoint.host.clone(),
            self.clock.clone(),
            directory,
            ticket,
        )
    }
    pub(super) fn recover_finish_project(
        &self,
        project: Arc<Project>,
        catalog: &Catalog,
    ) -> Result<(), String> {
        if !project.finish_registered.swap(true, Ordering::AcqRel) {
            let has_jobs = project
                .hub
                .lock()
                .map_err(|_| "项目 hub 锁损坏")?
                .has_finish_jobs();
            if matches!(has_jobs, Ok(false)) {
                project.finish_recovery.store(false, Ordering::Release);
            }
        }
        if catalog.config.project_id() != project.project_id
            || catalog.common_dir != project.common_dir
        {
            return Err("finish recovery project identity changed".into());
        }
        self.schedule_finish_recovery(project)
    }
    pub(super) fn schedule_finish_recovery(&self, project: Arc<Project>) -> Result<(), String> {
        // The ticker calls this without Git, outbox I/O or a producer lock.
        if tokio::runtime::Handle::try_current().is_err() || *self.shutdown.borrow() {
            return Ok(());
        }
        let now = ClockReading {
            mono_ms: (self.mono_clock)(),
            unix_ms: (self.clock)()?,
        };
        let owned_due = self.finish_jobs.has_due_owned(
            &project.project_id,
            &project.common_dir,
            now.unix_ms,
        )?;
        if now.mono_ms < project.finish_retry_after.load(Ordering::Acquire) && !owned_due {
            return Ok(());
        }
        // This private-ticket hint schedules only process supervision. A broken
        // Session projection must not strand an actually owned overdue child.
        let maintenance = owned_due
            || (finish_maintenance_due(
                *project
                    .finish_maintenance_at
                    .lock()
                    .map_err(|_| "finish maintenance schedule lock poisoned")?,
                now,
            ) && project
                .hub
                .lock()
                .map_err(|_| "项目 hub 锁损坏")?
                .has_finish_maintenance(&self.endpoint.host, now.unix_ms)?);
        let authority_unavailable = self.main_refresh.last_fetch_error(&project.common_dir);
        if (!project.finish_recovery.load(Ordering::Acquire) && !maintenance)
            || project.finish_recovery_active.swap(true, Ordering::AcqRel)
        {
            return Ok(());
        }
        project.finish_recovery.store(false, Ordering::Release);
        // Preserve the source error for the worker; even this early failure
        // must still reach exact overdue owned-process cleanup outside locks.
        let root = project
            .root
            .lock()
            .map(|root| root.clone())
            .map_err(|_| "项目路径锁损坏".to_owned());
        let project_id = project.project_id.clone();
        let directory = self.runtime.join("recovery-outbox").join(&project_id);
        let jobs = self.finish_jobs.clone();
        let clock = self.clock.clone();
        let host = self.endpoint.host.clone();
        let mono = self.mono_clock.clone();
        let worker = project.clone();
        let result=self.finish_jobs.recover_project(move|| {
            let mut flight = FinishRecoveryFlight { project: worker.clone(), mono, succeeded: false, maintenance_at: now };
            let result=(|| {
                let root=root.as_ref().map_err(|error|error.clone())?;
                let authority_unavailable=authority_unavailable.as_ref().map_err(|error|error.clone())?;
                let catalog=Arc::new(Catalog::load(root)?);
                if catalog.common_dir!=worker.common_dir || catalog.config.project_id()!=project_id {return Err("finish recovery project identity changed".into());}
                let expired_ids=at_current_authority(&catalog,&project_id,|current| {
                    maintain_project_finishes(&jobs,&worker,current,&host,&directory,authority_unavailable.as_deref(),&*clock)
                })?;
                let statuses=at_current_authority(&catalog,&project_id,|current| {
                    let _producer=worker.producer.lock().map_err(|_|"项目写入队列锁损坏")?;
                    let outbox=towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(&directory,towermd_core::recovery_outbox::RecoveryContext::from_catalog(current)?,worker.outbox_uncertainty.clone())?;
                    ProjectSession::finish_jobs_shared(&worker.hub,current,&outbox.read_inputs()?,&host,clock()?)
                })?;
                let mut failures=Vec::new();
                for status in statuses {
                    if expired_ids.contains(&status.input.evidence_id) { continue; }
                    if jobs.active(&project_id,&catalog.common_dir,&status.input.evidence_id)? {continue;}
                    #[cfg(test)]
                    tests::finish_boundary(&catalog.common_dir, "recovery_before_producer");
                    let coordinated=at_current_authority(&catalog,&status.input.evidence_id,|catalog| {
                        let _producer=worker.producer.lock().map_err(|_|"项目写入队列锁损坏")?;
                        // Recheck after acquiring the producer: an explicit
                        // request may now own the actual execution output.
                        if jobs.active(&project_id,&catalog.common_dir,&status.input.evidence_id)? {
                            return Ok(None);
                        }
                        if jobs.has_executed(&project_id,&catalog.common_dir,&status.input.evidence_id)? {
                            if let Some(recorded)=record_retained_at(&jobs,&worker,catalog,&status,&directory,&*clock)? {
                                jobs.attach(&project_id,&catalog.common_dir,&status.input.evidence_id,recorded)?;
                                return Ok(None);
                            }
                            return Err("finish retained output vanished during recovery".to_owned());
                        }
                        let mut outbox=towermd_core::recovery_outbox::RecoveryOutbox::open_with_uncertainty(&directory,towermd_core::recovery_outbox::RecoveryContext::from_catalog(catalog)?,worker.outbox_uncertainty.clone())?;
                        let Some(current)=ProjectSession::finish_view_shared(&worker.hub,catalog,&host,&status.request_id,&mut outbox,||clock())? else {return Err("finish discovered original vanished".to_owned())};
                        let id=current.status.input.evidence_id.clone();
                        if current.run_state == "expired" || matches!(current.status.phase,JobPhase::Completed|JobPhase::Interrupted) {
                            jobs.attach(&project_id,&catalog.common_dir,&id,view_with_cleanup_diagnostic(&jobs,&project_id,&catalog.common_dir,current)?)?;return Ok(None);
                        }
                        let actual=run_catalog(catalog,&status)?;
                        if !jobs.active(&project_id,&actual.common_dir,&id)? {
                            match current.status.phase {
                                JobPhase::Queued=>{
                                    worker.hub.lock().map_err(|_|"项目 hub 锁损坏")?.renew_binding(&actual,&host,Some(&status.input.run_id),clock()?)?;
                                    if let FinishStart::Queued(queued)=ProjectSession::begin_finish_shared(&worker.hub,&actual,&host,&status.request_id,Some(&status.input.run_id),&mut outbox,||clock())? {
                                        // The actual recovered ticket is checked again at launch.
                                        // schedule_work installs its process-local queued view.
                                        return Ok(Some(queued));
                                    }
                                }
                                JobPhase::Running=>{
                                    let interrupted=ProjectSession::interrupt_finish_shared(&worker.hub,&actual,&host,&status.request_id,"previous daemon stopped without a durable completed result",&mut outbox,||clock())?;
                                    // A successful producer already confirmed and synchronized
                                    // this terminal receipt; subsequent reads remain full proofs.
                                    jobs.attach(&project_id,&actual.common_dir,&id,outcome(interrupted)?)?;
                                    return Ok(None);
                                }
                                _=>{},
                            }
                        }
                        let refreshed=ProjectSession::finish_view_shared(&worker.hub,&actual,&host,&status.request_id,&mut outbox,||clock())?.ok_or("finish recovered original vanished")?;
                        jobs.attach(&project_id,&actual.common_dir,&id,view_with_cleanup_diagnostic(&jobs,&project_id,&actual.common_dir,refreshed)?)?;
                        Ok::<_,String>(None)
                    });
                    // Only a successfully proved queued ticket leaves the
                    // producer. Source retries never execute or reuse a Running
                    // job, and scheduling occurs once after coordination.
                    let result=match coordinated {
                        Ok(Some(ticket))=>schedule_work(jobs.clone(),worker.clone(),catalog.clone(),host.clone(),clock.clone(),directory.clone(),ticket),
                        Ok(None)=>Ok(()),
                        Err(error)=>Err(error),
                    };
                    if let Err(error)=result {failures.push(format!("{}: {error}",status.input.evidence_id));}
                }
                if failures.is_empty(){Ok(())}else{Err(failures.join("; "))}
            })();
            // Normal proof/actual-main priority was attempted first. Even if
            // Catalog or Session proof failed before core could mint an expiry,
            // the original private ticket still bounds our own child lifetime.
            // This is lock-free process cleanup, never a substitute Ready fact.
            let cleanup = jobs.cancel_due_owned(&project_id, &worker.common_dir, clock().unwrap_or(now.unix_ms).max(now.unix_ms));
            let result = match (result, cleanup) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
                (Err(error), Err(cleanup)) => Err(format!("{error}; owned deadline cleanup: {cleanup}")),
            };
            flight.succeeded = result.is_ok();
            result
        });
        if result.is_err() {
            project
                .finish_retry_after
                .store((self.mono_clock)().saturating_add(5_000), Ordering::Release);
            project.finish_recovery.store(true, Ordering::Release);
            project
                .finish_recovery_active
                .store(false, Ordering::Release);
        }
        result
    }
    fn prepare_finish(
        &self,
        request: FinishRequest,
    ) -> Result<(String, String, PathBuf, Value), String> {
        if *self.shutdown.borrow() {
            return Err("daemon is stopping; retry the original finish ID".into());
        }
        if request.wait.is_some() && request.report.is_some() {
            return Err("finish --wait and --report are mutually exclusive".into());
        }
        if request
            .report
            .as_ref()
            .is_some_and(|report| report.len() > MAX_REPORT_BYTES)
        {
            return Err("finish report exceeds 1 MiB".into());
        }
        let accepted = Arc::new(self.catalog(&request.directory)?);
        // A clone does not copy the actual Hub directory. Check an existing
        // project binding first, without opening/creating a substitute Hub.
        if let Some(bound) = self.registered_project(accepted.config.project_id())? {
            if bound.common_dir != accepted.common_dir {
                return Err("finish project ID is already bound to another clone".into());
            }
        }
        if !probe_store(&accepted.common_dir.join("towermd"))? {
            return Err("hub journal 不存在；须先恢复，finish 不创建替代历史".into());
        }
        let project = self.project(&accepted)?;
        let _producer = project.producer.lock().map_err(|_| "项目写入队列锁损坏")?;
        let original_request_id = match request.wait.as_deref() {
            Some(id) => request_id(id)?.to_owned(),
            None => Uuid::now_v7().to_string(),
        };
        let directory = self
            .runtime
            .join("recovery-outbox")
            .join(accepted.config.project_id());
        let (id, result, ticket) =
            at_current_authority(&accepted, &original_request_id, |catalog| {
                let mut outbox = self.task_outbox(catalog)?;
                let start = if let Some(id) = &request.wait {
                    let request_id = request_id(id)?;
                    // Original receipt coordination precedes any renewal/current gate.
                    let existing = ProjectSession::finish_view_shared(
                        &project.hub,
                        catalog,
                        &self.endpoint.host,
                        request_id,
                        &mut outbox,
                        || (self.clock)(),
                    )?
                    .ok_or("finish Evidence ID not found")?;
                    wait_context(catalog, request.run.as_deref(), &existing.status)?;
                    if existing.run_state == "expired" {
                        let id = existing.status.input.evidence_id.clone();
                        let result = view_with_cleanup_diagnostic(
                            &self.finish_jobs,
                            catalog.config.project_id(),
                            &catalog.common_dir,
                            existing,
                        )?;
                        self.finish_jobs.attach(
                            catalog.config.project_id(),
                            &catalog.common_dir,
                            &id,
                            result.clone(),
                        )?;
                        return Ok((id, result, None));
                    }
                    FinishStart::Existing(existing.status)
                } else {
                    let inputs = outbox.read_inputs()?;
                    Self::reconcile_task_intents(
                        &project.hub,
                        catalog,
                        &inputs,
                        (self.clock)()?,
                        &mut outbox,
                    )?;
                    self.renew_read_hub(&project.hub, catalog, request.run.as_deref())?;
                    ProjectSession::begin_current_finish_shared(
                        &project.hub,
                        catalog,
                        &self.endpoint.host,
                        &original_request_id,
                        request.run.as_deref(),
                        &mut outbox,
                        || (self.clock)(),
                    )?
                };
                let (status, ticket) = match start {
                    FinishStart::Queued(ticket) => (ticket.status().clone(), Some(ticket)),
                    FinishStart::Existing(status) => (status, None),
                };
                let id = status.input.evidence_id.clone();
                let active = self.finish_jobs.active(
                    catalog.config.project_id(),
                    &catalog.common_dir,
                    &id,
                )?;
                let mut ticket = ticket;
                if !active && status.phase == JobPhase::Queued && ticket.is_none() {
                    self.renew_read_hub(&project.hub, catalog, Some(&status.input.run_id))?;
                    if let FinishStart::Queued(resumed) = ProjectSession::begin_finish_shared(
                        &project.hub,
                        catalog,
                        &self.endpoint.host,
                        &status.request_id,
                        Some(&status.input.run_id),
                        &mut outbox,
                        || (self.clock)(),
                    )? {
                        ticket = Some(resumed);
                    }
                }
                let retained = !active
                    && self.finish_jobs.has_executed(
                        catalog.config.project_id(),
                        &catalog.common_dir,
                        &id,
                    )?;
                if retained {
                    drop(outbox);
                    record_retained(
                        &self.finish_jobs,
                        &project,
                        catalog,
                        &status,
                        &directory,
                        &*self.clock,
                    )?
                    .ok_or("finish retained output vanished")?;
                    outbox = self.task_outbox(catalog)?;
                }
                if !active && !retained && status.phase == JobPhase::Running {
                    // No worker from this daemon owns the already-started job. The
                    // original completion was coordinated above first; no stale PID or
                    // cached report is treated as execution/termination evidence.
                    ProjectSession::interrupt_finish_shared(
                        &project.hub,
                        catalog,
                        &self.endpoint.host,
                        &status.request_id,
                        "previous daemon stopped without a durable completed result",
                        &mut outbox,
                        || (self.clock)(),
                    )?;
                }
                // A new started receipt was already confirmed and synchronized.
                // Return that past admission directly; only later reads/launch
                // determine current state and authority. Old/active/wait paths
                // always retain the full original-request reader below.
                let admission = if !active && request.wait.is_none() {
                    ticket
                        .as_ref()
                        .and_then(|ticket| ticket.admission_view())
                        .cloned()
                } else {
                    None
                };
                let current = match admission {
                    Some(current) => current,
                    None => ProjectSession::finish_view_shared(
                        &project.hub,
                        catalog,
                        &self.endpoint.host,
                        &status.request_id,
                        &mut outbox,
                        || (self.clock)(),
                    )?
                    .ok_or("finish original request disappeared")?,
                };
                let mut result = view_with_cleanup_diagnostic(
                    &self.finish_jobs,
                    catalog.config.project_id(),
                    &catalog.common_dir,
                    current,
                )?;
                if request.report.is_some() {
                    result["report_hint_received"] = json!(true);
                    result["report_hint_is_evidence"] = json!(false);
                }
                self.finish_jobs.attach(
                    catalog.config.project_id(),
                    &catalog.common_dir,
                    &id,
                    result.clone(),
                )?;
                drop(outbox);
                Ok((id, result, if active { None } else { ticket }))
            })
            .inspect_err(|_| {
                project.finish_recovery.store(true, Ordering::Release);
            })?;
        drop(_producer);
        if let Some(ticket) = ticket {
            self.schedule_finish(project, accepted.clone(), ticket)?;
        }
        Ok((
            accepted.config.project_id().to_owned(),
            id,
            accepted.common_dir.clone(),
            result,
        ))
    }
}
pub(super) async fn finish(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    Json(request): Json<FinishRequest>,
) -> (StatusCode, Json<Value>) {
    if !authorized(&headers, &server) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"守护进程实例已改变"})),
        );
    }
    let wait = request.wait.is_some();
    let operation = server.clone();
    let prepared = tokio::task::spawn_blocking(move || operation.prepare_finish(request)).await;
    let result = match prepared {
        Ok(Ok((project, id, common, initial))) => {
            if wait {
                server
                    .finish_jobs
                    .wait(&project, &common, &id, WAIT_LIMIT)
                    .await
            } else {
                Ok(initial)
            }
        }
        Ok(Err(error)) => Err(error),
        Err(error) => Err(format!("finish request uncertain: {error}")),
    };
    match result {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => {
            let status = if error.contains("uncertain") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(json!({"error":error})))
        }
    }
}

#[cfg(test)]
#[path = "finish_routes_tests.rs"]
pub(super) mod tests;
