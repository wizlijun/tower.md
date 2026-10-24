//! Temporary rules (core spec FR-69). Adding one is a person's act: only the
//! App reaches the route, signed `human:local`; the CLI has none. Rules in
//! force reach delegates in two places: every CLI call of a covered run
//! repeats them in `delivered[]` (like report requests, never acknowledged),
//! and `towermd task show` lists the ones covering the task. Maintenance
//! records each lapsed rule once as `rule.temp.expired`.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{hub_protocol::NewEvent, project_runtime::{ProjectRuntime, RuntimeError},
    question_model::{PromptScope, RunContext}, quiet::instant, temp_rules::{self, Added, TempRule}, tower_api};
use towermd_devkit::dev_state;

const HUMAN: &str = "human:local";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Add { request_id: String, text: String, scope: PromptScope, until: String }

pub fn rule_id(request_id: &str) -> String {
    format!("TR-{request_id}")
}

fn usage(error: String) -> ApiError {
    rejected_as(ErrorCode::Usage)(error)
}

fn invalid(error: RuntimeError) -> ApiError {
    match error { RuntimeError::Invalid(message) => bad(message), other => uncertain(other) }
}

/// A scope names a task or a catalog object that exists; a typo would
/// silently cover nothing.
fn known_scope(runtime: &ProjectRuntime, project: &Project, scope: &PromptScope) -> Result<(), ApiError> {
    match scope {
        PromptScope::Global => Ok(()),
        PromptScope::Task { task } => {
            let tasks = dev_state::tasks(runtime.projection()).map_err(bad)?;
            if tasks.tasks.contains_key(task) { Ok(()) } else { Err(rejected_as(ErrorCode::NotFound)(format!("没有任务 {task}"))) }
        }
        PromptScope::Scoped { scope } => {
            let catalog = current_catalog(project).map_err(bad)?;
            if catalog.objects.contains_key(scope) { Ok(()) } else { Err(rejected_as(ErrorCode::NotFound)(format!("没有模块 {scope}"))) }
        }
    }
}

pub async fn add(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Add>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    valid_scan_id(&input.request_id).map_err(bad)?;
    temp_rules::check_text(&input.text).map_err(usage)?;
    temp_rules::check_scope(&input.scope).map_err(usage)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let id = rule_id(&input.request_id);
        let data = serde_json::to_value(Added { text: input.text, scope: input.scope.clone(), until: input.until.clone() }).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == temp_rules::ADDED && old.obj == id && old.data == data => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => {
                temp_rules::check_until(&input.until, instant(now())).map_err(usage)?;
                known_scope(runtime, &project, &input.scope)?;
                NewEvent { eid: input.request_id.clone(), host: server.endpoint.host.clone(), actor: HUMAN.into(),
                    kind: temp_rules::ADDED.into(), layer: temp_rules::LAYER.into(), obj: id, data, guards: vec![] }
            }
        };
        // Like a follow-up, a rule waits for the next due snapshot to be archived.
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "rule":runtime.projection().get(temp_rules::GROUP).and_then(|group| group.get(&event.obj)).cloned()})))
    }).await.map_err(uncertain)?
}

/// Maintenance: records each rule whose expiry passed, once. The eid is
/// derived from the rule, so a retry or a second pass writes nothing new.
pub(super) fn expire(server: &Server, runtime: &mut ProjectRuntime) -> Result<usize, String> {
    let at = instant(now());
    let lapsed: Vec<String> = temp_rules::read(runtime.projection())?.into_values().filter(|rule| rule.lapsed(at)).map(|rule| rule.id).collect();
    let mut written = 0;
    for id in lapsed {
        let eid = tower_api::signal_eid(temp_rules::EXPIRED, &id);
        if runtime.receipt(&eid).is_some() { continue; }
        let request = match runtime.pending().map_err(|e| e.to_string())?.into_iter().find(|event| event.eid == eid) {
            Some(queued) => queued,
            None => NewEvent { eid, host: server.endpoint.host.clone(), actor: temp_rules::EXPIRER.into(), kind: temp_rules::EXPIRED.into(),
                layer: temp_rules::LAYER.into(), obj: id, data: json!({}), guards: vec![] },
        };
        runtime.append(request, now).map_err(|e| e.to_string())?;
        written += 1;
    }
    Ok(written)
}

fn rule_value(rule: &TempRule) -> Value {
    json!({"id":rule.id,"text":rule.text,"scope":rule.scope,"until":rule.until,"set_by":rule.added.actor})
}

/// The rules in force for a run, as `delivered[]` items repeated on every call.
pub(super) fn messages(projection: &Value, run: &RunContext) -> Result<Vec<Value>, String> {
    let rules = temp_rules::read(projection)?;
    Ok(temp_rules::covering(&rules, run, instant(now())).into_iter().map(|rule| json!({
        "message":{"kind":"rule","rule":rule.id},"run":run.run,"actor":rule.added.actor,"until":rule.until,
        "text":format!("临时规则（{} 前有效）：{}", rule.until, rule.text),
    })).collect())
}

/// The rules in force that cover a task, for `towermd task show`.
pub(super) fn for_task(projection: &Value, task: &str, module: &str) -> Result<Vec<Value>, String> {
    let rules = temp_rules::read(projection)?;
    let context = RunContext { run: String::new(), task: task.into(), scope: module.into() };
    Ok(temp_rules::covering(&rules, &context, instant(now())).into_iter().map(rule_value).collect())
}
