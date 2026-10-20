---
type: Plan
title: M0 整改批次 2：hub 协议、追加日志与增量投影
description: 联合实施 R6 与 R5，以协议隔离定序和业务解释，独立租约操作、追加并同步的 journal、可恢复检查点及增量业务投影一起完成验证。
tags: [towermd, plan, M0, remediation, hub, storage, TDD]
generated: { by: codex/gpt-6, at: 2026-10-17T00:00:00Z }
sources:
  - id: remediation
    resource: docs/superpowers/plans/2026-10-17-m0-remediation.md
    title: 用户已批准 D0–D6；本批执行 R6 与 R5
  - id: spec
    resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: FR-86 至 FR-93、6.3 与 16.3；R16 已提交 b5a881f
  - id: baseline
    resource: docs/superpowers/plans/2026-10-17-m0-fix-0-baseline-spec.md
    title: 整改基线 HEAD df5a8b5307a588dc987425cddc08d43159e6bd24
status: draft
---

# M0 整改批次 2：hub 协议、追加日志与增量投影 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 非心跳事件在截断与恢复之后仍可按 eid 去重；正常追加、续租、任务命令和 tick 仅处理新事件，业务状态能从 opaque 检查点加 tail 重建。

**Architecture:** `HubProtocol` 统一七个操作和连续前缀查询；hub 只定序信封、校验租约 fencing 并持久化系统元数据，任务和快照解释留在核心会话中。journal 的普通写入为单条 JSONL 追加加 fsync，ack 时以单文件原子替换折叠已覆盖前缀。协议、持久化、快照格式和调用方是一个联合闭环，内部逐个验证，全部可编译且安全回归通过后精确提交。

**Tech Stack:** 现有 Rust、serde/serde_json、uuid、fs2、tokio、axum、Git 和 tempfile；不新增 crate、Cargo feature、云服务、数据库或运行开关。

---

## 范围、冻结裁决与完成标准

本计划从已提交基线 `df5a8b5` 分析；执行前先确认批次 1 的提交号并重读其 `daemon.rs`、`recovery.rs` 变更，不能覆盖离线读取、后台刷新和写入前 fresh-fetch。当前文档是计划，所有实施复选框尚未完成。用户已批准全部推荐方案，不再请求裁决。

本批只有一个闭环 Task，下面各阶段各自有红测、最小实现和绿测。原因是旧接口将 `LeaseChange` 附在 append 上，旧快照将 receipts 与投影绑在一起；中途提交“新 hub + 旧 tasks”或“新快照 + 旧恢复器”不能形成可工作的软件。阶段通过后记录证据；联合验证通过后再提交。不能为追求每阶段提交而保留绕过新协议的生产路径。

冻结的边界：

1. `append` 不带租约变更，使用 `guards: Vec<Fence>`。任一 guard 不再有效，则整个事件照常定序、标 stale，不进入业务投影。顶层 `epoch` 为第一条 guard 的 epoch；完整 guards 是信封控制元数据，不从 `data.run.leases` 推断。
2. `lease_acquire/renew/release` 由 hub 产生 L5 `lease.granted/renewed/released`；到期由 hub 产生 L5 `lease.expired`。业务层另外产生 L5 `task.claimed/task.released`。客户端不能用普通 append 提交 `lease.*`。
3. 业务 checkpoint 是调用方提供的 `serde_json::Value`，hub 不反序列化为 `Snapshot`，不读取 commit、task、run 或其他业务键。hub 自有 seq、epoch、租约、ack、eid 索引独立于这份 opaque 值。
4. 普通追加、任务更新、续租、tick 和无新事件的订阅不能重放全部历史、重读全部 Git 归档或重写全部 journal。打开、迁移、压缩、authority 变化后的验证和恢复允许线性处理相应数据。
5. 非心跳 eid 索引、真实业务归档会随业务事件增长；本批不宣称总存储无限期固定大小。心跳按 seq 区间压缩，正常操作不随历史总量增长。
6. 恢复继续先 staging，再持久化 Git 边界，最后激活目标 hub；seq 与 epoch 跳过至少 2³²，旧租约作废。R3 私有引用、R4 App 端点/trailer/outbox 产品接入、R7 Git 重试在批次 3 实施，本批保持现有可验证恢复能力并留下协议接口。
7. 不改慢层审批、任务完成证据门禁、费用 null、CLI 身份；不实现 M2 Cloudflare hub。

实施时在规格 FR-87/16.3 加入以下最小澄清，不扩大原决定：

> FR-87 的客户端 eid 幂等适用于 append；lease.* 由 hub 产生，续租请求不携 eid、不进入发件箱，已省略续租事件不承诺恢复其历史 eid/seq。

普通 `append` 的非心跳 eid 跨截断与恢复保留原 seq。续租请求不是携带旧 eid 的重放；hub 自产的续租事件在未截断事件流中正常可读。`lease.granted/released/expired` 不是心跳，仍归档并保留 eid 索引。不要用“全部 eid 永久幂等”的宽泛措辞掩盖此边界。

## 文件地图与边界

