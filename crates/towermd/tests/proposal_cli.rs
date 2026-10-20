#![cfg(unix)]
use serde_json::{json,Value};
use std::{fs,path::Path,process::{Command,Output,Stdio},time::{Duration,Instant}};
struct Fixture{root:tempfile::TempDir,runtime:tempfile::TempDir}
fn git(root:&Path,args:&[&str])->String{
    let output=Command::new("git").current_dir(root).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1")
        .args(["-c","user.name=Proposal QA","-c","user.email=qa@example.invalid","-c","commit.gpgsign=false","-c","core.hooksPath=/dev/null"])
        .args(args).output().unwrap();
    assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));String::from_utf8(output.stdout).unwrap().trim().into()
}
impl Fixture{
    fn new()->Self{
        let fixture=Self{root:tempfile::tempdir().unwrap(),runtime:tempfile::tempdir().unwrap()};let root=fixture.root.path();git(root,&["init","-qb","main"]);
        fs::create_dir(root.join(".towermd")).unwrap();fs::write(root.join(".towermd/config.yaml"),"project_id: proposal-qa\ndocs_dir: docs/tower\nhub: local\n").unwrap();
        for name in ["alpha","beta","gamma"]{fs::create_dir(root.join(name)).unwrap();fs::write(root.join(name).join("code.rs"),"fn example() {}\n").unwrap();}
        fs::write(root.join("Makefile"),"test:\n\t@printf 'real proposal smoke\\n'\n").unwrap();git(root,&["add","."]);git(root,&["commit","-qm","enable fixture"]);fixture
    }
    fn output(&self,cwd:&Path,args:&[&str])->Output{Command::new(env!("CARGO_BIN_EXE_towermd")).args(args).current_dir(cwd).env("TOWERMD_RUNTIME_DIR",self.runtime.path()).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1").stdin(Stdio::null()).output().unwrap()}
    fn run(&self,cwd:&Path,args:&[&str])->Value{let output=self.output(cwd,args);assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));serde_json::from_slice(&output.stdout).unwrap()}
    fn request(&self,endpoint:&Value,path:&str,client:&str,body:Option<Value>)->(reqwest::StatusCode,Value){
        let http=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(60)).build().unwrap();let url=format!("http://127.0.0.1:{}{path}",endpoint["port"]);
        let request=if let Some(body)=body{http.post(url).json(&body)}else{http.get(url)};
        let response=request.header("x-towermd-instance",endpoint["instance"].as_str().unwrap()).header("x-towermd-client",client).send().unwrap();let status=response.status();(status,response.json().unwrap())
    }
    fn app(&self,endpoint:&Value,path:&str,body:Option<Value>)->Value{let(status,result)=self.request(endpoint,path,"app",body);assert!(status.is_success(),"{path}: {status} {result}");result}
    fn open(&self)->(Value,String){let endpoint=self.run(self.root.path(),&["daemon","ensure"]);let opened=self.app(&endpoint,"/projects/open",Some(json!({"path":self.root.path()})));(endpoint,opened["handle"].as_str().unwrap().into())}
    fn synthesized(&self)->Value{
        let root=self.root.path();let scan=self.run(root,&["baseline","scan"]);let id=scan["id"].as_str().unwrap();self.run(root,&["baseline","scan","--wait",id]);
        let template=self.run(root,&["baseline","template",id]);let path=self.runtime.path().join("submission.json");fs::write(&path,serde_json::to_vec(&template).unwrap()).unwrap();
        self.run(root,&["baseline","submit",path.to_str().unwrap(),"--producer","qa/1"])
    }
    fn verified(&self,endpoint:&Value,handle:&str,mut draft:Value)->Value{
        let modules=draft["review"]["modules"].as_array().cloned().unwrap_or_else(||draft["scan"]["modules"].as_array().unwrap().clone());
        for module in modules{draft=self.app(endpoint,&format!("/projects/{handle}/baselines/{}/review",draft["id"].as_str().unwrap()),Some(json!({"action":{"kind":"commands","module_id":module["id"],"commands":[{"directory":"","command":"printf 'real proposal reviewed proof\\n'"}]}})));}
        let request=uuid::Uuid::now_v7().to_string();self.app(endpoint,&format!("/projects/{handle}/baselines/{}/verify",draft["id"].as_str().unwrap()),Some(json!({"request_id":request})));
        let deadline=Instant::now()+Duration::from_secs(30);
        loop{let job=self.app(endpoint,&format!("/projects/{handle}/baseline-verifications/{request}"),None);if job["status"]!="running"{assert_eq!(job["status"],"ready","{job}");return self.app(endpoint,&format!("/projects/{handle}/baselines/{}",job["result_id"].as_str().unwrap()),None);}assert!(Instant::now()<deadline,"{job}");std::thread::sleep(Duration::from_millis(50));}
    }
    fn approve(&self,endpoint:&Value,path:&str)->(Value,Value){let preview=self.app(endpoint,&format!("{path}/prepare"),Some(json!({})));let input=json!({"request_id":preview["request_id"],"target":preview["target"],"prepared_at":preview["prepared_at"],"content_hash":preview["content_hash"]});let result=self.app(endpoint,&format!("{path}/approve"),Some(input.clone()));(result,input)}
    fn baseline(&self)->(Value,String,Value){let(endpoint,handle)=self.open();self.app(&endpoint,&format!("/projects/{handle}/snapshot"),Some(json!({})));let draft=self.verified(&endpoint,&handle,self.synthesized());git(self.root.path(),&["checkout","-qb","qa-owner"]);let(result,_)=self.approve(&endpoint,&format!("/projects/{handle}/baselines/{}/publication",draft["id"].as_str().unwrap()));assert_eq!(result["target"],"B0");(endpoint,handle,draft)}
    fn propose(&self,draft:&Value,request:&str)->(Value,std::path::PathBuf){
        let path=self.runtime.path().join(format!("proposal-{request}.json"));fs::write(&path,serde_json::to_vec(&json!({"draft_id":draft["id"],"kind":"merge","reason":"两个模块共同承担同一调用路径","alternatives":["保留两个模块","合并职责"],"recommendation":"按审阅后的模块合并"})).unwrap()).unwrap();
        (self.run(self.root.path(),&["propose",path.to_str().unwrap(),"--request-id",request]),path)
    }
    fn task(&self,id:&str,module:&str,path:&str){let file=self.runtime.path().join(format!("{id}.json"));fs::write(&file,serde_json::to_vec(&json!({"id":id,"title":id,"module":module,"allowed_paths":[path],"acceptance":["verification passes"]})).unwrap()).unwrap();self.run(self.root.path(),&["task","add",file.to_str().unwrap()]);}
}
impl Drop for Fixture{fn drop(&mut self){let _=self.output(self.root.path(),&["daemon","stop"]);}}

