//! Method dispatch and the four tools. `initialize` and `tools/list` need no
//! daemon, so a harness always learns the tools; only `tools/call` reaches
//! it, through `Daemon`. Every wait is bounded by the daemon: a question
//! defers, a review stays open, a permission expires into a denial. Nothing
//! here passes, approves or allows on its own.
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use towermd_core::question_model::ASK_WAIT_MS;
use towermd_devkit::gateway::{ASK, NOTE, PERMISSION, REVIEW};
use towermd_devkit::harness::Adapter;

use super::protocol;

/// One POST to this run's project on the daemon, as the gateway.
pub trait Daemon {
    fn post(&self, suffix: &str, body: &Value, timeout: Duration) -> Result<Value, String>;
}

pub struct Session<'a> {
    pub run: String,
    pub worktree: PathBuf,
    pub adapter: &'a dyn Adapter,
    pub daemon: &'a dyn Daemon,
    /// How long a review is waited on before the turn should end.
    pub review_ms: u64,
    /// Messages handed over in the last reply, acknowledged once it is written.
    pub delivered: RefCell<Vec<Value>>,
}

const PARKED: &str = "结束本回合：运行报告的 status 写 needs_input，并说明在等什么；回答会在运行继续时送达，不要再次等待。";

fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Each wait call is at most one ask-wait; the HTTP timeout leaves slack.
fn wait_timeout() -> Duration {
    Duration::from_millis(ASK_WAIT_MS + 4_000)
}

fn strings(args: &Value, key: &str) -> Value {
    args.get(key).filter(|value| value.is_array()).cloned().unwrap_or(json!([]))
}

fn text<'v>(args: &'v Value, key: &str) -> Result<&'v str, String> {
    args.get(key).and_then(Value::as_str).filter(|text| !text.trim().is_empty()).ok_or_else(|| format!("缺少 {key}"))
}

