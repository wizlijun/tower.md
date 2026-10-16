use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use towermd_core::config::Config;

const OBJECT: &str = "---\r\ntype: Module\r\nid: A\r\npaths: [src/**]\r\nfuture: 保留\r\n---\r\n权威模块🙂无末尾换行";
const REFERENCE: &str = "普通引用 café 中文🙂\r\n\r\n";

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
    daemon: Option<Child>,
}

impl Fixture {
    fn new() -> Self {
        let mut fixture = Self {
            root: tempfile::tempdir().unwrap(),
            runtime: tempfile::tempdir().unwrap(),
            daemon: None,
        };
        fixture.git(&["init", "-qb", "main"]);
        fixture.write(
            ".towermd/config.yaml",
            &Config::new(&uuid::Uuid::now_v7().to_string(), "docs/tower")
                .unwrap()
                .to_yaml(),
        );
        fixture.write("docs/tower/arch/A.md", OBJECT);
        fixture.write(
            "docs/tower/specs/AC.md",
            "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
        );
        fixture.write("README.md", REFERENCE);
        fixture.write("src/main.rs", "fn main() {}\n");
        fixture.write(
            "package.json",
            r#"{"scripts":{"test":"touch S5_EXECUTED"}}"#,
        );
        fixture.commit();
        fixture.start();
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn git(&self, args: &[&str]) -> String {
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
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .into()
    }

    fn commit(&self) {
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "fixture"]);
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
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn cli(&self, directory: &Path, args: &[&str]) -> Value {
        serde_json::from_slice(&self.success(directory, args).stdout).unwrap()
    }

    fn start(&mut self) {
        let previous = fs::read(self.runtime.path().join("endpoint.json")).ok();
        self.daemon = Some(
            self.command(self.root.path(), &["daemon", "serve"])
                .stdout(Stdio::null())
                .stderr(fs::File::create(self.runtime.path().join("daemon.log")).unwrap())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(bytes) = fs::read(self.runtime.path().join("endpoint.json")) {
                if Some(&bytes) != previous.as_ref() {
                    break;
                }
            }
            assert!(Instant::now() < deadline, "daemon 未启动");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn stop(&mut self) {
        if let Some(mut daemon) = self.daemon.take() {
            self.post("/shutdown", json!({}), true)
                .error_for_status()
                .unwrap();
            assert!(daemon.wait().unwrap().success());
        }
    }

    fn endpoint(&self) -> Value {
        serde_json::from_slice(&fs::read(self.runtime.path().join("endpoint.json")).unwrap())
            .unwrap()
    }

    fn client(&self) -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
    }

    fn post(&self, route: &str, body: Value, authorized: bool) -> reqwest::blocking::Response {
        let endpoint = self.endpoint();
        let mut request = self
            .client()
            .post(format!("http://127.0.0.1:{}{route}", endpoint["port"]))
            .json(&body);
        if authorized {
            request = request.header("x-towermd-instance", endpoint["instance"].as_str().unwrap());
        }
        request.send().unwrap()
    }

    fn get(
        &self,
        route: &str,
        directory: &Path,
        extra: &[(&str, &str)],
        authorized: bool,
    ) -> reqwest::blocking::Response {
        let endpoint = self.endpoint();
        let mut request = self
            .client()
            .get(format!("http://127.0.0.1:{}{route}", endpoint["port"]))
            .query(&[("directory", directory.to_str().unwrap())])
            .query(extra);
        if authorized {
            request = request.header("x-towermd-instance", endpoint["instance"].as_str().unwrap());
        }
        request.send().unwrap()
    }

    fn hub_dir(&self) -> PathBuf {
        self.root.path().join(".git/towermd")
    }

    fn journal(&self) -> Value {
        let text = fs::read_to_string(self.hub_dir().join("journal.jsonl")).unwrap();
        let mut lines = text.lines();
        let header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        let events = lines
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        json!({"snapshot_ack":header["checkpoint"]["ack"],"events":events})
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
fn scan_returns_core_preparation_without_publishing_or_executing() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "-qc", "t/scan"]);
    fixture.write("src/dirty.py", "dirty\n");
    fixture.git(&["add", "src/dirty.py"]);
    fixture.write("README.md", "dirty reference");
    let refs = fixture.git(&["show-ref"]);
    let status = fixture.git(&["status", "--porcelain"]);
    let index = fs::read(fixture.root.path().join(".git/index")).unwrap();
    let scan = fixture.cli(fixture.root.path(), &["baseline", "scan"]);
    let catalog = towermd_core::catalog::Catalog::load(fixture.root.path()).unwrap();
    let mut expected =
        towermd_core::coldstart::prepare_coldstart(&catalog, &Default::default()).unwrap();
    expected["main"] = serde_json::json!({"reference":"refs/heads/main", "fetched_at":null, "last_fetch_error":null});
    assert_eq!(scan, expected);
    assert_eq!(scan["stages"]["S5"], "not_run");
    assert_eq!(scan["coverage"]["source_files"], 1);
    assert_eq!(scan["options"]["history_limit"], 200);
    assert_eq!(
        fixture.cli(fixture.root.path(), &["baseline", "scan"]),
        scan
    );
    assert_eq!(fixture.git(&["show-ref"]), refs);
    assert_eq!(fixture.git(&["status", "--porcelain"]), status);
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).unwrap(),
        index
    );
    assert!(!fixture.hub_dir().exists());
    assert!(!fixture.root.path().join("docs/tower/tower.canvas").exists());
    assert!(!fixture.root.path().join("S5_EXECUTED").exists());
}

