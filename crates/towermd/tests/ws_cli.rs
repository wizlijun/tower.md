//! Real local daemon transport: complete prefixes, App identity and live Git observations.
#![cfg(unix)]
#[path = "support/envelope.rs"]
mod envelope;
use futures_util::{SinkExt,StreamExt};
use serde_json::{json,Value};
use std::{fs,path::Path,process::{Command,Stdio},time::Duration};
use tokio_tungstenite::{connect_async,tungstenite::{client::IntoClientRequest,Message}};
type Socket=tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Fixture{root:tempfile::TempDir,runtime:tempfile::TempDir}
fn git(root:&Path,args:&[&str])->String{
    let output=Command::new("git").current_dir(root).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1")
        .args(["-c","user.name=WS QA","-c","user.email=qa@example.invalid","-c","commit.gpgsign=false","-c","core.hooksPath=/dev/null"])
        .args(args).output().unwrap();
    assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().into()
}
impl Fixture{
    fn new()->Self{
        let result=Self{root:tempfile::tempdir().unwrap(),runtime:tempfile::tempdir().unwrap()};
        let root=result.root.path();git(root,&["init","-qb","main"]);
        fs::create_dir(root.join(".towermd")).unwrap();
        fs::write(root.join(".towermd/config.yaml"),"project_id: ws-qa\ndocs_dir: docs/tower\nhub: local\n").unwrap();
        fs::write(root.join("source.rs"),"fn task() {}\n").unwrap();
        git(root,&["add","."]);git(root,&["commit","-qm","enable fixture"]);result
    }
    fn cli(&self,args:&[&str])->Value{
        let output=Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN").args(args).current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR",self.runtime.path()).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1")
            .stdin(Stdio::null()).output().unwrap();
        assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));
        envelope::data_checked(args, &output)
    }
    fn app(&self,endpoint:&Value,path:&str,body:Option<Value>)->Value{
        let http=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(30)).build().unwrap();
        let url=format!("http://127.0.0.1:{}{path}",endpoint["port"]);
        let request=if let Some(body)=body{http.post(url).json(&body)}else{http.get(url)};
        let response=request.header("x-towermd-instance",endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client","app").send().unwrap();
        let status=response.status();let result:Value=response.json().unwrap();assert!(status.is_success(),"{path}: {status} {result}");result
    }
    fn open(&self)->(Value,String){
        let endpoint=self.cli(&["daemon","ensure"]);
        let result=self.app(&endpoint,"/projects/open",Some(json!({"path":self.root.path()})));
        (endpoint,result["handle"].as_str().unwrap().into())
    }
}
impl Drop for Fixture{fn drop(&mut self){
    let _=Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN").args(["daemon","stop"]).current_dir(self.root.path())
        .env("TOWERMD_RUNTIME_DIR",self.runtime.path()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}}
fn request(endpoint:&Value,handle:&str,client:&str,instance:&str,after:Option<u64>)->tokio_tungstenite::tungstenite::http::Request<()>{
    let mut url=format!("ws://127.0.0.1:{}/projects/{handle}/events",endpoint["port"]);
    if let Some(after)=after{url.push_str(&format!("?after={after}"));}
    let mut request=url.into_client_request().unwrap();
    request.headers_mut().insert("x-towermd-instance",instance.parse().unwrap());
    request.headers_mut().insert("x-towermd-client",client.parse().unwrap());request
}
async fn connect(endpoint:&Value,handle:&str,after:Option<u64>)->Socket{
    connect_async(request(endpoint,handle,"app",endpoint["instance"].as_str().unwrap(),after)).await.unwrap().0
}
async fn frame(socket:&mut Socket)->Value{
    tokio::time::timeout(Duration::from_secs(12),async{
        loop{match socket.next().await.expect("stream closed").expect("valid frame"){
            Message::Text(text)=>return serde_json::from_str(&text).unwrap(),
            Message::Ping(data)=>socket.send(Message::Pong(data)).await.unwrap(),
            other=>panic!("unexpected frame: {other:?}"),
        }}
    }).await.expect("state notification before deadline")
}
async fn state_matching(socket:&mut Socket,predicate:impl Fn(&Value)->bool)->Value{
    tokio::time::timeout(Duration::from_secs(15),async{
        loop{let value=frame(socket).await;assert_eq!(value["kind"],"state","{value}");
            if predicate(&value["state"]){return value["state"].clone();}}
    }).await.expect("matching complete state")
}

#[test]
fn app_only_subscription_pushes_changes_and_reconnects_with_complete_prefix(){
    let fixture=Fixture::new();let(endpoint,handle)=fixture.open();
    let executor=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    for(client,instance)in[("cli",endpoint["instance"].as_str().unwrap()),("app","wrong-instance")]{
        executor.block_on(async{
            match connect_async(request(&endpoint,&handle,client,instance,None)).await{
                Err(tokio_tungstenite::tungstenite::Error::Http(response))=>assert_eq!(response.status().as_u16(),409),
                _=>panic!("unauthorized client upgraded"),
            }
        });
    }
    let mut socket=executor.block_on(connect(&endpoint,&handle,None));
    let first=executor.block_on(frame(&mut socket));
    assert_eq!(first,json!({"kind":"state","state":{"status":"not_activated"}}));
    assert!(!fixture.root.path().join(".git/towermd/hub").exists());
    let pause_id=uuid::Uuid::now_v7().to_string();
    fixture.app(&endpoint,&format!("/projects/{handle}/pause"),Some(json!({"request_id":pause_id,"scope":"all"})));
    let paused=executor.block_on(state_matching(&mut socket,|state|state["projection"]["ProjectControl"]["pause_event"]==pause_id));
    assert_eq!(paused["projection"]["ProjectControl"]["paused"],true);
    assert!(paused["seq"].as_u64().unwrap()>0);
    executor.block_on(socket.close(None)).unwrap();
    for after in [paused["seq"].as_u64().unwrap(),u64::MAX]{
        let mut reconnected=executor.block_on(connect(&endpoint,&handle,Some(after)));
        let full=executor.block_on(frame(&mut reconnected));
        assert_eq!(full["kind"],"state");assert_eq!(full["state"]["status"],"ready");
        assert_eq!(full["state"]["projection"]["ProjectControl"]["pause_event"],pause_id);
        assert!(full["state"]["projection"]["Task"].is_object());
        // The derived delegation read model rides along; it is never stored.
        assert!(full["state"]["delegation_view"]["objects"].is_array(),"{full}");
        executor.block_on(reconnected.close(None)).unwrap();
    }
    let mut resumed_socket=executor.block_on(connect(&endpoint,&handle,None));
    executor.block_on(frame(&mut resumed_socket));
    fixture.app(&endpoint,&format!("/projects/{handle}/resume"),Some(json!({"request_id":uuid::Uuid::now_v7().to_string(),"pause_event":pause_id,"scope":"all"})));
    let resumed=executor.block_on(state_matching(&mut resumed_socket,|state|state["projection"]["ProjectControl"]["paused"]==false));
    assert!(resumed["seq"].as_u64().unwrap()>paused["seq"].as_u64().unwrap());
    // Wait for one quiet window, then prove the stream does not emit duplicate
    // projections on its timer. Initial real inspection may produce one state.
    executor.block_on(async{
        let deadline=tokio::time::Instant::now()+Duration::from_secs(8);
        loop{
            match tokio::time::timeout(Duration::from_millis(1300),resumed_socket.next()).await{
                Err(_)=>break,
                Ok(Some(Ok(Message::Text(text))))=>assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["kind"],"state"),
                other=>panic!("unexpected idle frame {other:?}"),
            }
            assert!(tokio::time::Instant::now()<deadline,"stream never settled");
        }
        assert!(tokio::time::timeout(Duration::from_millis(1300),resumed_socket.next()).await.is_err());
        resumed_socket.close(None).await.unwrap();
    });
    assert_eq!(fixture.app(&endpoint,&format!("/projects/{handle}/state"),None)["projection"]["ProjectControl"]["paused"],false);
}

