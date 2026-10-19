//! Fixed native-only scan transport. The WebView supplies a project token and
//! an issued scan id; it cannot choose an endpoint, path, method, actor or argv.
use crate::{
    cli::{self, CliAction},
    commands::AppError,
    project::ProjectRegistry,
};
use reqwest::{
    blocking::{Client, Response},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Endpoint {
    address: String,
    port: u16,
    pid: u32,
    instance: String,
    host: String,
}
impl Endpoint {
    fn validate(&self) -> Result<(), AppError> {
        if self.address != "127.0.0.1"
            || self.port == 0
            || self.pid == 0
            || self.instance.is_empty()
            || self.host.is_empty()
        {
            return Err(AppError::new(
                "protocol_error",
                "daemon endpoint 不是有效的本机实例",
            ));
        }
        Ok(())
    }
}
pub(crate) struct DaemonClient {
    endpoint: Endpoint,
    http: Client,
}
impl DaemonClient {
    pub(crate) fn connect(endpoint: Endpoint) -> Result<Self, AppError> {
        endpoint.validate()?;
        let http = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(95))
            .build()
            .map_err(|e| AppError::new("daemon_unavailable", e.to_string()))?;
        let client = Self { endpoint, http };
        let health = client
            .http
            .get(client.url("/health"))
            .timeout(Duration::from_secs(3))
            .send()
            .map_err(network_error)?;
        let actual: Endpoint = serde_json::from_value(read_response(health)?)
            .map_err(|e| AppError::new("protocol_error", format!("daemon health 无效：{e}")))?;
        actual.validate()?;
        if actual != client.endpoint {
            return Err(AppError::new(
                "daemon_changed",
                "daemon 实例已变化，请重新等待；不会停止其他实例",
            ));
        }
        Ok(client)
    }
    pub(crate) fn instance(&self) -> &str {
        &self.endpoint.instance
    }
    pub(crate) fn saved_rebuild(&self, id: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url(&format!("/hub/rebuild/{id}")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .send()
            .map_err(network_error)?;
        read_rebuild_response(response, false)
    }
    pub(crate) fn confirm_rebuild(&self, id: &str, actor: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url(&format!("/hub/rebuild/{id}/confirm")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(&json!({"actor":actor}))
            .send()
            .map_err(|error| {
                AppError::new(
                    "recovery_uncertain",
                    format!("原恢复请求送达状态不确定，不会自动重试：{error}"),
                )
            })?;
        read_rebuild_response(response, true)
    }
    pub(crate) fn host(&self) -> &str {
        &self.endpoint.host
    }
    pub(crate) fn interventions(&self, root: &Path) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url("/app/interventions"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )])
            .send()
            .map_err(network_error)?;
        let value = read_response(response)?;
        self.check_task_review_instance()?;
        Ok(value)
    }
    pub(crate) fn intervention_action(
        &self,
        request: &crate::intervention_requests::ControlHttpRequest<'_>,
    ) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url("/app/interventions/action"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(request)
            .send()
            .map_err(network_error)?;
        let result = read_mutation_response(response);
        self.check_task_review_instance()?;
        result
    }
    pub(crate) fn verify_control_instance(&self) -> Result<(), AppError> {
        self.check_task_review_instance()
    }
    pub(crate) fn saved(&self, root: &Path, id: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url(&format!("/app/drafts/{id}")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )])
            .send()
            .map_err(network_error)?;
        read_response(response)
    }
    pub(crate) fn current_baseline(&self, root: &Path) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url("/app/baselines/current"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )])
            .send()
            .map_err(network_error)?;
        read_response(response)
    }
    pub(crate) fn create(
        &self,
        request: &crate::draft_requests::CreateHttpRequest,
    ) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url("/drafts"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(request)
            .send()
            .map_err(network_error)?;
        read_mutation_response(response)
    }
    pub(crate) fn review(&self, root: &Path, id: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url(&format!("/app/baselines/{id}/review")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )])
            .send()
            .map_err(network_error)?;
        read_response(response)
    }
    pub(crate) fn review_action(
        &self,
        id: &str,
        request: &crate::review_requests::ReviewHttpRequest,
    ) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url(&format!("/app/baselines/{id}/action")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(request)
            .send()
            .map_err(network_error)?;
        read_mutation_response(response)
    }
    // Task Review uses only these fixed routes. Recheck health after reading the
    // response so a daemon replaced during the request cannot certify a result.
    fn check_task_review_instance(&self) -> Result<(), AppError> {
        let response = self
            .http
            .get(self.url("/health"))
            .timeout(Duration::from_secs(3))
            .send()
            .map_err(network_error)?;
        let actual: Endpoint = serde_json::from_value(read_response(response)?)
            .map_err(|e| AppError::new("protocol_error", e.to_string()))?;
        actual.validate()?;
        if actual != self.endpoint {
            return Err(AppError::new(
                "daemon_changed",
                "Task 审阅响应期间 daemon 实例已变化；保留原请求",
            ));
        }
        Ok(())
    }
    pub(crate) fn progress_stream(&self, root: &Path, after: u64) -> Result<String, AppError> {
        // The WebView gets only this fixed local read-only route. It cannot choose a
        // host, route, directory, actor, or write transport through this command.
        let mut url = reqwest::Url::parse(&self.url("/app/events"))
            .map_err(|error| AppError::new("protocol_error", error.to_string()))?;
        url.set_scheme("ws")
            .map_err(|_| AppError::new("protocol_error", "事件地址无效"))?;
        url.query_pairs_mut()
            .append_pair(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )
            .append_pair("instance", &self.endpoint.instance)
            .append_pair("after", &after.to_string());
        self.check_task_review_instance()?;
        Ok(url.into())
    }
    pub(crate) fn task_progress(&self, root: &Path) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url("/app/projection"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )])
            .send()
            .map_err(|error| {
                AppError::new("daemon_unavailable", format!("无法读取任务进度：{error}"))
            })?;
        let status = response.status();
        if status.is_redirection() {
            return Err(AppError::new(
                "protocol_error",
                "拒绝任务进度接口 HTTP 重定向",
            ));
        }
        let body: Value = response.json().map_err(|error| {
            AppError::new(
                "protocol_error",
                format!("任务进度响应不是有效 JSON：{error}"),
            )
        })?;
        if !status.is_success() {
            let code = match status.as_u16() {
                404 | 405 => "daemon_incompatible",
                409 if body["code"] == "daemon_changed" => "daemon_changed",
                409 => "task_progress_source_changed",
                _ => "task_progress_unavailable",
            };
            let mut error = AppError::new(
                code,
                body["error"].as_str().unwrap_or(
                    "当前 daemon 不支持任务进度读取，请核对随包版本；不会自动重启其他实例",
                ),
            );
            error.detail = Some(body.to_string());
            return Err(error);
        }
        self.check_task_review_instance()?;
        Ok(body)
    }
    pub(crate) fn task_review(&self, root: &Path, hold: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url(&format!("/app/interventions/reviews/{hold}")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )])
            .send()
            .map_err(network_error)?;
        let value = read_response(response)?;
        self.check_task_review_instance()?;
        Ok(value)
    }
    pub(crate) fn task_review_action(
        &self,
        request: &crate::task_review_requests::TaskReviewHttpRequest,
    ) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url("/app/interventions/action"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(request)
            .send()
            .map_err(network_error)?;
        let result = read_mutation_response(response);
        self.check_task_review_instance()?;
        result
    }
    pub(crate) fn task_recheck(&self, root: &Path, hold: &str) -> Result<Value, AppError> {
        crate::draft_requests::safe_id(hold)?;
        let response = self
            .http
            .get(self.url(&format!("/app/tasks/{hold}/recheck")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )])
            .send()
            .map_err(network_error)?;
        let value = read_response(response)?;
        self.check_task_review_instance()?;
        Ok(value)
    }
    pub(crate) fn task_recheck_action(
        &self,
        request: &crate::task_recheck_requests::TaskRecheckHttpRequest,
    ) -> Result<Value, AppError> {
        crate::draft_requests::safe_id(&request.action.binding.task_id)?;
        let response = self
            .http
            .post(self.url(&format!(
                "/app/tasks/{}/recheck/resolve",
                request.action.binding.task_id
            )))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(request)
            .send()
            .map_err(network_error)?;
        let result = read_mutation_response(response);
        self.check_task_review_instance()?;
        result
    }
    pub(crate) fn ask_answer(&self, root: &Path, hold: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url(&format!("/app/interventions/asks/{hold}")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str().ok_or_else(|| {
                    AppError::new("project_path_invalid", "Ask path is not UTF-8")
                })?,
            )])
            .send()
            .map_err(network_error)?;
        let value = read_response(response)?;
        self.check_task_review_instance()?;
        Ok(value)
    }
    pub(crate) fn ask_answer_action(
        &self,
        request: &crate::ask_answer_requests::AskAnswerHttpRequest,
    ) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url("/app/interventions/action"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(request)
            .send()
            .map_err(network_error)?;
        let result = read_mutation_response(response);
        self.check_task_review_instance()?;
        result
    }
    pub(crate) fn edit(
        &self,
        request: &crate::draft_requests::EditHttpRequest,
    ) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url("/drafts"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(request)
            .send()
            .map_err(network_error)?;
        read_mutation_response(response)
    }
    pub(crate) fn original_request(
        &self,
        root: &Path,
        id: &str,
    ) -> Result<Option<Value>, AppError> {
        let response = self
            .http
            .get(self.url(&format!("/app/requests/{id}")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是UTF-8"))?,
            )])
            .send()
            .map_err(|error| {
                AppError::new(
                    "request_uncertain",
                    format!("原请求 {id} 的状态读取失败：{error}"),
                )
            })?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        read_mutation_response(response).map(Some)
    }
    pub(crate) fn retry_original(&self, root: &Path, id: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url(&format!("/app/requests/{id}/retry")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .header("x-towermd-client", "app")
            .json(&json!({"directory":root}))
            .send()
            .map_err(network_error)?;
        read_mutation_response(response)
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.endpoint.port)
    }
    fn scan(&self, root: &Path, wait: Option<&str>) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.url("/baseline/coldstart"))
            .header("x-towermd-instance", &self.endpoint.instance)
            .json(&json!({"directory":root,"run":null,"options":null,"wait":wait}))
            .send()
            .map_err(network_error)?;
        let value = read_response(response)?;
        validate_status(&value, wait)?;
        Ok(value)
    }
    fn draft(&self, root: &Path, id: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .get(self.url(&format!("/drafts/{id}")))
            .header("x-towermd-instance", &self.endpoint.instance)
            .query(&[(
                "directory",
                root.to_str()
                    .ok_or_else(|| AppError::new("project_path_invalid", "项目路径不是 UTF-8"))?,
            )])
            .send()
            .map_err(network_error)?;
        let value = read_response(response)?;
        validate_status(&value, Some(id))?;
        if value["state"] == "completed" {
            let catalog = towermd_core::catalog::Catalog::load(root)
                .map_err(|e| AppError::new("project_read_failed", e))?;
            if value["input"]["project_id"] != catalog.config.project_id()
                || value["input"]["common_dir"] != catalog.common_dir.to_string_lossy().as_ref()
                || !value["modules"].is_array()
                || !value["canvas"]["nodes"].is_array()
                || !value["canvas"]["edges"].is_array()
                || !value["evidence"].is_array()
                || !value["report_eid"].is_string()
                || value["verification_report"]["commit"] != value["input"]["revision"]
                || value["verification_report"]["tree"] != value["input"]["tree"]
            {
                return Err(AppError::new(
                    "protocol_error",
                    "候选结果缺少真实来源或与所选项目不一致",
                ));
            }
            for key in ["revision", "tree"] {
                let oid = value["input"][key].as_str().unwrap_or_default();
                if !matches!(oid.len(), 40 | 64)
                    || !oid.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(AppError::new(
                        "protocol_error",
                        format!("候选 input.{key} 不是 Git 对象 ID"),
                    ));
                }
            }
        }
        Ok(value)
    }
}
fn read_rebuild_response(response: Response, mutation: bool) -> Result<Value, AppError> {
    let status = response.status();
    let text = response.text().map_err(|error| {
        AppError::new(
            if mutation {
                "recovery_uncertain"
            } else {
                "daemon_unavailable"
            },
            error.to_string(),
        )
    })?;
    if status.is_redirection() {
        return Err(AppError::new("protocol_error", "拒绝恢复接口 HTTP 重定向"));
    }
    if !status.is_success() {
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let code = match status.as_u16() {
            403 => "recovery_forbidden",
            404 => "recovery_missing",
            409 => "recovery_conflict",
            500..=599 if mutation => "recovery_uncertain",
            _ => "recovery_failed",
        };
        let mut error = AppError::new(
            code,
            value["error"].as_str().unwrap_or("daemon 未接受原恢复请求"),
        );
        error.detail = Some(text);
        return Err(error);
    }
    serde_json::from_str(&text).map_err(|error| {
        AppError::new(
            if mutation {
                "recovery_uncertain"
            } else {
                "protocol_error"
            },
            format!("恢复响应 JSON 无效：{error}"),
        )
    })
}

