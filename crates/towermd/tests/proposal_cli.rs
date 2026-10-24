#![cfg(unix)]
#[path = "support/envelope.rs"]
mod envelope;
#[path = "support/golden.rs"]
mod golden;
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
    fn output(&self,cwd:&Path,args:&[&str])->Output{Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN").args(args).current_dir(cwd).env("TOWERMD_RUNTIME_DIR",self.runtime.path()).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1").stdin(Stdio::null()).output().unwrap()}
    fn run(&self,cwd:&Path,args:&[&str])->Value{let output=self.output(cwd,args);assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));envelope::data_checked(args, &output)}
    fn request(&self,endpoint:&Value,path:&str,client:&str,body:Option<Value>)->(reqwest::StatusCode,Value){
        let http=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(60)).build().unwrap();let url=format!("http://127.0.0.1:{}{path}",endpoint["port"]);
        let request=if let Some(body)=body{http.post(url).json(&body)}else{http.get(url)};
        let response=request.header("x-towermd-instance",endpoint["instance"].as_str().unwrap()).header("x-towermd-client",client).send().unwrap();let status=response.status();let body=response.text().unwrap();
        let result=match serde_json::from_str::<Value>(&body){Ok(value)=>value,Err(error)=>{assert!(!status.is_success(),"{path}: {status}: invalid JSON ({error}): {body}");json!({"error":body,"path":path,"status":status.as_u16()})}};(status,result)
    }
    fn app(&self,endpoint:&Value,path:&str,body:Option<Value>)->Value{let(status,result)=self.request(endpoint,path,"app",body);assert!(status.is_success(),"{path}: {status} {result}");result}
    fn open(&self)->(Value,String){let endpoint=self.run(self.root.path(),&["daemon","ensure"]);let opened=self.app(&endpoint,"/projects/open",Some(json!({"path":self.root.path()})));(endpoint,opened["handle"].as_str().unwrap().into())}
    fn synthesized(&self)->Value{
        let root=self.root.path();let scan=self.run(root,&["dev","scan","start"]);let id=scan["id"].as_str().unwrap();
        // Exit 5 means the scan is still running after one wait window.
        let ready=loop{let output=self.output(root,&["dev","scan","wait",id]);if output.status.code()!=Some(5){break output;}};assert!(ready.status.success(),"{}",String::from_utf8_lossy(&ready.stderr));
        let template=self.run(root,&["dev","draft","template",id]);let path=self.runtime.path().join("submission.json");fs::write(&path,serde_json::to_vec(&template).unwrap()).unwrap();
        self.run(root,&["dev","draft","submit",path.to_str().unwrap(),"--producer","qa/1"])
    }
    fn verified(&self,endpoint:&Value,handle:&str,draft:Value)->Value{self.verified_with(endpoint,handle,draft,&|_|json!([{"directory":"","command":"printf 'real proposal reviewed proof\\n'"}]))}
    fn verified_with(&self,endpoint:&Value,handle:&str,mut draft:Value,commands:&dyn Fn(&str)->Value)->Value{
        let modules=draft["review"]["modules"].as_array().cloned().unwrap_or_else(||draft["scan"]["modules"].as_array().unwrap().clone());
        for module in modules{draft=self.app(endpoint,&format!("/projects/{handle}/baselines/{}/review",draft["id"].as_str().unwrap()),Some(json!({"action":{"kind":"commands","module_id":module["id"],"commands":commands(module["id"].as_str().unwrap())}})));}
        let request=uuid::Uuid::now_v7().to_string();self.app(endpoint,&format!("/projects/{handle}/baselines/{}/verify",draft["id"].as_str().unwrap()),Some(json!({"request_id":request})));
        let deadline=Instant::now()+Duration::from_secs(30);
        loop{let job=self.app(endpoint,&format!("/projects/{handle}/baseline-verifications/{request}"),None);if job["status"]!="running"{assert_eq!(job["status"],"ready","{job}");return self.app(endpoint,&format!("/projects/{handle}/baselines/{}",job["result_id"].as_str().unwrap()),None);}assert!(Instant::now()<deadline,"{job}");std::thread::sleep(Duration::from_millis(50));}
    }
    fn approve(&self,endpoint:&Value,path:&str)->(Value,Value){let preview=self.app(endpoint,&format!("{path}/prepare"),Some(json!({})));let input=json!({"request_id":preview["request_id"],"target":preview["target"],"prepared_at":preview["prepared_at"],"content_hash":preview["content_hash"]});let result=self.app(endpoint,&format!("{path}/approve"),Some(input.clone()));(result,input)}
    fn baseline(&self)->(Value,String,Value){let(endpoint,handle)=self.open();self.app(&endpoint,&format!("/projects/{handle}/snapshot"),Some(json!({})));let draft=self.verified(&endpoint,&handle,self.synthesized());self.publish_baseline(endpoint,handle,draft)}
    fn publish_baseline(&self,endpoint:Value,handle:String,draft:Value)->(Value,String,Value){git(self.root.path(),&["checkout","-qb","qa-owner"]);let(result,_)=self.approve(&endpoint,&format!("/projects/{handle}/baselines/{}/publication",draft["id"].as_str().unwrap()));assert_eq!(result["target"],"B0");(endpoint,handle,draft)}
    fn propose(&self,draft:&Value,request:&str)->(Value,std::path::PathBuf){self.propose_from(draft,request,None)}
    fn propose_from(&self,draft:&Value,request:&str,source:Option<&str>)->(Value,std::path::PathBuf){
        let path=self.runtime.path().join(format!("proposal-{request}.json"));fs::write(&path,serde_json::to_vec(&json!({"draft_id":draft["id"],"source_proposal_id":source,"kind":"merge","reason":"两个模块共同承担同一调用路径","alternatives":["保留两个模块","合并职责"],"recommendation":"按审阅后的模块合并"})).unwrap()).unwrap();
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
    let checked=fixture.output(&task_tree,&["dev","check"]);assert!(!checked.status.success());
    let proposals=fixture.app(&endpoint,&format!("/projects/{handle}/proposals"),None);let captured=proposals["proposals"].as_array().unwrap().iter().find(|entry|entry["proposal"]["source_commit"]==task_commit).unwrap();assert_eq!(captured["status"],"draft");assert!(captured["proposal"]["draft_id"].is_null());
    let finish=fixture.run(&task_tree,&["dev","finish"]);let evidence=finish["evidence_id"].as_str().unwrap();let result=fixture.output(&task_tree,&["dev","finish","--wait",evidence]);assert_eq!(envelope::code(&result),"gate_failed");let result=envelope::parse(&result)["data"].clone();assert_ne!(result["status"],"passed");assert_ne!(result["status"],"merged");
    let target=trees.path().join("manual");git(root,&["worktree","add","-b","manual-change",target.to_str().unwrap(),"main"]);
    let path=format!("docs/tower/arch/{module_id}.md");let mut source=fs::read_to_string(target.join(&path)).unwrap();source.push_str("\nUnauthorized slow edit.\n");fs::write(target.join(&path),source).unwrap();git(&target,&["add",&path]);git(&target,&["commit","-qm","manual slow change"]);let commit=git(&target,&["rev-parse","HEAD"]);git(root,&["update-ref","refs/heads/main",&commit]);
    let path=format!("/projects/{handle}/main-proposals");assert!(!fixture.request(&endpoint,&path,"cli",Some(json!({"commit":commit}))).0.is_success());
    let captured=fixture.app(&endpoint,&path,Some(json!({"commit":commit})));let retry=fixture.app(&endpoint,&path,Some(json!({"commit":commit})));assert_eq!(captured,retry);assert_eq!(captured["status"],"draft");assert_eq!(captured["proposal"]["source_commit"],commit);
    assert!(git(root,&["tag","--list","towermd/B1"]).is_empty());let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);assert_eq!(state["projection"]["MainInspection"]["passed"],false);
}

