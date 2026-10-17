//! Rebuildable, long-lived business projection over the hub protocol.
use crate::catalog::Catalog;
use crate::hub_protocol::*;
use crate::snapshot::{Snapshot, StateProjection};
use crate::tasks::{TaskCommand, TaskFailure, TaskService};
use serde_json::Value;

/// Derived observations of an actually proved and installed prefix. These values
/// never grant permission to write or replace the original recovery proof.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SnapshotLast {
    pub commit: String,
    pub seq: u64,
    pub epoch: u64,
    pub written_at: Option<u64>,
    pub written_seq: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SnapshotPending {
    pub seq: u64,
    pub max_seq: u64,
    pub unix_ms: u64,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SnapshotAlert {
    pub eid: String,
    pub seq: u64,
    pub since: u64,
    pub reason: String,
    pub interval: u64,
    pub basis: Value,
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct SnapshotSummary {
    pub generation: u64,
    pub cursor: u64,
    pub last: Option<SnapshotLast>,
    pub source_reference: Option<String>,
    pub fast_pending: Option<SnapshotPending>,
    pub periodic_pending_since: Option<u64>,
    pub pending_through: Option<u64>,
    pub alert: Option<SnapshotAlert>,
    pub closed_alerts: Vec<String>,
}

pub(crate) fn snapshot_alert(event: &Event) -> Option<SnapshotAlert> {
    if event.stale
        || event.kind != "alert.raised"
        || event.actor != "process:towermd-snapshot"
        || event.obj != "main"
        || event.data["component"] != "snapshot"
    {
        return None;
    }
    Some(SnapshotAlert {
        eid: event.eid.clone(),
        seq: event.seq,
        since: event.data["since"].as_u64()?,
        reason: event.data["reason"].as_str()?.into(),
        interval: event.data["interval"].as_u64()?,
        basis: event.data["basis"].clone(),
    })
}
pub(crate) fn is_snapshot_self_event(event: &Event) -> bool {
    matches!(event.kind.as_str(), "snapshot.written" | "snapshot.ack")
        || snapshot_alert(event).is_some()
}
pub(crate) fn matching_written(event: &Event, last: &SnapshotLast) -> bool {
    !event.stale
        && event.kind == "snapshot.written"
        && event.actor == "process:towermd-snapshot"
        && event.obj == "main"
        && crate::events::derived_eid(&last.commit, "snapshot.written")
            .ok()
            .as_ref()
            == Some(&event.eid)
        && event.data["commit"].as_str() == Some(&last.commit)
        && event.data["seq"].as_u64() == Some(last.seq)
        && event.data["epoch"].as_u64() == Some(last.epoch)
}
impl SnapshotSummary {
    fn observe(&mut self, event: &Event, through: u64) -> Result<(), String> {
        let time = event_millis(event)?;
        if let Some(last) = &mut self.last {
            if matching_written(event, last) {
                last.written_at = Some(time);
                last.written_seq = Some(event.seq);
            }
        }
        if let Some(alert) = snapshot_alert(event)
            .filter(|alert| event.seq > through && !self.closed_alerts.contains(&alert.eid))
        {
            if self.alert.is_none() {
                self.alert = Some(alert);
            }
        }
        if event.seq <= through || is_snapshot_self_event(event) {
            return Ok(());
        }
        self.periodic_pending_since.get_or_insert(time);
        self.pending_through = Some(event.seq);
        if !event.stale
            && matches!(
                event.kind.as_str(),
                "merge.done" | "hold.resolved" | "proposal.resolved" | "baseline.created"
            )
        {
            let pending = self.fast_pending.get_or_insert(SnapshotPending {
                seq: event.seq,
                max_seq: event.seq,
                unix_ms: time,
            });
            pending.max_seq = pending.max_seq.max(event.seq);
        }
        Ok(())
    }
}

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
    summary: SnapshotSummary,
    periodic_pending_since: Option<u64>,
    first_business_ms: Option<u64>,
    last_snapshot_ms: Option<u64>,
    applied_count: u64,
    pub(crate) authority_reads: u64,
    context: crate::recovery_outbox::RecoveryContext,
    root: std::path::PathBuf,
    common_dir: std::path::PathBuf,
    inputs: crate::recovery_outbox::RecoveryInputs,
    prepared_key: Option<String>,
    // Head of the actual verified business prefix bound by prepared_key. The
    // current observed system head may advance independently during maintenance.
    prepared_head: Option<HubHead>,
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

enum Preparation {
    Incremental,
    Complete,
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
struct CommandResult {
    result: Result<Value, TaskFailure>,
    pending: Option<crate::tasks::PendingClaim>,
    previous_pending: Option<String>,
    show_before: Option<RecordedPrefix>,
    renewal: Option<Event>,
}
struct RecordedPrefix {
    session: SessionCapture,
    projection: StateProjection,
    pending: Option<crate::tasks::PendingClaim>,
    proof_key: Option<String>,
    sources: Option<(String, Option<String>, String)>,
    observed_head: HubHead,
}
impl RecordedPrefix {
    fn new(live: &ProjectSession) -> Self {
        let head = match (&live.prepared_key, &live.prepared_head) {
            (Some(_), Some(head)) => head,
            _ => &live.head,
        };
        Self {
            session: SessionCapture {
                cursor: live.cursor,
                previous_head: head.clone(),
                head: head.clone(),
                base: live.base.clone(),
                tail: live.tail.clone(),
                fresh: vec![],
                restored: false,
            },
            projection: live.projection.clone(),
            pending: live.tasks.pending_claim.clone(),
            proof_key: live.prepared_key.clone(),
            sources: live.verified_sources.clone(),
            observed_head: live.head.clone(),
        }
    }
    fn business_matches(&self, live: &ProjectSession) -> bool {
        live.projection == self.projection
            && live.tasks.pending_claim == self.pending
            && live.prepared_key == self.proof_key
            && live.verified_sources == self.sources
    }
}
#[cfg(test)]
thread_local! {
    static PREPARE_VISITS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}
// This capability is constructed only from the Session's recorded verified prefix.
// It is not a new capture of the current Hub and cannot certify a changed source.
struct PreparedTaskDelta {
    before: RecordedPrefix,
    projection: StateProjection,
    tasks: TaskService,
}
struct PreparedSession {
    capture: SessionCapture,
    projection: StateProjection,
    tasks: TaskService,
    saved: Option<crate::snapshot_service::SavedSnapshot>,
    facts: SnapshotSummary,
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
    pub fn snapshot_summary(&self) -> Result<SnapshotSummary, String> {
        self.ensure_active()?;
        Ok(self.summary.clone())
    }
    /// Unix milliseconds, matching the confirmed Event.t anchors held by Session.
    pub fn periodic_due(&self, interval: u64, now: u64) -> bool {
        self.periodic_pending_since.is_some_and(|since| {
            now.saturating_sub(
                self.summary
                    .last
                    .as_ref()
                    .and_then(|last| last.written_at)
                    .unwrap_or(since),
            ) >= interval
        })
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
            summary: SnapshotSummary::default(),
            periodic_pending_since: None,
            first_business_ms: None,
            last_snapshot_ms: None,
            applied_count: 0,
            authority_reads: 0,
            context: crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?,
            root: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            inputs: inputs.clone(),
            prepared_key: None,
            prepared_head: None,
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
    fn apply_prepared_event(
        tasks: &mut TaskService,
        projection: &mut StateProjection,
        event: &Event,
    ) -> Result<(), SessionSyncError> {
        if !event.stale
            && event.kind.starts_with("task.")
            && event.data["task"]["state"] == "merged"
        {
            return Err("raw task event cannot certify merged".into());
        }
        #[cfg(test)]
        PREPARE_VISITS.with(|visits| {
            let (tasks, projection) = visits.get();
            visits.set((tasks + 1, projection));
        });
        let change = tasks.prepare(event)?;
        #[cfg(test)]
        PREPARE_VISITS.with(|visits| {
            let (tasks, projection) = visits.get();
            visits.set((tasks, projection + 1));
        });
        projection.apply_recorded(event)?;
        tasks.apply_prepared(change);
        Ok(())
    }
    fn prepare(
        capture: SessionCapture,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
    ) -> Result<PreparedSession, SessionSyncError> {
        let history = crate::recovery_git::prepare_session_history(catalog, inputs, &capture)?;
        let sources = history.snapshot_sources();
        let mut acknowledged = None;
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
            acknowledged = Some(saved);
        }
        let facts = history.snapshot_summary_with_ack(acknowledged.as_ref())?;
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
            Self::apply_prepared_event(&mut tasks, &mut projection, event)?;
            cursor = event.seq;
        }
        Ok(PreparedSession {
            capture,
            projection,
            tasks,
            saved,
            facts,
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
        Self::sync_shared_bound(session, catalog, inputs, now, None)
    }
    /// Lightweight maintenance may install ordinary verified deltas. Canonical
    /// or changed-source preparation is returned to the caller's one worker slot.
    pub fn tick_incremental_shared(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        host: &str,
        now: u64,
    ) -> Result<(), SessionSyncError> {
        Self::sync_shared_stage(
            session,
            catalog,
            inputs,
            now,
            None,
            Preparation::Incremental,
        )?;
        let (result, changed) = {
            let mut live = session.lock().map_err(|_| "session lock poisoned")?;
            live.bind_catalog(catalog)?;
            let cursor = live.cursor;
            let batch = live
                .hub
                .subscribe(cursor, now)
                .map_err(|error| error.to_string())?;
            if !batch.events.is_empty()
                || batch.head != live.head
                || live.hub.contiguous_seq() != cursor
            {
                return Err(SessionSyncError::SourceChanged(
                    "head changed before incremental maintenance".into(),
                ));
            }
            let live = &mut *live;
            let result = live.tasks.expire(&mut *live.hub, host, now);
            (result, live.hub.contiguous_seq() != cursor)
        };
        // Expiration is independent of snapshot retry eligibility. Any accepted
        // release still passes the same capture/source/CAS engine before display.
        if changed {
            Self::sync_shared_stage(
                session,
                catalog,
                inputs,
                now,
                None,
                Preparation::Incremental,
            )?;
        }
        result.map_err(Into::into)
    }
    fn sync_shared_bound(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
        expected_private: Option<&Option<String>>,
    ) -> Result<(), SessionSyncError> {
        Self::sync_shared_stage(
            session,
            catalog,
            inputs,
            now,
            expected_private,
            Preparation::Complete,
        )
    }
    fn sync_shared_stage(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
        expected_private: Option<&Option<String>>,
        preparation: Preparation,
    ) -> Result<(), SessionSyncError> {
        for _ in 0..3 {
            if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
                return Err(SessionSyncError::SourceChanged(
                    "authority changed; reload Catalog".into(),
                ));
            }
            let private = crate::snapshot_git::private_revision(&catalog.root)?;
            if expected_private.is_some_and(|expected| *expected != private) {
                return Err(SessionSyncError::SourceChanged(
                    "private snapshot source changed during request".into(),
                ));
            }
            if matches!(preparation, Preparation::Incremental) {
                // No-change maintenance uses the existing installed binding and
                // actual protocol head. It does not clone/hash the historical tail.
                let observed = {
                    let mut s = session.lock().map_err(|_| "session lock poisoned")?;
                    s.bind_catalog(catalog)?;
                    let binding = (
                        catalog.revision.clone(),
                        private.clone(),
                        inputs.digest().to_string(),
                    );
                    if s.verified_sources.as_ref() == Some(&binding)
                        && s.prepared_key.is_some()
                        && s.prepared_head.as_ref() == Some(&s.head)
                        && s.hub.contiguous_seq() == s.cursor
                    {
                        let cursor = s.cursor;
                        let batch = s
                            .hub
                            .subscribe(cursor, now)
                            .map_err(|error| SessionSyncError::Rejected(error.to_string()))?;
                        if batch.events.is_empty() && batch.head == s.head {
                            Some((s.cursor, s.prepared_key.clone(), s.head.clone(), binding))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some((cursor, key, head, binding)) = observed {
                    Self::probe_actual_binding(catalog, inputs, &private)?;
                    let s = session.lock().map_err(|_| "session lock poisoned")?;
                    s.ensure_active()?;
                    if s.cursor == cursor
                        && s.prepared_key == key
                        && s.head == head
                        && s.prepared_head.as_ref() == Some(&head)
                        && s.verified_sources.as_ref() == Some(&binding)
                        && s.hub.contiguous_seq() == cursor
                    {
                        return Ok(());
                    }
                    continue;
                }
            }
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
            let (recorded, tasks) = {
                let s = session.lock().map_err(|_| "session lock poisoned")?;
                (RecordedPrefix::new(&s), s.tasks.clone())
            };
            let delta =
                Self::prepare_task_delta(recorded, tasks, &capture, catalog, inputs, &private)?;
            if let Some(delta) = delta {
                Self::probe_actual_binding(catalog, inputs, &private)?;
                let mut s = session.lock().map_err(|_| "session lock poisoned")?;
                s.bind_catalog(catalog)?;
                if !delta.before.business_matches(&s) || s.capture(now)? != capture {
                    continue;
                }
                s.install_task_delta(capture, delta, catalog, inputs, &private)?;
                return Ok(());
            }
            if matches!(preparation, Preparation::Incremental) {
                return Err(SessionSyncError::PreparationRequired);
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
    fn prepare_task_delta(
        before: RecordedPrefix,
        mut tasks: TaskService,
        capture: &SessionCapture,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        private: &Option<String>,
    ) -> Result<Option<PreparedTaskDelta>, SessionSyncError> {
        let recorded = &before.session;
        if capture.fresh.is_empty()
            || capture.restored
            || capture.cursor != recorded.cursor
            || capture.previous_head != before.observed_head
            || recorded.head.contiguous_seq != recorded.cursor
            || capture.base != recorded.base
            || capture.head.floor_seq != recorded.head.floor_seq
            || capture.head.ack != recorded.head.ack
            || !capture.tail.starts_with(&recorded.tail)
            || capture.tail[recorded.tail.len()..] != capture.fresh
            || before.proof_key.as_ref() != Some(&Self::key(recorded, catalog, inputs, private)?)
            || before.sources.as_ref()
                != Some(&(
                    catalog.revision.clone(),
                    private.clone(),
                    inputs.digest().into(),
                ))
            || capture.fresh.iter().any(|event| {
                event.legacy
                    || (event.lease_delta.is_none()
                        && !matches!(
                            event.kind.as_str(),
                            "task.state" | "task.claimed" | "task.released"
                        ))
                    || (!event.stale
                        && event.kind.starts_with("task.")
                        && event.data["task"]["state"] == "merged")
            })
        {
            return Ok(None);
        }
        let mut system = crate::hub::SystemState::from_checkpoint(&crate::hub::SystemCheckpoint {
            floor_seq: recorded.head.floor_seq,
            epoch_high_water: recorded.head.epoch_high_water,
            leases: recorded.head.leases.clone(),
            receipts: Default::default(),
            ack: recorded.head.ack.clone(),
            checkpoint: serde_json::json!({}),
            legacy_through_seq: 0,
        })
        .map_err(|error| SessionSyncError::Rejected(error.to_string()))?;
        let mut projection = before.projection.clone();
        let mut through = recorded.cursor;
        for event in &capture.fresh {
            if through.checked_add(1) != Some(event.seq) {
                return Err("non-contiguous protocol events".into());
            }
            system
                .replay(event, 0)
                .map_err(|error| SessionSyncError::Rejected(error.to_string()))?;
            Self::apply_prepared_event(&mut tasks, &mut projection, event)?;
            through = event.seq;
        }
        if through != capture.head.contiguous_seq
            || system.leases != capture.head.leases
            || system.epoch != capture.head.epoch_high_water
        {
            return Err("incremental system replay differs from actual Hub head".into());
        }
        Ok(Some(PreparedTaskDelta {
            before,
            projection,
            tasks,
        }))
    }
    fn install_task_delta(
        &mut self,
        capture: SessionCapture,
        prepared: PreparedTaskDelta,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        private: &Option<String>,
    ) -> Result<(), SessionSyncError> {
        // Both source gate and capture/business/pending CAS have passed. No old
        // history is applied again, and authority_saved still belongs to that key.
        let installed = SessionCapture {
            cursor: capture.head.contiguous_seq,
            previous_head: capture.head.clone(),
            head: capture.head.clone(),
            base: capture.base.clone(),
            tail: capture.tail.clone(),
            fresh: vec![],
            restored: false,
        };
        let key = Self::key(&installed, catalog, inputs, private)?;
        for event in &capture.fresh {
            self.observe(event)?;
        }
        let pending = self.tasks.pending_claim.take();
        self.tasks = prepared.tasks;
        self.tasks.pending_claim = pending;
        self.projection = prepared.projection;
        self.applied_count += capture.fresh.len() as u64;
        self.cursor = installed.cursor;
        self.summary.cursor = self.cursor;
        self.head = capture.head;
        self.base = capture.base;
        self.tail = capture.tail;
        self.prepared_key = Some(key);
        self.prepared_head = Some(self.head.clone());
        self.inputs = inputs.clone();
        Ok(())
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
        let generation = self.summary.generation.saturating_add(1);
        self.summary = prepared.facts;
        self.summary.generation = generation;
        self.first_business_ms = None;
        self.last_snapshot_ms = self.summary.last.as_ref().and_then(|last| last.written_at);
        self.periodic_pending_since = None;
        self.authority_saved = prepared.saved;
        self.base = prepared.capture.base;
        self.cursor = self.base.as_ref().map_or(0, |b| b.through_seq);
        self.tail = vec![];
        for event in prepared.capture.tail {
            self.observe(&event)?;
            self.cursor = event.seq;
            self.tail.push(event);
        }
        self.head = prepared.capture.head;
        self.summary.cursor = self.cursor;
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
        self.prepared_head = Some(self.head.clone());
        self.verified_sources = Some((
            catalog.revision.clone(),
            private.clone(),
            inputs.digest().into(),
        ));
        self.authority_reads += 1;
        Ok(())
    }
    fn observe(&mut self, event: &Event) -> Result<(), String> {
        let through = self.authority_saved.as_ref().map_or_else(
            || self.base.as_ref().map_or(0, |base| base.through_seq),
            |saved| saved.snapshot.through_seq,
        );
        self.summary.observe(event, through)?;
        self.periodic_pending_since = self.summary.periodic_pending_since;
        self.last_snapshot_ms = self.summary.last.as_ref().and_then(|last| last.written_at);
        if event.seq > through
            && !event.stale
            && !event.is_pure_heartbeat()
            && !is_snapshot_self_event(event)
            && !(event.obj == "main" && event.kind.starts_with("lease."))
        {
            self.first_business_ms.get_or_insert(event_millis(event)?);
        }
        Ok(())
    }
    pub(crate) fn sync(&mut self, now: u64) -> Result<(), SessionSyncError> {
        let summary = self.summary.clone();
        let first = self.first_business_ms;
        let periodic = self.periodic_pending_since;
        let last = self.last_snapshot_ms;
        let result = self.sync_events(now);
        if result.is_err() {
            self.summary = summary;
            self.first_business_ms = first;
            self.periodic_pending_since = periodic;
            self.last_snapshot_ms = last;
        }
        result
    }
    fn sync_events(&mut self, now: u64) -> Result<(), SessionSyncError> {
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
            self.prepared_head = None;
            self.observe(&event)?;
            self.cursor = event.seq;
            self.applied_count += 1;
            self.tail.push(event);
        }
        self.head = batch.head;
        self.summary.cursor = self.cursor;
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
            summary: SnapshotSummary::default(),
            periodic_pending_since: None,
            first_business_ms: None,
            last_snapshot_ms: None,
            applied_count: 0,
            authority_reads: 0,
            context: crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?,
            root: catalog.root.clone(),
            common_dir: catalog.common_dir.clone(),
            inputs: inputs.clone(),
            prepared_key: None,
            prepared_head: None,
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
        let facts = crate::recovery_git::prepare_session_history(catalog, inputs, &capture)?
            .snapshot_summary()?;
        self.summary.last = facts.last;
        self.summary.alert = facts.alert;
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
        self.summary.cursor = self.cursor;
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
    pub(crate) fn read_status(
        &mut self,
        catalog: &Catalog,
        host: &str,
        run: Option<&str>,
        now: u64,
    ) -> Result<Value, TaskFailure> {
        self.bind_catalog(catalog)?;
        self.refresh_system_head(now)?;
        let mut value = self
            .tasks
            .read_status(&mut *self.hub, catalog, host, run, now)?;
        self.refresh_system_head(now)?;
        value["seq"] = serde_json::json!(self.cursor);
        if self.head.contiguous_seq > self.cursor {
            value["pending_verification"] = serde_json::json!(true);
        }
        Ok(value)
    }
    fn probe_actual_binding(
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        private: &Option<String>,
    ) -> Result<(), SessionSyncError> {
        inputs
            .probe(catalog)
            .map_err(SessionSyncError::SourceChanged)?;
        if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
            return Err(SessionSyncError::SourceChanged(
                "authority changed; reload Catalog".into(),
            ));
        }
        if &crate::snapshot_git::private_revision(&catalog.root)? != private {
            return Err(SessionSyncError::SourceChanged(
                "private snapshot source changed".into(),
            ));
        }
        Ok(())
    }
    // Only shared synchronization owns the three-round preparation budget.
    // A newly unprepared continuation ends this request; the closure runs once.
    fn continue_shared<T>(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
        continuation: impl FnOnce(&mut Self) -> T,
    ) -> Result<T, SessionSyncError> {
        Self::continue_shared_bound(session, catalog, inputs, now, None, continuation)
    }
    fn continue_shared_bound<T>(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
        expected_private: Option<&Option<String>>,
        continuation: impl FnOnce(&mut Self) -> T,
    ) -> Result<T, SessionSyncError> {
        Self::sync_shared_bound(session, catalog, inputs, now, expected_private)?;
        let mut live = session.lock().map_err(|_| "session lock poisoned")?;
        live.bind_catalog(catalog)?;
        let capture = live.capture(now)?;
        if capture.fresh.is_empty() && !capture.restored && capture.head == capture.previous_head {
            return Ok(continuation(&mut live));
        }
        Err(SessionSyncError::SourceChanged(
            "head changed before continuation".into(),
        ))
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
        if matches!(command, TaskCommand::Status) {
            return Self::read_status_shared(session, catalog, inputs, host, run, now);
        }
        // The command is owned by a single-use continuation. If expiration writes
        // a new tail, prove that tail first, then continue only the command.
        let show = matches!(&command, TaskCommand::Show { .. });
        let mut operation = Some(|live: &mut Self| {
            let previous_pending = live
                .tasks
                .pending_claim
                .as_ref()
                .map(|pending| pending.event.eid.clone());
            let show_before = show.then(|| RecordedPrefix::new(live));
            let (result, renewal) =
                live.tasks
                    .execute_captured(&mut *live.hub, catalog, host, command, run, now);
            CommandResult {
                result,
                pending: live.tasks.pending_claim.clone(),
                previous_pending,
                show_before,
                renewal,
            }
        });
        let (expired, executed, private) =
            Self::continue_shared(session, catalog, inputs, now, |live| {
                let private = live
                    .verified_sources
                    .as_ref()
                    .expect("prepared continuation has verified sources")
                    .1
                    .clone();
                let expired = live.tasks.expire(&mut *live.hub, host, now);
                let executed = if expired.is_ok() && live.hub.contiguous_seq() == live.cursor {
                    #[cfg(test)]
                    eprintln!(
                        "command continuation coalesced: cursor={} head={}",
                        live.cursor,
                        live.hub.contiguous_seq()
                    );
                    Some(operation.take().expect("command continuation runs once")(
                        live,
                    ))
                } else {
                    None
                };
                (expired, executed, private)
            })?;
        let outcome = match executed {
            Some(result) => result,
            None => {
                #[cfg(test)]
                eprintln!("command continuation waits for expiration tail proof");
                Self::finish_shared(session, catalog, inputs, now, &private, expired)?;
                Self::continue_shared_bound(
                    session,
                    catalog,
                    inputs,
                    now,
                    Some(&private),
                    |live| operation.take().expect("command continuation runs once")(live),
                )?
            }
        };
        let CommandResult {
            result,
            pending,
            previous_pending,
            show_before,
            renewal,
        } = outcome;
        let read_completion = match (&result, show_before.as_ref()) {
            (Ok(_), Some(before)) => Self::finish_show_shared(
                session,
                catalog,
                inputs,
                now,
                &private,
                before,
                renewal.as_ref(),
            ),
            _ => Ok(false),
        };
        // Accepted facts remain in the Hub, but only a final actual proof can
        // advance the live business view. Never fast-apply this tail first.
        let completion = read_completion.and_then(|finished| {
            if finished {
                Ok(())
            } else {
                Self::sync_shared_bound(session, catalog, inputs, now, Some(&private))
                    .and_then(|()| Self::probe_actual_binding(catalog, inputs, &private))
            }
        });
        if let Err(error) = completion {
            return match result {
                Err(mut original) => { original.message = format!("{}; synchronization pending: {error}", original.message); Err(original) }
                Ok(value) => Err(match pending {
                    Some(pending) => format!("accepted claim requires synchronization; original result retained; eid={} run_id={}: {error}", pending.event.eid, pending.run_id),
                    None => format!("accepted request requires synchronization; original_result={value}: {error}"),
                }.into()),
            };
        }
        if result.is_ok() {
            if let Some(pending) =
                pending.filter(|pending| previous_pending.as_ref() != Some(&pending.event.eid))
            {
                let mut live = session.lock().map_err(|_| "session lock poisoned")?;
                if live
                    .tasks
                    .pending_claim
                    .as_ref()
                    .is_some_and(|current| current.event == pending.event)
                {
                    live.tasks.pending_claim = None;
                }
            }
        }
        result
    }
    // A read may keep its verified business prefix while only its own actual
    // renewal changes the system head. This never installs or applies new facts.
    fn finish_show_shared(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
        private: &Option<String>,
        before: &RecordedPrefix,
        renewal: Option<&Event>,
    ) -> Result<bool, SessionSyncError> {
        let snapshot = &before.session;
        let capture = {
            let mut live = session.lock().map_err(|_| "session lock poisoned")?;
            live.bind_catalog(catalog)?;
            let capture = live.capture(now)?;
            let expected: Vec<_> = renewal.into_iter().cloned().collect();
            let mut tail = snapshot.tail.clone();
            tail.extend(expected.clone());
            if !before.business_matches(&live)
                || snapshot.head != snapshot.previous_head
                || capture.previous_head != snapshot.head
                || capture.cursor != snapshot.cursor
                || capture.base != snapshot.base
                || capture.restored
                || capture.tail != tail
                || capture.fresh != expected
                || capture.head.floor_seq != snapshot.head.floor_seq
                || capture.head.ack != snapshot.head.ack
                || capture.head.epoch_high_water != snapshot.head.epoch_high_water
                || capture.head.contiguous_seq
                    != snapshot
                        .head
                        .contiguous_seq
                        .checked_add(expected.len() as u64)
                        .ok_or("sequence overflow")?
            {
                return Ok(false);
            }
            let mut system =
                crate::hub::SystemState::from_checkpoint(&crate::hub::SystemCheckpoint {
                    floor_seq: snapshot.head.floor_seq,
                    epoch_high_water: snapshot.head.epoch_high_water,
                    leases: snapshot.head.leases.clone(),
                    receipts: Default::default(),
                    ack: snapshot.head.ack.clone(),
                    checkpoint: serde_json::json!({}),
                    legacy_through_seq: 0,
                })
                .map_err(|error| SessionSyncError::Rejected(error.to_string()))?;
            for event in &expected {
                if !matches!(
                    event.lease_delta,
                    Some(crate::hub::SystemDelta::Renew { .. })
                ) || event.legacy
                {
                    return Ok(false);
                }
                system
                    .replay(event, 0)
                    .map_err(|error| SessionSyncError::Rejected(error.to_string()))?;
            }
            if system.leases != capture.head.leases || system.epoch != capture.head.epoch_high_water
            {
                return Ok(false);
            }
            capture
        };
        Self::probe_actual_binding(catalog, inputs, private)?;
        let mut live = session.lock().map_err(|_| "session lock poisoned")?;
        live.bind_catalog(catalog)?;
        if !before.business_matches(&live) || live.capture(now)? != capture {
            return Ok(false);
        }
        live.head = capture.head;
        Ok(true)
    }
    fn finish_shared<T>(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
        private: &Option<String>,
        result: Result<T, String>,
    ) -> Result<T, String> {
        let completion = Self::sync_shared_bound(session, catalog, inputs, now, Some(private))
            .and_then(|()| Self::probe_actual_binding(catalog, inputs, private));
        match completion {
            Ok(()) => result,
            Err(error) => Err(match result {
                Err(original) => format!("{original}; synchronization pending: {error}"),
                Ok(_) => format!("accepted request requires synchronization; original operation must not be repeated: {error}"),
            }),
        }
    }
    pub fn tick_shared(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        host: &str,
        now: u64,
    ) -> Result<(), String> {
        let (result, private) = Self::continue_shared(session, catalog, inputs, now, |live| {
            let private = live
                .verified_sources
                .as_ref()
                .expect("prepared continuation has verified sources")
                .1
                .clone();
            let result = live.tasks.expire(&mut *live.hub, host, now);
            (result, private)
        })
        .map_err(|error| error.to_string())?;
        Self::finish_shared(session, catalog, inputs, now, &private, result)
    }
    pub fn reconcile_pending_shared(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        now: u64,
    ) -> Result<(), String> {
        let (result, original, private) =
            Self::continue_shared(session, catalog, inputs, now, |live| {
                let private = live
                    .verified_sources
                    .as_ref()
                    .expect("prepared continuation has verified sources")
                    .1
                    .clone();
                let original = live.tasks.pending_claim.clone();
                let result = live.tasks.reconcile_pending(&mut *live.hub, now);
                (result, original, private)
            })
            .map_err(|error| error.to_string())?;
        Self::finish_shared(session, catalog, inputs, now, &private, result).map_err(|error| {
            original.as_ref().map_or_else(
                || error.clone(),
                |pending| {
                    format!(
                        "original claim requires synchronization; eid={} run_id={}: {error}",
                        pending.event.eid, pending.run_id
                    )
                },
            )
        })?;
        let mut live = session.lock().map_err(|_| "session lock poisoned")?;
        if live
            .tasks
            .pending_claim
            .as_ref()
            .map(|pending| &pending.event.eid)
            == original.as_ref().map(|pending| &pending.event.eid)
        {
            live.tasks.pending_claim = None;
        }
        Ok(())
    }
    pub fn subscribe_shared(
        session: &std::sync::Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        after: u64,
        now: u64,
    ) -> Result<SubscriptionBatch, HubError> {
        let (batch, private) = Self::continue_shared(session, catalog, inputs, now, |live| {
            let private = live
                .verified_sources
                .as_ref()
                .expect("prepared continuation has verified sources")
                .1
                .clone();
            let batch = live.hub.subscribe(after, now).and_then(|batch| {
                if batch.events.iter().any(|event| event.seq > live.cursor) {
                    Err(HubError::Invalid(
                        "subscription exceeds applied prefix".into(),
                    ))
                } else {
                    Ok(batch)
                }
            });
            (batch, private)
        })
        .map_err(|error| HubError::Invalid(error.to_string()))?;
        let batch = batch?;
        Self::probe_actual_binding(catalog, inputs, &private)
            .map_err(|error| HubError::Invalid(error.to_string()))?;
        Ok(batch)
    }
    pub fn renew_binding(
        &mut self,
        catalog: &Catalog,
        host: &str,
        run: Option<&str>,
        now: u64,
    ) -> Result<(), String> {
        self.bind_catalog(catalog)?;
        self.refresh_system_head(now)?;
        self.tasks
            .renew_binding(&mut *self.hub, catalog, host, run, now)?;
        self.refresh_system_head(now)
    }
    pub(crate) fn tick(&mut self, host: &str, now: u64) -> Result<(), String> {
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
        self.summary.generation = self.summary.generation.saturating_add(1);
        self.summary.fast_pending = None;
        self.summary.periodic_pending_since = None;
        self.summary.pending_through = None;
        self.first_business_ms = None;
        self.periodic_pending_since = None;
        for event in self.tail.clone() {
            // Event timestamps were checked at the successful prefix installation.
            self.observe(&event).expect("installed event timestamp");
        }
    }
    pub(crate) fn installed_written(
        &mut self,
        commit: &str,
        snapshot: &Snapshot,
        receipt: &AppendReceipt,
    ) -> Result<SnapshotLast, String> {
        let last = self.written_observation(commit, snapshot, receipt)?;
        self.summary.last = Some(last.clone());
        self.last_snapshot_ms = last.written_at;
        Ok(last)
    }
    fn written_observation(
        &self,
        commit: &str,
        snapshot: &Snapshot,
        receipt: &AppendReceipt,
    ) -> Result<SnapshotLast, String> {
        let mut last = SnapshotLast {
            commit: commit.into(),
            seq: snapshot.through_seq,
            epoch: snapshot.writer_epoch,
            written_at: None,
            written_seq: match receipt {
                AppendReceipt::Duplicate { seq } => Some(*seq),
                _ => None,
            },
        };
        if let AppendReceipt::Event(event) = receipt {
            if matching_written(event, &last) {
                last.written_at = Some(event_millis(event)?);
                last.written_seq = Some(event.seq);
            }
        }
        if last.written_at.is_none() {
            if let Some(event) = self
                .tail
                .iter()
                .find(|event| matching_written(event, &last))
            {
                last.written_at = Some(event_millis(event)?);
                last.written_seq = Some(event.seq);
            } else if let Some(old) = &self.summary.last {
                if old.commit == last.commit && old.seq == last.seq && old.epoch == last.epoch {
                    last.written_at = old.written_at;
                    last.written_seq = old.written_seq;
                }
            }
        }
        Ok(last)
    }

    pub(crate) fn completed_snapshot_observation(&mut self) {
        self.summary.alert = None;
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
}
fn event_millis(event: &Event) -> Result<u64, String> {
    let time = chrono::DateTime::parse_from_rfc3339(&event.t)
        .map_err(|e| e.to_string())?
        .timestamp_millis();
    u64::try_from(time).map_err(|_| "negative event time".into())
}

#[cfg(test)]
mod show_capture_tests {
    use super::*;
    #[test]
    fn show_capture_checks_whole_projection_and_pending_request_identity() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(dir.path());
        let inputs = crate::test_support::inputs(&catalog);
        let mut live = ProjectSession::open(&dir.path().join("hub"), &catalog, &inputs, 0).unwrap();
        let pending = crate::tasks::PendingClaim {
            event: crate::hub_protocol::NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:towermd-cli".into(),
                kind: "task.claimed".into(),
                layer: "L5".into(),
                obj: "T".into(),
                guards: vec![crate::hub_protocol::Fence {
                    name: "task:T".into(),
                    holder: "host".into(),
                    epoch: 1,
                }],
                data: serde_json::json!({"run":{"id":"R","state":"running"}}),
            },
            run_id: "R".into(),
        };
        live.tasks.pending_claim = Some(pending.clone());
        let before = RecordedPrefix::new(&live);
        assert!(before.business_matches(&live));
        for coordinate in ["eid", "run", "result", "guards", "cleared"] {
            live.tasks.pending_claim = Some(pending.clone());
            match coordinate {
                "eid" => live
                    .tasks
                    .pending_claim
                    .as_mut()
                    .unwrap()
                    .event
                    .eid
                    .push('x'),
                "run" => live.tasks.pending_claim.as_mut().unwrap().run_id.push('x'),
                "result" => {
                    live.tasks.pending_claim.as_mut().unwrap().event.data["run"]["state"] =
                        serde_json::json!("ended")
                }
                "guards" => live.tasks.pending_claim.as_mut().unwrap().event.guards[0].epoch += 1,
                _ => live.tasks.pending_claim = None,
            }
            assert!(!before.business_matches(&live), "{coordinate}");
            assert_eq!(live.capture(0).unwrap().head, before.session.head);
        }
        live.tasks.pending_claim = Some(pending);
        live.projection
            .objects
            .entry("Task".into())
            .or_default()
            .insert("T".into(), serde_json::json!({"state":"merged"}));
        assert!(!before.business_matches(&live));
        assert_eq!(live.capture(0).unwrap().head, before.session.head);
    }
}

#[cfg(test)]
mod incremental_proof_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };
    #[test]
    fn ordinary_task_delta_never_rereads_archive_or_replays_verified_history() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(dir.path());
        let inputs = crate::test_support::inputs(&catalog);
        let hub = Mutex::new(
            ProjectSession::open(&dir.path().join("hub"), &catalog, &inputs, 0).unwrap(),
        );
        for number in 0..32 {
            hub.lock()
                .unwrap()
                .hub
                .append(
                    NewEvent {
                        eid: uuid::Uuid::now_v7().to_string(),
                        host: "host".into(),
                        actor: "process:audit".into(),
                        kind: "future.audit".into(),
                        layer: "L4".into(),
                        obj: format!("archived-{number}"),
                        data: serde_json::json!({"number":number}),
                        guards: vec![],
                    },
                    1,
                )
                .unwrap();
        }
        SnapshotWriterForTest::default()
            .write(&hub, &catalog, "host", true, || Ok(2))
            .unwrap();
        let current = Catalog::load(dir.path()).unwrap();
        let inputs = crate::test_support::inputs(&current);
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = reads.clone();
        crate::snapshot_service::authority_concurrency_tests::set_hook(
            current.root.clone(),
            Some(Arc::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            })),
        );
        ProjectSession::sync_shared(&hub, &current, &inputs, 3).unwrap();
        let archive_reads = reads.load(Ordering::SeqCst);
        assert!(
            archive_reads > 0,
            "fixture must actually decode a persisted archive"
        );
        let source = crate::snapshot_git::private_revision(dir.path()).unwrap();
        let ack = hub.lock().unwrap().snapshot_ack().cloned();
        let base = hub.lock().unwrap().base.clone();
        let old_tail = hub.lock().unwrap().tail.len();
        for number in 0..16 {
            let visits = PREPARE_VISITS.with(|visits| visits.get());
            ProjectSession::execute_shared(
                &hub,
                &current,
                &inputs,
                "host",
                TaskCommand::Add {
                    task: crate::tasks::TaskSpec {
                        id: format!("T-{number}"),
                        title: "incremental".into(),
                        module: "A".into(),
                        touches: vec![],
                        allowed_paths: vec!["src/**".into()],
                        depends_on: vec![],
                        acceptance: vec!["AC".into()],
                        decisions: vec![],
                    },
                },
                None,
                4 + number,
            )
            .unwrap();
            let after = PREPARE_VISITS.with(|visits| visits.get());
            eprintln!("ordinary delta {number}: task_prepare={} projection_apply={} archive_reads={} verified_tail={}", after.0-visits.0, after.1-visits.1, reads.load(Ordering::SeqCst)-archive_reads, hub.lock().unwrap().tail.len());
            assert_eq!(
                (after.0 - visits.0, after.1 - visits.1),
                (1, 1),
                "Add {number}: only the new event may enter actual task prepare/projection apply"
            );
            assert_eq!(
                reads.load(Ordering::SeqCst),
                archive_reads,
                "Add {number}: unchanged actual archive must not be read again"
            );
            let live = hub.lock().unwrap();
            assert_eq!(live.tail.len(), old_tail + number as usize + 1);
            assert_eq!(live.base, base);
            assert_eq!(live.snapshot_ack().cloned(), ack);
            assert_eq!(
                crate::snapshot_git::private_revision(dir.path()).unwrap(),
                source
            );
        }
        crate::test_support::git(dir.path(), &["switch", "-qc", "t/T-0"]);
        let current = Catalog::load(dir.path()).unwrap();
        let check_delta = |label: &str, previous_cursor: u64, visits: (usize, usize)| {
            let live = hub.lock().unwrap();
            let applied = (live.cursor - previous_cursor) as usize;
            let after = PREPARE_VISITS.with(|visits| visits.get());
            eprintln!("mixed {label}: task_prepare={} projection_apply={} new_applied={applied} archive_reads={} head={} cursor={}", after.0-visits.0, after.1-visits.1, reads.load(Ordering::SeqCst)-archive_reads, live.head.contiguous_seq, live.cursor);
            assert_eq!(
                (after.0 - visits.0, after.1 - visits.1),
                (applied, applied),
                "{label}: no old-history prepare/apply"
            );
            assert_eq!(
                reads.load(Ordering::SeqCst),
                archive_reads,
                "{label}: no unchanged archive reread"
            );
        };
        let before = hub.lock().unwrap().cursor;
        let visits = PREPARE_VISITS.with(|visits| visits.get());
        let claimed = ProjectSession::execute_shared(
            &hub,
            &current,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T-0".into(),
                harness: None,
                harness_version: None,
            },
            None,
            30,
        )
        .unwrap();
        check_delta("Claim", before, visits);
        let run = claimed["id"].as_str().unwrap();
        let before = hub.lock().unwrap().cursor;
        let visits = PREPARE_VISITS.with(|visits| visits.get());
        ProjectSession::read_status_shared(&hub, &current, &inputs, "host", Some(run), 31).unwrap();
        hub.lock()
            .unwrap()
            .renew_binding(&current, "host", Some(run), 32)
            .unwrap();
        {
            let live = hub.lock().unwrap();
            assert!(live.head.contiguous_seq > live.cursor);
        }
        check_delta("Status and system renewal", before, visits);
        let before = hub.lock().unwrap().cursor;
        let visits = PREPARE_VISITS.with(|visits| visits.get());
        let shown = ProjectSession::execute_shared(
            &hub,
            &current,
            &inputs,
            "host",
            TaskCommand::Show { id: None },
            Some(run),
            33,
        )
        .unwrap();
        assert_eq!(shown["id"], run);
        check_delta("Show after system head advance", before, visits);
        let before = hub.lock().unwrap().cursor;
        let visits = PREPARE_VISITS.with(|visits| visits.get());
        ProjectSession::execute_shared(
            &hub,
            &current,
            &inputs,
            "host",
            TaskCommand::Add {
                task: crate::tasks::TaskSpec {
                    id: "T-extra".into(),
                    title: "after Show".into(),
                    module: "A".into(),
                    touches: vec![],
                    allowed_paths: vec!["src/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC".into()],
                    decisions: vec![],
                },
            },
            Some(run),
            34,
        )
        .unwrap();
        check_delta("Add after Show head advance", before, visits);
        let before = hub.lock().unwrap().cursor;
        let visits = PREPARE_VISITS.with(|visits| visits.get());
        ProjectSession::execute_shared(
            &hub,
            &current,
            &inputs,
            "host",
            TaskCommand::Release,
            Some(run),
            35,
        )
        .unwrap();
        check_delta("Release", before, visits);
        crate::test_support::git(dir.path(), &["switch", "-qc", "t/T-1"]);
        let current = Catalog::load(dir.path()).unwrap();
        let before = hub.lock().unwrap().cursor;
        let visits = PREPARE_VISITS.with(|visits| visits.get());
        ProjectSession::execute_shared(
            &hub,
            &current,
            &inputs,
            "host",
            TaskCommand::Claim {
                id: "T-1".into(),
                harness: None,
                harness_version: None,
            },
            None,
            36,
        )
        .unwrap();
        check_delta("second Claim", before, visits);
        let before = hub.lock().unwrap().cursor;
        let visits = PREPARE_VISITS.with(|visits| visits.get());
        ProjectSession::tick_shared(&hub, &current, &inputs, "host", 15 * 60 * 1000 + 37).unwrap();
        check_delta("expired Tick", before, visits);
        let visits = PREPARE_VISITS.with(|visits| visits.get());
        for _ in 0..10 {
            ProjectSession::tick_shared(&hub, &current, &inputs, "host", 15 * 60 * 1000 + 38)
                .unwrap();
            ProjectSession::read_status_shared(
                &hub,
                &current,
                &inputs,
                "host",
                None,
                15 * 60 * 1000 + 38,
            )
            .unwrap();
        }
        assert_eq!(PREPARE_VISITS.with(|visits| visits.get()), visits);
        assert_eq!(reads.load(Ordering::SeqCst), archive_reads);
        crate::snapshot_service::authority_concurrency_tests::set_hook(current.root.clone(), None);
    }
    use crate::snapshot_service::SnapshotWriter as SnapshotWriterForTest;
}

#[cfg(test)]
mod snapshot_receipt_tests {
    use super::*;
    #[test]
    fn compacted_duplicate_keeps_actual_seq_and_never_fabricates_written_time() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = crate::test_support::catalog(dir.path());
        let inputs = crate::test_support::inputs(&catalog);
        let path = dir.path().join("hub");
        let live =
            std::sync::Mutex::new(ProjectSession::open(&path, &catalog, &inputs, 0).unwrap());
        let writer = crate::snapshot_service::SnapshotWriter::default();
        let first = writer.write_observed(&live, &catalog, "host", true, || Ok(1000));
        let original = first.completed().unwrap();
        let old = crate::snapshot_service::read_snapshot(&Catalog::load(dir.path()).unwrap())
            .unwrap()
            .unwrap();
        let seq = original.last.written_seq.unwrap();
        let crate::hub_protocol::AppendReceipt::Event(event) = &original.receipt else {
            panic!("new receipt")
        };
        let replay = NewEvent {
            eid: event.eid.clone(),
            host: event.host.clone(),
            actor: event.actor.clone(),
            kind: event.kind.clone(),
            layer: event.layer.clone(),
            obj: event.obj.clone(),
            data: event.data.clone(),
            guards: event.guards.clone(),
        };
        let current = Catalog::load(dir.path()).unwrap();
        let next = writer.write_observed(&live, &current, "host", true, || Ok(2000));
        assert!(next.completed().is_some());
        let current = Catalog::load(dir.path()).unwrap();
        ProjectSession::sync_shared(
            &live,
            &current,
            &crate::test_support::inputs(&current),
            2000,
        )
        .unwrap();
        let mut session = live.lock().unwrap();
        let receipt = session.protocol().unwrap().append(replay, 999_999).unwrap();
        assert_eq!(receipt, AppendReceipt::Duplicate { seq });
        let observation = session
            .written_observation(&old.commit, &old.snapshot, &receipt)
            .unwrap();
        assert_eq!(observation.written_seq, Some(seq));
        assert_eq!(
            observation.written_at, None,
            "no matching body available to this observation; query/retry time must not be used"
        );
    }
}