fn read_mutation_response(response: Response) -> Result<Value, AppError> {
    let status = response.status();
    let text = response.text().map_err(network_error)?;
    if !status.is_success() {
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let code = if status.as_u16() == 409 {
            "request_conflict"
        } else if status.as_u16() == 503 || status.is_server_error() {
            "request_uncertain"
        } else {
            "request_refused"
        };
        let mut error = AppError::new(code, value["error"].as_str().unwrap_or("草稿请求未获确认"));
        error.detail = Some(text);
        return Err(error);
    }
    serde_json::from_str(&text)
        .map_err(|e| AppError::new("request_uncertain", format!("保存回执 JSON 无效：{e}")))
}

fn read_response(response: Response) -> Result<Value, AppError> {
    let status = response.status();
    let text = response.text().map_err(network_error)?;
    if status.is_redirection() {
        return Err(AppError::new("protocol_error", "拒绝 daemon HTTP 重定向"));
    }
    if !status.is_success() {
        let code = if status.as_u16() == 409 {
            "daemon_changed"
        } else if status.as_u16() == 404 || status.as_u16() == 405 {
            "daemon_incompatible"
        } else {
            "scan_failed"
        };
        let mut error = AppError::new(
            code,
            if code == "daemon_incompatible" {
                "当前 daemon 不支持候选扫描接口或该扫描不存在；请检查 CLI/daemon 版本，不会自动停止其他实例。"
            } else {
                "daemon 未接受本次扫描操作，请查看原始诊断。"
            },
        );
        error.detail = Some(text);
        return Err(error);
    }
    serde_json::from_str(&text)
        .map_err(|e| AppError::new("protocol_error", format!("daemon 返回的 JSON 无效：{e}")))
}
fn network_error(error: reqwest::Error) -> AppError {
    AppError::new(
        "daemon_unavailable",
        format!("本机 daemon 请求失败；已启动的扫描不会因此重跑：{error}"),
    )
}
fn validate_status(value: &Value, expected: Option<&str>) -> Result<(), AppError> {
    let id = value["scan_id"].as_str().filter(|id| valid_scan_id(id));
    if id.is_none()
        || value["state"].as_str().is_none_or(str::is_empty)
        || expected.is_some_and(|expected| Some(expected) != id)
    {
        return Err(AppError::new("daemon_incompatible","当前 daemon 未返回兼容的扫描身份或状态；不会使用旧同步结果冒充已完成，也不会自动重启 daemon。"));
    }
    Ok(())
}
fn valid_scan_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
pub(crate) fn discover(app_data: &Path) -> Result<DaemonClient, AppError> {
    let cwd = app_data.join("daemon-client");
    std::fs::create_dir_all(&cwd)
        .map_err(|e| AppError::new("app_data_unavailable", e.to_string()))?;
    if crate::project::canonical_git_root(&cwd).is_ok() {
        return Err(AppError::new(
            "app_data_invalid",
            "daemon Ensure 的 App 数据目录不能在 Git 工作树内",
        ));
    }
    let exe = std::env::current_exe().map_err(|e| AppError::new("tool_missing", e.to_string()))?;
    let cli = cli::resolve_cli(&exe, cfg!(debug_assertions))?;
    let output = cli::run_cli(&cli, &cwd, CliAction::Ensure)?;
    let endpoint: Endpoint = serde_json::from_str(&output.stdout).map_err(|e| {
        AppError::new(
            "daemon_incompatible",
            format!("随包 CLI 的 Ensure 输出无效：{e}"),
        )
    })?;
    DaemonClient::connect(endpoint)
}

