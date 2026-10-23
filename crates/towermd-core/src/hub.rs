use crate::hub_journal::{Header, Journal};
use crate::hub_protocol::*;
use chrono::{DateTime, SecondsFormat, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::path::Path;
use uuid::{Uuid, Variant};

pub use crate::hub_protocol::NewEvent;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fence {
    pub name: String,
    pub holder: String,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    pub name: String,
    pub holder: String,
    pub epoch: u64,
    pub expires_at: u64,
}
impl Lease {
    pub fn fence(&self) -> Fence {
        Fence {
            name: self.name.clone(),
            holder: self.holder.clone(),
            epoch: self.epoch,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotAck {
    pub commit: String,
    pub seq: u64,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AppendReceipt {
    Event(Box<Event>),
    Duplicate { seq: u64 },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum SystemDelta {
    Grant { leases: Vec<Lease> },
    Renew { leases: Vec<Lease> },
    Release { fences: Vec<Fence> },
    Expire { fences: Vec<Fence> },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub eid: String,
    pub host: String,
    pub t: String,
    pub actor: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub layer: String,
    pub obj: String,
    pub data: Value,
    pub epoch: Option<u64>,
    pub stale: bool,
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub guards: Vec<Fence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lease_delta: Option<SystemDelta>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) heartbeat: bool,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}
impl Event {
    pub fn acquired_leases(&self) -> &[Lease] {
        match &self.lease_delta {
            Some(SystemDelta::Grant { leases }) => leases,
            _ => &[],
        }
    }
    pub fn updated_leases(&self) -> &[Lease] {
        match &self.lease_delta {
            Some(SystemDelta::Grant { leases } | SystemDelta::Renew { leases }) => leases,
            _ => &[],
        }
    }
    pub fn released_lease_names(&self) -> Vec<String> {
        match &self.lease_delta {
            Some(SystemDelta::Release { fences } | SystemDelta::Expire { fences }) => {
                fences.iter().map(|f| f.name.clone()).collect()
            }
            _ => vec![],
        }
    }
    pub fn released_fences(&self) -> &[Fence] {
        match &self.lease_delta {
            Some(SystemDelta::Release { fences } | SystemDelta::Expire { fences }) => fences,
            _ => &[],
        }
    }
    pub fn is_pure_heartbeat(&self) -> bool {
        self.heartbeat
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SystemCheckpoint {
    pub floor_seq: u64,
    pub epoch_high_water: u64,
    #[serde(deserialize_with = "crate::hub_journal::unique_map")]
    pub leases: BTreeMap<String, Lease>,
    #[serde(deserialize_with = "crate::hub_journal::unique_map")]
    pub receipts: BTreeMap<String, u64>,
    pub ack: Option<SnapshotAck>,
    pub checkpoint: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_event: Option<String>,
}
impl Default for SystemCheckpoint {
    fn default() -> Self {
        Self {
            floor_seq: 0,
            epoch_high_water: 0,
            leases: BTreeMap::new(),
            receipts: BTreeMap::new(),
            ack: None,
            checkpoint: serde_json::json!({}),
            pause_event: None,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub(crate) struct SystemState {
    pub leases: BTreeMap<String, Lease>,
    pub epoch: u64,
    pub deadlines: BTreeSet<(u64, String, u64)>,
    pub pause_event: Option<String>,
}
impl SystemState {
    pub fn from_checkpoint(base: &SystemCheckpoint) -> Result<Self, HubError> {
        let mut state = Self {
            leases: base.leases.clone(),
            epoch: base.epoch_high_water,
            deadlines: BTreeSet::new(),
            pause_event: base.pause_event.clone(),
        };
        if let Some(eid)=&state.pause_event {
            validate_eid(eid)?;
            if !base.receipts.contains_key(eid) {return Err(invalid("pause checkpoint lacks original receipt"));}
        }
        for (name, lease) in &state.leases {
            validate_fence(&lease.fence())?;
            timestamp(lease.expires_at)?;
            if *name != lease.name || lease.epoch > state.epoch {
                return Err(invalid("invalid checkpoint lease"));
            }
            state
                .deadlines
                .insert((lease.expires_at, name.clone(), lease.epoch));
        }
        Ok(state)
    }
    pub(crate) fn active(&self, fence: &Fence, now: u64) -> bool {
        self.leases
            .get(&fence.name)
            .is_some_and(|l| l.fence() == *fence && l.expires_at > now)
    }
    pub(crate) fn validate_fences(&self, fences: &[Fence], now: u64) -> Result<(), HubError> {
        validate_names(fences.iter().map(|f| f.name.as_str()))?;
        for fence in fences {
            validate_fence(fence)?;
        }
        let expired = fences
            .iter()
            .filter(|f| !self.active(f, now))
            .cloned()
            .collect::<Vec<_>>();
        if !expired.is_empty() {
            return Err(HubError::FenceExpired { fences: expired });
        }
        Ok(())
    }
    fn apply(&mut self, event: &Event) {
        if event.stale {
            return;
        }
        match event.kind.as_str() {
            "project.paused"=>self.pause_event=Some(event.eid.clone()),
            "project.resumed"=>self.pause_event=None,
            _=>{},
        }
        match &event.lease_delta {
            Some(SystemDelta::Grant { leases } | SystemDelta::Renew { leases }) => {
                for lease in leases {
                    if let Some(old) = self.leases.insert(lease.name.clone(), lease.clone()) {
                        self.deadlines
                            .remove(&(old.expires_at, old.name, old.epoch));
                    }
                    self.epoch = self.epoch.max(lease.epoch);
                    self.deadlines
                        .insert((lease.expires_at, lease.name.clone(), lease.epoch));
                }
            }
            Some(SystemDelta::Release { fences } | SystemDelta::Expire { fences }) => {
                for fence in fences {
                    if self
                        .leases
                        .get(&fence.name)
                        .is_some_and(|l| l.fence() == *fence)
                    {
                        let lease = self.leases.remove(&fence.name).unwrap();
                        self.deadlines
                            .remove(&(lease.expires_at, lease.name, lease.epoch));
                    }
                }
            }
            None => {}
        }
    }
    pub(crate) fn replay(&mut self, event: &Event) -> Result<(), HubError> {
        let now = event_time(event)?;
        validate_record(event)?;
        validate_control(self.pause_event.as_deref(),event)?;
        if self.pause_event.is_some() && event.kind=="task.claimed" && !event.stale {
            return Err(invalid("project pause forbids new task claims"));
        }
        {
            if event.lease_delta.is_none()
                && (event.epoch != event.guards.first().map(|f| f.epoch)
                    || event.stale != event.guards.iter().any(|f| !self.active(f, now)))
            {
                return Err(invalid("inconsistent journal epoch or stale fence"));
            }
            if let Some(delta) = &event.lease_delta {
                if event.actor != "process:towermd-hub"
                    || event.layer != "L5"
                    || event.stale
                    || !event.guards.is_empty()
                {
                    return Err(invalid("invalid system lease event envelope"));
                }
                let (kind, primary) = match delta {
                    SystemDelta::Grant { leases } => {
                        if self.pause_event.is_some() && leases.iter().any(|lease|is_work_lease(&lease.name)) {
                            return Err(invalid("project pause forbids new work leases"));
                        }
                        validate_names(leases.iter().map(|l| l.name.as_str()))?;
                        let mut epoch = self.epoch;
                        let mut holder = None;
                        for lease in leases {
                            validate_fence(&lease.fence())?;
                            timestamp(lease.expires_at)?;
                            epoch = epoch
                                .checked_add(1)
                                .ok_or_else(|| invalid("epoch overflow"))?;
                            if lease.epoch != epoch
                                || lease.expires_at <= now
                                || self
                                    .leases
                                    .get(&lease.name)
                                    .is_some_and(|l| l.expires_at > now)
                                || holder.is_some_and(|h| h != &lease.holder)
                            {
                                return Err(invalid("invalid lease grant"));
                            }
                            holder = Some(&lease.holder);
                        }
                        ("lease.granted", leases[0].epoch)
                    }
                    SystemDelta::Renew { leases } => {
                        validate_names(leases.iter().map(|l| l.name.as_str()))?;
                        for lease in leases {
                            if !self.active(&lease.fence(), now) || lease.expires_at <= now {
                                return Err(invalid("invalid lease renewal"));
                            }
                            timestamp(lease.expires_at)?;
                        }
                        ("lease.renewed", leases[0].epoch)
                    }
                    SystemDelta::Release { fences } => {
                        self.validate_fences(fences, now)?;
                        ("lease.released", fences[0].epoch)
                    }
                    SystemDelta::Expire { fences } => {
                        validate_names(fences.iter().map(|f| f.name.as_str()))?;
                        for fence in fences {
                            if !self
                                .leases
                                .get(&fence.name)
                                .is_some_and(|l| l.fence() == *fence && l.expires_at <= now)
                            {
                                return Err(invalid("invalid lease expiration"));
                            }
                        }
                        ("lease.expired", fences[0].epoch)
                    }
                };
                if event.kind != kind
                    || event.epoch != Some(primary)
                    || event.heartbeat != matches!(delta, SystemDelta::Renew { .. })
                {
                    return Err(invalid("forged system event metadata"));
                }
            } else if event.kind.starts_with("lease.") || event.heartbeat {
                return Err(invalid("reserved system metadata on client event"));
            }
        }
        self.apply(event);
        Ok(())
    }
}
pub struct Hub {
    _lock: File,
    journal: Journal,
    events: Vec<Event>,
    by_eid: BTreeMap<String, u64>,
    state: SystemState,
    base: SystemCheckpoint,
}
impl Drop for Hub {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._lock);
    }
}
impl Hub {
    pub fn open(dir: &Path) -> Result<Self, String> {
        Self::open_protocol(dir, true).map_err(|e| e.to_string())
    }
    /// Open an already existing Hub without creating its directory, lock or journal.
    pub fn open_existing(dir: &Path) -> Result<Self, String> {
        Self::open_protocol(dir, false).map_err(|e| e.to_string())
    }
    fn open_protocol(dir: &Path, create: bool) -> Result<Self, HubError> {
        if create {
            fs::create_dir_all(dir).map_err(|e| HubError::Io(e.to_string()))?;
        }
        let dir = fs::canonicalize(dir).map_err(|e| HubError::Io(e.to_string()))?;
        let lock = OpenOptions::new()
            .create(create)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("hub.lock"))
            .map_err(|e| HubError::Io(e.to_string()))?;
        FileExt::try_lock_exclusive(&lock)
            .map_err(|e| HubError::Io(format!("hub already locked or lock unavailable: {e}")))?;
        let (journal, header, events) = if create {
            Journal::open(&dir)?
        } else {
            Journal::open_existing(&dir)?
        };
        let base = header.checkpoint;
        let (state, by_eid) = replay_journal(&base, &events)?;
        Ok(Self {
            _lock: lock,
            journal,
            events,
            by_eid,
            state,
            base,
        })
    }
    fn head(&self) -> HubHead {
        HubHead {
            floor_seq: self.base.floor_seq,
            contiguous_seq: self.last_seq(),
            epoch_high_water: self.state.epoch,
            ack: self.base.ack.clone(),
            leases: self.state.leases.clone(),
            pause_event: self.state.pause_event.clone(),
        }
    }
    fn commit(&mut self, event: Event) -> Result<Event, HubError> {
        self.journal.append(&event)?;
        self.state.apply(&event);
        self.by_eid.insert(event.eid.clone(), event.seq);
        self.events.push(event.clone());
        Ok(event)
    }
    fn event(
        &self,
        input: crate::hub_protocol::NewEvent,
        now: u64,
        delta: Option<SystemDelta>,
    ) -> Result<Event, HubError> {
        validate_input(&input)?;
        let epoch = input.guards.first().map(|f| f.epoch);
        let stale = input.guards.iter().any(|f| !self.state.active(f, now));
        let mut event = Event {
            seq: self
                .last_seq()
                .checked_add(1)
                .ok_or_else(|| invalid("sequence overflow"))?,
            eid: input.eid,
            host: input.host,
            t: timestamp(now)?.to_rfc3339_opts(SecondsFormat::Millis, true),
            actor: input.actor,
            kind: input.kind,
            layer: input.layer,
            obj: input.obj,
            data: input.data,
            epoch,
            stale,
            cost_usd: None,
            guards: input.guards,
            lease_delta: delta,
            heartbeat: false,
            extra: BTreeMap::new(),
        };
        if let Some(delta) = &event.lease_delta {
            event.epoch = Some(match delta {
                SystemDelta::Grant { leases } | SystemDelta::Renew { leases } => leases[0].epoch,
                SystemDelta::Release { fences } | SystemDelta::Expire { fences } => fences[0].epoch,
            });
            event.heartbeat = matches!(delta, SystemDelta::Renew { .. });
        }
        Ok(event)
    }
    fn system(
        &mut self,
        host: String,
        kind: &str,
        delta: SystemDelta,
        now: u64,
    ) -> Result<LeaseReceipt, HubError> {
        let obj = match &delta {
            SystemDelta::Grant { leases } | SystemDelta::Renew { leases } => leases[0].name.clone(),
            SystemDelta::Release { fences } | SystemDelta::Expire { fences } => {
                fences[0].name.clone()
            }
        };
        let input = crate::hub_protocol::NewEvent {
            eid: Uuid::now_v7().to_string(),
            host,
            actor: "process:towermd-hub".into(),
            kind: kind.into(),
            layer: "L5".into(),
            obj,
            data: Value::Null,
            guards: vec![],
        };
        let event = self.event(input, now, Some(delta))?;
        let leases = event.updated_leases().to_vec();
        Ok(LeaseReceipt {
            event: self.commit(event)?,
            leases,
        })
    }
    fn expire(&mut self, now: u64) -> Result<(), HubError> {
        self.journal.certain()?;
        timestamp(now)?;
        let fences = self
            .state
            .deadlines
            .iter()
            .take_while(|(deadline, _, _)| *deadline <= now)
            .map(|(_, name, _)| self.state.leases[name].fence())
            .collect::<Vec<_>>();
        if !fences.is_empty() {
            self.system(
                "local".into(),
                "lease.expired",
                SystemDelta::Expire { fences },
                now,
            )?;
        }
        Ok(())
    }
    pub fn events(&self, after: u64) -> Vec<Event> {
        self.events[self.events.partition_point(|e| e.seq <= after)..].to_vec()
    }
    pub fn pause_event(&self)->Option<&str> {self.state.pause_event.as_deref()}
    pub fn leases(&self, now: u64) -> BTreeMap<String, Lease> {
        self.state
            .leases
            .iter()
            .filter(|(_, l)| l.expires_at > now)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    pub fn last_seq(&self) -> u64 {
        self.events.last().map_or(self.base.floor_seq, |e| e.seq)
    }
    pub fn epoch_high_water(&self) -> u64 {
        self.state.epoch
    }
    pub fn snapshot_ack(&self) -> Option<&SnapshotAck> {
        self.base.ack.as_ref()
    }
    pub fn receipts(&self) -> BTreeMap<String, u64> {
        let mut receipts = self.base.receipts.clone();
        receipts.extend(
            self.events
                .iter()
                .filter(|e| !e.is_pure_heartbeat())
                .map(|e| (e.eid.clone(), e.seq)),
        );
        receipts
    }
}
impl Hub {
    fn rebuild_with_sync(
        &mut self,
        seed: RebuildSeed,
        sync: impl FnOnce(&File) -> std::io::Result<()>,
    ) -> Result<RebuildReceipt, HubError> {
        self.journal.certain()?;
        let min_seq = seed
            .source_seq
            .checked_add(1 << 32)
            .ok_or_else(|| invalid("recovery sequence overflow"))?;
        let min_epoch = seed
            .source_epoch
            .checked_add(1 << 32)
            .ok_or_else(|| invalid("recovery epoch overflow"))?;
        if seed.seq_boundary < min_seq
            || seed.epoch_boundary < min_epoch
            || seed.seq_boundary <= self.last_seq()
            || seed.epoch_boundary <= self.state.epoch
        {
            return Err(invalid(
                "recovery boundaries must advance and skip at least 2^32",
            ));
        }
        if seed.receipts.values().any(|seq| *seq > seed.source_seq) {
            return Err(invalid("recovery receipts exceed source sequence"));
        }
        let mut base = SystemCheckpoint {
            floor_seq: seed.seq_boundary,
            epoch_high_water: seed.epoch_boundary,
            leases: BTreeMap::new(),
            receipts: seed.receipts,
            ack: None,
            checkpoint: seed.checkpoint,
            pause_event: seed.pause_event,
        };
        validate_checkpoint(&base)?;
        let mut state = SystemState::from_checkpoint(&base)?;
        let mut receipts = base.receipts.clone();
        let mut seq = base.floor_seq;
        for event in &seed.tail {
            seq = seq
                .checked_add(1)
                .ok_or_else(|| invalid("sequence overflow"))?;
            if event.seq != seq || receipts.insert(event.eid.clone(), event.seq).is_some() {
                return Err(invalid("invalid rebuild tail sequence or eid"));
            }
            state.replay(event)?;
        }
        let mut tail = seed.tail;
        if let Some(snapshot) = seed.final_snapshot {
            validate_ack(&snapshot.ack, seq, state.epoch)?;
            base = compact_checkpoint(&base, &tail, snapshot.ack, snapshot.checkpoint)?;
            let offset = tail.partition_point(|event| event.seq <= base.floor_seq);
            tail.drain(..offset);
            receipts = base.receipts.clone();
            receipts.extend(tail.iter().map(|event| (event.eid.clone(), event.seq)));
        }
        self.journal.replace_with_sync(
            &Header {
                version: 2,
                checkpoint: base.clone(),
            },
            &tail,
            sync,
        )?;
        self.base = base;
        self.state = state;
        self.events = tail;
        self.by_eid = receipts;
        Ok(RebuildReceipt {
            contiguous_seq: seq,
            epoch_high_water: self.state.epoch,
        })
    }
}

impl HubBackend for Hub {
    fn events(&self, after: u64) -> Vec<Event> {
        Hub::events(self, after)
    }
    fn leases(&self, now: u64) -> BTreeMap<String, Lease> {
        Hub::leases(self, now)
    }
    fn epoch_high_water(&self) -> u64 {
        Hub::epoch_high_water(self)
    }
    fn acked_snapshot(&self) -> Option<SnapshotAck> {
        Hub::snapshot_ack(self).cloned()
    }
    fn pause_event(&self) -> Option<String> {
        Hub::pause_event(self).map(str::to_owned)
    }
}

impl HubProtocol for Hub {
    fn append(
        &mut self,
        input: crate::hub_protocol::NewEvent,
        now: u64,
    ) -> Result<AppendReceipt, HubError> {
        self.journal.certain()?;
        if let Some(seq) = self.by_eid.get(&input.eid) {
            return Ok(match self.events.binary_search_by_key(seq, |e| e.seq) {
                Ok(i) => AppendReceipt::Event(Box::new(self.events[i].clone())),
                Err(_) => AppendReceipt::Duplicate { seq: *seq },
            });
        }
        validate_input(&input)?;
        if input.kind.starts_with("lease.") {
            return Err(invalid("reserved lease event type"));
        }
        if self.state.pause_event.is_some() && input.kind=="task.claimed" {
            return Err(invalid("项目已暂停，新任务领取被拒绝"));
        }
        let event = self.event(input, now, None)?;
        validate_control(self.state.pause_event.as_deref(),&event)?;
        Ok(AppendReceipt::Event(Box::new(self.commit(event)?)))
    }
    fn subscribe(&mut self, after: u64, now: u64) -> Result<SubscriptionBatch, HubError> {
        self.journal.certain()?;
        if after < self.base.floor_seq {
            return Err(HubError::SnapshotRequired {
                head: self.head(),
                checkpoint: self.base.checkpoint.clone(),
            });
        }
        if after > self.last_seq() {
            return Err(invalid("subscription cursor exceeds contiguous sequence"));
        }
        self.expire(now)?;
        Ok(SubscriptionBatch {
            events: self.events(after),
            head: self.head(),
        })
    }
    fn lease_acquire(&mut self, r: AcquireLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.journal.certain()?;
        validate_token("host", &r.host)?;
        validate_names(r.names.iter().map(String::as_str))?;
        if self.state.pause_event.is_some() && r.names.iter().any(|name|is_work_lease(name)) {
            return Err(invalid("项目已暂停，新任务领取和工作租约申请被拒绝"));
        }
        validate_token("lease holder", &r.holder)?;
        let expiry = expiration(now, r.ttl_ms)?;
        let owners = r
            .names
            .iter()
            .filter_map(|n| {
                self.state
                    .leases
                    .get(n)
                    .filter(|l| l.expires_at > now)
                    .cloned()
            })
            .collect::<Vec<_>>();
        if !owners.is_empty() {
            return Err(HubError::LeaseConflict { owners });
        }
        let mut epoch = self.state.epoch;
        let mut leases = vec![];
        for name in r.names {
            epoch = epoch
                .checked_add(1)
                .ok_or_else(|| invalid("epoch overflow"))?;
            leases.push(Lease {
                name,
                holder: r.holder.clone(),
                epoch,
                expires_at: expiry,
            })
        }
        self.system(r.host, "lease.granted", SystemDelta::Grant { leases }, now)
    }
    fn lease_renew(&mut self, r: RenewLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.journal.certain()?;
        validate_token("host", &r.host)?;
        self.state.validate_fences(&r.fences, now)?;
        let expiry = expiration(now, r.ttl_ms)?;
        let leases = r
            .fences
            .into_iter()
            .map(|f| Lease {
                name: f.name,
                holder: f.holder,
                epoch: f.epoch,
                expires_at: expiry,
            })
            .collect();
        self.system(r.host, "lease.renewed", SystemDelta::Renew { leases }, now)
    }
    fn lease_release(&mut self, r: ReleaseLease, now: u64) -> Result<LeaseReceipt, HubError> {
        self.journal.certain()?;
        validate_token("host", &r.host)?;
        self.state.validate_fences(&r.fences, now)?;
        self.system(
            r.host,
            "lease.released",
            SystemDelta::Release { fences: r.fences },
            now,
        )
    }
    fn snapshot_ack(&mut self, ack: SnapshotAck, checkpoint: Value) -> Result<(), HubError> {
        self.journal.certain()?;
        validate_ack(&ack, self.last_seq(), self.state.epoch)?;
        if ack.seq < self.base.floor_seq {
            return Err(invalid(
                "snapshot acknowledgement precedes checkpoint floor",
            ));
        }
        if let Some(previous) = &self.base.ack {
            if ack.seq < previous.seq {
                return Err(invalid(
                    "snapshot acknowledgement cannot move sequence backwards",
                ));
            }
            if ack.seq == previous.seq && self.base.floor_seq == ack.seq {
                if ack.epoch != previous.epoch || checkpoint != self.base.checkpoint {
                    return Err(invalid(
                        "same sequence requires the same epoch and checkpoint",
                    ));
                }
                if *previous == ack {
                    return Ok(());
                }
            }
        }
        let base = compact_checkpoint(&self.base, &self.events, ack, checkpoint)?;
        let offset = self.events.partition_point(|e| e.seq <= base.floor_seq);
        self.journal.replace(
            &Header {
                version: 2,
                checkpoint: base.clone(),
            },
            &self.events[offset..],
        )?;
        self.events.drain(..offset);
        self.by_eid = base.receipts.clone();
        self.by_eid
            .extend(self.events.iter().map(|e| (e.eid.clone(), e.seq)));
        self.base = base;
        Ok(())
    }
    fn rebuild(&mut self, seed: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        self.rebuild_with_sync(seed, File::sync_all)
    }
    fn contiguous_seq(&self) -> u64 {
        self.last_seq()
    }
}
fn replay_journal(
    base: &SystemCheckpoint,
    events: &[Event],
) -> Result<(SystemState, BTreeMap<String, u64>), HubError> {
    validate_checkpoint(base)?;
    let mut state = SystemState::from_checkpoint(base)?;
    let mut by_eid = base.receipts.clone();
    let mut seq = base.floor_seq;
    for event in events {
        seq = seq
            .checked_add(1)
            .ok_or_else(|| invalid("sequence overflow"))?;
        if event.seq != seq || by_eid.contains_key(&event.eid) {
            return Err(invalid("invalid hub journal sequence or duplicate eid"));
        }
        state.replay(event)?;
        by_eid.insert(event.eid.clone(), event.seq);
    }
    if let Some(ack) = &base.ack {
        validate_ack(ack, seq, state.epoch)?;
    }
    Ok((state, by_eid))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreState {
    Missing,
    Empty,
    Nonempty,
}

/// Recorded protocol bytes only. This cannot be deserialized into a writer or lease capability.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckedJournalRead {
    checkpoint: SystemCheckpoint,
    events: Vec<Event>,
}
impl CheckedJournalRead {
    pub(crate) fn empty(&self) -> bool {
        let base = &self.checkpoint;
        self.events.is_empty()
            && base.floor_seq == 0
            && base.epoch_high_water == 0
            && base.leases.is_empty()
            && base.receipts.is_empty()
            && base.ack.is_none()
            && (base.checkpoint == serde_json::json!({}) || base.checkpoint.is_null())
    }
}
/// No lock creation, migration, repair, expiry, or Hub construction.
pub(crate) fn read_checked_journal(dir: &Path) -> Result<Option<CheckedJournalRead>, String> {
    let Some((header, events)) = crate::hub_journal::read_only(dir).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let base = header.checkpoint;
    replay_journal(&base, &events).map_err(|e| e.to_string())?;
    Ok(Some(CheckedJournalRead {
        checkpoint: base,
        events,
    }))
}
/// Validate recorded bytes without locks, migration, tail repair, expiry or creation.
pub fn inspect_store(dir: &Path) -> Result<StoreState, String> {
    Ok(match read_checked_journal(dir)? {
        None => StoreState::Missing,
        Some(recording) if recording.empty() => StoreState::Empty,
        Some(_) => StoreState::Nonempty,
    })
}

// The checkpoint value is opaque; only protocol envelopes and system deltas
// determine the compacted leases, receipts and sequence/epoch bounds.
fn compact_checkpoint(
    base: &SystemCheckpoint,
    events: &[Event],
    ack: SnapshotAck,
    checkpoint: Value,
) -> Result<SystemCheckpoint, HubError> {
    if ack.seq < base.floor_seq {
        return Err(invalid(
            "snapshot acknowledgement precedes checkpoint floor",
        ));
    }
    let mut state = SystemState::from_checkpoint(base)?;
    let mut receipts = base.receipts.clone();
    for event in events.iter().take_while(|event| event.seq <= ack.seq) {
        state.replay(event)?;
        if !event.is_pure_heartbeat() {
            receipts.insert(event.eid.clone(), event.seq);
        }
    }
    validate_ack(&ack, ack.seq, state.epoch)?;
    Ok(SystemCheckpoint {
        floor_seq: ack.seq,
        epoch_high_water: state.epoch,
        leases: state.leases,
        receipts,
        ack: Some(ack),
        checkpoint,
        pause_event: state.pause_event,
    })
}

/// Pause is protocol metadata: the Hub reads only the envelope. Resume's object
/// is the exact paused event, so an old approval cannot clear a newer pause.
fn validate_control(current:Option<&str>,event:&Event)->Result<(),HubError> {
    if !matches!(event.kind.as_str(),"project.paused"|"project.resume.requested"|"project.resumed") {return Ok(());}
    if event.layer!="L5" || !event.guards.is_empty() || event.stale || event.heartbeat || event.lease_delta.is_some() {
        return Err(invalid("invalid project control envelope"));
    }
    match event.kind.as_str() {
        "project.paused"=>{
            if current.is_some() || !matches!(event.actor.as_str(),"human:local"|"process:towermd-cli") {
                return Err(invalid("project already paused or pause actor invalid"));
            }
        }
        "project.resume.requested"=>{
            if event.actor!="process:towermd-cli" || current!=Some(event.obj.as_str()) {
                return Err(invalid("resume request must name the current pause"));
            }
        }
        "project.resumed"=>{
            if event.actor!="human:local" || current!=Some(event.obj.as_str()) {
                return Err(invalid("resume requires human confirmation of the current pause"));
            }
        }
        _=>unreachable!(),
    }
    Ok(())
}

pub(crate) fn pause_from_originals<'a>(events:impl IntoIterator<Item=&'a Event>)->Result<Option<String>,HubError> {
    let mut events=events.into_iter().collect::<Vec<_>>();
    events.sort_by_key(|event|event.seq);
    let mut pause=None;
    for event in events {
        if event.stale {continue;}
        validate_control(pause.as_deref(),event)?;
        match event.kind.as_str() {
            "project.paused"=>pause=Some(event.eid.clone()),
            "project.resumed"=>pause=None,
            _=>{},
        }
    }
    Ok(pause)
}
pub(crate) fn invalid(message: &str) -> HubError {
    HubError::Invalid(message.into())
}
pub(crate) fn validate_checkpoint(base: &SystemCheckpoint) -> Result<(), HubError> {
    if base.floor_seq == 0 && base.epoch_high_water > 0 {
        return Err(invalid("epoch without sequence boundary"));
    }
    let mut seqs = BTreeSet::new();
    for (eid, seq) in &base.receipts {
        validate_eid(eid)?;
        if *seq == 0 || *seq > base.floor_seq || !seqs.insert(*seq) {
            return Err(invalid("invalid checkpoint receipt"));
        }
    }
    if let Some(ack) = &base.ack {
        if ack.seq != base.floor_seq {
            return Err(invalid("ack differs from checkpoint floor"));
        }
        validate_ack(ack, base.floor_seq, base.epoch_high_water)?;
    }
    SystemState::from_checkpoint(base)?;
    Ok(())
}
fn validate_record(event: &Event) -> Result<(), HubError> {
    validate_input(&crate::hub_protocol::NewEvent {
        eid: event.eid.clone(),
        host: event.host.clone(),
        actor: event.actor.clone(),
        kind: event.kind.clone(),
        layer: event.layer.clone(),
        obj: event.obj.clone(),
        data: Value::Null,
        guards: event.guards.clone(),
    })?;
    if event.cost_usd.is_some() {
        return Err(invalid("unexpected hub event cost"));
    }
    Ok(())
}
pub(crate) fn validate_input(input: &crate::hub_protocol::NewEvent) -> Result<(), HubError> {
    validate_eid(&input.eid)?;
    validate_token("host", &input.host)?;
    validate_actor(&input.actor)?;
    validate_token("type", &input.kind)?;
    validate_token("object", &input.obj)?;
    if crate::types::Layer::parse(&input.layer).is_none() {
        return Err(invalid("unknown event layer"));
    }
    if !input.guards.is_empty() {
        validate_names(input.guards.iter().map(|f| f.name.as_str()))?;
        for fence in &input.guards {
            validate_fence(fence)?;
        }
    }
    Ok(())
}
pub(crate) fn validate_eid(eid: &str) -> Result<(), HubError> {
    if let Some(token) = eid.strip_prefix("git:") {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("invalid git eid token"));
        }
    } else {
        let id = Uuid::parse_str(eid).map_err(|_| invalid("eid must be UUIDv7"))?;
        if id.get_version_num() != 7 || id.get_variant() != Variant::RFC4122 {
            return Err(invalid("eid must be UUIDv7"));
        }
    }
    Ok(())
}
pub(crate) fn validate_token(label: &str, value: &str) -> Result<(), HubError> {
    if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(HubError::Invalid(format!(
            "{label} must be a nonempty token"
        )));
    }
    Ok(())
}
fn validate_actor(actor: &str) -> Result<(), HubError> {
    validate_token("actor", actor)?;
    let part = |s: &str| !s.is_empty() && !s.contains([':', '/']);
    let valid = if let Some(id) = actor
        .strip_prefix("human:")
        .or_else(|| actor.strip_prefix("process:"))
    {
        part(id)
    } else if let Some((a, b)) = actor.split_once('/') {
        part(a) && part(b)
    } else {
        false
    };
    if !valid {
        return Err(invalid(
            "actor must be human:<id>, process:<id>, or <producer>/<version>",
        ));
    }
    Ok(())
}
pub(crate) fn validate_fence(f: &Fence) -> Result<(), HubError> {
    validate_token("lease name", &f.name)?;
    validate_token("lease holder", &f.holder)?;
    if f.epoch == 0 {
        return Err(invalid("fence epoch must be positive"));
    }
    Ok(())
}
pub(crate) fn validate_names<'a>(names: impl Iterator<Item = &'a str>) -> Result<(), HubError> {
    let mut seen = BTreeSet::new();
    for name in names {
        validate_token("lease name", name)?;
        if !seen.insert(name) {
            return Err(invalid("duplicate lease name"));
        }
    }
    if seen.is_empty() {
        return Err(invalid("lease batch must not be empty"));
    }
    Ok(())
}

/// Work leases are named `<kind>:<id>`; `main` and `maintenance` are not.
pub fn is_work_lease(name: &str) -> bool {
    name.split_once(':').is_some_and(|(kind, id)| !kind.is_empty() && !id.is_empty())
}

pub(crate) fn timestamp(now: u64) -> Result<DateTime<Utc>, HubError> {
    if now > 253_402_300_799_999 {
        return Err(invalid("time is outside RFC3339 range"));
    }
    DateTime::from_timestamp_millis(now as i64).ok_or_else(|| invalid("invalid time"))
}
pub(crate) fn expiration(now: u64, ttl: u64) -> Result<u64, HubError> {
    if ttl == 0 {
        return Err(invalid("lease TTL must be positive"));
    }
    let end = now
        .checked_add(ttl)
        .ok_or_else(|| invalid("lease expiration overflow"))?;
    timestamp(end)?;
    Ok(end)
}
pub(crate) fn event_time(event: &Event) -> Result<u64, HubError> {
    let time = DateTime::parse_from_rfc3339(&event.t)
        .map_err(|e| HubError::Invalid(e.to_string()))?
        .timestamp_millis();
    u64::try_from(time).map_err(|_| invalid("negative journal time"))
}
fn validate_ack(ack: &SnapshotAck, last: u64, epoch: u64) -> Result<(), HubError> {
    if !matches!(ack.commit.len(), 40 | 64)
        || !ack.commit.bytes().all(|b| b.is_ascii_hexdigit())
        || ack.seq > last
        || ack.epoch == 0
        || ack.epoch > epoch
    {
        return Err(invalid(
            "snapshot acknowledgement exceeds recorded sequence or epoch bounds",
        ));
    }
    Ok(())
}
/// Probe without creating a hub; opening performs strict validation under its lock.
pub fn probe_store(dir: &Path) -> Result<bool, String> {
    match fs::metadata(dir.join("journal.jsonl")) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err("invalid hub journal path: journal.jsonl".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> crate::hub_protocol::NewEvent {
        crate::hub_protocol::NewEvent {
            eid: Uuid::now_v7().to_string(),
            host: "host".into(),
            actor: "process:test".into(),
            kind: "test.event".into(),
            layer: "L5".into(),
            obj: "test".into(),
            data: Value::Null,
            guards: vec![],
        }
    }
    #[test]
    fn ten_thousand_appends_encode_and_write_one_event_each_without_replacing_inode() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(dir.path()).unwrap();
        let path = dir.path().join("journal.jsonl");
        let mut size = fs::metadata(&path).unwrap().len();
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        #[cfg(unix)]
        let inode = fs::metadata(&path).unwrap().ino();
        for index in 1..=10_000 {
            let AppendReceipt::Event(event) = hub.append(input(), 10).unwrap() else {
                panic!()
            };
            let metadata = fs::metadata(&path).unwrap();
            assert_eq!(
                metadata.len() - size,
                serde_json::to_vec(&event).unwrap().len() as u64 + 1
            );
            size = metadata.len();
            #[cfg(unix)]
            assert_eq!(metadata.ino(), inode);
            assert_eq!(hub.journal.serialized_events, index);
            assert_eq!(hub.journal.written_events, index);
        }
    }
    #[test]
    fn duplicate_checkpoint_receipt_keys_are_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let eid = Uuid::now_v7().to_string();
        let text = format!(
            r#"{{"version":2,"checkpoint":{{"floor_seq":2,"epoch_high_water":1,"leases":{{}},"receipts":{{"{eid}":1,"{eid}":2}},"ack":null,"checkpoint":{{}}}}}}"#
        );
        fs::write(dir.path().join("journal.jsonl"), format!("{text}\n")).unwrap();
        assert!(Hub::open(dir.path()).is_err());
    }
    #[test]
    fn epoch_overflow_does_not_apply_partial_batch() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(dir.path()).unwrap();
        let original = fs::read(dir.path().join("journal.jsonl")).unwrap();
        hub.state.epoch = u64::MAX - 1;
        let result = hub.lease_acquire(
            AcquireLease {
                host: "host".into(),
                names: vec!["a".into(), "b".into()],
                holder: "owner".into(),
                ttl_ms: 10,
            },
            10,
        );
        assert!(matches!(result, Err(HubError::Invalid(_))));
        assert_eq!(hub.state.epoch, u64::MAX - 1);
        assert!(hub.state.leases.is_empty());
        assert_eq!(hub.contiguous_seq(), 0);
        assert_eq!(
            fs::read(dir.path().join("journal.jsonl")).unwrap(),
            original
        );
    }
    #[test]
    fn write_or_sync_failure_poisons_even_duplicate_and_reopen_resolves_outcome() {
        for partial in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut hub = Hub::open(dir.path()).unwrap();
            let first = input();
            HubProtocol::append(&mut hub, first.clone(), 10).unwrap();
            let pending = input();
            hub.journal.fault = Some(partial);
            assert!(matches!(
                HubProtocol::append(&mut hub, pending.clone(), 11),
                Err(HubError::Uncertain(_))
            ));
            assert_eq!(hub.contiguous_seq(), 1);
            assert!(matches!(
                HubProtocol::snapshot_ack(
                    &mut hub,
                    SnapshotAck {
                        commit: "a".repeat(40),
                        seq: 1,
                        epoch: 1
                    },
                    Value::Null
                ),
                Err(HubError::Uncertain(_))
            ));
            assert!(matches!(
                HubProtocol::append(&mut hub, first, 12),
                Err(HubError::Uncertain(_))
            ));
            assert!(matches!(
                hub.rebuild(RebuildSeed {
                    pause_event: None,
                    final_snapshot: None,
                    source_seq: 0,
                    source_epoch: 0,
                    seq_boundary: 1 << 32,
                    epoch_boundary: 1 << 32,
                    checkpoint: Value::Null,
                    receipts: BTreeMap::new(),
                    tail: vec![]
                }),
                Err(HubError::Uncertain(_))
            ));
            drop(hub);
            let mut hub = Hub::open(dir.path()).unwrap();
            let receipt = HubProtocol::append(&mut hub, pending, 12).unwrap();
            let AppendReceipt::Event(event) = receipt else {
                panic!()
            };
            assert_eq!(event.seq, 2);
            assert_eq!(hub.events(0).len(), 2);
            assert_eq!(
                event.t,
                timestamp(if partial { 12 } else { 11 })
                    .unwrap()
                    .to_rfc3339_opts(SecondsFormat::Millis, true)
            );
        }
    }
    #[test]
    fn directory_sync_failure_poison_prevents_overwriting_uncertain_compaction() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(dir.path()).unwrap();
        hub.lease_acquire(
            AcquireLease {
                host: "host".into(),
                names: vec!["main".into()],
                holder: "writer".into(),
                ttl_ms: 100,
            },
            1,
        )
        .unwrap();
        let event = input();
        HubProtocol::append(&mut hub, event.clone(), 2).unwrap();
        let mut base = hub.base.clone();
        base.floor_seq = 2;
        base.epoch_high_water = 1;
        base.leases = hub.state.leases.clone();
        base.receipts = hub.receipts();
        base.ack = Some(SnapshotAck {
            commit: "a".repeat(40),
            seq: 2,
            epoch: 1,
        });
        base.checkpoint = Value::Null;
        assert!(matches!(
            hub.journal.replace_with_sync(
                &Header {
                    version: 2,
                    checkpoint: base
                },
                &[],
                |_| Err(std::io::Error::other("injected directory sync failure"))
            ),
            Err(HubError::Uncertain(_))
        ));
        assert!(matches!(
            HubProtocol::append(&mut hub, event.clone(), 3),
            Err(HubError::Uncertain(_))
        ));
        drop(hub);
        let mut reopened = Hub::open(dir.path()).unwrap();
        assert_eq!(
            HubProtocol::append(&mut reopened, event, 3).unwrap(),
            AppendReceipt::Duplicate { seq: 2 }
        );
    }
    #[test]
    fn atomic_rebuild_directory_sync_uncertain_reopens_complete_acknowledged_state() {
        let source_dir = tempfile::tempdir().unwrap();
        let mut source = Hub::open(source_dir.path()).unwrap();
        let mut seed = RebuildSeed {
            pause_event: None,
            source_seq: 0,
            source_epoch: 0,
            seq_boundary: 1 << 32,
            epoch_boundary: 1 << 32,
            checkpoint: Value::Null,
            receipts: BTreeMap::new(),
            tail: vec![],
            final_snapshot: None,
        };
        source.rebuild(seed.clone()).unwrap();
        let grant = source
            .lease_acquire(
                AcquireLease {
                    host: "host".into(),
                    names: vec!["main".into()],
                    holder: "writer".into(),
                    ttl_ms: 100,
                },
                10,
            )
            .unwrap();
        let original = input();
        let event = source.append(original.clone(), 11).unwrap();
        let AppendReceipt::Event(event) = event else {
            panic!()
        };
        seed.tail = source.subscribe(seed.seq_boundary, 11).unwrap().events;
        seed.final_snapshot = Some(RebuildSnapshot {
            ack: SnapshotAck {
                commit: "a".repeat(40),
                seq: event.seq,
                epoch: grant.leases[0].epoch,
            },
            checkpoint: serde_json::json!(["opaque"]),
        });
        let target_dir = tempfile::tempdir().unwrap();
        let mut target = Hub::open(target_dir.path()).unwrap();
        assert!(matches!(
            target.rebuild_with_sync(seed.clone(), |_| Err(std::io::Error::other(
                "injected directory sync failure"
            ))),
            Err(HubError::Uncertain(_))
        ));
        assert_eq!(
            target.contiguous_seq(),
            0,
            "no in-memory partial installation after uncertain persistence"
        );
        assert!(matches!(
            target.subscribe(0, 11),
            Err(HubError::Uncertain(_))
        ));
        assert!(
            Hub::open(target_dir.path()).is_err(),
            "uncertain owner remains exclusive"
        );
        drop(target);
        let mut reopened = Hub::open(target_dir.path()).unwrap();
        let snapshot = seed.final_snapshot.unwrap();
        let actual = reopened.subscribe(snapshot.ack.seq, 11).unwrap();
        assert_eq!(actual.head.ack, Some(snapshot.ack));
        assert_eq!(actual.head.floor_seq, event.seq);
        assert_eq!(actual.head.leases.get("main"), Some(&grant.leases[0]));
        assert!(actual.events.is_empty());
        assert!(
            matches!(reopened.subscribe(0, 11), Err(HubError::SnapshotRequired { checkpoint, .. }) if checkpoint == snapshot.checkpoint)
        );
        assert_eq!(
            reopened.append(original, 11).unwrap(),
            AppendReceipt::Duplicate { seq: event.seq }
        );
    }

    #[test]
    fn drop_releases_lock_even_when_an_inherited_descriptor_remains_open() {
        let dir = tempfile::tempdir().unwrap();
        let hub = Hub::open(dir.path()).unwrap();
        let inherited = hub._lock.try_clone().unwrap();
        assert!(Hub::open(dir.path()).is_err());
        drop(hub);
        let reopened = Hub::open(dir.path()).unwrap();
        drop(inherited);
        assert!(Hub::open(dir.path()).is_err());
        drop(reopened);
        assert!(Hub::open(dir.path()).is_ok());
    }
    #[test]
    fn existing_open_never_recreates_a_store_lock_or_journal_after_precheck() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("existing");
        assert!(Hub::open_existing(&directory).is_err());
        assert!(!directory.exists());
        let mut hub = Hub::open(&directory).unwrap();
        hub.append(input(), 1000).unwrap();
        let seq = hub.contiguous_seq();
        drop(hub);
        assert_eq!(inspect_store(&directory).unwrap(), StoreState::Nonempty);
        let bytes = fs::read(directory.join("journal.jsonl")).unwrap();
        let opened = Hub::open_existing(&directory).unwrap();
        assert_eq!(opened.contiguous_seq(), seq);
        drop(opened);
        assert_eq!(fs::read(directory.join("journal.jsonl")).unwrap(), bytes);
        // A complete precheck cannot authorize later tail repair. If a partial
        // append appears before existing-only open, all bytes remain untouched.
        assert_eq!(inspect_store(&directory).unwrap(), StoreState::Nonempty);
        let mut partial = bytes.clone();
        partial.extend_from_slice(b"{\"incomplete\":");
        fs::write(directory.join("journal.jsonl"), &partial).unwrap();
        assert!(Hub::open_existing(&directory).is_err());
        assert_eq!(fs::read(directory.join("journal.jsonl")).unwrap(), partial);
        fs::write(directory.join("journal.jsonl"), &bytes).unwrap();
        // Deterministic precheck/open race: the valid journal is removed after
        // inspect_store, before the existing-only atomic open.
        assert_eq!(inspect_store(&directory).unwrap(), StoreState::Nonempty);
        fs::remove_file(directory.join("journal.jsonl")).unwrap();
        assert!(Hub::open_existing(&directory).is_err());
        assert!(!directory.join("journal.jsonl").exists());
        fs::write(directory.join("journal.jsonl"), &bytes).unwrap();
        fs::remove_file(directory.join("hub.lock")).unwrap();
        assert!(Hub::open_existing(&directory).is_err());
        assert!(!directory.join("hub.lock").exists());
        assert_eq!(fs::read(directory.join("journal.jsonl")).unwrap(), bytes);
        // Restore the original empty lock only as fixture setup; a corrupt
        // complete journal is rejected and never reset to a new empty Hub.
        fs::write(directory.join("hub.lock"), b"").unwrap();
        fs::write(
            directory.join("journal.jsonl"),
            b"corrupt complete journal\n",
        )
        .unwrap();
        assert!(Hub::open_existing(&directory).is_err());
        assert_eq!(
            fs::read(directory.join("journal.jsonl")).unwrap(),
            b"corrupt complete journal\n"
        );
    }

    #[test]
    fn store_inspection_validates_without_creation_migration_or_tail_repair() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        assert_eq!(inspect_store(&missing).unwrap(), StoreState::Missing);
        assert!(!missing.exists());
        let dir = temp.path().join("target");
        let mut hub = Hub::open(&dir).unwrap();
        let original = fs::read(dir.join("journal.jsonl")).unwrap();
        assert_eq!(inspect_store(&dir).unwrap(), StoreState::Empty);
        assert_eq!(fs::read(dir.join("journal.jsonl")).unwrap(), original);
        hub.append(input(), 1000).unwrap();
        assert_eq!(inspect_store(&dir).unwrap(), StoreState::Nonempty);
        drop(hub);
        let mut partial = fs::read(dir.join("journal.jsonl")).unwrap();
        partial.extend(b"{\"partial\":");
        fs::write(dir.join("journal.jsonl"), &partial).unwrap();
        assert!(inspect_store(&dir).is_err());
        assert_eq!(fs::read(dir.join("journal.jsonl")).unwrap(), partial);
        for change in ["epoch", "sequence", "opaque"] {
            let mut header: Value = serde_json::from_slice(&original).unwrap();
            match change {
                "epoch" => header["checkpoint"]["epoch_high_water"] = serde_json::json!(1),
                "sequence" => header["checkpoint"]["floor_seq"] = serde_json::json!(2),
                _ => header["checkpoint"]["checkpoint"] = serde_json::json!({"untrusted":true}),
            }
            let mut bytes = serde_json::to_vec(&header).unwrap();
            bytes.push(b'\n');
            fs::write(dir.join("journal.jsonl"), &bytes).unwrap();
            assert!(
                !matches!(inspect_store(&dir), Ok(StoreState::Empty)),
                "{change}"
            );
            assert_eq!(fs::read(dir.join("journal.jsonl")).unwrap(), bytes);
        }
    }
}
