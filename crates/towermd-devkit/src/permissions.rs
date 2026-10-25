//! Permission requests through the interaction gateway (spec 12.1 授权,
//! 16.4 `towermd_permission`, FR-63). A running delegate asks to use a tool;
//! only a person allows it, from the App. Nobody answering in ten minutes is a
//! denial, and so is the run ending first: nothing here ever allows by waiting.
//!
//! The group is created lazily, so journals without permission requests
//! replay to the v0.1.1 projection unchanged.
use std::collections::BTreeMap;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::dev_state::Stamp;
use crate::run_record;
use crate::run_report::{RunFinished, FINISHED};
use towermd_core::hub::Event;
use towermd_core::project_state::insert_unique;

pub const GROUP: &str = "Permission";
pub const REQUESTED: &str = "permission.requested";
pub const RESOLVED: &str = "permission.resolved";
pub const EXPIRED: &str = "permission.expired";
pub const LAYER: &str = "L5";
/// Alert reason the App names a permission request by.
pub const ALERT: &str = "permission_requested";
/// Permission IDs are `K-<request ID>`.
pub const PREFIX: &str = "K-";
/// Spec 12.1: unanswered for ten minutes, the request is denied.
pub const WAIT_MS: u64 = 600_000;
/// What a person sees of the tool input; the gateway keeps the original.
pub const INPUT_CHARS: usize = 2_000;
const TOOL_CHARS: usize = 200;
const NOTE_CHARS: usize = 500;
const HUMAN: &str = "human:local";
const TOWER: &str = "process:towermd";

/// `permission.requested`, signed by the run's delegate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requested {
    pub run: String,
    pub task: String,
    pub tool: String,
    /// The tool input as text, cut to `INPUT_CHARS`.
    pub input: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

/// `permission.resolved`, a person's answer. v0.1 offers "allow once" and
/// "deny"; writing an allow rule (an L3 registration) is not offered yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolved {
    pub decision: Decision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Resolved {
    pub fn check(&self) -> Result<(), String> {
        match &self.note {
            Some(note) if note.trim().is_empty() || note.chars().count() > NOTE_CHARS => {
                Err(format!("授权备注须非空且不超过 {NOTE_CHARS} 字"))
            }
            _ => Ok(()),
        }
    }
}

/// Why a request closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Closed {
    Answered,
    Expired,
    RunEnded,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub decision: Decision,
    pub closed: Closed,
    pub by: String,
    pub at: Stamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Permission {
    pub run: String,
    pub task: String,
    pub agent: String,
    pub tool: String,
    pub input: String,
    pub requested: Stamp,
    /// RFC 3339; at or after it the request can only expire.
    pub deadline: String,
    pub outcome: Option<Outcome>,
}

pub fn read(state: &Value) -> Result<BTreeMap<String, Permission>, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}

