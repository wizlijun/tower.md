//! The attention list with the development kit's placement, against the
//! fixture the App's tests read too (`src/lib/attention-list.test.ts`): the
//! daemon's order is the one order, and the App's assembly finds the same items.
use std::path::Path;

use serde_json::{json, Value};
use towermd_core::attention_list::{self, parse_time};
use towermd_devkit::dev_state::KITS;

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attention").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// What both sides must agree on, per item.
fn summary(list: &attention_list::List) -> Value {
    let items: Vec<Value> = list.items.iter().map(|entry| json!({ "id": entry.item.id, "kind": entry.label, "class": entry.class,
        "timing": entry.timing, "quiet": entry.quiet, "objects": entry.item.objects, "areas": entry.item.areas })).collect();
    json!({ "items": items, "shown": list.shown, "asleep": list.asleep, "budget": list.budget })
}

#[test]
fn the_fixture_orders_as_expected() {
    let input = fixture("state.json");
    let now = parse_time(input["now"].as_str().unwrap());
    let list = attention_list::list(&input["state"], KITS, now);
    let mut actual = summary(&list);
    // The digest's heading quotes local time, so only its lines and details are shared.
    let since = parse_time(input["since"].as_str().unwrap());
    let digest = attention_list::digest(&input["state"], KITS, &list, since, false, now);
    actual["digest"] = json!({ "lines": digest.lines, "details": digest.details });
    if std::env::var_os("TOWERMD_BLESS").is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attention/expected.json");
        std::fs::write(path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
    }
    assert_eq!(actual, fixture("expected.json"));
    assert!(list.items.iter().all(|entry| entry.card.chars().count() <= attention_list::CARD_CHARS));
}

#[test]
fn the_kit_owns_its_check_failure_and_stuck_wording() {
    let input = fixture("state.json");
    let now = parse_time(input["now"].as_str().unwrap());
    let list = attention_list::list(&input["state"], KITS, now);
    let gate = list.items.iter().find(|entry| entry.item.id == "A-gate").unwrap();
    assert_eq!((gate.label, gate.item.title.as_str(), gate.item.detail.as_deref()), ("检查未通过", "任务未通过完成检查", Some("cargo test 退出码 101")));
    let digest = attention_list::digest(&input["state"], KITS, &list, parse_time("2026-10-23T00:00:00Z"), false, now);
    assert_eq!(digest.lines[0], "完成了：画布改色");
    // Three stuck tasks (the held 解析器拆分 too) are counted rather than named.
    assert_eq!(digest.lines[1], "卡在：3 件；另有 1 条提醒");
    assert_eq!(&digest.details[1..4], ["卡住 · 画布改色", "卡住 · 文档整理", "卡住 · 解析器拆分"]);
    assert_eq!(digest.lines[2], format!("需要你：3 件，另有 {} 条已延后", list.items.len() - 3));
}

/// FR-186: the due decision comes back with its question, options, decision
/// and reason; `attention-list.test.ts` expects the same card text.
#[test]
fn a_due_decision_comes_back_with_what_was_asked_and_why() {
    let input = fixture("state.json");
    let list = attention_list::list(&input["state"], KITS, parse_time(input["now"].as_str().unwrap()));
    let review = list.items.iter().find(|entry| entry.item.id == "decision-review:D-1").unwrap();
    assert_eq!((review.label, review.class, review.item.title.as_str(), review.item.object.as_deref()), ("复查决策", 4, "导出用什么格式", Some("决策 D-1")));
    assert_eq!(review.item.detail.as_deref(), Some("2026-10-20 到期复查；当时的选项：JSON、CSV；当时定为“JSON”，理由：下游只读 JSON"));
    assert!(!list.items.iter().any(|entry| ["D-2", "D-3", "D-4", "D-5", "D-6"].iter().any(|id| entry.item.id == format!("decision-review:{id}"))),
        "not yet due, kept to a later day, superseded, never dated, or voided");
}

