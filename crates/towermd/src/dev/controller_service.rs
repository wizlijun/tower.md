//! M3-D controller, M3-H cross review and WP-X raw output archive, run by the
//! host holding `main` after each scheduling pass. The controller and the
//! reviewer are read-only consultations of a harness; what they return is
//! recorded as their own process's events, never a person's. In-flight
//! consultations are this daemon's disposable state: a restart asks again.
use super::business::{now, ready};
use super::*;
use std::collections::BTreeSet;
use towermd_core::{hub::Event, hub_protocol::NewEvent, project_runtime::ProjectRuntime};
use towermd_devkit::{
    artifacts, controller::{self, Candidate, Outcome, Policy, Step, Triage}, controller_plan::{self, PlanEvent}, cross_review, dev_state,
    harness, proposal_model::Proposal, run_record, runner, task_model::VerifiedBaseline,
};
use towermd_core::question_model::CONTROLLER;

#[derive(Default)]
pub(super) struct Consults {
    /// Holds and proposals a harness is being asked about now.
    asking: Mutex<BTreeSet<String>>,
    /// FR-78: Arca as found when the project was first maintained.
    arca: Mutex<Option<Value>>,
}

fn append(runtime: &mut ProjectRuntime, input: NewEvent) -> Result<Event, String> {
    let receipt = runtime.append(input, now).map_err(|e| e.to_string())?;
    if receipt.stale { return Err("事件没有生效".into()); }
    Ok(receipt)
}

/// One pass under the runtime lock, after the scheduler's.
pub(super) fn pass(server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime, handle: &tokio::runtime::Handle,
    catalog: &Catalog, live: &BTreeSet<String>) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(error) = retry(server, project, runtime, handle, catalog) { errors.push(format!("总控重试：{error}")); }
    if let Err(error) = triage(server, project, runtime, handle, catalog) { errors.push(format!("总控：{error}")); }
    if let Err(error) = plans(server, project, runtime, handle, catalog) { errors.push(format!("总控计划：{error}")); }
    if let Err(error) = review(server, project, runtime, handle, catalog) { errors.push(format!("交叉评审：{error}")); }
    if let Err(error) = archive(server, project, runtime, catalog, live) { errors.push(format!("原始输出归档：{error}")); }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("；")) }
}

/// Starts asking about `key` unless it is already being asked.
fn claim(project: &Project, key: &str) -> Result<bool, String> {
    Ok(project.consults.asking.lock().map_err(|_| "总控状态不可用")?.insert(key.to_owned()))
}
fn unclaim(project: &Project, key: &str) {
    if let Ok(mut asking) = project.consults.asking.lock() { asking.remove(key); }
}

fn hand_over(server: &Server, runtime: &mut ProjectRuntime, hold: String, task: String, reason: String, level: Option<String>,
    agent: Option<String>, cost_usd: Option<f64>) -> Result<(), String> {
    let triage = Triage { hold, task, outcome: Outcome::HandedOver, answer: None,
        reason: Some(reason.chars().take(500).collect()), level, agent, cost_usd };
    append(runtime, controller::triage_event(&server.endpoint.host, &triage)?)?;
    Ok(())
}

/// FR-65: held tasks the policy lets the controller retry, then a new pass
/// so the scheduler relaunches them.
fn retry(server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime, handle: &tokio::runtime::Handle,
    catalog: &Catalog) -> Result<(), String> {
    let policy = Policy::from_config(catalog.config.section("controller"), catalog.config.section("runner"));
    let due = controller::retries(runtime.projection(), &policy)?;
    for retried in &due {
        append(runtime, controller::retry_event(&server.endpoint.host, retried)?)?;
    }
    if !due.is_empty() { super::schedules::kick(handle.clone(), Arc::clone(server), Arc::clone(project)); }
    Ok(())
}

