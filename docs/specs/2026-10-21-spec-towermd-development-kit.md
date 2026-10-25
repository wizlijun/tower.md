---
type: Spec
title: Tower.md 开发套件把 git、代码模块与 harness 接到核心，不改核心规则
description: Tower.md 开发套件规格 v0.6，由规格 v0.5 拆出：第一方开发套件（Development Kit）负责软件开发场景，包括六层对象、仓库布局与对象字段、分支与合并、模块租约与路径守卫、冷启动扫描源码、一句话生长的代码骨架、Tower.md 技能包、harness 适配器、无头双环与合并队列、门禁 G1–G5、授权与预算等中断、Arca 大文件；它经核心的接入接口工作，不改变核心的注意力、信任与命名规则。
tags: [towermd, spec, 开发套件, git, 模块, 基线, 门禁, 技能包, harness, 无头模式, 双环, 合并队列, Arca, 冷启动]
generated: { by: claude-cowork/opus-5.5, at: 2026-10-21T11:30:00Z }
sources:
  - id: core
    resource: 2026-10-21-spec-towermd-core.md
    title: Tower.md 核心规格 v0.6
  - id: v05
    resource: ../archive/2026-10-21-pre-core-split/2026-10-16-spec-towermd-layered-agent-canvas-mvp-v0.5.md
    title: Tower.md 规格 v0.5 全文（本文条款的原文）
  - id: boundary
    resource: ../../../sotvault/project/towermd/2026-10-21-product-towermd-boundary-core-extensions-emergent.md
    title: 产品边界文档（4.2 节"第一方开发套件"、第 7 节调整建议、附录归属总表）
  - id: attention
    resource: ../../../sotvault/project/towermd/2026-10-21-spec-towermd-attention-core-participation-contract.md
    title: Tower.md 注意力需求（FR-121 的出处）
  - id: owner-1910
    resource: 2026-10-21 19:10 用户对产品边界文档第 10 节的决定
    title: 接受"核心规格 + 开发套件规格"的拆分
status: draft
stale_after: 2027-01-21
---

# Tower.md 开发套件把 git、代码模块与 harness 接到核心，不改核心规则

| 项 | 内容 |
| --- | --- |
| 名称 | Tower.md 开发套件（Development Kit），第一方扩展中的"套件"类型 |
| 版本 | 开发套件规格 v0.6 草案，由规格 v0.5 拆出[^v05] |
| 姊妹文档 | [核心规格](2026-10-21-spec-towermd-core.md)：名词、注意力、画布与容器、信任底线、接入接口、hub 与快照[^core] |
| 读者 | 实现负责人；实现 Agent |

**编号约定。**本文保留 v0.5 的节号和 FR 编号，便于对照 v0.5、计划和验收记录；缺号的节和条款在核心规格中，去处见核心规格附录 B。本文中的"核心规格 X 节"指核心规格的节号。

## 修订记录

| 日期 | 修订 |
| --- | --- |
| 2026-10-21 | v0.6：按用户决定从 v0.5 拆出。v0.5 中与代码、git 和 harness 有关的条款原文移入本文；场景无关的条款移入核心规格。条款内容未改，只改了指向核心规格的引用。[^owner-1910] |

## 0. 摘要

**开发套件是 Tower.md 的第一个落地场景，但不定义核心。**Apple 用自己的框架做 Xcode；Xcode 很重要，但它不是 iOS。M1 已经做出来的东西，大部分属于开发套件。[^boundary]

开发套件把核心的名词落到软件开发上：

| 核心名词或概念 | 开发套件中的落地 |
| --- | --- |
| 项目 | 一个初始化了 Tower.md 的 git 仓库 |
| 任务 | 归属于一个模块、可独立验证的工作单元，带 `allowed_paths` |
| 完成标准 | 验收标准（`Acceptance`），附模块验证命令 |
| 受托者 | 在 harness 会话里加载 Tower.md 技能的 Agent；M3 起也可以是无头运行 |
| 信号 | 门禁结果、路径守卫拦截、授权与预算中断、harness 错误 |
| 版本 | 基线 `Baseline`，以 Git 标签 `towermd/B<n>` 标记 |
| 容器 | 模块（`Module`）：绑定一组代码路径和验证命令的人定容器 |

开发套件包括：六层对象与模块（第 3 节）；仓库布局与对象字段（第 6 节）；分支、工作树、合并与 trailer，模块租约与路径守卫（第 7 节）；冷启动扫描源码（第 8 节）；一句话长出代码骨架（第 9 节）；Tower.md 技能包、harness 适配器、无头双环与合并队列（第 10 节）；门禁 G1–G5（第 11 节）；授权、预算、harness 错误等中断与总控代答（第 12 节）；Arca 大文件（6.4 节）。

开发套件经核心的接入接口工作（核心规格第 5 节），受核心的注意力调度、信任底线和署名规则约束。它可以把核心的两类归属再细分为六层，但界面上仍只显示两类（核心规格 FR-188）；本文的行话（baseline、gate、lease、hold、harness、slow layer）只出现在开发者接口和本文中（核心规格 NFR-14）。

## 2. 术语

这些词只出现在开发者接口和本套件中，不出现在界面上（核心规格 NFR-14）。

| 术语 | 定义 |
| --- | --- |
| 层（layer） | 对象的变化速度类别，共 L0–L5 六层，见第 3 节 |
| 慢层 | L0–L2。只能由人批准改变 |
| 中层 | L3–L4。由总控（controller）在授权内决定，并留下记录 |
| 快层 | L5。由执行 Agent 自主推进，只产生事件 |
| 模块（module） | 架构中的一个组件，拥有一组路径（path globs）和验证命令 |
| 基线（baseline） | 人批准过的一组慢层对象版本，用 Git 标签 `towermd/B<n>` 标记 |
| 提案（proposal） | 修改慢层的申请，附影响分析，等待人批准 |
| 任务（task） | 归属于一个模块、可独立验证的工作单元 |
| 运行（run） | 执行某个任务的一次尝试。v0.1 中一次领取就是一次运行，运行 ID 由 `towermd task claim` 生成；M3 中是一次无头 harness 调用 |
| 检查点（checkpoint） | 运行中产生的提交，打断时最多损失到上一个检查点 |
| 证据（evidence） | 验证结果，如测试、构建、评审，绑定到具体提交 |
| 总控（controller） | 外环。负责规划、派发、回答内环问题、分类错误；M3 引入 |
| 集成器（integrator） | 串行合并队列的执行者，负责变基、验证、合并；M3 引入 |
| 租约（lease） | 由 hub 发放、带有效期的独占权，如"一个模块同一时间只有一个写任务"。每次发放带递增的 `epoch`，旧 `epoch` 下的写入标为过期、不计入进度 |
| hub | 活跃状态的唯一服务：给事件定序、发放租约、向各端实时推送。单机时由本机守护进程担任，多机时由用户自部署的 Cloudflare Worker 担任 |
| 快照（snapshot） | 主干租约持有者把 hub 当前状态和新归档事件提交到 `main` 或 FR-91 的私有引用中对应用户文档目录的一次写入，用于版本管理与重建 |
| 用户文档目录（docs_dir） | 存放慢层 md 文档、`tower.canvas` 与快照的目录，`towermd init` 时由人或 Agent 指定，默认 `docs/tower/` |
| 主干租约 | 全局唯一的写 `main` 权：持有者负责提交慢层变更、基线和快照；M3 起也负责合并队列 |
| 技能包（skill pack） | 约束 harness 工作流程的一组技能，例如 superpowers；按版本锁定在仓库中 |
| 交互点 | 技能包中会向人提问、等待评审或批准的步骤 |
| 中断（hold） | 运行因需要人或总控回应而暂停，类型见第 12 节 |
| 实时等待 | 中断期间 harness 进程保持运行，等待回答 |
| 停车续跑 | 中断期间 harness 结束回合并退出，回答后用会话 ID 续跑 |
| 门禁（gate） | 任务生命周期中的固定检查点，未通过时阻塞或告警 |
| 插话 | 人在运行中向 Agent 注入一条提示词 |
| 临时规则 | 带范围和到期时间的指令，注入相关任务卡 |

## 3. 六层对象、三类权限

核心对用户只呈现"人定的"和"受托者推进的"两类（核心规格 FR-188）；开发套件在内部把它们细分为下面六层。L0–L3 对应"人定的"，L4–L5 对应"受托者推进的"。FR-01–03 的条款在核心规格 3.6 节。

Tower.md 用一个 `layer` 字段区分对象的变化速度，再用三类权限决定谁能改。层的划分沿用前期研究中建筑六层到软件的映射。[^layered][^design]

| 层 | 名称 | 典型对象 | 权限类 | 变更等级 |
| --- | --- | --- | --- | --- |
| L0 | 目标与约束 | 意图、预算上限、合规约束 | 慢层：人批准 | 主要变更 |
| L1 | 架构基线 | 模块、接口、模块路径、验证命令 | 慢层：人批准 | 主要变更 |
| L2 | 规格与验收 | 验收标准、设计规范、对外接口契约 | 慢层：人批准 | 主要变更 |
| L3 | 流程与工具 | harness 配置、提示模板、检查规则 | 中层：总控提议，人登记 | 次要变更（需登记） |
| L4 | 模块计划 | 任务清单、依赖、优先级 | 中层：总控自批 | 次要变更（自批并记录） |
| L5 | 任务与提交 | 运行、提交、日志、证据 | 快层：执行 Agent | 不审批，只记事件 |

## 4. 开发场景的用户旅程

本节是核心规格 4.2、4.4 节在代码仓上的实现。日常查看与提问的旅程（v0.5 的 J3、J5）在核心规格 4.3、4.5 节。

### 4.1 J1 冷启动：现有仓库 → 基线 B0 → 开跑

1. 用户在已有 Git 仓库中执行 `towermd init`，或让 Agent 用 Tower.md 技能初始化。系统询问用户文档目录（默认 `docs/tower/`），创建 `.towermd/` 配置。
2. 系统运行冷启动管线（第 8 节），在 App 的画布上画出候选架构图：模块、接口、健康色、未分配文件。
3. 用户在画布上合并、拆分、改名、拖动模块，补充或删除接口。
4. 用户点击"批准为 B0"。系统把慢层对象和 `tower.canvas` 提交到用户文档目录，并打标签 `towermd/B0`。
5. 用户输入一个目标，或从候选任务中勾选几项，生成任务。用户在一个或多个工作树、一台或多台机器上开 harness 会话，Agent 用技能领取任务，执行开始。

### 4.2 J2 一句话生长

1. 用户在空画布上输入一句话，例如"做一个把会议录音转成待办的命令行工具"。
2. 系统最多问 3 个澄清问题，用户可以跳过。
3. 系统生成最小骨架：1–3 个模块、每个模块的验收标准、不超过 7 个初始任务。第一个任务是能跑通的空骨架。
4. 用户一键批准 B0，执行开始。
5. 生长信号出现时（第 9 节），系统提出新增或拆分模块的提案；用户批准后生成 B1、B2……

### 4.4 J4 重构：保留需求，重画架构

1. 用户在画布上进入"草稿基线"模式，画出新的模块和接口。
2. 系统对比当前基线与草稿，列出受影响的需求、接口、任务和代码路径。
3. 用户批准后，新基线生效，画布平滑过渡到新布局。
4. 总控按新基线重新生成任务；旧代码标为"参考"。

这一流程对应明豪讨论中"需求不变，重新整成几个大的架构图，再让 Agent 全部重构"的设想。[^mh-note]

## 5. 系统架构

本节描述 M1 的组件。hub、快照与署名（v0.5 的 5.2–5.4 节）在核心规格第 10 节。

### 5.1 组件

下图是 v0.1 的组件和数据流。单机时不需要 Cloudflare：本机守护进程自己担任 hub。

```mermaid
flowchart LR
  subgraph HOST["每台主机"]
    S["harness 会话（加载 Tower.md 技能）"] -->|"towermd 命令行"| D["守护进程"]
    APP["Tower.md App"] <-->|"127.0.0.1 的 HTTP 与 WS"| D
    D -->|"快照（持有主干租约时）"| GIT["本地 git 仓库"]
  end
  D <-->|"WSS（多机时）"| HUB["hub：Cloudflare Worker，每个项目一个 Durable Object"]
  GIT <-->|"push 与 fetch"| REMOTE["git 远端"]
```

技能只调用命令行；命令行只和本机守护进程通信；守护进程是本机唯一连接 hub、唯一写快照和慢层提交的进程；App 只连本机守护进程。

