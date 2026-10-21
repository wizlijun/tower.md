# 当前计划：骨架上的最小可测试 App

起点：用户保留的 `938b8ed`；旧计划和旧验收仅为历史，不作为当前完成证据。

本轮终点是一个可实际打开真实仓库的 macOS App，默认只有画布。实现 spec §15.1、Z0、FR-83、FR-100–102 的读取切片；批准、任务执行、快照恢复、实时状态仍按后续主干逐项接入，不能标记完整 M0/M1 已完成。

- [x] 阅读当前代码和 spec，核实框架实际能力；两项独立审查确认无可运行桌面壳。
- [x] 核心：从固定权威 main/origin/main 投影 JSON Canvas 卡片与关系，显示原文、悬空诊断和未摆放模块；不读工作树副本。
- [x] 桌面：原生目录选择，native token 绑定路径，取消保留、旧响应失效；不启动重复 App 或守护进程。
- [x] 画布：文本／文件／链接／分组、孤立节点与一般关系图，平移缩放；节点详情、项目说明和菜单均按需出现。
- [x] 错误和空状态可重试／切换；未初始化／缺 main／损坏画布不伪装批准模块或完成进度。
- [x] 完成核心边界测试、UI 流程测试、类型检查和实际原生构建；Root 单进程串行运行工具链，Cargo 并行最多2。
- [x] 创建独立真实 Git 验收仓库，实机确认画布、菜单、原文、刷新、取消；篡改工作树画布后仍读 main。只保留一份可交互 App。
- [x] 更新真实使用说明和中英未发布日志，精确提交验证代码，交付可运行候选及操作路径。

依赖沿仓库已确认选型和 Note.md 现有版本线：Tauri/dialog 2、Svelte/TypeScript 5、Vite 6、Svelte Flow 1.6.6、Vitest 4、testing-library 5、happy-dom 20；不新增未裁决库。画布使用既有坐标，无需新增自动布局依赖。

后续主干顺序：唯一 daemon 项目业务入口与快照写入/恢复 → Agent 候选与 App 草稿/批准 → 一个真实任务的领取、问答、验证及合并认证 → 完整 M1 验收。任何写入入口先闭合对应不变量，不用前端直写文件替代。


## 本轮结果

实现已提交：`c9c985143f5d0332f10b0c25b22ccee08b48837c`。177 项工作区 Rust 测试、3 项原生会话测试、16 项前端测试通过，类型检查零错误/警告；生产 App 已构建并实机操作。最终只保留一个 App 进程，打开随候选保留的真实 QA 仓库。

详细范围、实际操作、构建哈希和候选路径见 [验收记录](../docs/verification/2026-10-20-minimal-canvas-app.md)。当前终点是可运行的只读画布；完整 M1 未完成。用户原有未跟踪资料未纳入提交。


## 继续：真实仓库 → 冷启动候选画布

从 `01874ad` 继续，先闭合 spec §8.1 的 S1–S4 确定性扫描与只读审阅。S5 验证、S6 模型提交、S8 批准依次后续接入；不以未执行替代“无测试”或“通过”，不绕过唯一写入/快照/人工闸门。

- [x] 从固定权威提交盘点源码与清单，目录聚类并纳入历史耦合，产生真实 Module 草稿、候选验证命令、覆盖率/模块数/未划分诊断。
- [x] daemon 接项目读取、异步扫描、草稿读取；可重建结果只存公共 Git 目录；同 clone 工作树复用、同项目异 clone 拒绝。
- [x] CLI 提供扫描与等待结果，App 经打包的独立 CLI 启动 daemon 后由 native 客户端访问；不重新启动 App、不暴露 human 动作。
- [x] App 在画布右键/空状态启动扫描、查看候选及扫描说明、返回原 main 画布；默认仍只有画布。
- [x] 固定提交/脏工作树/冷启动指标/缓存重建/daemon 并发与重启/UI异步测试；串行构建，Cargo jobs≤2。
- [x] 新候选实机验证真实仓库扫描、预览、返回、错误恢复；只保留一个 App，提交源码并更新双语日志与验收记录。


### 本段结果（2026-10-21）

实现提交：`4d2d30c91f31099250d926a4816909dbdcb09c78`。完成固定提交扫描、daemon/CLI 接入、随包服务与候选画布。工作区 188 项 Rust 回归、原生 4 项、前端 23 项通过；实机发现的布局和合并模块命令遗漏已修正，最终冷启动 9 项、daemon 4 项专项复验通过，生产包重新构建并实机打开扫描。

最终 App：`.release/cold-start-20261021/Tower.md.app`，当前保留一份 App 与一个 daemon。真实 QA 扫描 40 个识别源码文件、3 个候选模块；main、index 与工作树不变。范围、构建哈希、实际扫描 ID 和限制见 [验收记录](../docs/verification/2026-10-21-cold-start-canvas.md)。未发布正式 release，未宣称完整 M0/M1。

下一段：S5 共用验证器（固定提交、分离工作树、真实命令结果、超时与进程清理）；随后接 S6 Agent 提交及唯一写入/恢复所需底座，再实现 App 审阅批准与任务闭环。不能把当前候选视为已批准结构。

## 继续：S5 固定提交的真实验证（2026-10-21）

沿 `851867d` 继续，不恢复旧业务模块。按 spec §8.1 / FR-107 建立可复用验证内核；`baseline scan` 扩展为 S1–S5，不增加独立 `baseline verify`。扫描明确会运行仓库中的候选命令，继承启动 daemon 时的环境并允许网络；不自动安装依赖。

- [x] 核心验证器：固定 commit/tree、公共 Git 目录下分离工作树、串行去重命令、真实 exit/耗时/有界日志、每模块通过/失败/无测试结果；共享命令只执行一次。
- [x] 生命周期：每命令及整批期限、取消与整个进程组回收、工作树清理；独立 worker 监视父 daemon 管道，daemon 意外退出时也取消子进程，不将中断冒充通过。
- [x] daemon 与 CLI：复用扫描 ID / wait / show，增加取消；先给候选，再推送验证结果；重启保留已完成报告，正在执行的请求不能恢复为通过；全局最多一个验证 worker 执行命令。
- [x] App：只在画布和按需详情呈现实际验证状态、共享命令、错误/日志摘要与取消；快层更新不移动节点，不增加批准入口。
- [x] 自动化：真实成功/失败/无命令、共享命令、环境继承、源码污染拒绝、超时/取消/父退出回收、缓存重启；Root 串行运行工具链，Cargo jobs≤2。
- [x] 构建与实机验收：独立小型真实 QA 项目，不运行用户开发仓库命令；一个 App、一个 daemon，精确提交并更新双语日志和产物证据。

