//! Fake time with actual imported Git authority, Task Add/Claim and core Ask
//! producers. These tests do not certify a human App flow or P7 approval.
use super::super::tests::{snapshot_fixture, snapshot_git, snapshot_server};
use super::*;
use towermd_core::{human_intervention::HumanCommand, tasks::TaskSpec};

const START: u64 = 1_800_000_000_000;
const LONG_INTERVAL: u64 = 7_200_000;
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    runtime: PathBuf,
    catalog: Catalog,
    server: Arc<Server>,
    project: Arc<Project>,
    unix: Arc<AtomicU64>,
    mono: Arc<AtomicU64>,
    hold: String,
    wait_id: String,
    deadline: u64,
    original_expiry: u64,
}
fn block_snapshot(project: &Project) {
    let mut schedule = project.schedule.lock().unwrap();
    // Scheduling state is a derived fixture, not a fabricated accepted fact.
    // Hold writer retry eligibility beyond both Ask deadlines.
    for _ in 0..20 {
        schedule.deferred_at(
            "test snapshot writer backoff".into(),
            ClockReading {
                mono_ms: 0,
                unix_ms: START,
            },
            LONG_INTERVAL,
        );
    }
    assert!(!schedule.can_attempt(1_800_001));
}
fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    snapshot_git(&root, &["init", "-qb", "main"]);
    towermd_core::init::init(&root, &towermd_core::init::InitOptions { docs_dir: None }).unwrap();
    let config = root.join(".towermd/config.yaml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(config, format!("{text}\nsnapshot_interval: 7200\n")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/a.rs"), "// actual source\n").unwrap();
    fs::create_dir_all(root.join("docs/tower/arch")).unwrap();
    fs::create_dir_all(root.join("docs/tower/specs")).unwrap();
    fs::write(
        root.join("docs/tower/arch/A.md"),
        "---\ntype: Module\nid: A\npaths: ['src/**']\nverify: []\ninterfaces: []\n---\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/tower/specs/AC.md"),
        "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [actual Ask scheduling]\n---\n",
    )
    .unwrap();
    // The entire first Tower enabling commit is the explicit import point.
    snapshot_git(&root, &["add", "."]);
    snapshot_git(&root, &["commit", "-qm", "actual imported Ask authority"]);
    snapshot_git(&root, &["switch", "-qc", "t/T"]);
    let runtime = temp.path().join("runtime");
    let unix = Arc::new(AtomicU64::new(START));
    let mono = Arc::new(AtomicU64::new(0));
    let server = snapshot_server(&root, runtime.clone(), mono.clone(), unix.clone());
    server
        .execute(Request {
            directory: root.clone(),
            run: None,
            command: TaskCommand::Add {
                task: TaskSpec {
                    id: "T".into(),
                    title: "actual Ask".into(),
                    module: "A".into(),
                    touches: vec![],
                    allowed_paths: vec!["src/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC".into()],
                    decisions: vec![],
                },
            },
        })
        .unwrap();
    server
        .execute(Request {
            directory: root.clone(),
            run: None,
            command: TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
        })
        .unwrap();
    let catalog = Catalog::load(&root).unwrap();
    let project = server.project(&catalog).unwrap();
    let original_expiry = project.hub.lock().unwrap().leases(START)["module:A"].expires_at;
    let mut outbox = server.task_outbox(&catalog).unwrap();
    let opened = ProjectSession::ask_shared(
        &project.hub,
        &catalog,
        &server.endpoint.host,
        &Uuid::now_v7().to_string(),
        &HumanCommand::AskOpen {
            question: "scheduling question?".into(),
            options: vec![],
            recommended: None,
            context: String::new(),
        },
        None,
        &mut outbox,
        || Ok(START),
    )
    .unwrap();
    let wait_id = Uuid::now_v7().to_string();
    let waiting = ProjectSession::ask_shared(
        &project.hub,
        &catalog,
        &server.endpoint.host,
        &wait_id,
        &HumanCommand::AskWait {
            hold: opened.hold.clone(),
        },
        None,
        &mut outbox,
        || Ok(START),
    )
    .unwrap();
    let deadline = waiting.wait_until.unwrap();
    assert_eq!(deadline, START + 90_000);
    block_snapshot(&project);
    Fixture {
        _temp: temp,
        root,
        runtime,
        catalog,
        server,
        project,
        unix,
        mono,
        hold: opened.hold,
        wait_id,
        deadline,
        original_expiry,
    }
}
fn advance(f: &Fixture, elapsed: u64) {
    f.unix.store(START + elapsed, Ordering::SeqCst);
    f.mono.store(elapsed, Ordering::SeqCst);
}
async fn tick(f: &Fixture) {
    f.server.tick().unwrap();
    f.server.drain_snapshots().await.unwrap();
}
fn finished(f: &Fixture) -> Option<u64> {
    f.project.hub.lock().unwrap().human().unwrap().holds[&f.hold].waits[0].finished_ms
}
fn actual(f: &Fixture) -> Vec<towermd_core::hub_protocol::Event> {
    f.server
        .recovery_inputs(&f.catalog)
        .unwrap()
        .confirmed()
        .to_vec()
}

