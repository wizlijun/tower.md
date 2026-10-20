---
type: Report
title: 首版未签名 App 发布前核对
status: draft
sources:
  - resource: ../specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: ../superpowers/plans/2026-10-17-m1-native-skills-acceptance.md
  - resource: ../superpowers/plans/2026-10-17-m1-unsigned-release.md
  - resource: ../../tasks/todo.md
---

# 发布前核对

## 当前自动化阶段

2026-10-20 解锁后已恢复f9edde6真实GUI：默认纯画布、实时已合并Task／Run ended／实际通过Evidence、按需B0→B1轨及原B0历史草稿末端均实际可见。FR42本次通过：App正常关闭后公开CLI新增T-qa-reopen-f9，重开接受#289 task.state，摘要准确显示关闭时间、原eid／actor，模块1/2已合并。原始记录为原QA `artifacts/app-f9edde6-resumed-gui-results-01.json` 及 `app-f9edde6-close-reopen-results-01.json`。这是agent_qa，不计真人P7。

实机同时确认两处失败：Merged Task历史入口仍错误读取新复核资格；扫描完成的候选节点被工具浮层遮挡。最小修复增加本机完整原件history读取、固定历史模式与原UUID回执重试，以及显式收起工具查看候选／保留同一扫描。前端272/272、类型与构建、本机9/9、fmt及native全目标release Clippy通过；不称修复包真机已通过。原f9 App已Cmd-W退出0；daemon公开stop exit1、快照结果不确定，随后实际无自有App／daemon。以下锁屏及旧FR42失败均为保留历史，不覆盖本次实际通过。

最新冻结候选为 `f9edde6ea161f6feb80afb480b07b95c58bc7740`，包含核心读取贯通和按需基线轨：实际构建65.12秒，417份HEAD对象与干净检出逐字／执行位一致，ARM包、实际包内CLI和技能检查通过。产物 `/private/tmp/towermd-candidate-f9edde6-20261020/Tower.md.app`；公开CLI status实际确认原Task=merged、Run=ended且无租约。唯一App75973启动后CUA返回cgWindowNotFound，实际CGSSessionScreenIsLocked=true，没有执行业务窗口动作，GUI验收仍未进行；用户解锁请求已发送。随后精确核路径SIGTERM本次App（exit=-15，不计正常关闭验收），公开daemon stop 0.386秒返回running:false，后验本候选App／daemon无残留。相关原始收据 `candidate-f9edde6-build-01.json`、`candidate-f9edde6-static-01.json` 及原QA `artifacts/app-f9edde6-initial-gui-blocked-01.json` 保留。不把构建成功当FR42／95秒／P7通过，不是正式发布。

核心有限历史读取修复已精确提交main `4a3675c45e15903c36e70ec43d5f6dd1347ba917`；新增完整B1计数1/1与旧角色／标签／完整人口／后续B2回归4/4分别通过，fmt及workspace全目标release Clippy通过，来源均不变。规格15.3基线轨补齐候选仅改App与按需组件，复用既有连续链证明；旧App入口真实RED2后，完整前端262/262（19.28秒命令）、类型零错零警告（2.54秒）、构建（1.56秒）通过。只显示核验链标签与上次真实完整读取草稿，不推定S回执、不新增API／依赖／样式／批准路径；独立源审无确定阻断，精确提交身份由外部owned收据登记，唯一新包实机仍待。

最新已提交候选 `b6918ef8193a2ace82063cdd9ec83a5caa42ae76` 已实际构建（23.56秒）、核415份HEAD对象与干净检出逐字／执行位一致，实际包内CLI和技能检查通过；不包含尚未提交的后端读取整改。实机Task详情历史入口可达，原UUID `01a157b5-9406-77d4-8b7b-34f3d5bce3c4` 显式重试后真实展示Current原回执、原body hash与原eid；第一次因Main实际更新返回uncertain也保留。两次App均Cmd-W退出0。第二次启动收据名误复用，保存碰撞文件及第一次工具stdout元数据，未伪称首份原始文件完好。

同一QA实际CLI主干闭环已完成，均标agent_qa：App关闭后新Run `01a15875-80ca-73b7-9975-f92471f64abc` 真正领取、代码与2项测试RED→GREEN、分离验证完成并passed，Evidence `E-01a15879-b42a-7216-be2e-4ca56e3b251d`；读取实际trailers后只改提交消息且tree不变，QA main快进至 `e35711aea8249c54e09c7dddce88fe4925de78ac`。actual `merge.done` #273／eid `git:20f4a9a663120efcc1aab9f23bdca9092a9f5c32855534f9c0154e28664d1898` 认证Task=merged／Run=ended，模块租约释放。原始证据见QA `artifacts/task-b6918ef-main-certification-actual-event-01.json`，摘要见 `task-b6918ef-main-certification-receipt-01.json`。公开stop后无自有App／daemon；不是两harness真人P7。

