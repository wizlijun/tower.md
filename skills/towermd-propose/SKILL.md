---
name: towermd-propose
description: Tower.md 任务需要修改 L0–L2 慢层或超出允许路径时，提交基于真实综合草稿的变更提案，交由 App 人工审阅和批准。
---

# 提出范围变更

1. 停止依赖越界或慢层变化的实现，保留允许范围内的工作。不要直接编辑用户文档对象、`tower.canvas`、`.towermd/` 或快照替代提案流程。
2. 用 [towermd-baseline](../towermd-baseline/SKILL.md) 生成并提交当前权威 main 的真实综合草稿，保存 `dev draft submit` 返回的 `d-…` 草稿 ID。已有已批准基线时，扫描用 `towermd dev scan start --from-baseline`，沿用已批准的模块与命令；人也可以在 App 里从基线直接生成草稿并审阅。提交前用 `towermd dev draft compare <draft-id>` 核对与当前基线的差异，`outdated` 为 true 时先重新从基线进入草稿；按差异选择 `kind`。若已有人在 App 审阅了同一真实草稿，沿用其 ID。来源过期时先重新扫描和审阅。
3. 写一份临时 JSON 输入，字段如下：
   - `draft_id`：上述真实草稿 ID。
   - `kind`：`module`、`split`、`merge`、`interface`、`acceptance`、`scope` 或 `other`。
   - `reason`：为什么需要这项变更。
   - `alternatives`：方案文字数组。
   - `recommendation`：建议方案及理由。
   - `source_proposal_id`：可选，仅在补全已有问答等捕获草案时填其真实 `P-…` ID。
   - `sources`：可选，这项变更依据的 Note.md 知识，数组，每项 `{"okf_id": "kb/raft", "block": "b-1a2b3c"}`。`okf_id` 是 Note.md 概念路径去掉 `.md`；`block` 可选，取自 Note.md 块引用 `((kb/raft.md#b-1a2b3c))`；另可加 `path`（`.md` 路径提示）和 `title`。只引用，不要把知识原文抄进提案。批准后它们进入出版的决策的 `sources`。
   - `impact_notes`：可选，按受影响对象 ID 各写一句影响说明，如 `{"M-a": "调用方改走新接口"}`。ID 只能取自 `towermd dev draft compare` 返回的 `impact` 中的意图、模块、接口和任务。项目定型后（`growth.mode: settled`，或已批准模块数达到 `growth.settle_at_modules`），每个受影响对象都必须有说明，否则返回 `gate_failed`。
4. 调用 `towermd propose <input.json>`；从其他目录操作已有运行时可加 `--run <run-id>`。不要提供自报的 `impact` 或文件内容；影响对象、实际差异和哈希由守护进程从原草稿与 Git 计算。只写这份临时输入，不写受保护对象。
5. 读取返回的真实提案 ID、`status`、`event` 和 `proposal`。提交成功仅表示提案已记录；人在 App 查看差异、影响范围，明确批准或驳回。批准后的基线必须真实出版，不能把会话回答、草稿 ID 或批准请求当成已生效基线。提案若删减完成标准、验证命令或放宽门禁与阈值，守护进程会把它标为承诺弱化并提醒人；在 `reason` 里写明为什么必须放宽。
6. 结果未知时保留错误中给出的 `--request-id <UUIDv7>`，用相同 JSON 重试，不更换 ID 或重造提案。来源或输入被明确拒绝时，解决实际问题后再提交新请求。
7. 新基线生效后重新核对任务授权和待复核状态，再继续开发。需要人澄清用 [towermd-ask](../towermd-ask/SKILL.md)；提案待批准时结束受阻回合。

CLI 不提供批准命令。临时回答、释放再领取、关闭门禁或手改 frontmatter 都不能代替慢层批准。
