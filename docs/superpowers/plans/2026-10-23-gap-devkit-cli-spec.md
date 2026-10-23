# Tower.md 开发套件规格与 CLI 规格：实现差距分析

- 基准：`/Users/bruce/git/tower.md`，分支 `feat/delegation-trust-ledger`，HEAD `795a5ce`，工作区干净。本次只读，没有改动仓库。
- 规格：
  - `docs/specs/2026-10-21-spec-towermd-development-kit.md`（下称"套件"，路线图见 §18）
  - `docs/specs/2026-10-21-spec-towermd-cli.md`（下称"CLI"，阶段 C0–C4）
- 判定口径：只有类型定义或文档提到的，不算实现；要有可运行的代码路径，最好还有测试。下文的代码路径默认相对 `crates/` 下对应的 crate。
- 只在核心规格里实现的条目（套件规格中只是引用）不在本文计数，列在第六节。

## 一、按里程碑计数

### 开发套件（共 105 项；按细项计，例如每个适配器、每个 MCP 工具、每个生长信号各算一项）

| 里程碑 | 已实现 | 部分实现 | 未实现 | 不适用 | 合计 |
|---|---|---|---|---|---|
| M0/M1（已完成） | 25 | 12 | 0 | 0 | 37 |
| M2 | 0 | 2 | 19 | 0 | 21 |
| M3 | 0 | 0 | 37 | 0 | 37 |
| 未分配（Arca、FR-08、FR-34、FR-55、FR-57、门禁失败中断、事件表） | 0 | 3 | 7 | 0 | 10 |

M0/M1 的 12 项"部分实现"如下。它们不影响 M1 验收，但都是真实缺口：

- FR-13：没有按任务回滚的入口
- FR-20：置信度固定为 0.5；虚线按"是否候选"而非"置信度低"显示
- FR-21：画布上没有"未划分区"组
- 8.2：性能未测
- FR-104：缺 seed 技能和 review 技能
- FR-105：init 不提示如何安装技能
- 10.2 状态机：没有 Failed 状态，任务从不进入 Held
- 10.4 任务卡：内容不全
- G2：没有敏感文件和危险命令规则
- 12.1 禁止中断：没有豁免
- 15.5：App 不能重派任务
- 16.2：缺少若干 API 路由

### CLI 规格（按细项计，与第五节逐行表一致）

| 阶段 | 已实现 | 部分实现 | 未实现 | 不适用 | 合计 |
|---|---|---|---|---|---|
| C0 | 31 | 9 | 3 | 0 | 43 |
| C1 | 0 | 1 | 14 | 1 | 16 |
| C2 | 1 | 0 | 3 | 0 | 4 |
| C3 | 0 | 3 | 16 | 1 | 20 |
| C4 | 0 | 0 | 6 | 0 | 6 |
| 套件 M2（`dev seed`） | 0 | 0 | 1 | 0 | 1 |
| 套件 M3（`dev run`/`queue`/`adapters`） | 0 | 0 | 1 | 0 | 1 |
| 未分配（§3.7 离线） | 0 | 0 | 1 | 0 | 1 |

## 二、工作包（M2、M3、CLI 缺口）

列说明：
- 大小：S 约 1–2 天，M 约 3–5 天，L 1 周以上。
- 新依赖：按 AGENTS.md 技术选型表判断。
- 外部工具：指运行时需要的外部工具。
- 不变量：指会碰到的 AGENTS.md 产品不变量。

### M2

| 包 | 需求 | 大小 | 依赖 | 新依赖 | 外部工具 | 不变量 |
|---|---|---|---|---|---|---|
| WP-M2-A 导入解析与聚类 | 8.1-S2、8.1-S3、FR-20（置信度随证据变化、按置信度显示虚线）、FR-21（`region: unassigned` 组节点）、8.2 覆盖率复检 | M | 无 | 否。手写逐行解析与 Louvain，规格要求不引入解析器 | 无 | 无（派生数据，可重建） |
| WP-M2-B 从基线进入草稿（J4） | M2 草稿模式：从当前已批准基线生成草稿、对比视图、批准后发布新基线 | M | 无 | 否 | 无 | 慢层保护：草稿只能经人批准的提案发布，沿用现有 publication 路径 |
| WP-M2-C 评审门禁 G1/G4 | G1、G4、FR-58（`risk: high`）、12.1 评审中断、FR-55 最小 `gates.yaml`、CLI `ask --review`、FR-104 评审技能 | M | 无 | 否 | 无 | 不自动放行：评审不得超时通过；进度只认证据 |
| WP-M2-D `core.verify` 内置检查 | FR-121；不配检查时只能由人确认完成 | S–M | 宜在 WP-M2-C 之后 | 否 | 无 | 进度只认证据；人的确认只能从 App 发出。与 todo.md:556 的现有偏差相反，要先裁决（见歧义 D4） |
| WP-M2-E 一句话生长（种子） | 9.1 种子、澄清、骨架、批准与 `git init`；`towermd-seed` 技能；CLI `dev seed`；API `POST /seed`；FR-22 | M–L | WP-M2-B（草稿与发布）；WP-M2-C 可选 | 否 | 未初始化目录需要 `git`（已有） | 慢层保护：B0 必须经人批准；"一处定序"：`git init` 由守护进程做 |
| WP-M2-F 生长信号与两种模式 | 9.2 生长期与定型期，9.3 的五个信号（同类错误反复依赖 M3 错误分类，可先做前四个） | M | WP-M2-E；WP-M2-A（未划分区） | 否 | 无 | 不打扰：信号只进关注清单，并要有阈值 |
| WP-M2-G 状态机与中断补完 | 10.2 的 Failed 和进入 Held；"验证 3 次未过"；G0 缺信息时生成提问中断；12.1 门禁失败中断的选项；FR-57 自动修复计数；12.1 禁止中断的豁免（带到期时间） | M | 无（豁免依赖歧义 D3） | 否 | 无 | 豁免必须带到期时间；不自动放行 |
| WP-M2-H App 操作与 API 补齐 | 15.5 App 重派与释放；16.2 的 `GET /modules/:id`、`GET /tasks/:id`、`GET /holds`、`tasks/:id/{pause,release,report}`；10.4 完整任务卡；FR-105 init 提示安装 | M | 无 | 否 | 无 | 人机署名：重派是人的动作，只能从 App 发出 |

### M3

