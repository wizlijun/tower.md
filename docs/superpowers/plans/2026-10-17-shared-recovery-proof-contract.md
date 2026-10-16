---
type: Plan
title: E/P2/P3/P4 共享恢复证明契约 v1
description: 冻结 confirmed 回执、历史证明输入、任务合并与基线出版的唯一证明边界及最小持久格式。
tags: [towermd, plan, recovery, proof, M0, M1]
generated: { by: codex/gpt-6, at: 2026-10-17T00:00:00Z }
sources:
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-3-snapshot-recovery.md
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
- 同 eid 的 original 逐值一致才可去重；同 eid 异 body，或**原接受回执角色**的 actual seq/time/stale/其他字段冲突均拒绝。archive 中的原接受回执与 confirmed 必须完整相等。恢复新 hub 的内部 replay 直接走 staging HubProtocol.append，**不调用 producer append_durable/persist_confirmed，不写第二条 confirmed**；它的新 seq/time/stale 是外层 replay 信封，与旧 confirmed 是不同角色，reader 不将两者强等。reader 按已核 recovery_gap/恢复边界识别外层 replay，核其 original 全业务字段/原 guards，内嵌 canonical proof 的原非 stale Event 独立按原接受角色核验；不能用新外层回执替换或“修正”原回执。

- legacy renew 输入丢弃；新入队拒绝 lease.renewed，其他 lease 操作仍只走协议专用接口。不保存全部 renew 来重建 TTL。confirmed 的稀疏 seq 最大值，以及每个原 Event.epoch 与该 Event **全部 guards 的 epochs** 的最大值，必须加入 prepare 的高水位（不能只取首 guard epoch，批量 grant 会逐项递增），再 checked_add(2^32)；不得把缺口伪装成连续前缀或只按 snapshot 旧高水位跳号。
- prepare/request 绑定全 inputs.digest；确认时输入变化必须重新请求。恢复重发 pending/普通历史事件时保留原 eid/guards，旧 fence 在新 hub 仍 stale，不复活旧 Run。confirmed 只证明过去被接受，不给新 hub 当前权限。


### core 组合入口与现有 claim 接线依赖

E 交付 `pub(crate) fn append_durable(hub: &mut dyn HubProtocol, outbox: &mut RecoveryOutbox, original: NewEvent, now_ms: u64) -> Result<AppendReceipt, HubError>`，位于 core 的 outbox 组合模块；普通业务 append 全部执行上面的顺序，pending/confirmed I/O 或未知 Duplicate 为 Uncertain/Io，不返回虚构 Event。Duplicate 可先查 outbox 已核 confirmed；只有 archive 的回执由 core 同一已核历史 reader 交给原请求协调器核验后调用 persist_confirmed，再重试。没有完整原回执就保持 uncertain。

此 helper 不控制 session 投影；Hub 耐久接受后连续 sync/广播/快照照常进行，即使本机 confirmed fsync 失败。只有本次 mutation 成功回包/幂等成功返回/Job Completed 等待确认耐久；不得因 confirmed 缺失停顿连续投影。P3 的 append_p3_event 与 P4 现有writer只是业务封装，必须调用它，不再重复实现三阶段持久逻辑。E 交付该 helper/reader 的核心测试，不把它当已完成 daemon claim 接线。

P2 Task1 在现有 `ProjectSession::execute(catalog,host,command,run,now)` 和其 `TaskService::execute(hub,catalog,host,command,run,now)` **尾部显式增加** `outbox: &mut RecoveryOutbox`；相应 tasks::execute 包装与 daemon 调用一起更新。daemon 按已核项目打开外部 runtime outbox 后传入，TaskService 所有普通业务 append（尤其 task.claimed 的完整 Task/Run/全 fences）调用 E helper；lease acquire/renew/release 仍委托原 HubProtocol 专门方法，不把离线pending当租约。confirmed失败时本次调用不宣告claim成功，但错误收尾sync与未来status仍可观察真实Hub已接受的Run；协调后沿原task/run/eid返回，不能重新领取/新建Run掩盖uncertain。


