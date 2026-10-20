# 任务计划

## 2026-10-20 继续原项目真实验收与首版发布

用户已要求继续，恢复实机验证。实现起点为 main `9a0e80d`；旧C4不包含后续expiry修复。本轮不新增外围功能或依赖。

- [ ] 冻结并构建新候选，核全部受控源字节、实际包内CLI／技能及ARM包；旧QA项目与原B1批准保留，只运行一份App。
- [ ] 原B1回执→真实画布→Ready／Running任务复核→正常关闭重开实机验收；95秒合同、原UUID和全部fences不变；自动实机标agent_qa。
- [ ] 对最终候选完成冻结workspace／native／前端及发布工具必要检查；每批串行、nice10、jobs2，保留实际失败，不重复计数。
- [ ] 完成两种真实harness、三个工作树、十任务P7；人工动作与自动QA证据明确区分。
- [ ] 逐项核对所有工作树／分支／stash和WIP保护，按冻结R/tag构建双架构未签名材料、实际上传及回读；发布后清理已保护本地分支。

## 2026-10-20 实机 B1 出版断点整改（当前阻塞）

2026-10-20 用户明确选择稍后实机测试：唯一C4 App PID24014已核完整路径后SIGTERM正常停止，无强杀、无其他进程被信号；保留QA目录和原批准，不计正常关闭重开验收。GUI测试暂缓，继续独立代码／自动回归。

**完整回归未通过，须重新规划**：`candidate-9a29d35-workspace-all-01` 在首个towermd单元批115项中观察到53项PASS、8项FAIL、2项挂起，其余及core／集成尚未执行。采样证明两个挂起测试在writer时钟回调中重入Session锁（`snapshot_hold`）；只核验并SIGINT自有无子进程的PID70055，18448.13秒后收口为中断，来源前后不变。保留原log、sample和partial收据，不把其包装为全测通过或确定8项同根因。

- [x] 在原源码上nocapture重现App接口和finish到期失败：`candidate-9a29d35-failures-reproduce-01` 实际两项FAIL，来源不变；并行源审不运行工具链。
- [x] 用实际Q冻结／锁边界修复三项时钟回调夹具，保留原真实later hold与期限断言；实际PASS，已提交 `de981c7`，生产时钟仍在最终授权锁内取时。
- [x] 最小修复后执行受影响回归并精确提交；core9项、Ask1项及最终daemon5项分别通过，详见本节真实收据。新的完整冻结回归仍待；GUI依用户选择暂缓，不再次催解锁。

当前最小修复计划（GUI依用户选择暂缓）：

- [x] 将 Ask 来源故障注入移至实际 Catalog pin 之后，保留503／原UUID／单条答复及无Review授权断言。实际 `ask-pinned-source-fixture-01` 1/1通过（70.40秒测试／133.61秒总耗时），来源前后一致；独立源码复核无确定阻断。此项仅cfg(test)夹具纠正，不是产品来源门禁变更；完整批次／实机仍未通过。精确提交身份登记外部owned-commit收据。
- [x] 补全 finish expiry 的真实 TaskCheck 历史上下文及严格来源校验；不改整Run一致性保护。实际核心9/9通过（715.99秒测试），workspace/all-targets release Clippy通过；三个原daemon到期失败回归已通过。精确提交身份登记外部owned-commit收据，未进入C4 App。
- [x] 已运行受影响核心与HTTP／到期回归并精确提交对应责任路径；Ask为 `8934533`，expiry context为 `929aa3c`，最后handoff提交身份由外部收据登记。完整回归及实机未通过前不发版。

精确owned交接最终回顾：生产仅增加真实输出retain之后、producer之前的私有只读合取检查：原ticket的project/common/run/evidence/deadline、active／expiring／cancelled、真实cleanup与保留输出、当前到期时间均匹配，才返回已有uncertain本地视图，不设置多余恢复信号；实际expiry仍走原core证明，未知所有权／普通shutdown不适用。原四用例函数后缀逐字未改，临时诊断已移除。独立审查无阻断，workspace/all-targets release Clippy实际通过；`expiry-owned-handoff-five-01` 实际 **5/5通过**（195.38秒测试／233.32秒总耗时），来源前后一致，包含原四项及真实ticket／执行输出／scope负例。原四项批次3PASS／1FAIL及两次单独RED均保留。新源码尚未进入C4 App；完整冻结回归、GUI、真人P7与正式发布仍待。

无扰动诊断实际结果：`expiry-owned-deadline-observed-01` 保持原断言FAIL。flight6在13.896秒内完成actual expiry与statuses；失败时job已expired、实际expiry eid存在、drained／cleanup均true、retained已由实际expiry清空，但worker的正常晚中断拒绝留下recovery信号，立即启动flight7。额外4.674秒晚中断proof及第二轮协调是已观测重复工作，不是清理死锁。临时3文件诊断已按实际测试哈希封存并恢复base，不进入产品提交。下一最小修复：仅在原actual-ticket精确绑定、真实输出已保留且cleanup已确认、原期限已过并由本daemon取消到期的路径交还maintenance；不制造终态／Ready、不丢输出、不影响普通shutdown或未知所有权，保留原15秒与实际expiry证明。

剩余owned到期用例重新规划：原源码单项 `expiry-owned-deadline-isolated-01` 再次实际FAIL（97.17秒），不能归咎于同批并发。先加仅测试、无阻塞且按目录隔离的阶段记录，在原15秒断言失败时显示维护是否仍在真实worker退出、expiry证明或第二轮协调；不改变生产调度／期限／原UUID。取得阶段证据后再决定最小修复，最后复验原用例及实际清理负例。

到期上下文修复实际回顾：`expiry-check-context-core-02` 9/9、`expiry-check-context-clippy-01` 通过，来源前后一致；首次core-01过滤0项只算编译，不计通过。`expiry-check-context-daemon-four-01` 实际3PASS／1FAIL（166.28秒测试），原queued到期、未知所有权Running、破损来源下自有子进程清理均通过。剩余owned续租用例在固定60分钟维护15秒观察窗失败，retry_at=0；worker的“deadline prevents a new interruption”本身是正确硬门禁拒绝，尚不能据此断定expiry producer错误。正在原源码单独重现，未改期限／断言／请求身份。完整批次与实机仍未通过。

夹具整改实际进展：恢复HTTP helper从仅测试的10秒对齐9a29d35中Native既有95秒transport合同，未改变产品期限；原actor／epoch两项实际PASS（budget-alignment-01整批为2PASS、1FAIL，失败指私有fixture误走retired business入口，保留）。安装失败用例改为读取registered owner后，其inactive／Status recovery_required／禁止claim／旧确认已消费／新确认等原断言实际PASS。三项快照用例改用隔离libtest child的真实commit-tree屏障，clock纯读，原25／35秒与late Busy断言保留；实际三项均PASS（child不重复计数）。`fixtures-and-ask-01`整批4PASS、1FAIL、95.92秒测试，来源前后一致；剩余Ask的503期望实际收到200，已定位旧clock hook在server.project阶段、目录pin之前提前改变main，独立候选校正中。完整批次和真实finish expiry代码修复仍未通过，不能用这六项分批绿替代。

**最新候选 C4** 为 `9a29d354f33c430eb239b97539c1759ec3037fd0`，tree `aea17d1ccad678f411c67c2293702b5682fc1a3c`。读取修复 `0e6f529` 和最终时钟修复 `9a29d35` 已精确提交 main。独立干净检出的414个受控文件逐字相等，完整App／技能构建通过（66.25秒），实际ARM／临时签名完整性／包内CLI代码等价／技能契约通过；格式及native全目标Clippy通过。前端90个输入与C3逐字相等，复用C3的230项和类型结果，未冒称重新执行。新App在 `/private/tmp/towermd-candidate-9a29d35-20261020/Tower.md.app`，构建后曾仅运行一份（PID24014，现依用户暂缓实机的选择已关闭）。当时系统实际报告锁屏，CUA返回`cgWindowNotFound`，原B1回执／复核GUI均未操作，收据 `candidate-9a29d35-gui-blocked-01.json`；未发布。

同C4的P7新准备目录 `/private/tmp/towermd-p7-9a29d35-20261020` 已由公开工具建立三个工作树，种子6测试通过，十卡全not_run；实际同包CLI init成功并检查生成内容后，以初始导入提交 `7e926592053dbbefdcf1e5183c4162b3a5541e60` 推入本地夹具origin/main。尚无B0、Task、模型会话或人工批准，不算P7通过。受影响的七项出版回归已实际7/7通过（818.58秒测试／818.67秒总耗时），冻结来源前后不变；完整workspace／native回归和GUI／真人P7／正式发布仍待。

本次接续计划（用户已再次解锁）：

- [x] 完成当前 V2 原请求／真实 B2 回归；保留完整退出状态，不将长跑或中断计为通过。`gui-proof-read-v2-b2-original-request-01` 实际 1/1 通过（1146.10 秒测试／1146.24 秒总耗时），源码前后不变；真实 B0/B1/B2、完整 OID 有限读取次数、旧 UUID 原回执与 B2 标记保持均通过。V2 上下文专项第二次实际已通过（1/1，186.64 秒测试），首次错误测试前置的失败保留；这不代表 App 95 秒验收已通过。
- [x] 审查并验证 TaskRecheck 最终锁内重新取时的最小修复：慢证明结束后处理实际租约到期，再核 Q、原件及完整 fences；实际旧时钟负控必须失败，修复负例及已有正例必须通过。最初隔离四路径候选的未编译状态保留在外部manifest，以下记录真实后续。
  - 后续实际推进：V2 读取修复已精确提交 main `0e6f529ca8a89d4a727937fca1d18abb9adef545`（10责任路径，index补丁与实际提交逐字相等）。旧时钟负控两例均真实 RED（337.40秒测试／417.80秒总耗时）：到期后仍写实际 resolved 原件并返回 HistoricalOnly。隔离414文件前后一致，未改失败断言。随后四路径修复按base哈希进入Root并格式化，独立复核仅格式差异；workspace/all-targets Clippy实际通过（14.46秒）。修正版两负例、五项不可变读取及既有Ready／Running正例整批运行中，仍未提交此安全修复／未构建新App。
  - 最终整批 `gui-recheck-fresh-clock-green-and-regressions-01` 实际 **9/9通过**（862.77秒测试／942.81秒含编译）：两种clock-only到期都拒绝且无放行原件，保留复核标记；既有Ready原UUID幂等、Running续租／完整expiry／归档省略／重建历史回执，以及五项不可变读取均通过。来源前后一致，最终四路径与独立审查仅格式差异。本段随精确提交登记于外部收据，不把代码通过当作新App／真人P7通过。
- [ ] 精确提交已验证的责任路径，冻结并构建下一候选；只运行一份 App，沿原 B1 UUID 验证回执、任务复核、正常关闭重开。95 秒请求预算不变，实机未通过前不称问题已解决。
- [ ] 随后完成冻结回归、真人 P7、正式未签名发布和受保护的本地分支清理；当前均未完成。

历史 C3 状态：核心与前端修复已分别精确提交 `524fd2950618eaf671c849b7def5883a6de23976`、`79ff51aff75a3cd26faa57731b90a5cff55926e3`。新候选 C3（524fd29）完整构建、ARM 包静态检查、前端230/230通过；413个受控源文件构建前后不变。实际原项目启动后，沿原批准自动恢复唯一 B1 canonical S=109，固定 Q commit `95155af1b107a43f968fe46eda9419c35cc98fd4` 的 through_seq=108，并记录 T-qa required marker=110；B1 tag及原批准UUID不变，只有一份 B1 approval。原始事件和断言在夹具 `artifacts/app-524fd29-canonical-b1-proof-01.json`。

**C3 GUI 仍未通过**：点击「读取或协调原审阅结果」后本机请求超过既有95秒总超时，App显示网络错误，未看到出版完成回执；回到主画布后真实态势仍在读取。真实进程采样定位 repeated recovery→publication→fixed_source 证明，不能用磁盘S已写入代替用户流程完成。未增加超时、手动snapshot、重批或另建UUID；Task复核／关闭重开未验。唯一App PID84749已正常Cmd-Q；公开daemon stop报告仍在停止，PID86735尚须后验检查，不强杀在途Git。

后验：上述两个PID均已实际消失，未强杀；C3 GUI结构收据为 `candidate-524fd29-gui-01.json`（10份原始材料哈希）。同候选类型检查0错误0警告（3.44秒），受控来源前后不变。以下原C2记录保留为历史，不表示当前源码仍未提交。

- [ ] 最小修复单次读取中的重复不可变Git证明工作；保留实际 refs、当前源后验、每个来源的canonical／original冲突检查和95秒限时。不得引入跨调用授权缓存。
  - 单次有限读取候选已进入工作区（未提交）：显式传递完整OID的Catalog／tree／blob／类型／父提交／固定快照读取；不缓存完整proof或outer角色，各来源仍独立验证canonical、冲突、refs、人口与物理inputs后验。check01缺既有fixed_source消费者wrapper已修，check02 workspace/all-targets通过；受影响及新增不可变读取回归5/5通过（9.03秒测试／89.42秒含编译）。真实B0/B1/B2旧请求隔离及有限读取次数回归正在执行；未据此声称95秒实机问题已解决。独立源审5路径SHA一致，无确定授权放宽，重复probe成本仍待实际验证。
  - 上述窄候选的B2长跑已因采样确认漏接TaskRecheck内部owner而转入新整改；1066.33秒后SIGINT中断，非PASS／非断言失败。新版（仍未提交）让TaskRecheck内部验证沿用同一有限owner，并以完整context／binding／parent key复用该次读取中已完整核对的提案上下文；不保存history、outer角色、复核成功或当前授权，命中前后实际核refs／来源及祖先。原件、canonical、人口与物理inputs检查保留，首次仍完整证明。新版全workspace/all-targets Clippy02通过；01多余引用已修。独立六路径源审无确定阻断。新上下文用例01在“不同parent”测试前置错误失败（实际Q恰好等于原parent）；已只将cfg(test)改为实际不同的publication commit，02重跑中；未将该失败冒称产品错误，生产校验未改，B2整项与GUI仍待。
- [ ] 新源码验证、精确提交和重新冻结候选，再沿本夹具同一批准完成GUI回执→任务复核→正常关闭恢复。
- [x] 独立P7准备工具已实际创建 `/private/tmp/towermd-p7-524fd29-20261020`，三个detached工作树、种子6测试通过，十卡全not_run；未init、未造B0／Task／真人批准或启动模型。来源为C3，不算新候选或真人P7通过。

候选 `4f6adf496da1c3912b851181323cdda3a4eacdd2` 已从冻结源码完整构建并通过实际包静态检查；未正式发布。用户解锁后，唯一 App 在独立 `agent_qa` 项目完成三范围插话送达、暂停／精确范围恢复和 G2 禁止原件下钻。B1 的实际原批准已记录，tag 已建，但 canonical 出版仍 pending：`publication population Q changed before canonical creation`；同 UUID 重读尚未恢复。不能把 tag 或批准当出版成功。

- [x] 保存当前实际 Run、CLI 原输出、App 原请求／差异、pending 结果与进程采样；不手动 snapshot，不新建批准，不作为真人 P7。
- [ ] 对照规范核 Q→S 与持续主干续租的实际并发；定位重试是否真正前进，形成不降低证明或门禁的最小修改。
- [x] 修复候选源码并验证真实续租穿插时的拒绝与原请求恢复；保留精确 Q+1、完整人口、原事件身份和来源后验。当前仍未提交／未实机复验，不算已进入发布。
- [ ] 新源码精确提交并冻结新候选；重建唯一 App，在实际时钟下重验 B1 出版→任务复核→关闭／重开。
- [ ] 之后继续双 harness／三工作树／十任务真人 P7、冻结完整回归及双架构正式发布和本地分支清理；缺项仍为未完成。

附带观察：正常并发写会让完整读取源校验拒绝旧视图；前端短暂显示“失联”或 `[object Object]` 后恢复。已完成只读定位，是否一并修复以最小用户主路径范围决定，不放宽来源校验或重试预算。

- [x] 瞬态读取提示窄修：严格按错误 code 区分 HTTP 来源竞争与已关闭的 WS，保留已核实画布和健康订阅，成功补读取消陈旧定时器；不因成功读取／open 重置原六次预算。实际旧产品 RED 3/11 保留，新版目标18/18、前端完整230/230及类型0错误0警告；前后来源不变，独立只读审查无阻塞问题。源码未提交时不算已进入候选，提交身份由外部收据登记；新包视觉复验仍待。
  - 上述前端窄修已精确提交 main `79ff51aff75a3cd26faa57731b90a5cff55926e3`，9责任路径；原index补丁与实际提交逐字相等。没有夹带当时尚未编译的 core 中间稿。
- 核心修复候选：完整出版／人口／Session 证明移至 Q 前，严格组合实际续租后缀，只有本次真实 writer 的 Written 结果可产生有限私有证明；Q 后核当前源、真实时钟和未到期 fence，仍要求 S=Q+1。通用读取及已有 S 的固定人口恢复不变；无新依赖、协议字段或跳过开关。初次编译2错误保留，最小修正后 check02通过；独立源审发现并修复旧clock读取顺序回归，另纠正首测把纯心跳当归档的错误断言，遵守FR-89 omitted覆盖。
- 最终冻结源码实际验证：新增四个 `bn_own_q_` 全PASS（428.32秒测试／680.58秒含编译）；既有AfterTag原批准恢复1/1（216.84秒），私有Q／已有S／缺marker恢复1/1（342.85秒）；workspace/all-targets Clippy `-D warnings` 通过。各批前后源码一致，包含新增未跟踪模块；独立源审7路径哈希一致、唯一finding已关闭。新App实机、B2旧UUID负例、完整workspace及真人P7仍待。

## 2026-10-19 按纠偏主路径实施至真实验收与发布

本轮续作（2026-10-20）：Bn 出版、完整任务复核、只读禁止诊断及同画布入口已通过本节目标回归；release profile 原3秒／15秒并发通过，debug超时保留。HTTP2项、技能与工具655项、前端226项与类型、native109个不同用例分别通过，workspace/native Clippy与格式检查通过。功能实现已提交 main `37e1d43`；0.1.0候选版本已提交 `422be45`，实际App已生成但技能归档发现旧署名白名单错误，已修待提交并重建；B2旧UUID负例、冻结后workspace完整回归、真人P7十任务及正式发行仍未完成。

实施顺序以 `docs/2026-10-19-mvp-path-deviation.md` 为准，不再用独立表单或预览包替代闭环。继续沿已有核心／writer／原请求身份；不新增依赖，不改变门禁，不使用虚构任务和人工批准。

- [ ] 第一竖切片：已有真实任务／Hold／证据／事件通过固定只读接口自动进入同一画布；固定坐标叠加状态，模块→任务→运行时间线，真实关注最多3项；点击问题直接打开既有原件答复／评审，不搬运ID。验证：真实daemon记录、WebSocket增量／断线水位、切项目隔离、快层不移节点、已回答不重复出现。
  - 代码已提交 main `a57f9ae837840ff0399f74bb4a66a11ce8df9f50`（49责任路径），自动验证通过、实机验收仍在进行：真实prefix的Hold及256条实际事件；只读WS变化通知；同版本画布叠加与Z0→Z1→Z2；直接打开已有Ask/Review原件。前端178/178与类型0错误0警告，相关Rust25/25，workspace/native Clippy与fmt通过。日志在 `.release/validation/2026-10-19-corrected-mvp/`；frontend-01/首次全测失败保留，第二全测通过。仍缺FR42上次查看时间窗／完整告警种类等，不称整条M1完成。
  - a57候选实际构建111.96秒、全部受控来源不变、arm64与ad-hoc完整性通过。实机自动载入真实夹具任务；使用同包CLI新增GUI-LIVE-1后无需刷新，任务1→2、seq20→26。发现旧工具占据首屏及目录HEAD与权威main不同导致永久版本不匹配，正在窄修主次布局和独立authorityRevision；不把该次GUI计为全通过。已正常退出自有App PID31403，并核实际daemon PID32389后正常stop，无残留（live-gui-01）。
  - 修复提交 `8ff3191555cccb03ee8a521d5feaae3dd3bfe370` 已构建并实机核：首屏地图优先，跨分支权威版本可进入任务；真实 claim 自动显示 running/Run/事件；Ask直接打开原件并回答后CLI收答、未决问题自动消失（live-gui-02）。仅自动GUI QA，不算真人P7。唯一App72671已退出，原Run正常release，拥有的daemon73649正常stop并核无残留。
- [ ] 技能入口：按已批准P7提供原生技能及本机安装指引，绑定实际CLI能力；接入现有Claude Code/Codex会话，不替用户新建付费模型调用。缺失propose／插话明确补齐后才称完整。
  - 六项技能源码／手动安装与checker已落；12项检查器测试及实际随包CLI帮助检查通过其可用部分，readiness按预期仅报propose缺口（exit1），init提示实际CLI回归通过。双harness实际加载仍待；源码同在a57提交，不称正式技能包。Claude实际plugin validate --strict通过；初次缺author警告保留，补静态署名后12项checker回归通过。
  - 原生 propose 技能已对齐实际公开命令、完整输入与原UUID重试；12项checker及技能格式检查通过，真实CLI帮助检查只因Bn工作流未完成而按预期exit1（skills-current-cli-02）。本次只提交Propose相关三路径与双语说明，消息前导契约待Delivery代码一起提交。
- [ ] 变更闭环：公开propose、审阅精确差异、后续基线出版与任务需复核，保持实际人口／来源证明与最终CAS；不得移除Bn硬阻冒充完成。
  - 公开 `towermd propose <json> --request-id <UUID>` 与固定 `/proposals` 已验证并提交 main `c001d8dd033f6067b135865221b05f3c3557b0cb`；真实HTTP2/2、CLI2/2，workspace/all-targets Clippy及fmt通过（propose-*-01）。沿用唯一writer、有限原件/完整Run授权，重复原UUID不重写，历史回执不授当前资格；不出版或批准Bn。后续基线仍保持原门禁。
- [ ] 人工干预：App按范围插话／暂停／恢复，任意CLI检查点投递真实消息；接问答和评审首答回执，禁止自动放行。
  - 消息投递候选已通过核心14/14、HTTP3/3、stdout4/4、实际CLI5/5与严格前导解析器；workspace Clippy03通过，尚未提交。旧CLI三项来源／快照前置失败保留，按真实snapshot与固定原UUID／全部原输出有限重试后CLI02通过；未降低原预算、禁快照或放松来源。native编译及既有问答／finish／到期／集成消费者回归正在继续。
  - 首读来源竞争补丁实际通过core7/7、既有HTTP6/6、loopback5/5、公开Propose实际CLI及输入检查3/3、workspace/all-targets Clippy（delivery-read-retry-*）。并发3秒断言仍失败；同profile旧fafc也失败，stdout/stderr=0，实际sample证明inbox等后台producer锁，Git trace窗口457次启动不全属于inbox。保留全部失败，不能计并发验收通过。
  - Control已合入工作区但未提交。暂停恢复旧产品真实RED在第一次实际rebuild丢失项目Pause（control-pause-rebuild-red-01）；新恢复补丁正在跑完整8项真实producer。初次check缺测试Catalog import已补，check02通过。UI/native联动已接入源码，尚未类型/编译/实机验收。
  - Control/Delivery源码本轮自动验证收口：前端类型04零错误警告、完整192/192；native控制8/8与观察偏好3/3；双端workspace/native全目标Clippy通过。核心8个Control用例已逐一真实通过（完整green02的7项＋repair-green02的1项，不能冒称同一整批8绿）；真实HTTP1/1、CLI角色边界1/1；maintenance-hint原实现实际RED、修补后2/2，原并发3s/15s验收06通过（96.85s总执行）。技能13项测试与真实help满足，checker仍仅因Bn未完成预期exit1。
  - 修补记录：增量Task claim补齐已证原件索引，旧5项RED保留；receipt重试的新增序号实测仅lease.expired，改精确禁止新人工控制并核原Pause原件及暂停保持；headless happy-dom缺OPTION :checked仅在该测试select补真实selected查询，必须继续实机核真实模块选择；App pause空struct拒绝页面伪造字段，native旧负例RED及新8项GREEN保留。UI新样式/控制操作尚未GUI验证，整个M1/P7仍未完成。
- [ ] 整体真实验收：随包CLI与技能、两个harness、三个工作树十个任务，真实App回答／批准／恢复；自动化与真人事实分别记录。必要真人动作等具体App就绪后交给用户，不伪造身份映射。
  - 已实现独立三树／十卡准备与只读材料收集工具并提交 main `0c70d56`；acceptance-tools-01 21/21通过（含真实临时Git/Python种子）。全部十卡初值not_run，不造B0／Task／人工输入；收集器核实际Run/fences、finish/merge/main/trailers/log/human来源，缺失为incomplete。publisher身份接线、实际双harness/十卡与真人验收仍未完成。
- [ ] 发布：冻结源码和tag、双语增量日志、Apple Silicon／Intel未签名包与技能包；核所有worktree／branch／stash范围，实际上传与下载回读后才称完成，再清理已核本地分支。

- 发布器已提交 main `af318a0`，接固定原始身份映射及严格C→R源码核对；publisher-raw-tests-01 223/223，完整publisher-release-tests-03 552/552。两轮旧build夹具未满足技能manifest／core path锁前置的失败保留，补齐真实临时Git夹具后全过，生产门禁未放松。尚无真人P7、冻结tag或实际上传，不能据此称发布完成。

- 发布准备的技能 manifest 版本同步已修复并提交 main `2df5ffe`：prepare 不再遗漏技能版本，失败时精确回滚并保留并发编辑；release-plugin-version-01 84/84通过。没有执行版本轮转或发布。

- 当前续作（未提交）：Bn完整出版／任务复核、同一画布提案与复核入口、P7实际producer采集映射已进入工作区。真实Bn首轮发现B1清单仍写B0身份，修补后已到实际B1提交／标签；第二轮在Record阶段误将自己目标判为已存在，正验证仅限精确VerifiedPublication的内部路径，公开新提案目标门禁保持不变。失败日志bn-publication-tests-01、bn-publication-real-02保留，不算完成。
- Bn真实正向bn-publication-real-03已通过1/1（433.75秒测试／446.61秒命令）：实际B1 commit/tag、B0/B1独立清单身份、完整S人口含提案后新增Task、领取/check/finish复核阻塞、原请求幂等均保留断言；同时实际证明公开分析仍拒绝已存在目标、仅精确已核自身出版可内部继续，错parent/target/hash或缺当前tag拒绝。故障矩阵和TaskRecheck仍在运行，未提交、未GUI，不称全链通过。
- 新前端类型检查bn-frontend-check-01通过；完整测试220/221，唯一失败为首屏测试在异步数据到达前计数，正在修正精确等待而保留三项／更多／原binding断言。该批遇失败即停，后续native／采集测试尚未执行；不以编译候选或旧通过替代本轮验收。

- 验收采集映射已实际通过 acceptance-live-dto-tests-01 的277/277：按真实 task.recheck_resolved、proposal.created→精确B1批准/出版，以及 human.prompt→原投递→下一status消息前导核对；跨任务复核必须绑定T08同一实际B1。全部为机制测试，真人P7十卡尚未执行；该独立改动提交号见 acceptance-dto-source-commit-01.json。

- 新前端最终bn-frontend-check-02零错误警告、bn-frontend-sync-tests-01（同版本失败恢复等）通过，完整bn-frontend-tests-02为226/226。仍未新包GUI；表单DOM通过不充真人事实。
- 单次history内精确S原件＋Q2人口计划复用的原样真实Bn正向bn-scope-reuse-real-01通过1/1（346.04秒测试，364.66秒命令；原先433.75秒测试）。只记本次实测差异，不声称通用性能保证。旧faults-03六项已实际显示通过，但整批为主动中断、最后私有Q2用例未完成，收据bn-producers-03-interrupted.json及原日志保留；新完整矩阵仍待。
- Forbidden12和复用负例已接入，bn-forbidden-check-01全workspace/all-targets编译通过；8项真实producer测试正在运行。仅首次check／finish拒绝记录不可变诊断，旧accepted finish UUID走原纯读路径；禁止卡只读、不造G3/G5能力。仍未提交或GUI。

- 本轮续作收口计划（仍未整体完成）：
  - [x] 在实际交付release profile验证真实双模块并行：原 status/claim 3秒及第二次 admission/worker 共享15秒均通过；debug超时记录保留，不扩大时限或宣称debug修复。
  - [ ] 完成 Bn 故障矩阵、TaskRecheck、HTTP 与同画布提案关注回归，收口当前 core/native/UI 源码并精确提交。
  - [ ] 对齐六技能、安装说明及0.1.0版本，构建唯一候选 App；先自行验证正常 App B1／复核／干预，避免用手动 snapshot 掩盖正常流程。
  - [ ] App 与实际项目就绪后安排原生双 harness／三树／十任务及真人操作；最终 R 冻结打包、真实上传与回读，再核本地分支和工作树清理。
- 已验证更新：Forbidden 诊断／快照回归 forbidden-core-tests-02 为10/10；修复仅避免无真实Hold时制造空Hold映射，未放松严格快照一致性。真实并行04已通过初始化、3秒交互与第二次接纳，但第二个worker未在原共享15秒内启动，仍为失败。源码检查发现新增诊断producer后立即重复完整G2，正做有限同调用复用，后续启动屏障不动。native复核02为6/6、review01为13/13；release/prepare/collect全工具 corrected-release-tools-all-01 为611/611。这些均不算新包GUI或真人P7。

- 发行包 CLI 重签校验独立切片：release-cli-signing-assets-tests-02 实际132/132通过；两份真实历史ARM产物对比通过 cli-signing-real-arm-candidate-01，target/staged原字节仍须相同，仅容许末尾签名及三个严格派生尺寸字段差异，manifest记实际包内CLI哈希。首轮131/132仅截断负例先触发segment bounds而非EOF，按真实拒绝顺序修正该错误文本断言、未改生产门禁。真实Intel静态校验、新包GUI和正式发布尚未执行；本切片已提交 main `021f493cb98f4523f2e897bc38214af9fa15b53a`，详见 release-cli-signing-source-commit-01.json。

- 本轮实际结果补记：bn-plan-reuse-negative-01 为1/1；Bn恢复selector缺口用原private-Q2用例实际RED（缺标记却退出待续作），仅修完整性判定后 bn-pending-markers-green-01 为1/1，保原S与精确人口。finish同调用报告复用 finish-recorded-observation-tests-02 为5/5；01因stable不支持测试参数fail-fast而未执行业务用例，不能计产品RED。真实原并发05仍失败；物理采样确认实际started追加后OwnFinishAppend回退完整历史，下一步先跑既有零重建回归再定位decline，保留成功GateEvidence（FR-56）及原3秒/15秒。basic TaskRecheck2项正在运行，余下恢复/HTTP/关注回归和GUI仍未完成。

- TaskRecheck基本2项首批已作为诊断主动中断，未通过也不计产品断言RED。真实采样表明human S接受后，outcome内对同一S/Q重复完整证明；原日志/采样/精确自有进程组清理留于 bn-task-recheck-basics-01-interrupted.json（已核无残留）。正在做仅单次history内完整原件相等的有限去重，外部来源检查与跨调用重证保留，之后重跑基本与恢复用例。
- 第一层去重后 `bn-task-recheck-ready-02` 实际1/1通过（467.36秒测试／486.48秒命令，源码前后相同）：真实首次解除、原human事件／Q-S绑定、同UUID不重复、改正文拒绝、解除后实际claim均通过。针对仍存在的同history重复getter证明，已采用私有sealed历史结果（不进入Session／不序列化／每次新history独立验证）；独立源审未见阻断问题，删除一处必被collect_originals重复执行的binding读，当前全workspace/all-targets Clippy07通过。新sealed实际running／finiteQ与GUI仍待，不把之前ready结果冒称在新源码重跑。
- 并发物理诊断收口：phase02真实beta install=false后fallback约2.83秒；combined03未达beta因snapshot返回裸head-changed，不能把该错误加prewrite白名单（实际route在返回同错误前可修复Gate/合并/续租）；combined04/05均beta install=true仍超原15秒。05实际发件箱证明interrupted原因是测试失败后的 `daemon stopped before verification` 清理，不能算独立产品执行器异常。主要新观察为check/history及启动前耗时，已停止盲追decline；所有临时追踪都已撤销并核原finish.rs hash，保原失败与物理事件。当前批次 `bn-task-recheck-sealed-subjects-01` 验running完整expiry与finiteQ伪造／真实tag删除，未计通过。
- `bn-task-recheck-sealed-subjects-01` 已实际输出finite-Q一项通过（未知字段／请求／来源伪造、完整原件正控、同eid自洽subject拒绝、同history真实B1 tag删除拒绝及恢复一致）；running项仍进行中，整批未完成，不计2/2。后续待采用的finish窄修只去除完整Event重复证明并复用既有同调用FinishReadScope，不移除门禁结果或改变原3秒／15秒。
- `bn-task-recheck-sealed-subjects-01` 最终实际2/2通过（887.14秒测试／905.33秒命令，源码前后不变）：running续租后旧binding拒绝、新精确expiry经真实Q压缩仍可复核、完整expiry伪造拒绝、真实hub重建后原回执仅HistoricalOnly；finite-Q及真实tag删除负例也通过。现已采用finish同调用历史复用小补丁与三行完整Event去重、同步技能/任务说明（不要求手工Q1，不把暂停新claim当终止Run）；这些新改动仍在验证，未提交。
- 新finish同调用复用已通过 `finish-history-reuse-guards-01` 5/5（159.98秒测试／178.62秒命令，源码不变）：原请求不重复追加，真实业务扩展失效，实际Git对象缺失拒绝、bare B1标签不能授权，同一真实诊断在confirmed/tail重复仍只有精确原件。原并发 `finish-concurrent-history-reuse-01` 未进入两worker预算断言，因快照maintenance-prefix准备拒绝停止；已精确增补fixture有限协调并保原15秒/3requests/实际maincoverage，裸headchanged及不确定/已落盘错误仍拒绝。当前parser01→原并发02顺序执行，未计通过。

