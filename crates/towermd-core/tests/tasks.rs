use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};
use support::Session as Hub;
use towermd_core::catalog::Catalog;
use towermd_core::hub_protocol::*;
use towermd_core::okf::Document;
use towermd_core::tasks::{execute, TaskCommand, TaskSpec};

const TTL: u64 = 15 * 60 * 1000;

mod support;
fn catalog(root: &Path, branch: &str) -> Catalog {
    fs::create_dir_all(root).unwrap();
    if !root.join(".git").exists() {
        support::catalog(root);
        support::git(root, &["switch", "main"]);
        for (id, kind, fields) in [
            ("A", "Module", "paths: ['src/a/**']"),
            ("B", "Module", "paths: ['src/b/**']"),
            ("C", "Module", "paths: ['src/c/**']"),
            ("AC-A", "Acceptance", "module: A\ncriteria: [works]"),
            ("AC-B", "Acceptance", "module: B\ncriteria: [works]"),
            ("AC-C", "Acceptance", "module: C\ncriteria: [works]"),
            ("D-1", "Decision", "decision: keep it local"),
        ] {
            fs::write(
                root.join(format!("docs/tower/{id}.md")),
                format!("---\ntype: {kind}\nid: {id}\n{fields}\n---\n"),
            )
            .unwrap();
        }
        support::git(root, &["add", "docs"]);
        support::git(root, &["commit", "-qm", "task context"]);
        support::git(root, &["switch", "t/T"]);
    }
    let mut c = Catalog::load(root).unwrap();
    c.branch = branch.into();
    c
}

fn linked_catalog(first: &Catalog, root: &Path, branch: &str) -> Catalog {
    support::git(
        &first.root,
        &[
            "worktree",
            "add",
            "-qb",
            branch,
            root.to_str().unwrap(),
            "main",
        ],
    );
    Catalog::load(root).unwrap()
}

fn task(id: &str, module: &str) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        title: format!("Implement {id}"),
        module: module.into(),
        touches: vec![],
        allowed_paths: vec![format!("src/{}/file.rs", module.to_lowercase())],
        depends_on: vec![],
        acceptance: vec![format!("AC-{module}")],
        decisions: vec![],
    }
}

fn call(hub: &mut Hub, catalog: &Catalog, command: TaskCommand, now: u64) -> Result<Value, String> {
    execute(
        &hub.cell,
        catalog,
        &support::inputs(catalog),
        "host",
        command,
        None,
        now,
    )
    .map_err(|e| e.to_string())
}

#[test]
fn readonly_renewal_skips_expired_implicit_binding_and_rejects_explicit_binding() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = catalog(temporary.path(), "t/T-1");
    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    let run = claim(&mut hub, &catalog, "T-1", 2);
    let before = hub.last_seq();
    towermd_core::tasks::renew_binding(
        &mut hub.cell.lock().unwrap(),
        &catalog,
        "host",
        None,
        TTL + 2,
    )
    .unwrap();
    assert!(towermd_core::tasks::renew_binding(
        &mut hub.cell.lock().unwrap(),
        &catalog,
        "host",
        run["id"].as_str(),
        TTL + 2,
    )
    .is_err());
    assert_eq!(hub.last_seq(), before);
    let system = hub
        .cell
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before, TTL + 2)
        .unwrap();
    assert_eq!(system.events.len(), 1);
    assert_eq!(system.events[0].kind, "lease.expired");
    assert!(system.head.leases.is_empty());
    assert_eq!(
        hub.projection().unwrap().objects["Run"][run["id"].as_str().unwrap()]["state"],
        "running"
    );
}

fn expire(hub: &mut Hub, host: &str, now: u64) -> Result<(), String> {
    let catalog = Catalog::load(&hub.root)?;
    towermd_core::tasks::expire(&hub.cell, &catalog, &support::inputs(&catalog), host, now)
}
fn add(hub: &mut Hub, catalog: &Catalog, spec: TaskSpec, now: u64) -> Value {
    call(hub, catalog, TaskCommand::Add { task: spec }, now).unwrap()
}

fn claim(hub: &mut Hub, catalog: &Catalog, id: &str, now: u64) -> Value {
    call(
        hub,
        catalog,
        TaskCommand::Claim {
            id: id.into(),
            harness: None,
            harness_version: None,
        },
        now,
    )
    .unwrap()
}

#[test]
fn lifecycle_is_atomic_replayable_and_keeps_unknown_cost() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("hub");
    let catalog = catalog(temporary.path(), "t/T-1");
    let mut hub = support::open_session(&path, &catalog, 0);
    let created = add(&mut hub, &catalog, task("T-1", "A"), 10);
    assert_eq!(created["type"], "Task");
    assert_eq!(created["state"], "ready");
    assert_eq!(created["allowed_paths"], json!(["src/a/file.rs"]));
    let sequence = hub.last_seq();
    let run = claim(&mut hub, &catalog, "T-1", 20);
    assert!(!hub.events(sequence).iter().any(|event| event.obj == "T-1"
        && event.kind == "task.state"
        && event.data["task"]["state"] == "ready"));
    assert_eq!(run["type"], "Run");
    assert_eq!(run["state"], "running");
    assert_eq!(run["task"], "T-1");
    assert_eq!(run["cost_usd"], Value::Null);
    assert_eq!(run["task_card"]["id"], "T-1");
    assert_eq!(run["task_card"]["allowed_paths"], json!(["src/a/file.rs"]));
    assert_eq!(run["task_card"]["acceptance"], json!(["AC-A"]));
    assert_eq!(hub.events(sequence)[0].kind, "lease.granted");
    assert_eq!(run["leases"][0]["name"], "module:A");
    assert_eq!(run["leases"][0]["holder"], run["id"]);
    assert!(run["leases"][0]["epoch"].as_u64().unwrap() > 0);
    assert_eq!(
        uuid::Uuid::parse_str(run["id"].as_str().unwrap().trim_start_matches("R-"))
            .unwrap()
            .get_version_num(),
        7
    );
    drop(hub);
    let mut hub = support::open_session(&path, &catalog, 0);
    assert_eq!(
        call(&mut hub, &catalog, TaskCommand::Show { id: None }, 30).unwrap(),
        run
    );
    assert_eq!(
        call(
            &mut hub,
            &catalog,
            TaskCommand::Show {
                id: Some("T-1".into())
            },
            31
        )
        .unwrap()["state"],
        "running"
    );
    let released = call(&mut hub, &catalog, TaskCommand::Release, 40).unwrap();
    assert_eq!(released["state"], "ready");
    assert!(hub.leases(40).is_empty());
    assert!(hub.events(0).iter().all(|event| event.actor
        == if event.kind.starts_with("lease.") {
            "process:towermd-hub"
        } else {
            "process:towermd-cli"
        }
        && event.cost_usd.is_none()));
    drop(hub);
    let mut hub = support::open_session(&path, &catalog, 0);
    let status = call(&mut hub, &catalog, TaskCommand::Status, 50).unwrap();
    assert_eq!(status["project_id"], "project");
    assert_eq!(status["seq"], hub.last_seq());
    assert_eq!(status["tasks"][0]["state"], "ready");
    assert_eq!(status["runs"][0]["state"], "released");
}

