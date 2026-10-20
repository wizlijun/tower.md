---
type: Plan
title: M1 P5 main 事后检测与人工处理实施计划
description: 通过薄观察器检查权威 main 的新增提交，复用唯一 Git 证明、主干写入与持久人工事实，实现确定性禁止中断及显式处理。
status: draft
tags: [towermd, M1, P5, main, observer, TDD]
sources:
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-3-snapshot-recovery.md
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-4-verifier-drafts.md
  - resource: docs/superpowers/plans/2026-10-17-m1-task-evidence.md
  - resource: docs/superpowers/plans/2026-10-17-m1-human-intervention.md
  - resource: docs/superpowers/plans/2026-10-17-m1-baseline-approval.md
  - resource: docs/superpowers/plans/2026-10-17-m1-app-foundation.md
  - resource: AGENTS.md
  - resource: tasks/lessons.md
---

# M1 P5 main 事后检测与人工处理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 每次成功刷新或无远端 main 推进后检查未观察提交，为未经批准的受保护改动只生成一个确定性禁止中断，并经 App 显式撤销、转提案或认领。

**Architecture:** daemon 只交付明确的权威 SHA 和调度；核心在 Session 锁外读取逐提交前后 tree，在短锁内重核项目与权威 revision 后追加。合法出版先协调，再复用批 3/P2 唯一 Git/Evidence 证明；人工处理复用 P3 持久事实/receipt、P4 Proposal/TaskRecheck 与同一个 writer。

**Tech Stack:** 已有 Rust、Git CLI、serde/serde_json、sha2、uuid、tokio、axum、reqwest；P6 既有 Tauri/Svelte 适配器。不新增依赖、云服务或运行时绕过开关。

---

## 开工事实与依赖

这是已提交的实现计划（原提交8ed564d）；本次边界补充已通过独立文档审查，observer尚未实施、未运行产品测试。当前 P5 只有研究；M0 source 尚未完成全审。2026-10-17 主目录 MainRefresh::schedule 调 origin::refresh_origin_main 后丢弃返回 SHA，只存 fetched_at；无 remote 时 observe 直接返回，不安排检查。不能把它写成已有 observer。

只读核对 `/private/tmp/towermd-m0-remediation-2` 的迁移版本 `4c18f7a`：daemon 为 `Arc<Mutex<ProjectSession>>`；Session 有 sync/bind_catalog、HubProtocol、projection、cursor/head、内部 authority_key/authority_saved。bind_catalog 绑定 project_id/common_dir，authority_key 是快照验证缓存，**不是**本计划所需的权威 revision 原子令牌。当前 Catalog::load 只从 main reference 取配置；它本身不足以检查“删除配置”提交。当前 origin::refresh_origin_main 返回实际 fetch SHA，CAS 更新直接 origin/main，不应另造 fetch。

开工必须等待 M0 批 2–5、P2 的纯路径守卫/唯一证明器、P3 持久人工事实及 receipt、P4 的统一 writer/合法出版证明全部验证并合入；读取实际源码和提交号后映射下方契约。P6 可先搭壳，Task 5 原生/前端接线等其基础文件落地。编译缺接口不是行为红测，不回接旧 Hub + LeaseChange，不复制 proof 或 writer。

**首次 inspection frontier 已由用户明确裁决。** 2026-10-17 用户答复“整笔启用提交作为导入起点（推荐）”，确认此前推荐工程假设；FR-98 允许 init 由人自行提交，主规格 FR-103 与以下边界一致。此前按 lessons 检索 sotvault 未找到另定边界的依据；本次以用户的明确答复为准：

- 在完整权威 main 历史中，首次引入当前 project_id 配置的提交为启用锚点；该提交整笔既有内容作为导入上下文，不产生旧违规，也不批准 B0。
- 锚点之后全部提交均检查，包含 B0 之前、daemon 首次运行前的违规；配置删除再添加不重置锚点。
- 不能选 daemon 首见 HEAD、当前 B0、快照时间或缓存游标作为首次边界。浅历史、锚点多义、项目来源不明、非快进回退不能证明完整检查时为 inspection_incomplete，阻止合并认证。
- 这是唯一实现语义，不提供 runtime flag 切换导入范围；今后若调整，先同步规格及对应验收。

Task 1 按主规格已冻结边界实现锚点与 fail-closed 测试；其余前置交付要求仍保留。此冻结不代表 observer 源码或产品验收完成。

## 文件地图与共同规则

| 文件 | 单一责任 |
| --- | --- |
| `crates/towermd-core/src/main_observer.rs`（新） | 不可变扫描输入、逐提交候选、检查结果及可重建 frontier；不执行 Git 写入 |
| `crates/towermd-core/src/main_violation.rs`（新） | 原始违规 diff、处理 subject、人工动作校验；不创建第二人工事实框架 |
| `crates/towermd-core/src/task_gates.rs`（P2） | 唯一 raw-z tree diff/前后 layer 保护判定，供 finish 与 observer 共享 |
| `crates/towermd-core/src/recovery_git.rs`（批 3/P2） | 唯一 trailer/Git/Evidence 证明与确定性 Git 事实构造 |
| `crates/towermd-core/src/{project_session,snapshot,human_intervention}.rs` | 增量投影、Hold 与人工事实、统一 outbox/receipt 持久链 |
| `crates/towermd-core/src/{snapshot_service,repository_writer,baseline_review,proposals}.rs` | 既有 snapshot/慢层协调、显式撤销写入与精确 Proposal/TaskRecheck 复用 |
| `crates/towermd/src/main_refresh.rs` | 成功 SHA 交付、无 remote 本地 main 观察、single-flight 和重试调度 |
| `crates/towermd/src/main_observer_jobs.rs`（新） | 锁外扫描与短锁接受结果的 daemon 组合，不含保护/完成规则 |
| `crates/towermd/src/main_violation_routes.rs`（新） | 仅 App 的固定读取/动作端点、项目绑定与 AppGate |
| `crates/towermd/src/{daemon,main}.rs` | 服务注册、启动/恢复安排与 CLI 无批准入口 |
| `crates/towermd-core/tests/{main_observer,main_violation,recovery_inputs}.rs` | 真实 Git、唯一证明三入口与故障窗口 |
| `crates/towermd/tests/main_observer_cli.rs`（新） | 真实二进制/daemon、App 客户端、重启/恢复与用户工作树保护 |
| `src-tauri/src/{commands,daemon_client}.rs`、`src/lib/daemon/{types,client}.ts` | P6 固定 invoke/typed wire 接线 |
| `src/components/AttentionList.svelte`、对应测试 | 显示原提交/diff 和三种显式动作，复用共享卡片样式 |