后台与协调签名也必须接线：`ProjectSession::tick(host,now,outbox)`、`ProjectSession::renew_binding(catalog,host,run,now,outbox)`、`TaskService::expire(hub,host,now,outbox)`、`TaskService::reconcile_pending(hub,now,outbox)`、`TaskService::renew_binding(hub,catalog,host,run,now,outbox)` 及 `tasks::expire/renew_binding` 包装尾部显式传同项目 `&mut RecoveryOutbox`；其内部 release_run/compensate_claim 等会产生 task.released 的普通producer也传同outbox、调用唯一helper。daemon 后台tick、快照前协调、请求前续租/到期检查及待定claim协调逐个传入，不依赖未来自动wrap。lease acquire/renew/release 的系统事件保持专用协议，不入outbox。P3 的 `ProjectSession::human_tick(host,now_ms,outbox:&mut RecoveryOutbox)->Result<(),String>` 同样显式注入同项目outbox，hold.wait_finished/hold.attention 的后台producer经append_p3_event→唯一helper；固定自动请求ID/原eid先持久，不因tick重试重复结算。

本机请求协调器从outbox派生 durability=uncertain/confirmed（可删除重建，非业务authority）。Hub中原 p3_receipt/result_ref 保留真实已接受结果，不因本机fsync失败改写。mutation同ID重试先核原project/request_id/body_hash冲突，再**在查询/返回幂等成功receipt之前**协调该请求原pending与真实原Event补confirmed；持续fsync失败仍返回uncertain，解除只补原Event、不重做业务。Duplicate只有已核archive/confirmed里完整匹配的原Event才可协调，不用seq或当前投影捏造。status展示Hubstate与本机durability是分开的事实。同ID协调优先定位已核原接受回执（archive/confirmed/canonicalproof中的原Event），不为已确认旧请求再次调用新hub append；恢复外层replay绝不能交persist_confirmed。仅有新外层而缺原Event时保持原接受结果未知，不能捏造旧success。请求自身补confirmed持续fsync失败仍uncertain；下述依赖可采用已核真实archive，不等于绕过请求自身成功门槛。

关键依赖窗口：claim confirmed失败而status已见Run后直接finish，freeze/finish成功路径必须先协调必需原claim；finish.started与finish.completed同样逐个满足耐久证明（原confirmed或已核真实archive），否则该finish请求仍uncertain，不能声称成功。只补此请求依赖的证明，不停顿全前缀。finish.completed已被Hub接受但confirm失败时Job保持Recording/uncertain，重启/重试补原Event确认、不重跑verifier、不新建Evidence或改结果。最终success前完整claim/started/completed证明均可恢复。

P2 Task2 的 record_finish 同样显式增加 `outbox: &mut RecoveryOutbox`，finish.started/completed 经该 helper。P2 Task1 必须先完成真实二进制CLI→daemon claim、drop/reopen后检查原confirmed完整回执、仅删hub保留Git/outbox并经真实App确认恢复的测试；确认原claim为历史且旧Run无新权限。Task2 再跑真实 claim→freeze→verifier pass→Git merge→hub丢失恢复链，缺已接线claim不得以手填Run或仅freeze/pass夹具算端到端成功。E早期契约历史夹具不能替代这些producer验收。

## 3. 唯一历史输入与任务证明

`VerifiedRecoveryHistory` 字段与构造器私有，不实现任意 JSON 的公开反序列化入口。唯一构造路径汇合：批 3 统一双源 snapshot/archive reader（含连续前缀、项目与 checkpoint/全归档一致性校验）和第 2 节已核 confirmed。正常 session 的已核连续 tail 也经同一校验构建只读历史。raw `StateProjection`、孤立对象文件、raw pending 和自报 verified 不能获得这个类型。

```rust
pub struct VerifiedRecoveryHistory { /* private: verified events/context/indexes */ }
pub fn prove_merge(catalog: &Catalog, history: &VerifiedRecoveryHistory,
    input: &MergeProofInput) -> Result<VerifiedMerge, String>;
// VerifiedMerge/VerifiedPublication 的字段私有；只由相应证明器构造。
```

