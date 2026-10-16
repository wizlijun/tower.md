//! Strict, isolated decoder for the original business-aware v1 journal.
use crate::hub::{self, Event, Fence, Lease, SystemCheckpoint, SystemDelta, SystemState};
use crate::hub_protocol::HubError;
use crate::snapshot::Snapshot;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LeaseChange {
    Acquire {
        names: Vec<String>,
        holder: String,
        ttl_ms: u64,
    },
    Renew {
        fences: Vec<Fence>,
        ttl_ms: u64,
    },
    Release {
        fences: Vec<Fence>,
    },
}

use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyJournal {
    version: u32,
    events: Vec<LegacyEvent>,
    #[serde(default)]
    snapshot_ack: Option<hub::SnapshotAck>,
    #[serde(default)]
    base_snapshot: Option<Snapshot>,
}
#[derive(Deserialize)]
struct LegacyEvent {
    seq: u64,
    eid: String,
    host: String,
    t: String,
    actor: String,
    #[serde(rename = "type")]
    kind: String,
    layer: String,
    obj: String,
    data: Value,
    epoch: Option<u64>,
    stale: bool,
    cost_usd: Option<f64>,
    fence: Option<Fence>,
    lease_delta: Option<LegacyDelta>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
#[derive(Deserialize)]
struct LegacyDelta {
    change: LeaseChange,
    updated: Vec<Lease>,
}
pub(crate) fn load(dir: &Path) -> Result<Option<(SystemCheckpoint, Vec<Event>)>, HubError> {
    let bytes = match fs::read(dir.join("journal.json")) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(HubError::Io(e.to_string())),
    };
    let old: LegacyJournal = serde_json::from_slice(&bytes)
        .map_err(|e| HubError::Invalid(format!("invalid legacy hub journal: {e}")))?;
    if old.version != 1 {
        return Err(hub::invalid("unsupported legacy hub journal"));
    }
    if let Some(base) = &old.base_snapshot {
        base.validate_metadata()?;
    }
    let floor = old.base_snapshot.as_ref().map_or(0, |s| s.through_seq);
    let mut checkpoint = SystemCheckpoint {
        floor_seq: floor,
        epoch_high_water: old.base_snapshot.as_ref().map_or(0, |s| s.epoch_high_water),
        leases: BTreeMap::new(),
        receipts: old
            .base_snapshot
            .as_ref()
            .map_or_else(BTreeMap::new, |s| s.legacy_receipts.clone()),
        ack: None,
        checkpoint: serde_json::to_value(&old.base_snapshot)
            .map_err(|e| HubError::Invalid(e.to_string()))?,
        legacy_through_seq: old.events.last().map_or(floor, |e| e.seq),
    };
    if let Some(base) = &old.base_snapshot {
        let mut omitted = base.omitted.clone();
        omitted.sort_unstable_by_key(|range| range.end);
        checkpoint.receipts.retain(|_, seq| {
            let index = omitted.partition_point(|range| range.end < *seq);
            !omitted.get(index).is_some_and(|range| range.start <= *seq)
        });
        let mut normalized = base.clone();
        normalized.normalize();
        checkpoint.checkpoint =
            serde_json::to_value(normalized).map_err(|e| HubError::Invalid(e.to_string()))?;
    }
    let mut state = SystemState::from_checkpoint(&checkpoint)?;
    let mut seq = floor;
    let mut seen = checkpoint.receipts.clone();
    let mut events = vec![];
    for old in old.events {
        seq = seq
            .checked_add(1)
            .ok_or_else(|| hub::invalid("legacy sequence overflow"))?;
        if seq != old.seq || seen.insert(old.eid.clone(), old.seq).is_some() {
            return Err(hub::invalid("legacy sequence or duplicate eid"));
        }
        let event = convert(old)?;
        state.replay(&event, checkpoint.legacy_through_seq)?;
        events.push(event);
    }
    if let Some(ack) = old.snapshot_ack {
        if ack.seq > seq
            || ack.epoch == 0
            || ack.epoch > state.epoch
            || !matches!(ack.commit.len(), 40 | 64)
            || !ack.commit.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(hub::invalid("invalid legacy acknowledgement"));
        }
        checkpoint.ack = Some(ack);
    }
    hub::validate_checkpoint(&checkpoint)?;
    Ok(Some((checkpoint, events)))
}
fn convert(old: LegacyEvent) -> Result<Event, HubError> {
    if old.eid.starts_with("git:") {
        let commit = old.data["commit"]
            .as_str()
            .ok_or_else(|| hub::invalid("legacy derived event requires commit"))?;
        if crate::events::derived_eid(commit, &old.kind)? != old.eid {
            return Err(hub::invalid("invalid legacy derived eid"));
        }
    }
    let heartbeat = old.kind == "lease.renewed"
        && old.cost_usd.is_none()
        && old
            .lease_delta
            .as_ref()
            .is_some_and(|d| matches!(d.change, LeaseChange::Renew { .. }))
        && old.data.as_object().is_some_and(|data| {
            data.iter().all(|(k, v)| {
                matches!(k.as_str(), "run" | "lease") && v.as_str().is_some_and(|s| !s.is_empty())
            })
        });
    let (delta, change) = match old.lease_delta {
        Some(d) => {
            let delta = match &d.change {
                LeaseChange::Acquire { .. } => SystemDelta::Grant { leases: d.updated },
                LeaseChange::Renew { .. } => SystemDelta::Renew { leases: d.updated },
                LeaseChange::Release { fences } => {
                    if !d.updated.is_empty() {
                        return Err(hub::invalid("legacy release contains updates"));
                    }
                    SystemDelta::Release {
                        fences: fences.clone(),
                    }
                }
            };
            (Some(delta), Some(d.change))
        }
        None => (None, None),
    };
    let event = Event {
        seq: old.seq,
        eid: old.eid,
        host: old.host,
        t: old.t,
        actor: old.actor,
        kind: old.kind,
        layer: old.layer,
        obj: old.obj,
        data: old.data,
        epoch: old.epoch,
        stale: old.stale,
        cost_usd: old.cost_usd,
        guards: old.fence.into_iter().collect(),
        lease_delta: delta,
        heartbeat,
        legacy: true,
        legacy_change: change,
        extra: old.extra,
    };
    Ok(event)
}
pub(crate) fn validate_converted(
    state: &SystemState,
    event: &Event,
    now: u64,
) -> Result<(), HubError> {
    let heartbeat = event.kind == "lease.renewed"
        && event.cost_usd.is_none()
        && matches!(event.lease_delta, Some(SystemDelta::Renew { .. }))
        && event.data.as_object().is_some_and(|data| {
            data.iter().all(|(key, value)| {
                matches!(key.as_str(), "run" | "lease")
                    && value.as_str().is_some_and(|value| !value.is_empty())
            })
        });
    if event.heartbeat != heartbeat {
        return Err(hub::invalid("inconsistent legacy heartbeat metadata"));
    }
    let stale = event.guards.iter().any(|f| !state.active(f, now));
    if event.epoch != event.guards.first().map(|f| f.epoch) || event.stale != stale {
        return Err(hub::invalid("inconsistent legacy epoch or stale"));
    }
    if stale {
        if event.lease_delta.is_some() || event.legacy_change.is_some() {
            return Err(hub::invalid("stale legacy event changes leases"));
        }
        return Ok(());
    }
    let expected = match &event.legacy_change {
        None => None,
        Some(LeaseChange::Acquire {
            names,
            holder,
            ttl_ms,
        }) => {
            hub::validate_names(names.iter().map(String::as_str))?;
            hub::validate_token("lease holder", holder)?;
            let expiry = hub::expiration(now, *ttl_ms)?;
            let mut epoch = state.epoch;
            let mut leases = vec![];
            for name in names {
                if state.leases.get(name).is_some_and(|l| l.expires_at > now) {
                    return Err(hub::invalid("occupied legacy grant"));
                }
                epoch = epoch
                    .checked_add(1)
                    .ok_or_else(|| hub::invalid("legacy epoch overflow"))?;
                leases.push(Lease {
                    name: name.clone(),
                    holder: holder.clone(),
                    epoch,
                    expires_at: expiry,
                })
            }
            Some(SystemDelta::Grant { leases })
        }
        Some(LeaseChange::Renew { fences, ttl_ms }) => {
            state.validate_fences(fences, now)?;
            let expiry = hub::expiration(now, *ttl_ms)?;
            Some(SystemDelta::Renew {
                leases: fences
                    .iter()
                    .map(|f| Lease {
                        name: f.name.clone(),
                        holder: f.holder.clone(),
                        epoch: f.epoch,
                        expires_at: expiry,
                    })
                    .collect(),
            })
        }
        Some(LeaseChange::Release { fences }) => {
            state.validate_fences(fences, now)?;
            Some(SystemDelta::Release {
                fences: fences.clone(),
            })
        }
    };
    if event.lease_delta != expected {
        return Err(hub::invalid("inconsistent legacy lease delta"));
    }
    Ok(())
}