#[test]
fn g0_rejects_invalid_inputs_without_events_and_allows_no_decision() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = catalog(temporary.path(), "main");
    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    let original = serde_json::to_value(task("T-1", "A")).unwrap();
    for (field, value) in [
        ("id", json!("")),
        ("id", json!("../T-1")),
        ("id", json!("-T-1")),
        ("id", json!("T/1")),
        ("id", json!("T..1")),
        ("title", json!(" ")),
        ("module", json!("missing")),
        ("module", json!("D-1")),
        ("touches", json!(["missing"])),
        ("touches", json!(["AC-A"])),
        ("allowed_paths", json!([])),
        ("allowed_paths", json!(["src/b/file.rs"])),
        ("allowed_paths", json!(["src/a/../b/file.rs"])),
        ("allowed_paths", json!(["docs/tower/arch/module.md"])),
        ("allowed_paths", json!([".towermd/config.yaml"])),
        ("acceptance", json!([])),
        ("acceptance", json!(["missing"])),
        ("acceptance", json!(["D-1"])),
        ("acceptance", json!(["AC-B"])),
        ("decisions", json!(["missing"])),
        ("decisions", json!(["A"])),
        ("depends_on", json!(["T-1"])),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        let spec = serde_json::from_value(invalid.clone()).unwrap();
        assert!(
            call(&mut hub, &catalog, TaskCommand::Add { task: spec }, 1).is_err(),
            "{invalid}"
        );
        assert_eq!(hub.last_seq(), 0, "{invalid}");
    }
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    assert!(call(
        &mut hub,
        &catalog,
        TaskCommand::Add {
            task: task("T-1", "A")
        },
        2
    )
    .is_err());
    let mut explicit = task("T-2", "B");
    explicit.decisions.push("D-1".into());
    add(&mut hub, &catalog, explicit, 3);
}

#[test]
fn dependencies_stay_proposed_until_evidence_of_merge_exists() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = catalog(temporary.path(), "t/T-2");
    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    let mut dependent = task("T-2", "B");
    dependent.depends_on.push("T-1".into());
    assert_eq!(add(&mut hub, &catalog, dependent, 1)["state"], "proposed");
    for now in [2, 4] {
        if now == 4 {
            add(&mut hub, &catalog, task("T-1", "A"), 3);
        }
        let before = hub.last_seq();
        let error = call(
            &mut hub,
            &catalog,
            TaskCommand::Claim {
                id: "T-2".into(),
                harness: None,
                harness_version: None,
            },
            now,
        )
        .unwrap_err();
        assert!(
            error.contains("dependencies are missing or not merged"),
            "{error}"
        );
        assert!(!hub.events(before).iter().any(|event| event.obj == "T-2"
            && (event.kind == "task.claimed"
                || event.kind == "task.state" && event.data["task"]["state"] == "ready")));
        assert_eq!(hub.last_seq(), before);
        let status = call(&mut hub, &catalog, TaskCommand::Status, now).unwrap();
        assert!(status["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "T-2" && task["state"] == "proposed"));
        assert!(status["runs"].as_array().unwrap().is_empty());
        assert!(hub.leases(now).is_empty());
    }
}

#[test]
fn multi_module_acquire_is_atomic_and_suggests_only_claimable_tasks() {
    let temporary = tempfile::tempdir().unwrap();
    let first = catalog(&temporary.path().join("first"), "t/T-1");
    let mut hub = support::open_session(&temporary.path().join("hub"), &first, 0);
    let second = linked_catalog(&first, &temporary.path().join("second"), "t/T-2");
    add(&mut hub, &first, task("T-1", "B"), 1);
    let mut multiple = task("T-2", "A");
    multiple.touches = vec!["B".into(), "A".into(), "B".into()];
    multiple.allowed_paths.push("src/b/file.rs".into());
    add(&mut hub, &first, multiple, 2);
    add(&mut hub, &first, task("T-3", "C"), 3);
    let mut invalid = task("T-4", "C");
    invalid.depends_on.push("missing".into());
    add(&mut hub, &first, invalid, 4);
    claim(&mut hub, &first, "T-1", 5);
    let before = hub.last_seq();
    let error = call(
        &mut hub,
        &second,
        TaskCommand::Claim {
            id: "T-2".into(),
            harness: None,
            harness_version: None,
        },
        6,
    )
    .unwrap_err();
    assert!(error.contains("T-3"), "{error}");
    assert!(!error.contains("T-4"), "{error}");
    assert_eq!(hub.last_seq(), before);
    assert_eq!(hub.leases(6).len(), 1);
    call(&mut hub, &first, TaskCommand::Release, 7).unwrap();
    let before = hub.last_seq();
    let run = claim(&mut hub, &second, "T-2", 8);
    assert_eq!(hub.last_seq(), before + 2);
    assert_eq!(run["leases"].as_array().unwrap().len(), 2);
}

#[test]
fn branch_changes_do_not_bind_or_allow_duplicate_worktree_runs() {
    let temporary = tempfile::tempdir().unwrap();
    let mut catalog = catalog(temporary.path(), "main");

    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    add(&mut hub, &catalog, task("T-2", "B"), 2);
    assert!(call(
        &mut hub,
        &catalog,
        TaskCommand::Claim {
            id: "T-1".into(),
            harness: None,
            harness_version: None
        },
        3
    )
    .is_err());
    catalog.branch = "t/T-1".into();
    let run = claim(&mut hub, &catalog, "T-1", 4);
    catalog.branch = "t/T-2".into();
    for command in [TaskCommand::Show { id: None }, TaskCommand::Release] {
        let error = call(&mut hub, &catalog, command, 5).unwrap_err();
        assert!(error.contains(run["id"].as_str().unwrap()), "{error}");
    }
    assert!(call(
        &mut hub,
        &catalog,
        TaskCommand::Claim {
            id: "T-2".into(),
            harness: None,
            harness_version: None
        },
        6
    )
    .is_err());
    assert_eq!(
        execute(
            &hub.cell,
            &catalog,
            &support::inputs(&catalog),
            "host",
            TaskCommand::Show { id: None },
            run["id"].as_str(),
            7
        )
        .unwrap(),
        run
    );
    assert!(execute(
        &hub.cell,
        &catalog,
        &support::inputs(&catalog),
        "other-host",
        TaskCommand::Show { id: None },
        run["id"].as_str(),
        8
    )
    .is_err());
    execute(
        &hub.cell,
        &catalog,
        &support::inputs(&catalog),
        "host",
        TaskCommand::Release,
        run["id"].as_str(),
        9,
    )
    .unwrap();
    claim(&mut hub, &catalog, "T-2", 10);
}

#[test]
fn core_status_keeps_last_verified_view_without_renewing_or_expiring() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = catalog(temporary.path(), "t/T-1");
    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    let run = claim(&mut hub, &catalog, "T-1", 10);
    let before = hub.last_seq();
    let lease = hub.leases(10)["module:A"].clone();
    let status = call(&mut hub, &catalog, TaskCommand::Status, TTL).unwrap();
    assert_eq!(status["runs"][0]["id"], run["id"]);
    assert_eq!(status["runs"][0]["state"], "running");
    assert_eq!(hub.leases(TTL)["module:A"], lease);
    let status = call(&mut hub, &catalog, TaskCommand::Status, TTL + 10).unwrap();
    assert_eq!(status["runs"][0]["state"], "running");
    assert_eq!(hub.last_seq(), before);
    assert!(!hub
        .events(before)
        .iter()
        .any(|event| event.kind == "lease.renewed" || event.kind == "task.released"));
    expire(&mut hub, "host", TTL + 10).unwrap();
    let status = call(&mut hub, &catalog, TaskCommand::Status, TTL + 11).unwrap();
    assert_eq!(status["runs"][0]["state"], "released");
    assert_eq!(status["tasks"][0]["state"], "ready");
    assert!(execute(
        &hub.cell,
        &catalog,
        &support::inputs(&catalog),
        "host",
        TaskCommand::Show { id: None },
        run["id"].as_str(),
        TTL + 11
    )
    .is_err());
}