`prove_merge` 必须从权威 main 可达实际 candidate commit 读取最后 trailer block，要求 Task/Run/Module/Evidence/Epoch 五项齐全且不重复。Epoch 显式等于主 Module 的 `module:<id>` fence，不能取排序后首项。Task+Epoch 是任务上下文；Snapshot+Epoch 是快照上下文；混合上下文拒绝。未知 Towermd 键聚合原文诊断，不给独立合法事实免责或阻塞。

完成证明必须同时得到以下原始、原非 stale 已接受事件，来自已核 archive 或 confirmed；对象文件可补字节，不能单独证明 producer 已接受：

| 历史事实 | 必须绑定的完整内容 |
| --- | --- |
| 原 task.claimed | 完整 Task（含未知 extra）与真实回执里的完整 Run（含 host/worktree/branch/task_card）；Task/Run 互引、完整主 Module+touches fences 及原 guards 全匹配 |
| 原 finish.started | 完整 FrozenFinish，task/run 与 claim 相同；其 guards 等于全 fences |
| 原 finish.completed | 对应冻结输入的不可变 FinishEvidence、实际 verifier 完成记录；process:towermd-verifier、passed、真实 tree 与 commands_hash/全 fences/其他上下文一致 |
| 需要的 G1/G4 事实 | P3 原合法 App producer 的非 stale ReviewFact、精确 run/task/baseline/Plan 或 tree/全 fences 绑定，以及 P2 独立 gate Evidence；普通答复不能替代 |

FrozenFinish 沿 P2 **全部字段**：evidence_id/project_id/host/root/common_dir/task_id/run_id/module_id/commit/tree/base/authority/baseline/plan_hash/commands/commands_hash/fences/entered_verifying_at/merge_deadline。root/common_dir 是原运行定位/审计内容，不要求恢复主机存在该路径，更不执行其中命令。固定 entered/deadline 原值，不因重启换时间；完成记录和 Evidence 必须可回溯同一输入。Task.allowed_paths/验收/相关决策与全 touches 不可从当前对象覆盖原值。

正常路径仍按当前有效 fences/期限及硬门禁认证；恢复用原非 stale claim/freeze/pass 证明历史，不拿新 hub epoch 否决已经合法完成的历史，也不重建所有 renew/TTL。原接受 claim 足以证明当时确有完整租约授予；新 Run 不能继承它。缺 claim、错 touches、错 tree、failed/no_tests、只有 passed 文件或 process 文字都不 merged。

tree/基底/G0/G2/G3/G5 与高风险评审继续用 P2 唯一规则：真实 Git tree，相同 tree 或只差 state/tower.canvas 的 G5 等价；代码/config 推进要求重新变基与 finish，任务自己仍不得写受保护 state/canvas。不复制 tree 守卫。

canonical `merge.done` 由该证明器确定生成，eid=derived_eid(actual_commit,"merge.done")；host 取原 claim 的 Run.host，actor 固定 process:towermd-integrator，layer=L4，obj=原 Task.id，guards 为空（认证既成 Git 事实，不授予租约）。data 至少含完整 Task/Run/Evidence、actual commit/tree，以及 `proof: {schema: 1, claim: 原Event, started: 原Event, completed: 原Event, reviews: [必要原Event]}`；没有高风险需求 reviews 为空。含上述原回执的最小证明上下文是业务 data，不是新 authority/注册对象/平行 receipt；相同 commit 的正常/恢复/P5/outbox builder 逐值相同：Task/Run 从原 claim 完整内容确定改为 merged/ended，Evidence 从原完成事件取值，不读取当前可变投影，不代入当前 host/lease/时间。统一归档 reader 递归校验 proof 与真实 Git；`verified:true` 没有任何授权效力。内部replay不产生第二confirmed；恢复新产生的外层只是新hub定序记录，原接受回执保留在原confirmed/已核原archive或canonicalproof（含publication实际commit内的精确原回执），不能被新外层取代。第一次恢复后新快照/归档保留这个上下文，第二次恢复即使没有旧 outbox、旧 hub/cache，也能复核：新stale外层replay与内嵌原非stale claim/freeze/pass proof各按自己的角色校验，外层不得作为第二confirmed，不因重放 stale 丢掉唯一历史依据。

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
| publication | `{schema: 1, publication_id, approval: ApprovalFact, approval_event: 原完整Event}`；approval_event 须原非 stale baseline.approved，obj/actor/data ApprovalFact/binding/eid 全匹配，并携 P3 原 request_body/p3_request/p3_receipt |