实施约定：不新增依赖；使用现有 serde/sha2/fs2 与标准库进程 API。验证工作树不是安全沙箱，执行权限仍是本用户；这一点在执行入口说明，不伪装隔离能力。结果绑定固定 tree，命令改变已跟踪源码或留下不可回收进程时不得通过。尚未接入 hub Evidence/快照/批准的结果明确为冷启动候选验证报告，不认证任务完成。

### S5 本段结果

实现提交：`c8629c1a8cb639b69f4b5754a4e80ec8f910b413`。工作区 Rust 206 项、原生 4 项、前端 34 项通过，类型检查零错误/警告，生产 App 构建成功。实机确认通过/失败/无测试、失败日志、执行中取消和清理后终态；取消后的真实子进程与临时工作树均已回收，QA main/index/工作树未变化。

App：`.release/s5-verification-20261021/Tower.md.app`，最终只保留一个窗口和一个 daemon。完整范围、实际扫描 ID、包哈希、使用方法与剩余主干见 [S5 验收记录](../docs/verification/2026-10-21-s5-verification-app.md)。本机候选可测试，尚未正式发布，也未完成全部 spec。

## 下一阶段：S6 综合提交与持久草稿

起点 `5c192a2`。按 §8.1、FR-18/19/95/104 实现用户会话 Agent 的一次综合，经 `baseline submit` 进入同一画布；使用已保留的 `baseline-submit.schema.json`。本阶段不新增依赖，不恢复旧框架，不创建批准事件、B0 或 main 提交。

- [x] 核心契约：同一 schema 校验；OKF 类型/层级/路径/引用及项目来源检查；保留未知键；拒绝伪造人类批准，验证证据只来自原 S5。
- [x] 最小综合允许模块名称/职责/置信度、意图、接口、验收、风险和待确认问题；模块 ID、文件边界与执行命令冻结，合并拆分留 S7。
- [x] 守护进程经独立 Git plumbing 写 `refs/towermd/drafts/<id>`；耐久保存输入、S5 报告与投影；幂等重试、重启读取、缓存删除可重建，main/index/用户文件不变。
- [x] CLI schema/template/submit/list/show；冷启动技能使用真实扫描输入和固定提交文档，只读综合，不修改全局 harness 配置。
- [x] App 按需选择综合草稿，同一画布显示模块、意图、接口和验收；详情展示原文、风险/问题及来源。切换/迟到响应不串项目。
- [x] 串行测试和构建，实际 App 查看 Agent 提交的 QA 草稿；核对私有引用/证据/清理，只保留一份 App，精确提交与双语日志、验收记录。

计划已按现行 spec 与独立审查核对。S6 原文不能只存可删除缓存：私有 Git 引用作为原件，画布与对象为可重建投影；日志丢失时不得假报仍可复核。producer 由 Agent 自报，仅作来源说明，不是可信人类身份。

### S6 本段结果

实现提交：`15e7e39d291d310ac5176d9f2fea0f55912305c3`。工作区 Rust 214 项、原生 5 项、前端 42 项通过，类型检查零错误/警告，生产 App 构建并实机验收。独立 Agent 按项目技能生成真实 QA 综合；同一画布显示 9 个节点、5 条关系，保持真实失败/通过/无测试。幂等重试、缓存移走与服务重启恢复原件、日志缺失实机警示均已验证，QA main/index/工作树未变。

App：`.release/s6-synthesis-20261021/Tower.md.app`，最终只有一个 App 和一个 daemon，保留 QA 草稿供试用。源码、包哈希、实际草稿与使用方法见 [S6 验收记录](../docs/verification/2026-10-21-s6-synthesis-app.md)。这是本机候选，未正式发布，完整 M1 尚未完成。

下一阶段：按 S7 补齐人工草稿审阅编辑；在 S8 批准前闭合唯一主干写入、租约与快照恢复。模块边界或验证命令改变必须重新绑定验证证据，不能沿用旧“通过”；批准仍只能从 App 的明确人工动作发出。

## 持续完成：S7 编辑到完整主干（2026-10-21）

用户要求继续直到全部完成。以 `f89da79` 为起点，逐段精确提交，不把 S7 或单个候选当作完整 M1。规划先按现行 spec §8.1 与产品不变量核对；保持单画布、单 App、单 daemon，工具链由 Root 串行执行。无新增依赖。

- [x] S7 核心：改名、合并、拆分、拖动、补接口、确认/修改验证命令、引用固定提交已有 Markdown。每次修改形成不可变草稿修订，原件可重放重建。
- [x] S7 边界：只有 App 人工入口记录 `human:local` 审阅动作，无 CLI 人类动作；结构/命令变化使受影响模块待重新验证，旧通过不能沿用。私有引用写入不改 main/index/工作树。
- [x] S7 App：操作只在选中/右键后出现；异步失败可用同输入重试，项目切换不串回执；坐标持久化，快层变化不重新布局。
- [x] S7 验证：核心真实文件归属、引用重映射、未知键保留、非法输入；daemon 持久化/重启/身份/并发；UI 流程；实机编辑及重新读取。构建并精确提交。
- [ ] S8 前置：唯一主干写入、租约、耐久快照与重建、App 人工批准身份；核对当前保留框架可复用 API，以实际状态机与故障测试证明。
- [ ] S8：完整审阅后的对象与画布经明确人工批准出版 B0，内容哈希一致、main/tag 可验证，失败/重试不重复批准或误报完成。
- [ ] 后续任务主干：领取、问答/规则去重、门禁、真实验证、合并认证；按现行 M1 条件完成真实多工作树与 Agent 验收。
- [ ] 交付：核对所有分支/worktree/stash/未提交范围，提交并构建无发行者签名 App，完成整体实机验收、增量中英日志、冻结产物证据与使用说明；没有证据的条目保持未完成。

本轮 S7 实现采用动作日志加原 S6 输入重放，避免可删除缓存成为唯一状态。合并/拆分后的证据重绑定和重新运行验证必须在批准前完成。审批与主干出版仍属于后续明确动作，编辑保存不会自动批准。

