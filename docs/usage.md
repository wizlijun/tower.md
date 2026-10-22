# Tower.md v0.1.1 使用说明

本文是 v0.1.1 预览版（M1）的使用说明，原为仓库首页。项目远景见 [README](../README.md)。命令行已按[命令行规格](specs/2026-10-21-spec-towermd-cli.md) C0 重构，本文的命令名已随之更新；其余内容描述的仍是 v0.1.1 的画布与流程。

## 当前可用范围

当前源码提供**画布、冷启动审阅与批准、任务执行证据和人工放行的最小框架**。默认主界面只有画布，详情与操作按需打开。模块结构来自已提交的 JSON Canvas 与对象；任务、问题、禁止和门禁失败通过本机 WebSocket 更新，快层变化不移动节点。

App 通过随包 CLI 连接本用户唯一的本机守护进程。扫描与验证使用公共 Git 目录中的缓存和独立工作树，不改用户工作树或暂存区；基线经 App 明确批准后，才由守护进程出版到 main 和 Git 标签。初始化使用 CLI，Claude Code/Codex 等会话由用户自行启动，Tower.md 提供技能与命令协作。

上述 M1 主路径已完成真实 App 验收：Claude Code 与 Codex 在三个工作树完成十项任务，另验证了 B1 批准、旧任务复核、主干处置、Hub 恢复及恢复后重新领取并完成任务。详见[验收报告](qa/2026-10-21-m1-live-acceptance.md)。本次为 **macOS Apple Silicon 最小预览**，无发行者签名或公证；Intel 运行、NFR-03 定量性能和指定 Note.md 仓库的 M0 验收尚未完成，M2/M3 不在本次范围。

### 先看功能框架

