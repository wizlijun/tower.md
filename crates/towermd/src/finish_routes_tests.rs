//! Actual adapter confirmation failure. No injected completion, report, receipt
//! or Job JSON can establish the positive result below.
use super::*;
use towermd_core::{
    baseline_review::review_baseline_shared, hub_protocol::*, recovery_contract::baseline::Verdict,
    snapshot_service::PublicationReport, tasks::TaskSpec,
};
#[allow(dead_code)]
#[path = "../tests/support/finish_fixture.rs"]
mod fixture;

#[derive(Clone)]
struct AcceptedCompletion {
    original: NewEvent,
    actual: Event,
    path: PathBuf,
    backup: PathBuf,
    pending_bytes: Vec<u8>,
}
type Capture = Arc<Mutex<Option<AcceptedCompletion>>>;
struct InterruptCompletedConfirmation {
    inner: Hub,
    kind: &'static str,
    directory: PathBuf,
    backup: PathBuf,
    capture: Capture,
}
impl HubProtocol for InterruptCompletedConfirmation {
    fn append(&mut self, original: NewEvent, now: u64) -> Result<AppendReceipt, HubError> {
        let receipt = self.inner.append(original.clone(), now)?;
        if original.kind == self.kind && self.capture.lock().unwrap().is_none() {
            let AppendReceipt::Event(actual) = &receipt else {
                panic!("first completion must be an actual durable Hub Event")
            };
            assert!(!actual.stale);
            let wanted = serde_json::to_value(&original).unwrap();
            let matches = fs::read_dir(&self.directory)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                .filter_map(|path| {
                    let bytes = fs::read(&path).unwrap();
                    let has_pending = std::str::from_utf8(&bytes).unwrap().lines().any(|line| {
                        let row: Value = serde_json::from_str(line).unwrap();
                        row["state"] == "pending" && row["original"] == wanted
                    });
                    has_pending.then_some((path, bytes))
                })
                .collect::<Vec<_>>();
            assert_eq!(
                matches.len(),
                1,
                "exact original must already have one fsynced pending host file"
            );
            let (path, pending_bytes) = matches.into_iter().next().unwrap();
            fs::rename(&path, &self.backup).unwrap();
            fs::create_dir(&path).unwrap();
            *self.capture.lock().unwrap() = Some(AcceptedCompletion {
                original,
                actual: actual.as_ref().clone(),
                path,
                backup: self.backup.clone(),
                pending_bytes,
            });
        }
        Ok(receipt)
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
struct RestorePending(Capture);
impl Drop for RestorePending {
    fn drop(&mut self) {
        if let Some(captured) = self.0.lock().unwrap().as_ref() {
            if captured.backup.exists() {
                if captured.path.is_dir() {
                    let _ = fs::remove_dir(&captured.path);
                }
                let _ = fs::rename(&captured.backup, &captured.path);
            }
        }
    }
}
async fn post(
    client: &reqwest::Client,
    server: &Server,
    path: &str,
    body: &impl Serialize,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .post(format!("http://127.0.0.1:{}{path}", server.endpoint.port))
        .header("x-towermd-instance", &server.endpoint.instance)
        .json(body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value = response.json().await.unwrap();
    (status, value)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finish_recording_confirmation_failure_repairs_original_through_actual_http_without_reexecution(
) {
    let mut f=fixture::configured_fixture(&[("docs/tower/specs/AC-recording.md","---\ntype: Acceptance\nid: AC-recording\nmodule: A-alpha\ncriteria: [actual recording confirmation]\n---\n")],&["count=\"$(git rev-parse --git-common-dir)/towermd/cache/finish-recording-count\"; printf x >> \"$count\"; exit 0"],false);
    let action = fixture::action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        fixture::now() + 3,
    )
    .unwrap();
    assert!(matches!(
        f.writer
            .publish_baseline(
                &f.session,
                &f.catalog,
                &mut f.outbox,
                &approval.receipt.result_ref.approval_eid,
                "host",
                || Ok(fixture::now() + 5)
            )
            .unwrap(),
        PublicationReport::Recorded { .. }
    ));
    fixture::git(f.root.path(), &["switch", "-qc", "t/T-recording", "main"]);
    let fixture::Fixture {
        root,
        runtime,
        session,
        outbox,
        ..
    } = f;
    drop(session);
    drop(outbox);
    let catalog = Catalog::load(root.path()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown, mut stop) = tokio::sync::watch::channel(false);
    let server = Arc::new(Server {
        finish_jobs: Arc::new(crate::finish_jobs::FinishJobs::default()),
        scan_jobs: crate::scan_jobs::ScanJobs::new(runtime.path()).unwrap(),
        source_status_cache: Default::default(),
        main_refresh: MainRefresh::default(),
        clock: Arc::new(now_ms),
        mono_clock: monotonic_clock(),
        endpoint: Endpoint {
            address: "127.0.0.1".into(),
            port,
            pid: std::process::id(),
            instance: Uuid::now_v7().to_string(),
            host: "host".into(),
        },
        runtime: runtime.path().to_owned(),
        projects: Mutex::new(BTreeMap::new()),
        registration: Mutex::new(()),
        shutdown,
    });
    let app = router(server.clone());
    let serving = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                while !*stop.borrow() {
                    if stop.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(100))
        .build()
        .unwrap();
    let command = |command| Request {
        directory: root.path().to_owned(),
        run: None,
        command,
    };
    let (code, added) = post(
        &client,
        &server,
        "/command",
        &command(TaskCommand::Add {
            task: TaskSpec {
                id: "T-recording".into(),
                title: "real recording fault".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-recording".into()],
                decisions: vec![],
            },
        }),
    )
    .await;
    assert_eq!(code, reqwest::StatusCode::OK, "{added}");
    let (code, claimed) = post(
        &client,
        &server,
        "/command",
        &command(TaskCommand::Claim {
            id: "T-recording".into(),
            harness: None,
            harness_version: None,
        }),
    )
    .await;
    assert_eq!(code, reqwest::StatusCode::OK, "{claimed}");
    let run_id = claimed["id"].as_str().unwrap().to_owned();
    let project = server.project(&catalog).unwrap();
    let inputs = server.recovery_inputs(&catalog).unwrap();
    let capture: Capture = Arc::new(Mutex::new(None));
    let _restore = RestorePending(capture.clone());
    // The placeholder only moves out and drops the former owned protocol while
    // the same registered mutex is held. It is never visible to a route, job,
    // source proof or positive assertion. A separate actual empty Git fixture
    // avoids making an empty Hub claim the original B0 snapshot floor.
    let (_placeholder_dir, placeholder_root) = super::super::tests::snapshot_fixture(600);
    let placeholder_catalog = Catalog::load(&placeholder_root).unwrap();
    let placeholder_inputs = towermd_core::recovery_outbox::RecoveryInputs::read(
        &_placeholder_dir.path().join("placeholder-inputs"),
        &placeholder_catalog,
    )
    .unwrap();
    let placeholder = ProjectSession::new(
        Box::new(Hub::open(&_placeholder_dir.path().join("placeholder-hub")).unwrap()),
        &placeholder_catalog,
        &placeholder_inputs,
        now_ms().unwrap(),
    )
    .unwrap();
    {
        let mut live = project.hub.lock().unwrap();
        let previous = std::mem::replace(&mut *live, placeholder);
        drop(previous);
        let actual = InterruptCompletedConfirmation {
            inner: Hub::open(&catalog.common_dir.join("towermd")).unwrap(),
            kind: "finish.completed",
            directory: runtime
                .path()
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            backup: runtime.path().join("original-recording-pending.saved"),
            capture: capture.clone(),
        };
        *live =
            ProjectSession::new(Box::new(actual), &catalog, &inputs, now_ms().unwrap()).unwrap();
    }
    let before_cursor = project
        .hub
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq();
    let count = catalog
        .common_dir
        .join("towermd/cache/finish-recording-count");
    assert!(!count.exists());
    let request = FinishRequest {
        directory: root.path().to_owned(),
        run: Some(run_id.clone()),
        wait: None,
        report: None,
    };
    let (code, queued) = post(&client, &server, "/finish", &request).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{queued}");
    let id = queued["evidence_id"].as_str().unwrap().to_owned();
    assert_eq!(queued["job_state"], "queued");
    assert!(queued.get("evidence").is_none());
    let uncertain = server
        .finish_jobs
        .wait(
            catalog.config.project_id(),
            &catalog.common_dir,
            &id,
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    assert_eq!(uncertain["job_state"], "uncertain");
    assert!(
        uncertain["diagnostic"]
            .as_str()
            .unwrap()
            .contains("uncertain"),
        "{uncertain}"
    );
    assert!(uncertain.get("evidence").is_none());
    let accepted = capture
        .lock()
        .unwrap()
        .clone()
        .expect("actual Hub accepted completed before the real confirmation IO failure");
    assert_eq!(accepted.original.kind, "finish.completed");
    assert_eq!(accepted.original.eid, accepted.actual.eid);
    assert_eq!(accepted.original.data, accepted.actual.data);
    assert_eq!(accepted.original.guards, accepted.actual.guards);
    assert_eq!(accepted.original.host, accepted.actual.host);
    assert_eq!(accepted.original.actor, accepted.actual.actor);
    assert_eq!(accepted.original.layer, accepted.actual.layer);
    assert_eq!(accepted.original.obj, accepted.actual.obj);
    assert_eq!(accepted.original.kind, accepted.actual.kind);
    assert!(!accepted.actual.stale);
    assert_eq!(accepted.actual.data["run"]["id"], run_id);
    assert_eq!(
        accepted.actual.data["run"]["finish"]["jobs"][&id]["phase"],
        "recording"
    );
    assert_eq!(accepted.actual.data["evidence"]["result"], "passed");
    assert_eq!(
        accepted.actual.data["run"]["finish"]["jobs"][&id]["input"],
        queued["input"]
    );
    assert_eq!(
        serde_json::to_value(&accepted.original.guards).unwrap(),
        queued["input"]["fences"]
    );
    assert!(accepted.path.is_dir());
    assert_eq!(fs::read(&accepted.backup).unwrap(), accepted.pending_bytes);
    assert!(server.recovery_inputs(&catalog).is_err());
    assert_eq!(fs::read(&count).unwrap(), b"x");
    let fault_batch = project
        .hub
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before_cursor, now_ms().unwrap())
        .unwrap();
    assert_eq!(
        fault_batch
            .events
            .iter()
            .filter(|event| event.kind == "finish.completed")
            .count(),
        1
    );
    assert_eq!(
        fault_batch
            .events
            .iter()
            .find(|event| event.kind == "finish.completed"),
        Some(&accepted.actual)
    );
    assert!(!fault_batch
        .events
        .iter()
        .any(|event| event.kind == "lease.granted"));
    let running = fault_batch
        .events
        .iter()
        .find(|event| event.kind == "finish.running")
        .unwrap();
    assert_eq!(
        project.hub.lock().unwrap().projection().unwrap().objects["Run"][&run_id],
        running.data["run"],
        "physical source failure must retain the last verified Running view"
    );
    fs::remove_dir(&accepted.path).unwrap();
    fs::rename(&accepted.backup, &accepted.path).unwrap();
    assert_eq!(fs::read(&accepted.path).unwrap(), accepted.pending_bytes);
    let pending = server.recovery_inputs(&catalog).unwrap();
    assert_eq!(
        pending
            .pending()
            .iter()
            .filter(|event| event.eid == accepted.original.eid)
            .collect::<Vec<_>>(),
        vec![&accepted.original]
    );
    assert!(!pending
        .confirmed()
        .iter()
        .any(|event| event.eid == accepted.original.eid));
    let wait = FinishRequest {
        directory: root.path().to_owned(),
        run: Some(run_id.clone()),
        wait: Some(id.clone()),
        report: None,
    };
    let (code, done) = post(&client, &server, "/finish", &wait).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{done}");
    assert_eq!(done["job_state"], "completed");
    assert_eq!(done["evidence_id"], id);
    assert_eq!(
        done["actual"],
        serde_json::to_value(&accepted.actual).unwrap()
    );
    assert_eq!(done["evidence"], accepted.actual.data["evidence"]);
    assert_eq!(done["report"], accepted.actual.data["report"]);
    assert_eq!(done["run_state"], "verifying");
    let confirmed = server.recovery_inputs(&catalog).unwrap();
    assert!(!confirmed
        .pending()
        .iter()
        .any(|event| event.eid == accepted.original.eid));
    assert_eq!(
        confirmed
            .confirmed()
            .iter()
            .filter(|event| event.eid == accepted.original.eid)
            .collect::<Vec<_>>(),
        vec![&accepted.actual]
    );
    let after = project
        .hub
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before_cursor, now_ms().unwrap())
        .unwrap();
    assert_eq!(
        after, fault_batch,
        "confirmation repair must not add another completion, grant, or any actual Hub event"
    );
    let (code, repeated) = post(&client, &server, "/finish", &wait).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{repeated}");
    assert_eq!(repeated, done);
    assert_eq!(fs::read(&count).unwrap(), b"x");
    let physical = fs::read_to_string(&accepted.path).unwrap();
    let records = physical
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .filter(
                |row| row["state"] == "confirmed" && row["actual"]["eid"] == accepted.original.eid
            )
            .count(),
        1,
        "only the exact original completion is confirmed once"
    );
    assert_eq!(
        records
            .iter()
            .filter(
                |row| row["state"] == "pending" && row["original"]["eid"] == accepted.original.eid
            )
            .count(),
        1,
        "the exact initial pending intent is retained once"
    );
    for row in records
        .iter()
        .filter(|row| row["original"]["eid"] == accepted.original.eid)
    {
        assert_eq!(
            row["original"],
            serde_json::to_value(&accepted.original).unwrap()
        );
    }
    server.shutdown.send_replace(true);
    server.finish_jobs.shutdown().await.unwrap();
    server.scan_jobs.shutdown().await.unwrap();
    server.main_refresh.drain().await.unwrap();
    server.drain_snapshots().await.unwrap();
    serving.await.unwrap();
}

#[test]
fn finish_background_retries_original_after_source_failure_without_another_cli_request() {
    let mut f=fixture::configured_fixture(&[("docs/tower/specs/AC-background.md","---\ntype: Acceptance\nid: AC-background\nmodule: A-alpha\ncriteria: [actual background recovery]\n---\n")],&["printf x >> \"$(git rev-parse --git-common-dir)/towermd/cache/finish-background-count\""],false);
    let action = fixture::action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        fixture::now() + 3,
    )
    .unwrap();
    f.writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(fixture::now() + 5),
        )
        .unwrap();
    fixture::git(f.root.path(), &["switch", "-qc", "t/T-background", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    for command in [
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-background".into(),
                title: "real background recovery".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-background".into()],
                decisions: vec![],
            },
        },
        TaskCommand::Claim {
            id: "T-background".into(),
            harness: None,
            harness_version: None,
        },
    ] {
        ProjectSession::execute_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            "host",
            command,
            None,
            fixture::now() + 10,
            &mut f.outbox,
        )
        .unwrap();
    }
    let original_id = Uuid::now_v7().to_string();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &original_id,
        None,
        &mut f.outbox,
        || Ok(fixture::now() + 20),
    )
    .unwrap() else {
        panic!("actual job must start queued")
    };
    let status = ticket.status().clone();
    drop(ticket);
    let fixture::Fixture {
        root,
        runtime,
        session,
        outbox,
        ..
    } = f;
    drop(session);
    drop(outbox);
    let mono = Arc::new(AtomicU64::new(1_000));
    let unix = Arc::new(AtomicU64::new(fixture::now() + 30));
    let mut server = super::super::tests::snapshot_server(
        root.path(),
        runtime.path().to_owned(),
        mono.clone(),
        unix,
    );
    Arc::get_mut(&mut server).unwrap().endpoint.host = "host".into();
    let catalog = Catalog::load(root.path()).unwrap();
    // Outside a Tokio runtime registration installs the true source but cannot
    // launch a worker. No test-only status or execution capability is injected.
    let project = server.project(&catalog).unwrap();
    let directory = runtime
        .path()
        .join("recovery-outbox")
        .join(catalog.config.project_id());
    let path = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    let original_bytes = fs::read(&path).unwrap();
    let backup = path.with_extension("saved");
    fs::rename(&path, &backup).unwrap();
    fs::create_dir(&path).unwrap();
    let count = catalog
        .common_dir
        .join("towermd/cache/finish-background-count");
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        server.schedule_finish_recovery(project.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while project.finish_recovery_active.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!project.finish_recovery_active.load(Ordering::Acquire));
        assert!(project.finish_recovery.load(Ordering::Acquire));
        assert_eq!(project.finish_retry_after.load(Ordering::Acquire), 6_000);
        assert!(!count.exists(), "source failure cannot authorize execution");
        fs::remove_dir(&path).unwrap();
        fs::rename(&backup, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original_bytes);
        mono.store(5_999, Ordering::Release);
        server.schedule_finish_recovery(project.clone()).unwrap();
        assert!(!project.finish_recovery_active.load(Ordering::Acquire));
        mono.store(6_000, Ordering::Release);
        server.schedule_finish_recovery(project.clone()).unwrap();
        // Repeated due observations must not submit a second recovery flight.
        server.schedule_finish_recovery(project.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let completed = loop {
            if let Ok(inputs) = server.recovery_inputs(&catalog) {
                if let Some(actual) = inputs
                    .confirmed()
                    .iter()
                    .find(|event| event.eid == status.completed_eid)
                {
                    break actual.clone();
                }
            }
            assert!(
                Instant::now() < deadline,
                "background did not durably record the original completion"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(
            completed.data["run"]["finish"]["jobs"][&status.input.evidence_id]["input"],
            json!(status.input)
        );
        assert_eq!(completed.guards, status.input.fences);
        assert_eq!(completed.data["run"]["state"], "verifying");
        assert_eq!(fs::read(&count).unwrap(), b"x");
        let inputs = server.recovery_inputs(&catalog).unwrap();
        let original_start = inputs
            .confirmed()
            .iter()
            .filter(|event| event.eid == status.started_eid)
            .collect::<Vec<_>>();
        assert_eq!(original_start.len(), 1);
        assert_eq!(original_start[0].data["input"], json!(status.input));
        assert_eq!(
            inputs
                .confirmed()
                .iter()
                .filter(|event| event.eid == status.completed_eid)
                .count(),
            1
        );
        server.shutdown.send_replace(true);
        server.finish_jobs.shutdown().await.unwrap();
        server.scan_jobs.shutdown().await.unwrap();
        server.main_refresh.drain().await.unwrap();
        server.drain_snapshots().await.unwrap();
    });
}

