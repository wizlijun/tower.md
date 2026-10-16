use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::catalog::{validate_paths, validate_protected_paths, Catalog};
use crate::hub_protocol::{
    AcquireLease, AppendReceipt, Event, Fence, HubError, HubProtocol, Lease, NewEvent,
    ReleaseLease, RenewLease,
};
use crate::okf::Document;
use crate::snapshot::StateProjection;

const LEASE_TTL_MS: u64 = 15 * 60 * 1000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub id: String,
    pub title: String,
    pub module: String,
    #[serde(default)]
    pub touches: Vec<String>,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TaskCommand {
    Status,
    Add {
        task: TaskSpec,
    },
    Claim {
        id: String,
        harness: Option<String>,
        harness_version: Option<String>,
    },
    Show {
        id: Option<String>,
    },
    Release,
}

impl<'de> Deserialize<'de> for TaskCommand {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            action: String,
            #[serde(default)]
            task: Option<TaskSpec>,
            #[serde(default)]
            id: Option<String>,
            #[serde(default)]
            harness: Option<String>,
            #[serde(default)]
            harness_version: Option<String>,
        }
        let input = Input::deserialize(deserializer)?;
        if input.action != "claim" && (input.harness.is_some() || input.harness_version.is_some()) {
            return Err(serde::de::Error::custom(
                "harness reporting is only valid for claim",
            ));
        }
        match (input.action.as_str(), input.task, input.id) {
            ("status", None, None) => Ok(Self::Status),
            ("add", Some(task), None) => Ok(Self::Add { task }),
            ("claim", None, Some(id)) => Ok(Self::Claim {
                id,
                harness: input.harness,
                harness_version: input.harness_version,
            }),
            ("show", None, id) => Ok(Self::Show { id }),
            ("release", None, None) => Ok(Self::Release),
            _ => Err(serde::de::Error::custom(
                "invalid task command or arguments",
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Task {
    #[serde(rename = "type")]
    pub object_type: String,
    #[serde(flatten)]
    pub spec: TaskSpec,
    pub state: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl<'de> Deserialize<'de> for Task {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(deserializer)?;
        let map = value
            .as_object_mut()
            .ok_or_else(|| serde::de::Error::custom("Task must be an object"))?;
        let object_type = map
            .remove("type")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| serde::de::Error::custom("Task type required"))?;
        let state = map
            .remove("state")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| serde::de::Error::custom("Task state required"))?;
        let mut spec = serde_json::Map::new();
        for key in [
            "id",
            "title",
            "module",
            "touches",
            "allowed_paths",
            "depends_on",
            "acceptance",
            "decisions",
        ] {
            if let Some(value) = map.remove(key) {
                spec.insert(key.into(), value);
            }
        }
        let spec = serde_json::from_value(Value::Object(spec)).map_err(serde::de::Error::custom)?;
        Ok(Self {
            object_type,
            spec,
            state,
            extra: map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    #[serde(rename = "type")]
    pub object_type: String,
    pub id: String,
    pub task: String,
    pub host: String,
    pub worktree: String,
    pub branch: String,
    pub state: String,
    pub leases: Vec<Fence>,
    pub cost_usd: Option<f64>,
    pub task_card: Value,
    #[serde(default)]
    pub self_reported: Option<SelfReported>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelfReported {
    pub harness: String,
    pub version: String,
}

#[derive(Default)]
pub struct TaskService {
    tasks: BTreeMap<String, Task>,
    runs: BTreeMap<String, Run>,
    pub pending_claim: Option<PendingClaim>,
    active_runs: BTreeSet<String>,
    bindings: BTreeMap<(String, String, String), BTreeSet<String>>,
    worktrees: BTreeMap<(String, String), BTreeSet<String>>,
    #[cfg(test)]
    active_visits: std::cell::Cell<u64>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Change {
    task: Task,
    #[serde(default)]
    run: Option<Run>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    self_reported: Option<SelfReported>,
}

impl TaskService {
    pub fn restore(&mut self, state: &StateProjection) -> Result<(), String> {
        let mut restored = Self::default();
        if let Some(tasks) = state.objects.get("Task") {
            for (id, value) in tasks {
                let task: Task = serde_json::from_value(value.clone())
                    .map_err(|e| format!("invalid restored Task {id}: {e}"))?;
                if task.spec.id != *id || task.object_type != "Task" {
                    return Err("restored Task ID/type mismatch".into());
                }
                restored.tasks.insert(id.clone(), task);
            }
        }
        if let Some(runs) = state.objects.get("Run") {
            for (id, value) in runs {
                let run: Run = serde_json::from_value(value.clone())
                    .map_err(|e| format!("invalid restored Run {id}: {e}"))?;
                if run.id != *id || run.object_type != "Run" {
                    return Err("restored Run ID/type mismatch".into());
                }
                restored.install_run(run);
            }
        }
        restored.pending_claim = self.pending_claim.take();
        *self = restored;
        Ok(())
    }
    pub(crate) fn prepare(&self, event: &Event) -> Result<Option<Change>, String> {
        if event.stale
            || !matches!(
                event.kind.as_str(),
                "task.state" | "task.claimed" | "task.released"
            )
        {
            return Ok(None);
        }
        let mut data = event.data.clone();
        if let Some(value) = data.get("run").filter(|v| !v.is_null()) {
            data["run"] = crate::snapshot::recorded_run(event, value)?;
        } else if event.kind == "task.claimed" {
            return Err("claim event has no Run".into());
        }
        let change: Change = serde_json::from_value(data)
            .map_err(|e| format!("invalid task event {}: {e}", event.seq))?;
        if change.task.object_type != "Task"
            || change
                .run
                .as_ref()
                .is_some_and(|r| r.object_type != "Run" || r.task != change.task.spec.id)
        {
            return Err("invalid task event object type or Run task binding".into());
        }
        Ok(Some(change))
    }
    pub(crate) fn apply_prepared(&mut self, change: Option<Change>) {
        if let Some(change) = change {
            self.tasks.insert(change.task.spec.id.clone(), change.task);
            if let Some(run) = change.run {
                self.install_run(run);
            }
        }
    }
    pub fn apply(&mut self, event: &Event) -> Result<(), String> {
        let change = self.prepare(event)?;
        self.apply_prepared(change);
        Ok(())
    }
    fn install_run(&mut self, run: Run) {
        if let Some(old) = self.runs.remove(&run.id) {
            self.unindex(&old);
        }
        if run.state == "running" {
            self.active_runs.insert(run.id.clone());
            self.bindings
                .entry((run.host.clone(), run.worktree.clone(), run.branch.clone()))
                .or_default()
                .insert(run.id.clone());
            self.worktrees
                .entry((run.host.clone(), run.worktree.clone()))
                .or_default()
                .insert(run.id.clone());
        }
        self.runs.insert(run.id.clone(), run);
    }
    fn unindex(&mut self, run: &Run) {
        self.active_runs.remove(&run.id);
        let binding = (run.host.clone(), run.worktree.clone(), run.branch.clone());
        if let Some(ids) = self.bindings.get_mut(&binding) {
            ids.remove(&run.id);
            if ids.is_empty() {
                self.bindings.remove(&binding);
            }
        }
        let worktree = (run.host.clone(), run.worktree.clone());
        if let Some(ids) = self.worktrees.get_mut(&worktree) {
            ids.remove(&run.id);
            if ids.is_empty() {
                self.worktrees.remove(&worktree);
            }
        }
    }
    fn dependencies_merged(&self, task: &TaskSpec) -> bool {
        task.depends_on.iter().all(|id| {
            self.tasks
                .get(id)
                .is_some_and(|dependency| dependency.state == "merged")
        })
    }

    fn binding(
        &self,
        catalog: &Catalog,
        host: &str,
        run_override: Option<&str>,
    ) -> Result<Option<Run>, String> {
        if let Some(id) = run_override {
            return self
                .runs
                .get(id)
                .filter(|run| run.host == host && run.state == "running")
                .cloned()
                .map(Some)
                .ok_or_else(|| {
                    format!(
                        "run {id} is not active on this host; {}",
                        self.local_runs(host)
                    )
                });
        }
        let key = (
            host.to_string(),
            catalog.root.to_string_lossy().into_owned(),
            catalog.branch.clone(),
        );
        #[cfg(test)]
        self.active_visits.set(self.active_visits.get() + 1);
        Ok(self
            .bindings
            .get(&key)
            .and_then(|ids| ids.first())
            .and_then(|id| self.runs.get(id))
            .cloned())
    }

    fn local_runs(&self, host: &str) -> String {
        let runs: Vec<_> = self.runs.values().filter(|run| run.host == host).map(|run| {
            json!({"id": run.id, "task": run.task, "worktree": run.worktree, "branch": run.branch, "state": run.state})
        }).collect();
        format!("local runs: {}", json!(runs))
    }
}

impl TaskService {
    pub fn execute(
        &mut self,
        hub: &mut dyn HubProtocol,
        catalog: &Catalog,
        host: &str,
        command: TaskCommand,
        run_override: Option<&str>,
        now_ms: u64,
    ) -> Result<Value, TaskFailure> {
        let projection = &*self;
        let bound = projection.binding(catalog, host, run_override)?;
        if let Some(run) = &bound {
            renew_run(hub, host, run, now_ms)?;
        }
        match command {
            TaskCommand::Status => Ok(json!({
                "project_id": catalog.config.project_id(),
                "seq": hub.contiguous_seq(),
                "tasks": projection.tasks.values().collect::<Vec<_>>(),
                "runs": projection.runs.values().collect::<Vec<_>>(),
                "leases": current_leases(hub,now_ms)?.values().collect::<Vec<_>>(),
            })),
            TaskCommand::Add { task: spec } => {
                validate_task(&spec, catalog)?;
                if projection.tasks.contains_key(&spec.id) || catalog.objects.contains_key(&spec.id)
                {
                    return Err(format!("task ID {} already exists", spec.id).into());
                }
                let state = if projection.dependencies_merged(&spec) {
                    "ready"
                } else {
                    "proposed"
                };
                let task = Task {
                    object_type: object_type("Task")?,
                    spec,
                    state: state.into(),
                    extra: BTreeMap::new(),
                };
                append_change(hub, host, "task.state", &task, None, vec![], now_ms)?;
                Ok(json!(task))
            }
            TaskCommand::Show { id: Some(id) } => {
                let task = projection
                    .tasks
                    .get(&id)
                    .ok_or_else(|| format!("unknown task {id}"))?;
                render_task_card(task, catalog).map_err(Into::into)
            }
            TaskCommand::Show { id: None } => {
                let mut run = bound.ok_or_else(|| {
                    format!(
                        "no run bound to current worktree and branch; {}",
                        projection.local_runs(host)
                    )
                })?;
                let task = projection
                    .tasks
                    .get(&run.task)
                    .ok_or_else(|| format!("unknown task {}", run.task))?;
                run.task_card = render_task_card(task, catalog)?;
                Ok(json!(run))
            }
            TaskCommand::Release => {
                let run = bound.ok_or_else(|| {
                    format!(
                        "no run bound to current worktree and branch; {}",
                        projection.local_runs(host)
                    )
                })?;
                release_run(hub, host, projection, run, now_ms, "process:towermd-cli")
                    .map_err(Into::into)
            }
            TaskCommand::Claim {
                id,
                harness,
                harness_version,
            } => {
                let self_reported = match (harness, harness_version) {
                    (None, None) => None,
                    (Some(harness), Some(version))
                        if !harness.trim().is_empty() && !version.trim().is_empty() =>
                    {
                        Some(SelfReported { harness, version })
                    }
                    _ => {
                        return Err(
                            "harness and harness_version must both be nonempty or both omitted"
                                .into(),
                        )
                    }
                };
                let mut task = projection
                    .tasks
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| format!("unknown task {id}"))?;
                validate_task(&task.spec, catalog)?;
                if !projection.dependencies_merged(&task.spec) {
                    return Err(format!("task {id} dependencies are missing or not merged").into());
                }
                if catalog.branch != format!("t/{id}") {
                    return Err(format!(
                        "claim {id} requires an existing t/{id} branch and target worktree"
                    )
                    .into());
                }
                if bound.is_some()
                    || projection.worktrees.contains_key(&(
                        host.to_string(),
                        catalog.root.to_string_lossy().into_owned(),
                    ))
                {
                    return Err(format!(
                        "current worktree already has an active run; {}",
                        projection.local_runs(host)
                    )
                    .into());
                }
                let names = lease_names(&task.spec);
                let leases = current_leases(hub, now_ms)?;
                if names.iter().any(|name| leases.contains_key(name)) {
                    let available: Vec<_> = projection
                        .tasks
                        .values()
                        .filter(|candidate| {
                            matches!(candidate.state.as_str(), "ready" | "proposed")
                                && projection.dependencies_merged(&candidate.spec)
                                && validate_task(&candidate.spec, catalog).is_ok()
                                && lease_names(&candidate.spec)
                                    .iter()
                                    .all(|name| !leases.contains_key(name))
                        })
                        .map(|candidate| candidate.spec.id.as_str())
                        .collect();
                    return Err(TaskFailure {
                        message: format!(
                            "task {id} module lease is occupied; available tasks: {}",
                            json!(available)
                        ),
                        owners: Some(
                            names
                                .iter()
                                .filter_map(|name| leases.get(name).cloned())
                                .collect(),
                        ),
                    });
                }
                if task.state != "ready" && task.state != "proposed" {
                    return Err(format!("task {id} is not ready").into());
                }
                task.state = "running".into();
                let mut run = Run {
                    object_type: object_type("Run")?,
                    id: Uuid::now_v7().to_string(),
                    task: id,
                    host: host.into(),
                    worktree: catalog.root.to_string_lossy().into_owned(),
                    branch: catalog.branch.clone(),
                    state: "running".into(),
                    leases: Vec::new(),
                    cost_usd: None,
                    task_card: render_task_card(&task, catalog)?,
                    self_reported,
                    extra: BTreeMap::new(),
                };
                let receipt = hub
                    .lease_acquire(
                        AcquireLease {
                            host: host.into(),
                            names,
                            holder: run.id.clone(),
                            ttl_ms: LEASE_TTL_MS,
                        },
                        now_ms,
                    )
                    .map_err(TaskFailure::from)?;
                run.leases = receipt.leases.iter().map(Lease::fence).collect();
                let event =
                    change_event(host, "task.claimed", &task, Some(&run), run.leases.clone());
                self.pending_claim = Some(PendingClaim {
                    event: event.clone(),
                    run_id: run.id.clone(),
                });
                match hub.append(event.clone(), now_ms) {
                    Ok(AppendReceipt::Event(recorded)) if !recorded.stale => {
                        self.pending_claim = None;
                    }
                    Err(HubError::Uncertain(error)) => {
                        return Err(format!(
                            "claim 结果不确定，重开后协调；eid={} run_id={}: {error}",
                            event.eid, run.id
                        )
                        .into())
                    }
                    result => {
                        self.pending_claim = None;
                        let original = match result {
                            Err(e) => e.to_string(),
                            _ => "run lease expired; claim the task again".into(),
                        };
                        match compensate_claim(hub, host, &run.leases, now_ms) {
                            Ok(_) => return Err(original.into()),
                            Err(e) => {
                                return Err(
                                    format!("{original}; claim compensation failed: {e}").into()
                                )
                            }
                        }
                    }
                }
                Ok(json!(run))
            }
        }
    }

    pub fn reconcile_pending(&mut self, hub: &mut dyn HubProtocol, now: u64) -> Result<(), String> {
        let Some(pending) = self.pending_claim.clone() else {
            return Ok(());
        };
        match hub.append(pending.event.clone(), now) {
            Ok(AppendReceipt::Event(event)) if event.stale => {
                compensate_claim(hub, &pending.event.host, &pending.event.guards, now)
                    .map_err(|e| format!("pending claim expired; compensation failed: {e}"))?;
                self.pending_claim = None;
                Err(format!(
                    "pending claim {} expired; claim again",
                    pending.event.eid
                ))
            }
            Ok(_) => {
                self.pending_claim = None;
                Ok(())
            }
            Err(error) => Err(format!(
                "claim 结果不确定，重开后协调；eid={} run_id={}: {error}",
                pending.event.eid, pending.run_id
            )),
        }
    }
    pub fn renew_binding(
        &mut self,
        hub: &mut dyn HubProtocol,
        catalog: &Catalog,
        host: &str,
        run_override: Option<&str>,
        now_ms: u64,
    ) -> Result<(), String> {
        let projection = &*self;
        let Some(run) = projection.binding(catalog, host, run_override)? else {
            return Ok(());
        };
        let leases = current_leases(hub, now_ms)?;
        if run.leases.is_empty() || run.leases.iter().any(|fence| !active(fence, &leases)) {
            return if run_override.is_some() {
                Err(format!("run {} 不再持有有效租约", run.id))
            } else {
                Ok(())
            };
        }
        renew_run(hub, host, &run, now_ms)
    }
}
fn renew_run(hub: &mut dyn HubProtocol, host: &str, run: &Run, now_ms: u64) -> Result<(), String> {
    hub.lease_renew(
        RenewLease {
            host: host.into(),
            fences: run.leases.clone(),
            ttl_ms: LEASE_TTL_MS,
        },
        now_ms,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

impl TaskService {
    pub fn expire(
        &mut self,
        hub: &mut dyn HubProtocol,
        host: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        let projection = &*self;
        let leases = current_leases(hub, now_ms)?;
        for id in &projection.active_runs {
            #[cfg(test)]
            self.active_visits.set(self.active_visits.get() + 1);
            let run = &projection.runs[id];
            if run.leases.is_empty() || run.leases.iter().any(|fence| !active(fence, &leases)) {
                release_run(
                    hub,
                    host,
                    projection,
                    run.clone(),
                    now_ms,
                    "process:towermd-daemon",
                )?;
            }
        }
        Ok(())
    }
}

fn active(fence: &Fence, leases: &BTreeMap<String, Lease>) -> bool {
    leases
        .get(&fence.name)
        .is_some_and(|lease| lease.holder == fence.holder && lease.epoch == fence.epoch)
}

fn compensate_claim(
    hub: &mut dyn HubProtocol,
    host: &str,
    fences: &[Fence],
    now: u64,
) -> Result<(), String> {
    let leases = current_leases(hub, now)?;
    let mut errors = Vec::new();
    for fence in fences.iter().filter(|f| active(f, &leases)) {
        // A fence can change again after the head read. Release independently
        // so that a replaced owner cannot prevent cleanup of the other grants.
        match hub.lease_release(
            ReleaseLease {
                host: host.into(),
                fences: vec![fence.clone()],
            },
            now,
        ) {
            Ok(_) | Err(HubError::FenceExpired { .. }) => {}
            Err(e @ HubError::Uncertain(_)) => return Err(e.to_string()),
            Err(e) => errors.push(e.to_string()),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn release_run(
    hub: &mut dyn HubProtocol,
    host: &str,
    projection: &TaskService,
    mut run: Run,
    now_ms: u64,
    actor: &str,
) -> Result<Value, String> {
    let mut task = projection
        .tasks
        .get(&run.task)
        .cloned()
        .ok_or_else(|| format!("unknown task {}", run.task))?;
    task.state = "ready".into();
    run.state = "released".into();
    let leases = current_leases(hub, now_ms)?;
    let fences: Vec<_> = run
        .leases
        .iter()
        .filter(|fence| active(fence, &leases))
        .cloned()
        .collect();
    if !fences.is_empty() {
        hub.lease_release(
            ReleaseLease {
                host: host.into(),
                fences,
            },
            now_ms,
        )
        .map_err(|e| e.to_string())?;
    }
    let mut event = change_event(host, "task.released", &task, Some(&run), vec![]);
    event.actor = actor.into();
    require_current(&append_event(hub, event, now_ms)?)?;
    Ok(json!(task))
}

#[derive(Clone, Debug)]
pub struct PendingClaim {
    pub event: NewEvent,
    pub run_id: String,
}
fn current_leases(hub: &mut dyn HubProtocol, now: u64) -> Result<BTreeMap<String, Lease>, String> {
    hub.subscribe(hub.contiguous_seq(), now)
        .map(|b| b.head.leases)
        .map_err(|e| e.to_string())
}
fn change_event(
    host: &str,
    kind: &str,
    task: &Task,
    run: Option<&Run>,
    guards: Vec<Fence>,
) -> NewEvent {
    new_event(
        host,
        kind,
        if kind == "task.state" { "L4" } else { "L5" },
        &task.spec.id,
        json!(Change {
            task: task.clone(),
            run: run.cloned(),
            self_reported: if kind == "task.claimed" {
                run.and_then(|r| r.self_reported.clone())
            } else {
                None
            }
        }),
        guards,
    )
}
fn append_event(hub: &mut dyn HubProtocol, event: NewEvent, now: u64) -> Result<Event, String> {
    match hub.append(event, now).map_err(|e| e.to_string())? {
        AppendReceipt::Event(e) => Ok(*e),
        AppendReceipt::Duplicate { seq } => Err(format!("event {seq} already checkpointed")),
    }
}
fn append_change(
    hub: &mut dyn HubProtocol,
    host: &str,
    kind: &str,
    task: &Task,
    run: Option<&Run>,
    guards: Vec<Fence>,
    now: u64,
) -> Result<Event, String> {
    let event = append_event(hub, change_event(host, kind, task, run, guards), now)?;
    require_current(&event)?;
    Ok(event)
}

fn require_current(event: &Event) -> Result<(), String> {
    if event.stale {
        Err("run lease expired; claim the task again".into())
    } else {
        Ok(())
    }
}

fn new_event(
    host: &str,
    kind: &str,
    layer: &str,
    obj: &str,
    data: Value,
    guards: Vec<Fence>,
) -> NewEvent {
    NewEvent {
        eid: Uuid::now_v7().to_string(),
        host: host.into(),
        actor: "process:towermd-cli".into(),
        kind: kind.into(),
        layer: layer.into(),
        obj: obj.into(),
        data,
        guards,
    }
}

fn object_type(name: &str) -> Result<String, String> {
    crate::types::lookup(name)
        .map(|registered| registered.name.into())
        .ok_or_else(|| format!("unregistered type {name}"))
}

fn modules(task: &TaskSpec) -> BTreeSet<&str> {
    std::iter::once(task.module.as_str())
        .chain(task.touches.iter().map(String::as_str))
        .collect()
}

fn lease_names(task: &TaskSpec) -> Vec<String> {
    modules(task)
        .into_iter()
        .map(|module| format!("module:{module}"))
        .collect()
}

fn document<'a>(catalog: &'a Catalog, id: &str, kind: &str) -> Result<&'a Document, String> {
    catalog
        .objects
        .get(id)
        .filter(|document| document.doc_type() == Some(kind))
        .ok_or_else(|| format!("G0: {id} is not an existing {kind} in authoritative main"))
}

fn safe_id(id: &str) -> bool {
    id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn expanded_document(document: &Document) -> Result<Value, String> {
    let mut value = serde_json::to_value(document.front())
        .map_err(|error| format!("cannot render execution context: {error}"))?;
    value["body"] = json!(document.body());
    Ok(value)
}

fn render_task_card(task: &Task, catalog: &Catalog) -> Result<Value, String> {
    let module_ids = modules(&task.spec);
    let mut interface_ids = BTreeSet::new();
    let mut module_context = Vec::new();
    for id in &module_ids {
        let module = document(catalog, id, "Module")?;
        if let Some(interfaces) = module.get("interfaces") {
            let references: Vec<String> = serde_yaml::from_value(interfaces.clone())
                .map_err(|error| format!("invalid interfaces for module {id}: {error}"))?;
            interface_ids.extend(references);
        }
        module_context.push(expanded_document(module)?);
    }
    for (id, interface) in &catalog.objects {
        if interface.doc_type() == Some("Interface")
            && ["from", "to"].iter().any(|key| {
                interface
                    .get_str(key)
                    .is_some_and(|module| module_ids.contains(module))
            })
        {
            interface_ids.insert(id.clone());
        }
    }
    let expand = |id: &str, kind: &str| expanded_document(document(catalog, id, kind)?);
    let acceptance: Vec<_> = task
        .spec
        .acceptance
        .iter()
        .map(|id| expand(id, "Acceptance"))
        .collect::<Result<_, _>>()?;
    let interfaces: Vec<_> = interface_ids
        .iter()
        .map(|id| expand(id, "Interface"))
        .collect::<Result<_, _>>()?;
    let decisions: Vec<_> = task
        .spec
        .decisions
        .iter()
        .map(|id| expand(id, "Decision"))
        .collect::<Result<_, _>>()?;
    let docs = catalog.config.docs_dir();
    let mut protected_paths = vec![".git".to_string(), ".towermd".to_string()];
    protected_paths.extend(
        [
            "intents",
            "arch",
            "specs",
            "baselines",
            "state",
            "tower.canvas",
        ]
        .iter()
        .map(|name| format!("{docs}/{name}")),
    );
    protected_paths.extend(catalog.protected_paths.iter().cloned());
    protected_paths.sort();
    protected_paths.dedup();
    let mut card = json!(task);
    card["revision"] = json!(catalog.revision);
    card["context"] = json!({"modules": module_context, "acceptance": acceptance, "interfaces": interfaces, "decisions": decisions});
    card["restrictions"] = json!({
        "allowed_paths": task.spec.allowed_paths,
        "protected_paths": protected_paths,
        "prohibitions": [
            "不得修改任何 L0–L2 慢层对象、tower.canvas 或快照，无论对象位于哪个目录。",
            "实际修改范围仅为任务 allowed_paths；Module.paths 是上限，不扩大任务授权。",
            "不得直接写 .towermd、用户文档目录或快照；状态变更只能通过 towermd 命令。",
            "未通过 towermd finish 不得合并；没有证据不得宣称任务已完成。"
        ]
    });
    card["instructions"] = json!({
        "interaction": "需要人工输入时使用 towermd ask；返回 deferred 时结束回合并说明等待事项。",
        "checkpoints": "每完成一个可验证的小步就提交一次。",
        "report": "收尾时使用 towermd finish --report <file> 提交报告；报告仅作线索，进度只认证据。",
        "availability": "M0-2 尚未提供 ask、finish、临时规则与人工插话接口；需要这些能力时停止并说明阻塞，不得绕过门禁。"
    });
    Ok(card)
}

fn validate_task(task: &TaskSpec, catalog: &Catalog) -> Result<(), String> {
    if !safe_id(&task.id) || task.title.trim().is_empty() {
        return Err("G0: task requires a safe nonempty ID and title".into());
    }
    if task
        .depends_on
        .iter()
        .any(|dependency| dependency == &task.id || !safe_id(dependency))
    {
        return Err("G0: task dependency ID is invalid or refers to itself".into());
    }
    let modules = modules(task);
    let mut paths = Vec::new();
    for module in &modules {
        let document = document(catalog, module, "Module")?;
        let value = document
            .get("paths")
            .ok_or_else(|| format!("G0: module {module} has no paths"))?;
        let module_paths: Vec<String> = serde_yaml::from_value(value.clone())
            .map_err(|error| format!("G0: invalid paths for module {module}: {error}"))?;
        if module_paths.is_empty() {
            return Err(format!("G0: module {module} has no paths"));
        }
        paths.extend(module_paths);
    }
    if task.allowed_paths.is_empty() {
        return Err("G0: allowed_paths must not be empty".into());
    }
    validate_paths(&task.allowed_paths, &paths, catalog.config.docs_dir())?;
    validate_protected_paths(&task.allowed_paths, &catalog.protected_paths)?;
    let mut primary_acceptance = false;
    for acceptance in &task.acceptance {
        let document = document(catalog, acceptance, "Acceptance")?;
        let criteria: Vec<String> = serde_yaml::from_value(
            document.get("criteria").cloned().unwrap_or_default(),
        )
        .map_err(|error| format!("G0: acceptance {acceptance} has invalid criteria: {error}"))?;
        if criteria.is_empty() || criteria.iter().any(|criterion| criterion.trim().is_empty()) {
            return Err(format!(
                "G0: acceptance {acceptance} requires nonempty criteria"
            ));
        }
        let module = document
            .get_str("module")
            .ok_or_else(|| format!("G0: acceptance {acceptance} has no module"))?;
        if !modules.contains(module) {
            return Err(format!(
                "G0: acceptance {acceptance} belongs to unrelated module {module}"
            ));
        }
        primary_acceptance |= module == task.module;
    }
    if !primary_acceptance {
        return Err("G0: task requires acceptance for its primary module".into());
    }
    for decision in &task.decisions {
        document(catalog, decision, "Decision")?;
    }
    Ok(())
}

pub fn execute(
    session: &mut crate::project_session::ProjectSession,
    catalog: &Catalog,
    host: &str,
    command: TaskCommand,
    run: Option<&str>,
    now: u64,
) -> Result<Value, TaskFailure> {
    session.execute(catalog, host, command, run, now)
}
pub fn renew_binding(
    session: &mut crate::project_session::ProjectSession,
    catalog: &Catalog,
    host: &str,
    run: Option<&str>,
    now: u64,
) -> Result<(), String> {
    session.renew_binding(catalog, host, run, now)
}
pub fn expire(
    session: &mut crate::project_session::ProjectSession,
    host: &str,
    now: u64,
) -> Result<(), String> {
    session.tick(host, now)
}

#[derive(Clone, Debug, Serialize)]
pub struct TaskFailure {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<Lease>>,
}
impl std::fmt::Display for TaskFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for TaskFailure {}
impl From<String> for TaskFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            owners: None,
        }
    }
}
impl From<&str> for TaskFailure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}
impl From<HubError> for TaskFailure {
    fn from(error: HubError) -> Self {
        let message = error.to_string();
        let owners = if let HubError::LeaseConflict { owners } = error {
            Some(owners)
        } else {
            None
        };
        Self { message, owners }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn catalog(root: &std::path::Path) -> Catalog {
        let mut objects = BTreeMap::new();
        for (id, text) in [
            ("A", "---\ntype: Module\nid: A\npaths: [src/**]\n---\n"),
            (
                "AC",
                "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
            ),
        ] {
            objects.insert(id.into(), Document::parse(text).unwrap());
        }
        Catalog {
            root: root.into(),
            common_dir: root.join(".git"),
            branch: "t/T".into(),
            revision: "authority".into(),
            config: crate::config::Config::new("project", "docs/tower").unwrap(),
            objects,
            object_paths: BTreeMap::new(),
            protected_paths: vec![],
        }
    }
    #[test]
    fn released_run_history_is_not_visited_by_binding_or_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = catalog(dir.path());
        let mut hub = crate::hub::Hub::open(&dir.path().join("hub")).unwrap();
        let receipt = hub
            .lease_acquire(
                AcquireLease {
                    host: "host".into(),
                    names: vec!["module:A".into()],
                    holder: "z-active".into(),
                    ttl_ms: 1000,
                },
                1,
            )
            .unwrap();
        let mut state = StateProjection::default();
        state.objects.entry("Task".into()).or_default().insert(
            "T".into(),
            json!({"type":"Task","id":"T","title":"Task","module":"A","state":"running"}),
        );
        for id in (0..10_000)
            .map(|n| format!("released-{n:05}"))
            .chain(std::iter::once("z-active".into()))
        {
            let active = id == "z-active";
            state.objects.entry("Run".into()).or_default().insert(id.clone(),json!({"type":"Run","id":id,"task":"T","host":"host","worktree":catalog.root,"branch":catalog.branch,"state":if active{"running"}else{"released"},"leases":if active{receipt.leases.iter().map(Lease::fence).collect::<Vec<_>>()}else{vec![]},"cost_usd":null,"task_card":{}}));
        }
        let mut service = TaskService::default();
        service.restore(&state).unwrap();
        for _ in 0..100 {
            assert_eq!(
                service.binding(&catalog, "host", None).unwrap().unwrap().id,
                "z-active"
            );
            service.expire(&mut hub, "host", 2).unwrap();
        }
        assert_eq!(service.active_visits.get(), 200);
    }
    struct FailClaim {
        inner: crate::hub::Hub,
        partial: bool,
        failed: bool,
    }
    impl HubProtocol for FailClaim {
        fn append(&mut self, event: NewEvent, now: u64) -> Result<AppendReceipt, HubError> {
            if event.kind == "task.claimed" && !self.failed {
                self.failed = true;
                self.inner.fail_next_append(self.partial);
            }
            self.inner.append(event, now)
        }
        fn subscribe(
            &mut self,
            a: u64,
            n: u64,
        ) -> Result<crate::hub_protocol::SubscriptionBatch, HubError> {
            self.inner.subscribe(a, n)
        }
        fn lease_acquire(
            &mut self,
            r: AcquireLease,
            n: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            self.inner.lease_acquire(r, n)
        }
        fn lease_renew(
            &mut self,
            r: RenewLease,
            n: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            self.inner.lease_renew(r, n)
        }
        fn lease_release(
            &mut self,
            r: ReleaseLease,
            n: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            self.inner.lease_release(r, n)
        }
        fn snapshot_ack(
            &mut self,
            a: crate::hub_protocol::SnapshotAck,
            c: Value,
        ) -> Result<(), HubError> {
            HubProtocol::snapshot_ack(&mut self.inner, a, c)
        }
        fn rebuild(
            &mut self,
            r: crate::hub_protocol::RebuildSeed,
        ) -> Result<crate::hub_protocol::RebuildReceipt, HubError> {
            self.inner.rebuild(r)
        }
        fn contiguous_seq(&self) -> u64 {
            self.inner.contiguous_seq()
        }
    }
    #[test]
    fn actual_claim_io_uncertainty_retains_the_product_eid_and_reopens_with_same_pending_event() {
        for partial in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let catalog = catalog(dir.path());
            let path = dir.path().join("hub");
            let protocol = FailClaim {
                inner: crate::hub::Hub::open(&path).unwrap(),
                partial,
                failed: false,
            };
            let mut session =
                crate::project_session::ProjectSession::new(Box::new(protocol), 0).unwrap();
            session
                .execute(
                    &catalog,
                    "host",
                    TaskCommand::Add {
                        task: TaskSpec {
                            id: "T".into(),
                            title: "Task".into(),
                            module: "A".into(),
                            touches: vec![],
                            allowed_paths: vec!["src/**".into()],
                            depends_on: vec![],
                            acceptance: vec!["AC".into()],
                            decisions: vec![],
                        },
                    },
                    None,
                    1,
                )
                .unwrap();
            let error = session
                .execute(
                    &catalog,
                    "host",
                    TaskCommand::Claim {
                        id: "T".into(),
                        harness: None,
                        harness_version: None,
                    },
                    None,
                    2,
                )
                .unwrap_err();
            let pending = session.tasks.pending_claim.clone().unwrap();
            assert!(error.to_string().contains(&pending.event.eid));
            assert!(error.to_string().contains(&pending.run_id));
            assert!(error.to_string().contains("结果不确定"));
            assert_eq!(session.hub.contiguous_seq(), 2);
            drop(session);
            let mut reopened = crate::project_session::ProjectSession::open(&path).unwrap();
            if partial {
                assert!(!reopened.projection.objects.contains_key("Run"));
                assert_eq!(reopened.leases(3).len(), 1);
            } else {
                assert_eq!(
                    reopened.projection.objects["Run"][&pending.run_id]["state"],
                    "running"
                );
            }
            reopened.tasks.pending_claim = Some(pending.clone());
            reopened.reconcile_pending(3).unwrap();
            assert!(reopened.tasks.pending_claim.is_none());
            assert_eq!(
                reopened.projection.objects["Run"][&pending.run_id]["state"],
                "running"
            );
            assert_eq!(
                reopened
                    .events(0)
                    .iter()
                    .filter(|e| e.eid == pending.event.eid)
                    .count(),
                1
            );
            assert_eq!(reopened.last_seq(), 3);
        }
    }
}
