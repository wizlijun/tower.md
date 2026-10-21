---
type: Verification
title: M1 本机真实主路径验收
status: draft
generated:
  by: process:towermd-qa-report
sources:
  - id: spec
    resource: ../specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: M0/M1 范围与验收标准
  - id: originals
    resource: /private/tmp/towermd-m1-qa-20261021/records/
    title: 本机隔离 QA 原件；逐文件摘要见正文
---

# 结论与范围

真实 Claude Code 2.1.303 与 Codex 0.163.0 在同一隔离仓库的三个工作树完成十个任务，均由 daemon 认证 `merged`；App 的 B0/B1、问答与插话、暂停/恢复、项目关注前三项、main 捕获/撤销/认领及 Hub 重建已经实际操作。本报告按当前审阅源码 `ba3a2f61b746b86bde24ef15ac5b9377dc376189` 汇总分阶段证据，不把全部步骤误记为同一候选完成。

这些证据支持发行 **macOS Apple Silicon 的 M1 最小预览**，不是全部 spec 完成证明。Intel、NFR-03 定量性能、指定 Note.md 仓库 M0 验收未完成；M2/M3 不在本报告范围。无发行者签名或公证。最终版本、发行 tag、冻结源码与二进制 manifest 仍由发行流程绑定；本报告本身不证明已经发布。

**恢复后续跑已完成：B1new 由真实 Claude 在新运行中完成验证与 main 认证，原十任务加额外一个任务均有实际完成证据。**

最终自动回归：Rust 工作区 380 项、前端 119 项、原生层 20 项（包含实际随包 daemon 重启及 WebSocket 重连）通过，类型检查零错误/警告，发行日志脚本 5 项通过。0.1.1 的项目版本配置与两份锁文件同步，未改变第三方依赖版本。最终包与本报告的冻结源码由发行附件 `release-evidence.json`、`SHA256SUMS` 及 `v0.1.1` tag 绑定；仅这些实际产物可证明交付内容。

# 环境与真实性边界

QA 根目录为 `/private/tmp/towermd-m1-qa-20261021`，三个工作树为 `worktrees/one`、`two`、`three`；独立 `origin.git` 保存实际 main、基线标签及证明归档。产品仓库和用户其他项目未作为故障注入对象。各 harness 与验证按单队列执行，争领场景通过保留同时有效的运行租约测试，没有启动多份 App/daemon。

十任务汇总绑定候选 `4e980742df2b788dfcead97c9e34d0b17857437e`；B0 在 e721cc3 操作，T01 暴露的问题经 9539a2f/9e8d0b8 修复后继续原仓库认证。B1 在 bc3c517 操作，main 捕获后的处置入口在 ba3a2f6 修复并重新实测。源码和实际阶段不能相互替代。

Codex 的共享 Git index 写入受其沙箱限制，Root 代做的 Git 写入和部分真实 CLI 检查点均另有记录；功能代码来自实际 harness 回合。T09 的 harness npm 调用被权限限制时先运行真实 Node 测试，最终仍由 daemon 执行已批准的 npm 命令。没有以聊天自述、手造 Evidence 或放松门禁代替完成。费用保持未知 `null`。

# 十任务与门禁

下列提交均是 certificate/summary 中的实际 main 接入提交。十份 `certificate.json` 的 SHA-256 已逐一与 `ten-tasks-summary.json` 核对；各证书含完整 Evidence、实际 merge 事件、快照归档及真实 main 测试记录。汇总时十个运行全部 released，main 为 `4c78c71d1881ddc5f7b4d9cbb24bd08995f391d0`。

| 案例 | harness / 工作树 | 证明的行为 | main 接入提交 |
| --- | --- | --- | --- |
| T01 | claude / one | 普通 finish→G5→main 认证 | `a616a133682ef0f5661e93f17749847d11e895ae` |
| T02 | codex / two | App 非推荐回答与插话实际送达 | `4c52b64906821b87a73662e5e44130623b789d73` |
| T03 | claude / three | 只自述 done 不计完成；再真实完成 | `f6d3b445aaeb470041bc72e3fb01493e8723d9bd` |
| T04 | codex / one | 真实 G3 失败后修实现；旧失败保留 | `9688493db6a3ea2d83f3e19c80c7a0c2dd0f309e` |
| T05 | claude / two | 同 tree amend 复用 G3 | `ec1891bba816e7f85afd2adc3a787a4e5d6c37e7` |
| T06 | codex / three | 同 tree squash 复用 G3、重做 G5 | `89af06d214d1bbea11c9a3ef465a374efbc352b5` |
| T07 | claude / one | main 仅 state/ 改变，rebase 复用 G3 | `a80de4255372b60d4c74278f72b48b60ad6cbd29` |
| T08 | codex / two | 代码基底改变，旧 G5 拒绝、新 finish 通过 | `cfe8c25c2d21e10511d7f00cc856660e4a7ff3ab` |
| T09 | claude / three | 慢层 canvas 越界被 G2 阻断；合法代码才合并 | `9a811c92b49b7f25a8b7ce702d5ad71a569e5448` |
| T10 | codex / one | 双模块争领失败不漏部分租约；同请求后成功 | `036b5461cfad2e81498cc6c07ce50b065a4e93d3` |

