//! Compile-time extension seam. A kit adds projection groups and business
//! reducers on top of the core; the core never names a kit's concepts.
use serde_json::Value;

use crate::{hub::Event, question_model::RunContext};

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
}
