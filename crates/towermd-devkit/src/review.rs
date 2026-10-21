//! Human draft edits are replayable projections; original synthesis and evidence stay immutable.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use sha2::{Digest, Sha256};

use crate::baseline::BaselineDraft;
use towermd_core::{okf::Document, project_view::{CanvasEdge, CanvasNode, ProjectView}};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewedCommand {
    pub directory: String,
    pub command: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewAction {
    Rename {
        node_id: String,
        title: String,
    },
    Move {
        node_id: String,
        x: i64,
        y: i64,
    },
    Merge {
        module_ids: Vec<String>,
        target_id: String,
        title: String,
    },
    Split {
        module_id: String,
        new_id: String,
        title: String,
        files: Vec<String>,
    },
    Interface {
        id: String,
        from: String,
        to: String,
        title: String,
        contract: String,
    },
    RemoveInterface {
        id: String,
    },
    Commands {
        module_id: String,
        commands: Vec<ReviewedCommand>,
    },
    Reference {
        path: String,
        x: i64,
        y: i64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewedModule {
    pub id: String,
    pub title: String,
    pub files: Vec<String>,
    pub commands: Vec<ReviewedCommand>,
    pub verification_pending: bool,
    pub commands_confirmed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewState {
    pub base_id: String,
    pub actions: Vec<ReviewAction>,
    pub actor: String,
    pub pending_modules: Vec<String>,
    pub confirmed_modules: Vec<String>,
    pub modules: Vec<ReviewedModule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<crate::review_verification::ReviewVerification>,
}

pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0', '\n', '\r'])
        && !path.starts_with('/')
        && !path.contains(':')
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && part != ".git" && part != ".towermd"
        })
}
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
fn text(value: &str, name: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(format!("{name} 不能为空或含 NUL"));
    }
    Ok(())
}
fn coordinates(x: i64, y: i64) -> Result<(), String> {
    if !(-10_000_000..=10_000_000).contains(&x) || !(-10_000_000..=10_000_000).contains(&y) {
        return Err("画布坐标超出范围".into());
    }
    Ok(())
}
fn node_mut<'a>(view: &'a mut ProjectView, id: &str) -> Result<&'a mut CanvasNode, String> {
    view.nodes
        .iter_mut()
        .find(|node| node.id == id)
        .ok_or_else(|| format!("画布节点不存在：{id}"))
}
fn module_index(modules: &[ReviewedModule], id: &str) -> Result<usize, String> {
    modules
        .iter()
        .position(|m| m.id == id)
        .ok_or_else(|| format!("模块不存在：{id}"))
}
fn yaml<T: Serialize>(value: T) -> Result<Value, String> {
    serde_yaml::to_value(value).map_err(|e| e.to_string())
}
fn invalidate(module: &mut ReviewedModule) {
    module.verification_pending = true;
    module.commands_confirmed = false;
}
fn rename(view: &mut ProjectView, id: &str, title: &str) -> Result<(), String> {
    text(title, "名称")?;
    let node = node_mut(view, id)?;
    if node.object_id.is_some() {
        let mut document = Document::parse(&node.content)?;
        document.set("title", title.into())?;
        node.content = document.to_text();
    }
    node.title = title.into();
    if let Some(edge) = view
        .edges
        .iter_mut()
        .find(|e| e.id == format!("interface-{id}"))
    {
        edge.label = Some(format!("接口：{title}"));
    }
    Ok(())
}
fn unused_id(view: &ProjectView, id: &str) -> Result<(), String> {
    if !safe_id(id) || view.nodes.iter().any(|n| n.id == id) {
        return Err("新增对象 ID 无效或重复".into());
    }
    let path = format!("arch/{id}.md");
    if view
        .nodes
        .iter()
        .any(|node| node.object_id.is_some() && node.file.as_deref() == Some(path.as_str()))
    {
        return Err("新增对象路径已被其他对象使用".into());
    }
    Ok(())
}

