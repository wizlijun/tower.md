use crate::{
    cli::{self, CliAction},
    commands::AppError,
};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedProject {
    pub token: String,
    pub display_path: String,
    pub has_config: bool,
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}
#[derive(Default)]
struct RegistryState {
    serial: u64,
    selection: u64,
    current: Option<(String, PathBuf)>,
    initializing: bool,
}
#[derive(Clone, Default)]
pub struct ProjectRegistry(Arc<Mutex<RegistryState>>);

fn canonical_git_root(path: &Path) -> Result<PathBuf, AppError> {
    let mut command = Command::new("git");
    // A folder selected in the App must not be redirected by inherited Git state.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_PREFIX",
        "GIT_GRAFT_FILE",
        "GIT_SHALLOW_FILE",
        "GIT_REPLACE_REF_BASE",
        "GIT_NAMESPACE",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(|name| {
            name.starts_with("GIT_CONFIG_KEY_") || name.starts_with("GIT_CONFIG_VALUE_")
        }) {
            command.env_remove(name);
        }
    }
    let output = command
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| AppError::new("git_unavailable", format!("无法执行 Git：{error}")))?;
    if !output.status.success() {
        let mut error = AppError::new("not_git_project", "请选择 Git 工作树中的文件夹");
        error.detail = Some(String::from_utf8_lossy(&output.stderr).into_owned());
        return Err(error);
    }
    let root = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::new("project_path_invalid", "Git 项目路径不是有效 UTF-8"))?;
    let root = root.strip_suffix('\n').unwrap_or(root);
    Path::new(root).canonicalize().map_err(|error| {
        AppError::new(
            "project_path_invalid",
            format!("无法定位项目根目录：{error}"),
        )
    })
}
fn has_config(root: &Path) -> Result<bool, AppError> {
    // Only inspect the fixed marker; worktree objects are never authority here.
    match std::fs::symlink_metadata(root.join(".towermd")) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(AppError::new(
                "config_marker_invalid",
                ".towermd 不是普通目录，请检查项目配置路径",
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(AppError::new(
                "config_marker_invalid",
                format!("无法检查项目配置目录：{error}"),
            ))
        }
    }
    match std::fs::symlink_metadata(root.join(".towermd/config.yaml")) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(AppError::new(
            "config_marker_invalid",
            "项目配置标记不是普通文件，请检查 .towermd/config.yaml",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::new(
            "config_marker_invalid",
            format!("无法检查项目配置标记：{error}"),
        )),
    }
}
impl ProjectRegistry {
    pub fn begin_selection(&self) -> Result<u64, AppError> {
        let mut state = self.0.lock().unwrap();
        if state.initializing {
            return Err(AppError::new("project_busy", "初始化期间不能切换项目"));
        }
        state.selection += 1;
        Ok(state.selection)
    }
    pub fn finish_selection(
        &self,
        request: u64,
        selection: Option<PathBuf>,
    ) -> Result<Option<SelectedProject>, AppError> {
        let Some(path) = selection else {
            return Ok(None);
        };
        let mut state = self.0.lock().unwrap();
        if request != state.selection {
            return Ok(None);
        }
        if state.initializing {
            return Err(AppError::new("project_busy", "初始化期间不能切换项目"));
        }
        let root = canonical_git_root(&path)?;
        let has_config = has_config(&root)?;
        state.serial += 1;
        let token = format!("project-{}", state.serial);
        state.current = Some((token.clone(), root.clone()));
        Ok(Some(SelectedProject {
            token,
            display_path: root.to_string_lossy().into_owned(),
            has_config,
        }))
    }
    pub fn select(&self, selection: Option<PathBuf>) -> Result<Option<SelectedProject>, AppError> {
        let request = self.begin_selection()?;
        self.finish_selection(request, selection)
    }
    pub fn resolve(&self, token: &str) -> Result<PathBuf, AppError> {
        self.0
            .lock()
            .unwrap()
            .current
            .as_ref()
            .filter(|(current, _)| current == token)
            .map(|(_, root)| root.clone())
            .ok_or_else(|| AppError::new("project_missing", "项目令牌已失效，请重新选择项目"))
    }
    fn prepare_init(&self, token: &str, docs_dir: &str) -> Result<InitPermit, AppError> {
        if docs_dir.trim().is_empty() {
            return Err(AppError::new("docs_dir_empty", "请填写文档目录"));
        }
        let mut state = self.0.lock().unwrap();
        let root = state
            .current
            .as_ref()
            .filter(|(current, _)| current == token)
            .map(|(_, root)| root.clone())
            .ok_or_else(|| AppError::new("project_missing", "项目令牌已失效，请重新选择项目"))?;
        if state.initializing {
            return Err(AppError::new("project_busy", "项目正在初始化"));
        }
        if canonical_git_root(&root)? != root {
            return Err(AppError::new(
                "project_changed",
                "项目路径已变化，请重新选择项目",
            ));
        }
        has_config(&root)?;
        state.selection += 1;
        state.initializing = true;
        Ok(InitPermit {
            registry: self.clone(),
            root,
        })
    }
}
struct InitPermit {
    registry: ProjectRegistry,
    root: PathBuf,
}

pub fn overview(
    registry: &ProjectRegistry,
    token: &str,
) -> Result<towermd_core::project_overview::ProjectOverview, AppError> {
    let root = registry.resolve(token)?;
    if canonical_git_root(&root)? != root {
        return Err(AppError::new(
            "project_changed",
            "项目路径已变化，请重新选择项目",
        ));
    }
    let result = towermd_core::project_overview::load(&root)
        .map_err(|error| AppError::new("project_read_failed", error))?;
    // A late response must not restore a project that the user already switched away from.
    registry.resolve(token)?;
    Ok(result)
}
impl Drop for InitPermit {
    fn drop(&mut self) {
        self.registry.0.lock().unwrap().initializing = false;
    }
}
pub async fn initialize(
    registry: ProjectRegistry,
    token: String,
    docs_dir: String,
) -> Result<InitOutput, AppError> {
    let (permit, docs_dir) = tauri::async_runtime::spawn_blocking(move || {
        registry
            .prepare_init(&token, &docs_dir)
            .map(|permit| (permit, docs_dir))
    })
    .await
    .map_err(|error| AppError::new("project_task_failed", format!("项目检查失败：{error}")))??;
    let output = cli::execute_cli(permit.root.clone(), CliAction::Init { docs_dir }).await?;
    Ok(InitOutput {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialization_blocks_switching_invalidates_pending_dialog_and_releases_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-b", "main"])
            .output()
            .unwrap()
            .status
            .success());
        let registry = ProjectRegistry::default();
        let token = registry
            .select(Some(dir.path().to_path_buf()))
            .unwrap()
            .unwrap()
            .token;
        let pending = registry.begin_selection().unwrap();
        let permit = registry.prepare_init(&token, "docs/tower").unwrap();
        assert_eq!(registry.begin_selection().unwrap_err().code, "project_busy");
        assert!(registry
            .finish_selection(pending, Some(dir.path().to_path_buf()))
            .unwrap()
            .is_none());
        assert!(registry.prepare_init(&token, "docs/tower").is_err());
        drop(permit);
        assert!(registry.resolve(&token).is_ok());
        assert!(registry.begin_selection().is_ok());
    }
}
