---
type: Plan
title: E/P2/P3/P4 共享恢复证明契约 v1
description: 冻结 confirmed 回执、历史证明输入、任务合并与基线出版的唯一证明边界及最小持久格式。
tags: [towermd, plan, recovery, proof, M0, M1]
generated: { by: codex/gpt-6, at: 2026-10-17T00:00:00Z }
sources:
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-3-snapshot-recovery.md
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-4-verifier-drafts.md
  - resource: docs/superpowers/plans/2026-10-17-m1-task-evidence.md
  - resource: docs/superpowers/plans/2026-10-17-m1-human-intervention.md
  - resource: docs/superpowers/plans/2026-10-17-m1-baseline-approval.md
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: ../note.md/docs/okf-v0.2-format-constraints.md
status: draft
---

# E/P2/P3/P4 共享恢复证明契约 v1

本文是批 3.3 E 开工前的共享实施契约，**尚未实现，也未运行本文预定的源码测试**；文档提交及独立复审记录见 `tasks/todo.md`。E/P2/P3/P4 以本文定义证明输入与持久顺序，各自计划仍决定产品功能和实施依赖。本文不构成真人批准、现有 API 或已发布功能的证据。

## 1. 边界与实施顺序

沿主规格 FR-12/93/99/111 和 5.4：v0.1 防流程误用，不防同机同用户恶意改写全部文件。不引入签名、账号、认证、新依赖或新注册对象；`confirmed` 是 core 私有持久入口保存的真实 hub 回执，不是客户端的可信声明。HTTP/CLI 不接收 confirmed JSON，不接受任意 `verified:true`、raw projection 或客户端提交的证明句柄。

1. E 先实现 outbox 的 pending/confirmed 双态、统一 core durable 组合入口、校验 reader 与私有 `VerifiedRecoveryHistory`，再实现唯一任务/出版证明消费边界及负例。
2. 现有 M0 已有真实 task.claimed producer（ProjectSession::execute→TaskService::execute→HubProtocol）；它尚未注入 outbox。P2 Task1 必须先给此既有路径接入 E durable 入口并交付真实CLI端到端证据，Task2 才能启动 finish producer 与完整冻结上下文；P3/P4 接入同一 durable 入口。E 早于这些 producer 时只能用明确标注的**契约历史夹具**验证消费者，不声称真人或 daemon finish/批准路径已可达。
3. 缺少 producer、未知 schema 或证据链不完整时留诊断，Task 不 merged、标签不激活基线。不得为过 E 夹具添加伪 verifier、批准 API 或临时自动通过分支。
4. E/P2/P5/恢复/outbox 必须调用同一 `prove_merge`；P4 normal/E recovery 必须调用同一 `VerifiedPublication` builder 和事件 builder。

### 1.1 实施所有权与无环前置

2026-10-17 对冻结源码 `7b13abd9d3a79c95e5d4c3a669b82ebce6ad3e91` 的只读核对：已有真实 claim、双源 snapshot/archive reader、HubProtocol 与 derived_eid；尚无本文的 outbox、证明器、finish/review/publication DTO、verifier 或 G5 守卫。以下均为待实现契约，不能把后续计划当可编译源码。

| 模块 | E 先交付的唯一责任 | 后续复用 |
| --- | --- | --- |
| `recovery_contract.rs` | 下节 DTO、canonical JSON、commands/report 摘要、出版冻结摘要 | 批4 verifier、P2 finish、P3 review、P4 draft/review 重导出同一类型/函数，不复制定义 |
| `recovery_outbox.rs` | 双态信封、context、reader/digest、原回执核验、append_durable | 各 producer 显式注入同项目 outbox |
| `task_gates.rs` | 纯 raw-z 解析、真实 Git tree/基底链/G5 比较；不执行命令、不取得租约 | P2 Task1 在同模块增加实时 check/G0/G2；P3/P4 复用 |
| `recovery_git.rs` | 私有历史构造、trailer parser、prove_merge/prove_publication、canonical event builders | normal observer/P5/恢复/outbox 调同一 consumer |

DTO 仅描述持久事实，不是批准凭据。E 不导入未来 `finish/human_intervention/baseline_draft/baseline_review/verifier` 模块；反向由这些模块 `pub use`。批4 `VerificationRequest/verify`、P2 Job/执行编排、P3 HumanState/AppGate、P4 DraftEdit/编辑与 writer 行为仍归原计划，不提前实现。P4 的编辑命令 VerifyCommand 沿原 typed 请求转换为共享 VerificationCommand，不改变 argv/cwd 原值，不引入第二份 commands hash。

E 分为两个可独立编译验证的闭环：**E0** 交 DTO、outbox/helper、纯 raw/G5/hash 与负例；**E1** 在 E0 实际 SHA 上交统一已核历史、canonical merge/publication consumer、业务投影与 staging。E0 不导出可从裸数据构造的 trusted history/proof，未有 consumer 不开放恢复旁路。两个闭环分别由新鲜实施者执行，完成后各走独立 SPEC 审查→quality 审查→必要验证→精确提交；E1 不在 E0 未通过时开始。具体步骤映射见 E 计划，产品 producer 与真人验收仍未交付。

`task_gates.rs` 纯基础类型也由E先实现，P2只扩展同模块实时行为；唯一签名如下（不新增可配置路径或门禁开关）：

```rust
pub enum Gate { G0, G2, G3, G5, G1, G4 }
pub enum GateError {
    MissingContext(String), Dirty, OperationInProgress(String), WrongBinding,
    StaleFence, NotLatestMain, Protected(Vec<u8>), OutsideTask(Vec<u8>),
    UnsupportedMode(Vec<u8>), InvalidObject(Vec<u8>), NoTests,
    ReviewBlocked(Gate), ProofRejected(String), Infrastructure(String),
}
pub struct RawChange {
    pub old_mode: u32, pub new_mode: u32, pub old_oid: String,
    pub new_oid: String, pub status: u8, pub path: Vec<u8>,
}
pub fn parse_raw_z(raw: &[u8]) -> Result<Vec<RawChange>, GateError>;
pub fn check_g5(catalog: &Catalog, frozen: &FrozenFinish,
    candidate: &str, candidate_base: &str) -> Result<(), GateError>;
// repo.rs；复用已有有界子进程，stdout不decode/trim。
pub fn git_bytes_timeout(dir: &std::path::Path, args: &[&str],
    timeout: std::time::Duration) -> Result<Vec<u8>, String>;
```

### 1.2 唯一共享 DTO 与摘要签名

以下定义归 `recovery_contract.rs` 的 `verification/finish/task_review/baseline` 命名空间，均派生 Clone/Debug/Serialize/Deserialize/PartialEq；单位枚举 snake_case，ReviewSubject 用 `#[serde(tag="kind", rename_all="snake_case")]`。固定机器 envelope 校验必需字段；Task/Run/Evidence 原对象未知扩展键继续原样保留。它们都不是新注册对象或 HTTP 请求类型。

```rust
// recovery_contract::verification；verifier.rs 重导出这些数据类型。
pub struct VerificationCommand { pub cwd: std::path::PathBuf, pub argv: Vec<String> }
pub struct VerificationReport {
    pub commit: String, pub tree: String, pub executions: Vec<CommandExecution>,
}
pub struct CommandExecution {
    pub command: VerificationCommand, pub result: String, pub exit_code: Option<i32>,
    pub timed_out: bool, pub stdout_log: String, pub stderr_log: String,
    pub stdout_sha256: String, pub stderr_sha256: String, pub diagnostic: Option<String>,
}
pub fn commands_hash(commands: &[VerificationCommand]) -> Result<String, String>;
pub fn report_hash(report: &VerificationReport) -> Result<String, String>;

// recovery_contract::finish；finish.rs 重导出，不能改为另一个 wire schema。
pub enum EvidenceResult { Passed, Failed, NoTests }
pub struct FrozenFinish {
    pub evidence_id: String, pub project_id: String, pub host: String,
    pub root: std::path::PathBuf, pub common_dir: std::path::PathBuf,
    pub task_id: String, pub run_id: String, pub module_id: String,
    pub commit: String, pub tree: String, pub base: String, pub authority: String,
    pub baseline: String, pub plan_hash: Option<String>,
    pub commands: Vec<super::verification::VerificationCommand>,
    pub commands_hash: String, pub fences: Vec<crate::hub_protocol::Fence>,
    pub entered_verifying_at: u64, pub merge_deadline: u64,
}
pub struct VerificationLogRef { pub report_eid: String, pub report_sha256: String }
pub struct FinishEvidence {
    pub id: String, pub kind: String, pub task: String, pub run: String,
    pub module: String, pub commit: String, pub tree: String, pub base: String,
    pub authority: String, pub baseline: String, pub plan_hash: Option<String>,
    pub commands_hash: String, pub fences: Vec<crate::hub_protocol::Fence>,
    pub result: EvidenceResult, pub generated_by: String,
    pub log: VerificationLogRef, pub gates: std::collections::BTreeMap<String, bool>,
    #[serde(flatten)] pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

// recovery_contract::task_review；human_intervention.rs 重导出。
pub enum ReviewSubject { Plan { sha256: String }, Tree { tree: String } }
pub struct ReviewBinding {
    pub run: String, pub task: String, pub baseline: String,
    pub fences: Vec<crate::hub_protocol::Fence>, pub subject: ReviewSubject,
}
pub enum ReviewVerdict { Approved, ChangesRequested, Rejected }
pub struct ReviewFact {
    pub binding: ReviewBinding, pub verdict: ReviewVerdict,
    pub actor: String, pub answer_eid: String,
}

// recovery_contract::baseline；与 task_review::ReviewBinding 是不同领域类型。
#[serde(deny_unknown_fields)]
pub struct DraftObject { pub id: String, pub path: String, pub text: String }
pub struct DraftBundle {
    pub id: String, pub project_id: String, pub docs_dir: String,
    pub source_revision: String, pub target_baseline: String,
    pub objects: std::collections::BTreeMap<String, DraftObject>, pub canvas_text: String,
    pub deletions: Vec<String>, pub questions: Vec<String>, pub risks: Vec<String>,
}
pub struct FrozenFile { pub mode: String, pub sha256: String, pub text: String }
pub struct FrozenDraft {
    pub bundle: DraftBundle, pub files: std::collections::BTreeMap<String, FrozenFile>,
    pub base_directory: std::collections::BTreeMap<String, String>, pub content_hash: String,
}
pub struct ReviewBinding {
    pub project_id: String, pub object_id: String, pub content_hash: String,
    pub target_baseline: String, pub draft_revision: String,
}
pub enum Verdict { Approved, Rejected }
pub struct ApprovalFact {
    pub approval_eid: String, pub binding: ReviewBinding, pub actor: String,
    pub verdict: Verdict, pub comment: String,
}
#[serde(deny_unknown_fields)]
pub struct PublicationFile { pub mode: String, pub sha256: String }
#[serde(deny_unknown_fields)]
pub struct PublicationFreeze {
    pub project_id: String, pub draft_id: String, pub docs_dir: String,
    pub source_revision: String, pub target_baseline: String,
    pub base_directory: std::collections::BTreeMap<String, String>,
    pub files: std::collections::BTreeMap<String, PublicationFile>, pub deletions: Vec<String>,
    pub questions: Vec<String>, pub risks: Vec<String>,
}
pub fn publication_freeze(draft: &FrozenDraft) -> Result<PublicationFreeze, String>;
pub fn publication_content_hash(freeze: &PublicationFreeze,
    texts: &std::collections::BTreeMap<String, String>) -> Result<String, String>;
```

