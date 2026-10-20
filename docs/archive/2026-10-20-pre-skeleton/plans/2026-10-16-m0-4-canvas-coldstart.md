---
type: ImplementationPlan
title: M0-4 canvas 编解码、对象加载与冷启动准备计划
description: 在不新增依赖、不实现 M1 App 批准或 Tauri UI 的前提下，准备可重建的候选画布与 S1–S5 输入，并明确实际执行和基线生效的边界。
status: complete
generated:
  by: codex/gpt-6
sources:
  - id: spec
    resource: /docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: 工作树内规格 v0.5，6.5、8.1–8.2、15.1、16、18 节
  - id: m01
    resource: /docs/superpowers/plans/2026-10-16-m0-1-core-objects-init.md
  - id: m02
    resource: /docs/superpowers/plans/2026-10-16-m0-2-local-hub-daemon.md
  - id: m03
    resource: /docs/superpowers/plans/2026-10-16-m0-3-snapshot-recovery.md
  - id: local-contracts
    resource: 本机 /Users/bruce/git/tower.md/AGENTS.md 与 tasks/lessons.md；本轮用户提供的仓库指令
  - id: okf
    resource: 本机 /Users/bruce/git/note.md/docs/okf-v0.2-format-constraints.md
  - id: canvas-design
    resource: 本机 /Users/bruce/git/note.md/docs/superpowers/specs/2026-09-04-infinite-canvas-design.md，第 3.1、7.1–7.2 节
  - id: writing-plans
    resource: 本机 /Users/bruce/.codex/.tmp/plugins/plugins/superpowers/skills/writing-plans/SKILL.md
  - id: vault-history
    resource: 本机 /Users/bruce/git/sotvault/project/towermd/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: 知识库旧版 v0.3，仅作需求溯源，不覆盖工作树规格
  - id: vault-layout
    resource: 本机 /Users/bruce/git/sotvault/research/2026-10-16-hemory-layered-situation-view-fast-slow-layers.md
    title: 第 4.5 节，快层反馈与慢层布局变更
---

# M0-4 Canvas 与冷启动 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 提供安全、宽容的 `tower.canvas` 编解码，按权威提交加载对象，产出可复算的候选模块图、固定初始坐标及 S5 验证计划；不把候选结果变成已批准基线。

**Architecture:** 业务逻辑放在 `towermd-core`，复用 M0-2 的 `Catalog`、OKF 和 Git 读取；daemon 提供只读查询及冷启动 prepare 请求，CLI 只传参和输出。已批准画布与候选画布是不同返回值，所有派生结果绑定同一次读取冻结的提交与 tree。

**Tech Stack:** Rust stable / edition 2021；现有 serde、serde_json 1、serde_yaml 0.9、sha2/hex、clap 4、axum/tokio；测试沿用 tempfile。`Cargo.toml`、`Cargo.lock` 不需变更，不引入 elkjs、TOML/YAML 新解析器、图算法库或沙箱库。

## 1. 本轮交付与实施前提

此计划由有界文档子任务产出，随后由主代理审定并继续实施；用户的实际要求是持续推进，并非“仅计划、不写代码”。以下复选框跟踪未来实施任务。执行采用精确文件、输入输出断言与红绿步骤，不创建源码提交；S5 实跑保留为明确缺口，不以准备模型代替全部 M0 验收。

读取位置为 `/tmp/towermd-m0-3`（实际路径 `/private/tmp/towermd-m0-3`），HEAD 为 `2a39a20e680a8995f1e9c2b260bcbbffff6c8148`。该工作树没有根 `AGENTS.md`、`tasks/lessons.md` 或 docs 下的嵌套 AGENTS；已读取用户提供的规则和主工作区对应文件。M0-1/2 按既有验收记录已完成；主代理验证 M0-3 快照/恢复集成测试已通过，正在独立 review，共享接口接入以 review 收敛后的版本为准。

规格读取时 SHA-256 为 `64381c367bf7fc13e7f2bb863fbf469dd0722637f4566cba3381e3fc0e435517`。实施前核对差异，尤其是 6.5、8.1、18 节；这是输入指纹，不是发布证据。代码与计划存在未提交修改，不以 HEAD 代替当前工作树内容。

独立的 codec 和纯计算任务可先实施；共享 `catalog.rs`、`repo.rs`、`daemon.rs` 的接入等 M0-3 稳定后进行。不得扩大 M0-3 的快照路径白名单，也不得借快照写入器提交 canvas 或基线。

## 2. S1–S5 与人工批准的边界

2026-10-16 用户已明确选择：本轮先交付画布编解码、候选模块扫描和测试计划，S5 实跑留待隔离运行器就绪；不得据此声称整个 M0 完成。该范围现已确认，不再重复询问。

### 2.1 三类行为必须分开

1. **读取 / prepare：** 读固定 Git 提交，计算候选模型、验证命令和草稿字节。没有新增慢层对象生效，不要求 App 人工批准。用户对开发工作的持续授权足以继续这些工作。
2. **执行验证：** S5 真正启动仓库程序，可能写构建缓存、启动子进程或访问外部资源。它不是纯读取，也不是基线批准。规格未写“S5 每次必须 App 批准”，不能自行把它扩大成新产品闸门；但本计划也不能把“发现命令”当作“已运行”或当作建立长期验证规则的批准。
3. **慢层生效：** 采纳模块划分、接口、验证契约和坐标，写入 `main` 并打 `towermd/B0`，必须由 App 人工批准后交给持主干租约的 daemon。CLI、Agent、prepare 结果、OKF `verified` 和聊天中的开发授权均不能代替该产品流程（FR-95、FR-111）。

