use super::*;
use crate::frames::{RulesInput, Template, CREATED};
use serde_json::json;

const BOARD: &str = "C-board";

fn kanban(views: Value) -> Value {
    json!({"id":"kanban","version":"1.1.0","type":"template",
        "layout":{"x":{"category":"state"},"y":{"order":"added"},"structure":"none"},
        "states":["To Do","Doing","Review","Done"],"transitions":{"Review":["Done","Doing"]},"done_states":["Done"],
        "card_fields":{"owner":"text"},"default_set_by":"human","views":views})
}

fn reduce(state: &mut Value, event: &Event) -> Result<(), String> {
    frames::apply(state, event, &[]).map(|_| ())
}

struct Log {
    events: Vec<Event>,
}

impl Log {
    fn new(definition: Value, t: &str) -> Self {
        let template = Template::parse(&definition).unwrap();
        let data = json!({"title":"板","template":{"id":template.id,"version":template.version,"definition":definition},
            "set_by":"human","mode":"free","rules":template.rules(RulesInput::default()).unwrap(),
            "place":{"x":0,"y":0,"width":880,"height":420}});
        let mut log = Log { events: Vec::new() };
        log.push(t, CREATED, BOARD, frames::LAYER, data);
        log
    }
    fn push(&mut self, t: &str, kind: &str, obj: &str, layer: &str, data: Value) {
        let seq = self.events.len() as u64 + 1;
        self.events.push(serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":t,
            "actor":"human:local","type":kind,"layer":layer,"obj":obj,"data":data,"guards":[],
            "epoch":null,"stale":false,"cost_usd":null})).unwrap());
    }
    fn person(&mut self, t: &str, target: &str, op: Value) {
        let kind = serde_json::from_value::<cards::Op>(op.clone()).unwrap().kind();
        self.push(t, kind, target, cards::LAYER, op);
    }
    fn flow(&self, today: &str) -> Option<Flow> {
        cumulative_flow(&self.events, BOARD, &json!({}), reduce, &[], today).unwrap()
    }
}

fn rows(flow: &Flow) -> Vec<(&str, Vec<u64>)> {
    flow.days.iter().map(|day| (day.date.as_str(), day.counts.clone())).collect()
}

#[test]
fn each_day_counts_the_cards_in_each_state_at_its_end_and_quiet_days_carry_over() {
    let mut log = Log::new(kanban(json!(["cumulative_flow"])), "2026-10-20T09:00:00Z");
    log.person("2026-10-20T09:01:00Z", BOARD, json!({"op":"add","title":"甲"}));
    log.person("2026-10-20T09:02:00Z", BOARD, json!({"op":"add","title":"乙"}));
    log.person("2026-10-20T23:59:00Z", "C-board.1", json!({"op":"move","state":"Doing"}));
    // Late on the 22nd in UTC+9 is already the 22nd in UTC; the day is UTC.
    log.person("2026-10-23T08:00:00+09:00", BOARD, json!({"op":"add","title":"丙"}));
    log.person("2026-10-22T12:00:00Z", "C-board.1", json!({"op":"move","state":"Done"}));
    log.person("2026-10-24T01:00:00Z", "C-board.2", json!({"op":"archive"}));
    let flow = log.flow("2026-10-25").unwrap();
    assert_eq!((flow.states.as_slice(), flow.done_states.as_slice()), (&["To Do", "Doing", "Review", "Done"].map(String::from)[..], &["Done".to_owned()][..]));
    assert_eq!(rows(&flow), [
        ("2026-10-20", vec![1, 1, 0, 0]),
        ("2026-10-21", vec![1, 1, 0, 0]),
        ("2026-10-22", vec![2, 0, 0, 1]),
        ("2026-10-23", vec![2, 0, 0, 1]),
        ("2026-10-24", vec![1, 0, 0, 1]),
        ("2026-10-25", vec![1, 0, 0, 1]),
    ]);
    assert_eq!(log.flow("2026-10-24").unwrap().days.last().unwrap().date, "2026-10-24");
    assert_eq!(log.flow("2026-10-01").unwrap().days.last().unwrap().date, "2026-10-24", "a clock behind the events still ends at the last event");
}