FR42仍未通过：关闭后实际claim存在，重开Task读取却报task_progress_source_changed，没有接受关闭摘要。完整hydrate物理读取新例在旧生产真实RED（B1 tree=2），单文件首GREEN仍FAIL（历史OID=10／2）；保全部原断言的八文件v2贯通静态读取后实际GREEN1（192.72秒测试／276.16秒含编译，来源不变），两次独立操作仍新鲜读取、实际blocks与水位保持。四项角色／标签／完整人口／后续真实B2回归实际4/4通过（1484.78秒测试／1484.91秒命令），来源不变；旧B1原回执重试保持历史效力，不能清除B2新要求。fmt通过，workspace全目标release Clippy通过（7.96秒，来源不变）；精确提交身份由外部owned收据登记，尚未构建，不据此称FR42／95秒GUI预算通过。

冻结候选 `77e177a` 已实际构建（65.68秒，415受控源相等），ARM包、实际CLI及技能契约检查通过。唯一新包实机已读取原QA实时Task与完整复核原件；原UUID只重试一次，界面在95秒传输合同下请求失败，没有成功回执。后台稍后已见原Q206／S207 confirmed，不能替代GUI验收。App29078已Cmd-W退出0，公开daemon stop返回running:false，后验无App／daemon。现按实际采样修复单次hydrate重复静态读取；后端尚未验证。原回执入口独立修复已通过旧生产真实RED及最终前端254/254（16.18秒）、类型零错误警告（2.43秒）、构建（1.56秒），来源不变；首批253/254与类型mock错误保留，未改生产来适配错误测试。精确提交身份由外部owned收据登记，同包GUI与整体验收未通过。原QA `artifacts/app-77e177a-recheck-gui-results-01.json` 保存失败和耐久事实。

本轮相关自动化已收口：前端249/249及类型零错误警告／生产构建通过；Hub9/9，冷／已登记复核HTTP3/3，新真实握手1/1及旧实时读取7/7，闲置Ready1/1及两项旧到期负例均已分别通过复测。初批归档断言错名导致的2PASS／1FAIL保留，生产没有为测试修改事件契约。最终workspace全目标release Clippy -D warnings（7.68秒）与fmt --check（0.95秒）通过，来源不变。所有独立改动精确提交main，未改依赖、超时、人工批准或完成认证条件；开始唯一候选构建，原QA／原UUID GUI和FR42仍待，完整workspace、B2旧UUID负例及真人双harness P7仍非完成。

冷启动复核HTTP实际3/3通过（670.17秒测试／670.35秒命令），来源前后不变：已有Hub但进程registry为空时纯GET可读，journal／refs／runtime／outbox不变且不登记；只有有效App人POST才打开实际已有Hub并完成原复核／同UUID重试／实际claim。缺失、完整损坏、无AppGate、坏UUID及异clone均拒绝。Hub9/9另覆盖预检后删journal／锁和追加不完整尾，existing模式不创建、不迁移、不修尾；v2源审发现被采用前修复，v3冻结版及格式化后实际测试通过。旧实时读取及最终质量仍在执行，新包GUI尚待。

2026-10-20 主干继续补证：旧九项回归实际7PASS／2FAIL，两个失败为main已到期却在有限证明封口后才重新申请租约，严格前缀检查正确拒绝；生产仅把真实main取得移至完整准备前，未放宽suffix或最终Run到期门禁。修复后两个旧到期负例均PASS；新增idle正例在Q/S成功后因测试误用事件名失败，初批2PASS／1FAIL保留。只纠正测试为实际`lease.granted`后，idle完整重跑1PASS（335.11秒测试／416.77秒命令），原UUID重试唯一、水位不变、新epoch真实grant及S=Q+1均通过，来源不变。最终全目标质量与HTTP／旧实时读取仍在队列，App尚未构建，不称整体发布通过。候选返回画布修复已提交main `da505af3792f0caee81545a8d8ee68df5edf18bf`，完整前端249/249、类型零错误警告及构建通过，待同包实机。

