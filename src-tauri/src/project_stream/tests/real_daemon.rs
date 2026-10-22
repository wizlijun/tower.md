//! Opt-in test of the actual packaged CLI and native stream, without a WebView.
use super::*;
use std::{fs, path::Path, process::{Command, Output, Stdio}, time::Instant as WallInstant};

const CHILD_ROOT: &str = "TOWERMD_NATIVE_RESTART_TEST_ROOT";
const TEST_NAME: &str = "project_stream::tests::real_daemon::packaged_daemon_reconnects_and_invalidates_http_cache";

fn sidecar() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("binaries/towermd-{}", env!("TOWERMD_TARGET")))
}

// File-backed output avoids a full pipe blocking either the CLI or test child.
// Every subprocess is bounded, including cleanup after a failed assertion.
fn run(mut command: Command, root: &Path, label: &str, timeout: Duration) -> Result<Output, String> {
    let log = root.join(format!("{label}-{}", uuid::Uuid::now_v7()));
    let stdout = log.with_extension("stdout");
    let stderr = log.with_extension("stderr");
    command.stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).map_err(|e| e.to_string())?)
        .stderr(fs::File::create(&stderr).map_err(|e| e.to_string())?);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let deadline = WallInstant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if WallInstant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            other => {
                let _ = child.kill(); let _ = child.wait();
                return Err(format!("{label} did not finish: {other:?}; logs: {}", log.display()));
            }
        }
    };
    Ok(Output { status, stdout: fs::read(stdout).map_err(|e| e.to_string())?,
        stderr: fs::read(stderr).map_err(|e| e.to_string())? })
}

fn cli_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(sidecar());
    command.args(args).current_dir(root.join("project"))
        .env("TOWERMD_RUNTIME_DIR", root.join("runtime"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

fn cli(root: &Path, args: &[&str]) -> Value {
    let result = run(cli_command(root, args), root, "cli", Duration::from_secs(30)).unwrap();
    assert!(result.status.success(), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
    serde_json::from_slice(&result.stdout).unwrap()
}

struct Cleanup { root: PathBuf, passed: bool }
impl Drop for Cleanup {
    fn drop(&mut self) {
        let stopped = run(cli_command(&self.root, &["daemon", "stop"]), &self.root,
            "cleanup", Duration::from_secs(30)).is_ok_and(|result| result.status.success());
        if self.passed && stopped {
            let _ = fs::remove_dir_all(&self.root);
        } else {
            eprintln!("Native daemon test logs retained: {} (cleanup stopped daemon: {stopped})", self.root.display());
        }
    }
}

fn fixture(root: &Path) {
    let project = root.join("project");
    fs::create_dir_all(project.join(".towermd")).unwrap();
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        command.current_dir(&project).args(["-c", "user.name=Native stream QA",
            "-c", "user.email=qa@example.invalid", "-c", "commit.gpgsign=false",
            "-c", "core.hooksPath=/dev/null"]).args(args);
        let result = run(command, root, "git", Duration::from_secs(15)).unwrap();
        assert!(result.status.success(), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
    };
    git(&["init", "-qb", "main"]);
    fs::write(project.join(".towermd/config.yaml"),
        "project_id: native-stream-qa\ndocs_dir: docs/tower\nhub: local\n").unwrap();
    fs::write(project.join("source.rs"), "fn task() {}\n").unwrap();
    git(&["add", "."]); git(&["commit", "-qm", "enable native stream fixture"]);
}

async fn matching(receiver: &mut mpsc::UnboundedReceiver<StreamMessage>,
    predicate: impl Fn(&StreamMessage) -> bool) -> StreamMessage {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let message = receiver.recv().await.expect("native delivery channel closed");
            if predicate(&message) { return message; }
        }
    }).await.expect("native state arrived before deadline")
}