`canonical_json_bytes` 是 core 内部唯一函数：typed 值先序列化为 JSON，递归按 object 键的 UTF-8 字节排序，数组保留次序与全部值，用紧凑 JSON UTF-8 编码，无额外空白或末尾换行；不归一化字符串、路径、CRLF、argv 或 null。不能序列化的路径明确拒绝。commands_hash 为 **SHA256(canonical_json_bytes(整个 commands 数组))** 的小写 hex，report_hash 同样覆盖完整 VerificationReport；不得只 hash 命令字符串、排序 argv 或丢掉 cwd。outbox/P3 请求摘要复用同一 canonical JSON 实现，各自保留既定外层域/字段范围。

`finish.started.data={task,run,input,review_context}`；`finish.completed.data={task,run,evidence,report,hold,review_context}`，其中 report 为上述完整 VerificationReport，不能仅存缓存路径。完成事件 eid 在制作 Evidence 前预分配，`evidence.log.report_eid` 必须等于该原完成 Event.eid，`report_sha256=report_hash(report)`；这是事件内报告 locator，不是新对象。stdout/stderr locator 与各自 hash 沿 report.executions 原字段完整保存。producer 在记录 passed 前实际持久日志并核 hash；历史 consumer 不执行命令，不要求恢复主机存在旧日志路径，不用孤立缓存替代已接受 report。report.commit/tree 等于 FrozenFinish.commit/tree，executions 中完整 command 序列逐值等于 input.commands；非空且每项 result=passed、exit_code=Some(0)、timed_out=false 才允许 Passed。failed/no_tests 不完成，no_tests 的空 executions 不伪造命令执行。Evidence/context/commands_hash 全部与原 started 绑定，extra 不得覆盖固定字段。

出版 hash 沿 P4 原长度前缀算法，现在冻结为精确 **10 个 parts**：前五项依次是 project_id/draft_id/docs_dir/source_revision/target_baseline 的原 UTF-8；第六项是按 path 排序的 `[path,modeOID]` 数组 canonical JSON；第七项是按 path 排序的 `[path,mode,text]` 数组 canonical JSON；第八项是排序去重 deletions 数组 canonical JSON；第九、十项分别是 questions、risks 数组 canonical JSON（保留次序）。hash 初始化 `b"towermd:baseline-freeze:v1\0"`，每个 part 先写其 UTF-8 字节数的 u64 big-endian，再写原字节，最后 SHA256 小写 hex。texts 键集必须恰好等于 freeze.files，逐项复算原文本 sha256 与 mode；非规范/重复 deletion 拒绝，不静默改变批准输入。publication_freeze 从 FrozenDraft 映射并用同一函数重核 content_hash；恢复从实际 publication commit 读 texts 调同一函数。摘要不含草稿 commit SHA、时钟、lease、content_hash 自身或两份出版审计文件，不改原始 OKF 文本。

### 1.3 证明所需的四种持久请求/回执视图

以下仅冻结事件data的窄typed视图，归E0 `recovery_contract::recorded`；不搬入P3全部AppAction，不新增路由、批准权限、注册对象或第二receipt。P3通用派生索引可将同一receipt的result_ref存为JSON；E证明器必须解析下述精确T，不接受客户端提供这些持久data。RequestMeta/RequestReceipt/RequestBody、action/result与四种recorded data均派生Clone/Debug/Serialize/Deserialize/PartialEq并deny_unknown_fields（BaselineCreated的flatten结果按其精确字段集校验）；固定字段严格比较，完整Hold及注册Evidence的其他原键通过flatten保留，不能覆盖固定字段。

```rust
pub struct RequestMeta { pub request_id: String, pub body_hash: String }
pub struct RequestReceipt<T> {
    pub request_id: String, pub body_hash: String, pub result_ref: T,
}
pub struct RequestBody<A> {
    pub project_id: String, pub actor: String, pub host: String,
    pub run: Option<String>, pub action: A,
}
// 以下action均#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]。
pub enum TaskReviewAction {
    Review { hold: String, binding: task_review::ReviewBinding,
        verdict: task_review::ReviewVerdict, comment: String },
}
pub enum BaselineReviewAction {
    ReviewBaseline { binding: baseline::ReviewBinding, verdict: baseline::Verdict,
        comment: String },
}
pub enum PublicationPrepareAction {
    PreparePublication { publication_id: String, approval_eid: String,
        binding: baseline::ReviewBinding, authority_revision: String, content_hash: String },
}
pub enum BaselineCreatedAction { RecordBaseline { result: BaselineCreatedResult } }
pub struct TaskReviewResult {
    pub hold: String, pub fact: task_review::ReviewFact,
    pub verdict: task_review::ReviewVerdict, pub comment: String,
}
pub struct PublicationPreparedResult {
    pub prepared_eid: String, pub publication_id: String, pub authority_revision: String,
    pub binding: baseline::ReviewBinding, pub content_hash: String,
}
pub struct BaselineCreatedResult {
    pub commit: String, pub publication_id: String, pub approval_eid: String,
    pub content_hash: String, pub target: String, pub tag: String, pub tags: Vec<String>,
}
// ReviewHoldView是原完整Hold的窄读取视图，未列出的原键仍保留，不创建另一种Hold。
pub struct ReviewHoldView {
    #[serde(rename="type")] pub object_type: String,
    pub id: String, pub body: ReviewBodyView, pub opened_ms: u64, pub opened_seq: u64,
    pub reply: Option<ReviewReplyView>,
    #[serde(flatten)] pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}
#[serde(tag="kind",rename_all="snake_case")]
pub enum ReviewBodyView {
    Review { binding: task_review::ReviewBinding,
        verdict: Option<task_review::ReviewVerdict>, plan_content: Option<String>,
        source_commit: Option<String>, source_path: Option<String>, comment: Option<String> },
}
pub struct ReviewReplyView {
    pub id: String, pub text: String, pub actor: String, pub at_ms: u64,
    #[serde(flatten)] pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}
pub struct TaskReviewRecorded {
    pub hold: ReviewHoldView, pub fact: task_review::ReviewFact, pub comment: String,
    pub request_body: RequestBody<TaskReviewAction>, pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<TaskReviewResult>,
}
pub struct BaselineReviewRecorded {
    pub approval: baseline::ApprovalFact,
    pub request_body: RequestBody<BaselineReviewAction>, pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<baseline::ApprovalFact>,
}
pub struct PublicationPrepared {
    pub publication_id: String, pub approval_eid: String,
    pub binding: baseline::ReviewBinding, pub authority_revision: String, pub content_hash: String,
    pub request_body: RequestBody<PublicationPrepareAction>, pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<PublicationPreparedResult>,
}
pub struct BaselineCreated {
    #[serde(flatten)] pub result: BaselineCreatedResult,
    pub request_body: RequestBody<BaselineCreatedAction>, pub p3_request: RequestMeta,
    pub p3_receipt: RequestReceipt<BaselineCreatedResult>,
}
pub fn request_event_eid(request_id: &str, kind: &str) -> Result<String, String>;
```

RequestBody不含request_id/instance/directory：项目从实际Catalog解析，actor/host取原producer，run保留**原请求显式run**，不能自动填入后来绑定。上述App请求原无run参数时固定null；内部prepared/created亦固定null。`body_hash=SHA256(canonical_json_bytes(request_body))`小写hex；p3_request与p3_receipt的request_id/body_hash逐值相同，receipt.result_ref必须等于下表固定响应，不混入后来的validity/当前状态。重复同project/request_id必须比较完整typed原请求、hash与原响应；不能仅相信保存的hash或自报actor。

原App/prepared请求ID为预分配的UUIDv7原文本；`request_event_eid(request_id,kind)=derived_eid(hex(SHA256(request_id原UTF-8)),kind)`，先核UUIDv7规范文本。这样复用现有只接受40/64位Git hash的derived_eid，不假定它能直接接受UUID，也不改变其签名。canonical created的内部request_id另按实际commit派生，见表；它不是App调用ID，不受UUIDv7输入限制。

| 原事件 | 必须核对的固定信封与结果 |
| --- | --- |
| `hold.reviewed` | layer=L5、obj=hold.id、actor为原合法App human actor、host=request_body.host、guards=原binding全部fences；eid=request_event_eid(request_id,"hold.reviewed")。fact.answer_eid=原Event.eid，fact.actor/hold.reply.actor=request_body.actor=Event.actor；Hold.body binding/verdict/comment、fact及action逐值相同，reply.text=原comment。result_ref及本次原响应都是TaskReviewResult{hold:hold.id,fact,verdict,comment}；不采用review.answered或普通hold.answered。 |
| `draft.saved` | layer=L1、obj=result.draft_id、actor/host逐值等完整原 request_body；SubmitBaseline 只允许合法 App human 或 process:towermd-cli，EditDraft 只允许合法 App human；guards=[]，不绑定临时 main fence。eid=request_event_eid(request_id,"draft.saved")。data 为唯一 DraftSavedRecorded，result=receipt.result_ref，完整原请求/meta/hash 严格一致；ref 固定 refs/towermd/drafts/{id}，revision/content_hash 只能从实际 opaque materialization 与 SavedDraft reader 对核。此事实不授批准或出版资格。 |
| `baseline.approved` / `baseline.rejected` | layer=L1、obj=approval.binding.object_id、actor为原合法App human actor、host=request_body.host、正常guards=[]；kind由原verdict Approved/Rejected唯一确定，eid=request_event_eid(request_id,kind)=approval.approval_eid。binding/comment/verdict/actor与request_body/action逐值匹配；result_ref与原响应就是原完整ApprovalFact。正常无guards事实不绑定临时writer fence，旧实际stale记录只供审计，不剥除guards升级。 |
| `baseline.publication_prepared` | layer=L1、actor=process:towermd-baseline、obj=publication_id、host=request_body.host=publication_host、guards恰为原唯一main fence；eid=request_event_eid(request_id,"baseline.publication_prepared")。原action/data/binding/hash/authority逐值相同，publication_id=approval_eid；result_ref就是{prepared_eid:原Event.eid,publication_id,authority_revision,binding,content_hash}，不含尚未生成的candidate commit。 |
| `baseline.created` | layer=L1、actor=process:towermd-baseline、host=manifest冻结publication_host、obj=publication_id、guards=[]；eid=derived_eid(actual_commit,"baseline.created")，内部request_id=derived_eid(actual_commit,"baseline.created.request")。BaselineCreatedResult严格取实际commit及manifest原publication_id/approval_eid、binding.content_hash/target_baseline，tag=该精确target，tags=该commit已认证标签的排序去重集合；action.result、data的扁平结果字段、receipt.result_ref及固定内部原响应完全相同，不注入当前host/epoch/时钟。 |

