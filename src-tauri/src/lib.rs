use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use towermd_core::project_view::ProjectView;
use uuid::Uuid;

mod backend;
mod project_stream;
type BackendState = Arc<Mutex<backend::Backend>>;
type StreamState = Arc<project_stream::Streams>;

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
    streams: tauri::State<'_, StreamState>,
) -> Result<Option<ProjectSelection>, String> {
    let state = Arc::clone(state.inner());
    let streams = Arc::clone(streams.inner());
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
        let mut session = lock_session(&state)?;
        let result = session.complete_selection(request, root);
        if matches!(&result, Ok(Some(_))) { streams.cancel(); }
        result
    })
    .await
    .map_err(|error| format!("项目选择失败：{error}"))?
}

#[tauri::command]
fn subscribe_project(
    token: String,
    subscription_id: String,
    on_message: tauri::ipc::Channel<project_stream::StreamMessage>,
    state: tauri::State<'_, SessionState>,
    streams: tauri::State<'_, StreamState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<(), String> {
    let session = lock_session(state.inner())?;
    let root = session.root_for(&token)?.to_path_buf();
    streams.subscribe(root, token, subscription_id, on_message, Arc::clone(backend.inner()))
}

#[tauri::command]
fn unsubscribe_project(
    subscription_id: String,
    streams: tauri::State<'_, StreamState>,
) -> Result<(), String> {
    streams.unsubscribe(&subscription_id)
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

#[tauri::command]
async fn cancel_scan(
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
        backend::Operation::Cancel(id),
    )
    .await
}

#[tauri::command]
async fn list_baselines(
    token: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::Baselines,
    )
    .await
}

#[tauri::command]
async fn load_baseline(
    token: String,
    id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    backend::validate_baseline_id(&id)?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::Baseline(id),
    )
    .await
}

#[tauri::command]
async fn review_baseline(
    token: String,
    id: String,
    action: towermd_devkit::review::ReviewAction,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    backend::validate_baseline_id(&id)?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::Review(id, action),
    )
    .await
}

#[tauri::command]
async fn verify_baseline(
    token: String,
    id: String,
    request_id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    backend::validate_baseline_id(&id)?;
    backend::validate_verification_id(&request_id)?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::VerifyBaseline(id, request_id),
    )
    .await
}
#[tauri::command]
async fn poll_baseline_verification(
    token: String,
    id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    backend::validate_verification_id(&id)?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::PollVerification(id),
    )
    .await
}
#[tauri::command]
async fn cancel_baseline_verification(
    token: String,
    id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    backend::validate_verification_id(&id)?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::CancelVerification(id),
    )
    .await
}

#[tauri::command]
async fn task_options(
    token: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::TaskOptions).await
}

#[tauri::command]
async fn create_human_task(
    token: String,
    input: backend::HumanTaskRequest,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::CreateHumanTask(input)).await
}

#[tauri::command]
async fn control_project(
    token: String,
    input: backend::ProjectControlRequest,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::ControlProject(input)).await
}

// FR-201–211 human actions; the daemon signs them, so none can name a signer.
#[tauri::command]
async fn finalize_delegation(
    token: String,
    input: backend::DelegationRequest<towermd_core::delegation_projection::Finalized>,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Delegation(backend::DelegationAction::Finalize(input))).await
}

#[tauri::command]
async fn record_acceptance_used(
    token: String,
    input: backend::DelegationRequest<towermd_core::delegation_projection::Used>,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Delegation(backend::DelegationAction::Used(input))).await
}

#[tauri::command]
async fn confirm_acceptance(
    token: String,
    input: backend::DelegationRequest<towermd_core::delegation_projection::Confirmed>,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Delegation(backend::DelegationAction::Confirm(input))).await
}

#[tauri::command]
async fn send_back_acceptance(
    token: String,
    input: backend::DelegationRequest<towermd_core::delegation_projection::SentBack>,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Delegation(backend::DelegationAction::SendBack(input))).await
}

#[tauri::command]
async fn record_rule(
    token: String,
    input: backend::DelegationRequest<towermd_core::delegation_projection::RuleRecorded>,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Delegation(backend::DelegationAction::Rule(input))).await
}

#[tauri::command]
async fn rename_work_kind(
    token: String,
    input: backend::LedgerRequest<towermd_core::delegation_projection::Renamed>,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Delegation(backend::DelegationAction::RenameWorkKind(input))).await
}

#[tauri::command]
async fn set_delegation_level(
    token: String,
    input: backend::LedgerRequest<towermd_core::delegation_projection::LevelSet>,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Delegation(backend::DelegationAction::SetLevel(input))).await
}

#[tauri::command]
async fn dismiss_signal(
    token: String,
    input: backend::LedgerRequest<towermd_core::tower_api::Dismissed>,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Delegation(backend::DelegationAction::DismissSignal(input))).await
}

#[tauri::command]
async fn add_idea(
    token: String,
    input: backend::IdeaRequest,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::AddIdea(input)).await
}

#[tauri::command]
async fn promote_idea(
    token: String,
    input: backend::PromoteIdeaRequest,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::PromoteIdea(input)).await
}

#[tauri::command]
async fn answer_question(
    token: String,
    input: backend::AnswerRequest,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    input.validate().map_err(|error| format!("操作已拒绝：{error}"))?;
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::AnswerQuestion(input)).await
}

#[tauri::command]
async fn send_prompt(
    token: String,
    input: backend::PromptRequest,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    input.validate().map_err(|error| format!("操作已拒绝：{error}"))?;
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::SendPrompt(input)).await
}

