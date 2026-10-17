---
type: Plan
title: 画布编辑 MVP 规格纠偏与实施边界
status: draft
sources:
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: docs/superpowers/plans/2026-10-17-m1-baseline-approval.md
  - resource: docs/superpowers/plans/2026-10-17-m1-app-workflows.md
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: 2026-10-18 用户要求人和 Agent 可编辑卡片与关系，并纠正未按 spec 开发 MVP
---

# 画布编辑 MVP 规格纠偏

本计划记录纠偏，不改写主规格的完成标准，不把原型或未提交实验算作 M0/M1 已完成。用户要求快速看到原型，不等于授权另建一个与工程驾驶舱分离的便签产品。

## 已确认偏差及处理

| 依据 | 本轮错误路线 | 纠正 |
| --- | --- | --- |
| FR-83、100–102：同一 tower.canvas、四种节点、真实对象与接口绑定 | 新建只允许 text 的独立自由画布，丢弃既有 file/link/group 编辑语义 | 编辑真实候选/基线中的同一画布；支持孤立节点和任意关系，不要求树形，但保留对象、接口、引用语义 |
| FR-95、P4 Task2：refs/towermd/drafts/<id>、FrozenDraft | 单独 refs/towermd/canvas-draft 和第二套元数据 | 使用既有 DraftBundle/FrozenDraft/ReviewBinding 与标准草稿引用 |
| P4 Task2：共享 writer/锁、事件与恢复 | 另写独立 Git 提交实现，只完成保存重开 | 接入 SnapshotWriter 与 repository_writer；耐久保存和事实记录走既有流程 |
| FR-43–48：慢层编辑进入草稿，批准后生效 | 将独立图保存当作完整编辑闭环 | 当前基线只读；草稿叠加显示；审阅绑定精确内容，批准后由持租约 writer 出版 |
| §5.4、FR-04：v0.1 不做新安全框架，CLI 可先启动 daemon | 加入 UDS/peer PID/App 路径认证，并限制启动顺序 | 撤销此安全扩展，沿用既定 App 客户端边界，不增加 App 必须先启动的前置 |
| §18 M1 验收 | 把能打开、保存卡片称作 MVP 完整 | 仍按技能、基线、人工闸门、证据与真实使用验收；画布增量通过不代表 M1 完成 |

偏离路线的23个自有修改文件已逐一校验归档到本机临时目录，tracked 接线恢复到 89ea925。未提交、未打包、未发布；不删除用户既有未跟踪文件。编辑器手势和测试仅可作为未来同规格实现的复用材料，不直接合入。

## 选取的垂直闭环

优先落实 J1：真实仓库扫描 → Agent 按同一 schema 提交候选 → App 显示真实模块/接口/引用 → S7 编辑并持久化 → S8 审阅、拒绝或批准 B0。再经同一提案路径处理后续变更。目录预览和演示图不能代替候选基线，fixture 不能代替真人验收。

MVP 的工程对象仍是意图、模块、接口、验收、任务、证据与决策。卡片和关系是这些对象及注释的画布表达。快层任务状态、健康色和进度只读，不为方便编辑增加人工直接改完成状态的路径。

用户新增的自由表达需求并入 FR-100 的同一画布：孤立 text 卡片、link、group、无对象注释关系可存在；关系可非树形。不得因此清除 file 节点、towermd.obj 绑定、未知扩展或不透明条目。

## 实施顺序及验收

- [ ] 先核准已批准 M0/P2/P3/P4/P6 的实际源码前置，标明已落地与缺失接口，不跳过前置另建替代存储。
- [ ] 完成真实扫描/候选草稿生产与读取链；每个模块节点解析到真实对象，接口带 ID 与 label，来源与验证状态明确。
- [ ] 沿 P4 Task1–2 实现唯一 schema、DraftBundle/FrozenDraft、八类既定 DraftEdit 与共用 writer；保存到 refs/towermd/drafts/<id>，expected revision 冲突不得覆盖。
- [ ] 将用户新增的有限注释节点/注释关系操作补入同一 typed 编辑契约及测试。Agent 沿 baseline submit/propose 参与候选修改，CLI 固定 process 署名，不能经 Agent 路径产生人工批准。
- [ ] 接 P6 的既定 native 动作与 Svelte Flow 画布：当前基线只读，草稿可编辑；保存重开保持坐标/对象/未知字段；快层更新不移节点。
- [ ] 接 P4 审阅/批准/拒绝与主干出版：拒绝不生效，内容改变后旧批准失效；批准后才出现对应 main 提交、Decision、基线标签与真实 baseline.created。main 检出时按规格 pending，不用私有快照激活 B0。
- [ ] 用真实 App 验证编辑、重开、Agent 提交/提案、拒绝与批准；再完成 §18 的 M1 技能与任务证据验收。始终分别报告自动测试、实机操作、真人批准和正式发布状态。

本文件不将八类 DraftEdit、P3 请求/回执或发布保护改成新的简化协议。下一次实现从已批准计划的准确接口继续；未经规格映射的功能不进入当前主线。
