//! FR-67 the controller's action plans (spec 12.5, J4 step 4). A person's
//! global or module input goes to the controller, which turns it into a plan
//! of task actions; a new baseline that leaves tasks to review gets one too.
//! Every plan is previewed and runs only when a person confirms it in the
//! App: nothing runs on a timeout, and a person not looking is no consent.
//! The person may take over instead, and then nothing runs.
//!
//! - One event kind, `plan.changed`, at L4 without fences; `obj` is the plan.
//!   `requested` is the person's (or, for a new baseline, the controller's),
//!   `previewed` and `failed` are the controller's, `confirmed` and
//!   `taken_over` are the person's.
//! - Steps are a closed set of fast-layer task actions. Slow-layer parts
//!   (modules, interfaces, acceptance, the baseline) are draft text only and
//!   never run: they go through a proposal as ever.
//! - Confirming applies every step at once, checked again against the
//!   projection then; one step that no longer holds rejects the confirmation.
//!   Nothing here marks work done.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use towermd_core::hub::Event;
use towermd_core::hub_protocol::NewEvent;
use towermd_core::project_state::insert_unique;
use towermd_core::question_model::CONTROLLER;

use crate::dev_state;
use crate::task_model::{self, Task, TaskDraft, TaskEvent, TaskOrigin, TaskProjection, TaskState, VerifiedBaseline};

pub const CHANGED: &str = "plan.changed";
pub const GROUP: &str = "ControllerPlan";
/// The alert a previewed or failed plan leaves for a person.
pub const ALERT: &str = "controller_plan";
pub const STEPS_MAX: usize = 30;
pub const ITEMS_MAX: usize = 10;
pub const TEXT_CHARS: usize = 2000;
const LINE_CHARS: usize = 500;

/// What the input was about: the project, one module, or a new baseline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Global,
    Module { module: String },
    /// J4 step 4: regenerate tasks after baseline `baseline` took effect.
    Baseline { baseline: String },
}

/// One fast-layer action a confirmed plan takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// A new task, as the person would add it in the App.
    Create { task: Task },
    Priority { task: String, priority: i32 },
    Pause { task: String },
    Resume { task: String },
    /// J4: the task still fits the newest baseline; bind it there and clear
    /// its review mark. Its old branch stays as a reference.
    Rebase { task: String, baseline: String },
}

impl Step {
    pub fn task(&self) -> &str {
        match self {
            Step::Create { task } => &task.input.id,
            Step::Priority { task, .. } | Step::Pause { task } | Step::Resume { task } | Step::Rebase { task, .. } => task,
        }
    }
}

