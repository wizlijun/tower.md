//! Durable business-event producer. A pending request is intent, never a receipt.
//!
//! The daemon owns one instance for the lifetime of its project session. Files
//! are immutable: retries recover the original hub envelope rather than create
//! a new event or replace an earlier (possibly stale) receipt.

use crate::hub_protocol::{AppendReceipt, Event, HubError, HubProtocol, NewEvent};
use fs2::FileExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug)]
pub enum OutboxError {
    Invalid(String),
    Corrupt(String),
    Uncertain(String),
    Hub(HubError),
    /// The original stale receipt is preserved, but cannot certify an action.
    Stale(Box<Event>),
}

impl std::fmt::Display for OutboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) | Self::Corrupt(s) | Self::Uncertain(s) => f.write_str(s),
            Self::Hub(e) => e.fmt(f),
            Self::Stale(e) => write!(f, "event {} was recorded with a stale fence", e.eid),
        }
    }
}
impl std::error::Error for OutboxError {}
impl From<HubError> for OutboxError {
    fn from(value: HubError) -> Self {
        Self::Hub(value)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record<T> {
    version: u32,
    payload: T,
    sha256: String,
}

pub struct DurableOutbox {
    root: PathBuf,
    _lock: File,
    confirmed_seq: BTreeMap<u64, String>,
    confirmed_ids: BTreeMap<String, Event>,
}
impl Drop for DurableOutbox {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._lock);
    }
}

