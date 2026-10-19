//! Read-only, observed task records. These DTOs grant no execution or human authority.
use crate::hub::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[path = "proposal_attention.rs"]
mod proposal_attention;
pub(crate) use proposal_attention::read_proposal_attention;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalAttentionState {
    ReviewPending,
}

/// A derived pending review item, never an approval or publication capability.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalAttention {
    pub state: ProposalAttentionState,
    pub binding: crate::baseline_review::ReviewBinding,
    pub created_seq: u64,
    pub created_at_ms: u64,
    pub reason: String,
    pub recommendation: String,
    pub impact: crate::proposals::ProposalImpact,
}

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
    #[serde(default)]
    pub proposals: Vec<ProposalAttention>,
    #[serde(default)]
    pub holds: Vec<Value>,
    /// Actual outer events only; recovered inner receipts are never flattened.
    #[serde(default)]
    pub events: Vec<Event>,
    /// Sequence immediately before the bounded event window, not the Hub floor.
    #[serde(default)]
    pub events_floor_seq: Option<u64>,
    pub diagnostics: Vec<String>,
}
