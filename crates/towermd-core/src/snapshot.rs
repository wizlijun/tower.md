use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::hub::{Event, Fence};
use crate::types;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StateProjection {
    pub objects: BTreeMap<String, BTreeMap<String, Value>>,
    pub lease_summaries: BTreeMap<String, Fence>,
}

impl StateProjection {
    pub fn apply_recorded(&mut self, event: &Event) -> Result<(), String> {
        if event.stale {
            return Ok(());
        }
        if event.kind == "hub.rebuilt" {
            self.lease_summaries.clear();
            return Ok(());
        }
        if matches!(
            event.kind.as_str(),
            "task.state" | "task.claimed" | "task.released"
        ) {
            let (task_id, task) = business_object(&event.data["task"], "Task")?;
            let run = match event.data.get("run").filter(|run| !run.is_null()) {
                Some(value) => {
                    let (id, _) = business_object(value, "Run")?;
                    let run = recorded_run(event, value)?;
                    Some((id, run))
                }
                None if event.kind == "task.claimed" => {
                    return Err(format!("claim event {} has no Run", event.seq));
                }
                None => None,
            };
            self.objects
                .entry("Task".into())
                .or_default()
                .insert(task_id, task);
            if let Some((id, run)) = run {
                self.objects
                    .entry("Run".into())
                    .or_default()
                    .insert(id, run);
            }
        }
        for lease in event.acquired_leases() {
            self.lease_summaries
                .insert(lease.name.clone(), lease.fence());
        }
        for name in event.released_lease_names() {
            self.lease_summaries.remove(&name);
        }
        Ok(())
    }
}

/// Validate the business binding separately from the opaque hub envelope.
pub(crate) fn recorded_run(event: &Event, value: &Value) -> Result<Value, String> {
    let mut run = value.clone();
    if event.kind == "task.claimed" {
        let fences: Vec<Fence> = if event.legacy {
            event.updated_leases().iter().map(|l| l.fence()).collect()
        } else {
            serde_json::from_value(run["leases"].clone())
                .map_err(|e| format!("claim {} has invalid Run fences: {e}", event.seq))?
        };
        crate::hub::validate_names(fences.iter().map(|f| f.name.as_str()))
            .map_err(|e| e.to_string())?;
        for fence in &fences {
            crate::hub::validate_fence(fence).map_err(|e| e.to_string())?;
            if run["id"].as_str() != Some(&fence.holder) {
                return Err("claim fence holder differs from Run ID".into());
            }
        }
        if !event.legacy
            && (fences != event.guards || event.epoch != fences.first().map(|f| f.epoch))
        {
            return Err("claim Run fences differ from event guards".into());
        }
        let task = &event.data["task"];
        let module = task["module"]
            .as_str()
            .ok_or("claim Task module must be a string")?;
        let touches: Vec<String> = match task.get("touches") {
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|e| format!("claim Task touches must be module IDs: {e}"))?,
            None => vec![],
        };
        let mut declared = BTreeSet::new();
        for id in std::iter::once(module).chain(touches.iter().map(String::as_str)) {
            validate_token(id)?;
            declared.insert(format!("module:{id}"));
        }
        if declared != fences.iter().map(|f| f.name.clone()).collect() {
            return Err("claim fences differ from Task module/touches declarations".into());
        }
        run["leases"] = serde_json::to_value(fences).map_err(|e| e.to_string())?;
    }
    Ok(run)
}

fn business_object(value: &Value, kind: &str) -> Result<(String, Value), String> {
    if types::lookup(kind).is_none() || value["type"].as_str() != Some(kind) {
        return Err(format!("expected registered object type {kind}"));
    }
    let id = value["id"].as_str().ok_or("object ID must be a string")?;
    validate_token(id)?;
    Ok((id.to_owned(), value.clone()))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SequenceRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub format_version: u32,
    pub project_id: String,
    pub through_seq: u64,
    pub writer_epoch: u64,
    pub epoch_high_water: u64,
    pub projection: StateProjection,
    #[serde(
        default,
        rename = "receipts",
        deserialize_with = "deserialize_receipts",
        skip_serializing
    )]
    pub(crate) legacy_receipts: BTreeMap<String, u64>,
    pub omitted: Vec<SequenceRange>,
    pub recovery_gaps: Vec<SequenceRange>,
}