impl DurableOutbox {
    /// `common_dir` is Git's resolved common directory, not a linked checkout's `.git`.
    pub fn open(common_dir: &Path) -> Result<Self, OutboxError> {
        ordinary_dir(common_dir)?;
        let parent = common_dir.join("towermd");
        create_dir(&parent)?;
        let root = parent.join("outbox");
        let new = create_dir(&root)?;
        let lock_path = root.join("lock");
        if present(&lock_path)? {
            ordinary_file(&lock_path)?;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(uncertain)?;
        lock.try_lock_exclusive().map_err(|e| {
            OutboxError::Uncertain(format!("outbox already owned or cannot lock: {e}"))
        })?;
        if new {
            create_dir(&root.join("pending"))?;
            create_dir(&root.join("confirmed"))?;
            install(&root.join("format.json"), &1_u32)?;
            sync_dir(&root)?;
        } else {
            ordinary_dir(&root.join("pending"))?;
            ordinary_dir(&root.join("confirmed"))?;
            let version: u32 = read(&root.join("format.json"))?;
            if version != 1 {
                return Err(OutboxError::Corrupt("unsupported outbox format".into()));
            }
        }
        let mut outbox = Self {
            root,
            _lock: lock,
            confirmed_seq: BTreeMap::new(),
            confirmed_ids: BTreeMap::new(),
        };
        // Reject missing pending originals, malformed records and duplicate seqs
        // before allowing any new event to reach the hub.
        for event in outbox.read_all()?.1 {
            outbox
                .confirmed_ids
                .insert(event.eid.clone(), event.clone());
            outbox.confirmed_seq.insert(event.seq, event.eid);
        }
        Ok(outbox)
    }

    /// Persist intent, append once (or recover its exact original), and persist
    /// the full receipt before returning. Lookup may consult hub events and an
    /// acknowledged snapshot archive; a bare duplicate seq is insufficient.
    pub fn deliver<H, F>(
        &mut self,
        hub: &mut H,
        input: NewEvent,
        now_ms: u64,
        mut lookup: F,
    ) -> Result<Event, OutboxError>
    where
        H: HubProtocol + ?Sized,
        F: FnMut(&mut H, u64, &str) -> Result<Option<Event>, HubError>,
    {
        crate::hub::validate_input(&input)?;
        let pending_path = self.path("pending", &input.eid);
        let confirmed_path = self.path("confirmed", &input.eid);
        if present(&pending_path)? {
            let old: NewEvent = read(&pending_path)?;
            if old != input {
                return Err(OutboxError::Invalid(
                    "event id already has a different pending body".into(),
                ));
            }
            durable(&pending_path)?;
        } else {
            if present(&confirmed_path)? {
                return Err(OutboxError::Corrupt(
                    "confirmed event is missing its pending original".into(),
                ));
            }
            install(&pending_path, &input)?;
        }
        if present(&confirmed_path)? {
            let event: Event = read(&confirmed_path)?;
            check_receipt(&input, &event)?;
            durable(&confirmed_path)?;
            self.remember(&event)?;
            return certified(event);
        } else if self.confirmed_ids.contains_key(&input.eid) {
            return Err(OutboxError::Corrupt(
                "confirmed receipt disappeared during this session".into(),
            ));
        }
        let (event, expected_seq) = match hub.append(input.clone(), now_ms)? {
            AppendReceipt::Event(event) => (*event, None),
            AppendReceipt::Duplicate { seq } => {
                let event = lookup(hub, seq, &input.eid)?.ok_or_else(|| {
                    OutboxError::Uncertain(format!(
                        "original full receipt unavailable for duplicate {} at seq {seq}",
                        input.eid
                    ))
                })?;
                (event, Some(seq))
            }
        };
        check_receipt(&input, &event)?;
        if expected_seq.is_some_and(|seq| seq != event.seq) {
            return Err(OutboxError::Corrupt(
                "duplicate lookup returned another sequence".into(),
            ));
        }
        // An outbox cannot attest two events at the same hub sequence.
        self.check_seq(&event)?;
        install(&confirmed_path, &event)?;
        self.remember(&event)?;
        certified(event)
    }

    /// Unconfirmed intents only. These must be delivered, never projected as
    /// successful actions. Confirmed stale events are not pending retries.
    /// A receipt is filed under its intent's name, so only intents without
    /// one are read; `open` already checked every delivered pair.
    pub fn pending(&self) -> Result<Vec<NewEvent>, OutboxError> {
        let confirmed: BTreeSet<_> = records(&self.root.join("confirmed"))?
            .into_iter()
            .filter_map(|path| path.file_name().map(ToOwned::to_owned))
            .collect();
        let mut pending = Vec::new();
        for path in records(&self.root.join("pending"))? {
            if path.file_name().is_some_and(|name| confirmed.contains(name)) {
                continue;
            }
            let input: NewEvent = read(&path)?;
            crate::hub::validate_input(&input).map_err(|e| OutboxError::Corrupt(e.to_string()))?;
            check_name(&path, &input.eid)?;
            pending.push(input);
        }
        pending.sort_by(|a, b| a.eid.cmp(&b.eid));
        Ok(pending)
    }

    /// Complete original envelopes, including stale receipts, ordered by seq.
    pub fn confirmed(&self) -> Result<Vec<Event>, OutboxError> {
        Ok(self.read_all()?.1)
    }

    pub fn confirmed_by_eid(&self, eid: &str) -> Result<Option<Event>, OutboxError> {
        let path = self.path("confirmed", eid);
        if !present(&path)? {
            return if self.confirmed_ids.contains_key(eid) {
                Err(OutboxError::Corrupt(
                    "confirmed receipt disappeared during this session".into(),
                ))
            } else {
                Ok(None)
            };
        }
        let event: Event = read(&path)?;
        let input: NewEvent = read(&self.path("pending", eid))?;
        check_name(&path, &event.eid)?;
        check_receipt(&input, &event)?;
        self.check_seq(&event)?;
        Ok(Some(event))
    }

    fn check_seq(&self, event: &Event) -> Result<(), OutboxError> {
        if self
            .confirmed_seq
            .get(&event.seq)
            .is_some_and(|eid| eid != &event.eid)
        {
            return Err(OutboxError::Corrupt(
                "hub sequence conflicts with an existing receipt".into(),
            ));
        }
        if self
            .confirmed_ids
            .get(&event.eid)
            .is_some_and(|old| old != event)
        {
            return Err(OutboxError::Corrupt(
                "confirmed original changed during this session".into(),
            ));
        }
        Ok(())
    }

    fn remember(&mut self, event: &Event) -> Result<(), OutboxError> {
        self.check_seq(event)?;
        self.confirmed_ids.insert(event.eid.clone(), event.clone());
        self.confirmed_seq.insert(event.seq, event.eid.clone());
        Ok(())
    }

    fn path(&self, kind: &str, eid: &str) -> PathBuf {
        self.root.join(kind).join(filename(eid))
    }

    fn read_all(&self) -> Result<(Vec<NewEvent>, Vec<Event>), OutboxError> {
        let mut pending = Vec::new();
        for path in records(&self.root.join("pending"))? {
            let input: NewEvent = read(&path)?;
            crate::hub::validate_input(&input).map_err(|e| OutboxError::Corrupt(e.to_string()))?;
            check_name(&path, &input.eid)?;
            pending.push(input);
        }
        let mut confirmed = Vec::new();
        let mut seqs = BTreeSet::new();
        for path in records(&self.root.join("confirmed"))? {
            let event: Event = read(&path)?;
            check_name(&path, &event.eid)?;
            let input = pending.iter().find(|p| p.eid == event.eid).ok_or_else(|| {
                OutboxError::Corrupt("confirmed event is missing its pending original".into())
            })?;
            check_receipt(input, &event)?;
            if !seqs.insert(event.seq) {
                return Err(OutboxError::Corrupt(
                    "duplicate sequence in confirmed receipts".into(),
                ));
            }
            confirmed.push(event);
        }
        pending.sort_by(|a, b| a.eid.cmp(&b.eid));
        confirmed.sort_by_key(|e| e.seq);
        Ok((pending, confirmed))
    }
}

fn certified(event: Event) -> Result<Event, OutboxError> {
    if event.stale {
        Err(OutboxError::Stale(Box::new(event)))
    } else {
        Ok(event)
    }
}

fn check_receipt(input: &NewEvent, event: &Event) -> Result<(), OutboxError> {
    let envelope = NewEvent {
        eid: event.eid.clone(),
        host: event.host.clone(),
        actor: event.actor.clone(),
        kind: event.kind.clone(),
        layer: event.layer.clone(),
        obj: event.obj.clone(),
        data: event.data.clone(),
        guards: event.guards.clone(),
    };
    if &envelope != input {
        return Err(OutboxError::Corrupt(
            "hub receipt does not match the complete pending body".into(),
        ));
    }
    if event.seq == 0
        || chrono::DateTime::parse_from_rfc3339(&event.t).is_err()
        || event.epoch != event.guards.first().map(|f| f.epoch)
        || event.cost_usd.is_some()
        || event.lease_delta.is_some()
        || event.heartbeat
        || (event.stale && event.guards.is_empty())
    {
        return Err(OutboxError::Corrupt(
            "invalid business-event receipt envelope".into(),
        ));
    }
    Ok(())
}

fn filename(eid: &str) -> String {
    format!("{}.json", hex::encode(Sha256::digest(eid.as_bytes())))
}

fn check_name(path: &Path, eid: &str) -> Result<(), OutboxError> {
    if path.file_name().and_then(|s| s.to_str()) != Some(filename(eid).as_str()) {
        return Err(OutboxError::Corrupt(
            "outbox filename does not match event id".into(),
        ));
    }
    Ok(())
}

fn uncertain(e: impl std::fmt::Display) -> OutboxError {
    OutboxError::Uncertain(e.to_string())
}

fn present(path: &Path) -> Result<bool, OutboxError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(uncertain(e)),
    }
}

