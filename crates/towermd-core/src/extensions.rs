//! Extension manifests and what an installed extension may send (FR-120,
//! FR-123).
//!
//! An extension only declares. The core reads `towermd-plugin.yaml`, never
//! loads or runs extension code, and spawns nothing for it.
//!
//! An extension is installed and enabled when its manifest is on the
//! authoritative main at `.towermd/extensions/<id>/towermd-plugin.yaml`.
//! Everything under `.towermd/` is human-set, so installing, enabling,
//! removing or reconfiguring one only ever lands through a proposal.
//!
//! Identity: a sender calling itself `pkg:<id>[@<version>][/<part>]` speaks
//! for the extension `<id>`. Permissions, checked where a message enters
//! (never on replay):
//!
//! - `pkg:<id>` must name an installed extension whose manifest has no
//!   problem (a broken manifest fails closed);
//! - its signals may only use kinds its manifest declares, at no more than
//!   the declared `max_level`, and claim `deterministic` only where declared;
//! - a kind any installed extension declares belongs to it: nobody else may
//!   send it;
//! - `human:` senders and the `core.` namespace are already refused by the
//!   envelope and the reducer. The inbound verbs only append fast-layer
//!   messages, so no message can change a human-set object or `gates.yaml`.
//!
//! An extension's own signals can only block, never pass: gates pass only on
//! check evidence signed by the verifier (FR-122, FR-124).
use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use serde_yaml::{Mapping, Value as Yaml};

use crate::{
    catalog,
    tower_api::{self, Envelope, Level, Verb, CORE_NAMESPACE},
};

/// Where installed manifests live on the authoritative main.
pub const DIR: &str = ".towermd/extensions";
pub const MANIFEST: &str = "towermd-plugin.yaml";
/// `from.id` prefix that names an extension.
pub const SENDER: &str = "pkg:";
/// The six extension types (§5.4). Others are read, never refused.
pub const TYPES: [&str; 6] = ["template", "check", "connector", "skills", "pack", "kit"];
/// The highest level a kind may be sent at when its manifest says nothing.
pub const DEFAULT_LEVEL: Level = Level::Active;

/// One declared signal kind.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Emit {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Must come from the core's visual vocabulary; not checked until that
    /// vocabulary is defined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub deterministic: bool,
    pub max_level: Level,
}

/// A manifest read leniently: unknown keys are kept, and anything that
/// cannot be honoured becomes a problem instead of a refusal to read.
#[derive(Clone, Debug, Serialize)]
pub struct Manifest {
    /// The installed path on main.
    pub path: String,
    /// The directory name; the manifest's `id` must equal it.
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub emits: Vec<Emit>,
    /// Check commands; only the verifier may ever run them.
    pub commands: BTreeMap<String, String>,
    /// Why the extension cannot speak; any problem fails it closed.
    pub problems: Vec<String>,
    /// The whole manifest as written, unknown keys included.
    pub manifest: Value,
    #[serde(skip)]
    map: Mapping,
}

fn string(map: &Mapping, key: &str, problems: &mut Vec<String>) -> Option<String> {
    match map.get(key) {
        None | Some(Yaml::Null) => None,
        Some(Yaml::String(value)) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        Some(Yaml::Number(value)) => Some(value.to_string()),
        Some(_) => {
            problems.push(format!("{key} 须是非空字符串"));
            None
        }
    }
}