| 文件 | 修改责任 |
| --- | --- |
| `crates/towermd-core/src/hub_protocol.rs`（新建） | 协议类型、七个操作、连续前缀查询、结构化错误 |
| `crates/towermd-core/src/hub.rs` | 本地协议实现、信封/fencing、租约自产事件、系统 checkpoint、内存 eid 索引 |
| `crates/towermd-core/src/hub_journal.rs`（新建） | JSONL 追加、fsync、断尾恢复、原子压缩、poison；不解释业务 checkpoint |
| `crates/towermd-core/src/hub_legacy.rs`（新建） | 核心组合层的只读 v1 journal/快照兼容转换；隔离旧 Snapshot/LeaseChange 格式，不由 hub 解释旧业务快照 |
| `crates/towermd-core/src/events.rs`（新建） | `derived_eid` 与调用方提交 SHA 校验；不放入 hub 实现 |
| `crates/towermd-core/src/lib.rs` | 导出协议与事件工具；journal/legacy 保持内部模块 |
| `crates/towermd-core/src/tasks.rs` | `TaskService` 增量投影、独立领取/续租/释放与补偿 |
| `crates/towermd-core/src/snapshot.rs` | 无 receipts 的新快照、归档 eid 推导、范围覆盖、业务事件应用 |
| `crates/towermd-core/src/project_session.rs`（新建） | 一个常驻核心会话，组合协议对象、增量任务/快照投影与 authority 缓存 |
| `crates/towermd-core/src/snapshot_service.rs` | 从连续前缀构建快照，Git 成功后 opaque ack，增量 due/authority 验证 |
| `crates/towermd-core/src/recovery.rs` | 核心解释恢复来源，协议 staging/激活；保持 AppConfirmation 私有 |
| `crates/towermd/src/daemon.rs` | 使用核心会话和协议；删除 journal.json 的直接读取及具体 Hub 业务依赖 |
| `crates/towermd-core/tests/hub.rs`、`hub_contract.rs`（新建） | 本地故障回归、可供后续实现复用的协议契约 |
| `crates/towermd-core/tests/tasks.rs`、`snapshot.rs`、`snapshot_service.rs` | 生命周期、快照与增量行为 |
| `crates/towermd/tests/daemon_cli.rs` | 真实 daemon/CLI、订阅、重启、离线续租回归 |
| 规格、README、双语 CHANGELOG、`tasks/todo.md` | 最小契约澄清、可见变化、验证与精确提交证据 |

现有大文件不做全量整理。把新增持久化和兼容代码拆出去，是为了把业务解释隔离于 hub；不顺手移动其他业务模块。测试专用 I/O 故障注入与计数器不得变成可绕过门禁的生产参数。

## 真实接口约定

以下类型名称和字段贯穿计划，实施不能在各阶段另起一套不相容名称。现有 `Event`、`Fence`、`Lease`、`AppendReceipt` 可移动或 re-export；不保留生产使用的旧 `append(event, LeaseChange, now)`。

```rust
pub trait HubProtocol: Send {
    fn append(&mut self, event: NewEvent, now_ms: u64)
        -> Result<AppendReceipt, HubError>;
    fn subscribe(&mut self, after: u64, now_ms: u64)
        -> Result<SubscriptionBatch, HubError>;
    fn lease_acquire(&mut self, request: AcquireLease, now_ms: u64)
        -> Result<LeaseReceipt, HubError>;
    fn lease_renew(&mut self, request: RenewLease, now_ms: u64)
        -> Result<LeaseReceipt, HubError>;
    fn lease_release(&mut self, request: ReleaseLease, now_ms: u64)
        -> Result<LeaseReceipt, HubError>;
    fn snapshot_ack(&mut self, ack: SnapshotAck, checkpoint: serde_json::Value)
        -> Result<(), HubError>;
    fn rebuild(&mut self, seed: RebuildSeed)
        -> Result<RebuildReceipt, HubError>;
    fn contiguous_seq(&self) -> u64;
}

pub struct NewEvent {
    pub eid: String,
    pub host: String,
    pub actor: String,
    pub kind: String,
    pub layer: String,
    pub obj: String,
    pub data: serde_json::Value,
    pub guards: Vec<Fence>,
}

pub struct AcquireLease {
    pub host: String,
    pub names: Vec<String>,
    pub holder: String,
    pub ttl_ms: u64,
}
pub struct RenewLease {
    pub host: String,
    pub fences: Vec<Fence>,
    pub ttl_ms: u64,
}
pub struct ReleaseLease {
    pub host: String,
    pub fences: Vec<Fence>,
}
pub struct LeaseReceipt {
    pub event: Event,
    pub leases: Vec<Lease>,
}
pub struct SnapshotAck {
    pub commit: String,
    pub seq: u64,
    pub epoch: u64,
}
pub struct HubHead {
    pub floor_seq: u64,
    pub contiguous_seq: u64,
    pub epoch_high_water: u64,
    pub ack: Option<SnapshotAck>,
    pub leases: std::collections::BTreeMap<String, Lease>,
}
pub struct SubscriptionBatch {
    pub events: Vec<Event>,
    pub head: HubHead,
}
pub enum HubError {
    Invalid(String),
    LeaseConflict { owners: Vec<Lease> },
    FenceExpired { fences: Vec<Fence> },
    SnapshotRequired { head: HubHead, checkpoint: serde_json::Value },
    Uncertain(String),
    Io(String),
}
pub struct RebuildSeed {
    pub source_seq: u64,
    pub source_epoch: u64,
    pub seq_boundary: u64,
    pub epoch_boundary: u64,
    pub checkpoint: serde_json::Value,
    pub receipts: std::collections::BTreeMap<String, u64>,
    pub tail: Vec<Event>,
}
pub struct RebuildReceipt {
    pub contiguous_seq: u64,
    pub epoch_high_water: u64,
}
```

