---
type: Plan
title: M1 双架构未签名 App 发布实施计划
description: 用逐项审计、脚本红绿测试、冻结源码、真实 App 验收与公开匿名回读交付 v0.1.0 macOS App、DMG 和原生技能包。
status: draft
sources:
  - resource: 2026-10-17 用户授权：持续开发直到发布首个可用 App，先不签名
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: docs/superpowers/plans/2026-10-17-m1-app-foundation.md
  - resource: docs/superpowers/plans/2026-10-17-m1-native-skills-acceptance.md
  - resource: AGENTS.md
  - resource: tasks/lessons.md
  - resource: ../note.md/package.json
  - resource: ../note.md/src-tauri/Cargo.toml
  - resource: ../note.md/src-tauri/tauri.conf.json
  - resource: ../note.md/scripts/release.sh
  - resource: ../note.md/scripts/changelog-core.mjs
  - resource: ../note.md/scripts/release-publish-core.mjs
  - resource: https://v2.tauri.app/reference/config/
  - resource: https://v2.tauri.app/develop/sidecar/
  - resource: https://v2.tauri.app/distribute/sign/macos/
---

# M1 双架构未签名 App 发布 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从同一个已验证的 `v0.1.0` 冻结提交交付 macOS aarch64/x86_64 的真实 `.app`、DMG、原生技能包、SHA256SUMS 和双语说明，并证明公开下载内容与本地产物一致。

**Architecture:** P6 提供固定 sibling CLI 的桌面包与项目工具启动器，P7 提供真实原生技能及十任务验收；P8 只增加发布输入审计、版本/日志门禁、分架构构建、产物检查和发布回读。脚本以显式参数和 Node 标准库编排，调用 Git、cargo、项目 pnpm、Tauri 和 macOS 系统工具，不引入新依赖、不经 shell 拼接用户参数、不调用 Note.md 的签名发行脚本。

**Tech Stack:** pnpm 11.7.0、Tauri 2、Svelte 5、Vite 6、TypeScript 5、Svelte Flow 1.6.6、Rust stable/edition 2021、Node 内置 `node:test`、现有 vitest；`git`、`ditto`、`hdiutil`、`lipo`、`codesign`、`spctl`、`shasum`、`gh`、`curl`。

---

## 当前事实、实施门槛和授权

本文件是未来 P8 的可执行计划，**不是发布记录**。编写时根 Cargo workspace 的版本为 `0.0.0`；没有前端 `package.json`、`src-tauri/`、`scripts/` 或技能实现，M0 批 3.1 仍在实施。P2–P7 目前不能写成已交付，P6 基础计划也不能替代 P6 人工闭环集成。开始实施各 Task 前重新核对最新文件，复用已落地的入口，避免覆盖唯一实现者的改动。

P8 脚本开发可在 P6 基础落地后进行；Task 6 的版本轮转/冻结必须等待 P1–P7 实际完成。必须具备 P7 的 **3 个真实工作树、Claude Code 与 Codex 真实会话、10 个真实任务**，以及真人从 App 操作的批准/恢复确认记录。fixture、自动调用 HTTP、UI 自动化和 Agent 声称完成均不替代这些证据。每个任务须能沿 Run → tree → Evidence → main trailer → 合并后验证追溯。

用户已授权持续开发及首版发布，不再请求笼统发布许可。需要真人操作时，先提供实际可打开的 `.app`、准确项目和卡片入口、预期结果，再请求该次具体验收；不能代点后署名为真人。操作系统联网安装权限若拦截 `rustup target add` 或项目 pnpm 安装，可为精确命令申请工具审批；不提前安装暂未使用的依赖，不换账号、不读取秘密。

