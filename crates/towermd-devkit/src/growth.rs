//! Spec 9.2–9.3: growing and settled, and the growth signals. Tower.md
//! measures them on the approved modules at `main`, the latest tasks and the
//! run records, and raises a signal when one crosses its threshold. A signal
//! only points a person at a proposal to draft; it changes no structure and
//! approves nothing. Thresholds and the mode live in `config.yaml`.
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{json, Value};
use serde_yaml::Value as Yaml;
use towermd_core::{
    catalog::Catalog,
    path_guard::path_is_allowed,
    repo,
    tower_api::{clip, signal_eid, Level, Signal, TowerLog, DETAIL_CHARS, TITLE_CHARS, TOWER},
};

use crate::{
    cold_start,
    run_record::RunRecord,
    run_report::{repeated_errors, ErrorClass, RepeatThresholds},
    task_model::ModuleScope,
};

pub const MODULE_TOO_LARGE: &str = "growth.module_too_large";
pub const CROSS_MODULE: &str = "growth.cross_module";
pub const UNASSIGNED: &str = "growth.unassigned";
pub const UNALLOCATED_INTENT: &str = "growth.unallocated_intent";
pub const REPEATED_ERROR: &str = "growth.repeated_error";
pub const KINDS: [&str; 5] = [MODULE_TOO_LARGE, CROSS_MODULE, UNASSIGNED, UNALLOCATED_INTENT, REPEATED_ERROR];

/// Spec 9.2: while growing, files may sit in the unassigned area; once
/// settled, every file must belong to a module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Growing,
    Settled,
}

/// `signals:` thresholds (spec 9.3 defaults) and `growth:` mode. Missing or
/// malformed values keep the defaults. There is no default module count that
/// settles a project: spec 20 leaves it open, so only a person's
/// `growth.mode` or `growth.settle_at_modules` settles it.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mode: Option<Mode>,
    pub settle_at_modules: Option<usize>,
    pub module_lines: usize,
    pub module_files: usize,
    pub cross_window: usize,
    /// Percent of the window's tasks that declare `touches`.
    pub cross_percent: usize,
    pub unassigned_files: usize,
    pub repeated: RepeatThresholds,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: None, settle_at_modules: None, module_lines: 3000, module_files: 40, cross_window: 10,
            cross_percent: 30, unassigned_files: 10, repeated: RepeatThresholds::default(),
        }
    }
}

fn number(section: Option<&Yaml>, path: &[&str]) -> Option<usize> {
    let mut value = section?;
    for key in path {
        value = value.get(*key)?;
    }
    value.as_u64().and_then(|value| usize::try_from(value).ok())
}