- 最新实际结果：snapshot-prewrite-fixture-parser-01 为1/1；原 finish-concurrent-history-reuse-02 仍在第二 worker 共享15秒失败。bn-task-recheck-durability-01 已实际输出4项通过（native完整绑定、原UUID确认不确定恢复、普通快照丢弃有限证明／真实重建不造租约、pending fsync原Q重试）；最后“无后续快照的原回执恢复”仍在运行，整批未计通过。
- Passed own-install checkpoint02 仍为未采用私有候选：复审发现 checkpoint01 会清掉别的 pending Task 归档回执，已加 needs_reconciliation 排除并扩真实归档原件恢复与 post-confirmed main 漂移测试；先实际验证后才合入，不将源审发现记作运行时红绿。

- `bn-task-recheck-durability-01` 最终5/5通过（1735.91秒测试／1735.98秒命令，冻结来源前后相同）：无后续快照的原回执恢复也经真实hub重建与后续claim验证。
- Passed own-install checkpoint02 通过第二次独立源审后已按5个精确base/结果哈希采用并fmt，新增pending Task排除及真实归档确认／main漂移测试。仍待实际编译与运行，不声称原15秒回归已修好。

- Passed own-install 实际验收：Clippy09 全workspace/all-targets通过；task-check-own-install-boundaries-01 为2/2（43.80秒测试／62.86秒命令），真实post-confirmed main漂移拒绝与pending归档原件确认恢复均通过；regressions-01为5/5（175.17秒测试／175.28秒命令），保真实Human Hold、旧Forbidden audit／resolved、零完整重建及same-eid body/metadata/stale拒绝／真实renew有限重放。来源前后相同。原 finish-concurrent-task-check-install-01 正在运行，尚不计通过。

- 原 `finish-concurrent-task-check-install-01` 实际仍FAIL：115.40秒命令／100.15秒测试，第二worker未在原共享15秒就绪；源未变，失败完整保留。重新规划先测交付profile：本轮之前均debug未优化，实际App为release；fixture由Cargo `CARGO_BIN_EXE_towermd` 启CLI，daemon沿该CLI current_exe启动，独立runtime不会接旧debug服务。因此下一步用release保持原同一测试／全部3秒15秒断言，不扩大预算，也不把release通过冒称debug性能修复；release若同样失败再启封私有阶段诊断，停止盲叠缓存。

- `bn-task-recheck-later-subjects-01` 运行约26分钟后按profile重规划主动中断；实际5项已完整ok（4项Proposal attention＋旧Ready点击不能迁移新Run），B2旧UUID项尚未结束，整批不得记6/6或产品断言失败。已核自有PGID34407并SIGINT回收无残留，独立中断收据与真实B2提案来源采样保留。正在无TEMP的release profile原样运行并发验收，之后在同profile重跑未完成B2负例；未扩大任何原预算。

- `finish-concurrent-release-profile-01` 实际PASS（2/2含一个stdout parser辅助测试，业务双worker1项；103.64秒测试／185.51秒命令，含首次release编译80秒）。同源码、无TEMP诊断、CARGO_BIN_EXE实际target/release CLI及独立daemon；原3秒／共享15秒／两个真实执行屏障／不同tree和报告均通过。debug失败保留，不能写“debug性能已修复”。不再叠加性能候选；当前只重跑未完B2负例、HTTP入口和技能／发行工具，然后收口App。

- 交付顺序调整：release B2 单项已执行约14分钟仍未结束，没有断言失败；为先完成用户可操作的正常 B1/TaskRecheck App 与真人P7交接，精确回收自有PGID44518无残留，保留中断收据，**B2负例仍是正式发布前必须补完的门禁**，不记通过或删除。候选包可以先作QA，不冒充已完成首版；正常B1路径已有多批实际证明。当前验证公开HTTP2项，再技能／版本／候选构建；正式全量与B2在冻结候选上收尾。

- `bn-task-recheck-http-release-01` 实际2/2通过（441.81秒测试／479.75秒命令，冻结来源不变）：真实B1 ready复核HTTP有限入口、完整原binding／原UUID及解除后实际CLI claim均通过；读取缺失项目不注册／创建也通过。当前候选收口脚本按序执行 init提示、技能及发行工具、实际CLI只读契约、完整前端、完整native与质量检查；任何失败即停，不计未运行项。

- 候选收口：init提示1/1、技能／验收／发行整合655/655、实际CLI技能契约ready=true、前端226/226及类型零错误警告。native首轮82/83仅旧DTO默认proposals集合期望失败，保旧输入兼容并补expected空集合后02单元83/83、CLI适配7/7、项目适配16/16及workflow2/3通过；剩余真实workflow在已完成编辑／B0／G1/G4后因精确snapshot maintenance-prefix准备拒绝停止。仅该fixture在原10秒重试环处理精确prepare拒绝（不接裸head-change／uncertain），03真实同项1/1通过（331.29秒测试／334.06秒命令），含后续真实Appgate恢复／旧Run失效及原快照/人工事实断言。不同用例109项已分别通过，不冒称02单一整批全绿，不重复统计隔离子进程同名测试。最终native Clippy、workspace/native fmt均通过。
- 提交前独立源审：核心36路径＋UI/native/skills/README40路径，逐SHA均匹配；Root审其余14路径的读模型、关注投影、CLI夹具与HTTP负例、文档，本轮新增native workflow仅已读精确测试hunk。未发现确定阻断或无关夹带；B2旧UUID未完成及整体P7仍明确留作正式发布门禁。

- 功能候选已精确提交 main `37e1d43857eb5b30b7ae252446ff0d83d60cdc99`，tree `bee0ad9709e4a796949a6c99db49bd6ba8ef8767`：91责任路径与实际index补丁逐字一致，原10个未跟踪文件字节／链接未变，受控工作树及index干净（candidate-feature-source-commit-01）。这不是正式发布完成。
- 按既有prepare工具完成0.1.0候选版本准备（执行日2026-10-20），根workspace、Tauri、package与技能manifest同步；两个Cargo.lock只更新本地Tower包版本，第三方锁字节未变，pnpm锁未变化。0.1.0双workspace/native全目标Clippy与frozen版本检查均通过。仍未创建tag／上传；候选版本提交后实际构建、GUI、B2／全workspace／P7门禁继续。

- 0.1.0候选版本已精确提交 main `422be454394367796ed0b4da81d54ad735213b6b`，tree `3f39710e1a06ba2022f7b40eeb07c1b0b7946a78`，11路径只有版本语义／对应锁及审计说明变化；没有tag或正式发布。干净共享clone中离线依赖安装缺本机tarball而停止，未下载；复制现有同锁依赖逐字核6401文件／292链接，31个生成shim/元数据路径重定位，candidate Git保持干净。
- 首次候选构建所有实际命令exit0，App／CLI均已生成；归档技能时checkSkills旧白名单遗漏现有author而失败，故整个候选构建无PASS收据，不以已有App冒称完整包。真实形状fixture复现 author-red-01 为1项断言RED；仅对齐现有checker的可选严格author.name，补5个无效author与hook负例，完整技能／验收／发行工具green-01 为661/661（25.87秒，冻结源不变）。下一候选将包含这一打包修补后重新构建；旧失败目录完整保留，没有启动旧App。

资源约束：Root唯一重型批次，nice10、jobs2／test-threads2；子Agent只编辑责任文件和只读分析，不自启Rust／Node／GUI。保留用户当前已打开的App，不重复启动调试实例。每个可独立验证改动精确提交；计划条目只在实际通过后勾选。

## 2026-10-19 原始 spec 与实际交付偏离复核

- [x] 对照原始 v0.5（906d462）与当前 §18 M1，分别只读核App/画布和技能/CLI；未运行构建、测试或GUI，未改变产品代码。
- [x] 核实已交付fafc27e的真实主路缺口：技能未交付，画布无实时快层及Z1/Z2，真实关注清单缺失，问答仍搬运Hold ID，插话与后续Bn未接通；不把M2/M3后置功能列为M1欠账。
- [x] 给用户画原始主路径与实际实施路径，标出撤回的独立画布／鉴权路线、重复画布整合债及多轮后端／打包投入；不编造浪费工时或完成百分比。
- [x] 按用户要求整理为单图 Markdown：`docs/2026-10-19-mvp-path-deviation.md`，包含主路径、实际路径、返工、当前断点与纠偏目标；明确工时统计和 M1 范围边界，保留源码依据。

回顾：此前把“单项真实实现／测试／预览包”连续当作进展重点，未以技能→真实任务→态势画布→放行→合并这条用户路径组织交付。基础和B0已有真实实现，但完整驾驶舱未完成；本次复核不代表产品整改已完成。

## 2026-10-19 可体验 App 与真实任务进度（继续）

- [x] 已把919f预览交到下载目录，说明与来源记录提交 `fa910c408448decfd0996cf9492129e4b9421506`；新包GUI工具不可定位，保留未验，不把演示当真实任务进度。
- [x] 真实任务列表已提交 main `f291050f0b66ad2d09956d36392bb14554092034`，tree `b6a9eb1453d63665ff82dfb25d33ac0d994611c1`，27责任路径、314个index来源逐字相等，原12个WIP路径保留（790／791）。冷／暖完整只读prepare、来源闭合、手动三项／展开；不续租、ack或恢复执行。
- [x] 含进度列表App已按actual f291050构建792（147.45秒，314源不变）；793同源CLI证明与794静态包检查通过，797交到下载目录 `Tower.md-preview-f291050`，ZIP SHA256 `166968cbc47aa13e28539ada815877586c9bfa102d30682aecd350a42c5056a6`。用户可双击App→查看演示，或打开项目→查看项目画布→任务进度→读取任务进度；附中文说明及SOURCE。仅本机开发预览，非正式版。
- [x] 760核心3／755冷读3／761真实Finish→Merge及快照3／787 HTTP3／779 native4、775前端166通过，共182个不同相关用例；776类型0错误0警告、777构建、788 workspace及783 native全目标Clippy、789／781格式通过。762独立源审无新增确定问题；05仅共用测试fixture，787实测3/3，不改业务／断言／时限。758选0、763无lib target、766依赖路径和782重复模块失败保留，不计通过。
- [ ] 新包GUI实机点击仍未验：精确路径两次 `cgWindowNotFound`，不推断锁屏。仅关闭本轮实际PID13078并确认已退出，未启动daemon，未动用户其他进程（796）。不以自动化或旧包GUI冒充本包视觉验收。
- [x] Propose核心已提交 main `7eac5367e13008e974506c6725fb5cde68609541`，tree `d9da41a22b419a78f720295f1aa71ee07640cb64`，17责任路径、318源与800逐字相等、12WIP保留（810／811）。801实际兼容26、802出版13、803只读3、805native4共46项及806／807双端Clippy、808fmt通过。producer13为741在729实际通过，15责任核心／测试来源在800不变（只加actual App读模块与日志），800等价回执说明，不冒称13已重跑800。公共CLI／HTTP与Bn仍待，保持Bn硬阻。
- [x] Task5候选在actual7eac前置组合固定60分钟deadline、真实私有执行清理证明与到期历史回执。818／824实际全目标check通过；819 cleanup 2/2、821 verifier 17/17（嵌套子测试不重复计数）。820维护14项仅2通过，保留失败；窄修实际TaskAdd层级、诊断eid与claim.data.run读取后，825原维护14/14、826私有恢复4/4通过，未改原断言或时限。
- [x] 重启后旧Running无法证明退出时，新增cleanup_unknown诊断和材料保留；828旧实现实际断言RED。829进一步发现到期task.released误放terminal-only actual造成重读不一致；窄修分离完整expiry_actual与原完成/中断actual。833同真实runner用例1/1通过（56.19秒），包含完整expiry Event原件、前后读一致、无伪终态与实际私有owner收尾；原14／4测试源码未变。835同最终测试旧产品真实RED（cleanup_unknown为空），833最终生产GREEN；两者保完整收据。
- [x] 836完整finish路由9项实际8通过／1失败；唯一旧queued测试还把到期Event放actual。842只迁移该测试角色、完整原件相等目标保留，843实际1/1通过；其余8项测试正文／全部生产与836逐字相同。844后台Job4/4通过。
- [x] CLI845失败保留、阶段未知；855加track_caller证明在后段再次claim失败。865只补真实快照前置，保持原共享15秒／三次及两个90秒期限，866随后发现完整历史读受内存缓存影响。871把已核完整expiry_actual放进核心只读视图并移除缓存继承；872实际CLI 1/1、873共享fixture消费者1/1通过，生产与最终877相等。
- [x] 846既有finish26/26、847 cleanup2/2通过；最终877的880关键维护／双重真实重建3/3、881私有历史1/1、882实际路由2/2及878全workspace/all-targets Clippy、879 fmt通过。883明确区分最终实测、生产等价和此前实测，不重复计算嵌套测试；原失败全部保留。
- [x] Task5已精确提交 main `fafc27e353796421558812a9ccdfab1d3084b48c`，tree `819cd38e00526546f0b42c3ede6c9540516f8b87`，30责任路径、324源码与877逐字相等，原12WIP保留、暂存区空（884／885）。815／816是准备失败，832遗漏共享target而主动中断，不算产品红绿；850仅回收自有4份重复target约2.3GB，冻结源码完整。
- [x] actual fafc预览888单批构建通过（109.84秒、324源码不变）；892仅Mach-O三个签名尺寸字段规范化后CLI代码相等，893 arm64／ad-hoc完整性与非仓库CLI探针通过。894交下载目录 `Tower.md-preview-fafc27e`；ZIP SHA256 `5ef51159e10eddbbf91f1960ad8d364d037c78fe64cd855aedda399e87ae0245`，App复制／ZIP字节逐项相等。887下载超时及889离线缺包失败保留；890同锁的完整私有依赖副本逐项核6778文件／链接，仅重定位生成元数据和6个shim，未共享可变node_modules。
- [x] 895在实际下载包实机操作成功：演示6节点／2模块、T-01详情、文件夹选择与取消、实际Root固定fafc目录画布7节点、任务进度入口展开。未点击真实任务读取／草稿保存／人工批准，不冒称整条工作流GUI完成；896用App退出回收唯一PID2871，实际无构建或App残留，syspolicyd抽样0%。897重核App／ZIP不变并更新中文说明和来源元数据；旧包796不可定位记录保留，本包有独立新证据。
- [ ] Bn仍硬阻，候选仅有精确提案subject与影响分析；缺少完整出版S人口崩溃恢复证明，不称可批准／出版。公共Propose adapter、P3投递、完整spec/P7、双架构正式发行及最终分支清理仍待。

回顾：产品改动只在独立源码副本；Root维护唯一重型批次、jobs2／test-threads2／nice10。实际本机预览并非完整spec发行；用户先体验、后台开发继续。

## 2026-10-19 Task4 已提交，Task5 与下一轮 App 继续

- [x] Task4 已实际提交 main `919f4adcdd1d3566440911e71e70e9c45b2193e9`，tree `758fb95f5eda50b830b8192b4aada7e437f81145`，26 路径；全部302冻结文件与实际提交相等，原12路径完整保留，提交后暂存区为空（722回执）。
- [x] 最终720合并矩阵23/23通过（675.84秒命令／664.94秒测试）；692核心361/361与693–697既有核心、人工评审、HTTP、并发和CLI回归通过。676 trailers CLI4/4使用674→687→715生产等价证据，不冒称最终输入重跑；718fmt／719全workspace/all-targets Clippy通过。
- [x] 原698／709故障及actual427控制材料保留；仅测试调度域成对时钟前置修正后707／716通过，717另证真实快照退避。生产代码未因这两个夹具变化，不宣称原时延修复。
- [x] 723结束盘点：实际exe `towermd-app`、Cargo／Rust及项目测试／daemon均无残留；syspolicyd抽样0%，未终止任何进程。本轮无GUI启动。
- [ ] Task5以actual919f为代码前置，core与薄适配器在独立源码副本实施固定60分钟deadline、后台续租、精确cancel/drain和历史合并冲突保护；Root独占串行工具验证，尚无通过／交付结论。
- [ ] 真实Propose producer在672仍12/13，裸Io丢原UUID的窄修已封存但未验证；CLI/HTTP接线与公共inbox/prelude均仍是未编译候选。保持Bn硬阻，不用源码候选冒充可用功能。
- [x] 919f开发预览已构建并交到下载目录 Tower.md-preview-919f4ad（735，ZIP SHA256 4f444af94105ffbe4f44a9ea4a976bfb7d4779c201c01f572f7ab8f5c1871c75）；726构建151.86秒、302源不变，733架构/adhoc/CLI probes与732 Mach-O仅签名尺寸字段等价通过。730朴素去签名比较准备失败保留，非产品失败/通过。734 CUA窗口不可定位，新包实机未验；本轮唯一实例51500已关闭，未启动daemon。目录含App、ZIP、来源/校验和与中文说明。
- [ ] 用户要用App查看真实任务进度，先实施窄只读列表/手动刷新；当前UI没有该总览，不能用演示或目录图冒充。完整spec/P7/两架构正式发布继续，预览不替代正式发行。
- [ ] 最终全部完成后核对全worktree／branch／stash，合入main并清理其他本地分支；当前不删除未验证内容，不动远端分支。

回顾：本节取代下方历史记录中“Task4未提交／Task5未实施”的当前状态；历史失败与当时判断保留。Task4及此前Ask/App代码没有自动进入旧055d预览，新包完成前仍不能称旧包包含新功能。

## 2026-10-19 Proposal 载荷基础已合入 main，Task4 继续验证

- [x] 精确提交 `427e389d7ca4d1d447d7c016c881d714626605a5`，tree `360978c7a58dea6d898a5d6927556448ca00967a`：原五路径载荷基础及双语未发布日志，保持全部 Bn 硬阻，不宣称真实 Propose 已交付。
- [x] 冻结606：607扫描17与提案6、609出版消费者13，合计36不同用例，显式两线程；610全workspace/all-targets Clippy及611 fmt通过。613全部292个index blob与606相等，原17文件逐字保留，无新分支或工作树。
- [x] Task4旧组合598在599全workspace测试目标编译通过，601真实core14/14通过（两线程，385.40秒测试）；600默认批次已按资源要求中断，不能计默认并发通过。独立审查发现failure到期阻塞、legacy外层未核及后台expiry饥饿，已在独立源码修正，旧14通过不证明这些修复。
- [ ] 候选617以实际main427为前置，精确三方合并保留Proposal publication校验，并接core06、adapter02及03-expiry；618格式化、619全目标编译通过，620真实core20/20通过（两线程，619.54秒测试）。新增HEAD竞态闭合07已在625/626全目标编译通过；627三项lint失败仅用08机械修，631与632全目标Clippy通过，633新增两unit及受影响两FrozenG4 integration全部4/4通过。634原双worker回归在新增beta任务的原3秒限制失败，不放宽断言；635实际main427对应源码同条件对照1/1通过（68.42秒测试）；09已冻结两路径，在全first-parent链按实际Run ID保守预筛，637在638真实双worker1/1通过（73.67秒测试/87.36秒命令），原3秒/共享15秒未改；639真实CLI为2/4：伪尾注、严格HTTP通过；positive已核actual canonical/G3/恰好一次执行，随后Status读取last verified prefix的竞争断言失败；高风险在原G3缺省G4键误当false处失败，未到后续G4。保原125.20秒命令失败，按真实接口精确修夹具，不改变产品Status快读/原期限。640在637核心23/23通过（664.85秒测试/675.52秒命令）。10-02新增完整confirmed行fsync不确定的有限原件修复，642/647绿色及644/645旧09负控已独立封存；646仅编译失败（fixture git名歧义），652 canonical实际RED但Gate错选既有记录不算产品RED；657按唯一completed绑定修夹具后两项均在预期补确认断言RED，658最终656两项2/2 GREEN（51.06秒测试），正负测试逐字相同。659最终全workspace/all-targets Clippy通过。660最终CLI仍2/4（190.74秒测试/204.38秒命令）：positive实际canonical已确认，但共享90s纯Status一直旧verified prefix，后台三次prepare CAS反复来源变化；这是待修产品竞争，不降低100ms查询频率或放宽期限。高风险已到真实G4与trailers，新断言错取outer completed_eid，实际在evidence.review，另修夹具。660队列已自动停止，661–670未执行，不计通过。11-01只在完整prepare末核actual纯Renew后缀及同业务身份、完整Head重放，保原sources/三次和所有非Renew拒绝；674组合adapter05后675全workspace/alltargets check通过（10.91秒），676在674真实CLI4/4通过（197.19秒测试），原100ms/90s保持，正常后台投影与高风险完整G4已验。11-02八项真实fullprepare来源/业务/epoch负控已冻679/680，681仅测试helper编译失败，正负同一行解Box后冻结687/688；689旧产品真实3个预期SourceChanged RED、5个保护负例通过，690新产品8/8 GREEN（10.81秒测试），691全workspace/all-targets Clippy通过；692最终687全core-lib361/361已通过（1856.61秒命令，全部冻结输入未变），693既有finish23/23、recovery_inputs15/15、tasks31/31全部通过（657.67秒命令），694人工评审15/15（375.69秒命令）、695HTTP4/4（22.15秒命令）通过，696原3s/共享15s并发1/1通过（73.66秒命令），697既有CLI52/52通过（227.15秒命令），698 ticker单项FAIL（23.84秒命令，嵌套子测试同一项不算两个不同用例）：真实续租、越原TTL及shutdown-drain均成立，但registered A archive barrier未进入；队列已自动停止，699–702均未运行。704 actual427同单项1/1通过（31.53秒命令，15.56秒测试），606所有292路径逐hash等actual427；只读定位为新MainObserver warm后due=mono5000，而旧夹具在mono2000等待archive直至超时，后才跳33000，形成确定的注入时钟前置循环。独立复核认为是新注入调度的因果前置，非已证实产品续租失效；5s节流不是spec批准数值，也不将698改称原延迟通过。705/706只在cfg(test)旧I2真实main变更和a-armed后增加not-admitted(2000)/admitted(5000)断言并推进成对注入时钟，原B路径、Real5s等待/轮询/TTL/收尾全部逐字保留，生产代码未变；707真实单项1/1通过（32.01秒命令，15.36秒测试），独立签收5be85e6f...核全部其余文件及生产字节相等；708全目标Clippy通过（12.50秒命令）；709 offline单项FAIL（8.34秒命令、4.36秒测试）：HTTP原2秒与5/10/15/20分钟Run/fences续租均成立，36分钟最后actual release缺失；该夹具Unix为fake但monotonic仍real，713 actual427同项1/1通过（20.49秒命令、4.71秒测试）；两份只读分析均定位为Unix跨分钟而真实mono未到观察due，非已证实worker内TTL失败。714/715只配对该cfg(test)夹具同一Atomic elapsed mono，保原每个分钟、HTTP2s、真实release/installed-prefix且无Status触发断言，716真实单项1/1通过（24.87秒命令、7.84秒测试），原断言全保留；旧710–712均未执行，新717专门snapshot backoff隔离真实1/1通过（197.05秒命令，模拟3601个逐秒tick），718fmt通过（0.76秒）、719全目标Clippy通过（11.57秒），720完整23项main-proof矩阵正执行。687→715其余301路径与daemon生产prefix逐字等价，source等价回执已封存；不冒称361整批在715重跑。最终main提交仍待。有候选仍完整同证明，Task4未提交、Task5未实施。
- [ ] Propose producer独立候选03经审查发现历史reader未核实际载荷与影响，04已补不可变原稿/原source/当时Task人口的同一分析器重核，Root621待验证。负控622/628只将该validator回退03，629真实用例编译后在impact篡改阶段出现`unwrap_err got Ok(())`，证实旧来源缺口；没有把该失败算通过或称04已验。Propose尚未提交。671/672真实13项12/13，原source/impact篡改、不可变ref删除和双重建已过；唯一pending-fsync在assert原UUID失败（实际错误裸Io），native窄修只在私有ref后append边界Io→Uncertain，另准确区分complete confirmed行，不弱化断言；新候选待专项与质量。
- [ ] Propose CLI/HTTP最小薄接线在core组合683上独立source-only准备（固定四字段wire、唯一writer、原UUID优先），不实施App/Bn，不跑重工具；01已冻manifest7859e9f...，3生产/2测试路径，实际HTTP2与CLI2仅源码；待producer实际提交后重锚合并并由Root串行真验，当前不算交付。此并行准备只减少等待，不将候选当main前置。
- [ ] P3 Task3 公共CLI投递切片从实际427独立开展source-only实现：固定inbox/投递确认wire，真实收件范围与统一原件账本，stdout全部写入并flush后才ack；hub rebuild不续租、旧Run只归档。与未提交Task4／Propose无API前置，不提前实施Pause／App或Bn。01已冻，25责任路径逐base/hash核对并物化703（300路径）；13core、4stdout、3HTTP、实际CLI4+parser2均只是测试源码，尚未编译/运行。Native冻结独审报告87300db...未见新确定问题，原Io/最终source原UUID窄修已纳入。Root串行验证后再合入，当前不算交付。
- [ ] 全spec、App重打包、真实P7与正式发布继续；最终全部合入main并清理其他本地分支/worktree尚未执行。现阶段不删除含未验证内容的分支，不改远端分支。

回顾：604准备时误将归档的6份交接材料也当责任源码，成员断言在写入前拒绝；完整原件和失败记录保留，605只覆盖逐项核过的15份责任文件，未覆盖根README。准备失败不计产品红测。用户再次报告卡顿后仅回收核实归属的600进程组，602保资源检查；后续nice10、Root唯一重型批次、两个测试线程、不开GUI，未动用户其他App。

## 2026-10-19 Task4 实施计划（Task3 已真实提交）

资源复查：用户再次反馈卡顿后先中断默认14夹具批次600，仅终止核实归属的进程组且复查无残留；未动其他App。596早期组合与599当前04+adapter02全workspace/all-targets编译通过，均不等于行为完成。后续601显式两个测试线程、nice10，Root只运行一个重型批次，不开GUI，不宣称默认并发验收通过；602保资源检查。

基线为实际main `5bed9b893a799f580bd03bfe55e8efdc41885472`，P2 Task3代码提交`3a38eb3`已是祖先；此处不再用未提交候选代替前置。Task4/5只读重核计划见本机 `.release/development/p2-main-proof-plan-2026-10-19/plan.md`，其中旧Task3未提交段落由本记录取代；权威仍是仓库P2计划与共享恢复证明契约。

- [ ] core专注实现trailers有限producer、实际晚G4/终态G5耐久原件及正常main observer；复用唯一parser/check_g5/prove_merge/main_entry，保持所有source/HEAD/完整fences/deadline与最终CAS，不新增raw proof/自动完成通道。先固定薄适配签名。
- [ ] adapter专注CLI/HTTP薄接线与后台观察时序；只directory/task/run，由核心读取实际证据，不收actor/Evidence/任意proof。真实main观察在对应expiry之前，sole ticker不做Git/网络等待；不自动amend/merge/push。
- [ ] 真实低风险vertical与高风险App G4、amend/state-only rebase/两父接入、故障原件重试及三入口同proof验证；候选枚举不能仅从最新snapshot下界扫描。Root独占串行重型验证，每批冻结精确源码与未提交状态。
- [ ] 质量、相关回归及精确index输入核对通过后提交main，再按真实Task4提交前置实施Task5固定期限/续租/精准取消，不提前把纯决策测试算TTL完成。
- [x] 独立核验并提交Proposal原WIP最小基础切片427e389；当前36项真实证据见顶部，不混入Task4、不绕Bn硬闸、不借旧319作为通过证明。

本轮Agent仍source-only；不各自启动Cargo/Node/GUI，Root未创建新worktree/分支。其他分支待完整开发与发布范围核验后统一清理。


## 2026-10-19 Task3 后台 finish 已精确提交 main

- [x] 已提交 `3a38eb3a143aea407d7263a116b8c20a4c7dc14b`，tree `2f1e057d604f7a539291e13d380c2a88e0229daa`，22个路径：真实CLI后台队列/有界等待、同活动输入不重跑、原回执补确认、恢复与进程回收；修正恢复释放verifying/held旧Run，包含双语未发布日志与使用说明。
- [x] 最终冻结587，全部291路径实际index逐blob相等，591记录真实main提交及原17文件逐字保留。原Proposal四个修改文件、未跟踪payload与用户配置/文档未混入，暂存区为空。
- [x] 62个不同相关用例：554 core-lib22；566真实HTTP故障/通知6；567既有finish默认22/23，其中唯一旧held恢复fixture按FR93仅两状态更新后578专项1/1，合计覆盖23项（不冒称最终默认23整批重跑）；581默认Cargo两个target依次9/9+1/1；582既有running恢复1/1。最终Clippy588全workspace/all-targets与fmt589通过；590记录精确输入等价及唯一保持Result/错误不变的inspect_err lint修正。
- [x] 真实并行验收保原status/claim3秒和second admission+worker-ready共用15秒，两个真实verifier仍在同项目并行、release前已返回/运行。与其他独立项目夹具分target隔离；不认证九项目压力性能。注册恢复最终实测queued24.245秒完成、orphan7.649秒中断；90秒仅一次共享测试watchdog，旧15秒目标未达与所有失败原样保留。
- [x] 592再次盘点本轮结束进程；本轮未打开App图形窗口，验证实例由夹具回收，未执行全局kill或改安全服务。
- [ ] Task4 trailers/正常main合并认证与Task5固定期限/续租继续按实际Task3提交开展；Proposal Bn、其余CLI/技能/App/P7及正式发布尚未完成。不存在发行tag或资产上传。
- [ ] 用户要求的最终全部合入main与清理其他本地分支/worktree仍在收口计划；当前开发未全部完成，不删除保有未验证内容的旧分支。最终须重新盘点并核保护496及新范围。

回顾：575因测试引用crate-private legacy字段编译失败，578改核既有公开canonical序列化（false省略字段），未暴露新生产API。583因manual_inspect lint失败，仅将原样传错的map_err改inspect_err，业务结果/错误和同一Release标记副作用相同。旧编译、时限、夹具和质量失败都未覆盖或计为PASS；无新依赖、无新GUI验收或正式发布结论。


## 后续 CLI/技能的已知最小缺口

- [ ] 当前Taskcard.instructions.availability仍沿M0-2声称ask/finish不可用，已与main Ask和本次finish接线矛盾。后续CLI/prelude批次按实际已交付能力更新，临时规则/插话等真正未实现部分保留准确状态；不让Agent因已可用能力而错误停工。

## 2026-10-19 Task3 时延测试隔离（继续）

- [x] 574默认10项9/10：真正注册后无CLI触发的恢复已通过，queued实际派发20.893秒、完整耐久结果26.967秒；orphan实际7.565秒中断且零执行。原15秒完成/派发限制未达继续如实记录，没有产品性能修复声明。
- [ ] 本次默认失败换为two-worker的原共享15秒worker-ready（556、570相同断言曾通过）；因同一test target同时运行九个独立真实项目/daemon夹具，Root选择将时延验收置于独立Cargo integration target。抽取共享CLI Fixture，basic9项+concurrency1项；真实同项目两个worker、期间其他claim/status及原3秒/15秒断言逐字不改。保持默认Cargo参数、未设置test-threads=1，不以串行产品worker替代并行。原574失败收据保留，不当作全量PASS。
- [ ] 新目标隔离的是其他独立项目的测试准备负载，不认证九项目压力下15秒性能；最终按默认Cargo逐target执行全部10项，再核实际迁移函数等价、Clippy/fmt与main精确提交。
- [ ] corrected held恢复575正运行；仅迁移旧fixture的目标Task/Run两状态，保完整原历史、唯一本次实际释放及无新验证/证据断言。


