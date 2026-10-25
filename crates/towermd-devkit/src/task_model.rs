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
    /// FR-58 `risk: high`: plan and code need a person's review.
    pub high_risk: bool,
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
    /// M3: the runner saw the run fail (10.2). Releasing the run makes the
    /// task ready again; the third failure holds it instead.
    Failed,
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
    /// Hard-gate failures since the last human decision (FR-57). The fields
    /// below are omitted when empty, so v0.1.1 projections keep their shape.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gate_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupt: Option<Interrupt>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// FR-57: a hard gate gets two automatic fixes after its first failure;
/// the next failure interrupts the task for a person.
pub const AUTO_FIX_MAX: u32 = 2;

/// Why a task waits for a person (spec 12.1). Questions and reviews are live:
/// they end when the person answers. The others stay until a person decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptKind {
    Question,
    Review,
    GateFailed,
    /// G0 found the card short of information; the task is not dispatched.
    MissingInfo,
    /// G2 stopped the change at finish (out of scope or protected paths).
    Forbidden,
    /// The runner saw a crash or the third failure (12.1 错误).
    RunError,
    /// The run reached its budget (12.1 预算).
    Budget,
}
impl InterruptKind {
    pub fn live(self) -> bool {
        matches!(self, Self::Question | Self::Review)
    }
    /// The alert a person sees for a sticky interrupt. The runner's
    /// interrupts keep the alert the run record raises with them.
    pub fn alert_reason(self) -> Option<&'static str> {
        match self {
            Self::GateFailed => Some(GATE_EXHAUSTED),
            Self::MissingInfo => Some(INFO_MISSING),
            Self::Forbidden => Some(FORBIDDEN),
            Self::Question | Self::Review | Self::RunError | Self::Budget => None,
        }
    }
}

/// Alert reasons for the interrupts this module opens.
pub const GATE_EXHAUSTED: &str = "gate_exhausted";
pub const INFO_MISSING: &str = "task_info_missing";
pub const FORBIDDEN: &str = "task_forbidden";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interrupt {
    pub kind: InterruptKind,
    pub reason: String,
    /// The event that opened it.
    pub opened: String,
    /// The blocking gate, for a gate failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// Where the task returns once the interrupt ends with the run intact.
    pub resume: TaskState,
}

/// A person's decision on a gate failure or missing information (12.1).
/// There is no "pass": only checks, reviews and waivers let work through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptChoice {
    /// Let the delegate fix it again; the failure count starts over.
    Retry,
    /// Rewrite the card (e.g. its acceptance): the run ends, the task goes
    /// back to proposed for the person to finalize.
    Rewrite,
    /// End the run and put the task back to ready for someone else.
    Reassign,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterruptResolved {
    pub choice: InterruptChoice,
    /// The interrupt decided, by its opening event.
    pub interrupt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// G0 failed for missing information (D6): a question to the task's creator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InfoRequested {
    pub reason: String,
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
    /// A module or touch is `risk: high` in the claimed baseline (FR-58).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub high_risk: bool,
    /// M3: the scheduler dispatched this run to a headless delegate; only
    /// the scheduler and integrator act on it, never the CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch: Option<Dispatch>,
}