/// The tool input as a person sees it.
pub fn summary(input: &Value) -> String {
    let text = match input {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    if text.chars().count() <= INPUT_CHARS {
        return text;
    }
    let mut cut: String = text.chars().take(INPUT_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// The one expiry event of a request: a second waiter finds the receipt.
pub fn expired_eid(id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(EXPIRED.as_bytes());
    hash.update(b"\0");
    hash.update(id.as_bytes());
    format!("git:{}", hex::encode(hash.finalize()))
}

fn time(text: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(text).map(|t| t.with_timezone(&Utc)).map_err(|_| "事件时间无效".to_owned())
}

/// Whether `now` (RFC 3339) is at or past the request's deadline.
pub fn due(permission: &Permission, now: &str) -> Result<bool, String> {
    Ok(time(now)? >= time(&permission.deadline)?)
}

fn envelope(event: &Event) -> Result<(), String> {
    if event.layer != LAYER || !event.guards.is_empty() || !event.obj.starts_with(PREFIX) || event.obj.len() <= PREFIX.len() {
        return Err("授权事件信封无效".into());
    }
    Ok(())
}

fn write(state: &mut Value, all: &BTreeMap<String, Permission>) -> Result<(), String> {
    state[GROUP] = serde_json::to_value(all).map_err(|e| e.to_string())?;
    Ok(())
}

fn settle_alert(state: &mut Value, id: &str) {
    if let Some(alert) = state.get_mut("Alert").and_then(|alerts| alerts.get_mut(id)) {
        alert["resolved"] = json!(true);
    }
}

pub(crate) fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    match event.kind.as_str() {
        REQUESTED => requested(state, event)?,
        RESOLVED | EXPIRED => closed(state, event)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn requested(state: &mut Value, event: &Event) -> Result<(), String> {
    envelope(event)?;
    let data: Requested = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let records = run_record::records(state)?;
    let record = records.get(&data.run).filter(|record| record.running.is_some())
        .ok_or("授权请求须来自运行器启动且仍在运行的运行")?;
    if event.actor != record.agent || data.task != record.task {
        return Err("授权请求须由该运行的受托者署名".into());
    }
    if data.tool.trim().is_empty() || data.tool.chars().count() > TOOL_CHARS || data.tool.chars().any(char::is_control)
        || data.input.chars().count() > INPUT_CHARS
    {
        return Err(format!("授权请求的工具名须非空且不超过 {TOOL_CHARS} 字，输入摘要不超过 {INPUT_CHARS} 字"));
    }
    let mut all = read(state)?;
    if all.contains_key(&event.obj) {
        return Err("授权请求已存在".into());
    }
    let deadline = (time(&event.t)? + Duration::milliseconds(WAIT_MS as i64)).to_rfc3339_opts(SecondsFormat::Millis, true);
    let permission = Permission { run: data.run.clone(), task: data.task.clone(), agent: record.agent.clone(),
        tool: data.tool.clone(), input: data.input.clone(),
        requested: Stamp { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq }, deadline: deadline.clone(), outcome: None };
    all.insert(event.obj.clone(), permission);
    write(state, &all)?;
    let title: String = format!("授权请求：{}", data.tool).chars().take(80).collect();
    insert_unique(state, "Alert", &event.obj, json!({
        "id": event.obj, "event": event.eid, "title": title, "resolved": false, "at": event.t,
        "data": {"reason": ALERT, "detail": {
            "task": data.task, "run": data.run, "agent": record.agent, "severity": "blocking",
            "tool": data.tool, "deadline": deadline, "error": data.input,
        }},
    }))
}

fn closed(state: &mut Value, event: &Event) -> Result<(), String> {
    envelope(event)?;
    let mut all = read(state)?;
    let permission = all.get_mut(&event.obj).ok_or("授权请求不存在")?;
    if permission.outcome.is_some() {
        return Err("授权请求已关闭".into());
    }
    let late = due(permission, &event.t)?;
    let at = Stamp { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq };
    permission.outcome = Some(if event.kind == RESOLVED {
        if event.actor != HUMAN {
            return Err("授权只能由人在 App 中回答".into());
        }
        if late {
            return Err("授权请求已超时，按拒绝处理".into());
        }
        let data: Resolved = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
        data.check()?;
        Outcome { decision: data.decision, closed: Closed::Answered, by: event.actor.clone(), at, note: data.note }
    } else {
        if event.actor != TOWER || event.eid != expired_eid(&event.obj) || event.data != json!({}) {
            return Err("授权超时须由守护进程按请求派生记录".into());
        }
        if !late {
            return Err("授权请求尚未到期".into());
        }
        Outcome { decision: Decision::Deny, closed: Closed::Expired, by: event.actor.clone(), at, note: None }
    });
    write(state, &all)?;
    settle_alert(state, &event.obj);
    Ok(())
}

/// A run's harness process ended: its open requests are denied with it.
pub(crate) fn run_ended(state: &mut Value, event: &Event) -> Result<(), String> {
    if event.kind != FINISHED || state.get(GROUP).is_none() {
        return Ok(());
    }
    let data: RunFinished = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let mut all = read(state)?;
    let mut ended = Vec::new();
    for (id, permission) in all.iter_mut().filter(|(_, permission)| permission.run == data.run && permission.outcome.is_none()) {
        permission.outcome = Some(Outcome { decision: Decision::Deny, closed: Closed::RunEnded, by: event.actor.clone(),
            at: Stamp { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq }, note: None });
        ended.push(id.clone());
    }
    if ended.is_empty() {
        return Ok(());
    }
    write(state, &all)?;
    for id in ended {
        settle_alert(state, &id);
    }
    Ok(())
}
