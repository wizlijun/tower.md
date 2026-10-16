use std::collections::BTreeMap;
use std::sync::{Mutex, TryLockError};

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::catalog::Catalog;
use crate::events::derived_eid;
use crate::hub_protocol::{AcquireLease, Event, Fence, NewEvent, RenewLease, SnapshotAck};
use crate::project_session::ProjectSession;
use crate::repo;
use crate::snapshot::{archive_events, Snapshot};
use crate::snapshot_git::{self, WriteIntent, WriteOutcome, WriteRequest};

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
        hub: &Mutex<ProjectSession>,
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
        hub: &Mutex<ProjectSession>,
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
        validate_authority_shared(hub, catalog)?;
        let main_writable = snapshot_git::main_writable(&catalog.root)?;

        let private_revision = snapshot_git::private_revision(&catalog.root)?;
        let (checkpoint, events, fence, previous, intent) = {
            let mut hub = hub.lock().map_err(|_| "hub 锁损坏")?;
            let time = now()?;
            let previous = hub.authority_saved.clone();
            if let Some(saved) = &previous {
                record_written(&mut hub, host, &saved.commit, &saved.snapshot, time)?;
            }
            crate::tasks::expire(&mut hub, host, time)?;
            let pending_promotion = previous
                .as_ref()
                .is_some_and(|saved| saved.source == SnapshotSource::Private && main_writable);
            if !force && !pending_promotion && !due(&hub, interval, time)? {
                return Ok(report("unchanged", None, hub.last_seq(), 0, None));
            }
            let fence = acquire_main(&mut hub, host, time)?;
            let promote = previous.as_ref().filter(|saved| {
                pending_promotion
                    && saved.snapshot.writer_epoch == fence.epoch
                    && hub.events(saved.snapshot.through_seq).iter().all(|event| {
                        event.kind == "snapshot.written"
                            && !event.stale
                            && event.eid
                                == derived_eid(&saved.commit, "snapshot.written")
                                    .unwrap_or_default()
                    })
                    && saved.snapshot.projection.lease_summaries
                        == hub
                            .leases(time)
                            .into_iter()
                            .map(|(name, lease)| (name, lease.fence()))
                            .collect()
            });
            let (checkpoint, intent) = match promote {
                Some(saved) => (
                    saved.snapshot.clone(),
                    WriteIntent::Promote {
                        source_commit: private_revision.clone().ok_or("私有来源已消失")?,
                    },
                ),
                None => (
                    hub.checkpoint(catalog.config.project_id(), &fence, time)?,
                    WriteIntent::Advance,
                ),
            };
            (checkpoint, hub.events(0), fence, previous, intent)
        };
        let after = previous
            .as_ref()
            .map_or(0, |saved| saved.snapshot.through_seq);
        let mut archives = previous.map_or_else(BTreeMap::new, |saved| saved.archives);
        for event in archive_events(&events, after, checkpoint.through_seq)? {
            let month = chrono::DateTime::parse_from_rfc3339(&event.t)
                .map_err(|error| format!("事件时间非法：{error}"))?
                .format("%Y-%m")
                .to_string();
            let content = archives.entry(month.clone()).or_insert_with(String::new);
            content.push_str(&serde_json::to_string(&event).map_err(|error| error.to_string())?);
            content.push('\n');
        }
        let request = WriteRequest {
            base: catalog.revision.clone(),
            docs_dir: catalog.config.docs_dir().into(),
            state_json: format!(
                "{}\n",
                serde_json::to_string_pretty(&checkpoint).map_err(|error| error.to_string())?
            ),
            archives,
            through_seq: checkpoint.through_seq,
            writer_epoch: fence.epoch,
        };
        let result =
            snapshot_git::write_snapshot_with_intent(&catalog.root, &request, &intent, || {
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
                        "Git 快照 {commit} 已写入，但 hub 确认失败；下次从 main/private 补确认：{error}"
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
            WriteOutcome::Superseded { commit, epoch, seq }
                if matches!(intent, WriteIntent::Promote { .. })
                    && epoch == checkpoint.writer_epoch
                    && seq == checkpoint.through_seq =>
            {
                let current = Catalog::load(&catalog.root)?;
                validate_authority_shared(hub, &current)?;
                let mut session = hub.lock().map_err(|_| "hub 锁损坏")?;
                let saved = session
                    .authority_saved
                    .clone()
                    .ok_or("搬送确认缺少主干快照")?;
                if saved.source != SnapshotSource::Main
                    || saved.commit != commit
                    || saved.snapshot != checkpoint
                    || saved.archives != request.archives
                {
                    return Err("搬送确认的主干快照不等价".into());
                }
                record_written(&mut session, host, &commit, &checkpoint, now()?)?;
                Ok(report("unchanged", Some(commit), seq, epoch, None))
            }
            WriteOutcome::Superseded { commit, epoch, seq } => Ok(report(
                "superseded",
                Some(commit),
                seq,
                epoch,
                Some("main/private 已有较新快照；重新加载后再操作".into()),
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

pub fn due(hub: &ProjectSession, interval: u64, now: u64) -> Result<bool, String> {
    Ok(hub.due(interval, now))
}

pub fn maintain_main(hub: &mut ProjectSession, host: &str, now: u64) -> Result<(), String> {
    if let Some(lease) = hub
        .leases(now)
        .get("main")
        .filter(|lease| lease.holder == format!("snapshot:{host}"))
    {
        renew_fence(hub, host, &lease.fence(), now)?;
    }
    Ok(())
}

pub(crate) fn acquire_main(
    hub: &mut ProjectSession,
    host: &str,
    now: u64,
) -> Result<Fence, String> {
    let holder = format!("snapshot:{host}");
    if let Some(lease) = hub.leases(now).get("main") {
        if lease.holder != holder {
            return Err("main 租约由其他持有者占用".into());
        }
        let fence = lease.fence();
        renew_fence(hub, host, &fence, now)?;
        return Ok(fence);
    }
    let receipt = hub
        .hub
        .lease_acquire(
            AcquireLease {
                host: host.into(),
                names: vec!["main".into()],
                holder,
                ttl_ms: TTL,
            },
            now,
        )
        .map_err(|e| e.to_string())?;
    hub.sync(now)?;
    Ok(receipt.leases[0].fence())
}

pub(crate) fn renew_fence(
    hub: &mut ProjectSession,
    host: &str,
    fence: &Fence,
    now: u64,
) -> Result<(), String> {
    hub.sync(now)?;
    let leases = hub.leases(now);
    let lease = leases
        .get("main")
        .filter(|lease| lease.fence() == *fence)
        .ok_or("main 租约已失效，拒绝写入")?;
    if lease.expires_at.saturating_sub(now) <= TTL - 30_000 {
        hub.hub
            .lease_renew(
                RenewLease {
                    host: host.into(),
                    fences: vec![fence.clone()],
                    ttl_ms: TTL,
                },
                now,
            )
            .map_err(|e| e.to_string())?;
        hub.sync(now)?;
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
        guards: vec![],
    }
}

pub(crate) fn record_written(
    hub: &mut ProjectSession,
    host: &str,
    commit: &str,
    snapshot: &Snapshot,
    now: u64,
) -> Result<(), String> {
    let mut event = new_event(host, "snapshot.written", "main");
    event.eid = derived_eid(commit, "snapshot.written")?;
    event.data = json!({"commit":commit,"seq":snapshot.through_seq,"epoch":snapshot.writer_epoch});
    hub.hub.append(event, now).map_err(|e| e.to_string())?;
    hub.hub
        .snapshot_ack(
            SnapshotAck {
                commit: commit.into(),
                seq: snapshot.through_seq,
                epoch: snapshot.writer_epoch,
            },
            serde_json::to_value(snapshot).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    hub.install_checkpoint(snapshot.clone());
    hub.sync(now)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotSource {
    Main,
    Private,
}

#[derive(Clone)]
pub struct SavedSnapshot {
    pub snapshot: Snapshot,
    pub commit: String,
    pub archives: BTreeMap<String, String>,
    pub source: SnapshotSource,
    pub authority_revision: String,
}

// Prepared values are constructed only by the verified Git reader. The caller
// never supplies a checkpoint or an equivalence flag to bypass validation.
struct PreparedAuthority {
    saved: Option<SavedSnapshot>,
    acknowledged: Option<SavedSnapshot>,
}

fn prepare_authority(
    catalog: &Catalog,
    ack: Option<&SnapshotAck>,
) -> Result<PreparedAuthority, String> {
    let sources = read_snapshot_sources(catalog)?;
    // Only the verified private source can prove a private-to-current-main
    // publication. Reading an arbitrary ack SHA and relabelling it Private would
    // incorrectly authorize unrelated same-watermark main rewrites.
    let acknowledged = sources
        .private
        .as_ref()
        .filter(|private| {
            ack.is_some_and(|ack| {
                ack.commit == private.commit
                    && ack.seq == private.snapshot.through_seq
                    && ack.epoch == private.snapshot.writer_epoch
            })
        })
        .cloned();
    let saved = sources.select()?;
    Ok(PreparedAuthority {
        saved,
        acknowledged,
    })
}

fn authority_key(hub: &ProjectSession, catalog: &Catalog, private: &Option<String>) -> String {
    format!(
        "{}:{private:?}:{:?}:{}",
        catalog.revision,
        hub.snapshot_ack(),
        hub.head.floor_seq
    )
}

/// Validate immutable Git evidence without holding the live session during I/O.
pub fn validate_authority_shared(
    hub: &Mutex<ProjectSession>,
    catalog: &Catalog,
) -> Result<(), String> {
    for _ in 0..3 {
        let private = snapshot_git::private_revision(&catalog.root)?;
        if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
            return Err("权威 main 已变化，须重新加载 Catalog".into());
        }
        let (key, ack) = {
            let mut session = hub.lock().map_err(|_| "hub 锁损坏")?;
            session.bind_catalog(catalog)?;
            let key = authority_key(&session, catalog, &private);
            if session.authority_key.as_ref() == Some(&key) {
                return Ok(());
            }
            (key, session.snapshot_ack().cloned())
        };
        let prepared = prepare_authority(catalog, ack.as_ref())?;
        // Probe the immutable sources again before installing. A changed ack/floor
        // also invalidates the preparation, even when the Git refs stayed fixed.
        if snapshot_git::private_revision(&catalog.root)? != private {
            continue;
        }
        if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
            return Err("权威 main 在归档准备期间变化，须重新加载 Catalog".into());
        }
        let mut session = hub.lock().map_err(|_| "hub 锁损坏")?;
        session.bind_catalog(catalog)?;
        if authority_key(&session, catalog, &private) != key {
            continue;
        }
        validate_saved(
            &session,
            catalog,
            prepared.saved.as_ref(),
            prepared.acknowledged.as_ref(),
        )?;
        session.authority_reads += 1;
        session.authority_saved = prepared.saved;
        session.authority_key = Some(key);
        return Ok(());
    }
    Err("快照来源或 hub 确认持续变化，稍后重新读取".into())
}

/// Compatibility for callers that exclusively own a session. Production shared
/// sessions must use validate_authority_shared so Git/archive I/O remains outside.
pub fn validate_authority(hub: &mut ProjectSession, catalog: &Catalog) -> Result<(), String> {
    hub.bind_catalog(catalog)?;
    let private = snapshot_git::private_revision(&catalog.root)?;
    let key = authority_key(hub, catalog, &private);
    if hub.authority_key.as_ref() == Some(&key) {
        return Ok(());
    }
    let prepared = prepare_authority(catalog, hub.snapshot_ack())?;
    validate_saved(
        hub,
        catalog,
        prepared.saved.as_ref(),
        prepared.acknowledged.as_ref(),
    )?;
    hub.authority_reads += 1;
    hub.authority_saved = prepared.saved;
    hub.authority_key = Some(key);
    Ok(())
}

fn validate_saved(
    hub: &ProjectSession,
    catalog: &Catalog,
    saved: Option<&SavedSnapshot>,
    acknowledged: Option<&SavedSnapshot>,
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
        .map_err(|error| format!("hub 与持久快照不一致，需要 App 确认恢复：{error}"))?;
        if expected != saved.snapshot {
            return Err("hub 历史与持久快照不一致，需要 App 确认恢复".into());
        }
        if hub.snapshot_ack().is_some_and(|ack| {
            let promotion = saved.source == SnapshotSource::Main
                && saved.authority_revision == catalog.revision
                && acknowledged
                    .is_some_and(|old| old.commit == ack.commit && equivalent(old, saved));
            ack.seq > saved.snapshot.through_seq
                || (ack.seq == saved.snapshot.through_seq
                    && (ack.epoch != saved.snapshot.writer_epoch
                        || (ack.commit != saved.commit && !promotion)))
        }) {
            return Err("持久快照回退或同水位提交冲突，拒绝覆盖".into());
        }
    } else if hub.snapshot_ack().is_some() {
        return Err("main/private 缺少已确认的快照，拒绝覆盖".into());
    }
    Ok(())
}

pub fn read_snapshot(catalog: &Catalog) -> Result<Option<SavedSnapshot>, String> {
    read_snapshot_sources(catalog)?.select()
}

pub(crate) struct SnapshotSources {
    pub main: Option<SavedSnapshot>,
    pub private: Option<SavedSnapshot>,
    pub private_revision: Option<String>,
}

pub(crate) fn read_snapshot_sources(catalog: &Catalog) -> Result<SnapshotSources, String> {
    let main = read_snapshot_at(catalog, &catalog.revision, SnapshotSource::Main)?;
    let private_revision = snapshot_git::private_revision(&catalog.root)?;
    let private = match private_revision.as_deref() {
        Some(revision) => {
            validate_private_context(catalog, revision)?;
            Some(
                read_snapshot_at(catalog, revision, SnapshotSource::Private)?
                    .ok_or("私有快照引用缺少 state.json")?,
            )
        }
        None => None,
    };
    Ok(SnapshotSources {
        main,
        private,
        private_revision,
    })
}

impl SnapshotSources {
    pub(crate) fn select(self) -> Result<Option<SavedSnapshot>, String> {
        match (self.main, self.private) {
            (Some(main), Some(private)) => {
                let boundary = |saved: &SavedSnapshot| {
                    (saved.snapshot.writer_epoch, saved.snapshot.through_seq)
                };
                match boundary(&main).cmp(&boundary(&private)) {
                    std::cmp::Ordering::Greater => Ok(Some(main)),
                    std::cmp::Ordering::Less => Ok(Some(private)),
                    std::cmp::Ordering::Equal if equivalent(&main, &private) => Ok(Some(main)),
                    std::cmp::Ordering::Equal => {
                        Err("main 与私有快照同水位内容冲突，拒绝覆盖".into())
                    }
                }
            }
            (main, private) => Ok(main.or(private)),
        }
    }
}

pub(crate) fn equivalent(a: &SavedSnapshot, b: &SavedSnapshot) -> bool {
    a.snapshot == b.snapshot && a.archives == b.archives
}

fn validate_private_context(catalog: &Catalog, revision: &str) -> Result<(), String> {
    let path = crate::config::CONFIG_PATH;
    let entry = repo::git(&catalog.root, &["ls-tree", "-z", revision, "--", path])?;
    let (metadata, _) = entry.split_once('\t').ok_or("私有快照缺少项目配置")?;
    let fields: Vec<_> = metadata.split_whitespace().collect();
    if fields.len() != 3 || !matches!(fields[0], "100644" | "100755") || fields[1] != "blob" {
        return Err("私有快照配置必须是普通文件".into());
    }
    let text = repo::git(&catalog.root, &["cat-file", "blob", fields[2]])?;
    let config = crate::config::Config::parse(&text)?;
    let private: serde_yaml::Value =
        serde_yaml::from_str(&config.to_yaml()).map_err(|e| e.to_string())?;
    let current: serde_yaml::Value =
        serde_yaml::from_str(&catalog.config.to_yaml()).map_err(|e| e.to_string())?;
    if config.project_id() != catalog.config.project_id()
        || crate::init::normalize_docs_dir(config.docs_dir())? != catalog.config.docs_dir()
        || private.get("hub") != current.get("hub")
    {
        return Err("私有快照与当前权威上下文不相容（project_id/docs_dir/hub）".into());
    }
    Ok(())
}

fn read_snapshot_at(
    catalog: &Catalog,
    revision: &str,
    source: SnapshotSource,
) -> Result<Option<SavedSnapshot>, String> {
    let prefix = format!("{}/state/", catalog.config.docs_dir());
    let state_path = format!("{prefix}state.json");
    let entries = repo::git(&catalog.root, &["ls-tree", "-r", "-z", revision])?;
    let mut snapshot = None;
    let mut state_blob = None;
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
            state_blob = Some(fields[2].to_string());
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
            #[cfg(test)]
            authority_concurrency_tests::archive_hook(&catalog.root);
            let mut normalized = String::new();
            for line in text.lines() {
                let event: Event = crate::hub_legacy::decode_archive_line(line)
                    .map_err(|error| format!("月归档非法：{error}"))?;
                if !event.t.starts_with(month) {
                    return Err("月归档包含其他月份事件".into());
                }
                normalized.push_str(&serde_json::to_string(&event).map_err(|e| e.to_string())?);
                normalized.push('\n');
                events.push(event);
            }
            archives.insert(month.into(), normalized);
        } else {
            return Err(format!("未知快照路径：{path}"));
        }
    }
    let Some(mut snapshot) = snapshot else {
        return if archives.is_empty() {
            Ok(None)
        } else {
            Err("存在归档但缺少 state.json".into())
        };
    };
    events.sort_by_key(|event| event.seq);
    snapshot.validate_archive(catalog.config.project_id(), &events)?;
    snapshot.normalize();
    let commit = snapshot_state_commit(
        &catalog.root,
        revision,
        &state_path,
        state_blob.as_deref().ok_or("快照缺少 state blob")?,
        &snapshot,
    )?;
    Ok(Some(SavedSnapshot {
        snapshot,
        commit,
        archives,
        source,
        authority_revision: catalog.revision.clone(),
    }))
}

// Follow the actual state blob through parents, preferring the first parent when
// an ordinary merge keeps its state. A snapshot footer on the current commit is
// significant even when its blob is TREESAME to the private parent (Promote).
fn snapshot_state_commit(
    root: &std::path::Path,
    revision: &str,
    path: &str,
    blob: &str,
    snapshot: &Snapshot,
) -> Result<String, String> {
    let mut current = revision.to_string();
    loop {
        if let Some(trailer) = snapshot_git::read_trailer(root, &current)? {
            if (trailer.epoch, trailer.seq) > (snapshot.writer_epoch, snapshot.through_seq) {
                return Err("快照历史包含更高边界，拒绝将旧 state 作为当前来源".into());
            }
            if trailer.seq != snapshot.through_seq || trailer.epoch != snapshot.writer_epoch {
                return Err("快照与提交 trailer 不一致".into());
            }
            return Ok(current);
        }
        let lineage = repo::git(root, &["rev-list", "--parents", "-n", "1", &current])?;
        let mut matching_parent = None;
        for parent in lineage.split_whitespace().skip(1) {
            let entry = repo::git(
                root,
                &["ls-tree", "-z", parent, "--", &format!(":(literal){path}")],
            )?;
            let metadata = entry
                .split_once('\t')
                .map(|(metadata, _)| metadata)
                .unwrap_or("");
            let fields: Vec<_> = metadata.split_whitespace().collect();
            if fields.len() == 3
                && fields[0] == "100644"
                && fields[1] == "blob"
                && fields[2] == blob
            {
                matching_parent = Some(parent.to_string());
                break;
            }
        }
        current = matching_parent.ok_or("快照 state 变更缺少提交 trailer")?;
    }
}

#[cfg(test)]
mod authority_concurrency_tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;
    use std::sync::{mpsc, Arc, OnceLock};
    use std::time::Duration;

    type ReadHook = Arc<dyn Fn() + Send + Sync>;
    static HOOKS: OnceLock<Mutex<BTreeMap<std::path::PathBuf, ReadHook>>> = OnceLock::new();

    pub(super) fn archive_hook(root: &Path) {
        let hook = HOOKS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .get(root)
            .cloned();
        if let Some(hook) = hook {
            hook();
        }
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    fn fixture() -> (tempfile::TempDir, Catalog, Arc<Mutex<ProjectSession>>) {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-qb", "main"]);
        crate::init::init(temp.path(), &crate::init::InitOptions { docs_dir: None }).unwrap();
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-qm", "fixture"]);
        let catalog = Catalog::load(temp.path()).unwrap();
        let hub = Arc::new(Mutex::new(
            ProjectSession::open(&catalog.common_dir.join("towermd")).unwrap(),
        ));
        SnapshotWriter::default()
            .write(&hub, &catalog, "host", true, || Ok(1000))
            .unwrap();
        (temp, catalog, hub)
    }

    #[test]
    fn slow_archive_preparation_allows_parallel_session_renew_and_status() {
        let (temp, catalog, hub) = fixture();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let once = std::sync::atomic::AtomicBool::new(false);
        HOOKS.get_or_init(Default::default).lock().unwrap().insert(
            catalog.root.clone(),
            Arc::new(move || {
                if !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3))
                        .unwrap();
                }
            }),
        );
        git(temp.path(), &["switch", "-qc", "read-worker"]);
        let worker_hub = hub.clone();
        let worker = std::thread::spawn(move || {
            SnapshotWriter::default().write(&worker_hub, &catalog, "host", false, || Ok(1001))
        });
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let available = if let Ok(mut session) = hub.try_lock() {
            maintain_main(&mut session, "host", 1001).unwrap();
            assert!(session.leases(1001).contains_key("main"));
            true
        } else {
            false
        };
        release_tx.send(()).unwrap();
        worker.join().unwrap().unwrap();
        HOOKS
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .remove(&temp.path().canonicalize().unwrap());
        assert!(
            available,
            "archive preparation held the session lock; renew/status could not complete"
        );
    }
    #[test]
    fn changed_source_during_preparation_never_installs_stale_snapshot() {
        let (temp, catalog, hub) = fixture();
        let root = catalog.root.clone();
        let once = std::sync::atomic::AtomicBool::new(false);
        HOOKS.get_or_init(Default::default).lock().unwrap().insert(
            root.clone(),
            Arc::new(move || {
                if !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    git(
                        &root,
                        &["update-ref", "--no-deref", "-d", "refs/towermd/snapshot"],
                    );
                }
            }),
        );
        let result = validate_authority_shared(&hub, &catalog);
        HOOKS
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .remove(&temp.path().canonicalize().unwrap());
        assert!(result.is_err());
        assert!(
            hub.lock().unwrap().authority_saved.is_none(),
            "a discarded preparation must never enter the authority cache"
        );
        assert_eq!(hub.lock().unwrap().authority_reads(), 1); // first write's empty authority only
    }

    #[test]
    fn static_authority_cache_does_not_decode_archives_again() {
        let (temp, catalog, hub) = fixture();
        let decoded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = decoded.clone();
        HOOKS.get_or_init(Default::default).lock().unwrap().insert(
            catalog.root.clone(),
            Arc::new(move || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }),
        );
        validate_authority_shared(&hub, &catalog).unwrap();
        let first = decoded.load(std::sync::atomic::Ordering::SeqCst);
        assert!(first > 0);
        for _ in 0..10 {
            validate_authority_shared(&hub, &catalog).unwrap();
        }
        assert_eq!(decoded.load(std::sync::atomic::Ordering::SeqCst), first);
        HOOKS
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .remove(&temp.path().canonicalize().unwrap());
    }
}
