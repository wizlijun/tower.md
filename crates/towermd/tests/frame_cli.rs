//! Templates and frames (FR-141–146, FR-153): anyone the daemon knows lists
//! templates; only a person in the App places a frame, moves or resizes it,
//! and the CLI only reads one back for a delegate.
#[path = "support/envelope.rs"]
mod envelope;

use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::Duration,
};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Tower Test", "-c", "user.email=tower@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

const SPRINT: &str = "id: sprint\nversion: 0.3.0\ntype: template\ntitle: 冲刺\nlayout: { x: { category: state }, y: { order: added } }\nstates: [Next, Doing, Done]\ndone_states: [Done]\ncard_fields: { owner: text }\nfuture_key: { kept: true }\n";
const BROKEN: &str = "id: broken\nversion: 1.0.0\ntype: template\nlayout: { x: { category: state } }\n";

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let fixture = Self { root: tempfile::tempdir().unwrap(), runtime: tempfile::tempdir().unwrap() };
        let root = fixture.root.path();
        git(root, &["init", "-qb", "main"]);
        for (id, text) in [("sprint", SPRINT), ("broken", BROKEN)] {
            fs::create_dir_all(root.join(format!(".towermd/extensions/{id}"))).unwrap();
            fs::write(root.join(format!(".towermd/extensions/{id}/towermd-plugin.yaml")), text).unwrap();
        }
        fs::write(root.join(".towermd/config.yaml"), "project_id: tower-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
        fs::write(root.join("README.md"), "fixture\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "fixture"]);
        fixture
    }
    fn output(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
            .args(args).current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null()).output().unwrap()
    }
    fn run(&self, args: &[&str]) -> Value {
        let output = self.output(args);
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stdout));
        envelope::data_checked(args, &output)
    }
    fn request(&self, endpoint: &Value, path: &str, client: &str, body: Option<Value>) -> (u16, Value) {
        let client_http = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap();
        let url = format!("http://127.0.0.1:{}{path}", endpoint["port"]);
        let request = match body {
            Some(body) => client_http.post(url).json(&body),
            None => client_http.get(url),
        }
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .header("x-towermd-client", client);
        let response = request.send().unwrap();
        let status = response.status().as_u16();
        (status, response.json().unwrap_or(Value::Null))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(&["daemon", "stop"]);
    }
}

fn code(output: &Output) -> Value {
    envelope::parse(output)["error"]["code"].clone()
}

fn request_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

#[test]
fn templates_list_starters_and_installed_ones_with_their_problems() {
    let fixture = Fixture::new();
    let listed = fixture.run(&["template", "ls"]);
    let ids: Vec<&str> = listed["templates"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["kanban", "quadrant", "timeline", "mindmap", "broken", "sprint"], "{listed}");
    let kanban = &listed["templates"][0];
    assert_eq!((kanban["origin"].as_str(), kanban["default_set_by"].as_str()), (Some("starter"), Some("delegate")));
    assert_eq!(kanban["layout"]["x"]["category"]["values"], json!(["待办", "进行中", "待评审", "完成"]));
    assert_eq!(listed["templates"][4]["problems"], json!(["按状态分栏的模板须声明 states"]));
    assert_eq!(listed["templates"][5]["origin"], "installed");
    assert!(listed["revision"].is_string());
    assert_eq!(code(&fixture.output(&["frame", "new", "--template", "kanban"])), "usage", "the CLI does not create frames");
}

