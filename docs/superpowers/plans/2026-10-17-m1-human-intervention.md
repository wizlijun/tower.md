---
type: Plan
title: M1 提问与人工干预实施计划
description: 以 hub 持久事件完成有界提问、App 回答、CLI 投递、暂停请求与高风险模块必要人工评审。
status: draft
sources:
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: docs/superpowers/plans/2026-10-17-m1-task-evidence.md
  - resource: docs/superpowers/plans/2026-10-17-m1-app-foundation.md
  - resource: AGENTS.md
  - resource: tasks/lessons.md
  - resource: docs/superpowers/plans/2026-10-17-shared-recovery-proof-contract.md
---

# M1 提问与人工干预 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让已有 harness 会话经 CLI 提问、在 App 得到本次回答，并可靠收到插话；无回答或无人工评审事实时保持阻塞。

**Architecture:** 业务判断放入核心 `HumanState`，沿 `ProjectSession` 从已定序事件增量投影。所有非 heartbeat P3 事实经 core 私有 durable 入口先 pending fsync、HubProtocol 定序、全字段比对原Event并confirmed fsync后返回本次成功；Hub耐久接受照常投影/广播/快照，不以confirmed隔离连续前缀；daemon 组合 outbox 路径；快照沿统一写入模块归档。请求receipt与历史答复从完整归档重建为派生索引，App native 仅代理固定操作，CLI 仅作 process 动作。所有新类型及函数均为本计划待实现契约，不代表当前代码已有实现。

**Tech Stack:** Rust 2021、serde/serde_json、tokio、axum、现有 sha2/hex/uuid；NFKC 沿 Note.md 的 `unicode-normalization = "0.1"`。不引入 Unicode 分类、队列、数据库或 harness SDK 新库。

**共享证明前置契约：** [2026-10-17-shared-recovery-proof-contract.md](2026-10-17-shared-recovery-proof-contract.md) 是 E/P2/P3/P4 的唯一恢复证明输入、confirmed 持久顺序、历史 merge 与 publication schema 来源。以下接口均待实现；开工按实际前置 SHA 对齐，缺 producer/schema 只诊断，不用夹具或字段自报代替真实产品路径。

## 开工条件与现状

- 本文件只规划 P3；P6 负责可见卡片、提示词输入栏、最多三项关注和真人验收。不得替换或重写 App 基础计划。
- 主仓库当前 `main.rs` 只有 task/status 等命令，没有 ask、review、pause/resume；`types.rs` 已登记 Hold/L5，不新增 Question、Answer、Prompt 等对象类型。
- `/private/tmp/towermd-m0-remediation-2` 正迁移 `hub_protocol.rs`、`project_session.rs` 和 `StateProjection`。其真实接口是 `HubProtocol::append(NewEvent, now_ms)`、`subscribe(after, now_ms)`、`lease_renew(RenewLease, now_ms)`，不得回接旧 `LeaseChange` 业务写入。
- `ProjectSession` 当前有 `tasks: TaskService`、`projection: StateProjection`、`sync/tick/renew_binding`；业务事件只在连续水位内应用。新增 human 必须同时支持 `restore`、`apply` 和快照重放。
- M0 批 2–5 合并后，先检查最终源码与本计划接口；接口变动在本计划中精确修订并重跑红测，不把临时 worktree 当作已交付基线。
- P2 与本计划已协调：计划在运行中写好后用既有 `review request` 提交不可变内容，daemon 重新计算 bytes 的 SHA256 并归档；不新增 claim 参数、Run.plan_path、计划必须已提交或位于 allowed_paths 的限制。
- P2 从 `ProjectSession.human.require_human_review(&ReviewBinding)` 读取唯一人工事实。P2 可先缺事实硬阻塞，再由本计划解锁；不能用“G1/G4 留 M2”临时放过 high 风险模块。
- P3 不生成长期 Decision/Rule，不调用模型，不控制 harness 子进程，不提供 retry 清硬门禁、豁免或批准命令。
- 用户已授权持续开发至首个可用未签名 App；本计划的编写不需要重复询问实施方式。真实 App 人工操作就绪后才收集真人证据。

## 冻结行为与规范对齐

1. Task 1 开工第一步最小修订 FR-96：CLI actor 永远 `process:towermd-cli`，`data.relayed_from: human` 与 `self_reported` 保留转述出处；Agent 自报不变成可信署名。脚注引用本会话用户提供的 AGENTS.md。FR-111 的 App 人工署名边界优先。[^actor]
2. FR-61 的“保留租约”指开中断不立即释放；仍受 FR-112 的 15 分钟活动 TTL 约束。live wait 及任意项目 CLI 活动续租，deferred 后无活动正常到期；30 分钟未答升级关注，不自动通过。
3. 指纹保留权威 Catalog 的 canonical module ID 原值，仅归一化问题文本；`A` 与 `a` 的同问不是同一指纹。同一模块的未处理同问复用 Hold ID。跨 run 自动回答只读取权威 main 中有效且精确匹配的 Decision；本次回答和 CLI 转述不能成为之后运行的自动知识。
4. 每次 ask 最多等 90 秒，累计 live wait 达 10 分钟返回 deferred；计时使用 daemon 时钟与持久 wait slice，不靠 CLI 内存。
5. 已到回答与插话先完整写 stdout 并 flush，再确认 delivered；输出失败不确认。重复输出沿稳定 message ID 幂等；delivered 不表示 processed。
6. pause --all 关 claim 并对既有活动 run 排队暂停插话；resume --all 仅请求，App 确认明确项目、暂停版本和运行范围后恢复。均不向 harness 发 signal。
7. G1 的计划 hash 与 G4 的 tree 必须绑定 run/task/baseline/全部 fences，只有 App human 评审事实可满足；内容、基线或任何 fence 改变，旧批准失效。
8. 回答、评审、投递、暂停/恢复及其请求receipt均先写本机恢复outbox并fsync，不能只append+sync。恢复保留原eid/guards/request metadata；旧guards仍stale，只在独立历史视图可读，不能恢复有效批准或继续旧run。

[^actor]: 2026-10-17 本会话用户提供的 [AGENTS.md](../../../AGENTS.md)：“命令行发出的事件署名 `process:towermd-cli`”；“人的动作署名 `human:<id>`，只能从 App 发出”。对应发布总表 P3 已冻结 FR-96 的最小对齐，不扩大 CLI 权限。

## 文件责任与统一契约

| 文件 | 职责 |
| --- | --- |
| `crates/towermd-core/src/human_intervention.rs` | typed Hold、问答去重、计时、评审事实、投递状态的纯业务投影 |
| `crates/towermd-core/src/question_punctuation.rs` | 固定 Unicode 13.0 P 类标点区间，仅供结尾标点判断 |
| `crates/towermd-core/src/project_session.rs` | human restore/apply、outbox先行写入、派生请求receipt/历史答复索引与故障协调 |
| `crates/towermd-core/src/recovery_outbox.rs`（批3已有） | 复用append_pending/read_inputs持久恢复输入，不增加远程传输 |
| `crates/towermd-core/src/tasks.rs` | 暴露运行绑定读取；claim 读取项目 pause 状态；不改证据规则 |
| `crates/towermd-core/src/snapshot.rs` | Hold 投影、wait/delivery/pause/review 事实恢复 |
| `crates/towermd/src/{main,daemon}.rs` | 参数解析、有限路由、公共 CLI prelude、计时等待 |
| `crates/towermd/src/cli_delivery.rs` | stdout 写入成功后 ack，独立可测试 |
| `crates/towermd-core/tests/human_intervention.rs` | 纯领域红绿与真实 HubProtocol 归档回放 |
| `crates/towermd/tests/human_intervention_cli.rs` | 真 daemon、真实 CLI 的问答/投递/过期/暂停拒绝 |
| `src-tauri/src/{commands,daemon_client}.rs` | 固定 native mutation 入口；沿 P6 ProjectToken 解析目录 |
| `src/lib/daemon/{types,wire,client}.ts` | P6 卡片所需 typed 数据与固定 invoke；不在此实现界面 |

