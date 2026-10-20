---
type: Plan
title: M1 P4 草稿、提案与基线人工放行实施计划
description: 以共享 schema、持久草稿引用、精确人工批准和唯一主干写入器闭合 S6–S8 与后续提案出版。
status: draft
tags: [towermd, M1, P4, baseline, proposal, TDD]
sources:
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-3-snapshot-recovery.md
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-4-verifier-drafts.md
  - resource: docs/superpowers/plans/2026-10-17-m1-task-evidence.md
  - resource: docs/superpowers/plans/2026-10-17-m1-human-intervention.md
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: ../note.md/docs/okf-v0.2-format-constraints.md
  - resource: AGENTS.md
  - resource: tasks/lessons.md
  - resource: docs/superpowers/plans/2026-10-17-shared-recovery-proof-contract.md
---

# M1 P4 草稿、提案与基线人工放行 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让用户现有 harness 会话提交 S6 结果，App 编辑并持久保存真实草稿，真人批准精确内容后，由唯一主干租约持有者出版 B0/后续基线与完整审计。

**Architecture:** 核心维护一个提交 schema、不可变草稿清单与批准绑定；所有非 heartbeat 事实和请求 receipt 复用 P3 的恢复 pending fsync→hub→核原Event→confirmed fsync→本次成功 持久链，daemon 调用统一 Git 写入模块保存私有草稿和出版主干。快照和慢层共享一个 writer 锁及底层 Git 提交/发布实现，人工批准不允许通过 CLI 发出；发布恢复以 Git 中的原 publication 身份与权威标签为依据。

**Tech Stack:** 现有 Rust、serde/serde_json/serde_yaml、sha2、hex、uuid、tempfile、tokio、axum、clap、Git CLI；不引入新库、无头 LLM、自由指定引用或绕过开关。

---

**共享证明前置契约：** [2026-10-17-shared-recovery-proof-contract.md](2026-10-17-shared-recovery-proof-contract.md) 是 E/P2/P3/P4 的唯一恢复证明输入、confirmed 持久顺序、历史 merge 与 publication schema 来源。以下接口均待实现；开工按实际前置 SHA 对齐，缺 producer/schema 只诊断，不用夹具或字段自报代替真实产品路径。

## 实施前事实与冻结范围

下述“实施前事实”为最初规划时的记录。实施提交、当前验证和未完成项以 `tasks/todo.md` 的 2026-10-18 记录为准；不能把原计划接口示意当作已落地 API。`ec260a1` 已交付共享 schema、纯编辑/冻结与真实扫描接线，`39496b7` 已交付同一画布的有限注释，`8fd0a24` 已交付私有草稿 materialization/reader 与精确审阅内核。Submit/Edit 统一 ledger、daemon 接线、App 有限编辑/审阅及 B0 出版仍是未提交增量，须完成各自验证后精确提交。当前基线开启草稿、完整 S7 表单、Bn/Proposal、任务复核、真人验收与正式 App 发布尚未完成；未按整 Task 勾选完成。

当前主目录的 `Catalog` 从缓存权威 main 读取 `objects/object_paths/config`，`Document` 保留未知键和原始文本，`types::TYPES` 是写入 type 唯一注册处；`CanvasDocument::decode` 保留未知/不透明条目，`from_value` 是严格候选入口。`coldstart::prepare_coldstart`、daemon `/baseline/coldstart` 仍是旧路径；当前 CLI baseline 只有 scan，没有 submit、批准或 propose。

`/private/tmp/towermd-m0-remediation-2` 正在迁移 `HubProtocol`、`NewEvent.guards`、`ProjectSession::sync/execute/tick/renew_binding/checkpoint`；字段和签名必须等该批提交后核定。M0 批 3 私有 snapshot、统一 writer、恢复，批 4 `coldstart_draft/scan_jobs/verifier` 都是尚待实施的计划，不能当作当前源码调用。

开工依赖：M0 批 2–5 完成并验证，P2 的权威任务/证据语义稳定；P3 的 AppGate、inbox 与投影恢复可复用，P6 基础 native 代理可随后接入。执行者先在 `tasks/todo.md` 记录准确前置 SHA、接口映射和本批红绿结果；前置不满足时保持未开始，不回接旧 `Hub + LeaseChange`。

1. S6 由用户已经打开的 Claude Code/Codex 会话按原生技能综合；输入 S1–S5 固定扫描结果及同一 schema，不启动模型进程，不要求新账号。
2. S7 必须有真实合并、拆分、改名、坐标拖动、接口补充、验证命令确认、已有 Markdown 引用的核心操作；候选模块、接口、引用节点都解析到真实文件/对象。
3. S1–S5 缓存可重建；S6 submit 及第一次人工编辑起立即保存 Git 草稿。FR-95 的引用为 `refs/towermd/drafts/<id>`，不是唯一状态留在 cache 的 JSON。
4. S8 前不写 main 或用户 docs_dir。草稿 ref 内可含未来 docs_dir 路径的 tree，但不检出到用户目录。已有普通 Markdown 只引用，不接管、不改写。
5. 人工事件只能来自有限 App 入口，actor 由 daemon 构造 `human:<id>`；CLI 永远 `process:towermd-cli`，没有 approve/reject/actor/human 参数。
6. 批准绑定项目、对象 ID、稳定内容 hash、目标基线、草稿 revision；审阅后替换内容或目标基线变化不能沿用批准；无guards的原合法批准不绑定临时writer租约，新holder仍须持fresh main lease并重核精确binding；实际旧guards stale批准只作历史。
7. 无 remote 且 main 在任意用户 worktree 检出：慢层/B0 保持 pending，给出可复制切离 main 指引；D1 私有 snapshot 例外只允许 state，不能在那里使 B0 生效。
8. 不 checkout/reset/stash、不修改用户 index、不移动用户分支；只在无人检出的本地 main 进行 CAS，remote 用普通非强制 push，保留批 1/3 的引用安全与有界重做。
9. 标签是编号权威；冲突拒绝或重新审阅，不能自动改成 B<n+1> 或重新批准新内容。App UI/拖拽手势由 P6 集成，本文先交付 typed 后端；最终首版验收必须真人在打包 App 走通。

## 文件责任与提交边界

| 文件 | 唯一职责 |
| --- | --- |
| `crates/towermd-core/schema/baseline-submit.schema.json` | S6 提交唯一 JSON Schema，技能和 App 读取同一份 |
| `crates/towermd-core/src/baseline_schema.rs` | 读取 schema、解释所用固定子集、反序列化 typed 提交 |
| `crates/towermd-core/src/baseline_draft.rs` | 纯草稿领域模型、编辑、对象/引用检查、冻结与 hash |
| `crates/towermd-core/src/baseline_review.rs` | 审阅 subject、批准/拒绝、publication 状态与恢复判定 |
| `crates/towermd-core/src/recovery_outbox.rs`（批 3 已有） | 复用 fsync 恢复输入；不新建 P4 唯一状态或注册对象 |
| `crates/towermd-core/src/proposals.rs` | FR-35/36 影响分析、提案与冲突任务复核计划 |
| `crates/towermd-core/src/repository_writer.rs` | 从 snapshot_git 提取共用 Git plumbing、目标保护和 ref 发布 |
| `crates/towermd-core/src/snapshot_git.rs` | 快照专有边界校验，委托 repository_writer，不保留第二套提交实现 |
| `crates/towermd-core/src/snapshot_service.rs` | 原 SnapshotWriter 扩展草稿/慢层编排，共用同一个 writing 锁 |
| `crates/towermd-core/src/{project_session,snapshot}.rs` | 基线/Proposal/Decision/需复核投影、连续应用与恢复 |
| `crates/towermd-core/src/lib.rs` | 只导出公共领域接口，Git 原语和批准凭据构造保持内部 |
| `crates/towermd/src/baseline_routes.rs` | 固定 daemon 草稿与 App 基线请求适配 |
| `crates/towermd/src/{daemon,main}.rs` | 注册服务、submit/propose 解析、P3 公共 prelude 复用 |
| `crates/towermd-core/tests/{baseline_schema,baseline_draft,baseline_review,baseline_publish,proposals}.rs` | 各闭环真实行为、Git 故障窗口和重放 |
| `crates/towermd/tests/baseline_cli.rs` | 真实二进制、daemon、App 程序边界、重启与 worktree 保护 |

实施时需要 README、双语 CHANGELOG 未发布条目、`tasks/todo.md` 结果登记，只能在对应已验证任务内精确修改和提交；本文写作阶段不改它们。P6 自有 `src-tauri`/`src` 文件不在本批抢建，P7 原生技能交付接收本批 schema 与 CLI 契约。

## 统一 typed 契约