impl Snapshot {
    pub fn extend(
        base: Option<&Self>,
        events: &[Event],
        through_seq: u64,
        writer_epoch: u64,
    ) -> Result<Self, String> {
        let base = base.ok_or("base snapshot required; use from_events to supply project_id")?;
        base.validate_metadata()?;
        if through_seq < base.through_seq
            || through_seq > validate_sequence(events, base.through_seq)?
        {
            return Err("snapshot exceeds confirmed contiguous sequence or precedes base".into());
        }
        if events
            .iter()
            .any(|event| base.legacy_receipts.contains_key(&event.eid))
        {
            return Err("tail event eid already exists in base snapshot".into());
        }
        let mut snapshot = base.clone();
        snapshot.normalize();
        snapshot.through_seq = through_seq;
        snapshot.writer_epoch = writer_epoch;
        for event in events.iter().take_while(|event| event.seq <= through_seq) {
            snapshot.projection.apply_recorded(event)?;
            snapshot.epoch_high_water = snapshot.epoch_high_water.max(issued_epoch(event));

            if event.is_pure_heartbeat() {
                match snapshot.omitted.last_mut() {
                    Some(range) if range.end.checked_add(1) == Some(event.seq) => {
                        range.end = event.seq;
                    }
                    _ => snapshot.omitted.push(SequenceRange {
                        start: event.seq,
                        end: event.seq,
                    }),
                }
            }
        }
        snapshot.validate_writer()?;
        Ok(snapshot)
    }

    pub fn from_events(
        project_id: &str,
        events: &[Event],
        through_seq: u64,
        writer_epoch: u64,
    ) -> Result<Self, String> {
        let base = Self {
            format_version: 2,
            project_id: project_id.into(),
            through_seq: 0,
            writer_epoch: 0,
            epoch_high_water: 0,
            projection: StateProjection::default(),
            legacy_receipts: BTreeMap::new(),
            omitted: Vec::new(),
            recovery_gaps: Vec::new(),
        };
        Self::extend(Some(&base), events, through_seq, writer_epoch)
    }

    pub fn validate_archive(&self, project_id: &str, archive: &[Event]) -> Result<(), String> {
        self.validate_metadata()?;
        validate_token(project_id)?;
        if !matches!(self.format_version, 1 | 2) || self.project_id != project_id {
            return Err("snapshot project or format version mismatch".into());
        }
        self.validate_writer()?;
        let mut coverage = self.omitted.clone();
        coverage.extend(self.recovery_gaps.iter().cloned());
        let mut replay = StateProjection::default();
        let mut previous = 0;
        let mut epoch_high_water = 0;
        let mut seen = BTreeSet::new();
        for event in archive {
            if event.seq <= previous
                || !seen.insert(&event.eid)
                || (self.format_version == 1
                    && self.legacy_receipts.get(&event.eid) != Some(&event.seq))
            {
                return Err("archive is out of order or inconsistent with receipts".into());
            }
            if event.is_pure_heartbeat() {
                return Err("archive contains a pure heartbeat".into());
            }
            previous = event.seq;
            epoch_high_water = epoch_high_water.max(issued_epoch(event));
            replay.apply_recorded(event)?;
            coverage.push(SequenceRange {
                start: event.seq,
                end: event.seq,
            });
        }
        validate_coverage(coverage, self.through_seq)?;
        if epoch_high_water > self.epoch_high_water {
            return Err("epoch high water is below recorded lease epochs".into());
        }
        if replay != self.projection {
            return Err("archive replay differs from snapshot projection".into());
        }
        Ok(())
    }

