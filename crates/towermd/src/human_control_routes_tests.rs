//! Loopback wire and real durable B0/Task/Run controls; no forged Hold facts.
use super::super::{app_answer_routes as fixture, tests::snapshot_server};
use super::*;
fn headers(server: &Server, app: bool) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-towermd-instance",
        server.endpoint.instance.parse().unwrap(),
    );
    if app {
        headers.insert("x-towermd-client", "app".parse().unwrap());
    }
    headers
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_app_gate_controls_pause_and_exact_resume_without_cli_approval() {
    let fixture::Fixture {
        root,
        runtime,
        catalog,
        session,
        outbox,
        writer,
        ..
    } = fixture::claimed();
    let project_id = catalog.config.project_id().to_owned();
    drop(session);
    drop(outbox);
    drop(writer);
    let parent = runtime.path().join("recovery-outbox");
    fs::create_dir(&parent).unwrap();
    fs::rename(runtime.path().join("outbox"), parent.join(project_id)).unwrap();
    let mut server = snapshot_server(
        root.path(),
        runtime.path().to_owned(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(fixture::NOW + 40)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let endpoint = Arc::get_mut(&mut server).unwrap();
    endpoint.endpoint.host = "host".into();
    endpoint.endpoint.port = port;
    let app = router(server.clone());
    let serving = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    struct Stop(tokio::task::JoinHandle<()>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _stop = Stop(serving);
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    let url = |path: &str| format!("http://127.0.0.1:{port}{path}");
    let pause = json!({"directory":root.path(),"request_id":Uuid::now_v7().to_string(),"human_id":"control-owner","action":{"action":"pause_all"}});
    let rejected = client
        .post(url("/app/interventions/action"))
        .headers(headers(&server, false))
        .json(&pause)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status().as_u16(), 403);
    let accepted = client
        .post(url("/app/interventions/action"))
        .headers(headers(&server, true))
        .json(&pause)
        .send()
        .await
        .unwrap();
    let status = accepted.status();
    let original: Value = accepted.json().await.unwrap();
    assert!(status.is_success(), "{original}");
    let read = client
        .get(url("/app/interventions"))
        .headers(headers(&server, true))
        .query(&[("directory", root.path())])
        .send()
        .await
        .unwrap();
    let current: Value = read.json().await.unwrap();
    assert_eq!(current["paused"], true);
    let invalid = json!({"directory":root.path(),"request_id":Uuid::now_v7().to_string(),"human_id":"control-owner","action":{"action":"resume_all"}});
    assert!(
        !client
            .post(url("/app/interventions/action"))
            .headers(headers(&server, true))
            .json(&invalid)
            .send()
            .await
            .unwrap()
            .status()
            .is_success(),
        "App ResumeAll cannot bypass explicit confirmation"
    );
    let requested=client.post(url("/interaction")).headers(headers(&server,false)).json(&json!({"directory":root.path(),"run":null,"request_id":Uuid::now_v7().to_string(),"command":{"action":"resume_all"}})).send().await.unwrap();
    let code = requested.status();
    let requested: Value = requested.json().await.unwrap();
    assert!(code.is_success(), "{requested}");
    let read = client
        .get(url("/app/interventions"))
        .headers(headers(&server, true))
        .query(&[("directory", root.path())])
        .send()
        .await
        .unwrap();
    let current: Value = read.json().await.unwrap();
    assert_eq!(current["paused"], true);
    let binding = &current["resume_request"];
    let confirmation = json!({"directory":root.path(),"request_id":Uuid::now_v7().to_string(),"human_id":"control-owner","action":{"action":"confirm_resume","request":binding["request"],"pause_revision":binding["pause_revision"],"runs":binding["runs"]}});
    let mut forged = confirmation.clone();
    forged["actor"] = json!("human:other");
    assert!(!client
        .post(url("/app/interventions/action"))
        .headers(headers(&server, true))
        .json(&forged)
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    let accepted = client
        .post(url("/app/interventions/action"))
        .headers(headers(&server, true))
        .json(&confirmation)
        .send()
        .await
        .unwrap();
    let status = accepted.status();
    let confirmed: Value = accepted.json().await.unwrap();
    assert!(status.is_success(), "{confirmed}");
    let read = client
        .get(url("/app/interventions"))
        .headers(headers(&server, true))
        .query(&[("directory", root.path())])
        .send()
        .await
        .unwrap();
    let current: Value = read.json().await.unwrap();
    assert_eq!(current["paused"], false);
    let catalog = Catalog::load(root.path()).unwrap();
    let project = server.project(&catalog).unwrap();
    let inputs = server.recovery_inputs(&catalog).unwrap();
    let facts: Vec<_> = inputs
        .confirmed()
        .iter()
        .filter(|event| {
            event.kind == "hold.opened"
                && matches!(
                    event.data["request_body"]["action"]["action"].as_str(),
                    Some("pause_all" | "resume_all" | "confirm_resume")
                )
        })
        .cloned()
        .collect();
    let prompts: Vec<_> = inputs
        .confirmed()
        .iter()
        .filter(|event| event.kind == "human.prompt")
        .collect();
    assert_eq!(prompts.len(), 2);
    assert!(prompts
        .iter()
        .all(|event| event.actor == "human:control-owner"));
    assert_eq!(facts.len(), 3);
    assert_eq!(facts[0].actor, "human:control-owner");
    assert_eq!(facts[1].actor, "process:towermd-cli");
    assert_eq!(facts[2].actor, "human:control-owner");
    assert!(!project.hub.lock().unwrap().human().unwrap().paused());
    let retry = client
        .post(url("/app/interventions/action"))
        .headers(headers(&server, true))
        .json(&pause)
        .send()
        .await
        .unwrap();
    let status = retry.status();
    let retry: Value = retry.json().await.unwrap();
    assert!(status.is_success(), "{retry}");
    assert_eq!(retry["receipt"], original["receipt"]);
}
