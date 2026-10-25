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
/// G2 stopped a request before anyone was asked (12.1 禁止).
pub const FORBIDDEN: &str = "permission.forbidden";
const RULE_CHARS: usize = 200;
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
    /// G2 found a dangerous command or a sensitive file: denied outright.
    Forbidden,
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

/// `permission.forbidden`, signed by the daemon: what G2 stopped and why.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forbidden {
    pub run: String,
    pub task: String,
    pub tool: String,
    /// The tool input as text, cut to `INPUT_CHARS`.
    pub input: String,
    /// The rule it hit.
    pub rule: String,
}

/// G2 dangerous commands (spec 11 G2, 12.1 禁止). The spec gives no list;
/// this conservative default only blocks: escalation, wiping the machine or
/// the home directory, piping a download into a shell, and steps that are
/// Tower.md's or a person's (pushing, publishing, opening or merging pull
/// requests, changing global Git configuration). A tool touching a
/// sensitive file is stopped too. A project adds text in `gates.yaml`
/// (`dangerous_commands`), matched case-insensitively; it cannot remove any.
/// The whole input is read, every string in it, not only `command`.
pub fn dangerous(tool: &str, input: &Value, extra: &[String]) -> Option<String> {
    let mut texts = Vec::new();
    let mut paths = Vec::new();
    strings(input, None, &mut texts, &mut paths);
    for path in paths {
        if towermd_core::path_guard::sensitive_file(path.trim_start_matches('/'), &[]) {
            return Some(format!("敏感文件：{path}").chars().take(RULE_CHARS).collect());
        }
    }
    let all = format!("{tool}\n{}", texts.join("\n")).to_lowercase();
    if let Some(text) = extra.iter().find(|text| all.contains(&text.to_lowercase())) {
        return Some(format!("项目声明的危险命令：{text}").chars().take(RULE_CHARS).collect());
    }
    texts.iter().find_map(|text| command_rule(text)).map(str::to_owned)
}

fn strings<'a>(value: &'a Value, key: Option<&str>, texts: &mut Vec<&'a str>, paths: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => {
            if matches!(key, Some("file_path" | "path" | "notebook_path")) { paths.push(text) }
            texts.push(text);
        }
        Value::Array(items) => items.iter().for_each(|item| strings(item, key, texts, paths)),
        Value::Object(map) => map.iter().for_each(|(key, item)| strings(item, Some(key), texts, paths)),
        _ => {}
    }
}

