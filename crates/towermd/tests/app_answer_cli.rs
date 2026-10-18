//! Actual CLI binary -> actual daemon -> fixed App HTTP Answer -> original
//! CLI wait. The authority is the supported entire first enabling commit,
//! never a forged B0/approval; core/adapter suites separately exercise real B0.
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use towermd_core::{
    ask_answer_contract::{AppAnswerAction, AskAnswerView},
    catalog::Catalog,
};
struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
    viewer: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let viewer = runtime.path().join("viewer");
        let f = Self {
            root,
            runtime,
            viewer,
        };
        f.git(&["init", "-qb", "main"]);
        f.success(&["init", "--docs-dir", "docs/tower"]);
        let config = f.root.path().join(".towermd/config.yaml");
        let text = fs::read_to_string(&config).unwrap();
        fs::write(config, format!("{text}\nsnapshot_interval: 7200\n")).unwrap();
        fs::create_dir(f.root.path().join("alpha")).unwrap();
        fs::write(f.root.path().join("alpha/a.rs"), "// actual source\n").unwrap();
        fs::write(
            f.root.path().join("docs/tower/arch/A.md"),
            "---\ntype: Module\nid: A\npaths: ['alpha/**']\nverify: []\ninterfaces: []\n---\n",
        )
        .unwrap();
        fs::write(f.root.path().join("docs/tower/specs/AC.md"), "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [actual App question and answer]\n---\n").unwrap();
        f.git(&["add", "."]);
        f.git(&["commit", "-qm", "actual first enabling import point"]);
        f.git(&["switch", "-qc", "t/T"]);
        f.git(&[
            "worktree",
            "add",
            "-qb",
            "viewer",
            f.viewer.to_str().unwrap(),
            "main",
        ]);
        f.success(&[
            "task",
            "add",
            "--id",
            "T",
            "--title",
            "Actual App Answer",
            "--module",
            "A",
            "--allow",
            "alpha/**",
            "--acceptance",
            "AC",
        ]);
        f.success(&["task", "claim", "T"]);
        f
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_towermd"));
        command
            .args(args)
            .current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null());
        command
    }
    fn success(&self, args: &[&str]) -> Output {
        let output = self.command(args).output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(self.root.path())
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }
    fn endpoint(&self) -> Value {
        serde_json::from_slice(&fs::read(self.runtime.path().join("endpoint.json")).unwrap())
            .unwrap()
    }
    fn actuals(&self) -> Vec<Value> {
        let catalog = Catalog::load(self.root.path()).unwrap();
        let directory = self
            .runtime
            .path()
            .join("recovery-outbox")
            .join(catalog.config.project_id());
        let mut events = Vec::new();
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let text = fs::read_to_string(path).unwrap();
            for line in text
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'))
            {
                let row: Value = serde_json::from_str(line).unwrap();
                if row["state"] == "confirmed" {
                    events.push(row["actual"].clone());
                }
            }
        }
        events
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.command(&["daemon", "stop"]).output();
    }
}
struct OwnedChild(Option<Child>);
impl OwnedChild {
    fn finish(&mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                return self.0.take().unwrap().wait_with_output().unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "actual CLI did not consume its answered original wait"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn client(endpoint: &Value) -> reqwest::blocking::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-towermd-instance",
        endpoint["instance"].as_str().unwrap().parse().unwrap(),
    );
    headers.insert("x-towermd-client", "app".parse().unwrap());
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .default_headers(headers)
        .build()
        .unwrap()
}
fn base(endpoint: &Value) -> String {
    format!("http://127.0.0.1:{}", endpoint["port"].as_u64().unwrap())
}
fn read(
    client: &reqwest::blocking::Client,
    base: &str,
    directory: &Path,
    hold: &str,
) -> AskAnswerView {
    for attempt in 0..3 {
        let response = client
            .get(format!("{base}/app/interventions/asks/{hold}"))
            .query(&[("directory", directory.to_string_lossy().to_string())])
            .send()
            .unwrap();
        let status = response.status();
        let value: Value = response.json().unwrap();
        if status.is_success() {
            return serde_json::from_value(value).unwrap();
        }
        let error = value["error"].as_str().unwrap_or("");
        assert!(
            attempt < 2
                && (error.contains("authority changed")
                    || error.contains("sources changed")
                    || error.contains("private snapshot source changed")),
            "{status}: {value}"
        );
    }
    unreachable!()
}
fn submit(client: &reqwest::blocking::Client, base: &str, body: &Value) -> Value {
    for attempt in 0..3 {
        let response = client
            .post(format!("{base}/app/interventions/action"))
            .json(body)
            .send()
            .unwrap();
        let status = response.status();
        let value: Value = response.json().unwrap();
        if status.is_success() {
            return value;
        }
        let error = value["error"].as_str().unwrap_or("");
        // A real concurrent snapshot can advance source authority. Retry only
        // this precise source refusal with the unchanged original UUID/body.
        assert!(
            attempt < 2
                && value["code"] == "request_uncertain"
                && (error.contains("authority changed")
                    || error.contains("sources changed")
                    || error.contains("private snapshot source changed")),
            "{status}: {value}"
        );
    }
    unreachable!()
}
#[test]
fn actual_cli_wait_receives_viewer_app_answer_and_same_original_survives_daemon_restart() {
    let f = Fixture::new();
    let id = uuid::Uuid::now_v7().to_string();
    let mut child = OwnedChild(Some(
        f.command(&[
            "ask",
            "是否继续本次实现？",
            "--option",
            "继续",
            "--option",
            "停止",
            "--recommended",
            "0",
            "--context",
            "真实CLI原上下文",
            "--request-id",
            &id,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap(),
    ));
    let stderr = child.0.as_mut().unwrap().stderr.take().unwrap();
    let (notice, notices) = std::sync::mpsc::channel();
    let stderr_reader = thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut all = Vec::new();
        loop {
            let mut line = Vec::new();
            if reader.read_until(b'\n', &mut line).unwrap() == 0 {
                break;
            }
            all.extend_from_slice(&line);
            let _ = notice.send(String::from_utf8_lossy(&line).into_owned());
        }
        all
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let hold = loop {
        let line = notices
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("CLI must reveal the real Hold ID on stderr before its bounded wait completes");
        if let Some(tail) = line.strip_prefix("Ask 已创建 hold=") {
            let (hold, explanation) = tail
                .split_once('；')
                .expect("actual Hold notice uses the fixed separator");
            assert!(hold.starts_with("H-"), "invalid actual Hold notice: {line}");
            assert_eq!(
                explanation.trim_end(),
                "可在 App 的“本次问答”中读取并回答。"
            );
            assert!(
                child.0.as_mut().unwrap().try_wait().unwrap().is_none(),
                "Hold notice must arrive while the original CLI is still waiting"
            );
            break hold.to_owned();
        }
    };
    // The notice is the user-visible discovery path. Actual durable records
    // verify its identity; they never manufacture or discover a substitute ID.
    let wait_id = loop {
        let actual = f.actuals();
        if let Some(opening) = actual
            .iter()
            .find(|e| e["type"] == "hold.opened" && e["data"]["p3_request"]["request_id"] == id)
        {
            assert_eq!(
                opening["obj"], hold,
                "stderr must reveal this exact actual accepted opening"
            );
            if let Some(wait) = actual
                .iter()
                .find(|e| e["type"] == "hold.wait_started" && e["obj"] == opening["obj"])
            {
                break wait["data"]["p3_request"]["request_id"]
                    .as_str()
                    .unwrap()
                    .to_owned();
            }
        }
        if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            let output = child.finish();
            panic!(
                "CLI exited before its actual wait: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        assert!(
            Instant::now() < deadline,
            "actual CLI did not persist its bounded original wait"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let endpoint = f.endpoint();
    let url = base(&endpoint);
    let app = client(&endpoint);
    let view = read(&app, &url, &f.viewer, &hold);
    view.binding.validate(&view.opening).unwrap();
    assert_eq!(view.current_hold["body"]["question"], "是否继续本次实现？");
    assert!(!view.actions.is_empty());
    let answer_id = uuid::Uuid::now_v7().to_string();
    let action = AppAnswerAction::Answer {
        hold: hold.clone(),
        opening: view.binding,
        text: "本次继续；保留人类原文\r\n不批准其他动作".into(),
    };
    let body = json!({"directory":f.viewer,"request_id":answer_id,"human_id":"cli-app-test-owner","action":action});
    let outcome = submit(&app, &url, &body);
    let output = child.finish();
    let stderr = stderr_reader.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&stderr)
    );
    let waited: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(waited["current"]["state"], "answered");
    assert_eq!(
        waited["current"]["reply"],
        outcome["receipt"]["result_ref"]["reply"]
    );
    assert_eq!(
        waited["current"]["reply"]["actor"],
        "human:cli-app-test-owner"
    );
    assert_eq!(waited["current"]["reply"]["relayed_from"], Value::Null);
    let original = f
        .actuals()
        .into_iter()
        .find(|e| {
            e["type"] == "hold.answered" && e["data"]["p3_request"]["request_id"] == answer_id
        })
        .unwrap();
    assert_eq!(original["data"]["p3_receipt"], outcome["receipt"]);
    f.success(&["daemon", "stop"]);
    f.success(&["daemon", "ensure"]);
    let restarted = f.endpoint();
    assert_ne!(endpoint["instance"], restarted["instance"]);
    let stale = app
        .post(format!("{}/app/interventions/action", base(&restarted)))
        .json(&body)
        .send()
        .unwrap();
    assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
    let next_app = client(&restarted);
    assert_eq!(submit(&next_app, &base(&restarted), &body), outcome);
    let resumed: Value = serde_json::from_slice(
        &f.success(&["ask", "--wait", &hold, "--request-id", &wait_id])
            .stdout,
    )
    .unwrap();
    assert_eq!(resumed["receipt"], waited["receipt"]);
    let after = f
        .actuals()
        .into_iter()
        .filter(|e| e["type"] == "hold.answered" && e["obj"] == hold)
        .collect::<Vec<_>>();
    assert_eq!(after, vec![original], "restart/old instance/original retry neither mint a second Answer nor rewrite original receipt");
}
