//! Pure consumers of actual Git and project-bound durable history.
//! Producer execution, business application and Session installation belong to E1b/P2/P3/P4.
use crate::catalog::Catalog;
use crate::hub_protocol::{Event, NewEvent};
use crate::recovery_contract::{self as contract, finish::*, recorded::*, verification::*};
use crate::recovery_outbox::RecoveryInputs;
use crate::snapshot_service;
use crate::types::{layer_of, LayerResolution};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskTrailers {
    pub task: String,
    pub run: String,
    pub module: String,
    pub evidence: String,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationTrailers {
    pub baseline: String,
    pub approval: String,
    pub publication: String,
    pub content_hash: String,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrailerContext {
    Task(TaskTrailers),
    Snapshot { seq: u64, epoch: u64 },
    Publication(PublicationTrailers),
    None,
}
pub fn parse_trailer_context(message: &str) -> Result<TrailerContext, String> {
    let fields = trailer_fields(message)?;
    if fields.is_empty() {
        return Ok(TrailerContext::None);
    }
    let get = |key: &str| {
        fields
            .get(key)
            .cloned()
            .ok_or_else(|| format!("missing Towermd-{key}"))
    };
    let number = |key: &str| -> Result<u64, String> {
        let text = get(key)?;
        let value: u64 = text.parse().map_err(|_| format!("invalid Towermd-{key}"))?;
        if value == 0 || value.to_string() != text {
            return Err(format!("invalid Towermd-{key}"));
        }
        Ok(value)
    };
    if fields.contains_key("Snapshot") {
        if fields.keys().any(|k| k != "Snapshot" && k != "Epoch") {
            return Err("mixed trailer contexts".into());
        }
        return Ok(TrailerContext::Snapshot {
            seq: number("Snapshot")?,
            epoch: number("Epoch")?,
        });
    }
    if fields.contains_key("Task") {
        if fields
            .keys()
            .any(|k| !["Task", "Run", "Module", "Evidence", "Epoch"].contains(&k.as_str()))
        {
            return Err("mixed trailer contexts".into());
        }
        return Ok(TrailerContext::Task(TaskTrailers {
            task: get("Task")?,
            run: get("Run")?,
            module: get("Module")?,
            evidence: get("Evidence")?,
            epoch: number("Epoch")?,
        }));
    }
    if fields.contains_key("Baseline") {
        if fields.keys().any(|k| {
            ![
                "Baseline",
                "Approval",
                "Publication",
                "Content-Hash",
                "Epoch",
            ]
            .contains(&k.as_str())
        }) {
            return Err("mixed trailer contexts".into());
        }
        let content_hash = get("Content-Hash")?;
        contract::sha256_hex(&content_hash)?;
        return Ok(TrailerContext::Publication(PublicationTrailers {
            baseline: get("Baseline")?,
            approval: get("Approval")?,
            publication: get("Publication")?,
            content_hash,
            epoch: number("Epoch")?,
        }));
    }
    Err("orphan companion Towermd trailer".into())
}
const KNOWN_TRAILERS: &[&str] = &[
    "Task",
    "Run",
    "Module",
    "Evidence",
    "Snapshot",
    "Epoch",
    "Baseline",
    "Approval",
    "Publication",
    "Content-Hash",
];
/// Unknown footer lines remain diagnostics; they never confer a known Git fact.
pub fn unknown_trailer_lines(message: &str) -> Vec<String> {
    trailer_block(message)
        .lines()
        .filter(|line| {
            line.strip_prefix("Towermd-").is_some_and(|tail| {
                !KNOWN_TRAILERS.contains(&tail.split(':').next().unwrap_or(tail))
            })
        })
        .map(String::from)
        .collect()
}
fn trailer_block(message: &str) -> &str {
    let text = message.trim_end();
    let mut offset = 0;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        if line.trim().is_empty() {
            start = offset;
        }
    }
    &text[start..]
}
pub(crate) fn towermd_trailer_lines(message: &str) -> Vec<String> {
    trailer_block(message)
        .lines()
        .filter(|line| line.starts_with("Towermd-"))
        .map(String::from)
        .collect()
}
pub(crate) fn has_snapshot_trailer(message: &str) -> bool {
    trailer_block(message).lines().any(|line| {
        line.split(':')
            .next()
            .is_some_and(|key| key.trim() == "Towermd-Snapshot")
    })
}
fn trailer_fields(message: &str) -> Result<BTreeMap<String, String>, String> {
    let block = trailer_block(message);
    let mut values = BTreeMap::new();
    for line in block.lines() {
        if let Some(line) = line.strip_prefix("Towermd-") {
            let (key, value) = line.split_once(':').ok_or("invalid Towermd trailer")?;
            if !KNOWN_TRAILERS.contains(&key) {
                continue;
            }
            let value = value.trim();
            if value.is_empty()
                || value.chars().any(char::is_whitespace)
                || values.insert(key.into(), value.into()).is_some()
            {
                return Err(format!("invalid/duplicate Towermd-{key}"));
            }
        }
    }
    Ok(values)
}

pub fn render_task_trailers(value: &TaskTrailers) -> Result<String, String> {
    let text=format!("Towermd-Task: {}\nTowermd-Run: {}\nTowermd-Module: {}\nTowermd-Evidence: {}\nTowermd-Epoch: {}\n",value.task,value.run,value.module,value.evidence,value.epoch);
    if parse_trailer_context(&text)? != TrailerContext::Task(value.clone()) {
        return Err("invalid task trailers".into());
    }
    Ok(text)
}

/// An exact actual outer Task receipt, usable only for durability repair.
/// Stale receipts retain their original status and grant no business authority.
#[derive(Clone)]
pub(crate) struct VerifiedTaskReceipt {
    project_id: String,
    event: Event,
}
impl VerifiedTaskReceipt {
    pub(crate) fn event_for(
        &self,
        project_id: &str,
        original: &NewEvent,
    ) -> Result<&Event, String> {
        if self.project_id != project_id
            || !task_receipt_kind(&original.kind)
            || crate::recovery_outbox::original_of(&self.event) != *original
        {
            return Err("verified Task receipt project/original binding differs".into());
        }
        crate::recovery_outbox::validate_actual(original, &self.event)?;
        Ok(&self.event)
    }
}
fn task_receipt_kind(kind: &str) -> bool {
    matches!(kind, "task.state" | "task.claimed" | "task.released")
}

/// Only actual core readers may construct this value. There is no raw-history deserializer.
pub struct VerifiedRecoveryHistory {
    project_id: String,
    authority: String,
    candidate: HistoryCandidate,
    sources: snapshot_service::SnapshotSources,
    // Actual outer records from the selected Git source and captured protocol tail.
    // Unlike originals, this excludes confirmed-only and inner proof roles.
    schedule_events: Vec<Event>,
    selected_archive_events: Vec<Event>,
    captured_recovery_gaps: Vec<crate::snapshot::SequenceRange>,
    coldstart_diagnostics: BTreeMap<String, Vec<String>>,
}
/// Own-request receipt roles are separate from the actual outer schedule.
/// These references can repair durability, never populate Human authorization.
pub(crate) struct HumanRequestHistory<'a> {
    pub(crate) accepted_originals: Vec<&'a Event>,
    pub(crate) actual_outer: Vec<&'a Event>,
}
impl VerifiedRecoveryHistory {
    /// An old system Ask result may be archived only as a receipt audit, never
    /// re-applied as a new non-stale Human transition in the rebuilt Hub.
    pub(crate) fn ask_system_original(
        &self,
        catalog: &Catalog,
        original: &NewEvent,
    ) -> Result<Option<&Event>, String> {
        validate_binding(catalog, self)?;
        crate::request_ledger::validate_ask_system_terminal(catalog.config.project_id(), original)?;
        let Some(actual) = self.candidate.originals.get(&original.eid) else {
            return Ok(None);
        };
        if crate::recovery_outbox::original_of(actual) != *original {
            return Err("Ask system original differs from its verified accepted receipt".into());
        }
        accepted(actual)?;
        Ok(Some(actual))
    }

    /// A finite guardless App Answer can only be replayed as an audit of this
    /// exact original accepted role, never as a fresh Human transition.
    pub(crate) fn app_answer_original(
        &self,
        catalog: &Catalog,
        original: &NewEvent,
    ) -> Result<Option<&Event>, String> {
        validate_binding(catalog, self)?;
        crate::ask_answer_contract::decode(catalog.config.project_id(), original)?;
        let Some(actual) = self.candidate.originals.get(&original.eid) else {
            return Ok(None);
        };
        if crate::recovery_outbox::original_of(actual) != *original || actual.stale {
            return Err("App Answer recovery differs from its original accepted role".into());
        }
        accepted(actual)?;
        Ok(Some(actual))
    }

