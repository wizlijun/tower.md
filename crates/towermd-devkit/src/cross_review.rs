//! M3-H cross review (FR-34). A submitted proposal changes the baseline, a
//! slow layer, so it may get one review by a different harness than the one
//! that executes tasks. The result is attached to the proposal for reference
//! only: nothing in approval, publication or gates reads it, a person still
//! decides, and a review that failed or timed out is `unfinished`, never a pass.
//!
//! Off unless `runner.roles.cross_reviewer` names a harness, and refused when
//! it names the executor's harness (the review would not be heterogeneous).
use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use towermd_core::hub::Event;
use towermd_core::hub_protocol::NewEvent;

use crate::proposal_model::{self, Proposal};
use crate::schedule::{self, Role};

pub const REVIEWER: &str = "process:towermd-reviewer";
pub const CROSS_REVIEWED: &str = "proposal.cross_reviewed";
pub const GROUP: &str = "CrossReview";
const DEFAULT_TIMEOUT_MINUTES: u64 = 10;
const TEXT_CHARS: usize = 2000;
const FINDINGS: usize = 20;
/// Each side of a changed file shown to the reviewer.
const CHANGE_CHARS: usize = 6000;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub role: Role,
    pub timeout: Duration,
}

/// `None` when cross review is off; `Err` when the configuration would not be heterogeneous.
pub fn from_config(runner: Option<&serde_yaml::Value>) -> Result<Option<Settings>, String> {
    let role = runner.and_then(|r| r.get("roles")).and_then(|r| r.get("cross_reviewer"));
    let text = |key: &str| role.and_then(|v| v.get(key)).and_then(serde_yaml::Value::as_str)
        .map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let Some(harness) = text("harness") else { return Ok(None) };
    if crate::harness::find(&harness).is_none() {
        return Err(format!("交叉评审的 harness 未知：{harness}"));
    }
    let executor = schedule::Settings::from_config(runner).executor;
    if executor.harness == harness {
        return Err(format!("交叉评审须用与执行者（{}）不同的 harness", executor.harness));
    }
    let timeout = role.and_then(|v| v.get("timeout_minutes")).and_then(serde_yaml::Value::as_u64).filter(|m| *m > 0)
        .map_or(Duration::from_secs(DEFAULT_TIMEOUT_MINUTES * 60), |m| Duration::from_secs(m.saturating_mul(60)));
    Ok(Some(Settings { role: Role { harness, model: text("model") }, timeout }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Agree,
    Concerns,
    Disagree,
    /// Failed, timed out or unreadable: no opinion, never a pass.
    Unfinished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub severity: Severity,
    pub text: String,
}

/// `proposal.cross_reviewed`, bound to the proposal's content hash.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossReview {
    pub proposal: String,
    pub proposal_hash: String,
    /// `<harness>/<version>` of the reviewer, when it ran.
    pub agent: Option<String>,
    pub harness: String,
    pub model: Option<String>,
    pub verdict: Verdict,
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub error: Option<String>,
    /// Unknown stays null, never 0.
    pub cost_usd: Option<f64>,
}

pub fn reviews(state: &Value) -> Result<BTreeMap<String, CrossReview>, String> {
    state.get(GROUP).map_or(Ok(BTreeMap::new()), |group| serde_json::from_value(group.clone()).map_err(|e| e.to_string()))
}

/// Submitted proposals with a full draft and no cross review yet.
pub fn plan(state: &Value) -> Result<Vec<Proposal>, String> {
    let done = reviews(state)?;
    let entries = state.get("Proposal").and_then(Value::as_object).into_iter().flatten();
    Ok(entries.filter(|(id, entry)| entry["status"] == "submitted" && !done.contains_key(*id))
        .filter_map(|(id, _)| proposal_model::from_state(state, id).ok()).collect())
}

pub fn reply_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["verdict", "summary", "findings"],
        "properties": {
            "verdict": {"enum": ["agree", "concerns", "disagree"]},
            "summary": {"type": "string"},
            "findings": {"type": "array", "maxItems": FINDINGS, "items": {
                "type": "object", "additionalProperties": false, "required": ["severity", "text"],
                "properties": {"severity": {"enum": ["high", "medium", "low"]}, "text": {"type": "string"}},
            }},
        },
    })
}

fn clip(text: &str, limit: usize) -> String {
    let mut clipped: String = text.chars().take(limit).collect();
    if clipped.len() < text.len() { clipped += "…（已截断）" }
    clipped
}

