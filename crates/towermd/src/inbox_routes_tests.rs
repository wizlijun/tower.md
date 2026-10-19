//! Actual loopback HTTP, local Hub, real B0/Task/Ask/App Answer producers.
use super::super::app_answer_routes as fixture;
use super::*;
use crate::daemon::finish_routes::tests::fixture::git;
use towermd_core::ask_answer_contract::AppAnswerAction;

struct HttpFixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
    server: Arc<Server>,
    client: reqwest::Client,
    serving: tokio::task::JoinHandle<()>,
}
impl HttpFixture {
    async fn start(root: tempfile::TempDir, runtime: tempfile::TempDir, now: u64) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (shutdown, mut stop) = tokio::sync::watch::channel(false);
        let server = Arc::new(Server {
            finish_jobs: Arc::new(FinishJobs::default()),
            scan_jobs: ScanJobs::new(runtime.path()).unwrap(),
            source_status_cache: Default::default(),
            main_refresh: MainRefresh::default(),
            clock: Arc::new(move || Ok(now)),
            mono_clock: Arc::new(|| 0),
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
            delivery_tokens: Mutex::new(DeliveryTokens::default()),
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
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap();
        Self {
            root,
            runtime,
            server,
            client,
            serving,
        }
    }
    async fn from_core(f: fixture::Fixture, now: u64) -> Self {
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
        let parent = runtime.path().join("recovery-outbox");
        fs::create_dir(&parent).unwrap();
        fs::rename(
            runtime.path().join("outbox"),
            parent.join(catalog.config.project_id()),
        )
        .unwrap();
        Self::start(root, runtime, now).await
    }
    async fn post(
        &self,
        path: &str,
        body: Value,
        instance: Option<&str>,
        activity: &str,
    ) -> (reqwest::StatusCode, Value) {
        let mut call = self
            .client
            .post(format!(
                "http://127.0.0.1:{}{path}",
                self.server.endpoint.port
            ))
            .header("x-towermd-inbox-activity", activity)
            .json(&body);
        if let Some(instance) = instance {
            call = call.header("x-towermd-instance", instance);
        }
        let response = call.send().await.unwrap();
        let code = response.status();
        let text = response.text().await.unwrap();
        let value = serde_json::from_str(&text).unwrap_or_else(|error| {
            assert!(
                !code.is_success(),
                "invalid successful HTTP JSON: {error}: {text}"
            );
            json!({"unparsed_error":text})
        });
        (code, value)
    }
    async fn read(&self) -> (reqwest::StatusCode, Value) {
        self.post(
            "/inbox",
            json!({"directory":self.root.path(),"run":null}),
            Some(&self.server.endpoint.instance),
            "recovery",
        )
        .await
    }
    fn cursor(&self) -> u64 {
        let catalog = Catalog::load(self.root.path()).unwrap();
        self.server
            .project(&catalog)
            .unwrap()
            .hub
            .lock()
            .unwrap()
            .last_seq()
    }
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.server.shutdown.send_replace(true);
        self.serving.abort();
    }
}
fn actual_answer(f: &mut fixture::Fixture, hold: &str, text: &str, now: u64) {
    let view = ProjectSession::load_ask_intervention_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        hold,
        || Ok(now),
    )
    .unwrap();
    ProjectSession::answer_ask_shared(
        &f.session,
        &f.catalog,
        "host",
        "human:owner",
        &Uuid::now_v7().to_string(),
        &AppAnswerAction::Answer {
            hold: hold.into(),
            opening: view.binding,
            text: text.into(),
        },
        &mut f.outbox,
        || Ok(now + 1),
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_http_read_has_no_ack_token_body_and_instance_are_bound_original_uuid_is_durable() {
    let mut core = fixture::claimed();
    let opened = fixture::call(
        &mut core,
        &Uuid::now_v7().to_string(),
        &fixture::open("HTTP delivery"),
        fixture::NOW + 20,
    );
    actual_answer(
        &mut core,
        &opened.hold,
        "actual human answer",
        fixture::NOW + 21,
    );
    let h = HttpFixture::from_core(core, fixture::NOW + 40).await;
    let cursor = h.cursor();
    let (code, value) = h.read().await;
    assert!(code.is_success(), "{value}");
    assert_eq!(h.cursor(), cursor);
    let offered: InboxResponse = serde_json::from_value(value).unwrap();
    assert_eq!(offered.messages.len(), 1);
    let id = Uuid::now_v7().to_string();
    let ids = vec![offered.messages[0].id.clone()];
    let body = json!({"directory":h.root.path(),"request_id":id,"delivery_token":offered.delivery_token,"message_ids":ids});
    for (instance, changed) in [
        (Some("another-instance"), body.clone()),
        (None, body.clone()),
        (Some(h.server.endpoint.instance.as_str()), {
            let mut body = body.clone();
            body["message_ids"] = json!(["not-offered"]);
            body
        }),
        (Some(h.server.endpoint.instance.as_str()), {
            let mut body = body.clone();
            body["delivery_token"] = json!("guessed");
            body
        }),
    ] {
        let (code, _) = h
            .post("/inbox/delivered", changed, instance, "recovery")
            .await;
        assert!(!code.is_success());
        assert_eq!(h.cursor(), cursor);
    }
    // Lose the successful HTTP body after the real server accepted/confirmed.
    // The next call must recover that same UUID, not append a replacement.
    let lost = h
        .client
        .post(format!(
            "http://127.0.0.1:{}/inbox/delivered",
            h.server.endpoint.port
        ))
        .header("x-towermd-instance", &h.server.endpoint.instance)
        .header("x-towermd-inbox-activity", "recovery")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(lost.status().is_success());
    drop(lost);
    assert_eq!(h.cursor(), cursor + 1);
    let (code, receipt) = h
        .post(
            "/inbox/delivered",
            body.clone(),
            Some(&h.server.endpoint.instance),
            "recovery",
        )
        .await;
    assert!(code.is_success(), "{receipt}");
    assert_eq!(receipt["receipt"]["result_ref"]["message_ids"], json!(ids));
    let accepted = h.cursor();
    assert_eq!(accepted, cursor + 1);
    let (code, retry) = h
        .post(
            "/inbox/delivered",
            body,
            Some(&h.server.endpoint.instance),
            "recovery",
        )
        .await;
    assert!(code.is_success(), "{retry}");
    assert_eq!(retry, receipt);
    assert_eq!(h.cursor(), accepted);
    let (code, empty) = h.read().await;
    assert!(code.is_success(), "{empty}");
    assert_eq!(empty, json!({"messages":[],"delivery_token":""}));
    assert_eq!(h.cursor(), accepted);
    let (code, _) = h
        .post(
            "/inbox",
            json!({"directory":h.root.path(),"run":null,"actor":"human:owner"}),
            Some(&h.server.endpoint.instance),
            "recovery",
        )
        .await;
    assert!(!code.is_success());
    assert_eq!(h.cursor(), accepted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn token_from_same_actual_scope_is_not_business_hash_and_cross_project_token_is_refused() {
    let mut core = fixture::claimed();
    let opened = fixture::call(
        &mut core,
        &Uuid::now_v7().to_string(),
        &fixture::open("token bytes"),
        fixture::NOW + 20,
    );
    actual_answer(
        &mut core,
        &opened.hold,
        "same frozen body",
        fixture::NOW + 21,
    );
    let h = HttpFixture::from_core(core, fixture::NOW + 40).await;
    let (_, a) = h.read().await;
    let (_, b) = h.read().await;
    assert_ne!(a["delivery_token"], b["delivery_token"]);
    let id = Uuid::now_v7().to_string();
    let message = a["messages"][0]["id"].clone();
    let make = |token: Value, directory: Value| json!({"directory":directory,"request_id":id,"delivery_token":token,"message_ids":[message]});
    let body = make(a["delivery_token"].clone(), json!(h.root.path()));
    let (code, first) = h
        .post(
            "/inbox/delivered",
            body,
            Some(&h.server.endpoint.instance),
            "recovery",
        )
        .await;
    assert!(code.is_success(), "{first}");
    let (code, same) = h
        .post(
            "/inbox/delivered",
            make(b["delivery_token"].clone(), json!(h.root.path())),
            Some(&h.server.endpoint.instance),
            "recovery",
        )
        .await;
    assert!(code.is_success(), "{same}");
    assert_eq!(first, same);
    let (other, other_root) = super::super::tests::snapshot_fixture(60);
    let cursor = h.cursor();
    let (code, _) = h
        .post(
            "/inbox/delivered",
            make(a["delivery_token"].clone(), json!(other_root)),
            Some(&h.server.endpoint.instance),
            "recovery",
        )
        .await;
    assert!(!code.is_success());
    assert_eq!(h.cursor(), cursor);
    drop(other);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_recovery_missing_or_empty_returns_unavailable_with_zero_side_effects_corruption_and_io_refuse(
) {
    for mode in ["missing", "empty", "corrupt", "io"] {
        let (root, project) = super::super::tests::snapshot_fixture(60);
        let runtime = tempfile::tempdir().unwrap();
        let catalog = Catalog::load(&project).unwrap();
        let store = catalog.common_dir.join("towermd");
        match mode {
            "empty" => {
                drop(Hub::open(&store).unwrap());
            }
            "corrupt" => {
                fs::create_dir_all(&store).unwrap();
                fs::write(store.join("journal.jsonl"), b"broken\n").unwrap();
            }
            "io" => {
                fs::create_dir_all(store.join("journal.jsonl")).unwrap();
            }
            _ => {}
        }
        let before = if store.join("journal.jsonl").is_file() {
            Some(fs::read(store.join("journal.jsonl")).unwrap())
        } else {
            None
        };
        // snapshot_fixture owns a parent directory. Select its project explicitly.
        let h = HttpFixture::start(root, runtime, fixture::NOW + 40).await;
        for activity in ["recovery", "invalid"] {
            let (code, value) = h
                .post(
                    "/inbox",
                    json!({"directory":project,"run":null}),
                    Some(&h.server.endpoint.instance),
                    activity,
                )
                .await;
            assert!(!code.is_success());
            if activity == "recovery" && matches!(mode, "missing" | "empty") {
                assert_eq!(value["code"], "inbox_unavailable_for_recovery");
            } else {
                assert_ne!(value["code"], "inbox_unavailable_for_recovery");
            }
            assert!(h.server.projects.lock().unwrap().is_empty());
            assert!(h.server.delivery_tokens.lock().unwrap().batches.is_empty());
            assert!(!h.runtime.path().join("recovery-outbox").exists());
            assert_eq!(
                if store.join("journal.jsonl").is_file() {
                    Some(fs::read(store.join("journal.jsonl")).unwrap())
                } else {
                    None
                },
                before
            );
        }
        if mode == "missing" {
            assert!(!store.exists(), "unavailable read must not create Hub");
        }
    }
}

fn reuse_source_fixture() -> (tempfile::TempDir, Catalog) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    fs::create_dir(root.path().join(".towermd")).unwrap();
    fs::write(
        root.path().join(towermd_core::config::CONFIG_PATH),
        towermd_core::config::Config::new("inbox-reuse", "docs/tower")
            .unwrap()
            .to_yaml(),
    )
    .unwrap();
    git(
        root.path(),
        &["add", "-f", towermd_core::config::CONFIG_PATH],
    );
    git(root.path(), &["commit", "-qm", "actual inbox source"]);
    let catalog = Catalog::load(root.path()).unwrap();
    (root, catalog)
}
fn inbox_refresh_loads(catalog: &Catalog) -> (Result<Option<Catalog>, String>, usize) {
    INBOX_REFRESH_FULL_LOADS.with(|count| count.set(0));
    let result = inbox_catalog(catalog);
    let loads = INBOX_REFRESH_FULL_LOADS.with(|count| count.get());
    (result, loads)
}

#[test]
fn unchanged_inbox_source_avoids_only_the_original_second_full_catalog_load() {
    let (_root, catalog) = reuse_source_fixture();
    let (result, loads) = inbox_refresh_loads(&catalog);
    assert!(result.unwrap().is_none());
    assert_eq!(
        loads, 0,
        "original current_scan_catalog call loaded once here"
    );
    // server.catalog's initial complete decode remains mandatory; this counter
    // does not include it or any Session/recovery/background source checks.
}

#[test]
fn new_main_and_branch_keep_original_independent_reload_semantics() {
    let (root, accepted) = reuse_source_fixture();
    git(
        root.path(),
        &["commit", "--allow-empty", "-qm", "new actual main"],
    );
    let (result, loads) = inbox_refresh_loads(&accepted);
    let refreshed = result.unwrap().unwrap();
    assert_ne!(refreshed.revision, accepted.revision);
    assert_eq!(loads, 1, "mismatch reloads; it is not a new equality gate");
    git(root.path(), &["switch", "-qc", "actual-inbox-branch"]);
    let (result, loads) = inbox_refresh_loads(&refreshed);
    assert_eq!(result.unwrap().unwrap().branch, "actual-inbox-branch");
    assert_eq!(loads, 1);
}

#[test]
fn changed_full_config_is_independently_reloaded_but_project_context_drift_stays_refused() {
    let (root, accepted) = reuse_source_fixture();
    let path = root.path().join(towermd_core::config::CONFIG_PATH);
    fs::write(
        &path,
        format!("{}unknown_probe_key: actual\n", accepted.config.to_yaml()),
    )
    .unwrap();
    git(
        root.path(),
        &["add", "-f", towermd_core::config::CONFIG_PATH],
    );
    git(root.path(), &["commit", "-qm", "actual extra config"]);
    let (result, loads) = inbox_refresh_loads(&accepted);
    let refreshed = result.unwrap().unwrap();
    assert!(refreshed.config.to_yaml().contains("unknown_probe_key"));
    assert_eq!(loads, 1);
    fs::write(
        &path,
        towermd_core::config::Config::new("different-project", "docs/tower")
            .unwrap()
            .to_yaml(),
    )
    .unwrap();
    git(
        root.path(),
        &["add", "-f", towermd_core::config::CONFIG_PATH],
    );
    git(root.path(), &["commit", "-qm", "actual changed project"]);
    let (result, loads) = inbox_refresh_loads(&refreshed);
    assert!(result.is_err());
    assert_eq!(loads, 1);
}
