use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::dev_state::Stamp;
use crate::run_record::{FinishMark, RunRecord};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn report() -> Value {
    json!({"status":"done","summary":"改好了","checkpoints":[SHA],"changed_paths":["src/a/x.rs"],
        "questions":[],"suspected_class":null,"proposal":null})
}

#[test]
fn reports_follow_fr_28_and_reject_bad_fields() {
    let parsed = RunReport::parse(&report()).unwrap();
    assert_eq!((parsed.status, parsed.checkpoints.len()), (ReportStatus::Done, 1));
    // Optional lists and nullable keys may be absent.
    assert!(RunReport::parse(&json!({"status":"needs_input","summary":"等回答","questions":["用哪个库？"]})).is_ok());
    let long: String = "字".repeat(SUMMARY_CHARS);
    assert!(RunReport::parse(&json!({"status":"done","summary":long})).is_ok());
    for (key, value) in [
        ("summary", json!(format!("{long}字"))), ("summary", json!(" ")), ("checkpoints", json!(["abc"])),
        ("changed_paths", json!(["../x"])), ("changed_paths", json!(["/abs"])), ("questions", json!([""])),
        ("status", json!("merged")), ("suspected_class", json!("bad")), ("proposal", json!({"kind":"","reason":"x"})),
        ("extra", json!(1)),
    ] {
        let mut value_report = report();
        value_report[key] = value.clone();
        assert!(RunReport::parse(&value_report).is_err(), "{key} {value}");
    }
}

#[test]
fn a_report_is_taken_from_the_whole_text_or_its_last_json_block() {
    let text = report().to_string();
    assert_eq!(RunReport::extract(&text).unwrap().summary, "改好了");
    let fenced = format!("先说明。\n```json\n{{\"status\":\"blocked\",\"summary\":\"旧\"}}\n```\n然后\n```json\n{text}\n```\n");
    assert_eq!(RunReport::extract(&fenced).unwrap().status, ReportStatus::Done);
    assert!(RunReport::extract("没有报告").is_none());
}

#[test]
fn the_schema_is_strict_and_matches_the_report_keys() {
    let schema = schema();
    assert_eq!(schema["additionalProperties"], false);
    let required: Vec<&str> = schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    let properties: Vec<&str> = schema["properties"].as_object().unwrap().keys().map(String::as_str).collect();
    let mut sorted = required.clone();
    sorted.sort();
    assert_eq!(sorted, properties);
    assert_eq!(required.len(), report().as_object().unwrap().len());
}

fn finished(outcome: Outcome, report: Option<RunReport>) -> RunFinished {
    RunFinished { run: "R".into(), task: "T".into(), agent: "a/1".into(), outcome, status: ReportStatus::Failed,
        stop: None, exit_code: Some(1), signal: None, crashed: false, report, summary: "s".into(), error_class: None,
        error: None, cost_usd: None, tokens: Tokens::default(), session: None, attempts: 1, raw: None }
}

#[test]
fn errors_are_classed_by_observation_then_by_the_reported_suspicion() {
    let mut run = finished(Outcome::Failed, None);
    run.classify(true);
    assert_eq!(run.error_class, Some(ErrorClass::External));
    run.classify(false);
    assert_eq!(run.error_class, Some(ErrorClass::Execution));
    let mut design = RunReport::parse(&report()).unwrap();
    design.status = ReportStatus::Failed;
    design.suspected_class = Some(ErrorClass::Design);
    let mut run = finished(Outcome::Reported, Some(design.clone()));
    run.classify(false);
    assert_eq!(run.error_class, Some(ErrorClass::Design));
    design.suspected_class = None;
    let mut run = finished(Outcome::Reported, Some(design));
    run.classify(false);
    assert_eq!(run.error_class, Some(ErrorClass::Execution));
    for outcome in [Outcome::Budget, Outcome::Interrupted] {
        let mut run = finished(outcome, None);
        run.classify(true);
        assert_eq!(run.error_class, None);
    }
    let mut run = finished(Outcome::Reported, Some(RunReport::parse(&report()).unwrap()));
    run.classify(false);
    assert_eq!(run.error_class, None);
}

fn record(task: &str, marks: &[(&str, Option<ErrorClass>)]) -> RunRecord {
    RunRecord { task: task.into(), module: "A".into(), agent: "a/1".into(), budget_usd: None, running: None, forgiven: 0,
        starts: marks.len() as u32, retries: 0, failures: 0, known_cost_usd: 0.0, unknown_cost: 0,
        finishes: marks.iter().enumerate().map(|(seq, (at, class))| FinishMark {
            at: Stamp { at: (*at).into(), eid: format!("e{seq}"), seq: seq as u64 }, outcome: Outcome::Failed,
            status: ReportStatus::Failed, stop: None, error_class: *class, cost_usd: None }).collect() }
}

#[test]
fn the_same_error_class_three_times_in_a_day_is_a_signal() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-24T12:00:00Z").unwrap().with_timezone(&chrono::Utc);
    let execution = Some(ErrorClass::Execution);
    let mut records = BTreeMap::new();
    records.insert("R-1".into(), record("T-1", &[("2026-10-24T01:00:00Z", execution), ("2026-10-23T11:00:00Z", execution)]));
    records.insert("R-2".into(), record("T-2", &[("2026-10-24T02:00:00Z", execution), ("2026-10-24T03:00:00Z", Some(ErrorClass::Design))]));
    let thresholds = RepeatThresholds::default();
    assert_eq!(repeated_errors(&records, now, thresholds), []);
    records.insert("R-3".into(), record("T-3", &[("2026-10-24T11:59:00Z", execution), ("bad", execution)]));
    assert_eq!(repeated_errors(&records, now, thresholds),
        [RepeatedError { class: ErrorClass::Execution, count: 3, runs: vec!["R-1".into(), "R-2".into(), "R-3".into()] }]);
    let config: serde_yaml::Value = serde_yaml::from_str("repeated_error: {count: 2, window_hours: 48}").unwrap();
    let wider = RepeatThresholds::from_config(Some(&config));
    assert_eq!(wider, RepeatThresholds { count: 2, window_hours: 48 });
    assert_eq!(repeated_errors(&records, now, wider)[0].count, 4);
    let bad: serde_yaml::Value = serde_yaml::from_str("repeated_error: {count: 0, window_hours: x}").unwrap();
    assert_eq!(RepeatThresholds::from_config(Some(&bad)), RepeatThresholds::default());
}
