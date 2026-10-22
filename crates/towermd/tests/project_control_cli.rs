//! Next control batch: requires pause/resume CLI and project control routes.
#![cfg(unix)]
#[path = "support/envelope.rs"]
mod envelope;
#[path = "support/golden.rs"]
mod golden;
use serde_json::{json,Value};
use std::{fs,path::Path,process::{Command,Output,Stdio},time::Duration};
struct Fixture{root:tempfile::TempDir,runtime:tempfile::TempDir}
fn git(root:&Path,args:&[&str])->String{
    let output=Command::new("git").current_dir(root).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1")
        .args(["-c","user.name=Control QA","-c","user.email=qa@example.invalid","-c","commit.gpgsign=false","-c","core.hooksPath=/dev/null"])
        .args(args).output().unwrap();assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));String::from_utf8(output.stdout).unwrap().trim().into()
}
impl Fixture{
    fn new()->Self{
        let result=Self{root:tempfile::tempdir().unwrap(),runtime:tempfile::tempdir().unwrap()};let root=result.root.path();git(root,&["init","-qb","main"]);
        fs::create_dir(root.join(".towermd")).unwrap();fs::write(root.join(".towermd/config.yaml"),"project_id: control-qa\ndocs_dir: docs/tower\nhub: local\n").unwrap();fs::write(root.join("source.rs"),"fn task() {}\n").unwrap();git(root,&["add","."]);git(root,&["commit","-qm","enable fixture"]);result
    }
    fn output(&self,args:&[&str])->Output{Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN").args(args).current_dir(self.root.path()).env("TOWERMD_RUNTIME_DIR",self.runtime.path()).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1").stdin(Stdio::null()).output().unwrap()}
    fn cli(&self,args:&[&str])->Value{let output=self.output(args);assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));envelope::data_checked(args, &output)}
    fn request(&self,endpoint:&Value,path:&str,client:&str,body:Option<Value>)->(reqwest::StatusCode,Value){
        let http=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(30)).build().unwrap();let url=format!("http://127.0.0.1:{}{path}",endpoint["port"]);let request=if let Some(body)=body{http.post(url).json(&body)}else{http.get(url)};
        let response=request.header("x-towermd-instance",endpoint["instance"].as_str().unwrap()).header("x-towermd-client",client).send().unwrap();let status=response.status();(status,response.json().unwrap())
    }
    fn app(&self,endpoint:&Value,path:&str,body:Option<Value>)->Value{let(status,result)=self.request(endpoint,path,"app",body);assert!(status.is_success(),"{path}: {status} {result}");result}
    fn open(&self)->(Value,String){let endpoint=self.cli(&["daemon","ensure"]);let result=self.app(&endpoint,"/projects/open",Some(json!({"path":self.root.path()})));(endpoint,result["handle"].as_str().unwrap().into())}
    fn original(&self,eid:&str)->Value{
        let root=self.root.path();let paths=git(root,&["ls-tree","-r","--name-only","refs/towermd/snapshot","--","docs/tower/state/events"]);
        for path in paths.lines(){for line in git(root,&["show",&format!("refs/towermd/snapshot:{path}")]).lines(){let event:Value=serde_json::from_str(line).unwrap();if event["eid"]==eid{return event;}}}
        panic!("canonical snapshot missing event {eid}")
    }
}
impl Drop for Fixture{fn drop(&mut self){let _=self.output(&["daemon","stop"]);}}
#[test]
fn cli_pause_and_resume_request_keep_process_identity_and_only_app_can_restore_admission(){
    let fixture=Fixture::new();let root=fixture.root.path();let head=git(root,&["rev-parse","HEAD"]);let index=fs::read(root.join(".git/index")).unwrap();
    let(endpoint,handle)=fixture.open();let pause_id=uuid::Uuid::now_v7().to_string();
    assert!(!fixture.output(&["pause"]).status.success());assert!(!fixture.output(&["resume"]).status.success());
    let paused=fixture.cli(&["pause","all","--request-id",&pause_id]);assert_eq!(paused["control"]["paused"],true);assert_eq!(paused["control"]["pause_event"],pause_id);
    let again=fixture.cli(&["pause","all","--request-id",&pause_id]);assert_eq!(again["event"],paused["event"]);assert_eq!(again["seq"],paused["seq"]);
    let original=fixture.original(&pause_id);assert_eq!(original["type"],"project.paused");assert_eq!(original["actor"],"process:towermd-cli");
    let request_id=uuid::Uuid::now_v7().to_string();let requested=fixture.cli(&["resume","all","--request-id",&request_id]);assert_eq!(requested["control"]["paused"],true);assert_eq!(requested["control"]["resume_requests"][0]["event"],request_id);
    let original=fixture.original(&request_id);assert_eq!(original["type"],"project.resume.requested");assert_eq!(original["actor"],"process:towermd-cli");
    let resume_id=uuid::Uuid::now_v7().to_string();let input=json!({"request_id":resume_id,"pause_event":pause_id,"scope":"all"});let path=format!("/projects/{handle}/resume");
    assert!(!fixture.request(&endpoint,&path,"cli",Some(input.clone())).0.is_success());
    let resumed=fixture.app(&endpoint,&path,Some(input.clone()));assert_eq!(resumed["control"]["paused"],false);assert_eq!(fixture.app(&endpoint,&path,Some(input.clone()))["event"],resumed["event"]);
    assert_eq!(fixture.original(&resume_id)["actor"],"human:local");
    let repeat=fixture.cli(&["resume","all","--request-id",&request_id,"--pause-event",&pause_id]);assert_eq!(repeat["event"],requested["event"]);assert_eq!(repeat["control"]["paused"],false);
    let next_pause=fixture.cli(&["pause","all"]);assert_eq!(next_pause["control"]["paused"],true);
    let stale=json!({"request_id":uuid::Uuid::now_v7().to_string(),"pause_event":pause_id,"scope":"all"});assert!(!fixture.request(&endpoint,&path,"app",Some(stale)).0.is_success());
    assert_eq!(head,git(root,&["rev-parse","HEAD"]));assert_eq!(index,fs::read(root.join(".git/index")).unwrap());
    fixture.cli(&["daemon","stop"]);let(endpoint,handle)=fixture.open();let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);assert_eq!(state["projection"]["ProjectControl"]["pause_event"],next_pause["control"]["pause_event"]);assert_eq!(state["projection"]["tasks_paused"],true);
    golden::record(root,"controls");
}
#[test]
fn snapshot_and_rebuild_requests_are_idempotent(){
    let fixture=Fixture::new();
    let snapshot_id=uuid::Uuid::now_v7().to_string();
    let first=fixture.cli(&["snapshot","--request-id",&snapshot_id]);
    let second=fixture.cli(&["snapshot","--request-id",&snapshot_id]);
    assert_eq!(first["commit"],second["commit"]);assert_eq!(second["request_id"],snapshot_id);
    let rebuild_id=uuid::Uuid::now_v7().to_string();
    let requested=fixture.cli(&["hub","rebuild","--request-id",&rebuild_id]);
    assert_eq!(requested["request_id"],rebuild_id);
    assert_eq!(fixture.cli(&["hub","rebuild","--request-id",&rebuild_id]),requested);
    let files=fs::read_dir(fixture.root.path().join(".git/towermd")).unwrap()
        .filter(|entry|entry.as_ref().unwrap().file_name().to_string_lossy().starts_with("rebuild-")).count();
    assert_eq!(files,1);
}
