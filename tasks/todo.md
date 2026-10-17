# 任务计划

## 2026-10-17 持续开发至首个可用 App（未签名）

用户追加目标：“请一直继续开发下去，直到发布第一个可用版本app，先不签名”。先完成下方 M0 整改，再按已批准规格推进 M1 技能与 App，完成首个可安装 macOS 版本；签名与公证暂不做。发布前核对所有 worktree、分支、stash 与冻结源码/验收证据，精确提交并更新双语 CHANGELOG。整改阶段继续不推送，发布动作以实际完成的可审阅产物为准，不把计划或工作区测试称为发布。

- [ ] 完成 M0 整改各批与真实 daemon 验收
- [ ] 规划并实现 M1 核心人工闸门、技能/CLI 与桌面 App 的可用闭环
- [ ] 实机验证 App、打包未签名安装产物、核对发布证据与发布记录

当前推进：P6基础Tasks1–3、workflow Task2维护Ensure与P8发布工具Tasks1–3已精确合入；E1b恢复安装窗口正在Q3修复。后续F/G与依赖其实际API的功能须等恢复双审合入后接线；批4验证器仅提前只读映射，不实施。实际版本仍0.0.0，未冻结tag或发布，App完整闭环和实机验收仍待完成。

- [x] P8 Task 1：纯发布门禁行为红→绿、冻结源码与证据
- [x] P8 Task 1：独立双审、精确合入与记录；不冒称已打包/发布

P8 Task1 回顾：实际合入 `da44ba6dcd88a5c31c8cd8913420c70ddb88b4d6`，来源 `5e199eac8c23ffacd5cb7ae26b1cffa26a983de4`，base `f73641d6`、tree `abe16958`；仅新增两个纯脚本文件。可载入 stub 行为红78项（70失败），最高版本边界补测真实红80项（1失败），最终80/80绿；独立SPEC和其后fresh quality分别80/80绿、C/I/M均0/0/0、Ready Yes。manifest `/private/tmp/towermd-release-core-task1-evidence/manifest.json` SHA256 `a5d44b0a6274a79c78bb7547a32940e332e820c7ca0df4accb12e037a2c28a42`；Root核两Git blob/父/tree及51项artifact，合入字节等同冻结。报告 `/private/tmp/towermd-release-core-task1-{spec,quality}-evidence/report.md`。无真实构建、网络、secret读取或发布动作；P8其余任务及版本冻结仍未完成。

P8 Task2 继续按已批准发布计划实施，独立 writer 仅新增 `scripts/release/audit.mjs`、`audit.test.mjs` 并精确增加 `.release/` ignore hunk。先用真实临时 Git 仓库证明 worktree/dirty/untracked/branch/stash 漏项与范围漂移被拒绝，再实现只读采集、逐项裁决与固定 CLI；文件名按 NUL 解析，测试保留其他工作且不调用破坏性命令。仅 Node 测试、不跑 Cargo、不读取 E1b WIP、不在本阶段对真实发布输入作范围裁决。冻结后 SPEC→fresh quality，双审后才合入。

- [x] P8 Task2：真实临时仓库行为红→绿、只读审计与裁决门禁、固定 CLI
- [x] P8 Task2：独立双审、精确合入与证据；实际发布范围审计仍待冻结前执行

P8 Task2 首候选 `8f29fe1fb5e27c97395e2de764873a8a3424c1f1`（base `8f3973e`，仅3路径）107/107通过，Root核manifest SHA `ed4072326fd18ddf09d050689eda1f13aacae65b50b5394be746b5d14133fe50`、三Git blob和7份原命令日志。独立SPEC PASS（107+6通过）后质量审查Ready No/C0/I2/M0：短名main被同名tag覆盖会漏未合入commit；继承Git定位环境时显式root A可成功扫描B、漏A dirty。独立5项3通过2行为失败，报告 `/private/tmp/towermd-release-audit-task2-quality-evidence/report.md` SHA `42136a1724584b4a63f15aec73b8647b08c895aa6e359d3289d74fea8f7f3d63`。审查STOP，当前不合入。原writer仅迁移相关查询为明确main引用/已采commit，并在Git子进程剔除重定向变量，保正常环境和global/system配置；自建双仓库/tag夹具红→绿、必要Node回归后精确Q1冻结，再SPECdelta→qualitydelta，旧证据保全，不扫描用户真实stash。

Task2 完成回顾：原实现实际合入 `12b409b`，Q1合入 `f8bd34d`，最终来源 `bde2b4e0d5133c721a6862dcf45a5b248e7158f8`；Root核全部3路径逐字节等同冻结。Q1真实29项2红/30项3红→110/110绿，原5独立探针绿；独立SPECdelta 122/122、原qualitydelta 115/115绿，C/I/M均0/0/0、Ready Yes。Q1 manifest SHA `e1dc6fcdeced519dcdb5ecd5002f04bade6aedd6a83f967f9e2d9914283f7505`，Root核新旧Git blob/5日志/原12证据清单；最终报告 `/private/tmp/towermd-release-audit-task2-q1-{spec,quality}-evidence/report.md`。本地JSON仅用于输入核对，不进入发行资产；尚未执行真实发布范围采集与裁决，没有版本轮转或发布。

P8 Task3 可与只读审计独立实施，唯一 writer 仅新增 `scripts/release/prepare.mjs`、`prepare.test.mjs`，消费已合入的 core 门禁。真实文件写入/锁更新仅在其自建 fixture；本仓库只读 check，不轮转实际 CHANGELOG、不改现有版本或依赖。以行为红→绿验证白名单定点版本更新、锁与 metadata 契约、双语轮转及重复执行拒绝；验证后精确冻结、SPEC→fresh quality再合入。实际 0.1.0 准备仍须全部前置验收。

- [x] P8 Task3：版本与日志准备工具的 fixture 行为红→绿、只读实际 check
- [x] P8 Task3：独立双审、精确合入与证据；不提前轮转或冻结 0.1.0

P8 Task3 首候选 `13d34c53986c3a849037cb568ce7a09bfde0144f`（base `486b24c`，仅2文件）129/129通过、实际0.0.0只读check成功、11项版本/锁/日志不变；Root核两Git blob、source manifest SHA `0efcc3c92cada0bbcf5975ac12b2ab40abe3ed3d1e02c63a92e3d13b2d55a21c`及22项证据。独立SPEC NOTPASS C0/I1/M0：离线runner明确报缺包时仍仅返回退出码，缺Task3要求的具体缺项诊断。独立129绿及1项诊断行为红并存；报告 `/private/tmp/towermd-release-prepare-task3-spec-evidence/report.md` SHA `5c4dd416fb420781be64a55a19c692c2c34b8f84943cdab147a8d547ada4ccec`。审查已STOP；原writer仅补受约束的离线缺包/版本诊断和测试，保持非零退出码及敏感输出遮蔽，所有写入仍仅自建fixture。Q1须行为红→绿、必要Node回归、精确冻结后SPECdelta→fresh quality，不进入质量或合入当前候选。

Task3 Q1冻结 `768cbbc3e08a07deec24c47ce949ac2bbda7912a` 后142项与原探针1项绿、SPECdelta PASS；source manifest SHA `189baba76b118ed57de379d34b357fa60518f2354c5e14c4028b60a290d6b02f`，Root核2文件及旧22项证据不变。fresh quality Ready No/C0/I1/M0：四manifest写入后offline失败会留下新manifest/旧锁，补齐缓存直接重试在前置检查失败、runner为0；独立4项诊断探针自然绿是准确断言错误行为，不是该产品行为通过。报告 `/private/tmp/towermd-release-prepare-task3-quality-evidence/report.md` SHA `3e47a51b12246e7e2d60c84208b2c3e090e62c3bd0752fccd5115f001d368f2a`，审查STOP。Q2仅补本次白名单版本写入的精确恢复或严格可恢复路径，恢复不得覆盖后来修改；用自建fixture验证写后metadata/锁更新/pnpm失败后同版可重试、保已识别缺项和秘密遮蔽，不删除锁或升级依赖。必要Node回归及只读check后精确冻结，SPECdelta→qualitydelta再合入，实际仓库不轮转。

Task3完成回顾：实际合入 `524361e`（原实现）、`d7cdefd`（安全离线诊断）、`837d009`（精确失败恢复），最终来源 `2e57f306e17e1c131b4b3bd7bf05fafe2911eb06`；Root核2源码等同冻结、原22+Q147证据不变，Q2 source manifest SHA `46b8e20ae494a4a54900160a8261f511d84f85148e7a3c7be468106b8b39b356`。七类恢复/冲突行为先7红→149绿，独立SPECdelta154/154、qualitydelta158/158，均C/I/M 0/0/0、Ready Yes；六个失败阶段可同版重试，额外改动保留并诊断。最终报告 `/private/tmp/towermd-release-prepare-task3-q2-{spec,quality}-evidence/report.md`。所有写入仅临时fixture，实际只读0.0.0检查成功、11输入未变；没有轮转真实版本/日志或发布。

主目录三个发布工具联合验证：source `837d009e6a967843991ca44464ba1423f0360c19` 上core80+audit30+prepare69共179/179自然exit0，6源码hash前后相同。证据 `/private/tmp/towermd-release-tools-integration-evidence/01-node-tests.json`，日志SHA `a11105e8fea7dc72cb7c2c5e9ccf5c02d4598d65f57e4ecaa8f929f32daa0fea`；不包括Rust、App或真实发布验收。

构建环境准备：已为现有 Rust 工具链安装 `x86_64-apple-darwin` 标准库目标，`rustup target list --installed` 确认与 `aarch64-apple-darwin` 均在；本机 Node v26.9.0、Xcode 26.6、macOS 26.5.1。仅完成环境准备，尚无 Intel/Apple Silicon App 构建、Intel 实机验收或发布产物。

App 基础实施计划：`docs/superpowers/plans/2026-10-17-m1-app-foundation.md`，提交 `6512179`。已对照实际 CLI/daemon、Note.md 工具线独立审查并修正项目 pnpm 路径、租约 epoch 与连接代际的区别、画布 opaque 条目解码，复审 PASS。当前仅完成计划，尚未实现或构建 App。

P6 人工闭环详细计划为 `docs/superpowers/plans/2026-10-17-m1-app-workflows.md`，提交 `a63d1af`，同时最小同步基础计划的启动与纯展示接线。独立审查及措辞 delta 复审 PASS：Ensure 复用唯一 daemon 启动原语，projection/object/events 展示不续租或消费消息；原请求协调、S7/S8、恢复确认、无 Run 违规处理和真人交接均有明确所有者。14 个实施 Tasks 尚未执行，不将文档审查称为 App 功能或真人验收。

P6 workflow Task2（Ensure）独立启动：E1b writer已冻结STOP，退出daemon共享文件；现有main的唯一 `connect(true)` 启动原语与native固定 `CliAction::Ensure` 已实际只读核实。独立writer仅补CLI/daemon维护入口及真实 `app_read_cli` 测试，复用native既有实现不造第二启动器；非Git app-data cwd启动、重复/并发同pid/instance、真实Run lease不变、无command/inbox/delivered。先可解析stub真实行为红→最小绿，保正常CLI活动续租与原startup/FD测试。独立worktree不读或修改E1b冻结源码；与E1b联合验证必须在两者实际合入后执行，当前不宣称接收未合入的恢复API。Rust验证与E1b复审直接协调，不能通过串行测试/放宽时限掩盖问题。冻结后SPEC→fresh quality再合入；展示/人工能力仍待后续。

- [x] P6 workflow Task2：固定维护Ensure、真实无副作用启动/复用/并发回归
- [ ] P6 workflow Task2：独立双审、精确合入及与实际E1b联合验证

Ensure候选已冻结 `c358585d5945d6a9ab8ba4c31108bdb29d452555`（base f6c43fc，tree72c07ac7），仅main/daemon维护入口与真实app_read_cli三路径；Root核125源码blob、32证据及staged CLI hash。manifest SHA `d7f08fbfa6ab65ab546a6747fd91d6b814715fc8f9d5dd8beafec5f12c5ea680`；作者15 unique回归及fmt/Clippy绿。独立SPEC PASS/C0I0M0，新target实际3/3自然exit0，报告 `/private/tmp/towermd-app-ensure-task2-spec-evidence/report.md`；fresh quality已启动。尚未合入、与E1b未联合，不冒实际Tauri app-data接线或GUI验收。

Task2已实际合入 `3cc37c8`，三路径逐字节等同冻结。独立fresh quality最终Ready Yes/C0I0M0，9 unique回归、fmt/diff通过；报告 `/private/tmp/towermd-app-ensure-task2-quality-evidence/report.md` SHA `66e8b51ce0e7e2ef1b0e35bcaf8504f037f73f1be2d986b7e8168b41b5858b15`，Root核其25证据hash，源和原证据不变。首次loopback EPERM分类保留、精确升级同三项绿；没有降低线程或测试时限。双审与合入已完成，第二条因实际E1b联合仍待而保留未完成；不将原基线15绿冒联合或实际App启动接线。

P2 任务证据与 P3 人工干预详细计划已写入 `docs/superpowers/plans/2026-10-17-m1-task-evidence.md`、`2026-10-17-m1-human-intervention.md`。独立联合审查发现并修复：模块 ID 不能随问题文本归一化；人工事实先经恢复发件箱持久化；压缩后的请求幂等由完整归档重建 receipt 索引。复审 PASS。已明确 P2 Task1 → P3 Task1 → P2 后续的真实编译顺序，G3 不可变证据与 G4 独立人工事实；这些仅为实施计划，未实现、未真人验收。

P4 详细计划为 `docs/superpowers/plans/2026-10-17-m1-baseline-approval.md`。独立审查后补齐批准/请求的持久 outbox、ready/running Task 的独立 App 复核解除路径、正常出版与恢复共用 commit 推导的基线 eid，复审 PASS。当前仅完成计划；慢层批准、基线出版和真人操作尚未实现或验收。