| 组件 | 职责 | 关键约束 |
| --- | --- | --- |
| Tower.md 技能包 | 引导 Agent 按流程领取任务、提问、提交证据、提提案、收尾（10.0） | 只调用 `towermd` 命令行；不直接写用户文档目录、快照或 `.towermd/` |
| `towermd` 命令行 | 技能与人的命令入口 | 经本机守护进程读写；不提供任何批准类命令（FR-95） |
| 守护进程 | hub 客户端（或本地 hub）；本机投影索引；验证器；持有主干租约时写快照与慢层提交；向 App 推送 | 只绑定 127.0.0.1；对外只有到 hub 的出站连接；首次调用命令行时自动启动 |
| 本地 hub | 单机时给事件定序、发放租约 | 数据放在 git 公共目录下，同一 clone 的所有工作树共享（FR-85） |
| Cloudflare hub | 多机时给事件定序、发放租约、广播 | 用户自部署；按项目 ID 区分项目（FR-97） |
| Tower.md App | 渲染画布、实时显示状态、接收人的批准与干预 | Tauri 桌面应用；批准只能从 App 发出（FR-95）；关闭 App 不影响 Agent |
| 代码仓（git） | 代码、慢层对象、`tower.canvas`、状态快照、基线标签 | 慢层以 `main` 为准（FR-83） |
| 大文件存储（Arca） | 运行原始输出、日志、截图、构建产物 | 按相对路径原样访问；只由守护进程写入主仓库目录 |
| 总控、调度器、运行器、集成器、交互网关 | 无头执行的双环、合并队列与 MCP 通道（第 10 节） | M3 引入，作为守护进程的组件，复用同一 hub 与技能 |

### 5.5 进程模型（开发套件部分）

FR-04、FR-05 在核心规格 10.5 节。

- **FR-113** 一台主机上可能有多个终端，每个终端里又可能有多个 Agent，各自在不同分支、不同工作树上干活。命令行按调用时的当前目录确定调用者：取 `git rev-parse --show-toplevel` 得到工作树，取当前分支，在 hub 中查找绑定到"本机 + 该工作树 + 该分支"的运行（FR-112）。找到就作用于该运行；找不到时，需要运行的命令（`ask`、`propose`、`check`、`finish`、`trailers`）报错并列出本机的活动运行。任何命令都可用 `--run <id>` 显式指定运行，覆盖自动判定。一个工作树同一时间最多绑定一个活动运行；不依赖终端级的环境变量，因为同一终端里的多个 Agent 共用环境。
- **FR-06** v0.1 的工作树由用户或 harness 自己创建，Tower.md 不要求特定布局。M3 的运行器把每个无头运行放在独立的 Git 工作树中，工作树目录不进入版本控制。

### 5.6 技术选型

技术选型与 Note.md 同构，完整清单与新增依赖规则见仓库根目录的 `AGENTS.md`。要点：

- 核心：Rust。一个与平台无关的核心 crate，守护进程与命令行、Tauri App、M3 的 MCP 交互网关是它的适配层。
- 守护进程：Rust（tokio 管理子进程与网络；clap 做命令行；axum 提供本机 HTTP 与 WebSocket）。理由：单二进制常驻、占用低，与 `notemd` 的 CLI 和 MCP 实现同构。
- App：Tauri 2 + Svelte 5，节点图用 `@xyflow/svelte`（Note.md 已在使用），初始布局用 elkjs。
- Cloudflare hub：TypeScript Worker 加 SQLite Durable Object，wrangler ^3、`@cloudflare/workers-types` ^4、vitest-in-workerd，与 Note.md 的 mdrelay 相同。
- 派生索引：SQLite（rusqlite）。
- Git：直接调用 `git` 命令，不依赖托管平台。

新依赖：axum 已确认；hub 客户端的 WebSocket 库类别已确认，具体库在 M2 计划中写明理由。人类签名与系统钥匙串访问的库随安全加固再定。

10.3 中的适配器接口用 TypeScript 写成，只为示意字段；实现为 Rust trait。

## 6. 数据模型与仓库布局

### 6.1 目录

`.towermd/` 只放隐藏的系统配置；md 文档、画布与快照放在用户文档目录。[^owner-1016pm]

```text
.towermd/                    # 系统配置（L3），提交进 git
  config.yaml                # docs_dir、项目 ID、hub 地址（不含密钥）、角色与模型、并发、预算、阈值、中断策略
  gates.yaml                 # 质量门禁定义
  skillpacks.yaml            # 各角色启用哪些技能、交互点映射
  skillpacks/<name>@x.y.z/   # 按版本锁定的技能包副本
  rules/R-3.md               # 登记过的流程规则
  AGENTS.md                  # 给任何 Agent 的仓库规矩（由 towermd 生成）

<docs_dir>/                  # 默认 docs/tower/，init 时由人或 Agent 指定
  tower.canvas               # 画布布局（L1，JSON Canvas 1.0，见核心规格 8.10 节）
  intents/I-1.md             # 意图与约束（L0）
  arch/A-sync.md             # 模块（L1）：路径、接口、验证命令
  arch/interfaces.yaml       # 接口清单（L1）
  specs/A-sync.accept.md     # 验收标准（L2）
  baselines/B3.yaml          # 基线清单：包含的对象与版本、批准人、时间（L1）
  decisions/D-40.md          # ADR，含层级与取代关系（L1–L4）
  state/state.json           # 快照：当前状态投影（只由快照写入者写，FR-89）
  state/events/2026-10.jsonl # 快照：归档的事件
  artifacts/<run>/           # Arca 管理的大文件，见 6.4

$(git rev-parse --git-common-dir)/towermd/   # 本地 hub 与本机缓存，不进版本控制（FR-85）
```

任务、中断、提案、证据、临时规则在活跃期只存在于 hub（FR-84），快照把它们写进 `state/`。提案被批准后，结论写成 `decisions/` 下的 ADR，变更写进对应的慢层对象。

FR-07 在核心规格 10.5 节。

**FR-08** 运行的原始输出、日志和截图不进 git，交给 Arca 管理（见 6.4）。运行报告与证据作为事件进入 hub，随快照归档。

- **FR-98（开发套件补充）** 核心规格 FR-98 之外，`towermd init` 在所选目录下已有与 `intents/`、`arch/`、`specs/`、`baselines/`、`state/` 同名的子目录时应警告：这些子目录会整体成为受保护路径（FR-17），建议另选一个专用目录。Agent 用技能初始化时，可以按项目已有的文档约定提出目录建议，由人确认。

FR-81 在核心规格 10.5 节；开发套件在 `.towermd/AGENTS.md` 中增加路径守卫与合并规则（FR-17、FR-108）。

### 6.2 对象字段

对象文件是 OKF v0.2 概念文档。[^okf] 所有对象共有字段：

| 字段 | 说明 |
| --- | --- |
| `type` | 必填。对象类型，取值见下表第一列 |
| `id` | 对象 ID，如 `A-sync`、`T-2-4` |
| `layer` | `L0`–`L5`，权限检查以此为准（FR-01） |
| `title` | 可选，人类可读名称 |
| `status` | OKF 生命周期：`draft`、`stable`、`deprecated`；缺省为 `stable`。业务状态另用 `state` 等字段，不复用 `status` |
| `owner` | 负责的 actor，格式同 6.3 |
| `generated` | `{ by: <actor>, at: <ISO 8601> }`，内容最后一次实质变更 |
| `verified` | 可选，`{ by, at }` 或其列表。慢层对象被纳入基线时，由批准人以 `human:<id>` 写入 |
| `sources` | 可选，OKF 来源列表，指向原文、提交或 Note.md 文档（FR-79） |

FR-82 在核心规格 10.5 节。

| 对象（`type`） | 关键字段 |
| --- | --- |
| 意图 `Intent` | `text`、`constraints`、`priority`、`allocated_to`（模块 ID 列表） |
| 模块 `Module` | `name`、`paths`、`interfaces`、`verify`（命令列表）、`baseline`、`landmark`（地标文字，可选）。坐标不在模块对象里，在 `tower.canvas` 中（FR-43） |
| 接口 `Interface` | `from`、`to`、`contract`（文件或说明）、`baseline` |
| 验收标准 `Acceptance` | `module`、`criteria`（条目列表）、`verify`（对应的验证命令）、`baseline` |
| 基线 `Baseline` | `objects`（ID 与内容哈希）、`approved_by`、`approved_at`、`git_tag` |
| 决策 `Decision` | `context`、`options`、`decision`、`rationale`、`affects`、`supersedes`、`class` |
| 提案 `Proposal` | `kind`（新增模块、拆分、合并、接口变更、验收变更等）、`draft_baseline`、`impact`、`requested_by` |
| 任务 `Task` | `module`、`allowed_paths`、`depends_on`、`acceptance`、`priority`、`harness_role`、`state` |
| 运行 `Run` | `task`、`harness`、`model`、`session_id`、`branch`、`checkpoints`、`cost_usd`（未知时为 `null`）、`exit` |
| 证据 `Evidence` | `kind`（test、build、review、lint、gate）、`commit`、`tree`（证据绑定的 tree，FR-107）、`result`、`log`（Arca 管理的相对路径） |
| 中断 `Hold` | `hold_type`、`run`、`task`、`mode`（live 或 parked）、`question`、`options`、`recommended`、`context`、`opened_at`、`deadline`、`resolved_by`、`resolution`、`remember_as` |
| 门禁 `Gate` | `stage`、`applies_to`（层、模块、风险等级）、`check`（命令、Agent、人）、`hard`、`auto_fix_max`、`on_fail` |
| 技能包 `SkillPack` | `name`、`version`、`origin`（上游地址）、`hash`、`enabled_for`（角色到技能列表）、`disabled`、`interaction_points` |
| 临时规则 `Rule` | `text`、`scope`（全局、模块、任务）、`until`、`created_by` |

中断的类别字段名为 `hold_type`，避免与 OKF 的 `type` 冲突。`interfaces.yaml`、`gates.yaml`、`skillpacks.yaml` 中的每个条目同样带 `type`。任务、运行、证据、中断、提案、临时规则在 hub 中以同样的字段表示，快照 `state.json` 按 `type` 分组存放。

模块对象示例：

```yaml
---
type: Module
id: A-sync
layer: L1
title: 同步
status: stable
owner: human:bruce
generated: { by: claude-code/opus-5.5, at: 2026-10-16T10:00:00Z }
verified: { by: human:bruce, at: 2026-10-16T10:30:00Z }
name: 同步
paths: ["src/sync/**", "tests/sync/**"]
interfaces: [IF-sync-store]
verify: ["pnpm test --filter sync"]
baseline: B3
landmark: 中转站
---
负责把本地变更同步到存储层。
```

### 6.3 事件

FR-09（事件字段）在核心规格 10.5 节。下表是全部事件类型；其中检查点、重试、计划调整、合并、技能加载属于开发套件，其余在核心规格中也有列出。

| 字段 | 说明 |
| --- | --- |
| `seq` | hub 分配的全局单调递增序号（FR-87） |
| `eid` | 客户端生成的 UUIDv7，用于幂等重发 |
| `host` | 产生事件的主机 ID（守护进程首次启动时生成，保存在本机） |
| `epoch` | 可选，事件依赖的租约 `epoch`；已失效 `epoch` 下的事件由 hub 标 `stale: true`，不参与投影（FR-88、FR-99） |
| `t` | ISO 8601 时间，带时区 |
| `actor` | 沿用 OKF 三种形式：人用 `human:<id>`，且只能从 App 发出（FR-95、FR-111）；守护进程内的组件用 `process:towermd-<组件>`，如 `process:towermd-verifier`、`process:towermd-snapshot`、`process:towermd-cli`（命令行发出的事件），M3 起还有 `process:towermd-controller`、`process:towermd-integrator`；执行 Agent 用 `<harness>/<version>`，如 `claude-code/2.1.0`，运行 ID 放进 `data.run`；v0.1 中这一署名由技能自报（FR-106） |
| `type` | 事件类型，见下表 |
| `layer` | 事件所属层 |
| `obj` | 关联对象 ID |
| `data` | 类型相关的负载 |
| `cost_usd` | 可选，本事件产生的费用估算；harness 未报告时为 `null`，不填 0 |

人的动作与自动流程的动作不得共用 actor：总控代人回答时署名 `process:towermd-controller`，人推翻或确认后另记一条 `human:<id>` 事件。[^principles]