| 包 | 需求 | 大小 | 依赖 | 新依赖 | 外部工具 | 不变量 |
|---|---|---|---|---|---|---|
| WP-M3-A 适配器与运行器 | FR-25（四个适配器）、FR-26、FR-27、FR-29、FR-06(M3)、FR-08 原始输出、10.4 的 `runs/<run>/task.md`、NFR-07(M3)、15.5(M3) | L | 无 | 需要 tokio 的 `process` 和 `io-util` feature（选型表已列，不是新 crate）；SIGINT/SIGTERM 可能要 `libc` 或 `nix`，需确认 | `claude`、`codex`、`agent` 已装，`gemini` 没装；真实运行要付费 token | harness 细节只在适配器；费用如实（未知记 `null`）；不改用户全局 harness 配置 |
| WP-M3-B 调度器与集成器 | FR-11 串行合并队列、FR-31 并发上限、FR-14 派发前领租约、FR-32 由集成器执行、CLI `dev queue`/`dev run`/`dev adapters`、6.3 的 merge.* 与 run.* 事件 | L | WP-M3-A | 否 | `git` | 一处定序、一个提交者：集成器只能经主干写入模块合并 |
| WP-M3-C MCP 交互网关 | 16.4 的四个工具、FR-60、FR-63、12.1 授权中断（10 分钟默认拒绝） | L | WP-M3-A；评审工具依赖 WP-M2-C | 否（手写 stdio JSON-RPC） | 无 | 授权超时默认拒绝；只接受本机连接 |
| WP-M3-D 总控双循环 | FR-23、FR-24、FR-30 角色映射、FR-65 中断策略、FR-66、FR-67、FR-70 `/retry` | L | WP-M3-A、WP-M3-C | 否 | 需要一个 harness 跑总控 | 慢层保护（总控只写 L3–L4）；署名 `process:`；不自动放行（`auto` 策略不能用于评审） |
| WP-M3-E 技能包锁定 | FR-51 至 FR-54、NFR-09(M3) | M | WP-M3-A | 否（sha2 已有） | 无 | 不改别人的地盘：只组装运行时目录，不写用户全局技能目录 |
| WP-M3-F 运行报告与错误分类 | FR-28（含 v0.1 `dev finish --report`）、14.3、12.1 错误中断、9.3 同类错误反复 | S–M | 报告本身无依赖；错误中断依赖 WP-M3-A | 否 | 无 | 进度只认证据：报告只是自述，不改变状态 |
| WP-M3-G 预算 | 12.1 预算中断 | S | WP-M3-A | 否 | 无 | 费用如实：未知费用不能当 0 计入预算 |
| WP-M3-H 交叉评审 | FR-34 | S–M | WP-M3-A、WP-M2-C | 否 | 另一种模型的 harness | 结果只供参考，不能作为放行依据 |

### 未分配

| 包 | 需求 | 大小 | 依赖 | 新依赖 | 外部工具 | 不变量 |
|---|---|---|---|---|---|---|
| WP-X Arca 大文件 | FR-75 至 FR-78、FR-08 | M | 验证日志可先接；运行输出依赖 WP-M3-A | 否 | `arca` CLI（本机没装，要退回本地忽略目录） | 本地优先；派生物可重建 |

### CLI

| 包 | 需求 | 大小 | 依赖 | 新依赖 | 外部工具 | 不变量 |
|---|---|---|---|---|---|---|
| WP-CLI-0 C0 收尾 | FR-193（`dev draft submit`、`scan cancel`、`init` 加请求 ID）、`task edit`/`task ls`、`task claim` 省略 id、`wait --seconds` 与等验证、`too_long` 错误码与 30 字检查、FR-199 帮助分组、`--project <路径>` | S–M | 无（省略 id 和长度单位要先裁决，见 C13、C12） | 否 | 无 | 无 |
| WP-CLI-0b `completion` | FR-199 的 `completion` | S | 无 | 是，`clap_complete`，需人确认 | 无 | 无 |
| WP-CLI-1 读命令（C1） | FR-198 / 不带参数的 `towermd`、`needs`、`since`、`show <id>`、`ls`、`history`、`open`、`watch`、`decision show/ls`、`rule ls`、`rebuild-index`、`doctor`、`whoami`、§3.9 上次查看 | M | `since` 依赖服务端的"上次查看"状态；不带参数的版本在项目外依赖多项目注册表（见 C7） | 否（`watch` 用已有的 tokio-tungstenite） | `open` 需要 App | 不打扰：首屏最多 3 条 |
| WP-CLI-2 Agent 汇报（C2） | `report`、`signal`、`frame` | M | WP-M3-F 报告结构可复用 | 否 | 无 | 进度只认证据：`report` 不改变状态 |
| WP-CLI-3 人的在场（C3） | FR-190、FR-195、FR-196、§4.3、§4.4；`approve`/`decline`/`allow`/`deny`/`done`/`answer`/`assign`/`promote`/`waive`/`level set`/`rule add`/`send`/`resume`/`pause`；一步完成的 `hub rebuild`；`human setup/status/revoke`；`overview` | L | 必须先改 AGENTS.md 不变量；需要签名构建 | 是，Secure Enclave/LocalAuthentication 或钥匙串 crate | Developer ID 签名；macOS 生物识别 | **直接违反**"人机署名不混"和"命令行不提供批准类命令"，要人先改不变量 |
| WP-CLI-4 扩展（C4） | `follow-up`、`frame new`、`idea`、`ext`、`template` | M | WP-CLI-2 | 未知 | 无 | `idea` 回写 Note.md 时须遵守第二道闸门 |
| WP-CLI-5 离线与远端 hub | §3.7、`hub_offline`、`queued:true`、`hub-worker/` | L | 安全加固（认证与加密） | 是，wrangler 一套（选型表已列） | Cloudflare 账号（可选，由用户自部署） | 本地优先：云端只能可选 |

## 三、会阻塞实现的规格歧义与矛盾

### 开发套件

- **D1 M2 的"草稿模式"范围不清。** S7 里的改名、移动、合并、拆分、接口编辑已经实现。剩下的只有 J4 那部分：从现有基线进入草稿，再和原基线对比。§18 没说清 M2 要做的是哪一部分。
- **D2 待决问题必须在开工前裁决。**
  - #3 豁免默认到期时间：WP-M2-G 需要。
  - #4 定型期模块阈值：WP-M2-F 需要。
  - #7 技能包来源与信任：WP-M3-E 需要。
- **D3 中断和告警混用。**
  - 12.1 要求"门禁失败"和"禁止"是中断，并带"豁免"选项。
  - 实现用的是核心的告警（`task_gate_failed`、`unapproved_protected_change`），任务不进入"中断中"。
  - 规格没有说明这两类是否需要把任务状态改成 Held，也没说明告警和中断怎样统一。
- **D4 FR-121 与现有偏差方向相反。**
  - FR-121 说"不配检查时，只能由人确认完成"。
  - `tasks/todo.md:556` 记录的现行决定是"开发套件的任务不能由人一步确认完成，必须经检查"。
  - 这两句要先统一。
