//! Native App subscription. Every frame replaces a complete projected prefix;
//! no patch replay, daemon credentials or user-controlled filesystem paths.
use super::*;
use super::business::{now,read_state,state_value};
use axum::{extract::ws::{Message,WebSocket,WebSocketUpgrade},response::Response};
use std::sync::atomic::Ordering;
use tokio::time::{interval,timeout,MissedTickBehavior};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {#[serde(default)] after:Option<u64>}

pub async fn subscribe(State(server):State<Arc<Server>>,headers:HeaderMap,
    RoutePath(handle):RoutePath<String>,Query(cursor):Query<Cursor>,ws:WebSocketUpgrade)->Result<Response,ApiError> {
    if !crate::daemon::authorized_app(&headers,&server) {return Err(denied());}
    let project=project(&server,&handle).map_err(bad)?;
    // The cursor is a reconnect hint, not permission to skip a prefix. Even an
    // ahead/truncated cursor receives a fresh complete first frame.
    let _=cursor.after;
    Ok(ws.on_upgrade(move|socket|stream(socket,server,project)))
}

#[derive(Clone,PartialEq,Eq)]
struct Watermark {seq:u64,snapshot:Option<String>}
fn watermark(state:&Value)->Option<Watermark> {
    (state["status"]=="ready").then(||Watermark{seq:state["seq"].as_u64().unwrap_or(0),
        snapshot:state["snapshot_commit"].as_str().map(str::to_string)})
}
#[derive(Default)]
struct Observer {mark:Option<Watermark>,references:Option<String>}
struct Update {state:Option<Value>,failed:bool}

fn reference_watermark(project:&Project)->Result<String,String> {
    // One bounded local ref read per second. No fetch, history scan, publication
    // proof or snapshot rebuild occurs when these refs have not changed.
    repo::git_timeout(&project.root,&["--no-replace-objects","for-each-ref","--format=%(refname):%(objectname)",
        "refs/heads/main","refs/remotes/origin/main","refs/towermd/snapshot"],Duration::from_secs(2))
}
impl Observer {
    fn poll(&mut self,server:&Server,project:&Project,check_refs:bool)->Result<Update,()> {
        let references=if check_refs {Some(reference_watermark(project).map_err(|_|())?)}else{None};
        let first_probe=self.references.is_none();
        let refs_changed=references.as_ref().is_some_and(|refs|self.references.as_ref().is_some_and(|old|old!=refs));
        if let Some(refs)=references {self.references=Some(refs);}
        let mut slot=project.runtime.lock().map_err(|_|())?;
        if let Some(runtime)=slot.as_mut() {
            runtime.tick(now).map_err(|_|())?;
            // Real Git proof is needed only on connection or an observed ref
            // change. Verification job events already advance the Hub directly.
            let failed=if check_refs&&(first_probe||refs_changed) {
                // A proven blocked main is business state, not a broken
                // transport. Keep its Alert live while the human resolves it.
                matches!(super::finish::maintain_project(server,project,runtime),
                    Err(error) if error!=super::finish::MAIN_INSPECTION_BLOCKED)
            }else{false};
            let mark=Watermark{seq:runtime.contiguous_seq(),snapshot:runtime.snapshot_commit().map(str::to_string)};
            let state=if failed||self.mark.as_ref()!=Some(&mark) {Some(state_value(runtime,project))}else{None};
            self.mark=Some(mark);
            Ok(Update{state,failed})
        }else if refs_changed||self.mark.is_some() {
            drop(slot);
            let state=read_state(server,project).map_err(|_|())?;
            self.mark=watermark(&state);
            Ok(Update{state:Some(state),failed:false})
        }else{Ok(Update{state:None,failed:false})}
    }
}
async fn send(socket:&mut WebSocket,value:Value)->bool {
    timeout(Duration::from_secs(5),socket.send(Message::Text(value.to_string().into()))).await
        .is_ok_and(|result|result.is_ok())
}
async fn failed(socket:&mut WebSocket) {
    // Underlying errors can include local paths, remote credentials or instance
    // details. Diagnostic HTTP status/recovery remains the established route.
    let _=send(socket,json!({"kind":"error","error":"项目状态暂不可用，请重新连接。"})).await;
    let _=timeout(Duration::from_secs(1),socket.send(Message::Close(None))).await;
}
async fn stream(mut socket:WebSocket,server:Arc<Server>,project:Arc<Project>) {
    let first={let server=Arc::clone(&server);let project=Arc::clone(&project);
        tokio::task::spawn_blocking(move||read_state(&server,&project)).await};
    let first=match first {Ok(Ok(state))=>state,_=>{failed(&mut socket).await;return;}};
    let mut observer=Observer{mark:watermark(&first),references:None};
    if !send(&mut socket,json!({"kind":"state","state":first})).await {return;}
    let mut timer=interval(Duration::from_millis(250));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick=0u64;
    loop {
        tokio::select! {
            incoming=socket.recv()=>match incoming {
                Some(Ok(Message::Ping(payload)))=>{
                    if !timeout(Duration::from_secs(1),socket.send(Message::Pong(payload))).await.is_ok_and(|result|result.is_ok()){return;}
                }
                Some(Ok(Message::Pong(_)))=>{},
                Some(Ok(Message::Close(_)))|None|Some(Err(_))=>return,
                Some(Ok(_))=>{failed(&mut socket).await;return;},
            },
            _=timer.tick()=>{
                if server.shutting_down.load(Ordering::SeqCst) {
                    let _=timeout(Duration::from_secs(1),socket.send(Message::Close(None))).await;return;
                }
                let check_refs=tick%4==0;tick=tick.wrapping_add(1);
                let task_server=Arc::clone(&server);let task_project=Arc::clone(&project);
                let result=tokio::task::spawn_blocking(move||{
                    let update=observer.poll(&task_server,&task_project,check_refs);(observer,update)
                }).await;
                let (next,update)=match result {Ok(value)=>value,Err(_)=>{failed(&mut socket).await;return;}};
                observer=next;
                let update=match update {Ok(value)=>value,Err(())=>{failed(&mut socket).await;return;}};
                if let Some(state)=update.state {
                    if !send(&mut socket,json!({"kind":"state","state":state})).await{return;}
                }
                if update.failed {failed(&mut socket).await;return;}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_does_not_accept_credential_or_project_overrides() {
        assert_eq!(serde_json::from_value::<Cursor>(json!({"after":42})).unwrap().after,Some(42));
        for key in ["token","instance","path","project_id"] {
            let mut input=json!({"after":42});input[key]=json!("override");
            assert!(serde_json::from_value::<Cursor>(input).is_err());
        }
    }
    #[test]
    fn prefix_watermark_tracks_ack_and_never_calls_non_ready_state_a_prefix() {
        assert!(watermark(&json!({"status":"needs_recovery"})).is_none());
        assert!(watermark(&json!({"status":"not_activated"})).is_none());
        assert!(watermark(&json!({"status":"ready","seq":42,"snapshot_commit":"a"}))
            !=watermark(&json!({"status":"ready","seq":42,"snapshot_commit":"b"})));
    }
}
