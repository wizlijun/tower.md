//! Typed task admission and projection. This module cannot certify completion.
//! Only the publication proof reader may construct an active baseline.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use towermd_core::delegation::{Brief, Checkpoint};
use towermd_core::hub::{Fence, Lease};
use towermd_core::path_guard::PathRule;
pub use towermd_core::path_guard::{path_is_allowed, path_is_within};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleScope {
    pub paths: Vec<String>,
}

/// Input for the same-crate publication reader, after validating the manifest,
/// actual Git publication and original accepted human approval envelope.
pub(crate) struct PublishedBaseline {
    pub tag: String,
    pub commit: String,
    pub approval_eid: String,
    pub content_hash: String,
    pub modules: BTreeMap<String, ModuleScope>,
    pub decisions: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct VerifiedBaseline {
    tag: String,
    commit: String,
    approval_eid: String,
    content_hash: String,
    modules: BTreeMap<String, ModuleScope>,
    decisions: BTreeSet<String>,
}

impl VerifiedBaseline {
    pub(crate) fn from_published(input: PublishedBaseline) -> Result<Self, String> {
        let number = input
            .tag
            .strip_prefix("towermd/B")
            .ok_or("批准基线 tag 无效")?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err("批准基线 tag 无效".into());
        }
        if !matches!(input.commit.len(), 40 | 64)
            || !input.commit.bytes().all(|b| b.is_ascii_hexdigit())
            || input.content_hash.len() != 64
            || !input.content_hash.bytes().all(|b| b.is_ascii_hexdigit())
            || input.approval_eid.trim().is_empty()
            || input.modules.is_empty()
        {
            return Err("批准基线证明元数据不完整".into());
        }
        for (id, module) in &input.modules {
            valid_id(id)?;
            if module.paths.is_empty() {
                return Err(format!("已批准模块 {id} 缺少路径上限"));
            }
            for path in &module.paths {
                PathRule::parse(path)?;
            }
        }
        for id in &input.decisions {
            valid_id(id)?;
        }
        Ok(Self {
            tag: input.tag,
            commit: input.commit,
            approval_eid: input.approval_eid,
            content_hash: input.content_hash,
            modules: input.modules,
            decisions: input.decisions,
        })
    }
    pub fn tag(&self) -> &str {
        &self.tag
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn approval_eid(&self) -> &str {
        &self.approval_eid
    }
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
    pub fn modules(&self) -> &BTreeMap<String, ModuleScope> {
        &self.modules
    }
    pub fn decisions(&self) -> &BTreeSet<String> {
        &self.decisions
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Proposed,
    Ready,
    Running,
    Paused,
    Verifying,
    Held,
    Merged,
}

/// `acceptance` is the card's `done_when` (on-disk name unchanged). The card
/// fields after `decisions` are v0.7 additions, omitted when empty so v0.1.1
/// events round-trip byte for byte.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDraft {
    pub id: String,
    pub title: String,
    pub module: String,
    #[serde(default)]
    pub touches: Vec<String>,
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<Checkpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_check: Option<String>,
    /// Ask the delegate to clarify first (FR-201): the task stays proposed
    /// until a person finalizes the card.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clarify: bool,
}

impl TaskDraft {
    /// The core delegation card; a kit fills values, never fields.
    pub fn brief(&self) -> Brief {
        Brief {
            why: self.why.clone(),
            done_when: self.acceptance.clone(),
            limits: self.limits.clone(),
            checkpoints: self.checkpoints.clone(),
            self_check: self.self_check.clone(),
        }
    }

