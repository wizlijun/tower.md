---
type: Verification
title: 骨架上的最小只读画布 App 验收
---

# 验收范围

起点是用户保留的 `938b8ed`，实现提交为 `c9c985143f5d0332f10b0c25b22ccee08b48837c`。本记录仅证明当前 macOS Apple Silicon 最小只读 App 可运行，不代表完整 M0/M1 或 spec 已完成。没有恢复旧业务模块或沿用旧包验收结果。

App 读取已提交配置、Module 文档与 JSON Canvas。默认只有画布；原文、来源、未摆放模块和菜单按需出现。编辑、批准、Agent 任务、实时状态、快照写入及恢复尚未接入。App 不启动 daemon，不写项目。Intel、Windows、Linux 本轮未验证。

# 自动化与构建

- `cargo test --workspace --offline -j 2`：177 项通过，包含 5 项新增 project_view 集成测试。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline -j 2`：3 项通过，覆盖取消选择、旧 token 与迟到选择。
- `pnpm check`：0 error、0 warning。
- `pnpm test`：16 项通过，覆盖异步选择/刷新、错误恢复、原文安全展示、任意关系、四边连线及视角保留。
- `pnpm app:build`：Vite 生产构建及 Tauri macOS `.app` 打包成功。构建使用两路 Cargo jobs、nice 10，未并发运行多套构建。
- 构建仅有 Tauri 的 Windows STATIC_VCRUNTIME 弃用提示，不影响本轮 macOS 产物。

# 实机证据

使用 Computer Use 操作真实原生窗口和目录选择框；未要求用户代点。验收项目是独立 Git 仓库，不是产品的已批准基线。

初始候选验证：

1. 原生目录选择打开、取消，返回空画布；已有项目取消切换保留原图。
2. 真实已提交的 4 个节点和 3 条关系显示正常，包含环和孤立节点，无常驻侧栏或工具栏。
3. 点击 Module 查看完整原文、文件路径和来源 SHA；Escape 返回画布。
4. 项目说明显示来源与未摆放 Module，点击可查看原文，不自动生成坐标。
5. 拖拽平移后刷新，视角保留。未提交修改标题、坐标和文本后刷新，仍显示原 main 提交 `1786f638dcefd31a029a6da17d1d2b6905fcdc5a`。
6. 将标题变更真正提交到 main，刷新显示“画布视图 · 已刷新”，原文来源更新到 `b2da354dd6e4c30f8e507f3257c84edd8c157e1a`。
7. 提交损坏 JSON 后刷新：保留旧图，显示具体解析错误与重试按钮。修复并提交后点击重试恢复。

最终候选在上述实现上仅调整滚轮直接缩放及只读辅助语义；重新通过类型检查、16 项前端测试及生产构建。最终产物再次实机验证打开项目、节点/关系渲染、滚轮缩放、鼠标右键菜单、适应画布和切换到保留的验收仓库。辅助树不再出现可删除边的误导提示或连接点按钮。

最终 QA main 为 `538108f30b9fdcb58720ebfade6d73f1111256ef`。测试后 QA 工作区干净，App 没有创建 `.git/towermd`。旧候选已退出，仅保留最终候选一个进程。

# 可运行产物

- App：`/Users/bruce/git/tower.md/.release/minimal-canvas-20261020/Tower.md.app`
- 保留的验收仓库：`/Users/bruce/git/tower.md/.release/minimal-canvas-20261020/qa-project`
- 产物清单：同目录 `manifest.json`，记录实现提交、架构、前端资源哈希、可执行文件哈希和 QA 提交。
- 最终可执行文件 SHA-256：`4f3e94165acf433bab7e2afbab199880e47af423cf4118d040f07331f6a3e64c`。
- 没有发行者签名或公证；Mach-O 保留工具链生成的 ad-hoc 签名。

这是本机可测试候选，未创建正式发布 tag，也未发布远端 release。

重新打开 App 后点击“打开项目”选择上述 QA 仓库；系统目录选择框可用 `Cmd+Shift+G` 输入路径。滚轮缩放，拖动空白平移，点击卡片看原文，右键刷新或适应画布。普通仓库需要在权威 main 中先有配置和画布；CLI init 只生成骨架。

# 范围与仓库清理核对

提交前仅有根 worktree 和 main，无 stash、无其他本地分支。用户原有未跟踪 `.towermd/`、`AGENTS.md`、`CLAUDE.md`、`docs/superpowers/`、`docs/tower/` 未修改、未加入提交。历史版本日志保留，未发布日志和 README 已改为当前实际能力。

后续主干仍是 daemon 唯一业务写入与恢复、Agent 候选和 App 批准、真实任务的领取/问答/验证/合并闭环。当前只读切片不提供任何绕过这些闸门的写入入口。
