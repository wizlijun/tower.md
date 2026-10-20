# Tower.md

Tower.md 是本地优先的多 Agent 工程驾驶舱：Agent 推进工作，人在关键处放行。Note.md 管知识，Tower.md 管行动。产品目标见[设计原则](docs/product-principles.md)，完整功能范围见[spec](docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md)。

## 当前可用范围

当前源码已重置到基本框架，并在其上提供**最小画布与冷启动验证 App**。它读取仓库中已提交的 JSON Canvas 与 Module 对象，也能从固定提交扫描源码目录与修改历史，运行验证命令并生成只读候选模块图。默认主界面只有画布，详情与操作按需打开。

App 通过随包 CLI 连接本用户唯一的本机守护进程。扫描与验证使用公共 Git 目录中的缓存和独立工作树，不改用户工作树、暂存区或 main。当前 App 尚未接入初始化、画布编辑、基线批准、任务或 Agent 执行。**完整 M1 尚未完成**；旧版日志中的功能及验收不能作为当前源码的能力证明。

## 启动与构建

macOS 开发环境需要 Git、Rust stable、Xcode Command Line Tools、Node.js，以及 `package.json` 固定的 pnpm 11.7.0。在本仓库根目录运行：

```sh
pnpm install
pnpm app:dev
```

`app:dev` 会先编译随包 CLI，再启动 Vite 与桌面窗口。原生目录选择需要桌面 App；单独运行 `pnpm dev` 的浏览器页面不具备本机项目读取能力。

构建可独立打开的 App：

```sh
pnpm app:build
```

默认生成 `src-tauri/target/release/bundle/macos/Tower.md.app`。若设置了 `CARGO_TARGET_DIR` 或 Cargo 的目标输出目录，产物位于对应目录的 `release/bundle/macos/`。此构建不要求发行者签名或公证；macOS 工具链可能生成临时 ad-hoc 签名。

## 如何查看项目

1. 打开 App，点击“打开项目”，或按 `Cmd+O`（其他平台为 `Ctrl+O`），选择 Git 仓库。取消选择会保留当前项目。
2. 已配置且提交了 `tower.canvas` 的项目会显示节点与关系；拖动空白处平移，滚轮或触控板捏合缩放。
3. 点击节点查看原文，按 `Esc` 关闭详情。空白处右键打开菜单，可刷新、适应画布、查看项目说明或切换项目；键盘也可用 `Shift+F10` 打开菜单。
4. “项目说明”显示读取来源、完整提交号、文档目录和读取诊断。已提交但未出现在画布上的 Module，可从“未摆放模块”查看。
5. 提交发生变化后，使用右键“刷新”或 `Cmd/Ctrl+R` 重新读取。

### 从源码生成候选画布

已配置的项目可在空画布或右键菜单点击“扫描并验证”。目录扫描完成后先显示候选模块图，验证完成后更新卡片状态，保持节点位置和视口。点击卡片查看实际结果与 Module 原文，右键“扫描说明”查看覆盖率、固定提交、历史共同修改关系、命令退出码和日志摘要。关系线表示共同修改，不代表已确认的接口。

这是 spec 的 S1–S5 冷启动切片。模块划分仍是目录启发式，需要后续 Agent 综合和人工审阅；S6 提交和 S8 批准尚未接通，候选不会成为 B0。卡片分别显示通过、失败、无测试、超时、取消或未运行；“无测试”表示未发现可执行的验证命令，并附补测建议，不是验证通过。

右键“返回当前画布”可回到 main。“刷新”也会返回 main；原候选仍固定于旧提交，只有再次扫描才会生成新候选。刷新、切换项目或关闭窗口不取消后台任务；需要停止时选择“取消本次验证”，并等待清理结果。刷新后可从菜单恢复进度跟踪。

验证在 `<git-common-dir>/towermd/verify/<scan-id>/worktree` 的分离工作树运行，结束后删除该工作树。执行继承启动 daemon 时的环境，允许网络，不自动安装依赖；分离工作树不是安全沙箱，命令使用当前用户权限。通过 CLI 启动 daemon 时可显式传入所需 PATH、工具链和凭据；App 沿用已经启动的服务环境。

