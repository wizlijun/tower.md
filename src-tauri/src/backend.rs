//! Native-only bridge. The WebView never receives a daemon address or chooses
//! an executable; only the packaged CLI may start the per-user daemon.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Deserialize, PartialEq, Eq)]
struct Endpoint {
    address: String,
    port: u16,
    instance: String,
}

impl Endpoint {
    fn validate(&self) -> Result<(), String> {
        if self.address != "127.0.0.1" || self.port == 0 || self.instance.is_empty() {
            return Err("守护进程返回了非法的本机地址".into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Backend {
    endpoint: Option<Endpoint>,
    projects: BTreeMap<PathBuf, String>,
}

pub enum Operation {
    View,
    State,
    /// `true` starts from the current approved baseline (J4).
    Scan(bool),
    Poll(String),
    /// J4: the draft for a finished from-baseline scan.
    DraftFromScan(String),
    /// J4: a from-baseline draft against its baseline; read only.
    CompareBaseline(String),
    Cancel(String),
    Baselines,
    Baseline(String),
    Review(String, towermd_devkit::review::ReviewAction),
    VerifyBaseline(String, String),
    PollVerification(String),
    CancelVerification(String),
    PreparePublication(String),
    ApprovePublication(String, PublicationApproval),
    RequestRecovery,
    ConfirmRecovery(String),
    AnswerQuestion(AnswerRequest),
    SendPrompt(PromptRequest),
    PrepareMainResolution(towermd_core::main_resolution::Operation, String),
    ConfirmMainResolution(towermd_core::main_resolution::Preview),
    RunDetails(String),
    Proposals,
    PrepareProposal(String),
    ApproveProposal(String, PublicationApproval),
    RejectProposal(String, ProposalRejection),
    CaptureMainProposal(String),
    CreateHumanTask(HumanTaskRequest),
    TaskOptions,
    ControlProject(ProjectControlRequest),
    Delegation(DelegationAction),
    AddIdea(IdeaRequest),
    PromoteIdea(PromoteIdeaRequest),
    AddFollowUp(FollowUpRequest),
    FollowUpDone(FollowUpDoneRequest),
    AddTempRule(TempRuleRequest),
    ResurfacedSeen(ResurfacedSeenRequest),
    Templates,
    CreateFrame(FrameRequest),
    PlaceFrame(PlaceFrameRequest),
    WriteCard(CardRequest),
    ChangeFrameRules(FrameRulesRequest),
    ResolveFrameItem(FrameDecisionRequest),
    PinCards(PinCardsRequest),
    /// FR-154: the exact new `tower.canvas` for one frame, for a person to read.
    PrepareMaterialize(String),
    /// FR-154: a person approves that preview; the daemon writes it.
    ConfirmMaterialize(towermd_core::frame_canvas::Preview),
    /// FR-157: a frame's steps, or the frame right after one step.
    FrameHistory(String, Option<u64>),
    /// FR-189: a person adopts or dismisses a starter kit.
    ChooseStarter(StarterRequest),
}

/// FR-201–211: a person's action on a delegation, the ledger's rules, work
/// kinds or levels. The body is exactly the event body plus a request ID;
/// the daemon signs it `human:<id>`, so no signer or layer can be chosen here.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationRequest<T> {
    pub request_id: String,
    /// The delegated object; for rules, the rule ID.
    pub object: String,
    pub data: T,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerRequest<T> {
    pub request_id: String,
    pub data: T,
}
pub enum DelegationAction {
    Finalize(DelegationRequest<towermd_core::delegation_projection::Finalized>),
    Used(DelegationRequest<towermd_core::delegation_projection::Used>),
    Confirm(DelegationRequest<towermd_core::delegation_projection::Confirmed>),
    SendBack(DelegationRequest<towermd_core::delegation_projection::SentBack>),
    Rule(DelegationRequest<towermd_core::delegation_projection::RuleRecorded>),
    RenameWorkKind(LedgerRequest<towermd_core::delegation_projection::Renamed>),
    SetLevel(LedgerRequest<towermd_core::delegation_projection::LevelSet>),
    /// "知道了" or "静音" on one merged signal.
    DismissSignal(LedgerRequest<towermd_core::tower_api::Dismissed>),
    /// FR-116/127: ask for a report or a node question; the daemon fixes who receives it.
    Ask(LedgerRequest<AskInput>),
    /// Withdraw an open request; `object` is its request ID.
    Withdraw(DelegationRequest<towermd_core::requests::Withdrawn>),
    /// FR-72: waive one declared hard gate on `object` until a time.
    Waive(DelegationRequest<towermd_core::waivers::Granted>),
    /// Spec 12.1: decide a held task (`object` is the task): try again,
    /// rewrite the card or reassign. There is no pass.
    Interrupt(DelegationRequest<towermd_devkit::task_model::InterruptResolved>),
    /// Spec 15.5: reassign a running task (`object`); the run ends, the
    /// task goes back to ready. Nothing completes.
    Reassign(DelegationRequest<Reassign>),
    /// FR-58: a verdict on a G1/G4 review; `object` is the review ID.
    Verdict(DelegationRequest<towermd_core::reviews::Resolved>),
}
/// The run a person reassigns away from: the one they saw, so a stale click
/// cannot end a newer run.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reassign {
    pub run: String,
}
/// A person's request on any object. There is no `to`: recipients are the
/// daemon's to fix, from who declared `answers` on the object.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AskInput {
    pub target: String,
    pub ask: towermd_core::requests::Ask,
    pub text: String,
    pub due: String,
}
impl DelegationAction {
    /// The daemon route (below the project) and body for this action.
    pub fn route(&self) -> Result<(String, Value), String> {
        fn body(request_id: &str, data: &impl Serialize, extra: Option<(&str, &str)>) -> Result<Value, String> {
            validate_human_request_id(request_id)?;
            let mut body = json!(data);
            let fields = body.as_object_mut().ok_or("操作已拒绝：请求内容无效")?;
            fields.insert("request_id".into(), json!(request_id));
            if let Some((key, value)) = extra { fields.insert(key.into(), json!(value)); }
            Ok(body)
        }
        fn object<T: Serialize>(input: &DelegationRequest<T>, action: &str) -> Result<(String, Value), String> {
            validate_object_segment(&input.object)?;
            Ok((format!("/delegations/{}/{action}", input.object), body(&input.request_id, &input.data, None)?))
        }
        match self {
            Self::Finalize(input) => object(input, "finalize"),
            Self::Used(input) => object(input, "used"),
            Self::Confirm(input) => object(input, "confirm"),
            Self::SendBack(input) => object(input, "send-back"),
            Self::Waive(input) => object(input, "waive"),
            Self::Interrupt(input) => object(input, "interrupt"),
            Self::Reassign(input) => {
                validate_object_segment(&input.object)?;
                Ok((format!("/tasks/{}/release", input.object), body(&input.request_id, &input.data, None)?))
            }
            Self::Verdict(input) => {
                validate_object_segment(&input.object)?;
                Ok((format!("/reviews/{}/resolve", input.object), body(&input.request_id, &input.data, None)?))
            }
            Self::Rule(input) => {
                validate_object_segment(&input.object)?;
                Ok(("/rules".into(), body(&input.request_id, &input.data, Some(("id", &input.object)))?))
            }
            Self::RenameWorkKind(input) => Ok(("/work-kinds/rename".into(), body(&input.request_id, &input.data, None)?)),
            Self::SetLevel(input) => Ok(("/delegation-levels".into(), body(&input.request_id, &input.data, None)?)),
            Self::DismissSignal(input) => Ok(("/signals/dismiss".into(), body(&input.request_id, &input.data, None)?)),
            Self::Ask(input) => Ok(("/requests".into(), body(&input.request_id, &input.data, None)?)),
            Self::Withdraw(input) => {
                validate_object_segment(&input.object)?;
                Ok((format!("/requests/{}/withdraw", input.object), body(&input.request_id, &input.data, None)?))
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTaskRequest {
    pub request_id: String,
    pub task: towermd_devkit::task_model::TaskDraft,
}
/// FR-181/183: one sentence jotted as an idea; the daemon signs it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdeaRequest {
    pub request_id: String,
    pub text: String,
}
/// FR-183: a person promotes an idea as the project's goal (`as: project`)
/// or as a task with a draft (`as: task`). No signer can be chosen here.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromoteIdeaRequest {
    pub request_id: String,
    pub idea: String,
    #[serde(rename = "as")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<towermd_devkit::task_model::TaskDraft>,
}
impl PromoteIdeaRequest {
    /// The idea goes in the path; the body is the daemon's `{request_id, as, task?}`.
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        validate_object_segment(&self.idea)?;
        let mut body = json!({"request_id":self.request_id,"as":self.kind});
        if let Some(task) = self.task { body["task"] = json!(task); }
        Ok((format!("/ideas/{}/promote", self.idea), body))
    }
}
/// FR-134: a person's follow-up with the condition that makes it due; the
/// daemon signs it `human:<id>`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FollowUpRequest {
    pub request_id: String,
    pub text: String,
    pub when: towermd_core::follow_ups::When,
}
/// FR-69: a person's temporary rule with a scope and an expiry; the daemon
/// checks the expiry and signs it `human:<id>`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TempRuleRequest {
    pub request_id: String,
    pub text: String,
    pub scope: PromptScope,
    pub until: String,
}
/// FR-134: a person completes one follow-up.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FollowUpDoneRequest {
    pub request_id: String,
    pub follow_up: String,
}
impl FollowUpDoneRequest {
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        validate_object_segment(&self.follow_up)?;
        Ok((format!("/follow-ups/{}/done", self.follow_up), json!({"request_id":self.request_id})))
    }
}
/// FR-136: a person looked at an object that resurfaced ("看过了").
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResurfacedSeenRequest {
    pub request_id: String,
    pub object: String,
}
impl ResurfacedSeenRequest {
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        validate_object_segment(&self.object)?;
        Ok((format!("/resurfaced/{}/seen", self.object), json!({"request_id":self.request_id})))
    }
}
/// FR-142/144/153: a person places a frame from a template by name. There is
/// no definition, signer or layer here: the daemon reads the template itself
/// and signs the frame `human:<id>`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrameRequest {
    pub request_id: String,
    pub id: String,
    pub title: String,
    pub template: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_by: Option<towermd_core::frames::SetBy>,
    #[serde(default)]
    pub rules: towermd_core::frames::RulesInput,
    pub place: towermd_core::frames::Place,
    /// FR-148: free unless the person binds the frame to core objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<towermd_core::frames::Mode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<String>,
}
/// FR-145: a person moves or resizes a frame.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceFrameRequest {
    pub request_id: String,
    pub frame: String,
    pub place: towermd_core::frames::Place,
}
impl FrameRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_human_request_id(&self.request_id)?;
        towermd_core::frames::check_id(&self.id).map_err(|error| format!("操作已拒绝：{error}"))?;
        validate_object_segment(&self.template)
    }
}
impl PlaceFrameRequest {
    /// The frame goes in the path; the body is the daemon's `{request_id, place}`.
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        towermd_core::frames::check_id(&self.frame).map_err(|error| format!("操作已拒绝：{error}"))?;
        Ok((format!("/frames/{}/place", self.frame), json!({"request_id":self.request_id,"place":self.place})))
    }
}
/// FR-147: a person writes a card directly; the daemon signs it `human:<id>`
/// and the frame's rules on who may write do not bind a person.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CardRequest {
    pub request_id: String,
    pub frame: String,
    /// The card written; the frame itself when adding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub op: towermd_core::cards::Op,
}
/// FR-146: a person changes a frame's rules, `update_until` included.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrameRulesRequest {
    pub request_id: String,
    pub frame: String,
    pub rules: towermd_core::frames::Rules,
}
/// A person accepts or ignores one waiting proposal, suggestion,
/// confirmation or rule proposal.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrameDecisionRequest {
    pub request_id: String,
    pub frame: String,
    pub pending: u64,
    pub accept: bool,
}
/// FR-149: a person drags cards to fixed places in their frame, or `null`
/// to let one go back to the derived layout.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PinCardsRequest {
    pub request_id: String,
    pub frame: String,
    pub pins: std::collections::BTreeMap<String, Option<towermd_core::frames::Pin>>,
}
/// FR-189: adopting or dismissing a starter kit, once.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StarterRequest {
    pub request_id: String,
    pub kit: String,
    pub adopt: bool,
}
impl StarterRequest {
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        validate_object_segment(&self.kit)?;
        let action = if self.adopt { "adopt" } else { "dismiss" };
        Ok((format!("/starters/{}/{action}", self.kit), json!({"request_id":self.request_id})))
    }
}
impl PinCardsRequest {
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        towermd_core::frames::check_id(&self.frame).map_err(|error| format!("操作已拒绝：{error}"))?;
        for card in self.pins.keys() {
            validate_card_id(card)?;
        }
        Ok((format!("/frames/{}/pins", self.frame), json!({"request_id":self.request_id,"pins":self.pins})))
    }
}
impl CardRequest {
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        towermd_core::frames::check_id(&self.frame).map_err(|error| format!("操作已拒绝：{error}"))?;
        if let Some(target) = &self.target {
            validate_card_id(target)?;
        }
        Ok((format!("/frames/{}/cards", self.frame), json!({"request_id":self.request_id,"target":self.target,"op":self.op})))
    }
}
impl FrameRulesRequest {
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        towermd_core::frames::check_id(&self.frame).map_err(|error| format!("操作已拒绝：{error}"))?;
        Ok((format!("/frames/{}/rules", self.frame), json!({"request_id":self.request_id,"rules":self.rules})))
    }
}
impl FrameDecisionRequest {
    pub fn route(self) -> Result<(String, Value), String> {
        validate_human_request_id(&self.request_id)?;
        towermd_core::frames::check_id(&self.frame).map_err(|error| format!("操作已拒绝：{error}"))?;
        Ok((format!("/frames/{}/resolve", self.frame), json!({"request_id":self.request_id,"pending":self.pending,"accept":self.accept})))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectControlRequest {
    Pause { request_id: String },
    Resume { request_id: String, pause_event: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalRejection {
    pub request_id: String,
    pub proposal_hash: String,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerRequest {
    pub request_id: String,
    pub hold: String,
    pub text: String,
    pub remember_as: AnswerDestination,
    #[serde(default)]
    pub targets: Vec<String>,
    /// FR-70 `/assume`: the assumption the person answered under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assumption: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerDestination { Temporary, ModuleDecision }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PromptScope { Global, Module { module: String }, Task { task: String } }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptRequest {
    pub request_id: String,
    pub scope: PromptScope,
    pub text: String,
    #[serde(default)]
    pub targets: Vec<String>,
}
fn validate_message(request: &str, text: &str, targets: &[String]) -> Result<(), String> {
    validate_human_request_id(request)?;
    if text.trim().is_empty() || text.len() > 32 * 1024 || targets.len() > 128
        || targets.iter().any(|id| id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control)) {
        return Err("人工消息内容或影响对象无效".into());
    }
    Ok(())
}
impl AnswerRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_message(&self.request_id, &self.text, &self.targets)?;
        if self.hold.trim().is_empty() || self.hold.len() > 256 || self.hold.chars().any(char::is_control) {
            return Err("问题 ID 无效".into());
        }
        if let Some(assumption) = &self.assumption {
            towermd_core::question_model::check_assumption(assumption)?;
        }
        Ok(())
    }
}
impl PromptRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_message(&self.request_id, &self.text, &self.targets)?;
        let id = match &self.scope { PromptScope::Global => None, PromptScope::Module { module } => Some(module), PromptScope::Task { task } => Some(task) };
        if id.is_some_and(|id| id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control)) {
            return Err("插话作用域无效".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationApproval {
    pub request_id: String,
    pub target: String,
    pub prepared_at: String,
    pub content_hash: String,
}
impl PublicationApproval {
    pub fn validate(&self) -> Result<(), String> {
        validate_human_request_id(&self.request_id)?;
        let number: u64 = self
            .target
            .strip_prefix('B')
            .ok_or("基线编号无效")?
            .parse()
            .map_err(|_| "基线编号无效")?;
        if self.target != format!("B{number}")
            || self.prepared_at.is_empty()
            || self.prepared_at.len() > 128
            || self.prepared_at.chars().any(char::is_control)
            || self.content_hash.len() != 64
            || !self
                .content_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("批准预览元数据无效".into());
        }
        Ok(())
    }
}

pub fn validate_human_request_id(id: &str) -> Result<(), String> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| "人工确认请求 ID 非法")?;
    if parsed.get_version_num() != 7 || parsed.to_string() != id {
        return Err("人工确认请求 ID 非法".into());
    }
    Ok(())
}

