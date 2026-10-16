use std::collections::BTreeMap;

use serde_json::{json, Value};
use towermd_core::{
    catalog::Catalog,
    config::Config,
    hub::Hub,
    hub_protocol::*,
    okf::Document,
    project_session::ProjectSession,
    tasks::{TaskCommand, TaskSpec},
};

fn catalog(root: &std::path::Path) -> Catalog {
    let mut objects = BTreeMap::new();
    for (id, kind, fields) in [
        ("A", "Module", "paths: ['src/a/**']"),
        ("B", "Module", "paths: ['src/b/**']"),
        ("AC", "Acceptance", "module: A\ncriteria: [works]"),
    ] {
        objects.insert(
            id.into(),
            Document::parse(&format!("---\ntype: {kind}\nid: {id}\n{fields}\n---\n")).unwrap(),
        );
    }
    Catalog {
        root: root.into(),
        common_dir: root.join(".git"),
        branch: "t/T".into(),
        revision: "authority".into(),
        config: Config::new("project", "docs/tower").unwrap(),
        objects,
        object_paths: BTreeMap::new(),
        protected_paths: vec![],
    }
}

fn add(session: &mut ProjectSession, catalog: &Catalog) -> Value {
    session
        .execute(
            catalog,
            "host",
            TaskCommand::Add {
                task: TaskSpec {
                    id: "T".into(),
                    title: "Task".into(),
                    module: "A".into(),
                    touches: vec!["B".into()],
                    allowed_paths: vec!["src/a/file.rs".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC".into()],
                    decisions: vec![],
                },
            },
            None,
            1,
        )
        .unwrap()
}

struct ChangedFence {
    inner: Hub,
    during_release: bool,
    replacement: Option<Fence>,
}

impl ChangedFence {
    fn replace(&mut self, old: Fence, now: u64) {
        self.inner
            .lease_release(
                ReleaseLease {
                    host: "racer".into(),
                    fences: vec![old.clone()],
                },
                now,
            )
            .unwrap();
        self.replacement = Some(
            self.inner
                .lease_acquire(
                    AcquireLease {
                        host: "racer".into(),
                        names: vec![old.name],
                        holder: "replacement".into(),
                        ttl_ms: 1000,
                    },
                    now,
                )
                .unwrap()
                .leases[0]
                .fence(),
        );
    }
}

impl HubProtocol for ChangedFence {
    fn append(&mut self, event: NewEvent, now: u64) -> Result<AppendReceipt, HubError> {
        if event.kind == "task.claimed" {
            if !self.during_release {
                self.replace(
                    event
                        .guards
                        .iter()
                        .find(|f| f.name == "module:B")
                        .unwrap()
                        .clone(),
                    now,
                );
            }
            return Err(HubError::Invalid("definite append rejection".into()));
        }
        self.inner.append(event, now)
    }
    fn subscribe(&mut self, after: u64, now: u64) -> Result<SubscriptionBatch, HubError> {
        self.inner.subscribe(after, now)
    }
    fn lease_acquire(&mut self, request: AcquireLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_acquire(request, now)
    }
    fn lease_renew(&mut self, request: RenewLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_renew(request, now)
    }
    fn lease_release(&mut self, request: ReleaseLease, now: u64) -> Result<LeaseReceipt, HubError> {
        if self.during_release && self.replacement.is_none() {
            self.replace(request.fences[0].clone(), now);
        }
        self.inner.lease_release(request, now)
    }
    fn snapshot_ack(&mut self, ack: SnapshotAck, checkpoint: Value) -> Result<(), HubError> {
        HubProtocol::snapshot_ack(&mut self.inner, ack, checkpoint)
    }
    fn rebuild(&mut self, seed: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        self.inner.rebuild(seed)
    }
    fn contiguous_seq(&self) -> u64 {
        self.inner.contiguous_seq()
    }
}

#[test]
fn definite_claim_failure_compensates_remaining_original_fences_after_partial_replacement() {
    for during_release in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let catalog = catalog(dir.path());
        let mut session = ProjectSession::new(
            Box::new(ChangedFence {
                inner: Hub::open(&dir.path().join("hub")).unwrap(),
                during_release,
                replacement: None,
            }),
            0,
        )
        .unwrap();
        add(&mut session, &catalog);
        let error = session
            .execute(
                &catalog,
                "host",
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                None,
                2,
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "definite append rejection");
        let leases = session.leases(2);
        assert_eq!(
            leases.len(),
            1,
            "still-valid original Run fence must be compensated"
        );
        let replacement = &leases[if during_release {
            "module:A"
        } else {
            "module:B"
        }];
        assert_eq!(replacement.holder, "replacement");
        assert_eq!(replacement.epoch, 3);
        assert_eq!(session.projection.objects["Task"]["T"]["state"], "ready");
        assert!(!session.projection.objects.contains_key("Run"));
        assert!(session.tasks.pending_claim.is_none());
    }
}