#[test]
fn every_bound_command_renews_and_expiration_never_revives_old_run() {
    for command in [
        TaskCommand::Show {
            id: Some("T-1".into()),
        },
        TaskCommand::Show { id: None },
        TaskCommand::Add {
            task: task("T-2", "B"),
        },
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("hub");
        let catalog = catalog(temporary.path(), "t/T-1");
        let mut hub = support::open_session(&path, &catalog, 0);
        add(&mut hub, &catalog, task("T-1", "A"), 1);
        let run = claim(&mut hub, &catalog, "T-1", 10);
        call(&mut hub, &catalog, command, TTL).unwrap();
        drop(hub);
        let mut hub = support::open_session(&path, &catalog, 0);
        expire(&mut hub, "host", TTL + 10).unwrap();
        assert_eq!(hub.leases(TTL + 10).len(), 1);
        expire(&mut hub, "host", 2 * TTL).unwrap();
        assert!(hub.leases(2 * TTL).is_empty());
        assert!(execute(
            &hub.cell,
            &catalog,
            &support::inputs(&catalog),
            "host",
            TaskCommand::Show { id: None },
            run["id"].as_str(),
            2 * TTL
        )
        .is_err());
        let status = call(&mut hub, &catalog, TaskCommand::Status, 2 * TTL).unwrap();
        assert_eq!(status["tasks"][0]["state"], "ready");
        assert_eq!(status["runs"][0]["state"], "released");
        let new_run = claim(&mut hub, &catalog, "T-1", 2 * TTL + 1);
        assert_ne!(new_run["id"], run["id"]);
        let leases = serde_json::to_value(hub.leases(2 * TTL + 2)).unwrap();
        assert!(execute(
            &hub.cell,
            &catalog,
            &support::inputs(&catalog),
            "host",
            TaskCommand::Release,
            run["id"].as_str(),
            2 * TTL + 2
        )
        .is_err());
        assert_eq!(
            serde_json::to_value(hub.leases(2 * TTL + 2)).unwrap(),
            leases
        );
    }
}

#[test]
fn expiration_does_not_release_replacement_holder_or_ignore_old_valid_events() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = catalog(temporary.path(), "t/T-1");
    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    let run = claim(&mut hub, &catalog, "T-1", 2);
    hub.cell
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .lease_acquire(
            AcquireLease {
                host: "another".into(),
                names: vec!["module:A".into()],
                holder: "replacement".into(),
                ttl_ms: TTL,
            },
            TTL + 2,
        )
        .unwrap();
    expire(&mut hub, "host", TTL + 3).unwrap();
    assert_eq!(
        hub.leases(TTL + 3)["module:A"].fence().holder,
        "replacement"
    );
    let status = call(&mut hub, &catalog, TaskCommand::Status, TTL + 3).unwrap();
    assert_eq!(status["tasks"][0]["state"], "ready");
    assert_eq!(status["runs"][0]["id"], run["id"]);
    assert_eq!(status["runs"][0]["state"], "released");
}

#[test]
fn claim_revalidates_current_authoritative_objects() {
    for changed in ["A", "AC-A", "D-1"] {
        let temporary = tempfile::tempdir().unwrap();
        let mut catalog = catalog(temporary.path(), "t/T-1");

        let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
        let mut spec = task("T-1", "A");
        spec.decisions.push("D-1".into());
        add(&mut hub, &catalog, spec, 1);
        catalog.objects.remove(changed);
        let before = hub.last_seq();
        assert!(call(
            &mut hub,
            &catalog,
            TaskCommand::Claim {
                id: "T-1".into(),
                harness: None,
                harness_version: None
            },
            2
        )
        .is_err());
        assert_eq!(hub.last_seq(), before);
        assert!(hub.leases(2).is_empty());
    }
}

#[test]
fn g0_requires_real_nonempty_acceptance_criteria_on_add_and_claim() {
    for criteria in [
        "",
        "criteria: []",
        "criteria: ['']",
        "criteria: ['   ']",
        "criteria: null",
        "criteria: works",
        "criteria: [42]",
        "criteria: [works, '']",
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let mut catalog = catalog(temporary.path(), "t/T-1");

        let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
        add(&mut hub, &catalog, task("T-1", "A"), 1);
        catalog.objects.insert(
            "AC-A".into(),
            Document::parse(&format!(
                "---\ntype: Acceptance\nid: AC-A\nmodule: A\n{criteria}\n---\n"
            ))
            .unwrap(),
        );
        let before = hub.last_seq();
        assert!(
            call(
                &mut hub,
                &catalog,
                TaskCommand::Add {
                    task: task("T-2", "A")
                },
                2
            )
            .is_err(),
            "{criteria}"
        );
        assert!(
            call(
                &mut hub,
                &catalog,
                TaskCommand::Claim {
                    id: "T-1".into(),
                    harness: None,
                    harness_version: None
                },
                3
            )
            .is_err(),
            "{criteria}"
        );
        assert_eq!(hub.last_seq(), before);
        assert!(hub.leases(3).is_empty());
    }
}

