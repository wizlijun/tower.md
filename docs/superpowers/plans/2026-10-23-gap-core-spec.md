# Tower.md 核心规格 v0.7 差距分析

- 规格：`docs/specs/2026-10-21-spec-towermd-core.md`（v0.7）
- 代码：分支 `feat/delegation-trust-ledger`，HEAD `795a5ce`；只读分析，未改动仓库
- 范围：`crates/towermd-core`、`crates/towermd-devkit`、`crates/towermd`、`src-tauri`、`src`、`skills/`；`hub-worker/` **不存在**
- 判定口径：只认有代码路径的实现，最好还要有测试。只有类型或文档提及的，不算实现。"部分实现"一栏写明缺什么。
- 阶段归属取自 §14。§14 未列、也不属于 M0/M1 交付面的条目记为"未排期"。附录 A 的第一方扩展按 §14 末句随第 2 阶段（PlantUML 模板第 1 阶段、Note.md 连接器、起步套装）。FR-116、FR-117 在第 1 阶段（FR-114–118）出现，在第 2 阶段再补 HTTP、文件入口和失联处理，这里计入第 1 阶段。

---

## 1. 按阶段统计

| 阶段 | 条目数 | 已实现 | 部分实现 | 未实现 | 不适用 |
| --- | --- | --- | --- | --- | --- |
| 已完成 M0/M1（含 NFR） | 59 | 44 | 13 | 0 | 2 |
| 第 1 阶段 | 14 | 0 | 8 | 6 | 0 |
| 第 2 阶段（含附录 A 第一方扩展） | 56 | 6 | 2 | 48 | 0 |
| 第 3 阶段 | 13 | 5 | 2 | 6 | 0 |
| 基础设施 | 2 | 0 | 1 | 1 | 0 |
| 未排期 | 19 | 3 | 6 | 9 | 1 |
| **合计** | **163** | **58** | **32** | **70** | **3** |

要点：
- M0/M1 交付面基本完整：hub、快照、租约与 epoch、署名、`tower.canvas`、提问与回答、提案批准。缺口集中在 v0.7 改写后的通用化部分：hub 地址只能是本地，通用租约还没覆盖容器和文本源文件，插话状态没有"已处理"。
- 第 1 阶段一条都没有完整实现。v0.7 的接入接口（`report`、`signal`、统一信封、4 档级别、信号合并）、确定性门禁声明、承诺弱化、"自上次查看以来"在代码里完全没有。已有的部分来自开发套件的专有路径（检查判定完成、提问、汇报请求）。
- 第 2 阶段只有本分支实现的委托与信任账本相关 6 条（FR-184、201、208–211）。布局模型、容器、模板、文本源容器、PlantUML、跟进项、休眠、立项、扩展清单全部为零。
- 第 3 阶段：信任账本 FR-203–207 已实现；FR-185（级别只记录、不执行）和 FR-202（过程类事实和越界恒为空）为部分实现。
- 基础设施：Cloudflare hub 不存在；持久发件箱有，但没有远端 hub 可补发。

---

## 2. 实现工作包

标记：**新依赖**指 Rust crate 或 npm 包，按 AGENTS.md 须人确认；**不变量**指可能触及的 AGENTS.md 产品不变量。

### 第 1 阶段

| 包 | FR | 规模 | 依赖 | 新依赖 | 不变量 |
| --- | --- | --- | --- | --- | --- |
| **P1-A 命名第一版与 `set_by`**：§13 名词表落进界面、接口和 CLI 输出；对象带 `set_by: human/delegate`，由 `layer` 换算；界面去掉 模块/门禁/慢层/基线/租约/main 等行话 | FR-188、NFR-14（顺带 FR-01 换算） | M | 无（建议最先做，后续包直接用新名词） | 否 | 慢层保护（`set_by` 和 L0–L2 的换算口径必须与 `Layer::is_slow` 一致，见歧义 1） |
| **P1-B 首次打开只有空画布**：App 首屏换成空画布加一句话输入框，不问项目类型；一句话变成线索或卡片 | FR-181（顺带 FR-182 的一句话起头） | M | P1-A；落卡的写入路径依赖 P2-G 的线索模型时可先简化 | 否 | 一处定序（输入框只能经守护进程写）；人机署名（`human:` 只从 App 发出） |
| **P1-C 接入接口：统一信封、`report`/`signal`/`reply`、4 档级别、信号合并**：核心层的信封类型和投影；CLI `towermd report`、`towermd signal`；同受托者、同 kind、同 target 合并计数；`request_report` 从检查点特例泛化 | FR-114、115、116（核心部分）、117（CLI 入口）、118、119；NFR-07 | L | P1-A | 否（serde 已有） | 人机署名（来源只能是 `process:` 或 `<harness>/<ver>`）；harness 细节只在适配器里；费用如实 |
| **P1-D 门禁声明与完成判定通用化**：`gates.yaml` 声明 `deterministic: true` 的信号种类为门禁；完成只认"所有门禁检查通过"或"人在 App 确认"；检查和门禁结果写成通用证据（`kind: gate/check`，绑定对象版本） | FR-122、124、56（核心化）、33 | M | P1-C | 否 | 进度只认证据；不自动放行；慢层保护（`gates.yaml` 本身属人定内容） |
| **P1-E 承诺弱化检测**：比较 `gates.yaml`、阈值、完成标准、检查集合的前后版本，生成 `active` 级 `core.weakening` | FR-125 | M | P1-C、P1-D | 否 | 慢层保护（弱化必须走人批准） |
| **P1-F 关注卡与"自上次查看以来"**：卡片 6 段固定格式、不超过 100 字；记录人上次关闭画布的时间（每用户、可重建）；打开项目先给 3 行摘要；排序改为 FR-128 规则 | FR-129、131、42、40/41（升级到 v0.7） | M | P1-C（信号来源） | 否 | 派生物可重建（上次查看时间不能成为仓库唯一状态，见歧义 9）；不打扰 |

