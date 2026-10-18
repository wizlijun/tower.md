//! Durable Task Review producers and a source-bound, read-only App view.
//! Slow Git/proof work stays outside the Session lock. This module does not
//! authenticate an App; the daemon must obtain the human actor from its AppGate.
use super::{ProjectSession, SnapshotSessionId};
use crate::{
    baseline_current::{current_baseline, CurrentBaseline},
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, Fence, HubError, NewEvent},
    human_intervention::{HumanCommand, ReviewBinding, ReviewFact, ReviewGate, ReviewSubject},
    recovery_contract::{
        self,
        recorded::{
            request_event_eid, RequestReceipt, ReviewBodyView, ReviewHoldView, ReviewReplyView,
            TaskReviewAction, TaskReviewRecorded, TaskReviewResult,
        },
    },
    recovery_outbox::{self, RecoveryInputs, RecoveryOutbox},
    request_ledger::{
        decode_review_request, decode_task_review, InterventionRequest, RequestIdentity,
        ReviewRequestResult, ReviewRequestedRecorded, ReviewValidity, TaskReviewOutcome,
        TaskReviewRequest, TaskReviewRequestOutcome, TaskReviewView,
    },
    tasks::{Run, Task},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::Duration,
};

struct Context {
    task: Task,
    run: Run,
    session: SnapshotSessionId,
    private: Option<String>,
    plan: Option<ReviewSubject>,
}
struct PreparedContext {
    context: Context,
    binding: ReviewBinding,
    baseline: CurrentBaseline,
    now: u64,
}
enum Subject {
    NewPlan(String),
    CurrentPlan,
    CurrentTree,
}

// Proof/I/O failures must preserve a mutation's original request as uncertain.
// Stable binding/gate refusals remain refusals and never become fake pending work.
enum PreparationFailure {
    Source(String),
    Refused(String),
}
impl PreparationFailure {
    fn for_request(self, request_id: &str) -> String {
        match self {
            Self::Source(reason) => crate::request_ledger::uncertain_request(request_id, reason),
            Self::Refused(reason) => reason,
        }
    }
    fn into_hub(self) -> HubError {
        match self {
            Self::Source(reason) => HubError::Uncertain(reason),
            Self::Refused(reason) => HubError::Invalid(reason),
        }
    }
}
impl From<&str> for PreparationFailure {
    fn from(reason: &str) -> Self {
        Self::Refused(reason.into())
    }
}
impl std::fmt::Display for PreparationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(reason) | Self::Refused(reason) => formatter.write_str(reason),
        }
    }
}

fn actual_worktree(catalog: &Catalog) -> Result<(), PreparationFailure> {
    if catalog.branch == "HEAD" {
        return Err("Review requires an attached Task branch".into());
    }
    if crate::repo::toplevel(&catalog.root).map_err(PreparationFailure::Source)? != catalog.root
        || crate::repo::git_timeout(
            &catalog.root,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            Duration::from_secs(10),
        )
        .map_err(PreparationFailure::Source)?
            != catalog.branch
    {
        return Err("Review worktree/branch changed".into());
    }
    Ok(())
}

