//! M3-D controller (总控, FR-23/24/30/65/66). A process, signed
//! `process:towermd-controller`: it answers fast-layer questions of
//! dispatched runs within the baseline and the acceptance, citing its basis,
//! and hands everything else to a person. It never approves, never answers a
//! slow-layer question, never touches reviews and never marks work done.
//!
//! - FR-65 `controller.policy`: questions default to `auto_notify`; a spent
//!   gate (`gate_failed`) and a runner error stop (`error`) default to
//!   `human` and may be set to `auto` or `auto_notify`, which lets the
//!   controller retry once per task and kind (`controller.retried`): the
//!   delegate tries again and the gates still decide. Reviews,
//!   authorizations, forbidden changes and budget stops are always a
//!   person's, whatever the policy says; an unreadable value means `human`.
//! - Even then the delegation level decides (`delegation_view::permit`,
//!   `AnswerByDecision`); the default level asks a person.
//! - Each question is triaged once (`controller.triaged`): answered, or handed
//!   over with the reason. A handed-over question stays open for a person.
//! - A person's answer replaces a controller answer (FR-66); what it replaced
//!   and the run finishes since are kept in `ControllerOverride`, derived on
//!   replay, and the App suggests rerunning the task.
use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use towermd_core::delegation::fingerprint;
use towermd_core::delegation_view::{permit, Capability, DelegationView, Permit};
use towermd_core::hub::Event;
use towermd_core::hub_protocol::NewEvent;
use towermd_core::question_model::{self, Question, QuestionEvent, BASIS_ITEMS, CONTROLLER};
use towermd_core::question_projection;

use crate::run_report::ErrorClass;
use crate::schedule::{self, Role};
use crate::task_model::{InterruptChoice, InterruptKind, InterruptResolved, TaskProjection};
use crate::{dev_state, run_record};
use towermd_core::project_state::insert_unique;

pub const TRIAGED: &str = "controller.triaged";
pub const TRIAGE: &str = "ControllerTriage";
pub const OVERRIDE: &str = "ControllerOverride";
/// FR-65: the controller let a held task's delegate try again.
pub const RETRIED: &str = "controller.retried";
pub const RETRY: &str = "ControllerRetry";
/// The alert an `auto_notify` retry leaves for a person to read.
pub const RETRY_NOTICE: &str = "controller_retried";
const DEFAULT_TIMEOUT_MINUTES: u64 = 5;
const REASON_CHARS: usize = 500;

/// FR-65 per-interrupt policy. `Human` is the only value most kinds take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Auto,
    AutoNotify,
    Human,
}

/// `controller:` in `config.yaml`, with the role from `runner.roles.controller` (FR-30).
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    /// Fast-layer questions; FR-65 default `auto_notify`.
    pub question: Mode,
    /// A hard gate whose automatic fixes ran out; default `human`.
    pub gate_failed: Mode,
    /// The runner's error stop (a crash or the third failure); default `human`.
    pub error: Mode,
    pub role: Role,
    pub timeout: Duration,
}
impl Default for Policy {
    fn default() -> Self {
        Self { question: Mode::AutoNotify, gate_failed: Mode::Human, error: Mode::Human, role: Role { harness: schedule::DEFAULT_HARNESS.into(), model: None },
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_MINUTES * 60) }
    }
}
impl Policy {
    /// Missing keys keep the default; a value nobody can read, or a policy
    /// that is not a mapping, means a person handles it. Only `question`,
    /// `gate_failed` and `error` are read: reviews, authorizations,
    /// forbidden changes and budget stops are a person's (FR-65).
    pub fn from_config(controller: Option<&serde_yaml::Value>, runner: Option<&serde_yaml::Value>) -> Self {
        let defaults = Self::default();
        let policy = controller.and_then(|c| c.get("policy"));
        let unreadable = policy.is_some_and(|policy| !policy.is_mapping());
        let mode = |key: &str, default: Mode| match policy.and_then(|p| p.get(key)) {
            _ if unreadable => Mode::Human,
            None => default,
            Some(mode) => mode.as_str().and_then(|mode| serde_json::from_value(json!(mode)).ok()).unwrap_or(Mode::Human),
        };
        let (question, gate_failed, error) =
            (mode("question", defaults.question), mode("gate_failed", defaults.gate_failed), mode("error", defaults.error));
        let role = runner.and_then(|r| r.get("roles")).and_then(|r| r.get("controller"));
        let text = |key: &str| role.and_then(|v| v.get(key)).and_then(serde_yaml::Value::as_str)
            .map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let timeout = controller.and_then(|c| c.get("timeout_minutes")).and_then(serde_yaml::Value::as_u64)
            .filter(|m| *m > 0).map_or(defaults.timeout, |m| Duration::from_secs(m.saturating_mul(60)));
        Self { question, gate_failed, error, role: Role { harness: text("harness").unwrap_or(defaults.role.harness), model: text("model") },
            timeout }
    }
    /// The mode for a sticky interrupt kind; `Human` for every kind but two.
    pub fn for_interrupt(&self, kind: InterruptKind) -> Mode {
        match kind {
            InterruptKind::GateFailed => self.gate_failed,
            InterruptKind::RunError => self.error,
            _ => Mode::Human,
        }
    }
}

