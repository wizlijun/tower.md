//! Finite non-authorizing TaskBranch G2/G5 diagnostics. Evidence and Holds use
//! existing registered types. The original checked error remains a hard error.
use crate::{
    catalog::Catalog,
    hub_protocol::{Event, NewEvent},
    recovery_contract::{
        self as contract,
        recorded::{self, GateEvidence, GateGenerated, GateResult},
    },
    recovery_outbox,
    snapshot::StateProjection,
    task_gates::{Gate, GateError},
    tasks::{Run, Task},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Rejection {
    Protected { path: Vec<u8> },
    OutsideTask { path: Vec<u8> },
    NotLatestMain,
}
impl Rejection {
    pub(crate) fn from_error(error: &GateError) -> Option<Self> {
        match error {
            GateError::Protected(path) => Some(Self::Protected { path: path.clone() }),
            GateError::OutsideTask(path) => Some(Self::OutsideTask { path: path.clone() }),
            GateError::NotLatestMain => Some(Self::NotLatestMain),
            _ => None,
        }
    }
    fn gate(&self) -> Gate {
        match self {
            Self::NotLatestMain => Gate::G5,
            _ => Gate::G2,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    pub task: Task,
    pub run: Run,
    pub authority: String,
    pub baseline: String,
    pub commit: String,
    pub tree: String,
    pub checked_ms: u64,
    pub observed_through_seq: u64,
    pub previous: Option<String>,
    pub rejection: Option<Rejection>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Recorded {
    pub schema: u32,
    pub context: Context,
    pub evidence: GateEvidence,
    pub hold: Option<Value>,
}
fn run_identity(run: &Run) -> Result<Value, String> {
    let mut value = json!(run);
    value
        .as_object_mut()
        .ok_or("Run is not object")?
        .remove("last_check");
    Ok(value)
}
#[allow(
    clippy::too_many_arguments,
    reason = "exact original Task/Run/Git/result identity is explicit"
)]
pub(crate) fn same_subject(
    context: &Context,
    task: &Task,
    run: &Run,
    authority: &str,
    baseline: &str,
    tree: &str,
    rejection: &Option<Rejection>,
) -> Result<bool, String> {
    Ok(json!(&context.task) == json!(task)
        && run_identity(&context.run)? == run_identity(run)?
        && context.authority == authority
        && context.baseline == baseline
        && context.tree == tree
        && context.rejection == *rejection)
}
fn uuid(context: &Context) -> Result<String, String> {
    recorded::request_event_eid(&context.run.id, "evidence.recorded")?;
    let source = uuid::Uuid::parse_str(&context.run.id).map_err(|e| e.to_string())?;
    let digest = contract::canonical_hash(&(
        "towermd.task-check",
        1u32,
        &context.task,
        run_identity(&context.run)?,
        &context.authority,
        &context.baseline,
        &context.tree,
        &context.previous,
        &context.rejection,
    ))?;
    let mut bytes = *source.as_bytes();
    for (index, slot) in bytes.iter_mut().enumerate().skip(6) {
        *slot =
            u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    bytes[6] = (bytes[6] & 15) | 0x70;
    bytes[8] = (bytes[8] & 63) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}
fn hold(context: &Context, id: &str) -> Value {
    let rejection = context
        .rejection
        .as_ref()
        .expect("only failure gets a Hold");
    let path = match rejection {
        Rejection::Protected { path } | Rejection::OutsideTask { path } => Some(path),
        Rejection::NotLatestMain => None,
    };
    json!({"type":"Hold","id":format!("H-{id}"),"kind":if rejection.gate()==Gate::G2{"forbidden"}else{"gatefailed"},"scope":"TaskBranch","gate":if rejection.gate()==Gate::G2{"G2"}else{"G5"},
        "task":context.task.spec.id,"run":context.run.id,"evidence":format!("E-{id}"),"commit":context.commit,"tree":context.tree,"authority":context.authority,"baseline":context.baseline,"fences":context.run.leases,
        "rejection":rejection,"path_bytes":path,"path":path.map(|p|String::from_utf8_lossy(p).into_owned()),"opened_ms":context.checked_ms})
}
pub(crate) fn planned(context: Context, project: &str) -> Result<NewEvent, String> {
    let id = uuid(&context)?;
    let which = context.rejection.as_ref().map_or(Gate::G2, Rejection::gate);
    let evidence = GateEvidence {
        object_type: "Evidence".into(),
        id: format!("E-{id}"),
        kind: "gate".into(),
        gate: which,
        project_id: project.into(),
        task: context.task.spec.id.clone(),
        run: context.run.id.clone(),
        module: context.task.spec.module.clone(),
        commit: Some(context.commit.clone()),
        tree: Some(context.tree.clone()),
        base: Some(context.authority.clone()),
        authority: Some(context.authority.clone()),
        baseline: Some(context.baseline.clone()),
        plan_hash: None,
        plan_opening_eid: None,
        commands_hash: None,
        fences: context.run.leases.clone(),
        result: if context.rejection.is_some() {
            GateResult::Failed
        } else {
            GateResult::Passed
        },
        review: None,
        diagnostic: context
            .rejection
            .as_ref()
            .map(|r| serde_json::to_string(r).map_err(|e| e.to_string()))
            .transpose()?,
        generated: GateGenerated {
            by: "process:towermd-gates".into(),
            at: crate::hub::timestamp(context.checked_ms)
                .map_err(|e| e.to_string())?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        },
        extra: BTreeMap::from([("task_check".into(), json!(context))]),
    };
    let record = Recorded {
        schema: 1,
        hold: context.rejection.as_ref().map(|_| hold(&context, &id)),
        context,
        evidence,
    };
    Ok(NewEvent {
        eid: recorded::request_event_eid(&id, "evidence.recorded")?,
        host: record.context.run.host.clone(),
        actor: "process:towermd-gates".into(),
        kind: "evidence.recorded".into(),
        layer: "L5".into(),
        obj: record.evidence.id.clone(),
        guards: record.context.run.leases.clone(),
        data: json!({"evidence":record.evidence,"task_check":record}),
    })
}
pub(crate) fn is_event(event: &Event) -> bool {
    event.kind == "evidence.recorded" && event.data.get("task_check").is_some()
}
pub(crate) fn decode(original: &NewEvent) -> Result<Recorded, String> {
    let record: Recorded =
        serde_json::from_value(original.data["task_check"].clone()).map_err(|e| e.to_string())?;
    contract::validate_gate_layout(&record.evidence)?;
    let c = &record.context;
    for oid in [&c.authority, &c.commit, &c.tree] {
        contract::object_id(oid)?;
    }
    crate::baseline_current::baseline_number(&c.baseline)?;
    let expected = std::iter::once(&c.task.spec.module)
        .chain(c.task.spec.touches.iter())
        .map(|id| format!("module:{id}"))
        .collect::<BTreeSet<_>>();
    if record.schema != 1
        || c.task.object_type != "Task"
        || c.run.object_type != "Run"
        || c.run.task != c.task.spec.id
        || c.run.branch != format!("t/{}", c.task.spec.id)
        || !matches!(c.task.state.as_str(), "running" | "verifying" | "held")
        || !matches!(c.run.state.as_str(), "running" | "verifying" | "held")
        || c.run.leases.len() != expected.len()
        || c.run.leases.windows(2).any(|w| w[0].name >= w[1].name)
        || c.run
            .leases
            .iter()
            .map(|f| f.name.clone())
            .collect::<BTreeSet<_>>()
            != expected
        || c.run
            .leases
            .iter()
            .any(|f| f.holder != c.run.id || f.epoch == 0)
        || c.previous.as_ref()
            != c.run
                .extra
                .get("last_check")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .as_ref()
        || *original != planned(c.clone(), &record.evidence.project_id)?
    {
        return Err("Task check diagnostic differs from its exact registered Task/Run/full fences/Git/error envelope".into());
    }
    Ok(record)
}
pub(crate) fn from_evidence(value: &Value) -> Result<Context, String> {
    let evidence: GateEvidence =
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    contract::validate_gate_layout(&evidence)?;
    let context: Context = serde_json::from_value(
        evidence
            .extra
            .get("task_check")
            .cloned()
            .ok_or("Evidence is not Task check diagnostic")?,
    )
    .map_err(|e| e.to_string())?;
    let original = planned(context.clone(), &evidence.project_id)?;
    if original.data["evidence"] != *value {
        return Err("Task check cached Evidence differs from exact original".into());
    }
    Ok(context)
}
pub(crate) fn verify_git(catalog: &Catalog, record: &Recorded) -> Result<(), String> {
    let c = &record.context;
    let at = Catalog::at_revision(&catalog.root, &c.authority)
        .map_err(|e| format!("task diagnostic source unavailable: {e}"))?
        .ok_or("Task check diagnostic authority lacks Catalog")?;
    if at.common_dir != catalog.common_dir
        || at.config.project_id() != catalog.config.project_id()
        || record.evidence.project_id != catalog.config.project_id()
    {
        return Err("Task check diagnostic belongs to another project/common/source".into());
    }
    crate::tasks::validate_task(&c.task.spec, &at)?;
    if crate::repo::git(
        &catalog.root,
        &["rev-parse", "--verify", &format!("{}^{{tree}}", c.commit)],
    )? != c.tree
    {
        return Err("Task check original tree differs from actual commit".into());
    }
    let latest = crate::task_gates::ancestor(&catalog.root, &c.authority, &c.commit)
        .map_err(|e| format!("{e:?}"))?;
    let actual = if !latest {
        Some(Rejection::NotLatestMain)
    } else {
        match crate::task_gates::check_paths_at(&at, &c.task, &c.authority, &c.commit) {
            Ok(_) => None,
            Err(error) => Some(
                Rejection::from_error(&error)
                    .ok_or_else(|| format!("unprovable Task check diagnostic {error:?}"))?,
            ),
        }
    };
    if actual != c.rejection {
        return Err("Task check diagnostic differs from original actual G2/G5 inspection".into());
    }
    Ok(())
}
pub(crate) fn project(projection: &mut StateProjection, event: &Event) -> Result<(), String> {
    let record = decode(&recovery_outbox::original_of(event))?;
    if event.stale {
        return Ok(());
    }
    if record.context.observed_through_seq >= event.seq {
        return Err("Task check diagnostic source is not before acceptance".into());
    }
    let task = projection
        .objects
        .get("Task")
        .and_then(|objects| objects.get(&record.context.task.spec.id))
        .ok_or("Task check diagnostic lacks actual Task")?;
    let run = projection
        .objects
        .get("Run")
        .and_then(|objects| objects.get(&record.context.run.id))
        .ok_or("Task check diagnostic lacks actual Run")?;
    if *task != json!(record.context.task) || *run != json!(record.context.run) {
        return Err("Task check diagnostic Task/Run differs at actual prefix".into());
    }
    let evidence = projection.objects.entry("Evidence".into()).or_default();
    if evidence
        .insert(record.evidence.id.clone(), json!(record.evidence))
        .is_some_and(|old| old != json!(record.evidence))
    {
        return Err("Task check immutable Evidence collision".into());
    }
    // Every proved new check supersedes earlier TaskBranch cards on this exact
    // Run/fence set. Their raw diagnosis and Evidence remain as history.
    // A Passed check cannot create an empty Hold map: canonical Human replay
    // has no such object, and the strict checkpoint comparison must stay exact.
    if let Some(holds) = projection.objects.get_mut("Hold") {
        for raw in holds.values_mut().filter(|raw| {
            raw["scope"] == "TaskBranch"
                && raw["run"] == record.context.run.id
                && raw["fences"] == json!(record.context.run.leases)
        }) {
            raw["resolved_by"] = json!(record.evidence.id);
        }
    }
    if let Some(hold) = record.hold {
        projection.objects.entry("Hold".into()).or_default().insert(
            hold["id"]
                .as_str()
                .ok_or("diagnostic Hold lacks ID")?
                .into(),
            hold,
        );
    }
    projection
        .objects
        .get_mut("Run")
        .and_then(|objects| objects.get_mut(&record.context.run.id))
        .ok_or("diagnostic Run disappeared")?["last_check"] = json!(record.evidence.id);
    Ok(())
}
pub(crate) fn strip_holds(projection: &mut StateProjection) -> Result<(), String> {
    let selected = projection
        .objects
        .get("Hold")
        .into_iter()
        .flat_map(|holds| holds.iter())
        .filter(|(_, hold)| {
            hold["scope"] == "TaskBranch"
                && matches!(hold["kind"].as_str(), Some("forbidden" | "gatefailed"))
        })
        .map(|(id, hold)| (id.clone(), hold.clone()))
        .collect::<Vec<_>>();
    for (id, raw) in selected {
        let evidence = projection
            .objects
            .get("Evidence")
            .and_then(|objects| raw["evidence"].as_str().and_then(|id| objects.get(id)))
            .ok_or("TaskBranch Hold lacks immutable Evidence")?;
        let context = from_evidence(evidence)?;
        let uuid = uuid(&context)?;
        let mut expected = hold(&context, &uuid);
        if let Some(resolved) = raw.get("resolved_by") {
            let passing = projection
                .objects
                .get("Evidence")
                .and_then(|objects| resolved.as_str().and_then(|id| objects.get(id)))
                .ok_or("TaskBranch resolution lacks actual new check Evidence")?;
            let next = from_evidence(passing)?;
            if next.run.id != context.run.id || next.run.leases != context.run.leases {
                return Err("TaskBranch resolution crosses Run fences".into());
            }
            expected["resolved_by"] = resolved.clone();
        }
        if raw != expected {
            return Err("TaskBranch Hold differs from typed original Evidence".into());
        }
        projection
            .objects
            .get_mut("Hold")
            .expect("selected Hold exists")
            .remove(&id);
    }
    Ok(())
}

pub(crate) fn source_unavailable(catalog: &Catalog, context: &Context) -> Option<String> {
    for (oid, expected) in [
        (&context.authority, "commit"),
        (&context.commit, "commit"),
        (&context.tree, "tree"),
    ] {
        match crate::repo::git_object_type(&catalog.root, oid) {
            Ok(Some(kind)) if kind == expected => {}
            Ok(_) => return Some(format!("原检查来源 {oid} 不再可核；仅保留历史原件")),
            Err(reason) => return Some(format!("原检查来源读取失败：{reason}；仅保留历史原件")),
        }
    }
    None
}
fn available_proof(catalog: &Catalog, context: &Context) -> Result<Option<String>, String> {
    if let Some(reason) = source_unavailable(catalog, context) {
        return Ok(Some(reason));
    }
    let original = planned(context.clone(), catalog.config.project_id())?;
    match verify_git(catalog, &decode(&original)?) {
        Ok(()) => Ok(None),
        Err(reason)
            if reason.contains("Infrastructure(")
                || reason.contains("task diagnostic source unavailable:") =>
        {
            Ok(Some(format!("原检查来源不可核：{reason}；仅保留历史原件")))
        }
        Err(reason) => Err(reason),
    }
}
pub(crate) fn decorate_view(
    catalog: &Catalog,
    projection: &StateProjection,
    leases: &BTreeMap<String, crate::hub_protocol::Lease>,
    now: u64,
    raw: &mut Value,
) -> Result<(), String> {
    if raw["scope"] != "TaskBranch" {
        return Ok(());
    }
    let evidence = projection
        .objects
        .get("Evidence")
        .and_then(|objects| raw["evidence"].as_str().and_then(|id| objects.get(id)))
        .ok_or("TaskBranch view lacks original Evidence")?;
    let context = from_evidence(evidence)?;
    let current = projection
        .objects
        .get("Run")
        .and_then(|runs| runs.get(&context.run.id));
    let reason = if raw.get("resolved_by").is_some() {
        Some("原检查已由后续真实检查取代；保留历史原件".into())
    } else if current.is_none_or(|run| {
        !matches!(
            run["state"].as_str(),
            Some("running" | "verifying" | "held")
        )
    }) || context.run.leases.iter().any(|f| {
        leases
            .get(&f.name)
            .is_none_or(|lease| lease.fence() != *f || lease.expires_at <= now)
    }) {
        Some("原检查的 Run/fences 已不再活动；保留历史原件".into())
    } else {
        available_proof(catalog, &context)?
    };
    if let Some(reason) = reason {
        raw["source_state"] = json!("historical_only");
        raw["source_reason"] = json!(reason);
    }
    Ok(())
}
pub(crate) fn known_originals(
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    inputs: &recovery_outbox::RecoveryInputs,
    run: &Run,
    now: u64,
) -> Result<BTreeMap<String, Event>, String> {
    let Some(selected) = run.extra.get("last_check").and_then(Value::as_str) else {
        return Ok(BTreeMap::new());
    };
    let history = crate::recovery_git::read_recovery_history(catalog, inputs)?;
    let tail = crate::project_session::ProjectSession::continue_shared(
        session,
        catalog,
        inputs,
        now,
        |live| {
            live.tail
                .iter()
                .filter(|event| is_event(event) && event.obj == selected)
                .cloned()
                .collect::<Vec<_>>()
        },
    )
    .map_err(|e| e.to_string())?;
    let mut known = BTreeMap::new();
    for event in inputs
        .confirmed()
        .iter()
        .chain(history.actual_outer_events(catalog)?)
        .chain(tail.iter())
        .filter(|event| is_event(event) && event.obj == selected)
    {
        if known.get(&event.eid) == Some(event) {
            continue;
        }
        let record = decode(&recovery_outbox::original_of(event))?;
        if available_proof(catalog, &record.context)?.is_some() {
            continue;
        }
        if known
            .insert(event.eid.clone(), event.clone())
            .is_some_and(|old| old != *event)
        {
            return Err("Task check original accepted receipt conflict".into());
        }
    }
    Ok(known)
}
#[allow(
    clippy::too_many_arguments,
    reason = "immutable checked source and same Session/outbox remain explicit"
)]
pub(crate) fn record_shared(
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    outbox: &mut recovery_outbox::RecoveryOutbox,
    inputs: &recovery_outbox::RecoveryInputs,
    task: &Task,
    run: &Run,
    identity: &crate::project_session::SnapshotSessionId,
    commit: &str,
    tree: &str,
    baseline: &str,
    rejection: Option<Rejection>,
    known: &BTreeMap<String, Event>,
    checked_ms: u64,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<Event, String> {
    let mut own_append = None;
    let result=crate::project_session::ProjectSession::continue_shared(session,catalog,inputs,checked_ms,|live| {
        let at=clock().map_err(crate::hub_protocol::HubError::Uncertain)?;
        if at<checked_ms||live.snapshot_session_id()!=*identity {return Err(crate::hub_protocol::HubError::Invalid("check diagnostic Session/time changed".into()));}
        let(current_task,current_run)=live.tasks.check_context(catalog,&run.host,Some(&run.id)).map_err(crate::hub_protocol::HubError::Invalid)?;
        if json!((&current_task,&current_run))!=json!((task,run))||run.leases.iter().any(|f|live.head.leases.get(&f.name).is_none_or(|lease|lease.fence()!=*f||lease.expires_at<=at)) {
            return Err(crate::hub_protocol::HubError::Invalid("check diagnostic Task/Run/full fences changed before append".into()));
        }
        let previous=run.extra.get("last_check").and_then(Value::as_str).map(str::to_owned);
        let previous_context=previous.as_ref().map(|id| {
            let value=live.projection.objects.get("Evidence").and_then(|objects|objects.get(id)).ok_or("Run last check lacks actual immutable Evidence")?;from_evidence(value)
        }).transpose().map_err(crate::hub_protocol::HubError::Invalid)?;
        let reused=match previous_context {
            Some(context) if same_subject(&context,task,run,&catalog.revision,baseline,tree,&rejection).map_err(crate::hub_protocol::HubError::Invalid)?
                && planned(context.clone(),catalog.config.project_id()).is_ok_and(|original|known.contains_key(&original.eid))=>Some(context),
            _=>None,
        };
        let context=reused.unwrap_or_else(||Context{task:task.clone(),run:run.clone(),authority:catalog.revision.clone(),baseline:baseline.into(),commit:commit.into(),tree:tree.into(),checked_ms:at,
            observed_through_seq:live.cursor,previous,rejection});
        let original=planned(context,catalog.config.project_id()).map_err(crate::hub_protocol::HubError::Invalid)?;
        // An already accepted record is durability maintenance only. Its actual
        // original coordinates are never replaced by a second append.
        if let Some(actual)=known.get(&original.eid) {
            if recovery_outbox::original_of(actual)!=original{return Err(crate::hub_protocol::HubError::Invalid("same-tree check original body conflict".into()));}
            outbox.append_pending(&original).map_err(crate::hub_protocol::HubError::Uncertain)?;
            outbox.persist_confirmed(&original,actual).map_err(crate::hub_protocol::HubError::Uncertain)?;return Ok(actual.clone());
        }
        if let Some(actual)=live.tail.iter().find(|actual|actual.eid==original.eid) {
            if recovery_outbox::original_of(actual)!=original {return Err(crate::hub_protocol::HubError::Invalid("Task check tail original body conflict".into()));}
            recovery_outbox::validate_actual(&original,actual).map_err(crate::hub_protocol::HubError::Invalid)?;
            outbox.append_pending(&original).map_err(crate::hub_protocol::HubError::Uncertain)?;
            outbox.persist_confirmed(&original,actual).map_err(crate::hub_protocol::HubError::Uncertain)?;return Ok(actual.clone());
        }
        // A pending fsync failure retains the original bytes and full guards.
        // Never regenerate its timestamp/previous context under the same eid.
        let original=inputs.pending().iter().find(|pending|pending.eid==original.eid).cloned().unwrap_or(original);
        let original_record=decode(&original).map_err(crate::hub_protocol::HubError::Invalid)?;
        if original_record.context.observed_through_seq!=live.cursor {return Err(crate::hub_protocol::HubError::Uncertain("Task check original pending prefix advanced; cannot mint a fresh current diagnostic".into()));}
        if original_record.context.rejection.is_none()
            && !live.tasks.needs_reconciliation(inputs)
            && !inputs.pending().iter().any(|pending| pending.eid == original.eid)
        {
            own_append = crate::project_session::OwnFinishAppend::capture_checked_task_passed(
                live, catalog, &original,
            ).map_err(crate::hub_protocol::HubError::Invalid)?;
        }
        match recovery_outbox::append_durable(&mut *live.hub,outbox,original,at)? {
            crate::hub_protocol::AppendReceipt::Event(actual)=>Ok(*actual),
            crate::hub_protocol::AppendReceipt::Duplicate{..}=>Err(crate::hub_protocol::HubError::Uncertain("Task check duplicate lacks actual original receipt".into())),
        }
    }).map_err(|e|e.to_string())?;
    let synchronized = clock().and_then(|now| {
        let minimum = result
            .as_ref()
            .ok()
            .map(crate::hub::event_time)
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(checked_ms)
            .max(checked_ms);
        if now < minimum {
            return Err("Task check clock moved backwards after acceptance".into());
        }
        outbox.read_inputs().and_then(|fresh| {
            if let (Some(own), Ok(actual)) = (own_append, result.as_ref()) {
                if own
                    .install(session, catalog, inputs, &fresh, actual, now)
                    .unwrap_or(false)
                {
                    return Ok(());
                }
            }
            crate::project_session::ProjectSession::sync_shared(session, catalog, &fresh, now)
                .map_err(|e| e.to_string())
        })
    });
    let actual = result.map_err(|e| e.to_string())?;
    synchronized?;
    Ok(actual)
}

pub(crate) fn validate_history(
    catalog: &Catalog,
    history: &crate::recovery_git::VerifiedRecoveryHistory,
) -> Result<(), String> {
    let mut projection = StateProjection::default();
    let mut leases = BTreeMap::new();
    for event in history.actual_outer_events(catalog)? {
        if is_event(event) {
            let record = decode(&recovery_outbox::original_of(event))?;
            if !event.stale
                && (record.context.observed_through_seq.checked_add(1) != Some(event.seq)
                    || record.context.checked_ms
                        > crate::hub::event_time(event).map_err(|e| e.to_string())?
                    || record.context.run.leases.iter().any(|f| {
                        leases
                            .get(&f.name)
                            .is_none_or(|lease: &crate::hub_protocol::Lease| {
                                lease.fence() != *f || lease.expires_at <= record.context.checked_ms
                            })
                    }))
            {
                return Err(
                    "Task check diagnostic lacks exact actual current source/fence prefix".into(),
                );
            }
        }
        if !event.stale {
            if event.kind == "hub.rebuilt" {
                leases.clear();
            }
            for lease in event.updated_leases() {
                leases.insert(lease.name.clone(), lease.clone());
            }
            for name in event.released_lease_names() {
                leases.remove(&name);
            }
        }
        projection.apply_recorded(event)?;
    }
    for run in projection
        .objects
        .get("Run")
        .into_iter()
        .flat_map(|runs| runs.values())
    {
        if !matches!(
            run["state"].as_str(),
            Some("running" | "verifying" | "held")
        ) {
            continue;
        }
        let Some(id) = run["last_check"].as_str() else {
            continue;
        };
        let value = projection
            .objects
            .get("Evidence")
            .and_then(|objects| objects.get(id))
            .ok_or("Run last check missing original Evidence")?;
        let context = from_evidence(value)?;
        // Missing Git never becomes a pass or a resolution. The App decorates
        // this original as source-unavailable/history; actual check/finish
        // always inspect the current real tree independently.
        let _ = available_proof(catalog, &context)?;
    }
    Ok(())
}
#[cfg(test)]
#[path = "task_check_recorded_tests.rs"]
mod tests;