以下接口均为**待实现契约**。公共 wire 类型派生 Clone/Debug/Serialize/Deserialize/PartialEq，固定请求 deny_unknown_fields；原始 OKF 对象继续宽容读取、保留未知键。完整 Git OID 从 Git 解析，不接收缩写；路径用 bytes，不能有损 UTF-8、trim NUL 或换行。

```rust
// main_observer.rs；字段可序列化，Proof 内部构造保持私有。
pub struct AuthorityToken {
    pub project_id: String, pub common_dir: std::path::PathBuf,
    pub reference: String, pub revision: String,
}
pub struct ObservationInput {
    pub root: std::path::PathBuf, pub authority: AuthorityToken, pub anchor: String,
    pub inspected: std::collections::BTreeSet<String>,
}
pub struct ProtectedDiff {
    pub commit: String, pub parents: Vec<String>,
    pub before_tree: String, pub after_tree: String,
    pub changes: Vec<crate::task_gates::RawChange>, pub patch: Vec<u8>,
    pub protected_changes: Vec<crate::task_gates::RawChange>,
    pub object_ids: Vec<String>, pub hash: String,
}
pub enum InspectionState { Complete, Incomplete { reason: String } }
pub struct Candidate { pub diff: ProtectedDiff, pub message: String }
pub struct ObservationBatch {
    pub authority: AuthorityToken, pub anchor: String,
    pub candidates: Vec<Candidate>, pub state: InspectionState,
}
pub fn scan(input: &ObservationInput) -> Result<ObservationBatch, String>;
pub fn protected_diff(root: &std::path::Path, commit: &str,
    project_id: &str) -> Result<ProtectedDiff, String>;
```

`scan` 只读 Git；读取对象、diff、解析历史均不持 Session 锁。`ProtectedDiff.changes/patch` 保存原commit完整diff，protected_changes是其中受保护部分，同时保留原始模式/OID、确定性patch bytes、对象IDs。hash为版本化规范序列化的project_id、commit、parents、前后tree和精确protected_changes/对应patch的SHA256，排序规则固定，不能仅hash卡片文本；人工审阅同时展示完整diff和保护子集。二进制diff保留OID与binary patch；无法读取blob/配置/层级为incomplete，不能当无保护改动。

## Task 1：完整逐提交 tree 检查与可重建 frontier

**Files:** 新建 `main_observer.rs`、`tests/main_observer.rs`；修改 P2 `task_gates.rs`、`lib.rs`。不接 daemon、hub 或 writer。

- [ ] **1.1 定义共享比较接口并写真实 Git 测试 helper。** P2 已有 RawChange/parse_raw_z；新增 `tree_changes(root,before,after)` 与 `protected_changes(root,before,after,project_id)`，返回同一 RawChange。以 `git diff-tree --raw -z --no-renames --no-abbrev -r` 比较并保留完整 OID，删除/添加覆盖 rename 两端，合并提交逐父比较并合并受保护路径；不使用工作树 diff。

```rust
// tests/main_observer.rs；后续测试复用这个真实 Git helper，不伪造 Git 输出。
use std::{fs, path::{Path, PathBuf}, process::Command};
use tempfile::{tempdir, TempDir};
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(root)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid",
               "-c", "commit.gpgsign=false"])
        .args(args).env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim_end_matches('\n').into()
}
fn repo() -> (TempDir, PathBuf) {
    let tmp = tempdir().unwrap(); let root = tmp.path().join("repo");
    fs::create_dir(&root).unwrap(); git(&root, &["init", "-qb", "main"]);
    (tmp, root)
}
fn put(root: &Path, path: &str, text: &str) {
    let target = root.join(path); fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, text).unwrap();
}
fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]); git(root, &["commit", "-qm", message]);
    git(root, &["rev-parse", "HEAD"])
}
#[test]
fn old_layer_and_renamed_delete_stay_protected() {
    let (_tmp, root) = repo();
    put(&root, ".towermd/config.yaml", "project_id: project\ndocs_dir: plans\n");
    put(&root, "decisions/one.md", "---\ntype: Decision\nlayer: L1\nid: D-1\n---\nold\n");
    commit(&root, "init");
    put(&root, "decisions/one.md", "---\ntype: Decision\nlayer: L3\nid: D-1\n---\nnew\n");
    let changed = commit(&root, "lower layer");
    let diff = towermd_core::main_observer::protected_diff(&root, &changed, "project").unwrap();
    assert!(diff.protected_changes.iter().any(|c| c.path == b"decisions/one.md"));
    fs::create_dir_all(root.join("plans/intents")).unwrap();
    git(&root, &["mv", "decisions/one.md", "plans/intents/renamed.md"]);
    let renamed = commit(&root, "rename");
    let diff = towermd_core::main_observer::protected_diff(&root, &renamed, "project").unwrap();
    assert!(diff.protected_changes.iter().any(|c| c.path == b"plans/intents/renamed.md"));
}
```

- [ ] **1.2 跑行为红。** 先完成类型/空扫描的可编译骨架，再运行 `cargo test -p towermd-core --test main_observer old_layer_and_renamed_delete_stay_protected --locked -- --exact`；红必须是旧 layer 或 rename 保护断言失败，不能仅 import 不存在。
- [ ] **1.3 最小保护算法。** 从父 tree 和当前 tree 分别读取 `.towermd/config.yaml`；两份 docs_dir 的 intents/arch/specs/baselines/state 全目录及 tower.canvas 都参与保护；另读取变化 blob 前后 layer，任一 L0–L2 即保护；config 登记项改动也保护。删除 config 使用父配置，docs_dir 迁移同时保护旧/新目录；project_id 改动标违规且阻止旧 Session 绑定新项目。

```text
for each commit in topo-order oldest-first:
  read all parents and tree; root uses empty tree
  for each parent: shared tree_changes -> shared protected_changes
  union changes by (parent tree, path, old/new mode, old/new OID)
  retain exact bytes and Git patch; derive object IDs from both blobs
  config missing after deletion does not remove before-side protection
  invalid/missing blob, shallow boundary, unparseable layer => incomplete
```

