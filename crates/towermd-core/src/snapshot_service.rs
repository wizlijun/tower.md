use std::collections::BTreeMap;
use std::sync::{Mutex, TryLockError};

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::catalog::Catalog;
use crate::hub::{derived_eid, Event, Fence, Hub, LeaseChange, NewEvent, SnapshotAck};
use crate::repo;
use crate::snapshot::{archive_events, Snapshot};
use crate::snapshot_git::{self, WriteOutcome, WriteRequest};

const TTL: u64 = 120_000;

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotReport {
    pub status: String,
    pub commit: Option<String>,
    pub seq: u64,
    pub epoch: u64,
    pub reason: Option<String>,
}

#[derive(Default)]
pub struct SnapshotWriter {
    writing: Mutex<()>,
}

impl SnapshotWriter {
    pub fn rebuild(
        &self,
        hub: &Mutex<Hub>,
        catalog: &Catalog,
        request: &crate::recovery::RecoveryRequest,
        confirmation: &crate::recovery::AppConfirmation,
        host: &str,
        now: impl FnMut() -> Result<u64, String>,
    ) -> Result<SnapshotReport, String> {
        let _guard = self.writing.try_lock().map_err(|_| "快照写入忙或锁损坏")?;
        crate::recovery::execute(hub, catalog, request, confirmation, host, now)
    }

    pub fn write(
        &self,
        hub: &Mutex<Hub>,
        catalog: &Catalog,
        host: &str,
        force: bool,
        mut now: impl FnMut() -> Result<u64, String>,
    ) -> Result<SnapshotReport, String> {
        let _guard = match self.writing.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => {
                return Ok(report(
                    "deferred",
                    None,
                    0,
                    0,
                    Some("快照写入正在进行".into()),
                ))
            }
            Err(TryLockError::Poisoned(_)) => return Err("快照写入锁损坏".into()),
        };
        let interval = interval_ms(catalog)?;
        let previous = read_snapshot(catalog)?;
        let (checkpoint, events, fence) = {
            let mut hub = hub.lock().map_err(|_| "hub 锁损坏")?;
            let time = now()?;
            validate_saved(&hub, catalog, previous.as_ref())?;
            if let Some(saved) = &previous {
                record_written(&mut hub, host, &saved.commit, &saved.snapshot, time)?;
            }
            crate::tasks::expire(&mut hub, host, time)?;
            if !force && !due(&hub, interval, time)? {
                return Ok(report("unchanged", None, hub.last_seq(), 0, None));
            }
            let fence = acquire_main(&mut hub, host, time)?;
            (
                hub.checkpoint(catalog.config.project_id(), &fence, time)?,
                hub.events(0),
                fence,
            )
        };
        let after = previous
            .as_ref()
            .map_or(0, |saved| saved.snapshot.through_seq);
        let mut archives = previous.map_or_else(BTreeMap::new, |saved| saved.archives);
        let mut changed = BTreeMap::new();
        for event in archive_events(&events, after, checkpoint.through_seq)? {
            let month = chrono::DateTime::parse_from_rfc3339(&event.t)
                .map_err(|error| format!("事件时间非法：{error}"))?
                .format("%Y-%m")
                .to_string();
            let content = archives.entry(month.clone()).or_insert_with(String::new);
            content.push_str(&serde_json::to_string(&event).map_err(|error| error.to_string())?);
            content.push('\n');
            changed.insert(month, content.clone());
        }
        let request = WriteRequest {
            base: catalog.revision.clone(),
            docs_dir: catalog.config.docs_dir().into(),
            state_json: format!(
                "{}\n",
                serde_json::to_string_pretty(&checkpoint).map_err(|error| error.to_string())?
            ),
            archives: changed,
            through_seq: checkpoint.through_seq,
            writer_epoch: fence.epoch,
        };
        let result = snapshot_git::write_snapshot(&catalog.root, &request, || {
            let mut hub = hub.lock().map_err(|_| "hub 锁损坏")?;
            renew_fence(&mut hub, host, &fence, now()?)
        })?;
        match result {
            WriteOutcome::Written { commit } => {
                let result = record_written(
                    &mut *hub.lock().map_err(|_| "hub 锁损坏")?,
                    host,
                    &commit,
                    &checkpoint,
                    now()?,
                );
                result.map_err(|error| {
                    format!(
                        "Git 快照 {commit} 已写入，但 hub 确认失败；下次从 main 补确认：{error}"
                    )
                })?;
                Ok(report(
                    "written",
                    Some(commit),
                    checkpoint.through_seq,
                    fence.epoch,
                    None,
                ))
            }
            WriteOutcome::Deferred { reason } => Ok(report(
                "deferred",
                None,
                checkpoint.through_seq,
                fence.epoch,
                Some(reason),
            )),
            WriteOutcome::Superseded { commit, epoch, seq } => Ok(report(
                "superseded",
                Some(commit),
                seq,
                epoch,
                Some("main 已有较新快照；重新加载后再操作".into()),
            )),
        }
    }
}