#[tokio::test]
async fn long_snapshot_backoff_and_restart_do_not_delay_unattended_slice_or_attention() {
    let mut f = fixture();
    advance(&f, 89_999);
    tick(&f).await;
    assert_eq!(finished(&f), None);
    // Release the actual old Hub/project lock, then rebuild all Ask scheduling
    // state from persistent actual history. No CLI or wait poll resumes it.
    f.server.projects.lock().unwrap().clear();
    drop(f.project);
    drop(f.server);
    f.server = snapshot_server(&f.root, f.runtime.clone(), f.mono.clone(), f.unix.clone());
    f.catalog = Catalog::load(&f.root).unwrap();
    f.project = f.server.project(&f.catalog).unwrap();
    block_snapshot(&f.project);
    advance(&f, 90_000);
    tick(&f).await;
    assert_eq!(finished(&f), Some(f.deadline));
    let recorded = actual(&f);
    let ended = recorded
        .iter()
        .filter(|e| e.kind == "hold.wait_finished" && e.obj == f.hold)
        .collect::<Vec<_>>();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].data["p3_request"]["request_id"], f.wait_id);
    assert!(ended[0].guards.is_empty());
    assert_eq!(
        f.project.hub.lock().unwrap().leases(START + 90_000)["module:A"].expires_at,
        f.original_expiry
    );
    assert!(!f.project.schedule.lock().unwrap().can_attempt(90_000));
    advance(&f, 1_799_999);
    tick(&f).await;
    assert!(!f.project.hub.lock().unwrap().human().unwrap().holds[&f.hold].attention);
    advance(&f, 1_800_000);
    tick(&f).await;
    assert!(f.project.hub.lock().unwrap().human().unwrap().holds[&f.hold].attention);
    assert!(!f
        .project
        .hub
        .lock()
        .unwrap()
        .leases(START + 1_800_000)
        .contains_key("module:A"));
    assert_eq!(
        actual(&f)
            .iter()
            .filter(|e| e.kind == "hold.attention" && e.obj == f.hold)
            .count(),
        1
    );
    assert!(!f.project.schedule.lock().unwrap().can_attempt(1_800_000));
    let current = project_hint(&f.project, START + 1_800_000).unwrap();
    assert!(!f
        .project
        .ask_tick
        .lock()
        .unwrap()
        .admitted(&current, 1_800_000));
    assert!(
        AskTickSchedule::default().admitted(&current, 1_800_000),
        "restart reconstructs one coordination observation without a persistent cache"
    );
}

#[tokio::test]
async fn failed_ask_worker_retries_after_five_seconds_without_snapshot_retry_or_module_renewal() {
    let f = fixture();
    advance(&f, 90_000);
    *f.project.root.lock().unwrap() = f.root.join("missing-authority");
    tick(&f).await;
    assert_eq!(finished(&f), None);
    let loads = f.project.background_catalog_loads.load(Ordering::SeqCst);
    assert_eq!(loads, 1);
    advance(&f, 91_000);
    tick(&f).await;
    assert_eq!(
        f.project.background_catalog_loads.load(Ordering::SeqCst),
        loads
    );
    assert!(!f.project.schedule.lock().unwrap().can_attempt(95_000));
    *f.project.root.lock().unwrap() = f.root.clone();
    advance(&f, 95_000);
    tick(&f).await;
    assert_eq!(finished(&f), Some(f.deadline));
    assert_eq!(
        f.project.background_catalog_loads.load(Ordering::SeqCst),
        loads + 1
    );
    assert_eq!(
        f.project.hub.lock().unwrap().leases(START + 95_000)["module:A"].expires_at,
        f.original_expiry
    );
    advance(&f, 96_000);
    tick(&f).await;
    assert_eq!(
        f.project.background_catalog_loads.load(Ordering::SeqCst),
        loads + 1,
        "a successful finite observation does not poll Git every second"
    );
}

