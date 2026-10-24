//! Core projection reducer. The core owns admission, controls, alerts,
//! snapshots, recovery and lease bookkeeping; every other business event is
//! offered to the assembled kits in order. Self-reported Agent text cannot
//! transition an object to completion.
use serde_json::{json, Map, Value};

use crate::{hub::Event, kit::Kit};

/// Groups every assembly has, after the kit groups.
pub const CORE_GROUPS: [&str; 4] = ["Hold", "Proposal", "Prompt", "Alert"];

pub fn initial_with(kits: &[&dyn Kit]) -> Value {
    let mut groups = Map::new();
    for name in kits.iter().flat_map(|kit| kit.state_groups().iter()).chain(CORE_GROUPS.iter()) {
        groups.insert((*name).to_owned(), json!({}));
    }
    Value::Object(groups)
}

pub fn core_initial() -> Value {
    initial_with(&[])
}

pub fn core_apply(state: &mut Value, event: &Event) -> Result<(), String> {
    apply_with(&[], state, event)
}

pub fn apply_with(kits: &[&dyn Kit], state: &mut Value, event: &Event) -> Result<(), String> {
    if event.stale {
        return Ok(());
    }
    if crate::resolution_projection::apply(state, event)? { return Ok(()); }
    if crate::project_controls::apply(state, event, kits)? { return Ok(()); }
    if crate::delegation_projection::apply(state, event, kits)? { return Ok(()); }
    if crate::tower_api::apply(state, event)? { return Ok(()); }
    if crate::requests::apply(state, event)? { return Ok(()); }
    if crate::gates::apply(state, event)? { return observe(kits, state, event); }
    if crate::ideas::apply(state, event, kits)? { return Ok(()); }
    if crate::follow_ups::apply(state, event)? { return Ok(()); }
    if crate::temp_rules::apply(state, event)? { return Ok(()); }
    if crate::waivers::apply(state, event)? { return Ok(()); }
    if crate::reviews::apply(state, event)? { return observe(kits, state, event); }
    if crate::frames::apply(state, event, kits)? { return Ok(()); }
    match event.kind.as_str() {
        "hub.rebuild.confirmed" => {
            if event.actor != "human:local"
                || event.layer != "L5"
                || event.obj.trim().is_empty()
                || event.data != json!({"request_id":event.eid})
            {
                return Err("Hub 重建须有 App 人工确认原件".into());
            }
        }
        "alert.raised" => {
            insert_unique(
                state,
                "Alert",
                &event.obj,
                json!({
                    "id":event.obj,"event":event.eid,"data":event.data,"resolved":false,
                }),
            )?;
            crate::resolution_projection::reconcile_alerts(state)?;
        }
        "alert.cleared" => {
            if !event.actor.starts_with("human:") && event.actor != "process:towermd" {
                return Err("告警不能由任意调用者清除".into());
            }
            if let Some(entry) = state["Alert"].get_mut(&event.obj) {
                entry["resolved"] = json!(true);
            }
        }
        "main.inspected" => {
            if event.actor != "process:towermd" || event.layer != "L5"
                || event.guards.len()!=1 || event.guards[0].name!="main"
                || event.guards[0].epoch==0 || event.epoch!=Some(event.guards[0].epoch)
                || event.data["main"].as_str().is_none_or(|oid| !valid_oid(oid))
                || event.data["complete"].as_bool().is_none()
                || event.data["passed"].as_bool().is_none()
            { return Err("main 检查记录须由守护进程保存真实固定提交".into()); }
            state["MainInspection"] = json!({"eid":event.eid,"main":event.data["main"],
                "previous_main":event.data["previous_main"],"complete":event.data["complete"],"passed":event.data["passed"],
                "error":event.data["error"]});
            if event.data["complete"] == false {
                let alert_id = crate::events::derived_eid(event.data["main"].as_str().unwrap(),
                    "alert.raised.inspection_incomplete")?;
                if let Some(alert) = state["Alert"].get_mut(&alert_id) {
                    // A new failed observation may revisit a previously
                    // cleared SHA; keep the original alert and expose the
                    // latest reason from this accepted observation.
                    alert["resolved"] = json!(false);
                    alert["inspection_eid"] = json!(event.eid);
                    alert["data"]["detail"]["error"] = event.data["error"].clone();
                }
            }
        }
        "hold.opened" | "hold.answered" | "hold.decision_reused"
        | "prompt.queued" | "prompt.delivered" | "prompt.handled" => {
            crate::question_projection::apply(state, event)?;
            return observe(kits, state, event);
        }
        "snapshot.written" => {
            let proof: crate::snapshot_publication::SnapshotProof =
                serde_json::from_value(event.data.clone()).map_err(|e|e.to_string())?;
            if proof.seq >= event.seq || proof.epoch == 0 || event.obj.is_empty() {
                return Err("快照回执水位或项目无效".into());
            }
            proof.check_receipt(&event.obj, event)?;
        }
        "recovery.completed" => {
            let jump: crate::snapshot::RecoveryJump =
                serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            if event.actor != "process:towermd-recovery"
                || event.layer != "L5"
                || event.obj.is_empty()
                || event.seq <= jump.seq_boundary
                || jump.source_seq > jump.seq_boundary
                || jump.source_epoch > jump.epoch_boundary
                || event.guards.len() != 1
                || event.guards[0].name != "main"
                || event.guards[0].epoch <= jump.epoch_boundary
            {
                return Err("恢复完成缺少越过原边界的新主干 fence".into());
            }
            for kit in kits {
                kit.reduce(state, event)?;
            }
        }
        "lease.expired" | "lease.released" => {
            if event.actor != "process:towermd-hub" {
                return Err("租约终止只能来自hub系统事件".into());
            }
            if !matches!(
                &event.lease_delta,
                Some(crate::hub::SystemDelta::Expire { .. } | crate::hub::SystemDelta::Release { .. })
            ) {
                return Err("租约终止事件缺少完整系统原件".into());
            }
            for kit in kits {
                kit.reduce(state, event)?;
            }
        }
        // Hub lease bookkeeping and snapshot notifications do not certify any
        // business outcome. Their envelopes remain in the checked archive.
        name if name.starts_with("lease.") => {}
        _ => {
            for kit in kits {
                if kit.reduce(state, event)? {
                    return Ok(());
                }
            }
            return Err(format!("尚未支持的业务事件：{}", event.kind));
        }
    }
    Ok(())
}

/// Kits see the core events that bear on their objects once the core has
/// applied them: questions, reviews and gate evidence.
fn observe(kits: &[&dyn Kit], state: &mut Value, event: &Event) -> Result<(), String> {
    for kit in kits {
        kit.observe(state, event)?;
    }
    Ok(())
}

pub fn insert_unique(state: &mut Value, kind: &str, id: &str, value: Value) -> Result<(), String> {
    let group = state
        .get_mut(kind)
        .and_then(Value::as_object_mut)
        .ok_or("项目投影格式无效")?;
    match group.get(id) {
        Some(old) if old != &value => Err(format!("{kind} 对象已有不同原件：{id}")),
        Some(_) => Ok(()),
        None => {
            group.insert(id.into(), value);
            Ok(())
        }
    }
}

pub fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