T03 的负向检查为 `evidence_count=0` 且工作内容未变化，不计完成；之后取得真实证据才进入上述十项。T04 原失败 `E-01a15c89-5a77-78b0-a94e-44cd99d92a06` 仍为 failed，新证据 `E-01a15c8a-efe1-70ad-beb9-6fdf238c99eb` 合并后才解除旧门禁告警。T09 被拒的慢层提交 `a4fcd2e85d9eb6fa92419d15d2c5387a03074a1a` 保存在 `refs/qa/T09-protected-negative`；finish 的真实路径是 queued 后在准备期因 G2 interrupted，没有生成通过证据。

T10 在 T08 占用 text 模块时失败；冻结 journal 证明未占用 math、未创建 T10 run。T08 合并后沿原 request-id 重试，最终验证覆盖 math/text 两个目录。不是改请求 ID 后跳过原失败。

# App 批准、消息与关注

- **B0：** App 审阅三模块、确认原命令、实际重验证后批准；提交/tag `6a3cb91453f5a76177320c2e4abb3e74d2b17252`，批准 eid `01a15c5f-ef7d-7268-bd7a-536ab465b6a1`，首快照 `a3258ae01a1dc2fb4dbe1c04d32498dcd42c2a69`。`binding.json` 核 actual tag、基线、批准和冻结草稿一致。
- **问答/插话：** T02 在 App 采用非推荐回答，真实实现使用对应分隔符；回答 eid `01a15c7b-e234-7be1-8476-c4af00e2820f`，回答和插话送达 eid 分别为 `01a15c7c-e5c1-707a-b052-42e4fb5e8519`、`01a15c7c-e664-74f9-9b2b-416f21cf5cd9`。CLI 实际输出后才记录 delivered。
- **暂停/恢复领取：** App 实际暂停 eid `01a15c62-c1f5-752d-acc4-e2a29e824034`，随后明确点击恢复，界面显示恢复确认及允许新领取。此项依据 GUI 操作记录，不将原记录中尚未更新的 `ui` 旧文案视作失败；`resume_ui` 记录了最终观察。
- **B1：** App 驳回未审阅提案，再审阅完整提案并批准；提交/tag `ecaa4055fda6fb129e9496bedc185651306b5b61`，批准 eid `01a15cb8-8998-760d-85f8-e6bd59c5a632`。`T-qa-b1-old` 保持 `needs_review=true`，旧 check 拒绝，按既有技能释放旧运行后改用新 Task ID `T-qa-b1-new`；原十任务仍 merged。
- **项目关注：** 已批准项目 region 显示四个关注中的前三项，“更多”可见第四项；回答第一项后后续项补位，坐标保持。旧 B0 不暗加结构，只有经批准的 B1 布局包含项目节点。默认关闭详情后只剩画布。

# main 事后检测与处置

故障注入仅发生在 QA origin/main。修改 canvas 的提交 `610a812b0ef5b99b9ae61f7f4e5df7557c40f942` 触发禁止；App 将原差异捕获为提案 `P-576f6217a28dec6bf91ab661423f062ad40d6addada6e12df0d0e17dd728fcac`，canonical eid 为 `git:f2dbff3826fc0910f8f73a7c82a35b818dee971e7192a35db18248de9c5d3d8d`。捕获后禁止仍保留，未自动批准。

App 随后明确确认撤销：批准 eid `01a15cbf-18fa-74bf-8118-62f887d1467f`，产生反向提交 `1ec5954f1293b41bbc9741685bf8639ec4e6cda1`，画布恢复逐字节一致，未改写原历史。另一个 QA 提交 `ca173f045bc21586e15e5aa024d24249708bfd39` 经 App 明确认领，批准 eid `01a15cc0-8f7d-72a6-819d-2192c1132833`；inspection 为 complete/passed，B0/B1 标签均保留。

# Hub 丢失与实际恢复

只备份并移走 QA 项目 Hub，再走 CLI 请求与 App 明确确认。确认 eid `01a15cc2-4523-7684-8f6c-68c0edfb0b74`，`recovery.completed` eid `01a15cc2-7149-7792-9ad0-ad82167352ee`；来源 epoch/seq 为 `14/712`，新 epoch 为 `4294967311`，恢复完成 seq 为 `4294968015`。恢复快照/main 为 `d4dc50292b48418cbf962e6f387fdb0cc6624dd3`。

旧运行 `R-01a15cb9-accb-7070-9a39-0d01efabf64c` 恢复后 released，新任务回到 ready、active_run 为空；不能继承旧租约权限。Evidence、Answer、Prompt、Baseline、MainResolution、MainResolutionApproval 六张表逐值保留，原十任务仍 merged；恢复未重新运行这些任务的验证。App 实际显示恢复完成和实时状态可用。

