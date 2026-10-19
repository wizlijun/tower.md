use super::super::app_answer_routes as fixture;
use super::super::tests::{snapshot_fixture, snapshot_server};
use super::*;

#[tokio::test]
async fn actual_http_reads_claimed_task_after_restart_without_registration_or_events() {
    let fixture::Fixture {
        root,
        runtime,
        catalog,
        session,
        outbox,
        writer,
        ..
    } = fixture::claimed();
    let expected = session.lock().unwrap().projection().unwrap().clone();
    let journal = catalog.common_dir.join("towermd/journal.jsonl");
    let bytes = fs::read(&journal).unwrap();
    drop(session);
    drop(outbox);
    drop(writer);
    fs::create_dir(runtime.path().join("recovery-outbox")).unwrap();
    fs::rename(
        runtime.path().join("outbox"),
        runtime
            .path()
            .join("recovery-outbox")
            .join(catalog.config.project_id()),
    )
    .unwrap();
    for _ in 0..2 {
        let server = snapshot_server(
            root.path(),
            runtime.path().to_owned(),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(fixture::NOW + 20)),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = router(server.clone());
        let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = http
            .get(format!("http://{address}/app/projection"))
            .header("x-towermd-instance", &server.endpoint.instance)
            .query(&[("directory", root.path())])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let view: Value = response.json().await.unwrap();
        assert_eq!(view["state"], "recorded");
        assert_eq!(
            view["tasks"],
            json!(expected.objects["Task"].values().collect::<Vec<_>>())
        );
        assert_eq!(
            view["runs"],
            json!(expected.objects["Run"].values().collect::<Vec<_>>())
        );
        assert!(
            server.projects.lock().unwrap().is_empty(),
            "read must not register or recover finish jobs"
        );
        assert_eq!(fs::read(&journal).unwrap(), bytes);
        let wrong = http
            .get(format!("http://{address}/app/projection"))
            .header("x-towermd-instance", "old-instance")
            .query(&[("directory", root.path())])
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::CONFLICT);
        let mixed = http
            .get(format!("http://{address}/app/projection"))
            .header("x-towermd-instance", &server.endpoint.instance)
            .query(&[
                ("directory", root.path().to_str().unwrap()),
                ("run", "invented"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(mixed.status(), StatusCode::BAD_REQUEST);
        serving.abort();
    }
}

#[tokio::test]
async fn projection_missing_storage_stays_unavailable_and_corrupt_bytes_are_never_empty_success() {
    let (temp, root) = snapshot_fixture(7200);
    let catalog = Catalog::load(&root).unwrap();
    let path = catalog.common_dir.join("towermd");
    let server = snapshot_server(
        &root,
        temp.path().join("runtime"),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::NOW)),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-towermd-instance",
        server.endpoint.instance.parse().unwrap(),
    );
    let response = projection(
        State(server.clone()),
        headers.clone(),
        Query(ProgressQuery {
            directory: root.clone(),
        }),
    )
    .await;
    assert_eq!(response.0, StatusCode::OK);
    assert_eq!(response.1 .0["state"], "unavailable");
    assert!(!path.exists());
    assert!(server.projects.lock().unwrap().is_empty());
    drop(Hub::open(&path).unwrap());
    fs::write(path.join("journal.jsonl"), "complete corrupt line\n").unwrap();
    let before = fs::read(path.join("journal.jsonl")).unwrap();
    let response = projection(
        State(server.clone()),
        headers,
        Query(ProgressQuery { directory: root }),
    )
    .await;
    assert_eq!(response.0, StatusCode::BAD_REQUEST);
    assert_eq!(fs::read(path.join("journal.jsonl")).unwrap(), before);
    assert!(server.projects.lock().unwrap().is_empty());
}

type ReadHook = Box<dyn FnOnce() + Send>;
fn read_hooks() -> &'static Mutex<BTreeMap<PathBuf, ReadHook>> {
    static HOOKS: std::sync::OnceLock<Mutex<BTreeMap<PathBuf, ReadHook>>> =
        std::sync::OnceLock::new();
    HOOKS.get_or_init(Default::default)
}
pub(super) fn before_proof(root: &Path) {
    let hook = read_hooks().lock().unwrap().remove(root);
    if let Some(hook) = hook {
        hook();
    }
}

#[tokio::test]
async fn actual_http_status_and_claim_proceed_while_progress_proof_entry_is_blocked() {
    let mut f = fixture::claimed();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Release,
        None,
        fixture::NOW + 12,
        &mut f.outbox,
    )
    .unwrap();
    let fixture::Fixture {
        root,
        runtime,
        catalog,
        session,
        outbox,
        writer,
        ..
    } = f;
    drop(session);
    drop(outbox);
    drop(writer);
    fs::create_dir(runtime.path().join("recovery-outbox")).unwrap();
    fs::rename(
        runtime.path().join("outbox"),
        runtime
            .path()
            .join("recovery-outbox")
            .join(catalog.config.project_id()),
    )
    .unwrap();
    let mut server = snapshot_server(
        root.path(),
        runtime.path().to_owned(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::NOW + 20)),
    );
    Arc::get_mut(&mut server).unwrap().endpoint.host = "host".into();
    let project = server
        .project(&Catalog::load(root.path()).unwrap())
        .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    let mut release = Release(Some(release_tx));
    read_hooks().lock().unwrap().insert(
        catalog.root.clone(),
        Box::new(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let read_http = http.clone();
    let directory = root.path().to_path_buf();
    let instance = server.endpoint.instance.clone();
    let pending = tokio::spawn(async move {
        read_http
            .get(format!("http://{address}/app/projection"))
            .header("x-towermd-instance", instance)
            .query(&[("directory", directory)])
            .send()
            .await
            .unwrap()
    });
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(3)))
        .await
        .unwrap()
        .expect("real HTTP read must reach proof entry after capturing actual inputs");
    let (status_code, status) = tokio::time::timeout(Duration::from_secs(3), async {
        let response = http
            .post(format!("http://{address}/command"))
            .header("x-towermd-instance", &server.endpoint.instance)
            .json(&json!({"directory":root.path(),"run":null,"command":{"action":"status"}}))
            .send()
            .await
            .unwrap();
        let code = response.status();
        let body: Value = response.json().await.unwrap();
        (code, body)
    })
    .await
    .expect("complete status response blocked behind progress proof");
    assert_eq!(status_code, StatusCode::OK);
    assert_eq!(status["tasks"][0]["state"], "ready");
    assert!(
        !pending.is_finished(),
        "progress proof barrier must still hold after the full status response"
    );
    let (claim_code,claim)=tokio::time::timeout(Duration::from_secs(3),async {
        let response=http.post(format!("http://{address}/command")).header("x-towermd-instance",&server.endpoint.instance).json(&json!({"directory":root.path(),"run":null,"command":{"action":"claim","id":"T-ask"}})).send().await.unwrap();
        let code=response.status();let body:Value=response.json().await.unwrap();(code,body)
    }).await.expect("complete claim response blocked behind progress proof");
    assert_eq!(claim_code, StatusCode::OK);
    assert_eq!(claim["task"], "T-ask");
    assert_eq!(claim["state"], "running");
    assert!(
        !pending.is_finished(),
        "same progress request must remain at the causal barrier"
    );
    release.0.take().unwrap().send(()).unwrap();
    let response = tokio::time::timeout(Duration::from_secs(10), pending)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "task_progress_source_changed");
    assert_eq!(
        project.hub.lock().unwrap().projection().unwrap().objects["Run"]
            [claim["id"].as_str().unwrap()]["state"],
        "running"
    );
    serving.abort();
}
