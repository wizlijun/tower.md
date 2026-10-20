//! Native-only bridge. The WebView never receives a daemon address or chooses
//! an executable; only the packaged CLI may start the per-user daemon.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Deserialize)]
struct Endpoint {
    address: String,
    port: u16,
    instance: String,
}

impl Endpoint {
    fn validate(&self) -> Result<(), String> {
        if self.address != "127.0.0.1" || self.port == 0 || self.instance.is_empty() {
            return Err("守护进程返回了非法的本机地址".into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Backend {
    endpoint: Option<Endpoint>,
    projects: BTreeMap<PathBuf, String>,
}

pub enum Operation {
    View,
    State,
    Scan,
    Poll(String),
    Cancel(String),
    Baselines,
    Baseline(String),
    Review(String, towermd_core::review::ReviewAction),
    VerifyBaseline(String, String),
    PollVerification(String),
    CancelVerification(String),
    PreparePublication(String),
    ApprovePublication(String, PublicationApproval),
    RequestRecovery,
    ConfirmRecovery(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationApproval {
    pub request_id: String,
    pub target: String,
    pub prepared_at: String,
    pub content_hash: String,
}
impl PublicationApproval {
    pub fn validate(&self) -> Result<(), String> {
        validate_human_request_id(&self.request_id)?;
        let number: u64 = self
            .target
            .strip_prefix('B')
            .ok_or("基线编号无效")?
            .parse()
            .map_err(|_| "基线编号无效")?;
        if self.target != format!("B{number}")
            || self.prepared_at.is_empty()
            || self.prepared_at.len() > 128
            || self.prepared_at.chars().any(char::is_control)
            || self.content_hash.len() != 64
            || !self
                .content_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("批准预览元数据无效".into());
        }
        Ok(())
    }
}

pub fn validate_human_request_id(id: &str) -> Result<(), String> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| "人工确认请求 ID 非法")?;
    if parsed.get_version_num() != 7 || parsed.to_string() != id {
        return Err("人工确认请求 ID 非法".into());
    }
    Ok(())
}

impl Backend {
    pub fn request(&mut self, root: &Path, operation: Operation) -> Result<Value, String> {
        let preview_only = matches!(&operation, Operation::PreparePublication(_));
        if self.endpoint.is_none() {
            self.endpoint = Some(ensure()?);
        }
        let endpoint = self.endpoint.clone().ok_or("守护进程未连接")?;
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|e| e.to_string())?;
        let result = (|| {
            let handle = match self.projects.get(root) {
                Some(handle) => handle.clone(),
                None => {
                    let opened = send(
                        &client,
                        &endpoint,
                        Method::POST,
                        "/projects/open",
                        Some(json!({"path": root})),
                    )?;
                    let handle = opened["handle"]
                        .as_str()
                        .ok_or("项目服务缺少 handle")?
                        .to_owned();
                    uuid::Uuid::parse_str(&handle).map_err(|_| "项目服务 handle 非法")?;
                    self.projects.insert(root.to_owned(), handle.clone());
                    handle
                }
            };
            let mut body = None;
            let (method, path) = match operation {
                Operation::View => (Method::GET, format!("/projects/{handle}/view")),
                Operation::State => (Method::GET, format!("/projects/{handle}/state")),
                Operation::PreparePublication(id) => {
                    validate_baseline_id(&id)?;
                    body = Some(json!({}));
                    (
                        Method::POST,
                        format!("/projects/{handle}/baselines/{id}/publication/prepare"),
                    )
                }
                Operation::ApprovePublication(id, approval) => {
                    validate_baseline_id(&id)?;
                    approval.validate()?;
                    body = Some(json!(approval));
                    (
                        Method::POST,
                        format!("/projects/{handle}/baselines/{id}/publication/approve"),
                    )
                }
                Operation::RequestRecovery => {
                    body = Some(json!({}));
                    (Method::POST, format!("/projects/{handle}/hub/rebuild"))
                }
                Operation::ConfirmRecovery(id) => {
                    validate_human_request_id(&id)?;
                    body = Some(json!({}));
                    (
                        Method::POST,
                        format!("/projects/{handle}/hub/rebuild/{id}/confirm"),
                    )
                }
                Operation::Scan => (Method::POST, format!("/projects/{handle}/scans")),
                Operation::Poll(id) => {
                    uuid::Uuid::parse_str(&id).map_err(|_| "扫描 ID 非法")?;
                    (Method::GET, format!("/projects/{handle}/scans/{id}"))
                }
                Operation::Cancel(id) => {
                    uuid::Uuid::parse_str(&id).map_err(|_| "扫描 ID 非法")?;
                    (
                        Method::POST,
                        format!("/projects/{handle}/scans/{id}/cancel"),
                    )
                }
                Operation::Baselines => (Method::GET, format!("/projects/{handle}/baselines")),
                Operation::Review(id, action) => {
                    validate_baseline_id(&id)?;
                    body = Some(json!({"action": action}));
                    (
                        Method::POST,
                        format!("/projects/{handle}/baselines/{id}/review"),
                    )
                }
                Operation::VerifyBaseline(id, request_id) => {
                    validate_baseline_id(&id)?;
                    validate_verification_id(&request_id)?;
                    body = Some(json!({"request_id": request_id}));
                    (
                        Method::POST,
                        format!("/projects/{handle}/baselines/{id}/verify"),
                    )
                }
                Operation::PollVerification(id) => {
                    validate_verification_id(&id)?;
                    (
                        Method::GET,
                        format!("/projects/{handle}/baseline-verifications/{id}"),
                    )
                }
                Operation::CancelVerification(id) => {
                    validate_verification_id(&id)?;
                    (
                        Method::POST,
                        format!("/projects/{handle}/baseline-verifications/{id}/cancel"),
                    )
                }
                Operation::Baseline(id) => {
                    validate_baseline_id(&id)?;
                    (Method::GET, format!("/projects/{handle}/baselines/{id}"))
                }
            };
            let mut value = send(&client, &endpoint, method, &path, body)?;
            if preview_only {
                if let Some(object) = value.as_object_mut() {
                    object.remove("files");
                }
            }
            Ok(value)
        })();
        if result.is_err() {
            // Retry is explicit in the App. The next attempt rechecks the
            // daemon instance and reopens this project's canonical path.
            self.endpoint = None;
            self.projects.clear();
        }
        result
    }
}

pub fn validate_baseline_id(id: &str) -> Result<(), String> {
    let digest = id.strip_prefix("d-").ok_or("草稿 ID 非法")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("草稿 ID 非法".into());
    }
    Ok(())
}

pub fn validate_verification_id(id: &str) -> Result<(), String> {
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| "验证请求 ID 非法")?;
    if !matches!(uuid.get_version_num(), 4 | 7) || uuid.to_string() != id {
        return Err("验证请求 ID 非法".into());
    }
    Ok(())
}

