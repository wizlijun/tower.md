use super::*;
use crate::{frames::{Rules, Template, CREATED}, hub::Fence};
use serde_json::json;

const KANBAN: &str = "C-board";

fn kanban() -> Value {
    json!({"id":"kanban","version":"1.0.0","type":"template",
        "layout":{"x":{"category":"state"},"y":{"order":"added"},"structure":"none"},
        "states":["To Do","Doing","Review","Done"],"transitions":{"Review":["Done","Doing"]},"done_states":["Done"],
        "card_fields":{"owner":"text","due":"date","source":"ref"},"limits":{"title_max":30,"body_max":100,"per_lane_max":2},
        "default_set_by":"delegate"})
}

fn graph() -> Value {
    json!({"id":"graph","version":"1","type":"template","layout":{"structure":["relations","nesting"]},
        "card_fields":{"source":"ref"},"default_set_by":"human"})
}

fn quadrant() -> Value {
    json!({"id":"quadrant","version":"1","type":"template",
        "layout":{"x":{"category":"urgency","values":["urgent","later"]},"y":{"time":"due"}},
        "card_fields":{"urgency":"text","due":"date"}})
}

fn created(definition: Value, set_by: &str, mode: &str, rules: RulesInput) -> Value {
    let template = Template::parse(&definition).unwrap();
    json!({"title":"板","template":{"id":template.id,"version":template.version,"definition":definition},
        "set_by":set_by,"mode":mode,"rules":template.rules(rules).unwrap(),"place":{"x":0,"y":0,"width":880,"height":420}})
}

struct Log {
    state: Value,
    seq: u64,
}

impl Log {
    fn new(definition: Value, set_by: &str, rules: RulesInput) -> Self {
        Self::with(definition, set_by, "free", rules)
    }
    fn with(definition: Value, set_by: &str, mode: &str, rules: RulesInput) -> Self {
        let mut log = Log { state: json!({}), seq: 0 };
        log.event(CREATED, "human:local", KANBAN, frames::LAYER, created(definition, set_by, mode, rules), vec![]).unwrap();
        log
    }
    fn event(&mut self, kind: &str, actor: &str, obj: &str, layer: &str, data: Value, guards: Vec<Fence>) -> Result<bool, String> {
        self.seq += 1;
        let event: Event = serde_json::from_value(json!({"seq":self.seq,"eid":format!("e{}",self.seq),"host":"h",
            "t":"2026-10-24T01:00:00Z","actor":actor,"type":kind,"layer":layer,"obj":obj,"data":data,"guards":guards,
            "epoch":null,"stale":false,"cost_usd":null})).unwrap();
        let mut next = self.state.clone();
        let result = frames::apply(&mut next, &event, &[]);
        if result.is_ok() {
            self.state = next;
        }
        result
    }
    /// A delegate's `card` envelope through the CLI entry, holding the lease.
    fn agent(&mut self, target: &str, op: Value) -> Result<bool, String> {
        self.agent_as("claude-code/2.0", target, op)
    }
    fn agent_as(&mut self, from: &str, target: &str, op: Value) -> Result<bool, String> {
        let parsed: Op = serde_json::from_value(op.clone()).map_err(|e| e.to_string())?;
        let frame = frame_of(&parsed, target)?.to_owned();
        let envelope = json!({"v":1,"project":"p","from":{"id":from,"kind":"delegate"},"target":target,"verb":"card",
            "data":op,"at":"2026-10-24T10:00:00+09:00"});
        let fence = Fence { name: lease_name(&frame), holder: from.into(), epoch: 1 };
        self.event(parsed.kind(), "process:towermd-cli", target, LAYER, envelope, vec![fence])
    }
    fn person(&mut self, target: &str, op: Value) -> Result<bool, String> {
        let kind = serde_json::from_value::<Op>(op.clone()).map_err(|e| e.to_string())?.kind();
        self.event(kind, "human:local", target, LAYER, op, vec![])
    }
    fn resolve(&mut self, pending: u64, accept: bool) -> Result<bool, String> {
        self.event(RESOLVED, "human:local", KANBAN, frames::LAYER, json!({"pending":pending,"accept":accept}), vec![])
    }
    fn frame(&self) -> Frame {
        frames::read(&self.state).unwrap()[KANBAN].clone()
    }
    fn card(&self, id: &str) -> Card {
        self.frame().cards.into_iter().find(|card| card.id == id).unwrap()
    }
}

