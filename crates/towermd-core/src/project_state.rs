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
            let entry = json!({
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
            ) || event.layer != "L1"
            {
                return Err("基线出版必须由守护进程确认".into());
            }
            let approval = event.data["approval_eid"]
                .as_str()
                .ok_or("出版缺少批准原件 ID")?;
            let approved = state["BaselineApproval"]
                .get(approval)
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
            insert_unique(state, "Baseline", &event.obj, entry)?;
            let mut projection = tasks(state)?;
            let current_tag = format!("towermd/{}", event.obj);
            for task in projection.tasks.values_mut() {
                if task.baseline != current_tag
                    && task.state != crate::task_model::TaskState::Merged
                {
                    task.needs_review = true;
                }
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
        }
        "alert.cleared" => {
            if !event.actor.starts_with("human:") && event.actor != "process:towermd" {
                return Err("告警不能由任意调用者清除".into());
            }
            if let Some(entry) = state["Alert"].get_mut(&event.obj) {
                entry["resolved"] = json!(true);
            }
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
        name if name.starts_with("lease.") || name == "snapshot.written" => {}
        _ => return Err(format!("尚未支持的业务事件：{}", event.kind)),
    }
    Ok(())
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