共享 DraftObject/DraftBundle/FrozenFile/FrozenDraft、ReviewBinding/Verdict/ApprovalFact 与 publication freeze/hash 须先由 E 按共享契约第1.2节交付；baseline_draft.rs、baseline_review.rs分别重导出同一类型，本批只增加编辑/真人producer/出版编排，不反向成为E编译前置。以下类型为待实现的完整公共边界；公共 wire 类型派生 Clone/Debug/Serialize/Deserialize/PartialEq，固定请求使用 `deny_unknown_fields`，tagged enum 使用 snake_case。对象内容本身是原始 OKF 文本，未知 type/键宽容读取并原样保留；不在 TS 重写 schema、hash 或门禁规则。

```rust
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
pub type ObjectId = String;
pub type Revision = String;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineSubmit {
    pub scan_id: String, pub project_id: String, pub source_revision: Revision,
    pub intents: Vec<DraftObject>, pub modules: Vec<DraftObject>,
    pub interfaces: Vec<DraftObject>, pub acceptances: Vec<DraftObject>,
    pub questions: Vec<String>, pub risks: Vec<String>,
}
pub use crate::recovery_contract::baseline::{DraftObject, DraftBundle, FrozenFile,
    FrozenDraft, ReviewBinding, Verdict, ApprovalFact, publication_freeze,
    publication_content_hash};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerifyCommand { pub cwd: String, pub argv: Vec<String> }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitPart {
    pub id: ObjectId, pub title: String, pub paths: Vec<String>,
    pub verify: Vec<VerifyCommand>, pub acceptance_ids: Vec<ObjectId>,
    pub x: i64, pub y: i64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftEdit {
    Merge { modules: Vec<ObjectId>, keep: ObjectId, title: String },
    Split { module: ObjectId, parts: Vec<SplitPart>,
            interface_endpoints: BTreeMap<ObjectId, [ObjectId; 2]> },
    Rename { object: ObjectId, title: String },
    Move { node: String, x: i64, y: i64 },
    PutInterface { object: DraftObject, from: ObjectId, to: ObjectId },
    RemoveInterface { interface: ObjectId },
    ConfirmVerify { module: ObjectId, commands: Vec<VerifyCommand> },
    AddDocref { node: String, path: String, x: i64, y: i64 },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewView {
    pub binding: ReviewBinding, pub current_baseline: Option<String>,
    pub draft: FrozenDraft, pub diff: Vec<PathDiff>,
    pub interface_modules: Vec<ObjectId>, pub publish_blockers: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathDiff { pub path: String, pub before: Option<String>, pub after: Option<String> }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewAction {
    pub request_id: String, pub binding: ReviewBinding,
    pub verdict: Verdict, pub comment: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Publication {
    Pending { approval_eid: String, binding: ReviewBinding, reason: String },
    Committed { approval_eid: String, binding: ReviewBinding, commit: String },
    Tagged { approval_eid: String, binding: ReviewBinding, commit: String, tag: String },
    Recorded { approval_eid: String, binding: ReviewBinding, commit: String, tag: String },
}
```

`target_baseline` 是拟出版的精确标签名（首个为 `towermd/B0`）；ReviewView 同时返回当前权威标签，避免“目标”歧义。`base_directory` 为审阅时 docs_dir 中除派生 artifacts 与快照 state 外的目录内容，以及目录外 L0–L2、config 的 `(mode, blobOID)` 快照，值采用 `mode + 空格 + oid`；删除/已有路径都纳入。引用文档另以 source_revision 绑定，不复制进出版覆盖集。

公共函数固定为 `parse_submit(&serde_json::Value)->Result<BaselineSubmit,String>`、`schema()->&'static str`、`from_scan(&Catalog,&ScanInput,BaselineSubmit)->Result<DraftBundle,String>`、`from_current_baseline(&Catalog,&str)->Result<DraftBundle,String>`、`apply_edit(&Catalog,&DraftBundle,&DraftEdit)->Result<DraftBundle,String>`、`freeze(&Catalog,&DraftBundle)->Result<FrozenDraft,String>`、`review_binding(&FrozenDraft,&str)->ReviewBinding`、`validate_review(&ReviewView,&ReviewAction,&str)->Result<ApprovalFact,String>`。ScanInput 来自已合入批 4；提交只接受真实 completed 扫描及完全相同 project/source revision。

`SnapshotWriter` 增加 `save_draft(session:&Mutex<ProjectSession>,catalog:&Catalog,draft:&FrozenDraft,expected_ref:Option<&str>,request_id:&str,outbox:&mut RecoveryOutbox,host:&str,now:impl FnMut()->Result<u64,String>)->Result<Revision,String>` 和 `publish_baseline(session:&Mutex<ProjectSession>,catalog:&Catalog,approval:&ApprovalFact,outbox:&mut RecoveryOutbox,host:&str,now:impl FnMut()->Result<u64,String>)->Result<Publication,String>`。两个方法都用现有 writing 锁；private writer 方法不向调用者暴露 arbitrary ref/path。

## Task 1：单 schema、提交验证与稳定冻结

**Files:** 新建 schema、`baseline_schema.rs`、`baseline_draft.rs`、两者测试；修改 `lib.rs`。本任务只做纯转换/校验，不出版、不增加 daemon 命令。

- [ ] **Step 1：先写以下完整 schema 红测试。** 将最小输入与递归 schema 校验器作为同一模块测试，避免测试专用规则代替生产规则。

```rust
#[test]
fn submit_rejects_wrong_shape_before_reading_objects() {
    use serde_json::json;
    use towermd_core::baseline_schema::{parse_submit, schema};
    let mut value = json!({"scan_id":"scan", "project_id":"project",
        "source_revision":"1111111111111111111111111111111111111111",
        "intents":[], "modules":[], "interfaces":[], "acceptances":[],
        "questions":[], "risks":[]});
    assert!(serde_json::from_str::<serde_json::Value>(schema()).is_ok());
    assert!(parse_submit(&value).is_err());
    value["modules"] = json!([{"id":"A-api","path":"arch/A-api.md",
        "text":"---\ntype: Module\nid: A-api\n---\nAPI\n"}]);
    assert!(parse_submit(&value).is_ok());
    value["modules"][0]["path"] = json!(3);
    assert!(parse_submit(&value).unwrap_err().contains("modules[0].path"));
    value["actor"] = json!("human:owner");
    assert!(parse_submit(&value).is_err());
}
```

- [ ] **Step 2：运行红测。** `~/.cargo/bin/cargo test -p towermd-core --test baseline_schema submit_rejects_wrong_shape_before_reading_objects --locked`；先补模块/签名到可编译，再记录空验证实现接受非法 path 的行为失败，不能只记录 import 缺失。
- [ ] **Step 3：写唯一 schema。** 根 required 为 BaselineSubmit 全字段，additionalProperties=false；模块 minItems=1，其余集合允许空。`$defs.object` required id/path/text，三者 string/minLength=1；questions/risks items string；所有 envelope object additionalProperties=false。使用 Draft 2020-12、固定 `$id: towermd:baseline-submit:v1`，不在技能/TS 复制字段约束。
- [ ] 使用 `include_str!("../schema/baseline-submit.schema.json")` 暴露原文件。固定解释器只实现本 schema 实际用到的 `$ref/$defs/type/properties/required/additionalProperties/items/minItems/minLength`；遇未知 schema 关键字启动报错，不能默默忽略验证。错误携完整 JSON 路径；再 `serde_json::from_value` 得 typed 提交。
- [ ] **Step 4：实现语义转换。** `from_scan` 核对扫描 ID、项目 UUID/source SHA；保留批 4 扫描中 Evidence/候选任务的真实读取关联，不丢失 `no_tests`；Evidence继续以hub/快照为准，候选任务仍在draft，均不夹带进慢层出版覆盖集。对象 ID 唯一、正文 id/type 与集合一致，写入新对象用 `Document::new` 与 `types::lookup`；现有对象用 parse/set，未知键原样保留。
- [ ] 校验相对路径、层级和引用：规范 POSIX、不得含 `..`、绝对路径、NUL、`.git`；提交 path 是相对 docs_dir 的路径，转换一次成为仓库根相对路径；新提交对象只能在配置目录对应 intents/arch/specs，Interface 两端必须存在 Module，Acceptance 引用必须存在。不能从客户端 docs_dir 覆盖 Catalog.config；不改 project_id，FR-98 init 是唯一初始化例外。
- [ ] **Step 5：复用 E 唯一冻结摘要。** 使用共享契约第1.2节的 publication_freeze/publication_content_hash 与精确10-parts长度前缀算法，不在本模块复制 digest_parts。files 含真实慢层对象与 tower.canvas，审计 manifest/Decision 排除；保留 UTF-8 原文、CRLF、无末尾换行与未知键。P4只把已核FrozenDraft转换为共享PublicationFreeze，reader从实际commit读相同字节复算；不得各自重排questions/risks或归一化YAML。
- [ ] **Step 6：补红绿矩阵。** 输入对象 Map 顺序不同 hash 相同；正文/未知字段/坐标/验证 argv/删除路径/目录占用变化 hash 不同；CRLF 和无末尾换行保持；未知 type 文档读取可往返但不能通过 S6 冒充 Module；同名既有普通文档拒绝覆盖。
- [ ] **Step 7：绿验证与精确提交。** `~/.cargo/bin/cargo test -p towermd-core --test baseline_schema --test baseline_draft --locked`。只提交本任务源码、schema、测试及对应任务记录，提交信息 `feat(baseline): validate and freeze shared-schema drafts`，记录真实 SHA。