/// A slow-layer change the controller would suggest: text for a proposal, never run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub target: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanEvent {
    Requested { scope: Scope, text: String },
    Previewed {
        summary: String,
        steps: Vec<Step>,
        #[serde(default)]
        drafts: Vec<Draft>,
        #[serde(default)]
        notes: Vec<String>,
        /// `<harness>/<version>` when a harness was consulted.
        #[serde(default)]
        agent: Option<String>,
        /// Unknown stays null, never 0.
        #[serde(default)]
        cost_usd: Option<f64>,
    },
    Failed {
        reason: String,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        cost_usd: Option<f64>,
    },
    Confirmed {
        #[serde(default)]
        note: Option<String>,
    },
    TakenOver {
        #[serde(default)]
        note: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Consulting,
    Previewed,
    Failed,
    Confirmed,
    TakenOver,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub scope: Scope,
    pub text: String,
    pub requested_by: String,
    pub requested_at: String,
    pub status: Status,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub drafts: Vec<Draft>,
    #[serde(default)]
    pub notes: Vec<String>,
    /// Why the controller could not plan.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// The open alert, by event.
    #[serde(default)]
    pub alert: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub resolved_at: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl Plan {
    /// The tasks the plan touches, for the preview.
    pub fn tasks(&self) -> BTreeSet<String> {
        self.steps.iter().map(|step| step.task().to_owned()).collect()
    }
}

pub fn plans(state: &Value) -> Result<std::collections::BTreeMap<String, Plan>, String> {
    state.get(GROUP).map_or(Ok(Default::default()), |group| serde_json::from_value(group.clone()).map_err(|e| e.to_string()))
}

/// The newest published baseline in the projection, by number.
pub fn latest_baseline(state: &Value) -> Option<String> {
    state.get("Baseline")?.as_object()?.keys()
        .filter_map(|id| Some((id.strip_prefix('B')?.parse::<u64>().ok()?, id)))
        .max_by_key(|(number, _)| *number).map(|(_, id)| id.clone())
}

pub fn event(host: &str, actor: &str, plan: &str, change: &PlanEvent) -> Result<NewEvent, String> {
    Ok(NewEvent { eid: uuid::Uuid::now_v7().to_string(), host: host.into(), actor: actor.into(), kind: CHANGED.into(),
        layer: "L4".into(), obj: plan.into(), data: serde_json::to_value(change).map_err(|e| e.to_string())?, guards: Vec::new() })
}

fn filled(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= limit
}

fn valid_plan_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 160 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn valid_cost(cost: Option<f64>) -> bool {
    cost.is_none_or(|cost| cost.is_finite() && cost >= 0.0)
}

/// Every step of `plan` applied to `state`, or why one does not hold.
fn execute(state: &mut Value, scope: &Scope, steps: &[Step]) -> Result<(), String> {
    let latest = latest_baseline(state);
    let mut projection = dev_state::tasks(state)?;
    let mut seen = BTreeSet::new();
    for step in steps {
        let id = step.task();
        if !seen.insert(id.to_owned()) { return Err(format!("计划对任务 {id} 有多个动作")) }
        let module = match step {
            Step::Create { task } => task.input.module.clone(),
            _ => projection.tasks.get(id).ok_or_else(|| format!("计划中的任务不存在：{id}"))?.input.module.clone(),
        };
        match scope {
            Scope::Module { module: only } if &module != only => return Err(format!("模块计划只能动模块 {only} 的任务：{id}")),
            Scope::Baseline { .. } if !matches!(step, Step::Rebase { .. }) => return Err("新基线的计划只能把任务绑到新基线".into()),
            _ => {}
        }
        match step {
            Step::Create { task } => {
                let baseline = task.baseline.strip_prefix("towermd/").ok_or("任务基线无效")?;
                if task.origin != TaskOrigin::AppHuman || latest.as_deref() != Some(baseline) {
                    return Err(format!("计划新增的任务 {id} 须是 App 人工任务并绑定最新基线"));
                }
                if let Some(missing) = task.input.depends_on.iter().find(|dep| !projection.tasks.contains_key(*dep)) {
                    return Err(format!("依赖任务不存在：{missing}"));
                }
                projection.apply(&TaskEvent::Created { task: task.clone() })?;
            }
            Step::Priority { priority, .. } => {
                let task = projection.tasks.get_mut(id).ok_or("任务不存在")?;
                if task.state == TaskState::Merged { return Err(format!("任务 {id} 已合并，不再调整优先级")) }
                task.input.priority = *priority;
            }
            Step::Pause { .. } => projection.apply(&TaskEvent::Paused { task_id: id.to_owned() })?,
            Step::Resume { .. } => projection.apply(&TaskEvent::Resumed { task_id: id.to_owned() })?,
            Step::Rebase { baseline, .. } => {
                if latest.as_deref() != Some(baseline.as_str()) || matches!(scope, Scope::Baseline { baseline: b } if b != baseline) {
                    return Err(format!("任务只能绑到最新基线：{baseline}"));
                }
                let task = projection.tasks.get_mut(id).ok_or("任务不存在")?;
                if !task.needs_review || task.state == TaskState::Merged {
                    return Err(format!("任务 {id} 不需要按新基线复核"));
                }
                let tag = format!("towermd/{baseline}");
                task.baseline = tag.clone();
                task.needs_review = false;
                for run in projection.runs.values_mut().filter(|run| run.task == id) { run.baseline = tag.clone(); }
            }
        }
    }
    dev_state::write_tasks(state, &projection)
}

fn alert(state: &mut Value, event: &Event, plan: &Plan, title: &str) -> Result<(), String> {
    insert_unique(state, "Alert", &event.eid, json!({
        "id": event.eid, "event": event.eid, "at": event.t, "title": title, "resolved": false,
        "data": {"reason": ALERT, "detail": {"plan": plan.id, "scope": plan.scope, "tasks": plan.tasks(),
            "summary": plan.summary.as_deref().or(plan.reason.as_deref())}},
    }))
}

fn store(state: &mut Value, plan: &Plan) -> Result<(), String> {
    let group = state.as_object_mut().ok_or("项目投影格式无效")?.entry(GROUP).or_insert_with(|| json!({}));
    group[&plan.id] = serde_json::to_value(plan).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != CHANGED { return Ok(false) }
    let change: PlanEvent = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if event.layer != "L4" || !event.guards.is_empty() || !valid_plan_id(&event.obj) {
        return Err("总控计划须在 L4 记录且不带租约".into());
    }
    let human = event.actor.starts_with("human:");
    let controller = event.actor == CONTROLLER;
    let mut plans = plans(state)?;
    if let PlanEvent::Requested { scope, text } = change {
        let signer = match &scope { Scope::Baseline { .. } => controller, _ => human };
        if !signer { return Err("全局与模块的计划请求只能由人在 App 发出；新基线的计划由总控发起".into()) }
        if plans.contains_key(&event.obj) { return Err("该计划已存在".into()) }
        if !filled(&text, TEXT_CHARS) { return Err("计划请求不能为空或过长".into()) }
        match &scope {
            Scope::Module { module } if !valid_plan_id(module) => return Err("模块 ID 无效".into()),
            Scope::Baseline { baseline } if state["Baseline"].get(baseline).is_none() => return Err("基线尚未出版".into()),
            _ => {}
        }
        let plan = Plan { id: event.obj.clone(), scope, text, requested_by: event.actor.clone(), requested_at: event.t.clone(),
            status: Status::Consulting, summary: None, steps: Vec::new(), drafts: Vec::new(), notes: Vec::new(), reason: None,
            agent: None, cost_usd: None, alert: None, resolved_by: None, resolved_at: None, note: None };
        store(state, &plan)?;
        return Ok(true);
    }
    let mut plan = plans.remove(&event.obj).ok_or("计划不存在")?;
    if let PlanEvent::Confirmed { note } | PlanEvent::TakenOver { note } = &change {
        if !human || note.as_deref().is_some_and(|n| n.chars().count() > TEXT_CHARS) {
            return Err("只有人能在 App 确认或接管总控的计划".into());
        }
    }
    match change {
        PlanEvent::Requested { .. } => unreachable!(),
        PlanEvent::Previewed { summary, steps, drafts, notes, agent, cost_usd } => {
            if !controller || plan.status != Status::Consulting { return Err("只有总控能给在等的计划出预览".into()) }
            if !filled(&summary, TEXT_CHARS) || steps.len() > STEPS_MAX || drafts.len() > ITEMS_MAX || notes.len() > ITEMS_MAX
                || (steps.is_empty() && drafts.is_empty() && notes.is_empty())
                || drafts.iter().any(|d| !filled(&d.target, LINE_CHARS) || !filled(&d.text, TEXT_CHARS))
                || notes.iter().any(|n| !filled(n, LINE_CHARS)) || !valid_cost(cost_usd)
            {
                return Err("计划预览须有摘要、1 项以上内容，且不超出长度".into());
            }
            // A dry run: the preview shows only what could be applied now.
            execute(&mut state.clone(), &plan.scope, &steps)?;
            (plan.status, plan.summary, plan.steps, plan.drafts, plan.notes, plan.agent, plan.cost_usd) =
                (Status::Previewed, Some(summary), steps, drafts, notes, agent, cost_usd);
            plan.alert = Some(event.eid.clone());
            alert(state, event, &plan, "总控的计划等你确认")?;
        }
        PlanEvent::Failed { reason, agent, cost_usd } => {
            if !controller || plan.status != Status::Consulting { return Err("只有总控能记录在等的计划没有生成".into()) }
            if !filled(&reason, TEXT_CHARS) || !valid_cost(cost_usd) { return Err("计划失败须写明原因".into()) }
            (plan.status, plan.reason, plan.agent, plan.cost_usd) = (Status::Failed, Some(reason), agent, cost_usd);
            plan.alert = Some(event.eid.clone());
            alert(state, event, &plan, "总控没能生成计划")?;
        }
        PlanEvent::Confirmed { note } => {
            if plan.status != Status::Previewed { return Err("只有已预览、未处理的计划能确认执行".into()) }
            let mut next = state.clone();
            execute(&mut next, &plan.scope, &plan.steps)?;
            *state = next;
            (plan.status, plan.note) = (Status::Confirmed, note);
        }
        PlanEvent::TakenOver { note } => {
            if !matches!(plan.status, Status::Consulting | Status::Previewed | Status::Failed) {
                return Err("计划已处理".into());
            }
            (plan.status, plan.note) = (Status::TakenOver, note);
        }
    }
    if matches!(plan.status, Status::Confirmed | Status::TakenOver) {
        (plan.resolved_by, plan.resolved_at) = (Some(event.actor.clone()), Some(event.t.clone()));
        if let Some(alert) = plan.alert.as_ref().and_then(|eid| state.get_mut("Alert")?.get_mut(eid)) {
            alert["resolved"] = json!(true);
        }
    }
    store(state, &plan)?;
    Ok(true)
}

/// Whether the newest baseline left tasks to review and has no plan yet; a
/// cheap look before the baseline is read from git.
pub fn regeneration_due(state: &Value) -> Result<bool, String> {
    let Some(id) = latest_baseline(state) else { return Ok(false) };
    let review = state.get("Task").and_then(Value::as_object)
        .is_some_and(|tasks| tasks.values().any(|task| task["needs_review"] == true && task["state"] != "merged"));
    Ok(review && !plans(state)?.contains_key(&format!("plan-J4-{id}")))
}

/// J4 step 4: after baseline `baseline` took effect, the tasks it left to
/// review. Those still fitting it get a rebase step; the rest are named for
/// the person to rewrite. Old branches stay as references. `None` when
/// nothing is left or a plan for this baseline exists already.
pub fn regenerate(state: &Value, baseline: &VerifiedBaseline) -> Result<Option<(String, PlanEvent, PlanEvent)>, String> {
    let id = baseline.tag().strip_prefix("towermd/").ok_or("基线 tag 无效")?.to_owned();
    if !regeneration_due(state)? || latest_baseline(state).as_deref() != Some(id.as_str()) { return Ok(None) }
    let plan_id = format!("plan-J4-{id}");
    let projection = dev_state::tasks(state)?;
    let stale: Vec<&Task> = projection.tasks.values()
        .filter(|task| task.needs_review && task.state != TaskState::Merged && task.baseline != baseline.tag()).collect();
    if stale.is_empty() { return Ok(None) }
    let (mut steps, mut notes) = (Vec::new(), Vec::new());
    for task in stale {
        let task_id = &task.input.id;
        let branch = projection.runs.values().any(|run| &run.task == task_id).then(|| format!("旧分支 t/{task_id} 保留作参考"));
        match task_model::fits(baseline, &task.input, task.origin) {
            Ok(()) if steps.len() < STEPS_MAX => {
                steps.push(Step::Rebase { task: task_id.clone(), baseline: id.clone() });
                if let Some(branch) = branch { notes.push(format!("{task_id}：{branch}")) }
            }
            Ok(()) => notes.push(format!("{task_id}：本计划步数已满，下次再按新基线复核")),
            Err(reason) => notes.push(format!("{task_id} 已不在新基线范围内（{reason}），须由人改写或新建任务{}",
                branch.map(|b| format!("；{b}")).unwrap_or_default())),
        }
    }
    notes.truncate(ITEMS_MAX);
    notes.iter_mut().for_each(|note| *note = note.chars().take(LINE_CHARS).collect());
    let requested = PlanEvent::Requested { scope: Scope::Baseline { baseline: id.clone() },
        text: format!("基线 {id} 已生效：按新基线重新生成受影响的任务（J4 第 4 步）") };
    let summary = format!("{} 项任务仍在新基线范围内，确认后绑到 {id} 并取消复核标记；{} 条说明须你处理", steps.len(), notes.len());
    let previewed = PlanEvent::Previewed { summary, steps, drafts: Vec::new(), notes, agent: None, cost_usd: None };
    Ok(Some((plan_id, requested, previewed)))
}

/// Plans waiting for the controller harness.
pub fn consulting(state: &Value) -> Result<Vec<Plan>, String> {
    Ok(plans(state)?.into_values().filter(|plan| plan.status == Status::Consulting && !matches!(plan.scope, Scope::Baseline { .. })).collect())
}

/// The reply the controller harness must give for a plan.
pub fn reply_schema() -> Value {
    let text = json!({"type": "string"});
    let list = json!({"type": "array", "items": {"type": "string"}});
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["summary", "steps", "drafts", "notes"],
        "properties": {
            "summary": text,
            "steps": {"type": "array", "maxItems": STEPS_MAX, "items": {
                "type": "object", "additionalProperties": false, "required": ["action", "task"],
                "properties": {
                    "action": {"enum": ["create", "priority", "pause", "resume"]},
                    "task": text, "priority": {"type": "integer"}, "title": text, "module": text,
                    "allowed_paths": list, "acceptance": list, "depends_on": list,
                },
            }},
            "drafts": {"type": "array", "maxItems": ITEMS_MAX, "items": {
                "type": "object", "additionalProperties": false, "required": ["target", "text"],
                "properties": {"target": text, "text": text},
            }},
            "notes": {"type": "array", "maxItems": ITEMS_MAX, "items": text},
        },
    })
}