P5 详细计划为 `docs/superpowers/plans/2026-10-17-m1-main-observer.md`。独立审查后补齐无 live Run 的 App 转提案上下文、永久精确认领事实与待点击 TOCTOU 的区别，复审 PASS。首次检测 frontier 已同步 FR-103 并独立审查 PASS：首次引入项目 ID 的整笔提交及祖先只作导入，不能批准 B0；其后全部检查，历史不完整则阻认证。2026-10-17 用户明确答复“整笔启用提交作为导入起点（推荐）”，确认此前工程假设，不再作为待答问题。事后检测源码尚未实现。

P8 详细计划为 `docs/superpowers/plans/2026-10-17-m1-unsigned-release.md`，已提交 `aa57256`。独立审查后补齐远端正文/精确资产集合的发布门禁、已有 index 与共享文件逐 hunk 保护，复审 PASS。该提交只有计划文档，尚未实施打包脚本、构建、tag 或发布。

P7 详细计划为 `docs/superpowers/plans/2026-10-17-m1-native-skills-acceptance.md`，已提交 `9a66aaf`。核对本机及官方原生技能契约，独立审查后统一 canonical `merge.done`、将收集器限定为已捕获材料与 Git 只读归并，避免额外续租/抢消息；delta 复审 PASS。实际结构为 8 Tasks、57 个未执行复选框、10 张任务卡。技能实现、安装实测、真人 App 与三树双 harness 十任务均未执行，不把计划当验收证据。

E/P2/P3/P4 共享恢复证明契约为 `docs/superpowers/plans/2026-10-17-shared-recovery-proof-contract.md`。独立审查后明确原 confirmed 与恢复 replay 分角色、成功回包前的耐久协调、后台业务 producer 显式 outbox 接线和完整 guards epoch 高水位，delta 复审 PASS。统一任务/出版证明及持久 schema 仍待实施，不把文档审查当 producer 或真人证据。

共享契约无环收口已提交 `17aedea`（七文档/规格同步），独立审查修复持久请求 envelope 缺口后 delta PASS。E 拆 E0 唯一 DTO/outbox/纯 raw-G5/hash → E1 已核历史/canonical proof/业务恢复，各自双审；批4/P2/P3/P4 producer 重导出并复用同一格式。完整 verifier report、typed receipt、原 opening/review/gate 上下文已冻结，合法稀疏 confirmed 不要求新增 renew/逐序覆盖日志；E0/E1、producer和真人仍未实现。

新增稳定性前置：批次 1 全量再次复现 daemon 继承非 CLOEXEC 管道，客户端已退出但 `Command::output` 无法收到 EOF。按持续发布目标修复子进程 FD 继承，先补稳定复现再验证；该次挂起不计通过。诊断 `/private/tmp/towermd-m0-fix1-hang-sample.txt`。

## 2026-10-17 M0 整改（D0–D6 已批准）

执行依据：`docs/superpowers/plans/2026-10-17-m0-remediation.md`。用户于本会话明确答复“全部按推荐方案执行”：D0 四批提交现状（不含 S5）；D1 私有快照引用；D2 App 确认恢复端点；D3 临时分离工作树验证、继承环境且允许网络；D4 init 尽力预检；D5 按表同步规格；D6 增补 M0 有效指标。不再重复询问这些裁决。本轮不推送。

- [x] 阅读整改总表、经验和现状，记录人的 D0–D6 裁决
- [x] D0：按精确路径分四个连续基线检查点提交 M0-1/2/3/4（完整基线为第四个提交）
- [x] 批次 0 余项：R16 同步规格、独立复审、完整基线复验并提交（`b5a881f8d5f8e32a0d17816fc81f12ad2edd3637`）
- [x] 批次 1：R1 离线读取与后台 fetch；R2 init 本地独立和显式目录（`e123b4c`、`683daa1`、`671c802`）
- [x] 批次 2：R6 HubProtocol 与租约事件；R5 追加 journal、压缩与增量投影（`3c937fd`、`363cfee`）
- [ ] 批次 3：R3 私有快照落点；R7 有界重做与连续前缀；R4 App 确认恢复、trailer/发件箱；R14 事件触发快照
- [ ] 批次 4：R8 共用分离工作树验证与异步 scan/wait、逐模块证据；R9 层级草稿与读取
- [ ] 批次 5：R10 clone 重绑；R11 非法层级保护；R12 画布逐条数值处理；R13 状态与接口诊断；R15 文档和提交记录
- [ ] 全量测试、fmt、Clippy、规格与质量复审、真实 daemon 路径验收；逐批记录提交号

本轮此前的沙箱 S5 工作仅保留在 `/private/tmp/towermd-m0-s5`，不并入现状基线；D3 已将其归入后续安全加固。根 README/中英文 CHANGELOG 已恢复到既有 M0 已验收内容，后续按整改结果重新更新。D0 精确路径冻结于 `/private/tmp/towermd-m0-baseline-paths.json`，46 个文件逐一匹配本轮开始 320 项测试通过的工作区。

D0 提交记录（本轮不推送）：M0-1 `be0dff184df47710a8c4a5ac350a69177e6a79cb`（13 文件）；M0-2 `9f9eee60335e4baf485b37dbb0416577202f4801`（13 文件）；M0-3 `fdba1fb24894278a5deaa95830bf7dc19dab2d1b`（9 文件）；M0-4 `d68b961fb4c92989e6eb30eaf516802293f80061`（11 文件）。这四个连续 checkpoint 保存同一已验收工作区的文件分组，只有齐全后才形成完整可构建基线，不把中间 checkpoint 当作独立发布验收。

批次 0 验证：完整四批基线在独立目标目录 `/private/tmp/towermd-m0-remediation-target` 串行运行全部 workspace 测试 **320/320 通过、0 跳过**；fmt 与 Clippy（全部 target、`-D warnings`）通过。R16 的 10 项文本一致性检查已红→绿，独立规格与文档质量复审均 PASS。FR-92 已明确业务投影由 daemon/core 计算、hub 只保存不透明检查点，不重新解析 `data`。

补充诊断：此前 S5 隔离工作树整套测试一次在既有并发首启动测试挂起，发现 daemon 继承测试管道导致 EOF 未到；单项重跑通过，原整套中断不计绿。诊断留存 `/private/tmp/towermd-task2-workspace-hang.txt`，不得把分项结果冒称完整 workspace 通过。

### 批次 1 回顾（已提交，未推送）

- 主工作区精确提交：`e123b4c7f0e479982e8ce1893aa5af1d58cf2d0b`（R1/R2 与 daemon FD 根因修复）、`683daa169436cb25324efd1fdc4e415dd8d68d91`（模板只保留规矩）、`671c8027f442b71a2e4953d72dea5bad81992bcf`（快照发布与缓存一致性）。源码、Cargo、README 与双语日志逐文件等同已审工作树最终树 `d748780`，未混入旧 S5 或原有未跟踪文件。
- 最终默认并发 `cargo test --workspace --locked` **344/344，通过，0 跳过**；fmt、Clippy 全 targets `-D warnings`、diff check 通过。独立规格与质量审查最终均 PASS；质量轮发现的快照缓存 Important 已修复，复验 7/7 通过。全量日志 `/private/tmp/towermd-m0-fix1-cache-workspace.log`。
- 真实 daemon/CLI 证明：10 个并发 status 共用一次后台 fetch；可信 HTTP 时钟推进 20 分钟持续活动仍同 epoch，再闲置 16 分钟确实过期；新 init 非交互缺目录零写失败、无 daemon 续租依赖；远端快照后立即断网仍能 status/show/续租，旧在途 fetch/旧 Catalog 竞争不会使缓存倒退或误拒，真实回退仍拒。
- 首次完整尝试曾因 daemon 继承非 CLOEXEC 管道挂起，保留原始日志/FD/线程栈并补确定性红测，已修复子进程继承及 exec 错误管道回归。不是以重跑掩盖原挂起。模板示例误含 availability 已删除；旧“只有 fetch 可更新 tracking ref”断言按发布后安全 CAS 的新契约调整，全部用户 heads/index/worktree 防护保留。
- 本批没有新增依赖、没有发布 App。批次 2–5 计划与首个未签名 App 执行总表已提交，继续实施，不把计划当成功能交付。

### 批次 2 进行中（隔离源码已提交，待双审与合入）

- 隔离工作树 `/private/tmp/towermd-m0-remediation-2`，基线 `00ac7dc` 已通过默认并发完整测试 **344/344、0 跳过**；日志 `/private/tmp/towermd-m0-fix2-baseline.log`。
- 七操作协议、独立租约事件、JSONL、opaque checkpoint 与常驻会话的生产代码已通过 workspace check。新协议测试含真实 10,000 次逐行追加与 inode 检查；增量会话重复 200 次无新增事件时不重复应用。这些定向结果不代替联合全量验收。
- 原实现者冻结写入后，将 `tests/hub.rs`、`hub_checkpoint.rs`、`snapshot.rs` 的旧契约测试交给专人顺序迁移；其余时间只读自审，不并行修改生产代码。迁移不得削弱旧 fencing、去重、恢复和持久化失败断言。
- 新领取事件的 Run/fences 完整验证与原子投影、压缩失败的写入隔离、v1 Git 归档解码与旧快照标准化均已修复。活动运行索引计数由 2,000,200 降到 200；真实 writer 的可见 103/已确认 101 输入只出版 101，确认后才推进；真实 HTTP owners、WS 压缩游标关闭及写前换主拒绝已有回归。
- 隔离源码冻结提交 `4c18f7af661420d8861af69752cfbb89ee852945`（26 个本批路径，未合入主分支）。最终默认并发完整 workspace **369/369、0 跳过、exit 0**；fmt 与全部 targets Clippy `-D warnings` 通过。最终日志 `/private/tmp/towermd-m0-fix2-workspace-final.log`；此前失败编译/诊断的实际节选与命令保留于 `/private/tmp/towermd-m0-fix2-prior-failures.log`，不计绿。独立规格审查已启动；质量审查须等规格通过，不提前勾选完成。
- SPEC 首轮独立真实复现两处缺口：部分模块换主会阻断其余有效原租约补偿；跨模块 claim 仅校验 guards 自洽却未匹配全部声明模块。修复追加提交 `5fb2207dab9e58b4dcafdbdf3ca8556d1cdea106`（4 个精确路径），真实红→绿，最终完整默认并发 **372/372、0 跳过、exit 0**，fmt/Clippy 通过，日志 `/private/tmp/towermd-m0-fix2-spec-delta-workspace.log`。SPEC delta 独立 22 项相关测试通过，最终 PASS；质量审查已开始。最新冻结源码为 `5fb2207d`，仍未合入主分支。

### 批次 2 回顾（已提交，未推送）

- 已双审并精确合入 `3c937fd`（协议/存储闭环）、`363cfee`（两处 claim 安全修复）。全部 crates、Cargo、README、双语日志和规格与冻结 `5fb2207d` 逐文件等同；未混入其他任务或原有未跟踪文件。
- 最终完整默认并发 372/372、0 跳过；fmt、Clippy 通过。独立 SPEC 修后 22 项、质量审查 30 项定向测试全绿，质量结论 Ready to merge: Yes，无 Critical/Important。
- 保留限制：非心跳 eid/业务归档随真实事件增长；当前仅持续续租时，ack 之间 tail 仍会增长。批 3 H4/H6 已补周期 checkpoint 条件：心跳不触发 25 秒快照，但达到正常 interval 且存在未覆盖 tail 时实际持久并 ack；空 tail/仅自身快照事件不得循环。此项尚未实现，不把批 2 当作长期心跳存储已完全有界。

### 批次 3 进行中

- 独立工作树 `/private/tmp/towermd-m0-remediation-3`，基线 `e6cd45a`；默认并发完整基线 372/372、exit 0，日志 `/private/tmp/towermd-m0-fix3-baseline.log`。独立 target 为 `/private/tmp/towermd-m0-fix3-target`。
- 接口核对后按实现者的上下文规模反馈拆成五个顺序闭环：3.1 A/B 私有快照基础 → 3.2 C/D 搬送与竞争 → 3.3 E 事实/outbox → 3.4 F/G App 恢复 → 3.5 H/V 调度收口。每个闭环独立双审与精确提交；总体仍未完成。3.1 必须同步所有现存恢复读路径，不能提交私有来源不可恢复的半成品。
- 3.1 已双审并合入主分支：`81266e4e051788f935a30e5cb3ae351517ceb1db`（私有快照基础）、`c9eb19dfda31bb4d3a432da575c4b56830795510`（双来源恢复尾扫描修复）。来源冻结提交为 `8f36c08`、`5176e502`；全部 crates、Cargo、README、双语日志及产品原则逐文件等同最终来源，主目录 fmt 通过，未混入原未跟踪文件或并行技能计划。
- 双来源选择/冲突、私有配置不相容、缓存失效、三个月完整归档和末次租约回查已有真实回归。真实 rebase（merge/apply）和 bisect 的 detached 暂停主干保护覆盖当前/linked 工作树，merge/cherry-pick 冲突亦保留安全断言。CLI 测试仅对明确的 writer singleflight 忙态有界等待，其余错误仍失败。
- 首轮 SPEC 通过 48 项 Git、21 项 service、12 项 recovery、1 项真实 CLI。质量审查随后以独立真实探针发现私有快照之后的未知 Towermd trailer 漏检，追加最小修复 `5176e502`。selected/unselected 两场景红→绿；各来源使用自身 state 提交为扫描边界，较旧独立快照、等价快照和普通尾不误拒。SPEC delta 与质量 delta 均独立通过 17 项 recovery、48 项 Git、21 项 service，原外部探针的 private/main 路径均拒未知 trailer；最终 Ready to merge: Yes，无 Critical/Important。
- 最终完整默认并发 **401/401、exit 0**，fmt、Clippy 全 targets `-D warnings`、diff 检查通过。首批 12 组红绿及精确文件索引 `/private/tmp/towermd-m0-fix3-B-evidence.json`；质量修复命令/5 条新回归/中间失败索引 `/private/tmp/towermd-m0-fix3-Bq1-evidence.json`，完整日志 `/private/tmp/towermd-m0-fix3-Bq1-workspace.log`。首轮 396 项不是最终数量，中间 higher-fence 诊断顺序失败日志已保留，不以覆盖日志或重跑掩盖。
- 默认沙箱完整测试曾因两个真实 loopback listener 的 EPERM 失败，日志 `/private/tmp/towermd-m0-fix3-B-workspace.log`，不计绿；最终在授权的本机测试环境完整重跑。保留限制：基线同步 authority cache miss 归档读取仍持调用方 session 锁，3.2 C3a 必须迁移锁外准备/短锁安装；同序号不同提交仍严格拒绝，C 才以两份已验证快照的等价证明放行。
- 恢复证明共享契约及四计划同步已提交 `d3ec459`，P8 未签名发布详细计划已提交 `aa57256`，均为独立复审通过的文档，尚未实施。普通 pending 不得证明完成；P2 现有 claim 接线与 P4 出版 schema 共用该契约，不扩展 3.1 源码范围。

