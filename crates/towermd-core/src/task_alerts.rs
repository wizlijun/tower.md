//! Derived alert resolution after the canonical task merge has been validated.
use serde_json::{json, Value};

use crate::hub::Event;

/// Called only inside the accepted `task.merged` reducer branch, after checking
/// the evidence, G5 receipt, run, and merge proof. A passed verification alone
/// does not resolve a failure. Replay order limits this to existing alerts;
/// later failures remain unresolved until a later valid merge.
pub(crate) fn resolve_after_merge(state: &mut Value, event: &Event) {
    if event.kind != "task.merged" || event.stale {
        return;
    }
    let Some(alerts) = state.get_mut("Alert").and_then(Value::as_object_mut) else {
        return;
    };
    for alert in alerts.values_mut() {
        if alert["resolved"] == true
            || alert["status"] == "resolved"
            || alert["state"] == "resolved"
            || alert["data"]["reason"] != "task_gate_failed"
            || alert["data"]["detail"]["task"].as_str() != Some(event.obj.as_str())
        {
            continue;
        }
        alert["resolved"] = json!(true);
        alert["resolved_by"] = json!(event.eid);
    }
}

#[cfg(test)]
#[path = "task_alerts_tests.rs"]
mod tests;