#[tokio::test]
async fn first_observed_actual_closed_slice_repairs_missing_confirmation_without_waiting_for_snapshot(
) {
    let mut f = fixture();
    advance(&f, 90_000);
    let mut outbox = f.server.task_outbox(&f.catalog).unwrap();
    ProjectSession::human_tick_shared(
        &f.project.hub,
        &f.catalog,
        &f.server.endpoint.host,
        &mut outbox,
        || Ok(START + 90_000),
    )
    .unwrap();
    assert_eq!(finished(&f), Some(f.deadline));
    let eid = towermd_core::recovery_contract::recorded::request_event_eid(
        &f.wait_id,
        "hold.wait_finished",
    )
    .unwrap();
    let original = actual(&f).into_iter().find(|e| e.eid == eid).unwrap();
    // Model a lost local confirmation while retaining its original pending
    // bytes and the actual Hub Event. Never forge a receipt from sequence.
    let directory = f
        .runtime
        .join("recovery-outbox")
        .join(f.catalog.config.project_id());
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let kept = text
            .lines()
            .filter(|line| {
                let value: Value = serde_json::from_str(line).unwrap();
                !(value["state"] == "confirmed" && value["original"]["eid"] == eid)
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path, format!("{kept}\n")).unwrap();
    }
    // The old Session is bound to the complete source digest before this
    // simulated disk loss. Release its Hub/server ownership and restore from
    // the actual new files instead of weakening exact-source rollback checks.
    drop(outbox);
    f.server.projects.lock().unwrap().clear();
    drop(f.project);
    drop(f.server);
    f.server = snapshot_server(&f.root, f.runtime.clone(), f.mono.clone(), f.unix.clone());
    f.catalog = Catalog::load(&f.root).unwrap();
    f.project = f.server.project(&f.catalog).unwrap();
    block_snapshot(&f.project);
    let before = f.server.recovery_inputs(&f.catalog).unwrap();
    assert!(has_request_hint(&before));
    assert!(before.pending().iter().any(|e| e.eid == eid));
    assert!(!before.confirmed().iter().any(|e| e.eid == eid));
    assert!(
        !project_hint(&f.project, START + 90_000).unwrap().due,
        "closed state needs coordination observation, not another deadline"
    );
    let cursor = f.project.hub.lock().unwrap().last_seq();
    tick(&f).await;
    let repaired = f.server.recovery_inputs(&f.catalog).unwrap();
    assert!(
        has_request_hint(&repaired),
        "complete confirmed rows can still need fsync repair"
    );
    assert_eq!(f.project.hub.lock().unwrap().last_seq(), cursor);
    assert!(!f.project.schedule.lock().unwrap().can_attempt(90_000));
    assert_eq!(
        repaired.confirmed().iter().find(|e| e.eid == eid),
        Some(&original)
    );
    assert_eq!(actual(&f).iter().filter(|e| e.eid == eid).count(), 1);
    assert_eq!(
        f.project.hub.lock().unwrap().leases(START + 90_000)["module:A"].expires_at,
        f.original_expiry
    );
}

