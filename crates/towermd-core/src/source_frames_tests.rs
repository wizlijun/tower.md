use super::*;
use crate::{
    cards::{lease_name, RESOLVED, SOURCE},
    frames::{RulesInput, Template, CREATED},
    hub::Fence,
};
use serde_json::json;

const FRAME: &str = "C-plan";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const FILE: &str = "@startuml\nrectangle \"Parser\" as P <<todo>>\nrectangle \"Writer\" as W <<doing>>\nP --> W\nnote right of P <<question>>\n  which subset?\nend note\n@enduml\n";

fn definition() -> Value {
    json!({"id":"plantuml","version":"1.0.0","type":"template","source":"file","parser":plantuml::PARSER,
        "file_types":[".puml",".plantuml"],"layout":{"structure":["nesting","relations"]},
        "states":["To Do","In Progress","Done"],"done_states":["Done"],
        "status_map":{"<<todo>>":"To Do","<<doing>>":"In Progress","<<done>>":"Done","#lightgreen":"Done"},
        "callout_map":{"<<why>>":"why","<<adjust>>":"adjust","<<question>>":"question"},"default_set_by":"delegate"})
}

fn created(definition: Value, set_by: &str, mode: &str, path: Option<&str>, rules: RulesInput) -> Value {
    let template = Template::parse(&definition).unwrap();
    let mut body = json!({"title":"计划","template":{"id":template.id,"version":template.version,"definition":definition},
        "set_by":set_by,"mode":mode,"rules":template.rules(rules).unwrap(),"place":{"x":0,"y":0,"width":880,"height":420}});
    if let Some(path) = path {
        body["source"] = json!({"path": path});
    }
    body
}

struct Log {
    state: Value,
    seq: u64,
}

impl Log {
    fn new(set_by: &str, rules: RulesInput) -> Self {
        let mut log = Log { state: json!({}), seq: 0 };
        log.event(CREATED, "human:local", frames::LAYER, created(definition(), set_by, "free", Some("docs/plan.puml"), rules), vec![], None).unwrap();
        log
    }
    fn event(&mut self, kind: &str, actor: &str, layer: &str, data: Value, guards: Vec<Fence>, epoch: Option<u64>) -> Result<bool, String> {
        self.seq += 1;
        let event: Event = serde_json::from_value(json!({"seq":self.seq,"eid":format!("e{}",self.seq),"host":"h",
            "t":"2026-10-24T01:00:00Z","actor":actor,"type":kind,"layer":layer,"obj":FRAME,"data":data,"guards":guards,
            "epoch":epoch,"stale":false,"cost_usd":null})).unwrap();
        let mut next = self.state.clone();
        let result = frames::apply(&mut next, &event, &[]);
        if result.is_ok() {
            self.state = next;
        }
        result
    }
    fn person(&mut self, change: Value) -> Result<bool, String> {
        self.event(SOURCE, "human:local", crate::cards::LAYER, json!({"op":"source","change":change}), vec![], None)
    }
    fn agent(&mut self, op: Value) -> Result<bool, String> {
        let from = "claude-code/2.0";
        let envelope = json!({"v":1,"project":"p","from":{"id":from,"kind":"delegate"},"target":FRAME,"verb":"card",
            "data":op.clone(),"at":"2026-10-24T10:00:00+09:00"});
        let kind = serde_json::from_value::<Op>(op).map_err(|e| e.to_string())?.kind();
        let fence = Fence { name: lease_name(FRAME), holder: from.into(), epoch: 1 };
        self.event(kind, "process:towermd-cli", crate::cards::LAYER, envelope, vec![fence], None)
    }
    fn receipt(&mut self, data: Value) -> Result<bool, String> {
        let fence = Fence { name: "main".into(), holder: "h".into(), epoch: 3 };
        self.event(WRITTEN, WRITER, frames::LAYER, data, vec![fence], Some(3))
    }
    fn frame(&self) -> Frame {
        frames::read(&self.state).unwrap().remove(FRAME).unwrap()
    }
}

fn spec_of() -> Spec {
    Spec::read(&definition(), &["To Do".into(), "In Progress".into(), "Done".into()]).unwrap().unwrap()
}

fn no_include() -> impl FnMut(&str) -> Option<Vec<u8>> {
    |_: &str| None
}