实际类型补齐与现有 serde 一致的 derives、`kind` 的 `#[serde(rename = "type")]`、错误的 Display/Error；`Event` 保留 `cost_usd: Option<f64>`、完整 guards 以及 hub 自有租约控制元数据。固定 `HubError` 使 `Box<dyn HubProtocol>` 可直接使用，不引入关联错误泛型传播。错误在 CLI JSON 边界保留结构化 owners，再生成用户可读描述。

`AppendReceipt` 保留现有 `Event(Box<Event>)` 与 `Duplicate { seq }`：未压缩重复可返回原正文，已压缩重复返回原 seq；二者都不执行新 guards 或新租约变更。`rebuild` 的 tail 是 staging hub 已定序并持久化的事件，安装时验证 seq/信封/系统租约控制一致性，不能解析其业务 data；空 tail 是新 staging seed。`source_epoch` 明确指来源快照的 `epoch_high_water`，不能以较小的 writer_epoch 代替。业务方验证项目 ID、Git 来源及人工授权，hub 验证边界至少各跳 2³²、目标边界严格前进、tail 连续和非重复。

`subscribe(after, now)` 在合法游标下执行到期租约维护，再返回增量事件和当前头；tick 调这个操作，不新增第八个写操作。游标小于 floor 必须返回 `SnapshotRequired`，大于连续水位拒绝，等于 floor 可以取 tail。过期事件与客户端追加是独立动作：非法追加本身不消耗 seq，不能把维护产生的 seq 伪称为该拒绝请求的 seq。

## Task 1：联合替换 hub 协议、存储与调用方

### 阶段 A：协议契约与不透明负载

**Files:** Create `hub_protocol.rs`、`events.rs`、`tests/hub_contract.rs`；Modify `hub.rs`、`lib.rs`。

- [ ] **A1：编写协议测试的固定输入。** `tests/hub_contract.rs` 使用真实临时本地 hub，所有断言通过 `&mut dyn HubProtocol`。定义下列 helper，不用特殊 git data 夹具使 hub 偷读业务内容。

```rust
fn event(kind: &str) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "contract-host".into(),
        actor: "process:towermd-cli".into(),
        kind: kind.into(),
        layer: "L5".into(),
        obj: "contract-object".into(),
        data: serde_json::json!([null, {"commit": false}, 17]),
        guards: Vec::new(),
    }
}

fn sequence(receipt: AppendReceipt) -> u64 {
    match receipt {
        AppendReceipt::Event(event) => event.seq,
        AppendReceipt::Duplicate { seq } => seq,
    }
}

fn opaque_append_contract(hub: &mut dyn HubProtocol) {
    let input = event("arbitrary.event");
    let first = sequence(hub.append(input.clone(), 10).unwrap());
    assert_eq!(first, hub.contiguous_seq());
    assert_eq!(sequence(hub.append(input, 11).unwrap()), first);
    let batch = hub.subscribe(first - 1, 12).unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.events[0].data, serde_json::json!([null, {"commit": false}, 17]));
}
```

- [ ] **A2：运行红测。** `cargo test -p towermd-core --test hub_contract --locked`。先补齐类型导出使测试可运行，再确认实际偏差：旧 hub 对确定性 eid 读取 data.commit，普通 append 能携带租约操作，或 trait 调用路径未接入。仅缺 import 的编译失败不算行为红测。
- [ ] **A3：加入完整业务无关的信封校验。** UUIDv7 规则、host/actor/type/layer/obj、guard 名称/holder/正 epoch 延续现有约束；`git:` eid 只检查确定性哈希 token 的结构，不读取 data 验证 SHA。`events::derived_eid(commit, kind)` 在调用方验证完整 SHA并计算 hash，测试相同 SHA/type 同 eid、不同 type 不同 eid。普通 append 拒绝 `kind.starts_with("lease.")`。
- [ ] **A4：加入两个行为测试并通过。** `derived_eid_does_not_require_hub_to_read_commit` 使用调用方算出的 git eid、任意数组 data，append 成功；`reserved_lease_type_cannot_be_client_appended` 对四种 lease 类型逐个断言 Invalid、continuous 水位未变。普通未知业务类型仍可追加。
- [ ] **A5：运行绿测。** `cargo test -p towermd-core --test hub_contract --locked`。记录已经成立的 opaque/幂等/拒绝不占 seq 契约；完整旧 tests 的 API 迁移在阶段 H 联合完成。

### 阶段 B：独立租约操作与全部 guards

**Files:** Modify `hub.rs`、`hub_protocol.rs`、`tests/hub_contract.rs`、`tests/hub.rs`。

