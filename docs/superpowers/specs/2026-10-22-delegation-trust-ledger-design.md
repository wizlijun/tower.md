---
type: Design
title: 托付单、逐条验收、证据 3 类与信任账本的实现设计
description: 按核心规格 v0.7 的 FR-184、FR-185、FR-201–211 设计实现：核心新增 Delegation 投影组与读模型，开发套件经 Kit::delegations 提供任务的托付事实；App 增加委托卡、澄清、验收卡、信任账本与抽查；命令行增加 dev clarify、reply 与配置指纹。
generated: { by: claude-code/opus-5.5, at: 2026-10-22T16:00:00Z }
sources:
  - id: core
    resource: ../../specs/2026-10-21-spec-towermd-core.md
    title: Tower.md 核心规格 v0.7（82f6583）
  - id: claims
    resource: ../../../../sotvault/project/towermd/2026-10-22-product-towermd-core-claims-candidates-agent-management.md
    title: 核心功能主张清单（用户勾选全部 22 条）
  - id: owner-1022
    resource: 2026-10-22 用户在对话中的要求
    title: "22 条都已写进规格，核心规格升到了 v0.7，请根据变更实现完成"；架构与产品取舍由 Claude 自行决定（见 memory decide-architecture-myself）
status: draft
stale_after: 2027-01-22
---

# 托付单、逐条验收、证据 3 类与信任账本的实现设计

## 0. 范围

实现核心规格 v0.7 新增与修订的条款：FR-184（委托卡 5 栏）、FR-185（委托级别按受托者配置 × 工作类别）、FR-201–207（6.5 节托付与信任账本）、FR-208（逐条验收）、FR-209（证据 3 类）、FR-210（待验收积压）、FR-211（检查点变汇报请求）。22 条中其余各条的去处：C 组（附录 A.5）未排期，不实现；D 组（附录 A.6）只写进使用说明；E 组、F 组已在规格与 README 中，无代码。

这些条款在路线图里排在第 2、3 阶段，依赖的若干第 1、2 阶段能力尚未实现。本设计只补它们**必需的最小部分**，不顺带实现整个阶段：

| 依赖 | 现状 | 本设计补到哪里 |
| --- | --- | --- |
| 已声称、未确认（FR-33、FR-124） | 开发套件的 `verifying` 状态即"受托者已提交、检查未完成" | 界面把 `verifying` 显示为"已声称、未确认"；核心读模型给出 `claimed_done` 阶段 |
| 人确认完成（FR-124 第二条路、FR-138） | 不存在 | 核心事件 `acceptance.confirmed`；套件声明哪些对象可由人确认完成（开发套件的任务一律由检查完成，人确认只记为"已验收"） |
| 汇报请求与失联（FR-116、FR-137） | 不存在 | 只做检查点产生的汇报请求、`towermd reply`、守护进程写 `report.overdue`；不做 `/report` 汇报请求快捷命令和 `towermd report`、`signal` |
| 规则（FR-205） | `ProcessRule` 在守护进程被拒 | 新的核心规则记录 `rule.recorded`，按工作类别进入任务卡；不复用 `ProcessRule` |
| 兜圈子、承诺弱化（FR-125、FR-126） | 不存在 | 账本的"过程"一栏如实记为未接入（`null`），不填 0 |
| App 国际化 | App 全部是中文硬编码 | 新界面沿用中文，名称取第 13 节中文列 |

## 1. 结构：核心读模型加套件接缝

沿用 `Kit::runs()` 的做法，新增一个接缝：

```rust
trait Kit {
    // 已有：state_groups, reduce, runs, prove_protected_commit, init
    fn delegations(&self, state: &Value) -> Result<Vec<KitDelegation>, String>;
}
```

`KitDelegation` 是套件对"一次托付"的中立描述，不含开发词汇：对象 ID、标题、委托卡（`Brief`）、默认工作类别、受托者自报配置、委托时间、声称完成时间、完成时间与方式、检查证据（含绑定的版本）、自述证据、阶段（`clarifying`、`ready`、`running`、`claimed_done`、`done`）、能否由人确认完成、越界（未知为 `None`）、本轮从第几次托付开始（退回后重开会递增）。

