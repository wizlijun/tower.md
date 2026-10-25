//! FR-28 run report, 14.3 error classes and the `run.*` event payloads.
//! A report is the delegate's own account: it is recorded and shown, and it
//! never moves a task (FR-33). Only what the runner itself observed (exit,
//! crash, timeout, budget) may move a task, and only to failed or held.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use towermd_core::project_state::valid_oid;

/// Spec limit for `summary`, counted in characters.
pub const SUMMARY_CHARS: usize = 300;
/// "同一任务失败 3 次" (10.2, 12.1) and "执行类失败 3 次" (14.3).
pub const FAILURES_BEFORE_HOLD: u32 = 3;
/// The runner signs `run.started`, `run.finished` and its own retries (6.3).
pub const RUNNER: &str = "process:towermd-runner";
pub const STARTED: &str = "run.started";
pub const FINISHED: &str = "run.finished";
pub const RETRY: &str = "run.retry";
/// 6.3: a commit the runner saw appear on the run's branch while it ran.
pub const CHECKPOINT: &str = "run.checkpoint";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Done,
    Blocked,
    NeedsInput,
    NeedsDecision,
    Failed,
}

/// 14.3: external errors are retried by the runner, execution errors by the
/// controller (at most 3 times), design errors interrupt a person.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    External,
    Execution,
    Design,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportProposal {
    pub kind: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunReport {
    pub status: ReportStatus,
    pub summary: String,
    #[serde(default)]
    pub checkpoints: Vec<String>,
    #[serde(default)]
    pub changed_paths: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspected_class: Option<ErrorClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ReportProposal>,
}

impl RunReport {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let report: Self = serde_json::from_value(value.clone()).map_err(|e| format!("运行报告格式无效：{e}"))?;
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.summary.trim().is_empty() || self.summary.chars().count() > SUMMARY_CHARS {
            return Err(format!("运行报告摘要须非空且不超过 {SUMMARY_CHARS} 字"));
        }
        if self.checkpoints.iter().any(|sha| !valid_oid(sha)) {
            return Err("运行报告的检查点须是完整提交号".into());
        }
        if self.changed_paths.iter().any(|path| !safe_path(path)) {
            return Err("运行报告的改动路径须是仓库内相对路径".into());
        }
        if self.questions.iter().any(|question| question.trim().is_empty()) {
            return Err("运行报告的问题不能为空".into());
        }
        if self.proposal.as_ref().is_some_and(|p| p.kind.trim().is_empty() || p.reason.trim().is_empty()) {
            return Err("运行报告的提案须有类型和理由".into());
        }
        let lists = [&self.checkpoints, &self.changed_paths, &self.questions];
        if lists.iter().any(|list| list.len() > 1000 || list.iter().any(|item| item.len() > 4096)) {
            return Err("运行报告列表过长".into());
        }
        Ok(())
    }

    /// The report pulled out of a harness's last message when the harness
    /// cannot enforce a schema: the whole text, or its last fenced JSON block.
    pub fn extract(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        let candidate = |body: &str| serde_json::from_str::<Value>(body.trim()).ok().and_then(|v| Self::parse(&v).ok());
        if let Some(report) = candidate(trimmed) {
            return Some(report);
        }
        let mut blocks = trimmed.split("```").skip(1).step_by(2).collect::<Vec<_>>();
        blocks.reverse();
        blocks.into_iter().find_map(|block| candidate(block.strip_prefix("json").unwrap_or(block)))
    }
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// FR-28 schema in the strict form schema-enforcing harnesses accept: every
/// property listed, optional ones nullable, no extra keys.
pub fn schema() -> Value {
    let strings = json!({"type":"array","items":{"type":"string"}});
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["status","summary","checkpoints","changed_paths","questions","suspected_class","proposal"],
        "properties":{
            "status":{"type":"string","enum":["done","blocked","needs_input","needs_decision","failed"]},
            "summary":{"type":"string","maxLength":SUMMARY_CHARS},
            "checkpoints":strings,
            "changed_paths":strings,
            "questions":strings,
            "suspected_class":{"type":["string","null"],"enum":["external","execution","design",null]},
            "proposal":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,
                "required":["kind","reason"],
                "properties":{"kind":{"type":"string"},"reason":{"type":"string"}}}]},
        }
    })
}

/// First `SUMMARY_CHARS` characters, for FR-29 summaries taken from the
/// harness's last message.
pub fn clip(text: &str) -> String {
    text.trim().chars().take(SUMMARY_CHARS).collect()
}

/// Why the runner stopped a process before it ended by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
    Timeout,
    Budget,
    Interrupted,
}