所有 Hold 都使用 `type: Hold`，子类在 `kind` 区分；项目暂停也为全局 Hold，不新增全局状态文件。以下类型放 `human_intervention.rs`，均派生 Clone/Debug/Serialize/Deserialize/PartialEq；枚举使用 `#[serde(tag="kind", rename_all="snake_case")]`，单位枚举使用 `rename_all="snake_case"`。

```rust
pub enum ReviewSubject { Plan { sha256: String }, Tree { tree: String } }
pub struct ReviewBinding {
    pub run: String, pub task: String, pub baseline: String,
    pub fences: Vec<Fence>, pub subject: ReviewSubject,
}
pub enum ReviewVerdict { Approved, ChangesRequested, Rejected }
pub struct ReviewFact {
    pub binding: ReviewBinding, pub verdict: ReviewVerdict,
    pub actor: String, pub answer_eid: String,
}
pub struct Recipient {
    pub run: String, pub task: String, pub host: String,
    pub worktree: String, pub branch: String, pub fences: Vec<Fence>,
}
pub struct WaitSlice {
    pub id: String, pub recipient_run: String, pub started_ms: u64,
    pub deadline_ms: u64, pub finished_ms: Option<u64>,
}
pub struct Reply {
    pub id: String, pub text: String, pub actor: String,
    pub relayed_from: Option<String>, pub self_reported: Option<SelfReported>,
    pub decision: Option<String>, pub decision_revision: Option<String>,
    pub at_ms: u64,
}
pub enum HoldBody {
    Ask { fingerprint: String, question: String, module: String,
          options: Vec<String>, recommended: Option<usize>, context: String },
    Prompt { text: String, scope: Scope },
    Review { binding: ReviewBinding, verdict: Option<ReviewVerdict>,
             plan_content: Option<String>, source_commit: Option<String>, source_path: Option<String>,
             comment: Option<String> },
    Pause { revision: String, active_runs: Vec<String>, resumed: bool },
    Resume { pause_revision: String, active_runs: Vec<String>, confirmed: bool },
}
pub enum Scope { Project, Module { module: String }, Task { task: String } }
pub struct Hold {
    #[serde(rename="type")] pub object_type: String,
    pub id: String, pub body: HoldBody, pub recipients: Vec<Recipient>,
    pub opened_ms: u64, pub opened_seq: u64, pub reply: Option<Reply>, pub waits: Vec<WaitSlice>,
    pub delivered: std::collections::BTreeMap<String, Vec<String>>,
    pub attention: bool,
}
pub enum AskState { Pending, Answered, Deferred, Expired }
pub struct AskResult { pub hold: String, pub state: AskState, pub reply: Option<Reply> }
pub struct Message {
    pub id: String, pub hold: String, pub run: String, pub text: String,
    pub kind: String, pub actionable: bool,
}
#[derive(Default)]
pub struct HumanState { pub holds: std::collections::BTreeMap<String, Hold>,
    review_facts: std::collections::BTreeMap<String, ReviewFact> }
```

`delivered` 的键为 recipient run，值为已确认 message IDs；消息 ID 是 `hold-id/reply-id/run-id` 或 `hold-id/prompt/run-id`。收件人是开立/插话时冻结的集合；后来的新 run 不继承本次回答或旧暂停提示。

所有业务mutation外层有稳定请求ID，CLI/native对一次调用生成UUID，传输重试沿同ID。`InteractionRequest { directory: PathBuf, run: Option<String>, request_id: String, command: HumanCommand }` 用于 `/interaction`，AppRequest同样携request_id；`/inbox/delivered`也必须携request_id。不能以hub的eid→seq或当前Hold替代请求语义幂等。

```rust
pub struct RequestReceipt {
    pub request_id: String, pub body_hash: String, pub result_ref: serde_json::Value,
}
```

`RequestReceipt`是事件data的普通嵌套值，不是注册object。每个非heartbeat P3事件均携 `data.p3_request={request_id,body_hash}`；该请求的最终事件另带 `data.p3_receipt`，其result_ref包含稳定hold/message/slice IDs及原始响应JSON。body_hash由核心将typed原始请求（去request_id/instance，目录解析为project_id，保留actor/host/显式run及全部业务参数）递归排序object键后序列化并SHA256；自动绑定得到的run存结果引用，不因重领改变同一原请求的hash。

`ProjectSession`保留私有派生 `p3_requests: BTreeMap<String, RequestReceipt>` 与 `historical_replies: BTreeMap<String, Reply>`。启动、恢复安装及SnapshotRequired从经批3校验的完整月份归档加连续tail重建，并持续增量消费新事件；正常CLI查询不读全历史。receipt索引按project+request_id唯一：异hash拒绝；同hash在查询/返回幂等成功receipt前，先由本机协调器按outbox原pending与真实原Event补confirmed，持续fsync失败仍uncertain；满足耐久条件才返回原result_ref，不重执行。完整归档内重复receipt结果不同也拒绝，不能只比hash。当前Hold后来改变也不能改写原响应；恢复后另报current validity=historical_only，不能把原批准结果当当前有效批准。

同问复用旧Hold、同内容复用Review、重复delivered/resume等没有新业务变化的请求，也须写独立 `hold.requested` 非heartbeat归档事件（data包含旧Hold及该请求receipt），成功持久后才返回旧结果；该事件不改opened_seq/currentPlan/送达集合。wait请求先写带metadata的started，再在finished事件保存最终receipt；未结束请求重开只接续原slice，不新增预算。

批3outbox只是adapter，以下统一入口由P3实际接线，不能假设它已经包住append：`ProjectSession::append_p3_event(&mut self, outbox: &mut RecoveryOutbox, event: NewEvent, now_ms: u64) -> Result<AppendReceipt,String>`。daemon显式在 `runtime/recovery-outbox/<project_id>/<host>.jsonl` 对应批3目录open并传入outbox（按批3最终open参数映射，不由core猜用户路径）；先 `append_pending(&event)` 完成JSONL/fsync，再 `hub.append(event,now_ms)`；按共享契约全字段/全guards比对实际完整 Event，经 core 内部 `persist_confirmed(&event,&actual)` fsync后，才返回本次mutation/幂等成功；Hub已耐久接受照常sync/广播/快照，原p3_receipt保留真实结果、不改为失败。本机协调器从outbox派生uncertain/confirmed，非authority、不local quarantine。仅 Duplicate{seq} 无已核原Event时不能造确认，返回uncertain。Mutation入口先核project+request_id/body_hash冲突，并在查询/返回成功receipt前协调该原请求pending/真实Event的confirmed耐久状态，再做tick/运行解析/业务判断，防止有效旧请求因后来过期或绑定变化被当新请求；启动从outbox.read_inputs重建未完成请求到原NewEvent的派生映射，uncertain重试只协调这些原eid/guards，不重新规划事件。pending fsync失败不得调用hub；confirmed fsync失败、hub/回包不确定则返回原request_id的uncertain，重试沿原eid/bodyhash，禁止生成新guards。

所有hold.opened/wait_started/wait_finished/answered/reviewed/attention、prompt.delivered、Prompt/Pause/Resume和hold.requested都用此入口；append_p3_event内部调用E唯一append_durable，不复制持久逻辑；live heartbeat/lease renewal沿现协议不入outbox。不扩展M2远程client，不增加唯一DB/新注册object。历史索引只收录已验证归档、已核confirmed原回执与恢复后已定序的stale记录；原非stale接受资格与新hub有效资格分开，confirmed不能复活旧fences；未进入hub的pending outbox仅作uncertain协调输入，不能直接变有效业务或宣称已回答。