## 2026-10-19 Task3 最终恢复验收预算裁定

- [x] 570实际默认CLI仍9/10；恢复在负载下15秒时仅原started，未执行，565单项原15秒时已有实际running和一次执行。未将其伪装为原15秒通过，也不凭一次结果归为环境故障。FR-107与P2计划没有重启恢复15秒SLA；该数字仍是本轮新夹具自行加入的要求。
- [ ] 采用一次共享90秒恢复观察watchdog（与既有finish有界等待一致），从status注册返回计时，派发与完成共用同一绝对期限，不再相加15+90或每阶段重置。整个期间只物理读取outbox，不调用finish/wait触发；同原request/input/actual事件、一次执行/孤儿零执行均保留。输出派发/完成耗时及是否达到原15秒，原失败日志完整保留，已知恢复延迟没有冒称修复。
- [ ] 真正交互并发仍保原status/claim三秒及two-worker共享15秒屏障，独立模块在verifier未释放时已返回/运行。只调整无批准来源的恢复完成watchdog，没有产品超时/门禁改动。
- [x] 567默认核心finish22/23，唯一失败为旧held恢复fixture预期；实际diff只有目标Run held→released、Task held→ready，符合FR93。新fixture仍全对象严格比较，仅两个字段更新，并核真实唯一release、finish/Evidence事件不增不改；待专项复验。

## 2026-10-19 Task3 队列恢复的分阶段验收

- [x] 553 core554真实22/22通过；CLI556默认10项9/10，通过真实双模块屏障、原status/claim三秒、停机cleanup、原ID重读、来源/错误项目拒绝。唯一失败为registration后15秒内未看到完整completed。
- [x] 同源码557单项仍失败；562只精简已获取真实Queued ticket/Interrupted Outcome后的重复完整重读，563单项仍失败。565原15秒诊断核actual confirmed started+running、pending空、executor count恰好x，说明注册已自动启动执行，超时卡在完整结果持久化观察。不能将三次失败称为环境故障或原性能预算通过。
- [ ] 修改新增registration测试为两个明确阶段：原15秒必须看到同ID实际running且执行恰好一次（无finish/wait触发）；其后只读poll confirmed completed，90秒watchdog对应已有有界等待契约，不触发调度。孤儿Running仍原15秒中断且零执行。保留派发与完成实测耗时；这是修正无批准完成SLA的测试语义，不冒称改善到15秒完成。
- [ ] 真实core恢复只匹配running的问题已修为与TaskService相同的running/verifying/held；保持原released断言，554真实重建用例已通过。恢复源码改动在冻结候选，尚未提交。
- [ ] 验证最终默认CLI、daemon故障、既有finish/恢复相关回归及Clippy/fmt，再精确提交main；Task4/5仍须真实Task3提交后实施。

## 2026-10-19 Task3 验收重新核对规格（当前计划）

- [x] 直接核FR-107（规格790行）及P2 Task3 3.4/3.5（计划329/333行）：要求后台验证、返回证据ID、真实多模块并行且status/claim可响应，没有finish全部preflight必须3秒的数值要求。3秒来自本轮新增测试的通用bounded helper；旧494/522失败及实际5.19秒准备耗时继续保留，不改写为原预算通过。
- [x] 选择最小候选finish-main542：沿已提交main1e2ce86，不纳入未验证的FreshPreparedHistory或FirstStartedAbsence；本次真实admission DTO、有限历史读取复用、已验证own-append完整来源保护仍在。
- [ ] 修复测试的快照竞争前置，区分已接受/uncertain与明确写前Catalog竞争；只核真实main覆盖原claim，保持固定15秒和最多三次，没有盲重试。
- [ ] 两真实worker因果验收：第二finish返回和第二worker就绪共用原15秒绝对watchdog；共享verifier屏障始终未释放，独立ID、queued/running无结果、独立树/报告和恰好执行次数均保留。status/claim原3秒保留。
- [ ] 加入真实admission六项负控，串行运行相关core、HTTP故障、全部9 CLI及Clippy/fmt；仅通过后精确提交main，保原Proposal工作区。
- [ ] Task3真实提交后继续Task4/5和其余已授权spec；最终发布前重新盘点并合并所有本次完成改动，清理其余已保护、已核实的本地开发分支/worktree。

回顾：548失败于快照测试前置的精确Catalog竞争，未到第二finish响应窗口；生产编译成功不算此行为通过。旧历史段落保留审计，以上计划取代此前“为新helper三秒继续添加FreshPreparedHistory”的方向。没有发行tag/资产上传，无App图形进程残留，syspolicyd当前0%。


## 2026-10-19 基线出版读取优化已合入 main

- [x] 精确提交 `58c1c41da8211178fdd4eb63fab2b26e90a75668`，tree `c8c1a2cbf37429b8e3258a9687f4b02fec6b536f`：只复用同一次证明中实际读取的Catalog/tree；完整OID不同仍独立读，来源、受保护文件、标签和人工批准检查保留，含双语增量CHANGELOG。
- [x] 冻结532：533新增4/4（包含在534的48/48中），535实际集成55/55，合计103个不同测试；536 workspace/all-targets Clippy与537 fmt通过。538逐blob核完整282路径index与532相同，539记录真实提交及main快进。
- [x] Root原17份未完成/用户文件已保存；同文件Proposal差异在合入前后规范化patch-id完全相等，其他原文件逐字不变。540仅删除本轮已完成且clean的独立delivery worktree，原其他branch/worktree尚未清理。
- [ ] Task3继续：519在初次main尚无checkpoint的测试前置失败，520/521仅修真实ls-tree缺失判断；522合法快照+rebase前置后仍超原3秒预算（第二prepare约5.19秒，cold Session capture约1.43秒），并非全通过。一次性FreshPreparedHistory生产/6负控源码已冻结，未编译/未提交，旧absence-cap方案不纳入。
- [ ] 完整spec、Task4/5、Proposal Bn、真实P7及自动发布仍未完成；没有发行tag或资产上传。最终全部合并main和清理其余本地分支的要求继续执行。


## 2026-10-19 Task3 响应性能与最终分支收口（进行中）

- [ ] 根因修复计划：完整Session prepare已产生真实VerifiedRecoveryHistory，用私有一次性FreshPreparedHistory连同同一install锁内Session身份/RecordedPrefix返回本次finish后续读取；不放入长驻Session缓存，cached/delta路径None，原wrapper行为和全部source/head/CAS/原三次界限保持。失配回原独立history读取，不拿较晚live给旧proof贴新身份；原FirstStartedAbsence候选继续归档不用。
- [ ] 独立publication切片525实际526为3/4：缺失blob已在Catalog实际ls-tree --long的exact OID/BAD记录提前拒绝，原测试误要求晚cat-file阶段，03仅精确修正该断言；527质量检查另发现两处复杂类型，需要命名现有tree/read-observation类型，无业务行为变更。完成回归/quality后精确提交main，保Root原Proposal字节。

- [x] 后续组合511的512实际22/22通过（测试173.14秒、含编译187.26秒），包括history02和旧Queued reader六负控。513全targets Clippy失败3类：临时Queued token过大、manual_contains、测试回调类型复杂；已在新候选用更简单真实admission DTO和等价lint修正处理，待重验。
- [ ] 当前采用更少代码方案：首次finish只回本次已确认同步的实际started接受观察（符合FR-107/P2共享耐久契约），原UUID/恢复/等待仍完整读，launch门禁不变。局部publication证明按完整OID相等复用同次已实际读取的Catalog/tree，不同OID保原独立读取。额外FirstStartedAbsence方案源码归档暂不纳入。
- [ ] 修正two-worker夹具真实前置：确认覆盖T-beta原claim的实际快照，再显式rebase Beta到main；仍测原3秒finish/15秒worker，不让快照合法推进误变为绕过NotLatestMain。组合518正跑519，尚无通过结论。

- [x] 489实际阶段诊断定位写后全历史重建；494真实新started/running私有增量已编译并执行，写后同步阶段从约3秒降至约0.2秒。完整双worker测试仍在第二finish原3秒预算失败（494：0/1，70.72秒含编译），不能宣称完成。
- [x] 07-02冻结497，498实际生产者/来源负控 **8/8通过**（测试105.47秒、含编译119.47秒），冻结输入前后不变；涵盖同eid异正文/metadata、外部pending、真实人工尾段、main/private/物理blob变化、确认失败和重建旧票据。
- [ ] 实现同一次无写入begin的有限历史复用和本次真实Queued票据视图，保持第二checked、G1/G5、确认、来源与最终CAS。最终原9CLI和质量通过后才精确提交Task3。
- [x] 491只读盘点17本地分支、47登记worktree，其中33路径缺失、无stash；495核所有旧分支非main祖先提交的git cherry均有补丁等价（没有`+`）。这只是初步证据，尚未核所有实际功能、脏文件和metadata独有对象，也未删除分支/worktree。
- [ ] 最终合并和清理前复核新鲜清单，保存缺失worktree metadata/index独有对象和活动目录内验收材料，再清理已验证本地开发分支。当前仍在开发，尚未执行删除。

## 2026-10-19 用户追加收口要求

- [ ] 开发完成且验证通过后，将本次开发统一合并到 `main`。
- [ ] 完整枚举全部worktree、分支未合入提交及stash，逐项核对真实功能和补丁等价；合并后清理其他本地开发分支和对应任务工作树，保留提交号、验收与发行证据。任务进行中的未验证改动先完成独立验证再收口。

## 2026-10-19 App 问答已提交与调试进程清理

- [x] App Answer 精确提交 `055d3e455d8b3aa158d4949d5105bd433e9b2ae1`，tree `5f0f40786267dd86cd4e596d3f3737d5a05baf92`，40路径。470逐blob核完整index与467冻结源相等，472/473记录真实提交及主树快进、原Proposal改动和任务文件逐字保留。
- [x] 独立native Ask468 **1/1通过**：实际CLI→原生App队列→原CLI回答、同UUID重试/原queue不变/真实Answer唯一；18.11秒父子不叠加，含编译命令34.28秒。469 native全targets Clippy、471 fmt通过；最终较435仅本测试文件变更，生产/前端与既有436/437/438/428/430验证及453实际构建等价。
- [x] CUA实机461已验证新包的真实提问、明确答复、CLI同原等待收到human:bruce及重读无二次回答入口。程序驱动GUI不作为P7真人验收。
- [x] 用户指出旧调试窗口过多后已清理；460实际盘点无Tower图形窗口残留，本轮GUI daemon已停止，所有预览文件保留。后续启动记录归属，测试结束回收，不再累积窗口。
- [ ] 完整spec/首版发布仍进行中：Task3核心476 **23/23通过**（356.90秒测试、368.81秒含编译），03故障474 **4/4**；与已提交App组合的480故障483 **4/4通过**。组合CLI481为 **5/9**，保留4失败：新快照前置来源变更、15秒worker/注册推进未到；单独two-workers482仍在第二finish的原3秒响应预算失败，正在补阶段诊断定位，不能归咎于并发或放宽预算。Task3未提交；Task4/5、Proposal Bn和真实P7/发布映射未完成。没有冻结发行tag或上传资产。

## 2026-10-19 解锁后实机与新版 App 验证（继续进行）

- [x] CUA 已实际读取窗口；旧 Review 包的演示画布卡片、关系线、节点详情可见，真实项目目录选择框可打开并取消返回。只验证到这些实际操作，不视为 Review/Answer 人工端到端通过。
- [x] App Answer 生产候选445与435生产源码逐字相同；锁定构建453通过（53.08秒），新预览 `.release/prototype/2026-10-19-app-answer/Tower.md.app`，包内CLI/version/schema、arm64、ad-hoc严格检查455通过。未发行者签名/公证；生产源码现已提交055d3e4，未正式发布。
- [x] GUI Answer：解锁后实际在新包读取真实问题、空答禁用/输入启用、明确发送并显示“回答已记录”；包内CLI原等待以0退出，收到同一human:bruce文本且非Relay/非Decision。重新读取显示首答且无二次回答表单；461核本机仅一份原请求和实际confirmed人答仅一条。QA程序操作不是P7真人验收。
- [ ] 完整native往返446在旧Task Claim的接受后来源同步失败；原eid/Run保留，未到新Ask步骤。457仅测试helper改为Claim一次、task show补同原件并核完整回执/无新claim或grant；458已跑完旧断言后在新Ask提示前EOF失败（455.56秒父子不叠加），保留443/446/458。独立合法first-enabling导入夹具的native Ask468已通过；最终旧mega源码完全恢复13d原件，未宣称完整mega重跑通过。
- [ ] 后台finish修正03→464：465 workspace/all-targets编译通过；466旧9项CLI为5/9通过（127.29秒命令），保留4失败：beta Acceptance引用未存在Module、首次noTests前NotLatestMain、注册恢复15秒未终态、foreign clone诊断断言。取消/同active不重执行/重启原Evidence/报告hint路径通过；仍未提交Task3，继续精确定位，不放宽门禁或期限。

构建准备记录：448离线cache缺包失败；449网络下载失败后主动终止；451因迁移依赖目录后pnpm重建、同三包下载失败而主动终止。452复制本机既有锁定私有缓存后offline安装通过（1.14秒）；根目录共享依赖未清空、未修改。sysPolicy服务抽查仍为0% CPU，未改安全设置。GUI测试仓库独立，App通过原生默认运行目录连接本次随包CLI启动的daemon。用户指出多批调试实例残留后，已清理旧调试App；460再次盘点无Tower图形窗口进程，本轮GUI daemon也已停止，保留全部预览与文档。此轮清理期间用户可能同时关闭窗口，不虚报归属清理数量。462核包内CLI执行内容与构建stage一致、仅ad-hoc签名字段差异。

## 2026-10-19 问答与后台 finish 接线（继续进行）

本轮按既定 MVP 顺序完成已实现部分，不新增依赖。所有重型验证仍由 Root 串行执行（构建 jobs=2），不再堆叠构建或校验进程。

- [x] Ask/finish 混合真实两次恢复用例360通过；实际 CLI 全量375 **52/52**通过，含90秒未答、重启与原请求重试。
- [x] 修复 CLI 混合参数误接受、Ask 关闭确认故障夹具的真实重开，以及 ProjectLock 句柄继承后的锁释放；默认 daemon371对应项通过。
- [x] 快照 writer 在完全相同权威上下文复用本次已读 Catalog；来源变化继续原完整读取，实际 RED380与最终模块385 **14/14**通过。
- [x] 完整 daemon386为 **65/67**，I2 5秒与离线 HTTP 2秒在并发负载下超时；已提交c092f70实际默认对照388为 **58/60**，相同两项超时。394/395按原期限分别执行各 **1/1**通过，不改变默认并发或放宽断言；完整默认套件仍有这项已知限制。
- [x] 最终核心392 **319/319**，393真实Ask14／human18／review15／snapshot40，共 **87/87**；workspace/all-targets Clippy396与fmt397通过。精确提交 `a85baa216a283cabb2d3e166f5630536ff744f5d`，tree `69a4523791a294040c365ab6847ca72554ccb38f`，152个Rust/Cargo输入与384逐字相等；398/399保提交与原Proposal工作保留证据。
- [ ] App显式回答候选已组装：03核心414 **5/5**、416真实双重建等integration **4/4**、417完整core **324/324**；426真实AppGate/CLI/重启 **4/4**；前端428 **158/158**、430类型 **0错0警告**；修纯DTO可见性/UUID夹具后436 native **61/61**，437/438 Clippy、439/440 fmt通过。最终生产核心较417仅纯AskHoldView公开导出，frontend与428/430逐字相等。完整native往返443在旧Review精确source-sync重试识别处失败，445只加同UUID精确理由、期限和断言不变，446失败详见顶部；App代码40路径已暂存于独立delivery树，尚未提交；453已打包，GUI461实机问答通过，见顶部。
- [ ] 后台 finish Task3已与实际Ask提交整合为403：standalone400核心401 **19/19**通过；实际组合HTTP确认故障及Notify404 **3/3**通过，原回执补齐且真实executor仅一次。CLI405实际 **2/9**，保留7项失败：普通Python夹具误作OKF已隔离修415；其余来源竞争/后台恢复due/执行材料保留正按原UUID修复，不改时限或断言。待最终组合门禁回归/质量与精确提交；Task4/5仍未交付。
- [ ] 继续 trailers/main证明、TTL、Proposal Bn 与发布硬门禁，保持真正未完成状态。

## 2026-10-18 持续推进至首个 MVP 自动发布（进行中）

用户新增指令“完成到自动发布”，沿用此前“首个可用版本、先不签名”的授权。目标是完整首个v0.1 MVP的可操作闭环与实际发布，不以现有开发预览或发布脚本桩宣称完成。发布位置先使用已配置origin `git@github.com:wizlijun/tower.md.git`核实，候选版本v0.1.0；未冻结tag、未轮转版本、未上传。发行者签名与公证不做，平台要求的ad-hoc保留。

新增依赖裁决：用户已明确批准“沿用 Note.md 的 0.1 版本线”，用于P3问答NFKC归一化的`unicode-normalization`；Note当前锁0.1.25。实施时同步root/native独立锁文件并核对新增最小依赖，不再次询问；未使用前不提前加入。

- [x] 完成当前FR-36/45审阅画布对照及精确原请求回归，独立验证和提交；GUI仍单独未验。
- [ ] 完成Proposal真实来源／影响分析、同writer持久与恢复，以及新基线冲突任务标记、App精确复核和领取／执行硬阻；满足后才开放Bn批准／出版。
- [ ] 补齐MVP任务check/finish、真实Evidence／合并证明、Ask／Review／恢复入口和原生技能接线，按已有P2–P7规格验证，不能以模型或桩替代可达服务。
- [ ] 完成App剩余MVP态势／关注项与人工操作的真实读取和动作接线；快层不重排节点，人工批准不自动通过。
- [ ] 完成同源码App／CLI／技能综合验收，保留失败、实机及真人条件缺失的准确状态；不将程序夹具伪装真人记录。
- [ ] 补齐发布器尚未接线的真实验收映射，进行全worktree／branch／stash盘点、双语CHANGELOG轮转、冻结tag、两架构锁定构建与资产校验，自动发布到已核仓库并验证下载资产。

既有发布器在`publish.mjs`保留“unwired P7 ... identity mappings”硬拒绝；该项须落实真实材料映射，不能删除检查或填假PASS。具体实现分段在下方记录，每段独立验证、精确提交。常规开发与测试自主继续；真正无法替代的外部条件只在备妥具体可审结果后报告。

## 2026-10-18 P2 finish 核心首批（已提交，CLI 接线继续）

- [x] 基于已提交 P3 Review 冻结权威验证输入，私有执行票、真实封存报告与不可变 Evidence；完成耐久 queued/running/completed/interrupted 生命周期。
- [x] 验证原 claim/started/completed 确认失败只修同原件，不重复执行；三次不同失败仅形成一个 Gatefailed，保持首次 60 分钟期限和高风险 G1/G4 硬门禁。
- [x] 兼容合法低风险自愿 G1 历史，但仍核真实原 Human/Gate/完整绑定；快照重开与六项负控通过。
- [x] 精确提交 `fa914abee53f9bf7caade32cb6a4d164df17fe02`，tree `6878a046c0d65d5fe93818d9a3362a4048ef9d39`；141 个 Rust/Cargo 输入与冻结338逐项相等。Root main 已快进，原 Proposal 工作区改动逐字保留。
- [ ] P2 Task3 后台 Job 与真实 CLI、Task4 trailers/main 证明、Task5 TTL/释放尚未交付；此核心首批不代表整个 finish 或 MVP 完成。

证据在 `.release/validation/2026-10-18-spec-resume/`：339 默认 core-lib **298/298**（371.05s）及 finish **14/14**（173.17s），总命令563.25s含编译，父子时间不叠加；344 workspace/all-targets Clippy `-D warnings`、347 fmt 通过。兼容修复前327源码的329相关回归 **96/96**，未冒称在338重跑。348/349保存精确提交与主树原工作保留证据。Ask335 **16/16**、334含3项late与14项旧Task相关测试 **17/17**、实际CLI336 **6/6**（含真实90秒未答段）均通过；Ask尚未提交，正以组合350执行两类Hold混合恢复351，不能据单支通过宣称组合已验。所有旧失败仍保留，未发布、无新增GUI/真人验收结论。

## 2026-10-18 Ask 实际 CLI 与有界 daemon 等待（规划后实施）

沿已批准 P3 Task2 接线，不新增依赖。核心候选先独立编译与原始回归；adapter 与 core 分开冻结，最终在精确组合树验证。等待仅用原 UUID／原正文，释放项目 producer 与 Session 锁；每段最多 90 秒，循环间隔不超过 15 秒，未答不自动批准。后台 tick 不替无活动 Run 永久续租。

- [x] 核 Ask 核心首轮故障，修复后验证真实来源／等待预算／重启与原回执。
- [x] 增加严格 clap 参数的 `ask`、`--wait`、`--reply` 和同 UUID 重试提示；无 actor／批准／保存规则参数。
- [x] 复用现 `/interaction` 有限 producer，实际 daemon 分段等待并响应停机，不占项目锁等待；接入无续租的 human tick。
- [x] 运行真实 CLI／HTTP 及并行其他模块响应测试，验证过期 Run、不确定原请求和 90 秒返回；精确提交Ask CLI批次a85baa2，使用说明与双语日志已纳入。

早期执行记录（已由本页顶部a85baa2及最终392–399证据取代，非当前未提交状态）：Ask checkpoint02 的真实14integration在295全部通过；最终组合323以全workspace/no-run编译324通过，325故障单元12/14，定位两项真实恢复缺口：冻结意图时间早于晚到Hub接受被错误拒绝；无guards系统终态重建outer与原receipt角色冲突。保留失败，正在精确修复。pending-beforeHub IO只修夹具按共享契约验证无Hub写、完整原pending，不更改产品错误分类。实际旧Review recipient_added漏洞在305真实RED，04拒绝保护在309/325通过；304错误过滤0项明确不计RED/GREEN。Root已写实际ask/relay/原UUID与显式run重试、锁外分段等待，并补独立于snapshot周期的后台due；CLI326正在自然执行，未计通过。

另两个隔离核心候选：finish `finish-core317` 默认14项在320 **14/14通过**（174.04秒子／183.46父，包含编译，时间不相加）；Proposal `proposal-core318` 在319 **15/15通过**（171.17秒子／180.49父）。前者修实际current Hub tail读取并保私有执行票／原报告，后者仍保持Bn硬拒。两者均待质量、旧路径回归与精确提交，不据专项通过宣称完整MVP。当前安装包仍为此前已核Review预览，未包含这些未提交候选。

## 2026-10-18 P3 人工评审首批（源码已提交，GUI 待验）

- [x] 独立编译真实 Review producer／CLI；修复闭包错误类型和测试只读 HumanState 接口。
- [x] Unicode 17 实际 RED→GREEN，投影／迁移 18/18、原真实评审 10/10；前端全量 151/151，类型检查 0 错误／0 警告。
- [x] 新 Review 协调原 Task intent，原 Review 身份与回执优先。原实现真实 RED 保留 pending claim；新实现专项通过完整回执、同 Run、真实 Hub 无新增 grant 与 detached viewer App 协调。
- [x] 修复原请求来源变化、G4 检查中途变化与最后阶段时钟回退的错误分类；稳定 dirty／未变基／旧 binding 仍为业务拒绝，新增真实故障负控。
- [x] 从修复后的冻结源码重建 CLI，完成原生队列及实际 CLI／App 适配恢复往返；受影响回归、Clippy／fmt 与精确提交。
- [x] 从本批实际提交构建新 App 预览；arm64、包内 CLI、ad-hoc 严格校验通过。
- [ ] 独立验收可见交互：CUA 定位实际包返回 `cgWindowNotFound`，尚未通过；不将协议用例计为 GUI 或真人通过。
- [ ] 接入后续 Ask／finish／Proposal，继续首版发布；本候选不代表全部 P3 或 M1 完成。

本批已精确提交 `b9284476109731d2329d3262093ccc6442ec8f4f`，tree `fe6246059d7c68ecb7c0f2ac416033d16b077206`，33路径；冻结源码 `p3-recovery-fixture274` 与全部index blobs相等。实际native workflow275 **1/1通过**：child361.78s、parent361.79s不叠加；G1／G4、非App拒绝、原UUID／完整正文／原回执、真正丢journal后App确认恢复、历史批准只读及新Run／新fences均通过。来源／ledger等integration261 **59项**、review CLI262 **2项**、daemon CLI263 **48项**、实时check CLI **3项**和App读取用例通过；workspace/native全targets Clippy276/277与fmt278/279通过。原core-lib235 **297项**为先前实际基线，后续三个core模块差异由本批定向完整测试覆盖，未冒称整棵最终core-lib重跑。前端55个源码／配置与typecheck210逐字节相同，151项基线仅一处测试Promise类型标注后经类型检查；详见source equivalence材料。所有中间真实失败保留。本批新预览包已构建于 `.release/prototype/2026-10-18-task-review/Tower.md.app`（0.0.0、arm64、ad-hoc，无发行者签名／公证）。构建281、静态283及代码签名差异285通过；CUA 实机定位返回 `cgWindowNotFound`，286保留失败及专用QA进程停止证据，未产生本次评审回答。尚无本批GUI、真人或正式发布结论。

证据在 `.release/validation/2026-10-18-spec-resume/`：human-green205（18+10）、front208（151）、typecheck210、maintenance212（11通过及一项观察水位错误）、maintenance213（修正观察后的专项通过）、maintenance-red214（旧实现真实失败）。同源码 CLI stage215 通过。native 编译216通过，native-lib220 55/55通过。workflow221在G1原请求来源竞争处拒绝，222在App只读原件authority切换处拒绝；均保留实际失败。CLI仅精确原UUID/原字节重试，GET只读重读已补夹具，尚待实际往返。core/native预静态检查233/234通过，默认core-lib235 **297/297通过**。新来源故障测试在旧实现red237真实失败；第一轮green238记录投影18/18、Review12/13，但后续证实其产物来源不可靠，不能当修复源码结论。独立复核另发现G4混合NotLatestMain和最后时钟回退分类缺口，正在继续修复。暂存候选tree `4035ee61b159fac9812d0a939cc8fa6f8ce8c8ff`已标为被后续修复取代，尚未提交，不得据此完成或发布；没有GUI或真人验收结论。

后续澄清：238切回较早mtime的绿测源码，共享Cargo target实际可能复用了237旧实现。已将Rust输入metadata刷新固化到运行器并保留逐项记录，源码字节仍前后核哈希。**同236源码重新编译241后原故障测试1/1通过**（编译8.02s、执行58.32s），不再据238判断产品修复失败。240受影响integration另暴露5项旧ack回退诊断被新ledger完整outer证明抢先，未接受越权；现仅将ledger水合移至原source／ack门禁后，保持旧断言和完整outer证明，243重新编译 **6/6通过**。

## 2026-10-18 实时 check CLI 交付

- [x] 独立冻结 Check 三文件提交树，真实缺命令 RED 后补 CLI／HTTP 接线；不混入人工评审候选。
- [x] 默认条件下实际 CLI／HTTP 3/3、daemon CLI 48/48、workspace/all-targets Clippy `-D warnings` 与 fmt 通过。
- [x] 精确提交 `00eb129`，tree `cf2d524cb428268d587613812f014e4e00b63cb8`；新增使用说明和双语未发布条目。
- [ ] 继续冻结 P3 候选，先独立编译，再验证 Unicode 17 红绿、评审耐久／恢复、原生 App 通路；通过后精确提交。

回顾：Check 只读取实际运行／提交输入、续租并检查 G0/G2/G5；G3=false，不调用 verifier 或生成 Evidence。证据为 `.release/validation/2026-10-18-spec-resume/check-slice-cf2d524cb428-manifest.json`，green136、daemon137、clippy138、fmt139。当前已安装 App 仍是旧候选，不含这些新提交；未发布。

## 2026-10-18 恢复开发与资源控制（进行中）

系统已恢复，用户明确继续开发并要求避免校验积压。保留所有既有门禁和当前候选，先验证、修复并提交耐久化，再推进真实人工评审/提案/finish，继续至首版发布。Root统一运行检查，Agent本轮仅只读审查；同一时刻一个重型批次，构建jobs=2；定向调试后最终默认测试条件仍须通过，不以资源限制替代原验收。

- [x] 复核Task23、Human4、Proposal6、Root10份源码哈希仍与交接一致；记录防重复进程经验。
- [x] 格式化冻结候选，单批编译，修复真实错误；先完成Task/outbox/snapshot/I2等原回归（`7e669ee`）。
- [ ] 完成P3/Proposal候选及live check独立验证与精确提交，再统一stage CLI跑真实App恢复链。
- [ ] 接通Review/Ask/Proposal/finish等既定产品闭环，按顶部发布目标继续，不绕过真实验收材料门禁。

恢复验证记录（未提交、未发布）：原Task两处来源变化回归先真实失败，修复接受结果的错误包装后，`tasks10`默认20/20、`outbox11`默认21/21通过。实时check捕获commit/tree的真实Git竞态先RED，再以固定commit解析tree修复，`guard-green07`新增13项和旧raw/G5共21项通过。Human协议/问答夹具修复后`human-fixtures15`11/11通过；快照持久评审来源项仍真实RED，未接线前不计完成。`outer17`真实outer历史3/3通过。`incremental12`、`outer16`误用过滤器执行0项，不作验收，已改用真实模块名补跑。

原I2 `ticker13`在17秒自然失败：B后台writer没有到达remote push屏障。保持原期限、续租断言和屏障，正查产品协调与轻ticker路径；不能把前述定向通过称为整批完成。证据保留在`.release/validation/2026-10-18-spec-resume/`，本轮暂无新源码提交。

后续实际记录：防积压工作流与恢复计划已提交`bd2a9da`；人已批准的NFKC依赖及两份锁文件精确0.1.25已提交`ac9a184`，与Note.md版本/checksum一致，未升级旧依赖。I2去重准备后`ticker20`自然通过，原5秒屏障3516ms进入，跨旧TTL续租、Run fence和shutdown drain原断言均通过；`snapshot21`完整40/40通过177.84s，原idle循环未减少。authority fixture将故障注入恢复到真实prepare后，原9项`authority24`全部通过。

大范围`corelib25`默认289项实际279通过、10失败319.35s：8项旧恢复夹具没有复用实际Session发件箱身份，2项fetch屏障内新建outbox提前读取权威配置；正精确修夹具，不改产品来源硬拒。终审新发现过期pending原领取可在同Session永久阻挡后续请求，`stale-red28`已实际复现，正以typed终结结果修复；只在耐久确认、补偿及最终来源证明全部成功后清完全相同原件，不对任意错误清pending。后续integration/daemon阶段因失败未启动，不冒称通过。

原I2只验证归档和远端写入阻塞下的响应；现有配置/ref本地Git探测仍在sole ticker，保留单Git超时。已记录额外屏障覆盖建议，不把当前通过推广成“任意Git阻塞均不影响续租”。P3 Unicode标点表发现13与NFKC库17版本不匹配，官方17数据生成补丁仅备存在`.release`，待P3批验证后应用，不算已完成。

新增归档回执修复计划（未提交）：`recovery-tests35`34/34通过；`recovery-git36`43/44通过。`pending-recovery38`实际证明剩余pending-only不是终结stale诊断：App恢复已将原pending Task真实append为stale并写入新快照，但Hub压缩后retry只拿到Duplicate，本地confirmed缺失，未读取归档中完整actual receipt。不能改成“未知就放行”或补造回执，也不修改该用例原G0 expected来掩盖产品缺口。

- [ ] 从VerifiedRecoveryHistory实际outer archive/tail提取有限Task种类的私有typed receipt，严格project/eid/完整original_of，禁止candidate.originals、confirmed-only、inner proof角色补信。
- [ ] 完整Session准备后构建可重建的原pending receipt缓存，重试仍经唯一pending fsync→实际完整receipt→confirmed fsync；保留来源/CAS校验及不确定结果。
- [ ] 真实恢复→原stale回执补齐→同原pending清理→原G0依赖拒绝回归通过；正文/guards/项目/内层伪回执负控必须拒绝。
- [ ] 冻结精确提交树，独立跑完整core、相关integration/daemon及Clippy/fmt，再提交；P3迁移补丁继续仅备存，不混入本批。