### 优先级调整：先交付功能组件框架与最简 sample

用户最新指示：先做功能组件框架，显示什么和流程细节待功能完整后由用户决定，采用 spec 最简设定作为 sample。暂停继续深挖 S8 底座，保留上述未完成清单；先交付可以看到完整组件关系的单画布示例。

- [x] 保留并验证本轮 S7 真实编辑实现，不为示例改动真实仓库语义。
- [x] 提供独立内存示例：3 个模块与基线、任务、提问、验证、放行组件；固定坐标、按需详情，主界面仍只有画布。
- [x] 最短可操作流程：确认示例基线→领取示例任务→回答问题→模拟验证→按证据结果放行或退回；失败/无测试不作为通过，退回后须重新验证。
- [x] 示例明确标识，与真实 daemon/仓库无读写耦合；可返回当前真实项目，不丢未确认写请求。
- [x] 自动化、生产构建与实机点击示例/S7编辑；精确提交、保留唯一 App，给用户简短操作路径和真实能力边界。

### 组件框架与 S7 结果

实现已提交：`a8becfa1aabf9c063d1e57b768b7db17b6bbd9b3`。225 项工作区 Rust、7 项原生、59 项前端测试通过，daemon 最终专项 4 项复验通过，类型检查零错误/警告。生产包实机确认示例流程、真实编辑、引用、位置保存与重启读取；QA main/index/工作树不变。

当前 App：`.release/s7-components-20261021/Tower.md.app`，保留初始示例画布。详细证据及剩余边界见 [组件示例与 S7 验收](../docs/verification/2026-10-21-component-sample-and-s7.md)。S8、审阅后重新验证与真实任务主干仍未完成；本轮按最新指示优先交付组件 sample，不把模拟流程计作真实能力。

## 继续完成 v0.1 主干（2026-10-21）

用户要求在组件框架基础上继续直到全部完成，沿用单画布、最简交互、无发行者签名、避免重复进程等既有要求。以 `61ec739` 为起点，完整范围按 spec 的 M0/M1 验收；M2/M3 的云端和无头调度不提前扩展。规划已对照现存代码核实：Hub 内核可复用，daemon 尚无业务会话，不能用增加批准按钮替代接通主干。

- [ ] 统一业务事件入口：pending 耐久、Hub 定序、完整回执核对、confirmed 耐久；业务投影可重放，丢失 Hub 时禁止静默创建空服务。
- [ ] 唯一主干写入器：临时 index、主干租约、快照连续前缀、私有引用降级、恢复边界先落 Git 后开放服务；App 确认恢复。
- [ ] 审阅后真实验证和 App B0：复用验证器、绑定审阅输入和固定 tree，原件耐久；批准绑定内容哈希，出版对象/清单/标签，故障幂等。
- [ ] 实际任务：创建、工作树运行绑定、领取/释放、模块租约与 G0，问答和人工插话，CLI 技能。
- [ ] 完成认证：路径/慢层守卫、真实验证、trailers、main 观察认证与违规中断；进度只认证据。
- [ ] 提案与后续基线、单画布实时投影、关注项与 App 人工入口；采用最简组件，不展开未定显示细节。
- [ ] 串行自动化和真实 3 工作树/10 任务主干验收，清理自有进程，精确提交并构建可运行 App；按证据核对全部 M0/M1 条目，剩余项不得冒充完成。

不新增依赖。并行 Agent 只承担不重叠源码和审查，工具链仍由主 Agent 串行运行、Cargo jobs≤2。每段验证完成后独立提交，最终记录冻结源码与产物。

### 主干第一批已验证结果

实现提交：`3b0dc84d60c3a84cefb9d3bd5fa721ce66cc4ab2`。292 项工作区 Rust、10 项原生、67 项前端测试通过，类型检查零错误/警告；Rust 源码随后仅作 rustfmt。原始日志在 `/private/tmp/towermd-mainline-workspace-third.log`、`towermd-mainline-native.log`、`towermd-mainline-ui-tests.log`、`towermd-mainline-ui-check.log`。

这一批接通审阅后重新验证、App 明确批准 B0、真实 main/tag 出版及未知结果原请求重试、pending/confirmed 与快照、App 确认恢复、CLI task add/claim/show/release。真实 Git 端到端验证覆盖有/无 origin、用户 index/工作树不变、Hub 删除及恢复重复确认、两工作树争领、旧租约与伪造基线拒绝。尚未打包新 App，也未把自动化结果当作本批 UI 实机验收。

继续项：补快照出版事件与私有快照转送；完成 finish/G5 与实际 main 接入认证、main 违规检测、问答/插话、提案、真实画布任务状态；随后整体验收和交付。新模块 `task_finish`、`main_inspection`、`snapshot_publication` 与问答模块尚未验证，未纳入以上提交。

问答归一化沿用用户已明确批准的 `unicode-normalization` 0.1 版本线；本轮复核 Note.md `src-tauri/Cargo.toml` 为 0.1、锁文件为 0.1.25。该依赖用于 spec FR-62 的 NFKC；属于此前已批准选择，不再次请求确认，其余不新增依赖。

### 继续实施与验收顺序

按 §18 的 M1 验收范围核对：本轮只完成 G0、G2、G3、G5；G1/G4/G6、seed、临时规则、回答出版为 Decision 属 M2，不为这些后续功能延迟第一版。已具备的回答投影去重保留，不能称已完成 M2 的仓库决策出版。

- [ ] 当前批：串行验证 finish/G5/main 认证、快照出版与恢复、实际问答/插话、App 任务状态；每个失败先核真实原因，不降低证明标准。
- [ ] M1 缺口：提案内容/差异/影响清单、App 批准或驳回、同一提交出版 ADR 与新基线；慢层越界转提案，main 禁止中断提供认领/撤销/转提案。
- [ ] 用户路径：技能按真实 CLI 补齐；App 入口和按需详情可达，暂停/汇报只走插话，禁止制造已控制 harness 的假状态。
- [ ] 发布前：新候选实机、3 工作树和 10 个真实任务、Claude Code 与 Codex 两种会话；核对所有现存源码与发行产物，不复用旧 App 的验收结论。

工具链仍由主 Agent 单队列运行，测试并行限制为 2；实现和只读审查可并行。当前运行的 S7 App 只代表旧候选，在新构建验收前不声称它包含本轮任务和问答功能。