baseline.created的原批准/准备全文仍从同一实际commit内精确Decision/manifest路径读取并递归重核，BaselineCreatedResult不是证明替代物。缺字段、未知布局、actor/obj/layer/guards/eid/request/receipt任一不符，只诊断，不授予批准或认证基线。

`draft.saved` 的私有 ref CAS 与业务事件之间发生崩溃时，只从实际 `SavedDraft` reader 核验的原 manifest 请求恢复。正常编辑覆盖 ref 前，须先协调旧 tip 的原保存回执。无 origin 的实际 Git 配置可证明 `LocalSaved`；存在 origin 或交付无法证明时，恢复结果保守记 `LocalSavedRemotePending`，原因固定为 `remote delivery not confirmed during recovery`，不能凭当前网络状态猜成 `RemoteSaved`。已有 accepted 记录始终返回原完整回执，当前草稿/保护目录是否仍匹配另列 validity。

已接受的上述五种 typed 基线动作（`draft.saved`、`baseline.approved`、`baseline.rejected`、`baseline.publication_prepared`、`baseline.created`）可由唯一 ledger 生成内部 `request.receipt.restored` 审计封装。封装只保存实际 reader 已核的完整原 Event，保留原 seq/time/stale/guards 与请求/回执，不重新 append 原 NewEvent 冒充旧接受。外层 actor=`process:towermd-daemon`、layer=L5、guards=[]，eid 从完整原 Event 的 canonical hash 派生，不能自身授予批准或复活旧 fence。旧 preparation 仅供审计；新 holder 要先协调原回执，再用当前真实主干 fence 产生新的 preparation 请求。created 封装仍须通过实际 main/tag/commit 的唯一证明器，并递归读取 commit 内原批准/准备全文；封装不是出版证明的替代物。

### 1.3.1 P3 Task 2：pending Ask 的显式 recipient 加入（待实现）

本段只冻结 `hold.recipient_added` 这一个有限持久事件，不开放客户端成员编辑，不搬入P3全部AppAction，不改变Review/Prompt/Pause，也不新增注册对象、ledger或hash算法。共享recorded维护AskOpen原请求、Ask Hold及Recipient的窄typed读取视图，P3由完整对象映射；以下字段与原响应为唯一布局，不能靠任意JSON/self-reported声明代替实际Task/Run授权。

| 项 | 必须核对的固定内容 |
| --- | --- |
| 原请求 | `RequestBody`的action仅为 `AskOpen { question, options, recommended, context }`；project_id从实际Catalog解析，actor固定process:towermd-cli，host为实际producer，run保留原请求显式值/null。canonical hash与UUIDv7校验复用第1.2–1.3节；不将自动解析的run或recipient回填请求改变hash。 |
| 信封 | kind=hold.recipient_added，layer=L5，obj=原Hold.id，actor=process:towermd-cli，host=added.host=request_body.host，guards逐项等于新增实际Run的完整fences，eid=request_event_eid(原request_id,"hold.recipient_added")。不要求旧recipient的fences仍有效，也不把它们并入本事件guards。 |
| data | 精确固定字段为 `{hold, added, opening_eid, request_body, p3_request, p3_receipt}`；完整Hold未知键原样保留，固定字段不能由extra覆盖。`added={run,task,host,worktree,branch,fences}`全部取同项目已同步实际Task/Run，不接受客户端路径、actor或自报fences。 |
| result_ref | 精确为 `{hold:原Hold.id, opening_eid:原开立eid, recipient:added, state:"pending", reply:null}`；p3_receipt的request_id/body_hash逐值等于p3_request及原typed正文摘要。此原响应不因后续Answer、过期或重建被重写；当前有效性另列。 |

仅在无有效权威Decision答案、同canonical module与fingerprint、原Ask在final CAS仍未回答且该run尚未加入时允许新加入。`opening_eid`必须定位同Hold实际原非stale Ask开立Event；开立顺序由该Event.seq证明，不信客户端opened_seq。以按实际外层顺序重放得到的上一份有效完整Hold为副本，recipients末尾只追加一个added，所有其他字段及既有recipient逐值不变，包括原问题/选项/context/fingerprint、opened_ms/opened_seq、reply、waits、delivered、attention和未知键。重复同完整recipient只产生既定 `hold.requested` no-op receipt；同run但其他身份/任一fence改变则拒绝，不替换原成员。claim、新Run出现及AskWait均不构成加入。

Answer与加入按实际outer Event.seq定序：Answer先接受便禁止对该已答会话加入，重新查权威Decision或开新Hold；加入先接受，后续Answer必须保留最新recipient，不得以旧Hold覆盖。加入不产生wait slice、不改变已有预算或复活过期Run；仅完整live recipient可收到后续actionable答复。Review/Prompt/Pause以及已答会话的旧Reply没有成员扩展或跨Run自动继承例外。

### 1.4 原评审opening、gate Evidence与阶段上下文

E0仅冻结证明所需的窄视图，P3沿原完整Hold生产/重导出，P2沿同一kind:gate schema生产；不建立通用历史系统，不保存renew或重建TTL。下列Gate沿第1.1节枚举，G1/G4 proof之外的G0/G2/G3/G5仍可用同一GateEvidence布局，缺上下文用null明确表示，不虚构值。

```rust
pub enum ReviewGate { G1, G4 } // serde snake_case
pub struct ReviewOpenView {
    pub hold: ReviewHoldView, pub gate: ReviewGate,
    pub binding: task_review::ReviewBinding, pub plan_content: Option<String>,
}
pub struct GateGenerated { pub by: String, pub at: String }
pub enum GateResult { Passed, Failed } // serde snake_case
pub struct GateReviewLink {
    pub opening: crate::hub_protocol::Event, pub reviewed: crate::hub_protocol::Event,
    pub completed_eid: Option<String>,
}
pub struct GateEvidence {
    #[serde(rename="type")] pub object_type: String,
    pub id: String, pub kind: String, pub gate: crate::task_gates::Gate,
    pub project_id: String, pub task: String, pub run: String, pub module: String,
    pub commit: Option<String>, pub tree: Option<String>, pub base: Option<String>,
    pub authority: Option<String>, pub baseline: Option<String>, pub plan_hash: Option<String>,
    pub plan_opening_eid: Option<String>, pub commands_hash: Option<String>,
    pub fences: Vec<crate::hub_protocol::Fence>, pub result: GateResult,
    pub review: Option<GateReviewLink>, pub diagnostic: Option<String>,
    pub generated: GateGenerated,
    #[serde(flatten)] pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}
pub struct ReviewContext {
    pub gate: ReviewGate, pub opening: crate::hub_protocol::Event,
    pub reviewed: crate::hub_protocol::Event, pub gate_evidence: crate::hub_protocol::Event,
}
```

以上是原事件的typed读取视图，不是客户端可信凭据。ReviewOpenView从原非stale `hold.opened` 的完整`data.hold`提取：事件actor=process:towermd-cli、layer=L5、obj=Hold.id、guards=原binding完整fences；Hold.body必须Review且verdict/comment/reply未答，顶层gate由Plan→G1、Tree→G4唯一派生，binding与plan_content逐值取原body，不新增客户端可报gate/latest标记。G1 plan_content必须Some且SHA256原UTF-8等于binding.subject.Plan.sha256；G4必须Tree且plan_content=None。opening Event.seq才是原开立顺序，Hold.opened_seq仅投影副本、不能用于证明；恢复外层seq也不能替代它。

`evidence.recorded.data={evidence:GateEvidence}`，layer=L5、actor=process:towermd-gates、obj=evidence.id、guards=完整fences；generated.by必须同actor，generated.at为原producer固定的合法时间。预分配UUIDv7 `u`，evidence.id=`E-<u>`，原event.eid=request_event_eid(u,"evidence.recorded")；原pending/重试保存同u/body，不创建第二条事实。object_type=Evidence、kind=gate，Passed才可能满足门禁；G1/G4必须有review，其他gate允许None，不能用它们冒充人工评审。

G1/G4的GateEvidence必需commit/tree/base/authority/baseline/commands_hash均Some且与对应FrozenFinish逐值绑定（G5认可的state/canvas变基由唯一守卫核，不用nullable绕过）；project/task/run/module/fences同claim/冻结输入。review.opening/reviewed必须分别是同Hold原opening与第1.3节合法hold.reviewed，binding/actor/Plan或tree精确相同。G1时plan_hash与plan_opening_eid必需匹配原G1 opening，completed_eid=None；G4时review.completed_eid=原finish.completed.eid，tree/commands_hash/完整fences同该完成报告与Evidence，高风险plan_hash/plan_opening_eid仍绑定原有效G1；非高风险无G1时两者为None。diagnostic不授予通过资格，extra不能覆盖固定字段。

P2正常producer用P3真实current_plan_subject按原hold.opened接受seq选当前Plan，并在freeze/record_finish及最终认证/CAS前重核；不能由FrozenFinish.plan_hash或一个latest=true声明取代。`finish.started.data`增加`review_context:Option<ReviewContext>`，高风险为原G1完整opening/reviewed/gateEvidence，非高风险无需G1时None；`finish.completed.data`保留**完全相同**G1上下文并在记录时重核当前subject。passed completed不等待未来G4：晚G4由独立kind:gate evidence.recorded携自己的opening/reviewed并绑定原completed eid，随后trailers/正常integrator消费，不改写completed/G3 Evidence。

原接受角色顺序必须`claim.seq < G1.opening.seq < G1.reviewed.seq < G1.gate_evidence.seq < started.seq < completed.seq`；G4必须`opening.seq < reviewed.seq < gate_evidence.seq`且`completed.seq < G4.gate_evidence.seq`（不要求completed已包含G4，不把允许更早开立的同tree请求改成额外批准门槛）。每一原Event必须来自已核原接受角色、非stale、eid/body/fullguards一致；合法稀疏业务链无已知冲突即可，不要求逐seq覆盖、leasegrant/renew或填补无关gap。

已核历史中若存在更晚G1 opening，且在started、record_finish或合法review/gate接受时点使该阶段subject不再当前，则拒绝旧context。最终gate检查后若已知另开不同Plan且没有新的合法final gate Evidence覆盖该subject，保守拒绝认证。比较只用原Event.seq/原上下文，不能用恢复outerseq或新hub epoch排列原事实；已经合法成立的canonical既成历史不因旧guards在新hub重放stale而失效。原必要事件缺失/冲突/原接受角色不明则fail closed；不声称密码学证明未留存opening不存在，信任范围仍是v0.1真实已接受coreproducer，不防全文件恶意重写。