#[test]
fn reads_use_main_object_and_reference_bytes_in_two_projects() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let canvas = json!({"nodes":[
        {"id":"a","type":"file","file":"docs/tower/arch/A.md","towermd":{"obj":"A"},"x":-10,"y":20,"width":100,"height":100},
        {"id":"ref","type":"file","file":"README.md","x":50,"y":60,"width":100,"height":100}
    ],"edges":[],"future":"未知🙂"}).to_string() + "\r\n";
    fixture.write("docs/tower/tower.canvas", &canvas);
    fixture.commit();
    let main = fixture.git(&["rev-parse", "main"]);
    fixture.git(&["switch", "-qc", "t/read"]);
    fixture.write(
        "docs/tower/arch/A.md",
        &OBJECT.replace("权威模块", "任务分支"),
    );
    fixture.write("README.md", "task branch");
    fixture.write("docs/tower/tower.canvas", "{}");
    fixture.commit();
    fixture.write(
        "docs/tower/arch/A.md",
        &OBJECT.replace("权威模块", "脏工作树"),
    );
    let view: Value = fixture
        .get("/canvas", fixture.root.path(), &[], true)
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(view["revision"], main);
    assert_eq!(view["canvas_text"], canvas);
    assert_eq!(view["nodes"][1]["text"], REFERENCE);
    let object: Value = fixture
        .get("/objects/A", fixture.root.path(), &[], true)
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(object["revision"], main);
    assert_eq!(object["path"], "docs/tower/arch/A.md");
    assert_eq!(object["text"], OBJECT);
    other.write(
        "docs/tower/arch/A.md",
        &OBJECT.replace("权威模块", "另一项目"),
    );
    other.commit();
    let second: Value = fixture
        .get("/objects/A", other.root.path(), &[], true)
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_ne!(second["project_id"], object["project_id"]);
    assert_eq!(second["text"], OBJECT.replace("权威模块", "另一项目"));
    assert!(!fixture.hub_dir().exists());
    assert!(!other.hub_dir().exists());
}

#[test]
fn missing_and_invalid_canvas_remain_read_only_and_unknown_ids_are_errors() {
    let fixture = Fixture::new();
    let missing: Value = fixture
        .get("/canvas", fixture.root.path(), &[], true)
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(missing["status"], "missing");
    assert!(!fixture.root.path().join("docs/tower/tower.canvas").exists());
    fixture.write("docs/tower/tower.canvas", "{broken");
    fixture.commit();
    let invalid: Value = fixture
        .get("/canvas", fixture.root.path(), &[], true)
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(invalid["status"], "invalid");
    assert_ne!(invalid["revision"], missing["revision"]);
    assert_eq!(invalid["canvas_text"], "{broken");
    for id in [
        "missing",
        "README.md",
        "%2e%2e%2fREADME.md",
        "%2fetc%2fpasswd",
    ] {
        let response = fixture.get(&format!("/objects/{id}"), fixture.root.path(), &[], true);
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        let error: Value = response.json().unwrap();
        assert!(error["error"].as_str().unwrap().contains("对象不存在"));
    }
    assert!(!fixture.hub_dir().exists());
}

