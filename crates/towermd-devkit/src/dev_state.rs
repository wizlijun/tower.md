//! Development kit projection: baselines, tasks, runs and finish evidence.
//! Event names and payloads are the v0.1.1 ones; only their owner moved.
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{baseline_publication, run_record, task_model::{self, ReleaseReason, Run, RunState, TaskEvent}};
use towermd_core::attention_list::{AlertText, Kind as AttentionKind, Located, Stuck};
use towermd_core::{delegation::KitDelegation, hub::Event, ideas::{self, Promote}, kit::{Assembly, InitContribution, Kit, KitRun}, project_state::{self, insert_unique, valid_oid}, question_model::RunContext, weakening::{Bar, Commitment}};

pub struct DevKit;
pub const GROUPS: &[&str] = &["Baseline", "BaselineApproval", "Task", "Run", "Evidence"];
pub const KITS: &[&dyn Kit] = &[&DevKit];
pub const ASSEMBLY: Assembly = Assembly { kits: KITS, reduce: apply };
/// FR-125: acceptance criteria, and the verify commands of modules and
/// acceptances, are what a change to the baseline can weaken.
pub const COMMITMENTS: &[Commitment] = &[
    Commitment { doc_type: "Acceptance", key: "criteria", bar: Bar::Criteria },
    Commitment { doc_type: "Acceptance", key: "verify", bar: Bar::Checks },
    Commitment { doc_type: "Module", key: "verify", bar: Bar::Checks },
];

/// A person promotes an idea as a task (FR-183).
pub const PROMOTE_TASK: &str = "task";

/// The task an `idea.promoted` event created, if it promoted one.
pub fn promoted_task(kind: &str, data: &Value) -> Result<Option<crate::task_model::Task>, String> {
    if kind != ideas::PROMOTED { return Ok(None) }
    let request: Promote = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
    if request.kind != PROMOTE_TASK { return Ok(None) }
    match serde_json::from_value(request.object.unwrap_or_default()).map_err(|e| e.to_string())? {
        TaskEvent::Created { task } => Ok(Some(task)),
        _ => Err("立为任务只能创建任务".into()),
    }
}

/// Promoting as a task is the App's own task creation, carried by the
/// promotion event: same signer, layer, object and baseline checks.
fn promote(state: &mut Value, event: &Event, request: &Promote) -> Result<bool, String> {
    if request.kind != PROMOTE_TASK { return Ok(false) }
    let task = promoted_task(&event.kind, &event.data)?.ok_or("立为任务缺少任务")?;
    if task.origin != crate::task_model::TaskOrigin::AppHuman {
        return Err("立为任务只能是 App 人工任务".into());
    }
    let mut created = event.clone();
    created.kind = "task.created".into();
    created.obj = request.target.clone().ok_or("立项缺少目标对象")?;
    created.data = serde_json::to_value(TaskEvent::Created { task }).map_err(|e| e.to_string())?;
    reduce(state, &created)
}

