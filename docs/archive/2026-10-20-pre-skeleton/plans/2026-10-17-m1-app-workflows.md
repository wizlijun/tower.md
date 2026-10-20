---
type: Plan
title: M1 P6 App 人工闭环集成实施计划
description: 在可运行 Tauri 基础上接入纯展示读模型、精确人工事实、真实草稿编辑及可追溯证据，完成首版 App 人工闭环。
status: draft
tags: [towermd, M1, P6, app, human, TDD]
sources:
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: docs/superpowers/plans/2026-10-17-m1-app-foundation.md
  - resource: docs/superpowers/plans/2026-10-17-m1-task-evidence.md
  - resource: docs/superpowers/plans/2026-10-17-m1-human-intervention.md
  - resource: docs/superpowers/plans/2026-10-17-m1-baseline-approval.md
  - resource: docs/superpowers/plans/2026-10-17-m1-main-observer.md
  - resource: docs/superpowers/plans/2026-10-17-m0-fix-3-snapshot-recovery.md
  - resource: docs/superpowers/plans/2026-10-17-shared-recovery-proof-contract.md
  - resource: docs/superpowers/plans/2026-10-17-m1-native-skills-acceptance.md
  - resource: docs/superpowers/plans/2026-10-17-m1-unsigned-release.md
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: docs/product-principles.md
  - resource: ../note.md/docs/okf-v0.2-format-constraints.md
  - resource: AGENTS.md
  - resource: tasks/lessons.md
---

# M1 P6 App 人工闭环集成 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从实际未签名 macOS App 查看真实态势、回答和评审、处理暂停/恢复/违规、完成 S7/S8 与任务复核，并在持续浏览时不替 Agent 续租或消费消息。

**Architecture:** Svelte 只组合展示与有限 typed 动作；Tauri 解析 canonical ProjectToken、提供本地人身份并代理固定 daemon 入口。P2–P5 与 M0 仍拥有业务事实、校验、请求 receipt、outbox、唯一证明器和 writer；本批补齐最小派生只读适配与实际界面，不复制业务规则。展示路径不进入项目 CLI 检查点或 inbox/ack，人工操作逐次绑定人已看到的精确 subject。

**Tech Stack:** 已批准的 Svelte 5、TypeScript 5、Vite 6、Tauri 2、Svelte Flow 1.6.6、vitest 4、@testing-library/svelte、happy-dom、svelte-check；现有 Rust/serde/axum/reqwest/tokio/uuid/Git。版本沿 P6 基础及 Note.md 同线，不增加未确认的新依赖、布局库、账户、认证、签名或无头 harness。

---

## 文档状态与接收门槛

本文件仅为详细实施计划，**尚未实现、未运行下述产品测试或真人验收**。规划变更只包含本文件及基础计划中与纯展示 bootstrap 冲突的最小同步，不包含当前 M0 实现工作树、源码或发布脚本；计划审查不代表产品功能已交付。

只读核对主目录 `d8d1111831cc734f183246086d56350fffc4706f`：批 3.1 已合入；P2–P5、App 基础与本计划的新接口不能因文档存在就视作交付。当前唯一源码实现者继续 M0 C/D，本批不并行修改其文件。用户已授权持续开发至首个可用未签名 App，实施交接不再问笼统执行方式；真人验收等真实可打开包和准确卡片就绪才请求。

实施分两段，避免循环依赖：Tasks1–3 的启动/纯展示段只接收对应 M0 已合入接口与 P6 基础 Tasks1–3 的桌面文件，不等完整人工 producer；它们交付后供基础 Task4 及后续回归接入。Tasks4–14 的人工闭环段必须接收 P2–P5 与完整 P6 基础实际合入 SHA、source tree、二进制 hash 和相关测试数。Task1按能力登记，未满足的后续 producer 不阻已独立可编译的启动/纯读段，也不能让未交付人工动作开放。P2 Task1→P3 Task1→P2 其余生产者的编译顺序沿总表；P4/P5 接收真实 typed 参数。某卡片的后端尚未交付时显示缺少该能力及所属 producer，禁用动作，不造默认 Approved、假 Run 或自动成功 stub。P5 frontier 按其实施前最终冻结规格接收；本批不替人裁决首次历史边界。

本批不做 Z3、M2 长文 Decision/Rule/知识回写、云 hub、MCP、自动调度、运行中 signal、豁免开关或完整 G6。M1 的 G1/G4 必要人工事实仍硬阻，不能因“完整门禁留 M2”忽略 high 风险。

## 已核缺口与最小工程收口

基础计划原 bootstrap 为固定 CLI `status`→`daemon status`→`GET /canvas`/`GET /objects/{id}`，WS 事件再触发 bootstrap。真实调用链有三处活动语义：

1. `TaskService::execute` 先绑定 Run、先 `renew_run`，再处理包括 Status 的任何 command。
2. `main.rs` 的 daemon status/stop 先 `daemon::renew_bound`；在已绑定任务 worktree 的 cwd 会发送 Status 续租。`daemon::status` 自身只 `connect(false)`，不能启动；唯一启动原语在私有 `connect(true)`，内含 single-flight、实例检测、spawn 和 FD 过滤。
3. daemon 的 canvas/object 经 `finish_read`→`renew_existing_read`；现 `/events` 每 100ms 经 `ProjectSession::subscribe`→HubProtocol subscribe/sync，后者可能结算到期。P3 项目 CLI 公共 prelude 还会先 flush 消息、再 ack delivered。

因此 App 打开任务工作树后持续刷新会延长 Agent 活动 TTL，未来还可能抢收消息。展示不是 Agent 检查点。最小修正是：

- 增加有限 `daemon ensure` 维护入口，复用 **同一个** `connect(true)`；不带项目、Run 或业务命令，不进入项目 prelude。App 由 native 在其应用数据的非 Git cwd 调用，随后 health 核验 endpoint。已有等价维护入口若在前置真正落地，优先复用并删去新增步骤。
- 固定 App projection/object/event 读适配从 daemon 已同步的 Session、权威 Catalog 和已校验归档派生。不得调用 execute(Status)、renew_binding、finish_read、inbox、delivered、HubProtocol subscribe 或借读建立新租约；正常 daemon 后台 tick/sync/expiry 继续工作，展示可能落后于最新后台水位，必须明确 through_seq。
- 保留 CLI 任意正常活动、live wait、后台验证续租原语义；不添加 `noRenew`、`skipGate`、`readOnly` 通用绕过参数。不在 Tauri 复制 daemon spawn/FD 协议，不刻意在非 Git 目录执行会报错的业务 status 再吞错启动。
- M0 missing/empty hub 的恢复只读规约优先。App 读取既有有效 snapshot 摘要与恢复状态，不创建 hub，不因持续看而复活旧 epoch。现存 corrupt hub 保留正文和诊断，不包装成可写空项目。

以上都是**未来待实现的有限维护/派生读适配需求**，不是现有 API 或第二 authority。所有新读路径须有 TTL/inbox/seq/文件副作用反例与重建测试。

## 文件地图与所有权

