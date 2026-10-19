use crate::canvas::CanvasDocument;
use crate::catalog::Catalog;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// App-local observation preference key for one physical project. This is
/// derivable metadata, never a project token, business identity or authority.
pub fn observation_key(project_id: &str, common_dir: &str) -> Result<String, String> {
    crate::recovery_contract::canonical_hash(&(project_id, common_dir))
}

pub struct CanvasView;

impl CanvasView {
    pub fn load(catalog: &Catalog) -> Result<Value, String> {
        let objects: BTreeMap<_,_> = catalog.objects.iter().map(|(id,document)| (id, json!({"path":catalog.object_paths.get(id),"text":document.to_text(),"type":document.get_str("type")}))).collect();
        let modules: BTreeSet<_> = catalog
            .objects
            .iter()
            .filter(|(_, document)| document.get_str("type") == Some("Module"))
            .map(|(id, _)| id.clone())
            .collect();
        let mut diagnostics: Vec<Value> = catalog
            .diagnostics
            .iter()
            .map(|diagnostic| json!(diagnostic))
            .collect();
        let path = format!("{}/tower.canvas", catalog.config.docs_dir());
        let mut view = json!({"project_id":catalog.config.project_id(),"revision":catalog.revision,"status":"missing","canvas_text":null,"nodes":[],"edges":[],"objects":objects,"unplaced":modules,"diagnostics":diagnostics});
        let text = match catalog.read_file(&path) {
            Ok(Some(text)) => text,
            Ok(None) => return Ok(view),
            Err(error) => {
                view["status"] = "invalid".into();
                diagnostics.push(json!({"code":"unsafe_canvas","reason":error}));
                view["diagnostics"] = json!(diagnostics);
                return Ok(view);
            }
        };
        view["canvas_text"] = text.clone().into();
        let canvas = match CanvasDocument::decode(&text) {
            Ok(canvas) => canvas,
            Err(error) => {
                view["status"] = "invalid".into();
                diagnostics.push(json!({"code":"invalid_canvas","reason":error}));
                view["diagnostics"] = json!(diagnostics);
                return Ok(view);
            }
        };
        let mut nodes = Vec::new();
        let mut placed = BTreeSet::new();
        for entry in canvas.nodes() {
            let node = &entry.raw;
            let mut result =
                json!({"index":entry.index,"entry":node,"status":"annotation","text":null});
            if entry.opaque {
                result["status"] = "opaque".into();
            } else if node["type"] == "file" {
                let file = normalize_reference(node["file"].as_str().unwrap_or(""));
                match file {
                    Err(error) => {
                        result["status"] = "unsafe".into();
                        result["reason"] = error.into();
                    }
                    Ok(path) => {
                        result["path"] = path.clone().into();
                        if let Some(object) = node["towermd"]["obj"].as_str() {
                            match catalog.object_paths.get(object) {
                                None => result["status"] = "dangling".into(),
                                Some(expected) if expected != &path => {
                                    result["status"] = "mismatch".into()
                                }
                                Some(_) => {
                                    result["status"] = "bound".into();
                                    placed.insert(object.to_owned());
                                    result["text"] = catalog.objects[object].to_text().into();
                                }
                            }
                        } else {
                            match catalog.read_file(&path) {
                                Ok(Some(text)) => {
                                    result["status"] = "reference".into();
                                    result["text"] = text.into();
                                }
                                Ok(None) => result["status"] = "dangling".into(),
                                Err(error) => {
                                    result["status"] = "unsafe".into();
                                    result["reason"] = error.into();
                                }
                            }
                        }
                    }
                }
            }
            nodes.push(result);
        }
        let edges: Vec<_> = canvas
            .edges()
            .iter()
            .map(|entry| {
                let status = if entry.opaque {
                    "opaque"
                } else if let Some(object) = entry.raw["towermd"]["obj"].as_str() {
                    match catalog.objects.get(object) {
                        Some(document) if document.get_str("type") == Some("Interface") => "bound",
                        Some(_) => "mismatch",
                        None => {
                            diagnostics.push(json!({
                                "code":"interface_unresolved", "collection":"edges", "index":entry.index,
                                "message":"接口未解析：当前未加载 interfaces.yaml，不能判定接口是否存在",
                                "object":object,
                            }));
                            "unresolved"
                        },
                    }
                } else {
                    "annotation"
                };
                json!({"index":entry.index,"entry":entry.raw,"status":status})
            })
            .collect();
        view["status"] = if canvas.raw().is_some() {
            "loaded"
        } else {
            "opaque"
        }
        .into();
        view["nodes"] = json!(nodes);
        view["edges"] = json!(edges);
        view["unplaced"] = json!(modules.difference(&placed).collect::<Vec<_>>());
        diagnostics.extend(
            canvas
                .diagnostics()
                .iter()
                .map(|diagnostic| json!(diagnostic)),
        );
        view["diagnostics"] = json!(diagnostics);
        Ok(view)
    }
}

fn normalize_reference(path: &str) -> Result<String, String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .next()
            .is_some_and(|part| part.contains(':'))
    {
        return Err("拒绝非仓库相对路径".into());
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("引用越过仓库根目录".into());
                }
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err("引用必须指向文件".into());
    }
    Ok(parts.join("/"))
}
