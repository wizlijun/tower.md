//! C1 read commands (spec §2.1): they look and never write. Each signs
//! nothing a person would sign, and the first screen stays at three items.
#[path = "support/envelope.rs"]
mod envelope;

use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Tower Test", "-c", "user.email=tower@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
    outside: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let fixture = Self { root: tempfile::tempdir().unwrap(), runtime: tempfile::tempdir().unwrap(), outside: tempfile::tempdir().unwrap() };
        let root = fixture.root.path();
        git(root, &["init", "-qb", "main"]);
        fs::create_dir(root.join(".towermd")).unwrap();
        fs::write(root.join(".towermd/config.yaml"), "project_id: read-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
        fs::write(root.join("README.md"), "fixture\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "fixture"]);
        fixture
    }
    fn output_in(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
            .args(args).current_dir(cwd)
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null()).output().unwrap()
    }
    fn output(&self, args: &[&str]) -> Output {
        self.output_in(self.root.path(), args)
    }
    fn run_in(&self, cwd: &Path, args: &[&str]) -> Value {
        let output = self.output_in(cwd, args);
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stdout));
        envelope::data_checked(args, &output)
    }
    fn run(&self, args: &[&str]) -> Value {
        self.run_in(self.root.path(), args)
    }
    fn daemon_started(&self) -> bool {
        self.runtime.path().join("endpoint.json").exists()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(&["daemon", "stop"]);
    }
}

#[test]
fn outside_a_project_the_reads_need_no_daemon() {
    let fixture = Fixture::new();
    let outside = fixture.outside.path();
    // No project yet: the bare command is the overview and points at init.
    let bare = envelope::parse(&fixture.output_in(outside, &[]));
    assert_eq!((bare["ok"].as_bool(), bare["data"]["projects"].clone()), (Some(true), json!([])), "{bare}");
    assert_eq!(bare["next"][0]["cmd"], "towermd init");
    assert_eq!(fixture.run_in(outside, &["overview"])["projects"], json!([]));
    assert!(!fixture.daemon_started(), "looking never starts the daemon");

    let completion = fixture.output_in(outside, &["completion", "zsh"]);
    assert!(completion.status.success());
    let script = String::from_utf8_lossy(&completion.stdout);
    assert!(script.contains("#compdef towermd") && script.contains("needs"), "{script}");
    assert_eq!(envelope::code(&fixture.output_in(outside, &["completion", "tcsh"])), "usage");

    let doctor = fixture.run_in(outside, &["doctor"]);
    assert!(doctor["git"].is_string() && doctor["project"].is_null(), "{doctor}");
    assert_eq!((doctor["daemon"]["running"].as_bool(), doctor["presence"]["available"].as_bool()), (Some(false), Some(false)));
    assert!(!fixture.daemon_started());

    // An unknown project is not found rather than guessed.
    let unknown = fixture.output_in(outside, &["--project", "read-project", "status"]);
    assert_eq!(envelope::code(&unknown), "not_found");
}