fn add(title: &str) -> Value {
    json!({"op":"add","title":title})
}

#[test]
fn a_delegate_adds_cards_in_order_and_the_card_records_who_wrote_it() {
    let mut log = Log::new(kanban(), "delegate", RulesInput::default());
    log.agent(KANBAN, json!({"op":"add","title":"读 RAFT","body":"一致性","fields":{"owner":"amy","due":"2026-11-01"}})).unwrap();
    log.agent(KANBAN, add("读 Paxos")).unwrap();
    let frame = log.frame();
    assert_eq!(frame.cards.iter().map(|card| card.id.as_str()).collect::<Vec<_>>(), ["C-board.1", "C-board.2"]);
    let card = &frame.cards[0];
    assert_eq!(card.state.as_deref(), Some("To Do"), "a card starts in the first open state");
    assert_eq!((card.by.as_deref(), card.changed.actor.as_str()), (Some("claude-code/2.0"), "process:towermd-cli"));
    assert!(card.human.is_empty());
    let round: Frames = serde_json::from_value(log.state[GROUP].clone()).unwrap();
    assert_eq!(serde_json::to_value(&round).unwrap(), log.state[GROUP]);
}

#[test]
fn every_non_compliant_write_is_rejected_with_its_reason_and_nothing_is_stored() {
    let rules = RulesInput { require_source: Some(true), add_link: Some(Who::Human), ..Default::default() };
    let mut log = Log::new(kanban(), "delegate", rules);
    log.agent(KANBAN, json!({"op":"add","title":"ok","fields":{"source":"notes/a.md"}})).unwrap();
    let before = log.state.clone();
    let cases = [
        (KANBAN, json!({"op":"add","title":"字".repeat(31),"fields":{"source":"s"}}), "标题不能超过 30 字"),
        (KANBAN, json!({"op":"add","title":"t","body":"字".repeat(101),"fields":{"source":"s"}}), "正文不能超过 100 字"),
        (KANBAN, json!({"op":"add","title":"t","state":"Lost","fields":{"source":"s"}}), "不在模板的状态集合里"),
        (KANBAN, json!({"op":"add","title":"t","fields":{"mood":"x","source":"s"}}), "不是这个容器可用的卡片字段"),
        (KANBAN, json!({"op":"add","title":"t","fields":{"due":"tomorrow","source":"s"}}), "YYYY-MM-DD"),
        (KANBAN, json!({"op":"add","title":"t"}), "要求填来源"),
        (KANBAN, json!({"op":"add","title":"t","state":"Done","fields":{"source":"s"}}), "须由人确认"),
        (KANBAN, json!({"op":"add","title":"t","parent":"C-board.1","fields":{"source":"s"}}), "不是嵌套或树"),
        ("C-board.1", json!({"op":"update","fields":{"source":null}}), "不能去掉 ref 字段"),
        ("C-board.1", json!({"op":"update"}), "没有要改的内容"),
        ("C-board.1", json!({"op":"move","state":"To Do"}), "已经是"),
        ("C-board.9", json!({"op":"move","state":"Doing"}), "不存在"),
        ("T-1", json!({"op":"move","state":"Doing"}), "绑定卡片跟随核心对象"),
        ("C-board.1", json!({"op":"link","to":"C-board.1"}), "只允许人加连线"),
        ("C-board.1", json!({"op":"rules","rules":{}}), "C-"),
    ];
    for (target, op, reason) in cases {
        let error = log.agent(target, op.clone()).unwrap_err();
        assert!(error.contains(reason), "{op}: {error}");
        assert_eq!(log.state, before, "{op} stored something");
    }
    assert!(log.agent(KANBAN, json!({"op":"add","title":"t","colour":"red"})).unwrap_err().contains("unknown field"));
}

