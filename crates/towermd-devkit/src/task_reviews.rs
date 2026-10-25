//! G1 and G4, the development kit's review gates (spec 9, FR-58). A delegate
//! asks a person with `towermd ask --review plan|code|design`; only the person's
//! verdict in the App counts. G1 reviews the plan of one attempt, checked at
//! finish; G4 reviews the code of the tested tree, checked before the merge
//! trailers are issued. An open review blocks either way; the latest verdict
//! on that attempt or tree must be a pass. A run in a `risk: high` module
//! needs both, and so does any run whose task matches a `gates.yaml`
//! override that asks for a person's review (FR-55). A design review
//! (spec 16.4) is advice the delegate asked for: it binds the attempt and,
//! while open, holds the task like any review, but no gate requires it.
//! Nothing passes on a timer.
use std::collections::BTreeMap;

use serde_json::Value;
use towermd_core::gates::GateFile;
use towermd_core::reviews::{self, Outcome, Review};

use crate::task_model::{Run, Task};

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

/// What `gates.yaml` overrides match a task on (FR-55): its layer (tasks
/// are L4), the modules it is in or touches, and, once a run bound it to a
/// baseline, its risk. Without a run the risk is unknown and every risk
/// override applies.
pub fn facets(task: &Task, run: Option<&Run>) -> BTreeMap<String, Vec<String>> {
    let modules = std::iter::once(task.input.module.clone()).chain(task.input.touches.iter().cloned()).collect();
    let mut facets = BTreeMap::from([("layer".to_owned(), vec!["L4".to_owned()]), ("module".to_owned(), modules)]);
    if let Some(run) = run {
        facets.insert("risk".into(), vec![if run.high_risk { "high" } else { "normal" }.into()]);
    }
    facets
}

/// Main's `gates.yaml`, under the overrides this run's task matches, asks
/// for a person's review at `stage`: a hard gate named after the review
/// gate (or added beside it), or any hard gate on the stage's review kind,
/// whose check is a person's review.
fn demanded(state: &Value, run: &Run, file: &GateFile, stage: &str, gate: &str) -> Result<bool, String> {
    let projection = crate::dev_state::tasks(state)?;
    let facets = match projection.tasks.get(&run.task) {
        Some(task) => facets(task, Some(run)),
        None => BTreeMap::new(),
    };
    let kind = reviews::check_kind(stage);
    Ok(file.scoped(&facets).gates.values().any(|declared| {
        let check = declared.check.as_deref().unwrap_or_default();
        declared.hard && declared.problem.is_none() && check.starts_with("human.review")
            && (declared.name == gate || declared.name.starts_with(&format!("{gate}/")) || check == kind)
    }))
}

/// G1 at finish: the plan review of this attempt.
pub fn g1(state: &Value, run: &Run, file: &GateFile) -> Result<(), String> {
    require(state, run, file, PLAN, "G1", |review| review.request.attempt.as_deref() == Some(run.binding.id.as_str()))
}

/// G4 before admission: the code review of the tested tree.
pub fn g4(state: &Value, run: &Run, file: &GateFile, tree: &str) -> Result<(), String> {
    require(state, run, file, CODE, "G4", |review| review.request.version.as_deref() == Some(tree))
}

fn require(state: &Value, run: &Run, file: &GateFile, stage: &str, gate: &str, bound: impl Fn(&Review) -> bool) -> Result<(), String> {
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
        None if demanded(state, run, file, stage, gate)? =>
            Err(format!("{gate}：gates.yaml 要求这项任务经人评审，先执行 towermd ask \"<摘要>\" --review {stage}")),
        // FR-53: a loaded skill's step maps to this review.
        None => match crate::skillpacks::gated(state, &run.binding.id, gate) {
            Some(skill) => Err(format!("{gate}：技能 {skill} 的交互点映射到这道评审（skillpacks.yaml），先执行 towermd ask \"<摘要>\" --review {stage}")),
            None => Ok(()),
        },
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