| 类型 | 层 | 含义 |
| --- | --- | --- |
| `run.started` / `run.finished` | L5 | 运行开始与结束，结束时附退出码和报告摘要 |
| `run.checkpoint` | L5 | 产生检查点提交 |
| `run.question` | L5 | 内环向外环提问 |
| `run.retry` | L5 | harness 报告的 API 重试 |
| `evidence.recorded` | L5 | 验证结果写入 |
| `task.state` | L4 | 任务状态变化 |
| `plan.changed` | L4 | 总控调整任务、依赖或优先级 |
| `decision.recorded` | L1–L4 | 新决策或取代 |
| `proposal.created` / `proposal.resolved` | L1–L2 | 提案生成与人工处理结果 |
| `baseline.created` | L1 | 新基线生效 |
| `merge.done` / `merge.rejected` | L5 | 集成器合并或拒绝 |
| `human.action` | 各层 | 人的干预，如暂停、重派、批准 |
| `alert.raised` / `alert.cleared` | 各层 | 告警升起与清除 |
| `hold.opened` / `hold.resolved` / `hold.escalated` | L5 | 中断打开、处理、超时升级 |
| `gate.passed` / `gate.failed` | L5 | 门禁结果 |
| `human.prompt` / `prompt.delivered` | 各层 | 人工提示词与送达确认 |
| `rule.temp.added` / `rule.temp.expired` | L3 | 临时规则生效与到期 |
| `skill.loaded` | L5 | 运行加载的技能包与版本 |
| `task.claimed` / `task.released` | L5 | Agent 经技能领取或放弃任务 |
| `lease.granted` / `lease.renewed` / `lease.expired` / `lease.released` | L5 | 租约变化，含 `epoch` |
| `snapshot.written` | L5 | 快照提交，含提交 SHA 与覆盖到的 `seq` |

### 6.4 大文件与 Arca

文本状态进 git，大文件进 Arca。两者都按仓库内的相对路径访问，画布和 Agent 不需要区分。[^arca]

| 内容 | 存放 | 理由 |
| --- | --- | --- |
| 慢层对象、`tower.canvas`、快照 | git | 可变文本，需要 diff 和版本管理 |
| harness 原始输出流、完整日志 | Arca | 体积大，写入后不再改 |
| 测试报告、截图、录屏、构建产物 | Arca | 二进制或大文件，作为证据不可变 |
| 冷启动扫描的中间结果 | Arca | 体积大，只供回溯 |

**FR-75** Arca 管理的文件应由守护进程写入主仓库目录下的 `<docs_dir>/artifacts/<run>/`，不得放在任务工作树中。理由：Arca 文档说明 `git clean -xdf` 会永久删除尚未同步到 Arca hub 的托管文件，而工作树在任务结束后通常会被删除。[^arca] 多机时，大文件先留在产生它的主机上，经 Arca 同步后其他主机才能看到（见第 20 节风险表）。

**FR-76** 证据对象应记录 Arca 文件的相对路径，以及 Arca 给出的内容哈希（若可取得）。证据写入后不得修改；更正时写新证据。

**FR-77** 本机未安装 Arca 时，守护进程应退回普通本地目录，该目录同样被 git 忽略；画布在相关证据旁标注"仅本机"。

**FR-78** 守护进程启动时宜运行 `arca doctor`，把"本地有、hub 无副本"的文件数显示在画布状态栏。

### 6.5 画布中的模块与接口

`tower.canvas` 的格式与通用对应关系见核心规格 8.10 节（FR-100–102）。开发套件增加两种对应：`file` 节点指向 `<docs_dir>/arch/*.md` 时对应模块，附加键 `towermd: { obj: "A-sync" }`；边对应接口，`label` 为接口名，附加键 `towermd: { obj: "IF-sync-store" }`。未划分的源码文件放在 `towermd: { region: "unassigned" }` 的 `group` 节点中（FR-21）。

## 7. 分支与提交策略（极简）

### 7.1 规则

只有三种 Git 引用：

| 引用 | 命名 | 生命周期 | 谁写 |
| --- | --- | --- | --- |
| 主干 | `main` | 长期 | 代码：v0.1 由 Agent 按技能收尾时合并，M3 起只由集成器合并；慢层、基线、快照：只由持有主干租约的守护进程提交 |
| 任务分支 | `t/<task-id>` | 一个任务，合并后删除 | 执行 Agent，在独立工作树中 |
| 基线标签 | `towermd/B<n>` | 永久 | 持有主干租约的守护进程，在人批准后 |

下图是一个任务从领取到合并的路径。v0.1 中由 Agent 按技能执行这些步骤；M3 中"检查"到"快进合并"由集成器串行执行。

```mermaid
flowchart LR
  M["main"] -->|"先建工作树与分支，再领取任务"| T["t/T-2-4"]
  T --> C["检查点提交"]
  C --> R["变基到 main"]
  R --> G["towermd check 路径守卫"]
  G --> V["验证并记录证据"]
  V -->|"通过"| F["快进合并并推送"]
  V -->|"失败"| T
  F -->|"推送被拒"| R
  F --> M
```

任务分支只活一个任务；合并前必须变基到最新 `main` 并通过验证；`main` 始终可运行。

**FR-10** 合并方式应为"变基后快进合并"。一个任务的多个检查点可在合并时压缩为一个提交，默认保留。v0.1 中多个 Agent 同时合并时，以推送是否被拒作为并发检查：被拒的一方 fetch、重新变基、重新验证后再推送。

**FR-11** M3 的集成器应串行处理合并队列，不做并行合并。理由：并发 Agent 数量在 10 个以内时，串行队列足够，且冲突处理最简单。

**FR-12** 每个任务合入 `main` 的末个提交应带以下 trailer，用于溯源（快进合并没有单独的合并提交）。v0.1 由技能在 `towermd finish` 通过后调用 `towermd trailers <task>` 生成，再用 `git commit --amend` 写进提交信息；证据绑定的是 tree，只改提交信息不影响证据（FR-107）：

```text
Towermd-Task: T-2-4
Towermd-Run: R-0193
Towermd-Module: A-sync
Towermd-Evidence: E-0877
Towermd-Epoch: 17
```

Epoch必须等于主Module的`module:<id>` fence epoch，不能取排序后首个touches fence。最后trailer block五项必须完整且各一次；四行旧格式只留诊断，不认证完成。恢复与正常观察的机器格式/唯一纯证明边界见[共享恢复证明契约](../archive/2026-10-20-pre-skeleton/plans/2026-10-17-shared-recovery-proof-contract.md)，它不授予执行或批准权限；其中四种窄持久请求/typed receipt及G1/G4原opening/review/gate Evidence上下文只用于核验真实已接受producer，currentPlan顺序取原opening Event.seq，晚G4不改写已完成的G3 Evidence，也不要求保存renew或补齐全部seq。

**FR-13** 回滚只用 `git revert` 撤销某个任务带入的提交，不改写 `main` 历史。

### 7.2 冲突预防：模块租约与路径守卫

- **FR-14** Agent 领取任务（`towermd task claim`）时，hub 应发放该任务所属模块的写租约。同一模块同一时间只有一个写任务；租约已被占用时，领取失败并返回可领取的其他任务。M3 中由调度器在派发前领取。
- **FR-112** 模块租约默认 TTL 15 分钟。`task claim` 生成运行 ID，并与"本机 + 当前工作树 + 分支 `t/<task-id>`"绑定（FR-113）；此后从该工作树发出的任何 `towermd` 调用都会续租，`finish` 验证进行期间由守护进程续租。超过 TTL 没有续租时租约过期，任务回到就绪；技能要求 Agent 在长时间操作前后调用一次 `towermd status`。任务进入验证中后，60 分钟内没有在 `main` 上看到合并，释放租约，任务回到就绪。
- **FR-15** 任务需要修改其他模块路径时，应在任务中声明 `touches`，并同时取得这些模块的租约。
- **FR-99** 在核心规格 10.4 节。
- **FR-16** 合并前应检查 diff：改动路径必须属于任务的 `allowed_paths`，否则拒绝合并；所属模块与 `touches` 模块的 `paths` 只作为任务授权范围的上限，不因属于模块就自动获得任务外路径权限（2026-10-17 追认）[^owner-1017-remediation]。Agent 经命令行新建任务时，`allowed_paths` 必须落在所属模块与 `touches` 模块的 `paths` 之内，超出部分被拒绝；只有人在 App 中建的任务可以放宽，防止 Agent 自建一个放开全部路径的任务来绕过守卫。v0.1 由技能在收尾时调用 `towermd check`；M3 由集成器执行。
- **FR-17** diff 中出现受保护路径时，应拒绝合并，并把改动内容转成一个提案。受保护路径包括：慢层对象，按 FR-01 以 `layer` 判定，即 `<docs_dir>` 下 `intents/`、`arch/`、`specs/`、`baselines/` 的全部文件与 `tower.canvas`，加上其他位置 `layer` 为 L0–L2 的对象（如 L1 决策），判定时同时检查改动前后的 `layer`，防止通过改 `layer` 字段绕过；快照路径 `<docs_dir>/state/`；`.towermd/` 下的配置（L3，须登记）。
- **FR-103** 在核心规格 10.4 节。

跨模块任务的比例同时是一个生长信号（第 9 节）。比例持续偏高，说明模块边界划分有问题。依据：Simon 指出，近可分解系统的子系统内部联系强于子系统之间的联系。[^simon-nd]

## 8. 冷启动：从现有项目计算基线

本节是核心规格 FR-182"从已有材料开始"在代码仓上的实现。

### 8.1 管线

下图是冷启动管线。前 5 步是确定性计算，第 6 步调用模型，最后由人审阅批准。

```mermaid
flowchart LR
  S1["文件盘点"] --> S2["依赖图"]
  S2 --> S3["聚类成模块"]
  S3 --> S4["历史耦合"]
  S4 --> S5["跑现有测试"]
  S5 --> S6["模型综合"]
  S6 --> S7["画布审阅"]
  S7 --> S8["批准 B0"]
```

| 步骤 | 做法 | 输出 |
| --- | --- | --- |
| S1 文件盘点 | `git ls-files`；识别语言、清单文件（如 package.json、Cargo.toml、go.mod、pyproject.toml）、构建与测试命令（脚本、Makefile、CI 配置） | 文件清单、候选验证命令 |
| S2 依赖图 | 按语言解析 import；解析不了时退回目录结构 | 文件级依赖图 |
| S3 聚类 | 以目录边界为初值，用社区发现算法在依赖图上聚类；每层模块数应为 3–9，超过 9 时按上级目录分组，不足 3 时下钻一级目录，无法满足时给出诊断 | 候选模块与路径 |
| S4 历史耦合 | 统计 `git log` 中经常一起修改的文件和改动热点，用于修正聚类 | 耦合矩阵、热点列表 |
| S5 跑现有测试 | 与 FR-107 的 `finish` 共用验证器，在不触碰用户工作树的临时分离工作树中运行所选提交的候选验证命令；继承用户的环境变量、允许网络，设超时并回收整个进程组 | 每个模块一条初始证据：通过、失败或无测试，署名 `process:towermd-verifier`，绑定 tree |
| S6 模型综合 | 用最强模型、只读权限运行一次 harness，输入 S1–S5 的结果，输出结构化 JSON。v0.1 由用户会话中的 Agent 按冷启动技能执行，结果经 `towermd baseline submit` 提交；M3 起也可由守护进程无头调用 | 模块名称与职责、接口、风险、待确认问题、从 README 与文档还原的意图草稿 |
| S7 画布审阅 | 人在 App 中合并、拆分、改名、拖动，补充接口，确认验证命令，把项目已有的 md 文档拖进画布作为引用节点 | 草稿基线与草稿 `tower.canvas` |
| S8 批准 B0 | 把慢层对象与 `tower.canvas` 提交到用户文档目录，并打标签 `towermd/B0` | 初始基线 |

v0.1 的实现取舍：M0 的 S2、S3 只按目录结构聚类，再用 S4 的历史耦合修正；M0 保留覆盖率指标，但目录聚类通常即可覆盖全部源码，需同时检查每层 3–9 个模块及“有验证命令或无测试候选任务”，不能只以覆盖率证明冷启动质量。M2 引入 import 解析后覆盖率才更有区分度。import 解析（Rust、TypeScript/JavaScript、Svelte、Python、Go，逐行模式匹配）与 Louvain 社区发现放到 M2，按 M1 试用中人工合并、拆分的次数决定是否需要。两者都不新增依赖；准确率仍不够时再评估 tree-sitter 一类的库，按 `AGENTS.md` 的新依赖规则请人确认。[^owner-1017-remediation]