#[test]
fn api_rejects_wrong_instance_and_unknown_execution_fields() {
    let fixture = Fixture::new();
    let body = json!({"directory":fixture.root.path(),"options":{}});
    assert_eq!(
        fixture
            .post("/baseline/coldstart", body.clone(), false)
            .status(),
        reqwest::StatusCode::CONFLICT
    );
    for route in ["/canvas", "/objects/A"] {
        assert_eq!(
            fixture.get(route, fixture.root.path(), &[], false).status(),
            reqwest::StatusCode::CONFLICT
        );
        for field in ["approve", "execute", "output", "actor", "path", "instance"] {
            assert_eq!(
                fixture
                    .get(route, fixture.root.path(), &[(field, "true")], true)
                    .status(),
                reqwest::StatusCode::BAD_REQUEST
            );
        }
    }
    for field in [
        "approve",
        "execute",
        "execute_tests",
        "output",
        "actor",
        "submit",
    ] {
        for nested in [false, true] {
            let mut bad = body.clone();
            if nested {
                bad["options"][field] = json!(true);
            } else {
                bad[field] = json!(true);
            }
            assert_eq!(
                fixture.post("/baseline/coldstart", bad, true).status(),
                reqwest::StatusCode::UNPROCESSABLE_ENTITY
            );
        }
    }
    assert!(!fixture.hub_dir().exists());
}

#[test]
fn cli_help_and_history_limit_expose_prepare_only() {
    let fixture = Fixture::new();
    for args in [
        vec!["baseline", "--help"],
        vec!["baseline", "scan", "--help"],
    ] {
        let output = fixture.success(fixture.root.path(), &args);
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(!help.contains("submit"));
        assert!(!help.contains("--approve"));
    }
    for args in [
        vec!["baseline", "submit"],
        vec!["baseline", "approve"],
        vec!["baseline", "scan", "--execute"],
        vec!["baseline", "scan", "--output", "x"],
        vec!["baseline", "scan", "--yes"],
        vec!["baseline", "scan", "--history-limit", "0"],
        vec!["baseline", "scan", "--history-limit", "201"],
        vec!["baseline", "scan", "--history-limit", "-1"],
    ] {
        assert!(
            !fixture
                .command(fixture.root.path(), &args)
                .output()
                .unwrap()
                .status
                .success(),
            "{args:?}"
        );
    }
    for limit in [1, 200] {
        let scan = fixture.cli(
            fixture.root.path(),
            &["baseline", "scan", "--history-limit", &limit.to_string()],
        );
        assert_eq!(scan["options"]["history_limit"], limit);
    }
    for limit in [0, 201] {
        assert_eq!(
            fixture
                .post(
                    "/baseline/coldstart",
                    json!({"directory":fixture.root.path(),"options":{"history_limit":limit}}),
                    true
                )
                .status(),
            reqwest::StatusCode::BAD_REQUEST
        );
    }
    assert!(!fixture.hub_dir().exists());
}

#[test]
fn explicit_bad_run_is_rejected_without_creating_hub() {
    let fixture = Fixture::new();
    let output = fixture
        .command(
            fixture.root.path(),
            &["baseline", "scan", "--run", "bad-run"],
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("bad-run"));
    for route in ["/canvas", "/objects/A"] {
        let response = fixture.get(route, fixture.root.path(), &[("run", "bad-run")], true);
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert!(response.json::<Value>().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("bad-run"));
    }
    assert!(!fixture.hub_dir().exists());
}

#[test]
fn copied_hub_cannot_renew_against_another_clones_persistent_binding() {
    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let mut fixture = Fixture::new();
    fixture.cli(
        fixture.root.path(),
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
            "AC",
            "--allow",
            "src/**",
        ],
    );
    fixture.git(&["switch", "-qc", "t/T-1"]);
    let run = fixture.cli(fixture.root.path(), &["task", "claim", "T-1"]);
    fixture.stop();
    let duplicate = tempfile::tempdir().unwrap();
    copy_tree(fixture.root.path(), duplicate.path());
    let journal_path = duplicate.path().join(".git/towermd/journal.jsonl");
    let before = fs::read(&journal_path).unwrap();
    let original = fixture.journal();
    fixture.start();
    for route in ["/canvas", "/objects/A", "/baseline/coldstart"] {
        let response = if route == "/baseline/coldstart" {
            fixture.post(
                route,
                json!({"directory":duplicate.path(),"run":run["id"],"options":{}}),
                true,
            )
        } else {
            fixture.get(
                route,
                duplicate.path(),
                &[("run", run["id"].as_str().unwrap())],
                true,
            )
        };
        assert_eq!(
            response.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "{route}"
        );
        assert!(response.text().unwrap().contains("另一个 clone"));
    }
    let implicit = fixture.cli(duplicate.path(), &["baseline", "scan"]);
    assert!(implicit["warnings"][0]
        .as_str()
        .unwrap()
        .contains("另一个 clone"));
    assert_eq!(fs::read(journal_path).unwrap(), before);
    assert_eq!(fixture.journal(), original);
}