#[test]
fn real_reviewed_proposal_publishes_deletion_decision_and_tag_then_replays_after_restart_and_recovery(){
    let fixture=Fixture::new();let(endpoint,handle,b0)=fixture.baseline();let root=fixture.root.path();
    let old_modules=b0["review"]["modules"].as_array().unwrap();let a=old_modules[0]["id"].as_str().unwrap();let b=old_modules[1]["id"].as_str().unwrap();
    fixture.task("T-removed",b,old_modules[1]["files"][0].as_str().unwrap());
    let trees=tempfile::tempdir().unwrap();let bound_tree=trees.path().join("removed-module-task");
    git(root,&["worktree","add","-b","t/T-removed",bound_tree.to_str().unwrap(),"main"]);fixture.run(&bound_tree,&["task","claim","T-removed"]);
    let draft=fixture.synthesized();let modules=draft["scan"]["modules"].as_array().unwrap();assert!(modules.iter().any(|module|module["id"]==a));assert!(modules.iter().any(|module|module["id"]==b));
    let merged=fixture.app(&endpoint,&format!("/projects/{handle}/baselines/{}/review",draft["id"].as_str().unwrap()),Some(json!({"action":{"kind":"merge","module_ids":[a,b],"target_id":a,"title":"合并模块"}})));
    let reviewed=fixture.verified(&endpoint,&handle,merged);let request=uuid::Uuid::now_v7().to_string();let(proposal,input_path)=fixture.propose(&reviewed,&request);let id=proposal["id"].as_str().unwrap();
    assert_eq!(proposal["status"],"submitted");assert!(proposal["proposal"]["impact"]["modules"].as_array().unwrap().contains(&json!(b)));
    let deleted=proposal["proposal"]["changes"].as_array().unwrap().iter().find(|change|change["after_oid"].is_null()&&change["before"].as_str().is_some_and(|text|text.contains(&format!("id: {b}")))).unwrap()["path"].as_str().unwrap().to_string();
    let path=format!("/projects/{handle}/proposals/{id}/publication");
    let (status,_)=fixture.request(&endpoint,&format!("{path}/prepare"),"cli",Some(json!({})));assert!(!status.is_success());
    let branch=git(root,&["rev-parse","HEAD"]);let index=fs::read(root.join(".git/index")).unwrap();
    let(published,approval)=fixture.approve(&endpoint,&path);assert_eq!(published["target"],"B1");let commit=published["commit"].as_str().unwrap();
    assert_eq!(git(root,&["rev-parse","towermd/B1"]),commit);assert!(git(root,&["ls-tree",commit,"--",&deleted]).is_empty());
    let decision_path=format!("docs/tower/decisions/D-{id}.md");let decision=git(root,&["show",&format!("{commit}:{decision_path}")]);assert!(decision.contains("proposal_ref:"));assert!(decision.contains(id));
    let manifest=git(root,&["show",&format!("{commit}:docs/tower/baselines/B1.md")]);assert!(manifest.contains(&decision_path));assert!(!manifest.contains(&deleted));
    assert_eq!(git(root,&["rev-parse","HEAD"]),branch);assert_eq!(index,fs::read(root.join(".git/index")).unwrap());
    let same=fixture.app(&endpoint,&format!("{path}/approve"),Some(approval.clone()));assert_eq!(same["commit"],published["commit"]);
    let retried=fixture.run(root,&["propose",input_path.to_str().unwrap(),"--request-id",&request]);assert_eq!(retried["status"],"approved");
    let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);assert_eq!(state["projection"]["Task"]["T-removed"]["needs_review"],true);
    let status=fixture.output(&bound_tree,&["status"]);assert!(status.status.success(),"{}",String::from_utf8_lossy(&status.stderr));
    assert!(String::from_utf8_lossy(&status.stderr).contains("T-removed 需复核"));assert!(serde_json::from_slice::<Value>(&status.stdout).is_ok());
    fixture.run(root,&["daemon","stop"]);let(endpoint,handle)=fixture.open();fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);
    let restored=fixture.app(&endpoint,&format!("/projects/{handle}/proposals/{id}"),None);assert_eq!(restored["status"],"approved");
    let same=fixture.app(&endpoint,&format!("/projects/{handle}/proposals/{id}/publication/approve"),Some(approval));assert_eq!(same["commit"],published["commit"]);
    fixture.run(root,&["daemon","stop"]);fs::remove_dir_all(root.join(".git/towermd/hub")).unwrap();let(endpoint,handle)=fixture.open();
    let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);assert_eq!(state["status"],"needs_recovery");
    let recovery=fixture.app(&endpoint,&format!("/projects/{handle}/hub/rebuild"),Some(json!({})));let confirm=format!("/projects/{handle}/hub/rebuild/{}/confirm",recovery["request_id"].as_str().unwrap());
    assert!(!fixture.request(&endpoint,&confirm,"cli",Some(json!({}))).0.is_success());let recovered=fixture.app(&endpoint,&confirm,Some(json!({})));assert_eq!(recovered["status"],"ready");
    let restored=fixture.app(&endpoint,&format!("/projects/{handle}/proposals/{id}"),None);assert_eq!(restored["proposal"]["content_hash"],proposal["proposal"]["content_hash"]);assert_eq!(restored["status"],"approved");
}