/// The state of `runtime.test.ts` "ranks them by FR-128": protection first,
/// then what waits on a person (longest wait first), then the rest by reach.
#[test]
fn ranking_follows_fr_128_with_tasks_placed_by_the_kit() {
    let task = json!({ "input": { "id": "T", "title": "T", "module": "A", "touches": [], "allowed_paths": ["src/**"], "depends_on": [], "acceptance": [] },
        "state": "running", "baseline": "towermd/B0", "needs_review": false, "active_run": null });
    let hold = |id: &str, at: u64| json!({ "draft": { "id": id, "text": id, "options": [], "context": [] }, "binding": { "task": "T", "module": "A" }, "targets": [], "opened_at": at });
    let state = json!({ "status": "ready", "projection": {
        "Task": { "T": task },
        "Alert": {
            "warning": { "at": "2026-10-20T00:00:00Z", "data": { "reason": "unknown_trailer", "detail": { "severity": "warning" } } },
            "failure": { "at": "2026-10-21T00:00:00Z", "data": { "reason": "task_gate_failed", "detail": { "task": "T", "error": "测试未通过" } } },
            "violation": { "data": { "reason": "unapproved_protected_change", "detail": { "commit": "a".repeat(40) } } },
        },
        "Hold": { "newer": hold("newer", 200), "older": hold("older", 100) },
        "Proposal": { "P": { "status": "submitted", "proposal": { "reason": "调整接口", "impact": { "modules": ["B"], "tasks": ["T-B"] } } } },
    } });
    let list = attention_list::list(&state, KITS, parse_time("2026-10-23T12:00:00Z"));
    let ids: Vec<&str> = list.items.iter().map(|entry| entry.item.id.as_str()).collect();
    assert_eq!(ids, ["violation", "older", "newer", "P", "failure", "warning"]);
    assert_eq!(list.shown, 3);
    let failure = &list.items[4];
    assert_eq!((failure.item.areas.clone(), failure.item.detail.as_deref()), (vec!["A".to_owned()], Some("测试未通过")));
}

/// FR-03 and FR-72: a minor process change and a waiver in its last day,
/// worded as the App words them; a waiver further out or already lapsed is silent.
#[test]
fn minor_changes_and_expiring_waivers_join_the_list() {
    let input = fixture("state.json");
    let now = parse_time(input["now"].as_str().unwrap());
    let list = attention_list::list(&input["state"], KITS, now);
    let find = |id: &str| list.items.iter().find(|entry| entry.item.id == id);
    let minor = find("A-minor").unwrap();
    assert_eq!((minor.label, minor.class, minor.item.title.as_str()), ("禁止", 1, "主干含未登记的流程配置变更（次要变更）"));
    assert_eq!(find("A-prot").unwrap().item.title, "主干含未经批准的人定内容变更");
    let soon = find("waiver:W-soon").unwrap();
    assert_eq!((soon.label, soon.symbol, soon.class, soon.item.delegation.as_deref()), ("豁免到期", "⌛", 3, Some("T-2")));
    assert!(soon.item.title.starts_with("门禁 lint 的豁免将于 ") && soon.item.title.ends_with(" 到期"), "{}", soon.item.title);
    assert_eq!(soon.item.detail.as_deref(), Some("豁免理由：上游修复下周发布；到期后不续期，门禁恢复拦截"));
    let free = find("waiver:W-free").unwrap();
    assert_eq!((free.item.object.as_deref(), free.item.objects.len()), (Some("ops/db"), 0));
    assert!(find("waiver:W-far").is_none() && find("waiver:W-gone").is_none());
}

/// Spec 12.1 and FR-58: held tasks and open reviews wait on a person (class 2)
/// with the kit's wording; run errors and budgets are class 3; a spent gate's
/// earlier per-attempt failure is folded into its interrupt; a resolved review is gone.
#[test]
fn interrupts_and_reviews_wait_on_a_person() {
    let input = fixture("state.json");
    let now = parse_time(input["now"].as_str().unwrap());
    let list = attention_list::list(&input["state"], KITS, now);
    let find = |id: &str| list.items.iter().find(|entry| entry.item.id == id);
    let spent = find("A-spent").unwrap();
    assert_eq!((spent.label, spent.symbol, spent.class, spent.item.title.as_str(), spent.item.delegation.as_deref()),
        ("中断", "⏸", 2, "验证连续 3 次未过", Some("T-4")));
    assert!(spent.item.action.contains("豁免声明的检查"));
    assert!(find("A-g2").unwrap().item.action.contains("路径守卫不能豁免"));
    assert_eq!(find("A-g2").unwrap().item.title, "task_forbidden");
    assert_eq!(find("A-info").unwrap().label, "中断");
    assert!(find("A-fail4").is_none() && find("A-gate").is_some(), "only the spent gate's failure is folded");
    let crash = find("A-crash").unwrap();
    assert_eq!((crash.label, crash.symbol, crash.class, crash.item.detail.as_deref()), ("错误", "‼", 3, Some("退出码 1")));
    let budget = find("A-budget").unwrap();
    assert_eq!((budget.label, budget.symbol, budget.quiet), ("预算", "¤", true));
    let review = find("review:V-1").unwrap();
    assert_eq!((review.label, review.symbol, review.class, review.item.title.as_str()), ("评审", "⚖", 2, "代码评审（G4）：拆出词法分析"));
    assert_eq!((review.item.objects.clone(), review.item.areas.clone(), review.item.delegation.as_deref()), (vec!["T-4".to_owned()], vec!["parser".to_owned()], Some("T-4")));
    let free = find("review:V-2").unwrap();
    assert_eq!((free.item.title.as_str(), free.item.object.as_deref()), ("outline：备份方案", Some("ops/db")));
    assert!(find("review:V-0").is_none());
    assert!(list.items.iter().all(|entry| entry.item.id != "A-old"));
}