| 文件 | 本批唯一职责 / 前置所有者 |
| --- | --- |
| `crates/towermd/src/{main,daemon}.rs` | 维护 Ensure 与注册有限 App read routes；现有业务 CLI 语义不改；M0 实现者退出共享文件后才接手 |
| `crates/towermd/src/app_read_routes.rs`（新） | 固定只读 projection/object/events/Evidence/log 适配，组合已核 producer；不写 hub/仓库、不创建批准 |
| `crates/towermd-core/src/project_session.rs` | 极窄 `&self` 展示/事件窗口 reader；保留后台 sync/tick 唯一调度 |
| `crates/towermd-core/src/{snapshot,finish,human_intervention,baseline_draft,baseline_review,proposals,main_violation}.rs` | 分别由 M0/P2/P3/P4/P5 提供事实与 typed contract；本批仅补必要纯 reader，不改门禁/证明/写入规则 |
| `crates/towermd/tests/app_read_cli.rs`（新） | 真实 CLI/daemon 打开任务 worktree、过期、消息未消费、恢复只读边界 |
| `src-tauri/src/{cli,project,daemon_client,commands,preferences}.rs` | P6 基础文件上最小扩展；canonical token/固定 invoke/人身份/instance/原请求 envelope |
| `src-tauri/src/workflow_requests.rs`（新） | 仅内存保存本次 prepared request 的原 payload 和 generation，协调 same-ID 重试；非唯一业务状态 |
| `src-tauri/tests/{workflow_adapter,workflow_roundtrip}.rs`（新） | 固定 wire 拒绝、ProjectToken 绑定与真实 daemon 集成 |
| `src/lib/daemon/{types,wire,client,session,events}.ts` | 接收既有 typed wire、宽容展示、切项目取消与有限 invoke，不复制 schema/hash/门禁 |
| `src/lib/workflows/{mutation,review-session}.ts`（新） | inflight/uncertain/conflict 和“已展示的 subject”生命周期；不生成 actor/批准/新 guards |
| `src/lib/canvas/attention.ts`、`src/components/AttentionList.svelte` | 合并真实卡片、遵守 backend priority、首屏三项及展开全部 |
| `src/components/{AskCard,ReviewCard,InterventionBar,PauseCard,RecoveryCard}.svelte`（新） | P3/M0 人工入口，键盘/focus/明确等待与历史 |
| `src/components/{DraftWorkspace,DraftEditForm,BaselineReview,PublicationStatus,TaskRecheckCard}.svelte`（新） | P4 真 S7/S8、CAS/精准复核，草稿虚线覆盖与权威层分开 |
| `src/components/{MainViolationCard,GateFailureCard,EvidencePanel}.svelte`（新） | P5 三动作、P2 阻塞与证据来源/日志，原提交与历史不可隐去 |
| `src/components/{TaskPanel,ObjectDrawer,CanvasView}.svelte` | 复用基础下钻/固定坐标；Task merged/Run ended 只读投影 |
| `src/styles/app.css` | 全局卡片、弹层、菜单、图标、focus、主题、reduced-motion 的唯一来源 |
| `src/test/workflows.ts`（新） | 明确标自动夹具的 transport mocks；没有批准默认值 |
| `docs/verification/2026-10-17-app-workflows.md`（实施时新） | 自动/实机/真人分栏、tree 与精确 binding/receipt/source 的证据索引 |
| `README.md`、双语 CHANGELOG、`tasks/todo.md` | 仅实施后对应已验证功能、限制及精确 SHA；写作阶段不修改 |

TS 测试与同名文件相邻。组件按职责分文件；不新建平行 App、第二全局 CSS 或第二业务控制器。前置已存在等价组件/reader 时直接扩展，不为匹配本文名称搬家。

## 接口账本：已有计划契约与有限新适配

### 1. 只读展示契约

下列新 DTO 放 `app_read_routes.rs`，serde 只序列化派生结果；identity/水位必需，业务内容保留原始 JSON 和未知键。`status` 输出基础计划 `Status` 的 project_id/seq/tasks/runs（实际 Task/Run 直接序列化，不重新构造其字段）；canvas 仍使用 `CanvasView::load` 的权威结果。

```rust
pub struct AppProjection {
    pub project_id: String,
    pub revision: String,
    pub through_seq: u64,
    pub floor_seq: u64,
    pub status: serde_json::Value,
    pub canvas: serde_json::Value,
    pub attention: Vec<serde_json::Value>,
    pub diagnostics: Vec<serde_json::Value>,
}
pub struct AppEventWindow {
    pub project_id: String,
    pub through_seq: u64,
    pub floor_seq: u64,
    pub events: Vec<towermd_core::hub_protocol::Event>,
}
pub struct AppEvidenceView {
    pub project_id: String,
    pub evidence: serde_json::Value,
    pub source: EvidenceReadSource,
    pub log: EvidenceLogView,
}
pub enum EvidenceReadSource {
    Projection { through_seq: u64 },
    Archive { eid: String, seq: u64 },
    GitSnapshot { commit: String, path: String },
}
pub enum EvidenceLogView {
    Available { text: String, truncated: bool, source: serde_json::Value },
    Unavailable { reason: String, source: serde_json::Value },
}
```

枚举使用内部 tag `kind`/snake_case；此处只有读 DTO，无新 object type。`EvidenceReadSource` 必须由 P2 实际 immutable Evidence 来源定位，`log.source` 原样来自已核 verifier locator；找不到则 404/Unavailable，不能假设 Evidence 必在 main 路径。日志只读关联 Evidence 的已核本机 cache/Arca 定位；不暴露任意文件、任意 URL、通用 opener 或跨主机取文件接口。日志内容按文本显示，大小上限沿 verifier 既有上限；缺上限时本读适配固定最多 1MiB 并标 truncated，不影响原日志 hash/证据。

固定新读路径为 `GET /app/projection?directory=…`、`GET /app/objects/{id}?directory=…`、`GET /app/evidence/{id}?directory=…`、`WS /app/events?directory=…&instance=…&after=…`。object 先权威 Catalog，再经实际 producer 可证的 runtime/归档 resolver；同 ID 来源冲突明确诊断，不随便以工作树覆盖。native 从 token 解析 directory，不由 WebView指定。

App events 只读 session 已同步的连续 tail：`after < floor_seq` 返回结构化 SnapshotRequired，`after > through_seq` 返回 cursor_invalid，均重新 bootstrap，不能补造缺失历史。握手用现 WebSocket 的 instance query 规则，绑定该项目与 generation；不声称 WebView WS 可以设置不存在的自定义 HTTP header。HTTP App read 请求验证 instance；human mutation 额外沿 AppGate。若最终基础已把 WS 包在 native，复用它的固定 transport，仍不得改成任意 WS URL。

恢复状态沿 M0 独立 reader 返回其实际结构；不能为了匹配 AppProjection 强行造 seq/tasks。没有独立 recovery identity 字段就不臆造；Event.epoch 是单事件 lease fence，正常交错模块 epoch 不重置窗口。仅 endpoint instance 改变、**已证实**恢复身份改变、floor/连续水位失效才重载窗口。

### 2. Native 动作与原请求身份

每次人审阅后准备一个原请求；WebView 不提供 human_id、actor、directory、URL、method、argv 或任意 cwd。native 当前本地明确身份进入 daemon `{human_id}`，daemon 经 `x-towermd-instance` + `x-towermd-client: app` 的 AppGate 构造 `human:<id>`。M0 确认是既定例外 wire `{actor}`，由 native 同一身份拼出，不能开放 actor 参数。v0.1 这是有限流程边界，不宣称密码学认证。

```rust
// native 内部适配 DTO；不用 JSON Value 作为 mutation 入参。
pub struct Submission<T> { pub request_id: String, pub payload: T }
pub struct PreparedRequest {
    pub id: String, pub project_id: String, pub generation: u64,
    pub action: PreparedAction,
}
// PreparedAction 仅内存保存以下既有 typed payload；不作为通用 invoke 入参。
pub enum PreparedAction {
    Intervention(AppAction), // 有限 transport enum，字段逐值沿 P3 定义
    CreateDraft { baseline: String, main_revision: String },
    EditDraft { id: String, expected_revision: String,
        edit: towermd_core::baseline_draft::DraftEdit },
    Baseline { id: String, action: towermd_core::baseline_review::ReviewAction },
    Recheck { id: String, action: towermd_core::proposals::TaskRecheckAction },
    Violation { hold: String, request: towermd_core::main_violation::ViolationActionRequest },
}
```

P3 `AppAction` 当前计划位于 daemon 适配段；本native引用P3定义的完整 `Answer/Interject/PauseAll/ConfirmResume/Review` transport enum（见其Task4），不假设core已有AppAction导出。沿P3所有者的真实wire模块导入；若该模块属于binary私有，则native有限镜像这些同一字段并用实际HTTP roundtrip比较JSON，业务校验仍只在core。本例表达 prepared 内部分类与原 payload 保存要求，不要求迁移服务内部 AppGate 或 writer。P4/P5 的类型模块按真实 SHA 核对后精确导入；不同计划同名 ReviewBinding 不能混用。

固定 native invoke 的参数如下；`Submission<T>` 里的 request_id 由 native UUIDv7准备器生成并回给前端，前端只能提交该 token/generation 下已登记原请求。准备本身无业务写，实际点击才发送；持久幂等身份在 daemon 的原 receipt/outbox。

