//! S6 synthesis is an unapproved, immutable draft derived from an actual S5 scan.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cold_start::{self, ScanReport};
use towermd_core::okf::Document;
use towermd_core::project_view::{CanvasEdge, CanvasNode, ProjectView};
use crate::verifier::VerificationStatus;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub scan_id: String,
    pub project_id: String,
    pub source_revision: String,
    pub intents: Vec<DraftObject>,
    pub modules: Vec<DraftObject>,
    pub interfaces: Vec<DraftObject>,
    pub acceptances: Vec<DraftObject>,
    pub questions: Vec<String>,
    pub risks: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftObject {
    pub id: String,
    pub path: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BaselineDraft {
    pub id: String,
    pub producer: String,
    pub self_reported: bool,
    pub created_at: String,
    /// Exact submitted OKF text is preserved separately from the derived view.
    pub submission: Submission,
    pub scan: ScanReport,
    pub view: ProjectView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<crate::review::ReviewState>,
}

pub fn schema() -> Value {
    serde_json::from_str(include_str!("../schema/baseline-submit.schema.json"))
        .expect("内置提交 schema 必须是合法 JSON")
}

// Interpret only the vocabulary actually used by the shared schema. Unknown
// keywords fail closed so editing the schema cannot silently relax validation.
fn validate(value: &Value, rule: &Value, root: &Value, path: &str) -> Result<(), String> {
    let object = rule.as_object().ok_or("schema 必须是对象")?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "$schema"
                | "$id"
                | "$ref"
                | "$defs"
                | "type"
                | "properties"
                | "required"
                | "additionalProperties"
                | "items"
                | "minItems"
                | "minLength"
        ) {
            return Err(format!("不支持的 schema 关键字：{key}"));
        }
    }
    if let Some(reference) = rule.get("$ref") {
        let pointer = reference
            .as_str()
            .and_then(|s| s.strip_prefix('#'))
            .ok_or("schema 引用无效")?;
        return validate(
            value,
            root.pointer(pointer).ok_or("schema 引用不存在")?,
            root,
            path,
        );
    }
    match rule.get("type").and_then(Value::as_str) {
        Some("object") => {
            let entries = value
                .as_object()
                .ok_or_else(|| format!("{path} 必须是对象"))?;
            if let Some(required) = rule.get("required").and_then(Value::as_array) {
                for key in required {
                    let key = key.as_str().ok_or("schema required 无效")?;
                    if !entries.contains_key(key) {
                        return Err(format!("{path}.{key} 必填"));
                    }
                }
            }
            for (key, entry) in entries {
                if let Some(child) = rule.get("properties").and_then(|p| p.get(key)) {
                    validate(entry, child, root, &format!("{path}.{key}"))?;
                } else if rule.get("additionalProperties") == Some(&Value::Bool(false)) {
                    return Err(format!("{path}.{key} 是未知字段"));
                }
            }
        }
        Some("array") => {
            let entries = value
                .as_array()
                .ok_or_else(|| format!("{path} 必须是数组"))?;
            if entries.len() < rule.get("minItems").and_then(Value::as_u64).unwrap_or(0) as usize {
                return Err(format!("{path} 数量不足"));
            }
            if let Some(items) = rule.get("items") {
                for (index, entry) in entries.iter().enumerate() {
                    validate(entry, items, root, &format!("{path}[{index}]"))?;
                }
            }
        }
        Some("string") => {
            let text = value
                .as_str()
                .ok_or_else(|| format!("{path} 必须是字符串"))?;
            if text.chars().count()
                < rule.get("minLength").and_then(Value::as_u64).unwrap_or(0) as usize
            {
                return Err(format!("{path} 字符串太短"));
            }
        }
        _ => return Err(format!("{path} 的 schema 类型不受支持")),
    }
    Ok(())
}