S5 的临时工作树位于 `$(git-common-dir)/towermd/verify/`，使用 `git worktree add --detach <sha>`；验证日志放本机缓存，大文件按 FR-75 处理。清单文件所在目录的命令归到包含该目录的模块；根目录命令只供没有自有命令的模块使用，证据标明“共享”，同一条命令只运行一次。沙箱实现留在 `/private/tmp` 的隔离工作树，属于核心规格 10.3 节所说的、未排期的安全加固，不进入本轮。S1–S5 草稿由守护进程保存在 `$(git-common-dir)/towermd/drafts/<scan-id>/`，包括候选模块 `arch/*.md`、草稿 `tower.canvas` 与未划分区清单；可由扫描重建，经 `GET /drafts/:id` 读取，画布节点引用草稿中真实存在的对象。S8 人批准之前不写用户文档目录或 `main`。[^owner-1017-remediation]

**FR-18** S6 应要求输出符合 JSON Schema，`towermd baseline submit` 按同一 schema 校验。无头调用时，Claude Code 可用 `--output-format json` 加 `--json-schema`；Codex 可用 `--output-schema`。[^cc-headless][^codex-exec]

**FR-19** 冷启动期间 harness 应只读运行。无头调用时，Claude Code 宜用 `--permission-mode dontAsk` 并只放行读工具；Codex 用默认的 `read-only` 沙箱。[^cc-headless][^codex-exec]

**FR-20** 每个候选模块应附置信度和理由。置信度低的模块在画布上以虚线边框显示，提醒人重点审阅。

**FR-21** 未分配到任何模块的文件应集中列出，并在画布上显示为"未划分区"。

### 8.2 验收

- 已跟踪源码文件中至少 95% 被分配到模块，其余列入未划分区；M0 目录聚类下仍保留此指标，M2 再结合 import 解析检验区分度。
- 每层模块数应为 3–9；不足时下钻、过多时分组，无法满足时须给出诊断，不能静默输出任意数量。
- 每个模块至少有一条验证命令，或明确标记为"无测试"并生成一条补测试的候选任务。
- 对 20 万行以内的仓库，S1–S6 在 15 分钟内完成（不含测试本身超过超时的部分）。

## 9. 一句话生长

本节是核心规格 FR-182"从一句话开始"在代码仓上的实现。

### 9.1 流程

1. **种子**：一句话写入意图 I-1（L0）。
2. **澄清**：最强模型最多提 3 个问题，以卡片形式显示在画布上。v0.1 由用户会话中的 Agent 按 `towermd-seed` 技能执行，提问经 `towermd ask`；M3 起也可由守护进程无头调用。
3. **骨架**：生成 1–3 个模块、验收标准、不超过 7 个任务。第一个任务是"空骨架能构建并通过一条冒烟测试"。骨架用与冷启动 S6 相同的 schema，经 `towermd baseline submit` 提交为草稿基线。
4. **批准**：人一键批准 B0。仓库未初始化时，系统先执行 `git init`。

   实现说明（2026-10-25）：批准时不执行 `git init`，改由 `towermd-seed` 技能在第一步运行。原因是批准 B0 之前，每一步都要求目录已经是仓库：`towermd init` 和守护进程要求仓库存在；B0 草稿必须来自对一个真实提交的固定扫描；出版只经持有主干租约的唯一写入模块完成。仓库不存在时，这些都无从发生，到了批准这一步才 `git init` 也没有意义。
5. **生长**：执行中积累生长信号；信号越过阈值时，守护进程在关注清单中提示。v0.1 由人让会话中的 Agent 起草提案（`towermd propose`）；M3 起由总控生成提案。
6. **定型**：模块数达到阈值，或人手动切换后，项目从"生长期"进入"定型期"。

### 9.2 生长期与定型期

| 项 | 生长期 | 定型期 |
| --- | --- | --- |
| 未划分区 | 允许 Agent 在未划分区新建文件 | 不允许；所有文件必须属于某个模块 |
| 新增模块 | 提案可一键批准，影响分析从简 | 提案必须附完整影响分析 |
| 接口契约 | 可以只写一句说明 | 必须有契约文件或测试 |
| 默认验证 | 冒烟测试 | 模块全部验证命令 |

这对应 Brand 所说的"已精装区域"与"可用但未精装区域"共存，也对应前期研究的结论：MVP 前让结构自然生长，产品化后先立慢层。[^layered]

**FR-22** 每一步生长结束时，`main` 应能通过当前基线定义的验证。依据：Simon 的钟表匠寓言说明，稳定的中间形态让复杂系统更快形成，被打断时损失也更小。[^simon-watch]

实现说明（2026-10-25）：

- “一步生长”指出版一个基线。表中第二到第四行与 FR-22 都在 App 准备批准这一步核对，不满足时拒绝准备，并写明缺什么。所处的期按主干上的 `config.yaml` 和当前已批准的模块数判断，与 9.3 的生长信号读法相同。
- **默认验证与 FR-22。** 审阅验证在草稿的来源提交上运行，也就是出版前的 `main`。
  - 生长期：B0 允许没有测试，因为种子的第一个任务才是补上冒烟测试。B1 起至少一条命令须真实通过。
  - 定型期：每个模块都须有自己的验证命令且全部通过，B0 也一样。
  - 任务完成验证不随期变化，一直运行任务所属模块与 `touches` 模块的全部已批准命令，比“冒烟测试”更严。
- **接口契约。** 定型期每个接口的 `contract` 文字里须写出至少一个仓库相对路径，指向的文件在来源提交上存在或随本次出版写入，例如 ``见 `api/tests/contract.rs` ``。生长期一句说明即可。
- **影响分析。** 影响对象仍由守护进程推算。提案输入增加可选的 `impact_notes`：按受影响对象 ID 各写一句说明，ID 只能取自推算出的意图、模块、接口和任务，说明不能为空。
  - 定型期每个受影响对象都须有说明，适用于所有提案种类。`towermd propose` 缺说明时返回 `gate_failed`，准备批准时也再核对一次。生长期可以不写（从简）。
  - 说明随批准写进本次 `D-<提案 ID>` 决策的 `impact_notes`。
  - 带说明的提案摘要用 `proposal-v3`；不带说明的提案仍用原来的 v1、v2 摘要，与原来逐字节相同。
- 第一行（未划分区）由 9.3 的未划分区信号承担：定型期阈值为 0。

### 9.3 生长信号与默认阈值

| 信号 | 计算方式 | 默认阈值 | 生成的提案 |
| --- | --- | --- | --- |
| 模块过大 | 模块内源码行数或文件数 | 超过 3000 行或 40 个文件 | 拆分模块 |
| 跨模块改动 | 最近 10 个任务中声明了 `touches` 的比例 | 超过 30% | 调整边界或新增接口 |
| 未划分区膨胀 | 未划分区文件数 | 超过 10 个 | 新增模块 |
| 需求未分配 | 意图的 `allocated_to` 为空 | 存在即触发 | 新增模块或扩展职责 |
| 同类错误反复 | 同一错误类别在 24 小时内出现次数 | 3 次及以上 | 修改 L3 流程或检查规则 |

阈值写在 `config.yaml`，人可以修改。这些数字是起点，需要在使用中校准。

## 10. 执行：技能优先，无头双环在 M3

### 10.0 v0.1 技能优先模式

v0.1 不由 Tower.md 启动 harness。用户在自己的 Claude Code、Codex 等会话里加载 Tower.md 技能包，可以在一台机器上开多个工作树，也可以在多台机器上开；Agent 按技能流程经 `towermd` 命令行与 hub 协作。[^owner-1016pm] M3 的无头调度复用同一套技能与命令行，只是把"开会话"换成运行器。

下图是一个 Agent 在 v0.1 中完成一个任务的路径。

```mermaid
sequenceDiagram
  participant A as Agent（用户会话）
  participant C as towermd 命令行与守护进程
  participant H as hub
  participant U as App 与人
  A->>C: towermd task claim
  C->>H: 申请模块租约
  H-->>C: 租约与 epoch
  C-->>A: 任务卡
  A->>C: towermd ask（需要人时）
  C->>H: hold.opened
  H->>U: 提问图标
  U->>H: 回答（经 App）
  H-->>A: 回答
  A->>C: towermd finish
  C->>C: 路径守卫与验证，记录证据
  A->>A: 变基、快进合并、推送
  C->>H: 在 main 上看到提交后记为已合并
```

**FR-104** Tower.md 技能包至少包含以下技能。每个技能只调用 `towermd` 命令行，不直接写用户文档目录、快照或 `.towermd/`；Agent 自己只做提交、变基、合并和推送，租约、事件、快照都由命令行处理。理由：模型不擅长管理复杂的 git 操作，除了提交和推送，最好不交给它。[^pi]

| 技能 | 何时用 | 调用的命令 |
| --- | --- | --- |
| `towermd-init` | 在项目中初始化 Tower.md | `towermd init` |
| `towermd-baseline` | 冷启动 S6 模型综合 | `towermd baseline scan`、`towermd baseline submit` |
| `towermd-seed` | 一句话生长的澄清与骨架（9.1） | `towermd seed`、`towermd ask`、`towermd baseline submit` |
| `towermd-claim` | 会话开始、找活干 | `towermd status`、`towermd task claim` |
| `towermd-ask` | 需要人回答、评审 | `towermd ask`、`towermd review request` |
| `towermd-propose` | 需要改慢层或越出允许路径 | `towermd propose` |
| `towermd-finish` | 收尾合并前 | `towermd check`、`towermd finish`、`towermd trailers` |

**FR-105** 技能包以 harness 原生的插件或技能目录形式发布，由用户自己安装到 harness 中。Tower.md 不修改用户全局的 harness 配置；`towermd init` 只提示安装方式。

**FR-106** `towermd task claim` 应在一次调用中完成 G0 检查、取得租约（FR-14）、记录 `task.claimed`，生成运行 ID（FR-112），并把任务卡打印到标准输出。harness 名称与版本由技能经参数传入，记为 `data.self_reported`，不作为可信署名。任务卡内容见 10.4。Agent 先自行建分支 `t/<task-id>` 与目标工作树，再在该工作树领取；M0 暂不支持领取后迁移运行绑定（2026-10-17 追认）[^owner-1017-remediation]。技能建议、但不强制工作树位置。

**FR-107** `towermd finish` 先运行路径守卫（FR-16、FR-17），再由守护进程在临时的分离工作树（`git worktree add --detach <sha>`，放在 `$(git-common-dir)/towermd/verify/` 下，与 S5 共用验证器；继承用户的环境变量、允许网络，设超时并回收整个进程组，日志放本机缓存，大文件按 FR-75 处理）中对当前提交运行模块验证命令，以 `process:towermd-verifier` 署名写成证据。工作树有未提交改动时 `finish` 拒绝执行。证据绑定提交的 tree，不绑定 SHA，因此写 trailer、压缩检查点都不会让证据失效。验证在后台进行：`finish` 立即返回证据 ID，`finish --wait` 每次最多等 90 秒，未完成时提示 Agent 再次调用，避免被 harness 的命令超时杀掉。Agent 自述的"完成"不产生任何状态转换。证据通过后，Agent 写入 trailer，按 FR-10 合并并推送。守护进程在 `main` 上看到带该任务 trailer 的提交时复核：该提交的 tree 与证据记录的 tree 相同，或二者只在 `<docs_dir>/state/` 与 `tower.canvas` 上不同（见 G5），才把任务记为已合并；否则生成门禁失败中断。完成事件必须携完整冻结输入所对应的结构化verifier报告，Evidence日志引用绑定原完成eid与报告摘要；commands摘要保留cwd/argv及数组全部原值。纯prove_merge只认证既成Git历史，正常integrator在唯一finish/观察流程先核当前完整fences、固定期限及G0–G5，再调用同一证明器；恢复不重跑命令、不获得当前租约或执行权限。trailers输出前耐久记录实际基底与Task父OID的终态G5 Evidence；认证实际main接入commit及其FR-12上下文，不能把不合法merge里的side Task tip仅按可达性算完成。同tree amend/squash不重测，纯state/canvas rebase重新trailers而复用G3；冷clone由实际树重建核原evidence tree，不依赖旧不可达对象。精确DTO/摘要、终态G5与接入边契约沿[共享恢复证明契约](../archive/2026-10-20-pre-skeleton/plans/2026-10-17-shared-recovery-proof-contract.md)第1.5/3.2节。

**FR-108** 用户会话里还可能加载了其他技能包（如 superpowers）。`.towermd/AGENTS.md` 与 `towermd-finish` 技能应写明：合并前必须先通过 `towermd finish`；其他技能中开拉取请求、合并的步骤排在它之后。Tower.md 不禁用用户安装的技能。

### 10.1 双环（M3）

- **外环（总控）**：读取当前基线和任务状态，生成或调整任务，派发给调度器，回答内环问题，分类错误，生成提案。总控本身也通过无头 harness 调用模型。
- **内环（执行 Agent）**：在任务工作树中完成一个任务，产生检查点提交和报告。