| 阶段 | M0-4 的具体输出 | 是否只读 / 派生 | App 边界与本计划状态 |
| --- | --- | --- | --- |
| S1 文件盘点 | 固定提交的跟踪文件、源码分类、清单与候选验证命令及来源 | 是；只读 manifest/script/CI 文本，不执行 | 不需 App 批准；未知脚本结构给诊断 |
| S2 依赖图 | 目录包含关系与文件归属图 | 是；M0 不解析 import | 不需批准；目录关系不冒充真实调用依赖或 Interface |
| S3 候选模块 | 路径分区、稳定候选 ID、置信度理由、未划分区 | 是；候选不是 L1 已生效对象 | 不需批准；调整权威模块仍走 S7/S8 |
| S4 历史耦合 | 固定历史窗口的共变统计、热点、候选分区修正 | 是；只读 Git history | 不需批准；不能因新提交、任务事件自动移动已有基线坐标 |
| S5 准备 | 每模块验证计划、命令来源、待执行状态；无命令时列补测试候选任务 | 是；不 spawn、不追加 Evidence、不创建 Task | 不需批准；本计划可完整交付此部分 |
| S5 实际执行 | 针对固定 tree 的通过/失败/超时、退出码和日志证据 | 否；存在执行副作用 | 当前缺运行隔离契约与验证器接入，明确未实现；不得用伪结果补齐 M0 验收 |
| S6 模型综合 | 模块职责、接口、风险、意图草稿 | 本计划不实现 | 属 M1 技能；不启动 harness，不实现 `baseline submit` |
| S7 画布审阅 | 合并、拆分、改名、拖动、确认验证命令后的草稿 | 涉及人编辑慢层草稿 | 属 M1 App；草稿编辑本身不生效，纯布局也需批准 |
| S8 批准 B0 | 与批准内容一致的慢层提交、基线标签 | 持久写入与生效 | 必须 App 人工批准，M0-4 无生产入口；不能做 `--yes`、`--approve` 或自动超时通过 |

### 2.2 S5 缺口的依据与停止点

已用 `notemd search --vault /Users/bruce/git/sotvault` 检索“冷启动”“分层态势”“只读工作树”“验证命令”，并读取命中的旧版规格和分层态势研究。旧规格仍写 S5“只读工作树运行测试”、S7“确认验证命令”，没有说明如何隔离任意测试命令，也未给出 S5 新增 App 批准的决定。旧文档含过时的无头运行器与里程碑安排，不能覆盖当前 v0.5。

据此，本计划的保守实施边界是 **S1–S4 + S5 prepare + 只读模型**，保留 S5 实跑缺口，而不是宣称已完成规格全部 S1–S5。需要后续补齐的工程契约：固定提交的临时执行树、允许写入的构建输出目录、超时与进程树回收、命令及依赖环境、日志保存与不可变证据接入。单纯 `git worktree --detach` 或事后检查 diff 不能证明执行只读；未找到已落实的隔离能力时不自动执行候选命令。

如果后续决定把发现的命令登记为慢层验证契约，须在 S7/S8 由 App 确认并批准；如果已有符合约束的验证器和既有执行授权，可另行接入 S5，无须凭空添加“每次执行都批准”。本任务不向用户发问，不替用户作上述未有依据的产品决定。

## 3. 复用点与文件职责

| 文件 | 现状与建议改动 |
| --- | --- |
| `crates/towermd-core/src/okf.rs` | 已有 `Document::parse/to_text`，不改动时保留原文；读取未知 type，写入 type 经登记。复用，只有被本计划测试证明的加载问题才修改 |
| `crates/towermd-core/src/catalog.rs` | 已有 `Catalog::load`，fetch 后固定 revision、读配置与对象；已有重复 ID、layer 与保护路径校验。补路径索引和固定 revision 的读取接口，不另造宽松授权目录 |
| `crates/towermd-core/src/repo.rs` | 复用 Git 环境隔离与超时；只在确有需要时补“不裁剪 blob 字节”的读取接口，不重写 M0-3 Git 写路径 |
| `crates/towermd-core/src/canvas.rs`（新增） | JSON Canvas 文档、原文、不透明条目、诊断、编码；不依赖 filesystem/hub/UI |
| `crates/towermd-core/src/canvas_view.rs`（新增） | 固定提交的画布/对象关联、引用文档、悬空、未摆放与只读投影 |
| `crates/towermd-core/src/coldstart.rs`（新增） | S1–S4 的确定性计算、S5 计划、覆盖率、候选 canvas；不执行验证命令 |
| `crates/towermd-core/src/lib.rs` | 仅导出新增模块 |
| `crates/towermd/src/daemon.rs` | 接入查询与 prepare；沿用项目定位，不建立第二个定序源，不触发快照或基线提交 |
| `crates/towermd/src/main.rs` | 增加 `baseline scan`，输出 JSON；不提供批准、submit 或写文件参数 |
| `crates/towermd-core/tests/canvas.rs`（新增） | codec 兼容性、不可安全编辑输入、原文往返 |
| `crates/towermd-core/tests/canvas_view.rs`（新增） | 固定提交、对象关联与路径安全、快层不改变序列化布局 |
| `crates/towermd-core/tests/catalog.rs` | 对现有加载器的针对性回归，保留任务授权原行为 |
| `crates/towermd-core/tests/coldstart.rs`（新增） | 可手算的小型 Git 仓库、分区/历史/覆盖率/S5 prepare、重建等价 |
| `crates/towermd/tests/coldstart_cli.rs`（新增） | daemon/CLI 契约、无写入与无批准入口；只使用隔离 runtime 和临时仓库 |