fn queued_owned_fixture(commands: &[&str]) -> (fixture::Fixture, FinishTicket) {
    let mut f=fixture::configured_fixture(&[("docs/tower/specs/AC-owned.md","---\ntype: Acceptance\nid: AC-owned\nmodule: A-alpha\ncriteria: [actual owned execution]\n---\n")],commands,false);
    let action = fixture::action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        fixture::now() + 3,
    )
    .unwrap();
    f.writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(fixture::now() + 5),
        )
        .unwrap();
    fixture::git(f.root.path(), &["switch", "-qc", "t/T-owned", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    for command in [
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-owned".into(),
                title: "actual owned execution".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-owned".into()],
                decisions: vec![],
            },
        },
        TaskCommand::Claim {
            id: "T-owned".into(),
            harness: None,
            harness_version: None,
        },
    ] {
        ProjectSession::execute_shared(
            &f.session,
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            "host",
            command,
            None,
            fixture::now() + 10,
            &mut f.outbox,
        )
        .unwrap();
    }
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &Uuid::now_v7().to_string(),
        None,
        &mut f.outbox,
        || Ok(fixture::now() + 20),
    )
    .unwrap() else {
        panic!("actual job must start queued")
    };
    (f, ticket)
}

#[cfg(unix)]
#[test]
fn retained_infrastructure_error_repairs_exact_interrupted_confirmation_and_clears_cache() {
    let (mut f, ticket) = queued_owned_fixture(&["exit 0"]);
    let status = ticket.status().clone();
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(fixture::now() + 21)
        })
        .unwrap();
    // A real owned verifier-path fault yields an infrastructure Err, independent
    // of cancellation semantics. It must still retain/repair the same interrupt.
    let verify = f.catalog.common_dir.join("towermd/verify");
    fs::remove_dir(&verify).unwrap();
    let foreign = f.runtime.path().join("foreign-verifier-root");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("keep"), "owned elsewhere").unwrap();
    std::os::unix::fs::symlink(&foreign, &verify).unwrap();
    let executed = finish::execute(
        execution,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let capture: Capture = Arc::new(Mutex::new(None));
    let _restore = RestorePending(capture.clone());
    drop(f.session);
    f.session = Arc::new(Mutex::new(
        ProjectSession::new(
            Box::new(InterruptCompletedConfirmation {
                inner: Hub::open(&f.catalog.common_dir.join("towermd")).unwrap(),
                kind: "finish.interrupted",
                directory: f
                    .runtime
                    .path()
                    .join("recovery-outbox")
                    .join(f.catalog.config.project_id()),
                backup: f.runtime.path().join("interrupted-pending.saved"),
                capture: capture.clone(),
            }),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            fixture::now() + 22,
        )
        .unwrap(),
    ));
    let jobs = crate::finish_jobs::FinishJobs::default();
    let project_id = f.catalog.config.project_id();
    let id = &status.input.evidence_id;
    jobs.attach(
        project_id,
        &f.catalog.common_dir,
        id,
        json!({"job_state":"running"}),
    )
    .unwrap();
    jobs.retain_executed(project_id, &f.catalog.common_dir, id, executed)
        .unwrap();
    let first = jobs
        .record_executed(project_id, &f.catalog.common_dir, id, |executed| {
            outcome(ProjectSession::record_borrowed_finish_shared(
                &f.session,
                &f.catalog,
                executed,
                &mut f.outbox,
                || Ok(fixture::now() + 23),
            )?)
        })
        .unwrap_err();
    assert!(
        first.contains("uncertain") && first.contains(&status.request_id),
        "{first}"
    );
    assert!(jobs
        .has_executed(project_id, &f.catalog.common_dir, id)
        .unwrap());
    let accepted = capture
        .lock()
        .unwrap()
        .clone()
        .expect("actual Interrupted must precede failed confirmation");
    assert_eq!(accepted.actual.kind, "finish.interrupted");
    assert_eq!(accepted.actual.data["run"]["finish"]["failures"], 0);
    fs::remove_dir(&accepted.path).unwrap();
    fs::rename(&accepted.backup, &accepted.path).unwrap();
    assert_eq!(fs::read(&accepted.path).unwrap(), accepted.pending_bytes);
    let done = jobs
        .record_executed(project_id, &f.catalog.common_dir, id, |executed| {
            outcome(ProjectSession::record_borrowed_finish_shared(
                &f.session,
                &f.catalog,
                executed,
                &mut f.outbox,
                || Ok(fixture::now() + 24),
            )?)
        })
        .unwrap()
        .expect("original interrupted receipt must settle retained output");
    assert_eq!(done["job_state"], "interrupted");
    assert_eq!(done["actual"], json!(accepted.actual));
    assert!(done.get("evidence").is_none());
    assert!(!jobs
        .has_executed(project_id, &f.catalog.common_dir, id)
        .unwrap());
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.eid == accepted.original.eid)
            .count(),
        1
    );
    assert!(!inputs
        .pending()
        .iter()
        .any(|event| event.eid == accepted.original.eid));
    assert!(!inputs
        .confirmed()
        .iter()
        .any(|event| event.eid == status.completed_eid));
    let live = f.session.lock().unwrap();
    let run = &live.projection().unwrap().objects["Run"][&status.input.run_id];
    assert_eq!(run["finish"]["failures"], 0);
    assert_eq!(run["finish"]["jobs"][id]["input"], json!(status.input));
}