2026-10-20 继续主干：Task复核Q后慢证明移至Q前，复用有限same-writer／exact-prefix／Written seal，Q后仍核全部实际来源、最终物理时钟、完整subject/fences和严格S=Q+1；原UUID先查原回执，历史恢复保持原完整证明。隔离旧源码实际RED1，错误与GUI一致；修复冻结三项真实生产路径GREEN3（967.24秒测试／1049.69秒含编译），来源不变，覆盖准备续租成功、Q后真实续租拒绝及tag漂移拒绝后沿原UUID恢复。fmt与workspace全目标release Clippy通过（7.91秒），独立源审无确定阻断；九项Running到期／回执恢复／旧基线出版回归在途，App新包尚未构建，不称整体或实机通过。模块验证摘要独立改动已精确提交main `8311f25fdeb89069eb62e2f839069604a340ee41`，前端243/243、类型及构建通过，尚待同包实机。

单画布最终候选 `92456eb8833797a36696e7cb4f13779b3316b8b4` 已在main；完整前端234/234（20.12秒）、类型0错误0警告（2.53秒）、构建（1.53秒）通过，来源前后不变。415受控源与干净构建检出相等，ARM包构建25.10秒及实际包内CLI／技能静态检查通过。最终包真机确认空画布／真实模块地图没有常驻栏、表单、默认侧栏及库链接；节点详情按需打开、关闭后恢复纯画布，右键只有一份菜单且Escape可关。前一4320fd8候选已实测任务下钻仍保留画布及工具开关；最终仅隐藏库署名。App54816已Cmd-W退出0，同runtime公开daemon stop返回running:false，后验无自有App／daemon进程。原QA `artifacts/app-92456eb-canvas-gui-results-01.json` 与外部 `canvas-only-final-gui-01.json` 保留实际结果；没有新批准，不将界面验收称真人P7或正式发布。

单画布主改动 `4320fd83b3f9521a16cf03266f2ecee37ebed00f` 已在main，415个受控源与干净检出相等，ARM候选构建25.03秒及实际包内CLI／技能静态检查通过。真机观察默认空画布、一次点击选择／取消、Cmd-O、真实模块地图、按需节点详情／Escape、单一右键菜单、工具开关、任务画布及临时任务详情均符合预期；没有新人工业务动作。实机仍见右下角库署名链接，正在最小移除后重验；前一候选不能算最终纯画布。唯一App Cmd-W退出0，同runtime公开daemon stop后无自有进程。原QA `artifacts/app-4320fd8-canvas-gui-results-01.json` 保留；任务复核／FR42／完整发布仍待。

2026-10-20 按用户“主界面只有一个画布”裁决，前端移除了常驻标题／状态／关注／工具／插话／默认侧栏，改为节点内状态及选中或右键后按需浮层。规格12.5及15节同步展示裁决，业务门禁不变。最终冻结完整前端234/234通过（16.70秒）、类型0错误0警告（2.56秒）、生产构建通过（1.54秒），来源前后不变；首批229/234及5个断言问题的实际日志保留。精确提交由外部owned收据登记；唯一候选实机仍待，不称发布完成。

上一候选C9 `d2cb9d1` 已构建核415个受控源相等，ARM包及实际包内CLI／技能静态检查通过。原QA的Task态势实际接受seq182，原B1批准UUID的GUI回执已真实展示，目标B1与提交fb6ca2e一致。随后Ready复核原请求01a157b5-9406-77d4-8b7b-34f3d5bce3c4返回uncertain（Q advanced before S），保留原UUID，不能算复核通过。第一次App存在未知发起的原生窗口关闭／exit0，第二次App以Cmd-W正常退出0；公开daemon stop后两自有PID均消失。实际结果保存在原QA `artifacts/app-d2cb9d1-gui-results-01.json`。FR42／真人P7／完整workspace及正式发布仍未通过。

后台出版selector的完整只读证明已移出producer锁，短锁只捕获输入；选出pending后仍重新持锁加载真实Catalog／outbox并走原完整publish，核心追加物理输入后验探针。实际屏障回归通过：后台Git仍blocked时App投影与Status均在原3秒返回、journal不变（9.10秒测试）；原Status／Claim并发回归及五种出版断点恢复已通过，真实outbox漂移仍拒绝（12.52秒测试）。两个初批均1PASS／1FAIL如实保留，失败分别是重复打开独占outbox、空记录误期望recorded，测试仅改用原owner和通过Hub实际接受审计事件，生产及原时限不变。格式及workspace全目标release Clippy通过（7.54秒），来源不变；新包真机尚待。