/// Which delegate the scheduler launches for a run (FR-30).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dispatch {
    /// Adapter ID, such as `claude-code`.
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
impl Run {
    /// The CLI drives a claimed run; the scheduler drives a dispatched one.
    pub fn driver(&self) -> &'static str {
        if self.dispatch.is_some() { crate::schedule::SCHEDULER } else { "process:towermd-cli" }
    }
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
    /// `towermd task edit`: the whole draft again, before anyone claims it.
    Edited {
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

/// Whether a card still fits a baseline: its modules are in it and, for a
/// command-line task, its paths stay within theirs (J4 step 4).
pub fn fits(baseline: &VerifiedBaseline, input: &TaskDraft, origin: TaskOrigin) -> Result<(), String> {
    validate_draft(input)?;
    validate_scope(baseline, input, origin)
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
    Ok(TaskEvent::Created { task: drafted(baseline, projection, input, origin) })
}

/// `towermd task edit`: a delegate may rewrite a task added from the command
/// line as long as nobody claimed it and it needs no review. A task a person
/// created in the App is the person's to change. Readiness is recomputed
/// exactly as on creation.
pub fn prepare_edit(
    baseline: &VerifiedBaseline,
    projection: &TaskProjection,
    input: TaskDraft,
) -> Result<TaskEvent, String> {
    validate_draft(&input)?;
    let old = projection.tasks.get(&input.id).ok_or("任务不存在")?;
    if old.origin != TaskOrigin::Cli {
        return Err("人在 App 中建的任务只能由人修改".into());
    }
    if !editable(old) {
        return Err("只有尚未领取、无需复核的任务能修改".into());
    }
    validate_scope(baseline, &input, TaskOrigin::Cli)?;
    for dependency in &input.depends_on {
        if !projection.tasks.contains_key(dependency) {
            return Err(format!("依赖任务不存在：{dependency}"));
        }
    }
    if depends_on_itself(projection, &input) {
        return Err("任务依赖不能成环".into());
    }
    Ok(TaskEvent::Edited { task: drafted(baseline, projection, input, TaskOrigin::Cli) })
}

/// Nobody claimed it, it is not merged and it needs no review.
pub fn editable(task: &Task) -> bool {
    matches!(task.state, TaskState::Proposed | TaskState::Ready) && task.active_run.is_none() && !task.needs_review
}

/// Whether the draft reaches itself through its dependencies.
fn depends_on_itself(projection: &TaskProjection, input: &TaskDraft) -> bool {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&String> = input.depends_on.iter().collect();
    while let Some(id) = stack.pop() {
        if *id == input.id {
            return true;
        }
        if seen.insert(id) {
            stack.extend(projection.tasks.get(id).into_iter().flat_map(|task| &task.input.depends_on));
        }
    }
    false
}

/// A new or rewritten task: ready only when nothing is left to clarify or wait for.
fn drafted(baseline: &VerifiedBaseline, projection: &TaskProjection, input: TaskDraft, origin: TaskOrigin) -> Task {
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
    Task {
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
        gate_failures: 0,
        interrupt: None,
    }
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
    if task.state == TaskState::Held {
        return Err("任务中断中，须由人在 App 处理后才能再领取".into());
    }
    if !matches!(task.state, TaskState::Ready | TaskState::Proposed) || task.active_run.is_some() {
        return Err("任务已有运行或不能领取".into());
    }
    validate_draft(&task.input)?;
    validate_scope(baseline, &task.input, task.origin)?;
    if task.input.clarify {
        return Err("G0：委托卡尚在澄清，须由人定稿后才能领取".into());
    }
    if let Some(missing) = missing_info(baseline, &task.input) {
        return Err(missing);
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

/// What G0 finds missing from the card itself (D6): only its creator can
/// supply it, so the claim becomes a question to them as well as an error.
pub fn missing_info(baseline: &VerifiedBaseline, input: &TaskDraft) -> Option<String> {
    if input.allowed_paths.is_empty() || input.acceptance.is_empty() {
        return Some("G0：缺少允许路径或验收标准".into());
    }
    input.decisions.iter().find(|id| !baseline.decisions.contains(*id))
        .map(|id| format!("G0：权威 main 缺少相关决策 {id}"))
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
    let high_risk = modules(&task.input)
        .into_iter()
        .any(|id| baseline.modules.get(id).is_some_and(|module| module.high_risk));
    let run = Run {
        binding,
        task: task_id.into(),
        module: task.input.module.clone(),
        baseline: baseline.tag.clone(),
        fences: leases.iter().map(Lease::fence).collect(),
        state: RunState::Active,
        cost_usd: None,
        high_risk,
        dispatch: None,
    };
    validate_fences(&task.input, &run)?;
    Ok(TaskEvent::Claimed { run })
}

/// FR-14 in M3: the scheduler claims before it launches a delegate, under
/// the same G0, binding and lease checks as a CLI claim.
pub fn prepare_dispatch(
    baseline: &VerifiedBaseline,
    projection: &TaskProjection,
    task_id: &str,
    binding: RunBinding,
    leases: &[Lease],
    now_ms: u64,
    dispatch: Dispatch,
) -> Result<TaskEvent, String> {
    let TaskEvent::Claimed { mut run } = prepare_claim(baseline, projection, task_id, binding, leases, now_ms)? else {
        unreachable!("prepare_claim only claims")
    };
    run.dispatch = Some(dispatch);
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
                    || task.gate_failures != 0
                    || task.interrupt.is_some()
                    || !matches!(task.state, TaskState::Proposed | TaskState::Ready)
                    || (task.input.clarify && task.state != TaskState::Proposed)
                {
                    return Err("非法任务创建事件".into());
                }
                self.tasks.insert(task.input.id.clone(), task.clone());
            }
            TaskEvent::Edited { task } => {
                validate_draft(&task.input)?;
                let old = self.tasks.get(&task.input.id).ok_or("修改事件缺少任务")?;
                if old.origin != TaskOrigin::Cli
                    || task.origin != TaskOrigin::Cli
                    || !editable(old)
                    || !editable(task)
                    || (task.input.clarify && task.state != TaskState::Proposed)
                    || task.input.depends_on.iter().any(|id| !self.tasks.contains_key(id))
                    || depends_on_itself(self, &task.input)
                {
                    return Err("非法任务修改事件".into());
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
                // A claim passed G0, which answers a missing-information request.
                task.interrupt = None;
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
                // A gate failure outlives the run: the task stays held for a person.
                if task.interrupt.as_ref().is_some_and(|interrupt| interrupt.kind.live()) {
                    task.interrupt = None;
                }
                // 15.5: a paused task stays paused when its run ends; only the person resumes it.
                task.state = if task.state == TaskState::Paused { TaskState::Paused }
                    else if task.interrupt.is_some() { TaskState::Held } else { TaskState::Ready };
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
                task.state = if task.interrupt.as_ref().is_some_and(|interrupt| interrupt.kind != InterruptKind::MissingInfo) {
                    TaskState::Held
                } else if task.active_run.is_some() {
                    TaskState::Running
                } else {
                    TaskState::Ready
                };
            }
        }
        Ok(())
    }

    /// A hard gate failed on the active run (FR-57). Within the auto-fix
    /// budget the delegate fixes it; past it the task is held for a person.
    pub fn gate_failed(&mut self, task_id: &str, gate: &str, reason: &str, eid: &str) -> Result<(), String> {
        let task = self.tasks.get_mut(task_id).ok_or("任务不存在")?;
        if task.active_run.is_none() || !matches!(task.state, TaskState::Running | TaskState::Verifying) {
            return Ok(());
        }
        task.gate_failures += 1;
        if task.gate_failures > AUTO_FIX_MAX {
            task.interrupt = Some(Interrupt {
                kind: InterruptKind::GateFailed,
                reason: format!("{gate} 累计 {} 次未通过：{reason}", task.gate_failures),
                opened: eid.into(),
                gate: Some(gate.into()),
                resume: task.state,
            });
            task.state = TaskState::Held;
        }
        Ok(())
    }

    /// Hold the current run's task for a person: G2 at finish, or the
    /// runner's error and budget stops. Only a working task is held.
    pub fn hold(&mut self, task_id: &str, run_id: &str, kind: InterruptKind, gate: Option<&str>, reason: &str, eid: &str) -> bool {
        let Some(task) = self.tasks.get_mut(task_id) else { return false };
        if kind.live() || task.interrupt.is_some() || task.active_run.as_deref() != Some(run_id)
            || !matches!(task.state, TaskState::Running | TaskState::Verifying)
        {
            return false;
        }
        task.interrupt = Some(Interrupt { kind, reason: reason.into(), opened: eid.into(), gate: gate.map(Into::into),
            resume: TaskState::Running });
        task.state = TaskState::Held;
        true
    }

    /// G0 found the card short of information: ask its creator (D6). The
    /// task is not dispatched and keeps its state; one request at a time.
    pub fn request_info(&mut self, task_id: &str, reason: &str, eid: &str) -> Result<(), String> {
        let task = self.tasks.get_mut(task_id).ok_or("任务不存在")?;
        if !matches!(task.state, TaskState::Proposed | TaskState::Ready) || task.active_run.is_some() {
            return Err("只有尚未委托的任务能请求补充信息".into());
        }
        if task.interrupt.is_some() {
            return Err("任务已有未处理的中断".into());
        }
        task.interrupt = Some(Interrupt {
            kind: InterruptKind::MissingInfo,
            reason: reason.into(),
            opened: eid.into(),
            gate: None,
            resume: task.state,
        });
        Ok(())
    }

    /// Open questions and reviews hold a working task; once none is open the
    /// task goes back to where it was. Returns whether anything changed.
    pub fn settle(&mut self, task_id: &str, question: bool, review: bool, eid: &str) -> bool {
        let Some(task) = self.tasks.get_mut(task_id) else { return false };
        let open = question || review;
        match &task.interrupt {
            None if open && task.active_run.is_some() && matches!(task.state, TaskState::Running | TaskState::Verifying) => {
                let (kind, reason) = if review {
                    (InterruptKind::Review, "等待人评审")
                } else {
                    (InterruptKind::Question, "等待人回答提问")
                };
                task.interrupt = Some(Interrupt { kind, reason: reason.into(), opened: eid.into(), gate: None, resume: task.state });
                task.state = TaskState::Held;
                true
            }
            Some(interrupt) if interrupt.kind.live() && !open => {
                let resume = interrupt.resume;
                task.interrupt = None;
                if task.state == TaskState::Held {
                    task.state = if task.active_run.is_some() { resume } else { TaskState::Ready };
                }
                true
            }
            _ => false,
        }
    }

    /// A person decided on a gate failure or missing information. Returns
    /// the active run the decision ends, whose leases the caller releases.
    pub fn resolve_interrupt(&mut self, task_id: &str, input: &InterruptResolved) -> Result<Option<String>, String> {
        let task = self.tasks.get(task_id).ok_or("任务不存在")?;
        let interrupt = task.interrupt.as_ref().ok_or("任务没有待处理的中断")?;
        if interrupt.opened != input.interrupt {
            return Err("中断已变化，请刷新后再处理".into());
        }
        if interrupt.kind.live() {
            return Err("提问与评审由回答或评审结论结束".into());
        }
        let resume = interrupt.resume;
        let ended = match input.choice {
            InterruptChoice::Retry => None,
            InterruptChoice::Rewrite | InterruptChoice::Reassign => task.active_run.clone(),
        };
        if let Some(run) = &ended {
            self.runs.get_mut(run).ok_or("中断任务的运行不存在")?.state = RunState::Released;
        }
        let task = self.tasks.get_mut(task_id).unwrap();
        task.interrupt = None;
        task.gate_failures = 0;
        if ended.is_some() {
            task.active_run = None;
        }
        task.state = match input.choice {
            InterruptChoice::Retry if task.active_run.is_some() => resume,
            InterruptChoice::Retry if task.state == TaskState::Held => TaskState::Ready,
            InterruptChoice::Retry => task.state,
            InterruptChoice::Rewrite => TaskState::Proposed,
            InterruptChoice::Reassign => TaskState::Ready,
        };
        Ok(ended)
    }

    /// A waiver let the blocking gate through: the gate failure is over.
    pub fn waived(&mut self, task_id: &str, gate: &str) -> bool {
        let Some(task) = self.tasks.get_mut(task_id) else { return false };
        if !task.interrupt.as_ref().is_some_and(|interrupt| interrupt.kind == InterruptKind::GateFailed && interrupt.gate.as_deref() == Some(gate)) {
            return false;
        }
        let resume = task.interrupt.take().unwrap().resume;
        task.gate_failures = 0;
        if task.state == TaskState::Held {
            task.state = if task.active_run.is_some() { resume } else { TaskState::Ready };
        }
        true
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
        let answered = task.interrupt.as_ref().is_some_and(|interrupt| interrupt.kind == InterruptKind::MissingInfo);
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
        // The finalized card answers a missing-information request.
        if answered {
            task.interrupt = None;
        }
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
                        high_risk: false,
                    },
                ),
                (
                    "A-api".into(),
                    ModuleScope {
                        paths: vec!["src/api/**".into()],
                        high_risk: true,
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
    fn edit_rewrites_only_unclaimed_cli_tasks_and_rejects_cycles() {
        let mut state = added(input("T-1"));
        let mut second = input("T-2");
        second.depends_on = vec!["T-1".into()];
        let event = prepare_add(&baseline(), &state, second, TaskOrigin::Cli).unwrap();
        state.apply(&event).unwrap();

        // Readiness is recomputed exactly as on creation.
        let mut draft = input("T-1");
        draft.title = "Rewritten".into();
        draft.clarify = true;
        let event = prepare_edit(&baseline(), &state, draft).unwrap();
        state.apply(&event).unwrap();
        assert_eq!((state.tasks["T-1"].input.title.as_str(), &state.tasks["T-1"].state), ("Rewritten", &TaskState::Proposed));

        let mut cycle = input("T-1");
        cycle.depends_on = vec!["T-2".into()];
        assert!(prepare_edit(&baseline(), &state, cycle).unwrap_err().contains("成环"));
        let mut own = input("T-1");
        own.depends_on = vec!["T-1".into()];
        assert!(prepare_edit(&baseline(), &state, own).is_err());
        assert!(prepare_edit(&baseline(), &state, input("T-missing")).is_err());

        // Needing review (or being claimed) ends the delegate's say.
        state.tasks.get_mut("T-1").unwrap().needs_review = true;
        assert!(prepare_edit(&baseline(), &state, input("T-1")).is_err());

        // A person's task stays the person's, and a forged event is refused.
        let event = prepare_add(&baseline(), &state, input("T-3"), TaskOrigin::AppHuman).unwrap();
        state.apply(&event).unwrap();
        assert!(prepare_edit(&baseline(), &state, input("T-3")).unwrap_err().contains("人"));
        let TaskEvent::Created { task } = event else { panic!() };
        assert!(state.apply(&TaskEvent::Edited { task }).is_err());
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
    fn a_paused_task_stays_paused_when_its_run_ends_until_the_person_resumes_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_str().unwrap();
        let mut state = added(input("T-1"));
        let claim = prepare_claim(&baseline(), &state, "T-1", binding("T-1", "R-1", path), &leases("R-1"), 1).unwrap();
        state.apply(&claim).unwrap();
        state.apply(&TaskEvent::Paused { task_id: "T-1".into() }).unwrap();
        state.apply(&TaskEvent::Released { run_id: "R-1".into(), reason: ReleaseReason::Expired }).unwrap();
        assert_eq!(state.tasks["T-1"].state, TaskState::Paused, "运行结束不解除人的暂停");
        assert!(check_g0(&baseline(), &state, "T-1").is_err());
        state.apply(&TaskEvent::Resumed { task_id: "T-1".into() }).unwrap();
        assert_eq!(state.tasks["T-1"].state, TaskState::Ready);
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
            fences: leases("R-1").iter().map(Lease::fence).collect(), state: RunState::Active, cost_usd: None, high_risk: false, dispatch: None,
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
    fn interrupts_survive_pause_and_release_and_are_never_created_or_counted_idle() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_str().unwrap();
        let mut state = added(input("T-1"));
        let mut forged = state.tasks["T-1"].clone();
        forged.input.id = "T-2".into();
        forged.gate_failures = 1;
        assert!(state.apply(&TaskEvent::Created { task: forged }).is_err(), "新任务不带计数或中断");
        state.gate_failed("T-1", "lint", "x", "e-0").unwrap();
        assert_eq!(state.tasks["T-1"].gate_failures, 0, "未委托的任务不计门禁失败");
        let claim = prepare_claim(&baseline(), &state, "T-1", binding("T-1", "R-1", path), &leases("R-1"), 1).unwrap();
        state.apply(&claim).unwrap();
        for n in 1..=AUTO_FIX_MAX + 1 {
            state.gate_failed("T-1", "lint", "x", &format!("e-{n}")).unwrap();
        }
        assert_eq!(state.tasks["T-1"].state, TaskState::Held);
        assert!(check_g0(&baseline(), &state, "T-1").is_err());
        // Paused and resumed, a held task is still held.
        state.apply(&TaskEvent::Paused { task_id: "T-1".into() }).unwrap();
        state.apply(&TaskEvent::Resumed { task_id: "T-1".into() }).unwrap();
        assert_eq!(state.tasks["T-1"].state, TaskState::Held);
        // A live interrupt ends with the run; a gate failure does not.
        assert!(!state.settle("T-1", true, false, "e-q"), "已有中断时提问不覆盖它");
        state.apply(&TaskEvent::Released { run_id: "R-1".into(), reason: ReleaseReason::Expired }).unwrap();
        assert_eq!(state.tasks["T-1"].state, TaskState::Held);
        assert!(!state.hold("T-1", "R-1", InterruptKind::Forbidden, None, "x", "e-x"), "没有活动运行不能再中断");
        let waived = state.waived("T-1", "lint");
        assert!(waived && state.tasks["T-1"].state == TaskState::Ready, "豁免后原运行已结束 → 就绪");
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