/// `controller.retried`: the controller let a held task's delegate try
/// again (FR-65), once per task and kind. Nothing passes by it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retried {
    pub task: String,
    /// The interrupt retried, by its opening event.
    pub interrupt: String,
    pub kind: InterruptKind,
    /// `auto_notify`: an alert tells a person afterwards.
    pub notify: bool,
}

/// Retries so far, by task, in order.
pub fn retried(state: &Value) -> Result<BTreeMap<String, Vec<Retried>>, String> {
    let Some(group) = state.get(RETRY) else { return Ok(BTreeMap::new()) };
    let group = group.as_object().ok_or("总控重试记录无效")?;
    group.iter().map(|(task, list)| {
        let list = list.as_array().ok_or("总控重试记录无效")?.iter()
            .map(|entry| serde_json::from_value(entry["retried"].clone()).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
        Ok((task.clone(), list))
    }).collect()
}

/// Why the controller may not retry this task's interrupt of `kind`, if it
/// may not: the policy is not read here, only what any retry needs.
fn refusal(projection: &TaskProjection, done: &BTreeMap<String, Vec<Retried>>, records: &BTreeMap<String, run_record::RunRecord>,
    task_id: &str, kind: InterruptKind) -> Option<&'static str> {
    let Some(task) = projection.tasks.get(task_id) else { return Some("任务不存在") };
    let Some(interrupt) = &task.interrupt else { return Some("任务没有待处理的中断") };
    if interrupt.kind != kind || !matches!(kind, InterruptKind::GateFailed | InterruptKind::RunError) {
        return Some("总控只能重试门禁失败与运行错误");
    }
    let Some(run) = task.active_run.as_ref().and_then(|run| projection.runs.get(run)) else { return Some("中断任务没有运行") };
    if run.dispatch.is_none() { return Some("交互会话的中断由该会话的人处理") }
    if done.get(task_id).is_some_and(|list| list.iter().any(|retry| retry.kind == kind)) {
        return Some("总控已重试过这项任务的同类中断，这次交人");
    }
    // Spec 14.3: a suspected design problem is a person's.
    let design = records.get(&run.binding.id).and_then(|record| record.finishes.last())
        .is_some_and(|finish| finish.error_class == Some(ErrorClass::Design));
    if kind == InterruptKind::RunError && design { return Some("运行报告怀疑是设计类错误，交人") }
    None
}

/// FR-65: held tasks of dispatched runs the policy lets the controller retry.
pub fn retries(state: &Value, policy: &Policy) -> Result<Vec<Retried>, String> {
    let projection = dev_state::tasks(state)?;
    let (done, records) = (retried(state)?, run_record::records(state)?);
    Ok(projection.tasks.iter().filter_map(|(id, task)| {
        let interrupt = task.interrupt.as_ref()?;
        let mode = policy.for_interrupt(interrupt.kind);
        if mode == Mode::Human || refusal(&projection, &done, &records, id, interrupt.kind).is_some() { return None }
        Some(Retried { task: id.clone(), interrupt: interrupt.opened.clone(), kind: interrupt.kind, notify: mode == Mode::AutoNotify })
    }).collect())
}

pub fn retry_event(host: &str, retry: &Retried) -> Result<NewEvent, String> {
    let mut event = event(host, RETRIED, &retry.task, serde_json::to_value(retry).map_err(|e| e.to_string())?);
    event.layer = "L4".into();
    Ok(event)
}