#[test]
fn automatic_expiration_and_explicit_release_have_distinct_actors() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = catalog(temporary.path(), "t/T-1");
    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    claim(&mut hub, &catalog, "T-1", 2);
    expire(&mut hub, "host", TTL + 2).unwrap();
    assert_eq!(
        hub.events(0).last().unwrap().actor,
        "process:towermd-daemon"
    );
    claim(&mut hub, &catalog, "T-1", TTL + 3);
    call(&mut hub, &catalog, TaskCommand::Release, TTL + 4).unwrap();
    assert_eq!(hub.events(0).last().unwrap().actor, "process:towermd-cli");
    claim(&mut hub, &catalog, "T-1", TTL + 5);
    let before = hub.last_seq();
    call(&mut hub, &catalog, TaskCommand::Status, 2 * TTL + 5).unwrap();
    assert!(!hub
        .events(before)
        .iter()
        .any(|event| event.kind == "task.released"));
    expire(&mut hub, "host", 2 * TTL + 5).unwrap();
    assert_eq!(
        hub.events(before)
            .iter()
            .find(|e| e.kind == "task.released")
            .unwrap()
            .actor,
        "process:towermd-daemon"
    );
}

#[test]
fn stale_task_events_do_not_change_projection_after_restart() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("hub");
    let catalog = catalog(temporary.path(), "t/T-1");
    let mut hub = support::open_session(&path, &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    let first = claim(&mut hub, &catalog, "T-1", 2);
    let old_fence = serde_json::from_value(first["leases"][0].clone()).unwrap();
    call(&mut hub, &catalog, TaskCommand::Release, 3).unwrap();
    let released_event = hub.events(0).last().unwrap().clone();
    let second = claim(&mut hub, &catalog, "T-1", 4);
    let AppendReceipt::Event(stale) = hub
        .cell
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:towermd-cli".into(),
                kind: "task.state".into(),
                layer: "L4".into(),
                obj: "T-1".into(),
                data: released_event.data,
                guards: vec![old_fence],
            },
            5,
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(stale.stale);
    drop(hub);
    let mut hub = support::open_session(&path, &catalog, 0);
    let status = call(&mut hub, &catalog, TaskCommand::Status, 6).unwrap();
    assert_eq!(status["tasks"][0]["state"], "running");
    assert_eq!(
        call(&mut hub, &catalog, TaskCommand::Show { id: None }, 7).unwrap(),
        second
    );
}

struct RejectOne {
    inner: Box<dyn HubProtocol>,
    kind: &'static str,
    rejected: bool,
}
impl HubProtocol for RejectOne {
    fn append(&mut self, event: NewEvent, now: u64) -> Result<AppendReceipt, HubError> {
        if !self.rejected && event.kind == self.kind {
            self.rejected = true;
            return Err(HubError::Invalid("injected deterministic rejection".into()));
        }
        self.inner.append(event, now)
    }
    fn subscribe(&mut self, after: u64, now: u64) -> Result<SubscriptionBatch, HubError> {
        self.inner.subscribe(after, now)
    }
    fn lease_acquire(&mut self, r: AcquireLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_acquire(r, now)
    }
    fn lease_renew(&mut self, r: RenewLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_renew(r, now)
    }
    fn lease_release(&mut self, r: ReleaseLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.inner.lease_release(r, now)
    }
    fn snapshot_ack(&mut self, a: SnapshotAck, c: Value) -> Result<(), HubError> {
        self.inner.snapshot_ack(a, c)
    }
    fn rebuild(&mut self, s: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        self.inner.rebuild(s)
    }
    fn contiguous_seq(&self) -> u64 {
        self.inner.contiguous_seq()
    }
}
fn rejecting_session(path: &Path, kind: &'static str) -> Hub {
    let catalog = catalog(path.parent().unwrap(), "t/T-1");
    Hub::new(
        Box::new(RejectOne {
            inner: Box::new(towermd_core::hub::Hub::open(path).unwrap()),
            kind,
            rejected: false,
        }),
        &catalog,
        &support::inputs(&catalog),
        0,
    )
    .unwrap()
}
#[test]
fn rejected_claim_is_compensated_without_running_projection() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("hub");
    let mut hub = rejecting_session(&path, "task.claimed");
    let catalog = catalog(temporary.path(), "t/T-1");
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    let before = hub.last_seq();
    assert!(call(
        &mut hub,
        &catalog,
        TaskCommand::Claim {
            id: "T-1".into(),
            harness: None,
            harness_version: None
        },
        2
    )
    .unwrap_err()
    .contains("deterministic"));
    assert_eq!(hub.last_seq(), before + 2);
    assert_eq!(
        hub.events(before)
            .iter()
            .map(|e| e.kind.as_str())
            .collect::<Vec<_>>(),
        ["lease.granted", "lease.released"]
    );
    assert!(hub.leases(2).is_empty());
    let status = call(&mut hub, &catalog, TaskCommand::Status, 2).unwrap();
    assert_eq!(status["tasks"][0]["state"], "ready");
    assert_eq!(status["runs"], json!([]));
    claim(&mut hub, &catalog, "T-1", 3);
}
#[test]
fn failed_expiration_business_append_can_be_reconciled_by_next_tick() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("hub");
    let mut hub = rejecting_session(&path, "task.released");
    let catalog = catalog(temporary.path(), "t/T-1");
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    claim(&mut hub, &catalog, "T-1", 2);
    let before = hub.last_seq();
    assert!(expire(&mut hub, "host", TTL + 2).is_err());
    assert_eq!(hub.last_seq(), before + 1);
    assert_eq!(hub.events(before)[0].kind, "lease.expired");
    expire(&mut hub, "host", TTL + 2).unwrap();
    let status = call(&mut hub, &catalog, TaskCommand::Status, TTL + 2).unwrap();
    assert_eq!(status["tasks"][0]["state"], "ready");
    assert_eq!(status["runs"][0]["state"], "released");
    assert_eq!(
        hub.events(before)
            .iter()
            .filter(|e| e.kind == "task.released")
            .count(),
        1
    );
}

#[test]
fn command_protocol_rejects_completion_and_identity_injection() {
    for value in [
        json!({"action": "done", "id": "T-1"}),
        json!({"action": "finish", "id": "T-1"}),
        json!({"action": "approve"}),
        json!({"action": "status", "actor": "human:admin"}),
    ] {
        assert!(
            serde_json::from_value::<TaskCommand>(value.clone()).is_err(),
            "{value}"
        );
    }
    let spec: TaskSpec =
        serde_json::from_value(json!({"id": "T-1", "title": "Task", "module": "A"})).unwrap();
    assert!(spec.touches.is_empty() && spec.depends_on.is_empty() && spec.decisions.is_empty());
}

