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
}
