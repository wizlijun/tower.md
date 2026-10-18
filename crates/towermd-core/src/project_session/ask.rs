//! Source-bound Ask producers. Transport waiting belongs to the daemon; no
//! operation sleeps or holds a Session lock across Git proof work.
use super::{ProjectSession, SnapshotSessionId};
use crate::{
    ask_contract::{
        self, AskAction, AskData, AskRecipientAddedRecorded, AskRecorded, AskResultRef,
    },
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, Fence, HubError, NewEvent},
    human_intervention::{
        self, AskResult, AskState, Hold, HoldBody, HumanCommand, Recipient, Reply, WaitSlice,
    },
    recovery_contract::{
        self,
        recorded::{request_event_eid, RequestReceipt},
    },
    recovery_outbox::{self, RecoveryOutbox},
    request_ledger::{uncertain_request, AskRequest, RequestIdentity},
    tasks::{Run, Task},
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::Duration,
};

#[derive(Clone, Debug, Serialize)]
pub struct AskProgress {
    pub hold: String,
    pub receipt: Option<RequestReceipt<AskResultRef>>,
    pub current: AskResult,
    pub actionable: bool,
    pub slice: Option<WaitSlice>,
    pub wait_until: Option<u64>,
}
struct Context {
    task: Task,
    run: Run,
    session: SnapshotSessionId,
    private: Option<String>,
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
        return Err("Ask requires this host's exact active Task worktree/branch".into());
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
                || live.head.leases.get(&f.name).is_none_or(|l| {
                    l.name != f.name
                        || l.holder != f.holder
                        || l.epoch != f.epoch
                        || l.expires_at <= now
                })
        })
    {
        return Err("Ask requires complete current Task fences".into());
    }
    Ok(Context {
        task,
        run,
        session: live.snapshot_session_id(),
        private: live
            .verified_sources
            .as_ref()
            .ok_or("Ask source proof is not prepared")?
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
fn recipient(run: &Run) -> Recipient {
    Recipient {
        run: run.id.clone(),
        task: run.task.clone(),
        host: run.host.clone(),
        worktree: run.worktree.clone(),
        branch: run.branch.clone(),
        fences: run.leases.clone(),
    }
}
fn actual_worktree(catalog: &Catalog) -> Result<(), String> {
    if catalog.branch == "HEAD"
        || crate::repo::toplevel(&catalog.root)? != catalog.root
        || crate::repo::git_timeout(
            &catalog.root,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            Duration::from_secs(10),
        )? != catalog.branch
    {
        return Err("Ask actual worktree/branch changed".into());
    }
    Ok(())
}
fn active_fences(live: &ProjectSession, now: u64) -> Vec<Fence> {
    live.head
        .leases
        .values()
        .filter(|l| l.expires_at > now)
        .map(|l| l.fence())
        .collect()
}
fn request_action(command: &HumanCommand) -> Result<AskAction, String> {
    Ok(match command {
        HumanCommand::AskOpen {
            question,
            options,
            recommended,
            context,
        } => AskAction::AskOpen {
            question: question.clone(),
            options: options.clone(),
            recommended: *recommended,
            context: context.clone(),
        },
        HumanCommand::AskWait { hold } => AskAction::AskWait { hold: hold.clone() },
        HumanCommand::Relay {
            hold,
            text,
            self_reported,
        } => AskAction::Relay {
            hold: hold.clone(),
            text: text.clone(),
            self_reported: self_reported.clone(),
        },
        _ => return Err("Ask producer accepts only AskOpen, AskWait and Relay".into()),
    })
}
fn append(
    live: &mut ProjectSession,
    outbox: &mut RecoveryOutbox,
    event: NewEvent,
    now: u64,
) -> Result<Option<Event>, HubError> {
    ask_contract::decode(
        live.identity
            .as_ref()
            .ok_or_else(|| HubError::Invalid("Ask project not bound".into()))?
            .0
            .as_str(),
        &event,
    )
    .map_err(HubError::Invalid)?;
    match recovery_outbox::append_durable(&mut *live.hub, outbox, event, now)? {
        AppendReceipt::Event(event) => Ok(Some(*event)),
        AppendReceipt::Duplicate { .. } => Err(HubError::Uncertain(
            "Ask duplicate lacks actual original receipt".into(),
        )),
    }
}
fn retry(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    request: &AskRequest,
    now: u64,
) -> Result<Option<Event>, String> {
    let id = &request.meta.request_id;
    let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
    let identity = RequestIdentity::Ask(request.clone());
    let result = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        let ledger = live
            .baseline_requests
            .as_mut()
            .ok_or_else(|| HubError::Invalid("Ask request history not prepared".into()))?;
        ledger
            .reserve(identity.clone())
            .map_err(HubError::Invalid)?;
        ledger.retry_original(catalog, &identity, &mut *live.hub, outbox, now)
    })
    .map_err(|e| uncertain_request(id, e))?;
    ProjectSession::finish_baseline_request_shared(session, catalog, outbox, id, result, now)
}
fn effective(live: &ProjectSession, id: &str) -> Result<(Hold, Value, Event), String> {
    let (raw, canonical) = live.human.projection();
    let opening = canonical
        .openings
        .get(id)
        .ok_or("Ask lacks an actual outer opening")?;
    let hold = live.human.holds.get(id).ok_or("Ask Hold not found")?;
    if !matches!(&hold.body, HoldBody::Ask { .. }) {
        return Err("operation requires an Ask, not Review/Prompt/Pause".into());
    }
    if opening.stale || opening.seq <= canonical.invalidated_through {
        return Err("Ask is historical after hub rebuild".into());
    }
    Ok((
        hold.clone(),
        raw.get(id).ok_or("Ask lacks raw effective Hold")?.clone(),
        opening.clone(),
    ))
}
fn result_ref(
    hold: &Hold,
    opening: &str,
    recipient: Option<&Recipient>,
    active: &[Fence],
    now: u64,
    slice: Option<WaitSlice>,
) -> AskResultRef {
    AskResultRef {
        hold: hold.id.clone(),
        opening_eid: opening.into(),
        recipient: recipient.cloned(),
        state: recipient
            .map(|r| human_intervention::ask_state(hold, &r.run, active, now))
            .unwrap_or(AskState::Expired),
        reply: hold.reply.clone(),
        slice,
    }
}
#[allow(
    clippy::too_many_arguments,
    reason = "the finite Ask envelope keeps each frozen request field explicit"
)]
fn event(
    request: &AskRequest,
    kind: &str,
    raw: Value,
    opening: &str,
    recipient: Option<Recipient>,
    slice: Option<WaitSlice>,
    planned_reply: Option<Reply>,
    terminal: bool,
    active: &[Fence],
    now: u64,
) -> Result<NewEvent, String> {
    let hold: Hold = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
    let result = result_ref(
        &hold,
        opening,
        recipient.as_ref(),
        active,
        now,
        slice.clone(),
    );
    let receipt = terminal.then(|| RequestReceipt {
        request_id: request.meta.request_id.clone(),
        body_hash: request.meta.body_hash.clone(),
        result_ref: result,
    });
    let record = AskRecorded {
        hold: serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?,
        opening_eid: opening.into(),
        recipient: recipient.clone(),
        slice,
        request_body: request.body.clone(),
        p3_request: request.meta.clone(),
        p3_receipt: receipt,
        planned_reply,
    };
    let mut data = serde_json::to_value(record).map_err(|e| e.to_string())?;
    data["hold"] = raw; // Preserve omitted/unknown keys rather than round-tripping raw Hold.
    let system = kind == "hold.wait_finished" || kind == "hold.attention";
    Ok(NewEvent {
        eid: request_event_eid(&request.meta.request_id, kind)?,
        host: request.body.host.clone(),
        actor: if system {
            "process:towermd-daemon".into()
        } else if kind == "hold.answered"
            && matches!(&request.body.action, AskAction::AskOpen { .. })
        {
            "process:towermd-dedupe".into()
        } else {
            request.body.actor.clone()
        },
        kind: kind.into(),
        layer: "L5".into(),
        obj: hold.id,
        data,
        guards: if system {
            vec![]
        } else {
            recipient.map(|r| r.fences).unwrap_or_default()
        },
    })
}
fn progress(
    live: &ProjectSession,
    catalog: &Catalog,
    actual: &Event,
    now: u64,
) -> Result<AskProgress, String> {
    let record = ask_contract::decode(
        actual.data["request_body"]["project_id"]
            .as_str()
            .ok_or("Ask receipt lacks project")?,
        &recovery_outbox::original_of(actual),
    )?;
    let frozen = record.hold();
    let recipient = match &record {
        AskData::Ordinary(r) => r.recipient.as_ref(),
        AskData::Added(r) => Some(&r.added),
    };
    let (hold, current_valid) = match effective(live, &frozen.id) {
        Ok((h, _, _)) => (h, true),
        Err(_) => (
            serde_json::from_value::<Hold>(actual.data["hold"].clone())
                .map_err(|e| e.to_string())?,
            false,
        ),
    };
    let active = if current_valid
        && recipient.is_some_and(|r| context(live, catalog, &r.host, Some(&r.run), now).is_ok())
    {
        active_fences(live, now)
    } else {
        vec![]
    };
    let state = recipient
        .map(|r| human_intervention::ask_state(&hold, &r.run, &active, now))
        .unwrap_or(AskState::Expired);
    let observed = match &record {
        AskData::Ordinary(r) => r.slice.as_ref(),
        AskData::Added(_) => None,
    };
    let slice = observed.map(|s| {
        hold.waits
            .iter()
            .find(|current| current.id == s.id)
            .unwrap_or(s)
            .clone()
    });
    let wait_until = if state == AskState::Pending && hold.reply.is_none() {
        slice
            .as_ref()
            .filter(|s| s.finished_ms.is_none() && s.deadline_ms > now)
            .map(|s| s.deadline_ms)
    } else {
        None
    };
    Ok(AskProgress {
        hold: hold.id.clone(),
        receipt: record.receipt().cloned(),
        current: AskResult {
            hold: hold.id,
            state: state.clone(),
            reply: hold.reply,
        },
        actionable: state != AskState::Expired,
        slice,
        wait_until,
    })
}
fn new_plan(
    live: &ProjectSession,
    catalog: &Catalog,
    request: &AskRequest,
    prepared: Option<&Context>,
    now: u64,
) -> Result<NewEvent, String> {
    let active = active_fences(live, now);
    match &request.body.action {
        AskAction::AskOpen {
            question,
            options,
            recommended,
            context,
        } => {
            let prepared = prepared.ok_or("AskOpen needs an active context")?;
            let to = recipient(&prepared.run);
            let module = &prepared.task.spec.module;
            let planned = human_intervention::decision_answer(catalog, module, question, now)?;
            // The authoritative Decision lookup precedes session-question reuse.
            if planned.is_none() {
                let fingerprint = human_intervention::fingerprint(module, question);
                let reused=live.human.holds.values().filter(|h|h.reply.is_none()).filter(|h|matches!(&h.body,HoldBody::Ask{fingerprint:old,module:old_module,..} if old==&fingerprint && old_module==module)).filter_map(|h|effective(live,&h.id).ok()).min_by_key(|(h,_,_)|h.opened_seq);
                if let Some((hold, mut raw, opening)) = reused {
                    if let Some(previous) = hold.recipients.iter().find(|r| r.run == to.run) {
                        if previous != &to {
                            return Err("Ask cannot replace the same Run's task/host/worktree/branch/fences".into());
                        }
                        return event(
                            request,
                            "hold.requested",
                            raw,
                            &opening.eid,
                            Some(to),
                            None,
                            None,
                            true,
                            &active,
                            now,
                        );
                    }
                    raw["recipients"]
                        .as_array_mut()
                        .ok_or("Ask recipients missing")?
                        .push(json!(to));
                    let record = AskRecipientAddedRecorded {
                        hold: serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?,
                        added: to.clone(),
                        opening_eid: opening.eid.clone(),
                        request_body: request.body.clone(),
                        p3_request: request.meta.clone(),
                        p3_receipt: RequestReceipt {
                            request_id: request.meta.request_id.clone(),
                            body_hash: request.meta.body_hash.clone(),
                            result_ref: AskResultRef {
                                hold: hold.id.clone(),
                                opening_eid: opening.eid,
                                recipient: Some(to.clone()),
                                state: AskState::Pending,
                                reply: None,
                                slice: None,
                            },
                        },
                    };
                    let mut data = serde_json::to_value(record).map_err(|e| e.to_string())?;
                    data["hold"] = raw;
                    return Ok(NewEvent {
                        eid: request_event_eid(&request.meta.request_id, "hold.recipient_added")?,
                        host: request.body.host.clone(),
                        actor: request.body.actor.clone(),
                        kind: "hold.recipient_added".into(),
                        layer: "L5".into(),
                        obj: hold.id,
                        data,
                        guards: to.fences,
                    });
                }
            }
            let opening = request_event_eid(&request.meta.request_id, "hold.opened")?;
            let attention_id = uuid::Uuid::now_v7().to_string();
            live.baseline_requests
                .as_ref()
                .ok_or("Ask ledger missing")?
                .require_unused_request_id(&attention_id)?;
            let raw = json!({"type":"Hold","id":format!("H-{}",request.meta.request_id),"body":{"kind":"ask","fingerprint":human_intervention::fingerprint(module,question),"question":question,"module":module,"options":options,"recommended":recommended,"context":context},"recipients":[to],"opened_ms":now,"opened_seq":0,"reply":null,"waits":[],"delivered":{},"attention":false,"attention_request_id":attention_id});
            event(
                request,
                "hold.opened",
                raw,
                &opening,
                Some(to),
                None,
                planned.clone(),
                planned.is_none(),
                &active,
                now,
            )
        }
        AskAction::AskWait { hold: id } => {
            let (hold, mut raw, opening) = effective(live, id)?;
            let to = if let Some(prepared) = prepared {
                Some(recipient(&prepared.run))
            } else {
                // No active Run: only the exact actual historical Run of this
                // worktree can be read. This never acquires or renews it.
                let candidates: Vec<_> = hold
                    .recipients
                    .iter()
                    .filter(|r| {
                        r.host == request.body.host
                            && r.worktree == catalog.root.to_string_lossy()
                            && r.branch == catalog.branch
                            && request.body.run.as_ref().is_none_or(|id| id == &r.run)
                    })
                    .filter(|r| {
                        live.tasks
                            .ask_recorded_run(&r.run)
                            .is_some_and(|run| recipient(&run) == **r)
                    })
                    .cloned()
                    .collect();
                if candidates.len() != 1 {
                    return Err("AskWait has no unambiguous actual Run binding".into());
                }
                candidates.first().cloned()
            };
            let active = if prepared.is_some() { active } else { vec![] };
            let state = to
                .as_ref()
                .map(|r| human_intervention::ask_state(&hold, &r.run, &active, now))
                .unwrap_or(AskState::Expired);
            if state != AskState::Pending {
                return event(
                    request,
                    "hold.requested",
                    raw,
                    &opening.eid,
                    to,
                    None,
                    None,
                    true,
                    &active,
                    now,
                );
            }
            let to = to.ok_or("AskWait recipient missing")?;
            if let Some(slice) = hold.waits.iter().find(|s| {
                s.recipient_run == to.run && s.finished_ms.is_none() && s.deadline_ms > now
            }) {
                return event(
                    request,
                    "hold.requested",
                    raw,
                    &opening.eid,
                    Some(to),
                    Some(slice.clone()),
                    None,
                    true,
                    &active,
                    now,
                );
            }
            let slice =
                human_intervention::next_slice(&hold, &to.run, now, &request.meta.request_id)?
                    .ok_or("Ask cannot start this bounded slice")?;
            raw["waits"]
                .as_array_mut()
                .ok_or("Ask waits missing")?
                .push(json!(slice));
            event(
                request,
                "hold.wait_started",
                raw,
                &opening.eid,
                Some(to),
                Some(slice),
                None,
                false,
                &active,
                now,
            )
        }
        AskAction::Relay {
            hold: id,
            text,
            self_reported,
        } => {
            let prepared = prepared.ok_or("relay needs the current live Run")?;
            let to = recipient(&prepared.run);
            let (hold, mut raw, opening) = effective(live, id)?;
            if !hold.recipients.contains(&to) {
                return Err("relay Run is not an explicit Ask recipient".into());
            }
            if hold.reply.is_some() {
                return Err("Ask already has its first answer".into());
            }
            let reply = Reply {
                id: request_event_eid(&request.meta.request_id, "hold.answered")?,
                text: text.clone(),
                actor: "process:towermd-cli".into(),
                relayed_from: Some("human".into()),
                self_reported: self_reported.clone(),
                decision: None,
                decision_revision: None,
                at_ms: now,
                extra: BTreeMap::new(),
            };
            install_reply(&mut raw, &reply)?;
            event(
                request,
                "hold.answered",
                raw,
                &opening.eid,
                Some(to),
                None,
                None,
                true,
                &active,
                now,
            )
        }
        AskAction::Attention { .. } => Err("internal tick intent is not an Ask input".into()),
    }
}
fn install_reply(raw: &mut Value, reply: &Reply) -> Result<(), String> {
    raw["reply"] = json!(reply);
    for slice in raw["waits"].as_array_mut().ok_or("Ask waits missing")? {
        if slice["finished_ms"].is_null() {
            slice["finished_ms"] = json!(reply.at_ms.min(
                slice["deadline_ms"]
                    .as_u64()
                    .ok_or("Ask slice deadline missing")?
            ));
        }
    }
    Ok(())
}
fn renew(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    request: &AskRequest,
    outbox: &mut RecoveryOutbox,
    now: u64,
    resolved: Option<&str>,
) -> Result<Context, String> {
    let id = &request.meta.request_id;
    let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
    actual_worktree(catalog).map_err(|e| uncertain_request(id, e))?;
    let (result, private) =
        ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
            let prepared = context(
                live,
                catalog,
                &request.body.host,
                resolved.or(request.body.run.as_deref()),
                now,
            )?;
            Ok::<_, String>((
                live.renew_binding(catalog, &request.body.host, Some(&prepared.run.id), now),
                prepared.private,
            ))
        })
        .map_err(|e| uncertain_request(id, e))??;
    ProjectSession::finish_shared(session, catalog, &inputs, now, &private, result)
        .map_err(|e| uncertain_request(id, e))?;
    let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
    ProjectSession::continue_shared_bound(session, catalog, &inputs, now, Some(&private), |live| {
        context(
            live,
            catalog,
            &request.body.host,
            resolved.or(request.body.run.as_deref()),
            now,
        )
    })
    .map_err(|e| uncertain_request(id, e))?
}
fn finish_decision(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    request: &AskRequest,
    opened: &Event,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<Event, String> {
    let id = &request.meta.request_id;
    let record = ask_contract::decode(
        catalog.config.project_id(),
        &recovery_outbox::original_of(opened),
    )?;
    let AskData::Ordinary(record) = record else {
        return Err("Decision opening has wrong original shape".into());
    };
    let planned = record
        .planned_reply
        .as_ref()
        .ok_or("Decision opening lacks planned Reply")?;
    if opened.stale {
        return Err("stale Ask opening cannot authorize a Decision answer".into());
    }
    let now = clock().map_err(|e| uncertain_request(id, e))?;
    if now < planned.at_ms {
        return Err(uncertain_request(
            id,
            "clock moved backwards after Decision opening",
        ));
    }
    let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
    actual_worktree(catalog).map_err(|e| uncertain_request(id, e))?;
    let prepared = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        context(
            live,
            catalog,
            &request.body.host,
            record.recipient.as_ref().map(|r| r.run.as_str()),
            now,
        )
    })
    .map_err(|e| uncertain_request(id, e))??;
    let final_now = clock().map_err(|e| uncertain_request(id, e))?;
    if final_now < now {
        return Err(uncertain_request(
            id,
            "clock moved backwards before Decision answer",
        ));
    }
    actual_worktree(catalog).map_err(|e| uncertain_request(id, e))?;
    let result = ProjectSession::continue_shared_bound(
        session,
        catalog,
        &inputs,
        final_now,
        Some(&prepared.private),
        |live| {
            // A concurrent original retry may already have finished the request.
            let saved = live
                .baseline_requests
                .as_ref()
                .ok_or_else(|| HubError::Invalid("Ask ledger missing".into()))?
                .retry_original(
                    catalog,
                    &RequestIdentity::Ask(request.clone()),
                    &mut *live.hub,
                    outbox,
                    final_now,
                )?;
            if let Some(saved) = saved {
                if saved.kind != "hold.opened" {
                    return Ok(Some(saved));
                }
            }
            let current = context(
                live,
                catalog,
                &request.body.host,
                Some(&prepared.run.id),
                final_now,
            )
            .map_err(HubError::Invalid)?;
            if !same_context(&prepared, &current).map_err(HubError::Invalid)?
                || recipient(&current.run)
                    != record.recipient.clone().ok_or_else(|| {
                        HubError::Invalid("Decision opening lacks recipient".into())
                    })?
            {
                return Err(HubError::Invalid(
                    "Decision original Run binding changed".into(),
                ));
            }
            let (hold, mut raw, opening) =
                effective(live, &record.hold.id).map_err(HubError::Invalid)?;
            if opening != *opened || hold.reply.is_some() {
                return Err(HubError::Invalid(
                    "Decision original Ask is no longer unanswered/current".into(),
                ));
            }
            let mut current = human_intervention::decision_answer(
                catalog,
                &record.hold.body.module,
                &record.hold.body.question,
                final_now,
            )
            .map_err(HubError::Invalid)?
            .ok_or_else(|| HubError::Invalid("planned Decision is no longer current".into()))?;
            current.at_ms = planned.at_ms;
            if current != *planned {
                return Err(HubError::Invalid(
                    "planned Decision source/answer changed; original request retained".into(),
                ));
            }
            install_reply(&mut raw, planned).map_err(HubError::Invalid)?;
            let original = event(
                request,
                "hold.answered",
                raw,
                &opening.eid,
                record.recipient.clone(),
                None,
                Some(planned.clone()),
                true,
                &active_fences(live, final_now),
                final_now,
            )
            .map_err(HubError::Invalid)?;
            append(live, outbox, original, final_now)
        },
    )
    .map_err(|e| uncertain_request(id, e))?;
    ProjectSession::finish_baseline_request_shared(session, catalog, outbox, id, result, final_now)?
        .ok_or("Decision answer lacks actual receipt".into())
}
impl ProjectSession {
    /// One finite Ask operation. A started wait returns a bounded continuation;
    /// the daemon must wait without this mutex and call again with the same UUID.
    #[allow(clippy::too_many_arguments)]
    pub fn ask_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        request_id: &str,
        command: &HumanCommand,
        run: Option<&str>,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<AskProgress, String> {
        let input_clock = clock;
        let observed = std::cell::Cell::new(None);
        let clock = || {
            let now = input_clock()?;
            if observed.get().is_some_and(|before| now < before) {
                return Err("clock moved backwards during Ask".into());
            }
            observed.set(Some(now));
            Ok(now)
        };
        let request = AskRequest::prepare(
            catalog.config.project_id(),
            host,
            request_id,
            request_action(command)?,
            run,
        )?;
        let started = clock().map_err(|e| uncertain_request(request_id, e))?;
        let original = retry(session, catalog, outbox, &request, started)?;
        let mut actual = if let Some(original) = original {
            original
        } else {
            // Only genuinely new requests reach maintenance and binding.
            let inputs = outbox
                .read_inputs()
                .map_err(|e| uncertain_request(request_id, e))?;
            if Self::task_reconciliation_needed_shared(session, catalog, &inputs)
                .map_err(|e| uncertain_request(request_id, e))?
            {
                Self::reconcile_pending_shared(session, catalog, &inputs, started, outbox)
                    .map_err(|e| uncertain_request(request_id, e))?;
            }
            Self::human_tick_shared(session, catalog, host, outbox, clock)
                .map_err(|e| uncertain_request(request_id, e))?;
            let inputs = outbox
                .read_inputs()
                .map_err(|e| uncertain_request(request_id, e))?;
            Self::tick_shared(session, catalog, &inputs, host, started, outbox)
                .map_err(|e| uncertain_request(request_id, e))?;
            let inputs = outbox
                .read_inputs()
                .map_err(|e| uncertain_request(request_id, e))?;
            let binding = Self::continue_shared(session, catalog, &inputs, started, |live| {
                context(live, catalog, host, run, started)
            })
            .map_err(|e| uncertain_request(request_id, e))?;
            let prepared = match binding {
                Ok(_) => Some(renew(session, catalog, &request, outbox, started, None)?),
                Err(_) if matches!(&request.body.action, AskAction::AskWait { .. }) => None,
                Err(reason) => return Err(reason),
            };
            let now = clock().map_err(|e| uncertain_request(request_id, e))?;
            if now < started {
                return Err(uncertain_request(
                    request_id,
                    "clock moved backwards before Ask append",
                ));
            }
            actual_worktree(catalog).map_err(|e| uncertain_request(request_id, e))?;
            let inputs = outbox
                .read_inputs()
                .map_err(|e| uncertain_request(request_id, e))?;
            let private = prepared.as_ref().map(|c| &c.private);
            let result =
                Self::continue_shared_bound(session, catalog, &inputs, now, private, |live| {
                    let identity = RequestIdentity::Ask(request.clone());
                    if let Some(saved) = live
                        .baseline_requests
                        .as_ref()
                        .ok_or_else(|| HubError::Invalid("Ask ledger missing".into()))?
                        .retry_original(catalog, &identity, &mut *live.hub, outbox, now)?
                    {
                        return Ok(Some(saved));
                    }
                    if let Some(prepared) = &prepared {
                        let current = context(live, catalog, host, Some(&prepared.run.id), now)
                            .map_err(HubError::Invalid)?;
                        if !same_context(prepared, &current).map_err(HubError::Invalid)? {
                            return Err(HubError::Invalid(
                                "Ask context changed before append".into(),
                            ));
                        }
                    }
                    let original = new_plan(live, catalog, &request, prepared.as_ref(), now)
                        .map_err(HubError::Invalid)?;
                    append(live, outbox, original, now)
                })
                .map_err(|e| uncertain_request(request_id, e))?;
            Self::finish_baseline_request_shared(session, catalog, outbox, request_id, result, now)?
                .ok_or("Ask append did not return actual receipt")?
        };
        let decoded = ask_contract::decode(
            catalog.config.project_id(),
            &recovery_outbox::original_of(&actual),
        )?;
        if decoded.receipt().is_none() {
            let now = clock().map_err(|e| uncertain_request(request_id, e))?;
            let inputs = outbox
                .read_inputs()
                .map_err(|e| uncertain_request(request_id, e))?;
            let historical = Self::continue_shared(session, catalog, &inputs, now, |live| {
                if actual.stale || effective(live, &decoded.hold().id).is_err() {
                    progress(live, catalog, &actual, now).map(Some)
                } else {
                    Ok(None)
                }
            })
            .map_err(|e| uncertain_request(request_id, e))??;
            if let Some(historical) = historical {
                // A verified original phase is readable after rebuild, but it
                // has no terminal receipt and cannot resume Decision/wait work.
                return Ok(historical);
            }
            if actual.kind == "hold.opened" {
                actual = finish_decision(session, catalog, outbox, &request, &actual, &clock)?;
            } else if actual.kind == "hold.wait_started" {
                let AskData::Ordinary(record) = decoded else {
                    return Err("Ask wait phase has wrong shape".into());
                };
                let now = clock().map_err(|e| uncertain_request(request_id, e))?;
                if now < started {
                    return Err(uncertain_request(
                        request_id,
                        "clock moved backwards while waiting",
                    ));
                }
                // Frozen original recipient, never resolve a later Run for an
                // original implicit-run request. Expiry only changes readability.
                if let Some(to) = &record.recipient {
                    let inputs = outbox
                        .read_inputs()
                        .map_err(|e| uncertain_request(request_id, e))?;
                    let live = Self::continue_shared(session, catalog, &inputs, now, |s| {
                        context(s, catalog, host, Some(&to.run), now).is_ok()
                    })
                    .map_err(|e| uncertain_request(request_id, e))?;
                    if live {
                        renew(session, catalog, &request, outbox, now, Some(&to.run))
                            .map_err(|e| uncertain_request(request_id, e))?;
                    }
                }
                Self::human_tick_shared(session, catalog, host, outbox, clock)
                    .map_err(|e| uncertain_request(request_id, e))?;
                actual = retry(session, catalog, outbox, &request, now)?
                    .ok_or("wait lost its durable original")?;
            }
        }
        // An observer's immutable no-op receipt may still be waiting on the
        // original slice. Renew only that frozen live recipient while pending;
        // never substitute a newer Run or keep a deferred run alive.
        if actual.kind == "hold.requested" {
            let observer = ask_contract::decode(
                catalog.config.project_id(),
                &recovery_outbox::original_of(&actual),
            )?;
            if let AskData::Ordinary(record) = observer {
                if matches!(&record.request_body.action, AskAction::AskWait { .. })
                    && record.slice.is_some()
                {
                    let now = clock().map_err(|e| uncertain_request(request_id, e))?;
                    let inputs = outbox
                        .read_inputs()
                        .map_err(|e| uncertain_request(request_id, e))?;
                    let waiting = Self::continue_shared(session, catalog, &inputs, now, |live| {
                        progress(live, catalog, &actual, now).map(|p| p.wait_until.is_some())
                    })
                    .map_err(|e| uncertain_request(request_id, e))??;
                    if waiting {
                        let to = record
                            .recipient
                            .as_ref()
                            .ok_or("wait observer lacks original recipient")?;
                        renew(session, catalog, &request, outbox, now, Some(&to.run))
                            .map_err(|e| uncertain_request(request_id, e))?;
                    }
                }
            }
        }
        let now = clock().map_err(|e| uncertain_request(request_id, e))?;
        if now < started {
            return Err(uncertain_request(
                request_id,
                "clock moved backwards before Ask result",
            ));
        }
        let inputs = outbox
            .read_inputs()
            .map_err(|e| uncertain_request(request_id, e))?;
        Self::continue_shared(session, catalog, &inputs, now, |live| {
            progress(live, catalog, &actual, now)
        })
        .map_err(|e| uncertain_request(request_id, e))?
    }