fn ordinary_dir(path: &Path) -> Result<(), OutboxError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| OutboxError::Corrupt(format!("{}: {e}", path.display())))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(OutboxError::Corrupt(format!(
            "outbox path must be an ordinary directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn ordinary_file(path: &Path) -> Result<(), OutboxError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| OutboxError::Corrupt(format!("{}: {e}", path.display())))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(OutboxError::Corrupt(format!(
            "outbox record must be an ordinary file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn create_dir(path: &Path) -> Result<bool, OutboxError> {
    match fs::create_dir(path) {
        Ok(()) => {
            sync_dir(path.parent().unwrap())?;
            Ok(true)
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            ordinary_dir(path)?;
            Ok(false)
        }
        Err(e) => Err(uncertain(e)),
    }
}

fn sync_dir(path: &Path) -> Result<(), OutboxError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(uncertain)
}

fn durable(path: &Path) -> Result<(), OutboxError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(uncertain)?;
    sync_dir(path.parent().unwrap())
}

fn records(path: &Path) -> Result<Vec<PathBuf>, OutboxError> {
    ordinary_dir(path)?;
    let mut paths = Vec::new();
    for entry in fs::read_dir(path).map_err(uncertain)? {
        let entry = entry.map_err(uncertain)?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| OutboxError::Corrupt("non-UTF8 filename in outbox".into()))?;
        // Interrupted atomic installs may leave scratch files. They are never
        // intents or receipts and cannot be replayed.
        if name.starts_with(".tmp-") && Uuid::parse_str(&name[5..]).is_ok() {
            ordinary_file(&entry.path())?;
            continue;
        }
        let hash = name.strip_suffix(".json").unwrap_or("");
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(OutboxError::Corrupt("unexpected file in outbox".into()));
        }
        ordinary_file(&entry.path())?;
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