## Task 2：真实 S7 编辑与 Git 持久草稿

**Files:** 修改 `baseline_draft.rs`、`snapshot_git.rs`、`snapshot_service.rs`；新建 `repository_writer.rs`；扩展 `baseline_draft`、现有 `snapshot_git/snapshot_service` 测试。先完成后端编辑契约，不创建 App 模拟按钮。

- [ ] **Step 1：当前基线开启草稿与纯编辑红测。** `from_current_baseline(catalog,expected_baseline)`核验当前权威tag与main，再复制当前Intent/Module/Interface/Acceptance及canvas原文为新Bundle，target取精确下一标签；旧Decision/Baseline审计、Task/Run/Evidence不进入出版覆盖集。App请求携当前baseline与main revision，不匹配返回409；保存新draft ref后才回包。 在 `baseline_draft.rs` 单元测试构造两个真实 Module Document 与两个 file 节点；调用 `apply_edit`，断言 Merge 后保留 ID 的路径集合是排序去重并集、其余对象移除且列入 deletions，关联接口/Acceptance 和节点全部重定向。旧 Bundle 原文必须未变。
- [ ] **Step 2：实现 merge/split 原子变换。** 先 clone 后校验成功返回。Merge 至少两个不同存在的 Module，keep 必须在其中；冲突验证命令去重但保留 cwd。Split 至少两份、ID 唯一、每个 path 恰归一份且覆盖原模块范围；Acceptance 明确分配，所有关联 Interface 必须通过 interface_endpoints 明确映射，缺映射拒绝，不能猜一端。
- [ ] Split 的新 Module 从原 Document 克隆后只替换显式字段，未知键保留。候选 Task 不自动 ready/claim；既有 live Task 不迁移，只在获批准后由 Task 5 复核。Merge 内部接口如两端折叠到同一模块，拒绝并给出需先用 RemoveInterface 删除或调整的诊断，不默默丢接口。
- [ ] **Step 3：实现其余编辑。** Rename 只改 title、不隐式变 ID/path；Move 只改命名节点 x/y，必须安全整数，不触碰其他节点；PutInterface 检查 registered type/id/端点并建立真实 Interface 对象及 edge；RemoveInterface 仅删除存在的 Interface 对象与其绑定 edge、登记删除路径；ConfirmVerify 写模块 verify 的 cwd/argv，复用共用 verifier 的合法命令检查，不执行命令。
- [ ] AddDocref 只接受 source_revision 中存在的普通 `.md` blob，不读用户工作树/symlink/submodule；file 路径以仓库根为准，无 towermd 键；拒绝同 node ID。既有文档不复制为 Tower 对象。操作未知/opaque canvas 条目时拒绝该目标，其他不透明条目原样保留。
- [ ] **Step 4：完整 Git 持久化红测代码加入现有 snapshot_git.rs 测试模块。** 沿现有真实临时仓库 helper `repository/git` 建立配置；需要的 writer API 和 fixture 内容在本任务实现，不修改用户仓库。

```rust
#[test]
fn draft_reference_is_persistent_without_touching_checked_out_main() {
    use crate::repo::git;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]).unwrap();
    std::fs::write(root.join("README.md"), "keep\n").unwrap();
    git(root, &["add", "README.md"]).unwrap();
    git(root, &["-c", "user.name=Test", "-c", "user.email=t@invalid",
        "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]).unwrap();
    let before = git(root, &["rev-parse", "HEAD"]).unwrap();
    let status = git(root, &["status", "--porcelain=v1"]).unwrap();
    let files = std::collections::BTreeMap::from([
        ("docs/tower/arch/A.md".into(), "---\ntype: Module\nid: A\n---\nA\n".into())]);
    let revision = crate::repository_writer::save_draft_tree_for_test(
        root, "draft-1", &before, &files, None).unwrap();
    assert_eq!(git(root, &["rev-parse", "HEAD"]).unwrap(), before);
    assert_eq!(git(root, &["status", "--porcelain=v1"]).unwrap(), status);
    assert!(!root.join("docs/tower/arch/A.md").exists());
    assert_eq!(git(root, &["rev-parse", "refs/towermd/drafts/draft-1"]).unwrap(), revision);
    assert!(git(root, &["show", "refs/towermd/drafts/draft-1:docs/tower/arch/A.md"])
        .unwrap().contains("type: Module"));
}
```

- [ ] 定义内部 `save_draft_tree_for_test(root:&Path,id:&str,base:&str,files:&BTreeMap<String,String>,expected:Option<&str>)->Result<String,String>`，仅 `#[cfg(test)] pub(crate)`；上例放 core 源文件单元测试，不公开测试写入逃生口给集成 crate。生产方法只有 SnapshotWriter.save_draft，要求项目 session/lease 校验。
- [ ] **Step 5：运行行为红测。** `~/.cargo/bin/cargo test -p towermd-core draft_reference_is_persistent_without_touching_checked_out_main --locked`；先给内部函数返回 base 的编译桩，预期 rev-parse 草稿 ref 失败。记录后再实现真实提交。
- [ ] **Step 6：抽出共用 writer。** 把现有 snapshot_git 的安全 Session、临时 index、blob、read-tree/update-index/write-tree/commit-tree、直接 ref 检查、CAS/普通 push集中到 repository_writer；snapshot_files/highest_fence/等价检查留 snapshot_git。不能复制整份 snapshot writer 另起提交者；SnapshotWriter 的同一个 writing Mutex 包住草稿、快照、恢复、慢层。
- [ ] 内部 `WritePurpose::{Snapshot,Draft,Baseline}` 固定目标与允许路径：Snapshot 私有例外仅 `<docs_dir>/state/`；Draft 仅 `refs/towermd/drafts/<校验id>`；Baseline 仅权威 main。Git I/O 不持 session 锁，锁顺序 writer→短 session；refs 更新前再次回查适用 lease，回包前保存业务事件。
- [ ] 草稿 tree 从固定 source/main 构建，新增 `.towermd/draft-manifest.json` 仅在草稿 ref 中存 FrozenDraft 元数据；发布时明确排除它。`update-ref --no-deref ref new expected`，已有 ref 不匹配 expected 则 409；人工编辑必须带 expected revision，禁止 last-write-wins。
- [ ] 有 remote 时只推 `refs/towermd/drafts/<id>:refs/towermd/drafts/<id>`，不推 main，不 force。推送失败保留本地 durable ref并返回 `local_saved_remote_pending`；不可显示已远端送达。无 remote 本地 ref即 durable。跨 clone拉取草稿后重算 hash/项目/目录，不信任 manifest自报摘要。
- [ ] **Step 7：重建与保护验证。** 删除扫描/cache 后从草稿 ref恢复人工 rename/move/verify；测试重启、push拒绝、ref被符号化/替换、旧 expected、未知 OKF/canvas字段。保存前后比较所有 heads、全部 worktree HEAD/status/index字节、未跟踪文件；只能新增准确 draft ref/对象。
- [ ] **Step 8：全绿和提交。** `~/.cargo/bin/cargo test -p towermd-core --test baseline_draft --test snapshot_git --test snapshot_service --locked` 加源内草稿测试。提交 `feat(drafts): persist human edits through the shared repository writer`，记录 SHA，不提交尚未通过的并行 P2/P3 文件。

## Task 3：App 审阅绑定、批准拒绝与可恢复事实

**Files:** `baseline_review.rs`、`project_session.rs`、`snapshot.rs`、`baseline_routes.rs`、`daemon.rs`、`baseline_review` tests。复用 P3 AppGate，不新建认证系统，不把 HTTP 夹具当真人证据。

- [ ] **Step 1：新增可独立运行的绑定红测。** `validate_review` 只验证不可变 view/action/actor，提交前 fresh-fetch 重验由 Task 4 执行。