#[test]
fn a_source_template_names_a_known_parser_and_maps_to_its_states() {
    let spec = spec_of();
    assert_eq!(spec.token("Done"), Some("#lightgreen"), "the first written token in order writes the state");
    assert_eq!(spec.note_token(NoteKind::Question), Some("<<question>>"));
    let states = ["To Do".to_owned()];
    assert_eq!(Spec::read(&json!({"id":"k"}), &states), Ok(None));
    for (patch, problem) in [
        (json!({"parser":"java:plantuml"}), "parser 须是"),
        (json!({"file_types":["puml"]}), "file_types"),
        (json!({"status_map":{"<<x>>":"Nope"}}), "status_map"),
        (json!({"callout_map":{"#red":"why"}}), "callout_map"),
        (json!({"source":"url"}), "source 只能是"),
    ] {
        let mut definition = definition();
        for (key, value) in patch.as_object().unwrap() {
            definition[key] = value.clone();
        }
        let problems = Template::parse(&definition).unwrap_err();
        assert!(problems.iter().any(|p| p.contains(problem)), "{problem}: {problems:?}");
    }
    let stray = json!({"id":"k","version":"1","type":"template","layout":{},"parser":plantuml::PARSER});
    assert!(Template::parse(&stray).unwrap_err().iter().any(|p| p.contains("只用于 source: file")));
}

#[test]
fn a_source_frame_is_free_and_names_a_plain_repository_file() {
    let mut log = Log { state: json!({}), seq: 0 };
    let rules = RulesInput::default;
    for (mode, path, problem) in [
        ("free", None, "须指明源文件"),
        ("mixed", Some("plan.puml"), "只能是自由容器"),
        ("free", Some("../plan.puml"), "相对路径"),
        ("free", Some("/abs/plan.puml"), "相对路径"),
        ("free", Some("plan.md"), "源文件的类型"),
        ("free", Some(".towermd/x.puml"), ".towermd/"),
    ] {
        let err = log.event(CREATED, "human:local", frames::LAYER, created(definition(), "human", mode, path, rules()), vec![], None).unwrap_err();
        assert!(err.contains(problem), "{path:?}: {err}");
    }
    let plain = json!({"id":"k","version":"1","type":"template","layout":{}});
    let err = log.event(CREATED, "human:local", frames::LAYER, created(plain, "human", "free", Some("a.puml"), rules()), vec![], None).unwrap_err();
    assert!(err.contains("只有 source: file"), "{err}");
    // FR-144 meets FR-163: a source frame's cards come from its file, never from pasted text.
    let mut pasted = created(definition(), "human", "free", Some("plan.puml"), rules());
    pasted["cards"] = json!([{"op":{"op":"add","title":"x"}}]);
    let err = log.event(CREATED, "human:local", frames::LAYER, pasted, vec![], None).unwrap_err();
    assert!(err.contains("来自源文件"), "{err}");
    let mut log = Log::new("delegate", rules());
    assert_eq!(log.frame().source, Some(Source { path: "docs/plan.puml".into() }));
    let err = log.person(json!({"do":"state","element":"P","state":"Done"})).map(|_| ()).and_then(|_| {
        log.event(crate::cards::ADDED, "human:local", crate::cards::LAYER, json!({"op":"add","title":"x"}), vec![], None).map(|_| ())
    }).unwrap_err();
    assert!(err.contains("frame edit"), "ordinary card ops are refused: {err}");
}

#[test]
fn edits_wait_as_writes_and_a_delegate_on_a_persons_frame_proposes() {
    let mut log = Log::new("delegate", RulesInput::default());
    log.person(json!({"do":"state","element":"P","state":"Done"})).unwrap();
    log.agent(json!({"op":"source","change":{"do":"add","title":"Reader","parent":"P","state":"To Do"}})).unwrap();
    let whys: Vec<Why> = log.frame().pending.iter().map(|item| item.why).collect();
    assert_eq!(whys, [Why::Write, Why::Write], "writes are never merged away");
    let err = log.agent(json!({"op":"source","change":{"do":"state","element":"P","state":"Done"}})).unwrap();
    assert!(err);
    assert_eq!(log.frame().pending.last().unwrap().why, Why::Confirm, "FR-152: done needs a person");
    let err = log.agent(json!({"op":"source","change":{"do":"init"}})).unwrap_err();
    assert!(err.contains("只允许人"), "{err}");
    let err = log.person(json!({"do":"state","element":"P","state":"Blocked"})).unwrap_err();
    assert!(err.contains("不在模板的状态集合"), "{err}");

    let mut log = Log::new("human", RulesInput { add_link: Some(frames::Who::Human), ..Default::default() });
    log.agent(json!({"op":"source","change":{"do":"title","element":"P","title":"Parser v2"}})).unwrap();
    let item = log.frame().pending[0].clone();
    assert_eq!(item.why, Why::Proposal, "FR-166");
    let err = log.agent(json!({"op":"source","change":{"do":"link","from":"P","to":"W"}})).unwrap_err();
    assert!(err.contains("只允许人"), "{err}");
    log.event(RESOLVED, "human:local", frames::LAYER, json!({"pending":item.id,"accept":true}), vec![], None).unwrap();
    let pending = log.frame().pending;
    assert_eq!((pending.len(), pending[0].why, pending[0].id), (1, Why::Write, item.id), "accepting queues the write");
}

