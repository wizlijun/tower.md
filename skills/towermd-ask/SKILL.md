---
name: towermd-ask
description: 将当前 Tower.md 运行中必须由人决定的问题送到 App，按返回状态等待或挂起。用于缺少决定或需要人工澄清，不代替正式评审或慢层批准。
---

# 需要人决定时

在已领取任务的工作树运行：

```sh
towermd ask "需要决定的问题" --option "方案 A" --option "方案 B" --recommended 0 --context src/example.rs
```

按实际问题填写文字与仓库相对路径，选项可省略；`--recommended` 是零起始下标，只是建议。已知影响对象时用重复的 `--target <object-id>`，由守护进程查真实层级。不要把慢层目标隐去以取得普通回答。暂不能继续当前回合时可用 `--parked`，保留问题并立即挂起。

- 返回 `answered`：读取真实回答及其来源。只按该回答的有效范围继续，不把推荐项当答案。已有模块决策只按同模块、归一化后的完全相同问题复用。
- 返回 `pending`：保存 Hold ID，再调用 `towermd wait <hold-id>` 继续等（仍未回答时退出码为 5）。每次最长 85 秒（可用 `--seconds` 缩短），从原问题开始累计最多 10 分钟；重试不能重开问题来重置期限。
- 返回 `deferred`（退出码同为 5，但没有 `next`）：结束当前任务回合，说明等待的 Hold ID。不要再调用 `towermd wait` 忙等，也不要猜答案或继续依赖该决定的工作；回答会出现在之后 `towermd` 调用的 `delivered[]` 里。若带 `proposal`，表示仍待慢层批准，回答未生效。
- 结果未知（`error.retry.safe` 为真）：沿用原 `--request-id` 和完整原输入重试，不生成另一份问题。

回答或插话可能出现在任何 `towermd` 业务命令信封的 `delivered[]` 里。先处理这些消息，再读命令结果；CLI 会在 stdout 实际成功输出后确认送达。不要自行写 `prompt.delivered`，也不要把读取队列当已送达。

若人确实在当前 harness 会话中直接回答了已记录的问题，可代为记录原回答：

```sh
towermd answer <hold-id> "人的实际回答" --relayed --harness <名称> --harness-version <版本>
```

`--relayed` 表示这是你转述的人的回答；不带它，命令以 `needs_human`（退出码 7）拒绝。只转述人实际给出的内容，不猜答案、不把推荐选项或 Agent 自述当成人的回答。该命令只解开所属运行的当前普通问题，署名仍是 `process:towermd-cli`，并标注 `relayed_from: human`；harness 名称、版本仅为自报信息。不会记为决策或规则，涉及慢层的问题拒绝转述放行；需要保存决策或处理 L0–L2 时请人在 App 中确认。结果未知时沿用 `--request-id` 和完整原输入重试。

涉及 L0–L2 的 App 回答只能成为待批准提案，不能直接实施。`towermd review request`、正式 G1/G4/G6 与临时流程规则属于后续 M2，当前技能不调用这些未提供的入口。
