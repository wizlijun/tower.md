//! Pure event projection for human main resolutions. Git authentication stays
//! in main_resolution and is performed before producing/recovering the fact.
use serde_json::{json, Value};
use crate::{events, hub::Event, main_resolution::{self, Operation, Preview, Resolution}};

pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if !matches!(event.kind.as_str(), "main.resolution.approved" | "main.change.resolved") { return Ok(false); }
    if event.stale { return Ok(true); }
    let mut next = state.clone();
    match event.kind.as_str() {
        "main.resolution.approved" => {
            let preview = main_resolution::from_approval(event)?;
            put(&mut next, "MainResolutionApproval", &event.eid, serde_json::to_value(preview).map_err(|error| error.to_string())?)?;
        }
        _ => {
            let resolution: Resolution = serde_json::from_value(event.data.clone()).map_err(|error| error.to_string())?;
            let preview: Preview = serde_json::from_value(next["MainResolutionApproval"][&resolution.approval_eid].clone())
                .map_err(|_| "主干处理缺少已接受的人工确认")?;
            if event.eid != events::derived_eid(&resolution.target_commit, "main.change.resolved")?
                || !matches!(event.actor.as_str(), "process:towermd" | "process:towermd-recovery")
                || event.layer != "L1" || event.obj != resolution.target_commit
                || event.guards.len() != 1 || event.guards[0].name != "main"
                || event.epoch != Some(event.guards[0].epoch)
                || resolution.approval_eid != preview.request_id || resolution.content_hash != preview.content_hash
                || resolution.target_commit != preview.target_commit || resolution.operation != preview.operation
                || preview.hash()? != preview.content_hash
                || (resolution.operation == Operation::Claim && resolution.resolution_commit != resolution.target_commit)
                || (resolution.operation == Operation::Revert && resolution.resolution_commit == resolution.target_commit)
            { return Err("主干处理事件与人工确认或主干租约不一致".into()); }
            events::derived_eid(&resolution.resolution_commit, "main.change.resolved")?;
            put(&mut next, "MainResolution", &resolution.target_commit,
                json!({"event":event.eid,"resolution":resolution}))?;
            reconcile_alerts(&mut next)?;
        }
    }
    *state = next;
    Ok(true)
}

/// Also call after inserting alert.raised, so a delayed detection cannot reopen
/// a commit whose actual resolution has already been accepted.
pub fn reconcile_alerts(state: &mut Value) -> Result<(), String> {
    let records = state.get("MainResolution").and_then(Value::as_object).cloned().unwrap_or_default();
    for record in records.values() {
        let resolution: Resolution = serde_json::from_value(record["resolution"].clone()).map_err(|error| error.to_string())?;
        for commit in [&resolution.target_commit, &resolution.resolution_commit] {
            let id = events::derived_eid(commit, "alert.raised.unapproved_protected_change")?;
            if let Some(alert) = state["Alert"].get_mut(&id) {
                alert["resolved"] = json!(true);
                alert["resolution_event"] = record["event"].clone();
            }
        }
    }
    Ok(())
}

fn put(state: &mut Value, group: &str, key: &str, value: Value) -> Result<(), String> {
    let object = state.as_object_mut().ok_or("业务投影不是对象")?;
    let entries = object.entry(group).or_insert_with(|| json!({})).as_object_mut().ok_or("处理记录不是对象")?;
    if entries.get(key).is_some_and(|previous| previous != &value) { return Err("主干处理原件冲突".into()); }
    entries.insert(key.into(), value);
    Ok(())
}
