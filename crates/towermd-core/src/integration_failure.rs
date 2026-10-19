//! Finite, non-authorizing provenance for a rejected main Task footer.
use crate::{
    hub_protocol::{Event, NewEvent},
    recovery_contract::{
        self as contract,
        recorded::{
            self, GateEvidence, GateResult, MainGateFailureContext, MainGateFailureRecorded,
        },
    },
    recovery_outbox,
    snapshot::StateProjection,
    task_gates::Gate,
    tasks::{Run, Task},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) fn is_event(event: &Event) -> bool {
    event.kind == "evidence.recorded" && event.data.get("main_gate_failure").is_some()
}
pub(crate) fn context(record: &MainGateFailureRecorded) -> MainGateFailureContext {
    MainGateFailureContext {
        schema: record.schema,
        claim: record.claim.clone(),
        candidate: record.candidate.clone(),
        authority: record.authority.clone(),
        trailers: record.trailers.clone(),
        observed_through_seq: record.observed_through_seq,
        rejection: record.rejection.clone(),
    }
}
pub(crate) fn stable_uuid(
    claim: &Event,
    candidate: &str,
    trailers: &crate::recovery_git::TaskTrailers,
) -> Result<String, String> {
    let run: Run = serde_json::from_value(claim.data["run"].clone()).map_err(|e| e.to_string())?;
    recorded::request_event_eid(&run.id, "evidence.recorded")?;
    let source = uuid::Uuid::parse_str(&run.id).map_err(|e| e.to_string())?;
    let digest = contract::canonical_hash(&(
        "towermd.main-rejection",
        1u32,
        &claim.eid,
        candidate,
        trailers,
    ))?;
    let mut bytes = *source.as_bytes();
    for (index, slot) in bytes.iter_mut().enumerate().skip(6) {
        *slot =
            u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    bytes[6] = (bytes[6] & 15) | 0x70;
    bytes[8] = (bytes[8] & 63) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}
pub(crate) fn hold(record: &MainGateFailureRecorded) -> Result<Value, String> {
    let task: Task =
        serde_json::from_value(record.claim.data["task"].clone()).map_err(|e| e.to_string())?;
    let run: Run =
        serde_json::from_value(record.claim.data["run"].clone()).map_err(|e| e.to_string())?;
    let id = stable_uuid(&record.claim, &record.candidate, &record.trailers)?;
    Ok(
        json!({"type":"Hold","id":format!("H-{id}"),"kind":"gatefailed","gate":"G5","task":task.spec.id,"run":run.id,"evidence":record.evidence.id,"commit":record.candidate,"tree":record.evidence.tree,"authority":record.authority,"fences":run.leases,"rejection":record.rejection}),
    )
}
pub(crate) fn decode(event: &Event) -> Result<MainGateFailureRecorded, String> {
    let original = recovery_outbox::original_of(event);
    recovery_outbox::validate_actual(&original, event)?;
    let record = decode_original(&original)?;
    if record.observed_through_seq >= event.seq {
        return Err("main failure prefix is not before actual acceptance".into());
    }
    Ok(record)
}
pub(crate) fn decode_original(event: &NewEvent) -> Result<MainGateFailureRecorded, String> {
    let data = event
        .data
        .as_object()
        .ok_or("main failure data is not an object")?;
    if data.len() != 2 || !data.contains_key("evidence") || !data.contains_key("main_gate_failure")
    {
        return Err("main failure outer data layout differs".into());
    }
    let record: MainGateFailureRecorded =
        serde_json::from_value(event.data["main_gate_failure"].clone())
            .map_err(|e| e.to_string())?;
    let evidence = &record.evidence;
    contract::validate_gate_layout(evidence)?;
    let task: Task =
        serde_json::from_value(record.claim.data["task"].clone()).map_err(|e| e.to_string())?;
    let run: Run = serde_json::from_value(crate::snapshot::recorded_run(
        &record.claim,
        &record.claim.data["run"],
    )?)
    .map_err(|e| e.to_string())?;
    recovery_outbox::validate_actual(&recovery_outbox::original_of(&record.claim), &record.claim)?;
    let id = stable_uuid(&record.claim, &record.candidate, &record.trailers)?;
    let expected_fences = std::iter::once(&task.spec.module)
        .chain(task.spec.touches.iter())
        .map(|id| format!("module:{id}"))
        .collect::<std::collections::BTreeSet<_>>();
    if task.object_type != "Task"
        || run.object_type != "Run"
        || run.branch != format!("t/{}", task.spec.id)
        || evidence.project_id.trim().is_empty()
        || expected_fences.len() != run.leases.len()
        || expected_fences
            != run
                .leases
                .iter()
                .map(|f| f.name.clone())
                .collect::<std::collections::BTreeSet<_>>()
        || run
            .leases
            .iter()
            .any(|f| f.holder != run.id || f.epoch == 0)
    {
        return Err("main failure lacks original registered objects/full Module fences".into());
    }
    let diagnostic = serde_json::to_value(&record.rejection).map_err(|e| e.to_string())?;
    if record.schema != 1
        || record.claim.stale
        || record.claim.kind != "task.claimed"
        || record.claim.actor != "process:towermd-cli"
        || record.claim.layer != "L5"
        || record.claim.obj != task.spec.id
        || record.claim.host != run.host
        || record.claim.guards != run.leases
        || run.task != task.spec.id
        || task.state != "running"
        || run.state != "running"
        || record.claim.seq > record.observed_through_seq
        || event.kind != "evidence.recorded"
        || event.actor != "process:towermd-gates"
        || event.layer != "L5"
        || event.host != run.host
        || event.obj != format!("E-{id}")
        || event.guards != run.leases
        || event.eid != recorded::request_event_eid(&id, "evidence.recorded")?
        || evidence.id != event.obj
        || evidence.gate != Gate::G5
        || evidence.result != GateResult::Failed
        || evidence.review.is_some()
        || evidence.diagnostic.as_deref() != diagnostic.as_str()
        || evidence.task != task.spec.id
        || evidence.run != run.id
        || evidence.module != task.spec.module
        || evidence.fences != run.leases
        || evidence.commit.as_deref() != Some(record.candidate.as_str())
        || evidence.base.as_deref() != Some(record.authority.as_str())
        || evidence.authority.as_deref() != Some(record.authority.as_str())
        || event.data["evidence"] != json!(evidence)
        || record.hold != hold(&record)?
        || evidence.extra != BTreeMap::from([("main_failure".into(), json!(context(&record)))])
    {
        return Err("main failure producer/context/Hold binding differs".into());
    }
    for oid in [&record.candidate, &record.authority] {
        contract::object_id(oid)?;
    }
    contract::object_id(
        evidence
            .tree
            .as_deref()
            .ok_or("main failure lacks actual tree")?,
    )?;
    Ok(record)
}
pub(crate) fn project(projection: &mut StateProjection, event: &Event) -> Result<(), String> {
    let record = decode(event)?;
    if event.stale {
        return Ok(());
    }
    let evidence = json!(record.evidence);
    let id = record.evidence.id.clone();
    let hold_id = record.hold["id"]
        .as_str()
        .ok_or("main failure Hold lacks ID")?
        .to_owned();
    for (kind, key, value) in [
        ("Evidence", &id, &evidence),
        ("Hold", &hold_id, &record.hold),
    ] {
        if projection
            .objects
            .get(kind)
            .and_then(|objects| objects.get(key))
            .is_some_and(|old| old != value)
        {
            return Err("conflicting main failure identity".into());
        }
    }
    projection
        .objects
        .entry("Evidence".into())
        .or_default()
        .insert(id, evidence);
    projection
        .objects
        .entry("Hold".into())
        .or_default()
        .insert(hold_id, record.hold);
    Ok(())
}
pub(crate) fn strip_holds(projection: &mut StateProjection) -> Result<(), String> {
    let candidates = projection
        .objects
        .get("Hold")
        .into_iter()
        .flat_map(|holds| holds.iter())
        .filter(|(_, hold)| hold["kind"] == "gatefailed" && hold["gate"] == "G5")
        .map(|(id, hold)| (id.clone(), hold.clone()))
        .collect::<Vec<_>>();
    for (id, raw) in candidates {
        let evidence: GateEvidence = serde_json::from_value(
            projection
                .objects
                .get("Evidence")
                .and_then(|objects| raw["evidence"].as_str().and_then(|id| objects.get(id)))
                .cloned()
                .ok_or("main failure Hold lacks Evidence provenance")?,
        )
        .map_err(|e| e.to_string())?;
        let c: MainGateFailureContext = serde_json::from_value(
            evidence
                .extra
                .get("main_failure")
                .cloned()
                .ok_or("main failure lacks exact provenance")?,
        )
        .map_err(|e| e.to_string())?;
        let record = MainGateFailureRecorded {
            schema: c.schema,
            evidence: evidence.clone(),
            hold: raw.clone(),
            claim: c.claim,
            candidate: c.candidate,
            authority: c.authority,
            trailers: c.trailers,
            observed_through_seq: c.observed_through_seq,
            rejection: c.rejection,
        };
        let original = NewEvent {
            eid: recorded::request_event_eid(
                evidence
                    .id
                    .strip_prefix("E-")
                    .ok_or("main failure lacks ID")?,
                "evidence.recorded",
            )?,
            host: record.claim.host.clone(),
            actor: "process:towermd-gates".into(),
            kind: "evidence.recorded".into(),
            layer: "L5".into(),
            obj: evidence.id.clone(),
            guards: evidence.fences.clone(),
            data: json!({"evidence":evidence,"main_gate_failure":record}),
        };
        decode_original(&original)?;
        projection
            .objects
            .get_mut("Hold")
            .expect("selected Hold")
            .remove(&id);
    }
    if projection
        .objects
        .get("Hold")
        .is_some_and(BTreeMap::is_empty)
    {
        projection.objects.remove("Hold");
    }
    Ok(())
}