由C8原retry实际采样定位的重复静态读取修复，已完成11项真实回归（572.12秒测试／653.30秒含编译），来源前后不变，冻结及格式化后版本均独立源审无阻断。复用范围限同一有限读取内的完整OID／实际历史配置／完整bundle，首次完整读取，失败不入缓存，命中前后仍核当前来源；不缓存当前权限或accepted事实。完整原件／实际人口／binding及祖先校验保留；真实tag/private/draft漂移、完整bundle与owner隔离、缺精确提案／父tag变更拒绝等全部通过。尚未构建实测此修复。

最新真机候选C8 `f972c64` 已实际构建（66.89秒）并核415个受控源相等，ARM包／实际CLI与技能静态检查通过。真实项目、控制记录、模块下钻、原草稿及完整审阅正常；Task仍报来源变化及finite publication reads来源变化，未接受水位，原B1 UUID只重试一次。期间真实snapshot.written=174、main从5fe466a3前移到8ce8dfe7；采样显示Task等待producer，后台pending出版selector持该锁执行完整只读证明。原回执结果观察前App退出0；系统极窄日志证明AppKit mouseUp／原生窗口关闭后退出，无本次崩溃报告，发起者未知，未归因用户或工具，也未找到retry主动close路径。公开daemon stop完成，两自有PID消失；原QA `artifacts/app-f972c64-gui-results-01.json` 保留实际结果。Ready／FR42／P7仍未通过。

针对C7实际请求账本采样的四路径身份预留修复完成十项真实回归（87.42秒测试／168.51秒含编译），来源前后不变；冻结版及格式化后版本均独立源审无阻断。预留扫描只返回完整canonical manifest内的typed request，不能构造SavedDraft、accepted或receipt；真实坏payload仍占住原UUID但无成功回执，同UUID改正文、完整读取／保存／批准均拒绝。项目／schema／hash／canonical与非commit负例、全引用集后验、历史完整reader及既有ledger不确定／身份冲突断言均通过。尚未构建此源码的App，不称任务读取实机已修复。

历史真机候选C7 `d389800` 已实际构建并核415个受控源文件相等，ARM包与包内CLI／技能静态检查通过。原项目控制记录、历史消息送达及模块下钻可用，任务读取仍返回 `task_progress_source_changed`，没有已接受水位，未进入Ready复核。现场采样定位到请求账本为预留UUID完整重验历史草稿；期间确有快照与main变化，未将来源变化判作误报。App经Cmd-W正常退出0，公开daemon stop完成且两个自有PID均消失；原QA `artifacts/app-d389800-gui-results-01.json` 保留实际结果。

原回执读取的最小修复已实际通过四项真实出版／重建／缺失私有快照回归（1150.82秒测试，1233.00秒含编译），来源前后不变。仅在既有候选唯一性、完整出版证明、原批准binding、S及所有标记的本地确认和最终来源探针均通过后读取已有回执；缺失仍由原恢复路径补齐。真实B1新增断言证明不进入标签写入、不改refs／journal／outbox／水位，并验证归档标记缺少本地确认时仍补齐。尚未构建实测此修复，不据此称95秒、Ready或FR42通过。

2026-10-20 用户已恢复真机测试。C5 原项目选择、历史草稿及完整审阅页可用；沿原 B1 批准 UUID 重读时，界面报“出版结果缺少真实提交或目标”。任务态势另报 `task_progress_source_changed`，Ready 复核／Running／关闭重开尚未通过。唯一 App 已 Cmd-W 正常关闭，daemon 公开停止，本次自有进程均退出。原 QA `artifacts/app-a46d396-resumed-gui-results-01.json` 保存结果，不是 P7 真人验收。

前端回执解析修复 `d7f0689` 已用真实 serde 事件取得 RED1（同 GUI 文案）→完整前端232/232、类型零错误警告、构建通过；独立源审无阻断。修复读取 `recorded.created`，保原批准绑定并展示提交／目标／出版回执。历史首次 `.resolved` 是旧 pending，不作为本次原响应。

C6 `d7f0689` 已实际构建，415个受控源文件与干净检出／构建前后相等，ARM包及CLI／技能静态检查通过。实机再次打开同一项目、历史草稿及完整审阅，沿原UUID协调在服务端阶段超时，界面显示 `/app/requests/<原UUID>/retry` 请求失败，未到达前端回执展示。因此修复尚无实机通过结论，Ready复核及FR42仍待。App以Cmd-W正常退出0，公开daemon stop返回running:false，两个自有PID均已消失；结果见原QA `artifacts/app-d7f0689-gui-results-01.json`。没有新批准或正式发行。

随后终态提案诊断的窄修复完成实际五项回归（606.59秒测试）、格式和workspace全目标release Clippy，来源前后不变。已批准提案不再为新的待评审卡重复执行完整出版检查，原批准／保存原件／完整binding校验保留；未决定提案仍完整检查。该结果不证明原回执95秒或任务进度实机通过，仍待新候选。

