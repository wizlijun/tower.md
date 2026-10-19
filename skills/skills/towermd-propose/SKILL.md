---
name: towermd-propose
description: 任务需要改变慢层、模块边界或授权范围时，提交源绑定的私有变更提案，等待人在 App 审阅。
---

先读取[共享会话契约](../../references/session-contract.md)。

停止依赖这项变更的执行；整理实际问题、精确差异、受影响对象/路径、备选及建议。提案引用已有的私有来源提交或已保存草稿，不直接改权威慢层、快照或 `.towermd/`。已有权限内且不依赖该变更的工作可继续。

在工作树外保存 ProposalInput JSON：`source` 为 `{"kind":"branch","revision":"<完整源提交>","base_revision":"<权威基线提交>"}` 或 `{"kind":"draft","id":"<已保存草稿ID>","revision":"<精确草稿提交>"}`；填写非空 `reason`、`recommendation`，至少一个含 `name`/`consequences` 的 `alternatives`，以及完整的 `affected_intents`、`affected_modules`、`affected_interfaces`、`affected_tasks`、`affected_paths` 数组。使用真实来源和精确影响，不能猜 ID 或扩大 Task 授权。

<!-- towermd-cli:propose -->
```sh
"$TOWERMD_CLI" propose "$PROPOSAL_JSON"
```

记录返回的原请求 ID、私有草稿 revision/content hash 和当前有效性。不确定时保留原 JSON、Run 选择和原 UUID，只重试同一请求：

<!-- towermd-cli:propose-retry -->
```sh
"$TOWERMD_CLI" propose "$PROPOSAL_JSON" --request-id "$REQUEST_ID"
```

提交成功仅表示保存私有提案，不是批准或新基线。把同一项目、提案草稿 ID、精确 revision/content hash、目标 B1（后续为 B<n+1>）及影响交给人在 App 审阅；若草稿变化而不再匹配原提案的精确版本，先停车并明确前次结果，再以当前真实来源提交新的提案；不能换原请求正文或沿用旧 hash 的批准。由人在 App 明确批准后，daemon 才能出版新基线；保存、拒绝、历史回执或结果不确定时，依赖该变更的执行继续停车。不要调用 baseline submit 冒充 Bn，不使用内部 HTTP 或批准类命令。

人在同一项目画布点“重新读取态势”，从真实提案关注项打开审阅，不需要搬运草稿 ID 作为正常入口。若审阅提示来源、目标或版本已变化，请人“重新加载审阅”后核对当前完整原件；不能对旧页面自动重放决定。人工批准记录与出版结果分开：结果 pending、blocked 或不确定时仍停车，已有原批准则使用“重读原批准与出版状态”协调原 UUID，不另建批准或重复提案。

出版后，请人在 App 打开实际受影响 Task 的“Task 需复核”原件，核对完整 Task 文本、新基线、全部 publication 原因，以及当时 Run/完整 fences 与到期时间（ready Task 可以没有 Run），明确解除本次标记。此复核只解除该绑定的需复核标记，不授予其他批准、不启动 harness，也不扩大任务卡授权。

回到原任务树先重新 status，核对真实新基线、需复核状态、原 Run 与完整有效租约；按任务卡授权同步最新权威 main。原 Run 仍有效且门禁满足才继续；已到期、释放或重建失效则停止旧 Run，按 claim 技能重新领取。新基线和复核回执均不能恢复旧 epoch、旧 G1/G4 或旧 Evidence；计划/tree/基线/fences 改变后重新请求所需评审和验证。
