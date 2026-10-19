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

提交成功仅表示保存私有提案，不是批准或新基线。当前后续基线出版尚未开放：明确报告此缺口，相关执行继续停车。不要调用 baseline submit 冒充 Bn，不使用内部 HTTP 或批准类命令；历史回执不恢复旧 Run 的资格。