impl Settings {
    pub fn from_config(signals: Option<&Yaml>, growth: Option<&Yaml>) -> Self {
        let defaults = Self::default();
        let positive = |path: &[&str], default: usize| number(signals, path).filter(|value| *value > 0).unwrap_or(default);
        let percent = signals.and_then(|signals| signals.get("cross_module")?.get("ratio")?.as_f64())
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0 && *ratio < 1.0)
            .map_or(defaults.cross_percent, |ratio| (ratio * 100.0).round() as usize);
        let mode = match growth.and_then(|growth| growth.get("mode")?.as_str()) {
            Some("growing") => Some(Mode::Growing),
            Some("settled") => Some(Mode::Settled),
            _ => None,
        };
        Self {
            mode,
            settle_at_modules: number(growth, &["settle_at_modules"]).filter(|value| *value > 0),
            module_lines: positive(&["module_size", "lines"], defaults.module_lines),
            module_files: positive(&["module_size", "files"], defaults.module_files),
            cross_window: positive(&["cross_module", "window"], defaults.cross_window),
            cross_percent: percent,
            unassigned_files: positive(&["unassigned", "files"], defaults.unassigned_files),
            repeated: RepeatThresholds::from_config(signals),
        }
    }
    /// A person's `growth.mode` wins; otherwise the project settles once it
    /// has `settle_at_modules` approved modules.
    pub fn mode(&self, modules: usize) -> Mode {
        self.mode.unwrap_or(match self.settle_at_modules {
            Some(at) if modules >= at => Mode::Settled,
            _ => Mode::Growing,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModuleSize {
    pub id: String,
    pub title: String,
    pub files: usize,
    /// Lines of the source files Tower.md could read (UTF-8, at most 1 MiB each).
    pub lines: usize,
}

/// What `main` looks like against the approved modules; derived from one
/// revision, so it can be cached by revision and rebuilt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Layout {
    pub revision: String,
    pub modules: Vec<ModuleSize>,
    pub unassigned: Vec<String>,
    /// Intents on `main` whose `allocated_to` is empty: `(id, title)`.
    pub unallocated: Vec<(String, String)>,
}

fn title(catalog: &Catalog, id: &str) -> String {
    catalog.objects.get(id).and_then(|object| object.get_str("title").or_else(|| object.get_str("name")))
        .filter(|title| !title.trim().is_empty()).unwrap_or(id).to_owned()
}

/// Source files at `catalog.revision`, each in the first approved module
/// whose paths allow it; the rest are unassigned, as in a scan.
pub fn layout(catalog: &Catalog, modules: &BTreeMap<String, ModuleScope>) -> Result<Layout, String> {
    let listing = repo::git(&catalog.root, &["ls-tree", "-r", "-z", "--full-tree", &catalog.revision])?;
    let mut objects = BTreeMap::new();
    for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
        let (metadata, path) = entry.split_once('\t').ok_or("无法解析主干文件清单")?;
        if (metadata.starts_with("100644 blob ") || metadata.starts_with("100755 blob "))
            && !cold_start::ignored(path, catalog.config.docs_dir()) && cold_start::source(path)
        {
            if let Some(oid) = metadata.split(' ').nth(2) {
                objects.insert(path.to_owned(), oid.to_owned());
            }
        }
    }
    let texts = cold_start::read_texts(catalog, &objects, |_| true)?;
    let mut sizes: Vec<ModuleSize> = modules.keys()
        .map(|id| ModuleSize { id: id.clone(), title: title(catalog, id), files: 0, lines: 0 }).collect();
    let mut unassigned = Vec::new();
    for path in objects.keys() {
        let mut owner = None;
        for (index, scope) in modules.values().enumerate() {
            if path_is_allowed(path, &scope.paths)? {
                owner = Some(index);
                break;
            }
        }
        match owner {
            Some(index) => {
                sizes[index].files += 1;
                sizes[index].lines += texts.get(path).and_then(Option::as_deref).map_or(0, |text| text.lines().count());
            }
            None => unassigned.push(path.clone()),
        }
    }
    let unallocated = catalog.objects.iter().filter(|(_, object)| object.doc_type() == Some("Intent"))
        .filter(|(_, object)| object.get("allocated_to").and_then(Yaml::as_sequence).is_none_or(Vec::is_empty))
        .map(|(id, _)| (id.clone(), title(catalog, id))).collect();
    Ok(Layout { revision: catalog.revision.clone(), modules: sizes, unassigned, unallocated })
}

/// One signal over its threshold now.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub target: String,
    pub signal: Signal,
}

fn finding(kind: &str, target: &str, title: String, detail: String, value: Value, threshold: Value) -> Finding {
    Finding {
        target: target.into(),
        signal: Signal {
            kind: kind.into(), level: Level::Active, title: clip(&title, TITLE_CHARS), detail: Some(clip(&detail, DETAIL_CHARS)),
            value: Some(value), threshold: Some(threshold), deterministic: true,
        },
    }
}

fn class(class: ErrorClass) -> &'static str {
    match class {
        ErrorClass::External => "外部错误",
        ErrorClass::Execution => "执行错误",
        ErrorClass::Design => "设计错误",
    }
}

