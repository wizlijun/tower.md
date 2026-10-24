//! Deterministic, read-only S1–S4 candidates. These are not baselines or evidence.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use towermd_core::catalog::{main_reference, Catalog};
use towermd_core::okf::Document;
use towermd_core::project_view::{CanvasEdge, CanvasNode, ProjectView};
use towermd_core::repo;

use crate::baseline_origin::Origin;
use crate::imports::{self, ImportGraph, ImportSummary};

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
    /// Module-to-module import dependencies (S2/S3); absent in older scans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Dependency>,
    /// The 8.2 acceptance recheck; absent in older scans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<CoverageChecks>,
    /// J4: the approved baseline this scan re-enters as a draft; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub from: String,
    pub to: String,
    pub imports: usize,
}
/// Spec 8.2, checked rather than asserted: each flag says whether the scan met
/// the bar, and the import counts show how much of the boundary is evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageChecks {
    pub coverage_ok: bool,
    pub module_count_ok: bool,
    pub commands_ok: bool,
    pub import_coverage: f64,
    pub imports: ImportSummary,
    pub low_confidence: Vec<String>,
}
/// Below this a candidate module is drawn dashed (FR-20).
pub const LOW_CONFIDENCE: f64 = 0.6;
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
    scan_with(catalog, id, None)
}

/// J4: scan the current commit with the approved modules of `origin` as the
/// boundaries, keeping their IDs, commands and canvas positions.
pub fn scan_from_baseline(catalog: &Catalog, id: &str, origin: Origin) -> Result<ScanReport, String> {
    scan_with(catalog, id, Some(origin))
}

fn scan_with(catalog: &Catalog, id: &str, origin: Option<Origin>) -> Result<ScanReport, String> {
    let reference = main_reference(&catalog.root)?.to_owned();
    let listing = repo::git(
        &catalog.root,
        &["ls-tree", "-r", "-z", "--full-tree", &catalog.revision],
    )?;
    let mut tracked = BTreeSet::new();
    let mut objects = BTreeMap::new();
    let mut excluded = Vec::new();
    for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
        let (metadata, path) = entry.split_once('\t').ok_or("无法解析固定提交文件清单")?;
        if (metadata.starts_with("100644 blob ") || metadata.starts_with("100755 blob "))
            && !ignored(path, catalog.config.docs_dir())
        {
            tracked.insert(path.to_owned());
            if let Some(oid) = metadata.split(' ').nth(2) {
                objects.insert(path.to_owned(), oid.to_owned());
            }
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
        match &origin {
            Some(origin) => format!("S2 按 Rust、TypeScript/JavaScript、Svelte、Python、Go 的 import 逐行模式匹配建文件级依赖图；S3 从 {} 进入草稿：沿用已批准模块边界、验证命令与布局，不重新聚类，不属于任何已批准模块的源码文件放入未划分区。依赖线来自 import，共同修改线仅代表历史共同修改，二者都不代表接口。", origin.baseline),
            None => IMPORT_DIAGNOSTIC.into(),
        },
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
    let texts = read_texts(catalog, &objects, |path| {
        (file_set.contains(path) && imports::language(path).is_some())
            || matches!(path.rsplit('/').next(), Some("Cargo.toml" | "go.mod"))
    })?;
    let graph = imports::graph(&file_set, &tracked, &texts);
    let (modules, commands) = match &origin {
        Some(origin) => seeded(origin, &files, &graph, &mut diagnostics)?,
        None => candidates(catalog, &files, &tracked, &graph, &history, &mut diagnostics)?,
    };
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
    let dependencies = module_dependencies(&modules, &graph);
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
            let [x, y, width, height] = placed(
                origin.as_ref(),
                &module.id,
                [(index % columns) as i64 * 360, (index / columns) as i64 * 260, 300, 180],
            );
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
                x,
                y,
                width,
                height,
                file: Some(format!("arch/{}.md", module.id)),
                url: None,
                object_id: Some(module.id.clone()),
                object_type: Some("Module".into()),
                set_by: None,
                region: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let sides = |from: &str, to: &str| {
        let same_row = positions[from] / columns == positions[to] / columns;
        (
            Some(if same_row { "right" } else { "bottom" }.to_owned()),
            Some(if same_row { "left" } else { "top" }.to_owned()),
        )
    };
    let mut edges: Vec<CanvasEdge> = couplings
        .iter()
        .enumerate()
        .map(|(index, coupling)| {
            let (from_side, to_side) = sides(&coupling.from, &coupling.to);
            CanvasEdge {
                id: format!("cochange-{}", index + 1),
                from_node: coupling.from.clone(),
                to_node: coupling.to.clone(),
                label: Some(format!("共同修改 {} 次", coupling.commits)),
                from_end: Some("none".into()),
                to_end: Some("none".into()),
                color: None,
                from_side,
                to_side,
            }
        })
        .collect();
    edges.extend(dependencies.iter().enumerate().map(|(index, dependency)| {
        let (from_side, to_side) = sides(&dependency.from, &dependency.to);
        CanvasEdge {
            id: format!("import-{}", index + 1),
            from_node: dependency.from.clone(),
            to_node: dependency.to.clone(),
            label: Some(format!("依赖 {} 处", dependency.imports)),
            from_end: Some("none".into()),
            to_end: Some("arrow".into()),
            color: None,
            from_side,
            to_side,
        }
    }));
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
        set_by: None,
        region: Some("project".into()),
    });
    if let Some(mut node) = unassigned_node(&unassigned) {
        [node.x, node.y, node.width, node.height] =
            placed(origin.as_ref(), &node.id, [node.x, node.y, node.width, node.height]);
        nodes.push(node);
    }
    if let Some(project) = nodes.iter_mut().find(|node| node.region.as_deref() == Some("project")) {
        [project.x, project.y, project.width, project.height] = placed(
            origin.as_ref(),
            &project.id,
            [project.x, project.y, project.width, project.height],
        );
    }
    let coverage = if files.is_empty() {
        0.0
    } else {
        assigned.len() as f64 / files.len() as f64
    };
    let checks = recheck(&modules, coverage, &graph.summary);
    diagnostics.push(describe(&checks, coverage, modules.len()));
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
        coverage,
        diagnostics,
        hotspots,
        couplings,
        verification: None,
        dependencies,
        checks: Some(checks),
        origin,
    })
}