- [ ] **1.4 实现已冻结 frontier 的历史选择。** 按主规格 FR-103 与用户已确认的边界实现对应锚点测试： `git rev-list --reverse --topo-order <authority>` 找 current project_id 首次引入点；只排除它及祖先，扫描之后全部可达提交。merge 的不同 ancestry 出现多个独立引入点则 incomplete；同 project_id 删除重加不重置。已扫集合是派生缓存，祖先闭包必须先验证；non-FF 不盲用 `old..new`，重新证明锚点与可达集合，不明回退保持 incomplete。
- [ ] **1.5 逐条红绿测试。** `pre_enable_slow_history_is_import_context`、`violation_after_enable_before_first_daemon_is_found`、`config_delete_readd_never_resets_frontier`、`docs_dir_changes_check_both_directories`、`canvas_state_and_external_slow_layer_are_protected`、`delete_and_binary_modes_retain_original_diff`、`shallow_or_ambiguous_anchor_is_incomplete`；每条先看断言失败，再补对应最小分支。
- [ ] **1.6 绿验证及精确提交。**

```bash
cargo test -p towermd-core --test main_observer --test task_gates --locked
git diff --check
git add crates/towermd-core/src/main_observer.rs crates/towermd-core/src/task_gates.rs crates/towermd-core/src/lib.rs crates/towermd-core/tests/main_observer.rs
git commit -m "feat(observer): inspect protected trees across main history"
```

预期全部通过并记录实际 SHA；锚点与 incomplete 反例必须一起验证，不以派生缓存或 B0 替代首次边界。

## Task 2：先协调合法出版，复用唯一 Git/Evidence 证明

**Files:** `recovery_git.rs`、`baseline_review.rs`、`snapshot_service.rs`、`main_observer.rs`；`tests/recovery_inputs.rs`、`tests/main_observer.rs`。不新增 proof 文件。

- [ ] **2.1 扩展现有唯一证明器 typed 输出。** P2 已定义 parse_trailer_context/prove_merge/VerifiedMerge；批3真实快照reader和P4 VerifiedPublication继续负责其原证明。增加私有构造的VerifiedObservation，只有recovery_git可构造；HTTP不能提供它。原P2 main观察器与恢复扫描都改为调用同一个prove_observation，先得到该commit完整的任务proof及保护诊断，再构造一次Hold，不允许两个调用者分别计算半份payload。

```rust
// recovery_git.rs；source只能来自可信投影+已校验归档/outbox。
pub enum ProtectedDisposition {
    Authorized { commit: String, approval_eid: Option<String> },
    Unauthorized { reasons: Vec<String> },
    Incomplete { reason: String },
}
pub fn prove_protected_commit(catalog: &crate::catalog::Catalog,
    source: &crate::snapshot::StateProjection,
    diff: &crate::main_observer::ProtectedDiff)
    -> Result<ProtectedDisposition, String>;
pub struct VerifiedObservation {
    authority: crate::main_observer::AuthorityToken,
    anchor: String, commits: Vec<VerifiedObservedCommit>,
}
struct VerifiedObservedCommit {
    commit: String, events: Vec<crate::hub_protocol::NewEvent>,
}
pub fn prove_observation(catalog: &crate::catalog::Catalog,
    source: &crate::snapshot::StateProjection,
    batch: crate::main_observer::ObservationBatch)
    -> Result<VerifiedObservation, String>;
// 所有已在 Git 出版但未 Recorded 的 publication 仍走原 writer 的协调入口。
// 不把“有 trailer/tag/作者名”作为 Authorized 的构造函数。
```

- [ ] **2.2 写矩阵红测。** 在现有 recovery_inputs 的真实 Git/Evidence 夹具加案例：合法 snapshot、私有 snapshot 合法搬送、批准 baseline、push 后 append/ack 失败、伪 snapshot 作者、未知 trailer、裸 B 标签、合法任务五行 trailer、same-tree amend/squash、错 Evidence tree/epoch、正文假 trailer。用同一候选分别调用 normal/recovery/outbox 的既有证明入口，结论和确定性 eid 必须一致。

```rust
#[test]
fn snapshot_author_name_alone_does_not_authorize_protected_commit() {
    let (_tmp, root) = repo();
    put(&root, ".towermd/config.yaml", "project_id: project\ndocs_dir: plans\n");
    commit(&root, "init");
    put(&root, "plans/state/state.json", "{\"forged\":true}\n");
    let sha = commit(&root, "process:towermd-snapshot\n\nTowermd-Snapshot: 7\nTowermd-Epoch: 1\n");
    let diff = towermd_core::main_observer::protected_diff(&root, &sha, "project").unwrap();
    assert!(!diff.changes.is_empty());
    // 使用真实 Catalog 与空可信投影；伪快照正文不能变成合法 source。
    let catalog = towermd_core::catalog::Catalog::load(&root).unwrap();
    let source = towermd_core::snapshot::StateProjection::default();
    let result = towermd_core::recovery_git::prove_protected_commit(&catalog, &source, &diff).unwrap();
    assert!(matches!(result, towermd_core::recovery_git::ProtectedDisposition::Unauthorized { .. }));
}
```

- [ ] **2.3 跑红。** `cargo test -p towermd-core --test main_observer snapshot_author_name_alone_does_not_authorize_protected_commit --locked -- --exact`；编译后故意只按 trailer 放行的最小骨架必须被断言击败。
- [ ] **2.4 最小证明顺序。** 先恢复 outbox 原事实并协调 exact publication；合法 snapshot 校验 state/全部归档/epoch/seq/合法 writer 及提交，仅允许 state 差异；合法 baseline/proposal 校验精确 commit、ApprovalFact human actor、object IDs、hash、target baseline、Decision/manifest、writer 与合法 tag。P4 已出版未 tag 的候选由同 writer 补 tag/Recorded；失败为 pending/incomplete，不先开违规 Hold。不能给任意 tag 构造批准。

```text
known published candidate -> original snapshot/P4 reconcile -> validated proof
snapshot proof covers only exact allowed state diff
slow proof covers only exact approved protected diff and target baseline
remaining protected diff -> Unauthorized; unavailable trusted source -> Incomplete
task trailer -> P2 prove_merge, never a protected-path exemption
unknown trailer -> existing raw alert, never authorization
```