`b184a9c` 的完整 workspace 批次已中断，无最终退出 JSON：6 个已结束 target 共142 PASS／3 FAIL，daemon_cli 仅观察30 PASS／10 FAIL，core 与后续集成尚未跑完，不能称整体通过。中断观察核413个其他受控输入仍与开跑冻结相等，tasks 计划仅在确认进程停止后更新。下方保留早期批次记录，现有失败未解决前不发布。

最新已实际构建候选为C5 `a46d396aa430300e87716ae15bea04bbd543ea1b`，包含C4后expiry修复；414个受控文件与干净检出逐字一致，实际App构建、ARM包静态检查及包内CLI／技能检查通过。新鲜前端230/230、工具664/664、类型零错误警告、前端构建、fmt和workspace全目标release Clippy通过。完整workspace首target实际114 PASS／2 FAIL，后续core及集成未执行，未达到全测通过；原收据 `candidate-a46d396-workspace-all-01` 保留。用户明确先自动化、稍后实机；唯一C5 App已核路径退出，未点击业务，不计关闭重开或P7。

冷启动offline测试边界修正已精确提交 `0fe3140`：原两秒首Add单独也实际失败；仅Add／Claim准备沿用既有CLI120s，Status／Show仍2s，时钟／20min续租／全部fence／36min到期断言逐字不变。实际新测试1/1通过，Add2.683s、Status176–184ms、Show696ms；不声称冷写入满足2s。

随后既有十并发CLI原2s测试实际失败3.047s，真实fetch仍blocked；阶段诊断定位inbox同项目producer排队。窄候选无待协调Delivery时沿完整来源证明并发poll，旧原件仍原队列；原2s测试已实际PASS（1.209s），workspace Clippy通过，新增来源／不确定原件回归实际2/2通过（91.55s测试），旧CLI delivery批次实际6/6通过（304.67s测试，含共享解析器），旧入口／ack／delivered后缀逐字不变。独立改动的精确提交身份由外部owned收据登记，尚未打包。完整workspace／native、GUI、真人P7及正式R/tag／双架构发布均待。

## 历史C4候选记录

本表按 2026-10-19 至 2026-10-20 的实际续作记录整理，尚未达到发布条件。最新候选 C4 为 `9a29d354f33c430eb239b97539c1759ec3037fd0`，tree `aea17d1ccad678f411c67c2293702b5682fc1a3c`：读取与最终时钟修复已提交，414个受控文件的实际完整构建、ARM包静态检查及技能／同包CLI契约通过；格式和native全目标Clippy通过。App当时仅启动一份，系统报告屏幕锁定且CUA返回`cgWindowNotFound`；用户随后选择稍后实机，唯一App已关闭，尚未进行C4业务点击，95秒原回执、复核及关闭重开均未通过验收。C3曾恢复真实原B1及标记但原回执超时，不能用其耐久事实替代C4 GUI。下方保留C3及更早记录；没有发行 R/tag、真人 P7 PASS 或已上传的 0.1.0 资产。本文不填写包含自身的提交号，最终来源身份由外部冻结收据登记。

C4产物 `/private/tmp/towermd-candidate-9a29d35-20261020/Tower.md.app`，CLI SHA-256 `9318e53f05232c3a68bcf4959072816b7b2d436000023f187a422c925025cbd0`，App二进制 SHA-256 `0a701789e12fc0440cccbb7604a74c8412ac813f139f0caad28c1ee00aec304e`；构建66.25秒，受控来源前后不变。前端90个输入与已测C3相等，复用230项／类型结果而非新执行。原始收据 `candidate-9a29d35-build-01.json`、`candidate-9a29d35-static-01.json`、`candidate-9a29d35-gui-blocked-01.json`。七项受影响出版回归实际7/7通过（818.58秒测试），冻结来源前后不变；完整workspace／native回归仍待；新P7项目已准备并完成初始配置导入，十卡not_run，未有B0／Task／真实harness会话或真人批准。

## C4 后的未打包修复

