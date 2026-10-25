//! One native WebSocket, replaced atomically when the selected project changes.
//! Only full project states cross IPC; daemon credentials remain in this module.
use std::{path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct StreamMessage {
    subscription: String,
    token: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<Value>,
}
type Deliver = Arc<dyn Fn(StreamMessage) -> Result<(), ()> + Send + Sync>;
type Resolve = Arc<dyn Fn() -> Result<crate::backend::StreamEndpoint, String> + Send + Sync>;

struct Active {
    id: String,
    token: String,
    cancelled: Arc<AtomicBool>,
    task: tauri::async_runtime::JoinHandle<()>,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.task.abort();
    }
}
#[derive(Default)]
pub(crate) struct Streams {
    active: Mutex<Option<Active>>,
    // A replacement cannot connect until the cancelled socket has been dropped.
    connection: Arc<tokio::sync::Mutex<()>>,
}
impl Streams {
    pub fn subscribe(&self, root: PathBuf, token: String, id: String,
        channel: tauri::ipc::Channel<StreamMessage>, backend: crate::BackendState) -> Result<(), String> {
        self.start(token, id, Arc::new(move || crate::backend::stream_endpoint(&root, &backend)),
            Arc::new(move |message| channel.send(message).map_err(|_| ())))
    }
    fn start(&self, token: String, id: String, resolve: Resolve, deliver: Deliver) -> Result<(), String> {
        crate::backend::validate_human_request_id(&id)?;
        let mut active = self.active.lock().map_err(|_| "项目订阅不可用")?;
        if let Some(current) = active.as_ref().filter(|current| current.id == id) {
            if current.token != token { return Err("订阅 ID 已用于其他项目".into()); }
            if !current.task.inner().is_finished() { return Ok(()); }
        }
        active.take();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&cancelled);
        let connection = Arc::clone(&self.connection);
        let subscription = id.clone();
        let selected = token.clone();
        let task = tauri::async_runtime::spawn(async move {
            let _single_connection = connection.lock().await;
            let emit: Deliver = Arc::new(move |mut message| {
                if stopped.load(Ordering::Acquire) { return Err(()); }
                message.subscription = subscription.clone(); message.token = selected.clone();
                deliver(message)
            });
            follow(resolve, emit).await;
        });
        *active = Some(Active { id, token, cancelled, task });
        Ok(())
    }
    pub fn unsubscribe(&self, id: &str) -> Result<(), String> {
        let mut active = self.active.lock().map_err(|_| "项目订阅不可用")?;
        if active.as_ref().is_some_and(|current| current.id == id) { active.take(); }
        Ok(())
    }
    pub fn cancel(&self) {
        if let Ok(mut active) = self.active.lock() { active.take(); }
    }
}

fn message(status: &'static str, state: Option<Value>) -> StreamMessage {
    StreamMessage { subscription: String::new(), token: String::new(), status, state }
}
fn state_frame(text: &str) -> Result<Value, ()> {
    let frame: Value = serde_json::from_str(text).map_err(|_| ())?;
    if frame["kind"] != "state" { return Err(()); }
    let state = frame.get("state").ok_or(())?;
    match state["status"].as_str() {
        Some("ready") if state["seq"].as_u64().is_some() && state["projection"].is_object() => (),
        Some("not_activated" | "needs_recovery") => (),
        _ => return Err(()),
    }
    Ok(state.clone())
}