/// `[a-z0-9][a-z0-9_-]*`, at most 64 bytes.
pub fn valid_id(id: &str) -> bool {
    id.len() <= 64
        && id.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

fn level(value: &Yaml) -> Option<Level> {
    serde_yaml::from_value(value.clone()).ok()
}

fn emits(map: &Mapping, problems: &mut Vec<String>) -> Vec<Emit> {
    let entries = match map.get("emits") {
        None | Some(Yaml::Null) => return Vec::new(),
        Some(Yaml::Sequence(entries)) => entries,
        Some(_) => {
            problems.push("emits 须是列表".into());
            return Vec::new();
        }
    };
    let mut emits: Vec<Emit> = Vec::new();
    for entry in entries {
        let Some(entry) = entry.as_mapping() else {
            problems.push("emits 的每一项须是映射".into());
            continue;
        };
        let mut local = Vec::new();
        let Some(kind) = string(entry, "kind", &mut local) else {
            problems.push("emits 的每一项须有 kind".into());
            continue;
        };
        if let Err(error) = tower_api::signal_kind(&kind) {
            problems.push(error);
            continue;
        }
        if kind.starts_with(CORE_NAMESPACE) {
            problems.push(format!("{kind}：core. 命名空间只属于 Tower.md"));
            continue;
        }
        if emits.iter().any(|emit| emit.kind == kind) {
            problems.push(format!("{kind} 重复声明"));
            continue;
        }
        let deterministic = match entry.get("deterministic") {
            None | Some(Yaml::Null) => false,
            Some(Yaml::Bool(value)) => *value,
            Some(_) => {
                local.push("deterministic 须是 true 或 false".into());
                false
            }
        };
        let max_level = match entry.get("max_level") {
            None | Some(Yaml::Null) => DEFAULT_LEVEL,
            Some(value) => level(value).unwrap_or_else(|| {
                local.push("max_level 须是 log、passive、active 或 time_sensitive".into());
                Level::Log
            }),
        };
        let emit = Emit {
            label: string(entry, "label", &mut local),
            icon: string(entry, "icon", &mut local),
            unit: string(entry, "unit", &mut local),
            kind,
            deterministic,
            max_level,
        };
        problems.extend(local.into_iter().map(|problem| format!("{}：{problem}", emit.kind)));
        emits.push(emit);
    }
    emits
}

fn commands(map: &Mapping, problems: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut commands = BTreeMap::new();
    match map.get("commands") {
        None | Some(Yaml::Null) => {}
        Some(Yaml::Mapping(entries)) => {
            for (name, command) in entries {
                match (name.as_str(), command.as_str()) {
                    (Some(name), Some(command)) if !name.trim().is_empty() && !command.trim().is_empty() => {
                        commands.insert(name.trim().to_owned(), command.to_owned());
                    }
                    _ => problems.push("commands 须是名称到非空命令的映射".into()),
                }
            }
        }
        Some(_) => problems.push("commands 须是映射".into()),
    }
    commands
}

impl Manifest {
    /// `path` is where it is installed; its directory names the extension.
    pub fn parse(path: &str, text: Option<&str>) -> Self {
        let directory = path
            .strip_prefix(&format!("{DIR}/"))
            .and_then(|rest| rest.strip_suffix(&format!("/{MANIFEST}")))
            .unwrap_or_default()
            .to_owned();
        let mut manifest = Self {
            path: path.into(), id: directory, version: None, kind: None, emits: Vec::new(),
            commands: BTreeMap::new(), problems: Vec::new(), manifest: Value::Null, map: Mapping::new(),
        };
        let Some(text) = text else {
            manifest.problems.push("清单不是普通文件".into());
            return manifest;
        };
        manifest.map = match serde_yaml::from_str(text) {
            Ok(Yaml::Mapping(map)) => map,
            Ok(_) => {
                manifest.problems.push("清单顶层必须是映射".into());
                return manifest;
            }
            Err(error) => {
                manifest.problems.push(format!("清单不是合法 YAML：{error}"));
                return manifest;
            }
        };
        let map = &manifest.map;
        let mut problems = Vec::new();
        match string(map, "id", &mut problems) {
            Some(id) if id == manifest.id => {}
            Some(id) => problems.push(format!("id {id} 与安装目录 {} 不一致", manifest.id)),
            None => problems.push("缺少 id".into()),
        }
        if !valid_id(&manifest.id) {
            problems.push("扩展 id 只能含小写字母、数字、_ 和 -，且不超过 64 字节".into());
        }
        manifest.version = string(map, "version", &mut problems);
        if manifest.version.is_none() {
            problems.push("缺少 version".into());
        }
        manifest.kind = string(map, "type", &mut problems);
        if manifest.kind.is_none() {
            problems.push("缺少 type".into());
        }
        if !matches!(map.get("config"), None | Some(Yaml::Null | Yaml::Mapping(_))) {
            problems.push("config 须是映射".into());
        }
        manifest.emits = emits(map, &mut problems);
        manifest.commands = commands(map, &mut problems);
        manifest.manifest = serde_json::to_value(map).unwrap_or(Value::Null);
        manifest.problems.extend(problems);
        manifest
    }

    /// The manifest as written, unknown keys included.
    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(&self.map).expect("YAML 映射总能序列化")
    }

    pub fn emit(&self, kind: &str) -> Option<&Emit> {
        self.emits.iter().find(|emit| emit.kind == kind)
    }
}

