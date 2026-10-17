---
type: Plan
title: M1 P7 原生技能交付与真实双 harness 验收实施计划
description: 从单一技能来源交付六项原生技能与安装说明，在实际 App 就绪后以本机三个工作树、两种真人打开的 harness 会话及十个真实任务证明 M1 闭环。
status: draft
tags: [towermd, M1, P7, skills, acceptance, Claude-Code, Codex]
sources:
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: docs/superpowers/plans/2026-10-17-m1-task-evidence.md
  - resource: docs/superpowers/plans/2026-10-17-m1-human-intervention.md
  - resource: docs/superpowers/plans/2026-10-17-m1-baseline-approval.md
  - resource: docs/superpowers/plans/2026-10-17-m1-main-observer.md
  - resource: docs/superpowers/plans/2026-10-17-m1-app-foundation.md
  - resource: docs/superpowers/plans/2026-10-17-m1-unsigned-release.md
  - resource: docs/superpowers/plans/2026-10-17-shared-recovery-proof-contract.md
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: ../note.md/docs/okf-v0.2-format-constraints.md
  - resource: AGENTS.md
  - resource: tasks/lessons.md
  - id: codex-skills
    resource: https://learn.chatgpt.com/docs/build-skills
  - id: claude-skills
    resource: https://code.claude.com/docs/en/skills
  - id: claude-plugins
    resource: https://code.claude.com/docs/en/plugins-reference
---

# M1 P7 原生技能交付与真实双 harness 验收 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付 init、baseline、claim、ask、propose、finish 六项真实原生技能、可核验安装步骤，并在 App 就绪后积累双 harness 十任务及真人放行的实际证据。

**Architecture:** `skills/` 是一个可分发包根，`skills/skills/` 是唯一技能正文来源；Claude Code 按会话加载该插件，Codex 的项目技能目录链接到同一来源。技能只编排已冻结 CLI 契约，状态由 CLI→daemon→hub/统一 writer 写入；测试和证据收集脚本没有批准、调度或第二份完成判定。

**Tech Stack:** 现有 Rust/CLI/daemon、Git CLI、Node 标准库 `node:test`/`child_process`/`fs`、Python 3 标准库 `unittest`（仅隔离验收项目）、用户已有 Claude Code 与 Codex；本批不新增依赖、账号或无头模型调用。

---

## 1. 文档状态、前置条件与成功定义

本文件仅为待独立审查的实施计划；本次只新增本文件，不实施、不安装、不请求用户现在打开 harness、不修改总表/任务表/源码/其他计划，也不提交。所有实施复选框均未执行。

首轮调研时主目录 HEAD 为 `aa57256896d248961cc9898ffe3cebf3c21bd5b6`，当时P1批3.1正在修review问题、`skills/` 尚不存在；这些是历史调研事实，不是当前实施状态。此次修订时Root main为 `b8e799f029cb48262348bfbfb72d5a28bd861f5e`；批3.1已双审并以401项测试绿合入 `81266e4`、`c9eb19d`，批3.2工作树以 `c76c3ca` 为基线、401项基线测试绿，正在TDD。后续P2/P3/P4/P6/P8仍不能因计划文字存在就声称其CLI、App或脚本已交付。首轮调研的CLI baseline只有scan，FR-12示例只有四项trailer；执行Task1重新核真实源码与help，确认P2已同步第五项 `Towermd-Epoch`，不能把历史调研当当前契约，也不接受四项旧示例作为最终契约。

分两段开工：

1. **技能编写/自动机制段：** P1 批 2–5 的真实 producer、P2/P3/P4 对应 CLI 已合入且回归通过，先记录各 SHA、help 和 schema。若只到部分前置，可写相应技能的可加载骨架与行为红测，不能把缺接口绕成成功。
2. **真人验收段：** P6 基础及 P2–P5 的 App 人工闭环已集成，可从实际 `.app` 打开项目、审阅 B0、回答、G1/G4 高风险评审、确认恢复并读证据。基础只读画布不满足此前置。此时才提供实际 App 路径、真实项目路径、任务卡和确切点击目标，请用户一次安排具体验收操作。

完成须同时成立：六项原生技能均可被各 harness 发现和加载；所有技能中引用的 CLI 与同一二进制 `--help` 一致；自动机制回归通过；三工作树上的十个真实任务由两种真实会话完成且逐项有 main 证据；真人 B0/回答/高风险评审/恢复操作可追溯；README/安装说明/双语未发布日志与结果一致；本批独立验证的改动精确提交并登记 SHA。未真人、未提交、未冻结、未发布分别记载，不能互相替代。

不交付 `towermd-seed`（M2），不引入 M3 无头运行器/总控/技能锁定，不自动改全局 Git 或 harness 配置，不把普通问答当批准，不自动移动用户 main/index、迁移运行绑定或禁用其他技能。没有 Intel 实机时只记实际构建/静态检查范围。

P5 首次 inspection frontier 已由用户于2026-10-17确认“整笔启用提交作为导入起点”，主规格FR-103与P5计划已同步：完整权威main历史中首次引入当前project_id配置的整笔提交及祖先为导入上下文，不批准B0；之后全部可达提交均检查，配置删除重加不重置锚点。Task1仍须接收P5实际实现SHA及验收证据，不能把已裁决语义当作已实现。锚点或完整历史无法核定时保持inspection_incomplete并阻止合并认证，不能自行选B0/daemon首见HEAD作为豁免边界。

## 2. 原生安装依据与单源布局

2026-10-17 已只读核实本机 `codex --version` 为 `codex-cli 0.163.0`、`claude --version` 为 `2.1.300 (Claude Code)`。`codex --help` 有 `-C/--cd`；`claude --help` 有会话级 `--plugin-dir`；`claude plugin validate --help` 有 `<path>`、`--strict`。本机没有从 CLI help 得到 Codex 任意目录插件安装承诺，因此不编造 `codex skills install` 或 `codex plugin install`。