/// Merge references with a many-to-one mapping. Ambiguous split references stay on
/// the original module and are explicitly reported for human review.
fn remap(view: &mut ProjectView, removed: &BTreeSet<String>, target: &str) -> Result<(), String> {
    let mut self_interfaces = BTreeSet::new();
    for node in &mut view.nodes {
        if node.object_id.is_none() {
            continue;
        }
        let mut document = Document::parse(&node.content)?;
        for key in ["from", "to", "module"] {
            if document.get_str(key).is_some_and(|id| removed.contains(id)) {
                document.set(key, target.into())?;
            }
        }
        if let Some(items) = document.get("allocated_to").and_then(Value::as_sequence) {
            let mapped: BTreeSet<_> = items
                .iter()
                .filter_map(Value::as_str)
                .map(|id| {
                    if removed.contains(id) {
                        target.to_owned()
                    } else {
                        id.to_owned()
                    }
                })
                .collect();
            document.set("allocated_to", yaml(mapped)?)?;
        }
        if document.doc_type() == Some("Interface")
            && document.get_str("from") == document.get_str("to")
        {
            self_interfaces.insert(node.id.clone());
        }
        node.content = document.to_text();
    }
    for node in &mut view.nodes {
        if node.object_id.is_none() {
            continue;
        }
        let mut document = Document::parse(&node.content)?;
        if let Some(items) = document.get("interfaces").and_then(Value::as_sequence) {
            let remaining: Vec<_> = items
                .iter()
                .filter(|id| !id.as_str().is_some_and(|id| self_interfaces.contains(id)))
                .cloned()
                .collect();
            document.set("interfaces", yaml(remaining)?)?;
            node.content = document.to_text();
        }
    }
    view.nodes
        .retain(|node| !removed.contains(&node.id) && !self_interfaces.contains(&node.id));
    for edge in &mut view.edges {
        if removed.contains(&edge.from_node) {
            edge.from_node = target.into();
        }
        if removed.contains(&edge.to_node) {
            edge.to_node = target.into();
        }
    }
    view.edges.retain(|edge| {
        edge.from_node != edge.to_node
            && !self_interfaces
                .iter()
                .any(|id| edge.id == format!("interface-{id}"))
    });
    for id in self_interfaces {
        view.diagnostics.push(format!(
            "合并后接口 {id} 成为模块内部关系，已移除；原文仍保存在综合草稿。"
        ));
    }
    Ok(())
}
fn project_modules(view: &mut ProjectView, modules: &[ReviewedModule]) -> Result<(), String> {
    for module in modules {
        let node = node_mut(view, &module.id)?;
        if !module.verification_pending {
            continue;
        }
        let original = Document::parse(&node.content)?;
        let mut front = original.front().clone();
        for key in [
            "initial_evidence",
            "verification",
            "verification_commands",
            "verify",
        ] {
            front.remove(Value::from(key));
        }
        let header = serde_yaml::to_string(&front).map_err(|e| e.to_string())?;
        let mut document = Document::parse(&format!("---\n{header}---\n{}", original.body()))?;
        document.set("paths", yaml(&module.files)?)?;
        document.set(
            "verify",
            yaml(
                module
                    .commands
                    .iter()
                    .map(|c| c.command.as_str())
                    .collect::<Vec<_>>(),
            )?,
        )?;
        document.set("verification_commands", yaml(&module.commands)?)?;
        document.set("verification", "notRun".into())?;
        document.set("review_verification_pending", true.into())?;
        node.content = document.to_text();
    }
    Ok(())
}