#[test]
fn events_before_the_frame_existed_and_other_frames_add_no_days() {
    let mut log = Log { events: Vec::new() };
    log.push("2026-10-01T00:00:00Z", "x.noted", "X-1", "L3", json!({}));
    let created = Log::new(kanban(json!(["cumulative_flow"])), "2026-10-03T00:00:00Z");
    log.push("2026-10-03T00:00:00Z", CREATED, BOARD, frames::LAYER, created.events[0].data.clone());
    let flow = log.flow("2026-10-04").unwrap();
    assert_eq!(rows(&flow), [("2026-10-03", vec![0, 0, 0, 0]), ("2026-10-04", vec![0, 0, 0, 0])]);
    assert!(cumulative_flow(&log.events, "C-other", &json!({}), reduce, &[], "2026-10-04").unwrap().is_none());
    assert!(cumulative_flow(&[], BOARD, &json!({}), reduce, &[], "2026-10-04").unwrap().is_none());
    assert!(cumulative_flow(&log.events, BOARD, &json!({}), reduce, &[], "tomorrow").is_err());
}

#[test]
fn the_flow_covers_at_most_the_last_year_carrying_the_older_counts_in() {
    let mut log = Log::new(kanban(json!(["cumulative_flow"])), "2024-01-01T00:00:00Z");
    log.person("2024-01-01T00:01:00Z", BOARD, json!({"op":"add","title":"甲"}));
    let flow = log.flow("2026-10-24").unwrap();
    assert_eq!(flow.days.len() as u64, DAYS_MAX);
    assert_eq!((flow.days[0].date.as_str(), flow.days[0].counts.as_slice()), ("2025-10-24", &[1, 0, 0, 0][..]));
    assert_eq!(flow.days.last().unwrap().date, "2026-10-24");
}

#[test]
fn a_frame_offers_only_views_its_template_declares_knows_and_fits() {
    let frame = |definition: Value| {
        let mut state = json!({});
        frames::apply(&mut state, &Log::new(definition, "2026-10-24T00:00:00Z").events[0], &[]).unwrap();
        frames::read(&state).unwrap()[BOARD].clone()
    };
    assert_eq!(offered(&frame(kanban(json!(["cumulative_flow"])))), [CUMULATIVE_FLOW]);
    assert!(offered(&frame(kanban(json!(null)))).is_empty(), "a template that declares nothing offers nothing");
    assert!(offered(&frame(kanban(json!(["evolution"])))).is_empty(), "a name from a later build is ignored");
    let stateless = json!({"id":"map","version":"1","type":"template","layout":{"structure":"tree"},"views":["cumulative_flow"]});
    assert!(offered(&frame(stateless)).is_empty(), "a flow needs states");
    let problems = Template::parse(&kanban(json!("cumulative_flow"))).unwrap_err();
    assert!(problems.iter().any(|problem| problem.contains("views")), "{problems:?}");
    assert!(offered(&frame(kanban(json!(["plan_deviation"])))).is_empty(), "a deviation needs a time axis");
    assert_eq!(offered(&frame(timeline(json!(["plan_deviation", "argument_evolution"])))), [PLAN_DEVIATION, EVOLUTION]);
    let undone = json!({"id":"t","version":"1","type":"template","layout":{"x":{"time":"date"}},"card_fields":{"date":"date"},"views":["plan_deviation"]});
    assert!(offered(&frame(undone)).is_empty(), "a deviation needs done states");
}

fn timeline(views: Value) -> Value {
    json!({"id":"timeline","version":"1.1.0","type":"template","layout":{"x":{"time":"date"},"y":"none","structure":"none"},
        "states":["计划","进行中","已完成"],"done_states":["已完成"],"card_fields":{"date":"date","source":"ref"},
        "default_set_by":"human","views":views})
}

