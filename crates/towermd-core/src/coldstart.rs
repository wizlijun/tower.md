use crate::catalog::Catalog;
use crate::repo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanOptions {
    #[serde(default = "default_history_limit")]
    pub history_limit: u16,
}

fn default_history_limit() -> u16 {
    200
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            history_limit: default_history_limit(),
        }
    }
}

pub const SCAN_ALGORITHM: &str = "directory-history-v2";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanInput {
    pub project_id: String,
    pub root: std::path::PathBuf,
    pub common_dir: std::path::PathBuf,
    pub revision: String,
    pub tree: String,
    pub options: ScanOptions,
    pub algorithm: String,
    pub history_available: bool,
}

pub fn capture_scan_input(catalog: &Catalog, options: &ScanOptions) -> Result<ScanInput, String> {
    if !(1..=200).contains(&options.history_limit) {
        return Err("history_limit 必须为 1–200".into());
    }
    crate::recovery_contract::object_id(&catalog.revision)?;
    let input = ScanInput {
        project_id: catalog.config.project_id().to_owned(),
        root: catalog
            .root
            .canonicalize()
            .map_err(|error| error.to_string())?,
        common_dir: catalog
            .common_dir
            .canonicalize()
            .map_err(|error| error.to_string())?,
        revision: catalog.revision.clone(),
        tree: read_git(
            catalog,
            &["rev-parse", &format!("{}^{{tree}}", catalog.revision)],
        )?,
        options: options.clone(),
        algorithm: SCAN_ALGORITHM.into(),
        history_available: read_git(catalog, &["rev-parse", "--is-shallow-repository"])? != "true",
    };
    Ok(input)
}

pub fn prepare_scan(input: &ScanInput) -> Result<Value, String> {
    validate_input(input)?;
    if input
        .root
        .canonicalize()
        .map_err(|error| error.to_string())?
        != input.root
        || input
            .common_dir
            .canonicalize()
            .map_err(|error| error.to_string())?
            != input.common_dir
    {
        return Err("scan coordinates must be canonical".into());
    }
    let catalog = Catalog::at_revision(&input.root, &input.revision)?
        .ok_or("scan revision has no project configuration")?;
    if catalog.root != input.root
        || catalog.common_dir != input.common_dir
        || catalog.revision != input.revision
        || catalog.config.project_id() != input.project_id
    {
        return Err("scan input does not match actual repository/project".into());
    }
    let result =
        prepare_coldstart_with_history(&catalog, &input.options, Some(input.history_available))?;
    validate_scan_result(input, &result)?;
    Ok(result)
}

fn validate_input(input: &ScanInput) -> Result<(), String> {
    crate::recovery_contract::object_id(&input.revision)?;
    crate::recovery_contract::object_id(&input.tree)?;
    if input.project_id.trim().is_empty()
        || input.algorithm != SCAN_ALGORITHM
        || !(1..=200).contains(&input.options.history_limit)
        || !input.root.is_absolute()
        || !input.common_dir.is_absolute()
    {
        return Err("invalid fixed scan input".into());
    }
    Ok(())
}

pub(crate) fn validate_scan_result(input: &ScanInput, result: &Value) -> Result<(), String> {
    validate_input(input)?;
    if result["history"]["shallow"] != !input.history_available {
        return Err("scan result/input history mismatch".into());
    }
    for (key, expected) in [
        ("project_id", json!(input.project_id)),
        ("revision", json!(input.revision)),
        ("tree", json!(input.tree)),
        ("algorithm", json!(input.algorithm)),
        ("options", json!(input.options)),
    ] {
        if result[key] != expected {
            return Err(format!("scan result/input mismatch: {key}"));
        }
    }
    Ok(())
}

pub fn prepare_coldstart(catalog: &Catalog, options: &ScanOptions) -> Result<Value, String> {
    prepare_coldstart_with_history(catalog, options, None)
}

