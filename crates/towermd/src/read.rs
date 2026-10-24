//! C1 read commands over one state body (`GET /state`). Pure: nothing here
//! talks to the daemon or writes anything, so every view is a function of
//! the same body the App reads. The order of the attention list is the
//! daemon's (`attention_list`); this module never sorts it again.
use serde_json::{json, Map, Value};
use towermd_core::attention_list::{self, parse_time};
use towermd_devkit::dev_state::KITS;

/// `ls` kinds and the projection group each one reads.
pub const KINDS: &[&str] = &["tasks", "decisions", "proposals", "frames", "follow-ups", "ideas", "holds"];

/// Groups `show <id>` searches, in this order; the first hit wins.
const GROUPS: &[(&str, &str)] = &[
    ("Task", "task"),
    ("Run", "run"),
    ("Hold", "hold"),
    ("Proposal", "proposal"),
    ("QuestionDecision", "decision"),
    ("Frame", "frame"),
    ("FollowUp", "follow_up"),
    ("Idea", "idea"),
    ("Alert", "alert"),
    ("Evidence", "evidence"),
    ("Baseline", "baseline"),
];

fn group<'a>(state: &'a Value, name: &str) -> Option<&'a Map<String, Value>> {
    state.get("projection")?.get(name)?.as_object()
}

fn text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(value, |value, key| value.get(*key))?.as_str()
}

/// RFC 3339, a local `YYYY-MM-DDTHH:MM`, a date, or a span back from now
/// (`30m`, `12h`, `2d`); `None` when unreadable.
pub fn parse_when(input: &str, now: u64) -> Option<u64> {
    let input = input.trim();
    if let Some((number, unit)) = input.len().checked_sub(1).map(|split| input.split_at(split)) {
        let scale = match unit {
            "m" => Some(60_000),
            "h" => Some(3_600_000),
            "d" => Some(86_400_000),
            _ => None,
        };
        if let (Some(scale), Ok(count)) = (scale, number.parse::<u64>()) {
            return Some(now.saturating_sub(count.saturating_mul(scale)));
        }
    }
    let at = parse_time(input);
    if at > 0 {
        return Some(at);
    }
    let at = parse_time(&format!("{input}T00:00"));
    (at > 0).then_some(at)
}

/// The object with this ID in any group, with its kind; a task also carries
/// its delegation card when one exists.
pub fn object(state: &Value, id: &str) -> Option<Value> {
    for (name, kind) in GROUPS {
        if let Some(found) = group(state, name).and_then(|group| group.get(id)) {
            let mut value = json!({"kind": kind, "id": id, "object": found});
            if *kind == "task" {
                let card = state["delegation_view"]["objects"].as_array().into_iter().flatten().find(|card| card["id"] == id);
                value["card"] = card.cloned().unwrap_or(Value::Null);
            }
            return Some(value);
        }
    }
    None
}

/// One `ls` row: an ID, a state word to filter on, a title and a time (ms,
/// or null when the projection keeps none).
fn row(id: &str, state: &str, title: &str, at: Option<&str>) -> Value {
    let at = at.map(parse_time).filter(|at| *at > 0);
    json!({"id": id, "state": state, "title": title, "at": at.map(attention_list::when), "at_ms": at})
}