- **D5 状态机的 Failed 状态没有入口定义。** v0.1 没有运行器，`run.finished=failed` 从哪来没说。"失败 3 次"和"验证 3 次未过"是否同一个计数器，也没说。
- **D6 G0 "缺信息"的形式不清。** G0 失败时，规格说要"生成提问中断"，但领取者是 Agent 自己。由谁提问、问谁，规格没写。现在的实现是直接报错。
- **D7 FR-104 的技能表仍用旧命令名。** 表里还有 `review request`、`baseline scan`。§16.1 的旧命令表已作废，但正文没改（todo Q7）。
- **D8 适配器的阶段自相矛盾。** 正文写"v0.1 宜先实现 Claude Code 和 Codex 两个适配器"，但 §18 把适配器放在 M3。
- **D9 M3 验收依赖真实付费运行。** "4 个无头 Agent 并发跑 20 个任务"需要真实的 harness CLI 和 token。规格没有给出用模拟 harness 的验收替代。
- **D10 FR-16 的人工放宽与 CLI C3 冲突。** FR-16 规定只有 App 建的任务可以放宽路径。CLI C3 让人从命令行签名操作后，"人从 CLI 建的任务"能否放宽路径没有定义。
- **D11 G4 在 v0.1 的形式不清。** v0.1 写的是"人在另一个会话或 App 处理"，评审结果怎样绑定提交（证据还是决策）没有写。
- **D12 FR-107 正文过密。** 它把大量契约细节引到已归档的计划里。改动 finish 时，需要回到归档计划核对，规格本身不自足。

### CLI（来自 CLI 审计）

- **C1 FR-195 直接违反 AGENTS.md 不变量。**
  - 不变量写的是"人的动作只能从 App 发出；命令行不提供批准类命令"。
  - 现有测试 `approval_commands_are_rejected` 和 `routes_require_known_client_and_instance_and_cli_has_no_approval` 守着这条。
  - 人改不变量之前，C3 不能做。
- **C2 C3 需要新依赖和签名构建。** Secure Enclave 或钥匙串相关的 crate，选型表写"随安全加固再定"。另外需要 Developer ID 签名，以及 App 显示 6 位码的登记流程。
- **C3 `completion` 需要新依赖 `clap_complete`**（Q3），需人确认。
- **C4 C0 的偏差还没裁决。**
  - §3.5 写 `--json <文件|->`，实现是位置参数（Q1）。
  - 实现多了错误码 `rejected`，规格表里没有（Q2）。
  - `hub rebuild` 规格写一步完成，实现是两步（Q4）。
- **C5 "等待只有一个入口"与保留的 `dev scan wait`、`dev finish --wait` 矛盾。**
- **C6 `ask --review` 的阶段矛盾。** §5 的映射算作 C0，但 G1/G4 在套件 M2。
- **C7 阶段排序矛盾。**
  - 不带参数的 `towermd` 在 C1，在项目外要显示总览，但 `overview` 在 C3。
  - 多项目注册表不存在，待决问题 3 也没定。
  - `--project <路径>` 同理。
- **C8 只有本地 hub 时，§3.7 离线无从触发。** `hub_offline` 和 `queued:true` 没有场景可用，规格没说本地 hub 时这些条款怎样适用。
- **C9 FR-197 没说语言由谁决定。** 守护进程的消息是由 CLI 在本地翻译，还是由请求带上语言交给守护进程，规格没写。
- **C10 FR-190 对等表缺命令名。** 以下 App 专用端点在 §2 命令树里没有对应命令：
  - 委托卡的 finalize、used、send-back
  - work-kinds rename
  - main-resolutions prepare/confirm
  - baselines review/verify
  - tasks/human、tasks/options
- **C11 `pause` 改成人的动作会破坏现有用法。** 改了之后，Agent 调用 `pause` 会得到 `needs_human`，规格没有把这一点列为破坏性变更。
- **C12 长度限制的单位没定。** "30 字 / 100 字"没写按字符还是字节计，也没写哪些字段算"标题"。
- **C13 `task claim` 省略 id 时怎么选没定义。**

## 四、开发套件逐条状态