pub fn brief(proposal: &Proposal) -> String {
    let mut text = String::from("# 交叉评审\n\n你是另一家模型的评审者。下面是一个改基线（慢层）的提案。请独立判断它是否成立：\
        理由是否充分、影响范围是否完整、有没有更好的备选、会不会削弱验收或检查。\n\
        你的意见只供人参考，不批准也不驳回；说不准就写 concerns。\n\n");
    text += &format!("- 提案：{}\n- 类型：{:?}\n- 基线：{}\n\n## 理由\n\n{}\n\n## 推荐\n\n{}\n",
        proposal.id, proposal.kind, proposal.baseline_tag, proposal.reason, proposal.recommendation);
    if !proposal.alternatives.is_empty() {
        text += "\n## 备选\n\n";
        for alternative in &proposal.alternatives { text += &format!("- {alternative}\n") }
    }
    let impact = &proposal.impact;
    text += &format!("\n## 影响\n\n- 意图：{:?}\n- 模块：{:?}\n- 接口：{:?}\n- 任务：{:?}\n- 路径：{:?}\n",
        impact.intents, impact.modules, impact.interfaces, impact.tasks, impact.paths);
    for change in &proposal.changes {
        text += &format!("\n## 改动 {}\n\n改前：\n```\n{}\n```\n改后：\n```\n{}\n```\n", change.path,
            clip(change.before.as_deref().unwrap_or("（无）"), CHANGE_CHARS), clip(change.after.as_deref().unwrap_or("（删除）"), CHANGE_CHARS));
    }
    text
}

/// The record of one review from what the reviewer returned.
pub fn record(proposal: &Proposal, settings: &Settings, agent: Option<String>, reply: Option<&Value>, error: Option<String>,
    cost_usd: Option<f64>) -> CrossReview {
    let mut review = CrossReview { proposal: proposal.id.clone(), proposal_hash: proposal.content_hash.clone(), agent,
        harness: settings.role.harness.clone(), model: settings.role.model.clone(), verdict: Verdict::Unfinished,
        summary: String::new(), findings: Vec::new(), error: None, cost_usd };
    let parsed = reply.and_then(|reply| {
        let verdict = match reply["verdict"].as_str()? { "agree" => Verdict::Agree, "concerns" => Verdict::Concerns,
            "disagree" => Verdict::Disagree, _ => return None };
        let summary = reply["summary"].as_str().map(str::trim).filter(|s| !s.is_empty())?;
        let findings = reply["findings"].as_array()?.iter().take(FINDINGS).filter_map(|finding| {
            let severity = serde_json::from_value(finding["severity"].clone()).ok()?;
            let text = finding["text"].as_str().map(str::trim).filter(|t| !t.is_empty())?;
            Some(Finding { severity, text: clip(text, TEXT_CHARS) })
        }).collect();
        Some((verdict, clip(summary, TEXT_CHARS), findings))
    });
    match parsed {
        Some((verdict, summary, findings)) => { review.verdict = verdict; review.summary = summary; review.findings = findings; }
        None => {
            review.summary = "交叉评审没有完成，不代表通过".into();
            review.error = Some(clip(&error.unwrap_or_else(|| "评审者没有给出可读的意见".into()), TEXT_CHARS));
        }
    }
    review
}

pub fn event(host: &str, review: &CrossReview) -> Result<NewEvent, String> {
    Ok(NewEvent { eid: uuid::Uuid::now_v7().to_string(), host: host.into(), actor: REVIEWER.into(), kind: CROSS_REVIEWED.into(),
        layer: "L5".into(), obj: review.proposal.clone(), data: serde_json::to_value(review).map_err(|e| e.to_string())?,
        guards: Vec::new() })
}

pub(crate) fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != CROSS_REVIEWED { return Ok(false) }
    let review: CrossReview = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if event.actor != REVIEWER || event.layer != "L5" || !event.guards.is_empty() || event.obj != review.proposal {
        return Err("交叉评审须由评审进程在 L5 记录".into());
    }
    let proposal = proposal_model::from_state(state, &review.proposal)?;
    if proposal.content_hash != review.proposal_hash { return Err("交叉评审不是针对该提案的当前内容".into()) }
    if review.cost_usd.is_some_and(|cost| !cost.is_finite() || cost < 0.0) || review.findings.len() > FINDINGS
        || (review.verdict == Verdict::Unfinished) != review.error.is_some()
    {
        return Err("交叉评审记录无效".into());
    }
    let group = state.as_object_mut().ok_or("项目投影格式无效")?.entry(GROUP).or_insert_with(|| json!({}));
    let value = serde_json::to_value(&review).map_err(|e| e.to_string())?;
    match group.get(&review.proposal) {
        Some(old) if old != &value => return Err("该提案已有交叉评审".into()),
        Some(_) => {}
        None => { group[&review.proposal] = value; }
    }
    Ok(true)
}
