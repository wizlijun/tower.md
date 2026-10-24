//! FR-84/FR-86/FR-88: the daemon's client of a self-deployed remote hub
//! (`hub-worker/`). The remote hub is the project's only sequencer; this
//! client never sequences anything itself. Every network failure becomes
//! `HubError::Offline`, which the runtime turns into an outbox entry for an
//! ordinary event and into an explicit failure for a lease operation.
//!
//! The runtime reads a hub through `HubBackend`. This client serves those
//! reads from a mirror of the retained events and the last hub head, both
//! refreshed by `subscribe`, so scheduling and integration cannot tell it
//! from the local journal.
//!
//! Wire protocol: one WebSocket per project at `<url>/projects/<project_id>`,
//! outbound only. A request is `{"id":n,"op":...}`; the reply is
//! `{"id":n,"ok":...}` or `{"id":n,"err":<HubError>}`. Event `data` and the
//! snapshot checkpoint travel as JSON text (`data_json`, `checkpoint_json`)
//! so the hub stores the producer's bytes without re-encoding them. Messages
//! without the awaited id are ignored.

use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use towermd_core::hub_protocol::*;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// After a failed connect, fail fast for a while instead of stalling every
/// caller for the full connect timeout.
const RETRY_AFTER: Duration = if cfg!(test) {
    Duration::from_millis(50)
} else {
    Duration::from_secs(2)
};

type Reply = Result<Value, HubError>;
struct Job {
    request: Value,
    reply: mpsc::Sender<Reply>,
}

pub struct RemoteHub {
    jobs: mpsc::Sender<Job>,
    /// Retained events after `head.floor_seq`, contiguous up to `last`.
    mirror: Vec<Event>,
    last: u64,
    head: Option<HubHead>,
}

impl RemoteHub {
    /// Connects lazily: constructing never touches the network.
    pub fn new(url: &str, project_id: &str) -> Result<Self, String> {
        if url.starts_with("wss://") {
            return Err(
                "本版本未内置 TLS，无法连接 wss:// hub；当前仅支持可信网络内的 ws://".into(),
            );
        }
        let endpoint = format!("{url}/projects/{}", encode(project_id));
        let (jobs, inbox) = mpsc::channel();
        std::thread::Builder::new()
            .name("towermd-hub-client".into())
            .spawn(move || serve(endpoint, inbox))
            .map_err(|e| format!("无法启动 hub 客户端线程：{e}"))?;
        Ok(Self {
            jobs,
            mirror: Vec::new(),
            last: 0,
            head: None,
        })
    }

    fn call<T: DeserializeOwned>(&self, request: Value) -> Result<T, HubError> {
        let (reply, answer) = mpsc::channel();
        let stopped = || offline("hub 客户端线程已停止");
        self.jobs
            .send(Job { request, reply })
            .map_err(|_| stopped())?;
        let value = answer.recv().map_err(|_| stopped())??;
        serde_json::from_value(value)
            .map_err(|e| HubError::Uncertain(format!("远端 hub 回复无法解析：{e}")))
    }

    /// Pulls everything after the mirror's end. A mirror left behind a
    /// compaction restarts at the hub floor; the runtime then sees
    /// `SnapshotRequired` for any cursor below it.
    fn refresh(&mut self) -> Result<(), HubError> {
        let batch: SubscriptionBatch =
            match self.call(json!({"op": "subscribe", "after": self.last})) {
                Err(HubError::SnapshotRequired { head, .. }) => {
                    self.mirror.clear();
                    self.last = head.floor_seq;
                    self.call(json!({"op": "subscribe", "after": self.last}))?
                }
                other => other?,
            };
        for event in batch.events {
            if event.seq <= self.last {
                continue;
            }
            if event.seq != self.last + 1 {
                return Err(HubError::Uncertain("远端 hub 回复存在序号缺口".into()));
            }
            self.last = event.seq;
            self.mirror.push(event);
        }
        if batch.head.contiguous_seq != self.last {
            return Err(HubError::Uncertain("远端 hub 水位与回复事件不一致".into()));
        }
        self.mirror.retain(|event| event.seq > batch.head.floor_seq);
        self.head = Some(batch.head);
        Ok(())
    }

