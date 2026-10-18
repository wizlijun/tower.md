//! Pure S7 edits of real draft objects. These values never confer publication authority.
#[path = "canvas_annotations.rs"]
mod annotations;
#[path = "baseline_scan.rs"]
mod scan;
pub use crate::recovery_contract::baseline::{
    publication_content_hash, publication_freeze, ApprovalFact, DraftBundle, DraftObject,
    FrozenDraft, FrozenFile, ReviewBinding, Verdict,
};
use crate::{canvas::CanvasDocument, okf::Document};
pub use annotations::{AnnotationContent, AnnotationNode, AnnotationRelation};
pub(crate) use scan::{current_source_bundle, freeze_for_save, freeze_source};
pub use scan::{freeze, from_current_baseline, from_scan};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyCommand {
    pub cwd: String,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitPart {
    pub id: String,
    pub title: String,
    pub paths: Vec<String>,
    pub verify: Vec<VerifyCommand>,
    pub acceptance_ids: Vec<String>,
    pub x: i64,
    pub y: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftEdit {
    PutAnnotation {
        node: AnnotationNode,
    },
    RemoveAnnotation {
        node: String,
    },
    PutRelation {
        relation: AnnotationRelation,
    },
    RemoveRelation {
        edge: String,
    },
    Merge {
        modules: Vec<String>,
        keep: String,
        title: String,
    },
    Split {
        module: String,
        parts: Vec<SplitPart>,
        interface_endpoints: BTreeMap<String, [String; 2]>,
    },
    Rename {
        object: String,
        title: String,
    },
    Move {
        node: String,
        x: i64,
        y: i64,
    },
    PutInterface {
        object: DraftObject,
        from: String,
        to: String,
    },
    RemoveInterface {
        interface: String,
    },
    ConfirmVerify {
        module: String,
        commands: Vec<VerifyCommand>,
    },
    AddDocref {
        node: String,
        path: String,
        x: i64,
        y: i64,
    },
}

/// Edits a clone, returning no partial result on failure. AddDocref needs `apply_edit_at`.
pub fn apply_edit(bundle: &DraftBundle, edit: &DraftEdit) -> Result<DraftBundle, String> {
    edit_inner(None, bundle, edit)
}

/// The repository is read only for AddDocref, at the bundle's fixed source revision.
pub fn apply_edit_at(
    root: &Path,
    bundle: &DraftBundle,
    edit: &DraftEdit,
) -> Result<DraftBundle, String> {
    edit_inner(Some(root), bundle, edit)
}

fn edit_inner(
    root: Option<&Path>,
    bundle: &DraftBundle,
    edit: &DraftEdit,
) -> Result<DraftBundle, String> {
    let mut next = bundle.clone();
    match edit {
        DraftEdit::PutAnnotation { node } => annotations::put_node(&mut next, node)?,
        DraftEdit::RemoveAnnotation { node } => annotations::remove_node(&mut next, node)?,
        DraftEdit::PutRelation { relation } => annotations::put_relation(&mut next, relation)?,
        DraftEdit::RemoveRelation { edge } => annotations::remove_relation(&mut next, edge)?,
        DraftEdit::Rename { object, title } => {
            let mut doc = editable(&next, object, None)?;
            nonempty(title, "title")?;
            set(&mut doc, "title", title)?;
            if doc.doc_type() == Some("Interface") {
                let (canvas, mut raw) = canvas(&next)?;
                let edges = bound_edges(&canvas, object)?;
                if !edges.is_empty() {
                    for index in edges {
                        raw["edges"][index]["label"] = json!(title);
                    }
                    save_canvas(&mut next, raw)?;
                }
            }
            write_doc(&mut next, object, doc)?;
        }
        DraftEdit::ConfirmVerify { module, commands } => {
            let mut doc = editable(&next, module, Some("Module"))?;
            validate_commands(commands)?;
            set(&mut doc, "verify", commands)?;
            write_doc(&mut next, module, doc)?;
        }
        DraftEdit::Move { node, x, y } => {
            coordinates(*x, *y)?;
            let (canvas, mut raw) = canvas(&next)?;
            let index = node_index(&canvas, node)?;
            raw["nodes"][index]["x"] = json!(x);
            raw["nodes"][index]["y"] = json!(y);
            save_canvas(&mut next, raw)?;
        }
        DraftEdit::AddDocref { node, path, x, y } => {
            coordinates(*x, *y)?;
            nonempty(node, "node")?;
            ordinary_blob(
                root.ok_or("AddDocref requires repository root")?,
                &next.source_revision,
                path,
            )?;
            let (canvas, mut raw) = canvas(&next)?;
            vacant_id(&canvas, node)?;
            array(&mut raw, "nodes")?.push(json!({"id":node,"type":"file","file":path,
                "x":x,"y":y,"width":320,"height":180}));
            save_canvas(&mut next, raw)?;
        }
        DraftEdit::Merge {
            modules,
            keep,
            title,
        } => merge(&mut next, modules, keep, title)?,
        DraftEdit::Split {
            module,
            parts,
            interface_endpoints,
        } => split(&mut next, module, parts, interface_endpoints)?,
        DraftEdit::PutInterface { object, from, to } => put_interface(&mut next, object, from, to)?,
        DraftEdit::RemoveInterface { interface } => {
            editable(&next, interface, Some("Interface"))?;
            let (canvas, mut raw) = canvas(&next)?;
            let indices = bound_edges(&canvas, interface)?;
            let edges = array(&mut raw, "edges")?;
            for index in indices.into_iter().rev() {
                edges.remove(index);
            }
            update_interface_references(&mut next, interface, &[])?;
            remove_object(&mut next, interface)?;
            save_canvas(&mut next, raw)?;
        }
    }
    Ok(next)
}

fn editable(bundle: &DraftBundle, id: &str, kind: Option<&str>) -> Result<Document, String> {
    let obj = bundle
        .objects
        .get(id)
        .ok_or_else(|| format!("对象 `{id}` 不存在"))?;
    let doc = Document::parse(&obj.text)?;
    if obj.id != id || doc.get_str("id") != Some(id) {
        return Err(format!("对象 `{id}` 身份不一致"));
    }
    let ty = doc.doc_type().unwrap();
    if !["Intent", "Module", "Interface", "Acceptance"].contains(&ty)
        || kind.is_some_and(|kind| kind != ty)
    {
        return Err(format!(
            "对象 `{id}` 不是可编辑的 {}",
            kind.unwrap_or("慢层对象")
        ));
    }
    Ok(doc)
}

fn write_doc(bundle: &mut DraftBundle, id: &str, doc: Document) -> Result<(), String> {
    bundle.objects.get_mut(id).ok_or("对象不存在")?.text = doc.to_text();
    Ok(())
}
fn set<T: Serialize + ?Sized>(doc: &mut Document, key: &str, value: &T) -> Result<(), String> {
    let value = serde_yaml::to_value(value).map_err(|e| e.to_string())?;
    if doc.get(key) == Some(&value) {
        return Ok(());
    }
    doc.set(key, value)
}
fn strings(doc: &Document, field: &str) -> Result<Vec<String>, String> {
    match doc.get(field) {
        None => Ok(vec![]),
        Some(value) => {
            serde_yaml::from_value(value.clone()).map_err(|_| format!("{field} 必须为字符串列表"))
        }
    }
}
fn verify(doc: &Document) -> Result<Vec<VerifyCommand>, String> {
    let value = match doc.get("verify") {
        None => return Ok(vec![]),
        Some(v) => v.clone(),
    };
    let commands: Vec<VerifyCommand> = serde_yaml::from_value(value)
        .map_err(|_| "verify 需要明确 cwd/argv，不能猜测旧命令字符串")?;
    validate_commands(&commands)?;
    Ok(commands)
}
fn validate_commands(commands: &[VerifyCommand]) -> Result<(), String> {
    let commands: Vec<_> = commands
        .iter()
        .map(|command| crate::verifier::VerificationCommand {
            cwd: command.cwd.clone().into(),
            argv: command.argv.clone(),
        })
        .collect();
    crate::verifier::validate_commands(&commands)
}
fn nonempty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.contains('\0') {
        Err(format!("{field} 必须非空且不含 NUL"))
    } else {
        Ok(())
    }
}
fn coordinates(x: i64, y: i64) -> Result<(), String> {
    const MAX: i64 = 9_007_199_254_740_991;
    if (-MAX..=MAX).contains(&x) && (-MAX..=MAX).contains(&y) {
        Ok(())
    } else {
        Err("坐标必须为 JS 安全整数".into())
    }
}
fn canvas(bundle: &DraftBundle) -> Result<(CanvasDocument, Value), String> {
    let canvas = CanvasDocument::decode(&bundle.canvas_text)?;
    let raw = canvas
        .raw()
        .cloned()
        .ok_or("画布含无法安全表示的数据，仅保留原文")?;
    Ok((canvas, raw))
}
fn array<'a>(raw: &'a mut Value, key: &str) -> Result<&'a mut Vec<Value>, String> {
    if raw.get(key).is_none() {
        raw[key] = json!([]);
    }
    raw.get_mut(key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("画布 {key} 必须是数组"))
}
fn save_canvas(bundle: &mut DraftBundle, raw: Value) -> Result<(), String> {
    // Do not run the strict candidate importer over existing, forward-compatible entries.
    bundle.canvas_text = serde_json::to_string_pretty(&raw).map_err(|e| e.to_string())?;
    Ok(())
}
fn node_index(canvas: &CanvasDocument, id: &str) -> Result<usize, String> {
    let matches: Vec<_> = canvas
        .nodes()
        .iter()
        .filter(|entry| entry.raw["id"] == id)
        .collect();
    if matches.len() != 1 || matches[0].opaque {
        return Err(format!("节点 `{id}` 不存在、不唯一或不透明"));
    }
    Ok(matches[0].index)
}
fn vacant_id(canvas: &CanvasDocument, id: &str) -> Result<(), String> {
    if canvas
        .nodes()
        .iter()
        .chain(canvas.edges())
        .any(|entry| entry.raw["id"] == id)
    {
        Err(format!("画布 ID `{id}` 已存在"))
    } else {
        Ok(())
    }
}
fn object_node(canvas: &CanvasDocument, object: &DraftObject) -> Result<usize, String> {
    let matches: Vec<_> = canvas
        .nodes()
        .iter()
        .filter(|entry| {
            entry.raw["towermd"]["obj"] == object.id
                || entry.raw["type"] == "file" && entry.raw["file"] == object.path
        })
        .collect();
    if matches.len() != 1
        || matches[0].opaque
        || matches[0].raw["type"] != "file"
        || matches[0].raw["file"] != object.path
    {
        return Err(format!("对象 `{}` 需要唯一真实 file 节点", object.id));
    }
    node_index(canvas, matches[0].raw["id"].as_str().unwrap())?;
    Ok(matches[0].index)
}
fn bound_edges(canvas: &CanvasDocument, id: &str) -> Result<Vec<usize>, String> {
    let mut indices = vec![];
    for edge in canvas
        .edges()
        .iter()
        .filter(|edge| edge.raw["towermd"]["obj"] == id)
    {
        if edge.opaque
            || canvas
                .edges()
                .iter()
                .filter(|other| other.raw["id"] == edge.raw["id"])
                .count()
                != 1
        {
            return Err(format!("接口 `{id}` 绑定了不透明或 ID 不唯一的边"));
        }
        indices.push(edge.index);
    }
    Ok(indices)
}
fn remove_object(bundle: &mut DraftBundle, id: &str) -> Result<(), String> {
    let obj = bundle.objects.remove(id).ok_or("对象不存在")?;
    bundle.deletions.push(obj.path);
    bundle.deletions.sort();
    bundle.deletions.dedup();
    Ok(())
}
fn references(bundle: &DraftBundle, kind: &str) -> Result<Vec<(String, Document)>, String> {
    let mut found = vec![];
    for (id, object) in &bundle.objects {
        // Unknown types are retained, rather than being rejected by an unrelated edit.
        let doc = Document::parse(&object.text)?;
        if doc.doc_type() == Some(kind) {
            found.push((id.clone(), editable(bundle, id, Some(kind))?));
        }
    }
    Ok(found)
}
fn update_interface_references(
    bundle: &mut DraftBundle,
    interface: &str,
    endpoints: &[&str],
) -> Result<(), String> {
    for (id, mut doc) in references(bundle, "Module")? {
        let before = strings(&doc, "interfaces")?;
        let mut after = before.clone();
        if endpoints.contains(&id.as_str()) {
            if !after.iter().any(|existing| existing == interface) {
                after.push(interface.into());
            }
        } else {
            after.retain(|existing| existing != interface);
        }
        if after != before {
            set(&mut doc, "interfaces", &after)?;
            write_doc(bundle, &id, doc)?;
        }
    }
    Ok(())
}
fn endpoints(doc: &Document) -> Result<[String; 2], String> {
    Ok([
        doc.get_str("from").ok_or("Interface 缺少 from")?.into(),
        doc.get_str("to").ok_or("Interface 缺少 to")?.into(),
    ])
}
fn rewrite_edge(
    raw: &mut Value,
    canvas: &CanvasDocument,
    interface: &str,
    from: &str,
    to: &str,
) -> Result<(), String> {
    for index in bound_edges(canvas, interface)? {
        raw["edges"][index]["fromNode"] = json!(from);
        raw["edges"][index]["toNode"] = json!(to);
    }
    Ok(())
}

