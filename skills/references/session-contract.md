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

项目命令的正文前可带逐行 `{"towermd_message": ...}` 消息；先读取全部前导消息再读取命令正文。稳定消息 ID 用于去重；“已送达”只表示完整输出，不表示 Agent 已处理。输出失败不确认送达，下次可能重复同一 ID。

正文可能是 JSON，也可能是 trailer 等文本；不要把含消息前导的完整 stdout 直接当成单一 JSON，也不要因正文失败或非零退出码丢弃已经输出的消息。分别保留完整 stdout、stderr 和退出码；只按原稳定 ID 去重，不凭文字生成替代 ID，不换 Run 或工作树来重复消费。

项目命令可能续租，并非无副作用采集器。普通活动 TTL 为 15 分钟；不要连续超过 15 分钟无检查点，长操作应分段。`finish` 的后台验证由 daemon 维护期限。收到暂停、租约丢失、旧 epoch、绑定不匹配或恢复待确认，立即停止该 Run 写入，保存当前证据和恢复指引。

仅在明确授权暂停整个项目时，先生成并保存本次动作的 UUIDv7 为 `PAUSE_REQUEST_ID`，再请求：

<!-- towermd-cli:pause-all -->
```sh
"$TOWERMD_CLI" pause --all --request-id "$PAUSE_REQUEST_ID"
```

这以 `process:towermd-cli` 暂停新 claim，并向当时冻结的实际 Run 请求在检查点停止；不会 kill 外部 harness。普通停车先停止自己的写入，不擅自扩大为全项目暂停。

仅在明确授权请求恢复整个项目时，为这个不同动作生成并保存独立 UUIDv7 `RESUME_REQUEST_ID`：

<!-- towermd-cli:resume-all -->
```sh
"$TOWERMD_CLI" resume --all --request-id "$RESUME_REQUEST_ID"
```

`resume` 只提交 process 恢复请求，项目仍暂停；请人在 App 核对当前 pause revision、请求和完整 Run 范围后明确确认。不能通过 CLI、会话回答或 `--approve`/`--confirm` 等自造参数代替。范围改变则让人重新核对。人确认后仍先 `status` 核对原 Run、完整租约和门禁，不自动恢复 harness、重新领租约或复活旧批准；重建也不会隐式撤销暂停。

两项命令 uncertain 时保留原请求 ID、动作、目录和运行上下文，只重试同一原请求；不能把暂停 UUID 用于恢复、换 UUID 绕过未知结果，或自行调用内部 HTTP。

问答每次等待不超过 90 秒，累计 10 分钟由 daemon 限定。返回 `deferred` 后结束当前回合，报告 hold/run/task 和下次先读 status 的指引，不后台无限轮询。授权超时不批准；15 分钟无活动释放后重新领取，旧 epoch 的迟答只作历史。

## 人工放行与慢层

L0–L2、Module.verify、允许路径等结构不能由 Agent 直接改写。需要变更时使用现有源绑定提案入口；提交私有提案不等于批准或新基线。由人在 App 核对提案草稿的精确 ID/revision/hash/目标基线后明确批准，daemon 才能出版后续基线。受影响 Task 由人在 App 对完整任务文本、publication 集合和当时 Run/fences 作精确复核；复核前依赖该变更的执行停车。回到原任务树先重新 status，核新基线、需复核状态和完整有效租约；旧 Run 失效则按 claim 技能重新领取，不能凭新基线或历史回执复活旧授权。不得手写对象或调用内部接口绕过。高风险计划 G1 与提交树 G4 必须由人在 App 对准确绑定作决定。普通问答、转述、Agent 自审及用户笼统授权都不是产品内批准事实。计划、tree、基线、租约 fences 改变后旧批准失效。

## 验证与完成

先 `check`，再 `finish`。dirty、旧 main、越界、慢层、缺批准或 `no_tests` 都是硬阻塞。`no_tests` 需先经 App 批准 Module.verify 并补真实测试；禁止使用 `true`、空测试或任务分支自造命令代替权威配置。

`finish` 在分离工作树运行权威模块验证；Agent 报告 done 不会完成任务。初次调用返回 Evidence/job ID，后续沿同一 ID 有界等待。queued/running/failed/no_tests 均不能合并。首次 verifying 起的 60 分钟 deadline 固定，轮询、重启、amend、纯快照变基都不重置。到期后重新领取和验证，不复用旧 fences/Evidence。

验证通过后读取真实五项 trailer：Towermd-Task、Towermd-Run、Towermd-Module、Towermd-Evidence、Towermd-Epoch；不猜值、不把 touches fences 缩成一个 epoch。tree 改变重新验证；只改提交消息且最终 tree 相同可保留原证据。其他技能的 PR、merge、push 收尾排在验证与必要人工评审通过之后，只按任务卡明确授权的方式集成，不 force push 或改写 main。main 观察认证前只能称验证通过；Task=merged、Run=ended 和真实 main 证据齐备才算完成。
