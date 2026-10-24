//! U-6: NFR-03 and NFR-08 measured against the real daemon binary.
//!
//! The canvas renders whatever the daemon pushes on the App's live
//! subscription (`/projects/{handle}/events`), so "shown on the canvas" is
//! measured here up to the moment that state frame reaches the subscriber;
//! the App's own share (frame to DOM) is measured by `src/perf.test.ts`.
//!
//! - NFR-03: a `towermd signal` written to the live state frame, ≤ 2 s.
//! - NFR-08: a `towermd ask` to the live state frame, ≤ 5 s; a person's answer
//!   in the App to the delegate's blocked `towermd wait` returning, ≤ 5 s.
//!
//! The default test uses a small project and a few samples, so CI stays fast.
//! The ignored test seeds the spec's scale first (500 tasks, 50 containers and
//! a few thousand events) and prints percentiles; see
//! `docs/superpowers/plans/2026-10-24-perf-acceptance.md` for how to run it.
#![cfg(unix)]
#[path = "support/envelope.rs"]
mod envelope;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, process::{Child, Command, Output, Stdio}, time::{Duration, Instant}};
use tokio_tungstenite::{connect_async, tungstenite::{client::IntoClientRequest, Message}};
type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const NFR03_EVENT_MS: u128 = 2_000;
const NFR08_INTERRUPT_MS: u128 = 5_000;
const NFR08_ANSWER_MS: u128 = 5_000;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Perf Test", "-c", "user.email=perf@example.invalid", "-c", "commit.gpgsign=false", "-c", "gc.auto=0"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn id() -> String { uuid::Uuid::now_v7().to_string() }
fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|value| value.parse().ok()).unwrap_or(default)
}

struct Fixture { root: tempfile::TempDir, runtime: tempfile::TempDir, trees: tempfile::TempDir }
struct Opened { endpoint: Value, handle: String }