Codex 官方说明 repo 技能从工作目录至仓库根的 `.agents/skills` 发现，支持技能目录 symlink，显式调用可用 `$技能名` 或 `/skills`；`SKILL.md` 必须含 name 和 description。[官方依据](https://learn.chatgpt.com/docs/build-skills)

Claude Code 官方说明插件技能位于包根 `skills/<name>/SKILL.md`，manifest 位于 `.claude-plugin/plugin.json`；插件技能调用有插件命名空间。使用会话参数加载可避免永久安装设置。[技能依据](https://code.claude.com/docs/en/skills)、[manifest 依据](https://code.claude.com/docs/en/plugins-reference)

上述路径是当前已查到的契约，不代替目标版本的加载实测。实施 Task 1 再保存实际 help/官方链接及日期；版本变更时重新核验，不沿用旧路径猜测。Codex 核实遵循 OpenAI Docs：先本机环境，浏览仅官方 OpenAI 域；本文不读取配置、认证文件或秘密。

### 文件地图

| 文件 | 职责与归属 |
| --- | --- |
| `skills/.claude-plugin/plugin.json` | Claude 原生包身份；name=`towermd`，version 与发布版本一致，无 hooks/MCP/自动执行 |
| `skills/skills/towermd-init/SKILL.md` | 初始化及安装提示，明确 FR-98 离线初始化例外 |
| `skills/skills/towermd-baseline/SKILL.md` | S1–S5 等待、S6 单 schema 综合、提交草稿、转 App S7/S8 |
| `skills/skills/towermd-claim/SKILL.md` | 自建分支/工作树、claim 绑定、检查点 status/续租 |
| `skills/skills/towermd-ask/SKILL.md` | 有界问答、G1/G4 请求、deferred/过期处理 |
| `skills/skills/towermd-propose/SKILL.md` | 理由/影响/备选/建议，提交提案，不直接写慢层 |
| `skills/skills/towermd-finish/SKILL.md` | 守卫、异步验证、五 trailer、合并认证及其他技能顺序 |
| `skills/references/session-contract.md` | 六项共用约束唯一正文：actor、绑定、续租、状态、退出与恢复 |
| `skills/references/cli-contracts.json` | CLI 示例与 help 校验的单一机器清单，不包含第二份门禁/schema |
| `skills/INSTALL.md` | 明确解包、两 harness 会话入口、CLI 定位、手动安装/冲突/卸载说明 |
| `skills/scripts/check-contracts.mjs`、`skills/scripts/check-contracts.test.mjs` | name/description/链接/包路径/全部引用命令和真实 help 校验 |
| `scripts/acceptance/prepare.mjs`、`scripts/acceptance/prepare.test.mjs` | 建立全新隔离验收项目和恰好三个工作树，禁止覆盖既有目录 |
| `scripts/acceptance/task-cards.json` | 下文十任务唯一描述、调度与验收条件，不保存产品状态 |
| `scripts/acceptance/collect.mjs`、`scripts/acceptance/collect.test.mjs` | 只读归并真实harness动作已保存的raw输出、对象/事件、Git及测试日志，输出证据索引；不主动调用项目CLI/API |
| `docs/verification/2026-10-17-native-skills.md` | 实施后的自动/真人矩阵、来源 SHA、十任务、限制与恢复记录 |
| `README.md`、`CHANGELOG.md`、`CHANGELOG.zh-CN.md`、`tasks/todo.md` | 实施阶段的用户入口、双语变化和精确提交/回顾，不在本次写作修改 |

不新建第二套 `claude/skills`、`codex/skills` 正文，不分发绝对 symlink，不把 core schema 复制进包。S6 运行时经 `baseline schema` 读取 P4 唯一 schema；包内只引用取得方法。共用文档使用 `../../references/session-contract.md`（从真实 SKILL 所在目录解析），单项正文只写该技能独有步骤。Codex 的 `agents/openai.yaml` 是可选展示文件，本批不增加无必要的依赖/展示配置。

`skills/` 全部在一个 tag 下进入 P8 已接收的 `git archive <tag> skills` 输入。包结构解开是 `towermd-skills-<version>/skills/`，这个内层 `skills/` 才是 `$TOWERMD_SKILLS_ROOT`；其下仍有原生 `skills/` 子目录。P8 负责归档和校验和，P7 不另造发布器。

## 3. 共享技能行为与 CLI 清单

以下均是 **前置任务待实现的命令签名**，不是当前已可运行入口。Task 1 只有在同 SHA 的真实 `--help` 验证通过后，才将它们放进可发行技能示例；不调用新造的 approve、baseline verify、seed、--json、daemon start 或 CLI export。

| 技能 | 需核验的 CLI 签名 | 拒绝/停车行为 |
| --- | --- | --- |
| init | `init [path] --docs-dir <relative-dir>` | docs_dir 由人确定或既有配置继承，不猜目录；重跑项目 ID 不变，不自动提交初始化文件 |
| baseline | `baseline scan`；`baseline scan --wait <scan-id>`；`baseline schema`；`baseline submit <json-file>` | 等待每次 ≤90s；失败/缺 schema 停止；S6 不写慢层；S7/S8 等人 App 审阅 |
| claim | `status`；`task show <task>`；`task claim <task> --harness <name> --harness-version <version>` | 主模块或 touches 租约被占拒绝；不能换目录后继续旧 run |
| ask | `ask "问题" [--option 文本] [--recommended 索引] [--context 文本]`；`ask --wait <hold>`；`review request --gate G1 --content <file>`；`review request --gate G4`；`review request --wait <hold>` | ≤90s 一次、累计10min deferred 就结束回合；不循环绕预算；旧 epoch 回答仅历史 |
| propose | `propose <json-file>` | 提案理由/影响/备选/建议完整；只提交草稿，任何批准等待 App |
| finish | `check`；`finish [--report <file>]`；`finish --wait <evidence>`；`trailers <task>` | dirty/越界/慢层/no_tests/缺人工事实阻塞；只有权威 main 认证才 merged |

共用 `session-contract.md` 完整写入以下规则，六份 SKILL 都要求先读取：

- 项目状态、Task/Run/Evidence、租约、草稿、快照、用户管理文档与 `.towermd/` 的后续写入全部经 CLI→daemon；FR-98 `init` 离线配置生成是已批准的专门例外，不由技能自行写文件。普通任务代码只在任务卡授权路径内编辑。
- 执行 `harness --version`，以参数自报名称/版本；`data.self_reported` 是声明，不是可信身份。CLI actor 固定 `process:towermd-cli`，不提供 human/actor/approve 参数；费用没有报告时为 `null`。
- 先自建 `t/<task-id>` 分支与目标工作树，再在该树 claim。绑定是本机+canonical worktree+分支；`--run` 不能绕绑定校验；不自动 checkout/reset 用户 main、不 stash、不迁移运行绑定。
- 每个检查点（读完任务卡、计划完成、每个测试红绿、提交前后、长操作前后、合并前）调用 status，先消费输出最前的消息，再做下一步。长测试须分段检查，不能用连续超过15min且无续租的工具调用；finish 验证期间由 daemon 续租。收到暂停停止写入并报告，其他模块可继续。
- ask/review wait 的90s/10min由 daemon 计时；deferred 后结束本回合，附 hold/run/task/下一次 status 指引。不能把10min当批准，不能后台无限轮询。15min无活动释放后须重新 claim；迟到回答不复活旧 epoch。
- 高风险计划 G1 与提交 tree 的 G4 均需实际 App 评审；改计划/tree/baseline/fences 后旧批准失效。普通问答、CLI转述、Agent自己的review均不能满足。
- check/finish 守卫在验证之前，拒绝未提交/未跟踪、旧 main、越界、慢层。no_tests 是硬阻塞：先经 S7 或提案让 App 批准 Module.verify，再在授权路径补真实测试，按同一 G3 重跑；禁止 `true`/空测试/从任务分支发现命令充当权威配置。
- finish 在后台分离工作树运行权威模块全部 verify，与 S5 同验证器；没有 Agent“done”状态入口。初次调用只取得 Evidence/job ID；重复 `finish --wait <id>` 每次≤90s，queued/running/failed/no_tests 均不能合并。
- 60min deadline 固定为首次进入 verifying 的时刻；通过、轮询、amend、重启或纯快照变基都不重置。过期先读状态、重新领取和验证，不沿用旧 fences/Evidence。
- 成功后读真实 trailers，不自行猜值：Task、Run、Module、Evidence、Epoch 五项齐全；Epoch 属主 Module，完整 touches fences 绑定 Evidence/Run。tree 改变要重新验证；只改消息/压缩但最终 tree相同可保留证据。main 观察复核完成前只能写 verifying/passed，不能计 merged。
- 其他 superpowers 的 PR、合并、推送收尾步骤必须排在 towermd finish 通过之后；不禁用用户技能，不跳硬门禁。只按任务卡既定集成方式做快进/推送，不 force push、不改写 main。

## 4. 实施 Task 与红绿顺序

每个编号动作以2–5分钟为粒度；涉及真实会话/等待时按一个检查点一次记录，不把整个十任务或人工会话算作一次动作。以下 Node 脚本和测试文件为本计划指定 **待实现接口**；本文没有声称它们现已存在。缺前置停在相应 Task，独立文档/静态工作可继续。

### Task 1：冻结真实接口，建立可加载包和自动契约检查

**Files:** 创建 manifest、六个可加载 SKILL 骨架、共用 references、`skills/scripts/check-contracts{,.test}.mjs`；实施时记录 verification/todo。

- [ ] **1.1 只读核前置（2–5分钟）。** 执行下面命令，记录 P1/P2/P3/P4/P5/P6 各实际提交 SHA、测试文件与 API 映射；不能将计划作者的示例签名当源码事实。

```sh
git status --short
git rev-parse HEAD
git log -12 --oneline
rg --files crates/towermd/src crates/towermd-core/schema src-tauri/src src/lib/daemon
rg -n 'enum Command|enum TaskArgs|enum BaselineArgs|Finish|Ask|Propose|Review' crates/towermd/src/main.rs
```

- [ ] **1.2 固定执行输入（2–5分钟）。** 从 P6 实际包中复制完整 CLI 路径到本次会话的 `TOWERMD_CLI`，从解包产物复制 `TOWERMD_SKILLS_ROOT`；执行 `test -x "$TOWERMD_CLI"`、`"$TOWERMD_CLI" --version`、`shasum -a 256 "$TOWERMD_CLI"`。记录源码 SHA、目标架构、CLI hash、包内版本及 P6 构建收据，先用开发候选 SHA，P8 冻结后重核。开发 CLI 不冒充随包 CLI，不强制全局 PATH 安装。

- [ ] **1.3 先可加载再行为红（2–5分钟）。** manifest 内容为 `{"name":"towermd","version":"0.1.0","description":"Tower.md local-first task workflow skills"}`；每份 SKILL 初始是下列骨架，name/description 按六项作用填写完整。stub 明确停止，不能危险地继续。

```markdown
---
name: towermd-claim
description: 在已有 Tower.md 项目中领取任务，先建立任务分支与工作树，再绑定运行并在检查点读取插话。
---

先读取 [共享会话契约](../../references/session-contract.md)。
此技能尚未接入完整行为；停止领取，不报告交付成功。
```

六个 description 分别明确“初始化现有仓库”“S6综合/等待App B0”“领取已有任务”“需要人的回答或高风险评审”“慢层或越界需提案”“合并前验证与trailer”，避免 seed/自动批准误触发。

- [ ] **1.4 写 checker 红测（2–5分钟）。** 定义 `checkPackage(root) -> diagnostics[]` 和 `checkCli(root, binary, run=spawnSync) -> diagnostics[]`，CLI 入口 `node skills/scripts/check-contracts.mjs --root skills --cli <absolute-binary>`；主入口非空诊断退出1。先导出返回空数组的可导入 stub，然后加入以下实际测试，确认因断言而红，不只停在 import 缺失：

```js
test('six_skills_require_bound_workflow', () => {
  const errors = checkPackage(fixtureWithLoadableStubs());
  assert(errors.some(x => x.code === 'missing_workflow'));
});
test('invented_cli_is_rejected', () => {
  const root = fixtureWithCliLine('"$TOWERMD_CLI" baseline verify');
  assert(checkPackage(root).some(x => x.code === 'unlisted_cli'));
});
```

`fixtureWithLoadableStubs()` 在临时目录写完整六骨架/manifest/reference；`fixtureWithCliLine(line)` 在同夹具增加 shell 围栏行。测试辅助函数在同一 test 文件定义，创建位置来自 `mkdtempSync(join(tmpdir(),'towermd-skills-test-'))`，不能读用户项目作为夹具。

- [ ] **1.5 最小结构检查（2–5分钟）。** 用 Node 标准库读取目录；只允许六个精确 name，各有非空 name/description、正文与共用链接。包内链接 realpath 必须在包内、目标存在；拒绝断链、路径逃逸、绝对链接冒充资源、重复技能正文、可执行 seed、hooks/MCP、错误manifest路径。普通官方 HTTPS 引用不当成本地链接；只读 SKILL 的简单 frontmatter 字符串，不宣称实现完整 YAML 解析器。

- [ ] **1.6 单一 CLI 清单（2–5分钟）。** `cli-contracts.json` 每项固定 `{id,help_argv,required_tokens,example_pattern}`；覆盖第3节全部命令以及安装说明引用的 `--version`/`--help`。所有可执行 CLI 示例带 `<!-- towermd-cli:<id> -->`；checker 扫描所有 shell 围栏的 `"$TOWERMD_CLI"` 行，要求有对应ID且整行符合锚定 example_pattern。未经登记行失败，不能只验证 manifest 而漏掉正文发明命令。help_argv 用 argv 调 `spawnSync`，如 `["finish","--help"]`；must tokens 含 `--wait`、`--report`，并断言不存在 approve/actor/human/skip-gate。执行 timeout 5s，非零/超时为失败，不执行示例业务命令。

- [ ] **1.7 真 help 红绿与提交（2–5分钟）。** 先对缺 P2–P4 的旧binary单独调用`checkCli`，预期`missing_cli_contract`，不能skip成通过；再对前置已合入同SHA binary核同一CLI清单，预期CLI契约诊断为空。运行 `node --test skills/scripts/check-contracts.test.mjs`，1.4的负例夹具必须因checker正确返回`missing_workflow`/`unlisted_cli`而测试通过；本Task完整unit suite须全绿。真实六骨架包此时仍由`checkPackage`返回`missing_workflow`，整个发行readiness checker仍非零，这是“可加载、行为未完成”的已知产品状态，不是允许提交失败unit suite。Tasks2–4再逐项增加真实技能positive workflow红测并实现转绿；全部完成前不可发行。只有本Task checker/骨架的独立测试全绿才可精确提交，提交守卫见第8节。

### Task 2：init 与 baseline，真正接入 S6–S8

**Files:** 修改 init/baseline SKILL、session-contract、cli-contracts；增加相应 checker 测试。

- [ ] **2.1 写行为红测（2–5分钟）。** 新测试要求 init 含离线配置例外/不自动提交/明确CLI定位；baseline含 scan/wait/schema/submit、读取单 schema、只读综合、App S7/S8、B0 tag。stub 不满足而红；真实 CLI 的离线 init/冷启动/基线测试沿 P1/P4 已合入的实际路径运行，不造 fixture 输出。
- [ ] **2.2 实现 init 正文（2–5分钟）。** 读取现有配置和 task/status 输出；新项目先由 App 提供用户已选 docs_dir，CLI非交互命令为 `"$TOWERMD_CLI" init . --docs-dir "$TOWERMD_DOCS_DIR"`。目录变量是 App/用户选择的具体相对值，无值停止，不猜默认值。输出初始化未提交文件、后续 baseline 入口和手动技能安装说明，不自行写 `.towermd/AGENTS.md` 或安装全局配置。
- [ ] **2.3 实现 baseline 读取正文（2–5分钟）。** scan返回ID→按 `baseline scan --wait "$SCAN_ID"` 有界取S1–S5结果；schema用 `baseline schema` 原样写到工作树外本次证据目录 `baseline/schema.json`；扫描原文另存 `baseline/scan.json`。只读相关README/模块/历史/测试证据，用用户当前会话最强可用模型综合，不选择新账号、不调用 `claude -p`/`codex exec`。
- [ ] **2.4 实现 S6 提交正文（2–5分钟）。** 根据同 schema 生成 `baseline/submit.json`，包括真实 project/scan/source revision、模块/意图/接口/验收、风险/待问问题；不在技能重列字段验证逻辑。经 `baseline submit <file>` 取得 draft ID/revision，schema拒绝则修 JSON；保存原输入及hash。S6不直接改权威文档或画布。
- [ ] **2.5 明确人的下一步（2–5分钟）。** 显示实际 draft ID及项目，不用捏造深链；App S7逐项审阅模块合并/拆分/名称/坐标/接口/验证命令/已有Markdown引用，再S8显示hash、目标B0、差异批准。daemon出版后检查真实 `towermd/B0`/Decision/manifest/canvas；main被检出时保持pending，只提供用户可审阅的切离指引，不自动执行。
- [ ] **2.6 绿与提交（2–5分钟）。** `node --test skills/scripts/check-contracts.test.mjs` 已加入的unit/negative及init/baseline positive组全绿，`checkCli`对真实help诊断为空；其余四技能仍stub，实际包readiness保持not_ready，不写完整checker发行通过。P4自动产品链仍单列，标记“真人S7/S8未开始”。精确提交两技能及本Task关联checker/reference，登记SHA。

### Task 3：claim 与 ask，续租、投递与真人评审

**Files:** 修改 claim/ask SKILL、session-contract、cli-contracts；checker增加对应行为断言。

- [ ] **3.1 写行为红测（2–5分钟）。** 必须出现先分支/工作树后claim、status检查点、self_reported、cost null、绑定不可迁移、90s/10min/deferred结束回合、G1/G4真实App、迟答旧epoch不复活；stub缺项断言失败。反例包含在main claim、自动改用户main、收到deferred继续实施。
- [ ] **3.2 实现 claim 前半（2–5分钟）。** status→task show→核路径/依赖/决策；在新目标位置自行 `git worktree add -b "t/$TASK_ID" "$TASK_TREE" "$AUTHORITY_REV"`，其中authority来自Task1已核 daemon权威读取。目标已是自建detached任务树则 `git switch -c "t/$TASK_ID" "$AUTHORITY_REV"`，不在用户main执行。路径冲突/已有分支不覆盖，先读其绑定并报告。进入该树执行 claim与harness版本参数，保存task/run/fences。
- [ ] **3.3 实现 claim 后半（2–5分钟）。** status检查点完整写入共用契约；版本输出只记声明，不能写human actor；task card约束比会话偏好优先。claim失败不写代码；新tree不能套旧run。运行恢复时先status，再核run/fences，失效重新claim或提案，不提供迁移绑定捷径。
- [ ] **3.4 实现 ask 正文（2–5分钟）。** 用具体问题/2–3选项/推荐/上下文发送ask；优先CLI返回的既有权威Decision引用。pending可续等，deferred输出恢复卡并结束回合。App本次回答和CLI转述不存为新的Decision/Rule；下一CLI输出的消息先阅读再行动。恢复卡固定 `task/run/hold/worktree/branch/last_commit/blocked_reason/next=status` 七项。
- [ ] **3.5 实现高风险正文（2–5分钟）。** 在工作树外写计划文件，请求G1；没有精确App事实就停车。实现并提交后请求G4绑定当前tree；没有App评审就停车。G1计划或G4 tree变化重新请求，ask回答“可以”不能解锁；review wait同有界逻辑。
- [ ] **3.6 绿与提交（2–5分钟）。** 已加入的checker unit/negative及四技能positive测试全绿，`checkCli`对真实help诊断为空；propose/finish仍stub，实际包readiness仍not_ready。跑P3已经提交的真实CLI delivery/ask/review产品测试并保存命令、非零/通过数。人工事实夹具只证明机制，不能写真人已批准。精确提交两技能及关联测试，登记SHA。

### Task 4：propose 与 finish，合并之前的唯一证据路径

**Files:** 修改 propose/finish SKILL、session-contract、cli-contracts；仅消费P2/P4接口，不改业务核心。

- [ ] **4.1 写行为红测（2–5分钟）。** 检测慢层/越界只提案、finish先守卫后验证、dirty/no_tests硬阻、异步ID/有界wait、固定deadline、五trailer/主模块epoch、合并前其他技能顺序、tree改动重验证、main认证前不计done。stub以及“Agent报告done即完成”反例必须红。
- [ ] **4.2 实现 propose 正文（2–5分钟）。** 先status、保存只读diff/原main revision；按P4实际 `ProposalInput` 创建树外JSON，source为已提交分支revision/base或真实draft id/revision，理由/影响intents/modules/interfaces/tasks/paths、alternatives及recommendation全填。调用 `propose <file>`，记录proposal/draft/revision；App批准前不改权威慢层，不把提案当原分支可合并许可证。
- [ ] **4.3 实现 finish 守卫正文（2–5分钟）。** status→检查分支/run/权威main→提交任务代码→check。dirty列文件，不自动清理；越界/慢层保留差异转提案；no_tests先App确认verify并补真实测试。旧main仅在自建任务树按任务卡变基，冲突停下解决，不自动处理用户索引。
- [ ] **4.4 实现异步正文（2–5分钟）。** `finish`返回Evidence ID，不宣称通过；`finish --wait <id>`每次最多90s，未完成返回checkpoint；再次调用前status。failed保留完整日志和原Evidence，修复后新输入再finish；无测试停车。首次 entered_verifying_at+60min写恢复卡，不通过轮询延后。
- [ ] **4.5 实现 trailer 与合并正文（2–5分钟）。** `trailers <task>`输出写工作树外 `trailer.txt`，先核五项值与本Run；读取当前提交消息到树外文件、追加真实trailer，用 `git commit --amend -F <file>`，先记录tree并核amend后相同。禁止 `--allow-empty` 冒充代码任务。其他技能PR/merge步骤此后才可执行；真实main快进/普通push后由执行Agent调用status并保存raw输出。只有P2/共享唯一Git-Evidence证明器认证实际main提交、产生canonical `merge.done`，且投影Task为`merged`、Run为`ended`才结案；不等待或新增`task.merged` producer。
- [ ] **4.6 压缩条件（2–5分钟）。** 任务有多个检查点时，可在自建任务树压缩，先后记录 `HEAD^{tree}`；最终tree与已通过Evidence完全相同才保留原证据并重新添加trailer，否则重跑finish。main只允许state/canvas差异的例外交P2唯一证明器核，不在技能自己比较/放宽。
- [ ] **4.7 绿与提交（2–5分钟）。** 六技能不再stub，运行 checker及P2/P4实际既有产品测试；确认SKILL均没有直接写`.towermd`/慢层、批准命令或自动global配置。精确提交两技能/共用约束/测试，登记 SHA。

### Task 5：自动机制集成与只读证据收集

**Files:** 创建 collect脚本和测试、verification文档；只消费前置真实产品链，发现前置缺陷交对应P批，不复制业务实现。

- [ ] **5.1 定义可导入收集器（2–5分钟）。** 先实现可导入stub，再写行为红测。接口固定 `collect({inputs,worktree,task,out,expectedSourceSha}) -> Promise<Record>`；脚本入口 `node scripts/acceptance/collect.mjs --inputs <captured-input-manifest.json> --worktree <path> --task <id> --out <external-dir> --source-sha <sha>`。inputs只引用真实harness在执行本身checkpoint、ask/review、finish/wait、trailers、合并确认时已保存的raw stdout/stderr、退出码、事件/对象来源及测试日志；另允许Git只读命令。collect不执行任何towermd命令、HTTP请求、测试或续租/投递动作；仅写树外审计副本。严格核已捕获的CLI版本/hash、Task1收据及输入文件hash，缺身份/对象/日志返回incomplete，不能默认PASS。
- [ ] **5.2 收集器红测（2–5分钟）。** 夹具返回Task merged却无Evidence、canonical `merge.done`或Run ended→incomplete；错误tree、缺Epoch、缺主模块fence、未知费用被填0、只有fixture来源、伪App human记录均拒绝真人PASS。注入进程/网络spy，除固定Git只读argv外任何调用均使测试失败，尤其禁止status/task show/daemon status/objects GET；active/deferred/迟答采集前后目标Run的TTL、inbox与delivered事件必须完全不变。夹具仅验证收集器判别，不算真实十任务；生产binary的help核验和有副作用的真实Agent动作另有明确执行记录。
- [ ] **5.3 来源先捕获，缺源不猜接口（2–5分钟）。** 执行Agent在其正常动作时保存真实`finish.completed`事件、finish/wait输出中的Evidence与日志定位、canonical proof、`merge.done`及投影；Task1核实际已实现的producer/wire后，才能解码这些已捕获原文。项目CLI会续租且P3 prelude可能flush后ack `prompt.delivered`，对象GET也可能续租，不能称“查询即只读”。如必要现场查询，须作为单独审计操作者动作在确认没有目标active或旧收件绑定的审计目录执行，先证明不会消费目标Run消息/续TTL并保存前后差异；不加入collect，也不新增只读绕过flag/API。现有GET若只读Catalog而不提供runtime Evidence，不虚构Evidence端点/文件/decoder；无已捕获的权威Evidence来源就incomplete。日志只按真实输出给出的本机cache/Arca定位读取，不执行对象文本、不遍历秘密目录。
- [ ] **5.4 main来源证据（2–5分钟）。** collect只读保存 `git rev-parse HEAD HEAD^{tree}`、`git worktree list --porcelain`、当前branch、`git log -1 --format=%B <actual-main-commit>`，并归并已经捕获的status/task card/Evidence原文与hash。只有来源已证实某对象快照到精确Git path/ref，才读 `git show <actual-main-commit>:<actual-evidence-path>`；Evidence未来不一定有main文件路径，不能将有Git路径设为一律通过条件。使用真实authority ref、唯一证明器的canonical proof/`merge.done`及Task merged/Run ended投影，不默认本地main就是远端权威main；没有快照路径记录“未快照”并引用原`finish.completed`/wait原文，缺这些来源则incomplete。
- [ ] **5.5 自动正负机制矩阵（每行一次2–5分钟动作）。** 在前置已实现的真实二进制→daemon→hub/Appgate集成测试中逐行运行：

| 自动用例 | 必须观察的事实 |
| --- | --- |
| 实际CLI finish→Appgate事实→trailers→push→main observer | 完整Evidence/fences、唯一证明器canonical proof与`merge.done`，投影Task merged/Run ended；程序Appgate事实不记真人 |
| Agent只输出done | 无Evidence/merged/进度变化 |
| dirty/untracked、越界、L0–L2改动 | 验证器尚未启动即check/finish阻；记录Hold/gate证据 |
| 跳过finish绕行提交 | P5确定性禁止中断；未经批准慢层不被认作有效基线 |
| no_tests及high risk缺G1/G4 | 硬阻；仅App批准verify/精确review可解 |
| ask pending/deferred、epoch过期迟答 | 有界计时；结束回合；迟答historical，不续活 |
| status和非status命令投递 | prelude先输出，flush后delivered；重复不重复处理 |
| 写trailer/压缩checkpoint | tree相同仍认证，wrong tree拒绝；主模块Epoch正确 |
| wait/重启/amend/纯快照变基 | deadline仍首次进入验证+60min，不被刷新 |
| CLI注入approve/actor/human/skip | clap或daemon拒绝，审计无批准/human事件 |
| 删除派生cache/索引、恢复hub | 真实producer/outbox/快照/trailer可重建；不伪造当前有效旧批准 |

时间边界复用已有注入时钟，不等待实际60min；这与真实用户会话的10min deferred检查分开登记。测试名必须用 `rg --files`/源码核到真实名称再运行；0 tests不算通过。前置测试失败先修所属任务并精确提交，再刷新SHA，不在技能写例外。

- [ ] **5.6 绿与提交（2–5分钟）。** `node --test scripts/acceptance/collect.test.mjs`；真实checker再次通过，实际产品测试保存日志。精确提交收集器与文档的自动证据段，仍写“真人未开始”。

### Task 6：准备隔离项目、十张任务卡及三工作树调度

**Files:** 创建 prepare脚本/tests/task-cards；验收资产输出到新临时目录，产品仓库不被当测试靶。

- [ ] **6.1 准备器红测（2–5分钟）。** 接口固定 `prepare({root,sourceSha,cli}) -> {project,worktrees,origin,evidence,cards}`；入口 `node scripts/acceptance/prepare.mjs --root <new-dir> --source-sha <sha> --cli <path>`。已有root、symlink root、重复执行、非空目标worktree、Git缺失、python3缺失、工具失败必须停止；不能覆盖/清理用户目录。先可导入stub，测试“返回恰好三个工作树并都不检出main”行为红。
- [ ] **6.2 实现项目种子（2–5分钟）。** 新root下生成 `origin.git`（本地bare远端）、`w1`（普通clone）、`w2/w3`（linked worktree）、`evidence/`（树外）及准备收据。`w1` 初始化main后提交README、三模块、真实unittest；推送origin main，再把w1显式切detached，建w2/w3 detached。只对脚本刚创建的夹具执行这些Git动作，不checkout/reset用户仓库；用户全局gitconfig不动，commit身份经 `git -c user.name=Tower.md-Acceptance -c user.email=acceptance@localhost commit ...` 单次传入。
- [ ] **6.3 最小可跑模块（2–5分钟）。** 三目录 `ledger/`、`reports/`、`filters/` 均含 `__init__.py`、实现及 `test_*.py`。种子功能分别为整数加总 `sum_minor([1,2]) == 3`、纯文本标题 `heading('x') == '# x'`、严格相等谓词 `equal(1,1) is True`；各测试同时有非平凡边界（空集合/空标题/异值）。初始全量命令 `python3 -m unittest discover -s . -p 'test_*.py'` 实跑≥6个测试且PASS。后续十任务不得预先写出实现或用占位测试代替。
- [ ] **6.4 安装资产与dirty兼容（2–5分钟）。** 准备阶段即将 `/.agents/skills/`、`/.claude/skills/`、`__pycache__/`、`*.pyc` 写入验收仓库已提交`.gitignore`；只忽略本机技能链接和Python派生物，不忽略代码/测试/管理文档。不得为了验收改变finish守卫。普通用户项目的安装说明给出可审阅的项目ignore或手动本地`.git/info/exclude`方法，由用户选择执行；安装步骤不自动修改它们、不写全局excludes、不tracked绝对symlink。三树装完后 `git status --porcelain=v2 --untracked-files=all` 必须为空。
- [ ] **6.5 App初始化与S6准备（2–5分钟）。** 在w1通过真实init选 `tower-docs`；初始化文件由夹具操作者检查后显式提交/推main，w1回detached，再scan。用户真实会话按baseline技能综合；S6的ledger Module从B0即声明risk high（金钱舍入和分摊），S7真人确认模块路径/verify/风险/引用，S8批准B0，daemon出版。ledger的T01/T04/T07均实际走G1/G4，不能只测试其中一个而豁免其余。三个Module的verify均为全量unittest命令（标明共享命令），所以每次所属模块finish都覆盖全项目，不假称独立小测试即可证明main。此步骤真人部分依赖Task7，不在自动准备器伪造B0。
- [ ] **6.6 十任务注册（2–5分钟一次一张）。** 从以下唯一 `task-cards.json` 读取id/title/module/allowed_paths/depends_on/acceptance，再按真实help生成 `task add` argv；无新命令、无手写`.towermd/tasks`。涉及跨模块T10列完整touches，不能只锁主模块。Task卡路径全部在代码/测试模块内；创建Task是快层，慢层B0须先由App批准。

| 任务 | 主模块 / 树 / harness | 真实变更与可检查验收 | 依赖与专项证据 |
| --- | --- | --- | --- |
| T01 | ledger / w1 / Claude Code | `ledger/money.py`与`ledger/test_money.py`实现 `parse_minor(text)`：'12.34'→1234，'-0.01'→-1；拒绝三位小数、非数字；精确十进制不走float | 无；App G1/G4，先留下红测/实现/finish基本链 |
| T02 | reports / w2 / Codex | `reports/money.py`及测试实现 `format_minor(value)`：1234→'12.34'，-1→'-0.01'，0→'0.00'；输入非整数拒绝 | 无；两会话真实并行、主模块不同 |
| T03 | filters / w3 / Claude Code | `filters/status.py`及测试实现 `normalize_status(text)`：trim+lower接受open/paid/void，空串/unknown拒绝 | 无；三工作树均出现真实Run |
| T04 | ledger / w1 / Codex | `ledger/totals.py`及测试实现 `sum_amounts(texts)`复用T01；['1.20','-0.20']→100，空→0，非法条目报错 | T01；App G1/G4，同树另一harness实做，不迁移旧Run |
| T05 | reports / w2 / Claude Code | `reports/groups.py`及测试实现按currency汇总整分金额，键排序、负数正确、空输入空映射 | T02；多检查点提交后保持最终tree压缩，trailer前后仍merged |
| T06 | filters / w3 / Codex | `filters/dates.py`及测试实现ISO日期含首尾区间，闰日正确，逆序边界拒绝 | T03；检查点前App插话，下一status最前送达 |
| T07 | ledger / w1 / Claude Code | `ledger/allocation.py`及测试实现分摊整数金额：10/3→[4,3,3]，-10/3→[-4,-3,-3]，零份数拒绝，总和守恒 | T04；B1影响任务复核，实际App G1计划及G4 tree评审 |
| T08 | reports / w2 / Codex | `reports/markdown.py`及测试实现表格单元格转义pipe、换行转空格，空表有标题/表头 | T05；本Run提交B1提案；展示main没有Evidence时自述done不计进度，再正常完成 |
| T09 | filters / w3 / Claude Code | `filters/combine.py`及测试实现AND/OR谓词组合；AND空为True；OR空策略经真人ask明确后写入测试，不擅自猜 | T06；一次真实App回答；另一次deferred/迟答作为负例独立记录 |
| T10 | reports主 / w2 / Codex；touches ledger、filters | `reports/summary.py`及测试把金额解析、status筛选、格式化串成摘要；paid/open/void混合输入、空集、非法金额有精确输出/错误 | T07/T08/T09；路径仅reports，但声明所依赖接口/全部fences；涉及high模块实际G1/G4，最终全量main验证 |

T01/T04/T07授权 `ledger/**`；T02/T05/T08/T10授权 `reports/**`；T03/T06/T09授权 `filters/**`。每卡acceptance为表中明确样例和错误条件，不使用“实现功能”空描述。真实慢层提案安排：T08先在reports模块取得真实Run，人在App从B0建立后续草稿，将ledger名称改为“金额解析与分摊”，使用P4已计划的Rename操作；T08按propose技能用真实draft id/revision提交理由/影响/备选/建议，人批准B1。提案有有效运行，不要求Agent直接编辑Module，也不假设尚无契约的App risk编辑操作。T07等冲突任务需复核由App解除；T08本Run如被实际P4影响分析标记，也须按App精确解除并更新main。ledger的high来自人批准B0，不是在Task卡里自报；每个ledger任务都不能省略G1/G4。T10三个模块都空闲后领取。

调度顺序：T01/02/03并行，T04/05/06在各自依赖merged后并行；随后T08先领取并发出B1提案，B1批准/影响任务复核完成后T07/08/09并行推进，最后T10。任何一次main推进后，尚未finish的其他树先status/fetch，再在自己的任务树更新到最新权威main，重跑受影响验证；不能把最初并行通过当后续main仍绿。两harness各实际完成5任务，3树有不同task/run记录，不以子代理或十次fixture CLI充数。

- [ ] **6.7 绿与提交（2–5分钟）。** `node --test scripts/acceptance/prepare.test.mjs`，检查新fixture三个worktree、branch、初始测试数、无global配置差异、prepare收据与dirty状态。精确提交脚本/cards/测试；尚未运行的十任务表status全部not_run。

### Task 7：安装实测、真人App与十任务会话执行

**Files:** 完善 `skills/INSTALL.md`、更新verification实际记录；不修改核心门禁迎合验收。

- [ ] **7.1 给准确可运行入口（2–5分钟）。** P6真实包已就绪后，将App绝对路径、CLI绝对路径、候选SHA/hash、fixture路径写入验收记录，运行 `open "$TOWERMD_APP"`。`TOWERMD_APP`来自实际构建产物，不硬写未证实bundle布局；CLI最终路径按P6实际sibling定位，不能假设`Contents/Resources`。等项目打开、daemon连接与关注卡片均真实，再请用户按一次具体验收安排做S7/S8、回答、G1/G4与恢复，未到此时不请求笼统批准。
- [ ] **7.2 Claude会话入口（2–5分钟）。** shell会话显式设置 `TOWERMD_CLI`/`TOWERMD_SKILLS_ROOT`，从目标任务树启动 `claude --plugin-dir "$TOWERMD_SKILLS_ROOT"`。保留已有账号，不启用bypass权限；在技能列表确认六项，然后实际调用 `/towermd:towermd-claim`，不是直接粘贴正文冒充原生加载。用户现有会话若不支持动态plugin加载，按相同目录重开/恢复真实会话；不改永久配置。
- [ ] **7.3 Codex手动repo安装（每树2–5分钟）。** 在忽略规则已提交的三树，分别手动执行以下步骤；每目标先检查不存在，任何现有文件/链接停止，不覆盖。根变量必须是解包后真实包含manifest与canonical skills的包根。

```sh
mkdir -p .agents/skills
for name in init baseline claim ask propose finish; do
  target=".agents/skills/towermd-$name"
  test ! -e "$target" && test ! -L "$target" || exit 1
  test -f "$TOWERMD_SKILLS_ROOT/skills/towermd-$name/SKILL.md" || exit 1
  ln -s "$TOWERMD_SKILLS_ROOT/skills/towermd-$name" "$target" || exit 1
done
git status --porcelain=v2 --untracked-files=all
```

该循环只创建用户执行的repo本机安装链接，不修改保护目录/globalconfig；若中途部分成功，先列出并核readlink，再仅手动移除指向本包的链接重试，不递归删目录。普通项目未忽略时先展示dirty结果与手动ignore选择，不能继续claim后到finish才发现。启动 `codex -C "$TASK_TREE"`，在`/skills`确认六项，调用`$towermd-claim`。不要声称存在任意目录 `codex plugin install`。共享server若不继承当前环境，先让会话执行 `test -x <App给出的精确CLI路径>`，任务prompt使用这个明确路径；不依赖未经验证的环境转发、不读全局配置。

- [ ] **7.4 原生加载证据（2–5分钟一次一harness）。** 保存实际版本、会话标识/本机导出位置、技能选择器截图和一次明确加载SKILL路径的会话片段；记录六项的SHA256与包版本。这是M1 self_report技能加载证据，不能声称密码学锁定或M3组装完成。名字冲突时显示实际选择路径，不能随意运行同名用户技能。
- [ ] **7.5 每卡启动prompt（2–5分钟一次一卡）。** 用户在已打开真实会话粘贴以下内容，替换值来自已经创建的任务卡，不把秘密放入prompt：

```text
使用已安装的 towermd-claim、towermd-ask、towermd-propose、towermd-finish 技能完成 <任务ID>。
项目为 <真实项目ID>，当前目标树 <实际路径>；CLI精确路径是 <实际App随包路径>。
先status/task show核卡片；先建t/<任务ID>分支再claim，不改变用户main/index或迁移Run。
按卡片的具体样例先红测再实现，每个检查点status。慢层或越界转提案；缺App回答/评审停车。
finish通过后才做其他技能的PR/merge步骤，写真实五trailer，等main认证merged。
结束附run、Evidence、tree、main提交和未决事项；不要把自己的done当完成。
```

- [ ] **7.6 每卡实现检查点（每阶段2–5分钟）。** 依次记录领取task card→具体测试红→最小代码→测试绿→提交/check→finish ID/wait终态→五trailer/tree核对→普通push→canonical `merge.done`与Task merged/Run ended→最新main全量测试。status/prelude由执行Agent按正常检查点调用，连同原始输出、退出码、续租及可能的投递ack副作用保存；这些是Agent动作，不是collect的“只读查询”。进入deferred/迟答负例后遵守停车安排，不为采集额外发status。harness真实模型实施代码及测试，准备器不得预填十任务实现。若自然实施需更久，仍拆成有证据检查点，不以时间超过5分钟削弱验证。
- [ ] **7.7 每次合并后的main验证（2–5分钟）。** 在已经没有活动Run的自建树或新的detached验证目录，用实际权威main commit运行 `python3 -m unittest discover -s . -p 'test_*.py'`，保存命令/cwd/sha/tree/测试数/exit/loghash。不得切换仍绑定活动Run的树；可以复用P2受控分离验证工作树的main验证证据，必须来源与实际main一致。任何失败立即停后续合并并修复，不宣布“main始终绿”。
- [ ] **7.8 真人B0/B1、回答与G1/G4（每次操作2–5分钟）。** 记录实际人的App操作时间、项目、对象id/revision/hash/target、human actor/eid、before/after。S7编辑及S8批准B0；T08发出Rename提案经App批准B1并复核T07等影响任务；T01/T04/T07及涉及high模块的T10各自G1/G4；T09回答。截图和会话片段为补充，决定性事实是合法App producer事件与归档binding，不能只凭`human:`字样或测试HTTP body。未点击写not_run。
- [ ] **7.9 真人恢复确认（2–5分钟一个检查点）。** 仅在可丢弃fixture：先快照并采集Git/outbox证据，停止该fixture daemon，按P1真实测试已验证的路径隔离/备份hub存储（不猜全局目录、不删除其他项目数据）；用真实`hub rebuild`取得请求ID，App显示该项目精确恢复范围后真人确认。保存确认eid/新epoch/恢复水位/任务历史；旧run迟答仅历史，重新status/claim后继续。若P1不能隔离单项目恢复，先修隔离前置，不触碰用户正在运行的Tower项目。
- [ ] **7.10 真人负例（每例2–5分钟）。** T08先让会话报告done而不finish，App进度不动；在独立负例分支分别尝试越界文件/Module慢层修改，finish必须阻或P5标出，样本总数与检测数记录为N/N=100%。主十任务路径不合入故意违规修改。一次ask实际等到deferred后会话结束；等租约过期后人答复，旧epoch仍不可继续，恢复卡用新claim，保留旧记录。CLI非法approve/human参数实跑非零及无相应事件。失败不隐藏，不为了十条PASS删除原尝试。
- [ ] **7.11 十条审计（2–5分钟每条）。** 用collect归并各harness已保存的raw输出/事件/日志及Git只读材料，逐项核第5节字段，不主动查询目标Run、不抢消息或续TTL；T05压缩后/T01等amend后仍有唯一证明器认证、canonical `merge.done`及Task merged/Run ended；至少两种harness均有5个真实实施片段。缺任一关键证据标incomplete/failed，不能用自动测试补为真人PASS。仅审计完的任务可在verification表标pass。

### Task 8：用户文档、精确提交与P8冻结交接

**Files:** 完成 INSTALL、README、双语日志、verification和todo结果；P8脚本不在本批修改。

- [ ] **8.1 安装说明闭环（2–5分钟）。** 写明验证SHA256SUMS→解包→真实包根→CLI定位→Claude会话plugin/Codex手动repo链接→选择器确认→init/baseline→恢复/更新/卸载。更新时比较六blob hash，既有同名路径不覆盖；卸载只手动移除readlink确指向本包的链接，Claude下次不传plugin参数。没有永久安装命令/全局PATH安装要求/自动gitconfig修改。
- [ ] **8.2 README与双语变化（2–5分钟）。** README链接安装说明/实际App入口/六技能/seed未交付/无头未交付；中英文Unreleased各加一条用户可感知的原生技能协作交付，说明需要现有harness与App放行。只描述实际已通过能力，未真人不能写“已完成十任务”。
- [ ] **8.3 结果回顾（2–5分钟）。** todo登记每Task精确实现SHA与红绿日志；verification填开发候选R、binary/hash/arch、schema hash、自动结果、真实十任务、人操作、失败恢复和限制。记录测试是本机Apple Silicon还是Intel，不能虚构后者。未提交写“未提交”。
- [ ] **8.4 最终静态/适当回归（2–5分钟一组）。** 运行下列本批已实现入口；只有变更了相应Rust/前端才增加所属检查，不因技能文档重复全仓重型测试。

```sh
node --test skills/scripts/check-contracts.test.mjs
node --test scripts/acceptance/prepare.test.mjs scripts/acceptance/collect.test.mjs
node skills/scripts/check-contracts.mjs --root skills --cli "$TOWERMD_CLI"
claude plugin validate --strict skills
git diff --check
```

- [ ] **8.5 P8同tag交接（2–5分钟）。** 向P8提供canonical目录/包根/安装实测矩阵/每blob hash，P8按其Task5从同一冻结tag归档，拒绝混入当前工作区文件和逃逸symlink。P7本轮先记录候选提交R，不能提前写`v0.1.0`已冻结。
- [ ] **8.6 冻结后复用裁决（2–5分钟一项）。** P8冻结R后比对P7验收候选的CLI/core/daemon/App/skills/schema及依赖锁相关blob；完全等价才复用对应证据并记录diff/hash。若只有版本字符串/打包布局变化，重跑解包安装、真实help与两harness加载/一次端到端并记录证据复用理由；若状态/证据/门禁/技能流程/验证命令有实质变更，重跑受影响十任务或整轮，未满足不得发布。P8产物安装测试不得用源码目录启动代替。
- [ ] **8.7 最终提交（2–5分钟）。** 按第8节守卫精确提交文档/结果，登记真实提交号。当前写作阶段到独立review即止，不执行本步骤；实施完成后才交P8，不能仅勾选计划当发布证据。

## 5. 十任务证据目录、记录 schema 与判定

准备器返回具体根，例如 `/private/tmp/towermd-m1-acceptance-<随机后缀>/`，实际路径以其收据为准，不共享硬编码目录。所有会话记录/临时JSON/日志/trailer文件存 `evidence/`，不产生任务树untracked文件。该目录是验收副本，不是唯一产品状态；产品事实仍由hub/仓库保存。

```text
evidence/
  input.json                         # 候选R、binary/App/skill/schema hash、架构、工具版本
  baseline/{scan.json,schema.json,submit.json,draft.json,b0.json,b1.json}
  automated/{contracts.log,mechanisms.json}
  T01/ ... T10/
    task.json                        # 原始卡片、run与完整fences
    session.json                     # harness版本/会话id/技能加载声明与片段定位
    checkpoints.jsonl                # Agent实际status/插话/red/green/commit/wait及续租/投递副作用
    evidence.json                    # 已捕获finish.completed/wait中的权威Evidence原文；可无Git路径
    merge-proof.json                 # 已捕获canonical proof/merge.done及Task merged/Run ended投影
    verification.log                 # 原始验证日志副本与hash
    trailers.txt                     # CLI原输出及main实际消息另存main-message.txt
    git.json                         # before/after tree、压缩/amend、main实际SHA/authority
    main-tests.log                   # 每次实际main的全量测试
    record.json                      # 以下schema；原始文件为支持材料
  human/{b0,b1,g1,g4,answer,rebuild}/  # 原App事实、binding、before/after、截图/操作者记录
  failures/<attempt-id>/             # 原失败不可覆盖，含恢复卡与新run链接
  summary.json                       # 自动/真人/十任务分别汇总
```

`record.json` schema由collect唯一实现，不新建Tower对象type：

```json
{
  "task": "T01", "attempt": 1, "result": "not_run",
  "source": {"sha": "", "tag": null, "cli_sha256": "", "app_sha256": "", "skill_hashes": {}},
  "session": {"kind": "human_opened", "harness": "", "version": "", "id": "", "skill_loaded": "self_report", "transcript_ref": "", "cost_usd": null},
  "binding": {"project": "", "run": "", "host": "", "worktree": "", "branch": "", "module": "", "fences": []},
  "verification": {"commit": "", "tree": "", "evidence_id": "", "captured_source_ref": "", "object_ref": null, "object_sha256": "", "commands_hash": "", "log_ref": "", "log_sha256": "", "result": "", "entered_at": "", "deadline": ""},
  "main": {"authority_ref": "", "commit": "", "tree": "", "trailers": {}, "proof_ref": "", "merge_done_eid": "", "task_state": "", "run_state": "", "test_count": 0, "test_exit": null, "test_log_ref": ""},
  "human_refs": [], "checkpoint_refs": [], "failure_refs": [], "missing": []
}
```

这里的空值表示尚未执行的记录模板，不是通过默认值。result仅not_run/incomplete/failed/pass；pass时所有身份/绑定/版本/hash/证据来源/log/main/测试数与exit/需要的人工事实均非空且一致，测试数>0且exit=0；canonical proof与`merge_done_eid`必需，Task/Run投影分别为merged/ended。`object_ref`只有确有Git快照路径时填写，可为null，但`captured_source_ref`必须指向已捕获的权威Evidence原文，不能以无路径代替无证据。费用未知保持null。各对象原始字段以实际已冻结schema保存，不凭此审计schema重定义产品Evidence。Run历史与主模块epoch来自真实绑定；版本声明及skill_loaded明确不可信自报。

验证记录必须能回答：谁在何树/何分支领取哪个Run；实际运行哪套verify；证据绑定哪个tree；trailer在main哪个提交；主Module epoch和全部touches是否一致；合并后最新main是否实跑验证；人做了哪次App动作；失败如何恢复。不读取harness全量历史数据库，只采用用户可见导出或本次会话片段，且不收集token/认证材料。

## 6. 用户操作边界与失败恢复

自动测试负责人可完成：文档/链接/help检查、真实CLI与daemon集成、程序Appgate回归、夹具准备与只读证据采集。它们证明机制，不能证明真人点击、真实模型会话或新版本已发行。

App就绪后的具体请求应包含：“请打开 `<实际App路径>` 中的 `<实际项目>`，按验收卡依次审阅 `<draft id/revision>` 并批准B0、在已有Claude Code/Codex会话中执行这十张卡；T01/T04/T07/T10时审阅G1/G4，T08时审阅B1提案，T09时回答，恢复卡出现时核范围确认。每张卡有精确CLI与路径，遇deferred结束回合。”一次提出可执行安排，后续App正常呈现具体待办，不能因最初安排而自动产生任何批准事实。

| 失败 | 保留与恢复操作 |
| --- | --- |
| 原生技能未出现/名字冲突 | 保存版本/目录/hash/选择器；核包根或链接、同名优先级，重开同目录会话；不修改全局配置 |
| CLI不能定位/会话没继承env | 从App复制真实绝对路径，`test -x`/version/help核同binary；提示词显式带路径，不安装全局PATH |
| claim被占/绑定不匹配 | 不写代码；status列可领取任务，等待模块或结束当前绑定后自建新任务树，不迁移旧Run |
| 人未答/deferred/旧epoch | 保存恢复卡并结束回合；回来先status；旧答仅历史，按新Run重新确认问题，不能静默应用旧批准 |
| dirty/越界/慢层 | 列具体diff与gate证据；保留用户原改动，不reset；自己的越界转propose，App批准前不合并 |
| verification失败/no_tests | 保留原Evidence/log；失败修授权代码再验证；no_tests先App批准verify并补测试 |
| main推进/冲突/tree变化 | 在任务树显式解决并重核路径/基线/fences；tree变重新finish，旧deadline不重置 |
| 验证已过但60min到期 | 保存旧attempt，status确认释放，重新claim/验证；不能只重写trailer复活 |
| push失败/观察尚未merged | 保存远端refs/退出码，核实际authority再普通重试；不force、不自报done |
| 事后检测发现违规 | 保存违规提交与Hold，App查看diff并按已有撤销/转提案入口处置；只用revert撤销main历史 |
| 实机/真人条件缺失 | 标not_run/incomplete并记录具体阻塞；完成其他可执行准备，不生成虚假PASS |

## 7. 覆盖与验收门槛

| 规格/总表目标 | 本计划位置 | 需要的证据类别 |
| --- | --- | --- |
| FR-104–105六项原生技能/用户安装 | Tasks1–4、7.2–7.4、8.1 | 静态/help自动 + 两harness真实加载 |
| FR-106先树后claim/self_reported | Task3、Task6调度、逐卡binding | 实际CLI/Run/worktree/会话 |
| FR-107守卫/验证/tree/trailer/合并认证 | Tasks4–5、7.6–7.11 | 自动负例 + 十任务真实Evidence/main |
| FR-108其他技能顺序 | 共享契约、Task4.5、启动prompt | 正文自动检查 + 实际收尾顺序 |
| FR-109检查点投递 | Task3、T06、checkpoints | 真实App插话→CLI prelude→delivered |
| S6单schema、S7/S8 B0 | Task2、6.5、7.8 | schema hash + 真实会话综合 + 真人App/出版 |
| 高风险G1/G4/no_tests不绕过 | Task3.5、4.3、T07及自动矩阵 | 精确App binding/硬阻日志 |
| 三树/两harness/十任务/main始终绿 | Task6表、Task7、record schema | 十条真实完整记录，每次main测试 |
| done无证据不计进度/违规100% | Tasks5、7.10 | 进度before/after、N/N阻断或P5检测 |
| trailer/amend/压缩仍merged | Task4.5–4.6、T05 | tree前后/实际main五trailer/merged事实 |
| 真人恢复确认/CLI不能批准human | Tasks5、7.9–7.10 | App合法producer事实/旧epoch/CLI拒绝 |
| 版本化技能与P8同tag | Tasks1.2、8.5–8.6 | 候选R/冻结tag/blob与binary hash/复用裁决 |

十任务完成数、自动回归通过数、真人操作通过数分别统计。未执行的真实任务、没有截图以外事实的批准、测试HTTP请求、子代理、自带fixtures或无头exec均不能进入真实十任务分子。越界/慢层负例报告完整样本及处置，不声称v0.1有M3不可绕过的合并前集成器。

## 8. 每次精确提交守卫与最终回顾

此节是未来实施每个Task共用的提交步骤；当前写作明确不提交。实施者开工先记录 `git status --short`、`git diff --cached --binary`及stage0 index条目，把原index diff保存工作树外。禁止`git add .`/`git commit -a`/reset index；未跟踪AGENTS/其他并行计划不属于本批。

1. 完整阅读本次路径的 `git diff -- <精确文件列表>`、`git diff --cached -- <精确文件列表>`，确认每个hunk直接属于该Task且对应验证已通过。
2. 新文件精确stage；共享文件先核原staged内容/并行hunk，只stage本Task补丁。若同一hunk混入另一任务改动，暂不提交该hunk，先与唯一责任人协调或隔离验证，不把它顺带提交。
3. 原index有其他任务内容时，保留其原状；在独立干净worktree/独立分支装入本Task补丁并验证提交。若使用临时`GIT_INDEX_FILE`，也必须绑定独立分支/工作树，从该分支HEAD用`git read-tree HEAD`初始化，不能在共享分支推进HEAD改变原index含义。保存原index文件SHA256，结束核hash不变；不能把原暂存内容复制进本次提交，也不能自动整合另一分支。无法隔离共享hunk时停在“已验证、未提交”。
4. 只有初始index没有待提交差异时，才在当前树精确stage本Task。提交前跑 `git diff --check`、`git diff --cached --check`、本Task测试并逐行审阅完整`git diff --cached --binary`；记录commit SHA及 `git show --format= --binary <sha>`、实际文件列表。提交后核他人工作区/index保留，verification/todo登记结果；stat和name-only不能代替完整内容审阅。

建议独立提交依次是包/契约检查、init+baseline、claim+ask、propose+finish、只读收集器、隔离验收准备、真实验收与安装结果、最终用户文档。每个提交只能包括本Task已验证的变更；未提交结果明确说明，不能声称已进入发布。

实施后的回顾固定填写：前置真实SHA/候选R、六技能与CLI/schema hash、自动红绿命令/测试数、两harness版本/三树/十条record路径、App真人操作事实、失败恢复、精确提交号、P8冻结复用裁决、未验证架构与其他限制。发布前全部worktree/分支/stash审计仍由P8负责，不因P7表打勾跳过。

编写阶段结构自审：本文件保留全部未执行状态；安装依据已本机help及官方核实；六技能、S6–S8、十张具体任务卡、三树调度、自动/真人分界、证据schema/路径、恢复、同tag和提交守卫均有对应Task。P5 frontier语义已由用户裁决；当前未完成条件为P5及其余前置真正实现后的SHA/字段/包布局核验、App就绪后的真人操作与最终冻结tag。它们有明确接收动作和阻塞规则，不能以假命令、模拟完成或自动批准填补。
