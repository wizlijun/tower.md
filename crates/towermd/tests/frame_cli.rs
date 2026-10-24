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

fn with<'a>(args: &[&'a str], from: &[&'a str]) -> Vec<&'a str> {
    [args, from].concat()
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

#[test]
fn delegates_write_cards_by_the_rules_under_the_frame_lease() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let (_, opened) = fixture.request(&endpoint, "/projects/open", "app", Some(json!({"path":fixture.root.path()})));
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let state = || fixture.request(&endpoint, &format!("/projects/{handle}/state"), "app", None).1;
    let place = json!({"x":0,"y":0,"width":960,"height":420});
    let (status, placed) = fixture.request(&endpoint, &format!("/projects/{handle}/frames"), "app", Some(json!({"request_id":request_id(),
        "id":"C-board","title":"看板","template":"kanban","rules":{"done_needs_human":true},"place":place})));
    assert_eq!(status, 200, "{placed}");
    let a = ["--from", "agent-a", "--from-kind", "delegate"];

    // A write within the rules takes effect and names the new card.
    let added = fixture.run(&with(&["frame", "add", "C-board", "--title", "读解析器", "--field", "owner=bruce"], &a));
    assert_eq!((added["card"]["status"].as_str(), added["card"]["card"].as_str()), (Some("applied"), Some("C-board.1")), "{added}");
    assert_eq!(added["envelope"]["verb"], "card");

    // Writes against the rules are refused with the reason, and nothing is stored.
    let long = "长".repeat(31);
    for (args, reason) in [
        (vec!["frame", "add", "C-board", "--title", long.as_str()], "标题"),
        (vec!["frame", "add", "C-board", "--title", "x", "--state", "搁置"], "搁置"),
        (vec!["frame", "add", "C-board", "--title", "x", "--field", "color=red"], "color"),
        (vec!["frame", "add", "C-board", "--title", "x", "--field", "due=tomorrow"], "due"),
        (vec!["frame", "move", "C-board.9", "完成"], "C-board.9"),
    ] {
        let output = fixture.output(&with(&args, &a));
        assert!(!output.status.success(), "{args:?}");
        let message = envelope::parse(&output)["error"]["message"].as_str().unwrap_or_default().to_owned();
        assert!(message.contains(reason), "{args:?}: {message}");
    }
    let frame = &state()["projection"]["Frame"]["C-board"];
    assert_eq!(frame["cards"].as_array().map(Vec::len), Some(1), "{frame}");

    // One writer per frame: another sender waits for the lease.
    let busy = fixture.output(&["frame", "add", "C-board", "--title", "抢写", "--from", "agent-b", "--from-kind", "delegate"]);
    assert_eq!(code(&busy), "lease_busy");
    assert!(state()["projection"]["Frame"]["C-board"]["cards"].as_array().unwrap().iter().all(|card| card["title"] != "抢写"));

    // Moving into a done state that needs a person waits for confirmation.
    assert_eq!(fixture.run(&with(&["frame", "move", "C-board.1", "进行中"], &a))["card"]["status"], "applied");
    let done = fixture.run(&with(&["frame", "move", "C-board.1", "完成"], &a));
    assert_eq!(done["card"]["status"], "awaiting_confirmation", "{done}");
    let nested = fixture.output(&with(&["frame", "add", "C-board", "--title", "第二篇", "--parent", "C-board.1"], &a));
    assert!(envelope::parse(&nested)["error"]["message"].as_str().unwrap().contains("嵌套"), "a kanban has no nesting");
    assert_eq!(fixture.run(&with(&["frame", "add", "C-board", "--title", "第二篇"], &a))["card"]["card"], "C-board.2");
    let linked = fixture.output(&with(&["frame", "link", "C-board.2", "C-board.1"], &a));
    assert!(envelope::parse(&linked)["error"]["message"].as_str().unwrap().contains("关系线"), "a kanban has no links");
    assert_eq!(fixture.run(&with(&["frame", "update", "C-board.2", "--body", "读了一半", "--clear", "owner"], &a))["card"]["status"], "applied");
    let rules = fixture.runtime.path().join("rules.json");
    fs::write(&rules, r#"{"per_lane_max": 3}"#).unwrap();
    let proposed = fixture.run(&with(&["frame", "propose", "C-board", rules.to_str().unwrap()], &a));
    assert_eq!(proposed["card"]["status"], "proposed", "{proposed}");
    assert_eq!(fixture.run(&with(&["frame", "archive", "C-board.2"], &a))["card"]["status"], "applied");

    let shown = fixture.run(&["frame", "show", "C-board"]);
    let cards = shown["cards"].as_array().unwrap();
    assert_eq!((cards[0]["state"].as_str(), cards[1]["archived"].as_bool()), (Some("进行中"), Some(true)), "{shown}");
    assert_eq!((cards[1]["body"].as_str(), cards[1].get("fields")), (Some("读了一半"), None));
    let pending: Vec<&str> = shown["pending"].as_array().unwrap().iter().map(|item| item["why"].as_str().unwrap()).collect();
    assert_eq!(pending, ["confirm", "rules"], "{shown}");
    assert_eq!(shown["frozen"], false);
    assert_eq!(shown["rules"]["per_lane_max"], 7, "a delegate's rule proposal changes nothing by itself");

    // Only the App confirms, decides, writes as a person or changes the rules.
    let route = |name: &str| format!("/projects/{handle}/frames/C-board/{name}");
    let confirm = shown["pending"][0]["id"].clone();
    let (status, _) = fixture.request(&endpoint, &route("resolve"), "cli", Some(json!({"request_id":request_id(),"pending":confirm,"accept":true})));
    assert_eq!(status, 409);
    let (status, _) = fixture.request(&endpoint, &route("cards"), "cli", Some(json!({"request_id":request_id(),"op":{"op":"add","title":"冒充人"}})));
    assert_eq!(status, 409);
    let (status, _) = fixture.request(&endpoint, &route("rules"), "cli", Some(json!({"request_id":request_id(),"rules":shown["rules"]})));
    assert_eq!(status, 409);
    let (status, confirmed) = fixture.request(&endpoint, &route("resolve"), "app", Some(json!({"request_id":request_id(),"pending":confirm,"accept":true})));
    assert_eq!(status, 200, "{confirmed}");
    assert_eq!(confirmed["frame"]["cards"][0]["state"], "完成");
    let (status, mine) = fixture.request(&endpoint, &route("cards"), "app", Some(json!({"request_id":request_id(),"op":{"op":"add","title":"人加的"}})));
    assert_eq!((status, mine["receipt"]["card"].as_str()), (200, Some("C-board.3")), "{mine}");
    assert_eq!(mine["frame"]["cards"][2]["changed"]["actor"], "human:local");
    // Only a person pins a card (FR-149); the pin is all that changes.
    let pin = json!({"request_id":request_id(),"pins":{"C-board.1":{"x":24,"y":80}}});
    let (status, _) = fixture.request(&endpoint, &route("pins"), "cli", Some(pin.clone()));
    assert_eq!(status, 409);
    let (status, pinned) = fixture.request(&endpoint, &route("pins"), "app", Some(pin));
    assert_eq!(status, 200, "{pinned}");
    assert_eq!(pinned["frame"]["pins"]["C-board.1"], json!({"x":24,"y":80}));
    assert_eq!(pinned["frame"]["cards"], mine["frame"]["cards"], "pinning changes no card");
    let (status, _) = fixture.request(&endpoint, &route("pins"), "app", Some(json!({"request_id":request_id(),"pins":{"C-board.7":{"x":1,"y":1}}})));
    assert_eq!(status, 400, "only the frame's cards");
    assert_eq!(fixture.run(&["frame", "show", "C-board"])["pins"]["C-board.1"]["x"], 24);
    let mut changed = shown["rules"].clone();
    changed["update_until"] = json!({"date":"2000-01-01"});
    let (status, frozen) = fixture.request(&endpoint, &route("rules"), "app", Some(json!({"request_id":request_id(),"rules":changed})));
    assert_eq!(status, 200, "{frozen}");
    assert_eq!(fixture.run(&["frame", "show", "C-board"])["frozen"], true);
    let late = fixture.output(&with(&["frame", "add", "C-board", "--title", "过期"], &a));
    assert!(!late.status.success(), "a frozen frame takes no writes");
}

#[test]
fn a_frame_a_person_set_turns_delegate_writes_into_proposals() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let (_, opened) = fixture.request(&endpoint, "/projects/open", "app", Some(json!({"path":fixture.root.path()})));
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let place = json!({"x":0,"y":0,"width":960,"height":420});
    let (status, _) = fixture.request(&endpoint, &format!("/projects/{handle}/frames"), "app", Some(json!({"request_id":request_id(),
        "id":"C-sprint","title":"冲刺","template":"sprint","place":place})));
    assert_eq!(status, 200);
    let proposed = fixture.run(&["frame", "add", "C-sprint", "--title", "提个卡片", "--from", "agent-a", "--from-kind", "delegate"]);
    assert_eq!((proposed["card"]["status"].as_str(), proposed["card"]["card"].as_str()), (Some("proposed"), None), "{proposed}");
    let shown = fixture.run(&["frame", "show", "C-sprint"]);
    assert_eq!(shown["cards"], json!([]), "a proposal is not a card yet");
    let pending = shown["pending"][0]["id"].clone();
    let route = format!("/projects/{handle}/frames/C-sprint/resolve");
    let (status, accepted) = fixture.request(&endpoint, &route, "app", Some(json!({"request_id":request_id(),"pending":pending,"accept":true})));
    assert_eq!(status, 200, "{accepted}");
    assert_eq!(accepted["frame"]["cards"][0]["title"], "提个卡片");
    assert_eq!(accepted["frame"]["pending"], Value::Null, "nothing waits any more: {accepted}");
    let (status, again) = fixture.request(&endpoint, &route, "app", Some(json!({"request_id":request_id(),"pending":pending,"accept":true})));
    assert_eq!(status, 400, "a decision is made once: {again}");
}

#[test]
fn a_person_starts_a_frame_from_pasted_text_and_anyone_reads_it_back_as_text() {
    let fixture = Fixture::new();
    let endpoint = fixture.run(&["daemon", "ensure"]);
    let (_, opened) = fixture.request(&endpoint, "/projects/open", "app", Some(json!({"path":fixture.root.path()})));
    let handle = opened["handle"].as_str().unwrap().to_owned();
    let frames = format!("/projects/{handle}/frames");
    let place = json!({"x":0,"y":0,"width":960,"height":420});
    let text = "# 发布\n\n## 待办\n\n- 写说明\n- 打标签\n\n## 完成\n\n- 冻结分支\n";
    let body = |id: &str, text: &str| json!({"request_id":request_id(),"id":id,"title":"发布","template":"kanban","place":place,"text":text});

    // Text that does not fit the template is refused by line; nothing is created.
    let (status, refused) = fixture.request(&endpoint, &frames, "app", Some(body("C-bad", "## 搁置\n- 翻旧账\n")));
    assert_eq!((status, refused["code"].as_str()), (400, Some("usage")), "{refused}");
    assert!(refused.to_string().contains("第 2 行：「搁置」不是模板「看板」的状态"), "{refused}");
    let (status, refused) = fixture.request(&endpoint, &frames, "app", Some(json!({"request_id":request_id(),"id":"C-bound",
        "title":"发布","template":"kanban","place":place,"mode":"bound","text":text})));
    assert_eq!(status, 400, "only a free frame starts from text: {refused}");
    let (status, refused) = fixture.request(&endpoint, &frames, "cli", Some(body("C-cli", text)));
    assert_eq!(status, 409, "only the App imports: {refused}");

    let (status, placed) = fixture.request(&endpoint, &frames, "app", Some(body("C-release", text)));
    assert_eq!(status, 200, "{placed}");
    let cards = placed["frame"]["cards"].as_array().unwrap();
    let read: Vec<(&str, &str, &str)> = cards.iter().map(|card| (card["id"].as_str().unwrap(), card["title"].as_str().unwrap(), card["state"].as_str().unwrap())).collect();
    assert_eq!(read, [("C-release.1", "写说明", "待办"), ("C-release.2", "打标签", "待办"), ("C-release.3", "冻结分支", "完成")]);
    assert_eq!(cards[0]["added"]["actor"], "human:local", "imported cards are the person's own writes");

    // Export is derived on read; the CLI and any client may read it.
    let exported = fixture.run(&["frame", "export", "C-release", "--format", "markdown"]);
    assert_eq!((exported["text"].as_str(), exported["formats"].clone()), (Some(text), json!(["mermaid", "markdown"])), "{exported}");
    let mermaid = fixture.run(&["frame", "export", "C-release"]);
    assert_eq!(mermaid["format"], "mermaid", "the template's declared order picks the default");
    assert!(mermaid["text"].as_str().unwrap().starts_with("kanban\n  lane1[待办]\n    n1[写说明]\n"), "{mermaid}");
    let (status, missing) = fixture.request(&endpoint, &format!("{frames}/C-none/export"), "cli", None);
    assert_eq!((status, missing["code"].as_str()), (400, Some("not_found")), "{missing}");
}
