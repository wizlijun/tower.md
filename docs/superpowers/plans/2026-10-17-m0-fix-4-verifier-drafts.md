---
type: Plan
title: M0 整改批次 4：共用验证器与可重建冷启动草稿
description: 联合实施 R8 与 R9，通过真实 daemon 的异步 baseline scan 生成逐模块证据和可读取草稿，保护用户工作树、文档目录及 main。
tags: [towermd, plan, M0, remediation, verifier, coldstart, TDD]
generated: { by: codex/gpt-6, at: 2026-10-17T00:00:00Z }
sources:
  - id: remediation
    resource: docs/superpowers/plans/2026-10-17-m0-remediation.md
    title: 用户已批准 D0–D6；本批执行 R8 与 R9
  - id: spec
    resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: R16 已提交 b5a881f；第 8 节、FR-107、16.1 与 M0 验收
  - id: protocol-plan
    resource: docs/superpowers/plans/2026-10-17-m0-fix-2-hub-storage.md
    title: 批 2 的 HubProtocol、ProjectSession 与增量 StateProjection 接口计划
status: draft
---

# M0 整改批次 4：共用验证器与冷启动草稿 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking。

**Goal:** `baseline scan` 经真实守护进程异步完成 S1–S5，每个模块获得绑定 tree 的初始证据，并生成可重建、可读取且引用真实对象的草稿画布。

**Architecture:** 核心提供可直接供 M1 `finish` 使用的 detached worktree 验证器，coldstart 只负责命令归属、逐模块聚合和草稿数据。daemon 负责后台任务、派生文件、等待与 HTTP，证据经 `HubProtocol::append` 进入真实核心投影和快照；扫描不出版慢层或改变任务完成状态。

**Tech Stack:** 沿用 Rust stable、std::process、serde/serde_json/serde_yaml、uuid、sha2/hex、tempfile、tokio、axum、clap 与现有 Git CLI；不增加 crate、Cargo feature、沙箱或云服务。

---

## 开工前提与冻结边界

本计划是可执行设计，尚未实施或通过测试。当前旧源码只有同步 S1–S4；这里引用的批 2 接口是未来接口，不代表当前 checkout 已能编译。实施前必须先核对批 2、批 3 实际合入提交，重读 `HubProtocol`、`ProjectSession`、`StateProjection`、恢复发件箱与 daemon 项目注册路径，再把本文接口名对应到真实代码。若签名不同，只调整连接代码，不另建一套 Hub 或投影。

R8 的证据写入依赖批 2；真实重启、快照重放及发件箱验收依赖批 3。不得在未合入接口上编造绿测，不得复用旧 `append(event, LeaseChange, now)`。执行者在 `tasks/todo.md` 写入本批计划、前置提交号和检查项后开始；本计划不要求重新批准用户已经授权的 D3/D6。

本批一个联合闭环 Task，内部阶段逐个红绿验证，最终全绿后精确提交。中途可保留本地进度，但不能提交“CLI 已承诺 S5、证据仍只有临时 JSON”的半成品。

冻结要求：

1. `git worktree add --detach <path> <selected-commit>`，路径固定在 `$(git-common-dir)/towermd/verify/`；不物化 blob 代替 Git worktree，不清空环境，不禁网。
2. S5 与 M1 `finish` 共用核心验证器；本批不实现 finish 命令、S6–S8、B0 批准、App 或基线标签。
3. 每模块恰好一条初始 Evidence，结果仅 `passed`、`failed`、`no_tests`；无测试和发现不足都不能伪装为通过。
4. 非根命令归属包含清单目录的模块；根命令只 fallback 给没有自身命令的模块，标 `shared: true`；全批同 `(cwd, argv)` 只运行一次。
5. 待确认或不支持的命令不执行，不转成 shell 字符串偷偷运行；发现不足写明诊断，并生成候选任务供后续确认或补测试。
6. 扫描立即返回 ID；`baseline scan --wait <scan-id>` 每次最多等 90 秒；没有 `baseline verify`，没有 CLI 批准参数。
7. 草稿在 `$(git-common-dir)/towermd/drafts/<scan-id>/`；用户 `docs_dir`、`main`、工作树、index 不因扫描改动。
8. 25 个平级目录没有语义父分组时，允许按目录 slug 稳定排序后均衡分桶；根 3–9 组、组内 3–9 模块，附虚拟分组诊断。超过 81 递归；不足 3 下钻仍无法满足则诊断。
9. 模块 slug 不依赖虚拟 group；候选任务只在草稿中，不自动进入 ready、claim 或完成状态。
10. 批 4 不推送、不发版；后续 M1 接续未签名 App 的目标，不能提前批准 B0 以简化验收。

## 文件地图