fn second_actual_project(f: &Fixture) -> (Catalog, Arc<Project>, String) {
    let root = f._temp.path().join("second-project");
    fs::create_dir(&root).unwrap();
    snapshot_git(&root, &["init", "-qb", "main"]);
    towermd_core::init::init(&root, &towermd_core::init::InitOptions { docs_dir: None }).unwrap();
    let config = root.join(".towermd/config.yaml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(config, format!("{text}\nsnapshot_interval: 7200\n")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/a.rs"), "// second actual source\n").unwrap();
    fs::create_dir_all(root.join("docs/tower/arch")).unwrap();
    fs::create_dir_all(root.join("docs/tower/specs")).unwrap();
    fs::write(
        root.join("docs/tower/arch/A.md"),
        "---\ntype: Module\nid: A\npaths: ['src/**']\nverify: []\ninterfaces: []\n---\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/tower/specs/AC.md"),
        "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [second real Ask]\n---\n",
    )
    .unwrap();
    snapshot_git(&root, &["add", "."]);
    snapshot_git(
        &root,
        &["commit", "-qm", "second actual imported authority"],
    );
    snapshot_git(&root, &["switch", "-qc", "t/T"]);
    f.server
        .execute(Request {
            directory: root.clone(),
            run: None,
            command: TaskCommand::Add {
                task: TaskSpec {
                    id: "T".into(),
                    title: "second actual Ask".into(),
                    module: "A".into(),
                    touches: vec![],
                    allowed_paths: vec!["src/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC".into()],
                    decisions: vec![],
                },
            },
        })
        .unwrap();
    f.server
        .execute(Request {
            directory: root.clone(),
            run: None,
            command: TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
        })
        .unwrap();
    let catalog = Catalog::load(&root).unwrap();
    let project = f.server.project(&catalog).unwrap();
    let mut outbox = f.server.task_outbox(&catalog).unwrap();
    let opened = ProjectSession::ask_shared(
        &project.hub,
        &catalog,
        &f.server.endpoint.host,
        &Uuid::now_v7().to_string(),
        &HumanCommand::AskOpen {
            question: "second scheduling?".into(),
            options: vec![],
            recommended: None,
            context: String::new(),
        },
        None,
        &mut outbox,
        || Ok(START),
    )
    .unwrap();
    ProjectSession::ask_shared(
        &project.hub,
        &catalog,
        &f.server.endpoint.host,
        &Uuid::now_v7().to_string(),
        &HumanCommand::AskWait {
            hold: opened.hold.clone(),
        },
        None,
        &mut outbox,
        || Ok(START),
    )
    .unwrap();
    block_snapshot(&project);
    (catalog, project, opened.hold)
}

#[tokio::test]
async fn one_bad_project_hint_or_poisoned_schedule_cannot_abort_later_project_progress() {
    for poison_schedule in [false, true] {
        let f = fixture();
        let (other_catalog, other, other_hold) = second_actual_project(&f);
        // Server.projects follows BTreeMap project order. Put the fault first
        // without inventing IDs or bypassing either project's real producers.
        let (bad, good, good_hold) =
            if f.catalog.config.project_id() < other_catalog.config.project_id() {
                (f.project.clone(), other, other_hold)
            } else {
                (other, f.project.clone(), f.hold.clone())
            };
        let poisoned = bad.clone();
        let fault = std::thread::spawn(move || {
            if poison_schedule {
                let _guard = poisoned.ask_tick.lock().unwrap();
                panic!("test project Ask schedule poison");
            } else {
                let _guard = poisoned.hub.lock().unwrap();
                panic!("test project Session source unavailable");
            }
        })
        .join();
        assert!(fault.is_err());
        advance(&f, 90_000);
        // A known-unavailable first project's hint must never make this global
        // tick return before the second project's existing worker is admitted.
        f.server.tick().unwrap();
        f.server.drain_snapshots().await.unwrap();
        assert_eq!(
            good.hub.lock().unwrap().human().unwrap().holds[&good_hold].waits[0].finished_ms,
            Some(START + 90_000)
        );
        if poison_schedule {
            assert_eq!(
                bad.background_catalog_loads.load(Ordering::SeqCst),
                0,
                "poisoned scheduling state is diagnosed per project, not trusted"
            );
        } else {
            assert_eq!(
                bad.background_catalog_loads.load(Ordering::SeqCst),
                1,
                "unknown hint schedules full proof, not a direct append"
            );
            assert!(bad.ask_tick.lock().unwrap().retry_pending);
            advance(&f, 91_000);
            f.server.tick().unwrap();
            f.server.drain_snapshots().await.unwrap();
            assert_eq!(
                bad.background_catalog_loads.load(Ordering::SeqCst),
                1,
                "failed full proof uses the existing five-second Ask backoff"
            );
        }
    }
}

#[test]
fn actual_empty_project_inputs_do_not_request_an_ask_proof() {
    let (_temp, root) = snapshot_fixture(7200);
    let catalog = Catalog::load(&root).unwrap();
    let inputs =
        towermd_core::recovery_outbox::RecoveryInputs::read(&root.join("absent-outbox"), &catalog)
            .unwrap();
    assert!(inputs.pending().is_empty() && inputs.confirmed().is_empty());
    assert!(!has_request_hint(&inputs));
}