默认条件终审续作（未提交）：`task-final-core91`294/294及`task-final-integrations92`八目标全部通过；测试内clone引用经一行Clippy修复后，精确树`a22f22a9ab65c608fae65105f30259bc2b047925` workspace/all-targets Clippy与fmt通过。`task-final-daemon98`仍实际复现合法App确认10秒超时和I2后台writer进入push超过原5秒；不以此前五项定向通过代替默认57项，原长时backoff用例继续原循环自然执行。

- [ ] 去掉同一次maintenance中紧邻重复的producer context probe；保留首inputs实际探测、同directory/context/完整物理records相等、执行前shared source/head CAS及最终写入证明。
- [ ] 用私有`prepare_verified`复用同次恢复请求已核history/saved/facts；每个后续Git边界仍重新核实际来源、人工期限、fences、ref CAS和安装水位，不做跨请求授权缓存。
- [ ] 新精确树重新完整core/相关integration、默认daemon、Clippy/fmt；保留旧失败与新原始证据再精确提交。

P3最小Review候选已在工作区接线，仍未编译／验证／提交；Task精确index不含这部分。新增原回执／历史角色六路径冻结交接`human-review-history/candidate.json`，Root核全部SHA256；native真实G1/G4→原队列→实际恢复→旧批准只读链路为新增测试源码，尚未执行。Unicode17测试／表补丁仍待先真实RED后应用，不能以官方数据核对当Rust验证。

P2 finish下游只能先只读规划。Root发现提前草稿未满足P2规定“P3首批实际提交＋回归”前置，已停止扩展，保存草稿不接线、不验收；P3真实提交后按新基线重核再继续。

空闲维护优化续作（未提交）：`daemon98`原H5自然通过；精确树`4eae7659e7ebaf5be536fea8c49956e7f72ba703`的core294、八项integration、Clippy/fmt和定向I2/actor均通过，但默认`daemon106`仍有I2屏障及offline查询超时。该次H5在其余测试结束后由Root明确中断，单列为中断，不能计自然失败或通过。

后续实测：v1空闲候选因只读审查指出缺periodic锚点而中止；v2树`d987245d56d622b07436bc23e8e23786c72cf7f0`默认daemon自然56/57，H5原循环/断言自然PASS（64次实际重试、8,240次实际probe），仅I2实际约5.4秒miss。Root尝试中断H5时子进程已经自然退出，未发信号，不能记为中断。v3新增测试缺trait import而停，负例已通过；补import后v4树`7fa58fbafe596ef93783643aa628b35b721fb69b`新增空闲两项、物理配置blob缺失负例、cold项目共享uncertainty及Clippy全过。默认I2原5秒屏障实测4,879ms进入并完整通过，offline和actor也通过；其余全批仍在跑，未提交。

v4后续自然结果：daemon58/58通过，H5为7,460次真实probe；core292/294，两项均因Root去重错误新增“prepared与bound必须同目录”约束而失败。原快照接口分别核两份来源，旧夹具正确，保留原样。v5仅在来源exact相等时合并，否则原expected.probe错误原样传播，再保bound sync完整证明；候选重新冻结，先补跑原两项，再运行最终批次。没有将v4 core记为通过。

本批已精确提交 `7e669eef4661382e693e4974eb2aeccc06200d15`，32路径，tree `465e5899c4e7faa628e61b131ca1e3b1b0de0333`。v5/a05树默认daemon58、core294、八integration通过；其余五目标通过，snapshot39/40只剩身份错误顺序。73树仅补Session身份预检，identity负例、authority9、snapshot40及coldstartCLI16通过。daemonCLI首次47/48发现WS实际竞态，定向165与诊断166均RED，日志明确`recovery inputs changed during preparation`；修复在同一producer边界取inputs并完成订阅证明，guard不跨socket await。最终465树原WS回归、canonical／compaction负控2、默认daemonCLI48、全目标Clippy/fmt通过。全量58/294属于a05，未冒称在最终树重跑；复用差异和逐文件哈希在`task-final-commit-evidence.json`中记录。未发布，未作GUI或真人验收。

- [x] 完成Task实际outer归档回执、原pending补确认、终结stale协调及原G0拒绝回归，已入上述提交。
- [x] 完成无活动Run的system-tail路由提示、原periodic锚点与物理outbox身份负例；不安装业务事实或产生写入能力。
- [x] 完成相邻context读取复用和私有prepare_verified；独立来源仍分别probe，后续实际来源与CAS保留。
- [ ] 单独验证并提交CLI `check`三路径，再以实际提交为基线验证P3 Review核心／原生／前端。

- [ ] 空闲项目仅以物理outbox、真实Git common与连续system tail决定是否跳过无效维护；不安装业务状态、不产生写入权限，实际动作和deadline仍走完整证明。
- [ ] 增加真实Hub租约、业务tail及outbox身份/内容变化回归，证明提示不推进投影、水位和snapshot摘要。
- [ ] 冻结精确index，先定向及默认daemon验证，再完成受影响core/integration与Clippy/fmt；保留原线程数、期限、H5循环和原断言。

## 2026-10-18 工具启动阻塞及续作交接（未完成，未发布）

已验证的Proposal真实来源/声明影响分析精确提交`f689695`（6文件）；实际6/6、旧Catalog31/31及owned Clippy通过，全targets曾因并行Task lint未绿，未改写该事实。后续窄L4 Proposal载体/consumer、actual outer history helper、P3人工状态/NFKC/Decision/等待模型和Task durable整链仍未提交；没有用已提交analyzer冒称完整propose或Bn出版。

Root最初live check 10项与既有raw/G5 21项实际全绿(matrix05)，之后增加实际Session短锁捕获/复核、过期检查和globstar前缀负控，最新版本尚未编译验证。真实native恢复全链扩展也未执行。新增依赖两锁精确0.1.25，原其他包版本未改；NFKC实际行为尚待P3测试。所有这些未验证改动不勾完成、不纳发布。

本机新cargo/rustfmt/测试binary在启动前等待，cargo --version也无输出；Python仍可读取和保存源码。Task owner记录syspolicyd约315%CPU。仅取消明确属于本任务的过时排队检查，不改系统安全服务、不推断用户锁屏；已异步请求系统重启后继续。尚未实际运行的测试不计RED或GREEN，SIGTERM不计自然完成。

恢复顺序：先核候选hash/两锁/所有owner修改及既有未跟踪组；完成Task durable最终编译、确认失败/重开/原intent协调/全部snapshot与I2回归，精确提交；再验证Root live check及P3/Proposal候选，阶段提交；stage同源码CLI跑实际native领取→丢journal→原App请求确认全链。然后继续真实P3Review/Ask、Proposal producer/recheck、P2finish、P5/P7和冻结发布，不绕过发布器unwired gate或真人条件。

交接材料分别在`.release/validation/2026-10-18-spec-continue/{task-check,task-durable,human-intervention,proposal-payload}`，源码备份和哈希只是未验证进度保存，不是发布/验收凭据。

## 2026-10-18 P2 实时提交守卫（实施中）

按P2 Task1既定check顺序，补同task_gates唯一raw/G5之上的只读入口，为P3 G4和后续finish捕获实际提交。保持core适配层分离，不运行verify、不产生passed Evidence、不批准任何Review。实现前已读P2Task1、当前Catalog授权与Task/Run实际签名；不新增依赖。

- [ ] 真实Hub Add/Claim夹具获取Task/Run/全部lease；先证明clean commit检查行为RED。夹具第一次遗漏TaskService实际事件apply，记准备失败，不作为产品RED。
- [ ] 复用validate_task/raw-z，核host/worktree/t分支、完整模块fences/TTL、操作中/dirty/untracked、权威main祖先和变更范围；旧新blob慢层与非法对象、mode及非UTF8拒绝，最终重读commit/tree/status/main。
- [ ] 真实Git矩阵验证越界/删除/改layer/新慢层、touches epoch、换目录分支、staged/untracked、操作中、main变化、glob边界；检查不改变refs/index，G3保持false。
- [ ] 冻结源码与定向Clippy/fmt后精确提交；真实Session调用端还须按已同步Task投影核依赖merged和复核状态，再接daemon/CLI，不把可手造纯函数参数当产品授权。

## 2026-10-18 App 确认原恢复请求（入口已提交，真实恢复链补验中）

- [x] 只接受CLI已登记的64位原request_id，显式读取原项目、来源、高水位、原期限；默认不发请求。
- [x] native admission绑定所选项目、实际common_dir、本机human身份和daemon实例；确认前重读完整原DTO，固定POST仅带native actor；过期、换项目、实例变化、未知结果都不自动重试。
- [x] 前端140/140、check 0错误0警告、Vite414模块；native48/48及Clippy/fmt通过。另以固定92aa8a6核心加精确native覆盖在独立临时源码验证同48项，未借并行Task未完成行为证明本段。
- [x] Root核12源码、20日志和4关联材料全部字节/哈希，精确提交`53446bc`。这是自动TCP协议测试，不是真人或GUI验收。
- [ ] 现有真实CLI/daemon/native全链增加领取→快照→仅删除夹具journal→原CLI请求→native load/confirm→旧Run不得复活及新epoch领取。Root扩展测试未提交，待Task耐久化最终冻结后统一stage实际CLI。

回顾：保存的原恢复请求和业务状态仍归daemon/core；App只保存可丢弃admission，未新建用户文档写路径。证据在`.release/validation/2026-10-18-spec-continue/recovery-app/handoff.json`。当前包仍为旧审阅画布预览，不声称已经包含本提交。

## 2026-10-18 规格续作：审阅时的基线／草稿对照（源码及预览完成，GUI未验）

已核主规格FR-36、FR-43–45及P6 Task9.6、10.1：现审阅会隐藏画布，接口影响只显示JSON，缺少可见的主干／拟议变更区分。本段以固定审阅响应中的真实main画布和精确草稿构造只读叠加，保留坐标、对象／边身份及原文，接口影响使用既有核心计算的两端模块集合。不新增依赖、存储或写入入口；后续Bn批准、Proposal、任务复核及FR-46出版切换动画仍须完整契约，不在UI自行放行。

- [x] 冻结只读投影契约：core返回实际Catalog main revision及画布，native复用同一CanvasDocument／对象读取投影；没有画布、损坏或历史来源明确诊断。
- [x] 审阅保留同一画布，主干实线、拟议草稿虚线，显式切换叠加／主干／草稿；节点坐标不重排，原始ID冲突由显示命名空间隔离，不作为可编辑ID写回。
- [x] 接口影响两端同时突出显示并带形状／文字说明；旧端点仅在主干存在、删除对象及无法定位模块均可辨，完整diff与原binding继续展示。
- [x] 真实core／daemon／native读链与前端行为验证；保证纯读取无hub／续租／main/index写入，旧批准／重试逻辑不变。
- [x] 精确提交已验证源码，更新双语日志，构建独立新预览并检查实际产物；GUI工具故障与自动验证分报。

实施前已按现有P4/P6批准规格确认范围，无需新增技术选型裁决。另安排只读分析Proposal/Bn下一阶段共享契约，不能用纯模型或UI演示冒称该完整闭环完成。

回顾：前端精确提交`b5117e7`，core实际main provenance及有限纯路径校验共享`49a1108`，native同一投影`8a0b7ec`，真实native全链`92aa8a6`。前端128/128、check0错误0警告、Vite412模块；native lib39/39；core审阅8/8(26.78s)、daemon路由3/3(70.44s)、workspace与native all-targets Clippy及fmt通过。新增core测试先真实Null≠actual main RED，再保持fixed draft source不变GREEN；AGENTS.md普通引用先因误用init目录校验RED，复用唯一repo_path后GREEN；同名自由卡片／Interface不能冒充Module端点的负控也已补齐。独立只读终审无P1/P2，未重复测试或冒称GUI。

最终真实native单条链1/1，281.46s子进程／281.47s父进程不重复计数：原S7八动作与精确审阅/原请求仍通过，实际B0 Recorded后创建B1、改名与新增Interface，再重启核对ReviewView的精确draft投影及实际main Git画布节点/边，旧main没有新Interface、两端模块集合精确，Bn批准仍阻断，main/HEAD/index/status不变。staged CLI SHA256 `13297142989986af3a011d4a31ad2d2bf4e210618fc8644b050a4bbbd2cacca7`。来源近结束记录而非执行前封存的限制见`review-canvas-core/native-workflow-final-receipt.json`，不改写历史。

中间预览`.release/prototype/2026-10-18-review-canvas/Tower.md.app`固定`92aa8a6`，从独立detached工作树构建，全部226个tracked文件与Git blobs相等且前后不变（不含所有host/toolchain/依赖环境），5文件精确复制，版本0.0.0、arm64、release profile、仅ad-hoc；包内CLI非Git目录version/schema、两Mach-O及strict签名验证通过。实际内嵌JS/CSS字节与该隔离树dist一致、HTML完整DOM一致。构建期依赖安装目录曾symlink共用而触发主树pnpm重装，真实日志与lessons已记录，不把依赖环境称封闭。准确新路径CUA仍报`-10005: cgWindowNotFound`，无实际窗口或点击证据；原构建manifest及后续GUI/内嵌证据分开保留。该包不含之后进行中的Proposal、Task durable或RecoveryCard，不是正式MVP发布。

后续前置已核：Proposal仍缺真实生产／窄L4例外及任务复核，P2缺任务durable与check/finish，P3缺人工事实producer。用户要求做到自动发布后继续实施，不在该切片收工。GitHub origin已核为公开`wizlijun/tower.md`，gh当前有push权限，远端main为`7d6cb428abf3cd0436353279d397ffe0a6181ec5`且查询时无v0.1.0 tag，未执行远端写入。P3 NFKC依赖已获人明确批准沿Note的unicode-normalization 0.1版本线（Note锁0.1.25）；实施时同步锁文件，不重复询问。

## 2026-10-18 从当前权威基线开启变更草稿（源码及预览完成，GUI未验）

依据P4 Task2.1、Task6既定AppGate create入口及P6 Task9.3，在现有S6/S7保存、原请求ledger和唯一writer上补当前基线建草稿。先完成“已核B0→App创建B1草稿→同画布编辑→保存/重开/审阅”的可见闭环；Proposal、语义影响和任务需复核尚未完整接线，不删除后续基线出版的现有硬拒绝，不把本段称为Bn出版完成。无新增依赖、无CLI create或人工批准命令。

- [x] 核对真实publication proof和当前基线/main绑定，落实共享Create typed action、原UUID到稳定草稿ID及历史兼容。
- [x] 从权威提交逐字复制Intent/Module/Interface/Acceptance与canvas，排除旧审计及快层对象；真实保存不触碰用户工作树/main/index，陈旧或伪造基线拒绝。
- [x] 扩展现有AppGate `/drafts` create及纯读当前基线元数据；native创建前fsync原请求，同UUID/正文重试及无草稿时的pending恢复可见，旧Edit/Review队列兼容。
- [x] App显式“创建变更草稿”后打开真实durable ref，复用既有编辑；后续出版仍明确阻断，未完成Proposal/复核不得伪装可批准。
- [x] 验证core真实B0→Create→编辑/重开、daemon门禁/旧main冲突/重试、native原队列与前端交互；通过后精确提交、更新双语日志并构建新的开发预览。

只读核对确认最终recovery proof已有Bn递归链，但producer/audit/tag仍限B0；Proposal登记为L4且现publication consumer只收四慢层，必须在独立完整提案契约中窄化许可，不能扩大全部L4出版白名单。当前段不改这条证明边界。

已验证并精确提交：core `5159ea5`、App `c73d91b`、daemon `07bb7ad`；发布准备工具独立修复 `2b80428`（原已有native共享core锁未被允许，无新增依赖）。core最终新增4/scan17/save6；daemon最终3/3（70.53s）；前端119/119、native lib31/31、类型检查零错误零警告、Vite410模块，workspace与native全targets Clippy及fmt通过。真实native链及新的App构建均已通过；实际GUI点击仍独立未验。

真实native单条测试精确提交 `95d5b21`：原八动作、原队列fsync恢复及同UUID回执继续通过；实际B0 Recorded后从当前main创建B1私有草稿，创建前daemon重启恢复原UUID/body，重试同回执，真实改名保存、再次重启重开一致，新Bn批准仍阻断，main/HEAD/index/status与原队列字节保持。最终1/1，child255.71s、parent255.72s（父子不重复计数），目标Clippy/fmt均0；实际staged CLI SHA256 `c1769ad58bd4fbda7e376d0ad025b5a57949613de956d03a4f26571cfb28922f`，证据`current-draft-daemon/native-workflow-final-receipt.json`。attempt01独立快照合法推进main导致末尾夹具断言失败，测试在B0出版后检出main隔离该背景写入；attempt02初始S6证明来源竞争，夹具仅对含同UUID的精确来源竞争错误在原30秒内以原正文重试；均保留失败日志，未改产品proof/时限。

本段回顾：新开发包 `.release/prototype/2026-10-18-current-baseline/Tower.md.app` 已从固定 `95d5b21` 自然构建并复制，0.0.0、release profile、arm64、仅ad-hoc。Root逐项核对189个记录的tracked输入与Git blob相等且构建前后不变（明确只是源码子集），5/5实际包文件复制一致；包内CLI在非Git目录version/schema成功，两二进制arm64及deep/strict签名验证通过；实际App内嵌压缩JS/CSS精确字节对应dist，HTML完整DOM对应。三个旧包与三份原来源manifest共18文件不变。记录在`current-baseline-app/candidate-manifest.json`，后续静态与GUI结果另存`validation-summary.json`，不改写原构建历史。

精确新路径CUA实报`Computer Use server error -10005: cgWindowNotFound`；无新包窗口、创建、编辑、保存、重开、审阅的实际点击证据，不推断用户锁屏或产品崩溃，不要求用户代做常规测试。本段不是完整M1或正式首发：Proposal、语义影响/任务需复核、Bn批准/出版、任务finish/技能、实时态势/中断入口及真人综合验收仍缺，后续批准硬门禁保留，无tag/推送/版本轮转/发行者签名/公证/Intel验收。

打包前只读盘点覆盖35个worktree登记，只有主树可用，另外34个不存在或Git状态不可读，逐项注明不能枚举未提交内容，未清理；15旧分支35个ancestry未合入提交均已核稳定patch-id等价及main具体提交，唯一非等价detached旧提交逐29路径核对为main替代实现，无当前切片遗漏；stash为空。审计固定`2b80428`且期间HEAD未动；后续测试提交`95d5b21`和最终文档更新另记，不冒称盘点包含未来提交。用户原未跟踪组均排除、不改动。

## 2026-10-18 环境恢复后的闭环验收（进行中）

用户确认 Mac 已解锁且操作正常。当前工具链已可立即启动；上轮 `/tmp` 临时证据已不存在，历史记录保留为历史结果，不作为本轮源码通过证明。沿用已批准的规格内最小闭环计划，不新增需求或依赖。

- [x] 验证并修复当前模型/原请求 ledger/真实审阅与恢复；保留既有出版证明契约。
- [x] 验证并修复真实 B0 writer、五个中断窗口、hub 丢失及远端拒绝/竞争。
- [x] 验证固定 App/daemon 路由、纯读取不续租与真实 S6→保存→编辑→重开闭环。
- [x] 完成前端及 native 检查，构建可脱离源码目录的 arm64 release-profile 开发预览（0.0.0，仅 ad-hoc）。
- [ ] 实机连续点击完成编辑、保存、重开与审阅；当前仅启动/选择项目/载入真实草稿已观察。
- [x] 已验证功能按精确范围提交、记录提交号；产物输入与提交对应关系单独核对，剩余规格缺项保留。

本轮精确提交：内核与唯一 writer `690f75e`；固定 daemon/CLI 接线 `5390468`；同画布编辑及精确审阅 App `5e90774`。core 完整 lib 277/277、真实 writer 专项11/11、Review/DraftSave 定向集成13项通过；workspace fmt、全部 targets check 与 Clippy `-D warnings` 通过。最终 daemon bin54/54（旧长时间 H5 用例1项过滤，未称完整workspace）、CLI schema/拒绝2项与完整coldstart16项全过；真实S6→保存→编辑→重启→读取→拒绝链包含在16项内。该链首轮发现精确authority重试失配并修复；旧help禁止submit断言按新命令修正；新夹具原本释放main却断言独立快照不推进main，改为保留main检出后原断言通过，未放宽原时限或降并发。原失败日志保留。

App前端95/95、native45项、类型检查、build、native Clippy通过，31个冻结源码逐项核hash后精确提交。Root已实机启动新开发包，通过系统文件夹选择并读取自有QA项目的真实S6草稿，截图确认模块节点、编辑表单和审阅入口；尚未证明实际编辑/保存/审阅操作，窗口工具随后再次cgWindowNotFound。另构建仅本地identifier不同的同源QA候选，窗口工具仍报cgWindowNotFound；最新release-profile候选也定位失败，不据此推断用户锁屏或产品崩溃。自动验证不冒真人批准，QA项目不是用户项目。

新增精确提交：`a51814d` 修复非交互构建环境过滤掉 CI 导致 pnpm 重建中断，实测 RED→81/81 GREEN；第二次实际构建成功。可移植预览位于 `.release/prototype/2026-10-18-draft-review/Tower.md.app`，版本0.0.0、arm64、包内CLI在非Git目录执行version/schema成功，ad-hoc校验通过，非正式release、无tag/发行者签名/公证。原manifest保留实际构建HEAD及当时CI补丁未提交事实，不改写构建历史。

`d0442c5` 新增真实native→daemon测试1/1通过（64.98s），native Clippy和fmt通过。覆盖首次发出前fsync队列重开保持UUID/body、卡片/关系实际保存、daemon重启读取、精确拒绝及新修订明确批准后Pending；只对自有一次性夹具批准，不冒真人GUI验收，也未测“服务已成功但响应丢失”窗口。

本轮证据持久保存在忽略目录 `.release/validation/2026-10-18-resume/`，含core最终99源码清单、writer两版冻结源码、App31源码归档与全部本轮日志；历史临时证据不可用的限制仍保留。当前未冻结版本tag、未正式发布，未运行完整workspace全测试或Intel实机；后续S7及交接增量见下节，从当前基线建草稿/Bn、提案、任务完成与技能仍属后续规格工作。

## 2026-10-18 S6 交接直达已保存草稿（源码及预览完成，GUI未验）

依据既有 P6 Task9.1，只读规格复核发现候选与真实保存入口仍须手抄ID，说明文字仍称编辑未接入。下一最小切片只接已存在的同scan ID/native load，不新增协议、存储、依赖或模型运行。S7已构建包保留原样；新交接源码独立验证、精确提交后再构建另一目录的新预览。

- [x] 候选完成后说明既有会话如何使用真实scan ID与共享schema提交S6；不自动生成意图/JSON，不执行提交或批准。
- [x] 显式读取此扫描的持久草稿，实际ref存在才进入已有编辑/原队列；不存在时保留候选只读及明确诊断。
- [x] 前端验证同ID、无重复扫描/隐式读取、切项目旧响应丢弃、已有手动ID入口继续可用；check/build通过后精确提交`937f1f4`。
- [x] 更新使用说明和双语日志，构建新的arm64开发预览并核对实际产物；GUI工具失败另报，不冒验收通过。

只读核对证据 `.release/validation/2026-10-18-resume/mvp-gap-review-42542a0.json` 记录仍缺当前基线草稿/Bn/Proposal、任务finish/技能、人工中断入口、实时态势与真人综合验收，不将本小切片标为完整M1。

最终前端107/107（15.00s）、check零错误零警告、Vite408模块构建通过；Root逐项核冻结6源码（5改、1既有未改组件测试）与12日志hash，精确提交5path。新增3行为用例先可编译RED再GREEN；首次无TTY未进测试、测试deferred Promise泛型推断错误分别保留。CLI指引修成可直接复制的`baseline submit input.json`，不使用会被shell当作重定向的尖括号路径。证据 `.release/validation/2026-10-18-resume/s6-handoff/handoff.json`。

最终交接包已按固定`937f1f4`自然构建，位于`.release/prototype/2026-10-18-s7-handoff/Tower.md.app`，0.0.0、release profile、arm64、仅ad-hoc。Root在构建前记录183个tracked输入（含index.html/pnpm-workspace.yaml）、逐项等于固定Git blob并在构建后保持；这不是全部工具链/环境的封闭清单。完整复制5文件hash一致，包内CLI非Git目录version/schema、两个二进制架构与strict签名验证通过；实际App二进制内的压缩JS/CSS对应dist精确字节，HTML完整DOM对应。两个旧包与原manifest均未变。新包准确路径CUA再次返回`cgWindowNotFound`，仍无新窗口、编辑或审阅点击证据；不推断锁屏或崩溃。构建收据与后续静态/GUI结果分开保存在`s7-handoff-app/candidate-manifest.json`及`validation-summary.json`，不改写原构建时的记录。无版本tag、发行者签名、公证、正式发布或Intel验收。

## 2026-10-18 补齐既定 S7 结构编辑（源码及预览完成，GUI未验）

依据主规格§6.5 S7及P6 Task9.4–9.7，继续已批准范围，不新增存储、业务协议或依赖。当前共享Rust/TS DraftEdit及native固定接线已有八类结构动作；本段只补前端缺少的Merge、Split、PutInterface、RemoveInterface、ConfirmVerify操作，并扩充真实native保存验证。现有Rename、Move、AddDocref与自由卡片编辑继续复用。

- [x] 核对共享动作、对象原文/fields及约束，确认最小表单与显式路径/验收/端点分配方案。
- [x] 表单输出既有typed值；Split完整分配不猜映射，Interface保留原文/未知字段，verify以cwd/argv提交且不执行。
- [x] 组件验证真实输入与busy/错误保护，check与build；真实native→daemon逐动作保存后核对实际对象、接口和坐标以及重启耐久、用户工作树不变。
- [x] 精确提交本段源码、同步中英文未发布日志并构建新预览；GUI无法操作时单独保留未验项，不能标S7真人验收通过。

本段发现并处理的必要闭环问题：

- [x] `60b1616`：FR-101真实Interface名称写入S6与PutInterface边label，接口Rename同步label；3项行为RED后既有draft+scan28项、快层守卫2项、fmt/Clippy通过，几何与未知字段不变。
- [x] `e5d3c97`：原请求状态读取遇来源竞争/IO保持带同UUID的uncertain，不误报扫描拒绝或清空本机队列；真实routes2/2、native lib25/25及分类、fmt/Clippy通过。字段投影仅缺失时omit，显式null/旧串/不可JSON的存在值保留不能读取标记与原文。
- [x] `c2dac1b`：仅新Submit/Edit保存取消固定source中不存在的删除；旧manifest、hash、review subject与同UUID回执保持原样。scan17、draft14、Review7、DraftSave6及原IO/uncertain单测3，共47项通过；fmt、core全targets Clippy通过。真实free-main B0为Recorded，main/tag唯一提交、真实Existing删除、未知CRLF字节和用户HEAD/index/status均核对。首轮辅助trim导致CRLF断言失败已保留，改为读取真实Git stdout后整组17绿；不弱化publication proof。

前端最终104/104、check零错误零警告、Vite408模块构建通过，5路径已精确提交 `c67dd72`，冻结源码与日志在 `.release/validation/2026-10-18-resume/s7-frontend/manifest.json`。首轮3新组件用例暴露固定happy-dom的`:checked`只匹配INPUT、不匹配OPTION；已实测确认，控件沿仓库既有显式onchange写法，不模拟DOM选择器或改断言。只读交叉核对另指出真实S5候选verify是展示用string[]，不能shell split；已补明确“手动配置替换验证命令”，默认保留原文且禁保存，用户主动填写cwd/argv才提交。新旧串两种真实形态均有测试。

最新staged CLI的SHA256为`f05d1da212c5f9cb445484e77f1258491c27496308e6b74b8b7d3378a6f267f1`，生产3path与core冻结一致。实际native八动作attempt04自然1/1通过（187.79s，父/子重执行不重复计数）：Rename、Move、ConfirmVerify、PutInterface、Split、RemoveInterface、Merge、AddDocref逐次核对真实ref和load；继续卡片队列同UUID原回执、daemon重启、拒绝→新修订→批准Pending，用户main/index/status/README不变。attempt01夹具来源路径假设错误、02原请求读取真实产品失败、03人为中断均保留，不计通过。该测试单路径已精确提交`42542a0`，目标Clippy及fmt通过；Root补充workspace全targets Clippy也自然0（2.53s）。S7新App已自然构建并复制至`.release/prototype/2026-10-18-s7-editing/Tower.md.app`，Root核5/5包文件、所录181输入逐项等于固定HEAD、原旧包未变，包内CLI/架构/ad-hoc验证通过。原输入清单遗漏index.html/pnpm-workspace.yaml另补证据，不改原构建历史；完整GUI仍未验，准确新路径CUA实际返回`cgWindowNotFound`，不据此推断用户锁屏或产品崩溃。

实现前已核准这是既定规格缺项，不用新设计代替S7。生产内核不复制到TS；前端只做输入表单与transport形状检查，域错误由既有P4返回。

## 2026-10-18 继续规格内画布闭环（实施中）

下一段按既定 P4/P6 顺序推进：先冻结已验证的真实 Git 草稿与精确审阅内核；将 Submit/Edit/Review 纳入同一原请求 ledger 后接固定 daemon/App 路由；Svelte Flow 只做同一 tower.canvas 的显示和有限编辑投影；最后接原批准的慢层出版并重新构建/实机验收。保存与批准分别验证，业务回执耐久前不得显示成功。

开始时基底507eb9c，生产源码无未提交改动；保留五组用户既有未跟踪文件。执行既定纠偏计划，不重启已归档的canvas-draft/Unix鉴权路线。

- [x] P4 Task1：唯一S6 JSON Schema及typed解析，沿共享DraftObject保留原文，非法输入携完整路径
- [x] P4 Task2纯模型：复用共享DraftBundle，八类既定编辑作用于真实对象和同一tower.canvas，失败原子且未知/不透明条目保留
- [x] P4共享writer第一段：仅抽取现有Session隔离Git基础，快照业务/fencing/重试行为保持；不开放无producer的草稿写入逃生口
- [x] M0批4 D6–D8：冷启动共享报告一次耐久、逐模块Evidence、投影与归档恢复，为真实扫描job前置
- [x] 真实scan/wait/派生drafts接线：后台真实验证、耐久证据、90秒等待/取消/恢复/派生缓存重建；保留App/CLI当前实际边界
- [ ] S6 submit、Git草稿保存、App编辑/审阅接线：P3原请求/回执底座、同writer持租约写私有ref、有限native候选展示及同tower.canvas注释编辑正在实施，不用fixture/便签存储替代

边界：schema和纯编辑本身不启动服务、不授人工批准、不改main；这不代表P4生产接口、M0批4或M1已完成。共用验证命令检查从verifier现有validate_request精确提取，不另造校验规则。

实施回顾（已精确提交 `ec260a1d7c52c89f017b8a1876d75725c976626d`）：真实 CLI 后台扫描/等待/候选读取、证据耐久、重启与派生缓存重建已接通，15项 coldstart_cli 全过；验证器16项通过。候选原子出版在运行缓存，不改用户文档或批准基线。来源固定为真实Git输入，只有真实验证器能构造执行能力；S6转换只接唯一已核reader生成的CompletedColdstart。纯编辑与冻结覆盖保留源对象的最终引用，接口增删同步Module.interfaces，原文/未知字段/不透明条目保持。

审查修复了并发扫描outbox争锁、验证期间main前进、首次pending前来源竞争、恢复请求取消提前释放互斥及live缓存删除不可重建。默认bin回归首轮49/50通过（另过滤旧H5长集），旧Busy用例5秒未达真实CAS检查点，单独同用例通过；不改时限或降低默认测试并发，不把该轮记整组通过。后续同冻结二进制、默认线程与同过滤集合50/50通过（53.59s）；原失败保留，未放宽5秒。最终同源core全部lib227、定向集成190、coldstart_cli15通过，fmt与workspace全targets Clippy -Dwarnings通过。未运行旧H5长集或完整workspace全测试，未发布新App。

第二段实施中：同一tower.canvas新增四个有限注释动作，text/link/group卡片与普通关系可孤立、交叉或成环，file/obj绑定/不透明目标仍拒绝，未知字段与无变化原文保留；独立提交 `39496b7499c4d25def4ea572ebd33d29610b47ad`。7项注释测试、原八类编辑12项及真实source草稿freeze/原refs/index/status不变测试通过；包括转换节点类型时保留原类型未知键的可编译RED→GREEN。该提交不含App保存或批准，也未发布新App。

- [x] P3基线审阅内核：原请求/回执耐久与恢复、固定binding真实ref reader及审阅producer已验证（Submit/Edit统一ledger仍待后续）
- [x] P4 Git materialization：同writer/主干租约写草稿与真实ref reader，完整原请求进入manifest（draft.saved业务receipt尚未接入，不标P4全量完成）
- [ ] App：明确点击生成真实候选→同ID等待→显示真实模块/四种节点/证据三态，再接草稿编辑/保存/审阅，最后统一打包与实机验收