fn triage(server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime, handle: &tokio::runtime::Handle,
    catalog: &Catalog) -> Result<(), String> {
    let policy = Policy::from_config(catalog.config.section("controller"), catalog.config.section("runner"));
    let delegation = super::delegations::view(runtime, project)?;
    for step in controller::plan(runtime.projection(), &policy, &delegation)? {
        match step {
            Step::HandOver { hold, task, reason, permit } => {
                hand_over(server, runtime, hold, task, reason, permit.map(|permit| permit.level), None, None)?;
            }
            Step::Consult(candidate) => {
                let hold = candidate.question.draft.id.clone();
                let Some(adapter) = harness::find(&policy.role.harness) else {
                    let reason = format!("总控的 harness 未知：{}", policy.role.harness);
                    hand_over(server, runtime, hold, candidate.task, reason, Some(candidate.permit.level), None, None)?;
                    continue;
                };
                if !claim(project, &hold)? { continue; }
                let brief = match controller::brief(runtime.projection(), &candidate) {
                    Ok(brief) => brief,
                    Err(error) => { unclaim(project, &hold); return Err(error); }
                };
                let consultation = consultation(server, project, runtime, &format!("Q-{hold}"),
                    Some(&candidate.question.binding.run), brief, controller::reply_schema(), &policy.role.model, policy.timeout);
                spawn(server, project, handle, adapter, consultation, hold, move |runtime, server, consulted| {
                    answered(runtime, server, &candidate, consulted)
                });
            }
        }
    }
    Ok(())
}

/// Recording the controller's reply, if the question is still its to answer.
fn answered(runtime: &mut ProjectRuntime, server: &Server, candidate: &Candidate, consulted: Result<runner::Consulted, String>)
    -> Result<(), String> {
    let hold = candidate.question.draft.id.clone();
    let projection = runtime.projection();
    if projection["Answer"].get(&hold).is_some() || controller::triages(projection)?.contains_key(&hold) { return Ok(()); }
    let level = Some(candidate.permit.level.clone());
    let (agent, cost_usd, verdict) = match consulted {
        Ok(consulted) => (Some(consulted.agent), consulted.cost_usd,
            controller::verdict(consulted.reply.as_ref(), consulted.error.as_deref())),
        Err(error) => (None, None, Err(error)),
    };
    match verdict {
        Ok((text, basis)) => {
            let answer = append(runtime, controller::answer_event(&server.endpoint.host, candidate, text, basis)?)?;
            let triage = Triage { hold, task: candidate.task.clone(), outcome: Outcome::Answered, answer: Some(answer.eid),
                reason: None, level, agent, cost_usd };
            append(runtime, controller::triage_event(&server.endpoint.host, &triage)?)?;
            Ok(())
        }
        Err(reason) => hand_over(server, runtime, hold, candidate.task.clone(), reason, level, agent, cost_usd),
    }
}

#[allow(clippy::too_many_arguments)]
fn consultation(server: &Server, project: &Project, runtime: &ProjectRuntime, id: &str, run: Option<&str>, brief: String,
    schema: Value, model: &Option<String>, timeout: Duration) -> runner::Consultation {
    // The delegate's worktree when it is on this host; the project otherwise.
    let cwd = run.and_then(|run| runtime.projection()["Run"].get(run))
        .filter(|run| run["binding"]["host"] == server.endpoint.host.as_str())
        .and_then(|run| run["binding"]["worktree"].as_str().map(PathBuf::from)).filter(|path| path.is_dir())
        .unwrap_or_else(|| project.root.clone());
    runner::Consultation { host: server.endpoint.host.clone(), id: id.into(), cwd, dir: runner::consult_dir(&project.common, id),
        brief, schema, model: model.clone(), timeout, env: Vec::new() }
}