#[test]
fn claim_requires_all_declared_modules_in_both_atomic_business_projections() {
    for complete in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let catalog = catalog(dir.path());
        let mut session = ProjectSession::open(&dir.path().join("hub")).unwrap();
        let mut task = add(&mut session, &catalog);
        task["state"] = json!("running");
        task["future"] = json!({"nested":[null, {"opaque":true}]});
        let grant = session
            .hub
            .lease_acquire(
                AcquireLease {
                    host: "host".into(),
                    names: if complete {
                        vec!["module:A".into(), "module:B".into()]
                    } else {
                        vec!["module:A".into()]
                    },
                    holder: "run-1".into(),
                    ttl_ms: 1000,
                },
                2,
            )
            .unwrap();
        session.sync(2).unwrap();
        let before = session.projection.clone();
        let guards: Vec<_> = grant.leases.iter().map(Lease::fence).collect();
        let receipt = session.hub.append(NewEvent { eid: uuid::Uuid::now_v7().to_string(), host: "host".into(), actor: "process:towermd-cli".into(), kind: "task.claimed".into(), layer: "L5".into(), obj: "T".into(), data: json!({"task":task,"run":{"type":"Run","id":"run-1","task":"T","host":"host","worktree":catalog.root,"branch":catalog.branch,"state":"running","leases":guards,"cost_usd":null,"task_card":{},"future":[null,1]}}), guards }, 2).unwrap();
        let AppendReceipt::Event(event) = receipt else {
            panic!("expected new event")
        };
        let mut standalone = before.clone();
        if complete {
            standalone.apply_recorded(&event).unwrap();
            session.sync(2).unwrap();
            assert_eq!(session.projection, standalone);
            assert_eq!(
                session.projection.objects["Task"]["T"]["future"],
                task["future"]
            );
            assert_eq!(
                session.projection.objects["Run"]["run-1"]["future"],
                json!([null, 1])
            );
            assert!(session.projection.objects["Run"]["run-1"]["cost_usd"].is_null());
            let shown = session
                .tasks
                .execute(
                    &mut *session.hub,
                    &catalog,
                    "host",
                    TaskCommand::Show { id: None },
                    None,
                    2,
                )
                .unwrap();
            assert_eq!(shown["id"], "run-1");
        } else {
            let projection_result = standalone.apply_recorded(&event);
            let session_result = session.sync(2);
            assert!(
                projection_result.is_err(),
                "projection accepted incomplete declared module fences"
            );
            assert_eq!(standalone, before);
            assert!(
                session_result.is_err(),
                "session accepted incomplete declared module fences"
            );
            assert_eq!(session.projection, before);
            let status = session
                .tasks
                .execute(
                    &mut *session.hub,
                    &catalog,
                    "host",
                    TaskCommand::Status,
                    None,
                    2,
                )
                .unwrap();
            assert_eq!(status["tasks"][0]["state"], "ready");
            assert_eq!(status["runs"], json!([]));
        }
    }
}

#[test]
fn v1_claim_with_complete_acquired_modules_restores_fences_and_unknown_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hub");
    std::fs::create_dir(&path).unwrap();
    // The v1 producer recorded an empty Run.leases and acquired both modules
    // atomically in the same L4 claim's LeaseChange, without a top-level epoch.
    let task = json!({"type":"Task","id":"T","title":"Task","module":"A","touches":["B"],"allowed_paths":["src/a/file.rs"],"depends_on":[],"acceptance":["AC"],"decisions":[],"state":"running","future":{"opaque":null}});
    let legacy = json!({"version":1,"events":[{
        "seq":1,"eid":uuid::Uuid::now_v7().to_string(),"host":"host",
        "t":"1970-01-01T00:00:00.002Z","actor":"process:towermd-cli",
        "type":"task.claimed","layer":"L4","obj":"T","epoch":null,
        "stale":false,"cost_usd":null,"fence":null,
        "data":{"task":task,"run":{"type":"Run","id":"run-1","task":"T","host":"host","worktree":dir.path(),"branch":"t/T","state":"running","leases":[],"cost_usd":null,"task_card":{},"future":[null,1]}},
        "lease_delta":{"change":{"Acquire":{"names":["module:A","module:B"],"holder":"run-1","ttl_ms":900000}},"updated":[
            {"name":"module:A","holder":"run-1","epoch":1,"expires_at":900002},
            {"name":"module:B","holder":"run-1","epoch":2,"expires_at":900002}
        ]}
    }]});
    let bytes = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(path.join("journal.json"), &bytes).unwrap();
    let hub = Hub::open(&path).unwrap();
    let mut session = ProjectSession::new(Box::new(hub), 2).unwrap();
    assert_eq!(session.projection.objects["Task"]["T"], task);
    let run = &session.projection.objects["Run"]["run-1"];
    assert_eq!(run["leases"].as_array().unwrap().len(), 2);
    assert_eq!(run["future"], json!([null, 1]));
    assert!(run["cost_usd"].is_null());
    let catalog = catalog(dir.path());
    let status = session
        .tasks
        .execute(
            &mut *session.hub,
            &catalog,
            "host",
            TaskCommand::Status,
            None,
            2,
        )
        .unwrap();
    assert_eq!(status["tasks"][0]["future"], task["future"]);
    assert_eq!(status["runs"][0]["future"], json!([null, 1]));
    assert_eq!(status["runs"][0]["leases"].as_array().unwrap().len(), 2);
    assert_eq!(std::fs::read(path.join("journal.json")).unwrap(), bytes);
    drop(session);
    let reopened = ProjectSession::new(Box::new(Hub::open(&path).unwrap()), 2).unwrap();
    assert_eq!(reopened.projection.objects["Task"]["T"], task);
    assert_eq!(
        reopened.projection.objects["Run"]["run-1"]["leases"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