#[test]
fn transitions_and_element_permissions_bind_delegates_but_not_people() {
    let rules = RulesInput { change_state: Some(Who::Anyone), done_needs_human: Some(false), ..Default::default() };
    let mut log = Log::new(kanban(), "delegate", rules);
    log.agent(KANBAN, add("a")).unwrap();
    log.agent("C-board.1", json!({"op":"move","state":"Review"})).unwrap();
    let error = log.agent("C-board.1", json!({"op":"move","state":"To Do"})).unwrap_err();
    assert!(error.contains("不允许从 Review 转到 To Do"), "{error}");
    log.agent("C-board.1", json!({"op":"move","state":"Done"})).unwrap();
    assert_eq!(log.card("C-board.1").state.as_deref(), Some("Done"), "without done_needs_human a free card may finish");
    let mut log = Log::new(kanban(), "delegate", RulesInput { add_card: Some(Who::Human), ..Default::default() });
    assert_eq!(log.agent(KANBAN, add("a")).unwrap_err(), "容器规则只允许人加卡片");
    log.person(KANBAN, add("a")).unwrap();
    assert_eq!(log.agent("C-board.1", json!({"op":"archive"})).unwrap_err(), "容器规则只允许人归档卡片");
    log.person("C-board.1", json!({"op":"move","state":"Review"})).unwrap();
    log.person("C-board.1", json!({"op":"move","state":"To Do"})).unwrap();
    assert_eq!(log.card("C-board.1").state.as_deref(), Some("To Do"), "a person moves freely within the states");
}

#[test]
fn a_delegate_write_needs_the_frame_lease_it_holds() {
    let mut log = Log::new(kanban(), "delegate", RulesInput::default());
    let envelope = json!({"v":1,"project":"p","from":{"id":"a/1","kind":"delegate"},"target":KANBAN,"verb":"card",
        "data":add("t"),"at":"2026-10-24T10:00:00Z"});
    let error = log.event(ADDED, "process:towermd-cli", KANBAN, LAYER, envelope.clone(), vec![]).unwrap_err();
    assert_eq!(error, "写卡片须持有容器租约 frame:C-board");
    let other = Fence { name: lease_name(KANBAN), holder: "b/1".into(), epoch: 3 };
    assert!(log.event(ADDED, "process:towermd-cli", KANBAN, LAYER, envelope.clone(), vec![other]).is_err());
    let wrong = Fence { name: lease_name("C-other"), holder: "a/1".into(), epoch: 3 };
    assert!(log.event(ADDED, "process:towermd-cli", KANBAN, LAYER, envelope.clone(), vec![wrong]).is_err());
    assert!(log.event(UPDATED, "process:towermd-cli", KANBAN, LAYER, envelope.clone(), vec![]).is_err(), "kind must match the op");
    assert!(log.event(ADDED, "human:local", KANBAN, LAYER, envelope, vec![]).is_err(), "a person sends the op, not an envelope");
}