### 第二批验证记录

本批接通 finish/G5/真实 main 合并认证、问答/插话与运行证据详情、main 违规认领和精确撤销、完整提案及后续基线出版，并修复伪快照 trailer 阻断恢复和合并模块遗漏旧文件的问题。实现已提交 `fbb1e3f`，尚未实机验收；下一批的暂停/恢复和 App 创建任务文件不纳入本批。

- 工作区全量：349 项通过，问答 CLI 新增 finish 夹具因缺实际提交失败；补真实代码提交和 rebase 后，问答专项 1 项通过，未修改门禁。日志：`/private/tmp/towermd-proposal-resolution-workspace-first.log`、`/private/tmp/towermd-question-cli-second.log`。
- 原生 11 项通过：`/private/tmp/towermd-proposals-native-first.log`。
- 前端 85 项通过：`/private/tmp/towermd-proposals-ui-tests-fourth.log`。新增互斥测试的夹具改用含人工审阅信息的草稿；不确定人工请求期间禁止另一批准/验证。
- 静态检查日志：`/private/tmp/towermd-proposals-ui-check-second.log`；完整产品验收仍待新 App 和真实多工作树执行。

后续基线影响任务时，M1 保留 needs_review 和历史证据；技能明确释放旧运行、按新批准基线创建新任务及依赖后再领取，不提供清除标记的绕过入口。

### 第三批：闭合最简使用路径

- [x] App 创建任务，全局暂停/人工确认恢复，CLI 恢复仅请求；暂停在 Hub 拦新模块租约，既有运行只发检查点插话。自动化已验证，实机待验。
- [x] 主画布角标纳入真实禁止/验证失败/提问/提案，关注项按严重度排序，保持节点位置；不新增常驻面板。自动化已验证，实机待验。
- [x] 慢层回答捕获的提案可经正式提交补全，不因回答而自动放行。真实 Git 出版与恢复专项已验证，实机待验。
- [x] FR-49 原生 WebSocket 推送：采用用户批准的 tokio-tungstenite 0.29；daemon、原生单订阅和画布已接通，恢复、乱序及取消自动化通过，实机待验。
- [ ] 串行定向与全量回归、单实例 App 构建、真实主路径验收、精确提交及最终发布范围核对。

FR-49 依赖取舍：Note.md 主 App 未用 WebSocket，其 openclaw 插件使用 tokio-tungstenite 0.24；本仓库 axum 0.8.9 的 ws 功能使用 0.29。建议 App 原生层同用 0.29，避免两条协议库版本线，限本地 ws 不加 TLS。此前按 AGENTS 规则提请确认；用户已于本轮明确批准 0.29，现已按该版本线实现，等待最终回归和候选构建。

### 第三批自动化结果与交付边界

实现提交：`4736d96`。工作区回归 365 项通过、1 项失败；失败源于新增测试以 JSON 解码框架实际返回的非 JSON 422 错误。保留产品拒绝，测试改为核实际状态和正文后，该慢层回答→正式提案→B1 出版→重启/Hub 重建专项 1 项通过。没有以回答直接解除慢层阻塞。

证据：`/private/tmp/towermd-controls-workspace-third.log`、`/private/tmp/towermd-slow-answer-focused-fourth.log`。原生 12 项通过（`towermd-controls-native-first.log`）；前端最终 97 项通过（`towermd-controls-ui-tests-final.log`）；静态检查零错误/警告（`towermd-controls-ui-check-third.log`）。日志均在 `/private/tmp/`，此前失败日志保留。

下一步先用这一批冻结源码构建唯一候选，再做实际 App 用户路径和真实双 harness 三工作树十任务验收。准备中的 QA 脚本与模板不计完成证据。FR-49、完整实机与正式发布仍未完成；旧 S7 App 不包含这三批主干更新。

候选 `.release/mainline-4736d96-20261021/Tower.md.app` 已构建并启动；旧 S7 App 与旧 daemon 已退出。源码归档、逐文件 SHA-256、构建日志已保存在同目录，包内六个技能与冻结源码一致。仅 macOS Apple Silicon，ad-hoc 标记、无发行者签名或公证，尚无正式发布 tag。

隔离 QA 位于 `/private/tmp/towermd-m1-qa-20261021`，真实三个工作树已建，初始 main 为 `b0249c66ea0030fed7fd513fd9aa2d5521219da1`。候选 CLI 实际扫描 `01a15abd-446a-729d-a293-bbc1c9fb1e21` 的三个模块验证通过。CLI 与 App 使用同一个默认 runtime（不是准备脚本 setup 的临时 runtime；setup 不启动 daemon），恢复时只处理此 QA project 的数据。屏幕锁定导致 `cgWindowNotFound`，不能把启动进程计作 GUI 验收；真实 S6 综合继续，B0 与十任务仍待验收。

真实 Codex 0.163.0 已生成并提交 S6 草稿 `d-c7e3bed335e06faca9cb379c0e2da69e2d304af004c1f48ed08e8fb265091c3f`，包括 3 模块、1 意图、3 验收、无虚构接口。首次提交被 harness 工作区沙箱拒绝访问实际 `runtime/startup.lock`；原错误保留，随后只在本次 harness argv 加入该运行目录的 `--add-dir`，以完全相同 JSON 重试成功（SHA-256 `abe435cebd3973727e54f8fe62be058ed9f52a22866baed07980458fc8e10bc5`）。没有修改全局 harness/Git 配置。实际回执为 `s6-submit-retry-stdout.json`，两次启动与完整输出在 QA 目录中。

实际 Claude Code 版本为 2.1.303，现有账号状态可用；尚未运行十任务中的任何一个，S6 不计入十任务验收。最后核对 QA main 保持初始提交、工作树干净、没有 B0 tag。下一步必须由真实 App 审阅并批准，不能以 CLI 或直接 HTTP 代替；待用户解锁和依赖选择答复后继续。

### FR-49 执行计划（用户已确认依赖）

用户明确同意 tokio-tungstenite 0.29，与 axum 匹配；先继续自动化，稍后解锁做实机。复用现有 tokio/futures-util 版本线，不改全局配置、不另开 App。