canonical merge proof的`review_contexts:Vec<ReviewContext>`按G1、G4固定顺序保存必要原opening/reviewed/gateEvidence；低风险无需要则空。独立gate Evidence内再保原opening/reviewed，二次只靠snapshot/archive+Git仍能重核完整原绑定、hash与阶段顺序，不只保hold.reviewed列表。

### 1.5 终态G5与实际main接入（同tree冷clone所需）

纯Git历史不能把多Task提交与finish后的main代码推进唯一分开。不得取candidate直接父或“最早非state前缀”猜基底。复用P2已计划的 `evidence.recorded` / `GateEvidence`，在 `trailers` 输出前由真实core producer完成fresh G5并耐久记录终态事实；不新增注册type、批准或执行权限。

终态事件data固定为 `{evidence: GateEvidence, completed_eid: 原finish.completed.eid, parents: [当时Task HEAD的实际父OID，保持Git原顺序]}`。沿第1.4节固定Event信封/producer/完整fences与Evidence布局，gate=G5、result=passed、review=null、diagnostic=null；task/run/module/project/baseline/plan与commands摘要绑定原FrozenFinish，commit/tree取本次实际HEAD，base=authority取本次实际权威main，不能从HTTP或声明填值。parents必须从该不可变commit读取，捕获后重核HEAD/tree/authority；pending→Hub原回执全核→confirmed耐久后才输出五行trailer。普通check的G5不因缺此链接被拒读，但不能充当终态完成证明。

原接受seq须claim<started<completed<终态G5；必要G1仍先于started、必要晚G4 gate先于终态G5。终态producer重核当前Plan/G1/G4、全部fences与固定deadline；consumer核原完整上下文，final gate后已知不同Plan无新有效终态仍保守拒绝。合法稀疏confirmed不要求补齐全部seq或renew。

canonical proof在原字段之外必须保存 `integration_gate: 原完整终态Event`。历史从该原事实取candidate_base，再用唯一purecheck_g5核实际Git；缺事实/错completed/错字段/原stale/来源冲突均不得merged，不以客户端base替代。多个重复有效事实按原seq、eid固定取最早一个，后来的同义事实不改变既成canonical body；实际冲突仍拒绝。

认证对象是实际main接入commit：沿真实main first-parent历史定位使Task候选首次进入该线的提交，不能把不合法merge的second-parent Task tip另按“main可达”认证。该接入commit必须具有FR-12完整五行Task上下文；输入若是其side Task提交，必须先归一到该接入commit并核上下文相同。直接FF/amend/squash时，实际Task提交tree等于终态tree，父数组恰为捕获parents或 `[gate.base]`；常规两父merge时，接入commit第一父恰为gate.base，第二Task tip满足同一tree/parents规则，接入tree也相同。未知接入形态只诊断，不猜基底；原Task自身多父在captured parents精确匹配时仍可作为直接接入。

同tree改提交信息或squash不重跑verifier；state/canvas-only rebase后重新调用trailers取得实际新基底/parents的终态G5，仍复用原G3，代码/config推进则必须重新finish。Gate.commit旧对象可因amend消失，其原审计值不改；真实candidate/parents/tree、原接受回执与第3.2节树重建一起闭合证明。canonical merge.done的actual_commit/eid/data.commit统一取实际main接入commit，各入口逐值相同，二次恢复保留integration_gate，不新增第二confirmed。

## 2. outbox：真实接受回执先落盘，再返回本次成功

`RecoveryOutbox` 固定为 `open(directory, context: RecoveryContext)`、`append_pending(&NewEvent)`、`read_inputs()`；`RecoveryContext {project_id, docs_dir, hub}` 由 core 从已校验 Catalog 提取，字段私有；这是原 E 的 open(directory) 在实现前必须对齐的最小项目绑定参数。新增 **core 内部** `persist_confirmed(original: &NewEvent, actual: &Event) -> Result<(), String>`。daemon 只调用业务/持久编排，不能自行构造已确认事实。项目目录由 daemon 传入，core 不猜路径；上下文取已校验 Catalog（project_id、docs_dir、hub），不能从 JSON 自报值选另一个项目。

单行 JSONL 信封固定：`{schema: 1, project_id, context: {docs_dir, hub}, state: "pending", original: NewEvent}` 或 `{schema: 1, project_id, context: {docs_dir, hub}, state: "confirmed", original: NewEvent, actual: Event}`。`hub` 沿 Config 原值/类型；信封固定键严格校验，事件 data/对象的未知键原样保留。schema 不支持、上下文不符、完整行损坏均拒绝；末尾不完整行保留供修复、不当 confirmed。

所有需要恢复的非 heartbeat 业务事件（含 task.claimed、finish.started/completed、P3/P4 事实及最终 receipt）遵守：

```text
冻结原 NewEvent / 原请求身份
 -> append_pending(original) + fsync（必要的创建目录/文件也持久化）
 -> hub.append(original, now)
 -> 核对真实完整 Event
 -> persist_confirmed(original, actual) + fsync
 -> 返回本次 mutation 成功 / 幂等 success receipt / 将 Job 标为 Completed
Hub 已耐久接受后可正常 sync/广播/快照，不等待本机 confirmed；以上只约束本次成功回包。
```

- pending fsync 失败不得调用 hub。hub 失败或返回结果不确定，保留原 eid/body/guards，返回 uncertain。confirmed fsync 失败也为 uncertain：hub 可能已接受，不能生成新 eid、换 guards、改请求 hash 或先将 Job/业务响应标成功。Hub 已耐久接受的真实事实仍可正常连续 sync、广播和快照，未来 status 可展示它；不做本地隔离或停顿整个前缀。confirmed 只 gate 本次 mutation/幂等成功回包与 Job Completed，不能把已接受的业务结果改成失败。
- hub 对重复 eid 可能直接返回旧 Event，且不比较本次 body。必须逐值比较 `eid/host/actor/type/layer/obj/data/guards`，其中 guards 全数组的 name/holder/epoch 及次序均相同；不只核 eid 或 data hash。不同 body 一律冲突，不能记录 confirmed。
- 核验 actual 的 seq 非零、时间为合法 hub 时间、epoch 与普通事件原 guards 的首项 epoch（空则 None）一致、stale 为 hub 实际结果、cost 沿当前 hub 合法值（未报告为 null）。普通 append 回执不得含 lease_delta、heartbeat、legacy 或未知系统状态变更。完整 Event 原样保存，不自行补 seq/time/stale。stale 可以作为历史审计保存，不能当原非 stale 接受证明。
- `Duplicate { seq }` 只有去重位置，不能据此用 original 捏造 Event。只能在已核验 archive/confirmed 中找到同 eid、同 seq、完整匹配的原 Event 才协调成功；找不到保持 uncertain/需补原回执诊断。不能只拿 Snapshot 的 eid→seq 索引反推正文。
- `read_inputs` 返回私有字段的 `RecoveryInputs`，分别暴露 pending 与已校验 confirmed 的只读 reader；不再用公共 `events: Vec<NewEvent>` 混合两者。digest 由 core 重算：按 host 文件名及行位置稳定读取，递归排序 JSON object 键、保留数组次序与全值、带 schema/context/state 域标识，覆盖 original 和 actual 的全部内容。排序、等价和摘要实现只有一处；pending→confirmed 会改变摘要。
- 同进程的项目 reader 与 outbox 重开共享只减权限的 `OutboxUncertainty`：confirmed 写入前先屏蔽该 eid 的成功回执，只有本次完整持久化成功才清除，失败保留；旧 `RecoveryInputs` 亦现场读取该共享状态。它只阻止未确认 fsync 的 mutation/幂等成功响应，不能阻止 hub 已接受事实的同步、广播、快照或另授业务权限。只读 GET 不进行 fsync、追加或协调 pending；不得通过重开 outbox、重读完整 JSONL 字节或重建缓存清掉本次进程的不确定性。
- 同 eid 的 original 逐值一致才可去重；同 eid 异 body，或**原接受回执角色**的 actual seq/time/stale/其他字段冲突均拒绝。archive 中的原接受回执与 confirmed 必须完整相等。恢复新 hub 的内部 replay 直接走 staging HubProtocol.append，**不调用 producer append_durable/persist_confirmed，不写第二条 confirmed**；它的新 seq/time/stale 是外层 replay 信封，与旧 confirmed 是不同角色，reader 不将两者强等。reader 按已核 recovery_gap/恢复边界识别外层 replay，核其 original 全业务字段/原 guards，内嵌 canonical proof 的原非 stale Event 独立按原接受角色核验；不能用新外层回执替换或“修正”原回执。

- P3 Task2的 `hold.recipient_added` 同样使用上述唯一durable顺序。原project/request_id/完整正文/hash与原receipt优先于tick、运行解析、同问去重和当前是否已回答；重试只补保存的原eid/原guards/原完整Event，不能重算成员或另开Hold。已接受原角色/confirmed-only仅可修复本次请求耐久回执；有效成员变更只从已核实际outer中的非stale事件按seq重放，stale外层不能恢复成员或Run权限。新增kind不自动进入`request.receipt.restored`等审计封装白名单；缺精确原接受回执时保持uncertain，不据pending或Duplicate.seq造确认。
- 对该有限事件增加真实producer/恢复负控：同recipient新UUID留no-op receipt、同UUID无重复、异body拒绝；Answer/加入两种顺序；同run身份/fence替换、缺完整touches、伪opening_eid、一次追加两人、修改原Hold已知/未知键、非Ask/已答Hold均拒绝。pending/confirmed故障、重启、checkpoint+tail及App确认重建后，核原UUID/body/guards/receipt与旧成员预算不变；R1过期后R2只有显式AskOpen才可加入，不能自动继承会话答复或复活R1。

- legacy renew 输入丢弃；新入队拒绝 lease.renewed，其他 lease 操作仍只走协议专用接口。不保存全部 renew 来重建 TTL。confirmed 的稀疏 seq 最大值，以及每个原 Event.epoch 与该 Event **全部 guards 的 epochs** 的最大值，必须加入 prepare 的高水位（不能只取首 guard epoch，批量 grant 会逐项递增），再 checked_add(2^32)；不得把缺口伪装成连续前缀或只按 snapshot 旧高水位跳号。
- prepare/request 绑定全 inputs.digest；确认时输入变化必须重新请求。恢复重发 pending/普通历史事件时保留原 eid/guards，旧 fence 在新 hub 仍 stale，不复活旧 Run。confirmed 只证明过去被接受，不给新 hub 当前权限。


### core 组合入口与现有 claim 接线依赖

E 交付 `pub(crate) fn append_durable(hub: &mut dyn HubProtocol, outbox: &mut RecoveryOutbox, original: NewEvent, now_ms: u64) -> Result<AppendReceipt, HubError>`，位于 core 的 outbox 组合模块；普通业务 append 全部执行上面的顺序，pending/confirmed I/O 或未知 Duplicate 为 Uncertain/Io，不返回虚构 Event。Duplicate 可先查 outbox 已核 confirmed；只有 archive 的回执由 core 同一已核历史 reader 交给原请求协调器核验后调用 persist_confirmed，再重试。没有完整原回执就保持 uncertain。