#[test]
fn a_person_set_frame_turns_every_delegate_write_into_a_dashed_proposal() {
    let mut log = Log::new(graph(), "human", RulesInput::default());
    log.person(KANBAN, add("API")).unwrap();
    log.agent(KANBAN, add("DB")).unwrap();
    log.agent("C-board.1", json!({"op":"update","title":"Gateway"})).unwrap();
    let frame = log.frame();
    assert_eq!(frame.cards.len(), 1, "nothing a delegate wrote took effect");
    assert_eq!(frame.cards[0].title, "API");
    assert_eq!(frame.pending.iter().map(|item| item.why).collect::<Vec<_>>(), [Why::Proposal, Why::Proposal]);
    assert!(log.agent(KANBAN, json!({"op":"add","title":"x","state":"Done"})).is_err(), "proposals are checked too");
    let add_id = frame.pending[0].id;
    log.resolve(add_id, true).unwrap();
    log.resolve(frame.pending[1].id, false).unwrap();
    let frame = log.frame();
    assert!(frame.pending.is_empty());
    assert_eq!(frame.cards.iter().map(|card| card.title.as_str()).collect::<Vec<_>>(), ["API", "DB"]);
    assert_eq!(frame.cards[1].changed.actor, "human:local", "the person's approval made it");
    assert_eq!(log.resolve(add_id, true).unwrap_err(), "待处理项不存在或已处理");
    assert!(log.event(RESOLVED, "process:towermd-cli", KANBAN, frames::LAYER, json!({"pending":1,"accept":true}), vec![]).is_err());
}

#[test]
fn a_delegate_only_suggests_changes_to_what_a_person_set() {
    let mut log = Log::new(kanban(), "delegate", RulesInput::default());
    log.agent(KANBAN, json!({"op":"add","title":"draft","fields":{"owner":"bot"}})).unwrap();
    log.person("C-board.1", json!({"op":"update","title":"Final title"})).unwrap();
    assert!(log.card("C-board.1").human.contains("title"));
    log.agent("C-board.1", json!({"op":"update","title":"Bot title","fields":{"owner":"amy"}})).unwrap();
    let card = log.card("C-board.1");
    assert_eq!(card.title, "Final title", "the person's title stays");
    assert_eq!(card.fields["owner"], "amy", "the unprotected part applies");
    let frame = log.frame();
    assert_eq!(frame.pending.len(), 1);
    assert_eq!(frame.pending[0].why, Why::Suggestion);
    assert_eq!(frame.pending[0].op, Op::Update { title: Some("Bot title".into()), body: None, fields: BTreeMap::new() });
    let outcome = outcome(&log.state, KANBAN, log.seq).unwrap();
    assert_eq!(outcome["status"], "partly_suggested");
    log.agent("C-board.1", json!({"op":"update","title":"Bot title 2"})).unwrap();
    assert_eq!(log.frame().pending.len(), 1, "a newer suggestion replaces the older one");
    log.resolve(log.frame().pending[0].id, true).unwrap();
    assert_eq!(log.card("C-board.1").title, "Bot title 2");
    log.person("C-board.1", json!({"op":"move","state":"Doing"})).unwrap();
    log.agent("C-board.1", json!({"op":"move","state":"Review"})).unwrap();
    assert_eq!(log.card("C-board.1").state.as_deref(), Some("Doing"));
    assert_eq!(log.frame().pending[0].why, Why::Suggestion);
}

#[test]
fn a_free_card_enters_a_done_state_only_with_a_person_when_the_rules_say_so() {
    let mut log = Log::new(kanban(), "delegate", RulesInput::default());
    log.agent(KANBAN, add("a")).unwrap();
    log.agent("C-board.1", json!({"op":"move","state":"Review"})).unwrap();
    log.agent("C-board.1", json!({"op":"move","state":"Done"})).unwrap();
    assert_eq!(log.card("C-board.1").state.as_deref(), Some("Review"), "a claim is not completion");
    assert_eq!(outcome(&log.state, KANBAN, log.seq).unwrap()["status"], "awaiting_confirmation");
    let pending = log.frame().pending[0].id;
    log.resolve(pending, true).unwrap();
    assert_eq!(log.card("C-board.1").state.as_deref(), Some("Done"));
}

