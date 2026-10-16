use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::catalog::{validate_paths, validate_protected_paths, Catalog};
use crate::hub::{Event, Fence, Hub, Lease, LeaseChange, NewEvent};
use crate::okf::Document;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    #[serde(rename = "type")]
    pub object_type: String,
    #[serde(flatten)]
    pub spec: TaskSpec,
    pub state: String,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelfReported {
    pub harness: String,
    pub version: String,
}

#[derive(Default)]
struct Projection {
    tasks: BTreeMap<String, Task>,
    runs: BTreeMap<String, Run>,
}

#[derive(Serialize, Deserialize)]
struct Change {
    task: Task,
    #[serde(default)]
    run: Option<Run>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    self_reported: Option<SelfReported>,
}

impl Projection {
    fn replay(hub: &Hub) -> Result<Self, String> {
        let mut projection = Self::default();
        if let Some(base) = hub.base_snapshot() {
            if let Some(tasks) = base.projection.objects.get("Task") {
                for (id, value) in tasks {
                    let task: Task = serde_json::from_value(value.clone())
                        .map_err(|error| format!("invalid restored Task {id}: {error}"))?;
                    if task.spec.id != *id {
                        return Err("restored Task ID mismatch".into());
                    }
                    projection.tasks.insert(id.clone(), task);
                }
            }
            if let Some(runs) = base.projection.objects.get("Run") {
                for (id, value) in runs {
                    let run: Run = serde_json::from_value(value.clone())
                        .map_err(|error| format!("invalid restored Run {id}: {error}"))?;
                    if run.id != *id {
                        return Err("restored Run ID mismatch".into());
                    }
                    projection.runs.insert(id.clone(), run);
                }
            }
        }
        for event in hub.events(0) {
            if event.stale || !matches!(event.kind.as_str(), "task.state" | "task.claimed") {
                continue;
            }
            let change: Change = serde_json::from_value(event.data.clone())
                .map_err(|error| format!("invalid task event {}: {error}", event.seq))?;
            projection
                .tasks
                .insert(change.task.spec.id.clone(), change.task);
            if let Some(mut run) = change.run {
                if event.kind == "task.claimed" {
                    run.leases = event.updated_leases().iter().map(Lease::fence).collect();
                    if run.leases.is_empty() {
                        return Err(format!("claim event {} has no leases", event.seq));
                    }
                }
                projection.runs.insert(run.id.clone(), run);
            }
        }
        Ok(projection)
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
        Ok(self
            .runs
            .values()
            .find(|run| {
                run.state == "running"
                    && run.host == host
                    && run.worktree == catalog.root.to_string_lossy()
                    && run.branch == catalog.branch
            })
            .cloned())
    }

    fn local_runs(&self, host: &str) -> String {
        let runs: Vec<_> = self.runs.values().filter(|run| run.host == host).map(|run| {
            json!({"id": run.id, "task": run.task, "worktree": run.worktree, "branch": run.branch, "state": run.state})
        }).collect();
        format!("local runs: {}", json!(runs))
    }
}

pub fn execute(
    hub: &mut Hub,
    catalog: &Catalog,
    host: &str,
    command: TaskCommand,
    run_override: Option<&str>,
    now_ms: u64,
) -> Result<Value, String> {
    expire(hub, host, now_ms)?;
    let projection = Projection::replay(hub)?;
    let bound = projection.binding(catalog, host, run_override)?;
    if let Some(run) = &bound {
        renew_run(hub, host, run, now_ms)?;
    }
    match command {
        TaskCommand::Status => Ok(json!({
            "project_id": catalog.config.project_id(),
            "seq": hub.last_seq(),
            "tasks": projection.tasks.values().collect::<Vec<_>>(),
            "runs": projection.runs.values().collect::<Vec<_>>(),
            "leases": hub.leases(now_ms).values().collect::<Vec<_>>(),
        })),
        TaskCommand::Add { task: spec } => {
            validate_task(&spec, catalog)?;
            if projection.tasks.contains_key(&spec.id) || catalog.objects.contains_key(&spec.id) {
                return Err(format!("task ID {} already exists", spec.id));
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
            };
            append_change(hub, host, "task.state", &task, None, None, now_ms)?;
            Ok(json!(task))
        }
        TaskCommand::Show { id: Some(id) } => {
            let task = projection
                .tasks
                .get(&id)
                .ok_or_else(|| format!("unknown task {id}"))?;
            render_task_card(task, catalog)
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
            release_run(hub, host, &projection, run, now_ms, "process:towermd-cli")
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
                        "harness and harness_version must both be nonempty or both omitted".into(),
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
                return Err(format!("task {id} dependencies are missing or not merged"));
            }
            if catalog.branch != format!("t/{id}") {
                return Err(format!(
                    "claim {id} requires an existing t/{id} branch and target worktree"
                ));
            }
            if bound.is_some()
                || projection.runs.values().any(|run| {
                    run.state == "running"
                        && run.host == host
                        && run.worktree == catalog.root.to_string_lossy()
                })
            {
                return Err(format!(
                    "current worktree already has an active run; {}",
                    projection.local_runs(host)
                ));
            }
            let names = lease_names(&task.spec);
            let leases = hub.leases(now_ms);
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
                return Err(format!(
                    "task {id} module lease is occupied; available tasks: {}",
                    json!(available)
                ));
            }
            if task.state != "ready" && task.state != "proposed" {
                return Err(format!("task {id} is not ready"));
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
            };
            let event = append_change(
                hub,
                host,
                "task.claimed",
                &task,
                Some(&run),
                Some(LeaseChange::Acquire {
                    names,
                    holder: run.id.clone(),
                    ttl_ms: LEASE_TTL_MS,
                }),
                now_ms,
            )?;
            run.leases = event.updated_leases().iter().map(Lease::fence).collect();
            Ok(json!(run))
        }
    }
}

