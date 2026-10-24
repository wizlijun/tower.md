//! Requests for a person's review (core spec 6.3 "请求人评审", FR-124). A
//! delegate asks for one; only a person resolves it in the App, by passing
//! it, requesting changes or rejecting it, with comments for the latter two.
//! Nothing resolves a review on a timer: an open review stays open.
//!
//! A resolved review is `check` evidence of kind `human.review.<stage>`,
//! bound to the reviewed version, or else to the attempt it was asked in. It
//! is trusted only when a person signed it, so a kit may declare such a kind
//! among its own checks and a project may name it in `gates.yaml`. The
//! `Review` group is created on first use, so older projections keep their
//! shape.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    delegation::{Evidence, SourceKind},
    delegation_projection::Mark,
    hub::Event,
    question_model::{human_actor, nonempty, safe_path, valid_id},
};

pub const GROUP: &str = "Review";
pub const LAYER: &str = "L5";
pub const REQUESTED: &str = "review.requested";
pub const RESOLVED: &str = "review.resolved";
/// Check kinds a resolved review reports: `human.review.<stage>`.
pub const CHECK_PREFIX: &str = "human.review.";
pub const SUMMARY_CHARS: usize = 2_000;
pub const COMMENT_CHARS: usize = 4_000;
pub const MAX_ARTIFACTS: usize = 16;

/// `review.requested`; the event's `obj` is the review's ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requested {
    /// The object under review.
    pub object: String,
    /// What is reviewed, named by the kit or skill, e.g. `plan`.
    pub stage: String,
    pub summary: String,
    /// Relative paths a reviewer should read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    /// The object version under review, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The attempt it was asked in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    ChangesRequested,
    Rejected,
}

/// `review.resolved`; the event's `obj` is the review's ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolved {
    pub verdict: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub verdict: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub mark: Mark,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub id: String,
    pub request: Requested,
    pub requested: Mark,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
}

impl Review {
    pub fn open(&self) -> bool {
        self.resolution.is_none()
    }
}

pub type Reviews = BTreeMap<String, Review>;

pub fn check_kind(stage: &str) -> String {
    format!("{CHECK_PREFIX}{stage}")
}

/// A review check kind: trusted only from a person.
pub fn is_review_kind(check: &str) -> bool {
    check.starts_with(CHECK_PREFIX)
}

pub fn read(state: &Value) -> Result<Reviews, String> {
    match state.get(GROUP) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
        None => Ok(Reviews::new()),
    }
}

/// Open reviews of one object, oldest first.
pub fn open_for<'a>(reviews: &'a Reviews, object: &'a str) -> impl Iterator<Item = &'a Review> + 'a {
    reviews.values().filter(move |review| review.open() && review.request.object == object)
}

fn bounded(text: &str, label: &str, max: usize) -> Result<(), String> {
    nonempty(text, label)?;
    if text.chars().count() > max {
        return Err(format!("{label}不能超过 {max} 字"));
    }
    Ok(())
}

impl Requested {
    pub fn check(&self) -> Result<(), String> {
        valid_id(&self.object)?;
        crate::tower_api::signal_kind(&check_kind(&self.stage)).map_err(|_| format!("评审阶段须是小写短名：{}", self.stage))?;
        bounded(&self.summary, "评审摘要", SUMMARY_CHARS)?;
        if self.artifacts.len() > MAX_ARTIFACTS {
            return Err(format!("评审材料最多 {MAX_ARTIFACTS} 项"));
        }
        self.artifacts.iter().try_for_each(|path| safe_path(path))?;
        for value in [&self.version, &self.attempt].into_iter().flatten() {
            valid_id(value)?;
        }
        Ok(())
    }
}

impl Resolved {
    /// Requesting changes and rejecting say why.
    pub fn check(&self) -> Result<(), String> {
        match (&self.verdict, &self.comment) {
            (_, Some(comment)) => bounded(comment, "评审意见", COMMENT_CHARS),
            (Outcome::Passed, None) => Ok(()),
            (_, None) => Err("要求修改或驳回须附意见".into()),
        }
    }
}

/// Resolved reviews of `object` as check evidence, signed by the person.
pub fn evidence(state: &Value, object: &str) -> Result<Vec<Evidence>, String> {
    Ok(read(state)?.into_values().filter(|review| review.request.object == object).filter_map(|review| {
        let resolution = review.resolution?;
        Some(Evidence {
            id: review.id, source_kind: SourceKind::Check, criteria: vec![],
            passed: Some(resolution.verdict == Outcome::Passed),
            version: review.request.version.or(review.request.attempt.clone()),
            actor: resolution.mark.actor, at: resolution.mark.at, note: resolution.comment,
            attempt: review.request.attempt.unwrap_or_default(), check: Some(check_kind(&review.request.stage)),
        })
    }).collect())
}

pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != REQUESTED && event.kind != RESOLVED {
        return Ok(false);
    }
    if event.layer != LAYER {
        return Err(format!("{} 须在 {LAYER}", event.kind));
    }
    valid_id(&event.obj)?;
    let mut reviews = read(state)?;
    let mark = Mark { at: event.t.clone(), eid: event.eid.clone(), seq: event.seq, actor: event.actor.clone() };
    if event.kind == REQUESTED {
        if human_actor(&event.actor) || event.actor.trim().is_empty() {
            return Err("评审由受托者请求，人在 App 中处理".into());
        }
        let request: Requested = serde_json::from_value(event.data.clone()).map_err(|e| format!("评审请求无效：{e}"))?;
        request.check()?;
        if reviews.contains_key(&event.obj) {
            return Err("评审 ID 重复".into());
        }
        if open_for(&reviews, &request.object).any(|review| review.request.stage == request.stage) {
            return Err("该对象的同一阶段已有待处理的评审".into());
        }
        reviews.insert(event.obj.clone(), Review { id: event.obj.clone(), request, requested: mark, resolution: None });
    } else {
        if !human_actor(&event.actor) {
            return Err("评审只能由人在 App 中处理".into());
        }
        if !event.guards.is_empty() {
            return Err("评审结论不带 fences".into());
        }
        let body: Resolved = serde_json::from_value(event.data.clone()).map_err(|e| format!("评审结论无效：{e}"))?;
        body.check()?;
        let review = reviews.get_mut(&event.obj).ok_or("评审不存在")?;
        if !review.open() {
            return Err("评审已处理".into());
        }
        review.resolution = Some(Resolution { verdict: body.verdict, comment: body.comment, mark });
    }
    state[GROUP] = serde_json::to_value(&reviews).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(seq: u64, kind: &str, actor: &str, obj: &str, data: Value) -> Event {
        serde_json::from_value(json!({"seq":seq,"eid":format!("0192c2d0-0000-7000-8000-00000000000{seq}"),"host":"h",
            "t":format!("2026-10-24T10:00:0{seq}Z"),"actor":actor,"type":kind,"layer":LAYER,"obj":obj,"data":data,
            "epoch":null,"stale":false,"cost_usd":null})).unwrap()
    }

    fn ask(seq: u64, id: &str, stage: &str) -> Event {
        event(seq, REQUESTED, "process:towermd-cli", id, json!({"object":"T-1","stage":stage,"summary":"计划见 docs/plan.md","attempt":"R-1"}))
    }

    #[test]
    fn other_events_leave_no_group() {
        let mut state = json!({});
        assert!(!apply(&mut state, &event(1, "waiver.granted", "human:local", "T-1", json!({}))).unwrap());
        assert_eq!(state, json!({}));
        assert!(read(&state).unwrap().is_empty());
    }

    #[test]
    fn a_delegate_asks_and_only_a_person_resolves_once() {
        let mut state = json!({});
        assert!(apply(&mut state, &event(1, REQUESTED, "human:local", "V-1", json!({"object":"T-1","stage":"plan","summary":"s"}))).is_err());
        assert!(apply(&mut state, &ask(1, "V-1", "plan")).unwrap());
        assert!(apply(&mut state, &ask(2, "V-2", "plan")).unwrap_err().contains("待处理"), "one open review per stage");
        assert!(apply(&mut state, &ask(3, "V-1", "code")).unwrap_err().contains("重复"));
        for actor in ["process:towermd-cli", "process:towermd", "claude-code/2.1"] {
            let forged = event(4, RESOLVED, actor, "V-1", json!({"verdict":"passed"}));
            assert!(apply(&mut state, &forged).is_err(), "{actor}");
        }
        assert!(apply(&mut state, &event(4, RESOLVED, "human:local", "V-1", json!({"verdict":"rejected"}))).unwrap_err().contains("附意见"));
        assert!(apply(&mut state, &event(4, RESOLVED, "human:local", "V-1", json!({"verdict":"changes_requested","comment":"补测试"}))).unwrap());
        assert!(apply(&mut state, &event(5, RESOLVED, "human:local", "V-1", json!({"verdict":"passed"}))).unwrap_err().contains("已处理"));
        assert!(apply(&mut state, &ask(6, "V-3", "plan")).unwrap(), "a new review after the old one was resolved");
        let reviews = read(&state).unwrap();
        assert_eq!(open_for(&reviews, "T-1").map(|review| review.id.as_str()).collect::<Vec<_>>(), ["V-3"]);
    }

    #[test]
    fn resolved_reviews_are_person_signed_check_evidence_bound_to_version_or_attempt() {
        let mut state = json!({});
        apply(&mut state, &ask(1, "V-1", "plan")).unwrap();
        assert!(evidence(&state, "T-1").unwrap().is_empty(), "an open review is no evidence, however long it waits");
        apply(&mut state, &event(2, RESOLVED, "human:local", "V-1", json!({"verdict":"passed"}))).unwrap();
        apply(&mut state, &event(3, REQUESTED, "process:towermd-cli", "V-2",
            json!({"object":"T-1","stage":"code","summary":"s","version":"abc123","attempt":"R-1"}))).unwrap();
        apply(&mut state, &event(4, RESOLVED, "human:local", "V-2", json!({"verdict":"rejected","comment":"不安全"}))).unwrap();
        let found = evidence(&state, "T-1").unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].check.as_deref(), found[0].version.as_deref(), found[0].passed), (Some("human.review.plan"), Some("R-1"), Some(true)));
        assert_eq!((found[1].check.as_deref(), found[1].version.as_deref(), found[1].passed), (Some("human.review.code"), Some("abc123"), Some(false)));
        assert!(found.iter().all(|entry| entry.actor == "human:local" && entry.source_kind == SourceKind::Check));
    }

    #[test]
    fn requests_are_bounded() {
        for data in [
            json!({"object":"T-1","stage":"Plan","summary":"s"}),
            json!({"object":"T-1","stage":"plan","summary":" "}),
            json!({"object":"T-1","stage":"plan","summary":"s","artifacts":["../x"]}),
            json!({"object":"T-1","stage":"plan","summary":"s","extra":1}),
        ] {
            assert!(apply(&mut json!({}), &event(1, REQUESTED, "process:towermd-cli", "V-1", data.clone())).is_err(), "{data}");
        }
    }
}
