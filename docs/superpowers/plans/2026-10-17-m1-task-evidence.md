---
type: Plan
title: M1 P2 任务门禁与证据链实施计划
description: 以权威 main、完整运行 fences 和不可变验证证据闭合 check、finish、trailers 与唯一合并认证。
status: draft
tags: [towermd, M1, P2, gates, evidence]
sources:
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-3-snapshot-recovery.md
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-4-verifier-drafts.md
  - resource: docs/superpowers/plans/2026-10-17-m1-human-intervention.md
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: AGENTS.md
  - resource: tasks/lessons.md
  - resource: docs/superpowers/plans/2026-10-17-shared-recovery-proof-contract.md
---

# M1 P2 任务门禁与证据链 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 G0/G2/G3/G5、异步 finish、有界等待和 trailers；只有权威 main 上经唯一证明器认证的合并才能完成任务。

**Architecture:** 核心捕获不可变输入、判断门禁、生成业务事件；daemon 运行批 4 共用验证器并续租，CLI 只发送 process 请求。普通观察、恢复与 outbox 继续调用批 3 唯一 Git/Evidence 证明器；不新增第二份 tree 比较或完成判定。

**Tech Stack:** 现有 Rust、serde、serde_json、serde_yaml、sha2、uuid、tokio、axum、clap 与 Git CLI；本计划不新增依赖。

---

**共享证明前置契约：** [2026-10-17-shared-recovery-proof-contract.md](2026-10-17-shared-recovery-proof-contract.md) 是 E/P2/P3/P4 的唯一恢复证明输入、confirmed 持久顺序、历史 merge 与 publication schema 来源。以下接口均待实现；开工按实际前置 SHA 对齐，缺 producer/schema 只诊断，不用夹具或字段自报代替真实产品路径。

## 开工事实与依赖

本文件只定义实施工作，所有检查框均未执行。当前主工作树已有 Catalog、任务租约、快照与 daemon/CLI，但没有生产 check/finish/trailers、共用 verifier 或完整 merge proof。批 3、批 4 文件是计划，不能当作已经存在的生产 API。

只读核对过 /private/tmp/towermd-m0-remediation-2：迁移中的 NewEvent.guards: Vec<Fence>、HubProtocol::append/subscribe/lease_acquire/lease_renew/lease_release、ProjectSession::sync/execute/tick/renew_binding/checkpoint、TaskService::apply/restore 将取代旧 Hub + LeaseChange 路径。该工作树尚在联合修改；其当前字段不是 P2 的稳定发布接口。

实施必须等待 **M0 批 2–5 全部合入并验证**，记录实际提交与签名后再开工。特别重核批 3 proof 的可见性、恢复 outbox、批 4 VerificationRequest/VerificationReport/verify 与 Evidence 格式。下面新增 typed API 是本计划要求实现的契约，不能在未定义时直接调用。

**与 P3 的真实编译顺序：** M0 批2–5完成 → P2 Task1复用E纯守卫并接线实时检查/claim durable → P3 Task1最小真实Review闭环（HumanState、ReviewSubject/Binding/Fact、ProjectSession.human、current_plan_subject/require_human_review、投影/快照恢复、review request与最小App事实入口、默认无事实硬阻reader）→ P2任务2及后续消费者 → P3其余问答/投递/暂停与P6完整界面集成。P2任务1不依赖human字段；P3 Task1的G4请求用其只读 `task_gates::check` 捕获tree，不依赖P2 finish。P2任务2开始前必须核对P3首批实际提交并跑其回归。P3最小App事实入口之外的完整人工界面尚未就绪时，相应高风险产品验收仍blocked，不能把fixture批准称为真人通过；P2完整验收需要与P3/P6联合完成。

本批测试可以在隔离仓库预置已批准 B0/Module/Acceptance 与人工评审事实；这只测试消费者，不构成人工批准。S7/后续 App 批准验证命令属于 P4/P6；必要高风险评审 Hold 与 App 确认属于 P3/P6。缺这些生产接口时明确 blocked，禁止临时自动通过。双 harness、三工作树、十真实任务属于 P7，不由本批 fixture 冒充。

本计划只写本文件，不修改其他计划、todo、changelog、源码，不安装依赖、不提交。未来实施在专用工作树执行六个可独立编译验证的闭环任务；每个任务的提交只包含本批已验证改动。

## 文件地图

| 文件 | 责任 |
| --- | --- |
| crates/towermd-core/src/task_gates.rs（E已有） | 复用唯一raw/G5，补G0/G2实时检查；不执行命令 |
| crates/towermd-core/src/finish.rs（新） | 输入冻结、Job/Evidence 区别、计时、失败计数与状态事件 |
| crates/towermd-core/src/recovery_git.rs（批 3） | 唯一 trailer 上下文与 Git/Evidence 证明；扩展而不复制 |
| crates/towermd-core/src/tasks.rs | 活动 Run 查询、verifying/held 绑定、G0、生命周期应用 |
| crates/towermd-core/src/project_session.rs | 经协议追加/同步、合并认证入口，不全量 replay |
| crates/towermd-core/src/snapshot.rs | Evidence 不可变投影、Run 计时/失败、Hold 重放 |
| crates/towermd-core/src/repo.rs | 复用E已交付的有界字节Git，不对raw-z做trim |
| crates/towermd-core/src/lib.rs | 导出本批核心入口 |
| crates/towermd/src/finish_jobs.rs（新） | 后台验证、通知/等待、恢复协调、续租 |
| crates/towermd/src/daemon.rs、main_refresh.rs | HTTP/实例/项目隔离，观察 main 后请求核心认证 |
| crates/towermd/src/main.rs | check/finish/trailers 参数与退出语义 |
| crates/towermd-core/tests/task_gates.rs、finish.rs（新） | 真实 Git diff 与状态/协议测试 |
| crates/towermd-core/tests/recovery_inputs.rs（批 3） | 同一证明器的正常/恢复/outbox 反例 |
| crates/towermd/tests/daemon_cli.rs | 扩展现有真实二进制 Fixture 的 P2 验收 |
| 主规格、README、双语 CHANGELOG、tasks/todo.md | 实施时同步冻结澄清、用户行为、实际证据与精确提交号 |