impl Kit for DevKit {
    fn state_groups(&self) -> &'static [&'static str] {
        GROUPS
    }
    fn reduce(&self, state: &mut Value, event: &Event) -> Result<bool, String> {
        let before = crate::task_interrupts::snapshot(state)?;
        let handled = reduce(state, event)?;
        if handled {
            crate::task_interrupts::settle(state, event, &before)?;
        }
        Ok(handled)
    }
    fn observe(&self, state: &mut Value, event: &Event) -> Result<(), String> {
        let before = crate::task_interrupts::snapshot(state)?;
        crate::task_interrupts::observe(state, event)?;
        crate::task_interrupts::settle(state, event, &before)
    }
    fn runs(&self, state: &Value) -> Result<Vec<KitRun>, String> {
        Ok(tasks(state)?
            .runs
            .into_values()
            .map(|run| KitRun {
                active: run.state == RunState::Active,
                context: RunContext{run: run.binding.id, task: run.task, scope: run.module},
            })
            .collect())
    }
    fn prove_protected_commit(
        &self,
        root: &Path,
        commit: &str,
        project_id: &str,
        receipt: &dyn Fn(&str) -> Option<Event>,
    ) -> Result<(), String> {
        prove_baseline(root, commit, project_id, receipt)
    }
    fn delegations(&self, state: &Value) -> Result<Vec<KitDelegation>, String> {
        crate::delegation_kit::delegations(state)
    }
    fn commitments(&self) -> &'static [Commitment] {
        COMMITMENTS
    }
    fn promote(&self, state: &mut Value, event: &Event, request: &Promote) -> Result<bool, String> {
        promote(state, event, request)
    }
    fn locate(&self, projection: &Value, object: &str) -> Option<Located> {
        let input = attention_tasks(projection).find(|input| input["id"] == object)?;
        Some(Located { area: input["module"].as_str()?.to_owned(),
            reaches: input.get("touches").and_then(Value::as_array).map(|touches| touches.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default() })
    }
    fn alert(&self, reason: &str) -> Option<AlertText> {
        let (kind, title, action) = match reason {
            "task_gate_failed" => (AttentionKind::CheckFailed, Some("任务未通过完成检查"), "查看验证证据，修正后重新运行完成验证。"),
            // Spec 12.1: what holds a task until a person chooses retry, rewrite or reassign.
            task_model::GATE_EXHAUSTED => (AttentionKind::Interrupt, None, "看验证证据后选择：让 Agent 再修、改写任务、重派，或豁免声明的检查。"),
            task_model::FORBIDDEN => (AttentionKind::Interrupt, None, "看越界的改动后选择：让 Agent 再修、改写任务或重派；路径守卫不能豁免。"),
            task_model::INFO_MISSING => (AttentionKind::Interrupt, None, "补全委托卡（允许路径、验收标准或相关决策）后定稿，或改写、重派。"),
            run_record::RUN_ERROR => (AttentionKind::Error, None, "重试、改写任务、换 harness 或模型，或放弃。"),
            run_record::BUDGET_EXHAUSTED => (AttentionKind::Budget, None, "追加预算、换便宜模型，或停止。"),
            run_record::BUDGET_UNKNOWN => (AttentionKind::Budget, None, "这个 harness 不报告费用：确认后取消预算，或换报告费用的 harness。"),
            _ => return None,
        };
        Some(AlertText { kind, title: title.map(str::to_owned), action: action.to_owned() })
    }
    /// A spent gate's interrupt (spec 12.1) supersedes the per-attempt check failures before it.
    fn folded(&self, projection: &Value, reason: &str, detail: &Value) -> bool {
        reason == "task_gate_failed" && detail["task"].as_str()
            .and_then(|task| projection.get("Task")?.get(task))
            .is_some_and(|task| task["interrupt"]["kind"] == "gate_failed")
    }
    fn review_stage(&self, stage: &str) -> Option<&'static str> {
        match stage { "plan" => Some("计划评审（G1）"), "code" => Some("代码评审（G4）"), _ => None }
    }
    fn stuck(&self, projection: &Value) -> Vec<Stuck> {
        let tasks = projection.get("Task").and_then(Value::as_object).into_iter().flat_map(|tasks| tasks.values());
        let mut tasks: Vec<&Value> = tasks.filter(|task| attention_task(task).is_some()).collect();
        tasks.sort_by(|a, b| a["input"]["id"].as_str().cmp(&b["input"]["id"].as_str()));
        tasks.into_iter().filter(|task| task["needs_review"] == Value::Bool(true) || task["state"] == "held" || task["state"] == "paused")
            .filter_map(|task| Some(Stuck { id: task["input"]["id"].as_str()?.to_owned(), title: task["input"]["title"].as_str()?.to_owned() })).collect()
    }
    fn init(&self) -> InitContribution {
        InitContribution {
            dirs: vec!["intents", "arch", "specs", "baselines"],
            agents_section: include_str!("../templates/agents-dev.md"),
            hints: vec!["请按 Tower.md 发行包 skills/README.md 安装技能；在会话中使用 towermd-init、towermd-baseline、towermd-claim、towermd-ask 和 towermd-finish。安装由你选择，不修改全局 harness 配置。".into()],
        }
    }
}

