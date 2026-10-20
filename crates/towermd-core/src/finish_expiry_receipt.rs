//! A finite historical audit layout, not a current Task/Run or execution proof.
//! Only VerifiedRecoveryHistory can mint the non-deserializable producer token.
use crate::{
    hub_protocol::{Event, NewEvent},
    recovery_contract as contract, recovery_outbox,
};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    pub schema: u32,
    pub project_id: String,
    pub source_commit: String,
    pub rebuild_eid: String,
    #[serde(deserialize_with = "events")]
    pub context: Vec<Event>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub schema: u32,
    #[serde(deserialize_with = "contract::deserialize_event")]
    pub original: Event,
    pub finish_expiry: Context,
}
fn events<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Event>, D::Error> {
    Vec::<serde_json::Value>::deserialize(deserializer)?
        .into_iter()
        .map(|value| contract::deserialize_event(value).map_err(serde::de::Error::custom))
        .collect()
}
pub(crate) fn is_wrapper(event: &Event) -> bool {
    event.kind == "request.receipt.restored" && event.data.get("finish_expiry").is_some()
}
// Canonical bytes are only a layout/hash operation. This function does not
// create the VerifiedExpiryReceipt needed by recovery or durable backfill.
pub(crate) fn canonical(record: &Record, host: &str) -> Result<NewEvent, String> {
    crate::hub::validate_token("host", host).map_err(|e| e.to_string())?;
    let hash = contract::canonical_hash(record)?;
    Ok(NewEvent {
        eid: crate::events::derived_eid(&hash, "request.receipt.restored")?,
        host: host.into(),
        actor: "process:towermd-daemon".into(),
        kind: "request.receipt.restored".into(),
        layer: "L5".into(),
        obj: record.original.obj.clone(),
        guards: vec![],
        data: serde_json::to_value(record).map_err(|e| e.to_string())?,
    })
}
// A diagnostic is a Run transition even though its outer object is Evidence.
// Decode its exact existing producer layout before admitting it to this finite
// Task prefix; this does not grant an actual outer role or execution authority.
pub(crate) fn context_event(event: &Event, task: &str, project: &str) -> Result<bool, String> {
    if crate::task_check_recorded::is_event(event) {
        let record = crate::task_check_recorded::decode(&recovery_outbox::original_of(event))?;
        return Ok(record.context.task.spec.id == task
            && record.context.run.task == task
            && record.evidence.project_id == project);
    }
    Ok(event.obj == task
        && (matches!(
            event.kind.as_str(),
            "task.state" | "task.claimed" | "task.released"
        ) || super::is_event(&event.kind)))
}
pub(crate) fn decode(project: &str, event: &Event) -> Result<Option<Record>, String> {
    if !is_wrapper(event) {
        return Ok(None);
    }
    let record: Record = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    super::validate_expiry_event(&record.original)?;
    contract::object_id(&record.finish_expiry.source_commit)?;
    crate::hub::validate_token("eid", &record.finish_expiry.rebuild_eid)
        .map_err(|e| e.to_string())?;
    if record.schema != 1
        || record.finish_expiry.schema != 1
        || record.finish_expiry.project_id != project
        || record.finish_expiry.context.is_empty()
        || event.stale
        || event.legacy
        || event.epoch.is_some()
        || event.cost_usd.is_some()
        || event.seq <= record.original.seq
        || recovery_outbox::original_of(event) != canonical(&record, &event.host)?
    {
        return Err("invalid finite historical expiry receipt envelope".into());
    }
    let mut previous = 0;
    let mut ids = std::collections::BTreeSet::new();
    for original in &record.finish_expiry.context {
        recovery_outbox::validate_actual(&recovery_outbox::original_of(original), original)?;
        if original.stale
            || original.legacy
            || original.seq <= previous
            || original.seq >= record.original.seq
            || !ids.insert(&original.eid)
            || !context_event(original, &record.original.obj, project)?
        {
            return Err("historical expiry context is not one finite ordered Task prefix".into());
        }
        previous = original.seq;
    }
    Ok(Some(record))
}