## 4. API 与数据契约建议

以下名称是建议的新增接口，不声称当前已存在；错误风格沿用 `Result<T, String>`，面向人提示用中文。小型记录可放在所属模块，不为这些接口创建通用框架。

### 4.1 Canvas 编解码

- `CanvasDocument::decode(text)` → 文档与诊断。文档保留完整输入原文、按原顺序排列的节点/边及已知字段投影；四类节点为 text/file/link/group。根 `nodes`、`edges` 均可缺省，缺省不得被误判成格式错。
- `CanvasDocument::encode()` → 原文或确定性生成的 JSON。未改动导入文档逐字节返回原文，包括 CRLF、空白、未知字段、不透明条目、缺省字段。新建候选画布只生成合法字段；本阶段不提供导入文档的通用编辑 API。
- `CanvasDiagnostic` → 稳定诊断 code、节点/边索引、可读原因；不透明条目保留所在数组位置，不静默丢弃、截断、修复或补 ID。
- 已知几何限定整数像素及后续前端可精确表示的安全整数范围；允许负 x/y，width/height 必须大于 0。越界/分数/非法必填字段进入不透明投影；非法可选字段保留原值并报诊断，不擅自补默认值到文件。
- 根/节点/边的未知键及嵌套 `towermd` 扩展均保留。重复节点 ID、重复边 ID 不用后者覆盖前者；关联产生歧义诊断。节点与边各有命名空间，不把跨集合重名误判为格式错。
- 重复 JSON property name 用 serde 自定义读取 visitor 检出，整文件只读保留原文；不能依赖 `serde_json::Value` 的 last-wins 行为。高精度未知数字不经过 f64 再输出；无法安全建立数值投影时保留原文、给只读诊断。
- 非法 JSON、非对象根、nodes/edges 非数组不生成正常可用画布；返回文件诊断及原始内容，绝不以空画布覆盖。编码不可编辑导入文档只允许原文透传。
- M0-4 的“编解码”覆盖原文读取/透传与新建候选编码。带未知数字 token 的任意局部编辑及安全写回需要保留 token/span 的编辑模型，留给有编辑需求的后续任务；不能用普通 Value 重序列化声称完整实现 Note.md 编辑器的无损写回。

FR-100 与 FR-102 不冲突：输入已存在的未知 `viewport` 等键按原文保留，并报告不消费；**产品运行时**的 viewport、selection、hover、快层健康色、费用、角标、任务图从不作为编码输入。既有 `color` 是合法静态样式，不拿健康色覆盖它。

### 4.2 固定提交的对象加载与关联

- `Catalog::load` 继续决定权威版本；建议增加 `Catalog::read_document_at(path)`，只读本次 `revision` 中的普通 blob。已有内部 tree/blob 方法优先复用，不接受另一个客户端指定的“权威 ref”。
- `ObjectEntry`：对象 ID、repo-relative path、`Document`；在现有对象表旁提供 path→entry 索引，保留已有 Task 服务依赖的严格校验。
- `CanvasView::load(catalog)` → `revision`、`tree`、canvas 存在性、文档、关联结果、诊断、未摆放模块与引用文档。一次请求内配置、canvas、对象必须来自同一 revision；扫描中 main 前进不混读。
- `NodeBinding` 区分 object / reference / dangling / unsafe / ambiguous。file 节点的 `towermd.obj` 与文件路径中的对象 ID 必须一致；按 ID 找到另一个路径不能掩盖路径错误。边的 `towermd.obj` 尝试关联 Interface；`arch/interfaces.yaml` 不能被当成 OKF Markdown，当前缺其专用加载契约时返回接口未解析诊断。
- FR-101 的未摆放只含有效 Module 对象；纯 README 引用即使没有 frontmatter 也可阅读，不因此加入对象登记。未知 `type` 的 OKF 文档可以读取、往返和展示，不假装它一定是 Module。缺可选字段、无 `verified`、裸 mapping 的 `verified` 均不能导致拒读；若展示验证列表，仅在派生视图中把裸 mapping 视为一项，不回写原文。
- 对象加载排除 `<docs_dir>/tower.canvas`、`state/`、`artifacts/`、`.towermd/` 及其 AGENTS；保留名 `index.md`、`log.md` 不按普通对象解析。其他 YAML 如 baselines、interfaces 需要专用加载器，不泛化成 Markdown。
- 语法损坏、重复 ID、非法 layer 维持当前明确错误，不能为“宽容展示”削弱任务授权安全检查。无 ID 的合法 OKF 文档可以作为普通引用，不能伪造对象 ID。
- file 路径相对**仓库根**，不是 docs_dir 或 canvas 所在目录。规范化 POSIX `.`/`..` 后不得逃出根；拒读绝对路径、盘符/UNC、NUL、反斜线歧义；保留输入并诊断。按提交树读取，不跟随 symlink blob、不进入 submodule 或外部 URL。合法含空格/换行路径按 NUL 分隔解析，不 trim；非 UTF-8 明确报错。