#[test]
fn the_deviation_compares_each_cards_first_date_with_when_it_was_done() {
    let mut log = Log::new(timeline(json!(["plan_deviation"])), "2026-10-01T00:00:00Z");
    log.person("2026-10-01T01:00:00Z", BOARD, json!({"op":"add","title":"冻结","fields":{"date":"2026-10-10"}}));
    log.person("2026-10-01T02:00:00Z", BOARD, json!({"op":"add","title":"发布","fields":{"date":"2026-10-20"}}));
    log.person("2026-10-01T03:00:00Z", BOARD, json!({"op":"add","title":"复盘","fields":{"date":"2026-10-28"}}));
    log.person("2026-10-05T00:00:00Z", "C-board.2", json!({"op":"update","fields":{"date":"2026-10-24"}}));
    log.person("2026-10-12T00:00:00Z", "C-board.1", json!({"op":"move","state":"已完成"}));
    // Reopened and done again: the later day counts.
    log.person("2026-10-13T00:00:00Z", "C-board.1", json!({"op":"move","state":"进行中"}));
    log.person("2026-10-14T09:00:00Z", "C-board.1", json!({"op":"move","state":"已完成"}));
    log.person("2026-10-15T00:00:00Z", "C-board.3", json!({"op":"update","fields":{"date":"2026-10-30"}}));
    let view = plan_deviation(&log.events, BOARD, &json!({}), reduce, "2026-10-26").unwrap().unwrap();
    assert_eq!((view.field.as_str(), view.today.as_str()), ("date", "2026-10-26"));
    let rows: Vec<_> = view.cards.iter().map(|card| (card.title.as_str(), card.planned.as_deref(), card.plan.as_deref(), card.done.as_deref(),
        card.slip_days, card.deviation_days, card.overdue_days)).collect();
    assert_eq!(rows, [
        ("冻结", Some("2026-10-10"), Some("2026-10-10"), Some("2026-10-14"), Some(0), Some(4), None),
        ("发布", Some("2026-10-20"), Some("2026-10-24"), None, Some(4), None, Some(2)),
        ("复盘", Some("2026-10-28"), Some("2026-10-30"), None, Some(2), None, None),
    ]);
    assert_eq!(view.cards[1].replans, [Replan { date: "2026-10-05".into(), from: Some("2026-10-20".into()), to: Some("2026-10-24".into()) }]);
    assert_eq!(view.cards[2].replans, [Replan { date: "2026-10-15".into(), from: Some("2026-10-28".into()), to: Some("2026-10-30".into()) }]);
    log.person("2026-10-16T00:00:00Z", "C-board.3", json!({"op":"archive"}));
    let view = plan_deviation(&log.events, BOARD, &json!({}), reduce, "2026-10-26").unwrap().unwrap();
    assert_eq!(view.cards.len(), 2, "archived cards leave the view");
    assert!(plan_deviation(&log.events, "C-other", &json!({}), reduce, "2026-10-26").unwrap().is_none());
    let board = Log::new(kanban(json!([])), "2026-10-01T00:00:00Z");
    assert!(plan_deviation(&board.events, BOARD, &json!({}), reduce, "2026-10-26").unwrap().is_none(), "no time axis");
}

#[test]
fn the_evolution_lists_each_change_of_each_card_oldest_first() {
    let argument = json!({"id":"argument","version":"1","type":"template","layout":{"structure":"relations"},
        "states":["To Do","Done"],"done_states":["Done"],"views":["argument_evolution"]});
    let mut log = Log::new(argument, "2026-10-01T00:00:00Z");
    log.person("2026-10-01T01:00:00Z", BOARD, json!({"op":"add","title":"主张甲"}));
    log.person("2026-10-02T01:00:00Z", BOARD, json!({"op":"add","title":"证据乙"}));
    log.person("2026-10-03T01:00:00Z", "C-board.2", json!({"op":"link","to":"C-board.1"}));
    log.person("2026-10-04T01:00:00Z", "C-board.1", json!({"op":"update","title":"主张甲（修订）"}));
    log.person("2026-10-05T01:00:00Z", "C-board.1", json!({"op":"move","state":"Done"}));
    log.person("2026-10-06T01:00:00Z", "C-board.2", json!({"op":"archive"}));
    let view = evolution(&log.events, BOARD, &json!({}), reduce).unwrap().unwrap();
    let steps: Vec<_> = view.steps.iter().map(|step| (step.date.as_str(), step.card.as_str(), step.change, step.state.as_deref(), step.other.as_deref())).collect();
    assert_eq!(steps, [
        ("2026-10-01", "C-board.1", Change::Proposed, Some("To Do"), None),
        ("2026-10-02", "C-board.2", Change::Proposed, Some("To Do"), None),
        ("2026-10-03", "C-board.2", Change::Linked, None, Some("C-board.1")),
        ("2026-10-04", "C-board.1", Change::Renamed, None, None),
        ("2026-10-05", "C-board.1", Change::State, Some("Done"), None),
        ("2026-10-06", "C-board.2", Change::Archived, None, None),
    ]);
    assert!(view.steps.iter().all(|step| step.by == "human:local"));
    assert_eq!((view.steps[3].title.as_str(), view.omitted), ("主张甲（修订）", 0));
    assert_eq!(view.done_states, ["Done"]);
    assert!(evolution(&log.events, "C-other", &json!({}), reduce).unwrap().is_none());
}
