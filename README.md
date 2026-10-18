# Tower.md

Tower.md 管执行 issue，Note.md 管 AI 知识，两者的关系类比 Jira 与 Confluence。

官网：[towermd.net](https://towermd.net)

Tower.md 是一个本地优先的多 Agent 工程驾驶舱，由技能包、`towermd` 命令行和画布 App 三部分组成：慢层（目标、架构基线、验收标准）由人批准，快层（任务、运行、提交）由加载了 Tower.md 技能的 Agent 在各自的 harness 会话里推进，可以跨工作树、跨机器。活跃状态由唯一的 hub 协调，定期快照回 git；文档与 `tower.canvas` 放在初始化时指定的文档目录，大文件交给 [Arca](https://github.com/wizlijun/arca)。

## 设计原则

与 Note.md 的五个信念一一对应，详见 [设计原则](docs/product-principles.md)。

1. **Agent 的动作无限，你的注意力有限——放行才是残余。** 默认沉默，首屏只给 3 件事，同一个问题不问第二遍。
2. **仓库高于服务。** 活跃状态由唯一的 hub 协调，定期快照回仓库；守护进程、索引、画布乃至 hub 都能从仓库重建。
3. **Agent 推进，你放行。** 结构只在人批准处改变；Agent 说完成不算，有证据才算。
4. **你的回答属于仓库，不属于会话。** 人的回答、授权、豁免落为决策、规则或证据，换 Agent、换机器都还在。
5. **一个仓库，多个 harness——你是塔台。** harness 可替换，仓库中立，Tower.md 自己也能被移走。

与 Note.md 之间还有三条接口原则：主张与承诺分开出版；两道人工闸门，每个方向一道；同一套契约，学一次两边用。

## 桌面壳开发

最新 Apple Silicon [Tower.md 问答与任务评审开发预览](.release/prototype/2026-10-19-app-answer/Tower.md.app) 的生产源码固定于 `055d3e4`（0.0.0、release profile、仅 ad-hoc，无发行者签名或公证）。打开真实项目后，可查看画布、编辑已保存草稿、读取任务评审及“本次问答”；在任务工作树运行 `towermd ask` 后，把立即显示的 Hold ID 粘贴到 App，即可明确回答。已实机验证 App 回答被同一包内 CLI 收到，以及重读首答不会出现第二次回答入口；原生队列的同请求重试也已通过。该预览尚未包含后续后台 finish 接线；完整 spec、Proposal／后续基线与正式发布仍在推进。[实际构建与包检查](.release/validation/2026-10-18-spec-resume/app-answer-app-static455.json)、[实机问答证据](.release/validation/2026-10-18-spec-resume/app-answer-gui-evidence461.json)可单独核对。此前调试窗口已关闭，预览文件保留。

此前的 Apple Silicon [Tower.md 0.0.0 当前基线草稿开发预览](.release/prototype/2026-10-18-current-baseline/Tower.md.app) 独立保留。采用 release profile，携带前端与 CLI，可复制完整 App 使用；支持从已核当前基线创建或恢复草稿，并沿同一画布编辑、保存和重开。保留 S6 交接、S7 结构表单及卡片／关系编辑。仅有 ad-hoc 签名，无发行者签名或公证，尚非完整 M1 或正式版本。

最小画布 App 的使用路径：“打开真实项目”→“打开项目”→选择 Git 项目→“查看项目画布”。不要求先初始化 Tower.md；有权威画布时读取 main 的节点、关系和文档原文，保留画布坐标；没有画布时展示固定 HEAD 的真实目录包含关系，明确标为目录预览。点击节点查看详情，拖动空白处平移，右下角缩放或适应画布；提交后点击“刷新”读取最新内容。空仓库、读取失败和损坏画布有明确提示，可重试或切换项目。单纯展示不启动 daemon、不续租、不写回，未提交文件不进入画布。

已提交初始化配置的项目可点击“生成候选画布”，由随包 CLI 启动或发现本机 daemon，后台扫描固定提交并执行发现的验证命令。界面展示真实模块原文、通过／失败／无可执行测试结果及共享报告；继续等待复用同一扫描 ID。候选未批准且只读，不等于 S6 综合或已生效基线。

扫描完成后，App 显示扫描 ID、来源提交和交给现有 Agent 的命令指引。在所选项目目录，让已有会话中的 Agent 读取 `towermd baseline scan --wait <scan-id>` 和 `towermd baseline schema`，综合并保存 `input.json`，再运行 `towermd baseline submit input.json`。提交后点击“打开此扫描的 S6 草稿”，即可读取同一 ID 的真实已保存草稿，无需抄写 ID；其他草稿仍可通过“已保存草稿 ID”打开。App 不执行这些指引命令，也不自动综合、提交或批准 S6。文本、链接、分组和普通关系可孤立或成环；文件节点仍绑定真实对象，保存不代表批准。

已有已认证基线的项目，可明确点击“创建或恢复变更草稿”，先查看已核基线号与固定 main 提交，再点击“确认创建变更草稿”。App 经同一守护进程与唯一 writer 复制慢层对象和画布，保存到私有 Git 草稿后再打开现有画布；模块、接口、验证命令及卡片／关系编辑继续走相同保存与重开流程。查看项目本身不会开启此服务动作。CLI 不提供创建或批准该人工草稿的命令。

创建面板显示尚未确认的原请求，关闭重开后仍可用原 UUID、身份和来源绑定重试。准备结果未知时须先重新读取本机队列，读取失败继续禁止另建请求；已确认保存但草稿暂时打不开时，“打开已创建草稿”只重读相同 ID，不重新创建或提交。后续草稿可审阅和拒绝，新 Bn 的批准与出版仍在后端硬拒绝，Proposal 和任务冲突复核尚未开放；不能将保存草稿视为新基线生效。

上述当前基线旧预览固定于 `95d5b21`：核心 `5159ea5`、App `c73d91b`、daemon `07bb7ad`、发布准备修复 `2b80428` 及真实全链测试 `95d5b21`。189 项已记录源码输入均与提交一致、构建前后不变；包内 CLI 的非 Git 调用、两份 arm64 Mach-O、严格 ad-hoc 校验与实际内嵌前端资源检查通过。见[原构建清单](.release/validation/2026-10-18-resume/current-baseline-app/candidate-manifest.json)及[内嵌资源与 GUI 核验补充](.release/validation/2026-10-18-resume/current-baseline-app/validation-summary.json)。此前三个 App 与原清单保持不变。

只想先看演示，运行 `CI=true node scripts/project-pnpm.mjs dev`，浏览器打开 `http://127.0.0.1:1420/` 后点击“查看演示”。真实文件夹选择与初始化须在桌面 App 中使用。

Tauri App 同时保留可选的项目初始化：已有配置不再询问文档目录；新项目显示文档目录与受保护区域说明，保留 CLI 诊断并提示检查未提交文件。初始化须明确点击，不自动提交、切分支或批准基线。

`towermd daemon ensure` 可从非 Git 目录启动或发现本用户唯一守护进程，返回真实 endpoint；重复或并发调用复用同一实例，不续租任务或投递消息。App 只在明确生成候选、等待或打开已保存草稿等服务操作时调用该入口，核对 loopback 地址及实际 health 实例，不自动停止旧 daemon。

上述当前基线旧预览的验收范围：前端 119 项、native lib 31 项、类型检查（零错误／警告）、Vite 构建（410 modules）、daemon 路由 3 项及 workspace／native Clippy 通过。核心 `5159ea5` 最终定向验证为当前基线 4 项、扫描 17 项和保存 6 项；既有 B0 出版矩阵 11 项通过于最后一处仅针对 Bn 的保存守卫加入之前，原证明器与 writer 未改，不将适用回归称为最终源码全工作区测试。

该旧预览的真实 native→daemon 单条完整链通过（255.72 秒，父子进程不重复计数）：八类 S7 编辑后实际出版 B0，从原 UUID 创建队列重启恢复并核原回执，再编辑 B1 草稿、重启重读一致；新 B1 批准被拒绝，主干、HEAD、index 与用户状态保持。对新包准确路径的桌面控制仍返回 `-10005: cgWindowNotFound`，未取得窗口或点击证据。自动化程序与静态包检查不代替 GUI 验收；完整 spec、实时任务、Proposal／后续基线出版、任务收尾与技能、Intel 实机和正式发布仍待完成。

需要 Node.js 22.13+、npm、Rust stable 和 macOS Xcode Command Line Tools。项目启动器在 `.tools/` 内安装固定的 pnpm 11.7.0，不要求全局 pnpm 或 corepack：

```sh
node scripts/project-pnpm.mjs install
node --test scripts/project-pnpm.test.mjs
node scripts/project-pnpm.mjs test
node scripts/project-pnpm.mjs check
node scripts/project-pnpm.mjs build
node scripts/stage-cli.mjs --target aarch64-apple-darwin
node scripts/project-pnpm.mjs tauri dev
```

桌面编译、原生测试和启动前，先按目标架构准备随包 CLI；Intel 使用 `node scripts/stage-cli.mjs --target x86_64-apple-darwin`。核心 CLI 源码改变后须重新运行此步骤。脚本使用 Cargo 实际报告的输出目录，支持 `CARGO_TARGET_DIR` 和 Cargo 的 `build.target-dir` 配置，构建失败或缺少目标产物时停止。

`CI=true node scripts/build-app.mjs --target aarch64-apple-darwin --locked` 可顺序准备同架构 CLI、构建前端和 App bundle；Intel 使用 `x86_64-apple-darwin`。入口检查仅 ad-hoc 的配置并过滤签名环境，保留非交互构建所需的 `CI`；前序失败或缺少 CLI／前端产物时停止。Tauri 原有前端构建 hook 保留，可能再次构建前端。开发预览保留自然构建日志、实际包与来源哈希、包内 CLI 的非 Git 目录调用、架构及 ad-hoc 静态检查收据；完整 GUI 与 Intel 包仍待验收。

此前 [S6 交接包](.release/prototype/2026-10-18-s7-handoff/Tower.md.app) 固定于 `937f1f4`，包含 `60b1616` 的真实接口标题标签、`e5d3c97` 的原请求队列保护、`c67dd72` 的结构表单，以及 `c2dac1b` 的草稿临时删除修复；`42542a0` 补齐真实 native 八动作测试，`937f1f4` 仅增加扫描到已有 S6 草稿的显式交接。[该交接版构建与验收收据](.release/validation/2026-10-18-resume/s7-handoff-app/) 独立保存，构建输入包含入口 `index.html` 和 `pnpm-workspace.yaml`。

此前 [S7 编辑包](.release/prototype/2026-10-18-s7-editing/Tower.md.app) 固定于 `42542a0`，[原 manifest](.release/validation/2026-10-18-resume/s7-app/candidate-manifest.json) 及[输入补充](.release/validation/2026-10-18-resume/s7-app/input-supplement.json) 原样保留；其原 181 项记录是输入子集，不代表全部构建输入，也不包含 S6 交接。更早的 [draft-review 包](.release/prototype/2026-10-18-draft-review/Tower.md.app) 与[来源收据](.release/validation/2026-10-18-resume/app/release-profile-provenance.json) 同样保留。后续源码不会自动进入已经冻结的旧包。

版本与双语日志轮转、冻结 tag 后，可用 `node scripts/release/build.mjs --root . --tag v0.1.0 --target aarch64-apple-darwin --output /private/tmp/towermd-arm64-release` 编排锁定的 App／DMG 构建；Intel 换用 `x86_64-apple-darwin` 和另一全新输出目录。工具核对 tag、源码和锁文件，隔离目标输出，校验 CLI 架构及合并后的 ad-hoc 配置，并保存构建收据与目录外日志；现阶段仅工具已验，尚未冻结首版或生成发布包。

两架构构建及原生技能准备完成后，可用 `node scripts/release/assets.mjs --root . --tag v0.1.0 --arm-build /private/tmp/towermd-arm64-release --intel-build /private/tmp/towermd-intel-release --output /private/tmp/towermd-release-assets` 检查静态包、冻结来源和收据，组装四个 App ZIP／DMG、技能包、manifest、双语说明及 SHA256SUMS。输出目录必须全新且在源码外；技能缺项、架构／依赖／签名错误或资产漂移会拒绝。该工具不启动 App，内嵌前端字节、安装、真人与 Intel 实机仍须另行验收。

同架构运行 `node scripts/project-pnpm.mjs tauri build --debug --bundles app --no-sign` 构建无发行者签名的开发桌面壳；跨架构构建时先 stage 对应 CLI，再给 Tauri 显式传相同 `--target`。可能存在 linker/ad-hoc 签名，尚无发行者签名或公证。桌面 crate 的锁文件与核心 workspace 独立；核心 `cargo test --workspace` 不编译 Tauri。上述 Node、npm、Rust 和 Xcode 是开发构建工具；release-profile 预览已携带前端与 CLI，项目验证仍使用本机对应工具。完整桌面交互及 Intel 运行仍待验收。

`node --test scripts/release/*.test.mjs` 验证发布工具的双语日志、版本、ad-hoc 配置、输入范围、包检查、冻结技能归档、资产集合和校验和契约。当前已有校验、输入审计、版本准备、冻结构建与资产组装工具；首版冻结 tag、两架构发行资产及正式发布仍待完成。

发布输入审计使用 `node scripts/release/audit.mjs scan --root . --output .release/inventory.json` 采集所有工作树、分支未合入提交和 stash。为每项填写范围、理由与证据后，执行 `node scripts/release/audit.mjs check --root . --decisions .release/scope.json` 重新采集并核对；输入漂移或漏项会拒绝。未提交内容须先单独验证提交再重采，补丁等价须附冻结功能测试依据。`.release/` 只存本地审计 JSON，包含本机路径，不放入发行资产。

`node scripts/release/prepare.mjs check --root . --version 0.0.0 --phase prepare` 只读检查当前项目版本、锁与双语日志。版本准备会定点同步本项目版本并轮转日志；离线缺包提供受限诊断，中途失败恢复本次写入，额外改动保留并报告冲突。实际版本轮转须等功能和发布前置验收完成后执行。

## 开发与初始化（M0）

冷启动已具备按固定提交、扫描选项和历史可用性重建的纯草稿数据内核，可生成相互对应的 Module 文档与 Canvas 字节，保留 Unicode 模块 ID。后台真实扫描、验证报告／Evidence、私有草稿保存与读取已接线；扫描本身不进行 S6 综合，也不批准基线。

需要 Rust stable（edition 2021）与 Git。构建和验证：

```sh
cargo build --workspace
cargo test --workspace
```

在已有 Git 工作树中初始化 Tower.md：

```sh
cargo run -p towermd -- init /path/to/repo --docs-dir docs/tower
```

省略路径时使用当前目录；从工作树的子目录运行时仍写入该工作树根目录。首次运行未传 `--docs-dir` 且标准输入是终端时会询问目录，回车使用 `docs/tower`；非交互新项目必须显式传入 `--docs-dir`，否则退出且不生成文件；已有配置重入时可省略并沿用旧目录。

`init` 生成 UUIDv7 项目 ID、`.towermd/config.yaml`、`.towermd/AGENTS.md` 和文档目录骨架。再次运行保留项目 ID 与配置原文，不覆盖已有文档或用户修改过的规则；不连接 daemon、不校验运行或续租，不修改根目录 `AGENTS.md`、暂存文件或创建提交。请检查生成内容，并按提示自行添加规则引用、提交文件。

Tower.md 技能包尚未发布。拿到正式技能包后，由用户按 harness 原生方式将每个含 `SKILL.md` 的技能子目录放入仓库 `.agents/skills/`（Codex）或 `.claude/skills/`（Claude Code）；`init` 只提示，不创建这些目录或修改 harness 配置。

文档目录必须是工作树内的相对路径；初始化路径中的符号链接、保留目录和文件/目录冲突会报错。多个初始化同时发生时，竞争失败的一方可重试，已有项目 ID 不会被覆盖。

## 本地任务与守护进程（M0-2）

`status` 与任务命令返回 JSON，并自动连接本用户唯一的守护进程（只监听 `127.0.0.1`）。它读取权威 `main` 上的配置和对象：有 `origin` 时读取缓存的 `origin/main`，没有时读本地 `main`（即使有残留 tracking ref）；工作树里未提交或任务分支里的配置、模块和验收变更不生效。首次使用前，请先由人检查并提交初始化结果及模块、验收对象到 `main`；有 origin 时还须首次成功 fetch。缺少缓存权威时，`status` 返回 `authority_unavailable`，对象读取与写操作拒绝。

读取与续租不等待网络；daemon 对同一公共 Git 目录每 60 秒最多启动一次后台 fetch，失败也限频。JSON 的 `main.fetched_at` 只表示本次 daemon 生命周期最近一次成功后台 fetch 的 Unix 毫秒，未知为 `null`；失败诊断保存在 `main.last_fetch_error` 并附警告，失败保留成功时间，重启后这些派生信息清空并重新刷新。无 origin 时两字段均为 `null`。

以下假设 `main` 中已有模块 `A-sync`（paths 包含 `src/sync/**`）与对应验收对象 `AC-sync`：

```sh
towermd status
towermd task add --id T-1 --title '完善同步' --module A-sync \
  --acceptance AC-sync --allow 'src/sync/**'
git worktree add -b t/T-1 ../sync-task main
cd ../sync-task
towermd task claim T-1
towermd task show
towermd check
towermd status
towermd task release
```

领取后可运行 `towermd check`（或 `towermd check --run <id>`），检查当前提交的 commit／tree、基准与实际变更路径，以及当前运行的 G0／G2／G5 门禁。它不执行验证命令、不生成 Evidence；结果中的 G3 始终为 false，不能据此认定任务完成。工作树须干净，越界、受保护路径或失效运行会被拒绝。

提交任务改动并变基到当前 main 后，在原任务工作树执行后台验证：

```bash
towermd finish
towermd finish --wait <evidence-id>
# 可选：报告放在任务工作树外，只作为线索，不作为验证结果
towermd finish --report ../report.md
```

首次调用返回 `E-…` ID 及 `queued`／`running`，守护进程在独立分离工作树运行已批准的模块命令。等待每次最多90秒，未完成可用同一 ID 再次等待；同一活动输入不会重复执行，其他模块可并行验证。终态返回实际封存报告，`failed`、`no_tests` 和中断会以非零退出；来源或确认失败时保留原请求并按提示协调。重启可恢复未启动的队列任务，已启动但没有耐久结果的任务记为中断。后台验证本身不代表已合入 main；trailers 与正常合并观察链仍待后续实现。


`--allow`、`--acceptance`、`--touches`、`--depends-on`、`--decision` 可重复。任务允许路径必须能证明是模块路径的子集，且不能包含受保护目录；目前对子集判断采用保守规则（相同 glob、明确文件和目录前缀 `/**`），无法证明时拒绝。显式决策引用必须存在；没有相关决策时不要求虚构一个。未合并依赖阻止领取。

显式非法的 `layer` 保留原文并返回路径诊断，按慢层保护；缺失字段才采用已知类型的默认层级。目录外或没有 ID 的非法层级对象也受保护，出版与删除仍要求合法慢层资格。

必须先建目标工作树及 `t/<task-id>` 分支再领取；一次领取原子取得全部模块租约。一个工作树只能有一个活动运行；`task show` 默认按本机、工作树、分支选择，`--run <id>` 可显式覆盖。用户命令为调用工作树的绑定运行续租，TTL 15 分钟；`init` 不参与续租。长期操作前后应调用 `status`，超时或主动 release 后任务回到就绪。未知费用保持 `null`。验证通过仍不等于已合并；CLI 不提供批准或放宽路径的命令。

无法确定运行绑定时，提示只列本机仍在运行的 Run；`status` 保留已释放、已到期及其他主机的运行历史。

领取时可同时提供 `--harness <名称> --harness-version <版本>`；只记录为技能自报信息，不改变 `process:towermd-cli` 的事件署名。任务卡包含权威 main 的验收、验证命令、接口与决策上下文。

当前源码还提供任务人工评审。在仍持有运行的任务工作树中提交 G1 计划（明确的 UTF-8 文件，可以尚未提交、位于允许路径之外），或请求 G4 评审当前干净工作树的已提交 tree：

```sh
towermd review request --gate G1 --content /path/to/plan.md
towermd review request --gate G4
```

复制返回的 Hold ID，在 App 打开同一项目，点击“任务评审”并读取原件。核对完整计划或 tree、运行、基线和全部租约绑定后，明确选择批准、需要修改或拒绝，再保存提交；CLI 不能批准。App 可以从同一仓库的另一个工作树查看实际任务。计划、tree、基线或租约改变时，旧批准失效；历史卡不允许新决定。不确定结果保留原 UUID、意见与署名，只能明确重试原请求。

文首最新预览已包含任务评审入口。评审本身不执行验证、不认证任务完成；源码已接入后台 finish 验证，trailers 与正常合并观察仍待完成。

任务工作树中的问答使用以下命令。首次提问和每次续等最多等待 90 秒；同一接收运行累计等待 10 分钟后返回 `deferred`，应结束当前回合等待下一步。答复来源为已批准 main 的有效 Decision、App 的明确回答，或本次会话的明确转述；转述不会发布 Decision／Rule，也不会批准评审。

```sh
towermd ask "是否继续？" --option "继续" --option "停止" --recommended 0
towermd ask --wait <hold-id>
towermd ask --reply <hold-id> --answer "人的原话"
```
提问后 CLI 会立即在 stderr 显示 `hold=H-…`。在 App 打开同一项目，进入“本次问答”，输入该 ID 读取原问题，填写答案后显式发送。失败时重试原请求；恢复前的历史问题目前只读，不支持新答复。


返回值中的 `current` 是当前可读状态，`receipt` 是原请求的不可变回执；历史答复的 `actionable=false`。遇到不确定结果，应保留错误给出的 `--request-id`、原输入及原 `--run` 重试；首次提问与随后等待各有独立 UUID。等待和后台关注不会自动批准、生成完成证据或复活过期运行。Ask 的 App 本次答复已接入；插话投递仍待后续接入。

同一 clone 的工作树共享 `git-common-dir/towermd` 下的持久化 hub。事件先落盘再响应，重启重放状态；相同项目 ID 的另一个 clone 会被拒绝，避免生成两个定序源。用户级运行目录保存实例锁、主机 ID 和 clone 绑定，不修改全局 Git 或 harness 配置。测试或隔离环境可显式设置 `TOWERMD_RUNTIME_DIR`，不要为同一个生产项目配置多个运行目录。

并发首次调用会隔离守护进程继承的客户端文件描述符，避免客户端退出后仍等不到输出结束。

`towermd daemon status` 查询服务；`towermd daemon stop` 停止服务，不删除 hub。daemon 管理命令只连接已经运行的服务做隐式续租，不会因为查询而重新启动已停止的服务；显式 `--run` 则仍要求连接服务并严格校验运行。停止服务不会自动释放运行，停机期间不做隐式续租；再次调用项目命令会重启服务并检查租约期限。

本机 `/events` WebSocket 可用 `directory`、`after` 与运行描述文件中的 `instance` 参数订阅指定项目：先返回水位之后的历史事件，再持续发送新事件。它不开放任意写入，游标不能超过 hub 连续水位；游标落后于压缩水位时返回 `SnapshotRequired` 并关闭流，须从返回的检查点恢复后继续订阅。

## 快照与恢复请求（M0-3）

```sh
towermd snapshot
towermd hub rebuild
```

`snapshot` 由守护进程持主干租约执行，生成 `docs_dir/state/state.json` 与按月 JSONL 归档，并在成功提交后确认 hub 水位。有业务变化时默认每 30 分钟自动快照；可在权威 main 的 `.towermd/config.yaml` 设置正整数秒 `snapshot_interval`。纯续租心跳不归档，也不保留已省略心跳的历史 eid/seq；非心跳事件的去重索引留在 hub 检查点，恢复时从归档推导，`state.json` 只保存合并的心跳省略区间。主干租约 TTL 为 2 分钟，每 30 秒续租；普通 daemon 重启不作废仍有效的租约。

已核实的合并、解除中断、提案提交和基线出版事件另有固定 25 秒首窗口，后续事件不会将它推迟；正常本地成功路径以事件后 30 秒内完成实际 Git 写入及 hub 确认为目标。纯心跳仍按周期压缩。失败时按退避重试，一个失败周期只提醒一次，实际持久化与确认的证据齐备后才关闭提醒；重启后继续核对原提醒。

`status.snapshot` 在活动、待恢复与来源不可用时均提供 `last`、`deferred_reason`、`deferred_since`；最后写入时间未知时 `last.written_at` 为 `null`。状态查询不会推迟已安排的快照，迟到的旧结果也不会覆盖更新的成功或失败事实。只读来源缓存可删除重建，读取失败会显示诊断。

本地 hub 使用逐条追加并同步的 `journal.jsonl`，正常命令只应用新增事件；确认快照后压缩已覆盖的日志内容。首次打开旧 `journal.json` 时会严格校验并迁移，旧文件保留为备份。订阅游标落后于压缩水位时返回 `SnapshotRequired` 并关闭流，客户端须从返回的检查点恢复后继续订阅。写入或同步失败若提示“结果不确定”，须关闭并重新打开 hub 后协调；领取会保留实际 eid 与运行 ID，不能另造事件冒充回滚。非心跳索引和业务归档仍随业务事件增长。

有 `origin` 时在 fresh fetch 的 main 上生成进程署名提交并快进推送，不移动本地分支或工作树；请确保 fetch/push URL 指向同一仓库。无远端时，main 未检出采用带旧值的引用更新；main 在任一工作树已检出时，快照以 CAS 写入非分支引用 `refs/towermd/snapshot`，不移动任何用户分支，也不写用户工作树或 index。读取与恢复统一验证 main 和私有引用的状态、完整月归档与提交 trailer，按 `(writer_epoch, through_seq)` 选择较新者；同水位内容冲突或私有项目上下文不相容时拒绝写入。主干再次可写时，写入器即使没有新业务也会搬送私有快照，仅叠加状态与全部归档并保留新主干代码；同租约且内容不变时保持原水位，新持有者使用当前租约推进新边界。远端非快进竞争最多尝试三次，每次重新 fetch、检查权威上下文与租约并基于最新主干重建；权限、认证、网络等错误不重试。归档验证和 Git 操作在会话锁外进行，期间状态读取与续租仍可完成。`written` 才代表新快照已写入。`init` 的远端预检先 fresh fetch，再用远端 main SHA 做 dry-run；网络、认证、空 origin 等不可用仅警告，只有明确服务端策略拒绝才在生成文件前失败。快照写入时仍重新检查。dry-run 只检查当前连接与推送协商，**不是服务端保护规则允许真实推送的保证**，不会实际推送初始化内容。

`hub rebuild` 只登记待 App 确认的恢复请求，不续租任务、不创建或修改 hub，也没有 `--approve`/`--yes`。请求原子保存五分钟，重复读取不延长；daemon 已提供当前实例绑定、严格 App 通道与仅 human actor 的确认端点，路径、来源与期限均取保存记录。hub missing／empty 且有合法快照时，`status` 返回只读 `recovery_required` 与来源任务摘要；损坏或仍有历史的 hub 不作为空目标覆盖。桌面确认界面与真人验收尚未接入，暂勿手动删除实际项目的 hub 数据。

核心层另已建立共享验证/评审/出版数据格式与稳定摘要、项目绑定的 pending/confirmed 耐久发件箱，以及纯 Git tree/基底链守卫。发件箱核对真实完整回执，确认落盘失败时本次请求保持结果不确定。只读历史解析器已核对实际 main／私有快照、完整归档与原接受回执，提供确定性的 Task 合并和基线出版证明及 canonical 事件构造；冷 clone 中缺失原提交的 G5 通过实际可达树严格复核，统一 footer 分类区分快照、Task 与出版。验证使用契约历史 fixture，不代表真实 App／生产者验收。冷启动证据与基线草稿／审阅／出版生产者已接线，任务后台验证生产者已接线，合并完成生产者及完整 App 工作流仍待后续阶段；这些底座本身不增加任意完成或批准入口。

项目会话已接入完整恢复历史与实际来源证明：来源变化时拒绝交付未核状态，保留原请求和事件身份；恢复时核对 Task/Run 终态，并将最终检查点、精确确认和尾事件一次持久安装。App 确认先把跳过 2^32 的新 seq／epoch 边界写入 main 或私有引用，再安装本地状态；旧租约失效，原运行释放，历史 eid 保持幂等。Git 边界已持久而本地安装失败时保持拒写、显示实际 commit，旧请求不能重用，须读取更高来源重新请求确认。原 Project／writer／文件锁在已注册项目中复用，冷恢复只在成功后发布项目；后台证明与运行租约维护分开。

## 候选画布与冷启动扫描（M0-4）

```sh
towermd baseline scan
towermd baseline scan --history-limit 100
towermd baseline scan --wait <scan-id>
```

扫描固定权威 main 的提交与 tree，返回已跟踪文件盘点、目录候选模块、最多 200 个非合并提交的共变统计、确定性候选画布与测试计划。浅克隆不使用残缺历史修正分区。覆盖率只针对响应列出的已识别源码；未识别文件、未划分项和诊断仍保留。

新扫描立即返回 `scan_id` 与 `queued`，后台执行已识别且无需确认的测试命令。`--wait` 最多等待90秒，可使用同一 ID 重复等待；不能同时指定 `--history-limit`。只有共享验证报告与每个模块的 Evidence 均已耐久记录后才返回 `completed`；它表示扫描完成，测试结果仍分别为 `passed`、`failed` 或 `no_tests`，不认证任何 Task 完成或批准基线。待确认或无法可靠解析的命令不会执行。扫描不接受批准或任意输出路径参数。

扫描规划使用稳定可读的模块 ID，优先按上级目录分组，必要时递归均衡为每层 3–9 项；无法满足下限时给出诊断。模块自有命令优先，根命令只供没有自有发现的模块共享，同一工作目录与参数组合只列一次。未确认命令继续保留为候选；补测建议的允许路径取自实际模块路径。

验证从指定提交创建独立 detached worktree，继承工具链环境和网络，在本机缓存封存日志，并在超时或取消时回收进程组；用户工作树和 index 保留。共享命令全批只运行一次，各模块引用同一报告。重启后从已确认报告继续记录或重建派生草稿，不重新执行已记录验证；只有输入而没有耐久报告时明确标记 `interrupted`。无法确认旧进程身份时保留诊断与目录，不据旧 PID 杀进程。`finish` 尚未接入该执行器。

守护进程的 `GET /canvas`、`GET /objects/{id}` 提供同一权威提交的只读视图，`POST /baseline/coldstart` 提供扫描与等待，`GET /drafts/{scan-id}?directory=<项目路径>` 读取已确认的候选模块原文、画布、未分配项、候选任务和证据引用；均需本机实例认证与项目定位。派生草稿原子写入运行缓存，可由耐久报告重建，不改用户文档或权威基线。Canvas 编解码保留导入原文、未知字段和不透明条目；未知扩展中的有限小数与大数可正常投影，非法几何只使对应节点成为不透明条目，无法表示的数值保留原文并给出诊断。快层状态不改变坐标。候选扫描只读；已保存草稿通过独立的固定 App 端点编辑，不能直接写当前基线。

接口绑定找到对应的 Markdown Interface 对象时正常解析；当前尚未加载 `interfaces.yaml`，找不到对应对象时显示“接口未解析”，保留画布其他诊断，不把清单文件存在视为已加载。

画布投影同时保留对象清单的非法层级诊断；画布缺失、不可安全读取、格式非法或含不透明条目时，已有诊断也会一并返回。

在完成扫描后，由已有 Agent 会话按唯一 schema 综合 S6 JSON，再在同一项目内运行：

```sh
towermd baseline schema > /tmp/towermd-s6-schema.json
towermd baseline submit draft.json
# 不确定时保留同一文件和运行，使用错误中返回的原 UUID 重试：
towermd baseline submit draft.json --request-id <原请求UUID>
```

App 打开返回的草稿 ID 后，可添加／修改／删除文本、链接、分组和普通关系，拖动单个节点、重命名模块并添加引用。编辑带 expected revision，冲突后刷新并重新审阅；不会自动重放旧编辑。草稿保存在私有 Git 引用，重开保留原文、坐标和未知扩展，尚未批准时不写用户文档。

已提交的 S7 源码还可展开“模块、接口与验证命令”，执行合并模块、拆分模块、新增／编辑接口、删除接口和确认验证命令五项结构操作。拆分时逐项分配原路径、验收与接口端点；无法安全处理的关系会返回具体原因。真实 Interface 的标题持久写入绑定关系标签，缺少标题时回退到接口 ID；接口编辑或改名同步标签，普通关系的自定义标签保留。

草稿中新建后又删除的接口、拆分后又合并的临时模块会取消创建，不再生成源中不存在的文件删除，从而保留正常保存与出版路径。实际源对象的删除仍然保留；既有历史草稿的原 manifest、内容哈希和原请求回执不改写。

验证命令按工作目录、程序和每个参数分别填写，保存时不执行。旧展示字符串或不可读取的验证字段保留原文，须明确点击“手动配置替换验证命令”再重新填写；不会自动拆分展示串，也不会默认保存为空列表。缺失的验证字段与显式 null／旧文本分别投影，避免误清空。

原请求状态读取遇到 IO 或恢复来源竞争时，App 保留原 UUID 与正文的待确认队列，不将它显示为确定拒绝；由人明确点击重试原请求。重试不会替换成当前草稿内容或自动发送另一笔编辑／决定。

“审阅与人工决定”显示精确五字段绑定、前后差异、问题／风险、接口影响和实际验证证据。只有明确批准当前内容后，持租约 writer 才尝试出版 B0；拒绝不激活基线。批准回执与出版状态分开显示。无远端且 main 在任一工作树检出时保持待出版，界面给出切换分支提示；用户自行释放 main 后可重试原请求，重开项目也会协调已有批准。CLI 不提供批准命令。

当前开发切片不代表完整 M1：完整桌面交互、从当前基线创建草稿、Bn／提案、技能、任务完成证据和完整里程碑验收仍待完成；`check` 已可用，`finish` 与自动合并拦截尚不可用，现阶段的合并仍须人检查执行。

## 文档

- [设计原则](docs/product-principles.md)
- [MVP 规格 v0.5](docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md)

规格中引用的研究与会议原文位于同级目录 `~/git/sotvault`，链接按相对路径 `../../../sotvault/` 指向那里。