### 4.3 冷启动 prepare

- `prepare_coldstart(catalog, options)` → `ColdStartPreparation`；options 仅含可复现扫描参数，不含 `approve`、`actor`、输出路径或任意 shell 命令。
- `ColdStartPreparation`：`project_id`、`revision`、`tree`、算法版本、参数、S1–S5 阶段状态、files、modules、history、coverage、unassigned、validation_plans、candidate_tasks、candidate_canvas、diagnostics。`state = prepared`、`requires_app_approval = true` 表达未来基线生效要求，不能表明已有审批请求或批准事件。
- `CandidateModule`：稳定候选 ID、目录/文件路径、可复算成员列表、confidence 与 reasons、验证计划引用、仅文字 landmark（若有）。没有自动推断的意图、正式接口或虚构的批准人。
- `ValidationPlan`：module ID、候选命令的 argv 或未解析原文、repo-relative cwd、来源路径、固定 tree、执行状态和原因。只有不需猜 shell 语义的候选才能结构化成 argv；其余标记需要确认，不通过拼接字符串隐式执行。
- `ValidationState` 在本任务中只有 `not_run`（发现命令但未执行）、`no_tests_discovered`（在已声明识别范围内未发现）、`unsupported`（读取到但不能可靠识别）。三者都不是通过/失败证据；`no_tests_discovered` 附“无测试（扫描未发现）”和补测试候选任务。不能把未执行显示为无测试。
- 候选 canvas 的 file 节点指向将来的模块文档路径，`towermd.obj` 引用候选 ID；同一响应内附候选对象内容供预览解析。尚不存在的文件只在候选集合内解析，不混进已批准对象目录。用 group 表示未划分区，不把共变统计画成已知接口边。
- 不落盘候选文档，不建立 refs，不创建 Task/Evidence、不触发 hub 事件；结果可丢弃并从同一提交和参数重建。需要持久化冷启动中间产物时，由 daemon 按 FR-75–77 接入 Arca/本地忽略目录，这是另一个明确的缺口，不把 git-common-dir 缓存作为唯一证据库。

### 4.4 daemon / CLI

| 建议入口 | 请求与响应 | 边界 |
| --- | --- | --- |
| `GET /canvas` | 沿用 daemon 项目定位方式，返回权威 `CanvasView`；没有 canvas 时明确 `missing`，不生成并落盘一个空画布 | 不接收客户端 canvas，不读取工作树替代 main |
| `GET /objects/:id` | 返回同一 Catalog 的路径、原始 OKF 文本及 revision；未知 ID 返回明确不存在 | 不是任意文件读取接口 |
| `POST /baseline/coldstart` | 仅接受 prepare 请求，返回 `ColdStartPreparation` | 明确不启动 S5，不申请 main 写租约、不写 hub 状态；未知执行/批准字段拒绝 |
| `towermd baseline scan` | 由 cwd 定位项目，经 daemon 调用 prepare，stdout JSON | stderr 放诊断；不用 stdout 提示交互，不提供 submit/approve/write/output 参数 |

prepare 成功只证明准备完成；coverage 不足、S5 未执行必须在响应中可见，不把 HTTP 成功或 CLI 退出 0 当作里程碑完成。沿用 FR-113 及现有运行上下文处理，不新增改变任务状态的副作用；已有绑定续租如由公共入口发生，应在测试中单独核对，不把它误算成基线写入。

## 5. 可复算算法与固定布局

这些是 M0 的工程建议，不是新的用户产品决定。算法版本和参数进入 prepare 输出，便于替换时比较；不修改 spec。

