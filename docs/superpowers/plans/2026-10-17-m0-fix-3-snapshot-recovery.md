---
type: Plan
title: M0 整改批次 3：本地快照、Git 重做与 App 恢复
description: 联合实施 R3、R7、R4、R14，建立私有快照引用、三次有界 Git 重做、App 确认恢复与增量事件驱动的后台快照闭环。
tags: [towermd, plan, M0, remediation, snapshot, recovery, TDD]
generated: { by: codex/gpt-6, at: 2026-10-17T00:00:00Z }
sources:
  - id: remediation
    resource: docs/superpowers/plans/2026-10-17-m0-remediation.md
    title: 用户已批准 D0–D6 推荐方案；本批 R3、R7、R4、R14
  - id: spec
    resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: FR-12、FR-83、FR-87 至 FR-94、FR-107、FR-110、16.2、M0 验收
  - id: previous
    resource: docs/superpowers/plans/2026-10-17-m0-fix-2-hub-storage.md
    title: 必须先完成的协议、opaque 检查点、归档 eid 与 ProjectSession 接口
  - id: principles
    resource: docs/product-principles.md
    title: 信念二的一处定序、仓库来源与可重建性
  - resource: docs/superpowers/plans/2026-10-17-shared-recovery-proof-contract.md
status: draft
---

# M0 整改批次 3：本地快照、Git 重做与 App 恢复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 无远端且 `main` 已检出的仓库也能安全持久化快照；用户通过 App 客户端确认后能从快照、Git 事实与持久发件箱恢复 hub，后台快照有时限、退避和可观察状态。

**Architecture:** 所有快照读取统一比较权威 `main` 与私有引用；写入器使用临时 index、租约回查和 CAS/普通 push，将允许的状态文件叠加到最新主干树。恢复在核心内验证人、请求和来源，先 staging、再持久化新 Git 边界、最后安装目标 hub；守护进程只承担 App 入口与可重建调度状态。

**Tech Stack:** 现有 Rust、Git、serde/serde_json、sha2、uuid、tempfile、tokio、axum、reqwest；不新增依赖、云客户端、绕过开关或 CLI 批准命令。

**共享证明前置契约：** [2026-10-17-shared-recovery-proof-contract.md](2026-10-17-shared-recovery-proof-contract.md) 是 E/P2/P3/P4 的唯一恢复证明输入、confirmed 持久顺序、历史 merge 与 publication schema 来源。以下接口均待实现；开工按实际前置 SHA 对齐，缺 producer/schema 只诊断，不用夹具或字段自报代替真实产品路径。

---

## 范围、前置条件与执行纪律

本文件只规划 M0 的 R3/R7/R4/R14。用户已批准 D0–D6 全部推荐方案，并授权继续开发至首个未签名可用 App；后者由总计划继续推进，本批不实现 M1 画布按钮、完整 finish/提案审批、远端 hub 或发布。CLI 只能请求恢复，M0 用真实 HTTP App 客户端身份验收。

**接口基线不能假定已经存在。** 写计划时主目录仍是旧 `Mutex<Hub>`、`LeaseChange`、`Snapshot.receipts`、`journal.json`；批 1 的 `cc0ef23` 在整改工作树待双审，批 2 尚未实施。以下使用批 2 定义的未来契约：`ProjectSession`、`HubProtocol`、`guards: Vec<Fence>`、`events::derived_eid`、`snapshot_ack(ack, opaque)`、`RebuildSeed`。执行本批前必须等批 2 验证并提交，在整改工作树读取其实际源码，把本计划签名映射到已落地接口并记录提交号。编译失败只算接口准备，红测必须是可运行的行为失败；严禁以旧 Hub API 写“可编译”示例绕过批 2。

2026-10-17 实施前更新：批 2 已经双审通过并合入 `3c937fd`、`363cfee`，完整默认并发 372 项通过，源码等同冻结工作树 `5fb2207d`。`ProjectSession`、typed `TaskFailure`、增量 active 索引、claim 完整模块凭据校验与逐 fence 补偿均已落地，不能被本批覆盖。质量审查确认一个须在 H 阶段收口的调度限制：当前仅心跳/主干维护时不再触发快照，tail 会增长。以下区分“心跳不触发 25 秒快速快照”与“达到默认周期后压缩有变化的 tail”，按 FR-90 定期快照完成该闭环；不是声称批 2 已有长期心跳空间界限。

实施接口复核后，原联合任务超过单个实现者可可靠验证的上下文，改为下表五个顺序执行、各自可编译和验证的闭环；A–H 保留为详细 TDD 步骤。每个闭环独立规格审查、质量审查、精确提交，全部完成后才勾选批次 3 总体完成。不得以拆分为由留下旧恢复器看不到私有引用的盲区、空 stub、虚构批准或未验证兼容桥。

| 执行任务 | 详细阶段 | 独立完成条件 |
| --- | --- | --- |
| 3.1 私有快照基础 | A、B | 私有 CAS、双来源与双历史 fencing、缓存/ack 一致，现存恢复 prepare/rebuild 读路径同步支持私有来源，CLI 与现有回归可工作 |
| 3.2 搬送与竞争重做 | C、D | 同界精确搬送、新 holder Advance、三次有界重做与上下文/租约检查；真实 CLI 竞争成功 |
| 3.3 恢复事实与持久输入 | E | 唯一 Git/Evidence 证明器与 outbox 真持久输入接到核心恢复，业务投影/依赖/旧 guards 行为可验证，不新增 CLI 批准能力 |
| 3.4 App 恢复闭环 | F、G | 请求零续租、只读需恢复、保存请求/身份/时效验证、真实 HTTP 确认、Git 先于安装与失败后更高边界 |
| 3.5 快照调度与整批收口 | H、V | 实际事件触发、周期压缩、退避/提醒/状态、全批联合回归和文档证据 |

阶段内每步保持 2–5 分钟的可检查动作；表中每个测试各走一次“写测试→看行为失败→最小实现→同命令通过”。后续任务开工重核上一任务实际接口，不并行两个源码实现者；无法形成独立可工作的边界时先向 Root 报告并重新规划。本计划文件本身不声称任何实现已完成。

固定锁顺序是 writer 锁→短暂 session 锁。Git fetch/push/归档读取不持有 session 锁；后台 tick 不持有 session 锁等待 writer。沿用批 1 的读取缓存与写前 fresh-fetch；保留批 1 修复的远端 raw-URL push 成功后、ack 之前 cached `refs/remotes/origin/main` 的直接引用检查和 no-deref/CAS 协调，不能覆盖并发 fetch 的更高结果。不能把 Catalog 恢复成每次读取同步 fetch，也不能让快照退避阻碍任务租约维护。

3.1 接口复核补充：基线 `validate_authority(&mut ProjectSession, &Catalog)` 在 cache miss 时同步读取归档，writer 和 daemon 的 `validated_catalog` 调用仍持 session 锁。3.1 保留该既有接口，仅完成双来源与缓存一致性，不声称全部归档 I/O 已移出锁。3.2 的 C3a 必须收口生产调用；H 再以慢 Git/归档并发续租测试联合验收，不能把限制遗留到发布。

## 文件地图

| 文件 | 本批责任 |
| --- | --- |
| `crates/towermd-core/src/snapshot_git.rs` | 双历史 fencing、私有直接引用 CAS、状态树重建、结构化 push 分类与三次尝试 |
| `crates/towermd-core/src/snapshot_service.rs` | 双来源选择、state/归档等价证明、合法搬送补 ack、真实连续前缀与公开恢复编排 |
| `crates/towermd-core/src/recovery.rs` | 来源与请求绑定、staging、顺序恢复事实、新边界持久后安装；AppConfirmation 保持私有 |
| `crates/towermd-core/src/recovery_git.rs`（新建、内部模块） | 正序扫描 main 提交/trailer/基线标签、验证 Evidence 和 tree、产生确定性恢复事实 |
| `crates/towermd-core/src/recovery_outbox.rs`（新建） | 最小本机持久恢复输入：JSONL、fsync、完整行读取、稳定摘要；不实现远端传输 |
| `crates/towermd-core/src/project_session.rs`、`snapshot.rs`、`tasks.rs` | 恢复事实真实进入业务投影、后续依赖检查、增量快照触发观测 |
| `crates/towermd-core/src/lib.rs` | 导出最小恢复输入/编排 API；Git 解析和确认类型不导出 |
| `crates/towermd/src/daemon.rs` | 持久请求、App gate、可打开待恢复项目、只读 status、真实后台工作接线 |
| `crates/towermd/src/snapshot_schedule.rs`（新建） | 可删除的快照调度状态、去抖、退避、单次超时提醒 |
| `crates/towermd/src/main.rs` | 注册调度模块；确认 rebuild 不隐式续租，保持 CLI 无批准入口 |
| `crates/towermd-core/tests/snapshot_git.rs`、`snapshot_service.rs`、`snapshot.rs`、`tasks.rs` | Git 安全、双来源、搬送、连续前缀、业务恢复回归 |
| `crates/towermd-core/src/recovery.rs` 的测试模块 | 已有 staging 故障窗口与授权单元回归 |
| `crates/towermd-core/tests/recovery_inputs.rs`（新建） | 持久 outbox 输入和 Git 恢复事实的联合验证 |
| `crates/towermd/tests/daemon_cli.rs` 与 `daemon.rs` 的测试模块 | 真 daemon 的 App 恢复、请求零续租、后台触发；测试内可信时钟与计数器 |
| `docs/product-principles.md`、`README.md`、`CHANGELOG.md`、`CHANGELOG.zh-CN.md`、`tasks/todo.md` | 私有引用例外、可见变化、红绿及提交证据 |

新增模块按职责拆分，不搬动无关代码。测试故障注入仅在 `#[cfg(test)]` 的私有组合点，不能加入环境变量或公开 HTTP “跳过校验”参数。

## 统一接口与判定

### 快照身份与写入用途

保留 `WriteRequest` 的现有字段，`base` 始终指本次主干基底，`archives` 表示要覆盖的完整月份内容。上层必须把来源快照所有归档月份与新事件合并后交下层；只传“本次变化月份”会在从私有引用搬到 main 时漏掉旧月份。