/// Ask off the runtime lock, then record under it and kick the next pass.
fn spawn(server: &Arc<Server>, project: &Arc<Project>, handle: &tokio::runtime::Handle, adapter: &'static dyn harness::Adapter,
    consultation: runner::Consultation, key: String,
    record: impl FnOnce(&mut ProjectRuntime, &Server, Result<runner::Consulted, String>) -> Result<(), String> + Send + 'static) {
    let (server, project, handle) = (Arc::clone(server), Arc::clone(project), handle.clone());
    let program = super::schedules::program(adapter);
    std::thread::spawn(move || {
        let consulted = handle.block_on(async {
            let probed = runner::probe(adapter, program).await?;
            runner::consult(&probed, &consultation).await
        });
        if let Ok(mut slot) = project.runtime.lock() {
            if let Ok(runtime) = ready(&mut slot, &server, &project) {
                if let Err(error) = record(runtime, &server, consulted) { eprintln!("咨询结果待记录：{error}"); }
            }
        }
        unclaim(&project, &key);
        super::schedules::kick(handle, server, project);
    });
}

/// FR-67: a new baseline that left tasks to review gets a plan of its own,
/// made here without a harness (J4 step 4); a person's input is turned into
/// a plan by the controller harness. Either way it is only a preview.
fn plans(server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime, handle: &tokio::runtime::Handle,
    catalog: &Catalog) -> Result<(), String> {
    let host = server.endpoint.host.clone();
    let due = controller_plan::regeneration_due(runtime.projection())?;
    let waiting = controller_plan::consulting(runtime.projection())?;
    if !due && waiting.is_empty() { return Ok(()) }
    let baseline = super::tasks::baseline(project, runtime).ok();
    if let (true, Some(baseline)) = (due, &baseline) {
        if let Some((id, requested, previewed)) = controller_plan::regenerate(runtime.projection(), baseline)? {
            append(runtime, controller_plan::event(&host, CONTROLLER, &id, &requested)?)?;
            append(runtime, controller_plan::event(&host, CONTROLLER, &id, &previewed)?)?;
        }
    }
    if waiting.is_empty() { return Ok(()) }
    let policy = Policy::from_config(catalog.config.section("controller"), catalog.config.section("runner"));
    for plan in waiting {
        let Some(adapter) = harness::find(&policy.role.harness) else {
            let failed = PlanEvent::Failed { reason: format!("总控的 harness 未知：{}", policy.role.harness), agent: None, cost_usd: None };
            append(runtime, controller_plan::event(&host, CONTROLLER, &plan.id, &failed)?)?;
            continue;
        };
        let key = plan.id.clone();
        if !claim(project, &key)? { continue; }
        let brief = match controller_plan::brief(runtime.projection(), &plan, baseline.as_ref()) {
            Ok(brief) => brief,
            Err(error) => { unclaim(project, &key); return Err(error); }
        };
        let consultation = consultation(server, project, runtime, &format!("C-{key}"), None, brief, controller_plan::reply_schema(),
            &policy.role.model, policy.timeout);
        let baseline = baseline.clone();
        spawn(server, project, handle, adapter, consultation, key, move |runtime, server, consulted| {
            planned(runtime, server, &plan, baseline.as_ref(), consulted)
        });
    }
    Ok(())
}

/// The controller's plan as a preview, or why it has none, if the plan still waits for it.
fn planned(runtime: &mut ProjectRuntime, server: &Server, plan: &controller_plan::Plan, baseline: Option<&VerifiedBaseline>,
    consulted: Result<runner::Consulted, String>) -> Result<(), String> {
    let current = controller_plan::plans(runtime.projection())?.remove(&plan.id);
    if current.is_none_or(|current| current.status != controller_plan::Status::Consulting) { return Ok(()); }
    let change = match consulted {
        Ok(consulted) => controller_plan::preview(runtime.projection(), plan, baseline, consulted.reply.as_ref(),
            consulted.error.as_deref(), Some(consulted.agent), consulted.cost_usd),
        Err(error) => PlanEvent::Failed { reason: error.chars().take(controller_plan::TEXT_CHARS).collect(), agent: None, cost_usd: None },
    };
    append(runtime, controller_plan::event(&server.endpoint.host, CONTROLLER, &plan.id, &change)?)?;
    Ok(())
}

