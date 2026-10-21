//! Deterministic, read-only S1–S4 candidates. These are not baselines or evidence.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use towermd_core::catalog::{main_reference, Catalog};
use towermd_core::okf::Document;
use towermd_core::project_view::{CanvasEdge, CanvasNode, ProjectView};
use towermd_core::repo;

pub use crate::cold_start_verification::{
    apply_verification, validate_verification, validate_verification_logs,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanReport {
    pub id: String,
    pub project_id: String,
    pub reference: String,
    pub revision: String,
    pub tree: String,
    pub view: ProjectView,
    pub modules: Vec<ScanModule>,
    pub commands: Vec<ScanCommand>,
    pub unassigned: Vec<String>,
    pub source_file_count: usize,
    pub assigned_file_count: usize,
    pub coverage: f64,
    pub diagnostics: Vec<String>,
    pub hotspots: Vec<Hotspot>,
    pub couplings: Vec<Coupling>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<crate::verifier::VerificationReport>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanModule {
    pub id: String,
    pub title: String,
    pub paths: Vec<String>,
    pub files: Vec<String>,
    pub confidence: f64,
    pub rationale: String,
    pub command_ids: Vec<String>,
    pub suggested_task: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanCommand {
    pub id: String,
    pub directory: String,
    pub command: String,
    pub modules: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hotspot {
    pub path: String,
    pub commits: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coupling {
    pub from: String,
    pub to: String,
    pub commits: usize,
}

const EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "svelte", "vue", "py", "go", "c", "h", "cc",
    "cpp", "hpp", "java", "kt", "swift", "rb", "sh", "css", "scss", "html",
];
const EXCLUDED: &[&str] = &[
    ".git",
    ".towermd",
    "node_modules",
    "vendor",
    "target",
    "dist",
    "build",
    "coverage",
    ".next",
    ".svelte-kit",
    "__pycache__",
    ".venv",
    "venv",
    "generated",
    ".release",
];

fn ignored(path: &str, docs: &str) -> bool {
    path == docs
        || path.starts_with(&format!("{docs}/"))
        || path.split('/').any(|part| EXCLUDED.contains(&part))
        || path.ends_with(".min.js")
        || path.ends_with(".min.css")
        || path.ends_with(".generated.ts")
        || path.ends_with(".g.rs")
}
fn source(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext))
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}
fn under(path: &str, directory: &str) -> bool {
    directory.is_empty()
        || path
            .strip_prefix(directory)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub fn scan(catalog: &Catalog, id: &str) -> Result<ScanReport, String> {
    let reference = main_reference(&catalog.root)?.to_owned();
    let listing = repo::git(
        &catalog.root,
        &["ls-tree", "-r", "-z", "--full-tree", &catalog.revision],
    )?;
    let mut tracked = BTreeSet::new();
    let mut excluded = Vec::new();
    for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
        let (metadata, path) = entry.split_once('\t').ok_or("无法解析固定提交文件清单")?;
        if (metadata.starts_with("100644 blob ") || metadata.starts_with("100755 blob "))
            && !ignored(path, catalog.config.docs_dir())
        {
            tracked.insert(path.to_owned());
        } else {
            excluded.push(path.to_owned());
        }
    }
    let files: Vec<String> = tracked
        .iter()
        .filter(|path| source(path))
        .cloned()
        .collect();
    let file_set: BTreeSet<String> = files.iter().cloned().collect();
    let mut diagnostics = vec![
        "仅 S1–S4 候选：S5 尚未执行，所有命令均尚未验证；S6 尚未由 Agent 综合，尚未批准 B0。".into(),
        format!("源码识别范围：{}；不识别的扩展名不计入源码覆盖率。", EXTENSIONS.join(", ")),
        format!("排除依赖/生成目录 {}、用户文档目录 {}、符号链接、子模块和已识别压缩/生成文件，共 {} 项。", EXCLUDED.join(", "), catalog.config.docs_dir(), excluded.len()),
        "按目录构造单层候选，并用最近 64 个非合并提交的共同修改修正；未解析 import，关系线仅代表历史共同修改，不代表接口。布局为确定性网格初值。".into(),
    ];
    if !excluded.is_empty() {
        diagnostics.push(format!(
            "排除项（最多列出 20 项）：{}",
            excluded
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let non_source: Vec<_> = tracked
        .iter()
        .filter(|path| !source(path))
        .take(20)
        .cloned()
        .collect();
    if !non_source.is_empty() {
        diagnostics.push(format!(
            "非源码文件不计覆盖率（最多列出 20 项）：{}",
            non_source.join(", ")
        ));
    }
    let (history, hotspots) = history(catalog, &file_set, &mut diagnostics)?;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in &files {
        let key = file.split_once('/').map(|(dir, _)| dir).unwrap_or("");
        groups.entry(key.into()).or_default().push(file.clone());
    }
    // Drill through common wrapper directories until there are at least 3 groups.
    while groups.len() < 3 {
        let split = groups.iter().find_map(|(directory, members)| {
            let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for file in members {
                let relative = if directory.is_empty() {
                    file.as_str()
                } else {
                    &file[directory.len() + 1..]
                };
                let key = match relative.split_once('/') {
                    Some((child, _)) if directory.is_empty() => child.into(),
                    Some((child, _)) => format!("{directory}/{child}"),
                    None => directory.clone(),
                };
                children.entry(key).or_default().push(file.clone());
            }
            if children.len() > 1 || children.keys().next().is_some_and(|key| key != directory) {
                Some((directory.clone(), children))
            } else {
                None
            }
        });
        let Some((directory, children)) = split else {
            break;
        };
        groups.remove(&directory);
        groups.extend(children);
    }
    // Coalesce siblings at their real parent, never invent arbitrary partitions.
    while groups.len() > 9 {
        let mut parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for key in groups.keys() {
            parents
                .entry(parent(key).into())
                .or_default()
                .push(key.clone());
        }
        let Some((directory, siblings)) = parents
            .into_iter()
            .filter(|(_, keys)| keys.len() > 1)
            .max_by_key(|(_, keys)| keys.len())
        else {
            break;
        };
        let mut members = Vec::new();
        for key in siblings {
            members.extend(groups.remove(&key).unwrap());
        }
        members.sort();
        groups.entry(directory).or_default().extend(members);
    }
    let mut corrections = Vec::new();
    // Repeated co-change can merge sibling candidates while retaining >=3 groups.
    while groups.len() > 3 {
        let mut strongest: Option<(usize, String, String)> = None;
        let keys: Vec<_> = groups.keys().cloned().collect();
        for (index, a) in keys.iter().enumerate() {
            for b in &keys[index + 1..] {
                if parent(a) != parent(b) {
                    continue;
                }
                let count = history
                    .iter()
                    .filter(|change| {
                        groups[a].iter().any(|f| change.contains(f))
                            && groups[b].iter().any(|f| change.contains(f))
                    })
                    .count();
                // A one-off repository-wide edit is not enough to change a boundary.
                if count >= 2 && strongest.as_ref().is_none_or(|best| count > best.0) {
                    strongest = Some((count, a.clone(), b.clone()));
                }
            }
        }
        let Some((count, a, b)) = strongest else {
            break;
        };
        let mut members = groups.remove(&b).unwrap();
        groups.get_mut(&a).unwrap().append(&mut members);
        groups.get_mut(&a).unwrap().sort();
        corrections.push(format!(
            "历史修正：{a} 与 {b} 在 {count} 次提交中共同修改，合为一个候选模块；需人确认边界。"
        ));
    }
    diagnostics.extend(corrections.iter().cloned());
    if !(3..=9).contains(&groups.len()) {
        diagnostics.push(format!(
            "当前目录结构无法形成每层 3–9 个模块：得到 {} 个；请在 S6/S7 审阅时调整，未虚构模块。",
            groups.len()
        ));
    }
    let mut modules = Vec::new();
    for (index, (directory, members)) in groups.into_iter().enumerate() {
        let title = if directory.is_empty() {
            "根目录".into()
        } else {
            directory
        };
        modules.push(ScanModule {
            id: format!("A-scan-{}", index + 1),
            title,
            paths: members.clone(),
            files: members,
            confidence: 0.5,
            rationale: if corrections.is_empty() {
                "按已跟踪源码的目录边界分组；未做语义综合，需人审阅。".into()
            } else {
                format!(
                    "目录边界结合有限历史共同修改修正；未做语义综合。{}",
                    corrections.join(" ")
                )
            },
            command_ids: Vec::new(),
            suggested_task: None,
        });
    }
    let mut commands = detect_commands(catalog, &tracked, &mut diagnostics)?;
    for module in &mut modules {
        let own: Vec<usize> = commands
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                !c.directory.is_empty() && module.files.iter().any(|file| under(file, &c.directory))
            })
            .map(|(index, _)| index)
            .collect();
        let needs_root_fallback = module.files.iter().any(|file| {
            !own.iter()
                .any(|index| under(file, &commands[*index].directory))
        });
        let mut selected = own;
        if needs_root_fallback {
            selected.extend(
                commands
                    .iter()
                    .enumerate()
                    .filter(|(_, command)| command.directory.is_empty())
                    .map(|(index, _)| index),
            );
        }
        for index in selected {
            module.command_ids.push(commands[index].id.clone());
            commands[index].modules.push(module.id.clone());
        }
        if module.command_ids.is_empty() {
            module.suggested_task = Some(format!(
                "为 {} 补充测试与验证命令（候选任务，尚未创建或执行）",
                module.title
            ));
        }
    }
    let mut couplings = Vec::new();
    for (index, from) in modules.iter().enumerate() {
        for to in &modules[index + 1..] {
            let commits = history
                .iter()
                .filter(|change| {
                    from.files.iter().any(|file| change.contains(file))
                        && to.files.iter().any(|file| change.contains(file))
                })
                .count();
            if commits > 0 {
                couplings.push(Coupling {
                    from: from.id.clone(),
                    to: to.id.clone(),
                    commits,
                });
            }
        }
    }
    let columns = match modules.len() {
        0..=1 => 1,
        2..=4 => 2,
        _ => 3,
    };
    let positions: BTreeMap<_, _> = modules
        .iter()
        .enumerate()
        .map(|(index, module)| (module.id.as_str(), index))
        .collect();
    let mut nodes = modules
        .iter()
        .enumerate()
        .map(|(index, module)| {
            let directories: BTreeSet<_> = module
                .files
                .iter()
                .map(|file| {
                    let directory = parent(file);
                    if directory.is_empty() {
                        "根目录"
                    } else {
                        directory
                    }
                })
                .collect();
            let directory_summary = directories
                .iter()
                .take(3)
                .copied()
                .collect::<Vec<_>>()
                .join("、");
            let more = if directories.len() > 3 {
                format!("等 {} 个目录", directories.len())
            } else {
                String::new()
            };
            let mut doc = Document::new(
                "Module",
                &format!(
                    "{} 个源码文件 · {}{}\n\n{}\n\n{}\n",
                    module.files.len(),
                    directory_summary,
                    more,
                    if module.command_ids.is_empty() {
                        "未发现验证命令；补测试任务仅为候选。"
                    } else {
                        "候选验证命令尚未执行，不构成证据。"
                    },
                    module.rationale,
                ),
            )?;
            for (key, value) in [
                ("id", module.id.as_str()),
                ("title", &module.title),
                ("name", &module.title),
                ("layer", "L1"),
                ("status", "draft"),
            ] {
                doc.set(key, value.into())?;
            }
            doc.set(
                "paths",
                serde_yaml::to_value(&module.paths).map_err(|e| e.to_string())?,
            )?;
            let selected: Vec<_> = commands
                .iter()
                .filter(|command| module.command_ids.contains(&command.id))
                .collect();
            let verify: Vec<_> = selected
                .iter()
                .map(|command| command.command.as_str())
                .collect();
            doc.set(
                "verify",
                serde_yaml::to_value(verify).map_err(|e| e.to_string())?,
            )?;
            doc.set(
                "verification_commands",
                serde_yaml::to_value(selected).map_err(|e| e.to_string())?,
            )?;
            doc.set(
                "confidence",
                serde_yaml::to_value(module.confidence).map_err(|e| e.to_string())?,
            )?;
            doc.set("source_revision", catalog.revision.clone().into())?;
            doc.set("verification", "not_run".into())?;
            Ok(CanvasNode {
                id: module.id.clone(),
                kind: "file".into(),
                title: module.title.clone(),
                content: doc.to_text(),
                x: (index % columns) as i64 * 360,
                y: (index / columns) as i64 * 260,
                width: 300,
                height: 180,
                file: Some(format!("arch/{}.md", module.id)),
                url: None,
                object_id: Some(module.id.clone()),
                object_type: Some("Module".into()),
                region: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let edges = couplings
        .iter()
        .enumerate()
        .map(|(index, coupling)| {
            let same_row = positions[coupling.from.as_str()] / columns
                == positions[coupling.to.as_str()] / columns;
            CanvasEdge {
                id: format!("cochange-{}", index + 1),
                from_node: coupling.from.clone(),
                to_node: coupling.to.clone(),
                label: Some(format!("共同修改 {} 次", coupling.commits)),
                from_end: Some("none".into()),
                to_end: Some("none".into()),
                color: None,
                from_side: Some(if same_row { "right" } else { "bottom" }.into()),
                to_side: Some(if same_row { "left" } else { "top" }.into()),
            }
        })
        .collect();
    let assigned: BTreeSet<_> = modules
        .iter()
        .flat_map(|module| module.files.iter().cloned())
        .collect();
    let unassigned: Vec<_> = files
        .iter()
        .filter(|file| !assigned.contains(*file))
        .cloned()
        .collect();
    // A project summary is a reviewed layout region, not an invented object.
    // It is generated only for this new scan; loading older scans/baselines never adds it.
    nodes.push(CanvasNode {
        id: format!("project-{}", catalog.config.project_id()),
        kind: "group".into(),
        title: catalog.root.file_name().unwrap_or_default().to_string_lossy().into(),
        content: String::new(),
        x: -380,
        y: 0,
        width: 320,
        height: 240,
        file: None,
        url: None,
        object_id: None,
        object_type: None,
        region: Some("project".into()),
    });
    let view = ProjectView {
        title: format!(
            "{} · 候选架构（未批准）",
            catalog
                .root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ),
        reference: reference.clone(),
        revision: catalog.revision.clone(),
        docs_dir: Some(catalog.config.docs_dir().into()),
        diagnostics: diagnostics.clone(),
        nodes,
        edges,
        unplaced: Vec::new(),
    };
    Ok(ScanReport {
        id: id.into(),
        project_id: catalog.config.project_id().into(),
        reference: reference.clone(),
        revision: catalog.revision.clone(),
        tree: repo::git(
            &catalog.root,
            &["rev-parse", &format!("{}^{{tree}}", catalog.revision)],
        )?,
        view,
        modules,
        commands,
        unassigned,
        source_file_count: files.len(),
        assigned_file_count: assigned.len(),
        coverage: if files.is_empty() {
            0.0
        } else {
            assigned.len() as f64 / files.len() as f64
        },
        diagnostics,
        hotspots,
        couplings,
        verification: None,
    })
}

fn history(
    catalog: &Catalog,
    files: &BTreeSet<String>,
    diagnostics: &mut Vec<String>,
) -> Result<(Vec<BTreeSet<String>>, Vec<Hotspot>), String> {
    let commits = repo::git(
        &catalog.root,
        &[
            "log",
            "--format=%H",
            "--no-merges",
            "-n",
            "64",
            &catalog.revision,
            "--",
        ],
    )?;
    let mut changes = Vec::new();
    let mut counts = BTreeMap::new();
    for sha in commits.lines() {
        let paths = repo::git(
            &catalog.root,
            &[
                "diff-tree",
                "--root",
                "--no-commit-id",
                "--name-only",
                "-r",
                "-z",
                sha,
                "--",
            ],
        )?;
        let changed: BTreeSet<String> = paths
            .split('\0')
            .filter(|path| files.contains(*path))
            .map(str::to_owned)
            .collect();
        for path in &changed {
            *counts.entry(path.clone()).or_insert(0usize) += 1;
        }
        if changed.len() <= 200 {
            changes.push(changed);
        } else {
            diagnostics.push(format!(
                "提交 {sha} 同时修改超过 200 个源码文件，仅计热点，不作为模块耦合依据。"
            ));
        }
    }
    let mut hotspots: Vec<_> = counts
        .into_iter()
        .map(|(path, commits)| Hotspot { path, commits })
        .collect();
    hotspots.sort_by(|a, b| b.commits.cmp(&a.commits).then_with(|| a.path.cmp(&b.path)));
    Ok((changes, hotspots))
}

fn detect_commands(
    catalog: &Catalog,
    tracked: &BTreeSet<String>,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<ScanCommand>, String> {
    let mut candidates = BTreeSet::new();
    for path in tracked {
        let name = path.rsplit('/').next().unwrap_or(path);
        if !["package.json", "Cargo.toml", "Makefile"].contains(&name) {
            continue;
        }
        let text = catalog
            .read_file(path)?
            .ok_or_else(|| format!("固定提交缺少 {path}"))?;
        let directory = parent(path).to_owned();
        match name {
            "package.json" => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(value) => {
                    let manager = value["packageManager"]
                        .as_str()
                        .and_then(|v| v.split('@').next())
                        .filter(|v| ["pnpm", "npm", "yarn", "bun"].contains(v))
                        .unwrap_or("npm");
                    for script in ["test", "check"] {
                        if value["scripts"][script]
                            .as_str()
                            .is_some_and(|script| !script.trim().is_empty())
                        {
                            candidates
                                .insert((directory.clone(), format!("{manager} run {script}")));
                        }
                    }
                }
                Err(error) => diagnostics.push(format!("{path} 无法读取候选脚本：{error}")),
            },
            "Cargo.toml"
                if text
                    .lines()
                    .any(|line| matches!(line.trim(), "[package]" | "[workspace]")) =>
            {
                candidates.insert((directory, "cargo test".into()));
            }
            "Makefile" => {
                for target in ["test", "check"] {
                    if text
                        .lines()
                        .any(|line| line.starts_with(&format!("{target}:")))
                    {
                        candidates.insert((directory.clone(), format!("make {target}")));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(candidates
        .into_iter()
        .enumerate()
        .map(|(index, (directory, command))| ScanCommand {
            id: format!("command-{}", index + 1),
            directory,
            command,
            modules: Vec::new(),
        })
        .collect())
}

pub fn canvas_json(report: &ScanReport) -> Result<String, String> {
    let nodes: Vec<_> = report.view.nodes.iter().map(|node| {
        if node.kind == "group" {
            let mut value = json!({"id":node.id,"type":"group","label":node.title,"x":node.x,"y":node.y,"width":node.width,"height":node.height});
            if let Some(region) = &node.region {
                value["towermd"] = json!({"region":region});
            }
            value
        } else {
            json!({"id":node.id,"type":"file","file":node.file,"x":node.x,"y":node.y,"width":node.width,"height":node.height,"towermd":{"obj":node.object_id,"confidence":0.5}})
        }
    }).collect();
    let edges: Vec<_> = report.view.edges.iter().map(|edge| json!({"id":edge.id,"fromNode":edge.from_node,"toNode":edge.to_node,"label":edge.label,"fromEnd":"none","toEnd":"none","fromSide":edge.from_side,"toSide":edge.to_side})).collect();
    serde_json::to_string_pretty(&json!({"nodes":nodes,"edges":edges}))
        .map_err(|error| error.to_string())
}