历史只读入口固定为 `ProjectSession::historical_reply(&self, reply_id: &str) -> Option<&Reply>`；daemon现有App干预读取返回独立 `historical_replies` 列表，包含原Reply、来源eid/原fences与historical_only。CLI的 `ask --wait <id>` 对旧run仍返回expired，可附此历史答复但actionable=false，不新增批准/恢复入口。该视图不能传给HumanState::apply以绕过stale。

核心公共入口固定为：

```rust
impl HumanState {
    pub fn restore(&mut self, projection: &StateProjection) -> Result<(), String>;
    pub fn apply(&mut self, event: &Event) -> Result<(), String>;
    pub fn current_plan_subject(&self, run: &str) -> Result<ReviewSubject, String>;
    pub fn require_human_review(&self, binding: &ReviewBinding) -> Result<&ReviewFact, String>;
    pub fn inbox(&self, recipients: &[Recipient], active: &[Fence]) -> Vec<Message>;
    pub fn paused(&self) -> bool;
}
pub fn normalize_question(text: &str) -> String;
pub fn fingerprint(module: &str, question: &str) -> String;
pub fn wait_budget(hold: &Hold, run: &str, now_ms: u64) -> u64;
pub fn next_slice(hold: &Hold, run: &str, now_ms: u64) -> Option<WaitSlice>;
pub fn ask_state(hold: &Hold, run: &str, live_fences: &[Fence], now_ms: u64) -> AskState;
```

`HumanState` 的私有派生 `review_facts` 从 Hold Review 的最终答复重建，不能作为唯一状态；每个 Review 的 `plan_content/comment` 和 human Reply 都必须保存在 Hold 中，快照后仍可恢复 fact。`current_plan_subject` 选择同 run 的 G1 Review 中 opened_seq 最大的 subject；该值由已定序 hold.opened 的 event.seq 投影赋值，不能信任客户端自报。`require_human_review` 返回其中引用，完整比较排序后的 fence 集合，拒绝重复 fence 名；禁止只比较主模块 epoch。

## Task 1：独立人工评审契约、真实投影与最小 App 评审闭环

**Files:** 创建 `human_intervention.rs` 及核心测试；修改 `src/lib.rs`、`project_session.rs`、`snapshot.rs`、`tasks.rs` 与 daemon/main；在P6 native基础落地后只加固定 `request_review/answer_review` 适配。这个Task不依赖P2 FrozenFinish，也不要求先实现ask/插话。

- [ ] **Step 1：落实上方FR-96/61最小规格修订。** 记录用户AGENTS来源脚注；不修改AGENTS。定义本文件所有共享types（暂未使用的Ask/Prompt也必须可serde往返），`HumanState`真实restore/apply/current_plan_subject/require_human_review；空状态返回缺事实硬阻塞，不能用默认Approved stub。
- [ ] **Step 2：先写核心红测。** `ReviewBinding`完全按统一契约构造；私有review_facts的纯reader测试放 `human_intervention.rs` 模块内，实际路由/恢复测试禁止直接插入事实。

```rust
#[test]
fn review_requires_exact_human_binding() {
    let base = ReviewBinding {run:"R".into(),task:"T".into(),baseline:"B0".into(),
        fences:vec![Fence{name:"module:A".into(),holder:"R".into(),epoch:7}],
        subject:ReviewSubject::Plan{sha256:"a".repeat(64)}};
    let mut state = HumanState::default();
    assert!(state.current_plan_subject("R").is_err());
    assert!(state.require_human_review(&base).is_err());
    let fact = ReviewFact {binding:base.clone(),verdict:ReviewVerdict::Approved,
        actor:"process:towermd-cli".into(),answer_eid:"answer-1".into()};
    state.review_facts.insert("review-1".into(), fact);
    assert!(state.require_human_review(&base).is_err());
    state.review_facts.get_mut("review-1").unwrap().actor="human:owner".into();
    assert!(state.require_human_review(&base).is_ok());
    let mut changed=base.clone(); changed.fences[0].epoch=8;
    assert!(state.require_human_review(&changed).is_err());
    changed=base.clone(); changed.subject=ReviewSubject::Plan{sha256:"b".repeat(64)};
    assert!(state.require_human_review(&changed).is_err());
    changed=base.clone(); changed.subject=ReviewSubject::Tree{tree:"a".repeat(40)};
    assert!(state.require_human_review(&changed).is_err());
}
```

- [ ] **Step 3：运行红测。** `~/.cargo/bin/cargo test -p towermd-core human_intervention::tests::review_requires_exact_human_binding -- --exact`，预期新type未实现或reader错误；确认真实执行1测试。追加真实Hub写hold.opened/hold.reviewed后重开session与checkpoint+tail恢复测试，reader不得只在内存fixture可用。
- [ ] **Step 4：真实业务接线。** `ProjectSession`增加 `pub human: HumanState`，new/SnapshotRequired都restore；sync在连续水位内apply；snapshot投影存完整Hold/计划内容/Reply/verdict/comment，restore据此恢复ReviewFact。hold.opened用event.seq填opened_seq，hold.reviewed不能改opening顺序。archive保留旧内容，最新同run G1 subject由opened_seq确定。
- [ ] 从现TaskService.binding提取 `bound_run(&Catalog,host:&str,run:Option<&str>)->Result<Run,String>`，不改运行状态或租约规则。`ReviewGate`仅G1/G4，`HumanCommand`采用tag action且deny_unknown_fields，先支持 `ReviewRequest {gate,content:Option<String>,source_commit:Option<String>,source_path:Option<String>}`，创建后立即返回pending/稳定Hold ID；Task2以同一预算算法加入 `ReviewWait {hold}` 与Ask，Task3–4再扩Relay/Pause/Resume，不在P2定义另一套事实。
- [ ] 新增 `ProjectSession::human_command(&Catalog,host,&HumanCommand,run,&mut RecoveryOutbox,now)->Result<Value,String>`，tick/绑定/续租后核心执行，所有新业务事件按上方append_p3_event先pending fsync→HubProtocol.append→核完整Event→confirmed fsync→本次成功；最终请求receipt也走同一入口。/interaction只允许CLI typed命令且actor固定process:towermd-cli，无自由actor/verdict。
- [ ] CLI沿规格既有 `review request --gate G1 --content <文件>` 在运行中读取明确UTF-8文本，daemon按 `hex::encode(Sha256::digest(content.as_bytes()))` 重算并归档完整内容；source path/commit仅溯源、不执行shell、不强制已提交或allowed_paths。重提不同内容创建新Hold替换currentPlan；重复当前同hash复用请求。缺当前Plan时reader明确失败。
- [ ] `review request --gate G4` 不接受content，读取当前run工作树已提交HEAD tree；用Catalog受保护路径/Task.allowed_paths及最新权威main只读校验，脏工作树/越界/慢层改动/旧main拒绝。调用P2 Task1已提交 `task_gates::check(catalog,task,run,host,&leases,now_ms)`，取其 `CheckReport.tree`；若P2 Task1尚未合入，本Task停在此真实依赖，不复制另一验证器。G4读取tree不依赖FrozenFinish/Evidence，P2随后对相同验证tree消费事实。
- [ ] 新Hold的ReviewBinding从当前Run/Task/权威baseline与全部live fences生成；fences排序并拒绝重复名称。模块risk high不产生自动答案；普通回答、转述、delivered和超时都不进入review_facts。
- [ ] **Step 5：最小App路由。** daemon加入固定 `/app/interventions/action`，最先只支持 `AppAction::Review {hold,binding,verdict,comment}`；`AppGate`沿M0已有有限客户端校验复用，要求instance及固定App客户端标记，通过才构造human:<id>，CLI route拒绝该输入。Task4给出其余AppAction的准确类型；本Task先独立编译该Review变体。
- [ ] App Review重新检查当前run全部live fences、权威baseline、currentPlan或HEAD tree，再比较回传binding；内容/基线/任一epoch变化409，不迁移批准。记录 `hold.reviewed` actor human，完整Hold里保存human Reply、verdict/comment与来源；ChangesRequested/Rejected保持硬阻塞，Approved才产生匹配事实。不同答复冲突，重试同request ID幂等。
- [ ] **Step 6：绿与提交。** 真实CLI review request→Appgate路由→reader→snapshot恢复保持事实；测试无App标记/CLI伪human/过期fences/改计划/tree/baseline/重复审批均拒绝。运行 `~/.cargo/bin/cargo test -p towermd-core --test human_intervention`、`~/.cargo/bin/cargo test -p towermd review_`；只精确提交本Task文件，提交 `feat(review): persist App-bound human review facts`，记录SHA。HTTP夹具不是真人验收。