impl Session<'_> {
    fn ask(&self, args: &Value) -> Result<Value, String> {
        let body = json!({"request_id": id(), "cwd": self.worktree, "run": self.run, "text": text(args, "question")?,
            "options": strings(args, "options"), "recommended": args.get("recommended"), "context": strings(args, "context"),
            "targets": [], "mode": "live"});
        let asked = self.daemon.post("/ask", &body, Duration::from_secs(30))?;
        let hold = asked["hold"].as_str().ok_or("守护进程未返回问题 ID")?.to_owned();
        let mut value = asked.get("wait").cloned().unwrap_or(asked);
        while value["status"] == "pending" {
            value = self.daemon.post("/wait", &json!({"cwd": self.worktree, "run": self.run, "hold": hold, "wait_ms": ASK_WAIT_MS}), wait_timeout())?;
        }
        value["hold"] = json!(hold);
        if value["status"] == "deferred" {
            value["instruction"] = json!(PARKED);
        }
        Ok(value)
    }

    fn review(&self, args: &Value) -> Result<Value, String> {
        let body = json!({"request_id": id(), "cwd": self.worktree, "run": self.run, "stage": text(args, "stage")?,
            "summary": text(args, "summary")?, "artifacts": strings(args, "artifacts")});
        let started = Instant::now();
        let mut value = self.daemon.post("/reviews", &body, Duration::from_secs(30))?;
        let review = value["review"].as_str().ok_or("守护进程未返回评审 ID")?.to_owned();
        while value["status"] == "pending" {
            let left = self.review_ms.saturating_sub(started.elapsed().as_millis() as u64);
            value = self.daemon.post("/reviews/wait", &json!({"cwd": self.worktree, "run": self.run, "review": review,
                "wait_ms": left.min(ASK_WAIT_MS)}), wait_timeout())?;
            if left == 0 {
                break;
            }
        }
        if value["status"] == "pending" {
            value["instruction"] = json!(PARKED);
        }
        Ok(value)
    }

    /// Always a decision in the harness's protocol: anything short of a
    /// person's allow is a denial.
    fn permission(&self, args: &Value) -> Value {
        let Some(ask) = self.adapter.permission_request(args) else {
            return json!({"allow": false, "message": "无法识别授权请求，按拒绝处理"});
        };
        let decided = (|| -> Result<Value, String> {
            let asked = self.daemon.post("/permissions", &json!({"request_id": id(), "run": self.run, "tool": ask.tool, "input": ask.input}),
                Duration::from_secs(30))?;
            let permission = asked["permission"].as_str().ok_or("守护进程未返回授权请求 ID")?.to_owned();
            let mut value = asked;
            while value["status"] == "pending" {
                value = self.daemon.post("/permissions/wait", &json!({"run": self.run, "permission": permission, "wait_ms": ASK_WAIT_MS}), wait_timeout())?;
            }
            Ok(value)
        })();
        match decided {
            Ok(value) if value["status"] == "allowed" => self.adapter.permission_reply(&ask, true, value["note"].as_str().unwrap_or("")),
            Ok(value) => {
                let why = match value["closed"].as_str() {
                    Some("expired") => "10 分钟内无人回答，按拒绝处理",
                    Some("run_ended") => "运行已结束",
                    _ => "人拒绝了本次使用",
                };
                self.adapter.permission_reply(&ask, false, value["note"].as_str().unwrap_or(why))
            }
            Err(error) => self.adapter.permission_reply(&ask, false, &format!("授权请求未送达，按拒绝处理：{error}")),
        }
    }

    fn note(&self, args: &Value) -> Result<Value, String> {
        let mut data = json!({"state": "in_progress", "summary": text(args, "summary")?});
        if let Some(next) = args.get("next").and_then(Value::as_str).filter(|next| !next.trim().is_empty()) {
            data["next"] = json!(next);
        }
        self.daemon.post("/tower", &json!({"request_id": id(), "cwd": self.worktree, "run": self.run, "verb": "report", "data": data}),
            Duration::from_secs(30))
    }

    /// Answers and interjections waiting for this run ride on the result of
    /// any tool call but a permission (FR-60), in `delivered`. A failed
    /// fetch leaves them for the next call.
    fn deliver(&self, value: &mut Value) {
        let Ok(fetched) = self.daemon.post("/messages", &json!({"cwd": self.worktree, "run": self.run}), Duration::from_secs(30)) else {
            return;
        };
        if let Some(review) = fetched.get("needs_review").filter(|review| !review.is_null()) {
            value["needs_review"] = review.clone();
        }
        let messages = fetched["messages"].as_array().cloned().unwrap_or_default();
        if messages.is_empty() {
            return;
        }
        let mut held = self.delivered.borrow_mut();
        for message in &messages {
            if !held.iter().any(|known| known["message"] == message["message"] && known["run"] == message["run"]) {
                held.push(message.clone());
            }
        }
        value["delivered"] = json!(messages);
    }

    /// Records delivery of what the last reply carried, after it reached the
    /// harness. Report requests repeat until answered and rules until they
    /// expire; they are never acknowledged. A failed ack means delivering again.
    pub fn acknowledge(&self) {
        for message in self.delivered.borrow_mut().drain(..) {
            if matches!(message["message"]["kind"].as_str(), Some("request_report" | "rule")) {
                continue;
            }
            let body = json!({"request_id": id(), "cwd": self.worktree, "run": message["run"], "message": message["message"]});
            if let Err(error) = self.daemon.post("/messages/ack", &body, Duration::from_secs(30)) {
                eprintln!("送达确认失败，消息会再次送达：{error}");
            }
        }
    }
}