```rust
#[test]
fn approval_never_follows_replaced_content_or_another_target() {
    use towermd_core::baseline_review::{matches_binding, ReviewBinding};
    let viewed = ReviewBinding { project_id: "p".into(), object_id: "D-1".into(),
        content_hash: "a".repeat(64), target_baseline: "towermd/B0".into(),
        draft_revision: "1".repeat(40) };
    assert!(matches_binding(&viewed, &viewed));
    let mut changed = viewed.clone();
    changed.content_hash = "b".repeat(64);
    assert!(!matches_binding(&viewed, &changed));
    changed = viewed.clone(); changed.target_baseline = "towermd/B1".into();
    assert!(!matches_binding(&viewed, &changed));
    changed = viewed.clone(); changed.draft_revision = "2".repeat(40);
    assert!(!matches_binding(&viewed, &changed));
}
```

- [ ] `matches_binding(&ReviewBinding,&ReviewBinding)->bool` 在 baseline_review 定义，必须完整值相等，不能只比较 hash。运行 `~/.cargo/bin/cargo test -p towermd-core --test baseline_review --locked`；编译桩返回 true 后必须看到替换内容断言失败。
- [ ] **Step 2：生成真实 ReviewView。** daemon从已存草稿 ref读取并重算 FrozenDraft，展示所有 path before/after、删除、验证命令、风险/问题、当前与目标基线、hash及draft_revision；不可只显示摘要标题。接口变更返回两端 Module IDs供P6同时高亮，引用节点返回真实 source文档。
- [ ] **Step 3：固定 App wire。** `GET /app/baselines/{id}/review?directory=…` 返回 ReviewView；`POST /app/baselines/{id}/action` body为 `{directory, human_id, action:ReviewAction}`，AppGate通过后server构造actor；通用 `/command`、`/drafts`、CLI客户端无此能力。human_id规则复用P3，body不能携自由actor。
- [ ] 固定 `request_id` UUIDv7：先核P3统一request ID/body_hash冲突；同ID+完全相同body_hash在查/返成功receipt前协调原pending/真实Event的confirmed（持续失败仍uncertain），耐久后返回原响应及当前validity，不同payload冲突；不能用当前ApprovalFact替代原请求receipt。第一次批准或拒绝都是终态事实；不能后续“重试”把拒绝变批准。修改后产生新草稿revision、新审阅和request ID；旧拒绝留审计。
- [ ] **Step 4：追加 typed 事实。** 核心 `ProjectSession::review_baseline(catalog,&ReviewView,&ReviewAction,actor,&mut RecoveryOutbox,now)->Result<ApprovalFact,String>`先核原请求ID/body_hash冲突，并在查询/返回统一成功receipt前协调原pending/真实Event的confirmed，持续fsync失败仍uncertain；再sync核验项目、现ref、当前target与human actor。规划 `baseline.approved/baseline.rejected` 的完整NewEvent后，复用P3实际 `append_p3_event`：先 `RecoveryOutbox::append_pending(&event)` 完成JSONL/fsync，再HubProtocol.append→全字段/全guards核对真实完整Event→core内部persist_confirmed fsync后才返回本次mutation/幂等success；Hub已耐久接受可正常sync/广播/快照，p3_receipt保原结果。data含完整ApprovalFact及下述request metadata，actor human；批准不是baseline.created，不立即生效。
- [ ] 原data/请求/meta/typedreceipt严格采用共享第1.3节BaselineReviewRecorded，`ApprovalFact.approval_eid=原Event.eid=request_event_eid(request_id,kind)`（UUIDv7原文本先SHA256，再derived_eid），事件obj与binding.object_id一致；baseline人工批准绑定只使用ReviewBinding，不额外绑定临时main writer fence；正常合法人工批准事件guards为空。任何原事件实际携带的guards都原样保留，不能为恢复剥除或重签；原eid/host/actor/完整guards与业务data一并进入outbox。沿P3的 `data.p3_request={request_id,body_hash}` 和最终 `data.p3_receipt={request_id,body_hash,result_ref}`，同时在 `data.request_body` 保存完整typed原请求与显式actor/host（目录按P3解析为project_id）；不只存hash/结果，不新建P4 receipt对象/索引。body_hash复用P3递归键排序的typed原始请求算法：去instance/request_id、目录解析project_id、保留human actor/host/显式run与完整action/binding/comment。恢复保留原始请求与结果，不能从当前ApprovalFact或snapshot只保最终状态。
- [ ] 所有P4非heartbeat事实都显式通过上述已接线入口，包括草稿保存/Proposal请求、批准/拒绝、baseline.created、需复核/解除及其独立receipt。初始申请、同内容复用、重复click等没有新业务变化的新请求，也须按P3已有requested归档事件写独立receipt后返回。pending fsync失败禁止hub append；confirmed fsync失败、hub或回包不确定返回原request_id uncertain，重试只协调outbox中的原NewEvent，不改eid/guards/body_hash。
- [ ] StateProjection保存有效事实；P3完整已校验归档+连续tail重建统一receipt和人工历史视图。恢复后实际带旧guards且stale的事件仍归档可读，不能恢复有效批准或重获租约；未定序outbox pending只作uncertain协调输入。无guards的合法baseline人工批准，若恢复后原ReviewBinding仍精确有效，可由新holder持fresh有效main lease出版，不无故要求人重批。stale/失效绑定的原请求返回旧结果时另报 `validity: historical_only`，不autoapprove；需要新人工批准时由App产生新的明确请求。
- [ ] **Step 5：拒绝和崩溃窗口。** 拒绝不删草稿、不写main、不创建tag/Decision；批准已outbox fsync/append但未checkpoint、回包前崩溃，同request重试先补原Event的confirmed、持续失败仍uncertain，满足耐久后返回原receipt且不重规划批准；Hub已耐久批准/receipt照常sync/广播/快照，原result不改失败。过期/重建后原event保留审计，但实际带旧guards且stale的批准只进入历史视图；新lease不自动把它变有效。无guards合法批准恢复后只重核原ReviewBinding，仍有效则可由fresh main lease holder出版；绑定变化才由App新批准，不能凭旧fence发布。等待/轮询超时保持pending/rejected，不自动批准。
- [ ] **Step 6：自动边界验收。** CLI JSON注入human/action、缺App标记、另项目、旧revision/hash/target、重复不同payload均4xx且hub无批准；合法Appgate才有human事实；snapshot后删除派生session重新restore事实相同。
- [ ] 新增真实daemon测试 `baseline_approval_survives_hub_loss_before_checkpoint_as_history`：App批准后无remote main检出使publication pending；断言snapshot未覆盖批准seq且main未写，outbox含原ApprovalFact/eid/guards/完整request/body_hash/receipt。停夹具daemon，仅删hub，保留Git/outbox，经CLI rebuild请求和真实App确认路由恢复；历史视图有唯一原批准、同请求原响应可回读。分两组验证：正常无guards批准且binding未变，fresh holder可以原批准继续出版；真实带旧guards事件恢复后仍stale且原请求不能使B0生效。另将draft/hash/target改变，旧批准只历史可读，必须新App审阅新binding；不能靠新lease或receipt自动迁移权限。Run：`~/.cargo/bin/cargo test -p towermd --test baseline_cli baseline_approval_survives_hub_loss_before_checkpoint_as_history --locked -- --exact`。
- [ ] 另测pending fsync失败无hub事件、confirmed失败后reopen/status仍见真实批准而原请求uncertain、持续同ID失败不得success/解除仅补原Event、archiveDuplicate协调、hub同eid异body拒绝、Duplicate无完整原Event不能确认、append失败uncertain沿原eid/guards重试、拒绝请求及复用receipt经压缩/重启仍可查；异body校验先于当前租约活动校验。
- [ ] **Step 7：绿提交。** `~/.cargo/bin/cargo test -p towermd-core --test baseline_review --test snapshot --locked` 与 `~/.cargo/bin/cargo test -p towermd baseline_review_ --locked`；提交 `feat(baseline): bind App review facts to immutable draft content`，记录 SHA。真人验收仍未完成。

## Task 4：唯一 writer 原子慢层提交与 tag/ack 恢复

**Files:** `repository_writer.rs`、`snapshot_service.rs`、`baseline_review.rs`、`project_session.rs`、`snapshot.rs`、`baseline_publish` tests。任务必须在一个可验证闭环内包含成功、pending、崩溃恢复；不能先交付会重复基线的半成品。

- [ ] **Step 1：先写测试矩阵与状态判定红测。** core tests使用真实 Git临时bare remote、clone及两个worktree；故障注入仅 `#[cfg(test)]` 内部组合点：`BeforePublish/AfterPublish/BeforeTag/AfterTag/BeforeRecorded`，不提供env或HTTP参数。

