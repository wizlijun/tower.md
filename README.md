# Tower.md

Tower.md 是本地优先的多 Agent 工程驾驶舱：Agent 推进工作，人在关键处放行。Note.md 管知识，Tower.md 管行动。产品目标见[设计原则](docs/product-principles.md)，完整功能范围见[spec](docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md)。

## 当前可用范围

当前源码已重置到基本框架，并在其上提供**最小只读画布 App**。它读取仓库中已提交的 JSON Canvas 与 Module 对象，显示节点、关系和文档原文。默认主界面只有画布，详情与操作按需打开。

当前 App 尚未接入初始化、画布编辑、基线批准、任务、Agent 执行或实时状态。它不启动守护进程，也不写仓库。**完整 M1 尚未完成**；旧版日志中的功能及验收不能作为当前源码的能力证明。

## 启动与构建

macOS 开发环境需要 Git、Rust stable、Xcode Command Line Tools、Node.js，以及 `package.json` 固定的 pnpm 11.7.0。在本仓库根目录运行：

```sh
pnpm install
pnpm app:dev
```

`app:dev` 会启动 Vite 与桌面窗口。原生目录选择需要桌面 App；单独运行 `pnpm dev` 的浏览器页面不具备本机项目读取能力。

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

### 读取哪份内容

- 没有 `origin`：读取本地 `refs/heads/main`。
- 存在 `origin`：读取已 fetch 的 `refs/remotes/origin/main`。请自行完成 `git fetch origin`；App 不自动访问远端。
- 配置、对象、画布坐标与文件原文均来自同一固定提交。其他分支和未提交修改不会进入画布。
- `tower.canvas` 位于 `.towermd/config.yaml` 中的 `docs_dir` 下；文件节点路径以仓库根为基准。

没有配置或画布时，App 显示明确说明，不自动推断模块、不生成目录图或演示数据。引用缺失或画布条目非法时，可在“项目说明”查看诊断。

## CLI 基础框架

当前 CLI 提供 `init`、`status` 与守护进程基础管理。查看准确参数：

```sh
cargo run -p towermd -- --help
cargo run -p towermd -- init --help
```

只有在准备初始化一个项目时，才运行下面的命令，并将占位路径替换为实际仓库：

```sh
cargo run -p towermd -- init /path/to/project --docs-dir docs/tower
```

`init` 只生成项目 ID、配置、Agent 说明与文档目录骨架，不生成模块图、运行任务或提交 Git。已有 Tower.md 项目应沿用原配置和文档，不要用示例覆盖已有文件。检查生成内容后，自行决定如何提交；首次导入已有内容的提交不等于 B0 批准。

画布浏览需要权威 main 中已有相应配置、对象与 `tower.canvas`。仅执行 `init` 不会产生一张可浏览的模块图；自动冷启动与 App 批准属于后续实现范围。

## 开发验证

```sh
pnpm check
pnpm test
pnpm build
cargo test --workspace
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

桌面 crate 与核心 workspace 使用各自锁文件。以上是验证入口，不代表当前工作区已经通过这些检查；具体结果须与实际源码及构建产物对应。界面改动还需在桌面 App 中验证。

双语变更记录见 [CHANGELOG.md](CHANGELOG.md) 与 [CHANGELOG.zh-CN.md](CHANGELOG.zh-CN.md)。版本节保留历史，未发布区描述当前骨架上的变更。
