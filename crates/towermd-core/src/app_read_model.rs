//! Read-only, observed task records. These DTOs grant no execution or human authority.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskProgressState {
    Recorded,
    RecoveryRequired,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskProgressSource {
    HubJournal,
    RecoverySnapshot { commit: String, reference: String },
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskProgressView {
    pub project_id: String,
    pub common_dir: String,
    pub revision: String,
    pub captured_at: u64,
    pub through_seq: Option<u64>,
    pub floor_seq: Option<u64>,
    pub state: TaskProgressState,
    pub source: TaskProgressSource,
    pub tasks: Vec<Value>,
    pub runs: Vec<Value>,
    pub evidence: Vec<Value>,
    pub diagnostics: Vec<String>,
}
