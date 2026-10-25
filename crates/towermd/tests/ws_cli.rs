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

/// A commit on main made outside the daemon, through a throwaway worktree.
fn commit_on_main(root:&Path,author:&str,message:&str,config:Option<&str>)->String{
    let scratch=tempfile::tempdir().unwrap();let path=scratch.path().join("main");let path_text=path.to_str().unwrap();
    git(root,&["worktree","add","-q",path_text,"main"]);
    if let Some(config)=config{fs::write(path.join(".towermd/config.yaml"),config).unwrap();git(&path,&["add","."]);}
    git(&path,&["commit","-q","--allow-empty","--author",author,"-m",message]);
    let commit=git(&path,&["rev-parse","HEAD"]);
    git(root,&["worktree","remove","--force",path_text]);commit
}

#[test]
fn own_snapshot_skips_reinspection_but_look_alike_and_protected_commits_do_not(){
    let fixture=Fixture::new();let root=fixture.root.path();
    // With main not checked out, the daemon writes its snapshots to main itself.
    git(root,&["checkout","-qb","owner"]);
    let(endpoint,handle)=fixture.open();
    // Any App action opens the project runtime the observer inspects through.
    let pause_id=uuid::Uuid::now_v7().to_string();
    fixture.app(&endpoint,&format!("/projects/{handle}/pause"),Some(json!({"request_id":pause_id,"scope":"all"})));
    let executor=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let mut socket=executor.block_on(connect(&endpoint,&handle,None));
    let inspected=|state:&Value,main:&str|state["projection"]["MainInspection"]["main"]==main;
    let snapshot=||fixture.app(&endpoint,&format!("/projects/{handle}/snapshot"),Some(json!({})))["commit"].as_str().unwrap().to_owned();

    // Nothing was inspected yet, so the first snapshot's main is inspected in full.
    let first=snapshot();
    assert_eq!(git(root,&["rev-parse","main"]),first,"the snapshot lands on main");
    executor.block_on(state_matching(&mut socket,|state|inspected(state,&first)
        && state["projection"]["MainInspection"]["passed"]==true));
    // A business event makes the next snapshot a new own commit on top.
    fixture.app(&endpoint,&format!("/projects/{handle}/resume"),
        Some(json!({"request_id":uuid::Uuid::now_v7().to_string(),"pause_event":pause_id,"scope":"all"})));
    let own=snapshot();
    assert_eq!(git(root,&["rev-parse","main^"]),first);
    assert_eq!(git(root,&["rev-parse","main"]),own);
    // Several observer rounds later the passed inspection still names the
    // earlier main: the daemon's own proven snapshot is not inspected again.
    std::thread::sleep(Duration::from_secs(3));
    let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);
    assert!(inspected(&state,&first),"{}",state["projection"]["MainInspection"]);
    assert_eq!(state["projection"]["MainInspection"]["passed"],true);

    // Same author and trailers as a snapshot, but not this daemon's write.
    let look_alike=commit_on_main(root,"process:towermd-snapshot <snapshot@towermd.invalid>",
        "towermd snapshot\n\nTowermd-Epoch: 1\nTowermd-Snapshot: 1",None);
    executor.block_on(state_matching(&mut socket,|state|inspected(state,&look_alike)));

    // A person's protected change on top is still stopped.
    let protected=commit_on_main(root,"WS QA <qa@example.invalid>","unapproved config edit",
        Some("project_id: ws-qa\ndocs_dir: docs/tower\nhub: local\n# unauthorized protected change\n"));
    let blocked=executor.block_on(state_matching(&mut socket,|state|inspected(state,&protected)
        && state["projection"]["MainInspection"]["passed"]==false));
    assert!(blocked["projection"]["Alert"].as_object().unwrap().values()
        .any(|alert|alert["data"]["reason"]=="unapproved_protected_change"&&alert["resolved"]!=true));
    executor.block_on(socket.close(None)).unwrap();
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