#[test]
fn external_protected_main_change_pushes_canonical_alert_without_dropping_subscription(){
    let fixture=Fixture::new();let(endpoint,handle)=fixture.open();
    fixture.app(&endpoint,&format!("/projects/{handle}/pause"),Some(json!({"request_id":uuid::Uuid::now_v7().to_string(),"scope":"all"})));
    let executor=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let mut socket=executor.block_on(connect(&endpoint,&handle,None));executor.block_on(frame(&mut socket));
    fs::write(fixture.root.path().join(".towermd/config.yaml"),"project_id: ws-qa\ndocs_dir: docs/tower\nhub: local\n# unauthorized protected change\n").unwrap();
    git(fixture.root.path(),&["add",".towermd/config.yaml"]);git(fixture.root.path(),&["commit","-qm","unapproved config edit"]);
    let commit=git(fixture.root.path(),&["rev-parse","HEAD"]);
    let blocked=executor.block_on(state_matching(&mut socket,|state|state["projection"]["MainInspection"]["main"]==commit
        && state["projection"]["MainInspection"]["passed"]==false));
    assert!(blocked["projection"]["Alert"].as_object().unwrap().values()
        .any(|alert|alert["data"]["reason"]=="unapproved_protected_change"&&alert["resolved"]!=true));
    // A business prohibition is still a live state stream, not a reconnect loop.
    executor.block_on(async{
        socket.send(Message::Ping(vec![1,2,3].into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(4),async{
            loop{match socket.next().await.unwrap().unwrap(){
                Message::Pong(data)=>{assert_eq!(data.as_ref(),&[1,2,3]);break;},
                Message::Text(text)=>assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["kind"],"state"),
                other=>panic!("blocked main closed live subscription: {other:?}"),
            }}
        }).await.unwrap();socket.close(None).await.unwrap();
    });
}