### 第 2 阶段

| 包 | FR | 规模 | 依赖 | 新依赖 | 不变量 |
| --- | --- | --- | --- | --- | --- |
| **P2-A 扩展清单与权限**：读取 `towermd-plugin.yaml`；扩展只声明、核心不执行扩展代码；扩展禁止 `human:` 署名，不能改人定对象和 `gates.yaml` | FR-120、123 | L | P1-C、P1-D | 可能（清单 schema 校验；serde_yaml 够用就不需要） | 人机署名；慢层保护 |
| **P2-B HTTP 与文件入口**：`report`/`signal` 的本机 HTTP 端点（非 App 客户端）和文件投递入口 | FR-117（其余部分） | S–M | P1-C | 否 | 本地优先（只绑定 127.0.0.1）；一处定序（文件入口要经守护进程转成事件，不能让文件成为状态，见歧义 7） |
| **P2-C 汇报请求通用化与失联**：任意对象、任意受托者的 `request_report`；`core.silent`（连续 2 次升级），把现有 `report.overdue` 并进来 | FR-116、137 | S–M | P1-C | 否 | 不打扰 |
| **P2-D 兜圈子与节点提问**：从事件计算 `core.stuck`（阈值在 `config.yaml`）；对任意节点提问，转成发给声明了 `answers:` 的受托者的 `request_report` | FR-126、127 | M | P1-C、P2-C | 否 | 不打扰 |
| **P2-E 注意力预算与打扰时机**：跨来源统一出口，`active` 超过 3 条时按 FR-128 排序；立即、下次查看、摘要 3 档；在卡片上一步完成批准、回答、确认 | FR-128、130、138、40 | M | P1-F | 可能（`time_sensitive` 的系统通知需要 Tauri notification 插件，属新依赖） | 不打扰；不自动放行 |
| **P2-F 跟进项、休眠与浮现** | FR-134、135、136 | M | P1-F、P2-E | 否 | 派生物可重建（跟进项存 hub 并随快照归档） |
| **P2-G 立项（想法与承诺分开）**：线索和未立项卡片不进关注清单、不派发；"立项"是人的动作 | FR-183（及 FR-182 两种起头） | M | P1-B | 否 | 人机署名；慢层保护 |
| **P2-H 布局模型、模板、容器与容器规则**：3 项布局配置；`type: template` 清单；3 种建容器方式；容器规则版本化；`towermd frame` 等 CLI；3 种模式；稳定增量布局；人改字段的保护；卡片上限；状态集合；归属与存储分离；容器作为 `target`；旁注；生命周期；起步模板与起步套装 | FR-141–154、156、160–162、143、189（以及 FR-187 扩展到容器） | L（实际接近 XL，建议再切成 3–4 个子包） | P1-A、P1-C、P1-D、P2-A | 可能（elkjs 已在 AGENTS 表中登记，但 `package.json` 里还没有，引入前须人确认版本线） | 快层不移动节点（FR-132）；一处定序；慢层保护（人定容器的写入转提案）；人机署名 |
| **P2-I 时间滑块与变化高亮** | FR-157、158 | M | P2-H、P1-F | 否 | 派生物可重建（历史从事件和 git 计算，不另存） |
| **P2-J 文本源容器与 PlantUML 第一方模板**：`source: file` 解析器接缝；双向写入；稳定标识；冲突以文件为准；解析报告（已理解、看不懂、理解比例）；失败锁定；只读区域；信号默认 `passive`；大小和时间上限、不联网；PlantUML 映射、两种显示、图种分阶段、Agent 规则、Lint 扩展 | FR-163–169、174–178、170–173、179、180 | L | P2-H、P2-A | **很可能**：PlantUML 子集解析器（自写，或新 crate）；外部渲染器（Java 版 PlantUML）是运行时依赖 | 本地优先（渲染器不联网）；慢层保护（人定文件改动转提案）；一处定序（直接改文件和经画布写入要汇合到同一序列） |
| **P2-K Note.md 连接器**：回写捕获层的纯 `.md`（OKF，`generated.by: process:towermd`）；`sources` 记录 Note.md OKF id 和块引用 | FR-80、79 | M | P1-C | 否 | 两道人工闸门（不创建 `.note.md`、不自动结网）；不改别人的地盘 |

第 2 阶段已实现、无需新包：FR-184、201、208、209、210、211（本分支的委托与信任账本）。

### 第 3 阶段

| 包 | FR | 规模 | 依赖 | 新依赖 | 不变量 |
| --- | --- | --- | --- | --- | --- |
| **P3-A 多项目总览与健康色**：本机全部项目各为一个节点；健康色、待办数、最近活动；跨项目关注清单；项目和容器聚合值 | FR-133、39 | M | P1-F、P2-E | 否 | 本地优先；派生物可重建 |
| **P3-B 委托级别的执行与账本补全**：按"配置 × 工作类别"实际执行级别（目前只记录和建议）；补上账本的过程类事实（兜圈子、承诺弱化）和越界计数 | FR-185、202 | S–M | P1-E、P2-D | 否 | 不自动放行；慢层保护（级别再高也不能绕过人定内容的批准） |
| **P3-C 决策复查日期**：`review_on` 到期后，决策进入关注清单候选 | FR-186 | S | P1-F | 否 | 不打扰 |
| **P3-D 变化摘要**：扩展提供不超过 7 行的结构化摘要 | FR-139 | S–M | P2-A | 否 | — |
| **P3-E 视觉词汇**：约 16 个图标、4 档级别的颜色与形状、3 种连线；中断图标的聚合显示规则 | FR-140、73、44 | M | P1-A；**被 §16 开放问题 6 阻塞**（图标集未定义） | 否 | 不只靠颜色（UI 规则）；快层不移动节点 |
| **P3-F 时间派生视图**：累积流图等由事件计算的视图 | FR-159 | M | P2-H、P2-I | 可能（图表库） | 派生物可重建 |
| **P3-G 文本导入导出与模板分享**：容器导出为 Markdown、Mermaid、PlantUML；FR-144 的文本导入 | FR-155、FR-144（导入部分） | M | P2-H、P2-J | 可能（Mermaid 解析；渲染用已有的 mermaid npm 包） | 慢层保护（导入的内容按归属落地） |