这一结构来自红军的双环实践：外环负责总控和审核，内环负责执行；人只和外环对接。[^hj-note]

**FR-23** 总控只能写 L3–L4 对象、决策（次要）和提案，不能直接修改慢层对象。v0.1 没有总控进程：提问先按 FR-62 引用已有决策，否则直接交给人。

**FR-24** 总控回答内环问题时，只能在当前基线和验收标准范围内作答。超出范围的问题应转给人：中断的处理者改为人，并进入关注清单（见 12.4）。

### 10.2 任务状态机

```mermaid
stateDiagram-v2
  state "待定" as Proposed
  state "就绪" as Ready
  state "运行中" as Running
  state "已暂停" as Paused
  state "验证中" as Verifying
  state "已合并" as Merged
  state "失败" as Failed
  state "中断中" as Held
  [*] --> Proposed
  Proposed --> Ready
  Ready --> Running
  Running --> Paused
  Paused --> Running
  Running --> Verifying
  Running --> Failed
  Running --> Held
  Held --> Running
  Held --> Ready
  Running --> Ready
  Verifying --> Running
  Verifying --> Ready
  Verifying --> Merged
  Verifying --> Failed
  Verifying --> Held
  Failed --> Ready
  Failed --> Held
  Merged --> [*]
```

| 转换 | 触发条件 |
| --- | --- |
| 待定 → 就绪 | 依赖已合并，G0 通过 |
| 就绪 → 运行中 | Agent 领取并取得模块租约（v0.1）；调度器派发、运行器启动 harness（M3） |
| 运行中 → 就绪 | Agent 放弃任务，或租约过期未续（FR-112） |
| 运行中 → 已暂停 | 人暂停：v0.1 以插话送达，Agent 在下一个检查点停下（15.5）；M3 由运行器发送中断并等待到检查点 |
| 运行中 → 验证中 | Agent 执行 `towermd finish`（v0.1）；运行正常结束且报告状态为 `done`（M3）。进入验证中不等于完成 |
| 运行中 → 中断中 | Agent 提问或申请授权；报告状态为 `needs_input` 或 `needs_decision`；路径或策略被拦截；预算用完（M3） |
| 中断中 → 运行中 | 中断被回答或批准，实时等待的进程继续，或停车的运行续跑 |
| 中断中 → 就绪 | 人选择"改写任务"或"重派"，原运行结束 |
| 验证中 → 运行中 | v0.1：验证未通过，租约仍在，由同一领取者修改后重新 `finish`；累计 3 次未通过转为中断中 |
| 验证中 → 就绪 | 验证已通过，但 60 分钟内没有在 `main` 上看到合并，释放租约（FR-112） |
| 验证中 → 中断中 | 门禁要求人评审，或硬门禁自动修复次数用完 |
| 验证中 → 已合并 | 全部硬门禁通过，且守护进程在 `main` 上看到带该任务 trailer 的已验证提交（FR-107） |
| 失败 → 就绪 | M3：总控判为执行类错误，重试次数未超上限；v0.1：人在卡片上选择重派 |
| 失败 → 中断中 | 同一任务失败 3 次，或判为设计类错误，生成错误中断 |

### 10.3 harness 适配器

**FR-25** M3 起，Tower.md 无头调用的每个 harness 通过一个适配器接入。适配器接口如下：

```ts
interface HarnessAdapter {
  id: string;                       // "claude-code" | "codex" | "cursor" | "gemini" | ...
  probe(): Promise<{ version: string; capabilities: string[] }>;
  start(run: RunSpec): ChildHandle; // 在工作树中启动无头进程
  parse(line: string): TowerEvent[]; // 把一行原始输出转成标准事件
  interrupt(h: ChildHandle): Promise<void>; // 优雅中断
  resume?(run: RunSpec, sessionId: string, message: string): ChildHandle; // 停车续跑
  send?(h: ChildHandle, message: string): Promise<"delivered" | "queued">; // 运行中插话
  channels: {
    mcp: boolean;             // 能否连接交互网关（实时提问、评审）
    permissionRoute: boolean; // 能否把授权请求转给交互网关
    liveInput: boolean;       // 能否在运行中注入消息
    skills: "plugin-dir" | "skills-dir" | "none"; // 技能包注入方式
  };
}

interface RunSpec {
  runId: string; taskId: string; worktree: string;
  prompt: string;            // 固定模板，指向 task.md
  model?: string;
  permission: "read-only" | "workspace-write";
  budgetUsd?: number; timeoutSec: number;
  outputSchema?: object;     // 最终报告的 JSON Schema
  mcpConfig?: string;        // 交互网关的 MCP 配置
  skillDirs?: string[];      // 本次运行组装好的技能目录
}
```

v0.1 宜先实现 Claude Code 和 Codex 两个适配器。下表列出各 harness 文档中可用的无头调用方式。CLI 变化快，实现时应以 `probe()` 读到的版本和官方文档为准。

| harness | 无头调用与输出 | 提问通道 | 授权路由 | 运行中插话 | 技能注入 | 结构化报告 | 续跑与预算 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Claude Code | `claude --bare -p`；`--output-format stream-json --verbose`，最后一行 `result` 含费用 | MCP 工具（`--mcp-config`，宜加 `--strict-mcp-config`）；停车续跑 | `--permission-prompt-tool` 指向交互网关；完全无人值守时 `--permission-prompts none` | `--input-format stream-json`；`--replay-user-messages` 回显作送达确认 | `--plugin-dir` 或 `--add-dir`（bare 模式不自动发现） | `--json-schema` | `--resume <session_id>`；`--max-budget-usd`；`--max-turns` |
| Codex | `codex exec --json`（JSON Lines，含 `turn.completed`、`item.*`） | 停车续跑；MCP 未核对 | `--sandbox workspace-write`；`--approve-for-me` 走自动评审；被拒记为禁止 | 未核对，排队到续跑 | 未核对 | `--output-schema <file>`；`-o` 写出最后消息 | `codex exec resume <SESSION_ID>` |
| Cursor CLI | `agent -p --output-format stream-json`；`--force` 允许改文件 | 停车续跑 | 未核对 | 未核对 | 未核对 | `--output-format json` | 未核对 |
| Gemini CLI | `gemini -p --output-format stream-json` | 停车续跑 | 未核对 | 未核对 | 未核对 | `--output-format json`（含 `stats`） | 未核对 |

来源：[^cc-headless][^cc-cli][^codex-exec][^cursor-headless][^gemini-headless]

**FR-26** 运行器调用 Claude Code 时宜使用 `--bare`。理由：文档说明 `-p` 默认会加载工作目录中的 hooks 和 MCP 配置且不显示信任提示，`--bare` 跳过这些发现，使每台机器上的结果一致。[^cc-headless] bare 模式不会自动加载技能、插件和 MCP，所以技能包和交互网关都应由运行器显式传入（见 10.6 与第 12 节）。[^cc-cli]

**FR-27** 运行器中断运行时，应先发送 SIGINT，等待最多 60 秒；超时再发送 SIGTERM。理由：Claude Code 文档说明 SIGTERM 会让当前回合不完整，SIGINT 才会结束回合。[^cc-headless]

### 10.4 运行契约

任务卡是 Agent 执行一个任务所需的全部上下文。v0.1 中 `towermd task claim` 与 `towermd task show` 把它打印到标准输出；M3 中运行器把它写到 `$(git rev-parse --git-common-dir)/towermd/runs/<run>/task.md`，各 harness 使用同一个提示模板，只要求它读取这个文件。

任务卡包含：

- 任务 ID、所属模块、`allowed_paths`。
- 验收标准与验证命令。
- 相关接口契约（只读）与相关决策摘要。
- 禁止事项（与 `.towermd/AGENTS.md` 一致，见 FR-81）：不得修改慢层对象、`tower.canvas` 与快照；不得修改 `allowed_paths` 以外的文件；未通过 `towermd finish` 不得合并。M3 的无头运行另外禁止自行创建分支、工作树和拉取请求，这些由运行器和集成器管理。
- 交互约定：需要人时执行 `towermd ask`（M3 的无头运行调用交互网关的工具）；返回 `deferred` 时，结束回合并说明在等什么。
- 当前生效的临时规则和人工插话摘要。
- 检查点要求：每完成一个可验证的小步就提交一次。
- 报告要求：`towermd finish --report <file>` 时提交报告；M3 的无头运行结束前写出报告。

**FR-28** 运行报告应符合以下结构。harness 支持 JSON Schema 时，应同时用 schema 约束最终输出。报告只作摘要和线索，进度仍只认证据（FR-33）。

```json
{
  "status": "done | blocked | needs_input | needs_decision | failed",
  "summary": "不超过 300 字",
  "checkpoints": ["<commit sha>"],
  "changed_paths": ["src/sync/queue.ts"],
  "questions": ["需要外环回答的问题"],
  "suspected_class": "external | execution | design",
  "proposal": { "kind": "interface_change", "reason": "..." }
}
```

**FR-29** M3 中运行没有写出报告时，运行器应把 `run.finished` 的状态记为 `failed`，并把 harness 最后一条消息存为摘要。

### 10.5 角色与模型分层

| 角色 | 默认模型档位 | 理由 |
| --- | --- | --- |
| 冷启动综合、慢层提案起草 | 最强档 | 慢层后果大，明豪讨论中建议慢层用人或最贵的模型[^mh-note] |
| 总控 | 中档或最强档 | 需要判断优先级和错误类别 |
| 执行 Agent | 便宜档 | 边界清楚的小模块，便宜模型已能胜任[^mh-note] |
| 进度摘要、事件归纳 | 便宜档或纯规则 | 红军用便宜模型持续维护进度图[^hj-note] |
| 交叉评审（可选） | 与执行 Agent 不同的厂商 | 同一模型自评容易有共同盲点[^hj-note] |

**FR-30** 角色到 harness 与模型的映射应写在 `config.yaml`，可按模块覆盖。

**FR-31** v0.1 的并发由人开几个会话决定，Tower.md 只在关注清单里提示人工中断偏多；以下上限用于 M3。并发上限默认 4，可配置。超过上限的就绪任务排队。Note.md 的研究认为人能盯住的并行上限约 2–3 个；Tower.md 用总控代答和关注清单把人移出并行路径，所以默认 4 不矛盾。但应按"每任务人工中断数"和"中断等待时间"（核心规格第 15 节）校准：人工中断偏多时，画布建议把并发降到 2–3。[^principles]

### 10.6 工作流技能包

工作流技能包规定 harness 按什么步骤干活，例如先头脑风暴、再写计划、测试先行、完成前自检、请求代码评审。superpowers 是这类技能包的一个例子。[^superpowers] 这类技能包有两个特点：一是部分步骤会停下来问人或等人评审；二是部分步骤会自己管理分支和工作树，与 Tower.md 的职责重叠。

v0.1 中第三方技能包由用户安装在自己的 harness 里，Tower.md 只在技能经命令行报告时记录 `skill.loaded`；下面的锁定、组装与禁用规则用于 M3 的无头运行。

**FR-51** 技能包应按版本复制到 `.towermd/skillpacks/<name>@<version>/`，并记录内容哈希。升级或更换技能包属于 L3 变更，需要登记。

**FR-52** `skillpacks.yaml` 应按角色（总控、执行 Agent、评审 Agent）声明启用哪些技能。运行器为每个运行组装一个只含启用技能的目录，再按适配器的方式注入。Claude Code 用 `--plugin-dir` 或 `--add-dir`。[^cc-cli]

**FR-53** 每个技能包应声明它的交互点，并映射到 Tower.md 的中断类型或门禁：

```yaml
# skillpacks.yaml 片段
packs:
  - name: superpowers
    version: x.y.z
    enabled_for:
      controller: [brainstorming, writing-plans]
      executor: [writing-plans, executing-plans, test-driven-development,
                 systematic-debugging, verification-before-completion,
                 requesting-code-review, receiving-code-review]
    disabled: [using-git-worktrees, finishing-a-development-branch]
    interaction_points:
      brainstorming: { maps_to: hold.question, scope: slow_layer }
      requesting-code-review: { maps_to: gate.G4 }
```

**FR-54** 技能中与 Tower.md 职责冲突的步骤（建分支、建工作树、合并、开拉取请求）应被禁用，或由 `task.md` 明确覆盖。Tower.md 的分支策略优先。

下表以 superpowers 为例说明映射方式。技能说明来自第三方介绍，锁定版本时应按该版本的原文复核。[^superpowers]