核心新增：

- `towermd-core::delegation`：`Brief`、`Checkpoint`、`SourceKind`、`DelegateConfig`（即扩展后的 `SelfReported`）等类型与校验；新增投影组 `Delegation` 的事件归约（第 3 节）。
- `towermd-core::delegation_view`：纯函数 `view(state, kits, settings) -> DelegationView`，由事件派生出验收卡、信任账本、配置变化、抽查、待验收积压、汇报请求、级别建议。读模型不落盘，删掉后由事件重算（派生物可重建）。

守护进程在推送项目状态时附上 `DelegationView`（键 `delegation_view`），App 只渲染，不在前端重复计算规则。

**为什么这样分：**托付、验收、账本都是规格第 6.5 节的核心能力，必须对所有场景一致（放置检验 3）；但"任务"的具体形态（模块、路径、验证命令）属于开发套件。接缝只传中立事实，核心不认识 `module`、`worktree`、`harness`（`boundary.rs` 照旧守住）。

## 2. 数据形态

### 2.1 委托卡（FR-184）

核心类型，字段名取第 13 节的接口名，顺序固定：

```rust
pub struct Brief {
    pub why: Option<String>,          // 目的
    pub done_when: Vec<String>,       // 完成标准，唯一必填
    pub limits: Option<String>,       // 边界
    pub checkpoints: Vec<Checkpoint>, // 检查点
    pub self_check: Option<String>,   // 交付前自检
}
pub struct Checkpoint { pub text: String, pub due: Option<String> /* RFC 3339 */ }
```

开发套件的 `TaskDraft` 保留 `acceptance`（即 `done_when`，磁盘格式不变），新增 `why`、`limits`、`checkpoints`、`self_check`、`clarify` 五个可选键，全部 `serde(default, skip_serializing_if = 空)`，v0.1.1 的事件原样往返。`TaskDraft::brief()` 组出核心 `Brief`。扩展不能增删栏目：栏目只在核心类型里定义，套件只能填值。

G0 不变：没有完成标准时任务停在 `proposed`，不能领取。

### 2.2 受托者配置指纹（FR-203）

`SelfReported { name, version }` 增加可选的 `vendor`、`model`、`skills`（技能包版本），同样跳过空值、往返不变。指纹键是这 5 个字段的规范化 JSON。`towermd task claim` 增加 `--vendor`、`--model`、`--skills-version`。配置变化不需要单独事件：每次领取都在事件里，读模型按受托者名称列出各个指纹及首次出现时间，新指纹在账本中单独起算，并标注"配置已变化，建议提高抽查比例"。

### 2.3 证据 3 类（FR-209）

读模型中的证据条目：

```text
{ id, source_kind: check|claimed|used, criteria: [i] /* 空 = 针对整个对象 */,
  passed: bool|null, version: string|null, actor, at, note }
```

- `check`：开发套件的 finish 证据（`passed`/`failed`），绑定冻结的 tree 与合并提交。针对整个任务，不针对单条标准。
- `claimed`：受托者在 `towermd dev finish --claims <文件>` 中逐条写的自述，文件格式 `[{"criterion": 0, "note": "…"}]`。随 finish 请求进入事件（`FinishRequest.claims`，可选键）。
- `used`：人在 App 中点"我用过"产生的 `acceptance.used` 事件，署名 `human:`，可指定一条或多条标准。

### 2.4 逐条验收卡（FR-208）

卡片格式只由核心定义：每条完成标准一行，列出针对这条的证据（类别、版本、署名）；没有任何证据的行标为"未证实"。整体检查单列一行，不代替逐条证据。确认时仍未证实的条目写进 `acceptance.confirmed.unproven`，计入账本。

## 3. 事件