impl Backend {
    fn discard_replaced_endpoint(&mut self, observed: &Option<Endpoint>, live: &Endpoint) {
        if &self.endpoint == observed && self.endpoint.as_ref() != Some(live) {
            self.endpoint = None;
            self.projects.clear();
        }
    }

    pub fn request(&mut self, root: &Path, operation: Operation) -> Result<Value, String> {
        let preview_only = matches!(&operation, Operation::PreparePublication(_) | Operation::PrepareProposal(_));
        if self.endpoint.is_none() {
            self.endpoint = Some(ensure()?);
        }
        let endpoint = self.endpoint.clone().ok_or("守护进程未连接")?;
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|e| e.to_string())?;
        let result = (|| {
            let handle = match self.projects.get(root) {
                Some(handle) => handle.clone(),
                None => {
                    let opened = send(
                        &client,
                        &endpoint,
                        Method::POST,
                        "/projects/open",
                        Some(json!({"path": root})),
                    )?;
                    let handle = opened["handle"]
                        .as_str()
                        .ok_or("项目服务缺少 handle")?
                        .to_owned();
                    uuid::Uuid::parse_str(&handle).map_err(|_| "项目服务 handle 非法")?;
                    self.projects.insert(root.to_owned(), handle.clone());
                    handle
                }
            };
            let mut body = None;
            let (method, path) = match operation {
                Operation::View => (Method::GET, format!("/projects/{handle}/view")),
                Operation::State => (Method::GET, format!("/projects/{handle}/state")),
                Operation::TaskOptions => (Method::GET, format!("/projects/{handle}/tasks/options")),
                Operation::CreateHumanTask(input) => {
                    validate_human_request_id(&input.request_id)?;
                    body = Some(json!(input));
                    (Method::POST, format!("/projects/{handle}/tasks/human"))
                }
                Operation::AddIdea(input) => {
                    validate_human_request_id(&input.request_id)?;
                    body = Some(json!(input));
                    (Method::POST, format!("/projects/{handle}/ideas"))
                }
                Operation::PromoteIdea(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::AddFollowUp(input) => {
                    validate_human_request_id(&input.request_id)?;
                    body = Some(json!(input));
                    (Method::POST, format!("/projects/{handle}/follow-ups"))
                }
                Operation::FollowUpDone(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::AddTempRule(input) => {
                    validate_human_request_id(&input.request_id)?;
                    body = Some(json!(input));
                    (Method::POST, format!("/projects/{handle}/temp-rules"))
                }
                Operation::ResurfacedSeen(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::Templates => (Method::GET, format!("/projects/{handle}/templates")),
                Operation::CreateFrame(input) => {
                    input.validate()?;
                    body = Some(json!(input));
                    (Method::POST, format!("/projects/{handle}/frames"))
                }
                Operation::PlaceFrame(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::WriteCard(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::ChangeFrameRules(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::ResolveFrameItem(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::PinCards(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::PrepareMaterialize(frame) => {
                    towermd_core::frames::check_id(&frame).map_err(|error| format!("操作已拒绝：{error}"))?;
                    body = Some(json!({}));
                    (Method::POST, format!("/projects/{handle}/frames/{frame}/materialize/prepare"))
                }
                Operation::ConfirmMaterialize(preview) => {
                    validate_human_request_id(&preview.request_id)?;
                    towermd_core::frames::check_id(&preview.frame).map_err(|error| format!("操作已拒绝：{error}"))?;
                    if preview.hash()? != preview.content_hash {
                        return Err("操作已拒绝：容器落盘预览摘要不一致".into());
                    }
                    let frame = preview.frame.clone();
                    body = Some(json!({"preview":preview}));
                    (Method::POST, format!("/projects/{handle}/frames/{frame}/materialize/confirm"))
                }
                Operation::FrameHistory(frame, seq) => {
                    towermd_core::frames::check_id(&frame).map_err(|error| format!("操作已拒绝：{error}"))?;
                    match seq {
                        Some(seq) => (Method::GET, format!("/projects/{handle}/frames/{frame}/history/{seq}")),
                        None => (Method::GET, format!("/projects/{handle}/frames/{frame}/history")),
                    }
                }
                Operation::ChooseStarter(input) => {
                    let (route, input) = input.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::ControlProject(input) => {
                    let action = match input {
                        ProjectControlRequest::Pause { request_id } => {
                            validate_human_request_id(&request_id)?;
                            body = Some(json!({"request_id":request_id,"scope":"all"}));
                            "pause"
                        }
                        ProjectControlRequest::Resume { request_id, pause_event } => {
                            validate_human_request_id(&request_id)?;
                            validate_human_request_id(&pause_event)?;
                            body = Some(json!({"request_id":request_id,"pause_event":pause_event,"scope":"all"}));
                            "resume"
                        }
                    };
                    (Method::POST, format!("/projects/{handle}/{action}"))
                }
                Operation::Delegation(action) => {
                    let (route, input) = action.route()?;
                    body = Some(input);
                    (Method::POST, format!("/projects/{handle}{route}"))
                }
                Operation::Proposals => (Method::GET, format!("/projects/{handle}/proposals")),
                Operation::CaptureMainProposal(commit) => {
                    validate_commit(&commit)?;
                    body = Some(json!({"commit":commit}));
                    (Method::POST, format!("/projects/{handle}/main-proposals"))
                }
                Operation::PrepareProposal(id) => {
                    towermd_devkit::proposal_model::valid_id(&id)?;
                    body = Some(json!({}));
                    (Method::POST, format!("/projects/{handle}/proposals/{id}/publication/prepare"))
                }
                Operation::ApproveProposal(id, approval) => {
                    towermd_devkit::proposal_model::valid_id(&id)?;
                    approval.validate()?;
                    body = Some(json!(approval));
                    (Method::POST, format!("/projects/{handle}/proposals/{id}/publication/approve"))
                }
                Operation::RejectProposal(id, input) => {
                    towermd_devkit::proposal_model::valid_id(&id)?;
                    validate_human_request_id(&input.request_id)?;
                    if input.reason.trim().is_empty() || input.reason.len() > 32 * 1024
                        || input.proposal_hash.len() != 64
                        || !input.proposal_hash.bytes().all(|byte|byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    { return Err("操作已拒绝：驳回须包含原提案摘要和理由".into()); }
                    body = Some(json!(input));
                    (Method::POST, format!("/projects/{handle}/proposals/{id}/reject"))
                }
                Operation::RunDetails(id) => {
                    validate_object_segment(&id)?;
                    (Method::GET, format!("/projects/{handle}/runs/{id}/events"))
                }
                Operation::PrepareMainResolution(operation, commit) => {
                    validate_commit(&commit)?;
                    body = Some(json!({"operation":operation,"commit":commit}));
                    (Method::POST, format!("/projects/{handle}/main-resolutions/prepare"))
                }
                Operation::ConfirmMainResolution(preview) => {
                    validate_human_request_id(&preview.request_id)?;
                    validate_commit(&preview.target_commit)?;
                    if preview.hash()? != preview.content_hash {
                        return Err("操作已拒绝：主干处理预览摘要不一致".into());
                    }
                    body = Some(json!({"preview":preview}));
                    (Method::POST, format!("/projects/{handle}/main-resolutions/confirm"))
                }
                Operation::AnswerQuestion(input) => {
                    input.validate().map_err(|error| format!("操作已拒绝：{error}"))?;
                    body = Some(json!(input));
                    (Method::POST, format!("/projects/{handle}/answers"))
                }
                Operation::SendPrompt(input) => {
                    input.validate().map_err(|error| format!("操作已拒绝：{error}"))?;
                    body = Some(json!(input));
                    (Method::POST, format!("/projects/{handle}/prompts"))
                }
                Operation::PreparePublication(id) => {
                    validate_baseline_id(&id)?;
                    body = Some(json!({}));
                    (
                        Method::POST,
                        format!("/projects/{handle}/baselines/{id}/publication/prepare"),
                    )
                }
                Operation::ApprovePublication(id, approval) => {
                    validate_baseline_id(&id)?;
                    approval.validate()?;
                    body = Some(json!(approval));
                    (
                        Method::POST,
                        format!("/projects/{handle}/baselines/{id}/publication/approve"),
                    )
                }
                Operation::RequestRecovery => {
                    body = Some(json!({}));
                    (Method::POST, format!("/projects/{handle}/hub/rebuild"))
                }
                Operation::ConfirmRecovery(id) => {
                    validate_human_request_id(&id)?;
                    body = Some(json!({}));
                    (
                        Method::POST,
                        format!("/projects/{handle}/hub/rebuild/{id}/confirm"),
                    )
                }
                Operation::Scan(from_baseline) => {
                    if from_baseline {
                        body = Some(json!({"from_baseline": true}));
                    }
                    (Method::POST, format!("/projects/{handle}/scans"))
                }
                Operation::DraftFromScan(id) => {
                    uuid::Uuid::parse_str(&id).map_err(|_| "扫描 ID 非法")?;
                    body = Some(json!({}));
                    (Method::POST, format!("/projects/{handle}/scans/{id}/draft"))
                }
                Operation::CompareBaseline(id) => {
                    validate_baseline_id(&id)?;
                    (Method::GET, format!("/projects/{handle}/baselines/{id}/compare"))
                }
                Operation::Poll(id) => {
                    uuid::Uuid::parse_str(&id).map_err(|_| "扫描 ID 非法")?;
                    (Method::GET, format!("/projects/{handle}/scans/{id}"))
                }
                Operation::Cancel(id) => {
                    uuid::Uuid::parse_str(&id).map_err(|_| "扫描 ID 非法")?;
                    (
                        Method::POST,
                        format!("/projects/{handle}/scans/{id}/cancel"),
                    )
                }
                Operation::Baselines => (Method::GET, format!("/projects/{handle}/baselines")),
                Operation::Review(id, action) => {
                    validate_baseline_id(&id)?;
                    body = Some(json!({"action": action}));
                    (
                        Method::POST,
                        format!("/projects/{handle}/baselines/{id}/review"),
                    )
                }
                Operation::VerifyBaseline(id, request_id) => {
                    validate_baseline_id(&id)?;
                    validate_verification_id(&request_id)?;
                    body = Some(json!({"request_id": request_id}));
                    (
                        Method::POST,
                        format!("/projects/{handle}/baselines/{id}/verify"),
                    )
                }
                Operation::PollVerification(id) => {
                    validate_verification_id(&id)?;
                    (
                        Method::GET,
                        format!("/projects/{handle}/baseline-verifications/{id}"),
                    )
                }
                Operation::CancelVerification(id) => {
                    validate_verification_id(&id)?;
                    (
                        Method::POST,
                        format!("/projects/{handle}/baseline-verifications/{id}/cancel"),
                    )
                }
                Operation::Baseline(id) => {
                    validate_baseline_id(&id)?;
                    (Method::GET, format!("/projects/{handle}/baselines/{id}"))
                }
            };
            let mut value = send(&client, &endpoint, method, &path, body)?;
            if preview_only {
                if let Some(object) = value.as_object_mut() {
                    object.remove("files");
                }
            }
            Ok(value)
        })();
        if result.is_err() {
            // Retry is explicit in the App. The next attempt rechecks the
            // daemon instance and reopens this project's canonical path.
            self.endpoint = None;
            self.projects.clear();
        }
        result
    }
}

fn validate_commit(commit: &str) -> Result<(), String> {
    if !matches!(commit.len(), 40 | 64)
        || !commit.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("操作已拒绝：提交必须是完整 Git 对象 ID".into());
    }
    Ok(())
}

/// Cards are named `<object>` when bound or `<frame>.<n>` when free; the id
/// travels in the body, but stays one dotted name with no empty part.
fn validate_card_id(id: &str) -> Result<(), String> {
    id.split('.').try_for_each(validate_object_segment)
}

fn validate_object_segment(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128
        || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("操作已拒绝：对象 ID 无效".into());
    }
    Ok(())
}

pub fn validate_baseline_id(id: &str) -> Result<(), String> {
    let digest = id.strip_prefix("d-").ok_or("草稿 ID 非法")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("草稿 ID 非法".into());
    }
    Ok(())
}

pub fn validate_verification_id(id: &str) -> Result<(), String> {
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| "验证请求 ID 非法")?;
    if !matches!(uuid.get_version_num(), 4 | 7) || uuid.to_string() != id {
        return Err("验证请求 ID 非法".into());
    }
    Ok(())
}

fn send(
    client: &Client,
    endpoint: &Endpoint,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let mut request = client
        .request(method, format!("http://127.0.0.1:{}{path}", endpoint.port))
        .header("x-towermd-instance", &endpoint.instance)
        .header("x-towermd-client", "app");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .map_err(|e| format!("无法连接本机服务，请重试：{e}"))?;
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("项目读取结果超过 16 MiB，请缩小扫描范围".into());
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("本机服务返回了无法解析的响应（{status}）：{e}"))?;
    if !status.is_success() {
        let error = value["error"].as_str().unwrap_or("本机服务请求失败");
        return Err(
            if (path.ends_with("/review") || path.ends_with("/verify") || path.ends_with("/answers") || path.ends_with("/prompts")) && status.is_client_error()
            {
                format!("操作已拒绝：{error}")
            } else {
                error.into()
            },
        );
    }
    Ok(value)
}

fn executable() -> Result<PathBuf, String> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let path = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "binaries/towermd-{}{suffix}",
            env!("TOWERMD_TARGET")
        ))
    } else {
        std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("App 没有可执行目录")?
            .join(format!("towermd{suffix}"))
    };
    if !path.is_file() {
        return Err("缺少随 App 打包的 towermd，请重新运行 pnpm app:build 或 pnpm app:dev".into());
    }
    Ok(path)
}