#[test]
fn claim_and_show_expand_authoritative_execution_context() {
    let temporary = tempfile::tempdir().unwrap();
    catalog(temporary.path(), "t/T-1");

    git(temporary.path(), &["switch", "main"]);
    for (id, text) in [
        ("A", "---\ntype: Module\nid: A\npaths: ['src/a/**']\nverify: ['cargo test -p a']\ninterfaces: [IF-1]\n---\nModule instructions.\n"),
        ("AC-A", "---\ntype: Acceptance\nid: AC-A\nmodule: A\ncriteria: [preserve data]\nverify: ['cargo test round_trip']\n---\nAcceptance details.\n"),
        ("IF-1", "---\ntype: Interface\nid: IF-1\nfrom: A\nto: B\ncontract: input is UTF-8\n---\nInterface details.\n"),
        ("D-1", "---\ntype: Decision\nid: D-1\ndecision: keep it local\nrationale: offline use\n---\nDecision details.\n"),
    ] {
        fs::write(temporary.path().join(format!("docs/tower/{id}.md")), text).unwrap();
    }
    git(temporary.path(), &["add", "docs"]);
    git(temporary.path(), &["commit", "-qm", "execution context"]);
    git(temporary.path(), &["switch", "-c", "t/T-1"]);
    let mut catalog = Catalog::load(temporary.path()).unwrap();
    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    let mut spec = task("T-1", "A");
    spec.decisions.push("D-1".into());
    add(&mut hub, &catalog, spec, 1);
    let run = claim(&mut hub, &catalog, "T-1", 2);
    let card = &run["task_card"];
    assert_eq!(
        card["revision"],
        git(temporary.path(), &["rev-parse", "main"])
    );
    assert_eq!(card["allowed_paths"], json!(["src/a/file.rs"]));
    assert_eq!(card["acceptance"], json!(["AC-A"]));
    assert_eq!(
        card["context"]["acceptance"][0]["criteria"],
        json!(["preserve data"])
    );
    assert_eq!(
        card["context"]["acceptance"][0]["verify"],
        json!(["cargo test round_trip"])
    );
    assert_eq!(
        card["context"]["acceptance"][0]["body"],
        "Acceptance details.\n"
    );
    assert_eq!(
        card["context"]["modules"][0]["verify"],
        json!(["cargo test -p a"])
    );
    assert_eq!(card["context"]["modules"][0]["interfaces"], json!(["IF-1"]));
    assert_eq!(
        card["context"]["interfaces"][0]["contract"],
        "input is UTF-8"
    );
    assert_eq!(
        card["context"]["interfaces"][0]["body"],
        "Interface details.\n"
    );
    assert_eq!(card["context"]["decisions"][0]["decision"], "keep it local");
    assert_eq!(card["context"]["decisions"][0]["rationale"], "offline use");
    assert_eq!(
        card["context"]["decisions"][0]["body"],
        "Decision details.\n"
    );
    let restrictions = card["restrictions"].to_string();
    for protected in [
        ".towermd",
        ".git",
        "docs/tower/state",
        "docs/tower/tower.canvas",
        "allowed_paths",
        "L0",
        "L2",
        "towermd finish",
    ] {
        assert!(
            restrictions.contains(protected),
            "missing {protected}: {restrictions}"
        );
    }
    let shown = call(
        &mut hub,
        &catalog,
        TaskCommand::Show {
            id: Some("T-1".into()),
        },
        3,
    )
    .unwrap();
    assert_eq!(shown["context"], card["context"]);
    assert_eq!(shown["id"], "T-1");
    assert_eq!(shown["type"], "Task");
    let original_revision = catalog.revision.clone();
    git(temporary.path(), &["switch", "main"]);
    catalog
        .objects
        .get_mut("AC-A")
        .unwrap()
        .set("criteria", serde_yaml::from_str("[new criterion]").unwrap())
        .unwrap();
    fs::write(
        temporary.path().join("docs/tower/AC-A.md"),
        catalog.objects["AC-A"].to_text(),
    )
    .unwrap();
    git(temporary.path(), &["add", "docs/tower/AC-A.md"]);
    git(
        temporary.path(),
        &["commit", "-qm", "new acceptance criterion"],
    );
    git(temporary.path(), &["switch", "t/T-1"]);
    catalog = Catalog::load(temporary.path()).unwrap();
    assert_ne!(catalog.revision, original_revision);
    assert_eq!(
        catalog.revision,
        git(temporary.path(), &["rev-parse", "main"])
    );
    let shown_run = call(&mut hub, &catalog, TaskCommand::Show { id: None }, 4).unwrap();
    assert_eq!(shown_run["task_card"]["revision"], catalog.revision);
    assert_eq!(
        shown_run["task_card"]["context"]["acceptance"][0]["criteria"],
        json!(["new criterion"])
    );
    let status = call(&mut hub, &catalog, TaskCommand::Status, 5).unwrap();
    assert_eq!(status["runs"][0]["task_card"], *card);
    call(&mut hub, &catalog, TaskCommand::Release, 6).unwrap();
    let status = call(&mut hub, &catalog, TaskCommand::Status, 7).unwrap();
    assert_eq!(status["runs"][0]["task_card"], *card);
}

#[test]
fn harness_reporting_is_optional_untrusted_and_persisted() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("hub");
    let catalog = catalog(temporary.path(), "t/T-1");
    let mut hub = support::open_session(&path, &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    let command: TaskCommand = serde_json::from_value(json!({"action": "claim", "id": "T-1", "harness": "a custom harness", "harness_version": "next build"})).unwrap();
    let run = call(&mut hub, &catalog, command, 2).unwrap();
    let expected = json!({"harness": "a custom harness", "version": "next build"});
    assert_eq!(run["self_reported"], expected);
    let event = hub.events(0).pop().unwrap();
    assert_eq!(event.data["self_reported"], expected);
    assert_eq!(event.actor, "process:towermd-cli");
    assert!(event.cost_usd.is_none());
    drop(hub);
    let mut hub = support::open_session(&path, &catalog, 0);
    assert_eq!(
        call(&mut hub, &catalog, TaskCommand::Show { id: None }, 3).unwrap()["self_reported"],
        expected
    );
    call(&mut hub, &catalog, TaskCommand::Release, 4).unwrap();
    let command = serde_json::from_value(json!({"action": "claim", "id": "T-1"})).unwrap();
    let run = call(&mut hub, &catalog, command, 5).unwrap();
    assert!(run["self_reported"].is_null());
    assert!(hub.events(0).pop().unwrap().data["self_reported"].is_null());
}

