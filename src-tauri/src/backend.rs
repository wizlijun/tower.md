//! Native-only bridge. The WebView never receives a daemon address or chooses
//! an executable; only the packaged CLI may start the per-user daemon.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::Method;
use serde::Deserialize;
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
    Scan,
    Poll(String),
    Cancel(String),
}

impl Backend {
    pub fn request(&mut self, root: &Path, operation: Operation) -> Result<Value, String> {
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
            let (method, path) = match operation {
                Operation::View => (Method::GET, format!("/projects/{handle}/view")),
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
            };
            send(&client, &endpoint, method, &path, None)
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
        return Err(value["error"].as_str().unwrap_or("本机服务请求失败").into());
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