    pub fn validate_metadata(&self) -> Result<(), String> {
        validate_token(&self.project_id)?;
        if !matches!(self.format_version, 1 | 2)
            || self.writer_epoch > self.epoch_high_water
            || (self.through_seq > 0 && self.writer_epoch == 0)
        {
            return Err("invalid snapshot format or epoch metadata".into());
        }
        if self.format_version == 1 {
            let mut coverage = self.recovery_gaps.clone();
            for (eid, seq) in &self.legacy_receipts {
                validate_token(eid)?;
                coverage.push(SequenceRange {
                    start: *seq,
                    end: *seq,
                });
            }
            validate_coverage(coverage, self.through_seq)?;
        } else if !self.legacy_receipts.is_empty() {
            return Err("v2 snapshot must not contain receipts".into());
        }
        let mut ranges: Vec<_> = self.omitted.iter().chain(&self.recovery_gaps).collect();
        ranges.sort_unstable_by_key(|range| range.start);
        let mut previous = 0;
        for range in ranges {
            if range.start == 0
                || range.start <= previous
                || range.start > range.end
                || range.end > self.through_seq
            {
                return Err("invalid or overlapping snapshot omission ranges".into());
            }
            previous = range.end;
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.format_version = 2;
        self.legacy_receipts.clear();
    }

    fn validate_writer(&self) -> Result<(), String> {
        if self.writer_epoch == 0
            || self.writer_epoch > self.epoch_high_water
            || !self
                .projection
                .lease_summaries
                .get("main")
                .is_some_and(|lease| lease.name == "main" && lease.epoch == self.writer_epoch)
        {
            return Err("writer epoch must match the recorded main lease summary".into());
        }
        Ok(())
    }
}

pub fn archive_events(events: &[Event], after: u64, through: u64) -> Result<Vec<Event>, String> {
    let base = match events.first() {
        Some(event) => event
            .seq
            .checked_sub(1)
            .ok_or("sequence must be positive")?,
        None => after,
    };
    let last = validate_sequence(events, base)?;
    if after > through || base > after || through > last {
        return Err("archive range is not covered by contiguous input".into());
    }
    Ok(events
        .iter()
        .filter(|event| event.seq > after && event.seq <= through && !event.is_pure_heartbeat())
        .cloned()
        .collect())
}

fn issued_epoch(event: &Event) -> u64 {
    if event.stale {
        return 0;
    }
    event
        .acquired_leases()
        .iter()
        .map(|lease| lease.epoch)
        .max()
        .unwrap_or(0)
}

fn validate_token(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err("identifier must be a nonempty token".into());
    }
    Ok(())
}

fn validate_sequence(events: &[Event], base: u64) -> Result<u64, String> {
    let mut previous = base;
    let mut seen = BTreeSet::new();
    for event in events {
        validate_token(&event.eid)?;
        if previous.checked_add(1) != Some(event.seq) || !seen.insert(&event.eid) {
            return Err("events must have contiguous sequences and unique eids".into());
        }
        previous = event.seq;
    }
    Ok(previous)
}

fn validate_coverage(mut ranges: Vec<SequenceRange>, through: u64) -> Result<(), String> {
    ranges.sort_unstable_by_key(|range| range.start);
    let mut previous = 0_u64;
    for range in ranges {
        if range.start == 0
            || range.start > range.end
            || range.end > through
            || previous.checked_add(1) != Some(range.start)
        {
            return Err("sequence coverage has an overlap, gap, or invalid range".into());
        }
        previous = range.end;
    }
    if previous != through {
        return Err("sequence coverage does not reach snapshot boundary".into());
    }
    Ok(())
}

fn deserialize_receipts<'de, D>(deserializer: D) -> Result<BTreeMap<String, u64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ReceiptsVisitor;

    impl<'de> Visitor<'de> for ReceiptsVisitor {
        type Value = BTreeMap<String, u64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("unique eid to sequence receipts")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut receipts = BTreeMap::new();
            while let Some((eid, seq)) = map.next_entry::<String, u64>()? {
                if receipts.insert(eid, seq).is_some() {
                    return Err(serde::de::Error::custom("duplicate receipt eid"));
                }
            }
            Ok(receipts)
        }
    }

    deserializer.deserialize_map(ReceiptsVisitor)
}