const TASK_STATES: [&str; 7] = ["proposed", "ready", "running", "paused", "verifying", "held", "merged"];
/// A task's input as the attention list reads it: lenient, like the App.
fn attention_task(task: &Value) -> Option<&Value> {
    let input = task.get("input")?;
    let valid = input["id"].is_string() && input["module"].is_string() && input["title"].is_string()
        && task["state"].as_str().is_some_and(|state| TASK_STATES.contains(&state));
    valid.then_some(input)
}
fn attention_tasks(projection: &Value) -> impl Iterator<Item = &Value> {
    projection.get("Task").and_then(Value::as_object).into_iter().flat_map(|tasks| tasks.values()).filter_map(attention_task)
}

/// Per-run times taken from the accepted events' `t` (v0.7). Created lazily,
/// so the v0.1.1 initial projection is unchanged; rebuilt by replay.
pub const TIMING: &str = "RunTiming";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub at: String,
    pub eid: String,
    pub seq: u64,
}
impl Stamp {
    fn of(event: &Event) -> Self {
        Self { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTiming {
    pub task: String,
    /// The claim: the delegation took effect.
    pub claimed: Stamp,
    /// Latest finish request: the delegate claimed done.
    pub delivered: Option<Stamp>,
    /// Finish request stamp per evidence ID.
    pub finishes: BTreeMap<String, Stamp>,
    /// The merge proof: done by checks.
    pub done: Option<Stamp>,
}

pub fn timings(state: &Value) -> Result<BTreeMap<String, RunTiming>, String> {
    match state.get(TIMING) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(BTreeMap::new()),
    }
}

fn update_timing(state: &mut Value, run: &str, change: impl FnOnce(&mut RunTiming)) -> Result<(), String> {
    let mut timing: RunTiming = serde_json::from_value(state[TIMING][run].clone())
        .map_err(|_| "运行缺少领取时间记录")?;
    change(&mut timing);
    state[TIMING][run] = serde_json::to_value(timing).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn initial() -> Value {
    project_state::initial_with(KITS)
}

pub fn apply(state: &mut Value, event: &Event) -> Result<(), String> {
    project_state::apply_with(KITS, state, event)
}

fn reduce(state: &mut Value, event: &Event) -> Result<bool, String> {
    if crate::proposal_model::apply(state, event)? || crate::run_record::apply(state, event)?
        || crate::task_interrupts::reduce(state, event)?
    {
        return Ok(true);
    }
    match event.kind.as_str() {
        "baseline.approved" => {
            let publication = baseline_publication::from_approval(event)?;
            let proposal_link=crate::proposal_model::publication_link(&publication)?;
            if let Some(link)=&proposal_link {
                let proposal=crate::proposal_model::from_state(state,&link.id)?;
                if state["Proposal"][&link.id]["status"]!="submitted"
                    ||proposal.content_hash!=link.content_hash||proposal.impact.modules!=link.affected_modules
                    ||proposal.draft_id.as_deref()!=Some(publication.draft_id.as_str())
                    ||proposal.source_revision!=publication.source_revision
                    ||proposal.source_proposal_id!=link.source_proposal_id {
                    return Err("基线批准必须绑定待批准提案的原始差异和影响集合".into());
                }
                crate::proposal_model::validate_source(state,&proposal)?;
            } else if publication.target!="B0" {return Err("后续基线须经提案和 L1 决策批准".into());}
            let entry = json!({
                "proposal_ref":proposal_link,
                "id":publication.target,"approval_eid":event.eid,
                "draft_id":publication.draft_id,"content_hash":publication.content_hash,
                "source_revision":publication.source_revision,
                "approved_by":event.actor,"approved_at":publication.prepared_at,
            });
            insert_unique(state, "BaselineApproval", &event.eid, entry)?;
        }
        "baseline.created" => {
            if !matches!(
                event.actor.as_str(),
                "process:towermd" | "process:towermd-recovery"
            ) || event.layer != "L1" || event.guards.len() != 1
                || event.guards[0].name != "main" || event.guards[0].epoch == 0
                || event.epoch != Some(event.guards[0].epoch)
            {
                return Err("基线出版必须由守护进程确认".into());
            }
            let approval = event.data["approval_eid"]
                .as_str()
                .ok_or("出版缺少批准原件 ID")?.to_string();
            let approved = state["BaselineApproval"]
                .get(&approval)
                .ok_or("出版没有对应已接受批准")?;
            if event.data["content_hash"] != approved["content_hash"]
                || event.obj != approved["id"].as_str().unwrap_or_default()
                || event.data["commit"].as_str().is_none_or(|c| !valid_oid(c))
            {
                return Err("基线出版记录与批准内容不匹配".into());
            }
            let entry = json!({
                "type":"Baseline","id":event.obj,"commit":event.data["commit"],
                "approval_eid":approval,"content_hash":approved["content_hash"],
                "git_tag":format!("towermd/{}",event.obj),
                "approved_by":approved["approved_by"],"approved_at":approved["approved_at"],
            });
            let proposal_link:Option<crate::proposal_model::PublicationLink>=serde_json::from_value(approved["proposal_ref"].clone()).map_err(|e|e.to_string())?;
            insert_unique(state, "Baseline", &event.obj, entry)?;
            let mut projection = tasks(state)?;
            let current_tag = format!("towermd/{}", event.obj);
            let answer_tasks=if let Some(link)=&proposal_link {
                let formal=crate::proposal_model::from_state(state,&link.id)?;
                resolve_publication(state,&formal,event,&approval)?
            }else{Vec::new()};
            for task in projection.tasks.values_mut() {
                if task.baseline != current_tag
                    && task.state != crate::task_model::TaskState::Merged
                {
                    let affected=answer_tasks.contains(&task.input.id)||proposal_link.as_ref().is_none_or(|link|link.affected_modules.contains(&task.input.module)
                        ||task.input.touches.iter().any(|id|link.affected_modules.contains(id)));
                    if affected { task.needs_review = true; }
                    else {
                        task.baseline=current_tag.clone();
                        for run in projection.runs.values_mut().filter(|run|run.task==task.input.id) {
                            run.baseline=current_tag.clone();
                        }
                    }
                }
            }
            if let Some(link)=proposal_link {
                if let Some(source)=&link.source_proposal_id {
                    state["Proposal"][source]["status"]=json!("approved");
                    state["Proposal"][source]["linked_proposal"]=json!(link.id);
                    state["Proposal"][source]["baseline"]=json!(current_tag);
                    state["Proposal"][source]["approval_eid"]=json!(approval);
                }
                let proposal=&mut state["Proposal"][&link.id];
                proposal["status"]=json!("approved");proposal["baseline"]=json!(current_tag);
                proposal["approval_eid"]=json!(approval);
            }
            write_tasks(state, &projection)?;
        }
        "recovery.completed" => {
            // The real recovery.completed receipt revokes all old authority;
            // this is not a fabricated historical lease.expired event.
            release_runs(state, |_| true, ReleaseReason::Reassigned)?;
        }
        "lease.expired" | "lease.released" => {
            let fences = event.released_fences().to_vec();
            let reason = if event.kind == "lease.expired" {
                ReleaseReason::Expired
            } else {
                ReleaseReason::Voluntary
            };
            release_runs(state, |run| run.fences.iter().any(|fence| fences.contains(fence)), reason)?;
        }
        "task.created" | "task.edited" | "task.claimed" | "task.released" => {
            let action: crate::task_model::TaskEvent =
                serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            let mut projection = tasks(state)?;
            use crate::task_model::{TaskEvent, TaskOrigin};
            match (&action, event.kind.as_str()) {
                (TaskEvent::Created { task }, "task.created") => {
                    let actor_ok = match task.origin {
                        TaskOrigin::Cli => event.actor == "process:towermd-cli",
                        TaskOrigin::AppHuman => event.actor.starts_with("human:"),
                    };
                    if !actor_ok
                        || event.layer != "L4"
                        || event.obj != task.input.id
                        || !event.guards.is_empty()
                    {
                        return Err("任务创建来源或对象不匹配".into());
                    }
                    let baseline = task
                        .baseline
                        .strip_prefix("towermd/")
                        .ok_or("任务基线无效")?;
                    if state["Baseline"].get(baseline).is_none() {
                        return Err("任务没有已出版基线".into());
                    }
                }
                (TaskEvent::Edited { task }, "task.edited") => {
                    if event.actor != "process:towermd-cli"
                        || event.layer != "L4"
                        || event.obj != task.input.id
                        || !event.guards.is_empty()
                    {
                        return Err("任务修改来源或对象不匹配".into());
                    }
                    let baseline = task.baseline.strip_prefix("towermd/").ok_or("任务基线无效")?;
                    if state["Baseline"].get(baseline).is_none() {
                        return Err("任务没有已出版基线".into());
                    }
                    if card_finalized(state, &task.input.id) {
                        return Err("委托卡已由人定稿，命令行不能再修改该任务".into());
                    }
                }
                (TaskEvent::Claimed { run }, "task.claimed") => {
                    if event.actor != "process:towermd-cli"
                        || event.layer != "L4"
                        || event.obj != run.task
                        || event.guards != run.fences
                    {
                        return Err("任务领取须由CLI流程携完整模块fences".into());
                    }
                }
                (TaskEvent::Released { run_id, .. }, "task.released") => {
                    let run = projection.runs.get(run_id).ok_or("释放缺少运行")?;
                    if event.actor != "process:towermd-cli"
                        || event.layer != "L4"
                        || event.obj != run.task
                        || event.guards != run.fences
                    {
                        return Err("任务释放来源或fences不一致".into());
                    }
                }
                _ => return Err("任务事件类型与正文不匹配".into()),
            }
            projection.apply(&action)?;
            write_tasks(state, &projection)?;
            if let TaskEvent::Claimed { run } = &action {
                let timing = RunTiming { task: run.task.clone(), claimed: Stamp::of(event),
                    delivered: None, finishes: BTreeMap::new(), done: None };
                state[TIMING][&run.binding.id] = serde_json::to_value(timing).map_err(|e| e.to_string())?;
            }
        }
        "task.finish.requested" | "task.finish.started" | "task.finish.completed"
        | "task.finish.interrupted" | "task.g5.completed" | "task.merged" => {
            apply_finish(state, event)?;
        }
        "delegation.finalized" | "acceptance.confirmed" | "acceptance.sent_back" => {
            return crate::delegation_kit::react(state, event);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn release_runs(
    state: &mut Value,
    select: impl Fn(&Run) -> bool,
    reason: ReleaseReason,
) -> Result<(), String> {
    let mut projection = tasks(state)?;
    let released: Vec<_> = projection
        .runs
        .values()
        .filter(|run| run.state == RunState::Active && select(run))
        .map(|run| run.binding.id.clone())
        .collect();
    for run_id in released {
        projection.apply(&TaskEvent::Released { run_id, reason })?;
    }
    write_tasks(state, &projection)
}

fn apply_finish(state: &mut Value, event: &Event) -> Result<(), String> {
    use crate::task_finish::{self, FinishRequest, FrozenFinish, MergeProof};
    use crate::task_model::{RunState, TaskState};
    if event.layer != "L4" { return Err("任务证据层级无效".into()); }
    let mut projection = tasks(state)?;
    if event.kind == "task.finish.requested" {
        let request: FinishRequest = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
        let run = projection.runs.get(&request.run.binding.id).ok_or("finish 缺少活动运行")?;
        let task = projection.tasks.get_mut(&run.task).ok_or("finish 缺少任务")?;
        if event.actor != "process:towermd-cli" || event.host != run.binding.host
            || event.guards != run.fences || request.run != *run || run.state != RunState::Active
            || task.active_run.as_deref() != Some(run.binding.id.as_str())
            || !matches!(task.state, TaskState::Running | TaskState::Verifying)
            || projection.paused_all || task.needs_review
            || request.evidence_id != event.obj
            || request.deadline_ms != request.requested_at_ms.checked_add(task_finish::VERIFYING_TTL_MS).ok_or("验证期限溢出")?
            || request.started_eid.is_empty() || request.completion_eid.is_empty() || request.interrupted_eid.is_empty()
        { return Err("finish 请求与当前任务、运行、fences 或固定期限不同".into()); }
        if request.claims.iter().any(|claim| claim.criterion >= task.input.acceptance.len()
            || claim.note.trim().is_empty() || claim.note.len() > 64 * 1024)
        { return Err("自述须指向已有完成标准且说明非空".into()); }
        if let Some(report) = &request.report { report.validate()?; }
        if state["Evidence"].as_object().ok_or("Evidence 投影无效")?.values().any(|value| {
            value["run"] == run.binding.id && matches!(value["status"].as_str(), Some("queued" | "running"))
        }) { return Err("运行已有验证请求".into()); }
        task.state = TaskState::Verifying;
        let run_id = run.binding.id.clone();
        insert_unique(state, "Evidence", &event.obj, json!({
            "id":event.obj,"kind":"finish","task":run.task,"run":run.binding.id,
            "status":"queued","request_eid":event.eid,"request":request,
        }))?;
        update_timing(state, &run_id, |timing| {
            timing.delivered = Some(Stamp::of(event));
            timing.finishes.insert(event.obj.clone(), Stamp::of(event));
        })?;
        return write_tasks(state, &projection);
    }
    let id = if event.kind == "task.merged" {
        event.data["evidence_id"].as_str().ok_or("合并证明缺少证据 ID")?
    } else { event.obj.as_str() };
    let mut entry = state["Evidence"].get(id).cloned().ok_or("任务证据缺少原 finish 请求")?;
    let request: FinishRequest = serde_json::from_value(entry["request"].clone()).map_err(|e| e.to_string())?;
    match event.kind.as_str() {
        "task.finish.started" => {
            let input: FrozenFinish = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            let task = projection.tasks.get(&request.run.task).ok_or("验证任务不存在")?;
            if event.actor != "process:towermd-verifier" || event.host != request.run.binding.host
                || event.eid != request.started_eid || event.guards != request.run.fences
                || entry["status"] != "queued" || input.evidence_id != id || input.run != request.run
                || input.task.input != task.input || input.task.baseline != task.baseline
                || input.requested_at_ms != request.requested_at_ms || input.deadline_ms != request.deadline_ms
            { return Err("验证开始必须冻结原请求的完整输入".into()); }
            entry["status"] = json!("running"); entry["input"] = json!(input);
            entry["started_eid"] = json!(event.eid);
        }
        "task.finish.completed" => {
            let evidence = task_finish::completion_from_event(event)?;
            if entry["status"] != "running" || event.eid != request.completion_eid
                || serde_json::to_value(&evidence.input).map_err(|e|e.to_string())? != entry["input"]
            { return Err("完成证据与原耐久冻结输入不一致".into()); }
            entry["status"] = json!(if evidence.passed { "passed" } else { "failed" });
            entry["completion_eid"] = json!(event.eid);
            entry["completion"] = json!(event);
            entry["evidence"] = json!(evidence);
            if !evidence.passed {
                if let Some(task) = projection.tasks.get_mut(&request.run.task) {
                    if task.active_run.as_deref() == Some(request.run.binding.id.as_str()) && task.state == TaskState::Verifying {
                        task.state = TaskState::Running;
                    }
                }
                // D4, FR-57: the builtin check failed; the third time a person decides.
                projection.gate_failed(&request.run.task, crate::task_interrupts::VERIFY, "完成验证未通过", &event.eid)?;
            }
        }
        "task.finish.interrupted" => {
            if event.actor != "process:towermd-verifier" || event.eid != request.interrupted_eid
                || event.host != request.run.binding.host || !event.guards.is_empty()
                || event.data["request_eid"] != entry["request_eid"]
                || event.data["error"].as_str().is_none_or(|error| error.is_empty())
                || event.data.get("guard").is_some_and(|guard| guard != task_finish::G2)
                || !matches!(entry["status"].as_str(), Some("queued" | "running"))
            { return Err("验证中断必须绑定原请求，不能代替完成证据".into()); }
            entry["status"] = json!("interrupted"); entry["error"] = event.data["error"].clone();
            if let Some(task) = projection.tasks.get_mut(&request.run.task) {
                if task.active_run.as_deref() == Some(request.run.binding.id.as_str()) && task.state == TaskState::Verifying {
                    task.state = TaskState::Running;
                }
            }
            // 12.1 禁止: G2 stopped the change; the task waits for a person.
            if event.data.get("guard").is_some() {
                projection.hold(&request.run.task, &request.run.binding.id, crate::task_model::InterruptKind::Forbidden,
                    Some(task_finish::G2), event.data["error"].as_str().unwrap_or_default(), &event.eid);
            }
        }
        "task.g5.completed" => {
            let finish: Event = serde_json::from_value(entry["completion"].clone()).map_err(|e|e.to_string())?;
            task_finish::g5_from_event(&finish, event)?;
            if entry["status"] != "passed" { return Err("仅通过 G3 的证据能记录 G5".into()); }
            entry["g5_eid"] = json!(event.eid); entry["g5"] = json!(event);
        }
        "task.merged" => {
            let proof: MergeProof = serde_json::from_value(event.data.clone()).map_err(|e|e.to_string())?;
            let run = projection.runs.get_mut(&request.run.binding.id).ok_or("合并运行不存在")?;
            let task = projection.tasks.get_mut(&request.run.task).ok_or("合并任务不存在")?;
            let recovered = event.actor == "process:towermd-recovery";
            if !matches!(event.actor.as_str(), "process:towermd" | "process:towermd-recovery")
                || event.obj != task.input.id || proof.task_id != task.input.id || proof.run_id != run.binding.id
                || proof.evidence_id != id || proof.finish_eid != entry["completion_eid"].as_str().unwrap_or("")
                || proof.g5_eid != entry["g5_eid"].as_str().unwrap_or("")
                || event.eid != towermd_core::events::derived_eid(&proof.admission_commit, "task.merged")?
                || !valid_oid(&proof.main_revision) || !valid_oid(&proof.tree)
                || Some(proof.epoch) != run.primary_fence().map(|fence|fence.epoch)
                || entry["status"] != "passed"
                || (!recovered && (event.guards != run.fences || run.state != RunState::Active
                    || task.active_run.as_deref() != Some(run.binding.id.as_str()) || task.needs_review
                    || task.state != TaskState::Verifying || projection.paused_all))
                || (recovered && (event.guards.len()!=1 || event.guards[0].name!="main"
                    || event.guards[0].epoch==0 || event.epoch!=Some(event.guards[0].epoch)))
            { return Err("任务完成须由守护进程携真实 main 接入证明认证".into()); }
            task.state = TaskState::Merged; task.active_run = None;
            run.state = RunState::Released;
            entry["status"] = json!("merged"); entry["merge"] = json!(proof);
            crate::task_alerts::resolve_after_merge(state, event);
            update_timing(state, &proof.run_id, |timing| timing.done = Some(Stamp::of(event)))?;
        }
        _ => unreachable!(),
    }
    state["Evidence"][id] = entry;
    write_tasks(state, &projection)
}

/// A person finalized the task's card (FR-201): its fields are theirs now.
pub fn card_finalized(state: &Value, task: &str) -> bool {
    state[towermd_core::delegation_projection::GROUP]["objects"][task]["finalized"].as_array().is_some_and(|entries| !entries.is_empty())
}

pub fn tasks(state: &Value) -> Result<crate::task_model::TaskProjection, String> {
    Ok(crate::task_model::TaskProjection {
        tasks: serde_json::from_value(state["Task"].clone()).map_err(|e| e.to_string())?,
        runs: serde_json::from_value(state["Run"].clone()).map_err(|e| e.to_string())?,
        paused_all: state["tasks_paused"].as_bool().unwrap_or(false),
    })
}
pub(crate) fn write_tasks(
    state: &mut Value,
    projection: &crate::task_model::TaskProjection,
) -> Result<(), String> {
    state["Task"] = serde_json::to_value(&projection.tasks).map_err(|e| e.to_string())?;
    state["Run"] = serde_json::to_value(&projection.runs).map_err(|e| e.to_string())?;
    state["tasks_paused"] = json!(projection.paused_all);
    Ok(())
}

/// Called only after the kit has checked the actual baseline.created
/// receipt against its accepted approval. Answer text alone cannot call this
/// transition; the new baseline and original answer remain separately linked.
fn resolve_publication(state:&mut Value,proposal:&crate::proposal_model::Proposal,event:&Event,approval:&str)->Result<Vec<String>,String>{
    let Some(source)=crate::proposal_model::validate_source(state,proposal)? else{return Ok(Vec::new());};
    let mut projection=towermd_core::question_projection::read(state)?;
    let mut tasks=Vec::new();
    for (hold,answer) in projection.answers.iter_mut(){
        if answer.proposal.as_deref()!=Some(source.id.as_str()){continue;}
        if answer.event!=source.source_event{return Err("问题批准引用没有绑定原始回答事件".into());}
        let question=projection.holds.get(hold).ok_or("已批准回答缺少原问题")?;
        let proof=towermd_core::question_model::AnswerPublication{proposal:proposal.id.clone(),source_proposal:source.id.clone(),
            publication_event:event.eid.clone(),approval_event:approval.into(),baseline:format!("towermd/{}",event.obj),
            commit:event.data["commit"].as_str().ok_or("批准回执缺少实际提交")?.into()};
        if answer.publication.as_ref().is_some_and(|original|original!=&proof){return Err("回答已经绑定另一笔出版证明".into());}
        answer.publication=Some(proof);tasks.push(question.binding.task.clone());
    }
    state["Answer"]=serde_json::to_value(&projection.answers).map_err(|e|e.to_string())?;
    Ok(tasks)
}

fn prove_baseline(
    root: &Path,
    commit: &str,
    project_id: &str,
    receipt: &dyn Fn(&str) -> Option<Event>,
) -> Result<(), String> {
    let event = receipt(&towermd_core::events::derived_eid(commit, "baseline.created")?)
        .ok_or(towermd_core::kit::NO_PROOF)?;
    let approval_eid = event.data["approval_eid"].as_str().ok_or("基线出版事件缺少批准 ID")?;
    let approval = receipt(approval_eid).ok_or("基线出版缺少 canonical 批准原件")?;
    let publication = baseline_publication::from_approval(&approval)?;
    if event.stale || event.kind != "baseline.created" || event.layer != "L1"
        || event.eid != towermd_core::events::derived_eid(commit, "baseline.created")?
        || !matches!(event.actor.as_str(), "process:towermd" | "process:towermd-recovery")
        || event.obj != publication.target || publication.project_id != project_id
        || event.data != json!({"commit":commit, "approval_eid":approval_eid,
            "content_hash":publication.content_hash})
        || event.guards.len() != 1 || event.guards[0].name != "main"
        || event.epoch != Some(event.guards[0].epoch)
    {
        return Err("基线出版事件与项目、批准或主干租约不一致".into());
    }
    publication.prove_commit(root, commit, &approval)
}

#[cfg(test)]
#[path = "dev_state_tests.rs"]
mod tests;
