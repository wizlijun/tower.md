use serde_json::json;
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
use towermd_core::project_session::ProjectSession;
#[test]
fn no_change_status_and_tick_apply_nothing_and_reopen_restores_tail() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    HubProtocol::append(&mut hub,NewEvent{eid:uuid::Uuid::now_v7().to_string(),host:"host".into(),actor:"process:test".into(),kind:"task.state".into(),layer:"L4".into(),obj:"T".into(),data:json!({"task":{"type":"Task","id":"T","title":"Task","module":"A","state":"ready"}}),guards:vec![]},1).unwrap();
    let mut session = ProjectSession::new(Box::new(hub), 1).unwrap();
    session.sync(1).unwrap();
    let count = session.applied_count();
    for _ in 0..200 {
        session.sync(1).unwrap();
    }
    assert_eq!(session.applied_count(), count);
    let projection = session.projection.clone();
    drop(session);
    let mut session = ProjectSession::new(Box::new(Hub::open(dir.path()).unwrap()), 1).unwrap();
    session.sync(1).unwrap();
    assert_eq!(session.projection, projection);
}
#[test]
fn invalid_typed_business_event_leaves_both_projection_and_cursor_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = ProjectSession::open(dir.path()).unwrap();
    let before = session.projection.clone();
    session
        .hub
        .append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:test".into(),
                kind: "task.state".into(),
                layer: "L4".into(),
                obj: "T".into(),
                data: json!({"task":{"type":"Task","id":"T","state":"running"}}),
                guards: vec![],
            },
            1,
        )
        .unwrap();
    assert!(session.sync(1).is_err());
    assert_eq!(session.projection, before);
    assert_eq!(session.cursor, 0);
    assert_eq!(session.applied_count(), 0);
}
