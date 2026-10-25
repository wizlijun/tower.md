//! `towermd open ID`: ask the App to open a project and locate one object.
//! A locate request is a navigation hint held in memory only: it writes no
//! event, changes no state, and the first App socket takes it at most once
//! within `TTL`. Losing it (daemon restart, no App) loses nothing but a jump.
use super::*;
use axum::{extract::ws::{Message,WebSocket,WebSocketUpgrade},response::Response};
use std::{sync::atomic::Ordering,time::{Duration,Instant}};
use tokio::time::{interval,timeout,MissedTickBehavior};

/// Long enough for `towermd open` to launch the App and for it to connect.
const TTL:Duration=Duration::from_secs(60);

pub struct Pending {at:Instant,frame:Value}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locate {object:String}

fn valid_object(id:&str)->bool {
    !id.is_empty() && id.len()<=128 && id.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'-'|'_'|'.'))
        && !id.starts_with('.')
}

pub async fn request(State(server):State<Arc<Server>>,headers:HeaderMap,RoutePath(handle):RoutePath<String>,
    Json(input):Json<Locate>)->Result<Json<Value>,ApiError> {
    if !authorized_client(&headers,&server) {return Err(denied());}
    if !valid_object(&input.object) {return Err(bad("对象 ID 无效"));}
    let project=project(&server,&handle).map_err(bad)?;
    let id=project.project_id.clone().ok_or_else(||rejected_as(ErrorCode::NotInitialized)("项目尚未初始化，App 无法定位到对象"))?;
    let frame=json!({"kind":"locate","project":id,"object":input.object});
    server.projects.lock().map_err(|_|bad("项目注册不可用"))?.locate=Some(Pending{at:Instant::now(),frame});
    Ok(Json(json!({"status":"requested","project":id,"object":input.object})))
}

fn take(server:&Server)->Option<Value> {
    let mut registry=server.projects.lock().ok()?;
    registry.locate.take().filter(|pending|pending.at.elapsed()<TTL).map(|pending|pending.frame)
}

/// The App's one navigation socket; it carries locate frames only.
pub async fn subscribe(State(server):State<Arc<Server>>,headers:HeaderMap,ws:WebSocketUpgrade)->Result<Response,ApiError> {
    if !crate::daemon::authorized_app(&headers,&server) {return Err(denied());}
    Ok(ws.on_upgrade(move|socket|follow(socket,server)))
}
async fn follow(mut socket:WebSocket,server:Arc<Server>) {
    let mut timer=interval(Duration::from_millis(250));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            incoming=socket.recv()=>match incoming {
                Some(Ok(Message::Ping(payload)))=>{
                    if !timeout(Duration::from_secs(1),socket.send(Message::Pong(payload))).await.is_ok_and(|result|result.is_ok()){return;}
                }
                Some(Ok(Message::Pong(_)))=>{},
                _=>return,
            },
            _=timer.tick()=>{
                if server.shutting_down.load(Ordering::SeqCst) {
                    let _=timeout(Duration::from_secs(1),socket.send(Message::Close(None))).await;return;
                }
                if let Some(frame)=take(&server) {
                    if !timeout(Duration::from_secs(5),socket.send(Message::Text(frame.to_string().into()))).await
                        .is_ok_and(|result|result.is_ok()) {return;}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_plain_object_ids_are_located() {
        for id in ["T-1","D-12","C-1.2","rule_a"] {assert!(valid_object(id),"{id}");}
        for id in ["","../T-1","T 1",".hidden","a/b",&"x".repeat(129)] {assert!(!valid_object(id),"{id}");}
        assert!(serde_json::from_value::<Locate>(json!({"object":"T-1","path":"/tmp"})).is_err());
    }
}