| invoke | 精确业务参数与 daemon 契约所有者 |
| --- | --- |
| `load_project(token)` / `load_object(token,id)` / `load_evidence(token,id)` | 本文只读适配；返回原来源与数据 |
| `load_interventions(token)` | P3 GET `/app/interventions`，纯读，不读 inbox/不 ack |
| `prepare_answer(token,hold,text)` / `submit_answer(token,request_id)` | P3 `AppAction::Answer {hold,text}` |
| `prepare_review(token,hold,binding,verdict,comment)` / `submit_review(token,request_id)` | P3 `ReviewBinding` + `ReviewVerdict`，原 `Review` action |
| `prepare_interjection(token,scope,text)` / `submit_interjection(token,request_id)` | P3 Scope/Interject |
| `prepare_pause(token)` / `submit_pause(token,request_id)` | P3 PauseAll |
| `prepare_resume(token,request,pause_revision,runs)` / `submit_resume(token,request_id)` | P3 ConfirmResume 精确运行集合 |
| `load_rebuild_request(token,request_id)` / `confirm_rebuild(token,request_id)` | M0 GET `/hub/rebuild/{id}` 与 POST confirm `{actor}`；不新加expiry/source字段 |
| `load_baseline_schema(token)` / `load_draft(token,id)` | P4唯一schema与durable ref |
| `prepare_create_baseline_draft(token,baseline,main_revision)` / `submit_create_baseline_draft(token,request_id)` | P4 POST `/drafts` create，原准备ID绑定参数 |
| `prepare_draft_edit(token,id,expected_revision,edit)` / `submit_draft_edit(token,request_id)` | P4 `DraftEdit`，POST `/drafts` edit |
| `review_baseline(token,id)` / `prepare_baseline_action(token,id,binding,verdict,comment)` / `submit_baseline_action(token,request_id)` | P4 ReviewView/ReviewAction；Proposal 同其批准/拒绝入口 |
| `load_task_recheck(token,id)` / `prepare_task_recheck(token,id,binding,comment)` / `submit_task_recheck(token,request_id)` | P4 TaskRecheckView/Action |
| `review_main_violation(token,hold)` / `prepare_main_violation(token,hold,action)` / `submit_main_violation(token,request_id)` | P5 ViolationSubject/Action/ActionRequest |
| `load_workflow_request(token,request_id)` | 本文有限派生receipt读入口，供重开核原请求结果 |
| `retry_workflow_request(token,request_id)` | 只定位已准备/已发送原请求，不能接受新body或动作；按原有限分支重发到同固定路由 |

P3 原 convenience invoke若已存在，扩展原请求ID/原payload协调，删去重复准备入口；不并存两套提交协议。新 native类型若引 `towermd-core` path dependency，只复用真实公共 DTO，沿已锁uuid版本线；不得为 TS生成/状态管理新增库。core 负责 body_hash、schema、run/fences、CAS与全部门禁；TS既不计算hash，也不“修正”server bindings。

P3若没有只读receipt getter，最小缺口由P3所有者在其既有派生 `p3_requests`/outbox协调状态上补 `request_view(&self,catalog:&Catalog,id:&str)->Result<Option<AppRequestView>,String>`；daemon固定 `GET /app/requests/{id}?directory=…` 只读该索引，native固定 `load_workflow_request(token,id)` 接收，不执行协调/append。`AppRequestView {request_id:String,body_hash:String,result_ref:Option<Value>,durability:String,current_validity:String,request_body:Option<Value>}`只返回原请求元数据/原body及producer已有协调器的真实durability/current validity；不存在或无法核定的字段保留None/unknown诊断，不猜confirmed/current；未知原请求404，不从当前Hold猜原结果。若实际producer保存了完整typed原request_body，native可核project/action后恢复同ID原payload；没有该正文时只展示已接受结果/uncertain诊断，禁止猜body重试，交producer补其原请求持久输入。这个缺口有明确producer归属，不新增通用workflow业务操作、第二receipt或authority。首次发送后的原ID可保存在非权威的会话记录供核对，批准/回答/唯一原body不放preferences。

请求生命周期：`idle → prepared → inflight → confirmed | uncertain | conflict | rejected`。inflight禁连点；uncertain保留原ID与原body，只允许同请求协调。新subject/内容变化409触发读刷新、清已审阅资格，**不自动换ID并重新approve、不自动换guards、不重跑verifier**。请求已被Hub接受但本机confirmed失败时，App可显示真实投影事实，同时原提交仍uncertain；不能以当前卡片状态推断原receipt已成功。关闭App不把批准/回答留成偏好唯一状态；重开从daemon已归档原receipt/outbox协调结果接收，不用localStorage复活待提交动作。

## 执行约定与提交守卫

每个 checkbox 是一项约 2–5 分钟动作；编译/实机/真人等待分开启动与核结果。每个 Task 先建可载入签名/组件 stub，**红必须是行为断言失败**，import缺失、0 tests、无工具或无网络均不算红。先针对该任务绿，再跑必要相关回归；没有变化不反复全量重测。

实施提交前读 `git diff --cached --binary` 并核原index；原有暂存不动，在独立干净工作树处理。只 stage Task 明列且已验证路径/共享hunk，完整cached diff审查后 `git diff --cached --check`；不 `git add .`，不提交其他批内容。每次精确提交登记实际SHA/命令/测试数；文档写作阶段全部保持未提交。计划内提交信息是未来实施动作，不是本轮执行授权的文件范围。

## Task 1：接收真实 producer 与冻结接口清单

**Files:** 实施时修改 `docs/verification/2026-10-17-app-workflows.md`、`tasks/todo.md` 本批段；只读前置源码/计划。

- [ ] **1.1 核前置（2–5分钟）。** 执行下列只读命令，记录真实SHA和对应文件存在性，缺producer的Task保持未开始。

```bash
git rev-parse HEAD HEAD^{tree}
git log -12 --oneline
rg --files crates/towermd-core/src crates/towermd/src src-tauri/src src
rg -n 'AppAction|ReviewBinding|TaskRecheckBinding|ViolationSubject|confirm_rebuild|append_durable|RequestReceipt|DraftEdit' crates src-tauri
```

- [ ] **1.2 冻结契约（2–5分钟）。** 验证文档填 `producer_sha / type或route / 实际签名 / readonly副作用 / consumer_task` 表；分别列P3 ReviewBinding和P4 ReviewBinding。标清本文新读adapter与所有者，不能把未来端点写成实测已有。
- [ ] **1.3 前置必要回归（2分钟启动）。** 按当前准备启动的能力运行已交付的前置：启动段只跑所用M0恢复/Session；人工段才跑P2 finish/main proof、P3 intervention、P4 baseline、P5 main observer实际既有测试；用 `rg --files crates/*/tests` 核名称，记录非零测试数与日志。错误属于前置先回对应 producer修复，不改UI放宽。
- [ ] **1.4 接收基础（2–5分钟）。** 启动段只核P6基础Tasks1–3的package/桌面manifest、project token、CLI定位和global CSS，不先要求其Task4 native读适配；人工段接收完整基础及check/build脚本，运行各段实际已有相关测试。前置真实重命名时精确更新本计划路径/导入并review；不创建第二App。
- [ ] **1.5 提交（2–5分钟）。** 仅实际接收表及任务回顾提交 `docs(app): record implemented workflow producer contracts`；没有实际运行结果就不能勾选。

## Task 2：固定 daemon Ensure，不借项目活动启动

**Files:** 修改 `crates/towermd/src/{main,daemon}.rs`、`src-tauri/src/cli.rs`；扩展 `src-tauri/tests/cli_adapter.rs`；新建 `crates/towermd/tests/app_read_cli.rs`。

- [ ] **2.1 写行为红（2–5分钟）。** 新真实测试 `ensure_is_maintenance_and_does_not_renew_or_deliver`：隔离runtime，在尚无daemon的非Git app-data cwd执行ensure，要求启动并得真实endpoint；再在同fixture创建真实M0 Run，从app-data重复ensure，要求同pid/instance且lease expiry不变。spy实际router请求不得出现command/inbox/delivered。P3交付后Task13再以真实未投递Prompt复核，不将此早期spy冒称真实P3投递证据。先可解析Ensure骨架返回running=false，红为没有启动真实daemon。
- [ ] **2.2 跑红（2分钟）。** `~/.cargo/bin/cargo test -p towermd --test app_read_cli ensure_is_maintenance_and_does_not_renew_or_deliver --locked -- --exact`，确认实际命中1项，记录行为FAIL。
- [ ] **2.3 最小实现（2–5分钟）。** daemon增加下列包装，main的维护分派在P3项目prelude之外；Tauri固定argv只新增Ensure，生产cwd来自Tauri app-data，不是用户传值。