## 共享 typed 契约

共享契约第1.1–1.2节是唯一 DTO/摘要/纯守卫签名来源，E先交付；任务1扩展同一 task_gates 的实时检查，任务2实现 finish producer。以下仅列本批新增行为和对共享定义的重导出，不重复定义 schema；所有接口仍待实现。

~~~rust
// task_gates.rs
use std::{collections::BTreeMap, path::PathBuf};
use crate::{catalog::Catalog, hub_protocol::{Fence, Lease}, tasks::{Task, Run}};
// Gate/GateError/RawChange/parse_raw_z/check_g5已由E在本模块实现，见共享契约。
pub struct CheckReport {
    pub commit: String, pub tree: String, pub base: String,
    pub gates: BTreeMap<String, bool>, pub changed: Vec<RawChange>,
}
// parse_raw_z、RawChange、Gate/错误与纯 check_g5 沿 E 已交付定义；不重写。
pub fn check(catalog: &Catalog, task: &Task, run: &Run, host: &str,
    leases: &BTreeMap<String, Lease>, now_ms: u64) -> Result<CheckReport, GateError>;

// finish.rs
pub enum JobState { Queued, Running, Recording, Completed, Interrupted }
pub use crate::recovery_contract::finish::{
    EvidenceResult, FrozenFinish, FinishEvidence, VerificationLogRef,
};
pub struct FinishJob {
    pub input: FrozenFinish, pub state: JobState, pub diagnostic: Option<String>,
}
pub fn freeze(session: &crate::project_session::ProjectSession,
    catalog: &Catalog, task: &Task, run: &Run,
    checked: CheckReport, evidence_id: String,
    entered_at: u64) -> Result<FrozenFinish, GateError>;
pub fn require_finish_reviews(session: &crate::project_session::ProjectSession,
    catalog: &Catalog, input: &FrozenFinish, gate: Gate) -> Result<(), GateError>;
pub fn make_evidence(input: &FrozenFinish, completed_eid: &str,
    report: &crate::verifier::VerificationReport) -> Result<FinishEvidence, GateError>;
pub fn record_finish(session: &mut crate::project_session::ProjectSession,
    catalog: &Catalog, evidence: FinishEvidence, report: crate::verifier::VerificationReport,
    now: u64,
    outbox: &mut crate::recovery_outbox::RecoveryOutbox) -> Result<(), GateError>;
~~~

check 检查 G0/G2/G5；G3 未运行时 gates["G3"]=false，不制造 passed。每个门禁终态沿共享第1.4节唯一GateEvidence记录kind:gate；G1/G4带原opening/reviewed上下文，不能只保存ReviewFact或Frozen.plan_hash；G0/G2/G5 拒绝产生绑定 commit/tree 的 gate 证据和相应 Hold，不能以请求报错代替可追溯事件。任务验证总证据由 trailers 引用，各 gate Evidence IDs 保存其索引。

FinishEvidence 沿批 4 实际 Evidence schema 增加绑定字段，不另注册一种对象；序列化为注册 type: Evidence，扩展键往返保留。generated_by 对接 OKF generated.by，固定 process:towermd-verifier；不从 HTTP body 接收。fences 按 name 排序且无重复，含主模块与所有 touches；primary epoch 从 module:<module_id> 提取，禁止用 fences.first() 猜主模块。

Run 新增可选 entered_verifying_at/merge_deadline/verification_evidence 和默认零 verification_failures；恢复旧数据缺字段不报错。首次进入 verifying 固定 deadline；同一 Run 重试不重置。计划内容来自 P3 review request 归档的不可变内容与当前 Plan subject；不增加 claim 参数，不从可变工作树或任务路径授权推断计划。高风险无 current Plan 硬阻，普通风险允许 plan_hash=None。

### Task 1：权威 G0 与完整 G2/G5 检查

**Task1开工即接线的既有producer：** M0已有真实task.claimed，不是本计划未来才创建。先按共享契约第2节，ProjectSession::execute/tick/renew_binding、TaskService::execute/expire/reconcile_pending/renew_binding、tasks::execute/expire/renew_binding尾部显式增加同项目outbox并由daemon请求与后台循环传入；release_run/compensate_claim等普通task.released producer同样传入并调用E唯一append_durable，lease专门方法保持原协议、不入outbox。完成真实CLI claim→confirmed重开→删hub/App确认恢复历史测试，记录实际SHA。此依赖未完成不得开始Task2 finish；不能手填Run替代claim。

**Files:** 扩展 E 已交付 task_gates.rs、tests/task_gates.rs；修改 repo.rs、tasks.rs、lib.rs 与主规格 FR-12/FR-112 澄清。

- [ ] **1.1 核对前置提交与真实 API，运行基线。** 在专用工作树记录批 2–5 提交；读实现而非仅读计划。

~~~bash
git status --short
git log -12 --oneline
rg -n 'pub (struct|enum|trait)|pub fn|guards|verify|proof' crates/towermd-core/src/{hub_protocol,project_session,tasks,recovery_git,verifier}.rs
cargo test -p towermd-core --test tasks --test snapshot --test recovery_inputs --test verifier --locked
cargo test -p towermd --test daemon_cli --locked
~~~

预期前置测试通过；缺文件/API 则 blocked，补齐所属前置批，不安装库或退回 LeaseChange。

- [ ] **1.2 复核 E raw-z 回归并写实时守卫红测。** 以下raw-z用例由E0先交付，本Task复用，不重复新增；生产 OID 取 Git 输出，不硬编码 SHA 长度。

~~~rust
use towermd_core::task_gates::{parse_raw_z, GateError};
#[test]
fn raw_z_keeps_deletion_mode_and_non_line_paths() {
    let old = "a".repeat(40); let zero = "0".repeat(40);
    let raw = format!(":100644 000000 {old} {zero} D\0src/a\n b.rs\0");
    let changes = parse_raw_z(raw.as_bytes()).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, b"src/a\n b.rs");
    assert_eq!((changes[0].old_mode, changes[0].new_mode), (0o100644, 0));
    assert!(matches!(parse_raw_z(b":bad\0x\0"),
        Err(GateError::Infrastructure(_))));
}
#[test]
fn raw_z_rejects_truncated_or_rename_format() {
    assert!(parse_raw_z(b":100644 100644 a b M\0src/a").is_err());
    assert!(parse_raw_z(b":100644 100644 a b R100\0old\0new\0").is_err());
}
~~~

