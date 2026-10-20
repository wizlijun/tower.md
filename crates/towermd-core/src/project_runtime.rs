//! The daemon-owned local project runtime. Callers also hold the cross-clone
//! project-id lock; no App or CLI obtains this object or a writable Hub.
use crate::durable_outbox::{DurableOutbox, OutboxError};
use crate::hub::{inspect_store, Hub, StoreState};
use crate::hub_protocol::*;
use crate::main_writer::{self, WriteKind, WriteOutcome, WriteRequest};
use crate::repo;
use crate::snapshot::{recovered_original, Snapshot};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAIN_TTL_MS: u64 = 120_000;
pub const MAIN_RENEW_MS: u64 = 30_000;
pub const MODULE_TTL_MS: u64 = 900_000;
pub const SNAPSHOT_INTERVAL_MS: u64 = 30 * 60 * 1000;
pub type Reducer = fn(&mut Value, &Event) -> Result<(), String>;

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub root: PathBuf,
    pub common_dir: PathBuf,
    pub project_id: String,
    pub docs_dir: String,
    pub host: String,
}

#[derive(Debug)]
pub enum RuntimeError {
    NeedsRecovery(String),
    Invalid(String),
    Fenced(String),
    Uncertain(String),
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedsRecovery(s) | Self::Invalid(s) | Self::Fenced(s) | Self::Uncertain(s) => {
                f.write_str(s)
            }
        }
    }
}
impl std::error::Error for RuntimeError {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    version: u32,
    project_id: String,
    host: String,
    holder: String,
}

pub struct ProjectRuntime {
    config: RuntimeConfig,
    hub: Hub,
    outbox: DurableOutbox,
    marker: Marker,
    main: Lease,
    renewed_at: u64,
    initial: Value,
    reduce: Reducer,
    projection: Value,
    applied_seq: u64,
    canonical: BTreeMap<String, Event>,
    snapshot: Option<Snapshot>,
    snapshot_commit: Option<String>,
    dirty_seq: u64,
    snapshot_at: u64,
}

/// This hook is daemon code, never user-provided JSON. It must inspect actual
/// main commit trailers and business proof before returning deterministic
/// `git:<sha256>` process events. An unproved commit cannot become an event.
pub struct RecoveryEvidence<'a> {
    pub root: &'a Path,
    pub snapshot: &'a Snapshot,
    pub confirmed: &'a [Event],
    pub pending: &'a [NewEvent],
}

impl ProjectRuntime {
    pub fn open(
        config: RuntimeConfig,
        initial: Value,
        reduce: Reducer,
        mut clock: impl FnMut() -> u64,
    ) -> Result<Self, RuntimeError> {
        validate_config(&config)?;
        refresh(&config.root)?;
        let latest = read_latest(&config, &initial, reduce)?;
        let directory = config.common_dir.join("towermd");
        let marker_path = directory.join("runtime.json");
        let marker = read_marker(&marker_path, &config)?;
        let hub_path = directory.join("hub");
        let store = inspect_store(&hub_path).map_err(RuntimeError::NeedsRecovery)?;
        let active = marker.is_some() || latest.is_some();
        if active && matches!(store, StoreState::Missing | StoreState::Empty) {
            return Err(RuntimeError::NeedsRecovery(
                "已激活项目的 Hub 缺失或为空；需要 App 确认恢复".into(),
            ));
        }
        if !active && store == StoreState::Nonempty {
            return Err(RuntimeError::NeedsRecovery(
                "未标识 Hub 已有事件；不能作为空项目激活".into(),
            ));
        }
        let outbox = DurableOutbox::open(&config.common_dir)
            .map_err(|e| RuntimeError::NeedsRecovery(e.to_string()))?;
        let confirmed = outbox.confirmed().map_err(outbox_error)?;
        if !active && (!confirmed.is_empty() || !outbox.pending().map_err(outbox_error)?.is_empty())
        {
            return Err(RuntimeError::NeedsRecovery(
                "已有发件箱原件，不能激活新的空 Hub".into(),
            ));
        }
        let mut hub = if store == StoreState::Missing {
            Hub::open(&hub_path)
        } else {
            Hub::open_existing(&hub_path)
        }
        .map_err(RuntimeError::NeedsRecovery)?;
        let marker = match marker {
            Some(marker) => marker,
            None => {
                let marker = Marker {
                    version: 1,
                    project_id: config.project_id.clone(),
                    host: config.host.clone(),
                    holder: format!("daemon-{}", uuid::Uuid::now_v7()),
                };
                save_marker(&marker_path, &marker)?;
                marker
            }
        };
        let (snapshot_commit, snapshot) = match latest {
            Some((commit, snapshot)) => {
                if hub.last_seq() < snapshot.state.seq {
                    return Err(RuntimeError::NeedsRecovery(
                        "Git 快照水位高于本地 Hub；需要确认恢复".into(),
                    ));
                }
                let ack = SnapshotAck {
                    commit: commit.clone(),
                    seq: snapshot.state.seq,
                    epoch: snapshot.state.epoch,
                };
                if hub.snapshot_ack().is_some_and(|old| old.seq > ack.seq) {
                    return Err(RuntimeError::NeedsRecovery(
                        "Hub 确认的快照不在当前 Git 来源中".into(),
                    ));
                }
                for event in hub
                    .events(0)
                    .iter()
                    .take_while(|event| event.seq <= snapshot.state.seq)
                {
                    let covered = if event.is_pure_heartbeat() {
                        snapshot
                            .state
                            .omitted_heartbeats
                            .iter()
                            .any(|range| range.first <= event.seq && event.seq <= range.last)
                    } else {
                        snapshot.events.iter().any(|archived| archived == event)
                    };
                    if !covered {
                        return Err(RuntimeError::NeedsRecovery(
                            "Git 归档与 Hub 已保留原件不一致".into(),
                        ));
                    }
                }
                // Includes the crash window after Git publication and before ACK.
                HubProtocol::snapshot_ack(&mut hub, ack, checkpoint(&snapshot)?)
                    .map_err(|e| RuntimeError::NeedsRecovery(e.to_string()))?;
                (Some(commit), Some(snapshot))
            }
            None => {
                if hub.snapshot_ack().is_some() {
                    return Err(RuntimeError::NeedsRecovery(
                        "Hub 已截断，但 Git 快照缺失".into(),
                    ));
                }
                (None, None)
            }
        };
        let mut canonical = BTreeMap::new();
        let mut projection = initial.clone();
        if let Some(snapshot) = &snapshot {
            for event in &snapshot.events {
                apply(&mut projection, &mut canonical, event, reduce)
                    .map_err(RuntimeError::NeedsRecovery)?;
            }
        }
        let applied_seq = snapshot.as_ref().map_or(0, |s| s.state.seq);
        let now = clock();
        let main = match hub.leases(now).get("main").cloned() {
            Some(lease) if lease.holder == marker.holder => hub
                .lease_renew(
                    RenewLease {
                        host: marker.host.clone(),
                        fences: vec![lease.fence()],
                        ttl_ms: MAIN_TTL_MS,
                    },
                    now,
                )
                .map_err(hub_error)?
                .leases
                .remove(0),
            Some(_) => return Err(RuntimeError::Fenced("主干租约由其他持有者占用".into())),
            None => hub
                .lease_acquire(
                    AcquireLease {
                        host: marker.host.clone(),
                        names: vec!["main".into()],
                        holder: marker.holder.clone(),
                        ttl_ms: MAIN_TTL_MS,
                    },
                    now,
                )
                .map_err(hub_error)?
                .leases
                .remove(0),
        };
        let mut runtime = Self {
            config,
            hub,
            outbox,
            marker,
            main,
            renewed_at: now,
            initial,
            reduce,
            projection,
            applied_seq,
            canonical,
            snapshot,
            snapshot_commit,
            dirty_seq: applied_seq,
            snapshot_at: now,
        };
        runtime.sync()?;
        for original in confirmed {
            if runtime.canonical.get(&original.eid) != Some(&original) {
                return Err(RuntimeError::NeedsRecovery(
                    "Hub 与快照缺少发件箱已确认原件".into(),
                ));
            }
        }
        Ok(runtime)
    }