    fn set_brief(&mut self, brief: &Brief) {
        self.why = brief.why.clone();
        self.acceptance = brief.done_when.clone();
        self.limits = brief.limits.clone();
        self.checkpoints = brief.checkpoints.clone();
        self.self_check = brief.self_check.clone();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOrigin {
    Cli,
    AppHuman,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub input: TaskDraft,
    pub origin: TaskOrigin,
    pub baseline: String,
    pub state: TaskState,
    pub needs_review: bool,
    pub active_run: Option<String>,
}

pub use towermd_core::question_model::SelfReported as HarnessSource;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBinding {
    pub id: String,
    pub host: String,
    /// The daemon supplies the canonical absolute worktree path.
    pub worktree: String,
    pub branch: String,
    #[serde(default)]
    pub self_reported: Option<HarnessSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Active,
    Released,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub binding: RunBinding,
    pub task: String,
    pub module: String,
    pub baseline: String,
    pub fences: Vec<Fence>,
    pub state: RunState,
    pub cost_usd: Option<f64>,
}
impl Run {
    pub fn primary_fence(&self) -> Option<&Fence> {
        let name = format!("module:{}", self.module);
        self.fences.iter().find(|fence| fence.name == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseReason {
    Voluntary,
    Expired,
    Reassigned,
    VerificationTimeout,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskEvent {
    Created {
        task: Task,
    },
    Claimed {
        run: Run,
    },
    Released {
        run_id: String,
        reason: ReleaseReason,
    },
    NeedsReview {
        task_id: String,
    },
    Paused {
        task_id: String,
    },
    Resumed {
        task_id: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskProjection {
    pub tasks: BTreeMap<String, Task>,
    pub runs: BTreeMap<String, Run>,
    #[serde(default)]
    pub paused_all: bool,
}

fn valid_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('.')
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(format!("对象 ID 无效：{value}"));
    }
    Ok(())
}

fn unique_ids(values: &[String], label: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for value in values {
        valid_id(value)?;
        if !seen.insert(value) {
            return Err(format!("{label} 不得重复"));
        }
    }
    Ok(())
}

fn modules(input: &TaskDraft) -> Vec<&str> {
    std::iter::once(input.module.as_str())
        .chain(input.touches.iter().map(String::as_str))
        .collect()
}

fn validate_draft(input: &TaskDraft) -> Result<(), String> {
    valid_id(&input.id)?;
    if input.id.ends_with('.') || input.id.ends_with(".lock") || input.id.contains("..") {
        return Err("任务 ID 不能形成有效任务分支".into());
    }
    valid_id(&input.module)?;
    if input.title.trim().is_empty() || input.title.len() > 1024 {
        return Err("任务标题不能为空或超过 1024 字节".into());
    }
    unique_ids(&input.touches, "touches")?;
    unique_ids(&input.depends_on, "任务依赖")?;
    unique_ids(&input.decisions, "相关决策")?;
    if input.touches.contains(&input.module) || input.depends_on.contains(&input.id) {
        return Err("touches 不能重复主模块；任务不能依赖自己".into());
    }
    let mut paths = BTreeSet::new();
    for path in &input.allowed_paths {
        PathRule::parse(path)?;
        if !paths.insert(path) {
            return Err("allowed_paths 不得重复".into());
        }
    }
    if input.acceptance.iter().any(|item| item.trim().is_empty()) {
        return Err("验收条目不能为空".into());
    }
    input.brief().check(false)
}

fn validate_scope(
    baseline: &VerifiedBaseline,
    input: &TaskDraft,
    origin: TaskOrigin,
) -> Result<(), String> {
    let mut ceilings = Vec::new();
    for module in modules(input) {
        ceilings.extend(
            baseline
                .modules
                .get(module)
                .ok_or_else(|| format!("模块未进入已批准基线：{module}"))?
                .paths
                .iter()
                .map(|path| PathRule::parse(path))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    if origin == TaskOrigin::Cli {
        for path in &input.allowed_paths {
            let child = PathRule::parse(path)?;
            if !ceilings.iter().any(|parent| parent.covers(&child)) {
                return Err(format!("任务路径超出主模块与 touches 的已批准范围：{path}"));
            }
        }
    }
    Ok(())
}

pub fn prepare_add(
    baseline: &VerifiedBaseline,
    projection: &TaskProjection,
    input: TaskDraft,
    origin: TaskOrigin,
) -> Result<TaskEvent, String> {
    validate_draft(&input)?;
    validate_scope(baseline, &input, origin)?;
    if projection.tasks.contains_key(&input.id) {
        return Err("任务 ID 已存在；重试须使用原事件 eid".into());
    }
    for dependency in &input.depends_on {
        if !projection.tasks.contains_key(dependency) {
            return Err(format!("依赖任务不存在：{dependency}"));
        }
    }
    let ready = !input.clarify
        && !input.allowed_paths.is_empty()
        && !input.acceptance.is_empty()
        && input
            .decisions
            .iter()
            .all(|id| baseline.decisions.contains(id))
        && input.depends_on.iter().all(|id| {
            projection.tasks[id].state == TaskState::Merged && !projection.tasks[id].needs_review
        });
    Ok(TaskEvent::Created {
        task: Task {
            input,
            origin,
            baseline: baseline.tag.clone(),
            state: if ready {
                TaskState::Ready
            } else {
                TaskState::Proposed
            },
            needs_review: false,
            active_run: None,
        },
    })
}

/// Called before acquiring leases, and again while preparing the claimed event.
/// `Merged` must only enter the projection through the separate merge proof reader.
pub fn check_g0(
    baseline: &VerifiedBaseline,
    projection: &TaskProjection,
    task_id: &str,
) -> Result<(), String> {
    let task = projection.tasks.get(task_id).ok_or("任务不存在")?;
    if projection.paused_all || task.state == TaskState::Paused {
        return Err("任务领取已暂停".into());
    }
    if task.needs_review {
        return Err("任务需要按新基线复核".into());
    }
    if !matches!(task.state, TaskState::Ready | TaskState::Proposed) || task.active_run.is_some() {
        return Err("任务已有运行或不能领取".into());
    }
    validate_draft(&task.input)?;
    validate_scope(baseline, &task.input, task.origin)?;
    if task.input.clarify {
        return Err("G0：委托卡尚在澄清，须由人定稿后才能领取".into());
    }
    if task.input.allowed_paths.is_empty() || task.input.acceptance.is_empty() {
        return Err("G0：缺少允许路径或验收标准".into());
    }
    for id in &task.input.decisions {
        if !baseline.decisions.contains(id) {
            return Err(format!("G0：权威 main 缺少相关决策 {id}"));
        }
    }
    for id in &task.input.depends_on {
        if projection
            .tasks
            .get(id)
            .is_none_or(|task| task.state != TaskState::Merged || task.needs_review)
        {
            return Err(format!("G0：依赖任务尚未认证合并 {id}"));
        }
    }
    Ok(())
}

fn validate_binding(binding: &RunBinding, task_id: &str) -> Result<(), String> {
    valid_id(&binding.id)?;
    if binding.host.trim().is_empty()
        || binding
            .host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("运行缺少有效 host".into());
    }
    let path = Path::new(&binding.worktree);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || binding
            .worktree
            .split(std::path::MAIN_SEPARATOR)
            .any(|part| part == "." || part == "..")
        || binding.worktree.chars().any(char::is_control)
        || binding.branch != format!("t/{task_id}")
    {
        return Err("领取须在规范绝对工作树路径及 t/<task-id> 分支上".into());
    }
    if let Some(source) = &binding.self_reported {
        source.check().map_err(|_| "harness 自报来源缺少名称或版本，或可选字段为空")?;
    }
    Ok(())
}

fn validate_fences(input: &TaskDraft, run: &Run) -> Result<(), String> {
    let expected = modules(input)
        .into_iter()
        .map(|id| format!("module:{id}"))
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    for fence in &run.fences {
        if fence.holder != run.binding.id || fence.epoch == 0 || !actual.insert(fence.name.clone())
        {
            return Err("模块租约持有者、epoch 或唯一性无效".into());
        }
    }
    if actual != expected {
        return Err("必须同时持有主模块与全部 touches 的完整租约".into());
    }
    Ok(())
}

pub fn prepare_claim(
    baseline: &VerifiedBaseline,
    projection: &TaskProjection,
    task_id: &str,
    binding: RunBinding,
    leases: &[Lease],
    now_ms: u64,
) -> Result<TaskEvent, String> {
    check_g0(baseline, projection, task_id)?;
    validate_binding(&binding, task_id)?;
    if projection.runs.contains_key(&binding.id) {
        return Err("运行 ID 已存在".into());
    }
    if projection.runs.values().any(|run| {
        run.state == RunState::Active
            && run.binding.host == binding.host
            && run.binding.worktree == binding.worktree
    }) {
        return Err("该工作树已有活动运行".into());
    }
    if leases.iter().any(|lease| lease.expires_at <= now_ms) {
        return Err("模块租约已过期".into());
    }
    let task = &projection.tasks[task_id];
    let run = Run {
        binding,
        task: task_id.into(),
        module: task.input.module.clone(),
        baseline: baseline.tag.clone(),
        fences: leases.iter().map(Lease::fence).collect(),
        state: RunState::Active,
        cost_usd: None,
    };
    validate_fences(&task.input, &run)?;
    Ok(TaskEvent::Claimed { run })
}

pub fn prepare_release(
    projection: &TaskProjection,
    run_id: &str,
    reason: ReleaseReason,
) -> Result<TaskEvent, String> {
    let run = projection.runs.get(run_id).ok_or("运行不存在")?;
    if run.state != RunState::Active {
        return Err("运行已释放".into());
    }
    Ok(TaskEvent::Released {
        run_id: run_id.into(),
        reason,
    })
}

impl TaskProjection {
    /// Replay already accepted, non-stale typed business events in hub seq order.
    /// Failure leaves the projection unchanged.
    pub fn apply(&mut self, event: &TaskEvent) -> Result<(), String> {
        match event {
            TaskEvent::Created { task } => {
                validate_draft(&task.input)?;
                if self.tasks.contains_key(&task.input.id)
                    || task.active_run.is_some()
                    || task.needs_review
                    || !matches!(task.state, TaskState::Proposed | TaskState::Ready)
                    || (task.input.clarify && task.state != TaskState::Proposed)
                {
                    return Err("非法任务创建事件".into());
                }
                self.tasks.insert(task.input.id.clone(), task.clone());
            }
            TaskEvent::Claimed { run } => {
                let task = self.tasks.get(&run.task).ok_or("领取事件缺少任务")?;
                validate_binding(&run.binding, &run.task)?;
                validate_fences(&task.input, run)?;
                if self.paused_all
                    || task.needs_review
                    || task.active_run.is_some()
                    || !matches!(task.state, TaskState::Proposed | TaskState::Ready)
                    || task.input.clarify
                    || task.input.allowed_paths.is_empty()
                    || task.input.acceptance.is_empty()
                    || task.input.depends_on.iter().any(|id| {
                        self.tasks
                            .get(id)
                            .is_none_or(|task| task.state != TaskState::Merged || task.needs_review)
                    })
                    || run.state != RunState::Active
                    || run.cost_usd.is_some()
                    || run.module != task.input.module
                    || self.runs.contains_key(&run.binding.id)
                    || self.runs.values().any(|existing| {
                        existing.state == RunState::Active
                            && existing.binding.host == run.binding.host
                            && existing.binding.worktree == run.binding.worktree
                    })
                {
                    return Err("非法或重复运行绑定事件".into());
                }
                let task = self.tasks.get_mut(&run.task).unwrap();
                task.active_run = Some(run.binding.id.clone());
                task.state = TaskState::Running;
                self.runs.insert(run.binding.id.clone(), run.clone());
            }
            TaskEvent::Released { run_id, .. } => {
                let run = self.runs.get(run_id).ok_or("释放事件缺少运行")?;
                let task = self.tasks.get(&run.task).ok_or("释放事件缺少任务")?;
                if run.state != RunState::Active
                    || task.active_run.as_deref() != Some(run_id)
                    || task.state == TaskState::Merged
                {
                    return Err("释放事件不对应当前活动运行".into());
                }
                let task = self.tasks.get_mut(&run.task).unwrap();
                task.active_run = None;
                task.state = TaskState::Ready;
                self.runs.get_mut(run_id).unwrap().state = RunState::Released;
            }
            TaskEvent::NeedsReview { task_id } => {
                let task = self.tasks.get_mut(task_id).ok_or("任务不存在")?;
                if task.state != TaskState::Merged {
                    task.needs_review = true;
                }
            }
            TaskEvent::Paused { task_id } => {
                let task = self.tasks.get_mut(task_id).ok_or("任务不存在")?;
                if !matches!(
                    task.state,
                    TaskState::Ready | TaskState::Proposed | TaskState::Running | TaskState::Held
                ) {
                    return Err("任务当前不能暂停".into());
                }
                task.state = TaskState::Paused;
            }
            TaskEvent::Resumed { task_id } => {
                let task = self.tasks.get_mut(task_id).ok_or("任务不存在")?;
                if task.state != TaskState::Paused || task.needs_review || self.paused_all {
                    return Err("任务当前不能继续".into());
                }
                task.state = if task.active_run.is_some() {
                    TaskState::Running
                } else {
                    TaskState::Ready
                };
            }
        }
        Ok(())
    }

    /// A person finalized the card (FR-201). Only a proposed task changes;
    /// readiness is a hint, G0 is checked again at claim time. Tasks citing
    /// decisions stay proposed because the reducer cannot see main.
    pub fn finalize(&mut self, task_id: &str, brief: &Brief) -> Result<(), String> {
        brief.check(true)?;
        let task = self.tasks.get(task_id).ok_or("任务不存在")?;
        if task.state != TaskState::Proposed || task.active_run.is_some() {
            return Err("只有尚未委托的任务能定稿委托卡".into());
        }
        let mut input = task.input.clone();
        input.set_brief(brief);
        input.clarify = false;
        validate_draft(&input)?;
        let ready = !task.needs_review
            && !input.allowed_paths.is_empty()
            && input.decisions.is_empty()
            && input.depends_on.iter().all(|id| {
                self.tasks.get(id).is_some_and(|task| task.state == TaskState::Merged && !task.needs_review)
            });
        let task = self.tasks.get_mut(task_id).unwrap();
        task.input = input;
        if ready {
            task.state = TaskState::Ready;
        }
        Ok(())
    }

    /// A person sent a merged task back: it can be claimed again as a new
    /// delegation. Nothing here marks any work complete.
    pub fn reopen(&mut self, task_id: &str) -> Result<(), String> {
        let task = self.tasks.get_mut(task_id).ok_or("任务不存在")?;
        if task.state != TaskState::Merged || task.active_run.is_some() {
            return Err("只有已合并的任务能被退回重开".into());
        }
        task.state = TaskState::Ready;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> VerifiedBaseline {
        VerifiedBaseline::from_published(PublishedBaseline {
            tag: "towermd/B0".into(),
            commit: "a".repeat(40),
            approval_eid: uuid::Uuid::now_v7().to_string(),
            content_hash: "b".repeat(64),
            modules: BTreeMap::from([
                (
                    "A-engine".into(),
                    ModuleScope {
                        paths: vec!["src/engine/**".into(), "tests/engine/**".into()],
                    },
                ),
                (
                    "A-api".into(),
                    ModuleScope {
                        paths: vec!["src/api/**".into()],
                    },
                ),
            ]),
            decisions: BTreeSet::from(["D-api".into()]),
        })
        .unwrap()
    }
    fn input(id: &str) -> TaskDraft {
        TaskDraft {
            id: id.into(),
            title: "Implement increment".into(),
            module: "A-engine".into(),
            touches: vec![],
            allowed_paths: vec!["src/engine/math.rs".into()],
            depends_on: vec![],
            acceptance: vec!["integer increment passes".into()],
            priority: 1,
            decisions: vec![],
            ..TaskDraft::default()
        }
    }
    fn added(input: TaskDraft) -> TaskProjection {
        let mut state = TaskProjection::default();
        let event = prepare_add(&baseline(), &state, input, TaskOrigin::Cli).unwrap();
        state.apply(&event).unwrap();
        state
    }
    fn binding(task: &str, run: &str, path: &str) -> RunBinding {
        RunBinding {
            id: run.into(),
            host: "qa-host".into(),
            worktree: path.into(),
            branch: format!("t/{task}"),
            self_reported: Some(HarnessSource {
                name: "example".into(),
                version: "1".into(),
                ..HarnessSource::default()
            }),
        }
    }
    fn leases(run: &str) -> Vec<Lease> {
        vec![Lease {
            name: "module:A-engine".into(),
            holder: run.into(),
            epoch: 7,
            expires_at: 1_000,
        }]
    }

    #[test]
    fn paths_are_literal_or_directory_scopes_with_component_boundaries() {
        assert!(path_is_within("src/engine/math.rs", "src/engine/**").unwrap());
        assert!(path_is_within("src/engine/nested/**", "src/engine/").unwrap());
        assert!(!path_is_within("src/engine2/math.rs", "src/engine/**").unwrap());
        assert!(!path_is_within("src/engine/**", "src/engine/math.rs").unwrap());
        assert!(path_is_allowed("src/engine/math.rs", &["src/engine/**".into()]).unwrap());
        for unsafe_path in [
            "**",
            "*",
            "../src",
            "src/../secret",
            "/src",
            "C:\\src",
            ".git/config",
            "src/*.rs",
            "src//x",
            "./src",
            "src/?",
        ] {
            assert!(
                path_is_within(unsafe_path, "src/**").is_err(),
                "{unsafe_path}"
            );
        }
    }

    #[test]
    fn cli_paths_need_declared_touches_and_human_widening_is_explicit() {
        let baseline = baseline();
        let state = TaskProjection::default();
        let mut draft = input("T-1");
        draft.allowed_paths.push("src/api/router.rs".into());
        assert!(
            prepare_add(&baseline, &state, draft.clone(), TaskOrigin::Cli)
                .unwrap_err()
                .contains("超出")
        );
        draft.touches.push("A-api".into());
        assert!(prepare_add(&baseline, &state, draft.clone(), TaskOrigin::Cli).is_ok());
        draft.allowed_paths.push("new-package/**".into());
        assert!(prepare_add(&baseline, &state, draft.clone(), TaskOrigin::Cli).is_err());
        let event = prepare_add(&baseline, &state, draft, TaskOrigin::AppHuman).unwrap();
        let TaskEvent::Created { task } = event else {
            panic!()
        };
        assert_eq!(task.origin, TaskOrigin::AppHuman);
    }

    #[test]
    fn missing_acceptance_and_decisions_block_g0_without_inventing_readiness() {
        let mut draft = input("T-1");
        draft.acceptance.clear();
        let state = added(draft);
        assert_eq!(state.tasks["T-1"].state, TaskState::Proposed);
        assert!(check_g0(&baseline(), &state, "T-1")
            .unwrap_err()
            .contains("验收"));
        let mut draft = input("T-2");
        draft.decisions.push("D-unpublished".into());
        let state = added(draft);
        assert!(check_g0(&baseline(), &state, "T-2")
            .unwrap_err()
            .contains("权威 main"));
        assert!(check_g0(&baseline(), &added(input("T-3")), "T-3").is_ok());
    }

    #[test]
    fn dependencies_require_certified_merged_and_not_just_reported_done() {
        let mut state = added(input("T-1"));
        let mut draft = input("T-2");
        draft.depends_on.push("T-1".into());
        let event = prepare_add(&baseline(), &state, draft, TaskOrigin::Cli).unwrap();
        state.apply(&event).unwrap();
        assert!(check_g0(&baseline(), &state, "T-2").is_err());
        // Stand-in for the separate trusted merge-proof reader, not a CLI action.
        state.tasks.get_mut("T-1").unwrap().state = TaskState::Merged;
        assert!(check_g0(&baseline(), &state, "T-2").is_ok());
        state.tasks.get_mut("T-1").unwrap().needs_review = true;
        assert!(check_g0(&baseline(), &state, "T-2").is_err());
        assert!(serde_json::from_value::<TaskEvent>(serde_json::json!({
            "kind": "done", "task_id": "T-1"
        }))
        .is_err());
        assert!(serde_json::from_value::<TaskDraft>(serde_json::json!({
            "id": "T-9", "title": "bad", "module": "A-engine",
            "allowed_paths": ["src/engine/**"], "state": "merged"
        }))
        .is_err());
    }

    #[test]
    fn claim_requires_full_live_fences_and_primary_epoch_is_by_module_name() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_str().unwrap();
        let mut draft = input("T-1");
        draft.touches.push("A-api".into());
        let mut state = added(draft);
        let run = binding("T-1", "R-1", path);
        assert!(
            prepare_claim(&baseline(), &state, "T-1", run.clone(), &leases("R-1"), 10).is_err()
        );
        let mut grants = vec![Lease {
            name: "module:A-api".into(),
            holder: "R-1".into(),
            epoch: 6,
            expires_at: 1_000,
        }];
        grants.extend(leases("R-1"));
        assert!(prepare_claim(&baseline(), &state, "T-1", run.clone(), &grants, 1_000).is_err());
        let event = prepare_claim(&baseline(), &state, "T-1", run, &grants, 10).unwrap();
        state.apply(&event).unwrap();
        assert_eq!(state.tasks["T-1"].state, TaskState::Running);
        assert_eq!(state.runs["R-1"].primary_fence().unwrap().epoch, 7);
        assert_eq!(
            serde_json::to_value(&state.runs["R-1"]).unwrap()["cost_usd"],
            serde_json::Value::Null
        );
        assert!(state.apply(&event).is_err());
    }

    #[test]
    fn run_binding_checks_branch_and_one_active_run_per_host_worktree() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_str().unwrap();
        let mut state = added(input("T-1"));
        let add = prepare_add(&baseline(), &state, input("T-2"), TaskOrigin::Cli).unwrap();
        state.apply(&add).unwrap();
        let mut run = binding("T-1", "R-1", path);
        run.branch = "main".into();
        assert!(prepare_claim(&baseline(), &state, "T-1", run, &leases("R-1"), 1).is_err());
        let claim = prepare_claim(
            &baseline(),
            &state,
            "T-1",
            binding("T-1", "R-1", path),
            &leases("R-1"),
            1,
        )
        .unwrap();
        state.apply(&claim).unwrap();
        assert!(prepare_claim(
            &baseline(),
            &state,
            "T-2",
            binding("T-2", "R-2", path),
            &leases("R-2"),
            1
        )
        .unwrap_err()
        .contains("工作树已有"));
        let release = prepare_release(&state, "R-1", ReleaseReason::Expired).unwrap();
        state.apply(&release).unwrap();
        assert_eq!(state.tasks["T-1"].active_run, None);
        assert_eq!(state.tasks["T-1"].state, TaskState::Ready);
        assert!(prepare_claim(
            &baseline(),
            &state,
            "T-2",
            binding("T-2", "R-2", path),
            &leases("R-2"),
            1
        )
        .is_ok());
    }

    #[test]
    fn paused_or_changed_baseline_tasks_cannot_be_claimed() {
        let mut state = added(input("T-1"));
        state
            .apply(&TaskEvent::Paused {
                task_id: "T-1".into(),
            })
            .unwrap();
        assert!(check_g0(&baseline(), &state, "T-1").is_err());
        state
            .apply(&TaskEvent::Resumed {
                task_id: "T-1".into(),
            })
            .unwrap();
        assert!(check_g0(&baseline(), &state, "T-1").is_ok());
        state.paused_all = true;
        assert!(check_g0(&baseline(), &state, "T-1").is_err());
        state.paused_all = false;
        state
            .apply(&TaskEvent::NeedsReview {
                task_id: "T-1".into(),
            })
            .unwrap();
        assert!(check_g0(&baseline(), &state, "T-1")
            .unwrap_err()
            .contains("复核"));
    }

    #[test]
    fn invalid_reducer_event_is_atomic_and_cannot_assign_a_known_cost() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = added(input("T-1"));
        let mut event = prepare_claim(
            &baseline(),
            &state,
            "T-1",
            binding("T-1", "R-1", temp.path().to_str().unwrap()),
            &leases("R-1"),
            1,
        )
        .unwrap();
        let TaskEvent::Claimed { run } = &mut event else {
            panic!()
        };
        run.cost_usd = Some(0.0);
        let before = state.clone();
        assert!(state.apply(&event).is_err());
        assert_eq!(state, before);
        let mut created = prepare_add(&baseline(), &state, input("T-2"), TaskOrigin::Cli).unwrap();
        let TaskEvent::Created { task } = &mut created else {
            panic!()
        };
        task.state = TaskState::Merged;
        assert!(state.apply(&created).is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn card_fields_are_omitted_when_empty_and_brief_maps_acceptance() {
        let old = serde_json::to_value(input("T-1")).unwrap();
        assert_eq!(old.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>(),
            ["acceptance", "allowed_paths", "decisions", "depends_on", "id", "module", "priority", "title", "touches"]
                .into_iter().map(String::from).collect());
        let mut draft = input("T-1");
        draft.why = Some("why".into());
        draft.checkpoints = vec![Checkpoint { text: "stop".into(), due: Some("2026-10-25T10:00:00Z".into()) }];
        draft.clarify = true;
        let value = serde_json::to_value(&draft).unwrap();
        assert_eq!(value["clarify"], true);
        assert_eq!(serde_json::from_value::<TaskDraft>(value).unwrap(), draft);
        let brief = draft.brief();
        assert_eq!(brief.done_when, draft.acceptance);
        assert_eq!(brief.why.as_deref(), Some("why"));
        draft.limits = Some(" ".into());
        assert!(prepare_add(&baseline(), &TaskProjection::default(), draft, TaskOrigin::Cli).is_err());
    }

    #[test]
    fn clarifying_task_stays_proposed_until_finalized() {
        let temp = tempfile::tempdir().unwrap();
        let mut draft = input("T-1");
        draft.clarify = true;
        draft.acceptance.clear();
        let mut state = added(draft);
        assert_eq!(state.tasks["T-1"].state, TaskState::Proposed);
        assert!(check_g0(&baseline(), &state, "T-1").unwrap_err().contains("澄清"));
        let claim = TaskEvent::Claimed { run: Run {
            binding: binding("T-1", "R-1", temp.path().to_str().unwrap()), task: "T-1".into(),
            module: "A-engine".into(), baseline: "towermd/B0".into(),
            fences: leases("R-1").iter().map(Lease::fence).collect(), state: RunState::Active, cost_usd: None,
        } };
        assert!(state.apply(&claim).is_err());
        let mut created = state.tasks["T-1"].clone();
        created.input.id = "T-2".into();
        created.state = TaskState::Ready;
        assert!(state.apply(&TaskEvent::Created { task: created }).is_err());
        assert!(state.finalize("T-1", &Brief::default()).unwrap_err().contains("G0"));
        let brief = Brief { done_when: vec!["passes".into()], why: Some("w".into()), ..Brief::default() };
        state.finalize("T-1", &brief).unwrap();
        let task = &state.tasks["T-1"];
        assert_eq!((task.state, task.input.clarify), (TaskState::Ready, false));
        assert_eq!(task.input.brief(), brief);
        assert!(check_g0(&baseline(), &state, "T-1").is_ok());
        assert!(state.finalize("T-1", &brief).is_err(), "ready tasks are not re-finalized");
        state.apply(&claim).unwrap();
    }

    #[test]
    fn finalize_keeps_tasks_with_unseen_decisions_proposed() {
        let mut draft = input("T-1");
        draft.decisions.push("D-api".into());
        draft.clarify = true;
        let mut state = added(draft);
        state.finalize("T-1", &Brief { done_when: vec!["ok".into()], ..Brief::default() }).unwrap();
        assert_eq!(state.tasks["T-1"].state, TaskState::Proposed);
        assert!(check_g0(&baseline(), &state, "T-1").is_ok());
    }

    #[test]
    fn only_merged_tasks_reopen_and_reopened_tasks_can_be_claimed_again() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_str().unwrap();
        let mut state = added(input("T-1"));
        assert!(state.reopen("T-1").is_err());
        let claim = prepare_claim(&baseline(), &state, "T-1", binding("T-1", "R-1", path), &leases("R-1"), 1).unwrap();
        state.apply(&claim).unwrap();
        assert!(state.reopen("T-1").is_err());
        // Stand-in for the trusted merge-proof reader.
        let task = state.tasks.get_mut("T-1").unwrap();
        task.state = TaskState::Merged;
        task.active_run = None;
        state.runs.get_mut("R-1").unwrap().state = RunState::Released;
        state.reopen("T-1").unwrap();
        assert_eq!(state.tasks["T-1"].state, TaskState::Ready);
        let again = prepare_claim(&baseline(), &state, "T-1", binding("T-1", "R-2", path), &leases("R-2"), 1).unwrap();
        state.apply(&again).unwrap();
        assert_eq!(state.tasks["T-1"].active_run.as_deref(), Some("R-2"));
    }

    #[test]
    fn baseline_constructor_rejects_incomplete_publication_metadata() {
        let source = baseline();
        let input = PublishedBaseline {
            tag: source.tag,
            commit: String::new(),
            approval_eid: source.approval_eid,
            content_hash: source.content_hash,
            modules: source.modules,
            decisions: source.decisions,
        };
        assert!(VerifiedBaseline::from_published(input).is_err());
    }
}
