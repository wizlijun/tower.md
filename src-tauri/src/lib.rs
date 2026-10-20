use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri_plugin_dialog::DialogExt;
use towermd_core::project_view::ProjectView;
use uuid::Uuid;

mod backend;
type BackendState = Arc<Mutex<backend::Backend>>;

#[derive(Clone, Debug, Serialize)]
struct ProjectSelection {
    token: String,
    path: String,
}

#[derive(Default)]
struct ProjectSession {
    selection_request: u64,
    selected: Option<(ProjectSelection, PathBuf)>,
}

impl ProjectSession {
    fn begin_selection(&mut self) -> u64 {
        self.selection_request += 1;
        self.selection_request
    }

    fn complete_selection(
        &mut self,
        request: u64,
        root: Option<PathBuf>,
    ) -> Result<Option<ProjectSelection>, String> {
        if request != self.selection_request {
            return Err("已打开更新的项目选择，请使用当前窗口".into());
        }
        let Some(root) = root else {
            return Ok(None);
        };
        let selection = ProjectSelection {
            token: Uuid::now_v7().to_string(),
            path: root.to_string_lossy().into_owned(),
        };
        self.selected = Some((selection.clone(), root));
        Ok(Some(selection))
    }

    fn root_for(&self, token: &str) -> Result<&Path, String> {
        self.selected
            .as_ref()
            .filter(|(selection, _)| selection.token == token)
            .map(|(_, root)| root.as_path())
            .ok_or_else(|| "项目会话已改变，请重新打开项目".into())
    }
}

type SessionState = Arc<Mutex<ProjectSession>>;

fn lock_session(state: &SessionState) -> Result<std::sync::MutexGuard<'_, ProjectSession>, String> {
    state.lock().map_err(|_| "项目会话不可用".into())
}

#[tauri::command]
async fn choose_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, SessionState>,
) -> Result<Option<ProjectSelection>, String> {
    let state = Arc::clone(state.inner());
    let request = lock_session(&state)?.begin_selection();
    tauri::async_runtime::spawn_blocking(move || {
        let root = app
            .dialog()
            .file()
            .set_title("打开 Tower.md 项目")
            .blocking_pick_folder()
            .map(|selected| {
                let selected = selected
                    .into_path()
                    .map_err(|error| format!("无法读取所选目录：{error}"))?;
                let root = towermd_core::repo::toplevel(&selected)?;
                root.canonicalize()
                    .map_err(|error| format!("无法定位仓库：{error}"))
            })
            .transpose()?;
        let result = lock_session(&state)?.complete_selection(request, root);
        result
    })
    .await
    .map_err(|error| format!("项目选择失败：{error}"))?
}

#[tauri::command]
async fn load_project(
    token: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<ProjectView, String> {
    let value = request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::View,
    )
    .await?;
    serde_json::from_value(value).map_err(|e| format!("画布响应格式无效：{e}"))
}

#[tauri::command]
async fn start_scan(
    token: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::Scan,
    )
    .await
}

#[tauri::command]
async fn poll_scan(
    token: String,
    id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    uuid::Uuid::parse_str(&id).map_err(|_| "扫描 ID 非法")?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::Poll(id),
    )
    .await
}

async fn request_project(
    token: String,
    state: SessionState,
    backend: BackendState,
    operation: backend::Operation,
) -> Result<serde_json::Value, String> {
    let root = lock_session(&state)?.root_for(&token)?.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let mut backend = backend.lock().map_err(|_| "本机客户端不可用")?;
        lock_session(&state)?.root_for(&token)?;
        let view = backend.request(&root, operation)?;
        lock_session(&state)?.root_for(&token)?;
        Ok(view)
    })
    .await
    .map_err(|error| format!("项目读取失败：{error}"))?
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(SessionState::default())
        .manage(BackendState::default())
        .invoke_handler(tauri::generate_handler![
            choose_project,
            load_project,
            start_scan,
            poll_scan
        ])
        .run(tauri::generate_context!())
        .expect("无法启动 Tower.md");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_selection_keeps_current_project() {
        let mut session = ProjectSession::default();
        let first = session.begin_selection();
        let project = session
            .complete_selection(first, Some(PathBuf::from("/first")))
            .unwrap()
            .unwrap();
        let second = session.begin_selection();
        assert!(session.complete_selection(second, None).unwrap().is_none());
        assert_eq!(
            session.root_for(&project.token).unwrap(),
            Path::new("/first")
        );
    }

    #[test]
    fn selecting_another_project_invalidates_previous_token() {
        let mut session = ProjectSession::default();
        let first = session.begin_selection();
        let old = session
            .complete_selection(first, Some(PathBuf::from("/first")))
            .unwrap()
            .unwrap();
        let second = session.begin_selection();
        let current = session
            .complete_selection(second, Some(PathBuf::from("/second")))
            .unwrap()
            .unwrap();
        assert!(session.root_for(&old.token).is_err());
        assert_eq!(
            session.root_for(&current.token).unwrap(),
            Path::new("/second")
        );
        assert!(session.root_for("/second").is_err());
    }

    #[test]
    fn an_older_dialog_cannot_replace_the_latest_selection() {
        let mut session = ProjectSession::default();
        let first = session.begin_selection();
        let second = session.begin_selection();
        let current = session
            .complete_selection(second, Some(PathBuf::from("/second")))
            .unwrap()
            .unwrap();
        assert!(session
            .complete_selection(first, Some(PathBuf::from("/first")))
            .is_err());
        assert_eq!(
            session.root_for(&current.token).unwrap(),
            Path::new("/second")
        );
    }
}