| 技能 | 技能中的交互点 | Tower.md 中的处理 |
| --- | --- | --- |
| brainstorming | 逐个提问，等用户批准规格 | 只给总控在一句话生长和慢层提案中使用；问题转为提问中断；执行 Agent 不启用，因为规格已写在 `task.md` |
| writing-plans | 生成分任务计划 | 执行 Agent 可用；高风险模块的计划进入计划门禁 G1 |
| executing-plans、dispatching-parallel-agents | 无人工点 | 可用；子 Agent 的费用计入本运行预算 |
| test-driven-development、systematic-debugging | 无人工点 | 可用；与验证门禁 G3 一致 |
| verification-before-completion | 结果供人查看 | 可用；自检结果附在报告中，但不能替代 G3 |
| requesting-code-review、receiving-code-review | 人决定是否修复严重问题 | 映射到评审门禁 G4，按严重程度分流 |
| using-git-worktrees、finishing-a-development-branch | 建工作树、开拉取请求 | 禁用；由 Tower.md 的运行器和集成器负责 |

## 11. 质量门禁与证据

核心只保留 G0、G6 和"请求人评审"（核心规格 6.3 节）；G1–G5 属于开发套件。下表仍列出全部 7 个门禁，便于对照。

- **FR-32** 每次合并前，应在变基后的任务分支上运行所属模块的全部验证命令，结果写为证据并绑定提交。v0.1 由 `towermd finish` 执行（FR-107），M3 由集成器执行。
- **FR-33** 在核心规格第 6 节。
- **FR-34** 慢层提案可配置一次异构模型交叉评审，评审意见附在提案上，只供人参考，不自动决定。
- **FR-121** 规格 v0.5 中模块的 `verify` 命令视为一种内置的检查扩展：`kind: core.verify`，`deterministic: true`。项目可以不配置 `verify`，只配置其他检查；也可以一个检查都不配置，此时完成只能由人确认（FR-124）。

### 11.1 门禁清单

门禁是任务生命周期中的固定卡点。硬门禁不通过就不能进入下一步；软门禁不通过只标黄并附说明。

| 门禁 | 阶段 | 检查方式 | 默认 | 未通过时 | 画布图标 |
| --- | --- | --- | --- | --- | --- |
| G0 就绪 | 派发前 | 规则：有验收标准、允许路径、模块租约、相关决策 | 硬 | 任务不派发，生成提问中断"缺信息" | 提问 |
| G1 计划 | 运行中，写完计划后 | v0.1：高风险模块由技能经 `towermd review request` 请人评审，其余不评审；M3：总控对照规格评审，高风险模块由人评审 | 软；高风险模块为硬 | 退回 Agent 改计划，或生成评审中断 | 评审 |
| G2 路径与策略 | 合并前；v0.1 另在 `main` 上事后检测（FR-103） | 规则：路径守卫、慢层保护、敏感文件、危险命令 | 硬 | 拦截，生成禁止中断 | 禁止 |
| G3 验证 | 合并前 | 运行模块验证命令；可选检查行为改动是否带测试 | 硬 | 自动修复，次数用完生成门禁失败中断 | 门禁失败 |
| G4 评审 | 合并前 | v0.1：技能请求评审，人在 App 中处理，可附人在另一个会话里让评审 Agent 给出的意见；M3：评审 Agent（宜与执行 Agent 不同厂商）给出分级意见 | 严重问题为硬，其余为软 | 严重：自动修复或评审中断；重要：人决定（M3 起总控决定）；次要：记录 | 评审 |
| G5 合并 | 合并时 | 变基到最新 `main` 后再跑一次 G3；v0.1 中 `towermd finish` 拒绝不以最新 `main` 为基的提交。唯一raw-z/no-renames守卫逐提交核真实基底链；新旧基底之间若只有改动 `<docs_dir>/state/` 与 `<docs_dir>/tower.canvas` 的提交，只需实际变基，证据继续有效，不重跑验证；任务自身G2仍禁止写这两处。代码/config或其他路径推进必须重新finish，即使最终tree巧合相同也不豁免 | 硬 | 退回任务分支重做 | 门禁失败 |
| G6 基线 | 慢层变更 | 提案有影响分析且人已批准 | 硬 | 提案不生效 | 评审 |

**FR-55** 门禁定义写在 `.towermd/gates.yaml`，可以按层、模块或风险等级覆盖。修改门禁属于 L3 变更，需要登记。

G0 的相关决策判定：没有相关 Decision 时允许领取；任务显式引用的每个 Decision 必须存在于权威 `main`，不能以工作树里的未批准对象替代（2026-10-17 追认）[^owner-1017-remediation]。

- **FR-56** 在核心规格第 6 节。

**FR-57** 硬门禁的自动修复次数默认 2 次。次数用完后生成门禁失败中断，由人决定重试、改验收标准或授予豁免。

**FR-58** 模块可标记 `risk: high`。高风险模块的 G1 和 G4 必须有人评审。

```yaml
# gates.yaml 片段
gates:
  G3: { stage: pre_merge, check: module.verify, hard: true, auto_fix_max: 2 }
  G4: { stage: pre_merge, check: agent.review, reviewer_vendor: different,
        hard_on: [critical], on_important: controller, on_minor: log }
overrides:
  - match: { risk: high }
    G1: { hard: true, check: human.review }
    G4: { check: human.review }
```

## 12. 临时中断与人工干预（开发套件部分）

核心的中断类型、图标与关注清单在核心规格 7.1、7.6 节。下表列出 v0.5 的 7 类中断：提问、评审、禁止、门禁失败对应核心的提问、评审、人定内容被改、检查未通过；授权、预算、harness 错误是开发套件增加的 3 类。

### 12.1 中断类型

harness 运行时需要回应的情况分为 7 类。每类都有固定的图标、默认处理者和超时策略。

| 类型 | 图标 | 触发 | 默认处理者 | 人的选项 | 超时策略（默认） |
| --- | --- | --- | --- | --- | --- |
| 提问 | ❓ | Agent 调用 `towermd_ask`；报告为 `needs_input`；G0 缺信息 | 总控先代答，超出权限转人 | 选推荐项、自由回答、让总控代答、"按你的假设继续" | 30 分钟未答，升级到关注清单；不自动猜测 |
| 评审 | 👁 | G1、G4、G6 要求评审；技能包的评审交互点 | 按门禁配置：总控或人 | 通过、要求修改（附意见）、驳回 | 不会超时自动通过 |
| 授权 | 🔑 | harness 请求执行未放行的工具或命令 | 人 | 本次允许、写入允许规则（L3 登记）、拒绝 | 10 分钟未答则拒绝，Agent 改用其他方法 |
| 禁止 | ⛔ | G2 拦截：碰慢层、越界、敏感文件、危险命令 | 人 | 撤销改动、转为提案、授予豁免（带到期时间） | 保持拦截 |
| 门禁失败 | 🚧 | 硬门禁自动修复次数用完 | 人 | 查看日志、让 Agent 再修、改验收标准、豁免 | 保持阻塞 |
| 错误 | ❗ | 设计类错误；同一任务失败 3 次；harness 崩溃 | 人 | 重试、改写任务、换 harness 或模型、放弃 | 保持 |
| 预算 | 💰 | 运行或模块达到预算上限 | 人 | 追加预算、换便宜模型、停止 | 停止运行，保留检查点 |

图标只是示意。画布实现时，每个图标都要同时有形状、颜色和文字标签，不只靠颜色或表情符号区分（见核心规格 7.6 节）。

v0.1 中 harness 的工具授权仍在用户自己的会话里由 harness 询问，Tower.md 不介入，授权中断从 M3 的无头运行开始出现；总控代答也从 M3 开始，v0.1 的默认处理者除 FR-62 的决策引用外都是人。

### 12.2 两种等待方式

下图是一次提问从发出到回答的路径。

```mermaid
sequenceDiagram
  participant A as 执行 Agent
  participant G as 交互网关
  participant C as 总控
  participant U as 画布与人
  A->>G: towermd_ask 提问
  G->>C: 先请总控代答
  C-->>G: 超出权限，转人
  G->>U: 节点出现提问图标
  U->>G: 选择推荐项并记为决策
  G-->>A: 返回回答，继续执行
```

实时等待时，Agent 进程一直在等这次工具调用返回；超过等待上限时转为停车续跑。v0.1 中 Agent 用 `towermd ask`，命令行阻塞等待回答，行为与交互网关的工具相同；收到 `deferred` 后，人可以回到该会话继续，Agent 也会在下一次调用 `towermd` 时取到回答（FR-109）。

| 方式 | 做法 | 适用 | 代价 |
| --- | --- | --- | --- |
| 实时等待 | 交互网关的工具调用阻塞，直到有回答 | 预计几分钟内有人回答 | 进程常驻，但等待期间不耗 token |
| 停车续跑 | Agent 结束回合并写 `needs_input` 报告，进程退出；回答后运行器用会话 ID 续跑，把回答作为新提示 | 回答要等较久；harness 不支持 MCP | 续跑时需要重新加载上下文 |

- **FR-59** 在核心规格中（见核心规格附录 B）。

**FR-60** harness 不支持 MCP 时，只用停车续跑。

- **FR-61、FR-62** 在核心规格中（见核心规格附录 B）。

**FR-63** Claude Code 运行宜用 `--permission-prompt-tool` 把授权请求转给交互网关，生成授权中断。完全无人值守的运行才用 `--permission-prompts none`，此时被拒的请求记为禁止事件。[^cc-headless][^cc-cli]

### 12.3 回答记到哪里

中断卡片上的回答有 4 个去向，由人勾选：

| 去向 | 写入 | 效果 |
| --- | --- | --- |
| 只用于本次 | hub 中的中断记录，随快照归档 | 只影响当前运行 |
| 记为模块决策 | L4 次要决策，写入 `<docs_dir>/decisions/` | 同模块之后的任务卡自动带上；同类问题直接引用作答 |
| 记为流程规则 | L3 规则，需登记 | 所有相关任务卡带上 |
| 涉及慢层 | 生成草稿提案 | 走核心规格 9.2 节的变更控制，不直接生效 |

- **FR-64** 在核心规格中（见核心规格附录 B）。

### 12.4 总控代答策略

**FR-65** （M3）每类中断的处理策略写在 `config.yaml`，取值为 `auto`（总控直接处理）、`auto_notify`（总控处理并在日报中通知人）、`human`（必须人处理）。默认值如下：

| 类型 | 默认策略 | 说明 |
| --- | --- | --- |
| 提问（L4 范围内） | `auto_notify` | 总控只能依据基线、验收标准和已有决策作答，并注明依据 |
| 提问（涉及慢层） | `human` | 例如技能包的头脑风暴在种子流程中提出的问题 |
| 评审 | 按门禁配置 | 高风险模块为 `human` |
| 授权、禁止、错误、门禁失败、预算 | `human` | 可以对授权配置允许规则，减少重复请求 |

**FR-66** 人可以推翻总控的代答。推翻后，系统标记受影响的检查点，并建议重跑相关任务。

### 12.5 人工输入：提示词与临时干预

按 2026-10-20 用户裁决，主界面只有全窗口画布，不设置常驻输入栏。人工输入通过选中对象后的临时操作面板或画布右键菜单唤出，关闭后返回纯画布。输入的范围仍取决于当前选中的对象。

| 选中对象 | 输入发给 | 效果 |
| --- | --- | --- |
| 无（全局） | 总控（M3）；v0.1 作为插话发给全部运行中任务 | 总控把指令转成动作计划，例如新增任务、调整优先级、暂停一组任务 |
| 模块 | 总控（M3，带模块上下文）；v0.1 作为插话发给该模块的运行中任务 | 只影响该模块的计划和任务 |
| 运行中的任务 | 该任务的执行 Agent | 插话：v0.1 按 FR-109 随下一次命令行调用送达；M3 中支持运行中注入的 harness 立即送达，否则排队到下一次续跑 |
| 中断卡片 | 该中断 | 作为自由回答 |

**FR-67** M3 中总控生成的动作计划影响 2 个及以上任务，或涉及任何慢层对象时，应先在画布上预览，人确认后才执行。涉及慢层的部分只生成草稿提案。

- **FR-109、FR-68、FR-69** 在核心规格中（见核心规格附录 B）。
- **FR-70** 在核心规格中（见核心规格附录 B）；开发套件另有快捷命令 `/retry`，重跑选中的任务。
- **FR-71、FR-72** 在核心规格中（见核心规格附录 B）。

## 14. 错误分类

事件聚合、健康色与关注清单在核心规格 7.1、9.3 节。

### 14.3 错误分类