| 文件 | 责任 |
| --- | --- |
| `crates/towermd-core/src/verifier.rs`（新建） | 共用验证请求、detached worktree、命令执行、超时进程组回收、结果与日志哈希 |
| `crates/towermd-core/src/coldstart.rs` | 固定提交扫描、稳定 slug、3–9 分组、命令归属、逐模块 Evidence 与候选任务数据 |
| `crates/towermd-core/src/coldstart_draft.rs`（新建） | 从固定扫描输入生成草稿模块 OKF 文本、画布、未划分清单及草稿读取数据 |
| `crates/towermd-core/src/snapshot.rs` | 真正应用 `evidence.recorded`，保留 Evidence 至投影与重放 |
| `crates/towermd-core/src/project_session.rs` | 使用批 2 实际接口追加并应用逐模块证据，不绕过增量会话 |
| `crates/towermd-core/src/lib.rs` | 导出共用验证器和草稿类型 |
| `crates/towermd/src/scan_jobs.rs`（新建） | daemon 后台扫描、任务输入/进度、等待通知、恢复协调、草稿原子出版 |
| `crates/towermd/src/daemon.rs` | 项目与 HTTP 编排；启动扫描、等待、`GET /drafts/{id}`；不持 hub 锁执行测试 |
| `crates/towermd/src/main.rs` | `baseline scan --wait <id>` 与准确帮助文本 |
| `crates/towermd-core/tests/verifier.rs`（新建） | 选定提交、环境继承、日志与进程回收 |
| `crates/towermd-core/tests/coldstart.rs` | 命令归属、slug、递归分组、候选任务 |
| `crates/towermd-core/tests/coldstart_draft.rs`（新建） | OKF、真实文件引用、派生重建 |
| `crates/towermd-core/tests/snapshot.rs` | Evidence 投影、快照与归档重放 |
| `crates/towermd/tests/coldstart_cli.rs` | 真实二进制、daemon、HTTP、等待、重启与保护路径验收 |
| README、双语 CHANGELOG、`tasks/todo.md` | 用户行为变化与实际验证、提交证据 |

若批 3 已提供通用 outbox 或 artifact 模块，复用其真实实现；不因本文文件地图重复建设。旧 `/private/tmp/towermd-m0-s5` 只读参考命令去重与进程回收经验，不复制其 sandbox、环境清空、单批证据或 `baseline verify` 代码。

## 核心接口与数据契约

共享契约 [2026-10-17-shared-recovery-proof-contract.md](2026-10-17-shared-recovery-proof-contract.md) 第1.1–1.2节先由E交付VerificationCommand/VerificationReport/CommandExecution与摘要；本批verifier.rs重导出同一类型，只实现VerificationRequest/verify执行行为；共享第1.3–1.4节窄评审/出版recorded与gate Evidence仍由E定义、P2/P3/P4生产，本批冷启动不生成这些人工事实，不能让E依赖尚不存在的运行器。以下是本批新增接口的最小设计，实施时补 serde derives 和错误上下文。`Duration` 只用于内部请求，不暴露未经批准的 CLI 配置开关。验证器不依赖 Catalog、S5 或 Task，因此 M1 finish 可先做路径守卫，再传相同请求。

```rust
pub use crate::recovery_contract::verification::{
    VerificationCommand, VerificationReport, CommandExecution, commands_hash, report_hash,
};
pub struct VerificationRequest {
    pub root: std::path::PathBuf,
    pub common_dir: std::path::PathBuf,
    pub revision: String,
    pub commands: Vec<VerificationCommand>,
    pub timeout: std::time::Duration,
    pub log_root: std::path::PathBuf,
    pub cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
pub fn verify(request: &VerificationRequest)
    -> Result<VerificationReport, String>;
```

`CommandExecution.result` 只有 `passed/failed`；超时、命令启动失败、退出非零都为 failed，并保留可区分诊断。工作树创建失败、日志持久化失败是扫描基础设施错误，不能生成没有有效日志的 passed Evidence。`no_tests` 由模块聚合产生，验证器不虚构一次命令执行。

`ScanInput` 持久记录 `project_id`、canonical `root/common_dir`、完整 `revision`、`tree`、`ScanOptions`、`algorithm`；冻结提交在接受扫描时确定，后续 main 移动不改变输入。重建使用同一 commit/options/algorithm；新扫描 ID 可以不同，模块 ID 与草稿语义应一致。

```rust
pub struct ScanInput {
    pub project_id: String,
    pub root: std::path::PathBuf,
    pub common_dir: std::path::PathBuf,
    pub revision: String,
    pub tree: String,
    pub options: ScanOptions,
    pub algorithm: String,
}
pub struct ModuleVerification {
    pub module: String,
    pub commands: Vec<VerificationCommand>,
    pub shared: bool,
    pub diagnostics: Vec<String>,
}
```