```rust
pub fn ensure() -> Result<serde_json::Value, String> {
    let endpoint = connect(true)?.ok_or("守护进程未启动")?;
    serde_json::to_value(endpoint).map_err(|error| error.to_string())
}
// main: Command::Daemon { command: DaemonArgs::Ensure } => print_json(daemon::ensure()?)
// native CliAction::Ensure => Command::new(cli).args(["daemon", "ensure"])
```

- [ ] **2.4 拒绝与single-flight（2–5分钟）。** 原生测试无 shell、无task cwd、无run参数、bundle CLI绝对定位；两App并发ensure仅一pid，错误真实返回。CLI在任务cwd正常status仍续租/prelude，不能为了本测试删其语义；Ensure明确维护入口在所有cwd均不代替Agent活动。
- [ ] **2.5 绿与提交（2–5分钟）。** 跑app_read_cli该项、现daemon startup/FD回归及native cli_adapter，精确提交 `feat(daemon): expose fixed maintenance ensure for App discovery`；不提交全目录。

## Task 3：纯展示投影、事件窗口、对象和证据读源

**Files:** 新建 `crates/towermd/src/app_read_routes.rs`、core `app_read_model.rs`；修改daemon注册与core `project_session.rs/lib.rs`极窄reader导出；扩展 `app_read_cli.rs` 与核心session测试。native daemon_client及TS接入由基础Task4–5执行，其文件在此Task无需存在。本Task先独立交付M0+基础所需projection/object/events；Evidence读适配只接当时真实已有的M0来源，P2具体不可变Evidence/日志消费者在Task12接收真实producer后完成，不阻基础Task4开工。

- [ ] **3.1 写主要行为红（2–5分钟）。** `app_continuous_view_does_not_checkpoint_agent`：真实M0 claim后持续投影、对象、WS读取；冻结before lease expiry/原事件水位，只推进可信测试时钟越过15min并让正常daemon tick运行，要求旧Run按原规则到期，App未续租或ack、旧epoch不能继续。P3已落地才加入其真实ask deferred/Prompt/原request对照；未落地时本Task不冒充已验证P3，Task13必须补齐这部分联合链。测试不得通过status抓before/after而意外续租；在同进程daemon私有fixture读权威状态或用新纯reader。
- [ ] **3.2 运行红（2分钟）。** `~/.cargo/bin/cargo test -p towermd --test app_read_cli app_continuous_view_does_not_checkpoint_agent --locked -- --exact`；编译stub仍走旧status/read时预期TTL被延长/消息被消费断言FAIL。
- [ ] **3.3 实现纯reader（2–5分钟）。** `ProjectSession::app_event_window(&self,after:u64)->Result<AppEventWindow,AppReadError>`只检查head.floor_seq、cursor，克隆已应用tail；`app_projection(&self,catalog:&Catalog)->Result<AppProjection,AppReadError>`使用projection.objects、cursor/head与Catalog。reader DTO如core需要，放core `app_read_model.rs`以免core依赖daemon；该文件仅派生数据，AppReadError固定WindowCompacted/CursorAhead/IdentityMismatch/Unavailable，不含写入权限。按以下算法，不在reader调用sync。

```text
validate canonical project/common-dir against existing binding
freeze catalog revision and session contiguous cursor under short lock
clone Task/Run/Hold/Evidence projections; keep unknown JSON values
release lock; load authoritative CanvasView / known object bytes
if revision or project identity changed, return conflict; client rereads
return through_seq/floor_seq and honest diagnostics; no renew/inbox/ack
WS reads bounded synchronized tail, compacted window requests bootstrap
```

- [ ] **3.4 缺hub与恢复红绿（2–5分钟）。** 缺失/empty沿M0无写探测+verified snapshot返回recovery_required；corrupt为诊断，正常写不开放；uninitialized/authority_unavailable不造项目。实际读不会 `Hub::open`创建缺hub、subscribe到期或写runtime业务；冷daemon首次登记项目由既有项目生命周期先无写probe，只有已有合法hub才交给后台worker装载/sync；reader在已同步水位就绪前返回connecting/Unavailable，随后只读，不自己调用subscribe或execute。missing/empty/corrupt继续M0独立纯读，不用load读偷偷建hub或复活租约。
- [ ] **3.5 实现已有源的最小resolver（2–5分钟）。** Catalog普通对象沿现document读，M0已有Evidence只能按其真实projection/已校验归档与verifier locator接本文source/log DTO；P2尚未落地时不假称拥有完整finish/gate能力，明确capability unavailable。Task12由P2所有者提供其真实不可变Evidence reader再接全链；门禁Hold只引用实际Evidence。未知费用null不填0；缺日志显示所在host/真实原因。TS decoder保留未知kind/type/data/附加键，缺身份字段明确protocol_error，opaque条目有独立诊断不拒整页。
- [ ] **3.6 重建/窗口回归（2–5分钟）。** 删派生cache后reader从已核session/归档重建同内容；混合不同Event.epoch不重置，floor压缩/真正重建改变窗口按实际identity重载；原始历史不足显示时间窗，不补“早已完成”事件；no events polling不重扫归档。
- [ ] **3.7 绿与提交（2–5分钟）。** 跑 `cargo test -p towermd --test app_read_cli --locked`、core session reader测试和fmt/diff检查；WS测试复用现daemon私有测试的标准库TCP握手/frame读取，不引新client库。此时不要求尚未开始的基础Task4 native/TS模块；交接adapter/schema及实际SHA供其接线，提交 `feat(app): derive passive project and evidence read models`。此Task不创建人工事实或新workflow业务API。

## Task 4：有限 native 动作、canonical token 与原请求协调

**Files:** 新建 `src-tauri/src/workflow_requests.rs`、`src-tauri/tests/workflow_adapter.rs`；扩展project/commands/daemon_client；新建 `src/lib/workflows/mutation.ts` 与测试，扩展daemon client/types；如P3缺纯receipt读取，扩展core `project_session.rs/app_read_model.rs` 的派生getter与 `crates/towermd/src/app_read_routes.rs` 固定读取。

- [ ] **4.1 写边界红测（2–5分钟）。** workflow_adapter逐项检查没有选择项目/旧token/错common-dir/不同project/过期generation/远端URL/redirect/错instance均拒绝。invoke不接收human actor、任意cwd或通用文件/执行代理的path/argv；Task9中DraftEdit的合法typed相对path与VerifyCommand.argv是待审草稿数据，仍经原producer校验，不由native直接执行。可载入stub先接受旧generation，红为错项目请求被发送。
- [ ] **4.2 跑红（2分钟）。** `~/.cargo/bin/cargo test --manifest-path src-tauri/Cargo.toml --test workflow_adapter stale_project_generation_never_submits --locked -- --exact`；测试定义在此Task，先记录行为FAIL。
- [ ] **4.3 实现有限registry（2–5分钟）。** PreparedRequest以UUIDv7保存在内存map，canonical token解析得到project/common-dir/generation；prepared记录原typed payload及原项目。每个submit只能取对应variant到固定route；人身份由native选定本地身份构造，无自由actor。实际body沿P3/P4/P5/M0原wire，拒绝多余键；服务端body_hash仍只由P3核心计算。
- [ ] **4.4 写重复/uncertain红测（2–5分钟）。** mutation测试double click只有一次invoke；网络掉包uncertain后retry序列每次request_id及原JSON深相等；409触发refresh而不再次submit；项目切换取消旧response。fake transport只回已明确结果，不默认批准。