/// The extension a `from.id` speaks for: `pkg:<id>[@<version>][/<part>]`.
/// `None` when it does not claim one; an error when it claims one badly.
pub fn sender(from: &str) -> Result<Option<&str>, String> {
    if !from.get(..SENDER.len()).is_some_and(|prefix| prefix.eq_ignore_ascii_case(SENDER)) {
        return Ok(None);
    }
    let rest = &from[SENDER.len()..];
    let id = rest.split(['@', '/']).next().unwrap_or_default();
    if !from.starts_with(SENDER) || !valid_id(id) {
        return Err(format!("扩展来源须写成 pkg:<id>[@<版本>][/<部件>]：{from}"));
    }
    Ok(Some(id))
}

/// The installed extensions, as read from one revision of main.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Registry {
    /// The main revision they were read from; `None` before main exists.
    pub revision: Option<String>,
    pub extensions: Vec<Manifest>,
}

impl Registry {
    /// `files` are every entry under [`DIR`]; only `<id>/towermd-plugin.yaml`
    /// counts, other files beside it are ignored.
    pub fn from_files(revision: Option<String>, files: Vec<(String, Option<String>)>) -> Self {
        let mut extensions: Vec<Manifest> = files
            .iter()
            .filter(|(path, _)| {
                path.strip_prefix(&format!("{DIR}/"))
                    .and_then(|rest| rest.strip_suffix(&format!("/{MANIFEST}")))
                    .is_some_and(|directory| !directory.is_empty() && !directory.contains('/'))
            })
            .map(|(path, text)| Manifest::parse(path, text.as_deref()))
            .collect();
        extensions.sort_by(|a, b| a.id.cmp(&b.id));
        // A kind two extensions both declare belongs to neither.
        let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for manifest in &extensions {
            for emit in &manifest.emits {
                owners.entry(emit.kind.clone()).or_default().push(manifest.id.clone());
            }
        }
        for manifest in &mut extensions {
            for emit in &manifest.emits {
                let owners = &owners[&emit.kind];
                if owners.len() > 1 {
                    manifest.problems.push(format!("{} 同时由 {} 声明", emit.kind, owners.join("、")));
                }
            }
        }
        Self { revision, extensions }
    }

    /// The extensions installed on the authoritative main of `dir`'s repository.
    pub fn installed(dir: &Path) -> Result<Self, String> {
        let Some((root, revision)) = catalog::authority_revision(dir)? else {
            return Ok(Self::default());
        };
        let files = catalog::files_at(&root, &revision, DIR)?;
        Ok(Self::from_files(Some(revision), files))
    }

    pub fn get(&self, id: &str) -> Option<&Manifest> {
        self.extensions.iter().find(|manifest| manifest.id == id)
    }

    /// The extension that declares `kind`.
    pub fn owner(&self, kind: &str) -> Option<&Manifest> {
        self.extensions.iter().find(|manifest| manifest.emit(kind).is_some())
    }

