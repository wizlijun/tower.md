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

本表按 2026-10-19 至 2026-10-20 的实际续作记录整理，尚未达到发布条件。功能实现已提交 main `37e1d43857eb5b30b7ae252446ff0d83d60cdc99`；版本和技能归档修复后，实际候选 C 为 `4f6adf496da1c3912b851181323cdda3a4eacdd2`，tree `95acab45940b64a10cba987302fd0e346d483d0c`。该候选完整构建和包静态检查通过，但实机 B1 出版遇阻，必须修复并生成新候选；没有发行 R/tag、真人 P7 PASS 或已上传的 0.1.0 资产。本文不填写包含自身的提交号，最终来源身份由外部冻结收据登记。

## 用户流程、日志与来源

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

## 首次候选构建的实际失败与修复

候选版本提交为 `422be454394367796ed0b4da81d54ad735213b6b`。干净源码下实际App/CLI构建命令成功，但技能归档因旧白名单遗漏已有作者署名失败，没有完整候选PASS收据，未启动该App。仅对齐既有技能checker的严格可选author.name；真实形状Git归档正控实际RED后，修复版连同无效署名／hook负例的整合661项全PASS。必须在包含此修复的新候选来源下重建；此记录不代替新包、GUI或P7。
