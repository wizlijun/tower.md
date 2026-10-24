---
name: towermd-claim
description: 在 Tower.md 项目开始执行任务，读取委托卡、澄清待澄清的任务、绑定工作树并取得模块租约。用于澄清、领取、恢复会话、检查点续租、回应汇报请求与把范围外的事记成想法，不分配批准权限。
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

汇报不算进度；`claimed_done` 也只是自述。过期未回应的，Tower.md 会记一次失联。没写时间的检查点是停止点：做到这一步先停下，用 [towermd-ask](../towermd-ask/SKILL.md) 请人看过再继续。

人也会在 App 中对某个节点提问或要求汇报，同样以 `request_report` 送达，`envelope` 带上目标对象。节点提问的文字里带 `--ref <依据>`，回答时必须写明依据（文件、提交、文档，可重复）；答不了就如实说明，不要猜：

```sh
towermd reply <request-id> --state in_progress --summary "拆成两层是为了让解析器不依赖 IO" --ref src/parser.rs --ref docs/specs/parser.md
```

运行之外回答时用 `--from <你的名称>/<版本>` 写明回答者；只有收件人能回答。节点提问只发给声明过能回答该节点的人：确实熟悉某个节点时，汇报时加 `--answers` 声明（`towermd report --target <节点> --state in_progress --summary <摘要> --answers`），不再能答时用 `--no-answers` 撤回。不要为了收到提问而随意声明。

同一个下一步反复汇报、同一检查反复失败、短时间内频繁提问，或一次托付拖得远超同类，Tower.md 会判为兜圈子并请人注意。遇到这种情况，停下来换个思路或用 [towermd-ask](../towermd-ask/SKILL.md) 请人决定，不要原地重复。

## 插话

人在 App 中的插话以 `{"message": {"kind": "prompt", "id": "P-…"}, "text": …}` 出现在 `delivered[]` 里，只送一次。照办；办完后确认，人在 App 中会看到“已处理”：

```sh
towermd handled P-0190d2c3-0000-7000-8000-000000000001 --note "已回退依赖版本改动"
```

`--note` 一行写明做了什么，不超过 200 字。没办到就不要确认，用 `report` 如实汇报或用 [towermd-ask](../towermd-ask/SKILL.md) 提问。确认已处理不算进度，也不是批准。

## 主动汇报与上报

不必等汇报请求：工作中有进展或卡住时，用 `report` 主动汇报；发现值得人注意的事（指标越过阈值、外部依赖出错），用 `signal` 上报。在运行中二者默认指向当前任务、发送者是领取时自报的配置：

```sh
towermd report --state blocked --summary "等 API 密钥" --next "拿到密钥后补集成测试" --progress 3/7 --unit files
towermd signal eng.crap --level passive --title "parser.rs 复杂度偏高" --value 42 --threshold 30 --ref src/parser.rs
```

- `--state` 同上，没有 `done`；`claimed_done` 只是自述，不让任务完成。
- 种类带命名空间（如 `eng.crap`），`core.` 留给 Tower.md。级别：`log` 只进时间线，`passive` 计入角标，`active` 进入人的关注清单、等人下次查看，`time_sensitive` 排在最前并立即发系统通知打断人。默认用 `passive`，只有确实需要人现在看时才用 `active` 或 `time_sensitive`；问题本身要人回答时用 [towermd-ask](../towermd-ask/SKILL.md)。
- 标题不超过 30 字，摘要和说明不超过 100 字；超长会被拒绝（`too_long`），请改短再发，不要拆成多条。
- 同一种类对同一对象重复上报会合并计数；情况缓解后用 `--level passive` 再发一次即可撤出关注清单。

## 顺手想到的事

工作中想到任务范围之外、值得以后做的事（新功能、重构、另一个缺陷），记成想法，不要自己建任务：

```sh
towermd idea add "周报导出也该支持 CSV"
```

- 一句话，不换行，不超过 200 字；超长会被拒绝（`too_long`）。
- 想法只是线索：不进人的关注清单，不能被领取，也不会派给任何受托者。是否立项、立成项目还是任务，只由人在 App 中决定；命令行没有立项命令。
- 不要因为想到了就去做，也不要用 `towermd task add` 把想法直接变成任务；当前任务仍只做委托卡上的事。眼下就挡路的问题用 [towermd-ask](../towermd-ask/SKILL.md)，值得人立刻注意的情况用 `signal`。

## 需按新版本复核

收到“需按新版本复核”（`needs_review`）时，旧任务不能原地恢复。先停止修改并释放旧运行，再读取当前已批准基线，按新模块、路径上限和验收重新准备任务 JSON，使用**新的 Task ID** 经 `towermd task add` 创建；随后建立对应的 `t/<新 Task ID>` 分支并重新领取。旧任务及其运行、证据保留作追溯，不修改或清除旧 `needs_review` 标记，也不沿用旧运行的租约或验证证据。新任务必须显式填写依赖：仍需完成的旧依赖也须重新规划为新任务，并引用对应的新 ID；不能删掉未完成的依赖以绕过 G0。尚未解决的问题与禁止仍须先处理，不能利用新 Task ID 绕过。

只在 `allowed_paths` 内修改代码。需要人回答用 [towermd-ask](../towermd-ask/SKILL.md)，需要改慢层或扩大范围用 [towermd-propose](../towermd-propose/SKILL.md)。快层事件、租约、证据与快照均经守护进程；不要自己写事件或宣称完成。
