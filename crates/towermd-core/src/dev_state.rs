//! Development kit projection groups (moved to towermd-devkit later).
use serde_json::Value;

use crate::{kit::Kit, project_state};

pub struct DevKit;
pub const GROUPS: &[&str] = &["Baseline", "BaselineApproval", "Task", "Run", "Evidence"];
pub const KITS: &[&dyn Kit] = &[&DevKit];

impl Kit for DevKit {
    fn state_groups(&self) -> &'static [&'static str] {
        GROUPS
    }
}

pub fn initial() -> Value {
    project_state::initial_with(KITS)
}