fn rows(state: &Value, kind: &str, view: Option<&Value>) -> Vec<Value> {
    let entries = |name: &str| group(state, name).into_iter().flatten();
    match kind {
        "tasks" => entries("Task").map(|(id, task)| {
            let label = if task["needs_review"] == true { "needs_review" } else { task["state"].as_str().unwrap_or("?") };
            row(id, label, text(task, &["input", "title"]).unwrap_or_default(), None)
        }).collect(),
        "decisions" => decisions(state, view),
        "proposals" => entries("Proposal").map(|(id, proposal)| {
            let title = text(proposal, &["proposal", "reason"]).or_else(|| text(proposal, &["text"])).unwrap_or_default();
            row(id, proposal["status"].as_str().unwrap_or("?"), title, text(proposal, &["at"]))
        }).collect(),
        "frames" => entries("Frame").map(|(id, frame)| {
            let label = if frame.get("archived").is_some_and(|archived| !archived.is_null()) { "archived" } else { "active" };
            row(id, label, text(frame, &["title"]).unwrap_or_default(), text(frame, &["created", "at"]))
        }).collect(),
        "follow-ups" => entries("FollowUp").map(|(id, follow)| {
            let label = if follow["done"].is_null() { "open" } else { "done" };
            row(id, label, text(follow, &["text"]).unwrap_or_default(), text(follow, &["added", "at"]))
        }).collect(),
        "ideas" => entries("Idea").map(|(id, idea)| {
            let label = if idea.get("promoted").is_some_and(|promoted| !promoted.is_null()) { "promoted" } else { "open" };
            row(id, label, text(idea, &["text"]).unwrap_or_default(), text(idea, &["added", "at"]))
        }).collect(),
        "holds" => entries("Hold").map(|(id, hold)| {
            let answered = group(state, "Answer").is_some_and(|answers| answers.contains_key(id) || answers.values().any(|answer| text(answer, &["input", "hold"]) == Some(id)));
            row(id, if answered { "answered" } else { "open" }, text(hold, &["draft", "text"]).unwrap_or_default(), text(hold, &["opened_at"]))
        }).collect(),
        _ => vec![],
    }
}

/// `ls <kind> [--state S] [--since T]`. Rows without a time never pass
/// `--since`; how many were left out for that reason is reported.
pub fn list(state: &Value, kind: &str, wanted: Option<&str>, since: Option<u64>, view: Option<&Value>) -> Value {
    let all = rows(state, kind, view);
    let total = all.len();
    let untimed = since.map_or(0, |_| all.iter().filter(|row| row["at_ms"].is_null()).count());
    let items: Vec<Value> = all.into_iter()
        .filter(|row| wanted.is_none_or(|wanted| row["state"] == wanted))
        .filter(|row| since.is_none_or(|since| row["at_ms"].as_u64().is_some_and(|at| at >= since)))
        .collect();
    json!({"kind": kind, "items": items, "total": total, "untimed": untimed})
}

/// Tasks a claim could take now: open, unbound, finalized, with fences and
/// acceptance, and every dependency merged. A preselection only; the daemon
/// runs the full claim gate.
pub fn claimable(state: &Value) -> Vec<Value> {
    let tasks = group(state, "Task");
    let merged = |id: &Value| {
        id.as_str().and_then(|id| tasks?.get(id)).is_some_and(|task| task["state"] == "merged" && task["needs_review"] != true)
    };
    tasks.into_iter().flatten().filter(|(_, task)| {
        let input = &task["input"];
        matches!(task["state"].as_str(), Some("proposed" | "ready"))
            && task["needs_review"] != true
            && task["active_run"].is_null()
            && input["clarify"] != true
            && input["allowed_paths"].as_array().is_some_and(|paths| !paths.is_empty())
            && input["acceptance"].as_array().is_some_and(|items| !items.is_empty())
            && input["depends_on"].as_array().into_iter().flatten().all(merged)
    }).map(|(id, task)| row(id, task["state"].as_str().unwrap_or("?"), text(task, &["input", "title"]).unwrap_or_default(), None)).collect()
}

/// Decisions recorded from answers, then decision documents on the canvas.
pub fn decisions(state: &Value, view: Option<&Value>) -> Vec<Value> {
    let superseded: Vec<&str> = group(state, "QuestionDecision").into_iter().flatten()
        .flat_map(|(_, decision)| decision["supersedes"].as_array().into_iter().flatten().filter_map(Value::as_str)).collect();
    let mut found: Vec<Value> = group(state, "QuestionDecision").into_iter().flatten().map(|(id, decision)| {
        let label = if superseded.contains(&id.as_str()) { "superseded" } else { "accepted" };
        let mut value = row(id, label, text(decision, &["question"]).unwrap_or_default(), None);
        value["decision"] = decision["decision"].clone();
        value["scope"] = decision["module"].clone();
        value["source"] = json!("answer");
        value
    }).collect();
    let nodes = view.and_then(|view| view["nodes"].as_array()).into_iter().flatten();
    found.extend(nodes.filter(|node| node["object_type"] == "Decision").map(|node| {
        let id = node["object_id"].as_str().or(node["id"].as_str()).unwrap_or_default();
        let mut value = row(id, "document", node["title"].as_str().unwrap_or_default(), None);
        value["file"] = node["file"].clone();
        value["source"] = json!("document");
        value
    }));
    found
}

