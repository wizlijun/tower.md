# Tower.md v0.1.1 使用说明

本文是 v0.1.1 预览版（M1）的使用说明，原为仓库首页。项目远景见 [README](../README.md)。命令行已按[命令行规格](specs/2026-10-21-spec-towermd-cli.md) C0 重构，本文的命令名已随之更新；其余内容描述的仍是 v0.1.1 的画布与流程。

## 当前可用范围

当前源码提供**画布、冷启动审阅与批准、任务执行证据和人工放行的最小框架**。默认主界面只有画布，详情与操作按需打开。模块结构来自已提交的 JSON Canvas 与对象；任务、问题、禁止和检查未通过通过本机 WebSocket 更新，快层变化不移动节点。

App 通过随包 CLI 连接本用户唯一的本机守护进程。扫描与验证使用公共 Git 目录中的缓存和独立工作树，不改用户工作树或暂存区；基线经 App 明确批准后，才由守护进程出版到 main 和 Git 标签。初始化使用 CLI，Claude Code/Codex 等会话由用户自行启动，Tower.md 提供技能与命令协作。

上述 M1 主路径已完成真实 App 验收：Claude Code 与 Codex 在三个工作树完成十项任务，另验证了 B1 批准、旧任务复核、主干处置、Hub 恢复及恢复后重新领取并完成任务。详见[验收报告](qa/2026-10-21-m1-live-acceptance.md)。本次为 **macOS Apple Silicon 最小预览**，无发行者签名或公证；Intel 运行、NFR-03 定量性能和指定 Note.md 仓库的 M0 验收尚未完成，M2/M3 不在本次范围。

### 先看功能框架