- [ ] FR-107 的 state/canvas tree 等价只用于证明任务 Evidence；它不豁免同提交未批准慢层变化，也不让任务拥有 state writer 身份。合法 task+Evidence+Epoch 不误判坏 snapshot；对同一提交任务 proof 可成功而未批慢层仍生成禁止 Hold，任务认证须受 inspection 完整性与 G2 原门禁约束。
- [ ] **2.5 补故障红绿。** `published_snapshot_before_ack_is_reconciled_not_held`、`private_promotion_before_ack_is_not_new_slow_approval`、`approved_baseline_before_recorded_reconciles_exact_commit`、`unknown_tags_and_trailers_never_exempt_slow_diff`、`canvas_evidence_equivalence_does_not_exempt_unapproved_slow_diff`；注入点仅 cfg(test)，不用环境开关或 HTTP 绕过参数。
- [ ] **2.6 绿验证并提交。**

```bash
cargo test -p towermd-core --test main_observer --test recovery_inputs --test baseline_publish --test snapshot_service --locked
git diff --check
git add crates/towermd-core/src/recovery_git.rs crates/towermd-core/src/baseline_review.rs crates/towermd-core/src/snapshot_service.rs crates/towermd-core/src/main_observer.rs crates/towermd-core/tests/recovery_inputs.rs crates/towermd-core/tests/main_observer.rs
git commit -m "feat(observer): reuse verified publication and merge proofs"
```

## Task 3：确定性禁止 Hold、持久事实与可恢复游标

**Files:** 新建 `main_violation.rs`、`tests/main_violation.rs`；修改 `project_session.rs`、`snapshot.rs`、`human_intervention.rs`、`recovery_git.rs`、`lib.rs`。

- [ ] **3.1 定义 typed 事实与状态。** 复用注册 type: Hold、P3 HumanState/attention、P4 recheck 集合；只扩展 HoldBody::Prohibition，不造新注册对象或第二 Hold 框架。

```rust
pub struct Violation {
    pub id: String, pub project_id: String,
    pub original: crate::main_observer::ProtectedDiff,
    pub reasons: Vec<String>, pub resolved: Option<ViolationResolution>,
}
pub enum DiagnosticScope {
    MainCommit { commit: String },
    TaskBranch { run: String, commit: String },
}
pub enum DiagnosticOrigin { TaskProof, ProtectedPath }
pub struct MainDiagnostic {
    pub id: String, pub origin: DiagnosticOrigin, pub code: String,
    pub paths: Vec<Vec<u8>>, pub detail: serde_json::Value,
}
pub struct MainCommitDiagnostics {
    pub commit: String, pub original: crate::main_observer::ProtectedDiff,
    pub proof: Vec<MainDiagnostic>, pub protection: Vec<MainDiagnostic>,
}
pub struct HoldDiagnosticExtension {
    pub hold: String, pub commit: String,
    pub original: crate::main_observer::ProtectedDiff,
    pub protection: Vec<MainDiagnostic>,
}
pub enum ViolationResolution {
    Reverted { commit: String, request_id: String },
    Proposed { proposal_id: String, draft_revision: String, request_id: String },
    Claimed { approval_eid: String, protected_hash: String, request_id: String },
}
pub fn hold_eid(commit: &str) -> Result<String, String> {
    crate::events::derived_eid(commit, "hold.opened")
}
```

- [ ] **3.2 写重复观察红测。** 同一个真实 ProtectedDiff、相同失败原因重复接受两次，断言仅一条 hold.opened、一份原 diff、attention 数量一次；Session drop/reopen 后相同 eid/结果。原数据固定取 commit，不包含当前时间/检测 host 等跨主机变化值；完整 NewEvent 构造复用批 3 确定性 Git 事实规则。

```rust
#[test]
fn violation_identity_is_exact_commit_and_event_type() {
    let commit = "a".repeat(40);
    let id = towermd_core::main_violation::hold_eid(&commit).unwrap();
    assert_eq!(id, towermd_core::events::derived_eid(&commit, "hold.opened").unwrap());
    assert_eq!(id, towermd_core::main_violation::hold_eid(&commit).unwrap());
    assert_ne!(id, towermd_core::main_violation::hold_eid(&"b".repeat(40)).unwrap());
}
```

- [ ] **3.3 跑红并最小实现。** `cargo test -p towermd-core --test main_violation --locked`；行为红必须包含重复计数/丢失原diff的集成断言，不以纯哈希测试替代。新main路径一次prove_observation计算MainCommitDiagnostics，收齐proof/protection后按(origin,code,paths,id)排序去重，一次构造hold.opened；未知trailer原alert仍独立。诊断id是固定版本规范编码(origin,code,paths,detail)的SHA256，detail只有提交/绑定/OID等冻结值。任何证明输入读取失败先Incomplete，不生成半份Hold。
- [ ] 固定完整NewEvent：host使用确定性Git来源标识 `git:<commit>`，不是本次检测机器；guards为空，obj为hold_eid，layer为L5，kind为hold.opened。request_id由同commit固定推导，request_body仅含原commit/project/diff及稳定原因code，p3_receipt包含原Hold结果；不含当前main tip、恢复host、随机ID或墙钟时间。事件t/seq仍由hub定序，不能伪装成检测器的本地时刻。原因code按固定顺序去重，OS/网络错误为incomplete诊断，不进入确定性违规payload；跨主机与normal/recovery/outbox逐字节比较NewEvent。
- [ ] **3.3a 兼容已存在P2 main Hold。** 查明其scope为MainCommit且eid等于hold_eid后，永不再append另一份hold.opened。只追加 `hold.diagnostics_updated`，eid=`derived_eid(commit,"hold.diagnostics_updated")`，data为HoldDiagnosticExtension；它仅携本commit完整P5保护诊断与原diff，绝不复制当前整份Hold或依赖旧proof原因/timestamp/resolution。StateProjection/HumanState按diagnostic.id单调并集到同Hold，保存原opened_seq/原P2原因/答复/处理历史，禁止重复计数；更新不重新open。即使旧Hold已处理，也保留原处理审计并明确新增保护诊断仍需处理，不能把旧G3处理当慢层批准。
- [ ] 旧Hold升级的extension所有机器/恢复入口都由同不可变ProtectedDiff构造；同eid payload必须逐字节相同。第一次新版完整open已有相同protection时不需要update；若竞争主机已追加旧式open，accept按现存scope选择extension，不能补写冲突open。TaskBranch的预合并G2 Hold继续使用P2原request/run/fence身份，不使用derived_eid(commit,"hold.opened")；两个scope共享Hold产品类型，不能互相吞掉或复用批准。
- [ ] FR-87身份保持真实commit+真实eventtype：main的hold.opened与hold.diagnostics_updated各只有一个eid，不把scope拼成假eventtype后写另一种type。TaskBranch沿P2 gate Evidence/request的稳定UUID关联。P2本身仍是未发布计划，兼容只处理开工时实际存在的P2格式；未识别旧格式报告诊断，不建设通用历史迁移框架。
- [ ] **3.4 明确持久与锁边界。** 新 `ProjectSession::accept_observation(catalog:&Catalog,batch:VerifiedObservation,outbox:&mut RecoveryOutbox,now:u64)->Result<InspectionState,String>` 仅接受recovery_git私有构造的已证明批；调用前daemon取得当前authority token，短锁内sync/bind_catalog，并比较token的project_id/common_dir/reference/revision与当前已安装权威token及直接ref OID。变化则丢弃批、安排新扫，不追加旧结果。短锁内只允许有界ref/OID读取，不执行diff/blob/历史扫描。Session增加内部installed_authority，更新必须在同一短锁内由核验后的Catalog设置；authority_key不替代它。
- [ ] 采用 P3 已接线 `append_p3_event` 的 fsync→hub.append→sync 链，观察事实携固定内部 request_id/原 request_body/p3_request/p3_receipt；所有入口从 commit 原数据构造相同事件，不代入恢复 host/时间。先协调批 3/P2 原 proof，再逐 commit 追加其完整确定性结果；hub/追加失败不标该 commit 已检查，不越过失败点。