```rust
#[test]
fn published_commit_without_tag_is_resumed_as_same_publication() {
    use towermd_core::baseline_review::{publication_next, PublicationNext, PublicationProof};
    let proof = PublicationProof { approval_eid: "a".into(),
        commit: Some("1".repeat(40)), commit_on_main: true,
        tag: None, tag_commit: None, recorded: false };
    assert_eq!(publication_next(&proof).unwrap(),
        PublicationNext::CreateTag { commit: "1".repeat(40) });
    let tagged = PublicationProof { tag: Some("towermd/B0".into()),
        tag_commit: Some("1".repeat(40)), ..proof.clone() };
    assert_eq!(publication_next(&tagged).unwrap(), PublicationNext::RecordCreated);
    let wrong = PublicationProof { tag_commit: Some("2".repeat(40)), ..tagged };
    assert!(publication_next(&wrong).is_err());
}
```

- [ ] 实现 `PublicationProof {approval_eid:String,commit:Option<String>,commit_on_main:bool,tag:Option<String>,tag_commit:Option<String>,recorded:bool}` 与 `PublicationNext::{BuildCommit,PublishCommit{commit},CreateTag{commit},RecordCreated,Done}`，两者Clone/Debug/PartialEq。该结构仅是内部编排候选，不是可信证明；bool或公开字段不能转换为VerifiedPublication。recorded无匹配tag拒绝；最终认证仅调用共享prove_publication与baseline_created_event，缺tag保持pending。运行 `~/.cargo/bin/cargo test -p towermd-core --test baseline_publish --locked`，编译后使错误重建commit策略红测失败。
- [ ] **Step 2：准备 publication。** ID固定为approval_eid；先在writer锁内sync、取得主干lease及epoch，再短锁取ApprovalFact；拒绝事实或非human拒绝。读取ref重算hash，与ReviewBinding完整对比；fresh-fetch权威main和全部towermd/B标签，再比较target及base_directory。
- [ ] 权威标签必须为直接tag/ref、解析到commit，commit在当前权威main祖先链，manifest编号一致，且通过共享VerifiedPublication链校验；bare可达标签不算批准基线。禁止符号引用、悬空/非commit、同编号多义、缺中间编号。无B标签才能B0；已有Bn只允许人审阅的Bn+1；不能从“最大文件名”或cache分配编号。
- [ ] 审阅后的docs_dir/config/目录外慢层任一变化都要求重新审阅；即使payload hash相同也拒绝。代码main推进而冻结目录和当前基线均不变可在最新parent安全叠加原bytes；不得重跑S6/编辑或修改目标自动重基。第二次重验同样规则。
- [ ] **Step 3：先取得 writer 准备回执，再形成一次提交。** 本计划原无已接受writer准备事实；prepared/created的持久dataDTO、obj/eid/request_id/typedresult严格采用共享第1.3节，不以未来P3接口或Value占位；按共享契约第4节新增内部 baseline.publication_prepared，固定原binding/hash/authority/main fence/host，pending fsync→hub.append→核原非stale Event→confirmed fsync后才构建candidate；manifest嵌原完整Event。只由现有writer内部producer生成，无新HTTP/CLI/API或注册type。uncertain沿原attempt eid/guards协调；authority/fence改变重核并新准备，原approval/publication ID不变；已发布candidate只恢复原链。 从fresh main tree临时index只叠加已冻结且审阅过的慢层对象/删除/tower.canvas与提案审计对象，生成Decision和Baseline manifest，精确字段/路径/原批准Event/冻结文件与删除schema统一采用共享契约第4节；沿registered Decision/Baseline，补齐原ApprovalFact/ReviewBinding/原非stale完整批准回执与request metadata，不能只存human文字或hash。审计文件排除原payload hash，actual commit不写回manifest制造自引用。保留其他main文件，拒绝覆盖普通文档/符号链接/用户已有未登记对象。
- [ ] commit message记录 `Towermd-Baseline`、`Towermd-Approval`、`Towermd-Publication`、`Towermd-Content-Hash`、`Towermd-Epoch`；正文/Decision/manifest相互核对。快照state不夹进这次慢层提交；snapshot另行走同一writer。未批准的草稿manifest不写main。
- [ ] Decision和manifest的generated时间取已归档批准事件时间，ID由publication确定，禁止重试用当前时间改内容。候选commit通过CAS保留在daemon保留命名的 `refs/towermd/drafts/publication-<sha256(approval_eid)>`，再publish；该编码只处理原 `git:…` eid 中 Git ref 不允许的冒号，manifest/Decision/trailers 的 publication_id/approval_eid 保持原值。用户草稿ID禁止该前缀，不更新已批准的原draft ref/revision。重启可从该候选ref/main trailer重获同commit，不能丢失“已构建但未发布”状态；候选ref绝不使B0生效；Catalog/current-baseline reader明确排除所有draft/publication refs，只读权威main及合法基线tag。
- [ ] **Step 4：提交前二次TOCTOU重验。** 在临时tree完成后、任何main/ref/tag更新之前，再核验lease holder+epoch+expiry、draft revision/hash、main目录快照、target标签、已存在publication。失效立即停止；任何校验不能只在HTTP入口做一次。
- [ ] 无remote检查所有 `git worktree list --porcelain -z`，main被任何路径检出返回Pending，reason含精确路径和可复制 `git -C '<path>' switch -c <用户自选新分支>` 指引；shell单引号正确转义，说明由用户执行。绝不自动执行指引，不写refs/towermd/snapshot代替B0。
- [ ] 无remote且main无人检出只用no-deref CAS old→candidate；失败fresh读取再重验，最多沿批3有界次数，不能force。remote用已核验raw push URL普通push candidate:refs/heads/main；拒绝push不改本地heads/index，不把拒绝当成功，非FF重试重验scope/target/lease。
- [ ] **Step 5：tag并补审计。** main发布成功后校验candidate确在权威main祖先链及payload/Decision，持有效lease创建精确 `towermd/Bn` 指向candidate，remote推准确tag。已存在同tag同commit幂等；不同commit硬冲突，保留原tag，不分配下一编号。
- [ ] tag成功才追加 `baseline.created`（process:towermd-baseline，data严格为共享第1.3节BaselineCreated，扁平结果键含publication_id/approval_eid/content_hash/target/commit/tag/tags及同一typed请求/receipt）；基线读取只认合法权威tag+manifest，不认“Agent说已发布”。正常写入与批3 E2恢复观察必须调用同一唯一helper：`crate::events::derived_eid(actual_publication_commit,"baseline.created")`；publication_id/approval_eid放data作为重试身份，不以它们推导baseline eid，不修改FR-87体系。该Git事实的request_id取由同实际commit固定推导的内部请求ID，request_body/typedreceipt按共享BaselineCreatedResult固定记录该commit上的publication_id/approval_eid/content_hash/target/commit/tag/有序tags；normal与recovery都从原commit审计元数据构建，不代入当前恢复host/lease/时间或App调用ID，确保同eid的完整NewEvent与data/receipt一致。事件append/回包失败沿该原eid走outbox/receipt持久重试（原请求先协调confirmed再返success，恢复replay不写第二confirmed），不能生成第二Decision/提交/基线。baseline事件不冒充snapshot_ack。
- [ ] 复用 E 已交付的私有字段 VerifiedPublication、prove_publication 与 baseline_created_event，唯一签名见共享契约第5节；不得在baseline_review另定义可信Proof；唯一builder沿共享契约第4–5节，同时核实际main可达publicationcommit、tagpeel、精确Decision/manifest/trailers、原合法App producer的非staleApprovalFact/ReviewBinding完整回执、全部object/path/delete/hash/target/draftrevision及manifest内原非stale publication_prepared回执/main fence/host/实际parent。normal/E2先经该builder再构造事件；缺schema/回执或仅human署名拒绝，不能由调用者直接填proof。host取manifest的publication_host、actor固定process:towermd-baseline、guards为空；不是新注册对象或客户端输入。proof只证明已在main且有合法tag的既成事实，不能据此批准未发布草稿。
- [ ] **Step 6：恢复各窗口。** main已publish/tagfail或crash：先从main trailer+Decision+manifest验证原publication，补原tag，再补同eid审计；不得再写main或重复B0。tag成功/event未记：只补事件。publication还在本地但push结果未知：先fetch查main祖先链，不盲push新commit。
- [ ] 主干lease换人后，尚未发布的正常无guards批准按原ReviewBinding重核，仍有效可继续；若原事件实际带guards且stale只保历史，不凭新leaseautoapprove，需新确认时交App。已经main发布的原Decision/publication核验通过后，新holder仅用当前lease补原tag/同eid审计，不能扩大或重做已批准内容；原Decision与publication ID不变，不把旧epoch续活。若原commit已在main且后续代码推进，tag仍指原commit；若后来已有冲突目标tag则停止并诊断，不能覆盖。存在已publish待tag publication时阻止新慢层出版，先恢复/报冲突。
- [ ] **Step 7：Git证据自举。** main commit已包含human Decision与批准binding；hub事件尚未ack或hub丢失时，批3唯一恢复证明器扩展读取该链，核验原ApprovalFact并调用同一 `derived_eid(actual_publication_commit,"baseline.created")` 补baseline.created；normal/recovery构造data复用同一helper，不产生同eid异payload。不能仅因tag存在就造human批准，也不能从未验证trailer跳过Decision/hash检查。
- [ ] **Step 8：完整红绿矩阵。** 覆盖无remote两worktree(main在第二个检出)、remote推拒、非FF、内容替换、目录文件新增/删除、target变化、lease过期/epoch变、tag创建失败/tagpush拒、main后crash/tag后crash/eventack失败；每次比较原heads/index/各worktree文件，且重复调用只一个慢层commit/Decision/tag/eid。
- [ ] 新增 `normal_baseline_event_and_recovery_share_commit_eid`：正常baseline.created已经存在，再按批3E2从同实际commit/tag/Decision恢复，eid与payload逐值相同，事件数量、baseline数量与task.recheck_required标记数量均不增加；tagfail重试/重启同样不重出baseline或复核。Run：`~/.cargo/bin/cargo test -p towermd-core --test baseline_publish normal_baseline_event_and_recovery_share_commit_eid --locked -- --exact`。
- [ ] 同时验证私有snapshot仍只state、main/private比较仍走批3同一读取实现，快照与baseline并发互斥且不会死锁；不允许慢层发布扩大Snapshot私有路径白名单。
- [ ] **Step 9：绿与精确提交。** `~/.cargo/bin/cargo test -p towermd-core --test baseline_publish --test snapshot_git --test snapshot_service --test snapshot --locked`，提交 `feat(baseline): publish approved baselines with idempotent tag recovery`，记录SHA、每个故障窗口证据。

