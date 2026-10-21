---
type: Design
title: 代码按核心与开发套件拆成两个 crate，命令行落地 C0 契约
description: 依据 2026-10-21 的核心、开发套件、命令行三份规格重新调整代码框架：A 拆出 towermd-devkit crate 并以编译期 Kit trait 挂到核心；B 在新结构上实现命令行 C0（信封、错误码、请求 ID、语言、schema、按 FR-200 重构命令树）。
generated: { by: claude-code/opus-5.5, at: 2026-10-21T00:00:00Z }
sources:
  - id: core
    resource: ../../specs/2026-10-21-spec-towermd-core.md
    title: Tower.md 核心规格 v0.6
  - id: devkit
    resource: ../../specs/2026-10-21-spec-towermd-development-kit.md
    title: Tower.md 开发套件规格 v0.6
  - id: cli
    resource: ../../specs/2026-10-21-spec-towermd-cli.md
    title: Tower.md 命令行规格
  - id: owner-1021
    resource: 2026-10-21 用户在对话中的决定
    title: 本轮范围为 A+B；crate 结构选双库加 Kit trait；其余设计由 Claude 作为架构师与 PM 自行决定
status: draft
stale_after: 2027-01-21
---

# 代码按核心与开发套件拆成两个 crate，命令行落地 C0 契约

## 0. 范围

本轮做两件事，按顺序：

- **A 结构**：把与场景无关的核心和软件开发专属的开发套件拆成两个库 crate，开发套件经编译期 `Kit` trait 挂到核心上[^core-0.1][^devkit-5.6]。
- **B 命令行 C0**：在新结构上实现命令行规格的 C0 阶段：统一信封、错误码与退出码、请求 ID、语言、`schema`/`help`，并按 FR-200 重构命令树[^cli]。

不在本轮：

- 核心命名与数据模型迁移（`set_by`、四档信号级别、名词改名、`layer` 降为内部字段），即核心规格第 1 阶段，单独立项。
- `AGENTS.md` 产品不变量的改写（"人的动作只能从 App 发出""L0–L2""只认证据"）。要等你确认，并且只在 C3 签名实现时才需要改。
- C1–C4 的新命令、`completion`（需要新依赖 `clap_complete`，按 `AGENTS.md` 另行确认）、Secure Enclave 签名。

用户在 2026-10-21 定下 A+B 范围和 crate 结构，其余设计授权 Claude 决定[^owner]。

## 1. 依据与关键判断

1. **代码层的扩展边界由本设计补上。** 规格只对外规定了接入接口（FR-114–119）和扩展清单（FR-120），没有说第一方开发套件在 Rust 里怎样挂到核心上。本设计采用编译期 trait。第三方扩展以后仍按清单加接入接口接入，这一点不变。
2. **开发套件是第一方扩展，需要用到内部接口。** 规格说"扩展只能经接入接口使用核心"（核心规格 0.1 节），这是对第三方的约束。开发套件需要用到租约、门禁、事件投影，走 HTTP 自调既慢，也会破坏"一处定序"的进程内语义。所以开发套件经 `Kit` trait 进入核心，crate 依赖方向仍然只能是 devkit → core。
3. **核心的存储继续用 git。** 项目即仓库、权威版本以 `main` 为准、快照写入 git、hub 放在 `git-common-dir` 下（FR-83、FR-85、FR-91、FR-103），这些都是核心条款。所以 `repo.rs`、`snapshot`、`main_writer`、`main_inspection`、`main_resolution` 留在核心。核心不认的是分支 `t/<id>`、工作树、模块、G1–G5、harness，这些归开发套件。
4. **YAGNI。** `Kit` trait 只放这一轮真正有调用方的方法。检查和门禁的注册、中断类型的注册，等出现第二个 Kit 时再抽象。
5. **行为不变。** A 完成后对外行为与 v0.1.1 一致，测试数不减；B 的改动只限于命令行规格明确写出的变化。

## 2. A：crate 拆分

### 2.1 目标布局