    pub fn projection(&self) -> &Value {
        &self.projection
    }
    pub fn contiguous_seq(&self) -> u64 {
        self.hub.contiguous_seq()
    }
    pub fn main_fence(&self) -> Fence {
        self.main.fence()
    }
    pub fn snapshot_commit(&self) -> Option<&str> {
        self.snapshot_commit.as_deref()
    }
    pub fn receipt(&self, eid: &str) -> Option<&Event> {
        self.canonical.get(eid)
    }
    pub fn pending(&self) -> Result<Vec<NewEvent>, RuntimeError> {
        self.outbox.pending().map_err(outbox_error)
    }

    pub fn snapshot_due(&self, now: u64) -> bool {
        self.dirty_seq
            > self
                .snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.state.seq)
            && now.saturating_sub(self.snapshot_at) >= SNAPSHOT_INTERVAL_MS
    }

    pub fn module_leases(&self, now: u64) -> BTreeMap<String, Lease> {
        self.hub
            .leases(now)
            .into_iter()
            .filter(|(name, _)| name.starts_with("module:"))
            .collect()
    }

    pub fn acquire_module_leases(
        &mut self,
        holder: &str,
        names: Vec<String>,
        ttl_ms: u64,
        mut clock: impl FnMut() -> u64,
    ) -> Result<Vec<Lease>, RuntimeError> {
        module_names(names.iter().map(String::as_str))?;
        module_ttl(ttl_ms)?;
        self.poll(clock())?;
        let result = self.hub.lease_acquire(
            AcquireLease {
                host: self.marker.host.clone(),
                names,
                holder: holder.into(),
                ttl_ms,
            },
            clock(),
        );
        self.sync()?;
        Ok(result.map_err(hub_error)?.leases)
    }

    pub fn renew_module_leases(
        &mut self,
        fences: Vec<Fence>,
        ttl_ms: u64,
        mut clock: impl FnMut() -> u64,
    ) -> Result<Vec<Lease>, RuntimeError> {
        module_names(fences.iter().map(|fence| fence.name.as_str()))?;
        module_ttl(ttl_ms)?;
        self.poll(clock())?;
        let result = self.hub.lease_renew(
            RenewLease {
                host: self.marker.host.clone(),
                fences,
                ttl_ms,
            },
            clock(),
        );
        self.sync()?;
        Ok(result.map_err(hub_error)?.leases)
    }

    pub fn release_module_leases(
        &mut self,
        fences: Vec<Fence>,
        mut clock: impl FnMut() -> u64,
    ) -> Result<(), RuntimeError> {
        module_names(fences.iter().map(|fence| fence.name.as_str()))?;
        self.poll(clock())?;
        let result = self.hub.lease_release(
            ReleaseLease {
                host: self.marker.host.clone(),
                fences,
            },
            clock(),
        );
        self.sync()?;
        result.map_err(hub_error)?;
        Ok(())
    }

    pub fn tick(&mut self, mut clock: impl FnMut() -> u64) -> Result<(), RuntimeError> {
        let now = clock();
        self.poll(now)?;
        self.check_fence(now)?;
        if now.saturating_sub(self.renewed_at) >= MAIN_RENEW_MS {
            self.main = self
                .hub
                .lease_renew(
                    RenewLease {
                        host: self.marker.host.clone(),
                        fences: vec![self.main.fence()],
                        ttl_ms: MAIN_TTL_MS,
                    },
                    now,
                )
                .map_err(hub_error)?
                .leases
                .remove(0);
            self.renewed_at = now;
        }
        self.poll(clock())
    }

    pub fn append(
        &mut self,
        input: NewEvent,
        mut clock: impl FnMut() -> u64,
    ) -> Result<Event, RuntimeError> {
        if matches!(
            input.kind.as_str(),
            "recovery.confirmed" | "recovery.completed" | "hub.rebuild.confirmed"
        ) || input.kind.starts_with("lease.")
        {
            return Err(RuntimeError::Invalid(
                "普通 producer 不能制造系统或恢复事件".into(),
            ));
        }
        crate::hub::validate_input(&input).map_err(hub_error)?;
        if let Some(original) = self.canonical.get(&input.eid) {
            if !matches_input(original, &input) {
                return Err(RuntimeError::Invalid("同一 eid 的请求正文不同".into()));
            }
            let original = original.clone();
            let receipt = self
                .outbox
                .deliver(&mut self.hub, input, clock(), |_, seq, eid| {
                    Ok((original.seq == seq && original.eid == eid).then(|| original.clone()))
                })
                .map_err(outbox_error)?;
            if receipt != original {
                return Err(RuntimeError::NeedsRecovery(
                    "幂等追加未返回原 canonical Event".into(),
                ));
            }
            return Ok(receipt);
        }
        match self.tick(&mut clock) {
            // Ordinary events are not governed by the main writer lease.
            Err(RuntimeError::Fenced(_)) | Ok(()) => {}
            Err(error) => return Err(error),
        }
        let now = clock();
        // Pure preflight only: this envelope is neither stored nor returned as
        // evidence. It prevents an invalid business transition poisoning Hub.
        let trial = trial_event(&self.hub, &input, now)?;
        if !trial.stale {
            (self.reduce)(&mut self.projection.clone(), &trial).map_err(RuntimeError::Invalid)?;
        }
        let known = &self.canonical;
        let result = self
            .outbox
            .deliver(&mut self.hub, input, now, |hub, seq, eid| {
                Ok(hub
                    .events(seq.saturating_sub(1))
                    .into_iter()
                    .find(|e| e.seq == seq && e.eid == eid)
                    .or_else(|| known.get(eid).filter(|e| e.seq == seq).cloned()))
            });
        self.sync()?;
        result.map_err(outbox_error)
    }

    /// The daemon's sole slow-layer publication path.
    pub fn publish(
        &mut self,
        request: &WriteRequest,
        mut clock: impl FnMut() -> u64,
    ) -> Result<WriteOutcome, RuntimeError> {
        self.tick(&mut clock)?;
        if request.epoch != self.main.epoch
            || request.docs_dir != self.config.docs_dir
            || !matches!(request.kind, WriteKind::Slow { .. })
        {
            return Err(RuntimeError::Invalid(
                "慢层写入不属于当前主干租约或目录".into(),
            ));
        }
        main_writer::write(&self.config.root, request, || {
            self.check_fence(clock()).map_err(|e| e.to_string())
        })
        .map_err(RuntimeError::Uncertain)
    }

    pub fn snapshot(&mut self, mut clock: impl FnMut() -> u64) -> Result<String, RuntimeError> {
        self.tick(&mut clock)?;
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| self.dirty_seq <= snapshot.state.seq)
        {
            return self
                .snapshot_commit
                .clone()
                .ok_or_else(|| RuntimeError::NeedsRecovery("快照原件缺失".into()));
        }
        let prior = self.snapshot.as_ref().map_or(0, |s| s.state.seq);
        let next = Snapshot::build(
            &self.config.project_id,
            self.snapshot.as_ref(),
            &self.hub.events(prior),
            self.hub.contiguous_seq(),
            self.main.epoch,
            self.hub.epoch_high_water(),
            |events| replay(&self.initial, events, self.reduce),
        )
        .map_err(RuntimeError::NeedsRecovery)?;
        if next.state.projection != self.projection {
            return Err(RuntimeError::NeedsRecovery(
                "增量投影与快照重放不一致".into(),
            ));
        }
        let request = snapshot_request(&self.config, &next)?;
        let outcome = main_writer::write(&self.config.root, &request, || {
            self.check_fence(clock()).map_err(|e| e.to_string())
        })
        .map_err(RuntimeError::Uncertain)?;
        let commit = outcome_commit(outcome);
        let actual = Snapshot::read(
            &self.config.root,
            &commit,
            &self.config.docs_dir,
            &self.config.project_id,
            |events| replay(&self.initial, events, self.reduce),
        )
        .map_err(RuntimeError::NeedsRecovery)?;
        if actual.state != next.state || actual.events != next.events {
            return Err(RuntimeError::Fenced("其他写入者已出版不同快照".into()));
        }
        self.check_fence(clock())?;
        HubProtocol::snapshot_ack(
            &mut self.hub,
            SnapshotAck {
                commit: commit.clone(),
                seq: next.state.seq,
                epoch: next.state.epoch,
            },
            checkpoint(&next)?,
        )
        .map_err(hub_error)?;
        self.snapshot = Some(next);
        self.snapshot_commit = Some(commit.clone());
        self.snapshot_at = clock();
        Ok(commit)
    }

    fn check_fence(&self, now: u64) -> Result<(), RuntimeError> {
        if self
            .hub
            .leases(now)
            .get("main")
            .is_none_or(|lease| lease.fence() != self.main.fence())
        {
            return Err(RuntimeError::Fenced("主干租约已失效；停止写入".into()));
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<(), RuntimeError> {
        for event in self.hub.events(self.applied_seq) {
            if self.applied_seq.checked_add(1) != Some(event.seq) {
                return Err(RuntimeError::NeedsRecovery("增量事件存在序号缺口".into()));
            }
            apply(
                &mut self.projection,
                &mut self.canonical,
                &event,
                self.reduce,
            )
            .map_err(RuntimeError::NeedsRecovery)?;
            self.applied_seq = event.seq;
            if !event.is_pure_heartbeat() {
                self.dirty_seq = event.seq;
            }
        }
        Ok(())
    }

    fn poll(&mut self, now: u64) -> Result<(), RuntimeError> {
        self.hub
            .subscribe(self.applied_seq, now)
            .map_err(hub_error)?;
        self.sync()
    }

    /// Explicit App recovery only. A private candidate Hub is never exposed
    /// until its checked snapshot and skip boundaries are durable in Git.
    pub fn recover(
        config: RuntimeConfig,
        initial: Value,
        reduce: Reducer,
        mut clock: impl FnMut() -> u64,
        mut main_proof: impl FnMut(RecoveryEvidence<'_>) -> Result<Vec<NewEvent>, String>,
    ) -> Result<Self, RuntimeError> {
        Self::recover_inner(config, initial, reduce, None, &mut clock, &mut main_proof)
    }

    pub fn recover_confirmed(
        config: RuntimeConfig,
        initial: Value,
        reduce: Reducer,
        confirmation: NewEvent,
        mut clock: impl FnMut() -> u64,
        mut main_proof: impl FnMut(RecoveryEvidence<'_>) -> Result<Vec<NewEvent>, String>,
    ) -> Result<Self, RuntimeError> {
        validate_confirmation(&config, &confirmation)?;
        Self::recover_inner(
            config,
            initial,
            reduce,
            Some(confirmation),
            &mut clock,
            &mut main_proof,
        )
    }

    fn recover_inner(
        config: RuntimeConfig,
        initial: Value,
        reduce: Reducer,
        confirmation: Option<NewEvent>,
        clock: &mut impl FnMut() -> u64,
        main_proof: &mut impl FnMut(RecoveryEvidence<'_>) -> Result<Vec<NewEvent>, String>,
    ) -> Result<Self, RuntimeError> {
        validate_config(&config)?;
        refresh(&config.root)?;
        let directory = config.common_dir.join("towermd");
        let outbox = DurableOutbox::open(&config.common_dir).map_err(outbox_error)?;
        let marker_path = directory.join("runtime.json");
        let marker = read_marker(&marker_path, &config)?.unwrap_or(Marker {
            version: 1,
            project_id: config.project_id.clone(),
            host: config.host.clone(),
            holder: format!("daemon-{}", uuid::Uuid::now_v7()),
        });
        let old_hub = directory.join("hub");
        let old_lock = lock_existing_hub(&old_hub)?;
        let previous = read_latest(&config, &initial, reduce)?
            .map(|(_, s)| s)
            .unwrap_or(
                Snapshot::build(&config.project_id, None, &[], 0, 0, 0, |events| {
                    replay(&initial, events, reduce)
                })
                .map_err(RuntimeError::Invalid)?,
            );
        if let Some(request) = &confirmation {
            if let Some(original) = canonical_events(&previous.events)
                .map_err(RuntimeError::NeedsRecovery)?
                .get(&request.eid)
            {
                if !matches_input(original, request) || original.stale {
                    return Err(RuntimeError::Invalid("恢复请求 ID 已绑定不同原件".into()));
                }
                drop(old_lock);
                drop(outbox);
                return Self::resume_confirmed(config, initial, reduce, request, clock);
            }
        }
        let confirmed = outbox.confirmed().map_err(outbox_error)?;
        let pending = outbox.pending().map_err(outbox_error)?;
        let trailer_events = main_proof(RecoveryEvidence {
            root: &config.root,
            snapshot: &previous,
            confirmed: &confirmed,
            pending: &pending,
        })
        .map_err(RuntimeError::NeedsRecovery)?;
        let mut canonical =
            canonical_events(&previous.events).map_err(RuntimeError::NeedsRecovery)?;
        let mut additions = Vec::new();
        let mut source_seq = previous.state.seq;
        let mut source_epoch = previous.state.epoch_high_water;
        let mut receipts = previous.receipts();
        for original in &confirmed {
            source_seq = source_seq.max(original.seq);
            source_epoch = source_epoch.max(original.epoch.unwrap_or(0));
            for guard in &original.guards {
                source_epoch = source_epoch.max(guard.epoch);
            }
            match canonical.get(&original.eid) {
                Some(old) if old != original => {
                    return Err(RuntimeError::NeedsRecovery(
                        "发件箱与归档中的原件冲突".into(),
                    ))
                }
                Some(_) => {}
                None => {
                    additions.push(original.clone());
                    canonical.insert(original.eid.clone(), original.clone());
                }
            }
            if receipts
                .insert(original.eid.clone(), original.seq)
                .is_some_and(|seq| seq != original.seq)
            {
                return Err(RuntimeError::NeedsRecovery("恢复回执索引冲突".into()));
            }
        }
        let seq_boundary = source_seq
            .checked_add(1 << 32)
            .ok_or_else(|| RuntimeError::Invalid("恢复序号溢出".into()))?;
        let epoch_boundary = source_epoch
            .checked_add(1 << 32)
            .ok_or_else(|| RuntimeError::Invalid("恢复 epoch 溢出".into()))?;
        let boundary = previous
            .recovery_boundary(seq_boundary, epoch_boundary, |events| {
                replay(&initial, events, reduce)
            })
            .map_err(RuntimeError::NeedsRecovery)?;
        let candidate_path = directory.join(format!("recovery-{}", uuid::Uuid::now_v7()));
        let mut candidate = Hub::open(&candidate_path).map_err(RuntimeError::Uncertain)?;
        candidate
            .rebuild(RebuildSeed {
                source_seq,
                source_epoch,
                seq_boundary,
                epoch_boundary,
                checkpoint: checkpoint(&boundary)?,
                receipts,
                tail: vec![],
                final_snapshot: None,
            })
            .map_err(hub_error)?;
        let lease = candidate
            .lease_acquire(
                AcquireLease {
                    host: marker.host.clone(),
                    names: vec!["main".into()],
                    holder: marker.holder.clone(),
                    ttl_ms: MAIN_TTL_MS,
                },
                clock(),
            )
            .map_err(hub_error)?
            .leases
            .remove(0);
        additions.sort_by_key(|event| event.seq);
        for original in additions {
            let input = NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: marker.host.clone(),
                actor: "process:towermd-recovery".into(),
                kind: "recovery.confirmed".into(),
                layer: "L5".into(),
                obj: original.obj.clone(),
                data: serde_json::json!({"original": original}),
                guards: vec![lease.fence()],
            };
            append_candidate(&mut candidate, input, clock())?;
        }
        for mut input in trailer_events {
            if !input.eid.starts_with("git:")
                || input.actor != "process:towermd-recovery"
                || matches!(
                    input.kind.as_str(),
                    "recovery.confirmed" | "recovery.completed" | "hub.rebuild.confirmed"
                )
                || input.kind.starts_with("lease.")
            {
                return Err(RuntimeError::Invalid(
                    "main 恢复证明必须生成确定的 git 事件".into(),
                ));
            }
            input.guards = vec![lease.fence()];
            append_candidate(&mut candidate, input, clock())?;
        }
        if let Some(confirmation) = confirmation {
            append_candidate(&mut candidate, confirmation, clock())?;
        }
        append_candidate(
            &mut candidate,
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: marker.host.clone(),
                actor: "process:towermd-recovery".into(),
                kind: "recovery.completed".into(),
                layer: "L5".into(),
                obj: config.project_id.clone(),
                data: serde_json::to_value(boundary.state.recovery_jumps.last().unwrap())
                    .map_err(|e| RuntimeError::Invalid(e.to_string()))?,
                guards: vec![lease.fence()],
            },
            clock(),
        )?;
        let next = Snapshot::build(
            &config.project_id,
            Some(&boundary),
            &candidate.events(seq_boundary),
            candidate.contiguous_seq(),
            lease.epoch,
            candidate.epoch_high_water(),
            |events| replay(&initial, events, reduce),
        )
        .map_err(RuntimeError::NeedsRecovery)?;
        let request = snapshot_request(&config, &next)?;
        let outcome = main_writer::write(&config.root, &request, || {
            if candidate
                .leases(clock())
                .get("main")
                .is_none_or(|current| current.fence() != lease.fence())
            {
                Err("恢复候选的主干租约已失效".into())
            } else {
                Ok(())
            }
        })
        .map_err(RuntimeError::Uncertain)?;
        let commit = outcome_commit(outcome);
        let actual = Snapshot::read(
            &config.root,
            &commit,
            &config.docs_dir,
            &config.project_id,
            |events| replay(&initial, events, reduce),
        )
        .map_err(RuntimeError::NeedsRecovery)?;
        if actual.state != next.state || actual.events != next.events {
            return Err(RuntimeError::Fenced(
                "恢复边界被其他更高快照替代；需从最新边界重试".into(),
            ));
        }
        HubProtocol::snapshot_ack(
            &mut candidate,
            SnapshotAck {
                commit,
                seq: next.state.seq,
                epoch: next.state.epoch,
            },
            checkpoint(&next)?,
        )
        .map_err(hub_error)?;
        drop(candidate);
        if !marker_path.exists() {
            save_marker(&marker_path, &marker)?;
        }
        install_candidate(&candidate_path, &old_hub, &directory)?;
        drop(old_lock);
        drop(outbox);
        let mut runtime = Self::open(config, initial, reduce, &mut *clock)?;
        runtime.replay_pending(pending, clock)?;
        runtime.snapshot(&mut *clock)?;
        Ok(runtime)
    }

    fn resume_confirmed(
        config: RuntimeConfig,
        initial: Value,
        reduce: Reducer,
        confirmation: &NewEvent,
        clock: &mut impl FnMut() -> u64,
    ) -> Result<Self, RuntimeError> {
        match Self::open(config.clone(), initial.clone(), reduce, &mut *clock) {
            Ok(runtime) => return Ok(runtime),
            Err(RuntimeError::NeedsRecovery(_)) => {}
            Err(error) => return Err(error),
        }
        let directory = config.common_dir.join("towermd");
        let old_hub = directory.join("hub");
        let outbox = DurableOutbox::open(&config.common_dir).map_err(outbox_error)?;
        let old_lock = lock_existing_hub(&old_hub)?;
        let (commit, snapshot) = read_latest(&config, &initial, reduce)?
            .ok_or_else(|| RuntimeError::NeedsRecovery("已确认恢复的 Git 快照缺失".into()))?;
        let known = canonical_events(&snapshot.events).map_err(RuntimeError::NeedsRecovery)?;
        if known
            .get(&confirmation.eid)
            .is_none_or(|event| !matches_input(event, confirmation) || event.stale)
        {
            return Err(RuntimeError::NeedsRecovery(
                "最新 Git 快照缺少原恢复确认".into(),
            ));
        }
        let pending = outbox.pending().map_err(outbox_error)?;
        let marker_path = directory.join("runtime.json");
        for entry in fs::read_dir(&directory).map_err(|e| RuntimeError::Uncertain(e.to_string()))? {
            let entry = entry.map_err(|e| RuntimeError::Uncertain(e.to_string()))?;
            if !entry.file_name().to_string_lossy().starts_with("recovery-")
                || !entry
                    .file_type()
                    .map_err(|e| RuntimeError::Uncertain(e.to_string()))?
                    .is_dir()
            {
                continue;
            }
            let mut candidate = match Hub::open_existing(&entry.path()) {
                Ok(hub) => hub,
                Err(_) => continue,
            };
            if candidate.last_seq() != snapshot.state.seq
                || candidate.epoch_high_water() != snapshot.state.epoch_high_water
            {
                continue;
            }
            let retained = candidate.events(0);
            if retained.iter().any(|event| {
                if event.is_pure_heartbeat() {
                    !snapshot
                        .state
                        .omitted_heartbeats
                        .iter()
                        .any(|range| range.first <= event.seq && event.seq <= range.last)
                } else {
                    !snapshot.events.iter().any(|archived| archived == event)
                }
            }) {
                continue;
            }
            if HubProtocol::snapshot_ack(
                &mut candidate,
                SnapshotAck {
                    commit: commit.clone(),
                    seq: snapshot.state.seq,
                    epoch: snapshot.state.epoch,
                },
                checkpoint(&snapshot)?,
            )
            .is_err()
            {
                continue;
            }
            let lease = match candidate.leases(0).get("main").cloned() {
                Some(lease) => lease,
                None => continue,
            };
            let marker = read_marker(&marker_path, &config)?.unwrap_or(Marker {
                version: 1,
                project_id: config.project_id.clone(),
                host: config.host.clone(),
                holder: lease.holder.clone(),
            });
            if marker.holder != lease.holder {
                continue;
            }
            drop(candidate);
            if !marker_path.exists() {
                save_marker(&marker_path, &marker)?;
            }
            install_candidate(&entry.path(), &old_hub, &directory)?;
            drop(old_lock);
            drop(outbox);
            let mut runtime = Self::open(config, initial, reduce, &mut *clock)?;
            runtime.replay_pending(pending, clock)?;
            runtime.snapshot(&mut *clock)?;
            return Ok(runtime);
        }
        Err(RuntimeError::NeedsRecovery(
            "原恢复已在 Git 确认，但 Hub 与对应恢复候选均不可用；需新的人工恢复请求，不能复用旧批准再次跳号".into(),
        ))
    }

    fn replay_pending(
        &mut self,
        pending: Vec<NewEvent>,
        clock: &mut impl FnMut() -> u64,
    ) -> Result<(), RuntimeError> {
        // Pending intent remains untrusted until the actual hub appends it.
        // Old main/module guards naturally produce a retained stale original.
        for input in pending {
            let now = clock();
            let trial = trial_event(&self.hub, &input, now)?;
            if !trial.stale {
                (self.reduce)(&mut self.projection.clone(), &trial)
                    .map_err(RuntimeError::Invalid)?;
            }
            let known = &self.canonical;
            let result = self
                .outbox
                .deliver(&mut self.hub, input, now, |hub, seq, eid| {
                    Ok(hub
                        .events(seq.saturating_sub(1))
                        .into_iter()
                        .find(|e| e.seq == seq && e.eid == eid)
                        .or_else(|| known.get(eid).filter(|e| e.seq == seq).cloned()))
                });
            self.sync()?;
            match result {
                Ok(_) | Err(OutboxError::Stale(_)) => {}
                Err(error) => return Err(outbox_error(error)),
            }
        }
        Ok(())
    }
}

pub fn canonical_events(events: &[Event]) -> Result<BTreeMap<String, Event>, String> {
    let mut result = BTreeMap::new();
    for event in events {
        let original = recovered_original(event)?.unwrap_or_else(|| event.clone());
        if let Some(old) = result.insert(original.eid.clone(), original.clone()) {
            if old != original {
                return Err("同一 eid 的 canonical 原件冲突".into());
            }
        }
    }
    Ok(result)
}

pub fn replay(initial: &Value, events: &[Event], reduce: Reducer) -> Result<Value, String> {
    let mut value = initial.clone();
    let mut seen = BTreeMap::new();
    for event in events {
        apply(&mut value, &mut seen, event, reduce)?;
    }
    Ok(value)
}

fn apply(
    value: &mut Value,
    seen: &mut BTreeMap<String, Event>,
    event: &Event,
    reduce: Reducer,
) -> Result<(), String> {
    let original = recovered_original(event)?.unwrap_or_else(|| event.clone());
    if let Some(old) = seen.get(&original.eid) {
        if old != &original {
            return Err("同一 eid 的 canonical 原件冲突".into());
        }
        return Ok(());
    }
    if !original.stale
        && (!original.kind.starts_with("lease.")
            || matches!(original.kind.as_str(), "lease.expired" | "lease.released"))
    {
        let mut next = value.clone();
        reduce(&mut next, &original)?;
        *value = next;
    }
    seen.insert(original.eid.clone(), original);
    Ok(())
}

fn append_candidate(hub: &mut Hub, input: NewEvent, now: u64) -> Result<(), RuntimeError> {
    match hub.append(input, now).map_err(hub_error)? {
        AppendReceipt::Event(event) if !event.stale => Ok(()),
        _ => Err(RuntimeError::NeedsRecovery(
            "恢复候选事件未获得有效完整回执".into(),
        )),
    }
}

fn matches_input(event: &Event, input: &NewEvent) -> bool {
    event.eid == input.eid
        && event.host == input.host
        && event.actor == input.actor
        && event.kind == input.kind
        && event.layer == input.layer
        && event.obj == input.obj
        && event.data == input.data
        && event.guards == input.guards
}

fn module_names<'a>(names: impl Iterator<Item = &'a str>) -> Result<(), RuntimeError> {
    let names: Vec<_> = names.collect();
    if names.is_empty()
        || names
            .iter()
            .any(|name| name.strip_prefix("module:").is_none_or(str::is_empty))
    {
        return Err(RuntimeError::Invalid(
            "模块租约只接受非空 module:<id> 名称".into(),
        ));
    }
    Ok(())
}

fn module_ttl(ttl: u64) -> Result<(), RuntimeError> {
    if ttl == 0 || ttl > MODULE_TTL_MS {
        return Err(RuntimeError::Invalid(
            "模块租约 TTL 必须在 1–900000 毫秒内".into(),
        ));
    }
    Ok(())
}

fn validate_confirmation(config: &RuntimeConfig, input: &NewEvent) -> Result<(), RuntimeError> {
    crate::hub::validate_input(input).map_err(hub_error)?;
    if input.actor != "human:local"
        || input.kind != "hub.rebuild.confirmed"
        || input.layer != "L5"
        || input.obj != config.project_id
        || input.host != config.host
        || !input.guards.is_empty()
        || input.eid.starts_with("git:")
        || input.data != serde_json::json!({"request_id":input.eid})
    {
        return Err(RuntimeError::Invalid(
            "恢复确认必须是 App 的完整 human:local 请求原件".into(),
        ));
    }
    Ok(())
}

fn install_candidate(
    candidate: &Path,
    old_hub: &Path,
    directory: &Path,
) -> Result<(), RuntimeError> {
    if old_hub.exists() {
        fs::rename(
            old_hub,
            directory.join(format!("hub-before-recovery-{}", uuid::Uuid::now_v7())),
        )
        .map_err(|e| RuntimeError::Uncertain(e.to_string()))?;
        sync_dir(directory)?;
    }
    fs::rename(candidate, old_hub).map_err(|e| RuntimeError::Uncertain(e.to_string()))?;
    sync_dir(directory)
}

fn trial_event(hub: &Hub, input: &NewEvent, now: u64) -> Result<Event, RuntimeError> {
    let time = i64::try_from(now)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .ok_or_else(|| RuntimeError::Invalid("事件时间无效".into()))?;
    let leases = hub.leases(now);
    let stale = input.guards.iter().any(|guard| {
        leases
            .get(&guard.name)
            .is_none_or(|lease| lease.fence() != *guard)
    });
    serde_json::from_value(serde_json::json!({
        "seq": hub.last_seq().checked_add(1).ok_or_else(|| RuntimeError::Invalid("事件序号溢出".into()))?,
        "eid":input.eid, "host":input.host, "actor":input.actor, "type":input.kind,
        "layer":input.layer,"obj":input.obj,"data":input.data,"guards":input.guards,
        "epoch":input.guards.first().map(|guard|guard.epoch),"stale":stale,"cost_usd":null,
        "t":time.to_rfc3339_opts(chrono::SecondsFormat::Millis,true),
    })).map_err(|e|RuntimeError::Invalid(e.to_string()))
}

fn validate_config(config: &RuntimeConfig) -> Result<(), RuntimeError> {
    if config.project_id.trim().is_empty()
        || config.host.trim().is_empty()
        || crate::init::normalize_docs_dir(&config.docs_dir).map_err(RuntimeError::Invalid)?
            != config.docs_dir
        || !config.root.is_dir()
        || !config.common_dir.is_dir()
    {
        return Err(RuntimeError::Invalid("项目运行参数无效".into()));
    }
    Ok(())
}
fn refresh(root: &Path) -> Result<(), RuntimeError> {
    let remotes = repo::git_timeout(root, &["remote"], Duration::from_secs(30))
        .map_err(RuntimeError::Uncertain)?;
    if remotes.lines().any(|name| name == "origin") {
        repo::git_timeout(
            root,
            &[
                "fetch",
                "--no-tags",
                "origin",
                "+refs/heads/main:refs/remotes/origin/main",
            ],
            Duration::from_secs(30),
        )
        .map_err(RuntimeError::Uncertain)?;
    }
    Ok(())
}
fn read_latest(
    config: &RuntimeConfig,
    initial: &Value,
    reduce: Reducer,
) -> Result<Option<(String, Snapshot)>, RuntimeError> {
    main_writer::latest_snapshot(&config.root)
        .map_err(RuntimeError::NeedsRecovery)?
        .map(|latest| {
            let snapshot = Snapshot::read(
                &config.root,
                &latest.commit,
                &config.docs_dir,
                &config.project_id,
                |events| replay(initial, events, reduce),
            )
            .map_err(RuntimeError::NeedsRecovery)?;
            if snapshot.state.seq != latest.boundary.seq
                || snapshot.state.epoch != latest.boundary.epoch
            {
                return Err(RuntimeError::NeedsRecovery(
                    "快照 trailer 与正文边界不一致".into(),
                ));
            }
            Ok((latest.commit, snapshot))
        })
        .transpose()
}
fn snapshot_request(
    config: &RuntimeConfig,
    snapshot: &Snapshot,
) -> Result<WriteRequest, RuntimeError> {
    Ok(WriteRequest {
        docs_dir: config.docs_dir.clone(),
        epoch: snapshot.state.epoch,
        kind: WriteKind::Snapshot {
            seq: snapshot.state.seq,
            promote_from: None,
        },
        changes: snapshot
            .files(&config.docs_dir)
            .map_err(RuntimeError::Invalid)?,
        message: "Tower.md durable project snapshot".into(),
    })
}
fn checkpoint(snapshot: &Snapshot) -> Result<Value, RuntimeError> {
    serde_json::to_value(&snapshot.state).map_err(|e| RuntimeError::Invalid(e.to_string()))
}
fn outcome_commit(outcome: WriteOutcome) -> String {
    match outcome {
        WriteOutcome::Written { commit, .. } | WriteOutcome::Superseded { commit, .. } => commit,
    }
}
fn hub_error(error: HubError) -> RuntimeError {
    match error {
        HubError::FenceExpired { .. } | HubError::LeaseConflict { .. } => {
            RuntimeError::Fenced(error.to_string())
        }
        HubError::Invalid(s) => RuntimeError::Invalid(s),
        _ => RuntimeError::Uncertain(error.to_string()),
    }
}
fn outbox_error(error: OutboxError) -> RuntimeError {
    match error {
        OutboxError::Invalid(s) => RuntimeError::Invalid(s),
        OutboxError::Corrupt(s) => RuntimeError::NeedsRecovery(s),
        OutboxError::Stale(event) => {
            RuntimeError::Fenced(format!("事件 {} 的原 fence 已失效", event.eid))
        }
        OutboxError::Hub(error) => hub_error(error),
        OutboxError::Uncertain(s) => RuntimeError::Uncertain(s),
    }
}
fn read_marker(path: &Path, config: &RuntimeConfig) -> Result<Option<Marker>, RuntimeError> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
        _ => return Err(RuntimeError::NeedsRecovery("运行标记必须是普通文件".into())),
    }
    let marker: Marker = serde_json::from_slice(
        &fs::read(path).map_err(|e| RuntimeError::Uncertain(e.to_string()))?,
    )
    .map_err(|e| RuntimeError::NeedsRecovery(e.to_string()))?;
    if marker.version != 1
        || marker.project_id != config.project_id
        || marker.host != config.host
        || marker.holder.is_empty()
    {
        return Err(RuntimeError::NeedsRecovery(
            "运行标记与项目身份不一致".into(),
        ));
    }
    Ok(Some(marker))
}
fn save_marker(path: &Path, marker: &Marker) -> Result<(), RuntimeError> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::now_v7()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| RuntimeError::Uncertain(e.to_string()))?;
    file.write_all(&serde_json::to_vec(marker).map_err(|e| RuntimeError::Invalid(e.to_string()))?)
        .and_then(|_| file.sync_all())
        .map_err(|e| RuntimeError::Uncertain(e.to_string()))?;
    fs::hard_link(&temporary, path).map_err(|e| RuntimeError::Uncertain(e.to_string()))?;
    sync_dir(path.parent().unwrap())?;
    fs::remove_file(temporary).map_err(|e| RuntimeError::Uncertain(e.to_string()))?;
    sync_dir(path.parent().unwrap())
}
fn sync_dir(path: &Path) -> Result<(), RuntimeError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| RuntimeError::Uncertain(e.to_string()))
}
fn lock_existing_hub(path: &Path) -> Result<Option<File>, RuntimeError> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        _ => return Err(RuntimeError::NeedsRecovery("Hub 路径不是普通目录".into())),
    }
    let lock_path = path.join("hub.lock");
    if fs::symlink_metadata(&lock_path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err(RuntimeError::NeedsRecovery("Hub 锁路径不是普通文件".into()));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| RuntimeError::Uncertain(e.to_string()))?;
    file.try_lock_exclusive()
        .map_err(|_| RuntimeError::Fenced("Hub 仍被另一个运行实例持有".into()))?;
    Ok(Some(file))
}