AppGate增量：daemon私有gate复用既有instance与固定App标记，M0恢复保留原actor wire及完全相同的人名语法；新动作可由同gate构造human actor。2项gate单元与6项现有真实App恢复路由回归通过，不把客户端边界当密码学认证或真人批准。候选App展示前端最终79/79、native33项、check/build和开发App构建通过；Root实机接口三次返回cgWindowNotFound（含重置后），因此视觉与真实点击验收仍未通过，不把构建成功当实机成功。候选入口显式点击后才启动daemon/扫描，默认空态，演示须主动选择；界面明确候选未批准且只读。此段源码提交后继续同一画布编辑，未签发行者名、未公证、未正式发布。草稿保存与review producer继续实施。

内核第二段回顾：真实私有草稿ref保存/读取和精确审阅通过；writer源内12、snapshot_git12+54、snapshot_service40及authority并发8通过。Root最终定向review4+scan13+ledger3通过。Root扩大core lib首轮249/251，发现新review错误L5已严格修回权威L1；原publication恢复精确用例修后1/1，ledger9+review3+集成4+3与Clippy通过。另一旧fetch屏障超时独立同用例1/1（9.86s），未放宽原时限；不将原整组失败改写为全绿。新App候选源码已提交0fc2ac1；视觉自动化仍失败，未发布。

内核第二段已精确提交 `8fd0a24`，修后同冻结二进制的252项lib回归全过（231.52s），原失败记录保留；随后既有coldstart_cli15/15全过。Root固定daemon接线的先前切片已验证：真实私有ref优先GET、完整ReviewView、App gate、human署名、重复原请求与异body拒绝、main/index不变，真实HTTP测试1/1；`baseline schema` 经可编译行为RED→GREEN，离线无runtime创建且无CLI批准命令，1/1；对应Clippy通过。这些daemon/CLI增量尚未提交，继续接Submit/Edit与原请求GET/retry后统一验收；当前CLI格式拒绝2项已绿，新增真实全链结果见下段。Submit/Edit统一ledger、Svelte Flow有限编辑、B0出版各按既定文件边界并行，尚未完成或发布。

本轮后续仍未提交：Submit/Edit统一ledger与draft.saved、共享outbox不确定性mask、固定HTTP/CLI接线、有限Svelte Flow编辑，以及原批准B0出版。新增真实二进制S6→私有草稿→App编辑/原请求重试→删派生缓存→重启→同画布重读→精确拒绝全链1/1通过（46.75s）；保留先前snapshot竞争及系统阻塞失败记录。真实首条B0出版测试已通过，证明main/tag/耐久created与恢复事件一致且重复无第二次提交；多崩溃窗口、daemon重新打开项目后的恢复和App批准接线继续验证。新的daemon后台恢复复用现有worker，只重试实际reader找到的原批准，Busy或读源竞争保留下次，不生成新人工动作。App候选仍是上次只读构建，当前编辑源码尚未重新打包或实机通过。系统syspolicyd持续占用约280% CPU，新Rust/Node进程长时间未启动，CUA也超时；保留为环境阻塞，不计测试通过，不调整原产品时限或绕过系统校验。

本段收尾记录（均未提交，未重新打包）：五个真实出版窗口 `BeforePublish/AfterPublish/BeforeTag/AfterTag/BeforeRecorded` 的重新打开恢复矩阵1/1通过（205.53s），每个窗口保持同一提交与唯一created。App最小审阅源码已接线，明确人工决定绑定已加载的五字段subject，原UUID/body重试、批准receipt与出版状态分开展示；新增代码仍未通过完整check/test。静态复核另补Interface旧/新端点影响、Flow拖拽原坐标alias防护、回执派生eid逐值核实，以及固定纯App已保存草稿GET（不进入CLI续租/扫描恢复路径），这些新修正仍待运行验证。31文件App最新源码快照逐项31/31 hash核对相符，路径为 `/private/tmp/towermd-flow-review-current-source/`，它是开发快照，不能证明进入产物。Root对自有阻塞Cargo进程只读采样：启动16分钟后仍仅 `_dyld_start+0`、footprint112K、无已装载binary images，原记录 `/private/tmp/towermd-cargo-startup-blocked.sample`；因此新增定向检查尚未进入程序，不记失败断言或测试通过，也不放宽旧时限。更完整的真实hub丢失、publication桥与新审阅/只读边界回归待环境恢复后回收。

## 2026-10-18 按 spec 纠正画布 MVP 路线

用户纠正：“我觉得你没有按spec来开发mvp”。核对主规格§5.4、6.5、15、18与P4/P6后确认：独立自由画布存储和UDS进程认证偏离已批准主流程。已停止并归档本轮23个自有修改文件，校验后仅恢复本任务精确路径到89ea925；没有提交该路线、替换App或触及用户原有未跟踪文件。

- [x] 对照FR-95、100–102、43–48与M1验收列出偏差；独立只读复核确认
- [x] 撤下独立canvas-draft/API/CLI/Unix鉴权及编辑器接线，完整保留归档以供有限复用
- [x] 写入纠偏实施边界：docs/superpowers/plans/2026-10-18-canvas-mvp-spec-alignment.md
- [ ] 沿已批准M0/P4/P6补齐真实候选→同一画布草稿编辑→审阅/批准的实际前置与闭环，不用独立便签画布替代

归档：`/var/folders/2k/_zl87b6512v6sf4q167qvkjh0000gn/T/towermd-canvas-scope-correction-6d5e4dj0/`，含wip.patch、manifest与23文件原文/hash。实验的core/UI/CLI测试不算产品功能完成。当前927b3c5对应App仍是只读开发原型，不是spec定义的完整MVP；本轮没有新增发布产物。

## 2026-10-18 最小完整本地项目画布

按用户“请实现出来，要求功能最小化完整”：本轮交付打开真实 Git 项目→读取实际结构→画布与原文详情→刷新/切换项目→可安装 App 的闭环。已有权威画布读取固定 main；无画布时显示明确的 Git 目录预览，不冒充已批准结构或真实任务。只读路径不启动 daemon、不续租、不写仓库。保留已有显式初始化功能。本轮不扩展批准、调度、实时任务或发布工具。

- [x] core 只读项目概览：权威画布/真实目录预览、固定提交、无提交空态和错误诊断
- [x] native 有限令牌读取，前端选择/加载/刷新/错误/切换完整接线
- [x] 参数化画布，保留权威坐标，详情原文展示及大图导航；分组位于普通节点下方，目录预览每列最多三行
- [x] 针对性验证与 arm64 debug App 打包：前端67项、native24项、core概览5项通过，check零错误零警告，fmt与build通过；最终分组样式及布局后分别再跑App/Picker12项、core5项、native读取1项
- [x] 精确提交源码及记录提交号：`927b3c5effc95ddc4deac3520885e34ead2c88d4`

实机回顾：最终候选 `.release/prototype/2026-10-18-minimal-final/Tower.md.app` 已通过CUA启动并观察截图。先用系统文件夹选择器读取实际Tower.md仓库，展示7节点目录预览和src路径原文，缩放24%→35%；随后把目录预览收紧到三行多列。最终包在自有临时Git仓库读取带负坐标、后置group的既有canvas，显示3节点与关系，节点原文正常；空白拖动的前后截图确认视口平移，适应画布与刷新可用。临时仓库提交README新正文后，刷新从22932e1b切到7f710911，新正文实际可见。没有触发初始化、批准或daemon。此前窗口工具故障已在本轮恢复，不把旧记录抹去。

产物是Apple Silicon本地开发预览，版本仍0.0.0、无发行者签名/公证、非正式release；未验Intel。批准、Agent调度与实时任务展示不在本轮最小只读闭环范围。随包CLI由最终源码重新stage，App和CLI均arm64；bundle文件hash保存在同目录bundle-files.json。

最终补验：同一最终包切回实际Tower.md项目，提交927b3c5、7节点、三行两列目录布局初始适应51%，src节点原文包含本轮overview.ts；截图确认可读且节点详情正常。App窗口保持打开供直接查看。临时仓库在App读取后HEAD、clean status、index字节和canvas字节均保持测试设置后的状态，记录见产物目录ui-fixture-check.json。App为debug包；源码移除后的可选初始化路径未做可移植验收，不冒正式安装验收。

## 2026-10-18 优先交付可见画布原型

用户明确要求“快点看到原型，不要做太多细节”。当前优先级改为可操作的可见竖切片，完整后端与发布验收后续继续。

- [x] 启动直接显示有明确演示标识的目标、模块、任务关系图；保留真实项目入口
- [x] 支持选择节点查看详情、平移、缩放和复位；不提供虚假批准或完成写回
- [x] 前端 check 零错误零警告、5 文件64项测试通过、production build 通过；Vite 地址 `http://127.0.0.1:1420/` 已启动并核对 HTTP 响应
- [x] 原型视觉验证后续已补：首次浏览器连接和窗口定位失败保留；本轮以原生最终候选的实际窗口、截图和交互完成验证，见上方最小完整画布回顾
- [x] 更新双语未发布日志并精确提交：`7e09091`（仅原型与对应文档，未发布）

回顾：原型先交付可操作关系图，无新增依赖，无真实状态写入。5文件64项测试、check零错误零警告、build通过；补充鼠标拖动/释放停止移动断言后，App 3项再通过。实机视觉验证因上述工具故障仍未完成。保留Vite进程供直接查看，不构建或替换旧App、不生成发布tag。

后端联合检查状态：当前 main `6b9c0f4` 的 fmt、all-targets Clippy 通过；workspace 全量按用户最新优先级主动中断，exit -2，源文件前后相等，不能记为全量通过。原始日志与收据保存在 `/private/tmp/towermd-h-main-integration-evidence/03-final-workspace*`。未执行发布。

## 2026-10-17 持续开发至首个可用 App（未签名）

用户追加目标：“请一直继续开发下去，直到发布第一个可用版本app，先不签名”。先完成下方 M0 整改，再按已批准规格推进 M1 技能与 App，完成首个可安装 macOS 版本；签名与公证暂不做。发布前核对所有 worktree、分支、stash 与冻结源码/验收证据，精确提交并更新双语 CHANGELOG。整改阶段继续不推送，发布动作以实际完成的可审阅产物为准，不把计划或工作区测试称为发布。

- [ ] 完成 M0 整改各批与真实 daemon 验收
- [ ] 规划并实现 M1 核心人工闸门、技能/CLI 与桌面 App 的可用闭环
- [ ] 实机验证 App、打包未签名安装产物、核对发布证据与发布记录

当前推进：F/G与R11/R13已有实际主线630项联合；共用验证内核C/D1、扫描规划B、纯草稿内核E、D4/D5逐模块聚合及P8独立发布工具均已双审精确合入。H/V旧7c5冻结曾因旧Status回灌取消首窗口而SPEC FAIL，原337工件与所有失败保留。Q1现已冻结 `5c8042385eb6a3da94ec0ede60b6258e9b04a707`，作者同源26组682全绿、Root核591工件/137源码；整体SPEC PASS和fresh quality Ready Yes均C0I0M0、Root分别核64/41工件。五笔已精确合main至 `8f400774537f773163b4105db28249404d8a0a45`，当前实际main完整联合待执行，不能将旧base作者完整绿当主线联合。R10与完整批4/M1继续推进。版本仍0.0.0，无tag；Task9生产未接线明确硬拒，App/真人/实机发布验收未完成。

App常规实机检查已推进一条：Root以CUA绑定旧Q2 arm64 debug候选，实际点击“打开项目”打开原生文件夹选择框，点击Cancel后返回Tower.md，AX与截图确认按钮仍可用；未选择项目、未触发初始化或任何人工批准。此前启动因来源未验证被自动审批拒绝；先核32固定Git源码、13构建证据及实际App4文件hash/mode/无额外文件、开发staged CLI身份，再用同一CUA入口成功，不绕系统权限或换技术。该候选11项UI相关源码与当前main逐字相同；不将旧CLI/daemon或debug候选冒当前完整发布产物。收据 `/private/tmp/towermd-app-project-task3-root-evidence/ui-open-cancel-verification-20261017.json` SHA256 `853f4e2e824aaadadb9a141012863e029b6fd9f091986895a4e4f8cfdb621391`，截图已在CUA响应观察，未声称有本地PNG。旧cgWindowNotFound等记录保留，不覆盖历史未验结果；这份收据仅覆盖打开/取消，随后初始化进展见下一段。

App初始化实机补充：随后在纯自有临时项目 `/private/tmp/towermd-app-ui-init-0stxkhg_/project` 中，通过原生文件夹选择选中项目，指定文档目录 `tower-docs` 并点击初始化。实际生成配置、项目AGENTS及五个目录占位共7文件；HEAD/tree/main/README不变、cached diff为空，只有预期未跟踪文件，没有自动提交、基线批准或技能安装。初始化前未采index字节hash，不声称其字节完全相同。限定收据 `/private/tmp/towermd-app-project-task3-root-evidence/ui-init-verification-20261017.json` SHA256 `ea253b938c2747ab28772302b584b6297bb49b401d77a40b3187f1460ae421bd`。尝试重新选择已有项目时CUA报 `cgWindowNotFound`，该路径仍未验证；旧候选的来源与范围限制保持，不冒当前daemon、真人闸门或发布验收。

### H Q1：拒绝过期调度观察，保留首窗口和真实完成事实

并行文档核对：已将P5末尾映射表与P7前置/回顾中仍写“首次启用边界待裁决”的三处旧文字同步为用户既有答复及FR-103；只同步决策，不勾P5/P7实现或真人验收。原frontmatter不变、引用路径存在及diff-check通过，精确提交 `539d7ee10412d3a2177da3fdefbff77604fc5538`。

R15独立文档勘误：README的WebSocket段仍称“不压缩事件内容”，与同页现有快照压缩说明及实际daemon的SnapshotRequired关闭流分支矛盾。已按当前源码与既有真实WebSocket压缩测试改为检查点恢复说明，精确提交 `6d5c6932be6998eaaeeaa8d02094bc914eb40fb1`；只修文档，不改协议或新增测试，不将R15全量标为完成。

R15命令帮助勘误（2026-10-18）：固定0a17188的README命令与clap结构静态核对后，修正snapshot帮助仍称main检出时延期的旧说明，改为实际私有引用路径；rebuild继续仅生成请求等待App确认，删除“M0不执行恢复”的过时概括。只改两处帮助字符串及双语未发布日志，不改命令结构或批准权限。静态核原快照Written分支和App确认端点、无同文本测试依赖、diff-check通过；没有运行构建/实际help，不占H性能窗口，编译与实际main联合仍待。精确提交 `9869d978527adb5018042d16cc369e53dc236413`，不记完整R15完成。独立静态报告 `/private/tmp/towermd-r15-cli-doc-audit/notes.md` SHA256 `4f673e18f000d49f0a6ea04ed081c0494ba28c054f5c3c86021a72f0a9b4c6b7`，Root全文读并核五份固定Git blobs，收据 `r15-cli-doc-audit-verification.json`；没有伪造运行help或GUI验收。

独立整体SPEC报告 `/private/tmp/towermd-snapshot-h-final-spec-evidence/report.md` SHA256 `836cb995387e3ca6e3cd2facebdfaa189341898b59fa53af952636643ea163fa`；manifest `1e1982a4e8809763400852091e1edcd8634b67546630efcba8c94a236a15b103`、337工件索引 `87102289c64134d2ffe4ba856ddff8fc83543ee45736e47a72098315eb57132d`。Root已全文读报告、可移植两模型与真实Server补丁和原始失败日志，逐核全部size/hash/执行位及封存闭环；核验 `/private/tmp/towermd-snapshot-h-root-evidence/final-spec-seal-verification.json`。真实hold在Mono1000已核安装并获26000期限，旧Status在20000恢复后，21000常规tick将期限推至46000；26000与31001实际private ref皆为空。另一个同根模型能把真实written的last抹掉。原sourcecache5项及snapshot14项独立绿不能抵消此交错；06两环回EPERM保留，07同命令提权14绿，均非完整workspace重跑。

原唯一writer沿原worktree实施，新证据独立放 `/private/tmp/towermd-snapshot-h-q1-evidence/`，不修改旧316作者或337审查封存件。Root独占文档，先确认以下方案与验收，再开实现；无需再次向用户索要已授权整改许可。

- [x] 移植可移植真实Status/Hub/Git屏障反例，先保可编译行为RED，再作最小私有修复；保持原25秒固定首窗、正常本地30秒真实Git与精确ack，不扩大等待或改变默认线程
- [x] 审核所有install/written/failed消费者及worker尾回写，保证旧观察不能覆盖更新的调度、完成或失败事实；核Status、tick、完成回调、cold Project与Some/None恢复的生命周期。不能只移除Status消费或凭commit字母序判断新旧，也不能假设activate_rebuilt前后generation全局单调
- [x] 明确Session更换/新Schedule reset与并发旧观察的拒绝边界；原Some同Arc/writer/File及Persisted拒调度保持。完成/失败/Busy迟尾以实际Git+ack与重开组合验证，作者与独立双审证据见下文
- [x] 保原轻量准入、source-only缓存、唯一writer与原锁边界，不持Session/schedule/registry锁跨Git；不新增公共trusted/skip入口、Hub格式或依赖。新增失败分类和原件保留，修复后必要默认关键集/静态检查通过
- [ ] 冻结最终同源完整默认workspace/fmt/全targets Clippy与精确提交，原SPEC做整体delta复核，再交fresh quality；双审通过后精确合main并跑实际主线联合，更新双语未发布日志。Q1未验证或未提交不得勾完成，H/R10/完整批4/App仍未交付

Q1实施检查点（未验证修复/未提交代码）：原真实Server反例与两模型已在作者树分别可编译行为RED，日志SHA256 `a39f4bf6fda20a3eaa5608252b8bb1e9952c38da2245c347c90183b2ce614758`、`956ca5f57a3d6690268926ee6c2b5160adfe07be4cfaf50a5a3a6bdf9f231998`，Root已读原日志/命令并核hash及exit101。只读排序设计 `/private/tmp/towermd-snapshot-h-q1-ordering-design/notes.md` SHA256 `09d87b559eff4e8596d1302b876b5a6516f77db4ff4b95ee39b6ff7e87196231` 已全文读、核8固定Git blobs/3来源文件；无新测试，不作代码PASS。已确认实施方向：当前真实Hub摘要与schedule短锁消费；Session仅进程实例身份相等，Some只消费当前实例不无条件重置；实际writer结果与Busy在同一私有观察域取序，普通Status不推进结果水位。Busy取序至owner释放的缝隙、prepare票与新实例失效、附带context/reconcile尾写仍须实际组合验证。Some要求真实空target，不能用已有Written+ack后的不合法恢复时序报第二缺陷。精确范围仍原四生产路径及既有测试，旧封存件保留，尚未关闭I1。

Q1必要组合检查点（仍未冻结/双审/合main）：原三个反例03-first-green通过，日志SHA256 `d6a9d9df50436a234529239c9bb48afb39d954cb180422d68ba994b9e70159ea`，26000实际private存在且31001同提交。中间候选的真实Busy尾组合04可编译RED：owner真实Git+ack完成后旧Busy迟append，owner尾清内存但真实重开alert仍活，日志 `eb0e0379f7b1a81c1ceaaaa23f3ce53f2946f71a3cbb0bb70a99248803e61256`。将成功结果seal紧贴原Hub确认安装短区、同Hub现场核latest_actual_result后，05同组合及迟到completed/failed两向2/2绿，日志 `abbc95117f722e567e2ccd8ba1cc3802fac0be01a7ac5a2f5dda1bf7aaf83d8e`；原reader的W/alert证明不弱化。08合法Some激活后新实例先消费、旧准备失败尾拒绝、真实hold原26000窗口不被Some迟尾重置，实际tick/Git+ack通过，日志 `f74cf418e1f3558825eb29b99368d06ad295f974f9f8df6e7a33a70d8ef133f0`，137输入前后相同。06/07新增屏障未达目标属夹具失败，保留原件；08按既有confirm HTTP10s自然终态/激活二选一，原释放5s及旧产品时限不改。Root已读上述原日志/收据核hash/exit，辅助schema/文件名读取错误单列保留并更新lessons。必要默认回归、完整同源workspace/静态及双审尚待，不把这些中间绿冒最终修复完成。

Q1首完整失败及最小重规划：09核心生命周期8绿、10默认daemon短集42绿/仅唯一hour过滤、11默认CLI48绿（十Status1.770925208s<2s、upload仍阻塞）、12fmt与13全targets Clippy通过；Root已核原日志自然终态/hash，不把child重复输出累计。14-full-workspace自然exit101，仅daemon41/43，CLI/core未进入，原日志SHA256 `a48f9b42ff87f7584c0d4181ba0bb290953576ac72af869a111ce95808588520`。hour子进程0.70s因实际exec已删除的临时Git wrapper而失败，真实hour计数未开始；Root已读原log及冻结7c5原app_late_git_read_io_keeps_failure_category的父全局PATH修改。另一app_control_actor确认HTTP10s超时尚未定位，不能先归资源或PATH。

- [ ] 仅将原app_late_git_read_io_keeps_failure_category完整case按既有H5/I2的--exact子进程模式隔离，父进程不修改PATH；保真Git I/O故障/诊断分类/未变源与refs/保存请求/无目标发布断言和原HTTP10s，退出前停止并drain自有worker。不改四生产路径，不改hour与原2/3/5/10/25/30秒
- [ ] 隔离后默认短集验证原app10s；若仍失败定位实际阶段，不用扩大时限掩盖。原14日志、收据、137输入、tested-source.tar全部保留，新增输入另存；最终自然终态后自动采after，不用过程after冒终态。必要静态/完整默认同源再冻结→原SPECdelta→fresh quality

Q1第17轮中断及接管：隔离后15默认短集42绿、16fmt/Clippy通过，但17完整运行中已出现原I2 B-push5s与app_rebuild/app_control_actor原HTTP10s失败；Router实际Git+ack29173ms仍低于30s。14:07Z恢复工作时，旧agents/Cargo/runner均不在，17无自然终态收据。原log46148bytes、SHA256 `e9c0a7d97522079ec26b2bae9c61b4f64bd1581fbb450707f642af086bb128f9`；hour仅有14:04:43Z中途archive110/worktree108/probe5750，不能记最终计数/通过或编造exit。Root在14:10Z核接管时137源与17-before相同（身份 `b5121ffd3e3cba5ecbc89532fac559b73a8e6d6ba54da4c341bb2561732158a2`），这不是自然after；旧Q1实际83工件独立索引保留，证据 `/private/tmp/towermd-snapshot-h-root-evidence/q1-full17-interruption.json`。

- [ ] 替换唯一writer接管原H worktree/四路径，新增证据独立放 `/private/tmp/towermd-snapshot-h-q1-resume-evidence/`、编号18起，不改旧Q1原件；当前主工作区新出现的未跟踪 `.towermd/`、`docs/tower/` 与原三项一并保留，不纳入代码提交
- [ ] 先核真实confirm/I2阶段与新增Some夹具实际历史。只在证明无语义依赖后，将新增Q1 Some无关Module/AC/Task/Claim/Run材料换成最小合法真实private checkpoint；保App确认/registered-empty/真实恢复边界/旧失败拒绝/newhold原期限/Arc+writer+File及原强证明，不改原FG/App/I2夹具、不串行所有新测试、不降低默认线程或扩大时限。生产性能改动须先给实际计数和最小方案
- [ ] 必要默认诊断与回归关闭本轮失败后，同一最终源码完整默认workspace/静态自然终态，再精确提交冻结与双审；仍未完成H或M0/App

Q1接管诊断与最窄扩围裁决：20新增Some最小合法私有来源实测2协议事件/0投影对象，保全部恢复边界断言通过，但21实际daemon::tests过滤组为26/27、仅I2失败（16过滤含15模型与hour），不冒42通过；22真正默认短集39/42、I2及两个原HTTP10s失败。原生Git Trace2在B同一authority OID窗口实际看到initial/fresh-base/两次observed的四次完整Catalog树读取。每次clock有正常proof/lease阶段，不能称四次重试；23/44/82/40命令是含轻tick的B窗口统计，不冒writer独占计数。只读调查报告SHA256 `41b232d50cd24d9802781ea5e32fe9071fcfc6d9e18b80341247f22db59ca72f`、manifest `d3b6b72300885eb3593939723bf6fab2404ad8fa5b4e88d63e906097024cbd6b`，Root已全文读并核137归档/10固定输入及Trace2所读前缀；它不是代码审查或性能通过。

- [ ] 明确不以worker incremental替换现tick_shared/finish_shared，保持原前后来源证明。新增第5生产路径仅 `crates/towermd-core/src/snapshot_git.rs`：同一次write内freshfetch的base等于request.base时复用initial Catalog；变OID仍原at_revision与上下文检查。复用点继续用既有RecoveryContext::from_catalog核actual canonical common/config/context；保真实来源/私有引用探针、两次observed、完整reader、最高fence、全部lease和原NFF三轮，不能跨attempt/cache持久化或新增trusted参数
- [ ] 先实际完整Catalog物化计数可编译RED→最小复用GREEN，再核不同base、sameSHA actualcommon/config漂移、Module/config真实I/O坏、private变化与NFF负控；保原App late-I/O分类/请求身份与refs保护。测试限该模块私有测试或既有snapshot_service/snapshot_git测试文件，新增对应路径须精确冻结；不得以省reader/guard换性能，不预先承诺5s或10s通过
- [ ] 计数证明最小改动后再跑必要默认组，失败继续按真实阶段定位；最终同源完整自然终态/双审/实际main联合仍待

Q1下一步最小方案C（已授权，代码仍未提交）：A已证明同次initial/fresh-base物化由2降1，但28默认短集仍I2和一个HTTP超时；两次observed的解析复用对空对象B净省0 Git，明确不实施。只在既有第5路径 `snapshot_git.rs` 的 `validate_context` 中，当原配置/对象/路径比较成立，且root、canonical common字段、revision、protected_paths全同，才将相同显式committed输入的两遍读取缩为每path本次真实读取一次；所有读取错误原样传播，不比较缓存vec、不跨调用保存。不同输入域保原两侧完整读取；每点现场binding/private/source/reader/lease和两次observed均不变。对N个普通受保护文件可省2N条Git，B的N=0故直接收益为0，不能预称原5秒通过。

- [ ] C真实committed读取计数先RED→GREEN，异revision两侧各读、first missing/corrupt仍拒；保A同SHA actualcommon/config/Module I/O负控、完整Git54与原App late-I/O。必要默认短集后按真实失败重规划，不盲跑全量
- [ ] 若需测量repo等待开销，仅诊断overlay记录spawn/EOF/try_wait时刻，不改等待策略或时限；单独归档patch及自然前后收据，正式候选前恢复。原5ms轮询不能静态乘次数冒实际延迟。只读native Git启动选择调查独立进行，不运行Cargo或并发性能采样，不授权实现

C中间验证回顾（未提交、未冻结，I2仍失败）：37同输入实际读取2对预期1的可编译行为RED，日志SHA256 `14d916c21be2f7eb35f53905661a2595af6a13dc7b8b3dfd2979098bcc71807b`；38私有8项GREEN，含实际读取1、异revision各读、first missing/corrupt拒及A负控，日志 `8e421caf5972aaa9a288f1e6b5722e13f7ac09af240b65539df899deff0ff494`。39原Git54全绿。40默认42短集41绿/唯一I2 B进入push原5秒失败/仅hour过滤1，原两个App HTTP10秒及late-I/O均通过，日志 `5dddac599ec006890e89e185faf4b920300872a83d0f14b8b90b40fb19ac06fb`；不把child重复日志累计或将此轮HTTP绿冒稳定整体通过。Root已核27/28/30/31/37–40八轮全部137归档成员/hash/执行位、源码前后相同、自然终态后采集以及构建输入等价（仅采集时间不同），收据 `/private/tmp/towermd-snapshot-h-root-evidence/q1-resume-through-c-verification.json`。辅助核验漏归档source前缀的准备错误单列保留，不归产品失败。继续最小I2阶段测量，不进入完整/双审。

I2测量裁决：42仅原I2 --exact诊断自然绿，不冒默认42绿；127个bounded Git的native exit和父进程观察逐PID匹配，pending poll为0，5ms sleep未执行。Root独立重算后已核43将repo诊断overlay恢复，137候选身份回到 `01bf6aabeb6fe97950d61008431816f3cfc5f8d668480d9ad1613a65aa47fdc1`，不改等待策略。原native Git启动选择调查因PATH wrapper、工具链切换与环境语义未证而不实施；同probe成本调查确认看似重复的绑定读取跨过真实材料/锁/副作用边界，没有可安全省略的新调用。两份设计分别SHA256 `a97f4ce0a2fd391a79168b24052e8281664f96aeeafb01b2bf34d93b2a069ae6`、`a47147a6cde172177a276ac0697ab007ad8cdb9ee74a4d0058dc843fd233106d`，Root已全文读并核固定输入，不作代码审查。

下一步最小方案D（本计划确认后实施，仍第5生产路径）：私有引用不存在时，每个观察点原需symbolic-ref和for-each-ref两次Git；可支持的Git改用实际 `show-ref --exists` 的missing退出码确认Git引用缺失，每点降为一次。存在时仍用原精确refname/symref解析及独立object type读取，三条总成本保持，不顺带合并type。Git语义的missing不代表物理raw文件不存在：非refs目标的悬空FS symlink、空目录等，原reader也可能None。不能宣传全面损坏ref检查。所有跨阶段private/source观察点、reader、最高fence、租约和CAS均保留，无ref/材料缓存。

- [ ] D先做实际无private材料调用数2对预期1的行为RED；只读help正常129且明确advertise选项时启用优化，正常129无选项才走原完整路径，不提高最低Git版本。help真实spawn/read/timeout或异常exit原样Err且不缓存失败。进程内保守能力bool仅性能提示：锁外探测后set winner，不持全局初始化锁跨Git；不缓存exe或绕过当前PATH。true后换成不支持的Git明确Err，需重启重新探测；false后升级继续原路径至重启。Root接受这一保守兼容边界，不新增配置/force/reset
- [ ] D实际probe仅文档missing码2、空stdout且空/正常missing诊断可None；单条命令局部LC_ALL=C固定诊断，其他lookup/fatal/129/信号/超时/异常输出均Err，不fallback掩材料失败。exists0后无exact记录必须Err，dangling/非commit仍拒。fresh-child验证能力两分支及true后工具变化、故障分类、损坏packed refs、missing/corrupt objects、普通/悬空symref、FS旧语义和无用户状态写入；原54 Git/App late-I/O/原5秒与10秒默认验证保持，绿后再最终完整/冻结双审

D中间验证回顾（仍未提交/冻结）：49实际材料调用2对预期1行为RED，日志 `eff50fb0ac5914f2e2d7b4f936d42993f413f1caff5b99ffa0d823e8d8dc2809`。50实现误把正常帮助仅视作stderr，实际stdout帮助触发Err，属真实实现失败并保留；修为有效129单一通道完整help后，51实际absence调用1绿。52十个fresh-child边界绿（父只计一项，不累计child），53私有10/10绿且Some实际三调用，54原Git54/54绿。55默认42/42绿、仅hour过滤1，原I2屏障4703ms小于5秒，两个App原10秒及全部恢复/旧尾负控通过，日志 `8231daf5bceb395dfc6cb52f09cca5c529eb5ae925935ad422f6511e621f68f9`。Root核49–55全部137源/归档/hash/执行位与自然after及构建输入，收据 `/private/tmp/towermd-snapshot-h-root-evidence/q1-d-through-default-verification.json`；53–55源身份同 `fa06fb44ff5515265b918a9636e060837c6fb31946d97ad1fac0beb32019a37a`。56fmt、57全targets Clippy已自然绿，58同源完整默认workspace进行中，不能以live计数或短组绿勾完整完成。

后续R10只读预备已按固定40候选映射全部入口/prepare、Busy及实际seal后的context/schedule/alert尾链，明确Q1序域/Session实例并非生命周期permit；冷None续租、WS blocking和MainRefresh common仍须真实排空。报告 `/private/tmp/towermd-r10-q1-lifecycle-preflight/notes.md` SHA256 `f9586b4a50d63c60d2be0e9038101b418d2cc8d8facec69f96b8328fc558dcaa`，inputs `e86388a035a71f3bad776b227ec1d088d1003646957e73168ba09813533f6e20`；Root已全文读并核两旧设计、两正式Git blobs和六固定源。仅为下一步预备，40不是最终H；最终H双审实际main后须重核，不授R10实现/Ready/PASS。