全部新事件写入 hub，经现有的 `append` 试归约后才被接受。署名规则：

| 事件 | 署名 | 入口 | 作用 |
| --- | --- | --- | --- |
| `delegation.clarified` | `process:towermd-cli`（带 `self_reported`） | `towermd dev clarify <task> <文件>` | 受托者复述意图、起草完成标准、提出不超过 3 个问题；只在对象处于 `clarifying` 时接受 |
| `delegation.finalized` | `human:` | App"定稿并委托" | 人定稿委托卡并回答问题；开发套件据此更新任务卡、清除 `clarify`，满足 G0 即进入 `ready` |
| `acceptance.used` | `human:` | App"我用过" | 产生 `used` 证据 |
| `acceptance.confirmed` | `human:` | App"确认完成"或"已验收" | 带 `review_ms`、`unproven`、可选 `takeaway`（FR-206，存为决策，`set_by: human`） |
| `acceptance.sent_back` | `human:` | App"退回" | 带理由与 `review_ms`；开发套件把已合并的任务重开为 `ready`，退回理由进入下一次的任务卡 |
| `rule.recorded` | `human:` | App 在退回后建议"记为规则"，人确认 | 规则按工作类别进入以后同类任务的任务卡（FR-205） |
| `work_kind.renamed` | `human:` | App 账本中改名或合并 | 改名映射，合并即改成已有的名字（FR-207） |
| `delegation.level.set` | `human:` | App 账本中设置 | 键为（配置指纹或 `*`，工作类别或 `*`）；`*`/`*` 即项目级（FR-185） |
| `report.replied` | `process:towermd-cli`（带 `self_reported`） | `towermd reply <request-id>` | 回应汇报请求，字段同 `report`：`state`、`summary`（≤100 字）、`next` |
| `report.overdue` | `process:towermd` | 守护进程维护循环 | 汇报请求过期未回应即写一条，`eid` 由请求 ID 派生，多机重复只记一次；同一对象连续 2 次过期时级别为 `active`，否则 `passive`（FR-137） |

人的事件只从 App 发出：守护进程路由要求 `x-towermd-client: app`，署名由守护进程填 `human:local`，客户端不能指定（沿用现有人工任务、回答的做法）。命令行不新增任何 `human:` 事件。

## 4. 读模型的计算规则

**托付记录。**每次领取（开发套件的 `task.claimed`）是一次托付；退回后重开再领取是同一对象的新一次托付，退回次数记在被退回的那一次上。

**信任账本（FR-202）。**按（配置指纹，工作类别）分组，取最近 `track_record_window`（默认 20）次已结束的托付：

- 表现：一次通过比例（已完成且没有被退回）；按时比例（有带时间的检查点时，交付时间不晚于最后一个检查点；没有则不计入分母，全部没有时为 `null`）。
- 过程：兜圈子次数、承诺弱化次数——两项检测尚未实现，记 `null`，界面显示"未接入"。
- 目的：满足完成标准的比例（确认时没有未证实条目）；越界次数（套件报告，未知为 `null`）。
- 实测：托付到交付的时长、App 内验收时长、退回次数，各给中位数与合计。
- 不给总分，不评价人。

**工作类别（FR-207）。**默认取套件给出的容器名（开发套件取模块 ID），再按 `work_kind.renamed` 的映射链换名；系统不自动聚类。

**抽查（FR-204）。**按事件顺序逐次计算：某次托付完成时，取该组在它之前的记录，`rate = max(spot_check_min, 1 − 一次通过数 / max(记录数, 10))`。抽签值取 `sha256(对象 ID + 完成事件 eid)` 的前 8 字节除以 2⁶⁴，小于 `rate` 即抽中。结果由事件唯一决定，重放不变。新配置没有记录，`rate` 为 1，即"抽查比例回升"。抽中后进入关注清单候选，人确认或退回即消解。`spot_check_min` 默认 0.05，在 `config.yaml` 里改；低于 0.01 的值按 0.01 处理（不降到 0）。

