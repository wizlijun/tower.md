//! Pure module aggregation of the shared verifier's report; this does not record Evidence.
use crate::coldstart::{no_tests_reason, validate_scan_result, ScanInput};
use crate::recovery_contract::{
    finish::EvidenceResult,
    verification::{commands_hash, report_hash, VerificationCommand, VerificationReport},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};

/// A derived module result. Indices refer to the unchanged, shared verification report.
/// The caller must retain that real report and its sealed logs; this function does not
/// read logs, authenticate their bytes, append events, or confer any approval.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModuleVerification {
    pub module: String,
    pub result: EvidenceResult,
    pub shared: bool,
    pub diagnostics: Vec<Value>,
    /// Original discovery metadata, including unsupported and pending commands.
    pub validation_plan: Value,
    pub execution_indices: Vec<usize>,
    pub report_hash: String,
}

/// Extract exactly the scanner's deduplicated runnable commands in scanner order.
pub fn verification_commands(
    input: &ScanInput,
    scan: &Value,
) -> Result<Vec<VerificationCommand>, String> {
    validate_scan_result(input, scan)?;
    let modules = array(scan, "modules")?;
    let plans = array(scan, "validation_plans")?;
    let mut module_ids = BTreeSet::new();
    for module in modules {
        let id = module["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("module lacks id")?;
        if !module_ids.insert(id) {
            return Err("duplicate scan module".into());
        }
    }
    let mut plan_ids = BTreeSet::new();
    let mut union = BTreeMap::new();
    for plan in plans {
        let id = plan["module"]
            .as_str()
            .ok_or("validation plan lacks module")?;
        if !module_ids.contains(id) || !plan_ids.insert(id) {
            return Err("validation plans do not uniquely match modules".into());
        }
        if plan["tree"] != input.tree || !plan["shared"].is_boolean() {
            return Err("validation plan tree/shared mismatch".into());
        }
        for command in array(plan, "commands")? {
            if let Some(command) = runnable_command(command)? {
                // This is the scanner's actual BTreeMap key, including JSON argv order.
                let key = (
                    command
                        .cwd
                        .to_str()
                        .ok_or("command cwd is not UTF-8")?
                        .to_owned(),
                    serde_json::to_string(&command.argv).map_err(|e| e.to_string())?,
                );
                union.insert(key, command);
            }
        }
    }
    if module_ids != plan_ids {
        return Err("validation plans do not cover modules".into());
    }
    let commands: Vec<VerificationCommand> =
        serde_json::from_value(scan["verification_commands"].clone())
            .map_err(|e| format!("invalid verification commands: {e}"))?;
    let expected = union.into_values().collect::<Vec<_>>();
    if commands != expected || commands_hash(&commands)? != commands_hash(&expected)? {
        return Err("verification commands differ from ordered runnable module union".into());
    }
    Ok(commands)
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>, String> {
    value[field]
        .as_array()
        .ok_or_else(|| format!("scan lacks {field} array"))
}

fn runnable_command(command: &Value) -> Result<Option<VerificationCommand>, String> {
    let status = command["status"]
        .as_str()
        .ok_or("discovered command lacks status")?;
    if !matches!(status, "not_run" | "unsupported")
        || command
            .get("requires_confirmation")
            .is_some_and(|value| !value.is_boolean())
    {
        return Err("invalid discovered command status/confirmation".into());
    }
    if status != "not_run" || command["requires_confirmation"] == true {
        return Ok(None);
    }
    if !command["argv"]
        .as_array()
        .is_some_and(|argv| !argv.is_empty())
    {
        return Ok(None);
    }
    serde_json::from_value(json!({"cwd":command["cwd"], "argv":command["argv"]}))
        .map(Some)
        .map_err(|e| format!("invalid runnable command: {e}"))
}

/// Aggregate one real verifier report. Incomplete/interrupted or contradictory reports
/// are errors for the producer to handle, never fabricated `no_tests` or passed results.
pub fn aggregate_verification(
    input: &ScanInput,
    scan: &Value,
    report: &VerificationReport,
) -> Result<Vec<ModuleVerification>, String> {
    let commands = verification_commands(input, scan)?;
    if report.commit != input.revision || report.tree != input.tree {
        return Err("verification report commit/tree differs from scan input".into());
    }
    if report.executions.len() != commands.len() {
        return Err("verification report execution count differs from commands".into());
    }
    let mut log_directory = None;
    for (index, (execution, command)) in report.executions.iter().zip(&commands).enumerate() {
        if execution.command.cwd.as_os_str() != command.cwd.as_os_str()
            || execution.command.argv != command.argv
        {
            return Err("verification execution order/command mismatch".into());
        }
        match execution.result.as_str() {
            "passed" if execution.exit_code == Some(0) && !execution.timed_out => (),
            "failed" => (),
            _ => return Err("inconsistent verification result/exit/timeout".into()),
        }
        crate::recovery_contract::sha256_hex(&execution.stdout_sha256)?;
        crate::recovery_contract::sha256_hex(&execution.stderr_sha256)?;
        let stdout = Path::new(&execution.stdout_log);
        let directory = stdout.parent().ok_or("verification log lacks directory")?;
        let cache = input.common_dir.join("towermd/cache/verification");
        let relative = directory
            .strip_prefix(&cache)
            .map_err(|_| "verification log is outside cache")?;
        if relative.components().count() != 1
            || !matches!(relative.components().next(), Some(Component::Normal(_)))
            || directory.join(format!("{index}-stdout.log")).to_str()
                != Some(execution.stdout_log.as_str())
            || directory.join(format!("{index}-stderr.log")).to_str()
                != Some(execution.stderr_log.as_str())
            || log_directory.is_some_and(|previous| previous != directory)
        {
            return Err(
                "verification logs do not reference one report directory in execution order".into(),
            );
        }
        log_directory = Some(directory);
    }
    let hash = report_hash(report)?;
    let mut results = Vec::new();
    for module in array(scan, "modules")? {
        let id = module["id"].as_str().ok_or("module lacks id")?;
        let plan = array(scan, "validation_plans")?
            .iter()
            .find(|plan| plan["module"] == id)
            .ok_or("module lacks validation plan")?;
        let mut indices = Vec::new();
        for command in array(plan, "commands")? {
            if let Some(command) = runnable_command(command)? {
                let index = commands
                    .iter()
                    .position(|candidate| candidate == &command)
                    .ok_or("module command absent from report")?;
                if !indices.contains(&index) {
                    indices.push(index);
                }
            }
        }
        indices.sort_unstable();
        let mut diagnostics = Vec::new();
        let result = if indices.is_empty() {
            diagnostics.push(json!({"code":"no_runnable_commands", "reason":no_tests_reason(array(plan, "commands")?)}));
            EvidenceResult::NoTests
        } else {
            for &index in &indices {
                if let Some(reason) = &report.executions[index].diagnostic {
                    diagnostics.push(json!({"execution_index":index, "reason":reason}));
                }
            }
            if indices
                .iter()
                .any(|&index| report.executions[index].result == "failed")
            {
                EvidenceResult::Failed
            } else {
                EvidenceResult::Passed
            }
        };
        results.push(ModuleVerification {
            module: id.into(),
            result,
            shared: plan["shared"].as_bool().ok_or("plan lacks shared flag")?,
            diagnostics,
            validation_plan: plan.clone(),
            execution_indices: indices,
            report_hash: hash.clone(),
        });
    }
    Ok(results)
}
