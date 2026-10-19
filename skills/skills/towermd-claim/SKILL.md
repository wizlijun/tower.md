---
name: towermd-claim
description: 领取已有 Tower.md 任务，先建立任务分支和工作树，再绑定 Run，并在开发检查点读取状态。
---

先读取[共享会话契约](../../references/session-contract.md)。

在项目目录先读取状态和目标任务；`TASK_ID` 来自真实任务卡。

<!-- towermd-cli:status -->
```sh
"$TOWERMD_CLI" status
```
<!-- towermd-cli:task-show -->
```sh
"$TOWERMD_CLI" task show "$TASK_ID"
```

核对模块、touches、依赖、allowed_paths、Acceptance、权威 verify 和风险等级。不可领取时停止，不能通过自改 Module/Task 卡放宽范围。

先按用户授权在仓库外的明确目标路径创建任务树，例如 `git worktree add -b "t/$TASK_ID" "$TASK_TREE" main`（有 origin 时先核权威 main，不凭本地旧 main 开始）。路径/分支若已存在先核归属，不覆盖、不 reset。切到该树后重新 status / task show；用户自己的 main/index 保持不动。

从实际 harness 的版本输出设置 `HARNESS` 与 `HARNESS_VERSION`，然后在任务树领取：

<!-- towermd-cli:task-claim -->
```sh
"$TOWERMD_CLI" task claim "$TASK_ID" --harness "$HARNESS" --harness-version "$HARNESS_VERSION"
```

记录真实 Run、主模块和完整 touches fences。拒绝、租约被占或旧 epoch 则停止写入。根据任务卡执行；需要高风险计划批准先使用 towermd-ask 请求 G1。每个检查点先 status，未经验证不得报告任务完成。明确放弃自己的活动运行时可释放，保留代码不擅自删除工作树：

<!-- towermd-cli:task-release -->
```sh
"$TOWERMD_CLI" task release
```
