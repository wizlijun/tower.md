use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
    app_data: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir_in("/private/tmp").unwrap(),
            runtime: tempfile::tempdir_in("/private/tmp").unwrap(),
            app_data: tempfile::tempdir_in("/private/tmp").unwrap(),
        }
    }

    fn command(&self, directory: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_towermd"));
        command
            .args(args)
            .current_dir(directory)
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null());
        command
    }

    fn success(&self, directory: &Path, args: &[&str]) -> Output {
        let output = self.command(directory, args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        output
    }

    fn json(&self, directory: &Path, args: &[&str]) -> Value {
        serde_json::from_slice(&self.success(directory, args).stdout).unwrap()
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(self.root.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
    }

    fn claim(&self) -> (PathBuf, Value) {
        self.git(&["init", "-qb", "main"]);
        self.success(self.root.path(), &["init", "--docs-dir", "docs/tower"]);
        fs::write(
            self.root.path().join("docs/tower/arch/A.md"),
            "---\ntype: Module\nid: A\npaths: [src/**]\n---\n",
        )
        .unwrap();
        fs::write(
            self.root.path().join("docs/tower/specs/AC-A.md"),
            "---\ntype: Acceptance\nid: AC-A\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "fixture"]);
        self.success(
            self.root.path(),
            &[
                "task",
                "add",
                "--id",
                "T-1",
                "--title",
                "test",
                "--module",
                "A",
                "--acceptance",
                "AC-A",
                "--allow",
                "src/**",
            ],
        );
        let worktree = self.root.path().join("worktree");
        self.git(&[
            "worktree",
            "add",
            "-qb",
            "t/T-1",
            worktree.to_str().unwrap(),
            "main",
        ]);
        let run = self.json(&worktree, &["task", "claim", "T-1"]);
        (worktree, run)
    }

    fn journal(&self) -> Vec<u8> {
        fs::read(self.root.path().join(".git/towermd/journal.jsonl")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            if let Ok(log) = fs::read_to_string(self.runtime.path().join("daemon.log")) {
                eprintln!("fixture daemon.log: {log}");
            }
        }
        // Stop and drain our daemon before TempDir removes its runtime or project.
        let _ = self
            .command(self.app_data.path(), &["daemon", "stop"])
            .output();
    }
}

// The endpoint is redirected through a real TCP forwarder to the actual daemon
// router. No production hook or replacement response supplies discovery health.
struct RouterSpy {
    endpoint_path: PathBuf,
    original: Vec<u8>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl RouterSpy {
    fn new(fixture: &Fixture) -> Self {
        let endpoint_path = fixture.runtime.path().join("endpoint.json");
        let original = fs::read(&endpoint_path).unwrap();
        let mut endpoint: Value = serde_json::from_slice(&original).unwrap();
        let upstream = endpoint["port"].as_u64().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        endpoint["port"] = listener.local_addr().unwrap().port().into();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = requests.clone();
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();
            while !stopping.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("spy accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                let header = String::from_utf8(bytes).unwrap();
                let mut lines = header.split("\r\n");
                let first = lines.next().unwrap();
                seen.lock().unwrap().push(first.to_string());
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap().parse::<reqwest::Method>().unwrap();
                let path = parts.next().unwrap();
                let mut request =
                    client.request(method, format!("http://127.0.0.1:{upstream}{path}"));
                let mut length = 0;
                for line in lines.filter(|line| !line.is_empty()) {
                    let (name, value) = line.split_once(':').unwrap();
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                    if !["host", "connection", "content-length"]
                        .iter()
                        .any(|h| name.eq_ignore_ascii_case(h))
                    {
                        request = request.header(name, value.trim());
                    }
                }
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                let response = request.body(body).send().unwrap();
                let status = response.status().as_u16();
                let body = response.bytes().unwrap();
                write!(stream, "HTTP/1.1 {status} Forwarded\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        fs::write(&endpoint_path, serde_json::to_vec(&endpoint).unwrap()).unwrap();
        Self {
            endpoint_path,
            original,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for RouterSpy {
    fn drop(&mut self) {
        fs::write(&self.endpoint_path, &self.original).unwrap();
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn run_expiry(journal: &[u8], run: &Value) -> Vec<Value> {
    let text = std::str::from_utf8(journal).unwrap();
    let mut leases = std::collections::BTreeMap::new();
    for line in text.lines().skip(1) {
        let event: Value = serde_json::from_str(line).unwrap();
        for kind in ["Grant", "Renew"] {
            if let Some(values) = event["lease_delta"][kind]["leases"].as_array() {
                for lease in values.iter().filter(|lease| lease["holder"] == run["id"]) {
                    leases.insert(
                        lease["name"].as_str().unwrap().to_string(),
                        lease["expires_at"].clone(),
                    );
                }
            }
        }
    }
    leases.into_values().collect()
}

#[test]
fn ensure_is_maintenance_and_does_not_renew_or_deliver() {
    let fixture = Fixture::new();
    assert!(!fixture.runtime.path().join("endpoint.json").exists());
    assert!(!fixture.app_data.path().join(".git").exists());
    let endpoint = fixture.json(fixture.app_data.path(), &["daemon", "ensure"]);
    assert!(
        endpoint["pid"].is_u64(),
        "ensure did not start a daemon: {endpoint}"
    );
    assert_eq!(endpoint["address"], "127.0.0.1");
    assert!(endpoint["port"].as_u64().unwrap() > 0);
    assert!(endpoint["instance"].as_str().is_some_and(|s| !s.is_empty()));
    let (worktree, run) = fixture.claim();
    let before = fixture.journal();
    let expiry = run_expiry(&before, &run);
    assert!(!expiry.is_empty());
    let spy = RouterSpy::new(&fixture);
    for directory in [fixture.app_data.path(), worktree.as_path()] {
        for args in [
            vec!["daemon", "ensure"],
            vec!["daemon", "ensure", "--run", "missing"],
        ] {
            let actual = fixture.json(directory, &args);
            assert_eq!(actual["pid"], endpoint["pid"]);
            assert_eq!(actual["instance"], endpoint["instance"]);
        }
    }
    let after = fixture.journal();
    assert_eq!(run_expiry(&after, &run), expiry);
    assert_eq!(after, before, "maintenance changed journal bytes/sequence");
    let requests = spy.requests.lock().unwrap().clone();
    assert_eq!(
        requests.len(),
        4,
        "unexpected router requests: {requests:?}"
    );
    assert!(
        requests.iter().all(|line| line == "GET /health HTTP/1.1"),
        "maintenance entered command/inbox/delivered: {requests:?}"
    );
    drop(spy);
    // Normal CLI activity still takes the existing renewal path.
    thread::sleep(Duration::from_millis(20));
    fixture.success(&worktree, &["status"]);
    let active = fixture.journal();
    assert_ne!(active, after);
    let renewed = run_expiry(&active, &run);
    assert_eq!(renewed.len(), expiry.len());
    assert!(renewed
        .iter()
        .zip(expiry)
        .all(|(new, old)| new.as_u64().unwrap() > old.as_u64().unwrap()));
}

#[test]
fn concurrent_app_ensure_calls_share_one_daemon() {
    let fixture = Fixture::new();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let mut command = fixture.command(fixture.app_data.path(), &["daemon", "ensure"]);
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                command.output().unwrap()
            })
        })
        .collect();
    let outputs: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let endpoints: Vec<Value> = outputs
        .iter()
        .map(|output| {
            assert!(output.status.success(), "{output:?}");
            serde_json::from_slice(&output.stdout).unwrap()
        })
        .collect();
    assert!(endpoints[0]["pid"].is_u64());
    assert_eq!(endpoints[0], endpoints[1]);
    let saved: Value =
        serde_json::from_slice(&fs::read(fixture.runtime.path().join("endpoint.json")).unwrap())
            .unwrap();
    assert_eq!(saved, endpoints[0]);
}

#[test]
fn ensure_reports_real_runtime_failure_without_starting() {
    let fixture = Fixture::new();
    let blocked = fixture.app_data.path().join("runtime-file");
    fs::write(&blocked, "not a directory").unwrap();
    let output = fixture
        .command(fixture.app_data.path(), &["daemon", "ensure"])
        .env("TOWERMD_RUNTIME_DIR", &blocked)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("towermd:"), "{stderr}");
    assert!(
        stderr.contains("File exists") || stderr.contains("Not a directory"),
        "{stderr}"
    );
    assert_eq!(fs::read_to_string(blocked).unwrap(), "not a directory");
    assert!(!fixture.runtime.path().join("endpoint.json").exists());
}