#[test]
fn over_the_lane_limit_a_write_lands_and_the_writer_is_told_to_merge_or_archive() {
    let mut log = Log::new(kanban(), "delegate", RulesInput::default());
    for title in ["a", "b"] {
        log.agent(KANBAN, add(title)).unwrap();
        assert_eq!(outcome(&log.state, KANBAN, log.seq).unwrap()["hint"], Value::Null);
    }
    log.agent(KANBAN, add("c")).unwrap();
    let receipt = outcome(&log.state, KANBAN, log.seq).unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["card"].as_str()), (Some("applied"), Some("C-board.3")));
    assert!(receipt["hint"].as_str().unwrap().contains("「To Do」已有 3 张卡片，超过上限 2"), "{receipt}");
    log.agent("C-board.3", json!({"op":"archive"})).unwrap();
    assert!(log.card("C-board.3").archived);
    assert!(log.agent("C-board.3", json!({"op":"update","title":"x"})).unwrap_err().contains("已归档"));
}

#[test]
fn links_and_nesting_follow_the_structure() {
    let mut log = Log::new(graph(), "delegate", RulesInput::default());
    log.agent(KANBAN, add("a")).unwrap();
    log.agent(KANBAN, json!({"op":"add","title":"b","parent":"C-board.1"})).unwrap();
    log.agent("C-board.2", json!({"op":"link","to":"C-board.1"})).unwrap();
    assert_eq!(log.card("C-board.2").links, ["C-board.1"]);
    assert!(log.agent("C-board.2", json!({"op":"link","to":"C-board.1"})).unwrap_err().contains("已经连到"));
    assert!(log.agent("C-board.2", json!({"op":"link","to":"C-board.7"})).unwrap_err().contains("不存在"));
    // FR-141: a line may cross frame borders; it is stored on the card it starts from.
    log.event(CREATED, "human:local", "C-other", frames::LAYER, created(kanban(), "delegate", "free", RulesInput::default()), vec![]).unwrap();
    log.person("C-other", add("elsewhere")).unwrap();
    log.agent("C-board.2", json!({"op":"link","to":"C-other.1"})).unwrap();
    assert_eq!(log.card("C-board.2").links, ["C-board.1", "C-other.1"]);
    assert!(log.agent("C-board.1", json!({"op":"archive"})).unwrap_err().contains("未归档的卡片"));
    assert!(log.agent(KANBAN, json!({"op":"add","title":"c","state":"x"})).unwrap_err().contains("没有状态"));
}

#[test]
fn axis_fields_place_the_card_and_take_only_their_values() {
    let mut log = Log::new(quadrant(), "delegate", RulesInput::default());
    assert!(log.agent(KANBAN, json!({"op":"add","title":"a","fields":{"due":"2026-11-01"}})).unwrap_err().contains("须填字段 urgency"));
    let error = log.agent(KANBAN, json!({"op":"add","title":"a","fields":{"urgency":"now","due":"2026-11-01"}})).unwrap_err();
    assert!(error.contains("只能取 urgent、later"), "{error}");
    log.agent(KANBAN, json!({"op":"add","title":"a","fields":{"urgency":"urgent","due":"2026-11-01"}})).unwrap();
    assert!(log.agent("C-board.1", json!({"op":"update","fields":{"due":null}})).unwrap_err().contains("不能清空"));
}

#[test]
fn rules_change_only_by_a_person_and_a_delegate_only_proposes() {
    let mut log = Log::new(kanban(), "delegate", RulesInput::default());
    let mut rules: Rules = log.frame().rules;
    rules.text = "每读完一篇论文加一张卡片".into();
    log.event(RULES_CHANGED, "human:local", KANBAN, frames::LAYER, json!({"rules":rules}), vec![]).unwrap();
    assert_eq!(log.frame().rules.text, "每读完一篇论文加一张卡片");
    assert!(log.frame().rules_changed.is_some());
    assert!(log.event(RULES_CHANGED, "process:towermd-cli", KANBAN, frames::LAYER, json!({"rules":rules}), vec![]).is_err());
    let mut loose = rules.clone();
    loose.transitions.clear();
    assert!(log.event(RULES_CHANGED, "human:local", KANBAN, frames::LAYER, json!({"rules":loose}), vec![]).unwrap_err().contains("不能放宽"));
    log.agent(KANBAN, json!({"op":"rules","rules":{"per_lane_max":1}})).unwrap();
    assert_eq!(log.frame().rules.per_lane_max, Some(2), "a proposal changes nothing");
    assert_eq!(outcome(&log.state, KANBAN, log.seq).unwrap()["status"], "proposed");
    assert!(log.agent(KANBAN, json!({"op":"rules","rules":{"per_lane_max":9}})).unwrap_err().contains("不超过模板的上限"));
    log.resolve(log.frame().pending[0].id, true).unwrap();
    assert_eq!(log.frame().rules.per_lane_max, Some(1));
    assert!(log.person(KANBAN, json!({"op":"rules","rules":{}})).is_err());
}