/// Rules in force: those registered from answers (per scope) and those the
/// person set per work kind.
pub fn rules(state: &Value) -> Vec<Value> {
    let registered = group(state, "QuestionRule").into_iter().flatten().map(|(id, rule)| {
        json!({"id": id, "source": "answer", "scope": rule["module"], "text": rule["text"], "set_by": rule["registered_by"]})
    });
    let kinds = state["delegation_view"]["rules"].as_array().into_iter().flatten().map(|rule| {
        json!({"id": rule["id"], "source": "work_kind", "scope": rule["work_kind"], "text": rule["text"], "set_by": rule["event"]["actor"]})
    });
    registered.chain(kinds).collect()
}

/// The one command that handles an attention item from the terminal. Never an
/// approval: the command line offers none, so it shows the object instead.
fn handle(state: &Value, entry: &Value) -> String {
    let id = entry["id"].as_str().unwrap_or_default();
    let bare = id.split_once(':').map_or(id, |(_, rest)| rest.split(':').next().unwrap_or(rest));
    let objects = entry["objects"].as_array().into_iter().flatten().filter_map(Value::as_str);
    let target = [id, bare].into_iter().chain(objects).find(|candidate| object(state, candidate).is_some());
    match target {
        Some(target) => format!("towermd show {target}"),
        None => "towermd open".to_owned(),
    }
}

/// The daemon's ordered list: the first screen, or everything with `all`.
pub fn needs(state: &Value, all: bool) -> Value {
    let list = &state["attention_list"];
    let items = list["items"].as_array().cloned().unwrap_or_default();
    let shown = list["shown"].as_u64().unwrap_or(0) as usize;
    let total = items.len();
    let take = if all { total } else { shown.min(total) };
    let items: Vec<Value> = items.into_iter().take(take).map(|mut entry| {
        entry["cmd"] = json!(handle(state, &entry));
        entry
    }).collect();
    json!({"items": items, "shown": shown, "total": total, "later": total - shown.min(total), "asleep": list["asleep"], "budget": list["budget"]})
}

/// FR-131 digest since `since` (or the last 7 days when no view is known).
pub fn since(state: &Value, since: Option<u64>, now: u64) -> Value {
    let fallback = since.is_none();
    let since = since.unwrap_or_else(|| now.saturating_sub(attention_list::FALLBACK_MS));
    let list = attention_list::list(state, KITS, now);
    serde_json::to_value(attention_list::digest(state, KITS, &list, since, fallback, now)).unwrap_or(Value::Null)
}