/// `towermd --json daemon ensure` prints one C0 envelope (v1).
fn parse_ensure_output(stdout: &[u8]) -> Result<Endpoint, String> {
    let envelope: Value = serde_json::from_slice(stdout).map_err(|e| format!("无法读取守护进程地址：{e}"))?;
    if envelope["v"] != 1 {
        return Err("命令行输出版本不受支持，请重新构建 App".into());
    }
    if envelope["ok"] != true {
        return Err(envelope["error"]["message"].as_str().unwrap_or("守护进程启动失败").to_owned());
    }
    let endpoint: Endpoint =
        serde_json::from_value(envelope["data"].clone()).map_err(|e| format!("无法读取守护进程地址：{e}"))?;
    endpoint.validate()?;
    Ok(endpoint)
}

fn ensure() -> Result<Endpoint, String> {
    let mut child = Command::new(executable()?)
        .args(["--json", "daemon", "ensure"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动打包的命令行：{e}"))?;
    let mut stdout = child.stdout.take().ok_or("缺少 CLI 输出")?;
    let mut stderr = child.stderr.take().ok_or("缺少 CLI 错误输出")?;
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            other => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match other {
                    Err(error) => error.to_string(),
                    _ => "守护进程启动超时，请检查 daemon 日志".into(),
                });
            }
        }
    };
    let stdout = out
        .join()
        .map_err(|_| "读取 CLI 输出失败")?
        .map_err(|e| e.to_string())?;
    let stderr = err
        .join()
        .map_err(|_| "读取 CLI 错误失败")?
        .map_err(|e| e.to_string())?;
    if !status?.success() {
        let reported = serde_json::from_slice::<Value>(&stdout)
            .ok()
            .and_then(|envelope| envelope["error"]["message"].as_str().map(str::to_owned));
        return Err(reported.unwrap_or_else(|| String::from_utf8_lossy(&stderr).trim().to_string()));
    }
    parse_ensure_output(&stdout)
}