fn prepare_coldstart_with_history(
    catalog: &Catalog,
    options: &ScanOptions,
    history_available: Option<bool>,
) -> Result<Value, String> {
    if !(1..=200).contains(&options.history_limit) {
        return Err("history_limit 必须为 1–200".into());
    }
    let tree = read_git(
        catalog,
        &["rev-parse", &format!("{}^{{tree}}", catalog.revision)],
    )?;
    let entries = read_git(catalog, &["ls-tree", "-r", "-z", &catalog.revision])?;
    let mut files = Vec::new();
    let mut sources = BTreeSet::new();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut manifests = Vec::new();
    let mut unassigned = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in entries.split('\0').filter(|entry| !entry.is_empty()) {
        let (metadata, path) = entry.split_once('\t').ok_or("非法 Git tree 条目")?;
        let fields: Vec<_> = metadata.split_whitespace().collect();
        if fields.len() != 3 {
            return Err("非法 Git tree 元数据".into());
        }
        let regular = matches!(fields[0], "100644" | "100755") && fields[1] == "blob";
        let language = language(path);
        let excluded = path.starts_with(".towermd/")
            || path.starts_with(&format!("{}/state/", catalog.config.docs_dir()))
            || path.starts_with(&format!("{}/artifacts/", catalog.config.docs_dir()));
        files.push(json!({"path":path,"mode":fields[0],"language":language,"excluded":excluded}));
        if language.is_some() && !excluded {
            sources.insert(path.to_owned());
            if regular {
                groups
                    .entry(module_root(path))
                    .or_default()
                    .push(path.to_owned());
            } else {
                unassigned.push(json!({"path":path,"reason":"not_a_regular_blob"}));
            }
        }
        if regular && !excluded && is_manifest(path) {
            match read_git(catalog, &["cat-file", "blob", fields[2]]) {
                Ok(text) => manifests.extend(commands(path, &text, &mut diagnostics)),
                Err(error) => diagnostics
                    .push(json!({"path":path,"code":"manifest_unreadable","reason":error})),
            }
        }
    }
    refine_small_groups(&mut groups, &mut diagnostics);
    if sources.is_empty() {
        diagnostics.push(json!({"code":"no_source_files","reason":"No source files were found; coverage is not applicable."}));
    }
    let membership: BTreeMap<_, _> = groups
        .iter()
        .flat_map(|(root, members)| {
            members
                .iter()
                .map(move |path| (path.as_str(), root.as_str()))
        })
        .collect();
    let actual_shallow = read_git(catalog, &["rev-parse", "--is-shallow-repository"])? == "true";
    let shallow = match history_available {
        Some(true) if actual_shallow => {
            return Err("captured scan history is no longer fully available".into())
        }
        Some(available) => !available,
        None => actual_shallow,
    };
    let commits = if shallow {
        diagnostics.push(json!({"code":"history_unavailable_shallow","reason":"浅克隆历史不足，不据此修正目录分区"}));
        String::new()
    } else {
        read_git(
            catalog,
            &[
                "rev-list",
                "--no-merges",
                &format!("--max-count={}", options.history_limit),
                &catalog.revision,
            ],
        )?
    };
    let mut occurrences: BTreeMap<String, u64> = BTreeMap::new();
    let mut pairs: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut hotspots: BTreeMap<String, u64> = BTreeMap::new();
    let mut skipped = Vec::new();
    for commit in commits.lines() {
        let changed = read_git(
            catalog,
            &[
                "diff-tree",
                "--root",
                "--no-commit-id",
                "--name-only",
                "--no-renames",
                "-r",
                "-z",
                commit,
            ],
        )?;
        let changed: BTreeSet<_> = changed
            .split('\0')
            .filter(|path| sources.contains(*path))
            .collect();
        for path in &changed {
            *hotspots.entry((*path).into()).or_default() += 1;
        }
        if changed.len() > 100 {
            skipped
                .push(json!({"commit":commit,"reason":"bulk_change","source_files":changed.len()}));
            continue;
        }
        let roots: BTreeSet<_> = changed
            .iter()
            .filter_map(|path| membership.get(*path).map(|root| (*root).to_owned()))
            .collect();
        for root in &roots {
            *occurrences.entry(root.clone()).or_default() += 1;
        }
        let roots: Vec<_> = roots.into_iter().collect();
        for (index, left) in roots.iter().enumerate() {
            for right in &roots[index + 1..] {
                *pairs.entry((left.clone(), right.clone())).or_default() += 1;
            }
        }
    }
    let coupling: Vec<_> = pairs.iter().map(|((left,right),together)| {
        let union = occurrences[left] + occurrences[right] - together;
        json!({"left":left,"right":right,"together":together,"union":union,"score":*together as f64 / union as f64})
    }).collect();
    let mut candidates: Vec<_> = pairs
        .into_iter()
        .filter_map(|((left, right), together)| {
            let union = occurrences[&left] + occurrences[&right] - together;
            (together >= 3 && together * 5 >= union * 4).then_some((left, right, together, union))
        })
        .collect();
    candidates.sort_by(|left, right| {
        (right.2 * left.3)
            .cmp(&(left.2 * right.3))
            .then_with(|| (&left.0, &left.1).cmp(&(&right.0, &right.1)))
    });
    let mut partitions: Vec<Vec<String>> = groups.keys().map(|root| vec![root.clone()]).collect();
    let mut used = BTreeSet::new();
    let mut merges = Vec::new();
    for (left, right, together, union) in candidates {
        if partitions.len() <= 3 || used.contains(&left) || used.contains(&right) {
            continue;
        }
        partitions.retain(|roots| !roots.contains(&left) && !roots.contains(&right));
        partitions.push(vec![left.clone(), right.clone()]);
        used.insert(left.clone());
        used.insert(right.clone());
        merges.push(json!({"roots":[left,right],"together":together,"union":union}));
    }
    partitions.sort();
    let mut modules = Vec::new();
    let mut validations = Vec::new();
    let mut tasks = Vec::new();
    let mut nodes = Vec::new();
    let mut verification_commands = BTreeMap::new();
    for (roots, id) in partitions.iter().zip(module_ids(&partitions)) {
        let members: Vec<_> = roots.iter().flat_map(|root| groups[root].clone()).collect();
        let paths: Vec<_> = roots
            .iter()
            .flat_map(|root| {
                if root == "."
                    || groups
                        .keys()
                        .any(|other| other.starts_with(&format!("{root}/")))
                {
                    groups[root].clone()
                } else {
                    vec![format!("{root}/**")]
                }
            })
            .collect();
        let own: Vec<_> = manifests
            .iter()
            .filter(|command| {
                let cwd = command["cwd"].as_str().unwrap_or(".");
                if cwd == "." {
                    roots.iter().any(|root| root == ".")
                } else {
                    roots.iter().any(|root| root == cwd)
                        || paths.iter().any(|path| {
                            path.strip_suffix("/**").is_some_and(|root| {
                                cwd == root || cwd.starts_with(&format!("{root}/"))
                            })
                        })
                }
            })
            .cloned()
            .collect();
        let shared = own.is_empty();
        let applicable = merge_commands(if shared {
            manifests
                .iter()
                .filter(|command| command["cwd"] == ".")
                .cloned()
                .collect()
        } else {
            own
        });
        let runnable = applicable
            .iter()
            .filter(|command| {
                command["status"] == "not_run"
                    && command["requires_confirmation"] != true
                    && command["argv"]
                        .as_array()
                        .is_some_and(|argv| !argv.is_empty())
            })
            .collect::<Vec<_>>();
        for command in &runnable {
            let value = json!({"cwd":command["cwd"],"argv":command["argv"]});
            verification_commands.insert(
                (
                    command["cwd"].as_str().unwrap().to_owned(),
                    command["argv"].to_string(),
                ),
                value,
            );
        }
        let status = if applicable.is_empty() {
            "no_tests_discovered"
        } else if applicable
            .iter()
            .any(|command| command["status"] == "not_run")
        {
            "not_run"
        } else {
            "unsupported"
        };
        validations.push(json!({"module":id,"tree":tree,"status":status,"commands":applicable,"shared":shared && !applicable.is_empty()}));
        if runnable.is_empty() {
            let reason = no_tests_reason(&applicable);
            tasks.push(json!({"status":"candidate","module":id,"allowed_paths":paths,"title":"补充可重复验证的测试命令","reason":reason}));
        }
        let confidence = if commits.lines().count() < 3 || applicable.is_empty() {
            "low"
        } else if roots.len() > 1 {
            "high"
        } else {
            "medium"
        };
        let reasons = if roots.len() > 1 {
            vec!["目录分区经至少三次高比例共变修正"]
        } else {
            vec!["按目录边界分区，尚未解析 import"]
        };
        let object_path = format!("{}/arch/{id}.md", catalog.config.docs_dir());
        modules.push(json!({"id":id,"roots":roots,"paths":paths,"members":members,"confidence":confidence,"reasons":reasons,"candidate_path":object_path,"state":"candidate"}));
        nodes.push(json!({"id":id,"type":"file","file":object_path,"x":0,"y":0,"width":320,"height":180,"towermd":{"obj":id}}));
    }
    let (hierarchy, mut nodes, height) = build_hierarchy(&modules, nodes, &mut diagnostics);
    if !unassigned.is_empty() {
        nodes.push(json!({"id":"unassigned","type":"group","label":"未划分区","x":0,"y":height+80,"width":1120,"height":180,"towermd":{"region":"unassigned"}}));
    }
    let assigned: usize = groups.values().map(Vec::len).sum();
    let ratio = (!sources.is_empty()).then(|| assigned as f64 / sources.len() as f64);
    let coverage_status = match ratio {
        None => "not_applicable",
        Some(ratio) if ratio >= 0.95 => "sufficient",
        Some(_) => "insufficient",
    };
    Ok(json!({
        "state":"prepared","requires_app_approval":true,"project_id":catalog.config.project_id(),"revision":catalog.revision,"tree":tree,
        "algorithm":SCAN_ALGORITHM,"options":options,"stages":{"S1":"prepared","S2":"directory_fallback","S3":"prepared","S4":if shallow {"unavailable"} else {"prepared"},"S5":"not_run"},
        "files":files,"modules":modules,"unassigned":unassigned,"coverage":{"source_files":sources.len(),"assigned":assigned,"ratio":ratio,"status":coverage_status},
        "history":{"status":if shallow {"unavailable_shallow"} else {"available"},"commits":commits.lines().count(),"shallow":shallow,"coupling":coupling,"hotspots":hotspots,"skipped":skipped,"merges":merges},
        "validation_plans":validations,"verification_commands":verification_commands.into_values().collect::<Vec<_>>(),"candidate_tasks":tasks,"hierarchy":hierarchy,"candidate_canvas":{"nodes":nodes,"edges":[]},"diagnostics":diagnostics,
    }))
}