fn context(
    live: &ProjectSession,
    catalog: &Catalog,
    host: &str,
    run: Option<&str>,
    now: u64,
) -> Result<Context, String> {
    live.projection()?;
    let (task, run) = live.tasks.check_context(catalog, host, run)?;
    crate::tasks::validate_task(&task.spec, catalog)?;
    if task.object_type != "Task"
        || run.object_type != "Run"
        || !matches!(task.state.as_str(), "running" | "verifying" | "held")
        || !matches!(run.state.as_str(), "running" | "verifying" | "held")
        || run.task != task.spec.id
        || run.host != host
        || run.worktree != catalog.root.to_string_lossy()
        || run.branch != catalog.branch
        || run.branch != format!("t/{}", task.spec.id)
    {
        return Err("Review requires this host's exact active Task worktree/branch".into());
    }
    let expected: BTreeSet<_> = std::iter::once(&task.spec.module)
        .chain(task.spec.touches.iter())
        .map(|id| format!("module:{id}"))
        .collect();
    let names: BTreeSet<_> = run.leases.iter().map(|f| f.name.clone()).collect();
    if names != expected
        || names.len() != run.leases.len()
        || run.leases.iter().any(|f| {
            f.holder != run.id
                || f.epoch == 0
                || live.head.leases.get(&f.name).is_none_or(|lease| {
                    lease.name != f.name
                        || lease.holder != f.holder
                        || lease.epoch != f.epoch
                        || lease.expires_at <= now
                })
        })
    {
        return Err("Review requires the complete current Task fences".into());
    }
    Ok(Context {
        plan: live.human.current_plan_subject(&run.id).ok(),
        task,
        run,
        session: live.snapshot_session_id(),
        private: live
            .verified_sources
            .as_ref()
            .ok_or("Review source proof is not prepared")?
            .1
            .clone(),
    })
}
fn same_context(a: &Context, b: &Context) -> Result<bool, String> {
    Ok(a.session == b.session
        && a.private == b.private
        && recovery_contract::canonical_hash(&(&a.task, &a.run))?
            == recovery_contract::canonical_hash(&(&b.task, &b.run))?)
}
// App selection identifies the project, not the execution worktree. Resolve
// that worktree only from a synchronized actual Run, then prove its coordinates
// outside the Session lock. The CLI's new-request path never uses this helper.
fn resolved_review_catalog(
    session: &Mutex<ProjectSession>,
    selected: &Catalog,
    inputs: &RecoveryInputs,
    host: &str,
    run: &str,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<Catalog, PreparationFailure> {
    let started = clock().map_err(PreparationFailure::Source)?;
    let captured = ProjectSession::continue_shared(session, selected, inputs, started, |live| {
        live.projection()?;
        let (task, run) = live.tasks.check_context(selected, host, Some(run))?;
        Ok::<_, String>(Context {
            task,
            run,
            session: live.snapshot_session_id(),
            private: live
                .verified_sources
                .as_ref()
                .ok_or("Review source proof is not prepared")?
                .1
                .clone(),
            plan: None,
        })
    })
    .map_err(|error| PreparationFailure::Source(error.to_string()))?
    .map_err(PreparationFailure::Refused)?;
    let path = std::path::Path::new(&captured.run.worktree);
    if !path.is_absolute() {
        return Err("Review Run worktree must be an absolute actual binding".into());
    }
    let actual = Catalog::load(path).map_err(PreparationFailure::Source)?;
    if actual.revision != selected.revision {
        return Err(PreparationFailure::Source(
            "Review authority changed while resolving its worktree".into(),
        ));
    }
    if actual.root.to_string_lossy() != captured.run.worktree
        || actual.branch != captured.run.branch
        || actual.common_dir != selected.common_dir
        || actual.config.project_id() != selected.config.project_id()
        || actual.config.to_yaml() != selected.config.to_yaml()
    {
        return Err("Review Run worktree/project/config/authority binding changed".into());
    }
    let finished = clock().map_err(PreparationFailure::Source)?;
    if finished < started {
        return Err(PreparationFailure::Source(
            "clock moved backwards while resolving Review worktree".into(),
        ));
    }
    actual_worktree(&actual)?;
    let current = ProjectSession::continue_shared_bound(
        session,
        &actual,
        inputs,
        finished,
        Some(&captured.private),
        |live| context(live, &actual, host, Some(run), finished),
    )
    .map_err(|error| PreparationFailure::Source(error.to_string()))?
    .map_err(PreparationFailure::Refused)?;
    if !same_context(&captured, &current).map_err(PreparationFailure::Source)? {
        return Err("Review Task/Run/session changed while resolving its worktree".into());
    }
    Ok(actual)
}

fn sorted_fences(fences: &[Fence]) -> Vec<Fence> {
    let mut fences = fences.to_vec();
    fences.sort_by(|a, b| a.name.cmp(&b.name));
    fences
}
fn same_binding(a: &ReviewBinding, b: &ReviewBinding) -> bool {
    a.run == b.run
        && a.task == b.task
        && a.baseline == b.baseline
        && a.subject == b.subject
        && sorted_fences(&a.fences) == sorted_fences(&b.fences)
}
fn latest_review(live: &ProjectSession, run: &str, subject: &ReviewSubject) -> Option<Event> {
    let canonical = live.human.projection().1;
    canonical
        .openings
        .values()
        .filter(|event| {
            event.seq > canonical.invalidated_through
                && live
                    .human
                    .original_review_hold(&event.obj)
                    .is_ok_and(|hold| {
                        let ReviewBodyView::Review { binding, .. } = &hold.body;
                        binding.run == run
                            && matches!(binding.subject, ReviewSubject::Plan { .. })
                                == matches!(subject, ReviewSubject::Plan { .. })
                    })
        })
        .max_by_key(|event| event.seq)
        .cloned()
}

fn prepare_context(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    host: &str,
    run: Option<&str>,
    subject: Subject,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<PreparedContext, PreparationFailure> {
    let now = clock().map_err(PreparationFailure::Source)?;
    actual_worktree(catalog)?;
    let captured = ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
        context(live, catalog, host, run, now)
    })
    .map_err(|error| PreparationFailure::Source(error.to_string()))?
    .map_err(PreparationFailure::Refused)?;
    let baseline = current_baseline(catalog, inputs)
        .map_err(PreparationFailure::Source)?
        .ok_or("Review requires an actually approved current baseline")?;
    let subject = match subject {
        Subject::NewPlan(sha256) => ReviewSubject::Plan { sha256 },
        Subject::CurrentPlan => captured
            .plan
            .clone()
            .ok_or("current run has no canonical Plan opening")?,
        Subject::CurrentTree => {
            let checked_head = crate::repo::git_timeout(
                &catalog.root,
                &["rev-parse", "--verify", "HEAD^{commit}"],
                Duration::from_secs(10),
            )
            .map_err(PreparationFailure::Source)?;
            let check = match crate::task_gates::check_shared(
                session,
                catalog,
                inputs,
                host,
                Some(&captured.run.id),
                clock,
            ) {
                Ok(check) => check,
                Err(error) => {
                    let message = format!("G4 check refused: {error:?}");
                    if matches!(&error, crate::task_gates::GateError::NotLatestMain) {
                        // The shared check also uses NotLatestMain when HEAD
                        // or authority moves during its Git reads. Certify the
                        // stable behind-main refusal only against this same
                        // prepared source, never a newly absorbed private tip.
                        let refused_at = clock().map_err(PreparationFailure::Source)?;
                        if refused_at < now {
                            return Err(PreparationFailure::Source(
                                "clock moved backwards during G4 refusal proof".into(),
                            ));
                        }
                        ProjectSession::sync_shared_bound(
                            session,
                            catalog,
                            inputs,
                            refused_at,
                            Some(&captured.private),
                        )
                        .map_err(|failure| PreparationFailure::Source(failure.to_string()))?;
                        let current_head = crate::repo::git_timeout(
                            &catalog.root,
                            &["rev-parse", "--verify", "HEAD^{commit}"],
                            Duration::from_secs(10),
                        )
                        .map_err(PreparationFailure::Source)?;
                        if current_head != checked_head {
                            return Err(PreparationFailure::Source(
                                "Review HEAD changed during G4 check refusal".into(),
                            ));
                        }
                        actual_worktree(catalog)
                            .map_err(|failure| PreparationFailure::Source(failure.to_string()))?;
                        ProjectSession::probe_actual_binding(catalog, inputs, &captured.private)
                            .map_err(|failure| PreparationFailure::Source(failure.to_string()))?;
                    }
                    return Err(match error {
                        crate::task_gates::GateError::Infrastructure(_)
                        | crate::task_gates::GateError::ProofRejected(_) => {
                            PreparationFailure::Source(message)
                        }
                        _ => PreparationFailure::Refused(message),
                    });
                }
            };
            if check.base != baseline.main_revision {
                return Err(PreparationFailure::Source(
                    "G4 check authority differs from current baseline authority".into(),
                ));
            }
            ReviewSubject::Tree { tree: check.tree }
        }
    };
    actual_worktree(catalog)?;
    let finished = clock().map_err(PreparationFailure::Source)?;
    if finished < now {
        return Err(PreparationFailure::Source(
            "clock moved backwards while preparing Review".into(),
        ));
    }
    if current_baseline(catalog, inputs)
        .map_err(PreparationFailure::Source)?
        .as_ref()
        != Some(&baseline)
    {
        return Err(PreparationFailure::Source(
            "Review baseline changed during preparation".into(),
        ));
    }
    let current = ProjectSession::continue_shared_bound(
        session,
        catalog,
        inputs,
        finished,
        Some(&captured.private),
        |live| context(live, catalog, host, run, finished),
    )
    .map_err(|error| PreparationFailure::Source(error.to_string()))?
    .map_err(PreparationFailure::Refused)?;
    if !same_context(&captured, &current).map_err(PreparationFailure::Source)? {
        return Err("Review Task/Run/session changed during preparation".into());
    }
    let binding = ReviewBinding {
        run: current.run.id.clone(),
        task: current.task.spec.id.clone(),
        baseline: baseline.baseline.clone(),
        fences: sorted_fences(&current.run.leases),
        subject,
    };
    crate::request_ledger::validate_task_review_binding(&binding)
        .map_err(PreparationFailure::Refused)?;
    Ok(PreparedContext {
        context: current,
        binding,
        baseline,
        now: finished,
    })
}