## Task 5：propose、越界转提案与冲突任务需复核

**Files:** `proposals.rs`、`baseline_review.rs`、`project_session.rs`、`snapshot.rs`、`tasks.rs`；`proposals` tests；复用P3 inbox与P2越界判定，不实现第二套路径守卫。

- [ ] **Step 1：固定提案契约并写红测。** Proposal使用已有registered type `Proposal`；理由/备选/建议缺失拒绝，影响路径按P2规范路径判定，接口必须列两端模块。路径/对象变化从FrozenDraft对权威baseline真实diff计算，再核验用户声明覆盖真实影响，不能只信Agent影响清单。

```rust
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub enum ProposalSource {
    Draft { id: String, revision: String },
    Branch { revision: String, base_revision: String },
}
pub struct ProposalInput {
    pub source: ProposalSource, pub reason: String,
    pub affected_intents: Vec<String>, pub affected_modules: Vec<String>,
    pub affected_interfaces: Vec<String>, pub affected_tasks: Vec<String>,
    pub affected_paths: Vec<String>, pub alternatives: Vec<Alternative>,
    pub recommendation: String,
}
pub struct Alternative { pub name: String, pub consequences: String }
pub struct RecheckFact {
    pub task: String, pub old_baseline: String, pub new_baseline: String,
    pub publication_id: String, pub reasons: Vec<String>,
}
pub fn validate_proposal(input: &ProposalInput) -> Result<(), String>;
```

```rust
#[test]
fn proposal_requires_a_reason_alternative_and_recommendation() {
    use towermd_core::proposals::{Alternative, ProposalInput, ProposalSource, validate_proposal};
    let mut p = ProposalInput { source: ProposalSource::Draft {
        id:"D".into(), revision:"1".repeat(40) },
        reason:"remove shared coupling".into(), affected_intents:vec![],
        affected_modules:vec!["A".into()], affected_interfaces:vec![],
        affected_tasks:vec![], affected_paths:vec!["src/api/**".into()],
        alternatives:vec![Alternative {name:"keep".into(), consequences:"coupling stays".into()}],
        recommendation:"split the interface".into() };
    assert!(validate_proposal(&p).is_ok());
    p.reason.clear(); assert!(validate_proposal(&p).is_err());
    p.reason = "reason".into(); p.alternatives.clear();
    assert!(validate_proposal(&p).is_err());
}
```

- [ ] **Step 2：运行行为红测。** `~/.cargo/bin/cargo test -p towermd-core --test proposals proposal_requires_a_reason_alternative_and_recommendation --locked`；桩放行时应理由缺失断言失败。ProposalSource/ProposalInput/Alternative/RecheckFact派生serde/Clone/Debug/PartialEq，拒绝未知请求键，不拒绝已有OKF未知键。
- [ ] **Step 3：精确来源与提案持久化。** daemon先核验run/全部fences；Draft来源只读已保存的精确ref revision。Branch来源以完整revision/base_revision读取Git tree，用P2同一guard取精确受保护diff，base必须是声明的权威baseline/main祖先；重复ID、未知层级、符号链接或无法判定的对象拒绝，未知OKF键保留。仅把慢层对象/受保护canvas差异物化为草稿，不带入分支无关代码、Task状态、快照或配置改动。
- [ ] `SnapshotWriter::save_proposal(session:&Mutex<ProjectSession>,catalog:&Catalog,input:&ProposalInput,run:&str,request_id:&str,outbox:&mut RecoveryOutbox,host:&str,now:impl FnMut()->Result<u64,String>)->Result<String,String>`取得同writer锁，短session锁核验绑定后释放，读取冻结来源并构造registered Proposal对象到 `proposals/<id>.md`；字段含完整理由/影响/备选/建议和源revision。freeze/save为新draft revision后，短锁调用 `ProjectSession::record_proposal(catalog,host,proposal_id,draft_revision,run,request_id,&mut RecoveryOutbox,now)->Result<String,String>`经统一outbox/receipt入口append `proposal.created`，actor process:towermd-cli、完整guards。禁止持session锁调用writer。Proposal入FrozenDraft.files，同一hash算法覆盖提案语义；save后append前崩溃用确定性ID恢复。提案不改main、不建tag、不产生ApprovalFact。
- [ ] App审阅Proposal复用Task3同一ReviewBinding和action，object_id为Proposal ID，hash覆盖提案理由/备选/建议以及FrozenDraft全部内容；不能只批准正文后换draft。批准后Task4同一publish_baseline产生新baseline与Decision，拒绝留原基线。
- [ ] **Step 4：越界改动只成为提案输入。** P2 check/finish发现慢层/越界仍硬阻塞；返回结构化 `proposal_required` 及真实diff/路径，Agent通过propose提交理由与备选。禁止将自动转提案等同批准、偷偷合并原分支、把Task标done；P5事后“转提案”复用此服务。
- [ ] **Step 5：冲突任务算法。** 新baseline tag合法且baseline.created应用后，比较旧/新Module.paths、Interface两端、Acceptance与verify；对ready/running Task的module/touches/allowed_paths及验收依赖取交集，确定reasons、生成RecheckFact。纯布局无语义变化不标任务；未受影响模块不误标。
- [ ] `needs_recheck` 是独立字段/事实，不改Evidence为通过、不改Task state为done、不释放或复活run epoch；ready任务在P2 G0前读取此事实硬阻，running下一check/finish同样阻。解除只走下述独立typed App task recheck，不依赖需run/fences的P3 G1/G4；本批不做M3自动规划。
- [ ] `task.recheck_required` eid由publication+task推导，append含完整RecheckFact且guards为空（只标记冲突，不授予run权限）；应用/restore同一逻辑，重复恢复不重复标记。只有出版成功后发事实，pending/tagfail不提前把新基线算生效。
- [ ] **Step 6：独立 App 任务复核解除契约。** 在 `proposals.rs` 定义以下类型，派生Clone/Debug/Serialize/Deserialize/PartialEq，固定请求deny_unknown_fields。publication_ids必须排序去重；binding覆盖该Task当前全部待解除publication标记，不让客户端遗漏冲突。

```rust
pub struct RecheckRunBinding {
    pub run: String, pub fences: Vec<crate::hub_protocol::Fence>,
}
pub struct TaskRecheckBinding {
    pub task_id: String, pub target_baseline: String, pub task_content_hash: String,
    pub publication_ids: Vec<String>, pub running: Option<RecheckRunBinding>,
}
pub struct TaskRecheckView {
    pub binding: TaskRecheckBinding, pub task_text: String, pub reasons: Vec<RecheckFact>,
}
pub struct TaskRecheckAction {
    pub request_id: String, pub binding: TaskRecheckBinding, pub comment: String,
}
pub struct TaskRecheckResolution {
    pub request_id: String, pub binding: TaskRecheckBinding,
    pub actor: String, pub resolved_eid: String,
}
pub fn unresolved_publications(
    pending: &std::collections::BTreeSet<String>,
    resolved: &[String],
) -> std::collections::BTreeSet<String> {
    let resolved: std::collections::BTreeSet<_> = resolved.iter().cloned().collect();
    pending.difference(&resolved).cloned().collect()
}
```

