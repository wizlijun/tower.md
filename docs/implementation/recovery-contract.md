---
type: Design
title: v0.1 当前实现的事件与恢复证明契约
---

本文记录用户清理旧实现后重新接通主干时使用的最小契约。它不代表功能已完成，完成状态以测试、实际产物和验收记录为准。

业务事件生产者由 daemon 单一入口处理。先把完整 `NewEvent` 保存为 pending 并同步文件与目录，再交给 Hub 定序。首次追加返回完整 Event；幂等回执只有 seq 时，必须从 Hub 或已核归档找到原完整 Event，逐项核对 eid、host、actor、type、layer、obj、data、guards 和 seq。完整原件同步保存为 confirmed 后才能向调用方返回本次成功；pending 本身不构成成功证明。过期回执仍保存，但不能认证当前操作。

快照仅包含按 type 分组的业务投影、连续水位、epoch 水位、心跳省略区间及恢复跳号边界。原始非心跳 Event 在月度 JSONL 中保存，eid 索引从归档推导，不写进 state.json。统一 reader 检查事件、区间和跳号精确覆盖连续前缀，并用同一 reducer 重放核对投影。新的 Git 提交实际落入 main 或私有快照引用后才 ack。

Hub 丢失或损坏时拒绝普通写入，由 App 确认恢复。恢复使用尚未开放服务的候选 Hub，按最大已知 seq/epoch 跳过至少 2^32，取得新的主干租约；先将新恢复边界写入 Git，再安装/开放正式 Hub。安装失败后下一次恢复必须从 Git 中的新边界继续，不能复用旧区间。

已核 confirmed 但尚未进入快照的业务事件，作为内部 `recovery.confirmed` 容器中的 `data.original` 完整 Event 随新归档保存。容器的 actor 固定为 `process:towermd-recovery`，不能通过普通业务或 CLI 接口创建；不嵌套容器，不恢复 lease 事件。唯一 reader 读取原 Event，重复原 eid 必须逐字段相同，仅第一次参与业务重放；原 stale 仍然无效。容器只提供新的归档序号，原事件仍是业务证明，不产生第二份 outbox confirmed，也不恢复旧执行权限。二次恢复可以完全依赖 Git 中的原归档。

基线批准绑定冻结 `Publication`：请求 ID、草稿 ID、项目、来源提交、目标基线号、预览时间、全部输出文件与内容摘要。唯一出版证明器检查已接受的 `human:local` 批准完整 Event、真实提交父节点、文件逐字节内容及基线标签。裸 tag、投影中的“已批准”字段或 Agent 自报不构成证明。

任务完成证明还必须绑定真实结构化 verifier 报告、tree、完整 fences、路径门禁和实际 main 接入边。恢复读取历史证明不重新运行命令、不获取运行租约、不授予新的批准权；对应 DTO 和检查器随真实任务闭环落地补充，不用样例状态代替。