Ask 来源竞态夹具已提交 `8934533`，原断言实际1/1通过；仅cfg(test)，不改产品来源门禁。finish expiry 修复补全真实 TaskCheck 历史与来源角色，保留严格整Run一致性；核心9/9和全目标Clippy通过，queued／未知所有权Running／破损来源下自有清理三项实际PASS。四项daemon首次批次为3PASS／1FAIL，随后两次单独RED及无扰动诊断确认：真实expiry已完成，但worker留下的恢复信号启动重复flight。最小私有交接避免该重复工作，保留真实输出、原15秒／60分钟及完整expiry证明；最终 `expiry-owned-handoff-five-01` 实际5/5通过（195.38秒测试），新增真实ticket／输出与scope负例也通过，全目标Clippy通过，来源不变。原失败保留，完整冻结回归仍未通过。源码提交身份由外部owned-commit登记；这些后续修复尚未进入C4 App，也没有新的实机或正式发布结论。

## 用户流程、日志与来源

读取整改 V2 已精确提交 `0e6f529`，等待新候选实机复验：完整上下文／原件／binding／parent／tag 变化与有限 owner 边界用例实际 1/1 通过（186.64 秒测试）；真实 B0/B1/B2 和旧复核 UUID 隔离用例实际 1/1 通过（1146.10 秒测试）。两批各自来源前后不变，首次错误测试前置失败及 V1 中断仍保留。V2 只复用同次读取内已完整核对的提案上下文，仍实际检查 refs、来源、祖先和各来源角色。没有 95 秒 GUI 通过结论。

TaskRecheck 到期窗口已形成实际红绿证据：仅恢复最终旧 Q 时间的隔离负控两例 RED，均已接受到期后的 resolved 原件并返回 HistoricalOnly（337.40秒测试）。修正版最终锁内重取实际 clock、结算到期后检查完整 Q／subject／fences；整批两负例、Ready和Running正例及五项不可变读取实际 **9/9通过**（862.77秒测试），workspace全目标Clippy通过，来源前后不变，独立源码复核无阻断。本段代码提交由外部收据登记；未有新App实机／真人P7／正式发行结果。

| 功能／双语未发布条目 | 实现来源 | 已有实际验证 | 当前 main 与候选边界 |
| --- | --- | --- | --- |
| 实时态势、Z0→Z1→Z2、直接打开原问题／评审 | `a57f9ae`、`8ff3191` | 前端及核心回归；8ff 同包真实 GUI 问答、任务增量和运行状态 | 已进入 main；新整包仍须实机复验 |
| 插话投递、按范围暂停／恢复、关闭后变化和依赖 | `b1badeb` | 核心／HTTP／CLI、前端与 native 回归，详见任务记录 | 已进入 main；新 Control 及关闭窗口 GUI 尚未验 |
| 源绑定公开提案与原 UUID 重试 | `7eac536`、`c001d8d`、`219d10d` | 实际 core、HTTP、CLI 和技能契约 | 已进入 main；Bn 说明更新已随37e1d43提交 |
| 同画布提案审阅→后续基线→完整任务复核 | `37e1d43` | 真实 B1 出版、Q/S 人口、pending marker 修复；sealed subject 2项及耐久恢复5项；关注列表4项和旧 Ready 点击负例 | 已进入 main；HTTP两项已通过；真实 B2 旧请求负例和新包 GUI 待完成 |
| 越界检查只读诊断与真实新检查替代 | `37e1d43` | 真实 producer 10项；Passed 增量安装来源／归档边界2项、受影响回归5项 | 已进入 main；无完成／批准捷径，新包 GUI 尚未验 |
| 后台验证、trailers、main 认证与执行清理 | `919f4ad`、`fafc27e` 及后续 main | 原真实合并／到期恢复回归；本轮 release profile 原并发测试通过3秒交互与共享15秒启动 | 当前改动保留门禁与原 Evidence；debug 时延失败保留，不能称 debug 已修复 |
| 六项原生技能与手动安装 | `a57f9ae`、`219d10d`、`37e1d43` | 本轮实际 CLI 契约 ready=true、整合655项；strict manifest 历史实际检查 | 两种真实 harness 的选择器及十卡执行未验 |
| 原始验收材料核对、版本同步及临时重签兼容 | `0c70d56`、`af318a0`、`2df5ffe`、`5ebad5d`、`021f493` | 本轮技能／验收／发行整合655项；历史实际 DTO 277项、签名资产132项及两份 ARM 产物探针（不相加） | 工具测试不代表当前包／Intel／真人验收通过 |

本地原始日志及来源哈希保存在 `.release/validation/2026-10-19-corrected-mvp/`。每批冻结来源以各自 JSON 为准；不把旧源码通过数累加成当前源码全测结论。`bn-publication-faults-03` 是中断批次，仅6项完整通过；最后同名用例后来在 `bn-pending-markers-green-01` 单独通过，不改写为整批7/7。`bn-task-recheck-later-subjects-01` 中断前5项完整通过，未完成的 B2 用例在 release profile 重跑约14分钟后也按候选交付顺序主动中断；没有PASS或断言失败结论，仍是正式发布门禁，后续在冻结候选上补完。

