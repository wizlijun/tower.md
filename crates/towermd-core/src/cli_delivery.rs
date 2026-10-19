//! Finite delivery identity. A printed message is delivered, never processed.
use crate::{
    ask_contract::Recipient,
    hub_protocol::{Event, NewEvent},
    human_intervention::{Hold, Message},
    recovery_contract::{
        self,
        recorded::{request_event_eid, RequestBody, RequestMeta, RequestReceipt},
    },
    recovery_outbox,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// Only the first inbox continuation's typed pre-closure head race may be
/// reread. Display text alone never grants retry after original coordination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxReadFailure {
    message: String,
    read_changed: bool,
}
impl InboxReadFailure {
    pub fn is_read_changed(&self) -> bool {
        self.read_changed
    }
    pub(crate) fn before_poll(error: crate::project_session::SessionSyncError) -> Self {
        let read_changed = matches!(&error,
            crate::project_session::SessionSyncError::SourceChanged(reason)
                if reason == "head changed before continuation");
        Self {
            message: error.to_string(),
            read_changed,
        }
    }
    pub(crate) fn after_coordination(error: crate::project_session::SessionSyncError) -> Self {
        Self::from(error.to_string())
    }
}
impl From<String> for InboxReadFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            read_changed: false,
        }
    }
}
impl From<&str> for InboxReadFailure {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}
impl std::fmt::Display for InboxReadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for InboxReadFailure {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryGroup {
    pub hold: String,
    pub recipient_run: String,
    #[serde(deserialize_with = "recovery_contract::deserialize_event")]
    pub opening: Event,
    #[serde(deserialize_with = "recovery_contract::deserialize_event")]
    pub source: Event,
    pub message_ids: Vec<String>,
}
impl DeliveryGroup {
    pub(crate) fn key(&self) -> Result<String, String> {
        Ok(format!(
            "delivery:{}",
            recovery_contract::canonical_hash(&(&self.hold, &self.recipient_run))?
        ))
    }
    pub(crate) fn message(&self) -> Result<Message, String> {
        let hold: Hold = serde_json::from_value(self.source.data["hold"].clone())
            .map_err(|error| error.to_string())?;
        let (id, text, kind) = if let Some(reply) = &hold.reply {
            (
                format!("{}/{}/{}", self.hold, reply.id, self.recipient_run),
                reply.text.clone(),
                "reply",
            )
        } else if let crate::human_intervention::HoldBody::Prompt { text, .. } = &hold.body {
            (
                format!("{}/prompt/{}", self.hold, self.recipient_run),
                text.clone(),
                "prompt",
            )
        } else if let Some(recipient) = hold
            .recipients
            .iter()
            .find(|recipient| recipient.run == self.recipient_run)
        {
            let (id, text, kind) = crate::intervention_control::message(&hold, recipient)
                .ok_or("delivery source has no control message")?;
            if self.message_ids != [id.clone()] {
                return Err("control delivery IDs differ from original source".into());
            }
            return Ok(Message {
                id,
                hold: self.hold.clone(),
                run: self.recipient_run.clone(),
                text,
                kind,
                actionable: false,
            });
        } else {
            return Err("delivery source has no frozen recipient".into());
        };
        if self.message_ids != [id.clone()] {
            return Err("delivery stable IDs differ from their recorded source".into());
        }
        Ok(Message {
            id,
            hold: self.hold.clone(),
            run: self.recipient_run.clone(),
            text,
            kind: kind.into(),
            actionable: false,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryAction {
    pub directory: PathBuf,
    pub common_dir: PathBuf,
    pub branch: String,
    pub recipients: Vec<Recipient>,
    pub groups: Vec<DeliveryGroup>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PreparedDeliveryRequest {
    pub(crate) body: RequestBody<DeliveryAction>,
    pub(crate) meta: RequestMeta,
}
impl PreparedDeliveryRequest {
    pub(crate) fn prepare(
        project: &str,
        host: &str,
        id: &str,
        action: DeliveryAction,
    ) -> Result<Self, String> {
        let body = RequestBody {
            project_id: project.into(),
            host: host.into(),
            actor: "process:towermd-cli".into(),
            run: None,
            action,
        };
        let meta = RequestMeta {
            request_id: id.into(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Self::from_recorded(body, meta)
    }
    pub(crate) fn from_recorded(
        body: RequestBody<DeliveryAction>,
        meta: RequestMeta,
    ) -> Result<Self, String> {
        request_event_eid(&meta.request_id, "prompt.delivered")?;
        if body.actor != "process:towermd-cli"
            || body.run.is_some()
            || body.project_id.is_empty()
            || recovery_contract::canonical_hash(&body)? != meta.body_hash
            || !body.action.directory.is_absolute()
            || !body.action.common_dir.is_absolute()
            || body.action.branch.is_empty()
            || body.action.groups.is_empty()
            || body.action.recipients.is_empty()
        {
            return Err("delivery original identity/context is invalid".into());
        }
        crate::hub::validate_token("host", &body.host).map_err(|error| error.to_string())?;
        let mut runs = BTreeSet::new();
        for recipient in &body.action.recipients {
            crate::ask_contract::validate_recipient(recipient)?;
            if recipient.host != body.host
                || recipient.worktree != body.action.directory.to_string_lossy()
                || recipient.branch != body.action.branch
                || !runs.insert(recipient.run.clone())
            {
                return Err("delivery recipient is outside original host/worktree/branch".into());
            }
        }
        if body
            .action
            .recipients
            .windows(2)
            .any(|pair| pair[0].run >= pair[1].run)
        {
            return Err("delivery recipients must be sorted".into());
        }
        let mut keys = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for group in &body.action.groups {
            let recipient = body
                .action
                .recipients
                .iter()
                .find(|r| r.run == group.recipient_run)
                .ok_or("delivery group has no frozen recipient")?;
            let source: Hold = serde_json::from_value(group.source.data["hold"].clone())
                .map_err(|error| error.to_string())?;
            if source.object_type != "Hold"
                || group.hold != group.opening.obj
                || group.hold != group.source.obj
                || source.id != group.hold
                || group.opening.kind != "hold.opened"
                || group.opening.layer != "L5"
                || !source_recipient(&source, recipient)
                || !keys.insert(group.key()?)
            {
                return Err("delivery group differs from its original source/recipient".into());
            }
            recovery_outbox::validate_actual(
                &recovery_outbox::original_of(&group.opening),
                &group.opening,
            )?;
            recovery_outbox::validate_actual(
                &recovery_outbox::original_of(&group.source),
                &group.source,
            )?;
            if source_role(&group.opening) != source_role(&group.source)
                || source_role(&group.source).is_none()
            {
                return Err("delivery source kind differs from its original Hold kind".into());
            }
            group.message()?;
            for id in &group.message_ids {
                if !ids.insert(id.clone()) {
                    return Err("duplicate delivery message ID".into());
                }
            }
        }
        if body.action.groups.windows(2).any(|pair| {
            (&pair[0].hold, &pair[0].recipient_run) >= (&pair[1].hold, &pair[1].recipient_run)
        }) {
            return Err("delivery groups must be sorted".into());
        }
        Ok(Self { body, meta })
    }
    pub(crate) fn event_eid(&self, key: &str) -> Result<String, String> {
        request_event_eid(&self.meta.request_id, &format!("prompt.delivered/{key}"))
    }
    pub(crate) fn ids(&self) -> Vec<String> {
        self.body
            .action
            .groups
            .iter()
            .flat_map(|group| group.message_ids.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryGroupResult {
    pub group_key: String,
    pub message_ids: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryResult {
    pub message_ids: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryOutcome {
    pub receipt: RequestReceipt<DeliveryResult>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeliveryRecorded {
    pub hold: Value,
    pub recipient_run: String,
    pub message_ids: Vec<String>,
    pub group_key: String,
    pub request_body: RequestBody<DeliveryAction>,
    pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<DeliveryGroupResult>,
}
pub(crate) fn decode(project: &str, original: &NewEvent) -> Result<DeliveryRecorded, String> {
    let record: DeliveryRecorded =
        serde_json::from_value(original.data.clone()).map_err(|error| error.to_string())?;
    let request = PreparedDeliveryRequest::from_recorded(
        record.request_body.clone(),
        record.p3_request.clone(),
    )?;
    let group = request
        .body
        .action
        .groups
        .iter()
        .find(|group| group.key().as_deref() == Ok(record.group_key.as_str()))
        .ok_or("delivery original group is absent from complete batch")?;
    let hold: Hold =
        serde_json::from_value(record.hold.clone()).map_err(|error| error.to_string())?;
    if project != request.body.project_id
        || original.kind != "prompt.delivered"
        || original.layer != "L5"
        || original.actor != request.body.actor
        || original.host != request.body.host
        || !original.guards.is_empty()
        || original.eid != request.event_eid(&record.group_key)?
        || original.obj != group.hold
        || hold.object_type != "Hold"
        || hold.id != group.hold
        || record.recipient_run != group.recipient_run
        || record.message_ids != group.message_ids
        || record.p3_receipt.request_id != request.meta.request_id
        || record.p3_receipt.body_hash != request.meta.body_hash
        || record.p3_receipt.result_ref
            != (DeliveryGroupResult {
                group_key: record.group_key.clone(),
                message_ids: record.message_ids.clone(),
            })
    {
        return Err("delivery event/body/group/receipt identity differs".into());
    }
    Ok(record)
}

// Only one projection truth: actual finite delivery IDs. Hold.delivered is the
// same operation's materialized view, never a replacement of the old raw Hold.
pub(crate) fn apply(
    canonical: &mut crate::human_intervention::HumanProjection,
    holds: &mut BTreeMap<String, Value>,
    event: &Event,
) -> Result<(), String> {
    let original = recovery_outbox::original_of(event);
    recovery_outbox::validate_actual(&original, event)?;
    let record = decode(
        event.data["request_body"]["project_id"]
            .as_str()
            .ok_or("delivery lacks project")?,
        &original,
    )?;
    if event.stale {
        return Ok(());
    }
    reduce_ids(canonical, holds, &record)
}
pub(crate) fn is_receipt(event: &Event) -> bool {
    event.kind == "request.receipt.restored" && event.data["original"]["type"] == "prompt.delivered"
}
pub(crate) fn apply_receipt(
    canonical: &mut crate::human_intervention::HumanProjection,
    holds: &mut BTreeMap<String, Value>,
    event: &Event,
) -> Result<(), String> {
    let project = event.data["original"]["data"]["request_body"]["project_id"]
        .as_str()
        .ok_or("delivery audit lacks project")?;
    let original = crate::request_ledger::restored_original(project, event)?
        .ok_or("delivery audit lacks original")?;
    let record = decode(project, &recovery_outbox::original_of(&original))?;
    reduce_ids(canonical, holds, &record)
}
fn reduce_ids(
    canonical: &mut crate::human_intervention::HumanProjection,
    holds: &mut BTreeMap<String, Value>,
    record: &DeliveryRecorded,
) -> Result<(), String> {
    let mut ids = canonical.delivered_ids.clone();
    ids.extend(record.message_ids.iter().cloned());
    let hold_id = record.hold["id"]
        .as_str()
        .ok_or("delivery audit lacks Hold ID")?;
    if let Some(previous) = holds.get_mut(hold_id) {
        let mut raw = previous.clone();
        let delivered = raw
            .as_object_mut()
            .ok_or("delivery Hold is not an object")?
            .entry("delivered")
            .or_insert_with(|| json!({}));
        let map = delivered
            .as_object_mut()
            .ok_or("Hold delivered is not an object")?;
        let mut run_ids: BTreeSet<String> = map
            .get(&record.recipient_run)
            .map(|raw| serde_json::from_value(raw.clone()))
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        run_ids.extend(record.message_ids.iter().cloned());
        map.insert(record.recipient_run.clone(), json!(run_ids));
        *previous = raw;
    }
    canonical.delivered_ids = ids;
    Ok(())
}

// Source values in a body are only immutable bindings, never their own proof.
// Only exact actual outer events or already verified receipt-only originals
// supplied by the shared reader may satisfy these links.
pub(crate) fn validate_sources(
    record: &DeliveryRecorded,
    actual: Option<u64>,
    sources: &[&Event],
) -> Result<(), String> {
    let group = record
        .request_body
        .action
        .groups
        .iter()
        .find(|group| group.key().as_deref() == Ok(record.group_key.as_str()))
        .ok_or("delivery group missing")?;
    for source in [&group.opening, &group.source] {
        if actual.is_some_and(|seq| source.seq >= seq) || !sources.contains(&source) {
            return Err("delivery lacks its exact earlier actual source Event".into());
        }
    }
    for recipient in &record.request_body.action.recipients {
        let proved = sources.iter().any(|event| {
            if event.kind != "task.claimed" || actual.is_some_and(|seq| event.seq >= seq) {
                return false;
            }
            let Ok(run) = serde_json::from_value::<crate::tasks::Run>(event.data["run"].clone())
            else {
                return false;
            };
            run.id == recipient.run
                && run.task == recipient.task
                && run.host == recipient.host
                && run.worktree == recipient.worktree
                && run.branch == recipient.branch
                && run.leases == recipient.fences
        });
        if !proved {
            return Err("delivery recipient lacks its actual original Task claim scope".into());
        }
    }
    Ok(())
}

/// Captured by the Session; only a server-side token may retain this scope.
/// It is not deserializable and cannot authenticate a body from JSON.
#[derive(Clone, Debug)]
pub struct InboxBatch {
    pub(crate) project: String,
    pub(crate) host: String,
    pub(crate) action: DeliveryAction,
    pub(crate) messages: Vec<Message>,
}
impl InboxBatch {
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }
    pub fn directory(&self) -> &std::path::Path {
        &self.action.directory
    }
    pub(crate) fn selected(&self, ids: &[String]) -> Result<DeliveryAction, String> {
        if ids.is_empty() || ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("delivery IDs must be nonempty sorted and unique".into());
        }
        let selected: BTreeSet<_> = ids.iter().cloned().collect();
        if selected
            .iter()
            .any(|id| !self.messages.iter().any(|message| &message.id == id))
        {
            return Err("delivery token does not contain these message IDs".into());
        }
        let mut action = self.action.clone();
        action
            .groups
            .retain(|group| group.message_ids.iter().any(|id| selected.contains(id)));
        for group in &mut action.groups {
            group.message_ids.retain(|id| selected.contains(id));
        }
        Ok(action)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeliverySource {
    Ask,
    Review,
    Control,
}
fn source_role(event: &Event) -> Option<DeliverySource> {
    if event.layer != "L5" {
        return None;
    }
    if event.kind == "hold.opened"
        && crate::intervention_control::is_control(&event.kind, &event.data)
    {
        return Some(DeliverySource::Control);
    }
    match (
        event.kind.as_str(),
        event.data["hold"]["body"]["kind"].as_str(),
    ) {
        ("hold.opened" | "hold.answered", Some("ask")) => Some(DeliverySource::Ask),
        ("hold.opened" | "hold.reviewed", Some("review")) => Some(DeliverySource::Review),
        _ => None,
    }
}
pub(crate) fn is_source(event: &Event) -> bool {
    source_role(event).is_some() || event.kind == "task.claimed"
}
pub(crate) fn validate_source_event(project: &str, event: &Event) -> Result<(), String> {
    let original = recovery_outbox::original_of(event);
    match source_role(event) {
        Some(DeliverySource::Control) => {
            crate::intervention_control::decode(project, &original)?;
        }
        Some(DeliverySource::Ask) => {
            if crate::ask_answer_contract::is_app_answer(&event.kind, &event.data) {
                crate::ask_answer_contract::decode(project, &original)?;
            } else {
                crate::ask_contract::decode(project, &original)?;
            }
        }
        Some(DeliverySource::Review) if event.kind == "hold.opened" => {
            if event.data.get("p3_request").is_some() {
                crate::request_ledger::decode_review_request(project, &original)?;
            }
            crate::human_intervention::opening(event)?;
        }
        Some(DeliverySource::Review) => {
            crate::request_ledger::decode_task_review(project, &original)?;
        }
        None if event.kind == "task.claimed" => {
            let run: crate::tasks::Run =
                serde_json::from_value(crate::snapshot::recorded_run(event, &event.data["run"])?)
                    .map_err(|error| error.to_string())?;
            if event.actor != "process:towermd-cli"
                || event.layer != "L5"
                || run.object_type != "Run"
                || event.host != run.host
                || event.obj != run.task
                || event.data["task"]["id"].as_str() != Some(run.task.as_str())
            {
                return Err(
                    "delivery claim scope differs from its original Task/Run envelope".into(),
                );
            }
        }
        None => return Err("unsupported delivery source kind".into()),
    }
    Ok(())
}
fn source_recipient(hold: &Hold, recipient: &Recipient) -> bool {
    match &hold.body {
        crate::human_intervention::HoldBody::Review { binding, .. } => {
            recipient.run == binding.run
                && recipient.task == binding.task
                && recipient.fences == binding.fences
        }
        _ => hold.recipients.contains(recipient),
    }
}

pub(crate) fn source_records(
    project: &str,
    outer: &[&Event],
) -> Result<BTreeMap<String, Event>, String> {
    let mut sources: BTreeMap<String, Event> = BTreeMap::new();
    let mut install = |event: Event| -> Result<(), String> {
        // Replayed stale envelopes cannot supply an accepted original source.
        // Exact non-stale originals are installed separately from the verified
        // receipt history; skipping these does not restore Review authority.
        if event.stale || !is_source(&event) {
            return Ok(());
        }
        validate_source_event(project, &event)?;
        if let Some(previous) = sources.get(&event.eid) {
            if recovery_outbox::original_of(previous) != recovery_outbox::original_of(&event) {
                return Err("delivery source original identity conflicts".into());
            }
            if !previous.stale {
                return Ok(());
            }
        }
        sources.insert(event.eid.clone(), event);
        Ok(())
    };
    for event in outer {
        if let Some(original) = crate::request_ledger::restored_original(project, event)? {
            if crate::ask_answer_contract::is_app_answer(&original.kind, &original.data) {
                let answer = crate::ask_answer_contract::decode(
                    project,
                    &recovery_outbox::original_of(&original),
                )?;
                install(answer.opening_event)?;
            }
            install(original)?;
        }
        install((*event).clone())?;
    }
    Ok(sources)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxActivity {
    Project,
    Recovery,
}

#[cfg(test)]
#[path = "cli_delivery_tests.rs"]
mod tests;