```text
scan/prove with frozen trusted projection outside session lock
acquire short lock -> sync
compare expected authority token == installed authority token
for candidate in deterministic order:
  accept prebuilt immutable proof/violation events after binding recheck
  durable outbox append -> hub append -> sync
  only after every required event accepted mark this commit inspected
  on uncertain result retain original eid/body/guards; retry same event
```

- [ ] **3.5 游标没有唯一状态。** inspected/frontier 缓存仅优化扫描，从权威 Git 历史、已校验 snapshot/完整归档及 outbox 重建；合法无事件的普通提交可在缓存丢失后重扫。删 cache/SQLite/hub 不可把“未观察”变“已批准”；恢复扫描也调用同一保护/唯一 proof，生成同 hold_eid。保留原已 resolved Hold；重扫不能 reopen 或重复 attention。非FF 无法证明时不清旧 Hold，不认证任务 merged。
- [ ] **3.6 红绿故障矩阵。** `append_failure_never_skips_commit`、`uncertain_append_reuses_original_event`、`drop_cache_rescans_same_hold_eid`、`hub_rebuild_restores_original_violation_and_resolution`、`authority_or_identity_change_discards_batch`、`unchanged_authority_does_not_rescan_history`。再加 `proof_and_protected_diagnostics_open_one_complete_hold`（同commit两原因、一次open）；`legacy_main_hold_adds_one_stable_extension`（原事件bytes/原因/opened_seq不变）；`legacy_extension_retries_across_hosts_and_rebuild`（相同eid/data、count=1）；`task_branch_hold_does_not_mask_main_prohibition`；`resolved_legacy_gate_hold_does_not_approve_new_slow_diagnostic`。扫描计数在实际Git调用边界计，不测自述counter；恢复用批3真实App确认路径。
- [ ] **3.7 绿验证并精确提交。**

```bash
cargo test -p towermd-core --test main_observer --test main_violation --test recovery_inputs --test snapshot --locked
git diff --check
git add crates/towermd-core/src/main_violation.rs crates/towermd-core/src/project_session.rs crates/towermd-core/src/snapshot.rs crates/towermd-core/src/human_intervention.rs crates/towermd-core/src/recovery_git.rs crates/towermd-core/src/lib.rs crates/towermd-core/tests/main_violation.rs crates/towermd-core/tests/recovery_inputs.rs
git commit -m "feat(observer): persist deterministic violation holds"
```

## Task 4：MainRefresh 的薄调度与短锁接线

**Files:** 新建 `crates/towermd/src/main_observer_jobs.rs`；修改 `main_refresh.rs`、`daemon.rs`、`main.rs`；新建 `tests/main_observer_cli.rs`。

- [ ] **4.1 定义成功刷新输入。** 将现有 fetch worker 的 `Option<SHA>` 保留下来，通过 daemon 内部队列交付 `ObservedAuthority {common_dir,reference,revision}`；成功 fetch 才发送远端 SHA。无 remote 则解析 local main 精确 SHA，仍发同种观察输入；无 main 保留 authority_unavailable。不能使用 status 查询时重新读出的任意 HEAD 替代成功 SHA。

```rust
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObservedAuthority {
    pub common_dir: std::path::PathBuf,
    pub reference: String, pub revision: String,
}
pub(crate) struct MainObserverJobs {
    // 每个common_dir单飞；pending存最新明确SHA，completed为派生优化。
    entries: std::sync::Mutex<std::collections::BTreeMap<std::path::PathBuf, JobState>>,
}
pub(crate) struct JobState {
    pub running: bool, pub pending: Option<ObservedAuthority>,
    pub completed: Option<String>, pub last_error: Option<String>,
}
```

- [ ] **4.2 写真实 fetch 红测。** 扩展现 MainRefresh unit fixture 的 bare remote、clone、linked worktree；成功 fetch 后队列收到 remote/main 的确切 SHA，两个 worktree 共用单飞；失败 fetch 保留 fetched_at/已检查游标且不发 stale 成功通知；修复 URL 后同未检查 SHA 可重试。无 remote main 两次新提交均可观察。
- [ ] **4.3 跑红。** `cargo test -p towermd main_refresh::tests::successful_fetch_delivers_exact_authority_sha --locked -- --exact`、`cargo test -p towermd main_refresh::tests::local_main_is_observed_without_remote --locked -- --exact`；红为队列缺 SHA/本地早退，不是新增测试名未匹配。
- [ ] **4.4 最小 worker 算法。** 维护原 fetch 60 秒限流，不因 observer 失败重置 fetch 时间；成功取 SHA后以 Catalog 在该 SHA 的不可变读取入口冻结 project/config，锁外加载/扫描/协调 Git。只有 authority 改变或未完成检查才启动；ordinary tick/status 不重读全历史。扫描期间可继续续租/claim/status；最终短锁复核 Token，不持 Session 锁等 Git。writer 协调用原 writer 锁→短 Session 锁顺序，不反转。

```text
refresh success SHA / local main SHA changed -> enqueue by common_dir
worker consumes immutable SHA -> sync/freeze trusted projection under short lock
release lock -> reconcile original publication using shared writer -> scan/prove
if reconciliation changed authority: enqueue new SHA; discard old batch
short lock -> recheck installed authority and identity -> accept_observation
failure: retain pending SHA/error; retry without advancing completed
shutdown: stop new schedules, drain fetch/observer before snapshot drain
```