#[test]
fn incomplete_or_blank_harness_reporting_is_rejected_before_claim() {
    for (harness, version) in [
        (json!("custom"), Value::Null),
        (Value::Null, json!("v1")),
        (json!(""), json!("v1")),
        (json!("  \n"), json!("v1")),
        (json!("custom"), json!("")),
        (json!("custom"), json!(" \t")),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let catalog = catalog(temporary.path(), "t/T-1");

        let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
        add(&mut hub, &catalog, task("T-1", "A"), 1);
        let before = hub.last_seq();
        let command: TaskCommand = serde_json::from_value(
            json!({"action": "claim", "id": "T-1", "harness": harness, "harness_version": version}),
        )
        .unwrap();
        assert!(call(&mut hub, &catalog, command, 2).is_err());
        assert_eq!(hub.last_seq(), before);
        assert!(hub.leases(2).is_empty());
    }
}

#[test]
fn protected_slow_objects_outside_fixed_directories_block_add_and_claim() {
    for allowed in ["src/a/decision.md", "src/a/**", "src/a/*.md"] {
        let temporary = tempfile::tempdir().unwrap();
        let mut catalog = catalog(temporary.path(), "t/T-1");

        let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
        catalog
            .objects
            .get_mut("A")
            .unwrap()
            .set(
                "paths",
                serde_yaml::from_str("['src/a/**', 'src/a/*.md']").unwrap(),
            )
            .unwrap();
        let mut spec = task("T-1", "A");
        spec.allowed_paths = vec![allowed.into()];
        add(&mut hub, &catalog, spec.clone(), 1);
        catalog.protected_paths = vec!["src/a/decision.md".into()];
        let before = hub.last_seq();
        let command: TaskCommand =
            serde_json::from_value(json!({"action": "claim", "id": "T-1"})).unwrap();
        assert!(call(&mut hub, &catalog, command, 2).is_err(), "{allowed}");
        spec.id = "T-2".into();
        assert!(
            call(&mut hub, &catalog, TaskCommand::Add { task: spec }, 3).is_err(),
            "{allowed}"
        );
        assert_eq!(hub.last_seq(), before);
        assert!(hub.leases(3).is_empty());
        let mut safe = task("T-3", "A");
        safe.allowed_paths = vec!["src/a/implementation.rs".into()];
        add(&mut hub, &catalog, safe, 4);
    }
}

#[test]
fn execution_card_lists_actual_protected_slow_object_paths() {
    let temporary = tempfile::tempdir().unwrap();
    let mut catalog = catalog(temporary.path(), "t/T-1");

    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    catalog.protected_paths = vec!["docs/tower/decisions/D-1.md".into()];
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    let run = claim(&mut hub, &catalog, "T-1", 2);
    assert!(run["task_card"]["restrictions"]["protected_paths"]
        .as_array()
        .unwrap()
        .contains(&json!("docs/tower/decisions/D-1.md")));
}

#[test]
fn real_catalog_protects_external_slow_objects_and_ignores_context_tampering() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    fs::create_dir_all(root.join(".towermd")).unwrap();
    fs::create_dir_all(root.join("docs/tower/decisions")).unwrap();
    fs::create_dir_all(root.join("src/a")).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    let context = tempfile::tempdir().unwrap();
    let fixture = catalog(context.path(), "main");
    fs::write(root.join(".towermd/config.yaml"), fixture.config.to_yaml()).unwrap();
    for (id, document) in fixture.objects {
        fs::write(
            root.join(format!("docs/tower/decisions/{id}.md")),
            document.to_text(),
        )
        .unwrap();
    }
    fs::write(root.join("src/a/decision.md"), "---\ntype: Decision\nid: external-decision\nlayer: L1\ndecision: protected\n---\nExternal slow object.\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "authoritative context"]);
    git(root, &["checkout", "-qb", "t/T-1"]);
    fs::write(
        root.join("docs/tower/decisions/D-1.md"),
        "---\ntype: Decision\nid: D-1\ndecision: forged\n---\nForged working tree context.\n",
    )
    .unwrap();
    git(root, &["add", "docs/tower/decisions/D-1.md"]);
    let catalog = Catalog::load(root).unwrap();
    let mut hub = support::open_session(&catalog.common_dir.join("towermd"), &catalog, 0);
    let mut blocked = task("T-blocked", "A");
    blocked.allowed_paths = vec!["src/a/decision.md".into()];
    assert!(call(&mut hub, &catalog, TaskCommand::Add { task: blocked }, 1).is_err());
    assert_eq!(hub.last_seq(), 0);
    let mut spec = task("T-1", "A");
    spec.decisions = vec!["D-1".into()];
    add(&mut hub, &catalog, spec, 2);
    let run = claim(&mut hub, &catalog, "T-1", 3);
    assert_eq!(
        run["task_card"]["context"]["decisions"][0]["decision"],
        "keep it local"
    );
    assert_eq!(
        run["task_card"]["revision"],
        git(root, &["rev-parse", "main"])
    );
    assert!(run["task_card"]["restrictions"]["protected_paths"]
        .as_array()
        .unwrap()
        .contains(&json!("src/a/decision.md")));
}

#[test]
fn missing_referenced_interface_cannot_leave_partial_claim() {
    let temporary = tempfile::tempdir().unwrap();
    let mut catalog = catalog(temporary.path(), "t/T-1");

    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    catalog
        .objects
        .get_mut("A")
        .unwrap()
        .set(
            "interfaces",
            serde_yaml::from_str("[missing-interface]").unwrap(),
        )
        .unwrap();
    let before = hub.last_seq();
    let command = serde_json::from_value(json!({"action": "claim", "id": "T-1"})).unwrap();
    let error = call(&mut hub, &catalog, command, 2).unwrap_err();
    assert!(error.contains("missing-interface"), "{error}");
    assert_eq!(hub.last_seq(), before);
    assert!(hub.leases(2).is_empty());
}

#[test]
fn same_task_contention_also_reports_other_claimable_tasks() {
    let temporary = tempfile::tempdir().unwrap();
    let first = catalog(&temporary.path().join("first"), "t/T-1");
    let mut hub = support::open_session(&temporary.path().join("hub"), &first, 0);
    let second = linked_catalog(&first, &temporary.path().join("second"), "t/T-1");
    add(&mut hub, &first, task("T-1", "A"), 1);
    add(&mut hub, &first, task("T-2", "B"), 2);
    claim(&mut hub, &first, "T-1", 3);
    let before = hub.last_seq();
    let error = call(
        &mut hub,
        &second,
        TaskCommand::Claim {
            id: "T-1".into(),
            harness: None,
            harness_version: None,
        },
        4,
    )
    .unwrap_err();
    assert!(error.contains("T-2"), "{error}");
    assert_eq!(hub.last_seq(), before);
}

