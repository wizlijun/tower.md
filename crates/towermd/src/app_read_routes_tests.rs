use super::super::app_answer_routes as fixture;
use super::super::tests::{snapshot_fixture, snapshot_server};
use super::*;

async fn app_socket(
    address: std::net::SocketAddr,
    directory: PathBuf,
    instance: String,
    after: u64,
) -> (std::net::TcpStream, Vec<u8>) {
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        let mut url = reqwest::Url::parse(&format!("http://{address}/app/events")).unwrap();
        url.query_pairs_mut()
            .append_pair("directory", directory.to_str().unwrap())
            .append_pair("instance", &instance)
            .append_pair("after", &after.to_string());
        let target = format!("{}?{}", url.path(), url.query().unwrap());
        let mut socket = std::net::TcpStream::connect(address).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        write!(socket,"GET {target} HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").unwrap();
        let mut header = vec![];
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            socket.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
            assert!(header.len() < 16384);
        }
        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        (socket, header)
    }).await.unwrap()
}

fn app_frame(socket: &mut std::net::TcpStream) -> Value {
    use std::io::Read;
    let mut header = [0; 2];
    socket.read_exact(&mut header).unwrap();
    assert_eq!(header[0], 0x81, "expected one complete text frame");
    assert_eq!(header[1] & 0x80, 0, "server frame must be unmasked");
    let length = match header[1] & 0x7f {
        126 => {
            let mut size = [0; 2];
            socket.read_exact(&mut size).unwrap();
            u16::from_be_bytes(size) as usize
        }
        127 => {
            let mut size = [0; 8];
            socket.read_exact(&mut size).unwrap();
            usize::try_from(u64::from_be_bytes(size)).unwrap()
        }
        length => length as usize,
    };
    assert!(length < 1024 * 1024);
    let mut bytes = vec![0; length];
    socket.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn app_cursor_rejects_both_sides_of_actual_hub_window() {
    assert!(validate_cursor(9, 10, 20).is_err());
    assert!(validate_cursor(21, 10, 20).is_err());
    assert!(validate_cursor(10, 10, 20).is_ok());
    assert!(validate_cursor(20, 10, 20).is_ok());
}

#[tokio::test]
async fn app_socket_wrong_instance_and_unavailable_storage_do_not_create_or_register() {
    let (temp, root) = snapshot_fixture(7200);
    let catalog = Catalog::load(&root).unwrap();
    let storage = catalog.common_dir.join("towermd");
    let server = snapshot_server(
        &root,
        temp.path().join("runtime"),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::NOW)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_, rejected) = app_socket(address, root.clone(), "old-instance".into(), 0).await;
    assert!(rejected.starts_with(b"HTTP/1.1 409"));
    assert!(!storage.exists());
    let (mut socket, header) = app_socket(address, root, server.endpoint.instance.clone(), 0).await;
    assert!(header.starts_with(b"HTTP/1.1 101"));
    let reset = tokio::task::spawn_blocking(move || app_frame(&mut socket))
        .await
        .unwrap();
    assert_eq!(reset["kind"], "reset");
    assert!(reset["reason"]
        .as_str()
        .is_some_and(|reason| !reason.is_empty()));
    assert!(server.projects.lock().unwrap().is_empty());
    assert!(!storage.exists());
    serving.abort();
}