impl Fixture {
    fn new() -> Self {
        let fixture = Self { root: tempfile::tempdir().unwrap(), runtime: tempfile::tempdir().unwrap(), trees: tempfile::tempdir().unwrap() };
        let root = fixture.root.path();
        git(root, &["init", "-qb", "main"]);
        fs::create_dir(root.join(".towermd")).unwrap();
        fs::write(root.join(".towermd/config.yaml"), "project_id: perf-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
        for name in ["alpha", "beta", "gamma"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::write(root.join(name).join("code.rs"), "fn task() {}\n").unwrap();
        }
        fs::write(root.join("Makefile"), "test:\n\t@printf 'perf fixture verification\\n'\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "fixture"]);
        fixture
    }
    fn command(&self, cwd: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_towermd"));
        command.env("TOWERMD_LANG", "zh-CN").args(args).current_dir(cwd).env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").stdin(Stdio::null());
        command
    }
    fn output(&self, cwd: &Path, args: &[&str]) -> Output { self.command(cwd, args).output().unwrap() }
    fn run(&self, cwd: &Path, args: &[&str]) -> Value {
        let output = self.output(cwd, args);
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stdout));
        envelope::data_checked(args, &output)
    }
    fn spawn(&self, cwd: &Path, args: &[&str]) -> Child {
        self.command(cwd, args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
    }
    fn open(&self) -> Opened {
        let endpoint = self.run(self.root.path(), &["daemon", "ensure"]);
        let opened = post(&endpoint, "/projects/open", "app", json!({"path": self.root.path()}));
        Opened { handle: opened["handle"].as_str().unwrap().into(), endpoint }
    }
    /// Approve a real baseline so tasks can be added, claimed and asked from.
    fn publish(&self, opened: &Opened) -> Value {
        let (root, endpoint, handle) = (self.root.path(), &opened.endpoint, &opened.handle);
        post(endpoint, &format!("/projects/{handle}/snapshot"), "app", json!({}));
        let scan = self.run(root, &["dev", "scan", "start"]);
        let scan_id = scan["id"].as_str().unwrap();
        let ready = loop {
            let output = self.output(root, &["dev", "scan", "wait", scan_id]);
            if output.status.code() != Some(5) { break output; }
        };
        assert!(ready.status.success(), "{}", String::from_utf8_lossy(&ready.stdout));
        let template = self.run(root, &["dev", "draft", "template", scan_id]);
        let input = self.runtime.path().join("input.json");
        fs::write(&input, serde_json::to_vec(&template).unwrap()).unwrap();
        let mut draft = self.run(root, &["dev", "draft", "submit", input.to_str().unwrap(), "--producer", "test/1"]);
        for module in draft["scan"]["modules"].as_array().unwrap().clone() {
            draft = post(endpoint, &format!("/projects/{handle}/baselines/{}/review", draft["id"].as_str().unwrap()), "app",
                json!({"action":{"kind":"commands","module_id":module["id"],"commands":[{"directory":"","command":"printf 'perf baseline proof\\n'"}]}}));
        }
        let request = id();
        post(endpoint, &format!("/projects/{handle}/baselines/{}/verify", draft["id"].as_str().unwrap()), "app", json!({"request_id": request}));
        let deadline = Instant::now() + Duration::from_secs(30);
        let job = loop {
            let job = get(endpoint, &format!("/projects/{handle}/baseline-verifications/{request}"));
            if job["status"] != "running" { break job; }
            assert!(Instant::now() < deadline, "baseline verification did not finish");
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(job["status"], "ready", "{job}");
        draft = get(endpoint, &format!("/projects/{handle}/baselines/{}", job["result_id"].as_str().unwrap()));
        git(root, &["checkout", "-qb", "perf-owner"]);
        let path = format!("/projects/{handle}/baselines/{}/publication", draft["id"].as_str().unwrap());
        let prepared = post(endpoint, &format!("{path}/prepare"), "app", json!({}));
        post(endpoint, &format!("{path}/approve"), "app", json!({"request_id":prepared["request_id"],"target":prepared["target"],
            "prepared_at":prepared["prepared_at"],"content_hash":prepared["content_hash"]}));
        draft
    }
    fn modules(draft: &Value) -> Vec<(String, String)> {
        draft["review"]["modules"].as_array().unwrap().iter()
            .map(|module| (module["id"].as_str().unwrap().into(), module["files"][0].as_str().unwrap().into())).collect()
    }
    /// The task the measuring delegate claims, in its own worktree.
    fn claim(&self, draft: &Value) -> PathBuf {
        let (module, file) = Self::modules(draft).remove(0);
        let task = self.runtime.path().join("T-perf.json");
        fs::write(&task, serde_json::to_vec(&json!({"id":"T-perf","title":"Perf delegate","module":module,
            "allowed_paths":[file],"acceptance":["existing smoke test passes"]})).unwrap()).unwrap();
        self.run(self.root.path(), &["task", "add", task.to_str().unwrap()]);
        let tree = self.trees.path().join("perf");
        git(self.root.path(), &["worktree", "add", "-b", "t/T-perf", tree.to_str().unwrap(), "main"]);
        self.run(&tree, &["task", "claim", "T-perf"]);
        tree
    }
    /// The spec's scale (NFR-03): tasks spread over the modules, containers each
    /// bound to an equal share of them, then delegate reports on those tasks.
    fn seed(&self, opened: &Opened, draft: &Value, tasks: usize, frames: usize, events: usize) {
        let (endpoint, handle, root) = (&opened.endpoint, &opened.handle, self.root.path());
        let modules = Self::modules(draft);
        let started = Instant::now();
        let ids: Vec<String> = (0..tasks).map(|n| format!("T-{n:04}")).collect();
        for (n, task) in ids.iter().enumerate() {
            let (module, file) = &modules[n % modules.len()];
            post(endpoint, &format!("/projects/{handle}/tasks"), "cli", json!({"request_id":id(),"cwd":root,
                "task":{"id":task,"title":format!("规模任务 {n}"),"module":module,"allowed_paths":[file],"acceptance":["smoke passes"]}}));
        }
        let tasks_done = started.elapsed();
        let share = tasks.div_ceil(frames.max(1));
        for frame in 0..frames {
            let objects: Vec<&String> = ids.iter().skip(frame * share).take(share).collect();
            post(endpoint, &format!("/projects/{handle}/frames"), "app", json!({"request_id":id(),"id":format!("C-perf{frame:02}"),
                "title":format!("容器 {frame}"),"template":"kanban","mode":"bound","objects":objects,
                "place":{"x":(frame % 10) * 1000,"y":(frame / 10) * 480,"width":960,"height":420}}));
        }
        let frames_done = started.elapsed();
        for n in 0..events {
            let target = &ids[n % ids.len().max(1)];
            post(endpoint, &format!("/projects/{handle}/tower"), "cli", json!({"request_id":id(),"cwd":root,"verb":"report",
                "target":target,"from":{"id":format!("perf/source-{}", n % 7),"kind":"source"},
                "data":{"state":"in_progress","summary":format!("第 {n} 步"),"next":format!("下一步 {}", n % 13)},"refs":[]}));
        }
        eprintln!("PERF seed tasks={tasks} ({:.1}s) frames={frames} ({:.1}s) events={events} ({:.1}s)", tasks_done.as_secs_f64(),
            (frames_done - tasks_done).as_secs_f64(), (started.elapsed() - frames_done).as_secs_f64());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = self.output(self.root.path(), &["daemon", "stop"]); }
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(180)).build().unwrap()
}
fn send(request: reqwest::blocking::RequestBuilder, endpoint: &Value, actor: &str, path: &str) -> Value {
    let response = request.header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .header("x-towermd-client", actor).send().unwrap();
    let status = response.status();
    let body: Value = response.json().unwrap();
    assert!(status.is_success(), "{path}: {status} {body}");
    body
}
fn post(endpoint: &Value, path: &str, actor: &str, body: Value) -> Value {
    send(client().post(format!("http://127.0.0.1:{}{path}", endpoint["port"])).json(&body), endpoint, actor, path)
}
fn get(endpoint: &Value, path: &str) -> Value {
    send(client().get(format!("http://127.0.0.1:{}{path}", endpoint["port"])), endpoint, "app", path)
}

async fn subscribe(opened: &Opened) -> Socket {
    let mut request = format!("ws://127.0.0.1:{}/projects/{}/events", opened.endpoint["port"], opened.handle).into_client_request().unwrap();
    request.headers_mut().insert("x-towermd-instance", opened.endpoint["instance"].as_str().unwrap().parse().unwrap());
    request.headers_mut().insert("x-towermd-client", "app".parse().unwrap());
    connect_async(request).await.unwrap().0
}
const FRAME_WAIT: &str = "matching state frame within 180 s";
/// The first state frame whose text satisfies `found`, with when it arrived.
/// The wait is generous so an over-budget sample is still measured, not lost.
async fn frame_where(socket: &mut Socket, found: impl Fn(&str) -> bool) -> Option<(Instant, String)> {
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            match socket.next().await.expect("stream closed").expect("valid frame") {
                Message::Text(text) => {
                    let at = Instant::now();
                    if found(text.as_str()) { return (at, text.to_string()); }
                }
                Message::Ping(data) => socket.send(Message::Pong(data)).await.unwrap(),
                other => panic!("unexpected frame: {other:?}"),
            }
        }
    }).await.ok()
}