1. **冻结输入。** 复用 Catalog 取得权威 main 的 commit/tree/config。S1 的“git ls-files”语义是跟踪文件盘点：对固定 tree 用已有 `git ls-tree -rz` 等价读取，不依赖调用者 index。所有 blob、历史截止点、候选清单都绑定该 tree。
2. **源码分母。** 列出全部 tracked entries，再以明确后缀集合识别源码：Rust、TS/JS（含 tsx/jsx/mts/cts/mjs/cjs）、Svelte、Python、Go、C/C++/头文件、Java/Kotlin、Swift、Ruby、Shell。`.git`、`.towermd`、配置 docs_dir 下的状态/产物不算源码；tracked vendor/generated/build 目录是否排除必须按清单逐项展示，不能为了达标隐藏未分配源码。不识别后缀单列，不能声称覆盖所有语言。
3. **目录图。** 目录路径是顶点，目录包含文件/子目录是边；按 POSIX 路径排序。模块初值取顶级源码目录，`src`/`crates` 等容器只有一个候选时按下一层源码目录细分；根级源码作为一个显式 root 候选。3–9 是建议值，小仓库不硬凑三组，大仓库不把所有文件塞进一个组骗覆盖率。
4. **历史窗口。** 固定 commit 起向父方向取最多 200 个非 merge 提交，每提交的改动文件集合去重；只统计仍在当前源码清单中的路径。大于 100 个源码文件的批量提交不参与共变对计数，单列原因与数量；热点仍可单独报告。用有限 Git 超时，浅历史/空历史标为 unavailable，不虚构零耦合。
5. **历史修正。** 按候选模块汇总共变提交次数；建议仅在共变次数至少 3 且“共同出现次数 / 两模块任一出现次数”至少 0.8 时提出合并，按分数降序、路径字典序消歧；只接受不重叠的合并对，且不把原本至少三组降到三组以下。保留修正前/后分区和统计理由。没有满足阈值的证据则保持目录分区；不自动拆成正式接口。
6. **置信度。** 用可解释的 low/medium/high 候选等级及具体理由，避免伪概率：历史不足/未识别命令时 low；目录完整且无冲突时 medium；满足历史修正规则的合并可为 high。它只表达分区建议，不映射为健康色或已验收。
7. **覆盖率。** `assigned_unique_source_files / tracked_source_files`，同时输出两个整数、未划分文件及排除清单。多模块归属视为分区错误，不能重复计数；分母为 0 时比率为 null、状态 not_applicable，不能报 100%。真实验收要求分母大于 0 且比率至少 0.95。
8. **候选 ID。** 对规范化模块根/合并成员排序键使用现有 sha2/hex 生成稳定 ID；保留完整映射，检测截断哈希冲突后确定性扩长。禁止依赖随机 UUID、当前时钟或目录遍历偶然顺序。
9. **布局。** 仅在生成全新候选布局时，按稳定模块键排序，建议每行三列、卡片 320×180、水平/垂直间距 80，坐标从 (0,0) 开始；未划分区单独一行。group 放在被包含节点前，不改变 z-order。该固定网格是 M0 无新依赖的预览初值，非 elkjs 实现或已批准坐标。
10. **已存在布局。** `/canvas` 只取 main 坐标。重新 scan 可返回独立 candidate_canvas，但不修改原图。快层事件、任务增减、健康/费用变化、悬空诊断、未摆放托盘及读请求都不得触发重排。FR-43/44/48/102 的不变量用字节和坐标断言验证；M1 UI 过渡动画不在本计划内。

## 6. TDD 实施清单

每个测试先定义具体夹具与期望，再单独运行确认失败原因，最小实现后重跑通过。新增模块首次允许以缺少导出导致的编译失败进入红灯；接口建立后必须观察行为断言失败，不把零测试匹配当成功。每一步只做一件事；一组案例可逐条执行，不要求一次实现整张表。

### Task 1：codec 原文与正常条目

**文件：** 新增 `crates/towermd-core/src/canvas.rs`、`crates/towermd-core/tests/canvas.rs`；修改 `crates/towermd-core/src/lib.rs`。

- [ ] 写 `unmodified_canvas_is_byte_identical`：输入包含 text/file/link/group、负坐标、edge、根/节点/边未知嵌套键、CRLF 和无结尾换行；断言 encode 的 bytes 完全相等，节点/边顺序不变。
- [ ] 运行 `cargo test -p towermd-core --test canvas unmodified_canvas_is_byte_identical -- --exact`，应恰有 1 项失败；记录是缺接口还是字节丢失。
- [ ] 实现原文保留与只读已知字段投影，空对象和缺省 arrays 也能读取，不物化默认 edge 端点值。
- [ ] 重跑上述命令，预期 1 passed；增加 `generated_canvas_uses_only_document_fields`，断言新生成画布四类条目有效、整数几何、没有 runtime 字段且重复编码相同。
- [ ] 先观察新增断言红灯，再完成新建候选编码，运行 `cargo test -p towermd-core --test canvas`，全部通过。

### Task 2：不透明条目与不可安全写回输入

**文件：** `crates/towermd-core/src/canvas.rs`、`crates/towermd-core/tests/canvas.rs`。

- [ ] 写 `invalid_entries_keep_position_and_raw_content`：在两个正常节点之间插入未知 type、标量、缺 ID、宽为 0、分数坐标的条目；断言原位置保留、对应诊断、往返原文不变。
- [ ] 运行 `cargo test -p towermd-core --test canvas invalid_entries_keep_position_and_raw_content -- --exact`，观察 1 项失败；实现不透明投影后重跑为 1 passed。
- [ ] 写 `duplicate_properties_disable_projection_without_data_loss`：根与嵌套对象分别有重复 key；断言文件只读诊断、原文可取，不采用最后值覆盖后的普通画布。
- [ ] 对该名称运行同一 test target 的 `--exact` 红绿循环；补 `large_unknown_numbers_round_trip_verbatim`，用大于 u64 的整数、高精度小数、指数形式检查原 token 未损失。
- [ ] 加 `invalid_root_never_becomes_empty_canvas` 与 `optional_invalid_fields_are_not_repaired`：语法坏、根数组、错误 nodes 类型、非法 side、重复 ID；断言错误类别、保留原内容、不伪造空图、不写盘。
- [ ] 运行 `cargo test -p towermd-core --test canvas` 全绿；核对缺省/显式默认、两个 ID namespace、group z-order 均有断言。不能安全支持的输入必须诊断只读，不能直接删除未知字段让测试通过。

