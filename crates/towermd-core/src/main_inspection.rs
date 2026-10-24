//! FR-103: inspect complete main history against canonical accepted facts.
use std::{collections::BTreeMap, path::Path};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::{events, frame_canvas, hub::Event, hub_protocol::NewEvent, kit::{self, Assembly}, main_resolution,
    path_guard::{self, Violation}, snapshot_publication};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockedCommit {
    pub commit: String,
    pub violations: Vec<Violation>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inspection {
    pub main: String,
    pub import_anchor: Option<String>,
    /// False means the history boundary could not be established completely.
    pub complete: bool,
    /// Complete inspection with no unapproved protected writes.
    pub passed: bool,
    pub blocked_commits: Vec<BlockedCommit>,
    pub warning_commits: Vec<String>,
    pub alerts: Vec<NewEvent>,
    pub error: Option<String>,
}

pub fn inspect(
    root: &Path,
    main: &str,
    project_id: &str,
    docs_dir: &str,
    previous_main: Option<&str>,
    host: &str,
    receipt: impl Fn(&str) -> Option<Event>,
    assembly: &Assembly,
) -> Inspection {
    let mut result = Inspection {
        main: main.into(), import_anchor: None, complete: false, passed: false,
        blocked_commits: vec![], warning_commits: vec![], alerts: vec![], error: None,
    };
    let history = match path_guard::inspect_main(root, main, project_id, docs_dir, previous_main) {
        Ok(history) => history,
        Err(error) => {
            incomplete(&mut result, host, error);
            return result;
        }
    };
    result.import_anchor = Some(history.import_anchor);
    let mut resolutions: BTreeMap<String, Result<main_resolution::Resolution, String>> = BTreeMap::new();
    // inspect_main yields every commit once. Each proof is checked once during
    // this call, irrespective of the number of protected paths in that commit.
    for changed in history.commits {
        let message = match snapshot_publication::bytes(root, &["show", "-s", "--format=%B", &changed.commit])
            .and_then(|bytes| String::from_utf8(bytes).map_err(|_| "Git 提交说明不是 UTF-8".into())) {
            Ok(message) => message,
            Err(error) => {
                incomplete(&mut result, host, format!("inspection_incomplete: {error}"));
                return result;
            }
        };
        let raw: Vec<_> = message.lines().filter(|line| line.starts_with("Towermd-")).collect();
        let unknown: Vec<_> = raw.iter().copied().filter(|line| {
            !["Towermd-Epoch:", "Towermd-Snapshot:", "Towermd-Baseline:", "Towermd-Approval:", "Towermd-Content:", "Towermd-Resolution:",
                "Towermd-Task:", "Towermd-Run:", "Towermd-Module:", "Towermd-Evidence:"]
                .iter().any(|prefix| line.starts_with(prefix))
        }).collect();
        if !unknown.is_empty() {
            result.warning_commits.push(changed.commit.clone());
            result.alerts.push(alert(&changed.commit, host, "unknown_towermd_trailer",
                json!({"commit":changed.commit,"severity":"warning","trailers":unknown})));
        }
        if changed.violations.is_empty() { continue; }
        let proof = kit::prove_by_kits(assembly.kits, root, &changed.commit, project_id, &receipt).or_else(|baseline_error| {
            let eid = events::derived_eid(&changed.commit, "snapshot.written")?;
            match receipt(&eid) {
                Some(event) => snapshot_publication::prove(root, &changed.commit, docs_dir, project_id, &event, &receipt, assembly)
                    .map(|_| ()).map_err(|error| format!("{baseline_error}；{error}")),
                None => Err(baseline_error),
            }
        }).or_else(|previous_error| {
            // FR-154: a frame a person approved writing into tower.canvas.
            let eid = events::derived_eid(&changed.commit, frame_canvas::MATERIALIZED)?;
            if receipt(&eid).is_none() { return Err(previous_error); }
            frame_canvas::prove(root, project_id, docs_dir, &changed.commit, &receipt)
                .map_err(|error| format!("{previous_error}；{error}"))
        }).or_else(|previous_error| {
            let own = events::derived_eid(&changed.commit, "main.change.resolved")?;
            let target = if receipt(&own).is_some() {
                changed.commit.clone()
            } else {
                let targets: Vec<_> = message.lines().filter_map(|line| line.strip_prefix("Towermd-Resolution: ")).collect();
                if targets.len() != 1 { return Err(previous_error); }
                events::derived_eid(targets[0], "main.change.resolved")?;
                targets[0].into()
            };
            let proof = resolutions.entry(target.clone()).or_insert_with(|| {
                let event = receipt(&events::derived_eid(&target, "main.change.resolved")?)
                    .ok_or("缺少 canonical 主干处理原件")?;
                let approval_eid = event.data["approval_eid"].as_str().ok_or("主干处理缺少批准 ID")?;
                let approval = receipt(approval_eid).ok_or("主干处理缺少 canonical 人工确认")?;
                let proof = main_resolution::prove_resolution(root, project_id, docs_dir, &event, &approval)?;
                if !main_resolution::is_ancestor(root, &proof.resolution_commit, main)? {
                    return Err("主干处理结果不属于当前 main".into());
                }
                Ok(proof)
            });
            match proof {
                Ok(proof) if proof.target_commit == changed.commit || proof.resolution_commit == changed.commit => Ok(()),
                Ok(_) => Err("主干处理不能认证另一个提交".into()),
                Err(error) => Err(format!("{previous_error}；{error}")),
            }
        });
        if let Err(reason) = proof {
            result.alerts.push(alert(&changed.commit, host, "unapproved_protected_change",
                json!({"commit":changed.commit,"severity":"blocking","violations":changed.violations})));
            result.blocked_commits.push(BlockedCommit {
                commit: changed.commit, violations: changed.violations, reason,
            });
        }
    }
    result.complete = true;
    result.passed = result.blocked_commits.is_empty();
    result
}

fn incomplete(result: &mut Inspection, host: &str, error: String) {
    result.error = Some(error.clone());
    // Invalid caller input cannot name a deterministic Git event. Actual
    // daemon observations always use the resolved full main OID.
    if events::derived_eid(&result.main, "alert.raised.inspection_incomplete").is_ok() {
        result.alerts.push(alert(&result.main, host, "inspection_incomplete",
            json!({"commit":result.main,"severity":"blocking","error":error})));
    }
}

fn alert(commit: &str, host: &str, reason: &str, data: serde_json::Value) -> NewEvent {
    // Separate alert categories are independently idempotent for the same SHA.
    let eid = events::derived_eid(commit, &format!("alert.raised.{reason}"))
        .expect("inspect_main returned a full commit OID");
    NewEvent {
        eid: eid.clone(), host: host.into(), actor: "process:towermd".into(),
        kind: "alert.raised".into(), layer: "L5".into(), obj: eid,
        data: json!({"reason":reason,"detail":data}), guards: vec![],
    }
}