fn final_context(
    live: &ProjectSession,
    catalog: &Catalog,
    host: &str,
    prepared: &PreparedContext,
    now: u64,
) -> Result<(), PreparationFailure> {
    if now < prepared.now {
        return Err(PreparationFailure::Source(
            "clock moved backwards before Review append".into(),
        ));
    }
    let current = context(live, catalog, host, Some(&prepared.binding.run), now)
        .map_err(PreparationFailure::Refused)?;
    if !same_context(&prepared.context, &current).map_err(PreparationFailure::Source)? {
        return Err("Review Task/Run/session changed before append".into());
    }
    if matches!(prepared.binding.subject, ReviewSubject::Plan { .. })
        && prepared.context.plan != current.plan
    {
        return Err("current Plan changed before Review append".into());
    }
    Ok(())
}

fn validity_shared(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    host: &str,
    hold: &str,
    binding: &ReviewBinding,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<(ReviewValidity, Option<Catalog>), String> {
    let result = (|| {
        let subject = match binding.subject {
            ReviewSubject::Plan { .. } => Subject::CurrentPlan,
            ReviewSubject::Tree { .. } => Subject::CurrentTree,
        };
        let actual = resolved_review_catalog(session, catalog, inputs, host, &binding.run, clock)
            .map_err(|error| error.to_string())?;
        let catalog = &actual;
        let prepared = prepare_context(
            session,
            catalog,
            inputs,
            host,
            Some(&binding.run),
            subject,
            clock,
        )
        .map_err(|error| error.to_string())?;
        if !same_binding(binding, &prepared.binding) {
            return Err("original Review binding is no longer current".into());
        }
        let now = clock()?;
        actual_worktree(catalog).map_err(|error| error.to_string())?;
        ProjectSession::continue_shared_bound(
            session,
            catalog,
            inputs,
            now,
            Some(&prepared.context.private),
            |live| -> Result<(), String> {
                final_context(live, catalog, host, &prepared, now)
                    .map_err(|failure| failure.to_string())?;
                if latest_review(live, &binding.run, &binding.subject)
                    .as_ref()
                    .map(|e| e.obj.as_str())
                    != Some(hold)
                {
                    return Err("a later opening or rebuild superseded this Review".into());
                }
                Ok(())
            },
        )
        .map_err(|e| e.to_string())??;
        Ok::<_, String>(actual)
    })();
    match result {
        Ok(actual) => Ok((ReviewValidity::Current, Some(actual))),
        Err(reason) => {
            // A missing/expired current context may still have a historical
            // receipt, but a failed actual source proof is never downgraded.
            ProjectSession::sync_shared(session, catalog, inputs, clock()?)
                .map_err(|e| e.to_string())?;
            Ok((ReviewValidity::HistoricalOnly { reason }, None))
        }
    }
}

fn retry_shared(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    identity: &RequestIdentity,
    request_id: &str,
    now: u64,
) -> Result<Option<Event>, String> {
    let inputs = outbox
        .read_inputs()
        .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?;
    let result = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        let ledger = live
            .baseline_requests
            .as_mut()
            .ok_or_else(|| HubError::Invalid("project request history is not prepared".into()))?;
        ledger
            .reserve(identity.clone())
            .map_err(HubError::Invalid)?;
        ledger.retry_original(catalog, identity, &mut *live.hub, outbox, now)
    })
    .map_err(|error| crate::request_ledger::uncertain_request(request_id, error))?;
    ProjectSession::finish_baseline_request_shared(
        session, catalog, outbox, request_id, result, now,
    )
}