此 helper 不控制 session 投影；Hub 耐久接受后连续 sync/广播/快照照常进行，即使本机 confirmed fsync 失败。只有本次 mutation 成功回包/幂等成功返回/Job Completed 等待确认耐久；不得因 confirmed 缺失停顿连续投影。P3 的 append_p3_event 与 P4 现有writer只是业务封装，必须调用它，不再重复实现三阶段持久逻辑。E 交付该 helper/reader 的核心测试，不把它当已完成 daemon claim 接线。

P2 Task1 在现有 `ProjectSession::execute(catalog,host,command,run,now)` 和其 `TaskService::execute(hub,catalog,host,command,run,now)` **尾部显式增加** `outbox: &mut RecoveryOutbox`；相应 tasks::execute 包装与 daemon 调用一起更新。daemon 按已核项目打开外部 runtime outbox 后传入，TaskService 所有普通业务 append（尤其 task.claimed 的完整 Task/Run/全 fences）调用 E helper；lease acquire/renew/release 仍委托原 HubProtocol 专门方法，不把离线pending当租约。confirmed失败时本次调用不宣告claim成功，但错误收尾sync与未来status仍可观察真实Hub已接受的Run；协调后沿原task/run/eid返回，不能重新领取/新建Run掩盖uncertain。


后台与协调签名也必须接线：`ProjectSession::tick(host,now,outbox)`、`ProjectSession::renew_binding(catalog,host,run,now,outbox)`、`TaskService::expire(hub,host,now,outbox)`、`TaskService::reconcile_pending(hub,now,outbox)`、`TaskService::renew_binding(hub,catalog,host,run,now,outbox)` 及 `tasks::expire/renew_binding` 包装尾部显式传同项目 `&mut RecoveryOutbox`；其内部 release_run/compensate_claim 等会产生 task.released 的普通producer也传同outbox、调用唯一helper。daemon 后台tick、快照前协调、请求前续租/到期检查及待定claim协调逐个传入，不依赖未来自动wrap。lease acquire/renew/release 的系统事件保持专用协议，不入outbox。P3 的 `ProjectSession::human_tick(host,now_ms,outbox:&mut RecoveryOutbox)->Result<(),String>` 同样显式注入同项目outbox，hold.wait_finished/hold.attention 的后台producer经append_p3_event→唯一helper；固定自动请求ID/原eid先持久，不因tick重试重复结算。

本机请求协调器从outbox派生 durability=uncertain/confirmed（可删除重建，非业务authority）。Hub中原 p3_receipt/result_ref 保留真实已接受结果，不因本机fsync失败改写。mutation同ID重试先核原project/request_id/body_hash冲突，再**在查询/返回幂等成功receipt之前**协调该请求原pending与真实原Event补confirmed；持续fsync失败仍返回uncertain，解除只补原Event、不重做业务。Duplicate只有已核archive/confirmed里完整匹配的原Event才可协调，不用seq或当前投影捏造。status展示Hubstate与本机durability是分开的事实。同ID协调优先定位已核原接受回执（archive/confirmed/canonicalproof中的原Event），不为已确认旧请求再次调用新hub append；恢复外层replay绝不能交persist_confirmed。仅有新外层而缺原Event时保持原接受结果未知，不能捏造旧success。请求自身补confirmed持续fsync失败仍uncertain；下述依赖可采用已核真实archive，不等于绕过请求自身成功门槛。

关键依赖窗口：claim confirmed失败而status已见Run后直接finish，freeze/finish成功路径必须先协调必需原claim；finish.started与finish.completed同样逐个满足耐久证明（原confirmed或已核真实archive），否则该finish请求仍uncertain，不能声称成功。只补此请求依赖的证明，不停顿全前缀。finish.completed已被Hub接受但confirm失败时Job保持Recording/uncertain，重启/重试补原Event确认、不重跑verifier、不新建Evidence或改结果。最终success前完整claim/started/completed证明均可恢复。

P2 Task2 的 make_evidence/record_finish 固定接收原completed eid/完整report（第1.2节），record_finish尾部显式增加 `outbox: &mut RecoveryOutbox`，finish.started/completed 经该 helper。P2 Task1 必须先完成真实二进制CLI→daemon claim、drop/reopen后检查原confirmed完整回执、仅删hub保留Git/outbox并经真实App确认恢复的测试；确认原claim为历史且旧Run无新权限。Task2 再跑真实 claim→freeze→verifier pass→Git merge→hub丢失恢复链，缺已接线claim不得以手填Run或仅freeze/pass夹具算端到端成功。E早期契约历史夹具不能替代这些producer验收。

## 3. 唯一历史输入与任务证明

`VerifiedRecoveryHistory` 字段与构造器私有，不实现任意 JSON 的公开反序列化入口。唯一构造路径汇合：批 3 统一双源 snapshot/archive reader（含连续前缀、项目与 checkpoint/全归档一致性校验）和第 2 节已核 confirmed。正常 session 的已核连续 tail 也经同一校验构建只读历史。结构校验与原接受事实索引先在reader内部构成不逸出的临时候选；canonical校验共用同一纯算法读取该内部候选，不先向调用者发可信history以解循环。所有canonical proof及实际Git校验全部通过后才返回VerifiedRecoveryHistory；不能以公开字段的 SavedSnapshot 直接 mint。reader 的 snapshot 入口自行读实际双源，不接收调用者自报快照；session 入口只从该 session 的已核连续 tail/已核归档和同项目 RecoveryInputs 取值，不公开接收 Vec<Event>。raw `StateProjection`、孤立对象文件、raw pending 和自报 verified 不能获得这个类型。

```rust
pub struct VerifiedRecoveryHistory { /* private: verified events/context/indexes */ }
// core内部入口自行读取实际来源，不接收裸SavedSnapshot/投影/事件Vec。
pub(crate) fn read_recovery_history(catalog: &Catalog,
    inputs: &RecoveryInputs) -> Result<VerifiedRecoveryHistory, String>;
// Session只从第3.1节已安装的私有准备结果派生只读history；
// 不在持锁的&self方法中读取Git/归档，不公开接收事件Vec。
pub struct TaskTrailers {
    pub task: String, pub run: String, pub module: String,
    pub evidence: String, pub epoch: u64,
}
pub struct PublicationTrailers {
    pub baseline: String, pub approval: String, pub publication: String,
    pub content_hash: String, pub epoch: u64,
}
pub enum TrailerContext {
    Task(TaskTrailers), Snapshot { seq: u64, epoch: u64 },
    Publication(PublicationTrailers), None,
}
pub struct MergeProofInput { pub commit: String, pub trailers: TaskTrailers }
pub struct VerifiedMerge { event: NewEvent } // 私有字段，仅prove_merge可构造。
impl VerifiedMerge { pub fn into_event(self) -> NewEvent { self.event } }
pub fn parse_trailer_context(message: &str) -> Result<TrailerContext, String>;
pub fn render_task_trailers(value: &TaskTrailers) -> Result<String, String>;
pub fn prove_merge(catalog: &Catalog, history: &VerifiedRecoveryHistory,
    input: &MergeProofInput) -> Result<VerifiedMerge, String>;
// VerifiedPublication 同样字段/构造私有；签名见第5节。
```

### 3.1 E1 的有效 Session 入口与锁外证明接线（待实现）

对 Root `3dd78a9958a639ec72bccdfa2dadb8f19d791caf` 的接口核对：现 `ProjectSession::new(hub,now)` 在 Catalog 绑定前恢复 opaque checkpoint 并 apply tail，`bind_catalog` 仅核 project_id/common_dir；`sync(now)` 的 SnapshotRequired 分支也先 restore。现 daemon 冷开/临时读取在 new 后才 validate_authority，后台 tick 与已建立 WebSocket 每轮没有 Catalog 参数。上述旧调用顺序不满足本节证明要求；E1 必须一并迁移，不能留到 F/P2，也不能将 Git/归档 I/O 塞进锁内 sync。以下为确定的待实施签名，不声称 E0 WIP 已提供任何私有 helper：

```rust
pub enum SessionSyncError {
    PreparationRequired,
    SourceChanged(String),
    Rejected(String),
}
impl ProjectSession {
    pub fn new(hub: Box<dyn HubProtocol>, catalog: &Catalog,
        inputs: &RecoveryInputs, now: u64) -> Result<Self, SessionSyncError>;
    pub fn sync_shared(session: &std::sync::Mutex<Self>, catalog: &Catalog,
        inputs: &RecoveryInputs, now: u64) -> Result<(), SessionSyncError>;
    pub(crate) fn sync(&mut self, now: u64) -> Result<(), SessionSyncError>;
    pub fn applied_seq(&self) -> u64;
}
```

**所有权与输入。** new 独占尚未放入 Mutex 的 HubProtocol，先从实际 Catalog、双源/全归档、同项目 RecoveryInputs 与实际初始订阅构造内部候选，核 checkpoint 来源、canonical proof 与 Git 后才返回有效业务 view；失败不逸出临时 projection。Session 私有持有从 Catalog 提取的 root/common_dir/project_id/docs_dir/hub/authority revision 绑定，以及已核历史/精确证明准备结果，不借用外部 Catalog、不新增平行 authority。RecoveryInputs 必须由 core 在 session/projects 注册表锁外读取 daemon 指定 runtime outbox 实际来源；Q1 cold-only gate 的例外见下文。缺文件是由同一 reader 按实际 Catalog 上下文产生的合法 empty，不能由公开 empty/Vec 构造器代替。reader 私有保存可重探测的来源定位与全 digest；这只是原输入的定位，不是另一份持久状态。E0 实际 reader/helper 名称在其双审 SHA 上映射，不提前假定。

**共享两阶段入口。** sync_shared 同时承担正常同步与 authority 校验：①短锁从实际 hub 捕获 batch 或 opaque checkpoint、cursor/floor/ack/head 和已绑定上下文；②释放锁，读取实际双源/全归档与输入，按第3节内部候选算法核 canonical/Git；③锁外重探测 main/private 来源、inputs digest；④短锁重核捕获坐标/上下文及批次身份后原子安装已核 prefix/history/view。准备对象的字段、构造器及安装方法均为 core 私有，不能接收公开 SavedSnapshot、StateProjection、Vec<Event>、裸 seed 或 verified/cache bool 铸造信任。缓存只复用与实际 Catalog/来源提交、完整批次、cursor/floor/ack、输入 digest 及原接受上下文相绑定的已核证明；later Plan 等业务历史变化必须重新核验，不能仅按 commit 或旧 authority_key 命中。