## 发布前仍须完成

用户已选择稍后GUI，C4唯一App已正常SIGTERM停止；未计关闭重开验收。完整workspace首个CLI批115项仅观察到53PASS、8FAIL、2挂起，因确认的测试clock自重入锁而精确中断（18448.13秒，来源未变）；core及后续集成尚未执行，完整验证不通过。后续实际nocapture区分为fixture时序／transport合同和真实expiry证据上下文遗漏。首次夹具整改形成六项分批PASS：恢复actor／epoch两项，安装失败保护一项，三项实际Git屏障并发用例。后续Ask1项、expiry核心9项及最终daemon5项分别实际通过，详见上方C4后修复段；保留所有旧失败／中断，不能合并这些分批结果充作当前源码全测。全批及GUI／P7均待。

- [ ] 本轮实现及说明精确提交，受控源码／锁／版本同步为实际候选；以真实构建和源码比对记录 C。
- [ ] 版本提交后的 workspace、native、前端、类型／构建及发行工具必要完整检查全部通过；不以历史目标测试替代此项。
- [ ] 同一候选 App 自行实机验证 Control、禁止诊断、正常不手动快照的 B1 审阅／出版、完整任务复核和关闭重开；自动化结果明确为 agent_qa。
- [ ] P7：真实 Claude Code／Codex 各五卡、三个工作树、十任务 Run→tree→Evidence→main trailers→认证合并及合并后验证；真人实际 App 回答、批准和恢复确认。
- [ ] 最终枚举所有 worktree dirty/untracked、分支未合入提交和 stash；记录实际 inventory digest、补丁等价和每项裁决，保留尚未证明的旧工作稿。
- [ ] 核 C→R 受控差异；R/tag、Apple Silicon 与 Intel 未签名资产和九项发布材料逐字绑定；实际上传、下载回读和本机安装探针。
- [ ] 合入 main 并完成核实后的本地分支／工作树清理；不删除未保护的 WIP 或远端分支。

M2 多机／知识回写和 M3 无头调度不列作本次 M1 欠账。当前没有豁免、自动批准或把程序化 human actor 当真人证据的范围裁决。

## 2026-10-20 候选提交前补记

公开复核HTTP2项、release profile原并发用例、技能／验收／发行工具655项、实际CLI契约及前端226项和类型检查均通过。native 83单元、7 CLI适配、16项目适配以及3工作流项已分别通过；原整批02中真实workflow因准备前缀竞争失败，局部测试协调修补后03同项通过，其余生产源码未变。隔离子进程同名测试不重复计数。以上仍不等同新 App GUI、B2旧请求隔离负例、冻结后的完整workspace或真人P7通过。

0.1.0 版本准备使用既有工具，日期为2026-10-20；仅本地Tower包版本／对应锁及双语日志轮转，第三方锁内容未变。这里准备的是验收候选，不是正式发行R；候选C由后续实际提交和构建收据登记。

## 0.1.0 候选 C2 的实际实机结果

`4f6adf4` 完整构建成功，443 条构建命令退出零，412 个受控源文件前后逐字不变。包内 CLI SHA256 为 `b0db8b4e200b44df091fadd14a4ed27c378352f7589f1b8753dceb7bd237e7b7`；App 可执行文件为 `d0d32dde235c3fd7876a96e2275f724ae33a5c48bd394ccdca186c0c9648f558`。实际 arm64、临时签名完整性及同源 CLI 检查通过；单次有界系统评估按未签名包预期拒绝，没有绕过安全设置。仅 ARM 候选，不代表 Intel、DMG 或发行通过。

用户解锁后，唯一候选 App 在仓库外独立 `agent_qa` 项目实际打开权威画布；三种范围的插话由同包 CLI 完整输出并显示送达，暂停与精确范围恢复成功，G2 越界实际拦截并能直达只读原件。公开 `propose` 无手动快照即可进入首屏待批提案，审阅页显示完整绑定、原文差异及保留坐标。

**阻塞：B1 出版仍未完成。** 临时夹具原批准 `01a154d9-dae5-71ac-967e-916e2e188f48` 已记录，实际 `towermd/B1` tag 为 `fb6ca2e02517b4cced19a6dafb8fc2bb38f6245c`，但 canonical 记录返回 pending，原因是 Q 后的完整证明期间真实续租推进了水位。同一原请求重读仍 pending；历史效力提示不等于批准撤销，也不等于出版成功。未手动 snapshot、删除 tag 或另建批准。任务复核和关闭重开全链尚未完成。App 已正常退出、Run 已经公开 CLI 释放、本夹具 daemon 已正常 stop，原两个 PID 均不存在。