pub fn parse(value: Value) -> Result<Submission, String> {
    let definition = schema();
    validate(&value, &definition, &definition, "submission")?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn completed(scan: &ScanReport) -> Result<(), String> {
    cold_start::validate_verification(scan)?;
    let report = scan
        .verification
        .as_ref()
        .ok_or("S6 需要已完成的 S5 验证")?;
    if !report.cleanup_complete
        || report.commands.iter().any(|c| {
            !c.modules.is_empty()
                && matches!(
                    c.status,
                    VerificationStatus::Cancelled
                        | VerificationStatus::TimedOut
                        | VerificationStatus::NotRun
                )
        })
    {
        return Err("S5 尚未完整执行并清理，请重新扫描验证".into());
    }
    Ok(())
}

pub fn template(scan: &ScanReport) -> Result<Submission, String> {
    completed(scan)?;
    let mut modules = Vec::new();
    for module in &scan.modules {
        let node = scan
            .view
            .nodes
            .iter()
            .find(|n| n.object_id.as_deref() == Some(&module.id))
            .ok_or("扫描缺少模块对象")?;
        let mut document = Document::parse(&node.content)?;
        document.set("rationale", module.rationale.clone().into())?;
        document.set(
            "confidence",
            serde_yaml::to_value(module.confidence).map_err(|e| e.to_string())?,
        )?;
        modules.push(DraftObject {
            id: module.id.clone(),
            path: node.file.clone().ok_or("扫描模块缺少路径")?,
            text: document.to_text(),
        });
    }
    Ok(Submission {
        scan_id: scan.id.clone(),
        project_id: scan.project_id.clone(),
        source_revision: scan.revision.clone(),
        intents: vec![],
        modules,
        interfaces: vec![],
        acceptances: vec![],
        questions: vec![],
        risks: vec![],
    })
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}
fn check_path(path: &str, folder: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 2 && parts[0] == folder && parts[1].strip_suffix(".md").is_some_and(safe_id)
}
fn string_list(document: &Document, key: &str) -> Result<Vec<String>, String> {
    document
        .get(key)
        .and_then(serde_yaml::Value::as_sequence)
        .ok_or_else(|| format!("{key} 必须是字符串数组"))?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} 包含无效字符串"))
        })
        .collect()
}
fn reference(document: &Document, key: &str, modules: &BTreeSet<&str>) -> Result<String, String> {
    let id = document
        .get_str(key)
        .ok_or_else(|| format!("{key} 必须引用模块"))?;
    if !modules.contains(id) {
        return Err(format!("{key} 引用了未知模块 {id}"));
    }
    Ok(id.into())
}