// Only a genuinely new Review reaches project-level Task maintenance. Original
// Review identity/receipt recovery must never wait for another Task request.
fn new_review_inputs(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    request_id: &str,
    now: u64,
) -> Result<RecoveryInputs, String> {
    let inputs = outbox
        .read_inputs()
        .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?;
    if ProjectSession::task_reconciliation_needed_shared(session, catalog, &inputs)
        .map_err(|error| crate::request_ledger::uncertain_request(request_id, error))?
    {
        ProjectSession::reconcile_pending_shared(session, catalog, &inputs, now, outbox)
            .map_err(|error| crate::request_ledger::uncertain_request(request_id, error))?;
    }
    // Coordination may have durably appended the exact original confirmation.
    // Never pass the old physical-input fence into binding or renewal.
    outbox
        .read_inputs()
        .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))
}

fn append(
    live: &mut ProjectSession,
    outbox: &mut RecoveryOutbox,
    event: NewEvent,
    now: u64,
) -> Result<Option<Event>, HubError> {
    match recovery_outbox::append_durable(&mut *live.hub, outbox, event, now)? {
        AppendReceipt::Event(event) => Ok(Some(*event)),
        AppendReceipt::Duplicate { .. } => Err(HubError::Uncertain(
            "Review duplicate has no proven actual original receipt".into(),
        )),
    }
}

