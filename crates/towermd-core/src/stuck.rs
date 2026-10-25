//! Looping detection (core spec FR-126): Tower.md computes `core.stuck` from
//! events alone, with no domain knowledge. Any one rule raises it:
//!
//! 1. the same object's reports name basically the same `next` this many
//!    times in a row (`same_next`, default 3);
//! 2. the same check on the same object fails this many times in a row
//!    (`check_failures`, default 3); an errored run counts as a failure;
//! 3. the same object asks more than `asks` questions within
//!    `ask_window_hours` (default 3 in 24 hours);
//! 4. an open delegation has run longer than `duration_factor` times the
//!    median of finished delegations of the same work kind (default 3, with
//!    at least 3 finished samples).
//!
//! Every `core.stuck` is `active`. Rules 1–3 fire on the event that reaches
//! the threshold, and only within the last 24 hours, so turning detection
//! on does not flood the person with history. Each signal's hub ID derives
//! from its rule and source, so it is raised once. When the attempt is known,
//! the signal names the object and configuration (`delegation:`, `config:`)
//! and the trust ledger counts it (FR-202).
use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    delegation::{fingerprint, parse_time, KitAttempt, KitDelegation},
    delegation_projection as delegations,
    gates::{self, Recorded},
    hub::Event,
    kit::Kit,
    question_model::normalize_question,
    question_projection,
    requests,
    tower_api::{self, clip, Envelope, Level, Signal, DETAIL_CHARS, TITLE_CHARS},
    weakening::{REF_CONFIG, REF_DELEGATION},
};

/// The signal kind; only Tower.md raises it.
pub const KIND: &str = "core.stuck";
/// Ref naming the rule that fired.
pub const REF_RULE: &str = "stuck:";
/// Rules 1–3 fire only on events this recent.
pub const RECENT_MS: i64 = 24 * 60 * 60 * 1000;
/// Rule 4 needs at least this many finished delegations of the kind.
pub const MIN_SAMPLES: usize = 3;

/// `config.yaml` `stuck:`; see `Config::stuck`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    pub same_next: u32,
    pub check_failures: u32,
    pub asks: u32,
    pub ask_window_hours: u32,
    pub duration_factor: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { same_next: 3, check_failures: 3, asks: 3, ask_window_hours: 24, duration_factor: 3.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule { SameNext, CheckFailures, Asks, Duration }

impl Rule {
    pub fn name(self) -> &'static str {
        match self {
            Rule::SameNext => "same_next",
            Rule::CheckFailures => "check_failures",
            Rule::Asks => "asks",
            Rule::Duration => "duration",
        }
    }
}

/// One looping found, and the attempt it is attributed to when known.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Trigger {
    pub rule: Rule,
    pub target: String,
    /// The event, evidence, question or attempt that reached the threshold.
    pub source: String,
    /// `(attempt, configuration fingerprint)`.
    pub attempt: Option<(String, String)>,
    pub title: String,
    pub detail: String,
    pub value: Value,
    pub threshold: Value,
}

impl Trigger {
    /// Hub ID of the signal; one per rule and source.
    pub fn eid(&self) -> String {
        tower_api::signal_eid(KIND, &format!("{}:{}", self.rule.name(), self.source))
    }
    pub fn refs(&self) -> Vec<String> {
        let mut refs = vec![format!("{REF_RULE}{}", self.rule.name())];
        if let Some((_, config)) = &self.attempt {
            refs.push(format!("{REF_DELEGATION}{}", self.target));
            refs.push(format!("{REF_CONFIG}{config}"));
        }
        refs
    }
    pub fn signal(&self) -> Signal {
        Signal {
            kind: KIND.into(), level: Level::Active, title: clip(&self.title, TITLE_CHARS),
            detail: Some(clip(&self.detail, DETAIL_CHARS)), value: Some(self.value.clone()),
            threshold: Some(self.threshold.clone()), deterministic: true,
        }
    }
    pub fn envelope(&self, project: &str, at: String) -> Result<Envelope, String> {
        tower_api::tower_signal(project, &self.target, self.refs(), at, &self.signal())
    }
}

fn time(ms: u64) -> Result<DateTime<FixedOffset>, String> {
    Ok(i64::try_from(ms).ok().and_then(DateTime::from_timestamp_millis).ok_or("时间无效")?.fixed_offset())
}

fn recent(at: DateTime<FixedOffset>, now: DateTime<FixedOffset>) -> bool {
    (now - at).num_milliseconds() <= RECENT_MS
}

fn attributed(attempt: Option<&KitAttempt>) -> Option<(String, String)> {
    attempt.map(|attempt| (attempt.id.clone(), fingerprint(attempt.delegate.as_ref())))
}