- [ ] `Catalog::load_at_revision` 只在核心新增只读固定 revision 入口；配置删除等无法构造当前 Catalog时，仍用 Session 已绑定 project_id 与历史 TreeContext完成保护检测并报告 identity/inspection 不完整，不默默注销旧项目。不能把不可信配置切换成新 hub 后漏掉原项目违规。
- [ ] **4.5 真实竞争红绿。** 用 cfg(test) barrier 阻塞真实 Git blob/diff 读取；另线程 status/lease renew 必须及时完成。暂停扫描后使 main 前进或 project_id/common_dir 绑定变化，旧批无追加，最新批被重试；append失败重试不要求 main再变。nonFF/shallow/删除config时状态含 `inspection_incomplete`、诊断原因为何阻认证。
- [ ] **4.6 绿验证并提交。**

```bash
cargo test -p towermd main_refresh --locked
cargo test -p towermd --test main_observer_cli observer_ --locked
git diff --check
git add crates/towermd/src/main_refresh.rs crates/towermd/src/main_observer_jobs.rs crates/towermd/src/daemon.rs crates/towermd/src/main.rs crates/towermd/tests/main_observer_cli.rs crates/towermd-core/src/catalog.rs
git commit -m "feat(daemon): observe exact refreshed main without holding session locks"
```

## Task 5：App 显式撤销、精确转提案与认领

**Files:** `main_violation.rs`、新 `main_violation_routes.rs`；P4 writer/proposals/baseline_review、P3 human_intervention/session/snapshot；P6 既有 native/wire/AttentionList 适配及测试。

- [ ] **5.1 定义唯一处理 subject 与固定 wire。** 每次 GET 都从权威 Git 重验原 commit 可达及精确受保护 diff，不用卡片当前摘要推断批准。target_baseline 使用 P4 相同精确目标标签语义，subject 另含当前 baseline 和 authority revision。

```rust
pub struct ViolationSubject {
    pub project_id: String, pub hold: String, pub original_commit: String,
    pub protected_hash: String, pub object_ids: Vec<String>,
    pub target_baseline: String, pub current_baseline: Option<String>,
    pub authority_revision: String,
}
pub enum ViolationAction {
    GenerateRevert { subject: ViolationSubject },
    Propose { subject: ViolationSubject, input: crate::proposals::ProposalInput },
    Claim { subject: ViolationSubject, comment: String },
}
pub struct ViolationActionRequest {
    pub request_id: String, pub action: ViolationAction,
}
pub fn validate_subject(expected: &ViolationSubject,
    actual: &ViolationSubject) -> Result<(), String> {
    if expected != actual { return Err("违规提交审阅内容或权威 main 已变化，请重新审阅".into()); }
    Ok(())
}
```

- [ ] GET `/app/main-violations/{hold}/review?directory=…` 返回 subject、原完整 diff/原因、当前处理状态与 blocker；POST `/app/main-violations/{hold}/action` body `{directory,human_id,request:ViolationActionRequest}`。沿 P3 AppGate/human_id校验，由 daemon 构造 human actor；CLI `/command` 不注册该动作，无 actor/approve/claim-main 参数。
- [ ] **5.2 写 TOCTOU 红测并运行。** subject 逐字段变更均拒绝；人审阅后 main推进、protectedDiff/hash/IDs/target变化，原click失败；重复同request原body返回原receipt，异body409。Run `cargo test -p towermd-core --test main_violation subject_ --locked` 与 `cargo test -p towermd --test main_observer_cli app_action_ --locked`。

```rust
#[test]
fn subject_rejects_new_main_and_changed_hash() {
    use towermd_core::main_violation::{ViolationSubject, validate_subject};
    let old = ViolationSubject { project_id:"project".into(), hold:"hold".into(),
        original_commit:"a".repeat(40), protected_hash:"b".repeat(64),
        object_ids:vec!["D-1".into()], target_baseline:"towermd/B1".into(),
        current_baseline:Some("towermd/B0".into()), authority_revision:"c".repeat(40) };
    assert!(validate_subject(&old, &old).is_ok());
    let mut moved = old.clone(); moved.authority_revision = "d".repeat(40);
    assert!(validate_subject(&old, &moved).is_err());
    moved = old.clone(); moved.protected_hash = "e".repeat(64);
    assert!(validate_subject(&old, &moved).is_err());
    moved = old.clone(); moved.object_ids.push("D-2".into());
    assert!(validate_subject(&old, &moved).is_err());
    moved = old.clone(); moved.target_baseline = "towermd/B2".into();
    assert!(validate_subject(&old, &moved).is_err());
}
```
- [ ] **5.3 最小撤销实现。** “生成撤销提交”是人的明确动作，不是自动修复。取得原统一 writer锁、fresh-fetch、校验 subject/main lease 和精确原diff；用临时 index/核心 Git plumbing 在最新main构造原commit的逆向变更，形成新的 revert commit，保留原提交历史。冲突/父选择不明返回 blocker，不自动扩大范围或选择 merge mainline；提交前再核 subject、lease和main。普通push/CAS、无remote main已检出pending、outbox fsync/receipt与P4完全相同，不调用用户工作树的git revert，不改heads/index/worktrees。
- [ ] revert 的受保护逆向变化绑定该人动作的精确hash/对象IDs/target，作为同一writer合法出版证明；不得成为任意后续diff的权限。成功发布新commit且原事实持久后才 `hold.resolved`；push成功回包失败恢复原publication/新commit，不再生成第二revert。原Hold、原commit、原diff和原因始终可查。
- [ ] **5.4 精确转提案。** 扩展P4 ProposalSource增加 `MainViolation {commit,protected_hash,hold}`；来源固定原commit/parents/diff，不取当前工作树。P4当前save_proposal/record_proposal强制run；P5须修改 `proposals.rs`、`snapshot_service.rs`、`project_session.rs` 这三个实现文件的调用来源签名，复用同一提案服务而不伪造Run：

