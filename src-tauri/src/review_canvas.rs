//! Read-only canvas comparison tied to the exact fetched review and main snapshot.
use serde_json::{json, Value};
use std::collections::BTreeMap;
use towermd_core::{
    baseline_draft::{DraftObject, FrozenDraft},
    canvas::CanvasDocument,
    catalog::Catalog,
    okf::Document,
};

pub(crate) fn projection(
    text: &str,
    objects: &BTreeMap<String, DraftObject>,
) -> Result<Value, String> {
    let canvas = CanvasDocument::decode(text)?;
    let entries = |nodes: bool| -> Vec<Value> {
        (if nodes {
            canvas.nodes()
        } else {
            canvas.edges()
        })
        .iter()
        .map(|entry| json!({"index":entry.index,"opaque":entry.opaque,"raw":entry.raw}))
        .collect()
    };
    Ok(json!({
        "canvas_view":{"nodes":entries(true),"edges":entries(false),"diagnostics":canvas.diagnostics()},
        "object_views":objects.values().map(|object| {
            let doc = Document::parse(&object.text).ok();
            let fields:BTreeMap<_,_>=["paths","verify","module","from","to"].into_iter().filter_map(|key| {
                doc.as_ref().and_then(|doc|doc.get(key)).map(|value| (key,serde_json::to_value(value).unwrap_or(Value::Null)))
            }).collect();
            json!({"id":object.id,"path":object.path,"text":object.text,"type":doc.as_ref().and_then(|doc|doc.doc_type()),"title":doc.as_ref().and_then(|doc|doc.get_str("title")),"fields":fields})
        }).collect::<Vec<_>>()
    }))
}

fn safe_path(path: &str) -> Result<(), String> {
    towermd_core::recovery_contract::repo_path(path.as_bytes())
}

fn checked_projection(
    text: &str,
    objects: &BTreeMap<String, DraftObject>,
    docs_dir: &str,
) -> Result<Value, String> {
    let canvas = CanvasDocument::decode(text)?;
    if canvas.raw().is_none() {
        return Err("画布无法安全建立投影；仍可查看原文 diff".into());
    }
    for entry in canvas.nodes() {
        if entry.raw["type"] == "file" {
            safe_path(entry.raw["file"].as_str().ok_or("画布文件路径不是字符串")?)?;
        }
    }
    for (id, object) in objects {
        safe_path(&object.path)?;
        if id != &object.id || !object.path.starts_with(&format!("{docs_dir}/")) {
            return Err("比较对象身份或目录不属于所选项目".into());
        }
    }
    projection(text, objects)
}

fn same_project(left: &Catalog, right: &Catalog) -> bool {
    left.root == right.root
        && left.common_dir == right.common_dir
        && left.config.project_id() == right.config.project_id()
        && left.config.docs_dir() == right.config.docs_dir()
}

fn compare(catalog: &Catalog, review: &Value) -> Result<Value, String> {
    let revision = review["main_revision"]
        .as_str()
        .filter(|revision| !revision.is_empty())
        .ok_or("服务未提供审阅时的 main revision；请更新服务后重新加载")?;
    let actual = Catalog::load(&catalog.root)?;
    if !same_project(catalog, &actual) || revision != actual.revision {
        return Err("项目身份或 main 已变化；重新加载审阅后再比较".into());
    }
    let draft: FrozenDraft = serde_json::from_value(review["draft"].clone())
        .map_err(|error| format!("审阅草稿无法建立投影：{error}"))?;
    if draft.bundle.project_id != actual.config.project_id()
        || draft.bundle.docs_dir != actual.config.docs_dir()
    {
        return Err("审阅草稿不属于所选项目".into());
    }
    let baseline: Option<String> = serde_json::from_value(review["current_baseline"].clone())
        .map_err(|error| format!("审阅基线标签无效：{error}"))?;
    let draft_canvas = checked_projection(
        &draft.bundle.canvas_text,
        &draft.bundle.objects,
        actual.config.docs_dir(),
    )?;
    let path = format!("{}/tower.canvas", actual.config.docs_dir());
    let mut diagnostics = vec![];
    let baseline_canvas = if let Some(text) = actual.read_file(&path)? {
        let objects = actual
            .object_paths
            .iter()
            .map(|(id, path)| {
                let text = actual.read_file(path)?.ok_or("main 对象原文不存在")?;
                Ok((
                    id.clone(),
                    DraftObject {
                        id: id.clone(),
                        path: path.clone(),
                        text,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        checked_projection(&text, &objects, actual.config.docs_dir())?
    } else {
        diagnostics.push("所选 main 提交尚无 tower.canvas；没有生成目录预览代替主干画布");
        Value::Null
    };
    let after = Catalog::load(&catalog.root)?;
    if !same_project(&actual, &after) || actual.revision != after.revision {
        return Err("读取比较期间项目身份或 main 已变化；请重新加载审阅".into());
    }
    Ok(
        json!({"state":"available","main_revision":actual.revision,"baseline":baseline,"baseline_canvas":baseline_canvas,"draft_canvas":draft_canvas,"diagnostics":diagnostics}),
    )
}

pub(crate) fn comparison(catalog: &Catalog, review: &Value) -> Value {
    compare(catalog, review).unwrap_or_else(|reason| json!({"state":"unavailable","reason":reason}))
}

#[cfg(test)]
#[path = "review_canvas_tests.rs"]
mod tests;