模块 Evidence 使用注册类型 `Evidence`，字段包含 `id/module/kind/commit/tree/result/log/generated`、`scan_id`、`shared`、命令执行引用、日志哈希与可选诊断；`kind: test`、`generated.by: process:towermd-verifier`，费用保持未知 null。无测试 Evidence 的 log 指向不可变模块报告，不能省略可追溯结果。

写入事件遵循批 2 计划的实际 `NewEvent`：`kind: evidence.recorded`、`layer: L5`、`actor: process:towermd-verifier`、`obj: evidence.id`、`data: { evidence: ... }`、空 guards。没有任务租约的冷启动证据不得伪造 epoch，也不得触发 `task.state` 或 `merge.done`。

## Task 1：实现 S1–S5 到证据与草稿的产品闭环

### 阶段 A：前置 API 复核与行为红测

**Files:** `tasks/todo.md`；现有 `coldstart_cli.rs`、`coldstart.rs` 测试。

- [ ] **A1：记录前置提交与可用接口。** 读取批 2/3 的实际源码；记下 `append`、增量同步、Snapshot 构建、outbox、私有快照引用和项目注册入口，不改前置批的协议语义。
- [ ] **A2：先运行基线。** `cargo test -p towermd --test coldstart_cli --locked` 与 `cargo test -p towermd-core --test coldstart --locked`，预期当前既有测试通过；失败时先区分前置回归，不冒称本批已绿。
- [ ] **A3：替换同步准备测试的产品断言。** 保留现有 Fixture 的真实 daemon 启动和隔离 runtime；扫描返回 `scan_id/state`，等待后 S5 完成、模块证据非空。将“不创建 hub”改为“无未授权慢层与任务状态变化”，读取 canvas/object 的纯只读断言继续保留。

```rust
#[test]
fn baseline_scan_returns_id_and_wait_produces_module_evidence() {
    let fixture = Fixture::new();
    let started = fixture.cli(fixture.root.path(), &["baseline", "scan"]);
    let id = started["scan_id"].as_str().unwrap();
    assert!(uuid::Uuid::parse_str(id).is_ok());
    let result = fixture.cli(fixture.root.path(), &["baseline", "scan", "--wait", id]);
    assert_eq!(result["state"], "completed");
    assert_eq!(result["stages"]["S5"], "completed");
    assert_eq!(result["evidence"].as_array().unwrap().len(),
               result["modules"].as_array().unwrap().len());
}
```

- [ ] **A4：运行真实红测。** `cargo test -p towermd --test coldstart_cli baseline_scan_returns_id_and_wait_produces_module_evidence --locked`，预期旧返回没有 scan_id 或 CLI 不认识 --wait；记录失败输出。缺 import 修好后必须看到行为失败，不能只把编译失败当复现。
- [ ] **A5：检查命令边界。** help 只列 scan；`baseline verify`、`--approve`、`--yes`、`--execute`、用户自定输出路径均拒绝。历史上拒绝 execution 字段的 API 测试继续成立，S5 是批准后的扫描默认阶段。

### 阶段 B：稳定模块、命令归属与均衡层次

**Files:** `coldstart.rs`、`tests/coldstart.rs`。

- [ ] **B1：写混合 Rust/JS 清单红测。** 夹具包含根 Cargo workspace、`crates/core/Cargo.toml`、`packages/ui/package.json`（显式 pnpm）、`tools/check.sh`。断言 core 只取自身 cargo，ui 只取自身 pnpm，tools 才取共享根 cargo；不得对有自身命令模块再附根 cargo。
- [ ] **B2：实现清单归属。** 在目录模块确定后按规范相对 cwd 匹配清单所在目录；同目录 manifest/CI 的相同 argv 合并来源。CI 的 working-directory 是归属线索，不代表允许执行其任意 shell 脚本；不能用字符串前缀误把 `src/a2` 归给 `src/a`。
- [ ] **B3：实现保守命令分类。** 保留 `argv=null/requires_confirmation/unsupported` 元数据；它们属于“自有命令发现记录”，不能因为不可执行就给模块根命令并宣称已验证。模块没有任何自有发现记录才用根 fallback；没有可运行命令时出 no_tests 并说明是未发现、待确认或格式不支持。
- [ ] **B4：写 root 去重红测。** 两个没有自身命令模块都引用同一根命令，计划保留两个 shared 归属但执行集合只有一个 `(cwd, argv)`。不同 cwd 的相同 argv 是不同命令；相同 cwd/argv 的不同 source 不是不同执行。
- [ ] **B5：写稳定 slug 红测。** 用 `crates/core`、`packages/ui`、`foo-bar` 与 `foo/bar` 冲突夹具；两次扫描相同输入得到相同、可读且唯一的 `A-...`。对规范化后相同 slug 按完整目录键稳定排序加数字后缀，不能使用随机数或扫描 ID。
- [ ] **B6：实现分组算法。** 优先语义父目录；宽扇出节点按 slug 排序，将 n 个孩子均衡分成 `max(3, ceil(n/9))` 桶，每桶长度差不超过 1；若桶数超过 9，对这一层 group 继续向上递归。25 得到 3 桶（9/8/8）；82 先有10桶，再有3个父group（4/3/3）。10–18不能只分2桶，根层也必须满足下限；每次虚拟分组产生原因诊断。
- [ ] **B7：写下限与分组回归。** `<3` 时按一级子目录重新划分并重算成员、paths 与命令归属；不可继续下钻的 1/2 个模块保留原事实并出 `module_fanout_unachievable`，不造空模块凑数。0 源码仓库 coverage 为 not_applicable，并给诊断。
- [ ] **B8：保持历史修正一致。** S4 的高共变修正不能重新产生无诊断的 10+ 平铺模块；修正结束后统一构造分组树与命令归属，coverage 仍只算真实模块成员，group 不重复计数。
- [ ] **B9：运行绿测。** `cargo test -p towermd-core --test coldstart --locked`。精确检查 25、81、82、超过 729 的结构及所有可满足内部层的 3–9 fanout；unassigned 区域节点不计为模块。