pub fn interval_ms(catalog: &Catalog) -> Result<u64, String> {
    let config: serde_yaml::Value =
        serde_yaml::from_str(&catalog.config.to_yaml()).map_err(|error| error.to_string())?;
    let seconds = match config.get("snapshot_interval") {
        None => 1800,
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0)
            .ok_or("snapshot_interval 必须为正整数秒")?,
    };
    seconds
        .checked_mul(1000)
        .ok_or_else(|| "snapshot_interval 溢出".into())
}

pub fn due(hub: &Hub, interval: u64, now: u64) -> Result<bool, String> {
    let after = hub.snapshot_ack().map_or(0, |ack| ack.seq);
    let events = hub.events(0);
    let first = events.iter().find(|event| {
        event.seq > after
            && !event.stale
            && !event.is_pure_heartbeat()
            && event.kind != "snapshot.written"
            && !(event.obj == "main" && event.kind.starts_with("lease."))
    });
    let Some(first) = first else { return Ok(false) };
    let start = events
        .iter()
        .rev()
        .find(|event| event.kind == "snapshot.written" && !event.stale)
        .unwrap_or(first);
    let millis = chrono::DateTime::parse_from_rfc3339(&start.t)
        .map_err(|error| error.to_string())?
        .timestamp_millis();
    let millis = u64::try_from(millis).map_err(|_| "事件时间不能早于 Unix epoch")?;
    Ok(now.saturating_sub(millis) >= interval)
}

pub fn maintain_main(hub: &mut Hub, host: &str, now: u64) -> Result<(), String> {
    if let Some(lease) = hub
        .leases(now)
        .get("main")
        .filter(|lease| lease.holder == format!("snapshot:{host}"))
    {
        renew_fence(hub, host, &lease.fence(), now)?;
    }
    Ok(())
}

pub(crate) fn acquire_main(hub: &mut Hub, host: &str, now: u64) -> Result<Fence, String> {
    let holder = format!("snapshot:{host}");
    if let Some(lease) = hub.leases(now).get("main") {
        if lease.holder != holder {
            return Err("main 租约由其他持有者占用".into());
        }
        let fence = lease.fence();
        renew_fence(hub, host, &fence, now)?;
        return Ok(fence);
    }
    let event = hub.append(
        new_event(host, "lease.acquired", "main"),
        Some(LeaseChange::Acquire {
            names: vec!["main".into()],
            holder,
            ttl_ms: TTL,
        }),
        now,
    )?;
    Ok(event
        .updated_leases()
        .first()
        .ok_or("main 租约未取得")?
        .fence())
}

pub(crate) fn renew_fence(
    hub: &mut Hub,
    host: &str,
    fence: &Fence,
    now: u64,
) -> Result<(), String> {
    let leases = hub.leases(now);
    let lease = leases
        .get("main")
        .filter(|lease| lease.fence() == *fence)
        .ok_or("main 租约已失效，拒绝写入")?;
    if lease.expires_at.saturating_sub(now) <= TTL - 30_000 {
        let mut event = new_event(host, "lease.renewed", "main");
        event.fence = Some(fence.clone());
        hub.append(
            event,
            Some(LeaseChange::Renew {
                fences: vec![fence.clone()],
                ttl_ms: TTL,
            }),
            now,
        )?;
    }
    Ok(())
}

fn new_event(host: &str, kind: &str, obj: &str) -> NewEvent {
    NewEvent {
        eid: Uuid::now_v7().to_string(),
        host: host.into(),
        actor: "process:towermd-snapshot".into(),
        kind: kind.into(),
        layer: "L3".into(),
        obj: obj.into(),
        data: json!({"lease":"main"}),
        fence: None,
    }
}

pub(crate) fn record_written(
    hub: &mut Hub,
    host: &str,
    commit: &str,
    snapshot: &Snapshot,
    now: u64,
) -> Result<(), String> {
    let mut event = new_event(host, "snapshot.written", "main");
    event.eid = derived_eid(commit, "snapshot.written")?;
    event.data = json!({"commit":commit,"seq":snapshot.through_seq,"epoch":snapshot.writer_epoch});
    hub.append_receipt(event, None, now)?;
    hub.acknowledge(SnapshotAck {
        commit: commit.into(),
        seq: snapshot.through_seq,
        epoch: snapshot.writer_epoch,
    })
}

fn report(
    status: &str,
    commit: Option<String>,
    seq: u64,
    epoch: u64,
    reason: Option<String>,
) -> SnapshotReport {
    SnapshotReport {
        status: status.into(),
        commit,
        seq,
        epoch,
        reason,
    }
}