    /// FR-123: what an inbound message may claim, given who it says it is.
    pub fn permit(&self, envelope: &Envelope) -> Result<(), String> {
        let signal = match envelope.verb {
            Verb::Signal => Some(envelope.signal()?),
            _ => None,
        };
        let Some(id) = sender(&envelope.from.id)? else {
            if let Some((signal, owner)) = signal.as_ref().and_then(|signal| Some((signal, self.owner(&signal.kind)?))) {
                return Err(format!("信号种类 {} 属于扩展 {}，只能以 {SENDER}{} 发送", signal.kind, owner.id, owner.id));
            }
            return Ok(());
        };
        let manifest = self.get(id).ok_or_else(|| format!("扩展 {id} 未安装；安装扩展须经提案进入权威 main"))?;
        if !manifest.problems.is_empty() {
            return Err(format!("扩展 {id} 的清单有问题，拒绝其消息：{}", manifest.problems.join("；")));
        }
        let Some(signal) = signal else { return Ok(()) };
        let emit = manifest
            .emit(&signal.kind)
            .ok_or_else(|| format!("扩展 {id} 未声明信号种类 {}", signal.kind))?;
        if signal.level > emit.max_level {
            return Err(format!(
                "扩展 {id} 的 {} 最高只能是 {}",
                signal.kind,
                serde_json::to_value(emit.max_level).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
            ));
        }
        if signal.deterministic && !emit.deterministic {
            return Err(format!("扩展 {id} 未声明 {} 是确定性的", signal.kind));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CHECK: &str = "id: eng-clean\nversion: 1.2.0\ntype: check\nemits:\n  - kind: eng.crap\n    label: CRAP\n    icon: gauge\n    unit: 分\n    deterministic: true\n  - kind: eng.hint\n    max_level: passive\nconfig:\n  crap_max: { type: number, default: 12 }\ncommands:\n  crap: \"cargo run -p probe -- functions\"\nfuture: { kept: true }\n";

    fn path(id: &str) -> String {
        format!("{DIR}/{id}/{MANIFEST}")
    }

    fn registry(files: &[(&str, &str)]) -> Registry {
        Registry::from_files(Some("r".into()), files.iter().map(|(id, text)| (path(id), Some((*text).to_owned()))).collect())
    }

    fn envelope(from: &str, verb: &str, data: Value) -> Envelope {
        serde_json::from_value(json!({"v":1,"project":"p","from":{"id":from,"kind":"source"},"target":"p","verb":verb,
            "data":data,"at":"2026-10-23T00:00:00Z"})).unwrap()
    }

    fn signal(from: &str, kind: &str, level: &str, deterministic: bool) -> Envelope {
        envelope(from, "signal", json!({"kind":kind,"level":level,"title":"t","deterministic":deterministic}))
    }

    #[test]
    fn the_spec_example_parses_and_keeps_unknown_keys() {
        let manifest = Manifest::parse(&path("eng-clean"), Some(CHECK));
        assert!(manifest.problems.is_empty(), "{:?}", manifest.problems);
        assert_eq!((manifest.version.as_deref(), manifest.kind.as_deref()), (Some("1.2.0"), Some("check")));
        let crap = manifest.emit("eng.crap").unwrap();
        assert_eq!((crap.label.as_deref(), crap.icon.as_deref(), crap.unit.as_deref()), (Some("CRAP"), Some("gauge"), Some("分")));
        assert!(crap.deterministic);
        assert_eq!(crap.max_level, DEFAULT_LEVEL);
        assert_eq!(manifest.emit("eng.hint").unwrap().max_level, Level::Passive);
        assert!(!manifest.emit("eng.hint").unwrap().deterministic);
        assert_eq!(manifest.commands["crap"], "cargo run -p probe -- functions");
        assert_eq!(manifest.manifest["config"]["crap_max"]["default"], 12);
        assert_eq!(manifest.manifest["future"]["kept"], true);
        let again: Yaml = serde_yaml::from_str(&manifest.to_yaml()).unwrap();
        assert_eq!(again, serde_yaml::from_str::<Yaml>(CHECK).unwrap());
    }

    #[test]
    fn a_template_manifest_reads_without_emits_and_unknown_types_are_read() {
        let template = "id: kanban\nversion: 1.0.0\ntype: template\nlayout: { x: { category: state } }\nstates: [To Do, Done]\n";
        let manifest = Manifest::parse(&path("kanban"), Some(template));
        assert!(manifest.problems.is_empty(), "{:?}", manifest.problems);
        assert_eq!(manifest.manifest["states"][1], "Done");
        let other = Manifest::parse(&path("x"), Some("id: x\nversion: 1\ntype: someday\n"));
        assert!(other.problems.is_empty(), "{:?}", other.problems);
        assert!(!TYPES.contains(&other.kind.as_deref().unwrap()));
    }

    #[test]
    fn manifest_problems_are_recorded_not_fatal() {
        let cases = [
            ("a", "[1, 2]", "顶层"),
            ("a", "a: [", "YAML"),
            ("a", "id: b\nversion: 1\ntype: check\n", "安装目录"),
            ("a", "version: 1\ntype: check\n", "缺少 id"),
            ("Bad", "id: Bad\nversion: 1\ntype: check\n", "小写"),
            ("a", "id: a\ntype: check\n", "version"),
            ("a", "id: a\nversion: 1\n", "type"),
            ("a", "id: a\nversion: 1\ntype: check\nemits: eng.x\n", "列表"),
            ("a", "id: a\nversion: 1\ntype: check\nemits: [ { kind: crap } ]\n", "命名空间"),
            ("a", "id: a\nversion: 1\ntype: check\nemits: [ { kind: core.weakening } ]\n", "core."),
            ("a", "id: a\nversion: 1\ntype: check\nemits: [ { kind: e.x }, { kind: e.x } ]\n", "重复"),
            ("a", "id: a\nversion: 1\ntype: check\nemits: [ { kind: e.x, max_level: critical } ]\n", "max_level"),
            ("a", "id: a\nversion: 1\ntype: check\nemits: [ { kind: e.x, deterministic: yes } ]\n", "deterministic"),
            ("a", "id: a\nversion: 1\ntype: check\ncommands: [ x ]\n", "commands"),
            ("a", "id: a\nversion: 1\ntype: check\nconfig: 3\n", "config"),
        ];
        for (id, text, problem) in cases {
            let manifest = Manifest::parse(&path(id), Some(text));
            assert!(manifest.problems.iter().any(|p| p.contains(problem)), "{text}: {:?}", manifest.problems);
        }
        assert!(!Manifest::parse(&path("a"), None).problems.is_empty());
    }

    #[test]
    fn only_manifests_in_their_own_directory_count_and_shared_kinds_fail_closed() {
        let files = vec![
            (path("eng-clean"), Some(CHECK.to_owned())),
            (format!("{DIR}/eng-clean/README.md"), Some("x".into())),
            (format!("{DIR}/{MANIFEST}"), Some(CHECK.to_owned())),
            (format!("{DIR}/a/b/{MANIFEST}"), Some(CHECK.to_owned())),
        ];
        let only = Registry::from_files(None, files);
        assert_eq!(only.extensions.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["eng-clean"]);
        let twin = CHECK.replace("id: eng-clean", "id: twin");
        let both = registry(&[("eng-clean", CHECK), ("twin", &twin)]);
        assert!(both.extensions.iter().all(|m| m.problems.iter().any(|p| p.contains("同时由"))));
        assert!(both.permit(&signal("pkg:twin", "eng.crap", "passive", false)).is_err());
        assert!(both.permit(&signal("tool/1", "eng.crap", "passive", false)).is_err());
    }

    #[test]
    fn senders_name_an_extension_only_through_pkg() {
        assert_eq!(sender("pkg:eng-clean@1.2/crap"), Ok(Some("eng-clean")));
        assert_eq!(sender("pkg:eng-clean"), Ok(Some("eng-clean")));
        assert_eq!(sender("pkg:eng-clean/crap"), Ok(Some("eng-clean")));
        assert_eq!(sender("ci/1"), Ok(None));
        assert_eq!(sender("pk"), Ok(None));
        for bad in ["pkg:", "PKG:eng-clean", "pkg:Eng", "pkg:@1", "pkg:a b"] {
            assert!(sender(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_installed_extension_sends_only_what_it_declared() {
        let registry = registry(&[("eng-clean", CHECK)]);
        let allowed = [
            signal("pkg:eng-clean@1.2/crap", "eng.crap", "active", true),
            signal("pkg:eng-clean", "eng.crap", "log", false),
            signal("pkg:eng-clean", "eng.hint", "passive", false),
            envelope("pkg:eng-clean", "report", json!({"state":"working","summary":"s"})),
            signal("ci/1", "eng.other", "time_sensitive", true),
            envelope("ci/1", "report", json!({"state":"working","summary":"s"})),
        ];
        for message in allowed {
            assert_eq!(registry.permit(&message), Ok(()), "{:?}", message.from);
        }
        let refused = [
            (signal("pkg:eng-clean", "eng.other", "passive", false), "未声明信号种类"),
            (signal("pkg:eng-clean", "eng.crap", "time_sensitive", true), "最高只能是 active"),
            (signal("pkg:eng-clean", "eng.hint", "active", false), "最高只能是 passive"),
            (signal("pkg:eng-clean", "eng.hint", "passive", true), "确定性"),
            (signal("pkg:missing", "eng.crap", "passive", false), "未安装"),
            (envelope("pkg:missing", "report", json!({"state":"working","summary":"s"})), "未安装"),
            (signal("ci/1", "eng.crap", "active", true), "属于扩展 eng-clean"),
            (signal("pkg:other", "eng.crap", "active", true), "未安装"),
            (signal("PKG:eng-clean", "eng.crap", "active", true), "pkg:<id>"),
        ];
        for (message, reason) in refused {
            let error = registry.permit(&message).unwrap_err();
            assert!(error.contains(reason), "{error}");
        }
    }

    #[test]
    fn a_broken_manifest_silences_its_extension() {
        let registry = registry(&[("eng-clean", &CHECK.replace("version: 1.2.0\n", ""))]);
        let error = registry.permit(&signal("pkg:eng-clean", "eng.crap", "passive", false)).unwrap_err();
        assert!(error.contains("清单有问题"), "{error}");
        let error = registry.permit(&envelope("pkg:eng-clean", "report", json!({"state":"working","summary":"s"}))).unwrap_err();
        assert!(error.contains("清单有问题"), "{error}");
        // Its kinds stay reserved for it.
        assert!(registry.permit(&signal("ci/1", "eng.crap", "passive", false)).is_err());
    }

    #[test]
    fn nothing_installed_leaves_every_other_sender_as_before() {
        let registry = Registry::default();
        assert_eq!(registry.permit(&signal("ci/1", "eng.crap", "active", true)), Ok(()));
        assert!(registry.permit(&signal("pkg:eng-clean", "eng.crap", "active", true)).is_err());
    }
}