- [ ] GET `/app/tasks/{id}/recheck`返回TaskRecheckView（当前binding及Task完整内容/冲突理由）；POST `/app/tasks/{id}/recheck/resolve` body `{directory,human_id,action:TaskRecheckAction}`只接受AppGate。`ProjectSession::resolve_task_recheck(catalog,actor,&TaskRecheckAction,&mut RecoveryOutbox,now)->Result<TaskRecheckResolution,String>`复用P3统一请求receipt和持久入口；CLI没有resolve/approve命令，不提供忽略needs_recheck开关，不新增注册对象。
- [ ] task_content_hash由核心对Task当前内容（id/module/touches/allowed_paths/验收依赖/原文与未知扩展键）用Task1同稳定字节摘要构造；Run/lease与派生needs_recheck集合不混入内容hash，单独绑定。读取和点击间Task内容、目标baseline或publication集合变化均409，不自动补入新内容；显示最新subject后由人重新审阅。
- [ ] ready且无live run时running=None，App先复核Task本身即可解除，绝不要求先claim取得G1/G4。running任务必须绑定当前run及排序后全部live fences；run重领、任一epoch/holder/expiry变化均拒绝，禁止给旧run重获权限。Task状态在请求间改变也拒绝旧binding，不能把ready批准迁给新running任务。
- [ ] 通过后规划 `task.recheck_resolved`，actor human，data保存完整TaskRecheckResolution/request metadata/receipt；ready事件guards为空，running事件guards为原run全部fences。先pending fsync→append→核原Event→confirmed fsync→本次成功回包；Hub已耐久接受照常sync/广播/快照；effective projection只按该binding.publication_ids差集清精确标记，不清后来新publication、G0/G2/G3/G5失败或P3 G1/G4需求，不改Evidence/完成状态，不启动harness。
- [ ] 重复同request ID/hash返回原resolution及current validity，无第二解除；新请求复用已解除事实仍写独立receipt。恢复保留resolved原eid/guards，ready合法无guards事实按内容/baseline/标记匹配重放；running旧guards stale事实只历史可读，不清当前run的标记。新的publication标记即使晚到也保留，不能用task级布尔值清空全部。
- [ ] **解除红测代码。** 在proposals.rs测试精确差集；先用错误实现返回空集合，记录下面新标记断言失败，再实现上述函数。Run：`~/.cargo/bin/cargo test -p towermd-core --test proposals task_recheck_clears_only_reviewed_publications --locked -- --exact`。

```rust
#[test]
fn task_recheck_clears_only_reviewed_publications() {
    use std::collections::BTreeSet;
    use towermd_core::proposals::unresolved_publications;
    let pending = BTreeSet::from(["publication-old".into(), "publication-new".into()]);
    let once = unresolved_publications(&pending, &["publication-old".into()]);
    assert_eq!(once, BTreeSet::from(["publication-new".into()]));
    assert_eq!(unresolved_publications(&once, &["publication-old".into()]), once);
}
```

- [ ] **真实解除矩阵。** `task_recheck_ready_can_resolve_before_claim`：真实ready Task带需复核标记，CLI claim被阻；GET subject→App明确resolve→仅原publication标记消失→CLI claim成功，不伪造预先run。`task_recheck_running_never_crosses_fences`：活动Task冲突后check/finish阻，持匹配run/fences App复核才清；重领/次模块epoch变/过期拒绝旧click。Run：`~/.cargo/bin/cargo test -p towermd --test baseline_cli task_recheck_ --locked`，两个测试名均被task_recheck_过滤命中。
- [ ] 测同request重复点击、异body409、Task内容改/targetbaseline变/newpublication出现、ready→running竞态、fsync/append uncertain；恢复回放同resolution不重清标记，无guardsready事实仍有效，旧guardsrunning仅历史。resolved成功未checkpoint丢hub后经真实App恢复仍有原审计/receipt且不越新fence。P6显示明确“确认此任务适用于当前基线”，不能用G1/G4评审按钮替代。
- [ ] **Step 7：消息送达。** RecheckFact转换为P3同一inbox Message，精确run/task/old/newbaseline与reasons；Agent下一次任意CLI调用最前输出并flush后ack，失败不ack。不另建一套stdout消费；CLI送达只process，不产生人工复核批准。
- [ ] **Step 8：绿矩阵及提交。** proposal缺影响/接口一端/备选拒绝；真实diff遗漏路径拒绝；Approve B1后冲突ready/running标记且CLI可达、分别可经独立App task recheck解除；无关任务/纯坐标不标；重复事件/快照重建同结果。运行 `~/.cargo/bin/cargo test -p towermd-core --test proposals --test tasks --test snapshot --locked` 与P3既有delivery测试；提交 `feat(proposals): publish reviewed changes and flag conflicting tasks`，记录SHA。

## Task 6：真实 daemon/CLI 闭环、schema交付与P6交接

**Files:** `baseline_routes.rs`、`daemon.rs`、`main.rs`、`baseline_cli.rs`；README、双语CHANGELOG、tasks/todo实施结果。不在本任务实现P6完整UI/P7技能安装包。

- [ ] **Step 1：固定程序入口。** CLI `baseline submit <json>`读取该本地文件bytes并发送daemon，`baseline schema`只打印Task1同schema供已有harness技能取用；`propose <json>`读取ProposalInput。路径只是CLI读取输入，daemon不接受用户选择输出目录/ref。沿P3公共prelude，项目CLI首先送达inbox并续租。
- [ ] daemon `POST /baseline/submit` body `{directory,run,request_id,input:BaselineSubmit}`；`POST /drafts` 使用tagged body：`{action:"create",directory,request_id,baseline,main_revision}`从当前基线开启、或 `{action:"edit",directory,request_id,id,expected_revision,edit:DraftEdit}`人工编辑；两者仅AppGate，`GET /drafts/{id}`读durable ref优先，S1–S5未编辑缓存仍按批4派生读取。`GET /baseline/schema`返回同JSON Schema文件与版本。
- [ ] daemon `POST /proposals` body `{directory,run,request_id,input:ProposalInput}`无human字段；`POST /proposals/{id}/approve|reject`沿Task3有限AppGate并接受同ReviewAction，path verdict与body必须一致。与baseline action共用core校验，不增加第二批准实现。
- [ ] S6提交来自真实completed扫描，保留ScanInput及Evidence，模块verify需在S7确认；空verify仍no_tests且P2 G3阻塞，不能submit后自动通过初始验证。未经确认的argv不执行，不引入 `baseline verify` 或 `--yes`。
- [ ] **Step 2：真实CLI参数拒绝红测。** 在新baseline_cli.rs直接运行二进制help/非法子命令，不依赖网络或App；测试内容如下。

```rust
#[test]
fn cli_exposes_submission_but_never_human_approval() {
    use std::process::Command;
    let bin = env!("CARGO_BIN_EXE_towermd");
    let help = Command::new(bin).args(["baseline", "--help"]).output().unwrap();
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("submit")); assert!(text.contains("schema"));
    for args in [vec!["baseline","approve","D"], vec!["baseline","reject","D"],
        vec!["baseline","verify"], vec!["propose","--actor","human:owner"]] {
        let output = Command::new(bin).args(args).output().unwrap();
        assert!(!output.status.success());
    }
}
```