/// Bare `towermd` in a project: three digest lines and the first screen.
pub fn home(project: &str, state: &Value, last_view: Option<u64>, now: u64) -> Value {
    let digest = since(state, last_view, now);
    json!({"project": project, "status": state["status"], "digest": digest, "needs": needs(state, false)})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Value, u64) {
        let fixture: Value = serde_json::from_str(include_str!("../../towermd-devkit/tests/fixtures/attention/state.json")).unwrap();
        let now = parse_time(fixture["now"].as_str().unwrap());
        let mut state = fixture["state"].clone();
        state["attention_list"] = serde_json::to_value(attention_list::list(&state, KITS, now)).unwrap();
        (state, now)
    }

    #[test]
    fn needs_keeps_the_daemon_order_and_the_first_screen() {
        let (state, _) = fixture();
        let first = needs(&state, false);
        let all = needs(&state, true);
        let ids = |value: &Value| value["items"].as_array().unwrap().iter().map(|item| item["id"].clone()).collect::<Vec<_>>();
        assert!(first["items"].as_array().unwrap().len() <= 3);
        assert_eq!(first["items"].as_array().unwrap().len() as u64, first["shown"].as_u64().unwrap());
        assert_eq!(ids(&all)[..ids(&first).len()], ids(&first)[..], "the first screen is a prefix of the full list");
        let daemon: Vec<Value> = state["attention_list"]["items"].as_array().unwrap().iter().map(|item| item["id"].clone()).collect();
        assert_eq!(ids(&all), daemon, "never reordered");
        assert_eq!(first["later"].as_u64().unwrap() as usize, daemon.len() - ids(&first).len());
        for item in all["items"].as_array().unwrap() {
            let cmd = item["cmd"].as_str().unwrap();
            assert!(cmd.starts_with("towermd show ") || cmd == "towermd open", "{cmd}");
        }
    }

    #[test]
    fn home_is_three_lines_and_at_most_three_items() {
        let (state, now) = fixture();
        let home = home("p", &state, None, now);
        assert_eq!(home["digest"]["lines"].as_array().unwrap().len(), 3);
        assert_eq!(home["digest"]["fallback"], true);
        assert!(home["needs"]["items"].as_array().unwrap().len() <= 3);
        let since = since(&state, Some(now - 3_600_000), now);
        assert_eq!(since["fallback"], false);
    }

    #[test]
    fn show_finds_any_object_and_ls_filters() {
        let (state, now) = fixture();
        assert_eq!(object(&state, "T-1").unwrap()["kind"], "task");
        assert_eq!(object(&state, "P-1").unwrap()["kind"], "proposal");
        assert_eq!(object(&state, "H-1").unwrap()["kind"], "hold");
        assert!(object(&state, "nope").is_none());
        let running = list(&state, "tasks", Some("running"), None, None);
        assert!(running["items"].as_array().unwrap().iter().all(|row| row["state"] == "running"));
        assert!(running["items"].as_array().unwrap().iter().any(|row| row["id"] == "T-1"));
        let recent = list(&state, "proposals", None, Some(now), None);
        assert!(recent["items"].as_array().unwrap().is_empty());
        let tasks = list(&state, "tasks", None, Some(0), None);
        assert_eq!(tasks["untimed"], tasks["total"]);
        for kind in KINDS {
            assert_eq!(list(&state, kind, None, None, None)["kind"], *kind);
        }
    }

    #[test]
    fn decisions_and_rules_read_both_sources() {
        let state = json!({"projection": {
            "QuestionDecision": {"D-1": {"id": "D-1", "module": "A", "question": "Q?", "decision": "yes", "supersedes": []},
                                 "D-2": {"id": "D-2", "module": "A", "question": "Q?", "decision": "no", "supersedes": ["D-1"]}},
            "QuestionRule": {"R-1": {"id": "R-1", "module": "A", "text": "no new deps", "registered_by": "human:local", "event": "E-1"}}},
            "delegation_view": {"rules": [{"id": "W-1", "work_kind": "docs", "text": "cite sources", "source": null, "event": {"actor": "human:local"}}]}});
        let view = json!({"nodes": [{"id": "n", "object_id": "ADR-1", "object_type": "Decision", "title": "Use SQLite", "file": "docs/adr-1.md"}]});
        let found = decisions(&state, Some(&view));
        assert_eq!(found.len(), 3);
        assert_eq!(found[0]["state"], "superseded");
        assert_eq!(found[2]["id"], "ADR-1");
        let rules = rules(&state);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1]["scope"], "docs");
    }

    #[test]
    fn times_parse_absolute_and_relative() {
        let now = parse_time("2026-10-24T12:00:00Z");
        assert_eq!(parse_when("2d", now), Some(now - 2 * 86_400_000));
        assert_eq!(parse_when("90m", now), Some(now - 90 * 60_000));
        assert_eq!(parse_when("2026-10-24T11:00:00Z", now), Some(now - 3_600_000));
        assert!(parse_when("2026-10-20", now).is_some());
        assert_eq!(parse_when("soon", now), None);
        assert_eq!(parse_when("", now), None);
    }
}