#[derive(Default)]
struct FinishBoundary {
    state: Mutex<(bool, bool)>,
    changed: std::sync::Condvar,
}
impl FinishBoundary {
    fn enter(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
    }
    fn wait_entered(&self) {
        let (state, _) = self
            .changed
            .wait_timeout_while(
                self.state.lock().unwrap(),
                Duration::from_secs(15),
                |state| !state.0,
            )
            .unwrap();
        let entered = state.0;
        drop(state);
        assert!(
            entered,
            "actual worker did not reach its precise ownership boundary"
        );
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}
type BoundaryMap = BTreeMap<(PathBuf, &'static str), Arc<FinishBoundary>>;
fn finish_boundaries() -> &'static Mutex<BoundaryMap> {
    static BOUNDARIES: std::sync::OnceLock<Mutex<BoundaryMap>> = std::sync::OnceLock::new();
    BOUNDARIES.get_or_init(|| Mutex::new(BTreeMap::new()))
}
struct FinishBoundaryGuard {
    common: PathBuf,
    point: &'static str,
    barrier: Arc<FinishBoundary>,
}
impl FinishBoundaryGuard {
    fn install(common: &std::path::Path, point: &'static str) -> Self {
        let barrier = Arc::new(FinishBoundary::default());
        assert!(finish_boundaries()
            .lock()
            .unwrap()
            .insert((common.to_owned(), point), barrier.clone())
            .is_none());
        Self {
            common: common.to_owned(),
            point,
            barrier,
        }
    }
}
impl Drop for FinishBoundaryGuard {
    fn drop(&mut self) {
        self.barrier.release();
        finish_boundaries()
            .lock()
            .unwrap()
            .remove(&(self.common.clone(), self.point));
    }
}
pub(super) fn finish_boundary(common: &std::path::Path, point: &'static str) {
    let barrier = finish_boundaries()
        .lock()
        .unwrap()
        .get(&(common.to_owned(), point))
        .cloned();
    if let Some(barrier) = barrier {
        barrier.enter();
    }
}

#[test]
fn background_rechecks_actual_owner_after_waiting_for_producer_before_consuming_output() {
    let (f, original_ticket) = queued_owned_fixture(&[
        "printf x >> \"$(git rev-parse --git-common-dir)/towermd/cache/finish-owner-count\"",
    ]);
    let status = original_ticket.status().clone();
    drop(original_ticket);
    let fixture::Fixture {
        root,
        runtime,
        session,
        outbox,
        ..
    } = f;
    drop(session);
    drop(outbox);
    let mono = Arc::new(AtomicU64::new(1_000));
    let unix = Arc::new(AtomicU64::new(fixture::now() + 30));
    let mut server =
        super::super::tests::snapshot_server(root.path(), runtime.path().to_owned(), mono, unix);
    Arc::get_mut(&mut server).unwrap().endpoint.host = "host".into();
    let catalog = Arc::new(Catalog::load(root.path()).unwrap());
    // This actual reopen has no runtime yet, hence no worker can be scheduled
    // until the two precise ownership barriers are installed below.
    let project = server.project(&catalog).unwrap();
    let mut outbox = server.task_outbox(&catalog).unwrap();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &project.hub,
        &catalog,
        "host",
        &status.request_id,
        Some(&status.input.run_id),
        &mut outbox,
        || Ok(fixture::now() + 30),
    )
    .unwrap() else {
        panic!("original actual job remains queued")
    };
    drop(outbox);
    let rt = tokio::runtime::Runtime::new().unwrap();
    // These guards drop before the runtime even on assertion failure, so a
    // diagnostic cannot strand a blocked owned worker during runtime cleanup.
    let recovery = FinishBoundaryGuard::install(&catalog.common_dir, "recovery_before_producer");
    let owner = FinishBoundaryGuard::install(&catalog.common_dir, "executed_retained");
    rt.block_on(async {
        server.schedule_finish_recovery(project.clone()).unwrap();
        recovery.barrier.wait_entered();
        assert!(!server
            .finish_jobs
            .active(
                catalog.config.project_id(),
                &catalog.common_dir,
                &status.input.evidence_id
            )
            .unwrap());
        server
            .schedule_finish(project.clone(), catalog.clone(), ticket)
            .unwrap();
        owner.barrier.wait_entered();
        assert!(server
            .finish_jobs
            .active(
                catalog.config.project_id(),
                &catalog.common_dir,
                &status.input.evidence_id
            )
            .unwrap());
        assert!(server
            .finish_jobs
            .has_executed(
                catalog.config.project_id(),
                &catalog.common_dir,
                &status.input.evidence_id
            )
            .unwrap());
        recovery.barrier.release();
        let deadline = Instant::now() + Duration::from_secs(15);
        while project.finish_recovery_active.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!project.finish_recovery_active.load(Ordering::Acquire));
        assert!(
            server
                .finish_jobs
                .has_executed(
                    catalog.config.project_id(),
                    &catalog.common_dir,
                    &status.input.evidence_id
                )
                .unwrap(),
            "background cannot consume an actual active owner's output"
        );
        owner.barrier.release();
        let done = server
            .finish_jobs
            .wait(
                catalog.config.project_id(),
                &catalog.common_dir,
                &status.input.evidence_id,
                Duration::from_secs(30),
            )
            .await
            .unwrap();
        assert_eq!(done["job_state"], "completed", "{done}");
        assert_eq!(done["evidence_id"], status.input.evidence_id);
        assert_eq!(done["input"], json!(status.input));
        assert_eq!(done["run_state"], "verifying");
        assert!(!server
            .finish_jobs
            .has_executed(
                catalog.config.project_id(),
                &catalog.common_dir,
                &status.input.evidence_id
            )
            .unwrap());
        assert_eq!(
            fs::read(catalog.common_dir.join("towermd/cache/finish-owner-count")).unwrap(),
            b"x"
        );
        let inputs = server.recovery_inputs(&catalog).unwrap();
        assert_eq!(
            inputs
                .confirmed()
                .iter()
                .filter(|event| event.eid == status.completed_eid)
                .count(),
            1
        );
        server.shutdown.send_replace(true);
        server.finish_jobs.shutdown().await.unwrap();
        server.scan_jobs.shutdown().await.unwrap();
        server.main_refresh.drain().await.unwrap();
        server.drain_snapshots().await.unwrap();
    });
}

