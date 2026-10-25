#[path = "support/envelope.rs"]
mod envelope;

#[path = "support/golden.rs"]
mod golden;
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    root: tempfile::TempDir,
    runtime: tempfile::TempDir,
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Question Test",
            "-c",
            "user.email=question@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            runtime: tempfile::tempdir().unwrap(),
        };
        let root = fixture.root.path();
        git(root, &["init", "-qb", "main"]);
        fs::create_dir(root.join(".towermd")).unwrap();
        fs::write(
            root.join(".towermd/config.yaml"),
            "project_id: tasks-project\ndocs_dir: docs/tower\nhub: local\n",
        )
        .unwrap();
        for name in ["alpha", "beta", "gamma"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::write(root.join(name).join("code.rs"), "fn task() {}\n").unwrap();
        }
        fs::write(
            root.join("Makefile"),
            "test:\n\t@printf 'real task fixture verification\\n'\n",
        )
        .unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "fixture"]);
        fixture
    }
    fn output(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_towermd")).env("TOWERMD_LANG", "zh-CN")
            .args(args)
            .current_dir(cwd)
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
    fn run(&self, cwd: &Path, args: &[&str]) -> Value {
        let output = self.output(cwd, args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        envelope::data_checked(args, &output)
    }
    fn post(&self, endpoint: &Value, path: &str, body: Value) -> Value {
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap()
            .post(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", "app")
            .json(&body)
            .send()
            .unwrap();
        let status = response.status();
        let result: Value = response.json().unwrap();
        assert!(status.is_success(), "{path}: {status} {result}");
        result
    }
    fn get(&self, endpoint: &Value, path: &str) -> Value {
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{}{path}", endpoint["port"]))
            .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client", "app")
            .send()
            .unwrap();
        let status = response.status();
        let result: Value = response.json().unwrap();
        assert!(status.is_success(), "{path}: {status} {result}");
        result
    }
    fn publish(&self) -> Value {
        let root = self.root.path();
        let endpoint = self.run(root, &["daemon", "ensure"]);
        let opened = self.post(&endpoint, "/projects/open", json!({"path":root}));
        let handle = opened["handle"].as_str().unwrap();
        self.post(
            &endpoint,
            &format!("/projects/{handle}/snapshot"),
            json!({}),
        );
        let scan = self.run(root, &["dev", "scan", "start"]);
        let scan_id = scan["id"].as_str().unwrap();
        // Exit 5 means the scan is still running after one wait window.
        let ready = loop {
            let output = self.output(root, &["dev", "scan", "wait", scan_id]);
            if output.status.code() != Some(5) {
                break output;
            }
        };
        assert!(ready.status.success(), "{}", String::from_utf8_lossy(&ready.stderr));
        let template = self.run(root, &["dev", "draft", "template", scan_id]);
        let path = self.runtime.path().join("input.json");
        fs::write(&path, serde_json::to_vec(&template).unwrap()).unwrap();
        let mut draft = self.run(
            root,
            &[
                "dev",
                "draft",
                "submit",
                path.to_str().unwrap(),
                "--producer",
                "test/1",
            ],
        );
        for module in draft["scan"]["modules"].as_array().unwrap().clone() {
            draft=self.post(&endpoint,&format!("/projects/{handle}/baselines/{}/review",draft["id"].as_str().unwrap()),json!({"action":{"kind":"commands","module_id":module["id"],"commands":[{"directory":"","command":"printf 'task baseline proof\\n'"}]}}));
        }
        let request = uuid::Uuid::now_v7().to_string();
        self.post(
            &endpoint,
            &format!(
                "/projects/{handle}/baselines/{}/verify",
                draft["id"].as_str().unwrap()
            ),
            json!({"request_id":request}),
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        let ready = loop {
            let job = self.get(
                &endpoint,
                &format!("/projects/{handle}/baseline-verifications/{request}"),
            );
            if job["status"] != "running" {
                break job;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(ready["status"], "ready", "{ready}");
        draft = self.get(
            &endpoint,
            &format!(
                "/projects/{handle}/baselines/{}",
                ready["result_id"].as_str().unwrap()
            ),
        );
        git(root, &["checkout", "-qb", "qa-owner"]);
        let path = format!(
            "/projects/{handle}/baselines/{}/publication",
            draft["id"].as_str().unwrap()
        );
        let prepared = self.post(&endpoint, &format!("{path}/prepare"), json!({}));
        self.post(&endpoint,&format!("{path}/approve"),json!({"request_id":prepared["request_id"],"target":prepared["target"],"prepared_at":prepared["prepared_at"],"content_hash":prepared["content_hash"]}));
        draft
    }
    fn task(&self, id: &str, module: &str, path: &str) -> std::path::PathBuf {
        let target = self.runtime.path().join(format!("{id}.json"));
        fs::write(&target,serde_json::to_vec(&json!({"id":id,"title":format!("Task {id}"),"module":module,"allowed_paths":[path],"acceptance":["existing smoke test passes"]})).unwrap()).unwrap();
        target
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output(self.root.path(), &["daemon", "stop"]);
    }
}

impl Fixture {
    fn request(&self, endpoint:&Value, path:&str, actor:&str, body:Value)->(u16,Value) {
        let response=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap()
            .post(format!("http://127.0.0.1:{}{path}",endpoint["port"]))
            .header("x-towermd-instance",endpoint["instance"].as_str().unwrap())
            .header("x-towermd-client",actor).json(&body).send().unwrap();
        let status=response.status().as_u16();
        (status,response.json().unwrap())
    }
}
#[test]
fn questions_reuse_only_module_decisions_and_output_delivery_is_explicit_and_durable() {
    let fixture=Fixture::new();
    let draft=fixture.publish();
    let root=fixture.root.path();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let opened=fixture.post(&endpoint,"/projects/open",json!({"path":root}));
    let handle=opened["handle"].as_str().unwrap();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let task=fixture.task("T-question",module,path);
    fixture.run(root,&["task","add",task.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();
    let tree=trees.path().join("question");
    git(root,&["worktree","add","-b","t/T-question",tree.to_str().unwrap(),"main"]);
    let claimed=fixture.run(&tree,&["task","claim","T-question"]);
    let run=claimed["run"]["binding"]["id"].as_str().unwrap();
    let context=json!({"cwd":tree,"run":run});
    let ask_id=uuid::Uuid::now_v7().to_string();
    let ask=json!({"request_id":ask_id,"cwd":tree,"run":run,"text":"Output ＪＳＯＮ？","options":["Yes","No"],"recommended":0});
    let ask_path=format!("/projects/{handle}/ask");
    let (status,asked)=fixture.request(&endpoint,&ask_path,"cli",ask.clone());
    assert_eq!(status,200,"{asked}");
    assert_eq!(asked["wait"]["status"],"pending");
    let hold=asked["hold"].as_str().unwrap();
    let (status,retried)=fixture.request(&endpoint,&ask_path,"cli",ask.clone());
    assert_eq!(status,200,"{retried}");assert_eq!(retried["event"],asked["event"]);
    let answer_id=uuid::Uuid::now_v7().to_string();
    let answer=json!({"request_id":answer_id,"hold":hold,"text":"Yes, JSON","remember_as":"module_decision","assumption":"下游只读 JSON"});
    let answer_path=format!("/projects/{handle}/answers");
    assert_eq!(fixture.request(&endpoint,&answer_path,"cli",answer.clone()).0,409);
    let (status,answered)=fixture.request(&endpoint,&answer_path,"app",answer.clone());
    assert_eq!(status,200,"{answered}");assert_eq!(answered["event"],answer_id);
    let (status,retried)=fixture.request(&endpoint,&answer_path,"app",answer.clone());
    assert_eq!(status,200,"{retried}");assert_eq!(retried,answered);
    let mut changed=answer.clone();changed["text"]=json!("No");
    assert_eq!(fixture.request(&endpoint,&answer_path,"app",changed).0,400);
    let mut changed=answer.clone();changed["assumption"]=json!("另一个假设");
    assert_eq!(fixture.request(&endpoint,&answer_path,"app",changed).0,400,"a retry must carry the same assumption");
    let messages_path=format!("/projects/{handle}/messages");
    let first=fixture.request(&endpoint,&messages_path,"cli",context.clone());
    assert_eq!(first.0,200,"{:?}",first);assert_eq!(first.1["messages"].as_array().unwrap().len(),1);
    assert_eq!(first.1["messages"][0]["text"],"Yes, JSON\n（假设：下游只读 JSON）","the agent receives the assumption with the answer");
    assert_eq!(fixture.request(&endpoint,&messages_path,"cli",context.clone()).1,first.1);
    let ack=json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"message":first.1["messages"][0]["message"]});
    let ack_path=format!("/projects/{handle}/messages/ack");
    let receipt=fixture.request(&endpoint,&ack_path,"cli",ack.clone());assert_eq!(receipt.0,200,"{:?}",receipt);
    assert_eq!(fixture.request(&endpoint,&ack_path,"cli",ack).1,receipt.1);
    assert!(fixture.request(&endpoint,&messages_path,"cli",context.clone()).1["messages"].as_array().unwrap().is_empty());
    let reuse=json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"text":" output json? "});
    let reused=fixture.request(&endpoint,&ask_path,"cli",reuse);assert_eq!(reused.0,200,"{:?}",reused);
    assert_eq!(reused.1["wait"]["status"],"answered");
    assert_eq!(reused.1["wait"]["answer"]["actor"],"process:towermd-dedupe");
    let prompt=json!({"request_id":uuid::Uuid::now_v7().to_string(),"scope":{"kind":"module","module":module},"text":"Check the edge case"});
    let prompt_path=format!("/projects/{handle}/prompts");
    assert_eq!(fixture.request(&endpoint,&prompt_path,"cli",prompt.clone()).0,409);
    let sent=fixture.request(&endpoint,&prompt_path,"app",prompt.clone());assert_eq!(sent.0,200,"{:?}",sent);
    assert_eq!(fixture.request(&endpoint,&prompt_path,"app",prompt).1,sent.1);
    let queued=fixture.request(&endpoint,&messages_path,"cli",context.clone());
    assert_eq!(queued.1["messages"].as_array().unwrap().len(),2);
    fixture.run(root,&["daemon","stop"]);
    let new_endpoint=fixture.run(root,&["daemon","ensure"]);
    let opened=fixture.post(&new_endpoint,"/projects/open",json!({"path":root}));
    let new_handle=opened["handle"].as_str().unwrap();
    let restored=fixture.request(&new_endpoint,&format!("/projects/{new_handle}/messages"),"cli",context);
    assert_eq!(restored.0,200,"{:?}",restored);assert_eq!(restored.1["messages"],queued.1["messages"]);
    let state=fixture.get(&new_endpoint,&format!("/projects/{new_handle}/state"));
    assert_eq!(state["projection"]["Hold"][hold]["draft"]["recommended"],0);
    assert_eq!(state["projection"]["Answer"][hold]["event"],answer_id);
    assert_eq!(state["projection"]["Delivery"].as_object().unwrap().len(),1);
    let details_path=format!("/projects/{new_handle}/runs/{run}/events");
    let details=fixture.get(&new_endpoint,&details_path);
    assert_eq!(details["run_id"],run);assert_eq!(details["task_id"],"T-question");
    assert!(details["harness_output"].is_null());
    assert_eq!(details["messages"].as_array().unwrap().len(),5);
    assert!(details["messages"].as_array().unwrap().iter().any(|message|message["kind"]=="answer" && message["text"]=="Yes, JSON" && message["actor"]=="human:local"));
    assert!(details["messages"].as_array().unwrap().iter().any(|message|message["kind"]=="prompt" && message["text"]=="Check the edge case"));
    assert!(details["events"].as_array().unwrap().iter().any(|event|event["eid"]==answer_id));
    assert!(details["evidence"].as_array().unwrap().is_empty());
    // This endpoint never loads arbitrary log paths. Its evidence is the exact
    // accepted verifier report produced below, including the real output.
    // Human answers and daemon shutdown have legitimately advanced main with
    // snapshots. Finish requires a real task commit based on that current main.
    let source=fs::read_to_string(tree.join(path)).unwrap();
    fs::write(tree.join(path),format!("{source}\n// Question task verification change.\n")).unwrap();
    git(&tree,&["add","--",path]);
    git(&tree,&["commit","-qm","implement question fixture task"]);
    git(&tree,&["rebase","main"]);
    let started=fixture.request(&new_endpoint,&format!("/projects/{new_handle}/task-finish"),"cli",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run}));
    assert_eq!(started.0,200,"{:?}",started);
    let evidence_id=started.1["evidence_id"].as_str().unwrap();
    let deadline=Instant::now()+Duration::from_secs(20);
    let verified=loop {
        let value=fixture.get(&new_endpoint,&format!("/projects/{new_handle}/task-finish/{evidence_id}"));
        if !matches!(value["status"].as_str(),Some("queued"|"running")){break value;}
        assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(verified["status"],"passed","{verified}");
    let details=fixture.get(&new_endpoint,&details_path);
    assert_eq!(details["evidence"].as_array().unwrap().len(),1);
    assert_eq!(details["evidence"][0]["completion_eid"],verified["finish_eid"]);
    assert_eq!(details["evidence"][0]["report"],verified["evidence"]["report"]);
    assert!(details["evidence"][0]["report"]["commands"].as_array().unwrap().iter()
        .any(|command|command["log_excerpt"].as_str().is_some_and(|text|text.contains("task baseline proof"))));
}

#[test]
fn app_tasks_and_project_pause_use_real_baseline_and_frozen_checkpoint_messages() {
    let fixture=Fixture::new();
    let draft=fixture.publish();
    let root=fixture.root.path();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let opened=fixture.post(&endpoint,"/projects/open",json!({"path":root}));
    let handle=opened["handle"].as_str().unwrap();
    let options=fixture.get(&endpoint,&format!("/projects/{handle}/tasks/options"));
    assert_eq!(options["baseline"],"towermd/B0");
    assert_eq!(options["modules"].as_array().unwrap().len(),draft["review"]["modules"].as_array().unwrap().len());
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let route=format!("/projects/{handle}/tasks/human");
    let proposed=json!({"request_id":uuid::Uuid::now_v7().to_string(),"task":{"id":"T-human-proposed",
        "title":"Incomplete human draft","module":module,"allowed_paths":[],"acceptance":[]}});
    assert_eq!(fixture.request(&endpoint,&route,"cli",proposed.clone()).0,409);
    let created=fixture.post(&endpoint,&route,proposed.clone());
    assert_eq!(created["task"]["origin"],"app_human");
    assert_eq!(created["task"]["state"],"proposed");
    assert_eq!(fixture.post(&endpoint,&route,proposed.clone())["event"],created["event"]);
    let mut changed=proposed;changed["task"]["title"]=json!("Different content");
    assert_eq!(fixture.request(&endpoint,&route,"app",changed).0,400);
    let task=json!({"id":"T-human-active","title":"Human scoped task","module":module,
        "allowed_paths":[path],"acceptance":["real smoke test passes"]});
    let request=json!({"request_id":uuid::Uuid::now_v7().to_string(),"task":task});
    let added=fixture.post(&endpoint,&route,request.clone());
    assert_eq!(added["task"]["state"],"ready");
    assert_eq!(added["task"]["origin"],"app_human");
    let other=json!({"request_id":uuid::Uuid::now_v7().to_string(),"task":{"id":"T-human-next",
        "title":"Later task","module":module,"allowed_paths":[path],"acceptance":["real smoke test passes"]}});
    fixture.post(&endpoint,&route,other);
    let trees=tempfile::tempdir().unwrap();
    let tree=trees.path().join("human-active");
    let next_tree=trees.path().join("human-next");
    git(root,&["worktree","add","-b","t/T-human-active",tree.to_str().unwrap(),"main"]);
    git(root,&["worktree","add","-b","t/T-human-next",next_tree.to_str().unwrap(),"main"]);
    let claimed=fixture.run(&tree,&["task","claim","T-human-active"]);
    let run=claimed["run"]["binding"]["id"].as_str().unwrap();
    let pause_id=uuid::Uuid::now_v7().to_string();
    let pause=json!({"request_id":pause_id,"scope":"all"});
    let paused=fixture.request(&endpoint,&format!("/projects/{handle}/pause"),"cli",pause.clone());
    assert_eq!(paused.0,200,"{:?}",paused);
    let state=fixture.get(&endpoint,&format!("/projects/{handle}/state"));
    assert_eq!(state["projection"]["Run"][run]["state"],"active");
    assert_eq!(state["projection"]["Task"]["T-human-active"]["state"],"running");
    let blocked=fixture.request(&endpoint,&format!("/projects/{handle}/tasks/T-human-next/claim"),"cli",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":next_tree}));
    assert_eq!(blocked.0,400,"{:?}",blocked);
    assert!(blocked.1["error"].as_str().unwrap().contains("暂停"));
    let context=json!({"cwd":tree,"run":run});
    let queued=fixture.request(&endpoint,&format!("/projects/{handle}/messages"),"cli",context.clone());
    assert_eq!(queued.0,200,"{:?}",queued);
    assert_eq!(queued.1["messages"].as_array().unwrap().len(),1);
    assert_eq!(queued.1["messages"][0]["actor"],"process:towermd-cli");
    assert!(queued.1["messages"][0]["text"].as_str().unwrap().contains("检查点"));
    fixture.run(root,&["daemon","stop"]);
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let opened=fixture.post(&endpoint,"/projects/open",json!({"path":root}));
    let handle=opened["handle"].as_str().unwrap();
    let state=fixture.get(&endpoint,&format!("/projects/{handle}/state"));
    assert_eq!(state["projection"]["tasks_paused"],true);
    assert_eq!(state["projection"]["Task"]["T-human-active"]["origin"],"app_human");
    assert_eq!(fixture.request(&endpoint,&format!("/projects/{handle}/messages"),"cli",context.clone()).1["messages"],queued.1["messages"]);
    let ack=json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"message":queued.1["messages"][0]["message"]});
    assert_eq!(fixture.request(&endpoint,&format!("/projects/{handle}/messages/ack"),"cli",ack).0,200);
    let requested=fixture.request(&endpoint,&format!("/projects/{handle}/resume-request"),"cli",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"pause_event":pause_id,"scope":"all"}));
    assert_eq!(requested.0,200,"{:?}",requested);
    assert_eq!(requested.1["control"]["paused"],true);
    let resume=json!({"request_id":uuid::Uuid::now_v7().to_string(),"pause_event":pause_id,"scope":"all"});
    assert_eq!(fixture.request(&endpoint,&format!("/projects/{handle}/resume"),"cli",resume.clone()).0,409);
    let resumed=fixture.post(&endpoint,&format!("/projects/{handle}/resume"),resume);
    assert_eq!(resumed["control"]["paused"],false);
    let details=fixture.get(&endpoint,&format!("/projects/{handle}/runs/{run}/events"));
    assert!(details["events"].as_array().unwrap().iter().any(|event|event["kind"]=="project.paused"));
    assert!(details["messages"].as_array().unwrap().iter().any(|message|message["text"].as_str().is_some_and(|text|text.contains("解除项目暂停"))));
    assert_eq!(fixture.post(&endpoint,&format!("/projects/{handle}/tasks/human"),request)["event"],added["event"]);
    golden::record(root, "app_pause");
}