fn merge(
    bundle: &mut DraftBundle,
    modules: &[String],
    keep: &str,
    title: &str,
) -> Result<(), String> {
    nonempty(title, "title")?;
    let ids: BTreeSet<_> = modules.iter().cloned().collect();
    if ids.len() < 2 || ids.len() != modules.len() || !ids.contains(keep) {
        return Err("Merge 需要至少两个不同模块，keep 必须在其中".into());
    }
    let (canvas, mut raw) = canvas(bundle)?;
    let mut nodes = BTreeMap::new();
    let mut paths = BTreeSet::new();
    let mut commands = vec![];
    let mut interfaces = BTreeSet::new();
    for id in modules {
        let doc = editable(bundle, id, Some("Module"))?;
        paths.extend(strings(&doc, "paths")?);
        interfaces.extend(strings(&doc, "interfaces")?);
        for command in verify(&doc)? {
            if !commands.contains(&command) {
                commands.push(command);
            }
        }
        let index = object_node(&canvas, &bundle.objects[id])?;
        nodes.insert(
            id.clone(),
            (
                index,
                canvas.nodes()[index].raw["id"].as_str().unwrap().to_owned(),
            ),
        );
    }
    let keep_node = nodes[keep].1.clone();
    for (id, mut doc) in references(bundle, "Interface")? {
        let old = endpoints(&doc)?;
        let new = old.clone().map(|endpoint| {
            if ids.contains(&endpoint) {
                keep.to_owned()
            } else {
                endpoint
            }
        });
        if old == new {
            continue;
        }
        if new[0] == new[1] {
            return Err(format!(
                "接口 `{id}` 合并后成为内部接口；请先 RemoveInterface 删除或调整"
            ));
        }
        let mut ends = vec![];
        for endpoint in &new {
            editable(bundle, endpoint, Some("Module"))?;
            ends.push(
                canvas.nodes()[object_node(&canvas, &bundle.objects[endpoint])?].raw["id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        set(&mut doc, "from", &new[0])?;
        set(&mut doc, "to", &new[1])?;
        write_doc(bundle, &id, doc)?;
        rewrite_edge(&mut raw, &canvas, &id, &ends[0], &ends[1])?;
    }
    for (id, mut doc) in references(bundle, "Acceptance")? {
        if doc
            .get_str("module")
            .is_some_and(|module| ids.contains(module) && module != keep)
        {
            set(&mut doc, "module", keep)?;
            write_doc(bundle, &id, doc)?;
        }
    }
    // Redirect all canvas references to merged module nodes, retaining unrelated/unknown keys.
    let removed_nodes: BTreeSet<_> = nodes
        .iter()
        .filter(|(id, _)| id.as_str() != keep)
        .map(|(_, (_, node))| node.clone())
        .collect();
    for edge in canvas.edges() {
        for field in ["fromNode", "toNode"] {
            if edge.raw[field]
                .as_str()
                .is_some_and(|id| removed_nodes.contains(id))
            {
                if edge.opaque {
                    return Err("合并涉及不透明边".into());
                }
                raw["edges"][edge.index][field] = json!(keep_node);
            }
        }
    }
    array(&mut raw, "nodes")?.retain(|node| {
        !node["id"]
            .as_str()
            .is_some_and(|id| removed_nodes.contains(id))
    });
    let mut doc = editable(bundle, keep, Some("Module"))?;
    set(&mut doc, "title", title)?;
    set(&mut doc, "paths", &paths)?;
    set(&mut doc, "verify", &commands)?;
    set(&mut doc, "interfaces", &interfaces)?;
    write_doc(bundle, keep, doc)?;
    for id in modules.iter().filter(|id| id.as_str() != keep) {
        remove_object(bundle, id)?;
    }
    save_canvas(bundle, raw)
}

pub(crate) fn safe_id(id: &str) -> Result<(), String> {
    nonempty(id, "id")?;
    if id.chars().any(|c| {
        c.is_control()
            || c.is_whitespace()
            || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) || id.ends_with('.')
        || id == "."
        || id == ".."
    {
        Err("id 必须是安全的文件名组件".into())
    } else {
        Ok(())
    }
}
fn split(
    bundle: &mut DraftBundle,
    module: &str,
    parts: &[SplitPart],
    mappings: &BTreeMap<String, [String; 2]>,
) -> Result<(), String> {
    let original = editable(bundle, module, Some("Module"))?;
    let old = bundle.objects[module].clone();
    let (canvas, mut raw) = canvas(bundle)?;
    let old_index = object_node(&canvas, &old)?;
    let old_node = canvas.nodes()[old_index].raw.clone();
    let old_node_id = old_node["id"].as_str().unwrap();
    let ids: BTreeSet<_> = parts.iter().map(|p| p.id.clone()).collect();
    if parts.len() < 2 || ids.len() != parts.len() {
        return Err("Split 需要至少两个 ID 唯一的部分".into());
    }
    let original_paths: BTreeSet<_> = strings(&original, "paths")?.into_iter().collect();
    let mut paths = BTreeSet::new();
    let acceptances: BTreeSet<_> = references(bundle, "Acceptance")?
        .into_iter()
        .filter(|(_, doc)| doc.get_str("module") == Some(module))
        .map(|(id, _)| id)
        .collect();
    let mut assigned = BTreeMap::new();
    let parent = old.path.rsplit_once('/').ok_or("Module path 缺少目录")?.0;
    let mut new_objects = vec![];
    let mut new_nodes = vec![];
    let mut part_nodes = BTreeMap::new();
    let mut node_ids = BTreeSet::new();
    for part in parts {
        safe_id(&part.id)?;
        nonempty(&part.title, "title")?;
        coordinates(part.x, part.y)?;
        validate_commands(&part.verify)?;
        if part.id != module && bundle.objects.contains_key(&part.id) {
            return Err("Split ID 已存在".into());
        }
        let node_id = if part.id == module {
            old_node_id.to_owned()
        } else {
            part.id.clone()
        };
        if !node_ids.insert(node_id.clone()) {
            return Err("Split 生成了重复画布节点 ID".into());
        }
        if node_id != old_node_id {
            vacant_id(&canvas, &node_id)?;
        }
        if part.paths.is_empty() {
            return Err("Split 部分 paths 不能为空".into());
        }
        for path in &part.paths {
            if !original_paths.contains(path) || !paths.insert(path.clone()) {
                return Err("Split paths 必须恰好分配一次且覆盖原范围".into());
            }
        }
        for id in &part.acceptance_ids {
            if !acceptances.contains(id) || assigned.insert(id.clone(), part.id.clone()).is_some() {
                return Err("Split Acceptance 必须明确且恰好分配一次".into());
            }
        }
        let path = if part.id == module {
            old.path.clone()
        } else {
            format!("{parent}/{}.md", part.id)
        };
        if bundle
            .objects
            .values()
            .any(|object| object.id != module && object.path == path)
        {
            return Err("Split 对象路径已占用".into());
        }
        let mut doc = original.clone();
        set(&mut doc, "id", &part.id)?;
        set(&mut doc, "title", &part.title)?;
        set(&mut doc, "paths", &part.paths)?;
        set(&mut doc, "verify", &part.verify)?;
        // Interface ownership is recomputed below from explicit endpoint assignments.
        set(&mut doc, "interfaces", &Vec::<String>::new())?;
        new_objects.push(DraftObject {
            id: part.id.clone(),
            path: path.clone(),
            text: doc.to_text(),
        });
        let mut node = old_node.clone();
        node["id"] = json!(node_id);
        node["file"] = json!(path);
        node["towermd"]["obj"] = json!(part.id);
        node["x"] = json!(part.x);
        node["y"] = json!(part.y);
        new_nodes.push(node);
        part_nodes.insert(part.id.clone(), node_id);
    }
    if paths != original_paths || assigned.keys().cloned().collect::<BTreeSet<_>>() != acceptances {
        return Err("Split 未完整分配 paths 或 Acceptance".into());
    }
    let mut used = BTreeSet::new();
    let mut memberships: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, mut doc) in references(bundle, "Interface")? {
        let old_ends = endpoints(&doc)?;
        if !old_ends.iter().any(|id| id == module) {
            continue;
        }
        let new = mappings
            .get(&id)
            .ok_or_else(|| format!("接口 `{id}` 缺少显式 interface_endpoints"))?;
        if new[0] == new[1] {
            return Err("Split Interface 不得成为自环".into());
        }
        let mut node_ids = vec![];
        for index in 0..2 {
            if old_ends[index] == module {
                if !ids.contains(&new[index]) {
                    return Err("原模块端点必须明确分配到 Split 部分".into());
                }
                node_ids.push(part_nodes[&new[index]].clone());
                memberships
                    .entry(new[index].clone())
                    .or_default()
                    .push(id.clone());
            } else {
                if new[index] != old_ends[index] {
                    return Err("Split 不得改动无关 Interface 端点".into());
                }
                editable(bundle, &new[index], Some("Module"))?;
                node_ids.push(
                    canvas.nodes()[object_node(&canvas, &bundle.objects[&new[index]])?].raw["id"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                );
            }
        }
        set(&mut doc, "from", &new[0])?;
        set(&mut doc, "to", &new[1])?;
        write_doc(bundle, &id, doc)?;
        rewrite_edge(&mut raw, &canvas, &id, &node_ids[0], &node_ids[1])?;
        used.insert(id.clone());
    }
    if used != mappings.keys().cloned().collect() {
        return Err("interface_endpoints 含无关接口".into());
    }
    // A generic edge to the old module cannot be assigned to a part without an explicit interface.
    for edge in canvas.edges() {
        if ["fromNode", "toNode"]
            .iter()
            .any(|field| edge.raw[*field] == old_node_id)
            && !edge.raw["towermd"]["obj"]
                .as_str()
                .is_some_and(|id| used.contains(id))
        {
            return Err("Split 存在未明确分配的画布边".into());
        }
    }
    for (id, part) in assigned {
        let mut doc = editable(bundle, &id, Some("Acceptance"))?;
        set(&mut doc, "module", &part)?;
        write_doc(bundle, &id, doc)?;
    }
    remove_object(bundle, module)?;
    for mut object in new_objects {
        let mut doc = Document::parse(&object.text)?;
        set(
            &mut doc,
            "interfaces",
            memberships.get(&object.id).unwrap_or(&vec![]),
        )?;
        object.text = doc.to_text();
        bundle.deletions.retain(|path| path != &object.path);
        bundle.objects.insert(object.id.clone(), object);
    }
    array(&mut raw, "nodes")?.remove(old_index);
    array(&mut raw, "nodes")?.extend(new_nodes);
    save_canvas(bundle, raw)
}

fn put_interface(
    bundle: &mut DraftBundle,
    object: &DraftObject,
    from: &str,
    to: &str,
) -> Result<(), String> {
    safe_id(&object.id)?;
    if from == to {
        return Err("Interface 两端必须为不同 Module".into());
    }
    editable(bundle, from, Some("Module"))?;
    editable(bundle, to, Some("Module"))?;
    let mut doc = Document::parse(&object.text)?;
    if doc.doc_type() != Some("Interface") || doc.get_str("id") != Some(&object.id) {
        return Err("PutInterface 需要真实 Interface type/id".into());
    }
    let prefix = format!("{}/arch/", bundle.docs_dir);
    crate::recovery_contract::repo_path(object.path.as_bytes())?;
    if !object.path.starts_with(&prefix) || !object.path.ends_with(".md") {
        return Err("Interface path 必须在配置 arch 目录".into());
    }
    let original = if let Some(existing) = bundle.objects.get(&object.id) {
        let original = editable(bundle, &object.id, Some("Interface"))?;
        if existing.path != object.path {
            return Err("PutInterface 不隐式移动既有路径".into());
        }
        for (key, value) in original.front() {
            let key = key.as_str().ok_or("对象 frontmatter 键必须为字符串")?;
            if doc.get(key).is_none() {
                doc.set(key, value.clone())?;
            }
        }
        Some(original)
    } else {
        None
    };
    if bundle
        .objects
        .values()
        .any(|existing| existing.id != object.id && existing.path == object.path)
    {
        return Err("Interface path 已占用".into());
    }
    set(&mut doc, "from", from)?;
    set(&mut doc, "to", to)?;
    let (canvas, mut raw) = canvas(bundle)?;
    let from_node = canvas.nodes()[object_node(&canvas, &bundle.objects[from])?].raw["id"]
        .as_str()
        .unwrap();
    let to_node = canvas.nodes()[object_node(&canvas, &bundle.objects[to])?].raw["id"]
        .as_str()
        .unwrap();
    let edges = bound_edges(&canvas, &object.id)?;
    if edges.is_empty() {
        let edge_id = format!("interface:{}", object.id);
        vacant_id(&canvas, &edge_id)?;
        array(&mut raw, "edges")?.push(json!({"id":edge_id,"fromNode":from_node,"toNode":to_node,
                "label":doc.get_str("title").unwrap_or(&object.id),"towermd":{"obj":object.id}}));
    } else {
        rewrite_edge(&mut raw, &canvas, &object.id, from_node, to_node)?;
        for index in edges {
            raw["edges"][index]["label"] = json!(doc.get_str("title").unwrap_or(&object.id));
        }
    }
    bundle.deletions.retain(|path| path != &object.path);
    let text = match original {
        Some(original) if original.front() == doc.front() && original.body() == doc.body() => {
            original.to_text()
        }
        _ => doc.to_text(),
    };
    bundle.objects.insert(
        object.id.clone(),
        DraftObject {
            text,
            ..object.clone()
        },
    );
    update_interface_references(bundle, &object.id, &[from, to])?;
    save_canvas(bundle, raw)
}

fn ordinary_blob(root: &Path, revision: &str, path: &str) -> Result<(), String> {
    crate::recovery_contract::object_id(revision)?;
    crate::recovery_contract::repo_path(path.as_bytes())?;
    if !path.ends_with(".md") {
        return Err("引用只能指向普通 .md 文档".into());
    }
    if crate::repo::git(root, &["cat-file", "-t", revision])? != "commit" {
        return Err("source_revision 必须是固定 commit OID".into());
    }
    let bytes = crate::repo::git_bytes_timeout(
        root,
        &[
            "--literal-pathspecs",
            "ls-tree",
            "-z",
            "--full-tree",
            revision,
            "--",
            path,
        ],
        Duration::from_secs(10),
    )?;
    let record = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let (metadata, actual) = record
        .strip_suffix('\0')
        .ok_or("source_revision 中不存在引用文档")?
        .split_once('\t')
        .ok_or("无法读取引用 tree entry")?;
    let fields: Vec<_> = metadata.split(' ').collect();
    if fields.len() != 3
        || !["100644", "100755"].contains(&fields[0])
        || fields[1] != "blob"
        || actual != path
    {
        return Err("引用必须为固定 revision 中的普通文件，拒绝 symlink/submodule".into());
    }
    let bytes = crate::repo::git_bytes_timeout(
        root,
        &["cat-file", "blob", fields[2]],
        Duration::from_secs(10),
    )?;
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("引用文档不是 UTF-8：{e}"))?;
    if let Ok(doc) = Document::parse(text) {
        if doc.get("type").is_some() {
            return Err("AddDocref 只引用普通 Markdown，不接管 Tower 对象".into());
        }
    }
    // Ordinary YAML frontmatter may exist, but Tower identity/layer keys must not.
    if text.starts_with("---\n") || text.starts_with("---\r\n") {
        let front: String = text
            .lines()
            .skip(1)
            .take_while(|line| *line != "---")
            .map(|line| format!("{line}\n"))
            .collect();
        if let Ok(front) = serde_yaml::from_str::<serde_yaml::Mapping>(&front) {
            if ["type", "layer", "towermd"]
                .iter()
                .any(|key| front.contains_key(*key))
            {
                return Err("引用文档含 Tower 对象标记".into());
            }
        }
    }
    Ok(())
}
