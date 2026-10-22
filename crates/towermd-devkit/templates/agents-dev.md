## 执行状态、完成与合并

任务、运行和租约通过守护进程访问 hub。慢层对象与配置只从权威 main 读取，不以任务分支、工作树或 Agent 自述代替权威。先创建目标工作树和 `t/<task-id>` 分支，再领取任务；查询运行按工作树与分支选择绑定，长操作期间定期查询状态以续租。租约 TTL 为 15 分钟。

- 领取前用 `towermd task show <task-id>` 读委托卡（目的、完成标准、边界、检查点、交付前自检）、这类工作的规则和上次退回的理由，并照办。
- 任务处于先澄清（`clarifying`）时不领取：用 `towermd dev clarify <task-id> <file>` 交上复述、完成标准草稿和最多 3 个问题，然后停下，等人在 App 中定稿。
- 领取时如实传自报配置：`towermd task claim <task-id> --harness <name> --harness-version <version>`，能核实时再加 `--vendor`、`--model`、`--skills-version`；不能编造。
- `delivered[]` 中的 `request_report` 是汇报请求，用 `towermd reply <request-id> --state in_progress --summary <摘要>` 回应；`--state` 按实情取 `not_started`、`in_progress`、`waiting`、`blocked` 或 `claimed_done`，摘要不超过 100 字。汇报不算进度。
- 只改任务卡 `allowed_paths` 范围内的文件。
- 收尾前先做委托卡上的交付前自检。收尾必须先提交改动，再执行 `towermd dev finish --claims <file>`，文件按完成标准逐条写自述 `[{"criterion": 0, "note": "…"}]`，自述不算证据；使用返回的证据 ID 调用 `towermd dev finish --wait <id>`，只有真实验证通过后才能继续。
- 合并前执行 `towermd --text dev trailers <task-id>`，将输出的五行写入最后一笔任务提交，随后按“变基后快进合并”推送；推送被拒时先更新 main 并重新检查证据。
- 本流程优先于第三方技能中的开 PR、合并或“完成”步骤，不能因外部技能报告通过而跳过 Tower.md 门禁。
- Agent 自己报告“完成”不会改变任务状态，进度只认证据。
- 命令行不提供批准类命令，评审与授权不得超时自动放行。