fn now_ms()->i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}
fn at(offset_ms:i64)->String {
    towermd_core::tower_api::timestamp((now_ms()+offset_ms) as u64).unwrap()
}

#[test]
fn a_temporary_rule_comes_only_from_the_app_reaches_covered_runs_and_lapses_at_its_expiry() {
    let fixture=Fixture::new();let draft=fixture.publish();let root=fixture.root.path();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let opened=fixture.post(&endpoint,"/projects/open",json!({"path":root}));
    let handle=opened["handle"].as_str().unwrap();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let task=fixture.task("T-rule",module,path);
    fixture.run(root,&["task","add",task.to_str().unwrap()]);
    let route=format!("/projects/{handle}/temp-rules");
    let body=|text:&str,scope:Value,until:String|json!({"request_id":uuid::Uuid::now_v7().to_string(),"text":text,"scope":scope,"until":until});
    let lasting=body("今晚不要改依赖版本",json!({"kind":"module","module":module}),at(86_400_000));
    assert_eq!(fixture.request(&endpoint,&route,"cli",lasting.clone()).0,409,"the CLI cannot add a rule");
    for (scope,until,code) in [
        (json!({"kind":"global"}),at(-1_000),"usage"),
        (json!({"kind":"global"}),at(31*86_400_000),"usage"),
        (json!({"kind":"global"}),"tonight".to_owned(),"usage"),
        (json!({"kind":"task","task":"T-missing"}),at(60_000),"not_found"),
        (json!({"kind":"module","module":"no-such-module"}),at(60_000),"not_found"),
    ] {
        let response=fixture.request(&endpoint,&route,"app",body("x",scope.clone(),until.clone()));
        assert_eq!((response.0,response.1["code"].as_str()),(400,Some(code)),"{scope} {until}: {response:?}");
    }
    let added=fixture.request(&endpoint,&route,"app",lasting.clone());
    assert_eq!(added.0,200,"{added:?}");
    assert_eq!(added.1["rule"]["added"]["actor"],"human:local");
    assert_eq!(fixture.request(&endpoint,&route,"app",lasting.clone()).1["event"],added.1["event"],"a retry replays");
    let mut changed=lasting.clone();changed["text"]=json!("别的");
    assert_eq!(fixture.request(&endpoint,&route,"app",changed).0,400);
    let short_until=now_ms()+20_000;
    let short=body("二十秒内先别合并",json!({"kind":"task","task":"T-rule"}),at(20_000));
    assert_eq!(fixture.request(&endpoint,&route,"app",short.clone()).0,200);
    let shown=fixture.run(root,&["task","show","T-rule"]);
    assert_eq!(shown["temp_rules"].as_array().unwrap().len(),2,"{shown}");
    assert_eq!(shown["temp_rules"][0]["set_by"],"human:local");
    let trees=tempfile::tempdir().unwrap();let tree=trees.path().join("rule");
    git(root,&["worktree","add","-b","t/T-rule",tree.to_str().unwrap(),"main"]);
    fixture.run(&tree,&["task","claim","T-rule"]);
    let rules=|output:&Output|envelope::parse(output)["delivered"].as_array().unwrap().iter()
        .filter(|item|item["message"]["kind"]=="rule").map(|item|item["text"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
    let first=fixture.output(&tree,&["status"]);
    assert_eq!(rules(&first).len(),2,"{}",String::from_utf8_lossy(&first.stdout));
    assert!(rules(&first).iter().any(|text|text.contains("今晚不要改依赖版本")));
    // A rule is repeated on every call while it is in force; it is never acknowledged.
    assert_eq!(rules(&fixture.output(&tree,&["status"])).len(),2);
    std::thread::sleep(Duration::from_millis((short_until-now_ms()+200).max(0) as u64));
    let later=rules(&fixture.output(&tree,&["status"]));
    assert_eq!(later.len(),1,"the short rule lapsed at its expiry: {later:?}");
    let short_id=format!("TR-{}",short["request_id"].as_str().unwrap());
    let deadline=Instant::now()+Duration::from_secs(30);
    loop {
        let state=fixture.get(&endpoint,&format!("/projects/{handle}/state"));
        let rule=&state["projection"]["TempRule"][&short_id];
        if rule["expired"].is_object() {
            assert_eq!(rule["expired"]["actor"],"process:towermd");
            assert!(state["projection"]["TempRule"][format!("TR-{}",lasting["request_id"].as_str().unwrap())]["expired"].is_null());
            break;
        }
        assert!(Instant::now()<deadline,"maintenance never recorded the expiry: {rule}");
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[test]
fn a_delegate_marks_a_delivered_prompt_handled_and_the_app_cannot_sign_for_it() {
    let fixture=Fixture::new();let draft=fixture.publish();let root=fixture.root.path();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let opened=fixture.post(&endpoint,"/projects/open",json!({"path":root}));
    let handle=opened["handle"].as_str().unwrap();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let task=fixture.task("T-handled",module,path);
    fixture.run(root,&["task","add",task.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();let tree=trees.path().join("handled");
    git(root,&["worktree","add","-b","t/T-handled",tree.to_str().unwrap(),"main"]);
    let claimed=fixture.run(&tree,&["task","claim","T-handled"]);
    let run=claimed["run"]["binding"]["id"].as_str().unwrap();
    let request=uuid::Uuid::now_v7().to_string();
    let sent=fixture.request(&endpoint,&format!("/projects/{handle}/prompts"),"app",
        json!({"request_id":request,"scope":{"kind":"task","task":"T-handled"},"text":"Keep the old API"}));
    assert_eq!(sent.0,200,"{sent:?}");
    let prompt=format!("P-{request}");
    let route=format!("/projects/{handle}/messages/handled");
    let body=json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"prompt":prompt,"note":"Kept it"});
    let early=fixture.request(&endpoint,&route,"cli",body.clone());
    assert_eq!(early.0,400,"{early:?}");assert!(early.1["error"].as_str().unwrap().contains("尚未送达"));
    assert_eq!(fixture.request(&endpoint,&route,"app",body.clone()).0,409);
    // Delivery is recorded only after `status` wrote its envelope.
    let status=fixture.output(&tree,&["status"]);
    assert!(status.status.success());
    assert_eq!(envelope::parse(&status)["delivered"][0]["message"]["id"],json!(prompt));
    let done=fixture.run(&tree,&["handled",&prompt,"--note","Kept the old API"]);
    assert_eq!(done["status"],"accepted");
    let again=fixture.run(&tree,&["handled",&prompt]);
    assert_eq!(again["status"],"already_handled");assert_eq!(again["event"],done["event"]);
    let mut changed=body.clone();changed["request_id"]=done["request_id"].clone();
    assert_eq!(fixture.request(&endpoint,&route,"cli",changed).0,400);
    let blank=fixture.output(&tree,&["handled",&prompt,"--note"," "]);
    assert_eq!(envelope::code(&blank),"usage");
    let state=fixture.get(&endpoint,&format!("/projects/{handle}/state"));
    let handled=state["projection"]["PromptHandled"].as_object().unwrap();
    assert_eq!(handled.len(),1);
    let receipt=handled.values().next().unwrap();
    assert_eq!(receipt["note"],"Kept the old API");assert_eq!(receipt["delivery"]["run"],run);
    let details=fixture.get(&endpoint,&format!("/projects/{handle}/runs/{run}/events"));
    assert!(details["events"].as_array().unwrap().iter().any(|event|event["kind"]=="prompt.handled" && event["actor"]=="process:towermd-cli"));
}

#[test]
fn cli_relay_answers_are_bound_temporary_durable_and_delivered_only_after_output() {
    let fixture=Fixture::new();let draft=fixture.publish();let root=fixture.root.path();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let opened=fixture.post(&endpoint,"/projects/open",json!({"path":root}));
    let handle=opened["handle"].as_str().unwrap();
    let trees=tempfile::tempdir().unwrap();let mut bindings=vec![];
    for (index,id) in ["T-relay","T-other"].into_iter().enumerate() {
        let module=&draft["review"]["modules"][index];
        let task=fixture.task(id,module["id"].as_str().unwrap(),module["files"][0].as_str().unwrap());
        fixture.run(root,&["task","add",task.to_str().unwrap()]);
        let tree=trees.path().join(id);
        git(root,&["worktree","add","-b",&format!("t/{id}"),tree.to_str().unwrap(),"main"]);
        let claimed=fixture.run(&tree,&["task","claim",id]);
        bindings.push((tree,claimed["run"]["binding"]["id"].as_str().unwrap().to_owned()));
    }
    let (tree,run)=&bindings[0];let (other_tree,other_run)=&bindings[1];
    let asked=fixture.request(&endpoint,&format!("/projects/{handle}/ask"),"cli",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"text":"Which output format?"}));
    assert_eq!(asked.0,200,"{asked:?}");let hold=asked.1["hold"].as_str().unwrap();
    let id=uuid::Uuid::now_v7().to_string();
    let body=json!({"request_id":id,"cwd":tree,"run":run,"hold":hold,"text":"JSON for this task",
        "self_reported":{"name":"test-harness","version":"1"}});
    let route=format!("/projects/{handle}/answers/relay");
    assert_eq!(fixture.request(&endpoint,&route,"app",body.clone()).0,409);
    let mut foreign=body.clone();foreign["cwd"]=json!(other_tree);foreign["run"]=json!(other_run);
    let denied=fixture.request(&endpoint,&route,"cli",foreign);
    assert_eq!(denied.0,400,"{denied:?}");assert!(denied.1["error"].as_str().unwrap().contains("当前运行"));
    let accepted=fixture.request(&endpoint,&route,"cli",body.clone());assert_eq!(accepted.0,200,"{accepted:?}");
    assert!(accepted.1["proposal"].is_null());
    assert_eq!(fixture.request(&endpoint,&route,"cli",body.clone()).1,accepted.1);
    let mut changed=body.clone();changed["self_reported"]["version"]=json!("2");
    assert_eq!(fixture.request(&endpoint,&route,"cli",changed).0,400);
    let context=json!({"cwd":tree,"run":run});
    let queued=fixture.request(&endpoint,&format!("/projects/{handle}/messages"),"cli",context.clone());
    assert_eq!(queued.1["messages"].as_array().unwrap().len(),1);
    assert_eq!(queued.1["messages"][0]["actor"],"process:towermd-cli");
    assert_eq!(fixture.request(&endpoint,&format!("/projects/{handle}/messages"),"cli",context.clone()).1,queued.1);
    fixture.run(root,&["daemon","stop"]);
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let opened=fixture.post(&endpoint,"/projects/open",json!({"path":root}));let handle=opened["handle"].as_str().unwrap();
    let route=format!("/projects/{handle}/answers/relay");
    assert_eq!(fixture.request(&endpoint,&route,"cli",body.clone()).1,accepted.1);
    assert_eq!(fixture.request(&endpoint,&format!("/projects/{handle}/messages"),"cli",context.clone()).1["messages"],queued.1["messages"]);
    let state=fixture.get(&endpoint,&format!("/projects/{handle}/state"));
    assert_eq!(state["projection"]["Answer"][hold]["actor"],"process:towermd-cli");
    assert_eq!(state["projection"]["Answer"][hold]["input"]["remember_as"]["kind"],"temporary");
    for group in ["QuestionDecision","QuestionRule","QuestionProposal"] {
        assert!(state["projection"][group].as_object().unwrap().is_empty(),"{group}");
    }
    // The same question is not automatically answered by a temporary relay.
    let second=fixture.request(&endpoint,&format!("/projects/{handle}/ask"),"cli",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"text":"Which output format?"}));
    assert_eq!(second.0,200,"{second:?}");assert_eq!(second.1["wait"]["status"],"pending");
    let second_hold=second.1["hold"].as_str().unwrap();let second_id=uuid::Uuid::now_v7().to_string();
    // Explicit --run deliberately overrides the current worktree, as FR-113
    // requires, but the daemon still binds the answer to the question's run.
    let args=["--run",run,"answer",second_hold,"CSV this time","--relayed","--request-id",&second_id,
        "--harness","test-harness","--harness-version","1"];
    let output=fixture.output(root,&args);
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    envelope::data_checked(&args,&output);
    let envelope=envelope::parse(&output);
    assert_eq!(envelope["data"]["event"],second_id);
    assert!(envelope["delivered"].as_array().unwrap().iter().any(|item|item["text"]=="CSV this time"&&item["actor"]=="process:towermd-cli"));
    assert!(fixture.request(&endpoint,&format!("/projects/{handle}/messages"),"cli",context.clone()).1["messages"].as_array().unwrap().is_empty());
    let details=fixture.get(&endpoint,&format!("/projects/{handle}/runs/{run}/events"));
    assert_eq!(details["events"].as_array().unwrap().iter().filter(|event|event["eid"]==id).count(),1);
    assert!(details["messages"].as_array().unwrap().iter().any(|message|message["kind"]=="answer"&&message["actor"]=="process:towermd-cli"&&message["text"]=="CSV this time"));
    let slow=fixture.request(&endpoint,&format!("/projects/{handle}/ask"),"cli",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"text":"Change module boundary?","targets":[draft["review"]["modules"][0]["id"]]}));
    assert_eq!(slow.0,200,"{slow:?}");
    let rejected=fixture.request(&endpoint,&route,"cli",json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"hold":slow.1["hold"],"text":"approved"}));
    assert_eq!(rejected.0,400,"{rejected:?}");assert!(rejected.1["error"].as_str().unwrap().contains("App"));
    assert!(!fixture.output(root,&["answer",hold,"JSON","--remember-as","module_decision"]).status.success());
    assert_eq!(envelope::code(&fixture.output(root,&["answer",hold,"JSON","--remember-as","module_decision"])),"usage");
    let released=fixture.request(&endpoint,&format!("/projects/{handle}/task-release"),"cli",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run}));
    assert_eq!(released.0,200,"{released:?}");
    assert_eq!(fixture.request(&endpoint,&route,"cli",body).1,accepted.1,"confirmed relay retries retain their original event after release");
}

