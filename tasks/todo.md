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

### M1 主路径验收闭环与 0.1.1 预览发行

- [x] `ba3a2f6` 单一候选实际验证捕获后撤销，产生反向提交 `1ec5954f1293b41bbc9741685bf8639ec4e6cda1`，canvas 逐字恢复；独立认领目标 `ca173f045bc21586e15e5aa024d24249708bfd39`，原禁止解除且未生成 B2。
- [x] 仅备份移走 QA Hub；CLI 请求后由真实 App 确认恢复，确认 eid `01a15cc2-4523-7684-8f6c-68c0edfb0b74`，恢复快照 `d4dc50292b48418cbf962e6f387fdb0cc6624dd3`。seq/epoch 跨过 2^32；原 Evidence、回答、插话、基线和处置表逐值保留，旧运行失效且 check 拒绝。
- [x] B1new 恢复后由真实 Claude 在新 Run `R-01a15cc3-441c-771c-9d24-fdd94c4df98c` 完成输入不变性测试；实际 Evidence `E-01a15cc4-f086-7aa8-be14-f6fbe253ec06` 已认证合并，接入提交 `0bd4ee727c55bc11e82dc5727a40b6819e06dd39`，App 实际显示已合并。原十项加额外一项均完成，详见受控验收报告。
- [x] 最终整组自动回归：工作区 380、前端 119、原生 20（含真实随包 daemon 重启／WS 重连）、发行脚本 5 项通过；类型检查零错误/警告。保留首次旧命令清单失败与通过原日志。
- [x] 本机原件与两份经 verify 的 Git bundle 已归档 `.release/m1-live-originals-20261021/`；产品仓库只有 main／单 worktree／无 stash，远端无新增分歧，用户未跟踪目录保留且排除。项目版本统一 0.1.1，双语日志经硬门禁轮转，第三方依赖未变。
- [x] 冻结发行源码并构建产物，包内技能与源码逐字一致；CLI/Info.plist 均为 0.1.1，ZIP 内各文件与 manifest 的哈希一致。
- [x] 最终包实机复验后推 main/tag、发布 Apple Silicon 预览包及摘要；只保留单一 App/daemon。

最终 Rust 整组首跑发现 CLI 帮助测试旧白名单缺少已实现 `answer`，仅同步此测试的精确命令清单；FR-96 真实命令与权限边界专项此前通过。正在重跑完整工作区，原失败日志 `/private/tmp/towermd-m1-final-workspace.log` 保留。本次明确不把未测 Intel、NFR-03 和 Note.md M0 写成完成，不以此扩充 M2/M3 功能。

重跑已通过，测试清单修复独立提交 `9d56216`。最终通过日志 `/private/tmp/towermd-m1-final-workspace-green.log`、`towermd-captured-resolution-{ui,check}.log`、`towermd-v0.1.1-native.log`；发行日志轮转原件 `towermd-v0.1.1-changelog-rotation.log`。最终 0.1.1 CLI 已准备，实际原生重启测试也使用该随包版本。下一步仅冻结构建、最终包实机复验和上传；尚未将构建准备写成已发布。

`f10f932` 首个 0.1.1 包已构建，包内 CLI/Info.plist 均为 0.1.1，六个技能及 README 与源码归档逐字一致。实际打开示例发现两句“真实批准/合并尚未接通”的旧说明，现只更正文案及既有文本断言；前端 119 项、类型检查零错误/警告再次通过（`towermd-v0.1.1-final-{ui,check}.log`）。旧包保留为候选原件，关闭后重新冻结构建，不上传该过时说明的版本。

### 0.1.1 预览发布回执

