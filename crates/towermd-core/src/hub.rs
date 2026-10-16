use crate::snapshot::{SequenceRange, Snapshot};
use crate::types::Layer;
use chrono::{DateTime, SecondsFormat, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::{Uuid, Variant};

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
    pub fence: Option<Fence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fence {
    pub name: String,
    pub holder: String,
    pub epoch: u64,
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

pub fn derived_eid(commit: &str, kind: &str) -> Result<String, String> {
    validate_commit(commit)?;
    validate_token("type", kind)?;
    let mut hash = Sha256::new();
    hash.update(commit.as_bytes());
    hash.update([0]);
    hash.update(kind.as_bytes());
    Ok(format!("git:{}", hex::encode(hash.finalize())))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LeaseChange {
    Acquire {
        names: Vec<String>,
        holder: String,
        ttl_ms: u64,
    },
    Renew {
        fences: Vec<Fence>,
        ttl_ms: u64,
    },
    Release {
        fences: Vec<Fence>,
    },
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
    fence: Option<Fence>,
    lease_delta: Option<LeaseDelta>,
}

impl Event {
    pub fn acquired_leases(&self) -> &[Lease] {
        match &self.lease_delta {
            Some(delta) if matches!(delta.change, LeaseChange::Acquire { .. }) => &delta.updated,
            _ => &[],
        }
    }

    pub fn released_lease_names(&self) -> Vec<String> {
        match &self.lease_delta {
            Some(LeaseDelta {
                change: LeaseChange::Release { fences },
                ..
            }) => fences.iter().map(|fence| fence.name.clone()).collect(),
            _ => Vec::new(),
        }
    }

    pub fn is_pure_heartbeat(&self) -> bool {
        self.kind == "lease.renewed"
            && self.cost_usd.is_none()
            && self
                .lease_delta
                .as_ref()
                .is_some_and(|delta| matches!(delta.change, LeaseChange::Renew { .. }))
            && self.data.as_object().is_some_and(|data| {
                data.iter().all(|(key, value)| {
                    matches!(key.as_str(), "run" | "lease")
                        && value.as_str().is_some_and(|value| !value.is_empty())
                })
            })
    }

    pub fn updated_leases(&self) -> &[Lease] {
        self.lease_delta
            .as_ref()
            .map_or(&[], |delta| delta.updated.as_slice())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct LeaseDelta {
    change: LeaseChange,
    updated: Vec<Lease>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    events: Vec<Event>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot_ack: Option<SnapshotAck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_snapshot: Option<Snapshot>,
}

#[derive(Clone, Debug, Default)]
struct Projection {
    leases: BTreeMap<String, Lease>,
    last_epoch: u64,
}

impl Projection {
    fn active(&self, fence: &Fence, now_ms: u64) -> bool {
        self.leases.get(&fence.name).is_some_and(|lease| {
            lease.holder == fence.holder && lease.epoch == fence.epoch && lease.expires_at > now_ms
        })
    }

    fn change(&mut self, change: &LeaseChange, now_ms: u64) -> Result<Vec<Lease>, String> {
        let mut updated = Vec::new();
        match change {
            LeaseChange::Acquire {
                names,
                holder,
                ttl_ms,
            } => {
                validate_token("lease holder", holder)?;
                validate_names(names.iter().map(String::as_str))?;
                let expires_at = expiration(now_ms, *ttl_ms)?;
                for name in names {
                    if self
                        .leases
                        .get(name)
                        .is_some_and(|lease| lease.expires_at > now_ms)
                    {
                        return Err(format!("lease {name} is already held"));
                    }
                }
                for name in names {
                    self.last_epoch = self.last_epoch.checked_add(1).ok_or("epoch overflow")?;
                    let lease = Lease {
                        name: name.clone(),
                        holder: holder.clone(),
                        epoch: self.last_epoch,
                        expires_at,
                    };
                    self.leases.insert(name.clone(), lease.clone());
                    updated.push(lease);
                }
            }
            LeaseChange::Renew { fences, ttl_ms } => {
                self.validate_fences(fences, now_ms)?;
                let expires_at = expiration(now_ms, *ttl_ms)?;
                for fence in fences {
                    let lease = Lease {
                        name: fence.name.clone(),
                        holder: fence.holder.clone(),
                        epoch: fence.epoch,
                        expires_at,
                    };
                    self.leases.insert(fence.name.clone(), lease.clone());
                    updated.push(lease);
                }
            }
            LeaseChange::Release { fences } => {
                self.validate_fences(fences, now_ms)?;
                for fence in fences {
                    self.leases.remove(&fence.name);
                }
            }
        }
        Ok(updated)
    }

    fn validate_fences(&self, fences: &[Fence], now_ms: u64) -> Result<(), String> {
        validate_names(fences.iter().map(|fence| fence.name.as_str()))?;
        for fence in fences {
            validate_fence(fence)?;
            if !self.active(fence, now_ms) {
                return Err(format!(
                    "lease {} fence is expired or no longer held",
                    fence.name
                ));
            }
        }
        Ok(())
    }

    fn prepare(
        &self,
        input: NewEvent,
        change: Option<LeaseChange>,
        now_ms: u64,
        seq: u64,
    ) -> Result<(Event, Self), String> {
        validate_input(&input)?;
        let t = timestamp(now_ms)?.to_rfc3339_opts(SecondsFormat::Millis, true);
        let stale = input
            .fence
            .as_ref()
            .is_some_and(|fence| !self.active(fence, now_ms));
        let mut next = self.clone();
        let lease_delta = if stale {
            None
        } else {
            change
                .map(|change| {
                    let updated = next.change(&change, now_ms)?;
                    Ok::<_, String>(LeaseDelta { change, updated })
                })
                .transpose()?
        };
        Ok((
            Event {
                seq,
                eid: input.eid,
                host: input.host,
                t,
                actor: input.actor,
                kind: input.kind,
                layer: input.layer,
                obj: input.obj,
                data: input.data,
                epoch: input.fence.as_ref().map(|fence| fence.epoch),
                stale,
                cost_usd: None,
                fence: input.fence,
                lease_delta,
            },
            next,
        ))
    }
}

pub struct Hub {
    dir: PathBuf,
    _lock: File,
    events: Vec<Event>,
    by_eid: BTreeMap<String, usize>,
    projection: Projection,
    snapshot_ack: Option<SnapshotAck>,
    base_snapshot: Option<Snapshot>,
    poisoned: bool,
}

impl Drop for Hub {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._lock);
    }
}

impl Hub {
    pub(crate) fn stage_recovery(
        dir: &Path,
        source: &Snapshot,
        seq_boundary: u64,
        epoch_boundary: u64,
    ) -> Result<Self, String> {
        let minimum_seq = source
            .through_seq
            .checked_add(1_u64 << 32)
            .ok_or("recovery sequence boundary overflow")?;
        let minimum_epoch = source
            .epoch_high_water
            .checked_add(1_u64 << 32)
            .ok_or("recovery epoch boundary overflow")?;
        source.validate_metadata()?;
        if seq_boundary < minimum_seq || epoch_boundary < minimum_epoch {
            return Err("recovery boundaries must skip at least 2^32 sequences and epochs".into());
        }
        let mut seed = source.clone();
        seed.through_seq = seq_boundary;
        seed.epoch_high_water = epoch_boundary;
        seed.recovery_gaps.push(SequenceRange {
            start: source
                .through_seq
                .checked_add(1)
                .ok_or("recovery sequence overflow")?,
            end: seq_boundary,
        });
        seed.validate_metadata()?;
        match fs::symlink_metadata(dir) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(
                    "recovery staging path must be a directory, not a symlink or file".into(),
                )
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(dir)
                    .map_err(|error| format!("create staging directory: {error}"))?;
            }
            Err(error) => return Err(format!("inspect staging directory: {error}")),
        }
        if fs::read_dir(dir)
            .map_err(|error| format!("read staging directory: {error}"))?
            .next()
            .is_some()
        {
            return Err("recovery staging directory must be empty".into());
        }
        let mut hub = Self::open(dir)?;
        for entry in
            fs::read_dir(&hub.dir).map_err(|error| format!("read staging directory: {error}"))?
        {
            if entry
                .map_err(|error| format!("inspect staging entry: {error}"))?
                .file_name()
                != "hub.lock"
            {
                return Err("recovery staging directory is no longer empty".into());
            }
        }
        hub.persist(&[], None, Some(&seed), File::sync_all)?;
        hub.projection.last_epoch = epoch_boundary;
        hub.base_snapshot = Some(seed);
        Ok(hub)
    }

    pub fn base_snapshot(&self) -> Option<&Snapshot> {
        self.base_snapshot.as_ref()
    }

    pub(crate) fn install_recovery(&mut self, staged: &Hub) -> Result<(), String> {
        self.install_recovery_with_sync(staged, File::sync_all)
    }

    fn install_recovery_with_sync(
        &mut self,
        staged: &Hub,
        sync_parent: impl FnOnce(&File) -> io::Result<()>,
    ) -> Result<(), String> {
        self.require_certain()?;
        staged.require_certain()?;
        let seed = staged
            .base_snapshot
            .as_ref()
            .ok_or("staged hub requires a base snapshot")?;
        if staged.last_seq() <= self.last_seq()
            || staged.epoch_high_water() <= self.epoch_high_water()
        {
            return Err("recovery must advance both sequence and epoch boundaries".into());
        }
        if self
            .base_snapshot
            .as_ref()
            .is_some_and(|base| base.project_id != seed.project_id)
        {
            return Err("recovery project does not match target base snapshot".into());
        }
        self.persist(
            &staged.events,
            staged.snapshot_ack.as_ref(),
            Some(seed),
            sync_parent,
        )?;
        self.events = staged.events.clone();
        self.by_eid = staged.by_eid.clone();
        self.projection = staged.projection.clone();
        self.snapshot_ack = staged.snapshot_ack.clone();
        self.base_snapshot = staged.base_snapshot.clone();
        Ok(())
    }

    pub fn checkpoint(
        &self,
        project_id: &str,
        writer: &Fence,
        now_ms: u64,
    ) -> Result<Snapshot, String> {
        self.require_certain()?;
        if writer.name != "main" || !self.projection.active(writer, now_ms) {
            return Err("checkpoint requires the current main lease fence".into());
        }
        match self.base_snapshot.as_ref() {
            Some(base) => {
                if base.project_id != project_id {
                    return Err("checkpoint project does not match base snapshot".into());
                }
                Snapshot::extend(Some(base), &self.events, self.last_seq(), writer.epoch)
            }
            None => Snapshot::from_events(project_id, &self.events, self.last_seq(), writer.epoch),
        }
    }

    pub fn acknowledge(&mut self, ack: SnapshotAck) -> Result<(), String> {
        self.acknowledge_with_sync(ack, File::sync_all)
    }

    pub fn snapshot_ack(&self) -> Option<&SnapshotAck> {
        self.snapshot_ack.as_ref()
    }

    pub fn epoch_high_water(&self) -> u64 {
        self.projection.last_epoch
    }

    pub fn receipts(&self) -> BTreeMap<String, u64> {
        let mut receipts = self
            .base_snapshot
            .as_ref()
            .map_or_else(BTreeMap::new, |base| base.receipts.clone());
        receipts.extend(
            self.events
                .iter()
                .map(|event| (event.eid.clone(), event.seq)),
        );
        receipts
    }

    fn require_certain(&self) -> Result<(), String> {
        if self.poisoned {
            return Err(
                "hub commit outcome is uncertain; drop and reopen hub before writing".into(),
            );
        }
        Ok(())
    }

    fn acknowledge_with_sync(
        &mut self,
        ack: SnapshotAck,
        sync_parent: impl FnOnce(&File) -> io::Result<()>,
    ) -> Result<(), String> {
        self.require_certain()?;
        validate_ack(&ack, self.last_seq(), self.epoch_high_water())?;
        if let Some(previous) = &self.snapshot_ack {
            if ack.seq < previous.seq {
                return Err("snapshot acknowledgement cannot move sequence backwards".into());
            }
            if previous == &ack {
                return Ok(());
            }
        }
        let events = self.events.clone();
        let base_snapshot = self.base_snapshot.clone();
        self.persist(&events, Some(&ack), base_snapshot.as_ref(), sync_parent)?;
        self.snapshot_ack = Some(ack);
        Ok(())
    }

    pub fn open(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|error| format!("create hub directory: {error}"))?;
        let dir =
            fs::canonicalize(dir).map_err(|error| format!("resolve hub directory: {error}"))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("hub.lock"))
            .map_err(|error| format!("open hub lock: {error}"))?;
        FileExt::try_lock_exclusive(&lock)
            .map_err(|error| format!("hub already locked or lock unavailable: {error}"))?;
        let (events, snapshot_ack, base_snapshot) = match fs::read(dir.join("journal.json")) {
            Ok(bytes) => {
                let journal: Journal = serde_json::from_slice(&bytes)
                    .map_err(|error| format!("invalid hub journal: {error}"))?;
                if journal.version != 1 {
                    return Err("unsupported hub journal version".into());
                }
                (journal.events, journal.snapshot_ack, journal.base_snapshot)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Vec::new(), None, None),
            Err(error) => return Err(format!("read hub journal: {error}")),
        };
        if let Some(base) = &base_snapshot {
            base.validate_metadata()?;
        }
        let mut projection = Projection {
            last_epoch: base_snapshot
                .as_ref()
                .map_or(0, |base| base.epoch_high_water),
            ..Projection::default()
        };
        let mut by_eid = BTreeMap::new();
        let mut seq = base_snapshot.as_ref().map_or(0, |base| base.through_seq);
        for (index, event) in events.iter().enumerate() {
            seq = seq.checked_add(1).ok_or("journal sequence overflow")?;
            if event.seq != seq
                || base_snapshot
                    .as_ref()
                    .is_some_and(|base| base.receipts.contains_key(&event.eid))
                || by_eid.insert(event.eid.clone(), index).is_some()
            {
                return Err("invalid hub journal sequence or duplicate eid".into());
            }
            let now_ms = DateTime::parse_from_rfc3339(&event.t)
                .map_err(|error| format!("invalid journal time: {error}"))?
                .timestamp_millis();
            let now_ms = u64::try_from(now_ms).map_err(|_| "negative journal time")?;
            let input = NewEvent {
                eid: event.eid.clone(),
                host: event.host.clone(),
                actor: event.actor.clone(),
                kind: event.kind.clone(),
                layer: event.layer.clone(),
                obj: event.obj.clone(),
                data: event.data.clone(),
                fence: event.fence.clone(),
            };
            let change = event.lease_delta.as_ref().map(|delta| delta.change.clone());
            let (replayed, next) = projection
                .prepare(input, change, now_ms, seq)
                .map_err(|error| format!("invalid journal event {seq}: {error}"))?;
            if replayed != *event {
                return Err(format!("inconsistent journal event {seq}"));
            }
            projection = next;
        }
        if let Some(ack) = &snapshot_ack {
            validate_ack(ack, seq, projection.last_epoch)?;
        }
        Ok(Self {
            dir,
            _lock: lock,
            events,
            by_eid,
            projection,
            snapshot_ack,
            base_snapshot,
            poisoned: false,
        })
    }

    pub fn append(
        &mut self,
        input: NewEvent,
        change: Option<LeaseChange>,
        now_ms: u64,
    ) -> Result<Event, String> {
        match self.append_receipt(input, change, now_ms)? {
            AppendReceipt::Event(event) => Ok(*event),
            AppendReceipt::Duplicate { seq } => Err(format!(
                "historical event at sequence {seq} has no retained body; use append_receipt"
            )),
        }
    }

    pub fn append_receipt(
        &mut self,
        input: NewEvent,
        change: Option<LeaseChange>,
        now_ms: u64,
    ) -> Result<AppendReceipt, String> {
        self.require_certain()?;
        if input.eid.starts_with("git:") {
            validate_input(&input)?;
        }
        if let Some(index) = self.by_eid.get(&input.eid) {
            return Ok(AppendReceipt::Event(Box::new(self.events[*index].clone())));
        }
        if let Some(seq) = self
            .base_snapshot
            .as_ref()
            .and_then(|base| base.receipts.get(&input.eid))
        {
            return Ok(AppendReceipt::Duplicate { seq: *seq });
        }
        let seq = self.last_seq().checked_add(1).ok_or("sequence overflow")?;
        let (event, projection) = self.projection.prepare(input, change, now_ms, seq)?;
        let mut events = self.events.clone();
        events.push(event.clone());
        let snapshot_ack = self.snapshot_ack.clone();
        let base_snapshot = self.base_snapshot.clone();
        self.persist(
            &events,
            snapshot_ack.as_ref(),
            base_snapshot.as_ref(),
            File::sync_all,
        )?;
        self.by_eid.insert(event.eid.clone(), self.events.len());
        self.events = events;
        self.projection = projection;
        Ok(AppendReceipt::Event(Box::new(event)))
    }

    pub fn events(&self, after: u64) -> Vec<Event> {
        self.events
            .iter()
            .filter(|event| event.seq > after)
            .cloned()
            .collect()
    }

    pub fn leases(&self, now_ms: u64) -> BTreeMap<String, Lease> {
        self.projection
            .leases
            .iter()
            .filter(|(_, lease)| lease.expires_at > now_ms)
            .map(|(name, lease)| (name.clone(), lease.clone()))
            .collect()
    }

    pub fn last_seq(&self) -> u64 {
        self.events.last().map_or_else(
            || {
                self.base_snapshot
                    .as_ref()
                    .map_or(0, |base| base.through_seq)
            },
            |event| event.seq,
        )
    }

    fn persist(
        &mut self,
        events: &[Event],
        snapshot_ack: Option<&SnapshotAck>,
        base_snapshot: Option<&Snapshot>,
        sync_parent: impl FnOnce(&File) -> io::Result<()>,
    ) -> Result<(), String> {
        let parent =
            File::open(&self.dir).map_err(|error| format!("open journal parent: {error}"))?;
        let temporary = self.dir.join(format!(".journal-{}.tmp", Uuid::now_v7()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("create journal temporary file: {error}"))?;
        let result = (|| {
            serde_json::to_writer(
                &mut file,
                &Journal {
                    version: 1,
                    events: events.to_vec(),
                    snapshot_ack: snapshot_ack.cloned(),
                    base_snapshot: base_snapshot.cloned(),
                },
            )
            .map_err(|error| format!("serialize journal: {error}"))?;
            file.flush()
                .map_err(|error| format!("flush journal: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("sync journal: {error}"))?;
            fs::rename(&temporary, self.dir.join("journal.json"))
                .map_err(|error| format!("replace journal: {error}"))?;
            if let Err(error) = sync_parent(&parent) {
                self.poisoned = true;
                return Err(format!(
                    "journal renamed but directory sync failed; commit outcome is uncertain; drop and reopen hub: {error}"
                ));
            }
            Ok(())
        })();
        drop(file);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn validate_commit(commit: &str) -> Result<(), String> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("commit must be a full 40 or 64 character hexadecimal hash".into());
    }
    Ok(())
}

fn validate_ack(ack: &SnapshotAck, last_seq: u64, epoch_high_water: u64) -> Result<(), String> {
    validate_commit(&ack.commit)?;
    if ack.seq > last_seq || ack.epoch == 0 || ack.epoch > epoch_high_water {
        return Err("snapshot acknowledgement exceeds recorded sequence or epoch bounds".into());
    }
    Ok(())
}

fn validate_token(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(format!("{label} must be a nonempty token"));
    }
    Ok(())
}

fn validate_actor(actor: &str) -> Result<(), String> {
    validate_token("actor", actor)?;
    let valid_part = |part: &str| !part.is_empty() && !part.contains([':', '/']);
    let valid = if let Some(id) = actor
        .strip_prefix("human:")
        .or_else(|| actor.strip_prefix("process:"))
    {
        valid_part(id)
    } else if let Some((producer, version)) = actor.split_once('/') {
        valid_part(producer) && valid_part(version)
    } else {
        false
    };
    if !valid {
        return Err("actor must be human:<id>, process:<id>, or <harness>/<version>".into());
    }
    Ok(())
}

fn validate_input(input: &NewEvent) -> Result<(), String> {
    if input.eid.starts_with("git:") {
        let commit = input.data["commit"]
            .as_str()
            .ok_or("derived event requires data.commit")?;
        if input.eid != derived_eid(commit, &input.kind)? {
            return Err("derived eid does not match data.commit and event type".into());
        }
    } else {
        let eid = Uuid::parse_str(&input.eid).map_err(|_| "eid must be UUIDv7")?;
        if eid.get_version_num() != 7 || eid.get_variant() != Variant::RFC4122 {
            return Err("eid must be UUIDv7".into());
        }
    }
    validate_token("host", &input.host)?;
    validate_actor(&input.actor)?;
    validate_token("type", &input.kind)?;
    validate_token("object", &input.obj)?;
    validate_token("layer", &input.layer)?;
    if Layer::parse(&input.layer).is_none() {
        return Err("unknown event layer".into());
    }
    if let Some(fence) = &input.fence {
        validate_fence(fence)?;
    }
    Ok(())
}

fn validate_fence(fence: &Fence) -> Result<(), String> {
    validate_token("lease name", &fence.name)?;
    validate_token("lease holder", &fence.holder)?;
    if fence.epoch == 0 {
        return Err("fence epoch must be positive".into());
    }
    Ok(())
}

fn validate_names<'a>(names: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for name in names {
        validate_token("lease name", name)?;
        if !seen.insert(name) {
            return Err(format!("duplicate lease name {name}"));
        }
    }
    if seen.is_empty() {
        return Err("lease batch must not be empty".into());
    }
    Ok(())
}