// Discovery cannot prove the absence of existing tests; describe why none were run.
pub(crate) fn no_tests_reason(commands: &[Value]) -> &'static str {
    if commands.is_empty() {
        "扫描未发现测试；不是测试通过证据"
    } else if commands
        .iter()
        .any(|command| command["requires_confirmation"] == true)
    {
        "验证命令待确认；未执行，不是测试通过证据"
    } else {
        "验证入口格式不支持；未执行，不是测试通过证据"
    }
}

// Refine only when the initial directory heuristic cannot provide three modules.
fn refine_small_groups(groups: &mut BTreeMap<String, Vec<String>>, diagnostics: &mut Vec<Value>) {
    while groups.len() < 3 {
        let next = groups.iter().find_map(|(root, members)| {
            let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for path in members {
                let relative = if root == "." {
                    path.as_str()
                } else {
                    path.strip_prefix(&format!("{root}/")).unwrap()
                };
                let key = relative.split_once('/').map_or_else(
                    || root.clone(),
                    |(part, _)| {
                        if root == "." {
                            part.into()
                        } else {
                            format!("{root}/{part}")
                        }
                    },
                );
                children.entry(key).or_default().push(path.clone());
            }
            (children.len() > 1 || !children.contains_key(root)).then(|| (root.clone(), children))
        });
        let Some((root, children)) = next else {
            break;
        };
        diagnostics.push(json!({"code":"module_partition_refined","root":root,"children":children.keys().collect::<Vec<_>>()}));
        groups.remove(&root);
        groups.extend(children);
    }
    if !groups.is_empty() && groups.len() < 3 {
        diagnostics.push(json!({"code":"module_fanout_unachievable","modules":groups.len(),"reason":"No further source directory boundary can provide three modules."}));
    }
}

