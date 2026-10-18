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
use crate::recovery_outbox::{RecoveryInputs, RecoveryOutbox};
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelfReported {
    pub harness: String,
    pub version: String,
}

#[derive(Clone, Default)]
pub struct TaskService {
    tasks: BTreeMap<String, Task>,
    runs: BTreeMap<String, Run>,
    pub pending_claim: Option<PendingClaim>,
    active_runs: BTreeSet<String>,
    writes: Vec<TaskWrite>,
    // Derived capabilities rebuilt only from complete verified actual history.
    archived_receipts: BTreeMap<String, crate::recovery_git::VerifiedTaskReceipt>,
    bindings: BTreeMap<(String, String, String), BTreeSet<String>>,
    worktrees: BTreeMap<(String, String), BTreeSet<String>>,
    #[cfg(test)]
    active_visits: std::cell::Cell<u64>,
}

#[derive(Clone)]
pub(crate) struct TaskWrite {
    original: NewEvent,
    actual: Option<Event>,
}
pub(crate) fn validate_writes(
    before: &RecoveryInputs,
    after: &RecoveryInputs,
    writes: &[TaskWrite],
) -> Result<(), String> {
    let values = writes
        .iter()
        .map(|write| (&write.original, write.actual.as_ref()))
        .collect::<Vec<_>>();
    before
        .validate_task_writes(after, &values)
        .map_err(|error| {
            if writes.is_empty() {
                return error;
            }
            let identities = writes
                .iter()
                .map(|write| {
                    format!(
                        "eid={} run_id={}",
                        write.original.eid,
                        write.original.data["run"]["id"].as_str().unwrap_or("none")
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            if writes.iter().any(|write| write.actual.is_some()) {
                format!("accepted request requires synchronization; original {identities}: {error}")
            } else {
                format!("original task request requires synchronization; {identities}: {error}")
            }
        })
}
fn append_task(
    hub: &mut dyn HubProtocol,
    outbox: &mut RecoveryOutbox,
    original: NewEvent,
    now: u64,
    writes: &mut Vec<TaskWrite>,
    archived: Option<&crate::recovery_git::VerifiedTaskReceipt>,
) -> Result<AppendReceipt, HubError> {
    let result = match archived {
        Some(receipt) => {
            crate::recovery_outbox::append_task_durable(hub, outbox, original.clone(), now, receipt)
        }
        None => crate::recovery_outbox::append_durable(hub, outbox, original.clone(), now),
    };
    let actual = match &result {
        Ok(AppendReceipt::Event(actual)) => Some((**actual).clone()),
        _ => outbox.read_inputs().ok().and_then(|inputs| {
            inputs
                .confirmed()
                .iter()
                .find(|actual| crate::recovery_outbox::original_of(actual) == original)
                .cloned()
        }),
    };
    writes.push(TaskWrite { original, actual });
    result
}

#[derive(Debug)]
pub enum ReconcileOutcome {
    Completed,
    Expired(NewEvent),
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
    pub(crate) fn take_writes(&mut self) -> Vec<TaskWrite> {
        std::mem::take(&mut self.writes)
    }
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
    pub(crate) fn restore_pending(&mut self, inputs: &RecoveryInputs) -> Result<(), String> {
        let mut claims = pending_task_events(inputs)
            .into_iter()
            .filter(|event| event.kind == "task.claimed");
        let Some(event) = claims.next() else {
            return Ok(());
        };
        if claims.next().is_some() {
            return Err("multiple unresolved original task claims require coordination".into());
        }
        let run: Run =
            serde_json::from_value(event.data["run"].clone()).map_err(|error| error.to_string())?;
        if event.actor != "process:towermd-cli"
            || event.layer != "L5"
            || run.id.is_empty()
            || run.task != event.obj
            || run.leases != event.guards
        {
            return Err("durable pending claim lacks original complete Run binding".into());
        }
        self.pending_claim = Some(PendingClaim {
            event,
            run_id: run.id,
        });
        Ok(())
    }
    pub(crate) fn restore_receipts(
        &mut self,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        history: &crate::recovery_git::VerifiedRecoveryHistory,
        through: u64,
    ) -> Result<(), String> {
        let originals = pending_task_events(inputs);
        if originals.is_empty() {
            // No repair capability is needed. Source/ack and final Session
            // proofs remain mandatory in the surrounding preparation.
            self.archived_receipts.clear();
            return Ok(());
        }
        // Repair only receipts already persisted in the selected snapshot.
        // Live-tail retries retain their original Hub/source-fault semantics;
        // recovery staging gains its new receipts only after the final snapshot
        // is selected and the ordinary full preparation proves that source.
        let published_through = history
            .snapshot_sources()
            .clone()
            .select()?
            .map_or(0, |saved| saved.snapshot.through_seq)
            .min(through);
        self.archived_receipts = history.task_receipts(catalog, published_through, &originals)?;
        Ok(())
    }
    pub(crate) fn prepare(&self, event: &Event) -> Result<Option<Change>, String> {
        if event.stale
            || !matches!(
                event.kind.as_str(),
                "task.state"
                    | "task.claimed"
                    | "task.released"
                    | "merge.done"
                    | "finish.started"
                    | "finish.running"
                    | "finish.completed"
                    | "finish.interrupted"
            )
        {
            return Ok(None);
        }
        if event.kind == "merge.done" {
            crate::snapshot::merge_evidence(event)?;
        }
        if event.kind == "finish.completed" {
            if let Some(run) = event.data["run"]["id"]
                .as_str()
                .and_then(|id| self.runs.get(id))
            {
                if event.data["evidence"]["id"].as_str().is_some_and(|id| {
                    crate::finish::status_in(run, id)
                        .is_ok_and(|job| job.phase == crate::finish::JobPhase::Recording)
                }) {
                    return Ok(None);
                }
            }
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
        // Keep the same terminal rule as StateProjection while retaining the original audit event.
        if event.kind != "merge.done"
            && self
                .tasks
                .get(&change.task.spec.id)
                .is_some_and(|task| task.state == "merged")
        {
            return Ok(None);
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
        if matches!(run.state.as_str(), "running" | "verifying" | "held") {
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
                .filter(|run| {
                    run.host == host
                        && matches!(run.state.as_str(), "running" | "verifying" | "held")
                })
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

    pub(crate) fn has_maintenance_work(&self) -> bool {
        !self.active_runs.is_empty() || self.pending_claim.is_some() || !self.writes.is_empty()
    }

    pub(crate) fn needs_reconciliation(&self, inputs: &RecoveryInputs) -> bool {
        self.pending_claim.is_some() || !pending_task_events(inputs).is_empty()
    }

    /// Read-only historical binding for an expired Ask recipient. This does not
    /// acquire/renew leases or make an inactive Run eligible for a Task action.
    pub(crate) fn ask_recorded_run(&self, id: &str) -> Option<Run> {
        self.runs.get(id).cloned()
    }

    // Finish alone may inspect its durable verifying/held Run. Ordinary claim,
    // release and maintenance binding semantics remain unchanged.
    pub(crate) fn finish_context(
        &self,
        catalog: &Catalog,
        host: &str,
        run: Option<&str>,
    ) -> Result<(Task, Run), String> {
        let matching = self
            .runs
            .values()
            .filter(|r| {
                r.host == host
                    && r.worktree == catalog.root.to_string_lossy()
                    && r.branch == catalog.branch
                    && matches!(r.state.as_str(), "running" | "verifying" | "held")
                    && run.is_none_or(|id| r.id == id)
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err("finish requires one exact active Task Run".into());
        }
        let run = matching[0];
        let task = self.tasks.get(&run.task).ok_or("finish Run has no Task")?;
        if !self.dependencies_merged(&task.spec)
            || task
                .extra
                .get("needs_recheck")
                .is_some_and(|v| v != &Value::Bool(false))
        {
            return Err("finish dependencies/recheck block execution".into());
        }
        Ok((task.clone(), run.clone()))
    }

    pub(crate) fn check_context(
        &self,
        catalog: &Catalog,
        host: &str,
        run_override: Option<&str>,
    ) -> Result<(Task, Run), String> {
        let run = self
            .binding(catalog, host, run_override)?
            .ok_or_else(|| "no active run for this worktree".to_string())?;
        let task = self
            .tasks
            .get(&run.task)
            .ok_or_else(|| format!("active run references missing task {}", run.task))?;
        if !self.dependencies_merged(&task.spec) {
            return Err("task dependencies are not merged".into());
        }
        if task
            .extra
            .get("needs_recheck")
            .is_some_and(|value| value != &Value::Bool(false))
        {
            return Err("task requires recheck".into());
        }
        Ok((task.clone(), run))
    }

    fn local_runs(&self, host: &str) -> String {
        let runs: Vec<_> = self.runs.values().filter(|run| run.host == host && run.state == "running").map(|run| {
            json!({"id": run.id, "task": run.task, "worktree": run.worktree, "branch": run.branch, "state": run.state})
        }).collect();
        format!("local runs: {}", json!(runs))
    }
}

impl TaskService {
    pub(crate) fn read_status(
        &self,
        hub: &mut dyn HubProtocol,
        catalog: &Catalog,
        host: &str,
        run: Option<&str>,
        now: u64,
    ) -> Result<Value, TaskFailure> {
        self.binding(catalog, host, run)?;
        Ok(json!({
            "project_id": catalog.config.project_id(),
            "tasks": self.tasks.values().collect::<Vec<_>>(),
            "runs": self.runs.values().collect::<Vec<_>>(),
            "leases": current_leases(hub, now)?.values().collect::<Vec<_>>(),
        }))
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "explicit outbox and exact producer capture preserve task durability"
    )]
    pub fn execute(
        &mut self,
        hub: &mut dyn HubProtocol,
        catalog: &Catalog,
        host: &str,
        command: TaskCommand,
        run_override: Option<&str>,
        now_ms: u64,
        outbox: &mut RecoveryOutbox,
    ) -> Result<Value, TaskFailure> {
        self.execute_captured(hub, catalog, host, command, run_override, now_ms, outbox)
            .0
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "explicit outbox and exact producer capture preserve task durability"
    )]
    pub(crate) fn execute_captured(
        &mut self,
        hub: &mut dyn HubProtocol,
        catalog: &Catalog,
        host: &str,
        command: TaskCommand,
        run_override: Option<&str>,
        now_ms: u64,
        outbox: &mut RecoveryOutbox,
    ) -> (Result<Value, TaskFailure>, Option<Event>) {
        if let Err(error) = outbox
            .read_inputs()
            .and_then(|inputs| inputs.probe(catalog))
        {
            return (Err(error.into()), None);
        }
        let bound = match self.binding(catalog, host, run_override) {
            Ok(bound) => bound,
            Err(error) => return (Err(error.into()), None),
        };
        let renewal = match &bound {
            Some(run) => match renew_run(hub, host, run, now_ms) {
                Ok(receipt) => Some(receipt.event),
                Err(error) => return (Err(error.into()), None),
            },
            None => None,
        };
        let mut writes = vec![];
        let result = self.execute_bound(
            hub,
            catalog,
            host,
            command,
            bound,
            now_ms,
            outbox,
            &mut writes,
        );
        self.writes.extend(writes);
        (result, renewal)
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "explicit outbox and exact producer capture preserve task durability"
    )]
    fn execute_bound(
        &mut self,
        hub: &mut dyn HubProtocol,
        catalog: &Catalog,
        host: &str,
        command: TaskCommand,
        bound: Option<Run>,
        now_ms: u64,
        outbox: &mut RecoveryOutbox,
        writes: &mut Vec<TaskWrite>,
    ) -> Result<Value, TaskFailure> {
        let projection = &*self;
        match command {
            TaskCommand::Status => Ok(json!({
                "project_id": catalog.config.project_id(),
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
                append_change(
                    hub,
                    host,
                    "task.state",
                    &task,
                    None,
                    vec![],
                    now_ms,
                    outbox,
                    writes,
                )?;
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
                release_run(
                    hub,
                    host,
                    projection,
                    run,
                    now_ms,
                    "process:towermd-cli",
                    outbox,
                    writes,
                )
                .map_err(Into::into)
            }
            TaskCommand::Claim {
                id,
                harness,
                harness_version,
            } => {
                if let Some(pending) = &self.pending_claim {
                    return Err(format!("claim still requires coordination of the original request; eid={} run_id={}", pending.event.eid, pending.run_id).into());
                }
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
                if task.state == "proposed" {
                    task.state = "ready".into();
                    append_change(
                        hub,
                        host,
                        "task.state",
                        &task,
                        None,
                        vec![],
                        now_ms,
                        outbox,
                        writes,
                    )?;
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
                match append_task(hub, outbox, event.clone(), now_ms, writes, None) {
                    Ok(AppendReceipt::Event(recorded)) if !recorded.stale => {
                        // Session clears the original only after post-accept synchronization succeeds.
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
                        match compensate_claim(hub, host, &run.leases, now_ms, outbox) {
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

    pub fn reconcile_pending(
        &mut self,
        hub: &mut dyn HubProtocol,
        catalog: &Catalog,
        now: u64,
        outbox: &mut RecoveryOutbox,
    ) -> Result<ReconcileOutcome, String> {
        let inputs = outbox.read_inputs()?;
        inputs.probe(catalog)?;
        let mut originals = pending_task_events(&inputs)
            .into_iter()
            .map(|event| (event.eid.clone(), event))
            .collect::<BTreeMap<_, _>>();
        if let Some(pending) = &self.pending_claim {
            if originals
                .get(&pending.event.eid)
                .is_some_and(|event| event != &pending.event)
            {
                return Err("original pending claim differs from durable intent".into());
            }
            originals.insert(pending.event.eid.clone(), pending.event.clone());
        }
        let mut writes = vec![];
        let result = (|| {
            for original in originals.into_values() {
                match append_task(
                    hub,
                    outbox,
                    original.clone(),
                    now,
                    &mut writes,
                    self.archived_receipts.get(&original.eid),
                ) {
                    Ok(AppendReceipt::Event(event)) if event.stale => {
                        if original.kind == "task.claimed" {
                            compensate_claim(hub, &original.host, &original.guards, now, outbox)
                                .map_err(|error| {
                                    format!("pending claim expired; compensation failed: {error}")
                                })?;
                        }
                        return Ok(ReconcileOutcome::Expired(original));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        return Err(format!(
                            "task 结果不确定，重开后协调；eid={}: {error}",
                            original.eid
                        ))
                    }
                }
            }
            Ok(ReconcileOutcome::Completed)
        })();
        self.writes.extend(writes);
        result
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
        renew_run(hub, host, &run, now_ms).map(|_| ())
    }
}
fn renew_run(
    hub: &mut dyn HubProtocol,
    host: &str,
    run: &Run,
    now_ms: u64,
) -> Result<crate::hub_protocol::LeaseReceipt, String> {
    hub.lease_renew(
        RenewLease {
            host: host.into(),
            fences: run.leases.clone(),
            ttl_ms: LEASE_TTL_MS,
        },
        now_ms,
    )
    .map_err(|e| e.to_string())
}

impl TaskService {
    pub fn expire(
        &mut self,
        hub: &mut dyn HubProtocol,
        catalog: &Catalog,
        host: &str,
        now_ms: u64,
        outbox: &mut RecoveryOutbox,
    ) -> Result<(), String> {
        // No event, lease or source capability is produced by an empty pass.
        if self.active_runs.is_empty() {
            return Ok(());
        }
        outbox.read_inputs()?.probe(catalog)?;
        let mut writes = vec![];
        let result = (|| {
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
                        outbox,
                        &mut writes,
                    )?;
                }
            }
            Ok(())
        })();
        self.writes.extend(writes);
        result
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
    _outbox: &mut RecoveryOutbox,
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

#[allow(
    clippy::too_many_arguments,
    reason = "explicit outbox and exact producer capture preserve task durability"
)]
fn release_run(
    hub: &mut dyn HubProtocol,
    host: &str,
    projection: &TaskService,
    mut run: Run,
    now_ms: u64,
    actor: &str,
    outbox: &mut RecoveryOutbox,
    writes: &mut Vec<TaskWrite>,
) -> Result<Value, String> {
    let mut task = projection
        .tasks
        .get(&run.task)
        .cloned()
        .ok_or_else(|| format!("unknown task {}", run.task))?;
    if let Some(original) = pending_task_events(&outbox.read_inputs()?)
        .into_iter()
        .find(|event| event.kind == "task.released" && event.data["run"]["id"] == run.id)
    {
        let task = original.data["task"].clone();
        require_current(&append_event(hub, original, now_ms, outbox, writes)?)?;
        return Ok(task);
    }
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
    require_current(&append_event(hub, event, now_ms, outbox, writes)?)?;
    Ok(json!(task))
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingClaim {
    pub event: NewEvent,
    pub run_id: String,
}
fn pending_task_events(inputs: &RecoveryInputs) -> Vec<NewEvent> {
    let mut events = inputs
        .pending()
        .iter()
        .filter(|event| {
            matches!(
                event.kind.as_str(),
                "task.state" | "task.claimed" | "task.released"
            )
        })
        .map(|event| (event.eid.clone(), event.clone()))
        .collect::<BTreeMap<_, _>>();
    for actual in inputs.confirmed().iter().filter(|event| {
        matches!(
            event.kind.as_str(),
            "task.state" | "task.claimed" | "task.released"
        ) && !inputs.receipt_confirmed(event)
    }) {
        events.insert(
            actual.eid.clone(),
            crate::recovery_outbox::original_of(actual),
        );
    }
    events.into_values().collect()
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
fn append_event(
    hub: &mut dyn HubProtocol,
    event: NewEvent,
    now: u64,
    outbox: &mut RecoveryOutbox,
    writes: &mut Vec<TaskWrite>,
) -> Result<Event, String> {
    let event = if let Some(original) = pending_task_events(&outbox.read_inputs()?)
        .into_iter()
        .find(|original| original.kind == event.kind && original.obj == event.obj)
    {
        let mut expected = event.clone();
        expected.eid = original.eid.clone();
        if expected != original {
            return Err(format!(
                "original task request {} requires coordination before another body",
                original.eid
            ));
        }
        original
    } else {
        event
    };
    match append_task(hub, outbox, event, now, writes, None).map_err(|e| e.to_string())? {
        AppendReceipt::Event(e) => Ok(*e),
        AppendReceipt::Duplicate { seq } => Err(format!("event {seq} already checkpointed")),
    }
}
#[allow(
    clippy::too_many_arguments,
    reason = "explicit outbox and exact producer capture preserve task durability"
)]
fn append_change(
    hub: &mut dyn HubProtocol,
    host: &str,
    kind: &str,
    task: &Task,
    run: Option<&Run>,
    guards: Vec<Fence>,
    now: u64,
    outbox: &mut RecoveryOutbox,
    writes: &mut Vec<TaskWrite>,
) -> Result<Event, String> {
    let event = append_event(
        hub,
        change_event(host, kind, task, run, guards),
        now,
        outbox,
        writes,
    )?;
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

pub(crate) fn validate_task(task: &TaskSpec, catalog: &Catalog) -> Result<(), String> {
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

#[allow(
    clippy::too_many_arguments,
    reason = "explicit outbox and exact producer capture preserve task durability"
)]
pub fn execute(
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    host: &str,
    command: TaskCommand,
    run: Option<&str>,
    now: u64,
    outbox: &mut RecoveryOutbox,
) -> Result<Value, TaskFailure> {
    crate::project_session::ProjectSession::execute_shared(
        session, catalog, inputs, host, command, run, now, outbox,
    )
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
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    host: &str,
    now: u64,
    outbox: &mut RecoveryOutbox,
) -> Result<(), String> {
    crate::project_session::ProjectSession::tick_shared(session, catalog, inputs, host, now, outbox)
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
    #[test]
    fn actual_task_claim_populates_confirmed_outbox_before_returning_success() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(dir.path());
        let mut outbox = crate::recovery_outbox::RecoveryOutbox::open(
            &catalog.common_dir.join("test-outbox"),
            crate::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        let session = std::sync::Mutex::new(
            crate::project_session::ProjectSession::new(
                Box::new(crate::hub::Hub::open(&dir.path().join("hub")).unwrap()),
                &catalog,
                &outbox.read_inputs().unwrap(),
                0,
            )
            .unwrap(),
        );
        for (command, now) in [
            (
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
                1,
            ),
            (
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                2,
            ),
        ] {
            crate::project_session::ProjectSession::execute_shared(
                &session,
                &catalog,
                &outbox.read_inputs().unwrap(),
                "host",
                command,
                None,
                now,
                &mut outbox,
            )
            .unwrap();
        }
        let inputs = outbox.read_inputs().unwrap();
        let live = session.lock().unwrap();
        let claim = live
            .events(0)
            .iter()
            .find(|event| event.kind == "task.claimed")
            .cloned()
            .unwrap();
        assert!(
            inputs.receipt_confirmed(&claim),
            "successful real claim lacks its exact fsynced outbox receipt"
        );
        assert!(inputs.pending().is_empty());
        assert_eq!(
            claim.guards,
            serde_json::from_value::<Run>(claim.data["run"].clone())
                .unwrap()
                .leases
        );
    }
    #[test]
    fn wrong_project_outbox_cannot_acquire_a_task_lease_or_append_business_events() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(root.path());
        crate::test_support::catalog(other.path());
        crate::test_support::git(other.path(), &["switch", "main"]);
        std::fs::write(
            other.path().join(crate::config::CONFIG_PATH),
            crate::config::Config::new("foreign-project", "docs/tower")
                .unwrap()
                .to_yaml(),
        )
        .unwrap();
        crate::test_support::git(other.path(), &["add", crate::config::CONFIG_PATH]);
        crate::test_support::git(other.path(), &["commit", "-qm", "different project"]);
        let foreign = Catalog::load(other.path()).unwrap();
        let mut wrong = crate::test_support::outbox(&foreign);
        let session = std::sync::Mutex::new(crate::test_support::open(
            &root.path().join("hub"),
            &catalog,
            0,
        ));
        let before = session.lock().unwrap().last_seq();
        let result = crate::project_session::ProjectSession::execute_shared(
            &session,
            &catalog,
            &crate::test_support::inputs(&catalog),
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            1,
            &mut wrong,
        );
        assert!(result.unwrap_err().to_string().contains("context"));
        let live = session.lock().unwrap();
        assert_eq!(live.last_seq(), before);
        assert!(live.leases(1).is_empty());
        assert!(wrong.read_inputs().unwrap().pending().is_empty());
        assert!(wrong.read_inputs().unwrap().confirmed().is_empty());
    }
    #[test]
    fn a_different_same_project_outbox_is_rejected_before_task_actions() {
        let root = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(root.path());
        let session = std::sync::Mutex::new(crate::test_support::open(
            &root.path().join("hub"),
            &catalog,
            0,
        ));
        let mut other = RecoveryOutbox::open(
            &catalog.common_dir.join("other-producer-outbox"),
            crate::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        let result = crate::project_session::ProjectSession::execute_shared(
            &session,
            &catalog,
            &crate::test_support::inputs(&catalog),
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            1,
            &mut other,
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("outbox source identity"));
        assert_eq!(session.lock().unwrap().last_seq(), 0);
        assert!(session.lock().unwrap().leases(1).is_empty());
        assert!(other.read_inputs().unwrap().pending().is_empty());
    }
    fn repair_external_inputs(root: &std::path::Path) {
        for entry in std::fs::read_dir(
            crate::test_support::catalog(root)
                .common_dir
                .join("test-outbox"),
        )
        .unwrap()
        {
            let entry = entry.unwrap();
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                let text = std::fs::read_to_string(entry.path()).unwrap();
                let mut retained = String::new();
                for line in text.lines() {
                    let value: Value = serde_json::from_str(line).unwrap();
                    if value["original"]["obj"] != "changed-input" {
                        retained.push_str(line);
                        retained.push('\n');
                    }
                }
                std::fs::write(entry.path(), retained).unwrap();
            }
        }
    }
    struct DurableTaskFixture {
        _root: tempfile::TempDir,
        catalog: Catalog,
        session: std::sync::Mutex<crate::project_session::ProjectSession>,
        outbox: RecoveryOutbox,
    }
    impl DurableTaskFixture {
        fn new() -> Self {
            Self::with_protocol(|path| Box::new(crate::hub::Hub::open(path).unwrap()))
        }
        fn with_protocol(open: impl FnOnce(&std::path::Path) -> Box<dyn HubProtocol>) -> Self {
            let root = tempfile::tempdir().unwrap();
            let catalog = crate::test_support::catalog(root.path());
            let mut outbox = crate::test_support::outbox(&catalog);
            let session = std::sync::Mutex::new(
                crate::project_session::ProjectSession::new(
                    open(&root.path().join("hub")),
                    &catalog,
                    &outbox.read_inputs().unwrap(),
                    0,
                )
                .unwrap(),
            );
            crate::project_session::ProjectSession::execute_shared(
                &session,
                &catalog,
                &outbox.read_inputs().unwrap(),
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
                &mut outbox,
            )
            .unwrap();
            Self {
                _root: root,
                catalog,
                session,
                outbox,
            }
        }
        fn call(&mut self, command: TaskCommand, now: u64) -> Result<Value, TaskFailure> {
            crate::project_session::ProjectSession::execute_shared(
                &self.session,
                &self.catalog,
                &self.outbox.read_inputs().unwrap(),
                "host",
                command,
                None,
                now,
                &mut self.outbox,
            )
        }
        fn claim(&mut self) -> Value {
            self.call(
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                2,
            )
            .unwrap()
        }
        fn reconcile(&mut self, now: u64) {
            crate::project_session::ProjectSession::reconcile_pending_shared(
                &self.session,
                &self.catalog,
                &self.outbox.read_inputs().unwrap(),
                now,
                &mut self.outbox,
            )
            .unwrap();
        }
    }
    // A real persisted archive is the receipt authority. Any attempt to replay
    // its original into the reopened Hub, or grant a new lease, fails this test.
    struct ArchivedTaskNoAppend(crate::hub::Hub);
    impl HubProtocol for ArchivedTaskNoAppend {
        fn append(&mut self, _: NewEvent, _: u64) -> Result<AppendReceipt, HubError> {
            panic!("verified archived Task receipt must not append to Hub");
        }
        fn subscribe(
            &mut self,
            after: u64,
            now: u64,
        ) -> Result<crate::hub_protocol::SubscriptionBatch, HubError> {
            self.0.subscribe(after, now)
        }
        fn lease_acquire(
            &mut self,
            _: AcquireLease,
            _: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            panic!("archived Task coordination must not grant a lease");
        }
        fn lease_renew(
            &mut self,
            request: RenewLease,
            now: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            self.0.lease_renew(request, now)
        }
        fn lease_release(
            &mut self,
            request: ReleaseLease,
            now: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            self.0.lease_release(request, now)
        }
        fn snapshot_ack(
            &mut self,
            ack: crate::hub_protocol::SnapshotAck,
            checkpoint: Value,
        ) -> Result<(), HubError> {
            HubProtocol::snapshot_ack(&mut self.0, ack, checkpoint)
        }
        fn rebuild(
            &mut self,
            seed: crate::hub_protocol::RebuildSeed,
        ) -> Result<crate::hub_protocol::RebuildReceipt, HubError> {
            self.0.rebuild(seed)
        }
        fn contiguous_seq(&self) -> u64 {
            self.0.contiguous_seq()
        }
    }
    #[test]
    fn archived_task_receipt_retries_confirmation_without_reappend_or_new_grant() {
        use crate::project_session::ProjectSession;
        let mut f = DurableTaskFixture::new();
        let actual = f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|event| event.kind == "task.state" && event.obj == "T")
            .unwrap()
            .clone();
        let original = crate::recovery_outbox::original_of(&actual);
        let report = crate::snapshot_service::SnapshotWriter::default()
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(2),
                &mut f.outbox,
            )
            .unwrap();
        assert_eq!(report.status, "written");
        let storage = f.catalog.common_dir.join("test-outbox");
        let DurableTaskFixture {
            _root,
            session,
            mut outbox,
            ..
        } = f;
        drop(session);
        // Simulate loss of the producer confirmation after an actual accepted
        // event has already entered the exact persisted snapshot archive.
        for entry in std::fs::read_dir(&storage).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let retained = text
                .lines()
                .filter(|line| {
                    let row: Value = serde_json::from_str(line).unwrap();
                    !(row["state"] == "confirmed" && row["original"]["eid"] == original.eid)
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(path, format!("{retained}\n")).unwrap();
        }
        let catalog = Catalog::load(_root.path()).unwrap();
        let saved = crate::snapshot_service::read_snapshot(&catalog)
            .unwrap()
            .unwrap();
        assert!(saved.archives.values().any(|text| text
            .lines()
            .any(|line| { serde_json::from_str::<Event>(line).unwrap() == actual })));
        let inputs = outbox.read_inputs().unwrap();
        assert!(inputs.pending().contains(&original));
        assert!(!inputs
            .confirmed()
            .iter()
            .any(|event| event.eid == original.eid));
        let session = std::sync::Mutex::new(
            ProjectSession::new(
                Box::new(ArchivedTaskNoAppend(
                    crate::hub::Hub::open(&_root.path().join("hub")).unwrap(),
                )),
                &catalog,
                &inputs,
                3,
            )
            .unwrap(),
        );
        let proof = session.lock().unwrap().tasks.archived_receipts[&original.eid].clone();
        let before_digest = outbox.read_inputs().unwrap().digest().to_string();
        let mut changed = original.clone();
        changed.data["task"]["title"] = json!("different original body");
        let error = crate::recovery_outbox::append_task_durable(
            &mut *session.lock().unwrap().hub,
            &mut outbox,
            changed,
            3,
            &proof,
        )
        .unwrap_err();
        assert!(matches!(error, HubError::Invalid(_)));
        assert_eq!(outbox.read_inputs().unwrap().digest(), before_digest);
        let cursor = session.lock().unwrap().applied_seq();
        let leases = session.lock().unwrap().head.leases.clone();
        let private = crate::snapshot_git::private_revision(&catalog.root).unwrap();
        outbox.fail_next_confirmation();
        let error = ProjectSession::reconcile_pending_shared(
            &session,
            &catalog,
            &outbox.read_inputs().unwrap(),
            4,
            &mut outbox,
        )
        .unwrap_err();
        assert!(error.contains(&original.eid), "{error}");
        assert!(error.contains("injected outbox"), "{error}");
        let uncertain = outbox.read_inputs().unwrap();
        assert!(uncertain.confirmed().contains(&actual));
        assert!(!uncertain.receipt_confirmed(&actual));
        assert!(
            ProjectSession::task_reconciliation_needed_shared(&session, &catalog, &uncertain)
                .unwrap()
        );
        ProjectSession::reconcile_pending_shared(&session, &catalog, &uncertain, 5, &mut outbox)
            .unwrap();
        let confirmed = outbox.read_inputs().unwrap();
        assert!(confirmed.receipt_confirmed(&actual));
        assert!(
            !ProjectSession::task_reconciliation_needed_shared(&session, &catalog, &confirmed)
                .unwrap()
        );
        let live = session.lock().unwrap();
        assert_eq!(live.applied_seq(), cursor);
        assert_eq!(live.head.leases, leases);
        assert_eq!(
            crate::catalog::authority_revision(&catalog.root).unwrap(),
            catalog.revision
        );
        assert_eq!(
            crate::snapshot_git::private_revision(&catalog.root).unwrap(),
            private
        );
        assert_eq!(
            confirmed
                .confirmed()
                .iter()
                .filter(|event| event.eid == original.eid)
                .count(),
            1
        );
        assert_eq!(crate::recovery_outbox::original_of(&actual), original);
    }
    #[test]
    fn claim_confirmation_failure_keeps_live_fact_and_repairs_only_original() {
        let mut f = DurableTaskFixture::new();
        f.outbox.fail_next_confirmation();
        let error = f
            .call(
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                2,
            )
            .unwrap_err();
        let pending = f
            .session
            .lock()
            .unwrap()
            .pending_claim()
            .unwrap()
            .unwrap()
            .clone();
        assert!(error.to_string().contains(&pending.event.eid));
        let actual = f
            .session
            .lock()
            .unwrap()
            .events(0)
            .into_iter()
            .find(|event| event.eid == pending.event.eid)
            .unwrap();
        assert!(!f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
        assert_eq!(
            f.session.lock().unwrap().projection().unwrap().objects["Run"][&pending.run_id]
                ["state"],
            "running"
        );
        f.reconcile(3);
        assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
        assert!(f.session.lock().unwrap().pending_claim().unwrap().is_none());
        assert_eq!(
            f.session
                .lock()
                .unwrap()
                .events(0)
                .iter()
                .filter(|event| event.kind == "task.claimed")
                .count(),
            1
        );
        assert_eq!(crate::recovery_outbox::original_of(&actual), pending.event);
    }
    #[test]
    fn duplicate_physical_input_cannot_refresh_a_verified_task_prefix() {
        let mut f = DurableTaskFixture::new();
        let before = f.session.lock().unwrap().projection().unwrap().clone();
        let directory = f.catalog.common_dir.join("test-outbox");
        crate::recovery_outbox::CONFIRMATION_OBSERVER.with(|observer| {
            *observer.borrow_mut() = Some(Box::new(move || {
                use std::io::Write;
                let path = std::fs::read_dir(&directory)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.extension()
                            .is_some_and(|extension| extension == "jsonl")
                    })
                    .unwrap();
                let bytes = std::fs::read(&path).unwrap();
                let first = bytes.split_inclusive(|byte| *byte == b'\n').next().unwrap();
                let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
                file.write_all(first).unwrap();
                file.sync_all().unwrap();
            }));
        });
        let error = f
            .call(
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                2,
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside exact task producer records"),
            "{error}"
        );
        assert_eq!(*f.session.lock().unwrap().projection().unwrap(), before);
        assert_eq!(
            f.outbox
                .read_inputs()
                .unwrap()
                .confirmed()
                .iter()
                .filter(|event| event.kind == "task.claimed")
                .count(),
            1
        );
        assert!(f.session.lock().unwrap().pending_claim().unwrap().is_some());
    }
    #[test]
    fn release_and_expiry_confirmation_failure_reuse_the_exact_accepted_event() {
        for expiry in [false, true] {
            let mut f = DurableTaskFixture::new();
            let run = f.claim();
            f.outbox.fail_next_confirmation();
            let now = if expiry { LEASE_TTL_MS + 3 } else { 3 };
            let error = if expiry {
                crate::project_session::ProjectSession::tick_shared(
                    &f.session,
                    &f.catalog,
                    &f.outbox.read_inputs().unwrap(),
                    "host",
                    now,
                    &mut f.outbox,
                )
                .unwrap_err()
            } else {
                f.call(TaskCommand::Release, now).unwrap_err().to_string()
            };
            assert!(!error.is_empty());
            let actual = f
                .session
                .lock()
                .unwrap()
                .events(0)
                .into_iter()
                .find(|event| event.kind == "task.released")
                .unwrap();
            assert_eq!(actual.data["run"]["id"], run["id"]);
            assert!(!f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
            assert_eq!(
                f.session.lock().unwrap().projection().unwrap().objects["Task"]["T"]["state"],
                "ready"
            );
            f.reconcile(now + 1);
            f.reconcile(now + 2);
            assert!(f.outbox.read_inputs().unwrap().receipt_confirmed(&actual));
            assert_eq!(
                f.session
                    .lock()
                    .unwrap()
                    .events(0)
                    .iter()
                    .filter(|event| event.kind == "task.released")
                    .count(),
                1
            );
            assert_eq!(
                f.outbox
                    .read_inputs()
                    .unwrap()
                    .confirmed()
                    .iter()
                    .filter(|event| event.eid == actual.eid)
                    .collect::<Vec<_>>(),
                vec![&actual]
            );
        }
    }
    fn catalog(root: &std::path::Path) -> Catalog {
        crate::test_support::catalog(root)
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
            service
                .expire(
                    &mut hub,
                    &catalog,
                    "host",
                    2,
                    &mut crate::test_support::outbox(&catalog),
                )
                .unwrap();
        }
        assert_eq!(service.active_visits.get(), 200);
    }
    enum ClaimSourceChange {
        Main(std::path::PathBuf, String),
        Private(std::path::PathBuf, std::sync::Arc<std::sync::Mutex<String>>),
        Inputs(std::path::PathBuf),
    }
    impl ClaimSourceChange {
        fn apply(&self) {
            match self {
                ClaimSourceChange::Main(root, previous) => {
                    crate::test_support::git(root, &["update-ref", "refs/heads/main", previous]);
                }
                ClaimSourceChange::Private(root, previous) => {
                    crate::test_support::git(
                        root,
                        &[
                            "update-ref",
                            "refs/towermd/snapshot",
                            &previous.lock().unwrap(),
                        ],
                    );
                }
                ClaimSourceChange::Inputs(root) => {
                    // Deliberately bypass the producer lock as an external filesystem fault.
                    // Serialize the foreign record through the real outbox in a separate owned directory.
                    let catalog = Catalog::load(root).unwrap();
                    let temporary = tempfile::tempdir().unwrap();
                    let mut outbox = crate::recovery_outbox::RecoveryOutbox::open(
                        temporary.path(),
                        crate::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
                    )
                    .unwrap();
                    outbox
                        .append_pending(&NewEvent {
                            eid: uuid::Uuid::now_v7().to_string(),
                            host: "host".into(),
                            actor: "process:audit".into(),
                            kind: "alert.raised".into(),
                            layer: "L4".into(),
                            obj: "changed-input".into(),
                            guards: vec![],
                            data: json!({"reason":"post-accept input change"}),
                        })
                        .unwrap();
                    for entry in std::fs::read_dir(temporary.path()).unwrap() {
                        let entry = entry.unwrap();
                        if entry
                            .path()
                            .extension()
                            .is_some_and(|extension| extension == "jsonl")
                        {
                            use std::io::Write;
                            let bytes = std::fs::read(entry.path()).unwrap();
                            let mut target = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(
                                    crate::test_support::catalog(root)
                                        .common_dir
                                        .join("test-outbox")
                                        .join(entry.file_name()),
                                )
                                .unwrap();
                            target.write_all(&bytes).unwrap();
                            target.sync_all().unwrap();
                        }
                    }
                }
            }
        }
    }
    enum RenewChange {
        Source(ClaimSourceChange),
        Tail(NewEvent),
        ReceiptMismatch,
        TailDuringCapture(NewEvent, std::sync::atomic::AtomicUsize),
        ForgedSystem(std::sync::Mutex<Option<crate::hub::Event>>),
    }
    struct RenewEffect {
        armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
        change: RenewChange,
    }
    struct FailClaim {
        inner: crate::hub::Hub,
        accepted_kind: &'static str,
        after_renewal: Option<RenewEffect>,
        partial: bool,
        failed: bool,
        after_accept: Option<(ClaimSourceChange, bool)>,
    }
    impl HubProtocol for FailClaim {
        fn append(&mut self, event: NewEvent, now: u64) -> Result<AppendReceipt, HubError> {
            if event.kind == self.accepted_kind && !self.failed {
                self.failed = true;
                if let Some((change, canonical)) = &self.after_accept {
                    let receipt = self.inner.append(event, now)?;
                    change.apply();
                    if *canonical {
                        self.inner.append(
                            NewEvent {
                                eid: uuid::Uuid::now_v7().to_string(),
                                host: "host".into(),
                                actor: "process:self-reported".into(),
                                kind: "merge.done".into(),
                                layer: "L4".into(),
                                obj: "fake".into(),
                                data: json!({"commit":"unproved"}),
                                guards: vec![],
                            },
                            now,
                        )?;
                    }
                    return Ok(receipt);
                }
                self.inner.fail_next_append(self.partial);
            }
            self.inner.append(event, now)
        }
        fn subscribe(
            &mut self,
            a: u64,
            n: u64,
        ) -> Result<crate::hub_protocol::SubscriptionBatch, HubError> {
            if let Some(RenewEffect {
                change: RenewChange::TailDuringCapture(event, remaining),
                ..
            }) = &self.after_renewal
            {
                if remaining.load(std::sync::atomic::Ordering::SeqCst) > 0
                    && remaining.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1
                {
                    self.inner.append(event.clone(), n)?;
                }
            }
            let mut batch = self.inner.subscribe(a, n)?;
            if let Some(RenewEffect {
                change: RenewChange::ForgedSystem(forged),
                ..
            }) = &self.after_renewal
            {
                if let Some(forged) = &*forged.lock().unwrap() {
                    for event in &mut batch.events {
                        if event.seq == forged.seq {
                            *event = forged.clone();
                        }
                    }
                }
            }
            Ok(batch)
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
            let mut receipt = self.inner.lease_renew(r, n)?;
            if let Some(effect) = &self.after_renewal {
                if effect
                    .armed
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    match &effect.change {
                        RenewChange::Source(change) => change.apply(),
                        RenewChange::Tail(event) => {
                            self.inner.append(event.clone(), n)?;
                        }
                        RenewChange::ReceiptMismatch => {
                            receipt.event.data["forged_return"] = json!(true);
                        }
                        RenewChange::TailDuringCapture(_, remaining) => {
                            remaining.store(2, std::sync::atomic::Ordering::SeqCst);
                        }
                        RenewChange::ForgedSystem(forged) => {
                            receipt.event.actor = "process:forged-hub".into();
                            *forged.lock().unwrap() = Some(receipt.event.clone());
                        }
                    }
                }
            }
            Ok(receipt)
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
    fn terminal_stale_claim_reconciliation_clears_only_the_original_after_sync() {
        use crate::project_session::ProjectSession;
        let mut f = DurableTaskFixture::with_protocol(|path| {
            Box::new(FailClaim {
                inner: crate::hub::Hub::open(path).unwrap(),
                accepted_kind: "task.claimed",
                after_renewal: None,
                partial: true,
                failed: false,
                after_accept: None,
            })
        });
        let error = f
            .call(
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                2,
            )
            .unwrap_err();
        let pending = f
            .session
            .lock()
            .unwrap()
            .pending_claim()
            .unwrap()
            .unwrap()
            .clone();
        assert!(error.to_string().contains(&pending.event.eid));
        assert_eq!(
            f.outbox.read_inputs().unwrap().pending(),
            std::slice::from_ref(&pending.event)
        );
        assert!(!f
            .session
            .lock()
            .unwrap()
            .events(0)
            .iter()
            .any(|e| e.eid == pending.event.eid));

        // 实际 journal 部分写失败必须先重开修复；此时原租约尚未到期。
        // 以下恢复后的同一 Session 跨越 TTL，终结协调后不再重启来清 pending。
        let DurableTaskFixture {
            _root,
            catalog,
            session,
            outbox,
        } = f;
        drop(session);
        let session = std::sync::Mutex::new(
            ProjectSession::open(
                &_root.path().join("hub"),
                &catalog,
                &outbox.read_inputs().unwrap(),
                3,
            )
            .unwrap(),
        );
        let mut f = DurableTaskFixture {
            _root,
            catalog,
            session,
            outbox,
        };
        assert_eq!(
            f.session.lock().unwrap().pending_claim().unwrap(),
            Some(&pending)
        );
        let session_id = f.session.lock().unwrap().snapshot_session_id();
        let now = LEASE_TTL_MS + 3;
        let inputs = f.outbox.read_inputs().unwrap();
        let error = ProjectSession::reconcile_pending_shared(
            &f.session,
            &f.catalog,
            &inputs,
            now,
            &mut f.outbox,
        )
        .unwrap_err();
        assert!(error.contains("expired"), "{error}");
        let inputs = f.outbox.read_inputs().unwrap();
        let actual = inputs
            .confirmed()
            .iter()
            .find(|e| e.eid == pending.event.eid)
            .unwrap();
        assert!(actual.stale, "原 guards 到期后不得重新领取或改写原件");
        assert_eq!(crate::recovery_outbox::original_of(actual), pending.event);
        assert_eq!(actual.guards, pending.event.guards);
        assert!(inputs.receipt_confirmed(actual));
        assert!(inputs.pending().is_empty());
        {
            let live = f.session.lock().unwrap();
            assert_eq!(live.snapshot_session_id(), session_id);
            assert!(!live
                .projection()
                .unwrap()
                .objects
                .get("Run")
                .is_some_and(|runs| runs.contains_key(&pending.run_id)));
            assert_eq!(
                live.projection().unwrap().objects["Task"]["T"]["state"],
                "ready"
            );
            assert!(
                live.pending_claim().unwrap().is_none(),
                "原 stale claim 已耐久确认、补偿并完成最终同步，不能继续挡住新请求"
            );
        }
        assert!(!ProjectSession::task_reconciliation_needed_shared(
            &f.session, &f.catalog, &inputs
        )
        .unwrap());
        let next = f
            .call(
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                now + 1,
            )
            .unwrap();
        assert_ne!(next["id"].as_str(), Some(pending.run_id.as_str()));
        let live = f.session.lock().unwrap();
        assert_eq!(live.snapshot_session_id(), session_id);
        assert_eq!(
            live.events(0)
                .iter()
                .filter(|e| e.eid == pending.event.eid)
                .count(),
            1
        );
    }
    #[test]
    fn actual_claim_io_uncertainty_retains_the_product_eid_and_reopens_with_same_pending_event() {
        for partial in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let catalog = crate::test_support::catalog(dir.path());
            let path = dir.path().join("hub");
            let protocol = FailClaim {
                inner: crate::hub::Hub::open(&path).unwrap(),
                accepted_kind: "task.claimed",
                after_renewal: None,
                partial,
                failed: false,
                after_accept: None,
            };
            let session = std::sync::Mutex::new(
                crate::project_session::ProjectSession::new(
                    Box::new(protocol),
                    &catalog,
                    &crate::test_support::inputs(&catalog),
                    0,
                )
                .unwrap(),
            );
            crate::project_session::ProjectSession::execute_shared(
                &session,
                &catalog,
                &crate::test_support::inputs(&catalog),
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
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            let error = crate::project_session::ProjectSession::execute_shared(
                &session,
                &catalog,
                &crate::test_support::inputs(&catalog),
                "host",
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                None,
                2,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap_err();
            let pending = session.lock().unwrap().tasks.pending_claim.clone().unwrap();
            assert!(error.to_string().contains(&pending.event.eid));
            assert!(error.to_string().contains(&pending.run_id));
            assert!(error.to_string().contains("结果不确定"));
            assert_eq!(session.lock().unwrap().hub.contiguous_seq(), 2);
            drop(session);
            let reopened = crate::test_support::open(&path, &catalog, 0);
            if partial {
                assert!(!reopened.projection.objects.contains_key("Run"));
                assert_eq!(reopened.leases(3).len(), 1);
            } else {
                assert_eq!(
                    reopened.projection.objects["Run"][&pending.run_id]["state"],
                    "running"
                );
            }
            assert_eq!(reopened.tasks.pending_claim, Some(pending.clone()));
            let reopened = std::sync::Mutex::new(reopened);
            crate::project_session::ProjectSession::reconcile_pending_shared(
                &reopened,
                &catalog,
                &crate::test_support::inputs(&catalog),
                3,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            let reopened = reopened.lock().unwrap();
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
    #[test]
    fn show_checks_exact_renewal_other_tails_pending_and_changed_sources_before_delivery() {
        use crate::project_session::ProjectSession;
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        for scenario in [
            "exact_pending",
            "audit_tail",
            "receipt_mismatch",
            "tail_during_capture",
            "forged_system",
            "raw_merged_tail",
            "canonical_tail",
            "main",
            "inputs",
            "private",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let old = crate::test_support::catalog(dir.path()).revision;
            crate::test_support::git(dir.path(), &["switch", "main"]);
            crate::test_support::git(dir.path(), &["commit", "--allow-empty", "-qm", "Show S1"]);
            crate::test_support::git(dir.path(), &["switch", "t/T"]);
            let catalog = Catalog::load(dir.path()).unwrap();
            let mut inputs = crate::test_support::inputs(&catalog);
            let path = dir.path().join("hub");
            let change = match scenario {
                "main" => RenewChange::Source(ClaimSourceChange::Main(dir.path().into(), old)),
                "inputs" => RenewChange::Source(ClaimSourceChange::Inputs(dir.path().into())),
                "private" => {
                    let main = dir.path().join("main-worktree");
                    crate::test_support::git(
                        dir.path(),
                        &["worktree", "add", "-q", main.to_str().unwrap(), "main"],
                    );
                    let seed = std::sync::Mutex::new(
                        ProjectSession::open(&path, &catalog, &inputs, 0).unwrap(),
                    );
                    let writer = crate::snapshot_service::SnapshotWriter::default();
                    assert_eq!(
                        writer
                            .write(
                                &seed,
                                &catalog,
                                "host",
                                true,
                                || Ok(10),
                                &mut crate::test_support::outbox(&catalog)
                            )
                            .unwrap()
                            .status,
                        "written"
                    );
                    let s0 = crate::snapshot_git::private_revision(dir.path())
                        .unwrap()
                        .unwrap();
                    seed.lock()
                        .unwrap()
                        .hub
                        .append(
                            NewEvent {
                                eid: uuid::Uuid::now_v7().to_string(),
                                host: "host".into(),
                                actor: "process:audit".into(),
                                kind: "future.audit".into(),
                                layer: "L4".into(),
                                obj: "S1".into(),
                                guards: vec![],
                                data: json!({"source":"S1"}),
                            },
                            11,
                        )
                        .unwrap();
                    assert_eq!(
                        writer
                            .write(
                                &seed,
                                &catalog,
                                "host",
                                true,
                                || Ok(12),
                                &mut crate::test_support::outbox(&catalog)
                            )
                            .unwrap()
                            .status,
                        "written"
                    );
                    let s1 = crate::snapshot_git::private_revision(dir.path())
                        .unwrap()
                        .unwrap();
                    assert_ne!(s0, s1);
                    assert_eq!(seed.lock().unwrap().snapshot_ack().unwrap().commit, s1);
                    drop(seed);
                    RenewChange::Source(ClaimSourceChange::Private(
                        dir.path().into(),
                        Arc::new(std::sync::Mutex::new(s0)),
                    ))
                }
                "receipt_mismatch" => RenewChange::ReceiptMismatch,
                "forged_system" => RenewChange::ForgedSystem(std::sync::Mutex::new(None)),
                "tail_during_capture" => RenewChange::TailDuringCapture(
                    NewEvent {
                        eid: uuid::Uuid::now_v7().to_string(),
                        host: "host".into(),
                        actor: "process:audit".into(),
                        kind: "future.audit".into(),
                        layer: "L4".into(),
                        obj: "during-final-capture".into(),
                        guards: vec![],
                        data: json!({"retained":[3,2,1]}),
                    },
                    std::sync::atomic::AtomicUsize::new(0),
                ),
                "exact_pending" => RenewChange::Tail(NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "host".into(),
                    actor: "process:unused".into(),
                    kind: "unused".into(),
                    layer: "L4".into(),
                    obj: "unused".into(),
                    guards: vec![],
                    data: json!({}),
                }),
                _ => RenewChange::Tail(NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "host".into(),
                    actor: "process:test".into(),
                    kind: match scenario {
                        "audit_tail" => "future.audit",
                        "raw_merged_tail" => "task.state",
                        _ => "merge.done",
                    }
                    .into(),
                    layer: "L4".into(),
                    obj: "T".into(),
                    guards: vec![],
                    data: match scenario {
                        "audit_tail" => json!({"retained":[3,2,1]}),
                        "raw_merged_tail" => {
                            json!({"task":{"type":"Task","id":"T","state":"merged"}})
                        }
                        _ => json!({"commit":"a".repeat(40),"proof":{"schema":1}}),
                    },
                }),
            };
            let armed = Arc::new(AtomicBool::new(false));
            let session = std::sync::Mutex::new(
                ProjectSession::new(
                    Box::new(FailClaim {
                        inner: crate::hub::Hub::open(&path).unwrap(),
                        accepted_kind: "task.claimed",
                        partial: false,
                        failed: false,
                        after_accept: None,
                        after_renewal: None,
                    }),
                    &catalog,
                    &crate::test_support::inputs(&catalog),
                    0,
                )
                .unwrap(),
            );
            ProjectSession::execute_shared(
                &session,
                &catalog,
                &inputs,
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
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            inputs = crate::test_support::inputs(&catalog);
            let uncertain = ProjectSession::execute_shared(
                &session,
                &catalog,
                &inputs,
                "host",
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                None,
                2,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap_err();
            let pending = session
                .lock()
                .unwrap()
                .pending_claim()
                .unwrap()
                .unwrap()
                .clone();
            assert!(uncertain.to_string().contains(&pending.event.eid));
            // An uncertain durable append poisons this owner until it is closed.
            // Reopen the real journal, retaining only the original request identity.
            drop(session);
            let mut reopened = ProjectSession::new(
                Box::new(FailClaim {
                    inner: crate::hub::Hub::open(&path).unwrap(),
                    accepted_kind: "task.claimed",
                    partial: false,
                    failed: true,
                    after_accept: None,
                    after_renewal: Some(RenewEffect {
                        armed: armed.clone(),
                        change,
                    }),
                }),
                &catalog,
                &crate::test_support::inputs(&catalog),
                3,
            )
            .unwrap();
            reopened.tasks.pending_claim = Some(pending.clone());
            let session = std::sync::Mutex::new(reopened);
            inputs = crate::test_support::inputs(&catalog);
            let (before, projection, reads) = {
                let live = session.lock().unwrap();
                (
                    live.applied_seq(),
                    live.projection().unwrap().clone(),
                    live.authority_reads(),
                )
            };
            if scenario != "exact_pending" {
                armed.store(true, Ordering::SeqCst);
            }
            let result = ProjectSession::execute_shared(
                &session,
                &catalog,
                &inputs,
                "host",
                TaskCommand::Show { id: None },
                None,
                3,
                &mut crate::test_support::outbox(&catalog),
            );
            assert!(!armed.load(Ordering::SeqCst));
            let mut live = session.lock().unwrap();
            assert_eq!(
                live.pending_claim().unwrap().unwrap().event,
                pending.event,
                "{scenario}"
            );
            assert_eq!(
                live.pending_claim().unwrap().unwrap().run_id,
                pending.run_id
            );
            let actual = live.hub.subscribe(before, 3).unwrap();
            assert_eq!(
                actual
                    .events
                    .iter()
                    .filter(|event| event.kind == "lease.renewed")
                    .count(),
                1,
                "{scenario}: command must not renew twice"
            );
            assert_eq!(
                actual
                    .events
                    .iter()
                    .find(|event| event.kind == "lease.renewed")
                    .unwrap()
                    .updated_leases()
                    .iter()
                    .map(|lease| lease.fence())
                    .collect::<Vec<_>>(),
                pending.event.guards
            );
            match scenario {
                "exact_pending" => {
                    let shown = result.unwrap();
                    assert_eq!(shown["id"], pending.run_id);
                    assert_eq!(live.applied_seq(), before);
                    assert_eq!(live.authority_reads(), reads);
                    assert_eq!(*live.projection().unwrap(), projection);
                    assert_eq!(live.head, actual.head);
                }
                "audit_tail" | "receipt_mismatch" | "tail_during_capture" => {
                    result.unwrap();
                    assert_eq!(live.applied_seq(), actual.head.contiguous_seq);
                    if scenario == "receipt_mismatch" {
                        // The narrow path rejected the forged return. Shared sync
                        // validates the actual system delta without re-reading archives.
                        assert_eq!(live.authority_reads(), reads);
                        assert_eq!(live.events(before), actual.events);
                        assert!(live
                            .events(before)
                            .iter()
                            .all(|event| event.data.get("forged_return").is_none()));
                        assert_eq!(live.head, actual.head);
                    } else {
                        assert!(
                            live.authority_reads() > reads,
                            "{scenario}: unknown audit requires full history proof"
                        );
                    }
                    assert_eq!(live.projection().unwrap().objects, projection.objects);
                    if matches!(scenario, "audit_tail" | "tail_during_capture") {
                        assert!(live
                            .events(before)
                            .iter()
                            .any(|event| event.kind == "future.audit"
                                && event.data == json!({"retained":[3,2,1]})));
                    }
                }
                _ => {
                    let error = result.unwrap_err().to_string();
                    let expected = match scenario {
                        "raw_merged_tail" => "raw task event cannot certify merged",
                        "canonical_tail" => "invalid canonical merge proof",
                        "forged_system" => "invalid system lease event envelope",
                        "main" => "authority changed; reload Catalog",
                        "inputs" => "recovery inputs changed during preparation",
                        _ => "private snapshot source changed",
                    };
                    assert!(error.contains(expected), "{scenario}: {error}");
                    assert!(error.contains(&pending.event.eid), "{error}");
                    assert!(error.contains(&pending.run_id), "{error}");
                    assert_eq!(live.applied_seq(), before, "{scenario}");
                    assert_eq!(*live.projection().unwrap(), projection, "{scenario}");
                }
            }
        }
    }
    #[test]
    fn show_renews_exact_system_head_without_advancing_verified_business_prefix() {
        use crate::project_session::ProjectSession;
        let dir = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(dir.path());
        let mut inputs = crate::test_support::inputs(&catalog);
        let session = std::sync::Mutex::new(
            ProjectSession::open(&dir.path().join("hub"), &catalog, &inputs, 0).unwrap(),
        );
        ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
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
            &mut crate::test_support::outbox(&catalog),
        )
        .unwrap();
        inputs = crate::test_support::inputs(&catalog);
        let run = ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            2,
            &mut crate::test_support::outbox(&catalog),
        )
        .unwrap();
        inputs = crate::test_support::inputs(&catalog);
        let (before, projection, oldlease) = {
            let live = session.lock().unwrap();
            (
                live.applied_seq(),
                live.projection().unwrap().clone(),
                live.leases(2)["module:A"].clone(),
            )
        };
        let shown = ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Show { id: None },
            None,
            3,
            &mut crate::test_support::outbox(&catalog),
        )
        .unwrap();
        assert_eq!(shown["id"], run["id"]);
        assert_eq!(shown["leases"], run["leases"]);
        let mut live = session.lock().unwrap();
        let actual = live.hub.subscribe(before, 3).unwrap();
        assert_eq!(actual.events.len(), 1);
        assert_eq!(actual.events[0].kind, "lease.renewed");
        assert_eq!(actual.events[0].actor, "process:towermd-hub");
        assert_eq!(
            actual.events[0]
                .updated_leases()
                .iter()
                .map(|lease| lease.fence())
                .collect::<Vec<_>>(),
            serde_json::from_value::<Vec<Fence>>(run["leases"].clone()).unwrap()
        );
        assert!(actual.events[0].updated_leases()[0].expires_at > oldlease.expires_at);
        assert_eq!(*live.projection().unwrap(), projection);
        assert_eq!(
            live.applied_seq(),
            before,
            "read-only Show must preserve the verified business prefix"
        );
        assert_eq!(live.head, actual.head);
        drop(live);
        ProjectSession::sync_shared(
            &session,
            &catalog,
            &crate::test_support::inputs(&catalog),
            4,
        )
        .unwrap();
        assert_eq!(
            session.lock().unwrap().applied_seq(),
            actual.head.contiguous_seq
        );
        assert_eq!(*session.lock().unwrap().projection().unwrap(), projection);
    }
    #[test]
    fn command_continues_once_after_proving_a_new_expiration_tail() {
        use crate::project_session::ProjectSession;
        let dir = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(dir.path());
        let mut inputs = crate::test_support::inputs(&catalog);
        let session = std::sync::Mutex::new(
            ProjectSession::open(&dir.path().join("hub"), &catalog, &inputs, 0).unwrap(),
        );
        ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
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
            &mut crate::test_support::outbox(&catalog),
        )
        .unwrap();
        inputs = crate::test_support::inputs(&catalog);
        let run = ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            2,
            &mut crate::test_support::outbox(&catalog),
        )
        .unwrap();
        inputs = crate::test_support::inputs(&catalog);
        let shown = ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Show { id: None },
            None,
            3,
            &mut crate::test_support::outbox(&catalog),
        )
        .unwrap();
        inputs = crate::test_support::inputs(&catalog);
        assert_eq!(shown["id"], run["id"]);
        assert_eq!(shown["state"], "running");
        assert_eq!(shown["leases"], run["leases"]);
        let before = session.lock().unwrap().applied_seq();
        let shown = ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Show {
                id: Some("T".into()),
            },
            None,
            LEASE_TTL_MS + 4,
            &mut crate::test_support::outbox(&catalog),
        )
        .unwrap();
        assert_eq!(
            shown["state"], "ready",
            "command must see the proved expiration result"
        );
        let live = session.lock().unwrap();
        assert_eq!(
            live.projection().unwrap().objects["Run"][run["id"].as_str().unwrap()]["state"],
            "released"
        );
        assert_eq!(
            live.tasks.runs[run["id"].as_str().unwrap()].state,
            "released"
        );
        let releases: Vec<_> = live
            .events(before)
            .into_iter()
            .filter(|event| event.kind == "task.released")
            .collect();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].actor, "process:towermd-daemon");
        assert_eq!(
            live.events(0)
                .iter()
                .filter(|event| event.kind == "task.claimed")
                .count(),
            1
        );
    }
    #[test]
    fn tick_keeps_last_verified_view_when_source_changes_after_release_acceptance() {
        use crate::project_session::ProjectSession;
        for change_inputs in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let old = crate::test_support::catalog(dir.path()).revision;
            crate::test_support::git(dir.path(), &["switch", "main"]);
            crate::test_support::git(dir.path(), &["commit", "--allow-empty", "-qm", "tick S1"]);
            crate::test_support::git(dir.path(), &["switch", "t/T"]);
            let catalog = Catalog::load(dir.path()).unwrap();
            let mut inputs = crate::test_support::inputs(&catalog);
            let change = if change_inputs {
                ClaimSourceChange::Inputs(dir.path().into())
            } else {
                ClaimSourceChange::Main(dir.path().into(), old)
            };
            let session = std::sync::Mutex::new(
                ProjectSession::new(
                    Box::new(FailClaim {
                        inner: crate::hub::Hub::open(&dir.path().join("hub")).unwrap(),
                        accepted_kind: "task.released",
                        after_renewal: None,
                        partial: false,
                        failed: false,
                        after_accept: Some((change, false)),
                    }),
                    &catalog,
                    &crate::test_support::inputs(&catalog),
                    0,
                )
                .unwrap(),
            );
            ProjectSession::execute_shared(
                &session,
                &catalog,
                &inputs,
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
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            inputs = crate::test_support::inputs(&catalog);
            let run = ProjectSession::execute_shared(
                &session,
                &catalog,
                &inputs,
                "host",
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                None,
                2,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            inputs = crate::test_support::inputs(&catalog);
            let expired = LEASE_TTL_MS + 3;
            // Resolve only actual system expiration before testing the business release.
            ProjectSession::sync_shared(
                &session,
                &catalog,
                &crate::test_support::inputs(&catalog),
                expired,
            )
            .unwrap();
            let before = session.lock().unwrap().projection().unwrap().clone();
            let applied = session.lock().unwrap().applied_seq();
            let error = ProjectSession::tick_shared(
                &session,
                &catalog,
                &inputs,
                "host",
                expired,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap_err();
            assert!(
                error.contains("accepted request requires synchronization"),
                "{error}"
            );
            assert_eq!(*session.lock().unwrap().projection().unwrap(), before);
            assert_eq!(session.lock().unwrap().applied_seq(), applied);
            let releases: Vec<_> = session
                .lock()
                .unwrap()
                .hub
                .subscribe(applied, expired)
                .unwrap()
                .events
                .into_iter()
                .filter(|event| event.kind == "task.released")
                .collect();
            assert_eq!(releases.len(), 1);
            assert_eq!(releases[0].data["run"]["id"], run["id"]);
            assert_eq!(releases[0].actor, "process:towermd-daemon");
            assert!(!releases[0].stale);
            if change_inputs {
                repair_external_inputs(dir.path());
            } else {
                crate::test_support::git(
                    dir.path(),
                    &["update-ref", "refs/heads/main", &catalog.revision],
                );
            }
            ProjectSession::tick_shared(
                &session,
                &catalog,
                &crate::test_support::inputs(&catalog),
                "host",
                expired + 1,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            let live = session.lock().unwrap();
            assert_eq!(
                live.projection().unwrap().objects["Task"]["T"]["state"],
                "ready"
            );
            assert_eq!(
                live.projection().unwrap().objects["Run"][run["id"].as_str().unwrap()]["state"],
                "released"
            );
            assert_eq!(
                live.events(applied)
                    .iter()
                    .filter(|event| event.kind == "task.released")
                    .collect::<Vec<_>>(),
                vec![&releases[0]]
            );
        }
    }
    #[test]
    fn expired_original_keeps_pending_on_source_or_final_canonical_proof_failure() {
        use crate::project_session::ProjectSession;
        let partial = true;
        for canonical in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let old = crate::test_support::catalog(dir.path()).revision;
            crate::test_support::git(dir.path(), &["switch", "main"]);
            crate::test_support::git(
                dir.path(),
                &["commit", "--allow-empty", "-qm", "coordination S1"],
            );
            crate::test_support::git(dir.path(), &["switch", "t/T"]);
            let catalog = Catalog::load(dir.path()).unwrap();
            let mut inputs = crate::test_support::inputs(&catalog);
            let path = dir.path().join("hub");
            let session = std::sync::Mutex::new(
                ProjectSession::new(
                    Box::new(FailClaim {
                        inner: crate::hub::Hub::open(&path).unwrap(),
                        accepted_kind: "task.claimed",
                        after_renewal: None,
                        partial,
                        failed: false,
                        after_accept: None,
                    }),
                    &catalog,
                    &crate::test_support::inputs(&catalog),
                    0,
                )
                .unwrap(),
            );
            ProjectSession::execute_shared(
                &session,
                &catalog,
                &inputs,
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
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            inputs = crate::test_support::inputs(&catalog);
            let uncertain = ProjectSession::execute_shared(
                &session,
                &catalog,
                &inputs,
                "host",
                TaskCommand::Claim {
                    id: "T".into(),
                    harness: None,
                    harness_version: None,
                },
                None,
                2,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap_err();
            let pending = session
                .lock()
                .unwrap()
                .pending_claim()
                .unwrap()
                .unwrap()
                .clone();
            assert!(uncertain.to_string().contains(&pending.event.eid));
            assert!(uncertain.to_string().contains(&pending.run_id));
            drop(session);
            let reopened = ProjectSession::new(
                Box::new(FailClaim {
                    inner: crate::hub::Hub::open(&path).unwrap(),
                    accepted_kind: "task.claimed",
                    after_renewal: None,
                    partial: false,
                    failed: false,
                    after_accept: Some((
                        ClaimSourceChange::Main(
                            dir.path().into(),
                            if canonical {
                                catalog.revision.clone()
                            } else {
                                old
                            },
                        ),
                        canonical,
                    )),
                }),
                &catalog,
                &crate::test_support::inputs(&catalog),
                3,
            )
            .unwrap();
            assert_eq!(reopened.pending_claim().unwrap(), Some(&pending));
            let session = std::sync::Mutex::new(reopened);
            inputs = crate::test_support::inputs(&catalog);
            let now = LEASE_TTL_MS + 3;
            // Establish the verified prefix after normal lease expiration,
            // before injecting a failure in the stale claim's final proof.
            ProjectSession::sync_shared(&session, &catalog, &inputs, now).unwrap();
            let before = session.lock().unwrap().projection().unwrap().clone();
            let applied = session.lock().unwrap().applied_seq();
            assert_eq!(!before.objects.contains_key("Run"), partial);
            let error = ProjectSession::reconcile_pending_shared(
                &session,
                &catalog,
                &inputs,
                now,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap_err();
            let live = session.lock().unwrap();
            assert_eq!(*live.projection().unwrap(), before);
            assert_eq!(live.applied_seq(), applied);
            assert_eq!(live.pending_claim().unwrap().unwrap().event, pending.event);
            assert_eq!(
                live.pending_claim().unwrap().unwrap().run_id,
                pending.run_id
            );
            drop(live);
            let records = session
                .lock()
                .unwrap()
                .hub
                .subscribe(0, now)
                .unwrap()
                .events;
            let claims: Vec<_> = records
                .iter()
                .filter(|event| event.kind == "task.claimed")
                .collect();
            assert_eq!(claims.len(), 1);
            assert!(claims[0].stale, "负控必须实际走 Expired 原件路径");
            let confirmed = crate::test_support::inputs(&catalog);
            assert!(confirmed.receipt_confirmed(claims[0]));
            assert_eq!(claims[0].guards, pending.event.guards);
            if canonical {
                assert_eq!(
                    crate::catalog::authority_revision(&catalog.root).unwrap(),
                    catalog.revision
                );
                assert!(
                    records
                        .iter()
                        .any(|event| event.kind == "merge.done" && event.obj == "fake"),
                    "权威来源保持相同，必须由最终真实 canonical proof 拒绝未证明 merge"
                );
            }
            assert_eq!(
                crate::recovery_outbox::original_of(claims[0]),
                pending.event
            );
            assert_eq!(
                records
                    .iter()
                    .filter(|event| event.kind == "lease.granted")
                    .count(),
                1
            );
            assert!(
                error.contains(&pending.event.eid),
                "original request identity missing: {error}"
            );
            assert!(
                error.contains(&pending.run_id),
                "original Run identity missing: {error}"
            );
        }
    }
    #[test]
    fn uncertain_claim_reconciliation_preserves_original_when_source_changes_after_acceptance() {
        use crate::project_session::ProjectSession;
        for partial in [false, true] {
            for canonical in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let old = crate::test_support::catalog(dir.path()).revision;
                crate::test_support::git(dir.path(), &["switch", "main"]);
                crate::test_support::git(
                    dir.path(),
                    &["commit", "--allow-empty", "-qm", "coordination S1"],
                );
                crate::test_support::git(dir.path(), &["switch", "t/T"]);
                let catalog = Catalog::load(dir.path()).unwrap();
                let mut inputs = crate::test_support::inputs(&catalog);
                let path = dir.path().join("hub");
                let session = std::sync::Mutex::new(
                    ProjectSession::new(
                        Box::new(FailClaim {
                            inner: crate::hub::Hub::open(&path).unwrap(),
                            accepted_kind: "task.claimed",
                            after_renewal: None,
                            partial,
                            failed: false,
                            after_accept: None,
                        }),
                        &catalog,
                        &crate::test_support::inputs(&catalog),
                        0,
                    )
                    .unwrap(),
                );
                ProjectSession::execute_shared(
                    &session,
                    &catalog,
                    &inputs,
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
                    &mut crate::test_support::outbox(&catalog),
                )
                .unwrap();
                inputs = crate::test_support::inputs(&catalog);
                let uncertain = ProjectSession::execute_shared(
                    &session,
                    &catalog,
                    &inputs,
                    "host",
                    TaskCommand::Claim {
                        id: "T".into(),
                        harness: None,
                        harness_version: None,
                    },
                    None,
                    2,
                    &mut crate::test_support::outbox(&catalog),
                )
                .unwrap_err();
                let pending = session
                    .lock()
                    .unwrap()
                    .pending_claim()
                    .unwrap()
                    .unwrap()
                    .clone();
                assert!(uncertain.to_string().contains(&pending.event.eid));
                assert!(uncertain.to_string().contains(&pending.run_id));
                drop(session);
                let mut reopened = ProjectSession::new(
                    Box::new(FailClaim {
                        inner: crate::hub::Hub::open(&path).unwrap(),
                        accepted_kind: "task.claimed",
                        after_renewal: None,
                        partial: false,
                        failed: false,
                        after_accept: Some((
                            ClaimSourceChange::Main(dir.path().into(), old),
                            canonical,
                        )),
                    }),
                    &catalog,
                    &crate::test_support::inputs(&catalog),
                    3,
                )
                .unwrap();
                reopened.tasks.pending_claim = Some(pending.clone());
                let before = reopened.projection().unwrap().clone();
                let applied = reopened.applied_seq();
                assert_eq!(!before.objects.contains_key("Run"), partial);
                let session = std::sync::Mutex::new(reopened);
                inputs = crate::test_support::inputs(&catalog);
                let error = ProjectSession::reconcile_pending_shared(
                    &session,
                    &catalog,
                    &inputs,
                    4,
                    &mut crate::test_support::outbox(&catalog),
                )
                .unwrap_err();
                let live = session.lock().unwrap();
                assert_eq!(*live.projection().unwrap(), before);
                assert_eq!(live.applied_seq(), applied);
                assert_eq!(live.pending_claim().unwrap().unwrap().event, pending.event);
                assert_eq!(
                    live.pending_claim().unwrap().unwrap().run_id,
                    pending.run_id
                );
                drop(live);
                let records = session.lock().unwrap().hub.subscribe(0, 4).unwrap().events;
                let claims: Vec<_> = records
                    .iter()
                    .filter(|event| event.kind == "task.claimed")
                    .collect();
                assert_eq!(claims.len(), 1);
                assert_eq!(
                    crate::recovery_outbox::original_of(claims[0]),
                    pending.event
                );
                assert_eq!(
                    records
                        .iter()
                        .filter(|event| event.kind == "lease.granted")
                        .count(),
                    1
                );
                assert!(
                    error.contains(&pending.event.eid),
                    "original request identity missing: {error}"
                );
                assert!(
                    error.contains(&pending.run_id),
                    "original Run identity missing: {error}"
                );
                if !canonical {
                    crate::test_support::git(
                        dir.path(),
                        &["update-ref", "refs/heads/main", &catalog.revision],
                    );
                    ProjectSession::reconcile_pending_shared(
                        &session,
                        &catalog,
                        &crate::test_support::inputs(&catalog),
                        5,
                        &mut crate::test_support::outbox(&catalog),
                    )
                    .unwrap();
                    let live = session.lock().unwrap();
                    assert!(live.pending_claim().unwrap().is_none());
                    assert_eq!(
                        live.projection().unwrap().objects["Run"][&pending.run_id]["state"],
                        "running"
                    );
                    assert_eq!(
                        live.events(0)
                            .iter()
                            .filter(|event| event.eid == pending.event.eid)
                            .count(),
                        1
                    );
                }
            }
        }
    }
    fn accepted_claim_changed_source_keeps_original(source: &str, canonical: bool) {
        use crate::project_session::ProjectSession;
        let dir = tempfile::tempdir().unwrap();
        let old = crate::test_support::catalog(dir.path()).revision;
        crate::test_support::git(dir.path(), &["switch", "main"]);
        crate::test_support::git(
            dir.path(),
            &["commit", "--allow-empty", "-qm", "accepted S1"],
        );
        crate::test_support::git(dir.path(), &["switch", "t/T"]);
        let catalog = Catalog::load(dir.path()).unwrap();
        let mut inputs = crate::test_support::inputs(&catalog);
        let mut private_current = None;
        let mut private_change = None;
        let change = match source {
            "main" => ClaimSourceChange::Main(dir.path().into(), old),
            "inputs" => ClaimSourceChange::Inputs(dir.path().into()),
            "private" | "private-forward" => {
                let main_worktree = dir.path().join("main-worktree");
                crate::test_support::git(
                    dir.path(),
                    &[
                        "worktree",
                        "add",
                        "-q",
                        main_worktree.to_str().unwrap(),
                        "main",
                    ],
                );
                let seed = std::sync::Mutex::new(
                    ProjectSession::open(&dir.path().join("hub"), &catalog, &inputs, 0).unwrap(),
                );
                let writer = crate::snapshot_service::SnapshotWriter::default();
                assert_eq!(
                    writer
                        .write(
                            &seed,
                            &catalog,
                            "host",
                            true,
                            || Ok(10),
                            &mut crate::test_support::outbox(&catalog)
                        )
                        .unwrap()
                        .status,
                    "written"
                );
                let s0 = crate::snapshot_git::private_revision(dir.path())
                    .unwrap()
                    .unwrap();
                seed.lock()
                    .unwrap()
                    .hub
                    .append(
                        NewEvent {
                            eid: uuid::Uuid::now_v7().to_string(),
                            host: "host".into(),
                            actor: "process:audit".into(),
                            kind: "alert.raised".into(),
                            layer: "L4".into(),
                            obj: "private-S1".into(),
                            guards: vec![],
                            data: json!({"reason":"advance private source"}),
                        },
                        11,
                    )
                    .unwrap();
                assert_eq!(
                    writer
                        .write(
                            &seed,
                            &catalog,
                            "host",
                            true,
                            || Ok(12),
                            &mut crate::test_support::outbox(&catalog)
                        )
                        .unwrap()
                        .status,
                    "written"
                );
                let s1 = crate::snapshot_git::private_revision(dir.path())
                    .unwrap()
                    .unwrap();
                assert_ne!(s0, s1);
                assert_eq!(seed.lock().unwrap().snapshot_ack().unwrap().commit, s1);
                private_current = Some(s1);
                drop(seed);
                let target = std::sync::Arc::new(std::sync::Mutex::new(s0));
                private_change = Some(target.clone());
                ClaimSourceChange::Private(dir.path().into(), target)
            }
            _ => unreachable!(),
        };
        let protocol = FailClaim {
            inner: crate::hub::Hub::open(&dir.path().join("hub")).unwrap(),
            accepted_kind: "task.claimed",
            after_renewal: None,
            partial: false,
            failed: false,
            after_accept: Some((change, canonical)),
        };
        let session = std::sync::Mutex::new(
            ProjectSession::new(
                Box::new(protocol),
                &catalog,
                &crate::test_support::inputs(&catalog),
                0,
            )
            .unwrap(),
        );
        let record_floor = session
            .lock()
            .unwrap()
            .snapshot_ack()
            .map_or(0, |ack| ack.seq);
        ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
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
            &mut crate::test_support::outbox(&catalog),
        )
        .unwrap();
        inputs = crate::test_support::inputs(&catalog);
        if source == "private-forward" {
            // Persist a genuine higher snapshot of this very Hub prefix without
            // acknowledging or installing it, then reveal it only after Claim acceptance.
            let saved = crate::snapshot_service::read_snapshot(&catalog)
                .unwrap()
                .unwrap();
            let (snapshot, tail) = {
                let live = session.lock().unwrap();
                let fence = live.leases(1)["main"].fence();
                (
                    live.checkpoint(catalog.config.project_id(), &fence, 1)
                        .unwrap(),
                    live.events(saved.snapshot.through_seq),
                )
            };
            assert!(snapshot.through_seq > saved.snapshot.through_seq);
            let mut archives = saved.archives;
            for event in tail {
                archives
                    .entry(event.t[..7].to_string())
                    .or_default()
                    .push_str(&format!("{}\n", serde_json::to_string(&event).unwrap()));
            }
            let outcome = crate::snapshot_git::write_snapshot(
                &catalog.root,
                &crate::snapshot_git::WriteRequest {
                    base: catalog.revision.clone(),
                    docs_dir: catalog.config.docs_dir().into(),
                    state_json: serde_json::to_string(&snapshot).unwrap(),
                    archives,
                    through_seq: snapshot.through_seq,
                    writer_epoch: snapshot.writer_epoch,
                },
                || Ok(()),
            )
            .unwrap();
            let crate::snapshot_git::WriteOutcome::Written { commit, .. } = outcome else {
                panic!("higher private snapshot must persist")
            };
            assert_eq!(
                crate::snapshot_service::read_snapshot(&catalog)
                    .unwrap()
                    .unwrap()
                    .snapshot,
                snapshot
            );
            crate::test_support::git(
                dir.path(),
                &[
                    "update-ref",
                    "refs/towermd/snapshot",
                    private_current.as_ref().unwrap(),
                    &commit,
                ],
            );
            *private_change.as_ref().unwrap().lock().unwrap() = commit;
        }
        let projection_before = session.lock().unwrap().projection().unwrap().clone();
        let applied_before = session.lock().unwrap().applied_seq();
        let result = ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            },
            None,
            2,
            &mut crate::test_support::outbox(&catalog),
        );
        assert!(
            result.is_err(),
            "accepted claim source change must return original-result coordination: {result:?}"
        );
        let pending = session
            .lock()
            .unwrap()
            .pending_claim()
            .unwrap()
            .cloned()
            .expect("accepted original must survive post-sync failure");
        assert_eq!(
            *session.lock().unwrap().projection().unwrap(),
            projection_before,
            "failed final actual binding must not apply accepted ordinary tail"
        );
        assert_eq!(session.lock().unwrap().applied_seq(), applied_before);
        assert!(!session
            .lock()
            .unwrap()
            .tasks
            .runs
            .contains_key(&pending.run_id));
        let records = session
            .lock()
            .unwrap()
            .hub
            .subscribe(record_floor, 2)
            .unwrap()
            .events;
        let actual = records
            .iter()
            .find(|event| event.kind == "task.claimed")
            .unwrap();
        assert_eq!(crate::recovery_outbox::original_of(actual), pending.event);
        assert_eq!(pending.run_id, actual.data["run"]["id"]);
        assert_eq!(
            pending.event.guards,
            serde_json::from_value::<Vec<Fence>>(actual.data["run"]["leases"].clone()).unwrap()
        );
        if source == "private-forward" {
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("private snapshot source changed during request"),
                "{error}"
            );
            // This is a distinct normal request. It may prove the actual higher
            // source, and coordinates the original accepted claim without replaying Claim.
            ProjectSession::reconcile_pending_shared(
                &session,
                &catalog,
                &crate::test_support::inputs(&catalog),
                3,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            let live = session.lock().unwrap();
            assert!(live.pending_claim().unwrap().is_none());
            assert_eq!(
                live.projection().unwrap().objects["Run"][&pending.run_id]["state"],
                "running"
            );
            assert_eq!(
                live.events(record_floor)
                    .iter()
                    .filter(|event| event.eid == pending.event.eid)
                    .count(),
                1
            );
            assert_eq!(
                live.events(record_floor)
                    .iter()
                    .filter(|event| event.kind == "task.claimed")
                    .count(),
                1
            );
            return;
        }
        for operation in ["tick", "reconcile"] {
            let result = if operation == "tick" {
                ProjectSession::tick_shared(
                    &session,
                    &catalog,
                    &inputs,
                    "host",
                    3,
                    &mut crate::test_support::outbox(&catalog),
                )
            } else {
                ProjectSession::reconcile_pending_shared(
                    &session,
                    &catalog,
                    &inputs,
                    3,
                    &mut crate::test_support::outbox(&catalog),
                )
            };
            assert!(result.is_err());
            assert_eq!(
                session
                    .lock()
                    .unwrap()
                    .pending_claim()
                    .unwrap()
                    .unwrap()
                    .event,
                pending.event
            );
        }
        let again = ProjectSession::execute_shared(
            &session,
            &catalog,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "different-task".into(),
                harness: None,
                harness_version: None,
            },
            None,
            3,
            &mut crate::test_support::outbox(&catalog),
        );
        assert!(again.is_err());
        let records = session
            .lock()
            .unwrap()
            .hub
            .subscribe(record_floor, 3)
            .unwrap()
            .events;
        assert_eq!(
            records
                .iter()
                .filter(|event| event.kind == "task.claimed")
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|event| event.kind == "lease.granted" && event.obj != "main")
                .count(),
            1
        );
        if !canonical {
            match source {
                "main" => {
                    crate::test_support::git(
                        dir.path(),
                        &["update-ref", "refs/heads/main", &catalog.revision],
                    );
                }
                "inputs" => {
                    repair_external_inputs(dir.path());
                }
                "private" | "private-forward" => {
                    crate::test_support::git(
                        dir.path(),
                        &[
                            "update-ref",
                            "refs/towermd/snapshot",
                            private_current.as_ref().unwrap(),
                        ],
                    );
                }
                _ => unreachable!(),
            }
            ProjectSession::reconcile_pending_shared(
                &session,
                &catalog,
                &crate::test_support::inputs(&catalog),
                4,
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
            assert!(session.lock().unwrap().pending_claim().unwrap().is_none());
            assert_eq!(
                session.lock().unwrap().projection().unwrap().objects["Run"][&pending.run_id]
                    ["state"],
                "running"
            );
            let records = session
                .lock()
                .unwrap()
                .hub
                .subscribe(record_floor, 4)
                .unwrap()
                .events;
            assert_eq!(
                records
                    .iter()
                    .filter(|event| event.eid == pending.event.eid)
                    .count(),
                1
            );
        }
    }
    #[test]
    fn accepted_claim_private_advance_keeps_last_verified_view() {
        accepted_claim_changed_source_keeps_original("private-forward", false);
    }
    #[test]
    fn accepted_claim_inputs_change_retains_original_request_and_result() {
        accepted_claim_changed_source_keeps_original("inputs", false);
    }
    #[test]
    fn accepted_claim_inputs_change_during_proof_retains_original_request_and_result() {
        accepted_claim_changed_source_keeps_original("inputs", true);
    }
    #[test]
    fn accepted_claim_private_rollback_retains_original_request_and_result() {
        accepted_claim_changed_source_keeps_original("private", false);
    }
    #[test]
    fn accepted_claim_private_rollback_during_proof_retains_original_request_and_result() {
        accepted_claim_changed_source_keeps_original("private", true);
    }
    #[test]
    fn accepted_claim_source_change_retains_original_request_and_result() {
        accepted_claim_changed_source_keeps_original("main", false);
    }
    #[test]
    fn accepted_claim_post_sync_proof_failure_retains_original_request_and_result() {
        accepted_claim_changed_source_keeps_original("main", true);
    }
}
