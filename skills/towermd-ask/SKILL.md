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
- 返回 `pending`：保存 Hold ID，再调用 `towermd ask --wait <hold-id>`（兼容命令为 `towermd wait <hold-id>`） 继续等。每次最长 90 秒，从原问题开始累计最多 10 分钟；重试不能重开问题来重置期限。
- 返回 `deferred`：结束当前任务回合，说明等待的 Hold ID。不要忙等、猜答案或继续依赖该决定的工作。若带 `proposal`，表示仍待慢层批准，回答未生效。
- 结果未知：沿用原 `--request-id` 和完整原输入重试，不生成另一份问题。

回答或插话可能在任何 `towermd` 业务命令输出最前面出现。先处理这些消息，再读命令结果；CLI 会在 stdout 实际成功输出后确认送达。不要自行写 `prompt.delivered`，也不要把读取队列当已送达。

人只在 App 回答。涉及 L0–L2 的回答只能成为待批准提案，不能直接实施。`towermd review request`、正式 G1/G4/G6 与临时流程规则属于后续 M2，当前技能不调用这些未提供的入口。
