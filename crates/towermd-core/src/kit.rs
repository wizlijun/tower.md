//! Compile-time extension seam. A kit adds projection groups and business
//! reducers on top of the core; the core never names a kit's concepts.
use std::path::Path;

use serde_json::Value;

use crate::{hub::Event, project_runtime::Reducer, question_model::RunContext};

/// Error when no kit can prove a protected commit (v0.1.1 wording).
pub const NO_PROOF: &str = "受保护提交缺少可证明的出版事件";

/// A run a kit tracks, as seen by core prompt delivery.
pub struct KitRun {
    pub context: RunContext,
    pub active: bool,
}

pub trait Kit: Sync {
    /// Top-level projection groups this kit owns, inserted before core groups.
    fn state_groups(&self) -> &'static [&'static str] {
        &[]
    }
    /// Applies an event the core does not own. `Ok(false)` means not handled.
    fn reduce(&self, _state: &mut Value, _event: &Event) -> Result<bool, String> {
        Ok(false)
    }
    /// Runs that can receive core control prompts.
    fn runs(&self, _state: &Value) -> Result<Vec<KitRun>, String> {
        Ok(Vec::new())
    }
    /// Proves that a protected main commit was published by this kit.
    fn prove_protected_commit(
        &self,
        _root: &Path,
        _commit: &str,
        _project_id: &str,
        _receipt: &dyn Fn(&str) -> Option<Event>,
    ) -> Result<(), String> {
        Err(NO_PROOF.into())
    }
}

/// First successful kit proof wins; otherwise the last kit's error.
pub fn prove_by_kits(
    kits: &[&dyn Kit],
    root: &Path,
    commit: &str,
    project_id: &str,
    receipt: &dyn Fn(&str) -> Option<Event>,
) -> Result<(), String> {
    let mut last = Err(NO_PROOF.to_owned());
    for kit in kits {
        last = kit.prove_protected_commit(root, commit, project_id, receipt);
        if last.is_ok() {
            return last;
        }
    }
    last
}

/// A runtime assembly: the kits and the reducer built from exactly those kits.
pub struct Assembly {
    pub kits: &'static [&'static dyn Kit],
    pub reduce: Reducer,
}

impl Assembly {
    pub fn initial(&self) -> Value {
        crate::project_state::initial_with(self.kits)
    }
}
