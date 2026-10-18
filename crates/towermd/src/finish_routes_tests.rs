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