pub fn handle(session: &Session, message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone(); // A notification gets no reply.
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(json!({}));
    Some(match method {
        "initialize" => json!({"jsonrpc": "2.0", "id": id,
            "result": protocol::initialize_result(params.get("protocolVersion").and_then(Value::as_str))}),
        "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": protocol::tool_definitions()}),
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "tools/call" => {
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let mut result = match name {
                ASK => session.ask(&args),
                REVIEW => session.review(&args),
                PERMISSION => Ok(session.permission(&args)),
                NOTE => session.note(&args),
                other => Err(format!("未知工具 {other}")),
            };
            // A permission reply is the harness's protocol: nothing rides on it.
            if let Ok(value) = result.as_mut() {
                if name != PERMISSION && value.is_object() {
                    session.deliver(value);
                }
            }
            match result {
                Ok(value) => protocol::tool_ok(&id, &value),
                Err(error) => protocol::tool_error(&id, &error),
            }
        }
        other => protocol::error(&id, -32601, &format!("no such method: {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers each POST from a script and records what was sent.
    struct Fake {
        replies: RefCell<Vec<Result<Value, String>>>,
        sent: RefCell<Vec<(String, Value)>>,
    }
    impl Fake {
        fn new(replies: Vec<Result<Value, String>>) -> Self {
            Self { replies: RefCell::new(replies.into_iter().rev().collect()), sent: RefCell::new(vec![]) }
        }
        fn paths(&self) -> Vec<String> {
            self.sent.borrow().iter().map(|(path, _)| path.clone()).collect()
        }
    }
    impl Daemon for Fake {
        fn post(&self, suffix: &str, body: &Value, _: Duration) -> Result<Value, String> {
            self.sent.borrow_mut().push((suffix.to_owned(), body.clone()));
            self.replies.borrow_mut().pop().unwrap_or_else(|| Err("脚本已用完".into()))
        }
    }

    /// A harness-neutral adapter: prompts are `{tool, input}`, answers the
    /// default `{allow, message}`.
    struct Neutral;
    impl Adapter for Neutral {
        fn id(&self) -> &'static str { "neutral" }
        fn program(&self) -> &'static str { "neutral" }
        fn capabilities(&self) -> towermd_devkit::harness::Capabilities { unimplemented!() }
        fn launch(&self, _: &towermd_devkit::harness::RunSpec) -> towermd_devkit::harness::Launch { unimplemented!() }
        fn parse(&self, _: &str) -> Vec<towermd_devkit::harness::Signal> { vec![] }
        fn permission_request(&self, args: &Value) -> Option<towermd_devkit::harness::PermissionAsk> {
            Some(towermd_devkit::harness::PermissionAsk { tool: args.get("tool")?.as_str()?.into(), input: args.get("input")?.clone() })
        }
    }

    fn session(daemon: &Fake) -> Session<'_> {
        Session { run: "R-1".into(), worktree: "/w".into(), adapter: &Neutral, daemon, review_ms: 0, delivered: RefCell::new(vec![]) }
    }

    fn call(session: &Session, name: &str, args: Value) -> Value {
        let reply = handle(session, &json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":args}})).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    fn payload(result: &Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn lists_tools_without_a_daemon_and_ignores_notifications() {
        let fake = Fake::new(vec![]);
        let session = session(&fake);
        assert!(handle(&session, &json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let listed = handle(&session, &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 4);
        assert_eq!(handle(&session, &json!({"jsonrpc":"2.0","id":2,"method":"nope"})).unwrap()["error"]["code"], -32601);
        assert_eq!(call(&session, "towermd_approve", json!({}))["isError"], true);
        assert!(fake.paths().is_empty());
    }

    #[test]
    fn a_question_waits_until_answered_or_deferred() {
        let fake = Fake::new(vec![
            Ok(json!({"hold":"H-1","wait":{"status":"pending"}})),
            Ok(json!({"status":"pending"})),
            Ok(json!({"status":"answered","answer":{"text":"用 a"}})),
        ]);
        let result = call(&session(&fake), ASK, json!({"question":"用哪个库？","options":["a","b"]}));
        assert_eq!((payload(&result)["status"].as_str(), payload(&result)["hold"].as_str()), (Some("answered"), Some("H-1")));
        assert_eq!(fake.paths(), ["/ask", "/wait", "/wait", "/messages"]);
        let asked = &fake.sent.borrow()[0].1;
        assert_eq!((asked["run"].as_str(), asked["mode"].as_str(), asked["text"].as_str()), (Some("R-1"), Some("live"), Some("用哪个库？")));

        let fake = Fake::new(vec![Ok(json!({"hold":"H-2","wait":{"status":"deferred"}}))]);
        let deferred = payload(&call(&session(&fake), ASK, json!({"question":"q"})));
        assert!(deferred["instruction"].as_str().unwrap().contains("needs_input"), "{deferred}");
        assert_eq!(call(&session(&Fake::new(vec![])), ASK, json!({}))["isError"], true);
    }

    #[test]
    fn a_review_never_passes_by_waiting() {
        let fake = Fake::new(vec![Ok(json!({"review":"V-1","status":"pending"})), Ok(json!({"review":"V-1","status":"pending"}))]);
        let open = payload(&call(&session(&fake), REVIEW, json!({"stage":"plan","summary":"先拆模块"})));
        assert_eq!(open["status"], "pending");
        assert!(open["instruction"].as_str().unwrap().contains("needs_input"));
        assert_eq!(fake.paths(), ["/reviews", "/reviews/wait", "/messages"]);
        let fake = Fake::new(vec![Ok(json!({"review":"V-2","status":"pending"})), Ok(json!({"status":"resolved","verdict":"changes_requested"}))]);
        assert_eq!(payload(&call(&session(&fake), REVIEW, json!({"stage":"code","summary":"s"})))["verdict"], "changes_requested");
    }

    #[test]
    fn only_a_persons_allow_allows_and_everything_else_denies() {
        let args = json!({"tool":"Bash","input":{"command":"rm -rf build"}});
        let fake = Fake::new(vec![Ok(json!({"permission":"K-1","status":"pending"})), Ok(json!({"status":"allowed","decision":"allow"}))]);
        let allowed = payload(&call(&session(&fake), PERMISSION, args.clone()));
        assert_eq!(allowed["allow"], true, "{allowed}");
        let sent = &fake.sent.borrow()[0].1;
        assert_eq!((sent["tool"].as_str(), sent["input"]["command"].as_str(), sent["run"].as_str()), (Some("Bash"), Some("rm -rf build"), Some("R-1")));

        for (replies, why) in [
            (vec![Ok(json!({"permission":"K-2","status":"pending"})), Ok(json!({"status":"denied","closed":"expired"}))], "10 分钟"),
            (vec![Ok(json!({"permission":"K-3","status":"denied","closed":"answered","note":"别删"}))], "别删"),
            (vec![Err("守护进程未运行".into())], "未送达"),
        ] {
            let denied = payload(&call(&session(&Fake::new(replies)), PERMISSION, args.clone()));
            assert_eq!(denied["allow"], false, "{denied}");
            assert!(denied["message"].as_str().unwrap().contains(why), "{denied}");
        }
        let fake = Fake::new(vec![]);
        let unknown = payload(&call(&session(&fake), PERMISSION, json!({"x":1})));
        assert_eq!(unknown["allow"], false);
        assert!(fake.paths().is_empty(), "an unreadable prompt is denied without asking");
    }

    #[test]
    fn a_note_is_an_in_progress_report_not_a_completion() {
        let fake = Fake::new(vec![Ok(json!({"status":"accepted"}))]);
        call(&session(&fake), NOTE, json!({"summary":"接口已拆完","next":"写测试","state":"done"}));
        let (path, body) = fake.sent.borrow()[0].clone();
        assert_eq!((path.as_str(), body["verb"].as_str(), body["data"]["state"].as_str()), ("/tower", Some("report"), Some("in_progress")));
        assert_eq!(body["data"]["next"], "写测试");
    }

    /// FR-60: a running agent hears interjections and answers on its next
    /// tool call; their delivery is recorded only after the reply is out.
    #[test]
    fn waiting_messages_ride_on_the_next_tool_result_and_are_acked_after_it() {
        let interjection = json!({"run":"R-1","message":{"kind":"prompt","id":"P-1"},"text":"先别改接口","actor":"human:local"});
        let report = json!({"run":"R-1","message":{"kind":"request_report","id":"Q-1"},"text":"汇报进度"});
        let fake = Fake::new(vec![
            Ok(json!({"status":"accepted"})),
            Ok(json!({"run":"R-1","messages":[interjection, report],"needs_review":null})),
            Ok(json!({"status":"accepted"})),
        ]);
        let session = session(&fake);
        let noted = payload(&call(&session, NOTE, json!({"summary":"在拆接口"})));
        assert_eq!(noted["delivered"][0]["text"], "先别改接口", "{noted}");
        assert_eq!(noted["delivered"].as_array().unwrap().len(), 2);
        assert!(noted.get("needs_review").is_none());
        assert_eq!(fake.paths(), ["/tower", "/messages"], "nothing is acked before the reply is written");
        session.acknowledge();
        assert_eq!(fake.paths(), ["/tower", "/messages", "/messages/ack"], "a report request is not acked");
        let ack = fake.sent.borrow()[2].1.clone();
        assert_eq!((ack["run"].as_str(), ack["message"]["id"].as_str()), (Some("R-1"), Some("P-1")));
        session.acknowledge();
        assert_eq!(fake.paths().len(), 3, "acked once");
        let fake = Fake::new(vec![Ok(json!({"permission":"K-1","status":"denied","closed":"answered"}))]);
        call(&super::tests::session(&fake), PERMISSION, json!({"tool":"Bash","input":{}}));
        assert_eq!(fake.paths(), ["/permissions"], "nothing rides on a permission reply");
    }
}
