# 任务计划

## 2026-10-17 持续开发至首个可用 App（未签名）

用户追加目标：“请一直继续开发下去，直到发布第一个可用版本app，先不签名”。先完成下方 M0 整改，再按已批准规格推进 M1 技能与 App，完成首个可安装 macOS 版本；签名与公证暂不做。发布前核对所有 worktree、分支、stash 与冻结源码/验收证据，精确提交并更新双语 CHANGELOG。整改阶段继续不推送，发布动作以实际完成的可审阅产物为准，不把计划或工作区测试称为发布。

- [ ] 完成 M0 整改各批与真实 daemon 验收
- [ ] 规划并实现 M1 核心人工闸门、技能/CLI 与桌面 App 的可用闭环
- [ ] 实机验证 App、打包未签名安装产物、核对发布证据与发布记录

构建环境准备：已为现有 Rust 工具链安装 `x86_64-apple-darwin` 标准库目标，`rustup target list --installed` 确认与 `aarch64-apple-darwin` 均在；本机 Node v26.9.0、Xcode 26.6、macOS 26.5.1。仅完成环境准备，尚无 Intel/Apple Silicon App 构建、Intel 实机验收或发布产物。

App 基础实施计划：`docs/superpowers/plans/2026-10-17-m1-app-foundation.md`，提交 `6512179`。已对照实际 CLI/daemon、Note.md 工具线独立审查并修正项目 pnpm 路径、租约 epoch 与连接代际的区别、画布 opaque 条目解码，复审 PASS。当前仅完成计划，尚未实现或构建 App。

P6 人工闭环详细计划为 `docs/superpowers/plans/2026-10-17-m1-app-workflows.md`，提交 `a63d1af`，同时最小同步基础计划的启动与纯展示接线。独立审查及措辞 delta 复审 PASS：Ensure 复用唯一 daemon 启动原语，projection/object/events 展示不续租或消费消息；原请求协调、S7/S8、恢复确认、无 Run 违规处理和真人交接均有明确所有者。14 个实施 Tasks 尚未执行，不将文档审查称为 App 功能或真人验收。

P2 任务证据与 P3 人工干预详细计划已写入 `docs/superpowers/plans/2026-10-17-m1-task-evidence.md`、`2026-10-17-m1-human-intervention.md`。独立联合审查发现并修复：模块 ID 不能随问题文本归一化；人工事实先经恢复发件箱持久化；压缩后的请求幂等由完整归档重建 receipt 索引。复审 PASS。已明确 P2 Task1 → P3 Task1 → P2 后续的真实编译顺序，G3 不可变证据与 G4 独立人工事实；这些仅为实施计划，未实现、未真人验收。

P4 详细计划为 `docs/superpowers/plans/2026-10-17-m1-baseline-approval.md`。独立审查后补齐批准/请求的持久 outbox、ready/running Task 的独立 App 复核解除路径、正常出版与恢复共用 commit 推导的基线 eid，复审 PASS。当前仅完成计划；慢层批准、基线出版和真人操作尚未实现或验收。

P5 详细计划为 `docs/superpowers/plans/2026-10-17-m1-main-observer.md`。独立审查后补齐无 live Run 的 App 转提案上下文、永久精确认领事实与待点击 TOCTOU 的区别，复审 PASS。首次检测 frontier 已按推荐工程假设冻结并同步 FR-103，独立审查 PASS：首次引入项目 ID 的整笔提交及祖先只作导入，不能批准 B0；其后全部检查，历史不完整则阻认证。已给可选澄清和回复机会、知识库未找到另定边界依据，不冒称用户已裁决。事后检测源码尚未实现。

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
- [ ] 3.3 E0：唯一 DTO、持久输入与纯 raw/G5/hash
- [ ] 3.3 E1a：只读实际输入、私有可信历史、纯 proof 与 canonical builders
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
