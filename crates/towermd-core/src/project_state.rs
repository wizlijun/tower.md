//! Incremental business projection. Only typed daemon operations produce these
//! events; self-reported Agent text cannot transition an object to completion.
use serde_json::{json, Value};

use crate::{baseline_publication, hub::Event};

pub fn initial() -> Value {
    json!({"Baseline":{}, "BaselineApproval":{}, "Task":{}, "Run":{},
        "Evidence":{}, "Hold":{}, "Proposal":{}, "Prompt":{}, "Alert":{}})
}

pub fn apply(state: &mut Value, event: &Event) -> Result<(), String> {
    if event.stale {
        return Ok(());
    }
    if crate::resolution_projection::apply(state, event)? { return Ok(()); }
    if crate::proposal_model::apply(state, event)? { return Ok(()); }
    match event.kind.as_str() {
        "hub.rebuild.confirmed" => {
            if event.actor != "human:local"
                || event.layer != "L5"
                || event.obj.trim().is_empty()
                || event.data != json!({"request_id":event.eid})
            {
                return Err("Hub 重建须有 App 人工确认原件".into());
            }
        }
        "baseline.approved" => {
            let publication = baseline_publication::from_approval(event)?;
            let proposal_link=crate::proposal_model::publication_link(&publication)?;
            if let Some(link)=&proposal_link {
                let proposal=crate::proposal_model::from_state(state,&link.id)?;
                if state["Proposal"][&link.id]["status"]!="submitted"
                    ||proposal.content_hash!=link.content_hash||proposal.impact.modules!=link.affected_modules
                    ||proposal.draft_id.as_deref()!=Some(publication.draft_id.as_str())
                    ||proposal.source_revision!=publication.source_revision {
                    return Err("基线批准必须绑定待批准提案的原始差异和影响集合".into());
                }
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
            for task in projection.tasks.values_mut() {
                if task.baseline != current_tag
                    && task.state != crate::task_model::TaskState::Merged
                {
                    let affected=proposal_link.as_ref().is_none_or(|link|link.affected_modules.contains(&task.input.module)
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
                let proposal=&mut state["Proposal"][&link.id];
                proposal["status"]=json!("approved");proposal["baseline"]=json!(current_tag);
                proposal["approval_eid"]=json!(approval);
            }
            write_tasks(state, &projection)?;
        }
        "alert.raised" => {
            insert_unique(
                state,
                "Alert",
                &event.obj,
                json!({
                    "id":event.obj,"event":event.eid,"data":event.data,"resolved":false,
                }),
            )?;
            crate::resolution_projection::reconcile_alerts(state)?;
        }
        "alert.cleared" => {
            if !event.actor.starts_with("human:") && event.actor != "process:towermd" {
                return Err("告警不能由任意调用者清除".into());
            }
            if let Some(entry) = state["Alert"].get_mut(&event.obj) {
                entry["resolved"] = json!(true);
            }
        }
        "main.inspected" => {
            if event.actor != "process:towermd" || event.layer != "L5"
                || event.guards.len()!=1 || event.guards[0].name!="main"
                || event.guards[0].epoch==0 || event.epoch!=Some(event.guards[0].epoch)
                || event.data["main"].as_str().is_none_or(|oid| !valid_oid(oid))
                || event.data["complete"].as_bool().is_none()
                || event.data["passed"].as_bool().is_none()
            { return Err("main 检查记录须由守护进程保存真实固定提交".into()); }
            state["MainInspection"] = json!({"eid":event.eid,"main":event.data["main"],
                "previous_main":event.data["previous_main"],"complete":event.data["complete"],"passed":event.data["passed"],
                "error":event.data["error"]});
            if event.data["complete"] == false {
                let alert_id = crate::events::derived_eid(event.data["main"].as_str().unwrap(),
                    "alert.raised.inspection_incomplete")?;
                if let Some(alert) = state["Alert"].get_mut(&alert_id) {
                    // A new failed observation may revisit a previously
                    // cleared SHA; keep the original alert and expose the
                    // latest reason from this accepted observation.
                    alert["resolved"] = json!(false);
                    alert["inspection_eid"] = json!(event.eid);
                    alert["data"]["detail"]["error"] = event.data["error"].clone();
                }
            }
        }
        "task.finish.requested" | "task.finish.started" | "task.finish.completed"
        | "task.finish.interrupted" | "task.g5.completed" | "task.merged" => {
            apply_finish(state, event)?;
        }
        "hold.opened" | "hold.answered" | "hold.decision_reused"
        | "prompt.queued" | "prompt.delivered" => {
            crate::question_projection::apply(state, event)?;
        }
        "snapshot.written" => {
            let proof: crate::snapshot_publication::SnapshotProof =
                serde_json::from_value(event.data.clone()).map_err(|e|e.to_string())?;
            if proof.seq >= event.seq || proof.epoch == 0 || event.obj.is_empty() {
                return Err("快照回执水位或项目无效".into());
            }
            proof.check_receipt(&event.obj, event)?;
        }
        "recovery.completed" => {
            let jump: crate::snapshot::RecoveryJump =
                serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            if event.actor != "process:towermd-recovery"
                || event.layer != "L5"
                || event.obj.is_empty()
                || event.seq <= jump.seq_boundary
                || jump.source_seq > jump.seq_boundary
                || jump.source_epoch > jump.epoch_boundary
                || event.guards.len() != 1
                || event.guards[0].name != "main"
                || event.guards[0].epoch <= jump.epoch_boundary
            {
                return Err("恢复完成缺少越过原边界的新主干 fence".into());
            }
            let mut projection = tasks(state)?;
            let active: Vec<_> = projection
                .runs
                .values()
                .filter(|run| run.state == crate::task_model::RunState::Active)
                .map(|run| run.binding.id.clone())
                .collect();
            for run_id in active {
                // The real recovery.completed receipt revokes all old authority;
                // this is not a fabricated historical lease.expired event.
                projection.apply(&crate::task_model::TaskEvent::Released {
                    run_id,
                    reason: crate::task_model::ReleaseReason::Reassigned,
                })?;
            }
            write_tasks(state, &projection)?;
        }
        "task.created" | "task.claimed" | "task.released" => {
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
        }
        "lease.expired" | "lease.released" => {
            if event.actor != "process:towermd-hub" {
                return Err("租约终止只能来自hub系统事件".into());
            }
            let fences = match &event.lease_delta {
                Some(
                    crate::hub::SystemDelta::Expire { fences }
                    | crate::hub::SystemDelta::Release { fences },
                ) => fences,
                _ => return Err("租约终止事件缺少完整系统原件".into()),
            };
            let mut projection = tasks(state)?;
            let released: Vec<_> = projection
                .runs
                .values()
                .filter(|run| {
                    run.state == crate::task_model::RunState::Active
                        && run.fences.iter().any(|fence| fences.contains(fence))
                })
                .map(|run| run.binding.id.clone())
                .collect();
            for run_id in released {
                projection.apply(&crate::task_model::TaskEvent::Released {
                    run_id,
                    reason: if event.kind == "lease.expired" {
                        crate::task_model::ReleaseReason::Expired
                    } else {
                        crate::task_model::ReleaseReason::Voluntary
                    },
                })?;
            }
            write_tasks(state, &projection)?;
        }
        // Hub lease bookkeeping and snapshot notifications do not certify any
        // business outcome. Their envelopes remain in the checked archive.
        name if name.starts_with("lease.") => {}
        _ => return Err(format!("尚未支持的业务事件：{}", event.kind)),
    }
    Ok(())
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
        if state["Evidence"].as_object().ok_or("Evidence 投影无效")?.values().any(|value| {
            value["run"] == run.binding.id && matches!(value["status"].as_str(), Some("queued" | "running"))
        }) { return Err("运行已有验证请求".into()); }
        task.state = TaskState::Verifying;
        insert_unique(state, "Evidence", &event.obj, json!({
            "id":event.obj,"kind":"finish","task":run.task,"run":run.binding.id,
            "status":"queued","request_eid":event.eid,"request":request,
        }))?;
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
            }
        }
        "task.finish.interrupted" => {
            if event.actor != "process:towermd-verifier" || event.eid != request.interrupted_eid
                || event.host != request.run.binding.host || !event.guards.is_empty()
                || event.data["request_eid"] != entry["request_eid"]
                || event.data["error"].as_str().is_none_or(|error| error.is_empty())
                || !matches!(entry["status"].as_str(), Some("queued" | "running"))
            { return Err("验证中断必须绑定原请求，不能代替完成证据".into()); }
            entry["status"] = json!("interrupted"); entry["error"] = event.data["error"].clone();
            if let Some(task) = projection.tasks.get_mut(&request.run.task) {
                if task.active_run.as_deref() == Some(request.run.binding.id.as_str()) && task.state == TaskState::Verifying {
                    task.state = TaskState::Running;
                }
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
                || event.eid != crate::events::derived_eid(&proof.admission_commit, "task.merged")?
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
            if let Some(alert)=state["Alert"].get_mut(format!("gate-{id}")) {
                alert["resolved"]=json!(true);
            }
        }
        _ => unreachable!(),
    }
    state["Evidence"][id] = entry;
    write_tasks(state, &projection)
}

fn insert_unique(state: &mut Value, kind: &str, id: &str, value: Value) -> Result<(), String> {
    let group = state
        .get_mut(kind)
        .and_then(Value::as_object_mut)
        .ok_or("项目投影格式无效")?;
    match group.get(id) {
        Some(old) if old != &value => Err(format!("{kind} 对象已有不同原件：{id}")),
        Some(_) => Ok(()),
        None => {
            group.insert(id.into(), value);
            Ok(())
        }
    }
}

fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn tasks(state: &Value) -> Result<crate::task_model::TaskProjection, String> {
    Ok(crate::task_model::TaskProjection {
        tasks: serde_json::from_value(state["Task"].clone()).map_err(|e| e.to_string())?,
        runs: serde_json::from_value(state["Run"].clone()).map_err(|e| e.to_string())?,
        paused_all: state["tasks_paused"].as_bool().unwrap_or(false),
    })
}
fn write_tasks(
    state: &mut Value,
    projection: &crate::task_model::TaskProjection,
) -> Result<(), String> {
    state["Task"] = serde_json::to_value(&projection.tasks).map_err(|e| e.to_string())?;
    state["Run"] = serde_json::to_value(&projection.runs).map_err(|e| e.to_string())?;
    state["tasks_paused"] = json!(projection.paused_all);
    Ok(())
}
