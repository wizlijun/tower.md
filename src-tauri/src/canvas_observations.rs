//! App-local viewing preferences only. Deleting these cannot change any Task,
//! Hold, approval, event, snapshot, or daemon state.
use crate::{
    commands::AppError,
    draft_requests::{io_error, write_new},
    task_progress::LiveProgress,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadWatermark {
    pub revision: String,
    pub daemon_instance: String,
    pub through_seq: Option<u64>,
    pub floor_seq: Option<u64>,
    pub captured_at: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviousClose {
    schema: u32,
    pub project_id: String,
    pub common_dir: String,
    pub closed_at: u64,
    pub last_read: ReadWatermark,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ObservationWindow {
    Available {
        previous_close: Option<PreviousClose>,
    },
    Unavailable {
        reason: String,
    },
}
#[derive(Default, Clone)]
pub struct CanvasObservations(Arc<Mutex<BTreeMap<String, OpenObservation>>>);
struct OpenObservation {
    key: String,
    previous: Option<PreviousClose>,
    current: PreviousClose,
}
#[derive(Serialize)]
pub struct ObservedProgress {
    #[serde(flatten)]
    pub progress: LiveProgress,
    pub observation: ObservationWindow,
}
fn now() -> Result<u64, AppError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io_error)?
        .as_millis()
        .try_into()
        .map_err(io_error)
}
fn directory(app_data: &Path) -> Result<PathBuf, AppError> {
    let path = app_data.join("canvas-observations");
    fs::create_dir_all(&path).map_err(io_error)?;
    if !fs::symlink_metadata(&path)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err(io_error("画布观察记录目录类型无效"));
    }
    Ok(path)
}
fn key(project: &str, common: &str) -> Result<String, AppError> {
    towermd_core::canvas_view::observation_key(project, common).map_err(io_error)
}
fn load(
    app_data: &Path,
    key: &str,
    project: &str,
    common: &str,
) -> Result<Option<PreviousClose>, AppError> {
    let path = directory(app_data)?.join(format!("{key}.json"));
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    if !meta.file_type().is_file() || meta.len() > 64 * 1024 {
        return Err(io_error("上次关闭观察记录类型或大小无效"));
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(io_error)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let previous: PreviousClose = serde_json::from_slice(&bytes).map_err(io_error)?;
    if previous.schema != 1
        || previous.project_id != project
        || previous.common_dir != common
        || previous.closed_at < previous.last_read.captured_at
    {
        return Err(io_error("上次观察记录身份或关闭时间不一致"));
    }
    Ok(Some(previous))
}
fn save(
    app_data: &Path,
    key: &str,
    mut record: PreviousClose,
    closed_at: u64,
) -> Result<(), AppError> {
    if closed_at < record.last_read.captured_at {
        return Err(io_error("本机关闭时间早于真实读取时间；不改写旧观察起点"));
    }
    record.closed_at = closed_at;
    let directory = directory(app_data)?;
    let target = directory.join(format!("{key}.json"));
    if let Ok(meta) = fs::symlink_metadata(&target) {
        if !meta.file_type().is_file() {
            return Err(io_error("观察记录目标不是普通文件"));
        }
    }
    let temporary = directory.join(format!("{key}.{}.tmp", uuid::Uuid::now_v7()));
    write_new(&temporary, &record)?;
    fs::rename(&temporary, target).map_err(io_error)?;
    fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(io_error)
}
impl CanvasObservations {
    pub fn observe(
        &self,
        _token: &str,
        progress: &LiveProgress,
        app_data: &Path,
    ) -> ObservationWindow {
        match self.observe_inner(progress, app_data) {
            Ok(previous_close) => ObservationWindow::Available { previous_close },
            Err(error) => ObservationWindow::Unavailable {
                reason: error.message,
            },
        }
    }
    fn observe_inner(
        &self,
        progress: &LiveProgress,
        app_data: &Path,
    ) -> Result<Option<PreviousClose>, AppError> {
        let view = &progress.view;
        let key = key(&view.project_id, &view.common_dir)?;
        let current = PreviousClose {
            schema: 1,
            project_id: view.project_id.clone(),
            common_dir: view.common_dir.clone(),
            closed_at: 0,
            last_read: ReadWatermark {
                revision: view.revision.clone(),
                daemon_instance: progress.daemon_instance.clone(),
                through_seq: view.through_seq,
                floor_seq: view.floor_seq,
                captured_at: view.captured_at,
            },
        };
        // Only tiny App metadata I/O is serialized here. No Hub/session/producer or
        // project filesystem locks are held, and no business event can be appended.
        let mut open = self.0.lock().map_err(io_error)?;
        // Keep one personal window per physical project for this App lifetime.
        // A late read from a previously selected token must not close a newer view.
        if let Some(active) = open.get_mut(&key) {
            if current.last_read.captured_at >= active.current.last_read.captured_at {
                active.current = current;
            }
            return Ok(active.previous.clone());
        }
        let previous = load(app_data, &key, &view.project_id, &view.common_dir)?;
        open.insert(
            key.clone(),
            OpenObservation {
                key,
                previous: previous.clone(),
                current,
            },
        );
        Ok(previous)
    }
    pub fn close_all(&self, app_data: &Path) -> Result<(), AppError> {
        self.close_at(app_data, now()?)
    }
    fn close_at(&self, app_data: &Path, at: u64) -> Result<(), AppError> {
        let mut open = self.0.lock().map_err(io_error)?;
        for (_, record) in std::mem::take(&mut *open) {
            save(app_data, &record.key, record.current, at)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use towermd_core::app_read_model::{TaskProgressSource, TaskProgressState, TaskProgressView};
    fn progress() -> LiveProgress {
        LiveProgress {
            daemon_instance: "one".into(),
            view: TaskProgressView {
                project_id: "P".into(),
                common_dir: "/clone/.git".into(),
                revision: "a".repeat(40),
                captured_at: 10,
                through_seq: Some(2),
                floor_seq: Some(0),
                state: TaskProgressState::Recorded,
                source: TaskProgressSource::HubJournal,
                tasks: vec![],
                runs: vec![],
                evidence: vec![],
                holds: vec![],
                events: vec![],
                events_floor_seq: Some(2),
                diagnostics: vec![],
            },
        }
    }
    #[test]
    fn read_never_moves_previous_close_and_reopen_uses_actual_close() {
        let data = tempfile::tempdir().unwrap();
        let observations = CanvasObservations::default();
        let mut view = progress();
        assert!(matches!(
            observations.observe("selected", &view, data.path()),
            ObservationWindow::Available {
                previous_close: None
            }
        ));
        view.view.through_seq = Some(3);
        observations.observe("selected", &view, data.path());
        let directory = directory(data.path()).unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        observations.close_at(data.path(), 20).unwrap();
        let reopened = CanvasObservations::default();
        let ObservationWindow::Available {
            previous_close: Some(previous),
        } = reopened.observe("new-token", &view, data.path())
        else {
            panic!("actual previous close");
        };
        assert_eq!(previous.closed_at, 20);
        assert_eq!(previous.last_read.through_seq, Some(3));
        view.view.through_seq = Some(4);
        let ObservationWindow::Available {
            previous_close: Some(same),
        } = reopened.observe("new-token", &view, data.path())
        else {
            panic!("same window");
        };
        assert_eq!(same.closed_at, 20);
        assert_eq!(same.last_read.through_seq, Some(3));
    }
    #[test]
    fn project_switch_and_late_reads_do_not_write_a_close_or_replace_a_newer_watermark() {
        let data = tempfile::tempdir().unwrap();
        let observations = CanvasObservations::default();
        let mut first = progress();
        observations.observe("old-token", &first, data.path());
        let mut second = progress();
        second.view.common_dir = "/other/.git".into();
        observations.observe("other-token", &second, data.path());
        first.view.captured_at = 15;
        first.view.through_seq = Some(4);
        observations.observe("new-token", &first, data.path());
        first.view.captured_at = 10;
        first.view.through_seq = Some(2);
        observations.observe("old-token", &first, data.path());
        assert_eq!(
            fs::read_dir(directory(data.path()).unwrap())
                .unwrap()
                .count(),
            0,
            "selecting/reading never persists a close"
        );
        observations.close_at(data.path(), 20).unwrap();
        let reopened = CanvasObservations::default();
        let ObservationWindow::Available {
            previous_close: Some(actual),
        } = reopened.observe("reopen", &first, data.path())
        else {
            panic!("actual app close");
        };
        assert_eq!(actual.closed_at, 20);
        assert_eq!(actual.last_read.through_seq, Some(4));
        assert_eq!(actual.last_read.captured_at, 15);
    }
    #[test]
    fn deleting_preferences_only_loses_personal_window_and_other_clone_is_separate() {
        let data = tempfile::tempdir().unwrap();
        let observations = CanvasObservations::default();
        let view = progress();
        observations.observe("selected", &view, data.path());
        observations.close_at(data.path(), 20).unwrap();
        let mut clone = progress();
        clone.view.common_dir = "/other/.git".into();
        assert!(matches!(
            CanvasObservations::default().observe("other", &clone, data.path()),
            ObservationWindow::Available {
                previous_close: None
            }
        ));
        fs::remove_dir_all(directory(data.path()).unwrap()).unwrap();
        assert!(matches!(
            CanvasObservations::default().observe("new", &view, data.path()),
            ObservationWindow::Available {
                previous_close: None
            }
        ));
        assert_eq!(view.view.through_seq, Some(2));
    }
}
