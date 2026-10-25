//! Gates and the completion judgement (FR-122, FR-124, FR-56, FR-33).
//!
//! A person declares gates in `.towermd/gates.yaml`; each names a
//! deterministic signal kind. The judgement is a pure function of the
//! declaration, the object's check evidence and the merged signals:
//!
//! - a gate passes only on trusted check evidence (signed by the verifier)
//!   bound to the object's current version;
//! - any sender's deterministic signal at `active` or above fails it, since
//!   blocking is the safe direction and signals are self-reported;
//! - an unreadable file or entry fails closed, never open (FR-123).
//!
//! A person's waiver (FR-72, `waivers`) lets one declared hard gate through
//! on one object until it expires; the kit's own checks, broken declarations
//! and a broken file are never waived.
//!
//! An object is complete when every hard gate with a trusted producer has
//! passed or is waived; a person may complete it in the App only when it has no such gate
//! at all. A delegate's `claimed_done` never completes anything.
//!
//! Gate and check results are written as `evidence.recorded` events in their
//! own lazily created group; written once, corrected only by new evidence.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serde_yaml::{Mapping, Value as Yaml};

use crate::{
    delegation::{Evidence, KitDelegation, SourceKind},
    delegation_projection::Mark,
    hub::Event,
    kit::Kit,
    project_state::insert_unique,
    tower_api::{self, Level, Merged, Signal},
    waivers::Waiver,
};

pub const GATES_PATH: &str = ".towermd/gates.yaml";
pub const GROUP: &str = "GateEvidence";
pub const RECORDED: &str = "evidence.recorded";
pub const LAYER: &str = "L5";
/// Signs gate evidence: the daemon evaluates gates where it enforces them.
pub const TOWER: &str = "process:towermd";
/// Signs check evidence: only the verifier runs check programs.
pub const VERIFIER: &str = "process:towermd-verifier";

/// One declared gate, typed so two versions of the file can be compared
/// entry by entry (FR-125).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    pub name: String,
    /// The signal kind it watches, e.g. `eng.crap`.
    pub check: Option<String>,
    /// Soft gates only warn. Defaults to hard.
    pub hard: bool,
    /// Why the entry cannot be honoured; such a gate fails closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl Gate {
    fn parse(name: String, value: &Yaml) -> Self {
        let mut gate = Self { name, check: None, hard: true, problem: None };
        let entry = match value {
            Yaml::String(check) => {
                gate.check = Some(check.trim().to_owned());
                None
            }
            Yaml::Mapping(entry) => Some(entry),
            _ => {
                gate.problem = Some("门禁须写成映射或信号种类".into());
                return gate;
            }
        };
        if let Some(entry) = entry {
            match entry.get("hard") {
                None | Some(Yaml::Null) => {}
                Some(Yaml::Bool(hard)) => gate.hard = *hard,
                Some(_) => gate.problem = Some("hard 须是 true 或 false".into()),
            }
            match entry.get("check") {
                Some(Yaml::String(check)) => gate.check = Some(check.trim().to_owned()),
                _ => gate.problem = gate.problem.take().or(Some("缺少 check（信号种类）".into())),
            }
            if gate.hard && entry.get("deterministic") == Some(&Yaml::Bool(false)) {
                gate.problem = gate.problem.take().or(Some("非确定性信号不能做硬门禁，只能做软门禁".into()));
            }
        }
        if let Some(check) = &gate.check {
            if let Err(error) = tower_api::signal_kind(check) {
                gate.problem = gate.problem.take().or(Some(error));
            }
        }
        gate
    }
}

/// `.towermd/gates.yaml`, read leniently: unknown keys stay in the mapping
/// and round-trip unchanged; a missing file declares no gates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GateFile {
    map: Mapping,
    pub gates: BTreeMap<String, Gate>,
    /// The file as a whole is unusable: every completion is blocked.
    pub error: Option<String>,
    /// Waivers in force when the judgement is made. Never read from the
    /// file: the caller attaches them from the hub (`waive`).
    pub waivers: Vec<Waiver>,
    /// Paths a delegate may never commit, on top of the built-in sensitive
    /// files (`sensitive_paths`, path syntax of `allowed_paths`). Only adds.
    pub sensitive_paths: Vec<String>,
    /// Text that makes a tool request forbidden, on top of the built-in
    /// dangerous commands (`dangerous_commands`, matched case-insensitively
    /// as a substring). Only adds.
    pub dangerous_commands: Vec<String>,
    /// FR-55: per-facet tightenings (`overrides`), merged by `scoped`.
    pub overrides: Vec<Override>,
}

/// One `overrides` entry (FR-55): its `match` and the gate entries it adds
/// or tightens for the objects it matches.
#[derive(Clone, Debug, PartialEq)]
pub struct Override {
    /// Facet name to accepted values, e.g. `risk: [high]`.
    pub when: BTreeMap<String, Vec<String>>,
    entries: Vec<(String, Yaml)>,
}

impl Override {
    /// Every facet the object reports must be among the accepted values; a
    /// facet it does not report cannot rule the override out (the stricter
    /// reading).
    fn applies(&self, facets: &BTreeMap<String, Vec<String>>) -> bool {
        self.when.iter().all(|(facet, accepted)| match facets.get(facet) {
            Some(values) => values.iter().any(|value| accepted.iter().any(|one| one.eq_ignore_ascii_case(value))),
            None => true,
        })
    }

