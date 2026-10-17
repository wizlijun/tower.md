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

开发中的 Tauri App 可选择 Git 项目，并在明确点击后初始化项目。已有初始化配置不再询问文档目录；新项目显示文档目录与受保护区域说明，保留 CLI 诊断并提示检查未提交文件。初始化不自动提交、切分支或批准基线。

`towermd daemon ensure` 可从非 Git 目录启动或发现本用户唯一守护进程，返回真实 endpoint；重复或并发调用复用同一实例，不续租任务或投递消息。App 对 daemon 的实际启动与纯展示读取接线仍待后续实现。

当前仍是开发预览：已有 arm64 debug 候选已实测“打开项目→系统文件夹选择→取消返回”，并在自有临时 Git 项目中通过指定文档目录初始化，原提交、分支和暂存内容保持不变。已有配置的重新选择路径因桌面工具故障尚未验完；daemon 展示、画布与人工放行仍待验收或接入。这不是发布包验收。

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

`node scripts/build-app.mjs --target aarch64-apple-darwin` 可顺序准备同架构 CLI、构建前端和 App bundle；Intel 使用 `x86_64-apple-darwin`。入口检查仅 ad-hoc 的配置并过滤签名环境，前序失败或缺少 CLI／前端产物时停止。Tauri 原有前端构建 hook 保留，可能再次构建前端；完整 App 和源码外运行仍待验收。

版本与双语日志轮转、冻结 tag 后，可用 `node scripts/release/build.mjs --root . --tag v0.1.0 --target aarch64-apple-darwin --output /private/tmp/towermd-arm64-release` 编排锁定的 App／DMG 构建；Intel 换用 `x86_64-apple-darwin` 和另一全新输出目录。工具核对 tag、源码和锁文件，隔离目标输出，校验 CLI 架构及合并后的 ad-hoc 配置，并保存构建收据与目录外日志；现阶段仅工具已验，尚未冻结首版或生成发布包。

两架构构建及原生技能准备完成后，可用 `node scripts/release/assets.mjs --root . --tag v0.1.0 --arm-build /private/tmp/towermd-arm64-release --intel-build /private/tmp/towermd-intel-release --output /private/tmp/towermd-release-assets` 检查静态包、冻结来源和收据，组装四个 App ZIP／DMG、技能包、manifest、双语说明及 SHA256SUMS。输出目录必须全新且在源码外；技能缺项、架构／依赖／签名错误或资产漂移会拒绝。该工具不启动 App，内嵌前端字节、安装、真人与 Intel 实机仍须另行验收。

同架构运行 `node scripts/project-pnpm.mjs tauri build --debug --bundles app --no-sign` 构建无发行者签名的开发桌面壳；跨架构构建时先 stage 对应 CLI，再给 Tauri 显式传相同 `--target`。可能存在 linker/ad-hoc 签名，尚无发行者签名或公证。桌面 crate 的锁文件与核心 workspace 独立；核心 `cargo test --workspace` 不编译 Tauri。上述 Node、npm、Rust 和 Xcode 是开发构建工具；随包 CLI 的真实 App 运行闭环及 Intel 运行仍待验收。

`node --test scripts/release/*.test.mjs` 验证发布工具的双语日志、版本、ad-hoc 配置、输入范围、包检查、冻结技能归档、资产集合和校验和契约。当前已有校验、输入审计、版本准备、冻结构建与资产组装工具，实际打包与发布仍待完成。

发布输入审计使用 `node scripts/release/audit.mjs scan --root . --output .release/inventory.json` 采集所有工作树、分支未合入提交和 stash。为每项填写范围、理由与证据后，执行 `node scripts/release/audit.mjs check --root . --decisions .release/scope.json` 重新采集并核对；输入漂移或漏项会拒绝。未提交内容须先单独验证提交再重采，补丁等价须附冻结功能测试依据。`.release/` 只存本地审计 JSON，包含本机路径，不放入发行资产。

`node scripts/release/prepare.mjs check --root . --version 0.0.0 --phase prepare` 只读检查当前项目版本、锁与双语日志。版本准备会定点同步本项目版本并轮转日志；离线缺包提供受限诊断，中途失败恢复本次写入，额外改动保留并报告冲突。实际版本轮转须等功能和发布前置验收完成后执行。

## 开发与初始化（M0）