- [ ] **B1：写批量租约与结构化占用红测。** 第一批 acquire `[module:A,module:B]`、holder `run-1`；第二批 `[module:B,module:C]`、holder `run-2` 返回 `LeaseConflict`，owners 精确含 B 的 holder/epoch/expiry，C 不出现、epoch 高水位不变。成功 acquire 只有一次原子租约控制记录，所有更新随该记录 fsync 后生效。
- [ ] **B2：写所有 guards 的红测。** 取得 A/B，将 A 有效 fence 与 B 已失效 fence 一起放入 append 的 guards；事件必须 stale，epoch 是 A 的 epoch，业务投影不更新。两个 guards 均有效则不 stale。guard 格式无效或重复名称则 Invalid，不占 seq。不得只校验 `guards.first()`。
- [ ] **B3：实现 lease 准备与提交。** 沿用现有批量预检、checked_add epoch/TTL 校验，但不先 clone 全部历史或全部 lease map。先校验整批，创建不可变 lease delta 与自产 UUIDv7 Event，持久化单条后 apply。hub actor 固定 `process:towermd-hub`，host 来自已校验操作参数，类型 L5；Event 顶层 epoch 是第一条更新/释放 fence 的 epoch，全部 epoch 位于系统 lease delta。
- [ ] **B4：实现到期维护。** `subscribe` 使用当前 leases 的到期索引找到到期项；产生 `lease.expired` 后移除它们。重复轮询同一到期项不能重复产生 expired；renew 时同步更新到期索引，旧 deadline 不能使新有效租约过期。内存索引从系统 checkpoint+tail 重建，不保存唯一状态。
- [ ] **B5：写并通过租约生命周期测试。** 时间 `10` grant TTL `100`，`50` renew TTL `100` 保 epoch，`150` subscribe 产生 expired，随后 renew 旧 fence 返回 FenceExpired；`151` 重新 acquire epoch 必须增加；旧 release 不得删除新 holder。四种 lease 事件的 layer/epoch/actor 均有精确断言。
- [ ] **B6：运行绿测。** `cargo test -p towermd-core --test hub_contract --locked`，并运行改写后的 `acquisition_is_atomic`、`renew_is_atomic`、`expired_release` 回归筛选。只迁移调用语法，不削弱旧 holder/epoch 安全断言。

### 阶段 C：单条追加、失败结果与 v1 迁移

**Files:** Create `hub_journal.rs`、`hub_legacy.rs`；Modify `hub.rs`、`lib.rs`；Test journal 模块单元测试与 `tests/hub.rs`。

- [ ] **C1：写 10,000 条追加红测。** 初始 header 已持久化后，每条记录前后采集文件长度；断言增量精确等于该 Event JSON 字节加一个换行。macOS/Unix 另用 MetadataExt 断言 inode 不变；普通追加没有 journal 临时文件、rename 或目录 fsync。计数器断言 serialize/write 的 Event 数量每次为 1，不能用墙钟基准代替。
- [ ] **C2：实现版本化单文件日志。** 新文件为 `journal.jsonl`，第一行 `JournalHeader { version: 2, checkpoint: SystemCheckpoint }`；随后每行一条 Event。`SystemCheckpoint` 明确包含 floor seq、epoch 高水位、该 floor 上的 hub 租约状态、非心跳 receipts、ack 和 opaque checkpoint，不能包含 `Snapshot` 类型。空 hub 的 floor/epoch 为 0、租约/receipts 为空、opaque 值为空对象。

```rust
fn encode_record<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn append_record(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    file.write_all(bytes)?;
    file.sync_all()
}
```

上面编码在开始写入前完成；任何开始写入后的错误都由上层设置 poison。普通 append 在 `append_record` 成功后才 push Event、更新 eid 索引与 lease delta、对订阅可见。初次创建 journal 必须同步文件和父目录，不能把创建窗口当成普通追加。

- [ ] **C3：写结果不确定红测。** 注入“完整行已写但 sync_all 失败”和“只写半行后失败”；内存水位均不变，当前实例任何追加、重复 eid 返回、ack、rebuild 都拒绝 Uncertain。drop/reopen：完整行存在时同 eid 返回原 seq且只出现一次；半行被截掉后重发只出现一次，完整已确认前缀不变。
- [ ] **C4：实现严格打开。** 仅最后一段没有换行的 bytes 可截断到最后换行并 fsync；完整行即使 JSON 无效也报损坏。校验连续 seq、eid 唯一、顶层 epoch/stale 与 guards、hub 系统 lease delta；重播不解析业务 data。开放的文件始终指向实际当前 journal，不能在 rename 后继续向旧 inode 写入。
- [ ] **C5：写完整行损坏及边界红测。** 非法 JSON 完整行、seq 重复/空洞、篡改 epoch/stale、伪造内部 lease delta、坏 header、unsupported version 各自拒绝打开；不得降级到空 hub。有效 JSON但无换行的尾段按未提交记录丢弃，绝不发布其 seq。
- [ ] **C6：固定 v1 迁移夹具。** 从基线旧 API 生成一份包括 acquire/task.claimed/renew/release、snapshot_ack、恢复 base_snapshot 的 v1 journal；将固定 JSON 放入 journal 模块测试字符串，不依赖新 API 伪造旧格式。`hub_legacy` 保留 v1 反序列化类型，只在迁移分支解析旧业务 Snapshot，将其转换为 opaque checkpoint，并把旧单 fence 规范化为 guards。
- [ ] **C7：实施迁移事务。** 核心本地会话工厂取得原 `hub.lock`，在锁内调用 `hub_legacy` 严格验证并转换 v1；写 v2 临时文件，fsync、rename 为 journal.jsonl、目录 fsync；仅成功后将同一锁与已打开存储交给本地 hub，不能释放再重新抢锁。`Hub::open` 的生产内部只读 v2；需要迁移的调用走核心工厂，不能由 hub 持有或解释旧 Snapshot。旧 journal.json 留作只读迁移备份，永远不再写；v2 已存在时只认 v2，不能把旧历史拼回去。旧文件损坏不能创建新的空 v2。临时文件不作为已安装日志来源。
- [ ] **C8：写迁移每个故障窗口并通过。** temp 写/同步失败保留 v1；rename 前失败无有效 v2；rename 后目录同步失败当前实例 poison，reopen 读取完整 v2；两个文件并存只用 v2；重开迁移幂等不再序列化全部历史。v1 旧 claim 的已记录 lease delta保持历史含义，不能迁移时重发 grant 或改动旧 seq/eid。
- [ ] **C9：运行绿测。** `cargo test -p towermd-core hub_journal --locked`、`cargo test -p towermd-core hub_legacy --locked`、`cargo test -p towermd-core --test hub --locked`。后者中旧 append API 的夹具在此改成协议方法；rename 故障只保留在压缩/安装测试，普通追加故障使用真实 I/O 注入。