#[tauri::command]
async fn capture_main_proposal(
    token: String,
    commit: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::CaptureMainProposal(commit)).await
}

#[tauri::command]
async fn list_proposals(
    token: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::Proposals).await
}

#[tauri::command]
async fn prepare_proposal(
    token: String,
    id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::PrepareProposal(id)).await
}

#[tauri::command]
async fn approve_proposal(
    token: String,
    id: String,
    approval: backend::PublicationApproval,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::ApproveProposal(id, approval)).await
}

#[tauri::command]
async fn reject_proposal(
    token: String,
    id: String,
    input: backend::ProposalRejection,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::RejectProposal(id, input)).await
}

#[tauri::command]
async fn run_details(
    token: String,
    id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::RunDetails(id)).await
}

#[tauri::command]
async fn prepare_main_resolution(
    token: String,
    operation: towermd_core::main_resolution::Operation,
    commit: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::PrepareMainResolution(operation, commit)).await
}

#[tauri::command]
async fn confirm_main_resolution(
    token: String,
    preview: towermd_core::main_resolution::Preview,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(token, Arc::clone(state.inner()), Arc::clone(backend.inner()),
        backend::Operation::ConfirmMainResolution(preview)).await
}

#[tauri::command]
async fn project_state(
    token: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::State,
    )
    .await
}
#[tauri::command]
async fn prepare_publication(
    token: String,
    id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    backend::validate_baseline_id(&id)?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::PreparePublication(id),
    )
    .await
}
#[tauri::command]
async fn approve_publication(
    token: String,
    id: String,
    approval: backend::PublicationApproval,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    backend::validate_baseline_id(&id)?;
    approval.validate()?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::ApprovePublication(id, approval),
    )
    .await
}
#[tauri::command]
async fn request_recovery(
    token: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::RequestRecovery,
    )
    .await
}
#[tauri::command]
async fn confirm_recovery(
    token: String,
    id: String,
    state: tauri::State<'_, SessionState>,
    backend: tauri::State<'_, BackendState>,
) -> Result<serde_json::Value, String> {
    backend::validate_human_request_id(&id)?;
    request_project(
        token,
        Arc::clone(state.inner()),
        Arc::clone(backend.inner()),
        backend::Operation::ConfirmRecovery(id),
    )
    .await
}

fn relative_reference(root: &Path, path: &Path) -> Result<String, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("无法定位引用文件：{e}"))?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "只能引用当前项目中的 Markdown 文件")?;
    if path.extension().and_then(|value| value.to_str()) != Some("md") || !path.is_file() {
        return Err("请拖入一个 Markdown 文件".into());
    }
    relative
        .to_str()
        .map(|value| value.replace(std::path::MAIN_SEPARATOR, "/"))
        .ok_or_else(|| "引用路径不是有效文本".into())
}

#[tauri::command]
fn reference_path(
    token: String,
    path: String,
    state: tauri::State<'_, SessionState>,
) -> Result<String, String> {
    let session = lock_session(state.inner())?;
    relative_reference(session.root_for(&token)?, Path::new(&path))
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
        .manage(StreamState::default())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) {
                window.state::<StreamState>().cancel();
            }
        })
        .invoke_handler(tauri::generate_handler![
            choose_project,
            subscribe_project,
            unsubscribe_project,
            load_project,
            start_scan,
            poll_scan,
            cancel_scan,
            list_baselines,
            load_baseline,
            review_baseline,
            verify_baseline,
            poll_baseline_verification,
            cancel_baseline_verification,
            project_state,
            run_details,
            list_proposals,
            capture_main_proposal,
            prepare_proposal,
            approve_proposal,
            reject_proposal,
            prepare_main_resolution,
            confirm_main_resolution,
            answer_question,
            send_prompt,
            create_human_task,
            task_options,
            control_project,
            finalize_delegation,
            record_acceptance_used,
            confirm_acceptance,
            send_back_acceptance,
            record_rule,
            rename_work_kind,
            set_delegation_level,
            dismiss_signal,
            add_idea,
            promote_idea,
            prepare_publication,
            approve_publication,
            request_recovery,
            confirm_recovery,
            reference_path
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
    #[test]
    fn references_are_markdown_files_inside_the_selected_project() {
        let base = std::env::temp_dir().join(format!("towermd-reference-{}", Uuid::now_v7()));
        let root = base.join("project");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/design.md"), "draft").unwrap();
        std::fs::write(root.join("script.sh"), "exit 0").unwrap();
        std::fs::write(base.join("outside.md"), "outside").unwrap();
        let root = root.canonicalize().unwrap();
        assert_eq!(
            relative_reference(&root, &root.join("docs/design.md")).unwrap(),
            "docs/design.md"
        );
        assert!(relative_reference(&root, &base.join("outside.md")).is_err());
        assert!(relative_reference(&root, &root.join("script.sh")).is_err());
        assert!(relative_reference(&root, &root.join("missing.md")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("outside.md"), root.join("escape.md")).unwrap();
            assert!(relative_reference(&root, &root.join("escape.md")).is_err());
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn review_action_does_not_accept_caller_identity_or_approval() {
        use towermd_devkit::review::ReviewAction;
        assert!(serde_json::from_value::<ReviewAction>(serde_json::json!({
            "kind": "rename", "node_id": "node", "title": "name", "actor": "human:other"
        }))
        .is_err());
        assert!(serde_json::from_value::<ReviewAction>(serde_json::json!({
            "kind": "approve", "id": "B0"
        }))
        .is_err());
    }
}