- [x] daemon 提供仅本机 App 可订阅的 WebSocket，全量初始状态与连续水位，后续变化推送；空闲连接不重复生成 Git 证明。
- [x] 原生管理唯一连接，断线后重新发现 daemon 并重读全量状态，固定窗口合并快层更新；关闭/切项目取消。
- [x] App 去掉 2 秒快层轮询，按 token/订阅/水位拒绝旧状态，写回执后不回滚；节点与视口不因快层更新移动。
- [x] 串行运行 daemon 协议、原生生命周期、前端竞态测试与类型检查；精确提交并构建候选，实机保持待验。


### FR-49 自动化回顾

本段与实现一同提交；提交前状态为未提交，候选构建与实际提交号在后续记录确认。单连接推送、断线重连、固定 1 秒合并、快层不移动节点均已实现。恢复成功会换订阅，隔离旧连接不带序号的恢复消息；人工写入使用实际回执序号保护状态水位，等待最新状态期间不重复提交。

- 工作区全量 370 项通过：`/private/tmp/towermd-fr49-workspace-first.log`。随后仅关闭 futures-util 不需要的默认宏特性；最终依赖下 WebSocket 端到端 2 项再次通过：`/private/tmp/towermd-fr49-ws-final.log`。
- 原生 19 项通过：`/private/tmp/towermd-fr49-native-second.log`；包括真实 WebSocket、固定窗口、重连、取消及 HTTP 缓存替换保护。
- 前端 106 项通过：`/private/tmp/towermd-fr49-ui-final.log`；类型检查零错误/警告：`/private/tmp/towermd-fr49-ui-check-final.log`。
- 恢复迟到消息测试先真实失败，再修复通过；失败证据保留在 `towermd-fr49-recovery-red.log`。此前 prompt 测试夹具未更新实际回执后的状态水位，修正夹具，没有放松产品门禁。
- 用户本轮明确先完成自动化，稍后解锁。未增加 App/daemon 实例，未执行或宣称通过实机验收；真实 B0、恢复、Claude Code/Codex 三工作树十任务和正式发布仍待完成。
- 提交前盘点只有主仓库 worktree 和 main 分支，无 stash。用户已有 `.towermd/`、`AGENTS.md`、`CLAUDE.md`、`docs/superpowers/`、`docs/tower/` 未跟踪内容不纳入本次提交或候选。


### FR-49 冻结候选记录

实现提交 `9d84417c22c79c33dba427db86ae971dc581c2b4`，候选 `.release/fr49-9d84417-20261021/Tower.md.app` 已构建，未启动。源树 `09ecdc5b5f31f043b2db2f620be9b2811f562959`；构建前后受控源码无变化。目录包含 source.tar/source.json、manifest.json、build.log 与 evidence/results.json 和六份实际验证日志。六个 SKILL.md 加 README 与源码归档逐字一致。

App SHA-256：`13abd4ab45009e0dfd9be6051a39dc03bdf02ae6f35ab6561ca12219d9b183c3`；CLI SHA-256：`c56e3059b5509a814e7438f08508600afe4e8b49fa1c57fc7c15c664949ef777`。仅 macOS Apple Silicon，链接器 ad-hoc 标记，无发行者签名、公证或正式发布 tag。构建有 Tauri STATIC_VCRUNTIME 弃用警告，不影响本次 macOS 构建。

本轮没有新开 App/daemon；仍运行上一份 4736d96 候选各一实例，它不包含 FR-49。用户本轮选择稍后解锁再实机；因此没有把此次自动化与打包当作整体验收。下一步在用户方便时换成此新候选，沿原 QA 三工作树完成实际 B0、问答/控制/恢复及双 harness 十任务，再决定正式发布。无需再次确认已批准的 WebSocket 依赖。


### 下一步：真实验收接续（2026-10-21）

沿用 9d84417 候选及原 QA 仓库，不重做已经通过的整批回归、不扩大 M2/M3。当前系统仍报告锁屏，已询问是否方便解锁；等待期间只做独立可验证工作。

- [x] 核对新候选、原 QA 源码与三个工作树、验收驱动的真实契约，保留原记录；补上实际 daemon 重启的测试缺口。
- [ ] 桌面可用后退出旧 App，停止旧 daemon，再启动唯一新候选；实际 App 审阅并批准原 S6 草稿，核对 Git B0 与批准回执。
- [ ] 真实 Claude Code/Codex 按十任务停点推进，逐项保存主干测试、门禁拒绝与合并认证；再实测问答/控制/恢复。锁屏期间不代造 human 事件。
- [ ] 按实际结果更新证据和提交；只有完整主路径验收通过才正式发布。


本轮结果：候选 manifest 所有文件及 QA 冻结技能均匹配；QA 三工作树干净，实际 main `b0249c66ea0030fed7fd513fd9aa2d5521219da1` 再次通过 verify-main。原件在 `/private/tmp/towermd-m1-qa-20261021/pre-b0-fr49-verify-main.log` 及该目录 `main-checks/<main>/verification.json`；修正 QA README 仍写“未执行 setup”的过时状态。

补充测试 `real_daemon_restart_with_active_subscription_preserves_durable_state`：真实 CLI 暂停→保持活动 WS、客户端不预读关闭帧→daemon stop 在 10 秒内完成→旧 socket 结束→新实例拒绝旧凭据→重新打开项目的首帧保留实际暂停/序号/完整投影→恢复操作实时到达。WebSocket 整组 3 项通过，日志 `/private/tmp/towermd-fr49-live-restart.log`。只改测试，不改产品实现；9d84417 候选继续有效，无需重复构建。提交前本轮测试和记录均为未提交，实际提交号随补充证据归档。

本轮原生窗口检查返回 `cgWindowNotFound`，系统 `CGSSessionScreenIsLocked=Yes`，已异步请求解锁，未收到可操作确认。没有运行或代造 B0／human 事件；三工作树十任务仍为 0/10。测试临时 daemon 已回收，最后只有原 4736d96 App 与 daemon 各一实例，syspolicyd 抽样 0%。


### 暂缓实机期间的独立工作（2026-10-21）

用户明确暂不解锁，继续独立工作；不再探测或请求打开桌面，不启动候选 App，不用自动化替代真实人工批准。

- [x] 接通原生真实 daemon 专项：实际打包 CLI ensure/open、原生订阅、真实暂停事件、daemon 重启后的原生自动重连与 HTTP 缓存更新；取消后不再拉起 daemon。隔离子进程 runtime，默认测试不依赖打包产物。
- [x] 只读核对 M0/M1 主干：修复已证实的主干回执序号丢失，记录仍待补齐的节点内关注摘要，不扩展 M2/M3。
- [x] 根 Agent 单队列验证、精确提交与记录证据；本轮含一行产品修复，已构建新候选，保留实机待验状态。