    pub(crate) fn human_request_history<'a>(
        &'a self,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
    ) -> Result<HumanRequestHistory<'a>, String> {
        inputs.probe(catalog)?;
        let actual_outer = self.actual_outer_events(catalog)?;
        let mut accepted_originals = Vec::new();
        for event in self.candidate.originals.values().filter(|event| {
            (crate::ask_contract::ask_kind(&event.kind) || event.kind == "hold.reviewed")
                && (crate::request_ledger::is_human_request(&event.kind, &event.data)
                    || (event.kind == "hold.opened"
                        && event.layer == "L5"
                        && event.data["hold"]["body"]["kind"] == "review"))
        }) {
            // With no older accepted role, read_history also stores the new
            // stale outer record in candidate.originals. It cannot certify the
            // old request. A matching real confirmed stale receipt can.
            if event.stale && !inputs.confirmed().iter().any(|actual| actual == event) {
                continue;
            }
            crate::recovery_outbox::validate_actual(
                &crate::recovery_outbox::original_of(event),
                event,
            )?;
            accepted_originals.push(event);
        }
        for event in inputs
            .confirmed()
            .iter()
            .filter(|event| crate::request_ledger::is_human_request(&event.kind, &event.data))
        {
            if self.candidate.originals.get(&event.eid) != Some(event) {
                return Err("human receipt differs from the verified original role".into());
            }
        }
        accepted_originals.sort_by_key(|event| event.seq);
        Ok(HumanRequestHistory {
            accepted_originals,
            actual_outer,
        })
    }
    /// The complete proved actual outer prefix, never pending or inner facts.
    pub(crate) fn actual_outer_events(&self, catalog: &Catalog) -> Result<Vec<&Event>, String> {
        let through = self
            .sources
            .clone()
            .select()?
            .as_ref()
            .map_or(0, |saved| saved.snapshot.through_seq)
            .max(
                self.schedule_events
                    .iter()
                    .map(|event| event.seq)
                    .max()
                    .unwrap_or(0),
            );
        self.accepted_outer_events(catalog, through)
    }

    /// Recover only exact Task receipts in the verified actual outer schedule.
    /// Missing originals remain unresolved; sparse sequences never mint receipts.
    pub(crate) fn task_receipts(
        &self,
        catalog: &Catalog,
        through_seq: u64,
        originals: &[NewEvent],
    ) -> Result<BTreeMap<String, VerifiedTaskReceipt>, String> {
        let events = self.accepted_outer_events(catalog, through_seq)?;
        let mut requested = BTreeMap::new();
        for original in originals {
            if !task_receipt_kind(&original.kind) {
                return Err("Task receipt recovery only supports finite Task producers".into());
            }
            crate::hub::validate_input(original).map_err(|error| error.to_string())?;
            if requested
                .insert(original.eid.clone(), original)
                .is_some_and(|previous| previous != original)
            {
                return Err("Task receipt recovery has conflicting original requests".into());
            }
        }
        let mut receipts: BTreeMap<String, VerifiedTaskReceipt> = BTreeMap::new();
        for event in events {
            let Some(original) = requested.get(&event.eid) else {
                continue;
            };
            let receipt = VerifiedTaskReceipt {
                project_id: self.project_id.clone(),
                event: event.clone(),
            };
            receipt.event_for(catalog.config.project_id(), original)?;
            if receipts
                .insert(event.eid.clone(), receipt)
                .is_some_and(|previous| previous.event != *event)
            {
                return Err("Task receipt recovery has conflicting actual outer receipts".into());
            }
        }
        Ok(receipts)
    }

    /// The actual accepted outer schedule only. Omitted heartbeat/recovery
    /// ranges certify legal sequence gaps, never replacement business Events.
    pub(crate) fn accepted_outer_events(
        &self,
        catalog: &Catalog,
        through_seq: u64,
    ) -> Result<Vec<&Event>, String> {
        validate_binding(catalog, self)?;
        let selected = self.sources.clone().select()?;
        let boundary = selected
            .as_ref()
            .map_or(0, |saved| saved.snapshot.through_seq)
            .max(
                self.schedule_events
                    .iter()
                    .map(|event| event.seq)
                    .max()
                    .unwrap_or(0),
            );
        if through_seq > boundary {
            return Err("outer history exceeds the actual selected source/tail boundary".into());
        }
        let mut events = BTreeMap::new();
        for event in self
            .schedule_events
            .iter()
            .filter(|event| event.seq <= through_seq)
        {
            if events
                .insert(event.seq, event)
                .is_some_and(|old| old != event)
            {
                return Err("conflicting actual outer events at the same sequence".into());
            }
        }
        let mut coverage = Vec::new();
        let mut gaps = self.captured_recovery_gaps.clone();
        if let Some(saved) = selected {
            coverage.extend(saved.snapshot.omitted);
            gaps.extend(saved.snapshot.recovery_gaps);
        }
        if gaps
            .iter()
            .any(|gap| gap.start <= through_seq && through_seq <= gap.end)
        {
            return Err("outer history boundary is inside a recovery gap".into());
        }
        coverage.extend(gaps);
        coverage.extend(events.keys().map(|seq| crate::snapshot::SequenceRange {
            start: *seq,
            end: *seq,
        }));
        coverage.sort_by_key(|range| (range.start, range.end));
        let mut next = 1;
        let mut covered = through_seq == 0;
        for range in coverage {
            if range.start == 0 || range.end < range.start {
                return Err("outer history has invalid verified coverage metadata".into());
            }
            if range.start > through_seq {
                break;
            }
            if range.start > next {
                return Err("outer history has an uncovered required sequence range".into());
            }
            if range.end >= through_seq {
                covered = true;
                break;
            }
            next = next.max(range.end + 1);
        }
        if !covered {
            return Err("outer history does not cover the requested boundary".into());
        }
        Ok(events.into_values().collect())
    }
    pub(crate) fn baseline_review_original(&self, eid: &str) -> Option<&Event> {
        self.candidate
            .originals
            .get(eid)
            .filter(|event| crate::request_ledger::is_baseline_review(&event.kind))
    }
    pub(crate) fn baseline_request_original(&self, eid: &str) -> Option<&Event> {
        self.candidate.originals.get(eid).filter(|event| {
            matches!(
                event.kind.as_str(),
                "baseline.approved" | "baseline.rejected" | "draft.saved"
            )
        })
    }
    pub(crate) fn baseline_request_originals<'a>(
        &'a self,
        catalog: &Catalog,
    ) -> Result<Vec<&'a Event>, String> {
        if self.project_id != catalog.config.project_id() || self.authority != catalog.revision {
            return Err("baseline request history belongs to another project/authority".into());
        }
        Ok(self
            .candidate
            .originals
            .values()
            .filter(|event| {
                matches!(
                    event.kind.as_str(),
                    "baseline.approved" | "baseline.rejected" | "draft.saved"
                )
            })
            .collect())
    }
    pub(crate) fn publication_prepared_originals<'a>(
        &'a self,
        catalog: &Catalog,
    ) -> Result<Vec<&'a Event>, String> {
        validate_binding(catalog, self)?;
        Ok(self
            .candidate
            .originals
            .values()
            .filter(|event| event.kind == "baseline.publication_prepared")
            .collect())
    }
    pub(crate) fn publication_created_originals<'a>(
        &'a self,
        catalog: &Catalog,
    ) -> Result<Vec<&'a Event>, String> {
        validate_binding(catalog, self)?;
        Ok(self
            .candidate
            .originals
            .values()
            .filter(|event| event.kind == "baseline.created")
            .collect())
    }
    pub(crate) fn publication_created_original(&self, eid: &str) -> Option<&Event> {
        self.candidate
            .originals
            .get(eid)
            .filter(|event| event.kind == "baseline.created")
    }
    pub(crate) fn coldstart_diagnostics(&self) -> &BTreeMap<String, Vec<String>> {
        &self.coldstart_diagnostics
    }
    pub(crate) fn coldstart_for_scan(&self, scan_id: &str) -> Option<&Event> {
        self.candidate.originals.values().find(|event| {
            !event.stale && event.kind == "coldstart.reported" && event.obj == scan_id
        })
    }
    pub(crate) fn coldstart_original(&self, eid: &str) -> Option<&Event> {
        self.candidate.originals.get(eid).filter(|event| {
            !event.stale
                && (event.kind == "coldstart.reported"
                    || crate::coldstart_evidence::is_coldstart_evidence(event))
        })
    }
    pub(crate) fn snapshot_sources(&self) -> &snapshot_service::SnapshotSources {
        &self.sources
    }
    pub(crate) fn snapshot_summary(
        &self,
    ) -> Result<crate::project_session::SnapshotSummary, String> {
        let selected = self.sources.clone().select()?;
        self.snapshot_summary_with_ack(selected.as_ref())
    }
    pub(crate) fn snapshot_summary_with_ack(
        &self,
        acknowledged: Option<&snapshot_service::SavedSnapshot>,
    ) -> Result<crate::project_session::SnapshotSummary, String> {
        use crate::project_session::{
            matching_written, snapshot_alert, SnapshotLast, SnapshotSummary,
        };
        let selected = self.sources.clone().select()?;
        let last = selected.as_ref().map(|saved| {
            let mut last = SnapshotLast {
                commit: saved.commit.clone(),
                seq: saved.snapshot.through_seq,
                epoch: saved.snapshot.writer_epoch,
                written_at: None,
                written_seq: None,
            };
            if let Some(event) = self
                .schedule_events
                .iter()
                .find(|event| matching_written(event, &last))
            {
                last.written_at = chrono::DateTime::parse_from_rfc3339(&event.t)
                    .ok()
                    .and_then(|t| u64::try_from(t.timestamp_millis()).ok());
                last.written_seq = Some(event.seq);
            }
            last
        });
        let acknowledged_last = acknowledged.map(|saved| {
            let mut last = SnapshotLast {
                commit: saved.commit.clone(),
                seq: saved.snapshot.through_seq,
                epoch: saved.snapshot.writer_epoch,
                written_at: None,
                written_seq: None,
            };
            if let Some(event) = self
                .schedule_events
                .iter()
                .find(|event| matching_written(event, &last))
            {
                last.written_seq = Some(event.seq);
            }
            last
        });
        let mut closed_alerts = Vec::new();
        let mut alerts: Vec<_> = self
            .schedule_events
            .iter()
            .filter_map(snapshot_alert)
            .collect();
        alerts.sort_by_key(|alert| alert.seq);
        alerts.dedup_by(|a, b| a.eid == b.eid);
        let alert = alerts.into_iter().find(|alert| {
            let Some(last) = &acknowledged_last else {
                return true;
            };
            let covered = self.selected_archive_events.iter().any(|event| {
                event.eid == alert.eid
                    && event.seq == alert.seq
                    && event.seq <= last.seq
                    && serde_json::to_string(event).ok().is_some_and(|encoded| {
                        acknowledged.is_some_and(|saved| {
                            saved
                                .archives
                                .values()
                                .any(|text| text.lines().any(|line| line == encoded))
                        })
                    })
            });
            // A late preflight receipt for the old source is not completion.
            // Same-Q promotion is completion only at the actual new main source.
            let required = alert.basis["required_seq"].as_u64();
            let old = alert.basis["source"]["commit"].as_str();
            // Explicit null records a real pre-source failure. An absent or
            // malformed basis cannot prove that this is a new completion.
            let new_source = old.is_some_and(|commit| commit != last.commit)
                || alert.basis.get("source") == Some(&serde_json::Value::Null);
            let new_persistence = alert.basis["persistence"].as_str() == Some("observed")
                && match alert.basis.get("persisted") {
                    Some(serde_json::Value::Null) => true,
                    Some(persisted) => persisted["commit"]
                        .as_str()
                        .is_some_and(|commit| commit != last.commit),
                    None => false,
                };
            let target = alert.basis["target"].as_str();
            let completed = required.is_some_and(|q| last.seq >= q)
                && new_source
                && new_persistence
                && last.written_seq.is_some_and(|w| w > alert.seq)
                && target.is_none_or(|target| {
                    target != "main"
                        || acknowledged.is_some_and(|saved| {
                            saved.source == snapshot_service::SnapshotSource::Main
                        })
                });
            // Busy may observe a source that this same active writer already
            // published but has not acknowledged. Display last is not the
            // intent's original source. Require its exact frozen checkpoint,
            // new source identity, real post-alert receipt and exact ack.
            let inflight = &alert.basis["inflight"];
            let source_changed = match inflight.get("source_commit") {
                Some(serde_json::Value::Null) => true,
                Some(source) => source.as_str().is_some_and(|commit| commit != last.commit),
                None => false,
            };
            let inflight_completed = acknowledged.is_some_and(|saved| {
                source_changed
                    && inflight["seq"].as_u64() == Some(last.seq)
                    && inflight["epoch"].as_u64() == Some(last.epoch)
                    && last.written_seq.is_some_and(|w| w > alert.seq)
                    && match inflight["target"].as_str() {
                        Some("main") => saved.source == snapshot_service::SnapshotSource::Main,
                        Some("either") => true,
                        _ => false,
                    }
                    && serde_json::to_value(&saved.snapshot)
                        .ok()
                        .and_then(|value| crate::recovery_contract::canonical_hash(&value).ok())
                        .as_deref()
                        == inflight["checkpoint_hash"].as_str()
            });
            if covered || completed || inflight_completed {
                closed_alerts.push(alert.eid.clone());
                false
            } else {
                true
            }
        });
        Ok(SnapshotSummary {
            last,
            alert,
            closed_alerts,
            source_reference: selected.map(|saved| {
                match saved.source {
                    snapshot_service::SnapshotSource::Main => "main",
                    snapshot_service::SnapshotSource::Private => "refs/towermd/snapshot",
                }
                .into()
            }),
            ..Default::default()
        })
    }
}
#[derive(Clone)]
struct HistoryCandidate {
    originals: BTreeMap<String, Event>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MergeContext {
    schema: u32,
    #[serde(deserialize_with = "contract::deserialize_event")]
    claim: Event,
    #[serde(deserialize_with = "contract::deserialize_event")]
    started: Event,
    #[serde(deserialize_with = "contract::deserialize_event")]
    completed: Event,
    #[serde(deserialize_with = "contract::deserialize_event")]
    integration_gate: Event,
    review_contexts: Vec<ReviewContext>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Started {
    task: Value,
    run: Value,
    input: FrozenFinish,
    #[serde(deserialize_with = "contract::required_nullable")]
    review_context: Option<ReviewContext>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Completed {
    task: Value,
    run: Value,
    evidence: FinishEvidence,
    report: VerificationReport,
    hold: Value,
    #[serde(deserialize_with = "contract::required_nullable")]
    review_context: Option<ReviewContext>,
}
pub(crate) fn read_recovery_history(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
) -> Result<VerifiedRecoveryHistory, String> {
    read_history(catalog, inputs, &[], &[])
}

// Only Session capture and the private recovery staging builder supply additional records.
// No public event vector can mint a verified history.
fn read_history(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    tail: &[Event],
    gaps: &[crate::snapshot::SequenceRange],
) -> Result<VerifiedRecoveryHistory, String> {
    inputs.probe(catalog)?;
    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
        return Err("recovery authority changed".into());
    }
    let sources = snapshot_service::read_snapshot_sources(catalog)?;
    let private = sources.private_revision.clone();
    if let (Some(main), Some(private)) = (&sources.main, &sources.private) {
        if (main.snapshot.writer_epoch, main.snapshot.through_seq)
            == (private.snapshot.writer_epoch, private.snapshot.through_seq)
            && !snapshot_service::equivalent(main, private)
        {
            return Err("same-boundary dual snapshot conflict".into());
        }
    }
    let selected_commit = sources.clone().select()?.map(|saved| saved.commit);
    let mut selected_archive_events = Vec::new();
    let mut archives = Vec::new();
    for saved in [sources.main.as_ref(), sources.private.as_ref()]
        .into_iter()
        .flatten()
    {
        for text in saved.archives.values() {
            for line in text.lines() {
                let event =
                    crate::hub_legacy::decode_archive_line(line).map_err(|e| e.to_string())?;
                if selected_commit.as_ref() == Some(&saved.commit) {
                    selected_archive_events.push(event.clone());
                }
                archives.push((event, saved.snapshot.recovery_gaps.clone()));
            }
        }
    }
    archives.extend(tail.iter().cloned().map(|event| (event, gaps.to_vec())));
    let mut candidate = HistoryCandidate {
        originals: BTreeMap::new(),
    };
    for event in inputs.confirmed() {
        insert_original(&mut candidate.originals, event.clone())?;
    }
    // Extract original roles before considering replay envelopes. These remain private candidates
    // until the same pure algorithm verifies every canonical body and actual Git fact.
    for event in inputs
        .confirmed()
        .iter()
        .chain(archives.iter().map(|(e, _)| e))
    {
        collect_inner_originals(catalog, &mut candidate, event)?;
    }
    for (event, gaps) in &archives {
        if let Some(original) = candidate.originals.get(&event.eid) {
            if original != event {
                let replay = gaps
                    .iter()
                    .any(|g| original.seq <= g.end && event.seq > g.end)
                    && crate::recovery_outbox::original_of(original)
                        == crate::recovery_outbox::original_of(event);
                if !replay {
                    return Err("archive conflicts with original accepted role".into());
                }
                continue;
            }
        }
        insert_original(&mut candidate.originals, event.clone())?;
    }
    let mut sequences = BTreeMap::new();
    for event in candidate.originals.values() {
        if !event.stale && !event.legacy && !event.kind.starts_with("lease.") {
            crate::recovery_outbox::validate_actual(
                &crate::recovery_outbox::original_of(event),
                event,
            )?;
            if sequences
                .insert(event.seq, &event.eid)
                .is_some_and(|eid| eid != &event.eid)
            {
                return Err("conflicting original receipt sequence".into());
            }
        }
    }
    for event in inputs
        .confirmed()
        .iter()
        .chain(archives.iter().map(|(e, _)| e))
    {
        if event.kind != "baseline.created" {
            validate_canonical(catalog, &candidate, event)?;
        }
        if event.kind.starts_with("task.") && event.data["task"]["state"] == "merged" {
            return Err("raw task event cannot certify merged".into());
        }
    }
    // Receipt audit wrappers preserve an original role, never replace the
    // actual main/tag proof for an embedded canonical Git fact.
    for event in candidate
        .originals
        .values()
        .filter(|event| event.kind == "baseline.created")
    {
        validate_canonical(catalog, &candidate, event)?;
    }
    crate::coldstart_evidence::validate_history(catalog.config.project_id(), &candidate.originals)?;
    let coldstart_diagnostics =
        crate::coldstart_evidence::source_diagnostics(catalog, &candidate.originals);
    inputs.probe(catalog)?;
    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision
        || crate::snapshot_git::private_revision(&catalog.root)? != private
    {
        return Err("recovery sources changed during proof".into());
    }
    let mut schedule_events = selected_archive_events.clone();
    schedule_events.extend_from_slice(tail);
    Ok(VerifiedRecoveryHistory {
        project_id: catalog.config.project_id().into(),
        authority: catalog.revision.clone(),
        candidate,
        sources,
        schedule_events,
        selected_archive_events,
        captured_recovery_gaps: gaps.to_vec(),
        coldstart_diagnostics,
    })
}
/// Captured by the Session itself; callers cannot supply a public snapshot or event list.
pub(crate) fn prepare_session_history(
    catalog: &Catalog,
    inputs: &RecoveryInputs,
    capture: &crate::project_session::SessionCapture,
) -> Result<VerifiedRecoveryHistory, String> {
    let gaps = capture
        .base()
        .map(|b| b.recovery_gaps.as_slice())
        .unwrap_or(&[]);
    read_history(catalog, inputs, capture.history_tail(), gaps)
}

fn insert_original(events: &mut BTreeMap<String, Event>, event: Event) -> Result<(), String> {
    if !event.stale
        && !event.kind.starts_with("lease.")
        && events.values().any(|old| {
            !old.stale
                && !old.kind.starts_with("lease.")
                && old.seq == event.seq
                && old.eid != event.eid
        })
    {
        return Err("conflicting original receipt sequence".into());
    }
    if events.get(&event.eid).is_some_and(|old| old != &event) {
        return Err("conflicting original accepted receipt".into());
    }
    events.insert(event.eid.clone(), event);
    Ok(())
}
fn collect_inner_originals(
    catalog: &Catalog,
    history: &mut HistoryCandidate,
    event: &Event,
) -> Result<(), String> {
    if let Some(original) =
        crate::request_ledger::restored_original(catalog.config.project_id(), event)?
    {
        let publication_commit = if original.kind == "baseline.created" {
            Some(
                original.data["commit"]
                    .as_str()
                    .ok_or("restored baseline.created lacks actual commit")?
                    .to_owned(),
            )
        } else {
            None
        };
        insert_original(&mut history.originals, original)?;
        if let Some(commit) = publication_commit {
            collect_publication_originals(catalog, history, &commit, &mut BTreeSet::new())?;
        }
    }
    if let Some(original) = crate::coldstart_evidence::restored_original(event)? {
        insert_original(&mut history.originals, original)?;
    }
    if event.kind == "merge.done" {
        let proof: MergeContext = serde_json::from_value(event.data["proof"].clone())
            .map_err(|e| format!("invalid canonical merge proof: {e}"))?;
        if proof.schema != 1 {
            return Err("unknown merge proof schema".into());
        }
        for e in [
            &proof.claim,
            &proof.started,
            &proof.completed,
            &proof.integration_gate,
        ] {
            insert_original(&mut history.originals, e.clone())?;
        }
        for context in &proof.review_contexts {
            for e in [&context.opening, &context.reviewed, &context.gate_evidence] {
                insert_original(&mut history.originals, e.clone())?;
            }
        }
    }
    if event.kind == "baseline.created" {
        let commit = event.data["commit"]
            .as_str()
            .ok_or("baseline.created lacks actual commit")?;
        collect_publication_originals(catalog, history, commit, &mut BTreeSet::new())?;
    }
    Ok(())
}
fn collect_publication_originals(
    catalog: &Catalog,
    history: &mut HistoryCandidate,
    commit: &str,
    visited: &mut BTreeSet<String>,
) -> Result<(), String> {
    if !visited.insert(commit.into()) {
        return Err("cyclic publication proof chain".into());
    }
    let meta = read_publication_metadata(catalog, commit)?;
    insert_original(&mut history.originals, meta.decision_proof.approval_event)?;
    insert_original(&mut history.originals, meta.manifest_proof.prepared_event)?;
    if let Some(parent) = meta.manifest_proof.parent_baseline {
        let commit = peel_baseline(catalog, &parent)?;
        collect_publication_originals(catalog, history, &commit, visited)?;
    }
    Ok(())
}
fn validate_canonical(
    catalog: &Catalog,
    history: &HistoryCandidate,
    event: &Event,
) -> Result<(), String> {
    if event.kind == "merge.done" {
        let commit = event.data["commit"]
            .as_str()
            .ok_or("canonical merge lacks commit")?;
        let trailers = match parse_trailer_context(&git(
            catalog,
            &["show", "--no-patch", "--format=%B", commit],
        )?)? {
            TrailerContext::Task(value) => value,
            _ => return Err("canonical merge lacks task trailers".into()),
        };
        let canonical = prove_merge_candidate(
            catalog,
            history,
            &MergeProofInput {
                commit: commit.into(),
                trailers,
            },
        )?
        .into_event();
        if crate::recovery_outbox::original_of(event) != canonical {
            return Err("canonical merge body/envelope differs from proof".into());
        }
    }
    if event.kind == "baseline.created" {
        let commit = event.data["commit"]
            .as_str()
            .ok_or("canonical baseline lacks commit")?;
        let proof = prove_publication_candidate(
            catalog,
            &mut history.clone(),
            commit,
            &mut BTreeSet::new(),
        )?;
        if crate::recovery_outbox::original_of(event) != baseline_created_event(&proof)? {
            return Err("canonical baseline body/envelope differs from actual proof".into());
        }
    }
    Ok(())
}
pub struct MergeProofInput {
    pub commit: String,
    pub trailers: TaskTrailers,
}
pub struct VerifiedMerge {
    event: NewEvent,
}
impl VerifiedMerge {
    pub fn into_event(self) -> NewEvent {
        self.event
    }
}
pub fn prove_merge(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    input: &MergeProofInput,
) -> Result<VerifiedMerge, String> {
    validate_binding(catalog, history)?;
    prove_merge_candidate(catalog, &history.candidate, input)
}
fn validate_binding(catalog: &Catalog, history: &VerifiedRecoveryHistory) -> Result<(), String> {
    crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?;
    if history.project_id != catalog.config.project_id()
        || history.authority != catalog.revision
        || crate::catalog::authority_revision(&catalog.root)? != catalog.revision
    {
        return Err("history authority/project binding changed".into());
    }
    Ok(())
}
fn git(catalog: &Catalog, args: &[&str]) -> Result<String, String> {
    crate::repo::git_timeout(&catalog.root, args, std::time::Duration::from_secs(10))
}
fn ancestor(catalog: &Catalog, old: &str, new: &str) -> Result<bool, String> {
    contract::object_id(old)?;
    contract::object_id(new)?;
    let output = crate::repo::git_timeout_env_output(
        &catalog.root,
        &["merge-base", "--is-ancestor", old, new],
        std::time::Duration::from_secs(10),
        &[],
    )?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err("cannot inspect proof ancestry".into()),
    }
}
fn accepted(event: &Event) -> Result<(), String> {
    crate::recovery_outbox::validate_actual(&crate::recovery_outbox::original_of(event), event)?;
    if event.stale {
        return Err("stale original is not an accepted business proof".into());
    }
    Ok(())
}
fn required<'a>(history: &'a HistoryCandidate, event: &Event) -> Result<&'a Event, String> {
    accepted(event)?;
    history
        .originals
        .get(&event.eid)
        .filter(|e| *e == event)
        .ok_or_else(|| "original accepted role is absent/conflicting".into())
}
fn event_envelope(
    event: &Event,
    kind: &str,
    actor: &str,
    host: &str,
    layer: &str,
    obj: &str,
    guards: &[crate::hub_protocol::Fence],
) -> Result<(), String> {
    accepted(event)?;
    if event.kind != kind
        || event.actor != actor
        || event.host != host
        || event.layer != layer
        || event.obj != obj
        || event.guards != guards
    {
        return Err(format!("invalid {kind} producer/envelope binding"));
    }
    Ok(())
}
fn prove_merge_candidate(
    catalog: &Catalog,
    history: &HistoryCandidate,
    input: &MergeProofInput,
) -> Result<VerifiedMerge, String> {
    let actual_commit = main_entry(catalog, &input.commit)?;
    if parse_trailer_context(&git(
        catalog,
        &["show", "--no-patch", "--format=%B", &input.commit],
    )?)? != TrailerContext::Task(input.trailers.clone())
        || parse_trailer_context(&git(
            catalog,
            &["show", "--no-patch", "--format=%B", &actual_commit],
        )?)? != TrailerContext::Task(input.trailers.clone())
    {
        return Err("Task candidate/main-entry trailers differ".into());
    }
    let t = &input.trailers;
    let claims = history
        .originals
        .values()
        .filter(|e| {
            e.kind == "task.claimed" && e.obj == t.task && e.data["run"]["id"] == t.run && !e.stale
        })
        .collect::<Vec<_>>();
    if claims.len() != 1 {
        return Err("merge requires one original complete claim".into());
    }
    let claim = claims[0];
    accepted(claim)?;
    let task: crate::tasks::Task =
        serde_json::from_value(claim.data["task"].clone()).map_err(|e| e.to_string())?;
    let run: crate::tasks::Run =
        serde_json::from_value(crate::snapshot::recorded_run(claim, &claim.data["run"])?)
            .map_err(|e| e.to_string())?;
    if task.object_type != "Task"
        || run.object_type != "Run"
        || task.spec.id != t.task
        || task.spec.module != t.module
        || run.id != t.run
        || run.task != t.task
        || run.host != claim.host
        || task.state != "running"
        || run.state != "running"
    {
        return Err("claim Task/Run/trailers binding mismatch".into());
    }
    event_envelope(
        claim,
        "task.claimed",
        "process:towermd-cli",
        &run.host,
        "L5",
        &t.task,
        &run.leases,
    )?;
    if !run
        .leases
        .iter()
        .any(|f| f.name == format!("module:{}", t.module) && f.epoch == t.epoch)
    {
        return Err("task trailer epoch differs from main Module fence".into());
    }
    let mut matches = Vec::new();
    for started in history
        .originals
        .values()
        .filter(|e| e.kind == "finish.started" && e.obj == t.task && !e.stale)
    {
        let start: Started =
            serde_json::from_value(started.data.clone()).map_err(|e| e.to_string())?;
        if start.input.evidence_id != t.evidence || start.input.run_id != t.run {
            continue;
        }
        for completed in history
            .originals
            .values()
            .filter(|e| e.kind == "finish.completed" && e.obj == t.task && !e.stale)
        {
            let end: Completed =
                serde_json::from_value(completed.data.clone()).map_err(|e| e.to_string())?;
            if end.evidence.id == t.evidence && end.evidence.run == t.run {
                matches.push((
                    started,
                    start.input.clone(),
                    start.review_context.clone(),
                    start.task.clone(),
                    start.run.clone(),
                    completed,
                    end,
                ));
            }
        }
    }
    if matches.len() != 1 {
        return Err("merge requires unique original freeze/pass chain".into());
    }
    let (started, frozen, start_review, start_task, start_run, completed, end) = matches.remove(0);
    if frozen.project_id != catalog.config.project_id()
        || frozen.task_id != t.task
        || frozen.module_id != t.module
        || frozen.host != run.host
        || frozen.fences != run.leases
        || frozen.entered_verifying_at >= frozen.merge_deadline
        || claim.seq >= started.seq
        || started.seq >= completed.seq
    {
        return Err("frozen claim/context/phase order mismatch".into());
    }
    for (event, kind) in [(started, "finish.started"), (completed, "finish.completed")] {
        event_envelope(
            event,
            kind,
            "process:towermd-verifier",
            &frozen.host,
            "L4",
            &t.task,
            &frozen.fences,
        )?;
    }
    for (task_value, run_value) in [(&start_task, &start_run), (&end.task, &end.run)] {
        let mut original_task = claim.data["task"].clone();
        original_task["state"] = json!("verifying");
        if task_value != &original_task
            || run_value["id"] != t.run
            || run_value["task"] != t.task
            || run_value["host"] != run.host
            || run_value["leases"]
                != serde_json::to_value(&run.leases).map_err(|e| e.to_string())?
        {
            return Err("finish Task/Run differ from original claim".into());
        }
    }
    contract::validate_finish_report(&frozen, &completed.eid, &end.evidence, &end.report)?;
    if end.evidence.result != EvidenceResult::Passed
        || end.evidence.kind != "test"
        || end.evidence.extra.get("type") != Some(&json!("Evidence"))
        || !end.hold.is_null()
    {
        return Err("merge requires passed immutable verification Evidence".into());
    }
    for gate in ["G0", "G2", "G3", "G5"] {
        if end.evidence.gates.get(gate) != Some(&true) {
            return Err(format!("merge lacks passed {gate}"));
        }
    }
    if start_review != end.review_context {
        return Err("completion changed original G1 context".into());
    }
    let contexts = merge_reviews(
        catalog,
        history,
        claim,
        started,
        completed,
        &frozen,
        start_review.as_ref(),
    )?;
    let integration_gate = terminal_gate(
        catalog,
        history,
        &frozen,
        completed,
        &contexts,
        &actual_commit,
    )?;
    let recorded: IntegrationGateRecorded =
        serde_json::from_value(integration_gate.data.clone()).map_err(|e| e.to_string())?;
    let candidate_base = recorded
        .evidence
        .base
        .as_deref()
        .ok_or("terminal G5 lacks actual base")?;
    crate::task_gates::check_g5(catalog, &frozen, &actual_commit, candidate_base)
        .map_err(|e| format!("G5: {e:?}"))?;
    check_task_paths(catalog, &task, &frozen, candidate_base, &actual_commit)?;
    let mut task = claim.data["task"].clone();
    task["state"] = json!("merged");
    let mut run = claim.data["run"].clone();
    run["state"] = json!("ended");
    run["verification_evidence"] = json!(t.evidence);
    let tree = git(
        catalog,
        &["rev-parse", &format!("{}^{{tree}}", actual_commit)],
    )?;
    let proof = MergeContext {
        schema: 1,
        claim: claim.clone(),
        started: started.clone(),
        completed: completed.clone(),
        integration_gate,
        review_contexts: contexts,
    };
    Ok(VerifiedMerge {
        event: NewEvent {
            eid: crate::events::derived_eid(&actual_commit, "merge.done")?,
            host: frozen.host,
            actor: "process:towermd-integrator".into(),
            kind: "merge.done".into(),
            layer: "L4".into(),
            obj: t.task.clone(),
            guards: vec![],
            data: json!({"task":task,"run":run,"evidence":end.evidence,"commit":actual_commit,"tree":tree,"proof":proof}),
        },
    })
}
fn commit_parents(catalog: &Catalog, commit: &str) -> Result<Vec<String>, String> {
    contract::object_id(commit)?;
    Ok(git(catalog, &["rev-list", "--parents", "-n", "1", commit])?
        .split_whitespace()
        .skip(1)
        .map(String::from)
        .collect())
}
fn main_entry(catalog: &Catalog, candidate: &str) -> Result<String, String> {
    contract::object_id(candidate)?;
    if !ancestor(catalog, candidate, &catalog.revision)? {
        return Err("merge commit is not reachable from authority".into());
    }
    let first = git(catalog, &["rev-list", "--first-parent", &catalog.revision])?;
    if first.lines().any(|c| c == candidate) {
        return Ok(candidate.into());
    }
    for entry in git(
        catalog,
        &[
            "rev-list",
            "--first-parent",
            "--reverse",
            &catalog.revision,
            "--not",
            candidate,
        ],
    )?
    .lines()
    {
        if ancestor(catalog, candidate, entry)? {
            return Ok(entry.into());
        }
    }
    Err("Task has no actual main first-parent entry".into())
}
fn terminal_gate(
    catalog: &Catalog,
    history: &HistoryCandidate,
    f: &FrozenFinish,
    completed: &Event,
    contexts: &[ReviewContext],
    entry: &str,
) -> Result<Event, String> {
    let mut candidates = history
        .originals
        .values()
        .filter(|e| {
            e.kind == "evidence.recorded"
                && e.data.get("completed_eid").is_some()
                && e.data["evidence"]["run"] == f.run_id
                && e.data["evidence"]["task"] == f.task_id
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| (a.seq, &a.eid).cmp(&(b.seq, &b.eid)));
    let mut diagnostic = "missing original terminal G5".to_owned();
    for event in candidates {
        match validate_terminal_gate(catalog, history, event, f, completed, contexts, entry) {
            Ok(()) => return Ok(event.clone()),
            Err(error) => diagnostic = error,
        }
    }
    Err(format!("no valid original terminal G5: {diagnostic}"))
}
fn validate_terminal_gate(
    catalog: &Catalog,
    history: &HistoryCandidate,
    event: &Event,
    f: &FrozenFinish,
    completed: &Event,
    contexts: &[ReviewContext],
    entry: &str,
) -> Result<(), String> {
    required(history, event)?;
    let recorded: IntegrationGateRecorded = serde_json::from_value(event.data.clone())
        .map_err(|e| format!("invalid terminal G5 layout: {e}"))?;
    let e = &recorded.evidence;
    contract::validate_gate_layout(e)?;
    event_envelope(
        event,
        "evidence.recorded",
        "process:towermd-gates",
        &f.host,
        "L5",
        &e.id,
        &f.fences,
    )?;
    let u =
        e.id.strip_prefix("E-")
            .ok_or("terminal G5 lacks UUIDv7 id")?;
    let base = e.base.as_deref().ok_or("terminal G5 lacks actual base")?;
    let commit = e
        .commit
        .as_deref()
        .ok_or("terminal G5 lacks captured HEAD")?;
    let tree = e.tree.as_deref().ok_or("terminal G5 lacks captured tree")?;
    for oid in [base, commit, tree] {
        contract::object_id(oid)?;
    }
    for parent in &recorded.parents {
        contract::object_id(parent)?;
        if parent.len() != commit.len() {
            return Err("terminal G5 parent object format mismatch".into());
        }
    }
    let plan_opening = contexts
        .iter()
        .find(|c| c.gate == ReviewGate::G1)
        .map(|c| c.opening.eid.as_str());
    if e.gate != crate::task_gates::Gate::G5
        || e.result != GateResult::Passed
        || e.review.is_some()
        || e.diagnostic.is_some()
        || e.project_id != f.project_id
        || e.task != f.task_id
        || e.run != f.run_id
        || e.module != f.module_id
        || e.baseline.as_ref() != Some(&f.baseline)
        || e.plan_hash != f.plan_hash
        || e.plan_opening_eid.as_deref() != plan_opening
        || e.commands_hash.as_ref() != Some(&f.commands_hash)
        || e.fences != f.fences
        || e.authority.as_deref() != Some(base)
        || recorded.completed_eid != completed.eid
        || event.eid != request_event_eid(u, "evidence.recorded")?
        || completed.seq >= event.seq
        || contexts.iter().any(|c| c.gate_evidence.seq >= event.seq)
        || crate::hub::event_time(event).map_err(|e| e.to_string())? > f.merge_deadline
    {
        return Err("terminal G5 differs from passed freeze/phase/context".into());
    }
    if let Some(g1) = contexts.iter().find(|c| c.gate == ReviewGate::G1) {
        current_plan(history, f, g1, event.seq)?;
    }
    // When the captured object survives, its audit values must still equal real Git.
    match crate::repo::git_object_type(&catalog.root, commit)? {
        Some(kind) if kind == "commit" => {
            if git(catalog, &["rev-parse", &format!("{commit}^{{tree}}")])? != tree
                || commit_parents(catalog, commit)? != recorded.parents
            {
                return Err("terminal G5 captured object/parents mismatch".into());
            }
        }
        None => {}
        Some(_) => return Err("terminal G5 captured HEAD is not a valid commit".into()),
    }
    let tip_matches = |tip: &str| -> Result<bool, String> {
        let parents = commit_parents(catalog, tip)?;
        Ok(
            git(catalog, &["rev-parse", &format!("{tip}^{{tree}}")])? == tree
                && (parents == recorded.parents || parents == [base.to_owned()]),
        )
    };
    let parents = commit_parents(catalog, entry)?;
    if !tip_matches(entry)?
        && (parents.len() != 2
            || parents[0] != base
            || !tip_matches(&parents[1])?
            || git(catalog, &["rev-parse", &format!("{entry}^{{tree}}")])? != tree)
    {
        return Err("actual main entry does not match terminal G5 parents/base/tree".into());
    }
    crate::task_gates::check_g5(catalog, f, entry, base)
        .map_err(|e| format!("terminal G5 actual Git: {e:?}"))
}
fn check_task_paths(
    catalog: &Catalog,
    task: &crate::tasks::Task,
    frozen: &FrozenFinish,
    base: &str,
    candidate: &str,
) -> Result<(), String> {
    let source = Catalog::at_revision(&catalog.root, &frozen.authority)?
        .ok_or("task source has no Catalog")?;
    let mut module_paths = Vec::<String>::new();
    for fence in &frozen.fences {
        let id = fence
            .name
            .strip_prefix("module:")
            .ok_or("task fence is not a Module")?;
        let module = source
            .objects
            .get(id)
            .filter(|d| d.doc_type() == Some("Module"))
            .ok_or("task original Module is absent")?;
        let paths: Vec<String> =
            serde_yaml::from_value(module.get("paths").cloned().ok_or("Module lacks paths")?)
                .map_err(|e| e.to_string())?;
        module_paths.extend(paths);
    }
    if task.spec.allowed_paths.is_empty() {
        return Err("task has no original allowed_paths".into());
    }
    crate::catalog::validate_paths(
        &task.spec.allowed_paths,
        &module_paths,
        source.config.docs_dir(),
    )?;
    crate::catalog::validate_protected_paths(&task.spec.allowed_paths, &source.protected_paths)?;
    let at =
        Catalog::at_revision(&catalog.root, candidate)?.ok_or("task candidate has no Catalog")?;
    crate::catalog::validate_protected_paths(&task.spec.allowed_paths, &at.protected_paths)?;
    let changes = |patterns: &[String]| -> Result<BTreeSet<Vec<u8>>, String> {
        let mut args = vec![
            "diff-tree",
            "--no-commit-id",
            "-r",
            "--raw",
            "-z",
            "--full-index",
            "--abbrev=0",
            "--no-abbrev",
            "--no-renames",
            base,
            candidate,
            "--",
        ];
        args.extend(patterns.iter().map(String::as_str));
        let raw = git_bytes(catalog, &args)?;
        Ok(crate::task_gates::parse_raw_z(&raw)
            .map_err(|e| format!("invalid task scope diff: {e:?}"))?
            .into_iter()
            .map(|c| c.path)
            .collect())
    };
    let patterns = task
        .spec
        .allowed_paths
        .iter()
        .map(|p| format!(":(glob){p}"))
        .collect::<Vec<_>>();
    if changes(&[])? != changes(&patterns)? {
        return Err("actual task changes exceed original allowed_paths".into());
    }
    Ok(())
}
fn merge_reviews(
    catalog: &Catalog,
    history: &HistoryCandidate,
    claim: &Event,
    started: &Event,
    completed: &Event,
    frozen: &FrozenFinish,
    context: Option<&ReviewContext>,
) -> Result<Vec<ReviewContext>, String> {
    let source = Catalog::at_revision(&catalog.root, &frozen.authority)?
        .ok_or("missing frozen authority Catalog")?;
    let mut high = false;
    for fence in &frozen.fences {
        let id = fence
            .name
            .strip_prefix("module:")
            .ok_or("finish contains non-module fence")?;
        let module = source
            .objects
            .get(id)
            .filter(|o| o.doc_type() == Some("Module"))
            .ok_or("claim references unknown original Module")?;
        high |= module.get_str("risk") == Some("high");
    }
    let mut contexts = Vec::new();
    if high || context.is_some() || frozen.plan_hash.is_some() {
        let g1 = context.ok_or("high-risk merge lacks original G1 context")?;
        if g1.gate != ReviewGate::G1 {
            return Err("freeze context is not G1".into());
        }
        validate_review_context(history, g1, frozen, None, None)?;
        if !(claim.seq < g1.opening.seq
            && g1.opening.seq < g1.reviewed.seq
            && g1.reviewed.seq < g1.gate_evidence.seq
            && g1.gate_evidence.seq < started.seq)
        {
            return Err("invalid original G1 phase ordering".into());
        }
        for at in [
            g1.reviewed.seq,
            g1.gate_evidence.seq,
            started.seq,
            completed.seq,
        ] {
            current_plan(history, frozen, g1, at)?;
        }
        contexts.push(g1.clone());
    }
    // Late G4 is an independent accepted gate fact, never a mutation of completed.
    let mut g4s = Vec::new();
    for event in history
        .originals
        .values()
        .filter(|e| e.kind == "evidence.recorded" && !e.stale)
    {
        // read_history has already validated this exact original declaration.
        // Coldstart declarations cannot supply any Task/Run/Gate authorization.
        if crate::coldstart_evidence::is_coldstart_evidence(event) {
            continue;
        }
        let evidence: GateEvidence =
            serde_json::from_value(event.data["evidence"].clone()).map_err(|e| e.to_string())?;
        if evidence.gate != crate::task_gates::Gate::G4
            || evidence.run != frozen.run_id
            || evidence.task != frozen.task_id
            || evidence.result != GateResult::Passed
        {
            continue;
        }
        let link = evidence.review.as_ref().ok_or("G4 lacks original review")?;
        let c = ReviewContext {
            gate: ReviewGate::G4,
            opening: link.opening.clone(),
            reviewed: link.reviewed.clone(),
            gate_evidence: event.clone(),
        };
        if validate_review_context(history, &c, frozen, Some(&completed.eid), contexts.first())
            .is_ok()
            && event.seq > completed.seq
        {
            if let Some(g1) = contexts.first() {
                current_plan(history, frozen, g1, c.reviewed.seq)?;
                current_plan(history, frozen, g1, c.gate_evidence.seq)?;
            }
            g4s.push(c);
        }
    }
    g4s.sort_by_key(|c| c.gate_evidence.seq);
    if high && g4s.is_empty() {
        return Err("high-risk merge lacks original independent G4".into());
    }
    if let Some(g4) = g4s.into_iter().next() {
        contexts.push(g4);
    }
    if let Some(g1) = contexts.first().filter(|c| c.gate == ReviewGate::G1) {
        // Known later Plan openings cannot be hidden by claiming an earlier frozen hash.
        current_plan(history, frozen, g1, u64::MAX)?;
    }
    Ok(contexts)
}
fn opening_view(event: &Event) -> Result<ReviewOpenView, String> {
    let hold: ReviewHoldView =
        serde_json::from_value(event.data["hold"].clone()).map_err(|e| e.to_string())?;
    let ReviewBodyView::Review {
        binding,
        plan_content,
        ..
    } = &hold.body;
    let gate = match binding.subject {
        contract::task_review::ReviewSubject::Plan { .. } => ReviewGate::G1,
        contract::task_review::ReviewSubject::Tree { .. } => ReviewGate::G4,
    };
    let view = ReviewOpenView {
        gate,
        binding: binding.clone(),
        plan_content: plan_content.clone(),
        hold,
    };
    contract::validate_review_open(&view)?;
    event_envelope(
        event,
        "hold.opened",
        "process:towermd-cli",
        &event.host,
        "L5",
        &view.hold.id,
        &view.binding.fences,
    )?;
    Ok(view)
}
fn current_plan(
    history: &HistoryCandidate,
    frozen: &FrozenFinish,
    g1: &ReviewContext,
    at: u64,
) -> Result<(), String> {
    let mut latest = None;
    for event in history
        .originals
        .values()
        .filter(|e| e.kind == "hold.opened" && !e.stale && e.seq < at)
    {
        // Non-review Holds are unrelated. A malformed claimed Review remains a proof rejection.
        if event.data["hold"]["body"]["kind"] != "review" {
            continue;
        }
        let view = opening_view(event)?;
        if view.gate == ReviewGate::G1
            && view.binding.run == frozen.run_id
            && latest.is_none_or(|seq| event.seq > seq)
        {
            latest = Some(event.seq);
        }
    }
    if latest != Some(g1.opening.seq) {
        return Err("original Plan context superseded by known later opening".into());
    }
    Ok(())
}
fn validate_review_context(
    history: &HistoryCandidate,
    c: &ReviewContext,
    f: &FrozenFinish,
    completed: Option<&str>,
    g1: Option<&ReviewContext>,
) -> Result<(), String> {
    for event in [&c.opening, &c.reviewed, &c.gate_evidence] {
        required(history, event)?;
    }
    let open = opening_view(&c.opening)?;
    if open.gate != c.gate
        || open.binding.run != f.run_id
        || open.binding.task != f.task_id
        || open.binding.baseline != f.baseline
        || open.binding.fences != f.fences
    {
        return Err("review opening differs from frozen binding".into());
    }
    match (&c.gate, &open.binding.subject) {
        (ReviewGate::G1, contract::task_review::ReviewSubject::Plan { sha256 })
            if f.plan_hash.as_ref() == Some(sha256) => {}
        (ReviewGate::G4, contract::task_review::ReviewSubject::Tree { tree })
            if tree == &f.tree => {}
        _ => return Err("review subject differs from frozen Plan/tree".into()),
    }
    contract::validate_recorded_data(&f.project_id, "hold.reviewed", &c.reviewed.data)?;
    let reviewed: TaskReviewRecorded =
        serde_json::from_value(c.reviewed.data.clone()).map_err(|e| e.to_string())?;
    event_envelope(
        &c.reviewed,
        "hold.reviewed",
        &reviewed.fact.actor,
        &reviewed.request_body.host,
        "L5",
        &open.hold.id,
        &f.fences,
    )?;
    if reviewed.fact.binding != open.binding
        || reviewed.fact.verdict != contract::task_review::ReviewVerdict::Approved
        || reviewed.fact.answer_eid != c.reviewed.eid
        || reviewed.hold.opened_ms != open.hold.opened_ms
        || reviewed.hold.opened_seq != open.hold.opened_seq
    {
        return Err("review answer is not the original approved opening".into());
    }
    let mut original_hold = serde_json::to_value(&open.hold).map_err(|e| e.to_string())?;
    original_hold["body"]["verdict"] = json!("approved");
    original_hold["body"]["comment"] = json!(reviewed.comment);
    original_hold["reply"] =
        serde_json::to_value(&reviewed.hold.reply).map_err(|e| e.to_string())?;
    if original_hold != serde_json::to_value(&reviewed.hold).map_err(|e| e.to_string())? {
        return Err("review altered original Hold content".into());
    }
    let evidence: GateEvidence = serde_json::from_value(c.gate_evidence.data["evidence"].clone())
        .map_err(|e| e.to_string())?;
    contract::validate_gate_layout(&evidence)?;
    event_envelope(
        &c.gate_evidence,
        "evidence.recorded",
        "process:towermd-gates",
        &f.host,
        "L5",
        &evidence.id,
        &f.fences,
    )?;
    let id = evidence.id.strip_prefix("E-").ok_or("gate lacks id")?;
    if c.gate_evidence.eid != request_event_eid(id, "evidence.recorded")?
        || evidence.result != GateResult::Passed
        || evidence.gate
            != match c.gate {
                ReviewGate::G1 => crate::task_gates::Gate::G1,
                ReviewGate::G4 => crate::task_gates::Gate::G4,
            }
        || evidence.project_id != f.project_id
        || evidence.task != f.task_id
        || evidence.run != f.run_id
        || evidence.module != f.module_id
        || evidence.fences != f.fences
        || evidence.commit.as_ref() != Some(&f.commit)
        || evidence.tree.as_ref() != Some(&f.tree)
        || evidence.base.as_ref() != Some(&f.base)
        || evidence.authority.as_ref() != Some(&f.authority)
        || evidence.baseline.as_ref() != Some(&f.baseline)
        || evidence.commands_hash.as_ref() != Some(&f.commands_hash)
        || evidence.plan_hash != f.plan_hash
    {
        return Err("gate Evidence differs from frozen context".into());
    }
    let link = evidence
        .review
        .as_ref()
        .ok_or("gate lacks original links")?;
    let plan_eid = match c.gate {
        ReviewGate::G1 => Some(c.opening.eid.as_str()),
        ReviewGate::G4 => g1.map(|c| c.opening.eid.as_str()),
    };
    if link.opening != c.opening
        || link.reviewed != c.reviewed
        || link.completed_eid.as_deref() != completed
        || evidence.plan_opening_eid.as_deref() != plan_eid
        || !(c.opening.seq < c.reviewed.seq && c.reviewed.seq < c.gate_evidence.seq)
    {
        return Err("gate original links/phase order mismatch".into());
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionPublication {
    schema: u32,
    publication_id: String,
    approval: contract::baseline::ApprovalFact,
    #[serde(deserialize_with = "contract::deserialize_event")]
    approval_event: Event,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestPublication {
    schema: u32,
    publication_id: String,
    approval_eid: String,
    decision_path: String,
    #[serde(deserialize_with = "contract::required_nullable")]
    parent_baseline: Option<String>,
    publication_host: String,
    writer_epoch: u64,
    #[serde(deserialize_with = "contract::deserialize_event")]
    prepared_event: Event,
    binding: contract::baseline::ReviewBinding,
    freeze: contract::baseline::PublicationFreeze,
}
struct PublicationMetadata {
    decision: Value,
    manifest: Value,
    decision_proof: DecisionPublication,
    manifest_proof: ManifestPublication,
    decision_path: String,
    manifest_path: String,
    trailers: PublicationTrailers,
}
pub struct VerifiedPublication {
    event: NewEvent,
    diagnostics: Vec<String>,
}
impl VerifiedPublication {
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}
pub fn baseline_created_event(value: &VerifiedPublication) -> Result<NewEvent, String> {
    Ok(value.event.clone())
}
/// Actual immutable payload only. This does not certify main reachability, a
/// tag, current writer permission, or a completed publication.
pub(crate) struct PublicationCandidate {
    commit: String,
    parent: String,
    metadata: PublicationMetadata,
}
impl PublicationCandidate {
    pub(crate) fn commit(&self) -> &str {
        &self.commit
    }
    pub(crate) fn parent(&self) -> &str {
        &self.parent
    }
    pub(crate) fn approval_eid(&self) -> &str {
        &self.metadata.manifest_proof.approval_eid
    }
    pub(crate) fn binding(&self) -> &contract::baseline::ReviewBinding {
        &self.metadata.manifest_proof.binding
    }
    pub(crate) fn prepared(&self) -> &Event {
        &self.metadata.manifest_proof.prepared_event
    }
}

pub(crate) fn read_publication_candidate(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    commit: &str,
) -> Result<PublicationCandidate, String> {
    validate_binding(catalog, history)?;
    let metadata = check_publication_payload(catalog, &mut history.candidate.clone(), commit)?;
    if metadata.manifest_proof.binding.target_baseline != "towermd/B0"
        || metadata.manifest_proof.parent_baseline.is_some()
    {
        return Err("only initial B0 publication is supported by this writer".into());
    }
    let parent = git(catalog, &["rev-parse", &format!("{commit}^")])?;
    if !ancestor(catalog, &parent, &catalog.revision)? {
        return Err("publication parent is outside authority main".into());
    }
    Ok(PublicationCandidate {
        commit: commit.into(),
        parent,
        metadata,
    })
}

pub(crate) struct PublicationAudit {
    pub(crate) decision_path: String,
    pub(crate) decision: String,
    pub(crate) manifest_path: String,
    pub(crate) manifest: String,
    pub(crate) message: String,
}

/// Serialize the consumer's one machine schema; caller still must validate the
/// resulting actual commit before updating any retained/public reference.
pub(crate) fn publication_audit(
    catalog: &Catalog,
    frozen: &contract::baseline::FrozenDraft,
    approval_event: &Event,
    prepared_event: &Event,
) -> Result<PublicationAudit, String> {
    let freeze = contract::baseline::publication_freeze(frozen)?;
    let approval: BaselineReviewRecorded =
        serde_json::from_value(approval_event.data.clone()).map_err(|e| e.to_string())?;
    let prepared: PublicationPrepared =
        serde_json::from_value(prepared_event.data.clone()).map_err(|e| e.to_string())?;
    let fact = &approval.approval;
    let binding = &fact.binding;
    if binding.target_baseline != "towermd/B0" {
        return Err("only B0 publication is supported".into());
    }
    let (decision_path, manifest_path) = publication_paths(
        catalog.config.docs_dir(),
        &binding.target_baseline,
        &fact.approval_eid,
    )?;
    let parent_tree = tree(catalog, &catalog.revision)?;
    if parent_tree.contains_key(&decision_path) || parent_tree.contains_key(&manifest_path) {
        return Err("publication audit path already occupied".into());
    }
    let mut objects = BTreeMap::new();
    let mut affects = BTreeSet::new();
    for (path, file) in &frozen.files {
        if path == &format!("{}/tower.canvas", freeze.docs_dir) {
            continue;
        }
        let document = crate::okf::Document::parse(&file.text)?;
        let id = document
            .get_str("id")
            .ok_or("publication object lacks id")?;
        objects.insert(id.to_owned(), contract::sha256(file.text.as_bytes()));
        let unchanged = match parent_tree.get(path) {
            Some((mode, oid)) => mode == &file.mode && blob_text(catalog, oid)? == file.text,
            None => false,
        };
        if !unchanged {
            affects.insert(id.to_owned());
        }
    }
    for path in &freeze.deletions {
        let doc = crate::okf::Document::parse(&file_text(catalog, &catalog.revision, path)?)?;
        affects.insert(
            doc.get_str("id")
                .ok_or("publication deletion lacks object id")?
                .to_owned(),
        );
    }
    let decision_proof = DecisionPublication {
        schema: 1,
        publication_id: fact.approval_eid.clone(),
        approval: fact.clone(),
        approval_event: approval_event.clone(),
    };
    let manifest_proof = ManifestPublication {
        schema: 1,
        publication_id: fact.approval_eid.clone(),
        approval_eid: fact.approval_eid.clone(),
        decision_path: decision_path.clone(),
        parent_baseline: None,
        publication_host: prepared_event.host.clone(),
        writer_epoch: prepared_event
            .guards
            .first()
            .ok_or("preparation has no main fence")?
            .epoch,
        prepared_event: prepared_event.clone(),
        binding: binding.clone(),
        freeze,
    };
    let mut decision = crate::okf::Document::new("Decision", "")?;
    for (key, value) in [
        (
            "id",
            json!(format!(
                "publication-{}",
                contract::sha256(fact.approval_eid.as_bytes())
            )),
        ),
        ("layer", json!("L1")),
        (
            "context",
            json!(format!("baseline publication {}", binding.target_baseline)),
        ),
        ("options", json!(["approve", "reject"])),
        ("decision", json!("approve")),
        ("rationale", json!(fact.comment)),
        ("affects", json!(affects.into_iter().collect::<Vec<_>>())),
        ("supersedes", json!([])),
        ("class", json!("baseline-publication")),
        ("generated", json!({"by":fact.actor,"at":approval_event.t})),
        ("verified", json!([{"by":fact.actor,"at":approval_event.t}])),
        (
            "publication",
            serde_json::to_value(decision_proof).map_err(|e| e.to_string())?,
        ),
    ] {
        decision.set(key, serde_yaml::to_value(value).map_err(|e| e.to_string())?)?;
    }
    let mut manifest = crate::okf::Document::new("Baseline", "")?;
    for (key, value) in [
        ("id", json!("B0")),
        ("layer", json!("L1")),
        ("objects", json!(objects)),
        ("approved_by", json!(fact.actor)),
        ("approved_at", json!(approval_event.t)),
        ("git_tag", json!(binding.target_baseline)),
        (
            "generated",
            json!({"by":"process:towermd-baseline","at":approval_event.t}),
        ),
        (
            "publication",
            serde_json::to_value(&manifest_proof).map_err(|e| e.to_string())?,
        ),
    ] {
        manifest.set(key, serde_yaml::to_value(value).map_err(|e| e.to_string())?)?;
    }
    if prepared.authority_revision != catalog.revision {
        return Err("preparation parent differs from authority".into());
    }
    Ok(PublicationAudit { decision_path, decision: decision.to_text(), manifest_path, manifest: manifest.to_text(), message: format!(
        "Tower.md approved B0\n\nTowermd-Baseline: {}\nTowermd-Approval: {}\nTowermd-Publication: {}\nTowermd-Content-Hash: {}\nTowermd-Epoch: {}\n",
        binding.target_baseline, fact.approval_eid, fact.approval_eid, binding.content_hash, manifest_proof.writer_epoch
    ) })
}

pub fn prove_publication(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    actual_commit: &str,
) -> Result<VerifiedPublication, String> {
    validate_binding(catalog, history)?;
    prove_publication_candidate(
        catalog,
        &mut history.candidate.clone(),
        actual_commit,
        &mut BTreeSet::new(),
    )
}
/// Common deterministic audit paths for the future P4 writer and this consumer.
pub fn publication_paths(
    docs_dir: &str,
    target: &str,
    approval_eid: &str,
) -> Result<(String, String), String> {
    contract::repo_path(docs_dir.as_bytes())?;
    baseline_number(target)?;
    Ok((
        format!(
            "{docs_dir}/decisions/publication-{}.md",
            contract::sha256(approval_eid.as_bytes())
        ),
        format!(
            "{docs_dir}/baselines/{}.md",
            target.strip_prefix("towermd/").ok_or("invalid baseline")?
        ),
    ))
}
fn baseline_number(target: &str) -> Result<u64, String> {
    let text = target
        .strip_prefix("towermd/B")
        .ok_or("invalid precise baseline target")?;
    let n: u64 = text.parse().map_err(|_| "invalid baseline number")?;
    if n.to_string() != text {
        return Err("noncanonical baseline number".into());
    }
    Ok(n)
}
fn git_bytes(catalog: &Catalog, args: &[&str]) -> Result<Vec<u8>, String> {
    crate::repo::git_bytes_timeout(&catalog.root, args, std::time::Duration::from_secs(10))
}
type GitTreeEntries = BTreeMap<String, (String, String)>;
fn tree(catalog: &Catalog, revision: &str) -> Result<GitTreeEntries, String> {
    contract::object_id(revision)?;
    let bytes = git_bytes(catalog, &["ls-tree", "-r", "-z", "--full-tree", revision])?;
    #[cfg(test)]
    tests::publication_local_reads::record(catalog, revision, "tree");
    let mut result = BTreeMap::new();
    for entry in bytes.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let i = entry
            .iter()
            .position(|b| *b == b'\t')
            .ok_or("invalid Git tree row")?;
        let metadata = std::str::from_utf8(&entry[..i])
            .map_err(|e| e.to_string())?
            .split_whitespace()
            .collect::<Vec<_>>();
        if metadata.len() != 3 {
            return Err("invalid Git tree metadata".into());
        }
        let path = std::str::from_utf8(&entry[i + 1..])
            .map_err(|_| "publication tree contains non-UTF8 path")?;
        contract::repo_path(path.as_bytes())?;
        contract::object_id(metadata[2])?;
        if result
            .insert(path.into(), (metadata[0].into(), metadata[2].into()))
            .is_some()
        {
            return Err("duplicate Git path".into());
        }
    }
    Ok(result)
}
fn blob_text(catalog: &Catalog, oid: &str) -> Result<String, String> {
    contract::object_id(oid)?;
    if git(catalog, &["cat-file", "-t", oid])? != "blob" {
        return Err("publication path is not a blob".into());
    }
    String::from_utf8(git_bytes(catalog, &["cat-file", "blob", oid])?).map_err(|e| e.to_string())
}
fn file_text(catalog: &Catalog, revision: &str, path: &str) -> Result<String, String> {
    file_text_in_tree(catalog, &tree(catalog, revision)?, path)
}
// Only an actual tree read in this same publication operation supplies entries.
fn file_text_in_tree(
    catalog: &Catalog,
    entries: &GitTreeEntries,
    path: &str,
) -> Result<String, String> {
    let (mode, oid) = entries
        .get(path)
        .ok_or_else(|| format!("missing publication file: {path}"))?;
    contract::file_mode(mode)?;
    blob_text(catalog, oid)
}
fn front(text: &str) -> Result<Value, String> {
    let doc = crate::okf::Document::parse(text)?;
    serde_json::to_value(doc.front()).map_err(|e| e.to_string())
}
fn read_publication_metadata(
    catalog: &Catalog,
    commit: &str,
) -> Result<PublicationMetadata, String> {
    read_publication_metadata_with_tree(catalog, commit).map(|(metadata, _)| metadata)
}
fn read_publication_metadata_with_tree(
    catalog: &Catalog,
    commit: &str,
) -> Result<(PublicationMetadata, GitTreeEntries), String> {
    contract::object_id(commit)?;
    let trailers = match parse_trailer_context(&git(
        catalog,
        &["show", "--no-patch", "--format=%B", commit],
    )?)? {
        TrailerContext::Publication(t) => t,
        _ => return Err("commit has no complete publication trailers".into()),
    };
    let (decision_path, manifest_path) = publication_paths(
        catalog.config.docs_dir(),
        &trailers.baseline,
        &trailers.approval,
    )?;
    let entries = tree(catalog, commit)?;
    let decision = front(&file_text_in_tree(catalog, &entries, &decision_path)?)?;
    let manifest = front(&file_text_in_tree(catalog, &entries, &manifest_path)?)?;
    let decision_proof = serde_json::from_value(decision["publication"].clone())
        .map_err(|e| format!("invalid publication Decision proof: {e}"))?;
    let manifest_proof = serde_json::from_value(manifest["publication"].clone())
        .map_err(|e| format!("invalid publication manifest proof: {e}"))?;
    Ok((
        PublicationMetadata {
            decision,
            manifest,
            decision_proof,
            manifest_proof,
            decision_path,
            manifest_path,
            trailers,
        },
        entries,
    ))
}
/// Immutable protected directory snapshot; this data alone grants no publication authority.
pub fn publication_base_directory(
    catalog: &Catalog,
    revision: &str,
) -> Result<BTreeMap<String, String>, String> {
    let at = Catalog::at_revision(&catalog.root, revision)?
        .ok_or("publication source has no Catalog")?;
    #[cfg(test)]
    tests::publication_local_reads::record(catalog, revision, "catalog");
    publication_base_context(catalog, &at)?;
    let entries = tree(catalog, revision)?;
    publication_base_directory_in_tree(catalog, &at, &entries)
}
fn publication_base_context(catalog: &Catalog, at: &Catalog) -> Result<(), String> {
    if at.config.project_id() != catalog.config.project_id()
        || at.config.docs_dir() != catalog.config.docs_dir()
        || crate::recovery_outbox::RecoveryContext::extract(at)?
            != crate::recovery_outbox::RecoveryContext::extract(catalog)?
    {
        return Err("publication source context changed".into());
    }
    Ok(())
}
// Reuse local decoded inputs only. Every protected mode and physical blob type
// is still checked; this map never certifies a tag or a completed publication.
fn publication_base_directory_in_tree(
    catalog: &Catalog,
    at: &Catalog,
    entries: &GitTreeEntries,
) -> Result<BTreeMap<String, String>, String> {
    let prefix = format!("{}/", catalog.config.docs_dir());
    let mut result = BTreeMap::new();
    for (path, (mode, oid)) in entries {
        let directory = path
            .strip_prefix(&prefix)
            .is_some_and(|p| !p.starts_with("state/") && !p.starts_with("artifacts/"));
        let protected =
            directory || path == crate::config::CONFIG_PATH || at.protected_paths.contains(path);
        if protected {
            contract::file_mode(mode)?;
            if git(catalog, &["cat-file", "-t", oid])? != "blob" {
                return Err("protected publication path is not a blob".into());
            }
            result.insert(path.clone(), format!("{mode} {oid}"));
        }
    }
    Ok(result)
}
fn check_publication_payload(
    catalog: &Catalog,
    history: &mut HistoryCandidate,
    commit: &str,
) -> Result<PublicationMetadata, String> {
    contract::object_id(commit)?;
    let (meta, candidate_tree) = read_publication_metadata_with_tree(catalog, commit)?;
    let d = &meta.decision_proof;
    let m = &meta.manifest_proof;
    let b = &m.binding;
    let f = &m.freeze;
    let t = &meta.trailers;
    contract::object_id(&b.draft_revision)?;
    let parents = git(catalog, &["rev-list", "--parents", "-n", "1", commit])?;
    let parents = parents.split_whitespace().skip(1).collect::<Vec<_>>();
    if parents.len() != 1 {
        return Err("publication must have one actual authority parent".into());
    }
    let parent = parents[0];
    if d.schema != 1
        || m.schema != 1
        || m.writer_epoch == 0
        || d.publication_id != m.publication_id
        || d.publication_id != m.approval_eid
        || d.approval.approval_eid != m.approval_eid
        || d.approval.binding != *b
        || d.approval.verdict != contract::baseline::Verdict::Approved
        || m.decision_path != meta.decision_path
        || b.project_id != catalog.config.project_id()
        || b.object_id != f.draft_id
        || b.target_baseline != f.target_baseline
        || f.project_id != b.project_id
        || f.docs_dir != catalog.config.docs_dir()
        || t.baseline != b.target_baseline
        || t.approval != m.approval_eid
        || t.publication != m.publication_id
        || t.content_hash != b.content_hash
        || t.epoch != m.writer_epoch
    {
        return Err("publication manifest/Decision/trailers binding mismatch".into());
    }
    accepted(&d.approval_event)?;
    accepted(&m.prepared_event)?;
    contract::validate_recorded_data(&b.project_id, "baseline.approved", &d.approval_event.data)?;
    contract::validate_recorded_data(
        &b.project_id,
        "baseline.publication_prepared",
        &m.prepared_event.data,
    )?;
    let approval: BaselineReviewRecorded =
        serde_json::from_value(d.approval_event.data.clone()).map_err(|e| e.to_string())?;
    let preparation: PublicationPrepared =
        serde_json::from_value(m.prepared_event.data.clone()).map_err(|e| e.to_string())?;
    event_envelope(
        &d.approval_event,
        "baseline.approved",
        &d.approval.actor,
        &approval.request_body.host,
        "L1",
        &b.object_id,
        &[],
    )?;
    if approval.approval != d.approval || d.approval_event.eid != d.approval.approval_eid {
        return Err("publication approval is not exact original App receipt".into());
    }
    if m.prepared_event.guards.len() != 1 {
        return Err("publication preparation requires one main fence".into());
    }
    let fence = &m.prepared_event.guards[0];
    event_envelope(
        &m.prepared_event,
        "baseline.publication_prepared",
        "process:towermd-baseline",
        &m.publication_host,
        "L1",
        &m.publication_id,
        &m.prepared_event.guards,
    )?;
    if fence.name != "main"
        || fence.epoch != m.writer_epoch
        || preparation.publication_id != m.publication_id
        || preparation.approval_eid != m.approval_eid
        || preparation.binding != *b
        || preparation.authority_revision != parent
        || preparation.request_body.host != m.publication_host
        || preparation.content_hash != b.content_hash
        || m.prepared_event.eid
            != request_event_eid(
                &preparation.p3_request.request_id,
                "baseline.publication_prepared",
            )?
        || d.approval_event.seq >= m.prepared_event.seq
    {
        return Err("publication original preparation differs from parent/fence/binding".into());
    }
    // An embedded original can stand alone after outbox loss, but cannot conflict with known history.
    for original in [&d.approval_event, &m.prepared_event] {
        insert_original(&mut history.originals, original.clone())?;
    }
    let parent_tree = tree(catalog, parent)?;
    if parent_tree.contains_key(&meta.decision_path)
        || parent_tree.contains_key(&meta.manifest_path)
    {
        return Err("publication audit path already occupied".into());
    }
    if !ancestor(catalog, &f.source_revision, parent)? {
        return Err("publication protected base_directory changed".into());
    }
    let parent_catalog =
        Catalog::at_revision(&catalog.root, parent)?.ok_or("publication source has no Catalog")?;
    #[cfg(test)]
    tests::publication_local_reads::record(catalog, parent, "catalog");
    publication_base_context(catalog, &parent_catalog)?;
    let parent_directory =
        publication_base_directory_in_tree(catalog, &parent_catalog, &parent_tree)?;
    if parent_directory != f.base_directory {
        return Err("publication protected base_directory changed".into());
    }
    // Full commit OID equality selects local reuse, never a new source gate.
    // Different commits retain the original independent physical source read.
    if f.source_revision != parent
        && publication_base_directory(catalog, &f.source_revision)? != f.base_directory
    {
        return Err("publication protected base_directory changed".into());
    }
    let mut texts = BTreeMap::new();
    let mut objects = BTreeMap::new();
    let mut affects = BTreeSet::new();
    let mut expected = BTreeSet::from([meta.decision_path.clone(), meta.manifest_path.clone()]);
    let at = Catalog::at_revision(&catalog.root, commit)?
        .ok_or("publication candidate lacks Catalog")?;
    #[cfg(test)]
    tests::publication_local_reads::record(catalog, commit, "catalog");
    let prefix = format!("{}/", f.docs_dir);
    let canvas_path = format!("{}/tower.canvas", f.docs_dir);
    for (path, file) in &f.files {
        if !path.starts_with(&prefix)
            || path.starts_with(&format!("{}state/", prefix))
            || path.starts_with(&format!("{}artifacts/", prefix))
            || path == &meta.decision_path
            || path == &meta.manifest_path
            || path == &format!("{}AGENTS.md", prefix)
        {
            return Err("publication payload path is not a draft object/canvas".into());
        }
        let (mode, oid) = candidate_tree
            .get(path)
            .ok_or("publication payload missing")?;
        if mode != &file.mode {
            return Err("publication payload mode differs from freeze".into());
        }
        contract::file_mode(mode)?;
        let text = blob_text(catalog, oid)?;
        if contract::sha256(text.as_bytes()) != file.sha256 {
            return Err("publication payload bytes differ from freeze".into());
        }
        if path == &canvas_path {
            let decoded = crate::canvas::CanvasDocument::decode(&text)?;
            if !decoded.diagnostics().is_empty() {
                return Err("publication canvas has raw-byte diagnostics".into());
            }
            let canvas = crate::canvas::CanvasDocument::from_value(
                serde_json::from_str(&text).map_err(|e| e.to_string())?,
            )?;
            for entry in canvas.nodes().iter().chain(canvas.edges()) {
                if let Some(id) = entry.raw["towermd"]["obj"].as_str() {
                    if !at.objects.contains_key(id) {
                        return Err("canvas references missing publication object".into());
                    }
                    if entry.raw.get("fromNode").is_some()
                        && at.objects[id].doc_type() != Some("Interface")
                    {
                        return Err("canvas edge does not reference Interface".into());
                    }
                    if entry.raw["type"] == "file"
                        && entry.raw["file"].as_str() != at.object_paths.get(id).map(String::as_str)
                    {
                        return Err("canvas file/object path binding differs".into());
                    }
                }
            }
        } else {
            let doc = crate::okf::Document::parse(&text)?;
            let id = doc.get_str("id").ok_or("publication object lacks id")?;
            let proposal = doc.doc_type() == Some("Proposal");
            let allowed_type = proposal
                || crate::types::lookup(doc.doc_type().unwrap_or(""))
                    .is_some_and(|t| t.default_layer.is_slow());
            let allowed_layer = if proposal {
                matches!(
                    layer_of(&doc),
                    LayerResolution::Valid(crate::types::Layer::L4)
                        | LayerResolution::Missing(Some(crate::types::Layer::L4))
                )
            } else {
                matches!(layer_of(&doc), LayerResolution::Valid(layer) | LayerResolution::Missing(Some(layer)) if layer.is_slow())
            };
            if !allowed_type || !allowed_layer || at.object_paths.get(id) != Some(path) {
                return Err("publication object id/path/type/layer mismatch".into());
            }
            if proposal && parent_tree.contains_key(path) {
                return Err("proposal publication carrier must be a new object".into());
            }
            if let Some((old_mode, old_oid)) = parent_tree.get(path) {
                contract::file_mode(old_mode)?;
                let old = crate::okf::Document::parse(&blob_text(catalog, old_oid)?)?;
                if old.get_str("id") != Some(id) || old.doc_type() != doc.doc_type() {
                    return Err("publication overwrites unrelated document".into());
                }
            }
            if objects.insert(id.to_owned(), file.sha256.clone()).is_some() {
                return Err("duplicate publication payload object".into());
            }
            if parent_tree.get(path) != Some(&(mode.clone(), oid.clone())) {
                affects.insert(id.to_owned());
            }
        }
        if parent_tree.get(path) != candidate_tree.get(path) {
            expected.insert(path.clone());
        }
        texts.insert(path.clone(), text);
    }
    for path in &f.deletions {
        if !path.starts_with(&prefix)
            || path.starts_with(&format!("{}state/", prefix))
            || path.starts_with(&format!("{}artifacts/", prefix))
            || path == &format!("{}AGENTS.md", prefix)
            || path == &canvas_path
        {
            return Err("publication deletion is not a draft object".into());
        }
        let (mode, oid) = parent_tree
            .get(path)
            .ok_or("publication deletion has no original object")?;
        contract::file_mode(mode)?;
        if candidate_tree.contains_key(path)
            || path == &meta.decision_path
            || path == &meta.manifest_path
        {
            return Err("publication deletion not applied".into());
        }
        let doc = crate::okf::Document::parse(&blob_text(catalog, oid)?)?;
        let id = doc
            .get_str("id")
            .ok_or("cannot delete non-object document")?;
        if parent_catalog.object_paths.get(id) != Some(path)
            || !crate::types::lookup(doc.doc_type().unwrap_or(""))
                .is_some_and(|t| t.default_layer.is_slow())
            || !matches!(layer_of(&doc), LayerResolution::Valid(layer) | LayerResolution::Missing(Some(layer)) if layer.is_slow())
        {
            return Err("publication deletes unrelated path".into());
        }
        affects.insert(id.into());
        expected.insert(path.clone());
    }
    if contract::baseline::publication_content_hash(f, &texts)? != b.content_hash {
        return Err("publication exact frozen content_hash mismatch".into());
    }
    crate::proposals::validate_publication_carrier(f, &texts)?;
    let raw = git_bytes(
        catalog,
        &[
            "diff-tree",
            "--no-commit-id",
            "-r",
            "--raw",
            "-z",
            "--full-index",
            "--abbrev=0",
            "--no-abbrev",
            "--no-renames",
            parent,
            commit,
            "--",
        ],
    )?;
    let changes = crate::task_gates::parse_raw_z(&raw)
        .map_err(|e| format!("invalid publication diff: {e:?}"))?;
    let actual = changes
        .into_iter()
        .map(|c| String::from_utf8(c.path).map_err(|e| e.to_string()))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual != expected {
        return Err("publication diff contains unapproved/missing paths".into());
    }
    let actor = &d.approval.actor;
    let time = &d.approval_event.t;
    if meta.decision["type"] != "Decision"
        || meta.decision["id"]
            != format!(
                "publication-{}",
                contract::sha256(m.approval_eid.as_bytes())
            )
        || meta.decision["layer"] != "L1"
        || meta.decision["context"] != format!("baseline publication {}", b.target_baseline)
        || meta.decision["options"] != json!(["approve", "reject"])
        || meta.decision["decision"] != "approve"
        || meta.decision["rationale"] != d.approval.comment
        || meta.decision["affects"] != json!(affects.into_iter().collect::<Vec<_>>())
        || meta.decision["supersedes"] != json!([])
        || meta.decision["class"] != "baseline-publication"
    {
        return Err("publication Decision audit fields mismatch".into());
    }
    for (value, expected_by) in [
        (&meta.decision["generated"], actor.as_str()),
        (&meta.manifest["generated"], "process:towermd-baseline"),
    ] {
        if value["by"] != expected_by || value["at"] != *time {
            return Err("publication generated audit binding mismatch".into());
        }
    }
    let verified = &meta.decision["verified"];
    let rows = verified
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![verified.clone()]);
    if !rows.iter().any(|v| v["by"] == *actor && v["at"] == *time) {
        return Err("publication verified audit binding mismatch".into());
    }
    if meta.manifest["type"] != "Baseline"
        || meta.manifest["id"] != b.target_baseline.strip_prefix("towermd/").unwrap_or("")
        || meta.manifest["layer"] != "L1"
        || meta.manifest["objects"] != json!(objects)
        || meta.manifest["approved_by"] != *actor
        || meta.manifest["approved_at"] != *time
        || meta.manifest["git_tag"] != b.target_baseline
    {
        return Err("publication Baseline manifest fields mismatch".into());
    }
    Ok(meta)
}

fn prove_publication_candidate(
    catalog: &Catalog,
    history: &mut HistoryCandidate,
    commit: &str,
    visited: &mut BTreeSet<String>,
) -> Result<VerifiedPublication, String> {
    contract::object_id(commit)?;
    if !ancestor(catalog, commit, &catalog.revision)? || !visited.insert(commit.into()) {
        return Err("publication is unreachable or cyclic".into());
    }
    let meta = check_publication_payload(catalog, history, commit)?;
    let m = &meta.manifest_proof;
    let b = &m.binding;
    let parent = git(catalog, &["rev-parse", &format!("{commit}^")])?;
    let number = baseline_number(&b.target_baseline)?;
    let tag_commit = peel_baseline(catalog, &b.target_baseline)?;
    if tag_commit != commit {
        return Err("publication exact tag peels to different commit".into());
    }
    match number {
        0 if m.parent_baseline.is_none() => {}
        0 => return Err("B0 must have null parent_baseline".into()),
        n => {
            let prior = format!("towermd/B{}", n - 1);
            if m.parent_baseline.as_ref() != Some(&prior) {
                return Err("publication baseline chain has gap".into());
            }
            let prior_commit = peel_baseline(catalog, &prior)?;
            if !ancestor(catalog, &prior_commit, &parent)? {
                return Err("publication previous baseline is not on actual parent chain".into());
            }
            prove_publication_candidate(catalog, history, &prior_commit, visited)?;
        }
    }
    let mut diagnostics = Vec::new();
    for reference in git(
        catalog,
        &["for-each-ref", "--format=%(refname)", "refs/tags/towermd/"],
    )?
    .lines()
    {
        let Some(target) = reference.strip_prefix("refs/tags/") else {
            continue;
        };
        if !target.starts_with("towermd/B") {
            continue;
        }
        if baseline_number(target).is_err() {
            diagnostics.push(format!("invalid baseline tag: {target}"));
            continue;
        }
        if target != b.target_baseline && peel_baseline(catalog, target).is_ok_and(|c| c == commit)
        {
            diagnostics.push(format!("unverified additional baseline tag: {target}"));
        }
    }
    let result = BaselineCreatedResult {
        commit: commit.into(),
        publication_id: m.publication_id.clone(),
        approval_eid: m.approval_eid.clone(),
        content_hash: b.content_hash.clone(),
        target: b.target_baseline.clone(),
        tag: b.target_baseline.clone(),
        tags: vec![b.target_baseline.clone()],
    };
    let body = RequestBody {
        project_id: b.project_id.clone(),
        actor: "process:towermd-baseline".into(),
        host: m.publication_host.clone(),
        run: None,
        action: BaselineCreatedAction::RecordBaseline {
            result: result.clone(),
        },
    };
    let request_id = crate::events::derived_eid(commit, "baseline.created.request")?;
    let hash = contract::canonical_hash(&body)?;
    let data = BaselineCreated {
        result: result.clone(),
        request_body: body,
        p3_request: RequestMeta {
            request_id: request_id.clone(),
            body_hash: hash.clone(),
        },
        p3_receipt: RequestReceipt {
            request_id,
            body_hash: hash,
            result_ref: result,
        },
    };
    let data = serde_json::to_value(data).map_err(|e| e.to_string())?;
    contract::validate_recorded_data(&b.project_id, "baseline.created", &data)?;
    Ok(VerifiedPublication {
        diagnostics,
        event: NewEvent {
            eid: crate::events::derived_eid(commit, "baseline.created")?,
            host: m.publication_host.clone(),
            actor: "process:towermd-baseline".into(),
            kind: "baseline.created".into(),
            layer: "L1".into(),
            obj: m.publication_id.clone(),
            data,
            guards: vec![],
        },
    })
}
fn peel_baseline(catalog: &Catalog, target: &str) -> Result<String, String> {
    baseline_number(target)?;
    let reference = format!("refs/tags/{target}");
    let record = git(
        catalog,
        &[
            "for-each-ref",
            "--format=%(objectname)%00%(symref)",
            &reference,
        ],
    )?;
    if record.is_empty() {
        return Err(format!("pending publication: missing exact tag {target}"));
    }
    if !record
        .split_once('\0')
        .is_some_and(|(_, sym)| sym.is_empty())
    {
        return Err("baseline tag is symbolic".into());
    }
    git(
        catalog,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )
}

#[cfg(test)]
#[path = "recovery_outer_tests.rs"]
mod outer_tests;
#[cfg(test)]
#[path = "recovery_git_tests.rs"]
mod tests;

// Finite producer repair capability. Only the selected persisted actual outer
// schedule can supply a compacted finish/gate receipt; no raw pending/body or
// candidate.originals role can construct one.
pub(crate) struct VerifiedFinishReceipt {
    project_id: String,
    event: Event,
}
impl VerifiedFinishReceipt {
    pub(crate) fn event_for(
        &self,
        project_id: &str,
        original: &NewEvent,
    ) -> Result<&Event, String> {
        if self.project_id != project_id || !finish_receipt_kind(original) {
            return Err("finish receipt project/kind mismatch".into());
        }
        crate::recovery_outbox::validate_actual(original, &self.event)?;
        Ok(&self.event)
    }
}
fn finish_receipt_kind(original: &NewEvent) -> bool {
    crate::finish::is_event(&original.kind)
        || (original.kind == "evidence.recorded"
            && original.actor == "process:towermd-gates"
            && original.data["evidence"]["kind"] == "gate")
}
impl VerifiedRecoveryHistory {
    pub(crate) fn finish_receipt(
        &self,
        catalog: &Catalog,
        original: &NewEvent,
    ) -> Result<Option<VerifiedFinishReceipt>, String> {
        if !finish_receipt_kind(original) {
            return Err("not a finite finish producer event".into());
        }
        let through = self
            .sources
            .clone()
            .select()?
            .map_or(0, |s| s.snapshot.through_seq);
        let mut found: Option<Event> = None;
        for actual in self
            .accepted_outer_events(catalog, through)?
            .into_iter()
            .filter(|e| e.eid == original.eid)
        {
            crate::recovery_outbox::validate_actual(original, actual)?;
            if found.as_ref().is_some_and(|old| old != actual) {
                return Err("conflicting actual archived finish receipts".into());
            }
            found = Some(actual.clone());
        }
        Ok(found.map(|event| VerifiedFinishReceipt {
            project_id: catalog.config.project_id().into(),
            event,
        }))
    }
}