只发布 macOS 两架构；Windows/Linux、发行者证书、公证、updater 签名及更新通道均不在范围。这里的“未签名”指没有 Apple 发行者身份和公证。按 [Tauri 的 ad-hoc 文档](https://v2.tauri.app/distribute/sign/macos/#ad-hoc-signing)，为 Apple Silicon 运行需要允许 `signingIdentity: "-"`；报告实际 codesign 结果，明确其不是 Developer ID。系统仍可能要求在“隐私与安全性”允许打开，说明须如实告知，不能要求用户关闭全局 Gatekeeper。

## 文件地图和执行顺序

| 文件 | 责任与归属 |
| --- | --- |
| `scripts/release/core.mjs`、`scripts/release/core.test.mjs` | 纯函数：版本、双语日志阶段、unsigned 配置、资产集合、hash 与重试判断 |
| `scripts/release/audit.mjs`、`scripts/release/audit.test.mjs` | 全部 worktree/branch/stash 输入快照与逐项范围核对；只读 Git |
| `scripts/release/prepare.mjs`、`scripts/release/prepare.test.mjs` | 同步版本、轮转双语日志；仅显式列出的项目文件可写 |
| `scripts/release/build.mjs`、`scripts/release/build.test.mjs` | 冻结 tag 检查、逐 target 调 P6 stage/build、收集产物；禁止读取秘密 |
| `scripts/release/assets.mjs`、`scripts/release/assets.test.mjs` | App/DMG/技能核验、发行说明、manifest、SHA256SUMS |
| `scripts/release/publish.mjs`、`scripts/release/publish.test.mjs` | 精确 tag/draft 资产上传、重试一致性、公开发布、匿名回读 |
| `src-tauri/tauri.unsigned.conf.json` | 发行者未签名、ad-hoc、关闭 updater 的专用覆盖配置 |
| `scripts/{project-pnpm,stage-cli,build-app}.mjs` | P6 已有入口；仅补 `--locked`、显式 config/bundles 和环境转发，沿已有接口修改 |
| 根 `Cargo.toml`、`Cargo.lock`、`package.json`、`pnpm-lock.yaml`、`src-tauri/{Cargo.toml,Cargo.lock,tauri.conf.json}` | 版本与锁文件；P8 不重建 App 或升级依赖 |
| `CHANGELOG.md`、`CHANGELOG.zh-CN.md` | 用户变化；准备时从非空未发布区轮转成同日期 v0.1.0 节 |
| `docs/verification/2026-10-17-release-readiness.md` | 冻结前 P1–P7/功能/日志/提交对应及范围裁决，带 OKF frontmatter |
| `docs/verification/releases/v0.1.0.md` | 冻结后实际架构/包/真人/公开回读证据；在后续独立证据提交中登记，不能回写冻结 tag |
| `skills/` | P7 已验证的原生目录和安装说明；P8 只打包冻结 tree，不另造技能、不改 harness 全局配置 |
| `.release/` | 忽略的本地输入审计与临时凭据无关记录；不保存唯一业务状态，不属于 `.towermd/` |
| `/private/tmp/towermd-release-v0.1.0-*` | 两个干净 detached 构建 worktree、产物、日志和匿名下载；不提交二进制 |

**实施次序：Task 1–5 → Task 9 工具红绿测试与提交 → Task 6 → Task 7 → Task 8 → Task 9 实际发布/回读 → Task 10。** Task 1–5 交付可测试发布工具；Task 6 冻结前核验与版本提交；Task 7 冻结 tag 后两架构构建；Task 8 从冻结包真人/实机验收；Task 9 发布与回读；Task 10 单独提交证据。先开发发布工具、后执行外部发布，确保全部脚本在 R 里。每个 checkbox 是约 2–5 分钟的单一工作动作；编译、网络传输和真人验收可超过 5 分钟，单列启动/检查动作并持续报告状态，不为凑时长中断构建。

### 共同命令与接口

所有新脚本使用 ESM、Node 标准库和结构化 `spawnSync(file, args, {cwd, env, shell:false})`。以下签名是**本计划要新增的接口**，不声称仓库已有；仅 P6 工具启动器和 build/stage 入口在前置计划定义。CLI 解析拒绝未知参数、缺参数、相对输出逃逸、版本非法及非允许 target。没有 `--force`、`--skip-gate`、`--ignore-dirty` 或自动批准开关。

| 新增导出签名（JavaScript） | 返回值/结果 |
| --- | --- |
| `parseChangelog(text, lang)` | `{unreleased, versions:[{version,date,body}]}` |
| `checkChangelogs(en, zh, phase, version)` | 成功无返回，失败抛 Error |
| `assertVersions(values, expected)` / `assertUnsigned(config)` | 成功无返回，失败抛 Error |
| `unsignedEnv(env, pnpmBin)` | 允许字段组成的新环境对象 |
| `expectedAssets(version)` | 排序后的九个 basename |
| `assertAssetSet(names, version)` / `verifySums(text, files)` | 成功无返回，失败抛 Error |
| `assertRemoteReady(remote, tag, expectedBody)` | 精确远端 tag/九资产/完整双语正文一致，否则抛 Error；Task 9 增补到 core |
| `uploadAction(local, remote)` | `'upload'` / `'reuse'`，冲突抛 Error |
| `inventory(repo, execGit)` | `{head,main,items,inventorySha256}` |
| `checkScope(inventory, decisions)` | 成功无返回，缺项/漂移抛 Error |
| `prepareVersion(root, version, date, run)` | 更新白名单项目版本/日志，返回修改路径 |
| `buildFrozen({root,tag,target,output}, run)` | `{tag,commit,target,app,dmg,tools,lockfiles}` |
| `inspectApp({app,target,version}, run)` | 检查记录；不满足契约抛 Error |
| `assembleAssets({root,tag,output,builds}, run)` | manifest 与本地 asset records |
| `publishFrozen({repo,tag,output,download}, run)` | Promise，返回 release URL 与匿名回读收据 |

各 Task 的实现代码/算法、实际输入输出、失败边界和命令在下面定义。`run` 是测试可替换的固定 argv 执行器，返回 `{status, stdout, stderr}`；生产无任意执行 HTTP/App 入口。出错只打印文件名/字段名/退出码，不能输出环境值或 token。

### 所有 Task 的统一提交守卫

下面守卫适用于本文每一次 `git add` / `git commit`，包括版本准备 R、发布工具和证据 E；显式路径本身不能证明提交中没有其他任务内容。

- 暂存前只读检查 `git diff --cached --name-status` 和完整 `git diff --cached --binary`，记录当前 index 文件的 SHA256；有任何原已暂存内容就保留其原状，不能 reset、取消暂存、隐式 stash 或把它一起提交。只有初始 index 无待提交差异时，才允许在当前工作树暂存本任务改动。
- 原 index 非空，或无法把共享文件精确分离时，在独立干净 worktree/独立分支完成本任务的验证与提交；也可用临时 `GIT_INDEX_FILE`，但必须绑定独立分支/工作树，不能在共享分支推进 HEAD 使用户原 index 的含义改变。临时 index 从该分支 HEAD 用 `git read-tree HEAD` 初始化，不复制原暂存内容进入本次提交；结束后核原 index SHA256 未变，不自动整合尚未验证的其他分支。
- 新建且完全属于本任务的文件可 `git add -- <显式路径>`；`tasks/todo.md`、总表、`.gitignore` 等共享文件必须 `git add -p -- <显式路径>` 逐 hunk 暂存。一个 hunk 混有他人内容就拆分 hunk/使用只含本任务行的 index 补丁；无法安全拆分时停该提交、改用独立干净 worktree，不以整文件暂存代替。
- 提交前必须逐行审阅完整 `git diff --cached --binary`，核路径白名单和实际内容均只有本任务，并运行 `git diff --cached --check`；`--stat`/`--name-only` 仅辅助，不能替代内容审阅。若出现额外内容，拒绝提交并保留原 index，不自动清空重来。提交后核实际 `git show --format=fuller --stat HEAD` 及 `git show --format= --binary HEAD` 与已验证补丁一致，再记录提交号。

```bash
# 只读起始检查；非空时进入上面的隔离工作树路径，不执行后续共享树暂存。
git diff --cached --name-status
git diff --cached --binary
git diff --cached --quiet
# 精确暂存本任务路径/hunk 后、每次 commit 前都执行：
git diff --cached --binary
git diff --cached --check
```

## Task 1：纯发布契约和阶段门禁

**Files:** Create `scripts/release/core.mjs`、`scripts/release/core.test.mjs`。

- [ ] **写红测试（2–5 分钟）：** 用 `node:test` 和 `node:assert/strict` 写下列首组断言。日志 fixture 用真实根格式，不沿用 Note 的日期版本自动推导。

```js
import test from 'node:test';
import assert from 'node:assert/strict';
import { checkChangelogs, assertVersions, assertUnsigned, unsignedEnv,
  expectedAssets, assertAssetSet, verifySums, uploadAction } from './core.mjs';
const en = '# Changelog\n\n## Unreleased\n\n- Open projects.\n';
const zh = '# 更新日志\n\n## 未发布\n\n- 打开项目。\n';
test('冻结前双语非空，版本必须逐一对应', () => {
  assert.doesNotThrow(() => checkChangelogs(en, zh, 'prepare', '0.1.0'));
  assert.throws(() => checkChangelogs(en, '# 更新日志\n## 未发布\n', 'prepare', '0.1.0'));
  assert.throws(() => checkChangelogs(en + '\n## v0.0.1 — 2026-10-16\n- A\n', zh, 'prepare', '0.1.0'));
  assert.throws(() => checkChangelogs(en, zh, 'frozen', '0.1.0'));
});
test('任何版本来源漂移均阻止发行', () => {
  assert.doesNotThrow(() => assertVersions({core:'0.1.0', app:'0.1.0', frontend:'0.1.0'}, '0.1.0'));
  assert.throws(() => assertVersions({core:'0.1.0', app:'0.0.0'}, '0.1.0'));
});
test('ad-hoc 可以，发行者身份和 updater 不可以', () => {
  const c = { bundle:{ createUpdaterArtifacts:false,
    externalBin:['binaries/towermd'], macOS:{signingIdentity:'-'} }, plugins:{} };
  assert.doesNotThrow(() => assertUnsigned(c));
  assert.throws(() => assertUnsigned({...c,bundle:{...c.bundle,createUpdaterArtifacts:true}}));
  assert.throws(() => assertUnsigned({...c,bundle:{...c.bundle,macOS:{signingIdentity:'Developer ID'}}}));
  assert.throws(() => assertUnsigned({...c,plugins:{updater:{}}}));
  const e = unsignedEnv({PATH:'/usr/bin',HOME:'/tmp/test-home',
    APPLE_PASSWORD:'test-sentinel',TAURI_SIGNING_PRIVATE_KEY:'test-sentinel',TAURI_CONFIG:'bad'}, '/tmp/pnpm/bin');
  assert.equal(e.APPLE_PASSWORD, undefined);
  assert.equal(e.TAURI_SIGNING_PRIVATE_KEY, undefined);
  assert.equal(e.TAURI_CONFIG, undefined);
});
```

- [ ] **运行红测试（2 分钟）：** `node --test scripts/release/core.test.mjs`。先提供可载入的最小导出 stub（校验器暂时无操作、资产函数返回空数组，禁止任何真实构建/网络副作用），再运行测试；必须观察“非法版本/开启 updater 本应抛错却被接受”等行为断言 FAIL 才记红。import/语法/编译失败仅是接口准备，不能记作红；不能把无网络、无 target 或 pnpm 下载失败当逻辑红测试。空 stub 不得提交。
- [ ] **实现最小纯函数（2–5 分钟）：** `parseChangelog` 识别唯一且靠前的 `## Unreleased`/`## 未发布`，版本标题匹配 `^## v(\d+\.\d+\.\d+) — (\d{4}-\d{2}-\d{2})$`，返回 `{unreleased, versions:[{version,date,body}]}`；重复标题、重复版本、非法顶层版本标题拒绝。删除 HTML 注释后只有 `- ` / `* ` 实际条目才算内容，`### Added` 不算。
- [ ] **实现阶段区别（2–5 分钟）：** `prepare` 要求两份未发布都非空、版本序列和日期一致、没有目标版本；`frozen` 要求两份最高版本均为目标版本且正文非空、全部历史序列一致、顶部未发布保留但可空。轮转后绝不再调用 `prepare`，不以占位条目填空未发布区；下一次发布仍从非空 `prepare` 门禁开始。加入“轮转后空区可 frozen，但不可再次 prepare”和“只有注释/小标题不可 prepare”的断言。
- [ ] **实现环境/资产/hash 契约（2–5 分钟）：** 采用以下完整实现；`assertVersions` 还拒绝空 values 和非法 expected；`assertUnsigned` 明确检查 bundle/插件/sidecar；`verifySums` 拒绝重复、缺项、路径分隔符和多余项，files 为 `Map<basename,Buffer>`。

```js
import { createHash } from 'node:crypto';
export function assertVersions(values, expected) {
  if (!/^\d+\.\d+\.\d+$/.test(expected) || !Object.keys(values).length)
    throw new Error('invalid version input');
  for (const [name, value] of Object.entries(values))
    if (value !== expected) throw new Error(`version mismatch: ${name}`);
}
export function assertUnsigned(c) {
  if (c.bundle?.createUpdaterArtifacts !== false || c.plugins?.updater !== undefined)
    throw new Error('updater forbidden');
  if (c.bundle?.macOS?.signingIdentity !== '-') throw new Error('ad-hoc required');
  if (JSON.stringify(c.bundle?.externalBin) !== JSON.stringify(['binaries/towermd']))
    throw new Error('fixed CLI missing');
  if (c.bundle.macOS.providerShortName != null) throw new Error('notarization forbidden');
}
export function unsignedEnv(env, pnpmBin) {
  const out = {};
  for (const key of ['HOME','TMPDIR','LANG','LC_ALL','SDKROOT','DEVELOPER_DIR',
    'MACOSX_DEPLOYMENT_TARGET','CARGO_HOME','RUSTUP_HOME'])
    if (env[key] !== undefined) out[key] = env[key];
  out.PATH = `${pnpmBin}:${env.HOME}/.cargo/bin:${env.PATH ?? '/usr/bin:/bin:/usr/sbin:/sbin'}`;
  return out;
}
export function expectedAssets(v) {
  assertVersions({v}, v);
  return ['aarch64','x86_64'].flatMap(a => [
    `tower.md-${v}-${a}.app.zip`, `tower.md-${v}-${a}.dmg`
  ]).concat([`towermd-skills-${v}.tar.gz`, 'BUILD-MANIFEST.json',
    'RELEASE-NOTES.en.md','RELEASE-NOTES.zh-CN.md','SHA256SUMS']).sort();
}
export function assertAssetSet(names, v) {
  if (new Set(names).size !== names.length ||
      JSON.stringify([...names].sort()) !== JSON.stringify(expectedAssets(v)))
    throw new Error('asset set mismatch');
}
export function verifySums(text, files) {
  const rows = text.trim().split('\n'), seen = new Set();
  for (const row of rows) {
    const m = /^([a-f0-9]{64})  ([A-Za-z0-9_.-]+)$/.exec(row);
    if (!m || m[2] === 'SHA256SUMS' || seen.has(m[2]) || !files.has(m[2]))
      throw new Error('invalid checksum set');
    seen.add(m[2]);
    if (createHash('sha256').update(files.get(m[2])).digest('hex') !== m[1])
      throw new Error(`checksum mismatch: ${m[2]}`);
  }
  if (seen.size !== files.size) throw new Error('incomplete checksum set');
}
export function uploadAction(local, remote) {
  if (!remote) return 'upload';
  if (local.size === remote.size && local.sha256 === remote.sha256) return 'reuse';
  throw new Error(`remote asset conflict: ${local.name}`);
}
```

- [ ] **补边界并运行绿测试（2–5 分钟）：** 向 `verifySums` 输入一字节被改的 Buffer 必须抛 checksum mismatch；丢一项、重复项、`../file`、多余文件、hash 大小写不规范均拒绝；给同名远端不同 bytes 的 `uploadAction` 必须拒绝，完全相同复用。`node --test scripts/release/core.test.mjs` 全 PASS。
- [ ] **精确提交（2 分钟）：** `git add scripts/release/core.mjs scripts/release/core.test.mjs`，`git commit -m "test(release): define unsigned release gates"`。在 `tasks/todo.md` 登记真实提交号；未提交就写“未提交”，不替其他任务提交。

## Task 2：全仓输入范围审计，保护其他工作

**Files:** Create `scripts/release/audit.mjs`、`scripts/release/audit.test.mjs`；Modify `.gitignore` 仅加入 `.release/`。审计 JSON 是本地输入，不加入发布包。

- [ ] **写红测试（2–5 分钟）：** 临时 Git 仓库显式建立 main、两个 worktree、一个未合入 feature commit 和 stash；一处 tracked dirty，一处 untracked。`inventory(repo, execGit)` 必须列全；未裁决、缺 digest、增加新文件、stash 改变、branch tip 改变均使 `checkScope` 拒绝。
- [ ] **运行红测试（2 分钟）：** 先提供可载入 `inventory` 返回空 items / `checkScope` 无操作的最小 stub，再运行 `node --test scripts/release/audit.test.mjs`；预期真实临时仓库有 dirty/untracked/branch/stash 而 items 未列全、漂移输入未被拒绝的断言 FAIL。import/编译错误不记红；stub 不提交。
- [ ] **实现只读采集（2–5 分钟）：** 按下面固定命令获得原始记录，使用 `-z` 解析文件名（含空格/中文/换行），保留 XY 状态、原/新路径和内容 hash；无 shell、无 reset/stash/clean/checkout。binary dirty 用文件 SHA256，tracked index 用 `git diff --cached --binary` hash，工作区 diff 用 `git diff --binary` hash；不要把本机绝对路径公开到资产。

```bash
git worktree list --porcelain -z
git -C /absolute/worktree status --porcelain=v1 -z --untracked-files=all
git -C /absolute/worktree diff --binary
git -C /absolute/worktree diff --cached --binary
git for-each-ref --format='%(refname)%09%(objectname)' refs/heads
git log --format='%H' main..refs/heads/feature
git cherry main refs/heads/feature
git stash list --format='%H%x09%gd%x09%gs'
git stash show --include-untracked --name-status -z 'stash@{0}'
```

- [ ] **实现逐项裁决（2–5 分钟）：** `inventory` 返回 `{head,main,items:[{id,kind,sha256,summary,details}]}`；`id` 是 canonical worktree+status+路径、branch+tip+commit 或 stash object SHA；inventory 规范 JSON 的 SHA256 为 `inventorySha256`。裁决文件格式如下；`checkScope` 要求 inventory digest 精确相同，items 一一覆盖、无多余项，reason/evidence 非空。`include` dirty 必须先独立验证提交，再重采集，不允许带入冻结构建；`exclude` 保留原处，不删除/移动/提交；patch equivalent 必须同时有 `git cherry`/`patch-id --stable` 结果和冻结源码实际功能/测试证据，不能仅“祖先不在 main”或仅 `-` 号。

```json
{
  "inventorySha256": "64位实际采集hash",
  "items": [{
    "id": "实际inventory项目ID",
    "disposition": "exclude",
    "reason": "另一任务正在进行，原地保留且不进入冻结tree",
    "evidence": "实际负责批次/验证文档路径或实际patch-id与测试记录"
  }]
}
```

上面 JSON 是格式说明，执行时只使用采集的真实 ID/hash/依据。任何未知项都停止冻结并报告具体差异，不自动泛化白名单。原已存在的未跟踪 `AGENTS.md`、`CLAUDE.md`、`docs/superpowers/plans/2026-10-17-s5-isolated-verification.md` 必须逐项保留/核对，不顺手提交、清理或覆盖。并行实施中若它们已有合法独立提交，以新审计事实重新登记。

- [ ] **增加 CLI 和绿测（2–5 分钟）：** `node scripts/release/audit.mjs scan --root . --output .release/inventory.json` 写 JSON；`node scripts/release/audit.mjs check --root . --decisions .release/scope.json` 重新采集后比较，只有全部匹配才输出 `scope verified`。测试注入固定 execGit，证明 `git clean`、`reset`、`stash pop`、全局 config 从不调用，worktree 列表不可读时失败，stash patch 不保存秘密原文到发布包。
- [ ] **精确提交（2 分钟）：** 先执行统一提交守卫，`git add -- scripts/release/audit.mjs scripts/release/audit.test.mjs`，`git add -p -- .gitignore` 仅暂存本任务 ignore hunk；完整 cached 内容审阅后 `git commit -m "feat(release): audit all worktrees branches and stashes"`。只增加自己那条 ignore，不暂存别人的修改。

## Task 3：版本同步和双语日志轮转

**Files:** Create `scripts/release/prepare.mjs`、`scripts/release/prepare.test.mjs`；未来 Task 6 才修改版本/CHANGELOG。

- [ ] **写红测试（2–5 分钟）：** fixture 包含根 workspace version、两个 `version.workspace = true` 核心 crate、独立 `[workspace]` 桌面 package、frontend version、tauri version、两处 Cargo.lock；一个版本不等、锁里本项目 package 旧版、根 workspace 漏 exclude、桌面缺 `[workspace]` 都拒绝冻结检查。准备命令不接受把 Node/第三方依赖版本批量替换。
- [ ] **运行红测试（2 分钟）：** 先提供无文件副作用的可载入 stub，再运行 `node --test scripts/release/prepare.test.mjs`；预期“准备后版本仍旧/非法版本来源未拒绝”的行为断言 FAIL，import/编译失败不记红，stub 不提交。
- [ ] **实现 manifest 定点更新（2–5 分钟）：** JSON 用 parse/stringify 保持既有缩进；TOML 只更新唯一 `[workspace.package]` version 和桌面 `[package]` version，不做全文件正则替换。核心 version 来源用 `cargo metadata --no-deps --format-version 1` 验证；桌面单独传 `--manifest-path src-tauri/Cargo.toml` 验证。pnpm-lock 若根 importer 含项目版本字段则按现有锁格式更新，无该字段不杜撰。写入清单固定，检测未知版本来源时报文件/字段名停止。
- [ ] **实现锁文件更新（2–5 分钟）：** `prepareVersion(root, version, date, run)` 先跑 `checkChangelogs(...,'prepare',version)`，再写 manifest，调用以下精确命令；使用已缓存依赖优先，不安装新依赖。`--offline` 缺 cache 时报告具体缺项，再为正常必要联网安装走工具审批；不要删锁或升级依赖以解决缓存问题。

```bash
~/.cargo/bin/cargo metadata --offline --no-deps --format-version 1
~/.cargo/bin/cargo metadata --offline --no-deps --format-version 1 --manifest-path src-tauri/Cargo.toml
node scripts/project-pnpm.mjs install --offline --lockfile-only
```

metadata 若当前 Cargo 版本没有更新 package lock 的本地版本条目，运行对应 `cargo check --offline` / `cargo check --offline --manifest-path src-tauri/Cargo.toml` 生成后再检查；不得手编依赖 checksum。随后 `cargo metadata --locked --offline ...` 和项目 `pnpm install --frozen-lockfile --offline` 验证锁；所需网络与编译另列启动/结果动作。

- [ ] **实现双语轮转（2–5 分钟）：** 把两个非空未发布正文移入 `## v0.1.0 — YYYY-MM-DD`，顶部保留空 `## Unreleased`/`## 未发布`；旧版本正文逐字保留；日期取执行时显式 `--date`，不用当前日期偷偷漂移。再次运行时若已轮转，不重复轮转、不覆盖现有版本；使用只读 `check` 路径确认。以下行为由测试断言。

```js
// prepare.test.mjs 中 fixture 的关键后置断言。
assert.match(enAfter, /## Unreleased\n\n## v0\.1\.0 — 2026-10-17/);
assert.match(zhAfter, /## 未发布\n\n## v0\.1\.0 — 2026-10-17/);
assert.doesNotThrow(() => checkChangelogs(enAfter, zhAfter, 'frozen', '0.1.0'));
assert.throws(() => checkChangelogs(enAfter, zhAfter, 'prepare', '0.1.1'));
assert.equal(enAfter.slice(enAfter.indexOf('## v0.0.1')), oldEnglishSection);
```

- [ ] **绿测与只读命令（2–5 分钟）：** `node --test scripts/release/core.test.mjs scripts/release/prepare.test.mjs` PASS；`node scripts/release/prepare.mjs check --root . --version 0.0.0 --phase prepare` 此时只确认现有版本/日志，不轮转。未来真实调用为 `node scripts/release/prepare.mjs prepare --root . --version 0.1.0 --date 2026-10-17`；若执行日不同，显式传真实日期并保留双语一致。
- [ ] **精确提交（2 分钟）：** `git add scripts/release/prepare.mjs scripts/release/prepare.test.mjs`，`git commit -m "feat(release): synchronize versions and rotate bilingual changelogs"`。本任务不提前提交 0.1.0 版本，也不声称现有功能全部可发布。

## Task 4：隔离未签名环境和冻结构建编排

**Files:** Create `src-tauri/tauri.unsigned.conf.json`、`scripts/release/build.mjs`、`scripts/release/build.test.mjs`；Modify P6 `stage-cli.mjs`/`build-app.mjs` 及其原测试，仅补所需参数和 locked 构建。

- [ ] **写红测试（2–5 分钟）：** fake runner 记录每个 argv/cwd/env/readFile 路径。非允许 target、HEAD 不等 tag、tag 不存在、dirty detached tree、缺 CLI、CLI 错 arch、merge 后配置仍有 updater/证书身份，均须在 Tauri build 前拒绝。伪造 `.env.release`、默认 updater key 路径写入 getter trap，任何读取直接测试失败；断言所有 build 子进程没有 Apple/TAURI_SIGNING/TAURI_CONFIG 等继承变量。
- [ ] **运行红测试（2 分钟）：** 提供可载入且不调用真实 subprocess 的最小 stub 后运行 `node --test scripts/release/build.test.mjs`；预期 wrong arch/tag/dirty 输入未被拒绝、fake runner 未收到固定 argv 的行为断言 FAIL。import/编译失败不记红、stub 不提交；缺真实 target 不参与这组测试。
- [ ] **创建专用配置（2 分钟）：** 完整覆盖文件如下。基配置/P6 native crate 中不启用 updater 插件；平台附加 `tauri.macos.conf.json` 如存在也须审查，不能假设覆盖即移除了其他签名字段。[Tauri 配置参考](https://v2.tauri.app/reference/config/)是字段与合并依据。

```json
{
  "bundle": {
    "active": true,
    "targets": ["app", "dmg"],
    "createUpdaterArtifacts": false,
    "externalBin": ["binaries/towermd"],
    "macOS": {"signingIdentity": "-", "providerShortName": null}
  }
}
```

- [ ] **实现冻结检查和 CLI 构建（2–5 分钟）：** `buildFrozen` 只接受 `aarch64-apple-darwin` / `x86_64-apple-darwin` 和 `v0.1.0` 格式精确 tag；`git rev-parse <tag>^{commit}` 必须等 cwd HEAD，`git status --porcelain -z --untracked-files=all` 必须空（忽略生成物除外）。先 `prepare ... check --phase frozen`；P6 stage-cli 增加 `--locked`，实际子进程如下，复制必须来自对应 target，不取 host target/release。

```bash
~/.cargo/bin/cargo build --locked --release -p towermd --target aarch64-apple-darwin
~/.cargo/bin/cargo build --locked --release -p towermd --target x86_64-apple-darwin
```

stage 结果严格为 `src-tauri/binaries/towermd-aarch64-apple-darwin` 和 `...-x86_64-apple-darwin`；[externalBin 规则](https://v2.tauri.app/develop/sidecar/)要求源文件的 triple 后缀。包内生产运行仍是 `Contents/MacOS/towermd` sibling，Rust `Command` 固定 argv，不退回 shell、PATH 或源码目录。

- [ ] **实现 App/DMG 调用（2–5 分钟）：** P6 `build-app` 新增明确 `--config src-tauri/tauri.unsigned.conf.json --bundles app,dmg --locked` 参数并逐项校验；实调用 Tauri 以下 argv。先运行 `node scripts/project-pnpm.mjs tauri build --help` 确认锁定版本支持 `--` cargo 透传，然后保留这条命令的输出；若 wrapper 已支持等价 locked 方式，复用而不叠加第二套 build。

```bash
node scripts/project-pnpm.mjs tauri build --target aarch64-apple-darwin --bundles app,dmg --config src-tauri/tauri.unsigned.conf.json -- --locked
node scripts/project-pnpm.mjs tauri build --target x86_64-apple-darwin --bundles app,dmg --config src-tauri/tauri.unsigned.conf.json -- --locked
```

每个调用使用 `unsignedEnv` 的返回值，`.tools/pnpm/node_modules/.bin` 为显式 PATH 首项，cargo 用 `~/.cargo/bin/cargo`。不 `source` 任意 env、不读取 Note `.env.release`、私钥文件或钥匙串身份，不执行 `security find-identity` / notarytool / stapler / tauri signer，不带 updater 产物。必要安装只在 Task 7 发生；这里仅写/测编排。

- [ ] **实现结果检查（2–5 分钟）：** 构建前记录 tag commit、工具版本/两锁 hash，构建后重查 HEAD/版本/受控源码 hash 和 `git status` 不变。依实际目录唯一查找 `.app`/`.dmg`，零个或多个拒绝，不拿上一次残留包；每 target 用独立 worktree/target 目录。stdout/stderr 分别写 output 外日志，不上传完整构建环境。App 名称以 P6 实际 `productName` 和生成结果为准，验收确认是 `Tower.md.app` 后固化断言。
- [ ] **绿测与精确提交（2–5 分钟）：** `node --test scripts/release/build.test.mjs`，`node scripts/project-pnpm.mjs test -- scripts/stage-cli.test.ts scripts/build-app.test.ts` PASS；只暂存本任务脚本/config/对应测试，提交 `feat(release): build locked per-architecture unsigned bundles`，记录真实提交号。

## Task 5：检查真实包，组装原生技能和不可循环校验清单

**Files:** Create `scripts/release/assets.mjs`、`scripts/release/assets.test.mjs`。

- [ ] **写红测试（2–5 分钟）：** fake runner 的 App fixture 缺 `Contents/MacOS/towermd`、主程序为错误架构、CLI 与 App 架构不同、Info.plist 旧版、缺 dist、技能归档含非 tag 新文件/绝对路径/`..`/逃逸 symlink 时均拒绝。缺两架构任一 DMG、资产多 `latest.json`/`.sig`、SHA256SUMS 被改，均拒绝。
- [ ] **运行红测试（2 分钟）：** 提供可载入的最小 stub 后运行 `node --test scripts/release/assets.test.mjs`；预期缺 CLI/错架构/坏 checksum 未被拒绝的行为断言 FAIL。import/编译失败不记红、stub 不提交。
- [ ] **实现 App 静态检查（2–5 分钟）：** `inspectApp` 运行下列命令并解析结构化结果；单架构 `lipo -archs` 必须精确为 `arm64` 或 `x86_64`，不要误把 Rust triple 的 aarch64 当 Mach-O 名称。主程序和 CLI 均检查普通可执行文件、版本及动态库；`otool` 只允许系统路径、包内 `@rpath`/`@executable_path`/`@loader_path`，发现开发机 Node/Rust/自建绝对 dylib 依赖拒绝。WebView 内嵌前端可能在主二进制而不作为可见 dist 目录，按 P6 production 构建真实布局核验，不能硬要求虚构 `Contents/Resources/dist`。

```bash
plutil -extract CFBundleShortVersionString raw -o - '/absolute/Tower.md.app/Contents/Info.plist'
plutil -extract CFBundleExecutable raw -o - '/absolute/Tower.md.app/Contents/Info.plist'
lipo -archs '/absolute/Tower.md.app/Contents/MacOS/towermd-app'
lipo -archs '/absolute/Tower.md.app/Contents/MacOS/towermd'
otool -L '/absolute/Tower.md.app/Contents/MacOS/towermd-app'
otool -L '/absolute/Tower.md.app/Contents/MacOS/towermd'
codesign -dv --verbose=4 '/absolute/Tower.md.app'
codesign --verify --deep --strict '/absolute/Tower.md.app'
spctl --assess --type execute --verbose=4 '/absolute/Tower.md.app'
```

在本机目标运行包内 `towermd --version` 应为 `towermd 0.1.0`，Intel 没有运行环境时不执行其二进制并称通过；由 cargo metadata、target CLI hash、Mach-O 与源码关联提供静态版本证据。`codesign` 应显示 ad-hoc/无 TeamIdentifier、无 Developer ID Authority；`spctl` 未通过在本发布模式中如实记录，不把它转换成公证成功；真正缺必需 ad-hoc 导致不能运行须修正构建而不能忽略。

- [ ] **实现 DMG 检查（2–5 分钟）：** `hdiutil verify <dmg>`；`hdiutil attach -readonly -nobrowse -plist <dmg>` 从 plist 读此次挂载点；在该点核唯一 App，递归比较可执行、Info.plist、资源的 sha256/权限/symlink 与本 target App，一致才通过。finally 仅 detach 此次挂载的设备；不扫描并强制卸载用户 `/Volumes/dmg.*`。检查失败保留日志，不上传该包。
- [ ] **实现技能冻结归档（2–5 分钟）：** 用 `git ls-tree -r --name-only <tag> skills` 读取 P7 实际目录；对照 P7 安装说明验证 Claude Code/Codex 的 init、baseline、claim、ask、propose、finish 六项各有真实 `SKILL.md` 和引用资源，安装路径以已验收 P7 为准。`seed` 不交付可执行入口；技能只能经 CLI。调用 `git archive --format=tar --prefix=towermd-skills-0.1.0/ --output <temporary.tar> v0.1.0 skills`，再以 `gzip -n` 压缩；命令用 argv/文件流，不用 shell 重定向。解包后与 tag 每个 blob hash/模式逐项比，不混入工作区技能。P7 若有已验证的等价原生包入口直接复用，仍必须证明输入 tag。
- [ ] **实现 App 归档与资产清单（2–5 分钟）：** `ditto -c -k --sequesterRsrc --keepParent <app> <tower.md-0.1.0-ARCH.app.zip>`；DMG 拷贝并规范为 `tower.md-0.1.0-ARCH.dmg`。保留 `.app` 原目录作为本地验收产物，公开上传 ZIP 保持 bundle 结构。先构建 4 个包和技能，再生成 `BUILD-MANIFEST.json`，包含 `{version,tag,commit,targets,tools,lockfiles,artifacts}`；artifacts 只列这 5 个二进制资产的 basename/size/sha256/target，避免自引用。
- [ ] **实现双语发布说明与校验（2–5 分钟）：** 从冻结 tag 的双语 **v0.1.0 版本节**取正文，加安装/架构选择、内置 CLI、无需 Node/Rust、未签名/ad-hoc/未公证、没有自动更新、Intel 实际验收级别和技能安装入口；不从 git 提交流水账或当前 Unreleased 生成。然后 SHA256SUMS 按 basename 排序覆盖全部其他 **8 个资产**，不含自身；manifest 不引用 SHA256SUMS，notes 不嵌最终 checksum，避免任何内容 hash 循环。
- [ ] **绿测与精确提交（2–5 分钟）：** `node --test scripts/release/core.test.mjs scripts/release/assets.test.mjs` PASS；恶意 archive path 测试在 extract 前失败。提交 `feat(release): verify bundles and assemble frozen native skill assets`，只含本任务工具/测试。

## Task 6：P1–P7 收口、真实范围核对和版本提交

**Files:** Create `docs/verification/2026-10-17-release-readiness.md`；Modify 版本文件、双语 CHANGELOG、`tasks/todo.md` 中本次记录。此时还不创建 tag。

- [ ] **重新读取 P1–P7 证据（2–5 分钟）：** 收集每批实际提交号、对应验证路径，逐项核 main 实际源码；P6 基础只读 App 无批准按钮不能过关。真人恢复/批准、三个工作树/两 harness/十任务缺任一即记录阻塞，继续完成不依赖它的构建工具，不能冻结“可用首版”。
- [ ] **核用户可感知条目（2–5 分钟）：** 逐项建立 readiness 表格 `功能/日志条目 → 实现提交 → 验收证据 → 是否进入当前 main`；已有“扫描未执行 S5”“M0 无 App 批准”等未发布条目须按最终实现真实更新，不留下与 0.1.0 冲突的旧限制，也不把未实现 M2/M3 写进发布说明。双语同一功能各有对应条目。
- [ ] **执行全仓只读范围采集（2–5 分钟）：** 跑 Task 2 `scan`，将全部 worktree dirty/untracked、每个本地 branch 的 main 未合入提交、每条 stash 逐项裁决；cherry-pick/rewrite 同时看 patch-id 和实际功能。保留外部任务，冻结输入只取已验证提交的 main tree；在 readiness 记录 inventory digest/结论，不复制潜在敏感 stash 原文。
- [ ] **确认版本来源后准备（2–5 分钟）：** `node scripts/release/prepare.mjs check --root . --version 0.0.0 --phase prepare`，当前前置实际版本若变化先同步检查，不硬覆盖；再运行 Task 3 prepare 命令一次，日期传执行日。审 diff 仅项目版本/本次日志/锁自身本地 package 变化，不应新增依赖或更新第三方版本。
- [ ] **启动必要全量验证（2 分钟）：** 按下面命令分开启动；每个长运行完成后核退出码和日志，隔离输出只保本次必要证据。

```bash
~/.cargo/bin/cargo test --workspace --locked
~/.cargo/bin/cargo test --manifest-path src-tauri/Cargo.toml --locked
node scripts/project-pnpm.mjs test
node scripts/project-pnpm.mjs check
node scripts/project-pnpm.mjs build
node --test scripts/release/core.test.mjs scripts/release/audit.test.mjs scripts/release/prepare.test.mjs scripts/release/build.test.mjs scripts/release/assets.test.mjs scripts/release/publish.test.mjs
node scripts/release/prepare.mjs check --root . --version 0.1.0 --phase frozen
```

- [ ] **核验证结果（2–5 分钟）：** 全部 PASS 才提交；若此时 publish 测试尚未 Task 9 实现，必须先完成 Task 9 的纯测试/工具开发再回到此处，不能删测试命令凑通过。版本变化引入的实际 App 回归至少本机重建并启动验证；先前 P7 证明绑定的实现 tree 与版本 commit 差异必须只有审阅过的版本/日志/发布工具，功能变动要求重新验收受影响真实路径。
- [ ] **精确提交 release preparation（2 分钟）：** 先执行统一提交守卫；用 `git diff --name-only` 生成本批白名单，独占且全部 diff 属于本次的版本文件才显式 `git add -- Cargo.toml Cargo.lock package.json pnpm-lock.yaml src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json CHANGELOG.md CHANGELOG.zh-CN.md docs/verification/2026-10-17-release-readiness.md`。`git add -p -- tasks/todo.md` 仅暂存本次记录 hunk；版本/日志文件若混有其他任务行，同样改为按 hunk 暂存或独立工作树。逐行核完整 `git diff --cached --binary` 的路径和内容，再 `git diff --cached --check`；不能只看 stat。只有本任务补丁才 `git commit -m "chore(release): prepare first unsigned app 0.1.0"`，记录提交为 R。
- [ ] **核提交和最终审计（2–5 分钟）：** `git show --stat --oneline HEAD`、`git status --short`、Task 2 重采集和 scope check；R 的源码/版本/日志与通过验证的受控内容一致。ready 文档记录冻结前已有证据，不填写包含自身提交号的循环字段；R 的 SHA 在外部冻结收据中登记。

## Task 7：冻结 tag 和同源双架构构建

**Files:** 不改受控源码；生成临时 worktree、`build-receipt.json`、构建日志和初步资产。输入 tag 不可重写。

- [ ] **临近发布重核通道（2–5 分钟）：** `git remote -v` 确认真实 SSH origin；`git ls-remote origin refs/heads/main`、`git ls-remote origin refs/tags/v0.1.0`；`gh auth status`。只判断已有默认账号/权限，不打印 token、不读取密钥。之前 github.com DNS 失败/default token 无效不等于无 SSH 权限；此时分别记录 DNS、SSH Git、gh API 通道的实际结果。不可用则本地构建继续，公开发布保持未完成。
- [ ] **冻结前检查（2–5 分钟）：** 当前 HEAD 为已验证 R，版本和双语 frozen 门禁 PASS；远端/本地 tag 均不存在，或已存在且严格等于 R（重试）。本地 branch/main 和 origin/main 差异逐项确认：不能把本次之外的未发布提交偷推。已有用户发布授权覆盖本次经审阅的版本提交/tag，若 SSH 实际拒绝则报告，不自动改 transport/账号。
- [ ] **创建冻结 tag（2 分钟）：** 用下列命令创建不依赖 GPG 私钥的 annotated tag；`-c` 仅本次命令，不改用户全局 Git 配置。创建后立即保存 commit/tree/锁 hash 到输出目录的收据，tag 之后不 amend/rebase/force-tag。

```bash
git -c tag.gpgSign=false tag -a v0.1.0 -m 'Tower.md 0.1.0 unsigned macOS release'
git rev-parse 'v0.1.0^{commit}'
git rev-parse 'v0.1.0^{tree}'
```

无自引用顺序：先在 R 内提交版本、日志、构建脚本和冻结前 readiness → 标 v0.1.0 → 从 tag 构建 → 收据/manifest 写 R → 实机/下载证据写后续独立提交 E。E 不进入/替换 tag；不为把新 SHA 填进源码而再提交 R+1。发现功能/构建工具问题必须修复、重验、重新定正式冻结方案；**已推/已公开 tag 永远不改**，改版用新版本，不能把 v0.1.0 的另一套字节静默顶上去。

- [ ] **准备两个干净构建工作树（2–5 分钟）：** 下面变量只存本次临时目录，后续命令在同一发布 shell 使用；恢复会话时从本次收据读取已有绝对路径，不能重新生成目录冒充同一构建。用 Git argv 传递，不写入原用户工作树。

```bash
TOWER_RELEASE_DIR="$(mktemp -d /private/tmp/towermd-release-v0.1.0-XXXXXX)"
git worktree add --detach "$TOWER_RELEASE_DIR/arm64" v0.1.0
git worktree add --detach "$TOWER_RELEASE_DIR/intel" v0.1.0
```

两个 worktree 的 `git rev-parse HEAD` 均为 R；只有构建产物忽略，任何受控源码变动拒绝。这里新建 detached 工作树后重新审计并标记为本次发布生成的干净输入，不使 Task 2 旧 inventory 静默失效。

- [ ] **就用到的工具作准备（2–5 分钟）：** 每个 worktree 以 `node scripts/project-pnpm.mjs --version` 验证 11.7.0，项目安装优先 `install --frozen-lockfile --offline`，cache 缺失再必要联网；启动器安装到 `.tools/pnpm/node_modules/pnpm/bin/pnpm.cjs`，把相邻 `.bin` 加入本次 PATH。`~/.cargo/bin/rustup target list --installed` 当前仅 arm；此时才 `~/.cargo/bin/rustup target add x86_64-apple-darwin`。不修改全局 pnpm/corepack/harness/Git 配置，不假称未安装 target 的构建已通过。
- [ ] **启动 ARM 构建（2 分钟）：** 在 arm64 worktree 执行 `node scripts/release/build.mjs --root . --tag v0.1.0 --target aarch64-apple-darwin --output "$TOWER_RELEASE_DIR/arm64-output"`；保存工具版本、SDK/Xcode、Node/pnpm/rustc/cargo/Tauri、锁 hash 和固定 argv；等待期间定期报告实际状态。
- [ ] **检查 ARM 结果（2–5 分钟）：** Task 5 inspect/App/DMG PASS；tag/R/hash/本机 CLI `--version` 一致；不靠文件名推断 arch。失败保留当前目录，修根因后按冻结规则处理，不拿旧包补缺。
- [ ] **启动 Intel 构建（2 分钟）：** 在 intel worktree 执行相同命令但 target=`x86_64-apple-darwin`，output=`.../intel-output`。不并用同一 binaries/target 目录，不依赖 Rosetta 提供构建目标。
- [ ] **检查 Intel 结果（2–5 分钟）：** Task 5 Mach-O/CLI/版本/DMG 静态检查 PASS；保存“已构建+静态核验”。只有真的在 Intel Mac 上执行才记录“Intel 实机通过”；Rosetta 执行单列，不能替代 Intel 设备证据。无 Intel 设备时发布说明明确未实机验证，不能用 ARM 结果填 Intel 一栏。
- [ ] **组装九项资产（2–5 分钟）：** `node scripts/release/assets.mjs --root "$TOWER_RELEASE_DIR/arm64" --tag v0.1.0 --arm-build "$TOWER_RELEASE_DIR/arm64-output" --intel-build "$TOWER_RELEASE_DIR/intel-output" --output "$TOWER_RELEASE_DIR/assets"`。核两个收据同 R/同锁、技能同 tag；`shasum -a 256 -c SHA256SUMS` 在 assets cwd 全部 OK；根资产目录恰为 expectedAssets 9 项，logs/receipts/临时 App 不在上传目录。

## Task 8：从冻结包进行 Finder 和真人验收

**Files:** 仅输出验收截图/日志/收据到本次临时目录，Task 10 再引用并沉淀报告。不要改 tag 源码。

- [ ] **准备源码外安装入口（2–5 分钟）：** 从 ARM DMG 只读挂载取 App，用 `ditto` 复制到 `"$TOWER_RELEASE_DIR/installed/Tower.md.app"`，核包内 hash 等于组装输入；停止 Vite，不使用 dev server。打开 Finder 的 installed 目录，双击真实 App；UI 自动化可以辅助截图和回归，不伪造人的批准。
- [ ] **验证无工具链依赖（2–5 分钟）：** 从最小环境以包内主程序补测，命令只保系统 PATH 和真实 HOME/TMPDIR，不提供 Node/pnpm/Rust 路径。前端正常显示项目入口、静态资源无需 localhost:1420；同时确认 `Contents/MacOS/towermd` 固定定位。本机真实 Finder 成功与最小 PATH 补测分别记录。

```bash
env -i HOME="$HOME" TMPDIR="$TMPDIR" PATH=/usr/bin:/bin:/usr/sbin:/sbin "$TOWER_RELEASE_DIR/installed/Tower.md.app/Contents/MacOS/towermd-app"
```

目标仓库验证工具是仓库自己的要求：选一项需要非系统工具的验证时应显示缺失工具和可操作诊断，不能把产品无需工具链误写成所有项目验证无需工具链，不能自动改用户 shell 配置。

- [ ] **核真人入口（2–5 分钟）：** 展示已构建 App 的准确路径、验收项目目录，以及“恢复确认卡片 → 显示目标项目/状态变化 → 确认后恢复”和“草稿审阅 → hash/目标基线 → 批准后产生 B0”具体入口。此时才请用户执行所需真人验收；等待真实回复/动作，不能超时自动通过。P7 已有真人证据若对应同功能 tree 且安装包能追溯，记录复用范围；安装入口与冻结包仍实际验证。
- [ ] **检查闭环（2–5 分钟）：** 从冻结包显示当前 main、Z0–Z2、Evidence/日志、问答、禁止/门禁失败、草稿批准拒绝；快层变更前后节点坐标相同，首屏最多 3 项，中断有形状/色/文字，费用 null 为未知。任何未接入 API 的按钮/只读壳阻止“首个可用”验收，不把 fixture 状态当产品数据。
- [ ] **核窗口关闭/退出（2–5 分钟）：** 在隔离验收项目记录包内 CLI `daemon status` 的 pid/instance；关闭全部窗口并退出 App，再用包内 CLI `status`/`daemon status` 查询同 daemon，已有运行保持；重开 App 正常恢复。CLI 的当前实际 `--directory` 放置以 `--help` 为准，不发明 `daemon start`；测试只清理身份匹配的夹具 daemon，不停用户 daemon。
- [ ] **核 P7 真实性（2–5 分钟）：** 冻结 main 对十任务证据引用仍存在；三工作树、两个 harness 版本、每任务 Run/tree/Evidence/main trailer/合并验证完整，CLI 无批准和 human 入口，无证据 done 不计进度。缺失就阻止发布完成声明，不能追加十条模拟日志凑数。
- [ ] **确认归档仍同字节（2–5 分钟）：** 实机检查只复制/读取 App，不修改待上传资产；再验 SHA256SUMS。若操作系统允许打开动作只改 installed 副本属性，记副本与原包内容 hash，不回灌原包。只有真实通过的架构/操作标为通过，其他明确静态/未实机/未真人；本机失败必须解决。
- [ ] **汇总真实验收收据（2–5 分钟）：** 全部必需实际验收完成后，Root 按下节将真实材料副本及 hash 放到 `"$TOWER_RELEASE_DIR/assets.logs/acceptance-evidence/"`，复核 Task 6 readiness、P7 十任务/合法 App 人工事件、冻结安装包和架构实际级别，再写 `"$TOWER_RELEASE_DIR/assets.logs/acceptance.json"`。未知时间/标识不得预填；失败、未真人或缺材料只保留失败日志，不生成完整收据。收据由过程记录，不代用户点击、不伪造 human 事件；不回写 tag/R、manifest、SUMS 或双语说明。先完整写临时文件、核字段/hash和原始事实，再以不覆盖已有最终文件的方式落盘；纠错先保留旧版及更正缘由，再显式复核新一轮材料，不自动修补。

### 本地验收收据契约

这是 Task 8 已要求的本地外部收据的消费格式；不新增产品对象、批准机制、依赖、签名证明、CLI 参数或公开资产。`publishFrozen({repo,tag,output,download}, run)` 固定读取 `join(output + '.logs', 'acceptance.json')`，材料位于同一旁车 `acceptance-evidence/`。收据只在 Task 8 真实完成后产生，位于冻结 R/tag 之外；发布工具可在 R 前以明确 synthetic 夹具完成 TDD，但夹具不计真实验收。

以下是字段类型说明，尖括号和省略号不可作为实际值写入；不设 approved/ready/trusted/P7passed 等放行开关：

```js
{
  format: 'towermd-release-acceptance-v1', recordedAt: '<实际汇总时间 ISO8601>',
  recordedBy: 'process:towermd-release',
  identity: {tag, tagObject, commit, tree},
  assets: [{name, size, sha256}, /* 精确九项，含 SHA256SUMS 自身 */],
  readiness: {path:'docs/verification/2026-10-17-release-readiness.md',
              blob:'<tag 内实际 Git blob OID>', sha256, review:'<材料id>'},
  materials: {'<id>': {path:'acceptance-evidence/<相对普通文件路径>', sha256}},
  p7: {
    input:'<P7 input.json材料id>', reuse:'<P7 Task8.6实际复用复核材料id>', // 源与R有差异时reuse必需
    tasks:[{card:'T01' /* 至 T10 */, taskId:'<实际产品 TaskCardID>',
            harnessClass:'claudeCode' /* 或codex，仅验收类别 */,
            task:'<task.json材料id>', record:'<record.json材料id>', session:'<session.json材料id>',
            git:'<git.json材料id>',
            evidence:'<权威Evidence原文材料id>', proof:'<canonical proof/merge.done材料id>',
            trailers:'<CLI trailers材料id>', mainMessage:'<实际main消息材料id>',
            mainTests:'<合并后验证材料id>',
            bindings:{
              capturedSource:{sourceRef,material}, // record.verification.captured_source_ref
              proof:{sourceRef,material},          // record.main.proof_ref
              mainTests:{sourceRef,material},      // record.main.test_log_ref
              transcript:{sourceRef,material},     // record.session.transcript_ref
              verificationLog:{sourceRef,material},// record.verification.log_ref
              human:[{sourceRef,material}],        // record.human_refs，保原顺序/数量
              checkpoints:[{sourceRef,material}],  // record.checkpoint_refs，保原顺序/数量
              failures:[{sourceRef,material}]      // record.failure_refs，保原顺序/数量
            }}]
  },
  architectures:[{target:'aarch64-apple-darwin',level:'device',refs:['<材料id>']},
                 {target:'x86_64-apple-darwin',level:'static',refs:['<材料id>']}],
  checks:{
    installation:{sourceAsset:'tower.md-<version>-aarch64.dmg',installedTreeSha256,refs:['<材料id>']},
    finderLaunch:{refs:['<材料id>']}, minimalEnvironment:{refs:['<材料id>']},
    runtimeClosure:{refs:['<材料id>']},
    daemonLifetime:{before:{pid,instance},after:{pid,instance},refs:['<材料id>']},
    reopen:{refs:['<材料id>']}, archiveIntegrity:{refs:['<材料id>']},
    humanRecovery:{eventId,actor:'human:<实际id>',implementationTree,
                   event:'<事件原文id>',binding:'<归档binding id>',
                   implementation:'<实现tree来源id>',refs:['<观察材料id>']},
    humanBaselineApproval:{eventId,actor:'human:<实际id>',implementationTree,
                           event:'<事件原文id>',binding:'<归档binding id>',
                           implementation:'<实现tree来源id>',refs:['<观察材料id>']}
  }
}
```

机器校验与真实生成责任如下；缺任一必需事实/引用/实际材料，均具体报字段或材料 id 并拒绝，不能只看 JSON 存在或自报成功：

- **冻结绑定：** identity 同本地 tag object A/peeled R/tree 及 manifest；九项 name/size/hash 同当前资产原字节，八行 SUMS 用现有 core 核，SUMS 自身另比 size/hash。readiness 从 tag 固定路径取 blob OID/原始 bytes 核 SHA256，不读工作树替代件；review 引用 Task 6 对 `功能/双语日志 → 实现提交 → 验收证据 → 冻结 main`、P1–P7、范围裁决/inventory digest 的真实复核材料。Root 核这些对应的语义与实际进入 R，工具核原文身份、引用覆盖和完整性。
- **P7 单一来源与职责：** 沿 `2026-10-17-m1-native-skills-acceptance.md` 第 5 节真实 `record.json` 和固定角色原文核对，不把上面的摘要另当业务 proof。`card` 是 T01–T10 的验收卡标签，**不是产品 TaskCardID**；taskId、Run、Evidence 按最终实际 task.json、record.binding.run、record.verification.evidence_id 等字段交叉核，不另造标识正则或从 T01 生成产品 ID。Root 按 P7 Task7.11、第5节与Task8.6逐条核真实 Run→tree→Evidence→五 trailers/实际权威 main→canonical `merge.done`→Task merged/Run ended→合并后验证及专项/依赖事实，并在真实复核材料中保留来源、差异与复用依据。发布器只按 P7 最终实际 record/task/session 固定字段提取并交叉核对 task/run/evidence 唯一性、三工作树、原表规定的两 harness 各五卡及各卡实际版本、tree/main 身份、必需原文角色覆盖和材料 bytes/hash；同时核 record 中 main.task_state=merged/run_state=ended、test_count>0/test_exit=0 等已定义事实，与相应原文身份一致，不能只靠 result=pass 或字段非空代替 Root 真实语义核验。它不调用 collect 重采集，不调用或新增 Git-Evidence 证明器/业务 proof 消费 API，不重新作产品完成状态判定。实际 P7 字段映射或所需原文材料尚未交付时，明确报告未接线字段/缺失材料并拒绝生产；不得猜字段、用 returnOk 占位或以 synthetic 夹具顶替。
- **固定引用绑定：** p7.input、每卡 task/record/session/git/evidence/proof/trailers/mainMessage/mainTests 均为必需材料角色；input 与各 record.source 按 P7 实际字段核来源身份。bindings 只覆盖上面列明的固定 record 字段，不泛遍历任意 JSON 找 ref。每个 sourceRef 必须与对应原字段字面值逐字相同、material 命中 materials；capturedSource/proof/mainTests 的 material 分别等同该卡已有 evidence/proof/mainTests id。原 human_refs/checkpoint_refs/failure_refs 的每个条目按原顺序/数量恰有绑定，缺项/多重矛盾/额外绑定拒绝；原可选空列表允许空，不能造失败。record.verification.object_ref 可 null，非 null 的 Git path/ref 仅保留来源文字，不当本地文件路径，不因此要求不适用的快照字段；权威 captured_source_ref 仍必需。原 record/session/Evidence bytes 不重写，原路径即使绝对也只作 sourceRef 文字比较；实际读取只经 materials 的受控相对副本路径，不追随原路径/执行命令。记录原文内部未定型 Evidence/proof 字段的业务含义仍由 Root 按实际来源核，发布器只核已接线的固定身份映射，不增加通用 proof decoder。
- **P7 全体冻结复用：** p7.input 与每卡 record.source 指向的验收候选来源及相关 CLI/core/daemon/App/skills/schema/锁 blob，相对于 R 有差异时，p7.reuse 必须指向 Root 按 P7 Task8.6 实际完成的复用复核材料，记录候选→R 的身份/相关 blob diff/hash、包追溯和必要重跑；不只检查两个 human 项的 tree。工具核来源身份、覆盖与材料 hash，不自行判定功能等价；实质变化须 Root 重跑受影响真实验收，缺材料拒绝。
- **harness/actor 映射：** 每卡 harnessClass 的 claudeCode/codex 只是 P7 已约定的 Claude Code/Codex 两验收类别；机器以每卡实际 session.harness/version 和 Run self_reported 为准，版本非空，同类别不同卡可用不同真实版本，不设顶层单一版本/会话。不得用类别名替换原 harness 字符串或新增产品 enum；声明不升级为可信身份，Root 核真实会话/加载。CLI 事件 actor 仍为 `process:towermd-cli`，Agent actor 沿实际 `<producer>/<version>`。human 两项的 event、binding、implementation 三个固定材料角色都必需：eventId/actor 从实际事件原文摘录并与归档 binding 的对应事件/对象绑定交叉核，不以仅有截图或 human 字样放行；implementationTree 从 P7 输入/真实包来源绑定材料取，不假设产品 Event 自带 implementationTree。Root 核合法 App producer 与真实人操作，不能代操作或为收据伪造 human 事件，记录者过程身份不变。
- **Task 8 实际观察：** 两 target 唯一，level 仅 static/rosetta/device；ARM 必须有本步骤的真实设备验收，Intel 按实际级别，Rosetta不冒称Intel设备。九个 checks 固定键、每项 refs 非空；installation.sourceAsset 仅本次冻结 ARM DMG，沿本 Task 的安装步骤，不把 Task 5 用户可选 ZIP 安装说明扩成验收替代入口。installedTreeSha256 绑定源包与 installed 副本内容对比材料；daemon before/after 必须同合法 pid 和 instance。runtimeClosure 引用本 Task 的真实 API/数据闭环、拒绝路径、坐标/首屏/中断/费用观察，不造第二套 UI 状态。human 两项须不同实际事件，actor 不能是 process/Agent；复用 P7 真人动作且 implementationTree 与 R.tree 不同时，该项额外 `reuse:'<材料id>'` 绑定上述全体 P7 复用复核中的相应实施 tree/包追溯与必要重跑事实；有实质变化重新真实验收，材料缺失拒绝。
- **材料安全与限度：** 每个 refs/review/reuse/支持材料 id 都要命中 materials；普通文件及各父目录无 symlink，路径仅在这次 acceptance-evidence 内，拒绝绝对路径、`.`/`..`、控制字符、外逃/缺文件/错 hash。记录中的原始任意路径只作为待核文字，不访问用户目录、认证配置或环境秘密。Root 保证材料来自真实会话/运行/人的动作，逐项核业务事实；工具强制身份、覆盖和 bytes 完整性。hash/格式不证明真人点击或日志未编造，不能宣称密码学证明，也不能据此删除缺项硬门。

**强制 provenance 附件：** Task 9 同时只读 `${output}.logs/inputs-before.json`、`inputs-after.json`、`commands.jsonl`，沿 Task 5 真实 `{tag,commit,tree,tagObject,sources,builds}` 与 Task 4 build receipt schema。前后身份/sources/两 builds 一致，builds 的两 target、sources、三 lockfiles、tools 与 manifest 对应；commands 的 file/args/status/failed/输出 hash/实际时间须完整，全部 failed=false。Task 4 build.commands 须 status=0；Task 5 组装 commands 仅允许 inspectApp 已解析的 spctl status=3/rejected 或 status=0/accepted，按 target 和对应 app argv 关联到 `manifest.targets[].gatekeeper` 的 status/result，其余命令须 status=0；不把预期 Gatekeeper 拒绝改称公证通过，也不接受任意非零。保留已有构建/组装链的关键 argv 顺序，工具只核固定字段/身份/命令序与旁车 bytes 完整性。现有 stdout.log/stderr.log 是聚合输出，没有逐命令边界，不能从聚合日志重算每条摘要，也不能从 stdout/stderr 两个 digest 重算 gatekeeper 合并输出 hash；不另造分片 receipt，Root 仍须核日志真实性。附件原 bytes/hash 是追溯材料，不凭字符串自称执行成功，不是真人凭证。字段不符/缺失拒绝，不合成新 receipt、不 import 私有 checkReceipt、不复制整套 App/DMG/归档验证器、不重新执行日志命令。资产两 notes 沿 Task 5 从冻结日志生成的原字节单一来源，认证 SUMS/来源绑定后只按 Task 9 既定公式合并正文，不复制内联 install 模板。

**先后次序与失败：** 发布器不得生成/修补验收收据。任何 push/create/upload/publish 前完整核本节；临 `--draft=false` 前重读 receipt/所有引用材料 hash、provenance/九资产/readiness/tag A/R，变化即拒绝，不能凭第一次快照放行。draft/public 重跑同样检查，未知格式/缺项/路径或 hash 错误非零，不按时钟放行；仅报受控字段/id，不输出材料/环境秘密。工具在 R 前提交，Task 6–8 实际材料后产生，不向 R 填未来“通过”。

## Task 9：可重试上传、公开发布和匿名回读

**Files:** Create `scripts/release/publish.mjs`、`scripts/release/publish.test.mjs`；Modify `scripts/release/core.mjs` 仅新增共用远端门禁。本任务工具/TDD 应在 Task 6 冻结前完成并提交；本任务实际网络发布留到 Task 8 通过后执行。

- [ ] **写红测试（2–5 分钟）：** fake gh/curl runner 覆盖：remote tag != R、draft 缺一架构、重名异 hash、只有一半上传、API 失败、私有/404 匿名 URL、download 被改一字节、release 已 public 后 asset 不符。另写两个独立拒绝案例：①九项正确资产/bytes 再加 `latest.json` 或 `.sig`；②恰好九项且所有 bytes 正确，但远端 body 不是冻结双语说明的完整预期正文；两者都不得调用 `release edit --draft=false`，也不得自动删资产/覆盖正文。模拟第一次读正确、发布前重读新增额外 asset 或 body 被改，亦须拒绝。验证失败不发布、不 `--clobber`、不重打 tag；同名同 bytes 可复用；网络中断重试只上传缺项。fixture 不使用真实账号/token。

```js
// scripts/release/publish.test.mjs 的纯门禁用例，另用 fake runner 断言拒绝时没有 publish。
import test from 'node:test';
import assert from 'node:assert/strict';
import { assertRemoteReady, expectedAssets } from './core.mjs';
const tag = 'v0.1.0';
const expectedBody = 'English frozen notes.\n\n---\n\n冻结中文说明。\n';
const correctRemote = {tagName:tag, body:expectedBody,
  assets:expectedAssets('0.1.0').map(name => ({name}))};
test('九项正确资产再多一项也不能发布', () => {
  assert.doesNotThrow(() => assertRemoteReady(correctRemote,tag,expectedBody));
  for (const name of ['latest.json','old-updater.sig'])
    assert.throws(() => assertRemoteReady({...correctRemote,
      assets:[...correctRemote.assets,{name}]},tag,expectedBody), /asset set/);
});
test('九项字节已验证但正文错误也不能发布', () => {
  assert.throws(() => assertRemoteReady({...correctRemote,
    body:'Another task release notes.\n'},tag,expectedBody), /release body/);
});
```

- [ ] **运行红测试（2 分钟）：** 提供可载入且无真实网络副作用的最小 stub 后运行 `node --test scripts/release/publish.test.mjs`；预期 tag 冲突/同名异 bytes/匿名 404 被错误接受或错误发送 publish 的行为断言 FAIL。import/编译失败不记红、stub 不提交。
- [ ] **实现可重复远端门禁（2–5 分钟）：** 将下面纯函数加入 `core.mjs`；测试先以无操作可载入 stub 观察上面两个行为断言真实失败，再替换为实现。`publishFrozen` 在上传后和临发布前的每次重读均调用同一函数，不各写一套宽松判断；与 tag 的 peeled commit/资产下载 hash 校验并用。

```js
export function assertRemoteReady(remote, tag, expectedBody) {
  if (remote.tagName !== tag) throw new Error('remote tag mismatch');
  assertAssetSet(remote.assets.map(asset => asset.name), tag.slice(1));
  if (remote.body !== expectedBody) throw new Error('release body mismatch');
}
```

- [ ] **实现本地与 tag 前置检查（2–5 分钟）：** `publishFrozen` 先验证 asset set、SHA256SUMS、manifest R/version/target，并完整执行“本地验收收据契约”的固定 acceptance.json、材料/readiness 和强制 provenance 检查；缺真实十任务/真人材料具体报缺，任何远端 mutation 前拒绝。再用固定 argv 校对本地 tag object/peeled commit、远端 tag object/peeled commit。需要推送时仅推经核对的 main 快进和精确 tag，不推所有分支/所有 tag、不 force。

```bash
git push origin main
git push origin refs/tags/v0.1.0
git ls-remote origin refs/tags/v0.1.0 'refs/tags/v0.1.0^{}'
```

如果 main 因其他任务已前进，先重新枚举/核对本次之外提交，不把该命令当推所有未发布工作的许可。公开 release 使用精确 tag，不使用 `latest` 自动寻址。

- [ ] **实现 draft 与逐资产重试（2–5 分钟）：** repo 参数来自实际 `git remote get-url origin` 解析并核实，禁止默认到 Note.md。先 `gh release view v0.1.0 --repo <实际owner/repo> --json tagName,isDraft,assets,url,body`；404 才创建 draft。notes 主体由已校验的冻结两份说明生成唯一 `expectedBody`，完整字符串确定为 `englishNotes.trimEnd() + '\n\n---\n\n' + chineseNotes.trimEnd() + '\n'`；先用 Node 原样写 body 文件再 `--notes-file`，不在 shell 插值多行。读取远端 `body` 后与该完整正文逐字符比较，不只查包含版本/标题；已有 draft 的 body 不符立即拒绝并报告正文冲突，不自动覆盖别的任务正文。远端现存资产允许尚缺本次九项中的部分，但任何不属于 `expectedAssets` 的额外名称或重复名称立即拒绝，不能自动删除。

```js
// publishFrozen 的 repo/tag/output 来自已校验参数，bodyFile 由 Node 写入完整双语正文。
run('gh', ['release','create',tag,'--repo',repo,'--verify-tag','--draft',
  '--title',`Tower.md ${tag.slice(1)}`,'--notes-file',bodyFile]);
for (const name of expectedAssets(tag.slice(1))) {
  if (actions.get(name) === 'upload')
    run('gh', ['release','upload',tag,join(output,name),'--repo',repo]);
}
```

上面 `actions` 是逐资产下载比较后由 `uploadAction` 得到的 Map，`join` 从 `node:path` 导入；repo 从已核实 remote 解析，不硬写虚构仓库。对每个已存在远端资产，先经 `gh release download --pattern <精确basename>` 下载到新临时目录、算实际 hash/size；`uploadAction` 同 bytes 复用，异 bytes 停止。禁止 `--clobber` 和自动删资产；未完成 draft 保留可重试。若本次已冻结本地资产意外改变，先核 sum/manifest，不能改 checksum 掩盖。

- [ ] **实现 publish 门禁（2–5 分钟）：** 上传重试结束后重新执行包含 `body` 的 `release view`，对远端**完整名称集合**调用 `assertAssetSet(remote.assets.map(a => a.name), version)`，要求精确九项，拒绝额外 `latest.json`/`.sig`、重复或缺失；逐一认证下载/size/hash、tag==R，并要求 `remote.body === expectedBody` 完整匹配冻结双语说明。临 `--draft=false` 前再次读取 `tagName,isDraft,assets,url,body`，重新校验 tag object/peeled commit、精确集合、完整 body，并核资产 id/size 与刚验证下载的记录未变；同时重核“本地验收收据契约”的 receipt/材料/provenance/九 bytes/readiness 未变。发现变化重新核下载/拒绝，不能用旧快照放行。全部通过才 `run('gh', ['release','edit',tag,'--repo',repo,'--draft=false'])`。既有 draft 正文错或多资产直接报告具体冲突，不静默删除/覆盖；此前已公开且全部一致才进入匿名回读，公开后不一致不能静默混版本补丁。readiness 缺真实十任务/真人证据时返回缺项，不能按时钟放行。
- [ ] **验证验收缺项硬门（2–5 分钟）：** fake runner 覆盖缺 receipt、十卡少一/重复 Run/Evidence、少工作树/单 harness/空版本、缺 canonical proof/merge.done 原文、缺 Task merged/Run ended 原文或与 record 身份不符、固定 sourceRef/材料角色缺失或矛盾、缺 human/伪 process actor/全体 P7 来源有差异却缺复用材料、human 缺 event/binding/implementation 角色、readiness blob错、九 checks任缺、daemon instance变、provenance缺失/错源、材料缺失/错 hash/symlink/外逃、SUMS自身变、A变R不变，以及初读正确临publish材料漂移；另测同类别不同真实版本、可选空列表和 object_ref=null 不产生假失败。断言缺项时零 push/create/upload/publish、临publish变化时不 `--draft=false`。完整 synthetic 夹具只证明机器门禁，不登记真人/P7通过；不为通过测试添加 bypass/approved字段，不调用 collect 或业务 proof API。
- [ ] **实现公开匿名回读（2–5 分钟）：** 从 release API 的 `browser_download_url` 获取九项精确 URL，必须 https、同 owner/repo/tag/basename。用独立 curl 子进程不带 `Authorization`、`GH_TOKEN`、cookie，`--disable` 禁止用户 `.curlrc` 隐式注入；不依赖登录浏览器。重定向允许 HTTPS；输出到空目录防旧文件误判。

```js
// asset.url 已校验是当前 repo/tag/name 的 browser_download_url。
// download 是本次新建空目录，run 的 env 不继承认证变量。
run('curl', ['--disable','--fail','--location','--proto','=https',
  '--proto-redir','=https','--silent','--show-error','--output',
  join(download,asset.name),asset.url],
  {env:{PATH:'/usr/bin:/bin:/usr/sbin:/sbin',HOME:process.env.HOME,
    TMPDIR:process.env.TMPDIR}});
```

子进程只传系统 PATH/HOME/TMPDIR/locale，认证变量不透传；回读 bytes 与本地产物 size/hash 精确相同，SHA256SUMS 自身的 hash 也与本地 sum 文件比，之后在 public cwd 运行 `shasum -a 256 -c SHA256SUMS`。下载 SHA256SUMS 再用它自行验证不足以证明未混版，必须对比冻结本地 sum 字节与 manifest R。保存 URL、时间、size、SHA256、HTTP 结果及 release/tag 链接；不保存认证头。
- [ ] **绿测/精确提交工具（2–5 分钟）：** `node --test scripts/release/publish.test.mjs scripts/release/core.test.mjs` PASS，提交 `feat(release): publish and anonymously verify immutable assets`；该提交必须在 R 前。真正发布执行：`node scripts/release/publish.mjs --tag v0.1.0 --output "$TOWER_RELEASE_DIR/assets" --download "$TOWER_RELEASE_DIR/public"`；CLI 省略 `--repo` 时从当前 Git SSH origin 解析唯一 owner/repo，`--repo` 如显式传入必须与 origin 相同，保存退出码/收据。
- [ ] **核失败边界（2–5 分钟）：** DNS/SSH/gh/API/匿名下载任一仍不可用，可报告本地构建就绪或 draft 已上传，但保持“未公开发布”或“公开回读未完成”。不换账号、不读取 Note 秘密、不把私有登录下载称公开匿名可用；阻塞状态说清实际失败通道和已完成部分。

## Task 10：登记最终证据，不能靠任务打勾证明发布

**Files:** Create `docs/verification/releases/v0.1.0.md`（OKF `type: Verification`、sources 指向 tag/release/已存在验证记录）；Modify 本次 `tasks/todo.md` 回顾与总表 P8 真实证据栏。此任务是冻结后的独立证据提交 E。

- [ ] **写真实报告（2–5 分钟）：** 记录 R/tag/tree、各 P1–P7 实现提交、真实十任务/人工操作链接、工具和锁 hash、两架构检查结果、本机 Finder 截图/daemon 生命周期、9 资产/8 checksum、发布 URL、匿名回读 URL/hash/时间。Intel 若仅构建静态检查，表中写“未 Intel 实机验收”，不要空白或填 PASS。
- [ ] **登记验收材料绑定（2–5 分钟）：** 引用本次 `${output}.logs/acceptance.json` 的实际 hash、固定 readiness blob/hash、强制 provenance 附件与材料清单/hash；核 Root 按 P7 Task7.11/8.6 实际复核记录与报告一致。缺材料/未真人保持未完成，不把 JSON 自身或 synthetic 测试称真人证明。证据在 E 中引用和沉淀，不修改 R/tag 或增加公开第十项资产。
- [ ] **核冻结源码与声明（2–5 分钟）：** `git show v0.1.0:CHANGELOG.md`、中文对应节、`git show v0.1.0:docs/verification/2026-10-17-release-readiness.md` 与用户点名功能实际源码/测试逐项对应。P1–P7 被 cherry-pick/rewrite 的提交同时保存 patch 等价和实际功能证据；分支名、版本号或工作区测试不能证明在 tag 里。
- [ ] **核发布后仓库（2–5 分钟）：** 枚举全部 worktree/branch/stash，再次记录保留在原地的其他任务；确认 tag commit 仍为 R、public 资产与冻结本地 bytes 一致。不得用“已纳入下次发布”替代实际 tag 证据；证据只追加，不反改已发布 tag。
- [ ] **精确提交 E（2 分钟）：** 先执行统一提交守卫；新验证报告按显式路径暂存，`tasks/todo.md` 和总表只用 `git add -p --` 暂存本次事实 hunk。完整 cached 路径/内容审阅通过后，仅提交这些补丁：`git commit -m "docs(release): record 0.1.0 unsigned delivery evidence"`；报告正文不写 E 的自身 SHA，外部最终回复/任务后续登记该 SHA。不提交原 index 的其他任务内容、他人未验证改动、二进制、完整环境或敏感日志。
- [ ] **向用户交付（2–5 分钟）：** 提供公开 release 链接、Apple Silicon/Intel 对应 DMG、原生技能包与 SHA256SUMS 入口，说明无发行者签名/公证、实际 ad-hoc 结果、Intel 实测范围。只有匿名回读和必需功能/真人验收均成立才说“首个可用 App 已发布”；其他情形分别准确写“未提交/未完成/未实机/未真人/未公开”，附具体下一阻塞。

## 验收矩阵与自审

| 必需条件 | 负责 Task | 成立证据 |
| --- | --- | --- |
| P1–P7 实现/真实三工作树双 harness 十任务/真人 App 闸门 | 6、8 | 源码提交与真实 Run/tree/Evidence/main/真人记录 |
| 所有 worktree、branch 未合入与 stash 逐项范围；保留他人地盘 | 2、6、7、10 | 全量 inventory digest、精确裁决、patch 等价和实际功能 |
| 非空双语未发布门禁/版本序列/轮转后合理空区 | 1、3、6 | prepare/frozen 红绿测试与冻结版本正文 |
| Cargo/锁/前端/Tauri 同版，核心/桌面独立 workspace | 3、6 | metadata、锁、manifest 与测试 |
| 无 Note 秘密、发行者签名、公证/updater，ad-hoc 如实 | 1、4、5 | read/env trap 测试、有效配置、codesign/spctl 实际输出 |
| 同 R 双架构 App+CLI，源码外 Finder 启动，无 Node/Rust 依赖 | 4、5、7、8 | 两收据、Mach-O/otool、实际包/截图、最小 PATH |
| 退出 App 后 daemon 持续，人工闭环与稳定画布 | 8 | 同 pid/instance、真实操作与坐标比较 |
| 技能来自同 tag 原生目录，不含不存在 seed | 5、7 | git blob/归档逐项比较、P7 安装验收 |
| 九资产与八行 checksum，无 hash 自引用 | 1、5、7 | manifest/sum 测试与 shasum 全 OK |
| 失败可重试，tag 不重写/同名异 bytes 不覆盖；远端恰九资产且正文完全匹配 | 1、9 | fault injection、完整集合/双语 body 拒测、发布前重读、远端下载比较、精确 tag 校验 |
| 每次提交保护原 index，路径与共享 hunk 仅含本任务 | 全部提交 Task | 起始 index 检查/隔离分支、完整 cached diff、提交后实际补丁 |
| 公开匿名 bytes 等于冻结产物，不能用私有下载冒充 | 9、10 | 无认证 curl、HTTP 成功、size/hash、public sums |

实施前仍需以新事实解决的外部条件：P1–P7 尚未完成；Intel Rust target 尚未安装；Intel 设备是否可用未知；实际 SSH/DNS/gh 发布通道需临近发布复核；P6/P7 最终文件布局需开工时按已验证实现核对。本计划不把这些条件写成现有成果，也不为它们索取新的笼统授权。

计划文档验证仅检查结构、接口一致性、OKF 与范围覆盖；未运行任何本文预定 release 测试、构建、安装、tag 或网络发布。编写完毕应将该文件标注“计划已写，未实施/未提交”，具体执行已经获得用户持续开发授权，交接后按依赖推进，不再要求在两种执行方式中重新作笼统选择。
