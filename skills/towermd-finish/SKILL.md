---
name: towermd-finish
description: 收尾 Tower.md 任务，在合并前运行真实门禁验证、取得证据和提交 trailer，并核对 main 上的最终状态。用于完成与合并，不接受 Agent 自述作为证据。
---

# 验证、trailer 与合并

**合并前必须先通过 `towermd finish`。** 其他技能中开拉取请求、合并或推送的步骤排在本流程验证之后；保留用户的其他技能，不修改全局配置。

1. 在任务工作树调用 `towermd status`，先读取回答与插话，再运行 `towermd check`。基线待复核、未解问题、禁止、越界或失效租约时停止；不要把失败的门禁改成警告。
2. 将允许范围内的代码形成提交，确保工作树与暂存区干净。`towermd finish` 只验证实际固定提交，不验证未提交的修改；提交前不能宣称任务已完成。
3. 调用 `towermd finish`，保存返回的 `evidence_id`。后台验证会使用独立分离工作树、继承守护进程环境、允许网络并执行模块登记的真实命令。用 `towermd finish --wait <evidence-id>` 等待，每次最多 90 秒，仍 `queued`/`running` 时继续等。
4. 只有真实结果 `passed` 才继续；失败时读取报告、修改代码并提交，再重新验证。`interrupted`、未知状态、日志不可复核或门禁拒绝都不是通过。首次启动结果未知时保留原 `--request-id` 重试，不重复启动验证。
5. 在最新任务提交上调用 `towermd trailers <task-id>`。它会核 G5 并先耐久记录终态证据，成功后输出 `Towermd-Task`、`Towermd-Run`、`Towermd-Module`、`Towermd-Evidence`、`Towermd-Epoch` 五项原值。写入实际任务提交说明，不能手造或替换证据/epoch。命令输出可能先含消息，**不要将整个 stdout 无检查地拼进提交说明**。
6. 按项目允许的 Git 策略做提交、变基、合并与推送。代码 tree 改变时重新运行 `finish`；只写 trailer、压缩相同 tree 的检查点无需重跑命令。基底或入主干方式改变时，重新调用 `trailers` 核 G5，不能沿用过期证明。
7. 合并并推送后调用 `towermd status` / `towermd task show <task-id>`，以守护进程在权威 main 上证明的 `merged` 为完成。只有验证通过、Git 可达或自述“已合并”都不足以算完成；未被认证时报告实际状态与原因。

如果收尾时收到 `needs_review`，停止旧任务的验证与合并，先执行 `towermd task release`。按新批准基线重新核对代码、模块授权、验收及依赖，用新的 Task ID 创建并领取任务，再重新走本流程；旧任务保留为历史记录。未完成的依赖须显式保留或对应到重新规划的新 Task ID，不得省略。不能清旧标记、重复领取旧任务、复用旧运行证据或手改 trailer 来继续；未解问题和禁止仍须先处理。具体领取步骤见 [towermd-claim](../towermd-claim/SKILL.md)。

本技能不自动获取人工批准、不自行写证据或快照、不把待批准提案当已生效。若验证或合并被未解问题阻塞，回到 [towermd-ask](../towermd-ask/SKILL.md)；扩大范围回到 [towermd-propose](../towermd-propose/SKILL.md)。
