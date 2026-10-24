//! J4: re-enter the current approved baseline as a draft. The approved objects
//! seed a scan of the current commit, the draft keeps their identities and
//! layout, and a comparison lists what the reviewed draft changes. Nothing here
//! approves anything: publication still goes through a proposal the person
//! approves in the App.
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::baseline::{BaselineDraft, DraftObject, Submission};
use crate::cold_start::ScanReport;
use crate::review::ReviewedCommand;
use crate::task_model::VerifiedBaseline;
use towermd_core::catalog::Catalog;
use towermd_core::okf::Document;
use towermd_core::repo;

const KINDS: [&str; 4] = ["Intent", "Module", "Interface", "Acceptance"];
/// Approval and verification facts belong to the approved copy only; the draft
/// is unapproved again until a person approves the next baseline.
const APPROVAL_KEYS: &[&str] = &[
    "verified",
    "approved_by",
    "approved_at",
    "baseline",
    "git_tag",
    "generated",
    "review_verification_pending",
    "suggested_task",
    "initial_evidence",
    "verification",
    "verification_commands",
    "source_revision",
];
/// Keys whose change is reported on its own, or that are regenerated, and so
/// do not count as a content change.
const STRUCTURE_KEYS: &[&str] = &[
    "status", "title", "name", "paths", "verify", "from", "to", "module", "confidence",
    "rationale", "sources",
];