fn module_ids(partitions: &[Vec<String>]) -> Vec<String> {
    let bases: Vec<_> = partitions
        .iter()
        .map(|roots| {
            let mut slug = String::new();
            for c in roots.join("-and-").chars() {
                if c.is_alphanumeric() {
                    slug.extend(c.to_lowercase());
                } else if !slug.is_empty() && !slug.ends_with('-') {
                    slug.push('-');
                }
            }
            let slug = slug.trim_end_matches('-');
            format!("A-{}", if slug.is_empty() { "root" } else { slug })
        })
        .collect();
    let reserved: BTreeSet<_> = bases.iter().cloned().collect();
    let mut used = BTreeSet::new();
    bases
        .iter()
        .map(|base| {
            let mut id = base.clone();
            if bases.iter().filter(|other| *other == base).count() > 1 {
                let mut suffix = 1usize;
                loop {
                    id = format!("{base}-{suffix}");
                    if !reserved.contains(&id) && !used.contains(&id) {
                        break;
                    }
                    suffix += 1;
                }
            }
            used.insert(id.clone());
            id
        })
        .collect()
}

struct HierarchyNode {
    id: String,
    label: String,
    children: Vec<HierarchyNode>,
    module: Option<Value>,
    virtual_group: bool,
}

fn hierarchy_order(node: &HierarchyNode) -> &str {
    if node.module.is_some() {
        &node.id
    } else {
        node.children.iter().map(hierarchy_order).min().unwrap()
    }
}

