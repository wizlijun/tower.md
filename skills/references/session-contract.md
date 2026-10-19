# 共享会话契约

六项技能执行前先读本文件。技能编排 CLI，唯一状态由 daemon → hub / writer 保存；不要直接写用户文档目录、快照或 `.towermd/`。`init` 生成离线配置是专门例外。普通任务代码只在任务卡授权路径内编辑。

## 先确定项目与运行

使用用户给出的 App 内置 CLI 绝对路径设定 `TOWERMD_CLI`，不要靠同名 PATH 猜版本。先核 `--version` / `--help`。使用实际 harness 版本作为 `--harness`、`--harness-version` 参数；这是自报声明，不是可信身份。CLI 事件署名固定 `process:towermd-cli`，人的批准只能来自 App；未知费用为 `null`。

先读 Task 卡片，建立自己的 `t/<task-id>` 分支及目标工作树，再在目标树领取。绑定是本机、canonical 工作树与分支；全局 `--run` 不能绕过绑定。不要 checkout/reset 用户 main、stash 用户修改、迁移活动 Run，或改全局 Git/harness 配置。已有目标路径或分支则核对归属，不能覆盖。

## 检查点与停车

读取任务卡、计划完成、每次测试红绿、提交前后、长操作前后及集成前，调用以下命令，先处理输出中的消息再继续：

<!-- towermd-cli:status -->
```sh
"$TOWERMD_CLI" status
```

项目命令可能续租，并非无副作用采集器。普通活动 TTL 为 15 分钟；不要连续超过 15 分钟无检查点，长操作应分段。`finish` 的后台验证由 daemon 维护期限。收到暂停、租约丢失、旧 epoch、绑定不匹配或恢复待确认，立即停止该 Run 写入，保存当前证据和恢复指引。不要自行发明 pause/resume 命令；未实现的操作需在 App 或后续版本接通。

问答每次等待不超过 90 秒，累计 10 分钟由 daemon 限定。返回 `deferred` 后结束当前回合，报告 hold/run/task 和下次先读 status 的指引，不后台无限轮询。授权超时不批准；15 分钟无活动释放后重新领取，旧 epoch 的迟答只作历史。

## 人工放行与慢层

L0–L2、Module.verify、允许路径等结构不能由 Agent 直接改写。需要变更时转提案；若 CLI 尚缺提案入口，停止相关动作并说明缺口，不能手写对象绕过。高风险计划 G1 与提交树 G4 必须由人在 App 对准确绑定作决定。普通问答、转述、Agent 自审及用户笼统授权都不是产品内批准事实。计划、tree、基线、租约 fences 改变后旧批准失效。

## 验证与完成

先 `check`，再 `finish`。dirty、旧 main、越界、慢层、缺批准或 `no_tests` 都是硬阻塞。`no_tests` 需先经 App 批准 Module.verify 并补真实测试；禁止使用 `true`、空测试或任务分支自造命令代替权威配置。

`finish` 在分离工作树运行权威模块验证；Agent 报告 done 不会完成任务。初次调用返回 Evidence/job ID，后续沿同一 ID 有界等待。queued/running/failed/no_tests 均不能合并。首次 verifying 起的 60 分钟 deadline 固定，轮询、重启、amend、纯快照变基都不重置。到期后重新领取和验证，不复用旧 fences/Evidence。

验证通过后读取真实五项 trailer：Towermd-Task、Towermd-Run、Towermd-Module、Towermd-Evidence、Towermd-Epoch；不猜值、不把 touches fences 缩成一个 epoch。tree 改变重新验证；只改提交消息且最终 tree 相同可保留原证据。其他技能的 PR、merge、push 收尾排在验证与必要人工评审通过之后，只按任务卡明确授权的方式集成，不 force push 或改写 main。main 观察认证前只能称验证通过；Task=merged、Run=ended 和真实 main 证据齐备才算完成。
