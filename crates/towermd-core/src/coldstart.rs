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

pub fn prepare_coldstart(catalog: &Catalog, options: &ScanOptions) -> Result<Value, String> {
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
    let shallow = read_git(catalog, &["rev-parse", "--is-shallow-repository"])? == "true";
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
            .map(|path| module_root(path))
            .filter(|root| groups.contains_key(root))
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
    for (index, roots) in partitions.iter().enumerate() {
        let key = roots.join("\0");
        let id = format!("A-{}", hex::encode(Sha256::digest(key.as_bytes())));
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
        let applicable: Vec<_> = manifests
            .iter()
            .filter(|command| {
                let cwd = command["cwd"].as_str().unwrap_or(".");
                cwd == "."
                    || members
                        .iter()
                        .any(|path| path.starts_with(&format!("{cwd}/")))
            })
            .cloned()
            .collect();
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
        validations.push(json!({"module":id,"tree":tree,"status":status,"commands":applicable}));
        if applicable.is_empty() {
            tasks.push(json!({"status":"candidate","module":id,"title":"补充可重复验证的测试命令","reason":"扫描未发现测试；不是测试通过证据"}));
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
        nodes.push(json!({"id":id,"type":"file","file":object_path,"x":(index%3)*400,"y":(index/3)*260,"width":320,"height":180,"towermd":{"obj":id}}));
    }
    if !unassigned.is_empty() {
        nodes.push(json!({"id":"unassigned","type":"group","label":"未划分区","x":0,"y":(modules.len().div_ceil(3)+1)*260,"width":1120,"height":180,"towermd":{"region":"unassigned"}}));
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
        "algorithm":"directory-history-v1","options":options,"stages":{"S1":"prepared","S2":"directory_fallback","S3":"prepared","S4":if shallow {"unavailable"} else {"prepared"},"S5":"not_run"},
        "files":files,"modules":modules,"unassigned":unassigned,"coverage":{"source_files":sources.len(),"assigned":assigned,"ratio":ratio,"status":coverage_status},
        "history":{"status":if shallow {"unavailable_shallow"} else {"available"},"commits":commits.lines().count(),"shallow":shallow,"coupling":coupling,"hotspots":hotspots,"skipped":skipped,"merges":merges},
        "validation_plans":validations,"candidate_tasks":tasks,"candidate_canvas":{"nodes":nodes,"edges":[]},"diagnostics":diagnostics,
    }))
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
                        .split_whitespace()
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
                    commands.push(json!({"source":path,"cwd":normalized,"source_cwd":cwd,"argv":null,"script":script,"status":if safe{"not_run"}else{"unsupported"},"requires_confirmation":true}));
                }
            }
        }
    }
    commands
}