#[test]
fn a_parked_question_ends_the_turn_instead_of_offering_another_wait() {
    let fixture=Fixture::new();let draft=fixture.publish();let root=fixture.root.path();
    let module=&draft["review"]["modules"][0];
    let task=fixture.task("T-parked",module["id"].as_str().unwrap(),module["files"][0].as_str().unwrap());
    fixture.run(root,&["task","add",task.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();let tree=trees.path().join("parked");
    git(root,&["worktree","add","-b","t/T-parked",tree.to_str().unwrap(),"main"]);
    fixture.run(&tree,&["task","claim","T-parked"]);
    let asked=fixture.run(&tree,&["ask","Which output format?","--parked"]);
    assert_eq!(asked["wait"]["status"],"deferred","{asked}");
    let hold=asked["hold"].as_str().unwrap();
    let output=fixture.output(&tree,&["wait",hold]);
    let envelope=envelope::parse(&output);
    assert_eq!(output.status.code(),Some(5),"{envelope}");
    assert_eq!(envelope["error"]["code"],"deferred");
    assert_eq!(envelope["data"]["status"],"deferred");
    // Waiting again would return at once: no `towermd wait` and no safe retry.
    assert!(envelope["next"].as_array().unwrap().iter().all(|next|!next["cmd"].as_str().unwrap().contains("towermd wait")),"{envelope}");
    assert!(envelope["error"].get("retry").is_none(),"{envelope}");
    assert!(envelope["error"]["hint"].as_str().unwrap().contains("delivered"),"{envelope}");
}

/// FR-186: a module decision may carry a review day. From that day on it is
/// an attention item; only a person in the App keeps it (to a later day) or
/// voids it, and a voided decision is never reused for a new question.
#[test]
fn a_decision_comes_back_on_its_review_day_and_a_voided_one_is_not_reused() {
    let fixture=Fixture::new();
    let draft=fixture.publish();
    let root=fixture.root.path();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let handle=fixture.post(&endpoint,"/projects/open",json!({"path":root}))["handle"].as_str().unwrap().to_owned();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    let task=fixture.task("T-review",module,path);
    fixture.run(root,&["task","add",task.to_str().unwrap()]);
    let trees=tempfile::tempdir().unwrap();
    let tree=trees.path().join("review");
    git(root,&["worktree","add","-b","t/T-review",tree.to_str().unwrap(),"main"]);
    let run=fixture.run(&tree,&["task","claim","T-review"])["run"]["binding"]["id"].as_str().unwrap().to_owned();
    let ask_path=format!("/projects/{handle}/ask");
    let ask=|text:&str| fixture.request(&endpoint,&ask_path,"cli",json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"text":text,"options":["JSON","CSV"]}));
    let (status,asked)=ask("导出用什么格式？");
    assert_eq!(status,200,"{asked}");
    let hold=asked["hold"].as_str().unwrap().to_owned();
    let answer_path=format!("/projects/{handle}/answers");
    let (status,refused)=fixture.request(&endpoint,&answer_path,"app",json!({"request_id":uuid::Uuid::now_v7().to_string(),"hold":hold,"text":"JSON","remember_as":"temporary","review_on":"2026-01-01"}));
    assert_eq!(status,400,"only a module decision is reviewed: {refused}");
    let (status,refused)=fixture.request(&endpoint,&answer_path,"app",json!({"request_id":uuid::Uuid::now_v7().to_string(),"hold":hold,"text":"JSON","remember_as":"module_decision","review_on":"下个月"}));
    assert_eq!(status,400,"{refused}");
    let answer_id=uuid::Uuid::now_v7().to_string();
    let (status,answered)=fixture.request(&endpoint,&answer_path,"app",json!({"request_id":answer_id,"hold":hold,"text":"JSON","remember_as":"module_decision","assumption":"下游只读 JSON","review_on":"2026-01-01"}));
    assert_eq!(status,200,"{answered}");
    let decision=format!("D-{answer_id}");
    let state=|| fixture.get(&endpoint,&format!("/projects/{handle}/state"));
    let card=|state:&Value| state["attention_list"]["items"].as_array().unwrap().iter().find(|item| item["id"]==format!("decision-review:{decision}")).cloned();
    let due=card(&state()).expect("the review day passed");
    assert_eq!((due["label"].as_str(),due["class"].as_u64(),due["title"].as_str()),(Some("复查决策"),Some(4),Some("导出用什么格式？")));
    assert_eq!(due["detail"],"2026-01-01 到期复查；当时的选项：JSON、CSV；当时定为“JSON”，理由：下游只读 JSON");

    let review_path=format!("/projects/{handle}/decisions/{decision}/review");
    let keep=json!({"request_id":uuid::Uuid::now_v7().to_string(),"outcome":"keep","review_on":"2099-01-01"});
    assert_eq!(fixture.request(&endpoint,&review_path,"cli",keep.clone()).0,409,"the CLI never reviews");
    let (status,_)=fixture.request(&endpoint,&review_path,"app",json!({"request_id":uuid::Uuid::now_v7().to_string(),"outcome":"keep","review_on":"2026-01-02"}));
    assert_eq!(status,400,"the next review is after today");
    let (status,_)=fixture.request(&endpoint,&review_path,"app",json!({"request_id":uuid::Uuid::now_v7().to_string(),"outcome":"void","review_on":"2099-01-01"}));
    assert_eq!(status,400);
    let (status,kept)=fixture.request(&endpoint,&review_path,"app",keep.clone());
    assert_eq!(status,200,"{kept}");
    assert_eq!((kept["review"]["review_on"].as_str(),kept["review"]["kept"]["actor"].as_str()),(Some("2099-01-01"),Some("human:local")));
    assert_eq!(fixture.request(&endpoint,&review_path,"app",keep).1["seq"],kept["seq"],"a retry is the same event");
    assert!(card(&state()).is_none(),"kept until a later day");
    let missing=format!("/projects/{handle}/decisions/D-none/review");
    assert_eq!(fixture.request(&endpoint,&missing,"app",json!({"request_id":uuid::Uuid::now_v7().to_string(),"outcome":"void"})).0,400);

    // Before voiding, the same question reuses the decision; after, it asks again.
    let (_,reused)=ask("导出用什么格式？");
    assert_eq!(reused["wait"]["status"],"answered","{reused}");
    let (status,voided)=fixture.request(&endpoint,&review_path,"app",json!({"request_id":uuid::Uuid::now_v7().to_string(),"outcome":"void"}));
    assert_eq!(status,200,"{voided}");
    assert_eq!(voided["review"]["voided"]["actor"],"human:local");
    let (_,fresh)=ask("导出用什么格式？");
    assert_eq!(fresh["wait"]["status"],"pending","a voided decision is never cited: {fresh}");
}