```ts
// src/lib/workflows/mutation.test.ts；fake不产生业务事实。
import { expect, test } from 'vitest';
import { createMutation } from './mutation';
test('uncertain重试保持原请求，409要求重新展示', async () => {
  const calls: string[] = [];
  const m = createMutation(async id => {
    calls.push(id);
    return calls.length === 1 ? { state: 'uncertain' as const }
      : { state: 'conflict' as const, message: '内容已变化，请重新审阅' };
  });
  await m.submit('request-1');
  await m.retry();
  expect(calls).toEqual(['request-1', 'request-1']);
  expect(m.state()).toBe('conflict');
  expect(m.canRetry()).toBe(false);
});
```

- [ ] **4.5 实现最小状态机（2–5分钟）。** `createMutation(send:(id:string)=>Promise<MutationResult>)`返回submit(id)/retry()/state()/canRetry()；MutationResult为 `{state:'confirmed',result:unknown}`、`{state:'uncertain'}`、`{state:'conflict'|'rejected',message:string}`。inflight新submit直接返回不发送；retry仅uncertain保原id；conflict清review-session展示资格，read刷新后必须新的人点击。native重试沿原已登记payload；不接受前端body覆盖。
- [ ] **4.6 接只读原请求与真实durability回归（2–5分钟）。** P3已存在等价getter优先复用，否则按接口账本实现纯request_view及固定GET/native映射；先写真实同ID receipt重开读取、不协调/append、异项目拒绝、缺原body不允许猜payload重试的行为红测，再最小接既有派生索引。本原请求元数据不得从当前Hold重构。在P3已交付故障点confirmed fsync失败，Hubstate可见而原请求uncertain；同ID持续失败仍uncertain，解除后只补原Event，原eid/guards不变且不重跑finish/verifier。恢复后原receipt另显示current validity；prefs只保存路径/viewport/关闭时间，不存问答/批准。
- [ ] **4.7 绿与提交（2–5分钟）。** 跑native workflow_adapter、TS mutation/session测试、前置P3 receipt实际回归；提交 `feat(app): bind finite workflow requests to original human submissions`。Native查不到原请求时提示从daemon已持久记录核结果，不能新ID自动重做。

## Task 5：真实关注清单与 Ask 回答

**Files:** 修改attention/AttentionList、App；新建AskCard与测试、`src/test/workflows.ts`；修改全局app.css。

- [ ] **5.1 写排序/可见红（2–5分钟）。** 使用实际P2/P3/P5 DTO录制夹具：5条真实卡首屏仅3，更多展开全部，priority按daemon15.7结果、同级等待降序；每卡有发生/影响/建议/需人动作。缺backend字段不能显示“全部健康”；未知卡只保原值/诊断，不能给批准按钮。
- [ ] **5.2 跑红（2分钟）。** `node scripts/project-pnpm.mjs test -- src/components/AttentionList.test.ts src/components/AskCard.test.ts`；stub渲染全部5条或空回答自动发送时行为FAIL。
- [ ] **5.3 实现最小卡片（2–5分钟）。** AttentionList显示前三+“查看全部 N 项”，用后端priority/shape/color/label，不按前端猜type提升授权。AskCard显示question/options/recommended/context、module/task/run、等待/deferred/expired、reply/delivery与历史来源；选推荐不自动提交，文本经明确“提交本次回答” prepare→submit。

```svelte
<!-- AskCard.svelte 中的动作形态，表单值来自用户输入。 -->
<form onsubmit={(event) => { event.preventDefault(); onAnswer(text); }}>
  <label>本次回答<textarea bind:value={text} disabled={busy}></textarea></label>
  <button type="submit" disabled={busy || text.trim().length === 0}>提交本次回答</button>
</form>
```

组件props为 `{ card: InterventionCard, busy: boolean, onAnswer:(text:string)=>void }`，`InterventionCard`按P3真实wire接收；不是从任意对象文本创建Question。expired允许后端既有归档动作时标“只归档，不恢复旧运行”，无action时不显示可提交表单。

- [ ] **5.4 历史/重复红绿（2–5分钟）。** 已回答重开不再提问；first answer冲突409刷新真实答复；迟答不复活旧epoch；答复投递是delivered不是processed。App浏览前后Agent inbox不变，只有下一正常CLI prelude输出flush后ack。无存为Decision/Rule按钮。
- [ ] **5.5 可访问性与样式（2–5分钟）。** 禁止八边形/授权菱形/错误三角形/门禁方形/评审提问圆形加颜色和文字；键盘可入卡/表单，Escape关闭抽屉恢复焦点，状态用aria-live但不循环播报。样式仅app.css，实际WebView检查；HMR未生效重启dev再验。
- [ ] **5.6 绿与提交（2–5分钟）。** vitest相关组件/attention、svelte-check，真实daemon Ask→App自动操作→CLI链单列自动证据；提交 `feat(app): show bounded attention and answer real Ask holds`。

## Task 6：精确 G1/G4 Review，而非普通问答放行

**Files:** 新建ReviewCard、`src/lib/workflows/review-session.ts` 及测试；扩展native有限review和App选择状态；`crates/towermd/src/app_read_routes.rs`对P3既有GET返回补纯派生审阅内容，P3/P2所有者提供精确源reader。

- [ ] **6.1 红测展示先于点击（2–5分钟）。** 实际Review Hold：G1展示归档plan_content、SHA256/source_commit/path与完整binding；G4展示精确tree、实际Git对象/差异来源及完整binding；已存在Evidence才关联展示，G4可先于finish，不能为渲染评审伪造Evidence或passed。还未成功载入内容、未知subject、历史only或缺binding不得点击Approved；超时保持待审。
- [ ] **6.2 跑红（2分钟）。** `node scripts/project-pnpm.mjs test -- src/components/ReviewCard.test.ts src/lib/workflows/review-session.test.ts`；stub无内容也启用按钮时必须FAIL。
- [ ] **6.3 接纯内容reader与已展示subject（2–5分钟）。** P3 GET `/app/interventions`若只返回card摘要，则由本read adapter在同返回中附Review的原 `HoldBody::Review`（包含归档plan_content/source_commit/source_path/comment）与精确绑定树的只读Git内容/差异来源；不另造Review事实或审批API。G4从binding的真实tree读取，差异参考由core从权威baseline/Git来源解析；缺少参考可显示精确tree内容与缺diff原因，不能用可变工作树代替。树不可读/无法核绑定时禁Approved并交producer诊断；不要求尚未产生的FrozenFinish/Evidence。`createReviewSession<T>()`返回display(subject:T,generation:number)/invalidate()/current():Readonly<DisplayedReview<T>>|null；只存daemon返回原typed值，不hash、不从当前Run补fences。ReviewCard明确三按钮Approved/ChangesRequested/Rejected与comment，将展示时hold/binding/verdict/comment交有限prepare_review，提交前仍由daemon精确核验。

```ts
export interface DisplayedReview<T> { generation: number; subject: Readonly<T> }
export function createReviewSession<T>() {
  let shown: DisplayedReview<T> | null = null;
  return {
    display(subject: T, generation: number) {
      shown = { generation, subject: structuredClone(subject) };
    },
    invalidate() { shown = null; },
    current(): Readonly<DisplayedReview<T>> | null {
      return shown === null ? null : structuredClone(shown);
    }
  };
}
// 断连、切项目、409、输入subject更新调用invalidate。
// 三种verdict沿P3 wire: approved / changes_requested / rejected。
```

- [ ] **6.4 TOCTOU红绿（2–5分钟）。** plan bytes/hash、tree、baseline、task/run、任何touches fence变化逐项使旧click409，刷新必须重新展示再点击；不自动retryapprove。仅SHA改写但实际tree和全部binding不变的重用由P2/P3决定，前端不自行失效/授信。普通Answer“通过”不能满足Review。
- [ ] **6.5 真实门禁联验（2–5分钟）。** native→daemon Review事实后回读原human事件/receipt；G1满足仍等G4，G4不重写原G3 Evidence.gates；拒绝/超时不解门禁，旧guards恢复只历史。UI自动动作记自动，不记真人。
- [ ] **6.6 绿与提交（2–5分钟）。** component/native review roundtrip及P2/P3真实高风险消费测试绿后提交 `feat(app): review exact plan and tree subjects before human verdicts`。

## Task 7：范围 Prompt、PauseAll 与精确 ConfirmResume

**Files:** 新建InterventionBar/PauseCard和测试；扩展native固定intervention、App composition。