随后 B1new 重新领取为 `R-01a15cc3-441c-771c-9d24-fdd94c4df98c`，模块租约 epoch 为 `4294967312`。真实 Claude 完成新验证 `E-01a15cc4-f086-7aa8-be14-f6fbe253ec06`，main 接入 `0bd4ee727c55bc11e82dc5727a40b6819e06dd39` 已认证 merged；merge eid 为 `git:0b15a3d9b190269eeafbe46ced77645c4c5a25a5931cc13e61e00d384d54ab6c`，快照 `45d958877d90907401e4a470b5dcfa681902e90b`。该额外任务证明恢复后能够使用新权限继续工作，没有复用旧运行权限或旧证据。

# 原件摘要与复核

以下路径均相对于本机 QA 根目录，不是已发布下载地址。仅列必要标识和 SHA-256，不复制原始 harness 会话、账号或凭据。原件已另存本机产品仓库的 `.release/m1-live-originals-20261021/`：`qa-originals.tar` 保留原记录，`working-history.bundle` 与 `authoritative-origin.bundle` 均经 `git bundle verify` 验证；该目录仅供本机复核，不上传原始会话。归档 SHA-256 为 `6cc18e7aa445cd427db569830b5faed196f96ec45e452dd4670f4a0405230f9c`，两个 bundle 的摘要见同目录 `manifest.json`。摘要只能验证已有原件，不能恢复已丢失的内容。

| 原件相对路径 | SHA-256 |
| --- | --- |
| `records/ten-tasks-summary.json` | `06cd0a5232e411a5c5ea7b20777517fdfcefb75f645b4a7aa331b4e7ee6d92c8` |
| `records/binding.json` | `fef52260a59fe5a6369d677fdc6bb381d854d176a6fab6c50ec31d1603795255` |
| `records/T02/case-assertions.json` | `d51a511ffbc78fcc6c3e10211fe701b313febf6a677320f23e97ef914c51deb8` |
| `records/T03/selfreport-negative-proof.json` | `3dd0b26da6d0da3e9c4d8a9c26653ab95c296bf33b685897ce316dc87edbf296` |
| `records/T04/case-assertions.json` | `0b58717a53d2ca34ee0d54f2bee7ea1f9f8badad370ca0611c115103abc1da89` |
| `records/T05/case-assertions.json` | `38640cb14de5d10bc026a8afcd4fe0b5d764fb03740c04b2e66bbbe8e84339cd` |
| `records/T06/case-assertions.json` | `cc963cd2b50065bfb45fe6fc50a9239db79f7eaa7bda28a1fc749c55b9fc00ff` |
| `records/T07/case-assertions.json` | `ac7f27e9720c5189e2a016a0ed911aaba3ae40805c67594263e531c3efa154f6` |
| `records/T08/old-evidence-rejected.json` | `93a47c5890dc5f492cb29f0e19bc2598af7c2e85b0f8dbf70971f71d77465171` |
| `records/T09/g2-negative-proof.json` | `33b764561d91405b10eac493384cec913b0a387f6e8d9ed2f26ca53018d96d48` |
| `records/T10/conflict-proof.json` | `bf5e5e98175899523fb10feaa5f4b6330d56f0ad385d89264e917dbcfe3fd409` |
| `live-e721cc3-controls-complete.json` | `f5a9d33d2955f576baa1d38d75b8ef424c7c47ff7c33386da20d0911931a7f4d` |
| `records/B1old/b1-publish-proof.json` | `a3c5cffde3a0571a5516e623b8c7c04dfc80686d622e9b820da97ad4d63bee71` |
| `records/B1old/old-check-refused/latest.json` | `d01a0bcab06923041dfe6287a34f57c5d496a64c580c77a3b9af8a36f2204b83` |
| `records/B1new/project-attention-proof.json` | `c348feeaa3c34f57400c54dd6ddf334b3de1198b6b2e2f20db62af75b4e855ba` |
| `records/main-resolution/captured-canonical-events.json` | `1dee0235bf7e2f1482be53d2959916b2be0fe88e483f9bb3a0f143f88c6aae43` |
| `records/main-resolution/revert-proof.json` | `eadedb319b827952340fc3ad41ae9ae63a559922ef4ed1d3a9679a6f20041dc5` |
| `records/main-resolution/claim-proof.json` | `0958651d390a1f214b69a3c0458034d5f468d8d91ff2da36c0592e8287067809` |
| `records/recovery/proof.json` | `73246caf5a51f14ccf917920df126d8023e1fed084df53f0e129f61a499464d5` |
| `records/B1new/old-run-rejected/latest.json` | `12a9d2808c812b7e65ea8af2d0a3fd7ea97439c9ecfd35f005bb7b1741d5ee2c` |
| `records/B1new/certificate.json` | `de8e672984f89d0248df65f612b882768894927229ac420670e8e616ee55ffbf` |