pub(crate) const IMPORT_DIAGNOSTIC: &str = "S2 按 Rust、TypeScript/JavaScript、Svelte、Python、Go 的 import 逐行模式匹配建文件级依赖图，解析不到的文件退回目录结构；S3 以目录分组为初值做确定性 Louvain 聚类，再用最近 64 个非合并提交的共同修改修正。依赖线来自 import，共同修改线仅代表历史共同修改，二者都不代表接口。布局为确定性网格初值。";

/// Blob text for the paths `wanted` selects, read in one `git cat-file`
/// batch. Files over 1 MiB or not UTF-8 come back as `None`.
pub(crate) fn read_texts(
    catalog: &Catalog,
    objects: &BTreeMap<String, String>,
    wanted: impl Fn(&str) -> bool,
) -> Result<BTreeMap<String, Option<String>>, String> {
    let selected: Vec<(&String, &String)> = objects.iter().filter(|(path, _)| wanted(path)).collect();
    let mut texts = BTreeMap::new();
    if selected.is_empty() {
        return Ok(texts);
    }
    let input: String = selected.iter().map(|(_, oid)| format!("{oid}\n")).collect();
    let output = repo::git_timeout_env_input_output(
        &catalog.root,
        &["cat-file", "--batch"],
        std::time::Duration::from_secs(120),
        &[],
        Some(input.as_bytes()),
    )?;
    if !output.status.success() {
        return Err(format!("无法读取源码对象：{}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let bytes = output.stdout;
    let mut offset = 0;
    for (path, oid) in selected {
        let end = bytes[offset..]
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or("源码对象批量输出被截断")?
            + offset;
        let header = std::str::from_utf8(&bytes[offset..end]).map_err(|_| "源码对象头无效")?;
        let mut fields = header.split(' ');
        if fields.next() != Some(oid.as_str()) || fields.next() != Some("blob") {
            return Err(format!("源码对象 {path} 读取失败：{header}"));
        }
        let size: usize = fields.next().and_then(|v| v.parse().ok()).ok_or("源码对象大小无效")?;
        let start = end + 1;
        let content = bytes.get(start..start + size).ok_or("源码对象批量输出被截断")?;
        let text = (size <= 1024 * 1024)
            .then(|| std::str::from_utf8(content).ok().map(str::to_owned))
            .flatten();
        texts.insert(path.clone(), text);
        offset = start + size + 1;
    }
    Ok(texts)
}

/// S3: Louvain on the import graph, starting from the directory groups. A file
/// without resolved imports stays in its directory (the fallback); groups only
/// merge, so the count never rises above the directory result nor below 3.
fn cluster(
    groups: BTreeMap<String, Vec<String>>,
    graph: &ImportGraph,
) -> (BTreeMap<String, Vec<String>>, Vec<String>) {
    if graph.edges.is_empty() || groups.len() < 2 {
        return (groups, Vec::new());
    }
    let keys: Vec<String> = groups.keys().cloned().collect();
    let mut owner: BTreeMap<&str, usize> = BTreeMap::new();
    for (group, members) in groups.values().enumerate() {
        for file in members {
            owner.insert(file.as_str(), group);
        }
    }
    let nodes: Vec<&str> = owner.keys().copied().collect();
    let index: BTreeMap<&str, usize> = nodes.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let initial: Vec<usize> = nodes.iter().map(|file| owner[file]).collect();
    let mut weights: BTreeMap<(usize, usize), u64> = BTreeMap::new();
    for ((from, to), count) in &graph.edges {
        if let (Some(a), Some(b)) = (index.get(from.as_str()), index.get(to.as_str())) {
            *weights.entry(((*a).min(*b), (*a).max(*b))).or_default() += *count as u64;
        }
    }
    let edges: Vec<(usize, usize, u64)> = weights.into_iter().map(|((a, b), w)| (a, b, w)).collect();
    let result = crate::louvain::communities(nodes.len(), &edges, &initial, 3.min(groups.len()));
    let mut clustered: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut moves: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (node, community) in result.iter().enumerate() {
        clustered.entry(keys[*community].clone()).or_default().push(nodes[node].to_owned());
        if *community != initial[node] {
            *moves.entry((initial[node], *community)).or_default() += 1;
        }
    }
    let label = |key: &str| if key.is_empty() { "根目录".to_owned() } else { key.to_owned() };
    let notes = moves
        .into_iter()
        .map(|((from, to), count)| {
            if clustered.contains_key(&keys[from]) {
                format!("依赖聚类：{count} 个文件按 import 依赖从 {} 移入 {}，需人确认边界。", label(&keys[from]), label(&keys[to]))
            } else {
                format!("依赖聚类：{} 整体并入 {}（{count} 个文件），需人确认边界。", label(&keys[from]), label(&keys[to]))
            }
        })
        .collect();
    (clustered, notes)
}


/// S3 for a fresh scan: directory groups, import clustering and co-change
/// corrections, then the detected verification commands.
fn candidates(
    catalog: &Catalog,
    files: &[String],
    tracked: &BTreeSet<String>,
    graph: &ImportGraph,
    history: &[BTreeSet<String>],
    diagnostics: &mut Vec<String>,
) -> Result<(Vec<ScanModule>, Vec<ScanCommand>), String> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
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
    let (mut groups, clustering) = cluster(groups, &graph);
    diagnostics.extend(clustering.iter().cloned());
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
        let notes: Vec<&String> = corrections
            .iter()
            .chain(&clustering)
            .filter(|note| mentions(note, &directory))
            .collect();
        let merged = corrections.iter().any(|note| mentions(note, &directory));
        let (confidence, rationale) = evidence(&members, &graph, merged, &notes);
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
            confidence,
            rationale,
            command_ids: Vec::new(),
            suggested_task: None,
        });
    }
    let mut commands = detect_commands(catalog, tracked, diagnostics)?;
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
    Ok((modules, commands))
}

