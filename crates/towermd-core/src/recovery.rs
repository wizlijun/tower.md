use crate::catalog::Catalog;
use crate::hub::Hub;
use crate::hub_protocol::{HubHead, HubProtocol, NewEvent, RebuildSeed};
use crate::project_session::ProjectSession;
use crate::recovery_git::{self, TrailerContext};
use crate::recovery_outbox::RecoveryInputs;
use crate::snapshot::{archive_events, Snapshot};
use crate::snapshot_git::{WriteOutcome, WriteRequest};
use crate::{repo, snapshot_git, snapshot_service};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRequest {
    pub status: String,
    pub request_id: String,
    pub project_id: String,
    pub main_revision: String,
    pub source_commit: String,
    pub source_seq: u64,
    pub seq_boundary: u64,
    pub epoch_boundary: u64,
    pub source_epoch_high_water: u64,
    pub source_reference: String,
    pub inputs_digest: String,
    pub requested_at: u64,
    pub expires_at: u64,
    pub nonce: String,
}
/// Read-only source identity failure, before any recovery target or Git write.
#[derive(Debug)]
pub enum RequestSourceError {
    Changed,
    Read(String),
}
impl std::fmt::Display for RequestSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Changed => f.write_str("main/private 来源已变化，须重新请求 App 确认"),
            Self::Read(error) => f.write_str(error),
        }
    }
}

/// Reuse the authority and private-ref readers; a read error is never absence.
pub fn check_request_sources(
    root: &std::path::Path,
    request: &RecoveryRequest,
) -> Result<(), RequestSourceError> {
    let private = request
        .source_reference
        .strip_prefix("main;refs/towermd/snapshot:")
        .or_else(|| {
            request
                .source_reference
                .strip_prefix("refs/towermd/snapshot:")
        })
        .ok_or_else(|| RequestSourceError::Read("恢复请求来源绑定损坏".into()))?;
    let expected_private = if private == "missing" && request.source_reference.starts_with("main;")
    {
        None
    } else if matches!(private.len(), 40 | 64)
        && private
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Some(private)
    } else {
        return Err(RequestSourceError::Read("恢复请求来源绑定损坏".into()));
    };
    let reference = crate::catalog::main_reference(root).map_err(RequestSourceError::Read)?;
    if crate::catalog::read_authority_reference(root, reference)
        .map_err(RequestSourceError::Read)?
        .as_deref()
        != Some(&request.main_revision)
    {
        return Err(RequestSourceError::Changed);
    }
    if snapshot_git::private_revision(root)
        .map_err(RequestSourceError::Read)?
        .as_deref()
        != expected_private
    {
        return Err(RequestSourceError::Changed);
    }
    Ok(())
}

/// Confirmation failures retain their category through the writer and App adapter.
#[derive(Debug)]
pub enum ConfirmError {
    Conflict(String),
    ReadOrRefused(String),
    Persisted { commit: String, error: String },
}
impl std::fmt::Display for ConfirmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict(error) | Self::ReadOrRefused(error) => f.write_str(error),
            Self::Persisted { commit, error } => write!(f, "恢复边界 {commit} 已持久化，本地安装失败；须重新读取双来源的更高边界，不可重用旧请求：{error}"),
        }
    }
}
impl From<String> for ConfirmError {
    fn from(error: String) -> Self {
        Self::ReadOrRefused(error)
    }
}
impl From<&str> for ConfirmError {
    fn from(error: &str) -> Self {
        Self::ReadOrRefused(error.into())
    }
}
impl From<RequestSourceError> for ConfirmError {
    fn from(error: RequestSourceError) -> Self {
        match error {
            RequestSourceError::Changed => Self::Conflict(error.to_string()),
            RequestSourceError::Read(error) => Self::ReadOrRefused(error),
        }
    }
}

fn recheck_source_error(
    catalog: &Catalog,
    request: &RecoveryRequest,
    inputs: &RecoveryInputs,
    error: ConfirmError,
) -> ConfirmError {
    let ConfirmError::ReadOrRefused(original) = error else {
        return error;
    };
    match check_request_sources(&catalog.root, request) {
        Err(RequestSourceError::Changed) => {
            ConfirmError::Conflict(format!("{original}；{}", RequestSourceError::Changed))
        }
        Err(RequestSourceError::Read(read)) => {
            ConfirmError::ReadOrRefused(format!("{original}；{read}"))
        }
        Ok(()) => match inputs.reload(catalog) {
            Ok(current) if current.digest() != request.inputs_digest => {
                ConfirmError::Conflict(format!("{original}；恢复输入已变化，须重新请求 App 确认"))
            }
            Ok(_) => ConfirmError::ReadOrRefused(original),
            Err(read) => ConfirmError::ReadOrRefused(format!("{original}；{read}")),
        },
    }
}

struct AppConfirmation {
    request_id: String,
    actor: String,
    expires_at: u64,
}

pub(crate) struct VerifiedRecoverySeed {
    seed: RebuildSeed,
    checkpoint: Snapshot,
    request: RecoveryRequest,
}
impl VerifiedRecoverySeed {
    pub(crate) fn protocol_seed(&self) -> &RebuildSeed {
        &self.seed
    }
    pub(crate) fn checkpoint(&self) -> &Snapshot {
        &self.checkpoint
    }
    pub(crate) fn probe(&self, catalog: &Catalog, inputs: &RecoveryInputs) -> Result<(), String> {
        inputs.probe(catalog)?;
        if catalog.revision != self.request.main_revision
            || inputs.digest() != self.request.inputs_digest
        {
            return Err("verified recovery seed source changed".into());
        }
        Ok(())
    }
}

fn validate_confirmation(
    request: &RecoveryRequest,
    confirmation: &AppConfirmation,
    now: u64,
) -> Result<(), ConfirmError> {
    if !confirmation.actor.strip_prefix("human:").is_some_and(|id| {
        !id.is_empty()
            && !id.contains([':', '/'])
            && !id.chars().any(|c| c.is_whitespace() || c.is_control())
    }) {
        return Err("恢复需要匹配且未过期的 App 人工确认".into());
    }
    if confirmation.request_id != request.request_id
        || confirmation.expires_at != request.expires_at
        || confirmation.expires_at <= now
    {
        return Err(ConfirmError::Conflict(
            "恢复需要匹配且未过期的 App 人工确认".into(),
        ));
    }
    Ok(())
}

fn recovery_event(host: &str, kind: &str, data: serde_json::Value) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: host.into(),
        actor: "process:towermd-daemon".into(),
        kind: kind.into(),
        layer: "L3".into(),
        obj: "main".into(),
        data,
        guards: vec![],
    }
}

pub fn prepare(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    requested_at: u64,
    expires_at: u64,
    nonce: &str,
) -> Result<RecoveryRequest, String> {
    Ok(prepare_verified(catalog, inputs, requested_at, expires_at, nonce)?.request)
}

// Reuse one verified input set within this preparation only. Every later Git
// boundary still performs its own actual source and deadline checks.
struct PreparedRecoverySources {
    request: RecoveryRequest,
    history: recovery_git::VerifiedRecoveryHistory,
    saved: snapshot_service::SavedSnapshot,
    facts: Vec<NewEvent>,
}
fn prepare_verified(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    requested_at: u64,
    expires_at: u64,
    nonce: &str,
) -> Result<PreparedRecoverySources, String> {
    let parsed = uuid::Uuid::parse_str(nonce).map_err(|e| e.to_string())?;
    if parsed.get_version_num() != 7
        || parsed.to_string() != nonce
        || expires_at <= requested_at
        || expires_at - requested_at > 300_000
    {
        return Err("invalid recovery request nonce/expiry".into());
    }
    let history = recovery_git::read_recovery_history(catalog, inputs)?;
    let sources = history.snapshot_sources().clone();
    let private = sources.private_revision.clone();
    let saved = sources
        .select()?
        .ok_or("main 和私有引用均没有可恢复的快照")?;
    if let Some((_, trailer)) =
        snapshot_git::highest_snapshot_fence(&catalog.root, &catalog.revision)?
    {
        if (trailer.epoch, trailer.seq) > (saved.snapshot.writer_epoch, saved.snapshot.through_seq)
        {
            return Err("main 或私有快照历史曾有更高恢复水位；拒绝从回退快照恢复".into());
        }
    }
    let facts = recover_git_facts(catalog, &saved, &history)?;
    let (seq_boundary, epoch_boundary) =
        inputs.recovery_boundaries(saved.snapshot.through_seq, saved.snapshot.epoch_high_water)?;
    let mut request = RecoveryRequest {
        status: "awaiting_app_confirmation".into(),
        request_id: String::new(),
        project_id: catalog.config.project_id().into(),
        main_revision: catalog.revision.clone(),
        source_commit: saved.commit.clone(),
        source_seq: saved.snapshot.through_seq,
        seq_boundary,
        epoch_boundary,
        source_epoch_high_water: saved.snapshot.epoch_high_water,
        source_reference: match saved.source {
            snapshot_service::SnapshotSource::Main => format!(
                "main;refs/towermd/snapshot:{}",
                private.as_deref().unwrap_or("missing")
            ),
            snapshot_service::SnapshotSource::Private => format!(
                "refs/towermd/snapshot:{}",
                private.ok_or("private source disappeared")?
            ),
        },
        inputs_digest: inputs.digest().into(),
        requested_at,
        expires_at,
        nonce: nonce.into(),
    };
    request.request_id = crate::recovery_contract::canonical_hash(&request)?;
    Ok(PreparedRecoverySources {
        request,
        history,
        saved,
        facts,
    })
}

/// Disposable source-only display cache. It grants no recovery or writer capability.
#[derive(Default)]
pub struct SourceStatusCache {
    entries: Mutex<BTreeMap<(std::path::PathBuf, std::path::PathBuf), SourceStatusEntry>>,
}
#[derive(Clone, PartialEq)]
struct SourceStatusKey {
    root: std::path::PathBuf,
    common: std::path::PathBuf,
    authority: String,
    private: Option<String>,
    config: String,
    context: crate::recovery_outbox::RecoveryContext,
    inputs: String,
}
struct SourceStatusEntry {
    key: SourceStatusKey,
    generation: u64,
    value: Option<serde_json::Value>,
}
impl SourceStatusKey {
    fn probe(catalog: &Catalog, inputs: &RecoveryInputs) -> Result<Self, String> {
        inputs.probe(catalog)?;
        let authority = crate::catalog::authority_revision(&catalog.root)?;
        if authority != catalog.revision {
            return Err("source status authority changed; reload Catalog".into());
        }
        Ok(Self {
            root: catalog.root.canonicalize().map_err(|e| e.to_string())?,
            common: catalog
                .common_dir
                .canonicalize()
                .map_err(|e| e.to_string())?,
            authority,
            private: snapshot_git::private_revision(&catalog.root)?,
            config: catalog.config.to_yaml(),
            // inputs.probe above already read actual common/config and proved
            // its context equals this immutable Catalog and the bound inputs.
            context: crate::recovery_outbox::RecoveryContext::extract(catalog)?,
            inputs: inputs.digest().into(),
        })
    }
}
impl SourceStatusCache {
    pub fn status_source(
        &self,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
    ) -> Result<Option<serde_json::Value>, String> {
        let key = SourceStatusKey::probe(catalog, inputs)?;
        let identity = (key.common.clone(), key.root.clone());
        let (generation, cached) = {
            let entries = self
                .entries
                .lock()
                .map_err(|_| "source status cache lock poisoned")?;
            let entry = entries.get(&identity);
            (
                entry.map_or(0, |e| e.generation),
                entry.filter(|e| e.key == key).map(|e| e.value.clone()),
            )
        };
        // All Git, context/input probes and the sole history reader run outside
        // the cache lock. A blocked source cannot block another project's status.
        let value = match cached {
            Some(value) => value,
            None => status_source(catalog, inputs)?,
        };
        #[cfg(test)]
        tests::after_source_status_read(&catalog.root);
        if SourceStatusKey::probe(catalog, inputs)? != key {
            return Err("source status sources changed during preparation".into());
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "source status cache lock poisoned")?;
        let current = entries.get(&identity);
        if current.map_or(0, |e| e.generation) != generation {
            return current
                .filter(|e| e.key == key)
                .map(|e| e.value.clone())
                .ok_or_else(|| "source status cache changed during preparation".into());
        }
        entries.insert(
            identity,
            SourceStatusEntry {
                key,
                generation: generation.saturating_add(1),
                value: value.clone(),
            },
        );
        Ok(value)
    }
}

/// Source-only summary; never constructs a business session or grants write authority.
pub fn status_source(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
) -> Result<Option<serde_json::Value>, String> {
    let history = recovery_git::read_recovery_history(catalog, inputs)?;
    let Some(saved) = history.snapshot_sources().clone().select()? else {
        return Ok(None);
    };
    recover_git_facts(catalog, &saved, &history)?;
    let summary = history.snapshot_summary()?;
    let last = summary.last.as_ref().map(|last| json!({"commit":last.commit,"seq":last.seq,"epoch":last.epoch,"written_at":last.written_at}));
    let snapshot = json!({"last":last,"deferred_reason":summary.alert.as_ref().map(|alert| &alert.reason),"deferred_since":summary.alert.as_ref().map(|alert| alert.since)});
    let tasks: Vec<_> = saved
        .snapshot
        .projection
        .objects
        .get("Task")
        .into_iter()
        .flat_map(|tasks| tasks.values())
        .cloned()
        .collect();
    Ok(Some(
        json!({"status":"recovery_required", "message":"需恢复；请运行 hub rebuild 生成新的 App 确认请求", "project_id":catalog.config.project_id(),
        "source_commit":saved.commit,"source_seq":saved.snapshot.through_seq,
        "snapshot":snapshot,"source_epoch_high_water":saved.snapshot.epoch_high_water,"source_reference":match saved.source { snapshot_service::SnapshotSource::Main => "main", snapshot_service::SnapshotSource::Private => "refs/towermd/snapshot" },"tasks":tasks}),
    ))
}