| 类别 | 判定来源 | 默认处理 |
| --- | --- | --- |
| 外部 | harness 的重试事件、网络或鉴权类退出码 | M3 由运行器自动重试；v0.1 由 harness 自己处理；不着色 |
| 执行 | 验证失败；报告 `suspected_class: execution` | v0.1 由 Agent 修改后重新 `finish`；M3 由总控重试或改写任务；最多 3 次 |
| 设计 | 报告 `suspected_class: design`；执行类失败 3 次；验收标准互相冲突 | 生成错误中断，进入关注清单 |

这套分类来自红军的做法：外部错误是外部性，执行错误和设计错误分别指向内环和设计阶段，所有错误都是对流程的测试反馈。[^hj-note]

## 15. 开发场景的画布操作

### 15.5 人的操作（开发套件部分）

画布与人的 6 个动作在核心规格 3.2、第 8 节。下表是开发套件中各对象上的操作；v0.5 的"固定关注"（pin）已由核心的跟进项代替（核心规格 FR-134）。

| 对象 | 操作 | 系统动作 |
| --- | --- | --- |
| 任务 | 暂停 | 运行器发送中断，等待到检查点，状态变"已暂停" |
| 任务 | 继续 | 支持续跑的 harness 用会话 ID 续跑；否则新开运行，附上次报告 |
| 任务 | 重派 | 换 harness 或模型，新开运行，保留分支 |
| 任务 | 要求汇报 | 运行器在下一个回合结束后，让 Agent 写一份中间报告 |
| 模块 | 新增任务 | 写入 L4 任务，状态"待定" |
| 提案 | 批准、驳回、要求补充 | 见核心规格 9.2 节 |
| 基线 | 进入草稿模式 | 见 J4 |
| 全局 | 暂停全部 | 所有运行到检查点后停止，调度器停止派发 |
| 中断 | 回答、批准、授权、拒绝、豁免 | 见第 12 节；回答可选择记为决策或规则 |
| 任意 | 输入提示词 | 见 12.5；按选中范围发给总控或 Agent |

v0.1 中 Tower.md 不控制 harness 进程：暂停、要求汇报以插话形式送达（FR-109），Agent 在下一个检查点停下或汇报；重派等于释放租约、任务回到就绪；暂停全部时 hub 拒绝新的领取，并向所有在跑的 Agent 送达暂停插话。

## 16. 接口

核心的命令行、守护进程 API 与 hub 协议见核心规格第 11 节。为便于对照，16.1、16.2 节保留 v0.5 的完整表格；其中 `ask`、`propose`、`status`、`snapshot`、`hub rebuild` 等行属于核心。

### 16.1 命令行

> 2026-10-21 修订：命令名以[命令行规格](2026-10-21-spec-towermd-cli.md)为准。本套件的命令重构到 `towermd dev` 下（`dev scan`、`dev draft`、`dev check`、`dev finish`、`dev trailers`、`dev seed`），不保留旧名；批准类动作可以经在场验证从命令行发出。下表是 v0.5 原表，其中的命令名已作废。

| 命令 | 作用 | 谁用 |
| --- | --- | --- |
| `towermd init [path]` | 询问用户文档目录，创建 `.towermd/` 配置与 `AGENTS.md` | 人、`towermd-init` |
| `towermd up` | 启动守护进程与 App | 人 |
| `towermd status` | 当前项目、hub 连接、可领取任务、待送达的插话与回答 | 人、Agent |
| `towermd baseline scan` / `towermd baseline scan --wait <scan-id>` / `towermd baseline submit <json>` | 冷启动 S1–S5 异步返回扫描 ID，`--wait` 每次最多等 90 秒，未完成时提示再次等待；提交 S6 结果生成草稿基线；不增加独立的 `baseline verify` | `towermd-baseline` |
| `towermd seed "<一句话>"` | 写入意图 I-1 草稿，打印一句话生长的后续步骤 | 人、`towermd-seed` |
| `towermd task add` / `claim` / `show` / `release` | 新增任务（Agent 建的任务受 FR-16 限制）；领取任务并取得租约；打印任务卡；放弃任务 | 人、Agent |
| `towermd ask` / `towermd review request` | 提问、请求评审；每次最多等 `ask_wait`，可用 `--wait <id>` 续等，累计到 `live_wait_max`（FR-59） | Agent |
| `towermd propose` | 提出慢层变更提案，可附 diff | Agent |
| `towermd check` / `towermd finish` / `towermd trailers <task>` | 路径守卫；守卫加验证并记录证据；生成合并 trailer | `towermd-finish` |
| `towermd snapshot` | 立即写一次快照（需持有主干租约） | 人 |
| `towermd hub rebuild` | 发起从快照重建 hub 的请求并返回请求 ID，不续租；须在 App 中确认后由守护进程执行 | 人 |
| `towermd rebuild-index` | 重建本机派生索引 | 人 |
| `towermd pause --all` / `towermd resume --all` | 暂停或恢复全部执行（恢复须在 App 中确认） | 人 |

命令行不提供批准类命令（FR-95）。命令行发出的事件一律署名 `process:towermd-cli`，即使是人在终端里执行（FR-111）。所有需要运行的命令都接受 `--run <id>`；省略时按当前工作树与分支判定（FR-113）。

### 16.2 守护进程 API（本机）

只绑定 127.0.0.1，供 App 与命令行使用。

| 方法与路径 | 作用 |
| --- | --- |
| `GET /state?since=<seq>` | 全局状态与增量 |
| `GET /canvas`、`GET /objects/:id` | 读取 `main` 上的 `tower.canvas` 与慢层对象 |
| `GET /modules/:id`、`GET /tasks/:id`、`GET /runs/:id/events` | 下钻查询 |
| `POST /baseline/coldstart`、`POST /seed` | 启动两条起步路径；冷启动立即返回扫描 ID |
| `GET /drafts/:id` | 读取可重建的冷启动草稿（模块对象、画布与未划分区） |
| `POST /rebuild-request` | 登记重建请求到守护进程运行时目录，返回请求 ID；请求不续租 |
| `POST /hub/rebuild/:request_id/confirm` | 仅接受 App 客户端；构造确认并执行 FR-93 的重建；CLI 不提供确认命令 |
| `POST /drafts`、`POST /proposals/:id/approve`、`POST /proposals/:id/reject` | 草稿与提案；批准只接受来自 App 的请求 |
| `POST /tasks`、`POST /tasks/:id/{pause,resume,release,report}` | 任务操作 |
| `GET /holds`、`POST /holds/:id/resolve` | 查询与处理中断 |
| `POST /prompts` | 提交人工提示词，参数为范围与文本 |
| `POST /rules/temp` | 创建临时规则 |
| `WS /events` | 事件推送 |

凡是以 `human:<id>` 署名的写操作（批准、回答、提示词、临时规则、暂停与恢复）只接受来自 App 的请求；命令行的请求按 `process:towermd-cli` 记录，不能产生人的动作（FR-111）。v0.1 只按客户端类型区分，不做加密校验（5.4）。

### 16.4 交互网关的 MCP 工具（M3）

M3 的无头运行通过交互网关与 Tower.md 交互；语义与 `towermd ask`、`towermd review request` 相同。

| 工具 | 输入 | 返回 | 阻塞 |
| --- | --- | --- | --- |
| `towermd_ask` | 问题、选项、推荐项、上下文路径 | 回答，或 `deferred` | 是，最长 `live_wait_max` |
| `towermd_request_review` | 评审类型（计划、代码、设计）、产物路径、摘要 | 结论与意见，或 `deferred` | 是 |
| `towermd_permission` | harness 转来的授权请求 | 允许或拒绝 | 是，最长 10 分钟 |
| `towermd_note` | 进度备注 | 确认 | 否 |

`towermd_permission` 的输入输出格式应遵循各 harness 的授权工具协议，例如 Claude Code 的 `--permission-prompt-tool`。[^cc-cli]

## 17. 非功能需求（开发套件部分）

核心的非功能需求在核心规格第 12 节。开发套件另有以下 2 条：

| ID | 要求 |
| --- | --- |
| NFR-07 可替换（harness） | v0.1 中任何能执行命令行、加载技能的 harness 都能接入；M3 新增无头 harness 只需新增适配器，不修改调度、集成和画布代码 |
| NFR-09 技能可复现 | M3 的无头运行在任何机器上加载相同版本、相同哈希的技能包；v0.1 只记录会话报告的技能版本 |

## 18. 里程碑与验收

M0、M1 已按 v0.5 第 18 节完成，并以 v0.1.1 预览版发布，验收记录见 `docs/verification/` 与 `docs/qa/`。下表是开发套件的后续里程碑；核心的阶段见核心规格第 14 节，Cloudflare hub 已移入核心的基础设施。

| 里程碑 | 范围 | 验收标准 |
| --- | --- | --- |
| M0、M1（已完成） | 见 v0.5 第 18 节 | 见 v0.5 第 18 节与验收记录 |
| M2 | import 解析与社区发现（8.1 节）；草稿模式下的模块编辑；一句话生长（`towermd-seed`，第 9 节）；生长信号；门禁 G1、G4；模块验证命令改为内置检查（FR-121） | 从一句话长到 5 个模块，每次人定内容的变更都有决策和标签；在一个真实仓库上，人工合并、拆分模块的次数比 M1 减少 |
| M3 | 总控双环、调度器、运行器、集成器串行合并队列（第 10 节）；Claude Code、Codex、Cursor、Gemini 适配器；交互网关（实时等待、评审、授权路由）与运行中插话；技能包锁定与组装 | 4 个无头 Agent 并发跑 20 个任务，`main` 始终可通过验证；越界改动在合并前 100% 被拦截 |

## 20. 风险与已定问题

| 风险或问题 | 影响 | 当前处理 |
| --- | --- | --- |
| harness CLI 参数变化快 | 适配器失效 | `probe()` 检查版本与能力；适配器单独发版 |
| 冷启动聚类不准 | 模块边界错误，跨模块任务多 | 置信度可视化；人审阅；跨模块比例作为生长信号 |
| 串行合并成为瓶颈 | 并发高时排队变长 | v0.1 以推送被拒作并发检查；M3 后评估合并批处理 |
| 模块租约降低并发 | 同一模块的任务只能排队 | 这是有意的取舍；排队长度作为拆分模块的信号 |
| 生长阈值是猜测 | 提案过多或过少 | 阈值可配置；在 M2 用真实项目校准 |
| 订阅制与 API 计费混用 | 费用统计口径不一 | 费用字段注明来源（harness 报告或估算） |
| 技能被跳过 | v0.1 中 Agent 跑在用户会话里，可能不调用 `towermd finish` 就合并，或直接改慢层文件 | `.towermd/AGENTS.md` 与技能写明规矩；守护进程事后检测并生成禁止中断（FR-103）；M3 的集成器在合并前拦截 |
| 快照提交增加 `main` 的提交数 | 历史变吵 | 只在有变化时按间隔写，默认 30 分钟；提交只改 `state/`，带固定前缀与 trailer，便于过滤 |
| 技能包与 Tower.md 职责冲突 | 技能自行建分支、开拉取请求，破坏分支策略 | 禁用冲突技能；`task.md` 明确覆盖；G2 拦截 |
| 实时等待占用资源 | 大量进程常驻 | `live_wait_max` 后转停车续跑 |
| 技能包版本漂移 | 同一任务在不同机器上行为不同 | 锁定版本与哈希（NFR-09） |
| Arca 多机同步尚未完成 | 大文件只在产生它的主机上，其他主机看不到证据原文 | 证据摘要与哈希经 hub 可见；原文标注所在主机；`arca doctor` 提示未同步文件 |
| 授权协议变化 | 授权路由失效 | 适配器 `probe()` 检测能力；失效时退回 `--permission-prompts none` 并记禁止事件 |

v0.3 的 7 个待定问题，v0.4 按知识库中的依据定下 4 个，其余 3 个注明最晚在哪个里程碑前定；v0.5 新增的问题从第 8 个起：

