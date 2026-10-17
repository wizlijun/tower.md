# 任务计划

## 2026-10-18 最小完整本地项目画布

按用户“请实现出来，要求功能最小化完整”：本轮交付打开真实 Git 项目→读取实际结构→画布与原文详情→刷新/切换项目→可安装 App 的闭环。已有权威画布读取固定 main；无画布时显示明确的 Git 目录预览，不冒充已批准结构或真实任务。只读路径不启动 daemon、不续租、不写仓库。保留已有显式初始化功能。本轮不扩展批准、调度、实时任务或发布工具。

- [x] core 只读项目概览：权威画布/真实目录预览、固定提交、无提交空态和错误诊断
- [x] native 有限令牌读取，前端选择/加载/刷新/错误/切换完整接线
- [x] 参数化画布，保留权威坐标，详情原文展示及大图导航；分组位于普通节点下方，目录预览每列最多三行
- [x] 针对性验证与 arm64 debug App 打包：前端67项、native24项、core概览5项通过，check零错误零警告，fmt与build通过；最终分组样式及布局后分别再跑App/Picker12项、core5项、native读取1项
- [ ] 精确提交源码及记录提交号

实机回顾：最终候选 `.release/prototype/2026-10-18-minimal-final/Tower.md.app` 已通过CUA启动并观察截图。先用系统文件夹选择器读取实际Tower.md仓库，展示7节点目录预览和src路径原文，缩放24%→35%；随后把目录预览收紧到三行多列。最终包在自有临时Git仓库读取带负坐标、后置group的既有canvas，显示3节点与关系，节点原文正常；空白拖动的前后截图确认视口平移，适应画布与刷新可用。临时仓库提交README新正文后，刷新从22932e1b切到7f710911，新正文实际可见。没有触发初始化、批准或daemon。此前窗口工具故障已在本轮恢复，不把旧记录抹去。

产物是Apple Silicon本地开发预览，版本仍0.0.0、无发行者签名/公证、非正式release；未验Intel。批准、Agent调度与实时任务展示不在本轮最小只读闭环范围。随包CLI由最终源码重新stage，App和CLI均arm64；bundle文件hash保存在同目录bundle-files.json。

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