依赖链固定：M0批2–5 → P2 Task1纯guards → 本Task真实review提交 → P2 Task2 freeze/finish消费；P3 Task2–4可继续，Task5再做P2/P3联验。不能让P2先引用不存在的session.human字段，也不能用“将来注入reader”解除high风险硬阻塞。

## Task 2：hub 问答、90 秒等待与 10/30 分钟边界

**Files:** 修改根 `Cargo.toml/Cargo.lock`、`crates/towermd-core/Cargo.toml`、`human_intervention.rs`、`project_session.rs`、`tasks.rs`、`snapshot.rs`；创建 `question_punctuation.rs`，扩展核心测试；修改 daemon 有界等待与 CLI ask 参数。

先完成以下归一化与权威知识步骤，再实现问答等待；本Task一个闭环提交。

- [ ] **归一化步骤 1：核Task1最小规格对齐并加红测。** FR-96/61应已有用户来源脚注和活动TTL说明，不重复改规格。测试写入：

```rust
use towermd_core::human_intervention::{fingerprint, normalize_question};
#[test]
fn normalization_is_exact_and_module_scoped() {
    assert_eq!(normalize_question("  ＡＢＣ\t\n 写入？！  "), "abc 写入");
    assert_eq!(normalize_question("ÄBC + 队列"), "Äbc + 队列");
    assert_eq!(fingerprint("A", "写入？"), fingerprint("A", "写入!"));
    assert_ne!(fingerprint("A", "写入"), fingerprint("a", "写入"));
    assert_ne!(fingerprint("A", "写入"), fingerprint("B", "写入"));
    assert_ne!(fingerprint("A", "写入"), fingerprint("A", "写 入"));
    assert_eq!(normalize_question(" x—“?!"), "x");
}
```

- [ ] **归一化步骤 2：红测命令。** `~/.cargo/bin/cargo test -p towermd-core --test human_intervention normalization_is_exact_and_module_scoped -- --exact`。预期 unresolved import，而不是已有测试偶然失败；确认实际跑到该测试。
- [ ] **归一化步骤 3：实现明确的归一化。** 根 workspace 加 `unicode-normalization = "0.1"`，core 以 `.workspace = true` 使用，沿 Note 已有选择；仅此时更新锁文件。文本顺序为 NFKC → `split_whitespace().join(" ")` → `to_ascii_lowercase()` → 删除结尾全部 Unicode General_Category=P 字符 → 再 trim_end；不删除中间标点、不做相似匹配。module参数必须取权威Catalog/Task的canonical ID原值，不做NFKC、trim、空白合并或lower；Catalog中的A/a可不同，不合并ID。

```rust
use unicode_normalization::UnicodeNormalization;
pub fn normalize_question(text: &str) -> String {
    let compatible: String = text.nfkc().collect();
    let compact = compatible.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.to_ascii_lowercase().trim_end_matches(
        crate::question_punctuation::is_punctuation
    ).trim_end().to_owned()
}
pub fn fingerprint(module: &str, question: &str) -> String {
    serde_json::to_string(&(module, normalize_question(question))).unwrap()
}
```

- [ ] **归一化步骤 4：生成静态标点表，固定版本。** 执行下列 Python 标准库代码生成 `question_punctuation.rs`；仓库提交生成结果，运行时不需要 Python。升级 Unicode 表另行精确变更，不随机器静默变化。

```python
import unicodedata
from pathlib import Path
assert unicodedata.unidata_version == '13.0.0'
points = [n for n in range(0x110000) if unicodedata.category(chr(n)).startswith('P')]
ranges = []
for n in points:
    if ranges and ranges[-1][1] + 1 == n:
        ranges[-1] = (ranges[-1][0], n)
    else:
        ranges.append((n, n))
text = '// Unicode 13.0.0 General_Category=P; generated from Python unicodedata.\n'
text += 'const RANGES: &[(u32,u32)] = &[\n'
text += ''.join(f'    (0x{a:X}, 0x{b:X}),\n' for a,b in ranges) + '];\n'
text += 'pub fn is_punctuation(c: char) -> bool { let n=c as u32; RANGES.iter().any(|&(a,b)| a<=n && n<=b) }\n'
Path('crates/towermd-core/src/question_punctuation.rs').write_text(text)
```

- [ ] **归一化步骤 5：权威读取 red/green。** 定义 `decision_answer(catalog: &Catalog, module: &str, question: &str, now_ms: u64) -> Result<Option<Reply>, String>`。只遍历 `catalog.objects` 的 Decision；`affects` 包含 canonical 模块 ID，`context` 是原问题文本、`decision` 是非空答复。要求未 deprecated、未 stale_after、未被任何有效 Decision.supersedes 取代；未知/畸形相关字段不给自动答案。`status` 缺省按 stable；日期按 UTC 日，`today >= stale_after` 失效。多个仍有效且匹配的 Decision 返回冲突错误、保持提问，不任取一个。Reply.actor=`process:towermd-dedupe`，带 decision ID 与 catalog.revision。
- [ ] 将 `context: ＡＢＣ？`、`decision: 阻塞`、`affects: [A]` 的权威 Decision 放入现有 `tests/tasks.rs::catalog` 风格夹具；断言同问返回 ID；改变工作树文件无影响；deprecated/stale/superseded/别模块/不同指纹均不返回；相同问题两个有效不同答案报冲突。fixture 用 `Document::parse`，不伪造批准。
- [ ] **归一化步骤 6：局部绿测。** 执行 `~/.cargo/bin/cargo test -p towermd-core --test human_intervention`，再 `cargo fmt --all --check`。归一化与下面问答绿后只精确 add 本Task文件，一并提交 `feat(intervention): persist bounded asks and current-run replies`，记录实际提交号。提交里不得包含其他计划或 AGENTS 未跟踪文件。

- [ ] **Step 1：增加 typed 输入。** `HumanCommand` 采用 tag action、deny_unknown_fields；`AskOpen { question, options, recommended, context }`、`AskWait { hold }`、`Relay { hold, text, self_reported: Option<SelfReported> }`、`ReviewRequest { gate: ReviewGate, content: Option<String>, source_commit: Option<String>, source_path: Option<String> }`、`ReviewWait { hold }`、`PauseAll`、`ResumeAll`；`ReviewGate` 单位枚举只有 G1/G4。CLI 参数固定 `ask "问题" [--option 文本] [--recommended 索引] [--context 文本]`、`ask --wait <hold>`、`ask --reply <hold> --answer "答复" [--harness 名称 --harness-version 版本]`。互斥参数由 clap 拒绝；没有 `--actor`、`--approve`、`--save-decision`。
- [ ] **Step 2：写红测与完整fixture。** 预算测试使用纯Hold；真实租约测试必须另走TaskCommand::Add/Claim。

