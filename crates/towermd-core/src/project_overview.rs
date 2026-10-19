//! Read-only App overview. Repository previews are disposable and never publish a baseline.

use crate::{canvas_view::CanvasView, catalog::Catalog, repo};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverview {
    pub source: String,
    pub revision: String,
    pub title: String,
    pub nodes: Vec<OverviewNode>,
    pub edges: Vec<OverviewEdge>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewNode {
    pub id: String,
    pub title: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obj: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    pub status: String,
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewEdge {
    pub from: String,
    pub to: String,
}

pub fn load(root: &Path) -> Result<ProjectOverview, String> {
    let root = std::path::PathBuf::from(read_git(root, &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|error| format!("无法定位项目根目录：{error}"))?;
    let title = root
        .file_name()
        .unwrap_or(root.as_os_str())
        .to_string_lossy()
        .into_owned();
    if let Some(catalog) = Catalog::load_optional(&root)? {
        let view = CanvasView::load(&catalog)?;
        match view["status"].as_str() {
            Some("missing") => {
                let mut overview = repository_preview(&root, title)?;
                overview
                    .diagnostics
                    .push("权威 main 尚无画布，当前展示 HEAD 的只读目录草稿。".into());
                return Ok(overview);
            }
            Some("loaded") => return baseline_view(&catalog, &view, title),
            _ => {
                return Err(format!(
                    "权威画布无法建立可靠投影：{}",
                    diagnostic_text(&view["diagnostics"]).join("；")
                ))
            }
        }
    }
    repository_preview(&root, title)
}

fn baseline_view(
    catalog: &Catalog,
    view: &Value,
    title: String,
) -> Result<ProjectOverview, String> {
    let mut overview = ProjectOverview {
        source: "baseline".into(),
        revision: catalog.revision.clone(),
        title,
        nodes: Vec::new(),
        edges: Vec::new(),
        diagnostics: diagnostic_text(&view["diagnostics"]),
    };
    let mut seen = BTreeSet::new();
    for node in view["nodes"].as_array().ok_or("画布节点投影格式非法")? {
        if node["status"] == "opaque" {
            continue;
        }
        let entry = &node["entry"];
        let id = entry["id"].as_str().ok_or("画布节点 ID 非法")?;
        if !seen.insert(id.to_owned()) {
            return Err(format!("权威画布存在重复节点 ID：{id}"));
        }
        let kind = entry["type"].as_str().ok_or("画布节点类型非法")?;
        let object = entry["towermd"]["obj"]
            .as_str()
            .and_then(|id| catalog.objects.get(id));
        let text = node["text"]
            .as_str()
            .or_else(|| entry["text"].as_str())
            .or_else(|| entry["url"].as_str())
            .or_else(|| node["reason"].as_str())
            .or_else(|| entry["file"].as_str())
            .unwrap_or("");
        let title = object
            .and_then(|document| document.get_str("title"))
            .or_else(|| entry["label"].as_str())
            .or_else(|| entry["file"].as_str())
            .or_else(|| entry["text"].as_str().and_then(|text| text.lines().next()))
            .unwrap_or(id);
        overview.nodes.push(OverviewNode {
            id: id.into(),
            title: title.into(),
            kind: kind.into(),
            obj: entry["towermd"]["obj"].as_str().map(str::to_owned),
            object_type: object
                .and_then(|document| document.get_str("type"))
                .map(str::to_owned),
            status: node["status"].as_str().unwrap_or("annotation").into(),
            text: text.into(),
            x: entry["x"].as_f64().ok_or("画布 x 非法")?,
            y: entry["y"].as_f64().ok_or("画布 y 非法")?,
            width: entry["width"].as_f64().ok_or("画布 width 非法")?,
            height: entry["height"].as_f64().ok_or("画布 height 非法")?,
        });
    }
    for edge in view["edges"].as_array().ok_or("画布关系投影格式非法")? {
        if edge["status"] == "opaque" {
            continue;
        }
        let entry = &edge["entry"];
        let from = entry["fromNode"].as_str().ok_or("画布关系起点非法")?;
        let to = entry["toNode"].as_str().ok_or("画布关系终点非法")?;
        if seen.contains(from) && seen.contains(to) {
            overview.edges.push(OverviewEdge {
                from: from.into(),
                to: to.into(),
            });
        }
    }
    if let Some(unplaced) = view["unplaced"].as_array().filter(|ids| !ids.is_empty()) {
        overview
            .diagnostics
            .push(format!("{} 个模块尚未放置在权威画布中。", unplaced.len()));
    }
    Ok(overview)
}

fn diagnostic_text(values: &Value) -> Vec<String> {
    values
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| {
            let message = value["message"]
                .as_str()
                .or_else(|| value["reason"].as_str())
                .or_else(|| value["code"].as_str())
                .unwrap_or("未知画布诊断");
            match value["path"].as_str().filter(|path| !path.is_empty()) {
                Some(path) => format!("{path}：{message}"),
                None => message.into(),
            }
        })
        .collect()
}

fn repository_preview(root: &Path, title: String) -> Result<ProjectOverview, String> {
    let mut overview = ProjectOverview {
        source: "repository".into(),
        revision: String::new(),
        title,
        nodes: Vec::new(),
        edges: Vec::new(),
        diagnostics: vec![
            "只读目录草稿：仅展示已提交文件的包含关系，未批准基线，也不表示执行进度。".into(),
        ],
    };
    let head = repo::git_timeout_env_output(
        root,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        Duration::from_secs(10),
        &[],
    )?;
    if !head.status.success() {
        // An absent branch is an unborn repository; broken existing refs must remain errors.
        let branch = read_git(root, &["symbolic-ref", "--quiet", "HEAD"])?;
        let refs = repo::git_timeout_env_output(
            root,
            &["for-each-ref", "--format=%(refname)", &branch],
            Duration::from_secs(10),
            &[],
        )?;
        if !refs.status.success()
            || !refs.stderr.is_empty()
            || String::from_utf8_lossy(&refs.stdout)
                .lines()
                .any(|reference| reference == branch)
            || !head.stderr.is_empty()
        {
            return Err(format!(
                "无法读取项目 HEAD：{}",
                String::from_utf8_lossy(&head.stderr)
            ));
        }
        overview
            .diagnostics
            .push("空仓库尚无提交，提交文件后可刷新目录预览。".into());
        return Ok(overview);
    }
    let revision = String::from_utf8(head.stdout)
        .map_err(|error| error.to_string())?
        .trim_end_matches('\n')
        .to_owned();
    crate::recovery_contract::object_id(&revision)?;
    let tree = read_git(root, &["ls-tree", "-r", "-z", &revision])?;
    let mut paths = Vec::new();
    let mut directories: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in tree.split('\0').filter(|entry| !entry.is_empty()) {
        let (_, path) = entry.split_once('\t').ok_or("Git tree 条目非法")?;
        if path.starts_with(".towermd/") || path == ".towermd" {
            continue;
        }
        paths.push(path.to_owned());
        if let Some((directory, _)) = path.split_once('/') {
            directories
                .entry(directory.into())
                .or_default()
                .push(path.into());
        }
    }
    overview.revision = revision;
    if paths.is_empty() {
        overview
            .diagnostics
            .push("当前提交没有可展示的文件。".into());
        return Ok(overview);
    }
    overview.nodes.push(OverviewNode {
        id: "repository:root".into(),
        title: overview.title.clone(),
        kind: "root".into(),
        obj: None,
        object_type: None,
        status: "draft".into(),
        text: paths.join("\n"),
        x: 0.0,
        y: directories.len().min(3).saturating_sub(1) as f64 * 110.0,
        width: 300.0,
        height: 160.0,
    });
    for (index, (directory, paths)) in directories.into_iter().enumerate() {
        let id = format!("directory:{directory}");
        overview.edges.push(OverviewEdge {
            from: "repository:root".into(),
            to: id.clone(),
        });
        overview.nodes.push(OverviewNode {
            id,
            title: directory,
            kind: "directory".into(),
            obj: None,
            object_type: None,
            status: "draft".into(),
            text: paths.join("\n"),
            x: 400.0 + (index / 3) as f64 * 360.0,
            y: (index % 3) as f64 * 220.0,
            width: 300.0,
            height: 160.0,
        });
    }
    Ok(overview)
}

fn read_git(root: &Path, args: &[&str]) -> Result<String, String> {
    repo::git_timeout(root, args, Duration::from_secs(10))
}