#[test]
fn a_locate_request_reaches_the_app_navigation_socket_once_and_writes_nothing(){
    let fixture=Fixture::new();let(endpoint,handle)=fixture.open();
    let instance=endpoint["instance"].as_str().unwrap().to_owned();
    let post=|client:&str,body:Value|{
        let http=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(30)).build().unwrap();
        let response=http.post(format!("http://127.0.0.1:{}/projects/{handle}/locate",endpoint["port"])).json(&body)
            .header("x-towermd-instance",&instance).header("x-towermd-client",client).send().unwrap();
        let status=response.status().as_u16();(status,response.json::<Value>().unwrap_or(Value::Null))
    };
    let navigation=|client:&str|{
        let mut request=format!("ws://127.0.0.1:{}/app/locate",endpoint["port"]).into_client_request().unwrap();
        request.headers_mut().insert("x-towermd-instance",instance.parse().unwrap());
        request.headers_mut().insert("x-towermd-client",client.parse().unwrap());request
    };
    let executor=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    executor.block_on(async{
        match connect_async(navigation("cli")).await{
            Err(tokio_tungstenite::tungstenite::Error::Http(response))=>assert_eq!(response.status().as_u16(),409),
            _=>panic!("the command line opened the App's navigation socket"),
        }
    });
    for body in [json!({"object":"../T-1"}),json!({"object":"T-1","path":"/tmp"})]{
        let (status,_)=post("cli",body.clone());assert!(status>=400,"{body}");
    }
    let before=git(fixture.root.path(),&["status","--porcelain"]);
    // The request may come before the App connects; the App takes it once.
    let (status,requested)=post("cli",json!({"object":"T-1"}));
    assert_eq!(status,200,"{requested}");
    assert_eq!(requested,json!({"status":"requested","project":"ws-qa","object":"T-1"}));
    let mut socket=executor.block_on(async{connect_async(navigation("app")).await.unwrap().0});
    let located=executor.block_on(frame(&mut socket));
    assert_eq!(located,json!({"kind":"locate","project":"ws-qa","object":"T-1"}));
    executor.block_on(async{
        assert!(tokio::time::timeout(Duration::from_millis(800),socket.next()).await.is_err(),"a locate request is delivered once");
    });
    let (_,again)=post("cli",json!({"object":"D-2"}));assert_eq!(again["object"],"D-2");
    assert_eq!(executor.block_on(frame(&mut socket))["object"],"D-2");
    assert_eq!(git(fixture.root.path(),&["status","--porcelain"]),before);
    assert!(!fixture.root.path().join(".git/towermd/hub").exists(),"a navigation hint writes no event");
    executor.block_on(socket.close(None)).unwrap();
}

#[test]
fn the_app_initializes_an_existing_git_repository_and_never_creates_one(){
    let fixture=Fixture::new();let(endpoint,_)=fixture.open();
    let post=|client:&str,body:Value|{
        let http=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(30)).build().unwrap();
        let response=http.post(format!("http://127.0.0.1:{}/projects/init",endpoint["port"])).json(&body)
            .header("x-towermd-instance",endpoint["instance"].as_str().unwrap()).header("x-towermd-client",client).send().unwrap();
        let status=response.status().as_u16();(status,response.json::<Value>().unwrap_or(Value::Null))
    };
    let plain=tempfile::tempdir().unwrap();
    let (status,refused)=post("app",json!({"path":plain.path(),"docs_dir":"docs/tower"}));
    assert_eq!((status,refused["code"].as_str()),(400,Some("not_found")),"{refused}");assert!(refused["error"].as_str().unwrap().contains("git init"),"{refused}");
    assert_eq!(fs::read_dir(plain.path()).unwrap().count(),0,"a folder that is not a repository is left untouched");
    let repository=tempfile::tempdir().unwrap();git(repository.path(),&["init","-qb","main"]);
    let (status,_)=post("cli",json!({"path":repository.path(),"docs_dir":"docs/tower"}));
    assert_eq!(status,409,"the command line keeps its own towermd init");
    assert!(!repository.path().join(".towermd").exists());
    let (status,_)=post("app",json!({"path":repository.path(),"docs_dir":"docs/tower","project_id":"x"}));
    assert!(status>=400);
    let before=git(repository.path(),&["rev-list","--all","--count"]);
    let (status,initialized)=post("app",json!({"path":repository.path(),"docs_dir":"notes/tower"}));
    assert_eq!(status,200,"{initialized}");
    let config=fs::read_to_string(repository.path().join(".towermd/config.yaml")).unwrap();
    assert!(config.contains("docs_dir: notes/tower"),"{config}");
    assert_eq!(initialized["report"]["docs_dir"],"notes/tower");
    assert!(config.contains(initialized["report"]["project_id"].as_str().unwrap()));
    assert!(initialized["handle"].as_str().is_some_and(|handle|!handle.is_empty()));
    assert_eq!(git(repository.path(),&["rev-list","--all","--count"]),before,"init writes files and commits nothing");
}