| id | 摘要 | 里程碑 | 状态 | 证据 | 缺口 |
|---|---|---|---|---|---|
| FR-113 | 按"本机+工作树+分支"定位运行，`--run` 覆盖，找不到时列出活动运行 | M0/M1 | 已实现 | crates/towermd/src/cli/task_service.rs:137,155；测试 question_cli.rs:458（显式 --run 覆盖）、task_cli.rs `task_lifecycle_binds_worktrees_renews_and_serializes_module_ownership` | |
| FR-06(v0.1) | v0.1 工作树由用户/harness 自建，不要求布局 | M0/M1 | 已实现 | claim 在当前工作树绑定（task_model.rs:498 "该工作树已有活动运行"） | |
| FR-06(M3) | 运行器把每个无头运行放进独立工作树 | M3 | 未实现 | rg `runner|adapter|worktree add`（只有 verifier 的分离工作树）无运行器 | |
| FR-08 | 运行原始输出/日志/截图不进 git，交 Arca | 未分配 | 部分实现 | 验证日志写 `$(git-common-dir)/towermd/verify/<id>/logs`（towermd-devkit/src/verifier.rs:370-374），不进 git | 没有 Arca，也没有 run 原始输出（无无头运行） |
| FR-98(套件补充) | init 目录下已有 intents/arch/specs/baselines/state 时警告 | M0/M1 | 已实现 | towermd-core/src/init.rs:224 | |
| FR-75 | Arca 文件由守护进程写到 `<docs_dir>/artifacts/<run>/` | 未分配 | 未实现 | rg -i `arca` 无代码；只有 catalog.rs:144 把 `artifacts/` 排除出对象目录 | |
| FR-76 | 证据记录 Arca 相对路径与内容哈希，写后不可改 | 未分配 | 未实现 | 同上；证据日志只记本机缓存路径与哈希（verifier.rs:603） | |
| FR-77 | 未装 Arca 时退回 git 忽略的本地目录，画布标"仅本机" | 未分配 | 未实现 | rg `仅本机|local only` 无结果 | |
| FR-78 | 启动时跑 `arca doctor`，状态栏显示未同步文件数 | 未分配 | 未实现 | rg `doctor` 无 arca 调用；本机 `which arca` 无 | |
| FR-10 | 变基后快进合并；推送被拒则 fetch/变基/重验 | M0/M1 | 已实现 | G5 由 finish 强制以最新 main 为基（task_finish_tests.rs `state_only_main_advance_requires_actual_rebase_and_new_g5_but_reuses_g3`、`main_code_change_then_revert_still_requires_new_finish`）；合并动作由 Agent 按 agents-dev.md:11 执行 | |
| FR-11 | M3 集成器串行处理合并队列 | M3 | 未实现 | rg `integrator|merge queue|queue` 无组件 | |
| FR-12 | 末个提交带五行 trailer（Task/Run/Module/Evidence/Epoch） | M0/M1 | 已实现 | `dev trailers`（main.rs:246）；main_inspection.rs:65；task_finish_tests.rs `missing_or_duplicate_final_trailers_and_expired_execution_are_rejected` | |
| FR-13 | 回滚只用 git revert，不改写 main | M0/M1 | 部分实现 | 主干写入只追加；越界提交的撤销走 revert（towermd-core/src/main_resolution.rs:148,227；main_resolution_cli.rs `app_revert_preserves_index_and_worktree_and_does_not_repeat_inverse`） | 没有"按任务回滚"的入口，这条更像规则 |
| FR-14 | claim 时发放模块写租约，被占时失败并给其他可领任务 | M0/M1 | 已实现 | task_service.rs:296；task_cli.rs `task_lifecycle_binds_worktrees_renews_and_serializes_module_ownership` | M3 的"调度器派发前领取"未做（见 WP-M3-B） |
| FR-112 | 租约 TTL 15 分钟，任何调用续租；验证中 60 分钟未合并释放 | M0/M1 | 已实现 | task_service.rs:7 `MODULE_TTL`、:186 续租；task_finish.rs:21 `VERIFYING_TTL_MS`；finish_service.rs:394 验证期间续租 | |
| FR-15 | 跨模块任务声明 `touches` 并取全部租约 | M0/M1 | 已实现 | task_model.rs:287-304,475；task_finish.rs:292-298 | |
| FR-16 | diff 只许 `allowed_paths`；CLI 建的任务路径不能超出模块与 touches，只有 App 建的可放宽 | M0/M1 | 已实现 | task_model.rs:341；单测 `cli_paths_need_declared_touches_and_human_widening_is_explicit`（task_model.rs:764）；core tests/path_guard.rs `fixed_diff_enforces_task_scope_without_rejecting_ordinary_markdown` | |
| FR-17 | 受保护路径（慢层、state/、.towermd/、改前后 layer）拒绝并转提案 | M0/M1 | 已实现 | core tests/path_guard.rs `both_old_and_new_layers_default_types_and_invalid_layers_are_protected`、`rename_remains_two_checked_paths_and_all_reserved_directories_are_protected`；转提案：finish_service.rs:190,266 `capture_run_violations` | |
| FR-18 | S6 输出按 JSON Schema，`draft submit` 用同一 schema 校验 | M0/M1 | 已实现 | towermd-devkit/src/baseline.rs:48-54（内置 schema，未知关键字报错）；`towermd schema dev.draft.submit` | 无头 `--json-schema`/`--output-schema` 属 M3 |
| FR-19 | 冷启动期间 harness 只读 | M0/M1 | 已实现 | v0.1 由技能约束（skills/towermd-baseline/SKILL.md；docs/usage.md:66"只读固定提交"） | 无头 `--permission-mode dontAsk`/read-only 沙箱属 M3 |
| FR-20 | 候选模块附置信度与理由；低置信度虚线边框 | M0/M1 | 部分实现 | cold_start.rs:274 置信度固定 0.5、理由有；S6 可覆盖（baseline.rs:350）；app.css:71 所有候选卡都是虚线 | 置信度不随证据变化；虚线按"是否候选"而非"置信度低" |
| FR-21 | 未分配文件集中列出，画布上显示为"未划分区" | M0/M1 | 部分实现 | 面板列出 App.svelte:1336-1338；canvas.rs:181 接受 `region` 键 | 冷启动没有生成 `region: unassigned` 的 group 节点（cold_start.rs:441 `region: None`） |
| 8.2-覆盖率 | 已跟踪源码 ≥95% 归入模块 | M0/M1 | 已实现 | cold_start.rs:29,522 coverage；测试 `empty_source_repository_does_not_claim_complete_coverage` | M2 结合 import 解析的复检未做 |
| 8.2-每层 3–9 | 不足下钻、过多分组，不行给诊断 | M0/M1 | 已实现 | cold_start.rs:171,200,256；测试 `impossible_module_count_is_diagnosed_and_root_commands_are_shared` | |
| 8.2-验证命令 | 每模块至少一条验证命令或标"无测试"并给补测试候选任务 | M0/M1 | 已实现 | `suggested_task`（cold_start.rs:283）；App.svelte:1332 | |
| 8.2-性能 | 20 万行以内仓库 S1–S6 在 15 分钟内 | M0/M1 | 部分实现 | 无性能测试；tasks/todo.md:513 写明 NFR-03 定量性能未验收 | 缺定量测量 |
| FR-104 | 技能包至少含 init/baseline/seed/claim/ask/propose/finish，只调用命令行 | M0/M1 | 部分实现 | skills/ 下有 6 个技能 | 缺 `towermd-seed`（M2）；`towermd-ask` 的评审入口（`ask --review`）未实现（skills/towermd-ask/SKILL.md:31） |
| FR-105 | 技能以 harness 原生目录发布，用户自装；init 只提示安装方式 | M0/M1 | 部分实现 | skills/*/SKILL.md；docs/usage.md:66 | `towermd init` 输出里没有安装提示（init.rs、render.rs 中 rg `skill|技能|install` 无结果） |
| FR-106 | claim 一次完成 G0、租约、task.claimed、运行 ID、打印任务卡；自报身份记 self_reported | M0/M1 | 已实现 | task_service.rs:267-321；main.rs:408 自报字段；task_cli 测试 | 任务卡内容不全，见 10.4 |
| FR-107 | finish：守卫 → 分离工作树验证 → 绑定 tree 的证据；`--wait` ≤90 秒；main 上见 trailer 才记合并 | M0/M1 | 已实现 | finish_service.rs；verifier.rs；task_finish_tests.rs 14 个测试（如 `real_verifier_amend_and_cold_clone_keep_original_tree_proof`）；task_cli.rs `finish_persists_real_report_survives_restart_and_only_main_admission_merges_task` | |
| FR-108 | AGENTS.md 与 finish 技能写明合并前先 finish，第三方技能步骤排在后面 | M0/M1 | 已实现 | towermd-devkit/templates/agents-dev.md:10-12 | |
| FR-23 | 总控只写 L3–L4、次要决策与提案 | M3 | 未实现 | 无总控进程（rg `controller` 只在规格与 actor 名里） | v0.1 部分"先引用已有决策"在核心（FR-62） |
| FR-24 | 总控只在基线与验收范围内代答，超出转人 | M3 | 未实现 | 同上 | |
| 10.2 状态机 | 待定/就绪/运行中/暂停/验证中/中断中/失败/已合并及转换 | M0/M1 | 部分实现 | task_model.rs:100-108（无 `Failed`）；验证中由 dev_state.rs:322 设置；60 分钟释放 dev_state.rs:363-404 | 没有 `Failed` 状态；`Held` 从不被设置（rg `TaskState::Held` 只有匹配分支）；"验证累计 3 次未通过转中断中"、"失败 3 次生成错误中断"均无 |
| FR-25 | 适配器接口（probe/start/parse/interrupt/resume/send/channels） | M3 | 未实现 | rg `HarnessAdapter|adapter` 无 trait | |
| FR-25-ClaudeCode | Claude Code 适配器 | M3 | 未实现 | 同上；本机有 `claude` | |
| FR-25-Codex | Codex 适配器 | M3 | 未实现 | 同上；本机有 `codex` | |
| FR-25-Cursor | Cursor CLI 适配器 | M3 | 未实现 | 同上；本机有 `agent` | |
| FR-25-Gemini | Gemini CLI 适配器 | M3 | 未实现 | 同上；本机没有 `gemini` | |
| FR-26 | Claude Code 用 `--bare`，技能与网关显式传入 | M3 | 未实现 | rg `--bare` 只在测试的 `git init --bare` 里 | |
| FR-27 | 中断先 SIGINT，60 秒后 SIGTERM | M3 | 未实现 | rg `SIGINT` 无结果（verifier 只回收进程组） | |
| 10.4 任务卡 | 任务卡含验证命令、接口契约、决策摘要、禁止事项、交互约定、临时规则、检查点与报告要求；M3 写入 `runs/<run>/task.md` | M0/M1 | 部分实现 | claim/show 返回任务 JSON + 运行 + 委托卡（task_service.rs:321,433）；规矩在 agents-dev.md | 没有拼成完整任务卡（缺验证命令、接口契约、决策摘要、生效规则）；M3 的 task.md 文件未做 |
| FR-28 | 运行报告结构（status/summary/checkpoints/changed_paths/questions/suspected_class/proposal）；v0.1 `finish --report` | M3 | 未实现 | args.rs:146-152 只有 `--wait`、`--claims`；rg `suspected_class|needs_decision` 无结果 | `--claims`（FR-209）只覆盖逐条自述 |
| FR-29 | 无报告时 `run.finished` 记 failed，存最后一条消息 | M3 | 未实现 | 无运行器 | |
| FR-30 | 角色→harness/模型映射写进 config.yaml，可按模块覆盖 | M3 | 未实现 | config.rs 只有 project_id/docs_dir/委托设置（config.rs:47-96） | |
| FR-31 | 并发上限默认 4、排队；v0.1 在关注清单提示人工中断偏多 | M3 | 未实现 | rg `concurren|中断偏多` 无结果 | v0.1 的提示也没有 |
| FR-51 | 技能包按版本复制到 `.towermd/skillpacks/<name>@<ver>/` 并记哈希 | M3 | 未实现 | rg `skillpacks` 无代码 | |
| FR-52 | `skillpacks.yaml` 按角色启用技能，运行器组装目录并注入 | M3 | 未实现 | 同上 | |
| FR-53 | 技能包声明交互点并映射到中断/门禁 | M3 | 未实现 | 同上 | |
| FR-54 | 禁用与 Tower.md 冲突的步骤（建分支、合并、开 PR） | M3 | 未实现 | 同上；v0.1 只在 agents-dev.md:12 写明优先级 | |
| FR-32 | 合并前在变基后的分支上跑全部模块验证，证据绑定提交 | M0/M1 | 已实现 | v0.1 由 `dev finish` 执行（见 FR-107）；task_finish.rs:327 收集主模块与 touches 的命令 | M3 集成器执行未做 |
| FR-34 | 慢层提案可配一次异构模型交叉评审，只供参考 | 未分配 | 未实现 | rg `cross|交叉评审` 无结果 | |
| FR-121 | 模块 verify 视为内置检查 `kind: core.verify, deterministic: true`；不配检查时只能由人确认完成 | M2 | 未实现 | rg `core.verify|deterministic` 无结果；todo.md:556 记录"开发套件的任务不能由人一步确认完成，必须经检查"（与本条相反） | |
| G0 | 派发前：验收标准、允许路径、租约、相关决策 | M0/M1 | 已实现 | task_model.rs:410-426 | 规格要求"生成提问中断：缺信息"，现在只是领取报错 |
| G1 | 计划评审：v0.1 高风险模块经评审请求请人评审 | M2 | 未实现 | rg `G1|review request` 只在技能说明和 args.rs:309 `NOT_YET` 里 | |
| G2 | 路径与策略（守卫、慢层保护；敏感文件、危险命令） | M0/M1 | 部分实现 | `dev check`；path_guard.rs；事后检测在核心（FR-103） | "敏感文件、危险命令"两类规则没有 |
| G3 | 运行模块验证命令 | M0/M1 | 已实现 | 见 FR-107/FR-32 | "可选检查行为改动是否带测试"未做；自动修复次数（FR-57）未做 |
| G4 | 代码评审：v0.1 技能请求评审、人在 App 处理 | M2 | 未实现 | 无评审中断类型、无 `ask --review` | |
| G5 | 变基到最新 main 后再跑 G3；只差 state/ 与 canvas 时复用证据 | M0/M1 | 已实现 | task_finish_tests.rs `state_only_main_advance_requires_actual_rebase_and_new_g5_but_reuses_g3`、`side_task_tip_in_illegal_merge_is_not_an_admission` | |
| FR-55 | 门禁定义写在 `.towermd/gates.yaml`，可按层/模块/风险覆盖 | 未分配（G1/G4 需要，宜随 M2） | 未实现 | rg `gates.yaml` 无代码 | |
| FR-57 | 硬门禁自动修复默认 2 次，用完生成门禁失败中断 | 未分配 | 未实现 | rg `auto_fix` 无结果 | 失败告警 `task_gate_failed` 只用于合并证明（finish_service.rs:161） |
| FR-58 | 模块可标 `risk: high`，其 G1、G4 必须人评审 | M2 | 未实现 | rg `risk: high|"risk"` 无结果（baseline.rs 只有 `risks` 文字列表） | |
| 12.1-提问 | 提问中断 | M0/M1 | 已实现 | 核心 question_model；`ask`/`wait` | 状态机未进入"中断中"（见 10.2） |
| 12.1-评审 | 评审中断（G1/G4/G6、技能包评审交互点） | M2 | 未实现 | 无评审中断（G6 走核心提案批准） | |
| 12.1-授权 | 授权中断，10 分钟未答则拒绝 | M3 | 未实现 | 无 | |
| 12.1-禁止 | G2 拦截生成禁止中断，可撤销/转提案/豁免 | M0/M1 | 部分实现 | 越界提交生成告警 `unapproved_protected_change`（business_service.rs:593）并可转提案或撤销（main_resolution） | 豁免（带到期时间）未做 |
| 12.1-门禁失败 | 硬门禁自动修复用完时的中断 | 未分配 | 部分实现 | 告警 `task_gate_failed`（finish_service.rs:161；task_alerts.rs） | 无自动修复计数；无"重试/改验收/豁免"选项 |
| 12.1-错误 | 设计类错误、失败 3 次、harness 崩溃 | M3 | 未实现 | 无 | |
| 12.1-预算 | 运行或模块达到预算上限 | M3 | 未实现 | rg `budget|预算` 无代码 | |
| FR-60 | 不支持 MCP 的 harness 只用停车续跑 | M3 | 未实现 | 无适配器 | v0.1 的 `ask --parked`（args.rs:67）是另一回事 |
| FR-63 | Claude Code 用 `--permission-prompt-tool` 路由授权；无人值守时 `--permission-prompts none` | M3 | 未实现 | 无 | |
| FR-65 | 每类中断的处理策略写进 config.yaml（auto/auto_notify/human） | M3 | 未实现 | config.rs 无此键 | |
| FR-66 | 人可推翻总控代答，标记受影响检查点并建议重跑 | M3 | 未实现 | 无总控 | |
| FR-67 | 总控动作计划影响 ≥2 任务或涉及慢层时先预览，人确认后执行 | M3 | 未实现 | 无总控 | |
| FR-70(套件) | 快捷命令 `/retry` 重跑选中任务 | M3 | 未实现 | rg `/retry` 无结果 | |
| 14.3 | 错误分类（外部/执行/设计）及默认处理 | M3 | 未实现 | rg `suspected_class|external|design` 无相关代码 | v0.1 的"执行类：Agent 改完重新 finish"已自然满足 |
| 15.5(v0.1) | 任务暂停/汇报以插话送达；重派=释放租约回到就绪；暂停全部拒绝新领取 | M0/M1 | 部分实现 | TaskDetails.svelte:68-70（插话、请求暂停、要求汇报）；暂停全部见 ProjectControl 与 task_model.rs:398；新增任务 TaskCreate.svelte | App 没有"重派/释放运行"入口（守护进程只有 Agent 用的 `/task-release`） |
| 15.5(M3) | 运行器发中断、续跑、换 harness 重派、中间报告 | M3 | 未实现 | 无运行器 | |
| 16.2 | 守护进程 API 表 | M0/M1 | 部分实现 | daemon.rs:353-482 共 60 条路由（多数在 `/projects/{handle}/...` 下） | 缺 `GET /modules/:id`、`GET /tasks/:id`、`POST /seed`、`/tasks/:id/{pause,resume,release,report}`、`GET /holds`；路径形状与表不同 |
| 16.4-towermd_ask | MCP 工具：提问 | M3 | 未实现 | rg `mcp|json-rpc` 无结果 | |
| 16.4-towermd_request_review | MCP 工具：请求评审 | M3 | 未实现 | 同上 | |
| 16.4-towermd_permission | MCP 工具：授权（最长 10 分钟） | M3 | 未实现 | 同上 | |
| 16.4-towermd_note | MCP 工具：进度备注 | M3 | 未实现 | 同上 | |
| 6.3 事件表(套件部分) | run.checkpoint、run.retry、plan.changed、merge.done/rejected、skill.loaded 等 | 未分配 | 部分实现 | 已有 task.claimed/released、lease.*、evidence、task.merged、snapshot.written 等 | rg `run.checkpoint|run.retry|plan.changed|merge.done|skill.loaded` 无结果（多数依赖 M3） |
| 8.1-S2 | 按语言逐行解析 import（Rust/TS/JS/Svelte/Python/Go），失败退回目录 | M2 | 未实现 | cold_start.rs:139 诊断明写"未解析 import" | |
| 8.1-S3 | Louvain 社区发现聚类 | M2 | 未实现 | rg -i `louvain|communit` 无结果 | |
| M2-草稿模式 | 草稿模式下的模块编辑（含 J4：从当前基线进入草稿、对比影响后批准） | M2 | 部分实现 | S7 草稿编辑已有改名/移动/合并/拆分/接口/命令/引用（review.rs:19-60；DraftEditor.svelte）；提案带 `draft_id` 与影响分析（proposal_model.rs:11-22,199） | 草稿只能从扫描 + S6 提交生成（draft_store.rs:367），不能从当前已批准基线进入草稿；没有基线与草稿的对比视图 |
| 9.1-种子 | `dev seed` 把一句话写成意图 I-1 草稿；`POST /seed` | M2 | 未实现 | args.rs DevArgs 无 Seed；daemon 无 `/seed` | |
| 9.1-澄清 | 最多 3 个澄清问题以卡片显示，可跳过 | M2 | 未实现 | 无 seed 流程（`ask` 存在但没有接入） | |
| 9.1-骨架 | 1–3 个模块、验收、≤7 任务，首任务为冒烟；用 S6 schema 经 draft submit 提交 | M2 | 未实现 | `dev draft template` 必须给 scan id（args.rs:182） | |
| 9.1-批准 | 一键批准 B0；未初始化仓库先 `git init` | M2 | 未实现 | rg `"init"` 只有 init 命令里的 git 检查 | B0 批准本身已有（baseline publication） |
| towermd-seed 技能 | 一句话生长技能 | M2 | 未实现 | skills/ 无 towermd-seed | |
| 9.2 | 生长期/定型期两套规则与手动切换 | M2 | 未实现 | rg `生长|定型|growth|maturity` 只在 skills/README.md:23 | |
| 9.3-模块过大 | >3000 行或 >40 文件提示拆分 | M2 | 未实现 | 无 | |
| 9.3-跨模块改动 | 最近 10 任务 touches 比例 >30% | M2 | 未实现 | 无 | |
| 9.3-未划分区膨胀 | 未划分区 >10 文件 | M2 | 未实现 | 无 | |
| 9.3-需求未分配 | 意图 `allocated_to` 为空 | M2 | 未实现 | 无 | |
| 9.3-同类错误反复 | 24 小时内同类错误 ≥3 次 | M2 | 未实现 | 无（还依赖 M3 的错误分类） | |
| FR-22 | 每步生长后 main 能通过当前基线定义的验证 | M2 | 部分实现 | 每个任务的 G3/G5 保证合并前模块验证通过 | 没有基线级的整体验证，也没有生长步骤这个概念 |
| NFR-07(v0.1) | 能执行命令行、加载技能的 harness 都能接入 | M0/M1 | 已实现 | 技能只调用命令行；claim 接任意 `--harness` 自报 | |
| NFR-07(M3) | 新增无头 harness 只需新增适配器 | M3 | 未实现 | 无适配器层 | |
| NFR-09(v0.1) | 记录会话报告的技能版本 | M0/M1 | 已实现 | `task claim --skills-version`（args.rs:114-126，main.rs:408） | |
| NFR-09(M3) | 无头运行加载相同版本与哈希的技能包 | M3 | 未实现 | 见 FR-51 | |
| M2 验收 | 一句话长到 5 个模块，每次人定变更都有决策和标签；真实仓库上人工合并、拆分次数比 M1 少 | M2 | 未实现 | 依赖 seed、生长信号、import 解析 | |
| M3 验收 | 4 个无头 Agent 并发跑 20 个任务，main 始终可验证；越界改动合并前 100% 拦截 | M3 | 未实现 | 依赖 M3 全部组件 | |

## 五、CLI 规格逐条状态（来自 CLI 审计；计数时把合并在一行的命令组拆开算）

| id | 摘要 | 阶段 | 状态 | 证据 | 缺口 |
|---|---|---|---|---|---|
| FR-190 | CLI 与 App 能力对等，附对等表和测试 | C3 | 未实现 | grep `能力对等\|parity` 无结果。daemon.rs:353-480 里有一批只给 App 用的路由，CLI 没有对应命令：proposals 的 publication/approve 与 reject、baselines 的 publication/approve、review、verify、`/answers`、`/prompts`、`/resume`、`/tasks/human`、delegations 的 finalize、used、confirm、send-back、`/rules`、`/work-kinds/rename`、`/delegation-levels`、`/main-resolutions/confirm`、`/hub/rebuild/{id}/confirm`、WS `/events` | — |
| FR-191 | TTY 时输出文本，否则输出 JSON；单一信封 | C0 | 已实现 | main.rs:29、41；output/envelope.rs；测试 `json_success_is_one_envelope`、`text_failures_write_only_to_stderr` | `watch` 不存在 |
| FR-192 | 错误码与退出码 0–8 | C0 | 已实现 | output/code.rs:49-131、help.rs:144；测试 `unreachable_daemon_exits_eight`、`usage_errors_exit_two_without_retry` | 多了规格表里没有的 `rejected`（退出码 6）；代码里从没构造过 `hub_offline` |
| FR-193 | 所有写操作带请求 ID，可幂等重试 | C0 | 部分实现 | args.rs:257 `is_write`、main.rs:87；测试 `scan_start_retry_with_the_same_request_id_returns_the_same_job`、`snapshot_and_rebuild_requests_are_idempotent` | `dev draft submit`、`dev scan cancel`、`init` 不带请求 ID（main.rs:254、258） |
| FR-194 | 送达内容放进 `delivered`，输出成功后才 ack | C0 | 已实现 | main.rs:103-176；测试 `cli_relay_answers_are_bound_temporary_durable_and_delivered_only_after_output` | — |
| FR-195 | 人的动作经挑战、钥匙串签名后记为 `human:` | C3 | 未实现 | 无 `human` 命令；grep keychain、SecureEnclave、LocalAuthentication、security-framework 均无结果；测试 `answer_without_relayed_needs_a_human` | — |
| FR-196（含 §4.2 宽限期） | 两档验证：每次验证，或 5 分钟宽限期 | C3 | 未实现 | 没有相关代码 | — |
| FR-197 | 语言按 --lang、TOWERMD_LANG、LC_ALL、LANG、AppleLanguages 的顺序取 | C0 | 部分实现 | output/lang.rs:41-58，有单测；测试 `help_follows_the_language_and_shows_examples` | lang.rs:2 自己写明守护进程的消息不翻译，返回的 message 多为中文 |
| FR-198 | 不带参数的 `towermd`：最多 3 条关注项和 3 行摘要 | C1 | 未实现 | args.rs:25 子命令必填；main.rs:72 报 usage | — |
| FR-199 | 每个命令都有 schema；help 带例子并按组显示 | C0 | 部分实现 | schema.rs:7（单测 `every_visible_leaf_command_has_exactly_one_schema`）；help.rs:108 EXAMPLES（单测 `every_example_parses`） | help 没有按 §2.1–2.7 分组；没有 `completion` |
| FR-200 | 重构命令，不保留旧名 | C0 | 已实现 | 测试 `v0_1_1_command_names_are_usage_errors`、`help_lists_only_implemented_commands` | `review request → ask --review` 这一行没落地；三份规格正文仍有旧名（todo Q7） |
| `towermd`（无参数） | 3 行摘要与 3 条关注项；在项目外显示总览 | C1 | 未实现 | 同 FR-198 | — |
| status | 状态 | C0 | 已实现 | main.rs:277 → `/task-status`；task_cli 测试 | — |
| needs / since | 关注清单 / 自上次查看以来 | C1 | 未实现 | args.rs 里没有 | — |
| overview | 全部项目总览 | C3 | 未实现 | 没有命令，也没有多项目注册表 | — |
| show `<id>` | 显示任意对象 | C1 | 部分实现 | args.rs:34 只接 project；main.rs:213 | 只能看当前项目，不能看任意对象 |
| ls / history / open | 列表 / 时间线 / 在 App 中打开 | C1 | 未实现 | 没有 | — |
| watch | 事件流 | C1 | 未实现 | WS `/events` 只给 App（测试 `app_only_subscription_...`） | — |
| report / signal | 汇报进度 / 上报信号 | C2 | 未实现 | 没有 | — |
| ask | 提问，含 `--review` | C0 | 部分实现 | args.rs:54-68；question_cli 测试 | 没有 `--review`；skills/towermd-ask/SKILL.md:31 仍把 `review request` 写成 M2 |
| wait `[--seconds N]` | 等回答或等验证结果 | C0 | 部分实现 | args.rs:69、daemon.rs:901 | 没有 `--seconds`；只能等提问 |
| reply | 回应汇报请求 | C2 | 已实现 | delegation_service.rs:193；测试 `a_delegation_runs_from_clarify_to_send_back_...` | 摘要超过 100 字时报普通错误，不是 `too_long` |
| propose | 申请改人定内容 | C0 | 已实现 | main.rs:274；proposal_cli 测试 | — |
| frame | 卡片动词 | C2 | 未实现 | 没有 | — |
| approve / decline | 批准或驳回 | C3 | 未实现 | 只有 App 路由（daemon.rs:393-394）；测试 `approval_commands_are_rejected` 明确断言 CLI 没有这两个命令 | — |
| allow / deny | 授权或拒绝 | C3 | 未实现 | 无命令，也没有授权中断 | — |
| done | 确认完成 | C3 | 未实现 | 只有 App 的 `/delegations/{id}/confirm` | — |
| answer（人） | 人回答 | C3 | 未实现 | CLI 返回 needs_human；只有 App 的 `/answers` | — |
| answer --relayed | 受托者代为记录 | C0 | 已实现 | `/answers/relay`；测试 `cli_relay_answers_...` | — |
| assign / promote / waive | 委托 / 立项 / 豁免 | C3 | 未实现 | 没有 | — |
| level set | 设置委托级别 | C3 | 未实现 | 只有 App 的 `/delegation-levels` | — |
| rule add | 临时规则 | C3 | 未实现 | 只有 App 的 `/rules` | — |
| resume `<范围>` | 解除暂停（每次验证） | C3 | 部分实现 | args.rs:47 只接受 `all`；CLI 只发请求，由 App 确认（测试 `cli_pause_and_resume_request_keep_process_identity_...`） | 不能按范围；不能一步完成；没有在场验证 |
| hub rebuild | 一步完成 | C3 | 部分实现 | main.rs:310 只发请求，由 App 确认 | 仍是两步（todo Q4） |
| send | 插话 | C3 | 未实现 | 只有 App 的 `/prompts` | — |
| pause `<范围>` | 暂停（宽限期验证） | C3 | 部分实现 | args.rs:43 只接受 `all`，署名 process:towermd-cli | 不能按项目、容器、任务暂停；没有宽限期验证 |
| follow-up / frame new | 跟进项 / 建容器 | C4 | 未实现 | 没有 | — |
| task add | 新增任务 | C0 | 已实现 | args.rs:130；task_cli 测试 | — |
| task edit / task ls | 改任务 / 列任务 | C0 | 未实现 | args.rs:129-141 只有 Add、Claim、Show、Release | — |
| task show | 任务卡加委托信息 | C0 | 已实现 | task_service.rs:405-433 返回 `delegation`；CHANGELOG.md:20 | — |
| task claim `[id]` + 指纹参数 | 领取 | C0 | 部分实现 | args.rs:134、114-126；测试 | `id` 是必填，规格写的是可选 |
| task release | 放弃 | C0 | 已实现 | daemon.rs:865 | — |
| idea | 记下想法 | C4 | 未实现 | 没有 | — |
| decision show/ls、rule ls | 查看决策 / 查看规则 | C1 | 未实现 | 没有 | — |
| ext、template | 扩展 / 模板 | C4 | 未实现 | 没有 | — |
| init | 初始化 | C0 | 已实现 | init_cli 测试 | — |
| up | 启动守护进程与 App | C0 | 已实现 | app_launch.rs:96（只支持 macOS） | 没有自动化测试 |
| daemon ensure/status/stop | 管理守护进程 | C0 | 已实现 | 测试 `ensure_starts_one_loopback_daemon_and_stop_ends_it` | — |
| snapshot | 立即写快照 | C0 | 已实现 | 测试 `snapshot_and_rebuild_requests_are_idempotent` | — |
| rebuild-index / doctor / whoami | 重建索引 / 自检 / 我是谁 | C1 | 未实现 | 没有 | — |
| human setup/status/revoke | 人类签名密钥 | C3 | 未实现 | 没有 | — |
| schema | 输出 JSON Schema | C0 | 已实现 | 测试 `schema_lists_commands_and_describes_input_and_output` | — |
| help / --version | 帮助 / 版本 | C0 | 已实现 | clap 自带加 help.rs 本地化 | — |
| completion | shell 补全 | C0 | 未实现 | 需要新依赖 `clap_complete`（todo Q3） | — |
| dev scan / dev draft | 冷启动 S1–S5 / S6 草稿 | C0 | 已实现 | scan_cli 测试 | — |
| dev check | 路径守卫（G2） | C0 | 已实现 | 测试 `finish_rejects_dirty_and_protected_edits_...` | — |
| dev clarify | 复述任务、起草完成标准、提问 | C0 | 已实现 | main.rs:235；delegation_cli 测试 | — |
| dev finish [--wait] [--claims] | 冻结提交并后台验证 | C0 | 已实现 | main.rs:231-244；测试 | — |
| dev trailers | 输出合并 trailer | C0 | 已实现 | main.rs:246；task_cli 测试 | — |
| dev seed | 一句话长出骨架 | devkit M2 | 未实现 | DevArgs 里没有 | — |
| dev run / queue / adapters | 无头运行 / 合并队列 / 适配器 | devkit M3 | 未实现 | 没有 | — |
| --run / --json / --text / --lang / --request-id / --quiet | 全局参数 | C0 | 已实现 | args.rs:9-24；render.rs:35-92 | — |
| --project | 按路径或项目 ID 指定项目 | C0 | 部分实现 | main.rs:189；测试 `another_project_is_a_usage_error` | 只接受当前仓库的项目 ID，不接受路径，不能跨项目 |
| §3.5 `--json <文件\|->` | 结构化输入 | C0 | 部分实现 | 改成了位置参数加 `-`（测试 `input_dash_reads_standard_input`） | 规格还没改（Q1） |
| §3.5 长度限制 30 / 100 字 | 超长报 too_long | C0 | 部分实现 | 只有 core delegation_projection.rs:518 的摘要检查 | 报的不是 `too_long`；没有标题 30 字检查 |
| §3.5 输入上限 2 MiB | 输入文件上限 | C0 | 已实现 | main.rs:24、430；测试 `oversized_input_is_too_long` | — |
| §3.6 单次等待 ≤90s | 单次阻塞上限 | C0 | 已实现 | daemon.rs:639、901 | — |
| §3.6 累计超过 live_wait_max 返回 deferred | 累计等待上限 | C0 | 已实现 | question_model.rs:14；测试 `a_parked_question_ends_the_turn_...` | 上限是常量，不能配置 |
| §3.7 离线 | 过期警告、`queued:true`、退出码 8 | 未分配 | 未实现 | `HubOffline` 从未构造；没有 `hub-worker/` 目录 | — |
| §3.9 NO_COLOR | 颜色规则 | C1 | 不适用 | 根本不输出颜色，属于自然满足 | — |
| §3.9 更新"上次查看" | 上次查看时间 | C1 | 未实现 | 没有相关代码 | — |
| §4.3 规则 1–8 | Secure Enclave、App 显示 6 位码确认登记、挑战、终端检查、审计 | C3 | 未实现 | 没有相关依赖和代码 | — |
| §4.4 签名构建 | C3 的前提 | C3 | 不适用 | v0.2.0 仍无签名 | — |

## 六、只在核心规格中实现、本文不计数的条目

套件规格中的以下条目只是指向核心规格，或者属于核心机制。它们的状态要看核心规格的差距分析：

FR-04、FR-05、FR-07、FR-09、FR-33、FR-56、FR-59、FR-61、FR-62、FR-64、FR-68、FR-69、FR-70（核心部分）、FR-71、FR-72、FR-81、FR-82、FR-99、FR-103、FR-109。

## 七、本机环境

- 有：`claude`、`codex`、`agent`（Cursor CLI）、`git`。
- 没有：`gemini`、`arca`、`cursor-agent`。
- 依赖：
  - workspace 里没有 rusqlite、notify、clap_complete、钥匙串类 crate。
  - tokio 没开 `process`/`io-util` feature。
  - 前端没有 elkjs、marked、mermaid。
  - 没有 `hub-worker/` 目录。