Baseline manifest frontmatter：

| 字段 | 固定含义/类型 |
| --- | --- |
| type/id/layer | Baseline / `B<n>` / L1 |
| objects | BTreeMap<object_id, sha256原文件UTF-8字节>；只含出版 payload 中的真实注册对象，不把 canvas 冒充对象 |
| approved_by/approved_at/git_tag | 与 Decision 原批准 actor/Event.t/ReviewBinding.target_baseline 完全相同 |
| generated | `{by: process:towermd-baseline, at: 原批准Event.t}` |
| publication | `{schema: 1, publication_id, approval_eid, decision_path, parent_baseline, publication_host, writer_epoch, prepared_event, binding, freeze}` |

`binding` 就是 P4 原 ReviewBinding 的全部五字段。`parent_baseline` 为前一合法精确标签名（B0 为 null）；`publication_host` 与正整数 `writer_epoch` 取下述原 prepared_event 的 host/唯一 main fence epoch，同一 candidate 不换；尚未发布而 lease/authority 已变化时按下述规则重新准备，不把旧 writer_epoch 作为现权限。`freeze` 固定为 `{project_id, draft_id, docs_dir, source_revision, target_baseline, base_directory, files, deletions, questions, risks}`：沿 P4 原类型，files 为仓库根相对 path→`{mode,sha256}` 的排序 map，deletions 排序去重；questions/risks 保留数组次序。原 payload 字节直接从实际 publication commit 的 files 路径读出，复算 mode/sha256，再按 P4 Task1 唯一 parts 算法重建 content_hash。这样不依赖旧 draft ref/cache，又不复制完整 payload 文本。原 draft_revision 由 binding 保留作审计；writer 出版前实际读取该 revision 并完整重核，恢复不凭名称猜测该检查成功。

实际 parent tree 的受保护 base_directory 与 freeze 相同；publication commit 的改动集须恰好为已批准 files/deletions 加这两个审计文件（排除无变化项），没有额外 L0–L2/config/代码变动。所有对象 id/path/type/删除/原文未知键、canvas、源上下文与 hash 都重核。base_directory 对纯代码 main 推进的容许仍按 P4 原规则，不改变人工绑定范围。

publication_id 等于 approval_eid；所有 manifest/Decision 两处重复值必须逐值相等。publication_host 为原 candidate writer host，不要求等于点击批准的 host。publication commit 的实际 parent 不写入人工审批摘要，但必须等于 prepared_event.data.authority_revision，其受保护目录快照必须匹配 freeze.base_directory。

commit 最后 trailer block 必须且仅一次含 Towermd-Baseline/Approval/Publication/Content-Hash/Epoch；分别等于 binding.target_baseline/approval_eid/publication_id/binding.content_hash/原 writer epoch。原 writer epoch 必须是合法正整数、等于 manifest.publication.writer_epoch 和 prepared_event 原唯一 main fence epoch；实际写入/补 tag 仍由 P4 当前有效 main lease 回查，不凭这个审计字段授权。完成 publication proof 不向新 holder 授权。普通 author、human 字样、tag 或未知 manifest 均不能替代完整原批准 Event/精确绑定。


### 内部 writer 准备回执

P4 现有计划没有可复用的原已接受 writer 准备回执；manifest 的数字与 trailer 自相匹配不够。最小补充为 `baseline.publication_prepared` 普通非 heartbeat 事件，仅唯一 writer 的内部编排 producer 生成，不新增注册 type、HTTP/CLI 操作、批准或平行 authority。它记录准备事实，不代替原 App ApprovalFact，不允许修改人批准的内容。