- [x] 3.1 A/B：私有快照基础与现存恢复路径（`81266e4`、`c9eb19d`）
- [x] 3.2 C/D/C3a：精确搬送、三次竞争重做、authority 归档锁外准备（`0dc373e`、`9fe7be4`、`b0dbaf8`）
- [x] 3.3 E0：唯一 DTO、持久输入与纯 raw/G5/hash（`c4481c3`、`ae3666b`）
- [x] 3.3 E1a：只读实际输入、私有可信历史、纯 proof 与 canonical builders（`858abd5`、`a8625c9`）
- [ ] 3.3 E1b：业务投影、完整会话/daemon接线与 staging/空目标恢复
- [ ] 3.4 F/G：真实 App 恢复与 Git 先于安装
- [ ] 3.5 H/V：事件/周期调度与整批联合验收

3.2 采用新隔离工作树 `/private/tmp/towermd-m0-remediation-3-2`，基线 `c76c3ca`；独立 target `/private/tmp/towermd-m0-fix3-cd-target`。Root 完整默认并发基线 **401/401、0 跳过、exit 0**，日志 `/private/tmp/towermd-m0-fix3-cd-baseline.log`；新鲜实现者已完成接口映射，按 C1 搬送/新 holder → C3a 锁外读取 → D 有界重做顺序开始 TDD，尚未形成新冻结提交。

3.2 冻结待审：隔离提交 `7b13abd9d3a79c95e5d4c3a669b82ebce6ad3e91`，精确 10 个路径，未合入主分支、未推送。最终完整默认并发 workspace **425/425、0 跳过、exit 0**，fmt、Clippy 全 targets `-D warnings`、diff check 均通过；完整日志 `/private/tmp/towermd-m0-fix3-cd-full-final.log`，逐路径哈希、七组真实红绿及准备失败索引 `/private/tmp/towermd-m0-fix3-cd-manifest.json`。new-holder/D4/真实 CLI 属于相关回归，不补造独立先红历史。C4 已沿实际 state blob 的匹配父定位 producer，覆盖 Promote、普通 merge、合法 second-parent 来源及无 footer 的真实 state 变更拒绝。D5 使用真实 upload-pack 转发结束、push 广告之前的屏障；赢家代码/父提交、两次 fetch、ack/tracking 与随后断网读取续租证据为 `/private/tmp/towermd-m0-fix3-d5-cli.log`。生产分类器仍只重试客户端 non-fast-forward/fetch-first，服务端拒绝不会泛化重试。作者已停止写入，独立 SPEC 审查进行中；须通过后再做质量审查，未勾选本闭环完成。

3.2 独立 SPEC 已 PASS，115 项相关回归通过，Root 核对 manifest 十路径哈希和最终 22 组/425 项测试计数相符。D5 补做事后旧基线对照：`c76c3ca` 导出源码配冻结 CLI 测试可编译、真实命令以 `(non-fast-forward)` 失败，冻结版同测试成功；日志 `/private/tmp/towermd-m0-fix3-cd-spec-d5-baseline-comparison.log`。这是事后行为差异证明，不冒充历史先红。质量审查已启动；首次项目注册的全局 registry 锁覆盖归档读取作为待核并发事项交审，不将未复现风险直接宣称缺陷或已修复。

3.2 质量轮已真实复现上述并发缺口：项目A已注册并运行任务，项目B首次加载在月归档读取屏障暂停，A的status与绑定Run的task show均被全局projects锁阻塞；health仍立即响应，释放B屏障后A两请求才完成。复现脚本 `/private/tmp/towermd-cd-quality-two-project.py`。Root接收为Important，当前候选不得合入；须将冷注册I/O移出全局registry锁，并保留同项目单Session、clone绑定与并发首次注册保护，再独立复审。425项完整绿是修复前候选证据，不能用于尚未实现的修复版本。

3.2 Q1 隔离修复冻结 `d5768e28a41dce47ffb733eed7c3150569c5482c`，最终默认 workspace **427/427、0跳过**，fmt/Clippy通过，Root核24项artifact哈希与22组计数。SPEC delta PASS；原质量独立外部屏障确认B仍暂停时A查询/续租完成，同Run不变，关闭registry Important。独立SPEC并发daemon为39通过/1失败（D5 marker超时，定点复核绿），原质量另用真实在途writer稳定复现CLI提前busy退出而测试盲等marker，日志 `/private/tmp/towermd-cd-q1-quality-d5-busy.log`；不能把定点绿冒全套绿。当前仍Ready No，Q2仅修D5准备/worker诊断并保一竞争命令、真实两fetch及安全断言，源码尚未合入。

### 批次 3.2 回顾（已合入，未推送）

- 主提交 `0dc373e47eec790061a751a5d4967819c4335528`（C/D/C3a）、`9fe7be44ed746c4b0764b2ca1b7616ab18a413bc`（Q1注册锁）、`b0dbaf8ee11690b9d5246779afe46b795d93c591`（Q2测试同步），来源分别为 `7b13abd`、`d5768e2`、`a37a431`。全部 crates/Cargo/README/双语日志/产品原则与最终冻结源逐文件相同；主提交作者和提交者均为Agent，原来源SHA与证据未重写。原未跟踪三文件不变，主目录fmt通过。
- 最终默认workspace **22组、427通过、0失败/忽略**，fmt/Clippy/diffcheck通过；Q2 manifest `/private/tmp/towermd-m0-fix3-cd-q2-manifest.json`，Root核33项artifact哈希。独立SPEC最终daemon **40/40**，质量最终daemon **40/40**并发D5 **4/4**；作者压力 **12/12**。最终质量Ready to merge: Yes，无Critical/Important/Minor。
- Q1真实双项目和临时读取反例红→绿；原外部屏障复验在B仍暂停时A查询/续租完成且Run一致。Q2先完成后台fetch及待归档业务快照/ack，再启真实upload-pack竞争屏障；一次竞争命令、两fetch及原安全/离线断言逐字保留。十处同类准备使用原严格busy helper，未知deferred仍失败，非独立先红如实标相关回归。
- 首轮425全量、Q1独立39/1、Q2首次准备FF错误与daemon39/1等原日志均保留，不冒充最终全量或历史逐条TDD；最终427及双审只覆盖本闭环。E0/E1、F/G、H和整个M0尚未完成，App尚未构建/发布。

3.3 E0 已启动：新工作树 `/private/tmp/towermd-m0-remediation-3-3-e0`，基线 `9e865e0`；Root在独立target `/private/tmp/towermd-m0-fix3-e0-target`完整默认基线 **22组427通过、0失败/忽略、exit0**，日志 `/private/tmp/towermd-m0-fix3-e0-baseline.log`。新鲜实施者已只读映射并接收START，唯一源码writer；范围为共享DTO/摘要、JSONL outbox与私有durable helper、纯raw/G5守卫，不提前实现E1或真实producer。

3.3 E0 冻结待审：隔离提交 `343687963756eeb011c8759383e572e7dd4d0091`，精确10路径，作者/提交者均为Agent，尚未合入或推送。最终默认并发workspace **24组482通过、0失败/忽略、exit0**，fmt、Clippy全targets `-D warnings`、diffcheck通过；Root核manifest十路径源码/提交树及46项artifact哈希。证据 `/private/tmp/towermd-e0-evidence/manifest.json` 与 `index.md`；17号沙箱loopback EPERM失败保留，20号为获授权的完整绿，编译错误和相关回归未冒充独立行为红。raw完整OID补 `--no-abbrev`；真实commit-as普通文件的异常对象类型反例已红→绿。作者STOP，独立SPEC已开始，须通过后才quality；producer、可信历史/业务恢复及App仍未交付。

E1生命周期接线计划增量已独立复审PASS并精确提交 `aa5b70ce7d4edb35eb08f5d0009667ab5299b6d0`（四文档）：覆盖constructor/SnapshotRequired/tick/WS/writer的锁外证明与真实applied水位，保留Q1注册所有权。首轮恢复空target依赖有效Session参数循环的Important已关闭：core私有无业务view的引导支持缺失/空hub及已注册合法空Session，保唯一protocol/owner，Git确定持久且已核安装后才返回有效Session。无剩余Critical/Important/Minor；仅计划，E1源码仍未实现，开工须等E0双审合入。

E0首轮独立SPEC **Not PASS**：15+17+27项定向绿，但发现1个Important——首次创建文件/目录后fsync失败留下路径，原请求重试走已有路径分支，未补目录项耐久却成功调用Hub。独立gitarchive副本三项受控故障探针均可编译后实际红；完整路径首次Err/hub0，重试成功/hub1但创建后父目录sync次数0，日志 `/private/tmp/towermd-e0-spec-fsync-probe.log`。原冻结源码不改；原实现者已接Q1唯一writer修复，必须补创建窗口真实红绿与完整必要验证，再SPECdelta→quality，当前482绿不代表该缺口已关闭。

E0 Q1冻结待复审：`8f87624fa374e9dd408c7dcd92ae7024c5177efc`（直接parent `3436879`），精确仅 `recovery_outbox.rs`。file/dir的新建与已有路径分支汇合，成功前均同步自身及parent；六个创建窗口反例实际红→outbox18项绿，无globalpoison/新API。最终默认workspace **24组488通过、0失败/忽略、exit0**，fmt/Clippy/diff通过；Root核1路径源码/commit哈希、10项Q1artifact以及旧46项证据未变。Q1证据 `/private/tmp/towermd-e0-q1-evidence/manifest.json`，作者STOP；独立SPECdelta已PASS，65项定向及独立归档副本2项故障矩阵绿，探针日志 `/private/tmp/towermd-e0-spec-q1-probe.log`。完整E0 SPEC无剩余Critical/Important/Minor，新鲜quality已启动，尚未合入。

E1实施规模复核后已按安全边界拆为E1a纯reader/proof/builders与E1b全部业务/Session/staging接线，五行文档delta独立PASS并提交 `0f26f67`。新鲜E1实现者已只读映射，确认只读inputs reader/私有来源定位是本阶段必要增补，缺outbox时不能用会创建文件的open冒充只读。未START源码；须E0双审合入后用实际SHA新worktree开工，不将E1a契约夹具或单独证明器称为完整恢复/真人验收。

### 批次 3.3 E0 回顾（已合入，未推送）

- 主提交 `c4481c313abe16fdc224ae935fb3d3fd340e515d`（唯一DTO/摘要、outbox与纯Git守卫）、`ae3666bcca9bd22b156601964219140d2ef10b39`（创建目录项耐久修复），来源分别为 `3436879`、`8f87624`。主目录全部crates/Cargo/README/双语日志逐字节等同最终冻结源，作者和提交者均为Agent；原三未跟踪文件不变，主目录fmt通过。
- 最终完整默认workspace **24组488通过、0失败/忽略、exit0**，fmt/Clippy通过。独立SPEC修复复审65项与2个隔离故障矩阵通过；新鲜quality独立65项通过，Ready to merge: Yes，无Critical/Important/Minor。质量日志 `/private/tmp/towermd-e0-quality-integration.log`、`/private/tmp/towermd-e0-quality-unit.log`。
- 原创建窗口Important真实红→绿；同ID重试/重开必须补自身及parent同步后才能Hub接受。confirmed不落稳则本次请求uncertain，但真实Hubstate仍可见；原已确认请求不向新Hub重发。原失败、46项初版及10项Q1证据均保留，Root逐项核哈希，不以最终488抹去首轮缺口或环境EPERM。
- 本闭环只交共享格式、耐久基础与纯G5；未接真实producer，E1a/E1b、F/G、H及整个M0尚未完成，未构建或发布App。E1下一步按实际合入SHA在新worktree实现，不将本闭环当业务恢复完成。

### 批次 3.3 E1a 执行计划（进行中）

新工作树 `/private/tmp/towermd-m0-remediation-3-3-e1a`，实际基线 `e876f456d49c91cee1ab88ab0d240ee99a15f94b`；独立target `/private/tmp/towermd-m0-fix3-e1a-target`。Root完整默认基线 **24组488通过、0失败/忽略、exit0**，日志 `/private/tmp/towermd-m0-fix3-e1a-baseline.log`；新鲜实现者已复核实际E0接口，按已审E1a边界开始。只写纯consumer及其实际文件/Git契约测试，E1b业务接线仍未开工。

- [x] 只读实际inputs工厂与私有context/来源定位：缺文件零创建，绑定错误和来源变化拒绝
- [x] 实际双源/全归档、confirmed及canonical内嵌原回执汇合；全核后才构造私有历史
- [x] 唯一任务合并证明与builder：完整claim/freeze/pass、G1/晚G4、later Plan、真实Git/tree/epoch及二次恢复上下文
- [x] 冷clone G5树重建、终态Gate Evidence和实际main接入边：多Task提交不猜base，不让旧side tip绕过main代码推进
- [x] 唯一出版证明与builder：实际Decision/manifest/原批准/prepared、十段hash、精确改动集与可达tag链
- [x] 定向/必要全量验证，SPEC→quality双审，精确提交并记录实际SHA；未接业务入口的限制如实说明

执行中先保可编译行为红再绿，历史成功数据标为contract history fixture；不以夹具冒充真实producer/App真人，不接业务apply、Session/daemon/staging或F批准路由。Root维护主目录任务记录；源码实现者可在自己的隔离树记录本地步骤，冻结时仅处理自己的hunk并把完整过程移入独立证据目录，不提交Root任务状态副本。