#[test]
fn an_answer_recorded_as_a_process_rule_is_registered_by_the_person_and_rides_on_the_module_task_cards() {
    let fixture=Fixture::new();
    let draft=fixture.publish();
    let root=fixture.root.path();
    let endpoint=fixture.run(root,&["daemon","ensure"]);
    let handle=fixture.post(&endpoint,"/projects/open",json!({"path":root}))["handle"].as_str().unwrap().to_owned();
    let module=draft["review"]["modules"][0]["id"].as_str().unwrap();
    let path=draft["review"]["modules"][0]["files"][0].as_str().unwrap();
    for id in ["T-rule","T-rule-next"] {
        let task=fixture.task(id,module,path);
        fixture.run(root,&["task","add",task.to_str().unwrap()]);
    }
    let trees=tempfile::tempdir().unwrap();
    let tree=trees.path().join("rule");
    git(root,&["worktree","add","-b","t/T-rule",tree.to_str().unwrap(),"main"]);
    let run=fixture.run(&tree,&["task","claim","T-rule"])["run"]["binding"]["id"].as_str().unwrap().to_owned();
    let (status,asked)=fixture.request(&endpoint,&format!("/projects/{handle}/ask"),"cli",
        json!({"request_id":uuid::Uuid::now_v7().to_string(),"cwd":tree,"run":run,"text":"提交前要跑格式检查吗？"}));
    assert_eq!(status,200,"{asked}");
    let hold=asked["hold"].as_str().unwrap();
    let answer_path=format!("/projects/{handle}/answers");
    let answer_id=uuid::Uuid::now_v7().to_string();
    let answer=json!({"request_id":answer_id,"hold":hold,"text":"提交前先跑 make fmt","remember_as":"process_rule"});
    assert_eq!(fixture.request(&endpoint,&answer_path,"cli",answer.clone()).0,409,"only the person registers a rule");
    let mut reviewed=answer.clone();reviewed["review_on"]=json!("2099-01-01");
    assert_eq!(fixture.request(&endpoint,&answer_path,"app",reviewed).0,400,"only a module decision has a review day");
    let (status,answered)=fixture.request(&endpoint,&answer_path,"app",answer.clone());
    assert_eq!(status,200,"{answered}");
    assert_eq!(fixture.request(&endpoint,&answer_path,"app",answer).1,answered,"a retry is the same event");
    let rule=format!("RULE-{answer_id}");
    let state=fixture.get(&endpoint,&format!("/projects/{handle}/state"));
    let registered=&state["projection"]["QuestionRule"][&rule];
    assert_eq!((registered["module"].as_str(),registered["registered_by"].as_str()),(Some(module),Some("human:local")),"{state}");
    assert_eq!(state["projection"]["Answer"][hold]["input"]["remember_as"]["kind"],"process_rule");
    // Every task card of the module carries it, not only the asking task's.
    let card=fixture.get(&endpoint,&format!("/projects/{handle}/tasks/T-rule-next"))["task_card"].clone();
    assert!(card.to_string().contains("流程规则：提交前先跑 make fmt"),"{card}");
}