fn group_node(
    label: String,
    children: Vec<HierarchyNode>,
    virtual_group: bool,
    diagnostics: &mut Vec<Value>,
) -> HierarchyNode {
    let identity = json!([
        &label,
        children.iter().map(|child| &child.id).collect::<Vec<_>>()
    ]);
    let id = format!(
        "G-{}",
        hex::encode(Sha256::digest(identity.to_string().as_bytes()))
    );
    if virtual_group {
        diagnostics.push(json!({"code":"virtual_module_group","group":id,"children":children.len(),"reason":"Wide directory fanout was evenly partitioned in stable slug order."}));
    }
    HierarchyNode {
        id,
        label,
        children,
        module: None,
        virtual_group,
    }
}

fn balance_groups(
    mut children: Vec<HierarchyNode>,
    diagnostics: &mut Vec<Value>,
) -> Vec<HierarchyNode> {
    children.sort_by(|a, b| hierarchy_order(a).cmp(hierarchy_order(b)));
    while children.len() > 9 {
        let buckets = children.len().div_ceil(9).max(3);
        let size = children.len() / buckets;
        let extra = children.len() % buckets;
        let mut source = children.into_iter();
        children = (0..buckets)
            .map(|index| {
                let children = source
                    .by_ref()
                    .take(size + usize::from(index < extra))
                    .collect();
                group_node(
                    format!("Modules {}", index + 1),
                    children,
                    true,
                    diagnostics,
                )
            })
            .collect();
    }
    children
}