在 `snapshot_git.rs` 定义以下类型，`SnapshotSource` 放在 `snapshot_service.rs`：

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotSource { Main, Private }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteIntent {
    Advance,
    Promote { source_commit: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushFailure {
    NonFastForward,
    Other(String),
}
```

`write_snapshot(root, request, verify_lease)` 保留作为 Advance 包装，新增 `write_snapshot_with_intent(root, request, intent, verify_lease)`；二者都返回原有 `WriteOutcome`。实现内部 `WriteTarget` 区分 `RemoteMain { push_url }`、`LocalMain`、`Private`，不暴露让外部任意指定 ref 的参数。`SavedSnapshot` 增加 `source: SnapshotSource` 与 `authority_revision: String`；`commit` 是真正写入 state 的提交 SHA，不能拿 main 最新代码提交冒充。

`read_snapshot(catalog)` 先各自读取 `catalog.revision`（批 1 的权威 main）和安全直接的 `refs/towermd/snapshot`，先读取私有提交自身的 `.towermd/config.yaml` 并与当前 main 核对 project_id/docs_dir/hub 上下文，再按各自已核验的 docs_dir 读取并验证每份 state、全部归档及提交 trailer，然后比较 `(writer_epoch, through_seq)`。不存在私有引用是正常；符号引用、悬空引用、非提交对象是错误。对同边界使用反序列化 Snapshot 的逐值相等和归档按月规范化后的逐字节相等，冲突立即拒绝；等价时优先 Main。不能用只有 seq 的比较，也不能因 main 有一份旧快照就不读私有引用。私有提交的旧配置/docs_dir 与当前 main 不相容时明确报“私有快照与当前权威上下文不相容”，保留两处引用并拒绝写入；不能仅在当前 docs_dir 下找不到 state 就把私有快照当不存在并覆盖。

等价函数保持一处定义，并用于读取冲突、搬送和 `validate_saved`：

```rust
fn equivalent(a: &SavedSnapshot, b: &SavedSnapshot) -> bool {
    a.snapshot == b.snapshot && a.archives == b.archives
}
```

常规 Advance 遇任一历史中更大或相等边界都 Superseded。Promote 的唯一相等例外是：当前私有来源 SHA 与已验证的搬送来源相同、state/全部归档等价、当前 main 没有更大边界，而且来源 snapshot.writer_epoch 与当前有效主干 lease 的 epoch 一致、现有 lease 摘要/新 tail 不要求改变检查点。这里无需额外人工批准。当前 main 已有同边界等价内容时只补确认并返回 unchanged，不另造 commit；同边界不等价拒绝。历史中曾有更高边界即使 state 后来被回退也拒绝。重启或新 holder 接管导致当前 fence epoch/lease 事件或其他 tail 使 checkpoint 不同于私有来源时，不能强行沿用旧 writer_epoch/opaque 做 Promote；必须以来源完整归档加新 tail 构建当前有效 lease 的新 Snapshot，走正常 Advance，以更高 epoch/seq 写回 main。私有来源待搬送本身就是调度原因，即使没有新增真实业务事件、lease grant 被业务 due 排除，也不能跳过此 Advance。

### 恢复编排与跨 crate 边界

批 2 已保留私有确认构造。公开编排由 `SnapshotWriter` 提供下面的方法，内部取得同一个 writer 锁、构造并校验私有 `AppConfirmation`，调用私有 execute。existing 只可取 daemon 原已注册 Session 的 Arc；没有已注册 Session 时传 None，不先构造空业务 Session。core 内部 execute 采用同一 existing 参数与返回类型，目标打开/真空校验/私有退役沿共享契约第3.1节；daemon 不能直接构造确认或 opaque target。

```rust
pub fn confirm_rebuild(
    &self,
    existing: Option<std::sync::Arc<std::sync::Mutex<crate::project_session::ProjectSession>>>,
    catalog: &crate::catalog::Catalog,
    request: &crate::recovery::RecoveryRequest,
    actor: &str,
    expires_at: u64,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    host: &str,
    now: impl FnMut() -> Result<u64, String>,
) -> Result<(SnapshotReport,
    std::sync::Arc<std::sync::Mutex<crate::project_session::ProjectSession>>), String>;
```

成功返回 report 与有效 Session Arc：existing=Some 时为已核安装后原地切换的同一 Arc，None 时才由 core 构造新有效 Session。失败不发布业务 view/不开放写入；prepare 仍独立读取实际 Git/inputs，不创建 hub 或假 Session。本签名与 core 私有 empty-target 生命周期由 E1 交付，F 只接 App 入口。

核心 prepare 也采用固定接口，不能从 daemon 注入未经核心核验的 source：

```rust
pub fn prepare(
    catalog: &crate::catalog::Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    requested_at: u64,
    expires_at: u64,
    nonce: &str,
) -> Result<RecoveryRequest, String>;
```

prepare 校验 nonce 是 UUIDv7、expiry 严格大于 requested_at、寿命不超过 5 分钟；核心重算 inputs.digest，不信任外部传入的摘要字符串。确认时 `expires_at` 参数必须等于 request.expires_at，不能靠直接调用公开编排延长期限。以上是签名契约；执行者在阶段 A 对接真实批 2 组合接口。

这是公开核心编排方法的签名契约，不是另一个批准 API。它再次校验 `human:<id>`、request_id、expiry、项目/source/current authority/input digest；daemon 从已保存请求拿 expiry，HTTP body 只有 `actor`，客户端不能延长。`AppConfirmation` 本身及构造函数均不出 core 的私有恢复模块。

`RecoveryRequest` 保留现有来源与跳号字段，增加 `source_epoch_high_water`、`source_reference`、`inputs_digest`、`requested_at`、`expires_at`、`nonce`；`main_revision` 是待确认时的权威 main。核心提供的 source fingerprint 为全部这些来源字段（不含 request_id）的 SHA256；daemon 登记时再绑定 `requested_at`、`expires_at` 和 UUIDv7 nonce 计算 request_id，并把这些值纳入保存的请求与核心校验，避免过期后显式重请求复用旧批准 URL。重复未过期请求可返回原登记，不能延长其 expiry；过期后由新的 CLI 请求产生新 ID。确认前 fresh-fetch，再核对来源字段而不是重新调用带当前时间的 prepare 后做整对象相等比较；写前回调也校验 expiry、来源仍相同及 lease。新源出现、main 推进、outbox 新输入均要求重新请求，不能悄悄扩大人确认范围。

### 最小持久恢复输入

现仓库没有 outbox。不能在测试里仅向 execute 传一个 Vec 就宣称接入。新增 `RecoveryOutbox` 存于 `runtime/recovery-outbox/<project_id>/<host>.jsonl`，脱离可被删除的 Git common-dir/towermd hub 目录；daemon 明确传入目录和已核 Catalog 上下文。API 为 `open(directory, context)`（context 由 core 从已核 Catalog 构造）、`append_pending(&NewEvent)`、**core 内部** `persist_confirmed(original:&NewEvent,actual:&Event)`、`read_inputs()`，格式与校验完整沿 [2026-10-17-shared-recovery-proof-contract.md](2026-10-17-shared-recovery-proof-contract.md) 第 2 节，不在本计划另维护一份 JSON schema。

`RecoveryInputs` 改为私有字段，区分 pending 与已校验 confirmed、由 core 重算全内容 digest。原公开 `events: Vec<NewEvent>` 不能承载已接受证明。统一 `VerifiedRecoveryHistory` 只由双源 snapshot/archive reader、已核连续 tail 与 confirmed 构造；不接受 raw StateProjection/verified JSON。稀疏 confirmed 的 seq 与Event.epoch/全部guards epochs最大值参与prepare的2^32跳号，但不伪装连续前缀。

统一顺序：pending JSONL/fsync → hub.append → 完整原 Event 与 original 全字段/全 guards 比对 → confirmed JSONL/fsync → 本次mutation/幂等成功回包（Hub已接受照常sync/广播/快照）。hub 可能对重复 eid 返回不同本次 body 的旧 Event，所以 eid 相同不足以确认。confirmed fsync 失败保留原 eid/guards 并返回 uncertain；仅 Duplicate{seq} 无已有已核完整 Event 时不能捏造 confirmed。HTTP/CLI 不接收 confirmed JSON。Hub已耐久接受可正常sync/广播/快照；confirmed只gate本次mutation/幂等成功回包和JobCompleted，不隔离或停顿连续投影。

raw pending 只作原请求 uncertain 协调，不授信完成或批准；原已核 confirmed 是历史接受证明，恢复重发仍保原 guards、在新 hub stale，不复活旧 Run。新入队拒绝续租，legacy renew 读取丢弃；不增加全部 renew/TTL 日志。重建前不删输入，成功后靠原 eid 幂等。M0 不实现远端输送、自动跨主机收集或通用重试器。

## R3/R7/R4/R14 详细步骤（按上表五个执行任务分批冻结）

### 阶段 A：冻结真实批 2 接口和失败基线

**Files:** Read 本计划文件地图中的生产与测试文件；Modify `tasks/todo.md` 的本批计划/证据节。

- [ ] **A1：在整改工作树确认已有批 1/2 提交和脏文件。** 运行 `git status --short`、`git log -8 --oneline`、`git show --stat HEAD`，并从提交历史定位批 2 实际提交。把真实提交号写入 todo，不替其他任务暂存文件。尚未完成批 2 时停止本批实施并继续总计划的批 2，不在旧接口上抢跑。
- [ ] **A2：核对实际接口。** 运行下列命令，逐一记录 ProjectSession 的同步/只读状态/投影/快照取点、staging 工厂、`SnapshotWriter` 的签名映射；如果批 2 命名不同，先只调整本计划对应签名，再运行基线测试。

```bash
rg -n 'pub (struct|enum|trait)|pub fn|fn snapshot_ack|fn rebuild|contiguous_seq|guards|checkpoint' crates/towermd-core/src/hub_protocol.rs crates/towermd-core/src/project_session.rs crates/towermd-core/src/snapshot_service.rs crates/towermd-core/src/recovery.rs
cargo test -p towermd-core --test snapshot --test snapshot_service --test hub_contract --locked
cargo test -p towermd --test daemon_cli --locked
```

Expected：批 2 基线通过；普通生产调用没有 `LeaseChange`、`hub.events(0)` 全量 replay、daemon 读取 journal 格式或 state receipts。批 2 的连续前缀生产测试仍在，后续只扩展而不删除。
- [ ] **A3：冻结现有真实失败用例。** 现有 `snapshot_git.rs` 的 `competing_remote_push_is_rejected_without_retry`、服务层 `checked_main_defers_without_acknowledging_unwritten_sequence`、daemon 的 `snapshot_checked_main_reports_deferred_without_worktree_writes`/`hub_rebuild_implicitly_extends_bound_run_expiry_including_after_restart` 正是在本批要改变的行为；逐条改为下面新断言后执行得到行为失败。保留旧安全断言，不用删除测试来“通过”。

### 阶段 B：私有引用 CAS 与统一双来源

**Files:** Modify `crates/towermd-core/src/snapshot_git.rs`、`snapshot_service.rs`；Test `crates/towermd-core/tests/snapshot_git.rs`、`snapshot_service.rs`。

- [ ] **B1：先写最小 Git 红测。** 在 `tests/snapshot_git.rs` 复用已有 `Fixture`、`request`、`git`、`written`，加入下列测试。它直接走生产写入器，旧实现会返回 Deferred。

```rust
#[test]
fn checked_main_snapshots_to_private_ref_without_touching_user_state() {
    let f = Fixture::new();
    let request = f.request();
    fs::write(f.root.join("human.txt"), "staged\n").unwrap();
    git(&f.root, &["add", "human.txt"]);
    fs::write(f.root.join("human.txt"), "unstaged\n").unwrap();
    fs::write(f.root.join("untracked.txt"), "keep\n").unwrap();
    let flags = f.root.join("flags.txt");
    fs::write(&flags, "flags\n").unwrap();
    git(&f.root, &["add", "flags.txt"]);
    git(&f.root, &["update-index", "--assume-unchanged", "flags.txt"]);
    let index = fs::read(f.root.join(".git/index")).unwrap();
    let heads = git(&f.root, &["for-each-ref", "--format=%(refname) %(objectname)", "refs/heads"]);
    let status = git(&f.root, &["-c", "core.fsmonitor=false", "status", "--porcelain=v2", "-z"]);
    let bytes = fs::read(f.root.join("human.txt")).unwrap();
    let commit = written(write_snapshot(&f.root, &request, || Ok(())).unwrap());
    assert_eq!(git(&f.root, &["rev-parse", "refs/towermd/snapshot"]), commit);
    assert_eq!(git(&f.root, &["for-each-ref", "--format=%(refname) %(objectname)", "refs/heads"]), heads);
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.root.join("human.txt")).unwrap(), bytes);
    assert_eq!(fs::read_to_string(f.root.join("untracked.txt")).unwrap(), "keep\n");
    assert_eq!(git(&f.root, &["-c", "core.fsmonitor=false", "status", "--porcelain=v2", "-z"]), status);
    assert!(!f.root.join("docs/tower/state/state.json").exists());
}
```

Run：`cargo test -p towermd-core --test snapshot_git checked_main_snapshots_to_private_ref_without_touching_user_state --locked`。Expected 红：`expected Written, got Deferred`，不是编译失败。测试调用 status 前也要设置 `GIT_OPTIONAL_LOCKS=0`；本文件既有 helper 已隔离 Git 环境，给它补此变量，以免测量自身刷新用户 index。
- [ ] **B2：实现私有目标。** 复用 Session 临时 index；无 origin 且 `main_worktree` 找到任一工作树时选 Private。写入前用 `for-each-ref --format=%(refname)%00%(symref)` 确认私有 ref 是直接引用，记录旧 SHA（不存在时用与仓库对象格式一致的全零 SHA）。从本次 main 基底生成只含允许文件变化的 commit，以 `update-ref --no-deref refs/towermd/snapshot <new> <old>` CAS；末次租约回查必须紧邻 CAS。绝不 checkout/reset/stash/commit --only，CAS 不写 `refs/heads/*`。
- [ ] **B3：逐条加安全红测并实现。** 每一行以表中测试名运行红/绿；绿测再跑完整 `--test snapshot_git`。

| 测试名 | 固定输入与断言 |
| --- | --- |
| `linked_main_private_snapshot_preserves_all_worktrees` | main 在 linked worktree 检出，调用工作树为任务分支；两边 dirty/staged/untracked；保存每个 `git rev-parse --git-path index` 的原字节、HEAD 内容、文件字节、`ls-files -v` 和全部 heads，成功后全部相同；同时包含 skip-worktree 与 assume-unchanged 文件 |
| `private_symref_is_rejected_without_moving_target` | `git symbolic-ref refs/towermd/snapshot refs/heads/victim`；调用失败、victim/main/私有 symref/index 原字节不变；还覆盖悬空 symref |
| `private_cas_conflict_never_clobbers_the_winner` | 在写入器私有测试组合点 CAS 前由真实第二 Git 命令推进私有 ref；CAS 失败不覆盖赢家，不 ack；不得自动强推 |
| `main_checkout_race_falls_back_to_private_ref` | 首次检测 main 未检出，写提交后另一工作树检出 main；最终重新检查目标，改私有 CAS，原 main 与该工作树/index 不动 |
| `private_history_higher_fence_blocks_old_writer` | 私有历史已有更高 epoch 或同 epoch 更高 seq，头上再追加普通提交；旧写入 Superseded，完整历史 fencing 不只读头 trailer |

Run（每条替换测试名）：`cargo test -p towermd-core --test snapshot_git private_symref_is_rejected_without_moving_target --locked`。Expected 红是原代码未检查私有 symref/错误更新目标；绿为通过且无用户状态差异。
- [ ] **B4：先写服务双来源红测。** 在 `tests/snapshot_service.rs` 用批 2 已迁移的真实 fixture/TaskService 操作产生合法 Snapshot；不用 `{"through_seq":7}` 的底层 Git 夹具冒充可恢复快照。分别把两份已验证快照写到 main/private，运行以下组：

```bash
cargo test -p towermd-core --test snapshot_service read_snapshot_selects_max_epoch_then_seq --locked
cargo test -p towermd-core --test snapshot_service equal_boundary_equivalent_snapshots_prefer_main --locked
cargo test -p towermd-core --test snapshot_service equal_boundary_different_state_or_archive_is_rejected --locked
```

第一条覆盖 main/private 各获胜、epoch 大但 seq 小仍获胜；第二条 state 与所有归档等价但 SHA 不同；第三条单独改变 state、归档各一次（均保持各自内部有效），同边界拒绝。红测在当前 main-only reader 返回较旧状态或误接受冲突。
- [ ] **B5：实现统一 reader 与双历史 fence。** 把现 `read_snapshot` 的单 revision 读取提成内部 `read_snapshot_at(catalog, revision, source)`，保持普通文件模式、月份、项目、archive replay、trailer 验证。统一入口读取两处，执行上述最大值/等价判定；`highest_fence` 扫主干与私有可达完整历史，保留浅克隆拒绝。恢复、上任补 ack、authority 验证全部调用统一入口。缓存键增加 main revision 与 private ref SHA，私有 ref 改变必须失效；无变化读取不能每次重新回放归档。现有 `checked_main_defers_user_files_and_intermediate_states` 等夹具仍保留，但“用户文件存在就 Deferred”的预期改为私有 Written，断言用户 state/symlink、merge/rebase/cherry-pick 标记和 index 字节均不变；这些工作树输入不能被用作快照来源。
- [ ] **B6：运行绿测。** `cargo test -p towermd-core --test snapshot_git --test snapshot_service --locked`。Expected：私有源可读/可确认，现有租约失效拒绝、普通文件范围、hook/filter 禁用与连续前缀回归仍通过。

### 阶段 C：合法搬送与同水位 ack

**Files:** Modify `crates/towermd-core/src/snapshot_service.rs`、`snapshot_git.rs`；Test `crates/towermd-core/tests/snapshot_service.rs`、`snapshot_git.rs`、`hub_contract.rs`。

- [ ] **C1：写真实无业务新增的搬送红测。** 测试名 `private_snapshot_promotes_without_new_business_and_keeps_new_main_code`：main 检出时写合法私有快照并 ack；停止后台/保持有效主干 lease，在 main 提交 `src/new.rs` 与慢层原有正常内容后切离 main；不追加新业务事件，调用非 force writer。断言 written、父提交为刚才新 main、`src/new.rs` blob 与 main 新值相同，所有非 state 文件与刚才 main 的 tree 相同，state 与全部归档和私有源等价，ack SHA 变为新 main SHA、seq/epoch/opaque 不变。配置 origin 的同一搬送也加一例，检查本地 heads 不动。再加 `new_holder_advances_private_snapshot_to_main_without_business_changes`：真实旧 holder 在 main 检出时写私有快照，drop/reopen 会话、可信时间推进使旧 lease 到期，新 holder 取得更高 epoch 后切离 main；不追加真实业务事件，非 force writer 仍写回 main。断言新 snapshot.writer_epoch 等于当前有效 lease epoch、seq 严格增加、所有旧月份及新 lease/recovery tail 归档完整且全量重放等于 checkpoint，父树保留 main 新代码，旧私有引用没有被覆盖为较小边界。此例必须走 Advance，不能用同 opaque 换 commit 伪造新 holder 的有效快照。

Run：`cargo test -p towermd-core --test snapshot_service private_snapshot_promotes_without_new_business_and_keeps_new_main_code --locked`。Expected 红：旧 due 直接 unchanged、相等 fence Superseded 或同 seq 不同 commit 被 `validate_saved` 拒绝。新 holder 例另跑 `cargo test -p towermd-core --test snapshot_service new_holder_advances_private_snapshot_to_main_without_business_changes --locked`；红为非业务 grant 被 due 排除而未写回或旧 epoch 校验失败，绿为当前 epoch 的新边界真实写入。
- [ ] **C2：实现先搬送判定再业务 due。** writer 先验证双来源；来源为 Private 且当前主干可写时，即便 `due=false` 也安排写回：当前有效 fence epoch 与来源一致且检查点/全部归档不变时才 Promote；新 holder/重启后新 grant、其他 tail 或 lease 摘要要求改变 checkpoint 时走 Advance，按当前连续前缀合并来源全部归档与新 tail，使用当前有效 writer_epoch 与更高 seq，不能把非业务 lease 变化被 due 忽略当作不搬送的理由。临时 index 必须 `read-tree <fresh-main>`，只复制来源的 `state.json` 和所有合法 `events/YYYY-MM.jsonl`；绝不 `read-tree <private>` 后整体覆盖 main。源 SHA/内容在搬送前再核对。若主干 tree 的 state 目录有额外月份，合并前比较集合并验证归档；未知/不等价文件拒绝，不静默删除。
- [ ] **C3：精准调整 authority/ack。** `validate_saved` 遇同 seq 不同 commit 时，仅在 core 已验证原 ack 提交与新提交的 `(epoch,seq)`、state、全部归档等价且新提交是当前权威 reader 的主干来源时允许搬送；不同 epoch、不同内容、回退都拒绝。不能把现有 `ack.commit != saved.commit` 简单删除。补发新 commit 的确定性 `snapshot.written` 后调用批 2 的 `snapshot_ack(ack, serde_json::to_value(&snapshot)?)`；hub 不解析 Git，协议仅允许同 seq、同 epoch、同 opaque 换 commit。

新增等价证明携带两个已验证 `SavedSnapshot`，不以裸 bool 作为公共绕过参数。成功后保留私有 ref；reader 同边界优先 main，避免删除私有 ref 与崩溃之间产生额外窗口。
- [ ] **C3a：将 authority cache miss 的归档读取移出 session 锁。** 拆分轻量来源探测、锁外准备已核 `SavedSnapshot`、短锁验证/安装三个阶段；准备对象字段私有，由统一 reader 构造，不允许 daemon 提交未核快照。缓存命中仍不重读/重放归档。writer 和 daemon `validated_catalog` 的生产调用都迁移；锁内只读取/比较项目绑定、ack/floor/来源版本及安装投影，所有 fetch、Catalog 全量加载、月归档解码在锁外。首次项目注册也不能持全局 projects 注册表锁进行这些 I/O：已注册项目短锁取 Arc 后立即释放，冷注册另行序列化并短锁重查/插入；未注册的临时只读打开沿同一注册门闩覆盖完整 Hub 生命周期，保持同项目单会话、clone 绑定与失败释放。真实双项目归档屏障下，另一个已运行项目的 status/绑定 Run 续租必须能在屏障释放前完成，同项目并发首次访问不得重复打开 Hub。准备期间 ack/floor/来源改变时丢弃过期准备并有界重新读取，不弱化批 1 的旧 Catalog 竞争处理和批 2 的连续前缀验证。先写真实慢归档准备与并发 renew/status 可完成、来源在准备期间改变不会安装旧快照、静态缓存不反复解码的行为红测，再实现；执行时按 3.1 实际接口冻结最小签名，不新增公开绕过参数。
- [ ] **C4：逐条红绿。** `private_snapshot_incompatible_config_is_not_treated_as_absent`、`promote_rejects_equal_boundary_different_archive`、`promote_rechecks_live_main_lease`、`promote_stops_at_higher_main_fence`、`promote_ack_failure_reconciles_after_restart`、`same_watermark_same_checkpoint_can_change_ack_commit`、`same_watermark_different_checkpoint_cannot_change_ack_commit`。逐条执行：

```bash
cargo test -p towermd-core --test snapshot_service private_snapshot_incompatible_config_is_not_treated_as_absent --locked
cargo test -p towermd-core --test snapshot_service promote_rejects_equal_boundary_different_archive --locked
cargo test -p towermd-core --test snapshot_service promote_rechecks_live_main_lease --locked
cargo test -p towermd-core --test snapshot_service promote_stops_at_higher_main_fence --locked
cargo test -p towermd-core --test snapshot_service promote_ack_failure_reconciles_after_restart --locked
cargo test -p towermd-core --test hub_contract same_watermark_same_checkpoint_can_change_ack_commit --locked
cargo test -p towermd-core --test hub_contract same_watermark_different_checkpoint_cannot_change_ack_commit --locked
```

配置不相容例分别改当前 main 的 docs_dir/project_id/hub，上述明确诊断成立且 main/private ref、ack 原值不变；不能视为缺失快照。批 2 已有后两条时直接回归，不能复制一套不同规则。故障断言 Git 已成功而 ack 失败后，重开从两源最大快照补确认，只补一个确定性事实，不再次搬送。
- [ ] **C5：绿测完整三组。** `cargo test -p towermd-core --test snapshot_git --test snapshot_service --test hub_contract --locked`。Expected：搬送保留新代码，同水位内容冲突仍是硬错误；无业务新增不妨碍搬送。

### 阶段 D：三次有界 fresh-fetch 重做

**Files:** Modify `crates/towermd-core/src/snapshot_git.rs`；Test `crates/towermd-core/tests/snapshot_git.rs`、`crates/towermd/tests/daemon_cli.rs`。

- [ ] **D1：把旧“不重试”测试改为一次竞争后成功。** 测试名 `competing_remote_push_rebuilds_on_fresh_main_once`；复用真实 bare origin，测试组合点在第一次 push 前将普通代码提交推进 origin/main，第一次非快进失败、第二次成功。记录每次 fetch、build parent、push 和 lease 回查次数；最终提交父 SHA 是赢家提交，赢家新代码保留，本地 heads/index/worktree 不变。

Run：`cargo test -p towermd-core --test snapshot_git competing_remote_push_rebuilds_on_fresh_main_once --locked`。Expected 红：原实现直接返回“push 未确认成功”。
- [ ] **D2：加入 push 结构化分类。** 使用现 `Session` 隔离的原始 Git Output 路径（批 1 `repo.rs` 已增加），固定 `LC_ALL=C`，执行 `git push --porcelain --no-verify --no-follow-tags --no-signed --recurse-submodules=no -- <url> <sha>:refs/heads/main`。只有目标 ref 的 porcelain 状态 `!` 且摘要为 `[rejected] (non-fast-forward)` 或 `[rejected] (fetch first)` 分类 NonFastForward；保护分支 `[remote rejected]`、认证/网络/超时、hook 拒绝和未知输出都是 Other。不靠“包含 rejected”或错误中文字符串泛重试；失败也保留完整诊断。

实现内部分类测试的固定输入：

```rust
#[test]
fn only_non_fast_forward_push_errors_are_retryable() {
    assert_eq!(classify_push("!\t123:refs/heads/main\t[rejected] (fetch first)\n", ""), PushFailure::NonFastForward);
    assert_eq!(classify_push("!\t123:refs/heads/main\t[rejected] (non-fast-forward)\n", ""), PushFailure::NonFastForward);
    for output in [
        "!\t123:refs/heads/main\t[remote rejected] (protected branch)\n",
        "!\t123:refs/heads/main\t[remote rejected] (pre-receive hook declined)\n",
        "fatal: Authentication failed\n",
    ] {
        assert!(matches!(classify_push(output, "transport error"), PushFailure::Other(_)));
    }
}
```

`classify_push(stdout: &str, stderr: &str) -> PushFailure` 为模块内部函数；测试放同模块，不为外部暴露。Run：`cargo test -p towermd-core snapshot_git::tests::only_non_fast_forward_push_errors_are_retryable --locked`，先红后绿。
- [ ] **D3：实现最多三次尝试。** 每次依次：fresh fetch 主干→按该 SHA 重新加载权威 Catalog/配置→重新解析当前 private ref→双历史 fencing/搬送等价检查→`read-tree fresh-main` 重新生成允许状态变更→回查 lease→新 `commit-tree`→回查 lease→push→批 1 的 tracking-ref CAS 协调→core ack。只有 NonFastForward 且 attempt 小于 3 时进入下一次；总 push 最多 3（不是初次加 3 次）。第一次 request.base 是调用时的 main，不要求它在重做时仍相等；重做只带同一个已确认 checkpoint 和完整归档，不从失败提交的整树复制。每轮比较 project_id、docs_dir、hub 配置和权限/基线相关的权威上下文：普通代码推进可自动重建；项目/配置/慢层上下文改变时明确返回“权威上下文已变化，须重新规划快照”，不把旧 snapshot_files 写进新路径，不迁移数据，不生成 ack。重新构造请求只可使用经过新 Catalog 核验的上下文；若变更影响 checkpoint 的业务解释，不能在 Git 层自行认定安全。更高快照立即返回 Superseded；相等只按 Promote 的精确例外。非竞争错误立即返回，不 ack。push 已成功但缓存 CAS 协调失败时，记录远端已持久的 commit 与需协调原因，不能重推或把错误当 NonFastForward；下次 fresh-fetch 读回最大快照补确认。
- [ ] **D4：逐条真实 bare 竞争红绿。**

| 测试名 | 竞争与断言 |
| --- | --- |
| `higher_remote_snapshot_stops_retry_before_second_push` | 第一次 push 前对方写合法更高 epoch/seq 的快照；fresh fetch 后 Superseded，push 只有 1 次，自己的 ack 不变 |
| `three_remote_races_stop_after_three_attempts` | 每次 push 前都由真实第二写入者推进 bare main；fetch/build/push 各至多 3 次，最后明确报三次竞争失败，未覆盖赢家 |
| `remote_policy_and_transport_errors_do_not_retry` | bare pre-receive 明确拒绝、不可达 URL 各一例；push 只有 1 次，原错误保留，不记录 snapshot.written |
| `retry_rejects_changed_project_docs_dir_or_authority_context` | 第一轮竞争同时改 project_id/docs_dir/影响业务的慢层配置，各一例；第二轮不生成旧路径提交、不 push、不 ack；报告上下文变化 |
| `successful_retry_coordinates_cached_main_before_ack` | 一次重做成功后立即断网，status/task show/已有 run 续租可用，不误判快照回退；缓存 CAS 竞争时不覆盖更高 tracking SHA |
| `lease_expires_between_attempts_and_never_pushes_again` | 第一次非快进后使可信测试时钟越过 lease 到期；新轮回查失败，push 只有 1 次，未复活旧 epoch |

Run：

```bash
cargo test -p towermd-core --test snapshot_git higher_remote_snapshot_stops_retry_before_second_push --locked
cargo test -p towermd-core --test snapshot_git three_remote_races_stop_after_three_attempts --locked
cargo test -p towermd-core --test snapshot_git remote_policy_and_transport_errors_do_not_retry --locked
cargo test -p towermd-core --test snapshot_git retry_rejects_changed_project_docs_dir_or_authority_context --locked
cargo test -p towermd-core --test snapshot_git successful_retry_coordinates_cached_main_before_ack --locked
cargo test -p towermd-core --test snapshot_git lease_expires_between_attempts_and_never_pushes_again --locked
```

使用私有测试 I/O 组合点控制竞争，不依赖 sleep 争抢时序，不用 mock 输出替代 bare Git 验收。
- [ ] **D5：真实 daemon 回归。** 在 `daemon_cli.rs` 改写同类 bare 夹具，`snapshot_cli_recovers_one_remote_race` 从实际 `towermd snapshot` 发起。竞争屏障放在真实 fetch 已取得旧远端广告/数据、push 尚未开始广告协商之间；不能放在 pre-receive 后再声称客户端 non-fast-forward。后者已经完成 push 广告，服务端引用竞争可返回 `[remote rejected] (failed to update ref)`，仍属 D2 的 Other，必须单轮失败，不扩大重试分类。

优先复用已有 count-upload-pack 测试夹具：仅在可丢弃 bare/clone 的 `remote.origin.uploadpack` 配置私有 wrapper，先运行真实 `git-upload-pack`、保留其退出码，在 wrapper 退出前发 marker 并有界等待 barrier。先预热并结束后台 fetch，再启用该一次性屏障；第二真实普通提交推进 bare 后释放屏障。须先实测第一轮 fetch 确实返回旧 SHA、随后实际 Git 输出是目标 ref `[rejected] (non-fast-forward)` 或 `(fetch first)`，再据此形成 CLI 红→绿；不捏造 porcelain、不依赖 sleep 争抢、不修改用户 hooks/全局配置或新增运行时测试开关。若该组合点在真实 transport 下不可用，先报告并选择等价的服务端协议组合点，不扭曲生产分类器。Run：`cargo test -p towermd --test daemon_cli snapshot_cli_recovers_one_remote_race --locked`。红是 CLI 失败，绿是一次用户命令成功、重做次数真实且最终父提交为赢家 SHA；保留 `blocked_periodic_push_keeps_all_main_leases_alive_and_shutdown_waits` 证明重试不占 session 锁。

### 阶段 E：Git 事实、有效 Evidence 与持久 outbox 恢复

**Files:** Create `crates/towermd-core/src/recovery_contract.rs`、`recovery_git.rs`、`recovery_outbox.rs`、`task_gates.rs`、`tests/recovery_inputs.rs`、`tests/task_gates.rs`；Modify `repo.rs`；Modify `recovery.rs`、`project_session.rs`、`snapshot.rs`、`tasks.rs`、`lib.rs`。E1 另允许 Modify `crates/towermd-core/src/snapshot_service.rs`、`crates/towermd/src/daemon.rs` 的 Session 生命周期/两阶段证明接线，及对应 `tests/project_session.rs`、`tests/snapshot_service.rs`、daemon 单元/CLI 测试；不在 E0 改这些接线，不提前实现 F 的 App 路由或 P2 的 durable claim producer。

本阶段按共享契约第1.1–1.2节先拆两个闭环，防止consumer反向依赖后续producer。本文仅排实施步骤；唯一schema/函数签名以共享契约为准，不能以任务划分改变产品规格。当前E0/E1产品测试均未执行，P2/P3/P4与批4producer均未交付。

- [ ] **E0闭环：共享数据、耐久输入与纯守卫。** 由新鲜实施者在已通过审查的C/D实际SHA上开工。只交recovery_contract DTO（含共享第1.3节四种窄recorded请求/typedreceipt及第1.4节review-open/GateEvidence/ReviewContext）/canonical JSON/commands与report hash/出版10-parts hash、recovery_outbox双态与唯一append_durable、task_gates纯raw-z/G5及repo字节Git适配。沿共享第6节写真实文件fsync/reopen/原回执负例；新增typed report/log绑定、全结构化commands数组顺序、出版固定10-parts字节向量和真实Git G5基底链测试（代码/config推进而最终tree巧合相同仍拒绝）。不提前建finish/verifier执行器、HumanState/AppGate/批准API或P4编辑writer；不导出裸state/history/proof构造器。成功标准：`cargo test -p towermd-core --test recovery_inputs --test task_gates --locked`通过本闭环用例、现有workspace必要回归无退化，producer尚未接线明确登记；独立SPEC→quality均通过并精确提交后才进入下一闭环。
- [ ] **E1闭环：已核历史与canonical业务恢复。** 由另一新鲜实施者以E0实际SHA复核签名，执行下列原E1–E7，交唯一reader/prove_merge/prove_publication/builders、Task/Run/Evidence投影与staging重放。只能从实际reader/session连续tail/已核outbox构造私有history；失败或缺schema一律诊断，不能临时返回Ok或直接apply自报merged。成功标准：原E7和共享第6节consumer矩阵通过，二次无旧outbox完整原review context恢复成立；原opening接受seq/已知laterPlan按共享第1.4节核，不新增seq覆盖或renew存储；真实producer/真人验收仍待P2/P3/P4，独立SPEC→quality→完整必要验证后精确提交。E0没有consumer时不得开放外部恢复旁路，E1不是承诺后续再补安全门禁的半成品。

实施规模复核后，E1按下面两个安全边界顺序冻结，各自独立SPEC→quality并精确提交；只有两者齐全才完成上面的E1。此拆分不改变证明契约或产品范围：

- [ ] **E1a：历史reader与纯证明。** 复用已双审E0，增补实际只读inputs reader（缺文件不创建outbox，私有绑定来源与context可重探测）、实际双源/全归档reader、私有VerifiedRecoveryHistory、唯一prove_merge/prove_publication和canonical builders及契约测试。覆盖原回执角色、完整评审、实际Git和二次恢复所需内嵌证明。不接业务apply，不改变生产恢复/Session入口，不开放裸history/seed构造；未使用的core私有consumer可局部说明后续接线，不为消除lint制造旁路。原E1–E7中编排测试仍待E1b，E1a测试只能称纯consumer/contract history fixture证据。
- [ ] **E1b：完整消费接线与恢复。** 以E1a实际提交复核接口，一起交Task/Run/Evidence业务apply、全部Session/daemon/writer生命周期、私有staging/空目标恢复和原E7完整矩阵。不得拆成先apply merged后补proof，或先开放恢复后补constructor/同步校验；E1接线前置表与共享第3.1节全部适用。真实producer/App真人仍归后续阶段。

- [ ] **E1接线前置：有效 constructor 与两阶段共享同步。** 固定采用共享契约第3.1节的 `ProjectSession::new(hub,catalog,inputs,now)`、`ProjectSession::sync_shared(session,catalog,inputs,now)`、私有 fast sync 与 `SessionSyncError`，不保留无上下文有效 constructor。先核实际 checkpoint/initial tail 才返回业务 view；共享路径短锁 capture→锁外实际 Catalog/双源归档/inputs/canonical/Git→来源 probe→短锁坐标重核/install，失效至多三轮，proof/cache 不能由公开裸值铸造。runtime outbox 在锁外由 core reader 读取，缺文件产生同项目合法 empty；本步不让现有 claim producer 提前写 confirmed。E0 WIP 不作为冻结 API，先记录双审后实际 SHA 及 helper 映射，不导入未来 F/P2 模块。下表为必须迁移的旧调用范围，具体错误继续点、所有权和旧 ack/staging 规则只维护于共享第3.1节：

| 旧调用/路径（Root `3dd78a9`） | E1 必须到达的证明边界 |
| --- | --- |
| `ProjectSession::new/open`、`sync` 的 SnapshotRequired、`bind_catalog` | new 返回前核实际来源与初始 tail；SnapshotRequired 未准备时不 restore/越过 cursor；bind 仍仅身份核对，不当 authority gate |
| daemon `project` 冷开、`renew_existing_read` 临时 Session | 保留 Q1 cold-only registration gate/临时 Session 完整生命周期、原项目文件锁与 owner/common_dir 绑定；constructor 不持 projects 注册表/live session 锁，已注册快路不等 gate；并发首次注册者等待后复用 Arc，不先开第二 Hub、不新增报忙策略，临时读取不创建缺失 hub/绑定 |
| daemon `execute` / `renew_read_hub` / 现有请求续租调用 | 先锁外准备 inputs 并 sync_shared，再短锁执行业务；PreparationRequired 释放锁补准备，已接受 mutation 保原 eid/result，不盲重跑；F 删除 rebuild 请求续租仍归 F |
| daemon `tick`、`tasks::expire`、Session reconcile_pending | 每轮接实际 Catalog/inputs 与共享证明边界；慢准备期间合法系统 lease 由真实协议回执/当前 fences 继续维护，业务 apply 待已核 prefix |
| daemon WS 初始握手与 `stream_events` 每轮 | 每轮同样预核新 canonical/checkpoint；仅交付已核 prefix，不能只首轮 validated_catalog 后一直无 Catalog sync |
| `validate_authority_shared`、SnapshotWriter 的 expire/acquire/checkpoint、写前续租回调、record_written | authority 与 sync 共用两阶段准备/安装；保 writer→短 session 锁序，回调若需 proof 先释放 session 锁；裸 install_checkpoint 不成为授信入口，慢 Git/归档不阻塞系统续租 |
| recovery prepare / staging new / target rebuild 后 sync | prepare 无需 Session；confirm_rebuild/internal execute 接 existing Option<Arc>，None 由 core actual-open 私有 raw target，Some 安全退役并复用原真空 Hub 所有权、不 reopen；私有 VerifiedRecoverySeed 仅来自已核来源/批准请求/跳号变换，Git 持久并安装后才返回/切换有效 Session，不以公开 seed/projection 绕过 |
| status/画布/订阅水位、TaskService Status 原 `hub.contiguous_seq()` | 业务 seq 改由 Session 实际 applied_seq 提供；hub head 与业务 cursor 落后时可读最后已核 view 并明确待核，不能显示未应用的新水位或未核 merged |

此接线属于 E1 消费边界完整性，不依赖 F/P2 producer。纯 StateProjection/TaskService apply 仍可无 Catalog；有效 Session、reader、writer 与恢复安装必须先通过同一 core proof。不得新增通用协调器、运行时跳过开关或安全 stub。

- [ ] **E1：先写恢复尾提交红测。** 使用 recovery 现有 unit fixture（阶段 A 后为批 2 版本，prepare 按上方固定参数调用）：写快照，随后以明确标注的 contract history fixture 经真实 hub 保存原非 stale task.claimed、完整 finish.started 与 verifier finish.completed 的 confirmed，再在权威 main 放同 tree 的任务末提交（FR-12 五项关联 trailer）与符合共享第1.2/4节 schema 的已批准 publication commit/`towermd/B0` 标签；再加普通快照 trailer 提交及未知 `Towermd-Future: raw value` 提交。调用真实 prepare/核心恢复编排；旧 prepare 遇任何 Towermd trailer 就拒绝。记录末提交 SHA，重复准备/恢复事实扫描生成的 eid 必须相同。按发布总表 P2 已冻结澄清，实施此 parser 时即同步 FR-12 的第五行示例；缺 epoch 的旧四行任务 trailer 只留诊断，不据此认证 merged。

Run：`cargo test -p towermd-core recovery::tests::trailers_and_baseline_tags_restore_verified_business_state --locked`。Expected 红：现有“尚不支持恢复的 Towermd trailer”。
- [ ] **E2：实现内部 Git 观察器。** `recover_git_facts(catalog, saved, history: &VerifiedRecoveryHistory) -> Result<Vec<NewEvent>, String>` 为私有函数，使用 `git rev-list --reverse --topo-order <authority>` 正序遍历；排除 saved.commit 的祖先集合（private 源不在 main 祖先链时不能使用错误的线性 `source..main` 假设），按归档 eid 再去重。每个候选提交读取消息最后 trailer block，不把正文中 `Towermd-*` 当 trailer。已知键为 Task/Run/Module/Evidence/Snapshot/Epoch；Run/Module 是 Task 的伴随键，不能误报未知。快照必须校验实际 state/归档/trailer一致再补 `snapshot.written`；基线标签只是候选：对权威 main 可达提交上的直接/annotated `refs/tags/towermd/B<n>` 调用共享 VerifiedPublication builder，核对精确 Decision/manifest/原合法批准回执/冻结范围/hash/trailers/tag peel 后，才由 P4 同一 builder 产生 baseline.created。bare tag、unknown manifest 或仅 human 字样只诊断，不激活基线；P4 producer 未落地时只测契约历史夹具，不造批准 API。已知 publication trailer 键为 Baseline/Approval/Publication/Content-Hash/Epoch，分类独立于 Task/Snapshot。

```rust
fn recovered_event_id(commit: &str, kind: &str) -> Result<String, String> {
    crate::events::derived_eid(commit, kind)
}
```

所有 `merge.done`、`snapshot.written`、`baseline.created`、未知 trailer 的 `alert.raised` 都通过这个函数，不能用 UUID 或 hub 猜 data。一个提交多条未知 Towermd 键聚合成一条 alert，data 保存 commit 和所有原始行，顺序不变；未知键不阻止其他有效事实恢复。重复 Task/Evidence 等已知键或值非法不推断成功，生成带原文的 gate.failed/hold.opened 恢复诊断（同样按 commit+type 确定 eid），任务保持未合并。
- [ ] **E3：实现并证明业务恢复，不只补事件名。** normal observer/P2/P5/recovery/outbox 共用 `prove_merge(catalog,&VerifiedRecoveryHistory,&MergeProofInput)`，边界与 canonical data 按 [2026-10-17-shared-recovery-proof-contract.md](2026-10-17-shared-recovery-proof-contract.md) 第 3 节。必须取得原非 stale 已接受 claim（完整 Task/Run 与主 Module+touches 全 fences）、完整 FrozenFinish 起始事件、原 verifier 完成记录及必要精确人工评审；来源仅已核 archive/confirmed，孤立对象文件/passed/process 文字/raw pending 不授信。Task trailer Epoch 显式匹配主模块，Task+Epoch 不误判 Snapshot；真实Git tree/基底沿E先交付、P2后复用的唯一task_gates纯规则；prove_merge仅证既成事实，正常integrator先在唯一finish流程核实时fences/期限/G0–G5，签名与边界见共享第3节。历史原接受链不受新恢复 epoch 否决，但旧事件重发仍 stale，不续活旧 Run；不为证明补存全部 renew。返回 canonical merge.done 携完整原 claim/freeze/pass/必要 review 上下文，保证二次仅 snapshot/archive+Git 可核；所有入口同 commit 的完整 NewEvent 必须一致，不用 verified:true 绕过。发件箱自带 merged 声明也只按实际 commit 交该证明器重建，不能直接 apply 自报状态。

在 core 的 `StateProjection` 与 `TaskService.apply` 加相同 `merge.done` 业务应用：写 Task.state=merged，关联 Run 已结束并存 Evidence 引用；使后续依赖检查能观察到已合并事实。纯 apply 没有 Catalog，不能自动把依赖任务 proposed 改为 ready。依赖任务仍保持 proposed，由后续真实 claim 经完整 G0 后先记录 task.state=ready，再记录 task.claimed；不绕过 R13 的状态顺序。ProjectSession 在应用任何新 merge.done 前调用相同 core 校验器，统一 reader 在接受恢复归档时也核对其中的完成证明；不以 `verified:true` 布尔字段作为绕过。事件带齐已验证数据，使整份归档重放与 checkpoint 投影一致；`merge.done` 仅含字符串日志不够。未知/stale 事件仍不改变业务。canonical merge.done/合法publication是重新认证的既成Git事实，guards为空；其proof中的旧事件仍保留原guards，不授予当前权限。审批与释放顺序：恢复已验证 Git 合并事实后，仅将仍运行且未合并的任务/Run 释放，不能把刚恢复的 merged 任务又重置成 ready。

E3 同时逐条完成共享第6节新增的 Session 接线矩阵：constructor/restore/SnapshotRequired/canonical tail 的假 proof 不得可见 merged；冷开/临时读取并发仍只有一个持原文件锁的 Hub；慢 proof 与 lease/status 并发且业务水位真实；来源/ack/floor/inputs 改变拒绝过期准备；后台 tick 与 WS 后续轮次不能漏检；hub floor/ack=101 而 Git snapshot=103、ack 失败时，从实际双源可达历史核精确旧 ack.commit 的 Snapshot/全归档，不能错拿最新 checkpoint；私有 staging seed 只来自已核原上下文/批准，裸 seed 无法进入有效 Session。missing/empty hub 且 Git 已有合法快照时 prepare 可待批而不构造假业务 Session，确认通过私有 raw target；已有注册合法空 Session 则原所有权安全退役/复用，不要求重启、不再 Hub::open；失败不发布有效 Session。保留 C3a 的慢归档与并发续租回归，不以锁内同步 Git 换取通过。
- [ ] **E4：逐条补证据红绿。** 测试名 `merge_recovery_requires_matching_evidence_tree`、`merge_recovery_accepts_only_snapshot_canvas_tree_differences`、`recovered_merge_unblocks_dependent_task`、`outbox_merge_claim_without_git_proof_cannot_complete_task`、`unknown_trailers_aggregate_one_alert_with_raw_lines`、`run_and_module_companion_trailers_are_known`、`baseline_tag_on_unreachable_commit_is_not_restored`、`all_recovered_git_eids_are_commit_and_type_derived`。Run：

```bash
cargo test -p towermd-core --test recovery_inputs merge_recovery_requires_matching_evidence_tree --locked
cargo test -p towermd-core --test recovery_inputs merge_recovery_accepts_only_snapshot_canvas_tree_differences --locked
cargo test -p towermd-core --test recovery_inputs recovered_merge_unblocks_dependent_task --locked
cargo test -p towermd-core --test recovery_inputs outbox_merge_claim_without_git_proof_cannot_complete_task --locked
cargo test -p towermd-core --test recovery_inputs unknown_trailers_aggregate_one_alert_with_raw_lines --locked
cargo test -p towermd-core --test recovery_inputs run_and_module_companion_trailers_are_known --locked
cargo test -p towermd-core --test recovery_inputs baseline_tag_on_unreachable_commit_is_not_restored --locked
cargo test -p towermd-core --test recovery_inputs all_recovered_git_eids_are_commit_and_type_derived --locked
```

同时执行共享契约第 6 节的缺 claim/错 touches/raw pending/孤立 passed/bare tag/unknown manifest/二次无旧 outbox 矩阵。每条测试同时断言实际 Task/Run/依赖业务投影；解阻测试先断言依赖任务仍 proposed，再调用真实 TaskService claim，证明 G0 的依赖检查通过且 ready→claimed 事件顺序成立；失败证据有 trailer 也不得 merged。`merge_recovery_requires_matching_evidence_tree` 覆盖未知 Evidence、result=failed、非验证器署名、tree 错误各一例。
- [ ] **E5：加入持久输入 adapter 红测并最小实现。** 在测试中通过 `RecoveryOutbox::append_pending` 写真实文件后 drop/reopen，再执行产品恢复，不直接传临时 Vec。归档里一条非心跳事件重放两次只保留原 seq/eid；两个 host 文件同 eid 内容冲突拒绝；legacy renew 记录被丢弃；保留旧 guards 的证据事件被 hub 标 stale；未带 guards 的普通审计事件保留，不能给旧事件换新 guards。API、JSONL 与 core 私有 append_durable(hub,outbox,original,now) 组合入口按共享契约第 2 节实现；E先交付helper/reader，现有真实claim的daemon接线在P2 Task1完成且阻断Task2提前开工；加入共享第6节confirmed失败→reopen/status真实state仍可见/本次uncertain、持续同ID失败不success/解除仅补原Event、archiveDuplicate协调、全guard epoch高水位、digest双态及外层stale replay/内嵌原proof二次恢复测试。producer端到端由P2/P3覆盖claim直接finish依赖修复、completed不重跑、后台Hold/taskexpire/pendingclaim三路径confirmed。

```bash
cargo test -p towermd-core --test recovery_inputs persisted_outbox_survives_reopen_and_deduplicates_against_archive --locked
cargo test -p towermd-core --test recovery_inputs recovery_drops_renewals_and_preserves_old_guards_as_stale --locked
cargo test -p towermd-core --test recovery_inputs conflicting_eid_or_malformed_complete_outbox_line_is_rejected --locked
```

Expected 红：无恢复输入适配/重复 eid 又生成一份或续租被重放；Expected 绿：原接受证明只从已核归档/confirmed 取得，Snapshot 新格式仍无 receipts；pending 不能凭正文相同升级为已接受。完整行写入后 fsync 失败的输入不返回成功，保留原 eid 重开可辨识；不能用“清空 outbox”掩盖不确定结果。
- [ ] **E6：联结 staging 顺序。** prepare 绑定输入摘要；staging seed 使用来源 Snapshot、归档推导 receipts、新 recovery_gap，并清旧租约。按顺序记录人批准、hub.rebuilt、有效 Git 事实、去重后的普通 outbox、未合并运行释放，再 acquire 新 main lease；source 的补发 snapshot.written 走确定性 eid 幂等。先构建可验证的 Git/Evidence 事实表，再按提交正序落事件；已核confirmed的原非stale claim/freeze/pass可用于历史证明；内部recovery replay直接走staging HubProtocol.append，不经producer append_durable/persist_confirmed、不写第二confirmed。新seq/time/stale外层与原confirmed为不同角色，reader按恢复边界校验、不强等；二次无旧outbox依靠canonicalproof内原非stale回执复核，不给旧Run当前权限。raw pending 与原已 stale 回执不能升级为接受证明。所有提交给 hub 的事实使用批 2 的 `append(event, now)`/guards，所有租约操作用协议专门方法。构建连续前缀 snapshot 与完整月归档，读回全归档重放与业务 checkpoint 比较相等后，才进入 Git 边界写入。
- [ ] **E7：绿测。** `cargo test -p towermd-core --test recovery_inputs --test snapshot --test tasks --locked` 与 `cargo test -p towermd-core recovery::tests --locked`。Expected：恢复业务状态成立，后续依赖 G0/claim 可通过；没有远端客户端、凭 Agent done 完成的转换或 hub 解析业务 data。

### 阶段 F：真实 App 确认入口与只读需恢复状态

**Files:** Modify `crates/towermd/src/daemon.rs`、`main.rs`、`crates/towermd-core/src/recovery.rs`、`snapshot_service.rs`、`project_session.rs`；Test `crates/towermd/tests/daemon_cli.rs`。

- [ ] **F1：写真实 daemon 失败测试。** 在 `daemon_cli.rs` 复用 `Fixture::new/add/json/success/git`，加入下列完整 HTTP helper 和测试。`init` fixture 沿用批 1 已修正的显式 `--docs-dir`；不重新写旧 journal 解析器。

```rust
fn confirm_http(f: &Fixture, request: &Value, client_type: Option<&str>, actor: &str)
    -> reqwest::blocking::Response
{
    let endpoint = f.json(f.root.path(), &["daemon", "status"]);
    let url = format!("http://127.0.0.1:{}/hub/rebuild/{}/confirm",
        endpoint["port"], request["request_id"].as_str().unwrap());
    let client = reqwest::blocking::Client::builder().no_proxy().build().unwrap();
    let mut call = client.post(url)
        .header("x-towermd-instance", endpoint["instance"].as_str().unwrap())
        .json(&serde_json::json!({"actor": actor}));
    if let Some(kind) = client_type { call = call.header("x-towermd-client", kind); }
    call.send().unwrap()
}

#[test]
fn app_confirmation_restores_private_snapshot_after_real_hub_loss() {
    let f = Fixture::new();
    f.add("T-1", "A");
    let saved = f.json(f.root.path(), &["snapshot"]);
    assert_eq!(saved["status"], "written");
    assert_eq!(f.git(&["rev-parse", "refs/towermd/snapshot"]), saved["commit"]);
    f.success(f.root.path(), &["daemon", "stop"]);
    let hub_dir = f.root.path().join(".git/towermd");
    fs::remove_dir_all(&hub_dir).unwrap();
    let status = f.json(f.root.path(), &["status"]);
    assert_eq!(status["status"], "recovery_required");
    assert!(!hub_dir.exists(), "只读 status 不创建 hub");
    let request = f.json(f.root.path(), &["hub", "rebuild"]);
    assert!(!hub_dir.exists(), "CLI 请求不创建 hub");
    for kind in [None, Some("cli"), Some("agent"), Some("unknown")] {
        assert_eq!(confirm_http(&f, &request, kind, "human:test").status(),
            reqwest::StatusCode::FORBIDDEN);
        assert!(!hub_dir.exists());
    }
    let response = confirm_http(&f, &request, Some("app"), "human:test");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let rebuilt: Value = response.json().unwrap();
    assert_eq!(rebuilt["status"], "rebuilt");
    assert!(rebuilt["seq"].as_u64().unwrap() > request["seq_boundary"].as_u64().unwrap());
    let status = f.json(f.root.path(), &["status"]);
    assert!(status["tasks"].as_array().unwrap().iter().any(|t| t["id"] == "T-1"));
}
```

Run：`cargo test -p towermd --test daemon_cli app_confirmation_restores_private_snapshot_after_real_hub_loss --locked`。先在阶段 B/C 已通过的版本观察红：status 被 authority 拒绝或端点 404；不能拿 B 尚未完成的 Deferred 作为 F 的红测。
- [ ] **F2：实现请求零续租和持久登记。** 删除现 rebuild_request 的整个 renewal closure，CLI 在 main 的通用预续租列表也排除 Hub Rebuild（即使带 `--run`）。使用核心 prepare 与 RecoveryOutbox reader，在 `runtime/rebuild-requests/<request_id>.json` 原子写入 `StoredRebuildRequest { request, directory, common_dir, expires_at }`，期限为创建时刻加 5 分钟，重复同请求不得无条件续期；App 可从 `GET /hub/rebuild/:request_id` 读取，返回对象不含可修改的批准字段。所有文件写入经过现有 runtime 原子 JSON helper；请求不会新建/修改 hub，不写 `.towermd/`、用户目录、Git ref，不伪造“已批准”事件。
- [ ] **F3：实现端点和独立恢复打开路径。** 注册 axum `/hub/rebuild/{request_id}/confirm`（路由源码使用当前 axum 版本实际语法，文档以 `:request_id` 表示），先检查 instance，再严格 `x-towermd-client: app`，再反序列化 `ConfirmRebuildBody { actor: String }` 且 deny_unknown_fields。无/未知/cli/agent client 返回 403，旧 instance 返回 409；缺失请求 404、过期/来源变化 409、格式非法 422。确认函数读取已保存路径/expiry，不接受 HTTP directory/source/expiry 代入。

正常有效 constructor 会阻止从不相符的空 hub 恢复；私有 `project_for_rebuild` 仅保留 Q1 clone/单实例/registration gate 与同一 writer 所有权，复用已注册 Arc 或传 None 给 E1 的核心 confirm_rebuild，不自行打开第二 Hub 或构造空业务 Session，不启动快照 worker。core 在核 source/current authority/input digest/human/expiry 后实际打开私有 raw empty target，或核真空水位后安全退役已注册空 Session 的业务入口并复用原 protocol；沿共享第3.1节保持唯一所有权，非空/损坏目标拒绝。Git 新边界确定持久、已核 seed/rebuild/ack/install 完成后，core 才返回新有效 Session 或同一已切换 Arc，F 接收并发布正常服务/安排 reconcile；失败不发布。损坏的现存 hub 可登记请求但确认不静默覆盖损坏正文，保留诊断与原文件；本批的可执行恢复目标是缺失或可读的空 hub。
- [ ] **F4：实现无副作用只读 status。** 有权威 main 且 hub missing/empty、统一 reader 找到合法 snapshot 时返回 `status="recovery_required"`、message“需恢复”、来源 commit/seq/epoch 和只读 source 任务摘要；不调用 `tasks::execute(Status)`、`TaskService.expire/renew_binding` 或 `subscribe`（批 2 subscribe 也可能 expire），不生成任何 lease 事件。只读路径用核心无写存储探测和已验证 Snapshot；正常写入口仍拒绝。保留批 1 的 `authority_unavailable` 语义，缺慢层来源不能假装已有可恢复项目。
- [ ] **F5：逐条真实回归。**

| 测试名 | 必须断言 |
| --- | --- |
| `rebuild_request_never_renews_even_with_explicit_run` | 建真实运行，冻结 journal 原字节/lease expiry/seq；CLI rebuild 带与不带 run，重启前后都不变；请求文件存在且无 approved 事实 |
| `recovery_status_is_read_only_and_task_writes_are_rejected` | 已有快照、empty hub，冻结 hub 文件字节，重复 status 退出 0、message“需恢复”、不 expire/renew；task add/claim/snapshot 写拒绝 |
| `confirm_rejects_process_actor_expiry_and_unknown_fields` | App + `process:towermd-cli` 拒绝；过期 stored request 409；额外 approve/expiry/directory 字段 422；hub/ref 无变化 |
| `confirm_requires_saved_request_and_current_instance` | 请求文件删掉/错 ID 404、旧 instance 409，App 不能凭 payload 自造批准 |
| `confirm_rejects_changed_main_private_source_or_outbox` | 请求后分别推进 main/private 或 append outbox，确认 409，目标 hub 仍不可写；新请求 ID 改变 |
| `rebuild_does_not_expose_cli_approval_commands` | `hub rebuild --approve/--confirm/--execute/--actor` 均 clap 拒绝；公开 JSON request deny_unknown_fields |
| `app_rebuild_makes_old_epoch_writes_stale` | App 恢复后，同进程 daemon 私有测试中经 ProjectSession 的协议入口追加旧 guards 事件，原 eid 被定序且 stale，Task/依赖不变；正常新领取取得严格更高 epoch |

Run：

```bash
cargo test -p towermd --test daemon_cli rebuild_request_never_renews_even_with_explicit_run --locked
cargo test -p towermd --test daemon_cli recovery_status_is_read_only_and_task_writes_are_rejected --locked
cargo test -p towermd --test daemon_cli confirm_rejects_process_actor_expiry_and_unknown_fields --locked
cargo test -p towermd --test daemon_cli confirm_requires_saved_request_and_current_instance --locked
cargo test -p towermd --test daemon_cli confirm_rejects_changed_main_private_source_or_outbox --locked
cargo test -p towermd --test daemon_cli rebuild_does_not_expose_cli_approval_commands --locked
cargo test -p towermd daemon::tests::app_rebuild_makes_old_epoch_writes_stale --locked
```

最后一条在 daemon 私有测试模块用真实 TCP App 确认并通过同进程 session 追加旧 guards；不在进程外并发打开正在运行的本地 hub 文件，也不为了测试新增公共 append 路由。旧“hub rebuild 会续租”测试要改名并反转断言，保留稳定过期时间与重启检查。旧“显式 run + missing journal 必须失败”改为请求成功且不创建 hub，因为请求不再依赖续租。
- [ ] **F6：绿测产品路径。** `cargo test -p towermd --test daemon_cli --locked`。Expected：停 daemon→删真实 hub 目录→status 需恢复→CLI 请求→非 App 拒绝→App 确认→status 正常→旧 epoch stale 全链路通过，且没有在测试中直接构造 AppConfirmation 代替 HTTP。

### 阶段 G：新边界先持久、安装失败不能重用

**Files:** Modify `crates/towermd-core/src/recovery.rs`、`snapshot_service.rs`；Test recovery 单元模块、`crates/towermd/tests/daemon_cli.rs`。

- [ ] **G1：保留并扩展已有安装故障红测。** 测试名 `persisted_boundary_then_failed_install_requires_strictly_higher_request`，沿用现 recovery 测试的本地安装失败注入：staging 已定序、私有或 main Git 提交成功，在 target install 前制造真实持久化 I/O 失败。确认失败，target 不开放写，但新的 Git 边界已经可读。恢复权限后重复旧请求必须拒绝；prepare 的新 source_seq 大于旧 boundary，新 seq/epoch boundary 再加 2³²，新 App 请求才能成功。main/private 两种目标各跑一次。

Run：`cargo test -p towermd-core recovery::tests::persisted_boundary_then_failed_install_requires_strictly_higher_request --locked`。Expected 红：旧 prepare 只读 main 导致重复旧边界，或 target 失败后把新 private 边界当成不存在。
- [ ] **G2：保持严格安装顺序。** 在 target 持锁时确认最初读取的目标 seq/epoch 与当前相同；staging 的完整 `RebuildSeed`/tail 通过批 2 协议重建。顺序严格为：新 snapshot 和全部恢复事实归档校验→Git 写入且确定成功→staging snapshot.written/ack 成功→再次确认目标未变→target.rebuild 安装→切换正常服务。任一步不确定结果保持拒绝写，并显示已持久的 commit；禁止 catch 后用旧请求重新 stage 或释放边界。source_epoch 使用来源 `epoch_high_water`，不能使用较小 writer_epoch。
- [ ] **G3：逐条故障红绿。** `git_boundary_failure_never_installs_recovery`、`target_changes_during_recovery_are_not_overwritten`、`expired_confirmation_before_final_git_write_leaves_target_inactive`、`recovery_reopens_without_unique_staging_state`。Run：

```bash
cargo test -p towermd-core recovery::tests::git_boundary_failure_never_installs_recovery --locked
cargo test -p towermd-core recovery::tests::target_changes_during_recovery_are_not_overwritten --locked
cargo test -p towermd-core recovery::tests::expired_confirmation_before_final_git_write_leaves_target_inactive --locked
cargo test -p towermd-core recovery::tests::recovery_reopens_without_unique_staging_state --locked
```

最后一条删除 staging 临时目录、drop/reopen 后只靠最大 Git 快照与持久 outbox 再次 prepare，重建结果一致；不能把唯一状态留在 staging 或请求缓存。
- [ ] **G4：从真实 daemon 故障入口回归。** 在 daemon 私有测试模块使用真实 TCP Router/真实临时项目和内部注入的 target 安装失败（不增加生产开关）执行完整 POST；观察 HTTP 错误含“边界已持久、须新请求”、随后 status 仍拒写，新请求 source/边界严格增加。Run：`cargo test -p towermd daemon::tests::app_confirm_install_failure_consumes_persisted_boundary --locked`。这条不能只调用核心 execute 冒充 daemon 回归。

### 阶段 H：增量触发、退避与可观察快照

**Files:** Create `crates/towermd/src/snapshot_schedule.rs`；Modify `crates/towermd/src/daemon.rs`、`main.rs`、`crates/towermd-core/src/project_session.rs`；Test 调度/daemon 单元模块及 `crates/towermd/tests/daemon_cli.rs`。

- [ ] **H1：固定派生调度模型。** 新内部 `SnapshotSchedule` 持有 `last: Option<SnapshotLast>`、`deferred_reason: Option<String>`、`deferred_since: Option<u64>`、`next_attempt_at: Option<u64>`、`event_deadline: Option<u64>`、连续失败次数、当前 episode 是否已 alert 和最后 consumed seq。`SnapshotLast { commit, seq, epoch, written_at }` 的 written_at 仅取真实 snapshot.written 时间，未知为 null；不能以 status 查询时刻伪造。`status.snapshot` 固定含 `last/deferred_reason/deferred_since`。

调度使用注入的可信单调时钟计算 deadline，展示 since 用 Unix 毫秒；进程重启可清退避，但从已验证 Git 快照+session opaque/tail 重建 last、业务 due 和未处理事件 deadline。不能把这些派生字段当唯一持久事实，也不能让没有变化的 status 强制读取全部归档。
- [ ] **H2：先写确定性调度红测。** 给模块定义 `observe(kind: &str, stale: bool, seq: u64, now: u64)`、`eligible(now: u64) -> bool`、`deferred(reason: String, now: u64, interval: u64) -> bool`（返回本 episode 是否首次越界需 alert）、`written(last: SnapshotLast)`、`next_attempt_at() -> Option<u64>`，字段保持私有。测试实际调用这些方法：

```rust
#[test]
fn snapshot_events_debounce_at_first_event_deadline() {
    let mut s = SnapshotSchedule::default();
    s.observe("hold.resolved", false, 1, 1_000);
    s.observe("proposal.resolved", false, 2, 20_000);
    assert!(!s.eligible(25_999));
    assert!(s.eligible(26_000));
}

#[test]
fn deferral_is_exponential_capped_and_alerts_once() {
    let mut s = SnapshotSchedule::default();
    assert!(!s.deferred("busy".into(), 0, 8_000));
    assert_eq!(s.next_attempt_at(), Some(1_000));
    assert!(!s.deferred("busy".into(), 1_000, 8_000));
    assert_eq!(s.next_attempt_at(), Some(3_000));
    assert!(!s.deferred("busy".into(), 3_000, 8_000));
    assert_eq!(s.next_attempt_at(), Some(7_000));
    assert!(!s.deferred("busy".into(), 7_000, 8_000));
    assert_eq!(s.next_attempt_at(), Some(15_000));
    assert!(s.deferred("busy".into(), 15_000, 8_000));
    assert_eq!(s.next_attempt_at(), Some(23_000));
    assert!(!s.deferred("busy".into(), 23_000, 8_000));
}
```

Run：`cargo test -p towermd snapshot_schedule::tests --locked`。先补类型/方法签名使测试编译，再看到旧无 deadline/退避行为失败，最后最小实现。指数用 checked/saturating 运算，`min(1000 * 2^(failure-1), interval)`；since 第一次失败固定，换原因不重置，成功 written 后清 episode。interval 可小于 1 秒时以上限 interval 为准。
- [ ] **H3：实现增量触发 hook。** ProjectSession 只在新有效事件被成功应用后通知/暴露新增事件批，daemon 消费一次。按规格真实类型接线：经证据验证的 `merge.done`（或已验证 task.state 的关闭转换）对应任务关闭，`hold.resolved` 对应中断处理，`proposal.resolved` 对应提案处理，另含 `baseline.created`。若批 2 已有 `task.closed`/`proposal.handled` 内部观测命名，只作这些规范事件的内部映射，不新增公共 hub 事件类型，也不把任意 task.state 当关闭。上述触发采用固定 25 秒聚合窗口（给正常本地 Git 工作留出 5 秒验收余量）：第一条事件设 deadline，窗口内后续事件合并进同一份快照且不能推迟 deadline。远端故障时按退避/alert 报告推迟，不声称仍保证 30 秒写入成功。忽略 stale、heartbeat、snapshot.written、snapshot.ack 和自身 `alert.raised`；观察 seq 去重，不扫描全历史。M0 未有生产批准/处理中断命令时只接核心事件流，不为验收新增假 CLI 完成命令。
- [ ] **H4：把 deadline 接到真实 tick。** tick 仍每秒做轻量维护/增量同步，但在加载 Catalog、列工作树或读归档前检查 shutdown/single-flight/eligible/next_attempt。条件不满足直接跳过重工作。触发 due、reconcile、event_deadline 统一安排同一个后台 worker；失败或 Deferred 更新退避与 reason/since，成功更新 last 并清失败。手动 `towermd snapshot` 可立即尝试，不重置未成功 episode 的 since。超过 snapshot_interval 只追加一次 `alert.raised`，data 带原因、since、interval；追加由协议定序、署名 process，不能读取 alert 后触发自己。alert eid 存在会话/归档可重建，重启若同一 episode 已有 alert 不再重复。

周期 due 独立于 H3 的快速事件 deadline：`ProjectSession` 增量维护 `periodic_pending_since: Option<u64>`，首次未覆盖的已确认心跳或其他需归档事件设值，包含 stale 的归档记录；`snapshot.written`/ack/自发提醒本身不设值。新增 `pub fn periodic_due(&self, interval: u64, now: u64) -> bool`，从真实最近写入时间（未知时从该 since）计算周期，只在存在未覆盖 tail 时为 true；每次安装 checkpoint 从剩余 tail 重建该小字段，普通 tick 不扫描历史。租约续租不触发 25 秒路径，但默认周期到达时允许一次实际 Git checkpoint+ack，截断心跳 tail。完全空 tail 或仅 `snapshot.written` 不触发快照；持续续租最多按正常 interval 出版，不形成每秒或每 25 秒自激循环。断网/Deferred 仍使用同一退避与提醒，不能为压缩绕过 Git 持久确认。
- [ ] **H5：先写真实 tick 计数回归再实现。** 在 daemon 私有测试构造器注入可信时钟、真实 Catalog load/Git operation 计数器及可控 writer 输出，不增加运行配置。`snapshot_backoff_skips_catalog_git_before_deadline` 连续推进每秒 tick 1 小时，持续故障；断言 Catalog load、worktree list、archive load 只在退避 deadline 被调用，稳态间隔上限为配置 interval，未变化业务事件 apply 数为 0，自身 alert 总计 1。另 `successful_snapshot_clears_deferral_status` 断言 last 来自真实 written commit，失败信息清除。

Run：`cargo test -p towermd daemon::tests::snapshot_backoff_skips_catalog_git_before_deadline --locked` 和 `cargo test -p towermd daemon::tests::successful_snapshot_clears_deferral_status --locked`。红是旧每秒 reconcile 都做全量操作；绿的计数以实际被调用函数为准，不能仅测 Schedule 计数器自述。
- [ ] **H6：真实 background hold.resolved 回归。** 在 daemon 内部 integration 测试启动真实 Router/Project/worker，使用内部已取得的 session protocol 追加正常 `hold.resolved` 并同步，不公开新测试路由。不直接调用 schedule/write；只运行真实 tick 通路，可信时钟推进 25 秒到触发点并 await worker，窗口内每隔 2 秒追加事件且总计只落一份快照，读真实 private/main ref 与 state：新快照包含该事件且完成时间减事件观察时间 <= 30_000ms。随后纯 renew 在周期之前不触发第二次快速快照；推进到配置 interval 后实际出版并 ack，断言 floor 前进、旧 heart 不留永久 eid、tail 仅保未覆盖事件。停止产生续租且只剩 snapshot.written/自发 alert 时，再推进多个周期不得自激出版。另 daemon_cli 测试在真实进程中读取 status 验证 last/deferral 字段。

```bash
cargo test -p towermd daemon::tests::background_hold_resolved_writes_real_git_snapshot_within_thirty_seconds --locked
cargo test -p towermd daemon::tests::snapshot_and_heartbeat_events_do_not_create_snapshot_loop --locked
cargo test -p towermd daemon::tests::heartbeat_only_tail_is_compacted_at_periodic_deadline --locked
cargo test -p towermd --test daemon_cli snapshot_status_reports_verified_last_and_deferral --locked
```

Expected 红：事件后无快照或写入只靠原长间隔；绿：30 秒内真实 Git commit，未靠人工调用 snapshot 或单独的 hook 单元测试。
- [ ] **H7：绿测。** `cargo test -p towermd --locked`。Expected：快照调度与批 1 后台 fetch 限频互不阻塞，批 2 tick 不随历史增长的断言保持，关机仍等待在途快照/重试明确结束。

## 联合验收、文档与精确提交

- [ ] **V1：同步最小产品说明。** `docs/product-principles.md` 信念二把“定期快照进 main”改为：

> 慢层 md 与 tower.canvas 以 main 上的用户文档目录为准；活跃快层以唯一 hub 为准，快照按 FR-91 持久化到 main；无远端且 main 已检出时写私有引用 refs/towermd/snapshot，恢复取两处 (epoch, seq) 最大的等价可验证快照，待主干可写再搬送。私有引用只保存快照，不改变慢层权威来源；所有快照仍只由持有主干租约的守护进程经同一写入器完成。

同步“一处定序，一个提交者”的概括，说明 main 或 FR-91 私有引用；不改产品其他信念，不改 Note.md 无对应快照规则的内容。规格已批准相同规则，不再新增裁决。README 说明无远端快照、需恢复只读 status、App 确认入口和三次竞争重做；双语 CHANGELOG 未发布区各加对应用户变化，不能写“真人 App 按钮已完成”。todo 记录每组真实红绿输出、故障窗口证据与最终提交 SHA。
- [ ] **V2：运行完整必要验证。** 在实施工作树由运行器显式传入独立 CARGO_TARGET_DIR：

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
rg -n 'AppConfirmation|LeaseChange|journal.json|receipts|events\(0\)' crates/towermd/src/daemon.rs crates/towermd-core/src/recovery.rs crates/towermd-core/src/snapshot_service.rs
```

Expected：格式、全部 Rust 测试、clippy 通过；rg 命中仅为私有确认/明确兼容层说明，daemon 不解析旧日志、不调用私有批准构造；没有新依赖/Cargo feature。若仓库有已知无关 clippy 失败，记录基线和差异，不静默声称全绿。
- [ ] **V3：逐项核对验收证据。**

| 要求 | 真实证据位置 |
| --- | --- |
| 无 origin、main 在任意 worktree 检出仍持久化 | B1/B3、F1；dirty/staged/untracked、index flags、全部 heads/HEAD/文件字节不变 |
| 最大双来源、相同边界冲突、保新代码搬送 | B4/C1/C4；归档/状态等价、当前 lease 一致的同 opaque 搬送、新 holder 无业务仍以当前 epoch Advance、旧配置不相容拒绝 |
| 一次竞争成功、更高 fence 停止、三次有界 | D1/D4/D5；真实 bare Git 与真实 CLI，不用纯 mock 代替 |
| 删除真实 hub→CLI 请求→非 App 拒绝→App 恢复→旧 epoch stale | F1/F5/F6；确认可打开待恢复 project，status 无 expire/renew |
| trailer/tag/unknown/outbox 去重/renew 丢弃恢复真实业务 | E1/E4/E5；Task merged、Evidence/tree 有效、依赖仍 proposed 且后续真实 G0→ready→claim 解阻、全归档重放相等 |
| 新边界持久而 target 安装失败后不可重用 | G1/G4；main/private 两条路径，下次 source/boundary 严格增加 |
| 退避不每秒重工作、超过 interval 一条提醒 | H2/H5；真实 Catalog/Git 计数、重启可重建、status last/reason/since |
| hold.resolved 真实后台 30 秒内写出快照 | H6；真实 worker/真实 Git、stale/heartbeat/自身事件不 loop |
| 连续前缀 101/103/102 | A2 保留批 2 生产 SnapshotWriter 测试，最终 workspace 回归通过 |

- [ ] **V4：自审优雅性与故障边界。** 逐项查看仅状态文件叠加、唯一 writer、双 ref 安全、无公开构造批准、无唯一 staging/cache 状态、无 Agent done 转换；authority 缓存含私有 SHA，重启读两处补 ack；离线普通读不依赖 fetch；不引入完整 M2 client。每个 diff 都能追溯本批整改项。若有问题先修对应最小实现，再只重复受影响测试及 workspace 总验收。
- [ ] **V5：精确提交闭环。** 只显式暂存本批实际改变且验证过的路径，检查 `git diff --cached --stat`、`git diff --cached --check` 与 `git diff --cached`，不得夹带 S5 工作树或其他任务改动。建议提交 `fix: persist local snapshots and restore hub through app confirmation`。运行 `git rev-parse HEAD`，将真实 SHA 与验证结果写入 todo；若尚未提交必须标“未提交”。本批不推送、不发版；完成后按总计划继续批 4/5 和首个未签名可用 App。

## 计划自审记录

本计划已对照总表 R3/R7/R4/R14、规格 FR-83/87–94/107/110 与 16.2 逐项映射；已检查跨 crate 私有确认、批 2 guards/opaque/eid 接口、双源相等例外、Git 后安装故障与实际后台事件链。当前所有实施步骤仍未执行。执行前实际批 2 API 复核是必要前置，不是允许跳过编译/行为红测的免责条款。本文只交付实施计划，不代表本批代码、测试或提交已经完成。