E1a联调真实复现E0未覆盖的冷clone边界：A仅amend成同tree B并FF到main，file:// --no-local clone缺A，原G5报Infrastructure。独立调查 `/private/tmp/towermd-g5-amended-clone-probe-68emqbjy/investigation.md` 是1项诊断probe通过，不是产品恢复通过。另实测gate后main代码推进并revert再以旧Task B为second-parent合并，B仍可达，不能仅拒merge后又认证B。四文档窄契约增量已独立PASS：唯一G5由实际树重建原evidence tree；trailers前复用既有GateEvidence记录原completed/真实base与HEAD父数组，canonical保原integration_gate并归一实际main接入commit；state-only rebase重trailers而复用G3，不新增type/批准/历史权限。源码正在实现，尚未冻结/提交或双审，不将契约PASS称为producer或App功能。

用户于本轮明确确认首次启用整笔提交作为导入起点，已精确同步规格/P5/发布计划/任务记录并提交 `e6080ff`；与此前推荐一致，不批准B0。该答复已归档，不再询问。

E1a 冻结待审：隔离源码 `42e4af89c11bba2fa33dd08b628eaaa6ac1fcf9e`，精确11路径，尚未合入/推送。最终完整默认workspace **24组519通过、0失败/忽略、exit0**，fmt、Clippy全部targets `-D warnings`与Python独立固定hash向量通过。Root逐项核11路径工作树/实际commit字节与63项artifact哈希，证据 `/private/tmp/towermd-e1a-evidence/manifest.json`、`index.md`；53沙箱loopback失败、54诊断关键字回归及所有中间错误原样保留，56定向和58完整验证关闭回归。终态G5/实际main接入、冷clone和二次仅归档恢复均为纯consumer/contract fixture证据，不称为业务恢复、producer或App完工。writer STOP，新鲜SPEC已START，须PASS再quality。E1b新鲜实现者仅完成实际入口只读映射，未写源码/运行测试，等待本候选双审合入后的实际SHA开工。

E1a只读审查期间顺序使用空闲writer实施已审App基础计划Task1：以 `9b1afa5` 为源码基线，新独立工作树 `/private/tmp/towermd-app-shell-task1`、target `/private/tmp/towermd-app-shell-task1-target`；仅项目pnpm入口/前端构建测试工具、极小Svelte入口、独立Tauri crate和窗口、必要ignore/workspace exclude。沿Note既有依赖版本线，不实现CLI/daemon适配、展示协议或批准。验证启动器/DOM行为、check/build、core全量与desktop check、真实WebView窗口，再精确冻结及SPEC→quality；Task1壳不称可用App或发布。E1a冻结树禁止写入，E1b继续等待START，不并行源码writer。

- [x] App基础Task1：固定项目工具与最小可启动桌面壳（`9e51371`）
- [x] App基础Task1：测试/构建/真实窗口、独立双审、精确提交与证据（下方回顾）

E1a首轮独立SPEC **Not PASS，0 Critical/1 Important/0 Minor**：26项history/proof与20项G5回归绿，但独立冷clone损坏对象探针实际红——原A的loose对象确实存在且Git报告inflate/unpack损坏、退出128，两个consumer边界仍把任意128当missing并认证merge.done。日志 `/private/tmp/towermd-e1a-spec-probes.log`；原freeze未修改，519绿不代表该缺口已关闭。App Task1仅完成项目启动器2项夹具绿，首次pnpm安装在途，已安全STOP保未提交WIP；原E1a writer顺序START Q1唯一对象状态分类修复，不新增producer/Run格式/Plan语义改动，须真实红绿与必要全量后SPECdelta→quality。App与E1b均等待各自START。

E1a Q1冻结待复审：`9e21ff9f0a50914cd586cc9c1e0814bc38157f3c`，直接parent `42e4af8`，精确五路径。唯一core私有Git batch查询只在成功、无stderr且精确OID missing时允许fallback；不按对象库文件路径推测。两损坏对象红→绿，实际缺失coldclone正负回归保持绿；最终默认workspace **24组521通过、0失败/忽略、exit0**，fmt/全部targets Clippy通过。Root核五路径/实际commit、7项Q1artifact、旧63项证据未变，manifest `/private/tmp/towermd-e1a-q1-evidence/manifest.json`。writer STOP，原独立SPECdelta START；未合入，不以521代替复审。App Task1已RESUME，先前pnpm首次安装实际ENOTFOUND失败日志保留，须授权网络重跑；E1b仍待本候选双审实际合入SHA。

E1a Q1独立SPEC delta **PASS，完整E1a剩余C/I/M为0/0/0**。原损坏对象探针用实际Q1 fresh源码副本和独立target复验，`accepted=false`、1项通过；独立core151/151、G5 21/21、Git timeout10/10。timeout首次7/3沙箱loopback失败日志保留，解除该环境限制后完整10绿。日志 `/private/tmp/towermd-e1a-spec-q1-probe.log`、`-q1-lib.log`、`-q1-gates-timeout-loopback.log`。新鲜quality已START，尚未合入。App启动器授权网络重跑实际11.7.0输出通过，前端同Note依赖安装中，尚无真实窗口或App构建。

后续执行调整：E1a双审合入后，E1b核心恢复与App基础Task1可在各自独立worktree/target并行推进。核心writer仅拥有core/daemon恢复路径，桌面writer仅拥有前端/独立Tauri壳和根workspace exclude，各自不读取/依赖对方WIP；同一工作树和同一实现路径仍只有一个writer。共享README/双语日志由Root按已验证提交逐hunk顺序合入，todo仍只由Root维护；冻结、双审与提交证据要求保持。此调整消除下载/构建等待对独立核心实施的串行阻塞，不提前接未冻结的daemon/App协议。

### 批次 3.3 E1a 回顾（已合入，未推送）

- 主提交 `858abd517cf231cb1b29250892ca5a8aaca1dab4`（实际history/纯proof）、`a8625c928db4d80e524812a8c9dc4d2fb22b64d6`（损坏对象拒绝），来源分别为 `42e4af8`、`9e21ff9`。全部crates/Cargo/README/双语日志逐字节等同最终来源，作者/提交者为Agent；主目录fmt通过，原三未跟踪文件不变。
- 最终默认workspace24组521/0/0、fmt/Clippy通过；SPEC修复复审151+21+10项与独立corruption probe1项通过。新鲜quality独立151+21+54+10项通过，Ready to merge: Yes，C/I/M为0/0/0；日志 `/private/tmp/towermd-e1a-quality-core.log`、`-gates-snapshot.log`、`-repo-timeout-loopback.log`。
- 两轮70项artifact、16条源码记录全核hash与实际commit字节；原519、环境EPERM、诊断及corruption红如实保留。仅完成纯consumer，Session/业务投影/实际恢复、producer和App未完成。

### 批次 3.3 E1b 执行计划（进行中）

续行核验：原实现代理已停止，当前无遗留 core 测试进程；新鲜 writer 接管同一工作树的未提交改动与原始日志，继续完整 E1b。`20-exact-old-ack.log` 的精确旧 ack 回归实际 1 项通过；原 `18-recovery-matrix.log` 仍有失败和未终态记录，不称全套通过。较低独立 private/main 的 lease 事件同 seq 冲突已定位为联调边界，修复仅排除双边非业务 lease 的 seq 冲突，所有类型同 eid 正文冲突及业务同 seq 冲突仍须拒绝。

新工作树 `/private/tmp/towermd-m0-remediation-3-3-e1b`，源码基线为实际合入E1a后的当前main；独立target `/private/tmp/towermd-m0-fix3-e1b-target`。基线crates/Cargo已逐字节核等同 `9e21ff9` 的完整521绿，不重复同源码全量；实现前复核实际reader/proof接口，以行为红测推进。按已审E1b/共享3.1节同一闭环，不先开放无proof的有效Session或恢复入口。

- [ ] 私有准备/缓存来源绑定与constructor：actual checkpoint/tail先核、锁外准备/短锁安装、最多三轮与SourceChanged
- [ ] Task/Run/Evidence业务apply与全部Session/daemon/tick/WS/writer接线；真实applied水位、接受mutation只补原结果同步
- [ ] 实际双源可达历史读取精确old ack；保冷注册gate、唯一Hub文件owner与慢归档并发续租
- [ ] 私有verified staging与None/Some空目标恢复：Git确定持久先于target rebuild/ack/install、失败不发布有效view
- [ ] 原E7及共享完整故障矩阵、必要全量/fmt/Clippy、SPEC→quality、精确冻结和实际提交证据

E1b 进行中检查点（未提交源码、未冻结、未审）：证据目录 `/private/tmp/towermd-e1b-evidence` 保留全部真实红/失败/回归。实际constructor拒伪canonical、旧ack精确来源、merged Task/Run/Evidence与依赖G0顺序、七类完整consumer负例、publication原builder及二次仅Git归档、未知footer逐提交诊断、空目标恢复/持久后失败更高请求均已推进；专项日志35完整Session矩阵10项、37负例、41出版、47冷B原2秒与归档读取精确1次、48阻塞push续租/关闭等待通过，尚不能当完整通过。51默认daemon40项为34通过/6失败，保原日志；周期/冷B并行耗时、真实restart ack边界、read-race来源诊断分开定位，不放宽时限或用串行/专项绿冒全量绿。

已按共享§3.1确认Status取舍：旧Catalog/inputs失效以SourceChanged结束本次请求，下一请求可读最后已核view；来源变化即使head等于cursor也明确pending_verification，不主动等待慢proof，不授信未核业务。真实rollback在task show/mutation/writer/WS仍硬拒；旧同次reload测试迁移保实际I/O屏障和文件断言，不新增外层循环。重复I/O仅复用本次已核完整双源历史与精确post-install私有cache；RecoveryContext探针可抽取共用实际authority/config读取的窄私有helper，保root/commonDir/符号引用/普通配置文件/parse/docsDir/local hub与forged配置负例，不伪造空objects Catalog，完整对象/层级校验仍由实际Catalog与proof承担。App另tree无Core WIP依赖。

E1b续行：87默认daemon41/41通过；88完整workspace在snapshot_git两处旧lease callback计数断言失败，原日志保留。89/90仅迁移这两个测试，分别经真实本地已观察变化与仅远端推进验证writer fetch1/2、callback2/3及具体拒绝/refs/index/文件不变；appeared_config另独立断言原init预检额外fetch1。完整snapshot_git54/54通过，生产源码未改。91默认workspace继续通过daemon12、coldCLI10、daemonCLI41、initCLI16、coreunit162及其余集成组，在最后tasks组24/27失败：三个旧fixture仍使用字面revision或尚未提交的main。正在用实际权威提交迁移执行卡动态上下文、外部慢层保护与linked worktree测试，保原全部行为断言；91不是完整绿，E1b仍未冻结/审/合。新鲜SPEC已仅完成权威契约/base准备，须实际freeze后START。F/G只读前置准备完成，明确instance/App身份检查须先于JSON正文提取、请求零续租与唯一恢复owner；实现须等E1b实际双审合入SHA。

92完整tasks27/27通过，三项真实main提交/reload及执行卡历史、慢层文件、linked worktree原保护断言均保留；93 Clippy发现测试初始化的无用赋值，最小移除后94 fmt与workspace全部targets Clippy `-D warnings`通过。95最终静态24路径摘要 `1773e48c73e3859ed2aa60df3f1b774b2718c806629ce3644f507b9f320ee9fb` 已开始默认workspace，不改线程或原时限；须自然终态和实际freeze再审，仍未提交源码。

E1b已实际冻结待审：commit `1fcfb3331901b0dc6cafc9639947f27530acf586`，parent/base `821bdc3e81b37b45095f02117cb668687d0aabbb`，tree `21a83e43e3d2e01a85e5a50c3aefaeef4523413f`；精确24个core/daemon路径，writer STOP，未合入main。95完整默认workspace自然exit0，24组545通过/0失败/0忽略；94fmt/全部targets Clippy及96diffcheck通过。manifest `/private/tmp/towermd-e1b-evidence/manifest.json` SHA256 `5bf031f2fb417a8fbc643d517540cb1e79072b2b0fcd36294e227f353c5b83d6`。Root独立核24gitblob、parent/tree、精确patch及147唯一源码/日志/证据路径hash，记录 `/private/tmp/towermd-e1b-root-evidence/freeze-verification.json`；原失败/trace/九类旧恢复测试映射全部保留。新鲜独立SPEC已START，随后必须fresh quality；所有E1b完成项仍待双审，不将545绿等同于SPEC通过，也不声明真实App/producer已经交付。

#### E1b SPEC修复计划（当前候选不合入）

独立SPEC已报告三个实际路径缺口，正在收口最终结论：canonical合并事实之后重放的无guards旧Task状态可覆盖merged；唯一真实ticker等待慢proof时不能继续自动main续租；部分旧公开执行包装仅fast sync，不能拒绝同head下变化的实际来源。先停止合入/quality，待审查者停止全部cargo和只读审查后，由原writer唯一修复，不让审查和源码写入重叠。

- [ ] 以完整恢复路径复现旧outbox覆盖merged，保持Task/Run/Evidence与归档重放一致；合并终态及后续依赖G0不得被旧事件降级，按E6原顺序修复
- [ ] 以真实daemon ticker及实际慢归档屏障复现自动续租停滞；让合法main维护独立于慢业务proof，保多项目、单owner、关闭drain及原时限，无无界并发或通用协调器
- [ ] 以真实来源回退/输入变化复现旧公开Claim/Show等入口缺少校验；全部有效业务入口收口到同一证明边界，内部fast路径不能对外提供来源校验旁路
- [ ] 以合法两父接入及中间真实Git事实复现canonical实际commit顺序/去重缺口；先按实际canonical commit建事实表，再按提交拓扑正序落事件，保正常/恢复/outbox全字段一致
- [ ] 补齐E4/E5完整恢复组合：snapshot/canvas差异成功、孤立passed/缺freeze或pass/原stale回执/outbox自报merge拒绝，以及真实pending/confirmed重开后恢复、archive去重、丢弃legacy renew、保普通无guards审计；逐项断实际Task/Run/依赖，不以pure proof或reader测试代替
- [ ] 保原冻结manifest/122项artifacts/失败记录；红绿、必要完整默认回归/fmt/Clippy通过后提交精确Q1，独立SPEC复审再fresh quality，不将原545绿继承为修复验收