/// Stagger samples across the daemon's 250 ms poll so the spread is real.
fn stagger(sample: usize) { std::thread::sleep(Duration::from_millis(150 + (sample as u64 * 53) % 250)); }

struct Series { name: &'static str, limit_ms: u128, samples: Vec<u128> }
impl Series {
    fn new(name: &'static str, limit_ms: u128) -> Self { Self { name, limit_ms, samples: vec![] } }
    fn push(&mut self, elapsed: Duration) { self.samples.push(elapsed.as_millis()); }
    fn percentile(sorted: &[u128], p: f64) -> u128 {
        sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
    }
    /// One line per metric, parsed by nothing; read by the person running it.
    fn report(&self) {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        if sorted.is_empty() { return eprintln!("PERF {:<28} n=0   not reached limit={}ms FAIL", self.name, self.limit_ms); }
        eprintln!("PERF {:<28} n={:<3} p50={:>5}ms p95={:>5}ms max={:>5}ms limit={}ms {}", self.name, sorted.len(),
            Self::percentile(&sorted, 0.5), Self::percentile(&sorted, 0.95), sorted[sorted.len() - 1], self.limit_ms,
            if sorted[sorted.len() - 1] <= self.limit_ms { "PASS" } else { "FAIL" });
    }
    fn assert_within(&self) {
        let worst = *self.samples.iter().max().unwrap_or_else(|| panic!("{}: no sample reached", self.name));
        assert!(worst <= self.limit_ms, "{}: worst {worst} ms exceeds {} ms ({:?})", self.name, self.limit_ms, self.samples);
    }
}

/// NFR-03: a signal from the CLI (spawned process, so its start-up counts)
/// until the state frame carrying it reaches the App's subscription.
fn event_to_live_state(fixture: &Fixture, executor: &tokio::runtime::Runtime, socket: &mut Socket, samples: usize) -> Series {
    let mut series = Series::new("nfr03.signal_to_live_state", NFR03_EVENT_MS);
    for sample in 0..samples {
        stagger(sample);
        let title = format!("perf-signal-{sample}-{}", &id()[24..]);
        let started = Instant::now();
        let child = fixture.spawn(fixture.root.path(), &["signal", "perf.tick", "--level", "passive", "--title", &title, "--from", "perf/bench"]);
        let (seen, _) = executor.block_on(frame_where(socket, |text| text.contains(&title))).expect(FRAME_WAIT);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
        series.push(seen - started);
    }
    series
}

/// NFR-08: a question (`towermd ask`) until it is on the live state, then a
/// person's answer in the App until the delegate's `towermd wait` returns it.
fn interrupt_and_answer(fixture: &Fixture, opened: &Opened, executor: &tokio::runtime::Runtime, socket: &mut Socket, tree: &Path, samples: usize) -> (Series, Series, Series) {
    let mut shown = Series::new("nfr08.ask_to_live_state", NFR08_INTERRUPT_MS);
    let mut answered = Series::new("nfr08.answer_to_waiting_cli", NFR08_ANSWER_MS);
    // Breakdown: how long the App's answer request itself takes to be accepted.
    let mut accepted = Series::new("part.answer_post_accepted", NFR08_ANSWER_MS);
    for sample in 0..samples {
        stagger(sample);
        let question = format!("性能问题 {sample} {}？", &id()[24..]);
        let started = Instant::now();
        let child = fixture.spawn(tree, &["ask", &question]);
        // Past the wait the sample is recorded as is and the series stops: the daemon is
        // still behind, so later samples would only measure the same backlog.
        let Some((seen, _)) = executor.block_on(frame_where(socket, |text| text.contains(&question))) else {
            shown.push(started.elapsed());
            eprintln!("PERF {} sample {sample}: no state frame within 180 s, series stopped", shown.name);
            break;
        };
        shown.push(seen - started);
        let output = child.wait_with_output().unwrap();
        let asked = envelope::parse(&output);
        let hold = asked["data"]["hold"].as_str().unwrap_or_else(|| panic!("ask gave no hold: {asked}")).to_owned();
        assert_eq!(asked["data"]["wait"]["status"], "pending", "{asked}");

        let mut waiting = fixture.spawn(tree, &["wait", &hold, "--seconds", "85"]);
        // Let the wait reach its polling loop: the person answers a delegate that is already waiting.
        std::thread::sleep(Duration::from_millis(600 + (sample as u64 * 53) % 250));
        assert!(waiting.try_wait().unwrap().is_none(), "wait returned before the answer");
        let started = Instant::now();
        post(&opened.endpoint, &format!("/projects/{}/answers", opened.handle), "app",
            json!({"request_id":id(),"hold":hold,"text":format!("回答 {sample}"),"remember_as":"temporary"}));
        accepted.push(started.elapsed());
        let returned = loop {
            if waiting.try_wait().unwrap().is_some() { break Some(Instant::now()); }
            if started.elapsed() > Duration::from_secs(90) { break None; }
            std::thread::sleep(Duration::from_millis(2));
        };
        // Past the CLI's own 85 s cap the sample is recorded as is and fails the series.
        let Some(returned) = returned else {
            answered.push(started.elapsed());
            waiting.kill().ok();
            continue;
        };
        answered.push(returned - started);
        let output = waiting.wait_with_output().unwrap();
        let waited = envelope::parse(&output);
        assert_eq!(waited["data"]["status"], "answered", "{waited}");
    }
    (shown, answered, accepted)
}

#[test]
fn live_state_and_answers_arrive_within_nfr03_and_nfr08_budgets() {
    let fixture = Fixture::new();
    let opened = fixture.open();
    let draft = fixture.publish(&opened);
    let tree = fixture.claim(&draft);
    let executor = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let mut socket = executor.block_on(subscribe(&opened));
    executor.block_on(frame_where(&mut socket, |_| true)).expect(FRAME_WAIT);
    let event = event_to_live_state(&fixture, &executor, &mut socket, 3);
    let (shown, answered, accepted) = interrupt_and_answer(&fixture, &opened, &executor, &mut socket, &tree, 2);
    accepted.report();
    for series in [&event, &shown, &answered] { series.report(); series.assert_within(); }
}

/// The spec's scale. Sizes and sample counts come from `TOWERMD_PERF_TASKS`
/// (500), `TOWERMD_PERF_FRAMES` (50), `TOWERMD_PERF_EVENTS` (3000) and
/// `TOWERMD_PERF_SAMPLES` (20). `TOWERMD_PERF_STATE_OUT=<file>` also writes the
/// real view and first state frame, for `src/perf.test.ts` to render.
#[test]
#[ignore = "U-6 scale measurement; run explicitly (see perf acceptance record)"]
fn scale_latency_at_spec_size() {
    let (tasks, frames, events, samples) = (env_usize("TOWERMD_PERF_TASKS", 500), env_usize("TOWERMD_PERF_FRAMES", 50),
        env_usize("TOWERMD_PERF_EVENTS", 3000), env_usize("TOWERMD_PERF_SAMPLES", 20));
    let fixture = Fixture::new();
    let opened = fixture.open();
    let draft = fixture.publish(&opened);
    fixture.seed(&opened, &draft, tasks, frames, events);
    let tree = fixture.claim(&draft);
    let executor = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();

    // The backend share of the first screen: the view and the first full state frame.
    let mut first_view = Series::new("nfr03.first_view_read", 1_000);
    let mut first_state = Series::new("nfr03.first_state_frame", 1_000);
    let mut size = 0;
    let mut dump = Value::Null;
    for _ in 0..5 {
        let started = Instant::now();
        let view = get(&opened.endpoint, &format!("/projects/{}/view", opened.handle));
        first_view.push(started.elapsed());
        let started = Instant::now();
        let mut socket = executor.block_on(subscribe(&opened));
        let (seen, text) = executor.block_on(frame_where(&mut socket, |_| true)).expect(FRAME_WAIT);
        first_state.push(seen - started);
        size = text.len();
        let state: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(state["state"]["status"], "ready", "{}", &text[..text.len().min(400)]);
        assert_eq!(state["state"]["frames"].as_array().map(Vec::len), Some(frames), "every container is in the state");
        assert!(state["state"]["projection"]["Task"].as_object().unwrap().len() > tasks, "every task is in the state");
        dump = json!({"view": view, "state": state["state"]});
        executor.block_on(socket.close(None)).ok();
    }
    first_view.report();
    first_state.report();
    eprintln!("PERF state frame bytes={size}");
    if let Ok(path) = std::env::var("TOWERMD_PERF_STATE_OUT") {
        fs::write(&path, serde_json::to_vec(&dump).unwrap()).unwrap();
        eprintln!("PERF wrote view and state to {path}");
    }

    // Breakdown, no limit of its own: the CLI's own round trip and the daemon's append alone.
    let mut cli = Series::new("part.cli_signal_roundtrip", NFR03_EVENT_MS);
    let mut append = Series::new("part.http_signal_append", NFR03_EVENT_MS);
    for sample in 0..samples {
        let started = Instant::now();
        fixture.run(fixture.root.path(), &["signal", "perf.part", "--level", "log", "--title", &format!("part-{sample}"), "--from", "perf/bench"]);
        cli.push(started.elapsed());
        let started = Instant::now();
        post(&opened.endpoint, &format!("/projects/{}/tower", opened.handle), "cli", json!({"request_id":id(),"cwd":fixture.root.path(),
            "verb":"signal","from":{"id":"perf/bench","kind":"source"},"data":{"kind":"perf.http","level":"log","title":format!("http-{sample}")},"refs":[]}));
        append.push(started.elapsed());
    }
    cli.report();
    append.report();

    let mut socket = executor.block_on(subscribe(&opened));
    executor.block_on(frame_where(&mut socket, |_| true)).expect(FRAME_WAIT);
    let event = event_to_live_state(&fixture, &executor, &mut socket, samples);
    event.report();
    let (shown, answered, accepted) = interrupt_and_answer(&fixture, &opened, &executor, &mut socket, &tree, samples);
    for series in [&shown, &answered, &accepted] { series.report(); }
    for series in [&event, &shown, &answered] { series.assert_within(); }
}

/// Dev-kit spec §冷启动: a repository of up to 200k lines finishes S1–S6 within
/// 15 minutes. The daemon's mechanical share, S1–S5 (`dev scan`), is measured on
/// a synthetic repository of `TOWERMD_PERF_LINES` lines (200000) spread over
/// 8 top-level directories with some shared history; S5 runs a trivial test
/// command. S6 is one read-only harness run by the user's Agent and is outside
/// this measurement.
#[test]
#[ignore = "U-6 cold-start measurement; run explicitly (see perf acceptance record)"]
fn cold_start_scan_of_a_200k_line_repository() {
    const COLD_START_MS: u128 = 15 * 60 * 1000;
    let lines = env_usize("TOWERMD_PERF_LINES", 200_000);
    let fixture = Fixture::new();
    let root = fixture.root.path();
    let (dirs, subdirs, per_file) = (8, 5, 100);
    let files = lines.div_ceil(per_file);
    let source = |index: usize| root.join(format!("pkg{}", index % dirs)).join(format!("part{}", (index / dirs) % subdirs)).join(format!("file{index}.rs"));
    for index in 0..files {
        fs::create_dir_all(source(index).parent().unwrap()).unwrap();
        let body: String = (0..per_file).map(|line| format!("pub fn f{index}_{line}() -> usize {{ {line} }}\n")).collect();
        fs::write(source(index), body).unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "synthetic source"]);
    // Some co-change history for S4: each commit touches a dozen files across directories.
    for commit in 0..30 {
        for index in (commit % dirs..files).step_by(dirs * 7).take(12) {
            let mut body = fs::read_to_string(source(index)).unwrap();
            body.push_str(&format!("// change {commit}\n"));
            fs::write(source(index), body).unwrap();
        }
        git(root, &["commit", "-qam", &format!("change {commit}")]);
    }
    let opened = fixture.open();
    post(&opened.endpoint, &format!("/projects/{}/snapshot", opened.handle), "app", json!({}));
    let started = Instant::now();
    let scan = fixture.run(root, &["dev", "scan", "start"]);
    let scan_id = scan["id"].as_str().unwrap();
    let ready = loop {
        let output = fixture.output(root, &["dev", "scan", "wait", scan_id]);
        if output.status.code() != Some(5) { break output; }
    };
    let mut series = Series::new("devkit.cold_start_s1_s5", COLD_START_MS);
    series.push(started.elapsed());
    assert!(ready.status.success(), "{}", String::from_utf8_lossy(&ready.stdout));
    eprintln!("PERF cold start lines={} files={files}", files * per_file);
    series.report();
    series.assert_within();
}