#[test]
fn slow_answer_is_completed_through_a_real_proposal_and_only_publication_resolves_the_hold(){
    let fixture=Fixture::new();let(endpoint,handle,b0)=fixture.baseline();let root=fixture.root.path();let module=&b0["review"]["modules"][0];let module_id=module["id"].as_str().unwrap();
    fixture.task("T-slow-answer",module_id,module["files"][0].as_str().unwrap());let trees=tempfile::tempdir().unwrap();let tree=trees.path().join("slow-answer");
    git(root,&["worktree","add","-b","t/T-slow-answer",tree.to_str().unwrap(),"main"]);let claim=fixture.run(&tree,&["task","claim","T-slow-answer"]);let run=claim["run"]["binding"]["id"].as_str().unwrap();
    let ask=json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"text":"可以调整该模块的慢层契约吗？","targets":[module_id]});
    let(status,asked)=fixture.request(&endpoint,&format!("/projects/{handle}/ask"),"cli",Some(ask));assert!(status.is_success(),"{asked}");let hold=asked["hold"].as_str().unwrap();
    let answer_id=uuid::Uuid::now_v7().to_string();let answer=fixture.app(&endpoint,&format!("/projects/{handle}/answers"),Some(json!({"request_id":answer_id,"hold":hold,"text":"请补全变更提案，让我审阅实际差异后再决定","remember_as":"temporary","targets":[module_id]})));
    let source=answer["proposal"].as_str().unwrap();let captured=fixture.app(&endpoint,&format!("/projects/{handle}/proposals/{source}"),None);assert_eq!(captured["source_event"],answer_id);assert_eq!(captured["status"],"draft");assert!(captured.get("proposal").is_none());
    let wait_path=format!("/projects/{handle}/wait");let wait=json!({"cwd":tree,"run":run,"hold":hold,"wait_ms":1});
    let(status,deferred)=fixture.request(&endpoint,&wait_path,"cli",Some(wait.clone()));assert!(status.is_success(),"{deferred}");assert_eq!(deferred["status"],"deferred");
    assert!(!fixture.output(&tree,&["dev","check"]).status.success());
    let malformed=json!({"request_id":uuid::Uuid::now_v7().to_string(),"hold":hold,"text":"forged publication","remember_as":"temporary","publication":{"commit":"fake"}});
    let malformed_path=format!("/projects/{handle}/answers");let(status,rejected)=fixture.request(&endpoint,&malformed_path,"app",Some(malformed));
    assert_eq!(status,reqwest::StatusCode::UNPROCESSABLE_ENTITY,"{malformed_path}: {status}: {rejected}");
    let error=rejected["error"].as_str().expect("schema rejection must describe the unknown field");assert!(error.contains("unknown field")&&error.contains("publication"),"{malformed_path}: {status}: {rejected}");
    let draft=fixture.verified(&endpoint,&handle,fixture.synthesized());
    let invalid_source=json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":root,"input":{"draft_id":draft["id"],"source_proposal_id":format!("P-{}",uuid::Uuid::now_v7()),"kind":"other","reason":"missing capture","alternatives":["keep"],"recommendation":"review"}});
    assert!(!fixture.request(&endpoint,&format!("/projects/{handle}/proposals"),"cli",Some(invalid_source)).0.is_success());
    let formal_id=uuid::Uuid::now_v7().to_string();let(formal,input_path)=fixture.propose_from(&draft,&formal_id,Some(source));let id=formal["id"].as_str().unwrap();assert_eq!(formal["proposal"]["source_proposal_id"],source);
    let path=format!("/projects/{handle}/proposals/{id}/publication");let preview=fixture.app(&endpoint,&format!("{path}/prepare"),Some(json!({})));
    assert_eq!(fixture.request(&endpoint,&wait_path,"cli",Some(wait.clone())).1["status"],"deferred","preparing never approves an answer");
    let approval=json!({"request_id":preview["request_id"],"target":preview["target"],"prepared_at":preview["prepared_at"],"content_hash":preview["content_hash"]});
    assert!(!fixture.request(&endpoint,&format!("{path}/approve"),"cli",Some(approval.clone())).0.is_success());
    let published=fixture.app(&endpoint,&format!("{path}/approve"),Some(approval));let commit=published["commit"].as_str().unwrap();
    let decision=git(root,&["show",&format!("{commit}:docs/tower/decisions/D-{id}.md")]);assert!(decision.contains(source));
    let(status,answered)=fixture.request(&endpoint,&wait_path,"cli",Some(wait.clone()));assert!(status.is_success(),"{answered}");assert_eq!(answered["status"],"answered");
    assert_eq!(answered["answer"]["event"],answer_id);assert_eq!(answered["answer"]["proposal"],source);assert_eq!(answered["answer"]["publication"]["proposal"],id);assert_eq!(answered["answer"]["publication"]["commit"],commit);
    let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);assert_eq!(state["projection"]["Task"]["T-slow-answer"]["needs_review"],true);assert_eq!(state["projection"]["Proposal"][source]["linked_proposal"],id);
    let messages_path=format!("/projects/{handle}/messages");let context=json!({"cwd":tree,"run":run});let queued=fixture.request(&endpoint,&messages_path,"cli",Some(context));assert!(queued.0.is_success(),"{:?}",queued);
    let delivered=queued.1["messages"].as_array().unwrap().iter().find(|message|message["message"]["hold"]==hold).unwrap();
    let ack=json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"message":delivered["message"]});let acknowledged=fixture.request(&endpoint,&format!("/projects/{handle}/messages/ack"),"cli",Some(ack));assert!(acknowledged.0.is_success(),"{:?}",acknowledged);
    assert_eq!(fixture.request(&endpoint,&wait_path,"cli",Some(wait)).1["answer"]["publication"]["commit"],commit);
    // The old authorization still cannot pass G0 merely because its question is
    // resolved: this task must be reviewed against the published baseline.
    let checked=fixture.output(&tree,&["dev","check"]);assert!(!checked.status.success());assert!(String::from_utf8_lossy(&checked.stderr).contains("需复核"));
    let retried=fixture.run(root,&["propose",input_path.to_str().unwrap(),"--request-id",&formal_id]);assert_eq!(retried["status"],"approved");
    golden::record(root,"proposal");
    fixture.run(root,&["daemon","stop"]);let(endpoint,handle)=fixture.open();let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);assert_eq!(state["projection"]["Answer"][hold]["publication"]["commit"],commit);assert_eq!(state["projection"]["Proposal"][source]["status"],"approved");
    fixture.run(root,&["daemon","stop"]);fs::remove_dir_all(root.join(".git/towermd/hub")).unwrap();let(endpoint,handle)=fixture.open();let recovery=fixture.app(&endpoint,&format!("/projects/{handle}/hub/rebuild"),Some(json!({})));
    fixture.app(&endpoint,&format!("/projects/{handle}/hub/rebuild/{}/confirm",recovery["request_id"].as_str().unwrap()),Some(json!({})));
    let state=fixture.app(&endpoint,&format!("/projects/{handle}/state"),None);assert_eq!(state["projection"]["Answer"][hold]["publication"]["commit"],commit);assert_eq!(state["projection"]["Task"]["T-slow-answer"]["needs_review"],true);
}