### Task 3：复用 Catalog 加载并关联对象

**文件：** 新增 `crates/towermd-core/src/canvas_view.rs` 与 `crates/towermd-core/tests/canvas_view.rs`；按需修改 `catalog.rs`、`repo.rs`、`lib.rs` 和 `tests/catalog.rs`。

- [ ] 写 `view_reads_one_authoritative_revision`：临时 origin/main 中放配置、canvas、Module，任务分支及脏工作树放不同内容；返回值必须全来自 fetch 后同一 SHA。另测无远端的本地 main。
- [ ] 运行 `cargo test -p towermd-core --test canvas_view view_reads_one_authoritative_revision -- --exact`，确认红灯；最小复用 Catalog 的固定 revision 读取后重跑通过。
- [ ] 写 `bindings_report_unplaced_dangling_and_mismatch`：A1 有文件节点，A2 无节点，A3 缺对象，README 无 frontmatter，另一节点 obj=A1 却指向 A2；分别断言已绑定、未摆放、悬空、普通引用和不一致诊断，canvas 原文不变。
- [ ] 对该测试完成红绿循环；补 `loader_keeps_unknown_okf_and_skips_non_objects`：未知 type 和裸 verified mapping 可读，state 中坏 Markdown、canvas JSON、AGENTS、index/log 不误解析；OKF 原文往返完全相等。
- [ ] 写 `file_references_cannot_escape_commit_tree`：`../outside.md`、绝对路径、反斜线、symlink、submodule 不可读取；根内正规化路径与含空格路径正确。外部哨兵内容不能出现在响应中。
- [ ] 完成上述红绿后运行 `cargo test -p towermd-core --test canvas_view` 与 `cargo test -p towermd-core --test catalog`；重复 ID、坏 layer、受保护路径原有测试保持通过。

### Task 4：S1–S3 盘点、分区与覆盖率

**文件：** 新增 `crates/towermd-core/src/coldstart.rs`、`crates/towermd-core/tests/coldstart.rs`，修改 `lib.rs`。

- [ ] 写 `inventory_is_bound_to_tree_not_worktree`：固定 tree 含源码、README、清单、配置、产物与未知后缀；调用工作树另有 staged/unstaged/untracked 文件。断言只统计固定 tree，并逐项列出分类与排除原因。
- [ ] 运行 `cargo test -p towermd-core --test coldstart inventory_is_bound_to_tree_not_worktree -- --exact`，确认失败；实现 NUL 分隔的 Git 清单读取与明确语言分类后重跑通过。
- [ ] 写 `directory_partition_is_stable`：三个源码目录、根源码、嵌套目录；打乱输入次序仍产生相同模块/成员/候选 ID，所有路径归属可解释，模块根不能越界。
- [ ] 对上述测试完成红绿循环；写 `coverage_counts_unique_files_and_reports_remainder`：100 个源码，95 个分配、5 个未划分得到 0.95；94 个得到 0.94；重复归属报错；空源码比率 null。
- [ ] 对覆盖率测试完成红绿循环，运行 `cargo test -p towermd-core --test coldstart`；未达到 95% 时保留失败数据，不通过改变分母掩盖。

### Task 5：S4 历史修正与候选固定布局

**文件：** `crates/towermd-core/src/coldstart.rs`、`crates/towermd-core/tests/coldstart.rs`、`crates/towermd-core/tests/canvas_view.rs`。

- [ ] 写 `history_correction_is_bounded_and_reproducible`：临时仓库建四个模块，三次只共同修改 a/b，另有 c/d 独立修改与一次超大提交；断言 a/b 达到阈值被建议合并，c/d 不误合并，批量提交不膨胀共变计数，理由可手算。
- [ ] 运行 `cargo test -p towermd-core --test coldstart history_correction_is_bounded_and_reproducible -- --exact`，确认红灯；实现固定窗口和稳定消歧后重跑通过。
- [ ] 写 `missing_history_preserves_directory_candidates`：浅历史或无有效共变数据，分区保持 S3 结果，history 状态 unavailable，不写假零矩阵或高置信度。
- [ ] 完成红绿循环；写 `candidate_layout_is_deterministic_and_separate`：相同 commit/参数连续两次与丢弃模型后重建，候选 JSON 完全相等；节点位置满足固定整数网格，未划分区存在，既有 main canvas bytes 未变。
- [ ] 完成红绿循环；在 `canvas_view` 写 `fast_state_cannot_change_layout_bytes`：对同一慢层视图施加不同任务数、健康/费用和中断输入，比较前后全部节点 x/y/width/height 与 codec 输出 bytes 相等，快层数据只存在独立测试叠加模型中。
- [ ] 运行 `cargo test -p towermd-core --test coldstart` 和 `cargo test -p towermd-core --test canvas_view`，全部通过。此处只证明模型不变量，不声称已验证 UI 动画或渲染性能。

### Task 6：S5 prepare 与不能执行的明确结果