```rust
// proposals.rs；此类型由core编排构造，不是HTTP/CLI可自报的wire参数。
pub enum ProposalCaller {
    TaskRun { run: String, fences: Vec<crate::hub_protocol::Fence> },
    MainViolation { subject: crate::main_violation::ViolationSubject,
        actor: String, receipt: crate::human_intervention::RequestReceipt,
        request_body: serde_json::Value },
}
// snapshot_service.rs：替换P4原run:&str，其他writer/outbox参数不变。
pub fn save_proposal(&self, session: &std::sync::Mutex<ProjectSession>,
    catalog: &Catalog, input: &ProposalInput, caller: &ProposalCaller,
    request_id: &str, outbox: &mut RecoveryOutbox, host: &str,
    now: impl FnMut() -> Result<u64, String>) -> Result<String, String>;
// project_session.rs：对应替换原run参数，不增加第二record实现。
pub fn record_proposal(&mut self, catalog: &Catalog, host: &str,
    proposal_id: &str, draft_revision: &str, caller: &ProposalCaller,
    request_id: &str, outbox: &mut RecoveryOutbox, now: u64) -> Result<String, String>;
```

- [ ] TaskRun保留P4原run/host/worktree/branch及全部live fences校验，actor仍process:towermd-cli、guards为原fences；Agent不能选择MainViolation。MainViolation仅由AppGate后的核心action编排构造：完整ViolationSubject已复核、actor为daemon构造的human，receipt沿P3统一request_id/body_hash/result_ref，request_body保存原typed请求。先查统一receipt，同ID原请求返回原Proposal ID；新请求的receipt先规划为pending，仅在同outbox fsync→hub→sync成功后登记有效，不把传入receipt本身当批准证据。
- [ ] MainViolation不要求项目存在任何Run，guards为空，不领取租约或假造fences；subject在writer读取和最终save/record前重验。Proposal ID由原request稳定确定，receipt.result_ref固定该ID；冻结draft实际revision由原writer保存，重试回读同ID及durable ref，不能再建一份。CLI继续只有TaskRun路径，伪造human/caller/receipt不能经通用endpoint进入MainViolation分支。
- [ ] 复用validate_proposal、冻结hash与审批/TaskRecheck流程；只把可提案慢层/canvas diff物化，state/config违规留原审计/blocker，不借提案修改快照或项目身份。App提供理由/影响/备选/建议，不能用空值自动批准。原Hold标Proposed只说明已转交，卡片保留“未批准/未撤销”；Proposal拒绝不删除违规，批准后真实新baseline仍走P4一次writer。受影响任务复用P4 RecheckFact和 **TaskRecheckBinding** 的解除入口，不造第二复核/批准事件。
- [ ] 红绿测试 `app_action_propose_without_any_run_uses_same_receipt`：从未创建Task/Run的项目，真实违规→App转提案成功，Run数量仍0；同request返回同Proposal ID/draft ref、outbox含原human/完整subject/receipt。删hub/cache后沿App确认恢复，回读原receipt与Proposal，不要求补Run。另测TaskRun缺/旧fences仍拒绝、CLI自报MainViolation被拒绝；运行 `cargo test -p towermd --test main_observer_cli app_action_propose_ --locked`。
- [ ] **5.5 精确认领事实。** 待点击、尚未接受的Claim仍调用validate_subject完整TOCTOU校验：审阅后main推进须重新GET/new view。成功接受后永久授权的范围仅是原commit+原exact protected diff，不能因之后普通main推进撤回。定义以下事实（不是新注册对象），它由合法App核心路径规划，不能由缓存或客户端自报恢复：

```rust
pub struct ClaimFact {
    pub claim_eid: String, pub request_id: String,
    pub accepted_subject: ViolationSubject,
    pub original: crate::main_observer::ProtectedDiff,
    pub actor: String, pub comment: String,
}
```

- [ ] 接受时先完成AppGate/human校验与精确subject校验，固定claim_eid沿P3原request稳定推导；追加 `hold.claimed` 保存完整ClaimFact，再按同请求追加 `hold.resolved` 及最终p3_receipt，结果为原claim_eid/hold/commit/hash。两条事件都走同一持久outbox原eid链，部分失败重试仅协调原事件。StateProjection/HumanState保存ClaimFact与原resolved审计，派生claim索引只由有效已定序事实及已验证完整归档重建；pending outbox不直接算已认领。
- [ ] 唯一 `recovery_git::prove_protected_commit` 增加ClaimFact读取：核对human actor、事实来源与成功接受时保存的完整request/receipt、原项目/原commit可达、原parents/tree/exactdiff/hash/objectIDs及接受时targetbaseline上下文，才将该精确原受保护diff识别为已批准慢层变更。恢复不要求**今天**authority_revision仍等于accepted_subject.authority_revision，也不要求今天targetbaseline仍等于当时目标；这两个字段用于证明原接受上下文，不是永久授权的新鲜度条件。当前project身份不符、原diff不符或来源不能证明仍拒绝，不能信缓存自报approved。
- [ ] Claim不自动创建baseline、不批准后续commit，也不伪造snapshot writer；state/非法配置等不能被“慢层认领”豁免。后续slow改动生成另一个derived_eid(new_commit,"hold.opened")，原Hold保持resolved/原原因可读。无guards有效认领随原commit永久保留；实际携旧guards且stale的事实仅历史。拒绝/超时不批准，恢复receipt不迁移授权。
- [ ] 红绿测试 `app_action_claim_survives_code_advance_and_hub_rebuild`：真实App认领成功→普通code main推进→停daemon删除hub/cache/index→App恢复，原Hold仍resolved、ClaimFact/原receipt唯一且可回读；随后新slowcommit产生另Hold，原认领不放行新hash。另测未接受旧subject在code推进后仍409、伪ClaimFact/outbox pending/错project/exactdiff不能授权。运行 `cargo test -p towermd --test main_observer_cli app_action_claim_ --locked` 与 `cargo test -p towermd-core --test recovery_inputs claim_ --locked`。
- [ ] **5.6 P6 最小接线。** native仅固定 `review_main_violation`/`act_main_violation` invoke，typed解码subject；AttentionList显示禁止形状+颜色+文字、原SHA/diff/原因、三按钮及pending/conflict。按钮提交刚展示的完整subject/request_id，不接受自由HTTP/路径/actor；成功刷新真实状态，超时显示uncertain并沿原request重试。共享样式用现app.css，不移动画布节点。
- [ ] **5.7 绿验证与提交。** 测真实 AppGate、三动作、冲突、main推进、旧hash、拒绝、相同请求重试、hub丢失恢复、回包不确定、main检出pending；无CLI人动作。运行后精确暂存上述已验证文件/hunk，提交 `feat(app): handle main violations with exact human review bindings` 并记录SHA；UI自动化不是真人验收。