pub(crate) fn decode_archive_line(line: &str) -> Result<Event, HubError> {
    match serde_json::from_str(line) {
        Ok(event) => Ok(event),
        Err(_) => convert(
            serde_json::from_str(line)
                .map_err(|e| HubError::Invalid(format!("invalid legacy archive event: {e}")))?,
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub_protocol::{AppendReceipt, HubProtocol, NewEvent};
    use serde_json::json;
    fn grant(eid: &str) -> Value {
        json!({"seq":1,"eid":eid,"host":"host","t":"1970-01-01T00:00:00.001Z","actor":"process:test","type":"lease.acquired","layer":"L3","obj":"main","data":{"lease":"main","future":[null,false]},"epoch":null,"stale":false,"cost_usd":null,"fence":null,"lease_delta":{"change":{"Acquire":{"names":["main"],"holder":"writer","ttl_ms":100}},"updated":[{"name":"main","holder":"writer","epoch":1,"expires_at":101}]},"future_envelope":{"unknown":null}})
    }
    #[test]
    fn old_archive_event_keeps_control_business_data_and_unknown_fields() {
        let id = uuid::Uuid::now_v7().to_string();
        let old = grant(&id);
        let event = decode_archive_line(&old.to_string()).unwrap();
        assert_eq!(event.seq, 1);
        assert_eq!(event.acquired_leases()[0].epoch, 1);
        assert_eq!(event.layer, "L3");
        assert_eq!(event.data, old["data"]);
        assert_eq!(
            serde_json::to_value(event).unwrap()["future_envelope"],
            old["future_envelope"]
        );
    }
    #[test]
    fn legacy_base_becomes_v2_opaque_and_drops_omitted_heartbeat_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let eid = uuid::Uuid::now_v7().to_string();
        let heart = uuid::Uuid::now_v7().to_string();
        let snapshot = json!({"format_version":1,"project_id":"project","through_seq":2,"writer_epoch":1,"epoch_high_water":1,"projection":{"objects":{},"lease_summaries":{"main":{"name":"main","holder":"writer","epoch":1}}},"receipts":{eid.clone():1,heart.clone():2},"omitted":[{"start":2,"end":2}],"recovery_gaps":[]});
        fs::write(
            dir.path().join("journal.json"),
            json!({"version":1,"events":[],"base_snapshot":snapshot}).to_string(),
        )
        .unwrap();
        let session = crate::project_session::ProjectSession::open(dir.path()).unwrap();
        assert_eq!(session.base_snapshot().unwrap().format_version, 2);
        drop(session);
        let mut hub = hub::Hub::open(dir.path()).unwrap();
        let input = |eid| NewEvent {
            eid,
            host: "host".into(),
            actor: "process:test".into(),
            kind: "future.event".into(),
            layer: "L5".into(),
            obj: "main".into(),
            data: Value::Null,
            guards: vec![],
        };
        assert_eq!(
            hub.append(input(eid), 3).unwrap(),
            AppendReceipt::Duplicate { seq: 1 }
        );
        let AppendReceipt::Event(event) = hub.append(input(heart), 3).unwrap() else {
            panic!()
        };
        assert_eq!(event.seq, 3);
    }
}
