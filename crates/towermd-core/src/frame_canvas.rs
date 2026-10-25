//! FR-154: write a frame into `tower.canvas`, only after a person approves
//! the exact bytes in the App. The frame becomes a group node carrying
//! `towermd.container` and `towermd.template`; a person-set frame's free
//! cards become text nodes carrying `towermd.card`. Every other node and edge
//! is kept as is. The daemon's single main writer makes the commit; the
//! approval and the receipt are hub events, so `main_inspection` can prove
//! the commit from the canonical history alone.
use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    canvas::CanvasDocument, events, frames::{self, Frame, SetBy}, hub::Event,
    main_writer::{WriteKind, WriteRequest}, path_guard,
    question_model::human_actor,
    snapshot_publication::{bytes, text},
};

pub const APPROVED: &str = "frame.materialize_approved";
pub const MATERIALIZED: &str = "frame.materialized";
/// `tower.canvas` is slow-layer: both events live on L1.
pub const LAYER: &str = "L1";
pub const WRITER: &str = "process:towermd";
const CARD_WIDTH: i64 = 240;
const CARD_HEIGHT: i64 = 120;
const GAP: i64 = 20;
const HEAD: i64 = 48;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub version: u32,
    pub request_id: String,
    pub project_id: String,
    pub docs_dir: String,
    pub frame: String,
    /// The hub sequence the frame was read at.
    pub seq: u64,
    pub main_revision: String,
    /// SHA-256 of the whole new `tower.canvas`.
    pub sha256: String,
    /// The nodes this write owns: the frame's group, then its cards.
    pub nodes: Vec<String>,
    /// The new file, for the person to read before approving.
    pub canvas: String,
    pub content_hash: String,
}

/// `frame.materialized`: the commit the single main writer made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Materialized {
    pub approval_eid: String,
    pub frame: String,
    pub commit: String,
    pub sha256: String,
}

/// What the frame keeps from its last materialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub commit: String,
    pub sha256: String,
    pub at: crate::delegation_projection::Mark,
}

pub fn path(docs: &str) -> String {
    format!("{docs}/tower.canvas")
}

impl Preview {
    pub fn hash(&self) -> Result<String, String> {
        let mut input = self.clone();
        input.content_hash.clear();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&input).map_err(|e| e.to_string())?)))
    }

    /// The approval must be the App's own record of exactly this preview.
    pub fn validate_approval(&self, event: &Event) -> Result<(), String> {
        let id = uuid::Uuid::parse_str(&self.request_id).map_err(|_| "人工确认 ID 无效")?;
        if self.version != 1 || id.get_version_num() != 7 || id.to_string() != self.request_id
            || self.project_id.trim().is_empty() || self.docs_dir.trim().is_empty()
            || hex::encode(Sha256::digest(self.canvas.as_bytes())) != self.sha256
        { return Err("容器落盘预览格式无效".into()); }
        frames::check_id(&self.frame)?;
        events::derived_eid(&self.main_revision, MATERIALIZED)?;
        if event.kind != APPROVED || !human_actor(&event.actor) || event.layer != LAYER
            || event.obj != self.frame || event.eid != self.request_id || !event.guards.is_empty()
            || event.data != serde_json::to_value(self).map_err(|e| e.to_string())?
            || self.hash()? != self.content_hash
        { return Err("容器落盘缺少与冻结预览一致的 App 人工确认原件".into()); }
        Ok(())
    }
}

pub fn from_approval(event: &Event) -> Result<Preview, String> {
    let preview: Preview = serde_json::from_value(event.data.clone()).map_err(|e| format!("容器落盘确认正文无效：{e}"))?;
    preview.validate_approval(event)?;
    Ok(preview)
}

fn owned(node: &Value, frame: &str) -> bool {
    let extension = &node["towermd"];
    extension["container"].as_str() == Some(frame)
        || extension["card"].as_str().and_then(|card| crate::cards::card_frame(card).ok()) == Some(frame)
}