### 阶段 C：共用 detached worktree 验证器

**Files:** 新建 `verifier.rs`、`tests/verifier.rs`；修改 `lib.rs`。

- [ ] **C1：写真实选定提交红测。** 初始化 Git，提交一个通过的 `sh check.sh`；工作区再将它改失败，同时制造 staged、unstaged、untracked 内容。调用 verify 指定原完整 SHA；断言报告通过、commit/tree 等于原提交，用户文件/index字节和 status 不变。
- [ ] **C2：建立与回收 worktree。** 独占 UUID 子目录，验证 common_dir 与仓库相符，执行 `git worktree add --detach <path> <sha>`；再次读取 HEAD/tree 验证。正常结束、命令失败和创建后异常都 remove --force；只删除自己创建的精确目录，不 prune 或清理其他工作树。

```rust
use std::os::unix::process::CommandExt;
let mut child_command = std::process::Command::new(&command.argv[0]);
child_command.args(&command.argv[1..]);
child_command.current_dir(worktree.join(&command.cwd));
child_command.stdin(std::process::Stdio::null());
child_command.stdout(std::process::Stdio::piped());
child_command.stderr(std::process::Stdio::piped());
child_command.process_group(0);
```

取消信号仅由 daemon 的 job 生命周期控制，默认 false，不提供 CLI/HTTP 绕过参数；执行轮询读取它，取消时回收当前进程组并把 job 标 interrupted，不把尚未执行模块记为 passed。正常 stop 先设置各 job 的取消信号，再等待它们完成清理。

上面 Unix 分支外保留平台 cfg；不调用 env_clear、不设置替代 HOME、不禁网。Git 管理命令沿用 repo 的 Git 环境隔离；测试命令继承工具链环境，但剔除能把 Git 指向用户工作树的 GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE 等重定向变量，避免继承环境破坏选定 tree 语义。

- [ ] **C3：校验执行输入。** argv 非空且每个参数不含 NUL；cwd 必须规范相对路径，拒绝绝对路径、`..` 和穿出 worktree 的符号链接。直接 argv 启动，不拼 shell；只有被清单明确识别的可运行 argv 进入请求。
- [ ] **C4：写环境与网络行为测试。** 子测试进程向 daemon/verify 传入专用环境标记，脚本读取标记并成功；本地 loopback listener 由脚本访问成功证明没有禁网，不能要求外部互联网可用。不得在并行测试线程修改全局环境变量。
- [ ] **C5：写进程组红测。** 脚本写入父/子 PID 后等待，子进程忽略 TERM；短超时后 `/bin/kill -0` 验证它们已退出，worktree list 不含验证目录。另写 leader 正常退出但后台子进程持有 stdout 的测试，调用必须有界返回并清理子进程。
- [ ] **C6：实现有界输出与回收。** 两条输出管道并发排空到日志文件；超时或收到内部取消信号时向负 PGID 发 KILL，再有界 wait/reap，清理后才 remove worktree。leader 正常退出仍清理残留组；捕获输出不在内存无界 read_to_end，不因子进程持管道永久阻塞。
- [ ] **C7：写命令失败绿测。** 非零退出、找不到程序、超时分别产生 failed 和明确诊断；环境问题不能因为工具不存在被当 no_tests。一个命令失败不阻止其他模块得到证据，基础设施失败则扫描 state=failed 并记录已经持久化证据 ID。
- [ ] **C8：运行验证器绿测。** `cargo test -p towermd-core --test verifier --locked`；退出后检查仅本次 verify 子目录消失，主 worktree 与其他验证 worktree 均仍存在。Windows 整组回收不冒充已验证；本轮 macOS 必须有真实 PID 验收。

### 阶段 D：不可变日志与逐模块 Evidence

