---
name: towermd-finish
description: 收尾 Tower.md 任务，先做交付前自检，在合并前运行真实门禁验证、附上逐条自述、取得证据和提交 trailer，并核对 main 上的最终状态。用于完成与合并，不接受 Agent 自述作为证据。
---

# 验证、trailer 与合并

**合并前必须先通过 `towermd dev finish`。** 其他技能中开拉取请求、合并或推送的步骤排在本流程验证之后；保留用户的其他技能，不修改全局配置。

1. 在任务工作树调用 `towermd status`，先读取回答与插话。基线待复核、未解问题、禁止或失效租约时停止；不要把失败的门禁改成警告。
2. 先按 `towermd task show` 中 `delegation.card.brief` 的 `self_check`（交付前自检）自己核对一遍，没过就继续改。然后将允许范围内的代码形成提交，确保工作树与暂存区干净，再运行 `towermd dev check` 检查固定提交的路径与准入条件。`dev check` 与 `dev finish` 都拒绝未提交的修改；不要先对脏工作树调用 `dev check` 再提交，也不能在提交后就宣称任务已完成。路径越界时停止并按提案流程处理。
3. 写一份临时 JSON，按 `delegation.card.brief.done_when` 的顺序（从 0 起）逐条写一句自述，说明在哪里、怎样满足：`[{"criterion": 0, "note": "…"}]`。没做到的条目不要写成已满足。
4. 调用 `towermd dev finish --claims claims.json`，保存返回的 `evidence_id`。自述在验收卡上标为“受托者自述”，不算证据，不会让任务完成；没有检查或人亲自用过的条目，人看到的是“未证实”。重试同一请求须带相同的自述。后台验证会使用独立分离工作树、继承守护进程环境、允许网络并执行模块登记的真实命令。用 `towermd wait <evidence-id>`（与 `towermd dev finish --wait <evidence-id>` 相同）等待，每次最多 85 秒，仍 `queued`/`running` 时退出码为 5，继续等；验证失败或中断时退出码为 6（`gate_failed`），`data` 里有原因。
5. 只有真实结果 `passed` 才继续；失败时读取报告、修改代码并提交，再带 `--claims` 重新验证。`interrupted`、未知状态、日志不可复核或门禁拒绝都不是通过。首次启动结果未知时保留原 `--request-id` 重试，不重复启动验证。`dev finish` 或 `dev trailers` 直接返回 `gate_failed`、原因以“检查未通过”开头时，是 `.towermd/gates.yaml`（只读权威 main）声明的检查被确定性上报阻止，可在 `towermd task show` 的 `delegation.card.completion` 看到哪一项及原因。修好问题，等发送者以较低级别解除后再收尾；不要修改 `gates.yaml`、不要代发降级上报来绕过，改检查声明只能走 [towermd-propose](../towermd-propose/SKILL.md)。豁免只能由人在 App 中授予，不要请求或暗示豁免来代替修复；`completion` 中状态为 `waived` 的检查在到期时间前放行，到期后恢复阻止，收尾要赶在到期前完成或先修好。
6. 在最新任务提交上调用 `towermd --text dev trailers <task-id>`。它会核 G5 并先耐久记录终态证据，成功后 stdout 只输出 `Towermd-Task`、`Towermd-Run`、`Towermd-Module`、`Towermd-Evidence`、`Towermd-Epoch` 五项原值，可直接写入实际任务提交说明；消息与下一步提示走 stderr。不能手造或替换证据/epoch。
7. 按项目允许的 Git 策略做提交、变基、合并与推送。代码 tree 改变时重新运行 `dev finish`；只写 trailer、压缩相同 tree 的检查点无需重跑命令。基底或入主干方式改变时，重新调用 `dev trailers` 核 G5，不能沿用过期证明。
8. 合并并推送后调用 `towermd status` / `towermd task show <task-id>`，以守护进程在权威 main 上证明的 `merged` 为完成。只有验证通过、Git 可达或自述“已合并”都不足以算完成；未被认证时报告实际状态与原因。

人可以在 App 中退回已完成的任务。退回后任务重新可领取，`task show` 的 `delegation.sent_back` 写着理由，人也可能把这次纠正记为规则（`delegation.rules`）。按 [towermd-claim](../towermd-claim/SKILL.md) 重新领取，先照理由和规则改，再走本流程。

如果收尾时收到 `needs_review`，停止旧任务的验证与合并，先执行 `towermd task release`。按新批准基线重新核对代码、模块授权、验收及依赖，用新的 Task ID 创建并领取任务，再重新走本流程；旧任务保留为历史记录。未完成的依赖须显式保留或对应到重新规划的新 Task ID，不得省略。不能清旧标记、重复领取旧任务、复用旧运行证据或手改 trailer 来继续；未解问题和禁止仍须先处理。具体领取步骤见 [towermd-claim](../towermd-claim/SKILL.md)。

本技能不自动获取人工批准、不自行写证据或快照、不把待批准提案当已生效。若验证或合并被未解问题阻塞，回到 [towermd-ask](../towermd-ask/SKILL.md)；扩大范围回到 [towermd-propose](../towermd-propose/SKILL.md)。