#[test]
fn only_a_person_in_the_app_places_and_moves_a_frame() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let (_, opened) = fixture.request(&endpoint, "/projects/open", "app", Some(json!({"path":fixture.root.path()})));
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let state = || fixture.request(&endpoint, &format!("/projects/{handle}/state"), "app", None).1;
    let frames = format!("/projects/{handle}/frames");

    let (status, templates) = fixture.request(&endpoint, &format!("/projects/{handle}/templates"), "cli", None);
    assert_eq!((status, templates["templates"].as_array().map(Vec::len)), (200, Some(6)), "{templates}");

    let place = json!({"x":40,"y":-80,"width":960,"height":420});
    let body = |request: &str, template: &str| json!({"request_id":request,"id":"C-reading","title":"读论文","template":template,
        "rules":{"text":"每读完一篇论文，加一张卡片","add_card":"human"},"place":place});
    let first = request_id();

    // The CLI may not create one, nor may a body sign or define itself.
    let (status, refused) = fixture.request(&endpoint, &frames, "cli", Some(body(&first, "kanban")));
    assert_eq!((status, refused["code"].as_str()), (409, Some("conflict")), "{refused}");
    let mut forged = body(&request_id(), "kanban");
    forged["actor"] = json!("human:other");
    assert!(fixture.request(&endpoint, &frames, "app", Some(forged)).0 >= 400);
    let mut defined = body(&request_id(), "kanban");
    defined["definition"] = json!({"id":"kanban"});
    assert!(fixture.request(&endpoint, &frames, "app", Some(defined)).0 >= 400);
    let (status, broken) = fixture.request(&endpoint, &frames, "app", Some(body(&request_id(), "broken")));
    assert_eq!(status, 400, "a broken template fails closed: {broken}");
    let (status, missing) = fixture.request(&endpoint, &frames, "app", Some(body(&request_id(), "nope")));
    assert_eq!((status, missing["code"].as_str()), (400, Some("not_found")), "{missing}");
    let mut bound = body(&request_id(), "kanban");
    bound["id"] = json!("reading");
    assert!(fixture.request(&endpoint, &frames, "app", Some(bound)).0 >= 400, "frame IDs read C-…");
    assert!(state()["projection"].get("Frame").is_none(), "nothing was written: {}", state());

    // The person places an installed template; the definition is frozen as written.
    let (status, placed) = fixture.request(&endpoint, &frames, "app", Some(body(&first, "sprint")));
    assert_eq!(status, 200, "{placed}");
    let frame = &placed["frame"];
    assert_eq!((frame["created"]["actor"].as_str(), frame["set_by"].as_str(), frame["mode"].as_str()),
        (Some("human:local"), Some("human"), Some("free")), "unstated ownership is the person's: {frame}");
    assert_eq!(frame["template"]["definition"]["future_key"], json!({"kept":true}));
    assert_eq!(frame["rules"]["add_card"], "human");
    assert_eq!(frame["rules"]["fields"], json!(["owner"]));
    assert_eq!(fixture.request(&endpoint, &frames, "app", Some(body(&first, "sprint"))).1["seq"], placed["seq"], "a retry is the same event");
    let (status, _) = fixture.request(&endpoint, &frames, "app", Some(body(&first, "kanban")));
    assert_eq!(status, 400, "a reused request ID with another body conflicts");
    let (status, taken) = fixture.request(&endpoint, &frames, "app", Some(body(&request_id(), "kanban")));
    assert_eq!((status, taken["code"].as_str()), (400, Some("conflict")), "{taken}");

    // Creating a frame is published at once: the snapshot archives its event.
    let root = fixture.root.path();
    let paths = git(root, &["ls-tree", "-r", "--name-only", "refs/towermd/snapshot", "--", "docs/tower/state/events"]);
    let archived: Vec<Value> = paths.lines()
        .flat_map(|path| git(root, &["show", &format!("refs/towermd/snapshot:{path}")]).lines().map(|line| serde_json::from_str(line).unwrap()).collect::<Vec<_>>())
        .filter(|event: &Value| event["eid"] == placed["event"]).collect();
    assert_eq!(archived.len(), 1, "{paths}");
    assert_eq!((archived[0]["type"].as_str(), archived[0]["actor"].as_str()), (Some("frame.created"), Some("human:local")), "{}", archived[0]);

    // A delegate reads it back compactly.
    let shown = fixture.run(&["frame", "show", "C-reading"]);
    assert_eq!((shown["template"].as_str(), shown["rules"]["text"].as_str()), (Some("sprint@0.3.0"), Some("每读完一篇论文，加一张卡片")));
    assert_eq!(shown["states"], json!(["Next", "Doing", "Done"]));
    assert_eq!(code(&fixture.output(&["frame", "show", "C-other"])), "not_found");
    assert_eq!(code(&fixture.output(&["frame", "show", "reading"])), "usage");

    // Only the App moves or resizes it.
    let moved = format!("/projects/{handle}/frames/C-reading/place");
    let to = json!({"x":400,"y":0,"width":1000,"height":500});
    let (status, _) = fixture.request(&endpoint, &moved, "cli", Some(json!({"request_id":request_id(),"place":to})));
    assert_eq!(status, 409);
    let (status, _) = fixture.request(&endpoint, &moved, "app", Some(json!({"request_id":request_id(),"place":{"x":0,"y":0,"width":20,"height":500}})));
    assert_eq!(status, 400, "a frame has a minimum size");
    let (status, after) = fixture.request(&endpoint, &moved, "app", Some(json!({"request_id":request_id(),"place":to})));
    assert_eq!(status, 200, "{after}");
    assert_eq!(after["frame"]["place"], to);
    assert_eq!(after["frame"]["placed"]["actor"], "human:local");
    let (status, _) = fixture.request(&endpoint, &format!("/projects/{handle}/frames/C-other/place"), "app", Some(json!({"request_id":request_id(),"place":to})));
    assert_eq!(status, 400);

    // A frame is a Tower API target like any object (FR-156).
    let signal = fixture.output(&["signal", "eng.backlog", "--level", "passive", "--title", "栏位已满", "--target", "C-reading", "--from", "cron/backlog", "--from-kind", "source"]);
    assert!(signal.status.success(), "{}", String::from_utf8_lossy(&signal.stdout));
    let signals = state()["projection"]["TowerApi"]["signals"].clone();
    assert!(signals.as_array().unwrap().iter().any(|entry| entry["target"] == "C-reading"), "{signals}");
}