- [ ] **1.3 跑红。** cargo test -p towermd-core --test task_gates raw_z --locked；E前置模块/纯parser必须已存在；新增本批实时守卫红测，已有raw-z用例只作回归，不将重复实现或缺import算本批红测。

- [ ] **1.4 实现字节适配与守卫。** 复用 E 已交付 repo::git_bytes_timeout(dir: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>, String>，复用已有有界 Git 子进程，保留 stdout 原 bytes，不经过 decode_output/trim。parser 每两段 NUL 一条，校验五字段、八进制 mode、hash、单字符 A/D/M/T，缺末尾 NUL 拒绝。check 按以下顺序执行：

~~~text
1. 权威Catalog校验Task、Module/touches、Acceptance、显式Decision；依赖仅认merged。
2. run.task/host/canonical worktree/branch均匹配，branch=t/<task>；显式--run也校验。
3. 全部Run.fences与未到期hub leases同name/holder/epoch，覆盖全部声明模块。
4. git status --porcelain=v2 -z --untracked-files=all必须为空；不清理用户文件。
5. git rev-parse --git-path定位MERGE_HEAD/CHERRY_PICK_HEAD/REVERT_HEAD/
   rebase-merge/rebase-apply/sequencer；任一活动操作存在就拒绝。
6. 捕获HEAD^{commit}/HEAD^{tree}；merge-base HEAD authority必须等于authority。
7. git diff --raw -z --full-index --abbrev=0 --no-renames --no-abbrev authority HEAD --；读取每项旧新mode/blob；raw 的完整 OID 由 --no-abbrev 保证。
8. 旧/新mode为120000或160000拒绝；只允许100644/100755与不存在000000。
9. 固定保护区、Catalog protected_paths、旧新blob layer=L0..L2任一命中拒绝；
   删除仍查旧blob，改layer查两边，新增慢层查新blob，非法层字段明确拒绝。
10. 每项必须匹配Task.allowed_paths，且受权威Module/touches.paths上限约束。
11. 重读HEAD/tree、authority与status；变化则拒绝，否则返回完整CheckReport。
~~~

固定保护区包括 docs_dir 的 intents/arch/specs/baselines/state/tower.canvas、.towermd 配置、其他位置 L0–L2 对象。敏感路径/现有 gates 策略继续硬拒绝，不提供危险命令执行入口。非 UTF8 路径保存原 bytes 诊断并拒绝，不能有损转换后授权。

匹配器沿 catalog 支持范围：literal、*、?、整段 **，按路径段 DP；不能 starts_with 放开 src/a2。--no-renames 将移动展开删除+新增，两端都守卫。删除读旧对象，不从已不存在工作树路径读对象；symlink 不跟随，目录链从 Git tree 校验。

- [ ] **1.5 补真实 Git 红绿矩阵。** tests/task_gates.rs 用 tempfile 与现有 repo::git 初始化 main/t/T，写 Config/Module/Acceptance 后提交基底，再提交变更、load Catalog、check。逐例覆盖旧/新层、删除、移动、执行位、symlink/gitlink、换行路径、路径上限、缺 Decision/Acceptance、错 host/branch/run、失效 touches fence、dirty/staged/untracked、merge/rebase/cherry-pick。每例断言拒绝前没有验证进程或总验证 Evidence。

- [ ] **1.6 跑绿并精确提交。**

~~~bash
cargo test -p towermd-core --test task_gates --test tasks --locked
git diff --check
git add crates/towermd-core/src/task_gates.rs crates/towermd-core/src/repo.rs crates/towermd-core/src/tasks.rs crates/towermd-core/src/lib.rs crates/towermd-core/tests/task_gates.rs docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
git diff --cached --stat
git commit -m "feat: guard task commits against authoritative main"
~~~

规格只补 FR-12 Towermd-Epoch=主模块 fence 与 FR-112 固定起点；不改批准边界。有共享文件其他任务改动时按 hunk 精确暂存，记录真实 SHA。

### Task 2：输入冻结、不可变 Evidence 与持久生命周期

**Files:** 新建 finish.rs、tests/finish.rs；修改 tasks.rs、snapshot.rs、project_session.rs、lib.rs。

- [ ] **2.1 核对 P3 核心首批提交，再写实际计时/失败红测。** 先核对Task1现有claim durable接线及真实producer测试SHA，再读取已合入 human_intervention.rs 与 ProjectSession.human，运行 cargo test -p towermd-core --test human_intervention --locked 与 cargo test -p towermd-core --test snapshot --locked，核验首批持久事实/快照测试；缺类型、reader 或实际提交则此任务 blocked，不写伪字段或临时自动通过适配器。 先在 tests/finish.rs 写下面测试（导入列出的生产类型/函数），运行得到缺接口的红，再在 finish.rs 实现所示最小逻辑；真实事件行为红测由2.5覆盖：

~~~rust
pub enum FailureAction { Running, GatefailedHold }
pub fn failure_action(prior: u32) -> (u32, FailureAction) {
    let next = prior.saturating_add(1);
    (next, if next >= 3 { FailureAction::GatefailedHold } else { FailureAction::Running })
}
pub fn fixed_deadline(entered_at: u64) -> Result<u64, String> {
    entered_at.checked_add(60 * 60 * 1000).ok_or("deadline overflow".into())
}
#[test]
fn third_failure_is_hold_and_deadline_has_one_origin() {
    use towermd_core::finish::{failure_action, fixed_deadline, FailureAction};
    assert!(matches!(failure_action(0), (1, FailureAction::Running)));
    assert!(matches!(failure_action(1), (2, FailureAction::Running)));
    assert!(matches!(failure_action(2), (3, FailureAction::GatefailedHold)));
    assert_eq!(fixed_deadline(123).unwrap(), 3_600_123);
    assert!(fixed_deadline(u64::MAX).is_err());
}
~~~

- [ ] **2.2 跑红。** cargo test -p towermd-core --test finish third_failure --locked，预期失败；纯函数绿后，下面实际 hub 生命周期测试必须另留红绿证据。

- [ ] **2.3 实现 freeze 与高风险评审读取。** guard 通过后冻结 commit/tree/base/authority/baseline、主 Module 全部 verify、计划内容 hash与全部 fences；commands_hash只调用共享commands_hash覆盖完整结构化命令。只能读权威 Module.verify，禁止任务分支 manifest 发现命令替代。已批准字符串命令转批 4 VerificationCommand { cwd: ".", argv: ["/bin/sh", "-c", command] }，保留批准语义；若批 4 实际有 typed argv schema 则复用其解析器，不另发明 DSL。

空 verify 不调用执行器，生成 NoTests 终态 Evidence 和硬失败 G3；工作分支有 scripts 也不能救活空权威配置。修复指引为 S7/后续 App 提案批准 L1 命令并补测试，再执行同一 G3。

唯一共享评审类型由E先定义、P3重导出并实现事实producer（均尚未实现）；本批只消费，不另建 ReviewFact：
~~~rust
// E定义，P3 human_intervention.rs重导出；本批消费相同类型。
use crate::human_intervention::{ReviewSubject, ReviewBinding, ReviewVerdict, ReviewFact};
// HumanState::current_plan_subject(&self, run: &str) -> Result<ReviewSubject, String>
// HumanState::require_human_review(&self, binding: &ReviewBinding)
//     -> Result<&ReviewFact, String>
// ProjectSession.pub human: HumanState；仅App human事件产生有效事实。
~~~

主模块或 touches 任一 high 则 G1/G4 都硬：freeze 从 session.human.current_plan_subject 取得该 Run 当前不可变 Plan hash；require_finish_reviews 从权威 catalog 推导风险，按 gate 参数分别构造相同 run/task/baseline/fences 的 G1 Plan{sha256}、G4 Tree{tree} binding，调用 session.human.require_human_review。只接受 Gate::G1/G4，其他值返回明确错误。G1在 freeze 后、finish.started 前调用；G4在验证通过后、trailers及main合并认证前硬检查。该 reader 必须精确匹配、未撤销、actor 为非空 human:、verdict Approved；拒绝 CLI 普通答案、自报评审、旧计划/tree/基线/fences。缺 P3 reader 或必要事实返回 ReviewBlocked 与 review Hold；不能编写返回 Ok 的临时适配器。

G1 按主规格在运行中写完计划后由已有 towermd review request 提交：CLI 从显式文件读取内容传 body，daemon 重算 SHA256 并归档不可变内容、更新该 Run 当前 Plan subject；不经 shell、不写慢层。重提计划替换 current subject，旧 Approved 即失效；不能读未提交可变文件来偷偷改变 subject。P2 只消费当前 subject 与精确事实；G4 对冻结验证 tree 请求。可附 sourceCommit/path 溯源，但不作 claim 前提。

- [ ] **2.4 实现原子业务事件与不可变 Evidence。** 定义 finish.started data={task,run,input,review_context}、finish.completed data={task,run,evidence,report,hold,review_context}；started/completed保同一G1 ReviewContext，record_finish用P3真实currentPlan重核；晚G4不阻塞passed completed，按共享第1.4节独立GateEvidence满足，原接受seq/已知laterPlan规则与canonical context均只用同一consumer。完整 report、Evidence.log 的原completed eid/report hash/日志locator及commands hash一律采用共享契约第1.2节。完成eid在make_evidence前预分配，record_finish核report与Evidence绑定；注册对象校验全过才一起 apply。NewEvent.guards=input.fences，stale 不更改业务。claim/started/completed一律用共享durable入口；confirmed失败只限制本次成功/JobCompleted，Hub已耐久接受仍照常sync/广播/快照，status可见Run。freeze/finish成功前协调必需原claim/started及完成证明（confirmed或已核真实archive），否则uncertain；completed接受后confirm失败保Recording，不重跑verifier。ID 预分配属于 Job，不是 pending Evidence；同ID同内容幂等、不同内容拒绝；业务事件校验保持原子apply，不能把confirmed缺失当投影隔离条件。

~~~text
claim: 冻结完整Task/Run/全module+touches fences -> pending fsync -> hub.append
       -> 全字段核原Event -> confirmed fsync -> 返回本次claim成功（Hubstate照常sync/广播/快照）
accept: 协调原claim耐久证明(confirmed或已核真实archive) -> guard -> freeze -> require_finish_reviews(G1) -> durable job input
        -> finish.started pending fsync -> hub.append -> 核原Event -> confirmed fsync -> enqueue/本次成功
        原started依赖证明未协调不能成功；Hub投影照常sync，不停顿全前缀
result: seal verifier logs -> make_evidence -> finish.completed pending fsync(original fixed eid)
        -> hub.append(guards=all fences) -> 核原Event -> confirmed fsync -> 补齐原claim/started/completed依赖证明 -> mark job Completed
        completed已接受但confirm失败=JobRecording/uncertain，重开仅补原Event、不重跑verifier
passed: Task/Run=verifying；绝不merged；G4缺事实生成review Hold并阻止trailers/merge
        不改写已封存G3 Evidence；后续有效G4由独立evidence.recorded kind:gate持久原opening/reviewed上下文并绑定原completed eid，再等待main proof
failed/no_tests: count+=1；第1/2回running；第3进入held并生成gatefailed Hold
hold ID/eid: sha256(project + run + "G3" + threshold=3)，重复wait/replay不增加次数
~~~

failure 消费键为 Evidence ID，已消费集合持久在 Run 随快照重建。Hold 绑定 Task/Run/基线/tree/fences，由 P3 处理；P2 不增 approve/retry-human 命令。confirmed缺失但Hub已有completed时JobRecording/uncertain，持续同ID重试不可success，解除只补原Event；基础设施错误没有可封存日志时 Job=Interrupted，不能造 passed，诊断也必须持久化。G0/G2/G5 拒绝不错误累计 G3 执行失败次数。

- [ ] **2.5 补重放红绿。** 真实 ProjectSession+Hub 追加三个不同失败 Evidence，再重复第三条原 eid；断言 failures=3、一个 Hold、Task=held。快照后销毁 session 重开仍一致；篡改同 ID result 拒绝。passed 后仍 verifying，普通 task.state=merged 继续拒绝。高风险补具体测试：第一次 review request 归档计划A并获App fixture批准后，G1通过；再次请求计划B但不批准，finish硬blocked；批准A不能满足B；缺currentPlan、普通CLI回答或仅G4事实均硬blocked。G1有效但缺G4时允许执行验证，G3证据passed后仍只verifying且trailers硬blocked；新增当前tree有效G4后才允许trailers。空命令、混合命令失败、第3次 no_tests 均硬失败；重试/amend/重启 deadline 等于首次 entered_at+3600000。

- [ ] **2.6 跑绿并提交。**

~~~bash
cargo test -p towermd-core --test finish --test snapshot --test tasks --locked
git diff --check
git add crates/towermd-core/src/finish.rs crates/towermd-core/src/tasks.rs crates/towermd-core/src/snapshot.rs crates/towermd-core/src/project_session.rs crates/towermd-core/src/lib.rs crates/towermd-core/tests/finish.rs
git diff --cached --stat
git commit -m "feat: persist immutable finish evidence and gate outcomes"
~~~

### Task 3：真实后台验证与有界 finish 等待

**Files:** 新建 crates/towermd/src/finish_jobs.rs；修改 daemon.rs、main.rs、daemon_cli.rs。

- [ ] **3.1 扩展现有 Fixture 写真实 CLI 红测。** Fixture::verified() 的完整新增实现如下，放在 daemon_cli.rs；只改隔离 fixture main，不代表人批准：

~~~rust
impl Fixture {
    fn verified() -> Self {
        let f = Self::new();
        let root = f.root.path();
        fs::create_dir_all(root.join("src/a")).unwrap();
        fs::write(root.join("src/a/file.rs"), "pub fn a() {}\n").unwrap();
        fs::write(root.join("docs/tower/arch/A.md"),
            "---\ntype: Module\nid: A\npaths: [src/a/**]\nverify: ['test -f src/a/file.rs']\n---\n").unwrap();
        f.git(&["add", "src/a/file.rs", "docs/tower/arch/A.md"]);
        f.git(&["commit", "-qm", "fixture verification command"]);
        f
    }
}
#[test]
fn finish_is_async_and_wait_reuses_preallocated_id() {
    let f = Fixture::verified();
    f.add("T-1", "A"); let wt = f.worktree("T-1");
    f.json(&wt, &["task", "claim", "T-1"]);
    let first = f.json(&wt, &["finish"]);
    let id = first["evidence_id"].as_str().unwrap();
    assert!(first["job_state"] == "queued" || first["job_state"] == "running");
    assert!(first.get("result").is_none());
    let done = f.json(&wt, &["finish", "--wait", id]);
    assert_eq!(done["evidence_id"], id);
    assert_eq!(done["evidence"]["result"], "passed");
    let again = f.json(&wt, &["finish", "--wait", id]);
    assert_eq!(done["evidence"], again["evidence"]);
    assert_eq!(done["run_state"], "verifying");
}
~~~

- [ ] **3.2 跑红。** cargo test -p towermd --test daemon_cli finish_is_async --locked，预期 finish 不存在；实现真实路由，不 mock CLI 输出。

- [ ] **3.3 定义传输/等待契约。** 复用实例校验、project/session 注册，不新开 daemon。

~~~rust
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishRequest {
    pub directory: std::path::PathBuf, pub run: Option<String>,
    pub wait: Option<String>, pub report: Option<String>,
}
// POST /check只接受{directory,run}；POST /finish接受FinishRequest。
// report是线索，不影响passed；wait/report互斥，不接收actor/commands/fences。
pub const WAIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(90);
pub async fn wait_job(notify: &tokio::sync::Notify,
    read: impl Fn() -> Result<serde_json::Value, String>,
    budget: std::time::Duration) -> Result<serde_json::Value, String>;
~~~

CLI 新增 Check、Finish { wait: Option<String>, report: Option<PathBuf> }；--run 沿全局参数。等待语法 finish --wait <evidence-id>，不是布尔无限等待。无 wait 返回 ID，queued/running 不带 passed。终态 failed/no_tests 非零退出但仍输出解析 JSON；未完成 wait 返回 Job 状态与重复调用指引。

report 文件在 CLI 只读，大小设内部上限 1MiB；HTTP 只传其文本，不让 daemon 跟随用户提供任意路径。它不设置完成状态，不影响命令、路径或 gates。

- [ ] **3.4 实现最小后台队列。** 每项目 FinishJobs 用 ID map+Notify；同 Run 相同冻结输入 active job 返回原 ID，不双跑；其他 Run 可同时验证。HTTP 锁只包 sync/check/freeze/append，随后释放；spawn_blocking 调批 4 verify(&VerificationRequest)，使用冻结 revision/commands、cache log root、内部 timeout/cancel 标记。环境/网络/worktree/进程组均复用批 4，不复制执行器。

wait_job 先注册 notified future 再读状态；终态立即返回；tokio::time::timeout(budget, notified) 到期重读一次返回。生产传 WAIT_LIMIT，测试传 10ms；HTTP client timeout 略多于90秒仅为传输余量。重复 wait 不 enqueue；内部执行 timeout 沿批 4 默认，不接受 skip/timeout 参数。

- [ ] **3.5 补并发/拒绝红绿。** 单测10ms返回 running、通知提前唤醒、完成无丢通知；真实 HTTP 启两个模块验证，ID/log/tree独立，长验证期间 status/其他 claim 仍响应。命令写 cache 计数，重复 wait 一次执行；dirty/G2 拒绝计数不存在。未知ID/错项目/错host/branch/显式跨worktree run 拒绝并列本机活动 Run。

- [ ] **3.6 跑绿并提交。**

~~~bash
cargo test -p towermd --test daemon_cli finish_ --locked
cargo test -p towermd finish_jobs --locked
git diff --check
git add crates/towermd/src/finish_jobs.rs crates/towermd/src/daemon.rs crates/towermd/src/main.rs crates/towermd/tests/daemon_cli.rs
git diff --cached --stat
git commit -m "feat: run task verification asynchronously with bounded waits"
~~~

### Task 4：trailers 与唯一 main 合并证明

**Files:** 修改批 3 recovery_git.rs、project_session.rs、recovery_inputs.rs；daemon main_refresh.rs/daemon.rs/main.rs/daemon_cli.rs。

- [ ] **4.1 冻结唯一证明器边界。** 批3若签名不同按此职责映射并统一所有调用点；不另建 proof 文件。

~~~rust
// 以下类型/签名由E recovery_git按共享契约实现，本批直接复用。
use crate::recovery_git::{TaskTrailers, TrailerContext, MergeProofInput,
    VerifiedMerge, parse_trailer_context, prove_merge, render_task_trailers};
~~~

唯一证明器构造 VerifiedMerge；正常 main observer、P5、恢复扫描、outbox 校验全部调用 prove_merge。VerifiedRecoveryHistory 只由批3统一已核 archive/tail/confirmed reader 构造，字段私有；raw StateProjection/任意 JSON/verified:true 无构造能力。daemon 只传 commit 候选，不能让客户端自报 merged。内部模块可只导出测试入口给现有 recovery_inputs，统一用同一个实现，不能为测试复制算法。

- [ ] **4.2 写上下文红测。**

~~~rust
#[test]
fn task_epoch_does_not_require_snapshot_context() {
    use towermd_core::recovery_git::{parse_trailer_context, TrailerContext};
    let message = "done\n\nTowermd-Task: T-1\nTowermd-Run: R-1\nTowermd-Module: A\nTowermd-Evidence: E-1\nTowermd-Epoch: 7\n";
    assert!(matches!(parse_trailer_context(message), Ok(TrailerContext::Task(_))));
    assert!(parse_trailer_context("x\n\nTowermd-Epoch: 7\n").is_err());
    assert!(matches!(parse_trailer_context("x\n\nTowermd-Snapshot: 3\nTowermd-Epoch: 7\n"),
        Ok(TrailerContext::Snapshot { seq: 3, epoch: 7 })));
}
~~~

- [ ] **4.3 跑红。** cargo test -p towermd-core --test recovery_inputs task_epoch --locked；预期旧 Task+Epoch parser 拒绝或没有统一接口。

- [ ] **4.4 实现 trailers/proof。** CLI trailers <task> 必须绑定当前本地 Run、有效全部 fences、成功 Evidence与有效G4；输出 Task/Run/Module/Evidence/Epoch 五行。主 epoch 显式查主模块；不 amend/merge/push，不署 human。HEAD tree非共享G5允许的state/canvas等价变化时拒绝；纯提交信息amend/squash仍有效。输出前沿共享第1.5节耐久记录终态G5（原completed_eid、实际HEAD parents及fresh base/authority），state-only rebase后重新trailers但不重跑verifier；非快进合并的实际main接入提交也必须带FR-12五行。

~~~text
prove_merge从权威main可达commit读最后trailer block；重复/缺失/非法已知键拒绝。
Task/Run/Module/Evidence互引一致；必须具备原非stale已接受task.claimed完整Task/Run/全module+touches fences、原finish.started完整FrozenFinish、原verifier finish.completed与必要ReviewFact；仅已核archive/confirmed授信，提交对象只补字节，raw pending/孤立passed不授信。
核验verifier署名、passed、真实tree、全fences、主epoch、命令hash、基线与人工评审。
Evidence.tree与candidate.tree相同接受；仅state/与tower.canvas差异允许G5等价。
旧base到新main链只有state/canvas提交才可复用，任务须实际变基到新main；candidate_base取共享第1.5节已核原终态G5的真实base，实际main接入边/捕获parents和tree均重核，再交唯一check_g5，不猜直接父/最早非state前缀，不接受客户端自报。
tree/基底链比较复用E已交付task_gates::check_g5与raw/no-renames守卫；G5等价绝不豁免任务自身G2禁写state/canvas。
返回commit+"merge.done"确定性eid与完整Task/Run/Evidence及共享契约原claim/freeze/pass/review_contexts/integration_gate proof上下文（commit统一为实际main接入commit），失败生成确定性Hold；二次仅snapshot/archive+Git可复核。
~~~

证据通过后 main 代码/配置推进必须重新变基与 finish；不能用最终 tree 巧合相等绕过基底变化。tree 等价只改 SHA 的情形仍校验完整上下文与全部硬门禁。

G3 Evidence 的 gates 只描述其冻结时刻已实际执行的结果；G4未批准时为false/未满足，并保持不可变。App批准不得把该字段改true。唯一proof不能简单 all(evidence.gates.values())：它显式验证G0/G2/G3/G5对应的证据与绑定，再读取当前精确G4 ReviewFact及独立 kind: gate Evidence引用；后者满足人工闸门，不改写G3 Evidence。G4缺失、旧tree/currentPlan/baseline/fences不符，trailers与main认证持续硬阻。

prove_merge只核既成历史事实，不承担实时授权；正常integrator在唯一finish/观察流程先核当前完整fences、固定deadline、G0–G5与最新authority，再调用同一proof，成功后才处理到期释放。不得增加historical bool或公开历史权限入口。恢复按共享契约已核原非stale claim/freeze/pass上下文判断原 fences，不要求保存全部renew重建TTL；不能拿新恢复 epoch 否决已合法合并的历史，也不能接受缺原历史证明的伪造 epoch。outbox 自带 task.state=merged 不可信。高风险证明读取归档中的有效 ReviewFact，沿 P3 相同精确绑定规则。

- [ ] **4.5 补三入口共用证明器矩阵。** 同一案例分别正常观察/恢复/outbox，结论与eid一致：同tree amend/squash、纯state/canvas rebase、代码main推进、缺Evidence、failed/no_tests、错署名/task/run/module/epoch/tree、失效touches fence、重复键、正文假trailer。补共享第6节confirmed失败reopen/status可见而请求uncertain、持续同ID失败/仅补原Event、archiveDuplicate、claim失败直接finish需修依赖proof、completed接受失败JobRecording不重跑、后台taskexpire/pendingclaim确认实测，以及同eid异body/缺claim/错touches/rawpending/新stale外层+原proof二次无旧outbox案例。Task+Epoch不误报Snapshot；Task+Snapshot上下文冲突拒绝；未知Towermd键沿批3聚合诊断，不阻止独立合法事实。重复观察只一条merge.done，Task merged/Run ended，依赖后续G0可领取。

- [ ] **4.6 跑绿并提交。**

~~~bash
cargo test -p towermd-core --test recovery_inputs --test finish --test tasks --locked
cargo test -p towermd --test daemon_cli --locked
git diff --check
git add crates/towermd-core/src/recovery_git.rs crates/towermd-core/src/project_session.rs crates/towermd-core/tests/recovery_inputs.rs crates/towermd/src/main_refresh.rs crates/towermd/src/daemon.rs crates/towermd/src/main.rs crates/towermd/tests/daemon_cli.rs
git diff --cached --stat
git commit -m "feat: certify task merges through the shared Git evidence proof"
~~~

### Task 5：续租、固定 deadline 与崩溃重建

**Files:** 修改 finish_jobs.rs、finish.rs、tasks.rs、project_session.rs、snapshot.rs 与对应测试。

- [ ] **5.1 写deadline/续租红测。** 用daemon现有注入clock，按首次entered_at、+14min、+59min、+60min驱动，不sleep60min。超过15min仍执行的job由daemon续租；通过/wait/重启/纯快照rebase不延长deadline。先放合法合并到main，再deadline tick，预期merged而非ready；未合并则ready。

新增 finish.rs 的纯决策 API，定时器必须消费同一个结果；先写测试，不能用这个单测替代上述真实 daemon 验收：
~~~rust
pub enum ExpiryAction { Keep, Merged, Release }
pub fn expiry_action(now: u64, deadline: u64, certified_main: bool) -> ExpiryAction {
    if certified_main { ExpiryAction::Merged }
    else if now >= deadline { ExpiryAction::Release } else { ExpiryAction::Keep }
}
#[test]
fn deadline_checks_certified_merge_before_releasing() {
    use towermd_core::finish::{expiry_action, ExpiryAction};
    assert!(matches!(expiry_action(59, 60, false), ExpiryAction::Keep));
    assert!(matches!(expiry_action(60, 60, false), ExpiryAction::Release));
    assert!(matches!(expiry_action(60, 60, true), ExpiryAction::Merged));
}
~~~

- [ ] **5.2 跑红。** cargo test -p towermd-core --test finish deadline --locked；cargo test -p towermd --test daemon_cli finish_lease --locked。预期旧expire仅识别running或先释放失败。

- [ ] **5.3 实现tick顺序与释放。** daemon内部每分钟处理活跃验证，调用协议RenewLease{host,fences,ttl_ms:900000}后sync；不能重新acquire旧Run。

~~~text
tick: refresh可用权威main -> shared prove_merge -> apply合法合并
      -> 未merged且now>=fixed deadline：取消验证、释放仍同holder/epoch的fences
         Task=ready、Run=expired，旧job结果不能再生效
      -> 未到期queued/running/recording：全fences续租
      -> 常规15min活动TTL处理
~~~

deadline到且main不可刷新则记录authority_unavailable，依固定期限释放未确认合并的运行，不能延期限或声称已证明无合并；恢复后合法历史proof仍可补merged。逐个核对lease，不能释放别的Run已拿到的新epoch。验证不再执行后不后台无限续租；passed等待合并仍按任意CLI调用续租与60min上限处理。

- [ ] **5.4 实现恢复协调。** Job文件只是派生调度，唯一输入/起始计时在finish.started，最终Evidence在hub/快照/outbox。启动照常投影Hub真实state，再按outbox协调本次durability；原eid用于producer不确定结果协调，recovery内部replay不写第二confirmed。Evidence已被Hub接受但本机confirm失败则JobRecording/uncertain，补原Event确认不重跑verifier；必要claim/started/completed依赖可从confirmed或已核真实archive证明，满足本次成功条件后才JobCompleted。执行中未知退出Job=Interrupted，重验用新ID与原Run deadline，不将旧ID failed改passed。

批4正常取消/进程组清理直接复用；异常重启不据旧PID盲kill、不全局prune、不删除占用worktree。不能证明进程身份/已退出时留cleanup诊断与目录，不生成成功Evidence。删除job cache后从started/completed快照重建；孤立缓存passed不能恢复进度。

- [ ] **5.5 补崩溃与fence绿测。** 日志封存后/append前、append后/job completed前、第三失败后/快照前分别终止重启，Evidence数量/内容、Hold ID、计数、deadline不变。删除cache/索引/hub后走批3App确认测试客户端恢复，同proof与outbox重建；此客户端不是真人验收。旧deadline不能释放新Run epoch。

- [ ] **5.6 跑绿并精确提交。**

~~~bash
cargo test -p towermd-core --test finish --test snapshot --test recovery_inputs --locked
cargo test -p towermd --test daemon_cli --locked
git diff --check
git add crates/towermd/src/finish_jobs.rs crates/towermd-core/src/finish.rs crates/towermd-core/src/tasks.rs crates/towermd-core/src/project_session.rs crates/towermd-core/src/snapshot.rs crates/towermd-core/tests/finish.rs crates/towermd/tests/daemon_cli.rs
git diff --cached --stat
git commit -m "fix: preserve finish deadlines and fences across daemon recovery"
~~~

### Task 6：产品路径验收、文档与交接

**Files:** daemon_cli.rs、README.md、双语CHANGELOG、tasks/todo.md；只修验收发现且属于本批的问题。

- [ ] **6.1 写无证据不完成的端到端红测。** 用Fixture::verified领取、提交伪造Evidence ID五行trailer、快进main；真实status/refresh断言未merged、有确定性gatefailed Hold、重复不新增。另finish passed→trailers→amend→快进main，只有proof让merged；不直接append merge.done代替产品链路。

下面是正常路径具体测试；Fixture::git_in 在本步骤定义，所有命令只操作临时仓库：
~~~rust
impl Fixture {
    fn git_in(&self, directory: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                   "-c", "commit.gpgsign=false"])
            .args(args).current_dir(directory)
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
}
#[test]
fn finish_main_requires_proof_then_records_merge_once() {
    let f = Fixture::verified();
    f.add("T-1", "A"); let wt = f.worktree("T-1");
    f.json(&wt, &["task", "claim", "T-1"]);
    let job = f.json(&wt, &["finish"]);
    let id = job["evidence_id"].as_str().unwrap();
    f.json(&wt, &["finish", "--wait", id]);
    let trailers = f.success(&wt, &["trailers", "T-1"]);
    let message = format!("task result\n\n{}", String::from_utf8(trailers.stdout).unwrap());
    f.git_in(&wt, &["commit", "--allow-empty", "-m", &message]);
    f.git(&["merge", "--ff-only", "t/T-1"]);
    for _ in 0..2 {
        let state = f.json(f.root.path(), &["status"]);
        let task = state["tasks"].as_array().unwrap().iter()
            .find(|task| task["id"] == "T-1").unwrap();
        assert_eq!(task["state"], "merged");
    }
}
~~~