fn read<T: DeserializeOwned + Serialize>(path: &Path) -> Result<T, OutboxError> {
    ordinary_file(path)?;
    let bytes = fs::read(path).map_err(uncertain)?;
    let record: Record<T> = serde_json::from_slice(&bytes)
        .map_err(|e| OutboxError::Corrupt(format!("{}: {e}", path.display())))?;
    let hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&record.payload).map_err(uncertain)?,
    ));
    if record.version != 1 || record.sha256 != hash {
        return Err(OutboxError::Corrupt(format!(
            "invalid outbox record digest or version: {}",
            path.display()
        )));
    }
    Ok(record.payload)
}

fn install<T: Serialize>(path: &Path, value: &T) -> Result<(), OutboxError> {
    let bytes = serde_json::to_vec(value).map_err(uncertain)?;
    let record = Record {
        version: 1,
        payload: value,
        sha256: hex::encode(Sha256::digest(&bytes)),
    };
    let parent = path.parent().unwrap();
    ordinary_dir(parent)?;
    let temporary = parent.join(format!(".tmp-{}", Uuid::now_v7()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(uncertain)?;
        file.write_all(&serde_json::to_vec(&record).map_err(uncertain)?)
            .map_err(uncertain)?;
        file.sync_all().map_err(uncertain)?;
        // Unlike rename, a hard link never overwrites an existing receipt.
        fs::hard_link(&temporary, path).map_err(uncertain)?;
        sync_dir(parent)?;
        fs::remove_file(&temporary).map_err(uncertain)?;
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_releases_lock_even_when_an_inherited_descriptor_remains_open() {
        let dir = tempfile::tempdir().unwrap();
        let outbox = DurableOutbox::open(dir.path()).unwrap();
        let inherited = outbox._lock.try_clone().unwrap();
        assert!(DurableOutbox::open(dir.path()).is_err());
        drop(outbox);
        let reopened = DurableOutbox::open(dir.path()).unwrap();
        drop(inherited);
        assert!(DurableOutbox::open(dir.path()).is_err());
        drop(reopened);
        assert!(DurableOutbox::open(dir.path()).is_ok());
    }
}