## Task 6：真实 daemon 联合验收与交接

**Files:** `tests/main_observer_cli.rs`、`tests/recovery_inputs.rs`、P6组件测试；实施时 README、双语CHANGELOG未发布条目与 `tasks/todo.md`。本计划写作不改这些文件。

- [ ] **6.1 构建真实 Fixture。** 新测试文件用 CARGO_BIN_EXE_towermd、隔离 TOWERMD_RUNTIME_DIR、bare remote、clone与两个worktree；调用真实CLI启动daemon，HTTP携实际instance/App标记，不能直接append hold.opened/merge.done冒充产品链路。daemon时钟沿现cfg(test)组合点，不增生产skip flag。
- [ ] **6.2 逐案例走红→最小接线→绿。** 下表每行是独立测试；过滤命令逐项命中测试名，不以测试0项通过算证据。

| 测试名 | 必须证明的实际结果 |
| --- | --- |
| `observer_finds_pre_start_post_enable_violation` | 按最终冻结frontier，启用前旧slowhistory不泛报，启用后违规再启动可检，B0不是豁免边界 |
| `observer_retry_fetch_and_append_never_skips_commit` | fetch失败无游标推进；修复后检查；append失败保留pending，下一轮重试同eid |
| `observer_local_main_without_remote` | 无remote main新提交被检查，用户heads/index/worktrees无变化 |
| `observer_rebuild_rescans_same_eid` | 停daemon删cache/index/hub，保留Git/outbox，CLI rebuild→App确认后原Hold及receipt同eid，重复不计数 |
| `observer_protects_old_new_layers_dirs_and_deletions` | L1降L3、docsdir切换、config删重加、slow删除/rename均检出，原diff可读 |
| `observer_normal_publications_and_ack_crashes` | snapshot/private搬送/批准baseline/push后ack失败先协调且无误报，伪作者不免检 |
| `observer_task_proof_shared_with_recovery_outbox` | 合法Task/amend/squash通过；错tree/Evidence/epoch伪trailer不merged，三入口结论同一 |
| `observer_canvas_equivalence_still_holds_slow_change` | FR107 state/canvas差异可证Evidence，夹带未批slow仍有禁止Hold |
| `app_action_claim_hash_invalid_after_main_moves` | 尚未接受认领时，人审阅后main推进旧subject失败，重新展示才可点击；不撤销已接受ClaimFact |
| `app_action_propose_without_any_run_uses_same_receipt` | 项目从无Run也可经App转同一提案服务，恢复回读同Proposal/receipt且无伪Run |
| `app_action_claim_survives_code_advance_and_hub_rebuild` | 已接受原认领在code推进与hub/cache删除恢复后仍resolved，后续slowcommit另Hold |
| `app_action_revert_propose_claim_preserves_history` | 三动作经App真实human/outbox/receipt，原commit/Hold/diff不删除，revert是新commit |
| `observer_nonff_shallow_and_identity_changes_block_certification` | 历史/身份不明为inspection_incomplete，未证明不merged，不盲跳提交 |
| `observer_blocked_diff_keeps_lease_and_status_responsive` | Gitdiff阻塞时另模块claim/status/续租成功，短锁复核丢弃旧authority批 |

- [ ] **6.3 运行准确命令。**

```bash
cargo test -p towermd --test main_observer_cli observer_ --locked
cargo test -p towermd --test main_observer_cli app_action_ --locked
cargo test -p towermd-core --test recovery_inputs --test main_observer --test main_violation --locked
cargo test --workspace --locked
pnpm test --run src/components/AttentionList.test.ts
pnpm check
git diff --check
```

- [ ] **6.4 用户工作区保护证据。** 每个Git写入/观察/恢复案例前后保存所有 `refs/heads/*`、两个用户worktree的HEAD/index原bytes、dirty/untracked文件bytes、全局Git配置bytes；除fixture主动推进的main外全部一致。观察不得创建用户docs/state，revert/提案只经统一writer；不把临时验证worktree清理混同用户worktree。
- [ ] **6.5 实机与文档。** P6打包App显示违规卡片、原diff和三动作，点击后回读真实daemon事件，重开仍在；UI快层不移动节点、首屏最多3项。记录版本/平台/截图/实际接口结果；真人操作尚未发生时明确“待真人验收”，不把App HTTP测试客户端称真人。README说明v0.1事后检测及incomplete诊断；双语未发布日志一条用户可感知变化。
- [ ] **6.6 精确提交与冻结交接。** 仅暂存本任务已验证测试/文档hunk，不 git add .，不夹带其他批源码；提交 `test(observer): verify main inspection and explicit App recovery end to end`，记录真实SHA/红绿命令/限制。交Root审阅后再按既有发布总表推进P6/P7/P8；不扩M2云端、M3集成器、长期豁免、密码学认证或自动revert/approve。

## 自审映射与完成界限

| 规格/红线 | 对应任务与证据 |
| --- | --- |
| FR-17：前后layer/完整保护路径、删除/rename/config/docsdir | Task1；Task6保护矩阵 |
| FR-98/FR-103：离线init例外、首次启用边界已由用户确认 | Task1.4按整笔启用提交及祖先作为导入上下文；之后全部可达提交检查，导入不批准B0 |
| FR-103：每fetch后检查、确定性禁止Hold、三种显式处理 | Task3–5；Task6真实daemon |
| FR-87：commit+hold.opened确定性eid、多机/重扫去重 | Task3.1–3.6；删除派生物/恢复同eid |
| FR-95：App human、精确hash/对象IDs/target、TOCTOU | Task5；待点击旧subject失效、无Run提案与永久精确ClaimFact/receipt恢复 |
| FR-89/91/94/110：唯一writer、合法snapshot/private搬送、不碰用户工作树 | Task2/5/6；push后ack失败不误报 |
| FR-107：唯一Git/Evidenceproof、无证据不merged、canvas等价非slow豁免 | Task2；三入口矩阵与Task6 |
| 派生物可重建、正常操作不全历史重扫 | Task3/4；实际Git调用计数、恢复重扫 |
| 原提交/原Hold审计、无自动放行/自动撤销/第二人事实框架 | Task3/5；App显式动作与outbox恢复 |

写作自审：所有新增类型/函数在本文或明确依赖计划中定义；未来API未当作现有源码。FR103以外的未知trailer诊断仍留批3原规则，不因本计划新增免检。实施完成必须有上述真实证据和精确提交号；本文件冻结交回Root review，保持未实施、未提交。