- [ ] **6.2 跑红与最小接线。** cargo test -p towermd --test daemon_cli finish_main --locked；若未接主循环，仅补main_refresh→core proof→session append/sync。无M3队列或完整合并前集成器。

- [ ] **6.3 本机真实CLI验收。** cargo build -p towermd --locked后，对冻结SHA的Tower.md临时clone建两个worktree，用真实CLI完成普通任务一次；登记binary源SHA、run、commands hash、tree、完整fences、Evidence/log hash、trailers、main commit、最终projection。再测dirty/no_tests/错tree/超时，记录退出码与日志。原仓库不动，不要求新账号或读取密钥。

- [ ] **6.4 最终必要检查。**

~~~bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
rg -n 'LeaseChange|env_clear|skip.gate|auto.approve|human:' crates/towermd/src/finish_jobs.rs crates/towermd-core/src/finish.rs
~~~

格式/测试/clippy/diff全过；最后命中仅可为拒绝校验或诊断，不能出现绕过。fixture、真实CLI演练、未运行、未提交、P3/P6/P7待验收分别登记。

- [ ] **6.5 同步文档。** README写check/finish/finish --wait <id>/trailers、no_tests入口、90秒/60min、人工high-risk blocked、process署名与passed仍verifying；双语Unreleased各写用户可感知的任务验证/合并认证。todo写六个真实提交SHA与验证输出；不把复选框当验收。技能由P7交付，不改全局harness配置。

