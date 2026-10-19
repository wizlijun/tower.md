//! Finite project controls. Text is an instruction, never a Decision or approval.
use crate::{
    ask_contract::Recipient,
    hub_protocol::{Event, NewEvent},
    human_intervention::{Hold, HoldBody, Scope},
    recovery_contract::{
        self,
        recorded::{request_event_eid, RequestBody, RequestMeta, RequestReceipt},
    },
    recovery_outbox,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AppControlAction {
    Interject {
        scope: Scope,
        text: String,
    },
    // Empty struct variants enforce deny_unknown_fields for the App wire;
    // internally tagged unit variants otherwise silently ignore extra fields.
    PauseAll {},
    ConfirmResume {
        request: String,
        pause_revision: String,
        runs: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlAction {
    Interject {
        scope: Scope,
        text: String,
    },
    PauseAll,
    ResumeAll,
    ConfirmResume {
        request: String,
        pause_revision: String,
        runs: Vec<String>,
    },
}
impl From<AppControlAction> for ControlAction {
    fn from(action: AppControlAction) -> Self {
        match action {
            AppControlAction::Interject { scope, text } => Self::Interject { scope, text },
            AppControlAction::PauseAll {} => Self::PauseAll,
            AppControlAction::ConfirmResume {
                request,
                pause_revision,
                runs,
            } => Self::ConfirmResume {
                request,
                pause_revision,
                runs,
            },
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ControlRequest {
    pub(crate) body: RequestBody<ControlAction>,
    pub(crate) meta: RequestMeta,
}
impl ControlRequest {
    /// Syntax and immutable identity only; no request value authenticates an
    /// App, proves acceptance or authorizes a current control transition.
    pub fn body(&self) -> &RequestBody<ControlAction> {
        &self.body
    }
    pub fn metadata(&self) -> &RequestMeta {
        &self.meta
    }
    /// Derive this original PauseAll request's version only. This does not
    /// prove acceptance, current pause state, or permission to resume.
    pub fn pause_revision(&self) -> Result<String, String> {
        if !matches!(&self.body.action, ControlAction::PauseAll) {
            return Err("pause revision requires an original PauseAll request".into());
        }
        revision(self)
    }
    pub fn prepare(
        project: &str,
        host: &str,
        actor: &str,
        id: &str,
        action: ControlAction,
    ) -> Result<Self, String> {
        let body = RequestBody {
            project_id: project.into(),
            host: host.into(),
            actor: actor.into(),
            run: None,
            action,
        };
        let meta = RequestMeta {
            request_id: id.into(),
            body_hash: recovery_contract::canonical_hash(&body)?,
        };
        Self::from_recorded(body, meta)
    }
    pub fn from_recorded(
        body: RequestBody<ControlAction>,
        meta: RequestMeta,
    ) -> Result<Self, String> {
        request_event_eid(&meta.request_id, "hold.opened")?;
        crate::hub::validate_token("host", &body.host).map_err(|e| e.to_string())?;
        let human = body
            .actor
            .strip_prefix("human:")
            .filter(|id| !id.is_empty() && !id.contains([':', '/']));
        if let Some(id) = human {
            crate::hub::validate_token("human", id).map_err(|e| e.to_string())?;
        }
        let process = body.actor == "process:towermd-cli";
        if body.project_id.is_empty()
            || body.run.is_some()
            || recovery_contract::canonical_hash(&body)? != meta.body_hash
        {
            return Err("control original project/run/hash mismatch".into());
        }
        match &body.action {
            ControlAction::Interject { scope, text } => {
                if human.is_none() || text.trim().is_empty() || text.len() > 1024 * 1024 {
                    return Err("interjection requires App human and nonempty bounded text".into());
                }
                match scope {
                    Scope::Project => {}
                    Scope::Module { module } => {
                        crate::hub::validate_token("module", module).map_err(|e| e.to_string())?
                    }
                    Scope::Task { task } => {
                        crate::hub::validate_token("task", task).map_err(|e| e.to_string())?
                    }
                }
            }
            ControlAction::PauseAll if human.is_some() || process => {}
            ControlAction::ResumeAll if process => {}
            ControlAction::ConfirmResume {
                request,
                pause_revision,
                runs,
            } if human.is_some() => {
                crate::hub::validate_token("resume Hold", request).map_err(|e| e.to_string())?;
                recovery_contract::sha256_hex(pause_revision)?;
                validate_runs(runs)?;
            }
            _ => {
                return Err(
                    "only App human may interject/confirm; CLI may only pause or request resume"
                        .into(),
                )
            }
        }
        Ok(Self { body, meta })
    }
}
pub(crate) fn validate_runs(runs: &[String]) -> Result<(), String> {
    if runs.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("resume scope must be sorted and unique".into());
    }
    for run in runs {
        crate::hub::validate_token("run", run).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(crate) fn input_fields(action: &ControlAction) -> (Scope, String) {
    match action {
        ControlAction::Interject { scope, text } => (scope.clone(), text.clone()),
        ControlAction::PauseAll => (Scope::Project, "暂停全部".into()),
        ControlAction::ResumeAll => (Scope::Project, "请求恢复全部".into()),
        ControlAction::ConfirmResume { .. } => (Scope::Project, "确认恢复".into()),
    }
}
pub(crate) fn revision(request: &ControlRequest) -> Result<String, String> {
    recovery_contract::canonical_hash(&(&request.meta.request_id, &request.meta.body_hash))
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlResult {
    pub hold: String,
    pub opening_eid: String,
    pub pause_revision: Option<String>,
    pub runs: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlOutcome {
    pub receipt: RequestReceipt<ControlResult>,
    pub historical_only: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlRecorded {
    pub schema: u32,
    pub hold: Hold,
    pub scope: Scope,
    pub text: String,
    pub produced_action: ControlAction,
    pub request_body: RequestBody<ControlAction>,
    pub p3_request: RequestMeta,
    pub p3_receipt: Option<RequestReceipt<ControlResult>>,
    pub prompt: Option<Event>,
    pub pause: Option<Event>,
    pub resume_request: Option<Event>,
    pub claims: Vec<Event>,
    pub source_barrier: u64,
}
pub(crate) fn is_control(kind: &str, data: &serde_json::Value) -> bool {
    matches!(kind, "human.prompt" | "hold.opened")
        && data["schema"] == 1
        && matches!(
            data["request_body"]["action"]["action"].as_str(),
            Some("interject" | "pause_all" | "resume_all" | "confirm_resume")
        )
}
pub(crate) fn decode(project: &str, original: &NewEvent) -> Result<ControlRecorded, String> {
    if !is_control(&original.kind, &original.data) {
        return Err("not a finite project control".into());
    }
    let record: ControlRecorded =
        serde_json::from_value(original.data.clone()).map_err(|e| e.to_string())?;
    let request =
        ControlRequest::from_recorded(record.request_body.clone(), record.p3_request.clone())?;
    let (scope, text) = input_fields(&request.body.action);
    if record.scope != scope || record.text != text || record.produced_action != request.body.action
    {
        return Err("human input scope/text/action differ from original request".into());
    }
    let hold = &record.hold;
    let eid = request_event_eid(&request.meta.request_id, &original.kind)?;
    let opening_eid = request_event_eid(&request.meta.request_id, "hold.opened")?;
    let input = original.kind == "human.prompt";
    let human = request.body.actor.starts_with("human:");
    if input && (!human || record.prompt.is_some() || record.p3_receipt.is_some()) {
        return Err(
            "human.prompt is only the immutable App input phase, never a Hold receipt".into(),
        );
    }
    if !input && human {
        let prompt = record
            .prompt
            .as_ref()
            .ok_or("App Hold lacks its exact accepted human.prompt input")?;
        recovery_outbox::validate_actual(&recovery_outbox::original_of(prompt), prompt)?;
        if prompt.kind != "human.prompt" || prompt.stale || prompt.legacy {
            return Err("App Hold input is not an accepted human.prompt".into());
        }
        let input_record = decode(project, &recovery_outbox::original_of(prompt))?;
        let mut expected = record.clone();
        expected.prompt = None;
        expected.p3_receipt = None;
        if input_record != expected {
            return Err("App Hold differs from frozen original human.prompt input".into());
        }
    } else if !input && record.prompt.is_some() {
        return Err("process control cannot impersonate human.prompt".into());
    }
    let recipients = &hold.recipients;
    if recipients.windows(2).any(|pair| pair[0].run >= pair[1].run) {
        return Err("control recipients must be sorted and unique".into());
    }
    for recipient in recipients {
        crate::ask_contract::validate_recipient(recipient)?;
    }
    if record.claims.len() != recipients.len() {
        return Err("control lacks complete original recipient claims".into());
    }
    for (recipient, claim) in recipients.iter().zip(&record.claims) {
        recovery_outbox::validate_actual(&recovery_outbox::original_of(claim), claim)?;
        crate::cli_delivery::validate_source_event(project, claim)?;
        let run: crate::tasks::Run =
            serde_json::from_value(claim.data["run"].clone()).map_err(|e| e.to_string())?;
        if claim.stale
            || claim.legacy
            || run.id != recipient.run
            || run.task != recipient.task
            || run.host != recipient.host
            || run.worktree != recipient.worktree
            || run.branch != recipient.branch
            || run.leases != recipient.fences
        {
            return Err("control recipient differs from exact original Task claim".into());
        }
    }
    let runs: Vec<_> = recipients.iter().map(|r| r.run.clone()).collect();
    let pause_revision = match &request.body.action {
        ControlAction::Interject { scope, text } => {
            if hold.body
                != (HoldBody::Prompt {
                    scope: scope.clone(),
                    text: text.clone(),
                })
                || record.pause.is_some()
                || record.resume_request.is_some()
            {
                return Err("interjection changed its original text/scope".into());
            }
            for recipient in recipients {
                match scope {
                    Scope::Project => {}
                    Scope::Module { module }
                        if recipient
                            .fences
                            .iter()
                            .any(|f| f.name == format!("module:{module}")) => {}
                    Scope::Task { task } if &recipient.task == task => {}
                    _ => return Err("interjection recipient outside selected scope".into()),
                }
            }
            None
        }
        ControlAction::PauseAll => {
            let revision = revision(&request)?;
            if hold.body
                != (HoldBody::Pause {
                    revision: revision.clone(),
                    active_runs: runs.clone(),
                    resumed: false,
                })
                || record.pause.is_some()
                || record.resume_request.is_some()
            {
                return Err("pause original differs from frozen scope".into());
            }
            Some(revision)
        }
        ControlAction::ResumeAll | ControlAction::ConfirmResume { .. } => {
            let pause = record.pause.as_ref().ok_or("resume lacks original Pause")?;
            recovery_outbox::validate_actual(&recovery_outbox::original_of(pause), pause)?;
            // Parse only this predecessor, which must itself be a PauseAll.
            if pause.data["request_body"]["action"]["action"] != "pause_all" {
                return Err("resume predecessor is not PauseAll".into());
            }
            let original_pause = decode(project, &recovery_outbox::original_of(pause))?;
            let HoldBody::Pause { revision, .. } = original_pause.hold.body else {
                return Err("resume original Pause body invalid".into());
            };
            if pause.stale || pause.legacy {
                return Err("resume cannot use stale/legacy pause".into());
            }
            let confirmed = matches!(&request.body.action, ControlAction::ConfirmResume { .. });
            if hold.body
                != (HoldBody::Resume {
                    pause_revision: revision.clone(),
                    active_runs: runs.clone(),
                    confirmed,
                })
            {
                return Err("resume changed frozen Pause/runs".into());
            }
            if let ControlAction::ConfirmResume {
                request: requested,
                pause_revision,
                runs: confirmed_runs,
            } = &request.body.action
            {
                let source = record
                    .resume_request
                    .as_ref()
                    .ok_or("confirmation lacks actual Resume request")?;
                recovery_outbox::validate_actual(&recovery_outbox::original_of(source), source)?;
                if source.data["request_body"]["action"]["action"] != "resume_all" {
                    return Err("confirmation source is not process ResumeAll".into());
                }
                let resume = decode(project, &recovery_outbox::original_of(source))?;
                if source.stale
                    || source.legacy
                    || &resume.hold.id != requested
                    || pause_revision != &revision
                    || confirmed_runs != &runs
                    || resume.hold.body
                        != (HoldBody::Resume {
                            pause_revision: revision.clone(),
                            active_runs: runs.clone(),
                            confirmed: false,
                        })
                    || resume.pause.as_ref() != Some(pause)
                {
                    return Err("confirmation differs from exact request/revision/run scope".into());
                }
            } else if record.resume_request.is_some() {
                return Err("new resume request cannot confirm itself".into());
            }
            Some(revision)
        }
    };
    if original.eid != eid
        || original.obj != hold.id
        || hold.id != format!("H-{}", request.meta.request_id)
        || original.layer != "L5"
        || !original.guards.is_empty()
        || original.host != request.body.host
        || original.actor != request.body.actor
        || request.body.project_id != project
        || hold.object_type != "Hold"
        || hold.opened_seq != 0
        || hold.reply.is_some()
        || !hold.waits.is_empty()
        || !hold.delivered.is_empty()
        || hold.attention
        || !hold.extra.is_empty()
        || (!input
            && record.p3_receipt
                != Some(RequestReceipt {
                    request_id: request.meta.request_id.clone(),
                    body_hash: request.meta.body_hash.clone(),
                    result_ref: ControlResult {
                        hold: hold.id.clone(),
                        opening_eid,
                        pause_revision,
                        runs,
                    },
                }))
    {
        return Err("control complete original envelope/receipt mismatch".into());
    }
    Ok(record)
}
// A project pause has no expiring guards. Rebuild revokes Run/review grants,
// but cannot silently execute a resume. Only reader-verified accepted PauseAll
// and ConfirmResume originals restore this one switch; no inbox, Run or fence
// gains current authority from these historical coordinates.
pub(crate) fn is_project_receipt(event: &Event) -> bool {
    event.kind == "request.receipt.restored"
        && event.data["original"]["type"] == "hold.opened"
        && matches!(
            event.data["original"]["data"]["request_body"]["action"]["action"].as_str(),
            Some("pause_all" | "confirm_resume")
        )
}
pub(crate) fn restore_project_switch(
    canonical: &mut crate::human_intervention::HumanProjection,
    holds: &mut std::collections::BTreeMap<String, serde_json::Value>,
    event: &Event,
) -> Result<(), String> {
    let project = event.data["original"]["data"]["request_body"]["project_id"]
        .as_str()
        .ok_or("project control receipt lacks project")?;
    let original = crate::request_ledger::restored_original(project, event)?
        .ok_or("project control receipt lacks original")?;
    let record = decode(project, &recovery_outbox::original_of(&original))?;
    if !matches!(
        record.request_body.action,
        ControlAction::PauseAll | ControlAction::ConfirmResume { .. }
    ) {
        return Err("only accepted project switch facts survive rebuild".into());
    }
    if let Some(previous) = canonical.openings.get(&original.obj) {
        if previous != &original {
            return Err("restored project control changed its original identity".into());
        }
    } else {
        let mut raw = serde_json::to_value(record.hold).map_err(|e| e.to_string())?;
        raw["opened_seq"] = serde_json::json!(original.seq);
        holds.insert(original.obj.clone(), raw);
        canonical.openings.insert(original.obj.clone(), original);
    }
    // Recovery outboxes from several hosts need not arrive in original order.
    // Derive resumed only from exact accepted confirmations, never arrival order.
    for (id, pause) in canonical
        .openings
        .iter()
        .filter(|(_, opening)| opening.data["request_body"]["action"]["action"] == "pause_all")
    {
        let resumed = canonical.openings.values().any(|confirmation| {
            confirmation.data["request_body"]["action"]["action"] == "confirm_resume"
                && confirmation.data["pause"] == serde_json::json!(pause)
        });
        let raw = holds
            .get_mut(id)
            .ok_or("restored project pause lacks Hold")?;
        raw["body"]["resumed"] = serde_json::json!(resumed);
    }
    Ok(())
}
pub(crate) fn validate_sources(
    record: &ControlRecorded,
    sequence: Option<u64>,
    history: &[&Event],
) -> Result<(), String> {
    for source in record
        .claims
        .iter()
        .chain(record.pause.iter())
        .chain(record.resume_request.iter())
        .chain(record.prompt.iter())
    {
        if sequence.is_some_and(|seq| source.seq >= seq) || !history.contains(&source) {
            return Err("control lacks its exact earlier actual source Event".into());
        }
    }
    Ok(())
}
/// Message identity uses the original Hold; delivery never changes its text.
pub(crate) fn message(hold: &Hold, recipient: &Recipient) -> Option<(String, String, String)> {
    let (text, kind) = match &hold.body {
        HoldBody::Prompt { text, .. } => (text.clone(), "prompt"),
        HoldBody::Pause { .. } => ("项目已暂停；请在下一个检查点停止写入并报告当前任务。不要终止其他 harness，等待新的明确恢复范围。".into(), "pause"),
        HoldBody::Resume { confirmed: true, .. } => ("人已确认当前范围恢复；先读取 status 核对原运行、租约和门禁，再决定继续。此消息不会自动恢复 harness 或重新领取租约。".into(), "resume"),
        _ => return None,
    };
    Some((
        format!("{}/{kind}/{}", hold.id, recipient.run),
        text,
        kind.into(),
    ))
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRun {
    pub run: String,
    pub task: String,
    pub module: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeRequestView {
    pub request: String,
    pub pause_revision: String,
    pub runs: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRecipientView {
    pub run: String,
    pub task: String,
    pub delivered: bool,
    pub actionable: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlMessageView {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub scope: Option<Scope>,
    pub recipients: Vec<ControlRecipientView>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlView {
    pub project_id: String,
    pub paused: bool,
    pub pause_revision: Option<String>,
    pub active_runs: Vec<ControlRun>,
    pub resume_request: Option<ResumeRequestView>,
    pub messages: Vec<ControlMessageView>,
}

#[cfg(test)]
#[path = "intervention_control_tests.rs"]
mod tests;
