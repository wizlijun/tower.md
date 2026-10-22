//! Delegation events (FR-185, FR-201–211): clarifying and finalizing the
//! card, acceptance, rules, work kind names, levels and report requests. The
//! `Delegation` group is created on first use, so older projections stay
//! byte-identical. Every check that depends on an object's phase asks the
//! kits; nothing here completes an object.
use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    delegation::{parse_time, Brief, Evidence, KitAttempt, KitDelegation, Phase, SourceKind, Stamp},
    hub::Event,
    kit::Kit,
    question_model::{human_actor, nonempty, valid_id, SelfReported},
};

pub const GROUP: &str = "Delegation";
pub const KINDS: [&str; 10] = [
    "delegation.clarified", "delegation.finalized", "acceptance.used", "acceptance.confirmed",
    "acceptance.sent_back", "rule.recorded", "work_kind.renamed", "delegation.level.set",
    "report.replied", "report.overdue",
];
/// FR-201: the delegate asks at most this many questions.
pub const MAX_QUESTIONS: usize = 3;
/// FR-115: a report summary has at most this many characters.
pub const SUMMARY_CHARS: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clarified {
    pub restated: String,
    #[serde(default)]
    pub done_when: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_reported: Option<SelfReported>,
}

/// `answers[i]` answers question `i` of the latest clarification; an empty
/// string leaves it unanswered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finalized {
    pub brief: Brief,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Used {
    pub criteria: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirmed {
    pub review_ms: u64,
    /// Criteria still without evidence; must match the card exactly.
    #[serde(default)]
    pub unproven: Vec<usize>,
    /// FR-206: kept as a decision set by the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeaway: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SentBack {
    pub reason: String,
    pub review_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleRecorded {
    pub kind: String,
    pub text: String,
    /// The send-back that prompted the rule, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Renamed {
    pub from: String,
    pub to: String,
}

/// `config` is a fingerprint or `*`; `kind` a work kind or `*`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelSet {
    pub config: String,
    pub kind: String,
    pub level: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportState { NotStarted, InProgress, Waiting, Blocked, ClaimedDone }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replied {
    pub state: ReportState,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_reported: Option<SelfReported>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverdueLevel { Active, Passive }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overdue {
    pub level: OverdueLevel,
}

/// An accepted event with its signer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub at: String,
    pub eid: String,
    pub seq: u64,
    pub actor: String,
}
impl Mark {
    fn of(event: &Event) -> Self {
        Self { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() }
    }
    pub fn stamp(&self) -> Stamp {
        Stamp { at: self.at.clone(), eid: self.eid.clone(), seq: self.seq }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry<T> {
    pub event: Mark,
    pub data: T,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict { Confirmed(Confirmed), SentBack(SentBack) }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptEntry<T> {
    pub event: Mark,
    pub attempt: String,
    pub data: T,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectLog {
    #[serde(default)]
    pub clarifications: Vec<Entry<Clarified>>,
    #[serde(default)]
    pub finalized: Vec<Entry<Finalized>>,
    #[serde(default)]
    pub used: Vec<AttemptEntry<Used>>,
    /// At most one per attempt.
    #[serde(default)]
    pub verdicts: Vec<AttemptEntry<Verdict>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportLog {
    pub replied: Option<Entry<Replied>>,
    pub overdue: Option<Entry<Overdue>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationLog {
    #[serde(default)]
    pub objects: BTreeMap<String, ObjectLog>,
    #[serde(default)]
    pub rules: BTreeMap<String, Entry<RuleRecorded>>,
    /// Latest rename per old name.
    #[serde(default)]
    pub renames: BTreeMap<String, Entry<Renamed>>,
    #[serde(default)]
    pub levels: Vec<Entry<LevelSet>>,
    #[serde(default)]
    pub reports: BTreeMap<String, ReportLog>,
}

impl DelegationLog {
    /// Follows the rename chain; renames never form a cycle.
    pub fn work_kind(&self, name: &str) -> String {
        let mut name = name.to_owned();
        for _ in 0..=self.renames.len() {
            match self.renames.get(&name) {
                Some(entry) => name = entry.data.to.clone(),
                None => break,
            }
        }
        name
    }
}

pub fn read(state: &Value) -> Result<DelegationLog, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(DelegationLog::default()),
    }
}

/// Every kit's delegable objects. Kits may not forge the person's evidence.
pub fn delegations(state: &Value, kits: &[&dyn Kit]) -> Result<Vec<KitDelegation>, String> {
    let mut all = Vec::new();
    for kit in kits {
        all.extend(kit.delegations(state)?);
    }
    if all.iter().flat_map(|item| &item.evidence).any(|evidence| evidence.source_kind == SourceKind::Used) {
        return Err("used 证据只能由人的动作产生".into());
    }
    Ok(all)
}

/// The evidence of one attempt: the kit's plus the person's `used` entries.
pub fn attempt_evidence(item: &KitDelegation, log: Option<&ObjectLog>, attempt: &str) -> Vec<Evidence> {
    let version = item.attempts.iter().find(|entry| entry.id == attempt).and_then(|entry| entry.version.clone());
    let mut evidence: Vec<_> = item.evidence.iter().filter(|entry| entry.attempt == attempt).cloned().collect();
    evidence.extend(log.into_iter().flat_map(|log| &log.used).filter(|entry| entry.attempt == attempt).map(|entry| {
        Evidence {
            id: entry.event.eid.clone(), source_kind: SourceKind::Used, criteria: entry.data.criteria.clone(),
            passed: None, version: version.clone(), actor: entry.event.actor.clone(), at: entry.event.at.clone(),
            note: entry.data.note.clone(), attempt: attempt.to_owned(),
        }
    }));
    evidence
}

/// FR-208: criteria no evidence points at. Whole-object evidence proves no row.
pub fn unproven(brief: &Brief, evidence: &[Evidence]) -> Vec<usize> {
    (0..brief.done_when.len()).filter(|index| !evidence.iter().any(|entry| entry.criteria.contains(index))).collect()
}

/// A report request made from a dated checkpoint when a delegation takes effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRequest {
    pub id: String,
    pub object: String,
    pub attempt: String,
    /// 1-based position on the card.
    pub checkpoint: usize,
    pub text: String,
    pub due: String,
    /// Still expecting a reply: not replied, attempt active, not claimed done.
    pub open: bool,
}

pub fn request_id(attempt: &str, checkpoint: usize) -> String {
    format!("R-{attempt}-{checkpoint}")
}

/// Hub IDs dedupe the same overdue written by several hosts.
pub fn overdue_eid(request: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"report.overdue\0");
    hash.update(request.as_bytes());
    format!("git:{}", hex::encode(hash.finalize()))
}

pub fn report_requests(items: &[KitDelegation], log: &DelegationLog) -> Vec<ReportRequest> {
    let mut requests = Vec::new();
    for item in items {
        let last = item.attempts.last().map(|attempt| attempt.id.as_str());
        for attempt in &item.attempts {
            for (index, checkpoint) in item.brief.checkpoints.iter().enumerate() {
                let Some(due) = &checkpoint.due else { continue };
                let id = request_id(&attempt.id, index + 1);
                let replied = log.reports.get(&id).is_some_and(|report| report.replied.is_some());
                let claimed = item.phase == Phase::ClaimedDone && last == Some(attempt.id.as_str());
                requests.push(ReportRequest {
                    open: !replied && attempt.active && !claimed,
                    id, object: item.id.clone(), attempt: attempt.id.clone(), checkpoint: index + 1,
                    text: checkpoint.text.clone(), due: due.clone(),
                });
            }
        }
    }
    requests
}

/// FR-137: a second overdue in a row for the same object is `active`.
fn overdue_level(requests: &[ReportRequest], log: &DelegationLog, request: &ReportRequest) -> Result<OverdueLevel, String> {
    let due = parse_time(&request.due)?;
    let mut earlier: Vec<(DateTime<FixedOffset>, &str)> = Vec::new();
    for other in requests.iter().filter(|other| other.object == request.object && other.id != request.id) {
        let at = parse_time(&other.due)?;
        if (at, other.id.as_str()) < (due, request.id.as_str()) {
            earlier.push((at, &other.id));
        }
    }
    let previous = earlier.into_iter().max().map(|(_, id)| id);
    Ok(match previous.and_then(|id| log.reports.get(id)).and_then(|report| report.overdue.as_ref()) {
        Some(_) => OverdueLevel::Active,
        None => OverdueLevel::Passive,
    })
}

fn parse<T: serde::de::DeserializeOwned>(event: &Event) -> Result<T, String> {
    serde_json::from_value(event.data.clone()).map_err(|e| format!("{} 正文无效：{e}", event.kind))
}

fn human(event: &Event, layer: &str) -> Result<(), String> {
    if !human_actor(&event.actor) || event.layer != layer {
        return Err(format!("{} 只能由 App 中的人发出", event.kind));
    }
    Ok(())
}

fn process(event: &Event, actor: &str, layer: &str) -> Result<(), String> {
    if event.actor != actor || event.layer != layer {
        return Err(format!("{} 的署名或层级无效", event.kind));
    }
    Ok(())
}

fn optional(text: &Option<String>, label: &str) -> Result<(), String> {
    text.as_deref().map_or(Ok(()), |text| nonempty(text, label))
}

fn token(text: &str, label: &str) -> Result<(), String> {
    nonempty(text, label)?;
    if text.trim() != text || text.chars().any(char::is_control) || text.len() > 256 {
        return Err(format!("{label}不能含首尾空白或控制字符，且不超过 256 字节"));
    }
    Ok(())
}

fn target<'a>(items: &'a [KitDelegation], id: &str) -> Result<&'a KitDelegation, String> {
    items.iter().find(|item| item.id == id).ok_or_else(|| format!("委托对象不存在：{id}"))
}

/// The attempt a verdict or `used` applies to: the latest one.
fn latest(item: &KitDelegation) -> Result<&KitAttempt, String> {
    item.attempts.last().ok_or_else(|| "对象尚未委托过".into())
}

/// Validates and records one delegation event; then lets kits react to the
/// person's card and verdicts. Returns `Ok(false)` for other events.
pub fn apply(state: &mut Value, event: &Event, kits: &[&dyn Kit]) -> Result<bool, String> {
    if !KINDS.contains(&event.kind.as_str()) {
        return Ok(false);
    }
    if !event.guards.is_empty() || event.obj.trim().is_empty() {
        return Err(format!("{} 信封无效", event.kind));
    }
    let items = delegations(state, kits)?;
    let mut log = read(state)?;
    let mark = Mark::of(event);
    let obj = event.obj.clone();
    match event.kind.as_str() {
        "delegation.clarified" => {
            process(event, "process:towermd-cli", "L4")?;
            let data: Clarified = parse(event)?;
            if target(&items, &obj)?.phase != Phase::Clarifying {
                return Err("只有待澄清的对象能接受澄清".into());
            }
            nonempty(&data.restated, "复述")?;
            if data.questions.len() > MAX_QUESTIONS {
                return Err(format!("澄清问题不能超过 {MAX_QUESTIONS} 个"));
            }
            for text in data.done_when.iter().chain(&data.questions) {
                nonempty(text, "完成标准或问题")?;
            }
            if let Some(source) = &data.self_reported {
                source.check()?;
            }
            log.objects.entry(obj).or_default().clarifications.push(Entry { event: mark, data });
        }
        "delegation.finalized" => {
            human(event, "L4")?;
            let data: Finalized = parse(event)?;
            data.brief.check(true)?;
            if !matches!(target(&items, &obj)?.phase, Phase::Clarifying | Phase::Proposed) {
                return Err("只有尚未委托的对象能定稿委托卡".into());
            }
            let asked = log.objects.get(&obj).and_then(|object| object.clarifications.last())
                .map_or(0, |entry| entry.data.questions.len());
            if data.answers.len() > asked || data.answers.iter().any(|answer| answer.len() > 64 * 1024) {
                return Err("回答须与受托者的问题一一对应".into());
            }
            log.objects.entry(obj).or_default().finalized.push(Entry { event: mark, data });
        }
        "acceptance.used" => {
            human(event, "L4")?;
            let data: Used = parse(event)?;
            let item = target(&items, &obj)?;
            if !matches!(item.phase, Phase::ClaimedDone | Phase::Done) {
                return Err("只有已声称完成或已完成的对象能记\"我用过\"".into());
            }
            let mut criteria = data.criteria.clone();
            criteria.sort_unstable();
            criteria.dedup();
            if criteria.is_empty() || criteria.len() != data.criteria.len()
                || criteria.iter().any(|index| *index >= item.brief.done_when.len())
            {
                return Err("\"我用过\"须指向一条或多条不重复的完成标准".into());
            }
            optional(&data.note, "说明")?;
            let attempt = latest(item)?.id.clone();
            log.objects.entry(obj).or_default().used.push(AttemptEntry { event: mark, attempt, data });
        }
        "acceptance.confirmed" | "acceptance.sent_back" => {
            human(event, "L4")?;
            let item = target(&items, &obj)?;
            if !(item.phase == Phase::Done || item.phase == Phase::ClaimedDone && item.human_can_complete) {
                return Err("对象尚未到可验收的阶段".into());
            }
            let attempt = latest(item)?.id.clone();
            let object = log.objects.get(&obj);
            if object.is_some_and(|object| object.verdicts.iter().any(|entry| entry.attempt == attempt)) {
                return Err("这次托付已经验收过".into());
            }
            let data = if event.kind == "acceptance.confirmed" {
                let data: Confirmed = parse(event)?;
                if data.unproven != unproven(&item.brief, &attempt_evidence(item, object, &attempt)) {
                    return Err("确认时的未证实条目与验收卡不符".into());
                }
                optional(&data.takeaway, "这次改变了什么看法")?;
                Verdict::Confirmed(data)
            } else {
                let data: SentBack = parse(event)?;
                nonempty(&data.reason, "退回理由")?;
                Verdict::SentBack(data)
            };
            log.objects.entry(obj).or_default().verdicts.push(AttemptEntry { event: mark, attempt, data });
        }
        "rule.recorded" => {
            human(event, "L3")?;
            let data: RuleRecorded = parse(event)?;
            valid_id(&obj)?;
            token(&data.kind, "工作类别")?;
            nonempty(&data.text, "规则")?;
            if let Some(source) = &data.source {
                let sent_back = log.objects.values().flat_map(|object| &object.verdicts)
                    .any(|entry| entry.event.eid == *source && matches!(entry.data, Verdict::SentBack(_)));
                if !sent_back {
                    return Err("规则来源须是一次退回".into());
                }
            }
            if log.rules.contains_key(&obj) {
                return Err(format!("规则已存在：{obj}"));
            }
            log.rules.insert(obj, Entry { event: mark, data });
        }
        "work_kind.renamed" => {
            human(event, "L3")?;
            let data: Renamed = parse(event)?;
            token(&data.from, "工作类别")?;
            token(&data.to, "工作类别")?;
            if obj != data.from || data.from == data.to || data.to == "*" {
                return Err("工作类别改名须从对象名改为另一个名字".into());
            }
            let mut rest = log.clone();
            rest.renames.remove(&data.from);
            if rest.work_kind(&data.to) == data.from {
                return Err("工作类别改名不能成环".into());
            }
            log.renames.insert(obj, Entry { event: mark, data });
        }
        "delegation.level.set" => {
            human(event, "L3")?;
            let data: LevelSet = parse(event)?;
            token(&data.config, "受托者配置")?;
            token(&data.kind, "工作类别")?;
            token(&data.level, "委托级别")?;
            if obj != data.kind {
                return Err("委托级别事件的对象须是工作类别".into());
            }
            log.levels.push(Entry { event: mark, data });
        }
        "report.replied" | "report.overdue" => {
            let requests = report_requests(&items, &log);
            let request = requests.iter().find(|request| request.id == obj && request.open)
                .ok_or("汇报请求不存在或已关闭")?;
            let report = log.reports.get(&obj).cloned().unwrap_or_default();
            if event.kind == "report.replied" {
                process(event, "process:towermd-cli", "L5")?;
                let data: Replied = parse(event)?;
                nonempty(&data.summary, "汇报摘要")?;
                if data.summary.chars().count() > SUMMARY_CHARS {
                    return Err(format!("汇报摘要不能超过 {SUMMARY_CHARS} 字"));
                }
                optional(&data.next, "下一步")?;
                if let Some(source) = &data.self_reported {
                    source.check()?;
                }
                log.reports.insert(obj, ReportLog { replied: Some(Entry { event: mark, data }), ..report });
            } else {
                process(event, "process:towermd", "L5")?;
                let data: Overdue = parse(event)?;
                if event.eid != overdue_eid(&obj) || report.overdue.is_some() {
                    return Err("汇报过期须由请求 ID 派生且只记一次".into());
                }
                if parse_time(&event.t)? < parse_time(&request.due)? {
                    return Err("汇报请求尚未到期".into());
                }
                if data.level != overdue_level(&requests, &log, request)? {
                    return Err("汇报过期级别与连续过期次数不符".into());
                }
                log.reports.insert(obj, ReportLog { overdue: Some(Entry { event: mark, data }), ..report });
            }
        }
        _ => unreachable!(),
    }
    let mut next = state.clone();
    next[GROUP] = serde_json::to_value(&log).map_err(|e| e.to_string())?;
    if matches!(event.kind.as_str(), "delegation.finalized" | "acceptance.confirmed" | "acceptance.sent_back") {
        let mut handled = false;
        for kit in kits {
            handled |= kit.reduce(&mut next, event)?;
        }
        if event.kind == "delegation.finalized" && !handled {
            return Err("没有套件接受委托卡定稿".into());
        }
    }
    *state = next;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One object a person completes, in the phase stored under `Fake`.
    struct Fake { forge_used: bool }
    impl Kit for Fake {
        fn delegations(&self, state: &Value) -> Result<Vec<KitDelegation>, String> {
            let phase = serde_json::from_value(state["Fake"]["phase"].clone()).map_err(|e| e.to_string())?;
            let stamp = Stamp { at: "2026-10-22T10:00:00Z".into(), eid: "c".into(), seq: 1 };
            let evidence = Evidence { id: "x".into(), source_kind: if self.forge_used { SourceKind::Used } else { SourceKind::Claimed },
                criteria: vec![0], passed: None, version: None, actor: "agent/1".into(), at: stamp.at.clone(), note: None, attempt: "A1".into() };
            Ok(vec![KitDelegation { id: "N-1".into(), title: "n".into(),
                brief: Brief { done_when: vec!["a".into(), "b".into()], ..Brief::default() }, work_kind: "notes".into(), phase,
                human_can_complete: true, attempts: vec![KitAttempt { id: "A1".into(), delegate: None, delegated: stamp,
                    delivered: None, done: None, active: true, version: None, out_of_bounds: None }],
                evidence: vec![evidence] }])
        }
    }

    fn event(kind: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":2,"eid":"e","host":"h","t":"2026-10-22T10:01:00Z","actor":"human:local",
            "type":kind,"layer":"L4","obj":"N-1","data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    #[test]
    fn people_confirm_claimed_objects_only_where_the_kit_allows_it() {
        let kit = Fake { forge_used: false };
        let mut state = json!({"Fake":{"phase":"claimed_done"}});
        let confirm = event("acceptance.confirmed", json!({"review_ms":3,"unproven":[1]}));
        assert!(apply(&mut state, &confirm, &[&kit]).unwrap());
        let verdicts = &read(&state).unwrap().objects["N-1"].verdicts;
        assert_eq!((verdicts[0].attempt.as_str(), &verdicts[0].data),
            ("A1", &Verdict::Confirmed(Confirmed { review_ms: 3, unproven: vec![1], takeaway: None })));
        let mut state = json!({"Fake":{"phase":"running"}});
        assert!(apply(&mut state, &confirm, &[&kit]).unwrap_err().contains("阶段"));
    }

    #[test]
    fn a_final_card_needs_a_kit_and_kits_cannot_forge_used_evidence() {
        let mut state = json!({"Fake":{"phase":"proposed"}});
        let card = event("delegation.finalized", json!({"brief":{"done_when":["a"]}}));
        assert!(apply(&mut state, &card, &[&Fake { forge_used: false }]).unwrap_err().contains("没有套件"));
        assert!(state.get(GROUP).is_none());
        assert!(apply(&mut state, &card, &[&Fake { forge_used: true }]).unwrap_err().contains("used"));
        assert!(!apply(&mut state, &event("task.created", json!({})), &[]).unwrap());
    }

    #[test]
    fn overdue_ids_are_valid_hub_ids_derived_from_the_request() {
        crate::hub::validate_eid(&overdue_eid("R-A1-1")).unwrap();
        assert_ne!(overdue_eid("R-A1-1"), overdue_eid("R-A1-2"));
    }
}