已发布 [v0.1.1](https://github.com/wizlijun/tower.md/releases/tag/v0.1.1)，冻结 tag 源码为 `1726f8fd5aa56202806e40c7a1f685cf7c4da51b`。ZIP SHA-256 `0524dbdea8eee7bf52111dea4541fe792ff457cffce8395549a192a6ada2d661`；App 二进制 `8142c6a18e47dfda37b59283921accd4fa611aa762506dc3bdb0bb6de4e002ba`；CLI `8fb6f21b5f9a8187e13df216a91a2a6916cfeb29725a4985b59e39ccec9d8047`。GitHub 四个附件（App ZIP、冻结源码 tar、release-evidence.json、SHA256SUMS）均 uploaded，服务端摘要与本地逐项一致。

最终包实际启动并核对更正后的示例说明、原真实项目、十一项 merged、旧 Task 需复核标记、四项问答及三项插话；关闭详情后仅画布。当前运行 `.release/v0.1.1/Tower.md.app`，App 与 daemon 各一实例，旧候选／测试／编译／本轮 harness 均已结束。只提供 Apple Silicon 无发行者签名预览（编译器 ad-hoc 签名，TeamIdentifier 未设置）；Intel、NFR-03 和指定 Note.md M0 的未验收范围保持明确，不声称全部 spec 完成。本条是发布后的记录，不改变 v0.1.1 冻结产物。


## 计划：规格拆为核心规格与开发套件规格（2026-10-21，用户已确认）

依据：vault《Tower.md 的核心只为人脑的共同局限做抽象》第 10 节的 4 项已定决策（接受拆分；名称暂定第一版；首次打开不问场景，onboarding 以后做；不做用法统计）。只改文档，不改代码。

- [x] 新建 `docs/specs/2026-10-21-spec-towermd-core.md`：10 条人脑局限、3 条放置检验、7 个名词与 6 个动作、6 项能力、接入接口、扩展机制、注意力与容器（并入 vault 注意力需求 FR-114–180 的核心部分）、hub 与快照等基础设施、命名表第一版。
- [x] 新建 `docs/specs/2026-10-21-spec-towermd-development-kit.md`：git 分支与工作树、模块路径与验证命令、G2/G3/G5、冷启动扫描源码、一句话生长的代码骨架、技能包、harness 适配器、无头双环与合并队列、Arca。
- [x] FR 编号保持不变，只换所在文档；新增条目从 FR-181 起。
- [x] 旧路径 `2026-10-16-spec-…-mvp.md` 改为索引页（FR → 新文档对照），v0.5 全文移入 `docs/archive/2026-10-21-pre-core-split/`。
- [x] 去掉偏差：项目画像/项目类型、pin、对用户可见的六层、界面行话、信号级别旧名、用法统计。
- [x] 核对：v0.5 的 113 条与注意力需求的 67 条 FR 全部有去处；链接有效；名词一致。


## 计划：命令行优先（2026-10-21，设计已写，实现前待确认）

依据：`docs/specs/2026-10-21-spec-towermd-cli.md`。用户已定：人的动作由命令行发起、经系统在场验证后签名；人读输出跟随系统语言、缺省英文；开发套件命令放进 `towermd dev`；不兼容旧命令，可以重构。

- [x] C0 契约：JSON 信封与 `delivered`、错误码与退出码、全部写操作带请求 ID、语言、`schema`/`help`、按规格第 5 节重构命令（不保留旧名）；技能、`AGENTS.md` 模板、使用说明和测试同步改写。（见下方 B 部分）
- [ ] C1 看：不带参数的概览、`needs`、`since`、`show`、`ls`、`history`、`watch`、`open`、`doctor`、`whoami`。
- [ ] C2 接入接口：`report`、`signal`、`reply`、`frame`。
- [ ] C3 人在场：`human setup/status/revoke` 与全部人的动作（前提：签名构建）。
- [x] C4 计划与扩展：`idea`、`promote`、`follow-up`、`frame new`、`ext`、`template`。（1dc5e0a：`idea add`、`ext ls/show`、`template ls` 在命令行；`promote`、跟进项、建容器、扩展安装是人的动作，只在 App；`snooze` 未定义于 FR-134，不做）
- [ ] 待确认后修改：仓库 `AGENTS.md` 不变量与 `docs/product-principles.md` 中"批准只能从 App 发出"的表述。

## 计划：核心与开发套件拆分 A 部分（2026-10-21，已实现）

依据：`docs/superpowers/specs/2026-10-21-core-devkit-split-and-cli-c0-design.md`；计划：`docs/superpowers/plans/2026-10-21-a-core-devkit-split.md`（偏差 D1–D14 见计划首节）。

- [x] docs: plan core/devkit split and record design revisions（fbec973）
- [x] test: record v0.1.1 replay golden fixtures from real CLI flows（0571b66）
- [x] refactor(core): add Kit seam for projection groups（ce169f9）
- [x] refactor(core): move PathRule into path_guard（0e80d8c）
- [x] refactor(core): name self-reported answer source in question model（9200f38）
- [x] refactor(core): split project_state reducer into core and dev kit（fda07f8）
- [x] refactor(core): prove protected commits through kit assembly（a151d3d）
- [x] refactor: move development kit modules into towermd-devkit（e34347e）
- [x] refactor(cli): group binary services into cli/ and dev/（ce0ca2a）
- [x] refactor(core): generalize module leases into work leases（3e3d094）
- [x] refactor(core): rename question binding module to scope（9cce3bd）
- [x] refactor(init): let kits contribute skeleton and AGENTS rules（e090b0b）
- [x] test(core): guard core against development kit vocabulary（d766b5e）
- [x] T13 文档：AGENTS.md 结构约定与目录布局加入 `towermd-devkit`（本条提交）
- [ ] 检查 Note.md 的 `AGENTS.md` 是否需要同步：本次改的是 Tower.md 专属的结构约定，预期不需要，由人确认（本轮未改 `../note.md`）
- [x] 决定第 R8 项文案变化是否写入 CHANGELOG 未发布区：仓库还没有 CHANGELOG，交给 B 部分（CLI C0，负责 CHANGELOG）一并处理：已写入（B 部分 T13）
- [ ] 修复 `project_runtime.rs` 的 `lock_existing_hub`：与 03f15f4 修过的 `durable_outbox` 同一种继承文件描述符持锁问题，本轮未修

### 回顾

- 测试：workspace 381 → 400 passed、0 failed（新增 19：重放样本 4、Kit 7、路径 1、自述来源 1、工作租约 1、scope 格式 1、init 2、边界 2），src-tauri 19 passed + 1 ignored 不变；与计划预期一致。`cargo tree -p towermd-core` 不含 `towermd-devkit`。
- v0.1.1 四份真实流程样本在每一步都逐字重放一致；磁盘格式与事件名、字段未变。样本是从快照归档加日志尾部录制的（快照确认会压缩日志，水位线以下的心跳不在样本里）。
- 重放覆盖缺口（R1）：样本不含 `recovery.completed` 与 `lease.expired`，这两条路径只由现有单元测试间接覆盖。
- 语义细节（R7）：控制插话的收件人运行改按 `binding.id` 查找，原来按 `runs` 映射的键查找；投影中两者一致。
- 用户可见文案变化（R8）：“项目暂停禁止新模块租约”改称工作租约（英、中两条）；“模块租约只接受非空 module:<id> 名称”改为“工作租约只接受非空 <kind>:<id> 名称”；“模块租约 TTL”改为“工作租约 TTL”；actor 格式说明 `<harness>/<version>` 改为 `<producer>/<version>`；自述来源校验“harness 名称/版本”改为“自述来源名称/版本”。
- 公开 API 变宽（R6）：`repo::git_timeout_env_output`、`repo::git_timeout_env_input_output` 与 `path_guard::PathRule` 改为 `pub`，供 devkit 使用。
- 有意保留：问题模型的线上标签 `module_decision`（`RememberAs::ModuleDecision`）与 `question_model.rs` 中仍写“模块”的几条中文提示未改，留给 C 部分；边界护栏只禁 `module:`，未命中它们，白名单仍只有 D11 的一行。核心模板 `templates/agents.md` 前 20 行仍列出 `intents/`、`arch/` 等慢层目录（D10、R10），护栏只扫 `.rs`。
- 遗留：types 注册表拆分（D7）、核心 Task 拆分（D4）另行立项。

## 计划：命令行 C0 契约 B 部分（2026-10-22）

依据：`docs/specs/2026-10-21-spec-towermd-cli.md`、`docs/superpowers/specs/2026-10-21-core-devkit-split-and-cli-c0-design.md`；计划：`docs/superpowers/plans/2026-10-22-b-cli-c0.md`（偏差 D1–D17、规格分歧 1–14、风险与待确认见计划末尾）。起点 `7641132`。

- [x] T1 错误码与信封（cee6e3f，workspace 405）
- [x] T2 输出语言（2788555，410）
- [x] T3 每次调用一个信封与 C0 退出码（dcc02dc，417）
- [x] T4 守护进程错误体带 `code`（c0826dd，419）
- [x] T5 扫描、快照与 hub 重建带请求 ID（618a3f1，421）
- [x] T6 `show`、`pause all`/`resume all`、标准输入与 `--project`（5c85582，424）
- [x] T7 `answer --relayed`，问题等待时退出码 5（b6cf30b，426）
- [x] T8 开发套件命令收进 `towermd dev`（98fc221，429）
- [x] T9 每个命令的 JSON Schema 与输出校验（6f36010，431）
- [x] T10 帮助本地化、示例与退出码表（aade890，434）
- [x] T11 App 按信封读取 `daemon ensure`（792e805，workspace 434；src-tauri 列出 21：20 通过、1 忽略）
- [x] T12 技能、`AGENTS.md` 模板与 App 说明改用新命令（eeb7fa0，435）
- [x] T13 CHANGELOG 与待办（本条提交）
- [ ] T14 README 与使用说明
- [ ] 真机验证：打包 App 能拉起并连上 api 7 的守护进程（T11）；发布前由人做
- [ ] 人裁决：计划“风险与待确认”中的 Q1–Q8
  - Q1 规格 3.5 的 `--json <文件|->` 输入改为位置参数（D1），需修订规格
  - Q2 新错误码 `rejected`（退出码 6）需补进规格错误码表（D3）
  - Q3 `completion` 需新依赖 `clap_complete`，已推迟（D10）
  - Q4 `hub rebuild` 仍为“CLI 请求、App 确认”，规格写“一步完成”
  - Q5 不带参数的 `towermd`（FR-198）在 C0 报 `usage`，留给 C1（D11）
  - Q6 原则 8（与 Note.md CLI 同构）与 `dev` 分组的关系
  - Q7 核心、开发套件与 CLI 规格里仍有旧命令名，请人统一修订（见下方旧名检查）
  - Q8 离线排队与 `hub_offline` 在 C0 没有代码路径

### 回顾

- 测试：workspace 400 → 435 passed、0 failed；src-tauri 19 passed + 1 ignored → 20 passed + 1 ignored。计划预期 432，T6、T8、T10 各比计划多 1 个测试。前端 119 个测试通过，`svelte-check` 零错误零警告。`cargo tree -p towermd-core` 不含 `towermd-devkit`。
- 与计划的偏差：
  - `Ask`/`Answer` 正文位置参数的 id 是 `body`，命令写法不变。
  - 新增 `dev draft show`；`baseline schema` 改为 `towermd schema dev.draft.submit`（D4、D13）。
  - `usage`、`needs_human` 错误不带 `retry`。
  - 守护进程错误体缺 `code` 或不是 JSON 时，客户端按 HTTP 状态回退（D3）。
  - 项目暂停导致的租约拒绝归为 `conflict`，租约冲突或过期归为 `lease_busy`（R5 已按来源区分）。
  - 没有可用基线时，`task add`/`task claim` 报 `gate_failed`。
  - `ask`/`wait` 在 `pending` 或 `deferred` 时退出码为 5（D6）；`answer` 必须带 `--relayed`；`pause all`/`resume all` 为位置参数（D16）；`api_version` 升为 7（D12）。
  - T12：`--text` 模式下 `delivered[]` 写到 stdout 而不是 stderr，所以 `towermd-finish` 技能第 5 步只改了命令名，保留“不要把整个 stdout 拼进提交说明”的提醒，没有改成 `--text dev trailers`。另外顺带改了计划表外的几处旧说法：`towermd-ask`、`towermd-claim` 里“消息出现在输出最前面”改为“在信封的 `delivered[]` 里”；`towermd-propose` 的 `baseline submit` 改为 `dev draft submit`；`towermd-finish` 第 6 步的裸 `finish`/`trailers` 改为 `dev finish`/`dev trailers`；App 综合草稿面板说明（`src/App.svelte`）及其测试改为 `dev draft submit`。
  - T13：CHANGELOG 把破坏性变更拆成六条放在最前（命令改名对照、重启守护进程、手工更新 `.towermd/AGENTS.md`、单一信封、退出码、`ask`/`wait` 退出码 5），A 部分 R8 的六处文案变化逐条列出。
  - T14：用户已在 `6a7dbc4` 提交 README 改写与 `docs/usage.md`，T14 只在已提交文件上改命令名，提交标题相应改为 `docs: use C0 command names in README and usage guide`。
- 旧命令名检查（排除 `docs/archive`、`docs/qa`、`docs/superpowers`、`tasks`、两份 CHANGELOG、`agents-v0.1.1.md` 原件）剩余：
  - `docs/specs/2026-10-21-spec-towermd-cli.md`、`-core.md`、`-development-kit.md`：人定内容，留给人改（Q7）。CLI 规格第 386、388 行本身就是新旧对照表。
  - `crates/towermd/tests/envelope_cli.rs` 的 `v0_1_1_command_names_are_usage_errors`：故意用旧名，断言它们报 `usage`。
  - `crates/towermd-devkit/tests/init.rs`：把 v0.1.1 原件替换成新名后再比较，故意出现旧名。
  - `docs/usage.md`：T14 处理。
- 待做与已知问题：
  - App 真机验证待做（发布前由人做）。
  - src-tauri 的 clippy 有 1 条已有 warning，本计划未引入也未修。
  - `cargo fmt --check` 早已不通过，本计划按约定不跑 rustfmt。
  - `resume all` 在没有待解除的暂停时报 `internal`，应为更具体的错误码。
  - HTTP 415 被归为 `internal`。
  - 守护进程 `stop()` 的 `wait_lock` 超时报 `internal`。
  - 扫描忙时，新请求 ID 会拿到已有扫描任务，以后用同一 ID 重试会启动新扫描；被中断的扫描用原 ID 重试返回 `interrupted`。两者都与计划 D7/R6 的描述不一致。
  - 重建确认事件的 eid 取自调用方的 `request_id`，可能冲突（可疑，未完全核实）。
  - claim 路径把所有 `RuntimeError::Invalid` 都映射为 `conflict`。

### 代码审查修复（2026-10-22，起点 bf576a7）

- `deferred` 仍是退出码 5，但不再给 `towermd wait` 的 `next`，也不标 `retry.safe`；hint 说明结束回合、回答从后续调用的 `delivered` 送达。`pending` 不变。`ErrorCode::transient()` 去掉 `deferred`。
- `classify_wait` 只把 `answered`（以及 `--parked` 时的 `pending`/`deferred`）当成功；其他或缺失的状态报 `internal`，提示可能是版本不一致。
- 重试规则：只有瞬时码（`pending`、`lease_busy`、`daemon_unavailable`、`hub_offline`）标 `safe:true`；确定性失败只回显 `request_id` 并标 `safe:false`；文本模式只在 `safe` 为真时打印“可安全重试”。Schema 已允许 `safe:false`，未改。
- `Failure` 新增 `lasting` 标记：守护进程 `api_version` 不一致时报 `daemon_unavailable` 但不给安全重试，`next` 为 `towermd daemon stop`。
- 输入路径是目录等读取错误报 `usage`；`schema`、`daemon`、`up` 不做 `--project` 检查（`Command::repo_free()`，`repo_bound` 复用它）。
- `.towermd/AGENTS.md` 模板改为 `towermd --text dev trailers <task-id>`，守卫测试替换表同步。
- 退出码表、`skills/README.md`、`towermd-ask` 技能与两份 CHANGELOG 统一为：有 `next` 就按 `next` 等，没有（`deferred`）就结束回合，不要忙等。

### 0.2.0 预览发行准备

范围：A 计划（核心与开发套件拆分）与 B 计划（命令行 C0 契约）。起点 `a15edb8`，main `a7feeaa` 是其祖先；单一 worktree、无 stash、无未提交或未跟踪文件，本地分支只有 main 与本分支。

- [x] 版本统一为 0.2.0：workspace `Cargo.toml`、`src-tauri/Cargo.toml`、`package.json`、`src-tauri/tauri.conf.json`；两份 lock 只改本仓库 crate（towermd、towermd-core、towermd-devkit、towermd-app）的版本行，第三方依赖未变。`docs/usage.md` 的下载链接指向 0.2.0 发行页；README 没有指向当前版本的下载链接，未改。v0.1.1 的固定样本、测试注释和历史记录不改。
- [x] 双语日志经 `pnpm release:prepare --version 0.2.0 --date 2026-10-22` 门禁后 `--write` 轮转；版本节首条补一句范围说明。轮转后未发布区为空，`pnpm release:check` 按设计失败，不补写条目。
- [x] 回归（`a15edb8`，日志 `target/release-0.2.0/logs/`）：Rust 工作区 445 通过、0 失败；src-tauri 21 通过（含默认忽略的真实随包 daemon 重连 1 项，用 `--include-ignored` 并以本提交代码重建的 sidecar 运行）；前端 119 通过；`svelte-check` 零错误零警告；发行脚本 5 通过；`cargo tree -p towermd-core` 中 `towermd-devkit` 出现 0 次。
- [x] 冻结构建（`b5ccb1d`）、核验包内版本／技能／哈希、隔离环境包级冒烟（CLI 全部通过；App 启动无崩溃，经对话框打开真实项目未实机验证）。
- [x] 合并 main、打 tag、发布（人已授权代办，见下方回执）。

范围限制：

- 只支持 macOS Apple Silicon，没有发行者签名和公证。
- 没有重做 harness 实机验收；本版只有自动回归和包级冒烟。
- Intel 运行、NFR-03 定量性能、指定 Note.md 仓库 M0 均未验收。
- C、D 两部分推迟，不在本版。
- 规格修订待人裁决（上文 Q1–Q8）。

### 0.2.0 预览发布回执（2026-10-22）

- 发行页：https://github.com/wizlijun/tower.md/releases/tag/v0.2.0（预发布）。
- main 由 `a7feeaa` 快进到 `b5ccb1d`；附注 tag `v0.2.0` 指向 `b5ccb1dd17c5606a5aaf46391b21ab0712f24557`，与 `release-evidence.json` 的 `source_commit` 一致。
- 资产 SHA-256（上传后重新下载，`shasum -c` 全部通过）：
  - `Tower.md-0.2.0-macos-arm64.zip`：`8410398bee5d5f2cda559215e2be35997597141abc78c822ca5a9cec53e1e40c`
  - `tower.md-v0.2.0-source.tar`：`017cfe456f2aa251102a676dd6941acd63b9bfd60e22ccf55d2bec9bab0f392f`
  - `release-evidence.json`：`c85f25ec21881baf09842942a1c18312d631ce4237dfdcf64fcbe759f67b9447`
  - `SHA256SUMS`：`66a8ff3af54e976f83ffe1bc4b24fc65636515a1dd5e5fee3957254e4f1270a7`
- 未验证：打包 App 经对话框打开真实项目并连上第 7 版守护进程；harness 实机验收；Intel；NFR-03；Note.md M0。
- 升级提示：先 `towermd daemon stop` 停掉 v0.1.1 守护进程；已初始化项目的 `.towermd/AGENTS.md` 需手动按新命令名更新。

## 计划：托付与信任账本（核心规格 v0.7，2026-10-22）

依据：核心规格 v0.7（82f6583）FR-184、FR-185、FR-201–211；设计：`docs/superpowers/specs/2026-10-22-delegation-trust-ledger-design.md`。用户要求"根据变更实现完成"，架构与产品取舍由 Claude 自定。分支 `feat/delegation-trust-ledger`，起点 `82f6583`。不新增依赖；不涉及 AGENTS.md 不变量的改动（人的动作只从 App 发出，不自动放行，自述不算完成，读模型由事件重算）。

- [x] D1（c820cea）核心类型：`Brief`、`Checkpoint`、`SourceKind`，`SelfReported` 加 `vendor`/`model`/`skills`，`config.yaml` 新键读取（`review_backlog`、`spot_check_min`、`track_record_window`、`delegation_levels`）
- [x] D2（3887d14）开发套件：`TaskDraft` 委托卡 5 栏与 `clarify`，任务时间与重开，`FinishRequest.claims`；重放样本仍逐字往返
- [x] D3（48d6145）核心 `Delegation` 投影组与 10 种新事件的归约；`Kit::delegations` 接缝与开发套件实现；开发套件响应 `delegation.finalized`、`acceptance.sent_back`
- [x] D4（fa22679）核心读模型：验收卡、证据 3 类、信任账本、配置变化、抽查、积压、汇报请求、级别建议
- [x] D5（9474c42，接口版本升到 8：4df4021）守护进程与命令行：App 专用的人的动作路由；`dev clarify`、`reply`、`task claim` 指纹参数、`dev finish --claims`；`delivered[]` 送达汇报请求；维护循环写 `report.overdue`；推送状态附读模型；Schema
- [x] D6（ee1f01e）Tauri 命令与前端 API
- [x] D7（50d44f9）App 界面：委托卡、澄清面板、验收卡、信任账本、关注清单新候选、积压行、"已声称、未确认"
- [x] D8（3facbde、2c0ec5f）技能、`AGENTS.md` 模板、使用说明（附录 A.6 两种用法）、双语 CHANGELOG 未发布区
- [ ] D9 全量回归与真实 App 走查；回顾（回归与守护进程实机走查已完成；App 窗口内的点击走查待人做，见回顾）

### 回顾（2026-10-22）

- 结果：v0.7 的 FR-184、FR-185、FR-201–211 已在 `feat/delegation-trust-ledger` 上实现。回归：Rust 工作区 480 通过（起点 445），src-tauri 21 通过，前端 142 通过（起点 119），svelte-check 0 错误 0 警告；v0.1.1 重放样本仍逐字往返，没有重录。
- 实机：用新构建的 sidecar 在 `/tmp/towermd-walk` 建了临时项目，从命令行和 App 路由走完"先澄清 → 定稿"和"领取 → 汇报请求 → 带 `--claims` 交付 → 合并 → 验收卡"两条路径。读模型给出逐条证据、"未证实"、整体检查和抽查。App 窗口已启动，但本机不给 osascript 辅助功能权限，没法点开文件夹选择框，界面截图走查待人做；D7 的组件另用 Chrome 无头模式和测试数据截图核对过。
- 偏差，都已写进各提交说明：
  - 任务阶段多了 `proposed`。
  - 开发套件的任务不能由人一步确认完成，必须经检查。
  - v0.7 之前的运行不进账本。
  - 委托级别只记录和建议，不限制 Agent。
  - 验收后的一句话存在人签名的确认事件里，读模型标 `set_by: human`。这与问题答复的决策同样只存在 hub 里，没有单独的决策文件。
  - 账本设置读工作树里的 `config.yaml`。
- 已知小问题：
  - 汇报请求 ID 按 `R-<运行>-<n>` 生成，开发套件的运行 ID 本身以 `R-` 开头，于是显示为 `R-R-…-1`。功能正确，只是不好看；要改须同时改设计与规格。
  - `--skills-version` 该填什么还没有统一说法。
- 教训：子代理之间并行时，`tasks/todo.md` 由主会话统一勾选，避免两个代理同时改同一文件。升级接口时先查 `require_current_api`：新增守护进程路由就要升接口版本，否则旧守护进程会被当作新的用。


## 计划：把三份规格实现完整（2026-10-23）

用户要求"继续下一阶段实现，直到 spec 完整实现"。差距分析见 `docs/superpowers/plans/2026-10-23-gap-core-spec.md`（核心 163 条：第 1 阶段 14 条全未完成，第 2 阶段 48 条未实现）和 `docs/superpowers/plans/2026-10-23-gap-devkit-cli-spec.md`（开发套件 M2 21 项、M3 37 项、CLI C1–C4 约 40 项未完成）。分支 `feat/spec-completion`，起点 795a5ce。按核心路线图顺序推进，每包单独测试、提交、勾选。

已自定的取舍（架构与产品取舍由 Claude 决定）：
- FR-188 的 `set_by` 换算：显示上 L0–L3 为 `human`；慢层保护口径仍是 L0–L2，不放宽。
- FR-121：模块验证命令即 `core.verify` 确定性检查；对象有确定性检查时只认检查，没有时才可由人在 App 确认完成。开发套件模块都有验证命令，因此与现行"开发套件任务必须经检查"一致。
- "自上次查看以来"的时间点是本机派生状态，丢失时退化为最近 7 天，不进仓库。
- 视觉词汇的图标集（核心 §16 待决 6）、豁免默认到期（7 天）、定型期阈值、技能包来源：按规格默认值或最保守选项自定，写进对应包的提交说明。
- hub-worker 沿用 Note.md mdrelay 的 wrangler ^3、@cloudflare/workers-types ^4、@cloudflare/vitest-pool-workers ^0.5。

人已裁定（2026-10-23）：
- CLI 在场签名的人的动作（CLI FR-195/196）：**保留 AGENTS.md 红线，不做 C3**；命令行规格写明原因并标为不做。
- 新依赖：**同意** `tokio-tungstenite`（hub 客户端）、`elkjs`（画布布局）、`clap_complete`（shell 补全）、`tauri-plugin-notification`（time_sensitive 系统通知）。
- M3 真实 harness 验收：**只做小规模冒烟**，每个适配器 1 个极小任务；完整 4 Agent × 20 任务验收留给人。

### 第 1 阶段（核心）
- [x] P1-A 命名第一版与 `set_by`（FR-188、NFR-14）— c47cab6、638ad67
- [x] P1-C 接入接口：信封、`report`/`signal`/`reply`、4 档级别、信号合并；CLI C2 `report`、`signal`（FR-114–119、NFR-07）— a72b08d、47e18ae、5e3fc89。遗留：信号消解与静音、passive 三源合卡 → P1-F；HTTP/文件入口 → P2-B；`core.silent` 与 `report.overdue` 并入信号 → P2-C；外发动词仍叫 `request_report`（§13 的 `request_update` 待命名收尾时再定）
- [x] P1-D 门禁声明与完成判定；`core.verify`（FR-122、124、56、33、121）— d0cb954、c937ad0、c29e6b5。已知限制：`--from` 自报，同名发送者可降级撤下阻止上报，真防护待签名与认证；上报只能阻止不能放行
- [x] P1-E 承诺弱化检测（FR-125）— 44f864a、579d0b8、01ea436。未覆盖：截止时间推后、范围缩小、扩展自定义阈值（→ P2-A）；人接受弱化并写理由记为决策待后续
- [x] P1-F 关注卡格式与"自上次查看以来"（FR-129、131、42、40、41）— 8b34ce5、5d17322、f33dd04，合并提交见 git log。卡片与排序在前端 `src/lib/attention.ts`；CLI C1 `needs`/概览需要时再下沉为共享视图。样式待实机验证
- [x] P1-B 首次打开只有空画布与一句话起头（FR-181、182）— 与 P2-G 同包：1c9b0b3、d136a21、94955d4，合并见 git log。FR-182 的 3 个澄清问题归 `towermd-seed` 技能；App 内初始化（FR-98）未做，未初始化文件夹提示先 `towermd init`

### 第 2 阶段（核心）
- [x] P2-C 汇报请求通用化与失联（FR-116、137）— 与 P2-D 同包：b2a0076、5810772、61bea60。`report.overdue` 不再写入，归约保留供回放。遗留：`answers:` 声明只能经 CLI `report --answers`；"把回答存为决策"未做；`tower_service::delegate` 与 `requests::delegate_id` 重复，待收敛
- [x] P2-D 兜圈子与节点提问（FR-126、127）— 见 P2-C。遗留：兜圈子检测每次全量扫事件，事件多时需增量化
- [x] P2-B HTTP 与文件入口（FR-117）— c9a176f、8d8fd28，合并 a6ab10d。偏差：文件入口放在运行时目录 `<runtime>/inbox/<project_id>/*.jsonl`，不在用户文档目录（守"不直接写用户文档目录"红线），已在用法文档说明
- [x] P2-E 注意力预算与打扰时机、关注卡上一步放行（FR-128、130、138）— 656dd4b、b21509e；tauri-plugin-notification 2.7.0。时机由级别推出（‼ 立即 / ○ 下次查看 / ≡ 摘要），只有 App 发通知。遗留：排序仍在 TS（C1 `needs` 需下沉）；一步回答只记"仅本次"；专注时段只显示、未接 macOS 专注模式；`help.rs` 中 time_sensitive"立即出现"文案未改
- [x] P2-F 跟进项、休眠与浮现（FR-134–136）— 2292c3b、0043a19。事件 `follow_up.added/done`、`resurfaced.seen`（仅 App）；配置 `quiet.dormant_after`、`quiet.check_every`。遗留：无 snooze；容器与线索的休眠未判断（→ P2-H）；CLI 列表归 C1
- [x] P2-G 立项（FR-183）— 与 P1-B 同包。立为项目只记目标（L4），不写 L0；立为任务需已发布版本
- [x] P2-A 扩展清单与权限（FR-120、123）— a630d9b、8d8fd28，合并 a6ab10d；CLI `ext ls`、`ext show`。扩展的检查只能阻止、不能放行（不当受信生产者）
- [x] P2-H 布局模型、模板、容器、容器规则、旁注、起步套装（FR-141–154、156、160–162，分子包；切分见 `docs/superpowers/plans/2026-10-23-p2h-split.md`）
  - [x] H1 地基：布局模型、4 个起步模板、容器对象、放置/移动/缩放（仅 App）、容器作 target 与角标；CLI `template ls`、`frame show` — 36ddd67、8b6a8e8、c748616，合并 cfaa28d。偏差：容器存 hub 事件加快照，物化进 `tower.canvas` 推迟到 H4（慢层保护）；elkjs 推迟到 H3。待人真机看一遍拖拽、缩放与分栏
  - [x] H2 卡片与规则执行 — 2d8edbc、a06bb57。事件 `card.*`；CLI `frame add/update/move/link/archive/propose`；Tauri `write_card`、`change_frame_rules`、`resolve_frame_item`。容器租约 60 秒按写入者续期，人的写入不占租约；超上限照写并折叠为"+N"。H2 单独时 App 写自由卡片会因带点 ID 被拒，修复在 6657c4e
  - [x] H3 布局推导（elkjs）— 6657c4e、467b9ee。事件 `card.pinned`（仅人，L3）；路由 `POST /frames/{id}/pins`；elkjs 0.12.0 只在"转为容器"按需加载一次，快层路径 0 次调用（有测试）；跨容器连线悬停显示、1 秒合并、细边框。偏差：冷启动仍用 Rust 网格布局。待人真机看拖拽固定、悬停连线、选中转容器
  - [x] H4 旁注、起步套装、物化与汇报 — 71822c4、f74f498、31221fe、54fc2d0。旁注 ◆✎? 跟随卡片不参与布局；起步套装 research/writing 内置、software 来自 devkit，采用/忽略仅 App；写入 `tower.canvas` 走预览→人批准→唯一写入模块慢层提交，`main_inspection` 可证明；`ask: update` 按规则更新；`review_every` 驱动浮现。遗留：基线发布重写 `tower.canvas` 与物化的交互未覆盖；`review_every` 不能清除；窄容器标题栏拥挤、旁注被裁切待真机看
- [x] P2-I 时间滑块与变化高亮（FR-157、158）— 同上提交。`frame_history` 重放得每步变化与任一 seq 的容器状态；范围高亮（自上次查看/今天/本周/某日起）只加角标与淡出，不动位置；时间滑块只读回看。偏差：默认仍为细边框，选了范围才分类高亮（不打扰优先）；无版本幽灵层与里程碑范围；历史每次整段重放无缓存
- [x] P2-K Note.md 连接器（FR-79、80）— 783673d、e661800，合并 a6ab10d。只建不改、纯 `.md`、不碰 vault 的 git；连接时静默建基线
- [x] P2-J 文本源容器与 PlantUML（FR-163–180）— bb94635、11dfdb6，合并 bcb2333（四套 780 / 31 / 323 / 0-0）。自写节点图子集解析（33 种元素、构造型与颜色、带标签连线、note、仓库内 `!include` ≤4 层 32 次、C4 宏只读），每次从 main 重新解析不另存；画布编辑与 `towermd frame edit` 经唯一写入者一行补丁提交；解析失败保留最后可解析版本并锁编辑，修复经预览由人在 App 套用；改名由人重新关联。事件 `frame.source_written/relinked`、`card.source`；66 命令 / 68 schema。遗留：Java 渲染器与原图显示（待人定）；时序/活动图计为未识别；frames 视图每个源容器一次 git 调用；重新关联不查目标存在；源容器无 FR-158 高亮角标；带空格名的源卡片不能钉住
- [x] U-1–U-4 豁免与次要变更、临时规则与快捷命令、插话"已处理"、画布视觉
  - [x] U-1 豁免与次要变更 — 494aed8，合并 8afd0fe。事件 `waiver.granted`（仅 App，默认 7 天、最长 30 天、不续期）；只豁免声明的硬检查且正在阻止的；到期按当前时间自动恢复阻止；到期前 24 小时进关注清单。FR-03 取保守：只改流程配置的主干提交标 `class: minor`，仍阻止，走"转提案"登记。遗留：门禁失败中断里的"豁免"选项留给 M2-G；不能提前撤销
  - [x] U-2 临时规则与快捷命令 — b3c7768。事件 `rule.temp.added/expired`；`/pause`、`/resume`、`/report`、`/rule … --until`、`/assume`、`//`。遗留：无 CLI 临时规则命令；不能提前撤销
  - [x] U-3 插话"已处理" — bd801a6、6d4f270、48f29ec。事件 `prompt.handled`；CLI `towermd handled`；排队中→已送达→已处理
  - [x] U-4 画布视觉 — 9d6bb28、c42b9a3、7854d39，合并见下一提交。卡片 1 秒合并（与容器共用 `coalesce.ts`）；拟议虚线 ┄、关系双线框 ⇄、新版本 0.5 秒缓动与 ◌ 旧版本幽灵层（减少动态效果时直接到位）；草稿对比叠加在画布上。测试直接比对 SvelteFlow 节点 transform 证明快层不移动。遗留：关系高亮不跟随当前选中提案；幽灵层不含分组容器；批准出版时旧版本闪一下；缓动、幽灵层可见度、开关遮挡待真机看

### 连续推进（2026-10-24 起）

人的指示：先不做真机走查，连续推进到"无需人确认的规格能力"全部完成。需要人确认的（新依赖、红线、发版、真实付费运行的大规模验收、`wss://` 的 TLS 依赖、Java 版 PlantUML 渲染器）一律跳过并记下，不停下来问。

波次（每波最多 4 条线：主工作区 1 条，其余各一个 worktree；主会话负责合并、跑四套、勾选、清理）：

- [x] 第 1 波（A、B、D 合并 8afd0fe；C 合并 62beaa5、27f4b6a；四套 677 / 28 / 273 / 0-0）：A 主区 P2-H2→H3；B U-1/U-2/U-3；C CLI C0 收尾（含 `completion`）+ C1 读命令（关注排序下沉 Rust 共享层）+ 小修（`help.rs` 文案、`delegate_id` 重复）；D M2-A 导入解析与聚类 + M2-B 从基线进入草稿
- [x] 第 2 波（四套 757 / 31 / 315 / 0-0，045ad2e）：P2-H4→P2-I；M2-C/D/G（G1/G4、`core.verify`、状态机与中断）；U-4 画布视觉 + U-6 性能基准；P3-A/C/D
  - [x] M2-C/D/G — f4c879a、a041259、492d561、403520a，合并 ba1f613、2666ec4，关注种类迁入 Rust 262693e。评审 `ask --review` 与 `V-` ID、`wait V-…`；无检查时 `core.verify` 兜底；门禁失败/禁止/缺信息/运行错误转 Held 并给中断选项（重试/改写/改派）；G1/G4 计划与代码评审；`TaskInterrupt` 面板；关注清单新增中断 ⏸、评审 ⚖、错误 ‼、预算 ¤（Kit 接缝 `alert`/`folded`/`review_stage`）。四套 745 / 29 / 307 / 0-0。遗留：RunError 与 Budget 真正停任务要等 M3-B 调度器
  - [x] M2-E/F/H — 0fc3ebc、75ae766、11d1885、12d84a5，合并 abd4e8f（四套 794 / 31 / 324 / 0-0）。M2-E：只读 `dev seed`、`POST /projects/{h}/seed`、`towermd-seed` 技能，已有批准版本时返回 `needs_proposal`；M2-F：生长信号（署名 `process:towermd`，只提醒），生长期/定型期不设默认阈值，由人写 `growth.mode` 或 `growth.settle_at_modules`；M2-H：App 重派复用 `task.released`，暂停与汇报只是给受托者的提示，任务卡在 `task claim/show` 首屏输出。67 命令 / 69 schema。遗留：批准 B0 时自动 `git init`（改由技能先 `git init`）；FR-22 每步生长后 main 可验证只靠技能与完成验证；§9.2 提案影响分析、接口契约、默认验证未做（FR-22 与 §9.2 已由 d84634b 补上；`git init` 裁定不做）
  - [x] U-6 性能基准 — 58d20fa、e3eedb7，合并见下一提交。记录 `docs/superpowers/plans/2026-10-24-perf-acceptance.md`。NFR-03（事件→实时状态 ≤2 s、Z0 首屏 ≤1 s）与冷启动在规格规模下达标；NFR-08（提问/回答 ≤5 s）在 200 任务以上不达标，根因：守护进程自己的快照提交触发全量重放、`dev_state::reduce` 每事件全表往返、发件箱全量读取、每次快照全量 `git log`、每次推完整状态帧。修复见下条
  - [x] 性能修复 perf08 — cbafc8e、9eb975c、48ca70e、80847e2、726463e、5a49216、371270c，合并 6ca9734 及下一提交。规格规模下 NFR-08 通过：提问→实时状态 max 710 ms，回答→等待中的 CLI max 1248 ms；首个状态帧 max 从 42.8 s 降到 36 ms。做法：自己写入模块写的快照提交登记在内存，从上次完整检查以来全是登记提交才沿用结论（外来、仿冒、改 L0–L2、重启后一律完整检查，有测试）；`dev_state` 与 Tower API 日志只读写涉及的记录；发件箱只读未确认记录；快照发布证明按提交缓存。遗留：发件箱不清理（恢复依赖已确认记录）；`main_writer::history` 仍全量 `git log`；外来提交后的完整检查仍持锁（中规模约 2.5 s，移出锁外涉及慢层检查路径，待写设计）；状态帧仍全量约 2 MB
- [x] 第 3 波（P3-E 视觉词汇待人定图标集，未做）：M2-E/F/H（种子、生长信号、App 重派）；M3-A/F/G（适配器与运行器用模拟 harness 验收）；P2-J（自写 PlantUML 子集解析，只做节点图显示，不接外部渲染器）；P3-B/E/F/G
- [x] 第 4 波：M3-B/C/D/E/H；WP-X Arca 退回本地目录；CLI C4 余项；最后统一回归（主分支 481415d 统一回归：四套 867 / 31 / 341 / 0-0，72 命令 / 74 schema，api_version 14）
  - 交付构建（2026-10-25，源码 914f7e5）：`APPLE_SIGNING_IDENTITY=<Apple Development, T5G56DH47L> pnpm app:build`，codesign --deep --strict 通过、hardened runtime；spctl 拒绝（开发证书，未公证）。实机：App 窗口渲染；`towermd open` 拉起守护进程（仅 127.0.0.1）；演示仓库 init + 冷启动扫描完成（3 模块，`make test`）。未验：App 内点“打开项目”后的画布与 B0 审批（本会话无桌面操作工具，留人走查）。产物 `src-tauri/target/release/bundle/macos/Tower.md-0.2.0-aarch64.zip`（sha256 b36345d8…ef53b），未打 tag、未推送、未发布。
  - [x] M3-B 调度器与集成器（合并 6117407，四套 830 / 31 / 334 / 0-0，72 命令 / 74 schema）— 分支 m3b：65c7969（核心 `Advance`：主干租约持有者经唯一写入模块快进 main）、3e976a8（派发标记与调度器领取路径）、8400a1f（队列、合并标记与调度计划）、4b85df0（集成步骤与 finish 请求接缝）、4f8d913（守护进程里的调度器与集成器服务）、a08d643（CLI `dev run`/`dev queue`/`dev dequeue`/`dev adapters`，模拟 harness 端到端合并测试）。FR-11 串行合并队列、FR-14 先拿租约再派发、FR-30 角色→harness/模型映射与按模块覆盖（`runner:`）、FR-31 并发上限默认 4、FR-32 变基后验证再合并；事件 `run.queued`/`run.dequeued`/`merge.done`/`merge.rejected`，调度器署名 `process:towermd-scheduler`、集成器 `process:towermd-integrator`；RunError 与 Budget 接入派发，真正停任务。70 命令 / 72 schema 文件，api_version 仍为 14，回放夹具未变。
    - 裁定：调度与集成只在持有主干租约的守护进程上跑（`maintain` 周期加事件触发，在运行时锁内执行，所以合并天然串行）；同优先级先进先出；守护进程重启后发现的孤儿运行记为 Interrupted，已启动但运行器出错的记为计数的 Failed；报告非 done、预算拒绝、找不到 harness → 提醒加撤下，等人决定；集成器从不替 Agent 提交未提交的已跟踪改动；变基/验证被拒计入 FR-57 自动修复次数，快进失败（main 已动）不计数且去重；验证中断也计数；调度器派发的运行不接受 CLI 的 `dev finish`/`task release`，改用 `dev dequeue`；门禁结果按"main 版本 + 豁免"记忆 10 分钟，避免反复写证据；只有分支头变化才重新验证，main 只多了状态提交时变基后沿用原证据（`prepare_g5` 比较代码清单，不含文档目录）；验证期限从第一次通过的证据起算；工作树放在 git 公共目录下，合并后保留；harness 程序覆盖只走环境变量 `TOWERMD_HARNESS_<ID>`。
    - 遗留：孤儿进程不杀，只记中断；合并后工作树不清理；租约过期或验证期限过期只留等待原因，需人在 App 改派；WP-X 原始输出只留本机 `<公共目录>/towermd/runs/<运行>/<次>/`，Arca 归档未做（已由下条 WP-X 补上）；派发的运行没接 MCP 配置、权限工具与技能目录（M3-C/E 范围）；调度器 INFO_REQUESTED 的 devkit 单测没加（端到端测试覆盖派发主路径）；真实 harness 未跑。
  - [x] M3-C/E 交互网关与技能包锁定（合并 f45237c，四套 854 / 31 / 336 / 0-0，72 命令 / 74 schema；分支 m3ce）— 1116be8、4ff9d65、6fcf9bf、1bd4846、af736da、194b8eb，文档见下一提交。隐藏命令 `towermd mcp`（手写 stdio JSON-RPC，只连已在运行的守护进程，`x-towermd-client: mcp`，以运行记录里的 Agent 署名，只服务运行器启动且仍在运行的运行）；4 个工具 ask/request_review/permission/note，无批准类工具，未答的提问与评审让 Agent 以 `needs_input` 结束本回合；授权 `K-` 惰性组与关注种类“授权”⚿，App 只能本次允许或拒绝，10 分钟未答与运行结束都按拒绝，由 `process:towermd` 记录；技能包按已提交版本读锁定、核哈希、只组装角色启用的技能到运行目录，经 `Skills` 通道显式传给适配器。遗留：授权“写入允许规则（L3）”；`--permission-prompts none` 下的拒绝记录；评审类型 design；`gateway::wire`/`skillpacks::wire` 与停车后续跑由 M3-B 调度器接入，接入前授权与网关的完整正路只用模拟测试；网关不转交 `delivered` 插话；交互点映射只校验与记录，尚不改变调度
    - 接入调度器（e720649，四套 855 / 31 / 336 / 0-0）：派发时以 executor 角色组装 main 上已提交的锁定、接上网关与授权工具；加载记为 `skill.loaded`（署名 `process:towermd-scheduler`，devkit 校验署名/层级/运行，投影 `SkillLoaded`），运行详情可见；锁定有误时派发提醒并撤下。遗留：停车后恢复运行（needs_input 之后自动续跑）未做，留给 M3-D 总控线。
  - [x] M3-D 总控、M3-H 交叉评审、WP-X Arca 归档退回本地目录（快进合并 481415d，四套 867 / 31 / 341 / 0-0）— 分支 m3dhx：6ab1223（核心问题模型：总控回答的依据、通知与人改答）、0d1ed87（开发套件：只读咨询、总控分诊、交叉评审、原始输出归档）、584f1a6（守护进程：调度之后跑总控、交叉评审与归档；模拟 harness 端到端代答测试）、bde488e（App：总控依据与改答提示、提案面板交叉评审、仅本机归档计数）及文档提交。事件 `hold.answered`（总控答，署名 `process:towermd-controller`，带 `basis`/`notify`）、`controller.triaged`、`proposal.cross_reviewed`（`process:towermd-reviewer`）、`artifact.archived`（`process:towermd-archiver`），新分组都惰性创建，回放夹具未变；72 命令 / 74 schema、api_version 14、invoke 26 均未变；无新依赖。
    - 裁定（M3-D）：总控只看调度器派发的运行的问题，交互会话的问题留给会话里的人；只有 `controller.policy.question` 可配（auto / auto_notify 默认 / human，写错按 human），其他中断种类一律问人；慢层目标、策略为 human、`delegation_view::permit(AnswerByDecision)` 不放行（默认级别 `report_only` 不放行）时直接交人并记原因；每个问题只分诊一次，交人的问题保持打开；不做"先按假设继续"；咨询只读（claude `plan`、codex `read-only`），工作目录为该运行的工作树，最多 1 次、无预算、默认 5 分钟；超时、失败、`in_scope: false`、空回答一律交人；进行中的咨询只在内存，重启后重问；已结束运行上的停放问题照样分诊（回答在下次尝试送达）。
    - 裁定（FR-66）：人另答替换总控回答并记 `overrides`；受影响项取总控回答之后该运行的 `run.finished`（检查点不在投影里），App 提示"需要时重新派发"。
    - 裁定（M3-H）：只在配置 `runner.roles.cross_reviewer` 时开启；"异构"按 harness ID 与执行者不同判定，相同或未知 harness 报配置错误；所有待审阅提案都算慢层，各评一次，结论绑定内容摘要；超时/失败/读不懂记 `unfinished`，从不算同意；批准路径不读评审结论，App 只作"仅供参考"展示。
    - 裁定（WP-X）：只在 `<docs_dir>/artifacts/.arca` 已存在（人用 `arca setup` 建好）时才用 Arca，Tower.md 从不 setup/push/gc；`arca` 只作子进程调用，`TOWERMD_ARCA` 可覆盖、设空禁用；任何失败退回本地 `<公共目录>/towermd/runs/`（本就不在仓库内）并在记录里写原因；归档是持有 main 的守护进程在调度之后补做的一轮（覆盖本机所有已结束的尝试，崩溃后也能补），不挂在 launch 里；`arca doctor` 在项目打开后首次归档或查状态时跑一次，近似规格的"守护进程启动时"；状态放在 `dev queue` 的 `archive` 字段。
    - 遗留：FR-67 代答前的行动计划预览未做；FR-70 `/retry` 快捷命令未做（`dev run` 入队只收 CLI，App 侧入队要新命令与 invoke，留给 App 线）；总控与评审的费用记在各自事件上，不计入模块预算；归档记录不随文件删除而失效，`arca doctor` 结果不刷新；App 改动是文字，未真机看；停车后恢复运行（M3-C/E 留给本线的 needs_input 之后自动续跑）未做：总控答复在下次尝试送达，不自动重启运行；真实 harness 与真实 Arca 未跑。

- [ ] 第 5 波（2026-10-25，起点 b1c86d3；两份重审见下）：剩余不需人定的规格缺口，四条 worktree 线
  - [ ] 5A 运行器与网关：needs_input 停车后回答即续跑（Adapter `resume`）；运行中插话经网关转交 `delivered`；孤儿进程按 pid 清理、合并后清工作树；`run.checkpoint` 与 FR-66 按检查点；FR-63 `--permission-prompts none` 的拒绝记为禁止；总控与评审费用计入模块预算；评审类型 design
  - [x] 5B 门禁与总控策略（合并 4245819，四套 888 / 32 / 348 / 0-0；perf_cli 在负载 36 时 NFR-03 超限一次，负载降到 12 以下连跑两次通过）— 9b1af40、9722edc、07b35d7、4ebf419、4f29387、7a13d40。72 命令 / 74 schema、api_version 14 不变；新事件 `controller.retried`。裁定：G2 默认清单保守、`gates.yaml` 只能追加，命中即禁止中断且不可豁免；FR-55 覆盖只收紧，对象没报告的维度取严，写错整份失效全阻止；FR-65 只有 `gate_failed`/`error` 可设 auto，每任务每类只重试一次且只限派发的运行，疑似设计错误、评审、授权、禁止、预算一律交人；FR-31 上限>3、在跑≥2 且每任务平均≥1 次中断或已等 30 分钟时建议降到 3（≥2 次降到 2），即时计算不存；FR-53 慢层提问作用于整个运行，`gate.G1`/`gate.G4` 要求人评审。遗留：FR-67 预览、`plan.changed`、12.5 全局输入交总控未做；FR-31 提示未进 App；禁止路径与总控重试无守护进程级集成测试；危险命令中断沿用“改动被路径守卫拦截”标题。原条目：G2 敏感文件与危险命令（保守默认，只拦不放）；FR-55 按层、模块、风险覆盖门禁；FR-65 其他中断种类的总控策略（不含"写入允许规则 L3"）；FR-53 交互点映射影响总控与门禁；FR-31 中断偏多时建议降并发；FR-67 代答前行动计划预览、`plan.changed`、12.5 全局输入交总控
  - [x] 5C 决策与弱化、基线物化（合并 ec64615，与 5D 的 5 处追加冲突手工合并；四套 904 / 32 / 352 / 0-0）— 643df40、ad1ed09、1f01bdf。72 命令 / 74 schema、api_version 14 不变；4 个 schema 的 source/state 枚举只加不改；invoke 合并后 32（新增 `record_decision`）；路由 `POST /projects/{h}/decisions` 只收 App。裁定：人记决策事件 `decision.recorded`，L3（与 `rule.recorded` 同层），ID `D-<request_id>`，来源只有已回答的节点提问（reply）或写理由接受的弱化（weakening），同一来源只记一次，命令行不提供；验收收获已是 `acceptance.confirmed` 里人的记录，不重写，`ls decisions` 直接列出（来源 acceptance）；问题决策仍 L4、提案决策仍 L1；FR-186 由守护进程从本机 main 读 `<docs_dir>/decisions/` 中带 `id` 与 `review_on` 的文档，按 main 版本缓存、最多 2 秒复查、不落盘，复查文档时 `decision.reviewed` 带 `document:true`；FR-125 模块 `paths` 减少算范围缩小，扩展 `<ns>.weakening[.<sub>]` 出卡并注明自报，`core.` 仍只归 Tower.md；FR-154 新基线重新生成 `tower.canvas` 时保留已物化画框（有复现测试）；FR-117 `reply` 进 `POST /tower` 与收件箱。顺带修：状态接口每读一次在项目锁内查 git 主干，改为最多每 2 秒一次（复查请求仍现查）。遗留：扩展自报的弱化不进信任账本；截止推迟待人定；复查卡“要改写，就重新回答这个问题”对 App 决策与决策文档不贴切；未真机看。原条目：FR-127/206 人的通用决策记录（节点提问回答记为决策、验收 takeaway、FR-125 接受理由）；FR-186 提案决策与手写 `docs/decisions` 的 `review_on` 出复查卡；FR-125 范围缩小与扩展子类；FR-117 `reply` 进 `POST /tower` 与文件入口；FR-154 出版新基线丢已物化容器（先写复现测试）
  - [x] 5D App 与命令行（合并 3ccd038，四套 876 / 32 / 348 / 0-0）— 03e6c0f、c98dce0。72 命令 / 74 schema、api_version 14 不变；invoke 27→31，另有 init_project、follow_locate。裁定：`/retry` 只限任务范围，署名 human，App 与 CLI 请求号不共用；暂停/继续是人的 L4 事件，运行结束后仍保持暂停，验证中不能暂停；流程规则 ID `RULE-<请求号>`，显示在同模块任务卡并提示在跑的受托者；回滚按三方合并算出撤销提交、人批准的就是该提交，守护进程在 main 围栏下推进，冲突时拒绝并列文件，main 前进后须重发，每次合入只回滚一次，回滚后任务暂停；`/projects/init` 只收 App、非 git 文件夹拒绝、从不 `git init`；`open` 定位请求只在内存、TTL 60 秒、取一次、不写事件。遗留：`attention.ts` 里仍有“门禁”字样（测试固定）；定位按路径字符串判断当前项目，规范化不一致会重开一次；初始化与定位界面未真机验证。原条目：FR-70 `/retry`（App 入队）；15.5 按任务暂停/继续；`towermd open [ID]` 经守护进程推送让 App 定位（不加 deep-link 插件）；12.3 回答"记为流程规则"；FR-13 按任务回滚；FR-98 App 内初始化已是 git 仓库的文件夹；NFR-14 界面行话清理
- [ ] 第 6 波：画布显示（FR-135 休眠变淡、FR-157/46 容器版本与幽灵层、FR-158 里程碑以来、FR-39/44 模块聚合与健康色）；模板与 PlantUML 小项（FR-173 规则、"写进容器规则"、FR-159 两种视图）；FR-190 能力对等表与测试；Arca 余项与 G3 测试检查；FR-172 第 2 阶段图种；FR-197 守护进程消息随语言（最后单独做）
  - 本轮仍需人定（不做）：FR-125 截止推迟（§16 待决 4）；FR-116 `request_report`/`request_update` 改名；首批锁定技能包（开放问题 7）；授权"写入允许规则（L3）"；其余见上文与第 3 阶段

主会话对规格歧义的裁定（供各线遵循，落地时同步进规格）：

- 豁免（U-1、M2-G，D2#3）：只能由人在 App 发出，必须带到期时间，默认 7 天、最长 30 天；到期即失效，不续期。
- FR-121 与本文件第 556 行（D4）：按"进度只认证据"，开发任务仍须经检查；没配检查时由内置 `core.verify` 兜底；"人在 App 确认完成"只用于非开发对象。
- 中断与告警（D3）：门禁失败与"禁止"把任务转入 Held 并给中断选项（含带到期的豁免）；旧告警事件保留以供回放。
- Failed 的入口（D5）：由运行器的 `run.finished=failed` 进入；"失败 3 次"与"验证 3 次未过"分开计数。
- G0 缺信息（D6）：转成发给任务创建者（人）的提问，进关注清单；Agent 侧照常报错。
- 适配器（D8、D9）：放在 M3，先做 Claude Code 与 Codex；验收用模拟 harness，真实 harness 只做人已同意的小规模冒烟，大规模由人跑。发 SIGINT 用系统 `kill` 命令，不加 `libc`/`nix`。
- CLI（C4、C5、C12、C13）：§3.5 的 `--json`、错误码 `rejected`、`hub rebuild` 两步保持现实现，改规格；`dev scan wait`、`dev finish --wait` 保留为兼容别名；长度按 Unicode 字符计；`task claim` 省略 id 时只有唯一可领任务才领，否则列出候选并报错。

### 第 3 阶段（核心）
- [ ] P3-A 多项目总览与健康色；P3-B 委托级别执行与账本补全；P3-C 决策复查；P3-D 变化摘要；P3-E 视觉词汇；P3-F 时间派生视图；P3-G 文本导入导出
  - [x] P3-B/F/G — 38d234d、b891621、d2dff74、a96b138，合并 9a11e8e、75152cc（四套 809 / 31 / 333 / 0-0）。P3-B：G2 越界按运行计数（FR-202）；委托级别写明允许的能力，只能设成已配置的级别，从不自动升级，`permit()` 永不放行人定内容（FR-185）。P3-G：粘贴文本只导入为新的自由容器（同一 `frame.created`，重放不跑解析器），任何容器可导出为文本（文本源容器除外）；超长标题拒绝不截断。P3-F：模板 `views` 字段，看板 1.1.0 声明累积流（FR-159），从事件按 UTC 日现算不存储，路由 `GET /frames/{id}/views/{name}`。68 命令 / 70 schema。遗留：P3-E 视觉词汇（FR-140、73、44）依赖规格 §16 开放问题 6 的图标集，待人定；FR-159 时间线偏差与论证图演变两种视图未做；M3-D 落地要接 `delegation_view::permit`
  - [x] P3-A/C/D — 18a2da4、9d46336、62d2813、7ff13b9，合并 9a075af、1a3cda8、045ad2e。P3-A：`towermd_core::health` 读时从事件重算（▲ 需处理 / ◆ 需留意 / ● 正常 / ○ 没有任务），`health.no_progress_hours` 默认 24；`GET /overview`、`attention_list::across` 跨项目首屏 ≤3；CLI `overview` 走守护进程；App"全部项目"面板。P3-C：事件 `decision.reviewed`，仅 App，关注种类"复查决策"。P3-D：Tower API `summary` 动词、事件 `summary.received`、CLI `towermd summary`（65 命令 / 67 schema）。裁定：中断/错误/预算算红，委托评审算黄；最近检查结果取最差当前结论。遗留：容器画布按模块健康上色未接（数据在 `health.areas`）；手写 `docs/decisions` 的 `review_on` 不出复查卡；扩展清单无摘要声明项；FR-39 扩展聚合字段未加

### 开发套件与命令行
- [x] CLI C0 收尾与 C1 读命令 — 02903bb、a58af9d、946a9ca，合并 62beaa5，迁移 27f4b6a。C0：`task edit/ls`、无 id 领取（唯一候选才领）、`wait <E-id>`（最长 85 秒）、`--project`、`completion`、FR-199 分组帮助；C1：`towermd`/`needs`/`since`/`overview`/`ls`/`history`/`watch`/`open`/`decision`/`rule ls`（含临时规则）/`rebuild-index`/`doctor`/`whoami`/根 schema。关注清单只在 `towermd_core::attention_list` 排一次（含豁免到期、次要变更），App 与 CLI 共用，Rust 与 TS 共享夹具。64 命令 / 66 schema 文件。遗留：`since` 文案只有中文；`open` 无深链；`rebuild-index` 空操作；`doctor` 不测 presence
- [x] M2：import 解析与聚类、从基线进入草稿（M2-A/B 完成 — 657354a、a6babfa、b711829、a83ee8e、f0a8f3b，合并 8afd0fe；路由 `POST /scans/{id}/draft`、`GET /baselines/{id}/compare`；CLI `dev scan start --from-baseline`、`dev draft compare`；遗留：J4 第 3–4 步归 M3，Reference 节点不沿用，无"把文件指派到模块"操作）、G1/G4、一句话生长、生长信号、状态机补完、App 重派与任务卡
  - [x] M2 收尾（§9.2 门禁、FR-22）— d84634b。生长模式读 main 配置与已批准模块数；生长期 B0 照旧放行，之后每步出版须有一条验证命令真实通过；定型期每个模块须由验证命令通过、接口 `contract` 须指向仓库内存在的文件、提案须按受影响对象 ID 附完整 `impact_notes`（新哈希 v3，进入决策）；不满足返回 `gate_failed`。批准 B0 时自动 `git init` 不做：init、守护进程、固定扫描和唯一写入者都要求仓库已存在，已记入开发套件规格 §9.1
- [x] M3：适配器与运行器（M3-A/F/G 完成于分支 m3a — f8bed53、5c938af、1b71caf，经 M2-C/D/G 一并合入；四个适配器，专有参数只在适配器内并有守卫测试；SIGINT→SIGTERM→SIGKILL 清进程组；费用未知不放行；报告不改状态；遗留：`dev adapters`、配置节接入、Arca 归档原始输出归 M3-B/WP-X）、调度器与集成器、MCP 交互网关、总控、技能包锁定、运行报告、预算中断、交叉评审
- [x] CLI C4 — 1dc5e0a。按 2026-10-23 裁定，人的动作只在 App，命令行不新增命令（68 命令 / 70 schema 不变）；验收测试走完"命令行记想法 → App 立项 → 命令行领取、完成、合入后经检查完成"。遗留：跟进项推迟（snooze）语义未定，待人定
- [x] 离线与远端 hub；基础设施 I-A Cloudflare hub — 69c300f、d1abcca、61d5101，合并 6dd047f。`hub: { url: ws://… }`；hub-worker 13 个测试；断网普通事件进发件箱按原 eid 补发，人的动作与租约离线即失败。遗留：只支持 `ws://`（`wss://` 需 TLS 依赖，待人定）；无认证加密；不能从本机 hub 迁到云端；跨语言冒烟测试需手动起 `wrangler dev`