**Files:** `verifier.rs`、`coldstart.rs`、`project_session.rs`、`snapshot.rs`；对应 core tests。

- [ ] **D1：实现本机缓存日志。** 普通日志与命令报告放 `git-common-dir/towermd/cache/verification/<id>/`，create_new 写入、flush/fsync，完成后计算 sha256。Evidence 只引用封存文件；相同路径第二次写入失败，后续更正产生新 Evidence，不修改旧日志。
- [ ] **D2：落实 FR-75 大文件路径。** 复用实际已有 artifact/Arca 写入器；大文件由 daemon 写主 clone 的受忽略 artifacts 路径，绝不写验证 worktree。未安装 Arca 用受忽略本地缓存 fallback，标 local_only，不新增依赖或必需账号；测试覆盖无 Arca 的真实路径和 hash。
- [ ] **D3：保护文档范围。** 扫描默认日志全留本机 cache，不主动创建/改写用户 docs。若显式 FR-75 大文件搬送与严格 docs 不变验收冲突，保留缓存并报告 local_only，不能以日志写入为由改 docs 或 main；实施前用批 3 实际 artifact 接口核对这个分支。
- [ ] **D4：写逐模块聚合测试。** commands 非空且全部执行通过才 passed；任一执行 failed 则模块 failed；没有允许执行的命令则 no_tests。共享根结果可被多个独立模块证据引用，但必须只有一个 execution/log，不能整批一个 Evidence。
- [ ] **D5：实现候选任务。** no_tests 模块生成一个候选 Task，包含 module、建议 allowed_paths、标题和“未发现/待确认/不支持”原因；候选仅写草稿列表，不 append task.state，不伪造 Acceptance，不自动 claim。补测试建议不得暗示已有测试不存在。
- [ ] **D6：先写 Evidence 投影红测。** 构造合法 `evidence.recorded` 事件，调用真实 `StateProjection::apply_recorded` 后 `objects["Evidence"][id]` 等于完整 Evidence；未知附加键保留，stale 事件不生效，错误 type/id 拒绝且不部分更新。
- [ ] **D7：接入真实增量会话。** core 按 evidence.recorded 分派并用类型登记校验对象；通过批 2 的 trait append 后执行相同增量同步路径。Duplicate receipt 只恢复原证据，不重复 apply；不能只在 scan response 挂 evidence JSON。
- [ ] **D8：写快照重放红测并通过。** 混合 Task、Run、三种 Evidence，生成快照与归档，销毁会话后从快照加 tail恢复，Evidence 完整相等、数量准确；归档全重放与 state.json 相同。不得用临时扫描 job 内存补齐丢失证据。
- [ ] **D9：运行绿测。** `cargo test -p towermd-core --test verifier --locked`、`cargo test -p towermd-core --test coldstart --locked`、`cargo test -p towermd-core --test snapshot --locked`；保持批 2 receipts/心跳区间和增量计数安全测试。

### 阶段 E：真实对象、层次画布与派生重建

**Files:** 新建 `coldstart_draft.rs`、`tests/coldstart_draft.rs`；修改 `coldstart.rs`。

- [ ] **E1：先读 OKF 约束。** 读取 `../note.md/docs/okf-v0.2-format-constraints.md` 与现有 `okf::Document` 写入方式；草稿 Module 用注册 type、非空 id、`status: draft`、process actor 和提交来源，不添加人批准字段或 baseline tag。
- [ ] **E2：生成完整草稿数据。** 保存 `input.json`、`result.json`、`arch/<module-id>.md`、`tower.canvas`、`unassigned.json`、`candidate-tasks.json`。Module 包含 name/paths/interfaces/verify，层次只用 group 节点表达，虚拟 group 不伪造新的 Module。
- [ ] **E3：写文件引用红测。** 每个 canvas file 为草稿根相对 `arch/<id>.md`，`towermd.obj` 等于对应 Module.id；逐个加载文件并解析 OKF，确认文件真实存在、内容属于同一扫描 input。不得继续引用 docs_dir 下尚不存在的 arch 文件。
- [ ] **E4：布局不改基线。** 本批在草稿中生成确定性布局，group 包围其子节点；新扫描不会修改已有权威 tower.canvas。层次和几何保持整数，以既有 CanvasDocument 解码回归证明可读；不新增 elkjs 或 UI 依赖。
- [ ] **E5：实现原子出版。** daemon 在 drafts 下独占临时目录写完并同步，再 rename 为 `<scan-id>`；GET 只能读已出版结果。拒绝扫描 ID 路径穿越与草稿目录符号链接；不跟随请求提供的任意文件路径。
- [ ] **E6：写重建测试。** 删除 drafts/<id> 后，用固定 input 的同一提交/options重跑；比较 Module.id/paths/verify、group结构和unassigned语义相同。日志、Evidence ID、scan ID与时间可以不同，不能要求第二次测试执行结果永远一样。
- [ ] **E7：运行绿测。** `cargo test -p towermd-core --test coldstart_draft --locked` 与 `cargo test -p towermd-core --test canvas --locked`；已有未知键往返和非法条目诊断继续通过。