#[test]
fn existing_unbound_or_corrupt_hub_is_not_changed_by_reads() {
    let mut fixture = Fixture::new();
    fixture.cli(
        fixture.root.path(),
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
            "AC",
            "--allow",
            "src/**",
        ],
    );
    fixture.stop();
    fixture.start();
    let journal_path = fixture.hub_dir().join("journal.jsonl");
    let before = fs::read(&journal_path).unwrap();
    fixture.cli(fixture.root.path(), &["baseline", "scan"]);
    for route in ["/canvas", "/objects/A"] {
        fixture
            .get(route, fixture.root.path(), &[], true)
            .error_for_status()
            .unwrap();
        let response = fixture.get(route, fixture.root.path(), &[("run", "bad-run")], true);
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    }
    assert_eq!(fs::read(&journal_path).unwrap(), before);
    fs::write(&journal_path, "broken journal").unwrap();
    let scan = fixture.cli(fixture.root.path(), &["baseline", "scan"]);
    assert_eq!(scan["stages"]["S5"], "not_run");
    assert!(!scan["warnings"].as_array().unwrap().is_empty());
    for route in ["/canvas", "/objects/A"] {
        let value: Value = fixture
            .get(route, fixture.root.path(), &[], true)
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        assert!(!value["warnings"].as_array().unwrap().is_empty());
        assert_eq!(
            fixture
                .get(route, fixture.root.path(), &[("run", "bad-run")], true)
                .status(),
            reqwest::StatusCode::BAD_REQUEST
        );
    }
    assert!(!fixture
        .command(
            fixture.root.path(),
            &["baseline", "scan", "--run", "bad-run"]
        )
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(fs::read_to_string(journal_path).unwrap(), "broken journal");
}