    /// Takes a receipt into the mirror, then refreshes when the hub is
    /// reachable. A failed refresh leaves a contiguous, merely older mirror.
    fn settle(&mut self, event: Option<&Event>, granted: &[Lease], released: &[Fence]) {
        if let Some(event) = event.filter(|event| event.seq == self.last + 1) {
            self.last = event.seq;
            self.mirror.push(event.clone());
            if let Some(head) = &mut self.head {
                head.contiguous_seq = self.last;
            }
        }
        if let Some(head) = &mut self.head {
            for lease in granted {
                head.epoch_high_water = head.epoch_high_water.max(lease.epoch);
                head.leases.insert(lease.name.clone(), lease.clone());
            }
            for fence in released {
                if head.leases.get(&fence.name).is_some_and(|l| l.fence() == *fence) {
                    head.leases.remove(&fence.name);
                }
            }
        }
        let _ = self.refresh();
    }

    fn lease_call(&mut self, op: &str, request: Value) -> Result<LeaseReceipt, HubError> {
        let receipt: LeaseReceipt = self.call(json!({"op": op, "request": request}))?;
        Ok(receipt)
    }
}

impl HubProtocol for RemoteHub {
    fn append(&mut self, event: NewEvent, _now_ms: u64) -> Result<AppendReceipt, HubError> {
        let data_json = serde_json::to_string(&event.data).map_err(|e| HubError::Invalid(e.to_string()))?;
        let mut body = serde_json::to_value(&event).map_err(|e| HubError::Invalid(e.to_string()))?;
        if let Some(body) = body.as_object_mut() {
            body.remove("data");
        }
        let receipt: AppendReceipt =
            self.call(json!({"op": "append", "event": body, "data_json": data_json}))?;
        match &receipt {
            AppendReceipt::Event(event) => self.settle(Some(event), &[], &[]),
            AppendReceipt::Duplicate { .. } => self.settle(None, &[], &[]),
        }
        Ok(receipt)
    }

    /// The hub's clock, not `now_ms`, decides lease expiry.
    fn subscribe(&mut self, after: u64, _now_ms: u64) -> Result<SubscriptionBatch, HubError> {
        self.refresh()?;
        let head = self.head.clone().expect("refresh sets the head");
        if after < head.floor_seq {
            return self.call(json!({"op": "subscribe", "after": after}));
        }
        if after > self.last {
            return Err(HubError::Invalid(format!(
                "cursor {after} is ahead of hub {}",
                self.last
            )));
        }
        Ok(SubscriptionBatch {
            events: self.events(after),
            head,
        })
    }

    fn lease_acquire(&mut self, request: AcquireLease, _now_ms: u64) -> Result<LeaseReceipt, HubError> {
        let receipt = self.lease_call("lease_acquire", json!(request))?;
        self.settle(Some(&receipt.event), &receipt.leases, &[]);
        Ok(receipt)
    }

    fn lease_renew(&mut self, request: RenewLease, _now_ms: u64) -> Result<LeaseReceipt, HubError> {
        let receipt = self.lease_call("lease_renew", json!(request))?;
        self.settle(Some(&receipt.event), &receipt.leases, &[]);
        Ok(receipt)
    }

    fn lease_release(&mut self, request: ReleaseLease, _now_ms: u64) -> Result<LeaseReceipt, HubError> {
        let fences = request.fences.clone();
        let receipt = self.lease_call("lease_release", json!(request))?;
        self.settle(Some(&receipt.event), &[], &fences);
        Ok(receipt)
    }