#[test]
fn wrong_branch_or_host_does_not_renew_implicit_binding() {
    let temporary = tempfile::tempdir().unwrap();
    let mut catalog = catalog(temporary.path(), "t/T-1");

    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    claim(&mut hub, &catalog, "T-1", 2);
    let before = hub.last_seq();
    execute(
        &hub.cell,
        &catalog,
        &support::inputs(&catalog),
        "other-host",
        TaskCommand::Status,
        None,
        TTL - 2,
    )
    .unwrap();
    catalog.branch = "main".into();
    call(&mut hub, &catalog, TaskCommand::Status, TTL - 1).unwrap();
    assert_eq!(hub.last_seq(), before);
    expire(&mut hub, "host", TTL + 2).unwrap();
    let status = call(&mut hub, &catalog, TaskCommand::Status, TTL + 2).unwrap();
    assert_eq!(status["runs"][0]["state"], "released");
}

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let output = command
        .args([
            "-c",
            "user.name=Tasks Test",
            "-c",
            "user.email=tasks@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}

#[test]
fn real_worktrees_resolve_separate_runs_and_do_not_mutate_git_or_documents() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("repository");
    fs::create_dir_all(root.join(".towermd")).unwrap();
    fs::create_dir_all(root.join("docs/tower/arch")).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    let context = tempfile::tempdir().unwrap();
    let fixture = catalog(context.path(), "main");
    fs::write(root.join(".towermd/config.yaml"), fixture.config.to_yaml()).unwrap();
    for (id, document) in fixture.objects {
        fs::write(
            root.join(format!("docs/tower/arch/{id}.md")),
            document.to_text(),
        )
        .unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "fixture"]);
    let first_root = temporary.path().join("first");
    let second_root = temporary.path().join("second");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-qb",
            "t/T-1",
            first_root.to_str().unwrap(),
        ],
    );
    git(
        &root,
        &[
            "worktree",
            "add",
            "-qb",
            "t/T-2",
            second_root.to_str().unwrap(),
        ],
    );
    let first = Catalog::load(&first_root).unwrap();
    let second = Catalog::load(&second_root).unwrap();
    let original_status = git(&first_root, &["status", "--porcelain"]);
    let original_head = git(&first_root, &["rev-parse", "HEAD"]);
    let mut hub = support::open_session(&first.common_dir.join("towermd"), &first, 0);
    add(&mut hub, &first, task("T-1", "A"), 1);
    add(&mut hub, &second, task("T-2", "B"), 2);
    let first_run = claim(&mut hub, &first, "T-1", 3);
    let second_run = claim(&mut hub, &second, "T-2", 4);
    assert_eq!(
        call(&mut hub, &first, TaskCommand::Show { id: None }, 5).unwrap(),
        first_run
    );
    assert_eq!(
        call(&mut hub, &second, TaskCommand::Show { id: None }, 6).unwrap(),
        second_run
    );
    assert_eq!(
        git(&first_root, &["status", "--porcelain"]),
        original_status
    );
    assert_eq!(git(&first_root, &["rev-parse", "HEAD"]), original_head);
    assert_eq!(git(&second_root, &["status", "--porcelain"]), "");
}
#[test]
fn occupied_claim_preserves_structured_lease_owners() {
    let temporary = tempfile::tempdir().unwrap();
    let first = catalog(&temporary.path().join("first"), "t/T-1");
    let mut hub = support::open_session(&temporary.path().join("hub"), &first, 0);
    let second = linked_catalog(&first, &temporary.path().join("second"), "t/T-2");
    add(&mut hub, &first, task("T-1", "A"), 1);
    add(&mut hub, &first, task("T-2", "A"), 2);
    let run = claim(&mut hub, &first, "T-1", 3);
    let error = execute(
        &hub.cell,
        &second,
        &support::inputs(&second),
        "host",
        TaskCommand::Claim {
            id: "T-2".into(),
            harness: None,
            harness_version: None,
        },
        None,
        4,
    )
    .unwrap_err();
    let owners = error.owners.unwrap();
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].holder, run["id"]);
    assert_eq!(owners[0].name, "module:A");
    assert_eq!(owners[0].expires_at, TTL + 3);
}
#[test]
fn task_and_run_unknown_keys_survive_replay_and_release_with_null_cost() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = catalog(temporary.path(), "t/T-1");
    let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
    add(&mut hub, &catalog, task("T-1", "A"), 1);
    claim(&mut hub, &catalog, "T-1", 2);
    let mut data = hub.events(0).last().unwrap().data.clone();
    data["task"]["future"] = json!({"unknown":[null,false]});
    data["run"]["future"] = json!([1, null]);
    hub.cell
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:test".into(),
                kind: "task.state".into(),
                layer: "L4".into(),
                obj: "T-1".into(),
                data,
                guards: vec![],
            },
            3,
        )
        .unwrap();
    hub = support::sync_session(hub, &catalog, 3);
    call(&mut hub, &catalog, TaskCommand::Release, 4).unwrap();
    let event = hub.events(0).last().unwrap().clone();
    assert_eq!(event.kind, "task.released");
    assert_eq!(
        event.data["task"]["future"],
        json!({"unknown":[null,false]})
    );
    assert_eq!(event.data["run"]["future"], json!([1, null]));
    assert!(event.data["run"]["cost_usd"].is_null());
}

