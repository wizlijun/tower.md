use serde_json::json;
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
use towermd_core::snapshot::{archive_events, Snapshot};
#[test]
fn state_json_has_no_receipts_and_heartbeat_ranges_are_merged() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(dir.path()).unwrap();
    let grant = hub
        .lease_acquire(
            AcquireLease {
                host: "host".into(),
                names: vec!["main".into()],
                holder: "writer".into(),
                ttl_ms: 10000,
            },
            1,
        )
        .unwrap();
    let fence = grant.leases[0].fence();
    for now in 2..1002 {
        hub.lease_renew(
            RenewLease {
                host: "host".into(),
                fences: vec![fence.clone()],
                ttl_ms: 10000,
            },
            now,
        )
        .unwrap();
    }
    let events = hub.subscribe(0, 1002).unwrap().events;
    let snapshot = Snapshot::from_events("project", &events, 1001, fence.epoch).unwrap();
    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(value["format_version"], json!(2));
    assert!(value.get("receipts").is_none());
    assert_eq!(snapshot.omitted.len(), 1);
    assert_eq!(snapshot.omitted[0].start, 2);
    assert_eq!(snapshot.omitted[0].end, 1001);
    snapshot
        .validate_archive("project", &archive_events(&events, 0, 1001).unwrap())
        .unwrap();
}