### 阶段 D：ack 压缩与 opaque 检查点

**Files:** Modify `hub.rs`、`hub_journal.rs`、`tests/hub_contract.rs`；保留 `hub.rs` 原 poison/恢复安装锁单元测试的安全断言。

- [ ] **D1：写 ack 红测。** 追加业务事件、grant、1000 次 renew；提供数组形式 opaque checkpoint 与 ack，断言 journal 变小、非心跳旧 eid 返回原 seq、旧 renew eid 不在持久 receipts、`subscribe(floor - 1)` 返回 SnapshotRequired、`subscribe(floor)` 正常。opaque 值重开后逐值相等，hub 不要求它符合 Snapshot schema。
- [ ] **D2：写 ack 前缀与 tail 红测。** 取得租约后在快照点之后 renew/release，再 ack 较早快照点；压缩后的 header 必须是该点的租约系统状态，tail 完整保留。重开后 current leases与压缩前一致；不能把“当前租约状态”塞入旧 floor 然后重复应用 tail。
- [ ] **D3：实现压缩。** 从旧 system checkpoint 增量到 ack.seq 计算前缀系统状态及非心跳 eid 索引；业务值仅接收调用方参数。用临时文件写新 header+`seq > ack.seq` tail，同步后 rename、目录 fsync，再替换句柄与内存。append tail 的索引改为 seq/eid 定位，不依赖旧 Vec 下标。历史重复 eid 查非心跳索引，保留尾部正文时可返回 Event，否则 Duplicate。
- [ ] **D4：实现 ack 拒绝规则。** seq 超连续前缀、seq 后退、epoch为0或超 epoch 高水位均拒绝；同 seq 必须具有相同 epoch 与相同 opaque checkpoint，内容不同拒绝。完全相同 ack 且相同 checkpoint 为无写幂等；同水位、同checkpoint但commit改变允许更新ack标识，为D1私有引用搬送到main保留路径。搬送是否为相同快照/归档的合法Git提交由core验证，hub仅比较opaque值，不读Git或解释其业务字段。不能允许以同ack偷换opaque内容。
- [ ] **D5：注入 compaction 故障并验证。** rename 前失败保旧文件和可用内存；rename 后目录同步失败 poison，当前实例不能以旧 by_eid 返回成功；reopen 要么旧完整代际要么新完整代际，不能混合。成功后再次 append 确认新 journal包含该行、旧 inode没有继续被写。重复 ack 计数为0次写。
- [ ] **D6：运行绿测。** `cargo test -p towermd-core --test hub_contract --locked`、`cargo test -p towermd-core hub::tests --locked`、`cargo test -p towermd-core hub_journal --locked`。本阶段不能把原 `ack_sync_failure`、`recovery_poison` 的断言删除来换取通过。

### 阶段 E：无 receipts 的快照、归档推导和心跳区间

**Files:** Modify `snapshot.rs`、`snapshot_service.rs`、`tests/snapshot.rs`、`tests/snapshot_service.rs`；兼容读取留在 `hub_legacy.rs` 或 Snapshot 的明确 legacy decoder。