sync_shared 内部至多三轮完整 capture→prepare→probe→install；floor/ack/head/private 来源变化即废弃旧准备并重读，不在短锁中补慢验证。传入 Catalog 的 authority/context 或 inputs digest 已失效时直接返回 SourceChanged，结束本次操作；正常下一次请求/tick 在锁外重载真实 Catalog/inputs，恢复则须重新 prepare，不能扩大旧请求已批准的来源/digest。三轮仍变化同样返回 SourceChanged，不在调用者外面套新的三轮循环。Rejected 表示证明/schema/绑定不合法，保留诊断与原文件，业务不推进。锁序仍 writer→短 session；Git/归档/outbox 读取、来源 probe 与等待 writer 不持 session 或 projects 注册表锁。Q1 仅冷开/临时读取使用的 registration gate 例外沿用下一段，不把它扩大到已注册项目快路。

冷开/临时读取保留已双审 Q1 的 cold-only registration gate：在 gate 内重查已注册 Arc，冷注册在 Hub::open 前沿原 bind_project 取得 `project-<project_id hash>.lock` 独占文件锁并核 owner/common_dir，所有权贯穿 constructor 并移交 Project._lock；临时读取沿原路径核已有 owner/hub，gate 覆盖临时 Session 完整生命周期，不创建缺失 hub/clone 绑定。gate 可覆盖这些尚未发布对象的慢 constructor/校验，但不持 projects 注册表锁或 live session 锁；已有项目快路不等待 gate。并发首次注册者等待原 gate 后复用同一已发布 Arc，保留原十并发首次注册成功保证，不新增“竞争者报忙”策略或 per-project 协调器。不能先打开两个相同 clone Hub 再争发布；跨 clone 拒绝及原文件锁/owner 绑定不变。

**短锁 fast path。** 原 sync 改为 core 内部 fast path，只消费该 session 已精确核验的 canonical/checkpoint；遇到新的未准备项，在 restore/apply/越过该项 cursor 前返回 PreparationRequired。调用者释放 session 锁后调用一次 sync_shared，再回短锁继续；若继续点又遇到未准备项则结束本次操作并报 SourceChanged/uncertain，不套无限循环，不能先显示 merged 再回滚。执行普通 mutation 前的 PreparationRequired 可以重新做前置检查；hub 已接受后则保留原结果/eid/guards，只补同步/原请求协调，不能重跑命令或新建 Run 来消除错误，必要时返回 uncertain，后续 P2 的 confirmed 成功门槛仍照第2节。bind_catalog 只核身份，不再被当作授信入口；旧无上下文 new/open 不保留为有效兼容 constructor，install_checkpoint 不提供接受裸 Snapshot 的公开授信路径。

后续 P3 等计划的旧 `new(hub,now)` 示例在其实际前置 SHA 开工复核时迁移为本节实际 Catalog/RecoveryInputs constructor；pure apply 示例只验证投影规则，不能作为产品授权或有效 Session 入口。

**opaque checkpoint 与旧 ack。** HubProtocol 继续只核系统信封，metadata/floor 相等不足以 restore 业务。若 hub floor/ack=101，而 Git 已写 snapshot 103、ack 失败，不能拿最新 103 等同 checkpoint 101；统一 reader 先核实际当前双源历史可达性，再读取其中精确 ack.commit 的真实 Snapshot/全归档/trailer，核 ack seq/epoch、floor、项目上下文、canonical/Git 与 opaque projection 一致后，作为旧 floor 证明继续校验 102+ 的真实连续 tail。ack.commit 不可达、缺全归档或不一致即拒绝；不能从任意 ack SHA/public SavedSnapshot 铸造可信历史。此处只证明旧 floor，不改变快照最新来源选择或 Promote 授权：同水位搬送仍只允许既定 current private→current main 的精确等价规则，不把历史 ack 改名为 private 来扩大例外。

**恢复 staging。** 跳号 checkpoint 尚未写 Git 时，仅 recovery 私有 builder 在核原 Catalog/history/inputs、有效原 AppConfirmation/request/source/digest 与 checked 边界后产生字段/构造器私有的 VerifiedRecoverySeed；它记录已核源 checkpoint/全归档及本次 recovery_gap 的确定变换。staging 专用 constructor 仅 core 私有、只收该 seed，不公开 raw Snapshot/RebuildSeed 免检构造器，也不让未核 projection 逸出。staging 的新 canonical tail 仍经同一证明算法；target 安装只消费已核 staging 准备结果并重核空目标水位。现有及 G 的硬顺序保持：完整归档/新 checkpoint 校验→Git 新边界确定持久→ack/安装准备成功→短锁 target.rebuild/install→正常服务；Git 成功但 target 安装失败必须保留新边界并要求更高的新请求。

**恢复空目标 bootstrap。** 已有 Git 快照而 hub 缺失/为空时，不能为调用恢复编排先 new 一个假有效 Session。`SnapshotWriter::confirm_rebuild` 与 core 私有 execute 的目标参数统一为 `existing:Option<Arc<Mutex<ProjectSession>>>`，返回统一为 `Result<(SnapshotReport,Arc<Mutex<ProjectSession>>),String>`；其余确认参数沿 E 计划固定签名。prepare 仅核实际 Git/inputs，缺失与空 hub 均可产生待批请求，不打开/创建 hub、不要求业务 Session。确认编排在原 Q1 gate/clone owner/同一 writer 所有权内先核批准、source/current authority/digest/expiry，再由 core 私有 empty-target builder 取得唯一 raw Hub 所有权；opaque target 的类型/字段/构造器均不公开，不暴露业务 view、execute、checkpoint 或 subscribe。

existing=None 时 core 按 Catalog 的真实 common_dir 实际打开缺失或可读空 hub，核系统 seq/epoch/floor/ack 与空水位，损坏/非空拒绝，不从 None 自报“已核空”。existing=Some 时只接受原已注册 Arc：在 writer→短 session 锁内核实际 protocol 真空水位并原子退役业务入口，raw protocol 仍留在同一个所有者内，仅由私有 opaque target 使用，不 Hub::open、不释放原文件锁、不出现第二 sequencer；原 Arc 的并发调用在退役期间不得取得业务 view 或继续普通写入。退役是 core 私有所有权生命周期，没有外部开关或跳过证明能力，不能自动丢弃非空 Session；私有 target 捕获原绑定与实际空 watermark，安装前再核未变。前置校验失败不退役；退役后的失败保留原 raw target/诊断，不发布或恢复正常写服务，由后续核心恢复重新核真实状态，不要求用户重启来避开文件锁冲突。

只有 Git 新边界确定持久、VerifiedRecoverySeed/全部事实与归档复核、target rebuild/ack/install 成功后才构造/切换有效 Session：None 路径从已安装真实 protocol 经有效 constructor 得到新 Arc，Some 路径以同一已核安装结果原地切换并返回原 Arc。F 的 project_for_rebuild 仅取得/保留原 owner/gate/writer、传入原 Arc 或 None 并发布成功结果，不实现第二套 bootstrap，不提前 App 路由；失败没有新有效 Session 逸出，Git 已成功时仍按 G 保留 commit 并要求更高的新请求。

**续租与可见水位。** 慢 proof 不持 session 锁；现有合法系统 lease acquire/renew/release 可按 HubProtocol 的实际回执与当前完整 fences 继续维护系统 head，不需要先放行待证业务事实，也不入 outbox。业务 cursor/view 只沿已核连续 prefix 推进；status 的业务 seq、画布/订阅已应用水位必须取 applied_seq，不能把 hub/head.contiguous_seq 当作已应用。两者落后时保留明确的待核诊断；只读已核 view/status 与合法续租不能因另一个慢准备占锁而等待。任务写入、checkpoint 发布和 WS 事件交付仍须通过证明边界，不能借独立维护系统 head 暴露未核完成事实。

### 3.2 既成任务合并证明

`prove_merge` 必须从权威 main 可达实际 candidate commit 读取最后 trailer block，要求 Task/Run/Module/Evidence/Epoch 五项齐全且不重复。Epoch 显式等于主 Module 的 `module:<id>` fence，不能取排序后首项。Task+Epoch 是任务上下文；Snapshot+Epoch 是快照上下文；混合上下文拒绝。未知 Towermd 键聚合原文诊断，不给独立合法事实免责或阻塞。

完成证明必须同时得到以下原始、原非 stale 已接受事件，来自已核 archive 或 confirmed；对象文件可补字节，不能单独证明 producer 已接受：

| 历史事实 | 必须绑定的完整内容 |
| --- | --- |
| 原 task.claimed | 完整 Task（含未知 extra）与真实回执里的完整 Run（含 host/worktree/branch/task_card）；Task/Run 互引、完整主 Module+touches fences 及原 guards 全匹配 |
| 原 finish.started | 完整 FrozenFinish，task/run 与 claim 相同；actor固定process:towermd-verifier、host=input.host、obj=task.id、layer=L4、guards=input.fences，不能从外部请求指定producer |
| 原 finish.completed | 对应冻结输入的不可变 FinishEvidence、实际 verifier 完成记录；process:towermd-verifier、passed、真实 tree 与 commands_hash/全 fences/其他上下文一致 |
| 需要的 G1/G4 事实 | P3 原合法 App producer 的非 stale ReviewFact、精确 run/task/baseline/Plan 或 tree/全 fences 绑定，以及 P2 独立 gate Evidence；普通答复不能替代 |
| 原终态G5 | 第1.5节原非stale已接受Gate Evidence/完整Event、completed_eid与实际HEAD parents；认证真实main接入边，不猜candidate_base |

FrozenFinish 沿 P2 **全部字段**：evidence_id/project_id/host/root/common_dir/task_id/run_id/module_id/commit/tree/base/authority/baseline/plan_hash/commands/commands_hash/fences/entered_verifying_at/merge_deadline。root/common_dir 是原运行定位/审计内容，不要求恢复主机存在该路径，更不执行其中命令。固定 entered/deadline 原值，不因重启换时间；完成记录和 Evidence 必须可回溯同一输入。Task.allowed_paths/验收/相关决策与全 touches 不可从当前对象覆盖原值。

`prove_merge` 是纯历史 consumer，只认证权威 main 上既成 Git 事实，不执行命令、不检查/取得当前租约，也不提供执行权限。正常 integrator 必须在 P2 唯一既有 finish/观察流程中先检查当前全部 fences、merge_deadline、G0–G5（含必要 G1/G4）及最新 authority，再调用相同 prove_merge；任何实时门禁失败都不追加 canonical merge.done。恢复用原非 stale claim/freeze/pass 证明历史，不拿新 hub epoch 否决已经合法完成的历史，也不重建所有 renew/TTL。原接受 claim 足以证明当时确有完整租约授予；新 Run 不能继承它。缺 claim、错 touches、错 tree、failed/no_tests、只有 passed 文件或 process 文字都不 merged。