58进行中发现新增Q1 Some测试确认HTTP10秒超时，原I2已绿，不能记完整通过。已观察Some writer实际8.884秒，activation人为暂停区还执行inputs/tick_shared/真实Status的额外Git，不能先归为资源问题。保当前源码直到58自然终态并采after后，才作以下既有daemon私有测试内最小调整：暂停区保真实激活Session、当前Hub消费、旧真实票拒、真实protocol hold；用同一生产 `consume_snapshot_schedule` 在mono1000现场观察并安装原26000窗口，核25999/26000。release后在任何重同步前立即核原Arc/File与26000窗口，防迟尾错误被后续Status自愈掩盖；再将原完整inputs/tick_shared/真实Status移至mono20000，保原deferred/null和26000断言，最后原26000实际Git+ack。原HTTP10/select/release5不改，不删reader/tick/Status或用裸摘要替代；先定向和必要默认短集，再最终同源静态/完整，不盲重跑。

58自然终态已确认：exit101、daemon42/43，仅新增Some超时，后续workspace组未进入；hour本身自然通过，attempts64/catalog64/worktree195/archive197/source_probes10863/alerts1，726.26秒。日志SHA256 `5238bd8db3ad5c23a5205894f6242ba4721eb7265b6dfceadaa01dbed439c721`，Root已核56–58全部137归档/前后同源fa06及自然after和构建输入，收据 `/private/tmp/towermd-snapshot-h-root-evidence/q1-full58-natural-verification.json`。中途计数均不冒终态。

59按前述方案实际失败于 `fast_pending=None`，并非HTTP超时：真实协议hold只进入原始Hub，尚未经Session完整sync认证，不能由consume代替证明。保失败原件，在既有测试暂停区补回必要fresh inputs及一次现有 `ProjectSession::sync_shared` 的完整actual binding/current sources/reader/proof/CAS，实际安装hold后才消费并核26000；不改生产sync、不用incremental或裸摘要。只将原tick_shared剩余expiry/finish及真实Status移至release后，保先核迟尾未reset及全部原断言。加inputs/sync/暂停区实际耗时记录，先exact验证预算，再必要默认组；原HTTP10/select/release5不变。

60该完整proof时序exact通过，61默认42/42通过（仅hour过滤），暂停区fresh inputs55ms、含完整sync总502ms；62fmt与63全targets Clippy通过。Root已核59–63全部137归档/前后同源、自然after和构建输入，收据 `/private/tmp/towermd-snapshot-h-root-evidence/q1-some-verified-pause-verification.json`。64同源 `059a67c4baa40e3c98c105d484ecdd805456175226d6ffaca8389d990fc4ede1` 完整默认仍运行：Some新时序及原I2均已绿，但原Busy强证明5秒未到实际private CAS失败，不记整体通过、不改该原测试的门限/断言。完整终态前不编辑受测源。

64封存后仅授权诊断overlay：既有cfgtest阶段trace也对BUSY_CHILD输出，并在原5秒until创建之后记录wall-ms和剩余微秒，不移动原Instant或延长时限，记录时钟映射精度/日志开销。先Busy exact及原生Trace2定位pre-CAS实际阶段，若需默认诊断另据证据决定，不盲full。先存完整daemon字节/hash，归档精确overlay，最终仅撤销诊断点恢复同一137源，不能checkout旧HEAD抹Q1。E候选仅调查已有引用时foreach实际objecttype材料读能否替代独立cat-file -t（Some3→2、None1、false旧Git原路径）；等待真实Busy预算后另行裁决，不删除reader/source/lease/CAS证明或缓存引用。

64自然终态与E裁决（2026-10-18，未提交实现）：64自然exit101、daemon42/43，仅原Busy5秒未到实际private CAS；Some新时序、I2及App10秒均绿，后续workspace组未进入。hour自然724.17秒、64次尝试/一次提醒，日志SHA256 `b353e9fbbec9c44b2c3b8c290985b573966c4e652cb31b03121802e0045888b5`。66 exact诊断绿仅作预算：真实5秒起点至CAS启动2791.245ms，pre-CAS304个Git，其中26次同实际private OID的独立type读取；不能据此称默认5秒关闭。67精确恢复两诊断点，137身份回到059a67c4。Root核64/66所有137归档/自然after/构建输入及恢复，收据 `q1-full64-busy-diagnostic-verification.json`。

E只读三份设计已全文读、核20份官方固定tag源码及冻结成员；`selection-supplement.md` SHA256 `1a33f3e4e85806433cfa6122b3dcc8f0be6233c7df09d2c8288ec47d8edb86df`，Root收据 `/private/tmp/towermd-snapshot-h-root-evidence/q1-objecttype-design-verification.json`。65b证实普通typed foreach额外读取坏child会新增拒绝；count/sort不能跨版本解决，不采用。69 literal尾斜线exclude自有Git12case原新分类相同，含nested missing/corrupt、exists后exact消失及exact坏对象；SHA256 `054d88c0f2553cffec7b638fec51c473c010acd468fed5f085c8f51608fd5aae`。这不是生产或性能PASS。

- [ ] E仅改既有第5生产路径snapshot_git.rs：captrue且exists0时，foreach加入 `--exclude=refs/towermd/snapshot/` 与非星号objecttype；命令成功后严格校验exact四字段、非空OID、空symref、type精确commit，以本次同OID真实类型读取替代独立cat-file。Some实际3→2；None仍1，capfalse完整legacy不变；不加count/sort/glob、能力框架、fallback、材料缓存或public开关
- [ ] 实际Some调用数先可编译RED3对预期2，再GREEN；真commit/tag/blob/tree/symref/missing/corrupt loose/packed、exact消失、子代/孙代及非零partial stdout/真实I/O错误回归。保D能力/工具切换负控、原Git54及App late-I/O分类；每个来源观察点仍真读，完整reader、actual binding/config/source、最高fence、lease和CAS均保
- [ ] 接受查询范围缩窄而非全存储fsck：exclude可能跳过后代packed记录的某些诊断；合并也不保稍后独立进程的物理对象读取时刻。原契约要求每次观察实际验证对应OID类型，不要求额外独立进程或无关后代全面检查；不宣传全行为等价。exact及实际遇到错误仍Err，后续完整快照校验不删；不提前承诺5秒收益
- [ ] 必要默认组及同源静态通过后，再完整默认workspace自然终态；失败按实际阶段重规划。最终冻结整体SPEC须覆盖A/C/D/E与Q1，再fresh quality及实际main联合，仍不记H/M0/App完成

E中间验证回顾（仍未提交实现/冻结）：70同实际Some调用3对预期2行为RED→71实际2 GREEN，Root核两轮137归档/自然after/构建输入。72异常输出组名字虽red但实际exit0，不计RED；73真正非末行CRLF exact输出被lines规范化后错误Some的行为RED，74仅现代split保CR后，另一个新增legacy invalid-ref测试预期Err不符旧None行为而失败，旧生产未改；75修正该预期后完整私有11/11绿，含D十freshchild分支、现代Some2/None1、legacySome3、真实tag/symref/missing/corrupt/packed、exact消失及子孙代、partial stdout+非零及字段异常。type只读header非fsck的边界另验证：坏body可被type接受，但后续统一完整snapshot reader仍Err。75源码身份 `2ac6549b5164581d972d17594002e4d9deef99afdc057923c1c2b9d9aec2fbbf`，日志 `78f5f376fae707f1216f24dc5c375d6cee67b7d075a5942e757537184edf587b`。

76原Git54/54、77原App late-I/O三处真Module读取故障分类及refs/request/target保护绿；78默认42/42、onlyhour过滤1、48.16秒，原Busy/I2/HTTP时限全保，日志 `f93ddbb7b069bc2efef0e57369325bb49c4cf989b03a1b6be041daf07015f937`；79fmt/80全targetsClippy同源绿。Root已核70–80全部137输入/归档/自然after和构建身份，独立核验收据以 `q1-e-` 前缀保存于Root evidence；辅助误猜77文件名的准备错误另存，不归产品失败。81同源完整默认workspace正在运行，不提前报通过。全绿后唯一writer可精确提交五路径、核最终137 Git字节/执行位等受测归档并封存manifest，随后仍需整体SPEC→fresh quality→实际main联合。

H Q1最终作者冻结（未合main）：精确提交 `5c8042385eb6a3da94ec0ede60b6258e9b04a707`，tree `082407d7725d2194c2715ca1cd557ddd727cc142`，父7c5afa5b，仍原五路径、工作树全干净。81完整默认自然exit0，26组682/0/0/0；日志SHA256 `2d8aa9e4e24a3a9b50b43f0f0792b6827ffde61fac22baebc1029a95cf098847`。daemon43、core单元215、CLI48全部通过；hour真实64尝试/1提醒，十Status1.886469375秒且上传仍阻。75–81同137源码，fmt/alltargetsClippy及原Git54/App late-I/O/默认42均自然绿。Root独立按Cargo外层终摘要重算26/682，核自动自然after/构建输入/所有137最终Git与工作树/归档/解压副本字节及执行位，原repo/Busy overlay已撤回。

封存 `/private/tmp/towermd-snapshot-h-q1-resume-evidence/82-manifest.json` SHA256 `be61a6ce9d15efdab658a10d671819ab7a70ae6642c3ad868b9b17e9114a7c46`；索引 `009491a534e2854994a730f939fe3179f07c4b36fe9beb9c2f876cda173f982a` 全591工件核hash/size/实际集合；交接 `d4cb15ef43fb4c15d1beb792e6e5be4f7491b1a669fa78602150c456228355ca`。Root已全文读交接/全失败分类、核精确五路径和两基底完整patch，收据 `q1-final-freeze-verification.json` 与 `q1-full81-independent-counts.json`。已以完整最终SHA和manifest正式START独立整体SPEC delta（原Hbase→final，含A/C/D/E和原反例），尚无PASS，随后fresh quality与实际main联合仍待。不得将此作者完整绿或已提交冒H/M0完成、App可发布或实际main联合通过。

H Q1整体SPEC delta已PASS/C0I0M0：报告 `/private/tmp/towermd-snapshot-h-q1-spec-evidence/report.md` SHA256 `442f5c9cffa277402475c149071f1175e18ec3f7532919d78f1ba53a563e72e1`，manifest `909769df57a9650644a916d32289972ea66c49884c1b55f6e33439e733ba540d`，index `e57453b253b93e7b10027154fc22de070b3751c1300bc4a651b82f91c4cec977`。Root全文读并核64工件及seal、137 frozen/restored字节与执行位、10命令自然收据与固定权威Git输入，收据 `q1-spec-delta-verification.json`。独立原portable3/生命周期3/强化portable与实际旧准备失败迟尾重开4/A-C-D-E9/Git54/默认daemon43仅hour过滤1/service40/cache5/authority锁8均通过；03仅新增审查测试E0716编译适配失败，clone实际ack修测试后绿，原失败保留。原I1已关闭，无新增产品行为失败；原SPEC失败报告不覆盖，不冒本审独立full/hour。已启动新鲜整体quality，仍未合main或发布。

R10并行只读接口草案已产出 `/private/tmp/towermd-r10-final-h-interface-design/notes.md` SHA256 `474675f35a672d5bcd3018832e3def08f66b94f08dde8f425441d8cb9779ffb5`。Root全文读233行、核最终H六源码与Root计划共7固定Git blobs/四旧设计；这是未实施未测试草案，尚未整体采纳。已要求另补真实同步调用链如何闭合async排空/Opening真实join，避免嵌套runtime与额外框架；旧common已属新ID的刷新在必要封口后恢复、迟票不得复活、同common短暂fetch暂停与跨common照常须明确。最终H双审实际合main新SHA后的重核门槛不变。

H Q1 fresh quality已Ready Yes/C0I0M0：报告 `/private/tmp/towermd-snapshot-h-q1-quality-evidence/report.md` SHA256 `26d2ad5a72fd46ef48a6f7a03efaa050c45f7c7b49e3d9a44263d3ed5858cbfb`，manifest `0de9ec9b6ae82fba08a5917a651f297ea4ca88e30a8c341cfc91b6078356dadd`，index `efce9b28f0eee93bcd13ff72ab8431bd534870358b14a766118279eed7e78275`。Root全文读并核41工件/137恢复源码/十命令自然收据，独立77通过；新增真实旧成功迟尾→新CAS失败alert→重开仍在→实际Git/exact ack与原Event归档→二次重开关闭组合绿。05审查夹具从压缩前序号订阅正确SnapshotRequired，修起点后06绿，原失败保；07/08短重叠仅作行为证据，不冒隔离性能。没有新产品缺陷，收据 `q1-fresh-quality-verification.json`。

H五提交已精确合main：来源27373a89→`6c3604c53d31cda9bd2bd883fc0658c78c1d7a8d`，a97a2659→`f2607082a828757eac2940a5c1fcab9bb17b1ac7`，02ee33fe→`d91691edea6b0e17cfcd5e0dc2dcd6fdf4d6c827`，7c5afa5b→`91608f1190f44c1ca0a4d1ce3320ce1cdb8d7b33`，5c804238→`8f400774537f773163b4105db28249404d8a0a45`。Root逐提交stable patch-id相等，整体11源码除已单独验证的两处CLI帮助文字外逐字等冻结，其余main路径不变；不是仅凭祖先关系。README与双语未发布已同步快触发/状态/失败提醒；当前实际主线联合尚待，仍不能用作者旧base682冒当前main通过。证据目录 `/private/tmp/towermd-h-main-integration-evidence/`，已有 `main-source-integration.json`、验证运行器准备及10个既有用户未跟踪文件的pre-integration指纹；没有版本/tag/push/release。

### 批3 H/V：调度与全状态快照展示（正式实施计划）

权威仍为已批准批3 H1–H7/V全文。F/G实际合入主线c30bf905已完成630项联合。Root完整读并核新只读报告 `/private/tmp/towermd-snapshot-h-post-fg-preflight/notes.md` SHA256 `29e3d990628693ed948eb67fe67649409a0267ff5841fd0e35b208c54c962478`、17输入及原始Git blob；旧restart设计SHA256 `982bf37c4e1ae8df3fff0e4779f086e430f4dca9ccb1d03c9a15611be38a64a4`继续保留。两份都是设计依据，不是实现/测试通过。新鲜唯一writer在本计划提交后的实际main开独立worktree/target，Root独占文档；R10/App读adapter不得并写daemon。

必要生产范围：新daemon `snapshot_schedule.rs`、main mod、daemon接线；core `project_session.rs`全部成功安装摘要、`recovery_git.rs`现有唯一history提摘要、`snapshot_service.rs`同writer阶段事实/到期内部入口、`recovery.rs`纯source状态DTO与专用可删缓存。对应既有project_session/snapshot_service/daemon_cli测试与私有单元模块。recovery不改confirm审批/安装边界，catalog/R11、snapshot_git三次算法、Hub公共事件/opaque/state格式不改；额外真实计数点如需扩大文件范围先说明必要性，不造生产故障环境、公开skip/trusted参数或第二reader/writer。

- [ ] H-A：纯私有Schedule及可信双时钟；25秒固定首窗口、seq去重、stale/self忽略、指数退避饱和与interval上限，Unix since首次真实失败固定，手动失败/换原因不重置；保原H2实际方法断言，未知written_at为null，不取查询/补ack时间
- [ ] H-B：Session成功安装后发布带安装代/cursor的摘要，覆盖new/shared增量/full install/checkpoint/staging激活及FG Some/None；唯一history的actual archive+tail提取，originals混有outbox/内层proof不能直接当installed流；同writer结构化记录preflight旧来源补事实、真实Git Written、精确ack/install，保旧write/String外壳与全部proof/fencing，内部到期不被旧normal-due吞掉
- [ ] H-C：status.snapshot固定last/reason/since覆盖active/recovery_required/authority_unavailable。专用source-only缓存按canonical common/root+实际authority/private+configcontext+既有inputs digest，命中前后探测、锁外miss复用status_source/read_recovery_history、来源再核/CAS；读错不回旧缓存、不创建Project/Hub/租约。实际归档计数证明静态命中零重读、删除重建同值/来源变化失效/并发不装旧摘要且不挡其他项目续租
- [ ] H-D：tick保轻量mainlease维护，抽用现capture/probe/CAS作轻量同步，需完整准备仅标pending，不凭raw head设deadline；shutdown/single-flight/eligible/next_attempt先于Catalog全量/列worktree/读archive，统一原slot完成准备、due/reconcile/event/重试。手动立即尝试反馈同schedule；两个Project构造及Some恢复成功重装摘要，Persisted失败仍拒写；不持session/schedule/registry锁跨Git等待，关机drain实际writer
- [ ] H-E：真正新事件已核后25秒、正常本地30秒内真实Git+ack；heartbeat不快触发但原周期压tail，停renew仅self事件不自激。真实1h tick函数计数/一次提醒、pre/post-alert完整重启、同Q私有搬送、旧来源晚补W后本次Git仍失败、Duplicate保原seq/t、Git已持久ack失败及FG所有权/慢proof/关机回归；纯模型、core、Router和CLI证据分开。只Q>=alert或只W>alert均不能判成功；alert附basis引用实际来源/待覆盖前缀/搬送目标，无新事件type，已提醒episode从已核tail/归档恢复原since/eid，事实不足保旧提醒抑制重复
- [ ] H最终静态完整默认workspace、fmt/全targets Clippy及原时限，精确提交冻结→整体SPEC→fresh quality→合main/Root文档；重启前无持久alert时可清退避并在下一次真实失败重新观测since，重启本身不制造失败。双时钟活进程deadline不随Unix跳变，重启按真实Unix锚一次换算有限剩余并固定；不承诺跨任意墙钟跳变恢复旧Instant

实施保持原2/3/5/10秒、Git30秒、25秒/30秒产品断言及Cargo默认线程。计数必须到实际函数/子进程阶段，core依赖不会自动继承daemon的cfg(test)，不能用不可达hook或仅Schedule调用数冒真实I/O；夹具先精确root/cwd/argv匹配，不改父进程全局PATH/git配置，清理先停并await真实worker再删自己的目录。首轮RED必须可编译且命中行为；全部失败分类保留，不重跑覆盖。每个已验证独立切片精确提交并注明完整H仍未交付，最终所有实现字节冻结后只做必要联合与双审。

H中途检查点（未整体审查/未合main）：H-A `27373a895de9ae8834296571103967a436ab99d0` 仅私有模型与mod，五项可编译行为红→绿；H-B `a97a2659b1e22c51acc58094e9f64d8727590625` 为Session/history/service与两测试，作者默认相关60项绿。H-C真实归档计数/来源并发CAS两项已绿；H-D已保旧tick真实首25秒未写RED，统一slot接线后实际private Git+精确ack GREEN。后两部分仍未提交，重启/周期/一小时实际计数/阶段反例与最终整体验证仍待。Root已读H-A冻结源码和原始绿收据，未将作者中间结果冒整体H完成。

H实施补充边界（仍未最终冻结）：区分实际已持久checkpoint与结果未知，失败None不抹已有P，也不把未知升级成已知无写入。真实Git CAS已成功、writer尚未返回时，Show可能先安装新来源；其后Busy提醒必须引用同writer实际冻结的inflight checkpoint/source/hash/目标，不能只比较展示last的commit。writer私有RAII guard先清inflight再释放原写入锁，不提供public setter/trusted开关。Root已读并核真实重开RED `h-post-published-busy-red2` 日志SHA256 `79a5e41686790830dbc65a7c8d6840381bc5fd22a132ac34d328a92c99db2370` 与GREEN `h-post-published-busy-green` 日志 `d14ddb0c34e9cb4ccbc8633b137cab46e70b0057d914142136ba732b88141bf2`、自然exit101/0；早期Status不安装来源的fixture失败单列保。旧Q完成后不得重置更高seq已授予的25秒Mono deadline；已编译命中真实hold反例 `h-later-hold-deadline-red3`（原35000到期的eligible检查失败），仍待窄修及最终联合。五核心反例、guard生命周期、默认短集与最终workspace须在最终同源码联合，当前不记H完成。

H后续回顾（仍未提交实现）：原later-hold反例窄修已绿，日志SHA256 `6e2ed3ebc2fb2cdb4b676c281a6aa67662724463ec409fdc097dab873d9ff0aa`；该早期定向命令漏`--locked`的限制保留，实际lock字节未变，不改原收据。原默认34短集同源码exit0，日志 `9387e1c6ef4904cde717746dc20e704d0c82f19d82bdb63fcbf786b677fda740`；继后的完整v3为exit101，日志 `7a2fcc509bda6875d533823f144dc97c209c7de72037de0d18a113fc2ee17f76`，一小时实测attempts64/catalog64/worktree195/archive197/source_probes10993/alert1，不将daemon35通过冒完整workspace通过。新增实际前缀反例first3/max4/Q3的旧25000期限丢失，RED `6f13db7a09a90f160e3c6ae5c72d7ca0e3d0332ed9f2acd5f9a6f07d7faae9f9`；同一真实case、原later-hold及min/max小表3/3 GREEN `bc95f0f92ff9925f8e0f109b5c163ac03e1752261989066c967c4cf6d0c69a83`。窗口只保存已核fast首/最大seq与原Mono期限，完成Q只覆盖前缀，合并当前真实已安装summary的max后判断，不新增Hub字段、信任旗标或第二writer。CLI须从真实checkpoint/system-state及尾事件核活跃租约与原fence/owner，保原10/3秒和停机drain；尚未证明失败根因已关闭。上述日志与实际命令收据位于 `/private/tmp/towermd-snapshot-h-evidence/`，整体静态全量及双审仍待。

H新候选必要联合已通过（仍非完整验收）：`h-short-workspace-candidate` 默认26组663/0/0，仅原唯一hour filtered1；原始日志SHA256 `1175214619732535e1ce31fb3af15e63e4de1da994409d33d0a338490784fdab`。其中daemon36、CLI48、core203、snapshot_service40均通过。CLI实际B仅剩snapshot.written尾而main续期已进checkpoint，按原seed lease对比holder/epoch不变、expires_at实际推进且大于当前时间；折叠Grant/Renew/Release/Expire，旧fence或stale不得覆盖当前租约，补到期/换owner/换epoch/释放负控，保原10/3秒、真remotehook、一push、stop300ms仍阻塞及release后remotehead等于真实ack。纯模型另补重复同Summary和live首次核新事件开窗，不把未知事件当已授期限。Root全文核终态收据/日志、独立重算26组663及137源码前后逐项相等；同源fmt-check与workspace/alltargets Clippy `-D warnings` exit0，核验 `/private/tmp/towermd-snapshot-h-root-evidence/short-static-candidate-verification.json`。同源码完整v4正在运行，过程中不改受控源；冻结、整体SPEC、fresh quality与实际main联合仍待，不能引用旧base检查声称当前main已联合通过。

H v4终态及最小重规划（实现仍未提交）：完整v4自然exit101，daemon37/37含真实hour通过，CLI47/48在原10并发status总elapsed<2s失败，core未运行；日志SHA256 `c4c5f3cdad9c394ac194acdd19c2044fb33f4ba3a8a48016b2dd37c9b770d849`。test-only默认48诊断复现2.05451525s且upload已进入/未解除、release不存在，原日志 `92d1963d69d6a69be1c9c1556f2b2b8b25bc7b7958778789acac87ef1a9be080`；不是等待fetch完成的证据。只读设计报告 `3c7c3684dfe8141d0e1e65eeea46a751d1198c1be7100b5ef4a350d295e5b36f` 已由Root全文读并核34工件/3固定Git blobs；其中“sync跨Git”误读已明确撤回，不列产品缺陷。实际config blob计数可编译RED为2（应1），仅将Key.context第二次from_catalog改extract后为1；同一次inputs.probe仍完整核actual common/config/context与实际目录digest，前后Key探测、唯一reader、generation CAS均保留，无新信任口。sourcecache专属miss/hit后sameSHA换actualcommon、actualconfig/outbox中途漂移拒绝且不装旧generation，最终5/5；首matrix的config夹具误在非权威分支提交，单列保留，改为真实推进actualmain并断言后再验证，不冒行为红。最终默认CLI48/48、原2秒/10并发、全部JSON已核后的总elapsed1.788635333s，真实upload仍受阻且最终counter1；日志 `641d1644663923d6352f01c04135273887530cb4fd89c75fd4df329b85c755bf`，137源码前后逐项相等，fmt与全workspace/alltargets Clippy `-D warnings`同源exit0。Root核完整原始日志哈希及终态收据，记录 `/private/tmp/towermd-snapshot-h-root-evidence/context-reuse-cli-verification.json`；新完整v5在固定源码运行，不能借旧663或v4的hour冒新完整通过。

H/V最终独立实现冻结（未合main）：tip `7c5afa5be766892a1dbc008867dd2b10376f0d60`、tree `6e5809a764c986c1002bac02ff8930fdd4c0eaf8`、base `7e9ccd1e95a711e73ed01ac30ab63222317dad8f`，前三条H-A/B/C加本次D/E精确8路径提交，整体delta严格7生产/3既有测试路径。最终v5自然exit0（12:51:54–13:09:41Z），26组666/0/0/0；原日志SHA256 `6bdbba3baf7a4e0d7d0ab7b5dc0fd34c78627c89d5106e3a9dfa3475574ba4e4`，含daemon37/core205/CLI48，hour实际3601tick、64尝试、一次提醒，Router真实Git+exactack28428ms，原10并发status1.792198584s且upload仍阻、最终fetch1。测试时真实HEAD02ee加8WIP，事后final commit137份Git源码bytes/执行位与原CLI/full前后输入逐等，实际工具链/lock/配置/白名单环境前后同值，未把测试归到未来提交时间。Root全文读交接并核实际Git137/安全tar137/精确4提交10路径/316件size/hash/mode；source.tar `a0336207c8f9f1dad6067b419162bf19cdb917614d9a128ad6aef16aa6c043a2`、artifact-index `16cf1a05a0a42721c8dfbf6e2a4c410b64cfa3d67b41f5606adb2025e2f4eb83`、交接 `b90d9ce185abf26121736351105bdbb87bb35bd245c4dcb77e8c73d4ee792845`。核验 `/private/tmp/towermd-snapshot-h-root-evidence/final-freeze-verification.json`；Root初补丁校验额外加入full-index导致仅index头缩写不同的辅助失败已单列，按原producer命令完整patch逐等，不改作者证据。新鲜整体SPEC已正式启动，未给PASS；须随后fresh quality和实际main联合，不能将旧base完整绿冒当前main或M0/App完成。

R10实施计划已按新只读设计替换原Task1B的锁内Git/双Arc计数/remove代码样例：连续移动同一个实际File，退役封所有入口并排空真实请求/WS批次/worker/恢复与逐common fetch，失败Opening保锁及实际持久阶段。Root完整读设计，核固定ab2480b的五原Git blobs；报告SHA256 `303f6bedb8697ad4643530ca0ffa8443769016222f95f0fc5185c67b225948f7`，Root核验 `/private/tmp/towermd-clone-r10-root-evidence/lock-transfer-design-verification.json`。仅规划，未实施/测试；H实际合入后仍须重核新SHA再开唯一writer。不能把文件路径/inode相同或Arc引用数当连续锁/排空证明。

R10的H生命周期提前映射已追加正式Task1B，仍非实施/测试：报告 `534263abbe21ffbc69ae0e875b96552dba0fa12868c69b6e5c031589a659e5e3` 与补充 `5602bd45ceef2a46efd78781ba1631c03c7b62a2062c9944cff65d46a43249dd` 已由Root全文读并核37输入（31份v4事后匹配源码、manifest、两旧设计及b311b60三Git blobs），根核验 `/private/tmp/towermd-clone-r10-root-evidence/h-lifecycle-mapping-verification.json`。新增接线要求覆盖轻tick已抓列表/slot竞争、worker最后schedule/context/alert回写、WS实际blocking批次、未来跨代Status原Arc回写及没有Project时的冷读续租真实File保护。原报告过宽的锁禁令由补充纠正：保原writing mutex跨Git/完整Some确认，保Hub唯一Journal短定序/fsync；协调者不持这些锁等排空，业务/维护是两类完成条件而非强制两个计数器。仅基于v4匹配副本，未声称覆盖v5全源或H缺陷；H实际合main后仍须新SHA重核，不提前开R10 writer。

### P8 Task9：发布工具与本地验收收据（正式实施计划）

原收据草案SPEC FAIL的F1/F2完整保留。Q1仅修文档职责/固定来源材料绑定及点名歧义，独立delta SPEC PASS、无阻断；报告SHA256 `7862098b8fb9c8dde8894b3efb72ad16eb1068ced5cd4a35bc787adc62ccd832`，索引 `581b9e45f464f04494d7f4b7720d6014e8b8a7b9523f74abb14e30c6a6487f7c`，证据 `/private/tmp/towermd-unsigned-publish-task9-contract-q1-spec-evidence/`。Root完整核Q1草案与最终契约、15旧材料/副本和14审查输入；P8计划精确同步SHA256 `e15ed884e3a1e9ee589a0e6f87bf6fa4b72d89fc9739b8a9cd75a428920a21db`。这只是文档契约通过，不是工具、P7/Task8真人或发布通过。

新鲜独立writer从本计划提交后的实际main开隔离worktree/target，按P8 Task9及新增“本地验收收据契约”实施，仅新publish.mjs/publish.test.mjs与core.mjs/core.test.mjs必要共用门禁；Root独占文档。四参接口/既有CLI、九公开资产不变；若有限材料检查确需独立模块，先说明必要性。不调用collect/虚构未来proof API、不增加第二业务证明器；只核实际固定字段身份/角色/原文hash，Root负责真实P7链与等价/真人语义。尚未交付的真实字段映射或材料缺失必须具体硬拒，不猜schema或用returnOk占位；synthetic测试不记真实验收。

- [ ] 先可载入行为RED；纯assertRemoteReady复用同一集合/body检查，保core原80回归，不复制资产组装器/receipt私有检查/说明模板
- [ ] 本地九bytes/SUMS自身、冻结A/R/tree/readiness blob、固定外部acceptance/material/sourceRef与真实Task4/5 provenance，所有mutation前和临publish重核；路径/父目录symlink/外逃/重复角色/缺项/漂移拒绝，不读取原引用路径或执行材料命令
- [ ] 真实origin只读解析fetch/push唯一同repo，精确main/tag、无force/all/tags/隐式followTags；本地main不同R先拒绝并要求范围复核，不能推送未审工作
- [ ] gh固定argv与本机2.101.0实际schema，404只看anchored stdout HTTP状态，不按stderr文字；远端annotated tag同时核object A与peeled R。draft仅九项子集且无重名/额外，同名认证下载同bytes才复用，冲突不clobber/delete/bodyrewrite；分页完整获取资产，认证下载逐id/size/hash核
- [ ] S1全九认证下载后，S2临publish重读tag/body/set/id/size及本地材料/资产；变化拒绝或重新认证，public竞争不继续edit。S3与匿名下载后最终再读完整远端身份，错误不静默补公开混版
- [ ] 匿名curl以--disable为首参数，系统PATH/HOME/TMPDIR/locale白名单无auth/cookie/netrc/代理CA；起始URL精确同GitHub repo/tag/name，仅HTTPS重定向，不保存signed query或认证头。独立空目录九项size/hash含SUMS自身逐一等冻结，再shasum八行；失败如实保“已公开但回读未完成”
- [ ] 所有缺项/冲突/半上传/分页/非404/API错误/匿名失败及临publish漂移用fake runner行为验证，零真实账号/token/网络/push/tag/release。完整必要Node测试、精确提交冻结→SPEC→fresh quality→精确合main；实际发布仅Task6–8真验收后执行

官方资料与本机gh源依据在 `/private/tmp/towermd-unsigned-publish-task9-root-evidence/official-references.json`；stdout/stderr聚合日志不能重算逐命令输出摘要，target Gatekeeper只用实际manifest.targets[].gatekeeper，不伪造逐命令分片收据。真实工具版本/时间/命令结果与全部失败如实保留，不冒工具测试为App或十任务完成。

#### P8 Task9 Q1：按原记录作用域核sourceRef，不以材料ID替代内容身份

工具独立切片冻结 `2731bca2e4b28f9a818d99755ecd86a1061f8d75`（base83fa02c、精确三路径）作者248/248 Node通过，生产仍具体硬拒未接线P7原文身份映射、私有远端状态机只供fake测试。Root核138实际Git/工作树/归档源和126工件，manifest SHA256 `414ab43175c7c594569d47a2022f88dd92d68a4831ba0730e6773da341041d55`。独立SPEC FAIL/C0I1M0：sourceRef→material-id Map跨十卡共用，误拒各卡相对verification.log和同源同bytes不同材料ID副本。五probe三正/负控制通过、两行为失败，46选定回归通过；Root完整读报告并核18工件，报告SHA256 `dbc2feb117024a1bec3e9ee60cc24c1653a3a289926e88af6502967a10a254b9`，索引 `dd7270d4c09f2e3765d630d839a2a06360e1dd037b772940894e6583cbdd8b25`。未合入或启动quality，原证据不变。