/// The built-in rule a shell text hits, read segment by segment.
fn command_rule(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    if lower.contains(":(){") || lower.contains(":() {") {
        return Some("危险命令：fork 炸弹");
    }
    let piped: Vec<&str> = lower.split('|').collect();
    for pair in piped.windows(2) {
        let fetches = pair[0].split_whitespace().any(|word| matches!(word.rsplit('/').next(), Some("curl" | "wget")));
        let shell = pair[1].split_whitespace().next().and_then(|word| word.rsplit('/').next());
        if fetches && matches!(shell, Some("sh" | "bash" | "zsh" | "sudo" | "python" | "python3" | "node")) {
            return Some("危险命令：把下载内容直接交给 shell 执行");
        }
    }
    let rootish = |word: &&str| matches!(*word, "/" | "/*" | "~" | "~/" | "~/*" | "$home" | "$home/" | "${home}" | "${home}/" | ".." | "../" | "." | "./" | "*");
    let recursive = |words: &[&str]| words.iter().any(|word| *word == "--recursive" || (word.starts_with('-') && !word.starts_with("--") && word.contains('r')));
    for segment in lower.split(|c| matches!(c, ';' | '&' | '|' | '\n' | '(' | ')' | '`')) {
        let mut words: Vec<&str> = segment.split_whitespace().collect();
        // Wrappers and leading assignments do not change what runs.
        while words.first().is_some_and(|word| matches!(*word, "env" | "command" | "exec" | "nohup" | "time" | "xargs" | "nice")
            || (word.contains('=') && !word.starts_with('-'))) {
            words.remove(0);
        }
        let Some(head) = words.first().and_then(|word| word.rsplit('/').next()) else { continue };
        let rule = match head {
            "sudo" | "su" | "doas" => Some("危险命令：提权（sudo、su、doas）"),
            "shutdown" | "reboot" | "halt" | "poweroff" => Some("危险命令：关机或重启"),
            head if head.starts_with("mkfs") => Some("危险命令：格式化磁盘"),
            "dd" if words.iter().any(|word| word.starts_with("of=/dev/")) => Some("危险命令：写裸设备"),
            "rm" if recursive(&words) && words.iter().any(rootish) => Some("危险命令：递归删除根目录、家目录、当前或上级目录"),
            "chmod" | "chown" if recursive(&words) && words.iter().any(rootish) => Some("危险命令：递归改根目录或家目录权限"),
            "git" => {
                let mut rest = words.iter().skip(1);
                let mut sub = None;
                while let Some(word) = rest.next() {
                    if matches!(*word, "-c" | "--git-dir" | "--work-tree") { rest.next(); continue }
                    if !word.starts_with('-') { sub = Some(*word); break }
                }
                match sub {
                    Some("push") => Some("危险命令：git push（合入只经 Tower.md 集成器）"),
                    Some("config") if words.iter().any(|word| matches!(*word, "--global" | "--system")) => Some("危险命令：改全局 Git 配置"),
                    _ => None,
                }
            }
            "gh" if matches!((words.get(1).copied(), words.get(2).copied()), (Some("pr"), Some("create" | "merge")) | (Some("repo"), Some("delete")))
                => Some("危险命令：开或合拉取请求、删仓库（分支策略归 Tower.md）"),
            "npm" | "pnpm" | "yarn" | "cargo" | "gem" if words.get(1) == Some(&"publish") => Some("危险命令：发布包"),
            "twine" if words.get(1) == Some(&"upload") => Some("危险命令：发布包"),
            _ => None,
        };
        if rule.is_some() { return rule }
    }
    None
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
        FORBIDDEN => forbidden(state, event)?,
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

/// The daemon stopped a request: denied at once, and the task held for a
/// person (12.1 禁止, the G2 interrupt) when its run is the working one.
fn forbidden(state: &mut Value, event: &Event) -> Result<(), String> {
    envelope(event)?;
    if event.actor != TOWER {
        return Err("G2 拦截只能由守护进程记录".into());
    }
    let data: Forbidden = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let records = run_record::records(state)?;
    let record = records.get(&data.run).filter(|record| record.running.is_some() && record.task == data.task)
        .ok_or("G2 拦截须针对运行器启动且仍在运行的运行")?;
    if data.tool.trim().is_empty() || data.tool.chars().count() > TOOL_CHARS || data.tool.chars().any(char::is_control)
        || data.input.chars().count() > INPUT_CHARS || data.rule.trim().is_empty() || data.rule.chars().count() > RULE_CHARS
    {
        return Err("G2 拦截的工具、输入或规则无效".into());
    }
    let mut all = read(state)?;
    if all.contains_key(&event.obj) {
        return Err("授权请求已存在".into());
    }
    let at = Stamp { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq };
    all.insert(event.obj.clone(), Permission { run: data.run.clone(), task: data.task.clone(), agent: record.agent.clone(),
        tool: data.tool.clone(), input: data.input.clone(), requested: at.clone(), deadline: event.t.clone(),
        outcome: Some(Outcome { decision: Decision::Deny, closed: Closed::Forbidden, by: TOWER.into(), at, note: Some(data.rule.clone()) }) });
    write(state, &all)?;
    let mut projection = crate::dev_state::tasks_scoped(state, &[&data.task], &[&data.run])?;
    let reason = format!("{}：{}（{}）", crate::task_finish::G2_TOOL_BLOCKED, data.rule, data.tool);
    if projection.hold(&data.task, &data.run, crate::task_model::InterruptKind::Forbidden, Some(crate::task_finish::G2), &reason, &event.eid) {
        crate::dev_state::write_scoped(state, &projection)?;
    }
    Ok(())
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
