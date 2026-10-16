---
type: Principle
title: Tower.md 设计原则：让 Agent 去做，人只在关键处放行
description: Tower.md 的五条核心信念，以及与 Note.md 成对的三条接口原则。逐条对应 Note.md 的产品原则：Note.md 管知识，Tower.md 管行动，两者共享 AI 原生和本地优先的地基。作为产品与架构决策的准绳，并列出 MVP 规格 v0.3 需要对齐的地方。
tags: [towermd, notemd, 产品原则, 设计原则, AI 原生, local-first, file-over-app, 快慢层, 放行]
generated: { by: claude-code/opus-5.5, at: 2026-10-16T10:10:00Z }
sources:
  - id: note-readme
    resource: ../../note.md/README.zh-CN.md
    title: note.md 五个信念
  - id: note-claude
    resource: ../../note.md/CLAUDE.md
    title: note.md 产品主张（设计决策准绳）
  - id: note-relations
    resource: ../../note.md/docs/product-principle-relationships-only-grow-where-confirmed.md
    title: 产品原则：关系只在人确认处生长
  - id: note-mirror
    resource: ../../note.md/docs/product-principle-mirror-hosted-marks.md
    title: 产品原则：你的批注属于 vault，不属于路径
  - id: note-vault
    resource: ../../note.md/docs/product-principle-one-vault-many-agents-you-orchestrate.md
    title: 产品原则：一个 vault，多个 agent，你是编排者
  - id: note-humans
    resource: ../../note.md/docs/2026-07-15-what-humans-do-while-agents-run.md
    title: 当 agent 在跑，人该做什么（并行的有界最优、数据归属）
  - id: spec
    resource: specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: Tower.md MVP 规格 v0.3
status: draft
---

# Tower.md 设计原则：让 Agent 去做，人只在关键处放行

> 状态：设计原则草案 / 外宣素材
> 适用：Tower.md 的全部产品与架构决策；与 Note.md 的五个信念成对使用

## 一句话

> **Note.md：读 AI 写的，留下你想的。**
> **Tower.md：让 AI 去做，只在关键处放行。**

Note.md 管知识，Tower.md 管行动，关系类比 Confluence 与 Jira。[^spec] 两者共享两块地基：**AI 原生**和**本地优先**。在这两块地基上，Tower.md 的每条原则都对应 Note.md 的一条原则：Note.md 在知识侧说了什么，Tower.md 就在行动侧说同一件事。

## 0. 共同地基：AI 原生、本地优先

| 地基 | 含义 | Note.md 的落点 | Tower.md 的落点 |
| --- | --- | --- | --- |
| AI 原生 | 主要的读写者是 Agent，状态必须是 Agent 不需适配就能读写的纯文本 | vault 约定：`AGENTS.md`、块引用、`.note.md`、`[[wikilink]]` | 用户文档目录里的 md 与 `tower.canvas`、hub 事件、状态快照；Agent 在自己的 harness 会话里经 Tower.md 技能和 `towermd` 命令行读写 |
| AI 原生 | 人只做 AI 做不了的那一步 | 阅读、判断、批注、确认记忆 | 批准慢层、回答中断、授权、豁免 |
| AI 原生 | 用你已有的 AI | 不另售 token，不对 token 加价 | 调用你已安装的 harness 和已有订阅，同样不另售、不加价 |
| 本地优先 | 本机副本是主副本 | vault 是磁盘上的纯 `.md` 文件夹 | 代码仓与用户文档目录；hub 默认在本机，云端 hub 可选、由用户自部署；守护进程只绑定 127.0.0.1 |
| 本地优先 | 索引是派生数据 | 搜索索引可以重建 | SQLite 索引、工作树可删除，凭 hub、git 与 Arca 恢复；hub 可从仓库中的快照重建 |
| 本地优先 | git 管同步与历史 | vault 走 git sync | 基线标签、状态快照、合并 trailer |

Jira 是人填表、给人看；Tower.md 是 Agent 写事件、人看态势。Confluence 是人写、人读；Note.md 是 AI 写、人读并留下判断。"AI 原生"指的就是这个分工的翻转。

## 1. 对照总表