/// The frame's nodes, in frame order: the group, then its live free cards
/// where a person set the frame. A pinned card keeps its pin; the others
/// fill a grid from the top left.
pub fn nodes(frame: &Frame) -> Vec<Value> {
    let place = frame.place;
    let mut out = vec![json!({
        "id": frame.id, "type": "group", "label": frame.title,
        "x": place.x, "y": place.y, "width": place.width, "height": place.height,
        "towermd": {"container": frame.id, "template": format!("{}@{}", frame.template.id, frame.template.version)},
    })];
    if frame.set_by != SetBy::Human {
        return out;
    }
    let columns = ((place.width - GAP) / (CARD_WIDTH + GAP)).max(1);
    let mut slot = 0;
    for card in frame.cards.iter().filter(|card| !card.archived) {
        let (x, y) = match frame.pins.get(&card.id) {
            Some(pin) => (pin.x, pin.y),
            None => {
                let at = (GAP + (slot % columns) * (CARD_WIDTH + GAP), HEAD + (slot / columns) * (CARD_HEIGHT + GAP));
                slot += 1;
                at
            }
        };
        let text = match Some(card.body.as_str()).filter(|body| !body.is_empty()) {
            Some(body) => format!("{}\n\n{body}", card.title),
            None => card.title.clone(),
        };
        out.push(json!({
            "id": card.id, "type": "text", "text": text,
            "x": place.x + x, "y": place.y + y, "width": CARD_WIDTH, "height": CARD_HEIGHT,
            "towermd": {"card": card.id},
        }));
    }
    out
}

