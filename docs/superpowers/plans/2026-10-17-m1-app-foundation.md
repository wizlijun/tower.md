---
type: Plan
title: M1 App 基础可运行子项目实施计划
description: 建立实际可启动的 macOS Tauri App，连接唯一 daemon，显示权威画布和实时任务状态，为后续人工放行集成提供可验证基础。
status: draft
sources:
  - resource: docs/superpowers/plans/2026-10-17-first-unsigned-app-release.md
  - resource: docs/superpowers/plans/2026-10-17-m1-app-workflows.md
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: docs/product-principles.md
  - resource: AGENTS.md
  - resource: ../note.md/package.json
  - resource: ../note.md/src-tauri/Cargo.toml
  - resource: ../note.md/src-tauri/tauri.conf.json
  - resource: ../note.md/src/styles/ui-foundation.css
---

# M1 App 基础可运行子项目 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从实际 macOS `.app` 打开 Git 项目，经随包 CLI 连接唯一 daemon，浏览权威 main 画布、任务、运行和实时事件，关闭 App 后 daemon 继续运行。

**Architecture:** Tauri 原生层只负责项目选择、固定 CLI 调用、已验证 loopback HTTP 代理与本地偏好；业务状态来自现有 daemon。Svelte 通过类型化 invoke 适配器读取，WebView 原生 WebSocket 订阅固定 `/app/events`，事件触发派生只读投影刷新；维护 bootstrap 与展示不替 Agent 续租或消费消息。画布保存 main 坐标，快层只叠加状态；本基础批不增加人工批准入口。

**Tech Stack:** pnpm 11.7.0、Tauri 2、Rust 2021、Svelte 5、TypeScript 5、Vite 6、Svelte Flow 1.6.6、vitest 4、@testing-library/svelte 5.4.2、happy-dom 20.9、svelte-check 4；serde/serde_json、reqwest、tokio 沿已有版本线。

---

## 开工边界与证据