从 [0.2.0 预览发行页](https://github.com/wizlijun/tower.md/releases/tag/v0.2.0) 下载 Apple Silicon ZIP，解压后打开 `Tower.md.app`，也可将它移入“应用程序”。本机只需启动一份 App；随包 CLI 位于 `Tower.md.app/Contents/MacOS/towermd`。真实项目需要本机 Git，以及项目验证命令所需的 Node.js、Rust 等工具。

打开 App，选择“打开功能示例”，无需选择仓库。示例用最简单的模块、版本、任务、提问、验证和放行组件串起流程：点击卡片打开操作，确认示例版本、领取任务、回答问题、选择模拟验证结果，最后放行或退回。

示例状态只保存在当前窗口的内存中，不启动受托者、不运行命令、不写 Git。示例画布明确标注“示例数据”；模拟通过或放行不代表真实证据、版本批准或任务合并。具体展示与流程可在试用组件后调整。真实仓库功能仍通过“打开项目”使用。

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

### 第一次打开：从一句话开始

还没有打开项目，或打开的项目画布上还没有节点时，App 只显示一张空画布和一句话输入框，不问项目类型、使用场景或模板。

1. **从一句话开始**：在输入框写下想做的事，例如“做一个把会议录音转成待办的命令行工具”，点“记下”。还没有打开项目时，App 先请你选择这件事所属的文件夹，然后把这句话记成一张“想法 · 未立项”卡片。所选文件夹还不是 Tower.md 项目时，这句话会留在输入框里；先在终端运行 `towermd init`（见下文“CLI 入口”）再打开。取消选择文件夹也会保留这句话。
2. **从已有材料开始**：点“打开项目”（`Cmd/Ctrl+O`），选择已有仓库，之后按下文扫描、审阅和批准。

想法和承诺分开：想法不进关注清单，不能领取，也不会派给受托者。每张想法卡有两个立项按钮，立项是你本人的动作，只能在 App 中做：

- **立项：作为项目目标**：这句话成为项目的目标，每个项目只有一个。目标不会直接改动人定的内容；接下来由受托者起草第一个版本，经你在 App 中批准后生效。
- **立项：作为任务…**：打开委托卡，标题预填这句话，按已批准版本的模块填写范围和完成标准后点“委托”，想法和任务在同一个事件里落定。没有已批准的版本时，只能先立为项目目标。

画布上已有节点时，想法收在左下角的“想法”栏里，点开才显示，不增加也不移动画布节点。一次查看中立项超过 3 个时，想法栏提示“另有 N 个已立项的事还没有人跟进”。Agent 和脚本用 `towermd idea add <文本>` 记下想法（见下文“CLI 入口”）。

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

### 托付：委托卡、验收与信任账本

本节对应[核心规格](specs/2026-10-21-spec-towermd-core.md) 4.7、6.1、6.3、6.5 节（FR-184、FR-185、FR-201–211）。

**委托卡。**创建任务就是填一张委托卡，5 栏顺序固定：目的、完成标准、边界、检查点、交付前自检。只有完成标准必填；没写时“委托”不可用。检查点每行一条，行尾写 `@ 2026-10-25 18:00` 即带上截止时间。Agent 用 `towermd task show` 读这张卡，同时读到这类工作已记下的规则和上次退回的理由。

**先澄清。**拿不准完成标准怎么写时，选“先澄清”。Agent 用 `towermd dev clarify` 交回复述、完成标准草稿和最多 3 个问题。你在澄清面板回答、改定完成标准，点“定稿并委托”后任务才能领取。定稿的卡片算人定内容。

**汇报请求。**带时间的检查点在领取时变成汇报请求，Agent 用 `towermd reply` 回应。到期没回应，守护进程记一次失联；同一任务连续两次过期才升为需要你注意。没写时间的检查点只是 Agent 的停止点。汇报不算进度。

**验收卡。**Agent 收尾时用 `towermd dev finish --claims` 逐条写自述。验收卡按完成标准逐条列证据，证据分 3 类，形状、颜色和文字标签都不同：

- 检查：门禁或检查程序的结果，绑定提交。整体检查单列一行，不代替逐条证据。
- 受托者自述：Agent 自己说的，不能单独让任务完成。
- 我用过：你在验收卡上点“我用过”产生，只有人能产生。系统无法核实你是否真的用过。

一条证据也没有的条目标为“未证实”。你仍可以一步确认；确认时还未证实的条目会记下来，计入信任账本。开发任务由检查和合并完成，你的确认记为“已验收”。确认时可以写一句“这次改变了什么看法”，不强制，也不会提醒补写。

**退回与规则。**不满意就退回，写上理由。已合并的任务会重新变为可领取，理由进入下一次的任务卡。退回后 App 会问是否把这次纠正记为规则；确认后，规则进入以后同类工作的任务卡，不确认就只留在这次的记录里。

**信任账本。**账本按“受托者配置 × 工作类别”分行。配置是 Agent 领取时自报的 harness、版本、厂商、模型和技能包版本，任何一项变了都对新配置重新起算，旧记录保留，可以对照。工作类别默认取模块 ID，你可以改名或合并，系统不自动归类。每行汇总最近 20 次托付（`config.yaml` 的 `track_record_window`）：一次通过比例、按时比例、满足完成标准的比例、越界次数，以及实测的托付到交付时长、验收时长和退回次数。验收时长只计 App 内从打开验收卡到做出动作的时间。承诺弱化计这一配置在本次托付期间造成的弱化次数（见下文“承诺弱化”）；兜圈子还未接入，显示“未接入”，不填 0。账本不打总分，不评价人，只存在本机和你的仓库里。

**委托级别。**级别同样按“配置 × 工作类别”设置，没设的继承项目级。默认 3 级：`report_only`、`answer_by_decision`、`approve_minor`，可在 `config.yaml` 的 `delegation_levels` 改。记录满一个窗口时，一次通过比例不低于 90% 会建议升一级，不高于 50% 会建议降一级。只给建议，级别只由你改。目前级别只记在账本上，还不改变 Agent 能做的事。

**抽查。**无论级别多高，已完成的任务都按比例抽出来请你复核，抽中的进入关注清单候选，确认或退回即消解。比例随一次通过比例下降，但不降到 0：默认最低 5%（`config.yaml` 的 `spot_check_min`，低于 1% 按 1% 算）。新配置没有记录，从 100% 起。

**待验收积压。**等你验收的事（待确认、待复核的抽查、待批准的提案）超过 5 件（`config.yaml` 的 `review_backlog`）时，首屏显示“N 件待验收”，委托按钮旁显示“先验收再托付”。这一行不占关注清单的 3 条名额，也不阻止委托。

**两种用法。**这两件事不做成功能，用现有的办法就能做到：

- 先写预期，再看结果。打开验收卡前，先写一句你对结果的判断，看完再对照。Tower.md 还没有旁注，可以先记在自己的笔记里；对照的结论写进“这次改变了什么看法”。
- 定期自己做一次。挑一类常托付的工作，隔一段时间自己做一次，免得验收的眼力退化。跟进项还没有上线，先用日历提醒自己。Tower.md 不判断你该不该自己动手。

升级到本版本后，先运行 `towermd daemon stop` 停掉旧的守护进程。此前领取的运行不进入信任账本。

### 接入接口：汇报与提醒

本节对应[核心规格](specs/2026-10-21-spec-towermd-core.md) 第 5 节（FR-114–119）。任何受托者或来源——Agent、CI、定时脚本——都用同一个信封向 Tower.md 汇报进度、上报值得注意的事。入口有三个：命令行、本机 HTTP 和文件收件箱，三者走同一套校验，写入同一个事件流。

```sh
# 在任务工作树里：默认指向当前运行的任务，发送者是领取时自报的受托者
towermd report --state in_progress --summary "解析器完成一半" --next "补测试" --progress 3/7 --unit files

# 任意 shell 脚本：运行之外须用 --from 写明发送者，--target 省略时指向整个项目
towermd signal eng.crap --level active --title "CRAP 超过 30" --value 42 --threshold 30 --from ci/nightly --ref src/parser.rs
towermd signal ops.disk --level passive --title "磁盘已用 85%" --from cron/disk
```

- **汇报不算进度。**`--state` 只有 `not_started`、`in_progress`、`waiting`、`blocked`、`claimed_done`，没有 `done`；`claimed_done` 只是自称完成，任务是否完成仍由检查与合并决定。
- **级别由发送者声明，打扰方式由 Tower.md 决定。**`log` 只进时间线；`passive` 计入角标和摘要；`active` 进入关注清单；`time_sensitive` 排在关注清单前面。关注清单里它们显示为“告警”，并注明是谁自报的。
- **合并。**同一发送者对同一对象的同种上报合并为一条，保留最新的值并计数。发送者用较低级别（如 `passive`）再发一次同种上报，就能把它撤出关注清单；你也可以在 App 里处理它，见下一节。
- **长度。**标题不超过 30 字，摘要与说明不超过 100 字；超长直接拒绝（`too_long`），不截断。
- **种类带命名空间。**如 `eng.crap`、`ops.disk`；`core.` 开头的种类留给 Tower.md 自己。
- **署名。**事件由命令行记为 `process:towermd-cli`；`--from` 只是自报，不代表可信身份，也不能自称为人（`human:` 开头会被拒绝）。App 只读取这些上报，不代发。
- **重试。**沿用同一个 `--request-id` 重试得到同一条记录；内容不同则报冲突。

结果在守护进程 `/state` 的 `projection.TowerApi` 中：`reports` 每个发送者 × 对象一条，`signals` 每个发送者 × 种类 × 对象一条，各带 `count`、首末两次的事件号与署名。带时间的检查点生成的汇报请求，在 `delivered[]` 中同时附上同一信封格式的 `envelope`（`verb` 为 `request_report`）。

升级到本版本后，守护进程接口升到第 12 版，先运行 `towermd daemon stop` 停掉旧的守护进程。

#### HTTP 入口与文件入口

不方便调用命令行的来源可以直接发信封。信封就是 `delivered[]` 里 `envelope` 的格式，`project` 是 `.towermd/config.yaml` 中的 `project_id`：

```json
{"v":1,"project":"<project_id>","from":{"id":"ci/nightly","kind":"source"},"target":"<project_id>",
 "verb":"signal","data":{"kind":"ops.disk","level":"passive","title":"磁盘已用 85%"},"at":"2026-10-23T10:00:00+09:00"}
```

- **HTTP。**`POST http://127.0.0.1:<port>/tower`，请求体是一个信封。`port` 与 `instance` 来自 `towermd daemon ensure` 的输出；请求头须带 `x-towermd-instance: <instance>`，用来确认连的是当前这份守护进程，不是身份认证。只监听 127.0.0.1；认证与加密留待安全加固。项目须已在 App 中打开，否则返回 404。事件署名 `process:towermd-http`。内容完全相同的信封重发得到同一条记录。
- **文件。**把信封逐行写进 `<运行目录>/inbox/<project_id>/*.jsonl`（一行一个，单行不超过 64 KiB）。运行目录默认是 `~/Library/Application Support/towermd/runtime`，可用 `TOWERMD_RUNTIME_DIR` 改。文件 2 秒没有改动后，守护进程在下一次维护（约每 10 秒一次）取走它、逐行记成事件（署名 `process:towermd-file`），然后删除文件；被拒绝的行连同原因写到 `inbox/<project_id>/rejected/<文件名>`。收件箱在仓库之外，文件不会变成状态，也不会写进任何工作树。只处理 App 中打开着的项目；守护进程暂时出错时文件保留到下一次。
- **同一套规则。**两个入口与命令行一样拒绝 `human:` 发送者、超长文本、`core.` 种类和 `request_report` 等由 Tower.md 自己发出的动词，也受下面的扩展权限约束。命令行沿用 `process:towermd-cli` 署名。

#### 扩展：声明，不执行

本节对应核心规格 FR-120、FR-123。扩展是一份清单，声明某个来源会发哪些种类的上报、最高到哪一级；Tower.md 不加载、不运行扩展的代码，也不替它启动进程。扩展自己的脚本仍经上面三个入口发信封。

清单放在 `.towermd/extensions/<id>/towermd-plugin.yaml`，只从权威 main 读取：在 main 上即为已安装并启用，工作树里的改动不生效。`.towermd/` 整体是人定内容，所以安装、修改、卸载扩展都要经提案由你批准。

```yaml
id: eng-clean            # 必须与目录名相同；小写字母、数字、- 与 _
version: 1.2.0
type: check              # template、check、connector、skills、pack、kit；未知类型也接受
emits:
  - kind: eng.crap       # 只有本扩展能发这个种类
    label: CRAP
    icon: gauge
    unit: 分
    deterministic: true  # 声明后才可发确定性上报
    max_level: active    # 最高级别，省略为 active
commands:
  crap: cargo run -p probe -- functions   # 只是说明，Tower.md 不执行
```

- **查看。**`towermd ext ls` 列出 main 上的扩展、声明的种类和清单问题；`towermd ext show <id>` 显示一份清单的全部内容。命令行没有安装或启用命令。
- **发送者。**扩展以 `pkg:<id>[@版本][/部件]` 署名发送，如 `pkg:eng-clean@1.2/crap`。

| 情况 | 结果 |
| --- | --- |
| `pkg:` 发送者，扩展不在 main 上，或清单有问题 | 拒绝 |
| `pkg:` 发送者发清单没声明的种类 | 拒绝 |
| 级别高于该种类的 `max_level` | 拒绝 |
| 声明了 `--deterministic`，清单没写 `deterministic: true` | 拒绝 |
| 其他发送者发某个扩展声明的种类 | 拒绝（种类归该扩展所有） |
| 两个扩展声明同一种类 | 两份清单都记为有问题，二者都不能发送 |
| 其他发送者发没有扩展声明的种类 | 照常接受 |
| 以 `human:` 署名，或用 `core.` 种类 | 拒绝（与没有扩展时相同） |

**扩展的检查只能阻止，不能放行。**扩展的确定性上报和其他来源一样，可以让 `gates.yaml` 中同种类的硬检查判为“检查未通过”；但一项检查要算“通过”仍须守护进程验证器的结果。让验证器去运行扩展的命令就等于执行扩展代码，因此本版本不这样做。

### 关注清单与“自上次查看以来”

本节对应[核心规格](specs/2026-10-21-spec-towermd-core.md) 第 7 节（FR-40–42、FR-128–131）。

**每条一张卡，6 段固定。**关注清单的每一条都按同一顺序写成一行，不超过 100 字：何时 · 哪个对象：发生了什么。因为……。建议……。需要你：……。过长时先缩短最长的几段（以“…”结尾），时间、对象和“需要你”尽量保持完整。卡片前有形状和类别文字，不只靠颜色区分。

**排序。**先是时间敏感和涉及人定内容的（如“禁止”和 `time_sensitive` 提醒）；再是等你回答或批准的（提问、待批提案），等得最久的在前；其余按影响的对象多少排。时间未知的排在同类最后。首屏只放 3 条，其余收在“另有 N 条已延后”里，画布上的项目卡也只显示这一行计数，不增加节点、不移动节点。

**知道了与静音。**`active`、`time_sensitive` 级的提醒卡下有两个按钮，只在 App 中提供，命令行没有对应命令：

- **知道了**：这条提醒离开关注清单，直到发送者再报一次。只对你看到的那一次生效；你打开后发送者又更新过，会要求你重新查看。
- **静音 7 天**：7 天内不再进入关注清单。发送者把级别升到高于静音时的级别（如从 `active` 升到 `time_sensitive`）会立刻重新出现。

两者都记为你本人（`human:local`）的事件，经守护进程写入，随事件日志可重放。已处理的条数显示在清单下方。

**自上次查看以来。**打开项目时，画布左上角显示一张 3 行摘要：完成了什么、卡在哪里、需要你做什么。离开不超过 24 小时且条数少时列名称，否则只给数目；“展开明细”可看逐条。`passive` 级提醒只在这里出现，同一对象收到 3 个以上来源的提醒时合成一条。

“上次查看”是你在本机最后一次关闭这个项目（切换项目或关闭窗口）的时间，只存在 App 的本地存储里，不写进仓库，也不进 hub。没有记录或记录读不出时，摘要改为“最近 7 天”。

### 检查：什么时候算完成

本节对应核心规格 FR-120–124。完成只认检查：对象的全部检查在当前版本上通过，才算完成；只有一项确定性检查都不适用的对象，才可由你在 App 中确认完成。开发任务总带一项内置检查 `core.verify`（`dev finish` 运行的模块验证命令），所以开发任务只能由检查和合并完成，你的确认记为“已验收”。

项目可以在 `.towermd/gates.yaml` 中另外声明检查。它是人定的内容：Agent 和命令行只能通过提案修改，守护进程只读权威 main 上的这一份，工作树里的改动不生效。

```yaml
gates:
  crap:                  # 检查名
    check: eng.crap      # 检查种类，与 towermd signal 的种类相同
    hard: true           # 省略即为 true；false 只提醒，不阻止
  style: eng.style       # 简写：只写种类，等同 hard: true
```

- **阻止。**任一来源对该对象发出的确定性上报（`towermd signal <种类> --deterministic`）达到 `active` 或 `time_sensitive` 时，同种类的硬检查判为“检查未通过”：`dev finish` 与 `dev trailers` 返回 `gate_failed`，App 也不接受确认完成。发送者以较低级别再发一次即解除。不带 `--deterministic` 的上报不影响检查。
- **放行。**上报只能阻止，不能放行：`--from` 是自报的。一项检查要算“通过”，须有守护进程验证器（`process:towermd-verifier`）在当前版本上的结果；目前只有内置的 `core.verify` 有这样的结果，其他种类在没有阻止上报时显示“未见阻止”。
- **出错即阻止。**`gates.yaml` 不是合法 YAML、某项缺 `check`、`hard` 不是布尔值、种类不合法，都判为“检查未通过”，直到人修好它。未知的键原样保留。
- **证据。**守护进程每次在 finish、trailers 或确认完成处作出判断，若项目声明了检查，就记一条 `kind: gate` 证据，写明对象、版本、读取的 main 提交与各项结果；`core.verify` 的结果是 `kind: check` 证据，绑定被验证的提交。
- **查看。**`towermd task show` 的 `delegation.card.completion` 与 App 委托详情的“检查”一栏列出每项检查的状态：检查通过、检查未通过、检查中、未见阻止。

### 承诺弱化

本节对应核心规格 FR-125。完成标准、检查或门禁被放宽时，Tower.md 自己判断并提醒你。提醒以“承诺弱化”出现在关注清单靠前的位置（图标 ⇣），由 Tower.md 署名（`process:towermd`），不是来源自报。提醒不批准、不阻止任何事：改动本身仍要你在 App 里批准才生效。

| 算弱化 | 不算（判断不了就不报） |
| --- | --- |
| `gates.yaml` 删掉一项检查，且没有别的门禁仍检查同一种类 | 新增、改名、由软改硬 |
| 硬门禁改为软（`hard: false`），且没有别的硬门禁仍检查同一种类 | 任一边的 `gates.yaml` 读不懂 |
| 门禁写上 `deterministic: false` | 改写措辞、调换顺序 |
| `config.yaml` 的 `review_backlog` 调高、`spot_check_min` 调低（按生效值比） | 其他阈值，以及扩展自定义的阈值 |
| 完成标准（委托卡的 `done_when`、验收标准）条数变少 | 条数不变而内容改写（是否放宽无法判定） |
| 模块或验收的验证命令被删掉，且同一提案里别处不再有同一条命令 | 命令挪到另一个模块 |

在这些地方检查：

- **提案。**提交或捕获提案时检查它带的改动，提醒指向这份提案。App 的提案详情另列“这是弱化”一栏，逐条写明放宽了什么；批准即接受这些放宽，不同意就驳回。
- **澄清。**受托者用 `towermd dev clarify` 起草的完成标准比委托卡上原有的少时，提醒指向这张委托卡。你在 App 里定稿时若删减了完成标准，面板标出“这是弱化”；定稿本身就是你的决定，不再另发提醒。
- **主干。**守护进程核对主干时，比较上次核对过的版本与当前版本的 `gates.yaml` 和 `config.yaml`，提醒指向项目。

同一来源只提醒一次，重试或多台主机都不会重复。由运行中提交的提案或受托者的澄清造成的弱化，计入该受托者配置在信任账本上的“削弱标准”；主干上的改动不归到任何配置。

### 读取哪份内容

- 没有 `origin`：读取本地 `refs/heads/main`。
- 存在 `origin`：读取已 fetch 的 `refs/remotes/origin/main`。请自行完成 `git fetch origin`；App 不自动访问远端。
- 配置、对象、画布坐标与文件原文均来自同一固定提交。其他分支和未提交修改不会进入画布。
- `tower.canvas` 位于 `.towermd/config.yaml` 中的 `docs_dir` 下；文件节点路径以仓库根为基准。

没有配置时，App 显示初始化说明；已有配置但没有画布时，可主动扫描并验证生成候选。引用缺失或画布条目非法时，可在“项目态势”查看诊断。

### 连接 Note.md

Tower.md 与 Note.md 只互相引用：Tower.md 不存长篇知识，Note.md 不存执行状态。默认不连接，不报错。要连接，在 `.towermd/config.yaml` 中写明知识库目录，并提交到权威 main：

```yaml
notemd:
  vault: ~/notes   # 绝对路径、~/ 开头，或相对仓库根
```

**回写（Tower.md → Note.md）。**守护进程每轮维护（约 10 秒）检查一次，把两类结果各写成一份纯 `.md` 摘要，放进知识库的捕获目录 `{vault}/{inboxDir}/towermd/`（`inboxDir` 取 Note.md 的 `.notemd/settings.json`，没设时为 `inbox`）：

- 已合并的任务：`task-<任务ID>-<提交前7位>.md`，写为什么做、合入提交与完成标准。重开后再次合并会另写一份。
- 已接受的决策（`status: accepted` 或 `stable`）：`decision-<决策ID>.md`，写问题、结论与它引用的 Note.md 知识。

摘要是 OKF v0.2 文档，`generated.by` 为 `process:towermd`，`sources` 指回合入提交和 Tower.md 对象：

```markdown
---
type: Decision Summary
title: 决策 D-7：用哪种共识？
description: 用 Raft
tags: [towermd]
generated: { by: process:towermd, at: 2026-10-23T08:00:00Z }
sources:
  - { id: commit, resource: git:<提交>, title: 提交 1a2b3c4 }
  - { id: decision, resource: towermd:<project_id>/D-7, title: Tower.md 决策 D-7 }
  - { id: note-1, resource: kb/raft.md, okf_id: kb/raft, block: b-00ff00 }
---
# 决策 D-7：用哪种共识？

问题：用哪种共识？

用 Raft

在 Tower.md 中接受（决策 D-7[^decision]），记录于提交 `1a2b3c4`。[^commit]

引用的 Note.md 知识：

- `((kb/raft.md#b-00ff00))`[^note-1]
```

回写只做这些事：只写 `.md`，从不写 `.note.md`，不加链接、不结网；文件名固定，只新建、从不覆盖，你改过、挪走或删掉的摘要不会被重写；不碰知识库里已有的文件，也不在知识库里运行 git。整理、链接与收进知识网仍由你在 Note.md 里做。第一次连接（或换了知识库）时只记下现有的任务和决策，不倒灌旧账，之后新出现的才写。记录存在本机运行目录，删掉只会让下一轮重新记一遍，不会重写。知识库位于本项目仓库内时拒绝回写。写入失败只在守护进程日志里提示，下一轮重试。

**引用（Note.md → Tower.md）。**提案输入可以带 `sources`，记下依据的 Note.md 知识：`okf_id` 是概念路径去掉 `.md`，`block` 是块 ID（Note.md 块引用 `((kb/raft.md#b-00ff00))` 中 `#` 之后的部分），可选 `path` 与 `title`。格式见[提案技能](../skills/towermd-propose/SKILL.md)。批准后它们写进出版的决策的 `sources`，决策摘要再把块引用列出来。读取时宽容：不认识或写错的来源条目原样保留，不会让对象被拒绝。

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

任务范围外想到的事，用一句话记成想法，而不是新建任务：

```sh
towermd idea add "给周报加 CSV 导出"
```

想法一行写完，不超过 200 字，署名 `process:towermd-cli`；同一 `--request-id` 重试得到同一条想法。命令行没有立项命令，想法只能由人在 App 中立项。

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
