//! Unbound annotations in the same draft canvas; object and interface edits stay typed.
use super::{
    array, canvas, coordinates, node_index, nonempty, save_canvas, vacant_id, DraftBundle,
};
use crate::canvas::CanvasDocument;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnnotationContent {
    Text { text: String },
    Link { url: String },
    Group { label: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotationNode {
    pub id: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub content: AnnotationContent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotationRelation {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: String,
}

pub(super) fn put_node(bundle: &mut DraftBundle, node: &AnnotationNode) -> Result<(), String> {
    nonempty(&node.id, "annotation id")?;
    coordinates(node.x, node.y)?;
    coordinates(node.width, node.height)?;
    if node.width <= 0 || node.height <= 0 {
        return Err("注释宽高必须为正的安全整数".into());
    }
    if let AnnotationContent::Link { url } = &node.content {
        nonempty(url, "url")?;
    }
    let (document, mut raw) = canvas(bundle)?;
    let index = if document
        .nodes()
        .iter()
        .any(|entry| entry.raw["id"] == node.id)
    {
        let index = node_index(&document, &node.id)?;
        annotation_node(&raw["nodes"][index])?;
        index
    } else {
        vacant_id(&document, &node.id)?;
        let nodes = array(&mut raw, "nodes")?;
        nodes.push(json!({"id":node.id}));
        nodes.len() - 1
    };
    let target = raw["nodes"][index]
        .as_object_mut()
        .ok_or("注释必须为对象")?;
    for (key, value) in [
        ("x", json!(node.x)),
        ("y", json!(node.y)),
        ("width", json!(node.width)),
        ("height", json!(node.height)),
    ] {
        target.insert(key.into(), value);
    }
    let (kind, key, value) = match &node.content {
        AnnotationContent::Text { text } => ("text", "text", text),
        AnnotationContent::Link { url } => ("link", "url", url),
        AnnotationContent::Group { label } => ("group", "label", label),
    };
    if target.get("type").and_then(Value::as_str) != Some(kind) {
        let previous_payload = match target.get("type").and_then(Value::as_str) {
            Some("text") => Some("text"),
            Some("link") => Some("url"),
            Some("group") => Some("label"),
            _ => None,
        };
        if let Some(key) = previous_payload {
            target.remove(key);
        }
    }
    target.insert("type".into(), json!(kind));
    target.insert(key.into(), json!(value));
    save_changed(bundle, &document, raw)
}
pub(super) fn remove_node(bundle: &mut DraftBundle, id: &str) -> Result<(), String> {
    let (document, mut raw) = canvas(bundle)?;
    let index = node_index(&document, id)?;
    annotation_node(&raw["nodes"][index])?;
    let incident: Vec<_> = document
        .edges()
        .iter()
        .filter(|entry| entry.raw["fromNode"] == id || entry.raw["toNode"] == id)
        .collect();
    if incident
        .iter()
        .any(|entry| entry.opaque || bound(&entry.raw))
    {
        return Err("注释关联了对象接口或不透明关系，须先通过对应编辑处理".into());
    }
    array(&mut raw, "nodes")?.remove(index);
    array(&mut raw, "edges")?.retain(|edge| edge["fromNode"] != id && edge["toNode"] != id);
    save_changed(bundle, &document, raw)
}
pub(super) fn put_relation(
    bundle: &mut DraftBundle,
    relation: &AnnotationRelation,
) -> Result<(), String> {
    nonempty(&relation.id, "relation id")?;
    let (document, mut raw) = canvas(bundle)?;
    node_index(&document, &relation.from)?;
    node_index(&document, &relation.to)?;
    let index = if document
        .edges()
        .iter()
        .any(|entry| entry.raw["id"] == relation.id)
    {
        relation_index(&document, &relation.id)?
    } else {
        vacant_id(&document, &relation.id)?;
        let edges = array(&mut raw, "edges")?;
        edges.push(json!({"id":relation.id}));
        edges.len() - 1
    };
    let edge = &mut raw["edges"][index];
    edge["fromNode"] = json!(relation.from);
    edge["toNode"] = json!(relation.to);
    edge["label"] = json!(relation.label);
    save_changed(bundle, &document, raw)
}
pub(super) fn remove_relation(bundle: &mut DraftBundle, id: &str) -> Result<(), String> {
    let (document, mut raw) = canvas(bundle)?;
    let index = relation_index(&document, id)?;
    array(&mut raw, "edges")?.remove(index);
    save_changed(bundle, &document, raw)
}

fn bound(raw: &Value) -> bool {
    raw.get("towermd")
        .and_then(|value| value.get("obj"))
        .is_some()
}
fn annotation_node(raw: &Value) -> Result<(), String> {
    if bound(raw) || !matches!(raw["type"].as_str(), Some("text" | "link" | "group")) {
        return Err("注释编辑不能修改或删除文件/对象绑定节点".into());
    }
    Ok(())
}
fn relation_index(document: &CanvasDocument, id: &str) -> Result<usize, String> {
    let matching: Vec<_> = document
        .edges()
        .iter()
        .filter(|edge| edge.raw["id"] == id)
        .collect();
    if matching.len() != 1 || matching[0].opaque || bound(&matching[0].raw) {
        return Err("关系不存在、不唯一、不透明或绑定了真实接口".into());
    }
    Ok(matching[0].index)
}
fn save_changed(
    bundle: &mut DraftBundle,
    document: &CanvasDocument,
    raw: Value,
) -> Result<(), String> {
    if document.raw() != Some(&raw) {
        save_canvas(bundle, raw)?;
    }
    Ok(())
}