#[test]
fn reads_renew_existing_run_after_restart_without_registering_snapshot_jobs() {
    let mut fixture = Fixture::new();
    fixture.write(
        ".towermd/config.yaml",
        &(fs::read_to_string(fixture.root.path().join(".towermd/config.yaml")).unwrap()
            + "\nsnapshot_interval: 1\n"),
    );
    fixture.commit();
    fixture.cli(
        fixture.root.path(),
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
            "AC",
            "--allow",
            "src/**",
        ],
    );
    let worktree = fixture.root.path().join("worktree");
    fixture.git(&[
        "worktree",
        "add",
        "-qb",
        "t/T-1",
        worktree.to_str().unwrap(),
        "main",
    ]);
    let run = fixture.cli(&worktree, &["task", "claim", "T-1"]);
    fixture.stop();
    let main = fixture.git(&["rev-parse", "main"]);
    fixture.start();
    for route in ["scan", "/canvas", "/objects/A", "explicit"] {
        let before = fixture.journal();
        let old = before["events"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|event| {
                event["lease_delta"]["Renew"]["leases"][0]["holder"] == run["id"]
                    || event["lease_delta"]["Grant"]["leases"][0]["holder"] == run["id"]
            })
            .map(|event| {
                if event["type"] == "lease.renewed" {
                    &event["lease_delta"]["Renew"]["leases"][0]["expires_at"]
                } else {
                    &event["lease_delta"]["Grant"]["leases"][0]["expires_at"]
                }
            })
            .unwrap()
            .as_u64()
            .unwrap();
        thread::sleep(Duration::from_millis(20));
        if route == "explicit" {
            fixture.cli(
                fixture.root.path(),
                &["baseline", "scan", "--run", run["id"].as_str().unwrap()],
            );
        } else if route == "scan" {
            fixture.cli(&worktree, &["baseline", "scan"]);
        } else {
            fixture
                .get(route, &worktree, &[], true)
                .error_for_status()
                .unwrap();
        }
        let after = fixture.journal();
        let renewed = after["events"].as_array().unwrap().last().unwrap();
        assert_eq!(renewed["type"], "lease.renewed");
        assert_eq!(
            renewed["lease_delta"]["Renew"]["leases"][0]["holder"],
            run["id"]
        );
        assert_eq!(renewed["actor"], "process:towermd-hub");
        assert!(
            renewed["lease_delta"]["Renew"]["leases"][0]["expires_at"]
                .as_u64()
                .unwrap()
                > old
        );
    }
    thread::sleep(Duration::from_millis(2200));
    assert_eq!(fixture.git(&["rev-parse", "main"]), main);
    assert!(!fixture.journal()["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["type"] == "snapshot.written"));
}
#[test]
fn read_renewal_must_not_release_an_unrelated_expired_run() {
    use towermd_core::{
        catalog::Catalog,
        project_session::ProjectSession as Hub,
        tasks::{self, TaskCommand},
    };
    let mut violations = Vec::new();
    for route in ["scan", "/canvas", "/objects/A"] {
        let mut fixture = Fixture::new();
        fixture.cli(fixture.root.path(), &["status"]);
        let host = fixture.endpoint()["host"].as_str().unwrap().to_owned();
        fixture.stop();
        fixture.write(
            "docs/tower/arch/B.md",
            "---\ntype: Module\nid: B\npaths: [lib/**]\n---\n",
        );
        fixture.write(
            "docs/tower/specs/AC-B.md",
            "---\ntype: Acceptance\nid: AC-B\nmodule: B\ncriteria: [works]\n---\n",
        );
        fixture.commit();
        let old_tree = fixture.root.path().join("old-tree");
        let active_tree = fixture.root.path().join("active-tree");
        fixture.git(&[
            "worktree",
            "add",
            "-qb",
            "t/T-old",
            old_tree.to_str().unwrap(),
            "main",
        ]);
        fixture.git(&[
            "worktree",
            "add",
            "-qb",
            "t/T-active",
            active_tree.to_str().unwrap(),
            "main",
        ]);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let earlier = now - 16 * 60 * 1000;
        let later = now - 14 * 60 * 1000;
        let catalog = Catalog::load(fixture.root.path()).unwrap();
        let mut hub = Hub::open(&fixture.hub_dir()).unwrap();
        for (id, module, paths, acceptance) in [
            ("T-old", "A", "src/**", "AC"),
            ("T-active", "B", "lib/**", "AC-B"),
        ] {
            let command: TaskCommand = serde_json::from_value(json!({"action":"add", "task":{"id":id,"title":id,"module":module,"allowed_paths":[paths],"acceptance":[acceptance]}})).unwrap();
            tasks::execute(&mut hub, &catalog, &host, command, None, earlier).unwrap();
        }
        for (directory, id, time) in [
            (&old_tree, "T-old", earlier),
            (&active_tree, "T-active", later),
        ] {
            let catalog = Catalog::load(directory).unwrap();
            tasks::execute(
                &mut hub,
                &catalog,
                &host,
                TaskCommand::Claim {
                    id: id.into(),
                    harness: None,
                    harness_version: None,
                },
                None,
                time,
            )
            .unwrap();
        }
        drop(hub);
        fixture.start();
        let before = fixture.journal()["events"].as_array().unwrap().len();
        if route == "scan" {
            fixture.cli(&active_tree, &["baseline", "scan"]);
        } else {
            fixture
                .get(route, &active_tree, &[], true)
                .error_for_status()
                .unwrap();
        }
        let after = fixture.journal();
        let new_events = &after["events"].as_array().unwrap()[before..];
        if new_events
            .iter()
            .filter(|event| event["type"] == "lease.renewed")
            .count()
            != 1
            || new_events
                .iter()
                .any(|event| event["type"] != "lease.renewed" && event["type"] != "lease.expired")
        {
            violations.push(json!({"route":route,"new_events":new_events.iter().map(|event|json!({"type":event["type"],"obj":event["obj"],"task_state":event["data"]["task"]["state"],"run_state":event["data"]["run"]["state"]})).collect::<Vec<_>>()}));
        }
    }
    assert!(
        violations.is_empty(),
        "read requests must only renew healthy binding, actual: {}",
        json!(violations)
    );
}