#[test]
fn only_the_main_writer_records_a_write_and_a_refused_one_stays_dashed() {
    let mut log = Log::new("delegate", RulesInput::default());
    log.person(json!({"do":"state","element":"P","state":"Done"})).unwrap();
    log.person(json!({"do":"note","element":"Q","kind":"why","text":"because"})).unwrap();
    let ids: Vec<u64> = log.frame().pending.iter().map(|item| item.id).collect();
    let sha = sha256(b"x");
    let written = json!({"pending":ids[0],"path":"docs/plan.puml","commit":COMMIT,"sha256":sha});
    let fence = Fence { name: "main".into(), holder: "h".into(), epoch: 3 };
    for (actor, guards, epoch) in [
        ("human:local", vec![fence.clone()], Some(3)),
        (WRITER, vec![], Some(3)),
        (WRITER, vec![fence.clone()], Some(2)),
        (WRITER, vec![Fence { name: "frame:C-plan".into(), ..fence.clone() }], Some(3)),
    ] {
        assert!(log.event(WRITTEN, actor, frames::LAYER, written.clone(), guards, epoch).is_err(), "{actor}");
    }
    assert!(log.receipt(json!({"pending":ids[0],"path":"docs/other.puml","sha256":sha})).is_err());
    assert!(log.receipt(json!({"pending":ids[0],"path":"docs/plan.puml","commit":"main","sha256":sha})).is_err());
    log.receipt(written.clone()).unwrap();
    let frame = log.frame();
    assert_eq!(frame.pending.len(), 1);
    assert_eq!(frame.source_write.as_ref().map(|r| (r.commit.as_deref(), r.sha256.as_str())), Some((Some(COMMIT), sha.as_str())));
    assert!(log.receipt(written).is_err(), "a write is recorded once");

    log.receipt(json!({"pending":ids[1],"path":"docs/plan.puml","failed":"元素 Q 不存在"})).unwrap();
    let item = log.frame().pending[0].clone();
    assert_eq!(item.note.as_deref(), Some("元素 Q 不存在"));
    assert!(log.receipt(json!({"pending":ids[1],"path":"docs/plan.puml","sha256":sha})).is_err(), "a refused write waits for a person");
    log.event(RESOLVED, "human:local", frames::LAYER, json!({"pending":item.id,"accept":true}), vec![], None).unwrap();
    assert_eq!(log.frame().pending[0].note, None, "redo queues it again");
    log.event(RESOLVED, "human:local", frames::LAYER, json!({"pending":item.id,"accept":false}), vec![], None).unwrap();
    assert!(log.frame().pending.is_empty());
}

#[test]
fn only_a_person_relinks_and_a_source_frame_pins_elements_by_key() {
    let mut log = Log::new("delegate", RulesInput::default());
    assert!(log.event(RELINKED, "process:towermd-cli", frames::LAYER, json!({"path":"docs/moved.puml"}), vec![], None).is_err());
    assert!(log.event(RELINKED, "human:local", frames::LAYER, json!({"path":"docs/moved.txt"}), vec![], None).is_err());
    assert!(log.event(RELINKED, "human:local", frames::LAYER, json!({"path":"docs/plan.puml"}), vec![], None).is_err());
    log.event(RELINKED, "human:local", frames::LAYER, json!({"path":"docs/moved.puml"}), vec![], None).unwrap();
    assert_eq!(log.frame().source.unwrap().path, "docs/moved.puml");
    log.event(crate::cards::PINNED, "human:local", frames::LAYER, json!({"pins":{"P":{"x":10,"y":20}}}), vec![], None).unwrap();
    assert_eq!(log.frame().pins["P"], frames::Pin { x: 10, y: 20 });
}