- [ ] **7.1 红测范围（2–5分钟）。** Project/Module/Task从权威列表选scope，未知目标禁用；发送只影响当时活动recipients，0人明确显示0；Prompt不生成Decision或慢层批准。PauseAll显式操作，不能在打开App或关窗自动触发。
- [ ] **7.2 跑红（2分钟）。** `node scripts/project-pnpm.mjs test -- src/components/InterventionBar.test.ts src/components/PauseCard.test.ts`，stub将scope默认project或自动resume时FAIL。
- [ ] **7.3 实现有限控件（2–5分钟）。** scope值严格为P3 `Project|Module{module}|Task{task}`；textarea及“发送插话”走typed prepare/submit。Pause卡显示pause revision/active runs/投递状态；按钮文案“暂停全部领取并发出暂停提示”，解释现有harness在下次CLI检查点收到，不宣称已停进程。
- [ ] **7.4 恢复审阅（2–5分钟）。** 仅P3既有Resume Hold提供“确认恢复”；展示request、pause_revision、精确sorted runs及项目，回传完全相同集合。范围/暂停版本变化409刷新，不能补入新run自动确认；批准不清G2/G3/G5/Review Hold、不启动harness、不acquire旧epoch。
- [ ] **7.5 端到端绿（2–5分钟）。** App pause→新claim拒绝；CLI resume仅建请求；App展示后确认才开放新领取；两种harness下一正常CLI有前置消息，App自身不ack。不调用kill/signal/运行器控制；相关vitest/native/P3测试绿后提交 `feat(app): expose scoped prompts and explicit pause resume confirmation`。

## Task 8：M0 恢复请求 GET 与 App 确认

**Files:** 新建RecoveryCard及测试；扩展nativecommands/daemon_client/workflow_adapter；复用M0 F/G真实daemon测试。

- [ ] **8.1 红测真实来源（2–5分钟）。** RecoveryCard仅接实际M0 RecoveryRequest，显示request_id、项目、source_reference/commit/seq/epoch高水位、main_revision、inputs_digest、requested_at/expires_at与跳号边界。无saved request显示CLI `hub rebuild`生成请求的指引，不能由App payload自造source/expiry或自动执行。
- [ ] **8.2 跑红（2分钟）。** `node scripts/project-pnpm.mjs test -- src/components/RecoveryCard.test.ts` 与native recovery adapter过滤，stub缺请求也显示确认、刷新延长expiry时FAIL。
- [ ] **8.3 最小固定接线（2–5分钟）。** native `load_rebuild_request(token,id)` GET已登记请求并验证其saved项目；`confirm_rebuild(token,id)`仅向既定ID路由发送 `ConfirmRebuildBody {actor: native human}`。App不传directory/source/expiry/nonce、不create批准JSON；HTTP过期/source变化/旧instance409，缺请求404，禁止client403如实展示。
- [ ] **8.4 到期/安装失败（2–5分钟）。** 倒计时只禁过期点击，不重新prepare续命；必须新CLI请求和新ID，旧卡留审计。来源main/private/outbox变化需新请求；G安装失败“边界已持久须新请求”显示commit并保持写拒绝，不能旧ID重stage。缺失/empty可恢复与corrupt须处理分清，不把恢复required变空ready。
- [ ] **8.5 真daemon绿（2–5分钟）。** 停夹具daemon→删夹具hub保留Git/outbox→App纯读→CLI请求→App程序确认→旧guards stale→新epoch；M0 F/G故障回归逐项运行。持续读TTL/inbox不变、读不创建hub；确认是明确human事实且恢复历史答复可读不自动继续。
- [ ] **8.6 提交（2–5分钟）。** native/组件/M0相关绿后提交 `feat(app): confirm saved hub recovery requests without extending scope`；实际真人确认尚未发生须仍未完成。

## Task 9：S1–S6 交接与真正 S7 草稿编辑

**Files:** 新建DraftWorkspace/DraftEditForm及测试，扩展CanvasView草稿overlay、native P4固定适配；全局CSS。

- [ ] **9.1 红测不造扫描/S6（2–5分钟）。** 项目无B0展示技能交接；用已有会话输出提供的真实scan ID，经M0批4既定 `GET /drafts/{id}`读取queued/running/完成结果及ScanInput，P4提交后同固定入口优先durable draft ref。先核该GET为纯读，不经finish_read/renew/inbox；若实现仍有活动语义，由本文固定read adapter调用其无副作用reader，不复制扫描器。不猜scan列表或另发扫描API，也不能App调用业务CLI无限刷新。展示 `baseline scan --wait <实际id>`/唯一schema/S6提交指引，S6由已有harness完成；App不启动模型、不替人合成fake draft。
- [ ] **9.2 跑红（2分钟）。** `node scripts/project-pnpm.mjs test -- src/components/DraftWorkspace.test.ts src/components/DraftEditForm.test.ts`，可载入stub将rename仅改本地标题/未保存仍标已保存时FAIL。
- [ ] **9.3 创建/载入真实草稿（2–5分钟）。** 已提交S6 draft从P4 GET durable ref载入id/revision/source/target/questions/risks/对象；后续baseline创建使用实际baseline+main_revision+原request_id。UI显示保存中/本地已保存远端待推/已保存；不以localStorage当草稿，不自动切主干。
- [ ] **9.4 实现合并与拆分表单（每种2–5分钟）。** Merge输入至少两Module、保留ID/title；Split输入parts{id,title,paths,verify,acceptance_ids,x,y}和interface_endpoints逐端映射，P4拒绝缺项就显示精确错误，前端不猜映射、不默默丢内部接口。操作只提交P4 DraftEdit原typed值；以下是使用既定参数的最小请求，实际不在TS重写schema。

```ts
// DraftEdit是P4真实Rust wire的transport类型；不含第二schema/hash规则。
const merge: DraftEdit = { action: 'merge', modules: ['A', 'B'], keep: 'A', title: '合并模块' };
const split: DraftEdit = { action: 'split', module: 'A', parts: [
  { id: 'A1', title: '读侧', paths: ['src/read/**'], verify: [], acceptance_ids: ['AC1'], x: 0, y: 0 },
  { id: 'A2', title: '写侧', paths: ['src/write/**'], verify: [], acceptance_ids: ['AC2'], x: 300, y: 0 }
], interface_endpoints: { I1: ['A1', 'C'] } };
// prepare_draft_edit(token,id,displayed_revision,merge或split)，明确点击才submit。
```

- [ ] **9.5 实现其余真实操作（一次一种2–5分钟）。** Rename仅title；Move只草稿node安全整数x/y；PutInterface表单提交DraftObject{id,path,text}+from/to，RemoveInterface显式选id；ConfirmVerify提交module+commands[{cwd,argv}]，不在编辑时执行；AddDocref提交node/path/x/y，引用只来自source_revision真实普通md。Unknown OKF/canvas键原样显示保留；opaque目标不能编辑但不挡其它节点。
- [ ] **9.6 CAS与草稿布局（2–5分钟）。** 每次保存提交expected_revision，成功使用新revision重新载入；409显示别人改动并重新审阅，不last-write-wins、不自动重新apply edit。权威画布坐标不随快层变；草稿overlay虚线，只有草稿拖动或新baseline批准切换可改其布局。初始布局复用基础既有逻辑，不新引布局库。
- [ ] **9.7 完整真实草稿回归（2–5分钟）。** table驱动8个DraftEdit变体每项native→P4 save→GET字节/引用变化；关App、删派生cache、daemon重启后原draft ref/revision内容仍在；两个App相同revision只一成功。main/用户docs/index/已有Markdown字节不变。UI不仅返回成功，还对照实际合并/拆分/接口/坐标结果。
- [ ] **9.8 绿与提交（2–5分钟）。** 组件/native/P4 draft实际回归与check/build绿，提交 `feat(app): edit durable baseline drafts through typed S7 operations`；真人S7留未验收。

## Task 10：S8 精确批准拒绝、出版状态与 TaskRecheck

**Files:** 新建BaselineReview/PublicationStatus/TaskRecheckCard及测试；扩展nativeP4适配与App路由。