从 [0.1.1 预览发行页](https://github.com/wizlijun/tower.md/releases/tag/v0.1.1) 下载 Apple Silicon ZIP，解压后打开 `Tower.md.app`，也可将它移入“应用程序”。本机只需启动一份 App；随包 CLI 位于 `Tower.md.app/Contents/MacOS/towermd`。真实项目需要本机 Git，以及项目验证命令所需的 Node.js、Rust 等工具。

打开 App，选择“打开功能示例”，无需选择仓库。示例用最简单的模块、基线、任务、提问、验证和放行组件串起流程：点击卡片打开操作，确认示例基线、领取任务、回答问题、选择模拟验证结果，最后放行或退回。

示例状态只保存在当前窗口的内存中，不启动 Agent、不运行命令、不写 Git。示例画布明确标注“示例数据”；模拟通过或放行不代表真实证据、基线批准或任务合并。具体展示与流程可在试用组件后调整。真实仓库功能仍通过“打开项目”使用。

## 启动与构建

macOS 开发环境需要 Git、Rust stable、Xcode Command Line Tools、Node.js，以及 `package.json` 固定的 pnpm 11.7.0。在本仓库根目录运行：

```sh
pnpm install
pnpm app:dev
```

`app:dev` 会先编译随包 CLI，再启动 Vite 与桌面窗口。原生目录选择需要桌面 App；单独运行 `pnpm dev` 的浏览器页面不具备本机项目读取能力。

构建可独立打开的 App：

```sh
pnpm app:build
```

默认生成 `src-tauri/target/release/bundle/macos/Tower.md.app`。若设置了 `CARGO_TARGET_DIR` 或 Cargo 的目标输出目录，产物位于对应目录的 `release/bundle/macos/`。此构建不要求发行者签名或公证；macOS 工具链可能生成临时 ad-hoc 签名。

## 如何查看项目

1. 打开 App，点击“打开项目”，或按 `Cmd+O`（其他平台为 `Ctrl+O`），选择 Git 仓库。取消选择会保留当前项目。
2. 已配置且提交了 `tower.canvas` 的项目会显示节点与关系；拖动空白处平移，滚轮或触控板捏合缩放。
3. 点击节点查看原文，按 `Esc` 关闭详情。空白处右键打开菜单，可刷新、适应画布、查看项目态势或切换项目；键盘也可用 `Shift+F10` 打开菜单。
4. “项目态势”显示读取来源、完整提交号、文档目录和读取诊断。已提交但未出现在画布上的 Module，可从“未摆放模块”查看。
5. 任务等快层状态自动更新；断线期间保留上次画布并在详情提示重连。需要重新读取仓库画布时，使用右键“刷新”或 `Cmd/Ctrl+R`。

### 从源码生成候选画布

已配置的项目可在空画布或右键菜单点击“扫描并验证”。目录扫描完成后先显示候选模块图，验证完成后更新卡片状态，保持节点位置和视口。点击卡片查看实际结果与 Module 原文，右键“扫描说明”查看覆盖率、固定提交、历史共同修改关系、命令退出码和日志摘要。关系线表示共同修改，不代表已确认的接口。

扫描实现 spec 的 S1–S5 冷启动切片。模块划分仍是目录启发式，可经下面的 S6 综合补充语义，再交人审阅；只有完成审阅并在 App 明确批准后才出版 B0，扫描本身不批准。卡片分别显示通过、失败、无测试、超时、取消或未运行；“无测试”表示未发现可执行的验证命令，并附补测建议，不是验证通过。

右键“返回当前画布”可回到 main。“刷新”也会返回 main；原候选仍固定于旧提交，只有再次扫描才会生成新候选。刷新、切换项目或关闭窗口不取消后台任务；需要停止时选择“取消本次验证”，并等待清理结果。刷新后可从菜单恢复进度跟踪。

验证在 `<git-common-dir>/towermd/verify/<scan-id>/worktree` 的分离工作树运行，结束后删除该工作树。执行继承启动 daemon 时的环境，允许网络，不自动安装依赖；分离工作树不是安全沙箱，命令使用当前用户权限。通过 CLI 启动 daemon 时可显式传入所需 PATH、工具链和凭据；App 沿用已经启动的服务环境。

同一 daemon 同时只运行一批验证，同一命令由关联模块共享、只执行一次。默认每条命令最多 2 分钟，整批最多 10 分钟；超时或取消回收整个进程组。报告绑定 commit/tree，包含 `process:towermd-verifier` 署名。命令改变已跟踪源码、HEAD 或 index，以及无法确认清理完成时，均不能报告通过。

报告与 Module 文件位于 `<git-common-dir>/towermd/drafts/<scan-id>/`；日志位于 `towermd/verify/<scan-id>/logs/`，相对公共 Git 目录。每条日志最多保留 1 MiB，界面最多显示开头 4 KiB，并明确标识存储截断；报告不枚举环境变量，命令输出会进入本机日志。重启可读取完成报告，日志缺失或摘要改变时拒绝沿用。删除缓存后须重新扫描并实际执行，不能恢复原来的执行证据。daemon 意外退出时，独立 worker 通过管道关闭请求取消并清理；未完成的扫描重启后显示中断，不自动重跑或变为通过。

退出服务可运行 `towermd daemon stop`，它会先取消活动验证。升级后若发现旧版服务，会明确提示先停止旧服务再重试，不自动启动第二个 daemon。

### Agent 综合 → 持久草稿

S5 完成后，让现有 harness 会话中的 Agent 使用仓库的 [towermd-baseline 技能](../skills/towermd-baseline/SKILL.md)，只读固定提交的源码与文档，进行一次综合。Tower.md 不要求云账号，不代为启动模型或修改全局 harness 配置。技能目录可由用户按自己的 harness 安装方式加载，也可以在会话中明确让 Agent 读取该文件。

在项目目录使用同一 `towermd`：

```sh
towermd schema dev.draft.submit
towermd --text dev draft template <扫描ID> > /tmp/baseline-input.json
# Agent 根据固定提交与 S1–S5 报告编辑临时 JSON，然后提交：
towermd dev draft submit /tmp/baseline-input.json --producer <harness/version>
towermd dev draft ls
towermd dev draft show <返回的d-草稿ID>
```

提交使用已保留的 JSON Schema；模块名称、职责和置信度可综合，意图、接口、验收、风险及问题可补充。当前 S6 保留扫描中的模块 ID、路径和验证命令；合并拆分留 S7。提交者是自报来源，不能产生人类批准；通过/失败只继承原 S5 结果，不能用 Agent 自述覆盖。

守护进程通过临时 index 创建 `refs/towermd/drafts/<id>` 私有提交，保存精确输入、扫描原件与草稿对象/画布；相同输入和 producer 幂等返回同一草稿。用户工作树、index 和 main 均不变。有 origin 时仅推送这个私有引用；推送失败会报错并保留本地原件，可用相同输入重试。读取列表不会访问网络。跨机器发现与获取草稿引用暂未接入 App。

在画布右键菜单选择“查看综合草稿”，打开对应条目。草稿仍位于同一画布，点击卡片看原文，按需查看综合说明。接口关系与历史共同修改分别标注；候选不是已批准结构。来源 main 改变后显示过期；本机验证日志丢失后原文仍可恢复，但明确显示证据不可复核，不显示可信通过。删除 S1–S5 派生草稿缓存不影响已提交的 S6 原件。

### 人在画布审阅草稿

打开综合草稿后，点击卡片，在详情里选择编辑。可改名、合并模块、选择一部分真实源码文件拆分、补充或删除接口、确认/修改验证命令。拖动卡片顶部手柄保存位置；模块边界和命令有变化时，相关模块显示“待重新验证”，原 S5 报告仅作历史证据。确认命令本身不会执行命令，也不会批准结构。

把当前项目已有的 Markdown 文件拖进画布，或在右键菜单输入项目相对路径，确认后形成引用节点。正文来自草稿绑定的固定提交；未提交的文件、外部文件或与草稿对象输出路径冲突的引用会被拒绝。

每次保存生成新的不可变私有草稿引用，保留原综合输入和人工动作，使用 `human:local` 标记本机 App 审阅。草稿列表仍可打开旧修订；删除缓存并重启服务后可重放恢复。保存或推送结果未确认时，界面保留原请求供重试，在确认之前阻止新的编辑与项目切换；重试不会重复创建修订。CLI 没有人工审阅或批准命令。

审阅后在“综合说明”选择“重新验证当前草稿”。检查真实结果，再点“准备批准”，核对冻结内容后点“批准 B0”。成功回执包含实际提交、标签和快照；结果不确定时使用“重试原批准”，不要创建另一个批准请求。草稿保存本身不会写 main，明确批准才触发出版。

### 批准后：任务与 Agent 协作

1. 在画布右键选择“创建任务”，按已批准模块填写目标、允许路径和验收条件；也可让 Agent 用 `towermd task add <json>` 创建任务。
2. 在独立工作树的 `t/<task-id>` 分支启动自己的 Claude Code 或 Codex 会话，让它读取 [领取技能](../skills/towermd-claim/SKILL.md) 与 [收尾技能](../skills/towermd-finish/SKILL.md)，按技能领取任务、提交代码、运行 `dev check` / `dev finish`。App 不代为启动 harness。
3. 在模块卡片或右键“任务与运行”查看真实任务状态、依赖与运行证据。Agent 用 `ask` 提问后，在 App 回答；若人在 harness 会话中已直接回答，可由 Agent 用 `towermd answer <hold-id> "人的实际回答" --relayed` 转述，仅解开当前普通问题，不形成决策、规则或慢层批准。“向项目插话”或任务详情的插话会在 Agent 下次调用 CLI 时送达。
4. 只有 `dev finish` 实际通过后，Agent 才能取得 `dev trailers`，按技能将五项原值写入提交说明并遵循项目 Git 合并流程。只有 daemon 核实权威 main 后显示的 `merged` 才算完成。
5. 需要改变模块等慢层结构时，让 Agent 读取 [提案技能](../skills/towermd-propose/SKILL.md)。在 App 的“提案”中审阅差异、影响对象并批准或驳回；已有任务的复核标记不会因为 Agent 自述而消失。

“暂停全部”会拒绝新的领取，并向既有运行发送检查点指令；它不直接停止 harness 进程。解除暂停须在 App 选择“确认恢复领取”；CLI 的 `resume` 只发请求。

Hub 丢失时，可通过 `towermd hub rebuild` 请求恢复，再在 App“项目态势”准备并确认恢复。恢复使用真实快照与原件，不重跑旧验证；CLI 没有人工确认命令。

### 读取哪份内容

- 没有 `origin`：读取本地 `refs/heads/main`。
- 存在 `origin`：读取已 fetch 的 `refs/remotes/origin/main`。请自行完成 `git fetch origin`；App 不自动访问远端。
- 配置、对象、画布坐标与文件原文均来自同一固定提交。其他分支和未提交修改不会进入画布。
- `tower.canvas` 位于 `.towermd/config.yaml` 中的 `docs_dir` 下；文件节点路径以仓库根为基准。

没有配置时，App 显示初始化说明；已有配置但没有画布时，可主动扫描并验证生成候选。引用缺失或画布条目非法时，可在“项目态势”查看诊断。

## CLI 入口

CLI 提供初始化、扫描与综合草稿、任务领取与证据、问答、提案、快照与守护进程管理。随包程序位于 `Tower.md.app/Contents/MacOS/towermd`，可使用完整路径调用，或在当前终端将该目录加入 PATH。查看准确参数：

```sh
cargo run -p towermd -- --help
cargo run -p towermd -- init --help
```

只有在准备初始化一个项目时，才运行下面的命令，并将占位路径替换为实际仓库：

```sh
cargo run -p towermd -- init /path/to/project --docs-dir docs/tower
```

`init` 只生成项目 ID、配置、Agent 说明与文档目录骨架，不生成模块图、运行任务或提交 Git。已有 Tower.md 项目应沿用原配置和文档，不要用示例覆盖已有文件。检查生成内容后，自行决定如何提交；首次导入已有内容的提交不等于 B0 批准。

仅执行 `init` 不会产生模块图。将配置提交到权威 main 后，可在 App 扫描，或在项目目录运行随包 `towermd`：

```sh
towermd show
towermd dev scan start
towermd dev scan wait <返回的扫描ID>
towermd dev scan show <扫描ID>
towermd dev scan cancel <扫描ID>
```

扫描会实际执行候选命令，立即返回 ID；等待每次最多 90 秒，仍在运行时可继续等同一个 ID。`ready` 表示报告已经生成，不代表所有测试通过，须查看模块与命令的实际状态。取消也需要等原 ID 进入终态，不能把请求已接受当作进程已结束。CLI 没有批准命令。同一 clone 的多个工作树共用项目服务；同一 project_id 的独立 clone 不可同时打开。

## 开发验证

```sh
pnpm check
pnpm test
pnpm build
cargo test --workspace
node scripts/prepare-sidecar.mjs
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

已准备 sidecar 后，还可显式运行不需要窗口的原生真实重连专项（默认忽略，使用隔离 runtime，不操作当前项目）：

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  project_stream::tests::real_daemon::packaged_daemon_reconnects_and_invalidates_http_cache \
  -- --exact --ignored --test-threads=1
```

桌面 crate 与核心 workspace 使用各自锁文件。以上是验证入口，不代表当前工作区已经通过这些检查；具体结果须与实际源码及构建产物对应。界面改动还需在桌面 App 中验证。

双语变更记录见 [CHANGELOG.md](../CHANGELOG.md) 与 [CHANGELOG.zh-CN.md](../CHANGELOG.zh-CN.md)。版本节保留历史，未发布区描述当前实现的变更。