/// The new `tower.canvas`: `existing` with this frame's nodes replaced.
pub fn build(existing: Option<&str>, frame: &Frame) -> Result<String, String> {
    let mut root: Value = match existing {
        Some(text) => {
            let document = CanvasDocument::decode(text)?;
            if let Some(problem) = document.diagnostics().first() {
                return Err(format!("现有 tower.canvas 需先修好：{}", problem.message));
            }
            document.raw().cloned().ok_or("现有 tower.canvas 无法读取")?
        }
        None => json!({"nodes": [], "edges": []}),
    };
    let ours = nodes(frame);
    // Our own nodes alone must pass the strict candidate check.
    CanvasDocument::from_value(json!({"nodes": ours.clone()}))?;
    let ids: Vec<&str> = ours.iter().filter_map(|node| node["id"].as_str()).collect();
    let mut kept = Vec::new();
    for node in root.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default() {
        if owned(&node, &frame.id) {
            continue;
        }
        if let Some(id) = node["id"].as_str().filter(|id| ids.contains(id)) {
            return Err(format!("tower.canvas 已有别人的节点 {id}，不能覆盖"));
        }
        kept.push(node);
    }
    kept.extend(ours);
    root["nodes"] = Value::Array(kept);
    if root.get("edges").is_none() {
        root["edges"] = json!([]);
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    let document = CanvasDocument::decode(&text)?;
    if let Some(problem) = document.diagnostics().first() {
        return Err(format!("落盘后的 tower.canvas 不合法：{}", problem.message));
    }
    Ok(text)
}

fn materialized(node: &Value) -> bool {
    node["towermd"]["container"].is_string() || node["towermd"]["card"].is_string()
}

/// A canvas regenerated from elsewhere (a new version) keeps the frames a
/// person already approved into `existing`: their group and card nodes, and
/// the edges between them. Without any, `canvas` is returned unchanged. A
/// node ID the new canvas already uses is refused, never overwritten.
pub fn carry(existing: Option<&str>, canvas: &str) -> Result<String, String> {
    let Some(existing) = existing else { return Ok(canvas.to_owned()) };
    let document = CanvasDocument::decode(existing)?;
    let old = document.raw().cloned().ok_or("现有 tower.canvas 无法读取")?;
    let list = |value: &Value, key: &str| value.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let kept: Vec<Value> = list(&old, "nodes").into_iter().filter(materialized).collect();
    if kept.is_empty() {
        return Ok(canvas.to_owned());
    }
    let mut root: Value = serde_json::from_str(canvas).map_err(|e| e.to_string())?;
    let taken: Vec<String> = ["nodes", "edges"].iter()
        .flat_map(|key| list(&root, key))
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    let ids: Vec<&str> = kept.iter().filter_map(|node| node["id"].as_str()).collect();
    let edges: Vec<Value> = list(&old, "edges").into_iter().filter(|edge| {
        [&edge["fromNode"], &edge["toNode"]].iter().all(|end| end.as_str().is_some_and(|end| ids.contains(&end)))
    }).collect();
    let carried = ids.iter().copied().chain(edges.iter().filter_map(|edge| edge["id"].as_str()));
    if let Some(id) = carried.into_iter().find(|id| taken.iter().any(|taken| taken == id)) {
        return Err(format!("新画布与已落盘容器的节点 {id} 重名"));
    }
    // The regenerated canvas may already carry tolerated diagnostics; only new ones refuse.
    let allowed = CanvasDocument::decode(canvas)?.diagnostics().len() + document.diagnostics().len();
    let mut nodes = list(&root, "nodes");
    nodes.extend(kept);
    root["nodes"] = Value::Array(nodes);
    let mut all = list(&root, "edges");
    all.extend(edges);
    root["edges"] = Value::Array(all);
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    let merged = CanvasDocument::decode(&text)?;
    if merged.diagnostics().len() > allowed {
        let problem = merged.diagnostics().last().map(|problem| problem.message.clone()).unwrap_or_default();
        return Err(format!("保留容器后的 tower.canvas 不合法：{problem}"));
    }
    Ok(text)
}

/// `tower.canvas` as committed at `revision`, if any.
pub fn read_at(root: &Path, revision: &str, docs: &str) -> Result<Option<String>, String> {
    let file = path(docs);
    if bytes(root, &["ls-tree", "-z", revision, "--", &file])?.is_empty() {
        return Ok(None);
    }
    let raw = bytes(root, &["show", &format!("{revision}:{file}")])?;
    String::from_utf8(raw).map(Some).map_err(|_| "tower.canvas 不是 UTF-8".into())
}

/// The preview a person reads before approving.
pub fn prepare(root: &Path, main: &str, project_id: &str, docs: &str, frame: &Frame, seq: u64, request_id: &str) -> Result<Preview, String> {
    let canvas = build(read_at(root, main, docs)?.as_deref(), frame)?;
    let mut preview = Preview {
        version: 1, request_id: request_id.into(), project_id: project_id.into(), docs_dir: docs.into(),
        frame: frame.id.clone(), seq, main_revision: main.into(), sha256: hex::encode(Sha256::digest(canvas.as_bytes())),
        nodes: nodes(frame).iter().filter_map(|node| node["id"].as_str().map(str::to_owned)).collect(),
        canvas, content_hash: String::new(),
    };
    preview.content_hash = preview.hash()?;
    Ok(preview)
}

pub fn write_request(preview: &Preview, epoch: u64) -> WriteRequest {
    WriteRequest {
        docs_dir: preview.docs_dir.clone(), epoch,
        kind: WriteKind::Slow { expected_base: preview.main_revision.clone(), tag: None },
        changes: BTreeMap::from([(path(&preview.docs_dir), Some(preview.canvas.clone().into_bytes()))]),
        message: format!("Write frame {} into tower.canvas\n\nTowermd-Approval: {}\nTowermd-Content: {}",
            preview.frame, preview.request_id, preview.sha256),
    }
}

/// The commit is exactly the approved write: made by the single main writer
/// right on the approved main, touching only `tower.canvas`, with the
/// approved bytes and trailers.
pub fn prove_commit(root: &Path, preview: &Preview, commit: &str) -> Result<(), String> {
    events::derived_eid(commit, MATERIALIZED)?;
    let parents = text(root, &["rev-list", "--parents", "-n", "1", commit])?;
    let parents: Vec<_> = parents.split_whitespace().collect();
    if parents.len() != 2 || parents[1] != preview.main_revision {
        return Err("容器落盘提交没有直接承接已批准的 main".into());
    }
    if text(root, &["show", "-s", "--format=%an", commit])? != WRITER {
        return Err("容器落盘提交必须由唯一主干写入者生成".into());
    }
    let message = text(root, &["show", "-s", "--format=%B", commit])?;
    for expected in [format!("Towermd-Approval: {}", preview.request_id), format!("Towermd-Content: {}", preview.sha256)] {
        let key = expected.split_once(':').unwrap().0;
        let lines: Vec<_> = message.lines().filter(|line| line.starts_with(&format!("{key}:"))).collect();
        if lines != [expected.as_str()] {
            return Err("容器落盘提交 trailer 与人工确认不一致".into());
        }
    }
    let report = path_guard::check(root, &preview.main_revision, commit, &preview.docs_dir, &[])?;
    let file = path(&preview.docs_dir);
    if report.changes.len() != 1 || report.changes[0].path != file {
        return Err("容器落盘提交只能改 tower.canvas".into());
    }
    let written = bytes(root, &["cat-file", "blob", &report.changes[0].new_oid])?;
    if hex::encode(Sha256::digest(&written)) != preview.sha256 {
        return Err("容器落盘提交的画布与批准的字节不一致".into());
    }
    Ok(())
}

/// FR-103 proof for one protected commit, from its canonical receipt and the
/// approval it names.
pub fn prove(root: &Path, project_id: &str, docs: &str, commit: &str, receipt: &dyn Fn(&str) -> Option<Event>) -> Result<(), String> {
    let event = receipt(&events::derived_eid(commit, MATERIALIZED)?).ok_or("缺少容器落盘回执")?;
    let done = check_receipt(&event)?;
    if done.commit != commit {
        return Err("容器落盘回执指向另一个提交".into());
    }
    let approval = receipt(&done.approval_eid).ok_or("容器落盘缺少 canonical 人工确认")?;
    let preview = from_approval(&approval)?;
    if preview.project_id != project_id || preview.docs_dir != docs || preview.sha256 != done.sha256 || preview.frame != done.frame {
        return Err("容器落盘回执与人工确认不一致".into());
    }
    prove_commit(root, &preview, commit)
}

fn check_receipt(event: &Event) -> Result<Materialized, String> {
    let done: Materialized = serde_json::from_value(event.data.clone()).map_err(|e| format!("容器落盘回执无效：{e}"))?;
    if event.kind != MATERIALIZED || event.actor != WRITER || event.layer != LAYER || event.obj != done.frame
        || event.eid != events::derived_eid(&done.commit, MATERIALIZED)?
        || event.guards.len() != 1 || event.guards[0].name != "main" || event.epoch != Some(event.guards[0].epoch)
    { return Err("容器落盘回执须由持有主干租约的写入者记录".into()); }
    Ok(done)
}

/// Records both events on the frame. The approval is checked against its
/// own preview; the receipt against the approval already recorded.
pub fn apply(state: &mut Value, event: &Event) -> Result<bool, String> {
    if event.kind != APPROVED && event.kind != MATERIALIZED {
        return Ok(false);
    }
    if event.stale {
        return Ok(true);
    }
    let mut all = frames::read(state)?;
    let frame = all.get_mut(&event.obj).ok_or("容器不存在")?;
    let mut next = state.clone();
    if event.kind == APPROVED {
        let preview = from_approval(event)?;
        next["FrameMaterialization"][&event.eid] = serde_json::to_value(preview).map_err(|e| e.to_string())?;
    } else {
        let done = check_receipt(event)?;
        let preview: Preview = serde_json::from_value(next["FrameMaterialization"][&done.approval_eid].clone())
            .map_err(|_| "容器落盘缺少已接受的人工确认")?;
        if preview.frame != done.frame || preview.sha256 != done.sha256 {
            return Err("容器落盘回执与人工确认不一致".into());
        }
        frame.materialized = Some(Record { commit: done.commit, sha256: done.sha256, at: frames::mark(event) });
        next[frames::GROUP] = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    }
    *state = next;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXISTING: &str = r#"{"nodes":[
        {"id":"m-old","type":"file","file":"docs/tower/arch/M-old.md","x":0,"y":0,"width":100,"height":60},
        {"id":"C-plan","type":"group","label":"计划","x":400,"y":0,"width":300,"height":200,"towermd":{"container":"C-plan","template":"kanban@1"}},
        {"id":"C-plan/K-1","type":"text","text":"卡","x":420,"y":48,"width":240,"height":120,"towermd":{"card":"C-plan/K-1"}}],
      "edges":[{"id":"e-card","fromNode":"C-plan/K-1","toNode":"C-plan"},{"id":"e-old","fromNode":"m-old","toNode":"C-plan"}]}"#;
    const NEW: &str = "{\n  \"nodes\": [\n    {\"id\":\"m-new\",\"type\":\"file\",\"file\":\"docs/tower/arch/M-new.md\",\"x\":0,\"y\":0,\"width\":100,\"height\":60}\n  ],\n  \"edges\": []\n}";

    #[test]
    fn a_new_canvas_keeps_materialized_frames_and_only_their_edges() {
        let text = carry(Some(EXISTING), NEW).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let ids: Vec<&str> = value["nodes"].as_array().unwrap().iter().map(|node| node["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["m-new", "C-plan", "C-plan/K-1"]);
        let edges: Vec<&str> = value["edges"].as_array().unwrap().iter().map(|edge| edge["id"].as_str().unwrap()).collect();
        assert_eq!(edges, ["e-card"], "an edge to a regenerated node is not carried");
    }

    #[test]
    fn nothing_to_carry_keeps_the_bytes_and_a_clash_is_refused() {
        assert_eq!(carry(None, NEW).unwrap(), NEW);
        let plain = r#"{"nodes":[{"id":"m-old","type":"file","file":"a.md","x":0,"y":0,"width":1,"height":1}],"edges":[]}"#;
        assert_eq!(carry(Some(plain), NEW).unwrap(), NEW);
        let clash = NEW.replace("m-new", "C-plan");
        assert!(carry(Some(EXISTING), &clash).unwrap_err().contains("C-plan"));
    }
}