/// What the runner observed, independent of what the report says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Exited by itself with a valid report; the report decides nothing.
    Reported,
    /// No report, a nonzero exit, a crash or a timeout (FR-29).
    Failed,
    /// Stopped at the budget limit (12.1); checkpoints are kept.
    Budget,
    /// Stopped on a person's request; pausing is the pause event's business.
    Interrupted,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
}
impl Tokens {
    pub fn add(&mut self, other: &Tokens) {
        let sum = |a: &mut Option<u64>, b: Option<u64>| if let Some(b) = b { *a = Some(a.unwrap_or(0).saturating_add(b)) };
        sum(&mut self.input, other.input);
        sum(&mut self.output, other.output);
        sum(&mut self.cache_read, other.cache_read);
        sum(&mut self.cache_write, other.cache_write);
    }
}

/// Where the raw output stays: on this machine, outside git (FR-08).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawOutput {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunStarted {
    pub run: String,
    pub task: String,
    /// `<harness>/<version>` of the delegate this run launched.
    pub agent: String,
    pub adapter: String,
    pub budget_usd: Option<f64>,
    pub task_card: String,
    pub raw: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunFinished {
    pub run: String,
    pub task: String,
    pub agent: String,
    pub outcome: Outcome,
    /// The report's status, or `failed` when there is none (FR-29).
    pub status: ReportStatus,
    pub stop: Option<Stop>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    /// Ended by a signal the runner did not send.
    pub crashed: bool,
    pub report: Option<RunReport>,
    pub summary: String,
    pub error_class: Option<ErrorClass>,
    pub error: Option<String>,
    /// `null` when the harness did not report it; never 0 by default.
    pub cost_usd: Option<f64>,
    pub tokens: Tokens,
    pub session: Option<String>,
    pub attempts: u32,
    pub raw: Option<RawOutput>,
}

impl RunFinished {
    /// 14.3: the runner's observation first, then the delegate's suspicion.
    pub fn classify(&mut self, external: bool) {
        self.error_class = match (self.outcome, &self.report) {
            (Outcome::Budget | Outcome::Interrupted, _) => None,
            (_, Some(report)) if report.suspected_class.is_some() => report.suspected_class,
            (Outcome::Failed, _) if external => Some(ErrorClass::External),
            (Outcome::Failed, _) => Some(ErrorClass::Execution),
            (Outcome::Reported, Some(report)) if report.status == ReportStatus::Failed => Some(ErrorClass::Execution),
            _ => None,
        };
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCheckpoint {
    pub run: String,
    pub task: String,
    /// The full commit ID.
    pub commit: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRetry {
    pub run: String,
    /// `harness` for the harness's own API retries, `runner` for a relaunch.
    pub source: String,
    pub attempt: u32,
    pub error: Option<String>,
}

/// 9.3 "同类错误反复" thresholds under `signals.repeated_error` in
/// `config.yaml`; missing or malformed values keep the defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeatThresholds {
    pub count: usize,
    pub window_hours: i64,
}
impl Default for RepeatThresholds {
    fn default() -> Self {
        Self { count: 3, window_hours: 24 }
    }
}
impl RepeatThresholds {
    pub fn from_config(signals: Option<&serde_yaml::Value>) -> Self {
        let defaults = Self::default();
        let Some(map) = signals.and_then(|s| s.get("repeated_error")) else { return defaults };
        let positive = |key: &str| map.get(key).and_then(serde_yaml::Value::as_u64).filter(|v| *v > 0);
        Self {
            count: positive("count").and_then(|v| usize::try_from(v).ok()).unwrap_or(defaults.count),
            window_hours: positive("window_hours").and_then(|v| i64::try_from(v).ok()).unwrap_or(defaults.window_hours),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RepeatedError {
    pub class: ErrorClass,
    pub count: usize,
    pub runs: Vec<String>,
}

/// 9.3 growth signal: error classes seen at least `count` times in the
/// window ending at `now`, from the finished runs' accepted times. Derived
/// only; turning it into a proposal is the growth signals' job.
pub fn repeated_errors(
    records: &BTreeMap<String, crate::run_record::RunRecord>,
    now: chrono::DateTime<chrono::Utc>,
    thresholds: RepeatThresholds,
) -> Vec<RepeatedError> {
    let since = now - chrono::Duration::hours(thresholds.window_hours);
    let mut seen: BTreeMap<ErrorClass, Vec<String>> = BTreeMap::new();
    for (run, record) in records {
        for finish in &record.finishes {
            let Some(class) = finish.error_class else { continue };
            let Ok(at) = chrono::DateTime::parse_from_rfc3339(&finish.at.at) else { continue };
            let at = at.with_timezone(&chrono::Utc);
            if at > since && at <= now {
                seen.entry(class).or_default().push(run.clone());
            }
        }
    }
    seen.into_iter()
        .filter(|(_, runs)| runs.len() >= thresholds.count)
        .map(|(class, runs)| RepeatedError { class, count: runs.len(), runs })
        .collect()
}

#[cfg(test)]
#[path = "run_report_tests.rs"]
mod tests;