- [ ] **E1：写心跳快照红测。** grant main、取得 writer fence后分别生成 1 次与1000次连续 renew，生成新快照；`serde_json::to_value` 两者都没有 receipts，omitted 各只有一个合并区间，projection 与非心跳归档相同。允许 through_seq 的十进制长度变化，断言不能错误地要求 JSON 总字节严格相等。
- [ ] **E2：去掉新格式 receipts。** 新快照 format_version 为2，保留 project_id、through_seq、writer_epoch、epoch_high_water、projection、omitted、recovery_gaps。业务投影从 task 事件 data 读取 Run.leases，不再从 task.claimed 的 hub lease_delta补租约。只在 core解释业务；lease summary从 hub自产事件的系统控制元数据更新。
- [ ] **E3：实现归档证明。** `validate_metadata` 验证版本、epoch、区间合法/不重叠，不再依赖 receipts虚构覆盖证明。`validate_archive` 一次线性遍历归档，构建 `BTreeMap<eid, seq>`，拒绝重复 eid/seq、顺序错误、纯心跳归档；把 archive单点、omitted和recovery_gaps排序合并证明 `[1, through_seq]` 覆盖，并重播业务比较 projection。范围遍历禁止枚举每一个省略 seq。
- [ ] **E4：保留旧快照读兼容。** v1读取校验旧 receipts/覆盖关系，利用归档获得非心跳 eid；进入内存规范化为 v2，并在下一次真实快照写出 v2。读取不能直接改 Git。旧省略续租的eid不会进入新state或非心跳索引；旧 journal中无法确定纯心跳的 legacy续租保守留正文，不丢任意业务data。
- [ ] **E5：补齐窗口与字段红测并通过。** 跨两次快照连续心跳合并；心跳与真实业务交错保留对应多个区间；重复归档eid、归档缺业务事件、投影篡改、区间重叠/反转/0/溢出、2³² recovery_gap均拒绝或按原安全语义通过。Task/Run未知字段与cost null往返保留。
- [ ] **E6：运行绿测。** `cargo test -p towermd-core --test snapshot --locked`。将 `receipts_include_heartbeats_but_state_json_has_no_event_bodies` 改为 `state_json_has_no_receipts_and_heartbeat_ranges_are_merged`，把 duplicate receipts旧格式测试归入legacy，不删除归档一致性证明。

### 阶段 F：任务常驻投影与领取补偿

**Files:** Modify `tasks.rs`、`tests/tasks.rs`；Create `project_session.rs`；Modify `lib.rs`。

- [ ] **F1：引入常驻服务与确定的应用接口。** `TaskService` 持有现有 tasks/runs map与applied_seq；提供 `restore(&Snapshot)`、`apply(&Event)`、`execute(&mut dyn HubProtocol, ...)`、`renew_binding`、`expire`。原 replay中的对象ID/业务字段校验迁入 restore/apply。`ProjectSession` 持有 `Box<dyn HubProtocol>`、TaskService、StateProjection和cursor；业务应用成功才推进cursor。StateProjection继续使用现有按注册类型组织的objects map，事件apply在core按kind分派，不将协议或checkpoint schema写死为Task/Run；后续批的merge.done、evidence.recorded可自然进入相应业务投影，恢复后仍能从归档重放。它们的业务实现由对应后续批负责，不能仅写日志保存唯一证据。
- [ ] **F2：写增量红测。** restore一次后应用N个tail事件，连续执行100次status与100次tick无新事件，业务apply计数增加0；追加一条task事件后增加1。销毁整个会话，只用协议 checkpoint+tail重建，得到同一 Task/Run/成本；不能从测试留下的旧service复制map充当重建。
- [ ] **F3：按增量同步替换全量 replay。** `subscribe(cursor, now)` 取新事件，依次应用任务与通用StateProjection；SnapshotRequired时反序列化core Snapshot、restore、再从floor订阅。tail事件用`partition_point`定位，不能每次扫全部Vec。相同事件只apply一次；遇未知业务kind忽略业务但推进有效cursor。
- [ ] **F4：写领取语义红测。** claim产生L5 grant再L5 task.claimed；Run.leases为完整fences、claim guards含全部fences、epoch是primary；task卡渲染失败发生在acquire之前，不留grant。claim追加确定失败后补release，task/runs保持ready/空，可能留下grant/release审计事件。
- [ ] **F5：实现领取顺序。** 保持G0、依赖merged、分支和worktree绑定、可领取建议；先完成所有本地可失败校验和渲染，再acquire。将leases放进Run后append task.claimed，不能把hub自产lease_delta挂在该业务事件上。确定拒绝时只释放本次取得且仍匹配的fences；补偿失败返回原错误与补偿错误，等待有效租约到期，不能报告成功。
- [ ] **F6：写不确定追加的安全测试。** grant成功、claim完整行写入后fsync报错：当前实例poison，不能假装claim失败而继续release。core在发出append前保留完整待追加NewEvent及run ID；不确定错误中返回实际claim eid与run ID，当前会话待协调项保留同一事件，不能丢掉eid后新生成一个去“重试”。重开先从完整记录恢复投影；仍持有待协调项时用同eid取得原receipt，已记录则恢复running与有效leases，确定未记录时按有效guards协调追加或补release。进程退出后内存待协调项可丢弃：完整记录从tail恢复，未形成完整记录的孤立grant按TTL回收，不宣称已立即回滚或已完成跨进程补偿。测试经真实任务入口捕获返回的eid/待协调状态再重开，不由测试私下手握一个产品未保留的eid。错误消息明确“结果不确定，重开后协调”。
- [ ] **F7：实现放弃/过期。** 显式放弃仅release该run当前有效fences，然后append L5 task.released；失效run由自动流程append L5 task.released，actor分别保持`process:towermd-cli`与`process:towermd-daemon`。释放旧run不能损伤替代holder。部分释放持久化后业务append失败时，下次tick从当前leases与running run再协调，最终产生一次有效released投影。
- [ ] **F8：运行绿测。** `cargo test -p towermd-core --test tasks --locked`。重写失败claim“seq完全不变”为“无running投影且已补偿”；保留错误branch/host不续租、旧epoch不投影、G0/保护路径、harness不可信、未知费用null和真实worktree不被修改。