/// FR-125: a proposal that drops a module's only check carries its weakening
/// items, and its submission raises one active `core.weakening` on the
/// proposal, attributed to the run it came from. It still needs the App.
#[test]
fn a_submitted_proposal_dropping_a_check_is_marked_signalled_and_attributed_to_its_run(){
    let fixture=Fixture::new();let root=fixture.root.path();let(endpoint,handle)=fixture.open();
    fixture.app(&endpoint,&format!("/projects/{handle}/snapshot"),Some(json!({})));
    let first=fixture.synthesized();let a=first["scan"]["modules"][0]["id"].as_str().unwrap().to_owned();
    let own=|id:&str|if id==a{json!([{"directory":"","command":"printf 'module a proof\\n'"}])}else{json!([{"directory":"","command":"printf 'shared proof\\n'"}])};
    let reviewed=fixture.verified_with(&endpoint,&handle,first,&own);
    let(endpoint,handle,b0)=fixture.publish_baseline(endpoint,handle,reviewed);
    let module=b0["review"]["modules"].as_array().unwrap().iter().find(|module|module["id"]==a.as_str()).unwrap().clone();
    fixture.task("T-propose",&a,module["files"][0].as_str().unwrap());
    let trees=tempfile::tempdir().unwrap();let tree=trees.path().join("task");
    git(root,&["worktree","add","-b","t/T-propose",tree.to_str().unwrap(),"main"]);
    fixture.run(&tree,&["task","claim","T-propose","--harness","claude-code","--harness-version","2.1"]);
    let dropped=|id:&str|if id==a{json!([])}else{json!([{"directory":"","command":"printf 'shared proof\\n'"}])};
    let draft=fixture.verified_with(&endpoint,&handle,fixture.synthesized(),&dropped);
    let request=uuid::Uuid::now_v7().to_string();let input=fixture.runtime.path().join("weaker.json");
    fs::write(&input,serde_json::to_vec(&json!({"draft_id":draft["id"],"kind":"merge","reason":"模块 a 暂无可用测试","alternatives":["保留检查"],"recommendation":"先去掉检查"})).unwrap()).unwrap();
    let proposal=fixture.run(&tree,&["propose",input.to_str().unwrap(),"--request-id",&request]);let id=proposal["id"].as_str().unwrap();
    assert_eq!(proposal["status"],"submitted");
    assert_eq!(proposal["weakening"],json!([{"change":"check_removed","text":"删除检查「printf 'module a proof\\n'」"}]),"{proposal}");
    let listed=fixture.app(&endpoint,&format!("/projects/{handle}/proposals"),None);
    assert!(listed["proposals"].as_array().unwrap().iter().any(|entry|entry["proposal"]["id"]==id && entry["weakening"]==proposal["weakening"]));
    let tower=||fixture.app(&endpoint,&format!("/projects/{handle}/state"),None)["projection"]["TowerApi"].clone();
    let log=tower();let signal=log["signals"].as_array().unwrap().iter().find(|entry|entry["data"]["kind"]=="core.weakening").unwrap().clone();
    assert_eq!((signal["target"].as_str(),signal["data"]["level"].as_str(),signal["last"]["actor"].as_str()),(Some(id),Some("active"),Some("process:towermd")),"{signal}");
    let refs:Vec<&str>=signal["refs"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(refs.contains(&format!("proposal:{id}").as_str())&&refs.contains(&"delegation:T-propose")&&refs.iter().any(|entry|entry.starts_with("config:")&&entry.contains("claude-code")),"{refs:?}");
    // A retried submission raises nothing new; only the App may approve.
    fixture.run(&tree,&["propose",input.to_str().unwrap(),"--request-id",&request]);
    assert_eq!(tower()["weakenings"].as_array().unwrap().len(),1);
    assert!(!fixture.request(&endpoint,&format!("/projects/{handle}/proposals/{id}/publication/prepare"),"cli",Some(json!({}))).0.is_success());
}

impl Fixture{
    /// J4: a from-baseline scan, waited to the end, then the daemon's draft for it.
    fn from_baseline(&self,endpoint:&Value,handle:&str)->(String,Value){
        let root=self.root.path();let scan=self.run(root,&["dev","scan","start","--from-baseline"]);let id=scan["id"].as_str().unwrap().to_string();
        let ready=loop{let output=self.output(root,&["dev","scan","wait",&id]);if output.status.code()!=Some(5){break output;}};assert!(ready.status.success(),"{}",String::from_utf8_lossy(&ready.stderr));
        let (status,draft)=self.request(endpoint,&format!("/projects/{handle}/scans/{id}/draft"),"cli",Some(json!({})));assert!(status.is_success(),"{status} {draft}");(id,draft)
    }
}

#[test]
fn a_draft_entered_from_the_baseline_is_compared_then_published_only_by_app_approval(){
    let fixture=Fixture::new();let root=fixture.root.path();
    // Without an approved baseline there is nothing to re-enter.
    let (endpoint,handle)=fixture.open();
    let refused=fixture.output(root,&["dev","scan","start","--from-baseline"]);assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("G0"),"{}",String::from_utf8_lossy(&refused.stderr));
    fixture.app(&endpoint,&format!("/projects/{handle}/snapshot"),Some(json!({})));
    let b0=fixture.verified(&endpoint,&handle,fixture.synthesized());let(endpoint,handle,b0)=fixture.publish_baseline(endpoint,handle,b0);
    let old:Vec<String>=b0["review"]["modules"].as_array().unwrap().iter().map(|m|m["id"].as_str().unwrap().into()).collect();
    let(a,b)=(old[0].clone(),old[1].clone());

    let(scan,draft)=fixture.from_baseline(&endpoint,&handle);
    assert!(draft["producer"].as_str().unwrap().starts_with("towermd/"),"{}",draft["producer"]);assert!(draft["review"].is_null());
    assert_eq!(draft["scan"]["origin"]["baseline"],"towermd/B0");
    let ids:Vec<&str>=draft["scan"]["modules"].as_array().unwrap().iter().map(|m|m["id"].as_str().unwrap()).collect();assert_eq!(ids,old);
    assert!(draft["scan"]["modules"].as_array().unwrap().iter().all(|m|m["rationale"].as_str().unwrap().starts_with("沿用 towermd/B0")));
    // The same scan gives the same draft; a plain scan cannot be turned into one this way.
    let(_,again)=fixture.request(&endpoint,&format!("/projects/{handle}/scans/{scan}/draft"),"cli",Some(json!({})));assert_eq!(again["id"],draft["id"]);
    let plain=fixture.run(root,&["dev","scan","start"]);let plain=plain["id"].as_str().unwrap();
    while fixture.output(root,&["dev","scan","wait",plain]).status.code()==Some(5){}
    let(status,error)=fixture.request(&endpoint,&format!("/projects/{handle}/scans/{plain}/draft"),"cli",Some(json!({})));assert!(!status.is_success());assert!(error["error"].as_str().unwrap().contains("不是从基线开始"),"{error}");
    let unreviewed=fixture.run(root,&["dev","draft","compare",draft["id"].as_str().unwrap()]);
    assert_eq!(unreviewed["comparison"]["baseline"],"towermd/B0");assert_eq!(unreviewed["outdated"],false);assert_eq!(unreviewed["stale"],false);
    assert!(unreviewed["comparison"]["objects"].as_array().unwrap().is_empty(),"{unreviewed}");

    // A person reviews: merge b into a; the comparison names it.
    let merged=fixture.app(&endpoint,&format!("/projects/{handle}/baselines/{}/review",draft["id"].as_str().unwrap()),Some(json!({"action":{"kind":"merge","module_ids":[&a,&b],"target_id":&a,"title":"合并模块"}})));
    let reviewed=fixture.verified(&endpoint,&handle,merged);
    let compared=fixture.run(root,&["dev","draft","compare",reviewed["id"].as_str().unwrap()]);let objects=compared["comparison"]["objects"].as_array().unwrap();
    let find=|id:&str|objects.iter().find(|o|o["id"]==id).unwrap_or_else(||panic!("{id}: {compared}")).clone();
    assert_eq!(find(&b)["kinds"],json!(["removed","merged_into"]));assert_eq!(find(&b)["related"],json!(a));
    assert!(find(&a)["kinds"].as_array().unwrap().contains(&json!("files")),"{compared}");
    assert!(compared["impact"]["modules"].as_array().unwrap().contains(&json!(b)),"{compared}");
    assert!(compared["changed_paths"].as_array().unwrap().iter().any(|p|p.as_str().unwrap().ends_with(&format!("{b}.md"))),"{compared}");

    // Publication is unchanged: the CLI proposes, only the App approves.
    let(proposal,_)=fixture.propose(&reviewed,&uuid::Uuid::now_v7().to_string());assert_eq!(proposal["status"],"submitted");
    let path=format!("/projects/{handle}/proposals/{}/publication",proposal["id"].as_str().unwrap());
    assert!(!fixture.request(&endpoint,&format!("{path}/prepare"),"cli",Some(json!({}))).0.is_success());
    let(direct,_)=fixture.request(&endpoint,&format!("/projects/{handle}/baselines/{}/publication/prepare",reviewed["id"].as_str().unwrap()),"app",Some(json!({})));assert!(!direct.is_success());
    let(published,_)=fixture.approve(&endpoint,&path);assert_eq!(published["target"],"B1");
    // A scan started from B0 no longer becomes a draft once B1 is current.
    let(status,error)=fixture.request(&endpoint,&format!("/projects/{handle}/scans/{scan}/draft"),"cli",Some(json!({})));assert!(!status.is_success());assert!(error["error"].as_str().unwrap().contains("towermd/B1"),"{error}");
    let stale=fixture.run(root,&["dev","draft","compare",reviewed["id"].as_str().unwrap()]);assert_eq!(stale["outdated"],true);
    let(_,next)=fixture.from_baseline(&endpoint,&handle);assert_eq!(next["scan"]["origin"]["baseline"],"towermd/B1");
    let ids:Vec<&str>=next["scan"]["modules"].as_array().unwrap().iter().map(|m|m["id"].as_str().unwrap()).collect();assert!(ids.contains(&a.as_str())&&!ids.contains(&b.as_str()),"{ids:?}");
}