```rust
fn ask_fixture() -> Hold {
    Hold { object_type:"Hold".into(),id:"H-1".into(),
        body:HoldBody::Ask { fingerprint:fingerprint("A","问题"),question:"问题".into(),
            module:"A".into(),options:vec![],recommended:None,context:String::new() },
        recipients:vec![Recipient {run:"R-1".into(),task:"T-1".into(),host:"host".into(),
            worktree:"wt".into(),branch:"t/T-1".into(),fences:vec![]}],
        opened_ms:0,opened_seq:0,reply:None,waits:vec![],
        delivered:std::collections::BTreeMap::new(),attention:false }
}
```

```rust
#[test]
fn waits_are_capped_persisted_and_never_authorize() {
    let mut h = ask_fixture();
    let s = next_slice(&h, "R-1", 0).unwrap();
    assert_eq!(s.deadline_ms, 90_000);
    h.waits.push(s);
    assert!(next_slice(&h, "R-1", 1).is_none());
    assert_eq!(wait_budget(&h, "R-1", 90_000), 90_000);
    h.waits[0].finished_ms = Some(90_000);
    for start in [90_000,180_000,270_000,360_000,450_000,540_000] {
        let mut slice = next_slice(&h, "R-1", start).unwrap();
        slice.finished_ms = Some(slice.deadline_ms);
        h.waits.push(slice);
    }
    assert_eq!(wait_budget(&h, "R-1", 700_000), 600_000);
    assert!(next_slice(&h, "R-1", 700_000).is_none());
    assert!(h.reply.is_none());
}
```

- [ ] **Step 3：运行红测。** `~/.cargo/bin/cargo test -p towermd-core --test human_intervention waits_are_capped_persisted_and_never_authorize -- --exact`；预期新函数未实现。补闭环测试使用真实 `Hub::open(tempdir)` 与 `ProjectSession::new(Box::new(hub), now)`，任务沿 `TaskCommand::Add/Claim` 建立，不能直接塞“有效运行”。
- [ ] **Step 4：最小实现路径。** `TaskService::bound_run(&Catalog, host: &str, run: Option<&str>) -> Result<Run,String>` 复用已有 binding 判断及活动状态，错误列本机运行；不另建 cwd/环境变量绑定。新增 `ProjectSession::human_command(&Catalog,host,&HumanCommand,run,&mut RecoveryOutbox,now)->Result<Value,String>`，先 tick/绑定/续租，再调用核心规划事件，通过append_p3_event先pending fsync→append→核完整Event→confirmed fsync→本次成功。AskOpen 拒绝空归一化问题和越界 recommended；module 来自 Run 对应 Task，不能让请求任意指定。
- [ ] 新事件 `hold.opened/hold.wait_started/hold.wait_finished/hold.answered/hold.attention` 的 data 统一 `{hold: <完整Hold>}`；obj=Hold.id、layer=L5。open/wait/CLI relay actor=`process:towermd-cli`；去重答复 actor=`process:towermd-dedupe`。所有活动 run 写入带完整 guards；App 迟到答复使用无 guards 的归档事件，绝不写 task/run 状态。
- [ ] 任务中断状态由Hold投影提供，Task的显示状态可由P6叠加；Run保留活动绑定而不伪造完成。TaskService的续租/expire仍按全部fences判断，开Hold不释放租约，过期继续沿已有task.released路径。去重在 daemon/core session 互斥锁内执行：先查询权威 Decision；否则按 fingerprint 复用未回答 Ask，追加冻结 recipient（run 独立累计预算）；即使recipient已存在、业务无变化也须独立写hold.requested及新请求receipt后返回，已回答本次 Hold 不用于新 run 自动回答。同 run 重复 open 返回同 ID/同预算。hub 只做 eid/guards/顺序，不理解指纹。
- [ ] slice.id 使用稳定请求 UUID；deadline=`now + min(90_000, 600_000 - spent)`。只允许一个 hold/run 的活动 slice；并发 wait 返回同 slice，不重复累计。预算为该 run 所有 slice 的 `min(finished_ms.unwrap_or(now),deadline)-started` 总和，上限600000；崩溃未结束 slice 到 deadline 自动结算，重启不重置。
- [ ] daemon后台tick调用 `ProjectSession::human_tick(host: &str, now_ms: u64, outbox: &mut RecoveryOutbox) -> Result<(),String>`：daemon按已核project显式传同outbox，结算到期slice/创建一次hold.attention的普通producer均经append_p3_event→E唯一helper，固定原eid/自动请求ID先持久；Hub已接受可sync，confirmed失败记协调uncertain、不重复结算；该方法不为deferred/无CLI活动续模块租约。daemon 等待循环每不超过 15 秒同步/续租，按到达答复或 slice deadline 返回；等待不得持有 ProjectSession 锁、HTTP 全局锁或占用其他模块。reply 到达结束 slice；未答且 spent>=600000 返回 deferred，否则 pending。30分钟阈值按 opened_ms，未答只写一次 attention；deferred 不再后台替 Agent 永久续租。
- [ ] 每次恢复/等待先检查全部 live fences；过期返回 expired 与归档可读答复，但 actionable=false，不重新 acquire、不把旧 Run 改 running、不自动继续。中断期间其他模块 claim 正常；当前模块在 TTL 到期前仍互斥。
- [ ] **Step 5：绿验收。** fake daemon clock 测 89.999/90/599.999/600/1799.999/1800秒；重启在 live slice 中、reply 到达瞬间、不同 run 同问、并发重复请求。真实 CLI→daemon ask 返回 Hold，CLI relay 只能解当前问题，事件仍 process，StateProjection 中无 Decision/Rule。
- [ ] **Step 6：精确提交。** `~/.cargo/bin/cargo test -p towermd-core --test human_intervention` 和 `~/.cargo/bin/cargo test -p towermd --test human_intervention_cli ask_` 绿后提交 `feat(intervention): persist bounded asks and current-run replies`。记录提交；不能把测试 API 回答算 App 真人回答。

## Task 3：公共 CLI prelude 与可幂等投递确认

**Files:** 创建 `cli_delivery.rs`；修改 `main.rs`、`daemon.rs`、`human_intervention.rs`；扩展两层测试。这个 Task 不实现 App UI。

- [ ] **Step 1：固定 wire。** `POST /inbox` 请求 `{directory,run}` 返回 `{messages:Vec<Message>,delivery_token:String}`；token 在 server 绑定项目、接收运行集合和稳定 message IDs，只允许这些 IDs 被确认，不接受 actor。`POST /inbox/delivered` 请求 `{directory,request_id,delivery_token,message_ids}`；delivery_token绑定范围先解码为稳定message/run集合再入body_hash，不以临时token字节作为业务身份。两者沿现 `x-towermd-instance` 校验；读 inbox 不确认，不提供 processed 路由。
- [ ] **Step 2：写 stdout 故障红测。** `cli_delivery.rs` 导出以下函数与测试，闭包让单元测试不依赖 daemon。

```rust
pub fn print_then_ack<W: std::io::Write>(
    out: &mut W, messages: &[Message],
    ack: impl FnOnce(&[String]) -> Result<(), String>,
) -> Result<(), String>;
#[test]
fn failed_stdout_does_not_ack() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let m = Message { id:"H-1/A-1/R-1".into(),hold:"H-1".into(),run:"R-1".into(),
        text:"阻塞写入".into(),kind:"answer".into(),actionable:true };
    let called = std::cell::Cell::new(false);
    assert!(print_then_ack(&mut Broken, &[m], |_| { called.set(true); Ok(()) }).is_err());
    assert!(!called.get());
}
```

