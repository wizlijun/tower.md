//! Rebuildable, long-lived business projection over the hub protocol.
use crate::catalog::Catalog;
use crate::hub_protocol::*;
use crate::snapshot::{Snapshot, StateProjection};
use crate::tasks::{TaskCommand, TaskFailure, TaskService};
use serde_json::Value;

pub struct ProjectSession {
    pub hub: Box<dyn HubProtocol>,
    identity: Option<(String, std::path::PathBuf)>,
    pub tasks: TaskService,
    pub projection: StateProjection,
    pub cursor: u64,
    pub head: HubHead,
    pub(crate) base: Option<Snapshot>,
    pub(crate) tail: Vec<Event>,
    pub(crate) authority_key: Option<String>,
    pub(crate) authority_saved: Option<crate::snapshot_service::SavedSnapshot>,
    first_business_ms: Option<u64>,
    last_snapshot_ms: Option<u64>,
    applied_count: u64,
    pub(crate) authority_reads: u64,
}
impl ProjectSession {
    pub fn open(dir: &std::path::Path) -> Result<Self, String> {
        Self::new(Box::new(crate::hub::Hub::open(dir)?), 0)
    }
    pub fn bind_catalog(&mut self, catalog: &Catalog) -> Result<(), String> {
        let identity = (
            catalog.config.project_id().to_string(),
            catalog.common_dir.clone(),
        );
        if self
            .identity
            .as_ref()
            .is_some_and(|existing| existing != &identity)
            || self
                .base
                .as_ref()
                .is_some_and(|base| base.project_id != identity.0)
        {
            return Err("core session project identity differs from authoritative Catalog".into());
        }
        self.identity = Some(identity);
        Ok(())
    }
    pub fn new(mut hub: Box<dyn HubProtocol>, now: u64) -> Result<Self, String> {
        let (head, base) = match hub.subscribe(0, now) {
            Ok(b) => (b.head, None),
            Err(HubError::SnapshotRequired { head, checkpoint }) => {
                let base: Snapshot = serde_json::from_value(checkpoint)
                    .map_err(|e| format!("invalid core checkpoint: {e}"))?;
                base.validate_metadata()?;
                if base.through_seq != head.floor_seq {
                    return Err("checkpoint floor mismatch".into());
                }
                (head, Some(base))
            }
            Err(e) => return Err(e.to_string()),
        };
        let projection = base
            .as_ref()
            .map_or_else(StateProjection::default, |b| b.projection.clone());
        let mut tasks = TaskService::default();
        tasks.restore(&projection)?;
        let cursor = base.as_ref().map_or(0, |b| b.through_seq);
        let mut session = Self {
            hub,
            identity: None,
            tasks,
            projection,
            cursor,
            head,
            base,
            tail: vec![],
            authority_key: None,
            authority_saved: None,
            first_business_ms: None,
            last_snapshot_ms: None,
            applied_count: 0,
            authority_reads: 0,
        };
        session.sync(now)?;
        Ok(session)
    }
    pub fn sync(&mut self, now: u64) -> Result<(), String> {
        let batch = match self.hub.subscribe(self.cursor, now) {
            Ok(batch) => batch,
            Err(HubError::SnapshotRequired { head, checkpoint }) => {
                let base: Snapshot = serde_json::from_value(checkpoint)
                    .map_err(|e| format!("invalid core checkpoint: {e}"))?;
                base.validate_metadata()?;
                if base.through_seq != head.floor_seq {
                    return Err("checkpoint floor mismatch".into());
                }
                self.tasks.restore(&base.projection)?;
                self.projection = base.projection.clone();
                self.cursor = head.floor_seq;
                self.base = Some(base);
                self.tail.clear();
                self.authority_key = None;
                self.first_business_ms = None;
                self.last_snapshot_ms = None;
                self.hub
                    .subscribe(self.cursor, now)
                    .map_err(|e| e.to_string())?
            }
            Err(e) => return Err(e.to_string()),
        };
        let through = self.hub.contiguous_seq();
        for event in batch.events.into_iter().take_while(|e| e.seq <= through) {
            if self.cursor.checked_add(1) != Some(event.seq) {
                return Err("non-contiguous protocol events".into());
            }
            let time = event_millis(&event)?;
            let change = self.tasks.prepare(&event)?;
            self.projection.apply_recorded(&event)?;
            self.tasks.apply_prepared(change);
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
            self.cursor = event.seq;
            self.applied_count += 1;
            self.tail.push(event);
        }
        self.head = batch.head;
        Ok(())
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
        let result = self
            .tasks
            .execute(&mut *self.hub, catalog, host, command, run, now);
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
        self.sync(now)
    }
    pub fn tick(&mut self, host: &str, now: u64) -> Result<(), String> {
        self.sync(now)?;
        self.tasks.expire(&mut *self.hub, host, now)?;
        self.sync(now)
    }
    pub fn due(&self, interval: u64, now: u64) -> bool {
        self.first_business_ms.is_some_and(|first| {
            now.saturating_sub(self.last_snapshot_ms.unwrap_or(first)) >= interval
        })
    }
    pub fn checkpoint(&self, id: &str, writer: &Fence, now: u64) -> Result<Snapshot, String> {
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
        let through = self.hub.contiguous_seq();
        match &self.base {
            Some(base) if base.project_id != id => {
                Err("checkpoint project does not match base snapshot".into())
            }
            Some(base) => Snapshot::extend(Some(base), &self.tail, through, writer.epoch),
            None => Snapshot::from_events(id, &self.tail, through, writer.epoch),
        }
    }
    pub fn install_checkpoint(&mut self, snapshot: Snapshot) {
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
        self.authority_key = None;
    }
    pub fn reconcile_pending(&mut self, now: u64) -> Result<(), String> {
        self.tasks.reconcile_pending(&mut *self.hub, now)?;
        self.sync(now)
    }
    pub fn applied_count(&self) -> u64 {
        self.applied_count
    }
    pub fn authority_reads(&self) -> u64 {
        self.authority_reads
    }
    pub fn last_seq(&self) -> u64 {
        self.hub.contiguous_seq()
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
        let batch = self.hub.subscribe(after, now)?;
        self.sync(now).map_err(HubError::Invalid)?;
        Ok(batch)
    }
}
fn event_millis(event: &Event) -> Result<u64, String> {
    let time = chrono::DateTime::parse_from_rfc3339(&event.t)
        .map_err(|e| e.to_string())?
        .timestamp_millis();
    u64::try_from(time).map_err(|_| "negative event time".into())
}