#[test]
fn proposal_rejection_is_app_only_idempotent_and_cannot_turn_into_approval(){
    let fixture=Fixture::new();let(endpoint,handle,_)=fixture.baseline();let draft=fixture.synthesized();let request=uuid::Uuid::now_v7().to_string();let(proposal,_)=fixture.propose(&draft,&request);let id=proposal["id"].as_str().unwrap();
    let reject=json!({"request_id":uuid::Uuid::now_v7().to_string(),"proposal_hash":proposal["proposal"]["content_hash"],"reason":"保留现有职责划分"});let path=format!("/projects/{handle}/proposals/{id}/reject");
    assert!(!fixture.request(&endpoint,&path,"cli",Some(reject.clone())).0.is_success());
    let first=fixture.app(&endpoint,&path,Some(reject.clone()));let second=fixture.app(&endpoint,&path,Some(reject.clone()));assert_eq!(first,second);assert_eq!(first["status"],"rejected");assert_eq!(first["rejection"]["eid"],reject["request_id"]);
    assert!(!fixture.request(&endpoint,&format!("/projects/{handle}/proposals/{id}/publication/prepare"),"app",Some(json!({}))).0.is_success());
    let mut changed=reject;changed["reason"]=json!("changed after acceptance");assert!(!fixture.request(&endpoint,&path,"app",Some(changed)).0.is_success());
    let output=fixture.output(fixture.root.path(),&["propose","--approve",id]);assert!(!output.status.success());
}