**待验收积压（FR-210）。**计数 = 等人确认完成的对象（套件声明可由人确认完成、且处于 `claimed_done`）+ 未消解的抽查 + 待批准的提案。超过 `review_backlog`（默认 5）时返回 `backlog: {count, threshold}`，App 显示"N 件等你验收"与"先验收再托付"；不阻止委托，不占关注清单名额。

**汇报请求（FR-211）。**委托生效（领取）时，每个带 `due` 的检查点生成一条汇报请求，ID 为 `R-<run>-<序号>`；没写时间的检查点只是停止点。受托者每次调用命令行时，未回应的请求出现在信封的 `delivered[]` 中（类别 `request_report`），直到 `towermd reply`。

**级别建议（FR-185）。**某组记录数达到窗口大小时：一次通过比例 ≥ 0.9 建议升一级，≤ 0.5 建议降一级。只给建议，级别只由 `delegation.level.set` 改变。级别清单取 `config.yaml` 的 `delegation_levels`，默认 `[report_only, answer_by_decision, approve_minor]`（规格 FR-185 的 3 个例子）；未设置的组继承项目级，项目级未设置时为清单第一项。

## 5. 命令行

- `towermd task claim … [--vendor V] [--model M] [--skills-version S]`
- `towermd dev clarify <task-id> <文件|->`：文件为 `{"restated": "…", "done_when": ["…"], "questions": ["…"]}`，问题不超过 3 个。
- `towermd dev finish … [--claims <文件>]`
- `towermd reply <request-id> --state <state> --summary <文本> [--next <文本>]`，`state` 取 FR-115 的固定词汇。
- `towermd task show` 的输出增加委托卡、该工作类别的规则、历次退回理由、未回应的汇报请求。
- 每个改动的命令更新 JSON Schema；技能 `towermd-claim`、`towermd-finish` 与 `.towermd/AGENTS.md` 模板同步说明。

## 6. App

- **委托卡**（替换现有"新建任务"表单）：5 栏固定顺序；检查点每行一条，行尾可写 `@ 2026-10-25 18:00` 作为截止时间；没有完成标准时"委托"不可用并提示补写；另有"先澄清"。积压超过阈值时，按钮旁显示"先验收再托付"。
- **澄清面板**：显示受托者的复述、起草的完成标准（可编辑）、不超过 3 个问题与回答框，"定稿并委托"。
- **验收卡**：逐条列出证据，3 类证据用不同的形状、颜色和文字标签（不只靠颜色）；"我用过""确认完成/已验收""退回"；确认时可写一句"这次改变了什么看法"。验收时长从卡片打开计到做出动作。退回后提示"把这次纠正记为规则？"。
- **信任账本**：每行一个（配置，工作类别），列出上面的实测数，写明"验收时长只计 App 内"；配置变化提示；级别选择与建议；工作类别改名或合并。
- **关注清单**：新增抽查和汇报过期两种候选；积压行显示在关注清单上方，不占 3 条名额。任务在 `verifying` 时显示"已声称、未确认"。

## 7. 兼容与验证

- 不新增依赖。磁盘格式只加可选键；v0.1.1 的重放样本继续逐字往返。重放测试比较投影时，先剔除本设计新增的空投影组与新键（列成显式清单），其余必须相同。
- 新增事件各有归约测试：署名错误、阶段不对、问题超过 3 个、完成标准为空等被拒绝。
- 读模型的抽查、账本、积压、级别建议有纯函数测试；抽查结果对同一事件序列确定。
- 守护进程路由有端到端测试：人的动作缺少 App 头被拒；`reply` 后请求不再送达；过期写入 `report.overdue` 且重复维护只记一次。
- 前端组件测试覆盖：缺完成标准时委托不可用、验收卡未证实行、3 类证据标签、积压行。
- 完成后在真实 App 中走一遍：委托 → 领取 → finish 带自述 → 合并 → 验收卡 → 我用过 → 退回 → 记规则 → 账本。
