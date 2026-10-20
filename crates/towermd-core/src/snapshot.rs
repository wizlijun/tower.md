//! Checked Git snapshot envelope. The caller supplies the one business reducer;
//! a stored projection is never accepted without replaying its canonical archive.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{hub::Event, repo};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceRange {
    pub first: u64,
    pub last: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryJump {
    pub source_seq: u64,
    pub source_epoch: u64,
    pub seq_boundary: u64,
    pub epoch_boundary: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotState {
    pub version: u32,
    pub project_id: String,
    pub seq: u64,
    /// Main writer's fencing epoch, also present in the commit trailer.
    pub epoch: u64,
    /// Includes module lease epochs, so recovery never reissues any old fence.
    pub epoch_high_water: u64,
    pub omitted_heartbeats: Vec<SequenceRange>,
    pub recovery_jumps: Vec<RecoveryJump>,
    pub projection: Value,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub state: SnapshotState,
    pub events: Vec<Event>,
}

impl Snapshot {
    pub fn build(
        project_id: &str,
        previous: Option<&Snapshot>,
        tail: &[Event],
        contiguous_seq: u64,
        epoch: u64,
        epoch_high_water: u64,
        reduce: impl Fn(&[Event]) -> Result<Value, String>,
    ) -> Result<Self, String> {
        let mut state = previous.map(|s| s.state.clone()).unwrap_or(SnapshotState {
            version: 1,
            project_id: project_id.into(),
            seq: 0,
            epoch: 0,
            epoch_high_water: 0,
            omitted_heartbeats: vec![],
            recovery_jumps: vec![],
            projection: Value::Null,
        });
        if state.project_id != project_id
            || contiguous_seq < state.seq
            || epoch < state.epoch
            || epoch_high_water < state.epoch_high_water
        {
            return Err("快照来源或水位回退".into());
        }
        let mut events = previous.map(|s| s.events.clone()).unwrap_or_default();
        let mut next = state.seq;
        for event in tail.iter().filter(|e| e.seq <= contiguous_seq) {
            next = next.checked_add(1).ok_or("快照序号溢出")?;
            if event.seq != next {
                return Err("快照不能跨越未连续定序的事件".into());
            }
            if event.is_pure_heartbeat() {
                match state.omitted_heartbeats.last_mut() {
                    Some(range) if range.last.checked_add(1) == Some(event.seq) => {
                        range.last = event.seq;
                    }
                    _ => state.omitted_heartbeats.push(SequenceRange {
                        first: event.seq,
                        last: event.seq,
                    }),
                }
            } else {
                events.push(event.clone());
            }
        }
        if next != contiguous_seq {
            return Err("快照缺少连续前缀中的事件".into());
        }
        state.seq = contiguous_seq;
        state.epoch = epoch;
        state.epoch_high_water = epoch_high_water;
        state.projection = reduce(&events)?;
        let result = Self { state, events };
        result.validate(reduce)?;
        Ok(result)
    }

    /// Stage a recovery boundary in Git before installing the rebuilt Hub.
    /// No event is invented and no old lease is reactivated.
    pub fn recovery_boundary(
        &self,
        seq_boundary: u64,
        epoch_boundary: u64,
        reduce: impl Fn(&[Event]) -> Result<Value, String>,
    ) -> Result<Self, String> {
        let mut result = self.clone();
        result.state.recovery_jumps.push(RecoveryJump {
            source_seq: self.state.seq,
            source_epoch: self.state.epoch_high_water,
            seq_boundary,
            epoch_boundary,
        });
        result.state.seq = seq_boundary;
        result.state.epoch = epoch_boundary;
        result.state.epoch_high_water = epoch_boundary;
        result.validate(reduce)?;
        Ok(result)
    }

    pub fn receipts(&self) -> BTreeMap<String, u64> {
        let mut receipts = BTreeMap::new();
        for event in &self.events {
            receipts.insert(event.eid.clone(), event.seq);
            if let Ok(Some(original)) = recovered_original(event) {
                receipts.insert(original.eid, original.seq);
            }
        }
        receipts
    }

    pub fn validate(
        &self,
        reduce: impl Fn(&[Event]) -> Result<Value, String>,
    ) -> Result<(), String> {
        let state = &self.state;
        if state.version != 1
            || state.project_id.trim().is_empty()
            || state.epoch > state.epoch_high_water
        {
            return Err("快照格式或 epoch 无效".into());
        }
        let mut spans = Vec::new();
        let mut eids = BTreeSet::new();
        let mut originals: BTreeMap<String, Event> = BTreeMap::new();
        let mut prior_event_seq = 0;
        let mut completed_recoveries = BTreeSet::new();
        for event in &self.events {
            if event.seq <= prior_event_seq
                || !eids.insert(&event.eid)
                || event.is_pure_heartbeat()
                || event
                    .epoch
                    .is_some_and(|epoch| epoch > state.epoch_high_water)
            {
                return Err("归档事件重复、乱序、含心跳或越过 epoch 水位".into());
            }
            chrono::DateTime::parse_from_rfc3339(&event.t).map_err(|_| "归档事件时间无效")?;
            prior_event_seq = event.seq;
            spans.push((event.seq, event.seq));
            if event.kind == "recovery.completed" {
                let boundary: RecoveryJump = serde_json::from_value(event.data.clone())
                    .map_err(|_| "恢复完成事件缺少精确跳号边界")?;
                let index = state
                    .recovery_jumps
                    .iter()
                    .position(|jump| jump == &boundary)
                    .ok_or("恢复完成事件不属于快照跳号边界")?;
                if event.stale
                    || event.actor != "process:towermd-recovery"
                    || event.layer != "L5"
                    || event.obj != state.project_id
                    || event.seq <= boundary.seq_boundary
                    || state
                        .recovery_jumps
                        .get(index + 1)
                        .is_some_and(|next| event.seq > next.source_seq)
                    || event.guards.len() != 1
                    || event.guards[0].name != "main"
                    || event.guards[0].epoch <= boundary.epoch_boundary
                    || event.epoch != Some(event.guards[0].epoch)
                    || !completed_recoveries.insert(boundary.seq_boundary)
                {
                    return Err("恢复完成事件的边界、身份或租约无效".into());
                }
            }
            let original = recovered_original(event)?.unwrap_or_else(|| event.clone());
            if original.kind=="project.paused" && original.obj!=state.project_id {
                return Err("项目暂停原件属于其他项目".into());
            }
            if let Some(previous) = originals.insert(original.eid.clone(), original.clone()) {
                if previous != original {
                    return Err("同一事件 ID 的恢复原件冲突".into());
                }
            }
        }
        for range in &state.omitted_heartbeats {
            if range.first == 0 || range.last < range.first {
                return Err("心跳省略区间无效".into());
            }
            spans.push((range.first, range.last));
        }
        let mut prior_jump = 0;
        for jump in &state.recovery_jumps {
            if jump.source_seq < prior_jump
                || jump
                    .source_seq
                    .checked_add(1 << 32)
                    .is_none_or(|v| jump.seq_boundary < v)
                || jump
                    .source_epoch
                    .checked_add(1 << 32)
                    .is_none_or(|v| jump.epoch_boundary < v)
                || jump.epoch_boundary > state.epoch_high_water
            {
                return Err("恢复跳号边界无效".into());
            }
            spans.push((jump.source_seq + 1, jump.seq_boundary));
            prior_jump = jump.seq_boundary;
        }
        spans.sort_unstable();
        let mut end = 0u64;
        for (first, last) in spans {
            if end.checked_add(1) != Some(first) || last > state.seq {
                return Err("归档、心跳省略与恢复跳号未精确覆盖快照前缀".into());
            }
            end = last;
        }
        if end != state.seq || reduce(&self.events)? != state.projection {
            return Err("快照投影与原始归档重放不一致".into());
        }
        Ok(())
    }

    pub fn files(&self, docs_dir: &str) -> Result<BTreeMap<String, Option<Vec<u8>>>, String> {
        safe_docs(docs_dir)?;
        let mut months: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for event in &self.events {
            let date =
                chrono::DateTime::parse_from_rfc3339(&event.t).map_err(|_| "归档事件时间无效")?;
            let month = date.format("%Y-%m").to_string();
            let bytes = months.entry(month).or_default();
            serde_json::to_writer(&mut *bytes, event).map_err(|e| e.to_string())?;
            bytes.push(b'\n');
        }
        let mut files = BTreeMap::new();
        files.insert(
            format!("{docs_dir}/state/state.json"),
            Some(serde_json::to_vec_pretty(&self.state).map_err(|e| e.to_string())?),
        );
        for (month, bytes) in months {
            files.insert(
                format!("{docs_dir}/state/events/{month}.jsonl"),
                Some(bytes),
            );
        }
        Ok(files)
    }

    pub fn read(
        root: &Path,
        commit: &str,
        docs_dir: &str,
        project_id: &str,
        reduce: impl Fn(&[Event]) -> Result<Value, String>,
    ) -> Result<Self, String> {
        safe_docs(docs_dir)?;
        let state: SnapshotState = serde_json::from_slice(&blob(
            root,
            commit,
            &format!("{docs_dir}/state/state.json"),
        )?)
        .map_err(|e| e.to_string())?;
        if state.project_id != project_id {
            return Err("快照项目 ID 不匹配".into());
        }
        let prefix = format!("{docs_dir}/state/events/");
        let listing = repo::git_timeout(
            root,
            &["ls-tree", "-r", commit, "--", &prefix],
            Duration::from_secs(30),
        )?;
        let mut events = Vec::new();
        for line in listing.lines() {
            let (entry, path) = line.split_once('\t').ok_or("归档 tree 条目无效")?;
            if !entry.starts_with("100644 blob ") {
                return Err("快照归档必须是普通文件".into());
            }
            let name = path.strip_prefix(&prefix).ok_or("归档路径无效")?;
            let month = name.strip_suffix(".jsonl").ok_or("归档扩展名无效")?;
            if month.len() != 7
                || chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").is_err()
            {
                return Err("归档月份无效".into());
            }
            let bytes = blob(root, commit, path)?;
            if !bytes.ends_with(b"\n") {
                return Err("归档存在未完成尾行".into());
            }
            for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
                let event: Event = serde_json::from_slice(line).map_err(|e| e.to_string())?;
                let event_month = chrono::DateTime::parse_from_rfc3339(&event.t)
                    .map_err(|_| "归档事件时间无效")?
                    .format("%Y-%m")
                    .to_string();
                if event_month != month {
                    return Err("事件不属于该月份归档".into());
                }
                events.push(event);
            }
        }
        events.sort_by_key(|event| event.seq);
        let snapshot = Self { state, events };
        snapshot.validate(reduce)?;
        Ok(snapshot)
    }
}