/// The approved baseline a scan was seeded from. Stored in the scan report, so
/// a draft built from it can be compared without reading the baseline again.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Origin {
    /// Baseline tag, e.g. `towermd/B0`.
    pub baseline: String,
    pub commit: String,
    pub modules: Vec<OriginModule>,
    pub objects: Vec<OriginObject>,
    /// Canvas node id → `[x, y, width, height]` in the approved `tower.canvas`.
    pub positions: BTreeMap<String, [i64; 4]>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OriginModule {
    pub id: String,
    pub title: String,
    pub paths: Vec<String>,
    pub commands: Vec<ReviewedCommand>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OriginObject {
    pub id: String,
    pub object_type: String,
    /// Relative to the docs directory, e.g. `arch/A-scan-1.md`.
    pub path: String,
    /// The approved original text, byte for byte.
    pub text: String,
}

fn title(document: &Document, id: &str) -> String {
    document
        .get_str("title")
        .or_else(|| document.get_str("name"))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(id)
        .to_owned()
}
fn strings(document: &Document, key: &str) -> Vec<String> {
    document
        .get(key)
        .and_then(Value::as_sequence)
        .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}
/// Both the reviewed `{directory, command}` and the scan's longer command
/// records are accepted; only directory and command matter.
fn commands(document: &Document) -> Vec<ReviewedCommand> {
    document
        .get("verification_commands")
        .and_then(Value::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(ReviewedCommand {
                        directory: item.get("directory")?.as_str()?.to_owned(),
                        command: item.get("command")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
fn without(document: &Document, keys: &[&str]) -> Result<Document, String> {
    let mut front = document.front().clone();
    for key in keys {
        front.remove(Value::from(*key));
    }
    let header = serde_yaml::to_string(&front).map_err(|e| e.to_string())?;
    Document::parse(&format!("---\n{header}---\n{}", document.body()))
}

/// Read the approved objects and layout of `baseline` from its own commit.
pub fn origin(catalog: &Catalog, baseline: &VerifiedBaseline) -> Result<Origin, String> {
    let docs = catalog.config.docs_dir();
    let mut modules = Vec::new();
    let mut objects = Vec::new();
    for (id, path, text, document) in crate::proposal_model::approved_objects(catalog, baseline)? {
        let Some(kind) = document.doc_type().filter(|kind| KINDS.contains(kind)) else {
            continue;
        };
        let relative = path
            .strip_prefix(&format!("{docs}/"))
            .ok_or("批准对象不在文档目录内")?;
        if kind == "Module" {
            let scope = baseline.modules().get(&id).ok_or("批准模块缺少路径上限")?;
            modules.push(OriginModule {
                id: id.clone(),
                title: title(&document, &id),
                paths: scope.paths.clone(),
                commands: commands(&document),
            });
        }
        objects.push(OriginObject {
            id,
            object_type: kind.into(),
            path: relative.into(),
            text,
        });
    }
    if modules.is_empty() {
        return Err("批准基线没有模块，无法从基线进入草稿".into());
    }
    modules.sort_by(|a, b| a.id.cmp(&b.id));
    objects.sort_by(|a, b| a.id.cmp(&b.id));
    // Layout is a convenience: an unreadable canvas only loses positions.
    let mut positions = BTreeMap::new();
    if let Ok(canvas) = repo::git_timeout(
        &catalog.root,
        &["show", &format!("{}:{docs}/tower.canvas", baseline.commit())],
        Duration::from_secs(30),
    ) {
        let value: serde_json::Value = serde_json::from_str(&canvas).unwrap_or_default();
        for node in value["nodes"].as_array().into_iter().flatten() {
            let number = |key: &str| node[key].as_i64();
            if let (Some(id), Some(x), Some(y), Some(w), Some(h)) = (
                node["id"].as_str(),
                number("x"),
                number("y"),
                number("width"),
                number("height"),
            ) {
                positions.insert(id.to_owned(), [x, y, w, h]);
            }
        }
    }
    Ok(Origin {
        baseline: baseline.tag().into(),
        commit: baseline.commit().into(),
        modules,
        objects,
        positions,
    })
}

/// The S6 submission for a seeded scan: the approved text of every object,
/// made a draft again, with module paths, verify commands, confidence and
/// rationale from the current scan. Objects that can no longer be carried
/// over are left out and named in `risks`.
pub(crate) fn submission(scan: &ScanReport, origin: &Origin) -> Result<Submission, String> {
    let mut result = Submission {
        scan_id: scan.id.clone(),
        project_id: scan.project_id.clone(),
        source_revision: scan.revision.clone(),
        intents: vec![],
        modules: vec![],
        interfaces: vec![],
        acceptances: vec![],
        questions: vec![],
        risks: vec![],
    };
    let approved = |id: &str, kind: &str| {
        origin
            .objects
            .iter()
            .find(|object| object.id == id && object.object_type == kind)
    };
    for module in &scan.modules {
        let node = scan
            .view
            .nodes
            .iter()
            .find(|n| n.object_id.as_deref() == Some(&module.id))
            .ok_or("扫描缺少模块对象")?;
        let current = Document::parse(&node.content)?;
        let (mut document, path) = match approved(&module.id, "Module") {
            Some(object) => (
                without(&Document::parse(&object.text)?, APPROVAL_KEYS)?,
                object.path.clone(),
            ),
            None => (current.clone(), node.file.clone().ok_or("扫描模块缺少路径")?),
        };
        for key in ["paths", "verify"] {
            document.set(key, current.get(key).cloned().ok_or("扫描模块缺少路径或验证命令")?)?;
        }
        document.set("status", "draft".into())?;
        document.set("rationale", module.rationale.clone().into())?;
        document.set(
            "confidence",
            serde_yaml::to_value(module.confidence).map_err(|e| e.to_string())?,
        )?;
        result.modules.push(DraftObject { id: module.id.clone(), path, text: document.to_text() });
    }
    let modules: BTreeSet<&str> = scan.modules.iter().map(|m| m.id.as_str()).collect();
    for object in &origin.objects {
        if object.object_type == "Module" {
            continue;
        }
        let mut document = without(&Document::parse(&object.text)?, APPROVAL_KEYS)?;
        document.set("status", "draft".into())?;
        let references: Vec<String> = match object.object_type.as_str() {
            "Interface" => ["from", "to"].iter().filter_map(|k| document.get_str(k).map(str::to_owned)).collect(),
            "Acceptance" => document.get_str("module").map(str::to_owned).into_iter().collect(),
            _ => strings(&document, "allocated_to"),
        };
        if let Some(missing) = references.iter().find(|id| !modules.contains(id.as_str())) {
            result.risks.push(format!("{} 引用的模块 {missing} 已不在草稿中，未带入；发布后它会被移除。", object.id));
            continue;
        }
        if object.object_type == "Acceptance" && document.get("verify").is_some() {
            let module = references.first().map(String::as_str).unwrap_or_default();
            let available: BTreeSet<&str> = scan
                .commands
                .iter()
                .filter(|c| c.modules.iter().any(|m| m == module))
                .map(|c| c.command.as_str())
                .collect();
            let verify = strings(&document, "verify");
            let kept: Vec<&String> = verify.iter().filter(|c| available.contains(c.as_str())).collect();
            if kept.len() != verify.len() {
                result.risks.push(format!(
                    "{} 的验收命令不再属于模块 {module} 的验证命令，已从草稿中去掉；请在审阅中确认。",
                    object.id
                ));
                document = if kept.is_empty() {
                    without(&document, &["verify"])?
                } else {
                    document.set("verify", serde_yaml::to_value(kept).map_err(|e| e.to_string())?)?;
                    document
                };
            }
        }
        let draft = DraftObject { id: object.id.clone(), path: object.path.clone(), text: document.to_text() };
        match object.object_type.as_str() {
            "Intent" => result.intents.push(draft),
            "Interface" => result.interfaces.push(draft),
            _ => result.acceptances.push(draft),
        }
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Added,
    Removed,
    Renamed,
    Moved,
    /// Module paths changed.
    Files,
    /// Interface ends or the acceptance's module changed.
    Endpoints,
    /// Module or acceptance verification commands changed.
    Commands,
    /// Other fields or the body changed.
    Content,
    /// A removed module whose files all went to one module.
    MergedInto,
    /// A new module whose files came mostly from a module that remains.
    SplitFrom,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ObjectDiff {
    pub id: String,
    pub object_type: String,
    pub title: String,
    pub kinds: Vec<DiffKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_added: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_removed: Vec<String>,
    /// The module merged into, or split from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Comparison {
    pub draft_id: String,
    pub baseline: String,
    pub commit: String,
    /// Changed objects only, sorted by ID.
    pub objects: Vec<ObjectDiff>,
    pub unchanged: Vec<String>,
    /// Source files that belong to no module in the draft (FR-21).
    pub unassigned: Vec<String>,
}

struct Side {
    object_type: String,
    document: Document,
    position: Option<[i64; 2]>,
}
fn pairs(document: &Document) -> BTreeSet<(String, String)> {
    commands(document).into_iter().map(|c| (c.directory, c.command)).collect()
}
fn rest(document: &Document) -> Result<(Value, String), String> {
    let mut keys: Vec<&str> = APPROVAL_KEYS.to_vec();
    keys.extend_from_slice(STRUCTURE_KEYS);
    let stripped = without(document, &keys)?;
    Ok((Value::Mapping(stripped.front().clone()), stripped.body().trim().to_owned()))
}

/// What a draft entered from a baseline changes against that baseline. Pure:
/// it reads only the draft, which carries its origin.
pub fn compare(draft: &BaselineDraft) -> Result<Comparison, String> {
    let origin = draft
        .scan
        .origin
        .as_ref()
        .ok_or("该草稿不是从已批准基线进入的，没有可对比的基线")?;
    let mut before = BTreeMap::new();
    for object in &origin.objects {
        before.insert(
            object.id.clone(),
            Side {
                object_type: object.object_type.clone(),
                document: Document::parse(&object.text)?,
                position: origin.positions.get(&object.id).map(|p| [p[0], p[1]]),
            },
        );
    }
    let mut after = BTreeMap::new();
    for node in &draft.view.nodes {
        if let (Some(id), Some(kind)) = (&node.object_id, &node.object_type) {
            if KINDS.contains(&kind.as_str()) {
                after.insert(
                    id.clone(),
                    Side {
                        object_type: kind.clone(),
                        document: Document::parse(&node.content)?,
                        position: Some([node.x, node.y]),
                    },
                );
            }
        }
    }
    let paths = |sides: &BTreeMap<String, Side>| -> BTreeMap<String, BTreeSet<String>> {
        sides
            .iter()
            .filter(|(_, side)| side.object_type == "Module")
            .map(|(id, side)| (id.clone(), strings(&side.document, "paths").into_iter().collect()))
            .collect()
    };
    let (old_paths, new_paths) = (paths(&before), paths(&after));
    let ids: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    let mut objects = Vec::new();
    let mut unchanged = Vec::new();
    for id in ids {
        let (old, new) = (before.get(id), after.get(id));
        let side = new.or(old).unwrap();
        let mut diff = ObjectDiff {
            id: id.clone(),
            object_type: side.object_type.clone(),
            title: title(&side.document, id),
            kinds: vec![],
            before_title: None,
            files_added: vec![],
            files_removed: vec![],
            related: None,
        };
        let empty = BTreeSet::new();
        let old_files = old_paths.get(id).unwrap_or(&empty);
        let new_files = new_paths.get(id).unwrap_or(&empty);
        diff.files_added = new_files.difference(old_files).cloned().collect();
        diff.files_removed = old_files.difference(new_files).cloned().collect();
        match (old, new) {
            (None, Some(_)) => {
                diff.kinds.push(DiffKind::Added);
                // The remaining approved module that gave this one most files.
                let source = old_paths
                    .iter()
                    .filter(|(other, _)| new_paths.contains_key(*other))
                    .map(|(other, files)| (files.intersection(new_files).count(), other))
                    .filter(|(count, _)| *count > 0)
                    .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(a.1)));
                if let Some((_, other)) = source {
                    diff.kinds.push(DiffKind::SplitFrom);
                    diff.related = Some(other.clone());
                }
            }
            (Some(_), None) => {
                diff.kinds.push(DiffKind::Removed);
                let targets: BTreeSet<&String> = new_paths
                    .iter()
                    .filter(|(_, files)| !files.is_disjoint(old_files))
                    .map(|(other, _)| other)
                    .collect();
                let covered = old_files.iter().all(|file| new_paths.values().any(|files| files.contains(file)));
                if targets.len() == 1 && covered {
                    diff.kinds.push(DiffKind::MergedInto);
                    diff.related = targets.into_iter().next().cloned();
                }
            }
            (Some(old), Some(new)) => {
                let before_title = title(&old.document, id);
                if before_title != diff.title {
                    diff.kinds.push(DiffKind::Renamed);
                    diff.before_title = Some(before_title);
                }
                if old.position.is_some() && old.position != new.position {
                    diff.kinds.push(DiffKind::Moved);
                }
                if !diff.files_added.is_empty() || !diff.files_removed.is_empty() {
                    diff.kinds.push(DiffKind::Files);
                }
                if ["from", "to", "module"].iter().any(|k| old.document.get_str(k) != new.document.get_str(k)) {
                    diff.kinds.push(DiffKind::Endpoints);
                }
                if pairs(&old.document) != pairs(&new.document)
                    || strings(&old.document, "verify").into_iter().collect::<BTreeSet<_>>()
                        != strings(&new.document, "verify").into_iter().collect::<BTreeSet<_>>()
                {
                    diff.kinds.push(DiffKind::Commands);
                }
                if rest(&old.document)? != rest(&new.document)? {
                    diff.kinds.push(DiffKind::Content);
                }
            }
            (None, None) => unreachable!(),
        }
        if diff.kinds.is_empty() {
            unchanged.push(id.clone());
        } else {
            objects.push(diff);
        }
    }
    Ok(Comparison {
        draft_id: draft.id.clone(),
        baseline: origin.baseline.clone(),
        commit: origin.commit.clone(),
        objects,
        unchanged,
        unassigned: draft.scan.unassigned.clone(),
    })
}