### 阶段 F：异步任务、90 秒等待与重启协调

**Files:** 新建 `scan_jobs.rs`；修改 `daemon.rs`、`main.rs`、`coldstart_cli.rs`。

- [ ] **F1：冻结接受请求。** 保留 `/baseline/coldstart` POST 起点，校验实例、项目/clone及run后捕获 ScanInput；不在请求线程扫描历史或执行测试。返回 `{scan_id, state: "queued"}`，后台工作不得持 projects/session 锁跑命令。
- [ ] **F2：定义等待请求。** 同一路由请求体加互斥 `wait: Option<String>`；CLI `Scan { wait: Option<String>, history_limit: Option<u16> }`，wait 与 history-limit 冲突。新扫描默认200，wait不接受执行选项、actor、output或approve。
- [ ] **F3：实现等待。** daemon 用通知量等待 job 状态，最多90秒返回当前 `{scan_id,state,stages,...}`；未完成提示再次调用。完成结果重复等待幂等，不能再次运行命令；CLI HTTP timeout略大于90秒只作传输余量，不把服务端等待延长到120秒。
- [ ] **F4：写有界等待测试。** 内部将等待预算参数化供单元测试用毫秒预算，生产常量90秒不可由客户端覆盖；使用现有tokio time与短deadline验证到期返回pending、完成通知提前唤醒，不新增test-util feature。真实 daemon用短任务验证CLI及时返回ID，长任务期间status仍响应。
- [ ] **F5：持久化工作事实。** runtime扫描索引保存输入、进度和已记录Evidence IDs；唯一业务证据必须在hub/快照。待追加事件经E唯一append_durable，固定NewEvent/eid先pending fsync→append→核真实完整Event→confirmed fsync；confirmed失败保持uncertain，Hub已接受事实仍正常sync。满足确认耐久后标记已发送，job不能在确认前宣称模块证据已记录。
- [ ] **F6：重启协调。** 启动时读job输入与outbox，先按原eid重发未确认事件、从核心投影恢复已记录证据。执行中被打断的命令标interrupted并重新验证生成新结果；不把未知退出状态当passed，不修改原Evidence，不因runtimejob丢失删hub证据。
- [ ] **F7：写两个崩溃窗口测试。** 证据append成功但job结果未写时终止daemon，重启后等待恢复同一Evidence且不重复追加；只有job输入而无Evidence时终止，重启重跑或明确interrupted，不能返回completed。故障注入只在测试内部，不加生产绕过参数。
- [ ] **F8：实现停止收尾与安全恢复。** daemon正常stop先取消/等待扫描，验证器严格清理进程组/worktree。异常重启后，持久job登记的PID/PGID可能已复用，不能据此盲kill，`kill -0`也只证明存活、不证明身份；只有验证当前进程身份及启动代际属于该验证任务后才允许终止。无法证明时明确记录`cleanup_identity_unverified`诊断，不杀用户进程；仅在确认验证已不再占用后移除对应自有worktree，否则保留并报告待清理。M0不新增大型进程监督架构，也不宣称异常退出总能自动回收；回归测试用存活的无关进程PID作为旧job登记，断言不发送终止信号、不误删仍占用目录，另验证正常stop确实清理子进程。证据/outbox持久化结果不依赖JoinHandle仍存在，不能全局prune。
- [ ] **F9：接入 GET `/drafts/{id}`。** 使用既有实例校验和项目读取上下文，返回input、模块对象文本、canvas、unassigned、candidate_tasks与证据引用；不存在404、其他项目ID拒绝、未出版任务返回状态而不读半份文件。权威 `/canvas` 路径保持原语义。
- [ ] **F10：运行绿测。** `cargo test -p towermd --test coldstart_cli --locked` 与 scan_jobs单元测试；新扫描、重复wait、坏id、跨项目、错误实例、重启与停止全部有真实HTTP断言。

### 阶段 G：真实 daemon 的联合验收

**Files:** `coldstart_cli.rs`；测试证据写 `tasks/todo.md`。