| # | Note.md（管知识） | Tower.md（管行动） | 共同立场 |
| --- | --- | --- | --- |
| 1 | AI 的文字无限，注意力有限，判断才是残余 | Agent 的动作无限，注意力有限，**放行才是残余** | 人的注意力是唯一稀缺的资源 |
| 2 | 文件高于应用 | **仓库高于服务** | 事实源是纯文本，应用可以删掉 |
| 3 | 它建议，你确认；关系只在人确认处生长 | **它推进，你放行；结构只在人批准处改变** | AI 的产出默认不可信，直到人确认或有证据 |
| 4 | 你的批注属于 vault，不属于路径 | **你的回答属于仓库，不属于会话** | 人留下的东西要挂在稳定、git 版本化的宿主上 |
| 5 | 一个 vault，多个 agent，你是编排者 | **一个仓库，多个 harness，你是塔台** | 工人可以换，地盘中立，决定权在人 |

第 6–8 条是两个产品之间的接口原则，见第 3 节。

## 2. 五条核心信念

### 信念一：Agent 的动作无限，你的注意力有限——放行才是残余

> Agent 可以并行跑一整夜，你一天只有几次清醒的查看。Tower.md 不汇报一切，只把需要你放行的少数事送到你面前；你的每一次放行都会被记下，同一个问题不问第二遍。

**与 Note.md 的对应。** Note.md 的信念一说，人留在字里行间的判断是任何模型都生成不出的残余。[^note-readme] 在行动侧，这个残余是**放行**：批准、回答、授权、豁免。Note.md 的研究还指出，并行有"有界最优"，超过之后人就成了瓶颈。[^note-humans] Tower.md 要做的，就是把这个瓶颈的消耗压到最低。

**核心主张：**

- **默认沉默。** 快层由 Agent 自治，越过阈值时才打扰人。塔台不该比机组更吵（对应 Note.md："你的第二大脑不该比你更聒噪"[^note-relations]）。
- **只给 3 件。** 首屏关注清单最多 3 条，每条写清 4 件事：发生了什么、影响多大、建议怎么做、需要你做什么。
- **先代答，再打扰。** 总控能在基线权限内回答的问题，不交给人。
- **不问第二遍。** 人的回答可以记为模块决策或流程规则，之后的同类问题由总控引用这条决策作答。

**不做：** 不做滚动日志墙；不用"Agent 正在思考"一类的动画抢注意力；不把通知数量当作活跃度。

**工程落点：** G1、FR-40–42、FR-62、12.3、12.4；度量"关注项采纳率""每任务人工中断数"。[^spec]

### 信念二：仓库高于服务（repo over service）

> 执行状态最终沉淀为代码仓里的纯文本。活跃状态由唯一的 hub 协调，定期快照回仓库；守护进程、索引、画布乃至 hub 都能从仓库重建，丢失范围不超过一个快照间隔。

**与 Note.md 的对应。** Note.md 的信念二是"文件高于应用：索引是派生数据，文件是唯一事实源"。[^note-readme] Tower.md 把这条从笔记推到项目执行状态：Jira 的 issue 住在别人的数据库里，Tower.md 的 issue 住在你的代码仓里，和代码共用同一段提交历史。

**核心主张：**

- **按变化速度分来源，每种只有一个权威。** 慢层 md 与 `tower.canvas` 以 `main` 上的用户文档目录为准；活跃的快层状态以唯一的 hub 为准，并定期快照进 `main`。git 管可变文本，Arca 管不可变大文件，都按仓库相对路径访问。多分支、多工作树、多台机器都不各存一份状态。
- **一处定序，一个提交者，多个读者。** 事件只由 hub 一处定序；`main` 上的快照和慢层提交只由持有主干租约的守护进程写入。任何 Agent、`grep`、编辑器都可以直接读。
- **派生物都能删除。** SQLite 索引、工作树、画布缓存删除后能重建；hub 丢失后能从快照和各主机发件箱重建。
- **状态和代码同版本。** 检出任意一个提交，就能看到当时的基线、`tower.canvas`、决策和最近一次状态快照。

**不做：** 不运营 Tower.md 自己的云服务；云端 hub 可选，由用户部署在自己的账号下；不要求登录才能看自己项目的状态，快照就在仓库里；不让关键状态只留在 hub 里而不快照；运行时不依赖 GitHub Issues、PR 这类托管平台。

**v0.5 修订。** 这条信念原来写的是"执行状态是代码仓里的纯文本""不建云端中枢""守护进程是 `.towermd/` 唯一的写入者"。多个 harness 在不同工作树、不同机器上并行时，状态若存在工作树文件里，每个分支都会各有一份，只能事后合并；所以 2026-10-16 改为"活跃状态以唯一 hub 为准，沉淀状态以仓库为准，二者用快照衔接"。