// These adapters use the actual B0/claim/started producer above. Fake clocks
// advance together; all child execution and durable receipts remain real.
struct MaintenanceFixture {
    _root: tempfile::TempDir,
    _runtime: tempfile::TempDir,
    server: Arc<Server>,
    project: Arc<Project>,
    catalog: Arc<Catalog>,
    status: FinishStatus,
    ticket: Option<FinishTicket>,
    mono: Arc<AtomicU64>,
    unix: Arc<AtomicU64>,
}
struct MaintenanceCancellation(Arc<Server>);
impl Drop for MaintenanceCancellation {
    fn drop(&mut self) {
        self.0.shutdown.send_replace(true);
        let _ = self.0.finish_jobs.cancel_all();
    }
}
impl Drop for MaintenanceFixture {
    fn drop(&mut self) {
        self.server.shutdown.send_replace(true);
        let _ = self.server.finish_jobs.cancel_all();
    }
}
fn maintenance_fixture(commands: &[&str], elapsed: u64) -> MaintenanceFixture {
    let (f, initial) = queued_owned_fixture(commands);
    let status = initial.status().clone();
    drop(initial);
    let fixture::Fixture {
        root,
        runtime,
        session,
        outbox,
        ..
    } = f;
    drop(session);
    drop(outbox);
    let mono = Arc::new(AtomicU64::new(elapsed));
    let unix = Arc::new(AtomicU64::new(status.input.entered_verifying_at + elapsed));
    let mut server = super::super::tests::snapshot_server(
        root.path(),
        runtime.path().to_owned(),
        mono.clone(),
        unix.clone(),
    );
    Arc::get_mut(&mut server).unwrap().endpoint.host = "host".into();
    let catalog = Arc::new(Catalog::load(root.path()).unwrap());
    // No runtime yet: registration cannot dispatch before assertions are ready.
    let project = server.project(&catalog).unwrap();
    let ticket = if elapsed < 60 * 60_000 {
        let mut outbox = server.task_outbox(&catalog).unwrap();
        match ProjectSession::begin_finish_shared(
            &project.hub,
            &catalog,
            "host",
            &status.request_id,
            Some(&status.input.run_id),
            &mut outbox,
            || Ok(unix.load(Ordering::Acquire)),
        )
        .unwrap()
        {
            FinishStart::Queued(ticket) => Some(ticket),
            _ => panic!("original actual queued request must remain recoverable"),
        }
    } else {
        None
    };
    MaintenanceFixture {
        _root: root,
        _runtime: runtime,
        server,
        project,
        catalog,
        status,
        ticket,
        mono,
        unix,
    }
}
async fn maintenance_settled(f: &MaintenanceFixture) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if !f.project.finish_recovery_active.load(Ordering::Acquire) {
            if !f.project.finish_recovery.load(Ordering::Acquire) {
                break;
            }
            // A just-drained worker may have reported its post-deadline record
            // refusal. The same real ticker scheduler coordinates that signal.
            f.server
                .schedule_finish_recovery(f.project.clone())
                .unwrap();
        }
        assert!(Instant::now() < deadline, "finish maintenance did not drain/coordinate within the existing worker observation budget; retry_at={} mono={}", f.project.finish_retry_after.load(Ordering::Acquire), f.mono.load(Ordering::Acquire));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
