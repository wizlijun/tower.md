//! Read-only desktop projection. All document bytes and coordinates come from
//! one authority commit; viewport and selection never become repository state.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canvas::CanvasDocument;
use crate::catalog::{main_reference, read_authority_reference, Catalog};
use crate::repo;
use crate::types::{layer_of, SetBy};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectView {
    pub title: String,
    pub reference: String,
    pub revision: String,
    pub docs_dir: Option<String>,
    pub diagnostics: Vec<String>,
    pub nodes: Vec<CanvasNode>,
    pub edges: Vec<CanvasEdge>,
    pub unplaced: Vec<CanvasNode>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanvasNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    /// FR-188, derived from the object's `layer`; absent for plain cards
    /// and unapproved drafts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_by: Option<SetBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanvasEdge {
    pub id: String,
    pub from_node: String,
    pub to_node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_side: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_side: Option<String>,
}

pub fn load(directory: &Path) -> Result<ProjectView, String> {
    let root = repo::toplevel(directory)?;
    let reference = main_reference(&root)?;
    let revision = read_authority_reference(&root, reference)?.ok_or_else(|| {
        format!("未找到 {reference}。请先建立 main；配置 origin 后需先 fetch origin/main。")
    })?;
    let mut view = ProjectView {
        title: root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        reference: reference.into(),
        revision,
        docs_dir: None,
        diagnostics: Vec::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        unplaced: Vec::new(),
    };
    if let Some(catalog) = Catalog::load_optional(&root)? {
        if catalog.revision != view.revision {
            return Err("读取期间主干已变化，请刷新画布。".into());
        }
        view.docs_dir = Some(catalog.config.docs_dir().into());
        view.diagnostics.extend(
            catalog
                .diagnostics
                .iter()
                .map(|d| format!("{}：{}", d.path, d.message)),
        );
        let canvas_path = format!("{}/tower.canvas", catalog.config.docs_dir());
        match catalog.read_file(&canvas_path)? {
            Some(text) => project_canvas(&catalog, &CanvasDocument::decode(&text)?, &mut view),
            None => view.diagnostics.push(format!(
                "{canvas_path} 尚未提交；已有模块可从未摆放列表查看。"
            )),
        }
        let placed: BTreeSet<_> = view
            .nodes
            .iter()
            .filter_map(|n| n.object_id.as_deref())
            .collect();
        for (id, document) in &catalog.objects {
            if document.doc_type() == Some("Module") && !placed.contains(id.as_str()) {
                view.unplaced.push(CanvasNode {
                    id: id.clone(),
                    kind: "file".into(),
                    title: document.get_str("title").unwrap_or(id).into(),
                    content: document.to_text(),
                    x: 0,
                    y: 0,
                    width: 280,
                    height: 160,
                    file: catalog.object_paths.get(id).cloned(),
                    url: None,
                    object_id: Some(id.clone()),
                    object_type: Some("Module".into()),
                    set_by: Some(layer_of(document).set_by()),
                    region: None,
                });
            }
        }
    } else {
        view.diagnostics.push("主干尚无 Tower.md 配置。App 只读取已提交的配置、对象和 tower.canvas；初始化方法见使用说明。".into());
    }
    if main_reference(&root)? != reference
        || read_authority_reference(&root, reference)?.as_deref() != Some(&view.revision)
    {
        return Err("读取期间主干来源已变化，请刷新画布。".into());
    }
    Ok(view)
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn project_canvas(catalog: &Catalog, canvas: &CanvasDocument, view: &mut ProjectView) {
    view.diagnostics
        .extend(canvas.diagnostics().iter().map(|d| d.message.clone()));
    let mut counts = BTreeMap::new();
    for entry in canvas.nodes() {
        if let Some(id) = entry.raw.get("id").and_then(Value::as_str) {
            *counts.entry(id).or_insert(0) += 1;
        }
    }
    for entry in canvas.nodes() {
        let raw = &entry.raw;
        if entry.opaque || counts.get(raw["id"].as_str().unwrap_or_default()) != Some(&1) {
            continue;
        }
        let kind = raw["type"].as_str().unwrap_or_default();
        let mut node = CanvasNode {
            id: raw["id"].as_str().unwrap_or_default().into(),
            kind: kind.into(),
            title: String::new(),
            content: String::new(),
            x: raw["x"].as_i64().unwrap_or_default(),
            y: raw["y"].as_i64().unwrap_or_default(),
            width: raw["width"].as_i64().unwrap_or(280),
            height: raw["height"].as_i64().unwrap_or(160),
            file: string(raw, "file"),
            url: string(raw, "url"),
            object_id: None,
            object_type: None,
            set_by: None,
            region: raw.get("towermd").and_then(|value| string(value, "region")),
        };
        match kind {
            "text" => {
                node.content = string(raw, "text").unwrap_or_default();
                node.title = node
                    .content
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("文本卡片")
                    .trim_start_matches('#')
                    .trim()
                    .chars()
                    .take(80)
                    .collect();
            }
            "group" => {
                node.title = string(raw, "label").unwrap_or_else(|| "容器".into());
                node.content = node.title.clone();
            }
            "link" => {
                node.title = node.url.clone().unwrap_or_default();
                node.content = node.title.clone();
            }
            "file" => {
                let path = node.file.as_deref().unwrap_or_default();
                node.title = path.rsplit('/').next().unwrap_or(path).into();
                match catalog.read_file(path) {
                    Ok(Some(content)) => node.content = content,
                    other => {
                        let reason = match other {
                            Err(e) => e,
                            _ => "该提交中没有此文件".into(),
                        };
                        node.content = format!("无法读取引用 {path}：{reason}");
                        node.title = format!("悬空引用 · {}", node.title);
                        view.diagnostics.push(node.content.clone());
                    }
                }
                if let Some(id) = raw
                    .get("towermd")
                    .and_then(|v| v.get("obj"))
                    .and_then(Value::as_str)
                {
                    if let Some(document) = catalog
                        .objects
                        .get(id)
                        .filter(|_| catalog.object_paths.get(id).map(String::as_str) == Some(path))
                    {
                        node.title = document.get_str("title").unwrap_or(id).into();
                        node.object_id = Some(id.into());
                        node.object_type = document.doc_type().map(str::to_owned);
                        node.set_by = Some(layer_of(document).set_by());
                    } else {
                        view.diagnostics.push(format!(
                            "节点 {} 的对象 {id} 不存在或与文件路径不匹配。",
                            node.id
                        ));
                        node.title = format!("悬空对象 · {id}");
                    }
                }
            }
            _ => continue,
        }
        view.nodes.push(node);
    }
    let ids: BTreeSet<_> = view.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut edge_counts = BTreeMap::new();
    for entry in canvas.edges() {
        if let Some(id) = entry.raw.get("id").and_then(Value::as_str) {
            *edge_counts.entry(id).or_insert(0) += 1;
        }
    }
    for entry in canvas.edges().iter().filter(|e| !e.opaque) {
        let raw = &entry.raw;
        let from = raw["fromNode"].as_str().unwrap_or_default();
        let to = raw["toNode"].as_str().unwrap_or_default();
        if ids.contains(from)
            && ids.contains(to)
            && edge_counts.get(raw["id"].as_str().unwrap_or_default()) == Some(&1)
        {
            view.edges.push(CanvasEdge {
                id: raw["id"].as_str().unwrap_or_default().into(),
                from_node: from.into(),
                to_node: to.into(),
                label: string(raw, "label"),
                from_end: string(raw, "fromEnd"),
                to_end: string(raw, "toEnd"),
                color: string(raw, "color"),
                from_side: string(raw, "fromSide"),
                to_side: string(raw, "toSide"),
            });
        }
    }
}