**工程落点：** FR-83–94、NFR-01、NFR-02、NFR-12、FR-05、FR-07、FR-08、6.4。[^spec]

### 信念三：Agent 推进，你放行——结构只在人批准处改变

> Agent 可以自主写代码、跑测试、合并到主干；但目标、架构和验收标准（慢层）只在你批准处改变。Agent 说自己完成了不算完成，有证据才算。

**与 Note.md 的对应。** Note.md 把"捕获"和"结网"分开：关系是人确认的副产品，不是导入的副作用。[^note-relations] Tower.md 把"执行"和"定结构"分开：**结构变化是人批准的副产品，不是 Agent 执行的副作用。** Note.md 说"模型推断出一句话，不代表它能自动成为可信记忆"[^note-readme]；Tower.md 说"Agent 报告 done，不代表任务完成"。

**核心主张：**

- **快慢分治。** L5 由 Agent 自治，只记事件；L3–L4 由总控决定并留痕；L0–L2 只能由人批准。
- **越界就转为提案。** Agent 的改动触及慢层时，不是简单拒绝，而是转成附带影响分析的提案，等人批准。
- **进度只认证据。** 没有验证的完成，在画布上显示为"已声称、未验证"。
- **一眼看出是谁定的。** 画布上实线表示人批准过，虚线表示拟议中；事件里 `human:` 署名与 Agent 署名不混用。这对应 Note.md 的"`✦` 是 AI 写的，`●` 是你想的"。[^note-claude]
- **不靠超时放行。** 评审不会因超时自动通过；授权超时默认拒绝。

**不做：** 不让 Agent 自动"优化"架构图；不让提示词绕过慢层保护或硬门禁；不用 Agent 自评代替验证。

**工程落点：** FR-01–03、FR-17、FR-33、FR-45、FR-47–48、FR-64、FR-72、第 13 节。[^spec]

### 信念四：你的回答属于仓库，不属于会话

> harness 会话、工作树、进程都是一次性的。你在中断里给出的回答、授权和豁免，一旦落笔，就成为仓库里的决策、规则或证据。换一个 Agent、换一台机器、换一家 harness，它都还在。

**与 Note.md 的对应。** Note.md 发现文件路径是脆弱的，所以把批注挂在 vault 内 git 版本化的镜像上，而不是挂在原文的路径上。[^note-mirror] Tower.md 面对的脆弱宿主是**会话**：你在某个终端里回答了 Claude Code 的一个问题，会话结束后这个回答就消失了，下一个 Agent 还会再问一遍。Tower.md 让人的输入挂在稳定的宿主上。

**核心主张：**

- **回答都有去处。** 只用于本次、记为模块决策、记为流程规则、转为慢层提案，四选一，由人勾选。
- **证据绑定提交，不绑定工作树。** 工作树在合并后删除，证据仍然指向提交和 Arca 路径。
- **位置属于基线，不属于实时状态。** 模块在画布上的坐标是慢层，快层变化从不移动节点；位置是记忆的锚。
- **不往别人的地盘写私货。** 运行配置由运行器显式传入，不改用户全局的 git 配置和 harness 配置（对应 Note.md 的"原文保持纯净"）。

**不做：** 不把人的回答只留在某个会话记录里；不让证据依赖会被清理的目录。

**工程落点：** 12.3、FR-26、FR-43–44、FR-62、FR-75–76。[^spec]

### 信念五：一个仓库，多个 harness——你是塔台

> Claude Code、Codex、Cursor CLI、Gemini CLI 是可替换的机组，按各自的长处分工。Tower.md 是塔台：不开飞机，只掌握全局、给出放行、在冲突前叫停。仓库是中立地带，不属于任何一家 harness，也不属于 Tower.md。

**与 Note.md 的对应。** Note.md 说"agent 和模型是可替换的工人，vault 是不可替换的工地"。[^note-vault] Tower.md 把同一立场从知识的 vault 推到执行的仓库。区别在于：Note.md 的编排靠人在关键处阅读和判断；Tower.md 把编排本身做成显式的结构——角色到模型的映射、并发上限、模块租约、串行合并队列。

**核心主张：**

- **不绑定 harness。** 新增一个 harness 只需要实现一个适配器。
- **按长处和代价分工。** 慢层交给人或最强的模型；执行交给便宜模型；评审交给另一家厂商。
- **Tower.md 自己也能被移走。** 去掉 Tower.md 后，代码仓仍是标准 git 仓库（只有 `main`、标签和已删除的短期分支），`.towermd/` 配置、用户文档目录和状态快照仍是能读懂的文本。
- **不卖 token。** 复用你已有的订阅、API 账户或本地模型；费用如实显示，未知时标"未知"，不填 0。
- **克制并行。** 并发上限、模块租约、串行合并都是有意设的上限。并行是手段，不是指标。