### 阶段 G：snapshot_service、authority 缓存与协议 staging

**Files:** Modify `snapshot_service.rs`、`recovery.rs`、`project_session.rs`、`tests/snapshot_service.rs`；保留recovery.rs单元测试。

- [ ] **G1：写生产连续前缀红测。** 测试替身通过HubProtocol提供可见事件103但连续前缀101；snapshot_service只能构建到101，102未到不能写103。102到达后推进到103。测试调用真实SnapshotWriter路径，而不是孤立EventBuffer。删除未接入的EventBuffer及其自证测试，保留相同安全断言到生产契约测试。
- [ ] **G2：搬走 Hub::checkpoint。** 快照点固定取 `contiguous_seq()`；core从opaque基线、已经应用的状态及新tail构建Snapshot，核验当前main fence与writer_epoch，再交现有Git写入器。Git成功后core生成snapshot.written并append，然后调用snapshot_ack提供本次Snapshot的opaque值。snapshot.written在快照点之后，是下次tail，不能错误截掉。
- [ ] **G3：写快照提交后ack失败回归。** Git成功、ack失败重开后从权威Git补确认；确定性snapshot.written只出现一次，未写出的快照不ack。调用方校验完整SHA与trailer，hub只存ack标识；补确认和业务checkpoint需对应同一through_seq/epoch。另写同水位相同checkpoint不同commit更新ack的协议测试与不同checkpoint拒绝测试；core只在证明(epoch,seq)、state与归档等价的合法搬送后发出更新，批3接入实际private→main搬送，不能一律禁止同水位换commit或放宽为任意不等价替换。
- [ ] **G4：实现authority验证缓存。** 缓存键为项目的权威revision与hub ack身份/水位；相同键不读归档、不重播历史。键变化时读Git快照/归档验证一次，与本地opaque基线+tail相符后更新缓存；不相符保持“须App恢复”错误。若保存快照早于floor，按已确认ack识别回退，不能强行从已删tail重算也不能默默接受回退。没有唯一业务状态留在缓存。
- [ ] **G5：写并实现增量due状态。** 核心保存最近snapshot.written时间与ack后第一条真实业务事件时间；只由新事件更新，忽略纯renew和main租约维护。tick不再调用现有due中的`hub.events(0)`；重启从checkpoint+tail或已验证Git快照恢复这些派生值。计数器证明无变化tick不读归档/不apply历史。
- [ ] **G6：将恢复staging接到协议。** 在恢复基础设施的本地工厂里打开临时Hub并立即擦除为`Box<dyn HubProtocol>`，使用`rebuild(empty-tail seed)`装载新边界；core从source Snapshot构建含recovery_gap的opaque seed，归档推导receipts。按现顺序追加补发snapshot.written、人工审批、hub.rebuilt、L5 task.released，再acquire main并生成快照。只在Git成功后，将同一seed及staging订阅的完整tail交给目标rebuild安装；target锁持续持有，current水位须仍等于最初读取值。
- [ ] **G7：保持恢复授权隔离。** AppConfirmation字段与构造留在core私有模块；本批不开放CLI批准入口。为批3预留公开核心编排方法，由它内部构造并校验request ID/human actor/expiry，daemon的App客户端检查在下一批接入。不能要求跨crate daemon直接调用core的pub(crate)构造函数。项目ID/source/回退等业务检查在核心执行，再进入不解释opaque内容的hub安装。
- [ ] **G8：运行绿测。** `cargo test -p towermd-core --test snapshot_service --locked`、`cargo test -p towermd-core recovery::tests --locked`。保持先Git后激活、重建高水位不可重用、锁不释放、非推进边界拒绝、原租约失效；opaque数组协议测试与业务Snapshot恢复测试分别存在。

### 阶段 H：daemon 产品路径与历史测试迁移

**Files:** Modify `crates/towermd/src/daemon.rs`、`crates/towermd/tests/daemon_cli.rs`；同步批1对刷新/离线续租的实际调用接口。