独立SPEC最终NOT PASS，报告 `/private/tmp/towermd-e1b-spec-final-report.md`：4项实现缺口及上述完整组合证据缺口；三名审查者已结束，无源码修改/在途cargo。独立Session+writer45、recovery23、daemon7项通过；daemon首次loopback EPERM原日志保留，精准升级原命令后通过。测试绿不改变NOT PASS；Q1在原冻结commit上继续，原manifest/日志/源码Gitblob保留，新证据另目录，不进入quality或合入。

Q1 修复进行中（源码未提交、未冻结、未审、未合入）：新证据单列 `/private/tmp/towermd-e1b-q1-evidence`。旧outbox覆盖合并终态、两父接入事实重复/顺序、真实ticker等待慢proof阻断续租及公开入口来源校验均已有可编译行为红和针对性绿；接受Claim后证明失败保留原pending请求也已完成专项红绿，尚须整体验证。私有来源矩阵的旧冻结版独立target对照23为5项真实失败；24是工件/API绑定失败，不能当行为红。定向清理实现者自有target的core工件后，25从当前工作树实际重新编译，私有回退5/5通过；26核Cargo metadata与rustc路径绑定当前工作树，同一静态摘要 `0716363d7cd4f2e9fa6d847dd0b2ddd9bfc29ef1bc1ed986c6320ed2c32b10cd` 上公开入口11项及私有回退5项全部通过、exit0。仍有待清理警告及完整组合/全量/双审，不继承原545绿。独立M草案仅在 `/private/tmp/towermd-e1b-q1-matrix-draft` 交付给唯一writer，未经编译执行，不算测试覆盖或验收。

Q1后续边界核验：36已接受后提前apply的6项真实红、39合法private前进同请求提前apply的真实红、41重开Hub后协调失败丢原eid/run诊断的真实红均保留；42修后11项通过，原pending/result/完整最后已核view保持，下一fresh请求才可核新private。44默认core180/47daemon8通过；49测试lint失败修后50全workspace/alltargets Clippy `-D warnings`及51fmt通过。但52静态 `30cbe933…` 的完整workspace尝试在daemonCLI为37/41失败，不能算全量绿：三处snapshot/旧rebuild_request未接独立合法Run续租，一处coldB实际归档屏障下A taskshow超原2秒。53补回既有system续租后hub_rebuild五项绿；snapshot及完整回归仍待。54/55只是过滤性能基线（Show约1.2秒、原2秒内通过），不是行为红；55精确A请求窗口275次Git/39blob/67ref，56第一次续点合并同窗口287/40/70且Show约1.265秒，未证改善。继续按实际阶段定位，不资源归因、不放宽时限/线程，不新增公开信任bool、跨请求缓存或同次来源重载；最终须重新静态完整workspace自然绿后才冻结双审。独立Q1 SPEC仅权威/原冻结预备READY STOP，未读WIP/运行测试或给当前候选结论。

57无trace额外I/O的默认daemonCLI为40/41，三处续租全绿，coldB下A Show仍3.207秒超原2秒，原日志保留。58私有分支回归证明无过期tail时coalesce、真实release tail时先proof再Show，原release/claim各一次；尚非整体性能完成。只读设计包 `/private/tmp/towermd-e1b-q1-perf-design` 的8源码hash已核，focused helper仅分析冻结包，不审WIP/写源码/跑cargo。当前实施边界：真正只读Show先完整sharedproof；若动作后capture只有原已核prefix加本请求Hub实际lease_renew完整回执，FullSystemState实际replay与head全等，ack/floor/epoch/previous_head及context等完整坐标无其它变化，则最终actual main/private/inputs/context复核与短锁capture CAS后可只更新system head、交付原已核Show结果，不apply tail或推进业务cursor。它是本请求私有续点，不新增public信任口/缓存；过期release、其它业务/未知tail、pending或坐标变化走原完整proof，来源变化拒交付，错误保原identity。该路径尚未验证，须补实际receipt/并发来源变化/其它tail回归及重新默认41和静态完整workspace，不能凭设计裁定称完成。

69完整默认workspace随后仍FAIL：静态源码 `747824661505527b77f1719dad4f3a6d32a44aa650a185134b6ba60544446556`，daemon41/41（原2秒场景Show1.598秒）、core184/184等已绿，但snapshot_service为33/34，最终闲置测试计数2而非1。71阶段实证显示Add前reads1/applied0、Add后reads2/applied1、后100idle仍reads2/applied1，ack均None；无Show，问题为普通业务新tail触发完整准备，不能误归因Show。只读权威核对 `/private/tmp/towermd-e1b-q1-spec-evidence/r5-authority-semantics.md` 明确R5/批2要求普通任务也不全读归档或全重放；现有authority_reads只是安装次数代理，不能当真实I/O。保持原失败/预期，新增真实archive读取与事件prepare/apply/replay计量；沿已有私有完整prefix key派生严格普通Task（不许merged）/合法system新delta，锁外stage，最终actualsource/inputs/context、原request privatepin及完整capture/业务projection/pending CAS通过后安装并更新完整key。canonical/opaque/ack/floor/Plan/unknown等仍完整proof或来源失效，不新增公开信任口/跨请求history缓存，不重复业务动作。尚未实现/冻结/审；必须保长tail连续Add只处理新delta、idle零增和既有安全矩阵，再完整静态回归。

E1b Q1现已冻结待独立复审：`1b05337fcca37b3267e84ac74ae7aa8bdf471c13`，直接parent `1fcfb3331901b0dc6cafc9639947f27530acf586`，tree `23ea54fa076010f6291a0ceefd3fdd3dc31beff1`。81完整默认workspace自然exit0：25组587通过，0失败/忽略/过滤；source `dac6f310528631bf8e92daffe6aa64d46754e4b110bc2a74759c3e57f78a2e5c` 前后静态，同源79全部targets Clippy `-D warnings`与80fmt通过。原69等失败保留；73普通追加与77混合systemhead领先cursor的真实RED修后，每次只prepare/apply实际新delta、不读旧archive，原Add前后及100+100idle的reads1断言保持。75旧receipt不匹配用例迁为核实际全Event/FullSystemState、无forged_return及实际cursor，合法shared回退不强制重读全部归档。manifest `/private/tmp/towermd-e1b-q1-evidence/manifest.json` SHA256 `1b7c377bd0b50481e512c6b1a0d3ef77ad1ae8e38d3dd024d32f8a9680b16cec`；Root独立核15delta/25union/58完整workspace输入、352项artifact及精确patch，原124证据文件和24Git blob未变，receipt `/private/tmp/towermd-e1b-q1-root-evidence/freeze-verification.json`。writer STOP，独立SPEC正式START；仍未合入，须PASS后fresh quality，不把587绿当规格通过或App完成。

E1b Q1 独立复审最终NOT PASS：原I1–I4已关闭，独立既有测试255项通过，但实际外部Hub包装器证明同次公开Add在业务FnOnce之前执行5轮完整准备（2轮丢弃、3轮安装），外层continuation重新发预算违反共享§3.1；不扩大为已证明source reload、业务重跑或未核泄漏。另外旧七类产品恢复负例未包含D→T，缺规格逐类要求的依赖投影与真实G0拒领断言；新六类组合不能替代旧七类。报告 `/private/tmp/towermd-e1b-q1-spec-evidence/final-spec-report.md` SHA256 `e41ca1920ef272eff51c317b97e966f15ec24b380c4508ac4d4d2e4efb4b3dce`；最终真实预算红为 `13-final-continuation-budget-red.log`，源码 `12-final-continuation-rounds-source.rs`。审查者/验证已全部STOP，不进入quality或合入。

Q2仅顺序修上述两项，原唯一core writer在1b05337上继续，Q1源码/manifest/352证据保持不动，新证据另目录。最小设计为只调用一次内部至多三轮的shared准备，短锁继续点再次遇新tail即SourceChanged；保动作后合法新tail同步、private pin与接受后原pending/eid/Run/result/guards/零apply。旧七类夹具补真实D→T与恢复后proposed/G0拒领，不改生产恢复语义。先复现预算行为红→绿，再必要回归及静态完整默认workspace/fmt/Clippy，精确冻结后SPECdelta→fresh quality。

- [x] E1b Q2：消除continuation外层重置预算，保原三轮与单次业务/接受后协调
- [x] E1b Q2：七类恢复负例逐条补实际D投影与真实G0拒领
- [ ] E1b Q2：静态完整验证、保全旧证据、冻结并独立SPEC→fresh quality；未合入前不标E1b完成

Q2已冻结 `aafa370c8497392c71489bfc262e79d3c03f89df`（直接parent1b05337，tree85332cea）：3delta/25union/58输入，09最终默认完整workspace25组588/0/0/0自然exit0、同源fmt/Clippy通过；source `d62222ea8e07bb5e5c1af9135ead62d9de7754b48b410c49a9476f8baebb71f4`。manifest `/private/tmp/towermd-e1b-q2-evidence/manifest.json` SHA `7452f8637011861214c2b5f8656256d0f44d8d78db2e079756bada0b0be9f1e5`，Root独核54新artifact、原124+Q1354及Q1原58Git输入不变。独立SPECdelta整体PASS，47/47（含七变体的一项M1及实际budget外部探针），报告 `/private/tmp/towermd-e1b-q2-spec-evidence/final-spec-report.md` SHA `8ceb01235ee08cc23e78530399c2d9ef028cff16cc0432c5bc676f352a58ae99`。writer/SPEC STOP，fresh quality已START；未合入，不以588冒主目录/真实App验收。

Q2 fresh quality实际复现新的安装窗口：`recovery.rs`两条目标路径先 `rebuild` 持久非空Hub，再独立写exact ack；第二次journal replacement遇EISDIR或两写之间崩溃后，实际drop/reopen见非空floor而ack=None，Session拒绝exact ack，更高新请求也因非空目标拒绝。独立诊断探针1绿仅断言坏行为；另有真正产品契约红 `10-install-contract-red.log`（可执行测试1 failed、自然exit101），要求新授权更高请求可完成，实际SnapshotRequired。证据 `/private/tmp/towermd-e1b-q2-quality-evidence`；不把夹具恢复第一步journal字节当产品补救。当前不合入，等质量有限回归/正式报告STOP后交原唯一writer。

Q3最小整改计划：使最终目标checkpoint、exact ack与tail的安装具备一次持久边界，或提供只针对已核部分安装的严格续接；先核实际Hub/七操作协议，再选择最小实现。普通非空Hub仍拒覆盖，不能新增公开trusted开关或跳门禁；Git确定持久、staging证明、目标安装、有效view的顺序保持。原Q2及所有旧证据保留，新证据单独 `/private/tmp/towermd-e1b-q3-evidence`。

质量正式STOP报告为 `/private/tmp/towermd-e1b-q2-quality-evidence/final-quality-report.md` SHA `2fc561eaa02e22cc68438a11f70cb054068fd3c6ae7e83d6b82dc518480bdc6c`，C0I1M0/ReadyNo；独立127仓库回归绿及1契约真红分别保留，Root核35新证据hash。原writer已接Q3唯一写入：扩展既有RebuildSeed可省略的最终确认，Hub在内存校验/压缩后一次journal replace安装，不加第8操作、不解业务。源码/旧证据保持Q2冻结直至正式START；后续无论写失败或重开均须证明没有无exact ack的不可恢复中间态。

- [ ] E1b Q3：主/私有快照与实际drop/reopen安装失败的契约行为红→绿，保普通非空目标拒绝及原owner
- [ ] E1b Q3：必要定向及静态默认完整workspace/fmt/Clippy，精确冻结，再SPECdelta→qualitydelta
- [ ] E1b最终双审后精确合入，并与main已有Canvas/接口/Ensure真实联合验证；不提前标完成或发布

#### 独立推进 R12 画布数值整改

按批5计划“R10–R12与R13接口诊断可独立实施”，仅提前执行Task3/R12，不宣称批5完成。另建隔离worktree及target，唯一writer只修改 `crates/towermd-core/src/canvas.rs` 和 `crates/towermd-core/tests/canvas.rs`；与E1b的24路径不重叠，不读其WIP，Rust验证窗口由实现者直接协调。

- [x] 验证未知扩展小数会使现有整份投影失效的可编译行为红
- [x] 按现有计划最小修复：只校验几何整数，非法几何仅对应节点opaque，未知数字与原文字节保留；不新增依赖
- [x] 验证canvas/canvas_view/coldstart相关消费者、fmt/Clippy，精确冻结两路径及原始红绿证据
- [x] 独立SPEC通过后fresh quality；双审后精确合入并更新双语未发布日志

R12 已冻结待审：`9fa8908f7df2e3a954767259ddd98ec7e7d15c16`，实际base `f10808ce5038a5c9210db112d2359cd50edc3f17`、tree `80a15e82955c371b99a434106cc1895930d9b61c`，仅上述两路径且工作树干净。原18项基线通过，3项可编译行为红分别实际执行1项失败；修后canvas20/canvas_view5/coldstart10共35/35，fmt与core全部targets Clippy `-D warnings`通过。01 PATH找不到cargo是环境失败，未启动测试，原日志保留；Task3约定的`1e999`原文及不透明诊断例外保留。manifest `/private/tmp/towermd-canvas-r12-evidence/manifest.json` SHA256 `df74738675058548d6b298b0d54eebd4ce4085dbdeaefb86770f1dfe21dde4c2`；Root核实际parent/tree/两gitblob及26项证据hash和9组命令日志计数，记录 `/private/tmp/towermd-canvas-r12-root-evidence/freeze-verification.json`。作者STOP，独立SPEC已开始；尚未合入/全workspace/批5验收，当前E1b全量也不包含R12。

R12 完成回顾（已合入、未发布）：主提交 `eb0b877`，来源为上述冻结 `9fa8908`，Root核最终两源码逐字节相等，未混入原三未跟踪文件。独立SPEC PASS、fresh quality Ready Yes，C/I/M均0/0/0；分别用独立target实际执行canvas20/view5共25项与canvas20/view5/coldstart10共35项，均0失败/忽略。报告 `/private/tmp/towermd-canvas-r12-{spec,quality}-evidence/review.md`；原manifest/日志未改。README与双语未发布日志同步，仅声明已验证R12；未运行完整workspace或App，不代表批5/M0/发布完成。