- [ ] **6.6 精确提交与交接。**

~~~bash
git status --short
git diff --stat
git add crates/towermd/tests/daemon_cli.rs README.md CHANGELOG.md CHANGELOG.zh-CN.md tasks/todo.md
git diff --cached --stat
git commit -m "test: verify task evidence through the daemon and CLI"
~~~

只暂存已验证本批文件/hunk，不git add .，不提交其他批改动。交接P3必要评审/Hold、P4权威命令批准、P6展示/确认、P7真实十任务；本批完成不代表App已发布。

## 自审与冻结边界映射

已按writing-plans自审规格范围、占位符和类型一致性；所有新接口有任务责任。前置真实接口必须在1.1重核；没有实现/测试已通过声明。

| 已冻结要求 | 落点与验收 |
| --- | --- |
| G0权威Module/Acceptance/Decision、依赖、全模块租约 | 1.4–1.5；claim完整G0 |
| G2先验、raw-z/no-renames、旧新层/删除/模式/symlink | 1.2–1.5、3.5；拒绝时执行计数为零 |
| dirty/staged/untracked/操作中、错run/host/branch、最新main | 1.4–1.5、3.5；显式run不豁免绑定 |
| commit/tree/base/权威命令/hash/完整fences冻结 | 共享契约、2.3、3.4 |
| 共用detached verifier、环境/网络/超时/日志/署名 | 2.4、3.4、5.4；只调批4verify |
| 预分配ID、queued/running非假Evidence、异步/90秒wait | 2.4、3.1–3.5 |
| passed仍verifying、唯一main proof、伪done不完成 | 4.1–4.5、6.1 |
| G5只state/canvas等价，amend/squash且不豁免G2 | 4.4–4.5；代码main推进重验 |
| 第1/2失败回running、第3持久gatefailed Hold、去重 | 2.4–2.5、5.5 |
| entered_at+60min固定、先认证main、只释放本run fences | 2.1、2.5、5.1–5.3 |
| 验证中daemon续租、15min TTL、重启不重置 | 5.1–5.5 |
| no_tests硬阻G3、S7/后续人工批准命令并补测试 | 2.3、2.5；P4/P6依赖明确 |
| high风险人工G1/G4，planHash/tree/baseline/fences绑定 | 开工顺序、2.1/2.3、4.4；先P3核心真实提交再消费 |
| FR12补主模块Epoch，Task/Evidence/Epoch不误判Snapshot | 1.6、4.1–4.5 |
| 正常观察/恢复/outbox共享批3唯一proof | 4.1、4.4–4.5、5.5 |
| CLI只process:towermd-cli，无human/批准入口 | 2.4、3.3、4.4、6.4 |
| 闭环编译、红绿命令、精确提交、fixture/真实验收区分 | 六任务各自提交；6.3真实CLI；P7十任务另验 |
| 不新增依赖、不读密钥、P3/P6人工事实缺失阻塞 | 开工边界、2.3、6.3–6.6 |

本计划保存状态为 **未提交**；尚未开始P2实现。执行前由主代理安排规格与质量审查，评审不能替代P3/P6真人事实或P7真实harness验收。
