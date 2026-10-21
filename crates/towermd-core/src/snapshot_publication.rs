//! Read-only proof of a snapshot's actual Git bytes and accepted archive.
//! Trailers describe the proof; they never substitute for it.
use std::{collections::BTreeSet, path::Path, time::Duration};
use serde::{Deserialize, Serialize};
use crate::{events, hub::Event, project_runtime, dev_state, repo, snapshot::Snapshot};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotProof {
    pub commit: String,
    pub seq: u64,
    pub epoch: u64,
}

pub fn prove(
    root: &Path,
    commit: &str,
    docs_dir: &str,
    project_id: &str,
    written: &Event,
    receipt: &impl Fn(&str) -> Option<Event>,
) -> Result<SnapshotProof, String> {
    let snapshot = Snapshot::read(root, commit, docs_dir, project_id, |events| {
        project_runtime::replay(&dev_state::initial(), events, dev_state::apply)
    })?;
    let proof = prove_checked(root, commit, docs_dir, project_id, &snapshot, receipt)?;
    proof.check_receipt(project_id, written)?;
    if receipt(&written.eid).as_ref() != Some(written) {
        return Err("快照写入事件不是 canonical 原件".into());
    }
    Ok(proof)
}

/// The caller supplies a snapshot already validated by the shared reader and
/// its business reducer. Used when recording a missing receipt after a crash:
/// the current main fence may be newer than this historical snapshot's epoch.
pub fn prove_checked(
    root: &Path,
    commit: &str,
    docs_dir: &str,
    project_id: &str,
    snapshot: &Snapshot,
    receipt: &impl Fn(&str) -> Option<Event>,
) -> Result<SnapshotProof, String> {
    events::derived_eid(commit, "snapshot.written")?;
    if snapshot.state.project_id != project_id {
        return Err("快照项目不一致".into());
    }
    let parents = text(root, &["rev-list", "--parents", "-n", "1", commit])?;
    if parents.split_whitespace().count() != 2 {
        return Err("快照提交必须只有一个父提交".into());
    }
    let author = text(root, &["show", "-s", "--format=%an", commit])?;
    if author != "process:towermd-snapshot" {
        return Err("快照提交署名无效".into());
    }
    let message = text(root, &["show", "-s", "--format=%B", commit])?;
    let expected = [
        format!("Towermd-Epoch: {}", snapshot.state.epoch),
        format!("Towermd-Snapshot: {}", snapshot.state.seq),
    ];
    let trailers: Vec<_> = message.lines().filter(|line| line.starts_with("Towermd-")).collect();
    if trailers.len() != 2 || expected.iter().any(|value| !trailers.contains(&value.as_str())) {
        return Err("快照 trailer 与实际归档水位不一致".into());
    }
    let files = snapshot.files(docs_dir)?;
    let changed = bytes(root, &[
        "diff-tree", "--no-commit-id", "--name-only", "-z", "--no-renames",
        "--no-ext-diff", "--no-textconv", "-r", commit,
    ])?;
    for path in changed.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
        let path = std::str::from_utf8(path).map_err(|_| "快照路径不是 UTF-8")?;
        if !files.contains_key(path) {
            return Err(format!("快照提交混入非归档改动：{path}"));
        }
    }
    let prefix = format!("{docs_dir}/state/");
    let listing = bytes(root, &["ls-tree", "-rz", commit, "--", &prefix])?;
    let mut actual_paths = BTreeSet::new();
    for entry in listing.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
        let entry = std::str::from_utf8(entry).map_err(|_| "快照 tree 不是 UTF-8")?;
        let (mode, path) = entry.split_once('\t').ok_or("快照 tree 条目无效")?;
        if !mode.starts_with("100644 blob ") {
            return Err("快照只能包含普通文件".into());
        }
        actual_paths.insert(path.to_string());
    }
    if actual_paths != files.keys().cloned().collect() {
        return Err("快照目录含未证明的额外文件或缺失文件".into());
    }
    for (path, expected) in files {
        let actual = bytes(root, &["cat-file", "blob", &format!("{commit}:{path}")])?;
        if expected.as_ref() != Some(&actual) {
            return Err(format!("快照实际字节与已核归档不一致：{path}"));
        }
    }
    let mut writer_grant = false;
    for original in project_runtime::canonical_events(&snapshot.events)?.values() {
        if receipt(&original.eid).as_ref() != Some(original) {
            return Err(format!("快照归档缺少相同 canonical 原件：{}", original.eid));
        }
        writer_grant |= !original.stale && original.acquired_leases().iter().any(|lease| {
            lease.name == "main" && lease.epoch == snapshot.state.epoch
        });
    }
    if !writer_grant {
        return Err("快照归档缺少历史主干写入租约".into());
    }
    Ok(SnapshotProof {
        commit: commit.into(), seq: snapshot.state.seq, epoch: snapshot.state.epoch,
    })
}

impl SnapshotProof {
    pub fn check_receipt(&self, project_id: &str, written: &Event) -> Result<(), String> {
        if written.eid != events::derived_eid(&self.commit, "snapshot.written")?
            || written.kind != "snapshot.written" || written.stale
            || !matches!(written.actor.as_str(), "process:towermd-snapshot" | "process:towermd-recovery")
            || written.layer != "L5" || written.obj != project_id
            || written.data != serde_json::to_value(self).map_err(|error| error.to_string())?
            || written.guards.len() != 1 || written.guards[0].name != "main"
            || written.guards[0].epoch < self.epoch
            || written.epoch != Some(written.guards[0].epoch)
        {
            return Err("快照写入原件与实际提交、项目或租约不一致".into());
        }
        Ok(())
    }
}

pub(crate) fn bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut options = vec!["--no-replace-objects"];
    options.extend_from_slice(args);
    let output = repo::git_timeout_env_output(root, &options, Duration::from_secs(30), &[])?;
    if !output.status.success() {
        return Err(format!("无法读取 Git 证明：{}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output.stdout)
}
pub(crate) fn text(root: &Path, args: &[&str]) -> Result<String, String> {
    String::from_utf8(bytes(root, args)?).map(|text| text.trim_end().into())
        .map_err(|_| "Git 证明不是 UTF-8".into())
}