### 基础设施

| 包 | FR | 规模 | 依赖 | 新依赖 | 不变量 |
| --- | --- | --- | --- | --- | --- |
| **I-A Cloudflare hub、WebSocket hub 客户端与离线补发**：`hub-worker/`（Worker 加每项目一个 SQLite Durable Object）；守护进程的远端 hub 客户端；断网时普通事件进发件箱、恢复后按原 `eid` 补发；领取、续租、取得主干租约必须在线；`config.yaml` 允许 hub URL（`catalog.rs:56` 目前拒绝非 local） | FR-86、88、84（hub 地址）、97（hub 地址） | L | 无（与第 1–3 阶段正交） | **是**：wrangler ^3、@cloudflare/workers-types ^4、vitest-in-workerd（类别 2026-10-16 已确认，版本需确认）；WebSocket 客户端 crate（类别已确认，具体库须在计划里写理由后确认） | 本地优先（云端 hub 可选、由用户自部署）；一处定序（hub 唯一定序）；守护进程只出站 |

### 未排期但有缺口的条目（建议纳入相关包或另立小包）

| 小包 | FR | 规模 | 说明 |
| --- | --- | --- | --- |
| U-1 变更等级：次要变更登记与豁免 | FR-03（及 FR-72 的豁免出口） | M | 目前只有"主要变更走提案"；没有带到期时间的豁免，也没有次要变更登记。触及"慢层保护不可绕过""豁免必须带到期时间" |
| U-2 临时规则与快捷命令 | FR-69、70 | M | `ProcessRule` 目前被拒绝；没有 `/report` 等快捷命令。人机署名：临时规则只能从 App 发出 |
| U-3 插话的"已处理"状态 | FR-68、71、74 | S | 已有已送达和排队中；缺"已处理"；事件名是 `prompt.queued/delivered`，不是 `human.prompt` |
| U-4 画布视觉：拟议变更虚线、过渡与幽灵层、1 秒合并、关系高亮 | FR-45、46、50、36、35（结构图） | M | 快层不移动节点 |
| U-5 CLI 在场签名的人类动作（CLI 规格 FR-195/196） | — | M | 与 AGENTS.md"命令行不提供批准类命令"冲突，须先由人裁定（歧义 2） |
| U-6 性能与时延验收 | NFR-03、NFR-08 | S | 没有基准测试；`tasks/todo.md` 记录 NFR-03 未验收 |

---

## 3. 阻塞实现的规格歧义与矛盾

1. **人定内容的层范围（FR-188 对 FR-01）**：FR-188 的换算把 L0–L3 视为人定；代码 `Layer::is_slow` 和 AGENTS.md 的慢层都是 L0–L2。L3 归哪边会直接改变慢层保护的范围，须人决定。
2. **CLI 人类动作（FR-95、FR-111 修订版，CLI 规格 FR-195/196）对 AGENTS.md 不变量**：v0.7 和 CLI 规格允许经在场签名从命令行发出 `human:` 动作；AGENTS.md 写的是"命令行不提供批准类命令"。属于红线冲突，须先改 AGENTS.md 或改规格。
3. **FR-124 对开发套件**：核心说"人在 App 确认"也能算完成；开发套件任务目前只认检查（`docs/superpowers/specs/2026-10-22-delegation-trust-ledger-design.md` 的取舍）。开发任务是否允许人确认完成，没有写清。
4. **FR-185 的"级别"如何执行没有定义**：各级别具体放开哪些动作（自动合并？免确认？）没有写，FR-204 只写了抽查下限。现在的实现只记录、建议，不执行。
5. **FR-202 的"越界"数据源未定义**：哪个事件代表越界（提案被拒？路径守卫命中？），规格没给，`delegation_kit.rs:48` 只能返回 `out_of_bounds: None`。
6. **FR-206 "保存为决策"**：实现存在确认事件里，没有写决策文件。决策文件属于人定内容，要经主干写入者提交。规格须明确是否必须是独立的决策对象。
7. **FR-117 文件入口对 FR-84 和"一处定序"**：文件入口要把文件放在哪里（用户文档目录？`.towermd/`？运行目录？）没有写。FR-84 规定推进状态不写进任何工作树文件。须明确：入口目录在仓库外（运行目录），由守护进程消费。
8. **过时的交叉引用**：FR-172 写的是"映射到 6.2 的布局模型"，应为 8.x（FR-141）；FR-86、FR-93 引用 v0.5 的 5.4、16.2 节（确认端点）。v0.7 中这些节号已不存在。
9. **"自上次查看以来"的时间点存在哪里（FR-42、FR-131、FR-158）**：它是每个人、每台主机的状态。存在仓库会造成多人冲突；存在 hub 又要考虑快照重建。须明确为本机派生状态（丢失时退化为"最近 N 天"）。
10. **FR-128 对 FR-40 的排序**：FR-40 说"同类按影响对象数降序"，FR-128 给了跨类的顺序，两者叠加后的总序没有给全（比如同为 `active` 的授权和失败）。
11. **§16 开放问题**：问题 6（16 个图标没有清单）直接阻塞 FR-140、FR-73；问题 1、3、4、5、7、8、10 涉及 hub 认证、扩展分发、多人场景等，第 1 阶段不受影响，第 2、3 阶段之前要定。
12. **PlantUML 解析的实现路线（FR-170–179）**：规格要求离线、限时，而且要求"看不懂的行带行号"。自写 Rust 子集解析器，还是调用 Java 版 PlantUML（运行时依赖），没有决定；两者对 FR-178 的安全配置含义不同。
13. **NFR-03/NFR-08 在多机下的口径**：本机延迟能测；"经 hub 转发"的时延取决于 I-A，第 1 阶段无法验收。