E1b Q1新增组合27已由唯一writer审用草案后实际编译执行，4/4通过：六类缺失/原stale事实拒绝与依赖G0拒绝、snapshot/canvas-only合法恢复、pending/confirmed两次重开后的归档去重/legacy renew过滤/普通无guards审计，以及raw merge.done两态拒绝/空target。Root核日志与命令静态摘要 `2a5869aca722b6a4145415239ddb57b4c4019490c0ea334c6acde4bb383062cd`、exit0；这些是补充consumer回归，不冒先红或真实producer/App人工验收。29共享入口不确定结果5/5通过；剩余来源变化/真实ticker关闭期间续租和必要完整回归仍在进行，Q1源码仍未提交。

#### 独立推进 R13.3 接口未解析诊断

按批5计划明确可独立实施的接口诊断执行Task6，另建worktree/target；唯一writer仅改 `crates/towermd-core/src/canvas_view.rs` 和 `crates/towermd-core/tests/canvas_view.rs`，不与E1b或R12源码重叠，不实现接口YAML加载器。

- [x] 真实Catalog/CanvasView路径复现未加载接口清单却被误判dangling的行为红
- [ ] 最小区分unresolved及interface_unresolved诊断，保bound/mismatch/annotation/opaque；Canvas和Catalog诊断汇合，错误分支不覆盖既有诊断
- [x] canvas_view/catalog相关验证、fmt/Clippy，精确冻结两路径与红绿证据
- [x] SPEC→fresh quality双审后精确合入并更新双语未发布日志；不以局部任务完成代表批5完成

实际基线映射发现Catalog尚无diagnostics字段，R11未实施；不能为了Task6造Catalog API或读E1b在途源码。当前切片先实现unresolved及Canvas诊断保留，Catalog诊断合流明确依赖后续R11真实接口，保持Task6总体未完成；独立切片有真实验证/提交/双审后可分别合入。

R13.3接口切片回顾（已合入、未发布）：主提交 `edfe84b9722d3d2441c7378fec05ac02dddec620`，来源冻结 `df5d0669325fb0a67b57beee732dbfc0f1793318`，base `d72f9e3`、tree `e3db47ad63b7da1049a8217cd4e76d7c8d0cf4cd`；精确仅canvas_view两路径，Root核最终字节等同冻结、8项命令日志SHA/计数及原行为红。原GREEN view7/catalog28/canvas18共53项，fmt/core全部targets Clippy通过；原canvas18不冒称已含R12的20项。manifest `/private/tmp/towermd-interface-r13-evidence/manifest.json` SHA256 `6e241175ba44d3f78555adb75069b3f13dd97bf7d71fdf5977fc08dd36910939`。独立SPEC PASS及fresh quality Ready Yes，C/I/M均0/0/0，分别独立target实跑view7/7；报告 `/private/tmp/towermd-interface-r13-{spec,quality}-evidence/review.md`。README和双语未发布日志已同步，原证据保留；R11/Catalog合流及真实main上R12+R13组合验证仍待，不将此切片勾为Task6整体完成。

真实main组合验收随后完成：源码 `01ca969fb548ffc2a7dcee0db8242fb5466104c3` 在独立target上实际运行canvas20/view7/catalog28/coldstart10，共65/65、0失败/忽略/过滤、exit0；tracked crates/Cargo源码前后hash相等。证据 `/private/tmp/towermd-canvas-interface-integration-evidence/01-consumers.json`，日志SHA256 `640f2d2a14cdd6407bcf1f2d2ecb23416476508780bfb2e3eb0dca96c7aef428`。仅验证已合R12与R13接口切片，不含未合入E1b或待实现R11，不冒称完整workspace/桌面/发布验收。

R11只读预备已完成 `/private/tmp/towermd-layer-r11-preflight/notes.md`，仅冻结main `2c5897d` Gitblob映射、未实施/未测试。实际旧计划遗漏recovery_git两处发布资格判断与两个Catalog完整字面量；三态的“非法按慢层保护”不能变成“非法可合法出版/删除”。必须等E1b双审实际合入SHA后重核接口，再开启R11唯一writer；不将READY当SPEC PASS。

### App 基础 Task1 冻结待审（已提交，未合入、未发布）

- 冻结候选 `2a5e0aec30daf4992547ae654d6bafc652e9d293`，实际基线 `9b1afa562f4e4179d7592d664d5cd0d029d6b192`；独立工作树 `/private/tmp/towermd-app-shell-task1`。29 个精确路径已提交，Root 核全部提交字节及79项源码/证据/开发bundle的哈希与大小，core源码未改；原 AGENTS.md 未跟踪且未纳入。
- 启动器真实行为红→3项绿、组件行为红→1项绿；Svelte check 0错误/0警告、frontend build、desktop check/fmt与diff通过。该实际基线默认核心完整24组488项通过，不将其冒称后来的521项。真实 `tauri dev` 和显式 `--no-sign` 的debug `.app` 构建成功；仅 linker/ad-hoc，无发行者身份或公证。
- 用户在本会话明确确认开发窗口“正常显示按钮和说明”，记录 `/private/tmp/towermd-app-shell-task1-evidence/human-visual-observation.json`。单窗截图失败日志原样保留，没有截图；此次观察仅验收壳的显示，不是B0放行或产品闭环验收。中断后系统核实该开发App自己的PID7853仍运行，未重复启动。
- 冻结证据 `/private/tmp/towermd-app-shell-task1-evidence/freeze-manifest.json`，SHA256 `e7067d50ebf5e2b6e252f4872b2dc4219dd65549ee4a3ffd5e2b9d68f4464c04`，保留旧manifest。新鲜独立SPEC已START，须PASS后新鲜quality，再精确合入。唯一“打开项目”入口仍禁用并明示尚未接入；Task2/daemon/批准均未实现，不能称首个可用App已完成。

### App 基础 Task2 执行计划（已合入）

Task1 完成回顾：实际主提交 `9e5137120593424ee4f3dc07a12846a22f1125d8`，来源 `2a5e0ae`。新鲜SPEC PASS、fresh quality Ready to merge: Yes，C/I/M均0/0/0；SPEC独立完整默认24组488项通过，quality独立启动器3项/组件1项/check0错误0警告/build通过。日志分别 `/private/tmp/towermd-app-task1-spec-fresh-*.log`、`/private/tmp/towermd-app-task1-quality-fresh-*.log`，首次SPEC loopback EPERM原样保留，授权完整重跑exit0。合入时双语日志的真实冲突已逐条保留E1a原段及Task1新增段；Root核26个非共享文档路径逐字节等同冻结，core源码/Cargo.lock未改，主目录fmt、diffcheck与两成员workspace metadata通过，受保护三文件不变。仅开发壳完成；不称首个可用App或发布完成。

只读核对 actual main `ef4da36` 与 Task1 候选完成，未实现 Task2。当前没有 `DaemonArgs::Ensure`；现有唯一启动路径 `daemon::connect(true)` 仍私有，不以 `status` 代替。项目 token/原生 app-data 调用端也待后续接入。本任务先验证固定原生 CLI 适配边界，真实 ensure 启动/复用/零续租须接收 workflows Task2 的实际 producer 后联合验证。不得开放任意 cwd/argv invoke。

Task1 双审及实际合入后，已创建新 detached 工作树 `/private/tmp/towermd-app-cli-task2`，实际基线 `fae0787b8fcb8c846f256200e3f31606c0f59797`；独立 target `/private/tmp/towermd-app-cli-task2-target`、证据目录 `/private/tmp/towermd-app-cli-task2-evidence`。新鲜 writer 已 START，仅拥有桌面 CLI 适配、stage 脚本及对应配置/测试，不改 core/daemon，不接未冻结协议；Root 继续独占共享任务记录和文档合入。

- [x] 在 Task1 实际合入 SHA 的新 worktree/target 开工；生产按 current_exe 绝对定位随包 CLI，开发固定 manifest/host triple，缺失不回退 PATH
- [x] 固定 Ensure/Init argv、stdin EOF、PATH 继承及 stdout/stderr/退出码，中文/空格/引号/`$()` 按单个参数；原生阻塞工作不阻塞 UI
- [x] stage-cli 显式双架构 target、对应实际 Cargo 输出与可执行权限；构建失败/缺目标产物禁止取 host 回退
- [x] 精确将脚本测试加入现有仅 `src/**/*.test.ts` 的匹配范围；先 stage 实际 host CLI 再接 externalBin，编译缺 sidecar 不算行为红
- [x] 必要红绿/desktop 与脚本检查、冻结证据、SPEC→quality、精确提交合入；不提前宣称 App 项目/daemon 已可用

Task2 冻结待审：候选 `889ef1131f4b3fabaa148ec89faad6c9c8961f95`，直接 parent 为上述 `fae0787`，精确11个路径；writer已STOP，未合入/发布。Root核35条源码/实际gitblob/产物/日志hash及父提交，core、根Cargo、共享文档/todo未改；证据 `/private/tmp/towermd-app-cli-task2-evidence/source-manifest.json`、`artifact-manifest.json`、`verification-manifest.json`。冻结版native7/7、stage9/9及原App1/1、desktopcheck、Sveltecheck0/0、fmt/diff均通过，实际aarch64CLI已构建stage，尚未打包运行App。解析2项/执行5项/stage6项真实行为红保留；DNS/安装中断非行为红，macOS canonical cwd首轮4/5失败及修正回归保留。新鲜独立SPEC已START，PASS后才quality。真实Ensure、Intel构建运行、ProjectToken/app-data和发布仍未验，不用fixture代替。

Task2 原SPEC与fresh quality均PASS 0/0/0，但Root合入前补查发现 **Important：stage错误取旧CLI**，当前不能合入。真实tiny Cargo项目在 `.cargo/config.toml` 配置 `build.target-dir="configured-target"` 且未设置CARGO_TARGET_DIR；实际build成功产出配置目录，默认target残留旧CLI时，冻结脚本却成功复制旧文件。独立真实探针 `/private/tmp/towermd-app-cli-task2-root-q1-evidence/probe.mjs`、`red.log` 明确staged SHA与本次构建SHA不符，exit1；不是环境/编译失败。原writer已START唯一Q1（stage脚本/测试），以Cargo实际报告的输出目录替代路径猜测，保原freeze与35条证据，真实红绿后冻结增量再SPECdelta→fresh quality。不得删残留文件掩盖选错产物，不能用原绿代替修复验收。

Q1 修复冻结：`9bdc27dc02f07b17cbe25c921f2178779f2ad08c`，直接 parent `889ef113`，仅stage脚本/测试两路径；writer已STOP，尚未合入。使用同Cargo/cwd/env的 `metadata.target_directory` 定位产物，原9项加真实配置目录/旧默认产物回归共10项、App1项全绿，真实host stage、desktopcheck、frontend0/0、fmt/diff通过；native未改，明确继承原7/7。Root核实际提交/父/两gitblob、16条源码日志探针hash、原source manifest与真实Tower CLI hash；增量证据 `/private/tmp/towermd-app-cli-task2-q1-evidence/manifest-delta.json` SHA256 `8cf8a810ef4f43a872fc28f28be7af3a9f41aefff08cbb9208ec7c1b01dcffcf`。原red/probe保持；新冻结探针绑定actual commit及stage脚本SHA，staged/actual SHA相等，原hardcoded commit只标原red基线。新鲜独立SPEC增量已START，PASS后fresh quality，不沿用原双审结论。

Task2 完成回顾：原适配实际合入 `1154e3b`，Q1 实际合入 `9b17457`；来源分别 `889ef113`、`9bdc27d`。新鲜SPEC与其后fresh quality均PASS/Ready Yes，C/I/M均0/0/0；两审分别独立stage10+App1、Svelte0/0、fmt/diff、实际冻结探针，并核原始证据不变。新日志 `/private/tmp/towermd-app-cli-task2-q1-spec-*.log`、`/private/tmp/towermd-app-cli-task2-q1-quality-*.log`；native未改，继承原7/7，不称重跑。无合入冲突，Root已核最终11条源码逐字节等同冻结，core/根Cargo未变；同步README先stage/同target/实际Cargo输出目录及开发工具边界、双语未发布日志，主目录fmt/diff通过。真实Ensure、ProjectToken/app-data、Intel、随包App和发布仍待后续，按钮仍disabled。

### App 基础 Task3 执行计划（进行中）

只读前置已核当前Task2实际接口和FR-98：扩展已有commands.rs，不覆盖AppError；复用固定Init适配，不调用未实现Ensure/status或假ready画布。前端官方 `@tauri-apps/api` 沿Note的Tauri2类别与锁版本2.11.0，不开启全局/内部桥，无新Rust库。新独立worktree/target由Root在Task2文档及字节核对提交后创建，唯一writer只改项目选择/初始化边界，Root继续独占共享文档/todo。

已创建独立detached工作树 `/private/tmp/towermd-app-project-task3`，实际base `ef84f90a78fdc8718b867e7c35bd41637a41bead`；独立target `/private/tmp/towermd-app-project-task3-target`、证据目录 `/private/tmp/towermd-app-project-task3-evidence`。仅复制未跟踪AGENTS以适用仓库规则，原保护文件不提交；Task2最终11路径已核逐字节等同冻结9bdc27d，core/根Cargo未变。即将新鲜唯一writer START，不复用Task2 writer上下文。

Task3 进行中实际验证：registry/组件/调用桥已观察可编译行为红再绿，native最终21项（unit1/既有CLI7/project13）与旧init_cli16项通过，前端精确8项、check0/0/build、fmt/diff通过，尚未冻结/独立审。真实Init比较HEAD、index原始字节及暂存条目、Markdown和重复config/项目ID，另有空仓库及marker损坏/符号链接拒绝；不把stub编译/网络失败当红。依赖大包下载失败日志保留，复制已验证Task2缓存到Task3独立store后由pnpm锁与策略复核，不修改源cache或全局配置。