- 本计划是总表 P6 的基础阶段，可在 M0 整改推进时搭壳；最终协议接入、打包验收须等 P1 对应接口通过回归。
- 用户已经授权持续开发至首个未签名 App。执行者先将本计划与当前工作区核对并记录在 `tasks/todo.md`，不重复索取笼统授权。
- 这里只计划，不实现、不安装依赖、不提交。执行时逐任务红绿验证，精确提交已验证文件并登记提交号。
- 基础完成不等于 M1 完成；P2–P5 的证据、提问、草稿批准、基线、恢复确认、事后检测另有实施和集成验收。
- 当前 CLI 为 `crates/towermd/src/main.rs`，daemon 为 `crates/towermd/src/daemon.rs`；开工先重新读最新实现，不能覆盖正在进行的 M0 改动。
- 当前 `init` 输出人类文本，`status`/`daemon status` 输出 JSON；后两者在任务上下文有活动续租语义，不能用作 App bootstrap。纯展示维护入口及派生读适配按 [P6 人工闭环计划 Tasks 2–3](2026-10-17-m1-app-workflows.md) 先交付并接收真实 SHA，尚未实现时保持阻塞；不凭空调用 `--json`、`canvas` 子命令或 `daemon start`。
- `init` 是 FR-98 明定的离线例外：仅随包 CLI 在选定工作树生成未提交文件。App 不自动 commit、checkout、reset，也不自动批准 B0。
- 现 Note.md 无根 Cargo workspace；`src-tauri` 是独立 crate。其 CLI/GUI 共用主程序，而非外置 sidecar；只复用工具线与配置经验，不复制其大型宿主。
- [Tauri sidecar 文档](https://v2.tauri.app/develop/sidecar/)规定 `externalBin` 的源文件带目标 triple 后缀；按该机制随包交付独立 `towermd`，运行定位由原生层管理。
- [Tauri invoke 文档](https://v2.tauri.app/develop/calling-rust/)作为原生桥依据；[配置参考](https://v2.tauri.app/reference/config/)用于验证 CSP、窗口、bundle 配置。仅使用官方 API，不复制 Note 的 updater、签名或公证配置。

## 文件职责与稳定边界

| 文件 | 单一责任 |
| --- | --- |
| `package.json`、`pnpm-lock.yaml` | 最小前端依赖与固定工具版本 |
| `scripts/project-pnpm.mjs`、`scripts/project-pnpm.test.mjs` | 项目范围 pnpm 启动；不更改全局工具 |
| `index.html`、`vite.config.ts`、`vitest.config.ts`、`svelte.config.js`、`tsconfig.json` | 单入口开发、构建、类型检查与 DOM 测试 |
| `src/main.ts`、`src/App.svelte` | Svelte 挂载、应用组合与销毁 |
| `src/lib/daemon/{types,wire,client}.ts` | 应用契约、现 daemon JSON 解码、invoke 调用 |
| `src/lib/daemon/{session,events}.ts` | 项目会话、重连与事件去重；不写业务真值 |
| `src/lib/canvas/{projection,attention}.ts` | 坐标投影、任务聚合与最多三项关注排序 |
| `src/components/{ProjectPicker,ConnectionBanner,AttentionList}.svelte` | 项目引导、真实连接状态与关注清单 |
| `src/components/{CanvasView,ModuleNode,ModulePanel,TaskPanel,ObjectDrawer}.svelte` | Z0–Z2、对象原文与按 ID 下钻 |
| `src/styles/app.css` | 共享变量、卡片、菜单、图标、焦点、错误和响应布局唯一来源 |
| `src-tauri/{Cargo.toml,Cargo.lock,build.rs,tauri.conf.json}` | 独立桌面 crate、可复现锁与构建配置 |
| `src-tauri/src/{main,lib,commands}.rs` | 桌面入口、状态注册、有限 invoke 命令 |
| `src-tauri/src/{project,cli,daemon_client,preferences}.rs` | 原生选择令牌、CLI 定位执行、loopback 代理、本地偏好 |
| `src-tauri/capabilities/main.json` | 仅主窗口必要能力；不授予前端通用 shell/fs/http |
| `scripts/{stage-cli,build-app}.mjs` | 从核心构建 CLI、按架构准备 sidecar、未签名 App 构建 |
| `src-tauri/tests/{cli_adapter,daemon_adapter,app_roundtrip}.rs` | 固定执行边界、真实 daemon 协议与 App 后端集成 |
| `docs/verification/2026-10-17-app-foundation.md` | 实机步骤、架构、版本、日志、截图路径和验收结果 |

测试文件放在对应 TS 文件旁；组件 DOM 测试放在 `src/components/*.test.ts`。`src-tauri/binaries/`、`dist/`、两处 target、node_modules 与生成 schema 忽略；锁文件入库。根 `Cargo.toml` 仅增加 `exclude = ["src-tauri"]`，桌面 manifest 自带 `[workspace]`，核心 `cargo test --workspace` 不编译 Tauri。

## 可编译的应用接口

先定义下面契约；wire 解码允许未知附加键，缺少必需身份/水位则失败，不生成演示默认状态。原始 canvas node/edge/对象文本保留供诊断。

```ts
export type ProjectToken = string;
export type Phase = 'unselected' | 'needs_init' | 'connecting' | 'ready'
  | 'authority_unavailable' | 'recovery_required' | 'disconnected' | 'error';
export interface Endpoint { address: '127.0.0.1'; port: number;
  pid: number; instance: string; host: string }
export interface Task { id: string; title: string; module: string; state: string;
  depends_on: string[]; decisions: string[]; acceptance: string[] }
export interface Run { id: string; task: string; state: string; cost_usd: number | null }
export interface Status { project_id: string; seq: number; tasks: Task[]; runs: Run[] }
export interface ObjectView { id: string; path: string; type: string; text: string }
export interface CanvasEntry { index: number; status: string; entry: unknown }
export interface CanvasView { project_id: string; revision: string; status: string;
  nodes: CanvasEntry[]; edges: CanvasEntry[]; objects: Record<string, unknown>;
  unplaced: string[]; diagnostics: unknown[]; canvas_text: string | null }
export interface LiveEvent { epoch: number | null; seq: number; eid: string;
  type: string; data: unknown; obj: string; t: string; stale: boolean }
export interface Bootstrap { endpoint: Endpoint; status: Status; canvas: CanvasView;
  through_seq: number; floor_seq: number; events_url: string }
export interface DaemonClient {
  chooseProject(): Promise<{ token: ProjectToken; displayPath: string } | null>;
  initialize(token: ProjectToken, docsDir: string): Promise<{ output: string }>;
  bootstrap(token: ProjectToken): Promise<Bootstrap>;
  object(token: ProjectToken, id: string): Promise<ObjectView>;
}
```

原生命令固定为 `choose_project`、`initialize_project`、`load_project`、`load_object`、`save_view_preference`。前端传 `token`，不传任意 cwd、URL、HTTP method、命令或 argv；原生 `ProjectRegistry` 将令牌绑定 canonical worktree。错误返回 `{ code, message, detail }`，`code` 对应上面的阶段及 `tool_missing`、`protocol_error`、`object_missing`。

待交付的 App 专用 wire：固定 CLI `daemon ensure` 在 native app-data 非 Git cwd 复用 daemon 唯一 `connect(true)`/single-flight/FD/spawn，返回 endpoint 后以 `/health` 核对 address、port、pid、instance；不带 project/run、不进入项目 CLI prelude。native 随后只读 `GET /app/projection?directory=…` 与 `GET /app/objects/{id}?directory=…`，带 `x-towermd-instance`；projection 的 status.seq 等于 through_seq，水位/画布来自同一已同步 Session 与权威 Catalog，不能进 execute(Status)、finish_read、renew_binding、inbox 或 ack。正常后台 tick 继续 sync/expire，CLI 任务活动规则保持；不加通用 noRenew/noGate 旗标，不在 Tauri 复制 daemon 启动协议。客户端关闭重定向和代理，仅允许 `127.0.0.1` 与非零端口。HTTP 请求只由 native 执行，避免 WebView 跨 origin CORS；不修改 daemon 为 `Access-Control-Allow-Origin: *`。

WS 使用浏览器原生 `WebSocket`：`ws://127.0.0.1:<port>/app/events?directory=<编码路径>&instance=<编码实例>&after=<连续seq>`。`load_project` 附加原生生成的 `events_url`，前端不得自行拼任意端点；路径来自选定项目，不来自不可信对象文本。CSP 只为此流允许 `ws://127.0.0.1:*`，生产 HTTP fetch 不开放 daemon origin；开发 Vite HMR 权限单独配置。

当前业务 status 无独立恢复身份/完整历史页，不能臆造字段。上述 App projection/event adapter 是待实现前置：`/app/events` 只读 Session 已同步 tail 与 floor/cursor，不调用 HubProtocol subscribe；after 低于 floor 返回 SnapshotRequired，客户端重载 projection 并显示历史窗口提示，不补造事件。从 bootstrap.through_seq 订阅；不同模块 Event.epoch 正常交错不重置窗口。M0 结构化 recovery 沿独立纯读路径，missing/empty/corrupt 不新建 hub、不恢复旧 epoch；真实新增 wire 集中改 `wire.ts`/`daemon_client.rs` 和契约测试。

原生错误类型完整定义放 `src-tauri/src/commands.rs`，由其他模块引用，不另建业务错误框架：

```rust
#[derive(Debug, serde::Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}
impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), detail: None }
    }
}
```

对应 TypeScript `AppError` 为 `{code:string;message:string;detail:string|null}`；不把 Rust panic 或字符串匹配当作稳定的 recovery 协议。wire 导出 `decodeEndpoint(value:unknown):Endpoint`、`decodeStatus(value:unknown):Status`、`decodeCanvas(value:unknown):CanvasView`、`decodeEvent(value:unknown):LiveEvent`，畸形输入抛含字段路径的 Error，由 client 统一转 protocol_error。

## Task 1：固定工具与可运行最小桌面壳

**Files:** 创建上述根前端配置、`scripts/project-pnpm.*`、`src/main.ts`、`src/App.svelte`、`src/styles/app.css`、桌面 manifest/入口/build.rs/配置/capability；修改 `.gitignore`、根 `Cargo.toml`。

- [ ] 写 `scripts/project-pnpm.test.mjs` 红测试：仓库 `packageManager` 是 Note 的 pnpm 11.7.0 完整标识；启动器不调用全局 pnpm/corepack、不写 shell rc 或全局 bin；`--version` 输出 11.7.0。
- [ ] 运行 `node --test scripts/project-pnpm.test.mjs`，预期缺启动器/manifest 而 FAIL；记录具体失败，不以下载失败作为红证据。
- [ ] 实现启动器：优先项目 `.tools/pnpm/node_modules/pnpm/bin/pnpm.cjs`，缺失时执行 `npm install --prefix .tools/pnpm --no-save --package-lock=false pnpm@11.7.0`；`.tools/` 忽略。用 `process.execPath` 启动，转发 argv、退出码和信号；各脚本把 `.tools/pnpm/node_modules/.bin` 加入本次子进程 PATH。测试覆盖首次安装后直接调用同一入口，不依赖全局 pnpm。
- [ ] 写组件红测试并运行 `node scripts/project-pnpm.mjs test -- src/App.test.ts`：`render(App)` 后可找到 `getByRole('button', {name:'打开项目'})`，没有批准/暂停按钮，也没有示例模块。
- [ ] 实现 `mount(App, {target})`、主窗口标题 `Tower.md`、960×700/min 700×480；`tauri::Builder::default().run(tauri::generate_context!())`，只启用必要 Tauri 特性与 Rust dialog 插件 2（沿 Note 现有类别）。
- [ ] 配置 `dev/build/check/test/tauri` 脚本、Vite `127.0.0.1:1420`/strictPort，vitest browser condition + happy-dom；`beforeDevCommand`/`beforeBuildCommand` 调用项目启动器。
- [ ] 运行前述红测试、`node scripts/project-pnpm.mjs check`、`node scripts/project-pnpm.mjs build`、`~/.cargo/bin/cargo test --workspace`、`~/.cargo/bin/cargo check --manifest-path src-tauri/Cargo.toml`；预期全 PASS。
- [ ] 经 `node scripts/project-pnpm.mjs tauri dev` 真正打开 WebView，记录实际窗口和控制台；精确提交本任务文件，提交信息 `feat(app): add runnable Tauri foundation`。

桌面入口保持完整且极小；后续任务只在 `lib.rs` 增加命令与 state 注册，不混入 daemon 业务：

```rust
// src-tauri/src/main.rs
fn main() { towermd_app::run(); }
// src-tauri/src/lib.rs
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("Tower.md 桌面运行失败");
}
```

manifest package/lib 名为 `towermd-app`/`towermd_app`，主二进制 `towermd-app`，初始版本 `0.0.0` 跟随根核心；`tauri.conf.json` 明确 `mainBinaryName`。Svelte 首个测试使用完整最小断言：

```ts
import { render, screen } from '@testing-library/svelte';
import { expect, test } from 'vitest';
import App from './App.svelte';
test('启动时提供项目入口且没有假批准动作', () => {
  render(App);
  expect(screen.getByRole('button', { name: '打开项目' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: '批准' })).toBeNull();
});
```

## Task 2：随包 CLI 定位与受限执行

**Files:** 创建 `src-tauri/src/cli.rs`、`scripts/stage-cli.mjs`、`scripts/stage-cli.test.ts`、`src-tauri/tests/cli_adapter.rs`；修改 desktop manifest/配置/module 声明。

- [ ] 写 Rust 红测试：`resolve_cli("/Applications/Tower.md.app/Contents/MacOS/towermd-app", false)` 返回同目录 `towermd`；任意开发 cwd 不改变结果；bundle 缺 CLI 时必须报错，不退回 PATH 中同名可执行文件。
- [ ] 先建立最小可编译的解析适配，再运行 `~/.cargo/bin/cargo test --manifest-path src-tauri/Cargo.toml --test cli_adapter`，红测须证明错误的bundle路径、cwd依赖或缺CLI时误回退PATH；未定义函数/import导致的编译失败不算行为红。
- [ ] 定义 `resolve_cli(exe: &Path, development: bool) -> Result<PathBuf, AppError>`；生产仅取 `current_exe().parent()/towermd`，开发取编译时 manifest 下 `binaries/towermd-<host triple>`，校验普通可执行文件；不依赖源码当前目录。
- [ ] 定义 `CliAction::Ensure | Init { docs_dir: String }`，只允许固定 argv；Ensure 只能从 native 生成的非 Git app-data cwd 执行，Init 只能从 canonical ProjectToken 解析的所选工作树执行，WebView 不传 cwd。`std::process::Command` stdin null、分别捕获 stdout/stderr/exit code，原生 blocking 工作放 `spawn_blocking`；Ensure 实现复用人工闭环 Task2 的唯一 daemon 维护入口，无 status/prelude/Run。
- [ ] PATH 继承 App 启动环境并显式传给子进程；Finder 缺少目标仓库验证工具时显示缺少工具、当前查找 PATH 和补充环境的操作说明，不修改用户 shell rc/harness 配置。生产不引用开发机器的 `~/.cargo/bin`，不将 Node/Rust/pnpm 设为 App 运行条件；已运行 daemon 的环境不能假称已随新 App PATH 更新。
- [ ] 写 argv 测试：中文/空格/引号/docsDir 含 `$()` 仅是单个参数；无 `sh -c`；无任意命令入口。`Init` stdout 保留原文，不按 JSON 解析。
- [ ] 编写 stage 脚本接受显式 `--target`，调用 `~/.cargo/bin/cargo build -p towermd --release --target <triple>`，复制至 `src-tauri/binaries/towermd-<triple>` 并保持执行权限；Tauri `externalBin` 为 `["binaries/towermd"]`。
- [ ] 运行 Rust 测试与 `node scripts/project-pnpm.mjs test -- scripts/stage-cli.test.ts`；测试 target 路径、缺产物非零退出、双架构不误取 host CLI，预期 PASS；精确提交 `feat(app): bundle and locate the Tower CLI`。

## Task 3：项目选择与 FR-98 初始化引导

**Files:** 创建 `src-tauri/src/project.rs`、`src-tauri/src/commands.rs`、`src/components/ProjectPicker.svelte` 与对应测试；连接 `lib.rs`、App。

- [ ] 写红测试：取消 folder dialog 不调用 CLI；选择非 Git 目录返回明确诊断；选择子目录得到 canonical worktree；选中项目与输入 docsDir 才能初始化；既有初始化不再次询问目录。
- [ ] 运行 `node scripts/project-pnpm.mjs test -- src/components/ProjectPicker.test.ts` 与 Rust `project` 单测，预期组件/注册表缺失 FAIL。
- [ ] 实现 native folder dialog 返回令牌；用固定 `git -C <path> rev-parse --show-toplevel` 校验，保留显示路径。注册表只存内存；未选定 token、过期 token 拒绝；切项目撤销旧请求订阅。
- [ ] 新配置引导显示相对 docsDir 输入（沿现 DEFAULT_DOCS_DIR 建议 `docs/tower`）、受保护子目录说明和“初始化项目”；用户显式点击才执行 `Init`。已有配置及本会话已确认的目录沿用，不问第二遍；路径检测只判断初始化标记存在，不用工作树对象代替 main。
- [ ] 展示 init 的 warning/hint/stdout 和“文件未提交，请检查后自行提交到 main”；不自动提交、不自动切分支、不把生成文件视作已批准基线。
- [ ] 红测试断言初始化前后 HEAD/index 不变、既有 Markdown 原文不变、重复 init 项目 ID 不变；运行现 `~/.cargo/bin/cargo test -p towermd --test init_cli` 保持 FR-98 原有回归绿，新的 App 边界测试先红再实现。
- [ ] 运行组件与 native 测试绿；以真实临时 Git 仓库执行 init，比较 `git rev-parse HEAD` 与 `git diff --cached --exit-code`；精确提交 `feat(app): open projects and guide offline initialization`。

## Task 4：唯一 daemon 的 native HTTP 适配

**Files:** 创建 `src-tauri/src/daemon_client.rs`、`src/lib/daemon/{types,wire,client}.ts`、对应测试与 `src-tauri/tests/daemon_adapter.rs`；修改 command 注册。

- [ ] 写红测试：endpoint 非 loopback、port 0、health instance/pid 不同、HTTP redirect 全拒绝；状态失去 authority 不渲染为任务空数组；畸形 JSON 报 protocol_error。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_adapter`（本次 PATH 先显式加入 `$HOME/.cargo/bin`）和 `node scripts/project-pnpm.mjs test -- src/lib/daemon/wire.test.ts`，预期缺适配器 FAIL。
- [ ] `load_project(token)` 先固定 Ensure 从非 Git app-data cwd 得 endpoint 并验证 health；不得调用业务 Status/DaemonStatus、吞非 Git status 错误启动或复制spawn协议。本Task须先接收人工闭环 Tasks2–3 已验证维护入口及纯读adapter，不提前回接旧读路径。`reqwest` 沿已有 0.12、`.no_proxy().redirect(Policy::none())`；health 验证后只代理固定路径。CLI/daemon 兼容失败如实提示版本，不私自停止别的实例。
- [ ] 纯读 `/app/projection` 得到权威不可用或 M0 recovery_required 时保留真实 diagnostics/来源，不把空任务当ready；正常时验证project_id、revision、through_seq/floor_seq及内嵌status/canvas身份一致。409先区分instance失效与内容冲突，必要重新发现一次且仅重试读请求，初始化不自动重放。
- [ ] JSON decoder 检查 mandatory 字段；保留未知状态为文字；`cost_usd:null` 保持 null；对象 ID 用 URL path segment 编码，directory 用 query 编码。`CanvasEntry.entry` 保留任意 JSON，仅在投影分支收窄为对象；正常节点与 null/字符串 opaque 条目混合的测试须能显示正常节点和逐条诊断，不拒整张画布。
- [ ] 实现 `load_object` 的 typed 固定代理，404 显示对象已不存在；不要读取前端传入路径，不允许 URL/protocol/path 注入。
- [ ] `lib.rs` 为 native 集成测试导出 `pub mod cli; pub mod daemon_client; pub mod project; pub mod commands;`；只导出应用适配边界，不把 daemon 的内部写入模块搬进桌面 crate。
- [ ] 启真实 CLI/daemon 的临时 HOME/runtime 夹具，验证 Ensure、`/app/projection`、`/app/objects/{id}`、`/app/events` 与 native 输出一致；持续查看绑定任务worktree不改变lease expiry、不投递/ack，正常后台过期仍工作；隔离测试只停自己启动且身份匹配的 daemon；绿后提交 `feat(app): proxy verified local daemon reads`。

`src/lib/daemon/wire.test.ts` 的首个红测试可直接运行；补测必需身份字段缺失、未知附加键保留，以及 Event 的 nullable epoch/任意 JSON data：

```ts
import { expect, test } from 'vitest';
import { decodeEndpoint, decodeEvent } from './wire';
test('拒绝远端且不伪造事件 epoch', () => {
  expect(() => decodeEndpoint({ address: '0.0.0.0', port: 80,
    pid: 1, instance: 'i', host: 'h' })).toThrow('address');
  const event = decodeEvent({ seq: 1, eid: 'e', epoch: null,
    type: 'task.state', data: [], obj: 'T1', t: '2026-10-17T00:00:00Z',
    stale: false });
  expect(event.epoch).toBeNull();
  expect(event.data).toEqual([]);
});
```

native 请求串行化同一项目的 bootstrap，App projection若status/canvas/project_id或revision不一致重新读取一次，仍不一致则报 protocol_error；不把跨Catalog与Session读取宣称为原子Git/hub快照。事件到达后仅重载纯展示投影，保证最终与当前权威投影一致；不重跑CLI检查点。

## Task 5：实时订阅、断线与切项目恢复

**Files:** 创建 `src/lib/daemon/{session,events}.ts` 及测试、`src/components/ConnectionBanner.svelte`；修改 types/bootstrap、App 生命周期。

- [ ] 写红测试：来自旧 project token/generation 的迟到响应被丢弃；重复 eid 不重复显示；seq 有缺口时不推进连续水位；同模块 1 秒内 8 个事件只触发一次视觉刷新；不同模块正常交错 fence epoch 不重置连接或事件窗口。
- [ ] 运行 `node scripts/project-pnpm.mjs test -- src/lib/daemon/session.test.ts src/lib/daemon/events.test.ts`，预期 FAIL；用 fake clock/WebSocket 做连接时序测试，真实 daemon 留 Task 9。
- [ ] 实现 `createSession(client, socketFactory, clock)` 返回 `open(token)`/`close()`/`subscribe(listener)`；bootstrap 后从 through_seq 订阅固定纯读 `/app/events`；批处理最多 1 秒，刷新状态而非信任 Agent done 文本，本次连接事件按 eid 去重。
- [ ] WS error/close 置 disconnected，标示最后成功时间；退避 1、2、4、8 秒至最多 15 秒，重取 native 验证 endpoint 和 bootstrap；实例改变、已确认的独立 hub 恢复身份改变或游标压缩失效才重载投影并标明事件窗口重置。`Event.epoch` 是该事件的租约 fence，不是连接代际，不用于窗口重置；目前无独立恢复身份字段时不臆造。
- [ ] floor_seq/连续cursor失效时不无限重试旧 after；重新 bootstrap 的 through_seq 作为新订阅点，保留历史不可得提示。监听到相邻事件缺口也走重载，不能按最大 seq 假装完整。
- [ ] 关闭/切项目取消定时器、socket 和监听，generation 检查旧 promise；清理只发生在 App 客户端，不执行 `daemon stop` 或 `/shutdown`。
- [ ] 运行以上绿测试和 `svelte-check`，断开模拟 socket 后断言原状态标为过期且未清为“无任务”；精确提交 `feat(app): reconnect live daemon sessions safely`。

## Task 6：Z0 权威画布与固定坐标

**Files:** 创建 `src/lib/canvas/projection.ts`、`src/components/{CanvasView,ModuleNode}.svelte` 和测试；修改 `src/styles/app.css`。

- [ ] 写红测试：同 revision 先后任务状态变化，节点 `{id,x,y,width,height}` 深相等；新 revision 才替换坐标；无 tower.canvas 显示“main 尚无已批准画布”；unsafe/dangling/mismatch/opaque 不崩溃、不偷偷解析路径。
- [ ] 运行 `node scripts/project-pnpm.mjs test -- src/lib/canvas/projection.test.ts src/components/CanvasView.test.ts`，预期 FAIL。
- [ ] 实现纯 `projectCanvas(canvas:CanvasView,status:Status)`：位置只取 entry.x/y，身份取 canvas node ID/绑定对象 ID；task/runs 按 module 叠加计数；unknown cost 显示“未知”，不求和为 0。
- [ ] SvelteFlow 使用受控 nodes/edges、`nodesDraggable={false}`、`nodesConnectable={false}`；禁删除/编辑/保存坐标；快层 patch 仅更新 node.data，不调用 fitView 或布局；只在首次载入/用户主动点击“适合画布”时调整 viewport。
- [ ] ModuleNode 用全局共享 card/badge 类，显示状态文本与计数；unknown/未放置模块独立列表供下钻，保留 annotations 与原始 canvas 查看入口，不丢未知格式。
- [ ] 在 WebView 验证缩放/平移与快层更新，记录同节点前后坐标；运行绿测试、check/build；精确提交 `feat(app): render authoritative canvas with stable positions`。

## Task 7：模块、任务与对象真实下钻

**Files:** 创建 `src/components/{ModulePanel,TaskPanel,ObjectDrawer}.svelte`、组件测试；扩展 projection 与 App selection。

- [ ] 写红测试：双击 module 显示该模块任务/依赖/运行/decision IDs；点 task 仅显示该任务运行与事件；点 object ID 调用 native `load_object`，404 可关闭返回；null 费用读“未知”。
- [ ] 运行 `node scripts/project-pnpm.mjs test -- src/components/ModulePanel.test.ts src/components/TaskPanel.test.ts src/components/ObjectDrawer.test.ts`，预期 FAIL。
- [ ] 实现 Z1 任务依赖关系与真实状态列表；拓扑有环显示诊断，不虚构关键路径。仅 daemon 提供验收耗时/关键路径字段时呈现，现无数据明确显示“暂无关键路径数据”。
- [ ] 实现 Z2 运行列表、本次连接事件、检查点/证据 ID 与原文 data 展开；服务端已提供完整历史时经适配器接入，否则显示可见时间窗，不能补造历史完成事件。
- [ ] ObjectDrawer 显示原文 `<pre>`，不需要 marked/mermaid；链接证据原文不存在时显示所在主机/缺失原因，不能把任意 URI 送通用 opener。后续 P2 可把证据详情接入同对象入口。
- [ ] 键盘可进入/退出抽屉、Escape 关闭并恢复焦点，空 module、没有运行、未知 type、对象已删除均有具体文本；测试选择保留与跨项目清空。
- [ ] 运行绿测试/check/build，真实 WebView 从 Z0→Z1→Z2→对象原文走通；精确提交 `feat(app): add module task and object drilldowns`。

## Task 8：关注、错误状态与关闭时间

**Files:** 创建 `src/lib/canvas/attention.ts`、`AttentionList.svelte`、`src-tauri/src/preferences.rs` 及对应测试；完善 `ConnectionBanner`/共享 CSS。

- [ ] 写红测试：输入 5 项只显示 3 项并显示“更多”；真实禁止/授权/错误/门禁/提问按 15.7 排序；每项都有发生什么/影响/建议/需人做什么；无中断字段时不捏造“全部健康”。
- [ ] 运行 attention/AttentionList 的 vitest 与 preferences Rust 测试，预期 FAIL；测试夹具可含未来中断 DTO，但生产只在 wire 明确认识该字段时映射。
- [ ] 实现 attention 纯排序，当前协议只有 diagnostics 则展示真实诊断；后续 P3/P5 接入确认过的中断契约；禁止八边形、授权菱形、错误三角形、门禁方形，加颜色和文字标签。
- [ ] 明确 unselected/needs_init/connecting/ready/authority_unavailable/recovery_required/disconnected/error 文案；recovery_required 仅在 daemon 结构化确认时使用，不能靠“没有任务”猜测。基础阶段显示恢复需 App 确认但无假确认按钮，人工闭环Task8接收M0 F/G后才启用。
- [ ] preferences 只存 app data 中已选项目显示路径、viewport、关闭时间（按 project_id 区分）；每次选择仍重新验证路径。业务对象、授权答案、运行状态与人工决策不存 localStorage/偏好。
- [ ] Tauri CloseRequested/ExitRequested 尽力记录最后查看时间，用于 FR-42；意外终止保留上次可知时间并标识；退出不停止 daemon。自定义 command 不接受任意写文件路径。
- [ ] 共享 CSS 增加 dark/light、focus-visible、reduced-motion、窄窗口布局；实机检查中断形状/标签与菜单一致，HMR 未生效重启 Vite 后重验；绿后提交 `feat(app): show bounded attention and honest connection states`。

## Task 9：真实 daemon 与完整客户端回归

**Files:** 创建 `src-tauri/tests/app_roundtrip.rs`、`src/lib/daemon/contracts.test.ts`、`docs/verification/2026-10-17-app-foundation.md`；本任务不更改业务协议语义。

- [ ] 写真实边界回归红测试：临时仓库 init 并由测试显式 commit main，准备已提交 Module/canvas，启动 native load，断言 revision 是 main SHA、任务来自同一已同步hub投影、工作树篡改不改变对象读取。绑定任务worktree持续bootstrap/object/WS读，lease expiry/Prompt delivered不变；正常background tick仍让无Agent活动的旧Run到期、旧epoch不能继续。
- [ ] 测试从现 `crates/towermd/tests/daemon_cli.rs` 复用搭建流程而非复制业务实现；测试进程显式注入隔离 HOME/runtime，构建 CLI 位置由测试参数传，生产无覆盖任意程序入口。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --test app_roundtrip`，先只放断言观察缺联通处 FAIL；修正已有适配层而非绕开 daemon/authority。
- [ ] 以 CLI task add/claim/release 生成真实事件，断言 App 状态刷新在 2 秒内且节点坐标不变；缺证据的 done 文本不改变已合并计数；运行未知费用为 null。
- [ ] stop/restart 仅针对夹具 daemon：重连更换 instance 后回到同项目；多 App固定Ensure/CLI 同时首次访问只一个 pid；Ensure不带project/run/prelude，现有CLI在任务上下文活动仍正常续租/投递；authority unavailable 不读取工作树假画布，invalid/empty canvas 可诊断。
- [ ] M0 结构化 recovery 接口落定后加入真实 recovery_required 夹具并验证拒绝写入；缺失/empty/corrupt hub读取不创建hub、不续租/ack，不凭空恢复旧epoch。本批不调用人工确认端点，GET已登记请求/确认按人工闭环Task8接M0 F/G，再由真人验收。
- [ ] 运行完整 `cargo test --workspace`、独立 desktop cargo test、前端 test/check/build；输出记录到验证文档，不把测试 API 调用写成真人操作；精确提交 `test(app): verify real daemon client boundaries`。

## Task 10：未签名 App 构建与 Finder 实机验收

**Files:** 创建 `scripts/build-app.mjs`、`scripts/build-app.test.ts`；更新验证文档、双语 CHANGELOG 未发布区、`tasks/todo.md` 回顾。

- [ ] 写脚本红测试：必须先 stage 同 target CLI；拒绝未支持 target；Tauri `createUpdaterArtifacts` 为 false；不引用 Note 的发布脚本、私钥、公证或 Developer ID；缺 CLI/前端 dist 立即失败。
- [ ] 运行 `node scripts/project-pnpm.mjs test -- scripts/build-app.test.ts`，预期 FAIL；实现脚本显式传 target、项目 pnpm 与本次 PATH，调用 `tauri build --target <triple> --bundles app`；不修改用户 shell 文件。
- [ ] 运行 `node scripts/build-app.mjs --target aarch64-apple-darwin`，检查 `src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Tower.md.app` 的主程序、`towermd`、静态资源；以实际生成名称校正配置和测试，不留下假路径。
- [ ] `file`/`lipo -info` 检查两个可执行文件架构一致；记录 `codesign -dv --verbose=4` 与 `spctl --assess` 的实际结果；发行者签名/公证未配置，若有运行所需 ad-hoc 标记如实区分。
- [ ] 把 `.app` 复制到临时无源码目录，从 Finder 打开；停止 Vite，用受控最小 PATH 直接运行包内主程序补查；确认不需要 Node/Rust/pnpm，缺 git 显示工具名与查找路径，不改 shell 配置或静默下载工具。
- [ ] 实机走空目录、空 Git repo、新初始化未提交、已初始化 main、权威不可用、真实模块/任务/事件、断线重连；记录 WebView 控制台、截图、关闭前后 daemon pid 和再次打开结果。
- [ ] 关闭全部 App 窗口及退出 App 后，用包内 CLI 查询同 daemon pid，任务仍可变更；重开 App 恢复权威投影。测试只清理临时项目，用户 daemon 不因 App 生命周期停止。
- [ ] 更新英文/中文未发布条目，内容为可感知的项目打开与实时只读浏览；不声称 M1 人工闭环/双架构发布完成。精确提交 `feat(app): build unsigned macOS application foundation`，记录提交号。

## 完成门槛、交接与自审

- [ ] 基础完成证据：实际 `.app` 能从源码外启动；随包 CLI 身份正确；项目选择/init 遵守 FR-98；真实 daemon 读/订阅与断线重连通过；退出不终止 daemon。
- [ ] UI 验收：Z0 固定坐标、Z1 模块任务依赖、Z2 真实可见事件、对象原文、费用未知、最多三关注、形状+色+文字、所有空/错状态实机有证据。
- [ ] 结构验收：核心 workspace 测试不拉 Tauri；HTTP 走 native 固定代理；没有全局配置改动、前端直接写仓库、开放 CORS、通用 shell 或虚假批准按钮。
- [ ] P6 后续集成按 [人工闭环详细计划](2026-10-17-m1-app-workflows.md) 执行；其Tasks2–3是本基础Task4所需的最小维护/纯读前置，人工UI部分等基础与P2–P5真实SHA。集成清单：P2 证据/完整事件与已合并认证；P3 提问回答/人工干预；P4 草稿、B0、批准拒绝；人工闭环Task8接收M0 F/G的真人恢复确认；P5 禁止与门禁失败卡片。每项使用已实现的 daemon App API，人的署名只从 App 产生。
- [ ] P8 后续发布清单：同步版本、Intel 构建与实机证据、DMG、冻结 tag、技能包、校验和、公开上传与匿名回读；本批 host App 不替代这些发布证据。
- [ ] 执行者逐项记录实际测试/提交/未完成项；尚未提交明确写“未提交”，未实机验证不勾选；基础完成不得修改总表为 M1 或首版发布完成。

编写时自审：覆盖 FR-04/42–44/47/49–50/73/83/95/98/111 的本批边界；未扩大 M2/M3；未引新类别库；固定 App 维护/派生读适配作为显式前置，未当作当前已有 API，使用原生 WS；现协议缺历史/epoch/中断时已明确降级与接入门槛。实施时把红测试先跑出真实失败，不能把本文代码片段本身当作验证结果。