pub fn renew_binding(
    hub: &mut Hub,
    catalog: &Catalog,
    host: &str,
    run_override: Option<&str>,
    now_ms: u64,
) -> Result<(), String> {
    let projection = Projection::replay(hub)?;
    let Some(run) = projection.binding(catalog, host, run_override)? else {
        return Ok(());
    };
    let leases = hub.leases(now_ms);
    if run.leases.is_empty() || run.leases.iter().any(|fence| !active(fence, &leases)) {
        return if run_override.is_some() {
            Err(format!("run {} 不再持有有效租约", run.id))
        } else {
            Ok(())
        };
    }
    renew_run(hub, host, &run, now_ms)
}

fn renew_run(hub: &mut Hub, host: &str, run: &Run, now_ms: u64) -> Result<(), String> {
    let event = hub.append(
        new_event(
            host,
            "lease.renewed",
            "L5",
            &run.id,
            json!({"run": run.id}),
            run.leases.first().cloned(),
        ),
        Some(LeaseChange::Renew {
            fences: run.leases.clone(),
            ttl_ms: LEASE_TTL_MS,
        }),
        now_ms,
    )?;
    require_current(&event)
}

pub fn expire(hub: &mut Hub, host: &str, now_ms: u64) -> Result<(), String> {
    let projection = Projection::replay(hub)?;
    let leases = hub.leases(now_ms);
    for run in projection
        .runs
        .values()
        .filter(|run| run.state == "running")
    {
        if run.leases.is_empty() || run.leases.iter().any(|fence| !active(fence, &leases)) {
            release_run(
                hub,
                host,
                &projection,
                run.clone(),
                now_ms,
                "process:towermd-daemon",
            )?;
        }
    }
    Ok(())
}

fn active(fence: &Fence, leases: &BTreeMap<String, Lease>) -> bool {
    leases
        .get(&fence.name)
        .is_some_and(|lease| lease.holder == fence.holder && lease.epoch == fence.epoch)
}

fn release_run(
    hub: &mut Hub,
    host: &str,
    projection: &Projection,
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
    let leases = hub.leases(now_ms);
    let fences: Vec<_> = run
        .leases
        .iter()
        .filter(|fence| active(fence, &leases))
        .cloned()
        .collect();
    let change = if fences.is_empty() {
        None
    } else {
        Some(LeaseChange::Release { fences })
    };
    let mut event = new_event(
        host,
        "task.state",
        "L4",
        &task.spec.id,
        json!(Change {
            task: task.clone(),
            run: Some(run),
            self_reported: None,
        }),
        None,
    );
    event.actor = actor.into();
    let event = hub.append(event, change, now_ms)?;
    require_current(&event)?;
    Ok(json!(task))
}

fn append_change(
    hub: &mut Hub,
    host: &str,
    kind: &str,
    task: &Task,
    run: Option<&Run>,
    change: Option<LeaseChange>,
    now_ms: u64,
) -> Result<Event, String> {
    let event = hub.append(
        new_event(
            host,
            kind,
            "L4",
            &task.spec.id,
            json!(Change {
                task: task.clone(),
                run: run.cloned(),
                self_reported: if kind == "task.claimed" {
                    run.and_then(|run| run.self_reported.clone())
                } else {
                    None
                },
            }),
            None,
        ),
        change,
        now_ms,
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
    fence: Option<Fence>,
) -> NewEvent {
    NewEvent {
        eid: Uuid::now_v7().to_string(),
        host: host.into(),
        actor: "process:towermd-cli".into(),
        kind: kind.into(),
        layer: layer.into(),
        obj: obj.into(),
        data,
        fence,
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