- [ ] **Step 3：运行红测再实现解析。** `~/.cargo/bin/cargo test -p towermd --test baseline_cli cli_exposes_submission_but_never_human_approval --locked`；预期help无submit/schema。完成clap参数与固定HTTP映射后测试通过，schema stdout可用 `serde_json::from_slice`解析且与core.schema逐值相等。
- [ ] **Step 4：复用现有coldstart_cli真实Fixture。** 仅把必要隔离daemon启动/停止、git、write、success/output helpers提取到 `crates/towermd/tests/common/mod.rs`，不改现有Fixture语义。每个测试自己tempdir/runtime，使用CARGO_BIN_EXE_towermd；stop只清理本实例，不停用户daemon。
- [ ] 首个完整链：init结果由测试显式commit main→baseline scan/wait→schema读取→已有会话等价提交JSON→GET draft验证真实对象→App程序入口Rename/Move/ConfirmVerify→删除cache并restart→ReviewView→拒绝→main无变化；再编辑形成新revision后新review批准→main检出时pending→测试显式切离main→重试同批准→B0 tag/Decision/manifest/canvas一次生成。
- [ ] 第二完整链：App从权威B0开启新草稿→修改Interface→propose→ReviewView两端模块→App批准→B1→冲突Task需复核→下一CLI首行送达且再次不重放；CLI注入human、普通问答“批准”、Agent输出done均不能产生基线/证据完成。
- [ ] 另测CLI Branch提案来源：分支同时含受保护Module改动和普通代码/Task状态改动，ReviewView只含物化慢层差异，批准后保留原main代码/Task状态。remote链使用temp bare origin与receiving clone，允许origin main不在工作树；验证正常draft push、main/tag发布，再注入pre-receive拒绝main/tag各自失败。记录远端实际refs，与本地heads/index区别；不能用仅本地refs存在冒充远端成功。
- [ ] 每条写路径检查root/main/关联worktree中的staged/unstaged/untracked字节、index SHA256与heads前后；唯一预期差异精确到draft ref、无人检出main CAS或remote main/tag。已有README/用户普通md逐字节保持。没有全局Git/harness配置改动。
- [ ] **Step 5：并发与崩溃回归。** 两App编辑同revision仅一成功；两holder/两批准争同B0只有一出版，另者目标变化回审；writer与snapshot并行不交叉提交。真实daemon kill在main发布后重启发现原publication，补原tag/event；测试专用内部fault不成为生产HTTP开关。
- [ ] 断网读取已有draft仍可用但不能声称远端publish；失效lease不能写任何main/tag；符号remote main/ref、重定向Git环境、恶意路径/argv与文件内容替换都拒绝且index不变。无remote main在第二worktree检出回pending，绝不checkout/reset或私有B0生效。
- [ ] **Step 6：P6 typed交接。** native固定命令为 `load_baseline_schema(token)`、`load_draft(token,id)`、`create_baseline_draft(token,baseline,main_revision)`、`edit_draft(token,id,revision,edit)`、`review_baseline(token,id)`、`act_on_baseline(token,id,action)`、`load_task_recheck(token,id)`、`resolve_task_recheck(token,id,action)`；ProjectToken在native解析directory，不能接受URL/actor/任意shell。P6读取同schema，不生成第二套字段规则。
- [ ] P6界面必须实现S7全部动作与真实对象详情，显示hash/目标/完整diff、接口两端高亮、Pending及切离main指引；拟议虚线覆盖不改变main坐标，批准才切换基线布局。shared styles与形状/颜色/文字规则仍适用，不能拿本文HTTP测试代替界面完成。
- [ ] **Step 7：真实人工验收留明确阻断项。** 给真人打包App项目入口与真实draft ID；真人合并/拆分/改名/拖动/接口/verify/docref后关闭重开，确认编辑仍在；真人先拒绝验证main不变，再审阅hash/目标后批准B0，后续真人批准Proposal形成B1。记录human事件、draft revision/hash、commit/tag、manifest/Decision与App操作证据。
- [ ] 若无人点击，回顾必须写“backend自动集成已验证；S7/P6界面与真人B0/B1验收未完成”，不得宣布首个可用版本完成。P7两个真实harness技能用同schema综合，十任务真人协作仍按发布总表独立验收。
- [ ] **Step 8：一次完整绿验证。** `~/.cargo/bin/cargo test --workspace --locked`；仅在实际P6接线落地后执行其native/前端相关测试。检查本批所有新增写路径都有对应拒绝、恢复与不改用户index测试。更新双语未发布日志：用户可保存审阅草稿、精确批准基线及追溯提案，保留未真人验收限制。
- [ ] **Step 9：精确提交与交回。** `git diff --check`；只stage本Task已验证文件和README/双语日志/任务证据，提交 `feat(cli): connect baseline drafts and proposals to daemon review`。在tasks/todo记录所有Task SHA及自动/真人证据；未提交改动明确标“未提交”，不替其他工作提交。

## 自审映射与完成边界

| 冻结要求/规格 | 对应任务与可验收证据 |
| --- | --- |
| S6、FR-18，共享schema、已有harness | Task1唯一schema/typed验证；Task6 schema CLI/HTTP与P7交接，无模型进程 |
| S7真实编辑、草稿真实对象、FR-98已有文档 | Task2七种typed操作、真实对象/普通md引用与字节保护；Task6 P6真人动作 |
| FR-95持久draft ref与App人工边界 | Task2 durable ref/cache删除恢复；Task3同ID/hash/target/revision审阅与拒绝 |
| S8、FR-37，一个commit+Decision+manifest+canvas+tag | Task4同writer发布及main/tag/ack各窗口；Task6真实CLI→daemon链 |
| D1私有state例外、无remote main检出 | Task2固定WritePurpose白名单；Task4/6第二worktree pending与用户指引 |
| 稳定hash、目录/manifest冻结、二次TOCTOU | Task1精确bytes摘要；Task3绑定；Task4 fresh-fetch/目录/tag/lease二次核验 |
| tag权威编号、并发不自动改目标 | Task4标签校验/同publication恢复；Task6两App/两批准竞争 |
| FR-35/36影响、备选/建议、接口两端 | Task5Proposal验证与真实diff；Task3ReviewView/P6高亮契约 |
| FR-38需复核、解除、CLI可送达、越界提案 | Task5独立App task recheck精准解除、ready可claim/running不越fence；P3inbox送达 |
| 人工事实/请求在checkpoint前丢hub仍可审计 | Task3复用P3 outbox fsync与统一receipt，原guards/绑定有效性分开重核 |
| 正常/恢复Git事件身份一致 | Task4唯一derived_eid(actual_commit,baseline.created)，tag重试不重复baseline/recheck |
| OKF未知键、type集中、canvas不透明保留 | Task1/2Document与CanvasDocument复用，类型只由types::TYPES写入 |
| 可重建/恢复、一处提交者、所有写路径测试 | Task2统一writer提取；Task3投影恢复；Task4故障矩阵；Task6全量绿 |
| 首版最终验收必须App真人完成 | Task6明确真人操作证据，自动HTTP/UI自动化不能替代 |

- [ ] 差量自审：Task3所有非heartbeat请求/事实先pending fsync并核原Event/confirmed fsync后成功，完整body/receipt可恢复；无guards baseline批准与带旧guards stale历史分开；Task5 ready/running独立复核及精确标记解除；Task4 normal/recovery共用commit-based eid/事件builder，tag重试不新增baseline或recheck。
- [ ] 自审schema规则不在TS重复，types/path/hash/ReviewBinding/publication字段名全篇一致；Task1–6没有调用未定义的P4接口；前置批接口在开工时按真实SHA核定。
- [ ] 自审不引入新依赖/自由开关/无头harness/CLI批准/私有B0；main/private快照安全继续共享批3统一读写，未复制提交者。
- [ ] 自审记录红测为可编译的行为失败、绿测命令/输出和精确SHA；真人与产物验收保持未完成直到实际证据成立。

当前状态：**仅完成首版实施计划，未实现、未运行上述测试、未提交；待Root独立审查冻结。** 本文件不构成已批准B0、已交付P4或已进入0.1.0发布产物的证据。


## Proposal 第 2 切片实施边界（源码候选）

基线 5bed9b8，叠加五路径载荷基础候选（哈希见隔离 proposal-inputs.json）。本切片仅 `Propose→同 writer 私有冻结稿→实际 accepted proposal.created→原 UUID 回执恢复`；共享事件/有限 receipt-only 角色见共享恢复契约新增章节。不依赖未提交 P2 Task4，不开放 Bn、Task recheck、CLI/daemon/App 新入口。Bn 全部既有硬阻不变。

原件先于续租/当前业务门禁协调；new UUID 才重新捕获实际 Run、全部 fences、当前基线与完整影响集合。只有 private ref 但无 accepted 不算成功；已保存稿缺原 binding 不补成新 epoch。下游 App/出版消费当前 Proposal 必须实际 outer provenance 和精确全稿绑定，不能信 carrier 或 confirmed-only。旧 319 的 15 项测试不计当前合格。Root 对冻结源码串行编译、真实故障回归和质量检查通过后才可记录提交。


### Proposal producer 04 静态审查修复（源码候选，未验证）

03 对 accepted Proposal 仅核 typed 正文和原 Task/Run/full-fences，未将 carrier 精确绑定实际保存 manifest，也未独立重算影响，实际 Hub non-stale 接受错误 carrier 可进入 accepted 索引。04 按共享恢复契约新增的有限历史校验，使用原不可变 writer commit、原 source Catalog、actual outer 中该事件前的完整 Task 人口复用同一 analyzer，核 saved payload/hash/impact。后来 mutable draft ref 编辑或删除不改变历史，Branch 与 Draft source 均固定原完整 OID；原件优先和 live 末 source/全 fences/CAS 不变。新增真实保存后篡改 carrier、真实 writer 保存错误影响及删除 ref 后的历史正控源码，由 Root 串行运行；未执行工具链、未提交，不代表 Bn 闭环。
