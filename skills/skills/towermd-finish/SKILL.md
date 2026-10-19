---
name: towermd-finish
description: 任务集成前执行路径守卫和后台真实验证，读取五项 trailer，并等 main 证据认证完成。
---

先读取[共享会话契约](../../references/session-contract.md)。

在原绑定任务树检查状态，确认已提交且工作树无未跟踪或未提交变更。不要为了变干净删除用户文件。

<!-- towermd-cli:check -->
```sh
"$TOWERMD_CLI" check
```

守卫失败先保留具体原因；越界/慢层转提案，高风险缺批准转 App，no_tests 不能用空命令绕过。通过后启动一次真实后台验证：

<!-- towermd-cli:finish -->
```sh
"$TOWERMD_CLI" finish
```

可选的 `REPORT_FILE` 是工作树外真实 UTF-8 报告，只是线索：

<!-- towermd-cli:finish-report -->
```sh
"$TOWERMD_CLI" finish --report "$REPORT_FILE"
```

以上两种启动方式择一，不重复执行。保存 Evidence ID，沿同一 ID 等待，每次最多 90 秒：

<!-- towermd-cli:finish-wait -->
```sh
"$TOWERMD_CLI" finish --wait "$EVIDENCE_ID"
```

同时核退出码、job_state 与实际 result。只有 completed 且 passed 才能进入集成；queued/running/failed/no_tests/uncertain/interrupted/expired 均不能合并。固定 60 分钟期限已到则先读 status，重新领取和验证，旧结果不复活。用户暂停或不确定清理时停车。

高风险任务在 App 完成对应提交树的 G4；随后读取真实五项 trailer：

<!-- towermd-cli:trailers -->
```sh
"$TOWERMD_CLI" trailers "$TASK_ID"
```

将原输出附在将实际进入 main 的提交末尾，不凭记忆拼接。Task、Run、Module、Evidence、Epoch 必须齐全。tree 变化要重新验证；其他 PR/merge/push 技能排在这些门禁通过之后，按用户明确授权的集成方式操作。最后 status 核实际 main 的合并认证：只有 Task=merged、Run=ended 并有 main 提交/Evidence 才报告完成；仅验证通过就明确“待集成／待认证”。返回 task、run、Evidence、tree、main commit 和未决项。