fn retry(state: &mut Value, event: &Event) -> Result<bool, String> {
    let input: Retried = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if event.actor != CONTROLLER || event.layer != "L4" || !event.guards.is_empty() || event.obj != input.task {
        return Err("总控重试须由总控在 L4 记录".into());
    }
    let mut projection = dev_state::tasks(state)?;
    let (done, records) = (retried(state)?, run_record::records(state)?);
    if let Some(reason) = refusal(&projection, &done, &records, &input.task, input.kind) { return Err(reason.into()) }
    let task = projection.tasks.get(&input.task).ok_or("任务不存在")?;
    let interrupt = task.interrupt.clone().ok_or("任务没有待处理的中断")?;
    let run = task.active_run.clone();
    let key = if input.kind == InterruptKind::GateFailed { "gate_failed" } else { "error" };
    let note = format!("总控按 controller.policy.{key} 重试一次；同类中断再出现就交人");
    projection.resolve_interrupt(&input.task, &InterruptResolved { choice: InterruptChoice::Retry, interrupt: input.interrupt.clone(),
        note: Some(note.clone()) })?;
    dev_state::write_tasks(state, &projection)?;
    if input.kind == InterruptKind::RunError {
        run_record::forgive(state, &input.task)?;
    }
    let group = state.as_object_mut().ok_or("项目投影格式无效")?.entry(RETRY).or_insert_with(|| json!({}));
    let list = group.as_object_mut().ok_or("总控重试记录无效")?.entry(input.task.clone()).or_insert_with(|| json!([]));
    list.as_array_mut().ok_or("总控重试记录无效")?.push(json!({"event": event.eid, "at": event.t, "retried": input}));
    if input.notify {
        insert_unique(state, "Alert", &event.eid, json!({
            "id": event.eid, "event": event.eid, "at": event.t, "title": "总控已按策略重试一次", "resolved": false,
            "data": {"reason": RETRY_NOTICE, "detail": {
                "task": input.task, "run": run, "gate": interrupt.gate, "severity": "warning", "error": interrupt.reason, "note": note,
            }},
        }))?;
    }
    Ok(true)
}

/// One open question the controller may look at.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub question: Question,
    pub task: String,
    pub notify: bool,
    pub permit: Permit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Ask the controller harness.
    Consult(Candidate),
    /// A person decides; record why, once.
    HandOver { hold: String, task: String, reason: String, permit: Option<Permit> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Answered,
    HandedOver,
}

/// `controller.triaged`: what the controller did with one question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Triage {
    pub hold: String,
    pub task: String,
    pub outcome: Outcome,
    /// The `hold.answered` event, when answered.
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    /// The delegation level that allowed or refused it.
    #[serde(default)]
    pub level: Option<String>,
    /// `<harness>/<version>` when a harness was consulted.
    #[serde(default)]
    pub agent: Option<String>,
    /// Unknown stays null, never 0.
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

pub fn triages(state: &Value) -> Result<BTreeMap<String, Triage>, String> {
    state.get(TRIAGE).map_or(Ok(BTreeMap::new()), |group| serde_json::from_value(group.clone()).map_err(|e| e.to_string()))
}

/// Open questions of scheduler-dispatched runs, not yet triaged. Questions
/// of interactive sessions are left to the person at that session.
pub fn plan(state: &Value, policy: &Policy, delegation: &DelegationView) -> Result<Vec<Step>, String> {
    let questions = question_projection::read(state)?;
    let projection = dev_state::tasks(state)?;
    let triaged = triages(state)?;
    let mut steps = Vec::new();
    for (hold, question) in &questions.holds {
        if questions.answers.contains_key(hold) || triaged.contains_key(hold) { continue }
        let Some(run) = projection.runs.get(&question.binding.run) else { continue };
        if run.dispatch.is_none() { continue }
        let task = run.task.clone();
        let human_set = question_model::slow(&question.targets);
        let hand = |reason: &str, permit: Option<Permit>| Step::HandOver { hold: hold.clone(), task: task.clone(), reason: reason.into(), permit };
        if human_set {
            steps.push(hand("涉及慢层（L0–L2），只能由人决定；总控只可起草提案", None));
            continue;
        }
        if policy.question == Mode::Human {
            steps.push(hand("项目策略要求问题由人回答（controller.policy.question: human）", None));
            continue;
        }
        let kind = delegation.objects.iter().find(|object| object.id == task).map_or(run.module.clone(), |object| object.work_kind.clone());
        let config = fingerprint(run.binding.self_reported.as_ref());
        let permit = permit(&delegation.levels, &delegation.settings, &config, &kind, Capability::AnswerByDecision, human_set);
        if !permit.allowed {
            let reason = permit.reason.clone();
            steps.push(hand(&reason, Some(permit)));
            continue;
        }
        steps.push(Step::Consult(Candidate { question: question.clone(), task, notify: policy.question == Mode::AutoNotify, permit }));
    }
    Ok(steps)
}