#[test]
fn writes_patch_the_file_and_init_or_stale_fixes_are_handled() {
    let spec = spec_of();
    let change = Change::State { element: "P".into(), state: "In Progress".into() };
    let out = write(&spec, "a.puml", Some(FILE.as_bytes()), &change, &mut no_include()).unwrap().unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("rectangle \"Parser\" as P <<doing>>"), "{text}");
    let changed = FILE.lines().zip(text.lines()).filter(|(a, b)| a != b).count();
    assert_eq!(changed, 1, "one line changes");
    let add = Change::Add { title: "Reader".into(), alias: None, parent: None, state: Some("To Do".into()) };
    let text = String::from_utf8(write(&spec, "a.puml", Some(FILE.as_bytes()), &add, &mut no_include()).unwrap().unwrap()).unwrap();
    assert!(text.contains("\"Reader\"") && text.contains("<<todo>>"), "{text}");
    let missing = Change::State { element: "Z".into(), state: "Done".into() };
    assert!(write(&spec, "a.puml", Some(FILE.as_bytes()), &missing, &mut no_include()).is_err());
    assert!(write(&spec, "a.puml", None, &change, &mut no_include()).unwrap_err().contains("不存在"));
    assert_eq!(write(&spec, "a.puml", None, &Change::Init {}, &mut no_include()).unwrap().unwrap(), b"@startuml\n@enduml\n");
    assert_eq!(write(&spec, "a.puml", Some(b"x"), &Change::Init {}, &mut no_include()).unwrap(), None);
    let fix = Change::Fix { fix: "end".into(), sha256: sha256(b"other") };
    assert!(write(&spec, "a.puml", Some(b"@startuml\nA --> B\n"), &fix, &mut no_include()).unwrap_err().contains("失效"));
    let broken = b"@startuml\nA --> B\n";
    let fix = Change::Fix { fix: "end".into(), sha256: sha256(broken) };
    let fixed = write(&spec, "a.puml", Some(broken), &fix, &mut no_include()).unwrap().unwrap();
    assert_eq!(plantuml::parse("a.puml", &fixed, &mut no_include()).status, Status::Ok);
}

#[test]
fn the_view_maps_states_and_notes_and_keeps_the_last_good_reading() {
    let log = Log::new("delegate", RulesInput::default());
    let frame = log.frame();
    let spec = spec_of();
    let read = |revision: &str, text: &str| Read {
        revision: revision.into(), bytes: text.as_bytes().to_vec(), parsed: plantuml::parse("docs/plan.puml", text.as_bytes(), &mut no_include()),
    };
    let view = super::view(&frame, &spec, Some(read("c1", FILE)), None, None, None, false);
    assert_eq!(view.status, FileStatus::Ok);
    assert!(!view.locked);
    let states: Vec<_> = view.cards.iter().map(|c| (c.key.as_str(), c.state.as_deref(), c.links.clone())).collect();
    assert_eq!(states, [("P", Some("To Do"), vec!["W".to_owned()]), ("W", Some("In Progress"), vec![])]);
    assert_eq!(view.callouts[0].kind, NoteKind::Question);
    assert_eq!(view.understood, 100);
    assert!(view.lint.is_empty(), "{:?}", view.lint);

    let broken = "@startuml\nrectangle \"Parser\" as P\n";
    let view = super::view(&frame, &spec, Some(read("c2", broken)), Some(read("c1", FILE)), None, Some(30), false);
    assert_eq!((view.status, view.locked, view.shown_from.as_deref()), (FileStatus::Unparsable, true, Some("c1")));
    assert_eq!(view.cards.len(), 2, "FR-175: the last good reading stays");
    assert!(!view.fixes.is_empty());
    assert_eq!(view.signal.as_ref().map(|s| s.level), Some("passive"), "FR-177: passive without bound work");
    let view = super::view(&frame, &spec, Some(read("c2", broken)), None, None, Some(30), true);
    assert_eq!(view.signal.as_ref().map(|s| s.level), Some("active"));
    let view = super::view(&frame, &spec, Some(read("c2", broken)), None, None, Some(2), true);
    assert_eq!(view.signal.as_ref().map(|s| s.level), Some("passive"));
    let view = super::view(&frame, &spec, None, None, Some("docs/new.puml".into()), None, false);
    assert_eq!((view.status, view.renamed_to.as_deref()), (FileStatus::Missing, Some("docs/new.puml")));
}

#[test]
fn the_check_and_history_speak_only_of_the_model() {
    let spec = spec_of();
    let text = "@startuml\nrectangle Plain <<urgent>>\nleft to right direction\nnote right of Plain\nlong\nend note\n@enduml\n";
    let parsed = plantuml::parse("a.puml", text.as_bytes(), &mut no_include());
    let codes: Vec<&str> = lint(&spec, &parsed, text.len()).iter().map(|l| l.code).collect();
    assert_eq!(codes, ["no_alias", "unmapped_status", "layout_hint"]);
    let frame = Log::new("delegate", RulesInput::default()).frame();
    let before = shown(&frame, &spec, &plantuml::parse("a.puml", FILE.as_bytes(), &mut no_include()));
    let next = FILE.replace("<<doing>>", "<<done>>").replace("P --> W\n", "P --> W\nrectangle \"Reader\" as R\n");
    let after = shown(&frame, &spec, &plantuml::parse("a.puml", next.as_bytes(), &mut no_include()));
    let summary = summary(Some(&before), &after);
    assert_eq!(summary.added, ["Reader"]);
    assert_eq!(summary.states, [("Writer".to_owned(), Some("In Progress".to_owned()), Some("Done".to_owned()))]);
    assert!(summary.text.contains("新增元素 1 个"), "{}", summary.text);
    assert_eq!(super::summary(Some(&after), &after).text, "没有模型上的变化");
}