- [ ] **Step 3：红命令。** `~/.cargo/bin/cargo test -p towermd cli_delivery::tests::failed_stdout_does_not_ack -- --exact`，预期函数不存在；实现后确认过滤器实际跑1测试。
- [ ] **Step 4：最小实现。** 每条消息输出一行 JSON `{towermd_message: <Message>}`，全部 write_all+flush 成功后 ack IDs；普通命令 JSON 紧随其后。命令执行报错也先投递。flush失败与进程死于输出和 ack 之间均保留未确认，下一次重放同 ID。ack 失败返回明确错误，下次仍允许同 ID。

```rust
let ids = messages.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
for message in messages {
    let mut bytes = serde_json::to_vec(&serde_json::json!({"towermd_message":message}))
        .map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    out.write_all(&bytes).map_err(|e| e.to_string())?;
}
out.flush().map_err(|e| e.to_string())?;
ack(&ids)
```

- [ ] 公共 prelude 放在 clap 成功解析且能定位现有项目之后、任何具体项目命令之前，包括 status/task/baseline/check/finish/trailers/snapshot/ask/rebuild-index/pause/resume；init 首次无项目不读，daemon 管理不是项目入口。显式 --run 仅覆盖选择，不允许跨 host 偷收；自动按 canonical 工作树+分支选择当前及已释放旧运行的未送达消息，旧运行仅显示归档且 actionable=false。
- [ ] 任意项目 CLI 活动先沿 `ProjectSession.renew_binding` 续有效绑定；过期绑定不复活。`hub rebuild` 的既有“不续租”特例按已批准 M0 保留，只执行投递读取；不能让 prelude 意外续租改变恢复规约。
- [ ] Ack 追加 `prompt.delivered`，actor=`process:towermd-cli`，data `{hold:<更新Hold>, message_ids, recipient_run, p3_request, p3_receipt}`，经append_p3_event先outbox fsync。已到回答也统一记该事件。重复ack新request_id仍单独归档receipt，非法token/未列 ID/另项目拒绝；epoch过期仍可记录已打印归档消息，无任何 task 状态转换。
- [ ] `HumanState::inbox` 从 Hold 当前投影与未送达集合生成消息，不逐 CLI 扫描全历史。`ProjectSession.sync` 增量应用；删除派生索引重建后结果一致，已 delivered 消息不重放。没有处理证明时 UI 只能排队中/已送达，不能显示已处理。
- [ ] **Step 5：绿验收。** 真实 CLI 在下一次非 status 命令的 stdout 首行得到插话；普通错误命令也先看到消息。故障注入 stdout partial/flush失败、daemon ack写入后回包前崩溃、CLI打印后ack前退出；重开后最多重复稳定ID，不丢消息、不重复扩张投影。
- [ ] **Step 6：提交。** 两层 `human_intervention` 测试与 `~/.cargo/bin/cargo test -p towermd cli_delivery` 通过后提交 `feat(cli): deliver intervention messages before project command output`，记录实际提交号。

## Task 4：App 本次回答、范围插话与暂停恢复请求

**Files:** 修改 core/session/daemon；扩展 `src-tauri/src/{commands,daemon_client}.rs` 与 `src/lib/daemon/{types,wire,client}.ts`；创建 `src-tauri/tests/intervention_adapter.rs`。等待 P6 基础这些文件落地，不创建平行 App 项目。

- [ ] **Step 1：有限 App 操作与卡片类型。** native 注册 `load_interventions(token)`、`answer_intervention(token,hold,text)`、`send_interjection(token,scope,text)`、`pause_project(token)`、`confirm_resume(token,request_id,pause_revision,run_ids)`；原生 ProjectRegistry 解 token；不接受 URL、argv、HTTP method、actor、任意 cwd。
- [ ] daemon `/app/interventions` 读当前 Hold；`POST /app/interventions/action` 使用下列 typed body。沿 P6 loopback/instance 校验，再通过有限 `AppGate` 区分客户端，只有此路由可构造 human actor；通用 `/command`、`/interaction` 拒绝 AppAction 与 human 字段。v0.1 客户端区分不声称密码学认证。

```rust
pub enum AppAction {
    Answer { hold: String, text: String },
    Interject { scope: Scope, text: String },
    PauseAll,
    ConfirmResume { request: String, pause_revision: String, runs: Vec<String> },
    Review { hold: String, binding: ReviewBinding, verdict: ReviewVerdict, comment: String },
}
pub struct AppRequest { pub directory: std::path::PathBuf,
    pub request_id: String, pub human_id: String, pub action: AppAction }
```

- [ ] `AppGate` 在 daemon 内私有，`validate_app(headers:&HeaderMap,endpoint:&Endpoint)->Result<AppGate,String>` 要求 instance 与固定 `x-towermd-client: app`；native client 仅这些固定路径加标记，CLI请求不加。human_id 为 native 当前本地身份，校验非空且无控制符/冒号；server 构造 `human:<id>`，请求中无自由 actor 字段。M0 已有 App 确认边界若提供同类 gate，复用而非建第二套。
- [ ] `InterventionCard` 为 `{id,kind,module,task,run,opened_ms,waited_ms,attention,question,options,recommended,context,reply,delivery,review_binding,paused_revision,affected_runs,actions}`；可空字段显式 null；kind 至少 ask/review/prompt/pause/resume。`actions` 仅当前实际支持操作，本次答复不出现存为决策/规则/总控代答。排序提供 priority/shape/color/label：review=5/圆/蓝/评审、ask=6/圆/黄/提问、pause=8/圆/灰/已暂停；其他类型由P2/P5合并，P6统一最高优先与同级等待降序。
- [ ] **Step 2：红测完整 actor 边界。** 在 daemon 测试调用实际路由：相同 Answer body 发 `/interaction` 或缺Appgate应4xx且无事件；带合法Appgate才写 hold.answered actor human；Relay只能process且带relayed_from。测试只验证程序边界，不写成人验收。

```rust
#[test]
fn relayed_reply_can_never_be_human_or_review() {
    let command = serde_json::json!({"action":"relay","hold":"H-1", "text":"通过",
        "self_reported":{"harness":"codex","version":"1"},"actor":"human:owner"});
    assert!(serde_json::from_value::<HumanCommand>(command).is_err());
    let review = serde_json::json!({"action":"approve","hold":"H-1"});
    assert!(serde_json::from_value::<HumanCommand>(review).is_err());
}
```

- [ ] **Step 3：执行红命令。** `~/.cargo/bin/cargo test -p towermd-core --test human_intervention relayed_reply_can_never_be_human_or_review -- --exact` 与 `~/.cargo/bin/cargo test -p towermd --test human_intervention_cli app_actor_`；预期新路由/解析拒绝尚未实现。
- [ ] **Step 4：最小实现。** App Answer/Review/Interject/Pause/ConfirmResume都经append_p3_event先outbox fsync并保存metadata/receipt；回答成功但下一快照前丢hub仍可经批3App确认恢复读到历史。App Answer 只能更新 Ask，拒绝以普通回答更新 Review；first answer获胜，同 request ID重试幂等，其他不同答复返回已回答冲突，P3不做推翻/存长期决策。迟到答复永久归档但不复活过期 recipient；提示文本涉及慢层仅作为文字，不改变对象或批准；实际慢层动作须P4提案流程。
- [ ] Interject scope 从权威 Task/Run 解析项目/模块/任务当时活动收件人；写 Prompt Hold 和 human事件；空范围显示0收件人，不宣称已送达。new run不补发过去提示，不直接写工作树。
- [ ] CLI `pause --all` 写process暂停事实；App PauseAll写human事实；两者先将完整Pause及冻结active_runs插话/receipt事件outbox fsync，再在同一序列中关claim并应用；hub写入未确定时不返回成功。ProjectSession同一锁序列中先读pause再调用Claim，不靠UI禁用按钮；已在claim途中按事件次序决定，先claim者纳入active集合。
- [ ] CLI `resume --all` 只建立Resume Hold，返回request ID/当前pause revision/affected runs，paused仍true。App确认必须显示并回传同项目/同revision/精确sorted run IDs；范围变化、旧revision、重复名字或缺项拒绝并提供最新范围。通过才将Pause.resumed=true，重试同ID幂等；不启动/恢复harness、不清G2/G3/G5/Review Hold、不替旧epoch acquire。
- [ ] **Step 5：绿验收。** 模块插话仅影响该模块活动run；全局暂停阻新领取且允许status/ask等待/归档/其他只读；无App确认resume不改变paused；过期旧恢复请求拒绝；App关窗后daemon继续；native固定命令无法通过参数变成其他HTTP操作。
- [ ] **Step 6：精确提交。** 核心/CLI绿后运行 `~/.cargo/bin/cargo test --manifest-path src-tauri/Cargo.toml --test intervention_adapter`、`node scripts/project-pnpm.mjs test -- src/lib/daemon/wire.test.ts`。提交 `feat(intervention): add App replies and scoped pause controls`。卡片渲染和实机样式留P6，不能据此声称真人已答。

