//! Project-bound producer durability. No history/proof can be minted from these inputs.
use crate::recovery_contract::{canonical_json_bytes, sha256};
use crate::{
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError, HubProtocol, NewEvent},
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Process-local uncertainty shared by an open project's writers and read-only
/// queries. It can only withhold receipts; it is never accepted-history authority.
#[derive(Clone, Default)]
pub struct OutboxUncertainty {
    confirmations: Arc<Mutex<BTreeSet<String>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryContext {
    project_id: String,
    docs_dir: String,
    hub: serde_json::Value,
}
impl RecoveryContext {
    pub fn from_catalog(catalog: &Catalog) -> Result<Self, String> {
        let (common_dir, config) = Catalog::configuration_at_authority(&catalog.root)?;
        let context = Self::extract_config(&config)?;
        let supplied = Self::extract(catalog)?;
        if context.project_id != supplied.project_id
            || context.wire() != supplied.wire()
            || common_dir != catalog.common_dir
        {
            return Err("outbox context differs from authoritative Catalog".into());
        }
        Ok(context)
    }
    pub(crate) fn extract(catalog: &Catalog) -> Result<Self, String> {
        Self::extract_config(&catalog.config)
    }
    fn extract_config(config: &crate::config::Config) -> Result<Self, String> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(&config.to_yaml()).map_err(|e| e.to_string())?;
        let hub = serde_json::to_value(value.get("hub").ok_or("missing hub configuration")?)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            project_id: config.project_id().into(),
            docs_dir: config.docs_dir().into(),
            hub,
        })
    }
    fn wire(&self) -> ContextWire {
        ContextWire {
            docs_dir: self.docs_dir.clone(),
            hub: self.hub.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ContextWire {
    docs_dir: String,
    hub: serde_json::Value,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Envelope {
    Pending {
        schema: u32,
        project_id: String,
        context: ContextWire,
        #[serde(deserialize_with = "deserialize_original")]
        original: NewEvent,
    },
    Confirmed {
        schema: u32,
        project_id: String,
        context: ContextWire,
        #[serde(deserialize_with = "deserialize_original")]
        original: NewEvent,
        #[serde(deserialize_with = "deserialize_actual")]
        actual: Box<Event>,
    },
}
#[derive(Clone)]
pub struct RecoveryInputs {
    pending: Vec<NewEvent>,
    confirmed: Vec<Event>,
    digest: String,
    records: Vec<(String, usize, serde_json::Value)>,
    high_water: (u64, u64),
    directory: PathBuf,
    context: RecoveryContext,
    uncertainty: OutboxUncertainty,
}
impl RecoveryInputs {
    /// Compare an already prepared Catalog without Git I/O under a Session lock.
    /// This is not an authority probe; preparation must still call `probe`.
    pub(crate) fn check_prepared_context(&self, catalog: &Catalog) -> Result<(), String> {
        if self.context != RecoveryContext::extract(catalog)? {
            return Err("recovery inputs differ from prepared Catalog context".into());
        }
        Ok(())
    }
    /// Read actual project inputs without creating files, directories, or taking a writer lock.
    pub fn read(directory: &Path, catalog: &Catalog) -> Result<Self, String> {
        Self::read_with_uncertainty(directory, catalog, OutboxUncertainty::default())
    }
    pub fn read_with_uncertainty(
        directory: &Path,
        catalog: &Catalog,
        uncertainty: OutboxUncertainty,
    ) -> Result<Self, String> {
        let mut inputs = read_directory(directory, &RecoveryContext::from_catalog(catalog)?)?;
        inputs.uncertainty = uncertainty;
        Ok(inputs)
    }
    pub(crate) fn probe(&self, catalog: &Catalog) -> Result<(), String> {
        let context = RecoveryContext::from_catalog(catalog)?;
        if context != self.context {
            return Err("recovery inputs project/context changed".into());
        }
        let current = read_directory(&self.directory, &context)?;
        if current.digest != self.digest {
            return Err("recovery inputs changed during preparation".into());
        }
        Ok(())
    }

    // Routing only: this checks the bound physical log, not current authority.
    // Any business operation must still use probe and the normal source/CAS path.
    pub(crate) fn shares_idle_source(&self, other: &Self) -> bool {
        Arc::ptr_eq(
            &self.uncertainty.confirmations,
            &other.uncertainty.confirmations,
        ) && self.validate_task_writes(other, &[]).is_ok()
    }

    pub(crate) fn unchanged_for_idle_maintenance(&self) -> Result<bool, String> {
        if !self.pending.is_empty()
            || !self
                .uncertainty
                .confirmations
                .lock()
                .map_err(|_| "outbox uncertainty lock poisoned")?
                .is_empty()
        {
            return Ok(false);
        }
        let current = read_directory(&self.directory, &self.context)?;
        Ok(current.digest == self.digest && current.pending.is_empty())
    }

    pub(crate) fn reload(&self, catalog: &Catalog) -> Result<Self, String> {
        Self::read_with_uncertainty(&self.directory, catalog, self.uncertainty.clone())
    }

    /// Physical inputs only, without authority. The shared Session preparation
    /// must still probe the actual context before returning or installing them.
    pub(crate) fn reread_bound(&self) -> Result<Self, String> {
        let mut inputs = read_directory(&self.directory, &self.context)?;
        inputs.uncertainty = self.uncertainty.clone();
        Ok(inputs)
    }

    /// Keep the Session and its project writers on the same withholding signal.
    pub fn outbox_uncertainty(&self) -> OutboxUncertainty {
        self.uncertainty.clone()
    }

    pub fn pending(&self) -> &[NewEvent] {
        &self.pending
    }
    pub fn confirmed(&self) -> &[Event] {
        &self.confirmed
    }
    pub(crate) fn receipt_confirmed(&self, actual: &Event) -> bool {
        self.uncertainty
            .confirmations
            .lock()
            .is_ok_and(|uncertain| !uncertain.contains(&actual.eid))
            && self.confirmed.contains(actual)
    }
    /// Reconcile only the physical records written by this actual task append scope.
    /// This predicts bytes' canonical digest; it constructs no accepted-history capability.
    pub(crate) fn validate_task_writes(
        &self,
        after: &Self,
        writes: &[(&NewEvent, Option<&Event>)],
    ) -> Result<(), String> {
        if self.context != after.context || self.directory != after.directory {
            return Err("task outbox source identity changed".into());
        }
        let mut records = self.records.clone();
        let mut pending = self
            .pending
            .iter()
            .map(|event| (event.eid.clone(), event.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut confirmed = self
            .confirmed
            .iter()
            .map(|event| (event.eid.clone(), event.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut append = |original: &NewEvent, envelope: Envelope| -> Result<(), String> {
            let name = host_file(&original.host);
            let position = records
                .iter()
                .filter(|record| record.0 == name)
                .map(|record| record.1 + 1)
                .max()
                .unwrap_or(0);
            records.push((
                name,
                position,
                serde_json::to_value(envelope).map_err(|error| error.to_string())?,
            ));
            Ok(())
        };
        for (original, actual) in writes {
            if pending
                .get(&original.eid)
                .is_some_and(|known| known != *original)
                || confirmed
                    .get(&original.eid)
                    .is_some_and(|actual| original_of(actual) != **original)
            {
                return Err("task source scope changed an original event".into());
            }
            if !pending.contains_key(&original.eid) && !confirmed.contains_key(&original.eid) {
                append(
                    original,
                    Envelope::Pending {
                        schema: 1,
                        project_id: self.context.project_id.clone(),
                        context: self.context.wire(),
                        original: (*original).clone(),
                    },
                )?;
                pending.insert(original.eid.clone(), (*original).clone());
            }
            if let Some(actual) = actual {
                validate_actual(original, actual)?;
                if let Some(previous) = confirmed.get(&original.eid) {
                    if previous != *actual {
                        return Err("task source scope changed an actual receipt".into());
                    }
                } else {
                    append(
                        original,
                        Envelope::Confirmed {
                            schema: 1,
                            project_id: self.context.project_id.clone(),
                            context: self.context.wire(),
                            original: (*original).clone(),
                            actual: Box::new((*actual).clone()),
                        },
                    )?;
                    confirmed.insert(original.eid.clone(), (*actual).clone());
                    pending.remove(&original.eid);
                }
            }
        }
        records.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let digest = records_digest(&self.context, &records)?;
        if digest != after.digest {
            return Err(
                "recovery inputs changed during preparation: outside exact task producer records"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    // Combine sparse maxima with the actual snapshot boundary.
    pub(crate) fn recovery_boundaries(
        &self,
        source_seq: u64,
        source_epoch: u64,
    ) -> Result<(u64, u64), String> {
        Ok((
            source_seq
                .max(self.high_water.0)
                .checked_add(1u64 << 32)
                .ok_or("recovery sequence overflow")?,
            source_epoch
                .max(self.high_water.1)
                .checked_add(1u64 << 32)
                .ok_or("recovery epoch overflow")?,
        ))
    }
    /// Sparse original receipt maxima, not a continuous archive prefix.
    pub fn high_water(&self) -> (u64, u64) {
        self.high_water
    }
}
pub struct RecoveryOutbox {
    directory: PathBuf,
    context: RecoveryContext,
    _lock: File,
    // A failed fsync can leave a complete readable line. Remember that failure
    // only to withhold success until this exact confirmation is fsynced again.
    uncertainty: OutboxUncertainty,
    #[cfg(test)]
    fault: Option<Fault>,
}
#[cfg(test)]
#[derive(Clone, Copy)]
enum Fault {
    PendingSync,
    ConfirmedSync,
    PartialPending,
    PartialConfirmed,
    ConfirmedSyncAfter(usize),
}

impl RecoveryOutbox {
    pub fn open(directory: &Path, context: RecoveryContext) -> Result<Self, String> {
        Self::open_with_uncertainty(directory, context, OutboxUncertainty::default())
    }
    pub fn open_with_uncertainty(
        directory: &Path,
        context: RecoveryContext,
        uncertainty: OutboxUncertainty,
    ) -> Result<Self, String> {
        create_directory(directory)?;
        let lock = open_regular(&directory.join(".lock"))?;
        lock.try_lock_exclusive()
            .map_err(|e| format!("outbox is already open: {e}"))?;
        let result = Self {
            directory: directory.to_path_buf(),
            context,
            _lock: lock,
            uncertainty,
            #[cfg(test)]
            fault: None,
        };
        result.read_inputs()?;
        Ok(result)
    }
    pub fn append_pending(&mut self, original: &NewEvent) -> Result<(), String> {
        crate::hub::validate_input(original).map_err(|e| e.to_string())?;
        if original.kind.starts_with("lease.") {
            return Err("lease events use dedicated HubProtocol methods".into());
        }
        let input = self.read_inputs()?;
        let existing = input
            .pending
            .iter()
            .find(|e| e.eid == original.eid)
            .cloned()
            .or_else(|| {
                input
                    .confirmed
                    .iter()
                    .find(|e| e.eid == original.eid)
                    .map(original_of)
            });
        if existing.as_ref().is_some_and(|e| e != original) {
            return Err("conflicting original event".into());
        }
        if input.confirmed.iter().any(|e| e.eid == original.eid) {
            return self.sync_host(&original.host, false);
        }
        if input.pending.iter().any(|e| e == original) {
            return self.sync_host(&original.host, false);
        }
        self.append_line(
            &Envelope::Pending {
                schema: 1,
                project_id: self.context.project_id.clone(),
                context: self.context.wire(),
                original: original.clone(),
            },
            &original.host,
        )
    }
    pub fn read_inputs(&self) -> Result<RecoveryInputs, String> {
        let mut inputs = read_directory(&self.directory, &self.context)?;
        inputs.uncertainty = self.uncertainty.clone();
        Ok(inputs)
    }
    #[cfg(test)]
    pub(crate) fn fail_next_confirmation(&mut self) {
        self.fault = Some(Fault::ConfirmedSync);
    }
    #[cfg(test)]
    pub(crate) fn fail_confirmation_after(&mut self, successful_confirmations: usize) {
        self.fault = Some(Fault::ConfirmedSyncAfter(successful_confirmations));
    }
    #[cfg(test)]
    pub(crate) fn fail_next_pending(&mut self) {
        self.fault = Some(Fault::PendingSync);
    }
    fn sync_host(&mut self, host: &str, confirmed: bool) -> Result<(), String> {
        let file = open_regular(&self.directory.join(host_file(host)))?;
        #[cfg(test)]
        if self.consume_fault(confirmed).is_some() {
            return Err("injected outbox fsync failure".into());
        }
        #[cfg(not(test))]
        let _ = confirmed;
        #[cfg(test)]
        if confirmed {
            CONFIRMATION_OBSERVER.with(|observer| {
                if let Some(observe) = observer.borrow_mut().take() {
                    observe();
                }
            });
        }
        file.sync_all().map_err(|e| e.to_string())
    }
    #[cfg(test)]
    fn consume_fault(&mut self, confirmed: bool) -> Option<Fault> {
        if confirmed {
            if let Some(Fault::ConfirmedSyncAfter(remaining)) = self.fault {
                if remaining > 0 {
                    self.fault = Some(Fault::ConfirmedSyncAfter(remaining - 1));
                    return None;
                }
                self.fault = Some(Fault::ConfirmedSync);
            }
        }
        let matches = self.fault.is_some_and(|f| {
            matches!(
                (confirmed, f),
                (false, Fault::PendingSync | Fault::PartialPending)
                    | (true, Fault::ConfirmedSync | Fault::PartialConfirmed)
            )
        });
        if matches {
            self.fault.take()
        } else {
            None
        }
    }
    fn append_line(&mut self, envelope: &Envelope, host: &str) -> Result<(), String> {
        let path = self.directory.join(host_file(host));
        if path.exists() {
            let bytes = fs::read(&path).map_err(|e| e.to_string())?;
            if !bytes.is_empty() && !bytes.ends_with(b"\n") {
                return Err("torn outbox tail requires repair before append".into());
            }
        }
        let mut file = open_regular(&path)?;
        let mut bytes = canonical_json_bytes(envelope)?;
        bytes.push(b'\n');
        #[cfg(test)]
        if let Some(fault) = self.consume_fault(matches!(envelope, Envelope::Confirmed { .. })) {
            let partial = matches!(fault, Fault::PartialPending | Fault::PartialConfirmed);
            file.write_all(if partial {
                &bytes[..bytes.len() / 2]
            } else {
                &bytes
            })
            .map_err(|e| e.to_string())?;
            return Err("injected outbox write/fsync uncertainty".into());
        }
        file.write_all(&bytes)
            .and_then(|_| {
                #[cfg(test)]
                if matches!(envelope, Envelope::Confirmed { .. }) {
                    CONFIRMATION_OBSERVER.with(|observer| {
                        if let Some(observe) = observer.borrow_mut().take() {
                            observe();
                        }
                    });
                }
                file.sync_all()
            })
            .map_err(|e| format!("outbox write/fsync uncertain: {e}"))
    }
    // E0 delivers durability before P2 wires any producer to it.
    #[allow(
        dead_code,
        reason = "P2/E1 will consume this core-only durability boundary"
    )]
    pub(crate) fn persist_confirmed(
        &mut self,
        original: &NewEvent,
        actual: &Event,
    ) -> Result<(), String> {
        validate_actual(original, actual)?;
        let inputs = self.read_inputs()?;
        if inputs
            .pending
            .iter()
            .any(|e| e.eid == original.eid && e != original)
        {
            return Err("conflicting pending original".into());
        }
        if let Some(previous) = inputs.confirmed.iter().find(|e| e.eid == original.eid) {
            if previous != actual {
                return Err("conflicting original receipt".into());
            }
            self.mark_confirmation_uncertain(&actual.eid)?;
            let result = self.sync_host(&original.host, true);
            self.note_confirmation_result(&actual.eid, &result)?;
            return result;
        }
        if inputs
            .confirmed
            .iter()
            .any(|e| e.seq == actual.seq && e.eid != actual.eid)
        {
            return Err("reused original receipt sequence".into());
        }
        self.mark_confirmation_uncertain(&actual.eid)?;
        let result = self.append_line(
            &Envelope::Confirmed {
                schema: 1,
                project_id: self.context.project_id.clone(),
                context: self.context.wire(),
                original: original.clone(),
                actual: Box::new(actual.clone()),
            },
            &original.host,
        );
        self.note_confirmation_result(&actual.eid, &result)?;
        result
    }
    fn mark_confirmation_uncertain(&self, eid: &str) -> Result<(), String> {
        self.uncertainty
            .confirmations
            .lock()
            .map_err(|_| "outbox uncertainty lock poisoned")?
            .insert(eid.into());
        Ok(())
    }
    fn note_confirmation_result(
        &self,
        eid: &str,
        result: &Result<(), String>,
    ) -> Result<(), String> {
        let mut uncertain = self
            .uncertainty
            .confirmations
            .lock()
            .map_err(|_| "outbox uncertainty lock poisoned")?;
        if result.is_ok() {
            uncertain.remove(eid);
        } else {
            uncertain.insert(eid.into());
        }
        Ok(())
    }
}
#[cfg(test)]
thread_local! {
    pub(crate) static CONFIRMATION_OBSERVER: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}
fn read_directory(
    directory: &Path,
    expected_context: &RecoveryContext,
) -> Result<RecoveryInputs, String> {
    let mut paths = match fs::symlink_metadata(directory) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
            return Err("outbox directory is not a real directory".into())
        }
        Ok(_) => fs::read_dir(directory)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
    };
    paths.sort_by_key(|p| p.file_name());
    let mut originals = BTreeMap::<String, NewEvent>::new();
    let mut confirmed = BTreeMap::<String, Event>::new();
    let mut sequences = BTreeMap::<u64, String>::new();
    let mut digest = Vec::new();
    for entry in paths {
        if entry.file_name() == ".lock" {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "non-UTF8 outbox file name")?;
        if !name.ends_with(".jsonl") {
            return Err("unknown outbox file".into());
        }
        regular(&entry.path())?;
        let bytes = fs::read(entry.path()).map_err(|e| e.to_string())?;
        for (position, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
            if !line.ends_with(b"\n") {
                digest.push((
                    name.clone(),
                    position,
                    serde_json::json!({"torn":hex::encode(line)}),
                ));
                continue;
            } // Retain a torn final record for explicit repair.
            let envelope: Envelope = serde_json::from_slice(line)
                .map_err(|e| format!("{name}:{}: {e}", position + 1))?;
            let canonical = serde_json::to_value(&envelope).map_err(|e| e.to_string())?;
            let (schema, project, context, original, actual) = match &envelope {
                Envelope::Pending {
                    schema,
                    project_id,
                    context,
                    original,
                } => (*schema, project_id, context, original, None),
                Envelope::Confirmed {
                    schema,
                    project_id,
                    context,
                    original,
                    actual,
                } => (*schema, project_id, context, original, Some(actual)),
            };
            if schema != 1
                || project != &expected_context.project_id
                || context != &expected_context.wire()
            {
                return Err("outbox schema/project/context mismatch".into());
            }
            if host_file(&original.host) != name {
                return Err("outbox host/file mismatch".into());
            }
            // Legacy heartbeats never become recovery facts.
            if original.kind == "lease.renewed" {
                digest.push((name.clone(), position, canonical));
                continue;
            }
            validate_original(original)?;
            if let Some(previous) = originals.insert(original.eid.clone(), original.clone()) {
                if previous != *original {
                    return Err("conflicting outbox original eid".into());
                }
            }
            if let Some(actual) = actual {
                validate_actual(original, actual)?;
                if confirmed
                    .get(&original.eid)
                    .is_some_and(|previous| previous != actual.as_ref())
                {
                    return Err("conflicting original receipt".into());
                }
                if sequences
                    .insert(actual.seq, actual.eid.clone())
                    .is_some_and(|eid| eid != actual.eid)
                {
                    return Err("reused original receipt sequence".into());
                }
                confirmed.insert(original.eid.clone(), (**actual).clone());
            }
            digest.push((name.clone(), position, canonical));
        }
    }
    let high_water = confirmed.values().fold((0, 0), |(seq, epoch), event| {
        (
            seq.max(event.seq),
            event
                .guards
                .iter()
                .map(|f| f.epoch)
                .chain(event.epoch)
                .fold(epoch, u64::max),
        )
    });
    let pending = originals
        .into_iter()
        .filter(|(id, _)| !confirmed.contains_key(id))
        .map(|(_, e)| e)
        .collect();
    let mut confirmed = confirmed.into_values().collect::<Vec<_>>();
    confirmed.sort_by_key(|e| e.seq);
    Ok(RecoveryInputs {
        uncertainty: OutboxUncertainty::default(),
        pending,
        confirmed,
        digest: records_digest(expected_context, &digest)?,
        records: digest,
        high_water,
        directory: directory.to_path_buf(),
        context: expected_context.clone(),
    })
}

fn records_digest(
    context: &RecoveryContext,
    records: &[(String, usize, serde_json::Value)],
) -> Result<String, String> {
    Ok(sha256(&canonical_json_bytes(
        &serde_json::json!({"schema":1,"project_id":context.project_id,"context":context.wire(),"records":records}),
    )?))
}

fn host_file(host: &str) -> String {
    format!("{}.jsonl", hex::encode(host.as_bytes()))
}
fn regular(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("outbox path is not a regular file".into());
    }
    Ok(())
}
fn open_regular(path: &Path) -> Result<File, String> {
    let file = if path.symlink_metadata().is_ok() {
        regular(path)?;
        OpenOptions::new().append(true).read(true).open(path)
    } else {
        OpenOptions::new()
            .create_new(true)
            .append(true)
            .read(true)
            .open(path)
    }
    .map_err(|e| e.to_string())?;
    // A previous creation sync may have failed after leaving this path behind.
    // Existence alone never proves the file or its parent directory entry durable.
    sync_creation(&file, path, CreationSync::File)?;
    sync_parent(path)?;
    Ok(file)
}
fn create_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err("outbox directory is not a real directory".into());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or("outbox directory has no parent")?;
            create_directory(parent)?;
            fs::create_dir(path).map_err(|e| e.to_string())?;
        }
        Err(e) => return Err(e.to_string()),
    }
    // Retrying nested creation repairs the highest existing unfinished layer
    // before its caller creates descendants; no in-memory success marker is needed.
    sync_creation(
        &File::open(path).map_err(|e| e.to_string())?,
        path,
        CreationSync::Directory,
    )?;
    sync_parent(path)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CreationSync {
    File,
    Directory,
    Parent,
}
fn sync_creation(file: &File, path: &Path, point: CreationSync) -> Result<(), String> {
    #[cfg(test)]
    if CREATION_FAULT.with(|fault| {
        let mut fault = fault.borrow_mut();
        if fault
            .as_ref()
            .is_some_and(|(target, stage)| target == path && *stage == point)
        {
            fault.take();
            true
        } else {
            false
        }
    }) {
        return Err(format!(
            "injected creation {point:?} fsync failure: {}",
            path.display()
        ));
    }
    file.sync_all()
        .map_err(|e| format!("outbox {point:?} fsync failed at {}: {e}", path.display()))?;
    #[cfg(test)]
    CREATION_COMPLETED.with(|events| events.borrow_mut().push((path.to_path_buf(), point)));
    Ok(())
}
fn sync_parent(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    sync_creation(
        &File::open(parent).map_err(|e| e.to_string())?,
        path,
        CreationSync::Parent,
    )
}
#[cfg(test)]
thread_local! {
    static CREATION_FAULT: std::cell::RefCell<Option<(PathBuf,CreationSync)>>=const { std::cell::RefCell::new(None) };
    static CREATION_COMPLETED: std::cell::RefCell<Vec<(PathBuf,CreationSync)>>=const { std::cell::RefCell::new(Vec::new()) };
}

// Deliberately private to core; no daemon/client may declare a confirmed fact.
#[allow(dead_code, reason = "P2 producer wiring is outside the E0 scope")]
pub(crate) fn append_durable(
    hub: &mut dyn HubProtocol,
    outbox: &mut RecoveryOutbox,
    original: NewEvent,
    now_ms: u64,
) -> Result<AppendReceipt, HubError> {
    append_durable_inner(hub, outbox, original, now_ms, None)
}

// Only the actual-history reader can supply an archived original receipt. This
// narrow coldstart fallback never synthesizes a confirmed record from a sequence.
pub(crate) fn append_coldstart_durable(
    hub: &mut dyn HubProtocol,
    outbox: &mut RecoveryOutbox,
    original: NewEvent,
    now_ms: u64,
    history: &crate::recovery_git::VerifiedRecoveryHistory,
) -> Result<AppendReceipt, HubError> {
    if original.kind != "coldstart.reported"
        && !(original.kind == "evidence.recorded"
            && original.data["evidence"]["coldstart"]["schema"] == 1)
    {
        return Err(HubError::Invalid(
            "coldstart durability requires coldstart event".into(),
        ));
    }
    let actual = history.coldstart_original(&original.eid);
    append_durable_inner(hub, outbox, original, now_ms, actual)
}

// Only a complete verified actual outer schedule can issue this capability.
// Pending intents and duplicate sequence numbers never supply Event bodies.
pub(crate) fn append_task_durable(
    hub: &mut dyn HubProtocol,
    outbox: &mut RecoveryOutbox,
    original: NewEvent,
    now_ms: u64,
    receipt: &crate::recovery_git::VerifiedTaskReceipt,
) -> Result<AppendReceipt, HubError> {
    if !matches!(
        original.kind.as_str(),
        "task.state" | "task.claimed" | "task.released"
    ) {
        return Err(HubError::Invalid(
            "task durability requires task event".into(),
        ));
    }
    let actual = receipt
        .event_for(&outbox.context.project_id, &original)
        .map_err(HubError::Invalid)?;
    append_durable_inner(hub, outbox, original, now_ms, Some(actual))
}

fn append_durable_inner(
    hub: &mut dyn HubProtocol,
    outbox: &mut RecoveryOutbox,
    original: NewEvent,
    now_ms: u64,
    archived_actual: Option<&Event>,
) -> Result<AppendReceipt, HubError> {
    outbox.append_pending(&original).map_err(HubError::Io)?;
    // A confirmed original is history, including after hub replacement. Never replay it as a producer retry.
    if let Some(actual) = outbox
        .read_inputs()
        .map_err(HubError::Io)?
        .confirmed
        .into_iter()
        .find(|e| e.eid == original.eid)
    {
        outbox
            .persist_confirmed(&original, &actual)
            .map_err(HubError::Uncertain)?;
        return Ok(AppendReceipt::Event(Box::new(actual)));
    }
    if let Some(actual) = archived_actual {
        outbox
            .persist_confirmed(&original, actual)
            .map_err(HubError::Uncertain)?;
        return Ok(AppendReceipt::Event(Box::new(actual.clone())));
    }
    let receipt = hub
        .append(original.clone(), now_ms)
        .map_err(|e| HubError::Uncertain(e.to_string()))?;
    let actual = match receipt {
        AppendReceipt::Event(actual) => actual,
        AppendReceipt::Duplicate { seq } => {
            let inputs = outbox.read_inputs().map_err(HubError::Uncertain)?;
            Box::new(
                inputs
                    .confirmed
                    .into_iter()
                    .find(|e| e.eid == original.eid && e.seq == seq)
                    .ok_or_else(|| {
                        HubError::Uncertain("duplicate has no original receipt".into())
                    })?,
            )
        }
    };
    outbox
        .persist_confirmed(&original, &actual)
        .map_err(HubError::Uncertain)?;
    Ok(AppendReceipt::Event(actual))
}

pub(crate) fn original_of(event: &Event) -> NewEvent {
    NewEvent {
        eid: event.eid.clone(),
        host: event.host.clone(),
        actor: event.actor.clone(),
        kind: event.kind.clone(),
        layer: event.layer.clone(),
        obj: event.obj.clone(),
        data: event.data.clone(),
        guards: event.guards.clone(),
    }
}
fn deserialize_actual<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Box<Event>, D::Error> {
    crate::recovery_contract::deserialize_event(d).map(Box::new)
}

fn deserialize_original<'de, D: serde::Deserializer<'de>>(d: D) -> Result<NewEvent, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Original {
        eid: String,
        host: String,
        actor: String,
        #[serde(rename = "type")]
        kind: String,
        layer: String,
        obj: String,
        data: serde_json::Value,
        #[serde(deserialize_with = "crate::recovery_contract::deserialize_fences")]
        guards: Vec<crate::hub_protocol::Fence>,
    }
    let e = Original::deserialize(d)?;
    Ok(NewEvent {
        eid: e.eid,
        host: e.host,
        actor: e.actor,
        kind: e.kind,
        layer: e.layer,
        obj: e.obj,
        data: e.data,
        guards: e.guards,
    })
}

fn validate_original(original: &NewEvent) -> Result<(), String> {
    crate::hub::validate_input(original).map_err(|e| e.to_string())?;
    if original.kind.starts_with("lease.") {
        return Err("lease system event in business outbox".into());
    }
    Ok(())
}
pub(crate) fn validate_actual(original: &NewEvent, actual: &Event) -> Result<(), String> {
    validate_original(original)?;
    if original_of(actual) != *original {
        return Err("hub receipt differs from complete original event".into());
    }
    if actual.seq == 0 || actual.epoch != original.guards.first().map(|f| f.epoch) {
        return Err("invalid receipt sequence/epoch".into());
    }
    let time = crate::hub::event_time(actual).map_err(|e| e.to_string())?;
    crate::hub::timestamp(time).map_err(|e| e.to_string())?;
    // Current HubProtocol does not report costs. Unknown is null, never zero.
    if actual.cost_usd.is_some()
        || actual.lease_delta.is_some()
        || actual.heartbeat
        || actual.legacy
        || actual.legacy_change.is_some()
        || !actual.extra.is_empty()
    {
        return Err("unexpected cost or system state in ordinary receipt".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn readonly_missing_inputs_do_not_create_storage() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let path = storage.path().join("absent");
        let o = RecoveryOutbox::open(&path, RecoveryContext::from_catalog(&c).unwrap()).unwrap();
        std::fs::remove_dir_all(&path).unwrap();
        let inputs = o.read_inputs().unwrap();
        assert!(inputs.pending().is_empty());
        assert!(inputs.confirmed().is_empty());
        assert!(!path.exists());
    }
    #[test]
    fn actual_readonly_factory_binds_missing_context_and_reprobes_without_writer_lock() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let path = storage.path().join("missing/nested");
        let empty = RecoveryInputs::read(&path, &c).unwrap();
        assert!(!path.exists());
        empty.probe(&c).unwrap();
        let mut writer =
            RecoveryOutbox::open(&path, RecoveryContext::from_catalog(&c).unwrap()).unwrap();
        assert_eq!(
            empty.digest(),
            RecoveryInputs::read(&path, &c).unwrap().digest()
        );
        writer.append_pending(&original()).unwrap();
        assert!(empty.probe(&c).is_err());
        let bound = RecoveryInputs::read(&path, &c).unwrap();
        bound.probe(&c).unwrap();
        let (_other, other) = catalog();
        assert!(bound.probe(&other).is_err());
    }
    use super::*;
    use crate::{
        hub::Hub,
        init::{init, InitOptions},
    };
    use serde_json::json;
    fn git(root: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=codex/gpt-6",
                "-c",
                "user.email=codex@towermd.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .into()
    }
    fn catalog() -> (tempfile::TempDir, Catalog) {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q", "-b", "main"]);
        init(
            root.path(),
            &InitOptions {
                docs_dir: Some("docs/tower".into()),
            },
        )
        .unwrap();
        git(root.path(), &["add", "--all"]);
        git(root.path(), &["commit", "-qm", "contract fixture"]);
        let c = Catalog::load(root.path()).unwrap();
        (root, c)
    }
    fn original() -> NewEvent {
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "h".into(),
            actor: "process:test".into(),
            kind: "task.added".into(),
            layer: "L5".into(),
            obj: "T".into(),
            data: json!({"future":{"null":null,"array":[2,1]}}),
            guards: vec![],
        }
    }
    #[test]
    fn task_scope_allows_exact_physical_appends_but_rejects_duplicate_and_reordered_records() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut outbox =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let before = outbox.read_inputs().unwrap();
        let original = original();
        let AppendReceipt::Event(actual) =
            append_durable(&mut hub, &mut outbox, original.clone(), 100).unwrap()
        else {
            panic!("no event")
        };
        let after = outbox.read_inputs().unwrap();
        before
            .validate_task_writes(&after, &[(&original, Some(&actual))])
            .unwrap();
        after
            .validate_task_writes(&after, &[(&original, Some(&actual))])
            .unwrap();
        let path = storage.path().join(host_file(&original.host));
        let bytes = fs::read(&path).unwrap();
        let lines = bytes
            .split_inclusive(|byte| *byte == b'\n')
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        let mut duplicated = bytes.clone();
        duplicated.extend_from_slice(lines[0]);
        fs::write(&path, duplicated).unwrap();
        let duplicated = outbox.read_inputs().unwrap();
        assert_eq!(duplicated.confirmed(), after.confirmed());
        assert!(before
            .validate_task_writes(&duplicated, &[(&original, Some(&actual))])
            .is_err());
        let reordered = [lines[1], lines[0]].concat();
        fs::write(&path, reordered).unwrap();
        let reordered = outbox.read_inputs().unwrap();
        assert_eq!(reordered.confirmed(), after.confirmed());
        assert!(before
            .validate_task_writes(&reordered, &[(&original, Some(&actual))])
            .is_err());
    }
    #[test]
    fn durable_helper_saves_real_full_receipt_before_success_and_reopens() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let original = original();
        let AppendReceipt::Event(actual) =
            append_durable(&mut hub, &mut o, original.clone(), 100).unwrap()
        else {
            panic!("no event")
        };
        assert_eq!(o.read_inputs().unwrap().confirmed(), &[*actual.clone()]);
        assert!(o.read_inputs().unwrap().pending().is_empty());
        drop(o);
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let AppendReceipt::Event(again) = append_durable(&mut hub, &mut o, original, 200).unwrap()
        else {
            panic!("no event")
        };
        assert_eq!(again, actual);
        assert_eq!(hub.events(0).len(), 1);
    }
    fn accepted(hub: &mut Hub, e: &NewEvent) -> Event {
        let AppendReceipt::Event(actual) = hub.append(e.clone(), 100).unwrap() else {
            panic!()
        };
        *actual
    }
    #[test]
    fn confirmed_rejects_every_original_field_mismatch_before_writing() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let e = original();
        o.append_pending(&e).unwrap();
        let actual = accepted(&mut hub, &e);
        let path = storage.path().join(host_file(&e.host));
        let before = fs::read(&path).unwrap();
        let value = serde_json::to_value(&e).unwrap();
        for (field, bad) in [
            ("eid", json!(uuid::Uuid::now_v7().to_string())),
            ("host", json!("other")),
            ("actor", json!("process:other")),
            ("type", json!("task.other")),
            ("layer", json!("L4")),
            ("obj", json!("other")),
            ("data", json!({"other":true})),
            (
                "guards",
                json!([{"name":"main","holder":"writer","epoch":9}]),
            ),
        ] {
            let mut changed = value.clone();
            changed[field] = bad;
            let changed: NewEvent = serde_json::from_value(changed).unwrap();
            assert!(o.persist_confirmed(&changed, &actual).is_err(), "{field}");
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }
    #[test]
    fn confirmed_rejects_invalid_receipt_metadata_and_system_fields() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let e = original();
        o.append_pending(&e).unwrap();
        let actual = accepted(&mut hub, &e);
        for (field, bad) in [
            ("seq", json!(0)),
            ("t", json!("not-time")),
            ("epoch", json!(1)),
            ("cost_usd", json!(0)),
            ("heartbeat", json!(true)),
            ("legacy", json!(true)),
            ("lease_delta", json!({"Release":{"fences":[]}})),
            ("unknown_system_state", json!(true)),
        ] {
            let mut changed = serde_json::to_value(&actual).unwrap();
            changed[field] = bad;
            let changed: Event = serde_json::from_value(changed).unwrap();
            assert!(o.persist_confirmed(&e, &changed).is_err(), "{field}");
        }
    }
    #[test]
    fn conflicting_original_receipts_and_reused_sequences_are_rejected() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let e = original();
        let actual = accepted(&mut hub, &e);
        o.persist_confirmed(&e, &actual).unwrap();
        for field in ["seq", "t", "stale"] {
            let mut changed = actual.clone();
            match field {
                "seq" => changed.seq += 1,
                "t" => changed.t = "1970-01-01T00:00:00.101Z".into(),
                _ => changed.stale = !changed.stale,
            };
            assert!(o.persist_confirmed(&e, &changed).is_err(), "{field}");
        }
        let e2 = original();
        let mut a2 = accepted(&mut hub, &e2);
        a2.seq = actual.seq;
        assert!(o.persist_confirmed(&e2, &a2).is_err());
    }
    #[test]
    fn pending_sync_failure_never_calls_hub_and_preserves_original_bytes() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let e = original();
        o.fault = Some(Fault::PendingSync);
        assert!(append_durable(&mut hub, &mut o, e.clone(), 100).is_err());
        assert!(hub.events(0).is_empty());
        drop(o);
        let o = RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
            .unwrap();
        assert_eq!(o.read_inputs().unwrap().pending(), &[e]);
    }
    #[test]
    fn confirmed_sync_failure_keeps_request_uncertain_and_repairs_only_original() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let e = original();
        for _ in 0..2 {
            let mut o =
                RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                    .unwrap();
            o.fault = Some(Fault::ConfirmedSync);
            assert!(matches!(
                append_durable(&mut hub, &mut o, e.clone(), 100),
                Err(HubError::Uncertain(_))
            ));
            assert_eq!(hub.events(0).len(), 1);
            assert_eq!(hub.events(0)[0].eid, e.eid);
        }
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let AppendReceipt::Event(actual) =
            append_durable(&mut hub, &mut o, e.clone(), 200).unwrap()
        else {
            panic!()
        };
        assert_eq!(actual.eid, e.eid);
        assert_eq!(hub.events(0).len(), 1);
        assert_eq!(o.read_inputs().unwrap().confirmed(), &[*actual]);
    }
    #[test]
    fn torn_pending_or_confirmed_record_is_retained_and_never_upgraded() {
        for confirmed in [false, true] {
            let (_root, c) = catalog();
            let storage = tempfile::tempdir().unwrap();
            let hd = tempfile::tempdir().unwrap();
            let mut hub = Hub::open(hd.path()).unwrap();
            let e = original();
            let mut o =
                RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                    .unwrap();
            o.fault = Some(if confirmed {
                Fault::PartialConfirmed
            } else {
                Fault::PartialPending
            });
            assert!(append_durable(&mut hub, &mut o, e.clone(), 100).is_err());
            let path = storage.path().join(host_file(&e.host));
            let bytes = fs::read(&path).unwrap();
            drop(o);
            let mut o =
                RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                    .unwrap();
            let inputs = o.read_inputs().unwrap();
            assert!(inputs.confirmed().is_empty());
            assert_eq!(inputs.pending().len(), usize::from(confirmed));
            assert!(append_durable(&mut hub, &mut o, e, 200).is_err());
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
    #[test]
    fn sparse_receipts_include_all_guard_epochs_without_claiming_continuity() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let mut e = original();
        e.guards = vec![
            crate::hub_protocol::Fence {
                name: "module:A".into(),
                holder: "old".into(),
                epoch: 3,
            },
            crate::hub_protocol::Fence {
                name: "module:B".into(),
                holder: "old".into(),
                epoch: 17,
            },
        ];
        // Contract history fixture: sparse original acceptance role, not a producer execution.
        let mut actual = accepted(&mut hub, &e);
        actual.seq = 500;
        o.persist_confirmed(&e, &actual).unwrap();
        let inputs = o.read_inputs().unwrap();
        assert_eq!(inputs.high_water(), (500, 17));
        assert_eq!(inputs.confirmed().len(), 1);
        assert_eq!(
            inputs.recovery_boundaries(10, 2).unwrap(),
            (500 + (1 << 32), 17 + (1 << 32))
        );
    }
    #[test]
    fn sparse_recovery_boundary_overflow_fails_closed() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let mut inputs = RecoveryInputs::read(storage.path(), &c).unwrap();
        inputs.high_water = (u64::MAX, u64::MAX);
        assert!(inputs.recovery_boundaries(0, 0).is_err());
    }

    #[test]
    fn already_confirmed_original_is_not_appended_to_a_new_hub() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let e = original();
        let actual = append_durable(&mut hub, &mut o, e.clone(), 100).unwrap();
        let newdir = tempfile::tempdir().unwrap();
        let mut newhub = Hub::open(newdir.path()).unwrap();
        assert_eq!(append_durable(&mut newhub, &mut o, e, 200).unwrap(), actual);
        assert!(newhub.events(0).is_empty());
    }
    struct DuplicateHub {
        seq: u64,
        calls: usize,
    }
    impl HubProtocol for DuplicateHub {
        fn append(&mut self, _: NewEvent, _: u64) -> Result<AppendReceipt, HubError> {
            self.calls += 1;
            Ok(AppendReceipt::Duplicate { seq: self.seq })
        }
        fn subscribe(
            &mut self,
            _: u64,
            _: u64,
        ) -> Result<crate::hub_protocol::SubscriptionBatch, HubError> {
            panic!("not a history reader")
        }
        fn lease_acquire(
            &mut self,
            _: crate::hub_protocol::AcquireLease,
            _: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            panic!("not a lease operation")
        }
        fn lease_renew(
            &mut self,
            _: crate::hub_protocol::RenewLease,
            _: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            panic!("not a lease operation")
        }
        fn lease_release(
            &mut self,
            _: crate::hub_protocol::ReleaseLease,
            _: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, HubError> {
            panic!("not a lease operation")
        }
        fn snapshot_ack(
            &mut self,
            _: crate::hub_protocol::SnapshotAck,
            _: serde_json::Value,
        ) -> Result<(), HubError> {
            panic!()
        }
        fn rebuild(
            &mut self,
            _: crate::hub_protocol::RebuildSeed,
        ) -> Result<crate::hub_protocol::RebuildReceipt, HubError> {
            panic!()
        }
        fn contiguous_seq(&self) -> u64 {
            self.seq
        }
    }
    #[test]
    fn duplicate_seq_alone_is_uncertain_until_original_receipt_is_repaired() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut actualhub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let e = original();
        let actual = accepted(&mut actualhub, &e);
        let mut hub = DuplicateHub {
            seq: actual.seq,
            calls: 0,
        };
        assert!(matches!(
            append_durable(&mut hub, &mut o, e.clone(), 200),
            Err(HubError::Uncertain(_))
        ));
        assert!(o.read_inputs().unwrap().confirmed().is_empty());
        // Model the future E1 reader handing the core coordinator the exact original archive receipt.
        o.persist_confirmed(&e, &actual).unwrap();
        let AppendReceipt::Event(result) = append_durable(&mut hub, &mut o, e, 300).unwrap() else {
            panic!()
        };
        assert_eq!(*result, actual);
    }
    #[test]
    fn digest_binds_pending_confirmed_and_each_receipt_field_and_preserves_arrays() {
        let (_root, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let e = original();
        o.append_pending(&e).unwrap();
        let pending = o.read_inputs().unwrap().digest().to_owned();
        let actual = accepted(&mut hub, &e);
        o.persist_confirmed(&e, &actual).unwrap();
        let confirmed = o.read_inputs().unwrap().digest().to_owned();
        assert_ne!(pending, confirmed);
        let path = storage.path().join(host_file(&e.host));
        for field in ["seq", "t", "stale", "data"] {
            let mut changed = actual.clone();
            let mut original = e.clone();
            match field {
                "seq" => changed.seq += 10,
                "t" => changed.t = "1970-01-01T00:00:00.101Z".into(),
                "stale" => changed.stale = true,
                _ => {
                    original.data["future"]["array"] = json!([1, 2]);
                    changed.data = original.data.clone();
                }
            };
            let value = Envelope::Confirmed {
                schema: 1,
                project_id: c.config.project_id().into(),
                context: o.context.wire(),
                original,
                actual: Box::new(changed),
            };
            let mut bytes = canonical_json_bytes(&value).unwrap();
            bytes.push(b'\n');
            fs::write(&path, bytes).unwrap();
            assert_ne!(confirmed, o.read_inputs().unwrap().digest(), "{field}");
        }
    }
    fn creation_fault(path: &Path, point: CreationSync) {
        CREATION_FAULT.with(|fault| *fault.borrow_mut() = Some((path.to_path_buf(), point)));
    }
    fn creation_completed(path: &Path, point: CreationSync) -> bool {
        CREATION_COMPLETED.with(|events| {
            events
                .borrow()
                .iter()
                .any(|(target, stage)| target == path && *stage == point)
        })
    }
    fn file_creation_repair(first: CreationSync, reopen: bool) {
        let (_project, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let mut outbox =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        let original = original();
        let path = storage.path().join(host_file(&original.host));
        CREATION_COMPLETED.with(|events| events.borrow_mut().clear());
        creation_fault(&path, first);
        assert!(matches!(
            append_durable(&mut hub, &mut outbox, original.clone(), 100),
            Err(HubError::Io(_))
        ));
        assert!(path.is_file());
        assert!(hub.events(0).is_empty());
        if reopen {
            drop(outbox);
            outbox =
                RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                    .unwrap();
        }
        // An existing file must still repair its creation parent before the original can reach Hub.
        creation_fault(&path, CreationSync::Parent);
        assert!(append_durable(&mut hub, &mut outbox, original.clone(), 200).is_err());
        assert!(hub.events(0).is_empty());
        let AppendReceipt::Event(actual) =
            append_durable(&mut hub, &mut outbox, original.clone(), 300).unwrap()
        else {
            panic!("missing original Event")
        };
        assert_eq!(actual.eid, original.eid);
        assert_eq!(hub.events(0).len(), 1);
        assert!(creation_completed(&path, CreationSync::Parent));
        assert_eq!(outbox.read_inputs().unwrap().confirmed(), &[*actual]);
    }
    #[test]
    fn file_creation_own_sync_retry_repairs_parent_before_hub() {
        file_creation_repair(CreationSync::File, false);
    }
    #[test]
    fn file_creation_parent_sync_retry_repairs_parent_before_hub() {
        file_creation_repair(CreationSync::Parent, false);
    }
    #[test]
    fn file_creation_own_sync_reopen_repairs_parent_before_hub() {
        file_creation_repair(CreationSync::File, true);
    }
    #[test]
    fn file_creation_parent_sync_reopen_repairs_parent_before_hub() {
        file_creation_repair(CreationSync::Parent, true);
    }
    fn directory_creation_repair(first: CreationSync) {
        let (_project, c) = catalog();
        let storage = tempfile::tempdir().unwrap();
        let failed = storage.path().join("new-a/new-b");
        let directory = failed.join("outbox");
        CREATION_COMPLETED.with(|events| events.borrow_mut().clear());
        creation_fault(&failed, first);
        assert!(
            RecoveryOutbox::open(&directory, RecoveryContext::from_catalog(&c).unwrap()).is_err()
        );
        assert!(failed.is_dir());
        assert!(!directory.exists());
        // Retrying nested creation must repair the highest existing unfinished layer before creating descendants.
        creation_fault(&failed, CreationSync::Parent);
        assert!(
            RecoveryOutbox::open(&directory, RecoveryContext::from_catalog(&c).unwrap()).is_err()
        );
        assert!(!directory.exists());
        let mut outbox =
            RecoveryOutbox::open(&directory, RecoveryContext::from_catalog(&c).unwrap()).unwrap();
        assert!(creation_completed(&failed, CreationSync::Parent));
        let hd = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(hd.path()).unwrap();
        let original = original();
        let AppendReceipt::Event(actual) =
            append_durable(&mut hub, &mut outbox, original.clone(), 100).unwrap()
        else {
            panic!("missing Event")
        };
        assert_eq!(actual.eid, original.eid);
        assert_eq!(outbox.read_inputs().unwrap().confirmed(), &[*actual]);
    }
    #[test]
    fn nested_directory_creation_own_sync_retry_repairs_existing_ancestor() {
        directory_creation_repair(CreationSync::Directory);
    }
    #[test]
    fn nested_directory_creation_parent_sync_retry_repairs_existing_ancestor() {
        directory_creation_repair(CreationSync::Parent);
    }
}