/// J4: the approved modules keep their identity, title and commands; each
/// current source file goes to the first module whose approved paths allow it,
/// and a file no approved module allows stays unassigned. No re-clustering.
fn seeded(
    origin: &Origin,
    files: &[String],
    graph: &ImportGraph,
    diagnostics: &mut Vec<String>,
) -> Result<(Vec<ScanModule>, Vec<ScanCommand>), String> {
    let mut members: Vec<Vec<String>> = vec![Vec::new(); origin.modules.len()];
    let mut placed_by_directory = Vec::new();
    for file in files {
        let mut owner = None;
        for (index, module) in origin.modules.iter().enumerate() {
            if crate::task_model::path_is_allowed(file, &module.paths)? {
                owner = Some(index);
                break;
            }
        }
        // A file added since the baseline joins the module that owns most
        // approved files in the same directory; the comparison shows it.
        if owner.is_none() {
            owner = origin
                .modules
                .iter()
                .enumerate()
                .map(|(index, module)| {
                    (module.paths.iter().filter(|path| parent(path) == parent(file)).count(), index)
                })
                .filter(|(count, _)| *count > 0)
                .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
                .map(|(_, index)| index);
            if let Some(index) = owner {
                placed_by_directory.push(format!("{file} → {}", origin.modules[index].title));
            }
        }
        if let Some(index) = owner {
            members[index].push(file.clone());
        }
    }
    if !placed_by_directory.is_empty() {
        diagnostics.push(format!(
            "{} 个新文件不在已批准路径内，按同目录归入已批准模块（需人在对比中确认）：{}",
            placed_by_directory.len(),
            placed_by_directory.into_iter().take(20).collect::<Vec<_>>().join("、")
        ));
    }
    let pairs: BTreeSet<(&str, &str)> = origin
        .modules
        .iter()
        .flat_map(|m| m.commands.iter().map(|c| (c.directory.as_str(), c.command.as_str())))
        .collect();
    let mut commands: Vec<ScanCommand> = pairs
        .into_iter()
        .enumerate()
        .map(|(index, (directory, command))| ScanCommand {
            id: format!("command-{}", index + 1),
            directory: directory.into(),
            command: command.into(),
            modules: Vec::new(),
        })
        .collect();
    let mut modules = Vec::new();
    for (module, files) in origin.modules.iter().zip(members) {
        let (internal, external) = references(&files, graph);
        let (confidence, _) = evidence(&files, graph, false, &[]);
        if files.is_empty() {
            diagnostics.push(format!(
                "{} 在当前提交下没有匹配的源码文件；请在审阅中合并或确认保留。",
                module.title
            ));
        }
        let mut command_ids = Vec::new();
        for command in &mut commands {
            if module
                .commands
                .iter()
                .any(|c| c.directory == command.directory && c.command == command.command)
            {
                command_ids.push(command.id.clone());
                command.modules.push(module.id.clone());
            }
        }
        modules.push(ScanModule {
            id: module.id.clone(),
            title: module.title.clone(),
            paths: files.clone(),
            confidence,
            rationale: format!(
                "沿用 {} 的已批准边界，未重新聚类：当前提交下模块内依赖 {internal} 处、跨模块依赖 {external} 处；需人审阅是否调整。",
                origin.baseline
            ),
            suggested_task: command_ids.is_empty().then(|| {
                format!("为 {} 补充测试与验证命令（候选任务，尚未创建或执行）", module.title)
            }),
            command_ids,
            files,
        });
    }
    Ok((modules, commands))
}