#[test]
fn past_update_until_the_frame_rejects_every_write() {
    let rules = RulesInput { update_until: Some(Until::Date("2026-10-23".into())), ..Default::default() };
    let mut log = Log::new(kanban(), "delegate", rules);
    let error = log.agent(KANBAN, add("late")).unwrap_err();
    assert!(error.contains("冻结为只读记录"), "{error}");
    assert!(log.person(KANBAN, add("late")).unwrap_err().contains("冻结"));
    let frame = log.frame();
    assert!(frozen(&frame, "2026-10-24", |_| false));
    assert!(!frozen(&frame, "2026-10-23", |_| false), "the day itself still takes writes");
    let mut rules = frame.rules.clone();
    rules.update_until = Some(Until::Date("2026-12-31".into()));
    log.event(RULES_CHANGED, "human:local", KANBAN, frames::LAYER, json!({"rules":rules}), vec![]).unwrap();
    log.person(KANBAN, add("reopened by a person")).unwrap();
    let mut until = log.frame();
    until.rules.update_until = Some(Until::Object("T-1".into()));
    assert!(frozen(&until, "2026-10-24", |id| id == "T-1"));
    assert!(Template::parse(&kanban()).unwrap().rules(RulesInput { update_until: Some(Until::Date("soon".into())), ..Default::default() }).is_err());
}

#[test]
fn a_bound_frame_takes_no_free_cards_and_maps_only_done_objects_to_done_states() {
    let mut log = Log::with(kanban(), "delegate", "bound", RulesInput::default());
    assert!(log.agent(KANBAN, add("a")).unwrap_err().contains("绑定容器"));
    assert!(log.person(KANBAN, add("a")).unwrap_err().contains("混合模式"));
    let frame = log.frame();
    let object = |phase, done| Object { id: "T-1".into(), title: "t".into(), phase, done };
    assert_eq!(bound_state(&frame, &object(Phase::Ready, false)).as_deref(), Some("To Do"));
    assert_eq!(bound_state(&frame, &object(Phase::Running, false)).as_deref(), Some("Doing"));
    assert_eq!(bound_state(&frame, &object(Phase::ClaimedDone, false)).as_deref(), Some("Review"), "a claim never reaches Done");
    assert_eq!(bound_state(&frame, &object(Phase::ClaimedDone, true)).as_deref(), Some("Done"));
    let mut mixed = Log::with(kanban(), "delegate", "mixed", RulesInput::default());
    mixed.agent(KANBAN, add("free")).unwrap();
    let views = view(&mixed.state, &[], "2026-10-24").unwrap();
    assert_eq!((views[0].frame.cards.len(), views[0].bound.len(), views[0].frozen), (1, 0, false));
    assert!(view(&json!({}), &[], "2026-10-24").unwrap().is_empty());
}

#[test]
fn card_ids_name_their_frame() {
    assert_eq!(card_frame("C-board.12"), Ok("C-board"));
    assert_eq!(card_frame("C-a.b.3"), Ok("C-a.b"));
    for bad in ["C-board", "C-board.", "C-board.x", "C-board.01", "T-1", "board.1"] {
        assert!(card_frame(bad).is_err(), "{bad}");
    }
    assert!(is_write(ADDED) && is_write(RULES_PROPOSED) && !is_write(RESOLVED) && !is_write(RULES_CHANGED));
}