fn exercise(root: &Path) {
    assert_eq!(PathBuf::from(std::env::var_os("TOWERMD_RUNTIME_DIR").unwrap()), root.join("runtime"));
    fixture(root);
    let project = root.join("project");
    let backend = crate::BackendState::default();
    // Populate the ordinary HTTP cache before the first stream connection.
    let initial = backend.lock().unwrap().request(&project, crate::backend::Operation::State).unwrap();
    assert_eq!(initial["status"], "not_activated");
    let endpoints = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&endpoints);
    let selected = project.clone(); let cached = Arc::clone(&backend);
    let resolve: Resolve = Arc::new(move || {
        let endpoint = crate::backend::stream_endpoint(&selected, &cached)?;
        observed.lock().unwrap().push((endpoint.port, endpoint.instance.clone(), endpoint.handle.clone()));
        Ok(endpoint)
    });
    let streams = Streams::default(); let (emit, mut receiver) = delivery();
    let subscription = uuid::Uuid::now_v7().to_string();
    streams.start("native-fixture".into(), subscription.clone(), resolve, emit).unwrap();
    let executor = runtime();
    let first = executor.block_on(matching(&mut receiver, |message| message.status == "connected"));
    assert_eq!(first.subscription, subscription); assert_eq!(first.token, "native-fixture");
    assert_eq!(first.state.unwrap()["status"], "not_activated");
    let old = endpoints.lock().unwrap().last().unwrap().clone();
    let pause = uuid::Uuid::now_v7().to_string();
    let receipt = cli(root, &["pause", "all", "--request-id", &pause]);
    let paused = executor.block_on(matching(&mut receiver, |message|
        message.state.as_ref().is_some_and(|state|
            state["projection"]["ProjectControl"]["pause_event"] == pause))).state.unwrap();
    assert_eq!(paused["projection"]["ProjectControl"]["paused"], true);
    assert!(paused["seq"].as_u64().unwrap() >= receipt["seq"].as_u64().unwrap());

    cli(root, &["daemon", "stop"]);
    executor.block_on(matching(&mut receiver, |message| message.status == "disconnected"));
    // No explicit ensure/open here: only the production native reconnect loop
    // may discover/start the replacement daemon and resubscribe this project.
    let renewed = executor.block_on(matching(&mut receiver, |message| message.status == "connected"));
    let state = renewed.state.unwrap();
    assert_eq!(state["status"], "ready");
    assert!(state["seq"].as_u64().unwrap() >= paused["seq"].as_u64().unwrap());
    assert_eq!(state["projection"]["ProjectControl"]["pause_event"], pause);
    assert_eq!(state["projection"]["ProjectControl"]["paused"], true);
    assert!(state["projection"]["Task"].is_object());
    let new = endpoints.lock().unwrap().last().unwrap().clone();
    assert_ne!(old.1, new.1); assert_ne!(old.2, new.2);

    let http = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
    let stale = http.get(format!("http://127.0.0.1:{}/projects/{}/state", new.0, old.2))
        .header("x-towermd-instance", &new.1).header("x-towermd-client", "app").send().unwrap();
    assert!(stale.status().is_client_error(), "old project handle must be invalid in the new daemon");
    // Exactly one request: a failed request would clear the cache and hide the
    // bug on retry. This first success proves reconnect invalidated both caches.
    let refreshed = backend.lock().unwrap().request(&project, crate::backend::Operation::State).unwrap();
    assert_eq!(refreshed["projection"]["ProjectControl"]["pause_event"], pause);
    assert!(refreshed["seq"].as_u64().unwrap() >= state["seq"].as_u64().unwrap());

    streams.cancel();
    // Wait for socket task destruction before stopping its daemon.
    executor.block_on(async { let _closed = streams.connection.lock().await; });
    cli(root, &["daemon", "stop"]);
    let resolutions = endpoints.lock().unwrap().len();
    std::thread::sleep(Duration::from_millis(2600));
    assert_eq!(cli(root, &["daemon", "status"])["running"], false);
    assert_eq!(endpoints.lock().unwrap().len(), resolutions, "cancelled stream must not resolve again");
}

#[test]
#[ignore = "requires prepared packaged CLI sidecar; starts isolated real daemons"]
fn packaged_daemon_reconnects_and_invalidates_http_cache() {
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = Path::new(&root);
        exercise(root);
        fs::write(root.join("completed"), b"native reconnect verified\n").unwrap();
        return;
    }
    assert!(sidecar().is_file(), "run pnpm app:build before this opt-in test");
    let root = std::env::temp_dir().join(format!("towermd-native-restart-{}", uuid::Uuid::now_v7()));
    fs::create_dir_all(root.join("project")).unwrap();
    let mut cleanup = Cleanup { root, passed: false };
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.args([TEST_NAME, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env(CHILD_ROOT, &cleanup.root).env("TOWERMD_RUNTIME_DIR", cleanup.root.join("runtime"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    let result = run(child, &cleanup.root, "child", Duration::from_secs(180)).unwrap();
    assert!(result.status.success(), "isolated native test failed; logs {}\n{}\n{}", cleanup.root.display(),
        String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    assert!(cleanup.root.join("completed").is_file(), "child must run the selected test, not zero matching tests");
    cleanup.passed = true;
}
