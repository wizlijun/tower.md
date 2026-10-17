//! Deterministic draft bytes. Only the daemon publishes these derived files.
use crate::{
    canvas::CanvasDocument,
    coldstart::{validate_scan_result, ScanInput},
    okf::Document,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct DraftBundle {
    pub files: BTreeMap<String, Vec<u8>>,
}

pub fn render_draft(input: &ScanInput, result: &Value) -> Result<DraftBundle, String> {
    validate_scan_result(input, result)?;
    let mut saved = result.clone();
    let modules = saved["modules"]
        .as_array_mut()
        .ok_or("scan modules must be an array")?;
    let plans = result["validation_plans"]
        .as_array()
        .ok_or("scan validation_plans must be an array")?;
    let mut by_module = BTreeMap::new();
    for plan in plans {
        let module = plan["module"]
            .as_str()
            .ok_or("validation plan requires module")?;
        if plan["tree"] != input.tree
            || !plan["commands"].is_array()
            || by_module.insert(module, plan).is_some()
        {
            return Err("invalid or duplicate module validation plan".into());
        }
    }
    let mut files = BTreeMap::new();
    let mut module_ids = BTreeSet::new();
    for module in modules {
        let id = module["id"]
            .as_str()
            .ok_or("module requires id")?
            .to_owned();
        if !id.starts_with("A-")
            || id.len() <= 2
            || !id.chars().all(|c| c.is_alphanumeric() || c == '-')
            || !module_ids.insert(id.clone())
        {
            return Err("unsafe or duplicate module id".into());
        }
        let plan = by_module
            .remove(id.as_str())
            .ok_or("missing module validation plan")?;
        let roots = module["roots"]
            .as_array()
            .ok_or("module roots must be an array")?;
        let roots: Vec<_> = roots
            .iter()
            .map(|root| root.as_str().ok_or("module root must be a string"))
            .collect::<Result<_, _>>()?;
        if roots.is_empty() || !module["paths"].is_array() {
            return Err("module needs roots and paths".into());
        }
        let name = roots.join(" + ");
        let mut document = Document::new("Module", "\n目录扫描生成的候选模块，尚未经人工审阅。\n")?;
        let verify = plan["commands"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|command| {
                command["status"] == "not_run" && command["requires_confirmation"] != true
            })
            .filter_map(command_text)
            .collect::<Vec<_>>();
        for (key, value) in [
            ("id", json!(id)),
            ("layer", json!("L1")),
            ("status", json!("draft")),
            ("title", json!(name)),
            ("name", json!(name)),
            ("paths", module["paths"].clone()),
            ("interfaces", json!([])),
            ("verify", json!(verify)),
            ("verification_plan", plan.clone()),
            ("commit", json!(input.revision)),
            ("tree", json!(input.tree)),
            ("generated", json!({"by":"process:towermd"})),
            (
                "sources",
                json!([{"resource":format!("git:{}",input.revision)}]),
            ),
            ("confidence", module["confidence"].clone()),
            ("reasons", module["reasons"].clone()),
        ] {
            document.set(
                key,
                serde_yaml::to_value(value).map_err(|error| error.to_string())?,
            )?;
        }
        let path = format!("arch/{id}.md");
        files.insert(path.clone(), document.to_text().into_bytes());
        module["candidate_path"] = json!(path);
    }
    if !by_module.is_empty() {
        return Err("validation plan references unknown module".into());
    }
    let mut canvas = result["candidate_canvas"].clone();
    let nodes = canvas["nodes"]
        .as_array_mut()
        .ok_or("candidate canvas needs nodes")?;
    let mut referenced = BTreeSet::new();
    for node in nodes {
        if node["type"] != "file" {
            continue;
        }
        let id = node["towermd"]["obj"]
            .as_str()
            .ok_or("file node requires module object id")?
            .to_owned();
        if !module_ids.contains(&id) || node["id"] != id || !referenced.insert(id.clone()) {
            return Err("file node must reference exactly one known module".into());
        }
        node["file"] = json!(format!("arch/{id}.md"));
    }
    if referenced != module_ids {
        return Err("every module requires a file node".into());
    }
    CanvasDocument::from_value(canvas.clone())?;
    saved["candidate_canvas"] = canvas.clone();
    for (path, value) in [
        (
            "input.json",
            serde_json::to_value(input).map_err(|error| error.to_string())?,
        ),
        ("result.json", saved),
        ("tower.canvas", canvas),
        ("unassigned.json", result["unassigned"].clone()),
        ("candidate-tasks.json", result["candidate_tasks"].clone()),
    ] {
        let mut bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        files.insert(path.into(), bytes);
    }
    Ok(DraftBundle { files })
}

// A displayable POSIX command in the Module's existing string-list contract.
// Execution continues to use the original cwd/argv plan, never this text.
fn command_text(command: &Value) -> Option<String> {
    let argv = command["argv"].as_array()?;
    if argv.is_empty() {
        return None;
    }
    let args: Vec<_> = argv
        .iter()
        .map(|arg| arg.as_str().map(shell_quote))
        .collect::<Option<_>>()?;
    let text = args.join(" ");
    let cwd = command["cwd"].as_str()?;
    Some(if cwd == "." {
        text
    } else {
        format!("cd {} && {text}", shell_quote(cwd))
    })
}
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