#[test]
fn g2_captures_real_task_violation_and_app_can_capture_real_main_violation_without_approving(){
    let fixture=Fixture::new();let(endpoint,handle,b0)=fixture.baseline();let root=fixture.root.path();let module=&b0["review"]["modules"][0];let module_id=module["id"].as_str().unwrap();
    fixture.task("T-guard",module_id,module["files"][0].as_str().unwrap());let trees=tempfile::tempdir().unwrap();let task_tree=trees.path().join("task");
    git(root,&["worktree","add","-b","t/T-guard",task_tree.to_str().unwrap(),"main"]);fixture.run(&task_tree,&["task","claim","T-guard"]);
    fs::write(task_tree.join("outside.txt"),"unauthorized code\n").unwrap();git(&task_tree,&["add","outside.txt"]);git(&task_tree,&["commit","-qm","outside allowed paths"]);let task_commit=git(&task_tree,&["rev-parse","HEAD"]);
    let checked=fixture.output(&task_tree,&["check"]);assert!(!checked.status.success());
    let proposals=fixture.app(&endpoint,&format!("/projects/{handle}/proposals"),None);let captured=proposals["proposals"].as_array().unwrap().iter().find(|entry|entry["proposal"]["source_commit"]==task_commit).unwrap();assert_eq!(captured["status"],"draft");assert!(captured["proposal"]["draft_id"].is_null());
    let finish=fixture.run(&task_tree,&["finish"]);let evidence=finish["evidence_id"].as_str().unwrap();let result=fixture.run(&task_tree,&["finish","--wait",evidence]);assert_ne!(result["status"],"passed");assert_ne!(result["status"],"merged");
    let target=trees.path().join("manual");git(root,&["worktree","add","-b","manual-change",target.to_str().unwrap(),"main"]);
    let path=format!("docs/tower/arch/{module_id}.md");let mut source=fs::read_to_string(target.join(&path)).unwrap();source.push_str("\nUnauthorized slow edit.\n");fs::write(target.join(&path),source).unwrap();git(&target,&["add",&path]);git(&target,&["commit","-qm","manual slow change"]);let commit=git(&target,&["rev-parse","HEAD"]);git(root,&["update-ref","refs/heads/main",&commit]);
    let path=format!("/projects/{handle}/main-proposals");assert!(!fixture.request(&endpoint,&path,"cli",Some(json!({"commit":commit}))).0.is_success());
    let captured=fixture.app(&endpoint,&path,Some(json!({"commit":commit})));let retry=fixture.app(&endpoint,&path,Some(json!({"commit":commit})));assert_eq!(captured,retry);assert_eq!(captured["status"],"draft");assert_eq!(captured["proposal"]["source_commit"],commit);
    assert!(git(root,&["tag","--list","towermd/B1"]).is_empty());let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);assert_eq!(state["projection"]["MainInspection"]["passed"],false);
}