真实桌面构建 `/private/tmp/towermd-app-project-task3-evidence/27-desktop-debug-build.log` 拒绝既有Rust `tauri 2.12.4` 与最初沿Note锁定的JS API `2.11.0` minor不兼容。Root已裁定仅将官方 `@tauri-apps/api` 实际锁对齐兼容2.12 minor，package仍`^2`，沿既有Tauri2类别/版本线，不新增库类别、不改Cargo；保原拒绝日志，重跑受影响前端/锁/桌面构建并以最终实际版本和产物冻结。此次构建失败非业务行为红，先前壳观察不能代替新UI验收。

Task3 已冻结待合入：实际候选 `90c248e2ce3c4e5c79ce7304c5aaee3f8d2e792f`，直接parent为上述ef84f90；13路径精确提交，仅保预存??AGENTS，writer已STOP。最终JS API实际2.12.4，真实debug App构建成功；完整manifest `/private/tmp/towermd-app-project-task3-evidence/manifest.json` SHA256 `ac4a3acbb37e5ea03b6ad2f39acc2704e08e4a8b65376a3d1630b520ac2bd065`。Root独立核全部59条源码/日志/产物与实际13gitblob/parent/scope；fresh SPEC PASS 0/0/0，独立target实际native21/21、精确front8/8、check0/0、临时outDir build/fmt/diff通过，init_cli16仅核原证据未重跑；原freeze/dist保持。日志 `/private/tmp/towermd-app-project-task3-spec-*.log`。fresh quality已START，不冒已合入。

常规新UI自测仍未完成：Root精确关闭旧壳PID7853并打开冻结候选PID73798，window6694标题Tower.md、960×700 onscreen；仅存在元数据，不等于内容/点击通过。用户纠正“你能不能自己测试”，已撤回按钮/dialog/cancel观察请求，改由独立Agent完成；真正B0/G1/G4等人工闸门仍交人。CUA文档读取成功，但getApp绝对候选路径请求10秒实际阻塞1031.7309秒后返回`-10005 cgWindowNotFound`；没有截图/成功点击证据，也不是自动审批拒绝。Root记录 `/private/tmp/towermd-app-project-task3-root-evidence/ui-automation-failure-and-user-correction.json`，不覆盖原manifest；UI工具由独立Agent使用，其他任务无需等待Root逐次资源START，确保工具故障不再阻断开发。debug App仍用manifest sidecar，不冒release随包定位、Intel或业务闭环验收。

Task3 Q1及代码合入回顾：原fresh quality发现Important跨层竞态（native已切B并撤销A，响应迟到时UI仍允许再选取消/Init提交A），原真实组件/模拟IPC探针两行为红保留。唯一writer仅修两个ProjectPicker文件：choosing状态、按钮禁用与两个handler guard，保generation；新候选 `51d7a6302fece52756be5bb486aadc0e0e9af8b3` 直接parent90c248e，front9/9和原字节probe2/2绿。Q1 manifest `/private/tmp/towermd-app-project-task3-q1-evidence/manifest.json` SHA256 `bef32cc5ed3845354ca03669318e46920814ea62dfadc9fba1260af295499bb7`；Root核新/保留共73记录，原13gitblob/38logs/8artifacts不变，原dist/App未覆盖。新鲜SPEC增量与其后fresh quality均PASS/Ready Yes，C/I/M均0/0/0，各自独立front9/probe2/check0/0/diff，SPEC临时outDir build通过；native21/init16未改准确继承。日志 `/private/tmp/towermd-app-project-task3-q1-{spec,quality}-*.log`。package实际`^2.12.4`、锁2.12.4沿既有Tauri2线已接受，不为旧文案`^2`再改依赖。

已精确合入Task3原实现 `d8efcd8` 与Q1 `c12b272`，来源90c248e/51d7a63；尚未构建Q1新App、常规实机自动化仍未通过，不能勾选完整GUI验收。Root已核最终13源码逐字节等同冻结，core/根Cargo未变，主目录fmt/diff通过；同步README/双语未发布日志，并去掉旧未发布条目中已过时的禁用/待项目选择表述。后续新App须独立q1-target/新frontend build目录，保全部原manifest路径。此时只实现项目选择/FR-98初始化，没有接Ensure、daemon只读投影、批准、Intel或发布。

Q1 新debug App随后已实际构建：source51d7a63/tree a92e1cc，独立q1-desktop-target和q1-build-dist，Tauri override仅两个build键，exit0；证据 `/private/tmp/towermd-app-project-task3-q1-build-evidence/manifest.json` SHA256 `5add145e08e5d552ba1c708ebc3ebf8a475b107b7f361059993baeb6e629712b`。Root核27git输入及46构建记录、App精确4文件、内置CLI等同冻结stagedCLI。实际arm64、linker ad-hoc/无Team与发行者/未公证；未launch，不冒release定位或GUI。后续Q2使其不再是最新源码产物，原artifact仍保。

Task3 Q2已合入：Root实际Git探针在两个临时仓库中证明继承`GIT_WORK_TREE`把native探测引向未选仓库；原rawGit记录 `/private/tmp/towermd-app-project-env-root-probe-evidence/probe.json` 只是旧命令证据。writer真实Registry隔离子进程先红再绿，选择/resolve/marker及Init前重核→真CLI重复初始化保原config、不写未选仓库；仅Git子进程剔除既有17项重定向变量及动态CONFIG_KEY/VALUE，正常父环境/PATH/global/system配置保。候选 `ae27059c9eb5928672402169c8d1a23cb8824a3a` 直接parent51d7a63，精确两native文件，manifest `/private/tmp/towermd-app-project-task3-q2-evidence/manifest.json` SHA256 `abd4b944ff890ac5c1feaaaaa696f5ff03ebdb12cf7ce81a83712dd6e908acdf`。fresh SPEC独立native23/23、fmt/diff与freeze核验通过，其后newfresh quality ReadyYes/C0I0M0，未重复Rust；Q1前端9/init16/stage10源码未改准确继承。Root独立核parent/tree/两blob与126新/保留记录，实际合入 `3317a3c`；日志 `/private/tmp/towermd-app-project-task3-q2-{spec,quality}-*.log`。Q2新App尚未构建，常规GUI仍未验，不以Q1App替代。

独立GUI自测记录 `/private/tmp/towermd-app-project-task3-ui-auto-evidence/receipt.json`：清单可见Tower.md运行，bundle绑定实际等待1509.4秒后中断，显示名/进程名均Invalid app，未得到AX/截图、零输入动作。随后仅有界复查清单与绑定，不向用户转交常规测试，不改系统权限。普通GUI未通过不是自动审批拒绝，也不影响继续整改/已验证代码合入。

GUI后续诊断已停止重复绑定：独立Agent在刷新清单后给bundle绑定增加客户端15秒`Promise.race`和工具20秒超时，仍实际阻塞998.1秒，Root中断后返回；没有AX、截图、点击或计时器回包，不能将该超时机制当作有效保护。独立增量 `/private/tmp/towermd-app-project-task3-ui-auto-evidence/receipt-delta.json` 保留原receipt SHA256 `c5ccf01045937d1b90234a461d8d7837da0a04a995ca9f31c0c83ad1b3df4b97`；未再执行绝对路径重试、不改系统权限、不让用户代测。新Q2候选由独立build-only Agent使用新target/dist构建，Rust构建与核心默认回归直接协调，不覆盖旧产物；构建及常规GUI尚待实际结果。

Q2新App实际构建完成：source `ae27059`，新q2-desktop-target/q2-build-dist，frontend及Tauri均exit0，dev编译32.78秒。build manifest `/private/tmp/towermd-app-project-task3-q2-build-evidence/manifest.json` SHA256 `3b94644cb05fdf567922955edf352210d132bfe111d3ca5ddea72e086eaeb671`；Root独立核32项实际git输入、172项新/保留文件记录及App精确4文件集合，内置CLI等同原已冻stagedCLI。App/CLI实际arm64，仅linker adhoc，无发行者签名或公证；debug仍定位编译时manifest CLI，不冒release relocation或Intel。Root精确关闭原90c候选PID73798、启动Q2 PID7132，window6697标题Tower.md/onScreen/962×700；记录 `/private/tmp/towermd-app-project-task3-q2-root-evidence/{freeze-verification,launch-and-window}.json`。这是构建/进程/窗口元数据证据，尚非GUI内容和交互验收；新进程仅做一次绝对路径绑定诊断，由Root独立控制中断，不再依赖失效超时。

Q2唯一新路径绑定仍无AX/截图/窗口内容，Root独立截止中断后实际56.8秒返回；零UI动作，按钮、原生选择框及取消返回三项均未验证。独立记录 `/private/tmp/towermd-app-project-task3-q2-ui-evidence/receipt.json`，旧receipt及manifest未变。新进程/路径假设已检验，停止无界重复绑定，常规GUI缺口保留，不转交用户或改系统权限。核心88完整workspace在snapshot_git两项旧callback计数断言失败，尚未冻结/审查；实际已观察来源在下一lease回调前被新早期校验拒绝，正区分本地已观察与仅远端推进路径，保真实fetch、错误及文件/ref约束，不将原失败日志计绿。

以下五项以源码和程序边界测试完成，真实OS窗口/选择框交互仍归最后未完成项，不能混为已验GUI。

- [x] native folder dialog取消不执行CLI；固定Git argv校验并canonicalize所选工作树，内存token绑定根目录、切换撤销旧token，CLI前重核，未知token拒绝
- [x] 有限choose_project/initialize_project typed invoke，WebView无cwd/argv/Ensure；hasConfig只检测固定marker且区分缺失/错误，不读取对象作为main权威
- [x] ProjectPicker显式Init、默认docs/tower与受保护目录说明；已有marker不再问目录、不自动Init；会话确认目录记住，原文诊断/未提交提示完整显示
- [x] 可编译行为红→绿：取消/非Git/子目录/linked worktree/撤销token/迟到响应/空输入/中文引号参数/已有配置/无假批准；初始化期间禁切换
- [x] 真实临时Git仓库native→随包CLI Init，HEAD/原index字节及暂存条目/Markdown不变、重复ID/config不变；空仓库不创建HEAD/index，原init_cli回归
- [ ] component/native/check/build/必要fmt与核心init回归、实机显示、实际冻结SHA/manifest；SPEC→fresh quality→精确提交合入，不声称daemon或人工闸门已完成

## 2026-10-17 M0 整改（对照规格 v0.5 评审后）

整改总表：[`docs/superpowers/plans/2026-10-17-m0-remediation.md`](../docs/superpowers/plans/2026-10-17-m0-remediation.md)（未提交，草稿）。上一节的 S5 沙箱路线暂停，等 D3 裁决。

- [ ] 人裁决 D0–D6（D0 提交基线、D1 快照落点、D2 重建确认端点、D3 S5 机制、D4 init 推送检查、D5 追认规格改写、D6 覆盖率指标）
- [ ] 批次 0：提交现状基线；R16 规格同步
- [ ] 批次 1：R1 离线可用、R2 init
- [ ] 批次 2：R6 协议抽象与事件、R5 存储有界
- [ ] 批次 3：R3 快照落点、R7 重做、R4 重建可达、R14 快照时机
- [ ] 批次 4：R8 验证器与 S5、R9 冷启动输出
- [ ] 批次 5：R10–R13、R15

## 2026-10-17 继续开发：隔离验证器与冷启动 S5

此执行方向已被上方 M0 整改及 D3 裁决替代。以下保留历史工作与验证记录，未同步、未提交、未发布。

承接已记录的下一项，先完成 S5 真实执行，不扩展到 M3 harness 调度或 App/B0。继续开发已获授权；沿用 `/private/tmp` detached 工作树、现有依赖、未提交交付约定。当前工作区已有交付内容不得混入新提交。

- [x] 回顾经验、现有规格和未完成项，核对执行与批准边界
- [x] 重跑当前基线：`cargo test --workspace --locked` 320 项通过；默认沙箱的本机端口限制已定位
- [x] 在独立工作树实现固定提交、只读源码、独立输出、禁网及超时回收的验证器；隔离不可用时拒绝执行（双审 PASS、12 项真实沙箱专项通过）
- [ ] 经 daemon 接入 S5 执行请求；扫描继续只读，结构化命令去重，待确认命令不执行
- [ ] 保存不可变日志及哈希，证据经唯一 hub 定序并可从快照重建；不改变 Task 完成状态或慢层
- [ ] 完成规格审查、质量审查、真实隔离执行和 workspace 验证
- [ ] 同步精确文件，更新 README、中英文未发布日志与回顾，明确未提交/未发布状态

验证计划：先重跑已有 320 项基线；再验证通过/失败/超时/缺工具、固定提交与脏工作树隔离、越界写和网络拒绝、输出截断、后台子进程回收、日志落盘失败及 hub 重放/快照恢复；最后运行 fmt、Clippy、build 和完整测试。当前受工具沙箱限制，daemon 本机端口与 macOS sandbox 探测须在批准的本机执行环境运行；已确认沙箱探测成功。

阶段记录（尚未提交/发布）：Task1 独立规格与质量审查均 PASS，新增真实 sandbox 测试 12/12、原 Git timeout 10/10、repo 单元 3/3 通过。审查修复 APFS 大小写/Unicode 目录别名、探针与合法源码名碰撞；真实依赖构建暴露 `/dev/null` 子进程重定向问题，已添加仅该无持久状态设备的例外并复审，隔离版本为 v2。退出码 71 加 `sandbox-exec:` stderr 前缀可能把真实 Failed 分类为 Unavailable，原始退出码/日志保留，不会误报通过；作为非阻塞限制记录。

真实源码 API 验收：由当前已验收 Tower.md 源码创建独立临时 clone `/private/tmp/towermd-s5-acceptance-oqsnf9i5`，仅添加初始化配置与定向 Makefile 验证入口形成夹具提交 `0bc9536919a8ccdee47f17de7205987d32b5c73c`，tree `7ed7a0f44be9c960e7d6b8d6d7820782b34f1d68`。只读沙箱离线完整编译依赖，`cargo test --offline --locked -p towermd-core --test canvas` 18/18 通过；日志在 `/private/tmp/towermd-s5-api-probe/`。此时仅验证核心 API，CLI 与 hub 证据接入仍在实施，不能当作最终 S5 产品验收或本仓库发布证据。

