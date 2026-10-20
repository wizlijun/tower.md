use super::*;
use crate::app_read_model::{TaskProgressSource, TaskProgressState, TaskProgressView};

impl ProjectSession {
    /// Read an actual recorded prefix without constructing or installing a Session.
    /// The daemon may serialize with its existing producer, but this method never writes.
    pub fn read_task_progress(
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        captured_at: u64,
        session: Option<&std::sync::Mutex<Self>>,
    ) -> Result<TaskProgressView, SessionSyncError> {
        crate::hub::timestamp(captured_at).map_err(|e| e.to_string())?;
        inputs
            .probe(catalog)
            .map_err(SessionSyncError::SourceChanged)?;
        if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
            return Err(SessionSyncError::SourceChanged(
                "authority changed; refresh task records".into(),
            ));
        }
        let private = crate::snapshot_git::private_revision(&catalog.root)?;
        let drafts =
            crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?;
        let recording = Self::read_progress_recording(catalog, session)?;
        let mut view = TaskProgressView {
            project_id: catalog.config.project_id().into(),
            common_dir: catalog.common_dir.to_string_lossy().into_owned(),
            revision: catalog.revision.clone(),
            captured_at,
            through_seq: None,
            floor_seq: None,
            state: TaskProgressState::Unavailable,
            source: TaskProgressSource::None,
            tasks: vec![],
            runs: vec![],
            evidence: vec![],
            proposals: vec![],
            holds: vec![],
            events: vec![],
            events_floor_seq: None,
            diagnostics: vec![],
        };
        if let Some(recording) = recording.as_ref().filter(|recording| !recording.empty()) {
            let (_, tail, head) = recording.parts();
            let prepared = Self::prepare_read_recording(recording, catalog, inputs)?;
            let objects = &prepared.projection.objects;
            let values = |kind: &str| -> Vec<serde_json::Value> {
                objects
                    .get(kind)
                    .into_iter()
                    .flat_map(|map| map.values())
                    .cloned()
                    .collect()
            };
            view.tasks = values("Task");
            view.runs = values("Run");
            view.evidence = values("Evidence");
            let (proposals, proposal_diagnostics) = crate::app_read_model::read_proposal_attention(
                catalog,
                inputs,
                &prepared.baseline_requests,
            );
            view.proposals = proposals;
            view.diagnostics.extend(proposal_diagnostics);
            let (_, human) = prepared.human.projection();
            view.holds = values("Hold")
                .into_iter()
                .filter(|hold| {
                    // Automatic gate failures already passed the complete finish/
                    // integration proof above. Human Holds additionally require
                    // a current actual opening: rebuilding invalidates old
                    // openings without erasing their historical raw objects.
                    hold["kind"] == "gatefailed"
                        || (hold["kind"] == "forbidden" && hold["scope"] == "TaskBranch")
                        || hold["id"].as_str().is_some_and(|id| {
                            human.openings.get(id).is_some_and(|opening| {
                                !opening.stale && opening.seq > human.invalidated_through
                            })
                        })
                })
                .collect();
            for hold in &mut view.holds {
                crate::task_check_recorded::decorate_view(
                    catalog,
                    &prepared.projection,
                    &head.leases,
                    captured_at,
                    hold,
                )?;
            }
            let start = tail.len().saturating_sub(256);
            view.events_floor_seq = Some(if start == 0 {
                head.floor_seq
            } else {
                tail[start - 1].seq
            });
            view.events = tail[start..].to_vec();
            view.through_seq = Some(head.contiguous_seq);
            view.floor_seq = Some(head.floor_seq);
            view.state = TaskProgressState::Recorded;
            view.source = TaskProgressSource::HubJournal;
            view.diagnostics.push(
                "显示已核实的记录状态；读取不续租、不推进任务，也不表示 Agent 当前仍在执行。"
                    .into(),
            );
        } else if let Some(summary) = crate::recovery::status_source(catalog, inputs)? {
            // Keep the existing strictly verified recovery summary historical.
            // It does not provide current Runs or execution permissions.
            let required = |key: &str| {
                summary[key].as_str().map(str::to_owned).ok_or_else(|| {
                    SessionSyncError::Rejected(format!("recovery summary lacks {key}"))
                })
            };
            view.tasks = summary["tasks"]
                .as_array()
                .cloned()
                .ok_or("recovery summary lacks tasks")?;
            view.through_seq = Some(
                summary["source_seq"]
                    .as_u64()
                    .ok_or("recovery summary lacks source_seq")?,
            );
            view.state = TaskProgressState::RecoveryRequired;
            view.source = TaskProgressSource::RecoverySnapshot {
                commit: required("source_commit")?,
                reference: required("source_reference")?,
            };
            view.diagnostics.push(
                "需恢复：这里是上次保存的任务记录，不是当前运行状态；可通过既有 hub 恢复入口处理。"
                    .into(),
            );
        } else {
            view.diagnostics
                .push("尚无可读取的任务记录；本次读取没有创建 hub 或初始化项目。".into());
        }
        // A complete second observation rejects source/head races instead of
        // manufacturing a new current state or advancing a Session cursor.
        #[cfg(test)]
        tests::before_final(&catalog.root);
        Self::probe_actual_binding(catalog, inputs, &private)?;
        if crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?
            != drafts
            || Self::read_progress_recording(catalog, session)? != recording
        {
            return Err(SessionSyncError::SourceChanged(
                "记录或来源在读取期间变化，请刷新任务进度".into(),
            ));
        }
        Ok(view)
    }

    /// Task eligibility from the same checked physical recording as App progress.
    /// This neither constructs a protocol owner nor registers, subscribes or expires a Hub.
    pub fn read_task_recheck(
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        task: &str,
        captured_at: u64,
        session: Option<&std::sync::Mutex<Self>>,
    ) -> Result<crate::proposals::TaskRecheckView, SessionSyncError> {
        crate::baseline_draft::safe_id(task)?;
        crate::hub::timestamp(captured_at).map_err(|e| e.to_string())?;
        let private = crate::snapshot_git::private_revision(&catalog.root)?;
        Self::probe_actual_binding(catalog, inputs, &private)?;
        let drafts =
            crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?;
        let recording = Self::read_progress_recording(catalog, session)?
            .filter(|recording| !recording.empty())
            .ok_or("Task recheck needs an existing verified Hub journal; missing state requires explicit recovery")?;
        let prepared = Self::prepare_read_recording(&recording, catalog, inputs)?;
        let baseline = crate::baseline_current::current_baseline(catalog, inputs)?
            .ok_or("Task recheck needs current approved baseline")?;
        prepared.tasks.require_publication_population()?;
        let view = crate::proposals::recheck_view(
            &prepared.projection,
            &prepared.capture.head.leases,
            &baseline.baseline,
            task,
            captured_at,
        )?;
        if prepared.tasks.publication_blocked(task) && view.reasons.is_empty() {
            return Err(
                "Task publication markers are incomplete; repair the original publication first"
                    .into(),
            );
        }
        Self::probe_actual_binding(catalog, inputs, &private)?;
        if crate::snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?
            != drafts
            || Self::read_progress_recording(catalog, session)?.as_ref() != Some(&recording)
        {
            return Err(SessionSyncError::SourceChanged(
                "Task recheck recording or sources changed; refresh the exact subject".into(),
            ));
        }
        Ok(view)
    }

    fn prepare_read_recording(
        recording: &crate::hub::CheckedJournalRead,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
    ) -> Result<PreparedSession, SessionSyncError> {
        let (checkpoint, tail, head) = recording.parts();
        let base = if head.floor_seq > 0 {
            let base: Snapshot = serde_json::from_value(checkpoint.checkpoint.clone())
                .map_err(|error| SessionSyncError::Rejected(error.to_string()))?;
            base.validate_metadata()?;
            if base.through_seq != head.floor_seq {
                return Err("checkpoint floor mismatch".into());
            }
            Some(base)
        } else {
            None
        };
        let capture = SessionCapture {
            cursor: 0,
            previous_head: head.clone(),
            head: head.clone(),
            base,
            tail: tail.to_vec(),
            fresh: tail.to_vec(),
            restored: head.floor_seq > 0,
        };
        Self::prepare(capture, catalog, inputs)
    }

    /// A notification cursor from the already-owned Hub. It grants no projected
    /// state or authority, performs no journal replay, and never subscribes,
    /// expires, renews or installs a Session. The App must reload its proved view.
    pub fn task_progress_cursor(
        &self,
        project_id: &str,
        common_dir: &std::path::Path,
    ) -> Result<(u64, u64), String> {
        self.ensure_active()?;
        if self.identity.as_ref() != Some(&(project_id.to_owned(), common_dir.to_path_buf())) {
            return Err("任务记录的实际 Session 身份已变化".into());
        }
        // install_checkpoint follows a successful physical snapshot_ack, even
        // when the following business sync has not yet installed the new head.
        let floor = self.head.floor_seq.max(
            self.base
                .as_ref()
                .map_or(0, |snapshot| snapshot.through_seq),
        );
        Ok((floor, self.hub.contiguous_seq()))
    }

    fn read_progress_recording(
        catalog: &Catalog,
        session: Option<&std::sync::Mutex<Self>>,
    ) -> Result<Option<crate::hub::CheckedJournalRead>, SessionSyncError> {
        let directory = catalog.common_dir.join("towermd");
        if let Some(session) = session {
            // Hub append commits its in-memory sequence only after journal fsync.
            // A complete physical line ahead of that sequence is not acceptance.
            let live = session.lock().map_err(|_| "session lock poisoned")?;
            live.ensure_active()?;
            if live.identity.as_ref()
                != Some(&(
                    catalog.config.project_id().to_owned(),
                    catalog.common_dir.clone(),
                ))
                || live.context != crate::recovery_outbox::RecoveryContext::extract(catalog)?
            {
                return Err(SessionSyncError::SourceChanged(
                    "任务记录的实际 Session 身份已变化".into(),
                ));
            }
            let recording = crate::hub::read_checked_journal(&directory)?;
            if recording
                .as_ref()
                .map_or(0, |recording| recording.parts().2.contiguous_seq)
                != live.hub.contiguous_seq()
            {
                return Err(SessionSyncError::SourceChanged(
                    "任务记录尚未得到实际 Hub 确认，请刷新".into(),
                ));
            }
            return Ok(recording);
        }
        // Never create a lock file. A cold reader cannot infer that another
        // runtime's complete bytes have passed fsync while it owns the Hub.
        match std::fs::File::open(directory.join("hub.lock")) {
            Ok(lock) => {
                fs2::FileExt::try_lock_shared(&lock).map_err(|error| {
                    SessionSyncError::SourceChanged(format!(
                        "Hub 正由其他实例使用，暂不能核实记录：{error}"
                    ))
                })?;
                let recording = crate::hub::read_checked_journal(&directory)?;
                drop(lock);
                Ok(recording)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if crate::hub::read_checked_journal(&directory)?.is_some() {
                    Err("已有 journal 缺少锁文件，不能确认只读来源；未创建或修复文件".into())
                } else {
                    Ok(None)
                }
            }
            Err(error) => Err(SessionSyncError::Rejected(error.to_string())),
        }
    }
}

#[cfg(test)]
#[path = "app_read_model_tests.rs"]
mod tests;
