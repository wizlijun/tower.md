//! A rebuildable scheduling hint, never permission to mutate a Hold. The sole
//! ticker reads only installed Human projection; the existing background worker
//! still proves actual sources and uses the durable producer.
use super::*;
use std::collections::BTreeSet;
use towermd_core::human_intervention::{HoldBody, HumanState};

const FAILURE_RETRY_MS: u64 = 5_000;
#[derive(Default)]
pub(super) struct AskTickSchedule {
    retry_pending: bool,
    retry_at_mono: Option<u64>,
    observed: BTreeSet<Observation>,
}
impl AskTickSchedule {
    pub(super) fn admitted(&self, hint: &Hint, mono_ms: u64) -> bool {
        (hint.due || self.retry_pending || !hint.observed.is_subset(&self.observed))
            && self.retry_at_mono.is_none_or(|at| mono_ms >= at)
    }
    pub(super) fn finished(&mut self, success: bool, mono_ms: u64, current: &Hint) {
        self.retry_pending = !success;
        self.retry_at_mono = (!success).then(|| mono_ms.saturating_add(FAILURE_RETRY_MS));
        if success {
            self.observed = current.observed.clone();
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Observation {
    Slice {
        hold: String,
        id: String,
        finished_ms: u64,
    },
    Attention {
        hold: String,
        request: String,
    },
}
#[derive(Default)]
pub(super) struct Hint {
    pub(super) due: bool,
    pub(super) present: bool,
    observed: BTreeSet<Observation>,
}
impl Hint {
    // Unknown installed state only schedules full background proof. It carries
    // no Hold identity, observation, authority or permission to append.
    pub(super) fn needs_proof() -> Self {
        Self {
            due: true,
            ..Self::default()
        }
    }
}
pub(super) fn hint(human: &HumanState, now: u64) -> Hint {
    let (_, canonical) = human.projection();
    let mut hint = Hint::default();
    for hold in human
        .holds
        .values()
        .filter(|h| matches!(&h.body, HoldBody::Ask { .. }))
    {
        let Some(opening) = canonical.openings.get(&hold.id) else {
            continue;
        };
        // Legacy/imported Holds cannot mint a new system identity. All fields
        // here came from installed actual outer projection, not a receipt role.
        if opening.stale
            || opening.seq <= canonical.invalidated_through
            || opening.obj != hold.id
            || opening.kind != "hold.opened"
            || opening.data["hold"]["body"]["kind"] != "ask"
            || opening.data["p3_request"]["request_id"]
                .as_str()
                .is_none_or(|id| {
                    towermd_core::recovery_contract::recorded::request_event_eid(id, "hold.opened")
                        .is_err()
                })
        {
            continue;
        }
        hint.present = true;
        let slice_due = hold
            .waits
            .iter()
            .any(|slice| slice.finished_ms.is_none() && now >= slice.deadline_ms);
        let attention_id = opening.data["hold"]["attention_request_id"].as_str();
        let attention_due = hold.reply.is_none()
            && !hold.attention
            && now >= hold.opened_ms.saturating_add(1_800_000)
            && attention_id.is_some_and(|id| {
                opening.data["p3_request"]["request_id"] != id
                    && towermd_core::recovery_contract::recorded::request_event_eid(
                        id,
                        "hold.attention",
                    )
                    .is_ok()
            });
        hint.due |= slice_due || attention_due;
        for slice in &hold.waits {
            if let Some(finished_ms) = slice.finished_ms {
                if towermd_core::recovery_contract::recorded::request_event_eid(
                    &slice.id,
                    "hold.wait_finished",
                )
                .is_ok()
                {
                    hint.observed.insert(Observation::Slice {
                        hold: hold.id.clone(),
                        id: slice.id.clone(),
                        finished_ms,
                    });
                }
            }
        }
        if hold.attention {
            if let Some(id) = attention_id.filter(|id| {
                towermd_core::recovery_contract::recorded::request_event_eid(id, "hold.attention")
                    .is_ok()
            }) {
                hint.observed.insert(Observation::Attention {
                    hold: hold.id.clone(),
                    request: id.into(),
                });
            }
        }
    }
    hint
}
// A finite read-only fallback for an already-admitted background worker. A
// complete confirmed row can still have a failed fsync in this process, so both
// pending and confirmed original records must retain a chance for core repair.
// These ordinary bytes neither prove acceptance nor authorize a transition.
pub(super) fn has_request_hint(inputs: &towermd_core::recovery_outbox::RecoveryInputs) -> bool {
    let finite = |kind: &str, data: &Value| {
        matches!(
            kind,
            "hold.wait_started" | "hold.wait_finished" | "hold.attention"
        ) && data["hold"]["body"]["kind"] == "ask"
    };
    inputs
        .pending()
        .iter()
        .any(|event| finite(&event.kind, &event.data))
        || inputs
            .confirmed()
            .iter()
            .any(|event| finite(&event.kind, &event.data))
}
pub(super) fn project_hint(project: &Project, now: u64) -> Result<Hint, String> {
    let live = project
        .hub
        .lock()
        .map_err(|_| "Ask hint Session lock poisoned")?;
    Ok(hint(live.human()?, now))
}
pub(super) fn finish(
    project: &Project,
    success: bool,
    mono_ms: u64,
    unix_ms: u64,
) -> Result<(), String> {
    // Caller still owns this finite producer. Newly accepted state can be
    // observed only after the same core tick has proved/durably confirmed it.
    let current = if success {
        project_hint(project, unix_ms)?
    } else {
        Hint::default()
    };
    project
        .ask_tick
        .lock()
        .map_err(|_| "Ask tick schedule lock poisoned")?
        .finished(success, mono_ms, &current);
    Ok(())
}

#[cfg(test)]
#[path = "ask_tick_tests.rs"]
mod tests;