- [x] 原writer仅改publish.mjs/publish.test.mjs，移植两原真实行为probe RED；补同卡同sourceRef同bytes材料别名正例及不同bytes负例，保每卡字面/角色/hash/list数量顺序约束
- [x] 矛盾判定限原记录作用域，同源引用按已核bytes/hash比较，不把裸相对文字升为十卡全局地址，不强制material-id canonicalization；不访问sourceRef原路径、不新增decoder/API或放行旗标
- [x] 原core80与必要完整publish测试同源最终绿、精确Q1冻结、全原工件保留，原SPEC整体delta→fresh quality；生产未接线具体硬拒不变，仍不记完整Task9/真人或实际发布通过

Task9工具切片完成回顾：Q1来源 `bd5bee695d9c9a5b129ba74d4e0f6d1ecc4f4a17` 最终254/254，138输入内容/执行位前后不变。原SPEC整体delta PASS/C0I0M0，原probe5、新delta3、选定回归16通过，报告SHA256 `afa5d2ac7a78a5a0b7b2a517ecb5057cfbcaf653ddf4340eed76468a7ff7f5af`，Root核165工件；fresh quality Ready Yes/C0I0M0，新增9风险probe与38选定回归通过，报告SHA256 `0bc15a1fc84e658b525042ac89502fabc58e1907b549dfcdf118dfa06013c923`，Root核223工件。主线Root并行todo变化导致首次辅助封存失败已原样保留，500固定输入未变；不冒全部501不变或产品行为红。四笔精确合main：`4dd29b2c230ee0a1394b79d6a8c53a5f4a7de932`, `1a99a2adfdf6229f90e671abfeb9aac32f5ad154`, `8406a18c0e6e1069117667d92cef10ec9ae7034f`, `7bf7126ffa5aed09eda841ae2c760f5e50ffb282`，三源码及整体binary patch逐bytes等冻结，未重复同字节Node全套。真实P7/人工原文字段映射未接，publish/CLI仍具体硬拒；私有remote仅fake可达，无真实网络/tag/push/release。完整Task9与发布仍未完成。Root证据 `/private/tmp/towermd-unsigned-publish-task9-root-evidence/main-integration.json`。

### 批4 独立 C/D1：共用验证运行器（已双审合入）

批4完整S1–S5/证据/草稿/daemon闭环仍待H及R10完成后接线；本切片只先落实已批准FR-107与批4阶段C、D1的无Catalog/Task运行器，不承诺CLI已有S5。实际前置83fa02cb已包含双审F/G与E1b，共享VerificationCommand/Report/CommandExecution/hash仍在recovery_contract唯一来源，现无verifier/artifact写入器；旧只读映射 `/private/tmp/towermd-verifier-batch4-preflight/notes.md` 仅作历史参考，writer须在新实际base重核而不能信旧“等待E1b”的状态。范围仅新core verifier.rs、tests/verifier.rs和lib.rs导出；不改project_session/daemon/main/snapshot/coldstart、恢复消费者或共享DTO，禁止与H同路径并写。Root独占文档，独立worktree/target，与H直接协调实际Cargo/短时限I/O资源。

- [x] 可编译stub的真实行为RED：指定完整旧提交而用户staged/unstaged/untracked均保留；未到实际执行阶段的编译/夹具失败不冒行为红
- [x] verify重导出唯一DTO/hash，严格canonical root/common-dir及完整commit/tree，独占common-dir/towermd/verify/<id> detached worktree，复核HEAD/tree；不变更分支/index，不prune，只回收本次精确worktree
- [x] argv直接执行、非空/NUL检查，规范相对cwd及符号链接不外逃；继承环境/网络，只去除Git重定向变量，不env_clear/替换HOME/改全局配置；环境marker和loopback用独立子测试进程传入，不改并行测试父进程环境
- [x] 双流并发写本机cache/verification/<id>新建日志，create_new/flush/fsync/封存SHA256，日志失败为基础设施错误；不在内存无界收集、不写用户docs，不假造Arca写入器或passed空日志
- [x] macOS真实父/子PGID超时/取消KILL及有界wait/reap，leader退出但后代持pipe仍有界返回且回收；先排空/确认回收再删worktree，cancel不把未执行项记passed，不杀无关进程；Unix cfg外如实平台未验
- [x] 真实非零/缺程序/超时为failed并有可区分诊断；日志/创建失败不造passed。核其他worktree保留、用户文件/index/status/ref前后字节；必要默认core verifier测试/fmt/alltargets Clippy，冻结精确三文件提交→独立SPEC→fresh quality

独立切片通过后只交付共用运行器；完整批4的模块归属/层次、Evidence耐久生产/恢复、派生草稿、90秒wait、GET与两真实仓库扫描继续未完成。此处不提前开放CLI或更改整批验收标准。

C/D1冻结候选 `f1653a700bba3d6924d7d54b858e6c1367ee6b84`（base ee9394b）精确三文件，默认验证器14项、fmt/core全targets Clippy同源通过。Root完整读生产源码及报告，核138个Git/工作树/归档输入与103工件。manifest SHA256 `4dad71862fa6a7005b67fdd6c7405833d0c459e4d03662a006c10c172cf4dbbc`、index `1ca2a6776115623e6ae45d395950d3ee4d4e5ab7e09924fa0b07aeb2ba140e6d`，证据 `/private/tmp/towermd-verifier-runner-evidence/`；已交新鲜独立SPEC，尚未通过双审或合main。01/02初stub红测缺当时自动源快照的限制如实保留，03编译失败及06/07 loopback EPERM不冒行为红；后续实际PID/环境/边界红绿证据完整，不覆盖旧失败。

C/D1完成回顾：独立SPEC PASS/C0I0M0（14项默认、环境EPERM原结果保留，精确升级绿），报告SHA256 `8b4d7d54efbf428468183d344fcae81ca20326c41d53052cf6aa64faa30f87b6`，Root核39工件；fresh quality Ready Yes/C0I0M0，两个新持续双流deadline/cancel真实probe通过，报告SHA256 `0b585cd39a31c3bad6a824a24278bcadb88b457cb60e988c62bf73567f8fff3d`，Root核305工件。已精确合main `9689983f9fd6e1cd9bcad13fec9dcb63b9493675`，完整binary patch及三实现源字节等冻结，其他受控输入只tasks/todo文档不同，因此复用同字节14+14+2与fmt/Clippy，不无谓重跑。未宣称workspace/H/完整批4/S5或App验收。Root证据 `/private/tmp/towermd-verifier-runner-root-evidence/main-integration.json`。

### 批4 独立 B：固定输入的扫描规划（已双审合入）

Root在独立worktree实施原批4 B1–B9，仅core coldstart.rs及tests/coldstart.rs，其他writer不改这两路径；不接daemon/验证执行/Evidence/草稿出版。沿现prepare_coldstart从权威固定revision读取，保S5 not_run与用户内容不变。先真实Git夹具行为RED：自身命令不加根fallback、待确认/unsupported也占自身发现、同cwd/argv去重、多源保留、可读稳定slug冲突、<3下钻/不可达诊断，以及10/25/81/82/>729层次。实现后统一在S4后重算成员/路径/归属与coverage，优先语义父目录，必要按已批准均衡3–9虚拟分组并诊断；分组不改变模块ID/coverage。执行集合仅包含已发现可直接执行的argv，不执行候选；无源码明确not_applicable。默认coldstart测试/fmt/必要core Clippy，精确两文件冻结→SPEC→fresh quality；不能据此勾完整批4或S5。

B冻结候选 `a6ae08be368337dcc5b2c98fe8c80b7547a90d8e`（base 0227bf7）仅两文件；最终同136静态输入的默认20核心+10真实CLI、fmt/core全targets Clippy通过，原时限不变。真实红包含归属/去重、多行脚本误执行、slug/层次/下钻与CI shell发现后错误fallback；05夹具数量预期失败完整保留，决定性执行边界与后续08单独真实归属红均在最终绿覆盖，未伪补早期全仓源快照。source.tar逐136 Git blob/工作树/执行位核等，manifest SHA256 `a5306fec2b12619e603435a2b573329d1a4bcfa5bfcbd17e97b0114b8d935bac`、索引 `70b12625686aa6971b6736de3dd0635efdcee076ebacf0a424ea642b6a74aa3b`，证据 `/private/tmp/towermd-coldstart-planning-evidence/`。尚未独立双审/合main。层次派生为hierarchy与候选画布共用树，verification_commands只为去重执行计划；算法标识directory-history-v2，S5仍not_run，真实draft引用文件/耐久Evidence仍待后续。

#### B Q1：模块目录归属与非ASCII可读slug