fn semantic_children(
    entries: Vec<(Vec<String>, Value)>,
    depth: usize,
    prefix: &str,
    diagnostics: &mut Vec<Value>,
) -> Vec<HierarchyNode> {
    let mut children = Vec::new();
    let mut branches: BTreeMap<String, Vec<(Vec<String>, Value)>> = BTreeMap::new();
    for (path, module) in entries {
        if path.len() <= depth {
            children.push(HierarchyNode {
                id: module["id"].as_str().unwrap().into(),
                label: String::new(),
                children: Vec::new(),
                module: Some(module),
                virtual_group: false,
            });
        } else {
            branches
                .entry(path[depth].clone())
                .or_default()
                .push((path, module));
        }
    }
    for (part, entries) in branches {
        let label = if prefix.is_empty() {
            part
        } else {
            format!("{prefix}/{part}")
        };
        let nested = semantic_children(entries, depth + 1, &label, diagnostics);
        if nested.len() < 3 {
            children.extend(nested);
        } else {
            let nested = balance_groups(nested, diagnostics);
            children.push(group_node(label, nested, false, diagnostics));
        }
    }
    children
}

fn node_size(node: &HierarchyNode) -> (usize, usize) {
    if node.module.is_some() {
        (320, 180)
    } else {
        let (width, height) = grid_size(&node.children);
        (width + 48, height + 80)
    }
}

fn grid_size(children: &[HierarchyNode]) -> (usize, usize) {
    let sizes: Vec<_> = children.iter().map(node_size).collect();
    let mut width = 0;
    let mut height = 0;
    for row in sizes.chunks(3) {
        width = width
            .max(row.iter().map(|size| size.0).sum::<usize>() + row.len().saturating_sub(1) * 80);
        height += row.iter().map(|size| size.1).max().unwrap_or(0) + 80;
    }
    (width, height.saturating_sub(80))
}

fn place_children(
    children: &[HierarchyNode],
    x: usize,
    y: usize,
    nodes: &mut Vec<Value>,
    groups: &mut Vec<Value>,
) {
    let mut row_y = y;
    for row in children.chunks(3) {
        let mut column_x = x;
        let mut row_height = 0;
        for child in row {
            let (width, height) = node_size(child);
            if let Some(module) = &child.module {
                let mut module = module.clone();
                module["x"] = json!(column_x);
                module["y"] = json!(row_y);
                nodes.push(module);
            } else {
                nodes.push(json!({"id":child.id,"type":"group","label":child.label,"x":column_x,"y":row_y,"width":width,"height":height}));
                groups.push(json!({"id":child.id,"label":child.label,"children":child.children.iter().map(|child|&child.id).collect::<Vec<_>>(),"virtual":child.virtual_group}));
                place_children(&child.children, column_x + 24, row_y + 56, nodes, groups);
            }
            column_x += width + 80;
            row_height = row_height.max(height);
        }
        row_y += row_height + 80;
    }
}