---

## 4. 逐条状态表

列：ID | 一句话 | 阶段 | 状态 | 证据或缺口

### 4.1 已完成 M0/M1 的交付面

| ID | 一句话 | 阶段 | 状态 | 证据或缺口 |
| --- | --- | --- | --- | --- |
| FR-01 | 对象带 `layer`，权限按层不按路径 | M0/M1 | 已实现 | `towermd-core/src/types.rs:4`（`Layer`，`is_slow` 为 ≤L2）；catalog 测试 `invalid_layer_remains_readable_and_protected`。按 FR-188 的换算见 FR-188 |
| FR-02 | 人定内容只能经 App 批准、由主干写入者提交；越界改动转提案 | M0/M1 | 已实现 | devkit `proposal_model.rs:234`、`task_finish.rs:233` 路径守卫；`main_writer.rs` 慢层发布；`tests/proposal_cli.rs` |
| FR-04 | 每主机每用户一个守护进程，服务所有项目 | M0/M1 | 已实现 | `towermd/src/daemon.rs:64` `try_lock_exclusive`；`/projects/open` 多项目句柄；`tests/daemon_cli.rs` |
| FR-05 | 重启后凭 hub、git、Arca 恢复；索引可删可重建 | M0/M1 | 已实现 | core `project_runtime.rs` 恢复路径；测试 `recovery...`、`hub_contract.rs` 的 rebuild |
| FR-07 | 对象文件用 Markdown 加 OKF frontmatter | M0/M1 | 已实现 | core `catalog.rs`；`preserves_unknown_documents_and_skips_non_objects` |
| FR-09 | 事件 JSON，按 `seq` 排序，归档时一行一个 | M0/M1 | 已实现 | core `hub.rs`、`hub_journal.rs`（seq/eid/epoch/stale）；`hub.rs` 测试 append 幂等 |
| FR-81 | `init` 生成 `.towermd/AGENTS.md` | M0/M1 | 已实现 | core `init.rs:11-12,34,109`；`tests/init_cli.rs` |
| FR-82 | `type` 一处登记，读取宽容，未知键往返保留 | M0/M1 | 已实现 | `types.rs:49+` TYPES；catalog 测试 |
| FR-98 | `init` 生成 UUIDv7 项目 ID，首次打开不提问 | M0/M1 | 已实现 | `init.rs:24` project_id；`tests/init_cli.rs` |
| FR-83 | 人定对象、canvas、配置以 `main` 为权威 | M0/M1 | 已实现 | `project_view.rs`（authority commit、origin main 缓存）及其测试；`main_resolution_cli.rs` |
| FR-84 | 推进状态不写工作树，只追加到 hub | M0/M1 | 部分实现 | 推进状态只进 hub（`project_runtime.rs:99` common-dir）；缺：hub 地址只允许 `local`（`catalog.rs:56`） |
| FR-85 | 本地 hub 放在 `git-common-dir/towermd/`，多克隆互斥 | M0/M1 | 已实现 | `project_runtime.rs:99`；`hub.rs:345` hub.lock；`scan_cli.rs:348 concurrent_scans_share_job_and_project_lock_rejects_another_clone` |
| FR-87 | `append` 按 eid 幂等；`lease.*` 由 hub 自产 | M0/M1 | 已实现 | `hub.rs` 测试（append 幂等、`work_lease_names_are_kind_colon_id`）；`hub_contract.rs` |
| FR-89 | 快照只由主干租约持有者写 | M0/M1 | 已实现 | `main_writer.rs`；测试 expired fence |
| FR-90 | 快照时机：事件触发加周期 | M0/M1 | 已实现 | `project_runtime.rs` 测试 `periodic_snapshot...`；`towermd snapshot` 命令 |
| FR-91 | 写快照不碰工作树和 index | M0/M1 | 已实现 | `main_writer.rs` 测试（dirty worktree preserved、private snapshot promotion、race retry） |
| FR-92 | ack 后才折叠事件，检查点保留 eid | M0/M1 | 已实现 | `hub_contract.rs` 的 ack compaction preserves eid |
| FR-93 | `hub rebuild` 只发请求，App 批准后重建 | M0/M1 | 已实现 | CLI `hub rebuild`；`tests/project_control_cli.rs`；引用的 16.2 节已过时（歧义 8） |
| FR-94 | 除快照写入者外不得改 `<docs>/state/` | M0/M1 | 已实现 | devkit `task_finish.rs:233`、`proposal_model.rs:234` |
| FR-110 | 主干租约 2 分钟 TTL、30 秒续租 | M0/M1 | 已实现 | `project_runtime.rs` 测试 `main_lease_renews_at_thirty_seconds...` |
| FR-95 | 批准类动作只从 App 发出 | M0/M1 | 已实现 | `x-towermd-client: app` 校验；daemon 测试。v0.7 的 CLI 签名动作没做（歧义 2） |
| FR-111 | `human:` 事件只从 App 发出 | M0/M1 | 已实现 | 同上；`question_service.rs:80` `human:local` 只走 App 路由 |
| FR-96 | Agent 代录人在会话里的回答，署名 process | M0/M1 | 已实现 | `question_service.rs:144` AnswerRelayed；`tests/envelope_cli.rs:142`；CLI `answer --relayed` |
| FR-97 | 多机以项目 ID 为准，hub 地址写在配置里 | M0/M1 | 部分实现 | 项目 ID 已有；缺：hub 地址只能 `local`，无多机 |
| FR-187 | 通用租约：同一对象一个写入者，带 epoch | M0/M1 | 部分实现 | `kind:id` 工作租约（`hub.rs` 测试）用于任务；缺：容器、文本源文件租约（容器不存在） |
| FR-99 | 依赖租约的写入带 epoch，失效 epoch 被围栏 | M0/M1 | 已实现 | `hub.rs` 测试 global epochs、stale fence |
| FR-103 | fetch 后检查 `main` 上的人定内容是否被绕过修改 | M0/M1 | 已实现 | devkit `main_inspection.rs` 及测试 |
| FR-100 | `tower.canvas` 用 JSON Canvas 1.0，往返无损 | M0/M1 | 已实现 | core `canvas.rs`；`tests/canvas.rs`（逐字节往返、不透明的非法条目、重复项） |
| FR-101 | 画布条目对应对象；悬空诊断；未放置对象托盘 | M0/M1 | 部分实现 | `project_view.rs` 悬空、unsafe、symlink 测试；`App.svelte:1526` 未放置托盘；缺：v0.7 的容器、frame 条目映射 |
| FR-102 | 推进状态只在 App 叠加，不写 canvas | M0/M1 | 已实现 | `src/lib/runtime.ts` taskGraph 运行时位置；canvas 写入路径不含推进状态 |
| FR-43 | 人定对象坐标属人定内容，只在版本变化或人拖动时变 | M0/M1 | 已实现 | `ProjectCanvas.svelte` 只在草稿里可拖动 |
| FR-47 | 受托者推进的内容在画布上只读 | M0/M1 | 已实现 | `ProjectCanvas.svelte`（非草稿不可编辑） |
| FR-48 | 人在画布上的编辑进草稿，批准后生效 | M0/M1 | 已实现 | 草稿编辑 → `/drafts` → 提案批准；devkit `draft_store.rs` |
| FR-49 | WebSocket 推送事件，按对象增量更新 | M0/M1 | 已实现 | `project_service` events subscribe（`daemon.rs:409`）；`tests/ws_cli.rs`；`src-tauri` project_stream.rs |
| FR-40 | 关注清单首屏最多 3 条，其余放"稍后" | M0/M1（v1） | 部分实现 | `runtime.ts:88-127` 优先级排序加 `slice(0,3)`；`AttentionList.svelte` 写"更多关注（N）"；缺：按 FR-128 的跨来源排序、同类按影响对象数排序 |
| FR-41 | 关注卡格式以 FR-129 为准 | M0/M1（v1） | 部分实现 | 卡片只有 `kind · title`（`AttentionList.svelte:14`）；缺 6 段格式 |
| FR-59 | `ask` 可等待：分段阻塞加实时等待 | M0/M1 | 已实现 | `question_model.rs:121` ASK_WAIT_MS、`question_service.rs:247` LIVE_WAIT_MAX_MS；`main.rs:323-335` |
| FR-61 | 中断只停提问的任务，租约不因中断延长 | M0/M1 | 已实现 | 问题模型不续租；`question_cli.rs` |
| FR-62 | 按对象加 NFKC 归一文本去重 | M0/M1 | 已实现 | `question_model.rs:16` normalize_question；`question_service.rs:79`；`question_cli.rs:267` |
| FR-64 | 回答涉及人定内容时转草稿提案 | M0/M1 | 已实现 | `question_service.rs:10,71` Destination；慢层回答转提案 |
| FR-109 | 插话和回答排在下一次调用返回内容的最前面，记 delivered | M0/M1 | 已实现 | `/messages`、`/messages/ack` 路由（`daemon.rs:410-411`）；`question_cli.rs` |
| FR-35 | 提案含差异、理由、影响清单、备选、建议 | M0/M1 | 部分实现 | devkit `proposal_model.rs`（reason、alternatives、recommendation、impact）；缺：v0.7 的"结构图展示"影响对象没有核实到 UI |
| FR-37 | 批准后一次提交：更新人定对象、决策、版本清单 | M0/M1 | 已实现 | `main_writer.rs` 慢层发布；Git 标签由 devkit 打 |
| FR-38 | 新版本冲突的任务标"需复核"，下次调用时提示 | M0/M1 | 已实现 | devkit `task_model.rs:178` needs_review；`proposal_model_tests.rs:89-90`；`runtime.ts` 写"需复核基线"（文案违反 NFR-14） |
| FR-68 | 插话的送达状态：已送达、排队中、已处理 | M0/M1 | 部分实现 | `App.svelte:1394-1395` 读状态；缺"已处理" |
| FR-71 | 每条人工输入记 `human.prompt` 事件，进时间轴 | M0/M1 | 部分实现 | 事件是 `prompt.queued`（`question_service.rs:80`）；缺：统一的 `human.prompt`、范围和产生的动作 |
| FR-74 | 人工输入立即进时间线，并随送达状态更新 | M0/M1 | 部分实现 | App 读送达状态；缺"已处理"和对象时间线的统一显示 |
| NFR-01 | 本地优先，单机不需要云 | M0/M1 | 已实现 | 默认 `hub: local`（`config.rs`） |
| NFR-02 | 删索引能恢复；hub 丢失能从快照重建 | M0/M1 | 已实现 | `hub_contract.rs` rebuild、`project_runtime.rs` recovery 测试 |
| NFR-03 | 写入到画布 ≤2 秒；Z0 首屏 ≤1 秒 | M0/M1 | 部分实现 | 有实时推送；缺：基准测试或验收（`tasks/todo.md` 记录未验收） |
| NFR-04 | 只绑定 127.0.0.1，密钥不进仓库 | M0/M1 | 已实现 | `daemon.rs:91` 拒绝非 127.0.0.1、`:297` |
| NFR-05 | 费用未报告时显示"未知"，记 null | M0/M1 | 已实现 | `runtime.ts` costLabel 为"未知（未报告）" |
| NFR-06 | 守护进程日志与事件流分开 | M0/M1 | 已实现 | `daemon.rs:173` `daemon.log` 在运行目录，与 hub 事件分开 |
| NFR-07 | 能执行 CLI 或 HTTP 的受托者都能接入，新增接入方不改核心 | M0/M1 | 部分实现 | CLI 可接入；缺：`report`/`signal` 动词（P1-C），HTTP 非 App 客户端入口 |
| NFR-08 | 中断上画布 ≤5 秒；回答到达 ≤5 秒 | M0/M1 | 部分实现 | 实时等待和 WS 推送已有；无时延测试 |
| NFR-10 | 不卖 token | M0/M1 | 不适用 | 商业原则；代码中无 token 转售路径 |
| NFR-11 | 卸载后仓库仍是标准 git 仓库；不改全局配置 | M0/M1 | 已实现 | 测试统一设 `GIT_CONFIG_GLOBAL=/dev/null`；hub 放在 common-dir 里，在工作树之外 |
| NFR-12 | 多机一致：单写租约、失效 epoch 不入投影、快照连续前缀、eid 不重复 | M0/M1 | 已实现 | `hub.rs`、`snapshot.rs`（contiguous prefix）、`hub_contract.rs` |
| NFR-13 | 不统计用法 | M0/M1 | 不适用 | 原则；搜索 telemetry、analytics 无命中 |