- [ ] **10.1 写S8行为红（2–5分钟）。** 展示P4 ReviewView全部path before/after/deletions、hash/object_id/draft_revision/target/current baseline、verify/风险/问题、接口两端高亮；内容未读不能批准。Rejected不写main/tag；Approved不等于baseline.created。stub点击即在前端切B0时FAIL。
- [ ] **10.2 跑红（2分钟）。** `node scripts/project-pnpm.mjs test -- src/components/BaselineReview.test.ts src/components/PublicationStatus.test.ts src/components/TaskRecheckCard.test.ts`，保存行为FAIL。
- [ ] **10.3 实现S8与出版状态（2–5分钟）。** submit原P4 ReviewBinding五字段+verdict/comment/requestID；409 invalidate刷新再审。Publication Pending/Committed/Tagged/Recorded分开，只有唯一VerifiedPublication的baseline.created才切权威基线；tagfail补原tag不另批/另编号。拒绝保留草稿和原审计。
- [ ] **10.4 main检出指引（2–5分钟）。** 无remote main任意worktree检出时pending，显示writer真实原因、可复制但不执行的切离main指引，先让人核dirty和绑定。App不得checkout/reset/stash、动index、借private snapshot使B0生效；后续标签pending仍同publication身份协调，不自动重发新批准。
- [ ] **10.5 TaskRecheck精确解除（2–5分钟）。** 展示TaskRecheckView的Task完整text、reasons、task_content_hash/target/publication_ids、running绑定。ready且running=None可直接复核，不要求先claim；running绑定当前run及全fences。明确按钮“确认此任务适用于当前基线”，提交原binding/comment；不借G1/G4 Review。
- [ ] **10.6 复核竞态红绿（2–5分钟）。** Task内容、target、publication集合、ready→running、任一fence变化409需重审；只清原publication_ids差集，后到标记保留，不清其它gate/证据失败，不完成Task或signal harness。同ID重试原receipt，旧running guards恢复只历史。
- [ ] **10.7 真实端到端绿与提交（2–5分钟）。** 从S6→S7→拒绝→main不变→新revision新审阅批准→main检出pending→夹具操作者切离→同publication B0 tag；后续Proposal B1→ready/running精确复核→CLI恢复相应gate。跑P4/native/组件相关测试，提交 `feat(app): approve exact baselines and resolve scoped task rechecks`。

## Task 11：门禁失败与 main 违规的精确三入口

**Files:** 新建GateFailureCard/MainViolationCard与测试；扩展nativeP5固定适配和EvidencePanel入口。

- [ ] **11.1 门禁红测（2–5分钟）。** P2真实G0/G2/G3/G5 Evidence/Hold显示gate、commit/tree、拒绝原因/路径/日志及具体下一步。dirty指回任务树保留修改；no_tests指向P4批准verify+补测试；ReviewBlocked指精确Review。没有“强行通过/忽略/清门禁”按钮，不由App随便重跑finish。
- [ ] **11.2 违规红测（2–5分钟）。** P5卡片显示原commit/parents、完整diff及protected subset/hash/object IDs/authority/current/target与处理状态；无任何Run项目也能显式转提案。运行 `node scripts/project-pnpm.mjs test -- src/components/MainViolationCard.test.ts src/components/GateFailureCard.test.ts`，fakeRun或只显示摘要就允许Claim的stub应FAIL。
- [ ] **11.3 接已有三动作（2–5分钟）。** 从review_main_violation取得ViolationSubject，分别准备GenerateRevert、Propose{subject,input:ProposalInput}、Claim{subject,comment}；Propose表单必须理由/影响/备选/建议，来源固定MainViolation原commit/hash/hold。仅P5 typed MainViolation caller路径，不自动claim任务、不伪造Run/fences。
- [ ] **11.4 精确审阅与反馈（2–5分钟）。** 点击前完整展示subject，main/hash/IDs/target变化409刷新需重审；撤销生成新commit保留原历史，冲突/父选择不明显示blocker；转提案显示未批准/未撤销，拒绝Proposal原违规仍在；认领只批准精确原commit/diff，不自动B标签或后续commit。成功后原Hold/原因/diff可回看。
- [ ] **11.5 持久与恢复回归（2–5分钟）。** 原Claim已接受后普通code推进与hub恢复仍保原精确事实；未接受旧view在main推进后409。无Run转提案同request同Proposal/ref，恢复不补Run；配置/state违规不能借慢层认领放行。复用P5同receipt/outbox/writer，不新加业务API。
- [ ] **11.6 绿与提交（2–5分钟）。** P5实际 `app_action_`/native/组件与P2 gate相关测试绿，提交 `feat(app): review gate failures and exact main violation actions`。

## Task 12：Evidence/日志/来源与真实合并态势

**Files:** 新建EvidencePanel及测试；修改TaskPanel/ObjectDrawer、canvas projection；复用Task3纯读resolver。

- [ ] **12.1 红测来源与完成（2–5分钟）。** passed Evidence但Task仍verifying不计merged；Agent文本done不计进度；正常canonical merge.done后Task=merged、Run=ended才显示已合并。恢复同proof不生成假历史。null费用显示“未知”，混合已知/未知总费用不得显示精确完整总价。
- [ ] **12.2 跑红（2分钟）。** `node scripts/project-pnpm.mjs test -- src/components/EvidencePanel.test.ts src/components/TaskPanel.test.ts src/lib/canvas/projection.test.ts`；stub按passed或done文本增加进度时FAIL。
- [ ] **12.3 接收P2 reader并实现证据面板（2–5分钟）。** 先将Task3的read adapter接到真实P2 immutable projection/已校验归档reader与verifier log locator，补有效/缺来源/异项目/失效schema/本机日志不可用的native行为红绿；不得假设Evidence一律在main路径。再直接显示真实Evidence id/kind/task/run/module/commit/tree/base/authority/baseline/commands_hash/fences/result/generated.by/gates；未知schema保原JSON明确诊断。source显示projection through_seq/归档eid seq/真正Git快照commit path；log显示原locator/hash/host/内容或Unavailable，不执行文本、不按任意URI打开。
- [ ] **12.4 绑定与可见窗口（2–5分钟）。** 当前有效/历史only分开呈现；G3不可变passed不因App G4变true，另显示独立Review事实。只展示daemon已提供的窗口/已核历史，不假补开始/结束。检查点/原成本/自报harness版本明确沿来源，不猜缺epoch/费用/关键路径。
- [ ] **12.5 与P7收集一致（2–5分钟）。** App加载不运行status/task show/inbox/ack；P7 collect继续只归并已捕获原文/Git，不为了收证主动调用本新endpoint。App实机日志可作为单独已捕获只读来源，保source/through_seq/hash；不能让collect成API轮询器或Agent续租者。
- [ ] **12.6 绿与提交（2–5分钟）。** pure read相关/native/组件绿及真实main proof→App状态对照，提交 `feat(app): expose immutable evidence sources and certified merge state`。

## Task 13：真实 daemon 联合回归与 WebView 自动验收

**Files:** 新建 `src-tauri/tests/workflow_roundtrip.rs`，扩展app_read_cli/组件contracts测试；实施verification文档自动证据段。

- [ ] **13.1 建真实fixture（2–5分钟）。** 复用前置真实CLI/daemon helper、隔离runtime/HOME、bare origin、clone和任务worktree。所有Task/Run/Hold/Evidence/Publication经实际producer，不直接塞hold.answered/merge.done。故障用既有cfg(test)内部clock/barrier，不加生产skip开关。自动AppGate身份用明确test标识，报告“自动程序边界”。
- [ ] **13.2 按行写与跑红（每行2–5分钟）。** 先可运行测试调用实际native/service，缺连接/错误状态应行为FAIL；已有前置case复用不重写业务算法。