**文件：** `crates/towermd-core/src/coldstart.rs`、`crates/towermd-core/tests/coldstart.rs`。

- [ ] 写 `validation_discovery_never_executes_scripts`：package.json 的 test 脚本会创建哨兵文件，CI/Makefile 中也有有副作用的文本；扫描只读内容，哨兵不存在，结果含来源与 not_run，进程启动测试替身的计数为 0。
- [ ] 运行 `cargo test -p towermd-core --test coldstart validation_discovery_never_executes_scripts -- --exact`，确认红灯；以现有 JSON/YAML 读取和保守文本识别实现发现，不引入 TOML parser、不执行脚本或 package manager。
- [ ] 重跑为 1 passed；写 `no_tests_and_not_run_are_distinct`：有明确 test 脚本→not_run；已支持格式且无测试入口→no_tests_discovered 加候选补测试任务；复杂未解析 Make/TOML→unsupported，不宣称通过或无测试。
- [ ] 完成红绿循环；写 `preparation_creates_no_evidence_or_task`，比较调用前后 hub 业务对象与 journal，不能出现 Evidence/Task/approved/baseline.created；已存在的外部结果也不自动当成该 tree 的证据。
- [ ] 运行 `cargo test -p towermd-core --test coldstart` 全绿；明确阶段报告为 S5 prepared/not_run，不能把这项测试通过写成“S5 跑现有测试已完成”。

### Task 7：daemon/CLI 只读接入与拒绝越界

**文件：** `crates/towermd/src/daemon.rs`、`crates/towermd/src/main.rs`、新增 `crates/towermd/tests/coldstart_cli.rs`。M0-3 共享改动稳定后开始。

- [ ] 写 `scan_returns_preparation_without_publishing_baseline`：在隔离 runtime 下调用 `baseline scan`，断言 JSON 的 revision/tree、candidate_canvas、coverage 与 S5 状态；比较所有分支、标签、草稿 refs、index、工作树与保护目录，均未被请求改变。
- [ ] 运行 `cargo test -p towermd --test coldstart_cli scan_returns_preparation_without_publishing_baseline -- --exact`，确认红灯；接入 core prepare，复用 daemon 项目定位，不注册 snapshot 调度器作为读请求副作用，然后重跑通过。
- [ ] 写 `read_endpoints_do_not_create_missing_canvas`：GET canvas/objects 只能读取权威版本，missing 保持 missing；损坏画布返回诊断；两次请求之间 main 改变时各自有明确 revision，不静默混合对象。
- [ ] 完成红绿循环；写 `cli_and_api_cannot_approve_or_execute`：CLI 的 approve/submit/--yes/--execute-tests/--actor human:x 均拒绝；prepare 请求夹带批准、执行或输出路径字段也拒绝，不产生 human 事件。不能通过伪造客户端类型触达不存在的批准路由。
- [ ] 完成红绿循环；覆盖两个项目与 linked worktree 的定位、读取失败与超时、非交互 stdin、stdout 关闭错误。运行 `cargo test -p towermd --test coldstart_cli` 全绿。

### Task 8：真实仓库验收与最终边界审查

**文件：** 复用以上测试文件；真实验收结果在未来执行记录中留存，本轮不创建产物或修改其他文档。