    /// Settle only proved old slices/attention. This never renews module leases.
    pub fn human_tick_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<(), String> {
        crate::hub::validate_token("host", host).map_err(|e| e.to_string())?;
        let now = clock()?;
        let inputs = outbox.read_inputs()?;
        let intents = Self::continue_shared(session, catalog, &inputs, now, |live| {
            let ledger = live
                .baseline_requests
                .as_ref()
                .ok_or("Ask ledger missing")?;
            Ok::<_, String>(
                ledger
                    .ask_tick_intents(&inputs)
                    .into_iter()
                    .filter(|intent| {
                        let request = match intent {
                            crate::request_ledger::AskTickIntent::ConfirmAccepted(_) => {
                                return true
                            }
                            crate::request_ledger::AskTickIntent::Current(request) => request,
                        };
                        let hold = match &request.body.action {
                            AskAction::AskWait { hold } | AskAction::Attention { hold } => hold,
                            _ => return false,
                        };
                        let Ok((current, _, _)) = effective(live, hold) else {
                            return false;
                        };
                        !matches!(&request.body.action, AskAction::AskWait { .. })
                            || current
                                .waits
                                .iter()
                                .any(|slice| slice.id == request.meta.request_id)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .map_err(|e| e.to_string())??;
        for intent in intents {
            let request = match intent {
                crate::request_ledger::AskTickIntent::ConfirmAccepted(request) => {
                    // Known original acceptance repairs only its fsynced receipt.
                    // Historical or answered Holds cannot turn this into a new event.
                    retry(session, catalog, outbox, &request, now)?;
                    continue;
                }
                crate::request_ledger::AskTickIntent::Current(request) => request,
            };
            let Some(started) = retry(session, catalog, outbox, &request, now)? else {
                continue;
            };
            if started.kind != "hold.wait_started" || started.stale {
                continue;
            }
            settle_wait(session, catalog, outbox, &request, &started, now)?;
        }
        let inputs = outbox.read_inputs()?;
        let candidates = Self::continue_shared(session, catalog, &inputs, now, |live| {
            live.human
                .holds
                .values()
                .filter(|h| {
                    h.reply.is_none()
                        && !h.attention
                        && now >= h.opened_ms.saturating_add(1_800_000)
                })
                .filter_map(|h| {
                    let (_, _, opening) = effective(live, &h.id).ok()?;
                    if !ask_contract::is_ask_request(&opening.kind, &opening.data) {
                        return None;
                    }
                    let id = opening.data["hold"]["attention_request_id"].as_str()?;
                    if request_event_eid(id, "hold.attention").is_err()
                        || opening.data["p3_request"]["request_id"] == id
                    {
                        return None;
                    }
                    Some((
                        h.id.clone(),
                        id.to_owned(),
                        opening.host.clone(),
                        opening.eid,
                    ))
                })
                .collect::<Vec<_>>()
        })
        .map_err(|e| e.to_string())?;
        for (hold, id, origin_host, opening_eid) in candidates {
            let request = AskRequest::prepare(
                catalog.config.project_id(),
                &origin_host,
                &id,
                AskAction::Attention { hold: hold.clone() },
                None,
            )?;
            if retry(session, catalog, outbox, &request, now)?.is_some() {
                continue;
            }
            let inputs = outbox
                .read_inputs()
                .map_err(|e| uncertain_request(&id, e))?;
            let result = Self::continue_shared(session, catalog, &inputs, now, |live| {
                if let Some(actual) = live
                    .baseline_requests
                    .as_ref()
                    .ok_or_else(|| HubError::Invalid("Ask ledger missing".into()))?
                    .retry_original(
                        catalog,
                        &RequestIdentity::Ask(request.clone()),
                        &mut *live.hub,
                        outbox,
                        now,
                    )?
                {
                    return Ok(Some(actual));
                }
                let (current, mut raw, opening) =
                    effective(live, &hold).map_err(HubError::Invalid)?;
                if current.reply.is_some() || current.attention {
                    return Ok(None);
                }
                if now < current.opened_ms.saturating_add(1_800_000)
                    || opening.eid != opening_eid
                    || opening.data["hold"]["attention_request_id"] != id
                {
                    return Err(HubError::Invalid(
                        "attention original opening/threshold changed".into(),
                    ));
                }
                raw["attention"] = json!(true);
                let original = event(
                    &request,
                    "hold.attention",
                    raw,
                    &opening.eid,
                    None,
                    None,
                    None,
                    true,
                    &[],
                    now,
                )
                .map_err(HubError::Invalid)?;
                append(live, outbox, original, now)
            })
            .map_err(|e| uncertain_request(&id, e))?;
            Self::finish_baseline_request_shared(session, catalog, outbox, &id, result, now)?;
        }
        Ok(())
    }
}
fn settle_wait(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    outbox: &mut RecoveryOutbox,
    request: &AskRequest,
    started: &Event,
    now: u64,
) -> Result<(), String> {
    let record = ask_contract::decode(
        catalog.config.project_id(),
        &recovery_outbox::original_of(started),
    )?;
    let AskData::Ordinary(record) = record else {
        return Err("wait started shape invalid".into());
    };
    let id = &request.meta.request_id;
    let inputs = outbox.read_inputs().map_err(|e| uncertain_request(id, e))?;
    let result = ProjectSession::continue_shared(session, catalog, &inputs, now, |live| {
        if let Some(actual) = live
            .baseline_requests
            .as_ref()
            .ok_or_else(|| HubError::Invalid("Ask ledger missing".into()))?
            .retry_original(
                catalog,
                &RequestIdentity::Ask(request.clone()),
                &mut *live.hub,
                outbox,
                now,
            )?
        {
            if actual.kind != "hold.wait_started" {
                return Ok(Some(actual));
            }
        }
        let (hold, mut raw, opening) = match effective(live, &record.hold.id) {
            Ok(found) => found,
            Err(_) => return Ok(None),
        };
        let before = record
            .slice
            .as_ref()
            .ok_or_else(|| HubError::Invalid("wait started has no slice".into()))?;
        let slice = hold
            .waits
            .iter()
            .find(|s| s.id == id.as_str())
            .ok_or_else(|| HubError::Invalid("wait lacks actual effective slice".into()))?;
        let mut expected = before.clone();
        expected.finished_ms = slice.finished_ms;
        if &expected != slice || record.opening_eid != opening.eid {
            return Err(HubError::Invalid(
                "wait original opening/slice changed".into(),
            ));
        }
        if hold.reply.is_none() && now < slice.deadline_ms {
            return Ok(None);
        }
        let mut finished = slice.clone();
        finished.finished_ms = Some(
            hold.reply
                .as_ref()
                .map(|r| r.at_ms.min(slice.deadline_ms))
                .unwrap_or(slice.deadline_ms),
        );
        for value in raw["waits"]
            .as_array_mut()
            .ok_or_else(|| HubError::Invalid("Ask waits missing".into()))?
        {
            if value["id"] == id.as_str() {
                value["finished_ms"] = json!(finished.finished_ms);
            }
        }
        let original = event(
            request,
            "hold.wait_finished",
            raw,
            &opening.eid,
            record.recipient.clone(),
            Some(finished),
            None,
            true,
            &active_fences(live, now),
            now,
        )
        .map_err(HubError::Invalid)?;
        append(live, outbox, original, now)
    })
    .map_err(|e| uncertain_request(id, e))?;
    ProjectSession::finish_baseline_request_shared(session, catalog, outbox, id, result, now)?;
    Ok(())
}

#[cfg(test)]
#[path = "ask_tests.rs"]
mod tests;