同一 daemon 同时只运行一批验证，同一命令由关联模块共享、只执行一次。默认每条命令最多 2 分钟，整批最多 10 分钟；超时或取消回收整个进程组。报告绑定 commit/tree，包含 `process:towermd-verifier` 署名。命令改变已跟踪源码、HEAD 或 index，以及无法确认清理完成时，均不能报告通过。

报告与 Module 文件位于 `<git-common-dir>/towermd/drafts/<scan-id>/`；日志位于 `towermd/verify/<scan-id>/logs/`，相对公共 Git 目录。每条日志最多保留 1 MiB，界面最多显示开头 4 KiB，并明确标识存储截断；报告不枚举环境变量，命令输出会进入本机日志。重启可读取完成报告，日志缺失或摘要改变时拒绝沿用。删除缓存后须重新扫描并实际执行，不能恢复原来的执行证据。daemon 意外退出时，独立 worker 通过管道关闭请求取消并清理；未完成的扫描重启后显示中断，不自动重跑或变为通过。

退出服务可运行 `towermd daemon stop`，它会先取消活动验证。升级后若发现旧版服务，会明确提示先停止旧服务再重试，不自动启动第二个 daemon。

### 读取哪份内容

- 没有 `origin`：读取本地 `refs/heads/main`。
- 存在 `origin`：读取已 fetch 的 `refs/remotes/origin/main`。请自行完成 `git fetch origin`；App 不自动访问远端。
- 配置、对象、画布坐标与文件原文均来自同一固定提交。其他分支和未提交修改不会进入画布。
- `tower.canvas` 位于 `.towermd/config.yaml` 中的 `docs_dir` 下；文件节点路径以仓库根为基准。

没有配置时，App 显示初始化说明；已有配置但没有画布时，可主动扫描并验证生成候选。引用缺失或画布条目非法时，可在“项目说明”查看诊断。

## CLI 基础框架

当前 CLI 提供 `init`、`status`、项目读取、候选扫描与守护进程管理。查看准确参数：

```sh
cargo run -p towermd -- --help
cargo run -p towermd -- init --help
```

只有在准备初始化一个项目时，才运行下面的命令，并将占位路径替换为实际仓库：

```sh
cargo run -p towermd -- init /path/to/project --docs-dir docs/tower
```

`init` 只生成项目 ID、配置、Agent 说明与文档目录骨架，不生成模块图、运行任务或提交 Git。已有 Tower.md 项目应沿用原配置和文档，不要用示例覆盖已有文件。检查生成内容后，自行决定如何提交；首次导入已有内容的提交不等于 B0 批准。

仅执行 `init` 不会产生模块图。将配置提交到权威 main 后，可在 App 扫描，或在项目目录运行随包 `towermd`：

```sh
towermd project view
towermd baseline scan
towermd baseline scan --wait <返回的扫描ID>
towermd baseline show <扫描ID>
towermd baseline cancel <扫描ID>
```

扫描会实际执行候选命令，立即返回 ID；等待每次最多 90 秒，仍在运行时可继续等同一个 ID。`ready` 表示报告已经生成，不代表所有测试通过，须查看模块与命令的实际状态。取消也需要等原 ID 进入终态，不能把请求已接受当作进程已结束。CLI 没有批准命令。同一 clone 的多个工作树共用项目服务；同一 project_id 的独立 clone 不可同时打开。

## 开发验证

```sh
pnpm check
pnpm test
pnpm build
cargo test --workspace
node scripts/prepare-sidecar.mjs
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

桌面 crate 与核心 workspace 使用各自锁文件。以上是验证入口，不代表当前工作区已经通过这些检查；具体结果须与实际源码及构建产物对应。界面改动还需在桌面 App 中验证。

双语变更记录见 [CHANGELOG.md](CHANGELOG.md) 与 [CHANGELOG.zh-CN.md](CHANGELOG.zh-CN.md)。版本节保留历史，未发布区描述当前骨架上的变更。