本轮实际修复：`MainResolution` 已传出 seq，但 App 回调忽略了参数，主干认领／撤销后旧状态可能回退。先补 WebSocket 与 HTTP 两个集成测试并真实失败，再仅转发 seq 给既有水位保护；前端整组 108 项通过，类型检查零错误/警告。日志 `/private/tmp/towermd-main-resolution-seq-red.log`、`towermd-main-resolution-seq-green.log`、`towermd-main-resolution-seq-check.log`。

原生专项 `project_stream::tests::real_daemon::packaged_daemon_reconnects_and_invalidates_http_cache` 显式运行 1 项通过，默认 ignored。日志 `/private/tmp/towermd-native-real-restart-first.log`；测试实际使用随包 sidecar（SHA-256 与 9d84417 候选 CLI 一致），只有原生重连循环负责启动新 daemon，第一次 HTTP State 请求必须成功，取消后跨过重连周期仍不拉起 daemon。子进程隔离环境，测试临时 daemon 已回收；未启动 App。

README 中“S8 批准、任务和恢复尚未接通”等过时描述已按实际源码改正，加入真实最短使用流程和专项测试入口。尚未改样式或增加常驻面板。审查另外发现 spec §15.3 的项目节点内最多三条关注摘要尚未显示：当前只有模块角标和按需完整清单。此项保留未完成，须沿纯画布原则处理项目节点呈现并实机验收，不能把已有面板当作摘要已实现。

本批变更验证完成，提交前均为未提交；以下继续精确提交并构建包含一行产品修复的候选。实机批准、真实十任务仍未开始，不再次要求用户解锁。


### 本轮候选与提交证据

实现／测试／说明提交 `e721cc3d24a19b77488e24c8f5b7f4832e889219`；候选 `.release/main-resolution-e721cc3-20261021/Tower.md.app` 已构建但未启动。source.tar/source.json、manifest.json、build.log 与四份实际测试原日志存于同目录；六个技能和 README 与源码归档逐字一致。构建前后受控源码无变化。

App SHA-256：`82d149e0b45105c00b7304e18ea8119918aae669262b1c646e2358ea10198a6e`；CLI SHA-256：`c56e3059b5509a814e7438f08508600afe4e8b49fa1c57fc7c15c664949ef777`（CLI 生产代码未变，与前候选相同）。Apple Silicon 候选，无发行者签名、公证或正式发布 tag。产品仓库仅 main／单 worktree／无 stash；用户未跟踪目录均保留未纳入提交。

最后资源检查无测试、编译或临时 daemon 残留；保留原旧候选 App 和 daemon 各一实例，syspolicyd 抽样 0%。下一步仍需补项目节点关注摘要及真实 GUI／双 harness 十任务；用户暂不解锁，本轮没有请求其再次解锁或执行 GUI。


### 用户已解锁：候选 e721cc3 实机验收

- [x] 关闭旧候选与 daemon，仅启动 e721cc3；实际读取首屏、目录选择、原 QA 草稿。
- [x] 在真实 App 审阅、重新验证并批准 B0，核对 Git 标签与 canonical 回执。
- [ ] 按原 QA 队列运行双 harness 十任务，实测问答／控制／恢复，保存实际原件；遇产品问题先修复和验证。
- [ ] 收集可复核结果与剩余问题；不把准备、启动或示例计作真实验收。


实际 B0 已通过 App 点击出版：草稿 `d-0ba88da00831cfcb33d5a544d29da50af11dab8c13736dda20375b7cc021d26f`，三模块原命令已逐一确认并真实重验证通过；基线提交 `6a3cb91453f5a76177320c2e4abb3e74d2b17252`、首快照 `a3258ae01a1dc2fb4dbe1c04d32498dcd42c2a69`、tag `towermd/B0`，实际批准 eid `01a15c5f-ef7d-7268-bd7a-536ab465b6a1`。QA bind 已核 canonical 内容与实际 origin tag/main，原件在 QA `records/binding.json`。首屏关闭详情后只有画布，选择目录/读取草稿/模块详情/验证/批准回执已实际操作；Go To 目录选择改用已解析的路径历史项双击后成功。

真实 Claude Code 2.1.303 已完成 T01 代码及本地两项测试，提交 `eb87ccc50885d44a6d9141f1e569525df24bf9cc`，但 check 拒绝已批准命令中的 `id` 字段，未执行 finish/trailers、未推 main、未计完成。保留 `/private/tmp/towermd-m1-qa-20261021/records/commands/1792551263684061000-T01-check-0/` 和 harness 原件。问题在产品批准文档与执行读取 DTO 不一致，禁止修改 B0 文件绕过；正在先补实际出版→finish 回归，再最小修复读取器。

另已通过 App 暂停项目（暂停 eid `01a15c62-c1f5-752d-acc4-e2a29e824034`）并明确点击恢复，实际界面显示“恢复确认已记录”和“项目允许领取新任务”。T01 没有 finish Evidence，因此该验证也没有把 running 当完成。GUI 观察与失败摘要保存在 QA `live-e721cc3-b0-and-first-task.json`，后续继续原仓库。

### 已批准命令兼容修复

- [x] 真实扫描→综合→原命令确认→复核→main writer 出版→finish 回归先重现 `unknown field id`，再修复批准文档读取类型。
- [x] 保持人工请求严格 DTO、目录/空命令/类型检查与 G0–G5 不变；核心 task_finish 14 项与 CLI finish 2 项实际通过。
- [x] 提交并构建更新候选，沿原 QA B0 与 T01 继续真实验收，不重写已批准文档。

通过日志：`/private/tmp/towermd-published-command-green-second.log`、`/private/tmp/towermd-published-command-cli.log`；真实 red 原件 `towermd-published-command-red-second.log`。本条提交前仍为未提交；真实十任务目前 0/10，不能据自动化声称整体验收通过。


候选更新：`9539a2f` 已提交并构建为 `.release/published-command-9539a2f-20261021/Tower.md.app`；source/manifest/build/evidence 均已归档，打包时受控源码干净。仅替换旧 App/daemon，各保持一实例。真实 App 打开原 QA 并保留 B0，默认纯画布已截图观察。T01 原代码仅 rebase 到已核实只改 state 的 main，新的 Claude 回合 check/finish/G5 实际通过，原 Evidence `E-01a15c6d-aac6-7107-ace2-33b6183d53fd`；App 已显示实际 npm 命令、两测试通过与“验证通过不等同已合并”。