#[test]
fn real_invalid_layer_blocks_add_and_claim_without_mutating_task_state() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("repo");
    let _ = catalog(&root, "main");
    support::git(&root, &["switch", "-qc", "t/T-invalid"]);
    let before = Catalog::load(&root).unwrap();
    let mut hub = support::open_session(&temporary.path().join("hub"), &before, 0);
    let mut spec = task("T-invalid", "A");
    spec.allowed_paths = vec!["src/a/invalid.md".into()];
    assert_eq!(add(&mut hub, &before, spec.clone(), 1)["state"], "ready");
    support::git(&root, &["switch", "main"]);
    fs::create_dir_all(root.join("src/a")).unwrap();
    let original = "---\ntype: Task\nlayer: null\nfuture: {retain: [yes, 0.5]}\n---\nKeep bytes.\n";
    fs::write(root.join("src/a/invalid.md"), original).unwrap();
    support::git(&root, &["add", "src/a/invalid.md"]);
    support::git(&root, &["commit", "-qm", "invalid layer authority"]);
    support::git(&root, &["switch", "t/T-invalid"]);
    let catalog = Catalog::load(&root).unwrap();
    let seq = hub.last_seq();
    let error = call(
        &mut hub,
        &catalog,
        TaskCommand::Claim {
            id: "T-invalid".into(),
            harness: None,
            harness_version: None,
        },
        2,
    )
    .unwrap_err();
    assert!(
        error.contains("保护") && error.contains("src/a/invalid.md"),
        "{error}"
    );
    spec.id = "T-blocked".into();
    let error = call(&mut hub, &catalog, TaskCommand::Add { task: spec }, 3).unwrap_err();
    assert!(
        error.contains("保护") && error.contains("src/a/invalid.md"),
        "{error}"
    );
    assert_eq!(hub.last_seq(), seq);
    assert!(hub.leases(3).is_empty());
    let status = call(&mut hub, &catalog, TaskCommand::Status, 4).unwrap();
    assert_eq!(status["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(status["tasks"][0]["state"], "ready");
    assert!(status["runs"].as_array().unwrap().is_empty());
    assert_eq!(
        catalog.read_file("src/a/invalid.md").unwrap().unwrap(),
        original
    );
    let mut safe = task("T-safe", "A");
    safe.allowed_paths = vec!["src/a/safe.rs".into()];
    add(&mut hub, &catalog, safe, 5);
    support::git(&root, &["switch", "-qc", "t/T-safe"]);
    let safe = Catalog::load(&root).unwrap();
    assert_eq!(claim(&mut hub, &safe, "T-safe", 6)["state"], "running");
}

#[test]
fn missing_run_diagnostic_lists_only_active_local_runs() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("repo");
    let _ = catalog(&root, "main");
    support::git(&root, &["switch", "-qc", "t/T-released"]);
    let released_catalog = Catalog::load(&root).unwrap();
    let mut hub = support::open_session(&temporary.path().join("hub"), &released_catalog, 0);
    add(&mut hub, &released_catalog, task("T-released", "A"), 1);
    let released = claim(&mut hub, &released_catalog, "T-released", 2);
    call(&mut hub, &released_catalog, TaskCommand::Release, 3).unwrap();
    support::git(&root, &["switch", "-qc", "t/T-expired"]);
    let expired_catalog = Catalog::load(&root).unwrap();
    add(&mut hub, &expired_catalog, task("T-expired", "B"), 4);
    let expired = claim(&mut hub, &expired_catalog, "T-expired", 5);
    call(&mut hub, &expired_catalog, TaskCommand::Status, TTL + 6).unwrap();
    support::git(&root, &["switch", "-qc", "t/T-active"]);
    let active_catalog = Catalog::load(&root).unwrap();
    add(&mut hub, &active_catalog, task("T-active", "A"), TTL + 7);
    let active = claim(&mut hub, &active_catalog, "T-active", TTL + 8);
    add(&mut hub, &active_catalog, task("T-other", "C"), TTL + 9);
    let other_catalog = linked_catalog(
        &active_catalog,
        &temporary.path().join("other"),
        "t/T-other",
    );
    let other = execute(
        &hub.cell,
        &other_catalog,
        &support::inputs(&other_catalog),
        "other-host",
        TaskCommand::Claim {
            id: "T-other".into(),
            harness: None,
            harness_version: None,
        },
        None,
        TTL + 10,
    )
    .unwrap();
    let error = execute(
        &hub.cell,
        &active_catalog,
        &support::inputs(&active_catalog),
        "host",
        TaskCommand::Show { id: None },
        Some("missing"),
        TTL + 11,
    )
    .unwrap_err()
    .to_string();
    let list: Value = serde_json::from_str(error.split("local runs: ").nth(1).unwrap()).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], active["id"]);
    assert_eq!(list[0]["state"], "running");
    for excluded in [&released, &expired, &other] {
        assert!(!list
            .as_array()
            .unwrap()
            .iter()
            .any(|run| run["id"] == excluded["id"]));
    }
    let status = call(&mut hub, &active_catalog, TaskCommand::Status, TTL + 12).unwrap();
    let history = status["runs"].as_array().unwrap();
    assert_eq!(history.len(), 4);
    for old in [&released, &expired] {
        assert!(history
            .iter()
            .any(|run| run["id"] == old["id"] && run["state"] == "released"));
    }
    assert!(history
        .iter()
        .any(|run| run["id"] == other["id"] && run["state"] == "running"));
}

#[test]
fn proposed_task_g0_or_branch_failure_keeps_zero_ready_claim_run_and_lease() {
    for scenario in ["decision", "protected", "branch"] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("repo");
        let _ = catalog(&root, "main");
        let mut spec = task("T-proposed", "A");
        if scenario == "decision" {
            spec.decisions = vec!["missing-decision".into()];
        }
        if scenario == "protected" {
            support::git(&root, &["switch", "main"]);
            fs::create_dir_all(root.join("src/a")).unwrap();
            fs::write(
                root.join("src/a/guard.md"),
                "---\ntype: Task\nlayer: null\n---\nProtected.\n",
            )
            .unwrap();
            support::git(&root, &["add", "src/a/guard.md"]);
            support::git(&root, &["commit", "-qm", "actual protected source"]);
            spec.allowed_paths = vec!["src/a/guard.md".into()];
        }
        support::git(
            &root,
            &[
                "switch",
                "-qc",
                if scenario == "branch" {
                    "t/wrong"
                } else {
                    "t/T-proposed"
                },
            ],
        );
        let catalog = Catalog::load(&root).unwrap();
        let mut hub = support::open_session(&temporary.path().join("hub"), &catalog, 0);
        let mut stored = serde_json::to_value(spec).unwrap();
        stored["type"] = "Task".into();
        stored["state"] = "proposed".into();
        let receipt = hub
            .cell
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .append(
                NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "host".into(),
                    actor: "process:test-fixture".into(),
                    kind: "task.state".into(),
                    layer: "L4".into(),
                    obj: "T-proposed".into(),
                    data: json!({"task":stored}),
                    guards: vec![],
                },
                1,
            )
            .unwrap();
        assert!(matches!(receipt, AppendReceipt::Event(_)));
        towermd_core::project_session::ProjectSession::sync_shared(
            &hub.cell,
            &catalog,
            &support::inputs(&catalog),
            1,
        )
        .unwrap();
        let before = hub.last_seq();
        let error = call(
            &mut hub,
            &catalog,
            TaskCommand::Claim {
                id: "T-proposed".into(),
                harness: None,
                harness_version: None,
            },
            2,
        )
        .unwrap_err();
        let expected = match scenario {
            "decision" => "missing-decision",
            "protected" => "保护",
            _ => "requires an existing t/T-proposed branch",
        };
        assert!(error.contains(expected), "{scenario}: {error}");
        assert!(!hub
            .events(before)
            .iter()
            .any(|event| event.obj == "T-proposed"
                && (event.kind == "task.claimed"
                    || event.kind == "task.state" && event.data["task"]["state"] == "ready")));
        assert_eq!(hub.last_seq(), before);
        let status = call(&mut hub, &catalog, TaskCommand::Status, 3).unwrap();
        assert_eq!(status["tasks"][0]["state"], "proposed");
        assert!(status["runs"].as_array().unwrap().is_empty());
        assert!(hub.leases(3).is_empty());
    }
}