    /// `risk=high` style, for messages.
    pub fn describe(&self) -> String {
        self.when.iter().map(|(facet, values)| format!("{facet}={}", values.join("|"))).collect::<Vec<_>>().join("，")
    }
}

fn scalar(value: &Yaml) -> Option<String> {
    let text = match value {
        Yaml::String(text) => text.trim().to_owned(),
        Yaml::Number(number) => number.to_string(),
        Yaml::Bool(flag) => flag.to_string(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// `overrides`: a list of mappings, each with a non-empty `match` and its
/// gate entries. Anything else is unreadable.
fn overrides(map: &Mapping) -> Result<Vec<Override>, String> {
    let bad = || format!("{GATES_PATH} 的 overrides 须是列表，每项是带非空 match 映射的门禁映射");
    let items = match map.get("overrides") {
        None | Some(Yaml::Null) => return Ok(Vec::new()),
        Some(Yaml::Sequence(items)) => items,
        Some(_) => return Err(bad()),
    };
    items.iter().map(|item| {
        let mut rule = Override { when: BTreeMap::new(), entries: Vec::new() };
        for (key, value) in item.as_mapping().ok_or_else(bad)? {
            let key = key.as_str().map(str::trim).filter(|key| !key.is_empty()).ok_or_else(bad)?;
            if key != "match" {
                rule.entries.push((key.to_owned(), value.clone()));
                continue;
            }
            for (facet, accepted) in value.as_mapping().ok_or_else(bad)? {
                let facet = facet.as_str().map(str::trim).filter(|facet| !facet.is_empty()).ok_or_else(bad)?;
                let values = match accepted {
                    Yaml::Sequence(values) => values.iter().map(scalar).collect::<Option<Vec<_>>>(),
                    other => scalar(other).map(|value| vec![value]),
                }.filter(|values| !values.is_empty()).ok_or_else(bad)?;
                rule.when.insert(facet.to_owned(), values);
            }
        }
        if rule.when.is_empty() { return Err(bad()); }
        Ok(rule)
    }).collect()
}

/// A list of non-empty strings, or why not.
fn strings(map: &Mapping, key: &str) -> Result<Vec<String>, String> {
    match map.get(key) {
        None | Some(Yaml::Null) => Ok(Vec::new()),
        Some(Yaml::Sequence(items)) => items.iter().map(|item| item.as_str().map(str::trim).filter(|text| !text.is_empty())
            .map(str::to_owned).ok_or_else(|| format!("{GATES_PATH} 的 {key} 须是非空字符串列表"))).collect(),
        Some(_) => Err(format!("{GATES_PATH} 的 {key} 须是列表")),
    }
}

impl GateFile {
    pub fn parse(text: &str) -> Self {
        let broken = |error: String| Self { error: Some(error), ..Self::default() };
        let map = match serde_yaml::from_str(text) {
            Ok(Yaml::Mapping(map)) => map,
            Ok(Yaml::Null) => Mapping::new(),
            Ok(_) => return broken(format!("{GATES_PATH} 顶层必须是映射")),
            Err(error) => return broken(format!("{GATES_PATH} 不是合法 YAML：{error}")),
        };
        let mut file = Self { map, ..Self::default() };
        match file.map.get("gates") {
            None | Some(Yaml::Null) => {}
            Some(Yaml::Mapping(entries)) => {
                for (key, value) in entries {
                    let gate = match key.as_str().map(str::trim).filter(|name| !name.is_empty()) {
                        Some(name) => Gate::parse(name.to_owned(), value),
                        None => Gate {
                            name: serde_yaml::to_string(key).unwrap_or_default().trim().to_owned(),
                            check: None, hard: true, problem: Some("门禁名须是非空字符串".into()),
                        },
                    };
                    file.gates.insert(gate.name.clone(), gate);
                }
            }
            Some(_) => file.error = Some(format!("{GATES_PATH} 的 gates 必须是映射")),
        }
        // An unreadable extra list blocks every completion rather than
        // dropping the project's own stricter rules.
        match strings(&file.map, "sensitive_paths").and_then(|paths| {
            for path in &paths { crate::path_guard::PathRule::parse(path)?; }
            Ok(paths)
        }) {
            Ok(paths) => file.sensitive_paths = paths,
            Err(error) => file.error = file.error.take().or(Some(error)),
        }
        match strings(&file.map, "dangerous_commands") {
            Ok(commands) => file.dangerous_commands = commands,
            Err(error) => file.error = file.error.take().or(Some(error)),
        }
        match overrides(&file.map) {
            Ok(rules) => file.overrides = rules,
            Err(error) => file.error = file.error.take().or(Some(error)),
        }
        file
    }

    /// FR-55: the declaration for one object, every override it matches
    /// merged in. Overrides only tighten: a gate once hard stays hard; an
    /// entry without `check` keeps the declared one; a different check is
    /// added beside the declared gate (as `<name>/<check>`), never in its
    /// place; a broken entry fails closed.
    pub fn scoped(&self, facets: &BTreeMap<String, Vec<String>>) -> Self {
        let mut file = self.clone();
        for rule in self.overrides.iter().filter(|rule| rule.applies(facets)) {
            for (name, value) in &rule.entries {
                let base = file.gates.get(name).cloned();
                let mut value = value.clone();
                if let (Yaml::Mapping(entry), Some(check)) = (&mut value, base.as_ref().and_then(|gate| gate.check.clone())) {
                    if !entry.contains_key("check") { entry.insert("check".into(), check.into()); }
                }
                let gate = Gate::parse(name.clone(), &value);
                let gate = match base {
                    Some(mut base) if gate.problem.is_none() && gate.check == base.check => {
                        base.hard |= gate.hard;
                        base
                    }
                    Some(_) if gate.problem.is_none() => {
                        let name = format!("{name}/{}", gate.check.as_deref().unwrap_or_default());
                        Gate { name, ..gate }
                    }
                    _ => gate,
                };
                file.gates.insert(gate.name.clone(), gate);
            }
        }
        file
    }

    /// `None` is a missing file.
    pub fn from_text(text: Option<&str>) -> Self {
        text.map(Self::parse).unwrap_or_default()
    }

    /// The working tree's copy, for display only; enforcement reads the
    /// authoritative main.
    pub fn load(root: &Path) -> Self {
        match fs::read_to_string(root.join(GATES_PATH)) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => Self { error: Some(format!("无法读取 {GATES_PATH}：{error}")), ..Self::default() },
        }
    }

    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(&self.map).expect("YAML 映射总能序列化")
    }

    /// The entry declares its check non-deterministic (`deterministic: false`).
    pub fn nondeterministic(&self, name: &str) -> bool {
        let entry = self.map.get("gates").and_then(|gates| gates.get(name));
        entry.and_then(|entry| entry.get("deterministic")) == Some(&Yaml::Bool(false))
    }

    /// The same declaration judged with these waivers in force.
    pub fn waive(mut self, waivers: Vec<Waiver>) -> Self {
        self.waivers = waivers;
        self
    }

    /// Nothing declared and nothing broken.
    pub fn is_empty(&self) -> bool {
        self.gates.is_empty() && self.error.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    /// Trusted check evidence passed on the current version.
    Passed,
    /// A deterministic signal at `active` or above, or a failed check.
    Failed,
    /// The declaration cannot be honoured.
    Error,
    /// A trusted producer exists but has no result on the current version.
    Pending,
    /// No blocking signal, and no trusted producer: blocks nothing, proves nothing.
    Clear,
    /// A failed or pending declared hard gate a person waived until a time
    /// (FR-72); its evidence names the waiver first.
    Waived,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    pub gate: String,
    pub check: Option<String>,
    pub hard: bool,
    /// One of the kit's own checks for this object rather than a `gates.yaml` entry.
    pub builtin: bool,
    pub status: GateStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Evidence IDs or signal event IDs behind the status.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

impl GateResult {
    pub fn blocks(&self) -> bool {
        self.hard && matches!(self.status, GateStatus::Failed | GateStatus::Error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Every hard gate with a trusted producer passed on the current version.
    Passed,
    /// Waiting for checks on the current version.
    Pending,
    /// A hard gate failed or cannot be honoured.
    Blocked,
    /// No deterministic check applies: only a person can complete it.
    HumanOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judgement {
    pub verdict: Verdict,
    pub version: Option<String>,
    pub gates: Vec<GateResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Judgement {
    pub fn blocking(&self) -> impl Iterator<Item = &GateResult> {
        self.gates.iter().filter(|gate| gate.blocks())
    }

    /// Blocks from `gates.yaml` alone: its errors and its failed hard gates.
    /// A kit enforces its own checks in its own pipeline.
    pub fn declared_blocks(&self) -> Option<String> {
        let mut reasons: Vec<String> = self.error.iter().cloned().collect();
        reasons.extend(self.blocking().filter(|gate| !gate.builtin).map(|gate| {
            format!("{}（{}）：{}", gate.gate, gate.check.as_deref().unwrap_or("?"),
                gate.reason.as_deref().unwrap_or("检查未通过"))
        }));
        (!reasons.is_empty()).then(|| format!("检查未通过：{}", reasons.join("；")))
    }
}

/// What the judgement looks at for one object.
pub struct Subject<'a> {
    pub object: &'a str,
    /// Versions counted as current; empty when unknown.
    pub versions: Vec<String>,
    /// The kit's own checks for this object, always hard (FR-121).
    pub builtin: &'a [String],
    pub evidence: &'a [Evidence],
    pub signals: &'a [Merged<Signal>],
}

/// Check programs are signed by the verifier; a review by the person who
/// resolved it (`reviews`).
fn trusted(entry: &Evidence) -> bool {
    match entry.check.as_deref() {
        Some(check) if crate::reviews::is_review_kind(check) => crate::question_model::human_actor(&entry.actor),
        _ => entry.actor == VERIFIER,
    }
}

fn evaluate(check: &str, subject: &Subject) -> (GateStatus, Option<String>, Vec<String>, bool) {
    let blocking: Vec<_> = subject.signals.iter()
        .filter(|signal| signal.target == subject.object && signal.data.kind == check)
        .filter(|signal| signal.data.deterministic && signal.data.level >= Level::Active)
        .collect();
    let trusted: Vec<_> = subject.evidence.iter()
        .filter(|entry| entry.source_kind == SourceKind::Check && trusted(entry))
        .filter(|entry| entry.check.as_deref() == Some(check))
        .collect();
    let produced = !trusted.is_empty() || subject.builtin.iter().any(|kind| kind == check);
    if !blocking.is_empty() {
        let reason = blocking.iter().map(|signal| format!("{}：{}", signal.from.id, signal.data.title)).collect::<Vec<_>>().join("；");
        return (GateStatus::Failed, Some(reason), blocking.iter().map(|signal| signal.last.eid.clone()).collect(), produced);
    }
    let current = trusted.iter()
        .filter(|entry| entry.version.as_ref().is_some_and(|version| subject.versions.contains(version)))
        .max_by(|a, b| (&a.at, &a.id).cmp(&(&b.at, &b.id)));
    match current {
        Some(entry) => match entry.passed {
            Some(true) => (GateStatus::Passed, None, vec![entry.id.clone()], true),
            Some(false) => (GateStatus::Failed, Some("检查未通过".into()), vec![entry.id.clone()], true),
            None => (GateStatus::Pending, Some("检查进行中".into()), vec![entry.id.clone()], true),
        },
        None if produced => (GateStatus::Pending, Some("当前版本尚无检查结果".into()), vec![], true),
        None => (GateStatus::Clear, Some("未见阻止信号；该种类尚无受信检查程序，只能阻止、不能放行".into()), vec![], false),
    }
}

/// FR-124. Pure: the same inputs always give the same judgement.
pub fn judge(file: &GateFile, subject: &Subject) -> Judgement {
    let mut gates = Vec::new();
    let mut produced_hard = false;
    let mut builtin: Vec<&String> = subject.builtin.iter().collect();
    builtin.dedup();
    for kind in &builtin {
        // A declared hard gate on the same kind replaces the implicit one;
        // a soft declaration can never soften the kit's own check.
        if file.gates.values().any(|gate| gate.hard && gate.problem.is_none() && gate.check.as_ref() == Some(*kind)) {
            continue;
        }
        let (status, reason, evidence, _) = evaluate(kind, subject);
        produced_hard = true;
        gates.push(GateResult { gate: (*kind).clone(), check: Some((*kind).clone()), hard: true, builtin: true, status, reason, evidence });
    }
    for gate in file.gates.values() {
        let is_builtin = gate.check.as_ref().is_some_and(|check| subject.builtin.contains(check));
        let result = match (&gate.problem, &gate.check) {
            (Some(problem), _) => GateResult { gate: gate.name.clone(), check: gate.check.clone(), hard: true,
                builtin: is_builtin, status: GateStatus::Error, reason: Some(problem.clone()), evidence: vec![] },
            (None, None) => unreachable!("没有 check 的门禁总带 problem"),
            (None, Some(check)) => {
                let (mut status, mut reason, mut evidence, produced) = evaluate(check, subject);
                produced_hard |= gate.hard && produced;
                let waiver = file.waivers.iter().find(|waiver| waiver.object == subject.object && waiver.gate == gate.name);
                if let Some(waiver) = waiver.filter(|_| gate.hard && !is_builtin && matches!(status, GateStatus::Failed | GateStatus::Pending)) {
                    status = GateStatus::Waived;
                    reason = Some(format!("已豁免至 {}：{}", waiver.until, waiver.reason));
                    evidence.insert(0, waiver.id.clone());
                }
                GateResult { gate: gate.name.clone(), check: Some(check.clone()), hard: gate.hard, builtin: is_builtin,
                    status, reason, evidence }
            }
        };
        gates.push(result);
    }
    let verdict = if file.error.is_some() || gates.iter().any(GateResult::blocks) {
        Verdict::Blocked
    } else if !produced_hard {
        Verdict::HumanOnly
    } else if gates.iter().filter(|gate| gate.hard && gate.status != GateStatus::Clear)
        .all(|gate| matches!(gate.status, GateStatus::Passed | GateStatus::Waived)) {
        Verdict::Passed
    } else {
        Verdict::Pending
    };
    Judgement { verdict, version: subject.versions.first().cloned(), gates, error: file.error.clone() }
}

/// The judgement for one kit object, under the overrides its facets match
/// (FR-55). Its current version is the latest
/// attempt's; once the kit proved that attempt done, its checked versions
/// count too (the kit's proof binds them to the admitted version).
pub fn judge_item(file: &GateFile, item: &KitDelegation, signals: &[Merged<Signal>]) -> Judgement {
    let mut versions = Vec::new();
    if let Some(attempt) = item.attempts.last() {
        versions.extend(attempt.version.clone());
        // Reviews asked before there was a version bind to the attempt.
        versions.push(attempt.id.clone());
        if attempt.done.is_some() {
            versions.extend(item.evidence.iter().filter(|entry| entry.attempt == attempt.id).filter_map(|entry| entry.version.clone()));
        }
    }
    judge(&file.scoped(&item.facets), &Subject { object: &item.id, versions, builtin: &item.checks, evidence: &item.evidence, signals })
}

/// The judgement for any object: a kit's delegable object, or else one with
/// only the generic check records and signals.
pub fn judge_object(state: &Value, kits: &[&dyn Kit], file: &GateFile, object: &str) -> Result<Judgement, String> {
    let signals = tower_api::read(state)?.signals;
    let items = crate::delegation_projection::delegations(state, kits)?;
    if let Some(item) = items.iter().find(|item| item.id == object) {
        return Ok(judge_item(file, item, &signals));
    }
    let evidence = check_evidence(state, object, None)?;
    let versions = evidence.iter().max_by(|a, b| (&a.at, &a.id).cmp(&(&b.at, &b.id)))
        .and_then(|entry| entry.version.clone()).into_iter().collect();
    Ok(judge(file, &Subject { object, versions, builtin: &[], evidence: &evidence, signals: &signals }))
}

/// FR-56 evidence bodies, by `kind`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recorded {
    /// The gates evaluated where they are enforced.
    Gate {
        object: String,
        version: Option<String>,
        /// Where they were enforced, e.g. `finish`, `confirm`.
        stage: String,
        /// Authoritative revision the declaration was read from.
        revision: Option<String>,
        verdict: Verdict,
        gates: Vec<GateResult>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// One check program's result on one version.
    Check {
        object: String,
        version: String,
        check: String,
        /// `None` is a check that errored: never a pass (FR-123).
        passed: Option<bool>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        criteria: Vec<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attempt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

impl Recorded {
    pub fn object(&self) -> &str {
        match self {
            Self::Gate { object, .. } | Self::Check { object, .. } => object,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub event: Mark,
    pub data: Recorded,
}

pub fn records(state: &Value) -> Result<BTreeMap<String, Record>, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}

/// Generic check records of one object as `check` evidence. Without its own
/// attempt, a record joins `fallback`.
pub fn check_evidence(state: &Value, object: &str, fallback: Option<&str>) -> Result<Vec<Evidence>, String> {
    Ok(records(state)?.into_iter().filter_map(|(id, record)| match record.data {
        Recorded::Check { object: of, version, check, passed, criteria, attempt, note } if of == object => Some(Evidence {
            id, source_kind: SourceKind::Check, criteria, passed, version: Some(version), actor: record.event.actor,
            at: record.event.at, note, attempt: attempt.or(fallback.map(str::to_owned)).unwrap_or_default(), check: Some(check),
        }),
        _ => None,
    }).collect())
}

/// Reducer for `evidence.recorded`. The group appears with the first record,
/// so projections without gate evidence keep their shape.
pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != RECORDED {
        return Ok(false);
    }
    if event.layer != LAYER || !event.guards.is_empty() {
        return Err("证据记录须在 L5 且不带围栏".into());
    }
    let data: Recorded = serde_json::from_value(event.data.clone()).map_err(|e| format!("证据记录无效：{e}"))?;
    let signer = match &data {
        Recorded::Gate { .. } => TOWER,
        Recorded::Check { version, check, .. } => {
            tower_api::signal_kind(check)?;
            if version.trim().is_empty() {
                return Err("检查证据须绑定对象版本".into());
            }
            VERIFIER
        }
    };
    if event.actor != signer {
        return Err(format!("{} 证据只能由 {signer} 写入", match data { Recorded::Gate { .. } => "门禁", Recorded::Check { .. } => "检查" }));
    }
    if data.object() != event.obj || event.obj.trim().is_empty() {
        return Err("证据记录的对象与事件不符".into());
    }
    if state.get(GROUP).is_none() {
        state[GROUP] = json!({});
    }
    let mark = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
    insert_unique(state, GROUP, &event.eid, serde_json::to_value(Record { event: mark, data }).map_err(|e| e.to_string())?)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn overrides_only_tighten_the_gates_of_the_objects_they_match() {
        let text = concat!("gates:\n  lint: { check: eng.lint, hard: false }\n  crap: { check: eng.crap }\n",
            "overrides:\n  - match: { risk: high, layer: [L4, L5] }\n    lint: { hard: true }\n    crap: { check: eng.crap2, hard: false }\n",
            "    sec: { check: eng.sec }\n  - match: { risk: low }\n    crap: { hard: false }\n");
        let file = GateFile::parse(text);
        assert!(file.error.is_none(), "{:?}", file.error);
        let facets = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), vec![v.to_string()])).collect::<BTreeMap<_, _>>();
        let hard = |file: &GateFile| file.gates.values().map(|gate| (gate.name.clone(), gate.hard, gate.problem.is_some())).collect::<Vec<_>>();
        // A matching object: lint becomes hard, a different check is added beside crap, sec is new.
        assert_eq!(hard(&file.scoped(&facets(&[("risk", "HIGH"), ("layer", "L4")]))), [("crap".into(), true, false),
            ("crap/eng.crap2".into(), false, false), ("lint".into(), true, false), ("sec".into(), true, false)]);
        // Not matching: the declaration as written. `hard: false` never softens crap.
        assert_eq!(hard(&file.scoped(&facets(&[("risk", "normal")]))), [("crap".into(), true, false), ("lint".into(), false, false)]);
        assert_eq!(hard(&file.scoped(&facets(&[("risk", "low")]))), [("crap".into(), true, false), ("lint".into(), false, false)]);
        // A facet the object does not report cannot rule an override out.
        assert_eq!(file.scoped(&facets(&[("layer", "L4")])).gates.len(), 4);
        // A broken override entry fails closed on what it matches.
        let broken = GateFile::parse("overrides:\n  - match: { risk: high }\n    sec: { check: 'not a kind!' }\n");
        assert!(broken.scoped(&facets(&[("risk", "high")])).gates["sec"].problem.is_some());
        // An unreadable list breaks the whole file.
        for bad in ["overrides: {a: 1}", "overrides: [1]", "overrides: [{lint: {hard: true}}]", "overrides: [{match: {}, lint: {hard: true}}]",
            "overrides: [{match: {risk: []}, lint: {hard: true}}]", "overrides: [{match: {risk: {a: 1}}, lint: {hard: true}}]"] {
            assert!(GateFile::parse(bad).error.is_some(), "{bad}");
        }
    }

    use super::*;
    use crate::tower_api::{From, FromKind, Muted};

    fn mark(seq: u64) -> Mark {
        Mark { at: format!("2026-10-23T10:00:{seq:02}Z"), eid: format!("e{seq}"), seq, actor: "process:towermd-cli".into() }
    }
    fn signal(from: &str, kind: FromKind, check: &str, level: Level, deterministic: bool) -> Merged<Signal> {
        Merged { from: From { id: from.into(), kind }, target: "T-1".into(),
            data: Signal { kind: check.into(), level, title: "t".into(), detail: None, value: None, threshold: None, deterministic },
            refs: vec![], at: "2026-10-23T10:00:00Z".into(), count: 1, first: mark(1), last: mark(1), acknowledged: None, muted: None }
    }
    fn check(id: &str, kind: &str, version: &str, passed: Option<bool>, actor: &str, at: u64) -> Evidence {
        Evidence { id: id.into(), source_kind: SourceKind::Check, criteria: vec![], passed, version: Some(version.into()),
            actor: actor.into(), at: format!("2026-10-23T10:00:{at:02}Z"), note: None, attempt: "A1".into(), check: Some(kind.into()) }
    }
    fn subject<'a>(builtin: &'a [String], evidence: &'a [Evidence], signals: &'a [Merged<Signal>]) -> Subject<'a> {
        Subject { object: "T-1", versions: vec!["v2".into()], builtin, evidence, signals }
    }

    #[test]
    fn the_file_is_read_leniently_and_round_trips_unknown_keys() {
        let text = "owner: me\ngates:\n  lint: { check: eng.lint, hard: false, stage: pre }\n  crap: eng.crap\n  bad: 3\n  \
            nocheck: { hard: true }\n  review: { check: agent.review, deterministic: false }\noverrides: [ { match: { risk: high } } ]\n";
        let file = GateFile::parse(text);
        assert_eq!(file.error, None);
        assert_eq!(file.gates["lint"], Gate { name: "lint".into(), check: Some("eng.lint".into()), hard: false, problem: None });
        assert_eq!(file.gates["crap"].check.as_deref(), Some("eng.crap"));
        assert!(file.gates["crap"].hard);
        for name in ["bad", "nocheck", "review"] {
            assert!(file.gates[name].problem.is_some(), "{name}");
        }
        let again: Yaml = serde_yaml::from_str(&file.to_yaml()).unwrap();
        assert_eq!(again, serde_yaml::from_str::<Yaml>(text).unwrap());
        assert!(GateFile::from_text(None).is_empty() && GateFile::parse("").is_empty());
        for broken in ["[1]", "gates: [a]", "gates: {", "sensitive_paths: x", "sensitive_paths: [\"/etc\"]",
            "sensitive_paths: [a/*.pem]", "dangerous_commands: [\" \"]", "dangerous_commands: {a: 1}"] {
            assert!(GateFile::parse(broken).error.is_some(), "{broken}");
        }
        let extra = GateFile::parse("sensitive_paths: [secrets/, config/prod.yaml]\ndangerous_commands: [\"terraform destroy\"]\n");
        assert_eq!((extra.error, extra.sensitive_paths.len(), extra.dangerous_commands), (None, 2, vec!["terraform destroy".to_owned()]));
    }

    #[test]
    fn a_missing_or_broken_file_never_opens_a_gate() {
        let file = GateFile::parse("gates: [x]");
        let judgement = judge(&file, &subject(&[], &[], &[]));
        assert_eq!(judgement.verdict, Verdict::Blocked);
        assert!(judgement.declared_blocks().unwrap().contains("gates"));
        let file = GateFile::parse("gates:\n  odd: { hard: maybe, check: eng.x }\n");
        let judgement = judge(&file, &subject(&[], &[], &[]));
        assert_eq!((judgement.verdict, judgement.gates[0].status), (Verdict::Blocked, GateStatus::Error));
        assert_eq!(judge(&GateFile::default(), &subject(&[], &[], &[])).verdict, Verdict::HumanOnly);
    }

    #[test]
    fn only_trusted_checks_on_the_current_version_pass_and_any_active_signal_fails() {
        let builtin = vec!["core.verify".to_owned()];
        let none = GateFile::default();
        assert_eq!(judge(&none, &subject(&builtin, &[], &[])).verdict, Verdict::Pending);
        let old = [check("E1", "core.verify", "v1", Some(true), VERIFIER, 1)];
        assert_eq!(judge(&none, &subject(&builtin, &old, &[])).gates[0].status, GateStatus::Pending);
        let forged = [check("E2", "core.verify", "v2", Some(true), "agent/1", 2)];
        assert_eq!(judge(&none, &subject(&builtin, &forged, &[])).verdict, Verdict::Pending);
        let failed_then_passed = [check("E3", "core.verify", "v2", Some(false), VERIFIER, 3), check("E4", "core.verify", "v2", Some(true), VERIFIER, 4)];
        let judgement = judge(&none, &subject(&builtin, &failed_then_passed, &[]));
        assert_eq!((judgement.verdict, judgement.gates[0].evidence.clone()), (Verdict::Passed, vec!["E4".to_owned()]));
        let errored = [check("E5", "core.verify", "v2", None, VERIFIER, 5)];
        assert_eq!(judge(&none, &subject(&builtin, &errored, &[])).verdict, Verdict::Pending);

        // A declared gate on a signal kind: active fails it from any sender;
        // quiet or self-described signals prove nothing.
        let file = GateFile::parse("gates:\n  crap: { check: eng.crap }\n");
        let quiet = [signal("ci", FromKind::Source, "eng.crap", Level::Passive, true)];
        let judgement = judge(&file, &subject(&builtin, &failed_then_passed, &quiet));
        assert_eq!((judgement.verdict, judgement.gates[1].status), (Verdict::Passed, GateStatus::Clear));
        for sender in [FromKind::Source, FromKind::Delegate] {
            let loud = [signal("x", sender, "eng.crap", Level::Active, true)];
            let judgement = judge(&file, &subject(&builtin, &failed_then_passed, &loud));
            assert_eq!(judgement.verdict, Verdict::Blocked);
            assert!(judgement.declared_blocks().unwrap().contains("crap"));
        }
        // Acknowledging or muting quiets the attention list, never a check.
        let quieted = [Merged {
            acknowledged: Some(mark(2)),
            muted: Some(Muted { until: "2026-10-30T10:00:00Z".into(), level: Level::Active, event: mark(3) }),
            ..signal("x", FromKind::Source, "eng.crap", Level::Active, true)
        }];
        assert_eq!(judge(&file, &subject(&builtin, &failed_then_passed, &quieted)).verdict, Verdict::Blocked);
        let opinion = [signal("x", FromKind::Source, "eng.crap", Level::TimeSensitive, false)];
        assert_eq!(judge(&file, &subject(&builtin, &failed_then_passed, &opinion)).verdict, Verdict::Passed);
        let elsewhere = [Merged { target: "T-2".into(), ..signal("x", FromKind::Source, "eng.crap", Level::Active, true) }];
        assert_eq!(judge(&file, &subject(&builtin, &failed_then_passed, &elsewhere)).verdict, Verdict::Passed);
        // Without a trusted producer a quiet gate leaves completion to the person.
        assert_eq!(judge(&file, &subject(&[], &[], &quiet)).verdict, Verdict::HumanOnly);
    }

    #[test]
    fn soft_gates_warn_and_never_soften_a_kit_check() {
        let builtin = vec!["core.verify".to_owned()];
        let file = GateFile::parse("gates:\n  verify: { check: core.verify, hard: false }\n  lint: { check: eng.lint, hard: false }\n");
        let loud = [signal("ci", FromKind::Source, "eng.lint", Level::Active, true)];
        let judgement = judge(&file, &subject(&builtin, &[], &loud));
        assert_eq!(judgement.verdict, Verdict::Pending, "the kit's check stays hard");
        assert!(judgement.gates.iter().any(|gate| gate.gate == "core.verify" && gate.hard && gate.builtin));
        assert!(judgement.gates.iter().any(|gate| gate.gate == "lint" && gate.status == GateStatus::Failed && !gate.blocks()));
        assert_eq!(judgement.declared_blocks(), None);
    }

    fn waiver(object: &str, gate: &str) -> Waiver {
        Waiver { id: "W1".into(), object: object.into(), gate: gate.into(), reason: "上游修复前先放行".into(),
            until: "2026-10-30T10:00:00Z".into(), granted: Mark { actor: "human:local".into(), ..mark(9) } }
    }

    #[test]
    fn a_waiver_lets_one_declared_hard_gate_through_and_nothing_else() {
        let builtin = vec!["core.verify".to_owned()];
        let passed = [check("E1", "core.verify", "v2", Some(true), VERIFIER, 1)];
        let loud = [signal("ci", FromKind::Source, "eng.crap", Level::Active, true)];
        let file = GateFile::parse("gates:\n  crap: { check: eng.crap }\n");
        assert_eq!(judge(&file, &subject(&builtin, &passed, &loud)).verdict, Verdict::Blocked);

        let waived = judge(&file.clone().waive(vec![waiver("T-1", "crap")]), &subject(&builtin, &passed, &loud));
        assert_eq!(waived.verdict, Verdict::Passed);
        let gate = waived.gates.iter().find(|gate| gate.gate == "crap").unwrap();
        assert_eq!(gate.status, GateStatus::Waived);
        assert!(!gate.blocks() && gate.reason.as_deref().unwrap().contains("2026-10-30T10:00:00Z"));
        assert_eq!(gate.evidence[0], "W1", "the waiver is cited before the signal it overrides");
        assert_eq!(waived.declared_blocks(), None);

        // Another object or another gate is not waived; a lapsed waiver is
        // simply not attached, so the gate blocks again.
        for other in [waiver("T-2", "crap"), waiver("T-1", "lint")] {
            assert_eq!(judge(&file.clone().waive(vec![other]), &subject(&builtin, &passed, &loud)).verdict, Verdict::Blocked);
        }
        // Without a trusted producer a waived gate only unblocks: a person still completes it.
        assert_eq!(judge(&file.clone().waive(vec![waiver("T-1", "crap")]), &subject(&[], &[], &loud)).verdict, Verdict::HumanOnly);

        // Never waived: the kit's own check, even when declared by name; a
        // broken entry; a broken file.
        let verify = GateFile::parse("gates:\n  verify: { check: core.verify }\n").waive(vec![waiver("T-1", "verify")]);
        let failed = [check("E2", "core.verify", "v2", Some(false), VERIFIER, 2)];
        assert_eq!(judge(&verify, &subject(&builtin, &failed, &[])).gates[0].status, GateStatus::Failed);
        let broken = GateFile::parse("gates:\n  crap: { check: eng.crap, hard: maybe }\n").waive(vec![waiver("T-1", "crap")]);
        assert_eq!(judge(&broken, &subject(&builtin, &passed, &[])).gates[1].status, GateStatus::Error);
        let unreadable = GateFile::parse("gates: [x]").waive(vec![waiver("T-1", "crap")]);
        assert_eq!(judge(&unreadable, &subject(&builtin, &passed, &[])).verdict, Verdict::Blocked);
        // A soft gate never blocks, so there is nothing to waive.
        let soft = GateFile::parse("gates:\n  crap: { check: eng.crap, hard: false }\n").waive(vec![waiver("T-1", "crap")]);
        assert_eq!(judge(&soft, &subject(&builtin, &passed, &loud)).gates[1].status, GateStatus::Failed);
    }

    fn recorded(actor: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":3,"eid":"g1","host":"h","t":"2026-10-23T10:00:03Z","actor":actor,
            "type":RECORDED,"layer":"L5","obj":"T-1","data":data,"epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    #[test]
    fn evidence_is_signed_by_its_producer_bound_to_a_version_and_written_once() {
        let gate = json!({"kind":"gate","object":"T-1","version":"v2","stage":"finish","revision":null,"verdict":"blocked",
            "gates":[{"gate":"crap","check":"eng.crap","hard":true,"builtin":false,"status":"failed"}]});
        let check = json!({"kind":"check","object":"T-1","version":"v2","check":"eng.lint","passed":true});
        let mut state = json!({});
        assert!(!apply(&mut state, &Event { kind: "x.y".into(), ..recorded(TOWER, gate.clone()) }).unwrap());
        for (actor, data) in [(VERIFIER, &gate), ("human:local", &gate), (TOWER, &check), ("process:towermd-cli", &check)] {
            assert!(apply(&mut state, &recorded(actor, data.clone())).is_err(), "{actor}");
        }
        assert!(state.get(GROUP).is_none(), "the group appears only with the first record");
        let mut unbound = check.clone();
        unbound["version"] = json!(" ");
        assert!(apply(&mut state, &recorded(VERIFIER, unbound)).is_err());
        let mut other = gate.clone();
        other["object"] = json!("T-2");
        assert!(apply(&mut state, &recorded(TOWER, other)).is_err());
        assert!(apply(&mut state, &Event { layer: "L4".into(), ..recorded(TOWER, gate.clone()) }).is_err());

        assert!(apply(&mut state, &recorded(TOWER, gate.clone())).unwrap());
        assert!(apply(&mut state, &recorded(TOWER, gate.clone())).unwrap(), "replay is idempotent");
        let mut changed = gate;
        changed["verdict"] = json!("passed");
        assert!(apply(&mut state, &recorded(TOWER, changed)).is_err(), "evidence is never rewritten");
        assert!(apply(&mut state, &Event { eid: "c1".into(), ..recorded(VERIFIER, check) }).unwrap());
        let evidence = check_evidence(&state, "T-1", Some("A1")).unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!((evidence[0].check.as_deref(), evidence[0].version.as_deref(), evidence[0].attempt.as_str(), evidence[0].actor.as_str()),
            (Some("eng.lint"), Some("v2"), "A1", VERIFIER));
        let file = GateFile::parse("gates:\n  lint: { check: eng.lint }\n");
        let judgement = judge_object(&state, &[], &file, "T-1").unwrap();
        assert_eq!((judgement.verdict, judgement.version.as_deref()), (Verdict::Passed, Some("v2")));
    }

    #[test]
    fn a_review_kind_passes_only_on_a_persons_passing_review_and_never_by_waiting() {
        let builtin = vec!["human.review.code".to_owned()];
        let none = GateFile::default();
        assert_eq!(judge(&none, &subject(&builtin, &[], &[])).verdict, Verdict::Pending, "an open review stays pending");
        for actor in [VERIFIER, TOWER, "agent/1", "process:towermd-cli"] {
            let forged = [check("V1", "human.review.code", "v2", Some(true), actor, 1)];
            assert_eq!(judge(&none, &subject(&builtin, &forged, &[])).verdict, Verdict::Pending, "{actor}");
        }
        let rejected = [check("V2", "human.review.code", "v2", Some(false), "human:local", 2)];
        assert_eq!(judge(&none, &subject(&builtin, &rejected, &[])).verdict, Verdict::Blocked);
        let other_version = [check("V3", "human.review.code", "v1", Some(true), "human:local", 3)];
        assert_eq!(judge(&none, &subject(&builtin, &other_version, &[])).verdict, Verdict::Pending, "a review binds its version");
        let passed = [check("V4", "human.review.code", "v2", Some(true), "human:local", 4)];
        assert_eq!(judge(&none, &subject(&builtin, &passed, &[])).verdict, Verdict::Passed);
        let waived = GateFile { waivers: vec![Waiver { id: "W".into(), object: "T-1".into(), gate: "human.review.code".into(),
            reason: "r".into(), until: "2099-01-01T00:00:00Z".into(), granted: mark(9) }], ..GateFile::default() };
        assert_eq!(judge(&waived, &subject(&builtin, &rejected, &[])).verdict, Verdict::Blocked, "a kit's review is never waived");
    }
}
