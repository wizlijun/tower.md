//! FR-160/161: callouts beside a frame's cards, derived on every read from
//! answers, prompts and open questions that name the frame, one of its free
//! cards or an object it shows. Nothing is stored; the App draws them next to
//! the card and never lets them take part in layout.
use serde::{Deserialize, Serialize};

use crate::{frames::Frame, question_model::{ChangeTarget, QuestionProjection}, tower_api};

/// How long one callout's text may be.
pub const TEXT_CHARS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An answer a person remembered as a decision: why it is this way.
    Why,
    /// An answer without a decision, or a prompt: how it was adjusted.
    Adjustment,
    /// A question still waiting for its answer.
    Question,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Callout {
    pub kind: Kind,
    /// The frame, one of its cards or a bound object.
    pub target: String,
    pub text: String,
    /// The question or prompt it comes from.
    pub source: String,
}

/// The callouts for `frame`, whose bound cards are `bound`; none when the
/// frame's rules turn them off.
pub fn for_frame(questions: &QuestionProjection, frame: &Frame, bound: &[String]) -> Vec<Callout> {
    if frame.rules.callouts_off {
        return Vec::new();
    }
    let shown = |id: &str| id == frame.id || bound.iter().any(|object| object == id)
        || frame.cards.iter().any(|card| card.id == id && !card.archived);
    let mut out = Vec::new();
    let mut push = |kind: Kind, targets: &mut dyn Iterator<Item = &ChangeTarget>, text: &str, source: &str| {
        let mut seen: Vec<&str> = Vec::new();
        for target in targets {
            if shown(&target.id) && !seen.contains(&target.id.as_str()) {
                seen.push(&target.id);
                out.push(Callout { kind, target: target.id.clone(), text: tower_api::clip(text, TEXT_CHARS), source: source.into() });
            }
        }
    };
    for (id, question) in &questions.holds {
        match questions.answers.get(id).filter(|answer| answer.effective()) {
            None => push(Kind::Question, &mut question.targets.iter(), &question.draft.text, id),
            Some(answer) => {
                let mut targets = question.targets.iter().chain(&answer.input.targets);
                match &answer.decision {
                    Some(decision) => push(Kind::Why, &mut targets, &decision.decision, id),
                    None => push(Kind::Adjustment, &mut targets, &answer.input.text, id),
                }
            }
        }
    }
    for (id, prompt) in &questions.prompts {
        push(Kind::Adjustment, &mut prompt.targets.iter(), &prompt.text, id);
    }
    out
}