/// The controller's brief: the person's words, the scope, the tasks and the
/// baseline's modules.
pub fn brief(state: &Value, plan: &Plan, baseline: Option<&VerifiedBaseline>) -> Result<String, String> {
    let projection = dev_state::tasks(state)?;
    let mut text = String::from("# 总控动作计划\n\n你是 Tower.md 项目的总控。人在 App 中给了下面这条指令，请把它转成动作计划。\n\n\
        计划只预览，人确认后才执行；你不执行任何动作，不批准任何东西，也不判定完成。\n\
        steps 只能用四种动作：create（新增任务，给出 task、title、module、allowed_paths、acceptance，可选 priority、depends_on）、\
        priority（调整优先级）、pause（暂停）、resume（继续）。\n\
        改模块、接口、验收标准或基线等慢层内容时，不要放进 steps，只写进 drafts（target 写对象，text 写建议），它们只作草稿提案。\n\
        拿不准或指令超出这些动作时，steps 留空，在 notes 里说明。\n\n## 指令\n\n");
    text += &format!("{}\n", plan.text);
    if let Scope::Module { module } = &plan.scope {
        text += &format!("\n只影响模块 {module} 的任务；新增任务的 module 必须是 {module}。\n");
    }
    if let Some(baseline) = baseline {
        text += &format!("\n## 基线 {}\n\n", baseline.tag());
        for (id, module) in baseline.modules() {
            text += &format!("- 模块 {id}：{}\n", module.paths.join("、"));
        }
    }
    text += "\n## 任务\n\n";
    let tasks: Vec<&Task> = projection.tasks.values()
        .filter(|task| !matches!(&plan.scope, Scope::Module { module } if &task.input.module != module)).collect();
    if tasks.is_empty() { text += "（还没有任务）\n" }
    for task in tasks {
        let state = serde_json::to_value(task.state).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        text += &format!("- {} {}（模块 {}，{state}，优先级 {}{}）\n", task.input.id, task.input.title, task.input.module,
            task.input.priority, if task.needs_review { "，待按新基线复核" } else { "" });
    }
    Ok(text)
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// The controller's reply as a preview, or why a person must plan it.
pub fn preview(state: &Value, plan: &Plan, baseline: Option<&VerifiedBaseline>, reply: Option<&Value>, error: Option<&str>,
    agent: Option<String>, cost_usd: Option<f64>) -> PlanEvent {
    let failed = |reason: String, agent: Option<String>| PlanEvent::Failed {
        reason: reason.chars().take(TEXT_CHARS).collect(), agent, cost_usd };
    let Some(reply) = reply else { return failed(error.unwrap_or("总控没有给出计划").to_owned(), agent) };
    let parsed = steps(state, plan, baseline, reply)
        .and_then(|parsed| execute(&mut state.clone(), &plan.scope, &parsed.1).map(|()| parsed)
            .map_err(|reason| format!("总控的计划不能执行：{reason}")));
    match parsed {
        Ok((summary, steps, drafts, notes)) => PlanEvent::Previewed { summary, steps, drafts, notes, agent, cost_usd },
        Err(reason) => failed(reason, agent),
    }
}

type Parsed = (String, Vec<Step>, Vec<Draft>, Vec<String>);

fn steps(state: &Value, plan: &Plan, baseline: Option<&VerifiedBaseline>, reply: &Value) -> Result<Parsed, String> {
    let summary = reply["summary"].as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("总控的计划没有摘要")?.to_owned();
    let mut projection: TaskProjection = dev_state::tasks(state)?;
    let mut steps = Vec::new();
    for item in reply["steps"].as_array().into_iter().flatten() {
        let task = item["task"].as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("计划步骤缺少任务")?.to_owned();
        let step = match item["action"].as_str() {
            Some("create") => {
                let baseline = baseline.ok_or("新增任务需要已出版基线")?;
                let module = item["module"].as_str().map(str::to_owned)
                    .or_else(|| match &plan.scope { Scope::Module { module } => Some(module.clone()), _ => None })
                    .ok_or("新增任务缺少模块")?;
                let draft = TaskDraft { id: task, title: item["title"].as_str().unwrap_or_default().trim().to_owned(), module,
                    allowed_paths: strings(&item["allowed_paths"]), acceptance: strings(&item["acceptance"]),
                    depends_on: strings(&item["depends_on"]),
                    priority: item["priority"].as_i64().and_then(|p| i32::try_from(p).ok()).unwrap_or_default(),
                    ..TaskDraft::default() };
                let event = task_model::prepare_add(baseline, &projection, draft, TaskOrigin::AppHuman)?;
                projection.apply(&event)?;
                let TaskEvent::Created { task } = event else { unreachable!() };
                Step::Create { task }
            }
            Some("priority") => Step::Priority { task,
                priority: item["priority"].as_i64().and_then(|p| i32::try_from(p).ok()).ok_or("调整优先级缺少数值")? },
            Some("pause") => Step::Pause { task },
            Some("resume") => Step::Resume { task },
            other => return Err(format!("总控计划里有不支持的动作：{}", other.unwrap_or("（空）"))),
        };
        steps.push(step);
    }
    let drafts = reply["drafts"].as_array().into_iter().flatten().map(|draft| Draft {
        target: draft["target"].as_str().unwrap_or_default().trim().chars().take(LINE_CHARS).collect(),
        text: draft["text"].as_str().unwrap_or_default().trim().chars().take(TEXT_CHARS).collect() })
        .filter(|draft| !draft.target.is_empty() && !draft.text.is_empty()).take(ITEMS_MAX).collect();
    let notes = strings(&reply["notes"]).into_iter().map(|n| n.chars().take(LINE_CHARS).collect()).take(ITEMS_MAX).collect();
    if steps.len() > STEPS_MAX { return Err(format!("总控的计划超过 {STEPS_MAX} 步，请拆小指令")) }
    Ok((summary.chars().take(TEXT_CHARS).collect(), steps, drafts, notes))
}
