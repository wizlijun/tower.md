pub use crate::hub::{AppendReceipt, Event, Fence, Lease, SnapshotAck};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewEvent {
    pub eid: String,
    pub host: String,
    pub actor: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub layer: String,
    pub obj: String,
    pub data: Value,
    pub guards: Vec<Fence>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HubHead {
    pub floor_seq: u64,
    pub contiguous_seq: u64,
    pub epoch_high_water: u64,
    pub ack: Option<SnapshotAck>,
    pub leases: BTreeMap<String, Lease>,
    /// The current pause original, so a remote reader sees the same gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_event: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionBatch {
    pub events: Vec<Event>,
    pub head: HubHead,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HubError {
    Invalid(String),
    LeaseConflict { owners: Vec<Lease> },
    FenceExpired { fences: Vec<Fence> },
    SnapshotRequired { head: HubHead, checkpoint: Value },
    Uncertain(String),
    Io(String),
    /// The hub could not be reached. The request may or may not have landed;
    /// only an idempotent retry with the same eid settles it (FR-88).
    Offline(String),
}
impl std::fmt::Display for HubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) | Self::Io(s) | Self::Uncertain(s) | Self::Offline(s) => {
                f.write_str(s)
            }
            Self::LeaseConflict { owners } => write!(
                f,
                "lease occupied: {}",
                serde_json::to_string(owners).unwrap_or_default()
            ),
            Self::FenceExpired { fences } => write!(f, "lease fence expired: {fences:?}"),
            Self::SnapshotRequired { head, .. } => write!(
                f,
                "SnapshotRequired: cursor precedes floor {}",
                head.floor_seq
            ),
        }
    }
}
impl std::error::Error for HubError {}
impl From<String> for HubError {
    fn from(s: String) -> Self {
        Self::Invalid(s)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcquireLease {
    pub host: String,
    pub names: Vec<String>,
    pub holder: String,
    pub ttl_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenewLease {
    pub host: String,
    pub fences: Vec<Fence>,
    pub ttl_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReleaseLease {
    pub host: String,
    pub fences: Vec<Fence>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LeaseReceipt {
    pub event: Event,
    pub leases: Vec<Lease>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RebuildSnapshot {
    pub ack: SnapshotAck,
    pub checkpoint: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RebuildSeed {
    pub source_seq: u64,
    pub source_epoch: u64,
    pub seq_boundary: u64,
    pub epoch_boundary: u64,
    pub checkpoint: Value,
    pub receipts: BTreeMap<String, u64>,
    pub tail: Vec<Event>,
    /// Derived from checked canonical originals, never from a bare projection.
    #[serde(default)]
    pub pause_event: Option<String>,
    /// Compact to this acknowledged checkpoint in the same durable install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_snapshot: Option<RebuildSnapshot>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RebuildReceipt {
    pub contiguous_seq: u64,
    pub epoch_high_water: u64,
}
pub trait HubProtocol: Send {
    fn append(&mut self, event: NewEvent, now_ms: u64) -> Result<AppendReceipt, HubError>;
    fn subscribe(&mut self, after: u64, now_ms: u64) -> Result<SubscriptionBatch, HubError>;
    fn lease_acquire(
        &mut self,
        request: AcquireLease,
        now_ms: u64,
    ) -> Result<LeaseReceipt, HubError>;
    fn lease_renew(&mut self, request: RenewLease, now_ms: u64) -> Result<LeaseReceipt, HubError>;
    fn lease_release(
        &mut self,
        request: ReleaseLease,
        now_ms: u64,
    ) -> Result<LeaseReceipt, HubError>;
    fn snapshot_ack(&mut self, ack: SnapshotAck, checkpoint: Value) -> Result<(), HubError>;
    fn rebuild(&mut self, seed: RebuildSeed) -> Result<RebuildReceipt, HubError>;
    fn contiguous_seq(&self) -> u64;
}

/// What the project runtime needs from a hub beyond the seven operations: a
/// read view of the retained events and the hub's own head. The local hub
/// reads its journal; a remote hub serves a mirror refreshed by `subscribe`.
/// The runtime's ordering and lease logic cannot tell the two apart.
pub trait HubBackend: HubProtocol {
    /// Retained events after `after`, in sequence order.
    fn events(&self, after: u64) -> Vec<Event>;
    /// Leases still live at `now`.
    fn leases(&self, now: u64) -> BTreeMap<String, Lease>;
    fn epoch_high_water(&self) -> u64;
    fn acked_snapshot(&self) -> Option<SnapshotAck>;
    fn pause_event(&self) -> Option<String>;
    /// A remote hub may be unreachable; the runtime then queues ordinary
    /// events and refuses lease operations instead of sequencing locally.
    fn is_remote(&self) -> bool {
        false
    }
}