/// The reply the controller harness must give.
pub fn reply_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["in_scope", "answer", "basis", "reason"],
        "properties": {
            "in_scope": {"type": "boolean"},
            "answer": {"type": ["string", "null"]},
            "basis": {"type": "array", "items": {"type": "string"}, "maxItems": BASIS_ITEMS},
            "reason": {"type": ["string", "null"]},
        },
    })
}

/// The controller's brief: the question, the task card (scope, allowed paths,
/// acceptance) and the module's remembered decisions.
pub fn brief(state: &Value, candidate: &Candidate) -> Result<String, String> {
    let projection = dev_state::tasks(state)?;
    let task = projection.tasks.get(&candidate.task).ok_or("任务不存在")?;
    let questions = question_projection::read(state)?;
    let draft = &candidate.question.draft;
    let mut text = String::from("# 总控答疑\n\n你是 Tower.md 项目的总控。一个无头运行的委托在等下面这个问题的回答。\n\n\
        只在基线、任务卡和验收标准已经定下的范围内作答，并引用依据（任务卡条目、验收标准、已记的模块决策）。\n\
        超出范围、需要改基线或慢层对象、需要人授权，或者拿不准时，不要猜：in_scope 写 false，answer 写 null，reason 写原因。\n\
        不要“先按假设继续”；不批准任何东西，也不判定完成。\n\n## 问题\n\n");
    text += &format!("{}\n", draft.text);
    for (index, option) in draft.options.iter().enumerate() {
        let mark = if draft.recommended == Some(index) { "（委托推荐）" } else { "" };
        text += &format!("- 选项 {}：{option}{mark}\n", index + 1);
    }
    if !draft.context.is_empty() {
        text += &format!("\n相关位置：{}\n", draft.context.join("、"));
    }
    text += "\n## 任务卡\n\n";
    text += &schedule::card(task, None);
    let decisions: Vec<_> = questions.decisions.values().filter(|decision| decision.scope == candidate.question.binding.scope).collect();
    if !decisions.is_empty() {
        text += "\n## 本模块已记的决策\n\n";
        for decision in decisions {
            text += &format!("- {}：{} → {}\n", decision.id, decision.question, decision.decision);
        }
    }
    Ok(text)
}

/// An answer with its basis, or why a person must answer.
pub fn verdict(reply: Option<&Value>, error: Option<&str>) -> Result<(String, Vec<String>), String> {
    let Some(reply) = reply else {
        return Err(clip(error.unwrap_or("总控没有给出回答")));
    };
    let reason = reply["reason"].as_str().map(str::trim).filter(|r| !r.is_empty());
    if reply["in_scope"] != Value::Bool(true) {
        return Err(clip(&format!("总控判断超出基线与验收范围：{}", reason.unwrap_or("未说明原因"))));
    }
    let answer = reply["answer"].as_str().map(str::trim).filter(|a| !a.is_empty()).ok_or("总控没有给出回答文本")?;
    let basis: Vec<String> = reply["basis"].as_array().into_iter().flatten().filter_map(Value::as_str)
        .map(str::trim).filter(|b| !b.is_empty()).map(str::to_owned).collect();
    if basis.is_empty() || basis.len() > BASIS_ITEMS
        || basis.iter().any(|b| b.chars().count() > question_model::ASSUMPTION_CHARS || b.chars().any(char::is_control))
    {
        return Err(format!("总控的回答没有引用 1 到 {BASIS_ITEMS} 条一行的依据，交给人"));
    }
    Ok((answer.to_owned(), basis))
}

fn clip(text: &str) -> String {
    text.chars().take(REASON_CHARS).collect()
}

fn event(host: &str, kind: &str, obj: &str, data: Value) -> NewEvent {
    NewEvent { eid: uuid::Uuid::now_v7().to_string(), host: host.into(), actor: CONTROLLER.into(), kind: kind.into(),
        layer: "L5".into(), obj: obj.into(), data, guards: Vec::new() }
}