pub fn replay(
    base: &BaselineDraft,
    actions: &[ReviewAction],
    actor: &str,
    id: &str,
    created_at: &str,
    references: &BTreeMap<String, String>,
) -> Result<BaselineDraft, String> {
    if base.review.is_some() {
        return Err("审阅必须从原始 S6 草稿重放".into());
    }
    if actor != "human:local" {
        return Err("审阅只接受 App 的本机人工身份".into());
    }
    if !id.strip_prefix("d-").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err("审阅 ID 无效".into());
    }
    chrono::DateTime::parse_from_rfc3339(created_at).map_err(|_| "审阅时间无效")?;
    if actions.is_empty() || actions.len() > 10_000 {
        return Err("审阅动作数量无效".into());
    }
    let mut draft = base.clone();
    let mut modules: Vec<_> = base
        .scan
        .modules
        .iter()
        .map(|module| ReviewedModule {
            id: module.id.clone(),
            title: base
                .view
                .nodes
                .iter()
                .find(|node| node.id == module.id)
                .map(|node| node.title.clone())
                .unwrap_or_else(|| module.title.clone()),
            files: module.files.clone(),
            commands: base
                .scan
                .commands
                .iter()
                .filter(|command| module.command_ids.contains(&command.id))
                .map(|command| ReviewedCommand {
                    directory: command.directory.clone(),
                    command: command.command.clone(),
                })
                .collect(),
            verification_pending: false,
            commands_confirmed: false,
        })
        .collect();
    let original_files: Vec<_> = modules.iter().flat_map(|m| m.files.clone()).collect();
    if original_files.iter().any(|file| !safe_path(file))
        || original_files.iter().collect::<BTreeSet<_>>().len() != original_files.len()
    {
        return Err("原始扫描文件路径或归属无效".into());
    }
    for action in actions {
        match action {
            ReviewAction::Rename { node_id, title } => {
                rename(&mut draft.view, node_id, title)?;
                if let Some(module) = modules.iter_mut().find(|m| m.id == *node_id) {
                    module.title = title.clone();
                }
            }
            ReviewAction::Move { node_id, x, y } => {
                coordinates(*x, *y)?;
                let node = node_mut(&mut draft.view, node_id)?;
                node.x = *x;
                node.y = *y;
            }
            ReviewAction::Merge {
                module_ids,
                target_id,
                title,
            } => {
                text(title, "模块名称")?;
                let selected: BTreeSet<_> = module_ids.iter().cloned().collect();
                if selected.len() < 2
                    || selected.len() != module_ids.len()
                    || !selected.contains(target_id)
                {
                    return Err("合并至少需要两个不同模块，目标须在其中".into());
                }
                for id in module_ids {
                    module_index(&modules, id)?;
                }
                let files: BTreeSet<_> = modules
                    .iter()
                    .filter(|m| selected.contains(&m.id))
                    .flat_map(|m| m.files.clone())
                    .collect();
                let mut commands = Vec::new();
                for command in modules
                    .iter()
                    .filter(|m| selected.contains(&m.id))
                    .flat_map(|m| m.commands.clone())
                {
                    if !commands.contains(&command) {
                        commands.push(command);
                    }
                }
                let target_index = module_index(&modules, target_id)?;
                let target = &mut modules[target_index];
                target.files = files.into_iter().collect();
                target.commands = commands;
                target.title = title.clone();
                invalidate(target);
                let removed = selected.into_iter().filter(|id| id != target_id).collect();
                remap(&mut draft.view, &removed, target_id)?;
                modules.retain(|m| !removed.contains(&m.id));
                rename(&mut draft.view, target_id, title)?;
            }
            ReviewAction::Split {
                module_id,
                new_id,
                title,
                files,
            } => {
                text(title, "模块名称")?;
                unused_id(&draft.view, new_id)?;
                let index = module_index(&modules, module_id)?;
                let chosen: BTreeSet<_> = files.iter().cloned().collect();
                if chosen.is_empty()
                    || chosen.len() != files.len()
                    || chosen.len() >= modules[index].files.len()
                    || chosen
                        .iter()
                        .any(|file| !safe_path(file) || !modules[index].files.contains(file))
                {
                    return Err("拆分须选择原模块部分且不重复的真实源码文件，不能抽空原模块".into());
                }
                let mut child = modules[index].clone();
                child.id = new_id.clone();
                child.title = title.clone();
                child.files = files.clone();
                invalidate(&mut child);
                modules[index].files.retain(|file| !chosen.contains(file));
                invalidate(&mut modules[index]);
                modules.push(child);
                let mut node = node_mut(&mut draft.view, module_id)?.clone();
                node.id = new_id.clone();
                node.object_id = Some(new_id.clone());
                node.file = Some(format!("arch/{new_id}.md"));
                node.x = node.x.saturating_add(340).min(10_000_000);
                node.title = title.clone();
                let mut document = Document::parse(&node.content)?;
                document.set("id", new_id.clone().into())?;
                document.set("title", title.clone().into())?;
                node.content = document.to_text();
                draft.view.nodes.push(node);
                for node in &mut draft.view.nodes {
                    if node.object_type.as_deref() != Some("Intent") {
                        continue;
                    }
                    let mut document = Document::parse(&node.content)?;
                    if let Some(items) = document.get("allocated_to").and_then(Value::as_sequence) {
                        if items.iter().any(|item| item.as_str() == Some(module_id)) {
                            let mut items = items.clone();
                            items.push(new_id.clone().into());
                            document.set("allocated_to", yaml(items)?)?;
                            node.content = document.to_text();
                        }
                    }
                }
                draft.view.diagnostics.push(format!("{module_id} 拆分出 {new_id}；原接口与验收暂仍指向 {module_id}，需要重新审阅归属和验证命令。"));
            }
            ReviewAction::Interface {
                id,
                from,
                to,
                title,
                contract,
            } => {
                text(title, "接口名称")?;
                text(contract, "接口契约")?;
                module_index(&modules, from)?;
                module_index(&modules, to)?;
                if from == to {
                    return Err("接口须连接两个不同模块".into());
                }
                let existing = draft.view.nodes.iter().position(|node| node.id == *id);
                let mut document = if let Some(index) = existing {
                    if draft.view.nodes[index].object_type.as_deref() != Some("Interface") {
                        return Err("接口 ID 与其他对象冲突".into());
                    }
                    let document = Document::parse(&draft.view.nodes[index].content)?;
                    for key in ["from", "to"] {
                        if let Some(old) = document.get_str(key) {
                            let index = module_index(&modules, old)?;
                            invalidate(&mut modules[index]);
                        }
                    }
                    document
                } else {
                    unused_id(&draft.view, id)?;
                    Document::new("Interface", "")?
                };
                for (key, value) in [
                    ("id", id.as_str()),
                    ("layer", "L1"),
                    ("status", "draft"),
                    ("title", title),
                    ("from", from),
                    ("to", to),
                    ("contract", contract),
                ] {
                    document.set(key, value.into())?;
                }
                if let Some(index) = existing {
                    draft.view.nodes[index].content = document.to_text();
                    draft.view.nodes[index].title = title.clone();
                } else {
                    let y = draft
                        .view
                        .nodes
                        .iter()
                        .map(|node| node.y + node.height)
                        .max()
                        .unwrap_or(0)
                        + 60;
                    draft.view.nodes.push(CanvasNode {
                        id: id.clone(),
                        kind: "file".into(),
                        title: title.clone(),
                        content: document.to_text(),
                        x: 0,
                        y,
                        width: 300,
                        height: 180,
                        file: Some(format!("arch/{id}.md")),
                        url: None,
                        object_id: Some(id.clone()),
                        object_type: Some("Interface".into()),
                        region: None,
                    });
                }
                draft
                    .view
                    .edges
                    .retain(|edge| edge.id != format!("interface-{id}"));
                draft.view.edges.push(CanvasEdge {
                    id: format!("interface-{id}"),
                    from_node: from.clone(),
                    to_node: to.clone(),
                    label: Some(format!("接口：{title}")),
                    from_end: Some("none".into()),
                    to_end: Some("arrow".into()),
                    color: None,
                    from_side: None,
                    to_side: None,
                });
                for id in [from, to] {
                    let index = module_index(&modules, id)?;
                    invalidate(&mut modules[index]);
                }
            }
            ReviewAction::RemoveInterface { id } => {
                let node = node_mut(&mut draft.view, id)?;
                if node.object_type.as_deref() != Some("Interface") {
                    return Err("只能移除接口对象".into());
                }
                let document = Document::parse(&node.content)?;
                for key in ["from", "to"] {
                    let index =
                        module_index(&modules, document.get_str(key).ok_or("接口缺少端点")?)?;
                    invalidate(&mut modules[index]);
                }
                draft.view.nodes.retain(|node| node.id != *id);
                draft
                    .view
                    .edges
                    .retain(|edge| edge.id != format!("interface-{id}"));
                for node in &mut draft.view.nodes {
                    if node.object_id.is_none() {
                        continue;
                    }
                    let mut document = Document::parse(&node.content)?;
                    if let Some(items) = document.get("interfaces").and_then(Value::as_sequence) {
                        let remaining: Vec<_> = items
                            .iter()
                            .filter(|item| item.as_str() != Some(id))
                            .cloned()
                            .collect();
                        document.set("interfaces", yaml(remaining)?)?;
                        node.content = document.to_text();
                    }
                }
            }
            ReviewAction::Commands {
                module_id,
                commands,
            } => {
                let index = module_index(&modules, module_id)?;
                let normalized: Vec<_> = commands
                    .iter()
                    .map(|command| ReviewedCommand {
                        directory: if command.directory == "." {
                            String::new()
                        } else {
                            command.directory.clone()
                        },
                        command: command.command.clone(),
                    })
                    .collect();
                let mut unique = BTreeSet::new();
                for command in &normalized {
                    if !command.directory.is_empty() && !safe_path(&command.directory) {
                        return Err("验证命令目录必须是仓库内相对路径".into());
                    }
                    text(&command.command, "验证命令")?;
                    if !unique.insert((&command.directory, &command.command)) {
                        return Err("验证命令重复".into());
                    }
                }
                if modules[index].commands != normalized {
                    modules[index].commands = normalized;
                    invalidate(&mut modules[index]);
                }
                modules[index].commands_confirmed = true;
            }
            ReviewAction::Reference { path, x, y } => {
                coordinates(*x, *y)?;
                if !safe_path(path) || !path.ends_with(".md") {
                    return Err("引用须是仓库内已有 Markdown 文件".into());
                }
                let content = references.get(path).ok_or("固定源提交中没有该引用原文")?;
                let node_id = format!("ref-{:x}", Sha256::digest(path.as_bytes()));
                if let Some(node) = draft.view.nodes.iter_mut().find(|node| node.id == node_id) {
                    if node.object_id.is_some() || node.file.as_deref() != Some(path.as_str()) {
                        return Err("引用节点 ID 与既有对象冲突".into());
                    }
                    node.x = *x;
                    node.y = *y;
                    continue;
                }
                draft.view.nodes.push(CanvasNode {
                    id: node_id,
                    kind: "file".into(),
                    title: path.clone(),
                    content: content.clone(),
                    x: *x,
                    y: *y,
                    width: 300,
                    height: 180,
                    file: Some(path.clone()),
                    url: None,
                    object_id: None,
                    object_type: None,
                    region: None,
                });
            }
        }
    }
    let final_files: Vec<_> = modules.iter().flat_map(|m| m.files.clone()).collect();
    if final_files.len() != original_files.len()
        || final_files.iter().collect::<BTreeSet<_>>()
            != original_files.iter().collect::<BTreeSet<_>>()
    {
        return Err("审阅改变了源码文件覆盖或重复归属".into());
    }
    project_modules(&mut draft.view, &modules)?;
    draft.id = id.into();
    draft.created_at = created_at.into();
    draft.view.title = base
        .view
        .title
        .replace("综合草稿（未批准）", "审阅草稿（未批准）");
    draft
        .view
        .diagnostics
        .push("S7 人工审阅草稿；改动尚未批准为 B0，结构或命令变化的模块须重新验证。".into());
    draft.review = Some(ReviewState {
        base_id: base.id.clone(),
        actions: actions.to_vec(),
        actor: actor.into(),
        pending_modules: modules
            .iter()
            .filter(|m| m.verification_pending)
            .map(|m| m.id.clone())
            .collect(),
        confirmed_modules: modules
            .iter()
            .filter(|m| m.commands_confirmed)
            .map(|m| m.id.clone())
            .collect(),
        modules,
        verification: None,
    });
    Ok(draft)
}
