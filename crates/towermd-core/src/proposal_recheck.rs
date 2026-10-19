//! Descriptive P4 task-review wire and pure identities. None of these DTOs
//! authorizes a resolution, publication, claim, or Run lease.
use crate::{hub_protocol::Fence, recovery_contract, tasks::Task};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RecheckFact {
    pub task: String,
    pub old_baseline: String,
    pub new_baseline: String,
    pub publication_id: String,
    pub reasons: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RecheckRunBinding {
    pub run: String,
    pub fences: Vec<Fence>,
    pub lease_expires_at: std::collections::BTreeMap<String, u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskRecheckBinding {
    pub task_id: String,
    pub target_baseline: String,
    pub task_content_hash: String,
    pub publication_ids: Vec<String>,
    pub running: Option<RecheckRunBinding>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskRecheckView {
    pub binding: TaskRecheckBinding,
    pub task_text: String,
    pub reasons: Vec<RecheckFact>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskRecheckAction {
    pub request_id: String,
    pub binding: TaskRecheckBinding,
    pub comment: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskRecheckResolution {
    pub request_id: String,
    pub binding: TaskRecheckBinding,
    pub actor: String,
    pub resolved_eid: String,
}

/// Hash the complete current Task, including lifecycle state and unknown OKF
/// data. The two derived recheck keys alone are bound independently by the
/// exact publication set; no Run/lease state is supplied by this pure helper.
pub fn task_content_hash(task: &Task) -> Result<String, String> {
    if task.object_type != "Task" {
        return Err("task recheck subject is not a Task".into());
    }
    crate::baseline_draft::safe_id(&task.spec.id)?;
    let mut value = serde_json::to_value(task).map_err(|error| error.to_string())?;
    let fields = value.as_object_mut().ok_or("Task is not an object")?;
    fields.remove("needs_recheck");
    fields.remove("recheck_facts");
    recovery_contract::canonical_hash(&value)
}

/// Clear only the reviewed immutable publication identities. A later
/// publication remains pending even when an older resolution is retried.
pub fn unresolved_publications(
    pending: &BTreeSet<String>,
    resolved: &[String],
) -> BTreeSet<String> {
    let resolved = resolved.iter().cloned().collect::<BTreeSet<_>>();
    pending.difference(&resolved).cloned().collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecheckRequired {
    pub schema: u32,
    pub fact: RecheckFact,
    pub source_commit: String,
    #[serde(deserialize_with = "recovery_contract::deserialize_event")]
    pub created: crate::hub_protocol::Event,
    pub request_body: recovery_contract::recorded::RequestBody<RequireRecheckAction>,
    pub p3_request: recovery_contract::recorded::RequestMeta,
    pub p3_receipt: recovery_contract::recorded::RequestReceipt<RecheckFact>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RequireRecheckAction {
    RequireTaskRecheck {
        fact: RecheckFact,
        source_commit: String,
        created_eid: String,
    },
}

pub(crate) fn required_event(
    project: &str,
    fact: &RecheckFact,
    source_commit: &str,
    created: &crate::hub_protocol::Event,
) -> Result<crate::hub_protocol::NewEvent, String> {
    use recovery_contract::recorded::{RequestBody, RequestMeta, RequestReceipt};
    let commit = created.data["commit"]
        .as_str()
        .ok_or("publication creation lacks commit")?;
    let request_id = crate::events::derived_eid(
        commit,
        &format!("task.recheck_required.request:{}", fact.task),
    )?;
    let body = RequestBody {
        project_id: project.into(),
        actor: "process:towermd-baseline".into(),
        host: created.host.clone(),
        run: None,
        action: RequireRecheckAction::RequireTaskRecheck {
            fact: fact.clone(),
            source_commit: source_commit.into(),
            created_eid: created.eid.clone(),
        },
    };
    let hash = recovery_contract::canonical_hash(&body)?;
    let record = RecheckRequired {
        schema: 1,
        fact: fact.clone(),
        source_commit: source_commit.into(),
        created: created.clone(),
        request_body: body,
        p3_request: RequestMeta {
            request_id: request_id.clone(),
            body_hash: hash.clone(),
        },
        p3_receipt: RequestReceipt {
            request_id,
            body_hash: hash,
            result_ref: fact.clone(),
        },
    };
    Ok(crate::hub_protocol::NewEvent {
        eid: crate::events::derived_eid(commit, &format!("task.recheck_required:{}", fact.task))?,
        host: created.host.clone(),
        actor: "process:towermd-baseline".into(),
        kind: "task.recheck_required".into(),
        layer: "L5".into(),
        obj: fact.task.clone(),
        guards: vec![],
        data: serde_json::to_value(record).map_err(|error| error.to_string())?,
    })
}

pub(crate) fn decode_required(
    project: &str,
    event: &crate::hub_protocol::NewEvent,
) -> Result<RecheckRequired, String> {
    let record: RecheckRequired =
        serde_json::from_value(event.data.clone()).map_err(|error| error.to_string())?;
    recovery_contract::object_id(&record.source_commit)?;
    crate::baseline_current::baseline_number(&record.fact.old_baseline)?;
    crate::baseline_current::baseline_number(&record.fact.new_baseline)?;
    if record.request_body.project_id != project
        || record.schema != 1
        || record.fact.reasons.is_empty()
        || record.fact.reasons.iter().any(|reason| reason.is_empty())
        || record.created.stale
        || record.created.kind != "baseline.created"
        || record.fact.publication_id != record.created.obj
        || record.fact.new_baseline != record.created.data["target"]
        || *event
            != required_event(
                &record.request_body.project_id,
                &record.fact,
                &record.source_commit,
                &record.created,
            )?
    {
        return Err(
            "task recheck marker differs from exact publication/source/request/receipt".into(),
        );
    }
    Ok(record)
}

pub(crate) fn required_record(
    event: &crate::hub_protocol::Event,
) -> Result<RecheckRequired, String> {
    let project = event.data["request_body"]["project_id"]
        .as_str()
        .ok_or("recheck marker lacks project")?;
    decode_required(project, &crate::recovery_outbox::original_of(event))
}

pub(crate) fn apply_required(
    projection: &mut crate::snapshot::StateProjection,
    event: &crate::hub_protocol::Event,
) -> Result<(), String> {
    if event.stale {
        return Ok(());
    }
    let record = required_record(event)?;
    let task = projection
        .objects
        .get_mut("Task")
        .and_then(|tasks| tasks.get_mut(&record.fact.task))
        .ok_or("recheck marker lacks original Task")?;
    let mut facts: Vec<RecheckFact> = task
        .get("recheck_facts")
        .map(|value| serde_json::from_value(value.clone()).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    if let Some(old) = facts
        .iter()
        .find(|fact| fact.publication_id == record.fact.publication_id)
    {
        if old != &record.fact {
            return Err("same publication has conflicting task recheck reasons".into());
        }
    } else {
        facts.push(record.fact);
        facts.sort_by(|a, b| a.publication_id.cmp(&b.publication_id));
    }
    task["needs_recheck"] = serde_json::Value::Bool(!facts.is_empty());
    task["recheck_facts"] = serde_json::to_value(facts).map_err(|error| error.to_string())?;
    crate::proposals::remember_required(projection, &required_record(event)?.fact)
}

impl TaskRecheckView {
    /// Pure wire/original-text validation for native readers. This does not
    /// authenticate an App, prove current Git/Hub state or authorize a clear.
    pub fn validate(&self) -> Result<(), String> {
        let task: Task = serde_yaml::from_str(&self.task_text).map_err(|e| e.to_string())?;
        let target = crate::baseline_current::baseline_number(&self.binding.target_baseline)?;
        if task.spec.id != self.binding.task_id
            || task_content_hash(&task)? != self.binding.task_content_hash
            || self
                .binding
                .publication_ids
                .windows(2)
                .any(|w| w[0] >= w[1])
            || self
                .reasons
                .iter()
                .map(|fact| fact.publication_id.clone())
                .collect::<Vec<_>>()
                != self.binding.publication_ids
        {
            return Err("Task recheck view text/hash/publication set differs".into());
        }
        for fact in &self.reasons {
            let old = crate::baseline_current::baseline_number(&fact.old_baseline)?;
            let new = crate::baseline_current::baseline_number(&fact.new_baseline)?;
            if fact.task != self.binding.task_id
                || fact.publication_id.is_empty()
                || old >= new
                || new > target
                || fact.reasons.is_empty()
                || fact.reasons.iter().any(|reason| reason.trim().is_empty())
            {
                return Err("Task recheck view reason differs from subject/baseline".into());
            }
        }
        match (&self.binding.running, task.state.as_str()) {
            (None, "ready") => {}
            (Some(running), "running" | "verifying" | "held") => {
                crate::hub::validate_token("Run", &running.run).map_err(|e| e.to_string())?;
                let expected = std::iter::once(&task.spec.module)
                    .chain(task.spec.touches.iter())
                    .map(|id| format!("module:{id}"))
                    .collect::<BTreeSet<_>>();
                if running.fences.len() != expected.len()
                    || running.fences.is_empty()
                    || running.fences.windows(2).any(|w| w[0].name >= w[1].name)
                    || running
                        .fences
                        .iter()
                        .map(|f| f.name.clone())
                        .collect::<BTreeSet<_>>()
                        != expected
                    || running
                        .fences
                        .iter()
                        .any(|f| f.holder != running.run || f.epoch == 0)
                    || running
                        .lease_expires_at
                        .keys()
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        != expected
                    || running.lease_expires_at.values().any(|expiry| *expiry == 0)
                {
                    return Err(
                        "Task recheck view lacks complete sorted Run fences/expiries".into(),
                    );
                }
            }
            _ => return Err("Task recheck view lifecycle and running binding differ".into()),
        }
        Ok(())
    }
}