| 实际测试名（本Task新增） | 必须证明 |
| --- | --- |
| `passive_view_never_renews_or_delivers` | task worktree打开持续刷新+WS，不续TTL不偷Prompt，后台仍过期，旧epoch拒绝 |
| `ask_review_pause_resume_roundtrip` | Ask回答/精确G1G4/Prompt/Pause/范围Resume→真实CLI prelude，拒绝/超时不通过 |
| `draft_s7_s8_roundtrip_preserves_user_tree` | 八种DraftEdit真实保存、两App CAS、拒绝、B0/B1/pending，用户index/docs不变 |
| `task_recheck_ready_running_roundtrip` | ready无Run可解除、running全fences精确、后到publication保留 |
| `main_violation_without_run_roundtrip` | 项目从无Run三动作来源精确，Propose不fakeRun、原历史保留 |
| `request_uncertain_retries_original_payload` | confirmed失败原结果可见/原请求uncertain，同ID原body协调不重跑verifier |
| `recovery_saved_request_and_consumed_boundary` | M0 GET/confirm exact ID/expiry/source，安装失败新边界，缺/损坏不假开放 |
| `events_instances_and_epochs_are_distinct` | 正常epoch交错不重置；实例/floor/实际恢复identity变化重新bootstrap，旧generation丢弃 |
| `evidence_sources_and_merge_projection_roundtrip` | 真Evidence/log/source，passed不等merged，canonical merge.done→Task merged/Run ended |

- [ ] **13.3 联合绿命令（2分钟启动）。**

```bash
~/.cargo/bin/cargo test -p towermd --test app_read_cli --locked
~/.cargo/bin/cargo test --manifest-path src-tauri/Cargo.toml --test workflow_adapter --test workflow_roundtrip --locked
node scripts/project-pnpm.mjs test
node scripts/project-pnpm.mjs check
node scripts/project-pnpm.mjs build
~/.cargo/bin/cargo test --workspace --locked
~/.cargo/bin/cargo fmt --all -- --check
git diff --check
```

记录实际非零测试数/日志；desktop独立workspace不因root测试通过就算覆盖。前置producer变化后重跑相应contract及联合受影响项，不无故循环全量。

- [ ] **13.4 自动WebView核验（2–5分钟）。** 在真实dev WebView与打包候选分别做显示/键盘/focus/Escape/窄窗口/dark/light/reduced-motion；快层任务/中断事件前后节点x/y/width/height一致，首屏3且可展开全部。HMR异常重启dev再验。自动点击生成的test human事实只记自动测试，不进真人栏。
- [ ] **13.5 精确提交（2–5分钟）。** 验证文档写source tree、前置SHA、实际commands/count、binding/原request/receipt/outbox source、失效与未完成项；提交 `test(app): verify native human workflows against real daemon producers`。未实机/未真人分别保留空缺，不凭截图推断后端事实。

## Task 14：源码外 App、真人入口及 P7/P8 同 tree 交接

**Files:** 实施时更新verification、README、双语CHANGELOG未发布条目、tasks/todo本批回顾；复用P6 build-app/P8构建，不新增发布器。

- [ ] **14.1 构建与身份（2分钟启动）。** 使用P6已交付 `node scripts/build-app.mjs --target aarch64-apple-darwin` 或P8同tree候选入口，记录source SHA/tree、App/内置CLI/hash、架构与codesign实际结果。本文不再实现DMG/双架构发行脚本；P8负责冻结tag和公开回读。
- [ ] **14.2 源码外运行（2–5分钟）。** 复制候选 `.app`到实际记录的临时非源码目录，停止Vite，从Finder打开；最小系统PATH补查无需Node/pnpm/Rust。固定Ensure从app-data启动daemon/health，项目可为绑定task worktree但持续看不续TTL/收消息；缺Git/验证工具有操作诊断不改全局配置。
- [ ] **14.3 生命周期/断连（2–5分钟）。** 记录真实daemon pid/instance；关闭所有窗口、退出App，已有后台执行仍在；重开连接同项目。停/重启只对隔离夹具做，断线显示最后成功时间/旧状态过期，重连不偷偷重播approve，切项目旧响应不能写新项目。Apppreferences无唯一业务事实。
- [ ] **14.4 准备具体真人入口（2–5分钟）。** 给出真实App路径、验收项目、draft id/revision、待答Hold/Review/rebuild request ID、需要看什么及点击后的预期真实事件/commit/tag；此时才请真人操作。先提供可审阅内容，再批准；自动化不代点后冒称真人。恢复请求过期需新CLI请求及重新展示，不按等待时长自动放行。
- [ ] **14.5 真人操作分项记录（每项2–5分钟）。** 真S7合并/拆分/改名/拖动/接口/verify/docref保存重开；S8先拒绝确认main不变，再新revision批准B0；后续Proposal B1与ready/running复核；Ask/G1/G4/Prompt暂停范围恢复；违规diff三入口按可执行验收卡；M0恢复确认。每项记录操作者观察、实际human事件及精确binding/request/receipt/commit/tag前后。没有点击就写“真人未完成”，不能把后端自动测试当替代。
- [ ] **14.6 P7/P8接收（2–5分钟）。** P7真实三工作树/双harness十任务使用该候选功能tree、同CLI/schema/技能hash，P6证据与P7 collect captured原文关联；P8候选R/tag若代码tree变化，列出补丁影响并重新验受影响功能，不机械复用旧UI截图。保留同tree追溯链，不在本计划凑十任务或重复发布资产脚本。
- [ ] **14.7 文档与提交（2–5分钟）。** README写真实入口和pending/uncertain/recovery/no_tests/历史only；双语未发布各新增用户可感知人工闭环变化且不声称已发布；todo记录精确提交/自动实机真人区别。仅已验证hunk提交 `docs(app): record workflow acceptance and release handoff`，交P7/P8；未提交明确写未提交。

## 覆盖矩阵、独立 review 与完成界限

| 冻结要求 | 落点与必需证据 |
| --- | --- |
| 展示不是检查点，正常CLI活动不变 | Tasks2–3/13 passive_view；TTL/inbox/delivered/epoch真实对照 |
| 真实关注首屏≤3、全部可展开、15.7编码 | Task5/13 WebView；后端priority、形状+颜色+文字 |
| Ask归档、不自动Decision、迟答不续活 | Tasks5/13；真人Task14，原Reply来源 |
| G1/G4 exact subject/verdict、变化409重审 | Task6；P2消费、不可变G3与独立Review |
| Prompt/PauseAll/精确范围ConfirmResume | Task7；不signal、旧scope拒绝、下一CLI投递 |
| gate失败/禁止、main精确三入口、无Run提案 | Task11；P5原diff/Subject/typed MainViolation/receipt |
| M0请求GET+确认、expiry/source与安装边界 | Task8；F/G真实回归、读不开放缺/损坏hub |
| S1–S6已有harness、真正S7、expected_revision/CAS | Task9；durable ref、八种edit、真实结果、cache删除恢复 |
| S8 hash/target/revision批准拒绝与B0/后续pending | Task10；原ApprovalFact/commit/tag/唯一baseline.created |
| ready/running TaskRecheck精确标记解除 | Task10；无fakeRun、差集/newpublication保留 |
| Evidence/log/source、费用null、canonical merge.done | Tasks3/12；Task merged/Run ended、无假历史 |
| finite native/AppGate/human、请求UUIDv7/原body/uncertain | Task4及全动作；无任意URL/path/cwd/command/CLIhuman |
| canonical ProjectToken/generation、epoch≠恢复identity | Tasks3–4/13；切项目取消、未知字段不臆造 |
| CSS唯一、快层固定坐标、WebView/Finder/关窗不stopdaemon | Tasks5/9/13–14；实机日志与坐标 |
| 自动/真人/P7/P8同tree、未发布/未提交明确 | Tasks13–14；不复制发行脚本、不虚构十任务 |

写作自审：检查OKF frontmatter与相对链接、全部future API的责任者、P3/P4同名type区分、细粒度动作/测试命令和行为红、TS无schema/hash/门禁复制、实现与发布边界。开工接收实际SHA后的签名调整必须先精确改计划并review；未合入producer不是补假后端的理由。

- [x] 独立审查本计划和只读bootstrap最小收口：PASS，无Critical/Important；统一恢复确认归属与typed草稿参数措辞，当前不能视为已实施。
- [ ] 实施后每Task具备行为红/绿、精确提交SHA及真实来源；结构/链接检查不能替代产品测试。
- [ ] 自动组件/native/真实daemon、实际WebView/源码外App、真人点击分列；未真人保持未完成。
- [ ] P7真实双harness十任务和P8冻结双架构发行/公开回读独立完成，才能宣布首版App交付。

当前结论：**详细计划已通过独立审查；生产源码、测试、依赖、App构建、真人验收和发布均未执行。** 实施必须按上文接收实际producer与适配接口SHA。