固定 NewEvent：actor=`process:towermd-baseline`，host=实际准备 writer host，layer=L1，obj=publication_id，guards **恰好**为当前有效 main lease 的一个完整 fence `{name:"main",holder,epoch}`。data 为 `{publication_id, approval_eid, binding: 原ReviewBinding, authority_revision: 本次fresh main父commit, content_hash: 原binding.content_hash}` 加 P3 同一内部请求的完整 request_body/p3_request/p3_receipt。先完整重核批准/freeze/目录/target并回查当前 main lease，再经 pending fsync→hub.append→全字段核实际非 stale Event→confirmed fsync；只有拿到该已核原 Event 后才构建 candidate。原 Event 全文嵌入 manifest.publication.prepared_event，不依赖旧 outbox 留存。

准备 attempt 的 UUIDv7 eid/request_id 在本次原 pending 前预分配并持久；未确定重试只能协调该原 eid/guards/body；Hubstate仍可正常投影，确认只限制本次writer成功推进/回包。重启从已有 archive/confirmed/pending 派生定位，不另建唯一索引。authority_revision 或 main fence 真正变化需先处理旧 attempt 的 uncertain，重核原人工绑定并写新的 prepared attempt；publication_id/approval_eid 不变，新 Event 不继承或去掉旧 guards。不把旧 prepared 迁移为新权限。若 candidate 已发布到 main，只恢复其原 manifest/prepared，不再重构 candidate。

builder 要求 prepared_event 原非 stale、合法内部 producer、非 lease 系统事件，完整 main fence/host/obj/data/binding/hash 与 manifest/trailers/实际 parent 逐值一致，并经相同历史 Event 校验。即使该准备回执合法，candidate 更新 main/ref/tag 前仍必须 P4 实时回查 holder/epoch/expiry/authority/目录/target；准备后 fence 过期/更换时不得写入。已发布后另一个 holder 仅持当前合法 lease 补原 tag/审计，原 prepared 只证明历史，不给当前 holder 授权。

## 5. 唯一出版证明器与事件

私有 `VerifiedPublication` builder 同时供 P4 正常发布、各崩溃窗口恢复和 E 使用：先验证实际 main 可达 publication commit、上节 Decision/manifest/trailers、合法原人工批准 producer 与原非 stale 回执，重算完整 freeze/hash/范围；再验证直接或 annotated 目标标签 peel 到**同一实际 commit**、编号/parent 链无缺口或冲突。Bare 可达 Bn 标签只是候选，不能产生 baseline.created 或成为当前基线。

- 缺 tag 的已验证 commit 只是 pending publication；当前合法 main lease holder 可以补**原** tag，补成功再构建 VerifiedPublication，不重写 main/Decision/批准或分配新 Bn。
- 正常 baseline.approved 无 guards，只要原 binding 精确仍有效，新 holder 可以持 fresh main lease 完成尚未发布内容，不要求人无故重批。原事件实际带旧 guards 且 stale 时只作历史，不续活旧批准/Run。已经合法发布的历史链可供补原 tag/审计，不给未发布的新内容批准。
- `baseline_created_event(&VerifiedPublication)` 是唯一 builder；eid=derived_eid(actual_publication_commit,"baseline.created")，actor 固定 process:towermd-baseline、host 取冻结 publication_host、guards 为空。完整 data、内部 request_body/receipt 按 P4 同一 builder 从原 commit 元数据构造，normal/recovery 不注入当前恢复 host/epoch/时钟；tags 为该 commit 上已通过出版证明的排序标签集合，冲突/额外未认证 B 标签诊断，不能扩充已记录事件 body。
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

E 的 recovery_inputs/snapshot/tasks 与 recovery unit 测试承载 reader/consumer；P2 finish/task_gates，P3 human_intervention，P4 baseline_review/publish 和真实 daemon 测试分别补 producer/崩溃窗口证据。提交前检查本文与四计划引用/字段/append 顺序一致；新增格式仅此处维护，不在 TS/CLI 复制证明逻辑。