```text
crates/towermd-core/     与场景无关：okf、config、types（注册表）、canvas、project_view、
                         catalog（人定对象目录与受保护路径）、path_guard（人定内容路径守卫）、
                         hub、hub_journal、hub_protocol、durable_outbox、events、
                         snapshot、snapshot_publication、repo、main_writer、
                         main_inspection、main_resolution、resolution_projection、
                         project_runtime、project_state（核心 reducer）、
                         question_model、question_projection、proposal_model、
                         project_controls、task（核心 Task）、init（核心部分）、kit（trait）
crates/towermd-devkit/   开发专属：verifier、cold_start、cold_start_verification、
                         task_workspace（t/<id>、工作树、ModuleScope、HarnessSource）、
                         task_finish（G0/G2/G3/G5）、task_alerts、baseline、
                         baseline_publication、draft_store、review、review_verification、
                         types（开发对象）、reducer（开发事件）、init（开发目录与模板段落）、schema/
crates/towermd/          二进制：main.rs（clap 入口）、daemon.rs（生命周期与路由组装）、
                         cli/（核心命令与核心服务）、dev/（`towermd dev` 命令与开发服务）、
                         output/（信封、错误码、文本渲染、语言）
```

依赖关系：`towermd → towermd-devkit → towermd-core`；`src-tauri` 同时依赖 core 和 devkit，因为它用到了 `review`。

`path_guard` 和 `catalog` 保护的是人定内容，属于"人定内容保护"这条核心底线，所以留在核心。规格里的"G2"指的是开发套件用模块路径去调用这个守卫，名字归开发套件，机制归核心。

**模块依赖的实测（2026-10-21）**：核心的 `project_state` 依赖 `task_finish`、`task_alerts`、`task_model`；`task_finish` 依赖 `verifier`、`cold_start`。这一组如果在 A1 搬走，会形成 core → devkit 的反向依赖，所以只能在 A2 解开 reducer 之后再搬。

### 2.2 Kit trait

```rust
// crates/towermd-core/src/kit.rs
pub trait Kit: Send + Sync + 'static {
    /// 扩展 ID，例如 "dev"；事件与对象类型按此命名空间登记。
    fn id(&self) -> &'static str;
    /// 加入唯一类型注册表的对象类型。
    fn object_types(&self) -> &'static [TypeSpec];
    /// 处理核心不认识的事件；不认识就返回 Ignored。必须是纯函数，便于重放。
    fn reduce(&self, state: &mut ProjectState, event: &Event) -> Result<Reduced, String>;
    /// `towermd init` 时补充目录与 `.towermd/AGENTS.md` 段落。
    fn init_project(&self, ctx: &InitContext) -> Result<(), String>;
}
```

- 注册表仍然只有一处（FR-82），位置在核心的 `types::Registry`，由核心类型加各 Kit 类型组成。
- `ProjectState` 留一个按 Kit ID 分区的扩展槽（`extensions: BTreeMap<String, serde_json::Value>`），开发套件把自己的投影放在 `"dev"` 下。快照照常序列化整个 `ProjectState`，所以重放和重建路径不变。
- 守护进程启动时组装 `Kits = [DevKit]`，把它传给 `ProjectRuntime`。

### 2.3 两步走

**A1 机械搬家（行为不变）。**

- 新建 `towermd-devkit` crate，把核心里没有其他模块依赖的开发模块整个移过去：`baseline`、`baseline_publication`、`draft_store`、`review`、`review_verification`。只改 `use` 路径，内联测试和 `tests/` 跟着一起移；`proposal_model_tests` 里对 `baseline_publication` 的引用改为测试数据，或者把这条测试移到 devkit。
- 二进制 crate 的服务文件按归属分到两个目录：
  - `cli/`：task、question、proposal、event、project_control、main_resolution、business、human_task、run_details、project_service
  - `dev/`：baseline、finish、scan 相关
- 验收标准：`cargo test --workspace` 和 `src-tauri` 测试全部通过，测试函数总数与搬家前相等，并把两个数都记录下来。

**A2 解开混在一起的模块。** 每解开一个模块，先写失败测试，再改代码。

| 模块 | 拆法 |
| --- | --- |
| `types.rs` | 核心登记 Intent、Decision、Rule、Proposal、Task、Run、Evidence、Hold；Module、Interface、Baseline、Acceptance、Gate、SkillPack 由 DevKit 登记 |
| `hub.rs` | 写死的 `module:` 租约前缀和暂停规则改成通用的租约键（`lease_key: String`），hub 不再知道"模块"；`git:` eid 校验留在核心，因为它属于存储层 |
| `project_state.rs` | 核心 reducer 处理通用事件（hold、prompt、alert、lease、snapshot、recovery、task 的创建/领取/释放）；`task.merged`、`task.finish.*`、`baseline.*`、"只有通过 G3 的证据才能记 G5"移进 devkit reducer |
| `task_model.rs` | 核心 `task`：id、标题、完成标准、状态、受托者、租约；devkit `task_workspace`：分支、工作树、ModuleScope、HarnessSource |
| `question_model.rs` | 记住的决策改为按通用 `scope: String` 存，devkit 填入模块 ID；旧数据读取时，把 `module` 字段映射成 `scope` |
| `init.rs` | 核心负责 `.towermd/config.yaml`、`docs_dir`、`tower.canvas`、AGENTS 模板的通用段落；devkit 负责 `intents/`、`arch/`、`specs/`、`baselines/`、`decisions/` 目录和开发规则段落 |