**不做：** 不做吞掉一切的编排入口；不把 Agent 绑死在 Tower.md 的私有协议上（交互网关走公共的 MCP 协议）；不比谁同时开的 Agent 多。

**工程落点：** G6、NFR-05、NFR-07、10.3、10.5、FR-14、FR-31。[^spec]

## 3. 成对原则：Note.md 与 Tower.md 的接口

### 原则六：主张与承诺分开出版

Note.md 回答"凭什么相信"，真值契约是证据、来源和有效期。Tower.md 回答"谁在什么时间前负责什么"，真值契约是负责人、验收标准、关闭条件和证据。[^spec] 两边各管各的契约，**不保存对方的状态，只互相引用**。

- Tower.md 不存长篇知识。ADR 只写决策要点，背景研究链接回 Note.md。
- Note.md 不存执行状态，只接收已关闭的 issue 和决策摘要。
- 个人待办两边都不管，交给 NEXT。

### 原则七：两道人工闸门，每个方向一道

```mermaid
flowchart LR
  N["Note.md 知识"] -->|"闸门 1：人批准提案"| T["Tower.md 慢层"]
  T --> R["运行与证据"]
  R -->|"回写：纯 .md 摘要"| C["Note.md 捕获层"]
  C -->|"闸门 2：人读过并写手记"| N
```

两个方向各有一道人工检查点，所以回路闭合了，但 Agent 的产出不会绕一圈变成 Agent 眼中的"已知事实"。

- **知识进入行动**，只能经由人批准的提案。Note.md 里的一篇研究再有说服力，也不会自动修改基线。
- **行动回到知识**，Tower.md 只写入捕获层的纯 `.md`（OKF 文档，`generated` 署名为 Tower.md 的流程），**不创建 `.note.md`，不自动结网**。这份摘要要成为知识网络的节点，仍然要人读过、留下手记。这一条是 Note.md "关系只在人确认处生长"对 Tower.md 的直接约束。[^note-relations]

### 原则八：同一套契约，学一次两边用

一个学会了 Note.md 约定的 Agent，应该不用再学就能读懂 Tower.md 的状态。

| 约定 | Note.md | Tower.md |
| --- | --- | --- |
| 文件格式 | Markdown 加 YAML frontmatter，遵循 OKF v0.2，必填 `type` | 同左（`type` 待补，见第 5 节） |
| 署名 | `<producer>/<version>`、`human:<id>`、`process:<id>` | 同左（待对齐，见第 5 节） |
| 来源与生命周期 | `sources`、`generated`、`verified`、`status`、`stale_after` | 同左 |
| 给 Agent 的规矩 | vault 根目录的 `AGENTS.md` | `.towermd/AGENTS.md`（建议）加每次运行的 `task.md` |
| 隐藏目录 | `.notemd/` | `.towermd/` |
| 命令行 | `notemd` | `towermd` |
| Agent 工具协议 | `notemd mcp`（只读） | 交互网关 MCP |
| 跨产品引用 | 块引用 `((file#b-xxxxxx))`、`[[wikilink]]` | 引用 Note.md 知识时用稳定引用，不只用文件路径（建议） |

## 4. 原则冲突时的取舍顺序

1. **慢层保护与安全**（信念三）
2. **可追溯**（信念二、四）
3. **不打扰**（信念一）
4. **自治与吞吐**（信念五中的并行）

例子：

- 禁止类中断即使打扰人，也要立即显示。（1 优先于 3）
- 为了少打扰，总控可以代答，但代答必须留痕、可以被人推翻。（2 优先于 3）
- 并行合并能提高吞吐，但会破坏"`main` 始终可运行"，所以保留串行合并。（1 优先于 4）
- 授权无人回应时，宁可拒绝、让 Agent 换一条路，也不默认放行。（1 优先于 4）

## 5. 对 MVP 规格 v0.3 的对齐建议

对照上面的原则，规格中有以下几处需要补齐。规格 v0.4 已落实全部 8 条：第 1、2 条见 6.2、6.3 与 FR-82；第 3 条见 FR-81；第 4、5 条见 FR-79、FR-80；第 6、7 条见 NFR-10、NFR-11；第 8 条见 FR-31。