#[test]
fn reads_inside_a_project_follow_the_daemon_and_change_nothing() {
    let fixture = Fixture::new();
    let root = fixture.root.path();
    let outside = fixture.outside.path();

    let whoami = fixture.run(&["whoami"]);
    assert_eq!((whoami["actor"].as_str(), whoami["can_act_as_human"].as_bool()), (Some("process:towermd-cli"), Some(false)), "{whoami}");
    assert!(whoami["run"].is_null());
    assert_eq!(fixture.run(&["rebuild-index"])["rebuilt"], 0);

    let idea = fixture.run(&["idea", "add", "给周报加 CSV 导出"]);
    let idea_id = idea["idea"]["id"].as_str().unwrap().to_owned();
    let main = fs::read(root.join(".git/refs/heads/main")).ok();

    let needs = fixture.run(&["needs"]);
    assert!(needs["shown"].as_u64().unwrap() <= 3 && needs["items"].as_array().unwrap().len() <= 3, "{needs}");
    assert_eq!(needs["later"].as_u64().unwrap(), needs["total"].as_u64().unwrap() - needs["shown"].as_u64().unwrap());
    for item in needs["items"].as_array().unwrap() {
        let cmd = item["cmd"].as_str().unwrap();
        assert!(cmd.starts_with("towermd show ") || cmd.starts_with("towermd open"), "never an approval: {cmd}");
    }
    let all = fixture.run(&["needs", "--all"]);
    assert_eq!(all["items"].as_array().unwrap().len() as u64, all["total"].as_u64().unwrap());

    let bare = fixture.run(&[]);
    assert_eq!(bare["project"], "read-project", "{bare}");
    assert!(bare["needs"]["items"].as_array().unwrap().len() <= 3);
    assert_eq!(bare["digest"]["lines"].as_array().unwrap().len(), 3, "{bare}");

    let since = fixture.run(&["since", "--from", "1h"]);
    assert_eq!(since["lines"].as_array().unwrap().len(), 3, "{since}");
    assert_eq!(envelope::code(&fixture.output(&["since", "--from", "last tuesday"])), "usage");

    let ideas = fixture.run(&["ls", "ideas"]);
    assert_eq!(ideas["items"][0]["id"], idea_id.as_str(), "{ideas}");
    assert_eq!(fixture.run(&["ls", "ideas", "--since", "1h"])["items"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.run(&["ls", "decisions"])["items"], json!([]));
    assert_eq!(fixture.run(&["decision", "ls"])["items"], json!([]));
    assert!(fixture.run(&["rule", "ls"])["items"].is_array());
    assert_eq!(envelope::code(&fixture.output(&["decision", "show", "D-nobody"])), "not_found");
    assert_eq!(envelope::code(&fixture.output(&["show", "H-nobody"])), "not_found");
    assert_eq!(fixture.run(&["show", &idea_id])["kind"], "idea");
    let canvas = fixture.run(&["show", "read-project"]);
    assert!(canvas.get("kind").is_none(), "the project's own ID is the canvas: {canvas}");

    let history = fixture.run(&["history", &idea_id]);
    assert_eq!(history["events"][0]["actor"], "process:towermd-cli", "{history}");

    // Follow the stream: one compact JSON envelope per line, then stop.
    let watch = fixture.output(&["watch", "--exit-after", "1"]);
    assert!(watch.status.success(), "{}", String::from_utf8_lossy(&watch.stderr));
    let lines: Vec<Value> = String::from_utf8_lossy(&watch.stdout).lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.last().unwrap()["data"]["stopped"], true, "{lines:?}");

    // `wait --seconds` is bounded like the default wait.
    assert_eq!(envelope::code(&fixture.output(&["wait", "H-1", "--seconds", "86"])), "usage");

    // `--project` by path from anywhere, and by ID once this user ran towermd there.
    let by_path = fixture.run_in(outside, &["--project", root.to_str().unwrap(), "status"]);
    let by_id = fixture.run_in(outside, &["--project", "read-project", "status"]);
    assert_eq!(by_path["project"], by_id["project"], "{by_path}");
    let overview = fixture.run_in(outside, &["overview"]);
    assert_eq!(overview["projects"][0]["id"], "read-project", "{overview}");
    // FR-133: colour with shape and text, the count, the latest activity, and one list of at most three.
    let project = &overview["projects"][0];
    assert!(["red", "yellow", "green", "grey"].contains(&project["health"]["color"].as_str().unwrap()), "{overview}");
    assert!(project["health"]["symbol"].is_string() && project["health"]["label"].is_string(), "{overview}");
    assert!(project["last_activity"].as_str().is_some_and(|at| at.starts_with("20")), "{overview}");
    let attention = &overview["attention"];
    let shown = attention["items"].as_array().unwrap().len();
    assert!(shown <= 3 && shown as u64 == attention["shown"].as_u64().unwrap(), "{overview}");
    assert_eq!(attention["total"].as_u64().unwrap(), project["needs"].as_u64().unwrap(), "one project: the same items");
    for item in attention["items"].as_array().unwrap() {
        assert_eq!(item["project"], "read-project");
        assert!(item["cmd"].as_str().unwrap().starts_with("towermd --project read-project "), "{item}");
    }
    let doctor = fixture.run(&["doctor"]);
    assert_eq!((doctor["project"]["id"].as_str(), doctor["daemon"]["running"].as_bool()), (Some("read-project"), Some(true)), "{doctor}");

    // Reading changed neither the branch nor the working tree.
    assert_eq!(fs::read(root.join(".git/refs/heads/main")).ok(), main);
    let status = Command::new("git").current_dir(root).args(["status", "--porcelain"]).output().unwrap();
    assert!(status.stdout.is_empty(), "{}", String::from_utf8_lossy(&status.stdout));
}