- [ ] **H1：把项目状态改为常驻核心会话。** Project持有`Arc<Mutex<ProjectSession>>`，Hub的创建只在基础设施composition函数中发生；daemon/tasks的业务函数参数不出现具体Hub。快照取点/打包时持锁，Git网络/提交阶段不持会话锁，写前回调重新验证main fence。锁顺序固定writer锁→短暂session锁，tick不得反向等待writer锁。
- [ ] **H2：删除daemon读journal.json的格式依赖。** `renew_existing_read`与`rebuild_request`当前用read+serde_json判断journal存在/有效，应改成核心本地存储探测/会话打开结果；探测识别v2与v1，损坏显式错误，不能在daemon解析任一格式。原“无hub且无显式run则跳过续租”保持；不能破坏批1的authority_unavailable状态。
- [ ] **H3：接入订阅与tick。** HTTP订阅初次与每次轮询都调用protocol subscribe；客户端游标落后于运行中压缩floor时，返回/发送明确SnapshotRequired后关闭当前流，不能静默空流。取返回tail使用seq索引，订阅延迟仍保持原100ms轮询与2秒send timeout。tick只维护当前租约、失效run和增量due，不读Git归档。
- [ ] **H4：写真实daemon红测。** 两个worktree add/claim/release，WS观察L5 grant/claim/release；快照ack后旧游标被要求加载快照；重启后任务状态相同。带origin但不可达时status/show及绑定续租继续成功，保留批1的main刷新元数据；命令不因迁移日志扩展名而跳过续租。
- [ ] **H5：迁移故障测试，不按旧物理实现造假。** `failed_rename_preserves_previous_journal`用于压缩而非append；替换路径为directory无法使已有append句柄写失败，因此普通write/fsync故障必须走测试注入。`valid_json_with_corrupt_sequence_epoch_or_stale`修改JSONL具体行；两段式claim/release的事件数与actor按新契约断言。旧历史仍保持旧语义，不能迁移时重写L4旧事件冒充新产生L5。
- [ ] **H6：运行绿测。** `cargo test -p towermd --test daemon_cli --locked`，再运行 `cargo test -p towermd-core --locked`。结果中分开记录“新契约替换的旧断言”与“保留安全回归”，不能以测试总数下降代替解释。

### 阶段 I：联合验收、文档与精确提交

**Files:** 前述本批文件；Modify规格、README、双语CHANGELOG、`tasks/todo.md`。

- [ ] **I1：一次运行完整必需检查。**

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

预期全部通过。若既有工具链检查在基线本就失败，记录具体原始证据与本批影响，先修本批新增失败；不得把未执行写成PASS。仅Rust/协议改动不必重复无关UI样式验收。

- [ ] **I2：检查职责和性能证据。**

```bash
rg -n 'data\[|data\.get|Snapshot|derived_eid|data.*commit' crates/towermd-core/src/hub.rs crates/towermd-core/src/hub_journal.rs
rg -n 'Projection::replay|events\(0\)|journal\.json|Mutex<Hub>|&mut Hub|&Hub' crates/towermd-core/src/tasks.rs crates/towermd/src/daemon.rs
git diff --check
```

第一条不得有业务解析或Snapshot类型依赖；第二条不得有生产全历史重播、journal物理解析或具体Hub业务参数。允许测试、兼容模块、基础设施composition的精确例外，但须逐条解释，不能用宽泛allowlist掩盖耦合。附加计数器验收：10,000次append每次仅编码一条Event；无变化status/tick的业务apply和归档读取数均为0；1000 renew只增加一个连续omitted区间；ack后journal变小，非心跳旧eid原seq。

- [ ] **I3：同步规格最小句和用户日志。** FR-87/16.3写入冻结的heartbeats边界一句；README说明旧游标需重载快照、日志自动迁移与不确定结果须重开。双语Unreleased增加“任务领取/释放与租约事件一致”“hub追加日志与快照压缩减少历史开销”条目，不声称业务归档/eid索引总大小固定。tasks/todo记录各阶段证据、测试命令、旧测试改义说明和本批提交号。
- [ ] **I4：核对可独立提交的闭环范围。**

```bash
git status --short
git diff --stat
git diff -- crates/towermd-core/src crates/towermd-core/tests crates/towermd/src/daemon.rs crates/towermd/tests/daemon_cli.rs
```

只能显式暂存经过验证的本批路径；不包含未跟踪AGENTS.md、CLAUDE.md、S5计划/隔离工作树，或批1尚未验证的修改。若批1与本批仍共享未提交文件，先由Root分离其已验证提交，不能混入一个“hub整改”提交。联合实现可一提交或按最终可独立绿测边界拆分，但每个提交必须可编译并记录SHA。
- [ ] **I5：提交并记录真实证据。** 使用显式路径`git add`后检查`git diff --cached --stat`与`git diff --cached --check`，提交标题建议`fix: align hub protocol and incremental durable storage`。记录`git rev-parse HEAD`，回看该提交实际source与测试；未提交就明确写“未提交”，不能勾选为已交付。此批不推送、不发版。

## 自审对照

| 要求 | 验证阶段 |
| --- | --- |
| 七操作、contiguous_seq、trait调用方 | A、G、H |
| 不解析业务data/commit、调用方derived eid | A、D、I |
| 全部guards、结构化占用、独立L5 lease事件 | B、F |
| L5 task.claimed/released与失败补偿 | F、H |
| 单条追加fsync、inode不变、断尾/poison | C |
| v1迁移与每个安装失败窗口 | C |
| ack压缩、非heartbeat旧eid原seq、旧订阅 | D、H |
| opaque业务checkpoint与system leases分开 | D、G |
| state无receipts、归档eid推导、区间有界 | E |
| 业务cache可重建、普通tick/命令只apply新事件 | F、G、H |
| 重建跳号、先Git后安装、私有人工确认 | G |
| 基线安全回归与精确提交 | H、I |

计划自审时尤其检查：所有例子使用guards而非旧单fence；snapshot_ack的业务值是参数而非hub自行计算；非heartbeat索引不依赖state.json；恢复seed的receipts来自归档；no-change tick不调用read_snapshot；提交Git后ack失败不丢事实；重新打开文件前不在poison状态重发。实际实施验证与两轮评审的结果由Root填入tasks/todo，本文不提前声称通过。