async fn follow(resolve: Resolve, emit: Deliver) {
    let mut disconnected = false;
    loop {
        let resolver = Arc::clone(&resolve);
        let target = tauri::async_runtime::spawn_blocking(move || resolver()).await;
        if let Ok(Ok(target)) = target {
            // A successful first state resets the disconnection transition.
            if let Err(channel_closed) = connection(target, &emit, &mut disconnected).await {
                if channel_closed { return; }
            }
        }
        if !disconnected {
            if emit(message("disconnected", None)).is_err() { return; }
            disconnected = true;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// Err(true) means the WebView channel is gone; Err(false) requests reconnection.
async fn connection(target: crate::backend::StreamEndpoint, emit: &Deliver,
    disconnected: &mut bool) -> Result<(), bool> {
    let mut request = format!("ws://127.0.0.1:{}/projects/{}/events", target.port, target.handle)
        .into_client_request().map_err(|_| false)?;
    request.headers_mut().insert("x-towermd-instance", target.instance.parse().map_err(|_| false)?);
    request.headers_mut().insert("x-towermd-client", "app".parse().map_err(|_| false)?);
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10),
        tokio_tungstenite::connect_async(request)).await.map_err(|_| false)?.map_err(|_| false)?;
    let mut first = true;
    let mut pending = None;
    let mut window = tokio::time::interval_at(Instant::now() + Duration::from_secs(1), Duration::from_secs(1));
    window.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = window.tick() => {
                if let Some(state) = pending.take() {
                    emit(message("connected", Some(state))).map_err(|_| true)?;
                }
            }
            frame = socket.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    let Ok(state) = state_frame(text.as_str()) else { break; };
                    if first {
                        emit(message("connected", Some(state))).map_err(|_| true)?;
                        *disconnected = false; first = false;
                    } else { pending = Some(state); }
                }
                Some(Ok(Message::Ping(bytes))) => if socket.send(Message::Pong(bytes)).await.is_err() { break; },
                Some(Ok(Message::Pong(_))) => (),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => break,
            }
        }
    }
    // A final proved alert may arrive just before a server error/close. Keep
    // that complete state even when its normal display window has not elapsed.
    if let Some(state) = pending.take() { emit(message("connected", Some(state))).map_err(|_| true)?; }
    Err(false)
}

/// `towermd open ID`: one locate request from the daemon's navigation socket.
/// Only a project ID and an object ID cross IPC.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Locate { project: String, object: String }

pub(crate) fn locate_frame(text: &str) -> Option<Locate> {
    let frame: Value = serde_json::from_str(text).ok()?;
    if frame["kind"] != "locate" { return None; }
    let id = |key: &str| frame[key].as_str()
        .filter(|id| !id.is_empty() && id.len() <= 128 && !id.starts_with('.')
            && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        .map(str::to_owned);
    Some(Locate { project: id("project")?, object: id("object")? })
}

/// The App's one navigation socket, replaced when the WebView follows again.
#[derive(Default)]
pub(crate) struct Navigation { task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>> }
impl Navigation {
    pub fn follow(&self, channel: tauri::ipc::Channel<Locate>, backend: crate::BackendState) -> Result<(), String> {
        let mut task = self.task.lock().map_err(|_| "导航订阅不可用")?;
        if let Some(old) = task.take() { old.abort(); }
        *task = Some(tauri::async_runtime::spawn(async move {
            loop {
                if let Some(target) = crate::backend::navigation_endpoint(&backend) {
                    if navigate(target, &channel).await.is_err() { return; }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }));
        Ok(())
    }
}
// Err means the WebView channel is gone; Ok asks for a reconnection.
async fn navigate((port, instance): (u16, String), channel: &tauri::ipc::Channel<Locate>) -> Result<(), ()> {
    let Ok(mut request) = format!("ws://127.0.0.1:{port}/app/locate").into_client_request() else { return Ok(()) };
    let (Ok(instance), Ok(client)) = (instance.parse(), "app".parse()) else { return Ok(()) };
    request.headers_mut().insert("x-towermd-instance", instance);
    request.headers_mut().insert("x-towermd-client", client);
    let Ok(Ok((mut socket, _))) = tokio::time::timeout(Duration::from_secs(10),
        tokio_tungstenite::connect_async(request)).await else { return Ok(()) };
    while let Some(frame) = socket.next().await {
        match frame {
            Ok(Message::Text(text)) => if let Some(locate) = locate_frame(text.as_str()) { channel.send(locate).map_err(|_| ())?; },
            Ok(Message::Ping(bytes)) => if socket.send(Message::Pong(bytes)).await.is_err() { break; },
            Ok(Message::Pong(_)) => (),
            _ => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