A2 的顺序：先拆 `project_state` 的 reducer 和 `task_model`，切断核心对 `task_finish`、`task_alerts` 的依赖；然后把 `task_finish`、`task_alerts`、`verifier`、`cold_start`、`cold_start_verification` 搬到 devkit；最后处理 `types`、`hub`、`question_model`、`init`。

A2 的护栏测试放在 `crates/towermd-core/tests/boundary.rs`：扫描核心 `src/` 的源码，`worktree`、`module:`、`G3`、`G5`、`task.merged`、`harness` 这些词必须零命中（注释也算）。actor 格式在核心里改称 `<producer>/<version>`，与 OKF 一致。`G2` 不在禁用词表里，因为核心的 `path_guard` 就是它的机制。

### 2.4 兼容

- 快照和 hub 日志中现有的事件类型名和字段都不变，v0.1.1 的数据可以直接重放。DevKit reducer 接手的事件，类型名保持原样。
- `.towermd/` 和 `docs/tower/` 的磁盘格式不变。

## 3. B：命令行 C0 契约

### 3.1 输出信封（FR-191、FR-192）

```json
{"v":1,"ok":true,"data":{},"delivered":[],"next":[{"cmd":"towermd ...","why":"..."}],"warnings":[]}
{"v":1,"ok":false,"error":{"code":"lease_busy","message":"...","hint":"...","retry":{"safe":true,"request_id":"..."}},"delivered":[],"warnings":[]}
```

- 新增模块 `crates/towermd/src/output/`：
  - `envelope.rs`：`Envelope`、`ErrorBody`
  - `code.rs`：`ErrorCode` 枚举，它到退出码的映射是唯一来源
  - `render.rs`：文本渲染
  - `lang.rs`：语言选择
- 退出码：0 成功、1 内部错误、2 用法错误、3 找不到、4 冲突、5 还在等、6 被拦下、7 需要人、8 连不上。
- 守护进程的 HTTP 错误统一返回 `{code, message, hint}`；命令行负责把它映射成退出码。
- 每次调用只输出一个信封；stdout 上不再出现多段 JSON。

### 3.2 输出模式与语言（FR-197）

- `--json` 或 `--text` 显式指定；都没给时，stdout 是终端（`std::io::IsTerminal`）就用文本，否则用 JSON。
- 语言选择顺序：`--lang` → `TOWERMD_LANG` → `LC_ALL` → `LANG` → macOS `defaults read -g AppleLanguages` → `en`。只支持 `en` 和 `zh-CN`。
- 文本渲染按"错误信息 + 通用的数据摘要 + 送达项 + 下一步"输出。C0 只为 `status` 写专用渲染，其余命令用通用渲染。
- JSON 字段、错误码、命令名永远是英文。

### 3.3 送达（FR-194）

- 把 `deliver_messages` 改成：先取回待送达的消息和复核提示，放进信封的 `delivered`。
- 信封整体写入 stdout 并 flush 成功之后，才逐条 ack，即记 `prompt.delivered`。写入失败就不 ack，下次调用重新送达。

### 3.4 请求 ID（FR-193）

- 全部写命令接受 `--request-id`。没有给时，命令行生成 UUIDv7，并在 `error.retry.request_id` 和成功信封的 `data.request_id` 中带回。
- 守护进程以请求 ID 派生事件 `eid`，依靠 hub 现有的 eid 幂等（FR-87），同一 ID 重试时返回第一次的回执。还没有走 eid 的写端点在计划中逐一列出补齐，每个都有"重试返回同一结果"的测试。

### 3.5 命令树（FR-200，不兼容 M1）

| 现在 | C0 之后 |
| --- | --- |
| `check`、`finish [--wait]`、`trailers <task>` | `dev check`、`dev finish [--wait]`、`dev trailers <task>` |
| `baseline scan [--wait]`、`baseline show`、`baseline cancel` | `dev scan start`、`dev scan wait <id>`、`dev scan show <id>`、`dev scan cancel <id>` |
| `baseline template`、`baseline submit`、`baseline list` | `dev draft template <id>`、`dev draft submit --json <文件\|->`、`dev draft ls` |
| `baseline schema` | `schema dev.draft.submit` |
| `project view` | `show <项目>`（C0 只支持当前项目） |
| `ask --wait <hold>` | `wait <id>` |
| `answer <hold> <text>`（转述） | `answer --relayed <hold> <text>`；不带 `--relayed` 返回 `needs_human`（退出码 7），C3 再开放 |
| `pause --all`、`resume --all` | `pause all`、`resume all`；resume 仍然只是请求，由 App 确认 |
| `hub rebuild`（请求 + App 确认） | 名字不变，行为不变，到 C3 再改为一步完成 |
| `task add <json>`、`propose <json>` | `task add --json <文件\|->`、`propose --json <文件\|->`，输入上限 2 MiB，超出返回 `too_long` |

