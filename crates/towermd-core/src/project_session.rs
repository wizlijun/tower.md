//! Rebuildable, long-lived business projection over the hub protocol.
use crate::catalog::Catalog;
use crate::hub_protocol::*;
use crate::snapshot::{Snapshot, StateProjection};
use crate::tasks::{TaskCommand, TaskFailure, TaskService};
use serde_json::Value;

pub struct ProjectSession {
    pub(crate) hub: Box<dyn HubProtocol>,
    identity: Option<(String, std::path::PathBuf)>,
    pub(crate) tasks: TaskService,
    pub(crate) projection: StateProjection,
    pub(crate) cursor: u64,
    pub(crate) head: HubHead,
    pub(crate) base: Option<Snapshot>,
    pub(crate) tail: Vec<Event>,
    pub(crate) authority_saved: Option<crate::snapshot_service::SavedSnapshot>,
    first_business_ms: Option<u64>,
    last_snapshot_ms: Option<u64>,
    applied_count: u64,
    pub(crate) authority_reads: u64,
    context: crate::recovery_outbox::RecoveryContext,
    root: std::path::PathBuf,
    common_dir: std::path::PathBuf,
    inputs: crate::recovery_outbox::RecoveryInputs,
    prepared_key: Option<String>,
    verified_sources: Option<(String, Option<String>, String)>,
    active: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSyncError {
    PreparationRequired,
    SourceChanged(String),
    Rejected(String),
}
impl std::fmt::Display for SessionSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreparationRequired => f.write_str("session proof preparation required"),
            Self::SourceChanged(s) | Self::Rejected(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for SessionSyncError {}
impl From<&str> for SessionSyncError {
    fn from(s: &str) -> Self {
        Self::Rejected(s.into())
    }
}
impl From<String> for SessionSyncError {
    fn from(s: String) -> Self {
        Self::Rejected(s)
    }
}
impl From<SessionSyncError> for String {
    fn from(e: SessionSyncError) -> Self {
        e.to_string()
    }
}
impl From<SessionSyncError> for TaskFailure {
    fn from(e: SessionSyncError) -> Self {
        e.to_string().into()
    }
}

// All fields are private: only an actual Session capture creates this input.
#[derive(PartialEq)]
pub(crate) struct SessionCapture {
    cursor: u64,
    previous_head: HubHead,
    head: HubHead,
    base: Option<Snapshot>,
    tail: Vec<Event>,
    fresh: Vec<Event>,
    restored: bool,
}
impl SessionCapture {
    pub(crate) fn base(&self) -> Option<&Snapshot> {
        self.base.as_ref()
    }
    pub(crate) fn history_tail(&self) -> &[Event] {
        &self.tail
    }
}
struct PreparedSession {
    capture: SessionCapture,
    projection: StateProjection,
    tasks: TaskService,
    saved: Option<crate::snapshot_service::SavedSnapshot>,
}
impl ProjectSession {
    pub fn open(
        dir: &std::path::Path,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
    ) -> Result<Self, SessionSyncError> {
        Self::new(Box::new(crate::hub::Hub::open(dir)?), catalog, inputs, now)
    }
    pub fn bind_catalog(&mut self, catalog: &Catalog) -> Result<(), String> {
        self.ensure_active()?;
        let identity = (
            catalog.config.project_id().to_string(),
            catalog.common_dir.clone(),
        );
        if self
            .identity
            .as_ref()
            .is_some_and(|existing| existing != &identity)
            || self.context != crate::recovery_outbox::RecoveryContext::extract(catalog)?
            || self.common_dir != catalog.common_dir
        {
            return Err("core session project identity differs from authoritative Catalog".into());
        }
        self.identity = Some(identity);
        self.root = catalog.root.clone();
        Ok(())
    }
    fn ensure_active(&self) -> Result<(), String> {
        if self.active {
            Ok(())
        } else {
            Err("session retired for recovery; business service inactive".into())
        }
    }
    pub fn projection(&self) -> Result<&StateProjection, String> {
        self.ensure_active()?;
        Ok(&self.projection)
    }
    pub fn pending_claim(&self) -> Result<Option<&crate::tasks::PendingClaim>, String> {
        self.ensure_active()?;
        Ok(self.tasks.pending_claim.as_ref())
    }
    pub fn protocol(&mut self) -> Result<&mut dyn HubProtocol, String> {
        self.ensure_active()?;
        Ok(&mut *self.hub)
    }
    pub fn new(
        mut hub: Box<dyn HubProtocol>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
    ) -> Result<Self, SessionSyncError> {
        let head = match hub.subscribe(0, now) {
            Ok(b) => b.head,
            Err(HubError::SnapshotRequired { head, .. }) => head,
            Err(e) => return Err(SessionSyncError::Rejected(e.to_string())),
        };
        let mut session = Self {
            hub,
            identity: Some((
                catalog.config.project_id().into(),
                catalog.common_dir.clone(),
            )),
            tasks: TaskService::default(),
            projection: StateProjection::default(),
            cursor: 0,
            head,
            base: None,
            tail: vec![],
            authority_saved: None,
            first_business_ms: None,
            last_snapshot_ms: None,
            applied_count: 0,
            authority_reads: 0,
            context: crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?,
            root: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            inputs: inputs.clone(),
            prepared_key: None,
            verified_sources: None,
            active: true,
        };
        session.prepare_owned(catalog, inputs, now)?;
        Ok(session)
    }
    fn capture(&mut self, now: u64) -> Result<SessionCapture, SessionSyncError> {
        self.ensure_active()?;
        let (head, base, mut tail, fresh, restored) = match self.hub.subscribe(self.cursor, now) {
            Ok(b) => (
                b.head,
                self.base.clone(),
                self.tail.clone(),
                b.events,
                false,
            ),
            Err(HubError::SnapshotRequired { head, checkpoint }) => {
                let base: Snapshot = serde_json::from_value(checkpoint)
                    .map_err(|e| SessionSyncError::Rejected(e.to_string()))?;
                base.validate_metadata()?;
                if base.through_seq != head.floor_seq {
                    return Err(SessionSyncError::Rejected(
                        "checkpoint floor mismatch".into(),
                    ));
                }
                let b = self
                    .hub
                    .subscribe(head.floor_seq, now)
                    .map_err(|e| SessionSyncError::Rejected(e.to_string()))?;
                (b.head, Some(base), vec![], b.events, true)
            }
            Err(e) => return Err(SessionSyncError::Rejected(e.to_string())),
        };
        let through = self.hub.contiguous_seq();
        let fresh: Vec<_> = fresh.into_iter().take_while(|e| e.seq <= through).collect();
        tail.extend(fresh.clone());
        Ok(SessionCapture {
            cursor: self.cursor,
            previous_head: self.head.clone(),
            head,
            base,
            tail,
            fresh,
            restored,
        })
    }
    fn prepare(
        capture: SessionCapture,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
    ) -> Result<PreparedSession, SessionSyncError> {
        let history = crate::recovery_git::prepare_session_history(catalog, inputs, &capture)?;
        let sources = history.snapshot_sources();
        // Opaque envelope metadata never grants business authority.
        if let Some(base) = &capture.base {
            let ack = capture
                .head
                .ack
                .as_ref()
                .ok_or("checkpoint lacks exact ack")?;
            let saved =
                crate::snapshot_service::read_ack_snapshot_in_sources(catalog, ack, sources)?;
            if &saved.snapshot != base || ack.seq != capture.head.floor_seq {
                return Err(SessionSyncError::Rejected(
                    "opaque checkpoint differs from exact reachable ack snapshot".into(),
                ));
            }
        }
        let private_source = sources.private.clone();
        let saved = sources.clone().select()?;
        if let Some(saved) = &saved {
            if let Some(ack) = &capture.head.ack {
                let promotion = saved.source == crate::snapshot_service::SnapshotSource::Main
                    && saved.authority_revision == catalog.revision
                    && private_source.as_ref().is_some_and(|old| {
                        old.commit == ack.commit && crate::snapshot_service::equivalent(old, saved)
                    });
                if ack.seq > saved.snapshot.through_seq
                    || (ack.seq == saved.snapshot.through_seq
                        && (ack.epoch != saved.snapshot.writer_epoch
                            || (ack.commit != saved.commit && !promotion)))
                {
                    return Err(SessionSyncError::Rejected(
                        "persistent snapshot rollback or same-watermark conflict".into(),
                    ));
                }
            }
            let expected = Snapshot::extend(
                capture.base.as_ref(),
                &capture.tail,
                saved.snapshot.through_seq,
                saved.snapshot.writer_epoch,
            )
            .or_else(|e| {
                if capture.base.is_none() {
                    Snapshot::from_events(
                        catalog.config.project_id(),
                        &capture.tail,
                        saved.snapshot.through_seq,
                        saved.snapshot.writer_epoch,
                    )
                } else {
                    Err(e)
                }
            })
            .map_err(|error| {
                SessionSyncError::Rejected(format!(
                    "hub history differs from persistent snapshot; 需要 App 恢复：{error}"
                ))
            })?;
            if expected != saved.snapshot {
                return Err(SessionSyncError::Rejected(
                    "hub history differs from persistent snapshot; App recovery required".into(),
                ));
            }
        } else if capture.head.ack.is_some() {
            return Err(SessionSyncError::Rejected(
                "persistent snapshot missing for hub ack".into(),
            ));
        }
        let mut projection = capture
            .base
            .as_ref()
            .map_or_else(StateProjection::default, |b| b.projection.clone());
        let mut tasks = TaskService::default();
        tasks.restore(&projection)?;
        let mut cursor = capture.base.as_ref().map_or(0, |b| b.through_seq);
        for event in &capture.tail {
            if cursor.checked_add(1) != Some(event.seq) {
                return Err(SessionSyncError::Rejected(
                    "non-contiguous protocol events".into(),
                ));
            }
            let change = tasks.prepare(event)?;
            projection.apply_recorded(event)?;
            tasks.apply_prepared(change);
            cursor = event.seq;
        }
        Ok(PreparedSession {
            capture,
            projection,
            tasks,
            saved,
        })
    }
    fn key(
        capture: &SessionCapture,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        private: &Option<String>,
    ) -> Result<String, SessionSyncError> {
        crate::recovery_contract::canonical_hash(&serde_json::json!({"authority":catalog.revision,"private":private,"inputs":inputs.digest(),"cursor":capture.cursor,"previous_head":capture.previous_head,"head":capture.head,"restored":capture.restored,"fresh":capture.fresh,"context":catalog.config.to_yaml(),"base":capture.base,"tail":capture.tail})).map_err(Into::into)
    }
    pub fn sync_shared(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
    ) -> Result<(), SessionSyncError> {
        for _ in 0..3 {
            if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
                return Err(SessionSyncError::SourceChanged(
                    "authority changed; reload Catalog".into(),
                ));
            }
            let private = crate::snapshot_git::private_revision(&catalog.root)?;
            let capture = {
                let mut s = session.lock().map_err(|_| "session lock poisoned")?;
                s.bind_catalog(catalog)?;
                s.capture(now)?
            };
            let key = Self::key(&capture, catalog, inputs, &private)?;
            let cached = capture.fresh.is_empty() && !capture.restored && {
                let s = session.lock().map_err(|_| "session lock poisoned")?;
                s.prepared_key.as_ref() == Some(&key)
            };
            if cached {
                inputs
                    .probe(catalog)
                    .map_err(SessionSyncError::SourceChanged)?;
                if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
                    return Err(SessionSyncError::SourceChanged(
                        "authority changed during cache probe".into(),
                    ));
                }
                if crate::snapshot_git::private_revision(&catalog.root)? != private {
                    continue;
                }
                let mut s = session.lock().map_err(|_| "session lock poisoned")?;
                s.bind_catalog(catalog)?;
                if s.capture(now)? == capture {
                    return Ok(());
                }
                continue;
            }
            let prepared = match Self::prepare(capture, catalog, inputs) {
                Ok(prepared) => prepared,
                Err(error) => {
                    inputs
                        .probe(catalog)
                        .map_err(SessionSyncError::SourceChanged)?;
                    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
                        return Err(SessionSyncError::SourceChanged(
                            "authority changed during preparation".into(),
                        ));
                    }
                    if crate::snapshot_git::private_revision(&catalog.root)? != private {
                        continue;
                    }
                    return Err(error);
                }
            };
            inputs
                .probe(catalog)
                .map_err(SessionSyncError::SourceChanged)?;
            if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
                return Err(SessionSyncError::SourceChanged(
                    "authority changed during preparation".into(),
                ));
            }
            if crate::snapshot_git::private_revision(&catalog.root)? != private {
                continue;
            }
            let mut s = session.lock().map_err(|_| "session lock poisoned")?;
            s.bind_catalog(catalog)?;
            let current = s.capture(now)?;
            if current != prepared.capture {
                continue;
            }
            s.inputs = inputs.clone();
            s.install(prepared, catalog, inputs, &private)?;
            return Ok(());
        }
        Err(SessionSyncError::SourceChanged(
            "session sources/head changed during three preparations".into(),
        ))
    }
    fn install(
        &mut self,
        prepared: PreparedSession,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        private: &Option<String>,
    ) -> Result<(), SessionSyncError> {
        self.ensure_active()?;
        let pending = self.tasks.pending_claim.take();
        self.tasks = prepared.tasks;
        self.tasks.pending_claim = pending;
        self.applied_count += prepared.capture.fresh.len() as u64;
        self.projection = prepared.projection;
        self.base = prepared.capture.base;
        self.cursor = self.base.as_ref().map_or(0, |b| b.through_seq);
        self.tail = vec![];
        for event in prepared.capture.tail {
            self.observe(&event)?;
            self.cursor = event.seq;
            self.tail.push(event);
        }
        self.head = prepared.capture.head;
        self.authority_saved = prepared.saved;
        // The verified prefix now belongs to these installed coordinates, not the old cursor.
        let installed = SessionCapture {
            cursor: self.cursor,
            previous_head: self.head.clone(),
            head: self.head.clone(),
            base: self.base.clone(),
            tail: self.tail.clone(),
            fresh: vec![],
            restored: false,
        };
        self.prepared_key = Some(Self::key(&installed, catalog, inputs, private)?);
        self.verified_sources = Some((
            catalog.revision.clone(),
            private.clone(),
            inputs.digest().into(),
        ));
        self.authority_reads += 1;
        Ok(())
    }
    fn observe(&mut self, event: &Event) -> Result<(), String> {
        let time = event_millis(event)?;
        if !event.stale {
            if event.kind == "snapshot.written" {
                self.last_snapshot_ms = Some(time);
            } else if !event.is_pure_heartbeat()
                && !(event.obj == "main" && event.kind.starts_with("lease."))
                && self.first_business_ms.is_none()
            {
                self.first_business_ms = Some(time);
            }
        }
        Ok(())
    }
    pub(crate) fn sync(&mut self, now: u64) -> Result<(), SessionSyncError> {
        self.ensure_active()?;
        let batch = match self.hub.subscribe(self.cursor, now) {
            Ok(b) => b,
            Err(HubError::SnapshotRequired { .. }) => {
                return Err(SessionSyncError::PreparationRequired)
            }
            Err(e) => return Err(SessionSyncError::Rejected(e.to_string())),
        };
        for event in batch
            .events
            .into_iter()
            .take_while(|e| e.seq <= batch.head.contiguous_seq)
        {
            if matches!(event.kind.as_str(), "merge.done" | "baseline.created") {
                return Err(SessionSyncError::PreparationRequired);
            }
            if !event.stale
                && event.kind.starts_with("task.")
                && event.data["task"]["state"] == "merged"
            {
                return Err(SessionSyncError::Rejected(
                    "raw task event cannot certify merged".into(),
                ));
            }
            if self.cursor.checked_add(1) != Some(event.seq) {
                return Err(SessionSyncError::Rejected(
                    "non-contiguous protocol events".into(),
                ));
            }
            let change = self.tasks.prepare(&event)?;
            self.projection.apply_recorded(&event)?;
            self.tasks.apply_prepared(change);
            self.prepared_key = None;
            self.observe(&event)?;
            self.cursor = event.seq;
            self.applied_count += 1;
            self.tail.push(event);
        }
        self.head = batch.head;
        Ok(())
    }
    pub(crate) fn refresh_system_head(&mut self, now: u64) -> Result<(), String> {
        self.ensure_active()?;
        self.head = self
            .hub
            .subscribe(self.hub.contiguous_seq(), now)
            .map_err(|e| e.to_string())?
            .head;
        Ok(())
    }
    pub(crate) fn bound_inputs(&self) -> crate::recovery_outbox::RecoveryInputs {
        self.inputs.clone()
    }
    pub fn applied_seq(&self) -> u64 {
        self.cursor
    }
    pub(crate) fn new_staging(
        mut protocol: Box<dyn HubProtocol>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        seed: &crate::recovery::VerifiedRecoverySeed,
        now: u64,
    ) -> Result<Self, String> {
        protocol
            .rebuild(seed.protocol_seed().clone())
            .map_err(|e| e.to_string())?;
        let head = protocol
            .subscribe(seed.checkpoint().through_seq, now)
            .map_err(|e| e.to_string())?
            .head;
        let projection = seed.checkpoint().projection.clone();
        let mut tasks = TaskService::default();
        tasks.restore(&projection)?;
        Ok(Self {
            hub: protocol,
            identity: Some((
                catalog.config.project_id().into(),
                catalog.common_dir.clone(),
            )),
            tasks,
            projection,
            cursor: seed.checkpoint().through_seq,
            head,
            base: Some(seed.checkpoint().clone()),
            tail: vec![],
            authority_saved: None,
            first_business_ms: None,
            last_snapshot_ms: None,
            applied_count: 0,
            authority_reads: 0,
            context: crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?,
            root: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            inputs: inputs.clone(),
            prepared_key: None,
            verified_sources: None,
            active: true,
        })
    }
    pub(crate) fn sync_staging(
        &mut self,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        seed: &crate::recovery::VerifiedRecoverySeed,
        now: u64,
    ) -> Result<(), String> {
        seed.probe(catalog, inputs)?;
        let capture = self.capture(now)?;
        crate::recovery_git::prepare_session_history(catalog, inputs, &capture)?;
        for event in &capture.fresh {
            let change = self.tasks.prepare(event)?;
            self.projection.apply_recorded(event)?;
            self.tasks.apply_prepared(change);
            self.observe(event)?;
            self.cursor = event.seq;
            self.tail.push(event.clone());
            self.applied_count += 1;
        }
        self.head = capture.head;
        Ok(())
    }
    pub(crate) fn validate_empty(
        &mut self,
        catalog: &Catalog,
        now: u64,
    ) -> Result<HubHead, String> {
        if self.context != crate::recovery_outbox::RecoveryContext::extract(catalog)?
            || self.common_dir != catalog.common_dir
        {
            return Err("recovery target context differs".into());
        }
        if self.hub.contiguous_seq() != 0 {
            return Err("nonempty target cannot be retired".into());
        }
        let head = self.hub.subscribe(0, now).map_err(|e| e.to_string())?.head;
        if head.contiguous_seq != 0
            || head.floor_seq != 0
            || head.ack.is_some()
            || !head.leases.is_empty()
        {
            return Err("nonempty target cannot be retired".into());
        }
        Ok(head)
    }
    pub(crate) fn retire_empty(&mut self, catalog: &Catalog, now: u64) -> Result<HubHead, String> {
        let head = self.validate_empty(catalog, now)?;
        self.active = false;
        self.projection = StateProjection::default();
        self.tasks = TaskService::default();
        Ok(head)
    }
    pub(crate) fn activate_rebuilt(&mut self, mut staged: Self) {
        // Preserve the target's original protocol and its exclusive file lock.
        std::mem::swap(&mut self.hub, &mut staged.hub);
        *self = staged;
    }
    fn prepare_owned(
        &mut self,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
    ) -> Result<(), SessionSyncError> {
        for _ in 0..3 {
            if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
                return Err(SessionSyncError::SourceChanged(
                    "authority changed; reload Catalog".into(),
                ));
            }
            let private = crate::snapshot_git::private_revision(&catalog.root)?;
            let capture = self.capture(now)?;
            let key = Self::key(&capture, catalog, inputs, &private)?;
            if capture.fresh.is_empty()
                && !capture.restored
                && self.prepared_key.as_ref() == Some(&key)
            {
                inputs
                    .probe(catalog)
                    .map_err(SessionSyncError::SourceChanged)?;
                if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
                    return Err(SessionSyncError::SourceChanged(
                        "authority changed during cache probe".into(),
                    ));
                }
                if crate::snapshot_git::private_revision(&catalog.root)? != private {
                    continue;
                }
                if self.capture(now)? == capture {
                    return Ok(());
                }
                continue;
            }
            let prepared = Self::prepare(capture, catalog, inputs);
            inputs
                .probe(catalog)
                .map_err(SessionSyncError::SourceChanged)?;
            if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
                return Err(SessionSyncError::SourceChanged(
                    "authority changed during preparation".into(),
                ));
            }
            if crate::snapshot_git::private_revision(&catalog.root)? != private {
                continue;
            }
            let prepared = prepared?;
            let current = self.capture(now)?;
            if current != prepared.capture {
                continue;
            }
            self.install(prepared, catalog, inputs, &private)?;
            return Ok(());
        }
        Err(SessionSyncError::SourceChanged(
            "session sources/head changed during three preparations".into(),
        ))
    }
    pub(crate) fn verify_owned(
        &mut self,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
    ) -> Result<(), String> {
        self.prepare_owned(catalog, inputs, now).map_err(Into::into)
    }
    pub fn read_status_shared(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        host: &str,
        run: Option<&str>,
        now: u64,
    ) -> Result<Value, TaskFailure> {
        // Status probes source identity outside the live lock; it never starts slow proof.
        inputs
            .probe(catalog)
            .map_err(|error| TaskFailure::from(format!("SourceChanged: {error}")))?;
        if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
            return Err(
                "SourceChanged: authority changed; reload Catalog on the next request".into(),
            );
        }
        let private = crate::snapshot_git::private_revision(&catalog.root)?;
        let binding = (
            catalog.revision.clone(),
            private,
            inputs.digest().to_string(),
        );
        let mut live = session.lock().map_err(|_| "session lock poisoned")?;
        let changed = live.verified_sources.as_ref() != Some(&binding);
        let mut status = live.read_status(catalog, host, run, now)?;
        if changed {
            status["pending_verification"] = serde_json::json!(true);
            status["verification_error"] = serde_json::json!(
                "authority or recovery inputs changed; showing the last verified prefix"
            );
        }
        Ok(status)
    }
    pub fn read_status(
        &mut self,
        catalog: &Catalog,
        host: &str,
        run: Option<&str>,
        now: u64,
    ) -> Result<Value, TaskFailure> {
        self.bind_catalog(catalog)?;
        self.refresh_system_head(now)?;
        let pending_error = self.sync(now).err().map(|e| e.to_string());
        let mut value =
            self.tasks
                .execute(&mut *self.hub, catalog, host, TaskCommand::Status, run, now)?;
        self.refresh_system_head(now)?;
        value["seq"] = serde_json::json!(self.cursor);
        if self.head.contiguous_seq > self.cursor {
            value["pending_verification"] = serde_json::json!(true);
        }
        if let Some(error) = pending_error {
            value["verification_error"] = serde_json::json!(error);
        }
        Ok(value)
    }
    pub fn execute_shared(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        host: &str,
        command: TaskCommand,
        run: Option<&str>,
        now: u64,
    ) -> Result<Value, TaskFailure> {
        Self::sync_shared(session, catalog, inputs, now)?;
        let (result, synced) = {
            let mut s = session.lock().map_err(|_| "session lock poisoned")?;
            s.bind_catalog(catalog)?;
            s.tick(host, now)?;
            let status = matches!(command, TaskCommand::Status);
            let mut result = {
                let s = &mut *s;
                s.tasks
                    .execute(&mut *s.hub, catalog, host, command, run, now)
            };
            let synced = s.sync(now);
            if status {
                if let Ok(value) = &mut result {
                    value["seq"] = serde_json::json!(s.cursor);
                }
            }
            (result, synced)
        };
        match synced {
            Ok(()) => result,
            Err(SessionSyncError::PreparationRequired) => {
                Self::sync_shared(session, catalog, inputs, now).map_err(|e| {
                    TaskFailure::from(format!(
                        "accepted request requires synchronization; original result retained: {e}"
                    ))
                })?;
                result
            }
            Err(e) => result.and_then(|_| Err(e.into())),
        }
    }
    pub fn execute(
        &mut self,
        catalog: &Catalog,
        host: &str,
        command: TaskCommand,
        run: Option<&str>,
        now: u64,
    ) -> Result<Value, TaskFailure> {
        self.bind_catalog(catalog)?;
        self.tick(host, now)?;
        let status = matches!(command, TaskCommand::Status);
        let mut result = self
            .tasks
            .execute(&mut *self.hub, catalog, host, command, run, now);
        if status {
            if let Ok(value) = &mut result {
                value["seq"] = serde_json::json!(self.cursor);
            }
        }
        match self.sync(now) {
            Ok(()) => result,
            Err(e) => match result {
                Err(original) => Err(original),
                Ok(_) => Err(e.into()),
            },
        }
    }
    pub fn renew_binding(
        &mut self,
        catalog: &Catalog,
        host: &str,
        run: Option<&str>,
        now: u64,
    ) -> Result<(), String> {
        self.bind_catalog(catalog)?;
        self.sync(now)?;
        self.tasks
            .renew_binding(&mut *self.hub, catalog, host, run, now)?;
        self.sync(now).map_err(Into::into)
    }
    pub fn tick(&mut self, host: &str, now: u64) -> Result<(), String> {
        self.sync(now)?;
        self.tasks.expire(&mut *self.hub, host, now)?;
        self.sync(now).map_err(Into::into)
    }
    pub fn due(&self, interval: u64, now: u64) -> bool {
        self.first_business_ms.is_some_and(|first| {
            now.saturating_sub(self.last_snapshot_ms.unwrap_or(first)) >= interval
        })
    }
    pub fn checkpoint(&self, id: &str, writer: &Fence, now: u64) -> Result<Snapshot, String> {
        self.ensure_active()?;
        if self
            .identity
            .as_ref()
            .is_some_and(|identity| identity.0 != id)
        {
            return Err("checkpoint project differs from bound session".into());
        }
        if writer.name != "main"
            || !self
                .head
                .leases
                .get("main")
                .is_some_and(|l| l.fence() == *writer && l.expires_at > now)
        {
            return Err("checkpoint requires the current main lease fence".into());
        }
        if self.cursor != self.hub.contiguous_seq() {
            return Err("checkpoint requires fully applied prefix".into());
        }
        let through = self.cursor;
        match &self.base {
            Some(base) if base.project_id != id => {
                Err("checkpoint project does not match base snapshot".into())
            }
            Some(base) => Snapshot::extend(Some(base), &self.tail, through, writer.epoch),
            None => Snapshot::from_events(id, &self.tail, through, writer.epoch),
        }
    }
    pub(crate) fn install_checkpoint(&mut self, snapshot: Snapshot) {
        let through = snapshot.through_seq;
        self.tail.retain(|e| e.seq > through);
        self.base = Some(snapshot);
        self.first_business_ms = self
            .tail
            .iter()
            .find(|e| {
                !e.stale
                    && !e.is_pure_heartbeat()
                    && e.kind != "snapshot.written"
                    && !(e.obj == "main" && e.kind.starts_with("lease."))
            })
            .and_then(|e| event_millis(e).ok());
    }
    pub fn reconcile_pending(&mut self, now: u64) -> Result<(), String> {
        self.tasks.reconcile_pending(&mut *self.hub, now)?;
        self.sync(now).map_err(Into::into)
    }
    pub fn applied_count(&self) -> u64 {
        self.applied_count
    }
    pub fn authority_reads(&self) -> u64 {
        self.authority_reads
    }
    pub fn last_seq(&self) -> u64 {
        self.applied_seq()
    }
    pub fn epoch_high_water(&self) -> u64 {
        self.head.epoch_high_water
    }
    pub fn base_snapshot(&self) -> Option<&Snapshot> {
        self.base.as_ref()
    }
    pub fn snapshot_ack(&self) -> Option<&SnapshotAck> {
        self.head.ack.as_ref()
    }
    pub fn leases(&self, now: u64) -> std::collections::BTreeMap<String, Lease> {
        self.head
            .leases
            .iter()
            .filter(|(_, l)| l.expires_at > now)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    pub fn events(&self, after: u64) -> Vec<Event> {
        self.tail[self.tail.partition_point(|e| e.seq <= after)..].to_vec()
    }
    pub fn subscribe(&mut self, after: u64, now: u64) -> Result<SubscriptionBatch, HubError> {
        self.sync(now)
            .map_err(|e| HubError::Invalid(e.to_string()))?;
        let batch = self.hub.subscribe(after, now)?;
        if batch.events.iter().any(|e| e.seq > self.cursor) {
            return Err(HubError::Invalid(
                "subscription exceeds applied prefix".into(),
            ));
        }
        Ok(batch)
    }
}
fn event_millis(event: &Event) -> Result<u64, String> {
    let time = chrono::DateTime::parse_from_rfc3339(&event.t)
        .map_err(|e| e.to_string())?
        .timestamp_millis();
    u64::try_from(time).map_err(|_| "negative event time".into())
}