tree/基底/G0/G2/G3/G5 与高风险评审继续用 P2 唯一规则：真实 Git tree，相同 tree 或只差 state/tower.canvas 的 G5 等价；代码/config 推进要求重新变基与 finish，任务自己仍不得写受保护 state/canvas。不复制 tree 守卫。唯一纯接口在 task_gates：`parse_raw_z(raw:&[u8])->Result<Vec<RawChange>,GateError>` 与 `check_g5(catalog:&Catalog, frozen:&FrozenFinish, candidate:&str, candidate_base:&str)->Result<(),GateError>`。使用有界字节 Git 输出和 `--raw -z --full-index --abbrev=0 --no-abbrev --no-renames`；核真实 tree/mode/OID，拒绝 symlink/gitlink/不支持模式与非法路径。纯check_g5的candidate可以是尚未合并的任务提交；prove_merge另核candidate实际main可达。candidate_base不是客户端声明：实时check取最新权威main，历史proof从实际Git父链与冻结输入定位并核对任务基底；它须为candidate与当前权威main的可核祖先，不能把任务代码当state推进跳过。原 frozen.base/authority 与该实际任务基底必须有可核祖先关系，基底推进链逐提交（含 merge 的实际父边）只准 `<docs_dir>/state/` 与 `<docs_dir>/tower.canvas` 差异，并证明任务实际已变基到该新基底；仅比较两棵最终 tree 不够。任何代码/config/其他路径推进必须重新变基并 finish，即使最终 tree 巧合相同也拒绝复用。candidate 与 evidence tree 的差异亦仅准该两处；任务自身 authority→task tree 的 G2 仍硬拒绝写这两处。该函数无 now/live lease 参数，不承担实时门禁，也无 historical bool。

冷clone不能依赖同tree amend/squash或纯state/canvas rebase前已不可达的旧对象。原frozen.commit保留为原接受freeze/report的严格审计绑定；对象存在时仍核真实commit类型、tree、authority祖先与原G2，存在但类型/内容不符不得当成缺失走fallback。仅旧对象确实缺失时，由实际candidate完整树与实际frozen.authority树项精确还原两处允许变化的state/canvas（含不存在与mode），在隔离临时index重建完整Git tree OID，必须等于原已接受输入的frozen.tree。仍核candidate_base为candidate及权威main的真实祖先、原base/authority到该基底逐父边仅state/canvas、candidate自身G2及全部mode/path/blob约束；不能信自报hash、跳过代码/config推进或改原回执。临时index和生成的Git树对象只是可删除派生物，不触用户index/heads/worktree，不要求额外push旧commit，不加historical bool。candidate在main可达仍由prove_merge核；purecheck_g5仍允许尚未合main的Task提交。

canonical `merge.done` 由该证明器确定生成，eid=derived_eid(actual_commit,"merge.done")；host 取原 claim 的 Run.host，actor 固定 process:towermd-integrator，layer=L4，obj=原 Task.id，guards 为空（认证既成 Git 事实，不授予租约）。data 至少含完整 Task/Run/Evidence、actual commit/tree，以及 `proof: {schema: 1, claim: 原Event, started: 原Event, completed: 原Event, review_contexts: [第1.4节必要原ReviewContext], integration_gate: 第1.5节原终态Event}`；没有高风险需求且无所需评审时review_contexts为空。含上述原回执的最小证明上下文是业务 data，不是新 authority/注册对象/平行 receipt；相同 commit 的正常/恢复/P5/outbox builder 逐值相同：Task/Run 从原 claim 完整内容确定改为 merged/ended，Evidence 从原完成事件取值，不读取当前可变投影，不代入当前 host/lease/时间。统一归档 reader 递归校验 proof 与真实 Git；`verified:true` 没有任何授权效力。内部replay不产生第二confirmed；恢复新产生的外层只是新hub定序记录，原接受回执保留在原confirmed/已核原archive或canonicalproof（含publication实际commit内的精确原回执），不能被新外层取代。第一次恢复后新快照/归档保留这个上下文，第二次恢复即使没有旧 outbox、旧 hub/cache，也能复核：新stale外层replay与内嵌原非stale claim/freeze/pass proof各按自己的角色校验，外层不得作为第二confirmed，不因重放 stale 丢掉唯一历史依据。

## 4. P4 出版持久 schema v1

这节冻结未来 writer/证明器共用的**最小具体格式**，沿现有 registered Decision/Baseline，不新增 type。仅对该机器证明 schema 严格验必需字段；其他 OKF 未知键仍保留可读，缺/未知 schema 的旧对象只能诊断，不能自动认证出版。

路径统一相对配置 docs_dir：`decisions/publication-<token>.md` 与 `baselines/B<n>.md`；token=SHA256(approval_eid UTF-8)，publication_id 仍为原 approval_eid。固定 path builder 共用、先检查占用，不覆盖既有无关对象。manifest/Decision 是审计派生物，排除出原 FrozenDraft.files/content_hash，避免自引用；manifest 不包含尚未产生的自身 commit SHA。

Decision frontmatter：

| 字段 | 固定含义/类型 |
| --- | --- |
| type/id/layer | Decision / `publication-<token>` / L1 |
| context/options/decision/rationale | context 为 `baseline publication <target>`；options 为 `[approve, reject]`；decision 为 `approve`；rationale 原 ApprovalFact.comment（可为空） |
| affects/supersedes/class | affects 为排序后实际变更对象 IDs；supersedes 为 []；class 为 `baseline-publication`（普通 Decision 字段，无新增枚举 authority） |
| generated/verified | by 为原合法 human actor，at 为原批准 Event.t；verified 沿 OKF 列表格式；这些字段本身不能证明批准 |
| publication | `{schema: 1, publication_id, approval: ApprovalFact, approval_event: 原完整Event}`；approval_event 须原非 stale baseline.approved，obj/actor/data ApprovalFact/binding/eid 全匹配，且data须完整匹配第1.3节BaselineReviewRecorded（含原request_body/p3_request/typed p3_receipt） |

Baseline manifest frontmatter：

| 字段 | 固定含义/类型 |
| --- | --- |
| type/id/layer | Baseline / `B<n>` / L1 |
| objects | BTreeMap<object_id, sha256原文件UTF-8字节>；只含出版 payload 中的真实注册对象，不把 canvas 冒充对象 |
| approved_by/approved_at/git_tag | 与 Decision 原批准 actor/Event.t/ReviewBinding.target_baseline 完全相同 |
| generated | `{by: process:towermd-baseline, at: 原批准Event.t}` |
| publication | `{schema: 1, publication_id, approval_eid, decision_path, parent_baseline, publication_host, writer_epoch, prepared_event, binding, freeze}` |

`binding` 就是 P4 原 ReviewBinding 的全部五字段。`parent_baseline` 为前一合法精确标签名（B0 为 null）；`publication_host` 与正整数 `writer_epoch` 取下述原 prepared_event 的 host/唯一 main fence epoch，同一 candidate 不换；尚未发布而 lease/authority 已变化时按下述规则重新准备，不把旧 writer_epoch 作为现权限。`freeze` 固定为 `{project_id, draft_id, docs_dir, source_revision, target_baseline, base_directory, files, deletions, questions, risks}`：沿 P4 原类型，files 为仓库根相对 path→`{mode,sha256}` 的排序 map，deletions 排序去重；questions/risks 保留数组次序。原 payload 字节直接从实际 publication commit 的 files 路径读出，复算 mode/sha256，再按第1.2节唯一 publication_content_hash 算法重建 content_hash。这样不依赖旧 draft ref/cache，又不复制完整 payload 文本。原 draft_revision 由 binding 保留作审计；writer 出版前实际读取该 revision 并完整重核，恢复不凭名称猜测该检查成功。

实际 parent tree 的受保护 base_directory 与 freeze 相同；publication commit 的改动集须恰好为已批准 files/deletions 加这两个审计文件（排除无变化项），没有额外 L0–L2/config/代码变动。所有对象 id/path/type/删除/原文未知键、canvas、源上下文与 hash 都重核。base_directory 对纯代码 main 推进的容许仍按 P4 原规则，不改变人工绑定范围。

publication_id 等于 approval_eid；所有 manifest/Decision 两处重复值必须逐值相等。publication_host 为原 candidate writer host，不要求等于点击批准的 host。publication commit 的实际 parent 不写入人工审批摘要，但必须等于 prepared_event.data.authority_revision，其受保护目录快照必须匹配 freeze.base_directory。

commit 最后 trailer block 必须且仅一次含 Towermd-Baseline/Approval/Publication/Content-Hash/Epoch；分别等于 binding.target_baseline/approval_eid/publication_id/binding.content_hash/原 writer epoch。原 writer epoch 必须是合法正整数、等于 manifest.publication.writer_epoch 和 prepared_event 原唯一 main fence epoch；实际写入/补 tag 仍由 P4 当前有效 main lease 回查，不凭这个审计字段授权。完成 publication proof 不向新 holder 授权。普通 author、human 字样、tag 或未知 manifest 均不能替代完整原批准 Event/精确绑定。


### 内部 writer 准备回执

P4 现有计划没有可复用的原已接受 writer 准备回执；manifest 的数字与 trailer 自相匹配不够。最小补充为 `baseline.publication_prepared` 普通非 heartbeat 事件，仅唯一 writer 的内部编排 producer 生成，不新增注册 type、HTTP/CLI 操作、批准或平行 authority。它记录准备事实，不代替原 App ApprovalFact，不允许修改人批准的内容。

固定 NewEvent：actor=`process:towermd-baseline`，host=实际准备 writer host，layer=L1，obj=publication_id，guards **恰好**为当前有效 main lease 的一个完整 fence `{name:"main",holder,epoch}`。data严格采用第1.3节PublicationPrepared及其typed原请求/receipt，authority_revision为本次fresh main父commit、content_hash为原binding.content_hash。先完整重核批准/freeze/目录/target并回查当前 main lease，再经 pending fsync→hub.append→全字段核实际非 stale Event→confirmed fsync；只有拿到该已核原 Event 后才构建 candidate。原 Event 全文嵌入 manifest.publication.prepared_event，不依赖旧 outbox 留存。

准备 attempt 的 UUIDv7 request_id及按第1.3节派生的eid在本次原 pending 前预分配并持久；未确定重试只能协调该原 eid/guards/body；Hubstate仍可正常投影，确认只限制本次writer成功推进/回包。重启从已有 archive/confirmed/pending 派生定位，不另建唯一索引。authority_revision 或 main fence 真正变化需先处理旧 attempt 的 uncertain，重核原人工绑定并写新的 prepared attempt；publication_id/approval_eid 不变，新 Event 不继承或去掉旧 guards。不把旧 prepared 迁移为新权限。若 candidate 已发布到 main，只恢复其原 manifest/prepared，不再重构 candidate。

builder 要求 prepared_event 原非 stale、合法内部 producer、非 lease 系统事件，完整 main fence/host/obj/data/binding/hash 与 manifest/trailers/实际 parent 逐值一致，并经相同历史 Event 校验。即使该准备回执合法，candidate 更新 main/ref/tag 前仍必须 P4 实时回查 holder/epoch/expiry/authority/目录/target；准备后 fence 过期/更换时不得写入。已发布后另一个 holder 仅持当前合法 lease 补原 tag/审计，原 prepared 只证明历史，不给当前 holder 授权。