- 下列命令名字不变，只是改用信封输出：`status`、`task claim/show/release`、`snapshot`、`init`、`up`、`daemon ensure/status/stop`、`wait`。
- 新增：
  - `schema [命令]`：输出该命令输入输出的 JSON Schema（FR-199）。
  - `--version`。
  - 全局参数 `--project`（C0 只校验它与当前项目是否一致）、`--quiet`。
  - `help` 用 clap 自带的。
- JSON Schema 手写，放在 `crates/towermd/schema/<命令>.json`，用 `include_str!` 编进二进制。不引入 `schemars` 或 `jsonschema`。测试保证每个命令都有 schema，且每个 schema 都是合法 JSON，并带有 `$id` 和 `type`。

### 3.6 同步改写

以下都在 B 的同一批改动里完成：

- 技能：`towermd-finish`、`towermd-baseline`、`towermd-ask`、`towermd-claim`，`skills/README.md`。
- 核心与开发套件 `init` 生成的 `.towermd/AGENTS.md` 模板。
- `README.md` 中的命令示例。
- `src-tauri/src/backend.rs`：`daemon ensure` 的输出改为从信封的 `data` 里读取。
- `CHANGELOG.md`、`CHANGELOG.zh-CN.md`："未发布"区写明命令行破坏性变更和新旧命令对照，放在版本节最前面。
- 原有的 CLI 集成测试（`crates/towermd/tests/*_cli.rs`）改成新的命令名和信封断言。

## 4. 错误处理

- 守护进程连不上返回 `daemon_unavailable`，退出码 8，`retry.safe=true`。
- 未初始化返回 `not_initialized`，退出码 3。
- 租约冲突返回 `lease_busy` 或 `stale_epoch`，退出码 4。
- 路径守卫拦下返回 `guard_blocked`，门禁失败返回 `gate_failed`，都是退出码 6。
- 输入解析失败或超长返回 `usage` 或 `too_long`，退出码 2。
- 无法归类的错误返回 `internal`，退出码 1，`message` 保留原文。不允许把"无法归类"映射成 0。

## 5. 测试与验收

| 阶段 | 验收 |
| --- | --- |
| A1 | 全部测试通过，测试数与搬家前相等；`cargo tree -p towermd-core` 中不出现 `towermd-devkit` |
| A2 | 边界护栏测试通过；v0.1.1 生成的 hub 日志和快照重放结果与搬家前一致（用现有的 replay 测试加一个固定样本） |
| B | 每个错误码有退出码测试；送达"写出后才 ack"的测试；请求 ID 重试幂等的测试；每个命令都有 schema 的测试；语言选择顺序的单元测试；技能文档中出现的每条 `towermd` 命令都能被 clap 解析（测试会扫描 `skills/**/SKILL.md`） |
| 收尾 | `pnpm test`、`pnpm check`、`cargo test --workspace`、`src-tauri` 测试全部通过；App 实机启动一次，确认 `daemon ensure` 能正常解析新信封 |

## 6. 风险

- **A2 拆 `task_model` 和 `project_state` 时最容易改坏重放。** 对策是先用固定样本锁住投影结果，再动代码。
- **B 是破坏性改动，外部脚本和技能会失效。** 用户已经接受"不兼容旧命令"；CHANGELOG 把这一点放在最前面。
- **改动面大。** A1、A2、B 分别作为独立提交批次，每批结束时都要求全绿，必要时可以单独回退。

[^core-0.1]: 核心规格 0.1 节：核心、扩展、用户用法三层，依赖只能从下往上。
[^devkit-5.6]: 开发套件规格 5.6 节：一个与平台无关的核心 crate，其余都是适配层；适配器接口实现为 Rust trait。
[^cli]: 命令行规格 3 节、5 节、FR-190–200。
[^owner]: 2026-10-21 对话：范围选 A+B；结构选"双库 + Kit trait"；用户表示"我没主意，你是个专业的架构师和PM，请自己决定"。
