use serde_json::{json, Value};

use super::resolve_after_merge;
use towermd_core::hub::Event;

fn merged() -> Event {
    serde_json::from_value(json!({
        "seq": 20, "eid": "merge-original", "t": "2026-10-21T00:00:00Z",
        "host": "test", "actor": "process:towermd", "type": "task.merged",
        "layer": "L4", "obj": "T-one", "data": {}, "epoch": 1,
        "stale": false, "cost_usd": null, "guards": []
    }))
    .unwrap()
}

fn failed(task: &str, evidence: &str) -> Value {
    json!({
        "event": format!("raised-{evidence}"), "resolved": false,
        "at": "2026-10-20T00:00:00Z",
        "data": {"reason": "task_gate_failed", "detail": {
            "task": task, "evidence_id": evidence, "severity": "blocking", "error": "test failed"
        }}
    })
}

#[test]
fn merge_resolves_all_earlier_failures_of_its_task_and_preserves_evidence() {
    let mut state = json!({
        "Alert": {
            "gate-old-one": failed("T-one", "old-one"),
            "gate-old-two": failed("T-one", "old-two"),
            "gate-success": failed("T-one", "success"),
            "gate-other": failed("T-other", "other"),
            "protected": {"resolved": false, "data": {
                "reason": "unapproved_protected_change", "detail": {"task": "T-one"}
            }},
            "already-cleared": {"resolved": true, "resolved_by": "previous",
                "data": {"reason": "task_gate_failed", "detail": {"task": "T-one"}}}
        },
        "Evidence": {
            "old-one": {"status": "failed", "task": "T-one", "error": "original failure"},
            "old-two": {"status": "interrupted", "task": "T-one"},
            "success": {"status": "merged", "task": "T-one"}
        }
    });
    let original = state.clone();
    resolve_after_merge(&mut state, &merged());
    for id in ["gate-old-one", "gate-old-two", "gate-success"] {
        assert_eq!(state["Alert"][id]["resolved"], true);
        assert_eq!(state["Alert"][id]["resolved_by"], "merge-original");
        assert_eq!(state["Alert"][id]["data"], original["Alert"][id]["data"]);
        assert_eq!(state["Alert"][id]["event"], original["Alert"][id]["event"]);
    }
    for id in ["gate-other", "protected", "already-cleared"] {
        assert_eq!(state["Alert"][id], original["Alert"][id]);
    }
    assert_eq!(state["Evidence"], original["Evidence"]);
}

#[test]
fn passing_verification_or_a_stale_merge_does_not_clear_failures() {
    let original = json!({"Alert": {"gate-old": failed("T-one", "old")}});
    for kind in ["task.finish.completed", "task.g5.completed"] {
        let mut event = merged();
        event.kind = kind.into();
        event.data = json!({"passed": true});
        let mut state = original.clone();
        resolve_after_merge(&mut state, &event);
        assert_eq!(state, original);
    }
    let mut event = merged();
    event.stale = true;
    let mut state = original.clone();
    resolve_after_merge(&mut state, &event);
    assert_eq!(state, original);
}

#[test]
fn ordered_replay_keeps_later_failures_unresolved() {
    fn replay() -> Value {
        let mut state = crate::dev_state::initial();
        state["Alert"]["gate-old"] = failed("T-one", "old");
        resolve_after_merge(&mut state, &merged());
        let mut later = merged();
        later.seq = 21;
        later.eid = "later-failure".into();
        later.kind = "alert.raised".into();
        later.layer = "L5".into();
        later.obj = "gate-later".into();
        later.data = failed("T-one", "later")["data"].clone();
        crate::dev_state::apply(&mut state, &later).unwrap();
        state
    }
    let state = replay();
    assert_eq!(state["Alert"]["gate-old"]["resolved_by"], "merge-original");
    assert_eq!(state["Alert"]["gate-later"]["resolved"], false);
    assert!(state["Alert"]["gate-later"].get("resolved_by").is_none());
    assert_eq!(state, replay());
}