fn send(
    client: &Client,
    endpoint: &Endpoint,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let mut request = client
        .request(method, format!("http://127.0.0.1:{}{path}", endpoint.port))
        .header("x-towermd-instance", &endpoint.instance)
        .header("x-towermd-client", "app");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .map_err(|e| format!("无法连接本机服务，请重试：{e}"))?;
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("项目读取结果超过 16 MiB，请缩小扫描范围".into());
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("本机服务返回了无法解析的响应（{status}）：{e}"))?;
    if !status.is_success() {
        let error = value["error"].as_str().unwrap_or("本机服务请求失败");
        return Err(
            if (path.ends_with("/review") || path.ends_with("/verify")) && status.is_client_error()
            {
                format!("操作已拒绝：{error}")
            } else {
                error.into()
            },
        );
    }
    Ok(value)
}

fn executable() -> Result<PathBuf, String> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let path = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "binaries/towermd-{}{suffix}",
            env!("TOWERMD_TARGET")
        ))
    } else {
        std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("App 没有可执行目录")?
            .join(format!("towermd{suffix}"))
    };
    if !path.is_file() {
        return Err("缺少随 App 打包的 towermd，请重新运行 pnpm app:build 或 pnpm app:dev".into());
    }
    Ok(path)
}

fn ensure() -> Result<Endpoint, String> {
    let mut child = Command::new(executable()?)
        .args(["daemon", "ensure"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动打包的命令行：{e}"))?;
    let mut stdout = child.stdout.take().ok_or("缺少 CLI 输出")?;
    let mut stderr = child.stderr.take().ok_or("缺少 CLI 错误输出")?;
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            other => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match other {
                    Err(error) => error.to_string(),
                    _ => "守护进程启动超时，请检查 daemon 日志".into(),
                });
            }
        }
    };
    let stdout = out
        .join()
        .map_err(|_| "读取 CLI 输出失败")?
        .map_err(|e| e.to_string())?;
    let stderr = err
        .join()
        .map_err(|_| "读取 CLI 错误失败")?
        .map_err(|e| e.to_string())?;
    if !status?.success() {
        return Err(String::from_utf8_lossy(&stderr).trim().to_string());
    }
    let endpoint: Endpoint =
        serde_json::from_slice(&stdout).map_err(|e| format!("无法读取守护进程地址：{e}"))?;
    endpoint.validate()?;
    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_confirmation_has_no_caller_identity_or_file_bytes() {
        let approval = json!({
            "request_id": uuid::Uuid::now_v7().to_string(), "target": "B0",
            "prepared_at": "2026-10-21T00:00:00Z", "content_hash": "a".repeat(64),
        });
        serde_json::from_value::<PublicationApproval>(approval.clone())
            .unwrap()
            .validate()
            .unwrap();
        for (key, value) in [
            ("actor", json!("human:other")),
            ("files", json!({})),
            ("approved", json!(true)),
        ] {
            let mut forged = approval.clone();
            forged[key] = value;
            assert!(serde_json::from_value::<PublicationApproval>(forged).is_err());
        }
        for target in ["../B0", "B01", "main", "B-1"] {
            let mut invalid = approval.clone();
            invalid["target"] = json!(target);
            assert!(serde_json::from_value::<PublicationApproval>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
    }
    #[test]
    fn recovery_and_publication_require_server_generated_uuid_v7_ids() {
        assert!(validate_human_request_id(&uuid::Uuid::now_v7().to_string()).is_ok());
        for id in [
            "../confirm",
            "00000000-0000-0000-0000-000000000000",
            "018fbd42-9959-4ae7-891b-7adb3eac1111",
        ] {
            assert!(validate_human_request_id(id).is_err());
        }
    }
    #[test]
    fn verification_request_ids_are_canonical_uuid_values() {
        assert!(validate_verification_id(&uuid::Uuid::now_v7().to_string()).is_ok());
        assert!(validate_verification_id("018fbd42-9959-4ae7-891b-7adb3eac1111").is_ok());
        for id in [
            "../request",
            "00000000-0000-0000-0000-000000000000",
            "018FBD42-9959-4AE7-891B-7ADB3EAC1111",
        ] {
            assert!(validate_verification_id(id).is_err());
        }
    }
    #[test]
    fn baseline_ids_accept_only_content_digests() {
        assert!(validate_baseline_id(&format!("d-{}", "a0".repeat(32))).is_ok());
        for id in ["../draft", "d-abc", "d-"] {
            assert!(validate_baseline_id(id).is_err());
        }
        assert!(validate_baseline_id(&format!("d-{}", "A".repeat(64))).is_err());
        assert!(validate_baseline_id(&format!("d-{}", "g".repeat(64))).is_err());
        assert!(validate_baseline_id(&format!("d-{}", "a".repeat(65))).is_err());
    }
    #[test]
    fn bridge_accepts_only_explicit_loopback_endpoint() {
        for (address, port, instance, valid) in [
            ("127.0.0.1", 1234, "instance", true),
            ("localhost", 1234, "instance", false),
            ("example.com", 1234, "instance", false),
            ("127.0.0.1", 0, "instance", false),
            ("127.0.0.1", 1234, "", false),
        ] {
            assert_eq!(
                Endpoint {
                    address: address.into(),
                    port,
                    instance: instance.into()
                }
                .validate()
                .is_ok(),
                valid
            );
        }
    }
}