### 4.2 第 1 阶段

| ID | 一句话 | 阶段 | 状态 | 证据或缺口 |
| --- | --- | --- | --- | --- |
| FR-188 | 每项内容带 `set_by: human/delegate` | 1 | 部分实现 | `set_by` 只出现在 `delegation_view.rs:49,102,309,346`（委托投影）；缺：对象模型、目录、画布通用字段及 `layer` 换算 |
| NFR-14 | 一个概念一个中英词，界面无行话 | 1 | 未实现 | 界面仍有 模块、门禁失败、慢层、main、基线、"取得模块租约"（`TaskDetails.svelte:36`、`runtime.ts`） |
| FR-181 | 首次打开只有空画布和一句话输入框 | 1 | 部分实现 | `App.svelte:1129-1136` 首屏是"打开项目"和"打开功能示例"；缺一句话输入框 |
| FR-114 | 所有消息用同一信封 | 1 | 部分实现 | 发往受托者的 `/messages` 通道已有；缺：入站统一信封（from、verb、kind、target、level、eid） |
| FR-115 | 动词集，`report`、`signal` 为最小集 | 1 | 部分实现 | 已有 ask、claim、finish、clarify 等专有命令（`args.rs`）；缺 `report`、`signal`、`reply` 动词（搜 `"report"`、`signal` 子命令无命中） |
| FR-116 | 发出 `request_report`、`answer`、`prompt` 三种消息 | 1（2 补全） | 部分实现 | `delegation_service.rs:52` 只为委托检查点生成 `R-<run>-<n>`；`answer`、`prompt` 已有；缺：通用 `request_report` |
| FR-117 | CLI、本机 HTTP、文件三种等价入口 | 1（2 补全） | 部分实现 | 只有 CLI 加 App 专用 HTTP；缺 `towermd report/signal`、非 App 的 HTTP 端点、文件入口 |
| FR-118 | 信号 4 档级别，对应 Apple 中断级别 | 1 | 未实现 | 搜 `time_sensitive`、`passive`/`active` 级别枚举：只有 `report.overdue` 用到 passive/active 字符串，没有通用级别 |
| FR-119 | 同来源、同 kind、同 target 的信号合并 | 1 | 未实现 | 没有信号模型；搜 coalesce、merge signal 无命中 |
| FR-122 | `gates.yaml` 把确定性信号种类声明为门禁 | 1 | 未实现 | 搜 `gates.yaml`、`deterministic` 无命中 |
| FR-124 | 完成只认门禁全过或人在 App 确认 | 1 | 部分实现 | devkit 任务只认检查（task_finish）；委托的人工确认（`/delegations/{id}/confirm`）；缺：基于门禁声明的通用判定 |
| FR-125 | 承诺弱化生成 `active` 的 `core.weakening` | 1 | 未实现 | 搜 weakening 无命中；`TrackRecord.weakened` 恒为 None（`delegation_view.rs:436`） |
| FR-129 | 关注卡 6 段固定格式，≤100 字 | 1 | 部分实现 | 委托卡、验收卡有结构；通用关注卡只有标题（`AttentionList.svelte:14`） |
| FR-131 | 打开项目先给"自上次查看以来"3 行摘要 | 1 | 未实现 | 搜 since-last-visit、上次查看、last_visit 无命中 |

