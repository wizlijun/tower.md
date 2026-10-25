//! G1 and G4, the development kit's review gates (spec 9, FR-58). A delegate
//! asks a person with `towermd ask --review plan|code|design`; only the person's
//! verdict in the App counts. G1 reviews the plan of one attempt, checked at
//! finish; G4 reviews the code of the tested tree, checked before the merge
//! trailers are issued. An open review blocks either way; the latest verdict
//! on that attempt or tree must be a pass. A run in a `risk: high` module
//! needs both. A design review (spec 16.4) is advice the delegate asked for:
//! it binds the attempt and, while open, holds the task like any review, but
//! no gate requires it. Nothing passes on a timer.
use serde_json::Value;
use towermd_core::reviews::{self, Outcome, Review};

use crate::task_model::Run;

pub const PLAN: &str = "plan";
pub const CODE: &str = "code";
pub const DESIGN: &str = "design";
/// The stages a delegate may ask about.
pub const STAGES: &[&str] = &[PLAN, CODE, DESIGN];
/// The stages a gate requires in a high-risk module.
pub const GATED: &[&str] = &[PLAN, CODE];

/// The builtin checks of a run: review checks join `core.verify` when the
/// run touches a high-risk module (FR-58).
pub fn checks(high_risk: bool) -> Vec<String> {
    let mut checks = vec![crate::delegation_kit::VERIFY_CHECK.to_owned()];
    if high_risk {
        checks.extend(GATED.iter().map(|stage| reviews::check_kind(stage)));
    }
    checks
}

/// G1 at finish: the plan review of this attempt.
pub fn g1(state: &Value, run: &Run) -> Result<(), String> {
    require(state, run, PLAN, "G1", |review| review.request.attempt.as_deref() == Some(run.binding.id.as_str()))
}

/// G4 before admission: the code review of the tested tree.
pub fn g4(state: &Value, run: &Run, tree: &str) -> Result<(), String> {
    require(state, run, CODE, "G4", |review| review.request.version.as_deref() == Some(tree))
}

fn require(state: &Value, run: &Run, stage: &str, gate: &str, bound: impl Fn(&Review) -> bool) -> Result<(), String> {
    let all = reviews::read(state)?;
    let mine: Vec<&Review> = all.values().filter(|review| review.request.object == run.task && review.request.stage == stage).collect();
    if let Some(open) = mine.iter().find(|review| review.open()) {
        return Err(format!("{gate}：评审 {} 尚待人处理，处理前不能继续", open.id));
    }
    let latest = mine.into_iter().filter(|review| bound(review))
        .filter_map(|review| review.resolution.as_ref()).max_by_key(|resolution| resolution.mark.seq);
    match latest {
        Some(resolution) if resolution.verdict == Outcome::Passed => Ok(()),
        Some(resolution) => Err(format!("{gate}：人{}：{}", if resolution.verdict == Outcome::Rejected { "驳回了评审" } else { "要求修改" },
            resolution.comment.as_deref().unwrap_or(""))),
        None if run.high_risk => Err(format!("{gate}：高风险模块须经人评审，先执行 towermd ask \"<摘要>\" --review {stage}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_design_review_may_be_asked_but_no_gate_requires_it() {
        assert!(STAGES.contains(&DESIGN));
        assert_eq!(checks(true), vec![crate::delegation_kit::VERIFY_CHECK.to_owned(), reviews::check_kind(PLAN), reviews::check_kind(CODE)]);
        let request = reviews::Requested { object: "T-1".into(), stage: DESIGN.into(), summary: "s".into(), artifacts: vec![],
            version: None, attempt: Some("R-1".into()) };
        assert!(request.check().is_ok());
    }
}