fn placed(origin: Option<&Origin>, id: &str, default: [i64; 4]) -> [i64; 4] {
    origin
        .and_then(|origin| origin.positions.get(id))
        .copied()
        .unwrap_or(default)
}

fn references(files: &[String], graph: &ImportGraph) -> (usize, usize) {
    let members: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let (mut internal, mut external) = (0usize, 0usize);
    for ((from, to), count) in &graph.edges {
        match (members.contains(from.as_str()), members.contains(to.as_str())) {
            (true, true) => internal += count,
            (true, false) | (false, true) => external += count,
            _ => {}
        }
    }
    (internal, external)
}

fn mentions(note: &str, directory: &str) -> bool {
    let label = if directory.is_empty() { "根目录" } else { directory };
    note.split([' ', '，', '；', '（']).any(|word| word == label)
}

/// FR-20: confidence and rationale from the evidence actually found.
/// Cohesion is internal / (internal + cross-module) import references; support
/// (references per file, capped at 1) pulls thin evidence back toward the
/// directory-only 0.5. A boundary changed by co-change alone loses 0.1.
fn evidence(files: &[String], graph: &ImportGraph, merged: bool, notes: &[&String]) -> (f64, String) {
    let (internal, external) = references(files, graph);
    let total = internal + external;
    let mut confidence = if total == 0 {
        0.5
    } else {
        let cohesion = internal as f64 / total as f64;
        let support = (total as f64 / files.len().max(1) as f64).min(1.0);
        0.5 + 0.8 * (cohesion - 0.5) * support
    };
    if merged {
        confidence -= 0.1;
    }
    let confidence = (confidence.clamp(0.05, 0.95) * 100.0).round() / 100.0;
    let mut rationale = if total == 0 {
        "按已跟踪源码的目录边界分组；未解析到仓库内 import，仅凭目录推断；未做语义综合，需人审阅。".to_owned()
    } else {
        format!(
            "目录边界为初值，按 import 依赖聚类：模块内依赖 {internal} 处、跨模块依赖 {external} 处，内聚度 {}%；未做语义综合，需人审阅。",
            (internal * 100) / total
        )
    };
    for note in notes {
        rationale.push(' ');
        rationale.push_str(note);
    }
    (confidence, rationale)
}