- [ ] 用 `~/git/note.md` 的固定提交作真实验收输入，记录 commit/tree 和语言分类。因该仓库未必初始化 Tower.md，在临时 clone 内制作必要配置夹具；不得在原仓库 init、改文档、提交或执行其测试脚本。清楚区分原源码 commit 与临时配置提交。
- [ ] 运行未来实现的 `towermd baseline scan`，记录分母、分子、排除、未划分、各模块理由与 S5 not_run；要求已跟踪源码覆盖率至少 95%，否则改进分类/分区并重验，不能手工把结果改成达标。
- [ ] 丢弃内存派生模型，从同一 commit/参数重新扫描；除明确不参与模型比较的观测耗时外，结果一致，candidate_canvas bytes 一致。因为本方案不建新 SQLite 索引，不伪称完成“新增索引删除重建”测试。
- [ ] 运行 `cargo test --workspace --locked`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked`。要求全部退出 0；M0-3 并发开发造成的失败单独记录，不能偷偷修复或宣称全绿。
- [ ] 审查变更只涉及计划实施文件、依赖锁未变、无批准或受保护路径写入入口；所有测试在专用临时仓库中提交夹具，不对真实仓库提交。按用户当前要求，交付仍标“未提交”。
- [ ] 最终报告分列“codec/read/prepare 已验收”与“S5 实跑、App 审阅/批准、B0 提交标签未实现”；不把本子任务成功写成 M0 总体验收通过，不声称 S1–S6 的 15 分钟或 Z0 渲染指标已验证。

## 7. 需求映射与保留缺口

| 需求 | 可验收覆盖 | 本计划不实现的部分 |
| --- | --- | --- |
| FR-07/81/82、OKF | Task 3：宽容读、原文往返、排除非对象、严格授权目录不变 | 不扩充 type 注册，不凭 verified 授权 |
| FR-83 | Task 3/7：固定权威 main，提交树读取 | 不把任务分支或脏工作树当权威 |
| FR-100 | Task 1/2：四类节点、整数几何、顺序/未知字段/不透明输入、原文往返、生成合法候选 | 导入文档任意字段编辑后的 token 级无损写回不在此阶段 |
| FR-101 | Task 3：对象关联、普通引用、悬空与未摆放派生模型 | App 托盘/诊断 UI；interfaces.yaml 专用语义加载尚无契约，不伪造接口 |
| FR-102、FR-43/44/48 | Task 5/7：快层不改布局、不进入编码、候选与正式画布分开 | Tauri、Svelte、Z1 自动布局、拖动与 300–800ms 过渡属于后续 UI |
| S1–S4、FR-20/21、8.2 覆盖率 | Task 4/5/8：目录分区、历史修正、置信度理由、未划分区、真实仓库 ≥95% | M2 import/Louvain；规格 8.1 的“G4 覆盖率”与第 11 节 G4 评审编号不一致，本计划以 8.2 数值验收，不修改 spec |
| S5、初始证据 | Task 6：验证计划、无测试候选任务、真实 not_run/unsupported 状态 | 真执行、隔离、超时回收和不可变证据仍是完成完整 M0 的缺口，不能用测试替身当验收证据 |
| FR-75–77 | prepare 结果绑定输入，可重算；不新增唯一持久状态 | Arca/本地忽略目录的扫描产物归档尚未接入，不能宣称持久草稿或证据归档已实现 |
| FR-95/111、S7/S8 | Task 7：无批准入口、无 human 伪署名、无写 main/打标签 | App 审阅、批准内容哈希核对、持租约唯一写入、B0 属 M1 |
| 18 节 M0 | 可生成并返回候选 `tower.canvas` 内容，能读取既有画布 | 若验收要求持久草稿文件/草稿 ref，尚需 daemon 草稿存储协议；不能直接写 `<docs_dir>/tower.canvas` 替代。S5 未实跑时整个 M0 仍未完成 |

本计划没有用“App 尚未存在”作为绕过人工批准的理由，也没有把所有计算步骤都加上人工确认。准备模型可继续自动推进；执行能力缺失如实返回未运行；只有真正改变慢层基线的路径必须等待 App 批准实现。

## 8. 本次文档审查记录

- 已对照工作树规格 6.5、8.1–8.2、15.1、16、18 节与本地 OKF、Note.md canvas 约束，区分事实、接口建议和实现缺口。
- 已核对现有 Catalog/Document/daemon 接口，不另起第二套对象权威或 Git 写入口；未承诺 M0-3 在开发接口已稳定。
- 已按 writing-plans 自查需求映射、接口名一致性和各任务的红绿断言；没有将不可实现部分写成空占位或已完成事项。
- 最初规划子任务只产出本文件；实施进度与实际测试名称以以下执行记录为准。前述测试名为规划示例，未机械勾选为已执行命令。

## 9. 执行记录（2026-10-17）

- [x] Codec：18 项测试通过，涵盖原文、未知字段、不透明条目、重复键、大数和候选输入拒绝运行时字段。
- [x] 只读视图：5 项测试通过，涵盖固定 main、原文引用、悬空/类型错误、换行路径、symlink 拒绝、缺失/损坏画布与不重排。
- [x] 冷启动准备：10 项测试通过，涵盖固定提交、覆盖率、共变分区、浅克隆降级、路径不重叠、未执行脚本、CI 命令与未知清单诊断。质量审查发现的 CI cwd 等价写法丢命令、未识别 Make/Cargo 被误报无测试均已红绿修复，专项与独立复现复审 PASS。
- [x] daemon/CLI 接入：10 项专项测试通过，涵盖不创建 hub/画布/证据、拒绝越权参数、现有运行续租、固定 main 及多项目读取。规格审查复现的“读取触发其他运行过期”已修复：核心 `tasks::renew_binding` 仅续租有效绑定，不调用全局 expire，三条读取路径均有回归；另有过期隐式绑定不写事件、过期显式绑定拒绝的核心测试。质量审查发现复制整个 hub 后可在其他 clone 续租，已补持久项目绑定核对与回归；未注册项目只检查已有绑定，不新建绑定或快照任务。
- [x] 真实 CLI：Note.md 原源码 `f2bdc01b2bfd0bf9f9129e144e550360aab444b8` 的临时 clone，配置夹具提交 `715ce69cb579d06b98f47c6693cd6febdc603a0f`、tree `aeba1ff7e15fc841dc7a0b1d81053b304c51eebf`；两次 `baseline scan` JSON 字节一致，1952/1952 已识别源码、25 个候选模块，S5 为 `not_run`。原仓库未改；临时 clone 无 hub、画布或工作树改动，临时 daemon 已停止。
- [x] 独立规格复审与质量审查收敛；core 与 adapter 均 PASS，所发现问题已逐项红绿修复并由原审查者复验。
- [x] 合入已审查的 M0-3 基础改动并同步主工作区；主工作区最终 320 项完整测试、fmt、Clippy（-D warnings）、build 全部通过。最终主工作区 CLI 对真实仓库夹具重复扫描仍字节一致，index 与工作树未变，S5 为 not_run。

未提交、未发布；S5 实跑、App 审阅/批准、B0 提交与标签均未实现，不宣称整个 M0 完成。
