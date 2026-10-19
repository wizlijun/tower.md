//! One source-proved producer for App controls and CLI pause/resume requests.
use super::ProjectSession;
use crate::{
    ask_contract::Recipient,
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError, NewEvent},
    human_intervention::{Hold, HoldBody, Scope},
    intervention_control::{self as control, *},
    recovery_contract::recorded::{request_event_eid, RequestReceipt},
    recovery_outbox::{self, RecoveryInputs, RecoveryOutbox},
    request_ledger::{uncertain_request, RequestIdentity},
    tasks::{Run, Task},
};
use std::{collections::BTreeMap, sync::Mutex};

fn active(live: &ProjectSession, now: u64) -> Result<Vec<(ControlRun, Recipient)>, String> {
    let projection = live.projection()?;
    let mut result = Vec::new();
    for raw in projection
        .objects
        .get("Run")
        .into_iter()
        .flat_map(|runs| runs.values())
    {
        let run: Run = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
        if !matches!(run.state.as_str(), "running" | "verifying" | "held")
            || crate::finish::run_deadline(&run)?.is_some_and(|deadline| now >= deadline)
            || run.leases.is_empty()
            || !run.leases.iter().all(|f| {
                f.holder == run.id
                    && live
                        .head
                        .leases
                        .get(&f.name)
                        .is_some_and(|l| l.fence() == *f && l.expires_at > now)
            })
        {
            continue;
        }
        let task: Task = serde_json::from_value(
            projection
                .objects
                .get("Task")
                .and_then(|tasks| tasks.get(&run.task))
                .cloned()
                .ok_or("active control Run lacks Task")?,
        )
        .map_err(|e| e.to_string())?;
        if task.spec.id != run.task
            || !matches!(task.state.as_str(), "running" | "verifying" | "held")
        {
            return Err("active control Task/Run binding differs".into());
        }
        let mut expected: Vec<_> = std::iter::once(&task.spec.module)
            .chain(task.spec.touches.iter())
            .map(|module| format!("module:{module}"))
            .collect();
        expected.sort();
        expected.dedup();
        let mut actual: Vec<_> = run.leases.iter().map(|f| f.name.clone()).collect();
        actual.sort();
        if actual != expected {
            return Err("active control Run lacks complete module fences".into());
        }
        result.push((
            ControlRun {
                run: run.id.clone(),
                task: run.task.clone(),
                module: task.spec.module,
            },
            Recipient {
                run: run.id,
                task: run.task,
                host: run.host,
                worktree: run.worktree,
                branch: run.branch,
                fences: run.leases,
            },
        ));
    }
    result.sort_by(|a, b| a.0.run.cmp(&b.0.run));
    Ok(result)
}
fn current_pause(live: &ProjectSession, catalog: &Catalog) -> Result<Option<Event>, String> {
    let (holds, canonical) = live.human.projection();
    let mut pauses = Vec::new();
    for (id, raw) in holds {
        if raw["body"]["kind"] == "pause" && raw["body"]["resumed"] == false {
            let original = canonical
                .openings
                .get(id)
                .ok_or("pause lacks canonical opening")?;
            control::decode(
                catalog.config.project_id(),
                &recovery_outbox::original_of(original),
            )?;
            pauses.push(original.clone());
        }
    }
    if pauses.len() > 1 {
        return Err("multiple current project pauses are ambiguous".into());
    }
    Ok(pauses.pop())
}
fn current_resume(
    live: &ProjectSession,
    pause: &Event,
    runs: &[String],
) -> Result<Option<Event>, String> {
    let (_, canonical) = live.human.projection();
    let revision = pause.data["hold"]["body"]["revision"]
        .as_str()
        .ok_or("Pause lacks revision")?;
    Ok(canonical
        .openings
        .values()
        .filter(|event| {
            event.seq > canonical.invalidated_through
                && event.data["request_body"]["action"]["action"] == "resume_all"
                && event.data["hold"]["body"]["pause_revision"] == revision
                && event.data["hold"]["body"]["active_runs"] == serde_json::json!(runs)
        })
        .max_by_key(|event| event.seq)
        .cloned())
}
fn selected(
    live: &ProjectSession,
    catalog: &Catalog,
    action: &ControlAction,
    now: u64,
) -> Result<Vec<Recipient>, String> {
    let values = active(live, now)?;
    if let ControlAction::Interject { scope, .. } = action {
        match scope {
            Scope::Module { module }
                if catalog.objects.get(module).and_then(|d| d.doc_type()) != Some("Module") =>
            {
                return Err("interjection Module is not in authoritative main".into())
            }
            Scope::Task { task }
                if !live
                    .projection()?
                    .objects
                    .get("Task")
                    .is_some_and(|tasks| tasks.contains_key(task)) =>
            {
                return Err("interjection Task is not recorded".into())
            }
            _ => {}
        }
    }
    Ok(values
        .into_iter()
        .filter(|(_, recipient)| match action {
            ControlAction::Interject {
                scope: Scope::Module { module },
                ..
            } => recipient
                .fences
                .iter()
                .any(|f| f.name == format!("module:{module}")),
            ControlAction::Interject {
                scope: Scope::Task { task },
                ..
            } => &recipient.task == task,
            _ => true,
        })
        .map(|(_, recipient)| recipient)
        .collect())
}
fn build(
    live: &ProjectSession,
    catalog: &Catalog,
    request: &ControlRequest,
    now: u64,
) -> Result<ControlRecorded, String> {
    let recipients = selected(live, catalog, &request.body.action, now)?;
    let runs: Vec<_> = recipients.iter().map(|r| r.run.clone()).collect();
    let ledger = live
        .baseline_requests
        .as_ref()
        .ok_or("control request history is not prepared")?;
    let claims = recipients
        .iter()
        .map(|r| ledger.control_claim(r))
        .collect::<Result<Vec<_>, _>>()?;
    let pause = current_pause(live, catalog)?;
    let mut source_pause = None;
    let mut source_resume = None;
    let (body, pause_revision) = match &request.body.action {
        ControlAction::Interject { scope, text } => (
            HoldBody::Prompt {
                scope: scope.clone(),
                text: text.clone(),
            },
            None,
        ),
        ControlAction::PauseAll => {
            if pause.is_some() {
                return Err("project already paused; use the existing revision".into());
            }
            let revision = control::revision(request)?;
            (
                HoldBody::Pause {
                    revision: revision.clone(),
                    active_runs: runs.clone(),
                    resumed: false,
                },
                Some(revision),
            )
        }
        ControlAction::ResumeAll | ControlAction::ConfirmResume { .. } => {
            let pause = pause.ok_or("project is not paused")?;
            let revision = pause.data["hold"]["body"]["revision"]
                .as_str()
                .ok_or("pause revision missing")?
                .to_owned();
            let confirmed = matches!(&request.body.action, ControlAction::ConfirmResume { .. });
            if let ControlAction::ConfirmResume {
                request: resume,
                pause_revision,
                runs: expected,
            } = &request.body.action
            {
                let original = current_resume(live, &pause, &runs)?
                    .ok_or("resume request scope changed; request current scope again")?;
                if original.obj != *resume || pause_revision != &revision || expected != &runs {
                    return Err(
                        "resume binding changed; refresh exact revision and run scope".into(),
                    );
                }
                source_resume = Some(original);
            }
            source_pause = Some(pause);
            (
                HoldBody::Resume {
                    pause_revision: revision.clone(),
                    active_runs: runs.clone(),
                    confirmed,
                },
                Some(revision),
            )
        }
    };
    let hold = Hold {
        object_type: "Hold".into(),
        id: format!("H-{}", request.meta.request_id),
        body,
        recipients,
        opened_ms: now,
        opened_seq: 0,
        reply: None,
        waits: vec![],
        delivered: BTreeMap::new(),
        attention: false,
        extra: BTreeMap::new(),
    };
    let result = ControlResult {
        hold: hold.id.clone(),
        opening_eid: request_event_eid(&request.meta.request_id, "hold.opened")?,
        pause_revision,
        runs,
    };
    let (scope, text) = control::input_fields(&request.body.action);
    Ok(ControlRecorded {
        schema: 1,
        hold,
        scope,
        text,
        produced_action: request.body.action.clone(),
        request_body: request.body.clone(),
        p3_request: request.meta.clone(),
        p3_receipt: Some(RequestReceipt {
            request_id: request.meta.request_id.clone(),
            body_hash: request.meta.body_hash.clone(),
            result_ref: result,
        }),
        prompt: None,
        pause: source_pause,
        resume_request: source_resume,
        claims,
        source_barrier: live.human.projection().1.invalidated_through,
    })
}
fn append_control(
    live: &mut ProjectSession,
    outbox: &mut RecoveryOutbox,
    original: NewEvent,
    now: u64,
) -> Result<Event, HubError> {
    match recovery_outbox::append_durable(&mut *live.hub, outbox, original, now).map_err(
        |error| match error {
            HubError::Io(reason) => HubError::Uncertain(reason),
            other => other,
        },
    )? {
        AppendReceipt::Event(actual) => Ok(*actual),
        AppendReceipt::Duplicate { .. } => Err(HubError::Uncertain(
            "control phase duplicate lacks actual original receipt".into(),
        )),
    }
}
impl ProjectSession {
    pub fn load_interventions_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<ControlView, String> {
        let now = clock()?;
        Self::continue_shared(session, catalog, inputs, now, |live| {
            let values = active(live, now)?;
            let runs: Vec<_> = values.iter().map(|(run, _)| run.run.clone()).collect();
            let pause = current_pause(live, catalog)?;
            let pause_revision = pause
                .as_ref()
                .map(|event| {
                    event.data["hold"]["body"]["revision"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("Pause lacks revision")
                })
                .transpose()?;
            let resume_request = if let Some(pause) = &pause {
                current_resume(live, pause, &runs)?.map(|event| ResumeRequestView {
                    request: event.obj,
                    pause_revision: pause_revision.clone().unwrap(),
                    runs: runs.clone(),
                })
            } else {
                None
            };
            let ledger = live
                .baseline_requests
                .as_ref()
                .ok_or("control history missing")?;
            let (raw_holds, canonical) = live.human.projection();
            let mut messages = Vec::new();
            for (event, hold) in ledger.control_records()? {
                let current = canonical.openings.get(&hold.id).is_some_and(|opening| {
                    opening == &event && opening.seq > canonical.invalidated_through
                }) && !raw_holds.get(&hold.id).is_some_and(|raw| {
                    raw["body"]["kind"] == "pause" && raw["body"]["resumed"] == true
                });
                let Some(first) = hold.recipients.first() else {
                    if let HoldBody::Prompt { text, scope } = &hold.body {
                        messages.push(ControlMessageView {
                            id: hold.id.clone(),
                            kind: "prompt".into(),
                            text: text.clone(),
                            scope: Some(scope.clone()),
                            recipients: vec![],
                        });
                    }
                    continue;
                };
                let Some((_, text, kind)) = control::message(&hold, first) else {
                    continue;
                };
                let recipients = hold
                    .recipients
                    .iter()
                    .map(|recipient| {
                        let (id, _, _) =
                            control::message(&hold, recipient).expect("same message kind");
                        ControlRecipientView {
                            run: recipient.run.clone(),
                            task: recipient.task.clone(),
                            delivered: canonical.delivered_ids.contains(&id),
                            actionable: current
                                && values.iter().any(|(_, actual)| actual == recipient),
                        }
                    })
                    .collect();
                messages.push(ControlMessageView {
                    id: hold.id.clone(),
                    kind,
                    text,
                    scope: match &hold.body {
                        HoldBody::Prompt { scope, .. } => Some(scope.clone()),
                        _ => None,
                    },
                    recipients,
                });
            }
            Ok(ControlView {
                project_id: catalog.config.project_id().into(),
                paused: pause.is_some(),
                pause_revision,
                active_runs: values.into_iter().map(|(run, _)| run).collect(),
                resume_request,
                messages,
            })
        })
        .map_err(|e| e.to_string())?
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "fixed actor, UUID, same project outbox and immutable action remain explicit"
    )]
    pub fn control_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        actor: &str,
        id: &str,
        action: ControlAction,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<ControlOutcome, String> {
        let request =
            ControlRequest::prepare(catalog.config.project_id(), host, actor, id, action)?;
        let identity = RequestIdentity::Control(request.clone());
        let first_now = clock()?;
        let inputs = outbox.read_inputs()?;
        let first = Self::continue_shared(session, catalog, &inputs, first_now, |live| {
            let ledger = live
                .baseline_requests
                .as_mut()
                .ok_or_else(|| HubError::Invalid("control ledger missing".into()))?;
            ledger
                .reserve(identity.clone())
                .map_err(HubError::Invalid)?;
            if ledger
                .control_phase(&request)
                .map_err(HubError::Invalid)?
                .is_some_and(|(_, actual)| actual.is_some())
            {
                ledger.retry_original(catalog, &identity, &mut *live.hub, outbox, first_now)
            } else {
                Ok(None)
            }
        })
        .map_err(|e| uncertain_request(id, e))?;
        let accepted =
            Self::finish_baseline_request_shared(session, catalog, outbox, id, first, first_now)?;
        let event = if let Some(event) = accepted {
            event
        } else {
            let now = clock()?;
            if now < first_now {
                return Err(uncertain_request(id, "clock moved backwards"));
            }
            let inputs = super::human_review::new_review_inputs(session, catalog, outbox, id, now)?;
            let result=Self::continue_shared(session,catalog,&inputs,now,|live| {
                let ledger=live.baseline_requests.as_mut().ok_or_else(||HubError::Invalid("control ledger missing".into()))?;
                ledger.reserve(identity.clone()).map_err(HubError::Invalid)?;
                let phase=ledger.control_phase(&request).map_err(HubError::Invalid)?;
                if phase.as_ref().is_some_and(|(_,actual)|actual.is_some()){return ledger.retry_original(catalog,&identity,&mut *live.hub,outbox,now);}
                let prompt_phase=ledger.control_prompt_phase(&request).map_err(HubError::Invalid)?;
                // Repair this already accepted input before new eligibility.
                if let Some((original,Some(actual)))=&prompt_phase{
                    outbox.append_pending(original).map_err(HubError::Uncertain)?;
                    outbox.persist_confirmed(original,actual).map_err(HubError::Uncertain)?;
                }
                let mut fresh=build(live,catalog,&request,now).map_err(HubError::Invalid)?;
                if actor.starts_with("human:"){
                    let mut input=fresh.clone();input.p3_receipt=None;
                    let original=if let Some((original,_))=&prompt_phase{
                        let old=control::decode(catalog.config.project_id(),original).map_err(HubError::Invalid)?;
                        input.hold.opened_ms=old.hold.opened_ms;
                        if old!=input || now<old.hold.opened_ms{return Err(HubError::Uncertain("human.prompt scope/barrier changed; cannot create a new Hold from an old input".into()));}
                        original.clone()
                    }else{NewEvent{eid:request_event_eid(id,"human.prompt").map_err(HubError::Invalid)?,host:host.into(),actor:actor.into(),kind:"human.prompt".into(),layer:"L5".into(),obj:input.hold.id.clone(),guards:vec![],data:serde_json::to_value(&input).map_err(|e|HubError::Invalid(e.to_string()))?}};
                    control::decode(catalog.config.project_id(),&original).map_err(HubError::Invalid)?;
                    let actual=if let Some((_,Some(actual)))=&prompt_phase{actual.clone()}else{append_control(live,outbox,original,now)?};
                    fresh.hold.opened_ms=input.hold.opened_ms;fresh.prompt=Some(actual);
                }
                let original=if let Some((original,None))=phase {
                    let old=control::decode(catalog.config.project_id(),&original).map_err(HubError::Invalid)?;
                    if !actor.starts_with("human:"){fresh.hold.opened_ms=old.hold.opened_ms;}
                    if old!=fresh || now<old.hold.opened_ms{return Err(HubError::Uncertain("pending control scope/barrier changed; old request cannot create a new control".into()));}
                    original
                }else{NewEvent { eid:request_event_eid(id,"hold.opened").map_err(HubError::Invalid)?,host:host.into(),actor:actor.into(),kind:"hold.opened".into(),layer:"L5".into(),obj:fresh.hold.id.clone(),guards:vec![],data:serde_json::to_value(fresh).map_err(|e|HubError::Invalid(e.to_string()))? }};
                control::decode(catalog.config.project_id(),&original).map_err(HubError::Invalid)?;
                append_control(live,outbox,original,now).map(Some)
            }).map_err(|e|uncertain_request(id,e))?;
            Self::finish_baseline_request_shared(session, catalog, outbox, id, result, now)?
                .ok_or("control returned no actual receipt")?
        };
        let record = control::decode(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&event),
        )?;
        let inputs = outbox.read_inputs()?;
        let now = clock()?;
        let historical_only = Self::continue_shared(session, catalog, &inputs, now, |live| {
            let (_, canonical) = live.human.projection();
            Ok::<_, String>(
                event.stale
                    || canonical.openings.get(&event.obj) != Some(&event)
                    || event.seq <= canonical.invalidated_through,
            )
        })
        .map_err(|e| uncertain_request(id, e))??;
        Ok(ControlOutcome {
            receipt: record.p3_receipt.ok_or("final Hold lacks its receipt")?,
            historical_only,
        })
    }
}