fn advance_maintenance(f: &MaintenanceFixture, elapsed: u64) {
    f.mono.store(elapsed, Ordering::Release);
    f.unix.store(
        f.status.input.entered_verifying_at + elapsed,
        Ordering::Release,
    );
    // This is the same actual system-fence phase which precedes business
    // scheduling in Server::tick, without unrelated snapshot publication.
    snapshot_service::maintain_main(
        &mut f.project.hub.lock().unwrap(),
        "host",
        f.unix.load(Ordering::Acquire),
    )
    .unwrap();
    f.server
        .schedule_finish_recovery(f.project.clone())
        .unwrap();
}
async fn stop_maintenance_fixture(f: &MaintenanceFixture) {
    f.server.shutdown.send_replace(true);
    f.server.finish_jobs.shutdown().await.unwrap();
    f.server.scan_jobs.shutdown().await.unwrap();
    f.server.main_refresh.drain().await.unwrap();
    f.server.drain_snapshots().await.unwrap();
}
#[test]
fn owned_verifier_renews_original_fences_at_14_and_59_minutes_then_drains_at_fixed_60_and_cannot_touch_new_run(
) {
    let mut f = maintenance_fixture(
        &[
            r#"printf x >> "$(git rev-parse --git-common-dir)/towermd/cache/finish-maintenance-count"; while :; do sleep 1; done"#,
        ],
        30,
    );
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _cancel_before_runtime_drop = MaintenanceCancellation(f.server.clone());
    rt.block_on(async {
        f.server
            .schedule_finish(
                f.project.clone(),
                f.catalog.clone(),
                f.ticket.take().unwrap(),
            )
            .unwrap();
        let count = f
            .catalog
            .common_dir
            .join("towermd/cache/finish-maintenance-count");
        let ready = Instant::now() + Duration::from_secs(15);
        while !count.exists() {
            assert!(
                Instant::now() < ready,
                "actual owned verifier never started"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let original = f.status.input.clone();
        assert_eq!(
            original.merge_deadline,
            original.entered_verifying_at + 60 * 60_000
        );
        for minute in [14, 28, 42, 56, 59] {
            advance_maintenance(&f, minute * 60_000);
            maintenance_settled(&f).await;
            let now = f.unix.load(Ordering::Acquire);
            let live = f.project.hub.lock().unwrap();
            let leases = live.leases(now);
            for fence in &original.fences {
                assert_eq!(
                    leases[&fence.name].fence(),
                    *fence,
                    "renewal must retain the original epoch"
                );
                assert_eq!(leases[&fence.name].expires_at, now + 900_000);
            }
            let projection = live.projection().unwrap();
            let run = &projection.objects["Run"][&original.run_id];
            assert_eq!(run["finish"]["merge_deadline"], original.merge_deadline);
            assert_eq!(
                run["finish"]["jobs"][&original.evidence_id]["input"],
                json!(original)
            );
            assert_eq!(run["state"], "verifying");
            drop(live);
            assert!(f
                .server
                .finish_jobs
                .active(
                    &f.project.project_id,
                    &f.catalog.common_dir,
                    &original.evidence_id
                )
                .unwrap());
        }
        // The minute scheduler is independent of new command/--wait activity.
        advance_maintenance(&f, 60 * 60_000);
        maintenance_settled(&f).await;
        assert!(!f
            .server
            .finish_jobs
            .active(
                &f.project.project_id,
                &f.catalog.common_dir,
                &original.evidence_id
            )
            .unwrap());
        assert!(!f
            .server
            .finish_jobs
            .has_executed(
                &f.project.project_id,
                &f.catalog.common_dir,
                &original.evidence_id
            )
            .unwrap());
        let inputs = f.server.recovery_inputs(&f.catalog).unwrap();
        let expired = inputs
            .confirmed()
            .iter()
            .filter(|event| {
                event.kind == "task.released"
                    && event.data["run"]["id"] == original.run_id
                    && event.data["run"]["state"] == "expired"
            })
            .collect::<Vec<_>>();
        assert_eq!(expired.len(), 1);
        assert!(!expired[0].stale);
        assert_eq!(expired[0].data["task"]["state"], "ready");
        assert_eq!(
            expired[0].data["run"]["finish"]["merge_deadline"],
            original.merge_deadline
        );
        assert_eq!(
            expired[0].data["run"]["finish"]["jobs"][&original.evidence_id]["input"],
            json!(original)
        );
        assert!(!inputs
            .confirmed()
            .iter()
            .any(|event| event.eid == f.status.completed_eid
                || (event.kind == "finish.interrupted"
                    && event.data["run"]["id"] == original.run_id)));
        assert_eq!(fs::read(&count).unwrap(), b"x");
        {
            let live = f.project.hub.lock().unwrap();
            assert_eq!(
                live.projection().unwrap().objects["Run"][&original.run_id]["state"],
                "expired"
            );
            for fence in &original.fences {
                assert!(!live
                    .leases(f.unix.load(Ordering::Acquire))
                    .contains_key(&fence.name));
            }
        }
        let drained_view = f
            .server
            .finish_jobs
            .wait(
                &f.project.project_id,
                &f.catalog.common_dir,
                &original.evidence_id,
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_ne!(
            drained_view["cleanup_unknown"], true,
            "this daemon's actual owned cancellation established cleanup"
        );
        // A fresh real claim receives new epochs. Original finish retry is
        // history only and cannot mutate that new Run or restart execution.
        let fresh = Catalog::load(&f.catalog.root).unwrap();
        {
            let _producer = f.project.producer.lock().unwrap();
            let mut outbox = f.server.task_outbox(&fresh).unwrap();
            let inputs = outbox.read_inputs().unwrap();
            f.unix.fetch_add(1, Ordering::AcqRel);
            f.mono.fetch_add(1, Ordering::AcqRel);
            ProjectSession::execute_shared(
                &f.project.hub,
                &fresh,
                &inputs,
                "host",
                TaskCommand::Claim {
                    id: "T-owned".into(),
                    harness: None,
                    harness_version: None,
                },
                None,
                f.unix.load(Ordering::Acquire),
                &mut outbox,
            )
            .unwrap();
        }
        let before = f.project.hub.lock().unwrap().projection().unwrap().clone();
        let new_run = before.objects["Run"]
            .iter()
            .find(|(id, raw)| {
                *id != &original.run_id && raw["task"] == "T-owned" && raw["state"] == "running"
            })
            .unwrap();
        let new_fences: Vec<Fence> = serde_json::from_value(new_run.1["leases"].clone()).unwrap();
        for prior in &original.fences {
            assert!(new_fences
                .iter()
                .any(|next| next.name == prior.name && next.epoch > prior.epoch));
        }
        let (_, _, _, historical) = f
            .server
            .prepare_finish(FinishRequest {
                directory: fresh.root.clone(),
                run: Some(original.run_id.clone()),
                wait: Some(original.evidence_id.clone()),
                report: None,
            })
            .unwrap();
        assert_eq!(historical["run_state"], "expired");
        assert_eq!(
            f.project.hub.lock().unwrap().projection().unwrap().objects,
            before.objects
        );
        assert_eq!(fs::read(&count).unwrap(), b"x");
        stop_maintenance_fixture(&f).await;
    });
}
#[test]
fn registered_queued_job_at_deadline_expires_without_execution_and_derived_jobs_rebuild_from_actual_receipt(
) {
    let f = maintenance_fixture(
        &[
            r#"printf x >> "$(git rev-parse --git-common-dir)/towermd/cache/finish-expired-restart-count""#,
        ],
        60 * 60_000,
    );
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _cancel_before_runtime_drop = MaintenanceCancellation(f.server.clone());
    rt.block_on(async {
        f.server
            .schedule_finish_recovery(f.project.clone())
            .unwrap();
        maintenance_settled(&f).await;
        let id = &f.status.input.evidence_id;
        assert!(!f
            .catalog
            .common_dir
            .join("towermd/cache/finish-expired-restart-count")
            .exists());
        let cached = f
            .server
            .finish_jobs
            .wait(
                &f.project.project_id,
                &f.catalog.common_dir,
                id,
                Duration::from_secs(15),
            )
            .await
            .unwrap();
        assert_eq!(cached["run_state"], "expired");
        let actual = f
            .server
            .recovery_inputs(&f.catalog)
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| {
                event.kind == "task.released" && event.data["run"]["id"] == f.status.input.run_id
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].data["run"]["state"], "expired");
        assert_eq!(cached["expiry_actual"], json!(actual[0]));
        assert!(cached["actual"].is_null());
        assert!(cached.get("evidence").is_none());
        // Delete only derived Job state; actual maintenance reconstructs from
        // the same verified durable receipt and cannot add an event or grant.
        let before = f
            .project
            .hub
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq();
        let rebuilt_jobs = crate::finish_jobs::FinishJobs::default();
        let directory = f
            .server
            .runtime
            .join("recovery-outbox")
            .join(&f.project.project_id);
        let repaired = maintain_project_finishes(
            &rebuilt_jobs,
            &f.project,
            &f.catalog,
            "host",
            &directory,
            None,
            &*f.server.clock,
        )
        .unwrap();
        assert!(repaired.contains(id));
        let reconstructed = rebuilt_jobs
            .wait(
                &f.project.project_id,
                &f.catalog.common_dir,
                id,
                Duration::from_secs(15),
            )
            .await
            .unwrap();
        assert_eq!(reconstructed["expiry_actual"], json!(actual[0]));
        assert!(reconstructed["actual"].is_null());
        assert!(reconstructed.get("evidence").is_none());
        assert_eq!(
            f.project
                .hub
                .lock()
                .unwrap()
                .protocol()
                .unwrap()
                .contiguous_seq(),
            before
        );
        assert!(!f
            .catalog
            .common_dir
            .join("towermd/cache/finish-expired-restart-count")
            .exists());
        stop_maintenance_fixture(&f).await;
    });
}
#[test]
fn minute_schedule_requires_paired_clocks_and_is_not_snapshot_backoff() {
    let last = ClockReading {
        mono_ms: 1_000,
        unix_ms: 2_000,
    };
    assert!(finish_maintenance_due(None, last));
    assert!(!finish_maintenance_due(
        Some(last),
        ClockReading {
            mono_ms: 60_999,
            unix_ms: 62_000
        }
    ));
    assert!(!finish_maintenance_due(
        Some(last),
        ClockReading {
            mono_ms: 61_000,
            unix_ms: 61_999
        }
    ));
    assert!(!finish_maintenance_due(
        Some(last),
        ClockReading {
            mono_ms: 0,
            unix_ms: 999_999
        }
    ));
    assert!(finish_maintenance_due(
        Some(last),
        ClockReading {
            mono_ms: 61_000,
            unix_ms: 62_000
        }
    ));
}

struct BrokenMaintenanceMain {
    path: PathBuf,
    backup: PathBuf,
}
impl BrokenMaintenanceMain {
    fn restore(&self) {
        if self.backup.exists() {
            if self.path.is_dir() {
                fs::remove_dir(&self.path).unwrap();
            }
            fs::rename(&self.backup, &self.path).unwrap();
        }
    }
}
impl Drop for BrokenMaintenanceMain {
    fn drop(&mut self) {
        self.restore();
    }
}
fn maintenance_outbox_bytes(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}
#[test]
fn original_ticket_deadline_cleans_owned_child_even_when_catalog_source_is_broken_without_faking_ready(
) {
    let mut f = maintenance_fixture(
        &[
            r#"printf x >> "$(git rev-parse --git-common-dir)/towermd/cache/finish-source-broken-count"; while :; do sleep 1; done"#,
        ],
        30,
    );
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _cancel_before_runtime_drop = MaintenanceCancellation(f.server.clone());
    rt.block_on(async {
        f.server
            .schedule_finish(
                f.project.clone(),
                f.catalog.clone(),
                f.ticket.take().unwrap(),
            )
            .unwrap();
        let count = f
            .catalog
            .common_dir
            .join("towermd/cache/finish-source-broken-count");
        let ready = Instant::now() + Duration::from_secs(15);
        while !count.exists() {
            assert!(
                Instant::now() < ready,
                "actual owned verifier did not start"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let directory = f
            .server
            .runtime
            .join("recovery-outbox")
            .join(&f.project.project_id);
        let before_bytes = maintenance_outbox_bytes(&directory);
        let before_seq = f
            .project
            .hub
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq();
        let path = f.catalog.common_dir.join("refs/heads/main");
        let backup = f
            .catalog
            .common_dir
            .join("refs/heads/main-task5-source-preserved");
        let main_before = fs::read(&path).unwrap();
        fs::rename(&path, &backup).unwrap();
        fs::create_dir(&path).unwrap();
        let restore = BrokenMaintenanceMain { path, backup };
        assert!(
            Catalog::load(&f.catalog.root).is_err(),
            "the real source must be unreadable"
        );
        // Normal recovery and the minute timer are deliberately not admitted.
        // Only the private actual-ticket owned-deadline hint can dispatch this
        // worker, so early Catalog errors cannot evade process supervision.
        f.project.finish_recovery.store(false, Ordering::Release);
        f.project
            .finish_retry_after
            .store(61 * 60_000, Ordering::Release);
        *f.project.finish_maintenance_at.lock().unwrap() = Some(ClockReading {
            mono_ms: 60 * 60_000,
            unix_ms: f.status.input.merge_deadline,
        });
        advance_maintenance(&f, 60 * 60_000);
        let stopped = Instant::now() + Duration::from_secs(15);
        while f
            .server
            .finish_jobs
            .active(
                &f.project.project_id,
                &f.catalog.common_dir,
                &f.status.input.evidence_id,
            )
            .unwrap()
            || f.project.finish_recovery_active.load(Ordering::Acquire)
        {
            assert!(
                Instant::now() < stopped,
                "unreadable local source stranded an overdue real owned verifier"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            f.project.finish_recovery.load(Ordering::Acquire),
            "source failure must remain uncertain"
        );
        assert!(
            f.server
                .finish_jobs
                .has_executed(
                    &f.project.project_id,
                    &f.catalog.common_dir,
                    &f.status.input.evidence_id
                )
                .unwrap(),
            "real cancelled execution material cannot be discarded without durable terminal/expiry"
        );
        assert_eq!(
            maintenance_outbox_bytes(&directory),
            before_bytes,
            "process cleanup cannot mint or confirm a business event"
        );
        assert_eq!(fs::read(&count).unwrap(), b"x");
        {
            let mut live = f.project.hub.lock().unwrap();
            let actual = live
                .protocol()
                .unwrap()
                .subscribe(before_seq, f.unix.load(Ordering::Acquire))
                .unwrap();
            assert!(!actual.events.iter().any(|event| matches!(
                event.kind.as_str(),
                "task.released" | "task.claimed" | "finish.completed" | "finish.interrupted"
            )));
            let state = live.projection().unwrap();
            assert_eq!(state.objects["Task"]["T-owned"]["state"], "verifying");
            assert_eq!(
                state.objects["Run"][&f.status.input.run_id]["state"],
                "verifying"
            );
            assert_eq!(
                state.objects["Run"][&f.status.input.run_id]["finish"]["merge_deadline"],
                f.status.input.merge_deadline
            );
        }
        restore.restore();
        assert_eq!(fs::read(&restore.path).unwrap(), main_before);
        advance_maintenance(&f, 60 * 60_000 + 6_000);
        maintenance_settled(&f).await;
        let inputs = f.server.recovery_inputs(&f.catalog).unwrap();
        let expired = inputs
            .confirmed()
            .iter()
            .filter(|event| {
                event.kind == "task.released"
                    && event.data["run"]["id"] == f.status.input.run_id
                    && event.data["run"]["state"] == "expired"
            })
            .collect::<Vec<_>>();
        assert_eq!(expired.len(), 1);
        assert_eq!(
            expired[0].data["run"]["finish"]["merge_deadline"],
            f.status.input.merge_deadline
        );
        assert!(!f
            .server
            .finish_jobs
            .has_executed(
                &f.project.project_id,
                &f.catalog.common_dir,
                &f.status.input.evidence_id
            )
            .unwrap());
        assert_eq!(fs::read(&count).unwrap(), b"x");
        stop_maintenance_fixture(&f).await;
    });
}

#[cfg(unix)]
#[test]
fn recovered_running_expiry_keeps_cleanup_unknown_without_killing_unowned_runner() {
    // This exercises a real accepted Running and a real still-live verifier,
    // but deliberately is not described as killing/restarting a daemon process.
    let (mut prior, ticket) = queued_owned_fixture(&[
        r#"common=$(git rev-parse --git-common-dir); printf '%s' "$$" > "$common/towermd/cache/finish-unknown-pid"; pwd > "$common/towermd/cache/finish-unknown-worktree"; printf x >> "$common/towermd/cache/finish-unknown-count"; while :; do sleep 1; done"#,
    ]);
    let status = ticket.status().clone();
    let execution = ProjectSession::launch_finish_shared(
        &prior.session,
        &prior.catalog,
        ticket,
        &mut prior.outbox,
        || Ok(status.input.entered_verifying_at + 1),
    )
    .unwrap();
    let running = prior
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| {
            event.kind == "finish.running" && event.data["run"]["id"] == status.input.run_id
        })
        .expect("actual original Running must already be durably confirmed")
        .clone();
    assert!(!running.stale);
    let fixture::Fixture {
        root,
        runtime,
        session,
        outbox,
        ..
    } = prior;
    drop(session);
    drop(outbox);
    let mono = Arc::new(AtomicU64::new(60 * 60_000));
    let unix = Arc::new(AtomicU64::new(status.input.merge_deadline));
    let mut server = super::super::tests::snapshot_server(
        root.path(),
        runtime.path().to_owned(),
        mono.clone(),
        unix.clone(),
    );
    Arc::get_mut(&mut server).unwrap().endpoint.host = "host".into();
    let catalog = Arc::new(Catalog::load(root.path()).unwrap());
    // Registration outside Tokio cannot run the recovery worker yet. No Job
    // or private execution ticket has ever been installed in the new registry.
    let project = server.project(&catalog).unwrap();
    let f = MaintenanceFixture {
        _root: root,
        _runtime: runtime,
        server,
        project,
        catalog,
        status: status.clone(),
        ticket: None,
        mono,
        unix,
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _cancel_before_runtime_drop = MaintenanceCancellation(f.server.clone());
    struct OriginalRunner {
        cancelled: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<towermd_core::finish::ExecutedFinish>>,
    }
    impl Drop for OriginalRunner {
        fn drop(&mut self) {
            self.cancelled.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let mut original = OriginalRunner {
        cancelled,
        worker: Some(std::thread::spawn(move || {
            finish::execute(execution, VERIFY_TIMEOUT, worker_cancelled)
        })),
    };
    let marker = f
        .catalog
        .common_dir
        .join("towermd/cache/finish-unknown-count");
    let started = Instant::now() + Duration::from_secs(15);
    while !marker.exists() {
        assert!(
            Instant::now() < started,
            "actual unowned verifier never reached its marker"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let pid = fs::read_to_string(
        f.catalog
            .common_dir
            .join("towermd/cache/finish-unknown-pid"),
    )
    .unwrap();
    let worktree = PathBuf::from(
        fs::read_to_string(
            f.catalog
                .common_dir
                .join("towermd/cache/finish-unknown-worktree"),
        )
        .unwrap()
        .trim(),
    );
    assert!(worktree.is_dir());
    rt.block_on(async {
        assert!(!f
            .server
            .finish_jobs
            .has_executed(
                &f.project.project_id,
                &f.catalog.common_dir,
                &status.input.evidence_id,
            )
            .unwrap());
        f.server
            .recover_finish_project(f.project.clone(), &f.catalog)
            .unwrap();
        maintenance_settled(&f).await;
        let recovered = f
            .server
            .finish_jobs
            .wait(
                &f.project.project_id,
                &f.catalog.common_dir,
                &status.input.evidence_id,
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(recovered["job_state"], "expired");
        assert_eq!(recovered["cleanup_unknown"], true);
        assert!(recovered["cleanup_diagnostic"]
            .as_str()
            .unwrap()
            .contains("identity/exit is unknown"));
        assert_eq!(
            recovered["input"],
            json!(status.input),
            "sealed original input must survive expiry diagnostics"
        );
        assert!(
            recovered["diagnostic"].is_null(),
            "cleanup annotation cannot replace the original diagnostic"
        );
        assert!(
            Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "new registry must not blindly signal the previous owner's process group"
        );
        assert!(
            worktree.is_dir(),
            "unproven old verification directory must be retained"
        );
        assert!(!original.worker.as_ref().unwrap().is_finished());
        assert_eq!(
            fs::read(&marker).unwrap(),
            b"x",
            "recovery never reexecutes the old request"
        );
        let inputs = f.server.recovery_inputs(&f.catalog).unwrap();
        assert!(!inputs.confirmed().iter().any(|event| matches!(
            event.kind.as_str(),
            "finish.completed" | "finish.interrupted"
        ) && event.data["run"]["id"]
            == status.input.run_id));
        let releases = inputs
            .confirmed()
            .iter()
            .filter(|event| {
                event.kind == "task.released"
                    && event.data["run"]["id"] == status.input.run_id
                    && event.data["run"]["state"] == "expired"
            })
            .collect::<Vec<_>>();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].data["task"]["state"], "ready");
        assert_eq!(
            recovered["expiry_actual"],
            json!(releases[0]),
            "retain the complete actual confirmed expiry as a separate audit"
        );
        assert!(
            recovered["actual"].is_null(),
            "task.released expiry cannot impersonate a finish terminal receipt"
        );
        // A subsequent full original-request read must preserve the same
        // warning instead of replacing it with a clean-looking derived view.
        let (_, _, _, refreshed) = f
            .server
            .prepare_finish(FinishRequest {
                directory: f.catalog.root.clone(),
                run: Some(status.input.run_id.clone()),
                wait: Some(status.input.evidence_id.clone()),
                report: None,
            })
            .unwrap();
        assert_eq!(refreshed["cleanup_unknown"], true);
        assert_eq!(refreshed["input"], json!(status.input));
        assert_eq!(refreshed["expiry_actual"], json!(releases[0]));
        assert_eq!(refreshed["expiry_actual"], recovered["expiry_actual"]);
        assert!(refreshed["actual"].is_null());
        assert_eq!(refreshed["actual"], recovered["actual"]);
        assert!(refreshed.get("evidence").is_none());
        stop_maintenance_fixture(&f).await;
    });
    // Only the test's original private runner capability cancels/joins its own
    // child. The new registry never manufactured an ExecutedFinish or cleanup.
    original.cancelled.store(true, Ordering::Release);
    let executed = original.worker.take().unwrap().join().unwrap();
    assert!(executed.cleanup_confirmed());
    assert!(!Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
    assert_eq!(fs::read(&marker).unwrap(), b"x");
}