独立SPEC为FAIL/C0I1M1：模块engine/**内的engine/qa清单在该目录没有源码时被members过滤漏掉，错误用根cargo；中文目录字符全被丢弃为A-root数字。两项真实Git probe编译成功后2/2行为失败，无环境/夹具故障。Root完整读报告并核294工件/冻结源不变，报告SHA256 `49fb541a2b793a6f8a8224a3dafc54dca3382de082638ab677cc7c0367e96cef`、索引 `311eac8bb5e4c6f3244a3ac42cd79a74900c17b43a92bf56fb9d1fe87d6550a2`，证据 `/private/tmp/towermd-coldstart-planning-spec-evidence/`。候选不合入，不启动quality。

- [x] 仍只改原coldstart两文件，先移植原两个probe，并补非根父清单不得反向归给子Module的真实行为RED；保原失败/原34作者工件和295审查文件不变
- [x] 归属改为cwd等真实Module根，或由已生成root/**目录范围包含，按路径段匹配；不再用源码members是否在cwd下授归属，不让无源码清单消失或祖先清单越界；根fallback与待确认规则保持
- [x] slug保留Unicode alphanumeric及确定性小写，原完整目录排序/数字碰撞/reserved slug不变，不引依赖/转写服务；保根目录真实root与非根目录含义
- [x] 默认相关core与CLI、fmt/core全targets Clippy必要最终验证，精确Q1提交/全源归档/原证据不变，原SPEC整体delta通过后fresh quality；完整批4仍未完成

B Q1已冻结 `1be2f9d97fc3f1a735768990d005b799c67a5ea0`，三项真实行为红→默认23核心与10真实CLI、fmt/core全targets Clippy同136静态输入通过；原329工件不变。原SPEC整体delta PASS/C0I0M0，独立新归档两原probe与父子方向共3通过，报告SHA256 `4dfc642ba179341f63181e4899280d109d02ba44e36873a08ba90d40b3f09b71`，Root核155工件。fresh quality正在进行，尚未合main。S5仍not_run，不记完整批4。

B完成回顾：fresh quality Ready Yes/C0I0M0，独立冻结只读/快层布局1项与两个组合风险probe通过，原509工件不变；报告SHA256 `5952e8cd6caeebc21bf754c0531ec6b4ad61de6244b895bc80d0209012fedd9d`，Root核296工件。来源a6ae08be/1be2f9d精确合入main `5db3f87b82329523ffc1be1ceecfc26befa3680f`与 `fe80096425e9a7480a74b91bbee6336055a5aac1`；两实际源码及整体binary patch逐bytes等冻结，未无谓重复同字节33回归。生产仍是只读准备，未记S5或整批完成。Root证据 `/private/tmp/towermd-coldstart-planning-root-evidence/main-integration.json`。E唯一writer从fe800964隔离开工，target与H分离。

### 批4 独立 E 核心：固定输入与可重建草稿数据（正式实施计划）

本切片沿批4 E1–E4/E6/E7，接收双审实际合入的B与共用verifier，不抢写H/R10的daemon/session/recovery路径。Root唯一writer，在B双审精确合main后的实际SHA开独立worktree/target。范围仅coldstart.rs的唯一ScanInput/固定提交准备、新coldstart_draft.rs与tests/coldstart_draft.rs、lib.rs导出。E5原子目录出版/GET与F后台jobs仍留给同一daemon编排阶段；本切片没有生产写目录入口，不把测试将内存文件写入自有fixture冒称E5通过。Root已读取Note.md OKF v0.2全文与现有Document/types/CanvasDocument契约，不新增依赖。

- [x] 先定义可编译最小stub，真实Git夹具RED：canvas file必须指向本草稿真实arch文件，Module.id匹配，Unicode路径安全、同固定输入删除重建、main移动后仍读原commit，以及身份/算法/tree/options混用拒绝
- [x] 唯一ScanInput保存project_id、canonical root/common_dir、完整revision/tree、原ScanOptions与directory-history-v2；从已加载Catalog捕获，不跑历史/测试。按固定提交复用Catalog::at_revision和prepare_coldstart，核实际来源、project/tree/options/algorithm，不借当前main替换输入或另造第二扫描器
- [x] 固定输入补存捕获时的历史可用性：真实本地浅克隆→unshallow已复现同一revision/options下历史耦合导致模块由4变3的行为RED（03-shallow-context-red）。重建保持原不可用历史语义；原可用历史后来缺失则硬拒，不把后补历史静默当原扫描输入。仅增加快照输入上下文，不改变目录算法/新CLI开关或批准规则
- [x] 纯数据渲染返回固定相对路径→bytes：input.json/result.json、arch/<id>.md、tower.canvas、unassigned.json、candidate-tasks.json。Module通过Document::new注册类型，含name/paths/interfaces/verify、L1/draft/process:towermd和提交来源；不写人批准/基线/verified，不执行verify或候选任务
- [x] 保现有整数层次布局与group子节点，file路径统一arch/<id>.md/towermd.obj=id，严格CanvasDocument::from_value可读；Module/node一一对应，拒重复ID、路径穿越或错配，虚拟group不变成Module。完整原扫描结果保留，统一修改其candidate_path/candidate_canvas供消费者读取
- [x] 默认coldstart_draft与canvas/coldstart必要回归、fmt/core全targets Clippy，同受控源码前后hash/执行位，精确冻结→SPEC→fresh quality→合main并双语日志。仅交付派生数据内核，完整S5/Evidence/原子出版/HTTP/两真实仓库验收继续未完成

E数据内核已冻结 `c46b25cfa7a6cf169d2995b1ce62d486ab450bd3`（basefe800964、tree5705471f），精确四路径。初可编译stub五项真实行为RED、浅克隆补全历史造成模块4→3的独立行为RED均保全源tar；最终默认三目标50/50（canvas20/coldstart23/draft7）、fmt与core全targets Clippy同140源内容/执行位前后不变。作者manifest SHA256 `71fb88f107a1028ed4ae3c671a72fb54dfd9ec9ce91f2dedbcf9760e9c715e68`、report `1c65cf23f46292eb5c3908d8fec0395a9f0b87fd7dab81548c4dc37231ef425e`，索引34工件SHA256 `b448fbf5b9948a5ed06bd5675798ae6ce844aca75419eb3fb49b2a3632496f5a`，实际Git/blob/tar/工作树140源核等。02/04中间绿仅保存完整before/after hash及mode、没有独立全源tar，明确记录，不补造。SPEC独立审查进行中，未合main；E5原子出版、GET、S5、耐久Evidence和完整批4仍未完成。证据 `/private/tmp/towermd-coldstart-draft-evidence/`。

E V1独立SPEC PASS/C0I0M0，默认draft7及两个linked-worktree/旧项目输入/命令字面等独立probe通过；报告SHA256 `b338fc00f4459e9d304f6a92a4be37e9423552a672da694e5fa501d2c1df4a7b`，Root核313工件。追加独立观察确认candidate_canvas为scalar时mutable JSON Index会panic，正常prepare输出不会触发，不冒产品崩溃或SPEC缺失；V1及其314材料不改。观察报告SHA256 `e83af2ed01cda1a46c6963bcee6a6a51c6ab7bc4cedea31d704028898f40713a`。

- [x] E Q1公共Result API窄修：仅renderer和既有新测试文件，先scalar输入须Err而非panic的编译行为RED，再get_mut/as_array_mut；原有效输入/Module/Canvas数据不改，原工件全保，必要静态验证→原SPEC delta→fresh quality；已随E三提交精确合main

E Q1已冻结 `1daee3eeeaac241e91e20668b6b74c1cac8b68aa`，默认8draft/fmt/core全targets Clippy同140源通过；SPEC整体delta PASS/C0I0M0，独立1项覆盖七坏形状与有效数据保持，报告SHA256 `86ebbc63628d0d93a27f7a9dd8d3e5d95ef96350943d2239a37ed6d4bf2e2a2f`，Root核306工件。随后fresh quality NOT READY/C0I1M0：真实已提交 `İ/main.rs` 的lowercase生成 `A-i\u0307`，原scanner成功但renderer字母数字白名单拒绝。中文正控制通过、Unicode行为RED，报告SHA256 `0b2ffe77c3b75cf413b488880dd84a6ffdfdb99b55c159a083052aac8f6c39d8`，Root完整读并核298工件；不合main。

- [x] E Q2：仅原renderer/测试两文件，移植真实Unicode lowercase展开回归先RED，再将字母数字语义白名单换成安全单文件名组件检查；保A-前缀/非空/重复，拒分隔符、控制/空白、Windows保留特殊字符及尾点，允许正常组合字符。稳定ID算法/原模块语义不变，不新增Unicode库
- [x] E Q2：合法Unicode文件实际落盘/OKF引用、原穿越等负例、默认draft及fmt/core全targets Clippy同源静态通过，精确冻结/保原材料→原SPEC整体delta→原fresh quality delta；完整批4仍未完成

E内核完成回顾：Q2冻结 `c44c3c9a6334039b9f90dcb90846a88258b38d09`，真实Unicode测试先RED，再默认draft9/9、fmt与core全targets Clippy同140源前后不变。SPEC整体delta PASS/C0I0M0，独立一项验证真实Unicode/中文/ASCII落盘、OKF读取及15种完整同步身份的非法组件，报告SHA256 `9b7f6aef3c8527c83ccad58a873aee23b14bc5a7dc5b983de6ce71b91995b61f`，Root核307工件。原fresh quality delta Ready Yes/C0I0M0，原两项Unicode probe原字节重放及新增组件守卫一项通过，报告SHA256 `5619f7c8de6d0db1304a969c58962107fad620193e63e1777261acb2f5566b8a`，Root核298工件；原I1关闭，1132旧材料及Q2作者/SPEC工件不变，原NOT READY保留。三笔精确合main：`5e3cf9519554243bf0151824957f8fd8e945b411`、`a53ab383f4220d992c72ef65bf38745f6016e51c`、`356495ebfc5377f5474454eca8861866b3e34573`；四源码与整体binary patch逐bytes等冻结，未重复同字节Cargo，实际主线联合留待H合入后。Root证据 `/private/tmp/towermd-coldstart-draft-root-evidence/main-integration.json`。生产仍无E5目录出版/GET/jobs/wait/S5耐久Evidence，不记完整批4交付。

批4耐久证据接线只读预检已封存 `/private/tmp/towermd-coldstart-evidence-preflight/notes.md` SHA256 `2d5af7b65f59ec01aaff6c8a2e969bb2a85aa387ec222b19127ffddb26fe9abe`。Root读全文，核64实际Git blobs/66工件；98摘录按原行仅尾空行表示不同，原文blob字节完全一致，不冒精确逐字相等。发现直接evidence.recorded当前不投影、Gate通解码误拒coldstart、旧快照Gate兼容，以及唯一durable helper锁外I/O与compact后原回执协调缺口。仅设计未测试/实施；H/R10实际合入新SHA须重新核，不据此勾D/F/G。

共享报告耐久布局补充仅设计：Root全文读 `/private/tmp/towermd-coldstart-report-durability-design/{notes,supplement}.md`，核固定5ad1564的五源码与规格/原则两blob。原建议首Evidence兼载报告，但Root选择独立 `coldstart.reported`（候选B），统一零模块与有模块扫描，随后每模块一Evidence引用原eid/report_hash；报告/命令/哈希均复用原契约，无第二证明器或Snapshot字段。补充明确旧Git重扫是额外推导，不作为整个恢复前置；旧源不可用仅该初始Evidence追溯诊断，不放松finish/Gate。报告原回执经outbox-only恢复后能否在Git保留仍是必须真实验证的缺口，不以outer新seq替代。已同步批4 D6–D8计划，证据 `/private/tmp/towermd-coldstart-evidence-root-evidence/shared-report-{design,supplement}-verification.json`；未实施/未测试，待H/R10新SHA。

### 批4 独立 D4/D5：逐模块验证聚合与候选路径（正式实施计划）

前置为实际main `356495ebfc5377f5474454eca8861866b3e34573` 的双审B/C/E；从本计划提交后的main开独立worktree/target。新鲜唯一writer仅改coldstart.rs、新coldstart_verification.rs及其测试、lib.rs导出；不抢写H/R10的Session/daemon/recovery，不改共用verifier和持久契约。此切片交付供后续唯一daemon生产者复用的纯计算，不提供record/append/通用Value授权入口；D6–D8耐久Evidence、jobs和完整S5仍未实现。

- [x] 可编译行为RED：真实Git扫描的模块命令与报告按固定commit/tree绑定；非空且全执行成功才passed，任一失败为failed，无可执行命令为no_tests并保留原因。缺失/额外/重复/错序执行、报告身份漂移或矛盾通过字段必须拒绝，不能伪造no_tests或通过
- [x] 直接复用唯一VerificationCommand/Report/CommandExecution及commands_hash/report_hash；从原扫描结果取去重执行集合，核对逐模块计划与该集合一致。结果引用同一报告的execution索引和日志，不复制运行或建立第二验证器；共享根命令在多模块结果中引用同一execution，不改原归属规则
- [x] no_tests候选建议allowed_paths等于实际模块paths，保未发现/待确认/不支持的诚实原因；只产生草稿候选，不创建任务/Acceptance/批准事实。纯聚合不打开日志或声称字节已认证，后续producer仍须绑定真实verify返回和封存报告
- [x] 至少一次真实owned Git+共用verify+聚合组合证明共享命令只运行一次、多模块引用同一日志；用已有make测试入口及真实非空断言，不能在继承同target的cargo测试内嵌套cargo。无命令模块/待确认脚本未执行、用户docs/index/main/dirty内容不变
- [x] 默认新目标及既有coldstart/coldstart_draft必要回归、fmt/core全targets Clippy同源码前后hash/执行位；与H直接协调短时限资源，不改原线程/时限。精确冻结→SPEC→fresh quality→合main/双语日志；旧32项复用已核前后仅聚合源/测试变化的证据，不冒最终同65输入；实际main全量联合留待H合入，不冒完整批4或耐久Evidence完成

D4/D5初次冻结检查点（此时未合main）：`b37c62ab2209756908a60aff8bacf06ac24817b4`（baseab2480b、treebb0703c6），精确四路径。作者最终新11项/fmt/core全targets Clippy同65实际Cargo输入静态通过；旧scanner23/draft9在此前41项联合通过，最终仅新聚合源/测试变化，未冒旧32为最终65输入同源。作者report SHA256 `e7c16d4bc4cab691262bfd487c250c6a49376b77ef0075efeaf525eb8522a353`，Root全文读并核111工件、base142/head144实际Git与tar/wt字节和执行位、完整补丁；Root辅助清单结构KeyError单列保留，修核验器后通过，非产品红。独立SPEC PASS/C0I0M0，新鲜默认11项及两项真实Git组合probe通过：同argv四cwd、共变合并模块引用非连续[1,3]、独立失败隔离、root多源去重、pending不执行及计划重排映射。报告SHA256 `c7b5389ad0fc9259ea2d79b3bf31d86be58828d7538ae0d021fffefda43d8836`，Root完整读并核457工件；原作者112工件及144源未变。随后转fresh quality，未称完整S5/耐久Evidence或批4完成。证据 `/private/tmp/towermd-coldstart-aggregation-{evidence,spec-evidence,root-evidence}/`。

D4/D5完成回顾：fresh quality Ready Yes/C0I0M0，两项新真实组合probe通过，分别验证混合自身命令先失败后成功仍失败，以及实际执行前取消的报告只产生Failed/NoTests、零Passed；不推导job完成资格。报告SHA256 `fcf8025b9ed59fd95a434e1c4adf8922708d2b2a021dd7fad65aa616f16d0130`，Root全文读并核338工件；144冻结源、作者112与SPEC458工件前后不变，准备脚本缺base键错误独立保留。精确合main `c81278370990934c10e76ccd04c24dba70eb4a9b`，四源字节/执行位及整体binary patch等冻结；未重复同字节测试，Root核验 `/private/tmp/towermd-coldstart-aggregation-root-evidence/main-integration.json`。逐模块NoTests可与其他模块的非空共享全局报告并存；后续耐久producer复用此聚合与真实封存报告，不借finish的整任务NoTests校验替代模块绑定。D6–D8/jobs/完整S5仍未完成。

### P8 Task5 Q1：封堵独立规格审查的三个包校验缺口

Task5首轮候选 `56ba8bd4611d318f0f78aa1c6bc83c2e9f51dfa6` 当时已冻结但未合main。Root核136输入Git blob/工作树/源码归档、77工件、24命令原始日志与完整补丁；作者指定assets92+core80共172项通过。独立SPEC为FAIL、C0/I3/M0：两条系统库绝对路径可经`..`逃逸，metadata移到Tauri之后仍通过，已生成ZIP在后续技能gzip时变化仍返回成功而manifest保留旧size/hash。独立原测试172通过，六项probe中两正控制通过、四行为断言失败；报告 `/private/tmp/towermd-unsigned-assets-task5-spec-evidence/report.md` SHA256 `61fa0ac161d42d409c30dc60289eced3f5e11c59febf877fa4b66af5121fc027`，索引SHA256 `3582f0f516161eaa14afbcf25aedfdc5035cfe996d70dbcbbc442a55dcbc246e`。当时先停止质量审查与合入，原全部失败与准备故障保留；后续完成见下方回顾。

- [x] 原writer仅改既有assets工具/测试两路径；按规范化后的系统绝对路径归属验证或明确拒非规范路径，保正常系统库和真实包内依赖，不读开发机依赖或加绕行开关
- [x] 将metadata纳入真实cargo build < cargo metadata < staged CLI lipo < frontend < Tauri严格命令序；消费既有Task4 receipt，不造字段/新信任协议
- [x] 在最终成功manifest/SUMS前，将五个已核二进制记录与实际size/hash对账；组装期间资产漂移必须拒绝，不能仅重算checksum或重写旧记录掩盖；失败无成功manifest/SUMS
- [x] 三缺口先行为RED→GREEN，含两条系统路径、metadata失序及后期ZIP变化和正控制；必要Node assets+core回归，原172与六probe证据保留，不跑Rust或真实App/挂载/发布
- [x] 精确两路径提交冻结，整体必要SPEC复审PASS→fresh quality→精确合main与双语日志；P6 production/P7加载/真人/Intel实机仍须后续真实证据

Task5 Q1完成回顾：实际main提交 `665d1fb10bc2e4af1bb46c26d99c205145522642`、`c20f721ac7b4393787521c52f67ab9d2968afd27`，对应来源 `56ba8bd`、`1225ce9700f56e8fcb21f4307b53eb467c52e541`；Root核两完整patch/两源码全等，除Root任务文档外全部136受控输入一致，未额外重复同字节测试。作者新增13项RED中两正控制通过/11行为失败→assets103+core80共183通过；manifest SHA256 `b6bd39d10bcb34d4cca3ea72b21daca72b8d09798c0d37ba224544e2dfa5c89e`，Root核新36与原112工件不变。独立SPEC PASS/C0I0M0、183通过，报告SHA256 `28fd2ab4ba7645ce4da44443240bfc1a59f270b3374097c89d0a5ce48c9de886`；旧六probe重放为五通过/一历史成功前提不适用（正确提前硬拒），新增独立断言确认漂移拒绝且无manifest/SUMS，不能冒称原六全绿。fresh quality Ready Yes/C0I0M0，103项与独立8探针各一次通过，报告SHA256 `e671e4dd46d2560e9b0b9f52da5563805d4aa9f54fb42bb33b5bbdc5d68a2d96`；Root核SPEC41/quality86工件、136实际输入与192旧证据。辅助准备/收尾故障均保留，不计产品红绿。README/双语未发布日志同步；全部macOS工具fake，无真实包构建/安装/真人/Intel实机/升版/tag/发布。

### F/G Q3：确认错误类型贯穿唯一 writer 到 HTTP

Q2冻结 `b1918ff4fcde59ce70f60efd03e3e4de350933df` 尚未合main。Root已核129 source blobs/工作树、92新工件、130旧工件及独立诊断4工件；core29/App4输入与最终CLI46仅独立CLI helper文件之差已逐128文件核实，不冒称同129 hash。整体SPEC再次FAIL/C0I1M0：I1保持关闭、I2两删除独立绿，新I3在daemon核验后发生真实Git blob读取失败，合法路径 `docs/tower/arch/changed-A.md` 的文字被当成来源变化，实际409应400；refs/来源身份/saved均不变，零target创建/Project发布。报告 `/private/tmp/towermd-fg-q2-spec-evidence/report.md` SHA256 `3d57cc18eaf01b3a0930a2c96117a5302b8c5f9f64ada56c52b856e34fcff5b6`，索引SHA256 `912d4ecd843062ba7484b4d11502d52f18362774e252a86b500fddd823df8f9b`，Root核16工件；独立1通过/1真实行为失败，无编译/环境/夹具失败，原证据保全。

已内部确认最小方案：Q3仅改core recovery.rs、snapshot_service.rs和daemon.rs，整体union由六增至七路径。保留原 `confirm_rebuild -> String` 薄兼容适配，转调唯一持锁的typed实现，不触碰已合R11测试、不复制业务或增加第二writer；新分类只用于错误传递，无授权/跳闸旗标或HTTP协议变化。

- [x] 新ConfirmError明确Conflict、ReadOrRefused、Persisted{actual commit,error}；任意String默认400，真实source/digest改变、prepare不匹配、确认过期明确409，非法actor原前置校验保持
- [x] core source核验/失败重探保类型；Git callback只用局部Option传回实际已发生的typed错误，成功清空，不按Display猜类型；真实读失败400，不把重新读取失败当变化，不增加reader
- [x] Written后安装失败只构造Persisted400及实际commit/新请求诊断；daemon按variant设置recovery_error与HTTP状态，删除关键词分类，不因自身Git前进事后改409；原String适配只投影Display
- [x] 原I3真实Git/TCP probe先RED→GREEN；含changed/已变化/未过期路径的普通IO400、原双ref删除/有效前进/late删除409、clock推进过期409、安装400+commit、I1/owner/零续租回归保；不加生产故障环境、时限或复杂测试框架
- [x] 必要相关默认线程测试、fmt/alltargets Clippy，精确三文件提交冻结；完整源码/原失败/旧工件不变证据齐备后整体SPEC→fresh quality；最终合main含R11后一次完整默认workspace，未完成前不勾批3或App交付

F/G完成回顾：来源7fbc7d3整体七路径已精确合main，四提交为 `3db1c157d78092892796bb188b33aa67950ac8c2`、`72fa9332d05115fef1adb2eb760991417b7f5a3b`、`1ab2c8a09930e2b08d3b677395299126af93bb1a`、`c30bf9052b91cc5bf6f64f8f67ff520e56ed3fa8`。六文件逐字节等冻结，catalog保R11完整补丁等价，其他R11源未变；没有冲突或覆盖R11。Q3作者同129源81项及fmt/Clippy通过，Root核67新/244旧工件及归档。独立整体SPEC PASS/C0I0M0，11命令19执行/18唯一测试通过，报告SHA256 `841078c41d3ec09a2767dc7a66bda64ca1cb9040a08410a6eccf4293aad7583c`（48工件）；fresh quality Ready Yes/C0I0M0、7唯一测试通过，含新真实TCP None/Some双确认探针，报告SHA256 `34d2a93543354df7493f90a5ac7fc52709250c4af2717b5d798c4d705b331e20`（84工件）。原I1/I2/I3关闭；环境失败、夹具失败及旧错误结果全部保留，quality探针/冻结target二进制身份歧义已独立冻结重建关闭。实际main默认全量26组630通过/0失败/忽略/过滤，fmt和workspace全targets Clippy同136输入SHA256 `811b9b75e5aa0aa736e47b8ddec89d2ab0652d6d95062969340da5894f91c626` 自然exit0；全量日志SHA256 `ceef6e50b0e9a941feaffd90b9deef22e3f2592ad406052906487f6d5ae6a6bf`，证据 `/private/tmp/towermd-fg-main-integration-evidence/final-verification.json`。Root两次核验/启动准备错误先于Cargo且未改变源码，另存preparation-notes；不能补造先红或覆盖旧失败。本次联合作为R11/R13最后验收，仍不代表H/V、M0全完或真人App。

### F/G Q2：保存来源消失的冲突分类

Q1来源 `6ec8d24adf077992f25757ed8541448ae34eb2b4` 的整体SPEC为FAIL、C0/I1/M0，未合main。I1控制字符前置拒绝独立HTTP/core均通过；新I2在保存请求后实际删除唯一私有快照ref或权威main，两条真实TCP均400，应409，且目标未创建、原请求未改。Root已核报告及27工件；报告 `/private/tmp/towermd-fg-q1-spec-evidence/report.md` SHA256 `2cbfca2d1d9de9349faa2ed97435f5bcacc8b8d476202099a1b595d57f13349f`，索引SHA256 `b31a8bb46d4f7a0763fecfe7e370776732cee4c97de68bfcf82dcdb342c71b0e`，行为2通过/1失败，首次probe移植括号编译失败单列保留。

- [x] 原writer基于Q1实施已确认的窄来源核验与Changed/Read分类；复用既有authority/private reader，catalog.rs仅增加既有reader的crate内可见性（联合范围由五路径增至六路径）；加载/证明早退前后核同一保存来源，写入/安装阶段不事后重判，不将全部I/O泛化为409，不新增协议、依赖、trusted开关或第二reader
- [x] 真实Git/TCP删除两种ref行为红→绿；保来源合法前进409、普通目标损坏/非空400、Git已持久后安装失败400及实际commit诊断、旧I1零副作用
- [x] 必要相关默认线程测试、fmt和全targets Clippy；保存原失败与本轮前后输入；精确提交冻结，原五路径以外的必要变动先说明
- [x] 冻结整体SPEC复审PASS→fresh质量审查→精确合main；再对含R11/R13的实际主线完整默认workspace联合一次，未完成前不宣称F/G或批3交付

Q2相关回归首轮为core29/App4通过、完整CLI45通过/1失败，原source `e3c0ce940a70b8096621bdb06fa3c1231b03a45910e325c23b2570788ef0aed8` 静态，失败日志 `/private/tmp/towermd-fg-q2-evidence/final-daemon-cli.log` SHA256 `61142c16ab1e3d2829561a06f243b5d0e458af7752904d65acbf4b1ef9cd3c55`。未以重跑覆盖：失败是切离main后的私有→main搬送阶段，测试helper遗漏第三种精确的前置SourceChanged文本；原日志未捕获具体CAS/OID，不能补造。独立只读诊断 `/private/tmp/towermd-fg-q2-cache-failure-diagnosis/notes.md` SHA256 `2ca29fcfb40acfc5bc02550fea65289d9058ae62109a7996385ca621c4e7cb0f`，Root核三工件和六个未变冻结路径，确认该裸错误不表示本请求已确定出版Git快照，持久后错误有commit包装；capture/续租仍可能产生系统事实，不声称整个请求零副作用。

- [x] 最小修正仅现存CLI测试helper接受exit1及完整 `towermd: authority changed during cache probe\n`，现存单测先补正例RED，再精确白名单GREEN；已持久/uncertain/accepted包装、额外行及错误码必须全部拒绝，不改生产重试、线程、10秒deadline或50ms等待
- [x] 以新日志重跑原完整CLI46、fmt/alltargets Clippy并冻结；此前core29/App4如复用则明确旧输入与后续仅CLI测试文件之差，不能冒称全129输入同hash；最终实际main完整workspace仍待双审合入后一次验证

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
- [x] P6 workflow Task2：独立双审、精确合入及与实际E1b联合验证

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
- [x] 3.3 E1b：业务投影、完整会话/daemon接线与 staging/空目标恢复
- [x] 3.4 F/G：真实 App 恢复与 Git 先于安装
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
- [x] E1b Q2：静态完整验证、保全旧证据、冻结并独立SPEC→fresh quality；质量发现的安装问题由Q3闭合，原NOT READY证据保留

Q2已冻结 `aafa370c8497392c71489bfc262e79d3c03f89df`（直接parent1b05337，tree85332cea）：3delta/25union/58输入，09最终默认完整workspace25组588/0/0/0自然exit0、同源fmt/Clippy通过；source `d62222ea8e07bb5e5c1af9135ead62d9de7754b48b410c49a9476f8baebb71f4`。manifest `/private/tmp/towermd-e1b-q2-evidence/manifest.json` SHA `7452f8637011861214c2b5f8656256d0f44d8d78db2e079756bada0b0be9f1e5`，Root独核54新artifact、原124+Q1354及Q1原58Git输入不变。独立SPECdelta整体PASS，47/47（含七变体的一项M1及实际budget外部探针），报告 `/private/tmp/towermd-e1b-q2-spec-evidence/final-spec-report.md` SHA `8ceb01235ee08cc23e78530399c2d9ef028cff16cc0432c5bc676f352a58ae99`。writer/SPEC STOP，fresh quality已START；未合入，不以588冒主目录/真实App验收。

Q2 fresh quality实际复现新的安装窗口：`recovery.rs`两条目标路径先 `rebuild` 持久非空Hub，再独立写exact ack；第二次journal replacement遇EISDIR或两写之间崩溃后，实际drop/reopen见非空floor而ack=None，Session拒绝exact ack，更高新请求也因非空目标拒绝。独立诊断探针1绿仅断言坏行为；另有真正产品契约红 `10-install-contract-red.log`（可执行测试1 failed、自然exit101），要求新授权更高请求可完成，实际SnapshotRequired。证据 `/private/tmp/towermd-e1b-q2-quality-evidence`；不把夹具恢复第一步journal字节当产品补救。当前不合入，等质量有限回归/正式报告STOP后交原唯一writer。

Q3最小整改计划：使最终目标checkpoint、exact ack与tail的安装具备一次持久边界，或提供只针对已核部分安装的严格续接；先核实际Hub/七操作协议，再选择最小实现。普通非空Hub仍拒覆盖，不能新增公开trusted开关或跳门禁；Git确定持久、staging证明、目标安装、有效view的顺序保持。原Q2及所有旧证据保留，新证据单独 `/private/tmp/towermd-e1b-q3-evidence`。

质量正式STOP报告为 `/private/tmp/towermd-e1b-q2-quality-evidence/final-quality-report.md` SHA `2fc561eaa02e22cc68438a11f70cb054068fd3c6ae7e83d6b82dc518480bdc6c`，C0I1M0/ReadyNo；独立127仓库回归绿及1契约真红分别保留，Root核35新证据hash。原writer已接Q3唯一写入：扩展既有RebuildSeed可省略的最终确认，Hub在内存校验/压缩后一次journal replace安装，不加第8操作、不解业务。源码/旧证据保持Q2冻结直至正式START；后续无论写失败或重开均须证明没有无exact ack的不可恢复中间态。

- [x] E1b Q3：主/私有快照与实际drop/reopen安装失败的契约行为红→绿，保普通非空目标拒绝及原owner
- [x] E1b Q3：必要定向及静态默认完整workspace/fmt/Clippy，精确冻结，再SPECdelta→qualitydelta
- [x] E1b最终双审后精确合入，并与main已有Canvas/接口/Ensure真实联合验证；不提前标完成或发布

Q3冻结 `026b354c99883fb48ac132fa59792153c23853c4`（直接parent aafa370，tree6f51fb17），精确6delta/整体28scope，生产仅Hub协议/Hub/recovery，其他为seed字面量与测试。10默认完整workspace自然exit0：25组594通过/0失败/忽略/过滤，同源08全部targets Clippy `-D warnings`与09fmt通过；source `8b62b11acf9f466151eb79cf825810064a0cc06120f9fddf558af75dfbb06c6d`。02真实第二写EISDIR行为红→04主/私有绿；05协议24、06真实rename后dirsync不确定Hub7、07恢复25绿。01夹具/03编译失败单列，不当红测；04末尾owner仍live的拒绝仅证明所有权，普通非空拒绝由07既有测试证明。manifest `/private/tmp/towermd-e1b-q3-evidence/manifest.json` SHA `acfc8b7e684081c1a82b707d1df5f188cb2618d81c570fdd8a03e59431ff0d2a`；Root独核28blob/58输入/59新artifact/两patch、534旧作者证据+36旧quality及140旧Git输入全等，receipt `/private/tmp/towermd-e1b-q3-root-evidence/freeze-verification.json`。writer STOP，独立SPECdelta已START；未合入，不称main联合或App验收。

E1b完成回顾：实际合入 `d172df8`、`eca14cd`、`7390269`、`22bf1d7`，分别对应原实现与Q1–Q3，最终来源026b354。Q3独立整体SPEC PASS/97项绿（报告SHA `8a086695be367921fb4d36291403af66a9a0e07f645ac8b9872ff481303452a1`），其后原qualitydelta Ready Yes/C0I0M0、36项绿（报告SHA `f6e3e5415cf2e0b29623e37db8b10a4cbe1bee8fd08fa007e204e60aa6fdcbe6`），旧I1–I5/M1及最后安装窗口均关闭，原失败证据不改标。Root核18份SPEC、19份quality新证据hash；28源码中27逐字节等同冻结，daemon仅额外保留已验证Ensure包装；Canvas/接口/Ensure其余6文件等同各自冻结。实际main `22bf1d7e63e471f2fc34f867bed3a786f06e300f` 完整默认并发26组601通过/0失败/忽略/过滤，fmt与全targets Clippy同静态source `5fadddbbc91fae9788158db189621595b75945112bb81f0d73cbe357cbcad77b` 自然exit0。证据 `/private/tmp/towermd-e1b-main-integration-evidence/final-verification.json`，全量日志SHA `02c0a2875237799bf404fcccf3b0c730caee3db2d01f3601a62edcbb14b1bb90`；准备脚本语法/错误预期SHA拦截单列且没有启动Cargo，不计行为红。Ensure第二条实际联合现已完成，F/G、M0整体、App真人和发布仍未完成。

批4只读映射已STOP：`/private/tmp/towermd-verifier-batch4-preflight/notes.md` SHA `9c01f893566980dc5401754312817b6be37124d6c77ee945e94d2657050ba355`，冻结main2ff022e、无Cargo/实现。已定位共享verification DTO、durable私有边界、Evidence投影与artifact封存、异步scan/wait/草稿重建的连接点；所有接口须在E1b及批3实际合入后重核，映射不是SPEC PASS或产品验证。

#### 批3 F/G：真实恢复端点（已双审合入）

沿批3已批准F1–F6/G1–G4，在E1b实际合入并完成main联合后，以真实SHA开新worktree。唯一writer接daemon/main、core recovery/snapshot_service/project_session及对应测试；Root继续独占文档。复用core实际confirm API和原注册/clone/owner守卫，不新建第二Hub或空业务Session。先真实CLI/TCP行为红，后实现；默认并发与原时限不变，冻结后SPEC→fresh quality。

- [x] 重核实际合入的E1b API及main联合证据，登记F/G开工base
- [x] CLI rebuild带/不带run均零续租；原子保存五分钟原请求，GET只读且重复不延长
- [x] 真实HTTP按instance→App client→严格body检查；确认仅用保存的路径/来源/expiry，接唯一owner并仅成功后开放服务
- [x] missing/empty合法快照返回需恢复与来源摘要，纯读不创建/expire/renew；authority_unavailable和corrupt原诊断保留
- [x] 主/私有真实TCP授权、失效来源、旧epoch及Git先持久/安装失败新边界矩阵，保用户文件/ref/index
- [x] 必要完整验证与独立双审、精确合入；App真人恢复点击仍留后续实际App验收

F/G实际开工base为已通过601项联合的22bf1d7，新worktree `/private/tmp/towermd-m0-remediation-3-4-fg`、branch `task/m0-remediation-3-4-fg`。actual只读映射 `/private/tmp/towermd-recovery-fg-actual-preflight/notes.md` SHA `b453161b8e3540f8cf5ac4f032619e818bba868f51a37bfe07e0922f4de1e8e6` 已STOP：main原已无Rebuild预续租，仅删除daemon续租closure；Q3原子安装复用；RecoveryRequest须窄Deserialize，missing/empty辨识须复用core真实解析器作无写探测。唯一writer允许为此最小扩展Hub/journal/legacy读解析及对应测试，不改七操作/门禁。读写鉴权/owner/源摘要/真实TCP故障矩阵沿F/G原计划，不重复旧全量基线；实现后再最终必要验证与双审。

F/G进行中检查点（源码未最终提交/冻结/审查）：真实CLI零续租、保存请求TTL及严格HTTP检查矩阵已在默认daemon_cli完整45项通过；真实TCP同原Arc恢复、旧guards拒绝、新epoch领取及主/私有Git先持久后安装失败路径已绿。自审另复现不完整journal尾行在冷confirm时被Hub::open修尾：HTTP路径先以无写probe拒绝并保原字节，core None入口同一保护正在补真实红绿，尚不能以较早core27项或HTTP绿称最终候选完成。环境EPERM、编译/夹具失败和真实行为红分别保留，最后静态完整workspace/fmt/Clippy及双审仍待。

F/G首轮冻结 `18bd413749cb09c31d4dde01fa2db7f6a1512c72`（base22bf1d7，精确五路径）已完成默认26组611通过/0失败/忽略/过滤，以及同源码fmt/全targets Clippy。manifest `/private/tmp/towermd-fg-evidence/manifest.json` SHA `6fe1042a36767262a5e97ab3994a737038440bb081f4a0fd73db919dd5b59d65`，Root核129输入、42artifacts、6外部和实际CLI binary；初期F1/F2没有当时逐次hash的限制如实保留。独立SPEC **FAIL/C0I1M0**：合法JSON中的控制字符actor通过前置校验，真实TCP两分支均返回400而非422；None新建hub、Some原Arc的Session退役。原refs/request未变，编译成功1项行为失败，日志SHA `5083dc1ac579ee7520b6d0dfcad9f8b7d8326d30c62fd28f000ea0768b1654eb`，报告 `/private/tmp/towermd-fg-spec-evidence/report.md` SHA `acda2dd7f37c5d3adf8b3ab4c28dfefebe2f2dcb747ededdd38ab74e5cb746c0`；Root核索引9项原始证据，不把611绿等同通过。当前不合入或启动quality。

- [x] F/G Q1：仅daemon/core recovery两路径统一拒绝控制字符，真实HTTP与direct core的None不创建、Some不退役/原Arc不变；先保真实红再绿，无新协议/依赖/信任旁路
- [x] F/G Q1：保首轮冻结/全部旧证据，定向及必要最终静态默认验证/fmt/Clippy，精确Q1提交后整体SPEC复审→fresh quality

H/V与R10只读预备均已STOP，尚未实施/测试，不是SPEC PASS：`/private/tmp/towermd-snapshot-h-actual-preflight/notes.md` SHA `096dfd5fbc5c74749bbd312acaf8a0f3ef4f6e8c730192e76ec5901e467f12ad`，`/private/tmp/towermd-clone-r10-actual-preflight/notes.md` SHA `768b82bd57ec1c5fd18dce9cf35077fe24d80fd737f79a88a8979ac82615075e`。前者定位due前重I/O、writer内部周期门、心跳tail、单调deadline及真实written时间重建；后者核对cached authority、耐久审计与File/请求/WS/worker排空，不能以移map、裸Arc计数或abort视为释放owner。两者都须待F/G实际双审合入新SHA重核；同一daemon路径不并行writer。

#### 批5 R11 / R13 剩余切片（实际E1b后开工）

沿批5Tasks2/4/5/6，用已联合通过的22bf1d7开独立工作树，与F/G文件分离。实际只读预备为 `/private/tmp/towermd-layer-r11-post-e1b-preflight/notes.md` SHA `f273c6aaf0f1dd34c595cf42690de5e650ee8624949fcbc4393cb8380516c216` 和 `/private/tmp/towermd-task-r13-actual-preflight/notes.md` SHA `4df7059b5be8abaaaefd0b364ea20438d02371c7438cd839cf096656a42bf181`；仅预备，不当SPEC PASS。唯一writer顺序四个精确提交后整体双审，最终静态完整验证一次，不并行写共享tasks路径。

- [x] R11三态layer：非法可读、同源诊断与慢层保护；发布/删除仍仅合法慢层，真实改前后守卫与宽容往返
- [x] R13.1仅修local_runs活动提示：真实Release/到期/其他host后仅列running，不过滤历史Status.runs
- [x] R13.2保既有生产转换，补直接G0失败零ready/claimed和ready不重复的契约回归；不伪造merged或重写生产同步
- [x] R13诊断合流：正常/missing/unsafe/invalid四路保Catalog与Canvas诊断，并保既有interface_unresolved
- [x] 最终静态必要完整验证、独立SPEC→fresh quality、精确合入与真实联合记录

R11最小7路径为 core/src/{types,catalog,tasks,recovery_git,recovery_git_tests}.rs、tests/{catalog,tasks}.rs；tasks生产仅单测字面量字段迁移，合法慢层资格两处不可用is_protected替代。R13.1复用tasks.rs/tests/tasks.rs；R13.2仅测试，既有生产来自d172df8、真实恢复ready<claimed/G0拒绝来自eca14cd并已在本轮main601覆盖，不为补契约人为倒退代码。最后仅加src/canvas_view.rs与tests/canvas_view.rs两路径合流，总scope9文件。不改R12、接口加载器、daemon或恢复协议；Root独占文档。各段新行为先可编译行为红→绿，已有正确行为的补充回归如实标绿，不冒先红；编译/夹具失败单列。Rust资源与F/G writer直接协调，保默认线程和原时限。

分段已验证提交（仅隔离树，未合入main、未整体双审）：R11 `e210d541d5e446cfdd3b3ab29f59fff756d951c9`，真实非法层级RED后Catalog31/tasks29/claim_safety3及fmt通过，合法出版/删除真实Git矩阵7项通过；矩阵首次排序夹具失败单列。R13.1 `cf19e5e3e9e6900519ab6990e528f560e89b1569`，真实Release/到期后列表3项对预期1项的行为RED，修单谓词后完整tasks30绿且Status保4条历史Run。R13.2 `67f3b14aff867c15e7659aa604c4783463a3d114` 仅测试，完整tasks31绿，不称新生产TDD。Canvas五路诊断丢失已真实行为RED，正在最小合流；整体最终静态验证及独立SPEC→fresh quality未完成，不勾选上述验收项。

最终四段冻结head `92967124030675b628ed8d682b101807f77e9773`，默认26组614通过/0失败/忽略/过滤、fmt/全targets Clippy/diffcheck均自然exit0，全部129输入静态。manifest `/private/tmp/towermd-layer-r11-r13-evidence/manifest.json` SHA `064d5efda5b870d5dd3e7a8c3f8fd56b30806dac5ba59118e51fb35eb191b6e5`；Root核129输入/68Rust源码输入摘要/48artifacts/16命令与全部原失败分类。静态SPEC未发现C/I/M，现接最终证据并做独立定向验证，整体PASS及fresh quality仍待。Root首份核验脚本误把带4个注释键的manifest记录与raw receipt直接判等，已改为核raw hash/全部原字段及精确注释键；这仅核验工具准备失败，没有启动Cargo或改变作者证据。

R11/R13完成回顾（已合入、未发布）：main四提交 `417fe8001c9759e07d6806238e4a87a227309688`、`0658ddba85830cd5c5c811333f8d8f349e9c4363`、`e50342622e70828c554ddaceefec87df99a6b282`、`1cd0274bab1a2c3fcbbf156f95d268cbf1aa63c4` 分别对应原四段，精确patch及九文件字节全等冻结，署名均Agent，原三未跟踪文件保留。独立整体SPEC PASS/C0I0M0、83定向通过，报告SHA `f3bf5bacb4da7b75bddf5f75f30ceef6df6aafd8454f312d9fbfa7dbe9a729eb`；fresh quality Ready Yes/C0I0M0、11通过，实际新增恢复矩阵为3测试18场景，报告SHA `20ed95ec8cbefab999b81e8bb6b717fd96e71df0185aceb634d6c4e0c449881d`。Root核SPEC16与quality18项证据和129输入前后，原失败保留；最后联合已在实际F/G主线c30bf905完成默认630项及fmt/Clippy，详见上方F/G完成回顾，不把原本树614冒作联合结果。README/双语未发布日志同步。

#### P6 Task10：独立构建工具切片（已合入；完整App验收待后续）

P6基础Task10的构建编排只依赖已交付stage CLI/项目pnpm和现有Tauri配置，可独立提前验证；P8 Task6明确允许在真实验收未齐时继续不依赖它的构建工具。本切片只交可实际执行的构建入口，不勾选Task10/Foundation，不运行或冻结首版，不把当前壳称完整App。只读预备 `/private/tmp/towermd-app-build-tool-preflight/notes.md` 将登记冻结22bf1d7的真实接口；源码开工以本计划提交后的main实际SHA为准。

- [x] 独立worktree，限定新增scripts/build-app.mjs及其.test.ts、修改vitest.config.ts显式收集该测试和src-tauri/tauri.conf.json显式未签名配置；Root仍独占文档
- [x] 先运行能收集且执行的行为RED，再实现显式受支持target、同target stage→项目前端build→真实CLI/dist校验→固定Tauri build argv；无shell拼接、无工具链全局配置、无源码输出路径猜测
- [x] 复用已合入release/core.mjs的unsignedEnv/assertUnsigned，唯一策略阻止发行者签名/公证/updater；同一环境传各子进程，只显式保留规范化CARGO_TARGET_DIR，不读取真实签名值；基配置为createUpdaterArtifacts:false、signingIdentity:'-'，ad-hoc如实区分
- [x] 覆盖两个target、原CLI或前端失败不使用旧产物、缺CLI/dist与非法配置拒绝、argv/环境/目录和错误退出；保Tauri现有beforeBuildCommand再次前端build的真实行为，不宣称已消除重复
- [x] 运行新工具与stage既有定向测试、前端check及diff；资源与Rust实现者直接协调，冻结源码/证据后独立SPEC→fresh quality，再精确合入/写双语日志

不提前实现P8专用unsigned覆盖配置、DMG、locked冻结构建编排、产物审计/发布；这些继续归P8 Tasks4–5。真正host App及源码外Finder/最小PATH/退出后daemon存活仍等P6实际客户端和完整Task10验收。

构建工具回顾：来源 `174f86b8d8768cc5a0446673f27d5a5c85f78bdb`，main精确合入 `5eff6a304d91714bba60d9cb65c0bb642a4340cf`；四文件与patch全等，manifest SHA `365acd61c4f95daeb1ab8f83b0c1a2a3e8555442b314e195dfd45eff435fddfa`。独立SPEC PASS/C0I0M0、34项绿，报告SHA `de4c88a0215968d10d97a80a3401127e24e4a3a56434f609a4a56c9189000618`；fresh quality Ready Yes/C0I0M0、31项绿，报告SHA `17f66063da5bf6c05e817c903af5540cb4596a4b496e03a4f428bca09a4629c7`。Root核131源码及source.tar、作者55/SPEC12/quality15工件；实际main37定向+pnpm3+check零错零警，131输入前后不变，日志 `/private/tmp/towermd-app-build-tool-main-integration-evidence/`。主目录最初未安装项目pnpm，npm启动ENOTFOUND且零测试，保原日志；仅复制已验证11.7.0缓存并离线frozen安装，99包复用/0下载，无锁或版本变化，再以新日志记录实际通过。初次cherry-pick配置参数位置错误在解析阶段退出，索引/源码未变，更正后精确成功。未构建真实App或发布，未勾选完整Task10/Foundation。

#### P8 Task4：独立冻结构建工具（已双审合入）

真实P6构建入口已双审合入，可沿P8原Task4实施独立编排工具；P6完整客户端、P1–P7/真人/十任务验收仍是实际版本冻结与打包的前置，不据本工具创建发布tag或首版产物。只读预备 `/private/tmp/towermd-unsigned-build-task4-preflight/notes.md` SHA `2c166345e312180e62ad1822e5b02df8a9cdc48ff13033dbe1f58e6cb0a01cb2` 已STOP，必须在本计划提交后的实际main重核接口。

- [x] 新隔离worktree/target，仅新增src-tauri/tauri.unsigned.conf.json、scripts/release/build.mjs及其.test.mjs，精准修改stage-cli/build-app及其原测试；Root独占文档，不改依赖/锁/native/HTTP
- [x] 按原Task4建立能执行的行为RED；冻结tag/HEAD/dirty、真实prepare frozen/双锁、两target CLI来源与Tauri前架构校验、有效合并配置/净化环境各有到达目标的正反控制
- [x] 保P6默认调用，增加有限locked/config/bundles参数，单次stage，不新增skipStage；复用unsignedEnv/assertUnsigned/checkVersion，同一净化runner覆盖prepare metadata与嵌套子进程，显式隔离CARGO_TARGET_DIR
- [x] 保存本机锁定Tauri CLI build --help，核实际config/bundles及cargo透传；新增配置按官方合并语义及已锁定实现核对，不凭overlay有效就声称最终配置安全
- [x] 核构建前后实际tag/HEAD/tree/全部受控源码与锁字节，唯一新App/DMG选择及输出/日志隔离；测试用合成产物，保Task5真实包检查的边界
- [x] 必要Node/Vitest/check回归、冻结与独立SPEC→fresh quality，精确提交合入/双语日志；不实际App/DMG构建、不升版/真实tag/上传，环境/编译/夹具失败单列

Task4首次冻结记录：实际从 `5260f1ff268be838448436e91eb1ab2295f7a72a` 在 `/private/tmp/towermd-unsigned-build-task4` 开工。依赖准备首轮未指定既有store而缺缓存，保为准备失败；显式使用已有store后offline frozen安装99包复用/0下载、锁未变。本机锁定Tauri build help已实际成功；锁定tauri-utils源码确认base→平台配置为RFC7396 merge，CLI追加config按顺序合并。候选现已冻结 `0df0639cd6b17c5b51568418997ef1aa450ec657`（tree `126e167ededed144f621b0496963ae285fae9f37`），七文件精确patch SHA256 `f29d7272dcd142d5efbb117157d9c6f9aa95e215db97e7a5c1a16eb09223d56f`。Root核134受控输入/Git blob/工作树、73工件与21份命令；manifest SHA256 `6ce0b60e9889c73b505c196f732a92a375808380f2908c08697a1e755d4f64a7`。作者release31、关联180（含31，不累加）、P6精确53、check零错零警。五次行为RED与两次准备失败/中断保留；第一次help中断缺精确时段/child输出的限制明示，后续help才作能力证据。当时独立SPEC刚启动，尚未双审或合main；之后完成状态见下方回顾，不称真实App构建通过。

P8 Task5只读预备已完成并STOP：`/private/tmp/towermd-unsigned-assets-task5-preflight/notes.md` SHA256 `4db52789a1a7558e6b79831a3b5e74bf2de7a8e28499b35d1234f3a669d4627c`，冻结main `68be997659ed3d69f66ee84dd19c0b763fb353cd`，Root核20 Git blobs及notes/index。明确复用core九资产/checksum契约；该预备基线上Task4实际收据、P6 production资源布局与P7原生技能尚缺，必须在真实新SHA接收，不造dist目录、占位技能或空包。旧debug仅静态布局参照，不是首版验收；未实施、测试、挂载DMG或发布。

Task4完成回顾：实际main `272d5e9c303cd04988793de157cdf905314482c0`，来源上述 `0df0639`；Root核精确七文件和patch全等，除Root任务文档外全部134受控输入与冻结候选相同。独立SPEC PASS/C0I0M0，原31/180/53及自写6探针通过，报告SHA256 `b0ea58a80452c2493f52952a30cae053b7ae9f81f7e81ba96e682a7d7a7d7624`；fresh quality Ready Yes/C0I0M0，独立31+11通过（含两架构真实嵌套Node执行链），报告SHA256 `3c52b1213656fd004eb22003ad08bffba58fe980d23d9cae6af25bf050024fee`。Root核SPEC178/quality150工件及134实际Git字节；主线仅任务文档不同，未额外重复同字节用例，最终发布全套仍待Task6。本轮README与双语未发布日志同步工具能力，无真实App/DMG构建、tag、升版、人工验收或发布。

#### P8 Task5：包检查与冻结资产工具（已双审合入）

仅新增 `scripts/release/assets.mjs`、`scripts/release/assets.test.mjs`，沿原Task5共同接口；从本计划提交后的实际main接收已双审Task4，不读其他writer WIP。Root独占文档，不改依赖/锁、P7技能或已审构建脚本。生产缺P6/P7/真实收据时必须拒绝；可在临时Git/合成bundle/fake runner验证工具，不能把夹具称已验真实包。

- [x] 核实际buildFrozen收据/路径/锁/commands来源，确定inspectApp与assembleAssets最小接线；复用core资产集合、SUMS与冻结双语日志，不造尚无的前端/验收receipt字段或trusted开关
- [x] 可执行stub行为RED→GREEN：普通可执行主程序/CLI、单架构Mach-O、plist版本、依赖、必需ad-hoc验证及spctl结果如实记录；前端允许真实内嵌布局，缺少来源证明不得假定通过
- [x] fake hdiutil/plutil核DMG与唯一App全树hash/权限/链接，finally只清本次返回的设备，失败保原错误；本阶段不实际挂载DMG/运行旧App或CLI
- [x] 真临时Git归档固定tree的六原生技能/引用，解包前拒绝危险tar/PAX路径和链接、解包后逐blob/mode对账；核git archive实际tar.umask，必要用单命令配置固定权限，不改全局配置；P7实际缺失硬拒，不造占位技能
- [x] 组装五二进制资产→manifest/双语notes→八行SUMS，公开集合恰九项且无hash自引用；新输出与日志分离、同tag/commit/target/锁核验、缺项/多项/篡改均拒
- [x] 必要Node回归、完整输入/原失败日志、精确两文件提交冻结；独立SPEC→fresh quality后合main。真实双架构包/Intel运行/Finder/真人和公开回读仍归后续实际发布验收

P8 Task9只读预备已STOP：`/private/tmp/towermd-unsigned-publish-task9-preflight/notes.md` SHA256 `6d089d44ea9ffd8fb4f3c8babec2de4b16e89c83972050b070518476363a378e`，冻结 `762fff19292abed13ceb9d07aa41815335ab6e6a`，Root核九输入/19份本机help及全部SUMS。明确gh嵌套asset响应须官方/实际结构核实，Task4/5/6/8真实输入接收后才实现最终发布接线；不得用approved布尔值冒真人验收。未联网、测试或实现发布工具。

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

P3 边界候选续作：冻结 `p3-review-boundaries247`，实际index tree `7a8fa25a11f405871cb01e4e8b57b716a45769dd`，33个变更路径及全部index源码逐blob相等，未提交。Task2 Ask规格对齐与Proposal工作区变化均未混入。G4 checker把观察到的HEAD／authority漂移改为已有typed ProofRejected，真正未变基才是NotLatestMain；Review对后者仍证明原source／private与HEAD稳定。最终时钟回退为Source/uncertain，原UUID与业务拒绝边界保持。旧实现clock-red248、check-race-red249均为实际执行断言失败；修复版green250正在默认整批验证，后续串行stage253／native254／workflow255／静态检查已编排，任一步失败即停。旧snapshot两fixture只是显式选择新Some模式种子，保留完整projection及业务断言；246为snapshot16/16、snapshot_service39/40（仅该旧模式fixture失败），其余目标当时未执行，不算通过。

P3 实际验证推进：green250在强制当前源码重编译后 **18+15+16+13 全通过**；snapshot追加迁移fixture251 **1/1通过**，tasks/verifier/task_gates252 **31+16+21 全通过**。实际CLI stage253完成（33.17s），同源native254 **55/55通过**。workflow255真实运行268.84s，在新G1请求遇到 `baseline request <原UUID> uncertain: history authority/project binding changed`；定位为唯一validate_binding读到已切换main，产品按原UUID保留不确定状态。只将该精确已观察诊断加入夹具原30秒同正文重试白名单；264仅native测试一文件变化，全部CLI源码及随包二进制与253逐字节相同，无重建CLI。新index tree `42d0f7b62fbcec52d10081e781007b1597385426` 未提交，完整workflow265及静态检查正在串行运行；255保留为失败，不计闭环通过。


2026-10-18 本次继续验证：finish格式化327的专项320为14/14，workspace/all-targets Clippy328通过，329默认96项相关回归（HumanReview15、Snapshot16、TaskCheck13、TaskGates21、Tasks31）通过；嵌套child测试不重复计数。独立审查发现低风险合法自愿G1历史兼容性边界，正在补最窄校验修复，原准备index330暂不提交。Ask07-r1历史终态封装与晚到接受修复已冻结为333；334实际17/17通过，其中3项late_acceptance和14项既有Task/恢复相关测试，过滤器未覆盖Ask主故障模块，335另按完整模块执行。旧325/326失败保留，未被新结果覆盖；仍无本段正式发布或GUI通过结论。