impl ProjectSession {
    /// Record only the CLI's typed ReviewRequest. There is no approval input.
    #[allow(clippy::too_many_arguments)]
    pub fn request_task_review_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        request_id: &str,
        command: &HumanCommand,
        run: Option<&str>,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<TaskReviewRequestOutcome, String> {
        let request = InterventionRequest::prepare(
            catalog.config.project_id(),
            host,
            request_id,
            command,
            run,
        )?;
        let identity = RequestIdentity::Intervention(request.clone());
        // Retry is deliberately before all current-run and new-subject work.
        let event = match retry_shared(
            session,
            catalog,
            outbox,
            &identity,
            request_id,
            clock()
                .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?,
        )? {
            Some(event) => event,
            None => {
                let HumanCommand::ReviewRequest {
                    gate,
                    content,
                    source_commit,
                    source_path,
                } = command
                else {
                    return Err("only ReviewRequest is supported here".into());
                };
                // An original retry never reaches here. Only a genuinely new
                // CLI interaction renews the existing active binding, using the
                // established system-lease path without acquiring new epochs.
                let renewal_now = clock().map_err(|reason| {
                    crate::request_ledger::uncertain_request(request_id, reason)
                })?;
                let renewal_inputs =
                    new_review_inputs(session, catalog, outbox, request_id, renewal_now)?;
                actual_worktree(catalog).map_err(|error| error.for_request(request_id))?;
                let (renewal, private) =
                    Self::continue_shared(session, catalog, &renewal_inputs, renewal_now, |live| {
                        let private = context(live, catalog, host, run, renewal_now)?.private;
                        Ok::<_, String>((
                            live.renew_binding(catalog, host, run, renewal_now),
                            private,
                        ))
                    })
                    .map_err(|error| {
                        crate::request_ledger::uncertain_request(request_id, error)
                    })??;
                Self::finish_shared(
                    session,
                    catalog,
                    &renewal_inputs,
                    renewal_now,
                    &private,
                    renewal,
                )
                .map_err(|error| crate::request_ledger::uncertain_request(request_id, error))?;
                let subject = match (gate, content) {
                    (ReviewGate::G1, Some(content)) => {
                        Subject::NewPlan(recovery_contract::sha256(content.as_bytes()))
                    }
                    (ReviewGate::G4, None) => Subject::CurrentTree,
                    _ => return Err("invalid Review gate/content".into()),
                };
                let inputs = outbox.read_inputs().map_err(|reason| {
                    crate::request_ledger::uncertain_request(request_id, reason)
                })?;
                let prepared =
                    prepare_context(session, catalog, &inputs, host, run, subject, &clock)
                        .map_err(|error| error.for_request(request_id))?;
                let now = clock().map_err(|reason| {
                    crate::request_ledger::uncertain_request(request_id, reason)
                })?;
                if current_baseline(catalog, &inputs)
                    .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?
                    .as_ref()
                    != Some(&prepared.baseline)
                {
                    return Err(crate::request_ledger::uncertain_request(
                        request_id,
                        "Review baseline changed before append",
                    ));
                }
                let result = Self::continue_shared_bound(
                    session,
                    catalog,
                    &inputs,
                    now,
                    Some(&prepared.context.private),
                    |live| {
                        let ledger = live.baseline_requests.as_ref().ok_or_else(|| {
                            HubError::Invalid("project request history is not prepared".into())
                        })?;
                        if let Some(event) = ledger.retry_original(
                            catalog,
                            &identity,
                            &mut *live.hub,
                            outbox,
                            now,
                        )? {
                            return Ok(Some(event));
                        }
                        final_context(live, catalog, host, &prepared, now)
                            .map_err(PreparationFailure::into_hub)?;
                        let previous =
                            latest_review(live, &prepared.binding.run, &prepared.binding.subject);
                        let reusable = previous.as_ref().and_then(|event| {
                            live.human
                                .original_review_hold(&event.obj)
                                .ok()
                                .filter(|hold| {
                                    let ReviewBodyView::Review { binding, .. } = &hold.body;
                                    same_binding(binding, &prepared.binding)
                                })
                                .map(|hold| (event, hold.clone()))
                        });
                        let (kind, hold, opening_eid, raw_hold) =
                            if let Some((event, hold)) = reusable {
                                (
                                    "hold.requested",
                                    hold,
                                    event.eid.clone(),
                                    Some(event.data["hold"].clone()),
                                )
                            } else {
                                let kind = "hold.opened";
                                let eid = request_event_eid(request_id, kind)
                                    .map_err(HubError::Invalid)?;
                                (
                                    kind,
                                    ReviewHoldView {
                                        object_type: "Hold".into(),
                                        id: format!("H-{request_id}"),
                                        body: ReviewBodyView::Review {
                                            binding: prepared.binding.clone(),
                                            verdict: None,
                                            plan_content: content.clone(),
                                            source_commit: source_commit.clone(),
                                            source_path: source_path.clone(),
                                            comment: None,
                                        },
                                        opened_ms: now,
                                        opened_seq: 0,
                                        reply: None,
                                        extra: BTreeMap::new(),
                                    },
                                    eid,
                                    None,
                                )
                            };
                        let ReviewBodyView::Review {
                            binding: original_binding,
                            ..
                        } = &hold.body;
                        let original_binding = original_binding.clone();
                        let record = ReviewRequestedRecorded {
                            hold: hold.clone(),
                            request_body: request.body.clone(),
                            p3_request: request.meta.clone(),
                            p3_receipt: RequestReceipt {
                                request_id: request_id.into(),
                                body_hash: request.meta.body_hash.clone(),
                                result_ref: ReviewRequestResult {
                                    hold: hold.id.clone(),
                                    binding: original_binding.clone(),
                                    opening_eid,
                                },
                            },
                        };
                        let mut data = serde_json::to_value(record)
                            .map_err(|e| HubError::Invalid(e.to_string()))?;
                        if let Some(raw) = raw_hold {
                            data["hold"] = raw;
                        }
                        let event = NewEvent {
                            eid: request_event_eid(request_id, kind).map_err(HubError::Invalid)?,
                            host: host.into(),
                            actor: request.body.actor.clone(),
                            kind: kind.into(),
                            layer: "L5".into(),
                            obj: hold.id,
                            data,
                            guards: original_binding.fences,
                        };
                        decode_review_request(catalog.config.project_id(), &event)
                            .map_err(HubError::Invalid)?;
                        append(live, outbox, event, now)
                    },
                )
                .map_err(|error| crate::request_ledger::uncertain_request(request_id, error))?;
                Self::finish_baseline_request_shared(
                    session, catalog, outbox, request_id, result, now,
                )?
                .ok_or("Review request has no actual receipt")?
            }
        };
        let record = decode_review_request(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&event),
        )
        .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?;
        let inputs = outbox
            .read_inputs()
            .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?;
        let current_validity = if event.stale {
            Self::sync_shared(
                session,
                catalog,
                &inputs,
                clock().map_err(|reason| {
                    crate::request_ledger::uncertain_request(request_id, reason)
                })?,
            )
            .map_err(|error| crate::request_ledger::uncertain_request(request_id, error))?;
            ReviewValidity::HistoricalOnly {
                reason: "original Review request was accepted stale".into(),
            }
        } else {
            validity_shared(
                session,
                catalog,
                &inputs,
                host,
                &record.hold.id,
                &record.p3_receipt.result_ref.binding,
                &clock,
            )
            .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?
            .0
        };
        Ok(TaskReviewRequestOutcome {
            receipt: record.p3_receipt,
            current_validity,
        })
    }

    /// AppGate is enforced by the adapter before supplying this finite human
    /// actor. Full original raw Hold identity is retained in the answer proof.
    #[allow(clippy::too_many_arguments)]
    pub fn answer_task_review_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        actor: &str,
        request_id: &str,
        action: &TaskReviewAction,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<TaskReviewOutcome, String> {
        let request = TaskReviewRequest::prepare(
            catalog.config.project_id(),
            host,
            actor,
            request_id,
            action,
        )?;
        let identity = RequestIdentity::TaskReview(request.clone());
        let event = match retry_shared(
            session,
            catalog,
            outbox,
            &identity,
            request_id,
            clock()
                .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?,
        )? {
            Some(event) => event,
            None => {
                let TaskReviewAction::Review {
                    hold,
                    binding,
                    verdict,
                    comment,
                } = action;
                let inputs = new_review_inputs(
                    session,
                    catalog,
                    outbox,
                    request_id,
                    clock().map_err(|reason| {
                        crate::request_ledger::uncertain_request(request_id, reason)
                    })?,
                )?;
                let actual =
                    resolved_review_catalog(session, catalog, &inputs, host, &binding.run, &clock)
                        .map_err(|error| error.for_request(request_id))?;
                let catalog = &actual;
                let subject = match binding.subject {
                    ReviewSubject::Plan { .. } => Subject::CurrentPlan,
                    ReviewSubject::Tree { .. } => Subject::CurrentTree,
                };
                let prepared = prepare_context(
                    session,
                    catalog,
                    &inputs,
                    host,
                    Some(&binding.run),
                    subject,
                    &clock,
                )
                .map_err(|error| error.for_request(request_id))?;
                if !same_binding(binding, &prepared.binding) {
                    return Err("Review answer binding is no longer current".into());
                }
                let now = clock().map_err(|reason| {
                    crate::request_ledger::uncertain_request(request_id, reason)
                })?;
                actual_worktree(catalog).map_err(|error| error.for_request(request_id))?;
                if current_baseline(catalog, &inputs)
                    .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?
                    .as_ref()
                    != Some(&prepared.baseline)
                {
                    return Err(crate::request_ledger::uncertain_request(
                        request_id,
                        "Review baseline changed before answer",
                    ));
                }
                let result = Self::continue_shared_bound(
                    session,
                    catalog,
                    &inputs,
                    now,
                    Some(&prepared.context.private),
                    |live| {
                        let ledger = live.baseline_requests.as_ref().ok_or_else(|| {
                            HubError::Invalid("project request history is not prepared".into())
                        })?;
                        if let Some(event) = ledger.retry_original(
                            catalog,
                            &identity,
                            &mut *live.hub,
                            outbox,
                            now,
                        )? {
                            return Ok(Some(event));
                        }
                        final_context(live, catalog, host, &prepared, now)
                            .map_err(PreparationFailure::into_hub)?;
                        if latest_review(live, &binding.run, &binding.subject)
                            .as_ref()
                            .map(|e| &e.obj)
                            != Some(hold)
                        {
                            return Err(HubError::Invalid(
                                "Review Hold is not the current original opening".into(),
                            ));
                        }
                        if live.human.projection().1.reviewed.contains_key(hold) {
                            return Err(HubError::Invalid(
                                "Review already answered; retry the original request".into(),
                            ));
                        }
                        let mut original = live
                            .human
                            .original_review_hold(hold)
                            .map_err(HubError::Invalid)?
                            .clone();
                        let ReviewBodyView::Review {
                            binding: raw_binding,
                            verdict: raw_verdict,
                            comment: raw_comment,
                            ..
                        } = &mut original.body;
                        if &*raw_binding != binding {
                            return Err(HubError::Invalid(
                                "Review answer must preserve exact original binding order".into(),
                            ));
                        }
                        *raw_verdict = Some(verdict.clone());
                        *raw_comment = Some(comment.clone());
                        let eid = request_event_eid(request_id, "hold.reviewed")
                            .map_err(HubError::Invalid)?;
                        original.reply = Some(ReviewReplyView {
                            id: request_id.into(),
                            text: comment.clone(),
                            actor: actor.into(),
                            at_ms: now,
                            extra: BTreeMap::new(),
                        });
                        let fact = ReviewFact {
                            binding: binding.clone(),
                            verdict: verdict.clone(),
                            actor: actor.into(),
                            answer_eid: eid.clone(),
                        };
                        let result = TaskReviewResult {
                            hold: hold.clone(),
                            fact: fact.clone(),
                            verdict: verdict.clone(),
                            comment: comment.clone(),
                        };
                        let record = TaskReviewRecorded {
                            hold: original,
                            fact,
                            comment: comment.clone(),
                            request_body: request.body.clone(),
                            p3_request: request.meta.clone(),
                            p3_receipt: RequestReceipt {
                                request_id: request_id.into(),
                                body_hash: request.meta.body_hash.clone(),
                                result_ref: result,
                            },
                        };
                        let event = NewEvent {
                            eid,
                            host: host.into(),
                            actor: actor.into(),
                            kind: "hold.reviewed".into(),
                            layer: "L5".into(),
                            obj: hold.clone(),
                            data: serde_json::to_value(record)
                                .map_err(|e| HubError::Invalid(e.to_string()))?,
                            guards: binding.fences.clone(),
                        };
                        decode_task_review(catalog.config.project_id(), &event)
                            .map_err(HubError::Invalid)?;
                        append(live, outbox, event, now)
                    },
                )
                .map_err(|error| crate::request_ledger::uncertain_request(request_id, error))?;
                Self::finish_baseline_request_shared(
                    session, catalog, outbox, request_id, result, now,
                )?
                .ok_or("Review answer has no actual receipt")?
            }
        };
        let record = decode_task_review(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&event),
        )
        .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?;
        let inputs = outbox
            .read_inputs()
            .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?;
        let current_validity = if event.stale {
            Self::sync_shared(
                session,
                catalog,
                &inputs,
                clock().map_err(|reason| {
                    crate::request_ledger::uncertain_request(request_id, reason)
                })?,
            )
            .map_err(|error| crate::request_ledger::uncertain_request(request_id, error))?;
            ReviewValidity::HistoricalOnly {
                reason: "original Review answer was accepted stale".into(),
            }
        } else {
            validity_shared(
                session,
                catalog,
                &inputs,
                host,
                &record.hold.id,
                &record.fact.binding,
                &clock,
            )
            .map_err(|reason| crate::request_ledger::uncertain_request(request_id, reason))?
            .0
        };
        Ok(TaskReviewOutcome {
            receipt: record.p3_receipt,
            current_validity,
        })
    }

    /// A read may synchronize verified state, but never renews, appends, or opens
    /// a Hub. The caller supplies an already selected project Session.
    pub fn load_task_review_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        host: &str,
        hold: &str,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<TaskReviewView, String> {
        crate::hub::validate_token("Hold ID", hold).map_err(|e| e.to_string())?;
        let view = Self::continue_shared(session, catalog, inputs, clock()?, |live| {
            if !live.human.projection().1.openings.contains_key(hold) {
                return live
                    .baseline_requests
                    .as_ref()
                    .and_then(|ledger| ledger.historical_task_review(hold))
                    .ok_or_else(|| "missing actual Review opening and archive history".into());
            }
            let raw = live.human.original_review_hold(hold)?;
            let ReviewBodyView::Review { binding, .. } = &raw.body;
            Ok::<_, String>(TaskReviewView {
                project_id: catalog.config.project_id().into(),
                opening: live
                    .human
                    .projection()
                    .1
                    .openings
                    .get(hold)
                    .ok_or("missing original Review Event")?
                    .clone(),
                current_hold: live
                    .human
                    .projection()
                    .0
                    .get(hold)
                    .ok_or("missing current Hold projection")?
                    .clone(),
                binding: binding.clone(),
                current_validity: ReviewValidity::Current,
            })
        })
        .map_err(|e| e.to_string())??;
        if matches!(view.current_validity, ReviewValidity::HistoricalOnly { .. }) {
            // This raw archived record is display-only. In particular its
            // opened_seq is not rewritten into a nonexistent Human projection.
            return Self::continue_shared(session, catalog, inputs, clock()?, |live| {
                if live.human.projection().1.openings.contains_key(hold)
                    || live
                        .baseline_requests
                        .as_ref()
                        .and_then(|ledger| ledger.historical_task_review(hold))
                        .as_ref()
                        != Some(&view)
                {
                    return Err("Review archive view changed while loading; reload".into());
                }
                Ok(view)
            })
            .map_err(|e| e.to_string())?;
        }
        let TaskReviewView {
            opening,
            current_hold,
            binding,
            ..
        } = view;
        let (current_validity, actual) =
            validity_shared(session, catalog, inputs, host, hold, &binding, &clock)?;
        let checked_catalog = actual.as_ref().unwrap_or(catalog);
        Self::continue_shared(session, checked_catalog, inputs, clock()?, |live| {
            if live.human.projection().1.openings.get(hold) != Some(&opening)
                || live.human.projection().0.get(hold) != Some(&current_hold)
            {
                return Err("Review view changed while loading; reload".into());
            }
            if current_validity == ReviewValidity::Current {
                let current = context(live, checked_catalog, host, Some(&binding.run), clock()?)?;
                if latest_review(live, &binding.run, &binding.subject)
                    .as_ref()
                    .map(|e| e.obj.as_str())
                    != Some(hold)
                    || sorted_fences(&current.run.leases) != sorted_fences(&binding.fences)
                {
                    return Err("Review current binding changed while loading; reload".into());
                }
            }
            Ok(TaskReviewView {
                project_id: catalog.config.project_id().into(),
                opening,
                current_hold,
                binding,
                current_validity,
            })
        })
        .map_err(|e| e.to_string())?
    }
}