#[test]
fn real_daemon_restart_with_active_subscription_preserves_durable_state(){
    let fixture=Fixture::new();let(endpoint,handle)=fixture.open();
    let executor=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let mut socket=executor.block_on(connect(&endpoint,&handle,None));
    executor.block_on(frame(&mut socket));
    let pause_id=uuid::Uuid::now_v7().to_string();
    let receipt=fixture.cli(&["pause","all","--request-id",&pause_id]);
    let paused=executor.block_on(state_matching(&mut socket,|state|
        state["projection"]["ProjectControl"]["pause_event"]==pause_id));
    assert_eq!(paused["projection"]["ProjectControl"]["paused"],true);
    assert!(paused["seq"].as_u64().unwrap()>=receipt["seq"].as_u64().unwrap());

    // Keep the client socket open and deliberately do not read its close frame
    // until stop has completed. Graceful shutdown must not wait on the App.
    let mut stop=Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN").args(["daemon","stop"])
        .current_dir(fixture.root.path()).env("TOWERMD_RUNTIME_DIR",fixture.runtime.path())
        .env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1")
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
    let deadline=std::time::Instant::now()+Duration::from_secs(10);
    loop{
        if let Some(status)=stop.try_wait().unwrap(){assert!(status.success(),"stop failed with active subscription");break;}
        if std::time::Instant::now()>=deadline{
            let _=stop.kill();let _=stop.wait();
            panic!("active subscription prevented daemon stop within 10 seconds");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(fixture.cli(&["daemon","status"])["running"],false);
    executor.block_on(async{
        tokio::time::timeout(Duration::from_secs(3),async{
            loop{match socket.next().await{
                None|Some(Err(_))|Some(Ok(Message::Close(_)))=>break,
                Some(Ok(Message::Text(text)))=>assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["kind"],"state"),
                other=>panic!("unexpected shutdown frame {other:?}"),
            }}
        }).await.expect("old subscription must terminate after daemon stop");
    });

    let(restarted,new_handle)=fixture.open();
    assert_ne!(restarted["instance"],endpoint["instance"]);
    executor.block_on(async{
        match connect_async(request(&restarted,&new_handle,"app",endpoint["instance"].as_str().unwrap(),None)).await{
            Err(tokio_tungstenite::tungstenite::Error::Http(response))=>assert_eq!(response.status().as_u16(),409),
            _=>panic!("old daemon credentials survived restart"),
        }
    });
    let mut renewed=executor.block_on(connect(&restarted,&new_handle,paused["seq"].as_u64()));
    let first=executor.block_on(frame(&mut renewed));
    assert_eq!(first["kind"],"state");assert_eq!(first["state"]["status"],"ready");
    assert!(first["state"]["seq"].as_u64().unwrap()>=paused["seq"].as_u64().unwrap());
    assert_eq!(first["state"]["projection"]["ProjectControl"]["pause_event"],pause_id);
    assert_eq!(first["state"]["projection"]["ProjectControl"]["paused"],true);
    assert!(first["state"]["projection"]["Task"].is_object());
    fixture.app(&restarted,&format!("/projects/{new_handle}/resume"),
        Some(json!({"request_id":uuid::Uuid::now_v7().to_string(),"pause_event":pause_id,"scope":"all"})));
    let resumed=executor.block_on(state_matching(&mut renewed,|state|
        state["projection"]["ProjectControl"]["paused"]==false));
    assert!(resumed["seq"].as_u64().unwrap()>first["state"]["seq"].as_u64().unwrap());
    executor.block_on(renewed.close(None)).unwrap();
}