pub struct SavedSnapshot {
    pub snapshot: Snapshot,
    pub commit: String,
    pub archives: BTreeMap<String, String>,
}

pub fn validate_authority(hub: &Hub, catalog: &Catalog) -> Result<(), String> {
    validate_saved(hub, catalog, read_snapshot(catalog)?.as_ref())
}

fn validate_saved(
    hub: &Hub,
    catalog: &Catalog,
    saved: Option<&SavedSnapshot>,
) -> Result<(), String> {
    if let Some(saved) = saved {
        let expected = match hub.base_snapshot() {
            Some(base) => Snapshot::extend(
                Some(base),
                &hub.events(0),
                saved.snapshot.through_seq,
                saved.snapshot.writer_epoch,
            ),
            None => Snapshot::from_events(
                catalog.config.project_id(),
                &hub.events(0),
                saved.snapshot.through_seq,
                saved.snapshot.writer_epoch,
            ),
        }
        .map_err(|error| format!("hub 与 main 快照不一致，需要 App 确认恢复：{error}"))?;
        if expected != saved.snapshot {
            return Err("hub 历史与 main 快照不一致，需要 App 确认恢复".into());
        }
        if hub.snapshot_ack().is_some_and(|ack| {
            ack.seq > saved.snapshot.through_seq
                || (ack.seq == saved.snapshot.through_seq && ack.commit != saved.commit)
        }) {
            return Err("main 快照回退或同水位提交冲突，拒绝覆盖".into());
        }
    } else if hub.snapshot_ack().is_some() {
        return Err("main 缺少已确认的快照，拒绝覆盖".into());
    }
    Ok(())
}

pub fn read_snapshot(catalog: &Catalog) -> Result<Option<SavedSnapshot>, String> {
    let prefix = format!("{}/state/", catalog.config.docs_dir());
    let state_path = format!("{prefix}state.json");
    let entries = repo::git(&catalog.root, &["ls-tree", "-r", "-z", &catalog.revision])?;
    let mut snapshot = None;
    let mut archives = BTreeMap::new();
    let mut events = Vec::new();
    for entry in entries.split('\0').filter(|entry| !entry.is_empty()) {
        let (metadata, path) = entry.split_once('\t').ok_or("Git tree 格式非法")?;
        if !path.starts_with(&prefix) {
            continue;
        }
        let fields: Vec<_> = metadata.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != "100644" || fields[1] != "blob" {
            return Err("快照路径必须是普通非执行文件".into());
        }
        let text = repo::git(&catalog.root, &["cat-file", "blob", fields[2]])?;
        if path == state_path {
            snapshot = Some(
                serde_json::from_str::<Snapshot>(&text)
                    .map_err(|error| format!("快照非法：{error}"))?,
            );
        } else if let Some(month) = path
            .strip_prefix(&format!("{prefix}events/"))
            .and_then(|name| name.strip_suffix(".jsonl"))
        {
            if month.len() != 7
                || month.as_bytes()[4] != b'-'
                || !month
                    .bytes()
                    .enumerate()
                    .all(|(index, byte)| index == 4 || byte.is_ascii_digit())
                || !("01"..="12").contains(&&month[5..])
            {
                return Err("月归档路径非法".into());
            }
            for line in text.lines() {
                let event: Event =
                    serde_json::from_str(line).map_err(|error| format!("月归档非法：{error}"))?;
                if !event.t.starts_with(month) {
                    return Err("月归档包含其他月份事件".into());
                }
                events.push(event);
            }
            archives.insert(
                month.into(),
                if text.is_empty() {
                    text
                } else {
                    format!("{}\n", text.trim_end_matches('\n'))
                },
            );
        } else {
            return Err(format!("未知快照路径：{path}"));
        }
    }
    let Some(snapshot) = snapshot else {
        return if archives.is_empty() {
            Ok(None)
        } else {
            Err("存在归档但缺少 state.json".into())
        };
    };
    events.sort_by_key(|event| event.seq);
    snapshot.validate_archive(catalog.config.project_id(), &events)?;
    let commit = repo::git(
        &catalog.root,
        &[
            "log",
            "-1",
            "--format=%H",
            &catalog.revision,
            "--",
            &format!(":(literal){state_path}"),
        ],
    )?;
    let trailer =
        snapshot_git::read_trailer(&catalog.root, &commit)?.ok_or("快照缺少提交 trailer")?;
    if trailer.seq != snapshot.through_seq || trailer.epoch != snapshot.writer_epoch {
        return Err("快照与提交 trailer 不一致".into());
    }
    Ok(Some(SavedSnapshot {
        snapshot,
        commit,
        archives,
    }))
}