/// The attempt running on `object` when the hub reached `seq`.
fn running_at<'a>(items: &'a [KitDelegation], object: &str, seq: u64) -> Option<&'a KitAttempt> {
    items.iter().find(|item| item.id == object)?.attempts.iter().rev().find(|attempt| attempt.delegated.seq <= seq)
}

fn attempt<'a>(items: &'a [KitDelegation], object: &str, id: &str) -> Option<&'a KitAttempt> {
    items.iter().find(|item| item.id == object)?.attempts.iter().find(|attempt| attempt.id == id)
}

/// Rule 1, over accepted reports and replies in hub order.
fn same_next(events: &[&Event], state: &Value, items: &[KitDelegation], limit: u32, now: DateTime<FixedOffset>)
    -> Result<Vec<Trigger>, String> {
    let checkpoints: BTreeMap<String, (String, String)> = delegations::report_requests(items, &delegations::read(state)?)
        .into_iter().map(|request| (request.id, (request.object, request.attempt))).collect();
    let generic = requests::read(state)?;
    let mut streaks: BTreeMap<String, (String, u32)> = BTreeMap::new();
    let mut found = Vec::new();
    for event in events.iter().filter(|event| !event.stale) {
        let (target, next, attempt_id) = match event.kind.as_str() {
            "report.received" => (event.obj.clone(), event.data["data"]["next"].as_str(), None),
            "report.replied" => match checkpoints.get(&event.obj) {
                Some((object, attempt)) => (object.clone(), event.data["next"].as_str(), Some(attempt.clone())),
                None => continue,
            },
            requests::REPLIED => match generic.get(&event.obj) {
                Some(request) => (request.data.target.clone(), event.data["next"].as_str(), None),
                None => continue,
            },
            _ => continue,
        };
        let Some(next) = next.map(normalize_question).filter(|next| !next.is_empty()) else {
            streaks.remove(&target);
            continue;
        };
        let streak = streaks.entry(target.clone()).or_insert_with(|| (next.clone(), 0));
        if streak.0 != next {
            *streak = (next.clone(), 0);
        }
        streak.1 += 1;
        if streak.1 != limit || !recent(parse_time(&event.t)?, now) {
            continue;
        }
        let attempt = match &attempt_id {
            Some(id) => self::attempt(items, &target, id),
            None => running_at(items, &target, event.seq),
        };
        found.push(Trigger {
            rule: Rule::SameNext, target: target.clone(), source: event.eid.clone(), attempt: attributed(attempt),
            title: format!("兜圈子：连续 {limit} 次下一步相同"),
            detail: format!("{target} 连续 {limit} 次汇报的下一步基本相同：「{}」", event.data["data"]["next"].as_str()
                .or(event.data["next"].as_str()).unwrap_or_default()),
            value: json!(limit), threshold: json!(limit),
        });
    }
    Ok(found)
}

struct Run {
    at: DateTime<FixedOffset>,
    id: String,
    passed: bool,
    attempt: Option<String>,
}

/// Rule 2: per object and check, in time order; an errored run fails too.
fn check_failures(state: &Value, items: &[KitDelegation], limit: u32, now: DateTime<FixedOffset>) -> Result<Vec<Trigger>, String> {
    let mut runs: BTreeMap<(String, String), Vec<Run>> = BTreeMap::new();
    for item in items {
        for evidence in &item.evidence {
            let Some(check) = &evidence.check else { continue };
            runs.entry((item.id.clone(), check.clone())).or_default().push(Run {
                at: parse_time(&evidence.at)?, id: evidence.id.clone(), passed: evidence.passed == Some(true),
                attempt: Some(evidence.attempt.clone()).filter(|id| !id.is_empty()),
            });
        }
    }
    for (id, record) in gates::records(state)? {
        if let Recorded::Check { object, check, passed, .. } = record.data {
            if items.iter().any(|item| item.id == object) {
                continue; // Already joined the object's evidence.
            }
            runs.entry((object, check)).or_default().push(Run {
                at: parse_time(&record.event.at)?, id, passed: passed == Some(true), attempt: None,
            });
        }
    }
    let mut found = Vec::new();
    for ((object, check), mut list) in runs {
        list.sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
        let mut failures = 0;
        for run in list {
            failures = if run.passed { 0 } else { failures + 1 };
            if failures != limit || !recent(run.at, now) {
                continue;
            }
            let attempt = run.attempt.as_deref().and_then(|id| attempt(items, &object, id));
            found.push(Trigger {
                rule: Rule::CheckFailures, target: object.clone(), source: run.id, attempt: attributed(attempt),
                title: format!("兜圈子：{check} 连续失败 {limit} 次"),
                detail: format!("{object} 的检查 {check} 连续 {limit} 次未通过"),
                value: json!(limit), threshold: json!(limit),
            });
        }
    }
    Ok(found)
}