#[derive(Default, Clone)]
pub struct CandidateScans(Arc<Mutex<BTreeMap<String, Progress>>>);
enum Progress {
    Starting,
    Active(Value),
    Uncertain,
}
impl CandidateScans {
    fn begin(&self, token: &str) -> Result<Option<Value>, AppError> {
        let mut scans = self
            .0
            .lock()
            .map_err(|_| AppError::new("scan_busy", "扫描状态锁不可用"))?;
        match scans.get(token) {
            Some(Progress::Active(value)) => return Ok(Some(value.clone())),
            Some(Progress::Starting) => {
                return Err(AppError::new(
                    "scan_busy",
                    "该项目正在启动扫描，请勿重复点击",
                ))
            }
            Some(Progress::Uncertain) => {
                return Err(AppError::new(
                    "scan_start_uncertain",
                    "扫描启动送达状态不确定，未取得扫描 ID；请查看 daemon 诊断，不会自动重复扫描。",
                ))
            }
            None => {}
        }
        scans.retain(|key, _| key == token);
        scans.insert(token.into(), Progress::Starting);
        Ok(None)
    }
    fn check(&self, token: &str, id: &str) -> Result<(), AppError> {
        if !valid_scan_id(id) {
            return Err(AppError::new("scan_missing", "扫描 ID 无效"));
        }
        let scans = self
            .0
            .lock()
            .map_err(|_| AppError::new("scan_busy", "扫描状态锁不可用"))?;
        if !matches!(scans.get(token),Some(Progress::Active(value)) if value["scan_id"]==id) {
            return Err(AppError::new(
                "scan_missing",
                "该扫描不是当前项目窗口取得的扫描，请重新选择项目",
            ));
        }
        Ok(())
    }
    fn remember(&self, token: &str, value: &Value) {
        if let Ok(mut scans) = self.0.lock() {
            scans.insert(token.into(), Progress::Active(value.clone()));
        }
    }
}
pub fn start(
    registry: &ProjectRegistry,
    scans: &CandidateScans,
    token: &str,
    app_data: &Path,
) -> Result<Value, AppError> {
    start_with(registry, scans, token, || discover(app_data))
}
fn start_with(
    registry: &ProjectRegistry,
    scans: &CandidateScans,
    token: &str,
    connect: impl FnOnce() -> Result<DaemonClient, AppError>,
) -> Result<Value, AppError> {
    let root = registry.initialized_root(token)?;
    if let Some(existing) = scans.begin(token)? {
        return Ok(existing);
    }
    let client = match connect() {
        Ok(client) => client,
        Err(error) => {
            scans.0.lock().unwrap().remove(token);
            return Err(error);
        }
    };
    registry.resolve(token)?;
    let value = match client.scan(&root, None) {
        Ok(value) => value,
        Err(error) => {
            scans
                .0
                .lock()
                .unwrap()
                .insert(token.into(), Progress::Uncertain);
            return Err(error);
        }
    };
    registry.resolve(token)?;
    scans.remember(token, &value);
    Ok(value)
}
pub fn wait(
    registry: &ProjectRegistry,
    scans: &CandidateScans,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<Value, AppError> {
    let root = registry.initialized_root(token)?;
    scans.check(token, id)?;
    let client = discover(app_data)?;
    registry.resolve(token)?;
    let value = client.scan(&root, Some(id))?;
    registry.resolve(token)?;
    scans.remember(token, &value);
    Ok(value)
}
pub fn draft(
    registry: &ProjectRegistry,
    scans: &CandidateScans,
    token: &str,
    id: &str,
    app_data: &Path,
) -> Result<Value, AppError> {
    let root = registry.initialized_root(token)?;
    scans.check(token, id)?;
    let client = discover(app_data)?;
    registry.resolve(token)?;
    let value = client.draft(&root, id)?;
    registry.resolve(token)?;
    Ok(value)
}

#[cfg(test)]
#[path = "daemon_client_tests.rs"]
pub(crate) mod tests;