fn review(server: &Arc<Server>, project: &Arc<Project>, runtime: &mut ProjectRuntime, handle: &tokio::runtime::Handle,
    catalog: &Catalog) -> Result<(), String> {
    let Some(settings) = cross_review::from_config(catalog.config.section("runner"))? else { return Ok(()) };
    let adapter = harness::find(&settings.role.harness).ok_or("交叉评审的 harness 未知")?;
    for proposal in cross_review::plan(runtime.projection())? {
        let key = proposal.id.clone();
        if !claim(project, &key)? { continue; }
        let consultation = consultation(server, project, runtime, &format!("X-{key}"), None, cross_review::brief(&proposal),
            cross_review::reply_schema(), &settings.role.model, settings.timeout);
        let settings = settings.clone();
        spawn(server, project, handle, adapter, consultation, key, move |runtime, server, consulted| {
            reviewed(runtime, server, &proposal, &settings, consulted)
        });
    }
    Ok(())
}

fn reviewed(runtime: &mut ProjectRuntime, server: &Server, proposal: &Proposal, settings: &cross_review::Settings,
    consulted: Result<runner::Consulted, String>) -> Result<(), String> {
    if cross_review::reviews(runtime.projection())?.contains_key(&proposal.id) { return Ok(()); }
    let review = match consulted {
        Ok(consulted) => cross_review::record(proposal, settings, Some(consulted.agent), consulted.reply.as_ref(),
            consulted.error, consulted.cost_usd),
        Err(error) => cross_review::record(proposal, settings, None, None, Some(error), None),
    };
    append(runtime, cross_review::event(&server.endpoint.host, &review)?)?;
    Ok(())
}

/// FR-78: whether Arca is installed and how many files it has not synced,
/// found once per project open (`arca doctor`).
pub(super) fn arca_status(project: &Project) -> Value {
    let Ok(mut slot) = project.consults.arca.lock() else { return Value::Null };
    slot.get_or_insert_with(|| match artifacts::arca() {
        Some(program) => json!({"installed": true, "unsynced": artifacts::unsynced(&program, &project.root)}),
        None => json!({"installed": false, "unsynced": null}),
    }).clone()
}

/// For `dev queue` and the App's status bar: where raw output goes, and how
/// many attempts are on this machine only.
pub(super) fn status(project: &Project, runtime: &ProjectRuntime) -> Result<Value, String> {
    let mut status = arca_status(project);
    status["local_only"] = json!(artifacts::local_only(runtime.projection())?);
    Ok(status)
}

/// Every ended attempt of a dispatched run on this host gets one archive record.
fn archive(server: &Server, project: &Project, runtime: &mut ProjectRuntime, catalog: &Catalog, live: &BTreeSet<String>)
    -> Result<(), String> {
    let done = artifacts::archives(runtime.projection())?;
    let runs = dev_state::tasks(runtime.projection())?.runs;
    let mut due = Vec::new();
    for (id, record) in run_record::records(runtime.projection())? {
        let Some(run) = runs.get(&id) else { continue };
        if run.dispatch.is_none() || run.binding.host != server.endpoint.host || live.contains(&id) { continue; }
        let ended = if record.running.is_some() { record.starts.saturating_sub(1) } else { record.starts };
        due.extend((1..=ended).filter(|attempt| !done.contains_key(&format!("{id}/{attempt}")))
            .map(|attempt| (id.clone(), record.task.clone(), attempt)));
    }
    if due.is_empty() { return Ok(()); }
    arca_status(project);
    let program = artifacts::arca();
    for (run, task, attempt) in due {
        let archived = artifacts::archive(&project.root, catalog.config.docs_dir(), &project.common, program.as_deref(),
            &run, &task, attempt);
        append(runtime, artifacts::event(&server.endpoint.host, &archived)?)?;
    }
    Ok(())
}