T01 快进提交 `a616a133682ef0f5661e93f17749847d11e895ae` 已进入 QA origin/main，actual-main 测试通过，但认证发现第二个产品兼容问题：末尾五项 Towermd trailer 完整且唯一，附加的 `Co-Authored-By` 被 exact_trailers 整块全等比较拒绝。任务仍 verifying，真实完成数 0/10。按 FR-12 修复为严格核对 Tower 命名空间、兼容普通共同作者署名；保留原 main、失败与通过证据，不改写历史来绕过。

### 继续完成实机发现项与项目摘要

- [x] 补真实 Git 署名 trailer 回归并修复；正常与恢复共享同一证明器，仍拒绝缺失/重复/错值/未知 Tower 字段。
- [ ] 冷启动草稿加入现有 project region 节点，审批出版后显示前三条关注；旧基线不暗加结构，快层不移动坐标，右键入口继续可用。
- [ ] 串行验证并精确提交/打包；真实 App 继续认证 T01、问答和余下验收。

整批 Rust 工作区回归 376 项通过，日志 `/private/tmp/towermd-project-node-trailer-workspace.log`；新增共同作者真实 Git 回归先 red（`towermd-collaborator-trailer-red.log`）后 green。此时前端/实机项目节点仍待验，两个独立改动将精确分别提交。

项目节点自动化完成：Rust 376 项、前端 111 项通过，类型检查零错误/警告；日志 `towermd-project-node-trailer-workspace.log`、`towermd-project-node-ui.log`、`towermd-project-node-check.log`。普通署名兼容已提交 `9e8d0b8`。项目节点代码单独提交后构建含两项的新候选；样式真实验收尚未完成，不声称已验收。旧 B0 不暗加 group，当前 QA 继续右键入口。


候选 `4e98074` 已构建并替换唯一 App/daemon，归档 `.release/project-attention-4e98074-20261021/` 含冻结源码/manifest/实际验证日志。真实 T01 不改写 main、沿用同一 G3 Evidence 已认证 merged；实际 App 模块任务显示“已合并”。合并提交仍 `a616a133682ef0f5661e93f17749847d11e895ae`，快照 `1975f4e9744931234c9a2af5b0d55a8ed2c6e187`，原件 `records/T01/certificate.json` 与 `live-4e98074-T01-merged.json`。当前真实验收 1/10，继续 T02 实际 Codex 问答。新项目节点实机样式仍待验，旧 B0 未暗改。

T02 实际 Codex 已按非推荐的 App 回答实现下划线 slug，回答及插话各有 CLI 送达原件；源码验证 3/3 通过，main 接入 `4c52b64906821b87a73662e5e44130623b789d73` 已 certified merged，App 实际显示已合并/运行已释放。QA `records/T02/certificate.json` 与 `case-assertions.json` 已核 canonical human 回答与 delivery。Codex 沙箱不能写共享 Git index，Root 只代做 commit/rebase/amend 并单独留痕，Codex 实际完成代码、测试、check/finish/G5，未放宽沙箱。

T03 第一回合真实 Claude 只自述完成，已核任务仍 running、没有 Evidence、无代码变化，负向原件 `records/T03/selfreport-negative-proof.json`。第二回合发现技能先 check 后 commit 与现实现都要求干净工作树不一致，导致两 harness 都先触发脏树拒绝。`904c646` 精确调整 finish 技能为 status→提交→check→finish，不修改任何门禁；原冻结技能和失败记录保留，更新技能另行冻结使用。当前真实认证 2/10。当前运行候选 `4e98074` 尚不包含此技能文字更新。

T03 后续真实实现、finish/G5 以及主干认证均通过，原件 `records/T03/certificate.json`，真实认证更新为 3/10。App 新扫描已实际显示项目卡片，并返回原已批准 B0；发现未批准项目卡片提示“选择查看项目态势”但实际只开草稿详情，现最小修复为仅批准态传 attention。前端 114 项通过，类型检查零错误/警告，日志 `/private/tmp/towermd-draft-project-ui.log`、`towermd-draft-project-check.log`。仅 UI 条件和测试变化，无坐标/样式变化；新候选实机复验待打包。

T04 Codex 已实际制造预定的 mean 空数组失败（3 pass/1 fail），Root 原样代 Git 提交并经真实 CLI finish 取得 failed Evidence；App 实际显示门禁失败、“真实模块验证未通过；进度未认证”。`records/T04/first-failure-proof.json` 保留原件，正在让同一 Codex 修复实现，不能把失败算完成。

T04 后续 Codex 只修实现、测试未改，实际 4/4 通过；Root 代 Git/真实 CLI 检查点完成新 finish 与合并。`records/T04/case-assertions.json` 已核旧 Evidence 仍 failed、旧告警已 resolved，新 Evidence merged；App 中旧关注项消失，任务显示已合并。T05 真实 Claude 实现并 finish passed 后，只 amend 提交说明加入真实 trailer；`records/T05/case-assertions.json` 已核新旧提交号不同、tree 完全相同、最终认证仍用原 Evidence。每项 actual-main 测试通过；当前认证 5/10，继续 T06 双检查点同 tree squash。


### 真实验收续跑与最后 M1 缺口（2026-10-21）

- [x] T06 两检查点同 tree squash，旧 G3、新 G5 与真实 main 认证；原件 `records/T06/case-assertions.json` 和 `certificate.json`。
- [ ] 完成 T07–T10 的快照/代码 rebase、G2 越界、双模块争领；仅三个 QA 工作树，串行 harness。
- [ ] 最小补齐 FR-96 CLI 转述临时回答，保持 process 署名、不可批准/形成决策/规则/触及慢层；针对耐久重放与权限边界验证。
- [ ] 增加发版双语 CHANGELOG 硬检查及轮转入口，不扩平台或 CI；最终发行前执行。
- [ ] 构建单一新候选，完成 B1/propose/needs_review、主干处置、App 确认重建及项目卡片实机，再冻结发布。