/// The recovery envelope only transports an already accepted full original.
/// It conveys no live lease or execution authority.
pub fn recovered_original(event: &Event) -> Result<Option<Event>, String> {
    if event.kind != "recovery.confirmed" {
        return Ok(None);
    }
    if event.actor != "process:towermd-recovery"
        || event.layer != "L5"
        || event.stale
        || event.data.as_object().is_none_or(|m| m.len() != 1)
    {
        return Err("恢复原件容器无效".into());
    }
    let original: Event = serde_json::from_value(event.data["original"].clone())
        .map_err(|_| "恢复容器缺少完整原 Event")?;
    if original.kind == "recovery.confirmed"
        || original.kind.starts_with("lease.")
        || original.is_pure_heartbeat()
        || original.seq >= event.seq
        || original.eid == event.eid
        || original.obj != event.obj
        || original.seq == 0
        || original.lease_delta.is_some()
        || original.epoch != original.guards.first().map(|fence| fence.epoch)
        || original.cost_usd.is_some()
        || (original.stale && original.guards.is_empty())
    {
        return Err("恢复原件身份、序号或类别无效".into());
    }
    chrono::DateTime::parse_from_rfc3339(&original.t).map_err(|_| "恢复原件时间无效")?;
    crate::hub::validate_input(&crate::hub_protocol::NewEvent {
        eid: original.eid.clone(),
        host: original.host.clone(),
        actor: original.actor.clone(),
        kind: original.kind.clone(),
        layer: original.layer.clone(),
        obj: original.obj.clone(),
        data: original.data.clone(),
        guards: original.guards.clone(),
    })
    .map_err(|e| e.to_string())?;
    Ok(Some(original))
}

fn safe_docs(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', ':', '\0'])
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("快照文档目录无效".into());
    }
    Ok(())
}

fn blob(root: &Path, commit: &str, path: &str) -> Result<Vec<u8>, String> {
    let object = format!("{commit}:{path}");
    let mode = repo::git_timeout(
        root,
        &["ls-tree", commit, "--", path],
        Duration::from_secs(30),
    )?;
    if !mode.starts_with("100644 blob ") {
        return Err("快照原件不是普通文件".into());
    }
    let size: usize =
        repo::git_timeout(root, &["cat-file", "-s", &object], Duration::from_secs(30))?
            .parse()
            .map_err(|_| "快照文件大小无效")?;
    if size > 64 * 1024 * 1024 {
        return Err("单个快照原件超过 64 MiB".into());
    }
    let output = repo::git_timeout_env_input_output(
        root,
        &["cat-file", "blob", &object],
        Duration::from_secs(30),
        &[],
        None,
    )?;
    if !output.status.success() {
        return Err("无法读取快照原件".into());
    }
    Ok(output.stdout)
}