fn build_hierarchy(
    modules: &[Value],
    leaves: Vec<Value>,
    diagnostics: &mut Vec<Value>,
) -> (Value, Vec<Value>, usize) {
    let entries = modules
        .iter()
        .zip(leaves)
        .map(|(module, leaf)| {
            let paths: Vec<Vec<_>> = module["roots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|root| {
                    root.as_str()
                        .unwrap()
                        .split('/')
                        .filter(|part| *part != ".")
                        .map(str::to_owned)
                        .collect()
                })
                .collect();
            let mut common = paths[0].clone();
            for path in &paths[1..] {
                let len = common.iter().zip(path).take_while(|(a, b)| a == b).count();
                common.truncate(len);
            }
            (common, leaf)
        })
        .collect();
    let mut children = semantic_children(entries, 0, "", diagnostics);
    while children.len() < 3 {
        let Some(index) = children.iter().position(|child| child.module.is_none()) else {
            break;
        };
        let node = children.remove(index);
        children.splice(index..index, node.children);
    }
    let children = balance_groups(children, diagnostics);
    let height = grid_size(&children).1;
    let root = children.iter().map(|child| &child.id).collect::<Vec<_>>();
    let mut nodes = Vec::new();
    let mut groups = Vec::new();
    place_children(&children, 0, 0, &mut nodes, &mut groups);
    (json!({"children":root,"groups":groups}), nodes, height)
}

fn read_git(catalog: &Catalog, args: &[&str]) -> Result<String, String> {
    repo::git_timeout(&catalog.root, args, Duration::from_secs(30))
}

fn language(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "rs" => Some("Rust"),
        "ts" | "tsx" | "mts" | "cts" => Some("TypeScript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "svelte" => Some("Svelte"),
        "py" | "pyi" => Some("Python"),
        "go" => Some("Go"),
        "c" | "h" => Some("C"),
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" => Some("C++"),
        "java" => Some("Java"),
        "kt" | "kts" => Some("Kotlin"),
        "swift" => Some("Swift"),
        "rb" => Some("Ruby"),
        "sh" | "bash" | "zsh" => Some("Shell"),
        _ => None,
    }
}

fn module_root(path: &str) -> String {
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() == 1 {
        ".".into()
    } else if parts.len() >= 3
        && matches!(parts[0], "src" | "crates" | "packages" | "apps" | "libs")
    {
        parts[..2].join("/")
    } else {
        parts[0].into()
    }
}

fn is_manifest(path: &str) -> bool {
    is_workflow(path)
        || matches!(
            path.rsplit('/').next().unwrap_or(path),
            "Cargo.toml" | "package.json" | "go.mod" | "pyproject.toml" | "Makefile" | "makefile"
        )
}

fn merge_commands(commands: Vec<Value>) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();
    for mut command in commands {
        let source = command["source"].clone();
        if command["argv"].is_array() {
            if let Some(existing) = result.iter_mut().find(|existing| {
                existing["cwd"] == command["cwd"] && existing["argv"] == command["argv"]
            }) {
                let sources = existing["sources"].as_array_mut().unwrap();
                if !sources.contains(&source) {
                    sources.push(source);
                    sources.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                }
                continue;
            }
        }
        command["sources"] = json!([source]);
        result.push(command);
    }
    result
}