## Task 5：P2 与人工评审事实的真实门禁联验

**Files:** 修改 `crates/towermd-core/src/{finish,recovery_git}.rs`、`crates/towermd-core/tests/{finish,recovery_inputs}.rs`、`crates/towermd/tests/daemon_cli.rs` 的P2调用点与对应测试；本计划core human reader是唯一来源。实施前读P2最终源码，文件名以 `docs/superpowers/plans/2026-10-17-m1-task-evidence.md` 已提交计划核对；不另建完整门禁引擎。

- [ ] **Step 1：写精确红测。** 在P2已有真实任务/冻结tree夹具中，risk high且没有currentPlan时finish返回blocked；已提交Plan但未批准仍blocked；App普通Answer文字“通过”或Relay不解锁。G1批准后仍须G4匹配验证tree；不得跳过G0/G2/G3/G5。
- [ ] 红测构造binding只使用本文件types：`let subject=session.human.current_plan_subject(&run.id)?;`，再从真实冻结run/task/baseline/fences构造 `ReviewBinding`，调用 `session.human.require_human_review(&binding)?;`；G4只将subject替换为 `ReviewSubject::Tree { tree: frozen_tree.clone() }`，不重新定义ReviewFact。
- [ ] **Step 2：红命令。** `~/.cargo/bin/cargo test -p towermd-core --test human_intervention review_`、`~/.cargo/bin/cargo test -p towermd-core --test finish`、`~/.cargo/bin/cargo test -p towermd-core --test recovery_inputs`、`~/.cargo/bin/cargo test -p towermd --test daemon_cli`；缺事实必须blocked，不能因为reader尚未接线直接成功。运行前用 `rg --files crates/towermd-core/tests crates/towermd/tests` 核实际测试路径，把本计划执行记录写成真实命令，不用0测试通过充数。
- [ ] **Step 3：最小实现。** P2守卫之后、验证之前检查currentPlan+精确G1；验证通过后、trailers/合并认证之前检查相同验证tree的G4。若G4在验证前已获批准，只能绑定同tree消费，改tree重新请求。P2验证不修改计划内容、不会创建review事实；缺事实走其现有blocked中断，不清理批准以伪装重试。
- [ ] `require_human_review`完整比较run/task/baseline/subject/排序后全部fences、human actor与Approved，消费时再次检查当前live fences和current subject；Task.touches第二模块epoch变化同样失效。已有事实仍归档，不修改成新epoch。
- [ ] **Step 4：绿验收矩阵。** 覆盖重提计划、只amend SHA而tree不变、tree变、baseline变、次模块epoch变、任务重领、另run复用、App重复批准、拒绝后重试、90秒/30分钟超时；旧内容批准永不解锁新内容，纯SHA变化且所有实际binding相同可复用。
- [ ] `review request --wait <hold>`沿Task2同一slice算法与90秒/10分钟预算；即使累计到deferred也不生成Approved。CLI没有review approve，timeout自动approve、retry清hardgate、普通问答替代review的参数均由解析测试拒绝。
- [ ] **Step 5：提交。** 执行P2 `check/finish/trailers`实际测试、P3核心/CLI回归后，`git diff --name-only`核责任，精确提交 `test(review): enforce human plan and tree facts in finish`，记录SHA；完整自动G1/G4/G6仍留M2。

## Task 6：快照恢复、故障窗与端到端验收交接

**Files:** 修改 `snapshot.rs`、session恢复接线；扩展 `tests/human_intervention.rs`、`tests/snapshot_v2.rs`、`tests/project_session.rs`、`human_intervention_cli.rs`；实现完成后更新双语未发布日志、`tasks/todo.md` 与新建 `docs/verification/2026-10-17-human-intervention.md`。验证文档只在真实实施之后登记证据。

- [ ] **Step 1：回放红测代码。** 下面仅测已记录事件投影，fixture-hash作为opaque元数据；产品mutation与hash验证另走真实路由/outbox测试。event由HubProtocol实际append返回，不能手填seq；在`ProjectSession::sync`后从StateProjection比较恢复值。

```rust
#[test]
fn hold_archive_survives_session_reopen() {
    let dir=tempfile::tempdir().unwrap();
    let mut hub=Hub::open(dir.path()).unwrap();
    let h=ask_fixture();
    let event=NewEvent{eid:"open-H-1".into(),host:"host".into(),
        actor:"process:towermd-cli".into(),kind:"hold.opened".into(),layer:"L5".into(),
        obj:h.id.clone(),data:serde_json::json!({"hold":h,
            "p3_request":{"request_id":"Q1","body_hash":"fixture-hash"},
            "p3_receipt":{"request_id":"Q1","body_hash":"fixture-hash",
                "result_ref":{"hold":"H-1","response":{"hold":"H-1","state":"pending"}}}}),guards:vec![]};
    hub.append(event,1).unwrap();
    let session=ProjectSession::new(Box::new(hub),1).unwrap();
    let saved=session.projection.clone();
    assert!(saved.objects["Hold"].contains_key("H-1"));
    drop(session);
    let session=ProjectSession::new(Box::new(Hub::open(dir.path()).unwrap()),2).unwrap();
    assert_eq!(session.projection,saved);
    assert_eq!(session.human.holds.len(),1);
}
```

- [ ] **Step 2：红命令。** `~/.cargo/bin/cargo test -p towermd-core --test human_intervention hold_archive_survives_session_reopen -- --exact`；预期Hold未投影或human未restore。依赖真实M0快照测试夹具继续增加checkpoint+tail路径，不能把只重开journal当完整恢复验收。
- [ ] **Step 3：最小实现。** `StateProjection::apply_recorded` 对hold.*、prompt.delivered验证data.hold是注册Hold且id=obj；hold.requested仅供receipt/历史索引，不覆盖有效Hold，其余有效事件更新objects["Hold"]；hold.opened 将opened_seq赋为event.seq，后续不允许修改该值；stale事件保留归档而不改变有效评审/运行判断。`HumanState.restore`从有效Hold投影重建指纹索引、inbox、wait预算、pause/review_facts；有效业务不从stale归档补状态。另按上方完整归档重建p3_requests与historical_replies；二者是派生视图，不能只靠currentHold恢复receipt或旧回答。
- [ ] 迟到answer是显式归档事件，不靠stale事件改变业务投影；正常迟到答复用既定归档语义写入。批3恢复outbox必须保留原guards，旧guards由hub标stale，严禁去guards/换epoch另发有效答案。独立只读历史视图从已验证归档读取含stale的原Reply（key=原reply.id），提供来源eid、原run/fences与historical_only标记；App归档卡与CLI归档读取可展示，不能进入有效inbox/actionable答案、Decision自动答复、require_human_review或run继续路径。hub.rebuilt后清有效运行/评审资格依据新的live fences，不删除历史答复；旧fences不被当新grant。共享契约confirmed失败uncertain、hub重复eid异body与Duplicate不能造Event测试必须覆盖实际append_p3_event入口。project身份、floor/连续seq、snapshot元数据校验继续沿M0，不另起cursor。
- [ ] `ProjectSession`创建、SnapshotRequired分支都restore human并按完整归档重建派生receipt/历史答复索引；sync逐连续Event同步tasks/human/projection，类型校验失败不推进cursor；checkpoint沿统一快照写入和main lease，不新增客户端文件写。无事件status/inbox不重放全历史，增加applied_count回归。
- [ ] **Step 4：故障矩阵。** 每项用确定fake clock/故障点验证一次，绿后不无故扩大循环：

