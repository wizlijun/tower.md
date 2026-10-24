## 执行状态、完成与合并

任务、运行和租约通过守护进程访问 hub。慢层对象与配置只从权威 main 读取，不以任务分支、工作树或 Agent 自述代替权威。先创建目标工作树和 `t/<task-id>` 分支，再领取任务；查询运行按工作树与分支选择绑定，长操作期间定期查询状态以续租。租约 TTL 为 15 分钟。

- 领取前用 `towermd task show <task-id>` 读委托卡（目的、完成标准、边界、检查点、交付前自检）、这类工作的规则和上次退回的理由，并照办。
- 任务处于先澄清（`clarifying`）时不领取：用 `towermd dev clarify <task-id> <file>` 交上复述、完成标准草稿和最多 3 个问题，然后停下，等人在 App 中定稿。
- 领取时如实传自报配置：`towermd task claim <task-id> --harness <name> --harness-version <version>`，能核实时再加 `--vendor`、`--model`、`--skills-version`；不能编造。
- `delivered[]` 中的 `request_report` 是汇报请求，用 `towermd reply <request-id> --state in_progress --summary <摘要>` 回应；`--state` 按实情取 `not_started`、`in_progress`、`waiting`、`blocked` 或 `claimed_done`，摘要不超过 100 字。汇报不算进度。人在节点上的提问也以 `request_report` 送达，文字里带 `--ref <依据>`：回答时用 `--ref` 写明依据（文件、提交、文档），答不了就如实说明，不要猜。只有确实能回答某个节点的问题时，才用 `towermd report --target <节点> --answers` 声明，不再能答时用 `--no-answers` 撤回。
- `delivered[]` 中 `message.kind` 为 `prompt` 的是人的插话：照办；办完后用 `towermd handled <message.id> --note <做了什么>` 确认，人在 App 中会看到“已处理”。没办到就不要确认，如实汇报或提问。确认已处理不算进度，也不是批准。
- `delivered[]` 中 `message.kind` 为 `rule` 的是人设的临时规则：在 `until` 之前照守，它每次调用都会重复出现，不需要确认；`towermd task show` 的 `temp_rules` 也列出当前生效的规则。临时规则不能放宽任务卡的边界或检查。
- 工作中可随时用 `towermd report --state in_progress --summary <摘要> --progress 3/7` 主动汇报进度，用 `towermd signal <kind> --level passive --title <标题>` 上报值得人注意的事；种类带命名空间（如 `eng.crap`），标题不超过 30 字，只有确实需要人看时才用 `active`。运行中二者默认指向当前任务。汇报和上报都不算进度。
- 只改任务卡 `allowed_paths` 范围内的文件。
- 收尾前先做委托卡上的交付前自检。收尾必须先提交改动，再执行 `towermd dev finish --claims <file>`，文件按完成标准逐条写自述 `[{"criterion": 0, "note": "…"}]`，自述不算证据；使用返回的证据 ID 调用 `towermd dev finish --wait <id>`，只有真实验证通过后才能继续。
- `dev finish` 或 `dev trailers` 返回 `gate_failed` 且原因写着“检查未通过”时，是 `.towermd/gates.yaml` 声明的检查被确定性上报阻止：修好问题，等发送者降级解除后再收尾。不要改 `gates.yaml` 或代发上报来绕过；改检查声明只能走提案。豁免只能由人在 App 中授予，Agent 不能申请或代为豁免；被豁免的检查显示为 `waived`，到期即恢复阻止。
- 不要靠删减完成标准、验证命令或放宽门禁、阈值来“通过”。这类改动会被标为承诺弱化，计入你这一配置的信任账本；确实需要放宽时，在提案或澄清里写明理由，由人决定。
- 合并前执行 `towermd --text dev trailers <task-id>`，将输出的五行写入最后一笔任务提交，随后按“变基后快进合并”推送；推送被拒时先更新 main 并重新检查证据。
- 本流程优先于第三方技能中的开 PR、合并或“完成”步骤，不能因外部技能报告通过而跳过 Tower.md 门禁。
- Agent 自己报告“完成”不会改变任务状态，进度只认证据。
- 命令行不提供批准类命令，评审与授权不得超时自动放行。