    fn snapshot_ack(&mut self, ack: SnapshotAck, checkpoint: Value) -> Result<(), HubError> {
        let checkpoint_json =
            serde_json::to_string(&checkpoint).map_err(|e| HubError::Invalid(e.to_string()))?;
        self.call::<Value>(json!({"op": "snapshot_ack", "ack": ack, "checkpoint_json": checkpoint_json}))?;
        if let Some(head) = &mut self.head {
            head.floor_seq = head.floor_seq.max(ack.seq);
            head.ack = Some(ack.clone());
        }
        self.mirror.retain(|event| event.seq > ack.seq);
        self.settle(None, &[], &[]);
        Ok(())
    }

    fn rebuild(&mut self, _seed: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        Err(HubError::Invalid("远端 hub 不在本机重建".into()))
    }

    fn contiguous_seq(&self) -> u64 {
        self.last
    }
}

impl HubBackend for RemoteHub {
    fn events(&self, after: u64) -> Vec<Event> {
        self.mirror.iter().filter(|event| event.seq > after).cloned().collect()
    }
    /// Expiry is stamped by the hub's clock; hosts are expected to keep
    /// their clocks in sync (NTP), as with any lease.
    fn leases(&self, now: u64) -> BTreeMap<String, Lease> {
        self.head.as_ref().map_or_else(BTreeMap::new, |head| {
            head.leases
                .iter()
                .filter(|(_, lease)| lease.expires_at > now)
                .map(|(name, lease)| (name.clone(), lease.clone()))
                .collect()
        })
    }
    fn epoch_high_water(&self) -> u64 {
        self.head.as_ref().map_or(0, |head| head.epoch_high_water)
    }
    fn acked_snapshot(&self) -> Option<SnapshotAck> {
        self.head.as_ref().and_then(|head| head.ack.clone())
    }
    fn pause_event(&self) -> Option<String> {
        self.head.as_ref().and_then(|head| head.pause_event.clone())
    }
    fn is_remote(&self) -> bool {
        true
    }
}

fn offline(reason: impl Into<String>) -> HubError {
    HubError::Offline(reason.into())
}

/// Percent-encodes everything outside the URL-unreserved set.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The socket lives on its own thread with a current-thread runtime, so the
/// blocking `HubProtocol` calls work from any caller, async or not.
fn serve(endpoint: String, jobs: mpsc::Receiver<Job>) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
        return;
    };
    let mut link = Link {
        endpoint,
        socket: None,
        next_id: 0,
        retry_at: None,
    };
    while let Ok(job) = jobs.recv() {
        let reply = runtime.block_on(link.exchange(job.request));
        let _ = job.reply.send(reply);
    }
}

struct Link {
    endpoint: String,
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    next_id: u64,
    retry_at: Option<Instant>,
}

impl Link {
    async fn exchange(&mut self, mut request: Value) -> Reply {
        if self.socket.is_none() {
            self.connect().await?;
        }
        self.next_id += 1;
        let id = self.next_id;
        request["id"] = json!(id);
        match tokio::time::timeout(REQUEST_TIMEOUT, self.round_trip(id, request.to_string())).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(reason)) => {
                self.socket = None;
                Err(offline(format!("与 hub 的连接中断：{reason}")))
            }
            Err(_) => {
                self.socket = None;
                Err(offline("hub 在超时内未回复"))
            }
        }
    }

    async fn connect(&mut self) -> Result<(), HubError> {
        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return Err(offline(format!("hub {} 刚才不可达，稍后重试", self.endpoint)));
        }
        let failure = match tokio::time::timeout(
            CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async(self.endpoint.as_str()),
        )
        .await
        {
            Ok(Ok((socket, _))) => {
                self.socket = Some(socket);
                self.retry_at = None;
                return Ok(());
            }
            Ok(Err(error)) => format!("连接 hub {} 失败：{error}", self.endpoint),
            Err(_) => format!("连接 hub {} 超时", self.endpoint),
        };
        self.retry_at = Some(Instant::now() + RETRY_AFTER);
        Err(offline(failure))
    }

    async fn round_trip(&mut self, id: u64, text: String) -> Result<Reply, String> {
        let socket = self.socket.as_mut().expect("connected before a round trip");
        socket
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| e.to_string())?;
        while let Some(message) = socket.next().await {
            let text = match message.map_err(|e| e.to_string())? {
                Message::Text(text) => text,
                Message::Close(_) => break,
                _ => continue,
            };
            let Ok(mut reply) = serde_json::from_str::<Value>(text.as_str()) else {
                continue;
            };
            if reply.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(error) = reply.get_mut("err") {
                return Ok(Err(serde_json::from_value(error.take()).unwrap_or_else(|e| {
                    HubError::Uncertain(format!("远端 hub 错误无法解析：{e}"))
                })));
            }
            return Ok(Ok(reply.get_mut("ok").map(Value::take).unwrap_or(Value::Null)));
        }
        Err("hub 关闭了连接".into())
    }
}

