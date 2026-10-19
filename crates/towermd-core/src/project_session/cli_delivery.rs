//! Source-bound inbox and delivery groups over the one original-request ledger.
use super::ProjectSession;
use crate::{
    catalog::Catalog,
    cli_delivery::{
        self, DeliveryAction, DeliveryGroup, DeliveryGroupResult, DeliveryOutcome,
        DeliveryRecorded, DeliveryResult, InboxActivity, InboxBatch, InboxReadFailure,
        PreparedDeliveryRequest,
    },
    hub_protocol::{AppendReceipt, Event, HubError, NewEvent},
    human_intervention::{Hold, Recipient},
    recovery_contract::recorded::RequestReceipt,
    recovery_outbox::{self, RecoveryOutbox},
    request_ledger::{uncertain_request, RequestIdentity},
    tasks::Run,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

#[cfg(test)]
thread_local! {
    static ACTUAL_WORKTREE_FULL_LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn actual_worktree(catalog: &Catalog) -> Result<(), String> {
    if catalog.matches_current_source().unwrap_or(false) {
        return Ok(());
    }
    // Equality only decides reuse. Probe failure or drift still runs the old
    // independent full read, including its original rejection conditions.
    #[cfg(test)]
    ACTUAL_WORKTREE_FULL_LOADS.with(|count| count.set(count.get() + 1));
    let actual = Catalog::load(&catalog.root)?;
    if actual.root != catalog.root
        || actual.common_dir != catalog.common_dir
        || actual.branch != catalog.branch
        || actual.revision != catalog.revision
        || actual.config.to_yaml() != catalog.config.to_yaml()
    {
        return Err("inbox actual project/worktree/branch/source changed".into());
    }
    Ok(())
}
fn selected_runs(
    live: &ProjectSession,
    catalog: &Catalog,
    host: &str,
    selector: Option<&str>,
) -> Result<Vec<Run>, String> {
    let mut runs = Vec::new();
    for raw in live
        .projection()?
        .objects
        .get("Run")
        .into_iter()
        .flat_map(|runs| runs.values())
    {
        let run: Run = serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?;
        if run.host == host
            && run.worktree == catalog.root.to_string_lossy()
            && run.branch == catalog.branch
            && selector.is_none_or(|id| id == run.id)
        {
            runs.push(run);
        }
    }
    // A recovered old claim is only a delivery recipient, never a current Run
    // or a reason to renew/reacquire a lease or install business projection.
    for run in live
        .baseline_requests
        .as_ref()
        .ok_or("inbox history is not prepared")?
        .delivery_archived_runs()?
    {
        if run.host == host
            && run.worktree == catalog.root.to_string_lossy()
            && run.branch == catalog.branch
            && selector.is_none_or(|id| id == run.id)
            && !runs.iter().any(|current| current.id == run.id)
        {
            runs.push(run);
        }
    }
    if selector.is_some() && runs.is_empty() {
        return Err("inbox run is not recorded on this host/worktree/branch".into());
    }
    runs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(runs)
}
fn as_recipient(run: &Run) -> Recipient {
    Recipient {
        run: run.id.clone(),
        task: run.task.clone(),
        host: run.host.clone(),
        worktree: run.worktree.clone(),
        branch: run.branch.clone(),
        fences: run.leases.clone(),
    }
}
fn capture(
    live: &ProjectSession,
    catalog: &Catalog,
    host: &str,
    selector: Option<&str>,
    now: u64,
) -> Result<InboxBatch, String> {
    let runs = selected_runs(live, catalog, host, selector)?;
    let recipients: Vec<_> = runs.iter().map(as_recipient).collect();
    let mut deadline_fences = Vec::new();
    for run in &runs {
        if crate::finish::run_deadline(run)?.is_some_and(|deadline| now >= deadline) {
            deadline_fences.extend(run.leases.iter().cloned());
        }
    }
    // Original TTL can extend beyond the fixed finish deadline. It still
    // supplies immutable recipients, never a fresh actionable Run after D.
    let active: Vec<_> = live
        .head
        .leases
        .values()
        .filter(|lease| lease.expires_at > now && !deadline_fences.contains(&lease.fence()))
        .map(|lease| lease.fence())
        .collect();
    let mut messages: BTreeMap<String, crate::human_intervention::Message> = live
        .human
        .inbox(&recipients, &active)
        .into_iter()
        .map(|message| (message.id.clone(), message))
        .collect();
    let ledger = live
        .baseline_requests
        .as_ref()
        .ok_or("inbox history is not prepared")?;
    let (raw_holds, canonical) = live.human.projection();
    let mut holds = BTreeMap::<String, (Event, Value)>::new();
    for (id, raw) in raw_holds {
        let opening = canonical
            .openings
            .get(id)
            .ok_or("inbox Hold lacks original opening")?;
        holds.insert(id.clone(), (opening.clone(), raw.clone()));
    }
    for view in ledger.delivery_historical_holds() {
        let hold: Hold =
            serde_json::from_value(view.current_hold.clone()).map_err(|error| error.to_string())?;
        for recipient in &hold.recipients {
            if !recipients.contains(recipient) {
                continue;
            }
            if let Some(reply) = &hold.reply {
                let id = format!("{}/{}/{}", hold.id, reply.id, recipient.run);
                if !canonical.delivered_ids.contains(&id) {
                    messages
                        .entry(id.clone())
                        .or_insert(crate::human_intervention::Message {
                            id,
                            hold: hold.id.clone(),
                            run: recipient.run.clone(),
                            text: reply.text.clone(),
                            kind: "reply".into(),
                            actionable: false,
                        });
                }
            }
        }
        holds
            .entry(hold.id)
            .or_insert((view.opening, view.current_hold));
    }
    for view in ledger.delivery_historical_reviews() {
        let hold: Hold =
            serde_json::from_value(view.current_hold.clone()).map_err(|error| error.to_string())?;
        let crate::human_intervention::HoldBody::Review { binding, .. } = &hold.body else {
            return Err("historical Review has another Hold kind".into());
        };
        for recipient in recipients.iter().filter(|r| {
            r.run == binding.run && r.task == binding.task && r.fences == binding.fences
        }) {
            if let Some(reply) = &hold.reply {
                let id = format!("{}/{}/{}", hold.id, reply.id, recipient.run);
                if !canonical.delivered_ids.contains(&id) {
                    messages
                        .entry(id.clone())
                        .or_insert(crate::human_intervention::Message {
                            id,
                            hold: hold.id.clone(),
                            run: recipient.run.clone(),
                            text: reply.text.clone(),
                            kind: "reply".into(),
                            actionable: false,
                        });
                }
            }
        }
        holds
            .entry(hold.id)
            .or_insert((view.opening, view.current_hold));
    }
    // Finite control receipts supply only original recipient text. They never
    // install the old Pause/Resume or make a new Run an eligible recipient.
    for (opening, hold) in ledger.control_records()? {
        for recipient in &hold.recipients {
            if !recipients.contains(recipient) {
                continue;
            }
            if let Some((id, text, kind)) = crate::intervention_control::message(&hold, recipient) {
                if !canonical.delivered_ids.contains(&id) {
                    messages
                        .entry(id.clone())
                        .or_insert(crate::human_intervention::Message {
                            id,
                            hold: hold.id.clone(),
                            run: recipient.run.clone(),
                            text,
                            kind,
                            actionable: false,
                        });
                }
            }
        }
        holds.entry(hold.id.clone()).or_insert((
            opening,
            serde_json::to_value(hold).map_err(|e| e.to_string())?,
        ));
    }
    let mut groups = Vec::new();
    // A readonly historical text without a proved actual source cannot be acked
    // or offered as a new actionable message merely by naming its JSON eid.
    let mut proved_messages = Vec::new();
    for message in messages.into_values() {
        let (opening, raw) = holds
            .get(&message.hold)
            .ok_or("inbox message lacks Hold provenance")?;
        let source = if message.kind != "reply" {
            ledger.delivery_source(&opening.eid)
        } else {
            let reply = raw["reply"]["id"]
                .as_str()
                .ok_or("inbox reply lacks stable source id")?;
            ledger.delivery_reply_source(&message.hold, reply)
        }
        .ok_or("inbox reply lacks its exact verified source Event")?;
        let original_opening = ledger
            .delivery_source(&opening.eid)
            .ok_or("inbox Hold lacks its exact verified opening Event")?;
        let group = DeliveryGroup {
            hold: message.hold.clone(),
            recipient_run: message.run.clone(),
            opening: original_opening.clone(),
            source: source.clone(),
            message_ids: vec![message.id.clone()],
        };
        if group.message()?.text != message.text {
            return Err("inbox text differs from actual recorded source".into());
        }
        groups.push(group);
        proved_messages.push(message);
    }
    groups.sort_by(|a, b| (&a.hold, &a.recipient_run).cmp(&(&b.hold, &b.recipient_run)));
    Ok(InboxBatch {
        project: catalog.config.project_id().into(),
        host: host.into(),
        action: DeliveryAction {
            directory: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            branch: catalog.branch.clone(),
            recipients,
            groups,
        },
        messages: proved_messages,
    })
}

fn poll(
    live: &mut ProjectSession,
    catalog: &Catalog,
    host: &str,
    selector: Option<&str>,
    activity: InboxActivity,
    now: u64,
) -> Result<InboxBatch, String> {
    let runs = selected_runs(live, catalog, host, selector)?;
    if activity == InboxActivity::Project {
        let mut current = Vec::new();
        for run in &runs {
            if matches!(run.state.as_str(), "running" | "verifying" | "held")
                && crate::finish::run_deadline(run)?.is_none_or(|deadline| now < deadline)
                && !run.leases.is_empty()
                && run.leases.iter().all(|fence| {
                    live.head
                        .leases
                        .get(&fence.name)
                        .is_some_and(|lease| lease.fence() == *fence && lease.expires_at > now)
                })
            {
                current.push(run);
            }
        }
        if current.len() > 1 {
            return Err("inbox has ambiguous current runs".into());
        }
        if let Some(run) = current.first() {
            live.renew_binding(catalog, host, Some(&run.id), now)?;
        }
    }
    capture(live, catalog, host, selector, now)
}

impl ProjectSession {
    pub fn inbox_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        selector: Option<&str>,
        activity: InboxActivity,
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<InboxBatch, InboxReadFailure> {
        actual_worktree(catalog)?;
        // Prior printed requests repair their own exact groups before discovering
        // new messages. A zero-message poll never invents an empty ack request.
        let inputs = outbox.read_inputs()?;
        let now = clock()?;
        let (requests, batch) = Self::continue_shared(session, catalog, &inputs, now, |live| {
            let requests: Vec<_> = live
                .baseline_requests
                .as_ref()
                .ok_or("inbox ledger is not prepared")?
                .delivery_requests(&inputs)
                .into_iter()
                .filter(|request| {
                    request.body.host == host
                        && request.body.action.directory == catalog.root
                        && request.body.action.common_dir == catalog.common_dir
                        && request.body.action.branch == catalog.branch
                })
                .collect();
            // With no original to coordinate, this same proved continuation
            // captures the inbox; there is no second identical source read.
            let batch = if requests.is_empty() {
                Some(poll(live, catalog, host, selector, activity, now)?)
            } else {
                None
            };
            Ok::<_, String>((requests, batch))
        })
        // continue_shared returns its outer Err before entering this closure;
        // String errors returned by poll remain ordinary rejections below.
        .map_err(InboxReadFailure::before_poll)??;
        if let Some(batch) = batch {
            return Ok(batch);
        }
        for request in requests {
            Self::finish_delivery_groups_shared(session, catalog, outbox, &request, &clock)?;
        }
        let inputs = outbox.read_inputs()?;
        let now = clock()?;
        Self::continue_shared(session, catalog, &inputs, now, |live| {
            poll(live, catalog, host, selector, activity, now)
        })
        // Own original groups may already be durable. Never classify this
        // later continuation as a fresh read, even for identical error text.
        .map_err(InboxReadFailure::after_coordination)?
        .map_err(InboxReadFailure::from)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "actual server scope, original identity, exact printed IDs and durable outbox stay explicit"
    )]
    pub fn acknowledge_inbox_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        host: &str,
        batch: &InboxBatch,
        request_id: &str,
        message_ids: &[String],
        outbox: &mut RecoveryOutbox,
        clock: impl Fn() -> Result<u64, String>,
    ) -> Result<DeliveryOutcome, String> {
        let action = batch.selected(message_ids)?;
        let request =
            PreparedDeliveryRequest::prepare(&batch.project, &batch.host, request_id, action)?;
        if host != batch.host
            || catalog.config.project_id() != batch.project
            || catalog.root != batch.action.directory
            || catalog.common_dir != batch.action.common_dir
            || catalog.branch != batch.action.branch
        {
            return Err("delivery token project/host/worktree/branch changed".into());
        }
        Self::finish_delivery_groups_shared(session, catalog, outbox, &request, &clock)
    }

    fn finish_delivery_groups_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        request: &PreparedDeliveryRequest,
        clock: &impl Fn() -> Result<u64, String>,
    ) -> Result<DeliveryOutcome, String> {
        let id = &request.meta.request_id;
        for group in &request.body.action.groups {
            let key = group.key()?;
            let inputs = outbox
                .read_inputs()
                .map_err(|error| uncertain_request(id, error))?;
            let now = clock().map_err(|error| uncertain_request(id, error))?;
            let result = Self::continue_shared(session, catalog, &inputs, now, |live| {
                let ledger = live
                    .baseline_requests
                    .as_mut()
                    .ok_or_else(|| HubError::Invalid("delivery ledger is not prepared".into()))?;
                ledger
                    .reserve(RequestIdentity::Delivery(request.clone()))
                    .map_err(HubError::Invalid)?;
                let phase = ledger
                    .delivery_phase(request, &key)
                    .map_err(HubError::Invalid)?;
                if let Some((original, Some(actual))) = &phase {
                    // Original acceptance first: no new planning, expiry/run check,
                    // Hub append, lease renewal or token/body substitution.
                    outbox
                        .append_pending(original)
                        .map_err(HubError::Uncertain)?;
                    outbox
                        .persist_confirmed(original, actual)
                        .map_err(HubError::Uncertain)?;
                    return Ok(Some(actual.clone()));
                }
                for source in [&group.opening, &group.source] {
                    if ledger.delivery_source(&source.eid) != Some(source) {
                        return Err(HubError::Invalid(
                            "delivery source is not the exact verified original".into(),
                        ));
                    }
                }
                let original = if let Some((original, None)) = phase {
                    original
                } else {
                    let raw = live
                        .human
                        .projection()
                        .0
                        .get(&group.hold)
                        .cloned()
                        .unwrap_or_else(|| group.source.data["hold"].clone());
                    let mut hold = raw;
                    let object = hold.as_object_mut().ok_or_else(|| {
                        HubError::Invalid("delivery original Hold is not an object".into())
                    })?;
                    let delivered = object
                        .entry("delivered")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                        .ok_or_else(|| {
                            HubError::Invalid("delivery Hold state is invalid".into())
                        })?;
                    let mut ids: BTreeSet<String> = delivered
                        .get(&group.recipient_run)
                        .map(|raw| serde_json::from_value(raw.clone()))
                        .transpose()
                        .map_err(|error| HubError::Invalid(error.to_string()))?
                        .unwrap_or_default();
                    ids.extend(group.message_ids.iter().cloned());
                    delivered.insert(group.recipient_run.clone(), json!(ids));
                    let record = DeliveryRecorded {
                        hold,
                        recipient_run: group.recipient_run.clone(),
                        message_ids: group.message_ids.clone(),
                        group_key: key.clone(),
                        request_body: request.body.clone(),
                        p3_request: request.meta.clone(),
                        p3_receipt: RequestReceipt {
                            request_id: id.clone(),
                            body_hash: request.meta.body_hash.clone(),
                            result_ref: DeliveryGroupResult {
                                group_key: key.clone(),
                                message_ids: group.message_ids.clone(),
                            },
                        },
                    };
                    NewEvent {
                        eid: request.event_eid(&key).map_err(HubError::Invalid)?,
                        host: request.body.host.clone(),
                        actor: "process:towermd-cli".into(),
                        kind: "prompt.delivered".into(),
                        layer: "L5".into(),
                        obj: group.hold.clone(),
                        guards: vec![],
                        data: serde_json::to_value(record)
                            .map_err(|error| HubError::Invalid(error.to_string()))?,
                    }
                };
                cli_delivery::decode(catalog.config.project_id(), &original)
                    .map_err(HubError::Invalid)?;
                match recovery_outbox::append_durable(&mut *live.hub, outbox, original, now)
                    .map_err(|error| match error {
                        // The original intent may already be on disk despite a
                        // failed pending fsync. Preserve its UUID for coordination.
                        HubError::Io(reason) => HubError::Uncertain(reason),
                        other => other,
                    })? {
                    AppendReceipt::Event(actual) => Ok(Some(*actual)),
                    AppendReceipt::Duplicate { .. } => Err(HubError::Uncertain(
                        "delivery duplicate lacks original actual receipt".into(),
                    )),
                }
            })
            .map_err(|error| uncertain_request(id, error))?;
            let actual =
                Self::finish_baseline_request_shared(session, catalog, outbox, id, result, now)?
                    .ok_or_else(|| uncertain_request(id, "delivery group missing acceptance"))?;
            if actual.stale {
                return Err(uncertain_request(id, "delivery group accepted stale"));
            }
        }
        let inputs = outbox
            .read_inputs()
            .map_err(|error| uncertain_request(id, error))?;
        let now = clock().map_err(|error| uncertain_request(id, error))?;
        Self::continue_shared(session, catalog, &inputs, now, |live| {
            let ledger = live
                .baseline_requests
                .as_ref()
                .ok_or("delivery ledger not prepared")?;
            for group in &request.body.action.groups {
                let (_, actual) = ledger
                    .delivery_phase(request, &group.key()?)?
                    .ok_or("delivery group absent")?;
                let actual = actual.ok_or("delivery group unconfirmed")?;
                if actual.stale || !inputs.receipt_confirmed(&actual) {
                    return Err(uncertain_request(
                        id,
                        "delivery group durability incomplete",
                    ));
                }
            }
            Ok(DeliveryOutcome {
                receipt: RequestReceipt {
                    request_id: id.clone(),
                    body_hash: request.meta.body_hash.clone(),
                    result_ref: DeliveryResult {
                        message_ids: request.ids(),
                    },
                },
            })
        })
        .map_err(|error| uncertain_request(id, error))?
        .map_err(|error| uncertain_request(id, error))
    }
}

#[cfg(test)]
#[path = "cli_delivery_source_tests.rs"]
mod source_tests;
