---
name: towermd-claim
description: 在 Tower.md 项目开始执行任务，读取委托卡、澄清待澄清的任务、绑定工作树并取得模块租约。用于澄清、领取、恢复会话、检查点续租与回应汇报请求，不分配批准权限。
---

# 领取与检查点

1. 在项目中调用 `towermd status`，先处理信封 `delivered[]` 里的回答或插话，再阅读任务状态。用 `towermd task show <task-id>` 核对任务、批准基线、模块、允许路径和验收。
2. 读 `task show` 返回的 `delegation`。`card.brief` 是人写的委托卡：`why`（目的）、`done_when`（完成标准）、`limits`（边界，不许碰的地方）、`checkpoints`（检查点）、`self_check`（交付前自检）。`rules` 是人给这类工作记下的规则，`sent_back` 是此前退回的理由，最后一条是上次退回的原因。按卡片做事，规则和上次退回的理由都要照办。`card.phase` 为 `clarifying` 时先按下面的“先澄清”处理，不要领取。
3. Agent 先准备 `t/<task-id>` 分支及其目标工作树，再进入该工作树。位置由用户或当前会话决定；不要改已有运行的绑定，也不要替其他会话搬工作树。
4. 调用 `towermd task claim <task-id> --harness <name> --harness-version <version> --vendor <vendor> --model <model> --skills-version <version>`。这些是自报的受托者配置，不是可信署名；信任账本按它分开计数，任何一项变化都会让账本对新配置重新起算。只填能核实的项，`--vendor`、`--model`、`--skills-version` 须与 `--harness` 同用；无法核实就省略，不能编造。
5. 读取返回的运行 ID 和任务卡。只有 claim 成功后才依任务卡执行；模块已占用、基线缺失、待处理提问或租约过期都不允许绕过。结果未知时按错误中给出的 `--request-id` 原值及原参数重试，不能换 ID 抢占。
6. 每个检查点调用 `towermd status`，同时续租、接收插话；长操作期间也定期检查，保持小于 15 分钟的租约 TTL。若从其他目录操作已绑定运行，显式使用 `towermd --run <run-id> status`，不要默选另一任务。
7. 停止或放弃时调用 `towermd task release`；完成时使用 [towermd-finish](../towermd-finish/SKILL.md)。释放再领取不能消除未回答的问题。

## 先澄清

人选了“先澄清”的任务处于 `clarifying`，还不能领取。在项目中写一份临时 JSON：`restated` 用自己的话复述目的；`done_when` 起草可核对的完成标准；`questions` 列出真正影响做法的问题，最多 3 个。然后调用：

```sh
towermd dev clarify <task-id> clarify.json --harness <name> --harness-version <version>
```

自报配置参数同领取，只填能核实的项。提交后结束本回合，等人在 App 中回答并定稿；不要领取、不要先动手、不要猜答案。人定稿后任务变为可领取，再从第 1 步开始，以定稿的委托卡为准。

## 汇报请求

委托卡上带时间的检查点会变成汇报请求，以 `{"message": {"kind": "request_report", …}, "due": …}` 出现在每次调用的 `delivered[]` 里，直到回应。用下面的命令回应，`--state` 取 `not_started`、`in_progress`、`waiting`、`blocked`、`claimed_done` 之一，`--summary` 不超过 100 字：

```sh
towermd reply <request-id> --state in_progress --summary "解析已完成，接着写测试" --next "补测试"
```

汇报不算进度；`claimed_done` 也只是自述。过期未回应的，守护进程会记一次失联。没写时间的检查点是停止点：做到这一步先停下，用 [towermd-ask](../towermd-ask/SKILL.md) 请人看过再继续。

## 需复核基线

收到“需复核基线”（`needs_review`）时，旧任务不能原地恢复。先停止修改并释放旧运行，再读取当前已批准基线，按新模块、路径上限和验收重新准备任务 JSON，使用**新的 Task ID** 经 `towermd task add` 创建；随后建立对应的 `t/<新 Task ID>` 分支并重新领取。旧任务及其运行、证据保留作追溯，不修改或清除旧 `needs_review` 标记，也不沿用旧运行的租约或验证证据。新任务必须显式填写依赖：仍需完成的旧依赖也须重新规划为新任务，并引用对应的新 ID；不能删掉未完成的依赖以绕过 G0。尚未解决的问题与禁止仍须先处理，不能利用新 Task ID 绕过。

只在 `allowed_paths` 内修改代码。需要人回答用 [towermd-ask](../towermd-ask/SKILL.md)，需要改慢层或扩大范围用 [towermd-propose](../towermd-propose/SKILL.md)。快层事件、租约、证据与快照均经守护进程；不要自己写事件或宣称完成。