#[cfg(test)]
mod tests {
    //! End to end over a real WebSocket: an in-process fake hub serves the
    //! wire protocol from the local `Hub`, so sequencing and idempotency are
    //! the local hub's own rules, reached only through the remote client.
    use super::*;
    use std::path::Path;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
    use std::sync::{Arc, Mutex};
    use towermd_core::hub::Hub;
    use towermd_core::project_runtime::{ProjectRuntime, RuntimeConfig, RuntimeError, WORK_LEASE_TTL_MS};

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    struct FakeHub {
        url: String,
        up: Arc<AtomicBool>,
        drop_next_append_reply: Arc<AtomicBool>,
        hub: Arc<Mutex<Hub>>,
        _dir: tempfile::TempDir,
    }
    impl FakeHub {
        fn start() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let hub = Arc::new(Mutex::new(Hub::open(&dir.path().join("hub")).unwrap()));
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("ws://{}", listener.local_addr().unwrap());
            let up = Arc::new(AtomicBool::new(true));
            let drop_next_append_reply = Arc::new(AtomicBool::new(false));
            let (shared, gate, lose) = (hub.clone(), up.clone(), drop_next_append_reply.clone());
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async move {
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    loop {
                        let (stream, _) = listener.accept().await.unwrap();
                        if gate.load(SeqCst) {
                            tokio::spawn(peer(stream, shared.clone(), gate.clone(), lose.clone()));
                        }
                    }
                });
            });
            Self { url, up, drop_next_append_reply, hub, _dir: dir }
        }
        fn copies(&self, eid: &str) -> usize {
            HubBackend::events(&*self.hub.lock().unwrap(), 0).iter().filter(|event| event.eid == eid).count()
        }
        fn head(&self) -> u64 {
            HubProtocol::contiguous_seq(&*self.hub.lock().unwrap())
        }
    }

    async fn peer(stream: TcpStream, hub: Arc<Mutex<Hub>>, up: Arc<AtomicBool>, lose: Arc<AtomicBool>) {
        let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else { return };
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            if !up.load(SeqCst) {
                return;
            }
            let request: Value = serde_json::from_str(text.as_str()).unwrap();
            let reply = dispatch(&mut hub.lock().unwrap(), &request);
            if request["op"] == "append" && lose.swap(false, SeqCst) {
                return; // landed, but the reply is lost with the connection
            }
            if socket.send(Message::Text(reply.to_string().into())).await.is_err() {
                return;
            }
        }
    }

    fn dispatch(hub: &mut Hub, request: &Value) -> Value {
        fn take<T: DeserializeOwned>(value: &Value) -> T {
            serde_json::from_value(value.clone()).unwrap()
        }
        fn text(value: &Value) -> Value {
            serde_json::from_str(value.as_str().unwrap()).unwrap()
        }
        let at = now();
        let result = match request["op"].as_str().unwrap() {
            "append" => {
                let mut body = request["event"].clone();
                body["data"] = text(&request["data_json"]);
                HubProtocol::append(hub, take(&body), at).map(|r| json!(r))
            }
            "subscribe" => HubProtocol::subscribe(hub, request["after"].as_u64().unwrap(), at).map(|r| json!(r)),
            "lease_acquire" => HubProtocol::lease_acquire(hub, take(&request["request"]), at).map(|r| json!(r)),
            "lease_renew" => HubProtocol::lease_renew(hub, take(&request["request"]), at).map(|r| json!(r)),
            "lease_release" => HubProtocol::lease_release(hub, take(&request["request"]), at).map(|r| json!(r)),
            "snapshot_ack" => HubProtocol::snapshot_ack(hub, take(&request["ack"]), text(&request["checkpoint_json"]))
                .map(|()| Value::Null),
            other => Err(HubError::Invalid(format!("unknown op {other}"))),
        };
        match result {
            Ok(ok) => json!({"id": request["id"], "ok": ok}),
            Err(err) => json!({"id": request["id"], "err": err}),
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "user.name=Remote Test", "-c", "user.email=qa@localhost", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    /// Host A owns the original clone; host B clones it, like a second machine.
    fn host(origin: Option<&Path>, name: &str, url: &str) -> (tempfile::TempDir, RuntimeConfig) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        match origin {
            Some(origin) => git(temp.path(), &["clone", "-q", origin.to_str().unwrap(), "repo"]),
            None => {
                std::fs::create_dir_all(root.join(".towermd")).unwrap();
                git(&root, &["init", "-qb", "main"]);
                std::fs::write(
                    root.join(".towermd/config.yaml"),
                    format!("project_id: remote-project\ndocs_dir: docs/tower\nhub: {{url: '{url}'}}\n"),
                )
                .unwrap();
                git(&root, &["add", "."]);
                git(&root, &["commit", "-qm", "initial"]);
            }
        }
        let config = RuntimeConfig {
            common_dir: root.join(".git"),
            root,
            project_id: "remote-project".into(),
            docs_dir: "docs/tower".into(),
            host: name.into(),
        };
        (temp, config)
    }

    fn reduce(value: &mut Value, event: &Event) -> Result<(), String> {
        if event.kind == "task.created" {
            value.as_array_mut().ok_or("array required")?.push(json!(event.obj));
        }
        Ok(())
    }

    fn open(fake: &FakeHub, config: RuntimeConfig) -> Result<ProjectRuntime, RuntimeError> {
        let hub = RemoteHub::new(&fake.url, "remote-project").unwrap();
        ProjectRuntime::open_remote(config, json!([]), reduce, Box::new(hub), now)
    }

    fn created(host: &str, actor: &str, obj: &str) -> NewEvent {
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: host.into(),
            actor: actor.into(),
            kind: "task.created".into(),
            layer: "L4".into(),
            obj: obj.into(),
            data: json!({"note": "原样保存", "n": 1.5}),
            guards: vec![],
        }
    }

    fn reconnect(fake: &FakeHub) {
        fake.up.store(true, SeqCst);
        std::thread::sleep(RETRY_AFTER * 2);
    }

    #[test]
    fn two_hosts_share_one_remote_sequencer_and_offline_events_resend_with_their_eid() {
        let fake = FakeHub::start();
        let (a_dir, a_config) = host(None, "host-a", &fake.url);
        let (_b_dir, b_config) = host(Some(&a_dir.path().join("repo")), "host-b", &fake.url);
        let mut a = open(&fake, a_config).unwrap();
        let mut b = open(&fake, b_config).unwrap();
        assert!(a.holds_main(), "the first host takes main");
        assert!(!b.holds_main(), "the second host follows instead of forking");
        assert!(b.main_fence().is_err());

        // One sequencer: both hosts' events get consecutive hub sequences.
        let first = a.append(created("host-a", "process:test", "t1"), now).unwrap();
        let second = b.append(created("host-b", "process:test", "t2"), now).unwrap();
        assert_eq!(second.seq, first.seq + 1);
        assert_eq!(second.data, json!({"note": "原样保存", "n": 1.5}));
        a.tick(now).unwrap();
        b.tick(now).unwrap();
        assert_eq!(a.projection(), &json!(["t1", "t2"]));
        assert_eq!(b.projection(), a.projection());
        assert_eq!(a.contiguous_seq(), fake.head());

        // Offline: an ordinary event waits in the outbox, nothing is sequenced locally.
        fake.up.store(false, SeqCst);
        let head = fake.head();
        let queued = created("host-a", "process:test", "t3");
        match a.append(queued.clone(), now) {
            Err(RuntimeError::Offline(message)) => assert!(message.contains(&queued.eid), "{message}"),
            other => panic!("expected an offline append, got {other:?}"),
        }
        assert_eq!(a.pending().unwrap(), vec![queued.clone()]);
        assert_eq!(a.contiguous_seq(), head, "no local sequence was assigned");
        // A human action is never queued behind the human's back.
        let human = created("host-a", "human:local", "t-human");
        assert!(matches!(a.append(human, now), Err(RuntimeError::Offline(_))));
        assert_eq!(a.pending().unwrap().len(), 1);
        assert!(matches!(b.tick(now), Err(RuntimeError::Offline(_))));
        assert_eq!(fake.head(), head);

        // Reconnect: the queued event lands once under its original eid.
        reconnect(&fake);
        a.tick(now).unwrap();
        assert!(a.pending().unwrap().is_empty());
        assert_eq!(fake.copies(&queued.eid), 1);
        let landed = a.receipt(&queued.eid).unwrap().clone();
        assert_eq!(landed.seq, head + 1);
        b.tick(now).unwrap();
        assert_eq!(b.projection(), &json!(["t1", "t2", "t3"]));

        // The hub took the event but the reply was lost: the resend is idempotent.
        fake.drop_next_append_reply.store(true, SeqCst);
        let lost = created("host-b", "process:test", "t4");
        assert!(matches!(b.append(lost.clone(), now), Err(RuntimeError::Offline(_))));
        assert_eq!(fake.copies(&lost.eid), 1);
        std::thread::sleep(RETRY_AFTER * 2);
        b.tick(now).unwrap();
        b.tick(now).unwrap();
        assert!(b.pending().unwrap().is_empty());
        assert_eq!(fake.copies(&lost.eid), 1);
        a.tick(now).unwrap();
        assert_eq!(a.projection(), &json!(["t1", "t2", "t3", "t4"]));
        assert_eq!(b.projection(), a.projection());
    }

    #[test]
    fn leases_and_opening_need_the_hub_and_never_fall_back_to_a_local_one() {
        let fake = FakeHub::start();
        let (dir, config) = host(None, "host-a", &fake.url);
        fake.up.store(false, SeqCst);
        assert!(matches!(open(&fake, config.clone()), Err(RuntimeError::Offline(_))));
        assert!(!config.common_dir.join("towermd/hub").exists(), "no local hub was created");

        reconnect(&fake);
        let mut a = open(&fake, config).unwrap();
        let (_b_dir, b_config) = host(Some(&dir.path().join("repo")), "host-b", &fake.url);
        let mut b = open(&fake, b_config).unwrap();
        let claimed = a
            .acquire_work_leases("agent-1", vec!["task:t1".into()], WORK_LEASE_TTL_MS, now)
            .unwrap();
        b.tick(now).unwrap();
        assert_eq!(b.work_leases(now()).len(), 1);
        // A claim on the same name from the other host is refused by the hub.
        assert!(b
            .acquire_work_leases("agent-2", vec!["task:t1".into()], WORK_LEASE_TTL_MS, now)
            .is_err());

        fake.up.store(false, SeqCst);
        let head = fake.head();
        let fences: Vec<Fence> = claimed.iter().map(Lease::fence).collect();
        assert!(matches!(
            a.acquire_work_leases("agent-1", vec!["task:t2".into()], WORK_LEASE_TTL_MS, now),
            Err(RuntimeError::Offline(_))
        ));
        assert!(matches!(a.renew_work_leases(fences.clone(), WORK_LEASE_TTL_MS, now), Err(RuntimeError::Offline(_))));
        assert!(matches!(a.release_work_leases(fences.clone(), now), Err(RuntimeError::Offline(_))));
        assert!(matches!(b.tick(now), Err(RuntimeError::Offline(_))));
        assert!(!b.holds_main(), "a follower cannot take main while the hub is away");
        assert_eq!(fake.head(), head);

        reconnect(&fake);
        a.release_work_leases(fences, now).unwrap();
        assert!(a.work_leases(now()).is_empty());
        assert!(a.holds_main());
    }

    /// Cross-language check against the real Worker. Run by hand:
    /// `cd hub-worker && pnpm dev --port 8799`, then
    /// `TOWERMD_TEST_HUB_URL=ws://127.0.0.1:8799 cargo test -p towermd worker_hub -- --ignored`.
    #[test]
    #[ignore]
    fn worker_hub_speaks_the_same_protocol() {
        let url = std::env::var("TOWERMD_TEST_HUB_URL").expect("TOWERMD_TEST_HUB_URL");
        let project = format!("cross-{}", uuid::Uuid::now_v7());
        let open = |config| {
            let hub = RemoteHub::new(&url, &project).unwrap();
            ProjectRuntime::open_remote(config, json!([]), reduce, Box::new(hub), now)
        };
        // Hosts share a git origin, as separate machines would; snapshots publish there.
        let (_a_dir, a_config) = host(None, "host-a", &url);
        let shared = tempfile::tempdir().unwrap();
        let origin = shared.path().join("origin.git");
        git(shared.path(), &["clone", "-q", "--bare", a_config.root.to_str().unwrap(), "origin.git"]);
        git(&a_config.root, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&a_config.root, &["fetch", "-q", "origin"]);
        let (_b_dir, b_config) = host(Some(&origin), "host-b", &url);
        let mut a = open(a_config).unwrap();
        let mut b = open(b_config).unwrap();
        assert!(a.holds_main() && !b.holds_main());
        let first = a.append(created("host-a", "process:test", "t1"), now).unwrap();
        let event = created("host-b", "process:test", "t2");
        let second = b.append(event.clone(), now).unwrap();
        assert_eq!(second.seq, first.seq + 1);
        assert_eq!(second.data, json!({"note": "原样保存", "n": 1.5}));
        assert_eq!(b.append(event, now).unwrap().seq, second.seq, "idempotent by eid");
        a.tick(now).unwrap();
        b.tick(now).unwrap();
        assert_eq!(a.projection(), &json!(["t1", "t2"]));
        assert_eq!(b.projection(), a.projection());
        let claimed = a
            .acquire_work_leases("agent-1", vec!["task:t1".into()], WORK_LEASE_TTL_MS, now)
            .unwrap();
        assert!(b
            .acquire_work_leases("agent-2", vec!["task:t1".into()], WORK_LEASE_TTL_MS, now)
            .is_err());
        let fences: Vec<Fence> = claimed.iter().map(Lease::fence).collect();
        a.renew_work_leases(fences.clone(), WORK_LEASE_TTL_MS, now).unwrap();
        a.release_work_leases(fences, now).unwrap();
        b.tick(now).unwrap();
        assert!(b.work_leases(now()).is_empty());
        // Snapshot acknowledgement compacts the Worker; a later clone starts from it.
        a.snapshot(now).unwrap();
        b.append(created("host-b", "process:test", "t3"), now).unwrap();
        let (_c_dir, c_config) = host(Some(&origin), "host-c", &url);
        let mut c = open(c_config).unwrap();
        c.tick(now).unwrap();
        a.tick(now).unwrap();
        assert_eq!(c.projection(), &json!(["t1", "t2", "t3"]));
        assert_eq!(c.projection(), a.projection());
    }

    #[test]
    fn wss_needs_tls_that_this_build_does_not_carry() {
        assert!(RemoteHub::new("wss://hub.example.invalid", "p").is_err());
        assert_eq!(encode("a b/ç"), "a%20b%2F%C3%A7");
    }
}