pub(crate) fn recover_git_facts(
    catalog: &Catalog,
    saved: &snapshot_service::SavedSnapshot,
    history: &recovery_git::VerifiedRecoveryHistory,
) -> Result<Vec<NewEvent>, String> {
    let mut facts = vec![];
    let mut seen = BTreeSet::new();
    let mut revisions = vec![catalog.revision.clone()];
    if let Some(private) = snapshot_git::private_revision(&catalog.root)? {
        revisions.push(private);
    }
    for revision in &revisions {
        for commit in repo::git(
            &catalog.root,
            &[
                "rev-list",
                "--reverse",
                "--topo-order",
                revision,
                "--not",
                &saved.commit,
            ],
        )?
        .lines()
        {
            if !seen.insert(commit.to_string()) {
                continue;
            }
            let message = repo::git(
                &catalog.root,
                &["show", "--no-patch", "--format=%B", commit],
            )?;
            let unknown = recovery_git::unknown_trailer_lines(&message);
            if !unknown.is_empty() {
                let mut event = recovery_event(
                    "git",
                    "alert.raised",
                    json!({"commit":commit,"reason":"unknown Towermd trailers","raw_lines":unknown}),
                );
                event.eid = crate::events::derived_eid(commit, "alert.raised")?;
                facts.push(event);
            }
            let fact = (|| -> Result<Option<NewEvent>, String> {
                Ok(match recovery_git::parse_trailer_context(&message)? {
                    TrailerContext::Task(trailers) => Some(
                        recovery_git::prove_merge(
                            catalog,
                            history,
                            &recovery_git::MergeProofInput {
                                commit: commit.into(),
                                trailers,
                            },
                        )?
                        .into_event(),
                    ),
                    TrailerContext::Publication(_) => Some(recovery_git::baseline_created_event(
                        &recovery_git::prove_publication(catalog, history, commit)?,
                    )?),
                    TrailerContext::Snapshot { .. } => {
                        let saved = snapshot_service::read_snapshot_commit(catalog, commit)?;
                        let mut event = recovery_event(
                            "git",
                            "snapshot.written",
                            json!({"commit":saved.commit,"seq":saved.snapshot.through_seq,"epoch":saved.snapshot.writer_epoch}),
                        );
                        event.eid = crate::events::derived_eid(commit, "snapshot.written")?;
                        Some(event)
                    }
                    TrailerContext::None => None,
                })
            })();
            match fact {
                Ok(Some(event)) => facts.push(event),
                Ok(None) => {}
                Err(reason) => {
                    let raw_lines = recovery_git::towermd_trailer_lines(&message);
                    for kind in ["gate.failed", "hold.opened"] {
                        let mut event = recovery_event(
                            "git",
                            kind,
                            json!({"commit":commit,"reason":reason,"raw_lines":raw_lines}),
                        );
                        event.eid = crate::events::derived_eid(commit, kind)?;
                        facts.push(event);
                    }
                }
            }
        }
    }
    // Tags are candidates only. Reachability and the same publication proof decide authority.
    let mut by_commit = BTreeMap::<String, Vec<String>>::new();
    for reference in repo::git(
        &catalog.root,
        &["for-each-ref", "--format=%(refname)", "refs/tags/towermd/"],
    )?
    .lines()
    {
        let target = reference
            .strip_prefix("refs/tags/")
            .ok_or("invalid baseline tag ref")?;
        let Ok(commit) = repo::git(
            &catalog.root,
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        ) else {
            continue;
        };
        if repo::git(
            &catalog.root,
            &["merge-base", "--is-ancestor", &commit, &catalog.revision],
        )
        .is_err()
        {
            continue;
        }
        by_commit.entry(commit).or_default().push(target.into());
    }
    for (commit, tags) in by_commit {
        let proof = recovery_git::prove_publication(catalog, history, &commit);
        match proof {
            Ok(proof) => {
                let event = recovery_git::baseline_created_event(&proof)?;
                let archived = saved.archives.values().any(|text| {
                    text.lines().any(|line| {
                        crate::hub_legacy::decode_archive_line(line)
                            .is_ok_and(|e| e.eid == event.eid)
                    })
                });
                if !archived && !facts.iter().any(|old| old.eid == event.eid) {
                    facts.push(event);
                }
                if proof.diagnostics().is_empty() {
                    continue;
                }
                let diagnostic = json!({"commit":commit,"reason":"unverified baseline tags","tags":tags,"diagnostics":proof.diagnostics()});
                let eid = crate::events::derived_eid(&commit, "alert.raised")?;
                if let Some(old) = facts.iter_mut().find(|event| event.eid == eid) {
                    old.data["baseline_tags"] = diagnostic;
                } else {
                    let mut event = recovery_event("git", "alert.raised", diagnostic);
                    event.eid = eid;
                    facts.push(event);
                }
            }
            Err(reason) => {
                let eid = crate::events::derived_eid(&commit, "alert.raised")?;
                let diagnostic = json!({"commit":commit,"reason":"baseline tag has no verified publication","tags":tags,"diagnostic":reason});
                if let Some(old) = facts.iter_mut().find(|event| event.eid == eid) {
                    old.data["baseline_tags"] = diagnostic;
                } else {
                    let mut event = recovery_event("git", "alert.raised", diagnostic);
                    event.eid = eid;
                    facts.push(event);
                }
            }
        }
    }
    // Candidates can normalize to the same actual main entry. Build one fact table first,
    // then order by actual commit topology, including tag candidates from older history.
    let mut table = BTreeMap::<String, NewEvent>::new();
    for event in facts {
        if let Some(previous) = table.get(&event.eid) {
            if previous != &event {
                return Err("conflicting canonical Git fact".into());
            }
        } else {
            table.insert(event.eid.clone(), event);
        }
    }
    let mut args = vec!["rev-list", "--reverse", "--topo-order"];
    args.extend(revisions.iter().map(String::as_str));
    let order = repo::git(&catalog.root, &args)?;
    let positions: BTreeMap<_, _> = order
        .lines()
        .enumerate()
        .map(|(i, commit)| (commit, i))
        .collect();
    let mut ordered = Vec::with_capacity(table.len());
    for event in table.into_values() {
        let commit = event.data["commit"]
            .as_str()
            .ok_or("canonical Git fact lacks actual commit")?;
        let rank = *positions
            .get(commit)
            .ok_or("canonical Git fact is not in actual source topology")?;
        ordered.push((rank, event));
    }
    ordered.sort_by(|(a, x), (b, y)| (a, &x.kind, &x.eid).cmp(&(b, &y.kind, &y.eid)));
    Ok(ordered.into_iter().map(|(_, event)| event).collect())
}
enum EmptyTarget {
    Registered {
        session: Arc<Mutex<ProjectSession>>,
        watermark: HubHead,
    },
    New {
        protocol: Box<dyn HubProtocol>,
        watermark: HubHead,
    },
}
fn empty_head(protocol: &mut dyn HubProtocol, now: u64) -> Result<HubHead, String> {
    let head = protocol.subscribe(0, now).map_err(|e| e.to_string())?.head;
    if head.contiguous_seq != 0
        || head.floor_seq != 0
        || head.ack.is_some()
        || !head.leases.is_empty()
    {
        return Err("hub journal 仍有可读历史，拒绝将其作为丢失数据覆盖".into());
    }
    Ok(head)
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn confirm(
    existing: Option<Arc<Mutex<ProjectSession>>>,
    catalog: &Catalog,
    request: &RecoveryRequest,
    actor: &str,
    expires_at: u64,
    inputs: &RecoveryInputs,
    host: &str,
    mut now: impl FnMut() -> Result<u64, String>,
) -> Result<(snapshot_service::SnapshotReport, Arc<Mutex<ProjectSession>>), ConfirmError> {
    let confirmation = AppConfirmation {
        request_id: request.request_id.clone(),
        actor: actor.into(),
        expires_at,
    };
    validate_confirmation(request, &confirmation, now()?)?;
    if let Some(session) = &existing {
        let time = now()?;
        session
            .lock()
            .map_err(|_| "hub 锁损坏")?
            .validate_empty(catalog, time)?;
    }
    let (current, history, saved, facts) = (|| -> Result<_, ConfirmError> {
        crate::origin::refresh_origin_main(&catalog.root)?;
        check_request_sources(&catalog.root, request)?;
        let current = Catalog::load(&catalog.root)?;
        let prepared = prepare_verified(
            &current,
            inputs,
            request.requested_at,
            request.expires_at,
            &request.nonce,
        )?;
        if prepared.request != *request {
            return Err(ConfirmError::Conflict(
                "main/private 或恢复输入已变化，须重新请求 App 确认".into(),
            ));
        }
        Ok((current, prepared.history, prepared.saved, prepared.facts))
    })()
    .map_err(|error| recheck_source_error(catalog, request, inputs, error))?;
    let mut checkpoint = saved.snapshot.clone();
    // Prove the old source in its own mode first, then derive Human state at
    // its actual Q before extending the legal recovery gap. The actual fresh
    // hub.rebuilt Event below subsequently invalidates all old human reviews.
    crate::project_session::hydrate_human_projection(
        &current,
        &history,
        saved.snapshot.through_seq,
        &mut checkpoint.projection,
    )?;
    checkpoint.through_seq = request.seq_boundary;
    checkpoint.epoch_high_water = request.epoch_boundary;
    checkpoint
        .recovery_gaps
        .push(crate::snapshot::SequenceRange {
            start: saved
                .snapshot
                .through_seq
                .checked_add(1)
                .ok_or("recovery sequence overflow")?,
            end: request.seq_boundary,
        });
    checkpoint.validate_metadata()?;
    let mut receipts = BTreeMap::new();
    for text in saved.archives.values() {
        for line in text.lines() {
            let event = crate::hub_legacy::decode_archive_line(line).map_err(|e| e.to_string())?;
            if !event.is_pure_heartbeat() {
                receipts.insert(event.eid, event.seq);
            }
        }
    }
    let seed = VerifiedRecoverySeed {
        seed: RebuildSeed {
            final_snapshot: None,
            source_seq: saved.snapshot.through_seq,
            source_epoch: saved.snapshot.epoch_high_water,
            seq_boundary: request.seq_boundary,
            epoch_boundary: request.epoch_boundary,
            checkpoint: serde_json::to_value(&checkpoint).map_err(|e| e.to_string())?,
            receipts,
            tail: vec![],
        },
        checkpoint,
        request: request.clone(),
    };
    let mut target = match existing {
        Some(session) => {
            let time = now()?;
            let watermark = session
                .lock()
                .map_err(|_| "hub 锁损坏")?
                .retire_empty(&current, time)?;
            EmptyTarget::Registered { session, watermark }
        }
        None => {
            let directory = current.common_dir.join("towermd");
            // Normal open can repair a partial tail; recovery never changes corrupt evidence.
            if crate::hub::inspect_store(&directory)? == crate::hub::StoreState::Nonempty {
                return Err("hub journal 仍有可读历史，拒绝将其作为丢失数据覆盖".into());
            }
            let mut protocol: Box<dyn HubProtocol> = Box::new(Hub::open(&directory)?);
            let watermark = empty_head(&mut *protocol, now()?)?;
            EmptyTarget::New {
                protocol,
                watermark,
            }
        }
    };
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut staged = ProjectSession::new_staging(
        Box::new(Hub::open(temporary.path())?),
        &current,
        inputs,
        &seed,
        now()?,
    )
    .map_err(|error| recheck_source_error(&current, request, inputs, error.into()))?;
    let time = now()?;
    let mut approval = recovery_event(
        host,
        "hub.rebuild.approved",
        json!({"request_id":request.request_id,"source_commit":request.source_commit}),
    );
    approval.actor = confirmation.actor.clone();
    staged
        .hub
        .append(approval, time)
        .map_err(|e| e.to_string())?;
    staged.hub.append(recovery_event(host,"hub.rebuilt",json!({"source_commit":request.source_commit,"seq_boundary":request.seq_boundary,"epoch_boundary":request.epoch_boundary})),time).map_err(|e|e.to_string())?;
    let mut seen: BTreeSet<_> = seed.seed.receipts.keys().cloned().collect();
    let mut written = recovery_event(
        host,
        "snapshot.written",
        json!({"commit":saved.commit,"seq":saved.snapshot.through_seq,"epoch":saved.snapshot.writer_epoch}),
    );
    written.eid = crate::events::derived_eid(&saved.commit, "snapshot.written")?;
    for event in std::iter::once(written).chain(facts) {
        if seen.insert(event.eid.clone()) {
            let event = if event.kind == "baseline.created" {
                match history.publication_created_original(&event.eid) {
                    Some(original) => crate::request_ledger::recovery_receipt(
                        current.config.project_id(),
                        original,
                        host,
                    )?,
                    None => event,
                }
            } else {
                event
            };
            staged.hub.append(event, time).map_err(|e| e.to_string())?;
        }
    }
    for event in inputs.pending().iter().cloned().chain(
        inputs
            .confirmed()
            .iter()
            .map(crate::recovery_outbox::original_of),
    ) {
        if !seen.insert(event.eid.clone()) {
            continue;
        }
        let event = if event.kind == "coldstart.reported"
            || (event.kind == "evidence.recorded"
                && event.data["evidence"]["coldstart"]["schema"] == 1)
        {
            match history.coldstart_original(&event.eid) {
                Some(original) => crate::coldstart_evidence::recovery_receipt(original, host)?,
                // Pending records have no original receipt to preserve. They
                // remain producer intents and acquire only their real new receipt.
                None => event,
            }
        } else if crate::request_ledger::is_restorable_original(&event) {
            let original = match event.kind.as_str() {
                "baseline.publication_prepared" => history
                    .publication_prepared_originals(&current)?
                    .into_iter()
                    .find(|original| original.eid == event.eid),
                "baseline.created" => history.publication_created_original(&event.eid),
                "hold.wait_finished" | "hold.attention" => Some(
                    history.ask_system_original(&current, &event)?
                        .ok_or("Ask system request uncertain: missing original accepted receipt; refusing a new recovery settlement")?,
                ),
                "hold.answered" if crate::ask_answer_contract::is_app_answer(&event.kind, &event.data) => Some(
                    history.app_answer_original(&current, &event)?
                        .ok_or("App Answer request uncertain: missing original accepted receipt; refusing a new recovery answer")?,
                ),
                _ => history.baseline_request_original(&event.eid),
            };
            match original {
                Some(original) => crate::request_ledger::recovery_receipt(
                    current.config.project_id(),
                    original,
                    host,
                )?,
                None => event,
            }
        } else if event.kind == "merge.done" {
            let commit = event.data["commit"]
                .as_str()
                .ok_or("outbox merge lacks actual commit")?;
            let message = repo::git(
                &current.root,
                &["show", "--no-patch", "--format=%B", commit],
            )
            .map_err(|error| recheck_source_error(&current, request, inputs, error.into()))?;
            let TrailerContext::Task(trailers) = recovery_git::parse_trailer_context(&message)?
            else {
                return Err("outbox merge lacks actual Task trailers".into());
            };
            recovery_git::prove_merge(
                &current,
                &history,
                &recovery_git::MergeProofInput {
                    commit: commit.into(),
                    trailers,
                },
            )
            .map_err(|error| recheck_source_error(&current, request, inputs, error.into()))?
            .into_event()
        } else {
            event
        };
        staged.hub.append(event, time).map_err(|e| e.to_string())?;
    }
    staged
        .sync_staging(&current, inputs, &seed, time)
        .map_err(|error| recheck_source_error(&current, request, inputs, error.into()))?;
    let runs = staged
        .projection
        .objects
        .get("Run")
        .cloned()
        .unwrap_or_default();
    for run in runs.values().filter(|run| run["state"] == "running") {
        let task_id = run["task"].as_str().ok_or("恢复 Run 缺少 task")?;
        let mut task = staged
            .projection
            .objects
            .get("Task")
            .and_then(|tasks| tasks.get(task_id))
            .cloned()
            .ok_or("恢复 Run 引用未知 Task")?;
        if task["state"] == "merged" {
            continue;
        }
        let mut run = run.clone();
        task["state"] = "ready".into();
        run["state"] = "released".into();
        let mut event = recovery_event(host, "task.released", json!({"task":task,"run":run}));
        event.layer = "L5".into();
        event.obj = task_id.into();
        staged.hub.append(event, time).map_err(|e| e.to_string())?;
    }
    staged
        .sync_staging(&current, inputs, &seed, time)
        .map_err(|error| recheck_source_error(&current, request, inputs, error.into()))?;
    let fence = snapshot_service::acquire_main(&mut staged, host, time)?;
    let checkpoint = staged.checkpoint(&request.project_id, &fence, time)?;
    let mut archives = saved.archives;
    for event in archive_events(
        &staged.events(0),
        request.seq_boundary,
        checkpoint.through_seq,
    )? {
        let month = chrono::DateTime::parse_from_rfc3339(&event.t)
            .map_err(|e| e.to_string())?
            .format("%Y-%m")
            .to_string();
        let content = archives.entry(month).or_default();
        content.push_str(&serde_json::to_string(&event).map_err(|e| e.to_string())?);
        content.push('\n');
    }
    let mut all = vec![];
    for text in archives.values() {
        for line in text.lines() {
            all.push(crate::hub_legacy::decode_archive_line(line).map_err(|e| e.to_string())?);
        }
    }
    all.sort_by_key(|e| e.seq);
    checkpoint.validate_archive(&request.project_id, &all)?;
    let write = WriteRequest {
        base: current.revision.clone(),
        docs_dir: current.config.docs_dir().into(),
        state_json: format!(
            "{}\n",
            serde_json::to_string_pretty(&checkpoint).map_err(|e| e.to_string())?
        ),
        archives,
        through_seq: checkpoint.through_seq,
        writer_epoch: fence.epoch,
    };
    // Preserve actual callback errors across the existing String-only Git interface.
    let mut callback_error = None;
    let outcome = snapshot_git::write_snapshot(&current.root, &write, || {
        callback_error = None;
        let checked = (|| -> Result<(), ConfirmError> {
            check_request_sources(&current.root, request)?;
            if prepare(
                &Catalog::load(&current.root)?,
                inputs,
                request.requested_at,
                request.expires_at,
                &request.nonce,
            )? != *request
            {
                return Err(ConfirmError::Conflict(
                    "source changed before recovery Git boundary".into(),
                ));
            }
            let time = now()?;
            validate_confirmation(request, &confirmation, time)?;
            snapshot_service::renew_fence(&mut staged, host, &fence, time)?;
            Ok(())
        })()
        .map_err(|error| recheck_source_error(&current, request, inputs, error));
        match checked {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = error.to_string();
                callback_error = Some(error);
                Err(message)
            }
        }
    })
    .map_err(|error| callback_error.unwrap_or(ConfirmError::ReadOrRefused(error)))?;
    let commit = match outcome {
        WriteOutcome::Written { commit } => commit,
        other => return Err(format!("恢复边界未写入，不激活服务：{other:?}").into()),
    };
    let installed = (|| -> Result<Arc<Mutex<ProjectSession>>, String> {
        let time = now()?;
        // Preserve the verified prefix before ack compacts the staging journal.
        let mut install = seed.seed.clone();
        install.tail = staged
            .hub
            .subscribe(request.seq_boundary, time)
            .map_err(|e| e.to_string())?
            .events
            .into_iter()
            .filter(|event| event.seq <= checkpoint.through_seq)
            .collect();
        snapshot_service::record_written(&mut staged, host, &commit, &checkpoint, time)?;
        let installed_catalog = Catalog::load(&current.root)?;
        staged.verify_owned(&installed_catalog, inputs, time)?;
        install.tail.extend(
            staged
                .hub
                .subscribe(checkpoint.through_seq, time)
                .map_err(|e| e.to_string())?
                .events,
        );
        install.final_snapshot = Some(crate::hub_protocol::RebuildSnapshot {
            ack: crate::hub_protocol::SnapshotAck {
                commit: commit.clone(),
                seq: checkpoint.through_seq,
                epoch: checkpoint.writer_epoch,
            },
            checkpoint: serde_json::to_value(&checkpoint).map_err(|e| e.to_string())?,
        });
        match &mut target {
            EmptyTarget::Registered { session, watermark } => {
                let mut target = session.lock().map_err(|_| "hub 锁损坏")?;
                if empty_head(&mut *target.hub, time)? != *watermark {
                    return Err("hub 在恢复期间已变化，拒绝覆盖".into());
                }
                target.hub.rebuild(install).map_err(|e| e.to_string())?;
                let actual = target
                    .hub
                    .subscribe(checkpoint.through_seq, time)
                    .map_err(|e| e.to_string())?;
                let expected = staged
                    .hub
                    .subscribe(checkpoint.through_seq, time)
                    .map_err(|e| e.to_string())?;
                if actual != expected {
                    return Err("installed target differs from verified staging".into());
                }
                target.activate_rebuilt(staged);
                drop(target);
                Ok(session.clone())
            }
            EmptyTarget::New {
                protocol,
                watermark,
            } => {
                if empty_head(&mut **protocol, time)? != *watermark {
                    return Err("hub 在恢复期间已变化，拒绝覆盖".into());
                }
                protocol.rebuild(install).map_err(|e| e.to_string())?;
                let actual = std::mem::replace(protocol, staged.hub);
                Ok(Arc::new(Mutex::new(
                    ProjectSession::new(actual, &installed_catalog, inputs, time)
                        .map_err(|e| e.to_string())?,
                )))
            }
        }
    })()
    .map_err(|error| ConfirmError::Persisted {
        commit: commit.clone(),
        error,
    })?;
    Ok((
        snapshot_service::SnapshotReport {
            status: "rebuilt".into(),
            commit: Some(commit),
            seq: checkpoint.through_seq,
            epoch: fence.epoch,
            reason: None,
        },
        installed,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{init, InitOptions};
    use crate::project_session::ProjectSession as Hub;
    use crate::snapshot_service::SnapshotWriter;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .into()
    }

    fn inputs(catalog: &Catalog) -> RecoveryInputs {
        RecoveryInputs::read(&catalog.root.join("runtime-outbox"), catalog).unwrap()
    }
    fn prepare(catalog: &Catalog) -> Result<RecoveryRequest, String> {
        super::prepare(
            catalog,
            &inputs(catalog),
            1000,
            10_000,
            "0192f0aa-0000-7000-8000-000000000001",
        )
    }
    fn fixture() -> (tempfile::TempDir, Catalog, Mutex<Hub>) {
        let temporary = tempfile::tempdir().unwrap();
        git(temporary.path(), &["init", "-qb", "main"]);
        init(temporary.path(), &InitOptions { docs_dir: None }).unwrap();
        git(temporary.path(), &["add", "."]);
        git(temporary.path(), &["commit", "-qm", "fixture"]);
        git(temporary.path(), &["switch", "-qc", "recovery-fixture"]);
        let catalog = Catalog::load(temporary.path()).unwrap();
        let hub = Mutex::new(crate::test_support::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            0,
        ));
        let report = SnapshotWriter::default()
            .write(
                &hub,
                &catalog,
                "host",
                true,
                || Ok(1000),
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
        assert_eq!(report.status, "written");
        let catalog = Catalog::load(temporary.path()).unwrap();
        (temporary, catalog, hub)
    }

    type SourceStatusHook = Arc<dyn Fn() + Send + Sync>;
    fn source_status_hooks(
    ) -> &'static Mutex<std::collections::BTreeMap<std::path::PathBuf, SourceStatusHook>> {
        static HOOKS: std::sync::OnceLock<
            Mutex<std::collections::BTreeMap<std::path::PathBuf, SourceStatusHook>>,
        > = std::sync::OnceLock::new();
        HOOKS.get_or_init(Default::default)
    }
    fn set_source_status_hook(root: &Path, hook: Option<SourceStatusHook>) {
        let mut hooks = source_status_hooks().lock().unwrap();
        if let Some(hook) = hook {
            hooks.insert(root.to_path_buf(), hook);
        } else {
            hooks.remove(root);
        }
    }
    pub(super) fn after_source_status_read(root: &Path) {
        let hook = source_status_hooks().lock().unwrap().get(root).cloned();
        if let Some(hook) = hook {
            hook();
        }
    }

    #[test]
    fn source_status_cache_final_probe_rejects_same_sha_common_and_changed_inputs() {
        let (_temporary, catalog, hub) = fixture();
        drop(hub);
        let linked = catalog.root.join("cache-linked");
        git(
            &catalog.root,
            &[
                "worktree",
                "add",
                "-qb",
                "cache-context",
                linked.to_str().unwrap(),
                "main",
            ],
        );
        let bound = Catalog::load(&linked).unwrap();
        let bound_inputs = inputs(&bound);
        let foreign = catalog.root.join("cache-foreign");
        git(
            &catalog.root,
            &[
                "clone",
                "-q",
                catalog.root.to_str().unwrap(),
                foreign.to_str().unwrap(),
            ],
        );
        let original = fs::read(linked.join(".git")).unwrap();
        for cached in [false, true] {
            let cache = SourceStatusCache::default();
            if cached {
                assert!(cache
                    .status_source(&bound, &bound_inputs)
                    .unwrap()
                    .is_some());
            }
            let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let reached = observed.clone();
            let git_file = linked.join(".git");
            let replacement = format!("gitdir: {}\n", foreign.join(".git").display());
            let old_revision = bound.revision.clone();
            let directory = linked.clone();
            set_source_status_hook(
                &bound.root,
                Some(Arc::new(move || {
                    // The cache's real sole reader (or hit) has completed, before
                    // its actual final Key/input probe and installation CAS.
                    fs::write(&git_file, &replacement).unwrap();
                    assert_eq!(
                        crate::catalog::authority_revision(&directory).unwrap(),
                        old_revision
                    );
                    reached.store(true, std::sync::atomic::Ordering::SeqCst);
                })),
            );
            let result = cache.status_source(&bound, &bound_inputs);
            fs::write(linked.join(".git"), &original).unwrap();
            set_source_status_hook(&bound.root, None);
            assert!(observed.load(std::sync::atomic::Ordering::SeqCst));
            assert!(
                matches!(result, Err(ref error) if error.contains("context differs")),
                "final same-SHA actual common drift must reject both miss and cached value"
            );
            let entries = cache.entries.lock().unwrap();
            assert_eq!(entries.len(), usize::from(cached));
            if cached {
                assert_eq!(entries.values().next().unwrap().generation, 1);
            }
        }

        for mutation in ["inputs", "configuration"] {
            for cached in [false, true] {
                let (_temporary, catalog, hub) = fixture();
                drop(hub);
                let input = inputs(&catalog);
                let cache = SourceStatusCache::default();
                if cached {
                    assert!(cache.status_source(&catalog, &input).unwrap().is_some());
                }
                let root = catalog.root.clone();
                let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let observed = reached.clone();
                set_source_status_hook(
                    &catalog.root,
                    Some(Arc::new(move || {
                        if mutation == "inputs" {
                            fs::create_dir_all(root.join("runtime-outbox")).unwrap();
                            fs::write(root.join("runtime-outbox/686f7374.jsonl"), "corrupt\n")
                                .unwrap();
                        } else {
                            fs::write(
                                root.join(crate::config::CONFIG_PATH),
                                crate::config::Config::new("actual-context-drift", "docs/tower")
                                    .unwrap()
                                    .to_yaml(),
                            )
                            .unwrap();
                            git(&root, &["add", crate::config::CONFIG_PATH]);
                            git(
                                &root,
                                &[
                                    "commit",
                                    "-qm",
                                    "actual authoritative context drift after read",
                                ],
                            );
                            let revision = git(&root, &["rev-parse", "HEAD"]);
                            git(&root, &["update-ref", "refs/heads/main", &revision]);
                            assert_eq!(
                                crate::catalog::authority_revision(&root).unwrap(),
                                revision
                            );
                        }
                        observed.store(true, std::sync::atomic::Ordering::SeqCst);
                    })),
                );
                let result = cache.status_source(&catalog, &input);
                set_source_status_hook(&catalog.root, None);
                assert!(reached.load(std::sync::atomic::Ordering::SeqCst));
                assert!(
                    result.is_err(),
                    "final {mutation} drift must reject cached={cached}"
                );
                let entries = cache.entries.lock().unwrap();
                assert_eq!(entries.len(), usize::from(cached));
                if cached {
                    assert_eq!(entries.values().next().unwrap().generation, 1);
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn source_status_key_validates_actual_context_once_per_probe() {
        use std::os::unix::fs::PermissionsExt;
        const CHILD: &str = "TOWERMD_SOURCE_CONTEXT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let (_temporary, catalog, hub) = fixture();
            drop(hub);
            let gates = tempfile::tempdir().unwrap();
            let actual = Command::new("which").arg("git").output().unwrap();
            assert!(actual.status.success());
            let wrapper = gates.path().join("git");
            fs::write(&wrapper, r#"#!/bin/sh
if test "$(pwd -P)" = "$TOWERMD_SOURCE_CONTEXT_ROOT" && test -f "$TOWERMD_SOURCE_CONTEXT_GATES/config-oid"; then
    oid=$(cat "$TOWERMD_SOURCE_CONTEXT_GATES/config-oid")
    previous= before=
    for arg do
        if test "$before" = cat-file && test "$previous" = blob && test "$arg" = "$oid"; then
            printf 'actual-config-blob\n' >> "$TOWERMD_SOURCE_CONTEXT_GATES/reads"
        fi
        before=$previous previous=$arg
    done
fi
exec "$TOWERMD_SOURCE_CONTEXT_GIT" "$@"
"#).unwrap();
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "recovery::tests::source_status_key_validates_actual_context_once_per_probe",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(
                    "TOWERMD_SOURCE_CONTEXT_ROOT",
                    catalog.root.canonicalize().unwrap(),
                )
                .env("TOWERMD_SOURCE_CONTEXT_GATES", gates.path())
                .env(
                    "TOWERMD_SOURCE_CONTEXT_GIT",
                    String::from_utf8(actual.stdout).unwrap().trim(),
                )
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        gates.path().display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(
                output.status.success(),
                "actual context probe subprocess failed"
            );
            return;
        }
        let root =
            std::path::PathBuf::from(std::env::var_os("TOWERMD_SOURCE_CONTEXT_ROOT").unwrap());
        let gates =
            std::path::PathBuf::from(std::env::var_os("TOWERMD_SOURCE_CONTEXT_GATES").unwrap());
        let catalog = Catalog::load(&root).unwrap();
        let input = inputs(&catalog);
        let oid = git(
            &root,
            &[
                "rev-parse",
                &format!("{}:.towermd/config.yaml", catalog.revision),
            ],
        );
        fs::write(gates.join("config-oid"), oid).unwrap();
        let key = SourceStatusKey::probe(&catalog, &input).unwrap();
        assert_eq!(
            key.context,
            crate::recovery_outbox::RecoveryContext::extract(&catalog).unwrap()
        );
        let reads = fs::read_to_string(gates.join("reads"))
            .unwrap()
            .lines()
            .count();
        eprintln!("source-key actual authoritative config blob reads per probe={reads}");
        assert_eq!(reads, 1, "inputs.probe already validates actual common/config/context; the same source probe must not parse that config twice");

        let linked = root.join("context-linked");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-qb",
                "source-context",
                linked.to_str().unwrap(),
                "main",
            ],
        );
        let bound = Catalog::load(&linked).unwrap();
        let bound_inputs = inputs(&bound);
        let foreign = root.join("context-foreign");
        git(
            &root,
            &[
                "clone",
                "-q",
                root.to_str().unwrap(),
                foreign.to_str().unwrap(),
            ],
        );
        let original = fs::read(linked.join(".git")).unwrap();
        fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", foreign.join(".git").display()),
        )
        .unwrap();
        assert_eq!(
            crate::catalog::authority_revision(&linked).unwrap(),
            bound.revision
        );
        let rejected = SourceStatusKey::probe(&bound, &bound_inputs);
        fs::write(linked.join(".git"), original).unwrap();
        assert!(
            matches!(rejected, Err(ref error) if error.contains("context differs")),
            "same SHA must retain actual canonical common/context validation"
        );
    }

    #[test]
    fn source_status_cache_reads_actual_archive_once_and_rebuilds_without_hub() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (temporary, catalog, hub) = fixture();
        drop(hub);
        std::fs::remove_dir_all(catalog.common_dir.join("towermd")).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = reads.clone();
        crate::snapshot_service::authority_concurrency_tests::set_hook(
            catalog.root.clone(),
            Some(Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            })),
        );
        let cache = SourceStatusCache::default();
        let input = inputs(&catalog);
        let first = cache.status_source(&catalog, &input).unwrap();
        assert!(
            reads.load(Ordering::SeqCst) > 0,
            "must hit the actual archive reader"
        );
        let count = reads.load(Ordering::SeqCst);
        for _ in 0..4 {
            assert_eq!(cache.status_source(&catalog, &input).unwrap(), first);
        }
        assert_eq!(
            reads.load(Ordering::SeqCst),
            count,
            "static source status must not reread archives"
        );
        let rebuilt = SourceStatusCache::default()
            .status_source(&catalog, &input)
            .unwrap();
        assert_eq!(rebuilt, first);
        assert!(
            !catalog.common_dir.join("towermd").exists(),
            "source display must not create a Hub"
        );
        let value = first.unwrap();
        assert_eq!(value["snapshot"]["last"]["commit"], value["source_commit"]);
        assert_eq!(value["snapshot"]["last"]["epoch"], 1);
        assert!(
            value["snapshot"]["last"]["written_at"].is_null(),
            "source contains no matching written Event"
        );
        std::fs::create_dir_all(temporary.path().join("runtime-outbox")).unwrap();
        std::fs::write(
            temporary.path().join("runtime-outbox/686f7374.jsonl"),
            "corrupt\n",
        )
        .unwrap();
        assert!(
            cache.status_source(&catalog, &input).is_err(),
            "input error cannot return old cached display"
        );
        crate::snapshot_service::authority_concurrency_tests::set_hook(catalog.root.clone(), None);
    }

    #[test]
    fn source_status_cache_invalidates_private_and_valid_bound_inputs() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (_temporary, catalog, hub) = fixture();
        let cache = SourceStatusCache::default();
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = reads.clone();
        crate::snapshot_service::authority_concurrency_tests::set_hook(
            catalog.root.clone(),
            Some(Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            })),
        );
        let original = cache
            .status_source(&catalog, &inputs(&catalog))
            .unwrap()
            .unwrap();
        let first_count = reads.load(Ordering::SeqCst);
        // Occupy main in the user's worktree so this actual writer must use its
        // private ref; authority remains fixed while only private changes.
        git(&catalog.root, &["switch", "main"]);
        let next = SnapshotWriter::default()
            .write(
                &hub,
                &catalog,
                "host",
                true,
                || Ok(2000),
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
        assert_ne!(
            next.commit.as_ref().unwrap(),
            original["source_commit"].as_str().unwrap()
        );
        let refreshed = cache
            .status_source(&catalog, &inputs(&catalog))
            .unwrap()
            .unwrap();
        assert_eq!(refreshed["source_commit"], next.commit.unwrap());
        assert!(reads.load(Ordering::SeqCst) > first_count);
        drop(hub);
        std::fs::remove_dir_all(catalog.common_dir.join("towermd")).unwrap();
        let old_inputs = inputs(&catalog);
        let mut outbox = crate::recovery_outbox::RecoveryOutbox::open(
            &catalog.root.join("runtime-outbox"),
            crate::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        outbox
            .append_pending(&NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "human:test".into(),
                kind: "hold.resolved".into(),
                layer: "L4".into(),
                obj: "pending".into(),
                data: json!({}),
                guards: vec![],
            })
            .unwrap();
        assert!(cache.status_source(&catalog, &old_inputs).is_err());
        let bound = inputs(&catalog);
        let before = reads.load(Ordering::SeqCst);
        assert_eq!(
            cache.status_source(&catalog, &bound).unwrap().unwrap(),
            refreshed
        );
        let after = reads.load(Ordering::SeqCst);
        assert!(
            after > before,
            "valid changed inputs must reenter the sole archive reader"
        );
        cache.status_source(&catalog, &bound).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), after);
        assert!(!catalog.common_dir.join("towermd").exists());
        git(
            &catalog.root,
            &["update-ref", "-d", "refs/towermd/snapshot"],
        );
        assert_eq!(
            cache.status_source(&catalog, &bound).unwrap().unwrap()["source_commit"],
            original["source_commit"],
            "private loss must reselect the actual surviving main source"
        );
        // Change actual committed authority configuration, keeping the bound
        // project/docs/hub context valid. The old Catalog must be refused and a
        // fresh Catalog must miss even though the saved snapshot is unchanged.
        std::fs::write(
            catalog.root.join(crate::config::CONFIG_PATH),
            format!("{}\nsnapshot_interval: 120\n", catalog.config.to_yaml()),
        )
        .unwrap();
        git(&catalog.root, &["add", crate::config::CONFIG_PATH]);
        git(
            &catalog.root,
            &["commit", "-qm", "change actual snapshot interval"],
        );
        assert!(cache.status_source(&catalog, &bound).is_err());
        let current = Catalog::load(&catalog.root).unwrap();
        let before_config = reads.load(Ordering::SeqCst);
        let changed = cache
            .status_source(&current, &inputs(&current))
            .unwrap()
            .unwrap();
        assert_eq!(changed["source_commit"], original["source_commit"]);
        assert!(reads.load(Ordering::SeqCst) > before_config);
        let entries = cache.entries.lock().unwrap();
        let key = &entries.values().next().unwrap().key;
        assert_eq!(key.config, current.config.to_yaml());
        assert_eq!(key.authority, current.revision);
        drop(entries);
        crate::snapshot_service::authority_concurrency_tests::set_hook(catalog.root.clone(), None);
        eprintln!("source-cache actual archive reads: first={first_count} input_before={before} input_after={after}; static-hit delta=0; config miss observed");
    }

    #[test]
    fn source_status_cache_changed_source_cas_does_not_block_newer_status() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        use std::time::Duration;
        let (_temporary, catalog, hub) = fixture();
        drop(hub);
        std::fs::remove_dir_all(catalog.common_dir.join("towermd")).unwrap();
        let catalog = Arc::new(catalog);
        let cache = Arc::new(SourceStatusCache::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let block = Arc::new(AtomicBool::new(true));
        crate::snapshot_service::authority_concurrency_tests::set_hook(
            catalog.root.clone(),
            Some(Arc::new(move || {
                if block.swap(false, Ordering::SeqCst) {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }
            })),
        );
        let old_cache = cache.clone();
        let old_catalog = catalog.clone();
        let old = std::thread::spawn(move || {
            old_cache.status_source(&old_catalog, &inputs(&old_catalog))
        });
        let reached = entered_rx.recv_timeout(Duration::from_secs(3));
        if reached.is_err() {
            let _ = release_tx.send(());
            let _ = old.join();
            panic!("fixture failed to reach actual archive stage");
        }
        let tree = git(
            &catalog.root,
            &["rev-parse", &format!("{}^{{tree}}", catalog.revision)],
        );
        let next = git(
            &catalog.root,
            &[
                "commit-tree",
                &tree,
                "-p",
                &catalog.revision,
                "-m",
                "new authority code",
            ],
        );
        git(
            &catalog.root,
            &["update-ref", "refs/heads/main", &next, &catalog.revision],
        );
        let current = Arc::new(Catalog::load(&catalog.root).unwrap());
        let (done_tx, done_rx) = mpsc::channel();
        let new_cache = cache.clone();
        let new_catalog = current.clone();
        let newer = std::thread::spawn(move || {
            let result = new_cache.status_source(&new_catalog, &inputs(&new_catalog));
            done_tx.send(result).unwrap();
        });
        let result = done_rx.recv_timeout(Duration::from_secs(3));
        release_tx.send(()).unwrap();
        let old_result = old.join().unwrap();
        newer.join().unwrap();
        crate::snapshot_service::authority_concurrency_tests::set_hook(catalog.root.clone(), None);
        assert!(result
            .expect("newer status must not wait on old cache preparation")
            .unwrap()
            .is_some());
        assert!(
            old_result.is_err(),
            "old preparation must fail actual source recheck"
        );
        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries.values().next().unwrap().key.authority, next);
        drop(entries);
        assert!(
            cache.status_source(&catalog, &inputs(&catalog)).is_err(),
            "stale Catalog cannot reuse newer cached summary"
        );
        let bound_inputs = inputs(&current);
        git(
            &current.root,
            &["update-ref", "-d", "refs/heads/main", &next],
        );
        assert!(
            cache.status_source(&current, &bound_inputs).is_err(),
            "missing authority cannot return cached value"
        );
    }

    #[test]
    fn recovery_rejects_higher_fence_hidden_in_private_history() {
        let (temporary, catalog, _hub) = fixture();
        let tree = git(
            temporary.path(),
            &["rev-parse", &format!("{}^{{tree}}", catalog.revision)],
        );
        let higher = git(
            temporary.path(),
            &[
                "commit-tree",
                &tree,
                "-p",
                &catalog.revision,
                "-m",
                "higher boundary\n\nTowermd-Snapshot: 999\nTowermd-Epoch: 999",
            ],
        );
        let ordinary = git(
            temporary.path(),
            &["commit-tree", &tree, "-p", &higher, "-m", "ordinary code"],
        );
        git(
            temporary.path(),
            &["update-ref", "refs/towermd/snapshot", &ordinary],
        );
        let error =
            prepare(&catalog).expect_err("private history must protect its issued boundary");
        assert!(error.contains("更高"), "{error}");
        assert_eq!(
            git(temporary.path(), &["rev-parse", "main"]),
            catalog.revision
        );
        assert_eq!(
            git(temporary.path(), &["rev-parse", "refs/towermd/snapshot"]),
            ordinary
        );
    }

    #[test]
    fn request_only_requires_app_confirmation_and_changes_no_state() {
        let (temporary, catalog, hub) = fixture();
        let journal = fs::read(catalog.common_dir.join("towermd/journal.jsonl")).unwrap();
        let request = prepare(&catalog).unwrap();
        assert_eq!(request.status, "awaiting_app_confirmation");
        assert_eq!(request.project_id, catalog.config.project_id());
        assert_eq!(request.seq_boundary, request.source_seq + (1 << 32));
        assert!(request.epoch_boundary > 1 << 32);
        assert_eq!(request, prepare(&catalog).unwrap());
        assert_eq!(
            git(temporary.path(), &["rev-parse", "main"]),
            catalog.revision
        );
        assert_eq!(
            fs::read(catalog.common_dir.join("towermd/journal.jsonl")).unwrap(),
            journal
        );
        assert!(hub.lock().unwrap().last_seq() < request.seq_boundary);
    }

    #[test]
    fn future_trailer_after_snapshot_is_not_silently_discarded() {
        let (temporary, _catalog, _hub) = fixture();
        git(temporary.path(), &["switch", "main"]);
        git(
            temporary.path(),
            &[
                "commit",
                "--allow-empty",
                "-qm",
                "future action\n\nTowermd-Task: T-future\nTowermd-Run: R-future",
            ],
        );
        let catalog = Catalog::load(temporary.path()).unwrap();
        prepare(&catalog).unwrap();
        let saved = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
        let history = recovery_git::read_recovery_history(&catalog, &inputs(&catalog)).unwrap();
        let facts = recover_git_facts(&catalog, &saved, &history).unwrap();
        assert!(facts.iter().any(|e| e.kind == "gate.failed"));
        assert!(facts.iter().any(|e| e.kind == "hold.opened"));
        assert!(!facts.iter().any(|e| e.kind == "merge.done"));
    }

    fn private_tail_fixture(selected: bool) -> (tempfile::TempDir, Catalog, Mutex<Hub>, String) {
        let (temporary, catalog, hub) = fixture();
        git(temporary.path(), &["switch", "main"]);
        let private = SnapshotWriter::default()
            .write(
                &hub,
                &catalog,
                "host",
                true,
                || Ok(1100),
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap()
            .commit
            .unwrap();
        if !selected {
            git(temporary.path(), &["switch", "recovery-fixture"]);
            SnapshotWriter::default()
                .write(
                    &hub,
                    &catalog,
                    "host",
                    true,
                    || Ok(1200),
                    &mut crate::test_support::outbox(&catalog),
                )
                .unwrap();
        }
        let catalog = Catalog::load(temporary.path()).unwrap();
        (temporary, catalog, hub, private)
    }

    fn recovery_observation(
        root: &Path,
        catalog: &Catalog,
    ) -> (String, String, Vec<Option<Vec<u8>>>) {
        let files = [
            ".git/index",
            ".git/HEAD",
            ".towermd/config.yaml",
            "docs/tower/state/state.json",
            "user.txt",
        ];
        (
            git(root, &["show-ref"]),
            git(root, &["status", "--porcelain=v2", "-z"]),
            files
                .iter()
                .map(|path| fs::read(root.join(path)).ok())
                .chain(std::iter::once(
                    fs::read(catalog.common_dir.join("towermd/journal.jsonl")).ok(),
                ))
                .collect(),
        )
    }

    fn assert_private_unknown_tail_is_diagnosed(selected: bool) {
        let (temporary, catalog, _hub, private) = private_tail_fixture(selected);
        let tree = git(
            temporary.path(),
            &["rev-parse", &format!("{private}^{{tree}}")],
        );
        let unknown = git(
            temporary.path(),
            &[
                "commit-tree",
                &tree,
                "-p",
                &private,
                "-m",
                "future action\n\nTowermd-Future: cannot-ignore",
            ],
        );
        git(
            temporary.path(),
            &["update-ref", "refs/towermd/snapshot", &unknown],
        );
        let chosen = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
        assert_eq!(
            chosen.source == snapshot_service::SnapshotSource::Private,
            selected
        );
        assert_ne!(chosen.commit, unknown);
        fs::write(temporary.path().join("user.txt"), b"user bytes\0\n").unwrap();
        let before = recovery_observation(temporary.path(), &catalog);
        let result = prepare(&catalog);
        assert_eq!(recovery_observation(temporary.path(), &catalog), before);
        result.unwrap();
        let saved = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
        let history = recovery_git::read_recovery_history(&catalog, &inputs(&catalog)).unwrap();
        let facts = recover_git_facts(&catalog, &saved, &history).unwrap();
        assert!(facts
            .iter()
            .any(|e| e.kind == "alert.raised" && e.data.to_string().contains("cannot-ignore")));
    }

    #[test]
    fn private_unknown_tail_is_diagnosed_when_private_is_selected() {
        assert_private_unknown_tail_is_diagnosed(true);
    }

    #[test]
    fn private_unknown_tail_is_diagnosed_when_main_is_selected() {
        assert_private_unknown_tail_is_diagnosed(false);
    }

    #[test]
    fn ordinary_private_tail_is_recoverable_for_selected_and_unselected_sources() {
        for selected in [true, false] {
            let (temporary, catalog, _hub, private) = private_tail_fixture(selected);
            let tree = git(
                temporary.path(),
                &["rev-parse", &format!("{private}^{{tree}}")],
            );
            let ordinary = git(
                temporary.path(),
                &["commit-tree", &tree, "-p", &private, "-m", "ordinary code"],
            );
            git(
                temporary.path(),
                &["update-ref", "refs/towermd/snapshot", &ordinary],
            );
            let chosen = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
            let before = recovery_observation(temporary.path(), &catalog);
            let request = prepare(&catalog).unwrap();
            assert_eq!(request.source_commit, chosen.commit);
            assert_eq!(recovery_observation(temporary.path(), &catalog), before);
        }
    }

    #[test]
    fn independent_lower_private_and_equivalent_same_boundary_sources_can_prepare() {
        for independent in [true, false] {
            let (temporary, catalog, hub) = fixture();
            let main = SnapshotWriter::default()
                .write(
                    &hub,
                    &catalog,
                    "host",
                    true,
                    || Ok(1100),
                    &mut crate::test_support::outbox(&catalog),
                )
                .unwrap()
                .commit
                .unwrap();
            if independent {
                let initial = git(
                    temporary.path(),
                    &["rev-parse", &format!("{}^1", catalog.revision)],
                );
                git(
                    temporary.path(),
                    &["update-ref", "refs/heads/main", &initial],
                );
                git(temporary.path(), &["switch", "main"]);
                let initial_catalog = Catalog::load(temporary.path()).unwrap();
                let fresh = Mutex::new(crate::test_support::open(
                    &temporary.path().join("independent-hub"),
                    &initial_catalog,
                    0,
                ));
                SnapshotWriter::default()
                    .write(
                        &fresh,
                        &initial_catalog,
                        "host",
                        true,
                        || Ok(1000),
                        &mut crate::test_support::outbox(&initial_catalog),
                    )
                    .unwrap();
                git(temporary.path(), &["switch", "recovery-fixture"]);
                git(temporary.path(), &["update-ref", "refs/heads/main", &main]);
                assert!(!git(
                    temporary.path(),
                    &["rev-list", "refs/towermd/snapshot", "--not", "main"]
                )
                .is_empty());
            } else {
                git(temporary.path(), &["switch", "--detach", &main]);
                let path = temporary.path().join("docs/tower/state/state.json");
                let snapshot: crate::snapshot::Snapshot =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();
                git(temporary.path(), &["add", "docs/tower/state/state.json"]);
                git(
                    temporary.path(),
                    &[
                        "commit",
                        "-qm",
                        &format!(
                            "equivalent checkpoint\n\nTowermd-Snapshot: {}\nTowermd-Epoch: {}",
                            snapshot.through_seq, snapshot.writer_epoch
                        ),
                    ],
                );
                let private = git(temporary.path(), &["rev-parse", "HEAD"]);
                assert_ne!(private, main);
                git(
                    temporary.path(),
                    &["update-ref", "refs/towermd/snapshot", &private],
                );
            }
            let catalog = Catalog::load(temporary.path()).unwrap();
            let chosen = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
            assert_eq!(chosen.source, snapshot_service::SnapshotSource::Main);
            let before = recovery_observation(temporary.path(), &catalog);
            assert_eq!(prepare(&catalog).unwrap().source_commit, main);
            assert_eq!(recovery_observation(temporary.path(), &catalog), before);
        }
    }

    #[test]
    fn source_without_checkpoint_still_diagnoses_unknown_history() {
        let (temporary, catalog, _hub) = fixture();
        let initial = git(
            temporary.path(),
            &["rev-parse", &format!("{}^1", catalog.revision)],
        );
        git(
            temporary.path(),
            &["update-ref", "refs/heads/main", &initial],
        );
        git(temporary.path(), &["switch", "main"]);
        let initial_catalog = Catalog::load(temporary.path()).unwrap();
        let fresh = Mutex::new(crate::test_support::open(
            &temporary.path().join("independent-hub"),
            &initial_catalog,
            0,
        ));
        SnapshotWriter::default()
            .write(
                &fresh,
                &initial_catalog,
                "host",
                true,
                || Ok(1000),
                &mut crate::test_support::outbox(&initial_catalog),
            )
            .unwrap();
        let tree = git(
            temporary.path(),
            &["rev-parse", &format!("{initial}^{{tree}}")],
        );
        let unknown = git(
            temporary.path(),
            &[
                "commit-tree",
                &tree,
                "-p",
                &initial,
                "-m",
                "future action\n\nTowermd-Future: cannot-ignore",
            ],
        );
        git(
            temporary.path(),
            &["update-ref", "refs/heads/main", &unknown],
        );
        let catalog = Catalog::load(temporary.path()).unwrap();
        let before = recovery_observation(temporary.path(), &catalog);
        prepare(&catalog).unwrap();
        let saved = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
        let history = recovery_git::read_recovery_history(&catalog, &inputs(&catalog)).unwrap();
        assert!(recover_git_facts(&catalog, &saved, &history)
            .unwrap()
            .iter()
            .any(|e| e.kind == "alert.raised"));
        assert_eq!(recovery_observation(temporary.path(), &catalog), before);
    }

    // Contract fixture: a historical host writes Git while the registered local target stays empty.
    // These are different hub files; the actual target retains exactly one protocol/file owner.
    fn registered_fixture(
        private: bool,
    ) -> (tempfile::TempDir, Catalog, Arc<Mutex<ProjectSession>>) {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-qb", "main"]);
        init(temp.path(), &InitOptions { docs_dir: None }).unwrap();
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-qm", "bootstrap fixture"]);
        if !private {
            git(temp.path(), &["switch", "-qc", "worker"]);
        }
        let catalog = Catalog::load(temp.path()).unwrap();
        let target = Arc::new(Mutex::new(crate::test_support::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            0,
        )));
        let source = Mutex::new(crate::test_support::open(
            &temp.path().join("historical-host-hub"),
            &catalog,
            0,
        ));
        SnapshotWriter::default()
            .write(
                &source,
                &catalog,
                "host",
                true,
                || Ok(1000),
                &mut crate::test_support::outbox(&catalog),
            )
            .unwrap();
        drop(source);
        let catalog = Catalog::load(temp.path()).unwrap();
        (temp, catalog, target)
    }
    #[test]
    fn recovery_bootstrap_accepts_missing_or_empty_target_without_fake_session() {
        for private in [false, true] {
            for empty in [false, true] {
                let (_temp, catalog, target) = registered_fixture(private);
                drop(target);
                let directory = catalog.common_dir.join("towermd");
                fs::remove_dir_all(&directory).unwrap();
                if empty {
                    drop(crate::hub::Hub::open(&directory).unwrap());
                }
                let before = directory.exists();
                let source = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
                let archived: crate::hub_protocol::Event = source
                    .archives
                    .values()
                    .flat_map(|text| text.lines())
                    .map(|line| serde_json::from_str(line).unwrap())
                    .find(|event: &crate::hub_protocol::Event| !event.is_pure_heartbeat())
                    .unwrap();
                let input = inputs(&catalog);
                let request = prepare(&catalog).unwrap();
                assert_eq!(directory.exists(), before);
                let (report, session) = SnapshotWriter::default()
                    .confirm_rebuild(
                        None,
                        &catalog,
                        &request,
                        "human:test",
                        request.expires_at,
                        &input,
                        "host",
                        || Ok(2000),
                    )
                    .unwrap();
                assert_eq!(report.status, "rebuilt");
                assert!(report.seq > request.seq_boundary);
                assert!(report.epoch > request.epoch_boundary);
                let current = Catalog::load(&catalog.root).unwrap();
                let saved = snapshot_service::read_snapshot(&current).unwrap().unwrap();
                assert_eq!(saved.commit, report.commit.unwrap());
                if private {
                    assert_eq!(git(&catalog.root, &["rev-parse", "main"]), catalog.revision);
                    assert_eq!(
                        git(&catalog.root, &["rev-parse", snapshot_git::PRIVATE_REF]),
                        saved.commit
                    );
                } else {
                    assert_eq!(git(&catalog.root, &["rev-parse", "main"]), saved.commit);
                }
                assert!(saved
                    .archives
                    .values()
                    .flat_map(|text| text.lines())
                    .any(|line| {
                        let event: crate::hub_protocol::Event = serde_json::from_str(line).unwrap();
                        event.kind == "snapshot.written"
                            && event.data["commit"] == request.source_commit
                    }));
                assert!(session
                    .lock()
                    .unwrap()
                    .leases(2000)
                    .values()
                    .all(|lease| { lease.epoch > request.epoch_boundary }));
                assert_eq!(
                    session
                        .lock()
                        .unwrap()
                        .protocol()
                        .unwrap()
                        .append(
                            NewEvent {
                                eid: archived.eid,
                                host: "host".into(),
                                actor: "process:test".into(),
                                kind: "ignored.on.duplicate".into(),
                                layer: "L5".into(),
                                obj: "main".into(),
                                data: serde_json::Value::Null,
                                guards: vec![],
                            },
                            2000
                        )
                        .unwrap(),
                    crate::hub_protocol::AppendReceipt::Duplicate { seq: archived.seq }
                );
                assert_eq!(
                    session.lock().unwrap().snapshot_ack().unwrap().seq,
                    saved.snapshot.through_seq
                );
                let expected = session.lock().unwrap().projection().unwrap().clone();
                drop(session);
                let reopened =
                    ProjectSession::open(&directory, &current, &inputs(&current), 2000).unwrap();
                assert_eq!(*reopened.projection().unwrap(), expected);
            }
        }
    }
    struct SecondInstallWriteFault {
        inner: Box<dyn HubProtocol>,
        directory: std::path::PathBuf,
        calls: Arc<Mutex<(usize, usize)>>,
    }
    impl HubProtocol for SecondInstallWriteFault {
        fn append(
            &mut self,
            event: NewEvent,
            now: u64,
        ) -> Result<crate::hub_protocol::AppendReceipt, crate::hub_protocol::HubError> {
            self.inner.append(event, now)
        }
        fn subscribe(
            &mut self,
            after: u64,
            now: u64,
        ) -> Result<crate::hub_protocol::SubscriptionBatch, crate::hub_protocol::HubError> {
            self.inner.subscribe(after, now)
        }
        fn lease_acquire(
            &mut self,
            request: crate::hub_protocol::AcquireLease,
            now: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, crate::hub_protocol::HubError> {
            self.inner.lease_acquire(request, now)
        }
        fn lease_renew(
            &mut self,
            request: crate::hub_protocol::RenewLease,
            now: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, crate::hub_protocol::HubError> {
            self.inner.lease_renew(request, now)
        }
        fn lease_release(
            &mut self,
            request: crate::hub_protocol::ReleaseLease,
            now: u64,
        ) -> Result<crate::hub_protocol::LeaseReceipt, crate::hub_protocol::HubError> {
            self.inner.lease_release(request, now)
        }
        fn contiguous_seq(&self) -> u64 {
            self.inner.contiguous_seq()
        }
        fn snapshot_ack(
            &mut self,
            ack: crate::hub_protocol::SnapshotAck,
            checkpoint: serde_json::Value,
        ) -> Result<(), crate::hub_protocol::HubError> {
            self.calls.lock().unwrap().1 += 1;
            self.inner.snapshot_ack(ack, checkpoint)
        }
        fn rebuild(
            &mut self,
            seed: RebuildSeed,
        ) -> Result<crate::hub_protocol::RebuildReceipt, crate::hub_protocol::HubError> {
            self.calls.lock().unwrap().0 += 1;
            let receipt = self.inner.rebuild(seed)?;
            // Keep the actual bytes of the first replacement. Only a second
            // journal replacement fails; read-only protocol observations work.
            fs::rename(
                self.directory.join("journal.jsonl"),
                self.directory.join("first-install.jsonl"),
            )
            .unwrap();
            fs::create_dir(self.directory.join("journal.jsonl")).unwrap();
            Ok(receipt)
        }
    }

    #[test]
    fn target_install_has_no_second_ack_write_and_reopens_with_exact_boundary() {
        for private in [false, true] {
            let (_temporary, catalog, target) = registered_fixture(private);
            let directory = catalog.common_dir.join("towermd");
            let calls = Arc::new(Mutex::new((0, 0)));
            let unused = tempfile::tempdir().unwrap();
            {
                let mut session = target.lock().unwrap();
                let inner = std::mem::replace(
                    &mut session.hub,
                    Box::new(crate::hub::Hub::open(unused.path()).unwrap()),
                );
                session.hub = Box::new(SecondInstallWriteFault {
                    inner,
                    directory: directory.clone(),
                    calls: calls.clone(),
                });
            }
            let request = prepare(&catalog).unwrap();
            let result = SnapshotWriter::default().confirm_rebuild(
                Some(target.clone()),
                &catalog,
                &request,
                "human:test",
                request.expires_at,
                &inputs(&catalog),
                "host",
                || Ok(2000),
            );
            // Remove only the filesystem obstruction and restore the exact
            // first successful replacement, never construct or repair bytes.
            assert!(
                crate::hub::Hub::open(&directory).is_err(),
                "original owner must remain held"
            );
            fs::remove_dir(directory.join("journal.jsonl")).unwrap();
            fs::rename(
                directory.join("first-install.jsonl"),
                directory.join("journal.jsonl"),
            )
            .unwrap();
            let (report, installed) = result.unwrap_or_else(|error| {
                panic!("first durable target installation must already include exact ack: {error}")
            });
            assert_eq!(*calls.lock().unwrap(), (1, 0));
            assert!(Arc::ptr_eq(&target, &installed));
            let current = Catalog::load(&catalog.root).unwrap();
            let saved = snapshot_service::read_snapshot(&current).unwrap().unwrap();
            assert_eq!(
                saved.source == snapshot_service::SnapshotSource::Private,
                private
            );
            assert_eq!(Some(&saved.commit), report.commit.as_ref());
            assert_eq!(report.seq, saved.snapshot.through_seq);
            let expected = installed.lock().unwrap().projection().unwrap().clone();
            let tail = installed
                .lock()
                .unwrap()
                .hub
                .subscribe(report.seq, 2000)
                .unwrap();
            assert_eq!(tail.head.ack.as_ref().unwrap().commit, saved.commit);
            assert_eq!(tail.head.floor_seq, report.seq);
            assert_eq!(tail.head.ack.as_ref().unwrap().seq, report.seq);
            assert_eq!(tail.head.ack.as_ref().unwrap().epoch, report.epoch);
            drop(installed);
            drop(target);
            let mut reopened =
                ProjectSession::open(&directory, &current, &inputs(&current), 2000).unwrap();
            assert_eq!(*reopened.projection().unwrap(), expected);
            assert_eq!(reopened.hub.subscribe(report.seq, 2000).unwrap(), tail);
            let fresh = prepare(&current).unwrap();
            assert!(
                SnapshotWriter::default()
                    .confirm_rebuild(
                        None,
                        &current,
                        &fresh,
                        "human:test",
                        fresh.expires_at,
                        &inputs(&current),
                        "host",
                        || Ok(2100)
                    )
                    .is_err(),
                "completed nonempty target must never be overwritten"
            );
        }
    }

    #[test]
    fn recovery_bootstrap_reuses_registered_empty_protocol_without_reopen() {
        let (_temp, catalog, target) = registered_fixture(true);
        let request = prepare(&catalog).unwrap();
        let input = inputs(&catalog);
        let mut observed = false;
        let (_, installed) =
            SnapshotWriter::default()
                .confirm_rebuild(
                    Some(target.clone()),
                    &catalog,
                    &request,
                    "human:test",
                    request.expires_at,
                    &input,
                    "host",
                    || {
                        let mut s = target.lock().unwrap();
                        if s.projection().is_err() {
                            observed = true;
                            assert!(s.protocol().is_err());
                            assert!(s.read_status(&catalog, "host", None, 2000).is_err());
                            assert!(
                                crate::hub::Hub::open(&catalog.common_dir.join("towermd")).is_err()
                            );
                        }
                        Ok(2000)
                    },
                )
                .unwrap();
        assert!(observed);
        assert!(Arc::ptr_eq(&target, &installed));
        assert!(installed.lock().unwrap().projection().is_ok());
    }
    #[test]
    fn stale_confirmation_or_changed_inputs_never_retires_target() {
        let (_temp, catalog, target) = registered_fixture(true);
        let request = prepare(&catalog).unwrap();
        let input = inputs(&catalog);
        let writer = SnapshotWriter::default();
        for (actor, expiry, time) in [
            ("process:towermd-cli", request.expires_at, 2000),
            ("human:test", request.expires_at + 1, 2000),
            ("human:test", request.expires_at, request.expires_at),
        ] {
            assert!(writer
                .confirm_rebuild(
                    Some(target.clone()),
                    &catalog,
                    &request,
                    actor,
                    expiry,
                    &input,
                    "host",
                    || Ok(time)
                )
                .is_err());
            assert!(target.lock().unwrap().projection().is_ok());
        }
        let mut tampered = request.clone();
        tampered.request_id = "another-request".into();
        assert!(writer
            .confirm_rebuild(
                Some(target.clone()),
                &catalog,
                &tampered,
                "human:test",
                tampered.expires_at,
                &input,
                "host",
                || Ok(2000),
            )
            .is_err());
        assert!(target.lock().unwrap().projection().is_ok());
        assert_eq!(git(&catalog.root, &["rev-parse", "main"]), catalog.revision);
        let mut outbox = crate::recovery_outbox::RecoveryOutbox::open(
            &catalog.root.join("runtime-outbox"),
            crate::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        outbox
            .append_pending(&recovery_event("host", "future.audit", json!({"value":1})))
            .unwrap();
        drop(outbox);
        assert!(writer
            .confirm_rebuild(
                Some(target.clone()),
                &catalog,
                &request,
                "human:test",
                request.expires_at,
                &input,
                "host",
                || Ok(2000)
            )
            .is_err());
        assert!(target.lock().unwrap().projection().is_ok());
    }
    #[test]
    fn persisted_boundary_then_failed_install_requires_strictly_higher_request() {
        for private in [false, true] {
            let (_temp, catalog, target) = registered_fixture(private);
            let request = prepare(&catalog).unwrap();
            let input = inputs(&catalog);
            let writer = SnapshotWriter::default();
            let journal = catalog.common_dir.join("towermd/journal.jsonl");
            let backup = catalog.common_dir.join("towermd/saved.jsonl");
            let mut injected = false;
            let result = writer.confirm_rebuild(
                Some(target.clone()),
                &catalog,
                &request,
                "human:test",
                request.expires_at,
                &input,
                "host",
                || {
                    let current = Catalog::load(&catalog.root)?;
                    if !injected
                        && snapshot_service::read_snapshot(&current)?
                            .unwrap()
                            .snapshot
                            .through_seq
                            > request.seq_boundary
                    {
                        fs::rename(&journal, &backup).unwrap();
                        fs::create_dir(&journal).unwrap();
                        injected = true;
                    }
                    Ok(2000)
                },
            );
            let error = result.err().expect("real target rename failure");
            assert!(injected);
            assert!(error.contains("已持久化"), "{error}");
            assert!(target.lock().unwrap().projection().is_err());
            fs::remove_dir(&journal).unwrap();
            fs::rename(&backup, &journal).unwrap();
            let current = Catalog::load(&catalog.root).unwrap();
            let saved = snapshot_service::read_snapshot(&current).unwrap().unwrap();
            assert!(error.contains(&saved.commit));
            assert!(writer
                .confirm_rebuild(
                    Some(target.clone()),
                    &current,
                    &request,
                    "human:test",
                    request.expires_at,
                    &input,
                    "host",
                    || Ok(2000)
                )
                .is_err());
            let fresh = prepare(&current).unwrap();
            assert!(fresh.source_seq > request.seq_boundary);
            assert!(fresh.seq_boundary >= request.seq_boundary + (1 << 32));
            assert!(fresh.epoch_boundary > request.epoch_boundary);
            let (_, installed) = writer
                .confirm_rebuild(
                    Some(target.clone()),
                    &current,
                    &fresh,
                    "human:test",
                    fresh.expires_at,
                    &inputs(&current),
                    "host",
                    || Ok(2100),
                )
                .unwrap();
            assert!(Arc::ptr_eq(&target, &installed));
        }
    }
    #[test]
    fn new_target_failed_atomic_replace_reopens_empty_and_accepts_only_higher_request() {
        for private in [false, true] {
            let (_temporary, catalog, target) = registered_fixture(private);
            drop(target);
            let request = prepare(&catalog).unwrap();
            let directory = catalog.common_dir.join("towermd");
            let journal = directory.join("journal.jsonl");
            let backup = directory.join("empty-before-install.jsonl");
            let original = fs::read(&journal).unwrap();
            let mut injected = false;
            let writer = SnapshotWriter::default();
            let error = writer
                .confirm_rebuild(
                    None,
                    &catalog,
                    &request,
                    "human:test",
                    request.expires_at,
                    &inputs(&catalog),
                    "host",
                    || {
                        let current = Catalog::load(&catalog.root)?;
                        if !injected
                            && snapshot_service::read_snapshot(&current)?
                                .unwrap()
                                .snapshot
                                .through_seq
                                > request.seq_boundary
                        {
                            assert!(
                                crate::hub::Hub::open(&directory).is_err(),
                                "new target owns journal before installing"
                            );
                            fs::rename(&journal, &backup).unwrap();
                            fs::create_dir(&journal).unwrap();
                            injected = true;
                        }
                        Ok(2000)
                    },
                )
                .err()
                .expect("real first atomic replacement must fail");
            assert!(injected);
            assert!(
                error.contains("已持久化") && error.contains("Is a directory"),
                "{error}"
            );
            fs::remove_dir(&journal).unwrap();
            fs::rename(&backup, &journal).unwrap();
            assert_eq!(fs::read(&journal).unwrap(), original);
            let mut reopened = crate::hub::Hub::open(&directory).unwrap();
            let empty = reopened.subscribe(0, 2000).unwrap();
            assert_eq!(empty.head.contiguous_seq, 0);
            assert_eq!(empty.head.floor_seq, 0);
            assert_eq!(empty.head.ack, None);
            drop(reopened);
            let current = Catalog::load(&catalog.root).unwrap();
            let saved = snapshot_service::read_snapshot(&current).unwrap().unwrap();
            assert_eq!(
                saved.source == snapshot_service::SnapshotSource::Private,
                private
            );
            assert!(error.contains(&saved.commit));
            assert!(writer
                .confirm_rebuild(
                    None,
                    &current,
                    &request,
                    "human:test",
                    request.expires_at,
                    &inputs(&current),
                    "host",
                    || Ok(2000)
                )
                .is_err());
            assert_eq!(fs::read(&journal).unwrap(), original);
            let fresh = prepare(&current).unwrap();
            assert!(fresh.source_seq > request.seq_boundary);
            assert!(fresh.seq_boundary >= request.seq_boundary + (1 << 32));
            let (report, installed) = writer
                .confirm_rebuild(
                    None,
                    &current,
                    &fresh,
                    "human:test",
                    fresh.expires_at,
                    &inputs(&current),
                    "host",
                    || Ok(2100),
                )
                .unwrap();
            let expected = installed.lock().unwrap().projection().unwrap().clone();
            assert_eq!(
                installed.lock().unwrap().snapshot_ack().unwrap().commit,
                report.commit.unwrap()
            );
            drop(installed);
            let latest = Catalog::load(&catalog.root).unwrap();
            let reopened =
                ProjectSession::open(&directory, &latest, &inputs(&latest), 2100).unwrap();
            assert_eq!(*reopened.projection().unwrap(), expected);
        }
    }

    #[test]
    fn target_changes_during_recovery_are_not_overwritten() {
        let (_temp, catalog, target) = registered_fixture(true);
        let request = prepare(&catalog).unwrap();
        let mut changed = false;
        let error = SnapshotWriter::default()
            .confirm_rebuild(
                Some(target.clone()),
                &catalog,
                &request,
                "human:test",
                request.expires_at,
                &inputs(&catalog),
                "host",
                || {
                    let mut s = target.lock().unwrap();
                    if !changed && s.projection().is_err() {
                        s.hub
                            .append(
                                recovery_event("racer", "future.audit", json!({"keep":true})),
                                2000,
                            )
                            .unwrap();
                        changed = true;
                    }
                    Ok(2000)
                },
            )
            .err()
            .unwrap();
        assert!(changed);
        assert!(error.contains("已持久化"), "{error}");
        let s = target.lock().unwrap();
        assert_eq!(s.hub.contiguous_seq(), 1);
        assert!(s.projection().is_err());
    }
    #[test]
    fn expired_confirmation_before_final_git_write_leaves_target_inactive() {
        let (_temp, catalog, target) = registered_fixture(true);
        let input = inputs(&catalog);
        let request = super::prepare(
            &catalog,
            &input,
            1000,
            1005,
            "0192f0aa-0000-7000-8000-000000000001",
        )
        .unwrap();
        let before = git(&catalog.root, &["show-ref"]);
        let mut time = 999;
        assert!(SnapshotWriter::default()
            .confirm_rebuild(
                Some(target.clone()),
                &catalog,
                &request,
                "human:test",
                request.expires_at,
                &input,
                "host",
                || {
                    time += 1;
                    Ok(time)
                }
            )
            .is_err());
        assert_eq!(git(&catalog.root, &["show-ref"]), before);
        assert!(target.lock().unwrap().projection().is_err());
    }
    #[test]
    fn expiry_advanced_during_final_actual_proof_never_writes_or_installs() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::mpsc;
        use std::time::Duration;
        let (_temp, catalog, target) = registered_fixture(true);
        let request = prepare(&catalog).unwrap();
        let input = inputs(&catalog);
        let before = git(&catalog.root, &["show-ref"]);
        let armed = Arc::new(AtomicBool::new(false));
        let arm = armed.clone();
        snapshot_git::tests::set_hook(
            catalog.root.clone(),
            Some(Arc::new(move |args| {
                if args.first() == Some(&"before-local-cas-validation") {
                    arm.store(true, Ordering::SeqCst);
                }
            })),
        );
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let once = AtomicBool::new(false);
        snapshot_service::authority_concurrency_tests::set_hook(
            catalog.root.clone(),
            Some(Arc::new(move || {
                if armed.load(Ordering::SeqCst) && !once.swap(true, Ordering::SeqCst) {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(30))
                        .unwrap();
                }
            })),
        );
        let clock = Arc::new(AtomicU64::new(2000));
        let current_clock = clock.clone();
        let worker_target = target.clone();
        let worker_catalog = Catalog::load(&catalog.root).unwrap();
        let expires_at = request.expires_at;
        let worker = std::thread::spawn(move || {
            SnapshotWriter::default().confirm_rebuild_classified(
                Some(worker_target),
                &worker_catalog,
                &request,
                "human:test",
                expires_at,
                &input,
                "host",
                || {
                    let current = current_clock.load(Ordering::SeqCst);
                    eprintln!("recovery confirmation sampled now={current}");
                    Ok(current)
                },
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(target.try_lock().unwrap().projection().is_err());
        assert!(crate::hub::Hub::open(&catalog.common_dir.join("towermd")).is_err());
        assert_eq!(git(&catalog.root, &["show-ref"]), before);
        eprintln!("final actual archive proof blocked; advancing now to expiry={expires_at}");
        clock.store(expires_at, Ordering::SeqCst);
        release_tx.send(()).unwrap();
        let result = worker.join().unwrap();
        snapshot_git::tests::set_hook(catalog.root.clone(), None);
        snapshot_service::authority_concurrency_tests::set_hook(catalog.root.clone(), None);
        let after = git(&catalog.root, &["show-ref"]);
        eprintln!(
            "final proof expiry result: {:?}; refs before={before:?}; after={after:?}",
            result.as_ref().map(|(report, _)| report)
        );
        assert!(
            result.is_err(),
            "authorization that expires during actual final proof must fail before Git write"
        );
        let error = result.err().unwrap();
        assert!(matches!(&error, ConfirmError::Conflict(_)), "{error}");
        assert!(error.to_string().contains("未过期"));
        assert_eq!(after, before);
        let mut retired = target.lock().unwrap();
        assert!(retired.projection().is_err());
        assert!(retired.protocol().is_err());
        assert_eq!(retired.hub.contiguous_seq(), 0);
        assert!(retired
            .hub
            .subscribe(0, expires_at)
            .unwrap()
            .events
            .is_empty());
    }

    #[test]
    fn recovery_rejects_corrupt_and_nonempty_targets_without_overwriting() {
        for kind in ["nonempty", "corrupt", "partial"] {
            let (_temp, catalog, target) = registered_fixture(true);
            drop(target);
            let directory = catalog.common_dir.join("towermd");
            if kind == "corrupt" {
                fs::write(directory.join("journal.jsonl"), b"corrupt original\n").unwrap();
            } else if kind == "partial" {
                let path = directory.join("journal.jsonl");
                let mut bytes = fs::read(&path).unwrap();
                bytes.extend(br#"{"partial":"#);
                fs::write(path, bytes).unwrap();
            } else {
                let mut raw = crate::hub::Hub::open(&directory).unwrap();
                raw.append(recovery_event("host", "future.audit", json!({})), 1000)
                    .unwrap();
            }
            let before = fs::read(directory.join("journal.jsonl")).unwrap();
            let refs = git(&catalog.root, &["show-ref"]);
            let request = prepare(&catalog).unwrap();
            assert!(SnapshotWriter::default()
                .confirm_rebuild(
                    None,
                    &catalog,
                    &request,
                    "human:test",
                    request.expires_at,
                    &inputs(&catalog),
                    "host",
                    || Ok(2000)
                )
                .is_err());
            assert_eq!(fs::read(directory.join("journal.jsonl")).unwrap(), before);
            assert_eq!(git(&catalog.root, &["show-ref"]), refs);
        }
    }

    #[cfg(unix)]
    fn rebuild_fetch_probe(test_name: &str, concurrent_write: bool) {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        let Ok(gates) = std::env::var("TOWERMD_RECOVERY_FETCH_GATES") else {
            let wrapper = tempfile::tempdir().unwrap();
            let original_git = Command::new("which").arg("git").output().unwrap();
            assert!(original_git.status.success());
            let original_git = String::from_utf8(original_git.stdout).unwrap();
            let program = wrapper.path().join("git");
            fs::write(
                &program,
                r#"#!/bin/sh
for argument in "$@"; do
    if test "$argument" = fetch; then
        gate="$TOWERMD_RECOVERY_FETCH_GATES/1"
        if test -f "$gate.entered"; then gate="$TOWERMD_RECOVERY_FETCH_GATES/2"; fi
        touch "$gate.entered"
        attempts=0
        while ! test -f "$gate.release"; do
            attempts=$((attempts + 1))
            if test "$attempts" -ge 200; then exit 99; fi
            sleep 0.05
        done
        break
    fi
done
exec "$TOWERMD_RECOVERY_REAL_GIT" "$@"
"#,
            )
            .unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &format!("recovery::tests::{test_name}"),
                    "--nocapture",
                ])
                .env("TOWERMD_RECOVERY_FETCH_GATES", wrapper.path())
                .env("TOWERMD_RECOVERY_REAL_GIT", original_git.trim())
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        wrapper.path().display(),
                        std::env::var("PATH").unwrap()
                    ),
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let (temporary, catalog, hub) = registered_fixture(false);
        let input = inputs(&catalog);
        let request = prepare(&catalog).unwrap();
        // Open before the remote/fetch fault: a Busy writer must not read authority.
        let mut busy_outbox = crate::test_support::outbox(&catalog);
        let remote = tempfile::tempdir().unwrap();
        git(
            temporary.path(),
            &["clone", "--bare", ".", remote.path().to_str().unwrap()],
        );
        git(
            temporary.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        let target_dir = catalog.common_dir.join("towermd");
        let writer = SnapshotWriter::default();

        let mut available = Vec::new();
        let mut writing_busy = Vec::new();
        let mut journal = None;
        let event = recovery_event("other-host", "other-project.tick", json!({"kept":true}));
        let result = std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                writer.confirm_rebuild(
                    Some(hub.clone()),
                    &catalog,
                    &request,
                    "human:test",
                    request.expires_at,
                    &input,
                    "host",
                    || Ok(2000),
                )
            });
            for phase in 1..=2 {
                let gate = Path::new(&gates).join(phase.to_string());
                let started = Instant::now();
                while !gate.with_extension("entered").exists() {
                    assert!(
                        started.elapsed() < Duration::from_secs(5),
                        "fetch {phase} never started"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                match hub.try_lock() {
                    Ok(mut target) => {
                        available.push(true);
                        if concurrent_write && phase == 2 {
                            target.hub.append(event.clone(), 2000).unwrap();
                            assert!(target.read_status(&catalog, "host", None, 2000).is_err());
                            journal = Some(fs::read(target_dir.join("journal.jsonl")).unwrap());
                        }
                    }
                    Err(_) => available.push(false),
                }
                writing_busy.push(
                    writer
                        .confirm_rebuild(
                            Some(hub.clone()),
                            &catalog,
                            &request,
                            "human:test",
                            request.expires_at,
                            &input,
                            "host",
                            || Ok(2000),
                        )
                        .is_err()
                        && writer
                            .write(&hub, &catalog, "host", true, || Ok(2000), &mut busy_outbox)
                            .unwrap()
                            .status
                            == "deferred",
                );
                fs::write(gate.with_extension("release"), "").unwrap();
            }
            worker.join().unwrap()
        });
        assert_eq!(
            available,
            [true, true],
            "Catalog/write_snapshot fetch held hub mutex"
        );
        assert_eq!(
            writing_busy,
            [true, true],
            "single-writer lock must span both fetches"
        );
        crate::origin::refresh_origin_main(temporary.path()).unwrap();
        let current = Catalog::load(temporary.path()).unwrap();
        if concurrent_write {
            let error = result.err().unwrap();
            assert!(
                error.contains(&current.revision)
                    && error.contains("已持久化")
                    && error.contains("仍有可读历史"),
                "{error}"
            );
            assert_ne!(current.revision, request.main_revision);
            let next = prepare(&current).unwrap();
            assert!(next.seq_boundary > request.seq_boundary);
            assert!(next.epoch_boundary > request.epoch_boundary);
            assert_eq!(
                fs::read(target_dir.join("journal.jsonl")).unwrap(),
                journal.unwrap()
            );
            let mut target = hub.lock().unwrap();
            assert_eq!(target.hub.contiguous_seq(), 1);
            assert_eq!(
                target.hub.subscribe(0, 2000).unwrap().events[0].eid,
                event.eid
            );
            assert!(target.projection().is_err());
            assert!(target.base_snapshot().is_none());
            assert!(target.snapshot_ack().is_none());
            assert!(target
                .verify_owned(&current, &inputs(&current), 2000)
                .is_err());
            drop(target);
            drop(hub);
            let mut reopened = crate::hub::Hub::open(&target_dir).unwrap();
            assert_eq!(
                reopened.subscribe(0, 2000).unwrap().events[0].eid,
                event.eid
            );
        } else {
            let (report, installed) = result.unwrap();
            assert!(Arc::ptr_eq(&hub, &installed));
            assert_eq!(report.status, "rebuilt");
            assert_eq!(report.commit.as_deref(), Some(current.revision.as_str()));
            ProjectSession::sync_shared(&hub, &current, &inputs(&current), 2000).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn rebuild_fetch_releases_hub_but_keeps_single_writer() {
        rebuild_fetch_probe("rebuild_fetch_releases_hub_but_keeps_single_writer", false);
    }

    #[cfg(unix)]
    #[test]
    fn rebuild_fetch_concurrent_write_preserves_target_without_seed() {
        rebuild_fetch_probe(
            "rebuild_fetch_concurrent_write_preserves_target_without_seed",
            true,
        );
    }

    #[test]
    fn recovered_running_task_is_ready_and_projection_survives_next_snapshot() {
        let (temporary, catalog, original) = fixture();
        git(temporary.path(), &["switch", "main"]);
        fs::write(
            temporary.path().join("docs/tower/arch/A.md"),
            "---\ntype: Module\nid: A\npaths: [src/**]\n---\n",
        )
        .unwrap();
        fs::write(
            temporary.path().join("docs/tower/specs/AC.md"),
            "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        git(temporary.path(), &["add", "."]);
        git(temporary.path(), &["commit", "-qm", "context"]);
        git(temporary.path(), &["switch", "-qc", "t/T-1"]);
        let catalog = Catalog::load(&catalog.root).unwrap();
        use crate::tasks::{self, TaskCommand, TaskSpec};
        let mut outbox = crate::test_support::outbox(&catalog);
        let task = TaskSpec {
            id: "T-1".into(),
            title: "task".into(),
            module: "A".into(),
            touches: vec![],
            allowed_paths: vec!["src/**".into()],
            depends_on: vec![],
            acceptance: vec!["AC".into()],
            decisions: vec![],
        };
        tasks::execute(
            &original,
            &catalog,
            &outbox.read_inputs().unwrap(),
            "host",
            TaskCommand::Add { task: task.clone() },
            None,
            1100,
            &mut outbox,
        )
        .unwrap();
        let mut other = task;
        other.id = "T-2".into();
        tasks::execute(
            &original,
            &catalog,
            &outbox.read_inputs().unwrap(),
            "host",
            TaskCommand::Add { task: other },
            None,
            1100,
            &mut outbox,
        )
        .unwrap();
        tasks::execute(
            &original,
            &catalog,
            &outbox.read_inputs().unwrap(),
            "host",
            TaskCommand::Claim {
                id: "T-1".into(),
                harness: None,
                harness_version: None,
            },
            None,
            1100,
            &mut outbox,
        )
        .unwrap();
        SnapshotWriter::default()
            .write(&original, &catalog, "host", true, || Ok(1100), &mut outbox)
            .unwrap();
        let catalog = Catalog::load(&catalog.root).unwrap();
        let request = super::prepare(
            &catalog,
            &outbox.read_inputs().unwrap(),
            1000,
            10_000,
            "0192f0aa-0000-7000-8000-000000000001",
        )
        .unwrap();
        drop(original);
        fs::remove_dir_all(catalog.common_dir.join("towermd")).unwrap();
        let (_, target) = SnapshotWriter::default()
            .confirm_rebuild(
                None,
                &catalog,
                &request,
                "human:test",
                request.expires_at,
                &outbox.read_inputs().unwrap(),
                "host",
                || Ok(2000),
            )
            .unwrap();
        let catalog = Catalog::load(&catalog.root).unwrap();
        let state = snapshot_service::read_snapshot(&catalog)
            .unwrap()
            .unwrap()
            .snapshot;
        assert_eq!(state.projection.objects["Task"]["T-1"]["state"], "ready");
        assert!(state.projection.objects["Run"]
            .values()
            .all(|run| run["state"] == "released"));
        let status = tasks::execute(
            &target,
            &catalog,
            &outbox.read_inputs().unwrap(),
            "host",
            TaskCommand::Status,
            None,
            2000,
            &mut outbox,
        )
        .unwrap();
        assert_eq!(status["tasks"].as_array().unwrap().len(), 2);
        assert!(status["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "T-1" && task["state"] == "ready"));
        SnapshotWriter::default()
            .write(&target, &catalog, "host", true, || Ok(2100), &mut outbox)
            .unwrap();
        let catalog = Catalog::load(&catalog.root).unwrap();
        ProjectSession::sync_shared(&target, &catalog, &outbox.read_inputs().unwrap(), 2100)
            .unwrap();
    }

    #[test]
    fn checked_main_recovers_from_private_and_preserves_user_files() {
        for dirty in [false, true] {
            let (_temporary, catalog, target) = registered_fixture(true);
            let request = prepare(&catalog).unwrap();
            let path = catalog
                .root
                .join(catalog.config.docs_dir())
                .join("state/state.json");
            if dirty {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, b"user data").unwrap();
            }
            let before = fs::read(&path).ok();
            let index = fs::read(catalog.common_dir.join("index")).unwrap();
            let (report, installed) = SnapshotWriter::default()
                .confirm_rebuild(
                    Some(target.clone()),
                    &catalog,
                    &request,
                    "human:test",
                    request.expires_at,
                    &inputs(&catalog),
                    "host",
                    || Ok(2000),
                )
                .unwrap();
            assert!(Arc::ptr_eq(&target, &installed));
            assert!(report.seq > request.seq_boundary);
            assert_eq!(git(&catalog.root, &["rev-parse", "main"]), catalog.revision);
            assert_eq!(
                git(&catalog.root, &["rev-parse", "refs/towermd/snapshot"]),
                report.commit.unwrap()
            );
            assert_eq!(fs::read(&path).ok(), before);
            assert_eq!(fs::read(catalog.common_dir.join("index")).unwrap(), index);
            ProjectSession::sync_shared(&target, &catalog, &inputs(&catalog), 2000).unwrap();
        }
    }
    #[test]
    fn rebuild_rejects_nonempty_or_poisoned_target_before_fetch() {
        for poisoned in [false, true] {
            let (_temporary, catalog, target) = registered_fixture(false);
            let request = prepare(&catalog).unwrap();
            let before = git(&catalog.root, &["show-ref"]);
            let input = inputs(&catalog);
            git(
                &catalog.root,
                &["remote", "add", "origin", "missing-remote"],
            );
            if poisoned {
                assert!(std::panic::catch_unwind(|| {
                    let _guard = target.lock().unwrap();
                    panic!("poison before fetch");
                })
                .is_err());
            } else {
                target
                    .lock()
                    .unwrap()
                    .hub
                    .append(recovery_event("other", "future.audit", json!({})), 1000)
                    .unwrap();
            }
            let error = SnapshotWriter::default()
                .confirm_rebuild(
                    Some(target.clone()),
                    &catalog,
                    &request,
                    "human:test",
                    request.expires_at,
                    &input,
                    "host",
                    || Ok(2000),
                )
                .err()
                .unwrap();
            assert!(
                if poisoned {
                    error.contains("hub 锁损坏")
                } else {
                    error.contains("nonempty")
                },
                "{error}"
            );
            assert_eq!(git(&catalog.root, &["show-ref"]), before);
            let guard = match target.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            assert_eq!(guard.hub.contiguous_seq(), if poisoned { 0 } else { 1 });
            assert!(guard.projection().is_ok());
        }
    }
    #[test]
    fn rebuild_poisoned_target_after_git_reports_persisted_boundary() {
        let (_temporary, catalog, target) = registered_fixture(false);
        let request = prepare(&catalog).unwrap();
        let mut poisoned = false;
        let error = SnapshotWriter::default()
            .confirm_rebuild(
                Some(target.clone()),
                &catalog,
                &request,
                "human:test",
                request.expires_at,
                &inputs(&catalog),
                "host",
                || {
                    if !poisoned && git(&catalog.root, &["rev-parse", "main"]) != catalog.revision {
                        assert!(std::panic::catch_unwind(|| {
                            let _guard = target.try_lock().unwrap();
                            panic!("poison after Git boundary");
                        })
                        .is_err());
                        poisoned = true;
                    }
                    Ok(2000)
                },
            )
            .err()
            .unwrap();
        let current = Catalog::load(&catalog.root).unwrap();
        assert!(poisoned);
        assert_ne!(current.revision, catalog.revision);
        assert!(
            error.contains(&current.revision)
                && error.contains("已持久化")
                && error.contains("hub 锁损坏"),
            "{error}"
        );
        let guard = target.lock().err().unwrap().into_inner();
        assert_eq!(guard.hub.contiguous_seq(), 0);
        assert!(guard.projection().is_err());
    }

    #[test]
    fn unknown_trailers_aggregate_one_alert_per_commit_with_raw_lines() {
        let (temporary, _catalog, _hub) = fixture();
        git(temporary.path(), &["switch", "main"]);
        let mut commits = vec![];
        for n in [1, 2] {
            git(
                temporary.path(),
                &[
                    "commit",
                    "--allow-empty",
                    "-qm",
                    &format!("future {n}\n\nTowermd-Future: raw {n}\nTowermd-Other: exact value"),
                ],
            );
            commits.push(git(temporary.path(), &["rev-parse", "HEAD"]));
        }
        let catalog = Catalog::load(temporary.path()).unwrap();
        let saved = snapshot_service::read_snapshot(&catalog).unwrap().unwrap();
        let history = recovery_git::read_recovery_history(&catalog, &inputs(&catalog)).unwrap();
        let facts = recover_git_facts(&catalog, &saved, &history).unwrap();
        let alerts: Vec<_> = facts.iter().filter(|e| e.kind == "alert.raised").collect();
        assert_eq!(alerts.len(), 2);
        for (n, (commit, alert)) in commits.iter().zip(alerts).enumerate() {
            assert_eq!(
                alert.eid,
                crate::events::derived_eid(commit, "alert.raised").unwrap()
            );
            assert_eq!(alert.data["commit"], *commit);
            assert_eq!(
                alert.data["raw_lines"],
                json!([
                    format!("Towermd-Future: raw {}", n + 1),
                    "Towermd-Other: exact value"
                ])
            );
        }
    }
    #[test]
    fn git_boundary_failure_never_installs_recovery() {
        for private in [false, true] {
            let (_temp, catalog, target) = registered_fixture(private);
            let request = prepare(&catalog).unwrap();
            let before = git(&catalog.root, &["show-ref"]);
            let journal = catalog.common_dir.join("towermd/journal.jsonl");
            let bytes = fs::read(&journal).unwrap();
            let lock = catalog.common_dir.join(if private {
                "refs/towermd/snapshot.lock"
            } else {
                "refs/heads/main.lock"
            });
            fs::write(&lock, "owned test ref lock").unwrap();
            let result = SnapshotWriter::default().confirm_rebuild(
                Some(target.clone()),
                &catalog,
                &request,
                "human:fixture",
                request.expires_at,
                &inputs(&catalog),
                "host",
                || Ok(2000),
            );
            let error = result
                .err()
                .expect("actual Git ref lock must prevent boundary write");
            assert!(
                error.contains("lock") || error.contains("边界未写入"),
                "{error}"
            );
            assert!(!error.contains("已持久化"), "{error}");
            assert_eq!(git(&catalog.root, &["show-ref"]), before);
            assert_eq!(fs::read(&journal).unwrap(), bytes);
            assert!(target.lock().unwrap().projection().is_err());
            assert_eq!(target.lock().unwrap().hub.contiguous_seq(), 0);
            fs::remove_file(lock).unwrap();
        }
    }

    #[test]
    fn recovery_reopens_without_unique_staging_state() {
        for private in [false, true] {
            let (_temp, catalog, target) = registered_fixture(private);
            let request = prepare(&catalog).unwrap();
            let (_, installed) = SnapshotWriter::default()
                .confirm_rebuild(
                    Some(target.clone()),
                    &catalog,
                    &request,
                    "human:fixture",
                    request.expires_at,
                    &inputs(&catalog),
                    "host",
                    || Ok(2000),
                )
                .unwrap();
            let expected = installed
                .lock()
                .unwrap()
                .projection()
                .unwrap()
                .objects
                .clone();
            drop(installed);
            drop(target);
            let directory = catalog.common_dir.join("towermd");
            fs::remove_dir_all(&directory).unwrap();
            fs::remove_dir_all(catalog.root.join("historical-host-hub")).unwrap();
            // confirm's private tempfile has already been dropped; no old target/source remains.
            let current = Catalog::load(&catalog.root).unwrap();
            let fresh = prepare(&current).unwrap();
            assert!(fresh.source_seq > request.seq_boundary);
            let (_, installed) = SnapshotWriter::default()
                .confirm_rebuild(
                    None,
                    &current,
                    &fresh,
                    "human:fixture",
                    fresh.expires_at,
                    &inputs(&current),
                    "host",
                    || Ok(2100),
                )
                .unwrap();
            assert_eq!(
                installed.lock().unwrap().projection().unwrap().objects,
                expected
            );
            drop(installed);
            let current = Catalog::load(&catalog.root).unwrap();
            let reopened =
                ProjectSession::open(&directory, &current, &inputs(&current), 2100).unwrap();
            assert_eq!(reopened.projection().unwrap().objects, expected);
        }
    }
    #[test]
    fn recovery_control_actor_is_rejected_before_target_changes() {
        let mut observations = vec![];
        for registered in [false, true] {
            for actor in ["human:\u{0}", "human:\u{7f}"] {
                let (_temp, catalog, target) = registered_fixture(true);
                let directory = catalog.common_dir.join("towermd");
                let original = if registered {
                    Some(target)
                } else {
                    drop(target);
                    fs::remove_dir_all(&directory).unwrap();
                    None
                };
                let request = prepare(&catalog).unwrap();
                let request_before = serde_json::to_vec(&request).unwrap();
                let journal_before = fs::read(directory.join("journal.jsonl")).ok();
                let refs_before = git(&catalog.root, &["show-ref"]);
                #[cfg(unix)]
                let lock_before = {
                    use std::os::unix::fs::MetadataExt;
                    fs::metadata(directory.join("hub.lock"))
                        .ok()
                        .map(|m| (m.dev(), m.ino()))
                };
                let writer = SnapshotWriter::default();
                let result = writer.confirm_rebuild(
                    original.clone(),
                    &catalog,
                    &request,
                    actor,
                    request.expires_at,
                    &inputs(&catalog),
                    "host",
                    || Ok(2000),
                );
                let active_after = original
                    .as_ref()
                    .map(|s| s.lock().unwrap().projection().is_ok());
                let journal_after = fs::read(directory.join("journal.jsonl")).ok();
                let observation = json!({"registered":registered,"actor":actor,"rejected":result.is_err(),
                    "error":result.err(),"active_after":active_after,
                    "target_exists_after":directory.exists(),"journal_before":journal_before,"journal_after":journal_after,
                    "bytes_equal":journal_before==journal_after,"refs_equal":refs_before==git(&catalog.root,&["show-ref"]),
                    "request_equal":request_before==serde_json::to_vec(&request).unwrap()});
                eprintln!("CORE_CONTROL_ACTOR_OBSERVATION={observation}");
                observations.push(observation);
                if registered {
                    assert!(
                        crate::hub::Hub::open(&directory).is_err(),
                        "original Hub file owner must stay held"
                    );
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let lock_after = fs::metadata(directory.join("hub.lock"))
                        .ok()
                        .map(|m| (m.dev(), m.ino()));
                    if registered {
                        assert_eq!(lock_after, lock_before);
                    }
                }
                // Positive control exercises the actual core bootstrap and the same registered Arc.
                if actor == "human:\u{0}" {
                    let (_, installed) = writer
                        .confirm_rebuild(
                            original.clone(),
                            &catalog,
                            &request,
                            "human:fixture",
                            request.expires_at,
                            &inputs(&catalog),
                            "host",
                            || Ok(2000),
                        )
                        .unwrap();
                    if let Some(original) = &original {
                        assert!(Arc::ptr_eq(original, &installed));
                    }
                    assert!(installed.lock().unwrap().projection().is_ok());
                }
            }
        }
        assert!(
            observations.iter().all(|o| o["rejected"] == true
                && o["bytes_equal"] == true
                && o["refs_equal"] == true
                && o["request_equal"] == true
                && (o["registered"] == false && o["target_exists_after"] == false
                    || o["registered"] == true && o["active_after"] == true)),
            "control actor must be rejected before opening or retiring target: {observations:?}"
        );
    }
    #[test]
    fn recovery_source_disappearance_rejects_before_target_creation() {
        for reference in ["refs/towermd/snapshot", "refs/heads/main"] {
            let (_temp, catalog, target) = registered_fixture(true);
            let directory = catalog.common_dir.join("towermd");
            let request = prepare(&catalog).unwrap();
            assert!(check_request_sources(&catalog.root, &request).is_ok());
            let mut malformed = request.clone();
            malformed.source_reference = "refs/towermd/snapshot:missing".into();
            assert!(matches!(
                check_request_sources(&catalog.root, &malformed),
                Err(RequestSourceError::Read(_))
            ));
            let inputs = inputs(&catalog);
            drop(target);
            fs::remove_dir_all(&directory).unwrap();
            git(&catalog.root, &["update-ref", "-d", reference]);
            let refs = git(&catalog.root, &["show-ref"]);
            let error = SnapshotWriter::default()
                .confirm_rebuild(
                    None,
                    &catalog,
                    &request,
                    "human:fixture",
                    request.expires_at,
                    &inputs,
                    "host",
                    || Ok(2000),
                )
                .err()
                .expect("source conflict");
            eprintln!("CORE_SOURCE_DISAPPEARED={reference:?} error={error:?}");
            assert!(!directory.exists());
            assert_eq!(refs, git(&catalog.root, &["show-ref"]));
            assert!(error.contains("已变化"), "{reference}: {error}");
        }
    }
}