| # | 问题 | 结论 | 依据 |
| --- | --- | --- | --- |
| 1 | 总控用哪种 harness、哪个模型档位，能否按项目切换？ | 已定：默认中档，经 `config.yaml` 的角色映射按项目或模块切换（FR-30）；harness 默认 Claude Code | 分层态势设计 4.8 节把"汇总状态、生成关注清单"归为中档模型[^design] |
| 2 | 运行原始输出在 Arca 中保留多久，何时执行 `arca gc`？ | 已定：Tower.md 从不调用 `arca gc`，保留与清理由用户按 Arca 的规则决定 | Arca 规格："GC 只能显式触发"，"默认安装不销毁任何东西"[^arca-spec] |
| 3 | 豁免的默认有效期是多少？ | M2 前定。豁免必须带到期时间这一点不变 | 无，需人定 |
| 4 | 定型期的模块数阈值是多少？ | M2 前定，用真实项目校准（见本节风险表"生长阈值是猜测"） | 无，需人定 |
| 5 | 画布地标样式（如把 Agent 模块画成机器人）是否进入 v0.1？ | 已定：M0–M2 只保存模块的 `landmark` 文字；地标图形样式与 pin、透镜阈值一起作为个性化设置，放入 M3。2026-10-21 修订：pin 已由核心的跟进项代替（核心规格 FR-134）；地标图形属于用户用法，不进核心 | 红军建议"Agent 放机器人"[^hj-note]；分层态势设计 4.6 节把地标样式列为个性化，第 6 节放在 v3[^design] |
| 6 | 提问中断超时后，总控能否"带假设继续"？ | 已定：不能自动继续。超时只升级到关注清单（12.1）；"按你的假设继续"只能由人在卡片上选择，选择记 `human:<id>` 事件 | 取舍顺序"可追溯优先于不打扰"；不变量"不自动放行"[^principles] |
| 7 | 首批锁定哪些技能包，执行 Agent 默认启用哪些技能？ | M3 前定（技能包锁定与组装属于 M3）；10.6 中的 superpowers 片段只是示例 | 无，需人定 |
| 8 | v0.1 由谁来跑 Agent？ | 已定：技能优先，调度后置。用户在自己的会话中加载 Tower.md 技能；无头调度与合并队列放到 M3 | 用户决定[^owner-1016pm] |
| 9 | 多工作树、多机时状态放在哪里？ | 已定：活跃状态以唯一 hub 为准，单机为本机守护进程，多机为用户自部署的 Cloudflare Worker；快照存回用户文档目录 | 用户决定[^owner-1016pm]；mdrelay 与项目分享方案的 Durable Object 先例[^mdrelay][^share-design] |
| 10 | 设计原则信念二与 `AGENTS.md` 不变量如何改写？ | 已定：信念二改为"活跃状态以唯一 hub 为准，沉淀状态以仓库为准，二者用快照衔接"；不变量"单一写入点"改为"一处定序，一个提交者"；本地优先允许用户自部署云端 hub；密钥只来自环境变量或系统钥匙串，不写入仓库；`human:` 动作只能从 App 发出。axum 与 WebSocket 客户端两类新依赖已确认；签名与钥匙串库随安全加固再定 | 用户确认[^owner-1016pm] |
| 11 | 安全（认证、签名、加密）何时做？ | 已定：v0.1 不做，将来再考虑；先靠结构性规则（命令行没有批准命令）防 Agent 粗心。待做项列在 5.4"安全加固" | 用户决定[^owner-1016pm] |

M0 的验收仓库用 `~/git/note.md`。依据：分层态势设计建议 v0 先在李峻自己的项目上试，个人工具类项目最好把控。[^design]

## 21. 需求追溯

本表沿用 v0.5，表中节号为 v0.5 的节号，包括已移入核心规格的条款。

| 需求 | 来源 |
| --- | --- |
| G1、FR-40–42 关注清单与"自上次查看" | 红军"18 件里关注 3 件""早上第一句问到哪了"；李峻"记得住才会问"[^hj-note][^mh-note] |
| FR-43–46 慢层定布局、快层叠加 | 分层态势视图设计；Simon 频率分离；红军"位置是记忆的锚"[^design][^simon-nd] |
| FR-02、FR-17、第 13 节 变更控制 | NASA 配置管理；SWEBOK 基线；用户批注"慢层由人架构绘制变更"[^nasa-65][^swebok-scm][^design] |
| 第 10 节 双环与错误分类 | 红军双环实践与错误分类[^hj-note] |
| FR-32–33 进度只认证据 | Simon 进展线索[^simon-cue] |
| FR-14–16、第 9.3 节 租约与生长信号 | Simon 近可分解[^simon-nd] |
| 第 9 节 生长期与定型期 | 前期研究"MVP 前自然生长，产品化后先立慢层"；Brand 未精装区[^layered] |
| FR-22 每步可运行 | Simon 钟表匠寓言[^simon-watch] |
| 第 10.5 节 模型分层 | 明豪讨论；红军便宜模型维护进度图[^mh-note][^hj-note] |
| FR-47–48 快层只读、慢层草稿 | 李峻"图不允许用户修改"与用户批注的折中[^design] |
| 第 10.3 节 适配器 | 各 harness 官方文档[^cc-headless][^cc-cli][^codex-exec][^cursor-headless][^gemini-headless] |
| 0.2、6.4、FR-75–78 Arca 存储层 | 用户 2026-10-16 说明大文件由其项目 Arca 管理[^owner-1822][^arca] |
| FR-79–82、6.2 OKF 字段、6.3 actor、FR-31 校准、NFR-10–11 | 设计原则第 5 节对齐建议；OKF v0.2[^principles][^okf] |
| 0 节产品组成、5.2–5.4 三个面与 hub、FR-83–99、FR-103–113、6.5 `tower.canvas`、10.0 技能优先 | 用户 2026-10-16 下午的产品形态说明与架构决定[^owner-1016pm]；Note.md 画布、mdrelay 与分享方案[^canvas-design][^mdrelay][^share-design]；fencing token[^ddia]；不让模型做复杂 git 操作[^pi] |
| G7–G8、10.6、第 11–12 节、15.7–15.8 技能包、门禁、中断、图标与提示词 | 用户 2026-10-16 补充需求[^owner-1811]；明豪讨论"随时打断、及时干预"[^mh-note]；superpowers 交互点[^superpowers] |

[^v05]: [Tower.md 规格 v0.5 全文](../archive/2026-10-21-pre-core-split/2026-10-16-spec-towermd-layered-agent-canvas-mvp-v0.5.md)（提交 `a7feeaa`）。
[^core]: [Tower.md 核心规格](2026-10-21-spec-towermd-core.md)。
[^owner-1910]: 2026-10-21 19:10 用户对产品边界文档第 10 节的决定：接受"核心规格 + 开发套件规格"的拆分。
[^boundary]: [产品边界文档](../../../sotvault/project/towermd/2026-10-21-product-towermd-boundary-core-extensions-emergent.md)，4.2 节与附录归属总表。
[^layered]: [分层架构研究](../../../sotvault/research/2026-09-23-claude-mvp-growth-then-layered-architecture.md)，第 2 节与第 4 节。
[^design]: [分层态势视图设计](../../../sotvault/research/2026-10-16-hemory-layered-situation-view-fast-slow-layers.md)，第 3–4 节与第 8 节。
[^mh-note]: [明豪讨论整理](../../../sotvault/hemory/2026-10-16-meeting-agent-engineering-dashboard.md)；原话见 [逐字稿](../../../sotvault/ssot/meetings/20261016_151704/transcript.md) 第 2–73 行。
[^owner-1016pm]: 2026-10-16 下午用户在对话中的说明：Tower.md 是一套类似 superpowers 的技能加独立 App，初始化到用户 git 项目，产生 `tower.canvas` 与作为引用源的 md 文档，跟随仓库版本管理；`.towermd/` 只放隐藏配置，md 文档放用户指定的目录；多 harness 在不同分支、工作树乃至多台服务器上工作，需要一个全局协调点（Cloudflare Worker）；活跃实时状态以唯一服务为主，快照存回仓库；v0.1 选"技能优先，调度后置"。 随后用户确认按评审建议修订设计原则信念二、`AGENTS.md` 不变量，并确认四类新依赖（"都按你的改"）。 之后用户补充：多个终端里可能各开多个 Agent 在不同分支干活，命令行要支持；共同配置在仓库初始化时生成唯一 ID，以此做多机协作；安全将来再考虑。
[^okf]: [OKF v0.2 格式约束](../../../note.md/docs/okf-v0.2-format-constraints.md)，第 4、5、7 节。
[^principles]: [Tower.md 设计原则](../product-principles.md)，第 5 节"对 MVP 规格 v0.3 的对齐建议"。
[^arca]: [wizlijun/arca](https://github.com/wizlijun/arca) README，2026-10-16 读取。
[^owner-1017-remediation]: 2026-10-17 用户对 [M0 整改总表](../archive/2026-10-20-pre-skeleton/plans/2026-10-17-m0-remediation.md) 的 D0–D6 明确答复“全部按推荐方案执行”：D0 现状按 M0-1/2/3/4 四批显式路径提交，不含 S5；D1 私有快照引用与 CAS；D2 App 客户端确认端点，M0 端到端验证、真人点击 M1；D3 共用 FR-107 分离工作树验证器，继承环境、允许网络并回收整个进程组；D4 init 本地优先、尽力检查；D5 追认任务路径上限、先工作树再领取、权威 main 的 Decision 判定，并改为有界快照恢复元数据；D6 保留覆盖率并加每层模块数与验证命令/无测试候选任务检查。
[^simon-nd]: [Simon 1962](../../../sotvault/ssot/books/2026-10/The Architecture of Complexity/book.typeset.md) 第 835–875 行与第 1210–1250 行附近，近可分解与频率分离。
[^cc-headless]: [Claude Code：Run Claude Code programmatically](https://code.claude.com/docs/en/headless)，2026-10-16 读取。
[^codex-exec]: [Codex：Non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode.md)，2026-10-16 读取。
[^simon-watch]: [Simon 1962](../../../sotvault/ssot/books/2026-10/The Architecture of Complexity/book.typeset.md) 第 500–540 行与第 790–800 行附近，钟表匠寓言与稳定中间形态。
[^pi]: [Mario Zechner 谈最小 harness](../../../sotvault/youtube-summaries/2026-08-18-youtube-pi-mario-zechner-minimal-harness.md)，第 136 行。
[^hj-note]: [红军实践经验整理](../../../sotvault/hemory/2026-10-15-meeting-hongjun-practices-and-reasons.md)，第 2–3 节；原话见 [逐字稿](../../../sotvault/ssot/meetings/20261015_124348/transcript.md) 13:04–13:12、18:07–18:24。
[^cc-cli]: [Claude Code：CLI reference](https://code.claude.com/docs/en/cli-reference)，2026-10-16 读取；涉及 `--bare`、`--plugin-dir`、`--add-dir`、`--permission-prompt-tool`、`--permission-prompts`、`--input-format`、`--replay-user-messages`、`--max-turns`。
[^cursor-headless]: [Cursor CLI：Headless mode](https://cursor.com/docs/cli/headless)，2026-10-16 读取。
[^gemini-headless]: [Gemini CLI：Headless mode reference](https://www.geminicli.com/docs/cli/headless)，2026-10-16 读取。
[^superpowers]: [Claude Superpowers Skill Guide](https://mcp.directory/blog/claude-superpowers-skill-guide)，2026-10-16 读取；第三方介绍，技能清单与交互点以锁定版本的原文为准。
[^arca-spec]: [Arca 规格](../../../sotvault/Sync/2026-08-04-ARCA-SPEC.CN.md)，第 7 节"删除、保留与 GC"。
[^mdrelay]: [Note.md mdrelay README](../../../note.md/mdrelay/README.md)：SQLite Durable Object、配对码换设备令牌、vitest-in-workerd。
[^share-design]: [Note.md 项目文档分享方案对话](../../../sotvault/agent-sessions/openai/oldbruce@gmail.com/codex/mdeditor/2026-10-10-现在有一个确切的需求-我要把一个项目中的文档分享给其他人时文档中包含了引用其他文档也需要一起分享给其他人-再进一.md)，第 122–126 行与第 215 行：每个空间一个 Durable Object，权威状态不用 KV。
[^nasa-65]: [NASA SEH 6.5 Configuration Management](https://www.nasa.gov/reference/6-5-configuration-management/)，2026-10-16 读取。
[^swebok-scm]: [SWEBOK v4](../../../sotvault/ssot/books/2026-10/swebok v4/book.typeset.md) 第 8 章 2.3 Baseline Identification，第 15173–15205 行附近。
[^simon-cue]: [Simon 1962](../../../sotvault/ssot/books/2026-10/The Architecture of Complexity/book.typeset.md) 第 625–700 行附近，进展线索。
[^owner-1822]: 2026-10-16 18:22–18:46 用户在对话中的改名要求、定位说明与定名 towermd，以及大文件由 Arca 管理的说明。
[^canvas-design]: [Note.md 无限画布设计](../../../note.md/docs/superpowers/specs/2026-09-04-infinite-canvas-design.md)，3.1 节 JSON Canvas 1.0、7.1–7.2 节宽容读取与安全写回、9.1 节路径与解析根。
[^ddia]: [Designing Data-Intensive Applications](../../../sotvault/ssot/books/2026-08/Designing Data-Intensive Applications/book.typeset.md)，第 12958–12972 行，fencing token。
[^owner-1811]: 2026-10-16 18:11 与 18:14 用户在对话中的补充需求。
