//! Pure human-intervention state. Only accepted Event envelopes can establish a
//! review fact; projected display fields never replace the original raw events.
mod ask_state;
pub use crate::recovery_contract::recorded::ReviewGate;
pub use crate::recovery_contract::task_review::{
    ReviewBinding, ReviewFact, ReviewSubject, ReviewVerdict,
};
pub use crate::tasks::SelfReported;
use crate::{
    hub::{Event, Fence},
    recovery_contract::{
        self as contract,
        recorded::{ReviewBodyView, ReviewHoldView, ReviewOpenView, TaskReviewRecorded},
    },
    snapshot::StateProjection,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

pub use crate::ask_contract::{AskHoldView, AskResult, AskState, Recipient, Reply, WaitSlice};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    Project,
    Module { module: String },
    Task { task: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoldBody {
    Ask {
        fingerprint: String,
        question: String,
        module: String,
        options: Vec<String>,
        recommended: Option<usize>,
        context: String,
    },
    Prompt {
        text: String,
        scope: Scope,
    },
    Review {
        binding: ReviewBinding,
        verdict: Option<ReviewVerdict>,
        plan_content: Option<String>,
        source_commit: Option<String>,
        source_path: Option<String>,
        comment: Option<String>,
    },
    Pause {
        revision: String,
        active_runs: Vec<String>,
        resumed: bool,
    },
    Resume {
        pause_revision: String,
        active_runs: Vec<String>,
        confirmed: bool,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Hold {
    #[serde(rename = "type")]
    pub object_type: String,
    pub id: String,
    pub body: HoldBody,
    #[serde(default)]
    pub recipients: Vec<Recipient>,
    pub opened_ms: u64,
    pub opened_seq: u64,
    pub reply: Option<Reply>,
    #[serde(default)]
    pub waits: Vec<WaitSlice>,
    #[serde(default)]
    pub delivered: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub attention: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: String,
    pub hold: String,
    pub run: String,
    pub text: String,
    pub kind: String,
    pub actionable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum HumanCommand {
    AskOpen {
        question: String,
        options: Vec<String>,
        recommended: Option<usize>,
        context: String,
    },
    AskWait {
        hold: String,
    },
    Relay {
        hold: String,
        text: String,
        self_reported: Option<SelfReported>,
    },
    ReviewRequest {
        gate: ReviewGate,
        content: Option<String>,
        source_commit: Option<String>,
        source_path: Option<String>,
    },
    ReviewWait {
        hold: String,
    },
    PauseAll,
    ResumeAll,
}

/// Snapshot metadata, reconstructed from the same actual outer archive/tail as
/// the ordinary Hold objects. Raw opening/reviewed Event bytes are not rewritten.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HumanProjection {
    pub schema: u32,
    pub openings: BTreeMap<String, Event>,
    pub reviewed: BTreeMap<String, Event>,
    pub invalidated_through: u64,
}
impl Default for HumanProjection {
    fn default() -> Self {
        Self {
            schema: 1,
            openings: BTreeMap::new(),
            reviewed: BTreeMap::new(),
            invalidated_through: 0,
        }
    }
}
// 仅兼容 recovery::recover_git_facts 的旧诊断；它不是 typed Hold 或人工事实。
fn is_legacy_git_recovery_hold(event: &Event) -> bool {
    if event.kind != "hold.opened"
        || event.layer != "L3"
        || event.actor != "process:towermd-daemon"
        || event.host != "git"
        || event.obj != "main"
        || !event.guards.is_empty()
        || event.epoch.is_some()
    {
        return false;
    }
    let Some(data) = event.data.as_object() else {
        return false;
    };
    let Some(commit) = data.get("commit").and_then(Value::as_str) else {
        return false;
    };
    data.len() == 3
        && data.get("reason").is_some_and(Value::is_string)
        && data
            .get("raw_lines")
            .and_then(Value::as_array)
            .is_some_and(|lines| lines.iter().all(Value::is_string))
        && contract::object_id(commit).is_ok()
        && crate::events::derived_eid(commit, "hold.opened")
            .is_ok_and(|expected| expected == event.eid)
}
pub(crate) fn is_human_event(event: &Event) -> bool {
    !is_legacy_git_recovery_hold(event)
        && matches!(
            event.kind.as_str(),
            "hold.opened"
                | "hold.requested"
                | "hold.recipient_added"
                | "hold.wait_started"
                | "hold.wait_finished"
                | "hold.answered"
                | "hold.attention"
                | "hold.reviewed"
                | "prompt.delivered"
                | "hub.rebuilt"
        )
}
fn hold_value(event: &Event) -> Result<(Hold, Value), String> {
    let value = event
        .data
        .get("hold")
        .cloned()
        .ok_or("human event lacks complete Hold")?;
    let hold: Hold =
        serde_json::from_value(value.clone()).map_err(|e| format!("invalid Hold: {e}"))?;
    if hold.object_type != "Hold" || hold.id != event.obj || event.layer != "L5" {
        return Err("Hold envelope identity mismatch".into());
    }
    crate::hub::validate_token("Hold ID", &hold.id).map_err(|e| e.to_string())?;
    Ok((hold, value))
}
fn sorted_fences(fences: &[Fence]) -> Result<Vec<Fence>, String> {
    if fences.is_empty() {
        return Err("review requires complete nonempty fences".into());
    }
    let mut sorted = fences.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    crate::hub::validate_names(sorted.iter().map(|f| f.name.as_str()))
        .map_err(|e| e.to_string())?;
    for fence in &sorted {
        crate::hub::validate_fence(fence).map_err(|e| e.to_string())?;
    }
    Ok(sorted)
}
fn same_binding(a: &ReviewBinding, b: &ReviewBinding) -> Result<bool, String> {
    Ok(a.run == b.run
        && a.task == b.task
        && a.baseline == b.baseline
        && a.subject == b.subject
        && sorted_fences(&a.fences)? == sorted_fences(&b.fences)?)
}
fn human_actor(actor: &str) -> Result<(), String> {
    let id = actor
        .strip_prefix("human:")
        .filter(|id| !id.is_empty() && !id.contains([':', '/']))
        .ok_or("review requires an App human actor")?;
    crate::hub::validate_token("human", id).map_err(|e| e.to_string())
}
fn review_envelope(event: &Event, binding: &ReviewBinding, actor: &str) -> Result<(), String> {
    if event.stale
        || event.seq == 0
        || event.actor != actor
        || event.layer != "L5"
        || event.guards != binding.fences
        || event.epoch != binding.fences.first().map(|f| f.epoch)
    {
        return Err("review Event envelope differs from its original binding".into());
    }
    let fences = sorted_fences(&binding.fences)?;
    if fences.iter().any(|f| f.holder != binding.run) {
        return Err("review fences belong to another run".into());
    }
    for (label, id) in [
        ("run", &binding.run),
        ("task", &binding.task),
        ("baseline", &binding.baseline),
    ] {
        crate::hub::validate_token(label, id).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn opening(event: &Event) -> Result<ReviewOpenView, String> {
    let hold: ReviewHoldView =
        serde_json::from_value(event.data["hold"].clone()).map_err(|e| e.to_string())?;
    let ReviewBodyView::Review {
        binding,
        plan_content,
        ..
    } = &hold.body;
    let gate = match binding.subject {
        ReviewSubject::Plan { .. } => ReviewGate::G1,
        ReviewSubject::Tree { .. } => ReviewGate::G4,
    };
    let view = ReviewOpenView {
        hold: hold.clone(),
        gate,
        binding: binding.clone(),
        plan_content: plan_content.clone(),
    };
    contract::validate_review_open(&view)?;
    if event.kind != "hold.opened" || event.obj != hold.id {
        return Err("invalid original review opening".into());
    }
    review_envelope(event, &view.binding, "process:towermd-cli")?;
    Ok(view)
}
fn reviewed(event: &Event, original: &Event) -> Result<TaskReviewRecorded, String> {
    let open = opening(original)?;
    let record: TaskReviewRecorded =
        serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
    contract::validate_recorded_data(
        &record.request_body.project_id,
        "hold.reviewed",
        &event.data,
    )?;
    if event.kind != "hold.reviewed"
        || event.obj != open.hold.id
        || event.host != record.request_body.host
        || event.seq <= original.seq
        || record.fact.answer_eid != event.eid
        || record.fact.binding != open.binding
    {
        return Err("review answer differs from original opening/Event identity".into());
    }
    human_actor(&record.fact.actor)?;
    review_envelope(event, &record.fact.binding, &record.fact.actor)?;
    let mut expected = serde_json::to_value(&open.hold).map_err(|e| e.to_string())?;
    expected["body"]["verdict"] = json!(record.fact.verdict);
    expected["body"]["comment"] = json!(record.comment);
    expected["reply"] = json!(record.hold.reply);
    if expected != serde_json::to_value(&record.hold).map_err(|e| e.to_string())? {
        return Err("review altered immutable original Hold".into());
    }
    Ok(record)
}
impl HumanProjection {
    pub fn apply_to(
        &mut self,
        holds: &mut BTreeMap<String, Value>,
        event: &Event,
    ) -> Result<(), String> {
        if event.stale || !is_human_event(event) {
            return Ok(());
        }
        if self.schema != 1 {
            return Err("unknown human projection schema".into());
        }
        if event.kind == "hub.rebuilt" {
            self.invalidated_through = self.invalidated_through.max(event.seq);
            return Ok(());
        }
        if event.kind == "hold.recipient_added" && event.data["hold"]["body"]["kind"] != "ask" {
            return Err("recipient_added is limited to Ask Holds".into());
        }
        if event.data["hold"]["body"]["kind"] == "ask" {
            return ask_state::apply(self, holds, event);
        }
        let (hold, mut raw) = hold_value(event)?;
        if event.kind == "hold.opened" {
            if let Some(old) = self.openings.get(&hold.id) {
                return if old == event {
                    Ok(())
                } else {
                    Err("Hold opening identity was reused".into())
                };
            }
            if matches!(hold.body, HoldBody::Review { .. }) {
                opening(event)?;
            }
            if hold.reply.is_some() {
                return Err("opening is already answered".into());
            }
            raw["opened_seq"] = json!(event.seq);
            self.openings.insert(hold.id.clone(), event.clone());
            holds.insert(hold.id, raw);
            return Ok(());
        }
        let original = self
            .openings
            .get(&hold.id)
            .ok_or("Hold update lacks original accepted opening")?;
        if original.seq >= event.seq
            || hold.opened_ms
                != original.data["hold"]["opened_ms"]
                    .as_u64()
                    .ok_or("opening lacks time")?
        {
            return Err("Hold update altered opening order/time".into());
        }
        let previous = holds
            .get(&hold.id)
            .ok_or("Hold update lacks projected state")?;
        if event.kind == "hold.requested" {
            let (_, original_hold) = hold_value(original)?;
            if raw["body"] != original_hold["body"] {
                return Err("Hold request changed original question/subject".into());
            }
            return Ok(());
        }
        if event.kind == "hold.reviewed" {
            if let Some(old) = self.reviewed.get(&hold.id) {
                return if old == event {
                    Ok(())
                } else {
                    Err("review already has an immutable human answer".into())
                };
            }
            let record = reviewed(event, original)?;
            // ReviewWait/delivery may have changed the runtime Hold. The proof's raw
            // Hold remains the original, while projection changes only answer fields.
            raw = previous.clone();
            raw["body"]["verdict"] = json!(record.fact.verdict);
            raw["body"]["comment"] = json!(record.comment);
            raw["reply"] = json!(record.hold.reply);
            self.reviewed.insert(hold.id.clone(), event.clone());
        } else if matches!(hold.body, HoldBody::Review { .. })
            && (raw["body"] != previous["body"] || raw["reply"] != previous["reply"])
        {
            return Err("ordinary Hold event cannot answer/change a review".into());
        }
        raw["opened_seq"] = json!(original.seq);
        holds.insert(hold.id, raw);
        Ok(())
    }
}
#[derive(Clone, Debug, Default)]
pub struct HumanState {
    pub holds: BTreeMap<String, Hold>,
    raw_holds: BTreeMap<String, Value>,
    canonical: HumanProjection,
    review_facts: BTreeMap<String, ReviewFact>,
    review_views: BTreeMap<String, ReviewHoldView>,
}
impl HumanState {
    /// Legacy StateProjection has no canonical human provenance. Until the typed
    /// snapshot schema is attached, missing provenance must never grant approval.
    pub fn restore(&mut self, projection: &StateProjection) -> Result<(), String> {
        if let Some(canonical) = &projection.human {
            return self.restore_projected(projection, canonical);
        }
        *self = Self::default();
        if projection
            .objects
            .get("Hold")
            .is_some_and(|holds| !holds.is_empty())
        {
            return Err(
                "Hold projection needs canonical human metadata rebuilt from verified outer events"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn restore_projected(
        &mut self,
        projection: &StateProjection,
        canonical: &HumanProjection,
    ) -> Result<(), String> {
        *self = Self::default();
        self.restore_parts(
            projection.objects.get("Hold").cloned().unwrap_or_default(),
            canonical.clone(),
        )
    }
    fn restore_parts(
        &mut self,
        raw_holds: BTreeMap<String, Value>,
        canonical: HumanProjection,
    ) -> Result<(), String> {
        if canonical.schema != 1 {
            return Err("unknown human projection schema".into());
        }
        let mut holds = BTreeMap::new();
        let mut facts = BTreeMap::new();
        let mut views = BTreeMap::new();
        for (id, raw) in &raw_holds {
            let hold: Hold = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
            let event = canonical
                .openings
                .get(id)
                .ok_or("projected Hold lacks canonical opening")?;
            if hold.object_type != "Hold"
                || hold.id != *id
                || hold.opened_seq != event.seq
                || hold.opened_ms
                    != event.data["hold"]["opened_ms"]
                        .as_u64()
                        .ok_or("opening lacks time")?
            {
                return Err("projected Hold changed canonical opening identity".into());
            }
            if let HoldBody::Review {
                binding,
                verdict,
                plan_content,
                source_commit,
                source_path,
                comment,
            } = &hold.body
            {
                let view = opening(event)?;
                let ReviewBodyView::Review {
                    source_commit: original_commit,
                    source_path: original_path,
                    ..
                } = &view.hold.body;
                if binding != &view.binding
                    || plan_content != &view.plan_content
                    || source_commit != original_commit
                    || source_path != original_path
                {
                    return Err("projected review differs from original subject/content".into());
                }
                match canonical.reviewed.get(id) {
                    Some(answer) => {
                        let record = reviewed(answer, event)?;
                        if verdict.as_ref() != Some(&record.fact.verdict)
                            || comment.as_ref() != Some(&record.comment)
                            || raw["reply"] != json!(record.hold.reply)
                        {
                            return Err(
                                "projected answer differs from original human answer".into()
                            );
                        }
                        if answer.seq > canonical.invalidated_through {
                            facts.insert(id.clone(), record.fact);
                        }
                    }
                    None if verdict.is_none() && comment.is_none() && hold.reply.is_none() => {}
                    None => return Err("projected review has no accepted answer Event".into()),
                }
                views.insert(id.clone(), view.hold);
            }
            holds.insert(id.clone(), hold);
        }
        if canonical.openings.keys().any(|id| !holds.contains_key(id))
            || canonical
                .reviewed
                .keys()
                .any(|id| !canonical.openings.contains_key(id))
        {
            return Err("human metadata is missing its complete Hold projection".into());
        }
        self.holds = holds;
        self.raw_holds = raw_holds;
        self.canonical = canonical;
        self.review_facts = facts;
        self.review_views = views;
        Ok(())
    }
    pub fn apply(&mut self, event: &Event) -> Result<(), String> {
        if event.stale || !is_human_event(event) {
            return Ok(());
        }
        let mut canonical = self.canonical.clone();
        let mut holds = self.raw_holds.clone();
        canonical.apply_to(&mut holds, event)?;
        self.restore_parts(holds, canonical)
    }
    pub fn projection(&self) -> (&BTreeMap<String, Value>, &HumanProjection) {
        (&self.raw_holds, &self.canonical)
    }
    fn latest_review(&self, run: &str, plan: bool) -> Option<&Event> {
        self.canonical
            .openings
            .values()
            .filter(|event| {
                event.seq > self.canonical.invalidated_through
                    && self.review_views.get(&event.obj).is_some_and(|hold| {
                        let ReviewBodyView::Review { binding, .. } = &hold.body;
                        binding.run == run
                            && matches!(binding.subject, ReviewSubject::Plan { .. }) == plan
                    })
            })
            .max_by_key(|event| event.seq)
    }
    pub fn current_plan_subject(&self, run: &str) -> Result<ReviewSubject, String> {
        let event = self
            .latest_review(run, true)
            .ok_or("current run has no canonical Plan opening")?;
        let ReviewBodyView::Review { binding, .. } = &self.review_views[&event.obj].body;
        Ok(binding.subject.clone())
    }
    pub fn require_human_review(&self, binding: &ReviewBinding) -> Result<&ReviewFact, String> {
        sorted_fences(&binding.fences)?;
        let original = self
            .latest_review(
                &binding.run,
                matches!(binding.subject, ReviewSubject::Plan { .. }),
            )
            .ok_or("current run has no matching review opening")?;
        let fact = self
            .review_facts
            .get(&original.obj)
            .ok_or("current review has no accepted human answer")?;
        human_actor(&fact.actor)?;
        if fact.verdict != ReviewVerdict::Approved || !same_binding(&fact.binding, binding)? {
            return Err(
                "current human approval does not match full run/task/baseline/subject/fences"
                    .into(),
            );
        }
        Ok(fact)
    }
    pub fn original_review_hold(&self, id: &str) -> Result<&ReviewHoldView, String> {
        self.review_views
            .get(id)
            .ok_or_else(|| "missing canonical raw review opening".into())
    }
    pub fn review_originals(&self, binding: &ReviewBinding) -> Result<(&Event, &Event), String> {
        self.require_human_review(binding)?;
        let opening = self
            .latest_review(
                &binding.run,
                matches!(binding.subject, ReviewSubject::Plan { .. }),
            )
            .ok_or("missing opening")?;
        Ok((
            opening,
            self.canonical
                .reviewed
                .get(&opening.obj)
                .ok_or("missing original reviewed Event")?,
        ))
    }
    pub fn paused(&self) -> bool {
        self.holds.values().any(|hold| {
            self.canonical
                .openings
                .get(&hold.id)
                .is_some_and(|e| e.seq > self.canonical.invalidated_through)
                && matches!(hold.body, HoldBody::Pause { resumed: false, .. })
        })
    }
    pub fn inbox(&self, recipients: &[Recipient], active: &[Fence]) -> Vec<Message> {
        let mut messages = vec![];
        for hold in self.holds.values() {
            let current = self
                .canonical
                .openings
                .get(&hold.id)
                .is_some_and(|event| event.seq > self.canonical.invalidated_through);
            for recipient in &hold.recipients {
                if !recipients.iter().any(|r| r == recipient) {
                    continue;
                }
                let message = if let Some(reply) = &hold.reply {
                    Some(Message {
                        id: format!("{}/{}/{}", hold.id, reply.id, recipient.run),
                        hold: hold.id.clone(),
                        run: recipient.run.clone(),
                        text: reply.text.clone(),
                        kind: "reply".into(),
                        actionable: current && live_recipient(recipient, active),
                    })
                } else if let HoldBody::Prompt { text, .. } = &hold.body {
                    Some(Message {
                        id: format!("{}/prompt/{}", hold.id, recipient.run),
                        hold: hold.id.clone(),
                        run: recipient.run.clone(),
                        text: text.clone(),
                        kind: "prompt".into(),
                        actionable: current && live_recipient(recipient, active),
                    })
                } else {
                    None
                };
                if let Some(message) = message {
                    if !hold
                        .delivered
                        .get(&recipient.run)
                        .is_some_and(|ids| ids.contains(&message.id))
                    {
                        messages.push(message);
                    }
                }
            }
        }
        messages
    }
}
fn live_recipient(recipient: &Recipient, active: &[Fence]) -> bool {
    sorted_fences(&recipient.fences).is_ok()
        && sorted_fences(active).is_ok()
        && recipient
            .fences
            .iter()
            .all(|f| f.holder == recipient.run && active.iter().any(|a| a == f))
}
pub fn normalize_question(text: &str) -> String {
    let compatible: String = text.nfkc().collect();
    compatible
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
        .trim_end_matches(crate::question_punctuation::is_punctuation)
        .trim_end()
        .to_owned()
}
pub fn fingerprint(module: &str, question: &str) -> String {
    serde_json::to_string(&(module, normalize_question(question)))
        .expect("string tuple serialization is infallible")
}
pub fn wait_budget(hold: &Hold, run: &str, now_ms: u64) -> u64 {
    hold.waits
        .iter()
        .filter(|slice| slice.recipient_run == run)
        .fold(0u64, |spent, slice| {
            spent.saturating_add(
                slice
                    .finished_ms
                    .unwrap_or(now_ms)
                    .min(slice.deadline_ms)
                    .saturating_sub(slice.started_ms),
            )
        })
        .min(600_000)
}
pub fn next_slice(
    hold: &Hold,
    run: &str,
    now_ms: u64,
    request_id: &str,
) -> Result<Option<WaitSlice>, String> {
    contract::recorded::request_event_eid(request_id, "hold.wait_started")?;
    if let Some(saved) = hold.waits.iter().find(|slice| slice.id == request_id) {
        if saved.recipient_run != run {
            return Err("wait request UUID belongs to another recipient run".into());
        }
        return Ok(Some(saved.clone()));
    }
    let spent = wait_budget(hold, run, now_ms);
    if hold.reply.is_some()
        || !hold.recipients.iter().any(|r| r.run == run)
        || spent >= 600_000
        || hold
            .waits
            .iter()
            .any(|s| s.recipient_run == run && s.finished_ms.is_none() && s.deadline_ms > now_ms)
    {
        return Ok(None);
    }
    Ok(Some(WaitSlice {
        id: request_id.into(),
        recipient_run: run.into(),
        started_ms: now_ms,
        deadline_ms: now_ms
            .checked_add(90_000.min(600_000 - spent))
            .ok_or("wait deadline overflow")?,
        finished_ms: None,
    }))
}
pub fn ask_state(hold: &Hold, run: &str, live_fences: &[Fence], now_ms: u64) -> AskState {
    if !hold
        .recipients
        .iter()
        .any(|r| r.run == run && live_recipient(r, live_fences))
    {
        AskState::Expired
    } else if hold.reply.is_some() {
        AskState::Answered
    } else if wait_budget(hold, run, now_ms) >= 600_000 {
        AskState::Deferred
    } else {
        AskState::Pending
    }
}

/// Only authoritative Catalog documents are eligible; worktree text is never read.
pub fn decision_answer(
    catalog: &crate::catalog::Catalog,
    module: &str,
    question: &str,
    now_ms: u64,
) -> Result<Option<Reply>, String> {
    if !catalog
        .objects
        .get(module)
        .is_some_and(|d| d.doc_type() == Some("Module"))
    {
        return Err("question module is not a canonical Catalog Module".into());
    }
    let normalized = normalize_question(question);
    if normalized.is_empty() {
        return Ok(None);
    }
    let now = i64::try_from(now_ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .ok_or("invalid question time")?;
    let today = now.date_naive();
    struct Candidate<'a> {
        id: &'a str,
        context: &'a str,
        text: &'a str,
        affects: Vec<&'a str>,
        supersedes: Vec<&'a str>,
    }
    fn refs(value: Option<&serde_yaml::Value>, allow_single: bool) -> Option<Vec<&str>> {
        match value {
            None => Some(vec![]),
            Some(serde_yaml::Value::String(id)) if allow_single && !id.is_empty() => Some(vec![id]),
            Some(serde_yaml::Value::Sequence(ids)) => ids
                .iter()
                .map(|id| id.as_str().filter(|id| !id.is_empty()))
                .collect(),
            _ => None,
        }
    }
    let mut valid = vec![];
    for (id, doc) in &catalog.objects {
        if doc.doc_type() != Some("Decision") {
            continue;
        }
        match doc.get("status") {
            None => {}
            Some(serde_yaml::Value::String(status))
                if matches!(status.as_str(), "stable" | "draft") => {}
            _ => continue,
        }
        if let Some(stale) = doc.get("stale_after") {
            let Some(raw) = stale.as_str() else { continue };
            let Ok(date) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") else {
                continue;
            };
            if date.format("%Y-%m-%d").to_string() != raw || today >= date {
                continue;
            }
        }
        let (Some(context), Some(text), Some(affects), Some(supersedes)) = (
            doc.get_str("context")
                .filter(|s| !normalize_question(s).is_empty()),
            doc.get_str("decision").filter(|s| !s.trim().is_empty()),
            refs(doc.get("affects"), false),
            refs(doc.get("supersedes"), true),
        ) else {
            continue;
        };
        if affects.is_empty()
            || supersedes.iter().any(|id| {
                !catalog
                    .objects
                    .get(*id)
                    .is_some_and(|d| d.doc_type() == Some("Decision"))
            })
        {
            continue;
        }
        valid.push(Candidate {
            id,
            context,
            text,
            affects,
            supersedes,
        });
    }
    let superseded: BTreeSet<_> = valid
        .iter()
        .flat_map(|c| c.supersedes.iter().copied())
        .collect();
    let matches = valid
        .iter()
        .filter(|c| {
            !superseded.contains(c.id)
                && c.affects.contains(&module)
                && normalize_question(c.context) == normalized
        })
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(
            "conflicting current Decisions answer the same normalized module question".into(),
        );
    }
    let Some(answer) = matches.first() else {
        return Ok(None);
    };
    let id = contract::canonical_hash(&(
        "decision-reply-v1",
        &catalog.revision,
        answer.id,
        module,
        &normalized,
    ))?;
    Ok(Some(Reply {
        id: format!("reply-decision-{id}"),
        text: answer.text.into(),
        actor: "process:towermd-dedupe".into(),
        relayed_from: None,
        self_reported: None,
        decision: Some(answer.id.into()),
        decision_revision: Some(catalog.revision.clone()),
        at_ms: now_ms,
        extra: BTreeMap::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_requires_exact_human_binding() {
        let binding = ReviewBinding {
            run: "R".into(),
            task: "T".into(),
            baseline: "B0".into(),
            fences: vec![Fence {
                name: "module:A".into(),
                holder: "R".into(),
                epoch: 7,
            }],
            subject: ReviewSubject::Plan {
                sha256: "a".repeat(64),
            },
        };
        let mut state = HumanState::default();
        assert!(state.require_human_review(&binding).is_err());
        state.review_facts.insert(
            "H".into(),
            ReviewFact {
                binding: binding.clone(),
                verdict: ReviewVerdict::Approved,
                actor: "process:towermd-cli".into(),
                answer_eid: "answer-1".into(),
            },
        );
        assert!(
            state.require_human_review(&binding).is_err(),
            "process report cannot authorize a human gate"
        );
    }
}
