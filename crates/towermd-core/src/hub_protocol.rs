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
}
impl std::fmt::Display for HubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) | Self::Io(s) | Self::Uncertain(s) => f.write_str(s),
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
pub struct RebuildSeed {
    pub source_seq: u64,
    pub source_epoch: u64,
    pub seq_boundary: u64,
    pub epoch_boundary: u64,
    pub checkpoint: Value,
    pub receipts: BTreeMap<String, u64>,
    pub tail: Vec<Event>,
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
