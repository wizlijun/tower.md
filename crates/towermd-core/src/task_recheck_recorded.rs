//! Finite App Task recheck request, original receipt and rebuild reducer.
//! Embedded receipts never join Human/Run or global accepted-original roles.
use crate::{
    hub_protocol::{Event, Lease, NewEvent},
    proposals::{
        RecheckFact, RecheckRunBinding, TaskRecheckAction, TaskRecheckBinding,
        TaskRecheckResolution, TaskRecheckView,
    },
    recovery_contract::{
        self,
        recorded::{request_event_eid, RequestBody, RequestMeta, RequestReceipt},
    },
    snapshot::StateProjection,
    tasks::{Run, Task},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolveRecheckAction {
    ResolveTaskRecheck {
        binding: TaskRecheckBinding,
        comment: String,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct RecheckRequest {
    pub(crate) body: RequestBody<ResolveRecheckAction>,
    pub(crate) meta: RequestMeta,
}
impl RecheckRequest {
    pub fn body(&self) -> &RequestBody<ResolveRecheckAction> {
        &self.body
    }
    pub fn metadata(&self) -> &RequestMeta {
        &self.meta
    }
    /// Validate immutable syntax/identity only; this grants no resolution,
    /// source proof, human authority, lease or state transition.
    pub fn prepare(
        project: &str,
        host: &str,
        actor: &str,
        action: &TaskRecheckAction,
    ) -> Result<Self, String> {
        let body = RequestBody {
            project_id: project.into(),
            host: host.into(),
            actor: actor.into(),
            run: action.binding.running.as_ref().map(|r| r.run.clone()),
            action: ResolveRecheckAction::ResolveTaskRecheck {
                binding: action.binding.clone(),
                comment: action.comment.clone(),
            },
        };
        let meta = RequestMeta {
            request_id: action.request_id.clone(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Self::from_recorded(body, meta)
    }
    pub fn binding(&self) -> &TaskRecheckBinding {
        let ResolveRecheckAction::ResolveTaskRecheck { binding, .. } = &self.body.action;
        binding
    }
    pub(crate) fn from_recorded(
        body: RequestBody<ResolveRecheckAction>,
        meta: RequestMeta,
    ) -> Result<Self, String> {
        request_event_eid(&meta.request_id, "task.recheck_resolved")?;
        let human = body
            .actor
            .strip_prefix("human:")
            .filter(|s| !s.is_empty() && !s.contains([':', '/']))
            .ok_or("Task recheck requires an App human actor")?;
        crate::hub::validate_token("human", human).map_err(|e| e.to_string())?;
        crate::hub::validate_token("host", &body.host).map_err(|e| e.to_string())?;
        let ResolveRecheckAction::ResolveTaskRecheck { binding, .. } = &body.action;
        crate::baseline_draft::safe_id(&binding.task_id)?;
        crate::baseline_current::baseline_number(&binding.target_baseline)?;
        recovery_contract::sha256_hex(&binding.task_content_hash)?;
        if body.project_id.is_empty()
            || body.run != binding.running.as_ref().map(|r| r.run.clone())
            || recovery_contract::canonical_hash(&body)? != meta.body_hash
            || binding.publication_ids.windows(2).any(|w| w[0] >= w[1])
            || binding.publication_ids.iter().any(|id| id.is_empty())
        {
            return Err("Task recheck request/project/hash/publication set mismatch".into());
        }
        if let Some(running) = &binding.running {
            crate::hub::validate_token("Run", &running.run).map_err(|e| e.to_string())?;
            if running.fences.is_empty()
                || running.fences.windows(2).any(|w| w[0].name >= w[1].name)
                || running
                    .fences
                    .iter()
                    .any(|f| f.holder != running.run || f.epoch == 0)
                || running
                    .fences
                    .iter()
                    .map(|f| &f.name)
                    .collect::<BTreeSet<_>>()
                    != running.lease_expires_at.keys().collect::<BTreeSet<_>>()
            {
                return Err(
                    "Task recheck requires sorted complete original Run fences and expiries".into(),
                );
            }
        }
        Ok(Self { body, meta })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecheckResolved {
    pub schema: u32,
    pub source_commit: String,
    pub source_seq: u64,
    pub subject: TaskRecheckView,
    pub request_body: RequestBody<ResolveRecheckAction>,
    pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<TaskRecheckResolution>,
}
pub(crate) fn resolved_event(
    request: &RecheckRequest,
    source_commit: &str,
    source_seq: u64,
    subject: TaskRecheckView,
) -> Result<NewEvent, String> {
    let eid = request_event_eid(&request.meta.request_id, "task.recheck_resolved")?;
    let result = TaskRecheckResolution {
        request_id: request.meta.request_id.clone(),
        binding: request.binding().clone(),
        actor: request.body.actor.clone(),
        resolved_eid: eid.clone(),
    };
    let record = RecheckResolved {
        schema: 1,
        source_commit: source_commit.into(),
        source_seq,
        subject,
        request_body: request.body.clone(),
        p3_request: request.meta.clone(),
        p3_receipt: RequestReceipt {
            request_id: request.meta.request_id.clone(),
            body_hash: request.meta.body_hash.clone(),
            result_ref: result,
        },
    };
    Ok(NewEvent {
        eid,
        host: request.body.host.clone(),
        actor: request.body.actor.clone(),
        kind: "task.recheck_resolved".into(),
        layer: "L5".into(),
        obj: request.binding().task_id.clone(),
        guards: request
            .binding()
            .running
            .as_ref()
            .map_or_else(Vec::new, |r| r.fences.clone()),
        data: serde_json::to_value(record).map_err(|e| e.to_string())?,
    })
}
pub(crate) fn decode_resolved(project: &str, event: &NewEvent) -> Result<RecheckResolved, String> {
    if event.kind != "task.recheck_resolved" {
        return Err("not a finite Task recheck resolution".into());
    }
    let record: RecheckResolved =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    let request =
        RecheckRequest::from_recorded(record.request_body.clone(), record.p3_request.clone())?;
    record.subject.validate()?;
    recovery_contract::object_id(&record.source_commit)?;
    let task: Task = serde_yaml::from_str(&record.subject.task_text).map_err(|e| e.to_string())?;
    let mut ids = record
        .subject
        .reasons
        .iter()
        .map(|f| f.publication_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    if record.schema != 1
        || request.body.project_id != project
        || record.subject.binding != *request.binding()
        || crate::proposals::task_content_hash(&task)? != request.binding().task_content_hash
        || task.spec.id != event.obj
        || ids != request.binding().publication_ids
        || record
            .subject
            .reasons
            .iter()
            .any(|fact| fact.task != event.obj)
        || *event
            != resolved_event(
                &request,
                &record.source_commit,
                record.source_seq,
                record.subject.clone(),
            )?
    {
        return Err(
            "Task recheck original differs from exact source/subject/request/receipt".into(),
        );
    }
    Ok(record)
}

/// Shared pure subject constructor. Callers provide only a source-proved current
/// projection and actual lease head, never a client-supplied Task or fence set.
pub(crate) fn recheck_view(
    projection: &StateProjection,
    leases: &BTreeMap<String, Lease>,
    baseline: &str,
    id: &str,
    now: u64,
) -> Result<TaskRecheckView, String> {
    let raw = projection
        .objects
        .get("Task")
        .and_then(|objects| objects.get(id))
        .ok_or("Task recheck subject does not exist")?;
    let task: Task = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
    if task.spec.id != id || task.object_type != "Task" {
        return Err("Task recheck subject ID/type changed".into());
    }
    let mut reasons: Vec<RecheckFact> = task
        .extra
        .get("recheck_facts")
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or_default();
    reasons.sort_by(|a, b| a.publication_id.cmp(&b.publication_id));
    if reasons
        .windows(2)
        .any(|w| w[0].publication_id == w[1].publication_id)
        || reasons.iter().any(|f| f.task != id)
    {
        return Err("Task recheck pending set is not exact".into());
    }
    let active = projection
        .objects
        .get("Run")
        .into_iter()
        .flat_map(|runs| runs.values())
        .map(|value| serde_json::from_value::<Run>(value.clone()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|r| r.task == id && matches!(r.state.as_str(), "running" | "verifying" | "held"))
        .collect::<Vec<_>>();
    let running =
        match task.state.as_str() {
            "ready" if active.is_empty() => None,
            "running" | "verifying" | "held" if active.len() == 1 => {
                let run = &active[0];
                let mut fences = run.leases.clone();
                fences.sort_by(|a, b| a.name.cmp(&b.name));
                let names = std::iter::once(&task.spec.module)
                    .chain(task.spec.touches.iter())
                    .map(|id| format!("module:{id}"))
                    .collect::<BTreeSet<_>>();
                if fences
                    .iter()
                    .map(|f| f.name.clone())
                    .collect::<BTreeSet<_>>()
                    != names
                    || fences.len() != names.len()
                    || fences.iter().any(|f| {
                        f.holder != run.id
                            || leases
                                .get(&f.name)
                                .is_none_or(|l| l.fence() != *f || l.expires_at <= now)
                    })
                {
                    return Err("Task recheck needs the original complete live Run fences".into());
                }
                Some(RecheckRunBinding {
                    run: run.id.clone(),
                    lease_expires_at: fences
                        .iter()
                        .map(|f| (f.name.clone(), leases[&f.name].expires_at))
                        .collect(),
                    fences,
                })
            }
            _ => return Err(
                "Task recheck requires ready or exactly one current running/verifying/held Task"
                    .into(),
            ),
        };
    Ok(TaskRecheckView {
        binding: TaskRecheckBinding {
            task_id: id.into(),
            target_baseline: baseline.into(),
            task_content_hash: crate::proposals::task_content_hash(&task)?,
            publication_ids: reasons.iter().map(|f| f.publication_id.clone()).collect(),
            running,
        },
        task_text: serde_yaml::to_string(&task).map_err(|e| e.to_string())?,
        reasons,
    })
}

/// Optional snapshot cache only; full actual events remain the reconstructible
/// authority. Kept outside Task so unknown Task keys still enter its exact hash.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecheckProjection {
    required: BTreeMap<String, BTreeMap<String, RecheckFact>>,
    resolved: BTreeMap<String, TaskRecheckBinding>,
}
impl RecheckProjection {
    pub(crate) fn reviewed(&self, task: &str, publication: &str) -> bool {
        self.resolved.values().any(|binding| {
            binding.task_id == task && binding.publication_ids.iter().any(|id| id == publication)
        })
    }
}
fn render(projection: &mut StateProjection) -> Result<(), String> {
    let Some(cache) = projection.rechecks.as_ref() else {
        return Ok(());
    };
    for (task, facts) in &cache.required {
        let resolved = cache
            .resolved
            .values()
            .filter(|b| b.task_id == *task)
            .flat_map(|b| b.publication_ids.iter())
            .collect::<BTreeSet<_>>();
        let pending = facts
            .iter()
            .filter(|(id, _)| !resolved.contains(id))
            .map(|(_, fact)| fact.clone())
            .collect::<Vec<_>>();
        let raw = projection
            .objects
            .get_mut("Task")
            .and_then(|tasks| tasks.get_mut(task))
            .ok_or("Task recheck cache lacks Task")?;
        raw["needs_recheck"] = serde_json::Value::Bool(!pending.is_empty());
        raw["recheck_facts"] = serde_json::to_value(pending).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn ensure_cache(projection: &mut StateProjection) -> Result<(), String> {
    if projection.rechecks.is_some() {
        return Ok(());
    }
    let mut cache = RecheckProjection::default();
    for (task, raw) in projection
        .objects
        .get("Task")
        .into_iter()
        .flat_map(|tasks| tasks.iter())
    {
        let facts: Vec<RecheckFact> = raw
            .get("recheck_facts")
            .map(|value| serde_json::from_value(value.clone()).map_err(|e| e.to_string()))
            .transpose()?
            .unwrap_or_default();
        for fact in facts {
            cache
                .required
                .entry(task.clone())
                .or_default()
                .insert(fact.publication_id.clone(), fact);
        }
    }
    projection.rechecks = Some(cache);
    Ok(())
}
pub(crate) fn remember_required(
    projection: &mut StateProjection,
    fact: &RecheckFact,
) -> Result<(), String> {
    ensure_cache(projection)?;
    let old = projection
        .rechecks
        .get_or_insert_with(Default::default)
        .required
        .entry(fact.task.clone())
        .or_default()
        .insert(fact.publication_id.clone(), fact.clone());
    if old.is_some_and(|old| old != *fact) {
        return Err("Task recheck immutable reason changed".into());
    }
    render(projection)
}
pub(crate) fn apply_resolved(
    projection: &mut StateProjection,
    event: &Event,
) -> Result<(), String> {
    if event.stale {
        return Ok(());
    }
    let project = event.data["request_body"]["project_id"]
        .as_str()
        .ok_or("resolution lacks project")?;
    let record = decode_resolved(project, &crate::recovery_outbox::original_of(event))?;
    let binding = &record.subject.binding;
    let task: Task = serde_json::from_value(
        projection
            .objects
            .get("Task")
            .and_then(|objects| objects.get(&event.obj))
            .cloned()
            .ok_or("resolution lacks Task")?,
    )
    .map_err(|e| e.to_string())?;
    if crate::proposals::task_content_hash(&task)? != binding.task_content_hash {
        return Err("resolution Task changed at actual prefix".into());
    }
    let ids = task
        .extra
        .get("recheck_facts")
        .map(|raw| {
            serde_json::from_value::<Vec<RecheckFact>>(raw.clone()).map_err(|e| e.to_string())
        })
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.publication_id)
        .collect::<BTreeSet<_>>();
    if ids != binding.publication_ids.iter().cloned().collect() {
        return Err("resolution publications changed at actual prefix".into());
    }
    ensure_cache(projection)?;
    projection
        .rechecks
        .get_or_insert_with(Default::default)
        .resolved
        .insert(event.eid.clone(), binding.clone());
    render(projection)
}
pub(crate) fn invalidate_guarded(projection: &mut StateProjection) -> Result<(), String> {
    if let Some(cache) = projection.rechecks.as_mut() {
        cache
            .resolved
            .retain(|_, binding| binding.running.is_none());
    }
    render(projection)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Restored {
    schema: u32,
    #[serde(deserialize_with = "recovery_contract::deserialize_event")]
    original: Event,
}
pub(crate) fn resolution_recovery_receipt(
    project: &str,
    original: &Event,
    host: &str,
) -> Result<NewEvent, String> {
    decode_resolved(project, &crate::recovery_outbox::original_of(original))?;
    crate::recovery_outbox::validate_actual(
        &crate::recovery_outbox::original_of(original),
        original,
    )?;
    if original.stale {
        return Err("Task recheck recovery requires original nonstale acceptance".into());
    }
    crate::hub::validate_token("host", host).map_err(|e| e.to_string())?;
    let hash = recovery_contract::canonical_hash(original)?;
    Ok(NewEvent {
        eid: crate::events::derived_eid(&hash, "task.recheck_receipt_restored")?,
        host: host.into(),
        actor: "process:towermd-daemon".into(),
        kind: "task.recheck_receipt_restored".into(),
        layer: "L5".into(),
        obj: original.obj.clone(),
        guards: vec![],
        data: serde_json::to_value(Restored {
            schema: 1,
            original: original.clone(),
        })
        .map_err(|e| e.to_string())?,
    })
}
pub(crate) fn restored_resolution(project: &str, event: &Event) -> Result<Option<Event>, String> {
    if event.kind != "task.recheck_receipt_restored" {
        return Ok(None);
    }
    let record: Restored = serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    if record.schema != 1
        || event.stale
        || event.legacy
        || event.epoch.is_some()
        || event.cost_usd.is_some()
        || event.seq <= record.original.seq
        || crate::recovery_outbox::original_of(event)
            != resolution_recovery_receipt(project, &record.original, &event.host)?
    {
        return Err("invalid finite Task recheck receipt audit".into());
    }
    Ok(Some(record.original))
}
pub(crate) fn apply_restored(
    projection: &mut StateProjection,
    event: &Event,
) -> Result<(), String> {
    let project = event.data["original"]["data"]["request_body"]["project_id"]
        .as_str()
        .ok_or("Task recheck receipt lacks project")?;
    let original = restored_resolution(project, event)?.ok_or("not Task recheck receipt")?;
    let record = decode_resolved(project, &crate::recovery_outbox::original_of(&original))?;
    // A guarded pre-rebuild receipt is audit-only; it cannot revive a Run.
    if record.subject.binding.running.is_some() {
        return Ok(());
    }
    let Some(raw) = projection
        .objects
        .get("Task")
        .and_then(|objects| objects.get(&event.obj))
    else {
        return Ok(());
    };
    let mut current: serde_json::Value = raw.clone();
    let task: Task = serde_yaml::from_str(&record.subject.task_text).map_err(|e| e.to_string())?;
    let mut source = serde_json::to_value(task).map_err(|e| e.to_string())?;
    for value in [&mut current, &mut source] {
        let map = value.as_object_mut().ok_or("Task is not object")?;
        for key in ["state", "needs_recheck", "recheck_facts"] {
            map.remove(key);
        }
    }
    if current != source {
        return Ok(());
    }
    ensure_cache(projection)?;
    projection
        .rechecks
        .get_or_insert_with(Default::default)
        .resolved
        .insert(original.eid, record.subject.binding);
    render(projection)
}