### 4.3 第 2 阶段

| ID | 一句话 | 阶段 | 状态 | 证据或缺口 |
| --- | --- | --- | --- | --- |
| FR-120 | 扩展清单 `towermd-plugin.yaml`，核心只读清单 | 2 | 未实现 | 搜 `towermd-plugin`、manifest 无命中 |
| FR-123 | 扩展不能用 human 署名、不能改人定对象和 gates | 2 | 未实现 | 没有扩展机制 |
| FR-137 | 汇报请求过期生成 `core.silent`，连续 2 次升级 | 2 | 部分实现 | `delegation_service.rs:73-80` 对检查点请求有 `report.overdue`（passive 后 active）；缺：通用请求和 `core.silent` 名称 |
| FR-126 | 兜圈子检测 `core.stuck` | 2 | 未实现 | 搜 stuck 无命中；`TrackRecord.loops` 恒为 None |
| FR-127 | 对任意节点提问，转成 request_report | 2 | 未实现 | 搜 `answers:` 声明无命中 |
| FR-128 | 跨来源唯一出口，active 超 3 条时按序 | 2 | 部分实现 | `runtime.ts:88-127` 合并 alerts、holds、proposals、delegation 后取前 3；缺：v0.7 排序规则与信号来源 |
| FR-130 | 打扰时机 3 档：立即、下次查看、摘要 | 2 | 未实现 | 搜 `time_sensitive`、notification 无命中 |
| FR-134 | 跟进项（≤5，到点条件） | 2 | 未实现 | 搜 follow、跟进 无命中 |
| FR-135 | 超过 `dormant_after` 进入休眠 | 2 | 未实现 | 搜 dormant 无命中 |
| FR-136 | 休眠对象重新浮现 | 2 | 未实现 | 搜 `review_every` 无命中 |
| FR-183 | 想法与承诺分开，"立项"是人的动作 | 2 | 未实现 | 没有线索或未立项状态；搜 lead、立项 无命中 |
| FR-184 | 委托是人的动作，必须先有完成标准，委托卡 5 栏 | 2 | 已实现 | core `delegation.rs`（Brief：why、done_when、limits、checkpoints、self_check）；`TaskCreate.svelte`；`tests/delegation_cli.rs:233` |
| FR-201 | 托付前澄清：复述、起草完成标准、≤3 问 | 2 | 已实现 | `/delegations/{id}/clarify`、`finalize`（`daemon.rs:412-413`）；`ClarificationPanel.svelte` 及测试；devkit `delegation_tests.rs` |
| FR-211 | 检查点生成汇报请求，超时升级 | 2 | 已实现 | `delegation_service.rs:52,73-80`；`delegation_projection.rs`；测试 |
| FR-208 | 逐条验收：每条完成标准对证据 | 2 | 已实现 | `AcceptanceCard.svelte`；`delegation_view.rs` 测试（542-683） |
| FR-209 | 证据 3 类：check、claimed、used | 2 | 已实现 | `SourceKind`（core delegation）；`/delegations/{id}/used`（`daemon.rs:414`） |
| FR-210 | 待验收积压超过 `review_backlog` 时提示 | 2 | 已实现 | `AttentionList.svelte` 积压行；`delegation_view.rs` backlog |
| FR-141 | 布局模型的 3 项配置 | 2 | 未实现 | 搜 container、frame、layout 配置无命中（画布只有任务图） |
| FR-142 | 模板以 `type: template` 清单声明 | 2 | 未实现 | 没有模板机制 |
| FR-144 | 3 种一步创建容器的方式 | 2（文本导入在 3） | 未实现 | 没有容器 |
| FR-145 | 容器默认尺寸，位置和大小只由人改 | 2 | 未实现 | 没有容器 |
| FR-146 | 容器规则随容器版本化，改规则是人的动作 | 2 | 未实现 | 没有容器 |
| FR-147 | Agent 用 CLI 操作容器，先校验 | 2 | 未实现 | `args.rs` 没有 frame、container 子命令 |
| FR-148 | 容器的 3 种模式 | 2 | 未实现 | — |
| FR-149 | 卡片位置由模板推导，稳定增量布局 | 2 | 未实现 | 没有 elkjs 等布局实现（`package.json` 只有 `@xyflow/svelte`） |
| FR-150 | 人改过的字段，Agent 只能提建议 | 2 | 未实现 | — |
| FR-151 | 标题 ≤30 字、正文 ≤100 字，超出显示 "+N" | 2 | 未实现 | — |
| FR-152 | 卡片状态取模板状态集；完成类状态遵守 FR-124 | 2 | 未实现 | — |
| FR-153 | 模板声明 `default_set_by` | 2 | 未实现 | — |
| FR-154 | 存储按归属分开 | 2 | 未实现 | — |
| FR-156 | 容器可以作为 `target` | 2 | 未实现 | — |
| FR-157 | 容器时间滑块 | 2 | 未实现 | 搜 slider、时间滑块 无命中 |
| FR-158 | 默认高亮"自上次查看以来"的变化 | 2 | 未实现 | — |
| FR-160 | 旁注 3 种，≤100 字 | 2 | 未实现 | 搜 callout、旁注 无命中 |
| FR-161 | 回答、插话、决策针对卡片时自动生成旁注 | 2 | 未实现 | — |
| FR-162 | 容器生命周期 `update_until`，到期冻结 | 2 | 未实现 | — |
| FR-163 | 模板声明 `source: file` 加解析器 | 2 | 未实现 | 搜 `source: file`、parser 无命中 |
| FR-164 | 文件、画布或 CLI 两条写入路，结果相同 | 2 | 未实现 | — |
| FR-165 | 元素身份以文本中的稳定标识为准 | 2 | 未实现 | — |
| FR-166 | 人定文件的改动转提案 | 2 | 未实现 | — |
| FR-167 | 文件和画布同时改时以文件为准 | 2 | 未实现 | — |
| FR-168 | 元素位置由核心布局决定，人拖过的存 canvas | 2 | 未实现 | — |
| FR-169 | 时间滑块同时用 git 历史和事件流 | 2 | 未实现 | — |
| FR-174 | 解析报告：已理解、看不懂、理解比例 | 2 | 未实现 | — |
| FR-175 | 解析失败时显示上次成功的结果并锁定编辑 | 2 | 未实现 | — |
| FR-176 | 只读区域的编辑被禁用 | 2 | 未实现 | — |
| FR-177 | 文本源问题信号默认 passive 并合并 | 2 | 未实现 | — |
| FR-178 | 解析器和渲染器限大小、限时、不联网 | 2 | 未实现 | — |
| FR-143 | 自带起步模板：看板、四象限、时间线、思维导图、PlantUML | 2（附录 A.1） | 未实现 | 没有模板 |
| FR-189 | 起步套装：推荐模板、检查和显示名 | 2（附录 A.1） | 未实现 | — |
| FR-170 | PlantUML 映射规则 | 2（附录 A.2） | 未实现 | 搜 plantuml 无命中 |
| FR-171 | 两种显示可切换 | 2（附录 A.2） | 未实现 | — |
| FR-172 | 图种按阶段支持 | 2（附录 A.2） | 未实现 | 引用"6.2"节号有误（歧义 8） |
| FR-173 | 模板自带 Agent 规则 | 2（附录 A.2） | 未实现 | — |
| FR-179 | 第一方 PlantUML Lint 扩展 | 2（附录 A.2） | 未实现 | — |
| FR-80 | 回写 Note.md 的摘要是 OKF 纯 `.md` | 2（附录 A.3） | 未实现 | 搜 notemd、note_md、捕获目录写入 无命中 |