冷启动已具备按固定提交、扫描选项和历史可用性重建的纯草稿数据内核，可生成相互对应的 Module 文档与 Canvas 字节，保留 Unicode 模块 ID。生产草稿目录出版、读取端点、后台扫描及验证证据接线仍待完成；当前 CLI 的准备操作不执行 S5 测试。

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
towermd status
towermd task release
```

`--allow`、`--acceptance`、`--touches`、`--depends-on`、`--decision` 可重复。任务允许路径必须能证明是模块路径的子集，且不能包含受保护目录；目前对子集判断采用保守规则（相同 glob、明确文件和目录前缀 `/**`），无法证明时拒绝。显式决策引用必须存在；没有相关决策时不要求虚构一个。未合并依赖阻止领取。

显式非法的 `layer` 保留原文并返回路径诊断，按慢层保护；缺失字段才采用已知类型的默认层级。目录外或没有 ID 的非法层级对象也受保护，出版与删除仍要求合法慢层资格。

必须先建目标工作树及 `t/<task-id>` 分支再领取；一次领取原子取得全部模块租约。一个工作树只能有一个活动运行；`task show` 默认按本机、工作树、分支选择，`--run <id>` 可显式覆盖。用户命令为调用工作树的绑定运行续租，TTL 15 分钟；`init` 不参与续租。长期操作前后应调用 `status`，超时或主动 release 后任务回到就绪。未知费用保持 `null`。本阶段没有完成、批准或放宽路径的命令。

无法确定运行绑定时，提示只列本机仍在运行的 Run；`status` 保留已释放、已到期及其他主机的运行历史。

领取时可同时提供 `--harness <名称> --harness-version <版本>`；只记录为技能自报信息，不改变 `process:towermd-cli` 的事件署名。任务卡包含权威 main 的验收、验证命令、接口与决策上下文。

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

本地 hub 使用逐条追加并同步的 `journal.jsonl`，正常命令只应用新增事件；确认快照后压缩已覆盖的日志内容。首次打开旧 `journal.json` 时会严格校验并迁移，旧文件保留为备份。订阅游标落后于压缩水位时返回 `SnapshotRequired` 并关闭流，客户端须从返回的检查点恢复后继续订阅。写入或同步失败若提示“结果不确定”，须关闭并重新打开 hub 后协调；领取会保留实际 eid 与运行 ID，不能另造事件冒充回滚。非心跳索引和业务归档仍随业务事件增长。

有 `origin` 时在 fresh fetch 的 main 上生成进程署名提交并快进推送，不移动本地分支或工作树；请确保 fetch/push URL 指向同一仓库。无远端时，main 未检出采用带旧值的引用更新；main 在任一工作树已检出时，快照以 CAS 写入非分支引用 `refs/towermd/snapshot`，不移动任何用户分支，也不写用户工作树或 index。读取与恢复统一验证 main 和私有引用的状态、完整月归档与提交 trailer，按 `(writer_epoch, through_seq)` 选择较新者；同水位内容冲突或私有项目上下文不相容时拒绝写入。主干再次可写时，写入器即使没有新业务也会搬送私有快照，仅叠加状态与全部归档并保留新主干代码；同租约且内容不变时保持原水位，新持有者使用当前租约推进新边界。远端非快进竞争最多尝试三次，每次重新 fetch、检查权威上下文与租约并基于最新主干重建；权限、认证、网络等错误不重试。归档验证和 Git 操作在会话锁外进行，期间状态读取与续租仍可完成。`written` 才代表新快照已写入。`init` 的远端预检先 fresh fetch，再用远端 main SHA 做 dry-run；网络、认证、空 origin 等不可用仅警告，只有明确服务端策略拒绝才在生成文件前失败。快照写入时仍重新检查。dry-run 只检查当前连接与推送协商，**不是服务端保护规则允许真实推送的保证**，不会实际推送初始化内容。

`hub rebuild` 只登记待 App 确认的恢复请求，不续租任务、不创建或修改 hub，也没有 `--approve`/`--yes`。请求原子保存五分钟，重复读取不延长；daemon 已提供当前实例绑定、严格 App 通道与仅 human actor 的确认端点，路径、来源与期限均取保存记录。hub missing／empty 且有合法快照时，`status` 返回只读 `recovery_required` 与来源任务摘要；损坏或仍有历史的 hub 不作为空目标覆盖。桌面确认界面与真人验收尚未接入，暂勿手动删除实际项目的 hub 数据。

核心层另已建立共享验证/评审/出版数据格式与稳定摘要、项目绑定的 pending/confirmed 耐久发件箱，以及纯 Git tree/基底链守卫。发件箱核对真实完整回执，确认落盘失败时本次请求保持结果不确定。只读历史解析器已核对实际 main／私有快照、完整归档与原接受回执，提供确定性的 Task 合并和基线出版证明及 canonical 事件构造；冷 clone 中缺失原提交的 G5 通过实际可达树严格复核，统一 footer 分类区分快照、Task 与出版。验证使用契约历史 fixture，不代表真实 App／生产者验收。CLI／daemon 的耐久生产者及 App 接线仍待后续阶段；这些底座不增加完成或批准入口。

项目会话已接入完整恢复历史与实际来源证明：来源变化时拒绝交付未核状态，保留原请求和事件身份；恢复时核对 Task/Run 终态，并将最终检查点、精确确认和尾事件一次持久安装。App 确认先把跳过 2^32 的新 seq／epoch 边界写入 main 或私有引用，再安装本地状态；旧租约失效，原运行释放，历史 eid 保持幂等。Git 边界已持久而本地安装失败时保持拒写、显示实际 commit，旧请求不能重用，须读取更高来源重新请求确认。原 Project／writer／文件锁在已注册项目中复用，冷恢复只在成功后发布项目；后台证明与运行租约维护分开。

## 候选画布与冷启动扫描（M0-4）

```sh
towermd baseline scan
towermd baseline scan --history-limit 100
```

扫描固定权威 main 的提交与 tree，返回已跟踪文件盘点、目录候选模块、最多 200 个非合并提交的共变统计、确定性候选画布与测试计划。浅克隆不使用残缺历史修正分区。覆盖率只针对响应列出的已识别源码；未识别文件、未划分项和诊断仍保留。

结果仅为 JSON 预览，不创建模块、任务或证据，不写 `tower.canvas`，不提交基线。S5 始终未运行：`not_run` 表示发现命令，`no_tests_discovered` 表示扫描未发现测试，`unsupported` 表示不能可靠解析；都不是通过证据。扫描的实际执行与证据写入仍待接线，不接受执行、批准或任意输出路径参数。

扫描规划使用稳定可读的模块 ID，优先按上级目录分组，必要时递归均衡为每层 3–9 项；无法满足下限时给出诊断。模块自有命令优先，根命令只供没有自有发现的模块共享，同一工作目录与参数组合只列一次。未确认命令继续保留为候选；补测建议的允许路径取自实际模块路径。

共用验证内核已实现：从指定提交创建独立 detached worktree，继承工具链环境和网络，在本机缓存封存日志，并在超时或取消时回收进程组；用户工作树和 index 保留。纯聚合内核可将同一报告汇总为独立模块结果，共享命令引用同一执行与日志，无可执行命令明确为无测试。执行与聚合尚未接入 `baseline scan` 或 `finish`，不代表 S5 已运行。

守护进程的 `GET /canvas`、`GET /objects/{id}` 提供同一权威提交的只读视图，`POST /baseline/coldstart` 提供扫描；均需本机实例认证与项目定位。Canvas 编解码保留导入原文、未知字段和不透明条目；未知扩展中的有限小数与大数可正常投影，非法几何只使对应节点成为不透明条目，无法表示的数值保留原文并给出诊断。快层状态不改变坐标；当前没有 UI 编辑或画布写回入口。

接口绑定找到对应的 Markdown Interface 对象时正常解析；当前尚未加载 `interfaces.yaml`，找不到对应对象时显示“接口未解析”，保留画布其他诊断，不把清单文件存在视为已加载。

画布投影同时保留对象清单的非法层级诊断；画布缺失、不可安全读取、格式非法或含不透明条目时，已有诊断也会一并返回。

当前已实现初始化、核心文档、权威对象读取、本地 hub、任务命令、快照及候选扫描。S5 实跑、技能和 App 仍属后续工作，整个 M0 尚未完成。没有自动合并拦截器，`check`、`finish` 与 App 批准尚不可用；现阶段的合并仍须人检查执行。

## 文档

- [设计原则](docs/product-principles.md)
- [MVP 规格 v0.5](docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md)

规格中引用的研究与会议原文位于同级目录 `~/git/sotvault`，链接按相对路径 `../../../sotvault/` 指向那里。