fn commands(path: &str, text: &str, diagnostics: &mut Vec<Value>) -> Vec<Value> {
    if is_workflow(path) {
        return workflow_commands(path, text, diagnostics);
    }
    let cwd = path.rsplit_once('/').map_or(".", |(parent, _)| parent);
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut commands = Vec::new();
    match name {
        "Cargo.toml"
            if text
                .lines()
                .any(|line| matches!(line.trim(), "[package]" | "[workspace]")) =>
        {
            commands
                .push(json!({"source":path,"cwd":cwd,"argv":["cargo","test"],"status":"not_run"}));
        }
        "go.mod" => commands
            .push(json!({"source":path,"cwd":cwd,"argv":["go","test","./..."],"status":"not_run"})),
        "package.json" => match serde_json::from_str::<Value>(text) {
            Ok(package) => {
                if let Some(script) = package["scripts"]["test"].as_str() {
                    let manager = package["packageManager"]
                        .as_str()
                        .and_then(|manager| manager.split('@').next())
                        .filter(|manager| matches!(*manager, "pnpm" | "npm" | "yarn"));
                    commands.push(json!({"source":path,"cwd":cwd,"argv":manager.map(|manager|vec![manager,"run","test"]),"script":script,"status":"not_run","requires_confirmation":manager.is_none()}));
                }
            }
            Err(error) => {
                diagnostics.push(
                    json!({"source":path,"code":"manifest_invalid","reason":error.to_string()}),
                );
                commands.push(json!({"source":path,"cwd":cwd,"argv":null,"status":"unsupported"}));
            }
        },
        "Makefile" | "makefile" => {
            for target in ["test", "check"] {
                if text
                    .lines()
                    .any(|line| line.starts_with(&format!("{target}:")))
                {
                    commands.push(
                        json!({"source":path,"cwd":cwd,"argv":["make",target],"status":"not_run"}),
                    );
                }
            }
        }
        "pyproject.toml" => {
            diagnostics.push(json!({"source":path,"code":"test_discovery_unsupported","reason":"未解析 pyproject 测试配置，需要确认验证命令"}));
            commands.push(json!({"source":path,"cwd":cwd,"argv":null,"status":"unsupported"}));
        }
        _ => {}
    }
    if commands.is_empty() && matches!(name, "Cargo.toml" | "Makefile" | "makefile") {
        diagnostics.push(json!({"source":path,"code":"test_discovery_unsupported","reason":"未识别清单中的验证入口，需要确认命令；不能据此判定无测试"}));
        commands.push(json!({"source":path,"cwd":cwd,"argv":null,"status":"unsupported"}));
    }
    commands
}

fn is_workflow(path: &str) -> bool {
    path.starts_with(".github/workflows/") && (path.ends_with(".yaml") || path.ends_with(".yml"))
}

fn workflow_commands(path: &str, text: &str, diagnostics: &mut Vec<Value>) -> Vec<Value> {
    let workflow: serde_yaml::Value = match serde_yaml::from_str(text) {
        Ok(workflow) => workflow,
        Err(error) => {
            diagnostics
                .push(json!({"source":path,"code":"workflow_invalid","reason":error.to_string()}));
            return vec![json!({"source":path,"cwd":".","argv":null,"status":"unsupported"})];
        }
    };
    let mut commands = Vec::new();
    if let Some(jobs) = workflow["jobs"].as_mapping() {
        for job in jobs.values() {
            if let Some(steps) = job["steps"].as_sequence() {
                for step in steps {
                    let Some(script) = step["run"].as_str() else {
                        continue;
                    };
                    if !script
                        .split(|c: char| {
                            c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')')
                        })
                        .any(|word| matches!(word, "test" | "pytest" | "vitest" | "jest" | "check"))
                    {
                        continue;
                    }
                    let cwd = step["working-directory"]
                        .as_str()
                        .or_else(|| job["defaults"]["run"]["working-directory"].as_str())
                        .or_else(|| workflow["defaults"]["run"]["working-directory"].as_str())
                        .unwrap_or(".");
                    let safe = !cwd.starts_with('/')
                        && !cwd.contains(['\\', '$'])
                        && !cwd.split('/').any(|part| part == "..");
                    let normalized = cwd
                        .split('/')
                        .filter(|part| !matches!(*part, "" | "."))
                        .collect::<Vec<_>>()
                        .join("/");
                    let normalized = if safe && !normalized.is_empty() {
                        normalized.as_str()
                    } else {
                        "."
                    };
                    let words: Vec<_> = script.split_whitespace().collect();
                    let known = safe
                        && !script.trim().contains(['\n', '\r'])
                        && matches!(
                            words.as_slice(),
                            ["cargo", "test"]
                                | ["go", "test", "./..."]
                                | ["pnpm" | "npm" | "yarn", "run", "test"]
                                | ["make", "test" | "check"]
                        );
                    commands.push(json!({"source":path,"cwd":normalized,"source_cwd":cwd,"argv":known.then_some(words),"script":script,"status":if safe{"not_run"}else{"unsupported"},"requires_confirmation":!known}));
                }
            }
        }
    }
    commands
}