fn timestamp(now_ms: u64) -> Result<DateTime<Utc>, String> {
    if now_ms > 253_402_300_799_999 {
        return Err("time is outside RFC3339 range".into());
    }
    DateTime::from_timestamp_millis(now_ms as i64).ok_or_else(|| "invalid time".into())
}

fn expiration(now_ms: u64, ttl_ms: u64) -> Result<u64, String> {
    if ttl_ms == 0 {
        return Err("lease TTL must be positive".into());
    }
    let expires_at = now_ms
        .checked_add(ttl_ms)
        .ok_or("lease expiration overflow")?;
    timestamp(expires_at)?;
    Ok(expires_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> NewEvent {
        NewEvent {
            eid: Uuid::now_v7().to_string(),
            host: "host".into(),
            actor: "process:test".into(),
            kind: "test.event".into(),
            layer: "L5".into(),
            obj: "test".into(),
            data: Value::Null,
            fence: None,
        }
    }

    #[test]
    #[cfg(unix)]
    fn drop_releases_lock_even_when_an_inherited_descriptor_remains_open() {
        let dir = tempfile::tempdir().unwrap();
        let hub = Hub::open(dir.path()).unwrap();
        let inherited = hub._lock.try_clone().unwrap();
        assert!(Hub::open(dir.path()).is_err());
        drop(hub);
        let reopened = Hub::open(dir.path()).unwrap();
        assert!(Hub::open(dir.path()).is_err());
        drop(inherited);
        assert!(Hub::open(dir.path()).is_err());
        drop(reopened);
        assert!(Hub::open(dir.path()).is_ok());
    }

    #[test]
    fn directory_sync_failure_poison_prevents_overwriting_uncertain_commit() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(dir.path()).unwrap();
        let first_input = input();
        let first = hub.append(first_input.clone(), None, 10).unwrap();
        let pending = input();
        let (event, _) = hub
            .projection
            .prepare(pending.clone(), None, 11, 2)
            .unwrap();
        let error = hub
            .persist(&[first.clone(), event.clone()], None, None, |_| {
                Err(io::Error::other("injected parent sync failure"))
            })
            .unwrap_err();
        assert!(error.contains("uncertain"));
        assert_eq!(hub.events(0), vec![first.clone()]);
        assert_eq!(hub.last_seq(), 1);
        assert!(hub
            .append(input(), None, 12)
            .unwrap_err()
            .contains("reopen"));
        assert!(hub.append(pending.clone(), None, 12).is_err());
        assert!(hub.append(first_input.clone(), None, 12).is_err());
        drop(hub);
        let mut reopened = Hub::open(dir.path()).unwrap();
        assert_eq!(reopened.append(first_input, None, 12).unwrap(), first);
        assert_eq!(reopened.append(pending, None, 12).unwrap(), event);
        assert_eq!(reopened.append(input(), None, 12).unwrap().seq, 3);
    }

    #[test]
    fn ack_sync_failure_keeps_memory_and_poisons_checkpoint_until_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(dir.path()).unwrap();
        let granted = hub
            .append(
                input(),
                Some(LeaseChange::Acquire {
                    names: vec!["main".into()],
                    holder: "writer".into(),
                    ttl_ms: 100,
                }),
                1,
            )
            .unwrap();
        let writer = granted.updated_leases()[0].fence();
        let previous = SnapshotAck {
            commit: "a".repeat(40),
            seq: 1,
            epoch: writer.epoch,
        };
        hub.acknowledge(previous.clone()).unwrap();
        hub.acknowledge_with_sync(previous.clone(), |_| {
            panic!("idempotent ack must not write")
        })
        .unwrap();
        hub.append(input(), None, 2).unwrap();
        hub.checkpoint("project", &writer, 2).unwrap();
        let events = hub.events(0);
        let receipts = hub.receipts();
        let pending = SnapshotAck {
            commit: "b".repeat(40),
            seq: 2,
            epoch: writer.epoch,
        };
        let error = hub
            .acknowledge_with_sync(pending.clone(), |_| {
                Err(io::Error::other("injected parent sync failure"))
            })
            .unwrap_err();
        assert!(error.contains("uncertain"));
        assert_eq!(hub.snapshot_ack(), Some(&previous));
        assert_eq!(hub.events(0), events);
        assert_eq!(hub.receipts(), receipts);
        assert!(hub
            .checkpoint("project", &writer, 3)
            .unwrap_err()
            .contains("reopen"));
        assert!(hub.acknowledge(previous).is_err());
        assert!(hub.acknowledge(pending.clone()).is_err());
        assert!(hub.append(input(), None, 3).is_err());
        drop(hub);
        let mut hub = Hub::open(dir.path()).unwrap();
        assert_eq!(hub.snapshot_ack(), Some(&pending));
        assert_eq!(hub.events(0), events);
        assert_eq!(hub.receipts(), receipts);
        hub.acknowledge(pending).unwrap();
        hub.checkpoint("project", &writer, 3).unwrap();
    }

    #[test]
    fn epoch_overflow_does_not_apply_partial_batch() {
        let projection = Projection {
            last_epoch: u64::MAX - 1,
            ..Projection::default()
        };
        let change = Some(LeaseChange::Acquire {
            names: vec!["a".into(), "b".into()],
            holder: "owner".into(),
            ttl_ms: 10,
        });
        assert!(projection.prepare(input(), change, 10, 1).is_err());
        assert_eq!(projection.last_epoch, u64::MAX - 1);
        assert!(projection.leases.is_empty());
    }

    fn recovery_source() -> (Snapshot, NewEvent, Fence) {
        let temporary = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(temporary.path()).unwrap();
        let main = hub
            .append(
                input(),
                Some(LeaseChange::Acquire {
                    names: vec!["main".into()],
                    holder: "writer".into(),
                    ttl_ms: 100,
                }),
                1,
            )
            .unwrap();
        let writer = main.updated_leases()[0].fence();
        let mut task = input();
        task.kind = "task.state".into();
        task.data = serde_json::json!({
            "task": {"type":"Task", "id":"T", "state":"running", "future":{"nested":[null, 42]}},
            "run": {"type":"Run", "id":"R", "state":"running", "cost_usd":null}
        });
        hub.append(task.clone(), None, 2).unwrap();
        hub.append(historical_derived(), None, 3).unwrap();
        (hub.checkpoint("project", &writer, 3).unwrap(), task, writer)
    }

    fn historical_derived() -> NewEvent {
        let mut event = input();
        event.kind = "snapshot.written".into();
        let commit = "a".repeat(40);
        event.eid = derived_eid(&commit, &event.kind).unwrap();
        event.data = serde_json::json!({"commit":commit});
        event
    }

    #[test]
    fn staged_seed_jumps_boundaries_invalidates_leases_and_survives_restart() {
        let (source, _, writer) = recovery_source();
        let temporary = tempfile::tempdir().unwrap();
        let seq = source.through_seq + (1_u64 << 32);
        let epoch = source.epoch_high_water + (1_u64 << 32);
        let mut hub = Hub::stage_recovery(temporary.path(), &source, seq, epoch).unwrap();
        assert_eq!(hub.last_seq(), seq);
        assert_eq!(hub.epoch_high_water(), epoch);
        assert_eq!(hub.receipts(), source.receipts);
        assert!(hub.events(0).is_empty());
        assert!(hub.leases(3).is_empty());
        assert_eq!(hub.base_snapshot().unwrap().projection, source.projection);
        assert_eq!(
            hub.base_snapshot().unwrap().recovery_gaps.last().unwrap(),
            &crate::snapshot::SequenceRange {
                start: source.through_seq + 1,
                end: seq,
            }
        );
        assert!(hub.checkpoint("project", &writer, 3).is_err());
        assert!(hub
            .append(
                input(),
                Some(LeaseChange::Renew {
                    fences: vec![writer.clone()],
                    ttl_ms: 100
                }),
                4
            )
            .is_err());
        let mut old = input();
        old.fence = Some(writer);
        let stale = hub.append(old, None, 4).unwrap();
        assert!(stale.stale);
        assert_eq!(stale.seq, seq + 1);
        let mut rebuilt = input();
        rebuilt.kind = "hub.rebuilt".into();
        hub.append(rebuilt, None, 5).unwrap();
        let granted = hub
            .append(
                input(),
                Some(LeaseChange::Acquire {
                    names: vec!["main".into()],
                    holder: "new".into(),
                    ttl_ms: 100,
                }),
                6,
            )
            .unwrap();
        let writer = granted.updated_leases()[0].fence();
        assert_eq!(writer.epoch, epoch + 1);
        let checkpoint = hub.checkpoint("project", &writer, 6).unwrap();
        assert_eq!(checkpoint.projection.objects, source.projection.objects);
        assert_eq!(checkpoint.projection.lease_summaries.len(), 1);
        assert_eq!(checkpoint.projection.lease_summaries["main"], writer);
        assert!(hub.checkpoint("another-project", &writer, 6).is_err());
        let ack = SnapshotAck {
            commit: "b".repeat(40),
            seq: hub.last_seq(),
            epoch: writer.epoch,
        };
        hub.acknowledge(ack.clone()).unwrap();
        let events = hub.events(0);
        let base = hub.base_snapshot().unwrap().clone();
        drop(hub);
        let hub = Hub::open(temporary.path()).unwrap();
        assert_eq!(hub.base_snapshot(), Some(&base));
        assert_eq!(hub.events(0), events);
        assert_eq!(hub.snapshot_ack(), Some(&ack));
        assert_eq!(hub.checkpoint("project", &writer, 6).unwrap(), checkpoint);
    }

    #[test]
    fn historical_receipts_never_fabricate_events_or_apply_lease_changes() {
        let (source, original, _) = recovery_source();
        let temporary = tempfile::tempdir().unwrap();
        let seq = source.through_seq + (1_u64 << 32);
        let epoch = source.epoch_high_water + (1_u64 << 32);
        let mut hub = Hub::stage_recovery(temporary.path(), &source, seq, epoch).unwrap();
        let invalid_change = Some(LeaseChange::Acquire {
            names: vec![],
            holder: "".into(),
            ttl_ms: 0,
        });
        assert_eq!(
            hub.append_receipt(original.clone(), invalid_change.clone(), u64::MAX)
                .unwrap(),
            AppendReceipt::Duplicate { seq: 2 }
        );
        assert!(hub
            .append(original.clone(), invalid_change, u64::MAX)
            .unwrap_err()
            .contains("append_receipt"));
        assert_eq!(
            hub.append_receipt(historical_derived(), None, 4).unwrap(),
            AppendReceipt::Duplicate { seq: 3 }
        );
        let mut invalid = historical_derived();
        invalid.data["commit"] = serde_json::json!("b".repeat(40));
        assert!(hub.append_receipt(invalid, None, 4).is_err());
        assert_eq!(hub.last_seq(), seq);
        assert_eq!(hub.epoch_high_water(), epoch);
        assert!(hub.events(0).is_empty());
        assert!(hub.leases(4).is_empty());
        drop(hub);
        let mut hub = Hub::open(temporary.path()).unwrap();
        assert_eq!(
            hub.append_receipt(original.clone(), None, 4).unwrap(),
            AppendReceipt::Duplicate { seq: 2 }
        );
        let mut pending = input();
        pending.kind = "future.event".into();
        let AppendReceipt::Event(recorded) = hub.append_receipt(pending.clone(), None, 4).unwrap()
        else {
            panic!("tail needs a body");
        };
        assert_eq!(recorded.seq, seq + 1);
        assert_eq!(hub.receipts()[&original.eid], 2);
        assert_eq!(hub.receipts()[&pending.eid], seq + 1);
        drop(hub);
        let path = temporary.path().join("journal.json");
        let mut damaged: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        damaged["events"][0]["eid"] = serde_json::json!(original.eid);
        fs::write(&path, serde_json::to_vec(&damaged).unwrap()).unwrap();
        assert!(Hub::open(temporary.path()).is_err());
    }

    #[test]
    fn staging_rejects_existing_content_bad_version_and_boundary_overflow() {
        let (source, _, _) = recovery_source();
        let temporary = tempfile::tempdir().unwrap();
        let seq = source.through_seq + (1_u64 << 32);
        let epoch = source.epoch_high_water + (1_u64 << 32);
        for (seq_boundary, epoch_boundary) in [(seq - 1, epoch), (seq, epoch - 1)] {
            assert!(
                Hub::stage_recovery(temporary.path(), &source, seq_boundary, epoch_boundary)
                    .is_err()
            );
            assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 0);
        }
        for field in ["version", "seq", "epoch"] {
            let mut invalid = source.clone();
            match field {
                "version" => invalid.format_version += 1,
                "seq" => invalid.through_seq = u64::MAX,
                _ => invalid.epoch_high_water = u64::MAX,
            }
            assert!(Hub::stage_recovery(temporary.path(), &invalid, u64::MAX, u64::MAX).is_err());
            assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 0);
        }
        let path = temporary.path().join("unrelated");
        fs::write(&path, "keep").unwrap();
        assert!(Hub::stage_recovery(temporary.path(), &source, seq, epoch).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep");
        fs::remove_file(&path).unwrap();
        let hub = Hub::stage_recovery(temporary.path(), &source, seq, epoch).unwrap();
        drop(hub);
        let journal = fs::read(temporary.path().join("journal.json")).unwrap();
        assert!(Hub::stage_recovery(temporary.path(), &source, seq, epoch).is_err());
        assert_eq!(
            fs::read(temporary.path().join("journal.json")).unwrap(),
            journal
        );
    }

    #[test]
    fn recovery_install_holds_target_lock_and_rejects_nonadvancing_or_other_project() {
        let (source, _, _) = recovery_source();
        let target_dir = tempfile::tempdir().unwrap();
        let stage_dir = tempfile::tempdir().unwrap();
        let mut target = Hub::open(target_dir.path()).unwrap();
        target.append(input(), None, 1).unwrap();
        let seq = source.through_seq + (1_u64 << 32);
        let epoch = source.epoch_high_water + (1_u64 << 32);
        let mut staged = Hub::stage_recovery(stage_dir.path(), &source, seq, epoch).unwrap();
        let ack = SnapshotAck {
            commit: "b".repeat(40),
            seq,
            epoch,
        };
        staged.acknowledge(ack.clone()).unwrap();
        let journal_path = target_dir.path().join("journal.json");
        let saved_path = target_dir.path().join("saved.json");
        fs::rename(&journal_path, &saved_path).unwrap();
        fs::create_dir(&journal_path).unwrap();
        assert!(target.install_recovery(&staged).is_err());
        assert!(target.base_snapshot().is_none());
        assert_eq!(target.last_seq(), 1);
        assert!(Hub::open(target_dir.path()).is_err());
        fs::remove_dir(&journal_path).unwrap();
        fs::rename(&saved_path, &journal_path).unwrap();
        target.install_recovery(&staged).unwrap();
        assert!(Hub::open(target_dir.path()).is_err());
        assert_eq!(target.base_snapshot(), staged.base_snapshot());
        assert_eq!(target.snapshot_ack(), Some(&ack));
        assert_eq!(target.last_seq(), seq);
        assert_eq!(target.epoch_high_water(), epoch);
        let journal = fs::read(target_dir.path().join("journal.json")).unwrap();
        assert!(target.install_recovery(&staged).is_err());
        for (seq_boundary, epoch_boundary) in [(seq + 1, epoch), (seq, epoch + 1)] {
            let candidate_dir = tempfile::tempdir().unwrap();
            let candidate =
                Hub::stage_recovery(candidate_dir.path(), &source, seq_boundary, epoch_boundary)
                    .unwrap();
            assert!(target.install_recovery(&candidate).is_err());
        }
        let empty_dir = tempfile::tempdir().unwrap();
        let empty = Hub::open(empty_dir.path()).unwrap();
        assert!(target.install_recovery(&empty).is_err());
        let other_dir = tempfile::tempdir().unwrap();
        let mut other = source.clone();
        other.project_id = "other-project".into();
        let other = Hub::stage_recovery(other_dir.path(), &other, seq + 10, epoch + 10).unwrap();
        assert!(target.install_recovery(&other).is_err());
        assert_eq!(
            fs::read(target_dir.path().join("journal.json")).unwrap(),
            journal
        );
        drop(target);
        let reopened = Hub::open(target_dir.path()).unwrap();
        assert_eq!(reopened.base_snapshot(), staged.base_snapshot());
        assert_eq!(reopened.snapshot_ack(), Some(&ack));
    }

    #[test]
    fn recovery_poison_rejects_duplicates_from_replaced_journal_and_seed() {
        let (old_source, historical, _) = recovery_source();
        let (new_source, _, _) = recovery_source();
        let target_dir = tempfile::tempdir().unwrap();
        let stage_dir = tempfile::tempdir().unwrap();
        let mut target = Hub::stage_recovery(
            target_dir.path(),
            &old_source,
            old_source.through_seq + (1_u64 << 32),
            old_source.epoch_high_water + (1_u64 << 32),
        )
        .unwrap();
        let tail = input();
        target.append(tail.clone(), None, 4).unwrap();
        let staged = Hub::stage_recovery(
            stage_dir.path(),
            &new_source,
            new_source.through_seq + (2_u64 << 32),
            new_source.epoch_high_water + (2_u64 << 32),
        )
        .unwrap();
        assert!(!staged.receipts().contains_key(&historical.eid));
        assert!(!staged.receipts().contains_key(&tail.eid));
        target
            .install_recovery_with_sync(&staged, |_| {
                Err(io::Error::other("injected recovery sync failure"))
            })
            .unwrap_err();
        for duplicate in [&historical, &tail, &historical_derived()] {
            assert!(target
                .append(duplicate.clone(), None, 5)
                .unwrap_err()
                .contains("reopen"));
            assert!(target
                .append_receipt(duplicate.clone(), None, 5)
                .unwrap_err()
                .contains("reopen"));
        }
        drop(target);
        let reopened = Hub::open(target_dir.path()).unwrap();
        assert_eq!(reopened.receipts(), staged.receipts());
        assert!(!reopened.receipts().contains_key(&historical.eid));
        assert!(!reopened.receipts().contains_key(&tail.eid));
    }

    #[test]
    fn recovery_install_sync_failure_retains_memory_and_lock_until_reopen() {
        let (source, _, _) = recovery_source();
        let target_dir = tempfile::tempdir().unwrap();
        let stage_dir = tempfile::tempdir().unwrap();
        let mut target = Hub::open(target_dir.path()).unwrap();
        let first = target
            .append(
                input(),
                Some(LeaseChange::Acquire {
                    names: vec!["main".into()],
                    holder: "old-writer".into(),
                    ttl_ms: 100,
                }),
                1,
            )
            .unwrap();
        let old_ack = SnapshotAck {
            commit: "c".repeat(40),
            seq: 1,
            epoch: 1,
        };
        target.acknowledge(old_ack.clone()).unwrap();
        let old_receipts = target.receipts();
        let old_leases = target.leases(2);
        let staged = Hub::stage_recovery(
            stage_dir.path(),
            &source,
            source.through_seq + (1_u64 << 32),
            source.epoch_high_water + (1_u64 << 32),
        )
        .unwrap();
        let error = target
            .install_recovery_with_sync(&staged, |_| {
                assert!(Hub::open(target_dir.path()).is_err());
                Err(io::Error::other("injected recovery sync failure"))
            })
            .unwrap_err();
        assert!(error.contains("uncertain"));
        assert!(target.base_snapshot().is_none());
        assert_eq!(target.last_seq(), 1);
        assert_eq!(target.epoch_high_water(), 1);
        assert_eq!(target.snapshot_ack(), Some(&old_ack));
        assert_eq!(target.receipts(), old_receipts);
        assert_eq!(target.leases(2), old_leases);
        assert_eq!(target.events(0), vec![first]);
        assert!(target.install_recovery(&staged).is_err());
        assert!(target.append(input(), None, 2).is_err());
        assert!(Hub::open(target_dir.path()).is_err());
        drop(target);
        let reopened = Hub::open(target_dir.path()).unwrap();
        assert_eq!(reopened.base_snapshot(), staged.base_snapshot());
        assert_eq!(reopened.last_seq(), staged.last_seq());
        assert_eq!(reopened.receipts(), staged.receipts());
        assert!(reopened.snapshot_ack().is_none());
        assert!(reopened.leases(2).is_empty());
    }
}