pub fn synthesize(
    scan: &ScanReport,
    submission: Submission,
    producer: &str,
    created_at: &str,
    id: &str,
) -> Result<BaselineDraft, String> {
    // Public typed callers obey the very same schema as CLI JSON callers.
    parse(serde_json::to_value(&submission).map_err(|e| e.to_string())?)?;
    completed(scan)?;
    if submission.scan_id != scan.id
        || submission.project_id != scan.project_id
        || submission.source_revision != scan.revision
    {
        return Err("综合结果与扫描、项目或源提交不匹配".into());
    }
    let producer_parts: Vec<_> = producer.split('/').collect();
    if producer_parts.len() != 2
        || producer_parts
            .iter()
            .any(|p| p.is_empty() || p.chars().any(|c| c.is_whitespace() || c == ':'))
    {
        return Err("综合署名必须是自报 harness/version".into());
    }
    if !id.strip_prefix("d-").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err("草稿 ID 无效".into());
    }
    chrono::DateTime::parse_from_rfc3339(created_at).map_err(|_| "草稿时间无效")?;
    let module_ids: BTreeSet<_> = scan.modules.iter().map(|m| m.id.as_str()).collect();
    if submission.modules.len() != module_ids.len()
        || submission
            .modules
            .iter()
            .map(|m| m.id.as_str())
            .collect::<BTreeSet<_>>()
            != module_ids
    {
        return Err("S6 模块须与扫描一一对应；结构变动留待人工审阅".into());
    }
    let mut view = scan.view.clone();
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut new_index = 0i64;
    let new_y = view.nodes.iter().map(|n| n.y + n.height).max().unwrap_or(0) + 100;
    for (objects, expected, folder) in [
        (&submission.modules, "Module", "arch"),
        (&submission.intents, "Intent", "intents"),
        (&submission.interfaces, "Interface", "arch"),
        (&submission.acceptances, "Acceptance", "specs"),
    ] {
        for object in objects {
            if !safe_id(&object.id) || !ids.insert(object.id.clone()) {
                return Err("对象 ID 无效或重复".into());
            }
            if !check_path(&object.path, folder) || !paths.insert(object.path.clone()) {
                return Err(format!("对象路径无效或重复：{}", object.path));
            }
            let mut document = Document::parse(&object.text)?;
            let layer = towermd_core::types::lookup(expected)
                .ok_or("对象类型未登记")?
                .default_layer
                .as_str();
            if document.doc_type() != Some(expected)
                || document.get_str("id") != Some(&object.id)
                || document.get_str("layer") != Some(layer)
            {
                return Err(format!("{} 的 type、id 或层级不匹配", object.id));
            }
            if document.get_str("status").is_some_and(|s| s != "draft")
                || [
                    "verified",
                    "approved_by",
                    "approved_at",
                    "baseline",
                    "git_tag",
                ]
                .iter()
                .any(|key| document.get(key).is_some())
            {
                return Err("S6 不能携带批准或已生效基线".into());
            }
            if ["owner", "actor"].iter().any(|key| {
                document
                    .get_str(key)
                    .is_some_and(|s| s.starts_with("human:"))
            }) || document
                .get("generated")
                .and_then(|v| v.get("by"))
                .and_then(serde_yaml::Value::as_str)
                .is_some_and(|s| s.starts_with("human:"))
            {
                return Err("S6 不能使用 human 身份".into());
            }
            let title = document
                .get_str("title")
                .or_else(|| document.get_str("name"))
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(&object.id)
                .to_owned();
            if expected == "Module" {
                let source = view
                    .nodes
                    .iter_mut()
                    .find(|n| n.object_id.as_deref() == Some(&object.id))
                    .ok_or("扫描缺少模块节点")?;
                let original = Document::parse(&source.content)?;
                for key in ["paths", "verify"] {
                    if string_list(&document, key)? != string_list(&original, key)? {
                        return Err(format!("S6 不得更改模块 {key}；请在人工审阅中修改"));
                    }
                }
                let confidence = document
                    .get("confidence")
                    .and_then(serde_yaml::Value::as_f64)
                    .ok_or("模块缺少置信度")?;
                if !confidence.is_finite()
                    || !(0.0..=1.0).contains(&confidence)
                    || document
                        .get_str("rationale")
                        .is_none_or(|r| r.trim().is_empty())
                {
                    return Err("模块置信度或理由无效".into());
                }
                for key in [
                    "initial_evidence",
                    "verification",
                    "verification_commands",
                    "source_revision",
                ] {
                    document.set(
                        key,
                        original.get(key).ok_or("扫描缺少验证派生字段")?.clone(),
                    )?;
                }
                source.title = title.clone();
                source.file = Some(object.path.clone());
            } else if ["initial_evidence", "verification", "verification_commands"]
                .iter()
                .any(|key| document.get(key).is_some())
            {
                return Err("非模块对象不能携带验证派生字段".into());
            }
            match expected {
                "Interface" => {
                    let from = reference(&document, "from", &module_ids)?;
                    let to = reference(&document, "to", &module_ids)?;
                    if from == to
                        || document
                            .get_str("contract")
                            .is_none_or(|s| s.trim().is_empty())
                    {
                        return Err("接口必须连接不同模块并说明契约".into());
                    }
                    view.edges.push(CanvasEdge {
                        id: format!("interface-{}", object.id),
                        from_node: from,
                        to_node: to,
                        label: Some(format!("接口：{title}")),
                        from_end: Some("none".into()),
                        to_end: Some("arrow".into()),
                        color: None,
                        from_side: None,
                        to_side: None,
                    });
                }
                "Acceptance" => {
                    let module = reference(&document, "module", &module_ids)?;
                    if string_list(&document, "criteria")?.is_empty() {
                        return Err("验收标准不能为空".into());
                    }
                    if document.get("verify").is_some() {
                        let commands: BTreeSet<_> = scan
                            .commands
                            .iter()
                            .filter(|command| command.modules.contains(&module))
                            .map(|command| command.command.as_str())
                            .collect();
                        if string_list(&document, "verify")?
                            .iter()
                            .any(|command| !commands.contains(command.as_str()))
                        {
                            return Err("验收验证命令必须来自对应模块的原 S5 命令".into());
                        }
                    }
                }
                "Intent" => {
                    if document.get("allocated_to").is_some() {
                        for module in string_list(&document, "allocated_to")? {
                            if !module_ids.contains(module.as_str()) {
                                return Err("意图引用未知模块".into());
                            }
                        }
                    }
                    if document.get("text").is_some()
                        && document
                            .get_str("text")
                            .is_none_or(|text| text.trim().is_empty())
                    {
                        return Err("意图 text 必须是非空文字".into());
                    }
                    if document.body().trim().is_empty() && document.get_str("text").is_none() {
                        return Err("意图需要正文或 text".into());
                    }
                    if document.get("constraints").is_some() {
                        string_list(&document, "constraints")?;
                    }
                    if let Some(priority) = document.get("priority") {
                        if !priority.as_i64().is_some_and(|number| number >= 0)
                            && !priority
                                .as_str()
                                .is_some_and(|text| !text.trim().is_empty())
                        {
                            return Err("意图 priority 必须是非负整数或非空文字".into());
                        }
                    }
                }
                _ => {}
            }
            if let Some(interfaces) = document.get("interfaces").filter(|_| expected == "Module") {
                let refs: Vec<String> = serde_yaml::from_value(interfaces.clone())
                    .map_err(|_| "interfaces 必须是字符串数组")?;
                if refs
                    .iter()
                    .any(|id| !submission.interfaces.iter().any(|item| &item.id == id))
                {
                    return Err("模块引用未知接口".into());
                }
            }
            document.set("status", "draft".into())?;
            document.set(
                "generated",
                serde_yaml::to_value(json!({"by": producer, "at": created_at}))
                    .map_err(|e| e.to_string())?,
            )?;
            if expected == "Module" {
                view.nodes
                    .iter_mut()
                    .find(|n| n.object_id.as_deref() == Some(&object.id))
                    .unwrap()
                    .content = document.to_text();
            } else {
                if view.nodes.iter().any(|n| n.id == object.id) {
                    return Err("对象 ID 与画布节点冲突".into());
                }
                view.nodes.push(CanvasNode {
                    id: object.id.clone(),
                    kind: "file".into(),
                    title,
                    content: document.to_text(),
                    x: (new_index % 3) * 360,
                    y: new_y + (new_index / 3) * 260,
                    width: 300,
                    height: 180,
                    file: Some(object.path.clone()),
                    url: None,
                    object_id: Some(object.id.clone()),
                    object_type: Some(expected.into()),
                    set_by: None,
                    region: None,
                });
                new_index += 1;
            }
        }
    }
    view.title = format!(
        "{} · 综合草稿（未批准）",
        scan.view.title.trim_end_matches(" · 候选架构（未批准）")
    );
    view.diagnostics.retain(|s| {
        !s.starts_with("仅 S1–S4 候选：") && !s.starts_with("S5 结果来自固定提交的实际命令运行；")
    });
    for message in &mut view.diagnostics {
        if message.starts_with("按目录构造单层候选，") {
            *message = message.replace(
                "关系线仅代表历史共同修改，不代表接口。",
                "共同修改连线代表历史耦合；标有“接口”的连线来自本次综合。",
            )
        }
    }
    view.diagnostics.push(
        "S6 会话 Agent 综合草稿；署名为自报，尚未经人审阅或批准 B0。模块验证结果仍来自原 S5。"
            .into(),
    );
    Ok(BaselineDraft {
        id: id.into(),
        producer: producer.into(),
        self_reported: true,
        created_at: created_at.into(),
        submission,
        scan: scan.clone(),
        view,
        review: None,
    })
}