#[tokio::test]
async fn app_socket_only_notifies_actual_accepted_head_and_resets_ahead_cursor() {
    let fixture::Fixture {
        root,
        runtime,
        catalog,
        session,
        outbox,
        writer,
        ..
    } = fixture::claimed();
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
    let server = snapshot_server(
        root.path(),
        runtime.path().to_owned(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::NOW + 20)),
    );
    // Explicit producer setup belongs to this fixture, never the App route.
    let project = server
        .project(&Catalog::load(root.path()).unwrap())
        .unwrap();
    let (_, after) = project
        .hub
        .lock()
        .unwrap()
        .task_progress_cursor(catalog.config.project_id(), &catalog.common_dir)
        .unwrap();
    let applied = project.hub.lock().unwrap().applied_seq();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let journal = catalog.common_dir.join("towermd/journal.jsonl");
    let before = fs::read(&journal).unwrap();
    let (mut ahead, header) = app_socket(
        address,
        root.path().to_owned(),
        server.endpoint.instance.clone(),
        after + 1,
    )
    .await;
    assert!(header.starts_with(b"HTTP/1.1 101"));
    let reset = tokio::task::spawn_blocking(move || app_frame(&mut ahead))
        .await
        .unwrap();
    assert_eq!(reset["kind"], "reset");
    assert_eq!(fs::read(&journal).unwrap(), before);
    let (mut socket, header) = app_socket(
        address,
        root.path().to_owned(),
        server.endpoint.instance.clone(),
        after,
    )
    .await;
    assert!(header.starts_with(b"HTTP/1.1 101"));
    assert_eq!(fs::read(&journal).unwrap(), before);
    project
        .hub
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(
            towermd_core::hub_protocol::NewEvent {
                eid: Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:test".into(),
                kind: "future.audit".into(),
                layer: "L5".into(),
                obj: "actual-app-notification".into(),
                data: json!({"actual":"fsynced"}),
                guards: vec![],
            },
            fixture::NOW + 21,
        )
        .unwrap();
    let actual = fs::read(&journal).unwrap();
    let frame = tokio::task::spawn_blocking(move || app_frame(&mut socket))
        .await
        .unwrap();
    assert_eq!(frame["kind"], "changed");
    assert_eq!(frame["through_seq"], after + 1);
    assert!(frame["floor_seq"].as_u64().unwrap() <= after);
    assert!(frame["revision"]
        .as_str()
        .is_some_and(|revision| !revision.is_empty()));
    assert_eq!(frame.as_object().unwrap().len(), 4);
    assert_eq!(
        project.hub.lock().unwrap().applied_seq(),
        applied,
        "notification never installs or advances Session state"
    );
    assert_eq!(
        fs::read(journal).unwrap(),
        actual,
        "stream never renews or appends events"
    );
    serving.abort();
}

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
    let expected_events = session.lock().unwrap().events(0);
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
        assert_eq!(
            view["holds"],
            json!(expected
                .objects
                .get("Hold")
                .into_iter()
                .flat_map(|holds| holds.values())
                .collect::<Vec<_>>())
        );
        assert_eq!(view["events"], json!(expected_events));
        assert_eq!(view["events_floor_seq"], view["floor_seq"]);
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