## 5. 唯一出版证明器与事件

唯一证明签名固定为 `prove_publication(catalog:&Catalog, history:&VerifiedRecoveryHistory, actual_commit:&str)->Result<VerifiedPublication,String>`，及 `baseline_created_event(value:&VerifiedPublication)->Result<NewEvent,String>`；VerifiedPublication 字段/构造器私有，不公开反序列化或从裸候选转换。P4 的 `PublicationProof` 只记录未认证候选/编排阶段，即使 commit_on_main=true 也不是可信输入，不能替代此证明器。缺 tag 时返回明确 pending 诊断、不产生 VerifiedPublication；补原 tag 后重核。该 builder 同时供 P4 正常发布、各崩溃窗口恢复和 E 使用：先验证实际 main 可达 publication commit、上节 Decision/manifest/trailers、合法原人工批准 producer 与原非 stale 回执，重算完整 freeze/hash/范围；再验证直接或 annotated 目标标签 peel 到**同一实际 commit**、编号/parent 链无缺口或冲突。Bare 可达 Bn 标签只是候选，不能产生 baseline.created 或成为当前基线。

- 缺 tag 的已验证 commit 只是 pending publication；当前合法 main lease holder 可以补**原** tag，补成功再构建 VerifiedPublication，不重写 main/Decision/批准或分配新 Bn。
- 正常 baseline.approved 无 guards，只要原 binding 精确仍有效，新 holder 可以持 fresh main lease 完成尚未发布内容，不要求人无故重批。原事件实际带旧 guards 且 stale 时只作历史，不续活旧批准/Run。已经合法发布的历史链可供补原 tag/审计，不给未发布的新内容批准。
- `baseline_created_event(&VerifiedPublication)` 是唯一 builder；eid=derived_eid(actual_publication_commit,"baseline.created")，actor 固定 process:towermd-baseline、host 取冻结 publication_host、guards 为空。完整data严格为第1.3节BaselineCreated，obj/request_id/result_ref与内部原响应按该表固定；P4同一builder从原commit元数据构造，normal/recovery 不注入当前恢复 host/epoch/时钟；tags 为该 commit 上已通过出版证明的排序标签集合，冲突/额外未认证 B 标签诊断，不能扩充已记录事件 body。
- baseline.created 保存足够完整的已核 publication 上下文/原批准回执（或指向仍可达的同 commit 内精确 Decision/manifest 路径），下一次恢复仅凭 snapshot/archive+Git 可重核，不依赖旧 outbox。未知/不完整链只诊断，不创造 human 批准。

## 6. 必须落地的契约测试

下面是待实现验收，不是已运行证据。成功历史夹具须明确标注 `contract history fixture`，由 core 私有测试 helper 生成匹配的原 hub 回执；不冒充真实 App 点击或真实 verifier 执行。producer 落地后对应 daemon 端到端测试仍必须完成。

| 测试名/场景 | 预期 |
| --- | --- |
| confirmed_rejects_same_eid_different_body | hub 回原 Event 但 original 任一字段/guard/data 不同，拒绝且无 confirmed |
| confirmed_fsync_failure_keeps_original_request_uncertain | pending已durable、Hub接受、confirm失败；reopen/status仍见真实Hubstate而请求uncertain，持续失败同ID不得success；解除只补原Event/原eid/guards，不重复事实 |
| archive_duplicate_coordinates_original_confirmation | Duplicate{seq}仅已有已核archive完整原Event可补confirmed，未知原Event仍uncertain，不按seq造回执 |
| claim_confirmation_failure_requires_proof_repair_before_finish_success | claimconfirm失败但status见Run后直接finish；必需claim/started/completed缺耐久证明则uncertain，修复后success并删hub可恢复真实merge |
| accepted_finish_completed_stays_recording_until_confirmation | Hub接受completed、confirm失败后JobRecording/uncertain；reopen补原Event，不重跑verifier、不新建Evidence |
| background_producers_persist_confirmed | 真实后台Hold wait_finished/attention、taskexpire产生task.released、pendingclaim协调三类producer均显式同项目outbox，reopen可核confirmed；lease系统事件未入outbox |
| duplicate_receipt_cannot_fabricate_confirmed_event | 仅 Duplicate{seq} 无匹配已核 Event 时 uncertain；有匹配原 Event 才协调 |
| recovery_digest_distinguishes_pending_confirmed_and_full_event | 状态/原 body/actual seq/time/stale/context 变化均改变 digest；跨 host 同 eid 冲突拒绝 |
| sparse_confirmed_high_water_does_not_claim_continuity | 稀疏 seq 与Event.epoch/全guards epoch最大值纳入 2^32 跳号，含非首guard最大epoch，缺口不作连续 archive 前缀；溢出拒绝 |
| existing_cli_claim_is_durable_before_finish_producer | P2 Task1真实CLI claim的原完整confirmed可重开读取，删hub后旧claim历史存在且Run不续活；未接线则Task2不开始 |
| confirmed_claim_freeze_pass_restore_git_merge_after_hub_loss | 旧 snapshot 不含三事实，仅 confirmed 原非 stale claim+完整 freeze+真实 verifier pass 及真实 Git 成功；任务 merged/Run ended/依赖后续完整 G0 可领 |
| pending_copies_of_accepted_body_do_not_prove_merge | 相同正文只在 raw pending，缺原已核 Event，不认证完成 |
| missing_claim_or_wrong_touch_tree_cannot_prove_merge | 缺 claim、次模块 fence 错、tree 错、孤立 passed 文件/process 文字分别拒绝 |
| old_guards_replay_stale_but_historical_merge_is_provable | 原已接受完整历史链可证明 Git merge；旧普通事件在新 hub stale，不复活旧 Run/权限 |
| publication_requires_original_nonstale_writer_preparation | 缺prepared原Event、仅writer_epoch数字、错main fence/host/parent/hash/binding、原stale准备均拒绝；prepare后lease失效仍不能CAS/tag；新holder重准备不迁移批准内容 |
| baseline_requires_approved_publication_not_bare_tag | bare 可达 Bn、伪 human Decision、unknown manifest、错 path/delete/hash/target/revision、错 tag peel 分别拒绝 |
| second_recovery_without_original_outbox_rechecks_canonical_proof | 首次恢复含新stale外层replay并写新快照/归档后删旧outbox/hub/cache；reader不强等外层与旧confirmed，凭内嵌原非stale canonical proof+Git复核merge/publication，无第二confirmed |
| normal_recovery_outbox_build_identical_git_events | 各入口同实际 commit 的 NewEvent 全字段/data/receipt 相等，重复恢复无重复完成/基线/需复核 |
| terminal_g5_proves_real_base_and_main_entry | 多Task commit、same-tree amend/squash、冷clone无旧commit/tree及state-only rebase后新trailers可核；缺终态/错completed或parents/旧G5/冲突回执拒绝；gate后main代码推进再revert并merge旧Task时，main接入第一父不符，不能改认其可达second-parent Task tip绕过；正常两父接入须FR-12且三入口归一同commit/eid |
| session_constructor_and_snapshot_required_reject_unproved_business_view | 真实 hub/checkpoint/归档与 Git 夹具分别覆盖 constructor、restore、SnapshotRequired、新 canonical tail；伪 proof/错 Git/裸 merged projection 均不能返回可见 merged，不能先应用再回滚；无上下文 constructor/raw seed 不提供有效入口 |
| slow_session_proof_keeps_leases_and_applied_status_truthful | 私有 I/O 屏障暂停实际 Git/归档 proof，同时合法 lease 续租与已核 view/status 可完成；head 可以前进，业务 seq 仍等于真实 applied cursor，无未核 merged/订阅事件泄露，解除后才安装已核 prefix |
| session_preparation_rejects_changed_source_ack_floor_or_inputs | 分别改变 main/private 来源、ack、floor、实际 outbox/digest 或绑定上下文；过期准备均不能 install，至多三轮；固定恢复请求不能自动扩容，公开 SavedSnapshot/cache bool 不能绕过 |
| cold_and_temporary_session_open_preserve_single_hub_ownership | 暂停真实慢 constructor，同时发起冷注册/临时读取；保留 Q1 原 gate/owner/文件锁与十并发首次注册成功回归，不出现第二 Hub；不同 clone 仍拒绝，已注册其他项目快路不等 cold gate |
| daemon_background_and_each_websocket_batch_share_proof_boundary | 真 daemon 初始成功后再加入非法 canonical/checkpoint；后台 tick、已建立 WS 后续轮次及 writer 回调均不 apply/交付未核事实，不只验证首轮；有效事实仍可经同一边界推进 |
| older_ack_floor_is_proved_from_reachable_snapshot_history | 真实 Git 成功写 103、注入 ack 失败使 hub floor/ack 保持 101；从实际双源可达旧 ack.commit 的真实 Snapshot/全归档核 floor 后应用真实 tail，不能拿 103 直接 restore 101；不可达/不符旧 ack 拒绝，Promote 例外不扩大 |
| private_recovery_seed_requires_verified_source_and_confirmation | 真实来源/inputs/history 与有效原批准/request 产生私有 staging seed；错项目/source/digest/批准/跳号拒绝；未写 Git 的 staging 仅私有消费该已核变换，target 仍严格 Git 先于 install，裸 Snapshot/RebuildSeed 无授信路径 |
| recovery_bootstrap_accepts_missing_or_empty_target_without_fake_session | Git 已有真实合法快照，分别 hub 缺失/可读空：prepare 可待批且不创建 hub/假业务 Session；合法核心确认经私有 raw target、Git 持久及已核安装后才返回有效 Arc；来源/批准/安装失败不发布，非空/损坏目标不覆盖 |
| recovery_bootstrap_reuses_registered_empty_protocol_without_reopen | init/旧 status 已注册合法空 Session 后取得合法 Git 恢复来源；确认核实际空 watermark 后安全退役并复用原 protocol/文件所有权，同一 Arc 的并发业务调用不可继续，不第二次 Hub::open、不要求重启；成功返回同 Arc，失败不恢复写服务、不自动丢非空 Session |

E0须补四种typed recorded data/meta/receipt全字段、requestID派生、review-open/gate布局负例；E1补晚G4不等待completed、已知laterPlan拒旧context、outerseq不能替原seq与二次完整context恢复；这些测试均未执行。

E0 须新增 commands/report hash 全字段及数组顺序、typed完成report/log绑定、出版10-parts字节向量、G5基底推进/代码推进最终tree巧合相同拒绝测试；E1消费同一实现，不能仅比较独立副本相等。上述所有产品测试尚未执行、producer尚未交付。

E 的 recovery_inputs/snapshot/tasks 与 recovery unit 测试承载 reader/consumer；P2 finish/task_gates，P3 human_intervention，P4 baseline_review/publish 和真实 daemon 测试分别补 producer/崩溃窗口证据。提交前检查本文与四计划引用/字段/append 顺序一致；新增格式仅此处维护，不在 TS/CLI 复制证明逻辑。
