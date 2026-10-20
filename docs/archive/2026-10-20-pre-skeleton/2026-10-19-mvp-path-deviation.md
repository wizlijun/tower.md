---
type: Report
title: MVP 主路径与实际开发路径偏离图
status: draft
sources:
  - resource: specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: superpowers/plans/2026-10-18-canvas-mvp-spec-alignment.md
  - resource: ../src/App.svelte
  - resource: ../src/components/TaskProgressPanel.svelte
  - resource: ../crates/towermd-core/src/init.rs
  - resource: ../crates/towermd-core/src/baseline_review.rs
---

# MVP 主路径与实际开发路径偏离图

对照原始 v0.5（`906d462`）的 M1 与已交付 App（`fafc27e`）。左侧是应闭合的用户流程，右侧是实际实施顺序；连线表示流程或对应关系，不表示工时、代码量或完成比例。

```mermaid
flowchart LR
  subgraph SPEC["原 spec 主路径 · M1 用户闭环"]
    direction TB
    S1["① 项目接入与技能安装<br/>Claude Code / Codex 使用同一流程"]
    S2["② 扫描 → 草稿审阅 → 人批准 B0<br/>得到真实模块、接口、验收与画布"]
    S3["③ Agent 领取并执行任务<br/>经 CLI 提问、提案、提交证据"]
    S4["④ 同一画布实时显示态势<br/>Z0 模块 → Z1 任务 → Z2 运行时间线<br/>首屏最多 3 条真实关注项"]
    S5["⑤ 人直接回答或放行<br/>验证 → 认证合并 → 状态回到画布<br/>批准变更 → 新基线 → 任务需复核"]
    S6["⑥ 整体真实验收后发布<br/>两种 harness · 3 个工作树 · 10 个任务"]
    S1 --> S2 --> S3 --> S4 --> S5 --> S6
  end

  subgraph ACTUAL["实际路径 · 部件不断增加，主流程未闭合"]
    direction TB
    A1["必要基础：Hub、快照、租约、恢复<br/>已有实现，可以复用"]
    A2["用户催原型后，先做演示图与 Git 目录图<br/>能看图，不能据此看真实执行态势"]
    A3["【走偏】独立卡片存储、第二套草稿、额外鉴权<br/>另建了偏离既有契约的路径"]
    A4["【返工】撤回错误路线，重接统一草稿与 B0<br/>纠偏记录记载 23 个修改文件归档撤回<br/>B0 审阅出版、卡片与关系编辑已有真实实现"]
    A5["继续深化任务门禁、finish、trailers、到期恢复<br/>验证与合并证据有价值<br/>但未同步闭合用户可见主流程"]
    A6["堆叠独立入口：进度、问答、评审、草稿<br/>任务列表手动刷新，问答需搬运 Hold ID<br/>主画布手写渲染，编辑与审阅另接 Svelte Flow"]
    A7["多轮构建和交付预览包<br/>当前 0.0.0：目录 / 基线查看与多个功能表单<br/>尚未成为原 spec 的工程驾驶舱"]
    A8["【当前断点】<br/>无可安装技能；缺实时快层、Z1/Z2、真实关注清单<br/>插话、公开提案到后续基线、整体真实验收未完成"]
    A1 --> A2
    A2 -->|错误分支| A3
    A3 -->|撤回并返工| A4
    A4 --> A5 --> A6 --> A7 --> A8
  end

  A1 -.->|提供基础，未交付技能入口| S1
  A4 -.->|局部主路径已有实现| S2
  A5 -.->|任务与证据后端已有实现| S3
  A8 -.->|下一步优先接通，不能用手动列表替代| S4

  subgraph FINDING["偏离判断与纠偏边界"]
    direction TB
    F1["确定返工：独立画布存储与额外鉴权路线<br/>重复投入：多条画布渲染与分散面板的整合债<br/>顺序失当：主流程未闭合，持续深化边界和重复打包"]
    F2["纠偏目标：真实任务 → 自动看见状态或问题<br/>→ 下钻 → 直接放行 → 看见验证与合并结果<br/>沿现有 Hub、writer、草稿与证据实现接通"]
    F3["统计边界：没有可靠工时数据，不编造浪费百分比<br/>不把全部基础与测试称为废代码<br/>不把 M2 多机、一句话生长或 M3 无头调度算成 M1 欠账"]
    F1 --> F2 --> F3
  end
  A8 --> F1
```

依据：[M1 原定验收](specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md#18-里程碑与验收)、[已记录的错误路线与撤回](superpowers/plans/2026-10-18-canvas-mvp-spec-alignment.md#已确认偏差及处理)、[App 主接线](../src/App.svelte)、[手动任务列表](../src/components/TaskProgressPanel.svelte)、[技能未交付提示](../crates/towermd-core/src/init.rs)、[后续基线硬阻](../crates/towermd-core/src/baseline_review.rs)。本图是偏离复核，不是产品整改完成证明。