## 2026-10-16 M0 后续执行（superpowers）

用户已要求持续推进，按 M0-2 → M0-3 → M0-4 的依赖顺序执行；不重复询问已有授权。M0-2、M0-3 及 M0-4 已确认的准备范围均已验证并同步主工作区；隔离 detached worktree 保留，不创建分支、不提交。

- [x] 重新验证 M0-1：77 项测试通过；阅读后续规格与 superpowers 技能
- [x] 独立审查 M0-2 边界；对相关决策、领取分支和有效路径三处歧义提请确认
- [x] M0-2：持久化 hub、幂等定序、原子多模块租约与 epoch（已验证并同步）
- [x] M0-2：权威 main 对象目录、用户级守护进程、工作树绑定与任务命令（含 WebSocket 订阅、任务卡与 harness 自报）
- [x] M0-2：规格/质量审查、并发与重启验收、同步已验证改动
- [x] M0-3：快照与恢复请求实现、双重审查、主工作区 276 项测试和 fmt/Clippy/build 通过；实际 App 放行仍不可用
- [x] M0-4 已确认范围：画布编解码、只读对象视图、候选模块扫描与测试计划；规格/质量复审和主工作区验收通过
- [ ] 后续缺口：隔离运行器及 S5 实跑、App 放行与基线发布；整个 M0 尚未完成

依赖：serde 1、serde_json 1、chrono 0.4、fs2 0.4、dirs 5、sha2 0.10、hex 0.4、tokio 1、reqwest 0.12 沿用 Note.md 版本线；axum 0.8 属于已获批准的守护进程 HTTP 选型。暂不引入 WebSocket 客户端库。hub 使用同步落盘的原子 journal，投影从事件重放，暂不引入 SQLite 派生索引。验证包含故障不更新内存、重启幂等、旧 epoch 无效、同 clone 多 worktree 和异 clone 同 ID 拒绝。

当前后续工作状态：**本轮确认范围已验收、未提交、未发布**。2026-10-16 人已确认：没有相关 Decision 时允许，显式引用须存在于权威 main；先创建目标工作树及 t/<task-id> 分支再领取，暂不迁移绑定；任务 allowed_paths 为实际范围，模块 paths 仅为上限。答复在本记录与实现计划中持久化，不重复询问。

M0-3 设计答复（2026-10-16）：人已确认 state.json 保存业务投影及 eid→seq 等最小恢复元数据，租约摘要仅追溯、灾难恢复全失效；恢复必须先将新 seq/epoch 边界写入权威 main，再开放服务，App 人工确认不得绕过。

M0-4 范围答复（2026-10-16）：人已确认本轮先交付画布编解码、候选模块扫描与测试计划；S5 在只读工作树实际执行测试，留待隔离运行器就绪。此范围不等于整个 M0 完成。M0-4 使用独立 `/tmp/towermd-m0-4` 工作树，不与 M0-3 复审混合。

M0-4 真实仓库验证：Note.md 固定源码提交 `f2bdc01b2bfd0bf9f9129e144e550360aab444b8` 的独立临时 clone，仅增加测试配置得到夹具提交 `715ce69cb579d06b98f47c6693cd6febdc603a0f`；tree `aeba1ff7e15fc841dc7a0b1d81053b304c51eebf`。最终主工作区构建的 CLI 两次扫描 JSON 字节完全相等：1952/1952 个已识别的已跟踪源码文件归入 25 个候选模块，S5 为 not_run。候选 Canvas 可解码，index 字节和工作树不变，无 hub 或 canvas 落盘，临时 daemon 已停止；Note.md 原仓库仍干净。

M0-2 历史验证进度：隔离工作树全量 158 项通过，随后补普通 frontmatter 与非对象路径两项红绿回归；fmt、Clippy、build 已通过。规格审查的六项缺口已补实现与回归：禁用 Git replacement refs、全树慢层对象路径保护、任务上下文展开、harness 自报、持续事件订阅、init/daemon 命令续租。最终结论见下段。

M0-2 规格与质量复审均 PASS；主工作区最终 163 项测试、fmt、Clippy（-D warnings）、build 全通过。质量修复包含 daemon 进程组隔离及显式无效 --run 拒绝停机。已精确同步主工作区，未提交、未发布；HEAD 未改变。随后在 `/tmp/towermd-m0-3` 开发快照与恢复基础，最终结论见下段。

M0-3 审查结论：2026-10-17 人已批准无远端且 main 在任一工作树检出时一律推迟快照，待用户自行切到其他分支再写；远端及未检出的本地 main 不受影响。已更新 FR-91 与计划并删除 commit --only 路径；符号 main 采用拒绝与 no-deref 防护，恢复网络期间释放 hub 锁并在安装前复查并发变化。独立质量复审关闭原三项 P1，专项 51/51 通过；隔离工作树及主工作区均完成 276 项完整测试与 fmt/Clippy/build。未提交、未发布。

M0-4 规格审查修复已完成红绿与复审：浅克隆不据残缺历史合并模块；父目录 paths 不跨入子模块；画布边只绑定 Interface；合法换行文件名按字面读取；只读续租不处理其他过期 Run。质量审查另修正 CI cwd 等价写法丢命令、未识别 Make/Cargo 误报无测试、复制 hub 后绕过持久 clone 绑定三项问题，原审查者均复验 PASS。S5 仍为 not_run，不生成通过证据。

### 本轮最终回顾（2026-10-17）

- 主工作区最终 `cargo test --workspace --locked` **320 项通过**；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked` 全部退出 0。
- 新增专项：codec 18、只读视图 5、coldstart 10、adapter 10；task 25 项含新续租边界回归。独立 clone 绑定故障矩阵另有 18 项验证通过。
- 精确同步 48 个交付文件并核对冻结内容；更新 README 与中英文未发布日志。没有新增 M0-4 依赖，没有批准、测试执行或慢层写回入口。
- 仍未提交、未发布，无新增提交号；HEAD 为 `2a39a20e680a8995f1e9c2b260bcbbffff6c8148`，暂存区为空，原有 AGENTS.md/CLAUDE.md 未改。保留隔离工作树，不自动清理未提交内容。
- 后续先补隔离运行器与 S5 真实证据；App 确认、B0 生效和离线发件箱恢复依然待实现，不能把本轮准备能力标为整个 M0 完成。

## 2026-10-16 M0-1 执行（superpowers）

范围：执行已有 M0-1 计划，完成核心对象、配置和 `towermd init`；M0-2–M0-4 保持后续计划。

- [x] 阅读规格、设计原则、OKF 约束与 M0-1 计划；确认沿用 Note.md 依赖版本线
- [x] 用户同意安装 Rust stable 并继续实现 M0-1
- [x] 安装工具链，在隔离 worktree 中建立 Rust workspace（Rust/Cargo 1.99.1；骨架构建通过）
- [x] TDD：OKF 往返与类型/层级登记（OKF 16、types 9 项通过，规格与质量审查通过）
- [x] TDD：配置读取与未知字段保留（10 项测试通过，规格与质量审查通过）
- [x] TDD：init 核心及路径、已有文件保护（规格与质量审查通过，含并发与 linked worktree 回归）
- [x] TDD：CLI 非交互与真实终端交互（15 集成、3 单测通过；PTY 默认/重入/EOF 验证通过）
- [x] 规格审查、代码质量审查、测试与构建验证（主工作区 77 项测试、fmt、Clippy、构建全部通过）
- [x] 更新使用说明、中英文未发布日志和执行回顾

执行约定：使用 `/tmp/towermd-m0-1` 的 detached worktree 隔离开发，验收后将本次文件同步回当前工作区。只安装工具链，不改 shell 启动文件或全局 git/harness 配置。当前交付状态为**未提交**；不把已有未跟踪文件混入提交。

### 回顾

- 已实现：Rust workspace、OKF 文档、14 项 type 登记、层级判定、配置读取和 `towermd init`。依赖沿用 Note.md 的版本线，clap 加 derive；没有另引入依赖。
- 工具链：经用户同意安装 Rust/Cargo 1.99.1 与 rustfmt、Clippy；使用 `~/.cargo/bin`，未修改 shell 启动文件。新终端可执行 `source "$HOME/.cargo/env"` 使用 cargo。
- TDD：OKF、types、config、init/repo、CLI 均记录失败再通过；审查发现的不可序列化 YAML 值与 broken pipe panic 均补红绿回归后修复。
- 隔离 worktree 验证：77 项测试通过（核心 59、CLI 单测 3、CLI 集成 15）；fmt、Clippy `-D warnings`、workspace build 通过；并发 init 专项重复 30 次通过。
- 真实终端：首次回车取默认目录，再次执行无询问且 ID 不变；Ctrl-D 退出码 1，未生成配置与文档目录。
- 子代理完成各模块规格与质量审查及最终跨模块审查；执行前发现的路径越界、符号链接、文件占位、并发覆盖、Git 路径裁剪等问题已修正并测试。
- 已将本次文件同步回 `/Users/bruce/git/tower.md`，主工作区使用 `--locked` 完成全部 77 项测试、Clippy 和构建，格式检查通过。**未提交、未发布，无提交号**；HEAD 仍为 `2a39a20`，暂存区为空；原有 `AGENTS.md`、`CLAUDE.md` 未修改，任务记录仍保持原有未跟踪状态。
- 下一步：M0-2 本地 hub、守护进程与任务命令；M0-3 补快照和 FR-91 远端可推送性检查，M0-4 补画布与冷启动。当前不宣称整个 M0 完成。

## 2026-10-16 规格 v0.5：技能加 App、用户文档目录、唯一 hub 与快照

背景：用户说明 Tower.md = 技能包 + 独立 App；md 文档放用户指定的文档目录，`.towermd/` 只放隐藏配置；多 harness 多工作树、多机执行需要一个全局协调点（Cloudflare Worker）；活跃实时状态以唯一服务为主，快照存回 git 仓库。v0.1 选"技能优先，调度后置"。

- [x] 0 摘要、0.2 依赖分层、1.2 非目标：改为技能包 + CLI + App；hub 为唯一活跃状态源
- [x] 5 系统架构：三个面、hub 的两种实现、快照、各端协作；FR-04/05/06 改写
- [x] 6.1 目录：`.towermd/` 只放配置；`<docs_dir>/` 放慢层 md、`tower.canvas`、快照
- [x] 6.2 模块去掉 `map`，坐标进 `tower.canvas`；新增 6.5 tower.canvas 数据模型
- [x] 6.3 事件：hub 定序 `seq` + 客户端 `eid`；租约与快照事件
- [x] 7 分支：技能优先下的分支约定、租约 epoch、路径守卫改为检查加事后检测
- [x] 10 执行：新增 10.0 技能优先模式与 Tower.md 技能包；无头双环标为 M3
- [x] 12 中断：CLI 通道（实时等待/deferred）；插话随下一次 CLI 调用送达
- [x] 16 接口：CLI、hub 协议
- [x] 17 NFR、18 里程碑重切、20 风险与待定问题、21 追溯
- [x] 自检（占位、一致性、范围、歧义）→ 独立检查 → 提交
- [x] 交人审阅；Q10 原则与不变量改写待人确认（2026-10-16 人答复"都按你的改"，已改 AGENTS.md 与 product-principles.md）

需人确认：`product-principles.md` 信念二与 `AGENTS.md` 不变量的改写建议（规格里注明，不直接改）。

### 回顾

- 自检：FR-01–FR-112 无重号缺号；脚注引用与定义一一对应；README 改为 v0.5 描述。
- 独立检查（子代理通读全文）提出 25 条，核对属实后全部处理。要点：
  - 证据改为绑定 tree，不绑定 SHA；写 trailer、压缩检查点不再让"已合并"无法成立（FR-107、FR-12）。
  - 快照与慢层提交一律用底层命令在 `origin/main` 上生成，不移动本地分支、不碰工作树、不用用户 git 身份；git 远端的比较并交换加 `(epoch, seq)` 检查实现 fencing（FR-91）；主干租约的取得与交接（FR-110）。
  - `seq` 连续无空洞、快照只覆盖连续前缀、截断后保留 `eid` 索引；重建时 `seq`/`epoch` 跳号、租约作废、从 `main` trailer 补事件（FR-87、FR-92、FR-93）。
  - 失效 `epoch` 的事件统一为"记录但标过期、不参与投影"。
  - 威胁模型写明 v0.1 防粗心不防恶意；`human:` 事件只能来自 App 签名（FR-111）；签名绑定内容哈希，公钥不读仓库。
  - 租约 TTL 与续租、`ask`/`finish` 分段等待以避开 harness 命令超时（FR-112、FR-59、FR-107）。
  - 里程碑：B0 批准与提案提前到 M1；一句话生长改由 `towermd-seed` 技能执行；import 解析与社区发现推迟到 M2。
  - 依赖总控的状态转换与交互逐条写出 v0.1 降级行为。
- 未处理：把 M3 细节移到附录（篇幅调整，留到写 M0 计划时再看）。

## 2026-10-16 M0 实现计划（superpowers writing-plans）

M0 拆成四份计划，放在 `docs/superpowers/plans/`，逐份写、逐份交人确认执行方式。

- [x] M0-1 核心对象与 init：`2026-10-16-m0-1-core-objects-init.md`
- [x] M0-2 本地 hub、守护进程单实例、多终端运行判定、`status` 与 `task add/claim/show/release`（主工作区 163 项验收通过，未提交）
- [x] M0-3 快照写入、主干租约、`hub rebuild` 请求（主工作区已验证，未提交；App 执行入口不在本轮）
- [ ] M0-4 `tower.canvas` 编解码、对象目录加载、冷启动 S1–S5

需人确认：

- 写计划时本机没有 Rust 工具链；2026-10-16 用户已同意安装并继续实现，执行状态见本文件顶部。
- clap 沿用 Note.md 的 4.x，但加 `derive` 特性（Note.md 用 builder 写法）。理由：M0 起子命令较多，derive 写法更短、更不易错。这不算新增依赖。
- `AGENTS.md`、`CLAUDE.md`、`tasks/` 目前未跟踪，是否纳入版本控制由人决定。