| # | 现状 | 建议 | 依据 |
| --- | --- | --- | --- |
| 1 | 6.2 的共有字段没有 `type` | 所有 `.towermd/` 对象加 OKF `type`（Intent、Module、Interface、Baseline、Decision、Proposal、Task、Run、Evidence、Hold、Gate、Rule），并像 Note.md 一样设一处唯一的取值登记 | 原则八 |
| 2 | 6.3 的 actor 取值为 `controller`、`agent:<run>`、`integrator`、`verifier` | 对齐 OKF：人用 `human:<id>`；守护进程内的组件用 `process:towermd-<组件>`；Agent 用 `<harness>/<version>`，run ID 放进 `data` | 原则八、信念三 |
| 3 | 慢层规则只通过 `task.md` 注入；用户自己在仓库里打开一个 harness 时看不到，只会在合并时被拦下 | 生成 `.towermd/AGENTS.md`（或在仓库的 `AGENTS.md` 中加一段），写明哪些路径属于慢层、如何提交提案 | 信念五、原则八 |
| 4 | 规格以 `../../../sotvault/...` 相对路径引用背景研究 | ADR 和提案引用 Note.md 知识时，记录 OKF `sources` 的 id 和块引用，路径只作提示 | 信念四 |
| 5 | 0.1 只说 Note.md 接收已关闭 issue 和决策摘要，没有规定形式 | 明确写成 OKF 文档，放到 vault 约定的目录；`generated.by` 为 `process:towermd`；`sources` 指回提交和 issue ID；不创建 `.note.md` | 原则七 |
| 6 | 规格没有写 token 政策 | 新增 NFR："不转售、不加价 token；费用如实显示" | 地基、信念五 |
| 7 | FR-05、NFR-02 规定了可恢复，没有规定可移除 | 新增 NFR："卸载 Tower.md 后，仓库仍是标准 git 仓库，`.towermd/` 能被普通编辑器和 Agent 读懂；Tower.md 不修改用户全局的 git 配置和 harness 配置" | 信念二、四、五 |
| 8 | FR-31 默认并发为 4 | Note.md 的研究认为人能盯住的并行上限约 2–3 个。[^note-humans] Tower.md 用代答和关注清单把人移出并行路径，所以 4 不矛盾；但应按"每任务人工中断数""中断等待时间"校准，人工中断偏多时降到 2–3 | 信念一、五 |

## 6. 可外宣的凝练版

> **让 Agent 去做，你只管放行。**
> 塔台不开飞机。Tower.md 让 Claude Code、Codex 们在你的代码仓里并行干活，只在关键处请你放行。目标、架构和验收标准只在你批准处改变；执行状态是仓库里的纯文本，换 Agent、换机器、甚至卸掉 Tower.md，都不会丢。

> **Agents fly. You give clearance.**
> Tower.md lets Claude Code, Codex and friends work your repo in parallel and asks you only when it matters. Goals, architecture and acceptance criteria change only where you approve. Execution state is plain text in your repo — swap the agent, the machine, or Tower.md itself, and nothing is lost.

成对版：

> **Note.md 管你相信什么，Tower.md 管你承诺什么。**
> 一个读 AI 写的，一个放行 AI 做的。都是你本机上的纯文本，都用你已有的 AI。
> **知识归你，行动由你放行；应用可换，Agent 可换，模型可换，只有你的判断不可替代。**

[^note-readme]: [note.md README](../../note.md/README.zh-CN.md)，"五个信念"与"Agent 发现，你逐条确认"两节。
[^note-claude]: [note.md CLAUDE.md](../../note.md/CLAUDE.md)，"五大核心信念"第 3 条与 OKF actor 约定。
[^note-relations]: [关系只在人确认处生长](../../note.md/docs/product-principle-relationships-only-grow-where-confirmed.md)。
[^note-mirror]: [你的批注属于 vault，不属于路径](../../note.md/docs/product-principle-mirror-hosted-marks.md)。
[^note-vault]: [一个 vault，多个 agent，你是编排者](../../note.md/docs/product-principle-one-vault-many-agents-you-orchestrate.md)。
[^note-humans]: [当 agent 在跑，人该做什么](../../note.md/docs/2026-07-15-what-humans-do-while-agents-run.md)，第 3 节"并行不是越多越好"与第 10 节"对 note.md 的产品指引"。
[^spec]: [Tower.md MVP 规格 v0.3](specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md)；文中 G、FR、NFR 编号与节号均指该规格。