T06 真实认证 6/10：Evidence `E-01a15c97-0f91-72b9-9dcd-b64bbd454ed7`，同 tree squash 已核两提交变一提交，G5 已刷新；第一次推送后立即读取尚 verifying，后续真实认证完成，未把 Git 可达当完成。当前 T07 已串行启动。新增实现按原规格执行，无新增依赖。

发行日志门禁脚本已独立验证：Node 5/5 通过，当前仓库双语只读检查通过。仅准备正式发行的硬检查和显式轮转入口，不改变当前版本或日志；产物构建仍走既有 app:build。

发行日志门禁已精确提交 `1ac951a`。App 实机点击 T06 后显示“已合并／已释放”，与同一 squash 认证原件一致；仍只运行 4e98074 App/daemon 各一实例。

T07 真实 state-only rebase 已认证 merged，同一 Evidence `E-01a15c99-696f-7f1f-b4ca-3ae0a03d8fe9`，实际 snapshot 前后 main 只改 `docs/tower/state/`，变基后重新 G5；原件 `records/T07/case-assertions.json`、`certificate.json`。当前认证 7/10，T08 正实际实现，后续用 T09 代码改变 main 验证旧证据必须失效。

FR-96 最小实现验证完成：核心 question_model/projection 15 项、真实 CLI/daemon 整组 3 项、前端 115 项通过，svelte-check 零错误/警告。原日志 `/private/tmp/towermd-fr96-{core,cli,ui,check}.log`。`answer` 固定 process 署名与 human 转述来源，普通临时回答只解原运行提问；跨运行/慢层/保存决策规则入口均拒绝，原请求重试、重启和序列化重放保持一致。UI/技能/README 明确仅本次转述；尚未进入当前运行的 4e98074 候选，等待新构建。

FR-96 已精确提交 `e9e7c19`，并构建 `.release/m1-acceptance-e9e7c19-20261021/Tower.md.app`。源码归档/manifest/构建日志/专项验证日志完整，包内技能与 source.tar 逐字一致，构建前后受控源码干净。App SHA-256 `d8c9adcb79dceeeed4c7134b46b350db3b7579488f5301411726557389f2d00c`，CLI SHA-256 `96541aafa137dce3723f2e970894f02700cd2fc7a56174d88644d734a898d360`。仅 Apple Silicon、无发行者签名或公证，尚未正式发布/尚未启动。

T08 实际 Codex 实现并经真实 verifier passed，等待代码 main 改变再验证旧证据拒绝。T10 实际 Codex 用原固定请求争领失败，`records/T10/conflict-proof.json` 从冻结原始 journal 重放已核 math 无部分租约、text 仍由 T08 持有、未产生 T10 run。当前 T09 正串行执行合法 quote 检查点，随后做 G2 负向。

T09 G2 负向已通过：保护路径提交 `a4fcd2e85d9eb6fa92419d15d2c5387a03074a1a` 保留于 `refs/qa/T09-protected-negative`，check 拒绝；finish 是先返回 queued，后台准备期返回 interrupted/G2、无通过证据（不是同步报错/测试 failed）。仅回退本 QA 分支到保存的合法代码检查点，再真实 finish/G5/主干认证；原件 `records/T09/g2-negative-proof.json` 与 `certificate.json`。

T08 在 T09 代码真正进入 main 后 rebase，旧 G5 明确拒绝“main 的代码、配置或其他路径已推进，必须重新 finish”；新 Evidence `E-01a15ca5-306b-76c0-9463-84ebbf2d5976` 才通过并 merged，原件 `records/T08/old-evidence-rejected.json` 与 `certificate.json`。当前认证 9/10，实际 App 同时显示 T09 已合并及原越界捕获提案仍待批准。T10 正沿原 request-id 重试领取并实现，不更换请求来掩盖首次失败。

原十任务已 10/10 真实 certified merged，全部运行已释放。汇总原件 `records/ten-tasks-summary.json` 包含每项证书 SHA-256、Evidence 与实际 admission commit；T10 同一 request-id 从占用拒绝到成功领取，真实验证覆盖 math/text 两个命令。App 旧候选实际显示十项全部已合并。

已关闭 4e98074 App/daemon，唯一启动 e9e7c19 App（PID65732）及 daemon（PID65771），重新打开原 QA，截图确认默认纯画布。正在用两个额外 Task ID 覆盖 B1 真实提案/批准/needs_review、项目卡片 top3、主干处置与 Hub 丢失/恢复，不重跑原十项。B1old 当前由真实 Claude 综合固定 main，等待 App 审阅。

真实 B1 综合及 App 三模块命令审阅、重新验证均通过；新提案 `P-01a15cb0-65eb-717a-af1e-360b7c1a20b3` 绑定精确审阅草稿。已在 App 驳回原未审阅提案，尚未批准 B1。实机发现成功回执后提案列表仍显示待审阅，现最小修复为仅在可信回执后更新本地列表及选中项，保留未知结果阻塞和原请求重试。前端 117 项通过、类型检查零错误/警告，日志 `/private/tmp/towermd-proposal-receipt-{ui,check}.log`；提交后替换单一候选继续实际批准和恢复验收。

`bc3c517` 候选实际完成 B1：App 批准 eid `01a15cb8-8998-760d-85f8-e6bd59c5a632`，tag `towermd/B1` 指向 `ecaa4055fda6fb129e9496bedc185651306b5b61`；提案列表立即已批准，切换回来无重复按钮。旧 Task needs_review、旧 check 拒绝，释放旧 Run 后新 ID 在 B1 领取成功，十项已合并状态保留。原件 `records/B1old/b1-publish-proof.json`。实际项目卡四项只显前三，展开能看第四；逐项 App 回答后下一项补位，截图及 Git canvas 比对均确认坐标不动，原件 `records/B1new/project-attention-proof.json`。

真实 main 越界 QA 提交 `610a812b0ef5b99b9ae61f7f4e5df7557c40f942` 已触发禁止，check 拒绝；App 捕获提案 `P-576f6217a28dec6bf91ab661423f062ad40d6addada6e12df0d0e17dd728fcac` 后禁止保留。发现回执隐藏后续撤销入口，最小修复为捕获后保留认领/撤销、禁止重复捕获；未知请求仍阻塞。前端 119 项、类型检查零错误/警告通过，日志 `/private/tmp/towermd-captured-resolution-{ui,check}.log`。将构建候选继续实际同一目标的捕获→撤销及 Hub 恢复；目前尚未完成这两项。