#[cfg(unix)]
#[test]
fn actual_background_publication_selector_keeps_app_reads_available() {
    use std::os::unix::fs::PermissionsExt;
    const CHILD: &str = "TOWERMD_SELECTOR_LOCK_CHILD";
    const GATES: &str = "TOWERMD_SELECTOR_LOCK_GATES";
    if std::env::var_os(CHILD).is_none() {
        let temporary = tempfile::tempdir().unwrap();
        let gates = temporary.path().canonicalize().unwrap();
        let bin = gates.join("bin");
        fs::create_dir(&bin).unwrap();
        let real = Command::new("which").arg("git").output().unwrap();
        assert!(real.status.success());
        let program = bin.join("git");
        fs::write(&program, r#"#!/bin/sh
if test -f "$TOWERMD_SELECTOR_LOCK_GATES/armed" && test . -ef "$TOWERMD_SELECTOR_LOCK_GATES/directory"; then
    selector=0
    for argument in "$@"; do
        if test "$argument" = 'refs/towermd/drafts/publication-*'; then selector=1; fi
    done
    if test "$selector" = 1 && mkdir "$TOWERMD_SELECTOR_LOCK_GATES/once" 2>/dev/null; then
        touch "$TOWERMD_SELECTOR_LOCK_GATES/entered"
        attempts=0
        while ! test -f "$TOWERMD_SELECTOR_LOCK_GATES/release"; do
            attempts=$((attempts + 1))
            if test "$attempts" -ge 1000; then exit 98; fi
            sleep 0.01
        done
    fi
fi
exec "$TOWERMD_SELECTOR_LOCK_REAL_GIT" "$@"
"#).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "daemon::app_read_routes::tests::actual_background_publication_selector_keeps_app_reads_available", "--nocapture"])
            .env(CHILD, "1").env(GATES, &gates)
            .env("TOWERMD_SELECTOR_LOCK_REAL_GIT", String::from_utf8(real.stdout).unwrap().trim())
            .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
            .output().unwrap();
        assert!(
            output.status.success(),
            "child status={:?}\n{}\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (temp, root) = snapshot_fixture(7200);
            let gates = PathBuf::from(std::env::var_os(GATES).unwrap());
            std::os::unix::fs::symlink(root.canonicalize().unwrap(), gates.join("directory"))
                .unwrap();
            let server = snapshot_server(
                &root,
                temp.path().join("runtime"),
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(fixture::NOW)),
            );
            let catalog = Catalog::load(&root).unwrap();
            // A real accepted event makes this a recorded, empty-task project.
            // An initialized repository alone correctly projects Unavailable.
            {
                use towermd_core::hub_protocol::HubProtocol;
                let mut hub =
                    towermd_core::hub::Hub::open(&catalog.common_dir.join("towermd")).unwrap();
                hub.append(
                    towermd_core::hub_protocol::NewEvent {
                        eid: uuid::Uuid::now_v7().to_string(),
                        host: "snapshot-host".into(),
                        actor: "process:test".into(),
                        kind: "future.audit".into(),
                        layer: "L5".into(),
                        obj: "audit".into(),
                        data: json!({}),
                        guards: vec![],
                    },
                    fixture::NOW,
                )
                .unwrap();
            }
            let project = server.project(&catalog).unwrap();
            struct Release(PathBuf);
            impl Drop for Release {
                fn drop(&mut self) {
                    let _ = fs::write(self.0.join("release"), "");
                }
            }
            let _release = Release(gates.clone());
            fs::write(gates.join("armed"), "").unwrap();
            server.tick().unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !gates.join("entered").exists() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(
                gates.join("entered").exists(),
                "actual background selector did not reach its Git read"
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let app = router(server.clone());
            let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let http = reqwest::Client::builder().no_proxy().build().unwrap();
            let journal = catalog.common_dir.join("towermd/journal.jsonl");
            let before = fs::read(&journal).unwrap();
            let (code, view) = tokio::time::timeout(Duration::from_secs(3), async {
                let response = http
                    .get(format!("http://{address}/app/projection"))
                    .header("x-towermd-instance", &server.endpoint.instance)
                    .query(&[("directory", &root)])
                    .send()
                    .await
                    .unwrap();
                let code = response.status();
                let view: Value = response.json().await.unwrap();
                (code, view)
            })
            .await
            .expect("actual App projection waited behind the selector producer lock");
            assert_eq!(code, StatusCode::OK, "{view}");
            assert_eq!(view["state"], "recorded", "{view}");
            assert_eq!(view["tasks"], json!([]));
            assert_eq!(fs::read(&journal).unwrap(), before);
            let (code, status) = tokio::time::timeout(Duration::from_secs(3), async {
                let response = http
                    .post(format!("http://{address}/command"))
                    .header("x-towermd-instance", &server.endpoint.instance)
                    .json(&json!({"directory":root,"run":null,"command":{"action":"status"}}))
                    .send()
                    .await
                    .unwrap();
                let code = response.status();
                let status: Value = response.json().await.unwrap();
                (code, status)
            })
            .await
            .expect("actual status waited behind the background selector");
            assert_eq!(code, StatusCode::OK, "{status}");
            assert!(!project
                .background
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_finished());
            assert!(
                !gates.join("release").exists(),
                "the actual selector must remain blocked until both HTTP reads finish"
            );
            fs::write(gates.join("release"), "").unwrap();
            server.drain_snapshots().await.unwrap();
            assert!(!project.publication_reconcile.load(Ordering::Acquire));
            server.shutdown.send_replace(true);
            server.main_refresh.drain().await.unwrap();
            serving.abort();
        });
}