/// The controller's `hold.answered`.
pub fn answer_event(host: &str, candidate: &Candidate, text: String, basis: Vec<String>) -> Result<NewEvent, String> {
    let hold = candidate.question.draft.id.clone();
    let data = serde_json::to_value(QuestionEvent::ControllerAnswered { hold: hold.clone(), text, basis, notify: candidate.notify })
        .map_err(|e| e.to_string())?;
    Ok(event(host, "hold.answered", &hold, data))
}

pub fn triage_event(host: &str, triage: &Triage) -> Result<NewEvent, String> {
    Ok(event(host, TRIAGED, &triage.hold, serde_json::to_value(triage).map_err(|e| e.to_string())?))
}

pub(crate) fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind == RETRIED { return retry(state, event) }
    if event.kind != TRIAGED { return Ok(false) }
    let triage: Triage = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if event.actor != CONTROLLER || event.layer != "L5" || !event.guards.is_empty() || event.obj != triage.hold {
        return Err("总控分诊须由总控在 L5 记录".into());
    }
    if triage.reason.as_deref().is_some_and(|r| r.chars().count() > REASON_CHARS)
        || triage.cost_usd.is_some_and(|cost| !cost.is_finite() || cost < 0.0)
    {
        return Err("总控分诊的原因或费用无效".into());
    }
    let questions = question_projection::read(state)?;
    if !questions.holds.contains_key(&triage.hold) { return Err("总控分诊的问题不存在".into()) }
    match triage.outcome {
        Outcome::Answered => {
            let answered = questions.answers.get(&triage.hold)
                .is_some_and(|answer| answer.actor == CONTROLLER && triage.answer.as_deref() == Some(answer.event.as_str()));
            if !answered { return Err("总控分诊记为已答，但没有总控的回答原件".into()) }
        }
        Outcome::HandedOver => {
            if triage.answer.is_some() || triage.reason.as_deref().is_none_or(|r| r.trim().is_empty()) {
                return Err("转交给人须写明原因，且不带回答".into());
            }
        }
    }
    let group = state.as_object_mut().ok_or("项目投影格式无效")?.entry(TRIAGE).or_insert_with(|| json!({}));
    let value = serde_json::to_value(&triage).map_err(|e| e.to_string())?;
    match group.get(&triage.hold) {
        Some(old) if old != &value => return Err("该问题已由总控分诊".into()),
        Some(_) => {}
        None => { group[&triage.hold] = value; }
    }
    Ok(true)
}

/// FR-66: the person replaced a controller answer. The run's finishes after
/// the controller answered are what may rest on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Override {
    pub hold: String,
    pub run: String,
    pub task: String,
    /// The controller answer replaced, and when it was given.
    pub replaced: String,
    pub replaced_at: String,
    pub replaced_text: String,
    /// The person's answer.
    pub answer: String,
    /// `run.finished` stamps of this run after the controller answered.
    pub affected: Vec<String>,
}

pub fn overrides(state: &Value) -> Result<BTreeMap<String, Override>, String> {
    state.get(OVERRIDE).map_or(Ok(BTreeMap::new()), |group| serde_json::from_value(group.clone()).map_err(|e| e.to_string()))
}

/// Called after a question event is applied (`Kit::observe`).
pub(crate) fn observe(state: &mut Value, event: &Event) -> Result<(), String> {
    if event.kind != "hold.answered" || event.stale || !event.actor.starts_with("human:") { return Ok(()) }
    let questions = question_projection::read(state)?;
    let Some(answer) = questions.answers.get(&event.obj).filter(|answer| answer.event == event.eid) else { return Ok(()) };
    let Some(replaced) = &answer.overrides else { return Ok(()) };
    let task = dev_state::tasks(state)?.runs.get(&answer.run).map(|run| run.task.clone()).unwrap_or_default();
    let affected = run_record::records(state)?.get(&answer.run).map(|record| record.finishes.iter()
        .filter(|finish| finish.at.at.as_str() > replaced.answered_at.as_str()).map(|finish| finish.at.eid.clone()).collect())
        .unwrap_or_default();
    let entry = Override { hold: event.obj.clone(), run: answer.run.clone(), task, replaced: replaced.event.clone(),
        replaced_at: replaced.answered_at.clone(), replaced_text: replaced.text.clone(), answer: event.eid.clone(), affected };
    let group = state.as_object_mut().ok_or("项目投影格式无效")?.entry(OVERRIDE).or_insert_with(|| json!({}));
    group[&event.obj] = serde_json::to_value(entry).map_err(|e| e.to_string())?;
    Ok(())
}