### 4.4 第 3 阶段

| ID | 一句话 | 阶段 | 状态 | 证据或缺口 |
| --- | --- | --- | --- | --- |
| FR-133 | 多项目总览 | 3 | 未实现 | App 只开单个项目；搜 overview、全部项目 无命中 |
| FR-185 | 委托级别按"配置 × 工作类别"设置并继承 | 3 | 部分实现 | `/delegation-levels`（`daemon.rs:420`）、TrustLedger UI 记录和建议；缺：级别对行为的实际约束（设计文档的取舍） |
| FR-202 | 信任账本：表现、过程、越界 | 3 | 部分实现 | 表现类已有（`delegation_view.rs` TrackRecord）；缺：`loops`、`weakened` 恒为 None（`:436`），`out_of_bounds: None`（devkit `delegation_kit.rs:48`） |
| FR-203 | 受托者配置指纹（自报） | 3 | 已实现 | core `delegation.rs` fingerprint；`delegation_tests.rs`；v0.7 之前的运行不计入 |
| FR-204 | 按比例抽查，不降到 0 | 3 | 已实现 | `delegation_view.rs` spot checks；测试 |
| FR-205 | 纠正变规则 | 3 | 已实现 | `/delegations/{id}/send-back`、`/rules`（`daemon.rs:416,418`；`delegation_service.rs:156`） |
| FR-206 | 验收后的一句话保存为决策 | 3 | 已实现（偏差） | 一句话存在确认事件里，不是独立的决策文件（歧义 6） |
| FR-207 | 工作类别由人命名、可改名合并 | 3 | 已实现 | `/work-kinds/rename`（`daemon.rs:419`）；测试 |
| FR-186 | 决策 `review_on` 复查 | 3 | 未实现 | 搜 review_on 无命中 |
| FR-139 | 扩展提供 ≤7 行变化摘要 | 3 | 未实现 | 没有扩展机制 |
| FR-140 | 固定视觉词汇：约 16 个图标、4 档、3 种连线 | 3 | 未实现 | 只有任务角标 ⛔ ? ! ▶ ✓ ○（`runtime.ts`）；词汇未定义（开放问题 6） |
| FR-159 | 模板声明时间派生视图 | 3 | 未实现 | — |
| FR-155 | 容器导出为 Markdown、Mermaid、PlantUML | 3 | 未实现 | — |