fn module_dependencies(modules: &[ScanModule], graph: &ImportGraph) -> Vec<Dependency> {
    let owner: BTreeMap<&str, usize> = modules
        .iter()
        .enumerate()
        .flat_map(|(index, module)| module.files.iter().map(move |file| (file.as_str(), index)))
        .collect();
    let mut counts: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for ((from, to), count) in &graph.edges {
        if let (Some(a), Some(b)) = (owner.get(from.as_str()), owner.get(to.as_str())) {
            if a != b {
                *counts.entry((*a, *b)).or_default() += count;
            }
        }
    }
    counts
        .into_iter()
        .map(|((from, to), imports)| Dependency {
            from: modules[from].id.clone(),
            to: modules[to].id.clone(),
            imports,
        })
        .collect()
}

/// FR-21: unassigned files sit in one `region: unassigned` group, only when
/// there are any. It is a layout region, never an object.
pub(crate) fn unassigned_node(unassigned: &[String]) -> Option<CanvasNode> {
    if unassigned.is_empty() {
        return None;
    }
    Some(CanvasNode {
        id: "unassigned".into(),
        kind: "group".into(),
        title: format!("未划分区（{} 个文件）", unassigned.len()),
        content: unassigned.join("\n"),
        x: -380,
        y: 280,
        width: 320,
        height: 200,
        file: None,
        url: None,
        object_id: None,
        object_type: None,
        set_by: None,
        region: Some("unassigned".into()),
    })
}

pub(crate) fn recheck(modules: &[ScanModule], coverage: f64, imports: &ImportSummary) -> CoverageChecks {
    CoverageChecks {
        coverage_ok: coverage >= 0.95,
        module_count_ok: (3..=9).contains(&modules.len()),
        commands_ok: modules
            .iter()
            .all(|module| !module.command_ids.is_empty() || module.suggested_task.is_some()),
        import_coverage: if imports.supported_files == 0 {
            0.0
        } else {
            (imports.connected_files as f64 / imports.supported_files as f64 * 100.0).round() / 100.0
        },
        imports: imports.clone(),
        low_confidence: modules
            .iter()
            .filter(|module| module.confidence < LOW_CONFIDENCE)
            .map(|module| module.id.clone())
            .collect(),
    }
}

pub(crate) fn describe(checks: &CoverageChecks, coverage: f64, modules: usize) -> String {
    let mark = |ok: bool| if ok { "通过" } else { "未通过" };
    let mut text = format!(
        "8.2 复检：源码分配 {}%（≥95% {}）；模块数 {modules}（3–9 {}）；每个模块有验证命令或补测试候选（{}）；import 证据覆盖 {}/{} 个可解析文件（{}%），仓库内依赖 {} 处、外部或未解析 {} 处。",
        (coverage * 100.0).round(),
        mark(checks.coverage_ok),
        mark(checks.module_count_ok),
        mark(checks.commands_ok),
        checks.imports.connected_files,
        checks.imports.supported_files,
        (checks.import_coverage * 100.0).round(),
        checks.imports.resolved,
        checks.imports.external,
    );
    if !checks.low_confidence.is_empty() {
        text.push_str(&format!(
            "低置信度模块（虚线显示，请重点审阅）：{}。",
            checks.low_confidence.join("、")
        ));
    }
    text
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
            // Scan evidence, not the synthesis' claim; older scans were all 0.5.
            let confidence = report
                .modules
                .iter()
                .find(|module| node.object_id.as_deref() == Some(module.id.as_str()))
                .map_or(0.5, |module| module.confidence);
            json!({"id":node.id,"type":"file","file":node.file,"x":node.x,"y":node.y,"width":node.width,"height":node.height,"towermd":{"obj":node.object_id,"confidence":confidence}})
        }
    }).collect();
    let edges: Vec<_> = report.view.edges.iter().map(|edge| json!({"id":edge.id,"fromNode":edge.from_node,"toNode":edge.to_node,"label":edge.label,"fromEnd":"none","toEnd":"none","fromSide":edge.from_side,"toSide":edge.to_side})).collect();
    serde_json::to_string_pretty(&json!({"nodes":nodes,"edges":edges}))
        .map_err(|error| error.to_string())
}