上述均为自动实机测试，尽管 App 原件使用本机 human actor，意见已明确标注 `agent_qa`，不能记作 P7 真人验收。原始图像、CLI 输出及采样在 `/private/tmp/towermd-c-agent-qa-4f6adf4-20261020/`；静态收据在既有 validation 目录。并发读取瞬态提示的修复已有实际 RED／GREEN 记录，正式进入新候选仍待精确提交和实机复验。

## Q 后慢证明阻塞的修复候选

有限私有 writer 证明将完整出版／人口／Session 校验前移至捕获 Q 之前，严格纳入真实续租，实际 Written 输出后继续核当前来源、真实时钟、完整 Session 及未到期 fence，再耐久追加唯一相邻 S。没有放宽 Q+1、停 ticker、增加 TTL 或修改格式；已有 S 仍从原 S 的固定人口恢复。首次编译错误及独立审查发现的旧时钟回归已修正；FR-89 心跳测试以 omitted 元数据证明覆盖，不归档纯续租。

冻结源码实际通过新增四项真实生产路径回归、既有 AfterTag 原批准恢复和私有 Q／已有 S／缺失标记恢复，共六个不同用例；workspace 全目标 Clippy 无警告，原失败日志保留。独立七路径源审记录在 `.release/development/corrected-own-q-review/report.json`，未冒充运行测试；Root 的批次收据均记录来源前后不变。本段不代表修复已实机验收：必须将源码精确提交、构建新 C，并在原临时项目沿原批准恢复 B1。实际 GUI、B2 旧请求隔离、冻结完整回归及真人 P7 仍是发布门禁。

## C3：canonical 已恢复，App 回执读取仍阻塞

核心修复已精确提交 main `524fd2950618eaf671c849b7def5883a6de23976`，tree `7e37dda3a00bf1589668d9696836419c91a3ba7a`；前端瞬态读取修复为 `79ff51a`。C3完整构建成功，413个受控文件不变；实际ARM／临时签名完整性、随包CLI／技能检查通过，前端230/230通过。CLI SHA256为 `ac43f3a7eb3ae8ebcbd103215b3715e1d6ebea10e5f450f357b487e9b7f19bf0`，App executable为 `260444c294d4d67d1328206af8043e2f481dd044de611a3902a20161670ca731`。不是正式发行。

用户解锁后，同一临时项目、同一原批准恢复实际B1：固定Q `95155af1b107a43f968fe46eda9419c35cc98fd4` through_seq=108、唯一canonical S=109、T-qa required marker=110。原批准仍为 `01a154d9-dae5-71ac-967e-916e2e188f48`、B1 tag未变且只有一份批准；事件与精确断言见夹具 `artifacts/app-524fd29-canonical-b1-proof-01.json`。

**用户流程尚未通过。** App「读取或协调原审阅结果」超过既有95秒总请求期限，显示网络错误，未展示出版完成回执；主画布随后仍在读取。真实采样显示历史出版及固定来源重复证明，正在做最小读取整改，不放宽超时、门禁或来源校验。Task复核和关闭重开仍未验；App已正常退出，公开daemon stop报告在途写入仍待停止，须另核PID。单次网络超时不证明原批准失败，不能另建请求。原始AX、截图、采样及stop结果留在同一夹具artifacts；自动QA不算真人P7。

P7准备工具已经创建 `/private/tmp/towermd-p7-524fd29-20261020`：实际三个detached工作树与6个种子测试，十卡全部not_run。尚未init或启动harness；该准备来源为C3，不能据此声称后续候选或真人验收通过。

C3 GUI结构收据 `candidate-524fd29-gui-01.json` 已封存10份原始材料哈希；后验确认本轮App与daemon两个PID均已消失，未强杀。C3类型检查另实际通过0错误0警告（3.44秒），来源前后不变。

## 首次候选构建的实际失败与修复

候选版本提交为 `422be454394367796ed0b4da81d54ad735213b6b`。干净源码下实际App/CLI构建命令成功，但技能归档因旧白名单遗漏已有作者署名失败，没有完整候选PASS收据，未启动该App。仅对齐既有技能checker的严格可选author.name；真实形状Git归档正控实际RED后，修复版连同无效署名／hook负例的整合661项全PASS。必须在包含此修复的新候选来源下重建；此记录不代替新包、GUI或P7。