- [ ] **G1：混合仓库产品验收。** Rust＋JS夹具从CLI scan启动、CLI wait完成，再HTTP GET drafts；用脚本在本机cache写执行计数，证明自身命令各跑一次、共享根命令全批一次。断言每个模块独立Evidence、shared标记与tree准确。
- [ ] **G2：保护用户内容。** 扫描前后采集用户tracked文件内容、index原始bytes、status --porcelain、refs/heads/main与docs目录文件清单/hash；staged、unstaged、untracked三种变化均保留。verify worktree的生成文件不得泄漏用户根目录。
- [ ] **G3：分层与引用验收。** 25、81、82顶级目录夹具通过真实daemon扫描；逐层检查fanout及虚拟分组诊断，逐file节点验证GET返回真实Module文本。重复扫描比较稳定module slug，不能只直接调用prepare充当产品验收。
- [ ] **G4：无测试与待确认验收。** 一模块无manifest、一模块只有unsupported或待确认CI、一模块真实失败；分别得到诚实no_tests/no_tests/failed与诊断，候选任务存在，未执行待确认脚本。真实任务投影前后无自动ready/claim/done。
- [ ] **G5：清理与快照验收。** 用真实daemon执行超时派生子进程命令，验证PID/worktree回收；请求snapshot，检查state里Evidence，重启后再读；按批3App确认恢复产品路径重建后Evidence仍可从归档恢复，不能读旧job响应蒙混。
- [ ] **G6：删除派生数据验收。** 移除某份draft后GET返回404但hub证据仍在；对保存的同一revision/options重新scan，得到可解析草稿与稳定模块，解释为何测试结果/证据时间允许变化。
- [ ] **G7：运行联合绿测。** `cargo test -p towermd --test coldstart_cli --locked`、`cargo test -p towermd --test daemon_cli --locked`、`cargo test -p towermd-core --test snapshot_service --locked`；保留离线读取、私有快照引用与审批隔离回归。

### 阶段 H：Tower.md / Note.md 真实仓库扫描与交付

**Files:** README、双语CHANGELOG、`tasks/todo.md`；前述本批源码测试。

- [ ] **H1：构建真实二进制。** `cargo build -p towermd --locked`。记录二进制对应的实现提交/未提交状态，不用直接调用prepare替代CLI。
- [ ] **H2：准备两个真实仓库副本。** 对 Tower.md 与 Note.md 各创建独立 `/private/tmp` clone，记录源仓库冻结SHA。若未初始化，仅在副本按显式docs_dir执行init并提交初始化产物；原仓库docs/main/index均不动。记录这一个初始化提交与原冻结SHA的关系。
- [ ] **H3：各跑一次真实scan产品路径。** 用隔离 `TOWERMD_RUNTIME_DIR` 启动本批二进制，执行 `baseline scan`，保存scan ID，再循环 `baseline scan --wait <id>`，每次<=90秒；通过真实HTTP GET drafts读取画布、模块与证据。不得另加baseline verify或绕过命令发现。
- [ ] **H4：记录真实验收结果。** 两仓库分别记录模块/组数、覆盖率、无法满足下限的诊断、passed/failed/no_tests数量、执行次数与耗时、tree、日志路径/hash、草稿路径、HTTP结果、main/index/docs前后比较。测试命令失败可以是真实初始证据，但基础设施错误或漏模块不能冒称验收完成。
- [ ] **H5：写用户日志。** README说明扫描会运行所选提交测试、异步ID与重复等待、草稿读取、环境继承及无测试含义；双语Unreleased各增加“冷启动生成可读取草稿与逐模块初始证据”。不宣称已批准B0、已发布或M1 finish/App已完成。
- [ ] **H6：完整检查与计划自审。**

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
rg -n 'baseline verify|env_clear|sandbox-exec' crates/towermd-core/src crates/towermd/src
```

预期全部必需检查通过，最后一条没有新的生产路径命中；测试里的拒绝字符串不算违规。复核证据确实在StateProjection/快照而非仅job JSON；若后续改动引入失败，修复后只重跑受影响检查与最终必需门禁。

- [ ] **H7：精确提交。** `git status --short`、`git diff --stat`后只显式暂存本批已验证路径，检查cached diff再提交，建议标题`fix: generate coldstart drafts with per-module verification evidence`。不混入其他批未验证修改、AGENTS.md/CLAUDE.md或旧S5工作树；记录真实SHA，未提交明确标“未提交”。
- [ ] **H8：交接M1。** 记录共用verify接口、待确认命令清单、可重建草稿输入以及App读取入口，供M1 finish、S6–S8和未签名App接续；本批没有批准B0、创建基线标签或伪造human事件。

## 关键红测代码与验收账本

下列用现有 `tests/coldstart.rs` 的 `fixture/write/commit` 辅助函数，补充阶段B的具体行为断言。新增接口以实现后的实际导出为准；这段使用保留的 `prepare_coldstart`、`ScanOptions` 与JSON字段，不宣称旧实现已经通过。

```rust
fn plan_for_root<'a>(scan: &'a serde_json::Value, root: &str)
    -> &'a serde_json::Value
{
    let module = scan["modules"].as_array().unwrap().iter()
        .find(|module| module["roots"].as_array().unwrap()
            .iter().any(|entry| entry.as_str() == Some(root)))
        .unwrap();
    scan["validation_plans"].as_array().unwrap().iter()
        .find(|plan| plan["module"] == module["id"]).unwrap()
}