### 4.5 基础设施

| ID | 一句话 | 阶段 | 状态 | 证据或缺口 |
| --- | --- | --- | --- | --- |
| FR-86 | Cloudflare hub：Worker 加每项目一个 SQLite DO | 基础设施 | 未实现 | `hub-worker/` 不存在 |
| FR-88 | 断网时普通事件进发件箱，恢复后按 eid 补发；租约必须在线 | 基础设施 | 部分实现 | core `durable_outbox.rs` 及测试；`code.rs:21` 有 `hub_offline` 错误码但没有代码路径；缺：远端 hub 和真实的断网补发 |

### 4.6 未排期

| ID | 一句话 | 阶段 | 状态 | 证据或缺口 |
| --- | --- | --- | --- | --- |
| FR-03 | 变更等级：主要、次要、豁免 | 未排期 | 部分实现 | 主要变更走提案；缺：次要变更登记和带到期时间的豁免（搜 waiver、豁免 无命中） |
| FR-182 | 两种起头方式，都不选场景 | 未排期 | 部分实现 | 只有 devkit 冷启动扫描；缺：一句话起头 |
| FR-33 | 进度只算已完成，`claimed_done` 不算 | 未排期 | 已实现 | devkit 任务只由检查完成；委托由人确认；自述完成显示为待验收 |
| FR-56 | 门禁和检查结果写成证据，绑定对象版本 | 未排期 | 部分实现 | devkit 完成证据绑定提交；缺：核心层通用的 `kind: gate/check` 证据 |
| FR-72 | 人工输入不能绕过人定内容的批准和硬门禁 | 未排期 | 已实现 | 没有绕过路径；豁免出口缺失归 FR-03 |
| FR-69 | 临时规则：带范围和到期时间 | 未排期 | 未实现 | ProcessRule 被拒绝（question_service） |
| FR-70 | 输入面板快捷命令 | 未排期 | 未实现 | 搜 slash、`/report` 无命中 |
| FR-42 | 首屏默认时间窗为"自上次查看以来" | 未排期 | 未实现 | 同 FR-131 |
| FR-73 | 中断图标在任务、容器、项目节点的显示规则 | 未排期 | 部分实现 | 任务节点有带文字标签的角标；缺：容器和项目的聚合图标与计数 |
| FR-44 | 推进状态只用填充色、角标、细微动画表达 | 未排期 | 部分实现 | 角标已有；缺：健康色填充和工作中的动画 |
| FR-45 | 拟议变更虚线叠加 | 未排期 | 未实现 | 搜 dashed、虚线 无命中 |
| FR-46 | 新版本 300–800 ms 过渡和幽灵层 | 未排期 | 未实现 | 搜 ghost、transition 无相关命中 |
| FR-50 | 同对象 1 秒内的事件合并为一次更新 | 未排期 | 未实现 | 没有 1 s 合并或节流 |
| FR-39 | 上层只显示聚合值 | 未排期 | 部分实现 | `runtime.ts` moduleActivity 计数；缺：容器和项目层的完整聚合字段 |
| FR-36 | 关系类提案高亮两端对象 | 未排期 | 未实现 | 没有找到关系高亮代码 |
| FR-79 | 引用 Note.md 时 `sources` 记 OKF id 加块引用 | 未排期 | 未实现 | 搜 block_ref、Note.md 来源 无命中（建议并入 P2-K） |
| FR-138 | 批准、回答、确认能在关注卡上一步完成 | 未排期 | 未实现 | `AttentionList.svelte:14` 只有"打开"按钮（建议并入 P2-E） |
| FR-132 | 位置是记忆的锚，不自动移动节点 | 未排期 | 已实现 | 非草稿不可拖动；运行时位置不写 canvas |
| FR-180 | 文件能解析就不阻止"乱用" | 未排期 | 不适用 | 原则；随 P2-J 落实 |
