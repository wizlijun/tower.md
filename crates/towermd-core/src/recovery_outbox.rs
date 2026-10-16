//! Project-bound producer durability. No history/proof can be minted from these inputs.
use crate::recovery_contract::{canonical_json_bytes, sha256};
use crate::{
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError, HubProtocol, NewEvent},
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct RecoveryContext {
    project_id: String,
    docs_dir: String,
    hub: serde_json::Value,
}
impl RecoveryContext {
    pub fn from_catalog(catalog: &Catalog) -> Result<Self, String> {
        let actual = Catalog::load(&catalog.root)?;
        let context = Self::extract(&actual)?;
        let supplied = Self::extract(catalog)?;
        if context.project_id != supplied.project_id
            || context.wire() != supplied.wire()
            || actual.common_dir != catalog.common_dir
        {
            return Err("outbox context differs from authoritative Catalog".into());
        }
        Ok(context)
    }
    fn extract(catalog: &Catalog) -> Result<Self, String> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(&catalog.config.to_yaml()).map_err(|e| e.to_string())?;
        let hub = serde_json::to_value(value.get("hub").ok_or("missing hub configuration")?)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            project_id: catalog.config.project_id().into(),
            docs_dir: catalog.config.docs_dir().into(),
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
pub struct RecoveryInputs {
    pending: Vec<NewEvent>,
    confirmed: Vec<Event>,
    digest: String,
    high_water: (u64, u64),
}
impl RecoveryInputs {
    pub fn pending(&self) -> &[NewEvent] {
        &self.pending
    }
    pub fn confirmed(&self) -> &[Event] {
        &self.confirmed
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    // E1 combines sparse maxima with the actual snapshot boundary.
    #[allow(dead_code, reason = "E1 prepares recovery boundaries after E0")]
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
}

impl RecoveryOutbox {
    pub fn open(directory: &Path, context: RecoveryContext) -> Result<Self, String> {
        create_directory(directory)?;
        let lock = open_regular(&directory.join(".lock"))?;
        lock.try_lock_exclusive()
            .map_err(|e| format!("outbox is already open: {e}"))?;
        let result = Self {
            directory: directory.to_path_buf(),
            context,
            _lock: lock,
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
        let mut paths = fs::read_dir(&self.directory)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
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
                    || project != &self.context.project_id
                    || context != &self.context.wire()
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
            pending,
            confirmed,
            digest: sha256(&canonical_json_bytes(
                &serde_json::json!({"schema":1,"project_id":self.context.project_id,"context":self.context.wire(),"records":digest}),
            )?),
            high_water,
        })
    }
    fn sync_host(&mut self, host: &str, confirmed: bool) -> Result<(), String> {
        let file = open_regular(&self.directory.join(host_file(host)))?;
        #[cfg(test)]
        if self.consume_fault(confirmed).is_some() {
            return Err("injected outbox fsync failure".into());
        }
        #[cfg(not(test))]
        let _ = confirmed;
        file.sync_all().map_err(|e| e.to_string())
    }
    #[cfg(test)]
    fn consume_fault(&mut self, confirmed: bool) -> Option<Fault> {
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
            .and_then(|_| file.sync_all())
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
            return self.sync_host(&original.host, true);
        }
        if inputs
            .confirmed
            .iter()
            .any(|e| e.seq == actual.seq && e.eid != actual.eid)
        {
            return Err("reused original receipt sequence".into());
        }
        self.append_line(
            &Envelope::Confirmed {
                schema: 1,
                project_id: self.context.project_id.clone(),
                context: self.context.wire(),
                original: original.clone(),
                actual: Box::new(actual.clone()),
            },
            &original.host,
        )
    }
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
    if path.symlink_metadata().is_ok() {
        regular(path)?;
        return OpenOptions::new()
            .append(true)
            .read(true)
            .open(path)
            .map_err(|e| e.to_string());
    }
    let file = OpenOptions::new()
        .create_new(true)
        .append(true)
        .read(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    File::open(path.parent().ok_or("outbox file has no parent")?)
        .and_then(|p| p.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(file)
}
fn create_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                Err("outbox directory is not a real directory".into())
            } else {
                Ok(())
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or("outbox directory has no parent")?;
            create_directory(parent)?;
            fs::create_dir(path).map_err(|e| e.to_string())?;
            File::open(path)
                .and_then(|p| p.sync_all())
                .map_err(|e| e.to_string())?;
            File::open(parent)
                .and_then(|p| p.sync_all())
                .map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}
// Deliberately private to core; no daemon/client may declare a confirmed fact.
#[allow(dead_code, reason = "P2 producer wiring is outside the E0 scope")]
pub(crate) fn append_durable(
    hub: &mut dyn HubProtocol,
    outbox: &mut RecoveryOutbox,
    original: NewEvent,
    now_ms: u64,
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
    let receipt = hub
        .append(original.clone(), now_ms)
        .map_err(|e| HubError::Uncertain(e.to_string()))?;
    let actual = match receipt {
        AppendReceipt::Event(actual) => actual,
        AppendReceipt::Duplicate { seq } => {
            let inputs = outbox.read_inputs().map_err(HubError::Io)?;
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

fn original_of(event: &Event) -> NewEvent {
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
fn validate_actual(original: &NewEvent, actual: &Event) -> Result<(), String> {
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
        let inputs = RecoveryInputs {
            pending: vec![],
            confirmed: vec![],
            digest: String::new(),
            high_water: (u64::MAX, u64::MAX),
        };
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
}