/// Rule 3: questions an object's runs asked, by when they were asked.
fn asks(state: &Value, items: &[KitDelegation], thresholds: &Thresholds, now: DateTime<FixedOffset>) -> Result<Vec<Trigger>, String> {
    let window = i64::from(thresholds.ask_window_hours) * 60 * 60 * 1000;
    let mut by_object: BTreeMap<String, Vec<(u64, String, String)>> = BTreeMap::new();
    for (id, question) in question_projection::read(state)?.holds {
        by_object.entry(question.binding.task.clone()).or_default().push((question.opened_at, id, question.binding.run.clone()));
    }
    let limit = thresholds.asks + 1;
    let mut found = Vec::new();
    for (object, mut list) in by_object {
        list.sort();
        for (index, (opened, id, run)) in list.iter().enumerate() {
            let within = list[..=index].iter().filter(|(at, _, _)| (*opened as i64) - (*at as i64) < window).count();
            if within != limit as usize || !recent(time(*opened)?, now) {
                continue;
            }
            found.push(Trigger {
                rule: Rule::Asks, target: object.clone(), source: id.clone(), attempt: attributed(attempt(items, &object, run)),
                title: format!("兜圈子：{} 小时内提问 {limit} 次", thresholds.ask_window_hours),
                detail: format!("{object} 在 {} 小时内提问 {limit} 次，超过 {} 次", thresholds.ask_window_hours, thresholds.asks),
                value: json!(limit), threshold: json!(thresholds.asks),
            });
        }
    }
    Ok(found)
}

fn span(from: &str, to: &str) -> Result<i64, String> {
    Ok((parse_time(to)? - parse_time(from)?).num_milliseconds())
}

fn median(mut values: Vec<i64>) -> i64 {
    values.sort_unstable();
    let count = values.len();
    if count % 2 == 1 { values[count / 2] } else { (values[count / 2 - 1] + values[count / 2]) / 2 }
}

fn hours(ms: i64) -> String {
    format!("{}", (ms as f64 / 3_600_000.0 * 10.0).round() / 10.0)
}

/// Rule 4: an open attempt against finished ones of the same work kind.
fn duration(state: &Value, items: &[KitDelegation], factor: f64, now: DateTime<FixedOffset>) -> Result<Vec<Trigger>, String> {
    let log = delegations::read(state)?;
    let mut finished: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for item in items {
        for attempt in &item.attempts {
            if let Some(end) = attempt.delivered.as_ref().or(attempt.done.as_ref()) {
                finished.entry(log.work_kind(&item.work_kind)).or_default().push(span(&attempt.delegated.at, &end.at)?);
            }
        }
    }
    let mut found = Vec::new();
    for item in items {
        let kind = log.work_kind(&item.work_kind);
        let Some(samples) = finished.get(&kind).filter(|samples| samples.len() >= MIN_SAMPLES) else { continue };
        let median = median(samples.clone());
        for attempt in item.attempts.iter().filter(|attempt| attempt.active && attempt.delivered.is_none() && attempt.done.is_none()) {
            let elapsed = (now - parse_time(&attempt.delegated.at)?).num_milliseconds();
            if median <= 0 || (elapsed as f64) <= factor * median as f64 {
                continue;
            }
            found.push(Trigger {
                rule: Rule::Duration, target: item.id.clone(), source: attempt.id.clone(), attempt: attributed(Some(attempt)),
                title: format!("兜圈子：用时超过同类中位数 {factor} 倍"),
                detail: format!("{} 已用 {} 小时，同类 {kind} 的中位数 {} 小时", item.id, hours(elapsed), hours(median)),
                value: json!(elapsed), threshold: json!(factor * median as f64),
            });
        }
    }
    Ok(found)
}

