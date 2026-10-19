---
name: towermd-ask
description: 任务中需要人的回答或高风险 G1/G4 评审时发出请求，按 Hold 状态有界等待，不代替人批准。
---

先读取[共享会话契约](../../references/session-contract.md)。

只在当前任务的绑定工作树提问。先整理最小必要上下文和清楚选项，推荐索引从 0 开始；以下变量均取自当前真实问题。

<!-- towermd-cli:ask -->
```sh
"$TOWERMD_CLI" ask "$QUESTION" --option "$OPTION_A" --option "$OPTION_B" --recommended 0 --context "$CONTEXT"
```

初次提问会最多等待 90 秒；保存 Hold ID 与原请求。仍 pending 时沿同一 Hold 等待：

<!-- towermd-cli:ask-wait -->
```sh
"$TOWERMD_CLI" ask --wait "$HOLD_ID"
```

只有当前有效运行的实际回答可用于继续；回答不是慢层批准。deferred、租约失效、旧 epoch 或恢复未确认则按共享契约停车。用户在会话里说了答案也不能因此记为 App 的 human 批准。

高风险计划先把真实计划存到工作树外 UTF-8 文件（`PLAN_FILE`），请求 G1：

<!-- towermd-cli:review-g1 -->
```sh
"$TOWERMD_CLI" review request --gate G1 --content "$PLAN_FILE"
```

G4 直接绑定当前已提交 tree，不接受计划文件：

<!-- towermd-cli:review-g4 -->
```sh
"$TOWERMD_CLI" review request --gate G4
```

保存返回的 Hold ID，请用户在 App 核对项目、计划/tree、Run、baseline 与 fences 后明确决定。当前 CLI 没有 review wait 入口，不发明参数；在正常 status 检查点观察真实评审状态，未明确通过不继续受闸门保护的动作，不能以超时或自己的 review 代替放行。重试必须用原 request_id、原输入，不新建多个评审绕过旧结果。