| 故障/重复 | 必须证明 |
| --- | --- |
| open已append回包前崩溃 | 重试同request_id/hash先协调原pending/真实Event补confirmed，持续失败仍uncertain；耐久后返原receipt，异body拒绝，不丢recipient |
| 同问复用旧Hold且receipt已压缩 | 重开/SnapshotRequired仍返回该请求原结果，异body拒绝，不从当前Hold猜测 |
| ack已压缩且Hold后来改变 | 完整归档重建原receipt，同请求不重复投递，异body拒绝 |
| pending fsync失败/hub append失败 | 前者无hub事件；后者uncertain沿原eid/guards协调，不返回业务成功 |
| confirmed失败与同ID重试 | reopen/status仍见Hub真实Hold/receipt，原结果不改失败；持续fsync失败同ID不得success，解除只补原Event；archiveDuplicate仅凭已核完整原Event协调 |
| 后台Hold/taskexpire/pendingclaim producer | human_tick带显式outbox，wait_finished/attention及P2 task.released/待定claim协调均reopen核confirmed；lease系统事件未入outbox |
| 回答成功未checkpoint便删除hub | App确认批3恢复后历史答复可读；旧guards stale，旧run不能继续或批准 |
| wait started后daemon重启 | 90秒slice结算、累计10分钟不重置 |
| 回答到达与TTL同刻 | 答复归档；过期epoch不能继续/批准 |
| 30分钟attention前后重启 | 不漏升级、不重复有效关注，不自动回答 |
| stdout写完ack前CLI退出 | 同message ID重放，delivered幂等 |
| ack已写回包前崩溃 | 重试成功且不重放已确认消息 |
| SnapshotRequired/压缩/删缓存 | Hold/预算/recipient/delivery/pause/review一致 |
| 重建hub恢复旧答复/批准 | 历史保留，新epoch不能复用旧评审 |
| App换项目/实例后迟到响应 | 原生ProjectToken/instance校验拒绝旧请求 |
| pause与claim并发 | 唯一序列顺序，无漏掉的活动收件人 |
| resume范围变化/旧revision | 返回新范围，保持暂停待明确确认 |
| 第二模块被中断 | 无关模块领取、验证与投递正常 |

- [ ] **Step 5：真实产品路径自动验收。** 创建真实临时Git仓库、提交权威Module/Acceptance、建`t/T-1`工作树、启动实际daemon，CLI claim→ask→native固定App端点Answer→任意CLI取得首行答复→第二次不再重放；记录eid、Hold、Run、全部fences、快照through_seq。夹具HTTP操作写为自动集成，不写真人。
- [ ] 新增 `answer_survives_hub_loss_before_checkpoint_as_history`：真实CLI开问并经Appgate回答成功，另经Appgate完成一次G1评审；确认snapshot through_seq仍低于answer/review seq、runtime outbox完整保存两个原eid/guards/receipt；停止夹具daemon，仅删除夹具hub，保留outbox和Git。经实际 `hub rebuild` 请求及App确认路由恢复，历史视图读原reply.id/text和评审答复，两个原事件的外层replay各只有一条且旧guards仍stale（不写第二confirmed、不强等原接受回执）；旧epoch ask返回expired、require_human_review拒绝、未触发run继续。不得在测试中剥guards或直接塞History。Run：`~/.cargo/bin/cargo test -p towermd --test human_intervention_cli answer_survives_hub_loss_before_checkpoint_as_history -- --exact`。
- [ ] 新增 `request_receipts_survive_reuse_compaction_and_restart`：请求Q1开Hold，Q2同问复用但有独立receipt；回答/ack后写快照并压缩，再重启与SnapshotRequired分别重建；Q2同body返回原result_ref（即使Hold已答仍保持原请求结果）、异body409，ack同ID重试幂等且异message_ids409；异body校验必须先于当前run活动校验。最后删除派生索引重建仍一致。Run：`~/.cargo/bin/cargo test -p towermd --test human_intervention_cli request_receipts_survive_reuse_compaction_and_restart -- --exact`。
- [ ] 同夹具验证 `pause --all`、新claim拒绝、`resume --all`仍拒绝、App明确范围确认后可claim；验证真实high模块无事实blocked、G1/G4 App事实满足才按P2门禁继续，树内容变化再次blocked。
- [ ] **Step 6：绿命令与精确提交。** 运行 `~/.cargo/bin/cargo test --workspace`、`~/.cargo/bin/cargo fmt --all --check`、`~/.cargo/bin/cargo test --manifest-path src-tauri/Cargo.toml --test intervention_adapter`、`node scripts/project-pnpm.mjs check`。双语CHANGELOG未发布加对应用户变化，不把未实现界面记成完成；提交 `test(intervention): verify recovery and real daemon interaction boundaries`，记录实际SHA。
- [ ] **Step 7：交P6/P7真人步骤。** 提供打包App具体项目打开入口、待答Hold ID和预期卡片；真人点击本次回答/评审/恢复后记录human事件与下一CLI结果。P6实机验证首屏≤3、15.7形状/颜色/文字与等待脉动、切项目旧响应、关闭App不停止daemon。无人点击时状态写“真人验收未完成”。

## 自审与实施完成判据

- [x] FR-59/61/62：Task 1–2 精确归一化、权威决策、90秒/10分钟/30分钟、TTL和迟到回答。
- [x] FR-96/111：Task 1规格对齐，Task 2/4署名与有限Appgate；CLI无批准/Decision/Rule。
- [x] FR-109/112：Task 2–3任意项目CLI prelude、live续租、旧epoch不复活；恢复请求保留M0不续租特例。
- [x] FR-58：Task 1真实reader/App评审与Task 5唯一typed绑定，P2/P3先硬阻后解锁；完整G1/G4/G6不提前扩张。
- [x] 15.7/FR-68/74：Task 4提供卡片字段与投递状态，P6负责可见UI；没有processed假确认。
- [x] 派生可重建、一处定序/写入：Task 1统一outbox fsync→hub入口；Task 6含回答成功未checkpoint丢hub恢复、原guards stale历史视图和完整归档receipt重建；没有客户端直接写state目录。
- [x] 请求幂等：每个请求独立receipt包含request_id/body_hash/result_ref；同问复用、ack压缩、重启/SnapshotRequired仍查原结果，异body拒绝；正常调用不全历史重放。
- [x] FR-62模块边界：canonical module ID原样入指纹，A/a同问不同；仅问题文本NFKC/空白/lower/末标点归一化。
- [x] 类型一致：ReviewBinding/ReviewFact唯一由P3 Task 1定义；P2的current_plan_subject消费与P6的ProjectToken沿各自最终实现核对；没有临时默认放行API。
- [x] 计划中没有未定义的“后续补齐”实现步骤；前置批次变化必须先精确更新本计划，而非假装接口存在。
- [ ] 实施时每Task红→最小实现→绿→精确提交，实际提交号填入执行记录；本文件不代表代码或真人验收已完成。

当前状态：仅计划已编写，生产/测试源码未修改，依赖未安装，未提交。Task 1–6 的实现、测试命令及真人验收均未执行；首版是否发布须按发布总表P8冻结tag与实际产物核对。