#[test]
fn own_commands_exclude_root_fallback() {
    let root = fixture();
    write(root.path(), "Cargo.toml", "[workspace]\nmembers = []\n");
    write(root.path(), "crates/core/lib.rs", "pub fn core() {}\n");
    write(root.path(), "crates/core/Cargo.toml",
          "[package]\nname = 'core'\nversion = '0.1.0'\n");
    write(root.path(), "packages/ui/index.js", "export const ui = 1;\n");
    write(root.path(), "packages/ui/package.json",
          r#"{"packageManager":"pnpm@10.0.0","scripts":{"test":"true"}}"#);
    write(root.path(), "tools/check.sh", "true\n");
    commit(root.path());
    let scan = prepare_coldstart(&Catalog::load(root.path()).unwrap(),
                                &ScanOptions::default()).unwrap();
    let core = plan_for_root(&scan, "crates/core");
    let ui = plan_for_root(&scan, "packages/ui");
    let tools = plan_for_root(&scan, "tools");
    assert_eq!(core["commands"].as_array().unwrap().len(), 1);
    assert_eq!(core["commands"][0]["cwd"], "crates/core");
    assert_eq!(core["shared"], false);
    assert_eq!(ui["commands"].as_array().unwrap().len(), 1);
    assert_eq!(ui["commands"][0]["argv"], serde_json::json!(["pnpm", "run", "test"]));
    assert_eq!(ui["shared"], false);
    assert_eq!(tools["commands"].as_array().unwrap().len(), 1);
    assert_eq!(tools["commands"][0]["cwd"], ".");
    assert_eq!(tools["shared"], true);
}

#[test]
fn unsupported_own_manifest_is_not_a_root_test_pass() {
    let root = fixture();
    write(root.path(), "Cargo.toml", "[workspace]\nmembers = []\n");
    write(root.path(), "worker/run.py", "pass\n");
    write(root.path(), "worker/pyproject.toml", "[project]\nname = 'worker'\n");
    commit(root.path());
    let scan = prepare_coldstart(&Catalog::load(root.path()).unwrap(),
                                &ScanOptions::default()).unwrap();
    let worker = plan_for_root(&scan, "worker");
    assert_eq!(worker["shared"], false);
    assert!(worker["commands"].as_array().unwrap().iter()
        .all(|command| command["cwd"] != "."));
    assert_ne!(worker["status"], "passed");
    assert!(!worker["diagnostics"].as_array().unwrap().is_empty());
}
```

先跑 `cargo test -p towermd-core --test coldstart own_commands_exclude_root_fallback --locked`，旧逻辑因root命令附给全部模块而失败。再跑 `unsupported_own_manifest_is_not_a_root_test_pass`，实施后必须通过；该红绿证据只证明归属，实际执行去重必须另由阶段G的真实daemon计数证明。

验收账本至少保存以下列，使用真实输出填值；没有运行的格子写“未运行”，不能由计划复选框推断成功。

| 场景 | 记录证据 |
| --- | --- |
| Rust/JS/root fallback | scan_id、模块→execution映射、每command执行次数 |
| selected commit/dirty tree | 原revision/tree、index hash、status与文件hash前后值 |
| timeout/background child | 父子PID、退出检查、worktree list前后值 |
| 分组10/25/81/82 | 各层fanout、稳定module IDs、虚拟分组诊断 |
| GET草稿 | 请求实例/项目、HTTP状态、每file路径及对应Module.id |
| no_tests与unsupported | 模块结果、原因、候选任务、脚本未执行证据 |
| append与重启 | 原event eid、原seq、重启后Evidence数量与内容 |
| 快照重放 | snapshot commit、through_seq、归档重放与投影比较 |
| 删除draft重建 | 固定input、两次语义比较、仍保留的hub Evidence |
| Tower.md与Note.md | 各自冻结SHA/初始化提交、二进制来源、真实CLI及HTTP输出 |

## 自审映射

| 要求 | 对应阶段 |
| --- | --- |
| FR-107共用detached verifier、继承环境/网络、整组清理 | C、G |
| 每模块三态Evidence、root fallback、全批dedup | B、D、G |
| 待确认/unsupported不执行、不虚构通过 | B、D、G |
| trait append、真实投影/快照/恢复保留Evidence | D、F、G |
| async ID、90秒wait、重启协调、无baseline verify | A、F、G |
| 稳定slug、25/81/递归分组与下限诊断 | B、E、G |
| 草稿真实文件、GET读取、固定输入派生重建 | E、F、G |
| no_tests候选任务、无自动claim/done/B0批准 | D、G、H |
| 用户dirty/staged/untracked、docs/main/index保护 | C、D、G、H |
| 两真实仓库产品验收与精确提交证据 | H |

执行前后的规格审查与质量审查由Root安排；本文所有框保持未完成，测试输出和真实提交号只在实施后记录。