// A stream always re-discovers the live daemon and reopens its selected root.
// Network operations never hold the ordinary Backend mutex.
pub(crate) struct StreamEndpoint {
    pub port: u16,
    pub instance: String,
    pub handle: String,
}

pub(crate) fn stream_endpoint(root: &Path, backend: &crate::BackendState) -> Result<StreamEndpoint, String> {
    // An in-flight human request may hold this mutex for its HTTP call. Never
    // wait for it, and never retry or replace that request from the stream.
    let observed = backend.try_lock().ok().map(|backend| backend.endpoint.clone());
    let endpoint = ensure()?;
    let client = Client::builder().no_proxy().timeout(Duration::from_secs(15))
        .build().map_err(|error| error.to_string())?;
    let opened = send(&client, &endpoint, Method::POST, "/projects/open", Some(json!({"path": root})))?;
    let handle = opened["handle"].as_str().ok_or("项目服务缺少 handle")?.to_owned();
    uuid::Uuid::parse_str(&handle).map_err(|_| "项目服务 handle 非法")?;
    if let Some(observed) = observed {
        if let Ok(mut backend) = backend.try_lock() {
            backend.discard_replaced_endpoint(&observed, &endpoint);
        }
    }
    Ok(StreamEndpoint { port: endpoint.port, instance: endpoint.instance, handle })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cached_endpoint(instance: &str) -> Endpoint {
        Endpoint { address: "127.0.0.1".into(), port: 12345, instance: instance.into() }
    }
    #[test]
    fn stream_reconnect_discards_matching_old_http_endpoint_and_handles() {
        let old = cached_endpoint("old-daemon");
        let mut backend = Backend { endpoint: Some(old.clone()),
            projects: BTreeMap::from([(PathBuf::from("/project"), "old-handle".into())]) };
        backend.discard_replaced_endpoint(&Some(old), &cached_endpoint("restarted-daemon"));
        assert!(backend.endpoint.is_none()); assert!(backend.projects.is_empty());
    }
    #[test]
    fn stream_reconnect_cannot_clear_a_newer_http_cache_or_unchanged_live_endpoint() {
        let old = cached_endpoint("old-daemon"); let current = cached_endpoint("current-daemon");
        let mut backend = Backend { endpoint: Some(current.clone()),
            projects: BTreeMap::from([(PathBuf::from("/project"), "current-handle".into())]) };
        backend.discard_replaced_endpoint(&Some(old), &cached_endpoint("stream-discovered-daemon"));
        assert!(backend.endpoint.as_ref() == Some(&current)); assert_eq!(backend.projects.len(), 1);
        backend.discard_replaced_endpoint(&Some(current.clone()), &current);
        assert!(backend.endpoint.as_ref() == Some(&current)); assert_eq!(backend.projects.len(), 1);
        backend.discard_replaced_endpoint(&None, &cached_endpoint("stream-discovered-daemon"));
        assert!(backend.endpoint.as_ref() == Some(&current)); assert_eq!(backend.projects.len(), 1);
    }
    #[test]
    fn task_and_control_requests_cannot_choose_actor_or_derived_state() {
        let task = json!({"request_id":uuid::Uuid::now_v7().to_string(),
            "task":{"id":"T-test","title":"Task","module":"A","allowed_paths":["src/a.ts"]}});
        serde_json::from_value::<HumanTaskRequest>(task.clone()).unwrap();
        let mut brief = task.clone();
        brief["task"]["why"] = json!("users need it"); brief["task"]["limits"] = json!("no new dependencies");
        brief["task"]["checkpoints"] = json!([{"text":"plan","due":"2026-10-23T09:00:00Z"},{"text":"stop and show"}]);
        brief["task"]["self_check"] = json!("run the tests"); brief["task"]["clarify"] = json!(true);
        let draft = serde_json::from_value::<HumanTaskRequest>(brief).unwrap().task;
        assert!(draft.clarify && draft.brief().checkpoints[0].due.is_some());
        let resume = json!({"kind":"resume","request_id":uuid::Uuid::now_v7().to_string(),
            "pause_event":uuid::Uuid::now_v7().to_string()});
        serde_json::from_value::<ProjectControlRequest>(resume.clone()).unwrap();
        for key in ["actor","guards","origin","state"] {
            let mut invalid=task.clone(); invalid[key]=json!("forged");
            assert!(serde_json::from_value::<HumanTaskRequest>(invalid).is_err());
            let mut invalid=resume.clone(); invalid[key]=json!("forged");
            assert!(serde_json::from_value::<ProjectControlRequest>(invalid).is_err());
        }
        let mut invalid=task; invalid["task"]["state"]=json!("merged");
        assert!(serde_json::from_value::<HumanTaskRequest>(invalid).is_err());
        let mut invalid=resume; invalid.as_object_mut().unwrap().remove("pause_event");
        assert!(serde_json::from_value::<ProjectControlRequest>(invalid).is_err());
    }
    #[test]
    fn delegation_actions_route_to_app_only_endpoints_without_signer_fields() {
        use towermd_core::delegation_projection as events;
        let id = uuid::Uuid::now_v7().to_string();
        let request = |object: &str, data: Value| json!({"request_id":id,"object":object,"data":data});
        let ledger = |data: Value| json!({"request_id":id,"data":data});
        let cases = [
            (DelegationAction::Finalize(serde_json::from_value(request("T-1", json!({"brief":{"done_when":["passes"]},"answers":["JSON"]}))).unwrap()),
                "/delegations/T-1/finalize", json!({"request_id":id,"brief":{"done_when":["passes"]},"answers":["JSON"]})),
            (DelegationAction::Used(serde_json::from_value(request("T-1", json!({"criteria":[0]}))).unwrap()),
                "/delegations/T-1/used", json!({"request_id":id,"criteria":[0]})),
            (DelegationAction::Confirm(serde_json::from_value(request("T-1", json!({"review_ms":5,"unproven":[1],"takeaway":"ok"}))).unwrap()),
                "/delegations/T-1/confirm", json!({"request_id":id,"review_ms":5,"unproven":[1],"takeaway":"ok"})),
            (DelegationAction::SendBack(serde_json::from_value(request("T-1", json!({"reason":"empty input","review_ms":5}))).unwrap()),
                "/delegations/T-1/send-back", json!({"request_id":id,"reason":"empty input","review_ms":5})),
            (DelegationAction::Rule(serde_json::from_value(request("rule-1", json!({"kind":"docs","text":"cite"}))).unwrap()),
                "/rules", json!({"request_id":id,"id":"rule-1","kind":"docs","text":"cite"})),
            (DelegationAction::RenameWorkKind(serde_json::from_value(ledger(json!({"from":"a","to":"b"}))).unwrap()),
                "/work-kinds/rename", json!({"request_id":id,"from":"a","to":"b"})),
            (DelegationAction::SetLevel(serde_json::from_value(ledger(json!({"config":"*","kind":"docs","level":"auto"}))).unwrap()),
                "/delegation-levels", json!({"request_id":id,"config":"*","kind":"docs","level":"auto"})),
            (DelegationAction::DismissSignal(serde_json::from_value(ledger(json!({"from":"ci/nightly","kind":"eng.crap","target":"T-1","mode":"ack","seen":"e1"}))).unwrap()),
                "/signals/dismiss", json!({"request_id":id,"from":"ci/nightly","kind":"eng.crap","target":"T-1","mode":"ack","seen":"e1"})),
            (DelegationAction::Ask(serde_json::from_value(ledger(json!({"target":"T-1","ask":"question","text":"why?","due":"2026-10-24T00:00:00Z"}))).unwrap()),
                "/requests", json!({"request_id":id,"target":"T-1","ask":"question","text":"why?","due":"2026-10-24T00:00:00Z"})),
            (DelegationAction::Withdraw(serde_json::from_value(request("q-1", json!({}))).unwrap()),
                "/requests/q-1/withdraw", json!({"request_id":id})),
            (DelegationAction::Waive(serde_json::from_value(request("T-1", json!({"gate":"crap","reason":"上游修复前","until":"2026-10-31T00:00:00Z"}))).unwrap()),
                "/delegations/T-1/waive", json!({"request_id":id,"gate":"crap","reason":"上游修复前","until":"2026-10-31T00:00:00Z"})),
            (DelegationAction::Interrupt(serde_json::from_value(request("T-1", json!({"choice":"rewrite","interrupt":"e-1"}))).unwrap()),
                "/delegations/T-1/interrupt", json!({"request_id":id,"choice":"rewrite","interrupt":"e-1"})),
            (DelegationAction::Reassign(serde_json::from_value(request("T-1", json!({"run":"R-1"}))).unwrap()),
                "/tasks/T-1/release", json!({"request_id":id,"run":"R-1"})),
            (DelegationAction::Verdict(serde_json::from_value(request("V-1", json!({"verdict":"changes_requested","comment":"拆小"}))).unwrap()),
                "/reviews/V-1/resolve", json!({"request_id":id,"verdict":"changes_requested","comment":"拆小"})),
        ];
        for (action, route, body) in cases {
            assert_eq!(action.route().unwrap(), (route.to_owned(), body));
        }
        for key in ["actor", "layer", "guards"] {
            let mut forged = request("T-1", json!({"reason":"x","review_ms":1})); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<DelegationRequest<events::SentBack>>(forged).is_err(), "{key}");
            let mut forged = request("T-1", json!({"reason":"x","review_ms":1})); forged["data"][key] = json!("human:other");
            assert!(serde_json::from_value::<DelegationRequest<events::SentBack>>(forged).is_err(), "data.{key}");
            let mut forged = ledger(json!({"from":"a","to":"b"})); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<LedgerRequest<events::Renamed>>(forged).is_err(), "{key}");
            let mut forged = ledger(json!({"from":"x/1","kind":"eng.crap","target":"T-1","mode":"ack","seen":"e1"})); forged["data"][key] = json!("human:other");
            assert!(serde_json::from_value::<LedgerRequest<towermd_core::tower_api::Dismissed>>(forged).is_err(), "data.{key}");
        }
        let mut addressed = ledger(json!({"target":"T-1","ask":"report","text":"x","due":"2026-10-24T00:00:00Z"}));
        addressed["data"]["to"] = json!(["someone/1"]);
        assert!(serde_json::from_value::<LedgerRequest<AskInput>>(addressed).is_err(), "the App cannot address a request");
        assert!(DelegationAction::Withdraw(serde_json::from_value(request("../q", json!({}))).unwrap()).route().is_err());
        assert!(DelegationAction::Verdict(serde_json::from_value(request("../V-1", json!({"verdict":"passed"}))).unwrap()).route().is_err());
        for data in [json!({"choice":"pass","interrupt":"e-1"}), json!({"choice":"retry","interrupt":"e-1","actor":"human:other"})] {
            assert!(serde_json::from_value::<DelegationRequest<towermd_devkit::task_model::InterruptResolved>>(request("T-1", data)).is_err());
        }
        assert!(serde_json::from_value::<DelegationRequest<towermd_core::reviews::Resolved>>(request("V-1", json!({"verdict":"timeout"}))).is_err());
        let bad_object = DelegationAction::Used(serde_json::from_value(request("../T-1", json!({"criteria":[0]}))).unwrap());
        assert!(bad_object.route().is_err());
        let mut wrong_id = request("T-1", json!({"criteria":[0]})); wrong_id["request_id"] = json!("018fbd42-9959-4ae7-891b-7adb3eac1111");
        assert!(DelegationAction::Used(serde_json::from_value(wrong_id).unwrap()).route().is_err());
    }
    #[test]
    fn idea_requests_carry_no_signer_and_promotion_routes_by_idea() {
        let id = uuid::Uuid::now_v7().to_string();
        let idea = json!({"request_id":id,"text":"做一个周报工具"});
        serde_json::from_value::<IdeaRequest>(idea.clone()).unwrap();
        let promote = json!({"request_id":id,"idea":"idea-1","as":"project"});
        let (route, body) = serde_json::from_value::<PromoteIdeaRequest>(promote.clone()).unwrap().route().unwrap();
        assert_eq!((route.as_str(), body), ("/ideas/idea-1/promote", json!({"request_id":id,"as":"project"})));
        let as_task = json!({"request_id":id,"idea":"idea-1","as":"task",
            "task":{"id":"T-1","title":"周报","module":"A","allowed_paths":["src"]}});
        let (_, body) = serde_json::from_value::<PromoteIdeaRequest>(as_task).unwrap().route().unwrap();
        assert_eq!((body["as"].as_str(), body["task"]["id"].as_str()), (Some("task"), Some("T-1")));
        for key in ["actor","layer","guards","origin"] {
            let mut forged = idea.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<IdeaRequest>(forged).is_err());
            let mut forged = promote.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<PromoteIdeaRequest>(forged).is_err());
        }
        let mut escape = promote.clone(); escape["idea"] = json!("../idea-1");
        assert!(serde_json::from_value::<PromoteIdeaRequest>(escape).unwrap().route().is_err());
        let mut stale = promote; stale["request_id"] = json!("018fbd42-9959-4ae7-891b-7adb3eac1111");
        assert!(serde_json::from_value::<PromoteIdeaRequest>(stale).unwrap().route().is_err());
    }
    #[test]
    fn follow_up_requests_carry_no_signer_and_route_by_follow_up_or_object() {
        let id = uuid::Uuid::now_v7().to_string();
        let add = json!({"request_id":id,"text":"回头看看","when":{"on":"signal","target":"T-1","kind":"eng.crap"}});
        let parsed = serde_json::from_value::<FollowUpRequest>(add.clone()).unwrap();
        assert_eq!(json!(parsed), add, "the daemon receives the body unchanged");
        assert!(serde_json::from_value::<FollowUpRequest>(json!({"request_id":id,"text":"x","when":{"on":"pin"}})).is_err());
        let done = json!({"request_id":id,"follow_up":"f-1"});
        let (route, body) = serde_json::from_value::<FollowUpDoneRequest>(done.clone()).unwrap().route().unwrap();
        assert_eq!((route.as_str(), body), ("/follow-ups/f-1/done", json!({"request_id":id})));
        let seen = json!({"request_id":id,"object":"T-1"});
        let (route, body) = serde_json::from_value::<ResurfacedSeenRequest>(seen.clone()).unwrap().route().unwrap();
        assert_eq!((route.as_str(), body), ("/resurfaced/T-1/seen", json!({"request_id":id})));
        for key in ["actor","layer","guards"] {
            for value in [&add, &done, &seen] {
                let mut forged = value.clone(); forged[key] = json!("human:other");
                assert!(serde_json::from_value::<FollowUpRequest>(forged.clone()).is_err()
                    && serde_json::from_value::<FollowUpDoneRequest>(forged.clone()).is_err()
                    && serde_json::from_value::<ResurfacedSeenRequest>(forged).is_err(), "{key}");
            }
        }
        let mut escape = done; escape["follow_up"] = json!("../f-1");
        assert!(serde_json::from_value::<FollowUpDoneRequest>(escape).unwrap().route().is_err());
        let mut escape = seen.clone(); escape["object"] = json!("../T-1");
        assert!(serde_json::from_value::<ResurfacedSeenRequest>(escape).unwrap().route().is_err());
        let mut stale = seen; stale["request_id"] = json!("018fbd42-9959-4ae7-891b-7adb3eac1111");
        assert!(serde_json::from_value::<ResurfacedSeenRequest>(stale).unwrap().route().is_err());
    }
    #[test]
    fn card_rule_and_decision_requests_carry_no_signer_and_stay_in_their_frame() {
        let id = uuid::Uuid::now_v7().to_string();
        let card = json!({"request_id":id,"frame":"C-board","op":{"op":"add","title":"读论文"}});
        let (route, body) = serde_json::from_value::<CardRequest>(card.clone()).unwrap().route().unwrap();
        assert_eq!((route.as_str(), body), ("/frames/C-board/cards", json!({"request_id":id,"target":null,"op":{"op":"add","title":"读论文"}})));
        let decision = json!({"request_id":id,"frame":"C-board","pending":7,"accept":false});
        let (route, body) = serde_json::from_value::<FrameDecisionRequest>(decision.clone()).unwrap().route().unwrap();
        assert_eq!((route.as_str(), body["accept"].clone()), ("/frames/C-board/resolve", json!(false)));
        for key in ["actor", "layer", "guards", "from"] {
            let mut forged = card.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<CardRequest>(forged).is_err(), "{key}");
            let mut forged = decision.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<FrameDecisionRequest>(forged).is_err(), "{key}");
        }
        let mut sneaky = card.clone(); sneaky["op"]["actor"] = json!("human:other");
        assert!(serde_json::from_value::<CardRequest>(sneaky).is_err());
        let mut free = card.clone(); free["target"] = json!("C-board.1"); free["op"] = json!({"op":"archive"});
        assert_eq!(serde_json::from_value::<CardRequest>(free).unwrap().route().unwrap().1["target"], json!("C-board.1"));
        for bad in ["../C-board.1", "C-board..1", ".C-board", "C-board.1/x"] {
            let mut escape = card.clone(); escape["target"] = json!(bad);
            assert!(serde_json::from_value::<CardRequest>(escape).unwrap().route().is_err(), "{bad}");
        }
        let mut escape = decision; escape["frame"] = json!("../C-board");
        assert!(serde_json::from_value::<FrameDecisionRequest>(escape).unwrap().route().is_err());
        let rules = json!({"request_id":id,"frame":"C-board","rules":{"text":"","add_card":"anyone"}});
        assert!(serde_json::from_value::<FrameRulesRequest>(rules).is_err(), "rules are whole, never partial");
    }
    #[test]
    fn pin_requests_carry_only_places_and_stay_in_their_frame() {
        let id = uuid::Uuid::now_v7().to_string();
        let pins = json!({"request_id":id,"frame":"C-board","pins":{"C-board.1":{"x":24,"y":80},"T-1":null}});
        let (route, body) = serde_json::from_value::<PinCardsRequest>(pins.clone()).unwrap().route().unwrap();
        assert_eq!((route.as_str(), body), ("/frames/C-board/pins", json!({"request_id":id,"pins":{"C-board.1":{"x":24,"y":80},"T-1":null}})));
        for key in ["actor", "layer", "guards"] {
            let mut forged = pins.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<PinCardsRequest>(forged).is_err(), "{key}");
        }
        let mut extra = pins.clone(); extra["pins"]["C-board.1"]["z"] = json!(1);
        assert!(serde_json::from_value::<PinCardsRequest>(extra).is_err());
        let mut escape = pins.clone(); escape["pins"] = json!({"../T-1":null});
        assert!(serde_json::from_value::<PinCardsRequest>(escape).unwrap().route().is_err());
        let mut escape = pins; escape["frame"] = json!("../C-board");
        assert!(serde_json::from_value::<PinCardsRequest>(escape).unwrap().route().is_err());
    }
    #[test]
    fn temp_rule_requests_carry_a_scope_and_expiry_but_no_signer() {
        let id = uuid::Uuid::now_v7().to_string();
        let rule = json!({"request_id":id,"text":"今晚不要改依赖版本","scope":{"kind":"module","module":"A"},"until":"2026-10-25T06:00:00+08:00"});
        assert_eq!(json!(serde_json::from_value::<TempRuleRequest>(rule.clone()).unwrap()), rule, "the daemon receives the body unchanged");
        for key in ["actor", "layer", "guards", "expired"] {
            let mut forged = rule.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<TempRuleRequest>(forged).is_err(), "{key}");
        }
        let mut missing = rule; missing.as_object_mut().unwrap().remove("until");
        assert!(serde_json::from_value::<TempRuleRequest>(missing).is_err(), "a rule always carries an expiry");
    }
    #[test]
    fn a_starter_choice_names_only_the_kit_and_whether_to_adopt_it() {
        let id = uuid::Uuid::now_v7().to_string();
        let adopt = json!({"request_id":id,"kit":"research","adopt":true});
        let (route, body) = serde_json::from_value::<StarterRequest>(adopt.clone()).unwrap().route().unwrap();
        assert_eq!((route.as_str(), body), ("/starters/research/adopt", json!({"request_id":id})));
        let mut dismiss = adopt.clone(); dismiss["adopt"] = json!(false);
        assert_eq!(serde_json::from_value::<StarterRequest>(dismiss).unwrap().route().unwrap().0, "/starters/research/dismiss");
        let mut forged = adopt.clone(); forged["actor"] = json!("human:other");
        assert!(serde_json::from_value::<StarterRequest>(forged).is_err());
        let mut escape = adopt; escape["kit"] = json!("../tasks");
        assert!(serde_json::from_value::<StarterRequest>(escape).unwrap().route().is_err());
    }
    #[test]
    fn frame_requests_name_a_template_and_carry_no_definition_or_signer() {
        let id = uuid::Uuid::now_v7().to_string();
        let place = json!({"x":0,"y":0,"width":880,"height":420});
        let frame = json!({"request_id":id,"id":"C-reading","title":"读论文","template":"kanban","rules":{"add_card":"human"},"place":place});
        serde_json::from_value::<FrameRequest>(frame.clone()).unwrap().validate().unwrap();
        let moved = json!({"request_id":id,"frame":"C-reading","place":place});
        let (route, body) = serde_json::from_value::<PlaceFrameRequest>(moved.clone()).unwrap().route().unwrap();
        assert_eq!((route.as_str(), body), ("/frames/C-reading/place", json!({"request_id":id,"place":place})));
        let mut bound = frame.clone(); bound["mode"] = json!("bound"); bound["objects"] = json!(["T-1"]);
        serde_json::from_value::<FrameRequest>(bound).unwrap().validate().unwrap();
        for key in ["actor", "layer", "guards", "definition"] {
            let mut forged = frame.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<FrameRequest>(forged).is_err(), "{key}");
            let mut forged = moved.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<PlaceFrameRequest>(forged).is_err(), "{key}");
        }
        let mut sneaky = frame.clone(); sneaky["rules"]["actor"] = json!("human:other");
        assert!(serde_json::from_value::<FrameRequest>(sneaky).is_err());
        let mut unnamed = frame.clone(); unnamed["id"] = json!("reading");
        assert!(serde_json::from_value::<FrameRequest>(unnamed).unwrap().validate().is_err());
        let mut escape = frame.clone(); escape["template"] = json!("../kanban");
        assert!(serde_json::from_value::<FrameRequest>(escape).unwrap().validate().is_err());
        let mut escape = moved.clone(); escape["frame"] = json!("../C-reading");
        assert!(serde_json::from_value::<PlaceFrameRequest>(escape).unwrap().route().is_err());
        let mut stale = moved; stale["request_id"] = json!("018fbd42-9959-4ae7-891b-7adb3eac1111");
        assert!(serde_json::from_value::<PlaceFrameRequest>(stale).unwrap().route().is_err());
    }
    #[test]
    fn human_messages_have_frozen_ids_and_no_caller_identity_or_layer() {
        let id = uuid::Uuid::now_v7().to_string();
        let answer = json!({"request_id":id,"hold":"H-1","text":"沿用现有实现","remember_as":"module_decision","targets":["A"]});
        serde_json::from_value::<AnswerRequest>(answer.clone()).unwrap().validate().unwrap();
        let prompt = json!({"request_id":id,"scope":{"kind":"task","task":"T-1"},"text":"先运行现有测试","targets":[]});
        serde_json::from_value::<PromptRequest>(prompt.clone()).unwrap().validate().unwrap();
        for key in ["actor", "layer", "guards"] {
            let mut forged = answer.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<AnswerRequest>(forged).is_err());
            let mut forged = prompt.clone(); forged[key] = json!("human:other");
            assert!(serde_json::from_value::<PromptRequest>(forged).is_err());
        }
        let mut unsupported = answer.clone(); unsupported["remember_as"] = json!("process_rule");
        assert!(serde_json::from_value::<AnswerRequest>(unsupported).is_err());
        let mut bad_target = answer.clone(); bad_target["targets"] = json!([{"id":"A","layer":"L4"}]);
        assert!(serde_json::from_value::<AnswerRequest>(bad_target).is_err());
        let mut empty = prompt.clone(); empty["text"] = json!("  ");
        assert!(serde_json::from_value::<PromptRequest>(empty).unwrap().validate().is_err());
        let mut assumed = answer.clone(); assumed["assumption"] = json!("用户都用 Excel 打开");
        let parsed = serde_json::from_value::<AnswerRequest>(assumed.clone()).unwrap();
        parsed.validate().unwrap();
        assert_eq!(json!(parsed), assumed, "the assumption reaches the daemon unchanged");
        assert_eq!(json!(serde_json::from_value::<AnswerRequest>(answer.clone()).unwrap()), answer, "no assumption, no field");
        assumed["assumption"] = json!("两行\n假设");
        assert!(serde_json::from_value::<AnswerRequest>(assumed).unwrap().validate().is_err());
        let mut wrong_id = prompt; wrong_id["request_id"] = json!("018fbd42-9959-4ae7-891b-7adb3eac1111");
        assert!(serde_json::from_value::<PromptRequest>(wrong_id).unwrap().validate().is_err());
    }
    #[test]
    fn publication_confirmation_has_no_caller_identity_or_file_bytes() {
        let approval = json!({
            "request_id": uuid::Uuid::now_v7().to_string(), "target": "B0",
            "prepared_at": "2026-10-21T00:00:00Z", "content_hash": "a".repeat(64),
        });
        serde_json::from_value::<PublicationApproval>(approval.clone())
            .unwrap()
            .validate()
            .unwrap();
        for (key, value) in [
            ("actor", json!("human:other")),
            ("files", json!({})),
            ("approved", json!(true)),
        ] {
            let mut forged = approval.clone();
            forged[key] = value;
            assert!(serde_json::from_value::<PublicationApproval>(forged).is_err());
        }
        for target in ["../B0", "B01", "main", "B-1"] {
            let mut invalid = approval.clone();
            invalid["target"] = json!(target);
            assert!(serde_json::from_value::<PublicationApproval>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
    }
    #[test]
    fn recovery_and_publication_require_server_generated_uuid_v7_ids() {
        assert!(validate_human_request_id(&uuid::Uuid::now_v7().to_string()).is_ok());
        for id in [
            "../confirm",
            "00000000-0000-0000-0000-000000000000",
            "018fbd42-9959-4ae7-891b-7adb3eac1111",
        ] {
            assert!(validate_human_request_id(id).is_err());
        }
    }
    #[test]
    fn verification_request_ids_are_canonical_uuid_values() {
        assert!(validate_verification_id(&uuid::Uuid::now_v7().to_string()).is_ok());
        assert!(validate_verification_id("018fbd42-9959-4ae7-891b-7adb3eac1111").is_ok());
        for id in [
            "../request",
            "00000000-0000-0000-0000-000000000000",
            "018FBD42-9959-4AE7-891B-7ADB3EAC1111",
        ] {
            assert!(validate_verification_id(id).is_err());
        }
    }
    #[test]
    fn baseline_ids_accept_only_content_digests() {
        assert!(validate_baseline_id(&format!("d-{}", "a0".repeat(32))).is_ok());
        for id in ["../draft", "d-abc", "d-"] {
            assert!(validate_baseline_id(id).is_err());
        }
        assert!(validate_baseline_id(&format!("d-{}", "A".repeat(64))).is_err());
        assert!(validate_baseline_id(&format!("d-{}", "g".repeat(64))).is_err());
        assert!(validate_baseline_id(&format!("d-{}", "a".repeat(65))).is_err());
    }
    #[test]
    fn bridge_accepts_only_explicit_loopback_endpoint() {
        for (address, port, instance, valid) in [
            ("127.0.0.1", 1234, "instance", true),
            ("localhost", 1234, "instance", false),
            ("example.com", 1234, "instance", false),
            ("127.0.0.1", 0, "instance", false),
            ("127.0.0.1", 1234, "", false),
        ] {
            assert_eq!(
                Endpoint {
                    address: address.into(),
                    port,
                    instance: instance.into()
                }
                .validate()
                .is_ok(),
                valid
            );
        }
    }
    #[test]
    fn ensure_output_is_read_from_the_c0_envelope() {
        let ok = br#"{"v":1,"ok":true,"data":{"address":"127.0.0.1","port":4100,"pid":7,"instance":"i-1","host":"h"},"delivered":[],"next":[],"warnings":[]}"#;
        let endpoint = parse_ensure_output(ok).unwrap();
        assert_eq!((endpoint.address.as_str(), endpoint.port, endpoint.instance.as_str()), ("127.0.0.1", 4100, "i-1"));
        let failed = r#"{"v":1,"ok":false,"error":{"code":"internal","message":"运行目录不可写"},"delivered":[],"next":[],"warnings":[]}"#.as_bytes();
        assert_eq!(parse_ensure_output(failed).err().unwrap(), "运行目录不可写");
        let newer = br#"{"v":2,"ok":true,"data":{}}"#;
        assert!(parse_ensure_output(newer).err().unwrap().contains("重新构建"));
        let remote = br#"{"v":1,"ok":true,"data":{"address":"10.0.0.1","port":4100,"instance":"i-1"},"delivered":[],"next":[],"warnings":[]}"#;
        assert!(parse_ensure_output(remote).is_err());
        assert!(parse_ensure_output(br#"{"address":"127.0.0.1","port":4100,"instance":"i-1"}"#).is_err(), "v0.1.1 bare output is refused");
    }
}