/// Every looping the events show at `now`, already raised or not; the
/// daemon skips the ones whose hub ID is already recorded.
pub fn detect(events: &[&Event], state: &Value, kits: &[&dyn Kit], thresholds: &Thresholds, now_ms: u64) -> Result<Vec<Trigger>, String> {
    let now = time(now_ms)?;
    let items = delegations::delegations(state, kits)?;
    let mut events = events.to_vec();
    events.sort_by_key(|event| event.seq);
    let mut found = same_next(&events, state, &items, thresholds.same_next, now)?;
    found.extend(check_failures(state, &items, thresholds.check_failures, now)?);
    found.extend(asks(state, &items, thresholds, now)?);
    found.extend(duration(state, &items, thresholds.duration_factor, now)?);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{delegation::{Brief, Evidence, Phase, SourceKind, Stamp}, question_model::SelfReported};

    const NOW: &str = "2026-10-23T12:00:00Z";
    fn ms(at: &str) -> u64 {
        parse_time(at).unwrap().timestamp_millis() as u64
    }
    fn event(seq: u64, kind: &str, obj: &str, t: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("e{seq}"),"host":"h","t":t,"actor":"process:towermd-cli",
            "type":kind,"layer":"L5","obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }
    fn report(seq: u64, target: &str, next: Option<&str>, t: &str) -> Event {
        let mut data = json!({"state":"in_progress","summary":"s"});
        if let Some(next) = next { data["next"] = json!(next); }
        event(seq, "report.received", target, t, json!({"v":1,"project":"p","from":{"id":"a/1","kind":"delegate"},
            "target":target,"verb":"report","data":data,"at":t}))
    }
    fn stamp(seq: u64, at: &str) -> Stamp {
        Stamp { at: at.into(), eid: format!("s{seq}"), seq }
    }
    fn attempt(id: &str, seq: u64, at: &str, delivered: Option<&str>) -> KitAttempt {
        KitAttempt {
            id: id.into(), delegate: Some(SelfReported { name: "a".into(), version: "1".into(), ..Default::default() }),
            delegated: stamp(seq, at), delivered: delivered.map(|at| stamp(seq + 1, at)), done: None,
            active: delivered.is_none(), version: None, out_of_bounds: None,
        }
    }
    fn item(id: &str, kind: &str, attempts: Vec<KitAttempt>, evidence: Vec<Evidence>) -> KitDelegation {
        KitDelegation {
            id: id.into(), title: id.into(), brief: Brief::default(), work_kind: kind.into(), phase: Phase::Running,
            human_can_complete: true, attempts, evidence, checks: Vec::new(), facets: Default::default(),
        }
    }
    struct Fixed(Vec<KitDelegation>);
    impl Kit for Fixed {
        fn delegations(&self, _: &Value) -> Result<Vec<KitDelegation>, String> { Ok(self.0.clone()) }
    }

    #[test]
    fn the_same_next_three_times_in_a_row_fires_once_on_the_third() {
        let events = [
            report(1, "T-1", Some("修测试"), "2026-10-23T08:00:00Z"),
            report(2, "T-1", Some("修测试。 "), "2026-10-23T09:00:00Z"),
            report(3, "T-2", Some("修测试"), "2026-10-23T09:30:00Z"),
            report(4, "T-1", Some("修测试"), "2026-10-23T10:00:00Z"),
            report(5, "T-1", Some("修测试"), "2026-10-23T11:00:00Z"),
        ];
        let refs: Vec<&Event> = events.iter().collect();
        let kit = Fixed(vec![item("T-1", "code", vec![attempt("A-1", 0, "2026-10-23T00:00:00Z", None)], vec![])]);
        let found = detect(&refs, &json!({}), &[&kit], &Thresholds::default(), ms(NOW)).unwrap();
        assert_eq!(found.iter().map(|t| (t.rule, t.target.as_str(), t.source.as_str())).collect::<Vec<_>>(),
            [(Rule::SameNext, "T-1", "e4")]);
        assert_eq!(found[0].attempt.as_ref().map(|a| a.0.as_str()), Some("A-1"));
        assert!(found[0].refs().iter().any(|r| r == "delegation:T-1"));
        assert!(found[0].signal().check().is_ok());
        // A report without `next` or a different one resets the streak.
        let broken = [report(1, "T-1", Some("x"), "2026-10-23T08:00:00Z"), report(2, "T-1", None, "2026-10-23T09:00:00Z"),
            report(3, "T-1", Some("x"), "2026-10-23T10:00:00Z"), report(4, "T-1", Some("y"), "2026-10-23T10:30:00Z"),
            report(5, "T-1", Some("x"), "2026-10-23T11:00:00Z")];
        let refs: Vec<&Event> = broken.iter().collect();
        assert!(detect(&refs, &json!({}), &[], &Thresholds::default(), ms(NOW)).unwrap().is_empty());
        // Older than 24 hours: history, not news.
        let old = [report(1, "T-1", Some("x"), "2026-10-20T08:00:00Z"), report(2, "T-1", Some("x"), "2026-10-20T09:00:00Z"),
            report(3, "T-1", Some("x"), "2026-10-20T10:00:00Z")];
        let refs: Vec<&Event> = old.iter().collect();
        assert!(detect(&refs, &json!({}), &[], &Thresholds::default(), ms(NOW)).unwrap().is_empty());
        // A higher threshold waits for more.
        let loose = Thresholds { same_next: 4, ..Thresholds::default() };
        let refs: Vec<&Event> = events.iter().collect();
        assert_eq!(detect(&refs, &json!({}), &[], &loose, ms(NOW)).unwrap()[0].source, "e5");
    }

    fn check(id: &str, at: &str, passed: Option<bool>) -> Evidence {
        Evidence { id: id.into(), source_kind: SourceKind::Check, criteria: vec![], passed, version: None, actor: "x".into(),
            at: at.into(), note: None, attempt: "A-1".into(), check: Some("eng.test".into()) }
    }

    #[test]
    fn a_check_failing_three_times_in_a_row_fires_and_a_pass_resets() {
        let evidence = vec![
            check("c1", "2026-10-23T08:00:00Z", Some(false)), check("c2", "2026-10-23T09:00:00Z", Some(true)),
            check("c3", "2026-10-23T10:00:00Z", Some(false)), check("c4", "2026-10-23T10:30:00Z", None),
            check("c5", "2026-10-23T11:00:00Z", Some(false)),
        ];
        let kit = Fixed(vec![item("T-1", "code", vec![attempt("A-1", 0, "2026-10-23T00:00:00Z", None)], evidence)]);
        let found = detect(&[], &json!({}), &[&kit], &Thresholds::default(), ms(NOW)).unwrap();
        assert_eq!(found.iter().map(|t| (t.rule, t.source.as_str())).collect::<Vec<_>>(), [(Rule::CheckFailures, "c5")]);
        assert!(found[0].title.contains("eng.test"));
        assert_eq!(found[0].eid(), found[0].clone().eid(), "derived from rule and source");
    }

    #[test]
    fn more_than_three_asks_in_the_window_fires_on_the_fourth() {
        let hold = |id: &str, at: &str| (id.to_owned(), json!({"draft":{"id":id,"text":"q"},
            "binding":{"run":"A-1","task":"T-1","module":"m"},"fingerprint":"f","opened_at":ms(at),"deadline":ms(at)+1,"targets":[]}));
        let holds: serde_json::Map<String, Value> = [hold("h1", "2026-10-23T01:00:00Z"), hold("h2", "2026-10-23T02:00:00Z"),
            hold("h3", "2026-10-23T03:00:00Z"), hold("h4", "2026-10-23T04:00:00Z")].into_iter().collect();
        let state = json!({"Hold": holds});
        let found = detect(&[], &state, &[], &Thresholds::default(), ms(NOW)).unwrap();
        assert_eq!(found.iter().map(|t| (t.rule, t.source.as_str())).collect::<Vec<_>>(), [(Rule::Asks, "h4")]);
        let wider = Thresholds { asks: 4, ..Thresholds::default() };
        assert!(detect(&[], &state, &[], &wider, ms(NOW)).unwrap().is_empty());
        let narrow = Thresholds { ask_window_hours: 2, ..Thresholds::default() };
        assert!(detect(&[], &state, &[], &narrow, ms(NOW)).unwrap().is_empty());
    }

    #[test]
    fn an_attempt_past_three_times_the_median_of_its_kind_fires() {
        let done = |id: &str, hours: u32| item(id, "code", vec![attempt(&format!("A-{id}"), 1, "2026-10-20T00:00:00Z",
            Some(&format!("2026-10-20T{hours:02}:00:00Z")))], vec![]);
        let running = |at: &str| item("T-9", "code", vec![attempt("A-9", 9, at, None)], vec![]);
        let kit = Fixed(vec![done("T-1", 1), done("T-2", 2), done("T-3", 3), running("2026-10-23T05:00:00Z")]);
        let found = detect(&[], &json!({}), &[&kit], &Thresholds::default(), ms(NOW)).unwrap();
        assert_eq!(found.iter().map(|t| (t.rule, t.target.as_str(), t.source.as_str())).collect::<Vec<_>>(),
            [(Rule::Duration, "T-9", "A-9")]);
        let fresh = Fixed(vec![done("T-1", 1), done("T-2", 2), done("T-3", 3), running("2026-10-23T07:00:00Z")]);
        assert!(detect(&[], &json!({}), &[&fresh], &Thresholds::default(), ms(NOW)).unwrap().is_empty());
        let few = Fixed(vec![done("T-1", 1), done("T-2", 2), running("2026-10-22T00:00:00Z")]);
        assert!(detect(&[], &json!({}), &[&few], &Thresholds::default(), ms(NOW)).unwrap().is_empty(), "too few samples");
    }
}