/// The five spec 9.3 signals that are over their thresholds. `recent` is
/// whether each of the latest tasks declared `touches`, oldest first; the
/// cross-module share is only judged once a full window of tasks exists.
/// `now_ms` is Unix milliseconds.
pub fn findings(
    settings: &Settings, project: &str, layout: &Layout, recent: &[bool],
    records: &BTreeMap<String, RunRecord>, now_ms: u64,
) -> Vec<Finding> {
    let mut found = Vec::new();
    for module in &layout.modules {
        if module.lines > settings.module_lines || module.files > settings.module_files {
            let (value, threshold) = if module.lines > settings.module_lines {
                (json!(module.lines), json!(settings.module_lines))
            } else {
                (json!(module.files), json!(settings.module_files))
            };
            found.push(finding(MODULE_TOO_LARGE, &module.id, format!("模块过大：{}", module.title),
                format!("{} 个源码文件、{} 行；阈值 {} 个文件或 {} 行。可起草拆分模块的提案。",
                    module.files, module.lines, settings.module_files, settings.module_lines), value, threshold));
        }
    }
    let window = &recent[recent.len().saturating_sub(settings.cross_window)..];
    if window.len() >= settings.cross_window {
        let touching = window.iter().filter(|touches| **touches).count();
        let percent = touching * 100 / window.len();
        if touching * 100 > settings.cross_percent * window.len() {
            found.push(finding(CROSS_MODULE, project, format!("跨模块改动 {percent}%"),
                format!("最近 {} 个任务中 {touching} 个声明了 touches。可起草调整边界或新增接口的提案。", window.len()),
                json!(percent), json!(settings.cross_percent)));
        }
    }
    let limit = match settings.mode(layout.modules.len()) {
        Mode::Growing => settings.unassigned_files,
        Mode::Settled => 0,
    };
    if layout.unassigned.len() > limit {
        let settled = if limit == 0 { "定型期不允许未划分文件；" } else { "" };
        found.push(finding(UNASSIGNED, project, format!("未划分区 {} 个文件", layout.unassigned.len()),
            format!("{settled}例如 {}。可起草新增模块的提案。", layout.unassigned.iter().take(3).cloned().collect::<Vec<_>>().join("、")),
            json!(layout.unassigned.len()), json!(limit)));
    }
    for (id, title) in &layout.unallocated {
        found.push(finding(UNALLOCATED_INTENT, id, format!("需求未分配：{title}"),
            "这条意图的 allocated_to 为空。可起草新增模块或扩展职责的提案。".into(), json!(0), json!(1)));
    }
    let now = i64::try_from(now_ms).ok().and_then(chrono::DateTime::from_timestamp_millis).unwrap_or_default();
    let repeated = repeated_errors(records, now, settings.repeated);
    if let Some(worst) = repeated.iter().max_by_key(|repeated| repeated.count) {
        let classes = repeated.iter().map(|repeated| format!("{} {} 次", class(repeated.class), repeated.count)).collect::<Vec<_>>().join("、");
        found.push(finding(REPEATED_ERROR, project, format!("同类错误反复：{}", class(worst.class)),
            format!("{} 小时内 {classes}。可起草修改 L3 流程或检查规则的提案。", settings.repeated.window_hours),
            json!(worst.count), json!(settings.repeated.count)));
    }
    found
}

/// A signal to raise: its deterministic hub ID, target and body.
#[derive(Clone, Debug, PartialEq)]
pub struct Raise {
    pub eid: String,
    pub target: String,
    pub signal: Signal,
}

/// Only transitions are raised: a finding without an active Tower.md entry
/// becomes active, and an active growth entry no longer found drops to `log`,
/// which takes it off the attention list. A standing finding is not resent.
pub fn transitions(log: &TowerLog, found: Vec<Finding>) -> Vec<Raise> {
    let ours = |kind: &str, target: &str| log.signals.iter()
        .find(|entry| entry.from.id == TOWER && entry.data.kind == kind && entry.target == target);
    let eid = |kind: &str, target: &str| signal_eid(kind, &format!("{target}#{}", ours(kind, target).map_or(0, |entry| entry.count) + 1));
    let live: BTreeSet<(String, String)> = found.iter().map(|finding| (finding.signal.kind.clone(), finding.target.clone())).collect();
    let mut raises: Vec<Raise> = found.into_iter()
        .filter(|finding| ours(&finding.signal.kind, &finding.target).is_none_or(|entry| entry.data.level < Level::Active))
        .map(|finding| Raise { eid: eid(&finding.signal.kind, &finding.target), target: finding.target, signal: finding.signal })
        .collect();
    for entry in log.signals.iter().filter(|entry| entry.from.id == TOWER && KINDS.contains(&entry.data.kind.as_str())) {
        if entry.data.level >= Level::Active && !live.contains(&(entry.data.kind.clone(), entry.target.clone())) {
            let signal = Signal { level: Level::Log, detail: Some("已回到阈值以内。".into()), value: None, threshold: None, ..entry.data.clone() };
            raises.push(Raise { eid: eid(&entry.data.kind, &entry.target), target: entry.target.clone(), signal });
        }
    }
    raises
}

#[cfg(test)]
#[path = "growth_tests.rs"]
mod tests;
