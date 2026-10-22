//! Delegation read model (FR-185, FR-202–211): acceptance cards, the trust
//! ledger, configuration changes, spot checks, the review backlog, report
//! requests and level suggestions. A pure function of the projection and the
//! settings; never stored, so deleting it loses nothing.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    delegation::{fingerprint, parse_time, Brief, DelegationSettings, Evidence, KitDelegation, Phase, Stamp},
    delegation_projection::{
        self as projection, attempt_evidence, report_requests, unproven, Clarified, DelegationLog, Entry, Mark,
        ObjectLog, Overdue, Replied, Verdict,
    },
    kit::Kit,
    question_model::SelfReported,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DelegationView {
    pub objects: Vec<ObjectView>,
    pub track_record: Vec<TrackRecord>,
    pub configs: Vec<ConfigHistory>,
    /// Selected spot checks only, oldest first.
    pub spot_checks: Vec<SpotCheck>,
    /// Present only above the threshold (FR-210).
    pub backlog: Option<Backlog>,
    pub report_requests: Vec<RequestView>,
    pub rules: Vec<RuleView>,
    /// Old work kind name to new name, as the person set them.
    pub renames: BTreeMap<String, String>,
    /// Explicit level settings, latest per (config, work kind).
    pub levels: Vec<LevelRow>,
    pub settings: DelegationSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectView {
    pub id: String,
    pub title: String,
    pub phase: Phase,
    /// After renames; `default_work_kind` is the kit's container name.
    pub work_kind: String,
    pub default_work_kind: String,
    pub brief: Brief,
    /// `human` once a person finalized the card (FR-201).
    pub brief_set_by: Option<String>,
    pub clarification: Option<ClarificationView>,
    pub human_can_complete: bool,
    pub attempts: Vec<AttemptView>,
    /// The latest delegation's acceptance card.
    pub card: Option<Card>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClarificationView {
    pub draft: Clarified,
    pub event: Mark,
    /// Answers from the latest final card, by question.
    pub answers: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoneBy { Check, Human }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttemptView {
    pub id: String,
    pub config: String,
    pub delegate: Option<SelfReported>,
    pub delegated: Stamp,
    pub delivered: Option<Stamp>,
    pub done: Option<Stamp>,
    pub done_by: Option<DoneBy>,
    pub active: bool,
    pub version: Option<String>,
    pub out_of_bounds: Option<u32>,
    pub verdict: Option<VerdictView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind { Confirmed, SentBack }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerdictView {
    pub kind: VerdictKind,
    pub event: Mark,
    pub review_ms: u64,
    pub unproven: Vec<usize>,
    pub reason: Option<String>,
    pub takeaway: Option<Takeaway>,
}

/// FR-206: one sentence after confirming, kept as the person's decision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Takeaway {
    pub text: String,
    pub set_by: String,
}

/// FR-208: one row per completion criterion; whole-object checks apart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub attempt: String,
    pub version: Option<String>,
    pub rows: Vec<CardRow>,
    pub whole: Vec<Evidence>,
    pub unproven: Vec<usize>,
    /// Whether the person may confirm or send back now.
    pub can_judge: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardRow {
    pub index: usize,
    pub text: String,
    pub evidence: Vec<Evidence>,
    pub unproven: bool,
}

/// Count, median and total of one measure; `median` is null with no values.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measure {
    pub count: usize,
    pub median: Option<u64>,
    pub total: u64,
}

/// Part of a whole; `rate` is null when nothing counts toward it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ratio {
    pub count: usize,
    pub total: usize,
    pub rate: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelSource { Exact, Config, WorkKind, Project, Default }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction { Up, Down }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub direction: Direction,
    pub level: String,
}

/// FR-202: one (config, work kind) group over its latest window of finished
/// delegations. No total score. Undetected measures are null.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackRecord {
    pub config: String,
    pub delegate: Option<SelfReported>,
    pub work_kind: String,
    pub records: usize,
    pub first_pass: Ratio,
    pub on_time: Ratio,
    pub loops: Option<u32>,
    pub weakened: Option<u32>,
    pub met_done_when: Ratio,
    pub out_of_bounds: Option<u32>,
    pub delegate_to_deliver_ms: Measure,
    /// Measured in the App only.
    pub review_ms: Measure,
    pub sent_back: Measure,
    /// The same delegate had another configuration before this one (FR-203).
    pub config_changed: bool,
    /// Rate the next finished delegation is drawn with.
    pub spot_check_rate: f64,
    pub level: String,
    pub level_source: LevelSource,
    pub suggestion: Option<Suggestion>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigHistory {
    pub name: String,
    /// In order of first delegation.
    pub configs: Vec<ConfigSeen>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigSeen {
    pub config: String,
    pub delegate: SelfReported,
    pub first: Stamp,
    pub delegations: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpotCheck {
    pub object: String,
    pub attempt: String,
    pub config: String,
    pub work_kind: String,
    pub done: Stamp,
    pub rate: f64,
    pub draw: f64,
    /// A confirmation or send-back of the delegation resolves it.
    pub resolved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backlog {
    pub count: usize,
    pub threshold: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestView {
    #[serde(flatten)]
    pub request: projection::ReportRequest,
    pub reply: Option<Entry<Replied>>,
    pub overdue: Option<Entry<Overdue>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleView {
    pub id: String,
    pub work_kind: String,
    pub text: String,
    pub source: Option<String>,
    pub event: Mark,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelRow {
    pub config: String,
    pub work_kind: String,
    pub level: String,
    pub event: Mark,
}

/// A finished delegation as the ledger counts it.
struct Record<'a> {
    object: &'a KitDelegation,
    attempt: &'a AttemptView,
    config: String,
    work_kind: String,
    done: Stamp,
}

pub fn view(state: &Value, kits: &[&dyn Kit], settings: &DelegationSettings) -> Result<DelegationView, String> {
    let items = projection::delegations(state, kits)?;
    let log = projection::read(state)?;
    let empty = ObjectLog::default();
    let objects: Vec<ObjectView> = items.iter()
        .map(|item| object_view(item, log.objects.get(&item.id).unwrap_or(&empty), &log)).collect();

    let mut records: Vec<Record> = items.iter().zip(&objects).flat_map(|(item, object)| {
        object.attempts.iter().filter_map(move |attempt| attempt.done.clone().map(|done| Record {
            object: item, attempt, config: attempt.config.clone(), work_kind: object.work_kind.clone(), done,
        }))
    }).collect();
    records.sort_by_key(|record| record.done.seq);

    let spot_checks = spot_checks(&records, settings);
    let levels = level_rows(&log);
    let configs = config_history(&objects);
    let mut groups: BTreeMap<(String, String), Vec<&Record>> = BTreeMap::new();
    for record in &records {
        groups.entry((record.config.clone(), record.work_kind.clone())).or_default().push(record);
    }
    let track_record = groups.into_iter().map(|((config, work_kind), group)| {
        let changed = configs.iter().any(|history| history.configs.iter().skip(1).any(|seen| seen.config == config));
        track(config, work_kind, &group, changed, &levels, settings)
    }).collect();

    let pending = objects.iter().filter(|object| object.human_can_complete && object.phase == Phase::ClaimedDone
        && object.card.as_ref().is_some_and(|card| card.can_judge)).count();
    let open_spot_checks = spot_checks.iter().filter(|check| !check.resolved).count();
    let proposals = state.get("Proposal").and_then(Value::as_object)
        .map_or(0, |group| group.values().filter(|proposal| proposal["status"] == "submitted").count());
    let count = pending + open_spot_checks + proposals;
    let backlog = (count > settings.review_backlog).then_some(Backlog { count, threshold: settings.review_backlog });

    let report_requests = report_requests(&items, &log).into_iter().map(|request| {
        let report = log.reports.get(&request.id).cloned().unwrap_or_default();
        RequestView { request, reply: report.replied, overdue: report.overdue }
    }).collect();
    let rules = log.rules.iter().map(|(id, entry)| RuleView {
        id: id.clone(), work_kind: log.work_kind(&entry.data.kind), text: entry.data.text.clone(),
        source: entry.data.source.clone(), event: entry.event.clone(),
    }).collect();
    let renames = log.renames.iter().map(|(from, entry)| (from.clone(), entry.data.to.clone())).collect();
    Ok(DelegationView { objects, track_record, configs, spot_checks, backlog, report_requests, rules, renames, levels,
        settings: settings.clone() })
}

fn object_view(item: &KitDelegation, log: &ObjectLog, all: &DelegationLog) -> ObjectView {
    let finalized = log.finalized.last();
    let clarification = log.clarifications.last().map(|entry| ClarificationView {
        draft: entry.data.clone(), event: entry.event.clone(),
        answers: finalized.filter(|card| card.event.seq > entry.event.seq)
            .map(|card| card.data.answers.clone()).unwrap_or_default(),
    });
    let attempts: Vec<AttemptView> = item.attempts.iter().map(|attempt| {
        let verdict = log.verdicts.iter().find(|entry| entry.attempt == attempt.id).map(|entry| match &entry.data {
            Verdict::Confirmed(data) => VerdictView {
                kind: VerdictKind::Confirmed, event: entry.event.clone(), review_ms: data.review_ms,
                unproven: data.unproven.clone(), reason: None,
                takeaway: data.takeaway.clone().map(|text| Takeaway { text, set_by: "human".into() }),
            },
            Verdict::SentBack(data) => VerdictView {
                kind: VerdictKind::SentBack, event: entry.event.clone(), review_ms: data.review_ms,
                unproven: vec![], reason: Some(data.reason.clone()), takeaway: None,
            },
        });
        // Where a person completes the object, the confirmation is the completion.
        let confirmed = verdict.as_ref().filter(|verdict| item.human_can_complete && verdict.kind == VerdictKind::Confirmed);
        let (done, done_by) = match (&attempt.done, confirmed) {
            (Some(done), _) => (Some(done.clone()), Some(DoneBy::Check)),
            (None, Some(verdict)) => (Some(verdict.event.stamp()), Some(DoneBy::Human)),
            (None, None) => (None, None),
        };
        AttemptView {
            id: attempt.id.clone(), config: fingerprint(attempt.delegate.as_ref()), delegate: attempt.delegate.clone(),
            delegated: attempt.delegated.clone(), delivered: attempt.delivered.clone(), done, done_by,
            active: attempt.active, version: attempt.version.clone(), out_of_bounds: attempt.out_of_bounds, verdict,
        }
    }).collect();
    let card = attempts.last().map(|attempt| {
        let evidence = attempt_evidence(item, Some(log), &attempt.id);
        let unproven = unproven(&item.brief, &evidence);
        let rows = item.brief.done_when.iter().enumerate().map(|(index, text)| CardRow {
            index, text: text.clone(), unproven: unproven.contains(&index),
            evidence: evidence.iter().filter(|entry| entry.criteria.contains(&index)).cloned().collect(),
        }).collect();
        let phase_allows = item.phase == Phase::Done || item.phase == Phase::ClaimedDone && item.human_can_complete;
        Card {
            attempt: attempt.id.clone(), version: attempt.version.clone(), rows,
            whole: evidence.iter().filter(|entry| entry.criteria.is_empty()).cloned().collect(),
            can_judge: phase_allows && attempt.verdict.is_none(), unproven,
        }
    });
    ObjectView {
        id: item.id.clone(), title: item.title.clone(), phase: item.phase, work_kind: all.work_kind(&item.work_kind),
        default_work_kind: item.work_kind.clone(), brief: item.brief.clone(),
        brief_set_by: finalized.map(|_| "human".to_owned()), clarification,
        human_can_complete: item.human_can_complete, attempts, card,
    }
}

fn first_pass(record: &Record, before: u64) -> bool {
    !record.attempt.verdict.as_ref()
        .is_some_and(|verdict| verdict.kind == VerdictKind::SentBack && verdict.event.seq < before)
}

/// The group's latest records finished before `seq`, at most `window`.
fn prior<'a>(records: &'a [Record<'a>], config: &str, kind: &str, seq: u64, window: usize) -> Vec<&'a Record<'a>> {
    let mut found: Vec<_> = records.iter()
        .filter(|record| record.config == config && record.work_kind == kind && record.done.seq < seq).collect();
    let skip = found.len().saturating_sub(window);
    found.drain(..skip);
    found
}

/// FR-204: `rate = max(min, 1 − first passes / max(records, 10))` over what
/// was known when the delegation finished.
fn rate(prior: &[&Record], seq: u64, settings: &DelegationSettings) -> f64 {
    let passes = prior.iter().filter(|record| first_pass(record, seq)).count();
    (1.0 - passes as f64 / prior.len().max(10) as f64).max(settings.spot_check_min)
}

/// First 8 bytes of sha256(object ID + completion eid) over 2^64.
pub fn draw(object: &str, completion: &str) -> f64 {
    let digest = Sha256::digest(format!("{object}{completion}").as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("8 bytes")) as f64 / 2f64.powi(64)
}

fn spot_checks(records: &[Record], settings: &DelegationSettings) -> Vec<SpotCheck> {
    records.iter().filter_map(|record| {
        let before = prior(records, &record.config, &record.work_kind, record.done.seq, settings.track_record_window);
        let rate = rate(&before, record.done.seq, settings);
        let draw = draw(&record.object.id, &record.done.eid);
        (draw < rate).then(|| SpotCheck {
            object: record.object.id.clone(), attempt: record.attempt.id.clone(), config: record.config.clone(),
            work_kind: record.work_kind.clone(), done: record.done.clone(), rate, draw,
            resolved: record.attempt.verdict.is_some(),
        })
    }).collect()
}

fn measure(mut values: Vec<u64>) -> Measure {
    values.sort_unstable();
    let count = values.len();
    let median = match count {
        0 => None,
        _ if count % 2 == 1 => Some(values[count / 2]),
        _ => Some((values[count / 2 - 1] + values[count / 2]) / 2),
    };
    Measure { count, median, total: values.iter().sum() }
}

fn ratio(count: usize, total: usize) -> Ratio {
    Ratio { count, total, rate: (total > 0).then(|| count as f64 / total as f64) }
}

fn millis(from: &str, to: &str) -> Option<u64> {
    let span = parse_time(to).ok()? - parse_time(from).ok()?;
    u64::try_from(span.num_milliseconds()).ok()
}

fn track(config: String, work_kind: String, group: &[&Record], config_changed: bool, levels: &[LevelRow],
    settings: &DelegationSettings) -> TrackRecord {
    let skip = group.len().saturating_sub(settings.track_record_window);
    let window = &group[skip..];
    let passes = window.iter().filter(|record| first_pass(record, u64::MAX)).count();
    let dated: Vec<_> = window.iter().filter_map(|record| {
        let due = record.object.brief.last_due()?;
        let delivered = record.attempt.delivered.as_ref().or(Some(&record.done))?;
        Some(parse_time(&delivered.at).is_ok_and(|at| at <= due))
    }).collect();
    let confirmed: Vec<_> = window.iter().filter_map(|record| record.attempt.verdict.as_ref())
        .filter(|verdict| verdict.kind == VerdictKind::Confirmed).collect();
    let out_of_bounds = window.iter().map(|record| record.attempt.out_of_bounds).sum::<Option<u32>>();
    let delegate_to_deliver = window.iter().filter_map(|record| {
        millis(&record.attempt.delegated.at, &record.attempt.delivered.as_ref()?.at)
    }).collect();
    let review = window.iter().filter_map(|record| record.attempt.verdict.as_ref().map(|verdict| verdict.review_ms)).collect();
    let sent_back = window.iter().map(|record| u64::from(!first_pass(record, u64::MAX))).collect();
    let (level, level_source) = effective_level(levels, &config, &work_kind, settings);
    let passed = ratio(passes, window.len());
    let suggestion = suggest(&level, passed.rate, window.len(), settings);
    TrackRecord {
        delegate: window.last().and_then(|record| record.attempt.delegate.clone()),
        config, work_kind, records: window.len(), first_pass: passed,
        on_time: ratio(dated.iter().filter(|on_time| **on_time).count(), dated.len()),
        loops: None, weakened: None,
        met_done_when: ratio(confirmed.iter().filter(|verdict| verdict.unproven.is_empty()).count(), confirmed.len()),
        out_of_bounds, delegate_to_deliver_ms: measure(delegate_to_deliver), review_ms: measure(review),
        sent_back: measure(sent_back), config_changed,
        spot_check_rate: rate(window, u64::MAX, settings), level, level_source, suggestion,
    }
}

fn level_rows(log: &DelegationLog) -> Vec<LevelRow> {
    let mut latest: BTreeMap<(String, String), LevelRow> = BTreeMap::new();
    for entry in &log.levels {
        let kind = if entry.data.kind == "*" { "*".to_owned() } else { log.work_kind(&entry.data.kind) };
        latest.insert((entry.data.config.clone(), kind.clone()), LevelRow {
            config: entry.data.config.clone(), work_kind: kind, level: entry.data.level.clone(), event: entry.event.clone(),
        });
    }
    latest.into_values().collect()
}

/// FR-185: exact setting, then the config's, the work kind's, the project's,
/// and finally the first level on the list.
pub fn effective_level(levels: &[LevelRow], config: &str, kind: &str, settings: &DelegationSettings) -> (String, LevelSource) {
    let order = [(config, kind, LevelSource::Exact), (config, "*", LevelSource::Config),
        ("*", kind, LevelSource::WorkKind), ("*", "*", LevelSource::Project)];
    for (config, kind, source) in order {
        if let Some(row) = levels.iter().find(|row| row.config == config && row.work_kind == kind) {
            return (row.level.clone(), source);
        }
    }
    (settings.delegation_levels[0].clone(), LevelSource::Default)
}

/// Only with a full window: ≥ 0.9 first pass suggests one level up, ≤ 0.5 one
/// down. Never applied by itself.
fn suggest(level: &str, rate: Option<f64>, records: usize, settings: &DelegationSettings) -> Option<Suggestion> {
    if records < settings.track_record_window {
        return None;
    }
    let position = settings.delegation_levels.iter().position(|known| known == level)?;
    let (direction, target) = match rate? {
        rate if rate >= 0.9 => (Direction::Up, position.checked_add(1)?),
        rate if rate <= 0.5 => (Direction::Down, position.checked_sub(1)?),
        _ => return None,
    };
    settings.delegation_levels.get(target).map(|level| Suggestion { direction, level: level.clone() })
}

fn config_history(objects: &[ObjectView]) -> Vec<ConfigHistory> {
    let mut attempts: Vec<&AttemptView> = objects.iter().flat_map(|object| &object.attempts).collect();
    attempts.sort_by_key(|attempt| attempt.delegated.seq);
    let mut histories: BTreeMap<String, Vec<ConfigSeen>> = BTreeMap::new();
    for attempt in attempts {
        let Some(delegate) = &attempt.delegate else { continue };
        let seen = histories.entry(delegate.name.clone()).or_default();
        match seen.iter_mut().find(|seen| seen.config == attempt.config) {
            Some(seen) => seen.delegations += 1,
            None => seen.push(ConfigSeen { config: attempt.config.clone(), delegate: delegate.clone(),
                first: attempt.delegated.clone(), delegations: 1 }),
        }
    }
    histories.into_iter().map(|(name, configs)| ConfigHistory { name, configs }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegation::{KitAttempt, SourceKind};
    use crate::delegation_projection::{AttemptEntry, Confirmed, Finalized, LevelSet, Renamed, SentBack, Used};
    use serde_json::json;

    /// Reads its delegations from `state.Fake`.
    struct Fake;
    impl Kit for Fake {
        fn delegations(&self, state: &Value) -> Result<Vec<KitDelegation>, String> {
            serde_json::from_value(state["Fake"].clone()).map_err(|e| e.to_string())
        }
    }

    fn stamp(seq: u64) -> Stamp {
        Stamp { at: format!("2026-10-22T10:{seq:02}:00Z"), eid: format!("e-{seq}"), seq }
    }
    fn mark(seq: u64) -> Mark {
        let stamp = stamp(seq);
        Mark { at: stamp.at, eid: stamp.eid, seq, actor: "human:local".into() }
    }
    fn attempt(id: &str, model: &str, start: u64, done: Option<u64>) -> KitAttempt {
        KitAttempt { id: id.into(), delegate: Some(SelfReported { name: "agent".into(), version: "1".into(),
                model: Some(model.into()), ..SelfReported::default() }),
            delegated: stamp(start), delivered: done.map(|seq| stamp(seq - 1)), done: done.map(stamp),
            active: done.is_none(), version: Some(format!("v-{id}")), out_of_bounds: None }
    }
    fn object(id: &str, kind: &str, phase: Phase, attempts: Vec<KitAttempt>) -> KitDelegation {
        KitDelegation { id: id.into(), title: id.into(), work_kind: kind.into(), phase, human_can_complete: false,
            brief: Brief { done_when: vec!["a".into(), "b".into()], ..Brief::default() }, attempts, evidence: vec![] }
    }
    fn verdict(attempt: &str, seq: u64, data: Verdict) -> AttemptEntry<Verdict> {
        AttemptEntry { event: mark(seq), attempt: attempt.into(), data }
    }
    fn confirmed(review_ms: u64, unproven: Vec<usize>) -> Verdict {
        Verdict::Confirmed(Confirmed { review_ms, unproven, takeaway: None })
    }
    fn render(items: &[KitDelegation], log: &DelegationLog, settings: &DelegationSettings) -> DelegationView {
        let state = json!({"Fake": items, "Delegation": log, "Proposal": {}});
        view(&state, &[&Fake], settings).unwrap()
    }

    #[test]
    fn the_card_lists_evidence_per_criterion_and_marks_rows_without_any() {
        let mut item = object("T-1", "A", Phase::Done, vec![attempt("A1", "m", 1, Some(3))]);
        let entry = |id: &str, kind, criteria: Vec<usize>| Evidence { id: id.into(), source_kind: kind, criteria,
            passed: None, version: Some("v-A1".into()), actor: "agent/1".into(), at: stamp(2).at, note: None, attempt: "A1".into() };
        item.evidence = vec![Evidence { passed: Some(true), ..entry("E", SourceKind::Check, vec![]) },
            entry("E#claim-0", SourceKind::Claimed, vec![0])];
        let mut log = DelegationLog::default();
        let object_log = log.objects.entry("T-1".into()).or_default();
        object_log.clarifications.push(Entry { event: mark(0), data: Clarified { restated: "r".into(),
            done_when: vec![], questions: vec!["q?".into()], self_reported: None } });
        object_log.finalized.push(Entry { event: mark(1), data: Finalized { brief: item.brief.clone(), answers: vec!["yes".into()] } });
        let found = render(&[item.clone()], &log, &DelegationSettings::default());
        let object = &found.objects[0];
        let card = object.card.as_ref().unwrap();
        assert_eq!((card.unproven.clone(), card.can_judge, card.whole.len()), (vec![1], true, 1));
        assert_eq!((card.rows[0].evidence[0].source_kind, card.rows[1].unproven), (SourceKind::Claimed, true));
        assert_eq!((object.brief_set_by.as_deref(), object.clarification.as_ref().unwrap().answers.clone()),
            (Some("human"), vec!["yes".to_owned()]));

        let object_log = log.objects.get_mut("T-1").unwrap();
        object_log.used.push(AttemptEntry { event: mark(4), attempt: "A1".into(), data: Used { criteria: vec![1], note: None } });
        object_log.verdicts.push(verdict("A1", 5, Verdict::Confirmed(Confirmed { review_ms: 7, unproven: vec![],
            takeaway: Some("checks were enough".into()) })));
        let found = render(&[item], &log, &DelegationSettings::default());
        let card = found.objects[0].card.as_ref().unwrap();
        assert_eq!((card.unproven.is_empty(), card.can_judge), (true, false));
        assert_eq!((card.rows[1].evidence[0].source_kind, card.rows[1].evidence[0].version.as_deref()),
            (SourceKind::Used, Some("v-A1")));
        let takeaway = found.objects[0].attempts[0].verdict.as_ref().unwrap().takeaway.clone().unwrap();
        assert_eq!((takeaway.text.as_str(), takeaway.set_by.as_str()), ("checks were enough", "human"));
    }

    #[test]
    fn the_ledger_groups_by_config_and_renamed_work_kind_with_unknown_measures_null() {
        let items = [
            object("T-1", "A", Phase::Done, vec![attempt("A1", "m1", 1, Some(3)), attempt("A2", "m1", 5, Some(7))]),
            object("T-2", "B", Phase::Done, vec![attempt("A3", "m1", 8, Some(10))]),
            object("T-3", "A", Phase::Done, vec![attempt("A4", "m2", 11, Some(13))]),
            object("T-4", "A", Phase::Running, vec![attempt("A5", "m1", 14, None)]),
        ];
        let mut log = DelegationLog::default();
        log.renames.insert("B".into(), Entry { event: mark(0), data: Renamed { from: "B".into(), to: "A".into() } });
        let verdicts = &mut log.objects.entry("T-1".into()).or_default().verdicts;
        verdicts.push(verdict("A1", 4, Verdict::SentBack(SentBack { reason: "no".into(), review_ms: 1000 })));
        verdicts.push(verdict("A2", 8, confirmed(3000, vec![1])));
        let found = render(&items, &log, &DelegationSettings::default());
        assert_eq!(found.track_record.len(), 2);
        let row = found.track_record.iter().find(|row| row.config.contains("m1")).unwrap();
        assert_eq!((row.work_kind.as_str(), row.records, row.first_pass.count, row.first_pass.total), ("A", 3, 2, 3));
        assert_eq!((row.on_time.rate, row.loops, row.weakened, row.out_of_bounds), (None, None, None, None));
        assert_eq!((row.met_done_when.count, row.met_done_when.total), (0, 1));
        assert_eq!(row.review_ms, Measure { count: 2, median: Some(2000), total: 4000 });
        assert_eq!(row.delegate_to_deliver_ms, Measure { count: 3, median: Some(60_000), total: 180_000 });
        assert_eq!(row.sent_back, Measure { count: 3, median: Some(0), total: 1 });
        assert!(!row.config_changed && (row.spot_check_rate - 0.8).abs() < 1e-9);
        let newer = found.track_record.iter().find(|row| row.config.contains("m2")).unwrap();
        assert!(newer.config_changed && (newer.spot_check_rate - 0.9).abs() < 1e-9);
        let history = &found.configs[0];
        assert_eq!((history.name.as_str(), history.configs.len(), history.configs[0].delegations, history.configs[1].first.seq),
            ("agent", 2, 4, 11));

        // Each finished delegation is drawn against what was known then.
        let checks: Vec<_> = found.spot_checks.iter().map(|check| (check.attempt.as_str(), check.rate, check.resolved)).collect();
        let a3 = draw("T-2", "e-10") < 0.9;
        let mut expected = vec![("A1", 1.0, true), ("A2", 1.0, true)];
        if a3 { expected.push(("A3", 0.9, false)); }
        expected.push(("A4", 1.0, false));
        assert_eq!(checks.len(), expected.len());
        for ((attempt, rate, resolved), (want, want_rate, want_resolved)) in checks.into_iter().zip(expected) {
            assert_eq!((attempt, resolved), (want, want_resolved));
            assert!((rate - want_rate).abs() < 1e-9);
        }
        assert_eq!(found, render(&items, &log, &DelegationSettings::default()));

        let small = DelegationSettings { track_record_window: 2, ..DelegationSettings::default() };
        let row = render(&items, &log, &small).track_record.into_iter().find(|row| row.config.contains("m1")).unwrap();
        assert_eq!((row.records, row.first_pass.count), (2, 2));
    }

    #[test]
    fn rates_never_drop_below_the_minimum_and_levels_are_only_suggested() {
        let items: Vec<_> = (0..12u64).map(|index| object(&format!("T-{index}"), "A", Phase::Done,
            vec![attempt(&format!("A{index}"), "m", index * 3 + 1, Some(index * 3 + 3))])).collect();
        let settings = DelegationSettings { track_record_window: 12, ..DelegationSettings::default() };
        let row = &render(&items, &DelegationLog::default(), &settings).track_record[0];
        assert_eq!(row.spot_check_rate, 0.05);
        assert_eq!((row.level.as_str(), row.level_source), ("report_only", LevelSource::Default));
        assert_eq!(row.suggestion, Some(Suggestion { direction: Direction::Up, level: "answer_by_decision".into() }));
        let floor = DelegationSettings { spot_check_min: 0.01, ..settings.clone() };
        assert_eq!(render(&items, &DelegationLog::default(), &floor).track_record[0].spot_check_rate, 0.01);

        let mut log = DelegationLog::default();
        let config = row.config.clone();
        let level = |seq, config: &str, kind: &str, level: &str| Entry { event: mark(seq),
            data: LevelSet { config: config.into(), kind: kind.into(), level: level.into() } };
        log.levels.push(level(50, "*", "*", "approve_minor"));
        let row = render(&items, &log, &settings).track_record.remove(0);
        assert_eq!((row.level.as_str(), row.level_source, row.suggestion), ("approve_minor", LevelSource::Project, None));
        log.levels.push(level(51, &config, "A", "answer_by_decision"));
        for index in 0..6 {
            log.objects.entry(format!("T-{index}")).or_default().verdicts
                .push(verdict(&format!("A{index}"), 60 + index as u64, Verdict::SentBack(SentBack { reason: "r".into(), review_ms: 1 })));
        }
        let row = render(&items, &log, &settings).track_record.remove(0);
        assert_eq!((row.level.as_str(), row.level_source), ("answer_by_decision", LevelSource::Exact));
        assert_eq!(row.suggestion, Some(Suggestion { direction: Direction::Down, level: "report_only".into() }));
        let short = DelegationSettings { track_record_window: 13, ..settings };
        assert_eq!(render(&items, &log, &short).track_record[0].suggestion, None);
    }

    #[test]
    fn the_backlog_counts_claimed_objects_open_spot_checks_and_proposals_above_the_threshold() {
        let mut waiting = object("N-1", "notes", Phase::ClaimedDone, vec![attempt("A1", "m", 1, None)]);
        waiting.human_can_complete = true;
        let done = object("T-1", "A", Phase::Done, vec![attempt("A2", "m", 2, Some(4))]);
        let state = |threshold| {
            let state = json!({"Fake": [waiting.clone(), done.clone()], "Proposal": {
                "P-1": {"status": "submitted"}, "P-2": {"status": "draft"}}});
            view(&state, &[&Fake], &DelegationSettings { review_backlog: threshold, ..DelegationSettings::default() }).unwrap()
        };
        assert_eq!(state(2).backlog, Some(Backlog { count: 3, threshold: 2 }));
        assert_eq!(state(3).backlog, None);
        let found = state(3);
        let human = &found.objects[0];
        assert!(human.card.as_ref().unwrap().can_judge && human.attempts[0].done.is_none());
    }

    #[test]
    fn a_person_confirmation_completes_where_the_kit_allows_it() {
        let mut item = object("N-1", "notes", Phase::ClaimedDone, vec![attempt("A1", "m", 1, None)]);
        item.human_can_complete = true;
        let mut log = DelegationLog::default();
        log.objects.entry("N-1".into()).or_default().verdicts.push(verdict("A1", 3, confirmed(5, vec![0, 1])));
        let found = render(&[item], &log, &DelegationSettings::default());
        let attempt = &found.objects[0].attempts[0];
        assert_eq!((attempt.done.as_ref().map(|done| done.seq), attempt.done_by), (Some(3), Some(DoneBy::Human)));
        assert_eq!(found.track_record[0].met_done_when.rate, Some(0.0));
    }

    #[test]
    fn draws_are_fixed_by_the_object_and_completion_event() {
        assert_eq!(draw("T-1", "e-5"), 0.6350516469545595);
        assert!((0.0..1.0).contains(&draw("x", "y")));
    }
}
