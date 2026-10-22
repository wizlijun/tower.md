---
name: towermd-claim
description: 在 Tower.md 项目开始执行任务，读取任务卡、绑定工作树并取得模块租约。用于领取、恢复会话与检查点续租，不分配批准权限。
---

# 领取与检查点

1. 在项目中调用 `towermd status`，先处理信封 `delivered[]` 里的回答或插话，再阅读任务状态。用 `towermd task show <task-id>` 核对任务、批准基线、模块、允许路径和验收。
2. Agent 先准备 `t/<task-id>` 分支及其目标工作树，再进入该工作树。位置由用户或当前会话决定；不要改已有运行的绑定，也不要替其他会话搬工作树。
3. 调用 `towermd task claim <task-id>`。若能核实当前 harness 名称和版本，可同时传 `--harness <name> --harness-version <version>`；无法核实就省略，不能编造。这是自报信息，不是可信署名。
4. 读取返回的运行 ID 和任务卡。只有 claim 成功后才依任务卡执行；模块已占用、基线缺失、待处理提问或租约过期都不允许绕过。结果未知时按错误中给出的 `--request-id` 原值及原参数重试，不能换 ID 抢占。
5. 每个检查点调用 `towermd status`，同时续租、接收插话；长操作期间也定期检查，保持小于 15 分钟的租约 TTL。若从其他目录操作已绑定运行，显式使用 `towermd --run <run-id> status`，不要默选另一任务。
6. 停止或放弃时调用 `towermd task release`；完成时使用 [towermd-finish](../towermd-finish/SKILL.md)。释放再领取不能消除未回答的问题。

收到“需复核基线”（`needs_review`）时，旧任务不能原地恢复。先停止修改并释放旧运行，再读取当前已批准基线，按新模块、路径上限和验收重新准备任务 JSON，使用**新的 Task ID** 经 `towermd task add` 创建；随后建立对应的 `t/<新 Task ID>` 分支并重新领取。旧任务及其运行、证据保留作追溯，不修改或清除旧 `needs_review` 标记，也不沿用旧运行的租约或验证证据。新任务必须显式填写依赖：仍需完成的旧依赖也须重新规划为新任务，并引用对应的新 ID；不能删掉未完成的依赖以绕过 G0。尚未解决的问题与禁止仍须先处理，不能利用新 Task ID 绕过。

只在 `allowed_paths` 内修改代码。需要人回答用 [towermd-ask](../towermd-ask/SKILL.md)，需要改慢层或扩大范围用 [towermd-propose](../towermd-propose/SKILL.md)。快层事件、租约、证据与快照均经守护进程；不要自己写事件或宣称完成。
